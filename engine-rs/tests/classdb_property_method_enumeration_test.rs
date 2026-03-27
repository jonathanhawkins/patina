//! pat-jpqv: Full ClassDB property and method enumeration against oracle output.
//!
//! Validates that get_property_list() walks the inheritance chain and returns
//! both own and inherited properties, matching Godot's ClassDB behavior.

use gdobject::class_db::*;
use gdobject::GodotObject;
use gdvariant::Variant;
use std::sync::Mutex;

static TEST_LOCK: Mutex<()> = Mutex::new(());

fn setup() -> std::sync::MutexGuard<'static, ()> {
    let guard = TEST_LOCK.lock().expect("test lock");
    clear_for_testing();

    // Register a minimal class hierarchy: Object -> Node -> Node2D -> Sprite2D
    register_class(ClassRegistration::new("Object"));
    register_class(
        ClassRegistration::new("Node")
            .parent("Object")
            .property(PropertyInfo::new("name", Variant::String(String::new())))
            .property(PropertyInfo::new("process_mode", Variant::Int(0)))
            .method(MethodInfo::new("get_name", 0))
            .method(MethodInfo::new("add_child", 1))
            .method(MethodInfo::new("get_parent", 0)),
    );
    register_class(
        ClassRegistration::new("Node2D")
            .parent("Node")
            .property(PropertyInfo::new(
                "position",
                Variant::Vector2(gdcore::math::Vector2::ZERO),
            ))
            .property(PropertyInfo::new("rotation", Variant::Float(0.0)))
            .property(PropertyInfo::new("visible", Variant::Bool(true)))
            .method(MethodInfo::new("get_position", 0))
            .method(MethodInfo::new("set_position", 1)),
    );
    register_class(
        ClassRegistration::new("Sprite2D")
            .parent("Node2D")
            .property(PropertyInfo::new("texture", Variant::Nil))
            .property(PropertyInfo::new("flip_h", Variant::Bool(false)))
            .method(MethodInfo::new("set_texture", 1)),
    );
    guard
}

// ===========================================================================
// get_property_list: inheritance chain
// ===========================================================================

#[test]
fn property_list_includes_own_properties() {
    let _g = setup();
    let props = get_property_list("Node2D", true); // own only
    let names: Vec<&str> = props.iter().map(|p| p.name.as_str()).collect();
    assert!(names.contains(&"position"));
    assert!(names.contains(&"rotation"));
    assert!(names.contains(&"visible"));
    assert!(!names.contains(&"name"), "own-only should not include inherited");
}

#[test]
fn property_list_includes_inherited_properties() {
    let _g = setup();
    let props = get_property_list("Node2D", false); // with inheritance
    let names: Vec<&str> = props.iter().map(|p| p.name.as_str()).collect();
    assert!(names.contains(&"position"), "own property");
    assert!(names.contains(&"name"), "inherited from Node");
    assert!(names.contains(&"process_mode"), "inherited from Node");
}

#[test]
fn property_list_deep_inheritance() {
    let _g = setup();
    let props = get_property_list("Sprite2D", false);
    let names: Vec<&str> = props.iter().map(|p| p.name.as_str()).collect();
    // Sprite2D own
    assert!(names.contains(&"texture"));
    assert!(names.contains(&"flip_h"));
    // Node2D inherited
    assert!(names.contains(&"position"));
    assert!(names.contains(&"rotation"));
    // Node inherited
    assert!(names.contains(&"name"));
}

#[test]
fn property_list_base_first_ordering() {
    let _g = setup();
    let props = get_property_list("Sprite2D", false);
    let names: Vec<&str> = props.iter().map(|p| p.name.as_str()).collect();

    // Base properties should come before derived
    let name_idx = names.iter().position(|n| *n == "name").unwrap();
    let pos_idx = names.iter().position(|n| *n == "position").unwrap();
    let tex_idx = names.iter().position(|n| *n == "texture").unwrap();

    assert!(
        name_idx < pos_idx,
        "Node.name should come before Node2D.position"
    );
    assert!(
        pos_idx < tex_idx,
        "Node2D.position should come before Sprite2D.texture"
    );
}

#[test]
fn property_list_empty_for_unknown_class() {
    let _g = setup();
    let props = get_property_list("NonexistentClass", false);
    assert!(props.is_empty());
}

// ===========================================================================
// class_has_property: walks inheritance
// ===========================================================================

#[test]
fn has_property_own() {
    let _g = setup();
    assert!(class_has_property("Node2D", "position"));
}

#[test]
fn has_property_inherited() {
    let _g = setup();
    assert!(class_has_property("Node2D", "name"));
    assert!(class_has_property("Sprite2D", "position"));
    assert!(class_has_property("Sprite2D", "name"));
}

#[test]
fn has_property_not_found() {
    let _g = setup();
    assert!(!class_has_property("Node", "position"));
    assert!(!class_has_property("Node2D", "texture"));
}

// ===========================================================================
// class_has_method: walks inheritance
// ===========================================================================

#[test]
fn has_method_own() {
    let _g = setup();
    assert!(class_has_method("Node2D", "get_position"));
}

#[test]
fn has_method_inherited() {
    let _g = setup();
    assert!(class_has_method("Node2D", "get_name"));
    assert!(class_has_method("Sprite2D", "add_child"));
}

#[test]
fn has_method_not_found() {
    let _g = setup();
    assert!(!class_has_method("Node", "get_position"));
}

// ===========================================================================
// instantiate: default properties from hierarchy
// ===========================================================================

#[test]
fn instantiate_applies_inherited_defaults() {
    let _g = setup();
    let obj = instantiate("Sprite2D").unwrap();
    // Should have defaults from all ancestors
    assert_eq!(
        obj.get_property("flip_h"),
        Variant::Bool(false),
        "own default"
    );
    assert_eq!(
        obj.get_property("visible"),
        Variant::Bool(true),
        "inherited from Node2D"
    );
}
