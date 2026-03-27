//! Integration tests for PhysicsRayQuery3D and PhysicsShapeQuery3D spatial queries.
//!
//! Covers ClassDB registration, ray queries (hit/miss/distance/exclusion/mask/box),
//! shape queries (overlap/closest/exclusion/mask/max_results), and world integration.

use gdobject::class_db;
use gdphysics3d::body::{BodyId3D, BodyType3D, PhysicsBody3D};
use gdphysics3d::query::{PhysicsRayQuery3D, PhysicsShapeQuery3D};
use gdphysics3d::shape::Shape3D;
use gdphysics3d::world::PhysicsWorld3D;
use gdcore::math::Vector3;
use gdscene::node::Node;
use gdscene::scene_tree::SceneTree;

// ── Helpers ──────────────────────────────────────────────────────────────────

fn sphere(id: u64, pos: Vector3, radius: f32) -> PhysicsBody3D {
    PhysicsBody3D::new(
        BodyId3D(id),
        BodyType3D::Rigid,
        pos,
        Shape3D::Sphere { radius },
        1.0,
    )
}

fn static_sphere(id: u64, pos: Vector3, radius: f32) -> PhysicsBody3D {
    PhysicsBody3D::new(
        BodyId3D(id),
        BodyType3D::Static,
        pos,
        Shape3D::Sphere { radius },
        0.0,
    )
}

fn box_body(id: u64, pos: Vector3, half: Vector3) -> PhysicsBody3D {
    PhysicsBody3D::new(
        BodyId3D(id),
        BodyType3D::Static,
        pos,
        Shape3D::BoxShape { half_extents: half },
        0.0,
    )
}

// ── ClassDB Registration ─────────────────────────────────────────────────────

#[test]
fn classdb_registers_physics_ray_query_parameters3d() {
    class_db::register_3d_classes();
    assert!(
        class_db::class_exists("PhysicsRayQueryParameters3D"),
        "PhysicsRayQueryParameters3D should be registered"
    );
}

#[test]
fn classdb_ray_query_inherits_refcounted() {
    class_db::register_3d_classes();
    let info = class_db::get_class_info("PhysicsRayQueryParameters3D").unwrap();
    assert_eq!(info.parent_class.as_str(), "RefCounted");
}

#[test]
fn classdb_ray_query_has_properties() {
    class_db::register_3d_classes();
    let props = class_db::get_property_list("PhysicsRayQueryParameters3D", false);
    let names: Vec<&str> = props.iter().map(|p| p.name.as_str()).collect();
    assert!(names.contains(&"from"), "missing 'from' property");
    assert!(names.contains(&"to"), "missing 'to' property");
    assert!(names.contains(&"collision_mask"), "missing 'collision_mask'");
    assert!(names.contains(&"collide_with_bodies"), "missing 'collide_with_bodies'");
    assert!(names.contains(&"collide_with_areas"), "missing 'collide_with_areas'");
    assert!(names.contains(&"hit_back_faces"), "missing 'hit_back_faces'");
    assert!(names.contains(&"hit_from_inside"), "missing 'hit_from_inside'");
}

#[test]
fn classdb_ray_query_has_create_method() {
    class_db::register_3d_classes();
    assert!(class_db::class_has_method("PhysicsRayQueryParameters3D", "create"));
}

#[test]
fn classdb_registers_physics_shape_query_parameters3d() {
    class_db::register_3d_classes();
    assert!(
        class_db::class_exists("PhysicsShapeQueryParameters3D"),
        "PhysicsShapeQueryParameters3D should be registered"
    );
}

#[test]
fn classdb_shape_query_inherits_refcounted() {
    class_db::register_3d_classes();
    let info = class_db::get_class_info("PhysicsShapeQueryParameters3D").unwrap();
    assert_eq!(info.parent_class.as_str(), "RefCounted");
}

#[test]
fn classdb_shape_query_has_properties() {
    class_db::register_3d_classes();
    let props = class_db::get_property_list("PhysicsShapeQueryParameters3D", false);
    let names: Vec<&str> = props.iter().map(|p| p.name.as_str()).collect();
    assert!(names.contains(&"shape"), "missing 'shape'");
    assert!(names.contains(&"position"), "missing 'position'");
    assert!(names.contains(&"collision_mask"), "missing 'collision_mask'");
    assert!(names.contains(&"collide_with_bodies"), "missing 'collide_with_bodies'");
    assert!(names.contains(&"collide_with_areas"), "missing 'collide_with_areas'");
    assert!(names.contains(&"motion"), "missing 'motion'");
    assert!(names.contains(&"margin"), "missing 'margin'");
    assert!(names.contains(&"max_results"), "missing 'max_results'");
}

// ── SceneTree Integration ────────────────────────────────────────────────────

#[test]
fn scene_tree_ray_query_node() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let node = Node::new("RayQuery", "PhysicsRayQueryParameters3D");
    tree.add_child(root, node).unwrap();
    assert_eq!(tree.node_count(), 2);
}

#[test]
fn scene_tree_shape_query_node() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let node = Node::new("ShapeQuery", "PhysicsShapeQueryParameters3D");
    tree.add_child(root, node).unwrap();
    assert_eq!(tree.node_count(), 2);
}

// ── PhysicsRayQuery3D API ────────────────────────────────────────────────────

#[test]
fn ray_query_defaults() {
    let q = PhysicsRayQuery3D::new(Vector3::ZERO, Vector3::new(0.0, 0.0, 10.0));
    assert_eq!(q.collision_mask, 0xFFFFFFFF);
    assert!(q.exclude.is_empty());
    assert!(q.collide_with_bodies);
    assert!(!q.collide_with_areas);
    assert!(!q.hit_back_faces);
    assert!(!q.hit_from_inside);
}

#[test]
fn ray_query_direction_length() {
    let q = PhysicsRayQuery3D::new(Vector3::ZERO, Vector3::new(3.0, 4.0, 0.0));
    assert!((q.max_distance() - 5.0).abs() < 0.01);
    let dir = q.direction();
    assert!((dir.x - 0.6).abs() < 0.01);
    assert!((dir.y - 0.8).abs() < 0.01);
}

#[test]
fn ray_hits_sphere_at_correct_distance() {
    let bodies = vec![sphere(1, Vector3::new(0.0, 0.0, 10.0), 1.0)];
    let q = PhysicsRayQuery3D::new(Vector3::ZERO, Vector3::new(0.0, 0.0, 20.0));
    let hit = q.intersect(bodies.iter()).unwrap();
    assert_eq!(hit.body_id, BodyId3D(1));
    assert!((hit.distance - 9.0).abs() < 0.01, "should hit at z=9 (sphere surface)");
}

#[test]
fn ray_misses_sphere_off_axis() {
    let bodies = vec![sphere(1, Vector3::new(10.0, 0.0, 0.0), 1.0)];
    let q = PhysicsRayQuery3D::new(Vector3::ZERO, Vector3::new(0.0, 0.0, 20.0));
    assert!(q.intersect(bodies.iter()).is_none());
}

#[test]
fn ray_respects_max_distance() {
    let bodies = vec![sphere(1, Vector3::new(0.0, 0.0, 10.0), 1.0)];
    let q = PhysicsRayQuery3D::new(Vector3::ZERO, Vector3::new(0.0, 0.0, 5.0));
    assert!(q.intersect(bodies.iter()).is_none(), "ray too short to reach sphere");
}

#[test]
fn ray_excludes_body_by_id() {
    let bodies = vec![sphere(1, Vector3::new(0.0, 0.0, 10.0), 1.0)];
    let mut q = PhysicsRayQuery3D::new(Vector3::ZERO, Vector3::new(0.0, 0.0, 20.0));
    q.exclude.insert(BodyId3D(1));
    assert!(q.intersect(bodies.iter()).is_none());
}

#[test]
fn ray_filters_by_collision_mask() {
    let mut body = sphere(1, Vector3::new(0.0, 0.0, 10.0), 1.0);
    body.collision_layer = 0b0010;
    let bodies = vec![body];

    let mut q = PhysicsRayQuery3D::new(Vector3::ZERO, Vector3::new(0.0, 0.0, 20.0));
    q.collision_mask = 0b0001;
    assert!(q.intersect(bodies.iter()).is_none(), "mask doesn't match layer");

    q.collision_mask = 0b0010;
    assert!(q.intersect(bodies.iter()).is_some(), "mask matches layer");
}

#[test]
fn ray_picks_closest_of_multiple() {
    let bodies = vec![
        sphere(1, Vector3::new(0.0, 0.0, 10.0), 1.0),
        sphere(2, Vector3::new(0.0, 0.0, 5.0), 1.0),
    ];
    let q = PhysicsRayQuery3D::new(Vector3::ZERO, Vector3::new(0.0, 0.0, 20.0));
    let hit = q.intersect(bodies.iter()).unwrap();
    assert_eq!(hit.body_id, BodyId3D(2), "should hit closer body");
}

#[test]
fn ray_hits_box_face() {
    let bodies = vec![box_body(
        1,
        Vector3::new(0.0, 0.0, 10.0),
        Vector3::new(2.0, 2.0, 2.0),
    )];
    let q = PhysicsRayQuery3D::new(Vector3::ZERO, Vector3::new(0.0, 0.0, 20.0));
    let hit = q.intersect(bodies.iter()).unwrap();
    assert!((hit.point.z - 8.0).abs() < 0.01, "should hit box face at z=8");
    assert!((hit.normal.z - (-1.0)).abs() < 0.01, "normal points toward ray origin");
}

#[test]
fn ray_zero_length_returns_none() {
    let bodies = vec![sphere(1, Vector3::ZERO, 1.0)];
    let q = PhysicsRayQuery3D::new(Vector3::ZERO, Vector3::ZERO);
    assert!(q.intersect(bodies.iter()).is_none());
}

#[test]
fn ray_behind_origin_no_hit() {
    let bodies = vec![sphere(1, Vector3::new(0.0, 0.0, -10.0), 1.0)];
    let q = PhysicsRayQuery3D::new(Vector3::ZERO, Vector3::new(0.0, 0.0, 20.0));
    assert!(q.intersect(bodies.iter()).is_none());
}

#[test]
fn ray_diagonal_hit() {
    let bodies = vec![sphere(1, Vector3::new(10.0, 10.0, 0.0), 2.0)];
    let q = PhysicsRayQuery3D::new(Vector3::ZERO, Vector3::new(20.0, 20.0, 0.0));
    assert!(q.intersect(bodies.iter()).is_some(), "diagonal ray should hit offset sphere");
}

#[test]
fn ray_multiple_exclusions() {
    let bodies = vec![
        sphere(1, Vector3::new(0.0, 0.0, 5.0), 1.0),
        sphere(2, Vector3::new(0.0, 0.0, 10.0), 1.0),
    ];
    let mut q = PhysicsRayQuery3D::new(Vector3::ZERO, Vector3::new(0.0, 0.0, 20.0));
    q.exclude.insert(BodyId3D(1));
    q.exclude.insert(BodyId3D(2));
    assert!(q.intersect(bodies.iter()).is_none());
}

// ── PhysicsShapeQuery3D API ──────────────────────────────────────────────────

#[test]
fn shape_query_defaults() {
    let q = PhysicsShapeQuery3D::new(Shape3D::Sphere { radius: 1.0 }, Vector3::ZERO);
    assert_eq!(q.collision_mask, 0xFFFFFFFF);
    assert!(q.exclude.is_empty());
    assert_eq!(q.max_results, 32);
    assert!(q.collide_with_bodies);
    assert!(!q.collide_with_areas);
    assert_eq!(q.motion, Vector3::ZERO);
    assert!((q.margin - 0.0).abs() < f32::EPSILON);
}

#[test]
fn shape_finds_overlapping_body() {
    let bodies = vec![sphere(1, Vector3::new(1.0, 0.0, 0.0), 1.0)];
    let q = PhysicsShapeQuery3D::new(Shape3D::Sphere { radius: 2.0 }, Vector3::ZERO);
    let results = q.intersect(bodies.iter());
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].body_id, BodyId3D(1));
}

#[test]
fn shape_no_overlap_far_body() {
    let bodies = vec![sphere(1, Vector3::new(100.0, 0.0, 0.0), 1.0)];
    let q = PhysicsShapeQuery3D::new(Shape3D::Sphere { radius: 1.0 }, Vector3::ZERO);
    assert!(q.intersect(bodies.iter()).is_empty());
}

#[test]
fn shape_excludes_body() {
    let bodies = vec![sphere(1, Vector3::new(1.0, 0.0, 0.0), 1.0)];
    let mut q = PhysicsShapeQuery3D::new(Shape3D::Sphere { radius: 5.0 }, Vector3::ZERO);
    q.exclude.insert(BodyId3D(1));
    assert!(q.intersect(bodies.iter()).is_empty());
}

#[test]
fn shape_collision_mask_filter() {
    let mut body = sphere(1, Vector3::new(1.0, 0.0, 0.0), 1.0);
    body.collision_layer = 0b1000;
    let bodies = vec![body];

    let mut q = PhysicsShapeQuery3D::new(Shape3D::Sphere { radius: 5.0 }, Vector3::ZERO);
    q.collision_mask = 0b0001;
    assert!(q.intersect(bodies.iter()).is_empty());

    q.collision_mask = 0b1000;
    assert_eq!(q.intersect(bodies.iter()).len(), 1);
}

#[test]
fn shape_max_results_limits_output() {
    let bodies = vec![
        sphere(1, Vector3::new(1.0, 0.0, 0.0), 1.0),
        sphere(2, Vector3::new(0.0, 1.0, 0.0), 1.0),
        sphere(3, Vector3::new(0.0, 0.0, 1.0), 1.0),
    ];
    let mut q = PhysicsShapeQuery3D::new(Shape3D::Sphere { radius: 5.0 }, Vector3::ZERO);
    q.max_results = 2;
    assert_eq!(q.intersect(bodies.iter()).len(), 2);
}

#[test]
fn shape_closest_finds_deepest_overlap() {
    let bodies = vec![
        sphere(1, Vector3::new(3.0, 0.0, 0.0), 1.0),
        sphere(2, Vector3::new(0.5, 0.0, 0.0), 1.0),
    ];
    let q = PhysicsShapeQuery3D::new(Shape3D::Sphere { radius: 5.0 }, Vector3::ZERO);
    let closest = q.intersect_closest(bodies.iter()).unwrap();
    assert_eq!(closest.body_id, BodyId3D(2), "body 2 is closer → deeper overlap");
}

#[test]
fn shape_closest_returns_none_for_no_overlap() {
    let bodies = vec![sphere(1, Vector3::new(100.0, 0.0, 0.0), 1.0)];
    let q = PhysicsShapeQuery3D::new(Shape3D::Sphere { radius: 1.0 }, Vector3::ZERO);
    assert!(q.intersect_closest(bodies.iter()).is_none());
}

#[test]
fn shape_multiple_overlaps() {
    let bodies = vec![
        sphere(1, Vector3::new(1.0, 0.0, 0.0), 1.0),
        sphere(2, Vector3::new(0.0, 1.0, 0.0), 1.0),
        sphere(3, Vector3::new(0.0, 0.0, 1.0), 1.0),
        sphere(4, Vector3::new(100.0, 0.0, 0.0), 1.0),
    ];
    let q = PhysicsShapeQuery3D::new(Shape3D::Sphere { radius: 3.0 }, Vector3::ZERO);
    let results = q.intersect(bodies.iter());
    assert_eq!(results.len(), 3, "should find 3 overlapping, skip 1 far body");
}

#[test]
fn shape_box_query_against_spheres() {
    let bodies = vec![
        sphere(1, Vector3::new(1.0, 0.0, 0.0), 0.5),
        sphere(2, Vector3::new(100.0, 0.0, 0.0), 0.5),
    ];
    let q = PhysicsShapeQuery3D::new(
        Shape3D::BoxShape {
            half_extents: Vector3::new(3.0, 3.0, 3.0),
        },
        Vector3::ZERO,
    );
    let results = q.intersect(bodies.iter());
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].body_id, BodyId3D(1));
}

#[test]
fn shape_query_reports_depth_and_normal() {
    let bodies = vec![sphere(1, Vector3::new(1.0, 0.0, 0.0), 1.0)];
    let q = PhysicsShapeQuery3D::new(Shape3D::Sphere { radius: 2.0 }, Vector3::ZERO);
    let results = q.intersect(bodies.iter());
    assert_eq!(results.len(), 1);
    assert!(results[0].depth > 0.0, "depth should be positive for overlap");
    assert!(results[0].normal.x > 0.0, "normal should point toward body");
}

// ── World Integration ────────────────────────────────────────────────────────

#[test]
fn ray_query_against_world_bodies() {
    let mut world = PhysicsWorld3D::new();
    world.gravity = Vector3::ZERO;
    let id1 = world.add_body(sphere(0, Vector3::new(0.0, 0.0, 5.0), 1.0));
    let _id2 = world.add_body(sphere(0, Vector3::new(0.0, 0.0, 10.0), 1.0));

    let hit = world.raycast(Vector3::ZERO, Vector3::new(0.0, 0.0, 1.0));
    assert!(hit.is_some());
    assert_eq!(hit.unwrap().body_id, id1, "should hit closer body");
}

#[test]
fn shape_query_against_world_after_step() {
    let mut world = PhysicsWorld3D::new();
    world.gravity = Vector3::ZERO;
    let id = world.add_body(sphere(0, Vector3::new(2.0, 0.0, 0.0), 1.0));
    world.step(1.0 / 60.0);

    let body = world.get_body(id).unwrap();
    let q = PhysicsShapeQuery3D::new(Shape3D::Sphere { radius: 5.0 }, Vector3::ZERO);
    let results = q.intersect(std::iter::once(body));
    assert_eq!(results.len(), 1);
}

#[test]
fn ray_query_with_static_bodies() {
    let bodies = vec![
        static_sphere(1, Vector3::new(0.0, 0.0, 5.0), 1.0),
        static_sphere(2, Vector3::new(0.0, 0.0, 15.0), 1.0),
    ];
    let q = PhysicsRayQuery3D::new(Vector3::ZERO, Vector3::new(0.0, 0.0, 20.0));
    let hit = q.intersect(bodies.iter()).unwrap();
    assert_eq!(hit.body_id, BodyId3D(1));
}

#[test]
fn shape_query_mixed_body_types() {
    let mut bodies = vec![
        sphere(1, Vector3::new(1.0, 0.0, 0.0), 1.0),
        static_sphere(2, Vector3::new(0.0, 1.0, 0.0), 1.0),
    ];
    bodies.push(PhysicsBody3D::new(
        BodyId3D(3),
        BodyType3D::Kinematic,
        Vector3::new(0.0, 0.0, 1.0),
        Shape3D::Sphere { radius: 1.0 },
        1.0,
    ));
    let q = PhysicsShapeQuery3D::new(Shape3D::Sphere { radius: 3.0 }, Vector3::ZERO);
    let results = q.intersect(bodies.iter());
    assert_eq!(results.len(), 3, "should detect all body types");
}

#[test]
fn ray_query_through_multiple_aligned_spheres() {
    let bodies = vec![
        sphere(1, Vector3::new(0.0, 0.0, 5.0), 1.0),
        sphere(2, Vector3::new(0.0, 0.0, 10.0), 1.0),
        sphere(3, Vector3::new(0.0, 0.0, 15.0), 1.0),
    ];
    let q = PhysicsRayQuery3D::new(Vector3::ZERO, Vector3::new(0.0, 0.0, 20.0));
    let hit = q.intersect(bodies.iter()).unwrap();
    assert_eq!(hit.body_id, BodyId3D(1), "first sphere in line");
}

#[test]
fn shape_query_at_offset_position() {
    let bodies = vec![
        sphere(1, Vector3::new(51.0, 50.0, 0.0), 1.0), // 1 unit away from query
        sphere(2, Vector3::ZERO, 1.0),                   // far from query
    ];
    let q = PhysicsShapeQuery3D::new(
        Shape3D::Sphere { radius: 3.0 },
        Vector3::new(50.0, 50.0, 0.0),
    );
    let results = q.intersect(bodies.iter());
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].body_id, BodyId3D(1));
}

#[test]
fn shape_query_multiple_exclusions() {
    let bodies = vec![
        sphere(1, Vector3::new(1.0, 0.0, 0.0), 1.0),
        sphere(2, Vector3::new(0.0, 1.0, 0.0), 1.0),
        sphere(3, Vector3::new(0.0, 0.0, 1.0), 1.0),
    ];
    let mut q = PhysicsShapeQuery3D::new(Shape3D::Sphere { radius: 5.0 }, Vector3::ZERO);
    q.exclude.insert(BodyId3D(1));
    q.exclude.insert(BodyId3D(3));
    let results = q.intersect(bodies.iter());
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].body_id, BodyId3D(2));
}
