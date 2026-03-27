//! pat-glof / pat-unv6: Object/property reflection parity tests.
//!
//! Verifies that gdobject's reflection system — ClassDB property/method/signal
//! enumeration, ObjectBase get/set/has, meta properties, inheritance-aware
//! lookups, property type/hint/usage metadata, method argument metadata,
//! signal class-level registration, and instantiation — matches expected
//! Godot runtime behavior.
//! Acceptance: measurable reflection parity tests cover core runtime classes.

use std::sync::Mutex;

use gdcore::math::Vector2;
use gdobject::class_db::{
    class_exists, class_has_method, class_has_property, class_has_signal, clear_for_testing,
    get_class_info, get_class_list, get_inheritors_list, get_method_info, get_method_list,
    get_parent_class, get_property_info, get_property_list, get_signal_info, get_signal_list,
    inheritance_chain, instantiate, is_parent_class, property_get_revert, register_class,
    ArgumentInfo, ClassRegistration, MethodInfo, PropertyInfo, SignalInfo,
};
use gdobject::object::{GenericObject, GodotObject, ObjectBase};
use gdvariant::Variant;

static TEST_LOCK: Mutex<()> = Mutex::new(());

fn setup() -> std::sync::MutexGuard<'static, ()> {
    let guard = TEST_LOCK.lock().expect("test lock poisoned");
    clear_for_testing();
    guard
}

/// Register a standard hierarchy: Object → Node → Node2D → Sprite2D
/// with realistic properties and methods.
fn register_standard_hierarchy() {
    register_class(ClassRegistration::new("Object"));
    register_class(
        ClassRegistration::new("Node")
            .parent("Object")
            .property(PropertyInfo::new("name", Variant::String(String::new())))
            .property(PropertyInfo::new("process_mode", Variant::Int(0)))
            .method(MethodInfo::new("_ready", 0))
            .method(MethodInfo::new("_process", 1))
            .method(MethodInfo::new("_enter_tree", 0))
            .method(MethodInfo::new("_exit_tree", 0))
            .method(MethodInfo::new("add_child", 1))
            .method(MethodInfo::new("remove_child", 1))
            .method(MethodInfo::new("get_node", 1))
            .method(MethodInfo::new("queue_free", 0)),
    );
    register_class(
        ClassRegistration::new("Node2D")
            .parent("Node")
            .property(PropertyInfo::new("position", Variant::Vector2(Vector2::ZERO)))
            .property(PropertyInfo::new("rotation", Variant::Float(0.0)))
            .property(PropertyInfo::new("scale", Variant::Vector2(Vector2::ONE)))
            .property(PropertyInfo::new("visible", Variant::Bool(true)))
            .method(MethodInfo::new("translate", 1))
            .method(MethodInfo::new("rotate", 1))
            .method(MethodInfo::new("look_at", 1)),
    );
    register_class(
        ClassRegistration::new("Sprite2D")
            .parent("Node2D")
            .property(PropertyInfo::new("texture", Variant::Nil))
            .property(PropertyInfo::new("centered", Variant::Bool(true)))
            .property(PropertyInfo::new("flip_h", Variant::Bool(false)))
            .property(PropertyInfo::new("flip_v", Variant::Bool(false)))
            .method(MethodInfo::new("set_texture", 1))
            .method(MethodInfo::new("get_rect", 0)),
    );
}

// ===========================================================================
// 1. get_property_list own_only=true returns only class-level properties
// ===========================================================================

#[test]
fn property_list_own_only_returns_class_properties() {
    let _g = setup();
    register_standard_hierarchy();

    let node2d_props = get_property_list("Node2D", true);
    let names: Vec<&str> = node2d_props.iter().map(|p| p.name.as_str()).collect();

    assert!(names.contains(&"position"));
    assert!(names.contains(&"rotation"));
    assert!(names.contains(&"scale"));
    assert!(names.contains(&"visible"));
    // Should NOT contain inherited properties.
    assert!(!names.contains(&"name"), "own_only should exclude parent props");
    assert!(!names.contains(&"process_mode"));
}

// ===========================================================================
// 2. get_property_list own_only=false includes inherited properties
// ===========================================================================

#[test]
fn property_list_inherited_includes_all_ancestors() {
    let _g = setup();
    register_standard_hierarchy();

    let all_props = get_property_list("Sprite2D", false);
    let names: Vec<&str> = all_props.iter().map(|p| p.name.as_str()).collect();

    // Sprite2D own props.
    assert!(names.contains(&"texture"));
    assert!(names.contains(&"centered"));
    // Node2D inherited props.
    assert!(names.contains(&"position"));
    assert!(names.contains(&"rotation"));
    // Node inherited props.
    assert!(names.contains(&"name"));
    assert!(names.contains(&"process_mode"));
}

// ===========================================================================
// 3. get_method_list own_only=true returns only class methods
// ===========================================================================

#[test]
fn method_list_own_only_returns_class_methods() {
    let _g = setup();
    register_standard_hierarchy();

    let node2d_methods = get_method_list("Node2D", true);
    let names: Vec<&str> = node2d_methods.iter().map(|m| m.name.as_str()).collect();

    assert!(names.contains(&"translate"));
    assert!(names.contains(&"rotate"));
    assert!(names.contains(&"look_at"));
    assert!(!names.contains(&"_ready"), "own_only should exclude parent methods");
    assert!(!names.contains(&"add_child"));
}

// ===========================================================================
// 4. get_method_list own_only=false includes all ancestor methods
// ===========================================================================

#[test]
fn method_list_inherited_includes_all_ancestors() {
    let _g = setup();
    register_standard_hierarchy();

    let all_methods = get_method_list("Sprite2D", false);
    let names: Vec<&str> = all_methods.iter().map(|m| m.name.as_str()).collect();

    // Sprite2D own.
    assert!(names.contains(&"set_texture"));
    assert!(names.contains(&"get_rect"));
    // Node2D inherited.
    assert!(names.contains(&"translate"));
    // Node inherited.
    assert!(names.contains(&"_ready"));
    assert!(names.contains(&"add_child"));
}

// ===========================================================================
// 5. get_property_list for unregistered class returns empty
// ===========================================================================

#[test]
fn property_list_unregistered_class_returns_empty() {
    let _g = setup();
    let props = get_property_list("NonExistentClass", false);
    assert!(props.is_empty());
}

// ===========================================================================
// 6. get_method_list for unregistered class returns empty
// ===========================================================================

#[test]
fn method_list_unregistered_class_returns_empty() {
    let _g = setup();
    let methods = get_method_list("NonExistentClass", false);
    assert!(methods.is_empty());
}

// ===========================================================================
// 7. Instantiation applies inherited default properties
// ===========================================================================

#[test]
fn instantiate_applies_full_inheritance_defaults() {
    let _g = setup();
    register_standard_hierarchy();

    let obj = instantiate("Sprite2D").expect("Sprite2D should instantiate");

    // Sprite2D own defaults.
    assert_eq!(obj.get_property("centered"), Variant::Bool(true));
    assert_eq!(obj.get_property("flip_h"), Variant::Bool(false));
    assert_eq!(obj.get_property("texture"), Variant::Nil);

    // Node2D inherited defaults.
    assert_eq!(obj.get_property("position"), Variant::Vector2(Vector2::ZERO));
    assert_eq!(obj.get_property("rotation"), Variant::Float(0.0));
    assert_eq!(obj.get_property("visible"), Variant::Bool(true));

    // Node inherited defaults.
    assert_eq!(obj.get_property("name"), Variant::String(String::new()));
    assert_eq!(obj.get_property("process_mode"), Variant::Int(0));
}

// ===========================================================================
// 8. Derived default overrides base default
// ===========================================================================

#[test]
fn derived_default_overrides_base_default() {
    let _g = setup();

    register_class(ClassRegistration::new("Object"));
    register_class(
        ClassRegistration::new("Base")
            .parent("Object")
            .property(PropertyInfo::new("hp", Variant::Int(100))),
    );
    register_class(
        ClassRegistration::new("Derived")
            .parent("Base")
            .property(PropertyInfo::new("hp", Variant::Int(200))),
    );

    let obj = instantiate("Derived").unwrap();
    assert_eq!(
        obj.get_property("hp"),
        Variant::Int(200),
        "derived default must override base default"
    );
}

// ===========================================================================
// 9. ObjectBase.is_class walks inheritance chain
// ===========================================================================

#[test]
fn is_class_walks_inheritance() {
    let _g = setup();
    register_standard_hierarchy();

    let base = ObjectBase::new("Sprite2D");

    assert!(base.is_class("Sprite2D"));
    assert!(base.is_class("Node2D"));
    assert!(base.is_class("Node"));
    assert!(base.is_class("Object"));
    assert!(!base.is_class("Label"));
}

// ===========================================================================
// 10. ObjectBase.has_method walks inheritance chain
// ===========================================================================

#[test]
fn has_method_walks_inheritance() {
    let _g = setup();
    register_standard_hierarchy();

    let base = ObjectBase::new("Sprite2D");

    // Own method.
    assert!(base.has_method("set_texture"));
    // Inherited from Node2D.
    assert!(base.has_method("translate"));
    // Inherited from Node.
    assert!(base.has_method("_ready"));
    assert!(base.has_method("add_child"));
    // Not registered anywhere.
    assert!(!base.has_method("nonexistent_method"));
}

// ===========================================================================
// 11. Meta properties are separate from regular properties
// ===========================================================================

#[test]
fn meta_properties_separate_namespace() {
    let mut base = ObjectBase::new("Node");

    // Set both a regular property and a meta property with the same key.
    base.set_property("tag", Variant::String("regular".into()));
    base.set_meta("tag", Variant::String("meta".into()));

    // They should be independent.
    assert_eq!(
        base.get_property("tag"),
        Variant::String("regular".into())
    );
    assert_eq!(base.get_meta("tag"), Variant::String("meta".into()));

    // has_property and has_meta don't cross namespaces.
    base.remove_property("tag");
    assert!(!base.has_property("tag"));
    assert!(base.has_meta("tag"), "meta should survive property removal");
}

// ===========================================================================
// 12. Meta property CRUD operations
// ===========================================================================

#[test]
fn meta_property_crud() {
    let mut base = ObjectBase::new("Node");

    // Create.
    assert!(!base.has_meta("editor_hint"));
    base.set_meta("editor_hint", Variant::Bool(true));
    assert!(base.has_meta("editor_hint"));

    // Read.
    assert_eq!(base.get_meta("editor_hint"), Variant::Bool(true));

    // Update.
    let old = base.set_meta("editor_hint", Variant::Bool(false));
    assert_eq!(old, Variant::Bool(true));
    assert_eq!(base.get_meta("editor_hint"), Variant::Bool(false));

    // Delete.
    let removed = base.remove_meta("editor_hint");
    assert_eq!(removed, Variant::Bool(false));
    assert!(!base.has_meta("editor_hint"));
    assert_eq!(base.get_meta("editor_hint"), Variant::Nil);
}

// ===========================================================================
// 13. get_meta_list enumerates all meta keys
// ===========================================================================

#[test]
fn get_meta_list_enumerates_keys() {
    let mut base = ObjectBase::new("Node");
    base.set_meta("alpha", Variant::Int(1));
    base.set_meta("beta", Variant::Int(2));
    base.set_meta("gamma", Variant::Int(3));

    let mut list = base.get_meta_list();
    list.sort();
    assert_eq!(list, vec!["alpha", "beta", "gamma"]);
}

// ===========================================================================
// 14. Property count parity: own vs inherited
// ===========================================================================

#[test]
fn property_count_own_vs_inherited() {
    let _g = setup();
    register_standard_hierarchy();

    let own = get_property_list("Sprite2D", true);
    let all = get_property_list("Sprite2D", false);

    assert_eq!(own.len(), 4, "Sprite2D has 4 own properties");
    // 4 (Sprite2D) + 4 (Node2D) + 2 (Node) + 0 (Object) = 10
    assert_eq!(all.len(), 10, "Sprite2D inherits 10 total properties");
}

// ===========================================================================
// 15. Method count parity: own vs inherited
// ===========================================================================

#[test]
fn method_count_own_vs_inherited() {
    let _g = setup();
    register_standard_hierarchy();

    let own = get_method_list("Sprite2D", true);
    let all = get_method_list("Sprite2D", false);

    assert_eq!(own.len(), 2, "Sprite2D has 2 own methods");
    // 2 (Sprite2D) + 3 (Node2D) + 8 (Node) + 0 (Object) = 13
    assert_eq!(all.len(), 13, "Sprite2D inherits 13 total methods");
}

// ===========================================================================
// 16. Property default values preserved through get_property_list
// ===========================================================================

#[test]
fn property_defaults_preserved_in_list() {
    let _g = setup();
    register_standard_hierarchy();

    let props = get_property_list("Node2D", true);
    let pos = props.iter().find(|p| p.name == "position").unwrap();
    assert_eq!(pos.default_value, Variant::Vector2(Vector2::ZERO));

    let vis = props.iter().find(|p| p.name == "visible").unwrap();
    assert_eq!(vis.default_value, Variant::Bool(true));
}

// ===========================================================================
// 17. Method argument count preserved through get_method_list
// ===========================================================================

#[test]
fn method_arg_count_preserved_in_list() {
    let _g = setup();
    register_standard_hierarchy();

    let methods = get_method_list("Node", true);
    let ready = methods.iter().find(|m| m.name == "_ready").unwrap();
    assert_eq!(ready.argument_count, 0);

    let process = methods.iter().find(|m| m.name == "_process").unwrap();
    assert_eq!(process.argument_count, 1);

    let add_child = methods.iter().find(|m| m.name == "add_child").unwrap();
    assert_eq!(add_child.argument_count, 1);
}

// ===========================================================================
// 18. class_has_method matches get_method_list results
// ===========================================================================

#[test]
fn class_has_method_consistent_with_method_list() {
    let _g = setup();
    register_standard_hierarchy();

    let all_methods = get_method_list("Sprite2D", false);
    for method in &all_methods {
        assert!(
            class_has_method("Sprite2D", &method.name),
            "class_has_method should return true for '{}'",
            method.name
        );
    }

    // And a non-existent method should return false.
    assert!(!class_has_method("Sprite2D", "nonexistent"));
}

// ===========================================================================
// 19. Inheritance chain reflects correct order
// ===========================================================================

#[test]
fn inheritance_chain_correct_order() {
    let _g = setup();
    register_standard_hierarchy();

    let chain = inheritance_chain("Sprite2D");
    assert_eq!(chain, vec!["Sprite2D", "Node2D", "Node", "Object"]);
}

// ===========================================================================
// 20. Object root (no parent) has empty parent_class
// ===========================================================================

#[test]
fn object_root_has_empty_parent() {
    let _g = setup();
    register_standard_hierarchy();

    let info = get_class_info("Object").unwrap();
    assert!(info.parent_class.is_empty());
    assert_eq!(inheritance_chain("Object"), vec!["Object"]);
}

// ===========================================================================
// 21. Property set/get round-trip on instantiated object
// ===========================================================================

#[test]
fn instantiated_object_property_roundtrip() {
    let _g = setup();
    register_standard_hierarchy();

    let mut obj = instantiate("Node2D").unwrap();

    // Verify default.
    assert_eq!(obj.get_property("position"), Variant::Vector2(Vector2::ZERO));

    // Set and verify.
    let new_pos = Vector2::new(100.0, 200.0);
    obj.set_property("position", Variant::Vector2(new_pos));
    assert_eq!(obj.get_property("position"), Variant::Vector2(new_pos));

    // Other properties unaffected.
    assert_eq!(obj.get_property("rotation"), Variant::Float(0.0));
}

// ===========================================================================
// 22. Unique instance IDs per instantiation
// ===========================================================================

#[test]
fn instantiated_objects_have_unique_ids() {
    let _g = setup();
    register_standard_hierarchy();

    let a = instantiate("Node2D").unwrap();
    let b = instantiate("Node2D").unwrap();
    let c = instantiate("Sprite2D").unwrap();

    assert_ne!(a.get_instance_id(), b.get_instance_id());
    assert_ne!(a.get_instance_id(), c.get_instance_id());
    assert_ne!(b.get_instance_id(), c.get_instance_id());
}

// ===========================================================================
// 23. class_exists for all registered classes
// ===========================================================================

#[test]
fn class_exists_for_all_hierarchy() {
    let _g = setup();
    register_standard_hierarchy();

    assert!(class_exists("Object"));
    assert!(class_exists("Node"));
    assert!(class_exists("Node2D"));
    assert!(class_exists("Sprite2D"));
    assert!(!class_exists("UnknownClass"));
}

// ===========================================================================
// 24. is_parent_class transitivity
// ===========================================================================

#[test]
fn is_parent_class_transitive() {
    let _g = setup();
    register_standard_hierarchy();

    // Direct parent.
    assert!(is_parent_class("Sprite2D", "Node2D"));
    // Grandparent.
    assert!(is_parent_class("Sprite2D", "Node"));
    // Root.
    assert!(is_parent_class("Sprite2D", "Object"));
    // Self.
    assert!(is_parent_class("Sprite2D", "Sprite2D"));
    // Not ancestor.
    assert!(!is_parent_class("Node", "Sprite2D"));
    assert!(!is_parent_class("Object", "Node2D"));
}

// ===========================================================================
// 25. Notification recording on GenericObject
// ===========================================================================

#[test]
fn notification_recording_on_generic_object() {
    use gdobject::notification::{NOTIFICATION_ENTER_TREE, NOTIFICATION_READY};

    let mut obj = GenericObject::new("Node");
    assert!(obj.base.notification_log().is_empty());

    obj.notification(NOTIFICATION_ENTER_TREE);
    obj.notification(NOTIFICATION_READY);

    let log = obj.base.notification_log();
    assert_eq!(log.len(), 2);
    assert_eq!(log[0], NOTIFICATION_ENTER_TREE);
    assert_eq!(log[1], NOTIFICATION_READY);
}

// ===========================================================================
// 26. ObjectBase.remove_property returns old value or Nil
// ===========================================================================

#[test]
fn remove_property_returns_old_value() {
    let mut base = ObjectBase::new("Node");
    base.set_property("hp", Variant::Int(100));

    let removed = base.remove_property("hp");
    assert_eq!(removed, Variant::Int(100));
    assert!(!base.has_property("hp"));

    // Removing again returns Nil.
    let removed_again = base.remove_property("hp");
    assert_eq!(removed_again, Variant::Nil);
}

// ===========================================================================
// 27. Property enumeration consistent with has_property
// ===========================================================================

#[test]
fn property_names_consistent_with_has_property() {
    let mut base = ObjectBase::new("Node");
    base.set_property("x", Variant::Int(1));
    base.set_property("y", Variant::Int(2));
    base.set_property("z", Variant::Int(3));

    let names = base.property_names();
    assert_eq!(names.len(), 3);
    for name in &names {
        assert!(base.has_property(name));
    }

    assert!(!base.has_property("w"));
}

// ===========================================================================
// 28. Deep inheritance: 5+ levels with cumulative properties and methods
// ===========================================================================

#[test]
fn deep_inheritance_cumulative_reflection() {
    let _g = setup();

    register_class(ClassRegistration::new("Object"));
    register_class(
        ClassRegistration::new("Node")
            .parent("Object")
            .property(PropertyInfo::new("name", Variant::String(String::new())))
            .method(MethodInfo::new("_ready", 0)),
    );
    register_class(
        ClassRegistration::new("CanvasItem")
            .parent("Node")
            .property(PropertyInfo::new("modulate", Variant::Nil))
            .method(MethodInfo::new("update", 0)),
    );
    register_class(
        ClassRegistration::new("Node2D")
            .parent("CanvasItem")
            .property(PropertyInfo::new("position", Variant::Vector2(Vector2::ZERO)))
            .method(MethodInfo::new("translate", 1)),
    );
    register_class(
        ClassRegistration::new("Sprite2D")
            .parent("Node2D")
            .property(PropertyInfo::new("texture", Variant::Nil))
            .method(MethodInfo::new("get_rect", 0)),
    );
    register_class(
        ClassRegistration::new("AnimatedSprite2D")
            .parent("Sprite2D")
            .property(PropertyInfo::new("frame", Variant::Int(0)))
            .method(MethodInfo::new("play", 1)),
    );

    // Chain should be 6 deep.
    let chain = inheritance_chain("AnimatedSprite2D");
    assert_eq!(chain.len(), 6);

    // All properties accumulated.
    let all_props = get_property_list("AnimatedSprite2D", false);
    let prop_names: Vec<&str> = all_props.iter().map(|p| p.name.as_str()).collect();
    assert!(prop_names.contains(&"frame"));
    assert!(prop_names.contains(&"texture"));
    assert!(prop_names.contains(&"position"));
    assert!(prop_names.contains(&"modulate"));
    assert!(prop_names.contains(&"name"));
    assert_eq!(all_props.len(), 5);

    // All methods accumulated.
    let all_methods = get_method_list("AnimatedSprite2D", false);
    let method_names: Vec<&str> = all_methods.iter().map(|m| m.name.as_str()).collect();
    assert!(method_names.contains(&"play"));
    assert!(method_names.contains(&"get_rect"));
    assert!(method_names.contains(&"translate"));
    assert!(method_names.contains(&"update"));
    assert!(method_names.contains(&"_ready"));
    assert_eq!(all_methods.len(), 5);

    // Own-only at the leaf.
    assert_eq!(get_property_list("AnimatedSprite2D", true).len(), 1);
    assert_eq!(get_method_list("AnimatedSprite2D", true).len(), 1);
}

// ===========================================================================
// 29. ClassInfo methods and properties match get_method_list/get_property_list
// ===========================================================================

#[test]
fn class_info_matches_list_functions() {
    let _g = setup();
    register_standard_hierarchy();

    let info = get_class_info("Node2D").unwrap();

    let prop_list = get_property_list("Node2D", true);
    assert_eq!(info.properties.len(), prop_list.len());
    for (a, b) in info.properties.iter().zip(prop_list.iter()) {
        assert_eq!(a.name, b.name);
    }

    let method_list = get_method_list("Node2D", true);
    assert_eq!(info.methods.len(), method_list.len());
    for (a, b) in info.methods.iter().zip(method_list.iter()) {
        assert_eq!(a.name, b.name);
    }
}

// ===========================================================================
// 30. Signals on ObjectBase: add, has, enumerate
// ===========================================================================

#[test]
fn signals_add_has_enumerate() {
    let mut base = ObjectBase::new("Node");

    assert!(!base.has_signal("pressed"));
    assert!(base.signals().signal_names().is_empty());

    base.signals_mut().add_signal("pressed");
    base.signals_mut().add_signal("toggled");

    assert!(base.has_signal("pressed"));
    assert!(base.has_signal("toggled"));
    assert!(!base.has_signal("clicked"));

    let mut names = base.signals().signal_names();
    names.sort();
    assert_eq!(names, vec!["pressed", "toggled"]);
}

// ===========================================================================
// pat-unv6: Property type/hint/usage metadata
// ===========================================================================

/// Register a hierarchy with full metadata matching Godot oracle probe output.
fn register_hierarchy_with_metadata() {
    register_class(ClassRegistration::new("Object"));
    register_class(
        ClassRegistration::new("Node")
            .parent("Object")
            .property(
                PropertyInfo::new("name", Variant::String(String::new()))
                    .with_type(21) // StringName
                    .with_usage(4102),
            )
            .property(
                PropertyInfo::new("process_mode", Variant::Int(0))
                    .with_type(2) // Int
                    .with_hint(2, "Inherit,Pausable,WhenPaused,Always,Disabled")
                    .with_usage(4102),
            )
            .method(
                MethodInfo::new("_ready", 0)
                    .with_return_type(0), // void
            )
            .method(
                MethodInfo::new("_process", 1)
                    .with_args(vec![ArgumentInfo::new("delta", 3)]) // Float
                    .with_return_type(0),
            )
            .method(
                MethodInfo::new("add_child", 1)
                    .with_args(vec![ArgumentInfo::new("node", 24).with_class("Node")])
                    .with_return_type(0),
            )
            .method(
                MethodInfo::new("get_child", 1)
                    .with_args(vec![ArgumentInfo::new("idx", 2)]) // Int
                    .with_return_type(24), // Object
            )
            .signal(SignalInfo::new("ready"))
            .signal(SignalInfo::new("tree_entered"))
            .signal(SignalInfo::new("tree_exited")),
    );
    register_class(
        ClassRegistration::new("Node2D")
            .parent("Node")
            .property(
                PropertyInfo::new("position", Variant::Vector2(Vector2::ZERO))
                    .with_type(5) // Vector2
                    .with_usage(4102),
            )
            .property(
                PropertyInfo::new("rotation", Variant::Float(0.0))
                    .with_type(3) // Float
                    .with_usage(4102),
            )
            .method(
                MethodInfo::new("translate", 1)
                    .with_args(vec![ArgumentInfo::new("offset", 5)]) // Vector2
                    .with_return_type(0),
            )
            .signal(SignalInfo::new("visibility_changed")),
    );
}

// ===========================================================================
// 31. PropertyInfo stores type metadata
// ===========================================================================

#[test]
fn property_info_type_metadata() {
    let _g = setup();
    register_hierarchy_with_metadata();

    let props = get_property_list("Node", true);
    let name_prop = props.iter().find(|p| p.name == "name").unwrap();
    assert_eq!(name_prop.property_type, 21, "name should be StringName type");
    assert_eq!(name_prop.usage, 4102);
    assert_eq!(name_prop.hint, 0);

    let mode_prop = props.iter().find(|p| p.name == "process_mode").unwrap();
    assert_eq!(mode_prop.property_type, 2, "process_mode should be Int type");
    assert_eq!(mode_prop.hint, 2, "process_mode should have Enum hint");
    assert!(mode_prop.hint_string.contains("Inherit"));
    assert!(mode_prop.hint_string.contains("Disabled"));
    assert_eq!(mode_prop.usage, 4102);
}

// ===========================================================================
// 32. PropertyInfo type metadata preserved through inheritance
// ===========================================================================

#[test]
fn property_type_metadata_inherited() {
    let _g = setup();
    register_hierarchy_with_metadata();

    let all_props = get_property_list("Node2D", false);
    // Inherited from Node
    let name_prop = all_props.iter().find(|p| p.name == "name").unwrap();
    assert_eq!(name_prop.property_type, 21);

    // Own property
    let pos_prop = all_props.iter().find(|p| p.name == "position").unwrap();
    assert_eq!(pos_prop.property_type, 5, "position should be Vector2 type");
}

// ===========================================================================
// 33. MethodInfo stores argument metadata
// ===========================================================================

#[test]
fn method_info_argument_metadata() {
    let _g = setup();
    register_hierarchy_with_metadata();

    let methods = get_method_list("Node", true);

    // _ready has no args
    let ready = methods.iter().find(|m| m.name == "_ready").unwrap();
    assert!(ready.arguments.is_empty());
    assert_eq!(ready.return_type, 0);

    // _process has one Float arg named "delta"
    let process = methods.iter().find(|m| m.name == "_process").unwrap();
    assert_eq!(process.arguments.len(), 1);
    assert_eq!(process.arguments[0].name, "delta");
    assert_eq!(process.arguments[0].arg_type, 3);
    assert!(process.arguments[0].class_name.is_empty());
    assert_eq!(process.return_type, 0);

    // add_child has one Object arg with class_name "Node"
    let add_child = methods.iter().find(|m| m.name == "add_child").unwrap();
    assert_eq!(add_child.arguments.len(), 1);
    assert_eq!(add_child.arguments[0].name, "node");
    assert_eq!(add_child.arguments[0].arg_type, 24);
    assert_eq!(add_child.arguments[0].class_name, "Node");
}

// ===========================================================================
// 34. MethodInfo return type
// ===========================================================================

#[test]
fn method_info_return_type() {
    let _g = setup();
    register_hierarchy_with_metadata();

    let methods = get_method_list("Node", true);

    // get_child returns Object (type 24)
    let get_child = methods.iter().find(|m| m.name == "get_child").unwrap();
    assert_eq!(get_child.return_type, 24);

    // _ready returns void (type 0)
    let ready = methods.iter().find(|m| m.name == "_ready").unwrap();
    assert_eq!(ready.return_type, 0);
}

// ===========================================================================
// 35. Method argument metadata preserved through inheritance
// ===========================================================================

#[test]
fn method_arg_metadata_inherited() {
    let _g = setup();
    register_hierarchy_with_metadata();

    let all_methods = get_method_list("Node2D", false);

    // Inherited from Node
    let process = all_methods.iter().find(|m| m.name == "_process").unwrap();
    assert_eq!(process.arguments.len(), 1);
    assert_eq!(process.arguments[0].name, "delta");

    // Own method
    let translate = all_methods.iter().find(|m| m.name == "translate").unwrap();
    assert_eq!(translate.arguments.len(), 1);
    assert_eq!(translate.arguments[0].name, "offset");
    assert_eq!(translate.arguments[0].arg_type, 5);
}

// ===========================================================================
// 36. SignalInfo registration in ClassDB
// ===========================================================================

#[test]
fn signal_info_registered_in_classdb() {
    let _g = setup();
    register_hierarchy_with_metadata();

    let info = get_class_info("Node").unwrap();
    assert_eq!(info.signals.len(), 3);

    let sig_names: Vec<&str> = info.signals.iter().map(|s| s.name.as_str()).collect();
    assert!(sig_names.contains(&"ready"));
    assert!(sig_names.contains(&"tree_entered"));
    assert!(sig_names.contains(&"tree_exited"));
}

// ===========================================================================
// 37. class_has_signal walks inheritance chain
// ===========================================================================

#[test]
fn class_has_signal_walks_inheritance() {
    let _g = setup();
    register_hierarchy_with_metadata();

    // Own signals
    assert!(class_has_signal("Node", "ready"));
    assert!(class_has_signal("Node", "tree_entered"));

    // Inherited signals
    assert!(class_has_signal("Node2D", "ready"));
    assert!(class_has_signal("Node2D", "tree_exited"));

    // Own signal on Node2D
    assert!(class_has_signal("Node2D", "visibility_changed"));

    // Non-existent
    assert!(!class_has_signal("Node", "visibility_changed"));
    assert!(!class_has_signal("Node", "nonexistent"));
}

// ===========================================================================
// 38. get_signal_list own_only=true
// ===========================================================================

#[test]
fn signal_list_own_only() {
    let _g = setup();
    register_hierarchy_with_metadata();

    let node_signals = get_signal_list("Node", true);
    assert_eq!(node_signals.len(), 3);

    let node2d_signals = get_signal_list("Node2D", true);
    assert_eq!(node2d_signals.len(), 1);
    assert_eq!(node2d_signals[0].name, "visibility_changed");
}

// ===========================================================================
// 39. get_signal_list own_only=false includes inherited
// ===========================================================================

#[test]
fn signal_list_inherited() {
    let _g = setup();
    register_hierarchy_with_metadata();

    let all_signals = get_signal_list("Node2D", false);
    // 1 (Node2D own) + 3 (Node) = 4
    assert_eq!(all_signals.len(), 4);

    let names: Vec<&str> = all_signals.iter().map(|s| s.name.as_str()).collect();
    assert!(names.contains(&"visibility_changed"));
    assert!(names.contains(&"ready"));
    assert!(names.contains(&"tree_entered"));
    assert!(names.contains(&"tree_exited"));
}

// ===========================================================================
// 40. get_signal_list for unregistered class returns empty
// ===========================================================================

#[test]
fn signal_list_unregistered_class_returns_empty() {
    let _g = setup();
    let signals = get_signal_list("NonExistent", false);
    assert!(signals.is_empty());
}

// ===========================================================================
// 41. SignalInfo with argument metadata
// ===========================================================================

#[test]
fn signal_info_with_args() {
    let _g = setup();

    register_class(ClassRegistration::new("Object"));
    register_class(
        ClassRegistration::new("Area2D")
            .parent("Object")
            .signal(
                SignalInfo::new("body_entered").with_args(vec![
                    ArgumentInfo::new("body", 24).with_class("Node2D"),
                ]),
            )
            .signal(SignalInfo::new("area_entered").with_args(vec![
                ArgumentInfo::new("area", 24).with_class("Area2D"),
            ])),
    );

    let signals = get_signal_list("Area2D", true);
    let body_entered = signals.iter().find(|s| s.name == "body_entered").unwrap();
    assert_eq!(body_entered.arguments.len(), 1);
    assert_eq!(body_entered.arguments[0].name, "body");
    assert_eq!(body_entered.arguments[0].arg_type, 24);
    assert_eq!(body_entered.arguments[0].class_name, "Node2D");

    let area_entered = signals.iter().find(|s| s.name == "area_entered").unwrap();
    assert_eq!(area_entered.arguments.len(), 1);
    assert_eq!(area_entered.arguments[0].class_name, "Area2D");
}

// ===========================================================================
// 42. ClassInfo.signals field accessible and matches get_signal_list
// ===========================================================================

#[test]
fn class_info_signals_match_signal_list() {
    let _g = setup();
    register_hierarchy_with_metadata();

    let info = get_class_info("Node").unwrap();
    let list = get_signal_list("Node", true);

    assert_eq!(info.signals.len(), list.len());
    for (a, b) in info.signals.iter().zip(list.iter()) {
        assert_eq!(a.name, b.name);
    }
}

// ===========================================================================
// 43. class_has_signal consistent with get_signal_list
// ===========================================================================

#[test]
fn class_has_signal_consistent_with_signal_list() {
    let _g = setup();
    register_hierarchy_with_metadata();

    let all_signals = get_signal_list("Node2D", false);
    for sig in &all_signals {
        assert!(
            class_has_signal("Node2D", &sig.name),
            "class_has_signal should return true for '{}'",
            sig.name
        );
    }
    assert!(!class_has_signal("Node2D", "nonexistent_signal"));
}

// ===========================================================================
// 44. PropertyInfo builder preserves all fields
// ===========================================================================

#[test]
fn property_info_builder_roundtrip() {
    let prop = PropertyInfo::new("hp", Variant::Int(100))
        .with_type(2)
        .with_hint(1, "0,1000,1")
        .with_usage(4102);

    assert_eq!(prop.name, "hp");
    assert_eq!(prop.default_value, Variant::Int(100));
    assert_eq!(prop.property_type, 2);
    assert_eq!(prop.hint, 1);
    assert_eq!(prop.hint_string, "0,1000,1");
    assert_eq!(prop.usage, 4102);
}

// ===========================================================================
// 45. MethodInfo with_args updates argument_count
// ===========================================================================

#[test]
fn method_info_with_args_updates_count() {
    let method = MethodInfo::new("test", 0).with_args(vec![
        ArgumentInfo::new("a", 2),
        ArgumentInfo::new("b", 3),
        ArgumentInfo::new("c", 4),
    ]);

    assert_eq!(method.argument_count, 3);
    assert_eq!(method.arguments.len(), 3);
    assert_eq!(method.arguments[0].name, "a");
    assert_eq!(method.arguments[2].name, "c");
}

// ===========================================================================
// 46. PropertyInfo defaults (no metadata) are zero/empty
// ===========================================================================

#[test]
fn property_info_defaults_zero() {
    let prop = PropertyInfo::new("x", Variant::Float(1.0));
    assert_eq!(prop.property_type, 0);
    assert_eq!(prop.hint, 0);
    assert_eq!(prop.hint_string, "");
    assert_eq!(prop.usage, 0);
}

// ===========================================================================
// 47. MethodInfo defaults (no metadata) are zero/empty
// ===========================================================================

#[test]
fn method_info_defaults_zero() {
    let method = MethodInfo::new("foo", 2);
    assert!(method.arguments.is_empty());
    assert_eq!(method.return_type, 0);
    assert_eq!(method.argument_count, 2);
}

// ===========================================================================
// pat-unv6: New ClassDB reflection APIs
// ===========================================================================

// ===========================================================================
// 48. get_class_list returns all registered classes
// ===========================================================================

#[test]
fn get_class_list_returns_all_registered() {
    let _g = setup();
    register_standard_hierarchy();

    let mut list = get_class_list();
    list.sort();
    assert_eq!(list, vec!["Node", "Node2D", "Object", "Sprite2D"]);
}

// ===========================================================================
// 49. get_class_list is empty when nothing registered
// ===========================================================================

#[test]
fn get_class_list_empty_when_cleared() {
    let _g = setup();
    assert!(get_class_list().is_empty());
}

// ===========================================================================
// 50. get_parent_class returns direct parent
// ===========================================================================

#[test]
fn get_parent_class_returns_direct_parent() {
    let _g = setup();
    register_standard_hierarchy();

    assert_eq!(get_parent_class("Sprite2D"), Some("Node2D".to_string()));
    assert_eq!(get_parent_class("Node2D"), Some("Node".to_string()));
    assert_eq!(get_parent_class("Node"), Some("Object".to_string()));
    // Root has empty parent.
    assert_eq!(get_parent_class("Object"), Some(String::new()));
    // Unregistered returns None.
    assert_eq!(get_parent_class("NotRegistered"), None);
}

// ===========================================================================
// 51. get_inheritors_list finds all subclasses
// ===========================================================================

#[test]
fn get_inheritors_list_finds_subclasses() {
    let _g = setup();
    register_standard_hierarchy();

    let mut inheritors = get_inheritors_list("Object");
    inheritors.sort();
    assert_eq!(inheritors, vec!["Node", "Node2D", "Sprite2D"]);

    let mut node_inheritors = get_inheritors_list("Node");
    node_inheritors.sort();
    assert_eq!(node_inheritors, vec!["Node2D", "Sprite2D"]);

    let mut node2d_inheritors = get_inheritors_list("Node2D");
    node2d_inheritors.sort();
    assert_eq!(node2d_inheritors, vec!["Sprite2D"]);

    // Leaf has no inheritors.
    assert!(get_inheritors_list("Sprite2D").is_empty());

    // Unregistered has no inheritors.
    assert!(get_inheritors_list("Unknown").is_empty());
}

// ===========================================================================
// 52. class_has_property walks inheritance chain
// ===========================================================================

#[test]
fn class_has_property_walks_inheritance() {
    let _g = setup();
    register_standard_hierarchy();

    // Own property.
    assert!(class_has_property("Node2D", "position"));
    // Inherited property.
    assert!(class_has_property("Sprite2D", "position"));
    assert!(class_has_property("Sprite2D", "name"));
    // Non-existent.
    assert!(!class_has_property("Node", "position"));
    assert!(!class_has_property("Node", "nonexistent"));
}

// ===========================================================================
// 53. property_get_revert returns ClassDB default
// ===========================================================================

#[test]
fn property_get_revert_returns_classdb_default() {
    let _g = setup();
    register_standard_hierarchy();

    // Own property default.
    assert_eq!(
        property_get_revert("Node2D", "position"),
        Some(Variant::Vector2(Vector2::ZERO))
    );
    assert_eq!(
        property_get_revert("Node2D", "visible"),
        Some(Variant::Bool(true))
    );

    // Inherited property default.
    assert_eq!(
        property_get_revert("Sprite2D", "name"),
        Some(Variant::String(String::new()))
    );

    // Non-existent property returns None.
    assert_eq!(property_get_revert("Node", "nonexistent"), None);
}

// ===========================================================================
// 54. property_get_revert on ObjectBase
// ===========================================================================

#[test]
fn object_base_property_get_revert() {
    let _g = setup();
    register_standard_hierarchy();

    let mut base = ObjectBase::new("Node2D");
    // Override the default.
    base.set_property("position", Variant::Vector2(Vector2::new(99.0, 99.0)));

    // The revert value should be the ClassDB default, not the current value.
    assert_eq!(
        base.property_get_revert("position"),
        Variant::Vector2(Vector2::ZERO)
    );

    // Unregistered property returns Nil.
    assert_eq!(base.property_get_revert("nonexistent"), Variant::Nil);
}

// ===========================================================================
// 55. get_property_info returns single property metadata
// ===========================================================================

#[test]
fn get_property_info_returns_metadata() {
    let _g = setup();
    register_hierarchy_with_metadata();

    let prop = get_property_info("Node", "process_mode").unwrap();
    assert_eq!(prop.name, "process_mode");
    assert_eq!(prop.property_type, 2);
    assert_eq!(prop.hint, 2);
    assert!(prop.hint_string.contains("Inherit"));

    // Inherited lookup.
    let pos = get_property_info("Node2D", "name").unwrap();
    assert_eq!(pos.name, "name");
    assert_eq!(pos.property_type, 21);

    // Non-existent returns None.
    assert!(get_property_info("Node", "nonexistent").is_none());
}

// ===========================================================================
// 56. get_method_info returns single method metadata
// ===========================================================================

#[test]
fn get_method_info_returns_metadata() {
    let _g = setup();
    register_hierarchy_with_metadata();

    let method = get_method_info("Node", "_process").unwrap();
    assert_eq!(method.name, "_process");
    assert_eq!(method.arguments.len(), 1);
    assert_eq!(method.arguments[0].name, "delta");
    assert_eq!(method.arguments[0].arg_type, 3);

    // Inherited lookup.
    let ready = get_method_info("Node2D", "_ready").unwrap();
    assert_eq!(ready.name, "_ready");
    assert_eq!(ready.return_type, 0);

    // Non-existent returns None.
    assert!(get_method_info("Node", "nonexistent").is_none());
}

// ===========================================================================
// 57. get_signal_info returns single signal metadata
// ===========================================================================

#[test]
fn get_signal_info_returns_metadata() {
    let _g = setup();
    register_hierarchy_with_metadata();

    let sig = get_signal_info("Node", "ready").unwrap();
    assert_eq!(sig.name, "ready");
    assert!(sig.arguments.is_empty());

    // Inherited lookup.
    let sig = get_signal_info("Node2D", "tree_entered").unwrap();
    assert_eq!(sig.name, "tree_entered");

    // Own signal.
    let sig = get_signal_info("Node2D", "visibility_changed").unwrap();
    assert_eq!(sig.name, "visibility_changed");

    // Non-existent returns None.
    assert!(get_signal_info("Node", "nonexistent").is_none());
}

// ===========================================================================
// 58. ObjectBase.has_signal checks both instance and ClassDB
// ===========================================================================

#[test]
fn has_signal_checks_instance_and_classdb() {
    let _g = setup();
    register_hierarchy_with_metadata();

    let mut base = ObjectBase::new("Node");

    // ClassDB signal.
    assert!(base.has_signal("ready"));
    assert!(base.has_signal("tree_entered"));

    // Instance user signal.
    base.signals_mut().add_signal("custom_signal");
    assert!(base.has_signal("custom_signal"));

    // has_user_signal only checks instance.
    assert!(base.has_user_signal("custom_signal"));
    assert!(!base.has_user_signal("ready"));

    // Non-existent.
    assert!(!base.has_signal("nonexistent"));
}

// ===========================================================================
// 59. ObjectBase.get_parent_class via ClassDB
// ===========================================================================

#[test]
fn object_base_get_parent_class() {
    let _g = setup();
    register_standard_hierarchy();

    let base = ObjectBase::new("Sprite2D");
    assert_eq!(base.get_parent_class(), "Node2D");

    let root = ObjectBase::new("Object");
    assert_eq!(root.get_parent_class(), "");

    let unregistered = ObjectBase::new("Unknown");
    assert_eq!(unregistered.get_parent_class(), "");
}

// ===========================================================================
// 60. Meta property get_meta_default with fallback
// ===========================================================================

#[test]
fn get_meta_default_with_fallback() {
    let mut base = ObjectBase::new("Node");
    base.set_meta("tag", Variant::String("hello".into()));

    // Existing key ignores default.
    assert_eq!(
        base.get_meta_default("tag", Variant::String("fallback".into())),
        Variant::String("hello".into())
    );

    // Missing key returns default.
    assert_eq!(
        base.get_meta_default("missing", Variant::Int(42)),
        Variant::Int(42)
    );
}

// ===========================================================================
// 61. GodotObject trait reflection: is_class
// ===========================================================================

#[test]
fn godot_object_trait_is_class() {
    let _g = setup();
    register_standard_hierarchy();

    let obj = instantiate("Sprite2D").unwrap();

    assert!(obj.is_class("Sprite2D"));
    assert!(obj.is_class("Node2D"));
    assert!(obj.is_class("Node"));
    assert!(obj.is_class("Object"));
    assert!(!obj.is_class("Label"));
}

// ===========================================================================
// 62. GodotObject trait reflection: has_method
// ===========================================================================

#[test]
fn godot_object_trait_has_method() {
    let _g = setup();
    register_standard_hierarchy();

    let obj = instantiate("Sprite2D").unwrap();

    assert!(obj.has_method("set_texture"));
    assert!(obj.has_method("translate")); // inherited from Node2D
    assert!(obj.has_method("_ready")); // inherited from Node
    assert!(!obj.has_method("nonexistent"));
}

// ===========================================================================
// 63. GodotObject trait reflection: has_signal
// ===========================================================================

#[test]
fn godot_object_trait_has_signal() {
    let _g = setup();
    register_hierarchy_with_metadata();

    let obj = instantiate("Node2D").unwrap();

    assert!(obj.has_signal("visibility_changed")); // own
    assert!(obj.has_signal("ready")); // inherited
    assert!(!obj.has_signal("nonexistent"));
}

// ===========================================================================
// 64. GodotObject trait reflection: get_property_list
// ===========================================================================

#[test]
fn godot_object_trait_get_property_list() {
    let _g = setup();
    register_standard_hierarchy();

    let obj = instantiate("Sprite2D").unwrap();
    let props = obj.get_property_list();

    assert!(props.contains(&"texture".to_string()));
    assert!(props.contains(&"position".to_string()));
    assert!(props.contains(&"name".to_string()));
    // 4 (Sprite2D) + 4 (Node2D) + 2 (Node) + 0 (Object) = 10
    assert_eq!(props.len(), 10);
}

// ===========================================================================
// 65. GodotObject trait reflection: get_method_list
// ===========================================================================

#[test]
fn godot_object_trait_get_method_list() {
    let _g = setup();
    register_standard_hierarchy();

    let obj = instantiate("Sprite2D").unwrap();
    let methods = obj.get_method_list();

    assert!(methods.contains(&"set_texture".to_string()));
    assert!(methods.contains(&"translate".to_string()));
    assert!(methods.contains(&"_ready".to_string()));
}

// ===========================================================================
// 66. ObjectBase.get_property_list_from_classdb
// ===========================================================================

#[test]
fn object_base_get_property_list_from_classdb() {
    let _g = setup();
    register_standard_hierarchy();

    let base = ObjectBase::new("Node2D");
    let props = base.get_property_list_from_classdb();

    // Node2D own (4) + Node inherited (2) = 6
    assert_eq!(props.len(), 6);
    assert!(props.contains(&"position".to_string()));
    assert!(props.contains(&"name".to_string()));
}

// ===========================================================================
// 67. MethodInfo flags and virtual/const markers
// ===========================================================================

#[test]
fn method_info_flags_and_markers() {
    let normal = MethodInfo::new("foo", 0);
    assert_eq!(normal.flags, 1); // METHOD_FLAG_NORMAL
    assert!(!normal.is_virtual);
    assert!(!normal.is_const);
    assert!(!normal.is_vararg);

    let virtual_method = MethodInfo::new("_process", 1).virtual_method();
    assert!(virtual_method.is_virtual);
    assert_eq!(virtual_method.flags & 32, 32);

    let const_method = MethodInfo::new("get_name", 0).const_method();
    assert!(const_method.is_const);
    assert_eq!(const_method.flags & 4, 4);

    // with_flags sets all at once
    let flagged = MethodInfo::new("complex", 0).with_flags(1 | 4 | 32); // NORMAL | CONST | VIRTUAL
    assert!(flagged.is_virtual);
    assert!(flagged.is_const);
    assert!(!flagged.is_vararg);
}

// ===========================================================================
// 68. GodotObject trait: get_signal_list
// ===========================================================================

#[test]
fn godot_object_trait_get_signal_list() {
    let _g = setup();
    register_hierarchy_with_metadata();

    let obj = instantiate("Node2D").unwrap();
    let signals = obj.get_signal_list();

    assert!(signals.contains(&"visibility_changed".to_string()));
    assert!(signals.contains(&"ready".to_string()));
    assert!(signals.contains(&"tree_entered".to_string()));
    assert!(signals.contains(&"tree_exited".to_string()));
    assert_eq!(signals.len(), 4);
}

// ===========================================================================
// 69. GodotObject trait: property_can_revert
// ===========================================================================

#[test]
fn godot_object_trait_property_can_revert() {
    let _g = setup();
    register_standard_hierarchy();

    let obj = instantiate("Node2D").unwrap();

    assert!(obj.property_can_revert("position"));
    assert!(obj.property_can_revert("name")); // inherited
    assert!(!obj.property_can_revert("nonexistent"));
}

// ===========================================================================
// 70. GodotObject trait: property_get_revert
// ===========================================================================

#[test]
fn godot_object_trait_property_get_revert() {
    let _g = setup();
    register_standard_hierarchy();

    let mut obj = instantiate("Node2D").unwrap();
    obj.set_property("position", Variant::Vector2(Vector2::new(50.0, 50.0)));

    // Revert should return ClassDB default, not current value.
    assert_eq!(
        obj.property_get_revert("position"),
        Variant::Vector2(Vector2::ZERO)
    );
    assert_eq!(obj.property_get_revert("nonexistent"), Variant::Nil);
}

// ===========================================================================
// 71. GodotObject trait: has_class_property
// ===========================================================================

#[test]
fn godot_object_trait_has_class_property() {
    let _g = setup();
    register_standard_hierarchy();

    let obj = instantiate("Sprite2D").unwrap();

    assert!(obj.has_class_property("texture")); // own
    assert!(obj.has_class_property("position")); // inherited from Node2D
    assert!(obj.has_class_property("name")); // inherited from Node
    assert!(!obj.has_class_property("nonexistent"));
}

// ===========================================================================
// 72. ObjectBase.property_can_revert
// ===========================================================================

#[test]
fn object_base_property_can_revert() {
    let _g = setup();
    register_standard_hierarchy();

    let base = ObjectBase::new("Node2D");
    assert!(base.property_can_revert("position"));
    assert!(base.property_can_revert("visible"));
    assert!(!base.property_can_revert("nonexistent"));
}

// ===========================================================================
// 73. ObjectBase.has_class_property
// ===========================================================================

#[test]
fn object_base_has_class_property() {
    let _g = setup();
    register_standard_hierarchy();

    let base = ObjectBase::new("Sprite2D");
    assert!(base.has_class_property("texture"));
    assert!(base.has_class_property("position")); // inherited
    assert!(!base.has_class_property("nonexistent"));
}

// ===========================================================================
// 74. ObjectBase.get_signal_list_from_classdb
// ===========================================================================

#[test]
fn object_base_get_signal_list_from_classdb() {
    let _g = setup();
    register_hierarchy_with_metadata();

    let base = ObjectBase::new("Node2D");
    let signals = base.get_signal_list_from_classdb();

    assert!(signals.contains(&"visibility_changed".to_string()));
    assert!(signals.contains(&"ready".to_string()));
    assert_eq!(signals.len(), 4);
}
