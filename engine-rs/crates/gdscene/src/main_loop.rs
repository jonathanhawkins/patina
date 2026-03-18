//! Main frame loop for deterministic scene execution.
//!
//! The [`MainLoop`] drives the scene tree through a Godot-compatible frame
//! loop with fixed-timestep physics and variable-timestep process callbacks.
//!
//! # Frame model
//!
//! Each call to [`MainLoop::step`] represents one visual frame:
//!
//! 1. **Physics phase** — the physics time accumulator is advanced by `delta`.
//!    While the accumulator holds at least one fixed-timestep tick, the tree
//!    receives [`NOTIFICATION_PHYSICS_PROCESS`](gdobject::NOTIFICATION_PHYSICS_PROCESS).
//!    At most `max_physics_steps_per_frame` ticks run per frame to avoid a
//!    spiral of death.
//! 2. **Process phase** — the tree receives
//!    [`NOTIFICATION_PROCESS`](gdobject::NOTIFICATION_PROCESS) exactly once.
//! 3. Frame counter and elapsed-time accumulators are updated.

use crate::scene_tree::SceneTree;

/// Drives a [`SceneTree`] through a deterministic frame loop.
///
/// # Example
///
/// ```
/// use gdscene::main_loop::MainLoop;
/// use gdscene::scene_tree::SceneTree;
///
/// let tree = SceneTree::new();
/// let mut main_loop = MainLoop::new(tree);
/// main_loop.step(1.0 / 60.0);
/// assert_eq!(main_loop.frame_count(), 1);
/// ```
#[derive(Debug)]
pub struct MainLoop {
    /// The scene tree being driven.
    tree: SceneTree,
    /// Number of physics ticks per second (default 60).
    physics_ticks_per_second: u32,
    /// Maximum number of physics steps allowed in a single frame (default 8).
    max_physics_steps_per_frame: u32,
    /// Total frames executed.
    frame_counter: u64,
    /// Total accumulated process (variable-timestep) time in seconds.
    process_time: f64,
    /// Total accumulated physics (fixed-timestep) time in seconds.
    physics_time: f64,
    /// Time left over from physics stepping, carried into the next frame.
    physics_accumulator: f64,
}

impl MainLoop {
    /// Creates a new `MainLoop` that owns the given [`SceneTree`].
    pub fn new(tree: SceneTree) -> Self {
        Self {
            tree,
            physics_ticks_per_second: 60,
            max_physics_steps_per_frame: 8,
            frame_counter: 0,
            process_time: 0.0,
            physics_time: 0.0,
            physics_accumulator: 0.0,
        }
    }

    /// Sets the number of physics ticks per second (default 60).
    pub fn set_physics_ticks_per_second(&mut self, tps: u32) {
        assert!(tps > 0, "physics_ticks_per_second must be > 0");
        self.physics_ticks_per_second = tps;
    }

    /// Returns the configured physics ticks per second.
    pub fn physics_ticks_per_second(&self) -> u32 {
        self.physics_ticks_per_second
    }

    /// Sets the maximum number of physics steps per frame (default 8).
    pub fn set_max_physics_steps_per_frame(&mut self, max_steps: u32) {
        assert!(max_steps > 0, "max_physics_steps_per_frame must be > 0");
        self.max_physics_steps_per_frame = max_steps;
    }

    /// Returns the maximum physics steps per frame.
    pub fn max_physics_steps_per_frame(&self) -> u32 {
        self.max_physics_steps_per_frame
    }

    /// Returns a reference to the owned [`SceneTree`].
    pub fn tree(&self) -> &SceneTree {
        &self.tree
    }

    /// Returns a mutable reference to the owned [`SceneTree`].
    pub fn tree_mut(&mut self) -> &mut SceneTree {
        &mut self.tree
    }

    /// Advances one frame by `delta_secs` seconds.
    ///
    /// 1. Accumulates physics time and runs fixed-timestep physics ticks
    ///    (dispatching `NOTIFICATION_PHYSICS_PROCESS`), up to
    ///    `max_physics_steps_per_frame`.
    /// 2. Dispatches `NOTIFICATION_PROCESS` once (variable timestep).
    /// 3. Increments the frame counter and elapsed time accumulators.
    pub fn step(&mut self, delta_secs: f64) {
        let physics_dt = 1.0 / self.physics_ticks_per_second as f64;

        // -- physics phase --
        self.physics_accumulator += delta_secs;
        let mut physics_steps = 0u32;
        while self.physics_accumulator >= physics_dt
            && physics_steps < self.max_physics_steps_per_frame
        {
            self.tree.process_physics_frame();
            self.physics_accumulator -= physics_dt;
            self.physics_time += physics_dt;
            physics_steps += 1;
        }

        // Clamp accumulator if we hit the step limit (spiral-of-death guard).
        if physics_steps >= self.max_physics_steps_per_frame {
            self.physics_accumulator = 0.0;
        }

        // -- process phase --
        self.tree.process_frame();

        // -- bookkeeping --
        self.process_time += delta_secs;
        self.frame_counter += 1;
    }

    /// Runs exactly `n` frames, each with the given `delta` seconds.
    ///
    /// This is the primary entry point for deterministic testing — the same
    /// `n` and `delta` values always produce the same results.
    pub fn run_frames(&mut self, n: u64, delta: f64) {
        for _ in 0..n {
            self.step(delta);
        }
    }

    /// Returns the total number of frames executed so far.
    pub fn frame_count(&self) -> u64 {
        self.frame_counter
    }

    /// Returns the total elapsed physics time in seconds.
    ///
    /// This only advances in fixed-timestep increments, so it may not
    /// exactly match wall-clock time.
    pub fn physics_time(&self) -> f64 {
        self.physics_time
    }

    /// Returns the total elapsed process time in seconds.
    ///
    /// This is the sum of all `delta_secs` values passed to [`step`](Self::step).
    pub fn process_time(&self) -> f64 {
        self.process_time
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node::Node;
    use gdobject::notification::{NOTIFICATION_PHYSICS_PROCESS, NOTIFICATION_PROCESS};

    /// Helper: build a MainLoop with a tree containing root + one child.
    fn make_loop_with_child() -> (MainLoop, crate::node::NodeId) {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let child = Node::new("Child", "Node");
        let child_id = tree.add_child(root, child).unwrap();
        (MainLoop::new(tree), child_id)
    }

    #[test]
    fn single_frame_increments_counter() {
        let (mut ml, _) = make_loop_with_child();
        assert_eq!(ml.frame_count(), 0);
        ml.step(1.0 / 60.0);
        assert_eq!(ml.frame_count(), 1);
    }

    #[test]
    fn ten_frames_increments_counter() {
        let (mut ml, _) = make_loop_with_child();
        ml.run_frames(10, 1.0 / 60.0);
        assert_eq!(ml.frame_count(), 10);
    }

    #[test]
    fn physics_process_fires_with_fixed_timestep() {
        let (mut ml, child_id) = make_loop_with_child();
        // At 60 TPS with delta = 1/60, each frame should produce exactly
        // one physics tick.
        ml.run_frames(5, 1.0 / 60.0);

        let log = ml.tree().get_node(child_id).unwrap().notification_log();
        let physics_count = log
            .iter()
            .filter(|&&n| n == NOTIFICATION_PHYSICS_PROCESS)
            .count();
        assert_eq!(physics_count, 5, "expected 5 physics ticks");
    }

    #[test]
    fn process_notification_dispatched_each_frame() {
        let (mut ml, child_id) = make_loop_with_child();
        ml.run_frames(3, 1.0 / 60.0);

        let log = ml.tree().get_node(child_id).unwrap().notification_log();
        let process_count = log
            .iter()
            .filter(|&&n| n == NOTIFICATION_PROCESS)
            .count();
        assert_eq!(process_count, 3, "expected 3 process calls");
    }

    #[test]
    fn delta_time_values_are_correct() {
        let (mut ml, _) = make_loop_with_child();
        let delta = 1.0 / 60.0;
        ml.run_frames(60, delta);

        // Process time should be ~1 second.
        let expected_process = 60.0 * delta;
        assert!(
            (ml.process_time() - expected_process).abs() < 1e-10,
            "process_time {} != expected {}",
            ml.process_time(),
            expected_process
        );

        // Physics time should also be ~1 second (60 ticks at 1/60).
        let expected_physics = 60.0 * (1.0 / 60.0);
        assert!(
            (ml.physics_time() - expected_physics).abs() < 1e-10,
            "physics_time {} != expected {}",
            ml.physics_time(),
            expected_physics
        );
    }

    #[test]
    fn deterministic_same_inputs_same_counts() {
        // Run two independent MainLoops with identical parameters and
        // verify they produce identical state.
        let run = || {
            let (mut ml, child_id) = make_loop_with_child();
            ml.run_frames(100, 1.0 / 60.0);
            let log = ml.tree().get_node(child_id).unwrap().notification_log().to_vec();
            (ml.frame_count(), ml.physics_time(), ml.process_time(), log)
        };

        let (fc1, pt1, prt1, log1) = run();
        let (fc2, pt2, prt2, log2) = run();

        assert_eq!(fc1, fc2);
        assert_eq!(pt1, pt2);
        assert_eq!(prt1, prt2);
        assert_eq!(log1, log2);
    }

    #[test]
    fn large_delta_caps_physics_steps() {
        let (mut ml, child_id) = make_loop_with_child();
        ml.set_max_physics_steps_per_frame(4);
        // A 1-second delta at 60 TPS would need 60 physics steps,
        // but the cap limits it to 4.
        ml.step(1.0);

        let log = ml.tree().get_node(child_id).unwrap().notification_log();
        let physics_count = log
            .iter()
            .filter(|&&n| n == NOTIFICATION_PHYSICS_PROCESS)
            .count();
        assert_eq!(physics_count, 4, "expected physics steps capped at 4");
    }

    #[test]
    fn notification_order_physics_then_process() {
        let (mut ml, child_id) = make_loop_with_child();
        ml.step(1.0 / 60.0);

        let log = ml.tree().get_node(child_id).unwrap().notification_log();
        // With delta = 1/60 at 60 TPS: one physics tick then one process.
        assert_eq!(log.len(), 2);
        assert_eq!(log[0], NOTIFICATION_PHYSICS_PROCESS);
        assert_eq!(log[1], NOTIFICATION_PROCESS);
    }
}
