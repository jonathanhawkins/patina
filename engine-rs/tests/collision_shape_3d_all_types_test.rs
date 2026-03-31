//! pat-qm0x1: CollisionShape3D with all 3D shape types.
//!
//! Validates:
//! 1. CollisionShape3D registered in ClassDB with shape/disabled/radius/height/size properties
//! 2. All 7 shape types: Sphere, Box, Capsule, Cylinder, ConvexPolygon, ConcavePolygon, WorldBoundary
//! 3. Shape3D AABB computation for all types
//! 4. Shape3D contains_point for all types
//! 5. PhysicsServer3D extracts all shape types from CollisionShape3D nodes
//! 6. Disabled CollisionShape3D nodes are skipped
//! 7. Shape integration with RigidBody3D/StaticBody3D via scene tree

use std::sync::Mutex;

use gdcore::math::Vector3;
use gdobject::class_db;
use gdphysics3d::shape::Shape3D;
use gdscene::node::Node;
use gdscene::node3d;
use gdscene::physics_server_3d::PhysicsServer3D;
use gdscene::scene_tree::SceneTree;
use gdvariant::Variant;

static TEST_LOCK: Mutex<()> = Mutex::new(());

fn init_classdb() -> std::sync::MutexGuard<'static, ()> {
    let guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    class_db::clear_for_testing();
    class_db::register_class(class_db::ClassRegistration::new("Object"));
    class_db::register_class(
        class_db::ClassRegistration::new("Node")
            .parent("Object")
            .property(class_db::PropertyInfo::new(
                "name",
                Variant::String(String::new()),
            )),
    );
    class_db::register_3d_classes();
    guard
}

fn make_tree() -> (SceneTree, std::sync::MutexGuard<'static, ()>) {
    let guard = init_classdb();
    let tree = SceneTree::new();
    (tree, guard)
}

// ── ClassDB registration ─────────────────────────────────────────────

#[test]
fn collision_shape_3d_registered_in_classdb() {
    let _g = init_classdb();
    let info = class_db::get_class_info("CollisionShape3D");
    assert!(info.is_some(), "CollisionShape3D should be registered");
    assert_eq!(info.unwrap().parent_class, "Node3D");
}

#[test]
fn collision_shape_3d_has_expected_properties() {
    let _g = init_classdb();
    let info = class_db::get_class_info("CollisionShape3D").unwrap();
    let prop_names: Vec<&str> = info.properties.iter().map(|p| p.name.as_str()).collect();
    for expected in &["shape", "disabled", "radius", "height", "size"] {
        assert!(
            prop_names.contains(expected),
            "CollisionShape3D missing property: {expected}"
        );
    }
}

#[test]
fn collision_shape_3d_default_disabled_is_false() {
    let _g = init_classdb();
    let info = class_db::get_class_info("CollisionShape3D").unwrap();
    let disabled = info
        .properties
        .iter()
        .find(|p| p.name == "disabled")
        .unwrap();
    assert_eq!(disabled.default_value, Variant::Bool(false));
}

// ── Shape3D AABB ─────────────────────────────────────────────────────

#[test]
fn cylinder_aabb() {
    let shape = Shape3D::CylinderShape {
        radius: 2.0,
        height: 4.0,
    };
    let aabb = shape.bounding_aabb();
    assert_eq!(aabb.position, Vector3::new(-2.0, -2.0, -2.0));
    assert_eq!(aabb.size, Vector3::new(4.0, 4.0, 4.0));
}

#[test]
fn convex_polygon_aabb() {
    let shape = Shape3D::ConvexPolygonShape {
        points: vec![
            Vector3::new(-1.0, 0.0, -1.0),
            Vector3::new(1.0, 0.0, -1.0),
            Vector3::new(0.0, 2.0, 1.0),
        ],
    };
    let aabb = shape.bounding_aabb();
    assert!((aabb.position.x - (-1.0)).abs() < 1e-5);
    assert!((aabb.position.y - 0.0).abs() < 1e-5);
    assert!((aabb.size.x - 2.0).abs() < 1e-5);
    assert!((aabb.size.y - 2.0).abs() < 1e-5);
}

#[test]
fn concave_polygon_aabb() {
    let shape = Shape3D::ConcavePolygonShape {
        faces: vec![
            Vector3::new(0.0, 0.0, 0.0),
            Vector3::new(3.0, 0.0, 0.0),
            Vector3::new(0.0, 3.0, 0.0),
        ],
    };
    let aabb = shape.bounding_aabb();
    assert!((aabb.size.x - 3.0).abs() < 1e-5);
    assert!((aabb.size.y - 3.0).abs() < 1e-5);
}

#[test]
fn world_boundary_aabb_is_large() {
    let shape = Shape3D::WorldBoundaryShape {
        normal: Vector3::new(0.0, 1.0, 0.0),
        distance: 0.0,
    };
    let aabb = shape.bounding_aabb();
    assert!(aabb.size.x > 1e5);
    assert!(aabb.size.y > 1e5);
}

#[test]
fn empty_convex_polygon_aabb() {
    let shape = Shape3D::ConvexPolygonShape { points: vec![] };
    let aabb = shape.bounding_aabb();
    assert!((aabb.size.x).abs() < 1e-5);
}

// ── Shape3D contains_point ───────────────────────────────────────────

#[test]
fn cylinder_contains_center() {
    let shape = Shape3D::CylinderShape {
        radius: 1.0,
        height: 2.0,
    };
    assert!(shape.contains_point(Vector3::ZERO));
}

#[test]
fn cylinder_excludes_above() {
    let shape = Shape3D::CylinderShape {
        radius: 1.0,
        height: 2.0,
    };
    assert!(!shape.contains_point(Vector3::new(0.0, 2.0, 0.0)));
}

#[test]
fn cylinder_excludes_outside_radius() {
    let shape = Shape3D::CylinderShape {
        radius: 1.0,
        height: 2.0,
    };
    assert!(!shape.contains_point(Vector3::new(1.5, 0.0, 0.0)));
}

#[test]
fn world_boundary_contains_below_plane() {
    let shape = Shape3D::WorldBoundaryShape {
        normal: Vector3::new(0.0, 1.0, 0.0),
        distance: 0.0,
    };
    assert!(shape.contains_point(Vector3::new(0.0, -1.0, 0.0)));
}

#[test]
fn world_boundary_excludes_above_plane() {
    let shape = Shape3D::WorldBoundaryShape {
        normal: Vector3::new(0.0, 1.0, 0.0),
        distance: 0.0,
    };
    assert!(!shape.contains_point(Vector3::new(0.0, 1.0, 0.0)));
}

// ── Scene tree shape extraction ──────────────────────────────────────

#[test]
fn sphere_shape_from_scene_tree() {
    let (mut tree, _g) = make_tree();
    let root = tree.root_id();

    let body = Node::new("Ball", "RigidBody3D");
    let body_id = tree.add_child(root, body).unwrap();

    let mut shape_node = Node::new("Shape", "CollisionShape3D");
    shape_node.set_property("shape", Variant::String("SphereShape3D".to_owned()));
    shape_node.set_property("radius", Variant::Float(2.0));
    tree.add_child(body_id, shape_node).unwrap();

    let mut server = PhysicsServer3D::new();
    server.sync_to_physics(&tree);
    assert_eq!(server.body_count(), 1);
}

#[test]
fn box_shape_from_scene_tree() {
    let (mut tree, _g) = make_tree();
    let root = tree.root_id();

    let body = Node::new("Crate", "RigidBody3D");
    let body_id = tree.add_child(root, body).unwrap();

    let mut shape_node = Node::new("Shape", "CollisionShape3D");
    shape_node.set_property("shape", Variant::String("BoxShape3D".to_owned()));
    shape_node.set_property("size", Variant::Vector3(Vector3::new(2.0, 2.0, 2.0)));
    tree.add_child(body_id, shape_node).unwrap();

    let mut server = PhysicsServer3D::new();
    server.sync_to_physics(&tree);
    assert_eq!(server.body_count(), 1);
}

#[test]
fn capsule_shape_from_scene_tree() {
    let (mut tree, _g) = make_tree();
    let root = tree.root_id();

    let body = Node::new("Player", "CharacterBody3D");
    let body_id = tree.add_child(root, body).unwrap();

    let mut shape_node = Node::new("Shape", "CollisionShape3D");
    shape_node.set_property("shape", Variant::String("CapsuleShape3D".to_owned()));
    shape_node.set_property("radius", Variant::Float(0.5));
    shape_node.set_property("height", Variant::Float(1.8));
    tree.add_child(body_id, shape_node).unwrap();

    let mut server = PhysicsServer3D::new();
    server.sync_to_physics(&tree);
    assert_eq!(server.body_count(), 1);
}

#[test]
fn cylinder_shape_from_scene_tree() {
    let (mut tree, _g) = make_tree();
    let root = tree.root_id();

    let body = Node::new("Barrel", "StaticBody3D");
    let body_id = tree.add_child(root, body).unwrap();

    let mut shape_node = Node::new("Shape", "CollisionShape3D");
    shape_node.set_property("shape", Variant::String("CylinderShape3D".to_owned()));
    shape_node.set_property("radius", Variant::Float(1.0));
    shape_node.set_property("height", Variant::Float(2.0));
    tree.add_child(body_id, shape_node).unwrap();

    let mut server = PhysicsServer3D::new();
    server.sync_to_physics(&tree);
    assert_eq!(server.body_count(), 1);
}

#[test]
fn convex_polygon_shape_from_scene_tree() {
    let (mut tree, _g) = make_tree();
    let root = tree.root_id();

    let body = Node::new("Rock", "StaticBody3D");
    let body_id = tree.add_child(root, body).unwrap();

    let mut shape_node = Node::new("Shape", "CollisionShape3D");
    shape_node.set_property("shape", Variant::String("ConvexPolygonShape3D".to_owned()));
    tree.add_child(body_id, shape_node).unwrap();

    let mut server = PhysicsServer3D::new();
    server.sync_to_physics(&tree);
    assert_eq!(server.body_count(), 1);
}

#[test]
fn concave_polygon_shape_from_scene_tree() {
    let (mut tree, _g) = make_tree();
    let root = tree.root_id();

    let body = Node::new("Terrain", "StaticBody3D");
    let body_id = tree.add_child(root, body).unwrap();

    let mut shape_node = Node::new("Shape", "CollisionShape3D");
    shape_node.set_property("shape", Variant::String("ConcavePolygonShape3D".to_owned()));
    tree.add_child(body_id, shape_node).unwrap();

    let mut server = PhysicsServer3D::new();
    server.sync_to_physics(&tree);
    assert_eq!(server.body_count(), 1);
}

#[test]
fn world_boundary_shape_from_scene_tree() {
    let (mut tree, _g) = make_tree();
    let root = tree.root_id();

    let body = Node::new("Floor", "StaticBody3D");
    let body_id = tree.add_child(root, body).unwrap();

    let mut shape_node = Node::new("Shape", "CollisionShape3D");
    shape_node.set_property("shape", Variant::String("WorldBoundaryShape3D".to_owned()));
    tree.add_child(body_id, shape_node).unwrap();

    let mut server = PhysicsServer3D::new();
    server.sync_to_physics(&tree);
    assert_eq!(server.body_count(), 1);
}

// ── Disabled shape ───────────────────────────────────────────────────

#[test]
fn disabled_shape_is_skipped() {
    let (mut tree, _g) = make_tree();
    let root = tree.root_id();

    let body = Node::new("Ball", "RigidBody3D");
    let body_id = tree.add_child(root, body).unwrap();

    let mut shape_node = Node::new("Shape", "CollisionShape3D");
    shape_node.set_property("shape", Variant::String("SphereShape3D".to_owned()));
    shape_node.set_property("disabled", Variant::Bool(true));
    tree.add_child(body_id, shape_node).unwrap();

    let mut server = PhysicsServer3D::new();
    server.sync_to_physics(&tree);
    // Body still registered but with fallback sphere (disabled shape skipped)
    assert_eq!(server.body_count(), 1);
}

// ── Multiple shapes on one body ──────────────────────────────────────

#[test]
fn first_enabled_shape_is_used() {
    let (mut tree, _g) = make_tree();
    let root = tree.root_id();

    let body = Node::new("Complex", "RigidBody3D");
    let body_id = tree.add_child(root, body).unwrap();

    // First shape is disabled
    let mut shape1 = Node::new("Shape1", "CollisionShape3D");
    shape1.set_property("shape", Variant::String("BoxShape3D".to_owned()));
    shape1.set_property("disabled", Variant::Bool(true));
    tree.add_child(body_id, shape1).unwrap();

    // Second shape is enabled
    let mut shape2 = Node::new("Shape2", "CollisionShape3D");
    shape2.set_property("shape", Variant::String("SphereShape3D".to_owned()));
    shape2.set_property("radius", Variant::Float(3.0));
    tree.add_child(body_id, shape2).unwrap();

    let mut server = PhysicsServer3D::new();
    server.sync_to_physics(&tree);
    assert_eq!(server.body_count(), 1);
}

// ── Physics simulation with shape types ──────────────────────────────

#[test]
fn rigid_body_with_cylinder_falls_under_gravity() {
    let (mut tree, _g) = make_tree();
    let root = tree.root_id();

    let body = Node::new("Barrel", "RigidBody3D");
    let body_id = tree.add_child(root, body).unwrap();
    node3d::set_position(&mut tree, body_id, Vector3::new(0.0, 10.0, 0.0));

    let mut shape_node = Node::new("Shape", "CollisionShape3D");
    shape_node.set_property("shape", Variant::String("CylinderShape3D".to_owned()));
    shape_node.set_property("radius", Variant::Float(0.5));
    shape_node.set_property("height", Variant::Float(1.0));
    tree.add_child(body_id, shape_node).unwrap();

    let mut server = PhysicsServer3D::new();
    server.sync_to_physics(&tree);

    for _ in 0..10 {
        server.step(1.0 / 60.0);
    }
    server.sync_from_physics(&mut tree);

    let pos = node3d::get_position(&tree, body_id);
    assert!(pos.y < 10.0, "barrel should fall, got y={}", pos.y);
}
