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
use gdobject::notification::{NOTIFICATION_PAUSED, NOTIFICATION_UNPAUSED};

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
    /// Whether the main loop is currently paused.
    paused: bool,
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
            paused: false,
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

    /// Returns whether the loop is currently paused.
    pub fn paused(&self) -> bool {
        self.paused
    }

    /// Sets the paused state and dispatches pause transition notifications.
    pub fn set_paused(&mut self, paused: bool) {
        if self.paused == paused {
            return;
        }
        self.paused = paused;
        let notification = if paused {
            NOTIFICATION_PAUSED
        } else {
            NOTIFICATION_UNPAUSED
        };
        let ids = self.tree.all_nodes_in_tree_order();
        for id in ids {
            if let Some(node) = self.tree.get_node_mut(id) {
                node.receive_notification(notification);
            }
        }
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
        // Sync trace frame counter so trace events have the correct frame.
        self.tree.set_trace_frame(self.frame_counter);

        if self.paused {
            self.process_time += delta_secs;
            self.frame_counter += 1;
            return;
        }

        let physics_dt = 1.0 / self.physics_ticks_per_second as f64;

        // -- physics phase --
        // Godot per-tick order: INTERNAL_PHYSICS_PROCESS -> PHYSICS_PROCESS
        self.physics_accumulator += delta_secs;
        let mut physics_steps = 0u32;
        while self.physics_accumulator >= physics_dt
            && physics_steps < self.max_physics_steps_per_frame
        {
            self.tree.process_internal_physics_frame();
            self.tree.process_physics_frame();
            self.tree.process_all_scripts_physics_process(physics_dt);
            self.physics_accumulator -= physics_dt;
            self.physics_time += physics_dt;
            physics_steps += 1;
        }

        // Clamp accumulator if we hit the step limit (spiral-of-death guard).
        if physics_steps >= self.max_physics_steps_per_frame {
            self.physics_accumulator = 0.0;
        }

        // -- animation / tween phase --
        self.tree.process_animations(delta_secs);
        self.tree.process_tweens(delta_secs);

        // -- process phase --
        // Godot per-frame order: INTERNAL_PROCESS -> PROCESS
        self.tree.process_internal_frame();
        self.tree.process_frame();
        self.tree.process_all_scripts_process(delta_secs);

        // -- collision detection phase --
        // Run after scripts so collision properties are set before next frame's _process.
        self.tree.process_collisions();

        // -- deferred deletion phase --
        // Remove nodes that called queue_free() during this frame.
        self.tree.process_deletions();

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
    use gdobject::notification::{
        NOTIFICATION_INTERNAL_PHYSICS_PROCESS, NOTIFICATION_INTERNAL_PROCESS,
        NOTIFICATION_PHYSICS_PROCESS, NOTIFICATION_PROCESS,
    };

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
        let process_count = log.iter().filter(|&&n| n == NOTIFICATION_PROCESS).count();
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
            let log = ml
                .tree()
                .get_node(child_id)
                .unwrap()
                .notification_log()
                .to_vec();
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
    fn notification_order_godot_contract() {
        let (mut ml, child_id) = make_loop_with_child();
        ml.step(1.0 / 60.0);

        let log = ml.tree().get_node(child_id).unwrap().notification_log();
        // With delta = 1/60 at 60 TPS: Godot fires in this order per frame:
        // INTERNAL_PHYSICS_PROCESS -> PHYSICS_PROCESS -> INTERNAL_PROCESS -> PROCESS
        assert_eq!(log.len(), 4);
        assert_eq!(log[0], NOTIFICATION_INTERNAL_PHYSICS_PROCESS);
        assert_eq!(log[1], NOTIFICATION_PHYSICS_PROCESS);
        assert_eq!(log[2], NOTIFICATION_INTERNAL_PROCESS);
        assert_eq!(log[3], NOTIFICATION_PROCESS);
    }

    #[test]
    fn step_with_delta_zero() {
        let (mut ml, child_id) = make_loop_with_child();
        ml.step(0.0);

        assert_eq!(ml.frame_count(), 1);
        assert_eq!(ml.process_time(), 0.0);

        // With delta=0, no physics ticks should fire, but process phase fires
        let log = ml.tree().get_node(child_id).unwrap().notification_log();
        let physics_count = log
            .iter()
            .filter(|&&n| n == NOTIFICATION_PHYSICS_PROCESS)
            .count();
        let internal_physics_count = log
            .iter()
            .filter(|&&n| n == NOTIFICATION_INTERNAL_PHYSICS_PROCESS)
            .count();
        let process_count = log.iter().filter(|&&n| n == NOTIFICATION_PROCESS).count();
        let internal_process_count = log
            .iter()
            .filter(|&&n| n == NOTIFICATION_INTERNAL_PROCESS)
            .count();
        assert_eq!(physics_count, 0);
        assert_eq!(internal_physics_count, 0);
        assert_eq!(process_count, 1);
        assert_eq!(internal_process_count, 1);
    }

    #[test]
    fn step_with_very_small_delta() {
        let (mut ml, child_id) = make_loop_with_child();
        // Much smaller than physics timestep (1/60)
        ml.step(1e-10);

        assert_eq!(ml.frame_count(), 1);

        let log = ml.tree().get_node(child_id).unwrap().notification_log();
        let physics_count = log
            .iter()
            .filter(|&&n| n == NOTIFICATION_PHYSICS_PROCESS)
            .count();
        let process_count = log.iter().filter(|&&n| n == NOTIFICATION_PROCESS).count();
        // Delta too small for any physics step
        assert_eq!(physics_count, 0);
        assert_eq!(process_count, 1);
    }

    #[test]
    fn run_frames_zero() {
        let (mut ml, child_id) = make_loop_with_child();
        ml.run_frames(0, 1.0 / 60.0);

        assert_eq!(ml.frame_count(), 0);
        let log = ml.tree().get_node(child_id).unwrap().notification_log();
        assert!(log.is_empty());
    }

    #[test]
    fn physics_ticks_per_second_accessor() {
        let (ml, _) = make_loop_with_child();
        assert_eq!(ml.physics_ticks_per_second(), 60);
    }

    #[test]
    fn max_physics_steps_per_frame_accessor() {
        let (ml, _) = make_loop_with_child();
        assert_eq!(ml.max_physics_steps_per_frame(), 8);
    }

    #[test]
    fn paused_accessor_defaults_false() {
        let (ml, _) = make_loop_with_child();
        assert!(!ml.paused());
    }

    #[test]
    fn paused_step_skips_process_and_physics_notifications() {
        let (mut ml, child_id) = make_loop_with_child();
        ml.set_paused(true);
        ml.step(1.0 / 60.0);

        let log = ml.tree().get_node(child_id).unwrap().notification_log();
        assert!(log.contains(&NOTIFICATION_PAUSED));
        assert!(!log.contains(&NOTIFICATION_PROCESS));
        assert!(!log.contains(&NOTIFICATION_PHYSICS_PROCESS));
        assert_eq!(ml.frame_count(), 1);
        assert_eq!(ml.physics_time(), 0.0);
    }

    #[test]
    fn unpausing_restores_frame_processing() {
        let (mut ml, child_id) = make_loop_with_child();
        ml.set_paused(true);
        ml.step(1.0 / 60.0);
        ml.set_paused(false);
        ml.step(1.0 / 60.0);

        let log = ml.tree().get_node(child_id).unwrap().notification_log();
        assert!(log.contains(&NOTIFICATION_PAUSED));
        assert!(log.contains(&NOTIFICATION_UNPAUSED));
        assert!(log.contains(&NOTIFICATION_PROCESS));
        assert!(log.contains(&NOTIFICATION_PHYSICS_PROCESS));
    }

    #[test]
    fn set_physics_ticks_per_second() {
        let (mut ml, _) = make_loop_with_child();
        ml.set_physics_ticks_per_second(120);
        assert_eq!(ml.physics_ticks_per_second(), 120);
    }

    #[test]
    #[should_panic(expected = "physics_ticks_per_second must be > 0")]
    fn set_physics_ticks_per_second_zero_panics() {
        let (mut ml, _) = make_loop_with_child();
        ml.set_physics_ticks_per_second(0);
    }

    #[test]
    #[should_panic(expected = "max_physics_steps_per_frame must be > 0")]
    fn set_max_physics_steps_zero_panics() {
        let (mut ml, _) = make_loop_with_child();
        ml.set_max_physics_steps_per_frame(0);
    }

    #[test]
    fn tree_mut_access() {
        let (mut ml, _) = make_loop_with_child();
        let child = Node::new("Extra", "Node");
        let root = ml.tree().root_id();
        ml.tree_mut().add_child(root, child).unwrap();
        assert_eq!(ml.tree().node_count(), 3); // root + Child + Extra
    }

    #[test]
    fn multiple_physics_steps_in_one_frame() {
        let (mut ml, child_id) = make_loop_with_child();
        // With delta = 2/60, at 60 TPS we should get 2 physics steps
        ml.step(2.0 / 60.0);

        let log = ml.tree().get_node(child_id).unwrap().notification_log();
        let physics_count = log
            .iter()
            .filter(|&&n| n == NOTIFICATION_PHYSICS_PROCESS)
            .count();
        assert_eq!(physics_count, 2);
    }

    #[test]
    fn accumulator_carries_over_between_frames() {
        let (mut ml, child_id) = make_loop_with_child();
        // At 60 TPS, physics dt = 1/60 ~= 0.01667
        // Step with half the physics dt twice — accumulator should carry over
        let half_dt = 0.5 / 60.0;
        ml.step(half_dt);
        ml.step(half_dt);

        let log = ml.tree().get_node(child_id).unwrap().notification_log();
        let physics_count = log
            .iter()
            .filter(|&&n| n == NOTIFICATION_PHYSICS_PROCESS)
            .count();
        // The first step accumulates half, second step accumulates to full => 1 physics step
        assert_eq!(physics_count, 1);
        assert_eq!(ml.frame_count(), 2);
    }

    // ── Bead B007: Godot frame-contract tests ───────────────────────────

    /// Helper: builds a tree with root -> Parent -> Child hierarchy.
    fn make_loop_with_parent_child() -> (MainLoop, crate::node::NodeId, crate::node::NodeId) {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let parent = Node::new("Parent", "Node");
        let parent_id = tree.add_child(root, parent).unwrap();
        let child = Node::new("Child", "Node");
        let child_id = tree.add_child(parent_id, child).unwrap();
        (MainLoop::new(tree), parent_id, child_id)
    }

    #[test]
    fn godot_four_notification_order_per_frame() {
        // Godot contract: each frame with one physics tick fires exactly
        // INTERNAL_PHYSICS -> PHYSICS -> INTERNAL_PROCESS -> PROCESS
        let (mut ml, child_id) = make_loop_with_child();
        ml.step(1.0 / 60.0);

        let log = ml.tree().get_node(child_id).unwrap().notification_log();
        assert_eq!(
            log,
            &[
                NOTIFICATION_INTERNAL_PHYSICS_PROCESS,
                NOTIFICATION_PHYSICS_PROCESS,
                NOTIFICATION_INTERNAL_PROCESS,
                NOTIFICATION_PROCESS,
            ]
        );
    }

    #[test]
    fn process_fires_after_physics_process_every_frame() {
        let (mut ml, child_id) = make_loop_with_child();
        ml.run_frames(5, 1.0 / 60.0);

        let log = ml.tree().get_node(child_id).unwrap().notification_log();
        // Every PROCESS should come after the last PHYSICS_PROCESS in each frame.
        // With 1 physics tick per frame, the pattern repeats every 4 notifications.
        for frame in 0..5 {
            let base = frame * 4;
            assert_eq!(
                log[base], NOTIFICATION_INTERNAL_PHYSICS_PROCESS,
                "frame {frame}: expected INTERNAL_PHYSICS_PROCESS at pos {base}"
            );
            assert_eq!(
                log[base + 1],
                NOTIFICATION_PHYSICS_PROCESS,
                "frame {frame}: expected PHYSICS_PROCESS at pos {}",
                base + 1
            );
            assert_eq!(
                log[base + 2],
                NOTIFICATION_INTERNAL_PROCESS,
                "frame {frame}: expected INTERNAL_PROCESS at pos {}",
                base + 2
            );
            assert_eq!(
                log[base + 3],
                NOTIFICATION_PROCESS,
                "frame {frame}: expected PROCESS at pos {}",
                base + 3
            );
        }
    }

    #[test]
    fn parent_processes_before_child_in_tree_order() {
        // Godot processes parent before child (depth-first, top-down).
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let parent = Node::new("Parent", "Node");
        let parent_id = tree.add_child(root, parent).unwrap();
        let child = Node::new("Child", "Node");
        let child_id = tree.add_child(parent_id, child).unwrap();

        let mut ml = MainLoop::new(tree);
        ml.step(1.0 / 60.0);

        // Both nodes should receive the same sequence. Since tree order is
        // root -> Parent -> Child, Parent receives each notification before Child.
        let parent_log = ml.tree().get_node(parent_id).unwrap().notification_log();
        let child_log = ml.tree().get_node(child_id).unwrap().notification_log();

        // Both must have the full set of notifications.
        assert_eq!(parent_log.len(), 4);
        assert_eq!(child_log.len(), 4);

        // Verify tree order via all_nodes_in_tree_order.
        let order = ml.tree().all_nodes_in_tree_order();
        let parent_pos = order.iter().position(|&id| id == parent_id).unwrap();
        let child_pos = order.iter().position(|&id| id == child_id).unwrap();
        assert!(
            parent_pos < child_pos,
            "parent (pos {parent_pos}) must be processed before child (pos {child_pos})"
        );
    }

    #[test]
    fn physics_delta_is_fixed_timestep() {
        // Physics delta should always be 1/physics_ticks_per_second,
        // regardless of the frame delta passed to step().
        let (mut ml, _) = make_loop_with_child();
        let physics_dt = 1.0 / ml.physics_ticks_per_second() as f64;

        // Run with non-standard delta (e.g. 2/60 => 2 physics ticks per frame).
        ml.step(2.0 / 60.0);

        // Each physics tick should advance physics_time by exactly physics_dt.
        assert!(
            (ml.physics_time() - 2.0 * physics_dt).abs() < 1e-12,
            "physics_time {} != expected {}",
            ml.physics_time(),
            2.0 * physics_dt
        );
    }

    #[test]
    fn process_delta_is_variable_timestep() {
        // Process time should exactly track the delta passed to step().
        let (mut ml, _) = make_loop_with_child();
        let variable_delta = 0.05; // 50ms — not aligned to physics tick
        ml.step(variable_delta);

        assert!(
            (ml.process_time() - variable_delta).abs() < 1e-12,
            "process_time {} != expected {}",
            ml.process_time(),
            variable_delta
        );
    }

    #[test]
    fn ten_frame_property_evolution_trace() {
        // Run 10 frames and verify per-frame notification accumulation is correct
        // at each step — not just at the end.
        let (mut ml, child_id) = make_loop_with_child();
        let delta = 1.0 / 60.0;

        for frame in 1..=10u64 {
            ml.step(delta);

            assert_eq!(ml.frame_count(), frame);

            let log = ml.tree().get_node(child_id).unwrap().notification_log();
            // Each frame adds 4 notifications (with 1 physics tick per frame).
            let expected_len = (frame as usize) * 4;
            assert_eq!(
                log.len(),
                expected_len,
                "frame {frame}: expected {expected_len} notifications, got {}",
                log.len()
            );

            // Physics process count = frame number
            let physics_count = log
                .iter()
                .filter(|&&n| n == NOTIFICATION_PHYSICS_PROCESS)
                .count();
            assert_eq!(
                physics_count, frame as usize,
                "frame {frame}: physics_count"
            );

            // Process count = frame number
            let process_count = log.iter().filter(|&&n| n == NOTIFICATION_PROCESS).count();
            assert_eq!(
                process_count, frame as usize,
                "frame {frame}: process_count"
            );

            // Cumulative time checks.
            let expected_process_time = frame as f64 * delta;
            assert!(
                (ml.process_time() - expected_process_time).abs() < 1e-10,
                "frame {frame}: process_time {} != {expected_process_time}",
                ml.process_time()
            );
        }
    }

    #[test]
    fn physics_accumulator_non_integer_frame_ratio() {
        // Run at 30 FPS with 60 TPS physics — each frame's delta (1/30)
        // holds exactly 2 physics ticks (1/30 / 1/60 = 2). No accumulator
        // remainder.
        let (mut ml, child_id) = make_loop_with_child();
        ml.run_frames(5, 1.0 / 30.0);

        let log = ml.tree().get_node(child_id).unwrap().notification_log();
        let physics_count = log
            .iter()
            .filter(|&&n| n == NOTIFICATION_PHYSICS_PROCESS)
            .count();
        assert_eq!(physics_count, 10, "5 frames * 2 physics ticks = 10");
        assert_eq!(ml.frame_count(), 5);
    }

    #[test]
    fn physics_accumulator_fractional_ratio() {
        // 50 FPS rendering with 60 TPS physics: delta=1/50=0.02, physics_dt=1/60≈0.01667
        // Frame 1: acc=0.02, 1 tick (acc=0.02-0.01667=0.00333)
        // Frame 2: acc=0.02333, 1 tick (acc=0.00667)
        // Frame 3: acc=0.02667, 1 tick (acc=0.01)
        // Frame 4: acc=0.03, 1 tick (acc=0.01333)
        // Frame 5: acc=0.03333, 2 ticks (acc=0.03333-0.03333=0.0)
        // Total: 6 physics ticks in 5 frames
        let (mut ml, child_id) = make_loop_with_child();
        ml.run_frames(5, 1.0 / 50.0);

        let log = ml.tree().get_node(child_id).unwrap().notification_log();
        let physics_count = log
            .iter()
            .filter(|&&n| n == NOTIFICATION_PHYSICS_PROCESS)
            .count();
        assert_eq!(
            physics_count, 6,
            "5 frames at 50fps with 60tps should give 6 physics ticks"
        );
    }

    #[test]
    fn frame_count_matches_run_frames() {
        let (mut ml, _) = make_loop_with_child();
        ml.run_frames(42, 1.0 / 60.0);
        assert_eq!(ml.frame_count(), 42);
    }

    #[test]
    fn frame_count_matches_manual_steps() {
        let (mut ml, _) = make_loop_with_child();
        for _ in 0..17 {
            ml.step(1.0 / 60.0);
        }
        assert_eq!(ml.frame_count(), 17);
    }

    #[test]
    fn internal_notifications_also_in_tree_order() {
        // Internal notifications must also respect parent-before-child order.
        let (mut ml, parent_id, child_id) = make_loop_with_parent_child();
        ml.step(1.0 / 60.0);

        // Verify both parent and child get internal notifications.
        let parent_log = ml.tree().get_node(parent_id).unwrap().notification_log();
        let child_log = ml.tree().get_node(child_id).unwrap().notification_log();

        assert!(parent_log.contains(&NOTIFICATION_INTERNAL_PHYSICS_PROCESS));
        assert!(parent_log.contains(&NOTIFICATION_INTERNAL_PROCESS));
        assert!(child_log.contains(&NOTIFICATION_INTERNAL_PHYSICS_PROCESS));
        assert!(child_log.contains(&NOTIFICATION_INTERNAL_PROCESS));
    }

    #[test]
    fn high_tps_caps_at_max_steps() {
        // With 1000 TPS, delta=1/60 would need ~16 physics ticks, but
        // max_physics_steps_per_frame (default 8) caps it.
        let (mut ml, child_id) = make_loop_with_child();
        ml.set_physics_ticks_per_second(1000);
        ml.step(1.0 / 60.0);

        let log = ml.tree().get_node(child_id).unwrap().notification_log();
        let physics_count = log
            .iter()
            .filter(|&&n| n == NOTIFICATION_PHYSICS_PROCESS)
            .count();
        assert_eq!(physics_count, 8, "capped at max_physics_steps_per_frame");
    }

    #[test]
    fn high_tps_uncapped_when_limit_raised() {
        // With limit raised to 20, 1000 TPS at 1/60 delta gives 16 ticks.
        let (mut ml, child_id) = make_loop_with_child();
        ml.set_physics_ticks_per_second(1000);
        ml.set_max_physics_steps_per_frame(20);
        ml.step(1.0 / 60.0);

        let log = ml.tree().get_node(child_id).unwrap().notification_log();
        let physics_count = log
            .iter()
            .filter(|&&n| n == NOTIFICATION_PHYSICS_PROCESS)
            .count();
        assert_eq!(physics_count, 16, "floor(1/60 / 1/1000) = 16");
    }

    #[test]
    fn internal_physics_count_matches_user_physics_count() {
        // Each physics tick should dispatch both internal and user physics.
        let (mut ml, child_id) = make_loop_with_child();
        ml.run_frames(7, 1.0 / 60.0);

        let log = ml.tree().get_node(child_id).unwrap().notification_log();
        let internal_phys = log
            .iter()
            .filter(|&&n| n == NOTIFICATION_INTERNAL_PHYSICS_PROCESS)
            .count();
        let user_phys = log
            .iter()
            .filter(|&&n| n == NOTIFICATION_PHYSICS_PROCESS)
            .count();
        assert_eq!(
            internal_phys, user_phys,
            "internal and user physics counts must match"
        );
    }

    #[test]
    fn internal_process_count_matches_user_process_count() {
        // Each frame dispatches both internal and user process exactly once.
        let (mut ml, child_id) = make_loop_with_child();
        ml.run_frames(7, 1.0 / 60.0);

        let log = ml.tree().get_node(child_id).unwrap().notification_log();
        let internal_proc = log
            .iter()
            .filter(|&&n| n == NOTIFICATION_INTERNAL_PROCESS)
            .count();
        let user_proc = log.iter().filter(|&&n| n == NOTIFICATION_PROCESS).count();
        assert_eq!(internal_proc, 7);
        assert_eq!(user_proc, 7);
        assert_eq!(internal_proc, user_proc);
    }

    #[test]
    fn process_count_equals_frame_count() {
        let (mut ml, child_id) = make_loop_with_child();
        ml.run_frames(20, 1.0 / 60.0);

        let log = ml.tree().get_node(child_id).unwrap().notification_log();
        let process_count = log.iter().filter(|&&n| n == NOTIFICATION_PROCESS).count();
        assert_eq!(process_count as u64, ml.frame_count());
    }

    #[test]
    fn deep_tree_processes_in_correct_order() {
        // Build root -> A -> B -> C and verify tree-order processing.
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let a = Node::new("A", "Node");
        let a_id = tree.add_child(root, a).unwrap();
        let b = Node::new("B", "Node");
        let b_id = tree.add_child(a_id, b).unwrap();
        let c = Node::new("C", "Node");
        let c_id = tree.add_child(b_id, c).unwrap();

        let mut ml = MainLoop::new(tree);
        ml.step(1.0 / 60.0);

        let order = ml.tree().all_nodes_in_tree_order();
        let a_pos = order.iter().position(|&id| id == a_id).unwrap();
        let b_pos = order.iter().position(|&id| id == b_id).unwrap();
        let c_pos = order.iter().position(|&id| id == c_id).unwrap();
        assert!(a_pos < b_pos, "A before B");
        assert!(b_pos < c_pos, "B before C");

        // Each node gets the full notification set.
        for &id in &[a_id, b_id, c_id] {
            let log = ml.tree().get_node(id).unwrap().notification_log();
            assert_eq!(log.len(), 4);
        }
    }

    #[test]
    fn sibling_order_preserved() {
        // root -> A, root -> B: A should process before B (insertion order).
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let a = Node::new("A", "Node");
        let a_id = tree.add_child(root, a).unwrap();
        let b = Node::new("B", "Node");
        let b_id = tree.add_child(root, b).unwrap();

        let ml = MainLoop::new(tree);
        let order = ml.tree().all_nodes_in_tree_order();
        let a_pos = order.iter().position(|&id| id == a_id).unwrap();
        let b_pos = order.iter().position(|&id| id == b_id).unwrap();
        assert!(a_pos < b_pos, "sibling A before sibling B");
    }

    #[test]
    fn call_deferred_stub_does_not_panic() {
        // The call_deferred stub should accept calls without panicking.
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        tree.call_deferred(root, "some_method", &[]);
    }
}
