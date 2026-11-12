//! pat-0q2kx.160 — `AudioEffectSpectrumAnalyzer` and its instance companion are
//! registered classes.
//!
//! Godot ground truth: `extension_api.json` — `AudioEffectSpectrumAnalyzer <-
//! AudioEffect`, `AudioEffectSpectrumAnalyzerInstance <- AudioEffectInstance`;
//! defaults from `doc/classes/AudioEffectSpectrumAnalyzer.xml`.
//!
//! The FFT and `get_magnitude_for_frequency_range` already existed in
//! `gdaudio::effects` with a passing unit test, but neither class was
//! registered — so a `.tscn`/`.tres` bus layout could not name the effect by
//! type and the editor's effect picker (which filters on "inherits
//! AudioEffect") never offered it.

use gdobject::class_db::{
    class_exists, class_has_property, is_parent_class, property_get_revert, register_2d_classes,
};
use gdvariant::Variant;

/// `register_2d_classes` is the entry point that registers the audio effects.
fn registered() {
    register_2d_classes();
}

#[test]
fn spectrum_analyzer_classes_are_registered() {
    registered();

    assert!(
        class_exists("AudioEffectSpectrumAnalyzer"),
        "the analyzer must be registered or no bus layout can name it"
    );
    assert!(
        is_parent_class("AudioEffectSpectrumAnalyzer", "AudioEffect"),
        "the analyzer must inherit AudioEffect — that is what the effect-slot \
         type check and the editor's effect picker filter on"
    );

    assert!(
        class_exists("AudioEffectSpectrumAnalyzerInstance"),
        "the instance companion must be registered"
    );
    assert!(
        is_parent_class("AudioEffectSpectrumAnalyzerInstance", "AudioEffectInstance"),
        "the instance derives from AudioEffectInstance"
    );
    // The instance is NOT an insertable effect. Registering it under
    // AudioEffect would make the bus effect picker offer it as something you
    // can add to a chain, which Godot does not.
    assert!(
        !is_parent_class("AudioEffectSpectrumAnalyzerInstance", "AudioEffect"),
        "the instance must NOT inherit AudioEffect — it is a query companion, \
         not an insertable effect"
    );
}

#[test]
fn analyzer_properties_carry_godot_defaults() {
    registered();

    for (property, want) in [
        ("buffer_length", Variant::Float(2.0)),
        // `fft_size` is the FFTSize *enum*; its default 2 is FFT_SIZE_1024, not
        // a raw sample count — asserting 1024 here would be wrong.
        ("fft_size", Variant::Int(2)),
        ("tap_back_pos", Variant::Float(0.01)),
    ] {
        assert!(
            class_has_property("AudioEffectSpectrumAnalyzer", property),
            "{property} must be registered on the analyzer"
        );
        assert_eq!(
            property_get_revert("AudioEffectSpectrumAnalyzer", property),
            Some(want.clone()),
            "{property} must carry its documented Godot default"
        );
    }
}
