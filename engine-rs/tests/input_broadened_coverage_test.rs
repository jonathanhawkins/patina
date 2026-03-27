//! pat-0jx: Broadened input handling coverage beyond the initial 2D slice.
//!
//! Tests the extended input APIs added to close Godot parity gaps:
//! - MouseMode (cursor visibility/confinement)
//! - GamepadButton/GamepadAxis name parsing and roundtrip
//! - Connected joypad tracking and disconnection
//! - Mouse velocity tracking
//! - InputMap runtime deadzone modification
//! - is_anything_pressed() utility
//! - Snapshot propagation of all new fields

use gdplatform::input::{
    ActionBinding, GamepadAxis, GamepadButton, InputEvent, InputMap, InputState, Key, MouseButton,
    MouseMode,
};

use gdcore::math::Vector2;

// ===========================================================================
// MouseMode
// ===========================================================================

#[test]
fn mouse_mode_default_is_visible() {
    let state = InputState::new();
    assert_eq!(state.get_mouse_mode(), MouseMode::Visible);
}

#[test]
fn mouse_mode_set_and_get() {
    let mut state = InputState::new();
    state.set_mouse_mode(MouseMode::Captured);
    assert_eq!(state.get_mouse_mode(), MouseMode::Captured);

    state.set_mouse_mode(MouseMode::ConfinedHidden);
    assert_eq!(state.get_mouse_mode(), MouseMode::ConfinedHidden);
}

#[test]
fn mouse_mode_snapshot_propagates() {
    let mut state = InputState::new();
    state.set_mouse_mode(MouseMode::Hidden);
    let snap = state.snapshot();
    assert_eq!(snap.get_mouse_mode(), MouseMode::Hidden);
}

#[test]
fn mouse_mode_all_variants_distinct() {
    let modes = [
        MouseMode::Visible,
        MouseMode::Hidden,
        MouseMode::Captured,
        MouseMode::Confined,
        MouseMode::ConfinedHidden,
    ];
    for (i, a) in modes.iter().enumerate() {
        for (j, b) in modes.iter().enumerate() {
            if i == j {
                assert_eq!(a, b);
            } else {
                assert_ne!(a, b);
            }
        }
    }
}

// ===========================================================================
// GamepadButton name parsing
// ===========================================================================

#[test]
fn gamepad_button_name_roundtrip() {
    let buttons = [
        GamepadButton::FaceA,
        GamepadButton::FaceB,
        GamepadButton::FaceX,
        GamepadButton::FaceY,
        GamepadButton::DPadUp,
        GamepadButton::DPadDown,
        GamepadButton::DPadLeft,
        GamepadButton::DPadRight,
        GamepadButton::LeftShoulder,
        GamepadButton::RightShoulder,
        GamepadButton::LeftTrigger,
        GamepadButton::RightTrigger,
        GamepadButton::LeftStick,
        GamepadButton::RightStick,
        GamepadButton::Start,
        GamepadButton::Select,
        GamepadButton::Guide,
    ];
    for btn in &buttons {
        let name = btn.name();
        let parsed = GamepadButton::from_name(name);
        assert_eq!(parsed, Some(*btn), "roundtrip failed for {name}");
    }
}

#[test]
fn gamepad_button_from_snake_case() {
    assert_eq!(GamepadButton::from_name("face_a"), Some(GamepadButton::FaceA));
    assert_eq!(GamepadButton::from_name("dpad_up"), Some(GamepadButton::DPadUp));
    assert_eq!(GamepadButton::from_name("left_shoulder"), Some(GamepadButton::LeftShoulder));
    assert_eq!(GamepadButton::from_name("right_stick"), Some(GamepadButton::RightStick));
}

#[test]
fn gamepad_button_from_common_aliases() {
    assert_eq!(GamepadButton::from_name("a"), Some(GamepadButton::FaceA));
    assert_eq!(GamepadButton::from_name("b"), Some(GamepadButton::FaceB));
    assert_eq!(GamepadButton::from_name("lb"), Some(GamepadButton::LeftShoulder));
    assert_eq!(GamepadButton::from_name("rb"), Some(GamepadButton::RightShoulder));
    assert_eq!(GamepadButton::from_name("lt"), Some(GamepadButton::LeftTrigger));
    assert_eq!(GamepadButton::from_name("rt"), Some(GamepadButton::RightTrigger));
    assert_eq!(GamepadButton::from_name("l3"), Some(GamepadButton::LeftStick));
    assert_eq!(GamepadButton::from_name("r3"), Some(GamepadButton::RightStick));
    assert_eq!(GamepadButton::from_name("back"), Some(GamepadButton::Select));
    assert_eq!(GamepadButton::from_name("home"), Some(GamepadButton::Guide));
}

#[test]
fn gamepad_button_from_unknown_returns_none() {
    assert_eq!(GamepadButton::from_name("nonexistent"), None);
    assert_eq!(GamepadButton::from_name(""), None);
}

// ===========================================================================
// GamepadAxis name parsing
// ===========================================================================

#[test]
fn gamepad_axis_name_roundtrip() {
    let axes = [
        GamepadAxis::LeftStickX,
        GamepadAxis::LeftStickY,
        GamepadAxis::RightStickX,
        GamepadAxis::RightStickY,
        GamepadAxis::LeftTriggerAnalog,
        GamepadAxis::RightTriggerAnalog,
    ];
    for axis in &axes {
        let name = axis.name();
        let parsed = GamepadAxis::from_name(name);
        assert_eq!(parsed, Some(*axis), "roundtrip failed for {name}");
    }
}

#[test]
fn gamepad_axis_from_snake_case() {
    assert_eq!(GamepadAxis::from_name("left_stick_x"), Some(GamepadAxis::LeftStickX));
    assert_eq!(GamepadAxis::from_name("right_stick_y"), Some(GamepadAxis::RightStickY));
    assert_eq!(
        GamepadAxis::from_name("left_trigger_analog"),
        Some(GamepadAxis::LeftTriggerAnalog)
    );
}

#[test]
fn gamepad_axis_from_aliases() {
    assert_eq!(GamepadAxis::from_name("left_x"), Some(GamepadAxis::LeftStickX));
    assert_eq!(GamepadAxis::from_name("right_y"), Some(GamepadAxis::RightStickY));
    assert_eq!(GamepadAxis::from_name("l2"), Some(GamepadAxis::LeftTriggerAnalog));
    assert_eq!(GamepadAxis::from_name("r2"), Some(GamepadAxis::RightTriggerAnalog));
}

#[test]
fn gamepad_axis_from_unknown_returns_none() {
    assert_eq!(GamepadAxis::from_name("z_axis"), None);
}

// ===========================================================================
// Connected joypad tracking
// ===========================================================================

#[test]
fn no_joypads_connected_initially() {
    let state = InputState::new();
    assert!(state.get_connected_joypads().is_empty());
}

#[test]
fn joypad_auto_connects_on_button_event() {
    let mut state = InputState::new();
    state.process_event(InputEvent::GamepadButton {
        button: GamepadButton::FaceA,
        pressed: true,
        gamepad_id: 0,
    });
    assert_eq!(state.get_connected_joypads(), vec![0]);
}

#[test]
fn joypad_auto_connects_on_axis_event() {
    let mut state = InputState::new();
    state.process_event(InputEvent::GamepadAxis {
        axis: GamepadAxis::LeftStickX,
        value: 0.5,
        gamepad_id: 2,
    });
    assert_eq!(state.get_connected_joypads(), vec![2]);
}

#[test]
fn multiple_joypads_tracked_and_sorted() {
    let mut state = InputState::new();
    state.process_event(InputEvent::GamepadButton {
        button: GamepadButton::FaceA,
        pressed: true,
        gamepad_id: 3,
    });
    state.process_event(InputEvent::GamepadButton {
        button: GamepadButton::FaceB,
        pressed: true,
        gamepad_id: 1,
    });
    state.process_event(InputEvent::GamepadAxis {
        axis: GamepadAxis::LeftStickX,
        value: 0.1,
        gamepad_id: 0,
    });
    assert_eq!(state.get_connected_joypads(), vec![0, 1, 3]);
}

#[test]
fn manual_joypad_connect() {
    let mut state = InputState::new();
    state.connect_joypad(5);
    state.connect_joypad(2);
    assert_eq!(state.get_connected_joypads(), vec![2, 5]);
}

#[test]
fn joypad_disconnect_clears_state() {
    let mut state = InputState::new();
    state.process_event(InputEvent::GamepadButton {
        button: GamepadButton::FaceA,
        pressed: true,
        gamepad_id: 0,
    });
    state.process_event(InputEvent::GamepadAxis {
        axis: GamepadAxis::LeftStickX,
        value: 0.8,
        gamepad_id: 0,
    });
    assert!(state.is_gamepad_button_pressed(0, GamepadButton::FaceA));
    assert!((state.get_gamepad_axis_value(0, GamepadAxis::LeftStickX) - 0.8).abs() < 0.001);

    state.disconnect_joypad(0);
    assert!(state.get_connected_joypads().is_empty());
    assert!(!state.is_gamepad_button_pressed(0, GamepadButton::FaceA));
    assert!((state.get_gamepad_axis_value(0, GamepadAxis::LeftStickX)).abs() < 0.001);
}

#[test]
fn joypad_disconnect_does_not_affect_other_pads() {
    let mut state = InputState::new();
    state.process_event(InputEvent::GamepadButton {
        button: GamepadButton::FaceA,
        pressed: true,
        gamepad_id: 0,
    });
    state.process_event(InputEvent::GamepadButton {
        button: GamepadButton::FaceB,
        pressed: true,
        gamepad_id: 1,
    });
    state.disconnect_joypad(0);
    assert_eq!(state.get_connected_joypads(), vec![1]);
    assert!(state.is_gamepad_button_pressed(1, GamepadButton::FaceB));
}

#[test]
fn connected_joypads_snapshot_propagates() {
    let mut state = InputState::new();
    state.connect_joypad(0);
    state.connect_joypad(1);
    let snap = state.snapshot();
    assert_eq!(snap.get_connected_joypads(), vec![0, 1]);
}

// ===========================================================================
// Mouse velocity tracking
// ===========================================================================

#[test]
fn mouse_velocity_initially_zero() {
    let state = InputState::new();
    assert_eq!(state.get_last_mouse_velocity(), Vector2::ZERO);
}

#[test]
fn mouse_velocity_updated_on_motion() {
    let mut state = InputState::new();
    state.process_event(InputEvent::MouseMotion {
        position: Vector2::new(100.0, 200.0),
        relative: Vector2::new(5.0, -3.0),
    });
    let vel = state.get_last_mouse_velocity();
    assert!((vel.x - 5.0).abs() < 0.001);
    assert!((vel.y - (-3.0)).abs() < 0.001);
}

#[test]
fn mouse_velocity_reset_on_flush() {
    let mut state = InputState::new();
    state.process_event(InputEvent::MouseMotion {
        position: Vector2::new(100.0, 200.0),
        relative: Vector2::new(5.0, -3.0),
    });
    state.flush_frame();
    assert_eq!(state.get_last_mouse_velocity(), Vector2::ZERO);
}

#[test]
fn mouse_velocity_snapshot_propagates() {
    let mut state = InputState::new();
    state.process_event(InputEvent::MouseMotion {
        position: Vector2::new(50.0, 50.0),
        relative: Vector2::new(10.0, 20.0),
    });
    let snap = state.snapshot();
    let vel = snap.get_last_mouse_velocity();
    assert!((vel.x - 10.0).abs() < 0.001);
    assert!((vel.y - 20.0).abs() < 0.001);
}

#[test]
fn mouse_velocity_last_motion_wins() {
    let mut state = InputState::new();
    state.process_event(InputEvent::MouseMotion {
        position: Vector2::new(10.0, 10.0),
        relative: Vector2::new(1.0, 1.0),
    });
    state.process_event(InputEvent::MouseMotion {
        position: Vector2::new(20.0, 20.0),
        relative: Vector2::new(7.0, 8.0),
    });
    let vel = state.get_last_mouse_velocity();
    assert!((vel.x - 7.0).abs() < 0.001);
    assert!((vel.y - 8.0).abs() < 0.001);
}

// ===========================================================================
// InputMap runtime deadzone modification
// ===========================================================================

#[test]
fn action_set_deadzone_modifies_existing() {
    let mut map = InputMap::new();
    map.add_action("jump", 0.2);
    assert!((map.get_deadzone("jump") - 0.2).abs() < 0.001);

    map.action_set_deadzone("jump", 0.5);
    assert!((map.get_deadzone("jump") - 0.5).abs() < 0.001);
}

#[test]
fn action_set_deadzone_ignores_unregistered() {
    let mut map = InputMap::new();
    map.action_set_deadzone("nonexistent", 0.5);
    // Should not create the action
    assert!(!map.has_action("nonexistent"));
    assert!((map.get_deadzone("nonexistent")).abs() < 0.001); // default 0.0
}

#[test]
fn action_set_deadzone_affects_event_matching() {
    let mut map = InputMap::new();
    map.add_action("accelerate", 0.1);
    map.action_add_event("accelerate", ActionBinding::GamepadAxisBinding(GamepadAxis::RightTriggerAnalog));

    let event = InputEvent::GamepadAxis {
        axis: GamepadAxis::RightTriggerAnalog,
        value: 0.3,
        gamepad_id: 0,
    };

    // With deadzone 0.1, value 0.3 should match
    assert!(map.event_matches_action(&event, "accelerate"));

    // Raise deadzone above the value
    map.action_set_deadzone("accelerate", 0.5);
    assert!(!map.event_matches_action(&event, "accelerate"));
}

// ===========================================================================
// is_anything_pressed
// ===========================================================================

#[test]
fn is_anything_pressed_initially_false() {
    let state = InputState::new();
    assert!(!state.is_anything_pressed());
}

#[test]
fn is_anything_pressed_with_key() {
    let mut state = InputState::new();
    state.process_event(InputEvent::Key {
        key: Key::Space,
        pressed: true,
        shift: false,
        ctrl: false,
        alt: false,
    });
    assert!(state.is_anything_pressed());
}

#[test]
fn is_anything_pressed_with_mouse() {
    let mut state = InputState::new();
    state.process_event(InputEvent::MouseButton {
        button: MouseButton::Left,
        pressed: true,
        position: Vector2::ZERO,
    });
    assert!(state.is_anything_pressed());
}

#[test]
fn is_anything_pressed_with_gamepad() {
    let mut state = InputState::new();
    state.process_event(InputEvent::GamepadButton {
        button: GamepadButton::FaceA,
        pressed: true,
        gamepad_id: 0,
    });
    assert!(state.is_anything_pressed());
}

#[test]
fn is_anything_pressed_with_touch() {
    let mut state = InputState::new();
    state.process_event(InputEvent::ScreenTouch {
        index: 0,
        position: Vector2::new(100.0, 200.0),
        pressed: true,
    });
    assert!(state.is_anything_pressed());
}

#[test]
fn is_anything_pressed_false_after_all_released() {
    let mut state = InputState::new();
    state.process_event(InputEvent::Key {
        key: Key::A,
        pressed: true,
        shift: false,
        ctrl: false,
        alt: false,
    });
    assert!(state.is_anything_pressed());

    state.process_event(InputEvent::Key {
        key: Key::A,
        pressed: false,
        shift: false,
        ctrl: false,
        alt: false,
    });
    assert!(!state.is_anything_pressed());
}

#[test]
fn is_anything_pressed_snapshot_propagates() {
    let mut state = InputState::new();
    state.process_event(InputEvent::Key {
        key: Key::Enter,
        pressed: true,
        shift: false,
        ctrl: false,
        alt: false,
    });
    let snap = state.snapshot();
    assert!(snap.is_anything_pressed());
}

// ===========================================================================
// Cross-device interaction (broadened beyond 2D slice)
// ===========================================================================

#[test]
fn gamepad_and_keyboard_simultaneous_input() {
    let mut state = InputState::new();
    state.process_event(InputEvent::Key {
        key: Key::W,
        pressed: true,
        shift: false,
        ctrl: false,
        alt: false,
    });
    state.process_event(InputEvent::GamepadAxis {
        axis: GamepadAxis::LeftStickY,
        value: -0.8,
        gamepad_id: 0,
    });
    assert!(state.is_key_pressed(Key::W));
    assert!((state.get_gamepad_axis_value(0, GamepadAxis::LeftStickY) + 0.8).abs() < 0.001);
    assert!(state.is_anything_pressed());
}

#[test]
fn touch_and_mouse_simultaneous() {
    let mut state = InputState::new();
    state.process_event(InputEvent::MouseButton {
        button: MouseButton::Left,
        pressed: true,
        position: Vector2::new(50.0, 50.0),
    });
    state.process_event(InputEvent::ScreenTouch {
        index: 0,
        position: Vector2::new(200.0, 300.0),
        pressed: true,
    });
    assert!(state.is_mouse_button_pressed(MouseButton::Left));
    assert!(state.is_touch_pressed(0));
    assert_eq!(state.get_touch_count(), 1);
}

#[test]
fn multi_gamepad_independent_state() {
    let mut state = InputState::new();
    // Gamepad 0: press A
    state.process_event(InputEvent::GamepadButton {
        button: GamepadButton::FaceA,
        pressed: true,
        gamepad_id: 0,
    });
    // Gamepad 1: press B and move left stick
    state.process_event(InputEvent::GamepadButton {
        button: GamepadButton::FaceB,
        pressed: true,
        gamepad_id: 1,
    });
    state.process_event(InputEvent::GamepadAxis {
        axis: GamepadAxis::LeftStickX,
        value: -0.5,
        gamepad_id: 1,
    });

    // Verify independent state
    assert!(state.is_gamepad_button_pressed(0, GamepadButton::FaceA));
    assert!(!state.is_gamepad_button_pressed(0, GamepadButton::FaceB));
    assert!(!state.is_gamepad_button_pressed(1, GamepadButton::FaceA));
    assert!(state.is_gamepad_button_pressed(1, GamepadButton::FaceB));
    assert!((state.get_gamepad_axis_value(0, GamepadAxis::LeftStickX)).abs() < 0.001);
    assert!((state.get_gamepad_axis_value(1, GamepadAxis::LeftStickX) + 0.5).abs() < 0.001);
    assert_eq!(state.get_connected_joypads(), vec![0, 1]);
}

#[test]
fn gamepad_action_binding_with_deadzone_through_state() {
    let mut map = InputMap::new();
    map.add_action("steer", 0.15);
    map.action_add_event(
        "steer",
        ActionBinding::GamepadAxisBinding(GamepadAxis::LeftStickX),
    );

    let mut state = InputState::new();
    state.set_input_map(map);

    // Below deadzone — action should NOT fire
    state.process_event(InputEvent::GamepadAxis {
        axis: GamepadAxis::LeftStickX,
        value: 0.1,
        gamepad_id: 0,
    });
    assert!(!state.is_action_pressed("steer"));

    // Above deadzone — action SHOULD fire
    state.process_event(InputEvent::GamepadAxis {
        axis: GamepadAxis::LeftStickX,
        value: 0.5,
        gamepad_id: 0,
    });
    assert!(state.is_action_pressed("steer"));
    assert!((state.get_action_strength("steer") - 0.5).abs() < 0.001);
}

#[test]
fn full_snapshot_roundtrip_with_all_device_types() {
    let mut state = InputState::new();
    state.set_mouse_mode(MouseMode::Captured);
    state.connect_joypad(0);

    // Key
    state.process_event(InputEvent::Key {
        key: Key::Space,
        pressed: true,
        shift: false,
        ctrl: false,
        alt: false,
    });
    // Mouse
    state.process_event(InputEvent::MouseButton {
        button: MouseButton::Right,
        pressed: true,
        position: Vector2::new(400.0, 300.0),
    });
    state.process_event(InputEvent::MouseMotion {
        position: Vector2::new(410.0, 305.0),
        relative: Vector2::new(10.0, 5.0),
    });
    // Gamepad
    state.process_event(InputEvent::GamepadButton {
        button: GamepadButton::Start,
        pressed: true,
        gamepad_id: 0,
    });
    state.process_event(InputEvent::GamepadAxis {
        axis: GamepadAxis::RightStickY,
        value: -0.7,
        gamepad_id: 0,
    });
    // Touch
    state.process_event(InputEvent::ScreenTouch {
        index: 0,
        position: Vector2::new(100.0, 200.0),
        pressed: true,
    });

    let snap = state.snapshot();

    // Verify everything propagated
    assert!(snap.is_key_pressed(Key::Space));
    assert!(snap.is_mouse_button_pressed(MouseButton::Right));
    assert!((snap.get_mouse_position().x - 410.0).abs() < 0.001);
    assert!((snap.get_last_mouse_velocity().x - 10.0).abs() < 0.001);
    assert_eq!(snap.get_mouse_mode(), MouseMode::Captured);
    assert!(snap.is_gamepad_button_pressed(0, GamepadButton::Start));
    assert!((snap.get_gamepad_axis_value(0, GamepadAxis::RightStickY) + 0.7).abs() < 0.001);
    assert_eq!(snap.get_connected_joypads(), vec![0]);
    assert!(snap.is_touch_pressed(0));
    assert!(snap.is_anything_pressed());
}
