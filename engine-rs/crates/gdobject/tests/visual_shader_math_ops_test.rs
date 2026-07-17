//! pat-nzgdu.169 — VisualShader math operator and function node classes.
//!
//! Covers the bead's ClassDB clauses. Every schema and all 171 enum constants
//! were **generated** from Godot's `extension_api.json` rather than typed, then
//! diffed back against it — at this size a single transposed value would
//! silently mis-compile a graph and no reviewer would spot it.
//!
//! The `generate_code()` clause is NOT covered: `gdserver2d::material`'s graph
//! compiler has no operator/function node kinds at all (its `VisualShaderNodeType`
//! is `Output/Float/Vec3/Color/Texture/Add/Multiply/Mix`), and no way to carry a
//! selected operator, so emitting `pow(...)`/`sin(...)` for a chosen enum needs
//! that enum plumbed into a real node type first. See the bead notes.

use gdobject::{
    class_exists, class_get_integer_constant, class_has_enum, get_property_list, is_parent_class,
    register_3d_classes,
};
// `is_abstract` is `pub` but absent from lib.rs's re-exports; reach it directly.
use gdobject::class_db::is_abstract;

fn props(class: &str) -> Vec<String> {
    get_property_list(class)
        .into_iter()
        .map(|p| p.name)
        .collect()
}

fn assert_props(class: &str, want: &[&str]) {
    let have = props(class);
    for w in want {
        assert!(
            have.iter().any(|h| h == w),
            "{class} must expose `{w}`; got {have:?}"
        );
    }
}

fn konst(class: &str, name: &str) -> Option<i64> {
    class_get_integer_constant(class, name)
}

/// The bead's ClassDB acceptance clause, verbatim.
#[test]
fn visual_shader_math_node_parents_match_godot() {
    register_3d_classes();

    for class in [
        "VisualShaderNodeFloatOp",
        "VisualShaderNodeCompare",
        "VisualShaderNodeMix",
    ] {
        assert!(
            is_parent_class(class, "VisualShaderNode"),
            "{class} derives VisualShaderNode directly"
        );
    }
    assert!(
        is_parent_class("VisualShaderNodeFaceForward", "VisualShaderNodeVectorBase"),
        "FaceForward derives the vector base, which carries the vec2/3/4 op_type"
    );
}

/// Regression: `VectorOp`/`VectorFunc` were registered under `VisualShaderNode`,
/// but Godot derives both from `VisualShaderNodeVectorBase` — so they inherit
/// its `op_type`. Only a diff against the real API surfaced this.
#[test]
fn vector_nodes_derive_vector_base_not_visual_shader_node_directly() {
    register_3d_classes();

    for class in [
        "VisualShaderNodeVectorOp",
        "VisualShaderNodeVectorFunc",
        "VisualShaderNodeFaceForward",
    ] {
        assert!(
            is_parent_class(class, "VisualShaderNodeVectorBase"),
            "{class} must derive VisualShaderNodeVectorBase"
        );
        // ...and therefore inherit op_type through it.
        assert_props(class, &["op_type"]);
        // The chain must still reach the root.
        assert!(is_parent_class(class, "VisualShaderNode"));
        assert!(is_parent_class(class, "Object"));
    }
}

#[test]
fn vector_base_is_abstract_and_carries_the_dimension_op_type() {
    register_3d_classes();
    assert!(class_exists("VisualShaderNodeVectorBase"));
    assert!(is_abstract("VisualShaderNodeVectorBase"), "it is a base, not a node");
    assert_props("VisualShaderNodeVectorBase", &["op_type"]);
    assert!(class_has_enum("VisualShaderNodeVectorBase", "OpType"));
    for (name, value) in [
        ("OP_TYPE_VECTOR_2D", 0),
        ("OP_TYPE_VECTOR_3D", 1),
        ("OP_TYPE_VECTOR_4D", 2),
        ("OP_TYPE_MAX", 3),
    ] {
        assert_eq!(konst("VisualShaderNodeVectorBase", name), Some(value));
    }
}

/// The two selectors the acceptance names: FloatOp(op=POW) and FloatFunc(func=SIN).
#[test]
fn float_op_and_func_expose_the_selectors_the_acceptance_uses() {
    register_3d_classes();
    assert_props("VisualShaderNodeFloatOp", &["operator"]);
    assert_props("VisualShaderNodeFloatFunc", &["function"]);
    assert_eq!(konst("VisualShaderNodeFloatOp", "OP_POW"), Some(5));
    assert_eq!(konst("VisualShaderNodeFloatFunc", "FUNC_SIN"), Some(0));
    // Spot-check values that are easy to transpose across a 33-value enum.
    assert_eq!(konst("VisualShaderNodeFloatFunc", "FUNC_ABS"), Some(12));
    assert_eq!(konst("VisualShaderNodeFloatFunc", "FUNC_ONEMINUS"), Some(31));
    assert_eq!(konst("VisualShaderNodeFloatFunc", "FUNC_MAX"), Some(32));
    assert_eq!(konst("VisualShaderNodeFloatOp", "OP_ATAN2"), Some(8));
    assert_eq!(konst("VisualShaderNodeFloatOp", "OP_ENUM_SIZE"), Some(10));
}

/// The int variants are NOT the float ones with a different name: IntOp adds the
/// bitwise operators, IntFunc is a much shorter list. Sibling classes with the
/// same constant names and different values — a copy-paste would pass a naive
/// test and be wrong.
#[test]
fn int_op_and_func_differ_from_their_float_counterparts() {
    register_3d_classes();
    assert_eq!(konst("VisualShaderNodeIntOp", "OP_BITWISE_AND"), Some(7));
    assert_eq!(
        konst("VisualShaderNodeIntOp", "OP_BITWISE_LEFT_SHIFT"),
        Some(10)
    );
    assert_eq!(konst("VisualShaderNodeIntOp", "OP_ENUM_SIZE"), Some(12));
    assert_eq!(
        konst("VisualShaderNodeFloatOp", "OP_ENUM_SIZE"),
        Some(10),
        "the float op list is shorter — it has no bitwise operators"
    );
    // IntFunc is 5 values, not the float's 33.
    assert_eq!(konst("VisualShaderNodeIntFunc", "FUNC_BITWISE_NOT"), Some(3));
    assert_eq!(konst("VisualShaderNodeIntFunc", "FUNC_MAX"), Some(4));
    assert_eq!(
        konst("VisualShaderNodeFloatFunc", "FUNC_MAX"),
        Some(32),
        "float and int FUNC_MAX must not be conflated"
    );
}

/// Compare carries three separate enums; the acceptance calls out its expression.
#[test]
fn compare_carries_type_function_and_condition() {
    register_3d_classes();
    // Godot names the first property `type` (a Rust keyword, but a plain name here).
    assert_props("VisualShaderNodeCompare", &["type", "function", "condition"]);
    for e in ["ComparisonType", "Function", "Condition"] {
        assert!(class_has_enum("VisualShaderNodeCompare", e));
    }
    assert_eq!(konst("VisualShaderNodeCompare", "CTYPE_SCALAR"), Some(0));
    assert_eq!(konst("VisualShaderNodeCompare", "CTYPE_TRANSFORM"), Some(7));
    assert_eq!(konst("VisualShaderNodeCompare", "FUNC_EQUAL"), Some(0));
    assert_eq!(
        konst("VisualShaderNodeCompare", "FUNC_GREATER_THAN_EQUAL"),
        Some(3)
    );
    assert_eq!(konst("VisualShaderNodeCompare", "COND_ALL"), Some(0));
    assert_eq!(konst("VisualShaderNodeCompare", "COND_ANY"), Some(1));
}

/// The remaining selector enums, so a future edit to any of them is caught by a
/// test rather than only by a one-off manual diff against the API dump.
#[test]
fn color_vector_and_mix_selectors_hold_their_godot_values() {
    register_3d_classes();

    // ColorOp's operators are blend modes, sharing no numbering with FloatOp:
    // OP_SCREEN is 0 where FloatOp's 0 is OP_ADD.
    assert_props("VisualShaderNodeColorOp", &["operator"]);
    assert_eq!(konst("VisualShaderNodeColorOp", "OP_SCREEN"), Some(0));
    assert_eq!(konst("VisualShaderNodeColorOp", "OP_DODGE"), Some(5));
    assert_eq!(konst("VisualShaderNodeColorOp", "OP_BURN"), Some(6));
    assert_eq!(konst("VisualShaderNodeColorOp", "OP_MAX"), Some(9));

    // VectorOp adds the geometric operators FloatOp has no room for.
    assert_eq!(konst("VisualShaderNodeVectorOp", "OP_CROSS"), Some(8));
    assert_eq!(konst("VisualShaderNodeVectorOp", "OP_REFLECT"), Some(10));
    assert_eq!(konst("VisualShaderNodeVectorOp", "OP_ENUM_SIZE"), Some(12));

    // Mix/MultiplyAdd select the operand width rather than an operation.
    assert_props("VisualShaderNodeMix", &["op_type"]);
    assert_eq!(konst("VisualShaderNodeMix", "OP_TYPE_SCALAR"), Some(0));
    assert_eq!(konst("VisualShaderNodeMix", "OP_TYPE_VECTOR_2D"), Some(1));
    assert_eq!(konst("VisualShaderNodeMix", "OP_TYPE_MAX"), Some(7));
    assert_props("VisualShaderNodeMultiplyAdd", &["op_type"]);
    assert_eq!(
        konst("VisualShaderNodeMultiplyAdd", "OP_TYPE_MAX"),
        Some(4),
        "MultiplyAdd's width list is shorter than Mix's — they must not be conflated"
    );
}

/// The fixed-behaviour nodes must stay property-free: an invented selector would
/// be worse than none. `DotProduct` is covered above; these are the rest.
#[test]
fn remaining_fixed_nodes_have_no_properties() {
    register_3d_classes();
    for class in [
        "VisualShaderNodeIf",
        "VisualShaderNodeFresnel",
        "VisualShaderNodeDeterminant",
        "VisualShaderNodeOuterProduct",
    ] {
        assert_eq!(
            props(class),
            Vec::<String>::new(),
            "{class} has no properties in Godot"
        );
    }
}

#[test]
fn derivative_func_carries_op_type_function_and_precision() {
    register_3d_classes();
    assert_props(
        "VisualShaderNodeDerivativeFunc",
        &["op_type", "function", "precision"],
    );
    assert_eq!(
        konst("VisualShaderNodeDerivativeFunc", "PRECISION_COARSE"),
        Some(1)
    );
    assert_eq!(
        konst("VisualShaderNodeDerivativeFunc", "PRECISION_FINE"),
        Some(2)
    );
}

#[test]
fn is_node_tests_inf_and_nan() {
    register_3d_classes();
    assert_props("VisualShaderNodeIs", &["function"]);
    assert_eq!(konst("VisualShaderNodeIs", "FUNC_IS_INF"), Some(0));
    assert_eq!(konst("VisualShaderNodeIs", "FUNC_IS_NAN"), Some(1));
}

/// Godot gives these no properties at all — their behaviour is fixed. Registering
/// an invented selector on them would be worse than registering nothing.
#[test]
fn fixed_behaviour_nodes_have_no_selectors() {
    register_3d_classes();
    for class in [
        "VisualShaderNodeIf",
        "VisualShaderNodeFresnel",
        "VisualShaderNodeDotProduct",
        "VisualShaderNodeDeterminant",
        "VisualShaderNodeOuterProduct",
        "VisualShaderNodeFaceForward",
    ] {
        assert!(class_exists(class), "ClassDB must know {class}");
        assert!(is_parent_class(class, "VisualShaderNode"));
    }
    // FaceForward's only property is the one it inherits from VectorBase.
    assert_eq!(
        props("VisualShaderNodeDotProduct"),
        Vec::<String>::new(),
        "DotProduct has no properties of its own or inherited"
    );
}

#[test]
fn every_math_node_is_registered_and_reaches_object() {
    register_3d_classes();
    for class in [
        "VisualShaderNodeVectorBase",
        "VisualShaderNodeFloatOp",
        "VisualShaderNodeIntOp",
        "VisualShaderNodeColorOp",
        "VisualShaderNodeVectorOp",
        "VisualShaderNodeFloatFunc",
        "VisualShaderNodeIntFunc",
        "VisualShaderNodeVectorFunc",
        "VisualShaderNodeMix",
        "VisualShaderNodeMultiplyAdd",
        "VisualShaderNodeCompare",
        "VisualShaderNodeIf",
        "VisualShaderNodeIs",
        "VisualShaderNodeFresnel",
        "VisualShaderNodeDotProduct",
        "VisualShaderNodeDeterminant",
        "VisualShaderNodeOuterProduct",
        "VisualShaderNodeDerivativeFunc",
        "VisualShaderNodeFaceForward",
    ] {
        assert!(class_exists(class), "ClassDB must know {class}");
        assert!(
            is_parent_class(class, "Object"),
            "{class} must reach Object — is an intermediate base unregistered?"
        );
    }
}

#[test]
fn registration_is_idempotent() {
    register_3d_classes();
    let before = props("VisualShaderNodeCompare");
    register_3d_classes();
    assert_eq!(before, props("VisualShaderNodeCompare"));
}
