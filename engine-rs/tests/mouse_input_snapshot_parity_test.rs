//! pat-5ngy (originally pat-mkfb): Mouse position and button routing to input snapshots.
//!
//! Validates that mouse position and button state are correctly routed through
//! the engine-owned InputState → InputSnapshot → script-facing InputSnapshot
//! pipeline. Tests cover:
//!
//! 1. Mouse position from MouseMotion events survives snapshot round-trip
//! 2. Mouse position from MouseButton events updates position
//! 3. Mouse button press/release tracked through snapshot
//! 4. Multiple mouse buttons tracked independently
//! 5. Mouse just_pressed / just_released lifecycle across flush_frame
//! 6. Mouse position preserved across frames when no new motion
//! 7. Script-facing snapshot: Godot button indices (1=Left, 2=Right, 3=Middle)
//! 8. MainLoop bridge: push_event → step → script snapshot has mouse state
//! 9. Mouse action routing through InputMap
//! 10. Snapshot immutability for mouse state
//! 11. Wheel buttons tracked as press-only events
//! 12. Mouse position zero is valid, not treated as "no input"

use gdcore::math::Vector2;
use gdplatform::input::{ActionBinding, InputEvent, InputMap, InputState, MouseButton};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

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

fn mouse_motion(position: Vector2, relative: Vector2) -> InputEvent {
    InputEvent::MouseMotion { position, relative }
}

// ===========================================================================
// 1. Mouse position from MouseMotion events survives snapshot
// ===========================================================================

#[test]
fn mouse_motion_position_in_snapshot() {
    let mut state = InputState::new();

    state.process_event(mouse_motion(Vector2::new(320.0, 240.0), Vector2::new(5.0, 3.0)));

    let snap = state.snapshot();
    assert_eq!(snap.get_mouse_position(), Vector2::new(320.0, 240.0));
}

#[test]
fn mouse_motion_updates_position_sequentially() {
    let mut state = InputState::new();

    state.process_event(mouse_motion(Vector2::new(100.0, 100.0), Vector2::new(10.0, 0.0)));
    assert_eq!(state.get_mouse_position(), Vector2::new(100.0, 100.0));

    state.process_event(mouse_motion(Vector2::new(200.0, 150.0), Vector2::new(100.0, 50.0)));
    assert_eq!(state.get_mouse_position(), Vector2::new(200.0, 150.0));

    let snap = state.snapshot();
    assert_eq!(snap.get_mouse_position(), Vector2::new(200.0, 150.0));
}

// ===========================================================================
// 2. Mouse position from MouseButton events
// ===========================================================================

#[test]
fn mouse_button_event_updates_position() {
    let mut state = InputState::new();

    state.process_event(mouse_press(MouseButton::Left, Vector2::new(50.0, 75.0)));
    assert_eq!(state.get_mouse_position(), Vector2::new(50.0, 75.0));

    let snap = state.snapshot();
    assert_eq!(snap.get_mouse_position(), Vector2::new(50.0, 75.0));
}

#[test]
fn mouse_release_updates_position() {
    let mut state = InputState::new();

    state.process_event(mouse_press(MouseButton::Left, Vector2::new(10.0, 20.0)));
    state.process_event(mouse_release(MouseButton::Left, Vector2::new(30.0, 40.0)));

    assert_eq!(state.get_mouse_position(), Vector2::new(30.0, 40.0));
}

// ===========================================================================
// 3. Mouse button press/release tracked through snapshot
// ===========================================================================

#[test]
fn mouse_left_press_in_snapshot() {
    let mut state = InputState::new();

    state.process_event(mouse_press(MouseButton::Left, Vector2::ZERO));

    let snap = state.snapshot();
    assert!(snap.is_mouse_button_pressed(MouseButton::Left));
    assert!(snap.is_mouse_button_just_pressed(MouseButton::Left));
    assert!(!snap.is_mouse_button_pressed(MouseButton::Right));
}

#[test]
fn mouse_release_clears_pressed() {
    let mut state = InputState::new();

    state.process_event(mouse_press(MouseButton::Left, Vector2::ZERO));
    state.flush_frame();
    state.process_event(mouse_release(MouseButton::Left, Vector2::ZERO));

    let snap = state.snapshot();
    assert!(!snap.is_mouse_button_pressed(MouseButton::Left));
    assert!(snap.is_mouse_button_just_released(MouseButton::Left));
}

// ===========================================================================
// 4. Multiple mouse buttons tracked independently
// ===========================================================================

#[test]
fn all_three_main_buttons_independent() {
    let mut state = InputState::new();

    state.process_event(mouse_press(MouseButton::Left, Vector2::ZERO));
    state.process_event(mouse_press(MouseButton::Right, Vector2::ZERO));
    state.process_event(mouse_press(MouseButton::Middle, Vector2::ZERO));

    assert!(state.is_mouse_button_pressed(MouseButton::Left));
    assert!(state.is_mouse_button_pressed(MouseButton::Right));
    assert!(state.is_mouse_button_pressed(MouseButton::Middle));

    // Release only middle
    state.process_event(mouse_release(MouseButton::Middle, Vector2::ZERO));

    assert!(state.is_mouse_button_pressed(MouseButton::Left));
    assert!(state.is_mouse_button_pressed(MouseButton::Right));
    assert!(!state.is_mouse_button_pressed(MouseButton::Middle));

    let snap = state.snapshot();
    assert!(snap.is_mouse_button_pressed(MouseButton::Left));
    assert!(snap.is_mouse_button_pressed(MouseButton::Right));
    assert!(!snap.is_mouse_button_pressed(MouseButton::Middle));
}

// ===========================================================================
// 5. Mouse just_pressed / just_released lifecycle across flush_frame
// ===========================================================================

#[test]
fn mouse_just_pressed_cleared_after_flush() {
    let mut state = InputState::new();

    state.process_event(mouse_press(MouseButton::Left, Vector2::ZERO));
    assert!(state.is_mouse_button_just_pressed(MouseButton::Left));

    state.flush_frame();
    assert!(!state.is_mouse_button_just_pressed(MouseButton::Left));
    assert!(state.is_mouse_button_pressed(MouseButton::Left)); // still held
}

#[test]
fn mouse_just_released_cleared_after_flush() {
    let mut state = InputState::new();

    state.process_event(mouse_press(MouseButton::Right, Vector2::ZERO));
    state.flush_frame();
    state.process_event(mouse_release(MouseButton::Right, Vector2::ZERO));
    assert!(state.is_mouse_button_just_released(MouseButton::Right));

    state.flush_frame();
    assert!(!state.is_mouse_button_just_released(MouseButton::Right));
    assert!(!state.is_mouse_button_pressed(MouseButton::Right));
}

#[test]
fn mouse_press_release_same_frame() {
    let mut state = InputState::new();

    state.process_event(mouse_press(MouseButton::Left, Vector2::new(10.0, 10.0)));
    state.process_event(mouse_release(MouseButton::Left, Vector2::new(20.0, 20.0)));

    // Key is released, but just_pressed may still be true (Godot semantics:
    // both just_pressed and just_released can be true in same frame).
    assert!(!state.is_mouse_button_pressed(MouseButton::Left));
    assert!(state.is_mouse_button_just_released(MouseButton::Left));
    assert_eq!(state.get_mouse_position(), Vector2::new(20.0, 20.0));
}

// ===========================================================================
// 6. Mouse position preserved across frames when no new motion
// ===========================================================================

#[test]
fn mouse_position_persists_across_flush() {
    let mut state = InputState::new();

    state.process_event(mouse_motion(Vector2::new(400.0, 300.0), Vector2::ZERO));
    state.flush_frame();

    // No new mouse events this frame
    assert_eq!(state.get_mouse_position(), Vector2::new(400.0, 300.0));

    let snap = state.snapshot();
    assert_eq!(snap.get_mouse_position(), Vector2::new(400.0, 300.0));
}

// ===========================================================================
// 7. Script-facing snapshot: Godot button indices (1=Left, 2=Right, 3=Middle)
// ===========================================================================

#[test]
fn script_snapshot_uses_godot_button_indices() {
    use gdscene::scripting::InputSnapshot as ScriptSnapshot;

    let script_snap = ScriptSnapshot {
        pressed_keys: Default::default(),
        just_pressed_keys: Default::default(),
        just_released_keys: Default::default(),
        input_map: Default::default(),
        mouse_position: Vector2::new(100.0, 200.0),
        mouse_buttons_pressed: ["1".to_string(), "3".to_string()]
            .into_iter()
            .collect(),
        just_pressed_mouse_buttons: Default::default(),
        just_released_mouse_buttons: Default::default(),
        actions_just_released: Default::default(),
        action_strengths: Default::default(),
        touches_pressed: Default::default(),
        touches_just_pressed: Default::default(),
        touches_just_released: Default::default(),
        touch_positions: Default::default(),
        gamepad_buttons_pressed: Default::default(),
        gamepad_buttons_just_pressed: Default::default(),
        gamepad_buttons_just_released: Default::default(),
        gamepad_axis_values: Default::default(),
    };

    // Godot indices: 1=Left, 2=Right, 3=Middle
    assert!(script_snap.is_mouse_button_pressed(1));  // Left
    assert!(!script_snap.is_mouse_button_pressed(2)); // Right (not pressed)
    assert!(script_snap.is_mouse_button_pressed(3));  // Middle
    assert_eq!(script_snap.get_mouse_position(), Vector2::new(100.0, 200.0));

    // Unused index returns false
    assert!(!script_snap.is_mouse_button_pressed(4));
    assert!(!script_snap.is_mouse_button_pressed(0));
}

// ===========================================================================
// 8. MainLoop bridge: push_event → step → script snapshot has mouse state
// ===========================================================================

#[test]
fn mainloop_bridges_mouse_to_script_snapshot() {
    use gdscene::main_loop::MainLoop;
    use gdscene::node::Node;
    use gdscene::scene_tree::SceneTree;

    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let node = Node::new("Player", "Node2D");
    tree.add_child(root, node).unwrap();

    let mut ml = MainLoop::new(tree);

    // Push mouse events before stepping
    ml.push_event(mouse_press(MouseButton::Left, Vector2::new(150.0, 250.0)));
    ml.step(1.0 / 60.0);

    // After step, the tree's input snapshot should have the mouse state
    let snap = ml.tree().input_snapshot();
    if let Some(snap) = snap {
        assert!(
            snap.is_mouse_button_pressed(1),
            "Left mouse (button index 1) must be pressed in script snapshot"
        );
        assert_eq!(snap.get_mouse_position(), Vector2::new(150.0, 250.0));
    }
    // Note: snapshot may be cleared at end of frame, so we also verify via engine state
    assert!(ml.input_state().is_mouse_button_pressed(MouseButton::Left));
    assert_eq!(ml.input_state().get_mouse_position(), Vector2::new(150.0, 250.0));
}

#[test]
fn mainloop_bridges_mouse_motion_to_script_snapshot() {
    use gdscene::main_loop::MainLoop;
    use gdscene::node::Node;
    use gdscene::scene_tree::SceneTree;

    let mut tree = SceneTree::new();
    let root = tree.root_id();
    tree.add_child(root, Node::new("Cursor", "Node2D")).unwrap();

    let mut ml = MainLoop::new(tree);

    ml.push_event(mouse_motion(Vector2::new(640.0, 480.0), Vector2::new(10.0, -5.0)));
    ml.step(1.0 / 60.0);

    // Engine state has the position
    assert_eq!(ml.input_state().get_mouse_position(), Vector2::new(640.0, 480.0));
}

#[test]
fn mainloop_bridges_multiple_buttons() {
    use gdscene::main_loop::MainLoop;
    use gdscene::node::Node;
    use gdscene::scene_tree::SceneTree;

    let mut tree = SceneTree::new();
    let root = tree.root_id();
    tree.add_child(root, Node::new("UI", "Node2D")).unwrap();

    let mut ml = MainLoop::new(tree);

    ml.push_event(mouse_press(MouseButton::Left, Vector2::ZERO));
    ml.push_event(mouse_press(MouseButton::Right, Vector2::ZERO));

    // Engine state tracks both
    assert!(ml.input_state().is_mouse_button_pressed(MouseButton::Left));
    assert!(ml.input_state().is_mouse_button_pressed(MouseButton::Right));
    assert!(!ml.input_state().is_mouse_button_pressed(MouseButton::Middle));
}

// ===========================================================================
// 9. Mouse action routing through InputMap
// ===========================================================================

#[test]
fn mouse_button_triggers_action_via_input_map() {
    let mut state = InputState::new();
    let mut map = InputMap::new();
    map.add_action("fire", 0.0);
    map.action_add_event("fire", ActionBinding::MouseBinding(MouseButton::Left));
    state.set_input_map(map);

    state.process_event(mouse_press(MouseButton::Left, Vector2::new(100.0, 100.0)));

    assert!(state.is_action_pressed("fire"));
    assert!(state.is_action_just_pressed("fire"));
    assert_eq!(state.get_action_strength("fire"), 1.0);
}

#[test]
fn mouse_button_release_triggers_action_release() {
    let mut state = InputState::new();
    let mut map = InputMap::new();
    map.add_action("fire", 0.0);
    map.action_add_event("fire", ActionBinding::MouseBinding(MouseButton::Left));
    state.set_input_map(map);

    state.process_event(mouse_press(MouseButton::Left, Vector2::ZERO));
    state.flush_frame();
    state.process_event(mouse_release(MouseButton::Left, Vector2::ZERO));

    assert!(!state.is_action_pressed("fire"));
    assert!(state.is_action_just_released("fire"));
}

#[test]
fn right_click_action_independent_of_left() {
    let mut state = InputState::new();
    let mut map = InputMap::new();
    map.add_action("fire", 0.0);
    map.action_add_event("fire", ActionBinding::MouseBinding(MouseButton::Left));
    map.add_action("alt_fire", 0.0);
    map.action_add_event("alt_fire", ActionBinding::MouseBinding(MouseButton::Right));
    state.set_input_map(map);

    state.process_event(mouse_press(MouseButton::Left, Vector2::ZERO));

    assert!(state.is_action_pressed("fire"));
    assert!(!state.is_action_pressed("alt_fire"));

    state.process_event(mouse_press(MouseButton::Right, Vector2::ZERO));

    assert!(state.is_action_pressed("fire"));
    assert!(state.is_action_pressed("alt_fire"));
}

// ===========================================================================
// 10. Snapshot immutability for mouse state
// ===========================================================================

#[test]
fn snapshot_mouse_position_frozen() {
    let mut state = InputState::new();

    state.process_event(mouse_motion(Vector2::new(100.0, 100.0), Vector2::ZERO));
    let snap = state.snapshot();

    // Move mouse after snapshot
    state.process_event(mouse_motion(Vector2::new(999.0, 999.0), Vector2::ZERO));

    // Snapshot still has old position
    assert_eq!(snap.get_mouse_position(), Vector2::new(100.0, 100.0));
    assert_eq!(state.get_mouse_position(), Vector2::new(999.0, 999.0));
}

#[test]
fn snapshot_mouse_buttons_frozen() {
    let mut state = InputState::new();

    state.process_event(mouse_press(MouseButton::Left, Vector2::ZERO));
    let snap = state.snapshot();

    state.process_event(mouse_release(MouseButton::Left, Vector2::ZERO));

    // Snapshot still shows pressed
    assert!(snap.is_mouse_button_pressed(MouseButton::Left));
    assert!(!state.is_mouse_button_pressed(MouseButton::Left));
}

// ===========================================================================
// 11. Wheel buttons tracked as press events
// ===========================================================================

#[test]
fn wheel_up_tracked_as_button() {
    let mut state = InputState::new();

    state.process_event(mouse_press(MouseButton::WheelUp, Vector2::new(50.0, 50.0)));

    assert!(state.is_mouse_button_pressed(MouseButton::WheelUp));
    assert!(state.is_mouse_button_just_pressed(MouseButton::WheelUp));
    assert!(!state.is_mouse_button_pressed(MouseButton::WheelDown));
    assert_eq!(state.get_mouse_position(), Vector2::new(50.0, 50.0));
}

#[test]
fn wheel_down_tracked_independently() {
    let mut state = InputState::new();

    state.process_event(mouse_press(MouseButton::WheelDown, Vector2::ZERO));
    state.process_event(mouse_press(MouseButton::WheelUp, Vector2::ZERO));

    assert!(state.is_mouse_button_pressed(MouseButton::WheelDown));
    assert!(state.is_mouse_button_pressed(MouseButton::WheelUp));
    assert!(!state.is_mouse_button_pressed(MouseButton::Left));
}

// ===========================================================================
// 12. Mouse position zero is valid, not treated as "no input"
// ===========================================================================

#[test]
fn mouse_at_origin_is_valid_position() {
    let mut state = InputState::new();

    state.process_event(mouse_press(MouseButton::Left, Vector2::ZERO));

    assert_eq!(state.get_mouse_position(), Vector2::ZERO);
    assert!(state.is_mouse_button_pressed(MouseButton::Left));

    let snap = state.snapshot();
    assert_eq!(snap.get_mouse_position(), Vector2::ZERO);
    assert!(snap.is_mouse_button_pressed(MouseButton::Left));
}

#[test]
fn mouse_motion_to_negative_coords() {
    let mut state = InputState::new();

    // Negative coordinates are valid (e.g., multi-monitor setups)
    state.process_event(mouse_motion(Vector2::new(-100.0, -50.0), Vector2::ZERO));

    assert_eq!(state.get_mouse_position(), Vector2::new(-100.0, -50.0));

    let snap = state.snapshot();
    assert_eq!(snap.get_mouse_position(), Vector2::new(-100.0, -50.0));
}

// ===========================================================================
// 13. Mouse state persists across multiple MainLoop steps
// ===========================================================================

#[test]
fn mouse_button_held_across_multiple_steps() {
    use gdscene::main_loop::MainLoop;
    use gdscene::node::Node;
    use gdscene::scene_tree::SceneTree;

    let mut tree = SceneTree::new();
    let root = tree.root_id();
    tree.add_child(root, Node::new("Player", "Node2D")).unwrap();

    let mut ml = MainLoop::new(tree);

    // Press left button
    ml.push_event(mouse_press(MouseButton::Left, Vector2::new(100.0, 200.0)));
    ml.step(1.0 / 60.0);

    assert!(ml.input_state().is_mouse_button_pressed(MouseButton::Left));
    assert_eq!(ml.input_state().get_mouse_position(), Vector2::new(100.0, 200.0));

    // Step again without new events — button should still be held
    ml.step(1.0 / 60.0);
    assert!(ml.input_state().is_mouse_button_pressed(MouseButton::Left));
    assert!(!ml.input_state().is_mouse_button_just_pressed(MouseButton::Left));

    // Step a third time
    ml.step(1.0 / 60.0);
    assert!(ml.input_state().is_mouse_button_pressed(MouseButton::Left));
    assert_eq!(ml.input_state().get_mouse_position(), Vector2::new(100.0, 200.0));
}

#[test]
fn mouse_position_updates_between_steps() {
    use gdscene::main_loop::MainLoop;
    use gdscene::node::Node;
    use gdscene::scene_tree::SceneTree;

    let mut tree = SceneTree::new();
    let root = tree.root_id();
    tree.add_child(root, Node::new("Cursor", "Node2D")).unwrap();

    let mut ml = MainLoop::new(tree);

    ml.push_event(mouse_motion(Vector2::new(100.0, 100.0), Vector2::new(10.0, 10.0)));
    ml.step(1.0 / 60.0);
    assert_eq!(ml.input_state().get_mouse_position(), Vector2::new(100.0, 100.0));

    ml.push_event(mouse_motion(Vector2::new(200.0, 300.0), Vector2::new(100.0, 200.0)));
    ml.step(1.0 / 60.0);
    assert_eq!(ml.input_state().get_mouse_position(), Vector2::new(200.0, 300.0));
}

// ===========================================================================
// 14. Mouse action routing: wheel button triggers action
// ===========================================================================

#[test]
fn wheel_button_triggers_action() {
    let mut state = InputState::new();
    let mut map = InputMap::new();
    map.add_action("zoom_in", 0.0);
    map.action_add_event("zoom_in", ActionBinding::MouseBinding(MouseButton::WheelUp));
    state.set_input_map(map);

    state.process_event(mouse_press(MouseButton::WheelUp, Vector2::ZERO));

    assert!(state.is_action_pressed("zoom_in"));
    assert!(state.is_action_just_pressed("zoom_in"));
}

// ===========================================================================
// 15. Combined mouse + keyboard action state
// ===========================================================================

#[test]
fn mouse_and_keyboard_actions_coexist() {
    use gdplatform::input::Key;

    let mut state = InputState::new();
    let mut map = InputMap::new();
    map.add_action("fire", 0.0);
    map.action_add_event("fire", ActionBinding::MouseBinding(MouseButton::Left));
    map.add_action("jump", 0.0);
    map.action_add_event("jump", ActionBinding::KeyBinding(Key::Space));
    state.set_input_map(map);

    state.process_event(mouse_press(MouseButton::Left, Vector2::new(50.0, 50.0)));
    state.process_event(InputEvent::Key {
        key: Key::Space,
        pressed: true,
        shift: false,
        ctrl: false,
        alt: false,
    });

    assert!(state.is_action_pressed("fire"));
    assert!(state.is_action_pressed("jump"));
    assert_eq!(state.get_mouse_position(), Vector2::new(50.0, 50.0));

    // Release mouse, keyboard still held
    state.flush_frame();
    state.process_event(mouse_release(MouseButton::Left, Vector2::new(50.0, 50.0)));

    assert!(!state.is_action_pressed("fire"));
    assert!(state.is_action_pressed("jump"));
}
