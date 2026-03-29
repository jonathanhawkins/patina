//! pat-jkwa: Input-map loading and action binding parity test.
//!
//! Covers gaps not exercised by `input_map_loading_test` (pat-vih) or
//! `input_action_coverage_test` (pat-g9k):
//!
//! - InputState → InputSnapshot lifecycle (snapshot captures state, survives flush)
//! - Snapshot query methods: key names, action→key map, action strength, axis, vector
//! - is_action_just_released / is_key_just_released / is_mouse_button_just_released
//! - is_key_just_pressed at InputState level
//! - Gamepad button press and axis value
//! - project.godot format loading
//! - Key::name ↔ Key::from_name roundtrip
//! - flush_frame clears just-pressed/just-released but preserves held state

use gdplatform::input::{
    ActionBinding, GamepadAxis, GamepadButton, InputEvent, InputMap, InputState, Key, MouseButton,
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn key_event(key: Key, pressed: bool) -> InputEvent {
    InputEvent::Key {
        key,
        pressed,
        shift: false,
        ctrl: false,
        alt: false,
    }
}

fn simple_map() -> InputMap {
    let mut map = InputMap::new();
    map.add_action("move_left", 0.0);
    map.action_add_event("move_left", ActionBinding::KeyBinding(Key::Left));
    map.add_action("move_right", 0.0);
    map.action_add_event("move_right", ActionBinding::KeyBinding(Key::Right));
    map.add_action("jump", 0.0);
    map.action_add_event("jump", ActionBinding::KeyBinding(Key::Space));
    map.add_action("shoot", 0.0);
    map.action_add_event("shoot", ActionBinding::MouseBinding(MouseButton::Left));
    map
}

fn state_with_map() -> InputState {
    let mut state = InputState::new();
    state.set_input_map(simple_map());
    state
}

// ===========================================================================
// 1. InputSnapshot lifecycle
// ===========================================================================

#[test]
fn snapshot_captures_pressed_keys() {
    let mut state = state_with_map();
    state.process_event(key_event(Key::Right, true));

    let snap = state.snapshot();
    assert!(snap.is_key_pressed(Key::Right));
    assert!(snap.is_key_just_pressed(Key::Right));
    assert!(snap.is_action_pressed("move_right"));
    assert!(snap.is_action_just_pressed("move_right"));
}

#[test]
fn snapshot_survives_flush() {
    let mut state = state_with_map();
    state.process_event(key_event(Key::Right, true));
    let snap = state.snapshot();

    // Flush clears just-pressed on the state...
    state.flush_frame();

    // ...but the snapshot is frozen.
    assert!(snap.is_key_just_pressed(Key::Right));
    assert!(snap.is_action_just_pressed("move_right"));
}

#[test]
fn snapshot_after_flush_has_no_just_pressed() {
    let mut state = state_with_map();
    state.process_event(key_event(Key::Space, true));
    state.flush_frame();

    let snap = state.snapshot();
    assert!(snap.is_key_pressed(Key::Space), "key still held");
    assert!(
        !snap.is_key_just_pressed(Key::Space),
        "just_pressed cleared by flush"
    );
    assert!(snap.is_action_pressed("jump"), "action still held");
    assert!(
        !snap.is_action_just_pressed("jump"),
        "action just_pressed cleared"
    );
}

// ===========================================================================
// 2. Snapshot query methods: pressed_key_names, just_pressed_key_names
// ===========================================================================

#[test]
fn snapshot_pressed_key_names() {
    let mut state = state_with_map();
    state.process_event(key_event(Key::A, true));
    state.process_event(key_event(Key::D, true));

    let snap = state.snapshot();
    let names = snap.pressed_key_names();
    assert!(names.contains(&"A".to_string()));
    assert!(names.contains(&"D".to_string()));
    assert_eq!(names.len(), 2);
}

#[test]
fn snapshot_just_pressed_key_names() {
    let mut state = state_with_map();
    state.process_event(key_event(Key::W, true));

    let snap = state.snapshot();
    let just = snap.just_pressed_key_names();
    assert_eq!(just.len(), 1);
    assert!(just.contains(&"W".to_string()));
}

#[test]
fn snapshot_action_pressed_key_map() {
    let mut state = state_with_map();
    state.process_event(key_event(Key::Right, true));

    let snap = state.snapshot();
    let map = snap.action_pressed_key_map();
    assert!(
        map.contains_key("move_right"),
        "action_pressed_key_map should contain move_right"
    );
    let keys = &map["move_right"];
    assert!(
        keys.contains(&"ArrowRight".to_string()),
        "pressed key 'ArrowRight' should appear in action map"
    );
}

// ===========================================================================
// 3. get_action_strength / get_axis / get_vector on InputState
// ===========================================================================

#[test]
fn action_strength_returns_one_when_pressed() {
    let mut state = state_with_map();
    state.process_event(key_event(Key::Right, true));

    assert!((state.get_action_strength("move_right") - 1.0).abs() < 1e-6);
}

#[test]
fn action_strength_returns_zero_when_not_pressed() {
    let state = state_with_map();
    assert_eq!(state.get_action_strength("move_right"), 0.0);
}

#[test]
fn get_axis_returns_correct_value() {
    let mut state = state_with_map();
    // Press right only → axis should be 1.0
    state.process_event(key_event(Key::Right, true));
    assert!((state.get_axis("move_left", "move_right") - 1.0).abs() < 1e-6);

    // Also press left → axis should be 0.0 (cancels out)
    state.process_event(key_event(Key::Left, true));
    assert!(state.get_axis("move_left", "move_right").abs() < 1e-6);
}

#[test]
fn get_axis_negative_only() {
    let mut state = state_with_map();
    state.process_event(key_event(Key::Left, true));
    assert!((state.get_axis("move_left", "move_right") + 1.0).abs() < 1e-6);
}

#[test]
fn get_vector_single_direction() {
    let mut state = state_with_map();
    // Add vertical actions
    state.set_input_map({
        let mut map = simple_map();
        map.add_action("move_up", 0.0);
        map.action_add_event("move_up", ActionBinding::KeyBinding(Key::Up));
        map.add_action("move_down", 0.0);
        map.action_add_event("move_down", ActionBinding::KeyBinding(Key::Down));
        map
    });

    state.process_event(key_event(Key::Right, true));
    let v = state.get_vector("move_left", "move_right", "move_up", "move_down");
    assert!((v.x - 1.0).abs() < 1e-6);
    assert!(v.y.abs() < 1e-6);
}

#[test]
fn get_vector_diagonal_normalized() {
    let mut state = InputState::new();
    let mut map = InputMap::new();
    map.add_action("left", 0.0);
    map.action_add_event("left", ActionBinding::KeyBinding(Key::Left));
    map.add_action("right", 0.0);
    map.action_add_event("right", ActionBinding::KeyBinding(Key::Right));
    map.add_action("up", 0.0);
    map.action_add_event("up", ActionBinding::KeyBinding(Key::Up));
    map.add_action("down", 0.0);
    map.action_add_event("down", ActionBinding::KeyBinding(Key::Down));
    state.set_input_map(map);

    state.process_event(key_event(Key::Right, true));
    state.process_event(key_event(Key::Down, true));
    let v = state.get_vector("left", "right", "up", "down");
    let len = (v.x * v.x + v.y * v.y).sqrt();
    assert!(
        (len - 1.0).abs() < 0.02,
        "diagonal vector should be normalized, got len={len}"
    );
}

// ===========================================================================
// 4. Snapshot get_action_strength / get_axis / get_vector
// ===========================================================================

#[test]
fn snapshot_action_strength() {
    let mut state = state_with_map();
    state.process_event(key_event(Key::Space, true));
    let snap = state.snapshot();
    assert!((snap.get_action_strength("jump") - 1.0).abs() < 1e-6);
    assert_eq!(snap.get_action_strength("move_left"), 0.0);
}

#[test]
fn snapshot_get_axis() {
    let mut state = state_with_map();
    state.process_event(key_event(Key::Right, true));
    let snap = state.snapshot();
    assert!((snap.get_axis("move_left", "move_right") - 1.0).abs() < 1e-6);
}

#[test]
fn snapshot_get_vector() {
    let mut state = InputState::new();
    let mut map = simple_map();
    map.add_action("up", 0.0);
    map.action_add_event("up", ActionBinding::KeyBinding(Key::Up));
    map.add_action("down", 0.0);
    map.action_add_event("down", ActionBinding::KeyBinding(Key::Down));
    state.set_input_map(map);

    state.process_event(key_event(Key::Left, true));
    let snap = state.snapshot();
    let v = snap.get_vector("move_left", "move_right", "up", "down");
    assert!((v.x + 1.0).abs() < 1e-6, "left only → x should be -1.0");
    assert!(v.y.abs() < 1e-6);
}

// ===========================================================================
// 5. is_action_just_released
// ===========================================================================

#[test]
fn action_just_released_after_release() {
    let mut state = state_with_map();
    state.process_event(key_event(Key::Space, true));
    state.flush_frame();
    state.process_event(key_event(Key::Space, false));

    assert!(state.is_action_just_released("jump"));
    assert!(!state.is_action_pressed("jump"));
}

#[test]
fn action_just_released_clears_after_flush() {
    let mut state = state_with_map();
    state.process_event(key_event(Key::Space, true));
    state.flush_frame();
    state.process_event(key_event(Key::Space, false));
    assert!(state.is_action_just_released("jump"));

    state.flush_frame();
    assert!(!state.is_action_just_released("jump"));
}

#[test]
fn snapshot_action_just_released() {
    let mut state = state_with_map();
    state.process_event(key_event(Key::Right, true));
    state.flush_frame();
    state.process_event(key_event(Key::Right, false));

    let snap = state.snapshot();
    assert!(snap.is_action_just_released("move_right"));
    assert!(!snap.is_action_pressed("move_right"));
}

// ===========================================================================
// 6. Key-level just_pressed / just_released on InputState
// ===========================================================================

#[test]
fn key_just_pressed_on_state() {
    let mut state = state_with_map();
    state.process_event(key_event(Key::A, true));
    assert!(state.is_key_just_pressed(Key::A));
    assert!(state.is_key_pressed(Key::A));

    state.flush_frame();
    assert!(!state.is_key_just_pressed(Key::A));
    assert!(state.is_key_pressed(Key::A));
}

#[test]
fn key_just_released_on_state() {
    let mut state = state_with_map();
    state.process_event(key_event(Key::B, true));
    state.flush_frame();
    state.process_event(key_event(Key::B, false));

    assert!(state.is_key_just_released(Key::B));
    assert!(!state.is_key_pressed(Key::B));

    state.flush_frame();
    assert!(!state.is_key_just_released(Key::B));
}

#[test]
fn snapshot_key_just_released() {
    let mut state = state_with_map();
    state.process_event(key_event(Key::C, true));
    state.flush_frame();
    state.process_event(key_event(Key::C, false));

    let snap = state.snapshot();
    assert!(snap.is_key_just_released(Key::C));
    assert!(!snap.is_key_pressed(Key::C));
}

// ===========================================================================
// 7. Mouse button just_pressed / just_released on InputState + Snapshot
// ===========================================================================

#[test]
fn mouse_button_just_pressed_on_state() {
    let mut state = state_with_map();
    state.process_event(InputEvent::MouseButton {
        button: MouseButton::Left,
        pressed: true,
        position: gdcore::math::Vector2::ZERO,
    });

    assert!(state.is_mouse_button_just_pressed(MouseButton::Left));
    assert!(state.is_mouse_button_pressed(MouseButton::Left));

    state.flush_frame();
    assert!(!state.is_mouse_button_just_pressed(MouseButton::Left));
    assert!(state.is_mouse_button_pressed(MouseButton::Left));
}

#[test]
fn mouse_button_just_released_on_state() {
    let mut state = state_with_map();
    state.process_event(InputEvent::MouseButton {
        button: MouseButton::Right,
        pressed: true,
        position: gdcore::math::Vector2::ZERO,
    });
    state.flush_frame();
    state.process_event(InputEvent::MouseButton {
        button: MouseButton::Right,
        pressed: false,
        position: gdcore::math::Vector2::ZERO,
    });

    assert!(state.is_mouse_button_just_released(MouseButton::Right));
    assert!(!state.is_mouse_button_pressed(MouseButton::Right));

    state.flush_frame();
    assert!(!state.is_mouse_button_just_released(MouseButton::Right));
}

#[test]
fn snapshot_mouse_button_just_pressed() {
    let mut state = state_with_map();
    state.process_event(InputEvent::MouseButton {
        button: MouseButton::Middle,
        pressed: true,
        position: gdcore::math::Vector2::ZERO,
    });

    let snap = state.snapshot();
    assert!(snap.is_mouse_button_just_pressed(MouseButton::Middle));
    assert!(snap.is_mouse_button_pressed(MouseButton::Middle));
}

#[test]
fn snapshot_mouse_button_just_released() {
    let mut state = state_with_map();
    state.process_event(InputEvent::MouseButton {
        button: MouseButton::Left,
        pressed: true,
        position: gdcore::math::Vector2::ZERO,
    });
    state.flush_frame();
    state.process_event(InputEvent::MouseButton {
        button: MouseButton::Left,
        pressed: false,
        position: gdcore::math::Vector2::ZERO,
    });

    let snap = state.snapshot();
    assert!(snap.is_mouse_button_just_released(MouseButton::Left));
    assert!(!snap.is_mouse_button_pressed(MouseButton::Left));
}

// ===========================================================================
// 8. Gamepad button and axis
// ===========================================================================

#[test]
fn gamepad_button_press() {
    let mut state = InputState::new();
    state.process_event(InputEvent::GamepadButton {
        button: GamepadButton::FaceA,
        pressed: true,
        gamepad_id: 0,
    });

    assert!(state.is_gamepad_button_pressed(0, GamepadButton::FaceA));
    assert!(!state.is_gamepad_button_pressed(0, GamepadButton::FaceB));
    // Different gamepad ID should not match
    assert!(!state.is_gamepad_button_pressed(1, GamepadButton::FaceA));
}

#[test]
fn gamepad_button_release() {
    let mut state = InputState::new();
    state.process_event(InputEvent::GamepadButton {
        button: GamepadButton::Start,
        pressed: true,
        gamepad_id: 0,
    });
    assert!(state.is_gamepad_button_pressed(0, GamepadButton::Start));

    state.process_event(InputEvent::GamepadButton {
        button: GamepadButton::Start,
        pressed: false,
        gamepad_id: 0,
    });
    assert!(!state.is_gamepad_button_pressed(0, GamepadButton::Start));
}

#[test]
fn gamepad_axis_value() {
    let mut state = InputState::new();
    state.process_event(InputEvent::GamepadAxis {
        axis: GamepadAxis::LeftStickX,
        value: 0.75,
        gamepad_id: 0,
    });

    assert!((state.get_gamepad_axis_value(0, GamepadAxis::LeftStickX) - 0.75).abs() < 1e-6);
    // Unset axis should be 0.0
    assert_eq!(
        state.get_gamepad_axis_value(0, GamepadAxis::LeftStickY),
        0.0
    );
}

#[test]
fn gamepad_axis_different_pads() {
    let mut state = InputState::new();
    state.process_event(InputEvent::GamepadAxis {
        axis: GamepadAxis::RightStickY,
        value: -0.5,
        gamepad_id: 1,
    });

    assert!((state.get_gamepad_axis_value(1, GamepadAxis::RightStickY) + 0.5).abs() < 1e-6);
    assert_eq!(
        state.get_gamepad_axis_value(0, GamepadAxis::RightStickY),
        0.0,
        "different gamepad should not see the value"
    );
}

// ===========================================================================
// 9. project.godot format loading
// ===========================================================================

#[test]
fn load_from_project_godot_basic() {
    let content = r#"
[input]

move_left={
"deadzone": 0.5,
"events": [Object(InputEventKey,"resource_local_to_scene":false,"resource_name":"","device":-1,"window_id":0,"alt_pressed":false,"shift_pressed":false,"ctrl_pressed":false,"meta_pressed":false,"pressed":false,"keycode":65,"physical_keycode":0,"key_label":0,"unicode":0,"location":0,"echo":false)]
}
move_right={
"deadzone": 0.5,
"events": [Object(InputEventKey,"resource_local_to_scene":false,"resource_name":"","device":-1,"window_id":0,"alt_pressed":false,"shift_pressed":false,"ctrl_pressed":false,"meta_pressed":false,"pressed":false,"keycode":68,"physical_keycode":0,"key_label":0,"unicode":0,"location":0,"echo":false)]
}

[rendering]

something=else
"#;

    let map = InputMap::load_from_project_godot(content);
    assert!(
        map.get_bindings("move_left").is_some(),
        "move_left should be loaded"
    );
    assert!(
        map.get_bindings("move_right").is_some(),
        "move_right should be loaded"
    );
    // Should not pick up entries from other sections
    assert!(
        map.get_bindings("something").is_none(),
        "entries from [rendering] should not appear"
    );
}

#[test]
fn load_from_project_godot_deadzone() {
    let content = r#"
[input]

jump={
"deadzone": 0.3,
"events": [Object(InputEventKey,"resource_local_to_scene":false,"resource_name":"","device":-1,"window_id":0,"alt_pressed":false,"shift_pressed":false,"ctrl_pressed":false,"meta_pressed":false,"pressed":false,"keycode":32,"physical_keycode":0,"key_label":0,"unicode":0,"location":0,"echo":false)]
}
"#;
    let map = InputMap::load_from_project_godot(content);
    assert!(
        (map.get_deadzone("jump") - 0.3).abs() < 1e-6,
        "deadzone should be parsed from project.godot"
    );
}

#[test]
fn load_from_project_godot_empty_input_section() {
    let content = "[input]\n\n[rendering]\n";
    let map = InputMap::load_from_project_godot(content);
    assert_eq!(map.actions().count(), 0);
}

#[test]
fn load_from_project_godot_no_input_section() {
    let content = "[rendering]\nfoo=bar\n";
    let map = InputMap::load_from_project_godot(content);
    assert_eq!(map.actions().count(), 0);
}

// ===========================================================================
// 10. Key::name ↔ Key::from_name roundtrip
// ===========================================================================

#[test]
fn key_name_roundtrip_letters() {
    for key in [Key::A, Key::M, Key::Z] {
        let name = key.name();
        let parsed = Key::from_name(name);
        assert_eq!(parsed, Some(key), "roundtrip failed for {name}");
    }
}

#[test]
fn key_name_roundtrip_specials() {
    let specials = [
        Key::Space,
        Key::Enter,
        Key::Escape,
        Key::Tab,
        Key::Up,
        Key::Down,
        Key::Left,
        Key::Right,
        Key::F1,
        Key::F12,
        Key::Backspace,
        Key::Delete,
    ];
    for key in specials {
        let name = key.name();
        let parsed = Key::from_name(name);
        assert_eq!(parsed, Some(key), "roundtrip failed for {name}");
    }
}

#[test]
fn key_from_name_case_insensitive_letters() {
    assert_eq!(Key::from_name("a"), Some(Key::A));
    assert_eq!(Key::from_name("A"), Some(Key::A));
    assert_eq!(Key::from_name("z"), Some(Key::Z));
}

#[test]
fn key_from_name_unknown_returns_none() {
    assert_eq!(Key::from_name("NonexistentKey"), None);
    assert_eq!(Key::from_name(""), None);
}

// ===========================================================================
// 11. flush_frame preserves held state
// ===========================================================================

#[test]
fn flush_frame_preserves_held_keys() {
    let mut state = state_with_map();
    state.process_event(key_event(Key::A, true));
    state.process_event(key_event(Key::Right, true));

    state.flush_frame();

    assert!(state.is_key_pressed(Key::A), "held keys survive flush");
    assert!(state.is_key_pressed(Key::Right));
    assert!(
        state.is_action_pressed("move_right"),
        "held action survives flush"
    );
    assert!(!state.is_key_just_pressed(Key::A), "just_pressed cleared");
    assert!(
        !state.is_action_just_pressed("move_right"),
        "action just_pressed cleared"
    );
}

#[test]
fn flush_frame_clears_just_released() {
    let mut state = state_with_map();
    state.process_event(key_event(Key::Space, true));
    state.flush_frame();
    state.process_event(key_event(Key::Space, false));
    assert!(state.is_key_just_released(Key::Space));

    state.flush_frame();
    assert!(!state.is_key_just_released(Key::Space));
    assert!(!state.is_key_pressed(Key::Space));
}

#[test]
fn multiple_flush_cycles() {
    let mut state = state_with_map();

    // Frame 1: press
    state.process_event(key_event(Key::Left, true));
    assert!(state.is_action_just_pressed("move_left"));
    state.flush_frame();

    // Frame 2: held
    assert!(state.is_action_pressed("move_left"));
    assert!(!state.is_action_just_pressed("move_left"));
    state.flush_frame();

    // Frame 3: release
    state.process_event(key_event(Key::Left, false));
    assert!(state.is_action_just_released("move_left"));
    assert!(!state.is_action_pressed("move_left"));
    state.flush_frame();

    // Frame 4: nothing
    assert!(!state.is_action_just_released("move_left"));
    assert!(!state.is_action_pressed("move_left"));
}

// ===========================================================================
// 12. Snapshot mouse position
// ===========================================================================

#[test]
fn snapshot_captures_mouse_position() {
    let mut state = state_with_map();
    state.process_event(InputEvent::MouseMotion {
        position: gdcore::math::Vector2::new(123.0, 456.0),
        relative: gdcore::math::Vector2::ZERO,
    });

    let snap = state.snapshot();
    let pos = snap.get_mouse_position();
    assert!((pos.x - 123.0).abs() < 1e-6);
    assert!((pos.y - 456.0).abs() < 1e-6);
}
