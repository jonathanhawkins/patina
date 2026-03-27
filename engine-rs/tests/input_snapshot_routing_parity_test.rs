//! pat-7us3: Engine-owned input snapshot and routing API parity coverage.
//!
//! Validates snapshot semantics and routing through the engine-owned InputState
//! without demo-local shortcuts:
//!
//! 1. Snapshot immutability — changes to InputState don't affect existing snapshots
//! 2. Mouse event routing through push_event
//! 3. Gamepad button/axis routing through push_event
//! 4. Action strength and analog axis semantics
//! 5. get_axis / get_vector from engine-owned state
//! 6. Key release → action just_released semantics
//! 7. Multiple simultaneous keys/actions
//! 8. InputMap hot-swap mid-session
//! 9. Empty/missing input map edge cases
//! 10. flush_frame lifecycle across multiple frames

use gdplatform::input::{
    ActionBinding, GamepadAxis, GamepadButton, InputEvent, InputMap, InputState, Key, MouseButton,
};
use gdcore::math::Vector2;
use gdscene::MainLoop;
use gdscene::scene_tree::SceneTree;

// ===========================================================================
// Helpers
// ===========================================================================

fn key_press(key: Key) -> InputEvent {
    InputEvent::Key {
        key,
        pressed: true,
        shift: false,
        ctrl: false,
        alt: false,
    }
}

fn key_release(key: Key) -> InputEvent {
    InputEvent::Key {
        key,
        pressed: false,
        shift: false,
        ctrl: false,
        alt: false,
    }
}

fn mouse_press(button: MouseButton, position: Vector2) -> InputEvent {
    InputEvent::MouseButton {
        button,
        pressed: true,
        position,
    }
}

fn mouse_release(button: MouseButton, position: Vector2) -> InputEvent {
    InputEvent::MouseButton {
        button,
        pressed: false,
        position,
    }
}

fn gamepad_button_press(button: GamepadButton, gamepad_id: u32) -> InputEvent {
    InputEvent::GamepadButton {
        button,
        pressed: true,
        gamepad_id,
    }
}

fn gamepad_button_release(button: GamepadButton, gamepad_id: u32) -> InputEvent {
    InputEvent::GamepadButton {
        button,
        pressed: false,
        gamepad_id,
    }
}

fn gamepad_axis_event(axis: GamepadAxis, value: f32, gamepad_id: u32) -> InputEvent {
    InputEvent::GamepadAxis {
        axis,
        value,
        gamepad_id,
    }
}

fn make_directional_map() -> InputMap {
    let mut map = InputMap::new();
    map.add_action("move_left", 0.0);
    map.action_add_event("move_left", ActionBinding::KeyBinding(Key::Left));

    map.add_action("move_right", 0.0);
    map.action_add_event("move_right", ActionBinding::KeyBinding(Key::Right));

    map.add_action("move_up", 0.0);
    map.action_add_event("move_up", ActionBinding::KeyBinding(Key::Up));

    map.add_action("move_down", 0.0);
    map.action_add_event("move_down", ActionBinding::KeyBinding(Key::Down));

    map.add_action("shoot", 0.0);
    map.action_add_event("shoot", ActionBinding::KeyBinding(Key::Space));

    map
}

// ===========================================================================
// 1. Snapshot immutability — frozen point-in-time
// ===========================================================================

#[test]
fn snapshot_is_frozen_after_creation() {
    let mut state = InputState::new();

    state.process_event(key_press(Key::A));
    let snap = state.snapshot();

    // Verify snapshot has the key
    assert!(snap.is_key_pressed(Key::A));
    assert!(snap.is_key_just_pressed(Key::A));

    // Now release the key in InputState
    state.process_event(key_release(Key::A));

    // Snapshot is immutable — still shows A pressed
    assert!(snap.is_key_pressed(Key::A));
    assert!(snap.is_key_just_pressed(Key::A));

    // But a new snapshot reflects the release
    let snap2 = state.snapshot();
    assert!(!snap2.is_key_pressed(Key::A));
    assert!(snap2.is_key_just_released(Key::A));
}

#[test]
fn snapshot_does_not_reflect_later_key_presses() {
    let mut state = InputState::new();

    let snap_empty = state.snapshot();
    assert!(!snap_empty.is_key_pressed(Key::B));

    // Press B after snapshot was taken
    state.process_event(key_press(Key::B));

    // Original snapshot unchanged
    assert!(!snap_empty.is_key_pressed(Key::B));
}

#[test]
fn multiple_snapshots_are_independent() {
    let mut state = InputState::new();

    state.process_event(key_press(Key::A));
    let snap1 = state.snapshot();

    state.flush_frame();
    state.process_event(key_press(Key::B));
    let snap2 = state.snapshot();

    // snap1 has A just_pressed, no B
    assert!(snap1.is_key_pressed(Key::A));
    assert!(snap1.is_key_just_pressed(Key::A));
    assert!(!snap1.is_key_pressed(Key::B));

    // snap2 has A pressed (still held), B just_pressed, A NOT just_pressed (flushed)
    assert!(snap2.is_key_pressed(Key::A));
    assert!(!snap2.is_key_just_pressed(Key::A));
    assert!(snap2.is_key_pressed(Key::B));
    assert!(snap2.is_key_just_pressed(Key::B));
}

// ===========================================================================
// 2. Mouse event routing
// ===========================================================================

#[test]
fn mouse_button_press_tracked_in_state_and_snapshot() {
    let mut state = InputState::new();

    state.process_event(mouse_press(MouseButton::Left, Vector2::new(100.0, 200.0)));

    assert!(state.is_mouse_button_pressed(MouseButton::Left));
    assert!(state.is_mouse_button_just_pressed(MouseButton::Left));
    assert!(!state.is_mouse_button_pressed(MouseButton::Right));

    let snap = state.snapshot();
    assert!(snap.is_mouse_button_pressed(MouseButton::Left));
    assert!(snap.is_mouse_button_just_pressed(MouseButton::Left));
    assert_eq!(snap.get_mouse_position(), Vector2::new(100.0, 200.0));
}

#[test]
fn mouse_button_release_tracked() {
    let mut state = InputState::new();

    state.process_event(mouse_press(MouseButton::Left, Vector2::new(10.0, 20.0)));
    state.flush_frame();
    state.process_event(mouse_release(MouseButton::Left, Vector2::new(30.0, 40.0)));

    assert!(!state.is_mouse_button_pressed(MouseButton::Left));
    assert!(state.is_mouse_button_just_released(MouseButton::Left));

    let snap = state.snapshot();
    assert!(!snap.is_mouse_button_pressed(MouseButton::Left));
    assert!(snap.is_mouse_button_just_released(MouseButton::Left));
    // Position updated to release position
    assert_eq!(snap.get_mouse_position(), Vector2::new(30.0, 40.0));
}

#[test]
fn mouse_motion_updates_position() {
    let mut state = InputState::new();

    state.process_event(InputEvent::MouseMotion {
        position: Vector2::new(320.0, 240.0),
        relative: Vector2::new(5.0, -3.0),
    });

    assert_eq!(state.get_mouse_position(), Vector2::new(320.0, 240.0));

    let snap = state.snapshot();
    assert_eq!(snap.get_mouse_position(), Vector2::new(320.0, 240.0));
}

#[test]
fn multiple_mouse_buttons_tracked_independently() {
    let mut state = InputState::new();

    state.process_event(mouse_press(MouseButton::Left, Vector2::ZERO));
    state.process_event(mouse_press(MouseButton::Right, Vector2::ZERO));

    assert!(state.is_mouse_button_pressed(MouseButton::Left));
    assert!(state.is_mouse_button_pressed(MouseButton::Right));
    assert!(!state.is_mouse_button_pressed(MouseButton::Middle));

    state.process_event(mouse_release(MouseButton::Left, Vector2::ZERO));
    assert!(!state.is_mouse_button_pressed(MouseButton::Left));
    assert!(state.is_mouse_button_pressed(MouseButton::Right));
}

// ===========================================================================
// 3. Gamepad button/axis routing
// ===========================================================================

#[test]
fn gamepad_button_press_tracked() {
    let mut state = InputState::new();

    state.process_event(gamepad_button_press(GamepadButton::FaceA, 0));

    assert!(state.is_gamepad_button_pressed(0, GamepadButton::FaceA));
    assert!(!state.is_gamepad_button_pressed(0, GamepadButton::FaceB));
    assert!(!state.is_gamepad_button_pressed(1, GamepadButton::FaceA));
}

#[test]
fn gamepad_button_release_tracked() {
    let mut state = InputState::new();

    state.process_event(gamepad_button_press(GamepadButton::FaceA, 0));
    state.process_event(gamepad_button_release(GamepadButton::FaceA, 0));

    assert!(!state.is_gamepad_button_pressed(0, GamepadButton::FaceA));
}

#[test]
fn gamepad_axis_value_tracked() {
    let mut state = InputState::new();

    state.process_event(gamepad_axis_event(GamepadAxis::LeftStickX, 0.75, 0));

    assert!(
        (state.get_gamepad_axis_value(0, GamepadAxis::LeftStickX) - 0.75).abs() < 0.001
    );
    // Different gamepad returns 0
    assert_eq!(state.get_gamepad_axis_value(1, GamepadAxis::LeftStickX), 0.0);
    // Different axis returns 0
    assert_eq!(state.get_gamepad_axis_value(0, GamepadAxis::LeftStickY), 0.0);
}

#[test]
fn multiple_gamepads_tracked_independently() {
    let mut state = InputState::new();

    state.process_event(gamepad_button_press(GamepadButton::FaceA, 0));
    state.process_event(gamepad_button_press(GamepadButton::FaceB, 1));

    assert!(state.is_gamepad_button_pressed(0, GamepadButton::FaceA));
    assert!(!state.is_gamepad_button_pressed(0, GamepadButton::FaceB));
    assert!(state.is_gamepad_button_pressed(1, GamepadButton::FaceB));
    assert!(!state.is_gamepad_button_pressed(1, GamepadButton::FaceA));
}

// ===========================================================================
// 4. Action strength and analog semantics
// ===========================================================================

#[test]
fn digital_action_has_strength_one() {
    let mut state = InputState::new();
    state.set_input_map(make_directional_map());

    state.process_event(key_press(Key::Right));

    assert_eq!(state.get_action_strength("move_right"), 1.0);
}

#[test]
fn released_action_has_strength_zero() {
    let mut state = InputState::new();
    state.set_input_map(make_directional_map());

    state.process_event(key_press(Key::Right));
    state.process_event(key_release(Key::Right));

    assert_eq!(state.get_action_strength("move_right"), 0.0);
}

#[test]
fn gamepad_axis_action_has_analog_strength() {
    let mut state = InputState::new();
    let mut map = InputMap::new();
    map.add_action("throttle", 0.1);
    map.action_add_event("throttle", ActionBinding::GamepadAxisBinding(GamepadAxis::RightTriggerAnalog));
    state.set_input_map(map);

    state.process_event(gamepad_axis_event(GamepadAxis::RightTriggerAnalog, 0.6, 0));

    assert!(state.is_action_pressed("throttle"));
    let strength = state.get_action_strength("throttle");
    assert!(
        (strength - 0.6).abs() < 0.01,
        "analog action strength should be 0.6, got {strength}"
    );
}

#[test]
fn gamepad_axis_below_deadzone_not_pressed() {
    let mut state = InputState::new();
    let mut map = InputMap::new();
    map.add_action("throttle", 0.3); // deadzone = 0.3
    map.action_add_event("throttle", ActionBinding::GamepadAxisBinding(GamepadAxis::RightTriggerAnalog));
    state.set_input_map(map);

    state.process_event(gamepad_axis_event(GamepadAxis::RightTriggerAnalog, 0.2, 0));

    assert!(!state.is_action_pressed("throttle"), "below deadzone should not be pressed");
    assert_eq!(state.get_action_strength("throttle"), 0.0);
}

#[test]
fn action_strength_in_snapshot_matches_state() {
    let mut state = InputState::new();
    state.set_input_map(make_directional_map());

    state.process_event(key_press(Key::Left));

    let snap = state.snapshot();
    assert_eq!(snap.get_action_strength("move_left"), 1.0);
    assert_eq!(snap.get_action_strength("move_right"), 0.0);
}

// ===========================================================================
// 5. get_axis / get_vector
// ===========================================================================

#[test]
fn get_axis_returns_positive_for_positive_action() {
    let mut state = InputState::new();
    state.set_input_map(make_directional_map());

    state.process_event(key_press(Key::Right));

    assert_eq!(state.get_axis("move_left", "move_right"), 1.0);
}

#[test]
fn get_axis_returns_negative_for_negative_action() {
    let mut state = InputState::new();
    state.set_input_map(make_directional_map());

    state.process_event(key_press(Key::Left));

    assert_eq!(state.get_axis("move_left", "move_right"), -1.0);
}

#[test]
fn get_axis_cancels_when_both_pressed() {
    let mut state = InputState::new();
    state.set_input_map(make_directional_map());

    state.process_event(key_press(Key::Left));
    state.process_event(key_press(Key::Right));

    assert_eq!(state.get_axis("move_left", "move_right"), 0.0);
}

#[test]
fn get_vector_returns_normalized_diagonal() {
    let mut state = InputState::new();
    state.set_input_map(make_directional_map());

    state.process_event(key_press(Key::Right));
    state.process_event(key_press(Key::Down));

    let v = state.get_vector("move_left", "move_right", "move_up", "move_down");
    // Should be normalized: (1, 1) → (0.707, 0.707)
    let len = (v.x * v.x + v.y * v.y).sqrt();
    assert!(
        (len - 1.0).abs() < 0.01,
        "diagonal get_vector should be normalized, got length {len}"
    );
    assert!(v.x > 0.5 && v.y > 0.5, "should point down-right: {v:?}");
}

#[test]
fn get_vector_cardinal_not_normalized() {
    let mut state = InputState::new();
    state.set_input_map(make_directional_map());

    state.process_event(key_press(Key::Right));

    let v = state.get_vector("move_left", "move_right", "move_up", "move_down");
    // Cardinal direction: (1, 0) — length is exactly 1, no normalization needed
    assert!((v.x - 1.0).abs() < 0.01);
    assert!(v.y.abs() < 0.01);
}

#[test]
fn snapshot_get_axis_and_get_vector_match_state() {
    let mut state = InputState::new();
    state.set_input_map(make_directional_map());

    state.process_event(key_press(Key::Left));
    state.process_event(key_press(Key::Up));

    let snap = state.snapshot();

    assert_eq!(
        snap.get_axis("move_left", "move_right"),
        state.get_axis("move_left", "move_right")
    );

    let sv = snap.get_vector("move_left", "move_right", "move_up", "move_down");
    let iv = state.get_vector("move_left", "move_right", "move_up", "move_down");
    assert!((sv.x - iv.x).abs() < 0.001 && (sv.y - iv.y).abs() < 0.001);
}

// ===========================================================================
// 6. Key release → action just_released semantics
// ===========================================================================

#[test]
fn action_just_released_on_key_release() {
    let mut state = InputState::new();
    state.set_input_map(make_directional_map());

    state.process_event(key_press(Key::Space));
    assert!(state.is_action_pressed("shoot"));
    assert!(state.is_action_just_pressed("shoot"));

    state.flush_frame();

    state.process_event(key_release(Key::Space));
    assert!(!state.is_action_pressed("shoot"));
    assert!(state.is_action_just_released("shoot"));

    let snap = state.snapshot();
    assert!(!snap.is_action_pressed("shoot"));
    assert!(snap.is_action_just_released("shoot"));
}

#[test]
fn just_released_cleared_after_flush() {
    let mut state = InputState::new();
    state.set_input_map(make_directional_map());

    state.process_event(key_press(Key::Space));
    state.flush_frame();
    state.process_event(key_release(Key::Space));

    assert!(state.is_action_just_released("shoot"));

    state.flush_frame();

    assert!(!state.is_action_just_released("shoot"));
}

// ===========================================================================
// 7. Multiple simultaneous keys/actions
// ===========================================================================

#[test]
fn multiple_keys_tracked_simultaneously() {
    let mut state = InputState::new();

    state.process_event(key_press(Key::A));
    state.process_event(key_press(Key::B));
    state.process_event(key_press(Key::C));

    assert!(state.is_key_pressed(Key::A));
    assert!(state.is_key_pressed(Key::B));
    assert!(state.is_key_pressed(Key::C));
    assert!(!state.is_key_pressed(Key::D));

    // Release B only
    state.process_event(key_release(Key::B));
    assert!(state.is_key_pressed(Key::A));
    assert!(!state.is_key_pressed(Key::B));
    assert!(state.is_key_pressed(Key::C));
}

#[test]
fn multiple_actions_from_different_keys() {
    let mut state = InputState::new();
    state.set_input_map(make_directional_map());

    state.process_event(key_press(Key::Left));
    state.process_event(key_press(Key::Space));

    assert!(state.is_action_pressed("move_left"));
    assert!(state.is_action_pressed("shoot"));
    assert!(!state.is_action_pressed("move_right"));
}

#[test]
fn action_event_directly_sets_action_state() {
    let mut state = InputState::new();
    // No input map needed — Action events are direct

    state.process_event(InputEvent::Action {
        action: "custom_action".to_string(),
        pressed: true,
        strength: 0.75,
    });

    assert!(state.is_action_pressed("custom_action"));
    assert!(state.is_action_just_pressed("custom_action"));
    assert_eq!(state.get_action_strength("custom_action"), 0.75);
}

// ===========================================================================
// 8. InputMap hot-swap mid-session
// ===========================================================================

#[test]
fn input_map_swap_changes_action_routing() {
    let mut state = InputState::new();

    // Map 1: Space → "shoot"
    let mut map1 = InputMap::new();
    map1.add_action("shoot", 0.0);
    map1.action_add_event("shoot", ActionBinding::KeyBinding(Key::Space));
    state.set_input_map(map1);

    state.process_event(key_press(Key::Space));
    assert!(state.is_action_pressed("shoot"));

    state.process_event(key_release(Key::Space));
    state.flush_frame();

    // Swap to Map 2: Space → "jump"
    let mut map2 = InputMap::new();
    map2.add_action("jump", 0.0);
    map2.action_add_event("jump", ActionBinding::KeyBinding(Key::Space));
    state.set_input_map(map2);

    state.process_event(key_press(Key::Space));
    assert!(state.is_action_pressed("jump"));
    assert!(!state.is_action_pressed("shoot"), "old action should not be triggered by new map");
}

// ===========================================================================
// 9. Empty/missing input map edge cases
// ===========================================================================

#[test]
fn no_input_map_still_tracks_keys_and_mouse() {
    let mut state = InputState::new();
    // No input map set

    state.process_event(key_press(Key::A));
    assert!(state.is_key_pressed(Key::A));

    state.process_event(mouse_press(MouseButton::Left, Vector2::new(50.0, 50.0)));
    assert!(state.is_mouse_button_pressed(MouseButton::Left));

    // Actions should not resolve without a map
    assert!(!state.is_action_pressed("anything"));
}

#[test]
fn action_query_for_unregistered_action_returns_false() {
    let mut state = InputState::new();
    state.set_input_map(make_directional_map());

    state.process_event(key_press(Key::Right));

    // Query an action that doesn't exist in the map
    assert!(!state.is_action_pressed("nonexistent_action"));
    assert_eq!(state.get_action_strength("nonexistent_action"), 0.0);

    let snap = state.snapshot();
    assert!(!snap.is_action_pressed("nonexistent_action"));
}

// ===========================================================================
// 10. flush_frame lifecycle
// ===========================================================================

#[test]
fn flush_frame_clears_just_pressed_and_just_released() {
    let mut state = InputState::new();

    state.process_event(key_press(Key::A));
    assert!(state.is_key_just_pressed(Key::A));

    state.flush_frame();
    assert!(!state.is_key_just_pressed(Key::A));
    assert!(state.is_key_pressed(Key::A)); // still held

    state.process_event(key_release(Key::A));
    assert!(state.is_key_just_released(Key::A));

    state.flush_frame();
    assert!(!state.is_key_just_released(Key::A));
    assert!(!state.is_key_pressed(Key::A)); // released
}

#[test]
fn flush_frame_clears_mouse_just_states() {
    let mut state = InputState::new();

    state.process_event(mouse_press(MouseButton::Left, Vector2::ZERO));
    assert!(state.is_mouse_button_just_pressed(MouseButton::Left));

    state.flush_frame();
    assert!(!state.is_mouse_button_just_pressed(MouseButton::Left));
    assert!(state.is_mouse_button_pressed(MouseButton::Left));
}

#[test]
fn flush_frame_clears_action_just_states() {
    let mut state = InputState::new();
    state.set_input_map(make_directional_map());

    state.process_event(key_press(Key::Space));
    assert!(state.is_action_just_pressed("shoot"));

    state.flush_frame();
    assert!(!state.is_action_just_pressed("shoot"));
    assert!(state.is_action_pressed("shoot"));
}

#[test]
fn repeated_press_without_release_does_not_retrigger_just_pressed() {
    let mut state = InputState::new();

    state.process_event(key_press(Key::A));
    assert!(state.is_key_just_pressed(Key::A));

    state.flush_frame();

    // Press A again without releasing — should NOT retrigger just_pressed
    state.process_event(key_press(Key::A));
    assert!(!state.is_key_just_pressed(Key::A));
    assert!(state.is_key_pressed(Key::A));
}

// ===========================================================================
// 11. Snapshot bridge methods (for script-facing conversion)
// ===========================================================================

#[test]
fn snapshot_pressed_key_names_returns_string_names() {
    let mut state = InputState::new();

    state.process_event(key_press(Key::A));
    state.process_event(key_press(Key::Space));

    let snap = state.snapshot();
    let names = snap.pressed_key_names();

    assert!(names.contains(&"A".to_string()) || names.contains(&"a".to_string()));
    assert!(names.contains(&" ".to_string()), "Space key name should be \" \" (literal space)");
    assert_eq!(names.len(), 2);
}

#[test]
fn snapshot_just_pressed_key_names_only_includes_new_keys() {
    let mut state = InputState::new();

    state.process_event(key_press(Key::A));
    state.flush_frame();
    state.process_event(key_press(Key::B));

    let snap = state.snapshot();
    let just_names = snap.just_pressed_key_names();

    // Only B was just pressed this frame
    assert_eq!(just_names.len(), 1);
}

#[test]
fn snapshot_action_pressed_key_map_includes_pressed_actions() {
    let mut state = InputState::new();
    state.set_input_map(make_directional_map());

    state.process_event(key_press(Key::Right));
    state.process_event(key_press(Key::Space));

    let snap = state.snapshot();
    let map = snap.action_pressed_key_map();

    assert!(map.contains_key("move_right"));
    assert!(map.contains_key("shoot"));
    assert!(!map.contains_key("move_left"));
}

// ===========================================================================
// 12. MainLoop end-to-end routing (push_event → step → SceneTree snapshot)
// ===========================================================================

const DT: f64 = 1.0 / 60.0;

#[test]
fn mainloop_push_event_routes_to_input_state() {
    let tree = SceneTree::new();
    let mut ml = MainLoop::new(tree);
    ml.set_input_map(make_directional_map());

    ml.push_event(key_press(Key::Right));

    assert!(ml.input_state().is_action_pressed("move_right"));
    assert!(ml.input_state().is_action_just_pressed("move_right"));
    assert!(ml.input_state().is_key_pressed(Key::Right));
}

#[test]
fn mainloop_step_flushes_just_pressed() {
    let tree = SceneTree::new();
    let mut ml = MainLoop::new(tree);
    ml.set_input_map(make_directional_map());

    ml.push_event(key_press(Key::Left));
    assert!(ml.input_state().is_action_just_pressed("move_left"));

    ml.step(DT);

    // just_pressed flushed, but action still held
    assert!(!ml.input_state().is_action_just_pressed("move_left"));
    assert!(ml.input_state().is_action_pressed("move_left"));
}

#[test]
fn mainloop_step_clears_script_snapshot_between_frames() {
    let tree = SceneTree::new();
    let mut ml = MainLoop::new(tree);
    ml.set_input_map(make_directional_map());

    ml.push_event(key_press(Key::Space));
    ml.step(DT);

    // After step, the SceneTree's script snapshot should be cleared
    assert!(!ml.tree().has_input_snapshot());
}

#[test]
fn mainloop_release_clears_action_after_step() {
    let tree = SceneTree::new();
    let mut ml = MainLoop::new(tree);
    ml.set_input_map(make_directional_map());

    ml.push_event(key_press(Key::Up));
    ml.step(DT);

    ml.push_event(key_release(Key::Up));
    assert!(!ml.input_state().is_action_pressed("move_up"));
    assert!(ml.input_state().is_action_just_released("move_up"));

    ml.step(DT);
    assert!(!ml.input_state().is_action_just_released("move_up"));
}

#[test]
fn mainloop_multi_frame_action_lifecycle() {
    let tree = SceneTree::new();
    let mut ml = MainLoop::new(tree);
    ml.set_input_map(make_directional_map());

    // Frame 1: press Right
    ml.push_event(key_press(Key::Right));
    assert!(ml.input_state().is_action_just_pressed("move_right"));
    ml.step(DT);

    // Frame 2: still held, no just_pressed
    assert!(ml.input_state().is_action_pressed("move_right"));
    assert!(!ml.input_state().is_action_just_pressed("move_right"));
    ml.step(DT);

    // Frame 3: release
    ml.push_event(key_release(Key::Right));
    assert!(!ml.input_state().is_action_pressed("move_right"));
    assert!(ml.input_state().is_action_just_released("move_right"));
    ml.step(DT);

    // Frame 4: all transient state cleared
    assert!(!ml.input_state().is_action_pressed("move_right"));
    assert!(!ml.input_state().is_action_just_released("move_right"));
}

#[test]
fn mainloop_mouse_routing_through_push_event() {
    let tree = SceneTree::new();
    let mut ml = MainLoop::new(tree);

    ml.push_event(mouse_press(MouseButton::Left, Vector2::new(100.0, 200.0)));

    assert!(ml.input_state().is_mouse_button_pressed(MouseButton::Left));
    assert_eq!(ml.input_state().get_mouse_position(), Vector2::new(100.0, 200.0));

    ml.step(DT);

    // still held after step
    assert!(ml.input_state().is_mouse_button_pressed(MouseButton::Left));
    // just_pressed flushed
    assert!(!ml.input_state().is_mouse_button_just_pressed(MouseButton::Left));
}

#[test]
fn mainloop_gamepad_routing_through_push_event() {
    let tree = SceneTree::new();
    let mut ml = MainLoop::new(tree);

    ml.push_event(gamepad_button_press(GamepadButton::FaceA, 0));
    assert!(ml.input_state().is_gamepad_button_pressed(0, GamepadButton::FaceA));

    ml.push_event(gamepad_axis_event(GamepadAxis::LeftStickX, 0.8, 0));
    assert!((ml.input_state().get_gamepad_axis_value(0, GamepadAxis::LeftStickX) - 0.8).abs() < 0.001);

    ml.step(DT);

    // Button still held, axis still set
    assert!(ml.input_state().is_gamepad_button_pressed(0, GamepadButton::FaceA));
    assert!((ml.input_state().get_gamepad_axis_value(0, GamepadAxis::LeftStickX) - 0.8).abs() < 0.001);
}

#[test]
fn mainloop_input_map_swap_mid_session() {
    let tree = SceneTree::new();
    let mut ml = MainLoop::new(tree);

    // Map 1: Space → shoot
    let mut map1 = InputMap::new();
    map1.add_action("shoot", 0.0);
    map1.action_add_event("shoot", ActionBinding::KeyBinding(Key::Space));
    ml.set_input_map(map1);

    ml.push_event(key_press(Key::Space));
    assert!(ml.input_state().is_action_pressed("shoot"));

    ml.push_event(key_release(Key::Space));
    ml.step(DT);

    // Swap to Map 2: Space → jump
    let mut map2 = InputMap::new();
    map2.add_action("jump", 0.0);
    map2.action_add_event("jump", ActionBinding::KeyBinding(Key::Space));
    ml.set_input_map(map2);

    ml.push_event(key_press(Key::Space));
    assert!(ml.input_state().is_action_pressed("jump"));
    assert!(!ml.input_state().is_action_pressed("shoot"));
}

#[test]
fn mainloop_no_input_map_still_tracks_raw_keys() {
    let tree = SceneTree::new();
    let mut ml = MainLoop::new(tree);
    // No input map set

    ml.push_event(key_press(Key::A));
    assert!(ml.input_state().is_key_pressed(Key::A));
    assert!(!ml.input_state().is_action_pressed("anything"));

    ml.step(DT);
    assert!(ml.input_state().is_key_pressed(Key::A));
}
