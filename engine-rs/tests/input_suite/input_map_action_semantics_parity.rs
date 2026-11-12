//! pat-0q2kx.89 acceptance: `InputMap` action semantics match Godot's.
//!
//! Patina's input map was a 9-action subset with no modifier support, a
//! direction-blind axis test, a strength that skipped the deadzone remap, a
//! `[input]` parser that dropped joypad bindings, and no `InputMap` singleton
//! for scripts. Each of those is a place where a real Godot project behaves one
//! way and Patina another, so these tests assert the *observable* difference —
//! what fires, how hard, and what a script can rebind at runtime.
//!
//! Ground truth: `upstream/godot/core/input/input_map.cpp` (`get_builtins`) and
//! `core/input/input_event.cpp` (`InputEventKey::action_match`,
//! `InputEventJoypadMotion::action_match`).

use gdplatform::input::{
    ActionBinding, GamepadAxis, GamepadButton, InputEvent, InputMap, InputState, Key, KeyCombo,
};

fn key(key: Key, pressed: bool, shift: bool, ctrl: bool, alt: bool) -> InputEvent {
    InputEvent::Key {
        key,
        pressed,
        shift,
        ctrl,
        alt,
    }
}

fn press(k: Key) -> InputEvent {
    key(k, true, false, false, false)
}

fn shift_press(k: Key) -> InputEvent {
    key(k, true, true, false, false)
}

fn axis(axis: GamepadAxis, value: f32) -> InputEvent {
    InputEvent::GamepadAxis {
        axis,
        value,
        gamepad_id: 0,
    }
}

fn pad(button: GamepadButton, pressed: bool) -> InputEvent {
    InputEvent::GamepadButton {
        button,
        pressed,
        gamepad_id: 0,
    }
}

// ---------------------------------------------------------------------------
// The bead's acceptance test
// ---------------------------------------------------------------------------

/// **The core parity test.** Direction-aware axis bindings, the deadzone
/// strength remap, joypad bindings parsed from `project.godot`, and
/// `ui_focus_prev` as Shift+Tab.
#[test]
fn input_map_action_semantics_parity() {
    // -- A negative-direction binding ignores positive deflection ----------
    let mut map = InputMap::new();
    map.add_action("strafe_left", 0.5);
    map.action_add_event(
        "strafe_left",
        ActionBinding::axis_negative(GamepadAxis::LeftStickX),
    );

    let left = map
        .action_match(&axis(GamepadAxis::LeftStickX, -0.9), "strafe_left")
        .expect("the axis is bound either way");
    assert!(left.pressed, "deflecting left must press a left binding");

    let right = map
        .action_match(&axis(GamepadAxis::LeftStickX, 0.9), "strafe_left")
        .expect("Godot matches the axis regardless of direction");
    assert!(
        !right.pressed,
        "deflecting right must NOT press a left-direction binding — this is what \
         makes left and right separate actions"
    );
    assert_eq!(
        right.raw_strength, 0.0,
        "the opposite half of the stick reads zero raw strength, not |v|"
    );

    // -- Strength is inverse_lerp(deadzone, 1, |v|), not |v| ---------------
    let mut map = InputMap::new();
    map.add_action("throttle", 0.5);
    map.action_add_event(
        "throttle",
        ActionBinding::axis_positive(GamepadAxis::RightTriggerAnalog),
    );
    let m = map
        .action_match(&axis(GamepadAxis::RightTriggerAnalog, 0.75), "throttle")
        .expect("bound");
    assert!(m.pressed);
    assert!(
        (m.strength - 0.5).abs() < 1e-5,
        "0.75 under a 0.5 deadzone is half-way to full deflection: \
         (0.75-0.5)/(1-0.5) = 0.5, got {}",
        m.strength
    );
    assert!(
        (m.raw_strength - 0.75).abs() < 1e-5,
        "raw strength ignores the deadzone, got {}",
        m.raw_strength
    );
    // Exactly at the deadzone counts as pressed (Godot uses `>=`).
    let at_edge = map
        .action_match(&axis(GamepadAxis::RightTriggerAnalog, 0.5), "throttle")
        .expect("bound");
    assert!(at_edge.pressed, "|v| == deadzone is pressed in Godot");
    assert_eq!(at_edge.strength, 0.0, "…at zero strength");

    // -- A project.godot JOY_BUTTON_A binding parses and fires -------------
    let project = r#"
[input]

jump={
"deadzone": 0.5,
"events": [Object(InputEventJoypadButton,"resource_local_to_scene":false,"device":-1,"button_index":0,"pressure":0.0,"pressed":false,"script":null)]
}
"#;
    let map = InputMap::load_from_project_godot(project);
    assert!(
        map.event_matches_action(&pad(GamepadButton::FaceA, true), "jump"),
        "a JOY_BUTTON_A binding in project.godot must reach the action — the \
         parser used to drop every joypad event"
    );
    assert!(
        !map.event_matches_action(&pad(GamepadButton::FaceB, true), "jump"),
        "…and only that button"
    );

    // -- ui_focus_prev is Shift+Tab, and only Shift+Tab --------------------
    let mut builtins = InputMap::new();
    builtins.install_builtin_ui_actions();
    assert!(
        builtins.event_matches_action(&shift_press(Key::Tab), "ui_focus_prev"),
        "ui_focus_prev must fire on Shift+Tab"
    );
    assert!(
        !builtins.event_matches_action(&press(Key::Tab), "ui_focus_prev"),
        "a bare Tab must NOT fire ui_focus_prev — it is ui_focus_next"
    );
    assert!(
        builtins.event_matches_action(&press(Key::Tab), "ui_focus_next"),
        "a bare Tab is ui_focus_next"
    );
}

// ---------------------------------------------------------------------------
// Modifier semantics
// ---------------------------------------------------------------------------

/// Godot's non-exact match is a **subset** test: the binding's modifiers must be
/// held, but the event may hold more
/// (`(action_mask & key_mask) == action_mask`).
#[test]
fn modifier_matching_is_a_subset_test() {
    let mut map = InputMap::new();
    map.add_action("copy", 0.5);
    map.action_add_event("copy", ActionBinding::ctrl_key(Key::C));

    assert!(
        map.event_matches_action(&key(Key::C, true, false, true, false), "copy"),
        "Ctrl+C fires Ctrl+C"
    );
    assert!(
        map.event_matches_action(&key(Key::C, true, true, true, false), "copy"),
        "Ctrl+Shift+C still fires Ctrl+C: extra modifiers are allowed"
    );
    assert!(
        !map.event_matches_action(&press(Key::C), "copy"),
        "a bare C must not fire a Ctrl-requiring binding"
    );

    // A binding with no modifier requirement is satisfied by anything.
    let mut plain = InputMap::new();
    plain.add_action("shoot", 0.5);
    plain.action_add_event("shoot", ActionBinding::KeyBinding(Key::C));
    assert!(
        plain.event_matches_action(&key(Key::C, true, true, true, true), "shoot"),
        "an unmodified binding matches however many modifiers are held"
    );
}

/// A release matches even if the modifier was let go first, so the action can
/// actually drop (Godot applies the modifier test only to presses).
#[test]
fn modifier_binding_still_releases_without_the_modifier() {
    let mut map = InputMap::new();
    map.add_action("copy", 0.5);
    map.action_add_event("copy", ActionBinding::ctrl_key(Key::C));

    let mut state = InputState::new();
    state.set_input_map(map);
    state.process_event(key(Key::C, true, false, true, false));
    assert!(state.is_action_pressed("copy"));

    // User lifts Ctrl before C: the key-up still ends the action.
    state.process_event(key(Key::C, false, false, false, false));
    assert!(
        !state.is_action_pressed("copy"),
        "the action must release, not stick pressed forever"
    );
}

// ---------------------------------------------------------------------------
// Axis state through InputState
// ---------------------------------------------------------------------------

/// Crossing the stick through centre releases the action it was driving.
#[test]
fn axis_action_releases_when_the_stick_crosses_centre() {
    let mut map = InputMap::new();
    map.add_action("ui_left", 0.5);
    map.action_add_event(
        "ui_left",
        ActionBinding::axis_negative(GamepadAxis::LeftStickX),
    );
    let mut state = InputState::new();
    state.set_input_map(map);

    state.process_event(axis(GamepadAxis::LeftStickX, -0.8));
    assert!(state.is_action_pressed("ui_left"));

    state.process_event(axis(GamepadAxis::LeftStickX, 0.8));
    assert!(
        !state.is_action_pressed("ui_left"),
        "pushing the stick the other way must release the left action; a \
         direction-blind match left it stuck pressed"
    );
    assert_eq!(state.get_action_strength("ui_left"), 0.0);
}

// ---------------------------------------------------------------------------
// The builtin table
// ---------------------------------------------------------------------------

/// The builtin set is Godot's, not a nine-action subset: the text-editing,
/// clipboard and paging actions all exist with their real bindings.
#[test]
fn builtin_ui_actions_cover_the_godot_table() {
    let mut map = InputMap::new();
    map.install_builtin_ui_actions();

    // A representative slice across every group in `get_builtins`.
    for action in [
        "ui_accept",
        "ui_select",
        "ui_cancel",
        "ui_close_dialog",
        "ui_focus_next",
        "ui_focus_prev",
        "ui_left",
        "ui_right",
        "ui_up",
        "ui_down",
        "ui_page_up",
        "ui_page_down",
        "ui_home",
        "ui_end",
        "ui_cut",
        "ui_copy",
        "ui_paste",
        "ui_undo",
        "ui_redo",
        "ui_text_caret_left",
        "ui_text_caret_word_right",
        "ui_text_select_all",
        "ui_text_submit",
        "ui_graph_delete",
        "ui_filedialog_refresh",
    ] {
        assert!(
            map.has_action(action),
            "builtin action {action} must be installed"
        );
    }
    assert!(
        map.get_actions().len() > 50,
        "Godot installs ~80 ui_* actions; got {}",
        map.get_actions().len()
    );

    // Spot-check the bindings themselves, not just the names.
    assert!(map.event_matches_action(&press(Key::Home), "ui_home"));
    assert!(map.event_matches_action(&press(Key::PageDown), "ui_page_down"));
    assert!(
        map.event_matches_action(&key(Key::C, true, false, true, false), "ui_copy"),
        "ui_copy is Ctrl+C"
    );
    assert!(
        !map.event_matches_action(&press(Key::C), "ui_copy"),
        "…and a bare C is not"
    );
    assert!(
        map.event_matches_action(&key(Key::Z, true, true, true, false), "ui_redo"),
        "ui_redo is Ctrl+Shift+Z"
    );
    // Directional actions carry their stick bindings, split by direction.
    assert!(map.event_matches_action(&axis(GamepadAxis::LeftStickX, -0.9), "ui_left"));
    assert!(!map.event_matches_action(&axis(GamepadAxis::LeftStickX, 0.9), "ui_left"));
    assert!(map.event_matches_action(&axis(GamepadAxis::LeftStickX, 0.9), "ui_right"));
}

/// A `project.godot` `[input]` block overrides a builtin wholesale, and its
/// modifier-carrying key events survive the parse.
#[test]
fn project_actions_parse_modifiers_and_joypad_motion() {
    let project = r#"
[input]

save={
"deadzone": 0.5,
"events": [Object(InputEventKey,"resource_local_to_scene":false,"device":-1,"ctrl_pressed":true,"shift_pressed":false,"alt_pressed":false,"meta_pressed":false,"keycode":83,"physical_keycode":0,"unicode":0,"echo":false,"script":null)]
}
walk_left={
"deadzone": 0.2,
"events": [Object(InputEventJoypadMotion,"resource_local_to_scene":false,"device":-1,"axis":0,"axis_value":-1.0,"script":null)]
}
"#;
    let map = InputMap::load_from_project_godot(project);

    assert!(
        map.event_matches_action(&key(Key::S, true, false, true, false), "save"),
        "Ctrl+S must fire the parsed ctrl_pressed binding"
    );
    assert!(
        !map.event_matches_action(&press(Key::S), "save"),
        "a bare S must not — the parser used to drop the modifier entirely"
    );

    assert!(
        map.event_matches_action(&axis(GamepadAxis::LeftStickX, -0.8), "walk_left"),
        "axis_value -1.0 binds the negative direction"
    );
    assert!(
        !map.event_matches_action(&axis(GamepadAxis::LeftStickX, 0.8), "walk_left"),
        "…and not the positive one"
    );
    assert!(
        (map.get_deadzone("walk_left") - 0.2).abs() < 1e-6,
        "the action's own deadzone must survive"
    );
}

/// The runtime rebinding API mutates the map a later event resolves against.
#[test]
fn script_api_rebinds_actions_at_runtime() {
    let mut map = InputMap::new();
    map.add_action("dash", 0.5);
    map.action_add_event("dash", ActionBinding::KeyBinding(Key::Q));
    assert!(map.has_action("dash"));
    assert_eq!(map.get_bindings("dash").map(|b| b.len()), Some(1));

    let mut state = InputState::new();
    state.set_input_map(map);
    state.process_event(press(Key::Q));
    assert!(state.is_action_pressed("dash"));

    // Rebind Q away and E in, the way a settings screen does.
    let live = state.input_map_mut();
    live.action_erase_event("dash", &ActionBinding::KeyBinding(Key::Q));
    live.action_add_event(
        "dash",
        ActionBinding::KeyWithModifiers(KeyCombo::new(Key::E)),
    );
    let mut state2 = InputState::new();
    state2.set_input_map(state.input_map().cloned().unwrap());
    state2.process_event(press(Key::Q));
    assert!(
        !state2.is_action_pressed("dash"),
        "the erased binding must stop firing"
    );
    state2.process_event(press(Key::E));
    assert!(
        state2.is_action_pressed("dash"),
        "the new binding must fire"
    );

    // erase_action removes it entirely.
    state2.input_map_mut().erase_action("dash");
    assert!(!state2.input_map().unwrap().has_action("dash"));
}
