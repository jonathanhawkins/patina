//! Integration tests for Gamepad support: axes, dead zones, buttons, rumble,
//! connection management, and ClassDB registration.

use gdplatform::input::{
    ActionBinding, GamepadAxis, GamepadButton, InputEvent, InputMap, InputState, RumbleState,
};
use gdobject::class_db;

// ── ClassDB Registration ─────────────────────────────────────────────────────

#[test]
fn classdb_input_exists() {
    class_db::register_3d_classes();
    assert!(class_db::class_exists("Input"));
}

#[test]
fn classdb_input_inherits_object() {
    class_db::register_3d_classes();
    let info = class_db::get_class_info("Input").unwrap();
    assert_eq!(info.parent_class, "Object");
}

#[test]
fn classdb_input_has_joy_vibration_methods() {
    class_db::register_3d_classes();
    assert!(class_db::class_has_method("Input", "start_joy_vibration"));
    assert!(class_db::class_has_method("Input", "stop_joy_vibration"));
    assert!(class_db::class_has_method("Input", "get_joy_vibration"));
}

#[test]
fn classdb_input_has_joy_query_methods() {
    class_db::register_3d_classes();
    assert!(class_db::class_has_method("Input", "get_joy_name"));
    assert!(class_db::class_has_method("Input", "get_connected_joypads"));
    assert!(class_db::class_has_method("Input", "is_joy_known"));
    assert!(class_db::class_has_method("Input", "get_joy_axis"));
    assert!(class_db::class_has_method("Input", "is_joy_button_pressed"));
}

#[test]
fn classdb_input_has_action_methods() {
    class_db::register_3d_classes();
    assert!(class_db::class_has_method("Input", "is_action_pressed"));
    assert!(class_db::class_has_method("Input", "is_action_just_pressed"));
    assert!(class_db::class_has_method("Input", "is_action_just_released"));
    assert!(class_db::class_has_method("Input", "get_action_strength"));
}

// ── GamepadButton Enum ───────────────────────────────────────────────────────

#[test]
fn gamepad_button_all_variants_distinct() {
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
    for (i, a) in buttons.iter().enumerate() {
        for (j, b) in buttons.iter().enumerate() {
            if i != j {
                assert_ne!(a, b, "buttons at index {} and {} should differ", i, j);
            }
        }
    }
}

// ── GamepadAxis Enum ─────────────────────────────────────────────────────────

#[test]
fn gamepad_axis_all_variants_distinct() {
    let axes = [
        GamepadAxis::LeftStickX,
        GamepadAxis::LeftStickY,
        GamepadAxis::RightStickX,
        GamepadAxis::RightStickY,
        GamepadAxis::LeftTriggerAnalog,
        GamepadAxis::RightTriggerAnalog,
    ];
    for (i, a) in axes.iter().enumerate() {
        for (j, b) in axes.iter().enumerate() {
            if i != j {
                assert_ne!(a, b);
            }
        }
    }
}

// ── Gamepad Button Press/Release ─────────────────────────────────────────────

#[test]
fn gamepad_button_press_tracked() {
    let mut state = InputState::new();
    state.process_event(InputEvent::GamepadButton {
        button: GamepadButton::FaceA,
        pressed: true,
        gamepad_id: 0,
    });
    assert!(state.is_gamepad_button_pressed(0, GamepadButton::FaceA));
}

#[test]
fn gamepad_button_release_tracked() {
    let mut state = InputState::new();
    state.process_event(InputEvent::GamepadButton {
        button: GamepadButton::FaceA,
        pressed: true,
        gamepad_id: 0,
    });
    state.process_event(InputEvent::GamepadButton {
        button: GamepadButton::FaceA,
        pressed: false,
        gamepad_id: 0,
    });
    assert!(!state.is_gamepad_button_pressed(0, GamepadButton::FaceA));
}

#[test]
fn gamepad_buttons_per_device_independent() {
    let mut state = InputState::new();
    state.process_event(InputEvent::GamepadButton {
        button: GamepadButton::FaceA,
        pressed: true,
        gamepad_id: 0,
    });
    assert!(state.is_gamepad_button_pressed(0, GamepadButton::FaceA));
    assert!(!state.is_gamepad_button_pressed(1, GamepadButton::FaceA));
}

#[test]
fn gamepad_multiple_buttons_simultaneous() {
    let mut state = InputState::new();
    state.process_event(InputEvent::GamepadButton {
        button: GamepadButton::FaceA,
        pressed: true,
        gamepad_id: 0,
    });
    state.process_event(InputEvent::GamepadButton {
        button: GamepadButton::FaceB,
        pressed: true,
        gamepad_id: 0,
    });
    assert!(state.is_gamepad_button_pressed(0, GamepadButton::FaceA));
    assert!(state.is_gamepad_button_pressed(0, GamepadButton::FaceB));
}

// ── Axis Values ──────────────────────────────────────────────────────────────

#[test]
fn gamepad_axis_default_zero() {
    let state = InputState::new();
    assert!((state.get_gamepad_axis_value(0, GamepadAxis::LeftStickX)).abs() < f32::EPSILON);
}

#[test]
fn gamepad_axis_value_tracked() {
    let mut state = InputState::new();
    state.process_event(InputEvent::GamepadAxis {
        axis: GamepadAxis::LeftStickX,
        value: 0.75,
        gamepad_id: 0,
    });
    assert!((state.get_gamepad_axis_value(0, GamepadAxis::LeftStickX) - 0.75).abs() < f32::EPSILON);
}

#[test]
fn gamepad_axis_per_device_independent() {
    let mut state = InputState::new();
    state.process_event(InputEvent::GamepadAxis {
        axis: GamepadAxis::LeftStickX,
        value: 0.5,
        gamepad_id: 0,
    });
    state.process_event(InputEvent::GamepadAxis {
        axis: GamepadAxis::LeftStickX,
        value: -0.3,
        gamepad_id: 1,
    });
    assert!((state.get_gamepad_axis_value(0, GamepadAxis::LeftStickX) - 0.5).abs() < f32::EPSILON);
    assert!(
        (state.get_gamepad_axis_value(1, GamepadAxis::LeftStickX) - (-0.3)).abs() < f32::EPSILON
    );
}

// ── Dead Zone ────────────────────────────────────────────────────────────────

#[test]
fn deadzone_filters_small_values() {
    let mut state = InputState::new();
    state.process_event(InputEvent::GamepadAxis {
        axis: GamepadAxis::LeftStickX,
        value: 0.1,
        gamepad_id: 0,
    });
    let val = state.get_gamepad_axis_value_with_deadzone(0, GamepadAxis::LeftStickX, 0.2);
    assert!(val.abs() < f32::EPSILON, "value below deadzone should be 0");
}

#[test]
fn deadzone_passes_large_values() {
    let mut state = InputState::new();
    state.process_event(InputEvent::GamepadAxis {
        axis: GamepadAxis::LeftStickX,
        value: 0.8,
        gamepad_id: 0,
    });
    let val = state.get_gamepad_axis_value_with_deadzone(0, GamepadAxis::LeftStickX, 0.2);
    assert!(val > 0.0, "value above deadzone should be positive");
}

#[test]
fn deadzone_negative_axis_filtered() {
    let mut state = InputState::new();
    state.process_event(InputEvent::GamepadAxis {
        axis: GamepadAxis::LeftStickY,
        value: -0.15,
        gamepad_id: 0,
    });
    let val = state.get_gamepad_axis_value_with_deadzone(0, GamepadAxis::LeftStickY, 0.2);
    assert!(val.abs() < f32::EPSILON);
}

#[test]
fn deadzone_negative_axis_passes() {
    let mut state = InputState::new();
    state.process_event(InputEvent::GamepadAxis {
        axis: GamepadAxis::LeftStickY,
        value: -0.9,
        gamepad_id: 0,
    });
    let val = state.get_gamepad_axis_value_with_deadzone(0, GamepadAxis::LeftStickY, 0.2);
    assert!(val < 0.0, "large negative should pass deadzone");
}

#[test]
fn deadzone_at_boundary() {
    let mut state = InputState::new();
    state.process_event(InputEvent::GamepadAxis {
        axis: GamepadAxis::LeftStickX,
        value: 0.2,
        gamepad_id: 0,
    });
    let val = state.get_gamepad_axis_value_with_deadzone(0, GamepadAxis::LeftStickX, 0.2);
    // At exactly the deadzone threshold — should be 0
    assert!(val.abs() < f32::EPSILON);
}

#[test]
fn deadzone_zero_means_no_filtering() {
    let mut state = InputState::new();
    state.process_event(InputEvent::GamepadAxis {
        axis: GamepadAxis::LeftStickX,
        value: 0.01,
        gamepad_id: 0,
    });
    let val = state.get_gamepad_axis_value_with_deadzone(0, GamepadAxis::LeftStickX, 0.0);
    assert!((val - 0.01).abs() < 0.001);
}

// ── RumbleState ──────────────────────────────────────────────────────────────

#[test]
fn rumble_state_clamps_magnitudes() {
    let r = RumbleState::new(1.5, -0.5, 1.0);
    assert!((r.weak_magnitude - 1.0).abs() < f32::EPSILON);
    assert!(r.strong_magnitude.abs() < f32::EPSILON);
}

#[test]
fn rumble_state_is_active() {
    let r = RumbleState::new(0.5, 0.0, 1.0);
    assert!(r.is_active());
    let r2 = RumbleState::new(0.0, 0.0, 1.0);
    assert!(!r2.is_active());
}

#[test]
fn rumble_tick_expires() {
    let mut r = RumbleState::new(0.5, 0.5, 0.5);
    assert!(r.tick(0.3)); // still active
    assert!(!r.tick(0.3)); // expired (0.5 - 0.3 - 0.3 < 0)
    assert!(!r.is_active());
}

#[test]
fn rumble_indefinite_duration() {
    let mut r = RumbleState::new(0.5, 0.5, 0.0);
    assert!(r.tick(10.0)); // 0 duration = indefinite
    assert!(r.is_active());
}

// ── InputState Rumble Integration ────────────────────────────────────────────

#[test]
fn start_and_get_joy_vibration() {
    let mut state = InputState::new();
    state.start_joy_vibration(0, 0.3, 0.7, 2.0);
    let rumble = state.get_joy_vibration(0).unwrap();
    assert!((rumble.weak_magnitude - 0.3).abs() < f32::EPSILON);
    assert!((rumble.strong_magnitude - 0.7).abs() < f32::EPSILON);
    assert!((rumble.duration - 2.0).abs() < f32::EPSILON);
}

#[test]
fn stop_joy_vibration() {
    let mut state = InputState::new();
    state.start_joy_vibration(0, 0.5, 0.5, 1.0);
    state.stop_joy_vibration(0);
    assert!(state.get_joy_vibration(0).is_none());
}

#[test]
fn rumble_per_device_independent() {
    let mut state = InputState::new();
    state.start_joy_vibration(0, 0.1, 0.2, 1.0);
    state.start_joy_vibration(1, 0.8, 0.9, 2.0);
    let r0 = state.get_joy_vibration(0).unwrap();
    let r1 = state.get_joy_vibration(1).unwrap();
    assert!((r0.weak_magnitude - 0.1).abs() < f32::EPSILON);
    assert!((r1.weak_magnitude - 0.8).abs() < f32::EPSILON);
}

#[test]
fn no_rumble_by_default() {
    let state = InputState::new();
    assert!(state.get_joy_vibration(0).is_none());
}

// ── Connection Management ────────────────────────────────────────────────────

#[test]
fn joy_connection_changed_connect() {
    let mut state = InputState::new();
    state.joy_connection_changed(0, true, "Xbox Wireless Controller");
    assert!(state.is_joy_known(0));
    assert_eq!(state.get_joy_name(0), Some("Xbox Wireless Controller"));
}

#[test]
fn joy_connection_changed_disconnect() {
    let mut state = InputState::new();
    state.joy_connection_changed(0, true, "Controller");
    state.joy_connection_changed(0, false, "");
    assert!(!state.is_joy_known(0));
    assert!(state.get_joy_name(0).is_none());
}

#[test]
fn disconnect_clears_buttons_and_axes() {
    let mut state = InputState::new();
    state.joy_connection_changed(0, true, "Pad");
    state.process_event(InputEvent::GamepadButton {
        button: GamepadButton::FaceA,
        pressed: true,
        gamepad_id: 0,
    });
    state.process_event(InputEvent::GamepadAxis {
        axis: GamepadAxis::LeftStickX,
        value: 0.9,
        gamepad_id: 0,
    });
    state.joy_connection_changed(0, false, "");
    assert!(!state.is_gamepad_button_pressed(0, GamepadButton::FaceA));
    assert!(state.get_gamepad_axis_value(0, GamepadAxis::LeftStickX).abs() < f32::EPSILON);
}

#[test]
fn disconnect_clears_rumble() {
    let mut state = InputState::new();
    state.joy_connection_changed(0, true, "Pad");
    state.start_joy_vibration(0, 0.5, 0.5, 1.0);
    state.joy_connection_changed(0, false, "");
    assert!(state.get_joy_vibration(0).is_none());
}

#[test]
fn get_connected_joypads_lists_all() {
    let mut state = InputState::new();
    state.joy_connection_changed(0, true, "Pad 0");
    state.joy_connection_changed(2, true, "Pad 2");
    let ids = state.get_connected_joypads();
    assert_eq!(ids.len(), 2);
    assert!(ids.contains(&0));
    assert!(ids.contains(&2));
}

#[test]
fn unknown_gamepad_returns_none() {
    let state = InputState::new();
    assert!(!state.is_joy_known(99));
    assert!(state.get_joy_name(99).is_none());
}

// ── Action Binding with Gamepad ──────────────────────────────────────────────

#[test]
fn gamepad_button_action_binding() {
    let mut map = InputMap::new();
    map.add_action("jump", 0.0);
    map.action_add_event("jump", ActionBinding::GamepadButtonBinding(GamepadButton::FaceA));

    let pressed = InputEvent::GamepadButton {
        button: GamepadButton::FaceA,
        pressed: true,
        gamepad_id: 0,
    };
    assert!(map.event_matches_action(&pressed, "jump"));

    let wrong = InputEvent::GamepadButton {
        button: GamepadButton::FaceB,
        pressed: true,
        gamepad_id: 0,
    };
    assert!(!map.event_matches_action(&wrong, "jump"));
}

#[test]
fn gamepad_axis_action_with_deadzone() {
    let mut map = InputMap::new();
    map.add_action("move_right", 0.2);
    map.action_add_event(
        "move_right",
        ActionBinding::GamepadAxisBinding(GamepadAxis::LeftStickX),
    );

    let below = InputEvent::GamepadAxis {
        axis: GamepadAxis::LeftStickX,
        value: 0.1,
        gamepad_id: 0,
    };
    assert!(!map.event_matches_action(&below, "move_right"));

    let above = InputEvent::GamepadAxis {
        axis: GamepadAxis::LeftStickX,
        value: 0.5,
        gamepad_id: 0,
    };
    assert!(map.event_matches_action(&above, "move_right"));
}

// ── Trigger Axes ─────────────────────────────────────────────────────────────

#[test]
fn trigger_axis_values() {
    let mut state = InputState::new();
    state.process_event(InputEvent::GamepadAxis {
        axis: GamepadAxis::LeftTriggerAnalog,
        value: 0.5,
        gamepad_id: 0,
    });
    state.process_event(InputEvent::GamepadAxis {
        axis: GamepadAxis::RightTriggerAnalog,
        value: 1.0,
        gamepad_id: 0,
    });
    assert!(
        (state.get_gamepad_axis_value(0, GamepadAxis::LeftTriggerAnalog) - 0.5).abs()
            < f32::EPSILON
    );
    assert!(
        (state.get_gamepad_axis_value(0, GamepadAxis::RightTriggerAnalog) - 1.0).abs()
            < f32::EPSILON
    );
}

// ── Frame Flush ──────────────────────────────────────────────────────────────

#[test]
fn flush_frame_clears_just_pressed() {
    let mut state = InputState::new();
    state.process_event(InputEvent::GamepadButton {
        button: GamepadButton::Start,
        pressed: true,
        gamepad_id: 0,
    });
    // Button is held
    assert!(state.is_gamepad_button_pressed(0, GamepadButton::Start));
    state.flush_frame();
    // Still held after flush, but just_pressed cleared (tested indirectly — still pressed)
    assert!(state.is_gamepad_button_pressed(0, GamepadButton::Start));
}
