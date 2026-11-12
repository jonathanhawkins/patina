//! pat-s3rrv.142 — the audio-input classes resolve in ClassDB.
//!
//! Godot ground truth: `servers/register_server_types.cpp:169,211,215` (all
//! three plain `GDREGISTER_CLASS`), `servers/audio/effects/audio_effect_capture.cpp:64-80`
//! and `audio_effect_record.cpp:278-290` for the bindings, and the matching
//! `doc/classes/*.xml` for defaults.
//!
//! ⚠️ **Registration only — nothing captures audio.** There is no input
//! stream, no ring buffer, no `getUserMedia` backend, and `AudioServer` is not
//! in ClassDB at all, so its input half (`get_input_device_list`,
//! `get_input_mix_rate`, …) has nowhere to live. A green run here means a
//! `.tscn` naming one of these classes resolves and introspection answers
//! correctly. It does NOT mean a microphone works.

use gdobject::class_db::{
    can_instantiate, class_exists, class_has_method, get_method_list, get_property_list,
    register_all_classes,
};
use gdobject::is_parent_class;
use gdvariant::Variant;

fn method_of(class_name: &str, name: &str) -> Option<gdobject::class_db::MethodInfo> {
    get_method_list(class_name)
        .into_iter()
        .find(|m| m.name == name)
}

fn default_of(class_name: &str, property: &str) -> Option<Variant> {
    get_property_list(class_name)
        .into_iter()
        .find(|p| p.name == property)
        .map(|p| p.default_value)
}

#[test]
fn the_three_audio_input_classes_are_registered_under_their_godot_parents() {
    register_all_classes();
    assert!(is_parent_class("AudioStreamMicrophone", "AudioStream"));
    assert!(is_parent_class("AudioEffectCapture", "AudioEffect"));
    assert!(is_parent_class("AudioEffectRecord", "AudioEffect"));
    for class in [
        "AudioStreamMicrophone",
        "AudioEffectCapture",
        "AudioEffectRecord",
    ] {
        assert!(class_exists(class));
        assert!(
            can_instantiate(class),
            "{class} is GDREGISTER_CLASS upstream — not abstract"
        );
    }
}

/// `AudioStreamMicrophone` declares nothing of its own.
///
/// ⚠️ Asserted as "adds nothing to `AudioStream`" rather than "has no methods":
/// `get_method_list` walks the inheritance chain, so a bare length check would
/// count every inherited AudioStream method and pass for any implementation.
#[test]
fn the_microphone_stream_adds_nothing_of_its_own() {
    register_all_classes();
    assert_eq!(
        get_method_list("AudioStreamMicrophone").len(),
        get_method_list("AudioStream").len(),
        "AudioStreamMicrophone is the capture device wearing the AudioStream \
         interface — everything interesting about it lives on AudioServer, so \
         it must add no methods of its own"
    );
}

/// Capture's nine bound methods.
#[test]
fn audio_effect_capture_binds_its_nine_methods() {
    register_all_classes();
    for (name, argc) in [
        ("can_get_buffer", 1usize),
        ("get_buffer", 1),
        ("clear_buffer", 0),
        ("set_buffer_length", 1),
        ("get_buffer_length", 0),
        ("get_frames_available", 0),
        ("get_discarded_frames", 0),
        ("get_buffer_length_frames", 0),
        ("get_pushed_frames", 0),
    ] {
        let m = method_of("AudioEffectCapture", name)
            .unwrap_or_else(|| panic!("AudioEffectCapture.{name}() must be registered"));
        assert_eq!(m.argument_count, argc, "{name} takes {argc}");
    }
    assert_eq!(
        default_of("AudioEffectCapture", "buffer_length"),
        Some(Variant::Float(0.1)),
        "buffer_length defaults to 0.1s"
    );
}

/// 🔑 `get_buffer` hands back one Vector2 PER FRAME, not a flat float array.
///
/// ⚠️ A caller that read it as mono floats would take every other sample and
/// get audio at half speed — plausible-sounding output rather than an error.
#[test]
fn capture_returns_stereo_frames() {
    register_all_classes();
    let m = method_of("AudioEffectCapture", "get_buffer").expect("get_buffer");
    assert!(
        matches!(
            m.return_value.as_ref().map(|r| &r.default_value),
            Some(Variant::PackedVector2Array(_))
        ),
        "get_buffer returns PackedVector2Array — one Vector2 (L, R) per frame"
    );
}

/// 🔑 The two names on these classes that cannot be derived.
///
/// ⚠️ `set_buffer_length`'s bound parameter is **`buffer_length_seconds`**,
/// while the property it backs is `buffer_length` — the two disagree, so
/// deriving the argument name from the property gives the wrong one.
///
/// ⚠️ `is_recording_active`, not `get_recording_active`: the bool convention,
/// and the only accessor across both classes that does not start with `get_`.
#[test]
fn the_two_non_derivable_names_are_correct() {
    register_all_classes();
    let setter = method_of("AudioEffectCapture", "set_buffer_length").expect("setter");
    assert_eq!(
        setter.arguments.first().map(|a| a.name.as_str()),
        Some("buffer_length_seconds"),
        "🔑 the parameter is `buffer_length_seconds`, not `buffer_length` \
         (`audio_effect_capture.cpp:68`)"
    );

    assert!(class_has_method("AudioEffectRecord", "is_recording_active"));
    assert!(
        !class_has_method("AudioEffectRecord", "get_recording_active"),
        "🔑 the getter is `is_recording_active`; a `get_` spelling is the \
         obvious guess and matches nothing"
    );
}

/// 🔑 `AudioEffectRecord.format` defaults to 1 (16-Bit), not 0.
///
/// ⚠️ The enum is `AudioStreamWAV.Format` — "8-Bit,16-Bit,IMA ADPCM,Quite OK
/// Audio" — so a 0 default is not an error anywhere: it silently records 8-bit
/// audio, which sounds like a bad microphone rather than a wrong default.
#[test]
fn record_defaults_to_sixteen_bit() {
    register_all_classes();
    assert_eq!(
        default_of("AudioEffectRecord", "format"),
        Some(Variant::Int(1)),
        "🔑 1 == 16-Bit. 0 would be 8-Bit and would degrade every recording \
         without reporting anything"
    );
    let props = get_property_list("AudioEffectRecord");
    let format = props.iter().find(|p| p.name == "format").expect("format");
    assert_eq!(
        format.hint_string, "8-Bit,16-Bit,IMA ADPCM,Quite OK Audio",
        "the enum names come from AudioStreamWAV.Format"
    );

    for (name, argc) in [
        ("set_recording_active", 1usize),
        ("is_recording_active", 0),
        ("set_format", 1),
        ("get_format", 0),
        ("get_recording", 0),
    ] {
        let m = method_of("AudioEffectRecord", name)
            .unwrap_or_else(|| panic!("AudioEffectRecord.{name}() must be registered"));
        assert_eq!(m.argument_count, argc);
    }
}
