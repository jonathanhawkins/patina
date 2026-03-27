//! pat-e3w9a: VoxelGI and LightmapGI stub nodes for global illumination prep.
//!
//! Integration tests covering:
//! 1. ClassDB registration (properties, inheritance, defaults)
//! 2. Scene tree integration (node creation, property get/set)
//! 3. VoxelGI struct defaults and geometry helpers
//! 4. LightmapGI struct defaults and configuration
//! 5. Enum value parity with Godot constants

use gdcore::math::Vector3;
use gdscene::node::Node;
use gdscene::node3d;
use gdscene::scene_tree::SceneTree;
use gdserver3d::gi::*;
use gdvariant::Variant;

const EPSILON: f32 = 1e-5;

fn approx(a: f32, b: f32) -> bool {
    (a - b).abs() < EPSILON
}

fn make_tree() -> SceneTree {
    SceneTree::new()
}

// ===========================================================================
// 1. ClassDB registration — VoxelGI
// ===========================================================================

#[test]
fn classdb_registers_voxelgi() {
    gdobject::class_db::register_3d_classes();
    assert!(gdobject::class_db::class_exists("VoxelGI"));
}

#[test]
fn classdb_voxelgi_inherits_node3d() {
    gdobject::class_db::register_3d_classes();
    let info = gdobject::class_db::get_class_info("VoxelGI").unwrap();
    assert_eq!(info.parent_class.as_str(), "Node3D");
}

#[test]
fn classdb_voxelgi_has_expected_properties() {
    gdobject::class_db::register_3d_classes();
    let props = gdobject::class_db::get_property_list("VoxelGI", false);
    let names: Vec<&str> = props.iter().map(|p| p.name.as_str()).collect();
    for expected in &["size", "subdiv", "energy", "bias", "normal_bias", "propagation", "interior"] {
        assert!(names.contains(expected), "Missing '{}' property", expected);
    }
}

#[test]
fn classdb_voxelgi_default_size() {
    gdobject::class_db::register_3d_classes();
    let props = gdobject::class_db::get_property_list("VoxelGI", false);
    let size = props.iter().find(|p| p.name == "size").unwrap();
    assert_eq!(
        size.default_value,
        Variant::Vector3(Vector3::new(20.0, 20.0, 20.0))
    );
}

#[test]
fn classdb_voxelgi_default_subdiv() {
    gdobject::class_db::register_3d_classes();
    let props = gdobject::class_db::get_property_list("VoxelGI", false);
    let subdiv = props.iter().find(|p| p.name == "subdiv").unwrap();
    assert_eq!(subdiv.default_value, Variant::Int(1)); // Subdiv128
}

#[test]
fn classdb_voxelgi_default_energy() {
    gdobject::class_db::register_3d_classes();
    let props = gdobject::class_db::get_property_list("VoxelGI", false);
    let energy = props.iter().find(|p| p.name == "energy").unwrap();
    assert_eq!(energy.default_value, Variant::Float(1.0));
}

#[test]
fn classdb_voxelgi_default_bias() {
    gdobject::class_db::register_3d_classes();
    let props = gdobject::class_db::get_property_list("VoxelGI", false);
    let bias = props.iter().find(|p| p.name == "bias").unwrap();
    assert_eq!(bias.default_value, Variant::Float(1.5));
}

#[test]
fn classdb_voxelgi_default_propagation() {
    gdobject::class_db::register_3d_classes();
    let props = gdobject::class_db::get_property_list("VoxelGI", false);
    let prop = props.iter().find(|p| p.name == "propagation").unwrap();
    assert_eq!(prop.default_value, Variant::Float(0.7));
}

// ===========================================================================
// 2. ClassDB registration — LightmapGI
// ===========================================================================

#[test]
fn classdb_registers_lightmapgi() {
    gdobject::class_db::register_3d_classes();
    assert!(gdobject::class_db::class_exists("LightmapGI"));
}

#[test]
fn classdb_lightmapgi_inherits_node3d() {
    gdobject::class_db::register_3d_classes();
    let info = gdobject::class_db::get_class_info("LightmapGI").unwrap();
    assert_eq!(info.parent_class.as_str(), "Node3D");
}

#[test]
fn classdb_lightmapgi_has_expected_properties() {
    gdobject::class_db::register_3d_classes();
    let props = gdobject::class_db::get_property_list("LightmapGI", false);
    let names: Vec<&str> = props.iter().map(|p| p.name.as_str()).collect();
    for expected in &[
        "quality", "bounces", "texel_scale", "use_denoiser", "denoiser_strength",
        "directional", "interior", "energy", "bias", "max_texture_size", "generate_probes",
    ] {
        assert!(names.contains(expected), "Missing '{}' property", expected);
    }
}

#[test]
fn classdb_lightmapgi_default_quality() {
    gdobject::class_db::register_3d_classes();
    let props = gdobject::class_db::get_property_list("LightmapGI", false);
    let quality = props.iter().find(|p| p.name == "quality").unwrap();
    assert_eq!(quality.default_value, Variant::Int(1)); // Medium
}

#[test]
fn classdb_lightmapgi_default_bounces() {
    gdobject::class_db::register_3d_classes();
    let props = gdobject::class_db::get_property_list("LightmapGI", false);
    let bounces = props.iter().find(|p| p.name == "bounces").unwrap();
    assert_eq!(bounces.default_value, Variant::Int(3));
}

#[test]
fn classdb_lightmapgi_default_max_texture_size() {
    gdobject::class_db::register_3d_classes();
    let props = gdobject::class_db::get_property_list("LightmapGI", false);
    let prop = props.iter().find(|p| p.name == "max_texture_size").unwrap();
    assert_eq!(prop.default_value, Variant::Int(16384));
}

#[test]
fn classdb_lightmapgi_default_use_denoiser() {
    gdobject::class_db::register_3d_classes();
    let props = gdobject::class_db::get_property_list("LightmapGI", false);
    let prop = props.iter().find(|p| p.name == "use_denoiser").unwrap();
    assert_eq!(prop.default_value, Variant::Bool(true));
}

// ===========================================================================
// 3. Scene tree integration — VoxelGI
// ===========================================================================

#[test]
fn scene_tree_voxelgi_subdiv_get_set() {
    let mut tree = make_tree();
    let root = tree.root_id();
    let node = Node::new("VGI", "VoxelGI");
    let id = tree.add_child(root, node).unwrap();

    assert_eq!(node3d::get_voxelgi_subdiv(&tree, id), 1); // default Subdiv128
    node3d::set_voxelgi_subdiv(&mut tree, id, 3);
    assert_eq!(node3d::get_voxelgi_subdiv(&tree, id), 3); // Subdiv512
}

#[test]
fn scene_tree_voxelgi_size_get_set() {
    let mut tree = make_tree();
    let root = tree.root_id();
    let node = Node::new("VGI", "VoxelGI");
    let id = tree.add_child(root, node).unwrap();

    assert_eq!(node3d::get_voxelgi_size(&tree, id), Vector3::new(20.0, 20.0, 20.0));
    node3d::set_voxelgi_size(&mut tree, id, Vector3::new(50.0, 30.0, 50.0));
    assert_eq!(node3d::get_voxelgi_size(&tree, id), Vector3::new(50.0, 30.0, 50.0));
}

// ===========================================================================
// 4. Scene tree integration — LightmapGI
// ===========================================================================

#[test]
fn scene_tree_lightmapgi_quality_get_set() {
    let mut tree = make_tree();
    let root = tree.root_id();
    let node = Node::new("LGI", "LightmapGI");
    let id = tree.add_child(root, node).unwrap();

    assert_eq!(node3d::get_lightmapgi_quality(&tree, id), 1); // Medium
    node3d::set_lightmapgi_quality(&mut tree, id, 3);
    assert_eq!(node3d::get_lightmapgi_quality(&tree, id), 3); // Ultra
}

#[test]
fn scene_tree_lightmapgi_bounces_get_set() {
    let mut tree = make_tree();
    let root = tree.root_id();
    let node = Node::new("LGI", "LightmapGI");
    let id = tree.add_child(root, node).unwrap();

    assert_eq!(node3d::get_lightmapgi_bounces(&tree, id), 3);
    node3d::set_lightmapgi_bounces(&mut tree, id, 5);
    assert_eq!(node3d::get_lightmapgi_bounces(&tree, id), 5);
}

#[test]
fn scene_tree_lightmapgi_data_path_get_set() {
    let mut tree = make_tree();
    let root = tree.root_id();
    let node = Node::new("LGI", "LightmapGI");
    let id = tree.add_child(root, node).unwrap();

    assert!(node3d::get_lightmapgi_data_path(&tree, id).is_none());
    node3d::set_lightmapgi_data_path(&mut tree, id, "res://lightmap.lmbake");
    assert_eq!(
        node3d::get_lightmapgi_data_path(&tree, id).as_deref(),
        Some("res://lightmap.lmbake")
    );
}

// ===========================================================================
// 5. VoxelGI struct — defaults and geometry
// ===========================================================================

#[test]
fn voxelgi_new_defaults_match_godot() {
    let gi = VoxelGI::new(VoxelGIId(1));
    assert_eq!(gi.size, Vector3::new(20.0, 20.0, 20.0));
    assert_eq!(gi.subdiv, VoxelGISubdiv::Subdiv128);
    assert!(approx(gi.energy, 1.0));
    assert!(approx(gi.bias, 1.5));
    assert!(approx(gi.normal_bias, 0.0));
    assert!(approx(gi.propagation, 0.7));
    assert!(!gi.interior);
    assert!(!gi.baked);
    assert!(gi.camera_attributes_path.is_none());
}

#[test]
fn voxelgi_world_aabb_at_origin() {
    let gi = VoxelGI::new(VoxelGIId(1));
    let (min, max) = gi.world_aabb_min_max();
    assert!(approx(min.x, -10.0));
    assert!(approx(min.y, -10.0));
    assert!(approx(min.z, -10.0));
    assert!(approx(max.x, 10.0));
    assert!(approx(max.y, 10.0));
    assert!(approx(max.z, 10.0));
}

#[test]
fn voxelgi_world_aabb_with_offset() {
    let mut gi = VoxelGI::new(VoxelGIId(1));
    gi.transform.origin = Vector3::new(50.0, 20.0, 0.0);
    let (min, max) = gi.world_aabb_min_max();
    assert!(approx(min.x, 40.0));
    assert!(approx(max.x, 60.0));
    assert!(approx(min.y, 10.0));
    assert!(approx(max.y, 30.0));
}

#[test]
fn voxelgi_contains_point_inside() {
    let gi = VoxelGI::new(VoxelGIId(1));
    assert!(gi.contains_point(Vector3::ZERO));
    assert!(gi.contains_point(Vector3::new(5.0, 5.0, 5.0)));
}

#[test]
fn voxelgi_contains_point_outside() {
    let gi = VoxelGI::new(VoxelGIId(1));
    assert!(!gi.contains_point(Vector3::new(11.0, 0.0, 0.0)));
    assert!(!gi.contains_point(Vector3::new(0.0, -11.0, 0.0)));
}

#[test]
fn voxelgi_contains_point_on_boundary() {
    let gi = VoxelGI::new(VoxelGIId(1));
    assert!(gi.contains_point(Vector3::new(10.0, 10.0, 10.0)));
    assert!(gi.contains_point(Vector3::new(-10.0, -10.0, -10.0)));
}

#[test]
fn voxelgi_grid_resolution_all_subdivs() {
    let mut gi = VoxelGI::new(VoxelGIId(1));
    assert_eq!(gi.grid_resolution(), 128); // default Subdiv128

    gi.subdiv = VoxelGISubdiv::Subdiv64;
    assert_eq!(gi.grid_resolution(), 64);

    gi.subdiv = VoxelGISubdiv::Subdiv256;
    assert_eq!(gi.grid_resolution(), 256);

    gi.subdiv = VoxelGISubdiv::Subdiv512;
    assert_eq!(gi.grid_resolution(), 512);
}

#[test]
fn voxelgi_custom_size_affects_aabb() {
    let mut gi = VoxelGI::new(VoxelGIId(1));
    gi.size = Vector3::new(40.0, 10.0, 60.0);
    let (min, max) = gi.world_aabb_min_max();
    assert!(approx(min.x, -20.0));
    assert!(approx(max.x, 20.0));
    assert!(approx(min.y, -5.0));
    assert!(approx(max.y, 5.0));
    assert!(approx(min.z, -30.0));
    assert!(approx(max.z, 30.0));
}

// ===========================================================================
// 6. VoxelGI enum parity
// ===========================================================================

#[test]
fn voxelgi_subdiv_enum_values_match_godot() {
    assert_eq!(VoxelGISubdiv::Subdiv64 as u32, 0);
    assert_eq!(VoxelGISubdiv::Subdiv128 as u32, 1);
    assert_eq!(VoxelGISubdiv::Subdiv256 as u32, 2);
    assert_eq!(VoxelGISubdiv::Subdiv512 as u32, 3);
}

#[test]
fn voxelgi_subdiv_default_is_128() {
    assert_eq!(VoxelGISubdiv::default(), VoxelGISubdiv::Subdiv128);
}

// ===========================================================================
// 7. LightmapGI struct — defaults and configuration
// ===========================================================================

#[test]
fn lightmapgi_new_defaults_match_godot() {
    let gi = LightmapGI::new(LightmapGIId(1));
    assert_eq!(gi.quality, LightmapBakeQuality::Medium);
    assert_eq!(gi.bounces, 3);
    assert!(approx(gi.texel_scale, 1.0));
    assert!(gi.use_denoiser);
    assert!(approx(gi.denoiser_strength, 0.1));
    assert!(!gi.directional);
    assert!(!gi.interior);
    assert!(approx(gi.energy, 1.0));
    assert!(approx(gi.bias, 0.0005));
    assert_eq!(gi.max_texture_size, 16384);
    assert_eq!(gi.generate_probes, LightmapProbeGeneration::Disabled);
    assert!(gi.camera_attributes_path.is_none());
    assert!(!gi.baked);
    assert!(gi.light_data_path.is_none());
}

#[test]
fn lightmapgi_set_baked_with_data_path() {
    let mut gi = LightmapGI::new(LightmapGIId(1));
    gi.light_data_path = Some("res://lightmap_data.lmbake".to_string());
    gi.baked = true;
    assert!(gi.baked);
    assert_eq!(gi.light_data_path.as_deref(), Some("res://lightmap_data.lmbake"));
}

#[test]
fn lightmapgi_interior_mode() {
    let mut gi = LightmapGI::new(LightmapGIId(1));
    assert!(!gi.interior);
    gi.interior = true;
    assert!(gi.interior);
}

#[test]
fn lightmapgi_configure_high_quality() {
    let mut gi = LightmapGI::new(LightmapGIId(1));
    gi.quality = LightmapBakeQuality::Ultra;
    gi.bounces = 6;
    gi.texel_scale = 2.0;
    gi.directional = true;
    assert_eq!(gi.quality, LightmapBakeQuality::Ultra);
    assert_eq!(gi.bounces, 6);
    assert!(approx(gi.texel_scale, 2.0));
    assert!(gi.directional);
}

// ===========================================================================
// 8. LightmapGI enum parity
// ===========================================================================

#[test]
fn lightmapgi_bake_quality_values_match_godot() {
    assert_eq!(LightmapBakeQuality::Low as u32, 0);
    assert_eq!(LightmapBakeQuality::Medium as u32, 1);
    assert_eq!(LightmapBakeQuality::High as u32, 2);
    assert_eq!(LightmapBakeQuality::Ultra as u32, 3);
}

#[test]
fn lightmapgi_bake_quality_default_is_medium() {
    assert_eq!(LightmapBakeQuality::default(), LightmapBakeQuality::Medium);
}

#[test]
fn lightmapgi_probe_generation_values_match_godot() {
    assert_eq!(LightmapProbeGeneration::Disabled as u32, 0);
    assert_eq!(LightmapProbeGeneration::Low as u32, 1);
    assert_eq!(LightmapProbeGeneration::Medium as u32, 2);
    assert_eq!(LightmapProbeGeneration::High as u32, 3);
}

#[test]
fn lightmapgi_probe_generation_default_is_disabled() {
    assert_eq!(LightmapProbeGeneration::default(), LightmapProbeGeneration::Disabled);
}

// ===========================================================================
// 9. ID equality and hashing
// ===========================================================================

#[test]
fn voxelgi_id_equality() {
    assert_eq!(VoxelGIId(42), VoxelGIId(42));
    assert_ne!(VoxelGIId(1), VoxelGIId(2));
}

#[test]
fn lightmapgi_id_equality() {
    assert_eq!(LightmapGIId(42), LightmapGIId(42));
    assert_ne!(LightmapGIId(1), LightmapGIId(2));
}

#[test]
fn voxelgi_id_usable_as_hash_key() {
    use std::collections::HashMap;
    let mut map = HashMap::new();
    map.insert(VoxelGIId(1), "probe_a");
    map.insert(VoxelGIId(2), "probe_b");
    assert_eq!(map.get(&VoxelGIId(1)), Some(&"probe_a"));
    assert_eq!(map.get(&VoxelGIId(2)), Some(&"probe_b"));
    assert_eq!(map.get(&VoxelGIId(3)), None);
}

// ===========================================================================
// 10. Multiple GI nodes in scene tree
// ===========================================================================

#[test]
fn multiple_voxelgi_nodes_independent() {
    let mut tree = make_tree();
    let root = tree.root_id();

    let v1 = tree.add_child(root, Node::new("VGI1", "VoxelGI")).unwrap();
    let v2 = tree.add_child(root, Node::new("VGI2", "VoxelGI")).unwrap();

    node3d::set_voxelgi_subdiv(&mut tree, v1, 0); // Subdiv64
    node3d::set_voxelgi_subdiv(&mut tree, v2, 3); // Subdiv512

    assert_eq!(node3d::get_voxelgi_subdiv(&tree, v1), 0);
    assert_eq!(node3d::get_voxelgi_subdiv(&tree, v2), 3);
}

#[test]
fn mixed_gi_nodes_in_scene() {
    let mut tree = make_tree();
    let root = tree.root_id();

    let vgi = tree.add_child(root, Node::new("VGI", "VoxelGI")).unwrap();
    let lgi = tree.add_child(root, Node::new("LGI", "LightmapGI")).unwrap();

    node3d::set_voxelgi_subdiv(&mut tree, vgi, 2);
    node3d::set_lightmapgi_quality(&mut tree, lgi, 2); // High

    assert_eq!(node3d::get_voxelgi_subdiv(&tree, vgi), 2);
    assert_eq!(node3d::get_lightmapgi_quality(&tree, lgi), 2);
}
