//! pat-s3rrv.258 — the `VisualShaderNodeUInt*` family in ClassDB.
//!
//! Godot ground truth: `upstream/godot/doc/classes/VisualShaderNodeUInt*.xml`.
//! The four unsigned nodes mirror the signed ones, which were already
//! registered, so this was a straight omission rather than a design decision.
//!
//! ## Why every enum value is asserted individually
//!
//! ⚠️ **The unsigned enums are NOT the signed enums renamed.** `IntFunc` is
//! `FUNC_ABS 0, FUNC_NEGATE 1, FUNC_SIGN 2, FUNC_BITWISE_NOT 3, FUNC_MAX 4`;
//! `UIntFunc` is `FUNC_NEGATE 0, FUNC_BITWISE_NOT 1, FUNC_MAX 2` — the unsigned
//! type has no ABS or SIGN, so **every shared name sits at a different index**.
//! Copying the sibling's constant list, which is the obvious way to write this
//! registration, produces four names that all exist and all have the wrong
//! value. A membership-only test would pass on exactly that bug, so the values
//! are pinned and the divergence has its own test below.
//!
//! The same trap sits in `UIntOp`: `OP_MAX` is a real operator (5), and the
//! terminator is `OP_ENUM_SIZE` (12), not `OP_MAX`.

use gdobject::class_db::{
    class_exists, class_get_integer_constant, class_has_enum, get_property_list,
    is_parent_class, register_3d_classes,
};
use gdvariant::Variant;

/// The default value ClassDB reports for `class_name.property`.
fn default_of(class_name: &str, property: &str) -> Option<Variant> {
    get_property_list(class_name)
        .into_iter()
        .find(|p| p.name == property)
        .map(|p| p.default_value)
}

/// Asserts an enum constant exists with exactly Godot's value.
fn assert_constant(class_name: &str, constant: &str, expected: i64) {
    match class_get_integer_constant(class_name, constant) {
        Some(actual) => assert_eq!(
            actual, expected,
            "{class_name}.{constant} must be {expected} per the doc XML, not {actual}"
        ),
        None => panic!("{class_name} must declare {constant}"),
    }
}

/// **The acceptance.**
#[test]
fn visualshadernode_uint_family_registered() {
    register_3d_classes();

    // -- existence and parents ----------------------------------------------
    //
    // The parents differ across the family: the constant and parameter nodes
    // hang off their own abstract bases, while Func and Op sit directly on
    // VisualShaderNode. Giving all four the same parent would still satisfy
    // `class_exists`.
    for (class_name, parent) in [
        ("VisualShaderNodeUIntConstant", "VisualShaderNodeConstant"),
        ("VisualShaderNodeUIntParameter", "VisualShaderNodeParameter"),
        ("VisualShaderNodeUIntFunc", "VisualShaderNode"),
        ("VisualShaderNodeUIntOp", "VisualShaderNode"),
    ] {
        assert!(class_exists(class_name), "{class_name} must be registered");
        assert!(
            is_parent_class(class_name, parent),
            "{class_name} inherits {parent} per its doc XML"
        );
        // The chain must reach VisualShaderNode itself, not stop at an
        // intermediate — that is what puts the node in a graph at all.
        assert!(
            is_parent_class(class_name, "VisualShaderNode"),
            "{class_name} must reach VisualShaderNode through its chain"
        );
    }

    // -- properties and their defaults ---------------------------------------
    assert_eq!(
        default_of("VisualShaderNodeUIntConstant", "constant"),
        Some(Variant::Int(0)),
        "UIntConstant.constant is an int defaulting to 0"
    );
    assert_eq!(
        default_of("VisualShaderNodeUIntParameter", "default_value"),
        Some(Variant::Int(0)),
        "UIntParameter.default_value is an int defaulting to 0"
    );
    assert_eq!(
        default_of("VisualShaderNodeUIntParameter", "default_value_enabled"),
        Some(Variant::Bool(false)),
        "UIntParameter.default_value_enabled is a bool defaulting to false — \
         the default doubles as the property TYPE here, so an Int(0) would \
         report the wrong type as well as the wrong value"
    );
    assert_eq!(
        default_of("VisualShaderNodeUIntFunc", "function"),
        Some(Variant::Int(0)),
        "UIntFunc.function defaults to 0"
    );
    assert_eq!(
        default_of("VisualShaderNodeUIntOp", "operator"),
        Some(Variant::Int(0)),
        "UIntOp.operator defaults to 0"
    );

    // -- UIntFunc.Function ----------------------------------------------------
    assert!(class_has_enum("VisualShaderNodeUIntFunc", "Function"));
    for (name, value) in [
        ("FUNC_NEGATE", 0),
        ("FUNC_BITWISE_NOT", 1),
        ("FUNC_MAX", 2),
    ] {
        assert_constant("VisualShaderNodeUIntFunc", name, value);
    }

    // -- UIntOp.Operator, all thirteen ---------------------------------------
    assert!(class_has_enum("VisualShaderNodeUIntOp", "Operator"));
    for (name, value) in [
        ("OP_ADD", 0),
        ("OP_SUB", 1),
        ("OP_MUL", 2),
        ("OP_DIV", 3),
        ("OP_MOD", 4),
        ("OP_MAX", 5),
        ("OP_MIN", 6),
        ("OP_BITWISE_AND", 7),
        ("OP_BITWISE_OR", 8),
        ("OP_BITWISE_XOR", 9),
        ("OP_BITWISE_LEFT_SHIFT", 10),
        ("OP_BITWISE_RIGHT_SHIFT", 11),
        ("OP_ENUM_SIZE", 12),
    ] {
        assert_constant("VisualShaderNodeUIntOp", name, value);
    }

    // ⚠️ The terminator is OP_ENUM_SIZE, and OP_MAX is a real operator at 5 —
    // the pair a reader is most likely to conflate, since in most Godot enums
    // the `_MAX` name IS the terminator.
    assert_ne!(
        class_get_integer_constant("VisualShaderNodeUIntOp", "OP_MAX"),
        class_get_integer_constant("VisualShaderNodeUIntOp", "OP_ENUM_SIZE"),
        "OP_MAX is an operator, not the terminator; OP_ENUM_SIZE terminates"
    );
}

/// The unsigned enums must NOT match their signed twins.
///
/// This is the test that fails if the registration was written by copying the
/// `Int*` sibling — which is the natural way to write it, and produces constants
/// that all exist under the right names with wrong values.
#[test]
fn visualshadernode_uint_family_registered_diverges_from_the_signed_twins() {
    register_3d_classes();

    // Same name, different index, because the unsigned type has no ABS/SIGN.
    assert_eq!(
        class_get_integer_constant("VisualShaderNodeUIntFunc", "FUNC_NEGATE"),
        Some(0),
        "unsigned FUNC_NEGATE is 0"
    );
    assert_eq!(
        class_get_integer_constant("VisualShaderNodeIntFunc", "FUNC_NEGATE"),
        Some(1),
        "signed FUNC_NEGATE is 1 — if these two are ever equal, one list was \
         copied from the other"
    );
    assert_eq!(
        class_get_integer_constant("VisualShaderNodeUIntFunc", "FUNC_MAX"),
        Some(2)
    );
    assert_eq!(
        class_get_integer_constant("VisualShaderNodeIntFunc", "FUNC_MAX"),
        Some(4)
    );

    // The unsigned function enum has no ABS or SIGN at all.
    for absent in ["FUNC_ABS", "FUNC_SIGN"] {
        assert_eq!(
            class_get_integer_constant("VisualShaderNodeUIntFunc", absent),
            None,
            "{absent} is a signed-only function and must not appear on UIntFunc"
        );
    }

    // The signed twins must survive this bead untouched.
    for class_name in [
        "VisualShaderNodeIntConstant",
        "VisualShaderNodeIntFunc",
        "VisualShaderNodeIntOp",
        "VisualShaderNodeIntParameter",
    ] {
        assert!(
            class_exists(class_name),
            "{class_name} predates this bead and must survive it"
        );
    }
}
