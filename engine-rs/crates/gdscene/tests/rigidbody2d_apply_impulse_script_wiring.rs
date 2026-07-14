//! pat-nzgdu.14 — RigidBody2D force/impulse/torque script API + angular dynamics.
//!
//! In Godot a `RigidBody2D` script drives its body through `apply_central_impulse`,
//! `apply_impulse(impulse, position)`, `apply_torque`, etc., called during
//! `_physics_process`; the body's linear AND angular velocity (and its node
//! transform) change on the next physics step. Before this bead those methods
//! were unreachable from GDScript and the physics bridge dropped script-set
//! velocity, so a scripted body never moved from an impulse.
//!
//! These tests build a scene whose script calls the force/impulse methods on a
//! child `RigidBody2D` every physics frame, run the `MainLoop`, and assert the
//! body's velocity and transform respond — with no manual `PhysicsServer` calls.

use gdcore::math::Vector2;
use gdscene::node::Node;
use gdscene::scene_tree::SceneTree;
use gdscene::{GDScriptNodeInstance, MainLoop};
use gdvariant::Variant;

/// Builds `root -> Main(Node2D, script) -> Body(RigidBody2D + CircleShape)`.
/// `Body` has `gravity_scale = 0` so assertions isolate the scripted forces.
fn build_scene(script_src: &str) -> (SceneTree, gdscene::NodeId, gdscene::NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let main_id = tree.add_child(root, Node::new("Main", "Node2D")).unwrap();

    let mut body = Node::new("Body", "RigidBody2D");
    body.set_property("position", Variant::Vector2(Vector2::ZERO));
    body.set_property("mass", Variant::Float(1.0));
    // Isolate the scripted forces from gravity.
    body.set_property("gravity_scale", Variant::Float(0.0));
    let body_id = tree.add_child(main_id, body).unwrap();
    let mut shape = Node::new("Shape", "CollisionShape2D");
    shape.set_property("radius", Variant::Float(8.0));
    tree.add_child(body_id, shape).unwrap();

    let script = GDScriptNodeInstance::from_source(script_src, main_id).unwrap();
    tree.attach_script(main_id, Box::new(script));

    (tree, main_id, body_id)
}

fn vec2_of(tree: &SceneTree, id: gdscene::NodeId, key: &str) -> Vector2 {
    match tree.get_node(id).map(|n| n.get_property(key)) {
        Some(Variant::Vector2(v)) => v,
        _ => Vector2::ZERO,
    }
}

fn float_of(tree: &SceneTree, id: gdscene::NodeId, key: &str) -> f32 {
    match tree.get_node(id).map(|n| n.get_property(key)) {
        Some(Variant::Float(f)) => f as f32,
        Some(Variant::Int(i)) => i as f32,
        _ => 0.0,
    }
}

/// A scene-loaded RigidBody2D whose `_physics_process` calls
/// `apply_central_impulse` and `apply_torque` gains both linear and angular
/// velocity and moves/rotates after the MainLoop steps — no manual force calls.
#[test]
fn rigidbody2d_apply_impulse_script_wiring() {
    let script = "\
extends Node2D
func _physics_process(delta):
    var b = get_node(\"Body\")
    b.apply_central_impulse(Vector2(5, 0))
    b.apply_torque(30)
";
    let (tree, _main_id, body_id) = build_scene(script);

    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();
    // A few fixed-timestep frames: frame 1's _physics_process queues the
    // impulse/torque, later frames' sync_to_physics drains them into the body.
    for _ in 0..3 {
        ml.step(1.0 / 60.0);
    }

    let lin = vec2_of(ml.tree(), body_id, "linear_velocity");
    let ang = float_of(ml.tree(), body_id, "angular_velocity");
    let pos = vec2_of(ml.tree(), body_id, "position");
    let rot = float_of(ml.tree(), body_id, "rotation");

    assert!(
        lin.x > 0.0,
        "apply_central_impulse must give the body linear velocity, got {lin:?}",
    );
    assert!(
        pos.x > 0.0,
        "the body's node position must advance after the impulse, got {pos:?}",
    );
    assert!(
        ang > 0.0,
        "apply_torque must give the body angular velocity, got {ang}",
    );
    assert!(
        rot > 0.0,
        "the body's node rotation must advance after the torque, got {rot}",
    );
}

/// An off-center `apply_impulse(impulse, position)` imparts angular velocity
/// (the lever arm produces torque about the center of mass).
#[test]
fn rigidbody2d_off_center_impulse_imparts_spin() {
    let script = "\
extends Node2D
func _physics_process(delta):
    var b = get_node(\"Body\")
    b.apply_impulse(Vector2(0, 10), Vector2(5, 0))
";
    let (tree, _main_id, body_id) = build_scene(script);

    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();
    for _ in 0..3 {
        ml.step(1.0 / 60.0);
    }

    let ang = float_of(ml.tree(), body_id, "angular_velocity");
    let lin = vec2_of(ml.tree(), body_id, "linear_velocity");
    assert!(
        ang.abs() > 1e-4,
        "off-center apply_impulse must impart angular velocity, got {ang}",
    );
    assert!(
        lin.y > 0.0,
        "off-center apply_impulse still changes linear velocity, got {lin:?}",
    );
}
