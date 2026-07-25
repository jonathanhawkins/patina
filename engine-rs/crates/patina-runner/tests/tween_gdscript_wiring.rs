//! pat-0q2kx.4 acceptance: a plain GDScript in a `.tscn` can build a tween and
//! actually receive its callbacks.
//!
//! Patina had the whole tween engine (`gdscene::tween`, five `TweenerKind`s,
//! easing, looping, subtweens) and stepped it every frame from
//! `MainLoop::step` — but **nothing was connected to a script**:
//!
//! * `create_tween` did not exist in `gdscript-interop` at all (grep found only
//!   the word "between" in comments), and `SceneTree::create_tween` had *only*
//!   test callers, so no script could make a tween.
//! * `SceneTree::process_tweens` applied property values and never called
//!   `Tween::method_values` / `poll_callbacks` / `take_just_finished`, so
//!   `tween_callback`, `tween_method` and `finished` silently never fired —
//!   even from Rust.
//!
//! These tests construct no tween by hand. The tween is built by the scene's
//! own GDScript and driven by real `MainLoop::step` frames through
//! `EngineBootstrap`. There is no `SceneTree::create_tween` or `add_tween` call
//! in this file — if the script bridge regresses, these tests fail.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use gdvariant::Variant;
use patina_runner::bootstrap::{BootConfig, EngineBootstrap};

/// `boot_project` mutates the process-global `res://` path roots, so serialize
/// these tests to stay correct under `cargo test` (one process, many threads).
/// `cargo nextest` isolates each test in its own process and doesn't need this.
static BOOT_LOCK: Mutex<()> = Mutex::new(());

fn unique_project_dir(tag: &str) -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "patina_0q2kx4_{}_{}_{}",
        tag,
        std::process::id(),
        n
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

const MAIN_TSCN: &str = r#"[gd_scene load_steps=2 format=3]

[ext_resource type="Script" path="res://main.gd" id="1_main"]

[node name="Main" type="Node2D"]
script = ExtResource("1_main")

[node name="Sprite2D" type="Sprite2D" parent="."]
position = Vector2(0, 0)
"#;

/// ⚠️ `Callable(self, "name")` is the Callable form Patina's interpreter
/// builds; a bare `_on_step_done` identifier is not a Callable here, unlike
/// Godot 4. `connect(signal, target, method)` is likewise Patina's supported
/// arity. Both are pre-existing interpreter surface, not something this bead
/// changes — the tween bridge accepts whatever the interpreter produces.
const MAIN_GD: &str = "\
extends Node2D
var callback_count = 0
var finished_count = 0

func _ready():
    var t = create_tween()
    t.tween_property($Sprite2D, \"position:x\", 100.0, 0.5)
    t.tween_callback(Callable(self, \"_on_step_done\"))
    t.connect(\"finished\", self, \"_on_finished\")

func _on_step_done():
    callback_count += 1

func _on_finished():
    finished_count += 1
";

/// Drives `tween_method`, whose interpolated value the engine computed every
/// frame and threw away before this bead.
const METHOD_GD: &str = "\
extends Node2D
var last_value = 0.0
var call_count = 0

func _ready():
    var t = create_tween()
    t.tween_method(Callable(self, \"_on_value\"), 0.0, 10.0, 0.5)

func _on_value(v):
    last_value = v
    call_count += 1
";

fn write_project(dir: &Path, tscn: &str, gd: &str) {
    std::fs::write(
        dir.join("project.godot"),
        "[application]\nconfig/name=\"TweenBoot\"\nrun/main_scene=\"res://main.tscn\"\n",
    )
    .unwrap();
    std::fs::write(dir.join("main.tscn"), tscn).unwrap();
    std::fs::write(dir.join("main.gd"), gd).unwrap();
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

/// Reads a script variable's current value from a node.
fn script_var(tree: &gdscene::SceneTree, node_id: gdscene::node::NodeId, var: &str) -> Variant {
    tree.get_script(node_id)
        .expect("node has a script")
        .list_properties()
        .into_iter()
        .find(|p| p.name == var)
        .map(|p| p.default_value)
        .unwrap_or(Variant::Nil)
}

fn as_int(v: Variant) -> i64 {
    match v {
        Variant::Int(i) => i,
        Variant::Float(f) => f as i64,
        other => panic!("expected a number, got {other:?}"),
    }
}

fn as_float(v: Variant) -> f64 {
    match v {
        Variant::Float(f) => f,
        Variant::Int(i) => i as f64,
        other => panic!("expected a number, got {other:?}"),
    }
}

fn sprite_x(tree: &gdscene::SceneTree) -> f32 {
    let sprite = tree
        .get_node_by_path("/root/Main/Sprite2D")
        .expect("Sprite2D instanced");
    match tree.get_node(sprite).unwrap().get_property("position") {
        Variant::Vector2(v) => v.x,
        other => panic!("Sprite2D.position should be a Vector2, got {other:?}"),
    }
}

/// **The acceptance test.** A script-built tween moves `$Sprite2D`'s
/// `position:x` across `MainLoop::step` frames, its `tween_callback` runs
/// exactly once, and the `finished` signal fires.
#[test]
fn gdscript_create_tween_property_and_callback() {
    let _guard = BOOT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = unique_project_dir("prop_cb");
    write_project(&dir, MAIN_TSCN, MAIN_GD);

    let mut boot = boot(&dir);
    let main_id = boot
        .main_loop()
        .expect("main loop built")
        .tree()
        .get_node_by_path("/root/Main")
        .expect("main scene instanced");

    // `_ready` ran and the *script* created a tween through the bridge. Before
    // this bead `create_tween` was an unknown function, so this count was 0.
    assert_eq!(
        boot.main_loop().unwrap().tree().tween_count(),
        1,
        "the script's create_tween() registered a tween in the tree"
    );

    // Nothing has advanced yet: the sprite sits at its .tscn position.
    assert_eq!(sprite_x(boot.main_loop().unwrap().tree()), 0.0);

    // Mid-flight: the property is being interpolated, not snapped.
    for _ in 0..2 {
        boot.main_loop_mut().unwrap().step(0.1);
    }
    let mid = sprite_x(boot.main_loop().unwrap().tree());
    assert!(
        mid > 0.0 && mid < 100.0,
        "position:x should be mid-interpolation after 0.2s of a 0.5s tween, got {mid}"
    );
    // The `:x` subname addressed the component, not a junk property. A flat
    // `set_property("position:x", ..)` would leave `position` at (0,0) and
    // `mid` above would still be 0.0 — this is what makes that check bite.
    assert_eq!(
        as_int(script_var(
            boot.main_loop().unwrap().tree(),
            main_id,
            "callback_count"
        )),
        0,
        "the callback must not fire while the property step is still running"
    );

    // Run past the end of the 0.5s tween.
    for _ in 0..10 {
        boot.main_loop_mut().unwrap().step(0.1);
    }

    let tree = boot.main_loop().unwrap().tree();
    assert_eq!(
        sprite_x(tree),
        100.0,
        "the tween settles exactly on its final value"
    );
    assert_eq!(
        as_int(script_var(tree, main_id, "callback_count")),
        1,
        "tween_callback fires EXACTLY once — not never (the bug) and not once per frame"
    );
    assert_eq!(
        as_int(script_var(tree, main_id, "finished_count")),
        1,
        "the `finished` signal reached the script's connected handler"
    );
    // A completed non-looping tween is reaped.
    assert_eq!(tree.tween_count(), 0);
}

/// `tween_method` delivers the interpolated value to the script each frame.
/// `Tween::method_values` computed these all along and nothing read them.
#[test]
fn gdscript_tween_method_delivers_interpolated_values() {
    let _guard = BOOT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = unique_project_dir("method");
    write_project(&dir, MAIN_TSCN, METHOD_GD);

    let mut boot = boot(&dir);
    let main_id = boot
        .main_loop()
        .expect("main loop built")
        .tree()
        .get_node_by_path("/root/Main")
        .expect("main scene instanced");

    for _ in 0..3 {
        boot.main_loop_mut().unwrap().step(0.1);
    }
    let tree = boot.main_loop().unwrap().tree();
    let calls = as_int(script_var(tree, main_id, "call_count"));
    assert!(
        calls >= 3,
        "the method should receive a value on every frame the tweener is active, got {calls}"
    );
    let v = as_float(script_var(tree, main_id, "last_value"));
    assert!(
        v > 0.0 && v < 10.0,
        "the delivered value is interpolated across 0..10, got {v}"
    );
}

/// The bead's third clause: the Tween family is introspectable. Registered in
/// `gdobject::class_db` — schema diffed against
/// `fixtures/editor_oracle/godot_extension_api.json`.
#[test]
fn tween_family_registered_in_classdb() {
    use gdobject::class_db::{
        class_exists, class_get_integer_constant, class_has_method, class_has_signal, is_abstract,
        is_parent_class, register_3d_classes,
    };
    register_3d_classes();

    assert!(class_exists("Tween"));
    // ⚠️ A RefCounted, NOT a Resource — a tween is live playback, never saved.
    assert!(is_parent_class("Tween", "RefCounted"));
    assert!(is_parent_class("Tween", "Object"));
    assert!(class_has_method("Tween", "tween_property"));
    assert!(class_has_method("Tween", "tween_callback"));
    assert!(class_has_signal("Tween", "finished"));

    // ⚠️ `Tweener` is the one abstract member: a script never constructs one, it
    // receives them from the `tween_*` factories.
    assert!(is_abstract("Tweener"));
    for t in [
        "PropertyTweener",
        "IntervalTweener",
        "CallbackTweener",
        "MethodTweener",
        "SubtweenTweener",
    ] {
        assert!(class_exists(t), "{t} must be registered");
        assert!(is_parent_class(t, "Tweener"), "{t} <- Tweener");
        assert!(!is_abstract(t), "{t} is instantiable in Godot");
    }

    // ⚠️ Enum VALUES, not just names — QUINT/QUART sit between SINE and QUAD, so
    // the "obvious" ordering gets TRANS_QUAD and TRANS_CUBIC wrong. These are
    // the numbers `set_trans`/`set_ease` receive from a script.
    for (name, value) in [
        ("TRANS_LINEAR", 0),
        ("TRANS_SINE", 1),
        ("TRANS_QUINT", 2),
        ("TRANS_QUART", 3),
        ("TRANS_QUAD", 4),
        ("TRANS_EXPO", 5),
        ("TRANS_ELASTIC", 6),
        ("TRANS_CUBIC", 7),
        ("TRANS_CIRC", 8),
        ("TRANS_BOUNCE", 9),
        ("TRANS_BACK", 10),
        ("TRANS_SPRING", 11),
        ("EASE_IN", 0),
        ("EASE_OUT", 1),
        ("EASE_IN_OUT", 2),
        ("EASE_OUT_IN", 3),
    ] {
        assert_eq!(
            class_get_integer_constant("Tween", name),
            Some(value),
            "{name} must match extension_api"
        );
    }

    // Not vacuous: an invented constant is absent, so the lookups above
    // discriminate rather than returning Some for anything.
    assert_eq!(class_get_integer_constant("Tween", "TRANS_NOT_REAL"), None);
    assert!(!class_has_method("Tween", "tween_nonexistent"));
}
