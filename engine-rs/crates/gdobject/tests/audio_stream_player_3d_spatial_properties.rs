//! pat-s3rrv.55: `AudioStreamPlayer3D`'s spatial surface is nameable.
//!
//! The emission cone, the attenuation filter and Doppler tracking were all
//! implemented and mirrored per frame by the audio bridge, but a property the
//! engine reads and ClassDB cannot name is invisible to reflection, to the
//! inspector, and to `.tscn` round-tripping. `emission_angle_filter_attenuation_db`
//! was the last one missing — the cone could be enabled and given an angle,
//! while the amount it attenuates by could not be named at all, so a scene got
//! the -12 dB default whatever it asked for.
//!
//! Defaults and accessor names come from `doc/classes/AudioStreamPlayer3D.xml`.

use gdobject::class_db::{
    get_property_getter, get_property_list, get_property_setter, property_get_revert,
    register_all_classes,
};
use gdvariant::Variant;

/// **The acceptance case**: every member of the spatial group is registered
/// with Godot's default.
///
/// Asserted as a GROUP rather than one property at a time: these are wired
/// together and were added in two separate passes, so the failure mode worth
/// catching is "one of them was forgotten", which a single-property test cannot
/// see.
#[test]
fn the_whole_spatial_group_is_registered_with_godots_defaults() {
    register_all_classes();

    for (member, want) in [
        ("emission_angle_enabled", Variant::Bool(false)),
        // A HALF-angle: 45 degrees is a 90-degree cone.
        ("emission_angle_degrees", Variant::Float(45.0)),
        // ⚠️ NEGATIVE — it is a cut, and it drives a high-shelf rather than the
        // volume, so an off-axis source is muffled rather than merely quieter.
        (
            "emission_angle_filter_attenuation_db",
            Variant::Float(-12.0),
        ),
        ("attenuation_filter_cutoff_hz", Variant::Float(5000.0)),
        ("attenuation_filter_db", Variant::Float(-24.0)),
        ("doppler_tracking", Variant::Int(0)),
        ("attenuation_model", Variant::Int(0)),
        ("max_db", Variant::Float(3.0)),
        ("unit_size", Variant::Float(10.0)),
    ] {
        assert_eq!(
            property_get_revert("AudioStreamPlayer3D", member),
            Some(want.clone()),
            "AudioStreamPlayer3D.{member} must be registered with Godot's \
             documented default {want:?}",
        );
    }
}

/// 🔑 The accessors no naming rule could produce.
///
/// `emission_angle_degrees` reads through **`get_emission_angle`** — the
/// `_degrees` suffix appears in neither accessor — and
/// `emission_angle_enabled` through an `is_`. A derived `get_<prop>` would
/// answer names that do not exist.
#[test]
fn the_cone_accessors_are_stored_not_derived() {
    register_all_classes();

    assert_eq!(
        get_property_getter("AudioStreamPlayer3D", "emission_angle_degrees"),
        "get_emission_angle",
    );
    assert_eq!(
        get_property_setter("AudioStreamPlayer3D", "emission_angle_degrees"),
        "set_emission_angle",
    );
    assert_eq!(
        get_property_getter("AudioStreamPlayer3D", "emission_angle_enabled"),
        "is_emission_angle_enabled",
    );
    // This one IS conventional, and is asserted so a future "simplification"
    // that derives the whole group cannot pass by accident.
    assert_eq!(
        get_property_getter(
            "AudioStreamPlayer3D",
            "emission_angle_filter_attenuation_db"
        ),
        "get_emission_angle_filter_attenuation_db",
    );
}

/// ⚠️ The spatial group belongs to the 3D player ONLY.
///
/// `AudioStreamPlayer2D`'s registration ends with the same
/// `panning_strength` + play/stop tail, so an edit anchored on that tail lands
/// these properties on the 2D class instead — which `class_db.rs` records as
/// having actually happened once. Godot's 2D player has no emission cone, so
/// its presence there would be a silent parity error that nothing else checks.
#[test]
fn the_2d_player_does_not_carry_the_emission_cone() {
    register_all_classes();

    let names: Vec<String> = get_property_list("AudioStreamPlayer2D")
        .into_iter()
        .map(|p| p.name)
        .collect();

    for member in [
        "emission_angle_enabled",
        "emission_angle_degrees",
        "emission_angle_filter_attenuation_db",
    ] {
        assert!(
            !names.contains(&member.to_string()),
            "AudioStreamPlayer2D must NOT have `{member}` — Godot's 2D player \
             has no emission cone; got {names:?}",
        );
    }

    // …while the 3D one does, so this is not vacuously true because the lookup
    // is broken.
    let names_3d: Vec<String> = get_property_list("AudioStreamPlayer3D")
        .into_iter()
        .map(|p| p.name)
        .collect();
    assert!(
        names_3d.contains(&"emission_angle_degrees".to_string()),
        "the 3D player must still have the cone",
    );
}
