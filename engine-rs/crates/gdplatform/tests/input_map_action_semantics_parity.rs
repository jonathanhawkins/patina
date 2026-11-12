//! pat-0q2kx.89 — `InputMap` action-matching semantics match Godot.
//!
//! The implementation for this bead had already landed when I picked it up
//! (signed axis directions, the deadzone strength remap, modifier-carrying key
//! bindings, 72 builtin `ui_*` actions, joypad parsing, and an `InputMap`
//! script singleton). **What was missing was this test** — the acceptance names
//! `input_map_action_semantics_parity` and nothing by that name existed, so the
//! semantics were unpinned and free to regress silently.
//!
//! Assertions go through `InputMap::action_match`, which returns Godot's
//! `action_match` out-parameters (`pressed` / `strength`) directly. That is the
//! function these clauses are about; routing through `InputState` would put
//! event plumbing between the test and the behaviour under test.
//!
//! The clause requiring `InputMap.add_action` **from a GDScript scene script**
//! is not here: it needs the interpreter in `gdscript-interop`, a crate that
//! depends on this one, so it belongs in a test there. Called out in the bead
//! report rather than silently dropped.
//!
//! Sibling coverage under a different name:
//! `input_map_default_ui_actions_complete.rs` covers the builtin `ui_*` table,
//! including that `ui_focus_prev` is Shift+Tab — I dropped a duplicate of that
//! assertion rather than pin the same behaviour twice.

use gdplatform::input::{ActionBinding, GamepadAxis, InputEvent, InputMap, Key};

/// A left-stick X deflection.
fn axis(value: f32) -> InputEvent {
    InputEvent::GamepadAxis {
        axis: GamepadAxis::LeftStickX,
        value,
        gamepad_id: 0,
    }
}

/// Whether the action reports pressed for this event (absent match = not pressed).
fn pressed(map: &InputMap, event: &InputEvent, action: &str) -> bool {
    map.action_match(event, action)
        .map(|m| m.pressed)
        .unwrap_or(false)
}

/// Godot's `InputEventJoypadMotion::action_match` requires the **same
/// direction**: a binding on the negative half of an axis must not fire when
/// the stick is pushed positive.
///
/// This is the clause most likely to regress into `value.abs() > deadzone`,
/// which fires on both halves and makes "move left" trigger on "right".
#[test]
fn a_negative_axis_binding_ignores_positive_deflection() {
    let mut map = InputMap::new();
    map.add_action("move_left", 0.2);
    map.action_add_event(
        "move_left",
        ActionBinding::axis_negative(GamepadAxis::LeftStickX),
    );

    assert!(
        !pressed(&map, &axis(1.0), "move_left"),
        "a negative-direction binding must not fire on positive deflection",
    );
    assert!(
        pressed(&map, &axis(-1.0), "move_left"),
        "the same binding must fire on its own direction",
    );
}

/// Strength is `inverse_lerp(deadzone, 1, |v|)`, not the raw magnitude.
///
/// The bead's worked example: axis `0.75` under deadzone `0.5` reads **0.5**,
/// because it sits halfway between the deadzone and full deflection. A naive
/// implementation reports `0.75`, and every deadzone-sensitive control feels
/// wrong near the threshold.
#[test]
fn strength_remaps_from_the_deadzone_not_from_zero() {
    let mut map = InputMap::new();
    map.add_action("aim", 0.5);
    map.action_add_event("aim", ActionBinding::axis_positive(GamepadAxis::LeftStickX));

    let m = map
        .action_match(&axis(0.75), "aim")
        .expect("the binding matches its own axis");
    assert!(m.pressed);
    assert!(
        (m.strength - 0.5).abs() < 1e-5,
        "0.75 under deadzone 0.5 must remap to 0.5, got {}",
        m.strength,
    );
}

/// A deflection at or below the deadzone does not press, and returning to
/// centre releases rather than latching on.
#[test]
fn deadzone_gates_pressing_and_centre_releases() {
    let mut map = InputMap::new();
    map.add_action("aim", 0.5);
    map.action_add_event("aim", ActionBinding::axis_positive(GamepadAxis::LeftStickX));

    assert!(
        !pressed(&map, &axis(0.25), "aim"),
        "a deflection below the deadzone must not press",
    );
    assert!(
        pressed(&map, &axis(0.9), "aim"),
        "past the deadzone it presses",
    );
    assert!(
        !pressed(&map, &axis(0.0), "aim"),
        "returning to centre must release rather than latch",
    );
}

/// A modifier-carrying binding matches **only** with that modifier held.
///
/// Godot binds `ui_focus_prev` to Shift+Tab; a bare Tab must not trigger it, or
/// forward and backward focus collapse onto the same key.
#[test]
fn a_shift_binding_requires_shift() {
    let mut map = InputMap::new();
    map.add_action("focus_prev", 0.5);
    map.action_add_event("focus_prev", ActionBinding::shift_key(Key::Tab));

    let bare = InputEvent::Key {
        key: Key::Tab,
        pressed: true,
        shift: false,
        ctrl: false,
        alt: false,
    };
    assert!(
        !map.event_matches_action(&bare, "focus_prev"),
        "a Shift-qualified binding must not fire on the bare key",
    );

    let shifted = InputEvent::Key {
        key: Key::Tab,
        pressed: true,
        shift: true,
        ctrl: false,
        alt: false,
    };
    assert!(
        map.event_matches_action(&shifted, "focus_prev"),
        "Shift+Tab must fire the Shift-qualified binding",
    );
}
