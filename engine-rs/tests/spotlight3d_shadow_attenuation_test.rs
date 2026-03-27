//! Integration tests for SpotLight3D with shadow mapping and attenuation.
//!
//! Validates that SpotLight3D is registered in ClassDB, synced through the
//! scene tree, and produces correct attenuation behavior in the shader pipeline.

use std::sync::Mutex;

use gdobject::class_db;
use gdscene::node::Node;
use gdscene::node3d;
use gdscene::scene_tree::SceneTree;
use gdvariant::Variant;

static TEST_LOCK: Mutex<()> = Mutex::new(());

/// Ensure ClassDB has Object, Node, and 3D classes registered.
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

/// Helper to build a tree with ClassDB initialized.
fn make_tree() -> (SceneTree, std::sync::MutexGuard<'static, ()>) {
    let guard = init_classdb();
    let tree = SceneTree::new();
    (tree, guard)
}

// ── ClassDB registration ─────────────────────────────────────────────

#[test]
fn spotlight3d_registered_in_classdb() {
    let _g = init_classdb();
    let info = class_db::get_class_info("SpotLight3D");
    assert!(info.is_some(), "SpotLight3D should be registered in ClassDB");
    let info = info.unwrap();
    assert_eq!(info.parent_class, "Node3D");
}

#[test]
fn spotlight3d_has_expected_properties() {
    let _g = init_classdb();
    let info = class_db::get_class_info("SpotLight3D").expect("registered");

    let prop_names: Vec<&str> = info.properties.iter().map(|p| p.name.as_str()).collect();
    for expected in &[
        "light_energy",
        "light_color",
        "shadow_enabled",
        "spot_range",
        "spot_attenuation",
        "spot_angle",
        "spot_angle_attenuation",
    ] {
        assert!(
            prop_names.contains(expected),
            "SpotLight3D missing property: {expected}"
        );
    }
}

#[test]
fn spotlight3d_default_property_values() {
    let _g = init_classdb();
    let info = class_db::get_class_info("SpotLight3D").unwrap();
    let find = |name: &str| info.properties.iter().find(|p| p.name == name);

    assert_eq!(
        find("spot_angle").unwrap().default_value,
        Variant::Float(45.0)
    );
    assert_eq!(
        find("spot_range").unwrap().default_value,
        Variant::Float(5.0)
    );
    assert_eq!(
        find("spot_attenuation").unwrap().default_value,
        Variant::Float(1.0)
    );
    assert_eq!(
        find("spot_angle_attenuation").unwrap().default_value,
        Variant::Float(1.0)
    );
    assert_eq!(
        find("light_energy").unwrap().default_value,
        Variant::Float(1.0)
    );
    assert_eq!(
        find("shadow_enabled").unwrap().default_value,
        Variant::Bool(false)
    );
}

// ── Scene tree property helpers ──────────────────────────────────────

#[test]
fn spotlight3d_scene_tree_properties_roundtrip() {
    let (mut tree, _g) = make_tree();
    let root = tree.root_id();
    let spot = Node::new("Spot", "SpotLight3D");
    let id = tree.add_child(root, spot).unwrap();

    // Set all spot-specific properties.
    node3d::set_spot_angle(&mut tree, id, 30.0);
    node3d::set_spot_range(&mut tree, id, 15.0);
    node3d::set_spot_attenuation(&mut tree, id, 2.0);
    node3d::set_spot_angle_attenuation(&mut tree, id, 0.5);
    node3d::set_light_energy(&mut tree, id, 0.8);
    node3d::set_shadow_enabled(&mut tree, id, true);

    // Verify roundtrip.
    assert!((node3d::get_spot_angle(&tree, id) - 30.0).abs() < 1e-5);
    assert!((node3d::get_spot_range(&tree, id) - 15.0).abs() < 1e-5);
    assert!((node3d::get_spot_attenuation(&tree, id) - 2.0).abs() < 1e-5);
    assert!((node3d::get_spot_angle_attenuation(&tree, id) - 0.5).abs() < 1e-5);
    assert!((node3d::get_light_energy(&tree, id) - 0.8).abs() < 1e-5);

    let node = tree.get_node(id).unwrap();
    assert_eq!(node.get_property("shadow_enabled"), Variant::Bool(true));
}

// ── Light3D struct ───────────────────────────────────────────────────

#[test]
fn light3d_spot_factory_defaults() {
    use gdcore::math::Vector3;
    use gdserver3d::light::{Light3D, Light3DId, LightType};

    let light = Light3D::spot(
        Light3DId(1),
        Vector3::new(0.0, 5.0, 0.0),
        Vector3::new(0.0, -1.0, 0.0),
    );
    assert_eq!(light.light_type, LightType::Spot);
    assert!((light.spot_angle - std::f32::consts::FRAC_PI_4).abs() < 1e-5);
    assert!((light.range - 10.0).abs() < 1e-5);
    assert!((light.attenuation - 1.0).abs() < 1e-5);
    assert!((light.spot_angle_attenuation - 1.0).abs() < 1e-5);
    assert!(!light.shadow_enabled);
}

// ── Shader cone + attenuation math ───────────────────────────────────

#[test]
fn spotlight_shader_cone_inside_vs_outside() {
    use gdcore::math::{Color, Vector3};
    use gdrender3d::shader::{LightKind, LightUniform};

    let spot = LightUniform {
        kind: LightKind::Spot,
        direction: Vector3::new(0.0, -1.0, 0.0), // Pointing down.
        color: Color::new(1.0, 1.0, 1.0, 1.0),
        position: Vector3::new(0.0, 10.0, 0.0),
        range: 20.0,
        attenuation: 1.0,
        spot_angle: std::f32::consts::FRAC_PI_6, // 30-degree half-angle.
        spot_angle_attenuation: 1.0,
        shadow_enabled: true,
    };

    // Directly below — inside cone.
    let (_dir, inside_i) = spot.evaluate(Vector3::new(0.0, 0.0, 0.0));
    assert!(inside_i > 0.3, "inside intensity = {inside_i}");

    // Far off to the side — outside cone.
    let (_dir, outside_i) = spot.evaluate(Vector3::new(50.0, 0.0, 0.0));
    assert!(
        outside_i < 0.01,
        "outside intensity should be ~0, got {outside_i}"
    );
}

#[test]
fn spotlight_shader_distance_attenuation_falloff() {
    use gdcore::math::{Color, Vector3};
    use gdrender3d::shader::{LightKind, LightUniform};

    let spot = LightUniform {
        kind: LightKind::Spot,
        direction: Vector3::new(0.0, -1.0, 0.0),
        color: Color::new(1.0, 1.0, 1.0, 1.0),
        position: Vector3::new(0.0, 10.0, 0.0),
        range: 10.0,
        attenuation: 2.0, // Quadratic falloff.
        spot_angle: std::f32::consts::FRAC_PI_4,
        spot_angle_attenuation: 1.0,
        shadow_enabled: false,
    };

    // Close to the light (1 unit below).
    let (_dir, close_i) = spot.evaluate(Vector3::new(0.0, 9.0, 0.0));
    // Far from the light (at the edge of range).
    let (_dir, far_i) = spot.evaluate(Vector3::new(0.0, 0.0, 0.0));

    assert!(
        close_i > far_i,
        "closer should be brighter: close={close_i}, far={far_i}"
    );
    assert!(close_i > 0.8, "close intensity = {close_i}");
    assert!(far_i < 0.05, "far intensity = {far_i}");
}

#[test]
fn spotlight_angle_attenuation_exponent_affects_falloff() {
    use gdcore::math::{Color, Vector3};
    use gdrender3d::shader::{LightKind, LightUniform};

    let make = |angle_atten: f32| LightUniform {
        kind: LightKind::Spot,
        direction: Vector3::new(0.0, -1.0, 0.0),
        color: Color::new(1.0, 1.0, 1.0, 1.0),
        position: Vector3::new(0.0, 10.0, 0.0),
        range: 20.0,
        attenuation: 1.0,
        spot_angle: std::f32::consts::FRAC_PI_4,
        spot_angle_attenuation: angle_atten,
        shadow_enabled: false,
    };

    // Fragment near the edge of the cone.
    let edge_frag = Vector3::new(5.0, 0.0, 0.0);

    let (_dir, sharp_i) = make(0.1).evaluate(edge_frag); // Sharp falloff.
    let (_dir, soft_i) = make(3.0).evaluate(edge_frag); // Soft falloff.

    // With higher angle attenuation exponent, edges dim faster.
    assert!(
        sharp_i > soft_i,
        "sharp={sharp_i} should be brighter than soft={soft_i} near cone edge"
    );
}
