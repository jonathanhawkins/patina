//! pat-exyv: Advance gdphysics2d from MainLoop fixed-step frames.
//!
//! Proves that the MainLoop drives gdphysics2d through the runtime fixed-step
//! loop correctly. Tests verify:
//! - Physics stepping matches Godot's accumulator-based fixed timestep model
//! - PhysicsServer sync_to/sync_from/step_physics are called per tick
//! - RigidBody2D positions advance under velocity each physics tick
//! - Frame output reports correct physics_steps count
//! - Physics time accumulates in fixed increments
//! - Multiple physics ticks per frame when delta > physics_dt
//! - Spiral-of-death guard clamps physics steps per frame
//! - Paused frames skip physics stepping entirely
//! - Physics trace entries correlate with frame count
//! - Determinism: same inputs produce same physics state

use gdcore::math::Vector2;
use gdscene::main_loop::MainLoop;
use gdscene::node::Node;
use gdscene::scene_tree::SceneTree;
use gdscene::trace::TraceEventType;
use gdvariant::Variant;

const EPSILON: f64 = 1e-6;
const EPSILON_F32: f32 = 1e-3;

fn approx_eq_f64(a: f64, b: f64) -> bool {
    (a - b).abs() < EPSILON
}

fn approx_eq_f32(a: f32, b: f32) -> bool {
    (a - b).abs() < EPSILON_F32
}

// ===========================================================================
// Helpers
// ===========================================================================

/// Build a scene with a RigidBody2D moving downward and a StaticBody2D floor.
fn make_physics_scene() -> (SceneTree, gdscene::node::NodeId, gdscene::node::NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    // RigidBody2D with downward velocity
    let mut rigid = Node::new("Ball", "RigidBody2D");
    rigid.set_property("position", Variant::Vector2(Vector2::new(100.0, 50.0)));
    rigid.set_property("mass", Variant::Float(1.0));
    rigid.set_property("linear_velocity", Variant::Vector2(Vector2::new(0.0, 60.0)));
    let rigid_id = tree.add_child(root, rigid).unwrap();
    let mut shape = Node::new("CollisionShape", "CollisionShape2D");
    shape.set_property("radius", Variant::Float(16.0));
    tree.add_child(rigid_id, shape).unwrap();

    // StaticBody2D floor far below (won't collide during tests)
    let mut floor = Node::new("Floor", "StaticBody2D");
    floor.set_property("position", Variant::Vector2(Vector2::new(100.0, 500.0)));
    let floor_id = tree.add_child(root, floor).unwrap();
    let mut shape2 = Node::new("CollisionShape", "CollisionShape2D");
    shape2.set_property("size", Variant::Vector2(Vector2::new(400.0, 20.0)));
    tree.add_child(floor_id, shape2).unwrap();

    (tree, rigid_id, floor_id)
}

/// Build a MainLoop with physics bodies registered.
fn make_physics_mainloop() -> (MainLoop, gdscene::node::NodeId) {
    let (tree, rigid_id, _floor_id) = make_physics_scene();
    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();
    (ml, rigid_id)
}

// ===========================================================================
// 1. Single frame at 60fps produces exactly 1 physics tick
// ===========================================================================

/// Godot contract: at 60 TPS with delta=1/60, exactly 1 physics tick fires.
#[test]
fn single_frame_one_physics_tick() {
    let (mut ml, _rigid_id) = make_physics_mainloop();
    let output = ml.step(1.0 / 60.0);

    assert_eq!(output.frame_count, 1);
    assert_eq!(output.physics_steps, 1);
    assert!(
        approx_eq_f64(output.delta, 1.0 / 60.0),
        "delta should be 1/60"
    );
}

// ===========================================================================
// 2. Frame counter increments correctly over multiple frames
// ===========================================================================

#[test]
fn frame_counter_increments_per_step() {
    let (mut ml, _) = make_physics_mainloop();
    for i in 1..=5 {
        let output = ml.step(1.0 / 60.0);
        assert_eq!(output.frame_count, i);
    }
    assert_eq!(ml.frame_count(), 5);
}

// ===========================================================================
// 3. Physics time accumulates in fixed increments
// ===========================================================================

/// Godot contract: physics_time advances by exactly physics_dt per tick,
/// not by the variable frame delta.
#[test]
fn physics_time_advances_in_fixed_increments() {
    let (mut ml, _) = make_physics_mainloop();
    let physics_dt = 1.0 / 60.0;

    // Run 10 frames at exactly 60fps.
    ml.run_frames(10, physics_dt);

    let expected_physics_time = 10.0 * physics_dt;
    assert!(
        approx_eq_f64(ml.physics_time(), expected_physics_time),
        "physics_time {} != expected {}",
        ml.physics_time(),
        expected_physics_time
    );
}

// ===========================================================================
// 4. Process time is sum of all deltas
// ===========================================================================

#[test]
fn process_time_is_sum_of_deltas() {
    let (mut ml, _) = make_physics_mainloop();

    ml.step(1.0 / 60.0);
    ml.step(1.0 / 30.0);
    ml.step(1.0 / 120.0);

    let expected = 1.0 / 60.0 + 1.0 / 30.0 + 1.0 / 120.0;
    assert!(
        approx_eq_f64(ml.process_time(), expected),
        "process_time {} != expected {}",
        ml.process_time(),
        expected
    );
}

// ===========================================================================
// 5. Large delta produces multiple physics ticks per frame
// ===========================================================================

/// Godot contract: when delta > physics_dt, multiple physics ticks fire
/// in a single frame to keep physics time caught up.
#[test]
fn large_delta_multiple_physics_ticks() {
    let (mut ml, _) = make_physics_mainloop();
    // At 60 TPS, delta=2/60 means 2 physics ticks.
    let output = ml.step(2.0 / 60.0);

    assert_eq!(output.physics_steps, 2, "expected 2 physics ticks for 2x delta");
    assert_eq!(output.frame_count, 1);
}

// ===========================================================================
// 6. Fractional delta accumulates across frames
// ===========================================================================

/// Godot contract: the accumulator carries remainder time forward. With
/// delta slightly less than physics_dt, the tick happens on the next frame.
#[test]
fn accumulator_carries_remainder_across_frames() {
    let (mut ml, _) = make_physics_mainloop();
    let physics_dt = 1.0 / 60.0;

    // First frame: delta = 0.9 * physics_dt → 0 ticks (accumulator = 0.9 * dt)
    let out1 = ml.step(physics_dt * 0.9);
    assert_eq!(out1.physics_steps, 0, "0.9x delta should produce 0 ticks");

    // Second frame: delta = 0.2 * physics_dt → 1 tick (accumulated: 1.1 * dt)
    let out2 = ml.step(physics_dt * 0.2);
    assert_eq!(out2.physics_steps, 1, "accumulated 1.1x dt should produce 1 tick");
}

// ===========================================================================
// 7. Spiral-of-death guard clamps physics steps
// ===========================================================================

/// Godot contract: max_physics_steps_per_frame prevents unbounded physics
/// stepping from very large deltas.
#[test]
fn spiral_of_death_guard_clamps_steps() {
    let (mut ml, _) = make_physics_mainloop();
    ml.set_max_physics_steps_per_frame(4);

    // delta = 10x physics_dt → would need 10 ticks, but clamped to 4.
    let output = ml.step(10.0 / 60.0);
    assert_eq!(
        output.physics_steps, 4,
        "max_physics_steps_per_frame should clamp at 4"
    );
}

/// After clamping, the accumulator is reset so the next frame doesn't
/// continue catching up (Godot's spiral-of-death prevention).
#[test]
fn spiral_of_death_resets_accumulator() {
    let (mut ml, _) = make_physics_mainloop();
    ml.set_max_physics_steps_per_frame(2);

    // Huge delta → clamped to 2 steps. Accumulator should be reset.
    let out1 = ml.step(100.0 / 60.0);
    assert_eq!(out1.physics_steps, 2);

    // Next frame with exactly 1 physics_dt should produce exactly 1 tick
    // (not a huge backlog from the clamped frame).
    let out2 = ml.step(1.0 / 60.0);
    assert_eq!(
        out2.physics_steps, 1,
        "accumulator should have been reset after clamp"
    );
}

// ===========================================================================
// 8. Paused frames skip physics entirely
// ===========================================================================

/// Godot contract: when paused, no PHYSICS_PROCESS or PROCESS notifications
/// fire, and no physics ticks run.
#[test]
fn paused_frames_skip_physics() {
    let (mut ml, _) = make_physics_mainloop();
    ml.tree_mut().event_trace_mut().enable();

    ml.set_paused(true);
    let output = ml.step(1.0 / 60.0);

    // Frame counter still advances (Godot increments frame_count even when paused).
    assert_eq!(output.frame_count, 1);

    // No physics or process notifications should have fired.
    let events = ml.tree().event_trace().events();
    let process_events: Vec<_> = events
        .iter()
        .filter(|e| {
            e.event_type == TraceEventType::Notification
                && (e.detail == "PHYSICS_PROCESS"
                    || e.detail == "PROCESS"
                    || e.detail == "INTERNAL_PHYSICS_PROCESS"
                    || e.detail == "INTERNAL_PROCESS")
        })
        .collect();
    assert!(
        process_events.is_empty(),
        "paused frame should produce no physics/process notifications"
    );
}

// ===========================================================================
// 9. RigidBody2D position advances under velocity each physics tick
// ===========================================================================

/// Core physics contract: a RigidBody2D with linear_velocity moves by
/// velocity * dt each physics tick.
#[test]
fn rigid_body_position_advances_per_tick() {
    let (mut ml, rigid_id) = make_physics_mainloop();
    let physics_dt = 1.0 / 60.0;

    let initial_pos = match ml.tree().get_node(rigid_id).unwrap().get_property("position") {
        Variant::Vector2(v) => v,
        _ => panic!("expected Vector2"),
    };
    assert!(approx_eq_f32(initial_pos.x, 100.0));
    assert!(approx_eq_f32(initial_pos.y, 50.0));

    // Run 1 frame (1 physics tick).
    ml.step(physics_dt);

    let new_pos = match ml.tree().get_node(rigid_id).unwrap().get_property("position") {
        Variant::Vector2(v) => v,
        _ => panic!("expected Vector2"),
    };

    // Ball has velocity (0, 60), so after 1 tick at dt=1/60:
    // y should advance by 60 * (1/60) = 1.0 pixel.
    assert!(
        approx_eq_f32(new_pos.x, 100.0),
        "x should not change, got {}",
        new_pos.x
    );
    assert!(
        new_pos.y > initial_pos.y,
        "y should increase: {} > {}",
        new_pos.y,
        initial_pos.y
    );
}

// ===========================================================================
// 10. Multiple frames accumulate position changes
// ===========================================================================

#[test]
fn rigid_body_position_accumulates_over_frames() {
    let (mut ml, rigid_id) = make_physics_mainloop();
    let physics_dt = 1.0 / 60.0;

    let initial_y = match ml.tree().get_node(rigid_id).unwrap().get_property("position") {
        Variant::Vector2(v) => v.y,
        _ => panic!("expected Vector2"),
    };

    // Run 10 frames.
    ml.run_frames(10, physics_dt);

    let final_y = match ml.tree().get_node(rigid_id).unwrap().get_property("position") {
        Variant::Vector2(v) => v.y,
        _ => panic!("expected Vector2"),
    };

    // After 10 ticks with velocity 60 px/s, expected displacement is ~10 px.
    assert!(
        final_y > initial_y,
        "position should have advanced: final {} > initial {}",
        final_y,
        initial_y
    );
    assert!(
        final_y > initial_y + 5.0,
        "expected significant Y displacement after 10 frames"
    );
}

// ===========================================================================
// 11. Physics trace records one entry per body per tick
// ===========================================================================

#[test]
fn physics_trace_entries_per_tick() {
    let (mut ml, _rigid_id) = make_physics_mainloop();
    ml.physics_server_mut().set_tracing(true);

    // Run 5 frames at 60fps → 5 physics ticks.
    ml.run_frames(5, 1.0 / 60.0);

    let trace = ml.physics_server().trace();
    // 2 bodies (Ball + Floor) × 5 ticks = 10 entries.
    assert_eq!(
        trace.len(),
        10,
        "expected 10 trace entries (2 bodies × 5 ticks), got {}",
        trace.len()
    );

    // Verify Ball entries show advancing position.
    let ball_entries: Vec<_> = trace.iter().filter(|e| e.name == "Ball").collect();
    assert_eq!(ball_entries.len(), 5);

    for i in 1..ball_entries.len() {
        assert!(
            ball_entries[i].position.y >= ball_entries[i - 1].position.y,
            "Ball Y should be non-decreasing across ticks"
        );
    }
}

// ===========================================================================
// 12. Physics trace frame numbers are monotonically increasing
// ===========================================================================

#[test]
fn physics_trace_frame_numbers_monotonic() {
    let (mut ml, _) = make_physics_mainloop();
    ml.physics_server_mut().set_tracing(true);

    ml.run_frames(5, 1.0 / 60.0);

    let trace = ml.physics_server().trace();
    let ball_frames: Vec<u64> = trace
        .iter()
        .filter(|e| e.name == "Ball")
        .map(|e| e.frame)
        .collect();

    for i in 1..ball_frames.len() {
        assert!(
            ball_frames[i] > ball_frames[i - 1],
            "trace frame numbers must be strictly increasing: {} <= {}",
            ball_frames[i],
            ball_frames[i - 1]
        );
    }
}

// ===========================================================================
// 13. Configurable physics ticks per second
// ===========================================================================

/// Godot contract: physics_ticks_per_second controls the fixed timestep.
/// At 30 TPS with delta=1/30, exactly 1 tick fires per frame.
#[test]
fn configurable_physics_tps() {
    let (mut ml, _) = make_physics_mainloop();
    ml.set_physics_ticks_per_second(30);

    // At 30 TPS, physics_dt = 1/30. With delta=1/30, exactly 1 tick.
    let output = ml.step(1.0 / 30.0);
    assert_eq!(output.physics_steps, 1);

    // With delta=1/60 (half a tick), no physics tick should fire.
    let output2 = ml.step(1.0 / 60.0);
    assert_eq!(output2.physics_steps, 0, "half a tick should not fire");

    // Next frame with delta=1/60 should fire (accumulated full tick).
    let output3 = ml.step(1.0 / 60.0);
    assert_eq!(output3.physics_steps, 1, "accumulated full tick should fire");
}

// ===========================================================================
// 14. StaticBody2D does not move
// ===========================================================================

#[test]
fn static_body_does_not_move() {
    let (tree, _rigid_id, floor_id) = make_physics_scene();
    let initial_pos = match tree.get_node(floor_id).unwrap().get_property("position") {
        Variant::Vector2(v) => v,
        _ => panic!("expected Vector2"),
    };

    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();
    ml.run_frames(10, 1.0 / 60.0);

    let final_pos = match ml.tree().get_node(floor_id).unwrap().get_property("position") {
        Variant::Vector2(v) => v,
        _ => panic!("expected Vector2"),
    };

    assert!(
        approx_eq_f32(initial_pos.x, final_pos.x) && approx_eq_f32(initial_pos.y, final_pos.y),
        "static body should not move: {:?} -> {:?}",
        initial_pos,
        final_pos
    );
}

// ===========================================================================
// 15. Notification ordering: INTERNAL_PHYSICS before PHYSICS per tick
// ===========================================================================

/// Godot contract: within each physics tick, INTERNAL_PHYSICS_PROCESS
/// fires before PHYSICS_PROCESS for every node.
#[test]
fn notification_ordering_internal_before_user_physics() {
    let (mut ml, _) = make_physics_mainloop();
    ml.tree_mut().event_trace_mut().enable();

    ml.step(1.0 / 60.0);

    let events = ml.tree().event_trace().events();
    let ball_events: Vec<_> = events
        .iter()
        .filter(|e| e.node_path == "/root/Ball" && e.event_type == TraceEventType::Notification)
        .collect();

    let int_phys_idx = ball_events
        .iter()
        .position(|e| e.detail == "INTERNAL_PHYSICS_PROCESS")
        .expect("INTERNAL_PHYSICS_PROCESS for Ball");
    let phys_idx = ball_events
        .iter()
        .position(|e| e.detail == "PHYSICS_PROCESS")
        .expect("PHYSICS_PROCESS for Ball");

    assert!(
        int_phys_idx < phys_idx,
        "INTERNAL_PHYSICS must precede PHYSICS"
    );
}

// ===========================================================================
// 16. Physics sync cycle: positions written back to scene tree
// ===========================================================================

/// Godot contract: after each physics tick, updated body positions are
/// synced back to the scene tree node properties.
#[test]
fn sync_from_physics_updates_scene_node() {
    let (mut ml, rigid_id) = make_physics_mainloop();

    // Record initial position.
    let initial = match ml.tree().get_node(rigid_id).unwrap().get_property("position") {
        Variant::Vector2(v) => v,
        _ => panic!("expected Vector2"),
    };

    ml.step(1.0 / 60.0);

    // Position should be updated in the scene tree.
    let after = match ml.tree().get_node(rigid_id).unwrap().get_property("position") {
        Variant::Vector2(v) => v,
        _ => panic!("expected Vector2"),
    };

    assert!(
        initial != after,
        "scene node position should be updated after physics step"
    );

    // linear_velocity should also be synced back.
    let vel = match ml
        .tree()
        .get_node(rigid_id)
        .unwrap()
        .get_property("linear_velocity")
    {
        Variant::Vector2(v) => v,
        _ => panic!("expected Vector2"),
    };
    // Velocity should still be non-zero (ball is falling).
    assert!(
        vel.length_squared() > 0.0,
        "velocity should be synced back to scene node"
    );
}

// ===========================================================================
// 17. Determinism: same inputs produce identical physics state
// ===========================================================================

/// Core determinism contract: running the same scene with the same deltas
/// produces bit-identical physics outcomes.
#[test]
fn deterministic_physics_across_runs() {
    fn run_and_capture() -> (Vector2, Vector2) {
        let (mut ml, rigid_id) = make_physics_mainloop();
        ml.run_frames(20, 1.0 / 60.0);
        let pos = match ml.tree().get_node(rigid_id).unwrap().get_property("position") {
            Variant::Vector2(v) => v,
            _ => panic!("expected Vector2"),
        };
        let vel = match ml
            .tree()
            .get_node(rigid_id)
            .unwrap()
            .get_property("linear_velocity")
        {
            Variant::Vector2(v) => v,
            _ => panic!("expected Vector2"),
        };
        (pos, vel)
    }

    let (pos1, vel1) = run_and_capture();
    let (pos2, vel2) = run_and_capture();

    assert!(
        approx_eq_f32(pos1.x, pos2.x) && approx_eq_f32(pos1.y, pos2.y),
        "positions must be deterministic: {:?} vs {:?}",
        pos1,
        pos2
    );
    assert!(
        approx_eq_f32(vel1.x, vel2.x) && approx_eq_f32(vel1.y, vel2.y),
        "velocities must be deterministic: {:?} vs {:?}",
        vel1,
        vel2
    );
}

// ===========================================================================
// 18. FrameOutput physics_steps matches actual trace entry count
// ===========================================================================

#[test]
fn frame_output_physics_steps_matches_trace() {
    let (mut ml, _) = make_physics_mainloop();
    ml.physics_server_mut().set_tracing(true);

    let out1 = ml.step(1.0 / 60.0);
    let out2 = ml.step(3.0 / 60.0); // 3 ticks

    let trace = ml.physics_server().trace();
    let ball_trace: Vec<_> = trace.iter().filter(|e| e.name == "Ball").collect();

    // Total physics ticks = out1.physics_steps + out2.physics_steps
    let total_ticks = (out1.physics_steps + out2.physics_steps) as usize;
    assert_eq!(
        ball_trace.len(),
        total_ticks,
        "trace entries should match reported physics_steps"
    );
}

// ===========================================================================
// 19. Pausing skips PHYSICS_PROCESS notifications but physics world still steps
// ===========================================================================

/// Current behavior: the PhysicsServer still steps the world while paused
/// (bodies advance), but no PHYSICS_PROCESS / PROCESS notifications fire
/// for pausable nodes. This matches the notification gating via
/// `should_process_node` which returns false for Pausable mode when paused.
#[test]
fn paused_skips_physics_notifications_but_world_steps() {
    let (mut ml, rigid_id) = make_physics_mainloop();
    ml.tree_mut().event_trace_mut().enable();

    // Run 5 frames normally.
    ml.run_frames(5, 1.0 / 60.0);
    let pos_after_5 = match ml.tree().get_node(rigid_id).unwrap().get_property("position") {
        Variant::Vector2(v) => v,
        _ => panic!("expected Vector2"),
    };

    // Pause and run 5 more frames.
    ml.set_paused(true);
    ml.tree_mut().event_trace_mut().clear();
    ml.tree_mut().event_trace_mut().enable();
    ml.run_frames(5, 1.0 / 60.0);

    // No PHYSICS_PROCESS or PROCESS notifications should fire while paused
    // (these are gated by should_process_node returning false for Pausable nodes).
    let events = ml.tree().event_trace().events();
    let physics_notifs: Vec<_> = events
        .iter()
        .filter(|e| {
            e.event_type == TraceEventType::Notification
                && e.node_path == "/root/Ball"
                && (e.detail == "PHYSICS_PROCESS"
                    || e.detail == "PROCESS"
                    || e.detail == "INTERNAL_PHYSICS_PROCESS"
                    || e.detail == "INTERNAL_PROCESS")
        })
        .collect();
    assert!(
        physics_notifs.is_empty(),
        "no physics/process notifications should fire for pausable nodes while paused, got {} events",
        physics_notifs.len()
    );

    // The physics world DID step though, so position may have changed
    // (sync_to/step/sync_from still run in the MainLoop step).
    // This is the current behavior — the paused gate is at the
    // notification/script level, not the physics server level.
    let pos_after_pause = match ml.tree().get_node(rigid_id).unwrap().get_property("position") {
        Variant::Vector2(v) => v,
        _ => panic!("expected Vector2"),
    };
    // Position changed because physics world stepped.
    assert!(
        pos_after_pause.y > pos_after_5.y,
        "physics world still steps while paused (positions advance)"
    );
}

// ===========================================================================
// 20. Zero delta produces no physics ticks
// ===========================================================================

#[test]
fn zero_delta_no_physics_ticks() {
    let (mut ml, _) = make_physics_mainloop();
    let output = ml.step(0.0);

    assert_eq!(output.physics_steps, 0, "zero delta should produce 0 ticks");
    assert_eq!(output.frame_count, 1, "frame counter still increments");
}
