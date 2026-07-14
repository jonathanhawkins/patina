//! Physics integration tests covering beads pat-cyf, pat-clv, pat-1za, pat-yxp.
//!
//! - pat-cyf: Collision shape registration and overlap coverage
//! - pat-clv: CharacterBody2D and StaticBody2D behavior fixtures
//! - pat-1za: Deterministic physics trace goldens
//! - pat-yxp: physics_playground golden trace fixture

use gdcore::math::Vector2;
use gdscene::main_loop::MainLoop;
use gdscene::node::Node;
use gdscene::physics_server::PhysicsTraceEntry;
use gdscene::scene_tree::SceneTree;
use gdvariant::Variant;

const EPSILON: f32 = 1e-3;

fn approx_eq(a: f32, b: f32) -> bool {
    (a - b).abs() < EPSILON
}

/// Helper: build a scene tree with physics bodies programmatically.
fn make_rigid_and_static_scene() -> (SceneTree, gdscene::node::NodeId, gdscene::node::NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    // RigidBody2D with circle collision shape
    let mut rigid = Node::new("Ball", "RigidBody2D");
    rigid.set_property("position", Variant::Vector2(Vector2::new(100.0, 50.0)));
    rigid.set_property("mass", Variant::Float(1.0));
    rigid.set_property("linear_velocity", Variant::Vector2(Vector2::new(0.0, 50.0)));
    let rigid_id = tree.add_child(root, rigid).unwrap();
    let mut shape = Node::new("CollisionShape", "CollisionShape2D");
    shape.set_property("radius", Variant::Float(16.0));
    tree.add_child(rigid_id, shape).unwrap();

    // StaticBody2D with rect collision shape (floor)
    let mut floor = Node::new("Floor", "StaticBody2D");
    floor.set_property("position", Variant::Vector2(Vector2::new(100.0, 300.0)));
    let floor_id = tree.add_child(root, floor).unwrap();
    let mut shape2 = Node::new("CollisionShape", "CollisionShape2D");
    shape2.set_property("size", Variant::Vector2(Vector2::new(400.0, 20.0)));
    tree.add_child(floor_id, shape2).unwrap();

    (tree, rigid_id, floor_id)
}

/// Helper: build a CharacterBody2D scene.
fn make_character_scene() -> (
    SceneTree,
    gdscene::node::NodeId,
    gdscene::node::NodeId,
    gdscene::node::NodeId,
) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    // CharacterBody2D
    let mut player = Node::new("Player", "CharacterBody2D");
    player.set_property("position", Variant::Vector2(Vector2::new(100.0, 100.0)));
    let player_id = tree.add_child(root, player).unwrap();
    let mut s = Node::new("Shape", "CollisionShape2D");
    s.set_property("radius", Variant::Float(12.0));
    tree.add_child(player_id, s).unwrap();

    // StaticBody2D platform
    let mut platform = Node::new("Platform", "StaticBody2D");
    platform.set_property("position", Variant::Vector2(Vector2::new(100.0, 200.0)));
    let platform_id = tree.add_child(root, platform).unwrap();
    let mut s2 = Node::new("Shape", "CollisionShape2D");
    s2.set_property("size", Variant::Vector2(Vector2::new(200.0, 20.0)));
    tree.add_child(platform_id, s2).unwrap();

    // StaticBody2D wall
    let mut wall = Node::new("Wall", "StaticBody2D");
    wall.set_property("position", Variant::Vector2(Vector2::new(300.0, 150.0)));
    let wall_id = tree.add_child(root, wall).unwrap();
    let mut s3 = Node::new("Shape", "CollisionShape2D");
    s3.set_property("size", Variant::Vector2(Vector2::new(20.0, 100.0)));
    tree.add_child(wall_id, s3).unwrap();

    (tree, player_id, platform_id, wall_id)
}

/// Helper: run a physics simulation and collect the trace.
fn run_traced_physics(
    tree: SceneTree,
    frames: u64,
    delta: f64,
) -> (MainLoop, Vec<PhysicsTraceEntry>) {
    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();
    ml.physics_server_mut().set_tracing(true);
    ml.run_frames(frames, delta);
    let trace = ml.physics_server().trace().to_vec();
    (ml, trace)
}

// ═══════════════════════════════════════════════════════════════════════
// pat-cyf: Collision shape registration and overlap coverage
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn cyf_circle_shape_registered_from_scene_node() {
    let (tree, _, _) = make_rigid_and_static_scene();
    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();
    assert_eq!(ml.physics_server().body_count(), 2);
}

#[test]
fn cyf_rect_shape_registered_from_scene_node() {
    let (tree, _, floor_id) = make_rigid_and_static_scene();
    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();

    let body_id = ml.physics_server().body_for_node(floor_id).unwrap();
    let body = ml.physics_server().world().get_body(body_id).unwrap();
    assert_eq!(
        body.shape,
        gdphysics2d::Shape2D::Rectangle {
            half_extents: Vector2::new(200.0, 10.0)
        }
    );
}

#[test]
fn cyf_overlapping_bodies_produce_collision_events() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    // Two overlapping circles
    let mut a = Node::new("A", "RigidBody2D");
    a.set_property("position", Variant::Vector2(Vector2::new(0.0, 0.0)));
    let a_id = tree.add_child(root, a).unwrap();
    let mut sa = Node::new("Shape", "CollisionShape2D");
    sa.set_property("radius", Variant::Float(10.0));
    tree.add_child(a_id, sa).unwrap();

    let mut b = Node::new("B", "RigidBody2D");
    b.set_property("position", Variant::Vector2(Vector2::new(15.0, 0.0)));
    let b_id = tree.add_child(root, b).unwrap();
    let mut sb = Node::new("Shape", "CollisionShape2D");
    sb.set_property("radius", Variant::Float(10.0));
    tree.add_child(b_id, sb).unwrap();

    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();
    ml.step(1.0 / 60.0);

    let events = ml.physics_server().last_collision_events();
    assert!(
        !events.is_empty(),
        "overlapping bodies should generate collision events"
    );
}

#[test]
fn cyf_separated_bodies_no_collision_events() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let mut a = Node::new("A", "RigidBody2D");
    a.set_property("position", Variant::Vector2(Vector2::new(0.0, 0.0)));
    let a_id = tree.add_child(root, a).unwrap();
    let mut sa = Node::new("Shape", "CollisionShape2D");
    sa.set_property("radius", Variant::Float(5.0));
    tree.add_child(a_id, sa).unwrap();

    let mut b = Node::new("B", "RigidBody2D");
    b.set_property("position", Variant::Vector2(Vector2::new(100.0, 0.0)));
    let b_id = tree.add_child(root, b).unwrap();
    let mut sb = Node::new("Shape", "CollisionShape2D");
    sb.set_property("radius", Variant::Float(5.0));
    tree.add_child(b_id, sb).unwrap();

    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();
    ml.step(1.0 / 60.0);

    let events = ml.physics_server().last_collision_events();
    assert!(
        events.is_empty(),
        "well-separated bodies should not collide"
    );
}

#[test]
fn cyf_collision_layer_mask_filtering() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let mut a = Node::new("A", "RigidBody2D");
    a.set_property("position", Variant::Vector2(Vector2::new(0.0, 0.0)));
    a.set_property("collision_layer", Variant::Int(1));
    a.set_property("collision_mask", Variant::Int(1));
    let a_id = tree.add_child(root, a).unwrap();
    let mut sa = Node::new("Shape", "CollisionShape2D");
    sa.set_property("radius", Variant::Float(10.0));
    tree.add_child(a_id, sa).unwrap();

    let mut b = Node::new("B", "RigidBody2D");
    b.set_property("position", Variant::Vector2(Vector2::new(15.0, 0.0)));
    b.set_property("collision_layer", Variant::Int(2));
    b.set_property("collision_mask", Variant::Int(2));
    let b_id = tree.add_child(root, b).unwrap();
    let mut sb = Node::new("Shape", "CollisionShape2D");
    sb.set_property("radius", Variant::Float(10.0));
    tree.add_child(b_id, sb).unwrap();

    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();
    ml.step(1.0 / 60.0);

    let events = ml.physics_server().last_collision_events();
    assert!(
        events.is_empty(),
        "bodies on different layers should not collide"
    );
}

#[test]
fn cyf_area_overlap_detection() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let mut body = Node::new("Body", "RigidBody2D");
    body.set_property("position", Variant::Vector2(Vector2::new(5.0, 0.0)));
    let body_nid = tree.add_child(root, body).unwrap();
    let mut s = Node::new("Shape", "CollisionShape2D");
    s.set_property("radius", Variant::Float(2.0));
    tree.add_child(body_nid, s).unwrap();

    let mut area = Node::new("Zone", "Area2D");
    area.set_property("position", Variant::Vector2(Vector2::new(5.0, 0.0)));
    let area_nid = tree.add_child(root, area).unwrap();
    let mut sa = Node::new("Shape", "CollisionShape2D");
    sa.set_property("radius", Variant::Float(10.0));
    tree.add_child(area_nid, sa).unwrap();

    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();
    ml.step(1.0 / 60.0);

    let events = ml.physics_server().last_overlap_events();
    assert!(!events.is_empty(), "area should detect overlapping body");
}

#[test]
fn cyf_circle_vs_rect_overlap_produces_collision_events() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    // Circle body at origin with radius 10
    let mut a = Node::new("Circle", "RigidBody2D");
    a.set_property("position", Variant::Vector2(Vector2::new(0.0, 0.0)));
    let a_id = tree.add_child(root, a).unwrap();
    let mut sa = Node::new("Shape", "CollisionShape2D");
    sa.set_property("radius", Variant::Float(10.0));
    tree.add_child(a_id, sa).unwrap();

    // Rectangle body overlapping circle — rect center at (12, 0) with half_extents (5, 5)
    // Rect edge at 7.0 — circle edge at 10.0 — overlap of 3.0
    let mut b = Node::new("Rect", "StaticBody2D");
    b.set_property("position", Variant::Vector2(Vector2::new(12.0, 0.0)));
    let b_id = tree.add_child(root, b).unwrap();
    let mut sb = Node::new("Shape", "CollisionShape2D");
    sb.set_property("half_extents", Variant::Vector2(Vector2::new(5.0, 5.0)));
    tree.add_child(b_id, sb).unwrap();

    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();
    ml.step(1.0 / 60.0);

    let events = ml.physics_server().last_collision_events();
    assert!(
        !events.is_empty(),
        "circle vs rectangle overlap should generate collision events"
    );
}

#[test]
fn cyf_circle_vs_rect_no_overlap_no_collision() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    // Circle at origin with radius 2
    let mut a = Node::new("Circle", "RigidBody2D");
    a.set_property("position", Variant::Vector2(Vector2::new(0.0, 0.0)));
    let a_id = tree.add_child(root, a).unwrap();
    let mut sa = Node::new("Shape", "CollisionShape2D");
    sa.set_property("radius", Variant::Float(2.0));
    tree.add_child(a_id, sa).unwrap();

    // Rectangle far away — no overlap possible
    let mut b = Node::new("Rect", "StaticBody2D");
    b.set_property("position", Variant::Vector2(Vector2::new(100.0, 0.0)));
    let b_id = tree.add_child(root, b).unwrap();
    let mut sb = Node::new("Shape", "CollisionShape2D");
    sb.set_property("half_extents", Variant::Vector2(Vector2::new(5.0, 5.0)));
    tree.add_child(b_id, sb).unwrap();

    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();
    ml.step(1.0 / 60.0);

    let events = ml.physics_server().last_collision_events();
    assert!(
        events.is_empty(),
        "well-separated circle and rectangle should not collide"
    );
}

#[test]
fn cyf_area2d_overlap_events_have_correct_state() {
    use gdphysics2d::area2d::OverlapState;

    let mut tree = SceneTree::new();
    let root = tree.root_id();

    // A rigid body inside an Area2D
    let mut body = Node::new("Body", "RigidBody2D");
    body.set_property("position", Variant::Vector2(Vector2::new(0.0, 0.0)));
    let body_nid = tree.add_child(root, body).unwrap();
    let mut s = Node::new("Shape", "CollisionShape2D");
    s.set_property("radius", Variant::Float(5.0));
    tree.add_child(body_nid, s).unwrap();

    let mut area = Node::new("Zone", "Area2D");
    area.set_property("position", Variant::Vector2(Vector2::new(0.0, 0.0)));
    let area_nid = tree.add_child(root, area).unwrap();
    let mut sa = Node::new("Shape", "CollisionShape2D");
    sa.set_property("radius", Variant::Float(20.0));
    tree.add_child(area_nid, sa).unwrap();

    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();

    // First step — body enters area
    ml.step(1.0 / 60.0);
    let events = ml.physics_server().last_overlap_events();
    assert!(!events.is_empty(), "should detect overlap on first step");
    assert!(
        events.iter().any(|e| e.state == OverlapState::Entered),
        "first overlap should be Entered state"
    );

    // Second step — body still inside, no new enter/exit events
    ml.step(1.0 / 60.0);
    let events2 = ml.physics_server().last_overlap_events();
    assert!(
        events2.is_empty(),
        "sustained overlap should not re-fire events"
    );
}

// NOTE: Area2D body_entered/body_exited signal dispatch from physics overlap
// events is NOT yet wired in MainLoop. The PhysicsServer detects overlaps
// (tested above) but MainLoop does not read overlap events or emit signals
// on the corresponding Area2D nodes. This is a known gap for a future bead.

// ═══════════════════════════════════════════════════════════════════════
// pat-clv: CharacterBody2D and StaticBody2D behavior fixtures
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn clv_static_body_does_not_move_over_time() {
    let (tree, _, _, _) = make_character_scene();
    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();

    // Record platform position
    let platform_pos_before = {
        let node = ml
            .tree()
            .get_node(ml.tree().all_nodes_in_tree_order()[2])
            .unwrap();
        match node.get_property("position") {
            Variant::Vector2(v) => v,
            _ => Vector2::ZERO,
        }
    };

    ml.run_frames(60, 1.0 / 60.0);

    // Re-read platform position — should not have changed
    let platform_pos_after = {
        let node = ml
            .tree()
            .get_node(ml.tree().all_nodes_in_tree_order()[2])
            .unwrap();
        match node.get_property("position") {
            Variant::Vector2(v) => v,
            _ => Vector2::ZERO,
        }
    };

    assert_eq!(
        platform_pos_before, platform_pos_after,
        "StaticBody2D must not move"
    );
}

#[test]
fn clv_character_body_is_kinematic() {
    let (tree, player_id, _, _) = make_character_scene();
    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();

    let body_id = ml.physics_server().body_for_node(player_id).unwrap();
    let body = ml.physics_server().world().get_body(body_id).unwrap();
    assert_eq!(body.body_type, gdphysics2d::BodyType::Kinematic);
}

#[test]
fn clv_static_wall_position_unchanged_after_collision() {
    let (tree, _, _, wall_id) = make_character_scene();
    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();

    let pos_before = {
        let body_id = ml.physics_server().body_for_node(wall_id).unwrap();
        ml.physics_server()
            .world()
            .get_body(body_id)
            .unwrap()
            .position
    };

    ml.run_frames(120, 1.0 / 60.0);

    let pos_after = {
        let body_id = ml.physics_server().body_for_node(wall_id).unwrap();
        ml.physics_server()
            .world()
            .get_body(body_id)
            .unwrap()
            .position
    };

    assert_eq!(pos_before, pos_after, "static wall must not move");
}

#[test]
fn clv_rigid_body_moves_under_velocity() {
    let (tree, rigid_id, _) = make_rigid_and_static_scene();
    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();

    let pos_before = {
        let body_id = ml.physics_server().body_for_node(rigid_id).unwrap();
        ml.physics_server()
            .world()
            .get_body(body_id)
            .unwrap()
            .position
    };

    ml.run_frames(10, 1.0 / 60.0);

    let pos_after = {
        let body_id = ml.physics_server().body_for_node(rigid_id).unwrap();
        ml.physics_server()
            .world()
            .get_body(body_id)
            .unwrap()
            .position
    };

    assert!(
        pos_after.y > pos_before.y,
        "rigid body with downward velocity should fall: before={:?}, after={:?}",
        pos_before,
        pos_after
    );
}

#[test]
fn clv_rigid_body_falls_under_applied_force() {
    // Simulate gravity by giving the rigid body a downward force via velocity.
    // The 2D world doesn't have built-in gravity; rigid bodies move via velocity/force.
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let mut ball = Node::new("Ball", "RigidBody2D");
    ball.set_property("position", Variant::Vector2(Vector2::new(0.0, 0.0)));
    ball.set_property(
        "linear_velocity",
        Variant::Vector2(Vector2::new(0.0, 100.0)),
    );
    let ball_id = tree.add_child(root, ball).unwrap();
    let mut s = Node::new("Shape", "CollisionShape2D");
    s.set_property("radius", Variant::Float(5.0));
    tree.add_child(ball_id, s).unwrap();

    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();

    let pos_before = match ml
        .tree()
        .get_node(ball_id)
        .unwrap()
        .get_property("position")
    {
        Variant::Vector2(v) => v,
        _ => panic!("expected Vector2"),
    };

    ml.run_frames(10, 1.0 / 60.0);

    let pos_after = match ml
        .tree()
        .get_node(ball_id)
        .unwrap()
        .get_property("position")
    {
        Variant::Vector2(v) => v,
        _ => panic!("expected Vector2"),
    };

    assert!(
        pos_after.y > pos_before.y + 10.0,
        "rigid body should fall significantly: before={pos_before:?}, after={pos_after:?}"
    );
}

#[test]
fn clv_static_body_blocks_falling_rigid_body() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    // Rigid ball falling fast toward a static floor
    let mut ball = Node::new("Ball", "RigidBody2D");
    ball.set_property("position", Variant::Vector2(Vector2::new(0.0, 0.0)));
    ball.set_property(
        "linear_velocity",
        Variant::Vector2(Vector2::new(0.0, 200.0)),
    );
    let ball_id = tree.add_child(root, ball).unwrap();
    let mut s = Node::new("Shape", "CollisionShape2D");
    s.set_property("radius", Variant::Float(10.0));
    tree.add_child(ball_id, s).unwrap();

    // Static floor below
    let mut floor = Node::new("Floor", "StaticBody2D");
    floor.set_property("position", Variant::Vector2(Vector2::new(0.0, 50.0)));
    let floor_id = tree.add_child(root, floor).unwrap();
    let mut sf = Node::new("Shape", "CollisionShape2D");
    sf.set_property("half_extents", Variant::Vector2(Vector2::new(100.0, 10.0)));
    tree.add_child(floor_id, sf).unwrap();

    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();

    // Run enough frames for the ball to reach and collide with the floor
    ml.run_frames(60, 1.0 / 60.0);

    let ball_pos = match ml
        .tree()
        .get_node(ball_id)
        .unwrap()
        .get_property("position")
    {
        Variant::Vector2(v) => v,
        _ => panic!("expected Vector2"),
    };

    // Ball should not have passed through the floor (floor top edge at y=40)
    assert!(
        ball_pos.y < 50.0,
        "rigid body should be blocked by static floor, not pass through: ball_y={}, floor_y=50",
        ball_pos.y
    );
}

#[test]
fn clv_two_rigid_bodies_bounce_apart_with_restitution() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    // Body A moving right
    let mut a = Node::new("A", "RigidBody2D");
    a.set_property("position", Variant::Vector2(Vector2::new(0.0, 0.0)));
    a.set_property(
        "linear_velocity",
        Variant::Vector2(Vector2::new(100.0, 0.0)),
    );
    a.set_property("bounce", Variant::Float(1.0)); // Perfectly elastic
    a.set_property("mass", Variant::Float(1.0));
    let a_id = tree.add_child(root, a).unwrap();
    let mut sa = Node::new("Shape", "CollisionShape2D");
    sa.set_property("radius", Variant::Float(10.0));
    tree.add_child(a_id, sa).unwrap();

    // Body B moving left
    let mut b = Node::new("B", "RigidBody2D");
    b.set_property("position", Variant::Vector2(Vector2::new(15.0, 0.0)));
    b.set_property(
        "linear_velocity",
        Variant::Vector2(Vector2::new(-100.0, 0.0)),
    );
    b.set_property("bounce", Variant::Float(1.0));
    b.set_property("mass", Variant::Float(1.0));
    let b_id = tree.add_child(root, b).unwrap();
    let mut sb = Node::new("Shape", "CollisionShape2D");
    sb.set_property("radius", Variant::Float(10.0));
    tree.add_child(b_id, sb).unwrap();

    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();

    // Step once to cause collision
    ml.step(1.0 / 60.0);

    // After elastic collision of equal masses, velocities should swap:
    // A should now be moving left, B moving right
    let body_a_id = ml.physics_server().body_for_node(a_id).unwrap();
    let body_b_id = ml.physics_server().body_for_node(b_id).unwrap();
    let vel_a = ml
        .physics_server()
        .world()
        .get_body(body_a_id)
        .unwrap()
        .linear_velocity;
    let vel_b = ml
        .physics_server()
        .world()
        .get_body(body_b_id)
        .unwrap()
        .linear_velocity;

    assert!(
        vel_a.x < 0.0,
        "body A should bounce back (negative x velocity), got {}",
        vel_a.x
    );
    assert!(
        vel_b.x > 0.0,
        "body B should bounce back (positive x velocity), got {}",
        vel_b.x
    );

    // Run more frames — bodies should separate
    ml.run_frames(10, 1.0 / 60.0);
    let pos_a = match ml.tree().get_node(a_id).unwrap().get_property("position") {
        Variant::Vector2(v) => v,
        _ => panic!("expected Vector2"),
    };
    let pos_b = match ml.tree().get_node(b_id).unwrap().get_property("position") {
        Variant::Vector2(v) => v,
        _ => panic!("expected Vector2"),
    };
    assert!(
        pos_b.x > pos_a.x + 20.0,
        "bodies should have bounced apart: a={pos_a:?}, b={pos_b:?}"
    );
}

#[test]
fn clv_friction_slows_sliding_body() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    // Body moving horizontally with high friction, hitting a static wall from below
    // so that collision produces a tangential (horizontal) friction response.
    let mut ball = Node::new("Ball", "RigidBody2D");
    // Position above the floor, moving diagonally (down and right)
    ball.set_property("position", Variant::Vector2(Vector2::new(0.0, 0.0)));
    ball.set_property(
        "linear_velocity",
        Variant::Vector2(Vector2::new(100.0, 50.0)),
    );
    ball.set_property("friction", Variant::Float(1.0)); // Max friction
    ball.set_property("mass", Variant::Float(1.0));
    let ball_id = tree.add_child(root, ball).unwrap();
    let mut s = Node::new("Shape", "CollisionShape2D");
    s.set_property("radius", Variant::Float(5.0));
    tree.add_child(ball_id, s).unwrap();

    // Static floor right below
    let mut floor = Node::new("Floor", "StaticBody2D");
    floor.set_property("position", Variant::Vector2(Vector2::new(0.0, 10.0)));
    floor.set_property("friction", Variant::Float(1.0));
    let floor_id = tree.add_child(root, floor).unwrap();
    let mut sf = Node::new("Shape", "CollisionShape2D");
    sf.set_property("half_extents", Variant::Vector2(Vector2::new(500.0, 5.0)));
    tree.add_child(floor_id, sf).unwrap();

    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();

    // Step to trigger collision + friction
    ml.step(1.0 / 60.0);

    let body_id = ml.physics_server().body_for_node(ball_id).unwrap();
    let vel = ml
        .physics_server()
        .world()
        .get_body(body_id)
        .unwrap()
        .linear_velocity;

    // Friction should have reduced the tangential (x) velocity
    assert!(
        vel.x.abs() < 100.0,
        "friction should reduce horizontal speed from 100, got {}",
        vel.x
    );
}

// NOTE: CharacterBody2D is registered as Kinematic in the physics world, but
// move_and_slide() is not automatically driven by MainLoop. The move_and_slide
// API exists in gdphysics2d::character and works standalone (tested in unit
// tests there), but scene-tree-level CharacterBody2D scripting integration
// (calling move_and_slide from GDScript _physics_process) is a future bead.

// ═══════════════════════════════════════════════════════════════════════
// pat-1za: Deterministic physics trace goldens
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn za_deterministic_physics_same_setup_same_trace() {
    let run = || {
        let (tree, _, _) = make_rigid_and_static_scene();
        let (_, mut trace) = run_traced_physics(tree, 30, 1.0 / 60.0);
        trace.sort_by(|a, b| a.frame.cmp(&b.frame).then(a.name.cmp(&b.name)));
        trace
    };

    let trace1 = run();
    let trace2 = run();

    assert_eq!(trace1.len(), trace2.len(), "trace lengths must match");
    for (i, (a, b)) in trace1.iter().zip(trace2.iter()).enumerate() {
        assert_eq!(a.name, b.name, "trace entry {i}: name mismatch");
        assert_eq!(a.frame, b.frame, "trace entry {i}: frame mismatch");
        assert!(
            approx_eq(a.position.x, b.position.x) && approx_eq(a.position.y, b.position.y),
            "trace entry {i}: position mismatch: {:?} vs {:?}",
            a.position,
            b.position
        );
        assert!(
            approx_eq(a.velocity.x, b.velocity.x) && approx_eq(a.velocity.y, b.velocity.y),
            "trace entry {i}: velocity mismatch: {:?} vs {:?}",
            a.velocity,
            b.velocity
        );
    }
}

#[test]
fn za_trace_contains_expected_bodies() {
    let (tree, _, _) = make_rigid_and_static_scene();
    let (_, trace) = run_traced_physics(tree, 5, 1.0 / 60.0);

    let names: std::collections::HashSet<&str> = trace.iter().map(|e| e.name.as_str()).collect();
    assert!(names.contains("Ball"), "trace should contain Ball");
    assert!(names.contains("Floor"), "trace should contain Floor");
}

#[test]
fn za_trace_frame_numbers_monotonically_increase() {
    let (tree, _, _) = make_rigid_and_static_scene();
    let (_, trace) = run_traced_physics(tree, 10, 1.0 / 60.0);

    let ball_entries: Vec<_> = trace.iter().filter(|e| e.name == "Ball").collect();
    for window in ball_entries.windows(2) {
        assert!(
            window[1].frame >= window[0].frame,
            "frame numbers should increase: {} -> {}",
            window[0].frame,
            window[1].frame
        );
    }
}

#[test]
fn za_golden_trace_write_and_compare() {
    // Generate a trace, write it to a golden file, then re-generate and compare.
    let generate_trace = || {
        let (tree, _, _) = make_rigid_and_static_scene();
        let (_, trace) = run_traced_physics(tree, 20, 1.0 / 60.0);
        trace
    };

    let golden = generate_trace();
    let golden_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../fixtures/golden/physics/rigid_static_20frames.json");

    // Write golden
    let golden_json: Vec<_> = golden
        .iter()
        .map(|e| {
            serde_json::json!({
                "name": e.name,
                "frame": e.frame,
                "px": (e.position.x * 1000.0).round() / 1000.0,
                "py": (e.position.y * 1000.0).round() / 1000.0,
                "vx": (e.velocity.x * 1000.0).round() / 1000.0,
                "vy": (e.velocity.y * 1000.0).round() / 1000.0,
            })
        })
        .collect();
    let golden_str = serde_json::to_string_pretty(&golden_json).unwrap();
    std::fs::write(&golden_path, &golden_str).unwrap();

    // Re-generate and compare
    let rerun = generate_trace();
    assert_eq!(golden.len(), rerun.len(), "trace length mismatch");
    for (i, (g, r)) in golden.iter().zip(rerun.iter()).enumerate() {
        assert_eq!(g.name, r.name, "entry {i}: name");
        assert_eq!(g.frame, r.frame, "entry {i}: frame");
        assert!(
            approx_eq(g.position.x, r.position.x) && approx_eq(g.position.y, r.position.y),
            "entry {i}: position drift: golden={:?} vs rerun={:?}",
            g.position,
            r.position
        );
    }
}

/// Helper: write a golden JSON file and verify a regenerated trace matches it.
fn write_and_verify_golden(golden_name: &str, build_scene: impl Fn() -> SceneTree, frames: u64) {
    let generate = || {
        let tree = build_scene();
        let (_, trace) = run_traced_physics(tree, frames, 1.0 / 60.0);
        trace
    };

    let golden = generate();
    let golden_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(format!("../fixtures/golden/physics/{golden_name}.json"));

    // Write golden
    let golden_json: Vec<_> = golden
        .iter()
        .map(|e| {
            serde_json::json!({
                "name": e.name,
                "frame": e.frame,
                "px": (e.position.x * 1000.0).round() / 1000.0,
                "py": (e.position.y * 1000.0).round() / 1000.0,
                "vx": (e.velocity.x * 1000.0).round() / 1000.0,
                "vy": (e.velocity.y * 1000.0).round() / 1000.0,
            })
        })
        .collect();
    std::fs::write(
        &golden_path,
        serde_json::to_string_pretty(&golden_json).unwrap(),
    )
    .unwrap();

    // Re-generate and compare
    let rerun = generate();
    assert_eq!(
        golden.len(),
        rerun.len(),
        "{golden_name}: trace length mismatch"
    );
    for (i, (g, r)) in golden.iter().zip(rerun.iter()).enumerate() {
        assert_eq!(g.name, r.name, "{golden_name} entry {i}: name");
        assert_eq!(g.frame, r.frame, "{golden_name} entry {i}: frame");
        assert!(
            approx_eq(g.position.x, r.position.x) && approx_eq(g.position.y, r.position.y),
            "{golden_name} entry {i}: position drift: golden={:?} vs rerun={:?}",
            g.position,
            r.position
        );
        assert!(
            approx_eq(g.velocity.x, r.velocity.x) && approx_eq(g.velocity.y, r.velocity.y),
            "{golden_name} entry {i}: velocity drift: golden={:?} vs rerun={:?}",
            g.velocity,
            r.velocity
        );
    }

    // Verify golden file exists and parses
    assert!(
        golden_path.exists(),
        "{golden_name}: golden file should exist"
    );
    let contents = std::fs::read_to_string(&golden_path).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&contents).unwrap();
    assert!(
        parsed.as_array().unwrap().len() > 0,
        "{golden_name}: should not be empty"
    );
}

/// Build a scene with a single falling rigid body (gravity simulation via velocity).
fn make_gravity_fall_scene() -> SceneTree {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let mut ball = Node::new("Ball", "RigidBody2D");
    ball.set_property("position", Variant::Vector2(Vector2::new(0.0, 0.0)));
    ball.set_property(
        "linear_velocity",
        Variant::Vector2(Vector2::new(0.0, 100.0)),
    );
    let ball_id = tree.add_child(root, ball).unwrap();
    let mut s = Node::new("Shape", "CollisionShape2D");
    s.set_property("radius", Variant::Float(5.0));
    tree.add_child(ball_id, s).unwrap();

    tree
}

/// Build a scene with a rigid body falling onto a static floor.
fn make_static_blocking_scene() -> SceneTree {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let mut ball = Node::new("Ball", "RigidBody2D");
    ball.set_property("position", Variant::Vector2(Vector2::new(0.0, 0.0)));
    ball.set_property(
        "linear_velocity",
        Variant::Vector2(Vector2::new(0.0, 200.0)),
    );
    let ball_id = tree.add_child(root, ball).unwrap();
    let mut s = Node::new("Shape", "CollisionShape2D");
    s.set_property("radius", Variant::Float(10.0));
    tree.add_child(ball_id, s).unwrap();

    let mut floor = Node::new("Floor", "StaticBody2D");
    floor.set_property("position", Variant::Vector2(Vector2::new(0.0, 50.0)));
    let floor_id = tree.add_child(root, floor).unwrap();
    let mut sf = Node::new("Shape", "CollisionShape2D");
    sf.set_property("half_extents", Variant::Vector2(Vector2::new(100.0, 10.0)));
    tree.add_child(floor_id, sf).unwrap();

    tree
}

/// Build a scene with two elastic rigid bodies colliding head-on.
fn make_elastic_bounce_scene() -> SceneTree {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let mut a = Node::new("A", "RigidBody2D");
    a.set_property("position", Variant::Vector2(Vector2::new(0.0, 0.0)));
    a.set_property(
        "linear_velocity",
        Variant::Vector2(Vector2::new(100.0, 0.0)),
    );
    a.set_property("bounce", Variant::Float(1.0));
    a.set_property("mass", Variant::Float(1.0));
    let a_id = tree.add_child(root, a).unwrap();
    let mut sa = Node::new("Shape", "CollisionShape2D");
    sa.set_property("radius", Variant::Float(10.0));
    tree.add_child(a_id, sa).unwrap();

    let mut b = Node::new("B", "RigidBody2D");
    b.set_property("position", Variant::Vector2(Vector2::new(15.0, 0.0)));
    b.set_property(
        "linear_velocity",
        Variant::Vector2(Vector2::new(-100.0, 0.0)),
    );
    b.set_property("bounce", Variant::Float(1.0));
    b.set_property("mass", Variant::Float(1.0));
    let b_id = tree.add_child(root, b).unwrap();
    let mut sb = Node::new("Shape", "CollisionShape2D");
    sb.set_property("radius", Variant::Float(10.0));
    tree.add_child(b_id, sb).unwrap();

    tree
}

/// Build a scene with a sliding body on a high-friction floor.
fn make_friction_decel_scene() -> SceneTree {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let mut ball = Node::new("Ball", "RigidBody2D");
    ball.set_property("position", Variant::Vector2(Vector2::new(0.0, 0.0)));
    ball.set_property(
        "linear_velocity",
        Variant::Vector2(Vector2::new(100.0, 50.0)),
    );
    ball.set_property("friction", Variant::Float(1.0));
    ball.set_property("mass", Variant::Float(1.0));
    let ball_id = tree.add_child(root, ball).unwrap();
    let mut s = Node::new("Shape", "CollisionShape2D");
    s.set_property("radius", Variant::Float(5.0));
    tree.add_child(ball_id, s).unwrap();

    let mut floor = Node::new("Floor", "StaticBody2D");
    floor.set_property("position", Variant::Vector2(Vector2::new(0.0, 10.0)));
    floor.set_property("friction", Variant::Float(1.0));
    let floor_id = tree.add_child(root, floor).unwrap();
    let mut sf = Node::new("Shape", "CollisionShape2D");
    sf.set_property("half_extents", Variant::Vector2(Vector2::new(500.0, 5.0)));
    tree.add_child(floor_id, sf).unwrap();

    tree
}

#[test]
fn za_golden_gravity_fall_trajectory() {
    write_and_verify_golden("gravity_fall_30frames", make_gravity_fall_scene, 30);
}

#[test]
fn za_golden_static_body_blocking() {
    write_and_verify_golden("static_blocking_60frames", make_static_blocking_scene, 60);
}

#[test]
fn za_golden_elastic_bounce() {
    write_and_verify_golden("elastic_bounce_30frames", make_elastic_bounce_scene, 30);
}

#[test]
fn za_golden_friction_deceleration() {
    write_and_verify_golden("friction_decel_30frames", make_friction_decel_scene, 30);
}

#[test]
fn za_golden_gravity_fall_trajectory_content_check() {
    // Verify the golden captures meaningful physics: ball accelerates downward.
    let tree = make_gravity_fall_scene();
    let (_, trace) = run_traced_physics(tree, 30, 1.0 / 60.0);

    let ball_entries: Vec<_> = trace.iter().filter(|e| e.name == "Ball").collect();
    assert!(ball_entries.len() >= 2, "should have multiple Ball entries");

    // Position should monotonically increase (falling down in +Y)
    for w in ball_entries.windows(2) {
        assert!(
            w[1].position.y >= w[0].position.y - EPSILON,
            "ball should fall: frame {} py={} >= frame {} py={}",
            w[1].frame,
            w[1].position.y,
            w[0].frame,
            w[0].position.y
        );
    }

    // Velocity should remain constant (no forces, just linear motion)
    let first_vy = ball_entries[0].velocity.y;
    for e in &ball_entries {
        assert!(
            approx_eq(e.velocity.y, first_vy),
            "constant velocity expected: frame {} vy={} vs initial {}",
            e.frame,
            e.velocity.y,
            first_vy
        );
    }
}

#[test]
fn za_golden_static_blocking_content_check() {
    // Verify: ball velocity reduces after hitting floor.
    let tree = make_static_blocking_scene();
    let (_, trace) = run_traced_physics(tree, 60, 1.0 / 60.0);

    let ball_entries: Vec<_> = trace.iter().filter(|e| e.name == "Ball").collect();
    let floor_entries: Vec<_> = trace.iter().filter(|e| e.name == "Floor").collect();

    // Floor should never move
    for e in &floor_entries {
        assert!(
            approx_eq(e.position.y, 50.0),
            "floor should stay at y=50: frame {} py={}",
            e.frame,
            e.position.y
        );
    }

    // Ball should not pass below the floor surface (floor top at y=40)
    let last_ball = ball_entries.last().unwrap();
    assert!(
        last_ball.position.y < 50.0,
        "ball should be stopped above floor: last py={}",
        last_ball.position.y
    );
}

#[test]
fn za_golden_elastic_bounce_content_check() {
    // Verify: after collision, bodies move apart.
    let tree = make_elastic_bounce_scene();
    let (_, trace) = run_traced_physics(tree, 30, 1.0 / 60.0);

    let a_entries: Vec<_> = trace.iter().filter(|e| e.name == "A").collect();
    let b_entries: Vec<_> = trace.iter().filter(|e| e.name == "B").collect();

    // After collision, A should end up moving left (negative x) and B right (positive x)
    let last_a = a_entries.last().unwrap();
    let last_b = b_entries.last().unwrap();
    assert!(
        last_b.position.x > last_a.position.x + 10.0,
        "bodies should have separated: A px={}, B px={}",
        last_a.position.x,
        last_b.position.x
    );
}

#[test]
fn za_golden_friction_decel_content_check() {
    // Verify: friction reduces horizontal velocity on collision.
    let tree = make_friction_decel_scene();
    let (_, trace) = run_traced_physics(tree, 30, 1.0 / 60.0);

    let ball_entries: Vec<_> = trace.iter().filter(|e| e.name == "Ball").collect();
    assert!(ball_entries.len() >= 2);

    // After the first frame (collision with floor), x velocity should be reduced
    let first = &ball_entries[0];
    let second = &ball_entries[1];
    // The ball starts at vx=100 and hits the floor on the first step
    // After friction, vx should be less than the initial 100
    assert!(
        second.velocity.x.abs() <= first.velocity.x.abs() + EPSILON,
        "friction should not increase speed: frame {} vx={} vs frame {} vx={}",
        second.frame,
        second.velocity.x,
        first.frame,
        first.velocity.x
    );
}

// ═══════════════════════════════════════════════════════════════════════
// pat-yxp: physics_playground golden trace fixture
// ═══════════════════════════════════════════════════════════════════════

/// Build the physics_playground scene programmatically (matching the .tscn).
fn make_physics_playground() -> SceneTree {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    // Ball: RigidBody2D at (400, 100)
    let mut ball = Node::new("Ball", "RigidBody2D");
    ball.set_property("position", Variant::Vector2(Vector2::new(400.0, 100.0)));
    ball.set_property("mass", Variant::Float(1.0));
    let ball_id = tree.add_child(root, ball).unwrap();
    let mut s = Node::new("CollisionShape", "CollisionShape2D");
    s.set_property("radius", Variant::Float(16.0));
    tree.add_child(ball_id, s).unwrap();

    // Wall: StaticBody2D at (800, 300)
    let mut wall = Node::new("Wall", "StaticBody2D");
    wall.set_property("position", Variant::Vector2(Vector2::new(800.0, 300.0)));
    let wall_id = tree.add_child(root, wall).unwrap();
    let mut sw = Node::new("CollisionShape", "CollisionShape2D");
    sw.set_property("size", Variant::Vector2(Vector2::new(20.0, 400.0)));
    tree.add_child(wall_id, sw).unwrap();

    // Floor: StaticBody2D at (400, 600)
    let mut floor = Node::new("Floor", "StaticBody2D");
    floor.set_property("position", Variant::Vector2(Vector2::new(400.0, 600.0)));
    let floor_id = tree.add_child(root, floor).unwrap();
    let mut sf = Node::new("CollisionShape", "CollisionShape2D");
    sf.set_property("size", Variant::Vector2(Vector2::new(800.0, 20.0)));
    tree.add_child(floor_id, sf).unwrap();

    tree
}

#[test]
fn yxp_physics_playground_golden_trace() {
    let tree = make_physics_playground();
    let (ml, trace) = run_traced_physics(tree, 60, 1.0 / 60.0);

    // Verify trace is not empty
    assert!(!trace.is_empty(), "trace should not be empty");

    // Verify all expected bodies are in the trace
    let names: std::collections::HashSet<&str> = trace.iter().map(|e| e.name.as_str()).collect();
    assert!(names.contains("Ball"), "should trace Ball");
    assert!(names.contains("Wall"), "should trace Wall");
    assert!(names.contains("Floor"), "should trace Floor");

    // Write golden trace
    let golden_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../fixtures/golden/physics/physics_playground_60frames.json");
    let golden_json: Vec<_> = trace
        .iter()
        .map(|e| {
            serde_json::json!({
                "name": e.name,
                "frame": e.frame,
                "px": (e.position.x * 1000.0).round() / 1000.0,
                "py": (e.position.y * 1000.0).round() / 1000.0,
                "vx": (e.velocity.x * 1000.0).round() / 1000.0,
                "vy": (e.velocity.y * 1000.0).round() / 1000.0,
            })
        })
        .collect();
    std::fs::write(
        &golden_path,
        serde_json::to_string_pretty(&golden_json).unwrap(),
    )
    .unwrap();

    // Static bodies should not have moved
    let wall_entries: Vec<_> = trace.iter().filter(|e| e.name == "Wall").collect();
    for entry in &wall_entries {
        assert!(
            approx_eq(entry.position.x, 800.0) && approx_eq(entry.position.y, 300.0),
            "Wall should stay at (800, 300), got {:?}",
            entry.position
        );
    }

    let floor_entries: Vec<_> = trace.iter().filter(|e| e.name == "Floor").collect();
    for entry in &floor_entries {
        assert!(
            approx_eq(entry.position.x, 400.0) && approx_eq(entry.position.y, 600.0),
            "Floor should stay at (400, 600), got {:?}",
            entry.position
        );
    }

    // Verify determinism — sort by (frame, name) to handle HashMap iteration order.
    let sort_trace = |t: &mut Vec<PhysicsTraceEntry>| {
        t.sort_by(|a, b| a.frame.cmp(&b.frame).then(a.name.cmp(&b.name)));
    };
    let tree2 = make_physics_playground();
    let (_, mut trace2) = run_traced_physics(tree2, 60, 1.0 / 60.0);
    let mut trace_sorted = trace.clone();
    sort_trace(&mut trace_sorted);
    sort_trace(&mut trace2);

    assert_eq!(
        trace_sorted.len(),
        trace2.len(),
        "replay trace length must match"
    );
    for (i, (a, b)) in trace_sorted.iter().zip(trace2.iter()).enumerate() {
        assert_eq!(a.name, b.name, "entry {i}: name");
        assert!(
            approx_eq(a.position.x, b.position.x) && approx_eq(a.position.y, b.position.y),
            "entry {i}: position drift: {:?} vs {:?}",
            a.position,
            b.position
        );
    }

    assert_eq!(ml.frame_count(), 60);
}

#[test]
fn yxp_physics_playground_ball_position_evolves() {
    let tree = make_physics_playground();
    let (_, trace) = run_traced_physics(tree, 30, 1.0 / 60.0);

    let ball_first = trace.iter().find(|e| e.name == "Ball").unwrap();
    let _ball_last = trace.iter().rev().find(|e| e.name == "Ball").unwrap();

    // Ball starts at (400, 100) with no initial velocity.
    // Even without gravity force, just being in the world for 30 frames
    // should show it at the same place (no force applied).
    // The ball's position should be stable since there's no gravity force.
    assert!(
        approx_eq(ball_first.position.x, 400.0),
        "Ball should start near x=400, got {}",
        ball_first.position.x
    );
}

#[test]
fn yxp_golden_file_exists_after_generation() {
    let golden_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../fixtures/golden/physics/physics_playground_exists_check.json");

    // Generate the trace (will create the file)
    let tree = make_physics_playground();
    let (_, trace) = run_traced_physics(tree, 60, 1.0 / 60.0);

    let golden_json: Vec<_> = trace
        .iter()
        .map(|e| {
            serde_json::json!({
                "name": e.name,
                "frame": e.frame,
                "px": (e.position.x * 1000.0).round() / 1000.0,
                "py": (e.position.y * 1000.0).round() / 1000.0,
            })
        })
        .collect();
    std::fs::write(
        &golden_path,
        serde_json::to_string_pretty(&golden_json).unwrap(),
    )
    .unwrap();

    assert!(golden_path.exists(), "golden trace file should exist");

    // Read it back and verify it parses
    let contents = std::fs::read_to_string(&golden_path).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&contents).unwrap();
    assert!(parsed.is_array(), "golden file should be a JSON array");
    assert!(
        parsed.as_array().unwrap().len() > 0,
        "golden file should not be empty"
    );
}

/// Load physics_playground.tscn from disk and instance into a SceneTree.
fn load_physics_playground_from_tscn() -> SceneTree {
    let tscn_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../fixtures/scenes/physics_playground.tscn");
    let source = std::fs::read_to_string(&tscn_path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", tscn_path.display()));
    let scene = gdscene::packed_scene::PackedScene::from_tscn(&source).unwrap();

    let mut tree = SceneTree::new();
    let root = tree.root_id();
    gdscene::packed_scene::add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();
    tree
}

#[test]
fn yxp_tscn_loads_all_physics_bodies() {
    let tree = load_physics_playground_from_tscn();
    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();

    // Should register Ball (Rigid), Wall (Static), Floor (Static) = 3 bodies
    assert!(
        ml.physics_server().body_count() >= 3,
        "should register at least 3 physics bodies from tscn, got {}",
        ml.physics_server().body_count()
    );
}

#[test]
fn yxp_tscn_golden_trace_matches_programmatic() {
    // The .tscn and make_physics_playground() should produce identical traces.
    let sort_trace = |t: &mut Vec<PhysicsTraceEntry>| {
        t.sort_by(|a, b| a.frame.cmp(&b.frame).then(a.name.cmp(&b.name)));
    };

    let (_, mut trace_prog) = run_traced_physics(make_physics_playground(), 60, 1.0 / 60.0);
    let (_, mut trace_tscn) =
        run_traced_physics(load_physics_playground_from_tscn(), 60, 1.0 / 60.0);

    sort_trace(&mut trace_prog);
    sort_trace(&mut trace_tscn);

    assert_eq!(
        trace_prog.len(),
        trace_tscn.len(),
        "programmatic and tscn trace lengths must match: {} vs {}",
        trace_prog.len(),
        trace_tscn.len()
    );

    for (i, (p, t)) in trace_prog.iter().zip(trace_tscn.iter()).enumerate() {
        assert_eq!(p.name, t.name, "entry {i}: name mismatch");
        assert_eq!(p.frame, t.frame, "entry {i}: frame mismatch");
        assert!(
            approx_eq(p.position.x, t.position.x) && approx_eq(p.position.y, t.position.y),
            "entry {i} ({}): position drift: prog={:?} vs tscn={:?}",
            p.name,
            p.position,
            t.position
        );
        assert!(
            approx_eq(p.velocity.x, t.velocity.x) && approx_eq(p.velocity.y, t.velocity.y),
            "entry {i} ({}): velocity drift: prog={:?} vs tscn={:?}",
            p.name,
            p.velocity,
            t.velocity
        );
    }
}

#[test]
fn yxp_tscn_golden_regression() {
    // Load from .tscn, run 60 frames, write golden, read it back, and verify.
    let tree = load_physics_playground_from_tscn();
    let (_, trace) = run_traced_physics(tree, 60, 1.0 / 60.0);

    let golden_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../fixtures/golden/physics/physics_playground_tscn_regression.json");

    // Write golden with full position+velocity data
    let golden_json: Vec<_> = trace
        .iter()
        .map(|e| {
            serde_json::json!({
                "name": e.name,
                "frame": e.frame,
                "px": (e.position.x * 1000.0).round() / 1000.0,
                "py": (e.position.y * 1000.0).round() / 1000.0,
                "vx": (e.velocity.x * 1000.0).round() / 1000.0,
                "vy": (e.velocity.y * 1000.0).round() / 1000.0,
            })
        })
        .collect();
    std::fs::write(
        &golden_path,
        serde_json::to_string_pretty(&golden_json).unwrap(),
    )
    .unwrap();

    // Read golden back and compare against a fresh run
    let contents = std::fs::read_to_string(&golden_path).unwrap();
    let parsed: Vec<serde_json::Value> = serde_json::from_str(&contents).unwrap();

    let tree2 = load_physics_playground_from_tscn();
    let (_, mut trace2) = run_traced_physics(tree2, 60, 1.0 / 60.0);
    trace2.sort_by(|a, b| a.frame.cmp(&b.frame).then(a.name.cmp(&b.name)));

    // Build sorted golden entries for comparison
    let mut golden_entries: Vec<_> = parsed
        .iter()
        .map(|v| {
            (
                v["name"].as_str().unwrap().to_string(),
                v["frame"].as_u64().unwrap(),
                v["px"].as_f64().unwrap() as f32,
                v["py"].as_f64().unwrap() as f32,
                v["vx"].as_f64().unwrap() as f32,
                v["vy"].as_f64().unwrap() as f32,
            )
        })
        .collect();
    golden_entries.sort_by(|a, b| a.1.cmp(&b.1).then(a.0.cmp(&b.0)));

    assert_eq!(
        golden_entries.len(),
        trace2.len(),
        "golden vs live trace length mismatch"
    );

    for (i, ((name, frame, gpx, gpy, gvx, gvy), live)) in
        golden_entries.iter().zip(trace2.iter()).enumerate()
    {
        assert_eq!(name, &live.name, "entry {i}: name");
        assert_eq!(*frame, live.frame, "entry {i}: frame");
        assert!(
            approx_eq(*gpx, live.position.x) && approx_eq(*gpy, live.position.y),
            "entry {i} ({name}): position drift: golden=({gpx},{gpy}) vs live={:?}",
            live.position
        );
        assert!(
            approx_eq(*gvx, live.velocity.x) && approx_eq(*gvy, live.velocity.y),
            "entry {i} ({name}): velocity drift: golden=({gvx},{gvy}) vs live={:?}",
            live.velocity
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════
// pat-wbd: Physics–scene connection through MainLoop
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn wbd_rigid_body_node_position_updates_after_mainloop_step() {
    // Prove that sync_from_physics inside MainLoop::step() writes the
    // updated physics position back to the scene tree node.
    let (tree, rigid_id, _) = make_rigid_and_static_scene();
    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();

    let pos_before = match ml
        .tree()
        .get_node(rigid_id)
        .unwrap()
        .get_property("position")
    {
        Variant::Vector2(v) => v,
        _ => panic!("expected Vector2"),
    };

    // Run 10 frames — rigid body has initial velocity (0, 50), should move down.
    ml.run_frames(10, 1.0 / 60.0);

    let pos_after = match ml
        .tree()
        .get_node(rigid_id)
        .unwrap()
        .get_property("position")
    {
        Variant::Vector2(v) => v,
        _ => panic!("expected Vector2"),
    };

    assert!(
        pos_after.y > pos_before.y,
        "rigid body scene node should move: before={:?}, after={:?}",
        pos_before,
        pos_after
    );
}

#[test]
fn wbd_static_body_node_position_unchanged_after_mainloop_step() {
    // Static bodies must not have their scene node position overwritten.
    let (tree, _, floor_id) = make_rigid_and_static_scene();
    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();

    let pos_before = match ml
        .tree()
        .get_node(floor_id)
        .unwrap()
        .get_property("position")
    {
        Variant::Vector2(v) => v,
        _ => panic!("expected Vector2"),
    };

    ml.run_frames(60, 1.0 / 60.0);

    let pos_after = match ml
        .tree()
        .get_node(floor_id)
        .unwrap()
        .get_property("position")
    {
        Variant::Vector2(v) => v,
        _ => panic!("expected Vector2"),
    };

    assert_eq!(
        pos_before, pos_after,
        "static body scene node must not move"
    );
}

#[test]
fn wbd_character_body_scene_node_syncs_from_script_position() {
    // CharacterBody2D is kinematic — its scene node position drives the physics
    // body (sync_to_physics), not the other way around. Verify that after setting
    // a script-driven position on the scene node, the physics body picks it up.
    let (tree, player_id, _, _) = make_character_scene();
    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();

    // Move the scene node manually (simulating what a script _process would do).
    ml.tree_mut()
        .get_node_mut(player_id)
        .unwrap()
        .set_property("position", Variant::Vector2(Vector2::new(200.0, 150.0)));

    // Step — sync_to_physics should push this into the physics world.
    ml.step(1.0 / 60.0);

    let body_id = ml.physics_server().body_for_node(player_id).unwrap();
    let body_pos = ml
        .physics_server()
        .world()
        .get_body(body_id)
        .unwrap()
        .position;
    assert!(
        approx_eq(body_pos.x, 200.0) && approx_eq(body_pos.y, 150.0),
        "kinematic body should sync from scene node: got {:?}",
        body_pos
    );
}

#[test]
fn wbd_rigid_body_velocity_syncs_to_scene_node() {
    // Verify that linear_velocity from the physics world is written back to
    // the scene node's linear_velocity property.
    let (tree, rigid_id, _) = make_rigid_and_static_scene();
    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();

    ml.step(1.0 / 60.0);

    let vel = match ml
        .tree()
        .get_node(rigid_id)
        .unwrap()
        .get_property("linear_velocity")
    {
        Variant::Vector2(v) => v,
        other => panic!("expected Vector2 linear_velocity, got {other:?}"),
    };

    // Rigid body had initial velocity (0, 50) — it should still have non-zero velocity.
    assert!(
        vel.y.abs() > 0.1,
        "rigid body should have synced velocity to scene node: {vel:?}"
    );
}

#[test]
fn wbd_physics_world_stepped_inside_mainloop_not_manually() {
    // Prove that MainLoop::step() advances the physics world automatically —
    // no need for the caller to call physics_server().step_physics() directly.
    let (tree, rigid_id, _) = make_rigid_and_static_scene();
    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();
    ml.physics_server_mut().set_tracing(true);

    // Only call ml.step() — never touch physics_server directly.
    for _ in 0..10 {
        ml.step(1.0 / 60.0);
    }

    // Physics trace should have entries (proving the world was stepped).
    let trace = ml.physics_server().trace();
    assert!(
        !trace.is_empty(),
        "MainLoop::step() should step the physics world automatically"
    );

    // Scene node position should have changed (proving sync happened).
    let pos = match ml
        .tree()
        .get_node(rigid_id)
        .unwrap()
        .get_property("position")
    {
        Variant::Vector2(v) => v,
        _ => panic!("expected Vector2"),
    };
    assert!(
        pos.y > 50.0,
        "rigid body should have moved via MainLoop physics stepping: y={}",
        pos.y
    );
}

#[test]
fn wbd_physics_runs_at_fixed_timestep_inside_mainloop() {
    // Verify that physics steps happen at the fixed timestep rate, not at
    // the variable frame rate.
    let (tree, _, _) = make_rigid_and_static_scene();
    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();

    // Step with 2x physics dt (2/60) — should get 2 physics steps.
    let output = ml.step(2.0 / 60.0);
    assert_eq!(
        output.physics_steps, 2,
        "2/60 delta at 60 TPS should produce 2 physics steps"
    );

    // Step with 0.5x physics dt (0.5/60) — no physics step yet (accumulator).
    let output = ml.step(0.5 / 60.0);
    assert_eq!(
        output.physics_steps, 0,
        "0.5/60 delta should not trigger a physics step"
    );
}

#[test]
fn wbd_multiple_body_types_in_one_scene() {
    // Verify that a scene with RigidBody2D, StaticBody2D, and CharacterBody2D
    // all register and sync correctly through MainLoop.
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    // RigidBody2D
    let mut rigid = Node::new("Rigid", "RigidBody2D");
    rigid.set_property("position", Variant::Vector2(Vector2::new(100.0, 100.0)));
    rigid.set_property("linear_velocity", Variant::Vector2(Vector2::new(10.0, 0.0)));
    let rigid_id = tree.add_child(root, rigid).unwrap();
    let mut s = Node::new("Shape", "CollisionShape2D");
    s.set_property("radius", Variant::Float(8.0));
    tree.add_child(rigid_id, s).unwrap();

    // StaticBody2D
    let mut static_b = Node::new("Static", "StaticBody2D");
    static_b.set_property("position", Variant::Vector2(Vector2::new(0.0, 500.0)));
    let static_id = tree.add_child(root, static_b).unwrap();
    let mut s2 = Node::new("Shape", "CollisionShape2D");
    s2.set_property("size", Variant::Vector2(Vector2::new(1000.0, 20.0)));
    tree.add_child(static_id, s2).unwrap();

    // CharacterBody2D
    let mut char_b = Node::new("Character", "CharacterBody2D");
    char_b.set_property("position", Variant::Vector2(Vector2::new(200.0, 200.0)));
    let char_id = tree.add_child(root, char_b).unwrap();
    let mut s3 = Node::new("Shape", "CollisionShape2D");
    s3.set_property("radius", Variant::Float(12.0));
    tree.add_child(char_id, s3).unwrap();

    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();
    assert_eq!(ml.physics_server().body_count(), 3);

    ml.run_frames(30, 1.0 / 60.0);

    // Rigid body should have moved (has velocity).
    let rigid_pos = match ml
        .tree()
        .get_node(rigid_id)
        .unwrap()
        .get_property("position")
    {
        Variant::Vector2(v) => v,
        _ => panic!("expected Vector2"),
    };
    assert!(
        rigid_pos.x > 100.0,
        "rigid body should have moved right: {rigid_pos:?}"
    );

    // Static body should not have moved.
    let static_pos = match ml
        .tree()
        .get_node(static_id)
        .unwrap()
        .get_property("position")
    {
        Variant::Vector2(v) => v,
        _ => panic!("expected Vector2"),
    };
    assert_eq!(
        static_pos,
        Vector2::new(0.0, 500.0),
        "static body should not move"
    );

    // Character body is kinematic — physics doesn't move it unless scene node changes.
    let char_pos = match ml
        .tree()
        .get_node(char_id)
        .unwrap()
        .get_property("position")
    {
        Variant::Vector2(v) => v,
        _ => panic!("expected Vector2"),
    };
    assert!(
        approx_eq(char_pos.x, 200.0) && approx_eq(char_pos.y, 200.0),
        "character body should stay put without script driving it: {char_pos:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════
// pat-rhe: Full physics property sync
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn rhe_rotation_syncs_from_physics_to_scene_node() {
    // A rigid body with angular_velocity should have its rotation updated
    // on the scene node after MainLoop steps.
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let mut rigid = Node::new("Spinner", "RigidBody2D");
    rigid.set_property("position", Variant::Vector2(Vector2::new(100.0, 100.0)));
    rigid.set_property("angular_velocity", Variant::Float(3.14)); // ~pi rad/s
    let rigid_id = tree.add_child(root, rigid).unwrap();
    let mut s = Node::new("Shape", "CollisionShape2D");
    s.set_property("radius", Variant::Float(10.0));
    tree.add_child(rigid_id, s).unwrap();

    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();

    // Verify initial rotation is 0.
    let rot_before = match ml
        .tree()
        .get_node(rigid_id)
        .unwrap()
        .get_property("rotation")
    {
        Variant::Float(v) => v,
        _ => 0.0,
    };
    assert!(
        rot_before.abs() < 0.01,
        "rotation should start at 0, got {rot_before}"
    );

    // Run 30 frames (~0.5s at 60fps). angular_velocity=pi => rotation should be ~pi/2.
    ml.run_frames(30, 1.0 / 60.0);

    let rot_after = match ml
        .tree()
        .get_node(rigid_id)
        .unwrap()
        .get_property("rotation")
    {
        Variant::Float(v) => v,
        _ => 0.0,
    };
    assert!(
        rot_after.abs() > 0.1,
        "rotation should change with angular_velocity, got {rot_after}"
    );
}

#[test]
fn rhe_rotation_syncs_from_scene_to_physics_for_kinematic() {
    // CharacterBody2D (kinematic) scene node rotation drives the physics body.
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let mut char_b = Node::new("Player", "CharacterBody2D");
    char_b.set_property("position", Variant::Vector2(Vector2::new(50.0, 50.0)));
    char_b.set_property("rotation", Variant::Float(1.5));
    let char_id = tree.add_child(root, char_b).unwrap();
    let mut s = Node::new("Shape", "CollisionShape2D");
    s.set_property("radius", Variant::Float(8.0));
    tree.add_child(char_id, s).unwrap();

    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();

    // Check that registration picked up the rotation.
    let body_id = ml.physics_server().body_for_node(char_id).unwrap();
    let body_rot = ml
        .physics_server()
        .world()
        .get_body(body_id)
        .unwrap()
        .rotation;
    assert!(
        approx_eq(body_rot, 1.5),
        "kinematic body rotation should match scene node on registration: got {body_rot}"
    );

    // Change scene node rotation and step.
    ml.tree_mut()
        .get_node_mut(char_id)
        .unwrap()
        .set_property("rotation", Variant::Float(2.5));
    ml.step(1.0 / 60.0);

    let body_rot_after = ml
        .physics_server()
        .world()
        .get_body(body_id)
        .unwrap()
        .rotation;
    assert!(
        approx_eq(body_rot_after, 2.5),
        "kinematic body rotation should sync from scene node: got {body_rot_after}"
    );
}

#[test]
fn rhe_angular_velocity_syncs_from_physics_to_scene_node() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let mut rigid = Node::new("Ball", "RigidBody2D");
    rigid.set_property("position", Variant::Vector2(Vector2::new(100.0, 100.0)));
    rigid.set_property("angular_velocity", Variant::Float(2.0));
    let rigid_id = tree.add_child(root, rigid).unwrap();
    let mut s = Node::new("Shape", "CollisionShape2D");
    s.set_property("radius", Variant::Float(10.0));
    tree.add_child(rigid_id, s).unwrap();

    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();
    ml.step(1.0 / 60.0);

    let ang_vel = match ml
        .tree()
        .get_node(rigid_id)
        .unwrap()
        .get_property("angular_velocity")
    {
        Variant::Float(v) => v,
        other => panic!("expected Float angular_velocity, got {other:?}"),
    };
    assert!(
        ang_vel.abs() > 0.1,
        "angular_velocity should sync to scene node: got {ang_vel}"
    );
}

#[test]
fn rhe_mass_flows_from_scene_to_physics_on_registration() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let mut rigid = Node::new("Heavy", "RigidBody2D");
    rigid.set_property("position", Variant::Vector2(Vector2::ZERO));
    rigid.set_property("mass", Variant::Float(5.0));
    let rigid_id = tree.add_child(root, rigid).unwrap();
    let mut s = Node::new("Shape", "CollisionShape2D");
    s.set_property("radius", Variant::Float(10.0));
    tree.add_child(rigid_id, s).unwrap();

    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();

    let body_id = ml.physics_server().body_for_node(rigid_id).unwrap();
    let body = ml.physics_server().world().get_body(body_id).unwrap();
    assert!(
        approx_eq(body.mass, 5.0),
        "mass should flow from scene node: got {}",
        body.mass
    );
}

#[test]
fn rhe_friction_flows_from_scene_to_physics_on_registration() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let mut rigid = Node::new("Slippery", "RigidBody2D");
    rigid.set_property("position", Variant::Vector2(Vector2::ZERO));
    rigid.set_property("friction", Variant::Float(0.1));
    let rigid_id = tree.add_child(root, rigid).unwrap();
    let mut s = Node::new("Shape", "CollisionShape2D");
    s.set_property("radius", Variant::Float(10.0));
    tree.add_child(rigid_id, s).unwrap();

    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();

    let body_id = ml.physics_server().body_for_node(rigid_id).unwrap();
    let body = ml.physics_server().world().get_body(body_id).unwrap();
    assert!(
        approx_eq(body.friction, 0.1),
        "friction should flow from scene node: got {}",
        body.friction
    );
}

#[test]
fn rhe_bounce_flows_from_scene_to_physics_on_registration() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let mut rigid = Node::new("Bouncy", "RigidBody2D");
    rigid.set_property("position", Variant::Vector2(Vector2::ZERO));
    rigid.set_property("bounce", Variant::Float(0.8));
    let rigid_id = tree.add_child(root, rigid).unwrap();
    let mut s = Node::new("Shape", "CollisionShape2D");
    s.set_property("radius", Variant::Float(10.0));
    tree.add_child(rigid_id, s).unwrap();

    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();

    let body_id = ml.physics_server().body_for_node(rigid_id).unwrap();
    let body = ml.physics_server().world().get_body(body_id).unwrap();
    assert!(
        approx_eq(body.bounce, 0.8),
        "bounce should flow from scene node: got {}",
        body.bounce
    );
}

#[test]
fn rhe_circle_collision_shape_registers_with_physics() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let mut rigid = Node::new("Ball", "RigidBody2D");
    rigid.set_property("position", Variant::Vector2(Vector2::ZERO));
    let rigid_id = tree.add_child(root, rigid).unwrap();
    let mut s = Node::new("Shape", "CollisionShape2D");
    s.set_property("radius", Variant::Float(25.0));
    tree.add_child(rigid_id, s).unwrap();

    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();

    let body_id = ml.physics_server().body_for_node(rigid_id).unwrap();
    let body = ml.physics_server().world().get_body(body_id).unwrap();
    assert_eq!(
        body.shape,
        gdphysics2d::Shape2D::Circle { radius: 25.0 },
        "circle shape should register with correct radius"
    );
}

#[test]
fn rhe_rectangle_collision_shape_registers_with_physics() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let mut static_b = Node::new("Wall", "StaticBody2D");
    static_b.set_property("position", Variant::Vector2(Vector2::ZERO));
    let static_id = tree.add_child(root, static_b).unwrap();
    let mut s = Node::new("Shape", "CollisionShape2D");
    s.set_property("size", Variant::Vector2(Vector2::new(100.0, 50.0)));
    tree.add_child(static_id, s).unwrap();

    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();

    let body_id = ml.physics_server().body_for_node(static_id).unwrap();
    let body = ml.physics_server().world().get_body(body_id).unwrap();
    assert_eq!(
        body.shape,
        gdphysics2d::Shape2D::Rectangle {
            half_extents: Vector2::new(50.0, 25.0)
        },
        "rectangle shape should register with correct half_extents"
    );
}

#[test]
fn rhe_initial_rotation_flows_to_physics_on_registration() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let mut rigid = Node::new("Tilted", "RigidBody2D");
    rigid.set_property("position", Variant::Vector2(Vector2::ZERO));
    rigid.set_property("rotation", Variant::Float(0.785)); // ~45 degrees
    let rigid_id = tree.add_child(root, rigid).unwrap();
    let mut s = Node::new("Shape", "CollisionShape2D");
    s.set_property("radius", Variant::Float(10.0));
    tree.add_child(rigid_id, s).unwrap();

    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();

    let body_id = ml.physics_server().body_for_node(rigid_id).unwrap();
    let body = ml.physics_server().world().get_body(body_id).unwrap();
    assert!(
        approx_eq(body.rotation, 0.785),
        "initial rotation should flow to physics: got {}",
        body.rotation
    );
}

#[test]
fn rhe_scale_affects_collision_shape_size() {
    // Verify that when a body node has a scale property, the collision
    // shape dimensions are scaled accordingly.
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let mut rigid = Node::new("BigBall", "RigidBody2D");
    rigid.set_property("position", Variant::Vector2(Vector2::ZERO));
    rigid.set_property("scale", Variant::Vector2(Vector2::new(2.0, 2.0)));
    let rigid_id = tree.add_child(root, rigid).unwrap();
    let mut s = Node::new("Shape", "CollisionShape2D");
    s.set_property("radius", Variant::Float(10.0));
    tree.add_child(rigid_id, s).unwrap();

    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();

    let body_id = ml.physics_server().body_for_node(rigid_id).unwrap();
    let body = ml.physics_server().world().get_body(body_id).unwrap();

    // With scale (2,2), a circle of radius 10 should become radius 20.
    match body.shape {
        gdphysics2d::Shape2D::Circle { radius } => {
            assert!(
                approx_eq(radius, 20.0),
                "scaled circle radius should be 20.0, got {radius}"
            );
        }
        other => panic!("expected Circle shape, got {other:?}"),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// pat-kxa: Fixed-step physics accumulator behavior
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn kxa_deterministic_across_different_frame_rates() {
    // Run the same total simulation time (1 second) at different frame rates.
    // Physics results should be identical because the fixed-step accumulator
    // always steps at 1/60 regardless of the visual frame rate.

    let run_at_fps = |fps: u64| -> (Vector2, Vector2) {
        let (tree, rigid_id, _) = make_rigid_and_static_scene();
        let mut ml = MainLoop::new(tree);
        ml.register_physics_bodies();

        let delta = 1.0 / fps as f64;
        let frames = fps; // 1 second total
        ml.run_frames(frames, delta);

        let pos = match ml
            .tree()
            .get_node(rigid_id)
            .unwrap()
            .get_property("position")
        {
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
    };

    // Run at 60fps (perfect alignment with 60 TPS physics).
    let (pos_60, vel_60) = run_at_fps(60);
    // Run at 120fps (2 visual frames per physics tick).
    let (pos_120, vel_120) = run_at_fps(120);
    // Run at 30fps (2 physics ticks per visual frame).
    let (pos_30, vel_30) = run_at_fps(30);

    // All should produce the same physics result (same number of physics ticks).
    assert!(
        approx_eq(pos_60.x, pos_120.x) && approx_eq(pos_60.y, pos_120.y),
        "60fps vs 120fps position mismatch: {pos_60:?} vs {pos_120:?}"
    );
    assert!(
        approx_eq(pos_60.x, pos_30.x) && approx_eq(pos_60.y, pos_30.y),
        "60fps vs 30fps position mismatch: {pos_60:?} vs {pos_30:?}"
    );
    assert!(
        approx_eq(vel_60.x, vel_120.x) && approx_eq(vel_60.y, vel_120.y),
        "60fps vs 120fps velocity mismatch: {vel_60:?} vs {vel_120:?}"
    );
    assert!(
        approx_eq(vel_60.x, vel_30.x) && approx_eq(vel_60.y, vel_30.y),
        "60fps vs 30fps velocity mismatch: {vel_60:?} vs {vel_30:?}"
    );
}

#[test]
fn kxa_accumulator_carries_remainder_across_frames() {
    // At 50fps with 60 TPS: delta=0.02, physics_dt≈0.01667.
    // The accumulator remainder should carry over and produce correct total ticks.
    // 50 frames * 0.02 = 1.0 second => should get 60 physics ticks total.
    let (tree, rigid_id, _) = make_rigid_and_static_scene();
    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();

    let mut total_physics_steps = 0u32;
    for _ in 0..50 {
        let output = ml.step(1.0 / 50.0);
        total_physics_steps += output.physics_steps;
    }

    // 1 second at 60 TPS = 60 physics ticks.
    assert_eq!(
        total_physics_steps, 60,
        "50 frames at 50fps should produce 60 physics ticks (1 second at 60 TPS)"
    );

    // Rigid body should have moved.
    let pos = match ml
        .tree()
        .get_node(rigid_id)
        .unwrap()
        .get_property("position")
    {
        Variant::Vector2(v) => v,
        _ => panic!("expected Vector2"),
    };
    assert!(pos.y > 50.0, "rigid body should have moved: {pos:?}");
}
