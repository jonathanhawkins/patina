//! pat-nzgdu.70 acceptance: `[autoload]` entries resolve by **global name** in GDScript.
//!
//! pat-nzgdu.3 already put the autoload node under `/root/<Name>` (covered by
//! `autoload_boot_test.rs`). This file covers the half that makes autoloads
//! actually usable the way Godot games use them: a script writing `Global.score`
//! with no `get_node`, resolved through the real `EngineBootstrap` boot path.
//!
//! The negative test is the important one — it pins that only *declared*
//! autoloads bind, so the resolver can't degrade into "any child of /root is a
//! global", which would swallow real `UndefinedVariable` errors.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use gdvariant::Variant;
use patina_runner::bootstrap::{BootConfig, EngineBootstrap};

/// Serializes the tests in this file. The autoload name table is process-global
/// (autoloads are declared once per engine run), so under `cargo test` — one
/// process, many threads — two tests booting at once would clear each other's
/// registrations. `cargo nextest` runs each test in its own process and doesn't
/// need this, but the file has to be correct under both runners.
static BOOT_LOCK: Mutex<()> = Mutex::new(());

/// Creates a fresh, unique temp directory for a project fixture.
fn unique_project_dir(tag: &str) -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "patina_runner_autoload_globals_{}_{}_{}",
        tag,
        std::process::id(),
        n
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Reads a script variable's current value from a node, or `None`.
fn script_var(
    tree: &gdscene::SceneTree,
    node_id: gdscene::node::NodeId,
    var: &str,
) -> Option<Variant> {
    tree.get_script(node_id)?
        .list_properties()
        .into_iter()
        .find(|p| p.name == var)
        .map(|p| p.default_value)
}

/// Writes a project whose `main.tscn` root is a `Main` node carrying `main_gd`,
/// alongside an autoload `Global` (`global.gd`) declaring `var score = 5`.
fn write_project(dir: &Path, main_gd: &str) {
    std::fs::write(
        dir.join("project.godot"),
        "[application]\nconfig/name=\"AutoloadGlobals\"\n\n[autoload]\nGlobal=\"*res://global.gd\"\n",
    )
    .unwrap();
    std::fs::write(dir.join("global.gd"), "extends Node\nvar score = 5\n").unwrap();
    std::fs::write(dir.join("main.gd"), main_gd).unwrap();
    std::fs::write(
        dir.join("main.tscn"),
        "[gd_scene format=3]\n\n\
         [ext_resource type=\"Script\" path=\"res://main.gd\" id=\"1_main\"]\n\n\
         [node name=\"Main\" type=\"Node2D\"]\nscript = ExtResource(\"1_main\")\n",
    )
    .unwrap();
}

/// Boots `dir` through the full runner phase sequence and returns the bootstrap.
fn boot_project(dir: &Path) -> EngineBootstrap {
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

/// The acceptance test: `Global="*res://global.gd"` (with `var score = 5`) boots
/// to a `/root/Global` node, and the **main scene's** script reads `Global.score`
/// as a bare global name — no `get_node` — observing `5`.
#[test]
fn project_autoload_singletons() {
    let _guard = BOOT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    gdscript_interop::autoload_registry::clear_autoloads();

    let dir = unique_project_dir("resolves");
    write_project(
        &dir,
        "extends Node2D\nvar observed = 0\nfunc _ready():\n    observed = Global.score\n",
    );

    let boot = boot_project(&dir);
    assert_eq!(boot.autoloads_instantiated(), 1, "one autoload instantiated");

    let tree = boot.main_loop().expect("main loop built").tree();

    // A node named `Global` exists under the root.
    let gid = tree
        .get_node_by_path("/root/Global")
        .expect("/root/Global exists after boot");
    assert_eq!(tree.get_node(gid).unwrap().name(), "Global");

    // The main-scene script resolved `Global.score` by global name during _ready.
    let mid = tree
        .get_node_by_path("/root/Main")
        .expect("/root/Main exists after boot");
    assert_eq!(
        script_var(tree, mid, "observed"),
        Some(Variant::Int(5)),
        "main-scene script read Global.score == 5 via the autoload global name"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// The autoload name is bound because it was **declared**, not merely because a
/// node of that name sits under `/root`. `Main` is a root child and is *not* an
/// autoload, so referencing it as a bareword stays an `UndefinedVariable` error
/// (which aborts `_ready`, leaving the sentinel untouched).
#[test]
fn undeclared_root_child_does_not_bind_as_a_global() {
    let _guard = BOOT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    gdscript_interop::autoload_registry::clear_autoloads();

    let dir = unique_project_dir("negative");
    write_project(
        &dir,
        "extends Node2D\nvar observed = -1\nfunc _ready():\n    observed = Main.score\n",
    );

    let boot = boot_project(&dir);
    let tree = boot.main_loop().expect("main loop built").tree();

    // `Global` is registered (it was declared) but `Main` never is...
    assert!(gdscript_interop::autoload_registry::is_autoload("Global"));
    assert!(
        !gdscript_interop::autoload_registry::is_autoload("Main"),
        "a plain /root child must not be registered as an autoload global"
    );

    // ...so `Main.score` never resolved and the sentinel is untouched.
    let mid = tree
        .get_node_by_path("/root/Main")
        .expect("/root/Main exists after boot");
    assert_eq!(
        script_var(tree, mid, "observed"),
        Some(Variant::Int(-1)),
        "referencing a non-autoload root child as a bareword must not resolve"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
