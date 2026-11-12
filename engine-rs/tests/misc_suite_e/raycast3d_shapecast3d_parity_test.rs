//! pat-nwt9y: RayCast3D / ShapeCast3D scene node parity.
//!
//! Proves the scene-side cast nodes are ClassDB-registered, exported from
//! `gdscene`, and correctly wire through `PhysicsServer3D` + the low-level
//! query layer for hit/miss, mask filtering, and parent exclusion.

use gdcore::math::Vector3;
use gdobject::class_db::{
    class_exists, clear_for_testing, is_parent_class, register_3d_classes, register_class,
    ClassRegistration,
};
use gdscene::node::Node;
use gdscene::node3d;
use gdscene::physics_server_3d::PhysicsServer3D;
use gdscene::scene_tree::SceneTree;
use gdscene::{raycast3d, shapecast3d};
use gdvariant::Variant;

fn ensure_node_base() {
    clear_for_testing();
    register_class(ClassRegistration::new("Object"));
    register_class(ClassRegistration::new("Node").parent("Object"));
    register_3d_classes();
}

fn make_sphere_body(
    tree: &mut SceneTree,
    parent: gdscene::NodeId,
    name: &str,
    class: &str,
    pos: Vector3,
    radius: f64,
) -> gdscene::NodeId {
    let body = Node::new(name, class);
    let body_id = tree.add_child(parent, body).unwrap();
    node3d::set_position(tree, body_id, pos);
    let mut shape = Node::new("Shape", "CollisionShape3D");
    shape.set_property("shape", Variant::String("SphereShape3D".to_owned()));
    shape.set_property("radius", Variant::Float(radius));
    tree.add_child(body_id, shape).unwrap();
    body_id
}

#[test]
fn classdb_registers_raycast3d_and_shapecast3d() {
    ensure_node_base();
    assert!(class_exists("RayCast3D"));
    assert!(class_exists("ShapeCast3D"));
    assert!(class_exists("SpringArm3D"));
    assert!(is_parent_class("RayCast3D", "Node3D"));
    assert!(is_parent_class("ShapeCast3D", "Node3D"));
    assert!(is_parent_class("RayCast3D", "Node"));
    assert!(is_parent_class("ShapeCast3D", "Node"));
}

#[test]
fn raycast3d_hits_static_body_at_expected_point() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let body_id = make_sphere_body(
        &mut tree,
        root,
        "Wall",
        "StaticBody3D",
        Vector3::new(0.0, 0.0, 10.0),
        1.0,
    );

    let ray = Node::new("Ray", "RayCast3D");
    let ray_id = tree.add_child(root, ray).unwrap();
    node3d::set_position(&mut tree, ray_id, Vector3::ZERO);
    raycast3d::set_target_position(&mut tree, ray_id, Vector3::new(0.0, 0.0, 20.0));
    raycast3d::set_enabled(&mut tree, ray_id, true);

    let mut physics = PhysicsServer3D::new();
    physics.sync_to_physics(&tree);

    assert!(raycast3d::force_raycast_update(&mut tree, &physics, ray_id));
    assert!(raycast3d::is_colliding(&tree, ray_id));
    assert_eq!(raycast3d::get_collider(&tree, ray_id), Some(body_id));

    let point = raycast3d::get_collision_point(&tree, ray_id);
    // Sphere at z=10, radius 1 → first surface at z=9 along +Z from origin.
    assert!(
        (point.z - 9.0).abs() < 0.05,
        "expected hit near sphere surface z=9, got {point:?}"
    );
}

#[test]
fn raycast3d_miss_when_mask_excludes_layer() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let body_id = make_sphere_body(
        &mut tree,
        root,
        "Wall",
        "StaticBody3D",
        Vector3::new(0.0, 0.0, 10.0),
        1.0,
    );
    tree.get_node_mut(body_id)
        .unwrap()
        .set_property("collision_layer", Variant::Int(0b0010));

    let ray = Node::new("Ray", "RayCast3D");
    let ray_id = tree.add_child(root, ray).unwrap();
    raycast3d::set_target_position(&mut tree, ray_id, Vector3::new(0.0, 0.0, 20.0));
    raycast3d::set_collision_mask(&mut tree, ray_id, 0b0001);

    let mut physics = PhysicsServer3D::new();
    physics.sync_to_physics(&tree);

    assert!(!raycast3d::force_raycast_update(
        &mut tree, &physics, ray_id
    ));
    assert!(!raycast3d::is_colliding(&tree, ray_id));

    raycast3d::set_collision_mask(&mut tree, ray_id, 0b0010);
    assert!(raycast3d::force_raycast_update(&mut tree, &physics, ray_id));
}

#[test]
fn raycast3d_exclude_parent_skips_parent_body() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let parent_id = make_sphere_body(
        &mut tree,
        root,
        "Parent",
        "StaticBody3D",
        Vector3::ZERO,
        2.0,
    );

    // Origin outside the parent sphere so ray_sphere can report a hit.
    let ray = Node::new("Ray", "RayCast3D");
    let ray_id = tree.add_child(parent_id, ray).unwrap();
    node3d::set_position(&mut tree, ray_id, Vector3::new(0.0, 0.0, -5.0));
    raycast3d::set_target_position(&mut tree, ray_id, Vector3::new(0.0, 0.0, 25.0));

    let target_id = make_sphere_body(
        &mut tree,
        root,
        "Target",
        "StaticBody3D",
        Vector3::new(0.0, 0.0, 10.0),
        1.0,
    );

    let mut physics = PhysicsServer3D::new();
    physics.sync_to_physics(&tree);

    // exclude_parent defaults true → reaches target.
    assert!(raycast3d::force_raycast_update(&mut tree, &physics, ray_id));
    assert_eq!(raycast3d::get_collider(&tree, ray_id), Some(target_id));

    raycast3d::set_exclude_parent(&mut tree, ray_id, false);
    assert!(raycast3d::force_raycast_update(&mut tree, &physics, ray_id));
    assert_eq!(raycast3d::get_collider(&tree, ray_id), Some(parent_id));
}

#[test]
fn shapecast3d_reports_overlap() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let body_id = make_sphere_body(
        &mut tree,
        root,
        "Target",
        "StaticBody3D",
        Vector3::new(0.0, 0.0, 10.0),
        1.0,
    );

    let cast = Node::new("Cast", "ShapeCast3D");
    let cast_id = tree.add_child(root, cast).unwrap();
    node3d::set_position(&mut tree, cast_id, Vector3::ZERO);
    shapecast3d::set_target_position(&mut tree, cast_id, Vector3::new(0.0, 0.0, 20.0));
    let mut cast_shape = Node::new("CastShape", "CollisionShape3D");
    cast_shape.set_property("shape", Variant::String("SphereShape3D".to_owned()));
    cast_shape.set_property("radius", Variant::Float(0.5));
    tree.add_child(cast_id, cast_shape).unwrap();

    let mut physics = PhysicsServer3D::new();
    physics.sync_to_physics(&tree);

    assert!(shapecast3d::force_shapecast_update(
        &mut tree, &physics, cast_id
    ));
    assert!(shapecast3d::is_colliding(&tree, cast_id));
    assert!(shapecast3d::get_collision_count(&tree, cast_id) >= 1);
    assert_eq!(shapecast3d::get_collider(&tree, cast_id, 0), Some(body_id));
}
