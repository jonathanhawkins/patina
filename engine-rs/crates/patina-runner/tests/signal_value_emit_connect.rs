//! pat-0q2kx.67 acceptance: `Signal` is a first-class value and `Callable`
//! carries its bound-argument surface.
//!
//! `node.signal_name.connect(cb)` / `sig.emit(x)` is THE Godot 4 idiom
//! (`Signal.xml`), replacing the string-based `connect("name", ...)` calls.
//! `Callable.bind(1).get_bound_arguments()` must report `[1]`, and binding is
//! cumulative across nested `bind`s.
//!
//! The `Signal` value itself landed with pat-0q2kx.9; this pins the end-to-end
//! script path and covers the Callable methods that were still missing
//! (`bindv`, `get_object`, bound/unbound argument introspection, `is_custom`,
//! `is_standard`, `hash`, …).

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use gdvariant::Variant;
use patina_runner::bootstrap::{BootConfig, EngineBootstrap};

static BOOT_LOCK: Mutex<()> = Mutex::new(());

fn unique_project_dir(tag: &str) -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "patina_0q2kx67_{}_{}_{}",
        tag,
        std::process::id(),
        n
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// `Main` connects to its child's `scored` signal through the Signal VALUE
/// (`emitter.scored.connect(...)`), then the child emits through the same value.
const MAIN_TSCN: &str = r#"[gd_scene load_steps=3 format=3]

[ext_resource type="Script" path="res://main.gd" id="1_main"]
[ext_resource type="Script" path="res://emitter.gd" id="2_emit"]

[node name="Main" type="Node2D"]
script = ExtResource("1_main")

[node name="Emitter" type="Node2D" parent="."]
script = ExtResource("2_emit")
"#;

const MAIN_GD: &str = "\
extends Node2D
var got_points = -1
var call_count = 0
var bound_count = -1
var first_bound = -1
var is_std = false

func _ready():
    var emitter = $Emitter
    emitter.scored.connect(_on_scored)

    var cb = Callable(self, \"_on_scored\").bind(7)
    var bound = cb.get_bound_arguments()
    bound_count = bound.size()
    first_bound = bound[0]
    is_std = cb.is_standard()

func _on_scored(points):
    got_points = points
    call_count += 1
";

/// Emits on the first PROCESS frame, not from `_ready`.
///
/// ⚠️ A running script is checked out of `SceneTree.scripts` while it executes,
/// so a signal emitted during `Main._ready()` — even indirectly, via a call into
/// this node — cannot be delivered back to `Main`'s own handler. Deferring the
/// emit by one frame is what makes the connection observable.
const EMITTER_GD: &str = "\
extends Node2D
signal scored(points)
var fired = false

func _process(delta):
    if not fired:
        fired = true
        scored.emit(5)
";

fn write_project(dir: &Path) {
    std::fs::write(
        dir.join("project.godot"),
        "[application]\nconfig/name=\"SignalValue\"\nrun/main_scene=\"res://main.tscn\"\n",
    )
    .unwrap();
    std::fs::write(dir.join("main.tscn"), MAIN_TSCN).unwrap();
    std::fs::write(dir.join("main.gd"), MAIN_GD).unwrap();
    std::fs::write(dir.join("emitter.gd"), EMITTER_GD).unwrap();
}

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

/// **The acceptance test.**
#[test]
fn signal_value_emit_connect() {
    let _guard = BOOT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = unique_project_dir("signal");
    write_project(&dir);

    // The scene references Node2D; `EngineBootstrap` leaves ClassDB population
    // to the caller.
    gdobject::class_db::register_2d_classes();

    let config = BootConfig {
        main_scene: Some(dir.join("main.tscn")),
        project_dir: dir.to_path_buf(),
        headless: true,
        ..BootConfig::default()
    };
    let mut boot = EngineBootstrap::new(config);
    boot.run_all().expect("boot succeeds");

    // One process frame: `_ready` made the connection, and the emitter fires on
    // its first `_process` — by which point Main's script is no longer executing
    // and can receive the callback.
    boot.main_loop_mut().expect("main loop").step(1.0 / 60.0);

    let tree = boot.main_loop().expect("main loop built").tree();
    let main_id = tree
        .get_node_by_path("/root/Main")
        .expect("main scene instanced");

    // -- the Signal VALUE delivered the callback ------------------------------
    //
    // This is the whole point: no `connect("scored", ...)` string anywhere —
    // `emitter.scored` resolved to a Signal, `.connect` took a Callable, and
    // `.emit(5)` on the same value delivered the argument.
    assert_eq!(
        script_var(tree, main_id, "got_points"),
        Some(Variant::Int(5)),
        "scored.emit(5) must deliver 5 to the connected callback"
    );
    assert_eq!(
        script_var(tree, main_id, "call_count"),
        Some(Variant::Int(1)),
        "the handler runs exactly once per emit"
    );

    // -- Callable bound-argument surface --------------------------------------
    assert_eq!(
        script_var(tree, main_id, "bound_count"),
        Some(Variant::Int(1)),
        "bind(7).get_bound_arguments() reports one bound argument"
    );
    assert_eq!(
        script_var(tree, main_id, "first_bound"),
        Some(Variant::Int(7)),
        "…and that argument is the 7 that was bound"
    );
    assert_eq!(
        script_var(tree, main_id, "is_std"),
        Some(Variant::Bool(true)),
        "an object+method Callable is 'standard', not 'custom'"
    );
}
