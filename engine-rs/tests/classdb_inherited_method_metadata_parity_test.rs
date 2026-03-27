//! pat-t1li: Match ClassDB method metadata for inherited node APIs.
//!
//! Godot's ClassDB.class_get_method_list() returns MethodInfo dictionaries
//! with: name, args, return, flags, is_virtual, is_const, is_vararg.
//!
//! Acceptance: inherited methods carry correct metadata (flags, return type,
//! virtual/const/vararg markers) through the ClassDB inheritance chain,
//! matching Godot's behavior where child classes see parent method metadata
//! unchanged.

use std::sync::Mutex;

use gdobject::class_db::{
    class_has_method, clear_for_testing, get_method_list, register_class, ClassRegistration,
    MethodFlags, MethodInfo, PropertyInfo,
};
use gdvariant::Variant;

static TEST_LOCK: Mutex<()> = Mutex::new(());

fn setup() -> std::sync::MutexGuard<'static, ()> {
    let guard = TEST_LOCK.lock().expect("test lock poisoned");
    clear_for_testing();
    guard
}

/// Registers a minimal hierarchy with rich method metadata:
///   Node (virtual lifecycle + normal methods)
///   ├── Node2D (normal transform methods, const getters)
///   │   └── Sprite2D (own methods)
///   └── Control (own methods)
///       └── Label (own methods)
fn register_hierarchy_with_metadata() {
    // Node: mix of virtual and normal methods
    register_class(
        ClassRegistration::new("Node")
            .property(PropertyInfo::new("name", Variant::String(String::new())))
            .method(MethodInfo::new("_ready", 0).with_virtual())
            .method(MethodInfo::new("_process", 1).with_virtual())
            .method(MethodInfo::new("_physics_process", 1).with_virtual())
            .method(MethodInfo::new("_enter_tree", 0).with_virtual())
            .method(MethodInfo::new("_exit_tree", 0).with_virtual())
            .method(MethodInfo::new("_input", 1).with_virtual())
            .method(MethodInfo::new("add_child", 1))
            .method(MethodInfo::new("remove_child", 1))
            .method(MethodInfo::new("get_child_count", 0).with_const().with_return_type("int"))
            .method(MethodInfo::new("get_children", 0).with_const().with_return_type("Array"))
            .method(MethodInfo::new("get_parent", 0).with_const().with_return_type("Node"))
            .method(MethodInfo::new("get_path", 0).with_const().with_return_type("NodePath"))
            .method(MethodInfo::new("is_inside_tree", 0).with_const().with_return_type("bool"))
            .method(MethodInfo::new("queue_free", 0))
            .method(MethodInfo::new("reparent", 1))
            .method(MethodInfo::new("propagate_call", 1).with_vararg()),
    );

    // Node2D: transform methods
    register_class(
        ClassRegistration::new("Node2D")
            .parent("Node")
            .property(PropertyInfo::new(
                "position",
                Variant::Vector2(gdcore::math::Vector2::ZERO),
            ))
            .method(MethodInfo::new("get_position", 0).with_const().with_return_type("Vector2"))
            .method(MethodInfo::new("set_position", 1))
            .method(MethodInfo::new("get_rotation", 0).with_const().with_return_type("float"))
            .method(MethodInfo::new("set_rotation", 1))
            .method(MethodInfo::new("get_global_position", 0).with_const().with_return_type("Vector2"))
            .method(MethodInfo::new("to_local", 1).with_const().with_return_type("Vector2"))
            .method(MethodInfo::new("to_global", 1).with_const().with_return_type("Vector2")),
    );

    // Sprite2D
    register_class(
        ClassRegistration::new("Sprite2D")
            .parent("Node2D")
            .property(PropertyInfo::new("texture", Variant::Nil))
            .method(MethodInfo::new("get_texture", 0).with_const().with_return_type("Texture2D"))
            .method(MethodInfo::new("set_texture", 1))
            .method(MethodInfo::new("get_rect", 0).with_const().with_return_type("Rect2"))
            .method(MethodInfo::new("is_flipped_h", 0).with_const().with_return_type("bool")),
    );

    // Control
    register_class(
        ClassRegistration::new("Control")
            .parent("Node")
            .property(PropertyInfo::new("visible", Variant::Bool(true)))
            .method(MethodInfo::new("get_minimum_size", 0).with_const().with_return_type("Vector2"))
            .method(MethodInfo::new("grab_focus", 0))
            .method(MethodInfo::new("has_focus", 0).with_const().with_return_type("bool")),
    );

    // Label
    register_class(
        ClassRegistration::new("Label")
            .parent("Control")
            .property(PropertyInfo::new("text", Variant::String(String::new())))
            .method(MethodInfo::new("get_text", 0).with_const().with_return_type("String"))
            .method(MethodInfo::new("set_text", 1)),
    );
}

// ===========================================================================
// 1. MethodInfo default construction preserves backward compatibility
// ===========================================================================

#[test]
fn method_info_defaults_are_backward_compatible() {
    let m = MethodInfo::new("test_method", 2);
    assert_eq!(m.name, "test_method");
    assert_eq!(m.argument_count, 2);
    assert_eq!(m.flags.0, MethodFlags::NORMAL.0);
    assert!(!m.is_virtual);
    assert!(!m.is_const);
    assert!(!m.is_vararg);
    assert_eq!(m.return_type, "void");
}

// ===========================================================================
// 2. Builder methods set correct metadata
// ===========================================================================

#[test]
fn method_info_builders_set_metadata() {
    let m = MethodInfo::new("_ready", 0)
        .with_virtual()
        .with_return_type("void");
    assert!(m.is_virtual);
    assert_eq!(m.flags.0, MethodFlags::VIRTUAL.0);
    assert_eq!(m.return_type, "void");

    let m2 = MethodInfo::new("get_name", 0)
        .with_const()
        .with_return_type("String");
    assert!(m2.is_const);
    assert!(!m2.is_virtual);
    assert_eq!(m2.return_type, "String");

    let m3 = MethodInfo::new("emit_signal", 1).with_vararg();
    assert!(m3.is_vararg);
}

// ===========================================================================
// 3. Inherited virtual methods preserve is_virtual flag
// ===========================================================================

#[test]
fn inherited_virtual_methods_preserve_flag() {
    let _g = setup();
    register_hierarchy_with_metadata();

    // _ready is virtual on Node. Sprite2D inherits it.
    let all_methods = get_method_list("Sprite2D", false);
    let ready = all_methods.iter().find(|m| m.name == "_ready");

    assert!(ready.is_some(), "Sprite2D must inherit _ready");
    let ready = ready.unwrap();
    assert!(
        ready.is_virtual,
        "_ready must be virtual when inherited by Sprite2D"
    );
    assert_eq!(
        ready.flags.0,
        MethodFlags::VIRTUAL.0,
        "_ready flags must be VIRTUAL"
    );
}

// ===========================================================================
// 4. Inherited const methods preserve is_const and return_type
// ===========================================================================

#[test]
fn inherited_const_methods_preserve_metadata() {
    let _g = setup();
    register_hierarchy_with_metadata();

    // get_child_count is const with return "int" on Node.
    let all_methods = get_method_list("Sprite2D", false);
    let gcc = all_methods.iter().find(|m| m.name == "get_child_count");

    assert!(gcc.is_some(), "Sprite2D must inherit get_child_count");
    let gcc = gcc.unwrap();
    assert!(gcc.is_const, "get_child_count must be const");
    assert_eq!(gcc.return_type, "int", "get_child_count return type must be int");
    assert_eq!(gcc.argument_count, 0);
}

// ===========================================================================
// 5. Inherited vararg methods preserve is_vararg flag
// ===========================================================================

#[test]
fn inherited_vararg_methods_preserve_flag() {
    let _g = setup();
    register_hierarchy_with_metadata();

    let all_methods = get_method_list("Sprite2D", false);
    let pc = all_methods.iter().find(|m| m.name == "propagate_call");

    assert!(pc.is_some(), "Sprite2D must inherit propagate_call");
    assert!(
        pc.unwrap().is_vararg,
        "propagate_call must be vararg when inherited"
    );
}

// ===========================================================================
// 6. Deep inheritance chain preserves all metadata
// ===========================================================================

#[test]
fn deep_chain_preserves_metadata_through_three_levels() {
    let _g = setup();
    register_hierarchy_with_metadata();

    // Label -> Control -> Node: _ready (virtual) and is_inside_tree (const, returns bool)
    let label_methods = get_method_list("Label", false);

    let ready = label_methods.iter().find(|m| m.name == "_ready").unwrap();
    assert!(ready.is_virtual, "Label must see _ready as virtual (from Node)");

    let iit = label_methods.iter().find(|m| m.name == "is_inside_tree").unwrap();
    assert!(iit.is_const, "Label must see is_inside_tree as const (from Node)");
    assert_eq!(iit.return_type, "bool");

    let gf = label_methods.iter().find(|m| m.name == "grab_focus").unwrap();
    assert!(!gf.is_const, "grab_focus is not const (from Control)");
    assert!(!gf.is_virtual, "grab_focus is not virtual");
}

// ===========================================================================
// 7. Own-only listing excludes inherited metadata
// ===========================================================================

#[test]
fn own_only_excludes_inherited_methods_with_metadata() {
    let _g = setup();
    register_hierarchy_with_metadata();

    let own = get_method_list("Sprite2D", true);
    let own_names: Vec<&str> = own.iter().map(|m| m.name.as_str()).collect();

    assert!(own_names.contains(&"get_texture"), "Own must include get_texture");
    assert!(!own_names.contains(&"_ready"), "Own must NOT include _ready (from Node)");
    assert!(!own_names.contains(&"get_position"), "Own must NOT include get_position (from Node2D)");
}

// ===========================================================================
// 8. Return type propagation through inheritance
// ===========================================================================

#[test]
fn return_types_propagate_through_inheritance() {
    let _g = setup();
    register_hierarchy_with_metadata();

    let sprite_methods = get_method_list("Sprite2D", false);

    // From Node: get_path returns NodePath
    let gp = sprite_methods.iter().find(|m| m.name == "get_path").unwrap();
    assert_eq!(gp.return_type, "NodePath");

    // From Node2D: get_position returns Vector2
    let gpos = sprite_methods.iter().find(|m| m.name == "get_position").unwrap();
    assert_eq!(gpos.return_type, "Vector2");

    // Own: get_texture returns Texture2D
    let gt = sprite_methods.iter().find(|m| m.name == "get_texture").unwrap();
    assert_eq!(gt.return_type, "Texture2D");

    // Non-returning: add_child returns void
    let ac = sprite_methods.iter().find(|m| m.name == "add_child").unwrap();
    assert_eq!(ac.return_type, "void");
}

// ===========================================================================
// 9. class_has_method works with enriched metadata
// ===========================================================================

#[test]
fn class_has_method_works_with_metadata() {
    let _g = setup();
    register_hierarchy_with_metadata();

    // Virtual method from Node
    assert!(class_has_method("Sprite2D", "_ready"));
    // Const method from Node2D
    assert!(class_has_method("Sprite2D", "get_position"));
    // Own method
    assert!(class_has_method("Sprite2D", "get_texture"));
    // Nonexistent
    assert!(!class_has_method("Sprite2D", "nonexistent"));
}

// ===========================================================================
// 10. Method ordering: base-to-derived in full listing
// ===========================================================================

#[test]
fn inherited_methods_ordered_base_to_derived() {
    let _g = setup();
    register_hierarchy_with_metadata();

    let all = get_method_list("Sprite2D", false);
    let names: Vec<&str> = all.iter().map(|m| m.name.as_str()).collect();

    // Node methods should come before Node2D methods, which come before Sprite2D methods.
    let ready_pos = names.iter().position(|n| *n == "_ready").unwrap();
    let get_pos_pos = names.iter().position(|n| *n == "get_position").unwrap();
    let get_tex_pos = names.iter().position(|n| *n == "get_texture").unwrap();

    assert!(
        ready_pos < get_pos_pos,
        "_ready (Node) must come before get_position (Node2D)"
    );
    assert!(
        get_pos_pos < get_tex_pos,
        "get_position (Node2D) must come before get_texture (Sprite2D)"
    );
}

// ===========================================================================
// 11. Total method count includes all ancestors
// ===========================================================================

#[test]
fn total_method_count_equals_sum_of_ancestors() {
    let _g = setup();
    register_hierarchy_with_metadata();

    let node_own = get_method_list("Node", true).len();
    let node2d_own = get_method_list("Node2D", true).len();
    let sprite2d_own = get_method_list("Sprite2D", true).len();
    let sprite2d_all = get_method_list("Sprite2D", false).len();

    assert_eq!(
        sprite2d_all,
        node_own + node2d_own + sprite2d_own,
        "Sprite2D total = Node + Node2D + Sprite2D own"
    );
}

// ===========================================================================
// 12. MethodFlags constants match Godot values
// ===========================================================================

#[test]
fn method_flags_constants_match_godot() {
    // Godot uses: METHOD_FLAG_NORMAL=1, METHOD_FLAG_VIRTUAL=2, METHOD_FLAG_EDITOR=4
    assert_eq!(MethodFlags::NORMAL.0, 1);
    assert_eq!(MethodFlags::VIRTUAL.0, 2);
    assert_eq!(MethodFlags::EDITOR.0, 4);
}
