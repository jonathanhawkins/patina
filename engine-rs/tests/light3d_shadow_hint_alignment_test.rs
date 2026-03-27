//! pat-dyfmh: Light3D shadow_enabled hint value alignment.
//!
//! Ensures that the ClassDB registration for Light3D and its subclasses
//! (DirectionalLight3D, OmniLight3D, SpotLight3D) includes shadow_enabled
//! with the correct default value and that all light properties are inherited.

use gdobject::class_db::*;
use gdvariant::Variant;
use std::sync::Mutex;

static TEST_LOCK: Mutex<()> = Mutex::new(());

fn setup() -> std::sync::MutexGuard<'static, ()> {
    let guard = TEST_LOCK.lock().unwrap();
    clear_for_testing();
    register_class(ClassRegistration::new("Object"));
    register_class(ClassRegistration::new("Node").parent("Object"));
    register_3d_classes();
    guard
}

// -- shadow_enabled property exists on all light types --

#[test]
fn directional_light_has_shadow_enabled() {
    let _g = setup();
    assert!(class_has_property("DirectionalLight3D", "shadow_enabled"));
}

#[test]
fn omni_light_has_shadow_enabled() {
    let _g = setup();
    assert!(class_has_property("OmniLight3D", "shadow_enabled"));
}

#[test]
fn spot_light_has_shadow_enabled() {
    let _g = setup();
    assert!(class_has_property("SpotLight3D", "shadow_enabled"));
}

// -- shadow_enabled defaults to false (matching Godot) --

#[test]
fn shadow_enabled_default_is_false() {
    let _g = setup();
    let props = get_property_list("DirectionalLight3D");
    let shadow = props.iter().find(|p| p.name == "shadow_enabled").unwrap();
    assert_eq!(shadow.default_value, Variant::Bool(false));
}

#[test]
fn omni_shadow_enabled_default_is_false() {
    let _g = setup();
    let props = get_property_list("OmniLight3D");
    let shadow = props.iter().find(|p| p.name == "shadow_enabled").unwrap();
    assert_eq!(shadow.default_value, Variant::Bool(false));
}

// -- light_energy property --

#[test]
fn light3d_has_light_energy() {
    let _g = setup();
    assert!(class_has_property("DirectionalLight3D", "light_energy"));
    assert!(class_has_property("OmniLight3D", "light_energy"));
    assert!(class_has_property("SpotLight3D", "light_energy"));
}

#[test]
fn light_energy_default_is_one() {
    let _g = setup();
    let props = get_property_list("DirectionalLight3D");
    let energy = props.iter().find(|p| p.name == "light_energy").unwrap();
    assert_eq!(energy.default_value, Variant::Float(1.0));
}

// -- shadow_bias and shadow_blur defaults --

#[test]
fn shadow_bias_default() {
    let _g = setup();
    let props = get_property_list("OmniLight3D");
    let bias = props.iter().find(|p| p.name == "shadow_bias").unwrap();
    assert_eq!(bias.default_value, Variant::Float(0.1));
}

#[test]
fn shadow_blur_default() {
    let _g = setup();
    let props = get_property_list("DirectionalLight3D");
    let blur = props.iter().find(|p| p.name == "shadow_blur").unwrap();
    assert_eq!(blur.default_value, Variant::Float(1.0));
}

// -- Inheritance: Light3D properties inherited by all subclasses --

#[test]
fn light_color_inherited_by_omni() {
    let _g = setup();
    assert!(class_has_property("OmniLight3D", "light_color"));
}

#[test]
fn light_color_inherited_by_spot() {
    let _g = setup();
    assert!(class_has_property("SpotLight3D", "light_color"));
}

// -- Subclass-specific properties --

#[test]
fn omni_has_range_property() {
    let _g = setup();
    assert!(class_has_property("OmniLight3D", "omni_range"));
}

#[test]
fn omni_has_shadow_mode_property() {
    let _g = setup();
    assert!(class_has_property("OmniLight3D", "omni_shadow_mode"));
}

#[test]
fn spot_has_angle_property() {
    let _g = setup();
    assert!(class_has_property("SpotLight3D", "spot_angle"));
}

#[test]
fn directional_has_shadow_mode_property() {
    let _g = setup();
    assert!(class_has_property("DirectionalLight3D", "directional_shadow_mode"));
}

// -- Light3D is a Node3D (inherits transform) --

#[test]
fn light3d_inherits_transform() {
    let _g = setup();
    assert!(class_has_property("DirectionalLight3D", "transform"));
    assert!(class_has_property("OmniLight3D", "transform"));
    assert!(class_has_property("SpotLight3D", "transform"));
}
