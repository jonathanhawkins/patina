//! pat-yupc: Connect gdphysics2d to scene nodes and fixed-step runtime.
//!
//! Verifies that gdphysics2d is wired into the scene runtime's fixed-step
//! lifecycle: body nodes registered from scene tree advance through fixed
//! steps with gravity, and the integration is deterministic.
//!
//! Coverage:
//! 1. Rigid body falls under gravity across fixed-step frames
//! 2. Static body remains stationary under gravity
//! 3. Gravity_scale=0 disables gravity for a rigid body
//! 4. Gravity_scale=2 doubles gravity effect
//! 5. Negative gravity_scale inverts gravity
//! 6. CharacterBody2D (kinematic) is not affected by gravity
//! 7. PhysicsServer default gravity matches Godot (0, 980)
//! 8. Custom gravity propagates through PhysicsServer
//! 9. Fixed-step determinism: identical runs produce identical traces
//! 10. Multiple rigid bodies advance independently under gravity
//! 11. MainLoop physics phase integrates gravity per tick

use gdcore::math::Vector2;
use gdscene::main_loop::MainLoop;
use gdscene::node::Node;
use gdscene::physics_server::PhysicsServer;
use gdscene::scene_tree::SceneTree;
use gdvariant::Variant;

const EPSILON: f32 = 1e-3;

fn approx_eq(a: f32, b: f32) -> bool {
    (a - b).abs() < EPSILON
}

/// Helper: add a physics body node with a circle collision shape.
fn add_body(
    tree: &mut SceneTree,
    parent: gdscene::node::NodeId,
    name: &str,
    class: &str,
    pos: Vector2,
    radius: f32,
) -> gdscene::node::NodeId {
    let mut node = Node::new(name, class);
    node.set_property("position", Variant::Vector2(pos));
    let id = tree.add_child(parent, node).unwrap();
    let mut shape = Node::new("CollisionShape", "CollisionShape2D");
    shape.set_property("radius", Variant::Float(radius as f64));
    tree.add_child(id, shape).unwrap();
    id
}

fn get_pos(tree: &SceneTree, id: gdscene::node::NodeId) -> Vector2 {
    tree.get_node(id)
        .map(|n| match n.get_property("position") {
            Variant::Vector2(v) => v,
            _ => Vector2::ZERO,
        })
        .unwrap_or(Vector2::ZERO)
}

// =========================================================================
// 1. Rigid body falls under gravity across fixed-step frames
// =========================================================================
#[test]
fn rigid_body_falls_under_gravity() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let ball = add_body(&mut tree, root, "Ball", "RigidBody2D", Vector2::new(100.0, 0.0), 10.0);

    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();

    let pos_before = get_pos(ml.tree(), ball);
    ml.run_frames(10, 1.0 / 60.0);
    let pos_after = get_pos(ml.tree(), ball);

    assert!(
        pos_after.y > pos_before.y + 1.0,
        "rigid body should fall under gravity: before={:?}, after={:?}",
        pos_before,
        pos_after
    );
    // X should remain unchanged (no horizontal forces)
    assert!(
        approx_eq(pos_after.x, pos_before.x),
        "x should be unchanged: before={}, after={}",
        pos_before.x,
        pos_after.x
    );
}

// =========================================================================
// 2. Static body remains stationary under gravity
// =========================================================================
#[test]
fn static_body_stationary_under_gravity() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let floor = add_body(
        &mut tree,
        root,
        "Floor",
        "StaticBody2D",
        Vector2::new(0.0, 200.0),
        50.0,
    );

    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();

    let pos_before = get_pos(ml.tree(), floor);
    ml.run_frames(20, 1.0 / 60.0);
    let pos_after = get_pos(ml.tree(), floor);

    assert_eq!(
        pos_before, pos_after,
        "static body must not move under gravity"
    );
}

// =========================================================================
// 3. gravity_scale=0 disables gravity
// =========================================================================
#[test]
fn gravity_scale_zero_disables_gravity() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let mut node = Node::new("Floater", "RigidBody2D");
    node.set_property("position", Variant::Vector2(Vector2::new(50.0, 50.0)));
    node.set_property("gravity_scale", Variant::Float(0.0));
    let id = tree.add_child(root, node).unwrap();
    let mut shape = Node::new("Shape", "CollisionShape2D");
    shape.set_property("radius", Variant::Float(5.0));
    tree.add_child(id, shape).unwrap();

    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();

    let pos_before = get_pos(ml.tree(), id);
    ml.run_frames(30, 1.0 / 60.0);
    let pos_after = get_pos(ml.tree(), id);

    assert!(
        approx_eq(pos_after.y, pos_before.y),
        "gravity_scale=0 body should not fall: before={:?}, after={:?}",
        pos_before,
        pos_after
    );
}

// =========================================================================
// 4. gravity_scale=2 doubles gravity effect
// =========================================================================
#[test]
fn gravity_scale_doubles_fall_rate() {
    let mut tree1 = SceneTree::new();
    let root1 = tree1.root_id();
    let ball1 = add_body(&mut tree1, root1, "Normal", "RigidBody2D", Vector2::new(0.0, 0.0), 5.0);

    let mut tree2 = SceneTree::new();
    let root2 = tree2.root_id();
    let mut node2 = Node::new("Double", "RigidBody2D");
    node2.set_property("position", Variant::Vector2(Vector2::ZERO));
    node2.set_property("gravity_scale", Variant::Float(2.0));
    let ball2 = tree2.add_child(root2, node2).unwrap();
    let mut shape = Node::new("Shape", "CollisionShape2D");
    shape.set_property("radius", Variant::Float(5.0));
    tree2.add_child(ball2, shape).unwrap();

    let mut ml1 = MainLoop::new(tree1);
    ml1.register_physics_bodies();
    ml1.run_frames(10, 1.0 / 60.0);

    let mut ml2 = MainLoop::new(tree2);
    ml2.register_physics_bodies();
    ml2.run_frames(10, 1.0 / 60.0);

    let y1 = get_pos(ml1.tree(), ball1).y;
    let y2 = get_pos(ml2.tree(), ball2).y;

    // gravity_scale=2 should fall roughly twice as far
    let ratio = y2 / y1;
    assert!(
        (ratio - 2.0).abs() < 0.1,
        "gravity_scale=2 should produce ~2x displacement: y1={}, y2={}, ratio={}",
        y1,
        y2,
        ratio
    );
}

// =========================================================================
// 5. Negative gravity_scale inverts gravity
// =========================================================================
#[test]
fn negative_gravity_scale_inverts_gravity() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let mut node = Node::new("Balloon", "RigidBody2D");
    node.set_property("position", Variant::Vector2(Vector2::new(0.0, 100.0)));
    node.set_property("gravity_scale", Variant::Float(-1.0));
    let id = tree.add_child(root, node).unwrap();
    let mut shape = Node::new("Shape", "CollisionShape2D");
    shape.set_property("radius", Variant::Float(5.0));
    tree.add_child(id, shape).unwrap();

    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();

    let pos_before = get_pos(ml.tree(), id);
    ml.run_frames(10, 1.0 / 60.0);
    let pos_after = get_pos(ml.tree(), id);

    assert!(
        pos_after.y < pos_before.y,
        "negative gravity_scale should make body rise: before={:?}, after={:?}",
        pos_before,
        pos_after
    );
}

// =========================================================================
// 6. CharacterBody2D (kinematic) is not affected by gravity
// =========================================================================
#[test]
fn character_body_not_affected_by_gravity() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let player = add_body(
        &mut tree,
        root,
        "Player",
        "CharacterBody2D",
        Vector2::new(100.0, 100.0),
        10.0,
    );

    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();

    let pos_before = get_pos(ml.tree(), player);
    ml.run_frames(20, 1.0 / 60.0);
    let pos_after = get_pos(ml.tree(), player);

    assert_eq!(
        pos_before, pos_after,
        "CharacterBody2D should not be affected by world gravity"
    );
}

// =========================================================================
// 7. PhysicsServer default gravity matches Godot (0, 980)
// =========================================================================
#[test]
fn physics_server_default_gravity() {
    let server = PhysicsServer::new();
    let gravity = server.gravity();
    assert_eq!(
        gravity,
        Vector2::new(0.0, 980.0),
        "default gravity should be (0, 980) matching Godot"
    );
}

// =========================================================================
// 8. Custom gravity propagates through PhysicsServer
// =========================================================================
#[test]
fn custom_gravity_propagates() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let ball = add_body(&mut tree, root, "Ball", "RigidBody2D", Vector2::new(0.0, 0.0), 5.0);

    let mut ml = MainLoop::new(tree);
    // Set horizontal gravity (like a side-scroller with sideways pull)
    ml.physics_server_mut().set_gravity(Vector2::new(500.0, 0.0));
    ml.register_physics_bodies();
    ml.run_frames(10, 1.0 / 60.0);

    let pos = get_pos(ml.tree(), ball);
    assert!(
        pos.x > 1.0,
        "horizontal gravity should push body right: {:?}",
        pos
    );
    assert!(
        pos.y.abs() < EPSILON,
        "no vertical gravity, y should stay ~0: {:?}",
        pos
    );
}

// =========================================================================
// 9. Fixed-step determinism: identical runs produce identical traces
// =========================================================================
#[test]
fn fixed_step_determinism() {
    fn run_simulation() -> Vec<Vector2> {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let ball = add_body(&mut tree, root, "Ball", "RigidBody2D", Vector2::new(50.0, 0.0), 8.0);

        let mut ml = MainLoop::new(tree);
        ml.register_physics_bodies();

        let mut positions = Vec::new();
        for _ in 0..20 {
            ml.step(1.0 / 60.0);
            positions.push(get_pos(ml.tree(), ball));
        }
        positions
    }

    let run1 = run_simulation();
    let run2 = run_simulation();

    assert_eq!(
        run1.len(),
        run2.len(),
        "both runs should produce same number of frames"
    );
    for (i, (a, b)) in run1.iter().zip(run2.iter()).enumerate() {
        assert!(
            approx_eq(a.x, b.x) && approx_eq(a.y, b.y),
            "frame {} differs: {:?} vs {:?}",
            i,
            a,
            b
        );
    }
}

// =========================================================================
// 10. Multiple rigid bodies advance independently under gravity
// =========================================================================
#[test]
fn multiple_bodies_advance_independently() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    // Ball A: normal gravity
    let a = add_body(&mut tree, root, "A", "RigidBody2D", Vector2::new(0.0, 0.0), 5.0);

    // Ball B: starts at different position
    let b = add_body(&mut tree, root, "B", "RigidBody2D", Vector2::new(100.0, 50.0), 5.0);

    // Static floor (should not move)
    let c = add_body(&mut tree, root, "C", "StaticBody2D", Vector2::new(50.0, 300.0), 50.0);

    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();

    let a_before = get_pos(ml.tree(), a);
    let b_before = get_pos(ml.tree(), b);
    let c_before = get_pos(ml.tree(), c);

    ml.run_frames(15, 1.0 / 60.0);

    let a_after = get_pos(ml.tree(), a);
    let b_after = get_pos(ml.tree(), b);
    let c_after = get_pos(ml.tree(), c);

    // Both rigid bodies should have fallen
    assert!(a_after.y > a_before.y, "A should fall");
    assert!(b_after.y > b_before.y, "B should fall");

    // They should have fallen the same amount (same mass, same gravity)
    let a_dy = a_after.y - a_before.y;
    let b_dy = b_after.y - b_before.y;
    assert!(
        approx_eq(a_dy, b_dy),
        "equal-mass bodies should fall same distance: a_dy={}, b_dy={}",
        a_dy,
        b_dy
    );

    // Static body should not move
    assert_eq!(c_before, c_after, "static body must not move");
}

// =========================================================================
// 11. MainLoop physics phase integrates gravity per tick
// =========================================================================
#[test]
fn mainloop_gravity_per_tick() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let ball = add_body(&mut tree, root, "Ball", "RigidBody2D", Vector2::new(0.0, 0.0), 5.0);

    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();

    // Run exactly 1 frame at 60 TPS with dt = 1/60 → 1 physics tick
    let output = ml.step(1.0 / 60.0);
    assert_eq!(output.physics_steps, 1, "should run exactly 1 physics tick");

    let pos = get_pos(ml.tree(), ball);
    // After 1 tick at 1/60s with gravity 980:
    // v = 0 + 980 * (1/60) = 16.333...
    // p = 0 + v * (1/60) ≈ 0.272 (semi-implicit Euler: v updated first)
    let expected_y = 980.0 / (60.0 * 60.0); // ≈ 0.2722
    assert!(
        (pos.y - expected_y).abs() < 0.01,
        "after 1 tick gravity integration should yield y≈{:.4}, got y={:.4}",
        expected_y,
        pos.y
    );
}
