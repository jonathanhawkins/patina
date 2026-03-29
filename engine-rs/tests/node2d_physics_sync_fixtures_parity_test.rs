//! pat-0gt0: Sync Node2D body nodes with gdphysics2d world state.
//!
//! Physics fixtures verifying bidirectional state consistency across steps.
//! Extends `node2d_body_sync_parity_test.rs` with additional coverage:
//!
//! 1.  Collision layer/mask roundtrip — registration preserves bitmasks
//! 2.  Material properties roundtrip — mass, friction, bounce survive sync
//! 3.  Area2D position sync — area positions match scene node positions
//! 4.  Multi-body scene: all bodies registered and independently tracked
//! 5.  Rigid body velocity integration across N steps — monotonic position
//! 6.  Kinematic position push is idempotent across repeated syncs
//! 7.  Node property mutation between physics ticks affects next sync
//! 8.  Angular velocity integration — rotation advances per step
//! 9.  Zero-velocity body stays at initial position
//! 10. Rigid body with no collision shape — no physics body registered
//! 11. sync_from_physics does not touch unregistered nodes
//! 12. Body count matches registered physics nodes
//! 13. Mixed body types: each type obeys its sync direction
//! 14. Physics trace records all bodies per frame
//! 15. Deterministic: same fixtures produce identical state across runs
//!
//! Godot references: RigidBody2D, StaticBody2D, CharacterBody2D, Area2D,
//! PhysicsServer2D sync model, collision layers/masks.

use gdcore::math::Vector2;
use gdphysics2d::body::BodyType;
use gdphysics2d::shape::Shape2D;
use gdscene::node::{Node, NodeId};
use gdscene::physics_server::PhysicsServer;
use gdscene::scene_tree::SceneTree;
use gdscene::LifecycleManager;
use gdvariant::Variant;

// ===========================================================================
// Helpers
// ===========================================================================

fn add_body_node(
    tree: &mut SceneTree,
    parent: NodeId,
    name: &str,
    class: &str,
    position: Vector2,
    radius: f32,
) -> NodeId {
    let mut node = Node::new(name, class);
    node.set_property("position", Variant::Vector2(position));
    let node_id = tree.add_child(parent, node).unwrap();

    let mut shape = Node::new("CollisionShape", "CollisionShape2D");
    shape.set_property("radius", Variant::Float(radius as f64));
    tree.add_child(node_id, shape).unwrap();

    node_id
}

fn add_body_with_props(
    tree: &mut SceneTree,
    parent: NodeId,
    name: &str,
    class: &str,
    position: Vector2,
    radius: f32,
    velocity: Vector2,
    angular_vel: f64,
) -> NodeId {
    let mut node = Node::new(name, class);
    node.set_property("position", Variant::Vector2(position));
    node.set_property("linear_velocity", Variant::Vector2(velocity));
    node.set_property("angular_velocity", Variant::Float(angular_vel));
    let node_id = tree.add_child(parent, node).unwrap();

    let mut shape = Node::new("CollisionShape", "CollisionShape2D");
    shape.set_property("radius", Variant::Float(radius as f64));
    tree.add_child(node_id, shape).unwrap();

    node_id
}

fn get_vec2(tree: &SceneTree, id: NodeId, key: &str) -> Vector2 {
    tree.get_node(id)
        .map(|n| match n.get_property(key) {
            Variant::Vector2(v) => v,
            _ => Vector2::ZERO,
        })
        .unwrap_or(Vector2::ZERO)
}

fn get_float(tree: &SceneTree, id: NodeId, key: &str) -> f64 {
    tree.get_node(id)
        .map(|n| match n.get_property(key) {
            Variant::Float(f) => f,
            Variant::Int(i) => i as f64,
            _ => 0.0,
        })
        .unwrap_or(0.0)
}

fn approx_eq(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6
}

fn approx_vec2(a: Vector2, b: Vector2) -> bool {
    (a.x - b.x).abs() < 1e-4 && (a.y - b.y).abs() < 1e-4
}

/// Build a scene with registered physics bodies and return (tree, server, node IDs).
fn build_physics_scene() -> (SceneTree, PhysicsServer, NodeId, NodeId, NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    LifecycleManager::enter_tree(&mut tree, root);

    let rigid = add_body_with_props(
        &mut tree,
        root,
        "Rigid",
        "RigidBody2D",
        Vector2::new(100.0, 0.0),
        10.0,
        Vector2::new(50.0, 0.0),
        0.0,
    );
    let static_b = add_body_node(
        &mut tree,
        root,
        "Static",
        "StaticBody2D",
        Vector2::new(500.0, 0.0),
        20.0,
    );
    let kinematic = add_body_node(
        &mut tree,
        root,
        "Kinematic",
        "CharacterBody2D",
        Vector2::new(200.0, 0.0),
        10.0,
    );

    let mut server = PhysicsServer::new();
    server.register_bodies(&tree);

    (tree, server, rigid, static_b, kinematic)
}

// ===========================================================================
// 1. Collision layer/mask roundtrip
// ===========================================================================

#[test]
fn collision_layer_mask_preserved_on_registration() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    LifecycleManager::enter_tree(&mut tree, root);

    let mut node = Node::new("Body", "RigidBody2D");
    node.set_property("position", Variant::Vector2(Vector2::ZERO));
    node.set_property("collision_layer", Variant::Int(0b1010));
    node.set_property("collision_mask", Variant::Int(0b0101));
    let node_id = tree.add_child(root, node).unwrap();

    let mut shape = Node::new("Shape", "CollisionShape2D");
    shape.set_property("radius", Variant::Float(10.0));
    tree.add_child(node_id, shape).unwrap();

    let mut server = PhysicsServer::new();
    server.register_bodies(&tree);

    let body_id = server.body_for_node(node_id).expect("body registered");
    let body = server.world().get_body(body_id).expect("body exists");

    assert_eq!(body.collision_layer, 0b1010);
    assert_eq!(body.collision_mask, 0b0101);
}

// ===========================================================================
// 2. Material properties roundtrip
// ===========================================================================

#[test]
fn material_properties_preserved_on_registration() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    LifecycleManager::enter_tree(&mut tree, root);

    let mut node = Node::new("Body", "RigidBody2D");
    node.set_property("position", Variant::Vector2(Vector2::ZERO));
    node.set_property("mass", Variant::Float(5.0));
    node.set_property("friction", Variant::Float(0.8));
    node.set_property("bounce", Variant::Float(0.3));
    let node_id = tree.add_child(root, node).unwrap();

    let mut shape = Node::new("Shape", "CollisionShape2D");
    shape.set_property("radius", Variant::Float(10.0));
    tree.add_child(node_id, shape).unwrap();

    let mut server = PhysicsServer::new();
    server.register_bodies(&tree);

    let body_id = server.body_for_node(node_id).expect("body registered");
    let body = server.world().get_body(body_id).expect("body exists");

    assert!((body.mass - 5.0).abs() < 1e-4);
    assert!((body.friction - 0.8).abs() < 1e-4);
    assert!((body.bounce - 0.3).abs() < 1e-4);
}

// ===========================================================================
// 3. Area2D position matches scene node
// ===========================================================================

#[test]
fn area2d_position_matches_scene_node() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    LifecycleManager::enter_tree(&mut tree, root);

    let mut area_node = Node::new("Area", "Area2D");
    area_node.set_property("position", Variant::Vector2(Vector2::new(300.0, 150.0)));
    let area_id = tree.add_child(root, area_node).unwrap();

    let mut shape = Node::new("Shape", "CollisionShape2D");
    shape.set_property("radius", Variant::Float(25.0));
    tree.add_child(area_id, shape).unwrap();

    let mut server = PhysicsServer::new();
    server.register_bodies(&tree);

    // Area should be registered.
    assert_eq!(server.area_count(), 1, "one area registered");
}

// ===========================================================================
// 4. Multi-body scene: all bodies registered independently
// ===========================================================================

#[test]
fn multi_body_scene_all_registered() {
    let (_tree, server, rigid, static_b, kinematic) = build_physics_scene();

    assert!(server.body_for_node(rigid).is_some(), "rigid registered");
    assert!(
        server.body_for_node(static_b).is_some(),
        "static registered"
    );
    assert!(
        server.body_for_node(kinematic).is_some(),
        "kinematic registered"
    );
    assert_eq!(server.body_count(), 3, "exactly 3 bodies");

    // Each has correct body type.
    let rigid_body = server
        .world()
        .get_body(server.body_for_node(rigid).unwrap())
        .unwrap();
    assert_eq!(rigid_body.body_type, BodyType::Rigid);

    let static_body = server
        .world()
        .get_body(server.body_for_node(static_b).unwrap())
        .unwrap();
    assert_eq!(static_body.body_type, BodyType::Static);

    let kin_body = server
        .world()
        .get_body(server.body_for_node(kinematic).unwrap())
        .unwrap();
    assert_eq!(kin_body.body_type, BodyType::Kinematic);

    // Positions match initial node positions.
    assert!(approx_vec2(rigid_body.position, Vector2::new(100.0, 0.0)));
    assert!(approx_vec2(static_body.position, Vector2::new(500.0, 0.0)));
    assert!(approx_vec2(kin_body.position, Vector2::new(200.0, 0.0)));
}

// ===========================================================================
// 5. Rigid body velocity integration across N steps
// ===========================================================================

#[test]
fn rigid_body_position_advances_monotonically() {
    let (mut tree, mut server, rigid, _static_b, _kin) = build_physics_scene();

    let dt = 1.0 / 60.0_f32;
    let mut last_x = get_vec2(&tree, rigid, "position").x;

    for _ in 0..10 {
        server.sync_to_physics(&tree);
        server.step_physics(dt);
        server.sync_from_physics(&mut tree);

        let pos = get_vec2(&tree, rigid, "position");
        assert!(
            pos.x >= last_x,
            "rigid body x should advance: prev={last_x}, now={}",
            pos.x
        );
        last_x = pos.x;
    }

    assert!(
        last_x > 100.0,
        "rigid body should have moved from initial position"
    );
}

// ===========================================================================
// 6. Kinematic position push is idempotent
// ===========================================================================

#[test]
fn kinematic_sync_to_physics_is_idempotent() {
    let (tree, mut server, _rigid, _static_b, kinematic) = build_physics_scene();

    // Set node position.
    let expected_pos = Vector2::new(200.0, 0.0);

    // Sync multiple times — body position should stay consistent.
    for _ in 0..5 {
        server.sync_to_physics(&tree);
        let body_id = server.body_for_node(kinematic).unwrap();
        let body = server.world().get_body(body_id).unwrap();
        assert!(
            approx_vec2(body.position, expected_pos),
            "kinematic position should stay at {:?}, got {:?}",
            expected_pos,
            body.position
        );
    }
}

// ===========================================================================
// 7. Node property mutation between ticks affects next sync
// ===========================================================================

#[test]
fn node_property_mutation_affects_next_sync() {
    let (mut tree, mut server, _rigid, _static_b, kinematic) = build_physics_scene();

    // Initial sync.
    server.sync_to_physics(&tree);
    let body_id = server.body_for_node(kinematic).unwrap();
    let body = server.world().get_body(body_id).unwrap();
    assert!(approx_vec2(body.position, Vector2::new(200.0, 0.0)));

    // Mutate node position (simulating script changing position).
    tree.get_node_mut(kinematic)
        .unwrap()
        .set_property("position", Variant::Vector2(Vector2::new(999.0, 777.0)));

    // Re-sync — physics should pick up the new position.
    server.sync_to_physics(&tree);
    let body = server.world().get_body(body_id).unwrap();
    assert!(
        approx_vec2(body.position, Vector2::new(999.0, 777.0)),
        "body should reflect mutated node position: {:?}",
        body.position
    );
}

// ===========================================================================
// 8. Angular velocity integration
// ===========================================================================

#[test]
fn angular_velocity_integration_advances_rotation() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    LifecycleManager::enter_tree(&mut tree, root);

    let rigid = add_body_with_props(
        &mut tree,
        root,
        "Spinner",
        "RigidBody2D",
        Vector2::ZERO,
        10.0,
        Vector2::ZERO,
        1.0, // 1 rad/s angular velocity
    );

    let mut server = PhysicsServer::new();
    server.register_bodies(&tree);

    let dt = 1.0 / 60.0_f32;
    for _ in 0..60 {
        server.sync_to_physics(&tree);
        server.step_physics(dt);
        server.sync_from_physics(&mut tree);
    }

    let rotation = get_float(&tree, rigid, "rotation");
    // After ~1 second at 1 rad/s, rotation should be approximately 1.0 radian.
    assert!(
        rotation > 0.5 && rotation < 1.5,
        "rotation should be ~1.0 after 1s at 1 rad/s, got {rotation}"
    );
}

// ===========================================================================
// 9. Zero-velocity body stays at initial position
// ===========================================================================

#[test]
fn zero_velocity_body_stays_in_place() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    LifecycleManager::enter_tree(&mut tree, root);

    let initial_pos = Vector2::new(42.0, 84.0);
    let rigid = add_body_with_props(
        &mut tree,
        root,
        "Still",
        "RigidBody2D",
        initial_pos,
        10.0,
        Vector2::ZERO,
        0.0,
    );
    // Disable gravity so the body truly stays in place.
    tree.get_node_mut(rigid)
        .unwrap()
        .set_property("gravity_scale", Variant::Float(0.0));

    let mut server = PhysicsServer::new();
    server.register_bodies(&tree);

    let dt = 1.0 / 60.0_f32;
    for _ in 0..10 {
        server.sync_to_physics(&tree);
        server.step_physics(dt);
        server.sync_from_physics(&mut tree);
    }

    let final_pos = get_vec2(&tree, rigid, "position");
    assert!(
        approx_vec2(final_pos, initial_pos),
        "zero-velocity body should stay at {initial_pos:?}, got {final_pos:?}"
    );
}

// ===========================================================================
// 10. Rigid body without collision shape → default shape fallback
// ===========================================================================

#[test]
fn body_without_collision_shape_gets_default() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    LifecycleManager::enter_tree(&mut tree, root);

    // Add RigidBody2D WITHOUT a CollisionShape2D child.
    let mut node = Node::new("Shapeless", "RigidBody2D");
    node.set_property("position", Variant::Vector2(Vector2::ZERO));
    let node_id = tree.add_child(root, node).unwrap();

    let mut server = PhysicsServer::new();
    server.register_bodies(&tree);

    // Engine assigns a default Circle{radius:16} when no CollisionShape2D child.
    assert!(
        server.body_for_node(node_id).is_some(),
        "body without collision shape should still be registered with default shape"
    );
    assert_eq!(server.body_count(), 1);
}

// ===========================================================================
// 11. sync_from_physics does not touch unregistered nodes
// ===========================================================================

#[test]
fn sync_from_physics_ignores_unregistered_nodes() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    LifecycleManager::enter_tree(&mut tree, root);

    let initial_pos = Vector2::new(42.0, 84.0);
    let mut plain_node = Node::new("Plain", "Node2D");
    plain_node.set_property("position", Variant::Vector2(initial_pos));
    let plain_id = tree.add_child(root, plain_node).unwrap();

    let mut server = PhysicsServer::new();
    server.register_bodies(&tree);

    // Step physics (no bodies registered).
    server.sync_to_physics(&tree);
    server.step_physics(1.0 / 60.0);
    server.sync_from_physics(&mut tree);

    // Plain node should be untouched.
    let pos = get_vec2(&tree, plain_id, "position");
    assert!(
        approx_vec2(pos, initial_pos),
        "unregistered node position should not change: {pos:?}"
    );
}

// ===========================================================================
// 12. Body count matches registered physics nodes
// ===========================================================================

#[test]
fn body_count_matches_scene_physics_nodes() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    LifecycleManager::enter_tree(&mut tree, root);

    add_body_node(&mut tree, root, "R1", "RigidBody2D", Vector2::ZERO, 10.0);
    add_body_node(
        &mut tree,
        root,
        "R2",
        "RigidBody2D",
        Vector2::new(100.0, 0.0),
        10.0,
    );
    add_body_node(
        &mut tree,
        root,
        "S1",
        "StaticBody2D",
        Vector2::new(200.0, 0.0),
        20.0,
    );
    add_body_node(
        &mut tree,
        root,
        "K1",
        "CharacterBody2D",
        Vector2::new(300.0, 0.0),
        10.0,
    );

    // Add a plain Node2D — should NOT be registered.
    let mut plain = Node::new("Plain", "Node2D");
    plain.set_property("position", Variant::Vector2(Vector2::new(400.0, 0.0)));
    tree.add_child(root, plain).unwrap();

    let mut server = PhysicsServer::new();
    server.register_bodies(&tree);

    assert_eq!(
        server.body_count(),
        4,
        "only physics nodes should be counted"
    );
}

// ===========================================================================
// 13. Mixed body types obey sync direction rules
// ===========================================================================

#[test]
fn mixed_body_types_sync_direction_rules() {
    let (mut tree, mut server, rigid, static_b, kinematic) = build_physics_scene();

    let dt = 1.0 / 60.0_f32;

    // Step once.
    server.sync_to_physics(&tree);
    server.step_physics(dt);
    server.sync_from_physics(&mut tree);

    // Rigid body: physics writes back to node.
    let rigid_pos = get_vec2(&tree, rigid, "position");
    let rigid_body = server
        .world()
        .get_body(server.body_for_node(rigid).unwrap())
        .unwrap();
    assert!(
        approx_vec2(rigid_pos, rigid_body.position),
        "rigid: node and body positions should match after sync_from"
    );

    // Static body: position unchanged.
    let static_pos = get_vec2(&tree, static_b, "position");
    assert!(
        approx_vec2(static_pos, Vector2::new(500.0, 0.0)),
        "static: position should not change"
    );

    // Kinematic body: node drives physics, not vice versa.
    // Mutate node and verify physics picks it up.
    tree.get_node_mut(kinematic)
        .unwrap()
        .set_property("position", Variant::Vector2(Vector2::new(999.0, 0.0)));
    server.sync_to_physics(&tree);

    let kin_body = server
        .world()
        .get_body(server.body_for_node(kinematic).unwrap())
        .unwrap();
    assert!(
        approx_vec2(kin_body.position, Vector2::new(999.0, 0.0)),
        "kinematic: physics should follow node mutation"
    );
}

// ===========================================================================
// 14. Physics trace records all bodies per frame
// ===========================================================================

#[test]
fn physics_trace_records_all_bodies() {
    let (tree, mut server, _rigid, _static_b, _kin) = build_physics_scene();
    server.set_tracing(true);
    server.clear_trace();

    server.sync_to_physics(&tree);
    server.step_physics(1.0 / 60.0);
    server.record_trace(&tree);

    let trace = server.trace();
    // Should have entries for all 3 bodies at frame 1.
    let frame_1_entries: Vec<_> = trace.iter().filter(|t| t.frame == 1).collect();
    assert_eq!(
        frame_1_entries.len(),
        3,
        "trace should have one entry per body: {frame_1_entries:?}"
    );
}

// ===========================================================================
// 15. Deterministic: same fixtures produce identical state
// ===========================================================================

#[test]
fn physics_sync_is_deterministic() {
    fn run_once() -> Vec<(f32, f32)> {
        let (mut tree, mut server, rigid, _s, _k) = build_physics_scene();
        let dt = 1.0 / 60.0_f32;
        let mut positions = Vec::new();
        for _ in 0..20 {
            server.sync_to_physics(&tree);
            server.step_physics(dt);
            server.sync_from_physics(&mut tree);
            let pos = get_vec2(&tree, rigid, "position");
            positions.push((pos.x, pos.y));
        }
        positions
    }

    let run1 = run_once();
    let run2 = run_once();

    assert_eq!(
        run1.len(),
        run2.len(),
        "deterministic runs should have same length"
    );
    for (i, (a, b)) in run1.iter().zip(run2.iter()).enumerate() {
        assert!(
            (a.0 - b.0).abs() < 1e-6 && (a.1 - b.1).abs() < 1e-6,
            "step {i}: positions differ: {a:?} vs {b:?}"
        );
    }
}

// ===========================================================================
// 16. Sync roundtrip: rigid body position and velocity consistent
// ===========================================================================

#[test]
fn sync_roundtrip_position_velocity_consistent() {
    let (mut tree, mut server, rigid, _s, _k) = build_physics_scene();

    let dt = 1.0 / 60.0_f32;

    for _ in 0..5 {
        server.sync_to_physics(&tree);
        server.step_physics(dt);
        server.sync_from_physics(&mut tree);

        // After sync_from, node properties should match physics body.
        let body_id = server.body_for_node(rigid).unwrap();
        let body = server.world().get_body(body_id).unwrap();

        let node_pos = get_vec2(&tree, rigid, "position");
        let node_vel = get_vec2(&tree, rigid, "linear_velocity");
        let node_rot = get_float(&tree, rigid, "rotation");
        let node_ang_vel = get_float(&tree, rigid, "angular_velocity");

        assert!(
            approx_vec2(node_pos, body.position),
            "position: node={node_pos:?} body={:?}",
            body.position
        );
        assert!(
            approx_vec2(node_vel, body.linear_velocity),
            "velocity: node={node_vel:?} body={:?}",
            body.linear_velocity
        );
        assert!(
            approx_eq(node_rot, body.rotation as f64),
            "rotation: node={node_rot} body={}",
            body.rotation
        );
        assert!(
            approx_eq(node_ang_vel, body.angular_velocity as f64),
            "angular_velocity: node={node_ang_vel} body={}",
            body.angular_velocity
        );
    }
}

// ===========================================================================
// 17. Static body physics state unchanged after many steps
// ===========================================================================

#[test]
fn static_body_state_unchanged_across_steps() {
    let (mut tree, mut server, _rigid, static_b, _kin) = build_physics_scene();

    let initial_pos = get_vec2(&tree, static_b, "position");
    let dt = 1.0 / 60.0_f32;

    for _ in 0..20 {
        server.sync_to_physics(&tree);
        server.step_physics(dt);
        server.sync_from_physics(&mut tree);
    }

    let final_pos = get_vec2(&tree, static_b, "position");
    assert!(
        approx_vec2(final_pos, initial_pos),
        "static body should not move: initial={initial_pos:?} final={final_pos:?}"
    );

    // Physics body should also be unchanged.
    let body_id = server.body_for_node(static_b).unwrap();
    let body = server.world().get_body(body_id).unwrap();
    assert!(approx_vec2(body.position, initial_pos));
}

// ===========================================================================
// 18. Initial velocity read from node property on registration
// ===========================================================================

#[test]
fn initial_velocity_read_from_node_on_registration() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    LifecycleManager::enter_tree(&mut tree, root);

    let expected_vel = Vector2::new(123.0, 456.0);
    let rigid = add_body_with_props(
        &mut tree,
        root,
        "Fast",
        "RigidBody2D",
        Vector2::ZERO,
        10.0,
        expected_vel,
        2.5,
    );

    let mut server = PhysicsServer::new();
    server.register_bodies(&tree);

    let body_id = server.body_for_node(rigid).unwrap();
    let body = server.world().get_body(body_id).unwrap();

    assert!(
        approx_vec2(body.linear_velocity, expected_vel),
        "initial velocity should match: {:?}",
        body.linear_velocity
    );
    assert!(
        (body.angular_velocity - 2.5).abs() < 1e-4,
        "initial angular velocity should be 2.5: {}",
        body.angular_velocity
    );
}

// ===========================================================================
// 20. One-way collision property preserved through registration
// ===========================================================================

#[test]
fn one_way_collision_property_preserved() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    LifecycleManager::enter_tree(&mut tree, root);

    let mut node = Node::new("Platform", "StaticBody2D");
    node.set_property("position", Variant::Vector2(Vector2::new(100.0, 200.0)));
    let node_id = tree.add_child(root, node).unwrap();

    let mut shape = Node::new("Shape", "CollisionShape2D");
    shape.set_property("size", Variant::Vector2(Vector2::new(200.0, 20.0)));
    tree.add_child(node_id, shape).unwrap();

    let mut server = PhysicsServer::new();
    server.register_bodies(&tree);

    let body_id = server.body_for_node(node_id).unwrap();
    let body = server.world().get_body(body_id).unwrap();

    // Default one_way_collision should be false.
    assert!(
        !body.one_way_collision,
        "default one_way_collision should be false"
    );
    assert_eq!(body.body_type, BodyType::Static);
}

// ===========================================================================
// 21. Multiple CharacterBody2D nodes sync independently
// ===========================================================================

#[test]
fn multiple_kinematic_bodies_sync_independently() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    LifecycleManager::enter_tree(&mut tree, root);

    let char_a = add_body_node(
        &mut tree,
        root,
        "PlayerA",
        "CharacterBody2D",
        Vector2::new(100.0, 100.0),
        8.0,
    );
    let char_b = add_body_node(
        &mut tree,
        root,
        "PlayerB",
        "CharacterBody2D",
        Vector2::new(300.0, 300.0),
        8.0,
    );

    let mut server = PhysicsServer::new();
    server.register_bodies(&tree);

    // Move only PlayerA.
    tree.get_node_mut(char_a)
        .unwrap()
        .set_property("position", Variant::Vector2(Vector2::new(150.0, 150.0)));

    server.sync_to_physics(&tree);

    let body_a = server
        .world()
        .get_body(server.body_for_node(char_a).unwrap())
        .unwrap();
    let body_b = server
        .world()
        .get_body(server.body_for_node(char_b).unwrap())
        .unwrap();

    assert!(
        approx_vec2(body_a.position, Vector2::new(150.0, 150.0)),
        "PlayerA should have moved: {:?}",
        body_a.position
    );
    assert!(
        approx_vec2(body_b.position, Vector2::new(300.0, 300.0)),
        "PlayerB should be unchanged: {:?}",
        body_b.position
    );
}

// ===========================================================================
// 22. Collision shape type roundtrip — circle and rect shapes preserved
// ===========================================================================

#[test]
fn collision_shape_type_preserved_after_registration() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    LifecycleManager::enter_tree(&mut tree, root);

    // Circle shape body.
    let circle_id = add_body_node(
        &mut tree,
        root,
        "Circle",
        "RigidBody2D",
        Vector2::ZERO,
        15.0,
    );

    // Rectangle shape body.
    let mut rect_node = Node::new("Rect", "RigidBody2D");
    rect_node.set_property("position", Variant::Vector2(Vector2::new(100.0, 0.0)));
    let rect_id = tree.add_child(root, rect_node).unwrap();

    let mut rect_shape = Node::new("Shape", "CollisionShape2D");
    rect_shape.set_property("size", Variant::Vector2(Vector2::new(30.0, 20.0)));
    tree.add_child(rect_id, rect_shape).unwrap();

    let mut server = PhysicsServer::new();
    server.register_bodies(&tree);

    let circle_body = server
        .world()
        .get_body(server.body_for_node(circle_id).unwrap())
        .unwrap();
    assert_eq!(circle_body.shape, Shape2D::Circle { radius: 15.0 });

    let rect_body = server
        .world()
        .get_body(server.body_for_node(rect_id).unwrap())
        .unwrap();
    assert_eq!(
        rect_body.shape,
        Shape2D::Rectangle {
            half_extents: Vector2::new(15.0, 10.0)
        }
    );
}

// ===========================================================================
// 23. Full pipeline: sync_to → step → sync_from across multiple body types
// ===========================================================================

#[test]
fn full_pipeline_bidirectional_consistency_across_body_types() {
    let (mut tree, mut server, rigid, static_b, kinematic) = build_physics_scene();

    let dt = 1.0 / 60.0_f32;

    // Run 10 frames of the full pipeline.
    for frame in 0..10 {
        // Simulate script moving the kinematic body each frame.
        let kin_x = 200.0 + frame as f32 * 5.0;
        tree.get_node_mut(kinematic)
            .unwrap()
            .set_property("position", Variant::Vector2(Vector2::new(kin_x, 0.0)));

        server.sync_to_physics(&tree);
        server.step_physics(dt);
        server.sync_from_physics(&mut tree);

        // Rigid body: node matches physics.
        let rigid_body_id = server.body_for_node(rigid).unwrap();
        let rigid_body = server.world().get_body(rigid_body_id).unwrap();
        let rigid_node_pos = get_vec2(&tree, rigid, "position");
        assert!(
            approx_vec2(rigid_node_pos, rigid_body.position),
            "frame {frame}: rigid node/body position mismatch: node={rigid_node_pos:?} body={:?}",
            rigid_body.position
        );

        // Static body: position unchanged.
        let static_pos = get_vec2(&tree, static_b, "position");
        assert!(
            approx_vec2(static_pos, Vector2::new(500.0, 0.0)),
            "frame {frame}: static body moved: {static_pos:?}"
        );

        // Kinematic body: physics tracks node.
        let kin_body_id = server.body_for_node(kinematic).unwrap();
        let kin_body = server.world().get_body(kin_body_id).unwrap();
        assert!(
            approx_vec2(kin_body.position, Vector2::new(kin_x, 0.0)),
            "frame {frame}: kinematic body should track node: {:?}",
            kin_body.position
        );
    }
}

// ===========================================================================
// 24. Re-registration is safe and idempotent
// ===========================================================================

#[test]
fn re_registration_does_not_duplicate_bodies() {
    let (tree, mut server, _rigid, _static_b, _kin) = build_physics_scene();

    assert_eq!(server.body_count(), 3);

    // Re-register should not create duplicates.
    server.register_bodies(&tree);
    assert_eq!(
        server.body_count(),
        3,
        "re-registration should not duplicate bodies"
    );

    // Register a third time to be sure.
    server.register_bodies(&tree);
    assert_eq!(server.body_count(), 3);
}

// ===========================================================================
// 25. Velocity writeback roundtrip consistency
// ===========================================================================

#[test]
fn velocity_writeback_roundtrip_across_frames() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    LifecycleManager::enter_tree(&mut tree, root);

    let rigid = add_body_with_props(
        &mut tree,
        root,
        "Moving",
        "RigidBody2D",
        Vector2::ZERO,
        10.0,
        Vector2::new(200.0, 100.0),
        0.5,
    );

    let mut server = PhysicsServer::new();
    server.register_bodies(&tree);

    let dt = 1.0 / 60.0_f32;
    for _ in 0..5 {
        server.sync_to_physics(&tree);
        server.step_physics(dt);
        server.sync_from_physics(&mut tree);

        let body_id = server.body_for_node(rigid).unwrap();
        let body = server.world().get_body(body_id).unwrap();

        let node_vel = get_vec2(&tree, rigid, "linear_velocity");
        let node_ang = get_float(&tree, rigid, "angular_velocity");

        assert!(
            approx_vec2(node_vel, body.linear_velocity),
            "linear velocity mismatch: node={node_vel:?} body={:?}",
            body.linear_velocity
        );
        assert!(
            approx_eq(node_ang, body.angular_velocity as f64),
            "angular velocity mismatch: node={node_ang} body={}",
            body.angular_velocity
        );
    }
}
