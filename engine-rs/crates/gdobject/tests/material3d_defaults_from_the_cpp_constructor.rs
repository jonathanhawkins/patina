//! pat-s3rrv.30: the BaseMaterial3D members whose doc XML states no `default=`.
//!
//! # Where these values come from, and why that matters
//!
//! Earlier metadata passes skipped every documented member without a
//! `default=` attribute, on the rule "never invent a default". That rule was
//! right, but the conclusion drawn from it — that the values need *a decision*
//! — was wrong. Godot omits `default=` from the XML when the value is not a
//! simple constant expression; the value itself is perfectly well defined, just
//! stated somewhere else:
//!
//! * `BaseMaterial3D::BaseMaterial3D()` (`scene/resources/material.cpp:3928`)
//!   sets nine of these ten with explicit `set_*` calls.
//! * `emission_intensity` is a field initialiser (`material.h:545`), commented
//!   there as "In nits, equivalent to indoor lighting".
//!
//! ⚠️ `extension_api.json` is NOT vendored in this repository, so the route
//! suggested on pat-s3rrv.57 — source them from the API dump — does not exist.
//! The C++ constructor is the oracle, and it is available for every class.
//!
//! Every value below is transcribed from those two places, not inferred from a
//! property's name or its documentation prose.

use gdobject::class_db::{
    get_property_getter, get_property_setter, property_get_revert, register_all_classes,
};
use gdvariant::Variant;

/// **The acceptance case**: all ten carry the constructor's values.
#[test]
fn the_undocumented_defaults_match_the_cpp_constructor() {
    register_all_classes();

    for (member, want) in [
        ("alpha_scissor_threshold", Variant::Float(0.5)),
        ("alpha_hash_scale", Variant::Float(1.0)),
        // ALPHA_ANTIALIASING_OFF.
        ("alpha_antialiasing_mode", Variant::Int(0)),
        ("alpha_antialiasing_edge", Variant::Float(0.3)),
        // In nits. ⚠️ NOT 1.0 — the tempting guess for anything called an
        // "intensity" — because Godot measures it in physical light units.
        ("emission_intensity", Variant::Float(1000.0)),
        ("heightmap_min_layers", Variant::Int(8)),
        ("heightmap_max_layers", Variant::Int(32)),
        ("particles_anim_h_frames", Variant::Int(1)),
        ("particles_anim_v_frames", Variant::Int(1)),
        ("particles_anim_loop", Variant::Bool(false)),
    ] {
        assert_eq!(
            property_get_revert("BaseMaterial3D", member),
            Some(want.clone()),
            "BaseMaterial3D.{member} must carry the value \
             BaseMaterial3D::BaseMaterial3D() sets, {want:?}",
        );
    }
}

/// 🔑 Two accessor pairs no naming rule could produce.
///
/// `alpha_antialiasing_mode` reads through **`get_alpha_antialiasing`** — the
/// `_mode` suffix is in the property name and neither accessor — and the
/// heightmap layer counts read through **`get_heightmap_deep_parallax_*`**,
/// where `deep_parallax` appears in the accessors and in neither property name.
///
/// A derived `get_<prop>` answers four names that do not exist, and would look
/// entirely plausible doing it.
#[test]
fn the_accessors_are_stored_not_derived() {
    register_all_classes();

    assert_eq!(
        get_property_getter("BaseMaterial3D", "alpha_antialiasing_mode"),
        "get_alpha_antialiasing",
    );
    assert_eq!(
        get_property_setter("BaseMaterial3D", "alpha_antialiasing_mode"),
        "set_alpha_antialiasing",
    );
    assert_eq!(
        get_property_getter("BaseMaterial3D", "heightmap_max_layers"),
        "get_heightmap_deep_parallax_max_layers",
    );
    assert_eq!(
        get_property_setter("BaseMaterial3D", "heightmap_min_layers"),
        "set_heightmap_deep_parallax_min_layers",
    );
}

/// The values have to reach the classes a scene actually names.
///
/// `BaseMaterial3D` is abstract; a `.tscn` writes `StandardMaterial3D` or
/// `ORMMaterial3D`. A registration that stopped at the base would leave the
/// inspector and `class_get_property_list` empty for both of them, which is the
/// "false green" this bead calls out.
#[test]
fn the_concrete_material_classes_inherit_them() {
    register_all_classes();

    for class in ["StandardMaterial3D", "ORMMaterial3D"] {
        assert_eq!(
            property_get_revert(class, "alpha_scissor_threshold"),
            Some(Variant::Float(0.5)),
            "{class} must inherit the BaseMaterial3D defaults",
        );
        assert_eq!(
            property_get_revert(class, "emission_intensity"),
            Some(Variant::Float(1000.0)),
        );
    }
}

/// `ArrayMesh.custom_aabb` — the one mesh member with a documented default that
/// was still missing.
///
/// An all-zero AABB means "no override"; Godot computes the real bounds from
/// the vertex data unless a custom one is set. Asserting the zero value pins
/// that a mesh does not start with a bogus one-unit box.
#[test]
fn array_mesh_custom_aabb_is_registered_empty() {
    register_all_classes();

    match property_get_revert("ArrayMesh", "custom_aabb") {
        Some(Variant::Aabb(a)) => {
            assert_eq!(a.position, gdcore::math::Vector3::ZERO);
            assert_eq!(a.size, gdcore::math::Vector3::ZERO);
        }
        other => panic!("ArrayMesh.custom_aabb must be an empty AABB, got {other:?}"),
    }
    assert_eq!(
        get_property_getter("ArrayMesh", "custom_aabb"),
        "get_custom_aabb",
    );
}
