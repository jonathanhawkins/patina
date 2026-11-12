//! pat-nzgdu.94 — the browser fixed-timestep driver advances a real `MainLoop`.
//!
//! `patina-runner`'s native CLI advances the engine with a for-loop over a fixed
//! frame count, which is not a real-time loop: it steps the same amount no matter
//! how much wall-clock time passed. A browser build cannot work that way — the
//! browser hands the engine a `requestAnimationFrame` timestamp and the engine
//! must turn that into engine time.
//!
//! `FrameDriver` is that layer, factored as pure logic over an injected timestamp
//! precisely so it can be proven here on native rather than only in a browser.
//!
//! The load-bearing property is that the driver is wired to **elapsed time**, not
//! to a frame count: the same one second of browser time must produce the same
//! Godot-correct physics-tick count and the same scene side effects regardless of
//! how the rAF callbacks were chunked. Both tests below feed one second — first as
//! 60 smooth 16.6ms frames, then as 20 coarse 50ms frames — and require identical
//! physics totals. A driver that stepped "once per callback" would score 60 vs 20
//! and fail the second.

use gdscene::frame_driver::FrameDriver;
use gdscene::node::Node;
use gdscene::scene_tree::SceneTree;
use gdscene::{GDScriptNodeInstance, MainLoop};
use gdscript_interop::bindings::ScriptInstance;
use gdvariant::Variant;

/// Physics rate for these tests — Godot's default.
const TPS: u32 = 60;

/// Builds a MainLoop whose `Main` node counts its own `_process` calls.
///
/// (`+=` avoided for parser-independence, matching the other scripted tests.)
fn scripted_loop() -> (MainLoop, gdscene::node::NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let main = Node::new("Main", "Node");
    let main_id = tree.add_child(root, main).unwrap();

    let script_src = "\
extends Node
var process_calls = 0
func _process(delta):
    self.process_calls = self.process_calls + 1
";
    let script = GDScriptNodeInstance::from_source(script_src, main_id).unwrap();
    tree.attach_script(main_id, Box::new(script));

    let mut ml = MainLoop::new(tree);
    ml.set_physics_ticks_per_second(TPS);
    (ml, main_id)
}

/// Feeds `frames` evenly-spaced animation-frame callbacks spanning `total_secs`,
/// returning `(engine steps, physics ticks)`.
///
/// The first callback only establishes the time origin, so `frames + 1`
/// timestamps are fed to produce `frames` steps.
fn feed(driver: &mut FrameDriver, ml: &mut MainLoop, total_secs: f64, frames: u32) -> (u32, u32) {
    let frame_ms = (total_secs * 1000.0) / frames as f64;
    let mut steps = 0u32;
    let mut physics = 0u32;
    for i in 0..=frames {
        if let Some(out) = driver.tick(i as f64 * frame_ms, ml) {
            steps += 1;
            physics += out.physics_steps;
        }
    }
    (steps, physics)
}

/// The acceptance test: one second of simulated browser time driven through the
/// accumulator produces Godot's physics-tick count on a real `MainLoop` and
/// advances a scripted `_process` counter — proving the driver is wired to the
/// scene tick, not to a fixed batch count.
#[test]
fn web_runtime_fixed_timestep_driver() {
    let (mut ml, main_id) = scripted_loop();
    let mut driver = FrameDriver::new();

    // One second, delivered as 60 smooth ~16.6ms animation frames.
    let (steps, physics) = feed(&mut driver, &mut ml, 1.0, 60);

    assert_eq!(steps, 60, "61 callbacks - 1 origin = 60 engine steps");
    assert_eq!(
        physics, TPS,
        "1s at {TPS}Hz must run exactly {TPS} physics ticks (Godot-correct)"
    );
    assert!(
        (driver.elapsed() - 1.0).abs() < 1e-6,
        "driver accepted 1s of browser time, got {}",
        driver.elapsed()
    );
    assert_eq!(driver.pending(), 0.0, "no time left banked");

    // The tick reached the scene: _process ran once per engine step.
    let script = ml.tree().get_script(main_id).expect("script attached");
    assert_eq!(
        script.get_property("process_calls"),
        Some(Variant::Int(60)),
        "_process must advance once per driven frame"
    );
}

/// The same second delivered in coarser chunks yields the same physics total and
/// the same elapsed time — only the frame (and therefore `_process`) count differs.
///
/// This is what distinguishes a real accumulator from "step once per callback":
/// each 50ms callback banks 3 physics ticks rather than 1.
#[test]
fn physics_total_is_independent_of_frame_chunking() {
    let (mut ml, main_id) = scripted_loop();
    let mut driver = FrameDriver::new();

    // One second, delivered as 20 coarse 50ms frames (a slow device).
    let (steps, physics) = feed(&mut driver, &mut ml, 1.0, 20);

    assert_eq!(steps, 20, "21 callbacks - 1 origin = 20 engine steps");
    assert_eq!(
        physics, TPS,
        "1s at {TPS}Hz is {TPS} ticks however the frames were chunked"
    );
    assert!((driver.elapsed() - 1.0).abs() < 1e-6);

    // Fewer frames => fewer _process calls, but the same simulated second. This
    // is exactly Godot: _process is per-frame, physics is per-tick.
    let script = ml.tree().get_script(main_id).expect("script attached");
    assert_eq!(
        script.get_property("process_calls"),
        Some(Variant::Int(20)),
        "_process is per-frame, so coarse frames call it less often"
    );
}
