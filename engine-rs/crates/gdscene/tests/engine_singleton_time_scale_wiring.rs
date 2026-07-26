//! pat-0q2kx.121 — the `Engine` singleton is wired to the running main loop.
//!
//! Godot's `Engine` singleton is read by `Main::iteration` every frame:
//! `time_scale` multiplies the frame delta before the process and physics
//! phases (doc/classes/Engine.xml:348-353), `physics_ticks_per_second` sets the
//! fixed step, `get_process_frames`/`get_physics_frames` count real frames, and
//! `get_frames_per_second` reports the *measured* rate.
//!
//! Patina's `call_engine_static` implemented exactly two methods
//! (`is_editor_hint`, `get_frames_per_second`), the FPS value was the hardcoded
//! `60.0` its atomic was initialised with because `set_engine_frames_per_second`
//! had no caller, and `Engine.time_scale` did not exist anywhere in the engine —
//! reading or assigning it from a script was an error. So a `.tscn`-loaded
//! script asking for slow motion changed nothing at all.
//!
//! This drives the real wiring: a scene script sets `Engine.time_scale = 0.5` in
//! `_ready` and the deltas `MainLoop::step` hands `_process` must be halved,
//! while the frame counters, the project-configured tick rate and the measured
//! FPS all read back live values rather than constants.

use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;
use gdscene::{GDScriptNodeInstance, LifecycleManager, MainLoop, NodeId};
use gdscript_interop::bindings::ScriptInstance;
use gdvariant::Variant;

const SCENE: &str = r#"[gd_scene format=3]

[node name="Root" type="Node2D"]

[node name="Ticker" type="Node2D" parent="."]
"#;

/// Slow motion is requested in `_ready`, exactly as a game would; every other
/// value is sampled from `_process` so the assertions read what a script
/// actually observes mid-frame.
const TICKER_SCRIPT: &str = "\
extends Node2D
var delta_sum = 0.0
var frames_seen = -1
var physics_frames_seen = -1
var tps_seen = 0
var fps_seen = 0.0
var scale_seen = 0.0
func _ready():
    Engine.time_scale = 0.5
func _process(delta):
    self.delta_sum = self.delta_sum + delta
    self.frames_seen = Engine.get_process_frames()
    self.physics_frames_seen = Engine.get_physics_frames()
    self.tps_seen = Engine.get_physics_ticks_per_second()
    self.fps_seen = Engine.get_frames_per_second()
    self.scale_seen = Engine.time_scale
";

/// The `Engine` singleton is process-global (that is the whole point — one
/// engine per process), so these tests must not interleave their resets under a
/// runner that shares a process across tests.
static ENGINE_STATE: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn lock_engine_state() -> std::sync::MutexGuard<'static, ()> {
    ENGINE_STATE.lock().unwrap_or_else(|e| e.into_inner())
}

fn script_prop(tree: &SceneTree, node: NodeId, prop: &str) -> Variant {
    tree.get_script(node)
        .expect("node has a script")
        .get_property(prop)
        .unwrap_or(Variant::Nil)
}

fn prop_f64(tree: &SceneTree, node: NodeId, prop: &str) -> f64 {
    match script_prop(tree, node, prop) {
        Variant::Float(f) => f,
        Variant::Int(i) => i as f64,
        other => panic!("{prop} should be a number, got {other:?}"),
    }
}

fn prop_i64(tree: &SceneTree, node: NodeId, prop: &str) -> i64 {
    match script_prop(tree, node, prop) {
        Variant::Int(i) => i,
        other => panic!("{prop} should be an int, got {other:?}"),
    }
}

/// Boots the `.tscn` the way the runner does: instance the packed scene, attach
/// the script, run `_ready` through `enter_tree`, then build the loop and apply
/// `project.godot`.
fn boot() -> (MainLoop, NodeId, std::sync::MutexGuard<'static, ()>) {
    let guard = lock_engine_state();
    gdscript_interop::reset_engine_singleton_state();

    let scene = PackedScene::from_tscn(SCENE).expect("scene parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let scene_root = add_packed_scene_to_tree(&mut tree, root, &scene).expect("scene instances");
    let ticker = tree
        .get_node_or_null(scene_root, "Ticker")
        .expect("Ticker node");

    let script = GDScriptNodeInstance::from_source(TICKER_SCRIPT, ticker).expect("script parses");
    tree.attach_script(ticker, Box::new(script));

    // `_ready` runs here — before the loop exists, which is why the Engine state
    // may not be reset by `MainLoop::new`.
    LifecycleManager::enter_tree(&mut tree, scene_root);

    let mut settings = gdcore::ProjectSettings::new();
    settings.set(
        "physics/common/physics_ticks_per_second",
        gdcore::SettingsValue::Int(30),
    );
    let mut ml = MainLoop::new(tree);
    ml.configure_from_project_settings(&settings);
    (ml, ticker, guard)
}

#[test]
fn engine_singleton_time_scale_wiring() {
    let (mut ml, ticker, _engine_state) = boot();

    // The `_ready` assignment reached the engine, not just a script-local slot.
    assert_eq!(
        gdscript_interop::engine_time_scale(),
        0.5,
        "`Engine.time_scale = 0.5` in _ready must land on the Engine singleton",
    );

    // (a) time_scale halves the delta the loop hands `_process`.
    let frames = 10u64;
    let driver_delta = 1.0 / 60.0;
    for _ in 0..frames {
        ml.step(driver_delta);
    }

    let expected_sum = frames as f64 * (driver_delta * 0.5);
    let observed_sum = prop_f64(ml.tree(), ticker, "delta_sum");
    assert!(
        (observed_sum - expected_sum).abs() < 1e-9,
        "10 frames at 1/60 with time_scale 0.5 should sum to ~{expected_sum} of \
         _process delta, got {observed_sum} (unscaled would be {})",
        frames as f64 * driver_delta,
    );
    assert_eq!(
        prop_f64(ml.tree(), ticker, "scale_seen"),
        0.5,
        "`Engine.time_scale` must read back the value the script set",
    );

    // (b) The frame counters are live, not constants. Godot increments the
    // process-frame counter at the end of the iteration, so the last `_process`
    // observed `frames - 1` and the post-step count is `frames`.
    assert_eq!(
        prop_i64(ml.tree(), ticker, "frames_seen"),
        frames as i64 - 1,
        "Engine.get_process_frames() must advance with real ticks",
    );
    assert_eq!(
        gdscript_interop::engine_process_frames(),
        frames,
        "the engine-side process frame count must match the frames driven",
    );

    // 10 scaled frames = 10 * (1/120) s of simulated time, which is 2 full
    // ticks of the 30 Hz fixed step, so the physics counter must have moved.
    let physics_frames = prop_i64(ml.tree(), ticker, "physics_frames_seen");
    assert!(
        physics_frames > 0,
        "Engine.get_physics_frames() must count real physics ticks, got {physics_frames}",
    );
    assert!(
        (physics_frames as u64) <= gdscript_interop::engine_physics_frames(),
        "the script's physics-frame reading must not exceed the engine's own count",
    );

    // (c) The tick rate comes from project.godot (30), not the 60 default.
    assert_eq!(
        prop_i64(ml.tree(), ticker, "tps_seen"),
        30,
        "Engine.get_physics_ticks_per_second() must reflect \
         physics/common/physics_ticks_per_second from project.godot",
    );
    assert_eq!(ml.physics_ticks_per_second(), 30);

    // (d) FPS tracks what the driver actually reports — it used to be a
    // hardcoded 60.0 whatever the frame times were.
    assert!(
        (prop_f64(ml.tree(), ticker, "fps_seen") - 60.0).abs() < 1e-6,
        "a 1/60 driver delta should report 60 fps",
    );
    for _ in 0..3 {
        ml.step(1.0 / 30.0);
    }
    let fps_after = prop_f64(ml.tree(), ticker, "fps_seen");
    assert!(
        (fps_after - 30.0).abs() < 1e-6,
        "Engine.get_frames_per_second() must follow the driver's frame rate, got {fps_after}",
    );
}

/// A script writing `Engine.physics_ticks_per_second` must retune the running
/// fixed step — the write is only meaningful if the loop that owns the step
/// adopts it.
#[test]
fn script_written_physics_tick_rate_retunes_the_loop() {
    let (mut ml, _ticker, _engine_state) = boot();
    assert_eq!(ml.physics_ticks_per_second(), 30);

    gdscript_interop::set_engine_physics_ticks_per_second(120);
    ml.step(1.0 / 60.0);
    assert_eq!(
        ml.physics_ticks_per_second(),
        120,
        "a script-side Engine.physics_ticks_per_second write must reach the loop",
    );
}

/// The rest of the Engine surface the audit called out: max_fps, the jitter fix,
/// the version dictionary and the singleton registry are reachable from script
/// rather than raising `UndefinedFunction`.
#[test]
fn engine_singleton_property_and_query_surface() {
    let _engine_state = lock_engine_state();
    gdscript_interop::reset_engine_singleton_state();

    let src = "\
extends Node
var max_fps = 0
var jitter = 0.0
var version_major = 0
var has_os = false
var has_bogus = true
var registered = 0
func run():
    Engine.max_fps = 144
    self.max_fps = Engine.get_max_fps()
    Engine.physics_jitter_fix = 0.25
    self.jitter = Engine.physics_jitter_fix
    var info = Engine.get_version_info()
    self.version_major = info[\"major\"]
    self.has_os = Engine.has_singleton(\"OS\")
    self.has_bogus = Engine.has_singleton(\"NotASingleton\")
    Engine.register_singleton(\"MyThing\", 7)
    self.registered = Engine.get_singleton(\"MyThing\")
";

    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let node = tree
        .add_child(root, gdscene::node::Node::new("Probe", "Node"))
        .unwrap();
    let script = GDScriptNodeInstance::from_source(src, node).expect("script parses");
    tree.attach_script(node, Box::new(script));
    tree.get_script_mut(node)
        .expect("script attached")
        .call_method("run", &[])
        .expect("Engine surface must not raise");

    assert_eq!(prop_i64(&tree, node, "max_fps"), 144);
    assert_eq!(prop_f64(&tree, node, "jitter"), 0.25);
    assert_eq!(prop_i64(&tree, node, "version_major"), 4);
    assert_eq!(script_prop(&tree, node, "has_os"), Variant::Bool(true));
    assert_eq!(script_prop(&tree, node, "has_bogus"), Variant::Bool(false));
    assert_eq!(prop_i64(&tree, node, "registered"), 7);
    assert_eq!(gdscript_interop::engine_max_fps(), 144);
}
