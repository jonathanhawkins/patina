//! pat-s3rrv.57: ClassDB must know which of a class's methods are virtual.
//!
//! `MethodInfo` grew `is_virtual` and an argument list earlier in this bead, but
//! a field no registration uses is inert. Node declared 0 of its 12 virtuals and
//! Control 0 of its 11 — so Godot's own Override-Virtual-Method dialog, which
//! filters `class_get_method_list` on METHOD_FLAG_VIRTUAL, would come up empty
//! for the base class of the entire scene tree.
//!
//! Ground truth is `upstream/godot/doc/classes/Node.xml` and `Control.xml`
//! (`qualifiers="virtual"` / `"virtual const"`), not memory.

use gdobject::class_db::{get_class_info, get_method_list, register_all_classes, MethodInfo};
use gdvariant::Variant;

/// Godot's `MethodFlags` (`core/object/object.h:224-233`).
const METHOD_FLAG_NORMAL: i64 = 1;
const METHOD_FLAG_CONST: i64 = 4;
const METHOD_FLAG_VIRTUAL: i64 = 8;

/// The methods a class declares ITSELF.
///
/// ⚠️ `get_method_list` walks the inheritance chain, so it cannot answer "how
/// many does Control declare" — Control's list carries Node's twelve too.
fn own_methods(class: &str) -> Vec<MethodInfo> {
    register_all_classes();
    get_class_info(class)
        .unwrap_or_else(|| panic!("{class} is registered"))
        .methods
}

fn find(methods: &[MethodInfo], name: &str) -> MethodInfo {
    methods
        .iter()
        .find(|m| m.name == name)
        .unwrap_or_else(|| panic!("{name} is declared"))
        .clone()
}

#[test]
fn node_declares_all_twelve_of_its_virtuals() {
    let methods = own_methods("Node");
    let virtuals: Vec<&str> = methods
        .iter()
        .filter(|m| m.is_virtual)
        .map(|m| m.name.as_str())
        .collect();

    assert_eq!(
        virtuals.len(),
        12,
        "Node.xml declares exactly 12 virtuals; got {virtuals:?}"
    );
    for expected in [
        "_ready",
        "_enter_tree",
        "_exit_tree",
        "_process",
        "_physics_process",
        "_input",
        "_shortcut_input",
        "_unhandled_input",
        "_unhandled_key_input",
        "_get_configuration_warnings",
        "_get_accessibility_configuration_warnings",
        "_get_focused_accessibility_element",
    ] {
        assert!(virtuals.contains(&expected), "{expected} must be declared");
    }
}

/// **The discriminating pair.** A concrete method next to a virtual one — the
/// distinction ClassDB could not make at all before, and the one an override
/// dialog filters on.
#[test]
fn virtual_and_concrete_report_different_flags() {
    let methods = own_methods("Node");

    assert_eq!(
        find(&methods, "_process").godot_flags(),
        METHOD_FLAG_NORMAL | METHOD_FLAG_VIRTUAL,
        "a virtual reports NORMAL|VIRTUAL (9)"
    );
    assert_eq!(
        find(&methods, "add_child").godot_flags(),
        METHOD_FLAG_NORMAL,
        "a concrete method reports NORMAL (1) — asserting only the virtual side \
         would pass against a table that flags everything"
    );
    // `virtual const` in Godot is BOTH flags, not one instead of the other.
    assert_eq!(
        find(&methods, "_get_configuration_warnings").godot_flags(),
        METHOD_FLAG_NORMAL | METHOD_FLAG_CONST | METHOD_FLAG_VIRTUAL,
        "a `virtual const` reports 13"
    );
}

#[test]
fn virtual_arguments_carry_their_names_and_types() {
    let methods = own_methods("Node");

    let process = find(&methods, "_process");
    assert_eq!(process.arguments.len(), 1);
    assert_eq!(process.arguments[0].name, "delta");
    assert_eq!(
        process.arguments[0].default_value,
        Variant::Float(0.0),
        "`delta` is a float in Godot, and the default doubles as the type"
    );

    // An object-typed parameter: the type is OBJECT and the class name rides in
    // `hint_string`, which is where `class_get_method_list` reads `class_name`.
    let input = find(&methods, "_input");
    assert_eq!(input.arguments.len(), 1);
    assert_eq!(input.arguments[0].name, "event");
    assert_eq!(input.arguments[0].hint_string, "InputEvent");
    assert!(matches!(
        input.arguments[0].default_value,
        Variant::ObjectId(_)
    ));
}

#[test]
fn control_declares_all_eleven_of_its_virtuals() {
    let methods = own_methods("Control");
    let virtuals: Vec<&str> = methods
        .iter()
        .filter(|m| m.is_virtual)
        .map(|m| m.name.as_str())
        .collect();

    assert_eq!(
        virtuals.len(),
        11,
        "Control.xml declares exactly 11 virtuals; got {virtuals:?}"
    );
    assert!(
        virtuals.contains(&"_gui_input"),
        "the one every UI script overrides"
    );

    // `data` is untyped in Godot — TYPE_NIL is the "any Variant" answer, not a
    // missing type, so the argument must still be present and named.
    let can_drop = find(&methods, "_can_drop_data");
    assert_eq!(can_drop.argument_count, 2);
    assert_eq!(can_drop.arguments[1].name, "data");
    assert_eq!(can_drop.arguments[1].default_value, Variant::Nil);
}

/// What an override dialog actually asks: standing on a Control, which methods
/// may this script override? The answer has to include what it inherits.
#[test]
fn an_inherited_virtual_is_reported_as_virtual() {
    register_all_classes();
    let inherited = get_method_list("Control");

    let ready = inherited
        .iter()
        .find(|m| m.name == "_ready")
        .expect("Control inherits Node's _ready");
    assert!(
        ready.is_virtual,
        "the flag must survive the walk up the chain — an override dialog on a \
         Control offers Node's virtuals too"
    );
}

/// The 12 non-void virtuals declare what they answer (pat-s3rrv.57).
///
/// ⚠️ The void control at the end is what makes this meaningful: `None` means
/// "not declared", and since a void method is *also* serialised as NIL, a table
/// that simply declared nothing would satisfy every positive assertion here if
/// they were phrased as "is not Some(wrong type)".
#[test]
fn the_non_void_virtuals_declare_their_return_type() {
    let node = own_methods("Node");
    let control = own_methods("Control");

    let ret = |methods: &[MethodInfo], name: &str| {
        find(methods, name)
            .return_value
            .unwrap_or_else(|| panic!("`{name}` must declare a return"))
    };

    assert_eq!(
        ret(&node, "_get_configuration_warnings").default_value,
        Variant::PackedStringArray(Vec::new()),
        "the editor reads the warnings a node reports about itself"
    );
    assert_eq!(
        ret(&control, "_get_minimum_size").default_value,
        Variant::Vector2(gdcore::math::Vector2::ZERO),
        "the whole container layout pass depends on this answering a Vector2"
    );
    assert_eq!(
        ret(&control, "_has_point").default_value,
        Variant::Bool(false)
    );

    // An object return and a typed-array return both name their class in
    // `hint_string`, which is where `class_get_method_list` reads `class_name`.
    let tooltip = ret(&control, "_make_custom_tooltip");
    assert!(matches!(tooltip.default_value, Variant::ObjectId(_)));
    assert_eq!(tooltip.hint_string, "Object");

    let parser = ret(&control, "_structured_text_parser");
    assert_eq!(parser.default_value, Variant::Array(Default::default()));
    assert_eq!(
        parser.hint_string, "Vector3i",
        "`Vector3i[]` without its element type is an untyped Array"
    );

    // The control: 11 of the 23 virtuals ARE void in Godot, and those must stay
    // undeclared rather than being given a NIL return that means the same thing
    // by accident.
    assert!(
        find(&node, "_ready").return_value.is_none(),
        "_ready is void; declaring a NIL return would be indistinguishable but          is not what the XML says"
    );
    assert!(find(&control, "_drop_data").return_value.is_none());
}
