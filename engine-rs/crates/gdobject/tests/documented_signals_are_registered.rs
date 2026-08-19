//! pat-s3rrv.57: ClassDB reports the signals Godot documents.
//!
//! # The gap this closes
//!
//! Signal metadata was the thinnest layer of the registry: 144 of 376
//! documented signals, on 49 of the 116 classes that declare any. A script
//! calling `has_signal()`, an editor node dock listing connectable signals, or
//! a `connect()` that validates its target all read this table — and for two
//! thirds of the engine it answered "no such signal".
//!
//! # Why the doc XML is an exact oracle here
//!
//! [`SignalInfo`] carries a name and ordered argument NAMES, and
//! `upstream/godot/doc/classes/*.xml` states exactly those two things
//! (`<signal name=…>` / `<param index=… name=…>`). Unlike property defaults —
//! where the XML omits `default=` whenever the value is not a simple constant —
//! there is nothing to infer and nothing to invent.
//!
//! ⚠️ The assertions below name concrete signals transcribed from that XML
//! rather than a count, so this test fails if a name or an argument ORDER is
//! wrong, not merely if the total moves.

use gdobject::class_db::{class_has_signal, get_signal_list, register_all_classes};

/// The registry is global and seeded by `register_all_classes`. Without this
/// every assertion below would read an empty table and the negative checks
/// would pass vacuously.
fn signals(class_name: &str) -> Vec<(String, Vec<String>)> {
    register_all_classes();
    get_signal_list(class_name)
        .into_iter()
        .map(|s| (s.name, s.arguments))
        .collect()
}

fn args_of(class_name: &str, signal: &str) -> Vec<String> {
    signals(class_name)
        .into_iter()
        .find(|(n, _)| n == signal)
        .unwrap_or_else(|| panic!("{class_name} must declare the `{signal}` signal"))
        .1
}

/// ⚠️ **Argument order is the assertion that matters.** A signal's parameters
/// are positional — a handler declared `func _on(result, code, headers, body)`
/// binds by position, so a transcription that reorders them produces a table
/// that looks complete and connects every argument to the wrong parameter.
#[test]
fn a_signals_arguments_keep_their_documented_order() {
    assert_eq!(
        args_of("HTTPRequest", "request_completed"),
        ["result", "response_code", "headers", "body"],
        "HTTPRequest.request_completed(result, response_code, headers, body)"
    );
    assert_eq!(
        args_of("GraphEdit", "connection_request"),
        ["from_node", "from_port", "to_node", "to_port"],
        "the two node/port pairs must not swap"
    );
    assert_eq!(
        args_of("CollisionObject3D", "input_event"),
        ["camera", "event", "event_position", "normal", "shape_idx"],
        "the longest documented argument list in the engine"
    );
    assert_eq!(
        args_of("Area2D", "body_shape_entered"),
        ["body_rid", "body", "body_shape_index", "local_shape_index"],
        "⚠️ `body_shape_index` is the OTHER body's shape and \
         `local_shape_index` is this area's — swapping them is silent"
    );
}

/// A no-argument signal must register with an EMPTY argument list, not with a
/// guessed one. `SignalInfo::new` defaults to no arguments, so this pins that
/// the transcription did not invent parameters for the many signals Godot
/// documents with none.
#[test]
fn a_no_argument_signal_carries_no_arguments() {
    for (class_name, signal) in [
        ("Timer", "timeout"),
        ("AudioStreamPlayer", "finished"),
        ("AcceptDialog", "confirmed"),
        ("AcceptDialog", "canceled"),
        ("AnimationTree", "animation_player_changed"),
    ] {
        assert_eq!(
            args_of(class_name, signal),
            Vec::<String>::new(),
            "{class_name}.{signal} takes no arguments"
        );
    }
}

/// Breadth: the families that had NO signals at all before this pass.
#[test]
fn previously_signal_less_classes_now_declare_theirs() {
    // ⚠️ Seeds the registry, exactly as the `signals()` helper does. This test
    // reaches `class_has_signal` DIRECTLY rather than through that helper, so it
    // was the only test in the file that never populated ClassDB — and nextest
    // runs each test in its own process, so a sibling seeding the global
    // registry does not carry over.
    //
    // The failure that produced was maximally misleading: an empty table makes
    // `class_has_signal` answer false for everything, so it tripped on the FIRST
    // pair (`Timer` / `timeout`) and read as "Timer does not declare timeout" —
    // a signal that has been registered all along. Nothing was missing from
    // class_db.rs; all 13 names asserted below are present.
    register_all_classes();
    for (class_name, expected) in [
        ("Timer", &["timeout"][..]),
        ("HTTPRequest", &["request_completed"][..]),
        (
            "Area2D",
            &[
                "area_entered",
                "area_exited",
                "area_shape_entered",
                "area_shape_exited",
                "body_entered",
                "body_exited",
                "body_shape_entered",
                "body_shape_exited",
            ][..],
        ),
        (
            "GraphEdit",
            &["connection_request", "disconnection_request"][..],
        ),
    ] {
        for signal in expected {
            assert!(
                class_has_signal(class_name, signal),
                "{class_name} must declare `{signal}`"
            );
        }
    }
}

/// ⚠️ `get_signal_list` walks the inheritance chain, so a subclass reports its
/// ancestors' signals too. That is Godot's behaviour (`class_get_signal_list`
/// with `no_inheritance = false`), and it is what makes a script's
/// `has_signal("tree_entered")` work on any Node — but it also means a
/// per-class count would be measuring the whole chain, which is why nothing
/// here asserts one.
#[test]
fn a_subclass_reports_inherited_signals_too() {
    register_all_classes();

    assert!(
        class_has_signal("Timer", "tree_entered"),
        "Timer inherits Node's signals"
    );
    assert!(
        class_has_signal("Timer", "timeout"),
        "…and still declares its own"
    );
    // A signal no class declares is still absent — the chain walk must not
    // manufacture one.
    assert!(!class_has_signal("Timer", "totally_made_up_signal"));
}

/// The registration is additive metadata: registering the same class twice must
/// not duplicate a signal, or a node dock would list it twice and a
/// `connect()` validator would see two candidates for one name.
#[test]
fn no_class_declares_the_same_signal_twice() {
    register_all_classes();
    // Idempotent by contract — the editor re-enters this on every boot.
    register_all_classes();

    for class_name in [
        "Timer",
        "HTTPRequest",
        "Area2D",
        "Area3D",
        "GraphEdit",
        "AnimationTree",
        "AudioStreamPlayer",
        "CollisionObject3D",
    ] {
        let names: Vec<String> = signals(class_name).into_iter().map(|(n, _)| n).collect();
        let mut unique = names.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(
            names.len(),
            unique.len(),
            "{class_name} reports a duplicate signal: {names:?}"
        );
    }
}
