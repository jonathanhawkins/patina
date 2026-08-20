//! pat-s3rrv.210 — the Vec2/Vec4 constants, Vec2/3/4 parameters and the three
//! VectorBase leaves are ClassDB-registered.
//!
//! Only `VisualShaderNodeVec3Constant` was registered, so a VisualShader `.tres`
//! could name the other eight and ClassDB could not describe them. The gap was
//! already load-bearing: `gdscene::visual_shader_load` special-cases the literal
//! `"VisualShaderNodeVec4Constant"` when parsing one, so the loader consumed a
//! class the ClassDB-driven create dialog and inspector could not show.
//!
//! Every parent and default below is transcribed from
//! `upstream/godot/doc/classes/VisualShaderNode*.xml`.

use gdobject::class_db::{
    get_class_info, get_property_getter, get_property_list, is_parent_class, register_all_classes,
};
use gdvariant::Variant;

const CONSTANTS: [&str; 2] = [
    "VisualShaderNodeVec2Constant",
    "VisualShaderNodeVec4Constant",
];
const PARAMETERS: [&str; 3] = [
    "VisualShaderNodeVec2Parameter",
    "VisualShaderNodeVec3Parameter",
    "VisualShaderNodeVec4Parameter",
];
const VECTOR_LEAVES: [&str; 3] = [
    "VisualShaderNodeVectorCompose",
    "VisualShaderNodeVectorDecompose",
    "VisualShaderNodeVectorDistance",
];

fn default_of(class: &str, name: &str) -> Variant {
    register_all_classes();
    get_property_list(class)
        .into_iter()
        .find(|p| p.name == name)
        .unwrap_or_else(|| panic!("{class} must expose `{name}`"))
        .default_value
}

/// All eight exist under their documented parents.
#[test]
fn all_eight_are_registered_under_their_godot_parents() {
    register_all_classes();
    for class in CONSTANTS {
        assert!(
            get_class_info(class).is_some(),
            "{class} must be registered"
        );
        assert!(
            is_parent_class(class, "VisualShaderNodeConstant"),
            "{class} inherits VisualShaderNodeConstant"
        );
    }
    for class in PARAMETERS {
        assert!(
            get_class_info(class).is_some(),
            "{class} must be registered"
        );
        assert!(
            is_parent_class(class, "VisualShaderNodeParameter"),
            "{class} inherits VisualShaderNodeParameter"
        );
    }
    for class in VECTOR_LEAVES {
        assert!(
            get_class_info(class).is_some(),
            "{class} must be registered"
        );
        assert!(
            is_parent_class(class, "VisualShaderNodeVectorBase"),
            "{class} inherits VisualShaderNodeVectorBase"
        );
    }
}

/// 🔑 **The two Vec4 types do NOT agree on their vector type**, and this is the
/// assertion most likely to catch a copy-paste: `Vec4Constant.constant` is a
/// **Quaternion(0, 0, 0, 1)** while `Vec4Parameter.default_value` is a
/// **Vector4(0, 0, 0, 0)**. Both the type and the value differ — the quaternion's
/// `w` is 1, so even reading it as a 4-vector of zeroes is wrong.
#[test]
fn the_two_vec4_types_are_not_interchangeable() {
    match default_of("VisualShaderNodeVec4Constant", "constant") {
        Variant::Quaternion(q) => assert!(
            q.x == 0.0 && q.y == 0.0 && q.z == 0.0 && q.w == 1.0,
            "doc default is Quaternion(0, 0, 0, 1) — the identity, whose w is \
             ONE, not a zero 4-vector; got {q:?}"
        ),
        other => panic!("Vec4Constant.constant must be a Quaternion, got {other:?}"),
    }
    match default_of("VisualShaderNodeVec4Parameter", "default_value") {
        Variant::Vector4(v) => assert!(
            v.x == 0.0 && v.y == 0.0 && v.z == 0.0 && v.w == 0.0,
            "doc default is Vector4(0, 0, 0, 0); got {v:?}"
        ),
        other => panic!("Vec4Parameter.default_value must be a Vector4, got {other:?}"),
    }
}

/// The remaining transcribed defaults.
#[test]
fn the_constant_and_parameter_defaults_match_the_documentation() {
    match default_of("VisualShaderNodeVec2Constant", "constant") {
        Variant::Vector2(v) => assert!(v.x == 0.0 && v.y == 0.0, "doc default is Vector2(0, 0)"),
        other => panic!("Vec2Constant.constant must be a Vector2, got {other:?}"),
    }
    match default_of("VisualShaderNodeVec2Parameter", "default_value") {
        Variant::Vector2(v) => assert!(v.x == 0.0 && v.y == 0.0, "doc default is Vector2(0, 0)"),
        other => panic!("Vec2Parameter.default_value must be a Vector2, got {other:?}"),
    }
    match default_of("VisualShaderNodeVec3Parameter", "default_value") {
        Variant::Vector3(v) => assert!(
            v.x == 0.0 && v.y == 0.0 && v.z == 0.0,
            "doc default is Vector3(0, 0, 0)"
        ),
        other => panic!("Vec3Parameter.default_value must be a Vector3, got {other:?}"),
    }
    for class in PARAMETERS {
        assert!(
            matches!(
                default_of(class, "default_value_enabled"),
                Variant::Bool(false)
            ),
            "{class}.default_value_enabled defaults to false"
        );
    }
}

/// ⚠️ `default_value_enabled` reads through `is_default_value_enabled` while
/// `default_value` beside it uses `get_` — no rule recovers that, so it is
/// asserted rather than assumed.
#[test]
fn the_parameter_accessors_are_the_documented_ones() {
    register_all_classes();
    for class in PARAMETERS {
        assert_eq!(
            get_property_getter(class, "default_value_enabled"),
            "is_default_value_enabled",
            "{class} must record the documented getter; `get_…` is the natural \
             guess and is wrong"
        );
        assert_eq!(
            get_property_getter(class, "default_value"),
            "get_default_value",
            "{class}'s sibling property really does use `get_`"
        );
    }
}

/// The acceptance's own check: `op_type` appears exactly once on each leaf.
///
/// ⚠️ **This no longer discriminates, and saying so matters.** The bead was
/// written against a `get_property_list` that concatenated the inheritance
/// chain without deduping (pat-0q2kx.148). `pat-s3rrv.74` since made the walk
/// treat a re-declared base property as an OVERRIDE — it replaces in place — so
/// this count is 1 whether or not a leaf re-declares `op_type`. It is kept
/// because the acceptance names it, but `the_vector_leaves_declare_no_members`
/// below is the test that actually holds the line.
#[test]
fn op_type_is_exposed_exactly_once_on_each_vector_leaf() {
    register_all_classes();
    for class in VECTOR_LEAVES {
        let count = get_property_list(class)
            .iter()
            .filter(|p| p.name == "op_type")
            .count();
        assert_eq!(count, 1, "{class} must expose op_type exactly once");
        assert!(
            matches!(default_of(class, "op_type"), Variant::Int(0)),
            "{class} inherits VectorBase's op_type default of 0"
        );
    }
}

/// 🔑 **The real invariant.** The three leaves must declare no members of their
/// OWN — `op_type` reaches them through VisualShaderNodeVectorBase, and their
/// doc XML lists nothing.
///
/// This is what the acceptance was reaching for. Unlike the count above it
/// inspects the class's own registration rather than the merged chain, so it
/// still fails if someone re-declares `op_type` on a leaf.
#[test]
fn the_vector_leaves_declare_no_members_of_their_own() {
    register_all_classes();
    for class in VECTOR_LEAVES {
        let own = get_class_info(class)
            .unwrap_or_else(|| panic!("{class} is registered"))
            .properties;
        assert!(
            own.is_empty(),
            "{class} must declare no properties of its own — its doc XML lists \
             none and `op_type` is inherited. Re-declaring it duplicates the \
             member in Godot's own `class_get_property_list` even though our \
             merged walk now hides that. got {:?}",
            own.iter().map(|p| &p.name).collect::<Vec<_>>()
        );
    }
}
