//! pat-s3rrv.90 — an INT-typed light property must not be silently dropped.
//!
//! `sync_light_properties` matched only `Variant::Float` for its ten numeric
//! properties. GDScript types a bare literal as an **Int**, so the natural
//! spellings — `light.light_temperature = 3000`, `light.shadow_opacity = 0` —
//! were ignored, leaving the property at its default with no error and no
//! diagnostic. `.tscn` files happen to write `3000.0`, which is exactly why
//! every scene-loaded test on this bead missed it.
//!
//! ⚠️ Every value below is chosen to DIFFER from the field's default
//! (`gdserver3d::light`, `Light3D::directional`): temperature 6500, opacity 1.0,
//! fade begin 40.0, length 10.0, split_1 0.1. A fixture that reused a default
//! would pass whether or not the write landed.

use gdscene::node::Node;
use gdscene::render_server_3d::sync_light_properties;
use gdserver3d::light::{Light3D, Light3DId};
use gdvariant::Variant;

fn light() -> Light3D {
    Light3D::directional(Light3DId(1))
}

fn node_with(props: &[(&str, Variant)]) -> Node {
    let mut node = Node::new("Light", "OmniLight3D");
    for (key, value) in props {
        node.set_property(key, value.clone());
    }
    node
}

#[test]
fn int_typed_light_properties_reach_the_renderer() {
    let mut l = light();
    let node = node_with(&[
        ("light_temperature", Variant::Int(3000)),
        ("shadow_opacity", Variant::Int(0)),
        ("distance_fade_begin", Variant::Int(75)),
        ("distance_fade_length", Variant::Int(25)),
        ("distance_fade_shadow", Variant::Int(90)),
        ("light_indirect_energy", Variant::Int(3)),
        ("light_volumetric_fog_energy", Variant::Int(4)),
        ("directional_shadow_split_1", Variant::Int(1)),
    ]);
    sync_light_properties(&node, &mut l);

    assert_eq!(
        l.temperature, 3000.0,
        "an int-typed light_temperature must reach the light; 6500 means the \
         write was dropped and the default stands"
    );
    assert_eq!(l.shadow_opacity, 0.0, "int-typed shadow_opacity must land");
    assert_eq!(l.distance_fade_begin, 75.0);
    assert_eq!(l.distance_fade_length, 25.0);
    assert_eq!(l.distance_fade_shadow, 90.0);
    assert_eq!(l.indirect_energy, 3.0);
    assert_eq!(l.volumetric_fog_energy, 4.0);
    assert_eq!(l.shadow_split_1, 1.0);
}

/// `light_temperature` is the one property on this bead that reaches pixels, so
/// it must not merely be stored — it must still tint the colour. 3000K is warm,
/// so red survives and blue is cut.
#[test]
fn an_int_temperature_still_tints_the_colour() {
    let mut l = light();
    let before = l.color;
    sync_light_properties(
        &node_with(&[("light_temperature", Variant::Int(3000))]),
        &mut l,
    );

    assert!(
        l.color.b < before.b,
        "3000K must cut blue relative to the untinted colour ({} vs {})",
        l.color.b,
        before.b
    );
    assert!(
        l.color.r >= l.color.b,
        "a warm temperature must leave red at least as strong as blue"
    );
}

/// Control: floats must still work. The fix routes both kinds through one
/// reader, so a regression here would mean the float path was traded away.
#[test]
fn float_typed_properties_still_work() {
    let mut l = light();
    sync_light_properties(
        &node_with(&[
            ("light_temperature", Variant::Float(3000.0)),
            ("shadow_opacity", Variant::Float(0.25)),
        ]),
        &mut l,
    );

    assert_eq!(l.temperature, 3000.0);
    assert_eq!(l.shadow_opacity, 0.25);
}

/// Control: an ABSENT property must leave the default alone. A reader that
/// answered `Some(0.0)` for a missing key would zero every unset property —
/// worse than dropping the write, and invisible to the tests above.
#[test]
fn an_absent_property_leaves_the_default() {
    let mut l = light();
    let defaults = (l.temperature, l.shadow_opacity, l.distance_fade_begin);
    sync_light_properties(&node_with(&[]), &mut l);

    assert_eq!(
        (l.temperature, l.shadow_opacity, l.distance_fade_begin),
        defaults,
        "a node that sets nothing must not disturb any light field"
    );
}

/// Control: a non-numeric value must be ignored rather than coerced. A String
/// silently becoming 0.0 would blank the light.
#[test]
fn a_non_numeric_value_is_ignored() {
    let mut l = light();
    let before = l.shadow_opacity;
    sync_light_properties(
        &node_with(&[("shadow_opacity", Variant::String("0.5".into()))]),
        &mut l,
    );

    assert_eq!(
        l.shadow_opacity, before,
        "a String must not be coerced into a numeric light property"
    );
}

/// `light_negative` is a plain Bool, so it was never part of the int/float
/// defect — but nothing read it at all until pat-s3rrv.90. This pins the read;
/// the SHADING half (a negative light actually subtracting) is asserted on
/// pixels by `a_negative_light_subtracts_instead_of_adding` in gdrender3d.
#[test]
fn light_negative_reaches_the_light() {
    let mut l = light();
    assert!(!l.negative, "Godot's light_negative defaults to false");

    sync_light_properties(
        &node_with(&[("light_negative", Variant::Bool(true))]),
        &mut l,
    );
    assert!(
        l.negative,
        "an authored light_negative must reach the light"
    );

    // Control: it must not latch — clearing the property clears the light.
    sync_light_properties(
        &node_with(&[("light_negative", Variant::Bool(false))]),
        &mut l,
    );
    assert!(
        !l.negative,
        "clearing light_negative must clear it on the light"
    );
}
