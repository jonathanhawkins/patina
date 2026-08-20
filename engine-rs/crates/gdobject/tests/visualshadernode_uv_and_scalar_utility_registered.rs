//! pat-s3rrv.259 — the UV and scalar-utility `VisualShaderNode` leaves.
//!
//! Godot ground truth: `doc/classes/VisualShaderNode{Step,SmoothStep,Switch,
//! UVFunc,UVPolarCoord,RotationByAxis}.xml`.
//!
//! # What was missing
//!
//! All six were absent while their comparable siblings (`Mix`, `Compare`, `If`)
//! were registered, so a `.tres` VisualShader graph using any of them had an
//! unresolvable node class.
//!
//! # 🔑 Two different `OpType` tables, and one constant that CANNOT tell them apart
//!
//! `Step`/`SmoothStep` and `Switch` each declare an enum named `OpType`. They
//! differ in membership *and* in numbering for the names they share:
//!
//! ```text
//!                 Step/SmoothStep   Switch
//!   VECTOR_2D            1            3
//!   VECTOR_3D            3            4
//!   VECTOR_4D            5            5   <- ⚠️ EQUAL
//!   OP_TYPE_MAX          7            8
//! ```
//!
//! ⚠️ **`OP_TYPE_VECTOR_4D` is 5 in both.** It is the one shared name that
//! cannot witness the difference — a "these tables differ" test written around
//! it passes even if one table were copied over the other. `d_*` uses
//! `VECTOR_2D`, `VECTOR_3D` and `OP_TYPE_MAX` as the witnesses and asserts the
//! `VECTOR_4D` coincidence explicitly, so the next reader meets it as a
//! documented result rather than rediscovering it.

use gdobject::class_db::{get_class_info, get_property_list, register_all_classes, ClassInfo};
use gdobject::{class_exists, is_parent_class};
use gdvariant::Variant;

const FAMILY: [&str; 6] = [
    "VisualShaderNodeRotationByAxis",
    "VisualShaderNodeSmoothStep",
    "VisualShaderNodeStep",
    "VisualShaderNodeSwitch",
    "VisualShaderNodeUVFunc",
    "VisualShaderNodeUVPolarCoord",
];

fn info(class: &str) -> ClassInfo {
    get_class_info(class).unwrap_or_else(|| panic!("{class} is registered"))
}

fn enum_constants(class: &str, enum_name: &str) -> Vec<(String, i64)> {
    info(class)
        .enums
        .into_iter()
        .find(|e| e.name == enum_name)
        .unwrap_or_else(|| panic!("{class} declares enum {enum_name}"))
        .constants
}

fn value(class: &str, enum_name: &str, constant: &str) -> i64 {
    enum_constants(class, enum_name)
        .into_iter()
        .find(|(n, _)| n == constant)
        .unwrap_or_else(|| panic!("{class}.{constant} present"))
        .1
}

/// 🔑 **THE ACCEPTANCE CLAUSE.** All six exist, parented to `VisualShaderNode`.
#[test]
fn a_all_six_are_registered_under_visualshadernode() {
    register_all_classes();

    for class in FAMILY {
        assert!(class_exists(class), "{class} must resolve");
        // The DIRECT parent — all six inherit VisualShaderNode itself, with no
        // intermediate base, so this is stronger than the transitive walk.
        assert_eq!(
            info(class).parent_class,
            "VisualShaderNode",
            "{class} has the wrong direct parent"
        );
        assert!(is_parent_class(class, "VisualShaderNode"));
    }
}

/// The documented members and their defaults.
#[test]
fn b_members_match_the_doc_xml() {
    register_all_classes();

    let prop = |class: &str, name: &str| {
        get_property_list(class)
            .into_iter()
            .find(|p| p.name == name)
            .unwrap_or_else(|| panic!("{class}.{name} is registered"))
    };

    for class in [
        "VisualShaderNodeStep",
        "VisualShaderNodeSmoothStep",
        "VisualShaderNodeSwitch",
    ] {
        let op = prop(class, "op_type");
        assert_eq!(op.default_value, Variant::Int(0), "{class}.op_type");
        assert_eq!(op.getter, "get_op_type");
    }

    let function = prop("VisualShaderNodeUVFunc", "function");
    assert_eq!(function.default_value, Variant::Int(0));
    assert_eq!(function.getter, "get_function");

    // ⚠️ The two marker nodes declare an EMPTY `<members>`. Asserted on the
    // class's OWN table, not `get_property_list`, which walks the chain and
    // returns `VisualShaderNode`'s members plus `Resource`'s — asserting the
    // merged list were empty would fail against correct code.
    for marker in [
        "VisualShaderNodeRotationByAxis",
        "VisualShaderNodeUVPolarCoord",
    ] {
        let own: Vec<String> = info(marker)
            .properties
            .into_iter()
            .map(|p| p.name)
            .collect();
        assert!(
            own.is_empty(),
            "{marker} declares no members in Godot; got {own:?}"
        );
        // Positive control: the chain walk still delivers the base's members, so
        // the emptiness above is about this class and not a broken lookup.
        assert!(
            get_property_list(marker)
                .iter()
                .any(|p| p.name == "output_port_for_preview"),
            "{marker} must still inherit VisualShaderNode's members"
        );
    }
}

/// Every enum constant, at Godot's exact value.
#[test]
fn c_enum_constants_match_godot_exactly() {
    register_all_classes();

    let scalar_table = vec![
        ("OP_TYPE_SCALAR".to_string(), 0),
        ("OP_TYPE_VECTOR_2D".to_string(), 1),
        ("OP_TYPE_VECTOR_2D_SCALAR".to_string(), 2),
        ("OP_TYPE_VECTOR_3D".to_string(), 3),
        ("OP_TYPE_VECTOR_3D_SCALAR".to_string(), 4),
        ("OP_TYPE_VECTOR_4D".to_string(), 5),
        ("OP_TYPE_VECTOR_4D_SCALAR".to_string(), 6),
        ("OP_TYPE_MAX".to_string(), 7),
    ];
    assert_eq!(
        enum_constants("VisualShaderNodeStep", "OpType"),
        scalar_table
    );
    assert_eq!(
        enum_constants("VisualShaderNodeSmoothStep", "OpType"),
        scalar_table,
        "SmoothStep carries the same table as Step"
    );

    assert_eq!(
        enum_constants("VisualShaderNodeSwitch", "OpType"),
        vec![
            ("OP_TYPE_FLOAT".to_string(), 0),
            ("OP_TYPE_INT".to_string(), 1),
            ("OP_TYPE_UINT".to_string(), 2),
            ("OP_TYPE_VECTOR_2D".to_string(), 3),
            ("OP_TYPE_VECTOR_3D".to_string(), 4),
            ("OP_TYPE_VECTOR_4D".to_string(), 5),
            ("OP_TYPE_BOOLEAN".to_string(), 6),
            ("OP_TYPE_TRANSFORM".to_string(), 7),
            ("OP_TYPE_MAX".to_string(), 8),
        ]
    );

    assert_eq!(
        enum_constants("VisualShaderNodeUVFunc", "Function"),
        vec![
            ("FUNC_PANNING".to_string(), 0),
            ("FUNC_SCALING".to_string(), 1),
            ("FUNC_MAX".to_string(), 2),
        ]
    );
}

/// 🔑 `Switch`'s `OpType` is a different table from `Step`'s.
///
/// ⚠️ The witness matters. `OP_TYPE_VECTOR_4D` is **5 in both**, so a
/// distinctness test built on it passes even against a shared table. The
/// assertions below use `VECTOR_2D` (1 vs 3), `VECTOR_3D` (3 vs 4) and
/// `OP_TYPE_MAX` (7 vs 8), and pin the `VECTOR_4D` coincidence so nobody
/// "corrects" it later.
#[test]
fn d_switch_optype_diverges_from_step_optype() {
    register_all_classes();

    let step = |c: &str| value("VisualShaderNodeStep", "OpType", c);
    let switch = |c: &str| value("VisualShaderNodeSwitch", "OpType", c);

    // The witnesses.
    assert_eq!(
        (step("OP_TYPE_VECTOR_2D"), switch("OP_TYPE_VECTOR_2D")),
        (1, 3)
    );
    assert_eq!(
        (step("OP_TYPE_VECTOR_3D"), switch("OP_TYPE_VECTOR_3D")),
        (3, 4)
    );
    assert_eq!((step("OP_TYPE_MAX"), switch("OP_TYPE_MAX")), (7, 8));

    // ⚠️ The coincidence point, asserted deliberately: equal in both, and
    // therefore useless as evidence that the tables differ.
    assert_eq!(
        (step("OP_TYPE_VECTOR_4D"), switch("OP_TYPE_VECTOR_4D")),
        (5, 5),
        "VECTOR_4D really is 5 in both — this is Godot's numbering, not a bug"
    );

    // Membership differs too, in both directions.
    let step_names: Vec<String> = enum_constants("VisualShaderNodeStep", "OpType")
        .into_iter()
        .map(|(n, _)| n)
        .collect();
    let switch_names: Vec<String> = enum_constants("VisualShaderNodeSwitch", "OpType")
        .into_iter()
        .map(|(n, _)| n)
        .collect();

    for only_switch in [
        "OP_TYPE_FLOAT",
        "OP_TYPE_INT",
        "OP_TYPE_UINT",
        "OP_TYPE_BOOLEAN",
        "OP_TYPE_TRANSFORM",
    ] {
        assert!(switch_names.contains(&only_switch.to_string()));
        assert!(
            !step_names.contains(&only_switch.to_string()),
            "{only_switch} must NOT be on Step"
        );
    }
    // ⚠️ Switch has no `_SCALAR` members of any kind.
    assert!(
        !switch_names.iter().any(|n| n.ends_with("_SCALAR")),
        "Switch's OpType has no _SCALAR members; got {switch_names:?}"
    );

    // ⚠️ Step's `_SCALAR` names are TWO different things, and a bare
    // `ends_with("_SCALAR")` conflates them — that predicate matches FOUR
    // names, because the standalone `OP_TYPE_SCALAR` mode ends in `_SCALAR`
    // too. Asserting "three _SCALAR variants" against it fails on correct
    // code (it did, on the first submission of this test). Both categories are
    // now pinned separately.
    assert_eq!(
        step_names
            .iter()
            .filter(|n| n.ends_with("D_SCALAR"))
            .count(),
        3,
        "the vector-scalar hybrids: VECTOR_2D_SCALAR / 3D_SCALAR / 4D_SCALAR"
    );
    assert_eq!(
        step_names.iter().filter(|n| *n == "OP_TYPE_SCALAR").count(),
        1,
        "…plus the standalone scalar mode, which is a separate thing"
    );
    assert_eq!(
        step_names.iter().filter(|n| n.ends_with("_SCALAR")).count(),
        4,
        "so the naive predicate matches 4, not 3 — pinned so it stays obvious"
    );

    assert_eq!(step_names.len(), 8);
    assert_eq!(switch_names.len(), 9);
}

/// No leaf re-declares a member it inherits.
///
/// ⚠️ Asserted on each class's OWN table. A merged-list duplicate check is
/// vacuous — `get_property_list` merges a re-declaration as an override
/// (pat-s3rrv.74), so it can never report a duplicate. The real hazard is that
/// silent override, and only the own-table view can see it.
#[test]
fn e_no_leaf_redeclares_an_inherited_member() {
    register_all_classes();

    let own = |class: &str| -> Vec<String> {
        info(class).properties.into_iter().map(|p| p.name).collect()
    };

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

    assert_eq!(own("VisualShaderNodeStep"), vec!["op_type".to_string()]);
    assert_eq!(own("VisualShaderNodeUVFunc"), vec!["function".to_string()]);
}
