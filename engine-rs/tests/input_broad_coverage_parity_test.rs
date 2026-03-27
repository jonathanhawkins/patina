//! pat-27wf: Broaden input handling coverage beyond the initial 2D slice.
//!
//! Tests the full InputState -> MainLoop -> InputSnapshot pipeline for:
//!   - Key just-released detection
//!   - Action just-released detection
//!   - Action strength (analog and digital)
//!   - get_axis / get_vector on InputSnapshot
//!   - Mouse button just-pressed / just-released
//!   - SceneAccess bridge methods for all new queries
//!
//! Each test pushes events into MainLoop, steps, and verifies the script-facing
//! InputSnapshot carries the correct state through the full pipeline.

use gdplatform::input::{ActionBinding, InputEvent, InputMap, Key, MouseButton};
use gdscene::main_loop::MainLoop;
use gdscene::scene_tree::SceneTree;
use gdscene::scripting::InputSnapshot;

/// Helper: create a key press/release event with default modifiers.
fn key_event(key: Key, pressed: bool) -> InputEvent {
    InputEvent::Key {
        key,
        pressed,
        shift: false,
        ctrl: false,
        alt: false,
    }
}

fn make_main_loop_with_actions(actions: &[(&str, &[Key])]) -> MainLoop {
    let tree = SceneTree::new();
    let mut ml = MainLoop::new(tree);
    let mut input_map = InputMap::new();
    for &(name, keys) in actions {
        input_map.add_action(name, 0.0);
        for &key in keys {
            input_map.action_add_event(name, ActionBinding::KeyBinding(key));
        }
    }
    ml.set_input_map(input_map);
    ml
}

// ===========================================================================
// 1. Key just-released through full pipeline
// ===========================================================================

#[test]
fn key_just_released_bridged_through_step() {
    let tree = SceneTree::new();
    let mut ml = MainLoop::new(tree);

    // Frame 1: press A
    ml.push_event(key_event(Key::A, true));
    ml.step(1.0 / 60.0);

    // Frame 2: release A
    ml.push_event(key_event(Key::A, false));
    assert!(
        ml.input_state().is_key_just_released(Key::A),
        "InputState should have A as just_released before step"
    );
    ml.step(1.0 / 60.0);

    // After step, just_released is flushed
    assert!(
        !ml.input_state().is_key_just_released(Key::A),
        "just_released should be flushed after step"
    );
}

// ===========================================================================
// 2. Action just-released through full pipeline
// ===========================================================================

#[test]
fn action_just_released_through_pipeline() {
    let mut ml = make_main_loop_with_actions(&[("jump", &[Key::Space])]);

    ml.push_event(key_event(Key::Space, true));
    ml.step(1.0 / 60.0);
    assert!(ml.input_state().is_action_pressed("jump"));

    ml.push_event(key_event(Key::Space, false));
    assert!(ml.input_state().is_action_just_released("jump"));
    ml.step(1.0 / 60.0);

    assert!(!ml.input_state().is_action_just_released("jump"));
    assert!(!ml.input_state().is_action_pressed("jump"));
}

// ===========================================================================
// 3. Action strength (digital keys report 1.0)
// ===========================================================================

#[test]
fn digital_action_strength_is_one_when_pressed() {
    let mut ml = make_main_loop_with_actions(&[("fire", &[Key::F])]);

    ml.push_event(key_event(Key::F, true));
    assert_eq!(ml.input_state().get_action_strength("fire"), 1.0);
    ml.step(1.0 / 60.0);

    ml.push_event(key_event(Key::F, false));
    ml.step(1.0 / 60.0);
    assert_eq!(ml.input_state().get_action_strength("fire"), 0.0);
}

// ===========================================================================
// 4. Analog action strength via Action event
// ===========================================================================

#[test]
fn analog_action_strength_through_pipeline() {
    let mut ml = make_main_loop_with_actions(&[("throttle", &[])]);

    ml.push_event(InputEvent::Action {
        action: "throttle".to_string(),
        pressed: true,
        strength: 0.75,
    });
    assert!(
        (ml.input_state().get_action_strength("throttle") - 0.75).abs() < 0.001
    );
    ml.step(1.0 / 60.0);

    ml.push_event(InputEvent::Action {
        action: "throttle".to_string(),
        pressed: true,
        strength: 0.3,
    });
    assert!(
        (ml.input_state().get_action_strength("throttle") - 0.3).abs() < 0.001
    );
}

// ===========================================================================
// 5. get_axis on InputSnapshot
// ===========================================================================

#[test]
fn snapshot_get_axis_positive() {
    let mut snap = InputSnapshot::default();
    snap.pressed_keys.insert("D".to_string());
    snap.input_map
        .insert("move_right".to_string(), vec!["D".to_string()]);
    snap.input_map
        .insert("move_left".to_string(), vec!["A".to_string()]);
    snap.action_strengths
        .insert("move_right".to_string(), 1.0);

    assert_eq!(snap.get_axis("move_left", "move_right"), 1.0);
}

#[test]
fn snapshot_get_axis_negative() {
    let mut snap = InputSnapshot::default();
    snap.pressed_keys.insert("A".to_string());
    snap.input_map
        .insert("move_left".to_string(), vec!["A".to_string()]);
    snap.input_map
        .insert("move_right".to_string(), vec!["D".to_string()]);
    snap.action_strengths
        .insert("move_left".to_string(), 1.0);

    assert_eq!(snap.get_axis("move_left", "move_right"), -1.0);
}

#[test]
fn snapshot_get_axis_both_pressed_cancels() {
    let mut snap = InputSnapshot::default();
    snap.pressed_keys.insert("A".to_string());
    snap.pressed_keys.insert("D".to_string());
    snap.input_map
        .insert("move_left".to_string(), vec!["A".to_string()]);
    snap.input_map
        .insert("move_right".to_string(), vec!["D".to_string()]);
    snap.action_strengths
        .insert("move_left".to_string(), 1.0);
    snap.action_strengths
        .insert("move_right".to_string(), 1.0);

    assert_eq!(snap.get_axis("move_left", "move_right"), 0.0);
}

// ===========================================================================
// 6. get_vector on InputSnapshot
// ===========================================================================

#[test]
fn snapshot_get_vector_single_axis() {
    let mut snap = InputSnapshot::default();
    snap.pressed_keys.insert("D".to_string());
    snap.input_map
        .insert("right".to_string(), vec!["D".to_string()]);
    snap.input_map
        .insert("left".to_string(), vec!["A".to_string()]);
    snap.input_map
        .insert("up".to_string(), vec!["W".to_string()]);
    snap.input_map
        .insert("down".to_string(), vec!["S".to_string()]);
    snap.action_strengths.insert("right".to_string(), 1.0);

    let v = snap.get_vector("left", "right", "up", "down");
    assert!((v.x - 1.0).abs() < 0.001);
    assert!(v.y.abs() < 0.001);
}

#[test]
fn snapshot_get_vector_diagonal_normalized() {
    let mut snap = InputSnapshot::default();
    snap.pressed_keys.insert("D".to_string());
    snap.pressed_keys.insert("S".to_string());
    snap.input_map
        .insert("right".to_string(), vec!["D".to_string()]);
    snap.input_map
        .insert("left".to_string(), vec!["A".to_string()]);
    snap.input_map
        .insert("up".to_string(), vec!["W".to_string()]);
    snap.input_map
        .insert("down".to_string(), vec!["S".to_string()]);
    snap.action_strengths.insert("right".to_string(), 1.0);
    snap.action_strengths.insert("down".to_string(), 1.0);

    let v = snap.get_vector("left", "right", "up", "down");
    let len = (v.x * v.x + v.y * v.y).sqrt();
    assert!(
        (len - 1.0).abs() < 0.01,
        "diagonal should be normalized, got len={}",
        len
    );
    assert!(v.x > 0.0 && v.y > 0.0, "should point down-right");
}

// ===========================================================================
// 7. Mouse button just-pressed / just-released
// ===========================================================================

#[test]
fn mouse_just_pressed_through_pipeline() {
    let tree = SceneTree::new();
    let mut ml = MainLoop::new(tree);

    ml.push_event(InputEvent::MouseButton {
        button: MouseButton::Left,
        pressed: true,
        position: gdcore::math::Vector2::ZERO,
    });
    assert!(ml.input_state().is_mouse_button_just_pressed(MouseButton::Left));
    ml.step(1.0 / 60.0);
    assert!(!ml.input_state().is_mouse_button_just_pressed(MouseButton::Left));
    assert!(ml.input_state().is_mouse_button_pressed(MouseButton::Left));
}

#[test]
fn mouse_just_released_through_pipeline() {
    let tree = SceneTree::new();
    let mut ml = MainLoop::new(tree);

    ml.push_event(InputEvent::MouseButton {
        button: MouseButton::Right,
        pressed: true,
        position: gdcore::math::Vector2::ZERO,
    });
    ml.step(1.0 / 60.0);

    ml.push_event(InputEvent::MouseButton {
        button: MouseButton::Right,
        pressed: false,
        position: gdcore::math::Vector2::ZERO,
    });
    assert!(ml.input_state().is_mouse_button_just_released(MouseButton::Right));
    ml.step(1.0 / 60.0);
    assert!(!ml.input_state().is_mouse_button_just_released(MouseButton::Right));
    assert!(!ml.input_state().is_mouse_button_pressed(MouseButton::Right));
}

// ===========================================================================
// 8. InputSnapshot direct unit tests for new methods
// ===========================================================================

#[test]
fn snapshot_is_key_just_released() {
    let mut snap = InputSnapshot::default();
    snap.just_released_keys.insert("Escape".to_string());
    assert!(snap.is_key_just_released("Escape"));
    assert!(!snap.is_key_just_released("Space"));
}

#[test]
fn snapshot_is_action_just_released() {
    let mut snap = InputSnapshot::default();
    snap.actions_just_released.insert("jump".to_string());
    assert!(snap.is_action_just_released("jump"));
    assert!(!snap.is_action_just_released("fire"));
}

#[test]
fn snapshot_mouse_button_just_pressed_and_released() {
    let mut snap = InputSnapshot::default();
    snap.just_pressed_mouse_buttons.insert("1".to_string());
    snap.just_released_mouse_buttons.insert("2".to_string());

    assert!(snap.is_mouse_button_just_pressed(1));
    assert!(!snap.is_mouse_button_just_pressed(2));
    assert!(snap.is_mouse_button_just_released(2));
    assert!(!snap.is_mouse_button_just_released(1));
}

// ===========================================================================
// 9. get_axis through full MainLoop pipeline
// ===========================================================================

#[test]
fn get_axis_through_mainloop_pipeline() {
    let mut ml = make_main_loop_with_actions(&[
        ("move_left", &[Key::A]),
        ("move_right", &[Key::D]),
    ]);

    // Press right only
    ml.push_event(key_event(Key::D, true));
    assert_eq!(ml.input_state().get_axis("move_left", "move_right"), 1.0);
    ml.step(1.0 / 60.0);

    // Press both -- cancels
    ml.push_event(key_event(Key::A, true));
    assert_eq!(ml.input_state().get_axis("move_left", "move_right"), 0.0);
    ml.step(1.0 / 60.0);

    // Release right, only left held
    ml.push_event(key_event(Key::D, false));
    assert_eq!(ml.input_state().get_axis("move_left", "move_right"), -1.0);
}

// ===========================================================================
// 10. get_vector through full MainLoop pipeline
// ===========================================================================

#[test]
fn get_vector_through_mainloop_pipeline() {
    let mut ml = make_main_loop_with_actions(&[
        ("left", &[Key::A]),
        ("right", &[Key::D]),
        ("up", &[Key::W]),
        ("down", &[Key::S]),
    ]);

    // Press right + down (diagonal)
    ml.push_event(key_event(Key::D, true));
    ml.push_event(key_event(Key::S, true));

    let v = ml.input_state().get_vector("left", "right", "up", "down");
    let len = (v.x * v.x + v.y * v.y).sqrt();
    assert!(
        (len - 1.0).abs() < 0.01,
        "diagonal should be normalized, got len={}",
        len
    );
}

// ===========================================================================
// 11. Unmapped actions return zero/false (Godot parity)
// ===========================================================================

#[test]
fn unmapped_action_returns_defaults() {
    let tree = SceneTree::new();
    let ml = MainLoop::new(tree);

    assert!(!ml.input_state().is_action_pressed("nonexistent"));
    assert!(!ml.input_state().is_action_just_pressed("nonexistent"));
    assert!(!ml.input_state().is_action_just_released("nonexistent"));
    assert_eq!(ml.input_state().get_action_strength("nonexistent"), 0.0);
    assert_eq!(ml.input_state().get_axis("no_neg", "no_pos"), 0.0);
}

// ===========================================================================
// 12. Multiple actions on same key
// ===========================================================================

#[test]
fn multiple_actions_same_key() {
    let mut ml = make_main_loop_with_actions(&[
        ("jump", &[Key::Space]),
        ("confirm", &[Key::Space]),
    ]);

    ml.push_event(key_event(Key::Space, true));
    assert!(ml.input_state().is_action_pressed("jump"));
    assert!(ml.input_state().is_action_pressed("confirm"));
    ml.step(1.0 / 60.0);

    ml.push_event(key_event(Key::Space, false));
    assert!(ml.input_state().is_action_just_released("jump"));
    assert!(ml.input_state().is_action_just_released("confirm"));
}

// ===========================================================================
// 13. Action with multiple keys -- releasing one releases action
// ===========================================================================
// NOTE: Godot keeps the action pressed if another bound key is still held.
// Our engine currently releases the action on any matching key release.
// This is a known parity gap tracked separately.

#[test]
fn action_with_multiple_keys_partial_release_current_behavior() {
    let mut ml = make_main_loop_with_actions(&[("fire", &[Key::F, Key::Space])]);

    ml.push_event(key_event(Key::F, true));
    ml.push_event(key_event(Key::Space, true));
    ml.step(1.0 / 60.0);

    // Release only F -- in our engine, action is released even though Space is still held.
    // Godot would keep the action pressed. This documents current behavior.
    ml.push_event(key_event(Key::F, false));
    assert!(
        !ml.input_state().is_action_pressed("fire"),
        "current behavior: action released when any bound key is released"
    );
}

// ===========================================================================
// 14. Touch events don't crash (basic smoke)
// ===========================================================================

#[test]
fn touch_events_basic_smoke() {
    let tree = SceneTree::new();
    let mut ml = MainLoop::new(tree);

    ml.push_event(InputEvent::ScreenTouch {
        index: 0,
        pressed: true,
        position: gdcore::math::Vector2::new(100.0, 200.0),
    });
    ml.step(1.0 / 60.0);

    ml.push_event(InputEvent::ScreenTouch {
        index: 0,
        pressed: false,
        position: gdcore::math::Vector2::new(100.0, 200.0),
    });
    ml.step(1.0 / 60.0);
}

// ===========================================================================
// 15. Gamepad events don't crash (basic smoke)
// ===========================================================================

#[test]
fn gamepad_events_basic_smoke() {
    let tree = SceneTree::new();
    let mut ml = MainLoop::new(tree);

    ml.push_event(InputEvent::GamepadButton {
        gamepad_id: 0,
        button: gdplatform::input::GamepadButton::FaceA,
        pressed: true,
    });
    ml.step(1.0 / 60.0);

    ml.push_event(InputEvent::GamepadButton {
        gamepad_id: 0,
        button: gdplatform::input::GamepadButton::FaceA,
        pressed: false,
    });
    ml.step(1.0 / 60.0);
}

// ===========================================================================
// 16. Input snapshot default is completely empty
// ===========================================================================

#[test]
fn default_snapshot_is_empty() {
    let snap = InputSnapshot::default();
    assert!(snap.pressed_keys.is_empty());
    assert!(snap.just_pressed_keys.is_empty());
    assert!(snap.just_released_keys.is_empty());
    assert!(snap.input_map.is_empty());
    assert!(snap.mouse_buttons_pressed.is_empty());
    assert!(snap.just_pressed_mouse_buttons.is_empty());
    assert!(snap.just_released_mouse_buttons.is_empty());
    assert!(snap.actions_just_released.is_empty());
    assert!(snap.action_strengths.is_empty());
    assert!(!snap.is_action_pressed("any"));
    assert!(!snap.is_action_just_pressed("any"));
    assert!(!snap.is_action_just_released("any"));
    assert_eq!(snap.get_action_strength("any"), 0.0);
    assert_eq!(snap.get_axis("neg", "pos"), 0.0);
    let v = snap.get_vector("l", "r", "u", "d");
    assert_eq!(v.x, 0.0);
    assert_eq!(v.y, 0.0);
}

// ===========================================================================
// 17. Gamepad button action through MainLoop pipeline
// ===========================================================================

#[test]
fn gamepad_button_triggers_action_through_mainloop() {
    let tree = SceneTree::new();
    let mut ml = MainLoop::new(tree);
    let mut input_map = InputMap::new();
    input_map.add_action("jump", 0.0);
    input_map.action_add_event("jump", ActionBinding::GamepadButtonBinding(
        gdplatform::input::GamepadButton::FaceA,
    ));
    ml.set_input_map(input_map);

    ml.push_event(InputEvent::GamepadButton {
        gamepad_id: 0,
        button: gdplatform::input::GamepadButton::FaceA,
        pressed: true,
    });
    assert!(ml.input_state().is_action_pressed("jump"));
    assert!(ml.input_state().is_action_just_pressed("jump"));

    ml.step(1.0 / 60.0);
    assert!(!ml.input_state().is_action_just_pressed("jump"));
    assert!(ml.input_state().is_action_pressed("jump"));

    ml.push_event(InputEvent::GamepadButton {
        gamepad_id: 0,
        button: gdplatform::input::GamepadButton::FaceA,
        pressed: false,
    });
    assert!(ml.input_state().is_action_just_released("jump"));
    ml.step(1.0 / 60.0);
    assert!(!ml.input_state().is_action_pressed("jump"));
}

// ===========================================================================
// 18. Mouse button action through MainLoop pipeline
// ===========================================================================

#[test]
fn mouse_button_triggers_action_through_mainloop() {
    let tree = SceneTree::new();
    let mut ml = MainLoop::new(tree);
    let mut input_map = InputMap::new();
    input_map.add_action("shoot", 0.0);
    input_map.action_add_event("shoot", ActionBinding::MouseBinding(MouseButton::Left));
    ml.set_input_map(input_map);

    ml.push_event(InputEvent::MouseButton {
        button: MouseButton::Left,
        pressed: true,
        position: gdcore::math::Vector2::new(320.0, 240.0),
    });
    assert!(ml.input_state().is_action_pressed("shoot"));
    assert_eq!(ml.input_state().get_action_strength("shoot"), 1.0);
    ml.step(1.0 / 60.0);

    ml.push_event(InputEvent::MouseButton {
        button: MouseButton::Left,
        pressed: false,
        position: gdcore::math::Vector2::new(320.0, 240.0),
    });
    assert!(ml.input_state().is_action_just_released("shoot"));
}

// ===========================================================================
// 19. Mouse motion updates position through pipeline
// ===========================================================================

#[test]
fn mouse_motion_updates_position_through_mainloop() {
    let tree = SceneTree::new();
    let mut ml = MainLoop::new(tree);

    ml.push_event(InputEvent::MouseMotion {
        position: gdcore::math::Vector2::new(400.0, 300.0),
        relative: gdcore::math::Vector2::new(10.0, -5.0),
    });
    let pos = ml.input_state().get_mouse_position();
    assert!((pos.x - 400.0).abs() < f32::EPSILON);
    assert!((pos.y - 300.0).abs() < f32::EPSILON);
}

// ===========================================================================
// 20. Multiple keys held then released in sequence
// ===========================================================================

#[test]
fn multiple_keys_held_then_released_in_sequence() {
    let tree = SceneTree::new();
    let mut ml = MainLoop::new(tree);

    // Press A, B, C across frames
    ml.push_event(key_event(Key::A, true));
    ml.step(1.0 / 60.0);
    ml.push_event(key_event(Key::B, true));
    ml.step(1.0 / 60.0);
    ml.push_event(key_event(Key::C, true));
    ml.step(1.0 / 60.0);

    // All three should be pressed
    assert!(ml.input_state().is_key_pressed(Key::A));
    assert!(ml.input_state().is_key_pressed(Key::B));
    assert!(ml.input_state().is_key_pressed(Key::C));

    // Release B only
    ml.push_event(key_event(Key::B, false));
    assert!(ml.input_state().is_key_just_released(Key::B));
    assert!(ml.input_state().is_key_pressed(Key::A));
    assert!(!ml.input_state().is_key_pressed(Key::B));
    assert!(ml.input_state().is_key_pressed(Key::C));
}

// ===========================================================================
// 21. Rapid press-release within same frame
// ===========================================================================

#[test]
fn rapid_press_release_same_frame() {
    let tree = SceneTree::new();
    let mut ml = MainLoop::new(tree);

    // Press and release in same frame (before step)
    ml.push_event(key_event(Key::Enter, true));
    ml.push_event(key_event(Key::Enter, false));

    // Key should not be pressed (released immediately)
    assert!(!ml.input_state().is_key_pressed(Key::Enter));
    // just_released should be set since it was pressed then released
    assert!(ml.input_state().is_key_just_released(Key::Enter));

    ml.step(1.0 / 60.0);
    assert!(!ml.input_state().is_key_just_released(Key::Enter));
}

// ===========================================================================
// 22. Action strength preserved across step (held key)
// ===========================================================================

#[test]
fn action_strength_persists_while_held() {
    let mut ml = make_main_loop_with_actions(&[("fire", &[Key::F])]);

    ml.push_event(key_event(Key::F, true));
    assert_eq!(ml.input_state().get_action_strength("fire"), 1.0);
    ml.step(1.0 / 60.0);
    // Strength should persist while key is held
    assert_eq!(ml.input_state().get_action_strength("fire"), 1.0);
    ml.step(1.0 / 60.0);
    assert_eq!(ml.input_state().get_action_strength("fire"), 1.0);
}

// ===========================================================================
// 23. Touch through MainLoop pipeline triggers touch state
// ===========================================================================

#[test]
fn touch_state_through_mainloop_pipeline() {
    let tree = SceneTree::new();
    let mut ml = MainLoop::new(tree);

    ml.push_event(InputEvent::ScreenTouch {
        index: 0,
        pressed: true,
        position: gdcore::math::Vector2::new(200.0, 150.0),
    });
    assert!(ml.input_state().is_touch_pressed(0));
    assert!(ml.input_state().is_touch_just_pressed(0));

    ml.step(1.0 / 60.0);
    assert!(ml.input_state().is_touch_pressed(0));
    assert!(!ml.input_state().is_touch_just_pressed(0));

    ml.push_event(InputEvent::ScreenTouch {
        index: 0,
        pressed: false,
        position: gdcore::math::Vector2::new(200.0, 150.0),
    });
    assert!(ml.input_state().is_touch_just_released(0));
    ml.step(1.0 / 60.0);
    assert!(!ml.input_state().is_touch_pressed(0));
    assert!(!ml.input_state().is_touch_just_released(0));
}
