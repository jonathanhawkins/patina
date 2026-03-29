//! pat-k6wr1: FogVolume node for volumetric fog regions.
//!
//! Integration tests covering:
//! 1. ClassDB registration (properties, inheritance)
//! 2. Scene tree integration (node creation, path lookup, properties)
//! 3. FogVolumeShape variants and conversions
//! 4. FogMaterial defaults and configuration
//! 5. FogVolume constructors
//! 6. Point containment for all shapes (Box, Ellipsoid, Cylinder, Cone, World)
//! 7. Density sampling with height falloff and edge fade
//! 8. Multiple fog volumes in scene tree

use gdcore::math::{Color, Vector3};
use gdscene::node::Node;
use gdscene::scene_tree::SceneTree;
use gdserver3d::fog_volume::*;
use gdvariant::Variant;

const EPSILON: f32 = 1e-3;

fn approx(a: f32, b: f32) -> bool {
    (a - b).abs() < EPSILON
}

// ===========================================================================
// 1. ClassDB registration
// ===========================================================================

#[test]
fn classdb_registers_fogvolume() {
    gdobject::class_db::register_3d_classes();
    assert!(gdobject::class_db::class_exists("FogVolume"));
}

#[test]
fn classdb_fogvolume_inherits_node3d() {
    gdobject::class_db::register_3d_classes();
    let info = gdobject::class_db::get_class_info("FogVolume").unwrap();
    assert_eq!(info.parent_class.as_str(), "Node3D");
}

#[test]
fn classdb_fogvolume_has_properties() {
    gdobject::class_db::register_3d_classes();
    let props = gdobject::class_db::get_property_list("FogVolume");
    let names: Vec<&str> = props.iter().map(|p| p.name.as_str()).collect();
    assert!(names.contains(&"size"), "Missing 'size' property");
    assert!(names.contains(&"shape"), "Missing 'shape' property");
    assert!(names.contains(&"material"), "Missing 'material' property");
}

#[test]
fn classdb_fogvolume_default_size() {
    gdobject::class_db::register_3d_classes();
    let props = gdobject::class_db::get_property_list("FogVolume");
    let size = props.iter().find(|p| p.name == "size").unwrap();
    assert_eq!(
        size.default_value,
        Variant::Vector3(Vector3::new(2.0, 2.0, 2.0))
    );
}

// ===========================================================================
// 2. Scene tree integration
// ===========================================================================

#[test]
fn fogvolume_node_creation() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let node = Node::new("Mist", "FogVolume");
    let id = tree.add_child(root, node).unwrap();
    assert_eq!(tree.get_node(id).unwrap().class_name(), "FogVolume");
    assert_eq!(tree.get_node(id).unwrap().name(), "Mist");
}

#[test]
fn fogvolume_path_lookup() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let world = Node::new("World", "Node3D");
    let wid = tree.add_child(root, world).unwrap();
    let fog = Node::new("ValleyFog", "FogVolume");
    tree.add_child(wid, fog).unwrap();

    let found = tree.get_node_by_path("/root/World/ValleyFog");
    assert!(found.is_some());
    assert_eq!(
        tree.get_node(found.unwrap()).unwrap().class_name(),
        "FogVolume"
    );
}

#[test]
fn fogvolume_set_get_properties() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let node = Node::new("F", "FogVolume");
    let id = tree.add_child(root, node).unwrap();

    tree.get_node_mut(id)
        .unwrap()
        .set_property("shape", Variant::Int(3)); // Box
    assert_eq!(
        tree.get_node(id).unwrap().get_property("shape"),
        Variant::Int(3)
    );
}

#[test]
fn fogvolume_multiple_instances() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let a = Node::new("Fog1", "FogVolume");
    let aid = tree.add_child(root, a).unwrap();
    let b = Node::new("Fog2", "FogVolume");
    let bid = tree.add_child(root, b).unwrap();

    tree.get_node_mut(aid)
        .unwrap()
        .set_property("shape", Variant::Int(0)); // Ellipsoid
    tree.get_node_mut(bid)
        .unwrap()
        .set_property("shape", Variant::Int(3)); // Box

    assert_eq!(
        tree.get_node(aid).unwrap().get_property("shape"),
        Variant::Int(0)
    );
    assert_eq!(
        tree.get_node(bid).unwrap().get_property("shape"),
        Variant::Int(3)
    );
}

// ===========================================================================
// 3. FogVolumeShape
// ===========================================================================

#[test]
fn shape_default_is_ellipsoid() {
    assert_eq!(FogVolumeShape::default(), FogVolumeShape::Ellipsoid);
}

#[test]
fn shape_godot_int_roundtrip_all() {
    let shapes = [
        (FogVolumeShape::Ellipsoid, 0),
        (FogVolumeShape::Cone, 1),
        (FogVolumeShape::Cylinder, 2),
        (FogVolumeShape::Box, 3),
        (FogVolumeShape::World, 4),
    ];
    for (shape, expected_int) in shapes {
        assert_eq!(shape.to_godot_int(), expected_int);
        assert_eq!(FogVolumeShape::from_godot_int(expected_int), shape);
    }
}

#[test]
fn shape_invalid_int_returns_ellipsoid() {
    assert_eq!(FogVolumeShape::from_godot_int(-1), FogVolumeShape::Ellipsoid);
    assert_eq!(FogVolumeShape::from_godot_int(99), FogVolumeShape::Ellipsoid);
}

// ===========================================================================
// 4. FogMaterial
// ===========================================================================

#[test]
fn material_default_density() {
    let m = FogMaterial::default();
    assert!(approx(m.density, 1.0));
}

#[test]
fn material_default_albedo_white() {
    let m = FogMaterial::default();
    assert_eq!(m.albedo, Color::WHITE);
}

#[test]
fn material_default_emission_black() {
    let m = FogMaterial::default();
    assert!(approx(m.emission.r, 0.0));
    assert!(approx(m.emission.g, 0.0));
    assert!(approx(m.emission.b, 0.0));
}

#[test]
fn material_default_no_height_falloff() {
    let m = FogMaterial::default();
    assert!(approx(m.height_falloff, 0.0));
}

#[test]
fn material_default_edge_fade() {
    let m = FogMaterial::default();
    assert!(approx(m.edge_fade, 0.1));
}

#[test]
fn material_no_density_texture() {
    let m = FogMaterial::default();
    assert!(m.density_texture.is_empty());
}

// ===========================================================================
// 5. FogVolume constructors
// ===========================================================================

#[test]
fn default_volume_ellipsoid_2x2x2() {
    let v = FogVolume::default();
    assert_eq!(v.shape, FogVolumeShape::Ellipsoid);
    assert!(approx(v.size.x, 2.0));
    assert!(approx(v.size.y, 2.0));
    assert!(approx(v.size.z, 2.0));
}

#[test]
fn box_constructor_stores_size() {
    let v = FogVolume::box_shape(Vector3::new(10.0, 5.0, 8.0));
    assert_eq!(v.shape, FogVolumeShape::Box);
    assert!(approx(v.size.x, 10.0));
    assert!(approx(v.size.y, 5.0));
    assert!(approx(v.size.z, 8.0));
}

#[test]
fn cylinder_constructor_diameter() {
    let v = FogVolume::cylinder(3.0, 6.0);
    assert_eq!(v.shape, FogVolumeShape::Cylinder);
    assert!(approx(v.size.x, 6.0)); // diameter
    assert!(approx(v.size.y, 6.0)); // height
}

#[test]
fn cone_constructor_diameter() {
    let v = FogVolume::cone(2.0, 8.0);
    assert_eq!(v.shape, FogVolumeShape::Cone);
    assert!(approx(v.size.x, 4.0)); // diameter
    assert!(approx(v.size.y, 8.0)); // height
}

#[test]
fn world_constructor_shape() {
    let v = FogVolume::world();
    assert_eq!(v.shape, FogVolumeShape::World);
}

// ===========================================================================
// 6. Point containment — Box
// ===========================================================================

#[test]
fn box_origin_inside() {
    let v = FogVolume::box_shape(Vector3::new(4.0, 4.0, 4.0));
    assert!(v.contains_point(Vector3::ZERO));
}

#[test]
fn box_corner_inside() {
    let v = FogVolume::box_shape(Vector3::new(4.0, 4.0, 4.0));
    assert!(v.contains_point(Vector3::new(1.9, 1.9, 1.9)));
}

#[test]
fn box_outside_x() {
    let v = FogVolume::box_shape(Vector3::new(4.0, 4.0, 4.0));
    assert!(!v.contains_point(Vector3::new(2.5, 0.0, 0.0)));
}

#[test]
fn box_outside_negative() {
    let v = FogVolume::box_shape(Vector3::new(2.0, 2.0, 2.0));
    assert!(!v.contains_point(Vector3::new(-2.0, 0.0, 0.0)));
}

// ===========================================================================
// 7. Point containment — Ellipsoid
// ===========================================================================

#[test]
fn ellipsoid_origin_inside() {
    let v = FogVolume::ellipsoid(Vector3::new(6.0, 4.0, 6.0));
    assert!(v.contains_point(Vector3::ZERO));
}

#[test]
fn ellipsoid_near_surface_inside() {
    let v = FogVolume::ellipsoid(Vector3::new(6.0, 6.0, 6.0));
    assert!(v.contains_point(Vector3::new(2.9, 0.0, 0.0)));
}

#[test]
fn ellipsoid_outside() {
    let v = FogVolume::ellipsoid(Vector3::new(2.0, 2.0, 2.0));
    assert!(!v.contains_point(Vector3::new(1.5, 0.0, 0.0)));
}

#[test]
fn ellipsoid_non_uniform_inside() {
    // Wide but short ellipsoid
    let v = FogVolume::ellipsoid(Vector3::new(10.0, 2.0, 10.0));
    assert!(v.contains_point(Vector3::new(4.0, 0.0, 0.0)));
    assert!(!v.contains_point(Vector3::new(0.0, 1.5, 0.0)));
}

// ===========================================================================
// 8. Point containment — Cylinder
// ===========================================================================

#[test]
fn cylinder_center_inside() {
    let v = FogVolume::cylinder(3.0, 6.0);
    assert!(v.contains_point(Vector3::ZERO));
}

#[test]
fn cylinder_near_edge_inside() {
    let v = FogVolume::cylinder(3.0, 6.0);
    assert!(v.contains_point(Vector3::new(2.9, 0.0, 0.0)));
}

#[test]
fn cylinder_outside_radius() {
    let v = FogVolume::cylinder(3.0, 6.0);
    assert!(!v.contains_point(Vector3::new(3.5, 0.0, 0.0)));
}

#[test]
fn cylinder_outside_height() {
    let v = FogVolume::cylinder(3.0, 6.0);
    assert!(!v.contains_point(Vector3::new(0.0, 4.0, 0.0)));
}

// ===========================================================================
// 9. Point containment — Cone
// ===========================================================================

#[test]
fn cone_bottom_wide_inside() {
    let v = FogVolume::cone(4.0, 10.0);
    // Bottom at y = -5, full radius = 4
    assert!(v.contains_point(Vector3::new(3.0, -4.5, 0.0)));
}

#[test]
fn cone_tip_narrow() {
    let v = FogVolume::cone(4.0, 10.0);
    // Near top (y = +5), radius approaches 0
    assert!(!v.contains_point(Vector3::new(1.0, 4.9, 0.0)));
}

#[test]
fn cone_center_inside() {
    let v = FogVolume::cone(4.0, 10.0);
    assert!(v.contains_point(Vector3::ZERO));
}

#[test]
fn cone_above_outside() {
    let v = FogVolume::cone(4.0, 10.0);
    assert!(!v.contains_point(Vector3::new(0.0, 6.0, 0.0)));
}

// ===========================================================================
// 10. Point containment — World
// ===========================================================================

#[test]
fn world_contains_origin() {
    let v = FogVolume::world();
    assert!(v.contains_point(Vector3::ZERO));
}

#[test]
fn world_contains_far_point() {
    let v = FogVolume::world();
    assert!(v.contains_point(Vector3::new(1e6, -1e6, 1e6)));
}

// ===========================================================================
// 11. Density sampling
// ===========================================================================

#[test]
fn density_zero_outside_volume() {
    let v = FogVolume::box_shape(Vector3::new(2.0, 2.0, 2.0));
    assert!(approx(v.sample_density(Vector3::new(5.0, 0.0, 0.0)), 0.0));
}

#[test]
fn density_returns_material_density_inside() {
    let mut v = FogVolume::box_shape(Vector3::new(4.0, 4.0, 4.0));
    v.material.density = 0.75;
    v.material.height_falloff = 0.0;
    v.material.edge_fade = 0.0;
    assert!(approx(v.sample_density(Vector3::ZERO), 0.75));
}

#[test]
fn density_height_falloff_bottom_denser() {
    let mut v = FogVolume::box_shape(Vector3::new(4.0, 10.0, 4.0));
    v.material.density = 1.0;
    v.material.height_falloff = 3.0;
    v.material.edge_fade = 0.0;
    let bottom = v.sample_density(Vector3::new(0.0, -4.0, 0.0));
    let top = v.sample_density(Vector3::new(0.0, 4.0, 0.0));
    assert!(
        bottom > top,
        "Height falloff: bottom ({bottom}) should be denser than top ({top})"
    );
}

#[test]
fn density_no_height_falloff_uniform() {
    let mut v = FogVolume::box_shape(Vector3::new(4.0, 10.0, 4.0));
    v.material.density = 1.0;
    v.material.height_falloff = 0.0;
    v.material.edge_fade = 0.0;
    let bottom = v.sample_density(Vector3::new(0.0, -4.0, 0.0));
    let top = v.sample_density(Vector3::new(0.0, 4.0, 0.0));
    assert!(approx(bottom, top), "No falloff should be uniform: {bottom} vs {top}");
}

#[test]
fn density_edge_fade_center_vs_edge() {
    let mut v = FogVolume::box_shape(Vector3::new(4.0, 4.0, 4.0));
    v.material.density = 1.0;
    v.material.height_falloff = 0.0;
    v.material.edge_fade = 0.5;
    let center = v.sample_density(Vector3::ZERO);
    let edge = v.sample_density(Vector3::new(1.9, 0.0, 0.0));
    assert!(
        edge < center,
        "Edge fade: edge ({edge}) should be less dense than center ({center})"
    );
}

#[test]
fn density_world_uniform_everywhere() {
    let mut v = FogVolume::world();
    v.material.density = 0.5;
    v.material.height_falloff = 0.0;
    assert!(approx(v.sample_density(Vector3::new(100.0, 200.0, 300.0)), 0.5));
    assert!(approx(v.sample_density(Vector3::new(-100.0, -200.0, -300.0)), 0.5));
}

#[test]
fn density_ellipsoid_center_vs_surface() {
    let mut v = FogVolume::ellipsoid(Vector3::new(6.0, 6.0, 6.0));
    v.material.density = 1.0;
    v.material.height_falloff = 0.0;
    v.material.edge_fade = 1.0;
    let center = v.sample_density(Vector3::ZERO);
    let near_surface = v.sample_density(Vector3::new(2.8, 0.0, 0.0));
    assert!(center > near_surface, "Center should be denser than near-surface");
}

// ===========================================================================
// 12. Scene tree with environment + fog volume
// ===========================================================================

#[test]
fn scene_with_environment_and_fog_volume() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let env = Node::new("Env", "WorldEnvironment");
    let env_id = tree.add_child(root, env).unwrap();

    let fog = Node::new("ValleyFog", "FogVolume");
    let fog_id = tree.add_child(root, fog).unwrap();

    assert_eq!(tree.get_node(env_id).unwrap().class_name(), "WorldEnvironment");
    assert_eq!(tree.get_node(fog_id).unwrap().class_name(), "FogVolume");
}

#[test]
fn multiple_fog_volumes_in_tree() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let fog1 = Node::new("GroundFog", "FogVolume");
    let fog2 = Node::new("MountainFog", "FogVolume");
    let fog3 = Node::new("CaveFog", "FogVolume");

    let id1 = tree.add_child(root, fog1).unwrap();
    let id2 = tree.add_child(root, fog2).unwrap();
    let id3 = tree.add_child(root, fog3).unwrap();

    assert_eq!(tree.get_node(id1).unwrap().name(), "GroundFog");
    assert_eq!(tree.get_node(id2).unwrap().name(), "MountainFog");
    assert_eq!(tree.get_node(id3).unwrap().name(), "CaveFog");
}

// ===========================================================================
// 13. Custom material configurations
// ===========================================================================

#[test]
fn high_density_fog() {
    let mut v = FogVolume::box_shape(Vector3::new(4.0, 4.0, 4.0));
    v.material.density = 5.0;
    v.material.height_falloff = 0.0;
    v.material.edge_fade = 0.0;
    assert!(approx(v.sample_density(Vector3::ZERO), 5.0));
}

#[test]
fn colored_fog() {
    let mut v = FogVolume::box_shape(Vector3::new(4.0, 4.0, 4.0));
    v.material.albedo = Color::new(0.2, 0.3, 0.8, 1.0);
    v.material.emission = Color::new(0.1, 0.0, 0.0, 1.0);
    assert!(approx(v.material.albedo.b, 0.8));
    assert!(approx(v.material.emission.r, 0.1));
}

#[test]
fn density_texture_path() {
    let mut m = FogMaterial::default();
    m.density_texture = "res://textures/fog_noise.tres".to_string();
    assert_eq!(m.density_texture, "res://textures/fog_noise.tres");
}
