//! pat-s3rrv.140 — the banded `AudioEffectEQ` family is registered with Godot's
//! exact band layouts.
//!
//! Godot ground truth: `servers/audio/effects/eq_filter.cpp:126-150` holds the
//! three frequency tables; `audio_effect_eq.cpp:120-122` binds the three band
//! methods and `:131` builds each band's property name; the family is
//! registered with plain `GDREGISTER_CLASS` at
//! `servers/register_server_types.cpp:175,196-198`.
//!
//! Before this, `AudioEffectEQ` was a bare propertyless registration and
//! `AudioEffectEQ6` / `10` / `21` did not exist in ClassDB at all, so a bus
//! layout carrying `band_db/*` had nothing to apply the values to.

use gdobject::class_db::{
    can_instantiate, class_exists, class_has_property, get_method_list, get_property_list,
    register_all_classes,
};
use gdobject::is_parent_class;
use gdvariant::Variant;

/// `eq_filter.cpp:128` — `{ 32, 100, 320, 1e3, 3200, 10e3 }`.
const EQ6: &[u32] = &[32, 100, 320, 1000, 3200, 10000];
/// `eq_filter.cpp:140` — `{ 31.25, 62.5, 125, … }`, truncated by `itos`.
const EQ10: &[u32] = &[31, 62, 125, 250, 500, 1000, 2000, 4000, 8000, 16000];
/// `eq_filter.cpp:147`.
const EQ21: &[u32] = &[
    22, 32, 44, 63, 90, 125, 175, 250, 350, 500, 700, 1000, 1400, 2000, 2800, 4000, 5600, 8000,
    11000, 16000, 22000,
];

fn band_names(class_name: &str) -> Vec<String> {
    get_property_list(class_name)
        .into_iter()
        .map(|p| p.name)
        .filter(|n| n.starts_with("band_db/"))
        .collect()
}

#[test]
fn the_eq_family_is_registered_under_audio_effect() {
    register_all_classes();
    for class in ["AudioEffectEQ6", "AudioEffectEQ10", "AudioEffectEQ21"] {
        assert!(class_exists(class), "{class} must be registered");
        assert!(
            is_parent_class(class, "AudioEffectEQ"),
            "{class} derives AudioEffectEQ, which is where the band methods live"
        );
        assert!(is_parent_class(class, "AudioEffect"), "…and AudioEffect");
        // All four are plain GDREGISTER_CLASS upstream — none is abstract,
        // including the base.
        assert!(can_instantiate(class), "{class} is not abstract in Godot");
    }
    assert!(can_instantiate("AudioEffectEQ"), "nor is the base");
}

/// 🔑 The exact band tables, in order.
///
/// ⚠️ These are **not** an even octave series — EQ6 runs 32/100/320/1000 and
/// EQ21 opens 22/32/44/63/90 — so a generated table is wrong from the second
/// band onward. A wrong band name is not a loud failure: it is a property a
/// `.tres` bus layout silently fails to apply, leaving the EQ flat.
#[test]
fn each_subclass_carries_godots_exact_band_frequencies() {
    register_all_classes();
    for (class, expected) in [
        ("AudioEffectEQ6", EQ6),
        ("AudioEffectEQ10", EQ10),
        ("AudioEffectEQ21", EQ21),
    ] {
        let got = band_names(class);
        let want: Vec<String> = expected.iter().map(|f| format!("band_db/{f}_hz")).collect();
        assert_eq!(
            got, want,
            "{class}'s bands must match `eq_filter.cpp` exactly, in order"
        );
    }
    assert_eq!(band_names("AudioEffectEQ6").len(), 6);
    assert_eq!(band_names("AudioEffectEQ10").len(), 10);
    assert_eq!(band_names("AudioEffectEQ21").len(), 21);
}

/// 🔑 The band name TRUNCATES its frequency — it does not round.
///
/// ⚠️ Godot builds the name with `itos(eq.get_band_frequency(i))`
/// (`audio_effect_eq.cpp:131`), and `itos` truncates toward zero. EQ10's second
/// band sits at **62.5 Hz**, so it is `band_db/62_hz`; rounding would give
/// `63` — a name nothing matches, on the one class where the frequencies are
/// fractional.
///
/// The contrast with EQ21 is what makes this sharp: `band_db/63_hz` is a REAL
/// band there, so the rounded name is not even obviously wrong — it is a valid
/// name belonging to a different class.
#[test]
fn fractional_band_frequencies_truncate_rather_than_round() {
    register_all_classes();
    assert!(
        class_has_property("AudioEffectEQ10", "band_db/62_hz"),
        "🔑 EQ10's 62.5 Hz band truncates to 62"
    );
    assert!(
        !class_has_property("AudioEffectEQ10", "band_db/63_hz"),
        "🔑 …and must NOT round to 63. `band_db/63_hz` is a real EQ21 band, so \
         the rounded spelling looks legitimate and silently matches nothing here"
    );
    assert!(
        class_has_property("AudioEffectEQ21", "band_db/63_hz"),
        "63 Hz IS an EQ21 band — the contrast that makes the assertion above bite"
    );
    // The first band takes the same treatment: 31.25 -> 31.
    assert!(class_has_property("AudioEffectEQ10", "band_db/31_hz"));
    assert!(!class_has_property("AudioEffectEQ10", "band_db/32_hz"));
    // …while 32 Hz is EQ6's and EQ21's opening band.
    assert!(class_has_property("AudioEffectEQ6", "band_db/32_hz"));
}

/// Bands default to 0 dB (flat) over Godot's asymmetric range.
///
/// ⚠️ `-60,24,0.1` is NOT symmetric: a band cuts 60 dB but boosts only 24. A
/// `±60` hint — which the bead's prose suggests — would let the inspector offer
/// a boost Godot rejects.
#[test]
fn bands_default_flat_over_godots_asymmetric_range() {
    register_all_classes();
    let props = get_property_list("AudioEffectEQ21");
    let first = props
        .iter()
        .find(|p| p.name == "band_db/22_hz")
        .expect("EQ21 opens at 22 Hz");
    assert_eq!(
        first.default_value,
        Variant::Float(0.0),
        "a band starts flat, not boosted"
    );
    assert_eq!(
        first.hint_string, "-60,24,0.1",
        "Godot's range cuts far deeper than it boosts"
    );
}

/// The three methods the base binds.
#[test]
fn audio_effect_eq_binds_the_band_accessors() {
    register_all_classes();
    let methods = get_method_list("AudioEffectEQ");
    for (name, argc) in [
        ("set_band_gain_db", 2usize),
        ("get_band_gain_db", 1),
        ("get_band_count", 0),
    ] {
        let m = methods
            .iter()
            .find(|m| m.name == name)
            .unwrap_or_else(|| panic!("AudioEffectEQ.{name}() must be registered"));
        assert_eq!(
            m.argument_count, argc,
            "AudioEffectEQ.{name}() takes {argc} argument(s)"
        );
    }
    // ⚠️ Inherited exactly ONCE, not redeclared. `get_method_list` walks the
    // inheritance chain, so a subclass that registered its own copy would
    // return the method TWICE — and the duplicate could carry a different
    // arity from the base while every presence check still passed.
    for class in ["AudioEffectEQ6", "AudioEffectEQ10", "AudioEffectEQ21"] {
        let count = get_method_list(class)
            .iter()
            .filter(|m| m.name == "get_band_count")
            .count();
        assert_eq!(
            count, 1,
            "{class} must inherit get_band_count from AudioEffectEQ exactly \
             once; 2 means it was redeclared on the subclass, 0 means the base \
             lost it"
        );
    }
}
