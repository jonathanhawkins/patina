//! pat-nzgdu.227 — the VisualShader vector family and `WorldPositionFromDepth`.
//!
//! ⚠️ **Registering this family exposed a shipped bug in the pre-existing
//! `VisualShaderNode` base: its `PortType` enum values were wrong.**
//! `PORT_TYPE_VECTOR_3D` onward were `5/7/8/9/10/12` where Godot has
//! `4/5/6/7/8/9` — the enum is consecutive, with no gaps. A script reading
//! `PORT_TYPE_BOOLEAN` got **8** where Godot says **6**, so every port-type
//! comparison past `VECTOR_2D` silently disagreed with the engine. The base was
//! also missing 8 of its 11 methods and all 4 properties, and had
//! `set_input_port_default_value` at arity 2 where Godot takes 3. All fixed and
//! diffed against the dump.
//!
//! ⚠️ **The bead invents enums for two of its four targets.** It says these
//! "carry an operator/type enum (... VectorLen: length; VectorRefract: refract)".
//! They do not: `VisualShaderNodeVectorOp` is the only node here with an
//! operator, and Len/Refract each *are* the operation — there is nothing to
//! choose. Their one configurable member, `op_type` (2D/3D/4D), is
//! `VisualShaderNodeVectorBase`'s and is inherited.
//!
//! ⚠️ Stale premise (fourth shader bead running): "Patina currently has only a
//! minimal internal 8-op VisualShaderNodeType enum in gdserver2d/material.rs,
//! not the Godot VisualShader\* class family" — **56 `VisualShader*` classes were
//! already registered**, and both "bases" the bead asks for
//! (`VisualShaderNode`, `VisualShaderNodeVectorBase`) plus `VisualShaderNodeVectorOp`
//! itself were among them.
//!
//! **`generate_code()` does not exist anywhere in the workspace**, so the
//! acceptance's "generated shader code for each node matches Godot's expression"
//! has nothing to call. Third bead blocked on it (.225, .226). See the notes.

use gdobject::class_db::{can_instantiate, is_abstract};
use gdobject::GodotObject;
use gdobject::{
    class_exists, class_get_integer_constant, get_method_list, get_property_list, instantiate,
    is_parent_class, register_3d_classes,
};
use gdvariant::Variant;

fn arity(class: &str, method: &str) -> usize {
    get_method_list(class)
        .iter()
        .rev()
        .find(|m| m.name == method)
        .unwrap_or_else(|| panic!("{class} has no method `{method}`"))
        .argument_count
}

fn own_methods(class: &str, base: &str) -> Vec<String> {
    let base_methods: Vec<String> = get_method_list(base).into_iter().map(|m| m.name).collect();
    get_method_list(class)
        .into_iter()
        .map(|m| m.name)
        .filter(|m| !base_methods.contains(m))
        .collect()
}

fn default_of(class: &str, name: &str) -> Variant {
    get_property_list(class)
        .into_iter()
        .rev()
        .find(|p| p.name == name)
        .unwrap_or_else(|| panic!("{class} has no property `{name}`"))
        .default_value
}

/// The acceptance's registration clause: all four with correct parents, each
/// constructible, and the two bases registered.
#[test]
fn visualshadernode_vector_family_registered_and_codegen() {
    register_3d_classes();

    // The bases the bead asks for — both were already registered.
    assert!(class_exists("VisualShaderNode"));
    assert!(class_exists("VisualShaderNodeVectorBase"));
    assert!(is_parent_class(
        "VisualShaderNodeVectorBase",
        "VisualShaderNode"
    ));

    for node in [
        "VisualShaderNodeVectorLen",
        "VisualShaderNodeVectorOp",
        "VisualShaderNodeVectorRefract",
    ] {
        assert!(class_exists(node), "{node} must be registered");
        assert_eq!(
            gdobject::get_class_info(node)
                .expect("registered")
                .parent_class,
            "VisualShaderNodeVectorBase",
            "{node} <- VisualShaderNodeVectorBase"
        );
        assert!(is_parent_class(node, "VisualShaderNode"));
        assert!(is_parent_class(node, "Resource"));
        assert!(!is_abstract(node));
        assert!(can_instantiate(node));
        let obj = instantiate(node).unwrap_or_else(|| panic!("{node} instantiates"));
        assert_eq!(obj.get_class(), node);
    }

    // ⚠️ NOT a vector node, despite arriving in this bead: it reconstructs a
    // world position from DEPTH_TEXTURE and hangs off VisualShaderNode directly.
    assert!(class_exists("VisualShaderNodeWorldPositionFromDepth"));
    assert_eq!(
        gdobject::get_class_info("VisualShaderNodeWorldPositionFromDepth")
            .expect("registered")
            .parent_class,
        "VisualShaderNode",
        "it is a depth node, not a vector node"
    );
    assert!(
        !is_parent_class(
            "VisualShaderNodeWorldPositionFromDepth",
            "VisualShaderNodeVectorBase"
        ),
        "it has no op_type — it is not a VectorBase"
    );
    assert!(can_instantiate("VisualShaderNodeWorldPositionFromDepth"));
}

/// ⚠️ THE SHIPPED BUG. `PortType`'s values were wrong from `VECTOR_3D` onward.
///
/// The enum is consecutive. The old registration had gaps (5, 7, 8, 9, 10, 12),
/// so `PORT_TYPE_BOOLEAN` read 8 where Godot says 6 — every port-type comparison
/// past `VECTOR_2D` silently disagreed with the engine.
#[test]
fn port_type_values_are_consecutive_and_match_godot() {
    register_3d_classes();

    for (name, value) in [
        ("PORT_TYPE_SCALAR", 0),
        ("PORT_TYPE_SCALAR_INT", 1),
        ("PORT_TYPE_SCALAR_UINT", 2),
        ("PORT_TYPE_VECTOR_2D", 3),
        ("PORT_TYPE_VECTOR_3D", 4),
        ("PORT_TYPE_VECTOR_4D", 5),
        ("PORT_TYPE_BOOLEAN", 6),
        ("PORT_TYPE_TRANSFORM", 7),
        ("PORT_TYPE_SAMPLER", 8),
        ("PORT_TYPE_MAX", 9),
    ] {
        assert_eq!(
            class_get_integer_constant("VisualShaderNode", name),
            Some(value),
            "{name} must be {value} — the enum is consecutive, with no gaps"
        );
    }

    // The specific values that were wrong, pinned so a regression is loud.
    assert_ne!(
        class_get_integer_constant("VisualShaderNode", "PORT_TYPE_BOOLEAN"),
        Some(8),
        "8 was the old, wrong value for BOOLEAN"
    );
    assert_ne!(
        class_get_integer_constant("VisualShaderNode", "PORT_TYPE_MAX"),
        Some(12),
        "12 was the old, wrong value for MAX"
    );
}

/// The base's surface, also fixed. ⚠️ `set_input_port_default_value` takes THREE
/// arguments (port, index, value) — it was registered with two.
#[test]
fn the_base_exposes_godots_full_surface() {
    register_3d_classes();

    assert_eq!(
        own_methods("VisualShaderNode", "Resource").len(),
        11,
        "Godot binds 11 methods on VisualShaderNode"
    );
    assert_eq!(
        arity("VisualShaderNode", "set_input_port_default_value"),
        3,
        "(port, index, value) — this was registered with two arguments"
    );
    assert_eq!(arity("VisualShaderNode", "get_default_input_port"), 1);
    assert_eq!(arity("VisualShaderNode", "set_frame"), 1);

    // ⚠️ -1, not 0: port 0 and frame 0 are real, so a 0 default would silently
    // mean "preview port 0" / "linked to frame 0".
    assert_eq!(
        default_of("VisualShaderNode", "output_port_for_preview"),
        Variant::Int(-1),
        "-1 means no preview port"
    );
    assert_eq!(
        default_of("VisualShaderNode", "linked_parent_graph_frame"),
        Variant::Int(-1),
        "-1 means not linked to a frame"
    );
    assert_eq!(
        default_of("VisualShaderNode", "default_input_values"),
        Variant::Array(Default::default())
    );
}

/// ⚠️ The bead invents enums for VectorLen/VectorRefract. They bind nothing.
///
/// `VectorOp` is the only node here with an operator; Len and Refract each ARE
/// the operation. Their one configurable member is the inherited `op_type`.
#[test]
fn len_and_refract_bind_nothing_and_inherit_op_type() {
    register_3d_classes();

    for node in ["VisualShaderNodeVectorLen", "VisualShaderNodeVectorRefract"] {
        assert!(
            own_methods(node, "VisualShaderNodeVectorBase").is_empty(),
            "{node} binds no methods of its own — it IS the operation"
        );
        let base_props: Vec<String> = get_property_list("VisualShaderNodeVectorBase")
            .into_iter()
            .map(|p| p.name)
            .collect();
        let own: Vec<String> = get_property_list(node)
            .into_iter()
            .map(|p| p.name)
            .filter(|p| !base_props.contains(p))
            .collect();
        assert!(
            own.is_empty(),
            "{node} has no enum or property of its own; found {own:?}"
        );

        // ...but it inherits op_type, which IS its configurable member.
        assert_eq!(
            default_of(node, "op_type"),
            Variant::Int(0),
            "{node} inherits op_type = OP_TYPE_VECTOR_2D"
        );
    }

    // Not vacuous: VectorOp — the same family, the same base — DOES bind an
    // operator, so an empty own-set is a fact about Len/Refract.
    assert_eq!(
        own_methods("VisualShaderNodeVectorOp", "VisualShaderNodeVectorBase"),
        vec!["set_operator", "get_operator"],
        "VectorOp binds the accessor pair the others lack"
    );
    assert_eq!(
        class_get_integer_constant("VisualShaderNodeVectorOp", "OP_CROSS"),
        Some(8)
    );
    // ⚠️ 13 values, not the bead's 12 — OP_ENUM_SIZE is the sentinel it omits.
    assert_eq!(
        class_get_integer_constant("VisualShaderNodeVectorOp", "OP_ENUM_SIZE"),
        Some(12),
        "the bead's list stops at OP_STEP; Godot has a 13th sentinel"
    );
}

#[test]
fn registration_is_idempotent() {
    // `class_count()` and property lists cannot catch a duplicate registration
    // (`classes` is name-keyed, so re-registering overwrites). The ClassId is
    // what actually changes.
    let ids = || -> Vec<_> {
        [
            "VisualShaderNodeVectorLen",
            "VisualShaderNodeVectorRefract",
            "VisualShaderNodeWorldPositionFromDepth",
        ]
        .iter()
        .map(|c| gdobject::get_class_info(c).expect("registered").class_id)
        .collect()
    };

    register_3d_classes();
    let first = ids();
    register_3d_classes();
    assert_eq!(
        first,
        ids(),
        "re-registering must not mint new ClassIds — a guard is missing if it does"
    );
}
