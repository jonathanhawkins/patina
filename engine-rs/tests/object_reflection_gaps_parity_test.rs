//! Object/property reflection gap closure parity tests.
//!
//! Validates the new rich reflection APIs on GodotObject trait and ObjectBase:
//! - `get_property_list_info()` / `get_method_list_info()` / `get_signal_list_info()`
//! - `get_property_info()` / `get_method_info()` / `get_signal_info()` (per-item)
//! - `get_parent_class()` on GodotObject trait
//! - `can_instantiate()` in ClassDB
//! - `get_signal_connection_list()` on ObjectBase
//!
//! These test the reflection surface that mirrors Godot's Object API.

use gdcore::id::ObjectId;
use gdcore::math::Vector2;
use gdobject::signal::Connection;
use gdobject::{
    can_instantiate, clear_for_testing, register_class, ArgumentInfo,
    ClassRegistration, GenericObject, GodotObject, MethodInfo, ObjectBase, PropertyInfo,
    SignalInfo,
};
use gdvariant::Variant;
use std::sync::Mutex;

static TEST_LOCK: Mutex<()> = Mutex::new(());

fn setup() -> std::sync::MutexGuard<'static, ()> {
    let guard = TEST_LOCK.lock().expect("test lock poisoned");
    clear_for_testing();

    // Register a standard hierarchy: Object -> Node -> CanvasItem -> Node2D -> Sprite2D
    register_class(
        ClassRegistration::new("Object")
            .method(MethodInfo::new("get_class", 0).const_method())
            .method(MethodInfo::new("get_instance_id", 0).const_method())
            .method(
                MethodInfo::new("set", 2).with_args(vec![
                    ArgumentInfo::new("property", 4), // StringName
                    ArgumentInfo::new("value", 0),     // Variant
                ]),
            )
            .method(
                MethodInfo::new("get", 1)
                    .with_args(vec![ArgumentInfo::new("property", 4)])
                    .const_method(),
            )
            .signal(SignalInfo::new("script_changed")),
    );
    register_class(
        ClassRegistration::new("Node")
            .parent("Object")
            .property(
                PropertyInfo::new("name", Variant::String(String::new()))
                    .with_type(4)
                    .with_usage(4102),
            )
            .property(
                PropertyInfo::new("process_mode", Variant::Int(0))
                    .with_type(2)
                    .with_hint(2, "Inherit,Pausable,WhenPaused,Always,Disabled")
                    .with_usage(4102),
            )
            .method(MethodInfo::new("_ready", 0).virtual_method())
            .method(
                MethodInfo::new("_process", 1)
                    .with_args(vec![ArgumentInfo::new("delta", 3)])
                    .virtual_method(),
            )
            .method(MethodInfo::new("add_child", 1).with_args(vec![ArgumentInfo::new(
                "node",
                24,
            )
            .with_class("Node")]))
            .method(MethodInfo::new("get_child_count", 0).const_method())
            .signal(
                SignalInfo::new("ready"),
            )
            .signal(
                SignalInfo::new("tree_entered"),
            )
            .signal(
                SignalInfo::new("tree_exited"),
            ),
    );
    register_class(
        ClassRegistration::new("CanvasItem")
            .parent("Node")
            .property(
                PropertyInfo::new("visible", Variant::Bool(true))
                    .with_type(1)
                    .with_usage(4102),
            )
            .property(
                PropertyInfo::new("modulate", Variant::Color(gdcore::math::Color::WHITE))
                    .with_type(20)
                    .with_usage(4102),
            )
            .method(MethodInfo::new("_draw", 0).virtual_method())
            .method(MethodInfo::new("queue_redraw", 0))
            .signal(SignalInfo::new("draw"))
            .signal(SignalInfo::new("visibility_changed")),
    );
    register_class(
        ClassRegistration::new("Node2D")
            .parent("CanvasItem")
            .property(
                PropertyInfo::new("position", Variant::Vector2(Vector2::ZERO))
                    .with_type(5)
                    .with_usage(4102),
            )
            .property(
                PropertyInfo::new("rotation", Variant::Float(0.0))
                    .with_type(3)
                    .with_usage(4102),
            )
            .property(
                PropertyInfo::new("scale", Variant::Vector2(Vector2::ONE))
                    .with_type(5)
                    .with_usage(4102),
            )
            .method(MethodInfo::new("get_position", 0).const_method())
            .method(MethodInfo::new("set_position", 1).with_args(vec![ArgumentInfo::new(
                "position",
                5,
            )]))
            .method(MethodInfo::new("get_global_position", 0).const_method()),
    );
    register_class(
        ClassRegistration::new("Sprite2D")
            .parent("Node2D")
            .property(
                PropertyInfo::new("texture", Variant::String(String::new()))
                    .with_type(24)
                    .with_usage(4102),
            )
            .property(
                PropertyInfo::new("centered", Variant::Bool(true))
                    .with_type(1)
                    .with_usage(4102),
            )
            .method(MethodInfo::new("get_rect", 0).const_method())
            .signal(
                SignalInfo::new("texture_changed"),
            ),
    );

    guard
}

// ── GodotObject::get_property_list_info ─────────────────────────────

#[test]
fn property_list_info_returns_full_metadata() {
    let _g = setup();
    let obj = GenericObject::new("Node2D");
    let props = obj.get_property_list_info();

    // Should include own + inherited properties
    let names: Vec<&str> = props.iter().map(|p| p.name.as_str()).collect();
    assert!(names.contains(&"position"), "missing own property 'position'");
    assert!(names.contains(&"name"), "missing inherited property 'name'");
    assert!(
        names.contains(&"visible"),
        "missing inherited property 'visible'"
    );
}

#[test]
fn property_list_info_includes_type_metadata() {
    let _g = setup();
    let obj = GenericObject::new("Node2D");
    let props = obj.get_property_list_info();

    let position = props.iter().find(|p| p.name == "position").unwrap();
    assert_eq!(position.property_type, 5, "position should be Vector2 type");
    assert_eq!(position.usage, 4102, "position should have standard usage");
}

#[test]
fn property_list_info_includes_hint_metadata() {
    let _g = setup();
    let obj = GenericObject::new("Node");
    let props = obj.get_property_list_info();

    let process_mode = props.iter().find(|p| p.name == "process_mode").unwrap();
    assert_eq!(process_mode.hint, 2, "process_mode should have Enum hint");
    assert!(
        process_mode.hint_string.contains("Pausable"),
        "hint_string should contain enum values"
    );
}

#[test]
fn property_list_info_count_matches_name_list() {
    let _g = setup();
    let obj = GenericObject::new("Sprite2D");
    let info_count = obj.get_property_list_info().len();
    let name_count = obj.get_property_list().len();
    assert_eq!(
        info_count, name_count,
        "info list and name list should have same count"
    );
}

// ── GodotObject::get_method_list_info ───────────────────────────────

#[test]
fn method_list_info_returns_full_metadata() {
    let _g = setup();
    let obj = GenericObject::new("Node2D");
    let methods = obj.get_method_list_info();

    let names: Vec<&str> = methods.iter().map(|m| m.name.as_str()).collect();
    assert!(
        names.contains(&"get_position"),
        "missing own method 'get_position'"
    );
    assert!(
        names.contains(&"_ready"),
        "missing inherited method '_ready'"
    );
    assert!(
        names.contains(&"get_class"),
        "missing Object method 'get_class'"
    );
}

#[test]
fn method_list_info_includes_argument_metadata() {
    let _g = setup();
    let obj = GenericObject::new("Node");
    let methods = obj.get_method_list_info();

    let process = methods.iter().find(|m| m.name == "_process").unwrap();
    assert_eq!(process.argument_count, 1);
    assert_eq!(process.arguments.len(), 1);
    assert_eq!(process.arguments[0].name, "delta");
    assert_eq!(process.arguments[0].arg_type, 3); // Float
}

#[test]
fn method_list_info_includes_flags() {
    let _g = setup();
    let obj = GenericObject::new("Node");
    let methods = obj.get_method_list_info();

    let ready = methods.iter().find(|m| m.name == "_ready").unwrap();
    assert!(ready.is_virtual, "_ready should be virtual");

    let get_child_count = methods.iter().find(|m| m.name == "get_child_count").unwrap();
    assert!(get_child_count.is_const, "get_child_count should be const");
}

#[test]
fn method_list_info_count_matches_name_list() {
    let _g = setup();
    let obj = GenericObject::new("Node2D");
    let info_count = obj.get_method_list_info().len();
    let name_count = obj.get_method_list().len();
    assert_eq!(
        info_count, name_count,
        "info list and name list should have same count"
    );
}

// ── GodotObject::get_signal_list_info ───────────────────────────────

#[test]
fn signal_list_info_returns_full_metadata() {
    let _g = setup();
    let obj = GenericObject::new("Sprite2D");
    let signals = obj.get_signal_list_info();

    let names: Vec<&str> = signals.iter().map(|s| s.name.as_str()).collect();
    assert!(
        names.contains(&"texture_changed"),
        "missing own signal 'texture_changed'"
    );
    assert!(
        names.contains(&"ready"),
        "missing inherited signal 'ready'"
    );
    assert!(
        names.contains(&"script_changed"),
        "missing Object signal 'script_changed'"
    );
}

#[test]
fn signal_list_info_count_matches_name_list() {
    let _g = setup();
    let obj = GenericObject::new("Node2D");
    let info_count = obj.get_signal_list_info().len();
    let name_count = obj.get_signal_list().len();
    assert_eq!(
        info_count, name_count,
        "info list and name list should have same count"
    );
}

// ── GodotObject::get_property_info (per-item) ───────────────────────

#[test]
fn get_property_info_own_property() {
    let _g = setup();
    let obj = GenericObject::new("Node2D");
    let info = obj.get_property_info("position").expect("position should exist");
    assert_eq!(info.name, "position");
    assert_eq!(info.property_type, 5); // Vector2
    assert_eq!(
        info.default_value,
        Variant::Vector2(Vector2::ZERO)
    );
}

#[test]
fn get_property_info_inherited_property() {
    let _g = setup();
    let obj = GenericObject::new("Sprite2D");
    let info = obj
        .get_property_info("name")
        .expect("inherited 'name' should be found");
    assert_eq!(info.name, "name");
    assert_eq!(info.property_type, 4); // String
}

#[test]
fn get_property_info_missing_returns_none() {
    let _g = setup();
    let obj = GenericObject::new("Node2D");
    assert!(obj.get_property_info("nonexistent").is_none());
}

// ── GodotObject::get_method_info (per-item) ─────────────────────────

#[test]
fn get_method_info_own_method() {
    let _g = setup();
    let obj = GenericObject::new("Node2D");
    let info = obj
        .get_method_info("set_position")
        .expect("set_position should exist");
    assert_eq!(info.name, "set_position");
    assert_eq!(info.argument_count, 1);
    assert_eq!(info.arguments[0].name, "position");
}

#[test]
fn get_method_info_inherited_method() {
    let _g = setup();
    let obj = GenericObject::new("Sprite2D");
    let info = obj
        .get_method_info("add_child")
        .expect("inherited add_child should be found");
    assert_eq!(info.arguments[0].class_name, "Node");
}

#[test]
fn get_method_info_missing_returns_none() {
    let _g = setup();
    let obj = GenericObject::new("Node");
    assert!(obj.get_method_info("nonexistent_method").is_none());
}

// ── GodotObject::get_signal_info (per-item) ─────────────────────────

#[test]
fn get_signal_info_own_signal() {
    let _g = setup();
    let obj = GenericObject::new("Sprite2D");
    let info = obj
        .get_signal_info("texture_changed")
        .expect("texture_changed should exist");
    assert_eq!(info.name, "texture_changed");
}

#[test]
fn get_signal_info_inherited_signal() {
    let _g = setup();
    let obj = GenericObject::new("Node2D");
    let info = obj
        .get_signal_info("ready")
        .expect("inherited ready signal should be found");
    assert_eq!(info.name, "ready");
}

#[test]
fn get_signal_info_missing_returns_none() {
    let _g = setup();
    let obj = GenericObject::new("Node");
    assert!(obj.get_signal_info("nonexistent_signal").is_none());
}

// ── GodotObject::get_parent_class ───────────────────────────────────

#[test]
fn get_parent_class_returns_direct_parent() {
    let _g = setup();
    let obj = GenericObject::new("Node2D");
    assert_eq!(obj.get_parent_class(), "CanvasItem");
}

#[test]
fn get_parent_class_root_returns_empty() {
    let _g = setup();
    let obj = GenericObject::new("Object");
    assert_eq!(obj.get_parent_class(), "");
}

#[test]
fn get_parent_class_unregistered_returns_empty() {
    let _g = setup();
    let obj = GenericObject::new("UnregisteredClass");
    assert_eq!(obj.get_parent_class(), "");
}

// ── ClassDB::can_instantiate ────────────────────────────────────────

#[test]
fn can_instantiate_registered_class() {
    let _g = setup();
    assert!(can_instantiate("Node2D"));
    assert!(can_instantiate("Sprite2D"));
    assert!(can_instantiate("Object"));
}

#[test]
fn can_instantiate_unregistered_returns_false() {
    let _g = setup();
    assert!(!can_instantiate("DoesNotExist"));
    assert!(!can_instantiate(""));
}

// ── ObjectBase::get_signal_connection_list ───────────────────────────

#[test]
fn signal_connection_list_empty_initially() {
    let _g = setup();
    let base = ObjectBase::new("Node");
    assert!(base.get_signal_connection_list("ready").is_empty());
}

#[test]
fn signal_connection_list_after_connect() {
    let _g = setup();
    let mut base = ObjectBase::new("Node");

    let target_id = ObjectId::from_raw(42);
    base.signals_mut()
        .connect("ready", Connection::new(target_id, "on_ready"));
    base.signals_mut()
        .connect("ready", Connection::new(ObjectId::from_raw(43), "on_ready_2"));

    let connections = base.get_signal_connection_list("ready");
    assert_eq!(connections.len(), 2);
    assert_eq!(connections[0].target_id, target_id);
    assert_eq!(connections[0].method, "on_ready");
    assert_eq!(connections[1].method, "on_ready_2");
}

#[test]
fn signal_connection_list_different_signals_independent() {
    let _g = setup();
    let mut base = ObjectBase::new("Node");

    base.signals_mut().connect(
        "ready",
        Connection::new(ObjectId::from_raw(1), "handler_a"),
    );
    base.signals_mut().connect(
        "tree_entered",
        Connection::new(ObjectId::from_raw(2), "handler_b"),
    );

    assert_eq!(base.get_signal_connection_list("ready").len(), 1);
    assert_eq!(base.get_signal_connection_list("tree_entered").len(), 1);
    assert_eq!(
        base.get_signal_connection_list("nonexistent").len(),
        0
    );
}

// ── ObjectBase per-item metadata methods ────────────────────────────

#[test]
fn object_base_get_property_info() {
    let _g = setup();
    let base = ObjectBase::new("Sprite2D");
    let info = base.get_property_info("centered").unwrap();
    assert_eq!(info.property_type, 1); // Bool
    assert_eq!(info.default_value, Variant::Bool(true));
}

#[test]
fn object_base_get_method_info() {
    let _g = setup();
    let base = ObjectBase::new("Node2D");
    let info = base.get_method_info("get_global_position").unwrap();
    assert!(info.is_const);
    assert_eq!(info.argument_count, 0);
}

#[test]
fn object_base_get_signal_info() {
    let _g = setup();
    let base = ObjectBase::new("CanvasItem");
    let info = base.get_signal_info("visibility_changed").unwrap();
    assert_eq!(info.name, "visibility_changed");
}

// ── Cross-cutting: info consistency ─────────────────────────────────

#[test]
fn property_info_names_match_property_list() {
    let _g = setup();
    let obj = GenericObject::new("Sprite2D");
    let info_names: Vec<String> = obj
        .get_property_list_info()
        .iter()
        .map(|p| p.name.clone())
        .collect();
    let list_names = obj.get_property_list();
    assert_eq!(info_names, list_names);
}

#[test]
fn method_info_names_match_method_list() {
    let _g = setup();
    let obj = GenericObject::new("Sprite2D");
    let info_names: Vec<String> = obj
        .get_method_list_info()
        .iter()
        .map(|m| m.name.clone())
        .collect();
    let list_names = obj.get_method_list();
    assert_eq!(info_names, list_names);
}

#[test]
fn signal_info_names_match_signal_list() {
    let _g = setup();
    let obj = GenericObject::new("Sprite2D");
    let info_names: Vec<String> = obj
        .get_signal_list_info()
        .iter()
        .map(|s| s.name.clone())
        .collect();
    let list_names = obj.get_signal_list();
    assert_eq!(info_names, list_names);
}

#[test]
fn per_item_lookup_matches_list_entry() {
    let _g = setup();
    let obj = GenericObject::new("Node2D");

    // Each property from the list should be individually retrievable
    for prop_info in obj.get_property_list_info() {
        let single = obj
            .get_property_info(&prop_info.name)
            .unwrap_or_else(|| panic!("get_property_info({}) should succeed", prop_info.name));
        assert_eq!(single.name, prop_info.name);
        assert_eq!(single.property_type, prop_info.property_type);
    }

    // Each method from the list should be individually retrievable
    for method_info in obj.get_method_list_info() {
        let single = obj
            .get_method_info(&method_info.name)
            .unwrap_or_else(|| panic!("get_method_info({}) should succeed", method_info.name));
        assert_eq!(single.name, method_info.name);
        assert_eq!(single.argument_count, method_info.argument_count);
    }

    // Each signal from the list should be individually retrievable
    for signal_info in obj.get_signal_list_info() {
        let single = obj
            .get_signal_info(&signal_info.name)
            .unwrap_or_else(|| panic!("get_signal_info({}) should succeed", signal_info.name));
        assert_eq!(single.name, signal_info.name);
    }
}

// ── Inheritance depth validation ────────────────────────────────────

#[test]
fn deep_inheritance_property_info_walks_full_chain() {
    let _g = setup();
    // Sprite2D -> Node2D -> CanvasItem -> Node -> Object
    let obj = GenericObject::new("Sprite2D");
    let props = obj.get_property_list_info();

    // From Sprite2D
    assert!(props.iter().any(|p| p.name == "texture"));
    // From Node2D
    assert!(props.iter().any(|p| p.name == "position"));
    // From CanvasItem
    assert!(props.iter().any(|p| p.name == "visible"));
    // From Node
    assert!(props.iter().any(|p| p.name == "name"));
}

#[test]
fn deep_inheritance_method_info_walks_full_chain() {
    let _g = setup();
    let obj = GenericObject::new("Sprite2D");
    let methods = obj.get_method_list_info();

    // From Sprite2D
    assert!(methods.iter().any(|m| m.name == "get_rect"));
    // From Node2D
    assert!(methods.iter().any(|m| m.name == "get_position"));
    // From CanvasItem
    assert!(methods.iter().any(|m| m.name == "queue_redraw"));
    // From Node
    assert!(methods.iter().any(|m| m.name == "_ready"));
    // From Object
    assert!(methods.iter().any(|m| m.name == "get_class"));
}

#[test]
fn deep_inheritance_signal_info_walks_full_chain() {
    let _g = setup();
    let obj = GenericObject::new("Sprite2D");
    let signals = obj.get_signal_list_info();

    // From Sprite2D
    assert!(signals.iter().any(|s| s.name == "texture_changed"));
    // From CanvasItem
    assert!(signals.iter().any(|s| s.name == "draw"));
    // From Node
    assert!(signals.iter().any(|s| s.name == "ready"));
    // From Object
    assert!(signals.iter().any(|s| s.name == "script_changed"));
}
