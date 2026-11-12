//! pat-mgixk.16 acceptance: `project.godot`'s `[input]` section round-trips.
//!
//! Godot stores every input action in `project.godot` as
//! `jump={"deadzone":0.3,"events":[Object(InputEventKey,…)]}` and
//! `InputMap::load_from_project_settings` reads it back
//! (`core/input/input_map.cpp:320`). Patina had **no** reader or writer for
//! that format: `grep '\[input\]|InputEventKey'` over `gdeditor` found
//! nothing, so the editor never loaded a project's own actions and never wrote
//! an edit back. The pre-existing `parse_input_events` only produced display
//! strings like `"InputEventKey:keycode=65"`, which cannot be round-tripped.
//!
//! ⚠️ These live **here**, under `tests/`, and not in a `#[cfg(test)] mod
//! tests` inside `project.rs`, because the verifier's red check reverts the
//! implementation file to HEAD and re-runs the test. An in-file test module is
//! reverted *along with* the code it covers, so HEAD's own tests run, pass, and
//! the check correctly concludes the test never exercised the change. A test
//! that cannot fail without the fix proves nothing about it.
//!
//! Everything below goes through the crate's public API only.

use std::path::Path;

use gdresource::project::{
    parse_input_action, serialize_input_action, GodotProjectFile, InputAction, InputActionEvent,
    ProjectLoader, DEFAULT_DEADZONE,
};

/// The exact text Godot 4 writes for an action bound to Space plus joypad
/// button 0 with a 0.3 deadzone — the shape pat-mgixk.16's acceptance names.
const JUMP_ACTION: &str = concat!(
    "{\n\"deadzone\": 0.3,\n\"events\": [",
    "Object(InputEventKey,\"resource_local_to_scene\":false,\"resource_name\":\"\",",
    "\"device\":-1,\"window_id\":0,\"alt_pressed\":false,\"shift_pressed\":false,",
    "\"ctrl_pressed\":false,\"meta_pressed\":false,\"pressed\":false,\"keycode\":0,",
    "\"physical_keycode\":32,\"key_label\":0,\"unicode\":0,\"location\":0,\"echo\":false,",
    "\"script\":null), ",
    "Object(InputEventJoypadButton,\"resource_local_to_scene\":false,\"resource_name\":\"\",",
    "\"device\":-1,\"button_index\":0,\"pressure\":0.0,\"pressed\":false,\"script\":null)",
    "]\n}"
);

/// ⚠️ The deadzone key is `"deadzone"`, one word. Godot 3 spelled it
/// `"dead_zone"` and `parse_input_events`' doc comment still says so; Godot
/// 4 reads `action["deadzone"]` (input_map.cpp:320). A reader matching only
/// the old spelling silently hands back the default for every action, which
/// looks like a working parser.
#[test]
fn input_action_parses_deadzone_and_typed_events() {
    let action = parse_input_action(JUMP_ACTION);
    assert!(
        (action.deadzone - 0.3).abs() < 1e-6,
        "deadzone should be read from the file, got {}",
        action.deadzone
    );
    assert_eq!(action.events.len(), 2, "both events must survive");
    assert_eq!(
        action.events[0],
        InputActionEvent::Key {
            keycode: 0,
            physical_keycode: 32,
            ctrl: false,
            shift: false,
            alt: false,
            meta: false,
        },
        "a physical Space binding keeps keycode 0 and physical_keycode 32 — \
         collapsing the two is how a layout-independent binding becomes \
         layout-dependent"
    );
    assert_eq!(
        action.events[1],
        InputActionEvent::JoypadButton { button_index: 0 },
        "the joypad button must stay a joypad button, not become a key"
    );
}

/// An action with no `"deadzone"` takes Godot 4's default, which is 0.2 —
/// NOT the 0.5 Godot 3 used (`input_map.h:55`).
#[test]
fn input_action_without_deadzone_takes_the_godot4_default() {
    let action = parse_input_action("{\n\"events\": []\n}");
    assert!((action.deadzone - DEFAULT_DEADZONE).abs() < 1e-6);
    assert!(
        (DEFAULT_DEADZONE - 0.2).abs() < 1e-6,
        "Godot 4 default is 0.2"
    );
}

/// Modifiers and the other three event classes survive a round trip.
#[test]
fn input_action_round_trips_through_serialize() {
    let original = InputAction {
        deadzone: 0.35,
        events: vec![
            InputActionEvent::Key {
                keycode: 65,
                physical_keycode: 0,
                ctrl: true,
                shift: false,
                alt: true,
                meta: false,
            },
            InputActionEvent::MouseButton { button_index: 1 },
            InputActionEvent::JoypadButton { button_index: 3 },
            // The SIGN selects the half-axis, so this must not be read back
            // as +1.0.
            InputActionEvent::JoypadMotion {
                axis: 0,
                axis_value: -1.0,
            },
        ],
    };
    let text = serialize_input_action(&original);
    let reparsed = parse_input_action(&text);
    assert_eq!(reparsed, original, "serialized text was:\n{text}");
}

/// The written text has to be the format Godot reads, not merely one this
/// parser accepts — a project the editor saves must still open in Godot.
#[test]
fn serialized_input_action_matches_godots_shape() {
    let text = serialize_input_action(&InputAction {
        deadzone: 0.3,
        events: vec![
            InputActionEvent::Key {
                keycode: 0,
                physical_keycode: 32,
                ctrl: false,
                shift: false,
                alt: false,
                meta: false,
            },
            InputActionEvent::JoypadButton { button_index: 0 },
        ],
    });
    for needle in [
        "\"deadzone\": 0.3",
        "\"events\": [",
        "Object(InputEventKey,",
        "\"physical_keycode\":32",
        "Object(InputEventJoypadButton,",
        "\"button_index\":0",
        // Fields Patina does not interpret but Godot writes; omitting them
        // parses fine here and is the tempting shortcut.
        "\"resource_local_to_scene\":false",
        "\"script\":null",
    ] {
        assert!(
            text.contains(needle),
            "serialized action is missing `{needle}`:\n{text}"
        );
    }
}

/// An `Object(...)` class this type does not model is preserved verbatim,
/// so a read/write cycle cannot silently delete a binding.
#[test]
fn unmodelled_event_class_survives_a_round_trip() {
    let raw = "{\n\"deadzone\": 0.2,\n\"events\": [Object(InputEventMIDI,\"device\":-1,\"channel\":2,\"script\":null)]\n}";
    let action = parse_input_action(raw);
    assert_eq!(action.events.len(), 1);
    match &action.events[0] {
        InputActionEvent::Other(kept) => {
            assert!(kept.contains("InputEventMIDI"), "got {kept}");
            assert!(
                kept.contains("\"channel\":2"),
                "fields must survive: {kept}"
            );
        }
        other => panic!("an unmodelled class should be kept as Other, got {other:?}"),
    }
    assert!(serialize_input_action(&action).contains("InputEventMIDI"));
}

/// The write-back path: an action set on the project file appears in
/// `to_text()` under `[input]` and parses back out through `ProjectLoader`.
/// This is the direction that did not exist at all.
#[test]
fn input_action_written_into_project_text_is_read_back() {
    let mut pf =
        GodotProjectFile::parse("[application]\nconfig/name=\"T\"\n\n[input]\n").expect("parses");
    pf.set_input_action(
        "jump",
        &InputAction {
            deadzone: 0.3,
            events: vec![InputActionEvent::JoypadButton { button_index: 0 }],
        },
    );
    let text = pf.to_text();
    assert!(
        text.contains("jump="),
        "the action must be emitted:\n{text}"
    );

    let loader = ProjectLoader::from_text(Path::new("/tmp/patina-input-test"), &text)
        .expect("the written project must parse again");
    let actions = loader.get_input_actions();
    let jump = actions.get("jump").expect("jump action round-trips");
    assert!((jump.deadzone - 0.3).abs() < 1e-6);
    assert_eq!(
        jump.events,
        vec![InputActionEvent::JoypadButton { button_index: 0 }]
    );

    assert!(pf.remove_input_action("jump"));
    assert!(!pf.remove_input_action("jump"), "removal is idempotent");
}
