//! pat-s3rrv.109: the `Performance` singleton is wired end to end.
//!
//! `gdcore::performance` held a complete 432-line monitor registry —
//! `PerformanceRegistry`, `FrameMonitorFeed`, a process-wide `global()` — whose
//! only callers were its own tests. Nothing fed it, and `Performance` was
//! absent from `BUILTIN_SINGLETONS` with no dispatch arm, so
//! `Performance.get_monitor(Performance.TIME_FPS)` failed on the identifier.
//!
//! Three things had to meet for this to be real: the frame loop must WRITE the
//! registry, a script must READ it, and the editor's monitors panel must read
//! the SAME registry rather than computing a second, disagreeing number.
//!
//! 🔑 The fps assertion is deliberately CADENCE-SPECIFIC. "fps > 0" would pass
//! on a registry fed a constant, and 60.0 is the number an untouched fixture
//! would most likely produce by coincidence — so these run at 10 fps, a value
//! neither a default nor a stuck counter produces.

use gdcore::performance::{global, Monitor};
use gdscene::main_loop::MainLoop;
use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscene::GDScriptNodeInstance;
use gdvariant::Variant;

fn script_var(ml: &MainLoop, node: NodeId, name: &str) -> Variant {
    ml.tree()
        .get_script(node)
        .and_then(|s| s.get_property(name))
        .unwrap_or(Variant::Nil)
}

fn as_f64(v: Variant) -> f64 {
    match v {
        Variant::Float(f) => f,
        Variant::Int(i) => i as f64,
        other => panic!("expected a number, got {other:?}"),
    }
}

/// `Root -> Runner` plus three inert siblings, so `object/node_count` is a
/// distinctive number rather than something a stray default could match.
fn scene() -> (MainLoop, NodeId) {
    gdobject::class_db::register_all_classes();

    // The registry is process-wide. The preferred runner gives each test its
    // own process, but resetting keeps these honest under a shared-process
    // runner too — a stale fps from a neighbouring test would otherwise read
    // as a live one.
    global().lock().unwrap().reset();

    let mut tree = SceneTree::new();
    let root_id = tree.root_id();
    let root = tree.add_child(root_id, Node::new("Root", "Node")).unwrap();
    for name in ["Filler1", "Filler2", "Filler3"] {
        tree.add_child(root, Node::new(name, "Node")).unwrap();
    }
    let runner = tree.add_child(root, Node::new("Runner", "Node")).unwrap();

    tree.attach_script(
        runner,
        Box::new(
            GDScriptNodeInstance::from_source(
                "\
extends Node
var frames = 0
var last_fps = -1.0
var last_nodes = -1.0
var custom_value = -1.0
var custom_known = false
var registered = false

func score_monitor():
    return 42.0

func _process(delta):
    self.frames = self.frames + 1
    if self.registered == false:
        Performance.add_custom_monitor(\"game/score\", Callable(self, \"score_monitor\"))
        self.registered = true
    self.last_fps = Performance.get_monitor(Performance.TIME_FPS)
    self.last_nodes = Performance.get_monitor(Performance.OBJECT_NODE_COUNT)
    self.custom_known = Performance.has_custom_monitor(\"game/score\")
    self.custom_value = Performance.get_custom_monitor(\"game/score\")
",
                runner,
            )
            .unwrap(),
        ),
    );

    (MainLoop::new(tree), runner)
}

/// **The acceptance.** A script reads live monitor values that the frame loop
/// put there, and a custom monitor round-trips through its script Callable.
#[test]
fn performance_singleton_monitors_live_from_frame_loop() {
    let (mut ml, runner) = scene();

    // 25 frames at 10 fps. The feed windows `time/fps` over the trailing
    // second, so by now the window is full and the frames before it dropped
    // out — a counter that merely accumulated would read 25, not ~10.
    ml.run_frames(25, 0.1);

    // CONTROL: the handler ran at all. Without this, every monitor assertion
    // below would fail identically whether the registry was never fed or the
    // script never executed — two very different bugs.
    assert_eq!(
        script_var(&ml, runner, "frames"),
        Variant::Int(25),
        "`_process` must have run once per frame"
    );

    // The sentinels are -1.0, so "never written" is distinguishable from a
    // legitimate zero reading.
    let fps = as_f64(script_var(&ml, runner, "last_fps"));
    assert!(
        (10.0..=11.0).contains(&fps),
        "`Performance.TIME_FPS` must report the loop's actual cadence, not a \
         default or a stuck counter. Expected ~10 for a 0.1s step, got {fps}. \
         (11 is also correct: the trailing-second window keeps its left edge, \
         so it holds one more stamp than the rate.)"
    );

    let nodes = as_f64(script_var(&ml, runner, "last_nodes"));
    assert_eq!(
        nodes,
        ml.tree().node_count() as f64,
        "`Performance.OBJECT_NODE_COUNT` must report the live tree"
    );
    assert!(
        nodes >= 5.0,
        "sanity: the fixture builds Root + 3 fillers + Runner, so a count of \
         {nodes} means the monitor is not reading the tree at all"
    );

    // The custom monitor round-trip: registered from script with a Callable,
    // then read back BY NAME through the singleton.
    assert_eq!(
        script_var(&ml, runner, "custom_known"),
        Variant::Bool(true),
        "`has_custom_monitor` must see the monitor the script registered"
    );
    assert_eq!(
        as_f64(script_var(&ml, runner, "custom_value")),
        42.0,
        "`get_custom_monitor` must INVOKE the registered Callable — 42.0 is \
         only reachable by actually calling `score_monitor()`"
    );
}

/// The panel and the script cannot disagree: both read one registry.
///
/// The editor's `/api/monitors` endpoint reads `time/fps` straight out of
/// `performance::global()` whenever a runtime `MainLoop` is live, which is the
/// same value the script above read. Asserting the shared source here keeps
/// that agreement pinned from the crate that actually writes it — gdeditor
/// depends on gdscene, so a gdscene test cannot reach the endpoint itself.
#[test]
fn the_registry_the_editor_reads_is_the_one_the_frame_loop_wrote() {
    let (mut ml, runner) = scene();

    ml.run_frames(25, 0.1);

    let seen_by_script = as_f64(script_var(&ml, runner, "last_fps"));
    let seen_by_reader = global().lock().unwrap().get(Monitor::TimeFps);

    assert!(
        seen_by_reader > 0.0,
        "the frame loop must leave a live fps in the process-wide registry; \
         0 here means nothing feeds it and the editor panel would read a flat \
         zero during Play"
    );
    // The script samples during frame 25 and the loop writes at the END of
    // each frame, so an outside reader is at most one frame ahead.
    assert!(
        (seen_by_reader - seen_by_script).abs() <= 1.0,
        "script saw {seen_by_script} fps but a registry reader saw \
         {seen_by_reader}; more than one frame of skew means they are reading \
         different sources, which is the defect this bead exists to remove"
    );
}

/// The script-facing constant names stay aligned with the monitor enum.
///
/// 🔑 This guards a transcription, and both obvious shortcuts are wrong:
/// upper-casing the monitor STRING `memory/msg_buf_max` gives
/// `MEMORY_MSG_BUF_MAX` (Godot says `MEMORY_MESSAGE_BUFFER_MAX`), and splitting
/// the Rust VARIANT `Physics2dActiveObjects` gives `PHYSICS2D_ACTIVE_OBJECTS`
/// (Godot says `PHYSICS_2D_ACTIVE_OBJECTS`). Either would silently resolve a
/// script's documented spelling to nothing, and `Performance.TIME_FPS` alone —
/// the one name both shortcuts get right — would never reveal it.
#[test]
fn every_monitor_constant_name_round_trips_to_its_monitor() {
    use gdcore::performance::{
        monitor_from_const_name, monitor_from_index, ALL_MONITORS, MONITOR_CONST_NAMES,
    };

    assert_eq!(
        MONITOR_CONST_NAMES.len(),
        ALL_MONITORS.len(),
        "one constant name per monitor"
    );
    for (i, name) in MONITOR_CONST_NAMES.iter().enumerate() {
        assert_eq!(
            monitor_from_const_name(name),
            Some(ALL_MONITORS[i]),
            "`Performance.{name}` must resolve to the monitor at index {i}"
        );
        assert_eq!(monitor_from_index(i as i64), Some(ALL_MONITORS[i]));
    }

    // An out-of-range id reads as "no such monitor" rather than clamping to
    // TIME_FPS, so a typo'd constant cannot masquerade as a live reading.
    assert_eq!(monitor_from_index(-1), None);
    assert_eq!(monitor_from_index(ALL_MONITORS.len() as i64), None);

    // The three spellings the shortcuts get wrong, pinned explicitly.
    assert!(monitor_from_const_name("PHYSICS_2D_ACTIVE_OBJECTS").is_some());
    assert!(monitor_from_const_name("NAVIGATION_3D_EDGE_COUNT").is_some());
    assert!(monitor_from_const_name("MEMORY_MESSAGE_BUFFER_MAX").is_some());
    assert!(
        monitor_from_const_name("PHYSICS2D_ACTIVE_OBJECTS").is_none(),
        "the derived-from-variant spelling must NOT resolve; accepting both \
         would let the table drift out of Godot's naming unnoticed"
    );
}
