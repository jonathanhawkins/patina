//! pat-s3rrv.121 — `$Timer.start()` / `stop()` / `is_stopped()` from GDScript.
//!
//! # The bug
//!
//! `SceneTree::process_timer_nodes` drives Timers from stashed
//! `__timer_running` / `time_left` properties, and **nothing outside that
//! function ever wrote them**. `SceneTreeAccessor::call_node_method` had no
//! Timer arm, so `$Timer.start()` was a silent no-op — a Timer without
//! `autostart = true` could never run at all, which is most of them.
//!
//! ⚠️ Firing is observed **in Rust**, by stepping one frame at a time and
//! reading the node's own state, not by a counter inside the script. This
//! fixture attaches the script and calls `process_script_ready` directly rather
//! than running the full `enter_tree` lifecycle, so the node is never
//! process-enabled and an in-script `_process` counter could only ever read 0 —
//! which looks identical to "fired immediately". The sibling
//! `scene_tree_timer_flags` test was burned by exactly that.

use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;
use gdscene::{GDScriptNodeInstance, MainLoop};
use gdscript_interop::bindings::ScriptInstance;
use gdvariant::Variant;

const FRAME: f64 = 1.0 / 60.0;

/// A half-second one-shot Timer that does **not** autostart — the case that
/// could never run before this change.
const SCENE: &str = r#"[gd_scene format=3]

[node name="Main" type="Node2D"]

[node name="Clock" type="Timer" parent="."]
wait_time = 0.5
one_shot = true
"#;

fn scene_with_script(script_src: &str) -> (MainLoop, gdscene::node::NodeId) {
    let scene = PackedScene::from_tscn(SCENE).expect("scene parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let main = add_packed_scene_to_tree(&mut tree, root, &scene).expect("scene instances");

    let script = GDScriptNodeInstance::from_source(script_src, main).expect("script parses");
    tree.attach_script(main, Box::new(script));

    let mut ml = MainLoop::new(tree);
    ml.tree_mut().process_script_ready(main);
    (ml, main)
}

fn script_var(ml: &MainLoop, id: gdscene::node::NodeId, name: &str) -> Variant {
    ml.tree()
        .get_script(id)
        .expect("script attached")
        .get_property(name)
        .unwrap_or(Variant::Nil)
}

fn clock_id(ml: &MainLoop) -> gdscene::node::NodeId {
    ml.tree()
        .all_nodes_in_tree_order()
        .into_iter()
        .find(|id| {
            ml.tree()
                .get_node(*id)
                .map(|n| n.name() == "Clock")
                .unwrap_or(false)
        })
        .expect("no node named Clock")
}

fn clock_prop(ml: &MainLoop, name: &str) -> Variant {
    ml.tree()
        .get_node(clock_id(ml))
        .expect("the Timer exists")
        .get_property(name)
}

fn running(ml: &MainLoop) -> bool {
    matches!(clock_prop(ml, "__timer_running"), Variant::Bool(true))
}

fn time_left(ml: &MainLoop) -> f64 {
    match clock_prop(ml, "time_left") {
        Variant::Float(f) => f,
        Variant::Int(i) => i as f64,
        _ => 0.0,
    }
}

/// **The core of the bead.** A script calling `$Timer.start()` makes the Timer
/// actually count down and fire.
#[test]
fn a_scripted_start_runs_the_timer_to_completion() {
    let (mut ml, _main) = scene_with_script("extends Node2D\nfunc _ready():\n\t$Clock.start()\n");

    assert!(running(&ml), "start() must arm the timer");
    assert!(
        (time_left(&ml) - 0.5).abs() < 1e-6,
        "start() seeds time_left from wait_time, got {}",
        time_left(&ml)
    );

    // 20 frames is 1/3 s — short of the half-second wait.
    for _ in 0..20 {
        ml.step(FRAME);
    }
    assert!(running(&ml), "the timer must still be running at 20 frames");

    // Past 30 frames it has fired; being one_shot, it stops.
    for _ in 0..12 {
        ml.step(FRAME);
    }
    assert!(
        !running(&ml),
        "a one_shot timer stops after firing at ~30 frames"
    );
}

/// 🔑 The control that shows the bug. Without the scripted `start()` — and with
/// no `autostart` — the timer must never run, which is precisely the state
/// every non-autostart Timer was stuck in.
#[test]
fn without_a_scripted_start_the_timer_never_runs() {
    let (mut ml, _main) = scene_with_script("extends Node2D\nfunc _ready():\n\tpass\n");

    for _ in 0..60 {
        ml.step(FRAME);
    }
    assert!(
        !running(&ml),
        "no autostart and no start() means no countdown"
    );
    assert_eq!(time_left(&ml), 0.0);
}

/// `stop()` halts a running timer, and `is_stopped()` reports the transition.
///
/// ⚠️ `is_stopped()` is read from the CLOCK, not from a running flag: Godot
/// defines it as `get_time_left() <= 0` (`timer.cpp:155-161`). Both readings
/// agree here, but they diverge for a paused timer, which reports *not*
/// stopped while holding its remaining time.
#[test]
fn stop_halts_the_timer_and_is_stopped_reports_it() {
    let (mut ml, main) = scene_with_script(
        "extends Node2D\nvar before = true\nvar after = true\nfunc _ready():\n\t\
         self.before = $Clock.is_stopped()\n\t$Clock.start()\n\t\
         self.after = $Clock.is_stopped()\n",
    );

    assert_eq!(
        script_var(&ml, main, "before"),
        Variant::Bool(true),
        "a timer that was never started is stopped"
    );
    assert_eq!(
        script_var(&ml, main, "after"),
        Variant::Bool(false),
        "a timer with time on the clock is NOT stopped"
    );

    for _ in 0..10 {
        ml.step(FRAME);
    }
    assert!(running(&ml), "still counting down before any stop()");
}

/// `stop()` sticks: the timer never resumes, and — because Godot's `stop()` also
/// clears `autostart` (`timer.cpp:131`) — it stays stopped even in a scene that
/// asked to autostart.
#[test]
fn a_stopped_timer_stays_stopped() {
    let (mut ml, main) = scene_with_script(
        "extends Node2D\nvar stopped = false\nfunc _ready():\n\t$Clock.start()\n\t\
         $Clock.stop()\n\tself.stopped = $Clock.is_stopped()\n",
    );

    assert_eq!(
        script_var(&ml, main, "stopped"),
        Variant::Bool(true),
        "stop() must make is_stopped() true"
    );
    assert!(!running(&ml), "stop() clears the running flag");

    for _ in 0..60 {
        ml.step(FRAME);
    }
    assert!(!running(&ml), "a stopped timer must not resume");
    assert_eq!(
        clock_prop(&ml, "autostart"),
        Variant::Bool(false),
        "stop() also clears autostart, so the init pass cannot restart it"
    );
}

/// 🔑 `start(time_sec)` changes `wait_time` **permanently**.
///
/// ⚠️ Godot calls `set_wait_time(p_time)` (`timer.cpp:121-123`); it does not
/// override the duration for a single run. A repeating timer restarted with
/// `start(0.25)` keeps firing every 0.25 s afterwards. The bead's own text
/// describes this as a per-run override, which would leave `wait_time` at 0.5
/// and make every later cycle twice as long.
#[test]
fn start_with_a_duration_rewrites_wait_time() {
    let (mut ml, _main) =
        scene_with_script("extends Node2D\nfunc _ready():\n\t$Clock.start(0.25)\n");

    match clock_prop(&ml, "wait_time") {
        Variant::Float(f) => assert!(
            (f - 0.25).abs() < 1e-6,
            "wait_time must be rewritten to 0.25, got {f}"
        ),
        other => panic!("wait_time must be a float, got {other:?}"),
    }
    assert!((time_left(&ml) - 0.25).abs() < 1e-6);

    // ~15 frames is a quarter second: still running just before, stopped after.
    for _ in 0..12 {
        ml.step(FRAME);
    }
    assert!(running(&ml), "must not fire before its quarter second");
    for _ in 0..6 {
        ml.step(FRAME);
    }
    assert!(!running(&ml), "must have fired by ~15 frames");
}
