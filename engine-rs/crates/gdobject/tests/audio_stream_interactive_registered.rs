//! pat-s3rrv.144 — `AudioStreamInteractive` completes the container-stream family.
//!
//! Godot ground truth:
//! `modules/interactive_music/doc_classes/AudioStreamInteractive.xml`.
//!
//! ⚠️ **Its doc XML is NOT under `doc/classes/`** — it lives in the
//! `interactive_music` module's own `doc_classes/`, so a sweep of the main doc
//! directory reports this class as undocumented and easily as nonexistent.
//!
//! The other four container streams (`Randomizer`, `Playlist`, `Synchronized`,
//! and the `Microphone`) were registered under pat-s3rrv.143 and .142; this is
//! the last one, and it is the only one with a transition MATRIX rather than a
//! flat stream list — hence 19 methods, four enums and a `CLIP_ANY` wildcard.
//!
//! ⚠️ Registration only: no container stream mixes anything yet.

use gdobject::class_db::{
    can_instantiate, class_get_integer_constant, class_has_enum, get_enum_constants,
    get_method_list, get_property_list, register_all_classes,
};
use gdobject::{class_exists, is_parent_class};
use gdvariant::Variant;

fn method(name: &str) -> gdobject::class_db::MethodInfo {
    get_method_list("AudioStreamInteractive")
        .into_iter()
        .find(|m| m.name == name)
        .unwrap_or_else(|| panic!("AudioStreamInteractive.{name}() must be registered"))
}

#[test]
fn interactive_is_registered_and_completes_the_container_family() {
    register_all_classes();
    assert!(class_exists("AudioStreamInteractive"));
    assert!(is_parent_class("AudioStreamInteractive", "AudioStream"));
    assert!(can_instantiate("AudioStreamInteractive"));

    // The whole family this bead names must now resolve.
    for class in [
        "AudioStreamRandomizer",
        "AudioStreamPlaylist",
        "AudioStreamInteractive",
        "AudioStreamSynchronized",
        "AudioStreamMicrophone",
    ] {
        assert!(class_exists(class), "{class} must be registered");
        assert!(
            is_parent_class(class, "AudioStream"),
            "{class} is an AudioStream in Godot"
        );
    }
    // Their playback companions were registered long before the resources —
    // the half-port this bead exists to close.
    for playback in [
        "AudioStreamPlaybackInteractive",
        "AudioStreamPlaybackPlaylist",
        "AudioStreamPlaybackSynchronized",
    ] {
        assert!(class_exists(playback));
    }
}

/// 🔑 `CLIP_ANY` is **-1**, not 0.
///
/// ⚠️ It is the "from any clip" wildcard in a transition. Registered as 0 it
/// would silently mean *clip index 0* — a real clip — so every wildcard
/// transition would attach to the first clip instead of all of them, with
/// nothing reporting an error.
#[test]
fn clip_any_is_negative_one() {
    register_all_classes();
    assert_eq!(
        class_get_integer_constant("AudioStreamInteractive", "CLIP_ANY"),
        Some(-1),
        "🔑 CLIP_ANY is the wildcard; 0 is a valid clip index and would bind \
         every 'any' transition to clip 0"
    );
}

/// The four enums and their constants.
#[test]
fn the_transition_enums_are_registered() {
    register_all_classes();
    for (name, expected) in [
        (
            "TransitionFromTime",
            vec![
                "TRANSITION_FROM_TIME_IMMEDIATE",
                "TRANSITION_FROM_TIME_NEXT_BEAT",
                "TRANSITION_FROM_TIME_NEXT_BAR",
                "TRANSITION_FROM_TIME_END",
            ],
        ),
        (
            "TransitionToTime",
            vec![
                "TRANSITION_TO_TIME_SAME_POSITION",
                "TRANSITION_TO_TIME_START",
            ],
        ),
        (
            "FadeMode",
            vec![
                "FADE_DISABLED",
                "FADE_IN",
                "FADE_OUT",
                "FADE_CROSS",
                "FADE_AUTOMATIC",
            ],
        ),
        (
            "AutoAdvanceMode",
            vec![
                "AUTO_ADVANCE_DISABLED",
                "AUTO_ADVANCE_ENABLED",
                "AUTO_ADVANCE_RETURN_TO_HOLD",
            ],
        ),
    ] {
        assert!(
            class_has_enum("AudioStreamInteractive", name),
            "enum {name} must be registered"
        );
        assert_eq!(
            get_enum_constants("AudioStreamInteractive", name),
            expected,
            "{name}'s constants must match the doc XML, in declaration order"
        );
    }
    // Values, not just names: FADE_AUTOMATIC is 4, and an off-by-one across the
    // fade modes would swap cross-fade for out-fade silently.
    assert_eq!(
        class_get_integer_constant("AudioStreamInteractive", "FADE_AUTOMATIC"),
        Some(4)
    );
    assert_eq!(
        class_get_integer_constant("AudioStreamInteractive", "FADE_CROSS"),
        Some(3)
    );
}

/// 🔑 `add_transition` takes NINE arguments, in an order that cannot be checked
/// by types.
///
/// ⚠️ `from_clip`, `to_clip`, `from_time`, `to_time`, `fade_mode` and
/// `filler_clip` are all ints; `use_filler_clip` and `hold_previous` are both
/// bools. Swap any same-typed pair and every call site still typechecks — the
/// music simply transitions wrongly. The registered argument NAMES are the only
/// thing that pins the order, so they are asserted in full.
#[test]
fn add_transition_argument_order_is_pinned_by_name() {
    register_all_classes();
    let m = method("add_transition");
    assert_eq!(m.argument_count, 9, "nine arguments");
    let names: Vec<&str> = m.arguments.iter().map(|a| a.name.as_str()).collect();
    assert_eq!(
        names,
        vec![
            "from_clip",
            "to_clip",
            "from_time",
            "to_time",
            "fade_mode",
            "fade_beats",
            "use_filler_clip",
            "filler_clip",
            "hold_previous",
        ],
        "🔑 six of the nine are interchangeable by type — only the order is \
         load-bearing"
    );
}

/// The remaining eighteen methods, and two names that cannot be derived.
///
/// ⚠️ `get_clip_name` returns a **StringName**, not a String — clip names are
/// interned identifiers, and a String return would compare unequal to the
/// StringName a script holds.
///
/// ⚠️ The two transition predicates are `is_transition_*`, not
/// `get_transition_*` — unlike the six `get_transition_*` accessors beside
/// them, so the prefix cannot be inferred from its neighbours.
#[test]
fn the_clip_and_transition_methods_match_upstream() {
    register_all_classes();
    for (name, argc) in [
        ("erase_transition", 2usize),
        ("get_clip_auto_advance", 1),
        ("get_clip_auto_advance_next_clip", 1),
        ("get_clip_name", 1),
        ("get_clip_stream", 1),
        ("get_transition_fade_beats", 2),
        ("get_transition_fade_mode", 2),
        ("get_transition_filler_clip", 2),
        ("get_transition_from_time", 2),
        ("get_transition_list", 0),
        ("get_transition_to_time", 2),
        ("has_transition", 2),
        ("is_transition_holding_previous", 2),
        ("is_transition_using_filler_clip", 2),
        ("set_clip_auto_advance", 2),
        ("set_clip_auto_advance_next_clip", 2),
        ("set_clip_name", 2),
        ("set_clip_stream", 2),
    ] {
        assert_eq!(
            method(name).argument_count,
            argc,
            "AudioStreamInteractive.{name}() takes {argc}"
        );
    }

    assert!(
        matches!(
            method("get_clip_name")
                .return_value
                .as_ref()
                .map(|r| &r.default_value),
            Some(Variant::StringName(_))
        ),
        "🔑 get_clip_name returns StringName — a String would not compare equal \
         to the interned name a script holds"
    );

    let props = get_property_list("AudioStreamInteractive");
    for member in ["clip_count", "initial_clip"] {
        assert!(
            props.iter().any(|p| p.name == member),
            "{member} must be registered"
        );
    }
}
