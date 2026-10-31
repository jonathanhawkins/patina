//! pat-mgixk.100 section 1 acceptance: `Object._init` runs when a script is
//! instantiated, and `.new(args)` forwards its arguments to it.
//!
//! Godot ground truth: `doc/classes/Object.xml` `_init` — "called when the
//! object's script is instantiated", receiving the `.new()` arguments; and
//! `GDScript::_create_instance` (`modules/gdscript/gdscript.cpp`), which calls
//! the initializer with `p_args`.
//!
//! # What was broken
//!
//! `_init` had **no caller anywhere**. The only `"_init"` in the whole
//! workspace outside ClassDB's virtual-method registrations was
//! `call_super`'s fallback *name* — so a `func _init():` body simply never ran.
//! And the interpreter's `GDScript.new()` arm called
//! `SceneAccess::instantiate_script(path)`, whose signature has no argument
//! list at all, so `load("res://h.gd").new(21)` constructed the instance and
//! discarded the 21.
//!
//! 🔑 The ORDER assertion is what makes this more than "the body ran": `_init`
//! must precede `_ready`. Godot constructs the object (running `_init`) and only
//! then enters the tree. A single shared `order` list records both, so one
//! assertion pins the sequence rather than two independent "did it run" flags.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use gdvariant::Variant;
use patina_runner::bootstrap::{BootConfig, EngineBootstrap};

/// The `res://` path roots are process-global, so serialize these tests to stay
/// correct under `cargo test` (one process, many threads). `cargo nextest` isolates
/// each test in its own process and does not need this.
static BOOT_LOCK: Mutex<()> = Mutex::new(());

/// Installs the project directory as the `res://` root.
///
/// ⚠️ **`EngineBootstrap` does NOT do this.** `grep set_path_roots
/// crates/patina-runner/src/bootstrap.rs` is empty; only `project_boot`'s
/// `boot_project` installs the roots. A `.tscn`'s `ext_resource` still resolves
/// without it, because the scene loader takes those relative to the scene file —
/// which is why `_init` and `_ready` run at all. But a script's
/// `load("res://h.gd")` goes through `gdcore::file_access::resolve_path`, finds
/// no project root, and falls back to the bare relative path `h.gd` against the
/// test process's CWD. So `load` answers null, `.new(21)` answers Nil, reading
/// `.value` off it aborts the remainder of `_ready`, and `helper_value` keeps
/// its declared `-1`.
///
/// 🔑 That is exactly how this test failed (`left: Int(-1)`) while assertions 1
/// and 2 passed — and the ordering assertion passed for a reason that hides it:
/// `order` is an Array, so `append` mutates shared state and never needs the
/// property write-back that `helper_value` does. The same trap is recorded in
/// `change_scene_keeps_autoloads_and_runs_new_scripts.rs`.
fn install_res_root(dir: &Path) {
    gdcore::file_access::set_path_roots(Some(dir.to_path_buf()), None);
}

fn unique_project_dir(tag: &str) -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "patina_mgixk100_{}_{}_{}",
        tag,
        std::process::id(),
        n
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// `main.gd` records `_init` and `_ready` into ONE list, so their order is a
/// single observable. It then builds the helper through `load(..).new(21)` —
/// the spelling the acceptance names — and reads the value its `_init` computed.
const MAIN_GD: &str = "\
extends Node
var tag = \"\"
var order = []
var helper_value = -1

func _init():
    self.tag = \"init\"
    self.order.append(\"init\")

func _ready():
    self.order.append(\"ready\")
    var h = load(\"res://h.gd\").new(21)
    self.helper_value = h.value
";

/// The helper. `_init` takes a parameter, which is the half that was silently
/// dropped: `21 * 2 == 42` can only appear if the argument arrived.
const HELPER_GD: &str = "\
extends RefCounted
var value = 0

func _init(n):
    self.value = n * 2
";

fn write_project(dir: &Path) {
    std::fs::write(
        dir.join("project.godot"),
        "config_version=5\n\n[application]\n\nrun/main_scene=\"res://main.tscn\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("main.tscn"),
        "[gd_scene load_steps=2 format=3]\n\n\
         [ext_resource type=\"Script\" path=\"res://main.gd\" id=\"1_main\"]\n\n\
         [node name=\"Main\" type=\"Node\"]\nscript = ExtResource(\"1_main\")\n",
    )
    .unwrap();
    std::fs::write(dir.join("main.gd"), MAIN_GD).unwrap();
    std::fs::write(dir.join("h.gd"), HELPER_GD).unwrap();
}

fn boot(dir: &Path) -> EngineBootstrap {
    let config = BootConfig {
        main_scene: Some(dir.join("main.tscn")),
        project_dir: dir.to_path_buf(),
        headless: true,
        ..BootConfig::default()
    };
    let mut boot = EngineBootstrap::new(config);
    boot.run_all().expect("boot succeeds");
    boot
}

fn script_var(boot: &EngineBootstrap, var: &str) -> Variant {
    let tree = boot.main_loop().unwrap().tree();
    let id = tree
        .get_node_by_path("/root/Main")
        .expect("the main scene root is in the tree");
    tree.get_script(id)
        .expect("Main has a script")
        .list_properties()
        .into_iter()
        .find(|p| p.name == var)
        .map(|p| p.default_value)
        .unwrap_or(Variant::Nil)
}

/// **The acceptance test.**
#[test]
fn script_init_constructor_runs() {
    let _guard = BOOT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = unique_project_dir("init");
    write_project(&dir);
    install_res_root(&dir);
    let mut boot = boot(&dir);
    boot.main_loop_mut().unwrap().step(1.0 / 60.0);

    // -- (1) `_init` ran on the scene's own script --------------------------
    //
    // `""` is the declared initial value, so this fails as the empty string
    // when `_init` never runs — which is exactly what HEAD does.
    assert_eq!(
        script_var(&boot, "tag"),
        Variant::String("init".into()),
        "`func _init()` must run when the scene's script is instantiated; the \
         empty string means the constructor body never executed"
    );

    // -- (2) ...before `_ready` ---------------------------------------------
    let order = match script_var(&boot, "order") {
        Variant::Array(a) => a.to_vec(),
        other => panic!("`order` should be an Array, got {other:?}"),
    };
    assert_eq!(
        order,
        vec![
            Variant::String("init".into()),
            Variant::String("ready".into())
        ],
        "`_init` must run BEFORE `_ready` — Godot constructs the object and \
         only then enters the tree. A list containing only \"ready\" means the \
         constructor was skipped; the reverse order means it ran too late"
    );

    // -- (3) `.new(21)` forwarded its argument ------------------------------
    //
    // 🔑 42 can only arise from `n * 2` with n == 21. The declared default is
    // 0 and the field starts at -1 here, so both "never constructed" and
    // "constructed with no argument" are distinguishable from success.
    assert_eq!(
        script_var(&boot, "helper_value"),
        Variant::Int(42),
        "`load(\"res://h.gd\").new(21)` must pass 21 to the helper's \
         `_init(n)`; 0 means `_init` ran without the argument and -1 that the \
         instance was never created"
    );
}
