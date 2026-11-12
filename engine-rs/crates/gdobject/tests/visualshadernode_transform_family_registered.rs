//! pat-s3rrv.257 — the seven `VisualShaderNodeTransform*` classes in ClassDB.
//!
//! Godot ground truth: `doc/classes/VisualShaderNodeTransform*.xml`.
//!
//! # What was missing
//!
//! All seven leaves were absent while **both of their bases were already
//! present** (`VisualShaderNodeConstant`, `VisualShaderNodeParameter`), so a
//! `.tres` VisualShader graph containing any transform node had nothing to
//! resolve its `sub_resource` type to.
//!
//! # 🔑 Two operator names do not survive derivation
//!
//! ⚠️ `OP_AxB` has a **lowercase `x`**, and `OP_3x3_AxB` puts a **digit**
//! immediately after the prefix. A screaming-snake-case guess (`OP_AXB`,
//! `OP_3X3_AXB`) produces names no script can reference — and `class_exists`
//! still answers true, so the class looks registered while every constant
//! lookup on it fails. `c_*` pins the exact spellings.
//!
//! # 🔑 The two `Operator` enums are DIFFERENT
//!
//! ⚠️ `TransformOp` and `TransformVecMult` both declare an enum called
//! `Operator`, and they agree on the first two constants — then diverge
//! completely. `TransformOp.OP_MAX` is **9**; `TransformVecMult.OP_MAX` is
//! **4**. Reusing one table for both passes any test that only checks
//! `OP_AxB`/`OP_BxA`, which is exactly the shape a careless test would take.

use gdobject::class_db::{get_class_info, get_property_list, register_all_classes, ClassInfo};
use gdobject::{class_exists, is_parent_class};
use gdvariant::Variant;

const FAMILY: [&str; 7] = [
    "VisualShaderNodeTransformCompose",
    "VisualShaderNodeTransformConstant",
    "VisualShaderNodeTransformDecompose",
    "VisualShaderNodeTransformFunc",
    "VisualShaderNodeTransformOp",
    "VisualShaderNodeTransformParameter",
    "VisualShaderNodeTransformVecMult",
];

fn info(class: &str) -> ClassInfo {
    get_class_info(class).unwrap_or_else(|| panic!("{class} is registered"))
}

/// Looks up one named enum's constants on a class.
fn enum_constants(class: &str, enum_name: &str) -> Vec<(String, i64)> {
    info(class)
        .enums
        .into_iter()
        .find(|e| e.name == enum_name)
        .unwrap_or_else(|| panic!("{class} declares enum {enum_name}"))
        .constants
}

/// 🔑 **THE ACCEPTANCE CLAUSE.** All seven exist with Godot's exact parents.
///
/// ⚠️ The DIRECT parent is asserted, not a transitive `is_parent_class` walk.
/// Five of these inherit `VisualShaderNode` directly, but `TransformConstant`
/// and `TransformParameter` inherit it only *through* `VisualShaderNodeConstant`
/// / `VisualShaderNodeParameter` — so a transitive check reaches
/// `VisualShaderNode` either way and cannot tell a flattened parent from the
/// right one.
#[test]
fn a_all_seven_are_registered_with_godots_parents() {
    register_all_classes();

    for class in FAMILY {
        assert!(class_exists(class), "{class} must resolve");
    }

    let expected_parent = |class: &str| match class {
        "VisualShaderNodeTransformConstant" => "VisualShaderNodeConstant",
        "VisualShaderNodeTransformParameter" => "VisualShaderNodeParameter",
        _ => "VisualShaderNode",
    };
    for class in FAMILY {
        assert_eq!(
            info(class).parent_class,
            expected_parent(class),
            "{class} has the wrong DIRECT parent"
        );
    }

    // The two indirect ones still reach VisualShaderNode — which is exactly why
    // the direct assertion above is the one that matters.
    for class in FAMILY {
        assert!(
            is_parent_class(class, "VisualShaderNode"),
            "chain broken: {class} does not reach VisualShaderNode"
        );
    }
}

/// The documented members, with their doc-XML defaults.
///
/// ⚠️ `Compose` and `Decompose` declare an EMPTY `<members>`. Their absence is
/// asserted so nobody "completes" them with invented ports — a wrong shape
/// passes every existence check while corrupting a `.tres` round trip.
#[test]
fn b_members_match_the_doc_xml() {
    register_all_classes();

    let identity = Variant::Transform3D(gdcore::math3d::Transform3D::IDENTITY);

    let prop = |class: &str, name: &str| {
        get_property_list(class)
            .into_iter()
            .find(|p| p.name == name)
            .unwrap_or_else(|| panic!("{class}.{name} is registered"))
    };

    assert_eq!(
        prop("VisualShaderNodeTransformConstant", "constant").default_value,
        identity,
        "constant defaults to the identity Transform3D"
    );
    assert_eq!(
        prop("VisualShaderNodeTransformFunc", "function").default_value,
        Variant::Int(0)
    );
    assert_eq!(
        prop("VisualShaderNodeTransformOp", "operator").default_value,
        Variant::Int(0)
    );
    assert_eq!(
        prop("VisualShaderNodeTransformVecMult", "operator").default_value,
        Variant::Int(0)
    );

    // ⚠️ The bool's getter is `is_default_value_enabled`, not `get_…`.
    let enabled = prop(
        "VisualShaderNodeTransformParameter",
        "default_value_enabled",
    );
    assert_eq!(enabled.default_value, Variant::Bool(false));
    assert_eq!(enabled.getter, "is_default_value_enabled");
    assert_eq!(
        prop("VisualShaderNodeTransformParameter", "default_value").default_value,
        identity
    );

    // The two marker nodes declare nothing of their own.
    //
    // ⚠️ `info(..).properties` — the class's OWN table — not
    // `get_property_list`, which walks the chain and would return
    // `VisualShaderNode`'s four members plus `Resource`'s. Asserting the merged
    // list were empty would fail against correct code.
    for marker in [
        "VisualShaderNodeTransformCompose",
        "VisualShaderNodeTransformDecompose",
    ] {
        let own: Vec<String> = info(marker)
            .properties
            .into_iter()
            .map(|p| p.name)
            .collect();
        assert!(
            own.is_empty(),
            "{marker} declares an empty <members> in Godot; got {own:?}"
        );
        // Positive control: the chain walk DOES deliver the base's members, so
        // the emptiness above is about this class and not a broken lookup.
        assert!(
            get_property_list(marker)
                .iter()
                .any(|p| p.name == "output_port_for_preview"),
            "{marker} must still inherit VisualShaderNode's members"
        );
    }
}

/// 🔑 Every enum constant, at Godot's exact spelling and value.
#[test]
fn c_enum_constants_match_godot_exactly() {
    register_all_classes();

    assert_eq!(
        enum_constants("VisualShaderNodeTransformFunc", "Function"),
        vec![
            ("FUNC_INVERSE".to_string(), 0),
            ("FUNC_TRANSPOSE".to_string(), 1),
            ("FUNC_MAX".to_string(), 2),
        ]
    );

    // ⚠️ Lowercase `x` throughout — OP_AxB, not OP_AXB.
    assert_eq!(
        enum_constants("VisualShaderNodeTransformOp", "Operator"),
        vec![
            ("OP_AxB".to_string(), 0),
            ("OP_BxA".to_string(), 1),
            ("OP_AxB_COMP".to_string(), 2),
            ("OP_BxA_COMP".to_string(), 3),
            ("OP_ADD".to_string(), 4),
            ("OP_A_MINUS_B".to_string(), 5),
            ("OP_B_MINUS_A".to_string(), 6),
            ("OP_A_DIV_B".to_string(), 7),
            ("OP_B_DIV_A".to_string(), 8),
            ("OP_MAX".to_string(), 9),
        ]
    );

    // ⚠️ A DIGIT right after the prefix: OP_3x3_AxB.
    assert_eq!(
        enum_constants("VisualShaderNodeTransformVecMult", "Operator"),
        vec![
            ("OP_AxB".to_string(), 0),
            ("OP_BxA".to_string(), 1),
            ("OP_3x3_AxB".to_string(), 2),
            ("OP_3x3_BxA".to_string(), 3),
            ("OP_MAX".to_string(), 4),
        ]
    );
}

/// 🔑 The two `Operator` enums must not be the same table.
///
/// ⚠️ They share a name and their first two constants, so a test that checks
/// only `OP_AxB` and `OP_BxA` passes even if one table were copied over the
/// other. `OP_MAX` is where they diverge: 9 against 4.
#[test]
fn d_the_two_operator_enums_are_distinct() {
    register_all_classes();

    let op = enum_constants("VisualShaderNodeTransformOp", "Operator");
    let vec_mult = enum_constants("VisualShaderNodeTransformVecMult", "Operator");

    assert_eq!(op.len(), 10, "TransformOp.Operator has ten constants");
    assert_eq!(vec_mult.len(), 5, "TransformVecMult.Operator has five");

    let value = |table: &[(String, i64)], name: &str| {
        table
            .iter()
            .find(|(n, _)| n == name)
            .unwrap_or_else(|| panic!("{name} present"))
            .1
    };
    assert_eq!(value(&op, "OP_MAX"), 9);
    assert_eq!(value(&vec_mult, "OP_MAX"), 4);

    // The 3x3 pair exists on VecMult ONLY.
    for name in ["OP_3x3_AxB", "OP_3x3_BxA"] {
        assert!(
            vec_mult.iter().any(|(n, _)| n == name),
            "{name} must be on TransformVecMult"
        );
        assert!(
            !op.iter().any(|(n, _)| n == name),
            "{name} must NOT be on TransformOp"
        );
    }
    // …and the COMP pair on TransformOp only.
    for name in ["OP_AxB_COMP", "OP_BxA_COMP"] {
        assert!(op.iter().any(|(n, _)| n == name));
        assert!(!vec_mult.iter().any(|(n, _)| n == name));
    }
}

/// 🔑 No leaf re-declares a member it inherits.
///
/// ⚠️ This asserts each class's OWN property table, not the merged list. A
/// merged-list duplicate check would be **vacuous**: `get_property_list` treats
/// a re-declaration as an OVERRIDE and merges it by name (pat-s3rrv.74), so the
/// merged list can never contain a duplicate and the assertion could never
/// fail. The real hazard is the silent override — a re-declared
/// `parameter_name` would change the inherited default with nothing visible in
/// the property list — and only the own-table view can see it.
#[test]
fn e_no_leaf_redeclares_an_inherited_member() {
    register_all_classes();

    let own = |class: &str| -> Vec<String> {
        info(class).properties.into_iter().map(|p| p.name).collect()
    };

    assert_eq!(
        own("VisualShaderNodeTransformParameter"),
        vec![
            "default_value_enabled".to_string(),
            "default_value".to_string()
        ],
        "TransformParameter must declare ONLY the two members it adds"
    );
    for inherited in ["parameter_name", "qualifier", "instance_index"] {
        assert!(
            !own("VisualShaderNodeTransformParameter").contains(&inherited.to_string()),
            "{inherited} is inherited from VisualShaderNodeParameter"
        );
    }
    assert_eq!(
        own("VisualShaderNodeTransformConstant"),
        vec!["constant".to_string()],
        "TransformConstant adds only `constant`; its base is abstract and empty"
    );

    // No leaf may re-declare any of VisualShaderNode's own members.
    let base_members = own("VisualShaderNode");
    assert!(
        !base_members.is_empty(),
        "positive control: the base really does declare members"
    );
    for class in FAMILY {
        for name in own(class) {
            assert!(
                !base_members.contains(&name),
                "{class} re-declares the inherited `{name}` — a silent override"
            );
        }
    }

    // The inherited members still arrive through the chain, exactly once each.
    let merged: Vec<String> = get_property_list("VisualShaderNodeTransformParameter")
        .into_iter()
        .map(|p| p.name)
        .collect();
    for inherited in ["parameter_name", "qualifier", "instance_index"] {
        assert_eq!(
            merged.iter().filter(|n| *n == inherited).count(),
            1,
            "{inherited} must reach the leaf exactly once; got {merged:?}"
        );
    }
}
