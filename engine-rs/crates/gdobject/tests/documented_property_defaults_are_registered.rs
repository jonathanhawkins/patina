//! pat-s3rrv.57 — registered classes carry their documented properties.
//!
//! The audit found **287 registered classes with zero properties** while Godot
//! documents some for many of them, so `ClassDB.class_get_property_list` and
//! anything seeded from it (node defaults, the inspector, save/load) simply had
//! nothing to say about those classes.
//!
//! # Where the values come from, and what is deliberately absent
//!
//! Every default here is the `default=` attribute on a `<member>` in
//! `upstream/godot/doc/classes/<Class>.xml` — the only place Godot states them.
//! A member with **no** `default=` is NOT registered: the doc omits it when the
//! default is not a simple constant, and inventing one would be a confidently
//! wrong answer that `class_exists`-style reflection then reports as fact.
//! Godot's own docs are the oracle, not the property's name or type.

use gdvariant::Variant;

/// The default recorded for `class.property`, or `None` when the class does not
/// declare it.
///
/// ⚠️ Takes the LAST match: `get_property_list` returns base-first, so a derived
/// class that redeclares a property appears after its base and is the one that
/// should win.
fn default_of(class: &str, property: &str) -> Option<Variant> {
    gdobject::class_db::register_all_classes();
    gdobject::class_db::get_property_list(class)
        .into_iter()
        .filter(|p| p.name == property)
        .next_back()
        .map(|p| p.default_value)
}

/// **The acceptance case**: a class that had zero properties now reports its
/// documented ones, with the documented values.
#[test]
fn documented_defaults_are_registered() {
    assert_eq!(
        default_of("GridContainer", "columns"),
        Some(Variant::Int(1)),
        "GridContainer.columns is documented `default=\"1\"`; None here means \
         the class still carries no properties at all"
    );
    assert_eq!(
        default_of("Marker3D", "gizmo_extents"),
        Some(Variant::Float(0.25)),
        "Marker3D.gizmo_extents is documented `default=\"0.25\"`"
    );
}

/// 🔑 Three different Variant types on one class.
///
/// A translation that handled only integers — the easy majority — would pass a
/// single-type test and silently drop every bool and String default.
#[test]
fn bool_int_and_string_defaults_all_survive_translation() {
    assert_eq!(
        default_of("AnimationNodeBlendSpace2D", "auto_triangles"),
        Some(Variant::Bool(true)),
        "a bool default must arrive as Bool(true), not Int(1) or the type's zero"
    );
    assert_eq!(
        default_of("AnimationNodeBlendSpace2D", "blend_mode"),
        Some(Variant::Int(0))
    );
    assert_eq!(
        default_of("AnimationNodeBlendSpace2D", "x_label"),
        Some(Variant::String("x".into())),
        "a String default is written `&quot;x&quot;` in the XML and must be \
         unescaped to `x` — registering the entity text would put literal \
         `&quot;` in the property"
    );
}

/// Composite defaults are parsed from their documented literal, not zeroed.
///
/// `default="Vector3(1, 1, 1)"` is a real value, and a translation that only
/// understood scalars would either skip these or register the type's zero —
/// and a `BoxShape3D` silently sized (0,0,0) collides with nothing, which is
/// exactly the kind of wrong-but-plausible default that is hard to trace back.
#[test]
fn vector_and_color_defaults_are_parsed_from_their_literal() {
    assert_eq!(
        default_of("BoxShape3D", "size"),
        Some(Variant::Vector3(gdcore::math::Vector3::new(1.0, 1.0, 1.0))),
        "BoxShape3D.size is documented `Vector3(1, 1, 1)` — a zeroed default \
         would give a box that collides with nothing"
    );
    assert_eq!(
        default_of("AnimationNodeBlendTree", "graph_offset"),
        Some(Variant::Vector2(gdcore::math::Vector2::new(0.0, 0.0)))
    );
    assert_eq!(
        default_of("Path3D", "debug_custom_color"),
        Some(Variant::Color(gdcore::math::Color::new(0.0, 0.0, 0.0, 1.0))),
        "a Color default carries all four channels; dropping alpha would make \
         it transparent"
    );
}

/// 🔑 A member whose default the docs do NOT state is left unregistered.
///
/// `AudioStreamPlayer3D.volume_linear` is documented, and its type is `float` —
/// a type this translation DOES handle — but the docs state no `default=`,
/// because the value is derived rather than constant. So it stays out.
///
/// That is the sharp version of the rule: the omission is about the missing
/// DEFAULT, not an unsupported type. Registering it with a fabricated `0.0`
/// would read back through reflection as Godot's answer, and nothing
/// downstream could tell the invention from a fact.
#[test]
fn a_member_with_no_documented_default_is_not_invented() {
    assert_eq!(
        default_of("AudioStreamPlayer3D", "volume_linear"),
        None,
        "volume_linear has no `default=` in the docs, so ClassDB must stay \
         silent about it rather than answer with a guess"
    );
}
