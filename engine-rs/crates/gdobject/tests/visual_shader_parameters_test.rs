//! pat-nzgdu.168 — VisualShader constant and parameter (uniform) node classes.
//!
//! Covers the bead's ClassDB clauses. Every schema here was diffed against
//! Godot's `extension_api.json` rather than recalled, which matters: the bead's
//! own description is wrong in several places (it omits `instance_index` and
//! `enum_names`, gives the Qualifier enum as NONE/GLOBAL/INSTANCE when Godot
//! has QUAL_NONE/QUAL_GLOBAL/QUAL_INSTANCE/QUAL_INSTANCE_INDEX/QUAL_MAX, calls
//! `VisualShaderNodeParameterRef`'s property `param_name` when Godot calls it
//! `parameter_name`, and claims the base registrations are missing when they
//! already exist). These tests encode the API dump, not the bead text.
//!
//! The `generate_code()` clause is NOT covered — see the bead notes: emitting
//! `uniform float <name> : hint_range(0,1);` needs a `parameter_name` on
//! `gdserver2d::material::VisualShaderNode` (which has no name field) plus a
//! uniform-declaration pass, and the `.tres` clause needs VisualShader resource
//! loading. Both are follow-up work.

use gdobject::{
    class_exists, class_get_integer_constant, class_has_enum, get_property_list, is_parent_class,
    register_3d_classes,
};
// `is_abstract` is `pub` but is not among lib.rs's re-exports (unlike every
// sibling query), so reach it through the module. Adding it to that list would
// be an unrelated change to a shared file.
use gdobject::class_db::is_abstract;
use gdvariant::Variant;

fn props(class: &str) -> Vec<String> {
    get_property_list(class)
        .into_iter()
        .map(|p| p.name)
        .collect()
}

fn default_of(class: &str, name: &str) -> Variant {
    get_property_list(class)
        .into_iter()
        .find(|p| p.name == name)
        .unwrap_or_else(|| panic!("{class} has no property `{name}`"))
        .default_value
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

/// The bead's ClassDB acceptance clause, verbatim.
#[test]
fn visual_shader_parameter_inheritance_matches_godot() {
    register_3d_classes();

    assert!(
        is_parent_class(
            "VisualShaderNodeFloatParameter",
            "VisualShaderNodeParameter"
        ),
        "FloatParameter derives the parameter base"
    );
    assert!(
        is_parent_class(
            "VisualShaderNodeCubemapParameter",
            "VisualShaderNodeTextureParameter"
        ),
        "CubemapParameter derives the *texture* parameter base, not the plain one"
    );

    // Every parameter node must reach Resource (VisualShaderNode : Resource) and
    // Object. An unregistered intermediate base severs the chain silently — a
    // bare class_exists check would not notice (pat-nzgdu.98).
    for class in [
        "VisualShaderNodeParameter",
        "VisualShaderNodeFloatParameter",
        "VisualShaderNodeIntParameter",
        "VisualShaderNodeColorParameter",
        "VisualShaderNodeTextureParameter",
        "VisualShaderNodeCubemapParameter",
        "VisualShaderNodeParameterRef",
    ] {
        assert!(class_exists(class), "ClassDB must know {class}");
        assert!(
            is_parent_class(class, "VisualShaderNode"),
            "{class} is a VisualShaderNode"
        );
        assert!(
            is_parent_class(class, "Object"),
            "{class} must reach Object — is an intermediate base unregistered?"
        );
    }
}

/// `ParameterRef` reads an existing uniform rather than declaring one, so it is
/// deliberately NOT a `VisualShaderNodeParameter`.
#[test]
fn parameter_ref_is_not_a_parameter() {
    register_3d_classes();
    assert!(
        !is_parent_class("VisualShaderNodeParameterRef", "VisualShaderNodeParameter"),
        "ParameterRef references a parameter; it does not declare one"
    );
    // Godot names this `parameter_name` — same as the base — not `param_name`.
    assert_props(
        "VisualShaderNodeParameterRef",
        &["parameter_name", "param_type"],
    );
}

#[test]
fn abstract_bases_are_marked_abstract() {
    register_3d_classes();
    // These are `.new()`-able in neither Godot nor here.
    assert!(is_abstract("VisualShaderNodeParameter"));
    assert!(is_abstract("VisualShaderNodeTextureParameter"));
    // The concrete ones are not.
    assert!(!is_abstract("VisualShaderNodeFloatParameter"));
    assert!(!is_abstract("VisualShaderNodeIntParameter"));
    assert!(!is_abstract("VisualShaderNodeColorParameter"));
    assert!(!is_abstract("VisualShaderNodeCubemapParameter"));
}

#[test]
fn parameter_base_carries_name_and_qualifier() {
    register_3d_classes();
    assert_props(
        "VisualShaderNodeParameter",
        // `instance_index` is easy to miss — it is real, and pairs with
        // QUAL_INSTANCE_INDEX.
        &["parameter_name", "qualifier", "instance_index"],
    );
    assert!(class_has_enum("VisualShaderNodeParameter", "Qualifier"));
    for (name, value) in [
        ("QUAL_NONE", 0),
        ("QUAL_GLOBAL", 1),
        ("QUAL_INSTANCE", 2),
        ("QUAL_INSTANCE_INDEX", 3),
        ("QUAL_MAX", 4),
    ] {
        assert_eq!(
            class_get_integer_constant("VisualShaderNodeParameter", name),
            Some(value),
            "Qualifier::{name} == {value} in Godot"
        );
    }
}

#[test]
fn float_parameter_carries_range_hint_schema() {
    register_3d_classes();
    assert_props(
        "VisualShaderNodeFloatParameter",
        &[
            "hint",
            "min",
            "max",
            "step",
            "default_value_enabled",
            "default_value",
        ],
    );
    // HINT_RANGE is what makes generate_code emit `: hint_range(min, max)`.
    assert_eq!(
        class_get_integer_constant("VisualShaderNodeFloatParameter", "HINT_RANGE"),
        Some(1)
    );
    assert_eq!(
        class_get_integer_constant("VisualShaderNodeFloatParameter", "HINT_RANGE_STEP"),
        Some(2)
    );
    // The float hint stops at HINT_MAX=3 — it has no HINT_ENUM.
    assert_eq!(
        class_get_integer_constant("VisualShaderNodeFloatParameter", "HINT_MAX"),
        Some(3)
    );
}

/// The int parameter's hint enum differs from the float one: it has HINT_ENUM,
/// and correspondingly an `enum_names` list. Getting this wrong would silently
/// shift HINT_MAX.
#[test]
fn int_parameter_hint_has_enum_variant_unlike_float() {
    register_3d_classes();
    assert_props(
        "VisualShaderNodeIntParameter",
        &[
            "hint",
            "min",
            "max",
            "step",
            "enum_names",
            "default_value_enabled",
            "default_value",
        ],
    );
    assert_eq!(
        class_get_integer_constant("VisualShaderNodeIntParameter", "HINT_ENUM"),
        Some(3),
        "the int hint has HINT_ENUM..."
    );
    assert_eq!(
        class_get_integer_constant("VisualShaderNodeIntParameter", "HINT_MAX"),
        Some(4),
        "...which pushes its HINT_MAX to 4, unlike the float's 3"
    );
    assert_eq!(
        default_of("VisualShaderNodeIntParameter", "enum_names"),
        Variant::PackedStringArray(Vec::new())
    );
}

#[test]
fn color_parameter_carries_default_value() {
    register_3d_classes();
    assert_props(
        "VisualShaderNodeColorParameter",
        &["default_value_enabled", "default_value"],
    );
    assert_eq!(
        default_of("VisualShaderNodeColorParameter", "default_value_enabled"),
        Variant::Bool(false),
        "a colour uniform has no default until asked for one"
    );
}

#[test]
fn texture_parameter_carries_all_five_sampler_enums() {
    register_3d_classes();
    assert_props(
        "VisualShaderNodeTextureParameter",
        &[
            "texture_type",
            "color_default",
            "texture_filter",
            "texture_repeat",
            "texture_source",
        ],
    );
    for e in [
        "TextureType",
        "ColorDefault",
        "TextureFilter",
        "TextureRepeat",
        "TextureSource",
    ] {
        assert!(
            class_has_enum("VisualShaderNodeTextureParameter", e),
            "TextureParameter must expose the {e} enum"
        );
    }
    // Spot-check values that are easy to transpose.
    assert_eq!(
        class_get_integer_constant("VisualShaderNodeTextureParameter", "TYPE_NORMAL_MAP"),
        Some(2)
    );
    assert_eq!(
        class_get_integer_constant(
            "VisualShaderNodeTextureParameter",
            "FILTER_LINEAR_MIPMAP_ANISOTROPIC"
        ),
        Some(6)
    );
    assert_eq!(
        class_get_integer_constant(
            "VisualShaderNodeTextureParameter",
            "SOURCE_NORMAL_ROUGHNESS"
        ),
        Some(3)
    );
}

/// Cubemap inherits its whole schema — Godot gives it no properties of its own.
/// The inherited fields must still be visible through the chain.
#[test]
fn cubemap_parameter_inherits_the_texture_schema() {
    register_3d_classes();
    assert_props(
        "VisualShaderNodeCubemapParameter",
        &[
            "texture_type",
            "color_default",
            "texture_filter",
            "texture_repeat",
            "texture_source",
            // ...and the parameter base's fields, two levels up.
            "parameter_name",
            "qualifier",
        ],
    );
}

/// The constant nodes the bead pairs with the parameters.
#[test]
fn constant_nodes_are_registered_with_their_value_property() {
    register_3d_classes();
    assert!(is_abstract("VisualShaderNodeConstant"));
    for class in [
        "VisualShaderNodeFloatConstant",
        "VisualShaderNodeIntConstant",
        "VisualShaderNodeBooleanConstant",
        "VisualShaderNodeColorConstant",
    ] {
        assert!(
            is_parent_class(class, "VisualShaderNodeConstant"),
            "{class} derives the constant base"
        );
        assert_props(class, &["constant"]);
    }
    // Each constant's default is typed for its node.
    assert_eq!(
        default_of("VisualShaderNodeIntConstant", "constant"),
        Variant::Int(0)
    );
    assert_eq!(
        default_of("VisualShaderNodeFloatConstant", "constant"),
        Variant::Float(0.0)
    );
}

#[test]
fn registration_is_idempotent() {
    register_3d_classes();
    let before = props("VisualShaderNodeFloatParameter");
    register_3d_classes();
    let after = props("VisualShaderNodeFloatParameter");
    assert_eq!(
        before, after,
        "re-registering must not duplicate properties"
    );
}
