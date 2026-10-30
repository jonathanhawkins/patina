//! pat-s3rrv.35 acceptance: SceneTreeTimers tick once per frame and honour
//! `create_timer`'s flags.
//!
//! # The bug
//!
//! `MainLoop::step` called BOTH `SceneTree::process_frame_with_scripts` and
//! `SceneTree::process_script_coroutines`, and each began with its own
//! `process_timers(delta)`. Every SceneTreeTimer therefore lost 2×delta per
//! frame, so `await get_tree().create_timer(1.0).timeout` resumed after ~0.5 s
//! of game time and every timed gameplay beat ran at double speed. The coroutine
//! pass is the correct single owner because it is the one that then drains
//! `take_timeout_signals` to wake the parked awaits.
//!
//! `create_timer` also dropped all three of Godot's flags
//! (`process_always`, `process_in_physics`, `ignore_time_scale`), so a pause
//! menu's timers could not keep running, a physics-rate timer could not exist,
//! and a UI timeout could not survive `Engine.time_scale`.
//!
//! Everything here is driven through `MainLoop::step` — the real frame loop —
//! rather than calling `process_timers` directly, because a direct unit call
//! could not have caught a double-advance that lives in the loop's wiring.

use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;
use gdscene::{GDScriptNodeInstance, MainLoop};
use gdscript_interop::bindings::ScriptInstance;
use gdvariant::Variant;

const FRAME: f64 = 1.0 / 60.0;

/// A one-node scene, instanced the way a real project's main scene is.
const SCENE: &str = r#"[gd_scene format=3]

[node name="Main" type="Node2D"]
"#;

/// Builds the scene, attaches `script_src` to its root, and returns a running
/// loop plus the root's id.
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

fn prop(ml: &MainLoop, id: gdscene::node::NodeId, name: &str) -> Variant {
    ml.tree()
        .get_script(id)
        .expect("script attached")
        .get_property(name)
        .unwrap_or(Variant::Nil)
}

/// Serializes every test in this file, because `Engine.time_scale` is a
/// PROCESS-GLOBAL atomic (`ENGINE_TIME_SCALE_MICRO`, behind
/// `gdscript_interop::set_engine_time_scale`) and not per-`MainLoop` state.
///
/// Each test here opens by setting the scale, and
/// `ignore_time_scale_timers_are_unaffected_by_engine_time_scale` holds it at
/// 0.5 for its whole body. Under the default parallel harness that 0.5 window
/// overlaps the others, so whichever test is mid-`step` reads a scale it never
/// set — which is why a DIFFERENT one of these four failed on every run while
/// all four passed under `--test-threads=1` (pat-21oab.3). Re-setting the value
/// at the top of each test is a *rearm*, not isolation: the writes interleave,
/// and nothing in the test can tell that it happened.
///
/// `--test-threads=1` is not the fix: it would serialize the whole binary and
/// leave the next test that touches this global flaky all over again.
static TIME_SCALE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Holds [`TIME_SCALE_LOCK`] for the rest of the test and restores Godot's
/// default `time_scale` of 1.0 on drop — including on an unwind, so a failing
/// test cannot leave 0.5 behind for whichever test runs next.
struct TimeScale(#[allow(dead_code)] std::sync::MutexGuard<'static, ()>);

impl Drop for TimeScale {
    fn drop(&mut self) {
        gdscript_interop::set_engine_time_scale(1.0);
    }
}

/// Takes the lock, then sets `Engine.time_scale`. Bind it with
/// `let _scale = time_scale(x);` — binding to plain `_` would drop the guard
/// immediately and release the lock before the test body ran.
///
/// Poison-tolerant, matching `gdcore::file_access`'s `ROOTS_LOCK`: a panicking
/// test must fail on its own rather than poison the mutex and cascade into
/// every other test in the binary.
fn time_scale(scale: f64) -> TimeScale {
    let lock = TIME_SCALE_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    gdscript_interop::set_engine_time_scale(scale);
    TimeScale(lock)
}

/// **The acceptance test.** A 1-second timer must resume near frame 60, not 30.
///
/// ⚠️ The firing frame is counted **in Rust**, by stepping one frame at a time
/// and watching for the post-`await` flag to flip. An earlier version of this
/// test had the script count its own frames in `_process` and stamped that
/// counter after the await — and it read `0`, which looks exactly like "the
/// timer fired immediately" but actually meant `_process` never ran, so the
/// counter never left its initialiser. This fixture attaches the script and
/// calls `process_script_ready` directly rather than running the full
/// `enter_tree` lifecycle, so the node is never process-enabled and the
/// in-script counter can only ever be 0.
///
/// Counting from the loop that does the stepping removes the dependency
/// entirely: the frame index is the test's own, it cannot be zero by accident,
/// and "never fired" becomes a distinct outcome from "fired on frame 0".
#[test]
fn scene_tree_timer_advances_once_per_frame_with_flags() {
    let _scale = time_scale(1.0);

    let (mut ml, main) = scene_with_script(
        "\
extends Node2D
var fired = false
func _ready():
\tawait get_tree().create_timer(1.0).timeout
\tself.fired = true
",
    );

    assert_eq!(
        prop(&ml, main, "fired"),
        Variant::Bool(false),
        "await must suspend _ready before the timer fires"
    );

    // Run up to 90 frames — well past both the correct answer (60) and the buggy
    // one (30) — and record the frame the flag actually flips on. A budget-style
    // "fired within 90 frames" assertion is what let the double-advance survive,
    // since firing at 30 satisfies it just as well as firing at 60.
    let mut fired_at: Option<usize> = None;
    for frame in 1..=90 {
        ml.step(FRAME);
        if prop(&ml, main, "fired") == Variant::Bool(true) {
            fired_at = Some(frame);
            break;
        }
    }

    let fired_at = fired_at.expect(
        "the timer never fired within 90 frames (1.5 s of game time). The await \
         is parked forever — either process_script_coroutines is not advancing \
         timers at all, or it is not draining take_timeout_signals to wake the \
         parked await.",
    );
    assert!(
        (59..=61).contains(&fired_at),
        "a 1.0 s timer at 60 fps must resume on frame ~60 (±1), got {fired_at}. \
         ~30 means the timer is still being advanced twice per frame — once by \
         process_frame_with_scripts and once by process_script_coroutines. A \
         very small number means it is being advanced by something far faster \
         than the frame delta."
    );
}

/// `process_always = false` freezes while the tree is paused; the default
/// (`true`) keeps running. Both timers live in the same tree, so this compares
/// them under identical stepping rather than across two runs.
#[test]
fn paused_tree_freezes_only_non_process_always_timers() {
    let _scale = time_scale(1.0);
    let mut ml = MainLoop::new(SceneTree::new());

    let pausable = ml
        .tree_mut()
        .create_timer_with_flags(0.5, false, false, false);
    let always = ml
        .tree_mut()
        .create_timer_with_flags(0.5, true, false, false);

    ml.tree_mut().set_paused(true);
    for _ in 0..60 {
        ml.step(FRAME);
    }

    // The `process_always` timer fired and was drained by the coroutine pass;
    // the pausable one is still sitting at its full 0.5 s.
    assert!(
        ml.tree().get_timer(always).is_none(),
        "a process_always timer must keep running while paused"
    );
    let left = ml
        .tree()
        .get_timer(pausable)
        .expect("a paused timer must still exist")
        .time_left;
    assert!(
        (left - 0.5).abs() < 1e-9,
        "a process_always=false timer must not advance at all while paused, \
         but {left} s remain of 0.5"
    );

    // Unpausing resumes it.
    ml.tree_mut().set_paused(false);
    for _ in 0..60 {
        ml.step(FRAME);
    }
    assert!(
        ml.tree().get_timer(pausable).is_none(),
        "unpausing must let the timer finish"
    );
}

/// `process_in_physics = true` advances on the fixed tick, so its rate follows
/// `physics_ticks_per_second` — the check the bead names explicitly.
#[test]
fn process_in_physics_timers_follow_the_physics_rate() {
    let _scale = time_scale(1.0);

    // At 10 ticks/second, 40 frames of 1/60 s yields 6 physics ticks = 0.6 s of
    // physics time, so a 0.4 s timer fires and a 0.9 s one does not.
    //
    // Deliberately NOT the tighter 30 frames: `6 * (1.0/60.0)` sums to
    // 0.09999999999999999 in binary floating point, a hair UNDER the 0.1 s step,
    // so ticks land on frames 7/13/19/25 rather than 6/12/18/24. 30 frames
    // therefore gives 4 ticks, and `0.4 - 0.1*4` leaves 5.55e-17 — still
    // positive, so the timer correctly had not finished. Both bounds here clear
    // the boundary by a wide margin so the test measures the phase routing, not
    // float accumulation.
    let mut ml = MainLoop::new(SceneTree::new());
    ml.set_physics_ticks_per_second(10);
    let short = ml
        .tree_mut()
        .create_timer_with_flags(0.4, true, true, false);
    let long = ml
        .tree_mut()
        .create_timer_with_flags(0.9, true, true, false);
    for _ in 0..40 {
        ml.step(FRAME);
    }
    assert!(
        ml.tree().get_timer(short).is_none(),
        "0.4 s of physics time must elapse within 6 ticks of 0.1 s. Still \
         running means process_in_physics timers are not being advanced by the \
         physics tick at all."
    );
    assert!(
        ml.tree().get_timer(long).is_some(),
        "0.9 s must NOT have elapsed in 0.6 s of physics time — if it did, the \
         timer is being advanced by the idle pass too"
    );

    // Same wall-clock stepping at 60 ticks/second: now the 0.9 s timer also
    // fires, which is what "follows physics_ticks_per_second" means. Were these
    // timers advanced by the idle frame they would behave identically in both
    // runs.
    let mut fast = MainLoop::new(SceneTree::new());
    fast.set_physics_ticks_per_second(60);
    let long_fast = fast
        .tree_mut()
        .create_timer_with_flags(0.9, true, true, false);
    for _ in 0..60 {
        fast.step(FRAME);
    }
    assert!(
        fast.tree().get_timer(long_fast).is_none(),
        "at 60 ticks/s, 60 frames is 1.0 s of physics time and 0.9 s must elapse"
    );
}

/// `ignore_time_scale = true` uses the driver's real delta, so `time_scale`
/// does not stretch it.
#[test]
fn ignore_time_scale_timers_are_unaffected_by_engine_time_scale() {
    let _scale = time_scale(0.5);
    let mut ml = MainLoop::new(SceneTree::new());

    let unscaled = ml
        .tree_mut()
        .create_timer_with_flags(0.5, true, false, true);
    let scaled = ml
        .tree_mut()
        .create_timer_with_flags(0.5, true, false, false);

    // 33 frames = 0.55 s of real time but only 0.275 s of scaled time.
    for _ in 0..33 {
        ml.step(FRAME);
    }

    assert!(
        ml.tree().get_timer(unscaled).is_none(),
        "an ignore_time_scale timer must use the real delta and fire after \
         0.5 s of wall time regardless of Engine.time_scale"
    );
    assert!(
        ml.tree().get_timer(scaled).is_some(),
        "…while an ordinary timer is stretched by time_scale = 0.5 and must \
         still be running. If this one also fired, the unscaled delta is being \
         applied to every timer."
    );
}
