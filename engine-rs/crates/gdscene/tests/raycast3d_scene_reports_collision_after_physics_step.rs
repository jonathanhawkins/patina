//! pat-nzgdu.12 — RayCast3D / ShapeCast3D / SpringArm3D are driven by the
//! MainLoop every physics frame and are queryable from GDScript.
//!
//! In Godot these query nodes update themselves during the physics step: after
//! one `_physics_process` tick a `RayCast3D` aimed at a body already reports
//! `is_colliding()` with no manual `force_raycast_update()` call, and a
//! `SpringArm3D` has already retracted around an obstacle. This test builds such
//! a scene, runs `MainLoop::step` (which internally syncs physics and then runs
//! the 3D query phase), and asserts both the Rust query API AND the GDScript
//! method dispatch (`$RayCast3D.is_colliding()`) observe the collision — with no
//! explicit `force_*` call anywhere in the test.

use gdcore::math::Vector3;
use gdscene::node::Node;
use gdscene::scene_tree::SceneTree;
use gdscene::{node3d, raycast3d, springarm3d, GDScriptNodeInstance, MainLoop};
use gdvariant::Variant;

/// Add a StaticBody3D carrying a unit-sphere CollisionShape3D at `pos`.
fn add_static_sphere(
    tree: &mut SceneTree,
    parent: gdscene::NodeId,
    name: &str,
    pos: Vector3,
) -> gdscene::NodeId {
    let body_id = tree
        .add_child(parent, Node::new(name, "StaticBody3D"))
        .unwrap();
    node3d::set_position(tree, body_id, pos);
    let mut shape = Node::new("Shape", "CollisionShape3D");
    shape.set_property("shape", Variant::String("SphereShape3D".to_owned()));
    shape.set_property("radius", Variant::Float(1.0));
    tree.add_child(body_id, shape).unwrap();
    body_id
}

/// A RayCast3D aimed at a StaticBody3D reports the collision after the MainLoop
/// steps a physics frame — via both `raycast3d::is_colliding` (Rust) and a
/// GDScript `$RayCast3D.is_colliding()` — with no manual `force_raycast_update`.
#[test]
fn raycast3d_scene_reports_collision_after_physics_step() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    // `Main` carries the driving script; the ray and body are its children so
    // the script can reach the ray with `$RayCast3D`.
    let main_id = tree.add_child(root, Node::new("Main", "Node3D")).unwrap();

    // Sphere body at (0,0,10); ray at Main's origin aimed +Z straight through it.
    let body_id = add_static_sphere(&mut tree, main_id, "Target", Vector3::new(0.0, 0.0, 10.0));
    let ray_id = tree
        .add_child(main_id, Node::new("RayCast3D", "RayCast3D"))
        .unwrap();
    node3d::set_position(&mut tree, ray_id, Vector3::ZERO);
    raycast3d::set_target_position(&mut tree, ray_id, Vector3::new(0.0, 0.0, 20.0));

    // The script samples the ray each frame through GDScript method dispatch.
    // `process_queries_3d` runs before `_process`, so the read sees this frame's
    // fresh collision state.
    let script_src = "\
extends Node3D
var ray_hit = false
func _process(delta):
    self.ray_hit = get_node(\"RayCast3D\").is_colliding()
";
    let script = GDScriptNodeInstance::from_source(script_src, main_id).unwrap();
    tree.attach_script(main_id, Box::new(script));

    let mut ml = MainLoop::new(tree);
    // A few fixed-timestep frames: first syncs the body into the physics world,
    // then the query phase updates the ray. No explicit force_* call.
    for _ in 0..3 {
        ml.step(1.0 / 60.0);
    }

    // Rust query API observes the hit purely from the MainLoop's query phase.
    assert!(
        raycast3d::is_colliding(ml.tree(), ray_id),
        "RayCast3D must report a collision after a physics step with no manual force_raycast_update",
    );
    assert_eq!(
        raycast3d::get_collider(ml.tree(), ray_id),
        Some(body_id),
        "RayCast3D collider must be the StaticBody3D",
    );

    // GDScript path: `$RayCast3D.is_colliding()` returned true and the script's
    // `ray_hit` var synced onto the Main node's property after `_process`.
    let ray_hit = ml
        .tree()
        .get_node(main_id)
        .expect("Main node still in tree")
        .get_property("ray_hit");
    assert_eq!(
        ray_hit,
        Variant::Bool(true),
        "GDScript $RayCast3D.is_colliding() must be true after the physics step",
    );
}

/// A SpringArm3D retracts around an obstacle sitting within its spring length
/// after the MainLoop drives it — again with no manual `force_*` call.
#[test]
fn springarm3d_retracts_after_physics_step() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    // Obstacle at (0,0,-5); the arm sits at the origin and extends along -Z
    // with a rest spring_length of 10.
    add_static_sphere(&mut tree, root, "Wall", Vector3::new(0.0, 0.0, -5.0));
    let arm_id = tree.add_child(root, Node::new("Arm", "SpringArm3D")).unwrap();
    node3d::set_position(&mut tree, arm_id, Vector3::ZERO);
    springarm3d::set_spring_length(&mut tree, arm_id, 10.0);
    springarm3d::set_margin(&mut tree, arm_id, 0.0);

    let mut ml = MainLoop::new(tree);
    for _ in 0..3 {
        ml.step(1.0 / 60.0);
    }

    // The ray from z=0 into a sphere centered at z=-5 (radius 1) enters at
    // z=-4, so the arm retracts from 10 to ~4.
    let length = springarm3d::get_hit_length(ml.tree(), arm_id);
    assert!(
        length < 9.9,
        "SpringArm3D must retract below its 10.0 spring length, got {length}",
    );
    assert!(
        (length - 4.0).abs() < 0.5,
        "retracted spring length should be ~4.0, got {length}",
    );
}
