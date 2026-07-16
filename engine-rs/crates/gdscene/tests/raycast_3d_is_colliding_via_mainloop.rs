//! pat-nzgdu.64 — enabled RayCast3D nodes are driven against the 3D physics
//! world every physics frame by the `MainLoop`, so a scene-loaded ray reports
//! its live collision state after a step with no manual `force_raycast_update`,
//! and the state re-queries each tick.
//!
//! Godot ground truth: an enabled RayCast3D updates itself during the physics
//! step. After a tick a ray aimed at a body reports `is_colliding()` and
//! `get_collider()`; once it no longer intersects the body it reports `false`
//! on the next tick. This exercises `MainLoop`'s 3D query phase directly (the
//! same phase that also drives ShapeCast3D and SpringArm3D — see
//! `raycast3d_scene_reports_collision_after_physics_step` and
//! `springarm3d_retracts_after_physics_step`).

use gdcore::math::Vector3;
use gdscene::node::Node;
use gdscene::scene_tree::SceneTree;
use gdscene::{node3d, raycast3d, MainLoop};
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

#[test]
fn raycast_3d_is_colliding_via_mainloop() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let main_id = tree.add_child(root, Node::new("Main", "Node3D")).unwrap();

    // Target sphere at (0,0,10); ray sits at Main's origin aimed +Z through it.
    let body_id = add_static_sphere(&mut tree, main_id, "Target", Vector3::new(0.0, 0.0, 10.0));
    let ray_id = tree
        .add_child(main_id, Node::new("RayCast3D", "RayCast3D"))
        .unwrap();
    node3d::set_position(&mut tree, ray_id, Vector3::ZERO);
    raycast3d::set_target_position(&mut tree, ray_id, Vector3::new(0.0, 0.0, 20.0));

    let mut ml = MainLoop::new(tree);
    // A few fixed-timestep frames: the first syncs the body into the physics
    // world, then the query phase updates the ray — no explicit force_* call.
    for _ in 0..3 {
        ml.step(1.0 / 60.0);
    }

    // The MainLoop's 3D query phase reports the live hit and its collider.
    assert!(
        raycast3d::is_colliding(ml.tree(), ray_id),
        "RayCast3D must report is_colliding after a physics step, no manual force_raycast_update",
    );
    assert_eq!(
        raycast3d::get_collider(ml.tree(), ray_id),
        Some(body_id),
        "RayCast3D collider must be the target StaticBody3D",
    );

    // Re-aim the ray so it no longer crosses the body (+Y instead of +Z): the
    // query phase re-runs each physics tick, so is_colliding flips to false on
    // the next step. (Re-aiming the ray, rather than translating the static
    // target, keeps this test independent of static-body transform re-sync,
    // which is a separate concern — pat-nzgdu.62.)
    raycast3d::set_target_position(ml.tree_mut(), ray_id, Vector3::new(0.0, 20.0, 0.0));
    for _ in 0..2 {
        ml.step(1.0 / 60.0);
    }

    assert!(
        !raycast3d::is_colliding(ml.tree(), ray_id),
        "a re-aimed RayCast3D reports no collision on the next tick (live per-frame re-query)",
    );
}
