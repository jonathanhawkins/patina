//! pat-mgixk.9 part 1 acceptance: a signal connection can target a NATIVE
//! method, including on a node with no script at all.
//!
//! In Godot, `Object::emit_signalp` ends at `Callable::callp` → `Object::callp`,
//! which tries the script instance and then the ClassDB method bind
//! (`core/object/object.cpp`). A signal can therefore target any native method,
//! which is why `timer.timeout.connect(enemy.queue_free)` and
//! `$Button.pressed.connect($Label.hide)` are everyday Godot.
//!
//! Patina's emission path had no second half. `SceneTree::emit_signal`
//! dispatched a no-callback connection only `if self.scripts.contains_key(..)`,
//! and `call_script_method` declines a method the script does not declare — so
//! both idioms, and a `.tscn` `[connection ... method="queue_free"]` onto a
//! scriptless node, connected successfully and then did **nothing**, silently.
//! The native dispatch table existed the whole time; emission never reached it.
//!
//! Two independent routes are pinned here, because they reach the target
//! through different code:
//!   * a `.tscn`-authored `[connection]` (`packed_scene::wire_connections`)
//!     onto `Enemy`, which has **no script**, calling `queue_free`;
//!   * a script-authored `connect` onto `Label`, which also has no script,
//!     calling `hide`.
//!
//! Entry point is the real runner: `patina_runner::bootstrap` loads a
//! `project.godot` + `main.tscn` from disk and `MainLoop::step` drives the
//! frames. Assertions are on what the game observes — a node gone from the
//! tree, and a node's own `visible` property.
//!
//! ⚠️ Deviation from the bead's literal wording, recorded on purpose: the
//! acceptance writes `$Button.pressed.connect($Label.hide)`, using the Signal
//! VALUE and a bare method reference. This test uses the equivalent
//! `connect("pressed", Callable($Label, "hide"))` / `emit_signal("pressed")`
//! spelling. Both land in the same `SignalStore` → `emit_signal` →
//! `call_object_method` path that this bead is about, and the Signal-value sugar
//! is already pinned by `signal_value_emit_connect` (pat-0q2kx.67); spelling it
//! this way keeps the test measuring native dispatch rather than re-measuring
//! Signal-value resolution.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use gdvariant::Variant;
use patina_runner::bootstrap::{BootConfig, EngineBootstrap};

/// `EngineBootstrap` touches process-global state (ClassDB, resource roots), so
/// the runner tests serialize on one lock.
static BOOT_LOCK: Mutex<()> = Mutex::new(());

const DT: f64 = 1.0 / 60.0;

fn unique_project_dir(tag: &str) -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "patina_mgixk9_{}_{}_{}",
        tag,
        std::process::id(),
        n
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// `Enemy` and `Label` carry NO script — that is the point. The `[connection]`
/// block is the editor-authored route: `Timer.timeout` → `Enemy.queue_free`.
const MAIN_TSCN: &str = r#"[gd_scene load_steps=2 format=3]

[ext_resource type="Script" path="res://main.gd" id="1_main"]

[node name="Main" type="Node2D"]
script = ExtResource("1_main")

[node name="Timer" type="Timer" parent="."]
wait_time = 0.05
one_shot = true
autostart = true

[node name="Enemy" type="Node2D" parent="."]

[node name="Button" type="Button" parent="."]

[node name="Label" type="Label" parent="."]

[connection signal="timeout" from="Timer" to="Enemy" method="queue_free"]
"#;

/// Connects in `_ready` and emits on the first `_process` frame.
///
/// ⚠️ The emit is deferred by a frame deliberately. A running script is checked
/// out of `SceneTree.scripts` for the duration of its own call, so a signal
/// emitted from `_ready` cannot reach a handler on a script that is still
/// executing. The target here is scriptless, so it would in fact work from
/// `_ready` — but keeping the emit in `_process` means this test is measuring
/// native dispatch and not accidentally resting on that re-entrancy quirk
/// (which is part 2 of this bead).
const MAIN_GD: &str = "\
extends Node2D
var emitted = false

func _ready():
    $Button.connect(\"pressed\", Callable($Label, \"hide\"))

func _process(delta):
    if not emitted:
        emitted = true
        $Button.emit_signal(\"pressed\")
";

fn write_project(dir: &Path) {
    std::fs::write(
        dir.join("project.godot"),
        "[application]\nconfig/name=\"SignalNative\"\nrun/main_scene=\"res://main.tscn\"\n",
    )
    .unwrap();
    std::fs::write(dir.join("main.tscn"), MAIN_TSCN).unwrap();
    std::fs::write(dir.join("main.gd"), MAIN_GD).unwrap();
}

fn boot(dir: &Path) -> EngineBootstrap {
    gdobject::class_db::register_2d_classes();
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

/// **The acceptance test.**
#[test]
fn signal_native_method_target() {
    let _guard = BOOT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = unique_project_dir("native");
    write_project(&dir);
    let mut boot = boot(&dir);

    // -- the control: both targets are present and untouched after boot ------
    //
    // Without this, "Enemy is gone" would also pass on a scene that failed to
    // instance Enemy at all, and "Label is hidden" on a Label that was never
    // visible.
    {
        let tree = boot.main_loop().expect("main loop built").tree();
        assert!(
            tree.get_node_by_path("/root/Main/Enemy").is_some(),
            "Enemy must exist before the timer fires, or the test proves nothing",
        );
        let label = tree
            .get_node_by_path("/root/Main/Label")
            .expect("Label must exist in the scene");
        assert_ne!(
            tree.get_node(label)
                .expect("Label node")
                .get_property("visible"),
            Variant::Bool(false),
            "Label must start visible, or the hide assertion proves nothing",
        );
    }

    // A second of frames: well past the Timer's 0.05s wait, and long enough for
    // `queue_free` to be reaped.
    for _ in 0..60 {
        boot.main_loop_mut().expect("main loop").step(DT);
    }

    let tree = boot.main_loop().expect("main loop built").tree();

    // -- route 1: a .tscn [connection] reached a native method on a
    //    SCRIPTLESS node -----------------------------------------------------
    assert!(
        tree.get_node_by_path("/root/Main/Enemy").is_none(),
        "Timer.timeout wired to Enemy.queue_free in the .tscn must free Enemy; \
         it is still in the tree, so the connection fired into nothing",
    );

    // -- route 2: a script-authored connect reached a native method ----------
    let label = tree
        .get_node_by_path("/root/Main/Label")
        .expect("Label must survive");
    assert_eq!(
        tree.get_node(label)
            .expect("Label node")
            .get_property("visible"),
        Variant::Bool(false),
        "pressed → Callable($Label, \"hide\") must hide the Label",
    );

    // The emitting script did run — a scene where `_process` never fired would
    // satisfy nothing above by accident.
    let main_id = tree
        .get_node_by_path("/root/Main")
        .expect("main scene instanced");
    assert_eq!(
        tree.get_script(main_id)
            .expect("Main has a script")
            .get_property("emitted"),
        Some(Variant::Bool(true)),
        "Main._process must have run and emitted",
    );
}
