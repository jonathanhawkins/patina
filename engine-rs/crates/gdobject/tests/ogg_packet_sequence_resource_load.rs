//! pat-nzgdu.188 — the `OggPacketSequence` / `AudioStream` resource family in ClassDB.
//!
//! ⚠️ **The bead asks for two classes; the gap was four.** Registering
//! `OggPacketSequence` exposed that Patina had registered every
//! `AudioStream*Playback*` class — the runtime instances Godot hands out — but
//! **not one of the streams they play**. `AudioStream` and `AudioStreamOggVorbis`
//! were both absent, so a stream could be played but never named: the playback
//! side had no resource to point back at. Both are registered here, since the
//! bead's own acceptance criterion names `AudioStreamOggVorbis`.
//!
//! ⚠️ The api dump types `OggPacketSequence.packet_data` as
//! `typedarray::PackedByteArray` — a flat array of byte arrays. That is **wrong**:
//! `ADD_PROPERTY` in `modules/ogg/ogg_packet_sequence.cpp:131` declares
//! `Variant::ARRAY`, and the setter takes `TypedArray<Array>` over
//! `Vector<Vector<PackedByteArray>>` — an array of *pages*, each holding packets.
//! The dump's shape would collapse the page grouping the granule positions index
//! against.

use gdobject::class_db::{can_instantiate, is_abstract, register_audio_classes};
use gdobject::GodotObject;
use gdobject::{
    class_exists, class_has_method, get_class_info, get_method_list, get_property_list,
    instantiate, is_parent_class,
};
use gdvariant::Variant;

fn arity(class: &str, method: &str) -> usize {
    get_method_list(class)
        .iter()
        .rev()
        .find(|m| m.name == method)
        .unwrap_or_else(|| panic!("{class} has no method `{method}`"))
        .argument_count
}

fn own_methods(class: &str, base: &str) -> Vec<String> {
    let base_methods: Vec<String> = get_method_list(base).into_iter().map(|m| m.name).collect();
    get_method_list(class)
        .into_iter()
        .map(|m| m.name)
        .filter(|m| !base_methods.contains(m))
        .collect()
}

fn default_of(class: &str, name: &str) -> Variant {
    get_property_list(class)
        .into_iter()
        .rev()
        .find(|p| p.name == name)
        .unwrap_or_else(|| panic!("{class} has no property `{name}`"))
        .default_value
}

/// The acceptance criterion, verbatim: both classes registered under the right
/// bases, and the sequence exposing `packet_data` and `sampling_rate`.
#[test]
fn ogg_packet_sequence_resource_load() {
    register_audio_classes();

    assert!(class_exists("OggPacketSequence"));
    assert!(is_parent_class("OggPacketSequence", "Resource"));
    assert!(class_exists("OggPacketSequencePlayback"));
    assert!(is_parent_class("OggPacketSequencePlayback", "RefCounted"));

    // The sequence exposes its data, with Godot's defaults.
    assert_eq!(
        default_of("OggPacketSequence", "packet_data"),
        Variant::Array(Vec::new()),
    );
    assert_eq!(
        default_of("OggPacketSequence", "sampling_rate"),
        Variant::Float(0.0),
    );

    // Both are concrete: GDREGISTER_CLASS, not GDREGISTER_ABSTRACT_CLASS.
    assert!(!is_abstract("OggPacketSequence"));
    assert!(can_instantiate("OggPacketSequence"));
    let obj = instantiate("OggPacketSequence").expect("instantiates");
    assert_eq!(obj.get_class(), "OggPacketSequence");
    assert!(can_instantiate("OggPacketSequencePlayback"));
}

/// ⚠️ The property is `granule_positions`; its accessors say `packet`. And the
/// values are one per PAGE, not per packet.
#[test]
fn granule_positions_property_and_accessors_disagree_by_name_as_in_godot() {
    register_audio_classes();

    assert_eq!(
        default_of("OggPacketSequence", "granule_positions"),
        Variant::PackedInt64Array(Vec::new()),
        "PACKED_INT64_ARRAY per ogg_packet_sequence.cpp:132",
    );
    // The accessors are named after packets even though the property is not.
    assert!(class_has_method(
        "OggPacketSequence",
        "set_packet_granule_positions"
    ));
    assert!(class_has_method(
        "OggPacketSequence",
        "get_packet_granule_positions"
    ));
    assert!(
        !class_has_method("OggPacketSequence", "set_granule_positions"),
        "Godot binds no accessor matching the property name",
    );
}

/// The sequence's full bound surface — 7 methods, no more.
#[test]
fn ogg_packet_sequence_binds_godots_exact_surface() {
    register_audio_classes();

    let mut own = own_methods("OggPacketSequence", "Resource");
    own.sort();
    assert_eq!(
        own,
        vec![
            "get_length",
            "get_packet_data",
            "get_packet_granule_positions",
            "get_sampling_rate",
            "set_packet_data",
            "set_packet_granule_positions",
            "set_sampling_rate",
        ],
        "Godot binds exactly these seven",
    );
    assert_eq!(arity("OggPacketSequence", "set_packet_data"), 1);
    assert_eq!(arity("OggPacketSequence", "get_length"), 0);

    // ⚠️ `instantiate_playback()` exists on the C++ class but is deliberately NOT
    // in _bind_methods — it must not appear here.
    assert!(!class_has_method(
        "OggPacketSequence",
        "instantiate_playback"
    ));
}

/// ⚠️ The playback binds nothing at all — Godot hands it out as an opaque handle.
#[test]
fn ogg_packet_sequence_playback_binds_nothing() {
    register_audio_classes();

    assert_eq!(
        own_methods("OggPacketSequencePlayback", "RefCounted"),
        Vec::<String>::new(),
        "it is an opaque handle",
    );
    let base: Vec<String> = get_property_list("RefCounted")
        .into_iter()
        .map(|p| p.name)
        .collect();
    let own: Vec<String> = get_property_list("OggPacketSequencePlayback")
        .into_iter()
        .map(|p| p.name)
        .filter(|p| !base.contains(p))
        .collect();
    assert_eq!(own, Vec::<String>::new(), "and exposes no properties");
}

/// ⚠️ The missing stream side. `AudioStreamOggVorbis` is the class the importer
/// has been naming all along without it existing.
#[test]
fn the_audiostream_resource_side_exists_and_references_the_sequence() {
    register_audio_classes();

    assert!(class_exists("AudioStream"));
    assert!(
        is_parent_class("AudioStream", "Resource"),
        "the stream is a Resource; its playback is a RefCounted",
    );
    // GDREGISTER_CLASS at servers/register_server_types.cpp:166 — a base, but
    // concrete, unlike AudioStreamPlayback above it.
    assert!(!is_abstract("AudioStream"));
    assert!(class_has_method("AudioStream", "instantiate_playback"));

    assert!(class_exists("AudioStreamOggVorbis"));
    assert_eq!(
        get_class_info("AudioStreamOggVorbis")
            .expect("registered")
            .parent_class,
        "AudioStream",
    );
    assert!(is_parent_class("AudioStreamOggVorbis", "Resource"));
    assert!(can_instantiate("AudioStreamOggVorbis"));

    // It references the sequence — the link the bead is about.
    assert!(class_has_method(
        "AudioStreamOggVorbis",
        "set_packet_sequence"
    ));
    assert!(class_has_method(
        "AudioStreamOggVorbis",
        "get_packet_sequence"
    ));
    assert_eq!(
        default_of("AudioStreamOggVorbis", "packet_sequence"),
        Variant::Nil,
        "no sequence attached by default",
    );
}

/// ⚠️ `bar_beats` defaults to 4, not 0 — a bar is four beats unless told otherwise.
#[test]
fn audiostream_ogg_vorbis_defaults_match_the_doc_xml() {
    register_audio_classes();

    assert_eq!(
        default_of("AudioStreamOggVorbis", "bar_beats"),
        Variant::Int(4),
        "4 — the value a zero-default would silently get wrong",
    );
    assert_eq!(
        default_of("AudioStreamOggVorbis", "beat_count"),
        Variant::Int(0)
    );
    assert_eq!(
        default_of("AudioStreamOggVorbis", "bpm"),
        Variant::Float(0.0)
    );
    assert_eq!(
        default_of("AudioStreamOggVorbis", "loop"),
        Variant::Bool(false)
    );
    assert_eq!(
        default_of("AudioStreamOggVorbis", "loop_offset"),
        Variant::Float(0.0)
    );

    // `loop` is a GDScript keyword, so Godot binds has_loop/set_loop rather than
    // the is_/get_ pair the property name would suggest.
    assert!(class_has_method("AudioStreamOggVorbis", "has_loop"));
    assert!(!class_has_method("AudioStreamOggVorbis", "get_loop"));
}

/// Registration must not mint new ClassIds on a second call.
#[test]
fn registration_is_idempotent() {
    let ids = || -> Vec<_> {
        [
            "OggPacketSequence",
            "OggPacketSequencePlayback",
            "AudioStream",
            "AudioStreamOggVorbis",
        ]
        .iter()
        .map(|c| get_class_info(c).expect("registered").class_id)
        .collect()
    };

    register_audio_classes();
    let first = ids();
    register_audio_classes();
    assert_eq!(
        first,
        ids(),
        "re-registering must not mint new ClassIds — a guard is missing if it does",
    );
}
