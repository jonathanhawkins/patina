//! pat-mgixk.16 acceptance: the editor's input map and `project.godot` convert
//! losslessly in both directions.
//!
//! `gdeditor::input_map::InputEvent` had only `Key(String)`, `MouseButton` and
//! `JoyButton` — no modifiers and no axis — and the only bridge was the
//! `to_runtime_map`/`from_runtime_map` pair, which wrapped every binding as
//! `Key(String)` on the way in and **dropped** every non-Key event on the way
//! out. So opening a project and saving it back rewrote a mouse binding as a
//! key and deleted a joypad axis.
//!
//! ⚠️ Under `tests/` rather than a `#[cfg(test)] mod tests` inside
//! `input_map.rs` on purpose: the red check reverts the implementation file to
//! HEAD, which would revert an in-file test module along with it, leaving
//! HEAD's own tests to run and pass.

use gdeditor::input_map::{InputEvent, InputMap};
use gdresource::project::InputActionEvent as ProjectEvent;

/// ⚠️ **The guarantee this bridge exists for.** Every event type survives a
/// trip out to `project.godot`'s shape and back. The old
/// `to_runtime_map`/`from_runtime_map` pair could not do this: it wrapped
/// every binding as `Key(String)` on the way in and DROPPED every non-Key
/// event on the way out, so opening a project and saving it rewrote a mouse
/// binding as a key and deleted a joypad axis (pat-mgixk.16).
#[test]
fn project_actions_round_trip_every_event_type() {
    let mut map = InputMap::new();
    assert!(map.add_action("jump"));
    assert!(map.set_deadzone("jump", 0.3));
    for event in [
        InputEvent::KeyEvent {
            keycode: 0,
            physical_keycode: 32,
            ctrl: true,
            shift: false,
            alt: true,
            meta: false,
        },
        InputEvent::MouseButton(1),
        InputEvent::JoyButton(0),
        // The sign picks the half-axis; +1.0 would be a different binding.
        InputEvent::JoyAxis {
            axis: 0,
            axis_value: -1.0,
        },
        InputEvent::Other(
            "Object(InputEventMIDI,\"device\":-1,\"channel\":2,\"script\":null)".to_string(),
        ),
    ] {
        assert!(map.bind("jump", event));
    }

    let project = map.to_project_actions();
    let back = InputMap::from_project_actions(&project);

    assert_eq!(back, map, "the map must survive the round trip unchanged");
    assert_eq!(
        back.deadzone("jump"),
        Some(0.3),
        "the deadzone must survive — it used to be dropped entirely"
    );
    assert_eq!(
        back.events("jump").map(|events| events.len()),
        Some(5),
        "no event may be lost, including the class Patina does not model"
    );
}

/// A key bound by NAME still reaches `project.godot` as a real numeric
/// keycode. Godot has no "Space" string in the file — it writes 32.
#[test]
fn a_named_key_converts_to_godots_numeric_keycode() {
    let event = InputMap::to_project_event(&InputEvent::Key("Space".into()))
        .expect("Space is a key gdplatform knows");
    assert_eq!(
        event,
        ProjectEvent::Key {
            // Logical, not physical: a name-only binding cannot say which,
            // and Godot's own default for a name is the logical keycode.
            keycode: 32,
            physical_keycode: 0,
            ctrl: false,
            shift: false,
            alt: false,
            meta: false,
        }
    );
    // ...and a name nothing recognises is reported rather than silently
    // written as keycode 0, which would be an unbindable event.
    assert!(InputMap::to_project_event(&InputEvent::Key("NoSuchKey".into())).is_none());
}

/// Godot 4's default deadzone is 0.2, not Godot 3's 0.5
/// (`core/input/input_map.h:55`).
#[test]
fn new_actions_take_the_godot4_default_deadzone() {
    let mut map = InputMap::new();
    map.add_action("fire");
    assert_eq!(map.deadzone("fire"), Some(0.2));
    assert!(
        (gdresource::project::DEFAULT_DEADZONE - 0.2).abs() < 1e-6,
        "the editor default must track the gdresource constant, not a copy"
    );
}
