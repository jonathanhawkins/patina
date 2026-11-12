//! pat-s3rrv.143 — the composite `AudioStream` types resolve in ClassDB.
//!
//! Godot ground truth: `doc/classes/AudioStreamGenerator.xml`,
//! `AudioStreamPolyphonic.xml`, `AudioStreamRandomizer.xml`, and
//! `modules/interactive_music/doc_classes/{AudioStreamPlaylist,AudioStreamSynchronized}.xml`.
//!
//! ⚠️ **`AudioStreamGeneratorPlayback` was already registered while
//! `AudioStreamGenerator` was not** — the product existed and its producer did
//! not, so `get_stream_playback()` had a registered return type and no class it
//! could be called on.
//!
//! ⚠️ **Registration only.** The generator and a polyphonic voice pool are
//! implemented and unit-tested in `gdaudio::decode`, but nothing outside that
//! crate references them — no `get_stream_playback()` on a player, no script
//! binding for `push_frame`. A green run here means these classes resolve, not
//! that procedural audio plays.
//!
//! `AudioStreamInteractive` is deliberately NOT registered here: it carries 19
//! methods and a clip-transition matrix, and the bead's own approach stages it
//! last.

use gdobject::class_db::{
    can_instantiate, class_has_property, get_method_list, get_property_list, register_all_classes,
};
use gdobject::{class_exists, is_parent_class};
use gdvariant::Variant;

fn default_of(class_name: &str, property: &str) -> Option<Variant> {
    get_property_list(class_name)
        .into_iter()
        .find(|p| p.name == property)
        .map(|p| p.default_value)
}

fn accessors(class_name: &str, property: &str) -> (String, String) {
    (
        gdobject::class_db::get_property_getter(class_name, property),
        gdobject::class_db::get_property_setter(class_name, property),
    )
}

fn arity(class_name: &str, method: &str) -> Option<usize> {
    get_method_list(class_name)
        .into_iter()
        .find(|m| m.name == method)
        .map(|m| m.argument_count)
}

#[test]
fn the_composite_streams_are_registered_under_audio_stream() {
    register_all_classes();
    for class in [
        "AudioStreamGenerator",
        "AudioStreamPolyphonic",
        "AudioStreamRandomizer",
        "AudioStreamPlaylist",
        "AudioStreamSynchronized",
    ] {
        assert!(class_exists(class), "{class} must be registered");
        assert!(is_parent_class(class, "AudioStream"), "{class} is a stream");
        assert!(can_instantiate(class));
    }
    // The asymmetry this bead closes: the playback half was already there.
    assert!(
        class_exists("AudioStreamGeneratorPlayback"),
        "the playback class was registered before its stream — both must exist \
         for get_stream_playback() to be expressible"
    );
}

/// 🔑 Two defaults that are not the obvious zero.
#[test]
fn generator_and_polyphonic_defaults_match_godot() {
    register_all_classes();
    assert_eq!(
        default_of("AudioStreamGenerator", "mix_rate_mode"),
        Some(Variant::Int(2)),
        "🔑 mix_rate_mode defaults to 2, not 0 — a 0 default silently changes \
         how every generated stream resamples"
    );
    assert_eq!(
        default_of("AudioStreamGenerator", "mix_rate"),
        Some(Variant::Float(44100.0))
    );
    assert_eq!(
        default_of("AudioStreamGenerator", "buffer_length"),
        Some(Variant::Float(0.5))
    );
    assert_eq!(
        default_of("AudioStreamPolyphonic", "polyphony"),
        Some(Variant::Int(32)),
        "🔑 32 voices, not 0 — a 0 default is a stream that can never play"
    );
}

/// 🔑 `random_pitch` is a MULTIPLIER (1.0) and `random_pitch_semitones` an
/// OFFSET (0.0).
///
/// ⚠️ Two neighbouring properties whose neutral values differ. Giving either
/// the other's default detunes every randomized sound — and 1.0 semitones or a
/// 0.0 pitch multiplier both look like plausible numbers in an inspector.
#[test]
fn randomizer_pitch_defaults_are_not_interchangeable() {
    register_all_classes();
    assert_eq!(
        default_of("AudioStreamRandomizer", "random_pitch"),
        Some(Variant::Float(1.0)),
        "a multiplier is neutral at 1.0"
    );
    assert_eq!(
        default_of("AudioStreamRandomizer", "random_pitch_semitones"),
        Some(Variant::Float(0.0)),
        "an offset is neutral at 0.0"
    );
    assert_eq!(
        default_of("AudioStreamRandomizer", "random_volume_offset_db"),
        Some(Variant::Float(0.0))
    );
    assert_eq!(
        default_of("AudioStreamRandomizer", "playback_mode"),
        Some(Variant::Int(0))
    );
}

/// 🔑 `loop` and `shuffle` disagree on BOTH the default and the getter prefix,
/// inside one class.
///
/// ⚠️ `loop` defaults to **true** with the getter **`has_loop`**; `shuffle`
/// defaults to false with a plain `get_shuffle`. Neither can be derived from
/// the other, and they sit adjacent in `AudioStreamPlaylist.xml`, so copying
/// one onto the other is the obvious mistake.
#[test]
fn playlist_loop_and_shuffle_share_nothing() {
    register_all_classes();
    assert_eq!(
        default_of("AudioStreamPlaylist", "loop"),
        Some(Variant::Bool(true)),
        "a playlist loops by default"
    );
    assert_eq!(
        default_of("AudioStreamPlaylist", "shuffle"),
        Some(Variant::Bool(false)),
        "…but does not shuffle"
    );
    assert_eq!(
        accessors("AudioStreamPlaylist", "loop"),
        ("has_loop".to_string(), "set_loop".to_string()),
        "🔑 the getter is `has_loop`, not `get_loop`"
    );
    assert_eq!(
        accessors("AudioStreamPlaylist", "shuffle"),
        ("get_shuffle".to_string(), "set_shuffle".to_string()),
        "…while its neighbour uses the plain `get_` form"
    );
    assert_eq!(
        default_of("AudioStreamPlaylist", "fade_time"),
        Some(Variant::Float(0.3))
    );
}

/// 🔑 `streams_count` is PLURAL on Randomizer and SINGULAR on the other two.
///
/// ⚠️ One letter, three classes, no rule — a `.tres` written against the wrong
/// spelling sets a property that does not exist and the stream list stays
/// empty.
#[test]
fn the_count_property_is_spelled_differently_per_class() {
    register_all_classes();
    assert!(
        class_has_property("AudioStreamRandomizer", "streams_count"),
        "🔑 Randomizer spells it PLURAL"
    );
    assert!(
        !class_has_property("AudioStreamRandomizer", "stream_count"),
        "…and must not also answer to the singular"
    );
    for class in ["AudioStreamPlaylist", "AudioStreamSynchronized"] {
        assert!(
            class_has_property(class, "stream_count"),
            "🔑 {class} spells it SINGULAR"
        );
        assert!(!class_has_property(class, "streams_count"));
    }
}

/// The bound method surfaces, with arities and the two renamed parameters.
#[test]
fn the_stream_list_methods_match_upstream() {
    register_all_classes();
    for (method, argc) in [
        ("add_stream", 3usize),
        ("get_stream", 1),
        ("get_stream_probability_weight", 1),
        ("move_stream", 2),
        ("remove_stream", 1),
        ("set_stream", 2),
        ("set_stream_probability_weight", 2),
    ] {
        assert_eq!(
            arity("AudioStreamRandomizer", method),
            Some(argc),
            "AudioStreamRandomizer.{method}() takes {argc}"
        );
    }
    for (method, argc) in [
        ("get_bpm", 0usize),
        ("get_list_stream", 1),
        ("set_list_stream", 2),
    ] {
        assert_eq!(arity("AudioStreamPlaylist", method), Some(argc));
    }
    for (method, argc) in [
        ("get_sync_stream", 1usize),
        ("get_sync_stream_volume", 1),
        ("set_sync_stream", 2),
        ("set_sync_stream_volume", 2),
    ] {
        assert_eq!(arity("AudioStreamSynchronized", method), Some(argc));
    }

    // ⚠️ The stream parameter is named `audio_stream` on these two, not
    // `stream` as on Randomizer — asserted because a caller reading the
    // registered signature would otherwise be told the wrong name.
    let m = get_method_list("AudioStreamPlaylist")
        .into_iter()
        .find(|m| m.name == "set_list_stream")
        .expect("set_list_stream");
    assert_eq!(
        m.arguments.get(1).map(|a| a.name.as_str()),
        Some("audio_stream"),
        "🔑 `audio_stream`, not `stream`"
    );
}
