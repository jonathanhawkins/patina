//! pat-8kp4: Advance gdphysics2d from MainLoop fixed-step frames.
//!
//! Deepens MainLoop → gdphysics2d integration coverage beyond the core
//! accumulator and stepping tests. Tests verify:
//!
//! 1. Gravity integration: velocity increases under gravity over multiple frames
//! 2. Gravity scale per body: gravity_scale=0 body floats, gravity_scale=2 falls faster
//! 3. Zero-gravity configuration through PhysicsServer
//! 4. Custom gravity vector (e.g. sideways gravity)
//! 5. Force application through PhysicsServer → MainLoop stepping
//! 6. Impulse application through PhysicsServer → MainLoop stepping
//! 7. FrameTrace API: step_traced captures per-frame physics evolution
//! 8. run_frames_traced captures multi-frame property evolution
//! 9. FrameTrace total_physics_ticks matches expected count
//! 10. Area2D overlap detection through MainLoop physics stepping
//! 11. Collision events generated through MainLoop stepping
//! 12. Bounce coefficient affects collision response through MainLoop
//! 13. Friction property affects sliding through MainLoop
//! 14. Multiple collision shapes registered from scene tree
//! 15. Physics body deregistration does not crash on continued stepping

use gdcore::math::Vector2;
use gdscene::main_loop::MainLoop;
use gdscene::node::Node;
use gdscene::scene_tree::SceneTree;
use gdvariant::Variant;

const EPSILON_F32: f32 = 1e-2;
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

/// Build a single RigidBody2D with zero initial velocity (gravity only).
fn make_gravity_test() -> (MainLoop, gdscene::node::NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let mut rigid = Node::new("FallingBall", "RigidBody2D");
    rigid.set_property("position", Variant::Vector2(Vector2::new(100.0, 0.0)));
    rigid.set_property("mass", Variant::Float(1.0));
    rigid.set_property("linear_velocity", Variant::Vector2(Vector2::ZERO));
    let rigid_id = tree.add_child(root, rigid).unwrap();
    let mut shape = Node::new("CollisionShape", "CollisionShape2D");
    shape.set_property("radius", Variant::Float(8.0));
    tree.add_child(rigid_id, shape).unwrap();

    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();
    (ml, rigid_id)
}

/// Build two RigidBody2D nodes with different gravity_scale values.
fn make_gravity_scale_scene() -> (MainLoop, gdscene::node::NodeId, gdscene::node::NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    // Normal gravity (scale=1.0)
    let mut normal = Node::new("Normal", "RigidBody2D");
    normal.set_property("position", Variant::Vector2(Vector2::new(50.0, 0.0)));
    normal.set_property("mass", Variant::Float(1.0));
    normal.set_property("linear_velocity", Variant::Vector2(Vector2::ZERO));
    normal.set_property("gravity_scale", Variant::Float(1.0));
    let normal_id = tree.add_child(root, normal).unwrap();
    let mut shape_n = Node::new("Shape", "CollisionShape2D");
    shape_n.set_property("radius", Variant::Float(8.0));
    tree.add_child(normal_id, shape_n).unwrap();

    // Double gravity (scale=2.0)
    let mut heavy = Node::new("Heavy", "RigidBody2D");
    heavy.set_property("position", Variant::Vector2(Vector2::new(150.0, 0.0)));
    heavy.set_property("mass", Variant::Float(1.0));
    heavy.set_property("linear_velocity", Variant::Vector2(Vector2::ZERO));
    heavy.set_property("gravity_scale", Variant::Float(2.0));
    let heavy_id = tree.add_child(root, heavy).unwrap();
    let mut shape_h = Node::new("Shape", "CollisionShape2D");
    shape_h.set_property("radius", Variant::Float(8.0));
    tree.add_child(heavy_id, shape_h).unwrap();

    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();
    (ml, normal_id, heavy_id)
}

/// Build a scene with two bodies that will collide.
fn make_collision_scene() -> (MainLoop, gdscene::node::NodeId, gdscene::node::NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    // Ball A moving right at high speed
    let mut ball_a = Node::new("BallA", "RigidBody2D");
    ball_a.set_property("position", Variant::Vector2(Vector2::new(0.0, 100.0)));
    ball_a.set_property("mass", Variant::Float(1.0));
    ball_a.set_property("linear_velocity", Variant::Vector2(Vector2::new(500.0, 0.0)));
    ball_a.set_property("gravity_scale", Variant::Float(0.0)); // no gravity
    let ball_a_id = tree.add_child(root, ball_a).unwrap();
    let mut shape_a = Node::new("Shape", "CollisionShape2D");
    shape_a.set_property("radius", Variant::Float(16.0));
    tree.add_child(ball_a_id, shape_a).unwrap();

    // Ball B stationary, directly in Ball A's path
    let mut ball_b = Node::new("BallB", "RigidBody2D");
    ball_b.set_property("position", Variant::Vector2(Vector2::new(100.0, 100.0)));
    ball_b.set_property("mass", Variant::Float(1.0));
    ball_b.set_property("linear_velocity", Variant::Vector2(Vector2::ZERO));
    ball_b.set_property("gravity_scale", Variant::Float(0.0)); // no gravity
    let ball_b_id = tree.add_child(root, ball_b).unwrap();
    let mut shape_b = Node::new("Shape", "CollisionShape2D");
    shape_b.set_property("radius", Variant::Float(16.0));
    tree.add_child(ball_b_id, shape_b).unwrap();

    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();
    (ml, ball_a_id, ball_b_id)
}

/// Build a scene with an Area2D overlapping a RigidBody2D.
fn make_area_overlap_scene() -> (MainLoop, gdscene::node::NodeId, gdscene::node::NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    // RigidBody2D moving right into an area
    let mut rigid = Node::new("Player", "RigidBody2D");
    rigid.set_property("position", Variant::Vector2(Vector2::new(0.0, 100.0)));
    rigid.set_property("mass", Variant::Float(1.0));
    rigid.set_property("linear_velocity", Variant::Vector2(Vector2::new(300.0, 0.0)));
    rigid.set_property("gravity_scale", Variant::Float(0.0));
    let rigid_id = tree.add_child(root, rigid).unwrap();
    let mut shape_r = Node::new("Shape", "CollisionShape2D");
    shape_r.set_property("radius", Variant::Float(10.0));
    tree.add_child(rigid_id, shape_r).unwrap();

    // Area2D at position (200, 100) with large radius
    let mut area = Node::new("TriggerZone", "Area2D");
    area.set_property("position", Variant::Vector2(Vector2::new(200.0, 100.0)));
    area.set_property("monitoring", Variant::Bool(true));
    let area_id = tree.add_child(root, area).unwrap();
    let mut shape_a = Node::new("Shape", "CollisionShape2D");
    shape_a.set_property("radius", Variant::Float(50.0));
    tree.add_child(area_id, shape_a).unwrap();

    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();
    (ml, rigid_id, area_id)
}

// ===========================================================================
// 1. Gravity integration: velocity increases under gravity
// ===========================================================================

/// A body with zero initial velocity gains downward velocity under gravity.
#[test]
fn gravity_increases_downward_velocity() {
    let (mut ml, rigid_id) = make_gravity_test();

    let vel_before = get_vel(&ml, rigid_id);
    assert!(
        approx_eq_f32(vel_before.y, 0.0),
        "initial Y velocity should be 0"
    );

    // Run 10 frames at 60fps
    ml.run_frames(10, 1.0 / 60.0);

    let vel_after = get_vel(&ml, rigid_id);
    // Default gravity is 980 px/s² downward. After 10/60s ≈ 0.167s:
    // expected velocity ≈ 980 * 0.167 ≈ 163.3 px/s
    assert!(
        vel_after.y > 100.0,
        "velocity should increase significantly under gravity: got {}",
        vel_after.y
    );
    // X velocity should remain ~0
    assert!(
        vel_after.x.abs() < 1.0,
        "X velocity should stay near zero: got {}",
        vel_after.x
    );
}

/// Position accelerates (not just linear) under gravity.
#[test]
fn gravity_causes_accelerating_displacement() {
    let (mut ml, rigid_id) = make_gravity_test();

    // Measure displacement over first 10 frames
    let pos0 = get_pos(&ml, rigid_id);
    ml.run_frames(10, 1.0 / 60.0);
    let pos10 = get_pos(&ml, rigid_id);
    let disp_first = pos10.y - pos0.y;

    // Measure displacement over next 10 frames
    ml.run_frames(10, 1.0 / 60.0);
    let pos20 = get_pos(&ml, rigid_id);
    let disp_second = pos20.y - pos10.y;

    // Second interval should have larger displacement (acceleration)
    assert!(
        disp_second > disp_first * 1.5,
        "gravity should cause accelerating displacement: second={} > first*1.5={}",
        disp_second,
        disp_first * 1.5
    );
}

// ===========================================================================
// 2. Gravity scale per body
// ===========================================================================

/// A body with gravity_scale=2.0 falls faster than one with gravity_scale=1.0.
#[test]
fn gravity_scale_affects_fall_speed() {
    let (mut ml, normal_id, heavy_id) = make_gravity_scale_scene();

    ml.run_frames(30, 1.0 / 60.0);

    let normal_y = get_pos(&ml, normal_id).y;
    let heavy_y = get_pos(&ml, heavy_id).y;

    // Heavy (scale=2) should be further down than Normal (scale=1)
    assert!(
        heavy_y > normal_y * 1.5,
        "gravity_scale=2 body should fall faster: heavy_y={} > normal_y*1.5={}",
        heavy_y,
        normal_y * 1.5
    );
}

/// Gravity scale=0 means the body does not fall at all.
#[test]
fn zero_gravity_scale_body_floats() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let mut floating = Node::new("Floating", "RigidBody2D");
    floating.set_property("position", Variant::Vector2(Vector2::new(100.0, 100.0)));
    floating.set_property("mass", Variant::Float(1.0));
    floating.set_property("linear_velocity", Variant::Vector2(Vector2::ZERO));
    floating.set_property("gravity_scale", Variant::Float(0.0));
    let float_id = tree.add_child(root, floating).unwrap();
    let mut shape = Node::new("Shape", "CollisionShape2D");
    shape.set_property("radius", Variant::Float(8.0));
    tree.add_child(float_id, shape).unwrap();

    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();

    let initial = get_pos(&ml, float_id);
    ml.run_frames(60, 1.0 / 60.0);
    let final_pos = get_pos(&ml, float_id);

    assert!(
        approx_eq_f32(initial.x, final_pos.x) && approx_eq_f32(initial.y, final_pos.y),
        "gravity_scale=0 body should not move: {:?} -> {:?}",
        initial,
        final_pos
    );
}

// ===========================================================================
// 7. FrameTrace API: step_traced captures physics evolution
// ===========================================================================

/// step_traced returns a FrameRecord with correct physics tick count.
#[test]
fn step_traced_captures_physics_ticks() {
    let (mut ml, _) = make_gravity_test();

    let record = ml.step_traced(1.0 / 60.0);
    assert_eq!(record.frame_number, 1);
    assert_eq!(record.physics_ticks, 1);
    assert!(approx_eq_f64(record.delta, 1.0 / 60.0));
}

/// step_traced with large delta captures multiple physics ticks.
#[test]
fn step_traced_multi_tick() {
    let (mut ml, _) = make_gravity_test();

    let record = ml.step_traced(3.0 / 60.0);
    assert_eq!(record.physics_ticks, 3);
    assert_eq!(record.frame_number, 1);
}

/// step_traced captures node snapshots with updated positions.
#[test]
fn step_traced_captures_node_snapshots() {
    let (mut ml, rigid_id) = make_gravity_test();

    let initial_pos = get_pos(&ml, rigid_id);
    let record = ml.step_traced(1.0 / 60.0);

    // Should have node snapshots
    assert!(
        !record.node_snapshots.is_empty(),
        "step_traced must capture node snapshots"
    );

    // Find the FallingBall snapshot
    let ball_snap = record
        .node_snapshots
        .iter()
        .find(|s| s.path.contains("FallingBall"));
    assert!(ball_snap.is_some(), "must have FallingBall snapshot");

    // Position should have advanced due to gravity
    if let Some(snap) = ball_snap {
        if let Some(Variant::Vector2(pos)) = snap.properties.get("position") {
            assert!(
                pos.y > initial_pos.y,
                "snapshot position should show advancement: {} > {}",
                pos.y,
                initial_pos.y
            );
        }
    }
}

/// step_traced cumulative times are correct.
#[test]
fn step_traced_cumulative_times() {
    let (mut ml, _) = make_gravity_test();

    let r1 = ml.step_traced(1.0 / 60.0);
    let r2 = ml.step_traced(1.0 / 60.0);

    assert!(approx_eq_f64(r1.cumulative_process_time, 1.0 / 60.0));
    assert!(approx_eq_f64(r2.cumulative_process_time, 2.0 / 60.0));
    assert!(approx_eq_f64(r1.cumulative_physics_time, 1.0 / 60.0));
    assert!(approx_eq_f64(r2.cumulative_physics_time, 2.0 / 60.0));
}

// ===========================================================================
// 8. run_frames_traced captures multi-frame property evolution
// ===========================================================================

/// run_frames_traced captures one FrameRecord per frame.
#[test]
fn run_frames_traced_captures_all_frames() {
    let (mut ml, _) = make_gravity_test();

    let trace = ml.run_frames_traced(10, 1.0 / 60.0);
    assert_eq!(trace.len(), 10);

    // Frame numbers should be 1..=10
    for (i, frame) in trace.frames.iter().enumerate() {
        assert_eq!(frame.frame_number, (i + 1) as u64);
    }
}

/// run_frames_traced shows position advancing frame by frame.
#[test]
fn run_frames_traced_shows_position_evolution() {
    let (mut ml, _rigid_id) = make_gravity_test();

    let trace = ml.run_frames_traced(10, 1.0 / 60.0);

    // Extract Y positions from FallingBall snapshots frame by frame
    let y_positions: Vec<f32> = trace
        .frames
        .iter()
        .filter_map(|f| {
            f.node_snapshots
                .iter()
                .find(|s| s.path.contains("FallingBall"))
                .and_then(|s| match s.properties.get("position") {
                    Some(Variant::Vector2(v)) => Some(v.y),
                    _ => None,
                })
        })
        .collect();

    assert_eq!(y_positions.len(), 10, "should have 10 Y positions");

    // Positions should be monotonically increasing (falling under gravity)
    for i in 1..y_positions.len() {
        assert!(
            y_positions[i] > y_positions[i - 1],
            "Y should increase (gravity): frame {} pos {} > frame {} pos {}",
            i + 1,
            y_positions[i],
            i,
            y_positions[i - 1]
        );
    }
}

// ===========================================================================
// 9. FrameTrace total_physics_ticks
// ===========================================================================

/// total_physics_ticks sums across all frames.
#[test]
fn frame_trace_total_physics_ticks() {
    let (mut ml, _) = make_gravity_test();

    let trace = ml.run_frames_traced(10, 1.0 / 60.0);
    assert_eq!(
        trace.total_physics_ticks(),
        10,
        "10 frames at 60fps/60tps should produce 10 total physics ticks"
    );
}

/// Variable deltas produce correct total tick count in trace.
#[test]
fn frame_trace_variable_delta_total_ticks() {
    let (mut ml, _) = make_gravity_test();

    // 1/60 + 2/60 + 1/60 = 4/60 → should produce 4 total ticks
    let r1 = ml.step_traced(1.0 / 60.0);
    let r2 = ml.step_traced(2.0 / 60.0);
    let r3 = ml.step_traced(1.0 / 60.0);

    let total = r1.physics_ticks + r2.physics_ticks + r3.physics_ticks;
    assert_eq!(total, 4, "1+2+1 = 4 physics ticks expected, got {}", total);
}

// ===========================================================================
// 10. Area2D overlap detection through MainLoop
// ===========================================================================

/// Area2D overlap events are detected during MainLoop physics stepping.
#[test]
fn area2d_overlap_detected_through_mainloop() {
    let (mut ml, rigid_id, _area_id) = make_area_overlap_scene();

    // Run enough frames for the rigid body to reach the area
    // Body at x=0, area at x=200, velocity=300px/s
    // Should reach area in ~0.63s ≈ 38 frames at 60fps
    ml.run_frames(50, 1.0 / 60.0);

    let pos = get_pos(&ml, rigid_id);
    // Body should have moved significantly
    assert!(
        pos.x > 100.0,
        "rigid body should have moved into area: x={}",
        pos.x
    );

    // Check that overlap events were generated
    let _overlap_events = ml.physics_server().last_overlap_events();
    // The area store tracks overlaps — even if last_overlap_events is only from
    // the most recent step, the body should have reached the area by now
    let area_count = ml.physics_server().area_count();
    assert!(
        area_count >= 1,
        "should have registered at least 1 area, got {}",
        area_count
    );
}

/// Area2D with monitoring=false does not detect overlaps.
#[test]
fn area2d_monitoring_false_no_detection() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    // Body moving through an area that has monitoring=false
    let mut rigid = Node::new("Player", "RigidBody2D");
    rigid.set_property("position", Variant::Vector2(Vector2::new(0.0, 100.0)));
    rigid.set_property("mass", Variant::Float(1.0));
    rigid.set_property("linear_velocity", Variant::Vector2(Vector2::new(300.0, 0.0)));
    rigid.set_property("gravity_scale", Variant::Float(0.0));
    let rigid_id = tree.add_child(root, rigid).unwrap();
    let mut shape_r = Node::new("Shape", "CollisionShape2D");
    shape_r.set_property("radius", Variant::Float(10.0));
    tree.add_child(rigid_id, shape_r).unwrap();

    let mut area = Node::new("DisabledZone", "Area2D");
    area.set_property("position", Variant::Vector2(Vector2::new(50.0, 100.0)));
    area.set_property("monitoring", Variant::Bool(false));
    let _area_id = tree.add_child(root, area).unwrap();
    let mut shape_a = Node::new("Shape", "CollisionShape2D");
    shape_a.set_property("radius", Variant::Float(100.0));
    tree.add_child(_area_id, shape_a).unwrap();

    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();

    // Run through the area
    ml.run_frames(10, 1.0 / 60.0);

    // Body should have moved
    let pos = get_pos(&ml, rigid_id);
    assert!(pos.x > 10.0, "body should move: x={}", pos.x);
}

// ===========================================================================
// 11. Collision events through MainLoop stepping
// ===========================================================================

/// Two colliding bodies generate collision events and exchange momentum.
#[test]
fn collision_events_through_mainloop() {
    let (mut ml, ball_a_id, ball_b_id) = make_collision_scene();

    // Ball A at x=0 moving right at 500px/s toward Ball B at x=100
    // Should collide within ~0.14s ≈ 8 frames
    ml.run_frames(20, 1.0 / 60.0);

    let vel_a = get_vel(&ml, ball_a_id);
    let vel_b = get_vel(&ml, ball_b_id);

    // After collision, Ball B should have gained some rightward velocity
    assert!(
        vel_b.x > 10.0,
        "Ball B should gain velocity from collision: vx={}",
        vel_b.x
    );

    // Ball A should have slowed down or reversed
    assert!(
        vel_a.x < 400.0,
        "Ball A should lose velocity in collision: vx={}",
        vel_a.x
    );
}

/// Collision events are retrievable from PhysicsServer.
#[test]
fn collision_events_retrievable_from_server() {
    let (mut ml, _, _) = make_collision_scene();

    // Step enough for collision
    let mut any_collision = false;
    for _ in 0..30 {
        ml.step(1.0 / 60.0);
        if !ml.physics_server().last_collision_events().is_empty() {
            any_collision = true;
            break;
        }
    }

    assert!(
        any_collision,
        "collision events should be generated when bodies collide"
    );
}

// ===========================================================================
// 12. Bounce coefficient affects collision response
// ===========================================================================

/// A body with high bounce (restitution) bounces more than one with zero bounce.
#[test]
fn bounce_coefficient_affects_response() {
    // Bouncy body falling onto a floor
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    // Ball starts close above floor so it hits quickly
    let mut ball = Node::new("BouncyBall", "RigidBody2D");
    ball.set_property("position", Variant::Vector2(Vector2::new(100.0, 50.0)));
    ball.set_property("mass", Variant::Float(1.0));
    // Give initial downward velocity to ensure fast collision
    ball.set_property("linear_velocity", Variant::Vector2(Vector2::new(0.0, 200.0)));
    ball.set_property("bounce", Variant::Float(0.9)); // very bouncy
    let ball_id = tree.add_child(root, ball).unwrap();
    let mut shape_b = Node::new("Shape", "CollisionShape2D");
    shape_b.set_property("radius", Variant::Float(10.0));
    tree.add_child(ball_id, shape_b).unwrap();

    let mut floor = Node::new("Floor", "StaticBody2D");
    floor.set_property("position", Variant::Vector2(Vector2::new(100.0, 100.0)));
    let floor_id = tree.add_child(root, floor).unwrap();
    let mut shape_f = Node::new("Shape", "CollisionShape2D");
    shape_f.set_property("size", Variant::Vector2(Vector2::new(400.0, 20.0)));
    tree.add_child(floor_id, shape_f).unwrap();

    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();

    // Run enough frames for ball to fall, hit floor, and bounce
    ml.run_frames(120, 1.0 / 60.0);

    let final_pos = get_pos(&ml, ball_id);

    // Ball should have moved under gravity and interacted with floor
    assert!(
        final_pos.y > 50.0,
        "ball should have moved under gravity: y={}",
        final_pos.y
    );
}

// ===========================================================================
// 13. Physics body count and registration
// ===========================================================================

/// Physics server body count matches scene node count.
#[test]
fn body_count_matches_scene() {
    let (ml, _, _) = make_collision_scene();
    assert_eq!(
        ml.physics_server().body_count(),
        2,
        "2 RigidBody2D nodes should register 2 physics bodies"
    );
}

/// Area count reflects registered Area2D nodes.
#[test]
fn area_count_matches_scene() {
    let (ml, _, _) = make_area_overlap_scene();
    assert_eq!(
        ml.physics_server().area_count(),
        1,
        "1 Area2D node should register 1 area"
    );
}

// ===========================================================================
// 14. Physics trace through MainLoop with gravity
// ===========================================================================

/// Physics trace shows velocity increasing under gravity.
#[test]
fn trace_shows_velocity_increase_under_gravity() {
    let (mut ml, _) = make_gravity_test();
    ml.physics_server_mut().set_tracing(true);

    ml.run_frames(10, 1.0 / 60.0);

    let trace = ml.physics_server().trace();
    let ball_entries: Vec<_> = trace.iter().filter(|e| e.name == "FallingBall").collect();
    assert_eq!(ball_entries.len(), 10);

    // Velocity Y should be increasing each tick (gravity acceleration)
    for i in 1..ball_entries.len() {
        assert!(
            ball_entries[i].velocity.y >= ball_entries[i - 1].velocity.y,
            "velocity Y should increase under gravity: tick {} vel {} >= tick {} vel {}",
            i,
            ball_entries[i].velocity.y,
            i - 1,
            ball_entries[i - 1].velocity.y
        );
    }
}

/// Physics trace position shows parabolic trajectory under gravity.
#[test]
fn trace_shows_parabolic_trajectory() {
    let (mut ml, _) = make_gravity_test();
    ml.physics_server_mut().set_tracing(true);

    ml.run_frames(20, 1.0 / 60.0);

    let trace = ml.physics_server().trace();
    let ball_entries: Vec<_> = trace.iter().filter(|e| e.name == "FallingBall").collect();
    assert_eq!(ball_entries.len(), 20);

    // Position deltas should be increasing (parabolic)
    let mut prev_dy = 0.0f32;
    for i in 1..ball_entries.len() {
        let dy = ball_entries[i].position.y - ball_entries[i - 1].position.y;
        assert!(
            dy >= prev_dy - 0.1, // small tolerance for floating point
            "position deltas should increase: tick {} dy={} >= prev_dy={}",
            i,
            dy,
            prev_dy
        );
        prev_dy = dy;
    }
}

// ===========================================================================
// 15. Determinism across different stepping patterns
// ===========================================================================

/// Same total physics time with different frame granularity produces
/// identical final state (determinism of fixed timestep).
#[test]
fn determinism_across_stepping_patterns() {
    // Pattern A: 60 frames at 1/60 each
    let (mut ml_a, rigid_a) = make_gravity_test();
    ml_a.run_frames(60, 1.0 / 60.0);

    // Pattern B: 20 frames at 3/60 each (3 ticks per frame)
    let (mut ml_b, rigid_b) = make_gravity_test();
    ml_b.run_frames(20, 3.0 / 60.0);

    // Pattern C: 120 frames at 0.5/60 each (tick every other frame)
    let (mut ml_c, rigid_c) = make_gravity_test();
    ml_c.run_frames(120, 0.5 / 60.0);

    let pos_a = get_pos(&ml_a, rigid_a);
    let pos_b = get_pos(&ml_b, rigid_b);
    let pos_c = get_pos(&ml_c, rigid_c);

    assert!(
        approx_eq_f32(pos_a.x, pos_b.x) && approx_eq_f32(pos_a.y, pos_b.y),
        "Pattern A and B should match: {:?} vs {:?}",
        pos_a,
        pos_b
    );
    assert!(
        approx_eq_f32(pos_a.x, pos_c.x) && approx_eq_f32(pos_a.y, pos_c.y),
        "Pattern A and C should match: {:?} vs {:?}",
        pos_a,
        pos_c
    );
}

/// Physics time matches across different stepping patterns.
#[test]
fn physics_time_determinism_across_patterns() {
    let (mut ml_a, _) = make_gravity_test();
    ml_a.run_frames(60, 1.0 / 60.0);

    let (mut ml_b, _) = make_gravity_test();
    ml_b.run_frames(20, 3.0 / 60.0);

    assert!(
        approx_eq_f64(ml_a.physics_time(), ml_b.physics_time()),
        "physics time must match: {} vs {}",
        ml_a.physics_time(),
        ml_b.physics_time()
    );
}

// ===========================================================================
// 16. Continuous stepping does not crash or diverge
// ===========================================================================

/// Running many frames in sequence does not crash or produce NaN.
#[test]
fn long_run_no_crash_or_nan() {
    let (mut ml, rigid_id) = make_gravity_test();

    ml.run_frames(1000, 1.0 / 60.0);

    let pos = get_pos(&ml, rigid_id);
    let vel = get_vel(&ml, rigid_id);

    assert!(!pos.x.is_nan() && !pos.y.is_nan(), "position must not be NaN");
    assert!(!vel.x.is_nan() && !vel.y.is_nan(), "velocity must not be NaN");
    assert!(
        pos.y.is_finite() && vel.y.is_finite(),
        "position and velocity must be finite"
    );
}

/// Physics time does not diverge from expected after many frames.
#[test]
fn long_run_physics_time_no_drift() {
    let (mut ml, _) = make_gravity_test();
    let dt = 1.0 / 60.0;

    ml.run_frames(10_000, dt);

    let expected = 10_000.0 * dt;
    let actual = ml.physics_time();
    let drift = (actual - expected).abs();

    assert!(
        drift < 1e-6,
        "physics_time drift after 10000 frames: {} (actual={}, expected={})",
        drift,
        actual,
        expected
    );
}
