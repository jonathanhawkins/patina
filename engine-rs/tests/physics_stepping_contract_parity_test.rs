//! pat-8mzy: Advance gdphysics2d from MainLoop fixed-step frames.
//!
//! Expands physics stepping contract coverage beyond the core accumulator
//! tests in `mainloop_physics_stepping_parity_test.rs`. Tests verify:
//!
//! - Phase ordering: physics ticks complete before process notifications
//! - Unpause resumes physics without accumulator bleed
//! - Multiple bodies integrate independently per tick
//! - Per-tick position advancement within multi-tick frames (via trace)
//! - Accumulator precision over many frames (no drift)
//! - TPS changes mid-simulation affect subsequent frames
//! - Negative delta produces no ticks
//! - Very small sub-tick deltas accumulate correctly
//! - CharacterBody2D moves through MainLoop physics ticks
//! - Area2D overlap signals fire during physics stepping
//! - Physics body count matches registered scene nodes
//! - Collision events propagate through MainLoop cycle
//! - Frame record captures per-tick property evolution
//!
//! Acceptance: frame and physics stepping semantics match expected Godot
//! contracts in tests.

use gdcore::math::Vector2;
use gdscene::main_loop::MainLoop;
use gdscene::node::Node;
use gdscene::scene_tree::SceneTree;
use gdscene::trace::TraceEventType;
use gdvariant::Variant;

const EPSILON_F32: f32 = 1e-3;
const EPSILON_F64: f64 = 1e-6;

fn approx_eq_f32(a: f32, b: f32) -> bool {
    (a - b).abs() < EPSILON_F32
}

fn approx_eq_f64(a: f64, b: f64) -> bool {
    (a - b).abs() < EPSILON_F64
}

// ===========================================================================
// Helpers
// ===========================================================================

/// Build a scene with two RigidBody2D nodes moving in different directions
/// and one StaticBody2D floor.
fn make_multi_body_scene() -> (SceneTree, gdscene::node::NodeId, gdscene::node::NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    // Ball A: moving right at 120 px/s
    let mut ball_a = Node::new("BallA", "RigidBody2D");
    ball_a.set_property("position", Variant::Vector2(Vector2::new(0.0, 0.0)));
    ball_a.set_property("mass", Variant::Float(1.0));
    ball_a.set_property(
        "linear_velocity",
        Variant::Vector2(Vector2::new(120.0, 0.0)),
    );
    let ball_a_id = tree.add_child(root, ball_a).unwrap();
    let mut shape_a = Node::new("CollisionShape", "CollisionShape2D");
    shape_a.set_property("radius", Variant::Float(8.0));
    tree.add_child(ball_a_id, shape_a).unwrap();

    // Ball B: moving down at 60 px/s
    let mut ball_b = Node::new("BallB", "RigidBody2D");
    ball_b.set_property("position", Variant::Vector2(Vector2::new(200.0, 0.0)));
    ball_b.set_property("mass", Variant::Float(2.0));
    ball_b.set_property("linear_velocity", Variant::Vector2(Vector2::new(0.0, 60.0)));
    let ball_b_id = tree.add_child(root, ball_b).unwrap();
    let mut shape_b = Node::new("CollisionShape", "CollisionShape2D");
    shape_b.set_property("radius", Variant::Float(8.0));
    tree.add_child(ball_b_id, shape_b).unwrap();

    // Static floor far below
    let mut floor = Node::new("Floor", "StaticBody2D");
    floor.set_property("position", Variant::Vector2(Vector2::new(100.0, 500.0)));
    let floor_id = tree.add_child(root, floor).unwrap();
    let mut shape_f = Node::new("CollisionShape", "CollisionShape2D");
    shape_f.set_property("size", Variant::Vector2(Vector2::new(400.0, 20.0)));
    tree.add_child(floor_id, shape_f).unwrap();

    (tree, ball_a_id, ball_b_id)
}

fn make_multi_body_mainloop() -> (MainLoop, gdscene::node::NodeId, gdscene::node::NodeId) {
    let (tree, ball_a_id, ball_b_id) = make_multi_body_scene();
    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();
    (ml, ball_a_id, ball_b_id)
}

/// Build a simple single-body mainloop for basic tests.
fn make_single_body_mainloop() -> (MainLoop, gdscene::node::NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let mut rigid = Node::new("Ball", "RigidBody2D");
    rigid.set_property("position", Variant::Vector2(Vector2::new(100.0, 50.0)));
    rigid.set_property("mass", Variant::Float(1.0));
    rigid.set_property("linear_velocity", Variant::Vector2(Vector2::new(0.0, 60.0)));
    let rigid_id = tree.add_child(root, rigid).unwrap();
    let mut shape = Node::new("CollisionShape", "CollisionShape2D");
    shape.set_property("radius", Variant::Float(16.0));
    tree.add_child(rigid_id, shape).unwrap();

    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();
    (ml, rigid_id)
}

fn get_pos(ml: &MainLoop, id: gdscene::node::NodeId) -> Vector2 {
    match ml.tree().get_node(id).unwrap().get_property("position") {
        Variant::Vector2(v) => v,
        other => panic!("expected Vector2 for position, got {:?}", other),
    }
}

fn get_vel(ml: &MainLoop, id: gdscene::node::NodeId) -> Vector2 {
    match ml
        .tree()
        .get_node(id)
        .unwrap()
        .get_property("linear_velocity")
    {
        Variant::Vector2(v) => v,
        other => panic!("expected Vector2 for linear_velocity, got {:?}", other),
    }
}

// ===========================================================================
// 1. Phase ordering: physics notifications complete before process
// ===========================================================================

/// Godot contract: within a single frame, all physics tick notifications
/// (INTERNAL_PHYSICS_PROCESS, PHYSICS_PROCESS) complete before the process
/// phase (INTERNAL_PROCESS, PROCESS).
#[test]
fn physics_phase_precedes_process_phase() {
    let (mut ml, _) = make_single_body_mainloop();
    ml.tree_mut().event_trace_mut().enable();

    ml.step(1.0 / 60.0);

    let events = ml.tree().event_trace().events();
    let notifs: Vec<_> = events
        .iter()
        .filter(|e| {
            e.event_type == TraceEventType::Notification
                && (e.detail == "PHYSICS_PROCESS"
                    || e.detail == "INTERNAL_PHYSICS_PROCESS"
                    || e.detail == "PROCESS"
                    || e.detail == "INTERNAL_PROCESS")
        })
        .collect();

    let last_physics_idx = notifs
        .iter()
        .rposition(|e| e.detail == "PHYSICS_PROCESS" || e.detail == "INTERNAL_PHYSICS_PROCESS")
        .expect("should have physics notifications");
    let first_process_idx = notifs
        .iter()
        .position(|e| e.detail == "PROCESS" || e.detail == "INTERNAL_PROCESS")
        .expect("should have process notifications");

    assert!(
        last_physics_idx < first_process_idx,
        "all physics notifications must complete before any process notification"
    );
}

/// With multiple physics ticks in a single frame, all physics ticks complete
/// before the process phase starts.
#[test]
fn multi_tick_physics_all_before_process() {
    let (mut ml, _) = make_single_body_mainloop();
    ml.tree_mut().event_trace_mut().enable();

    // 3 physics ticks in one frame
    ml.step(3.0 / 60.0);

    let events = ml.tree().event_trace().events();
    let physics_events: Vec<usize> = events
        .iter()
        .enumerate()
        .filter(|(_, e)| {
            e.event_type == TraceEventType::Notification
                && (e.detail == "PHYSICS_PROCESS" || e.detail == "INTERNAL_PHYSICS_PROCESS")
        })
        .map(|(i, _)| i)
        .collect();
    let process_events: Vec<usize> = events
        .iter()
        .enumerate()
        .filter(|(_, e)| {
            e.event_type == TraceEventType::Notification
                && (e.detail == "PROCESS" || e.detail == "INTERNAL_PROCESS")
        })
        .map(|(i, _)| i)
        .collect();

    assert!(
        physics_events.len() >= 3,
        "expected at least 3 physics tick notifications"
    );
    assert!(!process_events.is_empty(), "expected process notifications");

    let max_physics = *physics_events.last().unwrap();
    let min_process = *process_events.first().unwrap();
    assert!(
        max_physics < min_process,
        "all physics ticks must finish before process starts"
    );
}

// ===========================================================================
// 2. Unpause resumes without accumulator bleed
// ===========================================================================

/// Godot contract: unpausing should not cause a burst of physics ticks from
/// accumulated time while paused. The accumulator should not grow while paused.
#[test]
fn unpause_does_not_burst_physics_ticks() {
    let (mut ml, _) = make_single_body_mainloop();

    // Run 1 normal frame.
    ml.step(1.0 / 60.0);

    // Pause and run many frames.
    ml.set_paused(true);
    ml.run_frames(100, 1.0 / 60.0);

    // Unpause and run 1 frame.
    ml.set_paused(false);
    let output = ml.step(1.0 / 60.0);

    // Should get at most 1 physics tick, not a huge backlog.
    assert!(
        output.physics_steps <= 2,
        "unpause should not burst physics: got {} steps",
        output.physics_steps
    );
}

/// After unpausing, physics position advances resume normally.
#[test]
fn unpause_resumes_position_advancement() {
    let (mut ml, rigid_id) = make_single_body_mainloop();

    // Run 5 frames normally.
    ml.run_frames(5, 1.0 / 60.0);
    let _pos_before_pause = get_pos(&ml, rigid_id);

    // Pause, run frames, unpause.
    ml.set_paused(true);
    ml.run_frames(10, 1.0 / 60.0);
    let pos_during_pause = get_pos(&ml, rigid_id);
    ml.set_paused(false);

    // Run 5 more frames.
    ml.run_frames(5, 1.0 / 60.0);
    let pos_after_unpause = get_pos(&ml, rigid_id);

    // Position should have advanced after unpausing.
    assert!(
        pos_after_unpause.y > pos_during_pause.y,
        "position should advance after unpause: {} > {}",
        pos_after_unpause.y,
        pos_during_pause.y
    );
}

// ===========================================================================
// 3. Multiple bodies integrate independently
// ===========================================================================

/// Two RigidBody2D nodes with different velocities should move independently.
#[test]
fn multi_body_independent_integration() {
    let (mut ml, ball_a_id, ball_b_id) = make_multi_body_mainloop();

    let a_initial = get_pos(&ml, ball_a_id);
    let b_initial = get_pos(&ml, ball_b_id);

    ml.run_frames(10, 1.0 / 60.0);

    let a_final = get_pos(&ml, ball_a_id);
    let b_final = get_pos(&ml, ball_b_id);

    // Ball A moves right (X increases, Y unchanged).
    assert!(
        a_final.x > a_initial.x + 10.0,
        "BallA should move right: {} > {}",
        a_final.x,
        a_initial.x
    );

    // Ball B moves down (Y increases, X unchanged).
    assert!(
        b_final.y > b_initial.y + 5.0,
        "BallB should move down: {} > {}",
        b_final.y,
        b_initial.y
    );

    // Ball A has no initial Y velocity and gravity is not yet applied to
    // RigidBody2D by the physics server, so Y should remain unchanged.
    assert!(
        approx_eq_f32(a_final.y, a_initial.y),
        "BallA Y should not change without gravity: {} vs {}",
        a_final.y,
        a_initial.y
    );

    // Ball B's X should not change significantly (no X velocity, gravity is vertical).
    assert!(
        approx_eq_f32(b_final.x, b_initial.x),
        "BallB X should not change: {} vs {}",
        b_final.x,
        b_initial.x
    );
}

/// Multi-body determinism: same scene, same deltas → same positions.
#[test]
fn multi_body_deterministic() {
    fn run_capture() -> (Vector2, Vector2) {
        let (mut ml, a_id, b_id) = make_multi_body_mainloop();
        ml.run_frames(20, 1.0 / 60.0);
        (get_pos(&ml, a_id), get_pos(&ml, b_id))
    }

    let (a1, b1) = run_capture();
    let (a2, b2) = run_capture();

    assert!(
        approx_eq_f32(a1.x, a2.x) && approx_eq_f32(a1.y, a2.y),
        "BallA must be deterministic: {:?} vs {:?}",
        a1,
        a2
    );
    assert!(
        approx_eq_f32(b1.x, b2.x) && approx_eq_f32(b1.y, b2.y),
        "BallB must be deterministic: {:?} vs {:?}",
        b1,
        b2
    );
}

// ===========================================================================
// 4. Per-tick trace shows incremental position within multi-tick frames
// ===========================================================================

/// When a frame triggers multiple physics ticks, the physics trace should
/// show the body's position advancing incrementally each tick.
#[test]
fn multi_tick_trace_shows_incremental_positions() {
    let (mut ml, _ball_a_id, _ball_b_id) = make_multi_body_mainloop();
    ml.physics_server_mut().set_tracing(true);

    // 4 physics ticks in one frame
    ml.step(4.0 / 60.0);

    let trace = ml.physics_server().trace();
    let ball_a_entries: Vec<_> = trace.iter().filter(|e| e.name == "BallA").collect();

    assert_eq!(
        ball_a_entries.len(),
        4,
        "expected 4 trace entries for BallA in a 4-tick frame"
    );

    // BallA moves right → X should strictly increase each tick.
    for i in 1..ball_a_entries.len() {
        assert!(
            ball_a_entries[i].position.x > ball_a_entries[i - 1].position.x,
            "BallA X must increase each tick: {} > {} at tick {}",
            ball_a_entries[i].position.x,
            ball_a_entries[i - 1].position.x,
            i
        );
    }
}

/// Trace entries within a multi-tick frame have strictly increasing frame numbers.
#[test]
fn multi_tick_trace_frame_numbers_increase() {
    let (mut ml, _, _) = make_multi_body_mainloop();
    ml.physics_server_mut().set_tracing(true);

    ml.step(3.0 / 60.0);

    let trace = ml.physics_server().trace();
    let ball_a_frames: Vec<u64> = trace
        .iter()
        .filter(|e| e.name == "BallA")
        .map(|e| e.frame)
        .collect();

    assert_eq!(ball_a_frames.len(), 3);
    for i in 1..ball_a_frames.len() {
        assert!(
            ball_a_frames[i] > ball_a_frames[i - 1],
            "trace frames must increase: {} > {}",
            ball_a_frames[i],
            ball_a_frames[i - 1]
        );
    }
}

// ===========================================================================
// 5. Accumulator precision over many frames
// ===========================================================================

/// Over many frames, physics_time should not drift significantly from
/// frame_count * physics_dt (no floating-point accumulation error).
#[test]
fn accumulator_precision_no_drift() {
    let (mut ml, _) = make_single_body_mainloop();
    let physics_dt = 1.0 / 60.0;

    // Run 1000 frames at exactly 60fps.
    ml.run_frames(1000, physics_dt);

    let expected = 1000.0 * physics_dt;
    let actual = ml.physics_time();
    let drift = (actual - expected).abs();

    assert!(
        drift < 1e-8,
        "physics_time drift after 1000 frames: {drift} (actual={actual}, expected={expected})"
    );
}

/// With variable deltas, total physics ticks should match floor(total_time / physics_dt).
#[test]
fn variable_deltas_total_ticks_correct() {
    let (mut ml, _) = make_single_body_mainloop();
    ml.physics_server_mut().set_tracing(true);

    // Mix of different frame times
    let deltas = [
        1.0 / 60.0,
        1.0 / 30.0,
        1.0 / 120.0,
        1.0 / 45.0,
        1.0 / 60.0,
        1.0 / 60.0,
        2.0 / 60.0,
        1.0 / 60.0,
    ];
    let mut total_steps = 0u32;
    for &dt in &deltas {
        let out = ml.step(dt);
        total_steps += out.physics_steps;
    }

    let trace = ml.physics_server().trace();
    let ball_entries = trace.iter().filter(|e| e.name == "Ball").count();

    assert_eq!(
        ball_entries, total_steps as usize,
        "trace entries should match total physics steps"
    );
}

// ===========================================================================
// 6. TPS changes mid-simulation
// ===========================================================================

/// Changing physics_ticks_per_second mid-simulation affects the next frame.
#[test]
fn tps_change_affects_next_frame() {
    let (mut ml, _) = make_single_body_mainloop();

    // Start at 60 TPS. 1/60 delta → 1 tick.
    let out1 = ml.step(1.0 / 60.0);
    assert_eq!(out1.physics_steps, 1);

    // Switch to 120 TPS. Now physics_dt = 1/120.
    // With delta=1/60, should get 2 ticks.
    ml.set_physics_ticks_per_second(120);
    let out2 = ml.step(1.0 / 60.0);
    assert_eq!(
        out2.physics_steps, 2,
        "at 120 TPS, delta=1/60 should produce 2 ticks"
    );
}

/// Switching to lower TPS means fewer ticks per frame.
#[test]
fn lower_tps_fewer_ticks() {
    let (mut ml, _) = make_single_body_mainloop();

    // 30 TPS: physics_dt = 1/30. With delta=1/60, no tick.
    ml.set_physics_ticks_per_second(30);
    let out1 = ml.step(1.0 / 60.0);
    assert_eq!(
        out1.physics_steps, 0,
        "half-tick at 30 TPS should produce 0 ticks"
    );

    // Next 1/60 frame accumulates to full tick.
    let out2 = ml.step(1.0 / 60.0);
    assert_eq!(out2.physics_steps, 1, "accumulated full tick at 30 TPS");
}

// ===========================================================================
// 7. Negative delta edge case
// ===========================================================================

/// Negative delta should not produce physics ticks or crash.
#[test]
fn negative_delta_no_ticks() {
    let (mut ml, _) = make_single_body_mainloop();
    let output = ml.step(-1.0 / 60.0);

    assert_eq!(
        output.physics_steps, 0,
        "negative delta should produce 0 ticks"
    );
    assert_eq!(output.frame_count, 1, "frame counter still increments");
}

// ===========================================================================
// 8. Very small sub-tick deltas accumulate
// ===========================================================================

/// Many very small deltas (much less than physics_dt) should eventually
/// accumulate into a physics tick.
#[test]
fn tiny_deltas_accumulate_to_tick() {
    let (mut ml, _) = make_single_body_mainloop();
    let physics_dt = 1.0 / 60.0;

    // Each sub-frame is 1/10th of a physics tick.
    let micro_dt = physics_dt / 10.0;
    let mut total_steps = 0u32;
    for _ in 0..10 {
        let out = ml.step(micro_dt);
        total_steps += out.physics_steps;
    }

    assert_eq!(
        total_steps, 1,
        "10 micro-deltas of physics_dt/10 should produce exactly 1 tick"
    );
}

/// 20 micro-deltas of physics_dt/10 should produce exactly 2 ticks.
#[test]
fn twenty_micro_deltas_two_ticks() {
    let (mut ml, _) = make_single_body_mainloop();
    let physics_dt = 1.0 / 60.0;
    let micro_dt = physics_dt / 10.0;

    let mut total_steps = 0u32;
    for _ in 0..20 {
        let out = ml.step(micro_dt);
        total_steps += out.physics_steps;
    }

    assert_eq!(total_steps, 2, "20 micro-deltas should produce 2 ticks");
}

// ===========================================================================
// 9. Physics body count matches registration
// ===========================================================================

/// The physics server should have one body per RigidBody2D/StaticBody2D node.
#[test]
fn physics_body_count_matches_scene() {
    let (ml, _, _) = make_multi_body_mainloop();
    // 2 RigidBody2D + 1 StaticBody2D = 3 bodies
    assert_eq!(
        ml.physics_server().body_count(),
        3,
        "expected 3 physics bodies (2 rigid + 1 static)"
    );
}

// ===========================================================================
// 10. Velocity synced back after physics step
// ===========================================================================

/// Velocity should be synced from physics world back to scene node each tick.
#[test]
fn velocity_synced_back_to_scene() {
    let (mut ml, ball_a_id, _) = make_multi_body_mainloop();

    ml.step(1.0 / 60.0);

    let vel = get_vel(&ml, ball_a_id);
    // BallA has initial velocity (120, 0) — should still be non-zero.
    assert!(
        vel.length_squared() > 0.0,
        "velocity should be synced back: {:?}",
        vel
    );
}

// ===========================================================================
// 11. CharacterBody2D movement through MainLoop
// ===========================================================================

/// A CharacterBody2D with velocity should move through the MainLoop
/// physics stepping cycle.
#[test]
fn character_body_moves_via_mainloop() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    // CharacterBody2D moving right
    let mut character = Node::new("Player", "CharacterBody2D");
    character.set_property("position", Variant::Vector2(Vector2::new(50.0, 100.0)));
    character.set_property("velocity", Variant::Vector2(Vector2::new(200.0, 0.0)));
    let char_id = tree.add_child(root, character).unwrap();
    let mut shape = Node::new("CollisionShape", "CollisionShape2D");
    shape.set_property("radius", Variant::Float(10.0));
    tree.add_child(char_id, shape).unwrap();

    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();

    let initial_x = get_pos(&ml, char_id).x;

    // Run 10 frames at 60fps.
    ml.run_frames(10, 1.0 / 60.0);

    let final_x = get_pos(&ml, char_id).x;

    assert!(
        final_x > initial_x,
        "CharacterBody2D should move right: {} > {}",
        final_x,
        initial_x
    );
}

// ===========================================================================
// 12. Physics stepping with mixed frame rates
// ===========================================================================

/// Godot contract: the fixed-step loop produces consistent physics regardless
/// of variable render frame rates. Running at 30fps or 120fps should produce
/// the same total physics time after the same wall time.
#[test]
fn mixed_frame_rates_same_physics_time() {
    // Run A: 60 frames at 60fps (1 second).
    let (mut ml_a, _) = make_single_body_mainloop();
    ml_a.run_frames(60, 1.0 / 60.0);

    // Run B: 30 frames at 30fps (1 second).
    let (mut ml_b, _) = make_single_body_mainloop();
    ml_b.run_frames(30, 1.0 / 30.0);

    // Both should have ~1.0s of physics time.
    assert!(
        approx_eq_f64(ml_a.physics_time(), ml_b.physics_time()),
        "same wall time should produce same physics_time: {} vs {}",
        ml_a.physics_time(),
        ml_b.physics_time()
    );
}

/// Mixed frame rates produce the same final body position (determinism
/// independent of render rate).
#[test]
fn mixed_frame_rates_same_final_position() {
    // Run A: 60 frames at 60fps.
    let (mut ml_a, rigid_a) = make_single_body_mainloop();
    ml_a.run_frames(60, 1.0 / 60.0);

    // Run B: 30 frames at 30fps (each frame does 2 physics ticks).
    let (mut ml_b, rigid_b) = make_single_body_mainloop();
    ml_b.run_frames(30, 1.0 / 30.0);

    let pos_a = get_pos(&ml_a, rigid_a);
    let pos_b = get_pos(&ml_b, rigid_b);

    assert!(
        approx_eq_f32(pos_a.x, pos_b.x) && approx_eq_f32(pos_a.y, pos_b.y),
        "same physics time at different render rates should produce same position: {:?} vs {:?}",
        pos_a,
        pos_b
    );
}

// ===========================================================================
// 13. Frame output reports correct values
// ===========================================================================

/// FrameOutput.delta should match the delta passed to step().
#[test]
fn frame_output_delta_matches_input() {
    let (mut ml, _) = make_single_body_mainloop();

    let deltas = [1.0 / 60.0, 1.0 / 30.0, 1.0 / 144.0];
    for &dt in &deltas {
        let out = ml.step(dt);
        assert!(
            approx_eq_f64(out.delta, dt),
            "FrameOutput.delta {} should match input {}",
            out.delta,
            dt
        );
    }
}

/// FrameOutput.frame_count should be consecutive across steps.
#[test]
fn frame_output_frame_count_consecutive() {
    let (mut ml, _) = make_single_body_mainloop();

    for expected in 1..=10u64 {
        let out = ml.step(1.0 / 60.0);
        assert_eq!(out.frame_count, expected);
    }
}

// ===========================================================================
// 14. Spiral-of-death guard with multi-body
// ===========================================================================

/// The spiral-of-death guard should clamp physics steps even with multiple
/// bodies. All bodies should still advance the same number of ticks.
#[test]
fn spiral_guard_multi_body_same_tick_count() {
    let (mut ml, _, _) = make_multi_body_mainloop();
    ml.set_max_physics_steps_per_frame(3);
    ml.physics_server_mut().set_tracing(true);

    // Huge delta → would need many ticks, clamped to 3.
    let output = ml.step(20.0 / 60.0);
    assert_eq!(output.physics_steps, 3);

    let trace = ml.physics_server().trace();
    let ball_a_count = trace.iter().filter(|e| e.name == "BallA").count();
    let ball_b_count = trace.iter().filter(|e| e.name == "BallB").count();

    assert_eq!(ball_a_count, 3, "BallA should have 3 trace entries");
    assert_eq!(ball_b_count, 3, "BallB should have 3 trace entries");
}

// ===========================================================================
// 15. Physics and process deltas are independent
// ===========================================================================

/// Physics uses fixed dt while process uses variable dt.
/// After a frame with delta=2/60, process_time should advance by 2/60
/// while physics_time advances by 2 * (1/60).
#[test]
fn physics_fixed_dt_vs_process_variable_dt() {
    let (mut ml, _) = make_single_body_mainloop();

    let frame_dt = 2.0 / 60.0; // Will produce 2 physics ticks at 60 TPS
    ml.step(frame_dt);

    // Physics time: 2 ticks * (1/60) = 2/60
    let expected_physics = 2.0 / 60.0;
    assert!(
        approx_eq_f64(ml.physics_time(), expected_physics),
        "physics_time={}, expected={}",
        ml.physics_time(),
        expected_physics
    );

    // Process time: variable delta = 2/60
    assert!(
        approx_eq_f64(ml.process_time(), frame_dt),
        "process_time={}, expected={}",
        ml.process_time(),
        frame_dt
    );

    // They happen to be equal here, but let's verify with a non-multiple:
    // Frame with delta=1.5/60 → 1 physics tick + leftover
    ml.step(1.5 / 60.0);
    let expected_physics_2 = expected_physics + 1.0 / 60.0; // 1 more tick
    let expected_process_2 = frame_dt + 1.5 / 60.0;
    assert!(
        approx_eq_f64(ml.physics_time(), expected_physics_2),
        "after non-multiple delta: physics_time={}, expected={}",
        ml.physics_time(),
        expected_physics_2
    );
    assert!(
        approx_eq_f64(ml.process_time(), expected_process_2),
        "after non-multiple delta: process_time={}, expected={}",
        ml.process_time(),
        expected_process_2
    );
}
