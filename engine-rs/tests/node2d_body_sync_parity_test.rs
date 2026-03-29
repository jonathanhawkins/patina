//! pat-7gj1 / pat-ctv: Node2D body ↔ gdphysics2d world state bidirectional sync parity.
//!
//! Validates that Node2D transforms and physics body state stay synchronized
//! across the sync_to_physics / step_physics / sync_from_physics pipeline:
//!
//! 1. sync_to_physics — kinematic (CharacterBody2D) node properties push into physics
//! 2. sync_from_physics — rigid body physics state writes back to node properties
//! 3. Bidirectional round-trip — full cycle preserves consistency
//! 4. Rotation sync — both directions
//! 5. Velocity writeback — linear_velocity and angular_velocity
//! 6. Static body invariance — static bodies never receive writeback
//! 7. Multi-step accumulation — state consistency across multiple physics steps
//! 8. CharacterBody2D move_and_slide — position sync after character movement
//! 9. Scale-shape consistency — node scale correctly propagates to physics shape

use gdcore::math::Vector2;
use gdphysics2d::body::BodyType;
use gdphysics2d::shape::Shape2D;
use gdscene::node::{Node, NodeId};
use gdscene::physics_server::PhysicsServer;
use gdscene::scene_tree::SceneTree;
use gdvariant::Variant;

// ===========================================================================
// Helpers
// ===========================================================================

/// Helper to create a body node with a circle collision shape child.
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

/// Helper to create a body node with specific velocity.
fn add_body_with_velocity(
    tree: &mut SceneTree,
    parent: NodeId,
    name: &str,
    class: &str,
    position: Vector2,
    radius: f32,
    linear_velocity: Vector2,
    angular_velocity: f64,
) -> NodeId {
    let mut node = Node::new(name, class);
    node.set_property("position", Variant::Vector2(position));
    node.set_property("linear_velocity", Variant::Vector2(linear_velocity));
    node.set_property("angular_velocity", Variant::Float(angular_velocity));
    let node_id = tree.add_child(parent, node).unwrap();

    let mut shape = Node::new("CollisionShape", "CollisionShape2D");
    shape.set_property("radius", Variant::Float(radius as f64));
    tree.add_child(node_id, shape).unwrap();

    node_id
}

/// Helper to create a body node with a rectangle collision shape child.
fn add_rect_body_node(
    tree: &mut SceneTree,
    parent: NodeId,
    name: &str,
    class: &str,
    position: Vector2,
    size: Vector2,
) -> NodeId {
    let mut node = Node::new(name, class);
    node.set_property("position", Variant::Vector2(position));
    let node_id = tree.add_child(parent, node).unwrap();

    let mut shape = Node::new("CollisionShape", "CollisionShape2D");
    shape.set_property("size", Variant::Vector2(size));
    tree.add_child(node_id, shape).unwrap();

    node_id
}

/// Reads a Vector2 property from a node.
fn get_vec2(tree: &SceneTree, id: NodeId, key: &str) -> Vector2 {
    tree.get_node(id)
        .map(|n| match n.get_property(key) {
            Variant::Vector2(v) => v,
            _ => Vector2::ZERO,
        })
        .unwrap_or(Vector2::ZERO)
}

/// Reads a float property from a node.
fn get_float(tree: &SceneTree, id: NodeId, key: &str) -> f64 {
    tree.get_node(id)
        .map(|n| match n.get_property(key) {
            Variant::Float(f) => f,
            Variant::Int(i) => i as f64,
            _ => 0.0,
        })
        .unwrap_or(0.0)
}

// ===========================================================================
// 1. sync_to_physics — kinematic body node transforms push into physics
// ===========================================================================

#[test]
fn sync_to_physics_pushes_kinematic_position_to_body() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let char_id = add_body_node(
        &mut tree,
        root,
        "Player",
        "CharacterBody2D",
        Vector2::new(100.0, 200.0),
        8.0,
    );

    let mut server = PhysicsServer::new();
    server.register_bodies(&tree);

    // Change node position (simulating script update)
    tree.get_node_mut(char_id)
        .unwrap()
        .set_property("position", Variant::Vector2(Vector2::new(300.0, 400.0)));

    // Sync scene → physics
    server.sync_to_physics(&tree);

    let body_id = server.body_for_node(char_id).unwrap();
    let body = server.world().get_body(body_id).unwrap();

    assert_eq!(body.position, Vector2::new(300.0, 400.0));
}

#[test]
fn sync_to_physics_pushes_kinematic_rotation_to_body() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let char_id = add_body_node(
        &mut tree,
        root,
        "Player",
        "CharacterBody2D",
        Vector2::new(50.0, 50.0),
        8.0,
    );

    let mut server = PhysicsServer::new();
    server.register_bodies(&tree);

    // Set rotation on node
    tree.get_node_mut(char_id)
        .unwrap()
        .set_property("rotation", Variant::Float(1.57));

    server.sync_to_physics(&tree);

    let body_id = server.body_for_node(char_id).unwrap();
    let body = server.world().get_body(body_id).unwrap();

    assert!(
        (body.rotation - 1.57).abs() < 0.01,
        "kinematic rotation should sync, got {}",
        body.rotation
    );
}

#[test]
fn sync_to_physics_does_not_overwrite_rigid_body() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    // Register rigid body with velocity so it moves on step
    let rigid_id = add_body_with_velocity(
        &mut tree,
        root,
        "Ball",
        "RigidBody2D",
        Vector2::new(100.0, 50.0),
        16.0,
        Vector2::new(500.0, 0.0),
        0.0,
    );

    let mut server = PhysicsServer::new();
    server.register_bodies(&tree);

    // Step physics — body moves due to initial velocity
    server.step_physics(1.0 / 60.0);

    let body_id = server.body_for_node(rigid_id).unwrap();
    let pos_after_step = server.world().get_body(body_id).unwrap().position;
    assert!(pos_after_step.x > 100.0, "rigid body should have moved");

    // Change node position to something different (simulating erroneous write)
    tree.get_node_mut(rigid_id)
        .unwrap()
        .set_property("position", Variant::Vector2(Vector2::new(0.0, 0.0)));

    // sync_to_physics should NOT overwrite the rigid body
    server.sync_to_physics(&tree);

    let pos_after_sync = server.world().get_body(body_id).unwrap().position;
    assert_eq!(
        pos_after_sync, pos_after_step,
        "sync_to_physics must not overwrite rigid body position"
    );
}

// ===========================================================================
// 2. sync_from_physics — rigid body state writes back to node
// ===========================================================================

#[test]
fn sync_from_physics_writes_position_to_rigid_node() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let rigid_id = add_body_with_velocity(
        &mut tree,
        root,
        "Ball",
        "RigidBody2D",
        Vector2::new(100.0, 50.0),
        16.0,
        Vector2::new(600.0, 0.0),
        0.0,
    );

    let mut server = PhysicsServer::new();
    server.register_bodies(&tree);

    server.step_physics(1.0 / 60.0);

    let body_id = server.body_for_node(rigid_id).unwrap();
    let physics_pos = server.world().get_body(body_id).unwrap().position;

    // Sync back to scene
    server.sync_from_physics(&mut tree);

    let node_pos = get_vec2(&tree, rigid_id, "position");
    assert_eq!(
        node_pos, physics_pos,
        "node position should match physics body after sync_from_physics"
    );
}

#[test]
fn sync_from_physics_writes_rotation_to_rigid_node() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let rigid_id = add_body_with_velocity(
        &mut tree,
        root,
        "Spinner",
        "RigidBody2D",
        Vector2::new(100.0, 50.0),
        16.0,
        Vector2::ZERO,
        3.14,
    );

    let mut server = PhysicsServer::new();
    server.register_bodies(&tree);

    server.step_physics(1.0 / 60.0);

    let body_id = server.body_for_node(rigid_id).unwrap();
    let physics_rot = server.world().get_body(body_id).unwrap().rotation;
    assert!(physics_rot.abs() > 0.01, "body should have rotated");

    server.sync_from_physics(&mut tree);

    let node_rot = get_float(&tree, rigid_id, "rotation") as f32;
    assert!(
        (node_rot - physics_rot).abs() < 0.001,
        "node rotation ({node_rot}) should match physics ({physics_rot})"
    );
}

#[test]
fn sync_from_physics_writes_velocity_properties() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let rigid_id = add_body_with_velocity(
        &mut tree,
        root,
        "Ball",
        "RigidBody2D",
        Vector2::new(0.0, 0.0),
        16.0,
        Vector2::new(200.0, -50.0),
        2.0,
    );

    let mut server = PhysicsServer::new();
    server.register_bodies(&tree);

    server.step_physics(1.0 / 60.0);
    server.sync_from_physics(&mut tree);

    let lin_vel = get_vec2(&tree, rigid_id, "linear_velocity");
    let ang_vel = get_float(&tree, rigid_id, "angular_velocity");

    // Velocities may change slightly due to integration, but should be non-zero
    assert!(
        lin_vel.length_squared() > 0.0,
        "linear_velocity should be written back"
    );
    assert!(
        ang_vel.abs() > 0.0,
        "angular_velocity should be written back"
    );
}

// ===========================================================================
// 3. Static body invariance
// ===========================================================================

#[test]
fn static_body_node_never_receives_writeback() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let static_id = add_rect_body_node(
        &mut tree,
        root,
        "Floor",
        "StaticBody2D",
        Vector2::new(100.0, 500.0),
        Vector2::new(800.0, 20.0),
    );

    let mut server = PhysicsServer::new();
    server.register_bodies(&tree);

    let pos_before = get_vec2(&tree, static_id, "position");

    // Step and sync multiple times
    for _ in 0..20 {
        server.step_physics(1.0 / 60.0);
        server.sync_from_physics(&mut tree);
    }

    let pos_after = get_vec2(&tree, static_id, "position");
    assert_eq!(
        pos_before, pos_after,
        "static body node position must never change via sync"
    );
}

#[test]
fn static_body_physics_state_unchanged_after_steps() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let static_id = add_rect_body_node(
        &mut tree,
        root,
        "Wall",
        "StaticBody2D",
        Vector2::new(200.0, 300.0),
        Vector2::new(20.0, 400.0),
    );

    let mut server = PhysicsServer::new();
    server.register_bodies(&tree);

    let body_id = server.body_for_node(static_id).unwrap();
    let pos_before = server.world().get_body(body_id).unwrap().position;

    for _ in 0..10 {
        server.step_physics(1.0 / 60.0);
    }

    let pos_after = server.world().get_body(body_id).unwrap().position;
    assert_eq!(pos_before, pos_after, "static physics body must not move");
}

// ===========================================================================
// 4. Bidirectional round-trip consistency
// ===========================================================================

#[test]
fn full_sync_roundtrip_rigid_body_consistency() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let rigid_id = add_body_with_velocity(
        &mut tree,
        root,
        "Ball",
        "RigidBody2D",
        Vector2::new(100.0, 100.0),
        16.0,
        Vector2::new(300.0, 0.0),
        0.0,
    );

    let mut server = PhysicsServer::new();
    server.register_bodies(&tree);

    // Full cycle: sync_to → step → sync_from
    server.sync_to_physics(&tree);
    server.step_physics(1.0 / 60.0);
    server.sync_from_physics(&mut tree);

    // Verify node and physics are in agreement
    let body_id = server.body_for_node(rigid_id).unwrap();
    let node_pos = get_vec2(&tree, rigid_id, "position");
    let body_pos = server.world().get_body(body_id).unwrap().position;
    assert_eq!(
        node_pos, body_pos,
        "after full round-trip, node and body positions must match"
    );

    let node_rot = get_float(&tree, rigid_id, "rotation") as f32;
    let body_rot = server.world().get_body(body_id).unwrap().rotation;
    assert!(
        (node_rot - body_rot).abs() < 0.001,
        "rotation must match after round-trip"
    );
}

#[test]
fn full_sync_roundtrip_kinematic_body_consistency() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let char_id = add_body_node(
        &mut tree,
        root,
        "Player",
        "CharacterBody2D",
        Vector2::new(50.0, 50.0),
        8.0,
    );

    let mut server = PhysicsServer::new();
    server.register_bodies(&tree);

    // Move node via script (simulate)
    tree.get_node_mut(char_id)
        .unwrap()
        .set_property("position", Variant::Vector2(Vector2::new(200.0, 150.0)));
    tree.get_node_mut(char_id)
        .unwrap()
        .set_property("rotation", Variant::Float(0.785));

    // Full cycle
    server.sync_to_physics(&tree);
    server.step_physics(1.0 / 60.0);
    server.sync_from_physics(&mut tree);

    // Node and physics body should agree
    let body_id = server.body_for_node(char_id).unwrap();
    let body_pos = server.world().get_body(body_id).unwrap().position;
    let node_pos = get_vec2(&tree, char_id, "position");

    assert_eq!(
        node_pos, body_pos,
        "kinematic node and body positions must match after round-trip"
    );
}

// ===========================================================================
// 5. Multi-step accumulation
// ===========================================================================

#[test]
fn multi_step_rigid_body_position_advances_monotonically() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let rigid_id = add_body_with_velocity(
        &mut tree,
        root,
        "Projectile",
        "RigidBody2D",
        Vector2::new(0.0, 0.0),
        4.0,
        Vector2::new(100.0, 0.0),
        0.0,
    );

    let mut server = PhysicsServer::new();
    server.register_bodies(&tree);

    let mut prev_x = 0.0f32;
    for step in 0..10 {
        server.sync_to_physics(&tree);
        server.step_physics(1.0 / 60.0);
        server.sync_from_physics(&mut tree);

        let pos = get_vec2(&tree, rigid_id, "position");
        assert!(
            pos.x > prev_x,
            "step {step}: position.x ({}) should exceed previous ({prev_x})",
            pos.x
        );
        prev_x = pos.x;
    }
}

#[test]
fn multi_step_node_and_body_stay_in_sync() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let rigid_id = add_body_with_velocity(
        &mut tree,
        root,
        "Ball",
        "RigidBody2D",
        Vector2::new(50.0, 50.0),
        10.0,
        Vector2::new(0.0, 100.0),
        1.5,
    );

    let mut server = PhysicsServer::new();
    server.register_bodies(&tree);

    for _ in 0..20 {
        server.sync_to_physics(&tree);
        server.step_physics(1.0 / 60.0);
        server.sync_from_physics(&mut tree);

        // Verify sync at every step
        let body_id = server.body_for_node(rigid_id).unwrap();
        let node_pos = get_vec2(&tree, rigid_id, "position");
        let body_pos = server.world().get_body(body_id).unwrap().position;
        assert_eq!(
            node_pos, body_pos,
            "node and body positions must match at every step"
        );

        let node_rot = get_float(&tree, rigid_id, "rotation") as f32;
        let body_rot = server.world().get_body(body_id).unwrap().rotation;
        assert!(
            (node_rot - body_rot).abs() < 0.001,
            "node and body rotations must match at every step"
        );
    }
}

// ===========================================================================
// 6. CharacterBody2D move_and_slide sync
// ===========================================================================

#[test]
fn character_move_and_slide_syncs_position_to_node() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    // Floor far away (no collision)
    add_rect_body_node(
        &mut tree,
        root,
        "Floor",
        "StaticBody2D",
        Vector2::new(200.0, 500.0),
        Vector2::new(800.0, 20.0),
    );

    // Character with horizontal velocity
    let char_id = add_body_node(
        &mut tree,
        root,
        "Player",
        "CharacterBody2D",
        Vector2::new(200.0, 100.0),
        8.0,
    );
    tree.get_node_mut(char_id)
        .unwrap()
        .set_property("velocity", Variant::Vector2(Vector2::new(100.0, 0.0)));

    let mut server = PhysicsServer::new();
    server.register_bodies(&tree);

    let initial_pos = get_vec2(&tree, char_id, "position");

    // Run pipeline
    server.sync_to_physics(&tree);
    server.step_physics(1.0 / 60.0);
    server.sync_from_physics(&mut tree);
    server.process_character_movement(1.0 / 60.0, &mut tree);

    let final_pos = get_vec2(&tree, char_id, "position");

    // Character should have moved horizontally
    assert!(
        (final_pos.x - initial_pos.x).abs() > 0.1,
        "character should move: initial={initial_pos:?}, final={final_pos:?}"
    );
}

#[test]
fn character_move_and_slide_writes_state_properties() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    // Floor at y=100
    add_rect_body_node(
        &mut tree,
        root,
        "Floor",
        "StaticBody2D",
        Vector2::new(100.0, 100.0),
        Vector2::new(400.0, 20.0),
    );

    // Character above floor with downward velocity
    let char_id = add_body_node(
        &mut tree,
        root,
        "Player",
        "CharacterBody2D",
        Vector2::new(100.0, 82.0),
        8.0,
    );
    tree.get_node_mut(char_id)
        .unwrap()
        .set_property("velocity", Variant::Vector2(Vector2::new(0.0, 50.0)));

    let mut server = PhysicsServer::new();
    server.register_bodies(&tree);

    server.sync_to_physics(&tree);
    server.step_physics(1.0 / 60.0);
    server.sync_from_physics(&mut tree);
    server.process_character_movement(1.0 / 60.0, &mut tree);

    // Verify floor/wall/ceiling state properties are written (not Nil)
    let has_floor_prop = tree
        .get_node(char_id)
        .map(|n| !matches!(n.get_property("is_on_floor"), Variant::Nil))
        .unwrap_or(false);

    assert!(
        has_floor_prop,
        "is_on_floor property should be written by process_character_movement"
    );

    let has_wall_prop = tree
        .get_node(char_id)
        .map(|n| !matches!(n.get_property("is_on_wall"), Variant::Nil))
        .unwrap_or(false);
    assert!(has_wall_prop, "is_on_wall property should be written");

    let has_ceiling_prop = tree
        .get_node(char_id)
        .map(|n| !matches!(n.get_property("is_on_ceiling"), Variant::Nil))
        .unwrap_or(false);
    assert!(has_ceiling_prop, "is_on_ceiling property should be written");
}

// ===========================================================================
// 7. Scale-shape consistency
// ===========================================================================

#[test]
fn node_scale_propagates_to_circle_shape() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let mut node = Node::new("Scaled", "RigidBody2D");
    node.set_property("position", Variant::Vector2(Vector2::new(100.0, 100.0)));
    node.set_property("scale", Variant::Vector2(Vector2::new(2.0, 3.0)));
    let node_id = tree.add_child(root, node).unwrap();

    let mut shape = Node::new("CollisionShape", "CollisionShape2D");
    shape.set_property("radius", Variant::Float(10.0));
    tree.add_child(node_id, shape).unwrap();

    let mut server = PhysicsServer::new();
    server.register_bodies(&tree);

    let body_id = server.body_for_node(node_id).unwrap();
    let body = server.world().get_body(body_id).unwrap();

    // Circle scales by max(abs(scale.x), abs(scale.y)) = 3.0
    assert_eq!(
        body.shape,
        Shape2D::Circle { radius: 30.0 },
        "circle radius should be scaled by max(scale.x, scale.y)"
    );
}

#[test]
fn node_scale_propagates_to_rect_shape() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let mut node = Node::new("Scaled", "RigidBody2D");
    node.set_property("position", Variant::Vector2(Vector2::new(100.0, 100.0)));
    node.set_property("scale", Variant::Vector2(Vector2::new(2.0, 0.5)));
    let node_id = tree.add_child(root, node).unwrap();

    let mut shape = Node::new("CollisionShape", "CollisionShape2D");
    shape.set_property("size", Variant::Vector2(Vector2::new(20.0, 40.0)));
    tree.add_child(node_id, shape).unwrap();

    let mut server = PhysicsServer::new();
    server.register_bodies(&tree);

    let body_id = server.body_for_node(node_id).unwrap();
    let body = server.world().get_body(body_id).unwrap();

    // Rectangle half_extents: (20/2 * 2.0, 40/2 * 0.5) = (20.0, 10.0)
    assert_eq!(
        body.shape,
        Shape2D::Rectangle {
            half_extents: Vector2::new(20.0, 10.0)
        },
        "rectangle should scale each axis independently"
    );
}

// ===========================================================================
// 8. Initial registration preserves node properties
// ===========================================================================

#[test]
fn register_bodies_reads_initial_position() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let rigid_id = add_body_node(
        &mut tree,
        root,
        "Ball",
        "RigidBody2D",
        Vector2::new(42.0, 99.0),
        16.0,
    );

    let mut server = PhysicsServer::new();
    server.register_bodies(&tree);

    let body_id = server.body_for_node(rigid_id).unwrap();
    let body = server.world().get_body(body_id).unwrap();

    assert_eq!(
        body.position,
        Vector2::new(42.0, 99.0),
        "body should be registered at node's initial position"
    );
}

#[test]
fn register_bodies_reads_initial_velocity() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let rigid_id = add_body_with_velocity(
        &mut tree,
        root,
        "Ball",
        "RigidBody2D",
        Vector2::ZERO,
        16.0,
        Vector2::new(50.0, -25.0),
        1.5,
    );

    let mut server = PhysicsServer::new();
    server.register_bodies(&tree);

    let body_id = server.body_for_node(rigid_id).unwrap();
    let body = server.world().get_body(body_id).unwrap();

    assert_eq!(body.linear_velocity, Vector2::new(50.0, -25.0));
    assert!((body.angular_velocity - 1.5).abs() < 0.01);
}

#[test]
fn register_bodies_reads_initial_rotation() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let mut node = Node::new("Tilted", "RigidBody2D");
    node.set_property("position", Variant::Vector2(Vector2::ZERO));
    node.set_property("rotation", Variant::Float(0.785)); // ~45 degrees
    let rigid_id = tree.add_child(root, node).unwrap();

    let mut shape = Node::new("CollisionShape", "CollisionShape2D");
    shape.set_property("radius", Variant::Float(16.0));
    tree.add_child(rigid_id, shape).unwrap();

    let mut server = PhysicsServer::new();
    server.register_bodies(&tree);

    let body_id = server.body_for_node(rigid_id).unwrap();
    let body = server.world().get_body(body_id).unwrap();

    assert!(
        (body.rotation - 0.785).abs() < 0.01,
        "initial rotation should be read from node"
    );
}

// ===========================================================================
// 9. Mixed body types in same scene
// ===========================================================================

#[test]
fn mixed_body_types_sync_independently() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let rigid_id = add_body_with_velocity(
        &mut tree,
        root,
        "Ball",
        "RigidBody2D",
        Vector2::new(100.0, 100.0),
        16.0,
        Vector2::new(100.0, 0.0),
        0.0,
    );
    let static_id = add_rect_body_node(
        &mut tree,
        root,
        "Floor",
        "StaticBody2D",
        Vector2::new(100.0, 500.0),
        Vector2::new(800.0, 20.0),
    );
    let char_id = add_body_node(
        &mut tree,
        root,
        "Player",
        "CharacterBody2D",
        Vector2::new(50.0, 50.0),
        8.0,
    );

    let mut server = PhysicsServer::new();
    server.register_bodies(&tree);
    assert_eq!(server.body_count(), 3);

    // Verify body types
    let rigid_body = server
        .world()
        .get_body(server.body_for_node(rigid_id).unwrap())
        .unwrap();
    let static_body = server
        .world()
        .get_body(server.body_for_node(static_id).unwrap())
        .unwrap();
    let char_body = server
        .world()
        .get_body(server.body_for_node(char_id).unwrap())
        .unwrap();

    assert_eq!(rigid_body.body_type, BodyType::Rigid);
    assert_eq!(static_body.body_type, BodyType::Static);
    assert_eq!(char_body.body_type, BodyType::Kinematic);

    // Move character node position (simulating script update)
    tree.get_node_mut(char_id)
        .unwrap()
        .set_property("position", Variant::Vector2(Vector2::new(75.0, 75.0)));

    let static_pos_before = get_vec2(&tree, static_id, "position");

    server.sync_to_physics(&tree);
    server.step_physics(1.0 / 60.0);
    server.sync_from_physics(&mut tree);

    // Rigid: moved by physics
    let rigid_pos = get_vec2(&tree, rigid_id, "position");
    assert!(rigid_pos.x > 100.0, "rigid body should have moved right");

    // Static: unchanged
    let static_pos_after = get_vec2(&tree, static_id, "position");
    assert_eq!(static_pos_before, static_pos_after, "static body unchanged");

    // Character: kinematic synced both ways
    let char_body_id = server.body_for_node(char_id).unwrap();
    let char_phys_pos = server.world().get_body(char_body_id).unwrap().position;
    let char_node_pos = get_vec2(&tree, char_id, "position");
    assert_eq!(char_node_pos, char_phys_pos, "kinematic body synced");
}

// ===========================================================================
// 10. Physics material properties preserved through registration
// ===========================================================================

#[test]
fn material_properties_survive_registration() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let mut node = Node::new("Bouncy", "RigidBody2D");
    node.set_property("position", Variant::Vector2(Vector2::ZERO));
    node.set_property("mass", Variant::Float(5.0));
    node.set_property("bounce", Variant::Float(0.8));
    node.set_property("friction", Variant::Float(0.2));
    let id = tree.add_child(root, node).unwrap();

    let mut shape = Node::new("Shape", "CollisionShape2D");
    shape.set_property("radius", Variant::Float(16.0));
    tree.add_child(id, shape).unwrap();

    let mut server = PhysicsServer::new();
    server.register_bodies(&tree);

    let body_id = server.body_for_node(id).unwrap();
    let body = server.world().get_body(body_id).unwrap();

    assert_eq!(body.mass, 5.0);
    assert!((body.bounce - 0.8).abs() < 0.01);
    assert!((body.friction - 0.2).abs() < 0.01);
}

// ===========================================================================
// 11. Kinematic body repeated position updates
// ===========================================================================

#[test]
fn kinematic_body_tracks_repeated_node_position_updates() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let char_id = add_body_node(
        &mut tree,
        root,
        "Player",
        "CharacterBody2D",
        Vector2::new(0.0, 0.0),
        8.0,
    );

    let mut server = PhysicsServer::new();
    server.register_bodies(&tree);

    // Simulate multiple frames of script-driven movement
    let positions = [
        Vector2::new(10.0, 0.0),
        Vector2::new(20.0, 5.0),
        Vector2::new(30.0, 10.0),
        Vector2::new(40.0, 15.0),
    ];

    for (i, &pos) in positions.iter().enumerate() {
        tree.get_node_mut(char_id)
            .unwrap()
            .set_property("position", Variant::Vector2(pos));

        server.sync_to_physics(&tree);

        let body_id = server.body_for_node(char_id).unwrap();
        let body_pos = server.world().get_body(body_id).unwrap().position;
        assert_eq!(
            body_pos, pos,
            "frame {i}: kinematic body should track node position"
        );

        server.step_physics(1.0 / 60.0);
        server.sync_from_physics(&mut tree);
    }
}

// ===========================================================================
// 13. Collision layer/mask registration
// ===========================================================================

#[test]
fn collision_layer_and_mask_read_from_node() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let mut node = Node::new("Layered", "RigidBody2D");
    node.set_property("position", Variant::Vector2(Vector2::ZERO));
    node.set_property("collision_layer", Variant::Int(4));
    node.set_property("collision_mask", Variant::Int(6));
    let node_id = tree.add_child(root, node).unwrap();

    let mut shape = Node::new("Shape", "CollisionShape2D");
    shape.set_property("radius", Variant::Float(8.0));
    tree.add_child(node_id, shape).unwrap();

    let mut server = PhysicsServer::new();
    server.register_bodies(&tree);

    let body_id = server.body_for_node(node_id).unwrap();
    let body = server.world().get_body(body_id).unwrap();

    assert_eq!(body.collision_layer, 4);
    assert_eq!(body.collision_mask, 6);
}

// ===========================================================================
// 14. sync_from_physics writeback to kinematic bodies
// ===========================================================================

#[test]
fn sync_from_physics_writes_back_kinematic_position_after_step() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    // Create kinematic body with a velocity — kinematic bodies integrate
    // position from velocity during step
    let char_id = add_body_with_velocity(
        &mut tree,
        root,
        "Player",
        "CharacterBody2D",
        Vector2::new(50.0, 50.0),
        8.0,
        Vector2::new(120.0, 0.0),
        0.0,
    );

    let mut server = PhysicsServer::new();
    server.register_bodies(&tree);

    // Step physics — kinematic body integrates position from velocity
    server.step_physics(1.0 / 60.0);
    server.sync_from_physics(&mut tree);

    let pos = get_vec2(&tree, char_id, "position");
    // At 120px/s for 1/60s = 2px displacement
    assert!(
        pos.x > 50.0,
        "kinematic body with velocity should advance position, got x={}",
        pos.x
    );

    let vel = get_vec2(&tree, char_id, "linear_velocity");
    assert_eq!(
        vel,
        Vector2::new(120.0, 0.0),
        "kinematic velocity should be preserved and written back"
    );
}

// ===========================================================================
// 15. Angular velocity accumulation across steps
// ===========================================================================

#[test]
fn angular_velocity_accumulates_rotation_across_steps() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let rigid_id = add_body_with_velocity(
        &mut tree,
        root,
        "Spinner",
        "RigidBody2D",
        Vector2::new(100.0, 100.0),
        16.0,
        Vector2::ZERO,
        6.28, // ~1 full revolution per second
    );

    let mut server = PhysicsServer::new();
    server.register_bodies(&tree);

    let mut prev_rot = 0.0f64;
    for step in 0..10 {
        server.step_physics(1.0 / 60.0);
        server.sync_from_physics(&mut tree);

        let rot = get_float(&tree, rigid_id, "rotation");
        assert!(
            rot > prev_rot,
            "step {step}: rotation ({rot}) should increase, prev={prev_rot}"
        );
        prev_rot = rot;
    }
}

// ===========================================================================
// 16. Multiple rigid bodies sync independently
// ===========================================================================

#[test]
fn multiple_rigid_bodies_each_sync_own_state() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let ball_a = add_body_with_velocity(
        &mut tree,
        root,
        "BallA",
        "RigidBody2D",
        Vector2::new(0.0, 0.0),
        8.0,
        Vector2::new(100.0, 0.0),
        0.0,
    );

    let ball_b = add_body_with_velocity(
        &mut tree,
        root,
        "BallB",
        "RigidBody2D",
        Vector2::new(500.0, 0.0),
        8.0,
        Vector2::new(-100.0, 0.0),
        0.0,
    );

    let mut server = PhysicsServer::new();
    server.register_bodies(&tree);

    server.step_physics(1.0 / 60.0);
    server.sync_from_physics(&mut tree);

    let pos_a = get_vec2(&tree, ball_a, "position");
    let pos_b = get_vec2(&tree, ball_b, "position");

    // Ball A moves right, Ball B moves left
    assert!(pos_a.x > 0.0, "BallA should have moved right");
    assert!(pos_b.x < 500.0, "BallB should have moved left");

    // They should have different positions
    assert!(
        (pos_a.x - pos_b.x).abs() > 1.0,
        "each body syncs independently"
    );
}

// ===========================================================================
// 17. Body registration idempotency
// ===========================================================================

#[test]
fn register_bodies_twice_does_not_duplicate() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    add_body_node(
        &mut tree,
        root,
        "Ball",
        "RigidBody2D",
        Vector2::new(100.0, 100.0),
        16.0,
    );

    let mut server = PhysicsServer::new();
    server.register_bodies(&tree);
    assert_eq!(server.body_count(), 1);

    // Register again — should not add a duplicate
    server.register_bodies(&tree);
    assert_eq!(
        server.body_count(),
        1,
        "re-registering should not duplicate bodies"
    );
}

// ===========================================================================
// 19. External position write on rigid body: physics overwrites — pat-ctv
// ===========================================================================

#[test]
fn external_position_write_rigid_overwritten_by_sync_from() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let rigid_id = add_body_with_velocity(
        &mut tree, root, "Ball", "RigidBody2D",
        Vector2::new(100.0, 0.0), 8.0,
        Vector2::new(60.0, 0.0), 0.0,
    );

    let mut server = PhysicsServer::new();
    server.register_bodies(&tree);

    // Run one tick to establish trajectory.
    server.sync_to_physics(&tree);
    server.step_physics(1.0 / 60.0);
    server.sync_from_physics(&mut tree);
    let pos_after_first = get_vec2(&tree, rigid_id, "position");

    // External write: teleport to far-away position.
    tree.get_node_mut(rigid_id).unwrap()
        .set_property("position", Variant::Vector2(Vector2::new(9999.0, 9999.0)));

    // Next tick — physics engine is source of truth for rigid bodies.
    server.sync_to_physics(&tree);
    server.step_physics(1.0 / 60.0);
    server.sync_from_physics(&mut tree);

    let pos_after_second = get_vec2(&tree, rigid_id, "position");
    assert!(
        pos_after_second.x < 200.0,
        "rigid body external write must be overwritten by physics: got {pos_after_second:?}"
    );
    assert!(
        pos_after_second.x > pos_after_first.x - 1.0,
        "rigid body should continue trajectory: first={pos_after_first:?}, second={pos_after_second:?}"
    );
}

// ===========================================================================
// 20. External position write on kinematic body: synced next tick — pat-ctv
// ===========================================================================

#[test]
fn external_position_write_kinematic_synced_to_physics() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let kin_id = add_body_node(
        &mut tree, root, "Player", "CharacterBody2D",
        Vector2::new(0.0, 0.0), 8.0,
    );

    let mut server = PhysicsServer::new();
    server.register_bodies(&tree);

    // External write: teleport kinematic body.
    tree.get_node_mut(kin_id).unwrap()
        .set_property("position", Variant::Vector2(Vector2::new(500.0, 300.0)));

    // Next tick — sync_to_physics reads the node state.
    server.sync_to_physics(&tree);
    server.step_physics(1.0 / 60.0);
    server.sync_from_physics(&mut tree);

    let body_id = server.body_for_node(kin_id).unwrap();
    let phys_pos = server.world().get_body(body_id).unwrap().position;
    assert!(
        (phys_pos.x - 500.0).abs() < 1.0 && (phys_pos.y - 300.0).abs() < 1.0,
        "kinematic physics body must reflect external write: got {phys_pos:?}"
    );
}

// ===========================================================================
// 21. External position write on static body: persists — pat-ctv
// ===========================================================================

#[test]
fn external_position_write_static_persists_across_ticks() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let static_id = add_body_node(
        &mut tree, root, "Wall", "StaticBody2D",
        Vector2::new(0.0, 200.0), 16.0,
    );

    let mut server = PhysicsServer::new();
    server.register_bodies(&tree);

    server.sync_to_physics(&tree);
    server.step_physics(1.0 / 60.0);
    server.sync_from_physics(&mut tree);

    // External write after first tick.
    tree.get_node_mut(static_id).unwrap()
        .set_property("position", Variant::Vector2(Vector2::new(777.0, 888.0)));

    // Multiple ticks — static bodies never get writeback.
    for _ in 0..5 {
        server.sync_to_physics(&tree);
        server.step_physics(1.0 / 60.0);
        server.sync_from_physics(&mut tree);
    }

    let pos = get_vec2(&tree, static_id, "position");
    assert!(
        (pos.x - 777.0).abs() < 0.01 && (pos.y - 888.0).abs() < 0.01,
        "static body external write must persist: got {pos:?}"
    );
}

// ===========================================================================
// 22. External rotation write on rigid: overwritten — pat-ctv
// ===========================================================================

#[test]
fn external_rotation_write_rigid_overwritten() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let rigid_id = add_body_node(
        &mut tree, root, "Spinner", "RigidBody2D",
        Vector2::new(100.0, 100.0), 8.0,
    );

    let mut server = PhysicsServer::new();
    server.register_bodies(&tree);

    server.sync_to_physics(&tree);
    server.step_physics(1.0 / 60.0);
    server.sync_from_physics(&mut tree);

    // External rotation write.
    tree.get_node_mut(rigid_id).unwrap()
        .set_property("rotation", Variant::Float(99.0));

    server.sync_to_physics(&tree);
    server.step_physics(1.0 / 60.0);
    server.sync_from_physics(&mut tree);

    let rot = get_float(&tree, rigid_id, "rotation");
    assert!(
        rot.abs() < 10.0,
        "rigid body external rotation write must be overwritten: got {rot}"
    );
}

// ===========================================================================
// 23. External rotation write on kinematic: synced — pat-ctv
// ===========================================================================

#[test]
fn external_rotation_write_kinematic_synced() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let kin_id = add_body_node(
        &mut tree, root, "Player", "CharacterBody2D",
        Vector2::new(0.0, 0.0), 8.0,
    );

    let mut server = PhysicsServer::new();
    server.register_bodies(&tree);

    let target_rot = std::f64::consts::FRAC_PI_4;
    tree.get_node_mut(kin_id).unwrap()
        .set_property("rotation", Variant::Float(target_rot));

    server.sync_to_physics(&tree);
    server.step_physics(1.0 / 60.0);
    server.sync_from_physics(&mut tree);

    let body_id = server.body_for_node(kin_id).unwrap();
    let phys_rot = server.world().get_body(body_id).unwrap().rotation;
    assert!(
        (phys_rot as f64 - target_rot).abs() < 0.01,
        "kinematic rotation must reflect external write: got {phys_rot}"
    );
}

// ===========================================================================
// 24. External velocity write on rigid: overwritten by physics — pat-ctv
// ===========================================================================

#[test]
fn external_velocity_write_rigid_overwritten() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let rigid_id = add_body_with_velocity(
        &mut tree, root, "Ball", "RigidBody2D",
        Vector2::new(100.0, 0.0), 8.0,
        Vector2::new(60.0, 0.0), 0.0,
    );

    let mut server = PhysicsServer::new();
    server.register_bodies(&tree);

    server.sync_to_physics(&tree);
    server.step_physics(1.0 / 60.0);
    server.sync_from_physics(&mut tree);

    // External write: set huge velocity.
    tree.get_node_mut(rigid_id).unwrap()
        .set_property("linear_velocity", Variant::Vector2(Vector2::new(99999.0, 99999.0)));

    server.sync_to_physics(&tree);
    server.step_physics(1.0 / 60.0);
    server.sync_from_physics(&mut tree);

    let vel = get_vec2(&tree, rigid_id, "linear_velocity");
    assert!(
        vel.x < 1000.0,
        "rigid body velocity must not retain external write: got {vel:?}"
    );
}

// ===========================================================================
// 25. Multiple external writes between ticks: last write wins — pat-ctv
// ===========================================================================

#[test]
fn multiple_external_writes_last_wins_kinematic() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let kin_id = add_body_node(
        &mut tree, root, "Mover", "CharacterBody2D",
        Vector2::new(0.0, 0.0), 8.0,
    );

    let mut server = PhysicsServer::new();
    server.register_bodies(&tree);

    // Multiple writes before the next tick.
    tree.get_node_mut(kin_id).unwrap()
        .set_property("position", Variant::Vector2(Vector2::new(100.0, 0.0)));
    tree.get_node_mut(kin_id).unwrap()
        .set_property("position", Variant::Vector2(Vector2::new(200.0, 0.0)));
    tree.get_node_mut(kin_id).unwrap()
        .set_property("position", Variant::Vector2(Vector2::new(300.0, 0.0)));

    server.sync_to_physics(&tree);
    server.step_physics(1.0 / 60.0);
    server.sync_from_physics(&mut tree);

    let body_id = server.body_for_node(kin_id).unwrap();
    let phys_pos = server.world().get_body(body_id).unwrap().position;
    assert!(
        (phys_pos.x - 300.0).abs() < 1.0,
        "last external write should win for kinematic: got {phys_pos:?}"
    );
}

// ===========================================================================
// 26. Simultaneous external writes on mixed body types — pat-ctv
// ===========================================================================

#[test]
fn simultaneous_external_writes_mixed_body_types() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let rigid_id = add_body_with_velocity(
        &mut tree, root, "Ball", "RigidBody2D",
        Vector2::new(100.0, 0.0), 8.0,
        Vector2::new(60.0, 0.0), 0.0,
    );
    let static_id = add_body_node(
        &mut tree, root, "Wall", "StaticBody2D",
        Vector2::new(0.0, 200.0), 16.0,
    );
    let kin_id = add_body_node(
        &mut tree, root, "Player", "CharacterBody2D",
        Vector2::new(0.0, 0.0), 8.0,
    );

    let mut server = PhysicsServer::new();
    server.register_bodies(&tree);

    server.sync_to_physics(&tree);
    server.step_physics(1.0 / 60.0);
    server.sync_from_physics(&mut tree);

    // External writes on all three body types.
    tree.get_node_mut(rigid_id).unwrap()
        .set_property("position", Variant::Vector2(Vector2::new(5000.0, 5000.0)));
    tree.get_node_mut(static_id).unwrap()
        .set_property("position", Variant::Vector2(Vector2::new(400.0, 500.0)));
    tree.get_node_mut(kin_id).unwrap()
        .set_property("position", Variant::Vector2(Vector2::new(600.0, 700.0)));

    server.sync_to_physics(&tree);
    server.step_physics(1.0 / 60.0);
    server.sync_from_physics(&mut tree);

    // Rigid: overwritten.
    let rigid_pos = get_vec2(&tree, rigid_id, "position");
    assert!(rigid_pos.x < 300.0, "rigid external write overwritten: got {rigid_pos:?}");

    // Static: persists.
    let static_pos = get_vec2(&tree, static_id, "position");
    assert!(
        (static_pos.x - 400.0).abs() < 0.01,
        "static external write persists: got {static_pos:?}"
    );

    // Kinematic: synced.
    let kin_body_id = server.body_for_node(kin_id).unwrap();
    let kin_phys_pos = server.world().get_body(kin_body_id).unwrap().position;
    assert!(
        (kin_phys_pos.x - 600.0).abs() < 1.0,
        "kinematic external write synced: got {kin_phys_pos:?}"
    );
}

// ===========================================================================
// 27. Deterministic state after external writes across multiple ticks — pat-ctv
// ===========================================================================

#[test]
fn deterministic_state_with_external_writes() {
    fn run_scenario() -> Vec<(f64, f64)> {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let rigid_id = add_body_with_velocity(
            &mut tree, root, "Ball", "RigidBody2D",
            Vector2::new(100.0, 0.0), 8.0,
            Vector2::new(60.0, 0.0), 0.0,
        );
        let kin_id = add_body_node(
            &mut tree, root, "Player", "CharacterBody2D",
            Vector2::new(0.0, 0.0), 8.0,
        );

        let mut server = PhysicsServer::new();
        server.register_bodies(&tree);

        let mut positions = Vec::new();
        for i in 0..10 {
            // Externally write kinematic position every 3rd tick.
            if i % 3 == 0 {
                tree.get_node_mut(kin_id).unwrap()
                    .set_property("position", Variant::Vector2(Vector2::new(50.0 * i as f32, 0.0)));
            }
            // Externally write rigid position on tick 5 (should be ignored).
            if i == 5 {
                tree.get_node_mut(rigid_id).unwrap()
                    .set_property("position", Variant::Vector2(Vector2::new(9999.0, 9999.0)));
            }

            server.sync_to_physics(&tree);
            server.step_physics(1.0 / 60.0);
            server.sync_from_physics(&mut tree);

            let pos = get_vec2(&tree, rigid_id, "position");
            positions.push((pos.x as f64, pos.y as f64));
        }
        positions
    }

    let run1 = run_scenario();
    let run2 = run_scenario();

    for (i, (a, b)) in run1.iter().zip(run2.iter()).enumerate() {
        assert!(
            (a.0 - b.0).abs() < 1e-6 && (a.1 - b.1).abs() < 1e-6,
            "frame {i} mismatch with external writes: run1={a:?}, run2={b:?}"
        );
    }
}

// ===========================================================================
// 28. External write pinning kinematic each tick — pat-ctv
// ===========================================================================

#[test]
fn kinematic_pinned_by_external_write_does_not_accumulate() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let kin_id = add_body_with_velocity(
        &mut tree, root, "Mover", "CharacterBody2D",
        Vector2::new(50.0, 50.0), 4.0,
        Vector2::new(1000.0, 0.0), 0.0,
    );

    let mut server = PhysicsServer::new();
    server.register_bodies(&tree);

    for _ in 0..10 {
        // Pin position before each tick.
        tree.get_node_mut(kin_id).unwrap()
            .set_property("position", Variant::Vector2(Vector2::new(50.0, 50.0)));
        server.sync_to_physics(&tree);
        server.step_physics(1.0 / 60.0);
        server.sync_from_physics(&mut tree);
    }

    // Position should be near 50 + one tick's displacement, NOT accumulated.
    let pos = get_vec2(&tree, kin_id, "position");
    assert!(
        pos.x < 100.0,
        "kinematic body must not accumulate velocity when pinned: got {pos:?}"
    );
}

// ===========================================================================
// 29. Static body velocity property not overwritten — pat-ctv
// ===========================================================================

#[test]
fn static_body_velocity_property_preserved_across_ticks() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let static_id = add_body_node(
        &mut tree, root, "Wall", "StaticBody2D",
        Vector2::new(0.0, 200.0), 16.0,
    );

    let mut server = PhysicsServer::new();
    server.register_bodies(&tree);

    // Set a velocity property on the static body node.
    tree.get_node_mut(static_id).unwrap()
        .set_property("linear_velocity", Variant::Vector2(Vector2::new(42.0, 0.0)));

    for _ in 0..5 {
        server.sync_to_physics(&tree);
        server.step_physics(1.0 / 60.0);
        server.sync_from_physics(&mut tree);
    }

    // Velocity should not have been overwritten — sync_from skips static bodies.
    let vel = get_vec2(&tree, static_id, "linear_velocity");
    assert!(
        (vel.x - 42.0).abs() < 0.01,
        "static body velocity property must not be overwritten: got {vel:?}"
    );
}

// ===========================================================================
// 30. Parity report: external transform write contract — pat-ctv
// ===========================================================================

#[test]
fn external_transform_write_parity_report() {
    // Verify the complete external transform write contract against 4.6.1.
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let rigid_id = add_body_with_velocity(
        &mut tree, root, "Rigid", "RigidBody2D",
        Vector2::new(100.0, 0.0), 8.0,
        Vector2::new(60.0, 0.0), 0.0,
    );
    let static_id = add_body_node(
        &mut tree, root, "Static", "StaticBody2D",
        Vector2::new(0.0, 200.0), 16.0,
    );
    let kin_id = add_body_node(
        &mut tree, root, "Kinematic", "CharacterBody2D",
        Vector2::new(0.0, 0.0), 8.0,
    );

    let mut server = PhysicsServer::new();
    server.register_bodies(&tree);

    // 1. Baseline: one tick without external writes.
    server.sync_to_physics(&tree);
    server.step_physics(1.0 / 60.0);
    server.sync_from_physics(&mut tree);

    // 2. External writes on all three body types.
    tree.get_node_mut(rigid_id).unwrap()
        .set_property("position", Variant::Vector2(Vector2::new(5000.0, 5000.0)));
    tree.get_node_mut(static_id).unwrap()
        .set_property("position", Variant::Vector2(Vector2::new(400.0, 500.0)));
    tree.get_node_mut(kin_id).unwrap()
        .set_property("position", Variant::Vector2(Vector2::new(600.0, 700.0)));

    // 3. Second tick.
    server.sync_to_physics(&tree);
    server.step_physics(1.0 / 60.0);
    server.sync_from_physics(&mut tree);

    let rigid_pos = get_vec2(&tree, rigid_id, "position");
    let static_pos = get_vec2(&tree, static_id, "position");
    let kin_pos = get_vec2(&tree, kin_id, "position");

    let mut checks_passed = 0;
    let total_checks = 3;

    // Rigid: overwritten by physics (not at 5000).
    if rigid_pos.x < 300.0 { checks_passed += 1; }
    // Static: persists.
    if (static_pos.x - 400.0).abs() < 0.01 { checks_passed += 1; }
    // Kinematic: synced (near 600).
    let kin_body_id = server.body_for_node(kin_id).unwrap();
    let kin_phys = server.world().get_body(kin_body_id).unwrap().position;
    if (kin_phys.x - 600.0).abs() < 1.0 { checks_passed += 1; }

    eprintln!("\n=== External Transform Write Parity Report (4.6.1) ===");
    eprintln!("  Contract: physics.fixedstep.body_sync_external_transform_writes");
    eprintln!("  Rigid body external write overwritten:  {}", if rigid_pos.x < 300.0 { "PASS" } else { "FAIL" });
    eprintln!("  Static body external write persists:    {}", if (static_pos.x - 400.0).abs() < 0.01 { "PASS" } else { "FAIL" });
    eprintln!("  Kinematic body external write synced:   {}", if (kin_phys.x - 600.0).abs() < 1.0 { "PASS" } else { "FAIL" });
    eprintln!("  Result: {}/{} checks passed", checks_passed, total_checks);
    eprintln!("  Oracle: Godot 4.6.1-stable PhysicsServer2D sync contract");
    eprintln!("========================================================\n");

    assert_eq!(
        checks_passed, total_checks,
        "All external transform write contract checks must pass: {checks_passed}/{total_checks}"
    );
}
