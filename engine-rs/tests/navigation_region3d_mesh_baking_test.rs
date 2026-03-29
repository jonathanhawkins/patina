//! pat-jjw67: NavigationRegion3D with 3D navigation mesh baking.
//!
//! Integration tests covering:
//! 1. ClassDB registration (properties, inheritance)
//! 2. NavigationMesh3D — defaults, vertices, polygons, AABB, clear
//! 3. BakeSourceGeometry3D — triangle collection
//! 4. Baking pipeline — slope filtering, degenerate triangles, parameter preservation
//! 5. NavigationRegion3D — defaults, with_mesh, world AABB, navigation layers
//! 6. Region bake integration — bake from source geometry
//! 7. Polygon queries — find_closest_polygon, polygon_center
//! 8. Scene tree integration

use gdcore::math::Vector3;
use gdscene::node::Node;
use gdscene::scene_tree::SceneTree;
use gdserver3d::navigation::*;
use gdvariant::Variant;

const EPSILON: f32 = 1e-4;

fn approx(a: f32, b: f32) -> bool {
    (a - b).abs() < EPSILON
}

// ===========================================================================
// 1. ClassDB registration
// ===========================================================================

#[test]
fn classdb_registers_navigation_region3d() {
    gdobject::class_db::register_3d_classes();
    assert!(gdobject::class_db::class_exists("NavigationRegion3D"));
}

#[test]
fn classdb_navigation_region3d_inherits_node3d() {
    gdobject::class_db::register_3d_classes();
    let info = gdobject::class_db::get_class_info("NavigationRegion3D").unwrap();
    assert_eq!(info.parent_class.as_str(), "Node3D");
}

#[test]
fn classdb_navigation_region3d_properties() {
    gdobject::class_db::register_3d_classes();
    let props = gdobject::class_db::get_property_list("NavigationRegion3D");
    let names: Vec<&str> = props.iter().map(|p| p.name.as_str()).collect();
    assert!(names.contains(&"navigation_mesh"));
    assert!(names.contains(&"enabled"));
    assert!(names.contains(&"navigation_layers"));
    assert!(names.contains(&"enter_cost"));
}

#[test]
fn classdb_navigation_region3d_default_enabled() {
    gdobject::class_db::register_3d_classes();
    let props = gdobject::class_db::get_property_list("NavigationRegion3D");
    let prop = props.iter().find(|p| p.name == "enabled").unwrap();
    assert_eq!(prop.default_value, Variant::Bool(true));
}

#[test]
fn classdb_navigation_region3d_default_layers() {
    gdobject::class_db::register_3d_classes();
    let props = gdobject::class_db::get_property_list("NavigationRegion3D");
    let prop = props.iter().find(|p| p.name == "navigation_layers").unwrap();
    assert_eq!(prop.default_value, Variant::Int(1));
}

// ===========================================================================
// 2. NavigationMesh3D — defaults and basic operations
// ===========================================================================

#[test]
fn nav_mesh_defaults_match_godot() {
    let mesh = NavigationMesh3D::new();
    assert!(approx(mesh.cell_size, 0.25));
    assert!(approx(mesh.cell_height, 0.25));
    assert!(approx(mesh.agent_height, 1.5));
    assert!(approx(mesh.agent_radius, 0.5));
    assert!(approx(mesh.agent_max_climb, 0.25));
    assert!(approx(mesh.agent_max_slope, 45.0));
    assert!(approx(mesh.region_min_size, 8.0));
    assert!(approx(mesh.edge_max_length, 0.6));
    assert_eq!(mesh.vertex_count(), 0);
    assert_eq!(mesh.polygon_count(), 0);
}

#[test]
fn nav_mesh_add_vertices_and_polygon() {
    let mut mesh = NavigationMesh3D::new();
    mesh.vertices = vec![
        Vector3::new(0.0, 0.0, 0.0),
        Vector3::new(5.0, 0.0, 0.0),
        Vector3::new(0.0, 0.0, 5.0),
        Vector3::new(5.0, 0.0, 5.0),
    ];
    assert_eq!(mesh.vertex_count(), 4);
    assert!(mesh.add_polygon(&[0, 1, 2]));
    assert!(mesh.add_polygon(&[1, 3, 2]));
    assert_eq!(mesh.polygon_count(), 2);
}

#[test]
fn nav_mesh_reject_invalid_indices() {
    let mut mesh = NavigationMesh3D::new();
    mesh.vertices = vec![Vector3::ZERO, Vector3::new(1.0, 0.0, 0.0)];
    assert!(!mesh.add_polygon(&[0, 1, 5])); // index 5 out of bounds
    assert_eq!(mesh.polygon_count(), 0);
}

#[test]
fn nav_mesh_aabb_computation() {
    let mut mesh = NavigationMesh3D::new();
    mesh.vertices = vec![
        Vector3::new(-2.0, -1.0, 0.0),
        Vector3::new(3.0, 0.0, 0.0),
        Vector3::new(0.0, 2.0, 5.0),
    ];
    let aabb = mesh.get_aabb();
    assert!(approx(aabb.position.x, -2.0));
    assert!(approx(aabb.position.y, -1.0));
    assert!(approx(aabb.position.z, 0.0));
    assert!(approx(aabb.size.x, 5.0));
    assert!(approx(aabb.size.y, 3.0));
    assert!(approx(aabb.size.z, 5.0));
}

#[test]
fn nav_mesh_empty_aabb_is_zero() {
    let mesh = NavigationMesh3D::new();
    let aabb = mesh.get_aabb();
    assert!(approx(aabb.size.x, 0.0));
    assert!(approx(aabb.size.y, 0.0));
    assert!(approx(aabb.size.z, 0.0));
}

#[test]
fn nav_mesh_clear_removes_all_data() {
    let mut mesh = NavigationMesh3D::new();
    mesh.vertices = vec![Vector3::ZERO, Vector3::new(1.0, 0.0, 0.0), Vector3::new(0.0, 0.0, 1.0)];
    mesh.add_polygon(&[0, 1, 2]);
    assert_eq!(mesh.polygon_count(), 1);
    mesh.clear();
    assert_eq!(mesh.vertex_count(), 0);
    assert_eq!(mesh.polygon_count(), 0);
}

// ===========================================================================
// 3. BakeSourceGeometry3D
// ===========================================================================

#[test]
fn source_geometry_triangle_count() {
    let mut src = BakeSourceGeometry3D::new();
    assert_eq!(src.triangle_count(), 0);
    src.add_triangle(
        Vector3::new(0.0, 0.0, 0.0),
        Vector3::new(1.0, 0.0, 0.0),
        Vector3::new(0.0, 0.0, 1.0),
    );
    assert_eq!(src.triangle_count(), 1);
    src.add_triangle(
        Vector3::new(1.0, 0.0, 0.0),
        Vector3::new(1.0, 0.0, 1.0),
        Vector3::new(0.0, 0.0, 1.0),
    );
    assert_eq!(src.triangle_count(), 2);
}

// ===========================================================================
// 4. Baking pipeline
// ===========================================================================

#[test]
fn bake_flat_floor_produces_polygon() {
    let params = NavigationMesh3D::new();
    let mut src = BakeSourceGeometry3D::new();
    src.add_triangle(
        Vector3::new(0.0, 0.0, 0.0),
        Vector3::new(10.0, 0.0, 0.0),
        Vector3::new(0.0, 0.0, 10.0),
    );
    let result = bake_navigation_mesh(&params, &src);
    assert_eq!(result.polygon_count(), 1);
    assert_eq!(result.vertex_count(), 3);
}

#[test]
fn bake_vertical_wall_excluded_by_slope() {
    let params = NavigationMesh3D::new(); // max_slope = 45°
    let mut src = BakeSourceGeometry3D::new();
    // Vertical wall triangle — normal is horizontal, dot with up = 0
    src.add_triangle(
        Vector3::new(0.0, 0.0, 0.0),
        Vector3::new(0.0, 10.0, 0.0),
        Vector3::new(0.0, 0.0, 10.0),
    );
    let result = bake_navigation_mesh(&params, &src);
    assert_eq!(result.polygon_count(), 0);
}

#[test]
fn bake_degenerate_triangle_skipped() {
    let params = NavigationMesh3D::new();
    let mut src = BakeSourceGeometry3D::new();
    src.add_triangle(Vector3::ZERO, Vector3::ZERO, Vector3::ZERO);
    let result = bake_navigation_mesh(&params, &src);
    assert_eq!(result.polygon_count(), 0);
}

#[test]
fn bake_max_slope_90_accepts_all() {
    let mut params = NavigationMesh3D::new();
    params.agent_max_slope = 90.0;
    let mut src = BakeSourceGeometry3D::new();
    // 45° slope
    src.add_triangle(
        Vector3::new(0.0, 0.0, 0.0),
        Vector3::new(10.0, 10.0, 0.0),
        Vector3::new(0.0, 0.0, 10.0),
    );
    // Vertical wall
    src.add_triangle(
        Vector3::new(0.0, 0.0, 0.0),
        Vector3::new(0.0, 10.0, 0.0),
        Vector3::new(0.0, 0.0, 10.0),
    );
    let result = bake_navigation_mesh(&params, &src);
    assert_eq!(result.polygon_count(), 2, "90° max slope should accept everything");
}

#[test]
fn bake_preserves_parameters() {
    let mut params = NavigationMesh3D::new();
    params.cell_size = 0.1;
    params.agent_height = 2.5;
    params.agent_radius = 0.8;
    let src = BakeSourceGeometry3D::new();
    let result = bake_navigation_mesh(&params, &src);
    assert!(approx(result.cell_size, 0.1));
    assert!(approx(result.agent_height, 2.5));
    assert!(approx(result.agent_radius, 0.8));
}

#[test]
fn bake_multiple_floor_triangles() {
    let params = NavigationMesh3D::new();
    let mut src = BakeSourceGeometry3D::new();
    src.add_triangle(
        Vector3::new(0.0, 0.0, 0.0),
        Vector3::new(5.0, 0.0, 0.0),
        Vector3::new(0.0, 0.0, 5.0),
    );
    src.add_triangle(
        Vector3::new(5.0, 0.0, 0.0),
        Vector3::new(5.0, 0.0, 5.0),
        Vector3::new(0.0, 0.0, 5.0),
    );
    let result = bake_navigation_mesh(&params, &src);
    assert_eq!(result.polygon_count(), 2);
    assert_eq!(result.vertex_count(), 6);
}

// ===========================================================================
// 5. NavigationRegion3D — struct operations
// ===========================================================================

#[test]
fn region_defaults_match_godot() {
    let region = NavigationRegion3D::new();
    assert!(region.enabled);
    assert!(region.navigation_mesh.is_none());
    assert_eq!(region.navigation_layers, 1);
    assert!(approx(region.enter_cost, 0.0));
    assert!(approx(region.travel_cost, 1.0));
}

#[test]
fn region_with_mesh_constructor() {
    let mesh = NavigationMesh3D::new();
    let region = NavigationRegion3D::with_mesh(mesh);
    assert!(region.navigation_mesh.is_some());
    assert!(region.enabled);
}

#[test]
fn region_world_aabb_none_without_mesh() {
    let region = NavigationRegion3D::new();
    assert!(region.get_world_aabb().is_none());
}

#[test]
fn region_world_aabb_with_mesh() {
    let mut mesh = NavigationMesh3D::new();
    mesh.vertices = vec![
        Vector3::new(0.0, 0.0, 0.0),
        Vector3::new(10.0, 0.0, 0.0),
        Vector3::new(0.0, 0.0, 10.0),
    ];
    let region = NavigationRegion3D::with_mesh(mesh);
    let aabb = region.get_world_aabb().unwrap();
    assert!(approx(aabb.position.x, 0.0));
    assert!(approx(aabb.size.x, 10.0));
    assert!(approx(aabb.size.z, 10.0));
}

#[test]
fn region_world_aabb_translated() {
    let mut mesh = NavigationMesh3D::new();
    mesh.vertices = vec![
        Vector3::new(0.0, 0.0, 0.0),
        Vector3::new(1.0, 0.0, 0.0),
        Vector3::new(0.0, 0.0, 1.0),
    ];
    let mut region = NavigationRegion3D::with_mesh(mesh);
    region.transform.origin = Vector3::new(50.0, 10.0, 0.0);
    let aabb = region.get_world_aabb().unwrap();
    assert!(approx(aabb.position.x, 50.0));
    assert!(approx(aabb.position.y, 10.0));
}

#[test]
fn region_navigation_layers_get_set() {
    let mut region = NavigationRegion3D::new();
    assert!(region.get_navigation_layer_value(1));
    assert!(!region.get_navigation_layer_value(2));

    region.set_navigation_layer_value(2, true);
    assert!(region.get_navigation_layer_value(2));
    assert!(region.get_navigation_layer_value(1)); // unchanged

    region.set_navigation_layer_value(1, false);
    assert!(!region.get_navigation_layer_value(1));
    assert!(region.get_navigation_layer_value(2));
}

#[test]
fn region_navigation_layers_boundary_values() {
    let region = NavigationRegion3D::new();
    assert!(!region.get_navigation_layer_value(0));  // invalid
    assert!(!region.get_navigation_layer_value(33)); // invalid
}

// ===========================================================================
// 6. Region bake integration
// ===========================================================================

#[test]
fn region_bake_from_source_geometry() {
    let mut region = NavigationRegion3D::new();
    let mut src = BakeSourceGeometry3D::new();
    src.add_triangle(
        Vector3::new(0.0, 0.0, 0.0),
        Vector3::new(10.0, 0.0, 0.0),
        Vector3::new(0.0, 0.0, 10.0),
    );
    region.bake(&src);
    assert!(region.navigation_mesh.is_some());
    let mesh = region.navigation_mesh.as_ref().unwrap();
    assert_eq!(mesh.polygon_count(), 1);
    assert_eq!(mesh.vertex_count(), 3);
}

#[test]
fn region_bake_preserves_existing_params() {
    let mut mesh = NavigationMesh3D::new();
    mesh.cell_size = 0.1;
    mesh.agent_height = 3.0;
    let mut region = NavigationRegion3D::with_mesh(mesh);
    let mut src = BakeSourceGeometry3D::new();
    src.add_triangle(
        Vector3::new(0.0, 0.0, 0.0),
        Vector3::new(5.0, 0.0, 0.0),
        Vector3::new(0.0, 0.0, 5.0),
    );
    region.bake(&src);
    let mesh = region.navigation_mesh.as_ref().unwrap();
    assert!(approx(mesh.cell_size, 0.1));
    assert!(approx(mesh.agent_height, 3.0));
}

// ===========================================================================
// 7. Polygon queries
// ===========================================================================

#[test]
fn find_closest_polygon_selects_nearest() {
    let mut mesh = NavigationMesh3D::new();
    mesh.vertices = vec![
        // Polygon 0 centered near origin
        Vector3::new(-1.0, 0.0, -1.0),
        Vector3::new(1.0, 0.0, -1.0),
        Vector3::new(0.0, 0.0, 1.0),
        // Polygon 1 centered near (20, 0, 0)
        Vector3::new(19.0, 0.0, -1.0),
        Vector3::new(21.0, 0.0, -1.0),
        Vector3::new(20.0, 0.0, 1.0),
    ];
    mesh.add_polygon(&[0, 1, 2]);
    mesh.add_polygon(&[3, 4, 5]);

    assert_eq!(mesh.find_closest_polygon(Vector3::new(0.0, 0.0, 0.0)), Some(0));
    assert_eq!(mesh.find_closest_polygon(Vector3::new(20.0, 0.0, 0.0)), Some(1));
    assert_eq!(mesh.find_closest_polygon(Vector3::new(15.0, 0.0, 0.0)), Some(1));
}

#[test]
fn polygon_center_computation() {
    let mut mesh = NavigationMesh3D::new();
    mesh.vertices = vec![
        Vector3::new(0.0, 0.0, 0.0),
        Vector3::new(3.0, 0.0, 0.0),
        Vector3::new(0.0, 0.0, 6.0),
    ];
    mesh.add_polygon(&[0, 1, 2]);
    let center = mesh.polygon_center(0).unwrap();
    assert!(approx(center.x, 1.0));
    assert!(approx(center.z, 2.0));
}

#[test]
fn polygon_center_out_of_bounds_returns_none() {
    let mesh = NavigationMesh3D::new();
    assert!(mesh.polygon_center(0).is_none());
}

// ===========================================================================
// 8. Scene tree integration
// ===========================================================================

#[test]
fn scene_tree_navigation_region3d_node() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let node = Node::new("NavRegion", "NavigationRegion3D");
    let id = tree.add_child(root, node).unwrap();
    let n = tree.get_node(id).unwrap();
    assert_eq!(n.class_name(), "NavigationRegion3D");
    assert_eq!(n.name(), "NavRegion");
}
