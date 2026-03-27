//! pat-1942m: OmniLight3D with point shadow cubemap — integration tests.
//!
//! Validates the full OmniLight3D pipeline:
//! - Light3D construction (point type with position, range, attenuation)
//! - OmniShadowMode variants (DualParaboloid, Cube)
//! - ShadowCubemap creation, depth writing, and directional sampling
//! - CubeFace direction/up vectors and face selection
//! - ClassDB registration with correct default properties
//! - Scene tree property sync for OmniLight3D nodes

use gdcore::math::Vector3;
use gdscene::Node;
use gdserver3d::light::{
    CubeFace, Light3D, Light3DId, LightType, OmniShadowMode, ShadowCubemap,
};

// ============================================================================
// 1. Light3D point light construction
// ============================================================================

#[test]
fn point_light_has_correct_type() {
    let light = Light3D::point(Light3DId(1), Vector3::new(0.0, 3.0, 0.0));
    assert_eq!(light.light_type, LightType::Point);
}

#[test]
fn point_light_position_stored() {
    let pos = Vector3::new(2.0, 5.0, -3.0);
    let light = Light3D::point(Light3DId(1), pos);
    assert!((light.position.x - 2.0).abs() < 1e-6);
    assert!((light.position.y - 5.0).abs() < 1e-6);
    assert!((light.position.z - (-3.0)).abs() < 1e-6);
}

#[test]
fn point_light_default_energy_is_one() {
    let light = Light3D::point(Light3DId(1), Vector3::ZERO);
    assert!((light.energy - 1.0).abs() < 1e-6);
}

#[test]
fn point_light_default_color_is_white() {
    let light = Light3D::point(Light3DId(1), Vector3::ZERO);
    assert!((light.color.r - 1.0).abs() < 1e-6);
    assert!((light.color.g - 1.0).abs() < 1e-6);
    assert!((light.color.b - 1.0).abs() < 1e-6);
}

#[test]
fn point_light_default_range_is_positive() {
    let light = Light3D::point(Light3DId(1), Vector3::ZERO);
    assert!(light.range > 0.0, "default range should be positive");
}

#[test]
fn point_light_default_attenuation_is_positive() {
    let light = Light3D::point(Light3DId(1), Vector3::ZERO);
    assert!(light.attenuation > 0.0, "default attenuation should be positive");
}

#[test]
fn point_light_energy_can_be_modified() {
    let mut light = Light3D::point(Light3DId(1), Vector3::ZERO);
    light.energy = 2.5;
    assert!((light.energy - 2.5).abs() < 1e-6);
}

// ============================================================================
// 2. OmniShadowMode
// ============================================================================

#[test]
fn omni_shadow_mode_default_is_dual_paraboloid() {
    assert_eq!(OmniShadowMode::default(), OmniShadowMode::DualParaboloid);
}

#[test]
fn omni_shadow_mode_cube_variant_exists() {
    let mode = OmniShadowMode::Cube;
    assert_eq!(mode, OmniShadowMode::Cube);
    assert_ne!(mode, OmniShadowMode::DualParaboloid);
}

#[test]
fn point_light_default_omni_shadow_mode_is_dual_paraboloid() {
    let light = Light3D::point(Light3DId(1), Vector3::ZERO);
    assert_eq!(light.omni_shadow_mode, OmniShadowMode::DualParaboloid);
}

#[test]
fn point_light_omni_shadow_mode_can_be_set_to_cube() {
    let mut light = Light3D::point(Light3DId(1), Vector3::ZERO);
    light.omni_shadow_mode = OmniShadowMode::Cube;
    assert_eq!(light.omni_shadow_mode, OmniShadowMode::Cube);
}

#[test]
fn point_light_shadow_enabled_default_false() {
    let light = Light3D::point(Light3DId(1), Vector3::ZERO);
    assert!(!light.shadow_enabled, "shadow should be disabled by default");
}

#[test]
fn point_light_shadow_can_be_enabled() {
    let mut light = Light3D::point(Light3DId(1), Vector3::ZERO);
    light.shadow_enabled = true;
    assert!(light.shadow_enabled);
}

// ============================================================================
// 3. CubeFace directions and up vectors
// ============================================================================

#[test]
fn cube_face_all_has_six_faces() {
    assert_eq!(CubeFace::ALL.len(), 6);
}

#[test]
fn cube_face_forward_vectors_are_unit_length() {
    for face in CubeFace::ALL {
        let fwd = face.forward();
        let len = fwd.length();
        assert!(
            (len - 1.0).abs() < 1e-6,
            "face {:?} forward length: {len}",
            face
        );
    }
}

#[test]
fn cube_face_up_vectors_are_unit_length() {
    for face in CubeFace::ALL {
        let up = face.up();
        let len = up.length();
        assert!(
            (len - 1.0).abs() < 1e-6,
            "face {:?} up length: {len}",
            face
        );
    }
}

#[test]
fn cube_face_forward_and_up_are_orthogonal() {
    for face in CubeFace::ALL {
        let fwd = face.forward();
        let up = face.up();
        let dot = fwd.x * up.x + fwd.y * up.y + fwd.z * up.z;
        assert!(
            dot.abs() < 1e-6,
            "face {:?}: forward·up = {dot} (should be 0)",
            face
        );
    }
}

#[test]
fn cube_face_positive_x_direction() {
    let fwd = CubeFace::PositiveX.forward();
    assert!((fwd.x - 1.0).abs() < 1e-6);
    assert!(fwd.y.abs() < 1e-6);
    assert!(fwd.z.abs() < 1e-6);
}

#[test]
fn cube_face_negative_z_direction() {
    let fwd = CubeFace::NegativeZ.forward();
    assert!(fwd.x.abs() < 1e-6);
    assert!(fwd.y.abs() < 1e-6);
    assert!((fwd.z - (-1.0)).abs() < 1e-6);
}

#[test]
fn cube_faces_cover_all_axes() {
    let forwards: Vec<Vector3> = CubeFace::ALL.iter().map(|f| f.forward()).collect();
    assert!(forwards.iter().any(|v| v.x > 0.5), "missing +X");
    assert!(forwards.iter().any(|v| v.x < -0.5), "missing -X");
    assert!(forwards.iter().any(|v| v.y > 0.5), "missing +Y");
    assert!(forwards.iter().any(|v| v.y < -0.5), "missing -Y");
    assert!(forwards.iter().any(|v| v.z > 0.5), "missing +Z");
    assert!(forwards.iter().any(|v| v.z < -0.5), "missing -Z");
}

// ============================================================================
// 4. ShadowCubemap creation and depth operations
// ============================================================================

#[test]
fn shadow_cubemap_new_has_correct_resolution() {
    let cm = ShadowCubemap::new(128);
    assert_eq!(cm.resolution, 128);
}

#[test]
fn shadow_cubemap_new_initialized_to_max_depth() {
    let cm = ShadowCubemap::new(16);
    for face in CubeFace::ALL {
        for x in 0..16 {
            for y in 0..16 {
                assert_eq!(cm.get_depth(face, x, y), f32::MAX);
            }
        }
    }
}

#[test]
fn shadow_cubemap_has_six_faces() {
    let cm = ShadowCubemap::new(16);
    assert_eq!(cm.faces.len(), 6);
}

#[test]
fn shadow_cubemap_face_size_matches_resolution() {
    let cm = ShadowCubemap::new(32);
    for face in &cm.faces {
        assert_eq!(face.len(), 32 * 32);
    }
}

#[test]
fn shadow_cubemap_test_and_set_writes_closer_depth() {
    let mut cm = ShadowCubemap::new(16);
    let written = cm.test_and_set(CubeFace::PositiveX, 5, 5, 10.0);
    assert!(written, "should write to empty (MAX) texel");
    assert!((cm.get_depth(CubeFace::PositiveX, 5, 5) - 10.0).abs() < 1e-6);
}

#[test]
fn shadow_cubemap_test_and_set_rejects_farther_depth() {
    let mut cm = ShadowCubemap::new(16);
    cm.test_and_set(CubeFace::PositiveX, 5, 5, 10.0);
    let written = cm.test_and_set(CubeFace::PositiveX, 5, 5, 20.0);
    assert!(!written, "should not overwrite closer depth");
    assert!((cm.get_depth(CubeFace::PositiveX, 5, 5) - 10.0).abs() < 1e-6);
}

#[test]
fn shadow_cubemap_test_and_set_overwrites_farther_with_closer() {
    let mut cm = ShadowCubemap::new(16);
    cm.test_and_set(CubeFace::PositiveX, 5, 5, 10.0);
    let written = cm.test_and_set(CubeFace::PositiveX, 5, 5, 5.0);
    assert!(written, "should overwrite farther depth with closer");
    assert!((cm.get_depth(CubeFace::PositiveX, 5, 5) - 5.0).abs() < 1e-6);
}

#[test]
fn shadow_cubemap_faces_are_independent() {
    let mut cm = ShadowCubemap::new(16);
    cm.test_and_set(CubeFace::PositiveX, 0, 0, 5.0);
    assert_eq!(cm.get_depth(CubeFace::NegativeX, 0, 0), f32::MAX);
    assert_eq!(cm.get_depth(CubeFace::PositiveY, 0, 0), f32::MAX);
    assert_eq!(cm.get_depth(CubeFace::NegativeZ, 0, 0), f32::MAX);
}

#[test]
fn shadow_cubemap_clear_resets_all_faces() {
    let mut cm = ShadowCubemap::new(16);
    cm.test_and_set(CubeFace::PositiveX, 0, 0, 5.0);
    cm.test_and_set(CubeFace::NegativeZ, 8, 8, 3.0);
    cm.clear();
    assert_eq!(cm.get_depth(CubeFace::PositiveX, 0, 0), f32::MAX);
    assert_eq!(cm.get_depth(CubeFace::NegativeZ, 8, 8), f32::MAX);
}

#[test]
fn shadow_cubemap_out_of_bounds_returns_max() {
    let cm = ShadowCubemap::new(16);
    assert_eq!(cm.get_depth(CubeFace::PositiveX, 16, 0), f32::MAX);
    assert_eq!(cm.get_depth(CubeFace::PositiveX, 0, 16), f32::MAX);
    assert_eq!(cm.get_depth(CubeFace::PositiveX, 100, 100), f32::MAX);
}

// ============================================================================
// 5. ShadowCubemap directional sampling
// ============================================================================

#[test]
fn shadow_cubemap_sample_positive_x() {
    let mut cm = ShadowCubemap::new(16);
    cm.test_and_set(CubeFace::PositiveX, 8, 8, 5.0);
    let depth = cm.sample(Vector3::new(1.0, 0.0, 0.0));
    assert!(depth < f32::MAX, "should find written depth on +X face");
}

#[test]
fn shadow_cubemap_sample_negative_z() {
    let mut cm = ShadowCubemap::new(16);
    cm.test_and_set(CubeFace::NegativeZ, 8, 8, 7.0);
    let depth = cm.sample(Vector3::new(0.0, 0.0, -1.0));
    assert!(depth < f32::MAX, "should find written depth on -Z face");
}

#[test]
fn shadow_cubemap_sample_empty_returns_max() {
    let cm = ShadowCubemap::new(16);
    let depth = cm.sample(Vector3::new(1.0, 0.0, 0.0));
    assert_eq!(depth, f32::MAX, "empty cubemap should return MAX depth");
}

#[test]
fn shadow_cubemap_sample_zero_direction_does_not_panic() {
    let cm = ShadowCubemap::new(16);
    let _depth = cm.sample(Vector3::ZERO);
}

// ============================================================================
// 6. ClassDB OmniLight3D registration
// ============================================================================

#[test]
fn classdb_has_omnilight3d() {
    gdobject::class_db::register_3d_classes();
    assert!(
        gdobject::class_db::class_exists("OmniLight3D"),
        "ClassDB must register OmniLight3D"
    );
}

#[test]
fn classdb_omnilight3d_has_light_energy() {
    gdobject::class_db::register_3d_classes();
    let props = gdobject::class_db::get_property_list("OmniLight3D", false);
    let prop_names: Vec<&str> = props.iter().map(|p| p.name.as_str()).collect();
    assert!(
        prop_names.contains(&"light_energy"),
        "OmniLight3D must have light_energy property"
    );
}

#[test]
fn classdb_omnilight3d_has_light_color() {
    gdobject::class_db::register_3d_classes();
    let props = gdobject::class_db::get_property_list("OmniLight3D", false);
    let prop_names: Vec<&str> = props.iter().map(|p| p.name.as_str()).collect();
    assert!(
        prop_names.contains(&"light_color"),
        "OmniLight3D must have light_color property"
    );
}

#[test]
fn classdb_omnilight3d_has_omni_range() {
    gdobject::class_db::register_3d_classes();
    let props = gdobject::class_db::get_property_list("OmniLight3D", false);
    let prop_names: Vec<&str> = props.iter().map(|p| p.name.as_str()).collect();
    assert!(
        prop_names.contains(&"omni_range"),
        "OmniLight3D must have omni_range property"
    );
}

#[test]
fn classdb_omnilight3d_has_omni_attenuation() {
    gdobject::class_db::register_3d_classes();
    let props = gdobject::class_db::get_property_list("OmniLight3D", false);
    let prop_names: Vec<&str> = props.iter().map(|p| p.name.as_str()).collect();
    assert!(
        prop_names.contains(&"omni_attenuation"),
        "OmniLight3D must have omni_attenuation property"
    );
}

#[test]
fn classdb_omnilight3d_has_omni_shadow_mode() {
    gdobject::class_db::register_3d_classes();
    let props = gdobject::class_db::get_property_list("OmniLight3D", false);
    let prop_names: Vec<&str> = props.iter().map(|p| p.name.as_str()).collect();
    assert!(
        prop_names.contains(&"omni_shadow_mode"),
        "OmniLight3D must have omni_shadow_mode property"
    );
}

#[test]
fn classdb_omnilight3d_has_shadow_enabled() {
    gdobject::class_db::register_3d_classes();
    let props = gdobject::class_db::get_property_list("OmniLight3D", false);
    let prop_names: Vec<&str> = props.iter().map(|p| p.name.as_str()).collect();
    assert!(
        prop_names.contains(&"shadow_enabled"),
        "OmniLight3D must have shadow_enabled property"
    );
}

#[test]
fn classdb_omnilight3d_default_range_is_5() {
    gdobject::class_db::register_3d_classes();
    let props = gdobject::class_db::get_property_list("OmniLight3D", false);
    if let Some(prop) = props.iter().find(|p| p.name == "omni_range") {
        match &prop.default_value {
            gdvariant::Variant::Float(f) => {
                assert!((f - 5.0).abs() < 1e-6, "omni_range default should be 5.0, got {f}");
            }
            gdvariant::Variant::Int(i) => {
                assert_eq!(*i, 5, "omni_range default should be 5");
            }
            _ => panic!("omni_range should be numeric, got {:?}", prop.default_value),
        }
    }
}

#[test]
fn classdb_omnilight3d_default_shadow_mode_is_0() {
    gdobject::class_db::register_3d_classes();
    let props = gdobject::class_db::get_property_list("OmniLight3D", false);
    if let Some(prop) = props.iter().find(|p| p.name == "omni_shadow_mode") {
        match &prop.default_value {
            gdvariant::Variant::Int(i) => {
                assert_eq!(*i, 0, "omni_shadow_mode default should be 0 (DualParaboloid)");
            }
            gdvariant::Variant::Float(f) => {
                assert!(f.abs() < 1e-6, "omni_shadow_mode default should be 0, got {f}");
            }
            _ => {}
        }
    }
}

// ============================================================================
// 7. Scene tree OmniLight3D property helpers
// ============================================================================

#[test]
fn scene_tree_omnilight3d_node_can_be_created() {
    let mut tree = gdscene::SceneTree::new();
    let root = tree.root_id();
    let node = Node::new("TestOmni", "OmniLight3D");
    let light_id = tree.add_child(root, node);
    assert!(light_id.is_ok(), "should create OmniLight3D node");
}

#[test]
fn scene_tree_omnilight3d_default_shadow_mode_property() {
    let mut tree = gdscene::SceneTree::new();
    let root = tree.root_id();
    let node = Node::new("TestOmni", "OmniLight3D");
    let nid = tree.add_child(root, node).unwrap();
    let mode = gdscene::node3d::get_omni_shadow_mode(&tree, nid);
    assert_eq!(mode, 0, "default omni_shadow_mode should be 0 (DualParaboloid)");
}

#[test]
fn scene_tree_omnilight3d_set_shadow_mode_to_cube() {
    let mut tree = gdscene::SceneTree::new();
    let root = tree.root_id();
    let node = Node::new("TestOmni", "OmniLight3D");
    let nid = tree.add_child(root, node).unwrap();
    gdscene::node3d::set_omni_shadow_mode(&mut tree, nid, 1);
    let mode = gdscene::node3d::get_omni_shadow_mode(&tree, nid);
    assert_eq!(mode, 1, "should be 1 (Cube) after set");
}

#[test]
fn scene_tree_omnilight3d_energy_property() {
    let mut tree = gdscene::SceneTree::new();
    let root = tree.root_id();
    let node = Node::new("TestOmni", "OmniLight3D");
    let nid = tree.add_child(root, node).unwrap();
    gdscene::node3d::set_light_energy(&mut tree, nid, 2.5);
    let energy = gdscene::node3d::get_light_energy(&tree, nid);
    assert!((energy - 2.5).abs() < 1e-6);
}

#[test]
fn scene_tree_omnilight3d_shadow_enabled_property() {
    let mut tree = gdscene::SceneTree::new();
    let root = tree.root_id();
    let node = Node::new("TestOmni", "OmniLight3D");
    let nid = tree.add_child(root, node).unwrap();
    gdscene::node3d::set_shadow_enabled(&mut tree, nid, true);
    let n = tree.get_node(nid).unwrap();
    let shadow = match n.get_property("shadow_enabled") {
        gdvariant::Variant::Bool(b) => b,
        _ => false,
    };
    assert!(shadow, "shadow_enabled should be true after set");
}

// ============================================================================
// 8. Large cubemap resolution
// ============================================================================

#[test]
fn shadow_cubemap_large_resolution_allocates_correctly() {
    let cm = ShadowCubemap::new(256);
    assert_eq!(cm.resolution, 256);
    for face in &cm.faces {
        assert_eq!(face.len(), 256 * 256);
    }
}

// ============================================================================
// 9. Light type distinctions
// ============================================================================

#[test]
fn light_type_point_is_distinct_from_directional_and_spot() {
    assert_ne!(LightType::Point, LightType::Directional);
    assert_ne!(LightType::Point, LightType::Spot);
    assert_ne!(LightType::Directional, LightType::Spot);
}

#[test]
fn directional_light_default_omni_shadow_mode() {
    let light = Light3D::directional(Light3DId(1));
    assert_eq!(light.light_type, LightType::Directional);
    assert_eq!(light.omni_shadow_mode, OmniShadowMode::DualParaboloid);
}

#[test]
fn spot_light_is_not_point_type() {
    let light = Light3D::spot(
        Light3DId(1),
        Vector3::new(0.0, 3.0, 0.0),
        Vector3::new(0.0, -1.0, 0.0),
    );
    assert_eq!(light.light_type, LightType::Spot);
    assert_ne!(light.light_type, LightType::Point);
}

// ============================================================================
// 10. Multiple cubemaps can coexist
// ============================================================================

#[test]
fn multiple_cubemaps_are_independent() {
    let mut cm1 = ShadowCubemap::new(16);
    let mut cm2 = ShadowCubemap::new(16);

    cm1.test_and_set(CubeFace::PositiveX, 0, 0, 3.0);
    cm2.test_and_set(CubeFace::PositiveX, 0, 0, 7.0);

    assert!((cm1.get_depth(CubeFace::PositiveX, 0, 0) - 3.0).abs() < 1e-6);
    assert!((cm2.get_depth(CubeFace::PositiveX, 0, 0) - 7.0).abs() < 1e-6);
}

#[test]
fn cubemap_different_resolutions() {
    let small = ShadowCubemap::new(8);
    let large = ShadowCubemap::new(64);
    assert_eq!(small.faces[0].len(), 64);
    assert_eq!(large.faces[0].len(), 4096);
}
