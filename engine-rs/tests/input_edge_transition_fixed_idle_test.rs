//! pat-sai5: Input snapshot edge transitions across fixed and idle frames.
//!
//! Validates that press/release edge detection (just_pressed / just_released)
//! behaves correctly across both physics (fixed-timestep) and process
//! (idle/variable-timestep) phases within the MainLoop:
//!
//! - Edge state visible in both physics and idle phases of the same step
//! - Edge state does not leak to the next step (flush_frame clears it)
//! - Multiple physics ticks within one step all see the same edge state
//! - Hold across steps: is_pressed persists, just_pressed is one-shot
//! - Release in a subsequent step: just_released fires in that step only
//! - Press and release in the same step: just_released set, not pressed
//! - Rapid toggle: press → flush → release → flush → re-press re-triggers
//! - Snapshot taken before physics phase, used unchanged through idle phase

use gdplatform::input::{ActionBinding, InputEvent, InputMap, InputState, Key, MouseButton};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

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

fn mouse_press(button: MouseButton) -> InputEvent {
    InputEvent::MouseButton {
        button,
        pressed: true,
        position: gdcore::math::Vector2::ZERO,
    }
}

fn mouse_release(button: MouseButton) -> InputEvent {
    InputEvent::MouseButton {
        button,
        pressed: false,
        position: gdcore::math::Vector2::ZERO,
    }
}

fn jump_action_map() -> InputMap {
    let mut map = InputMap::new();
    map.add_action("jump", 0.0);
    map.action_add_event("jump", ActionBinding::KeyBinding(Key::Space));
    map.add_action("move_left", 0.0);
    map.action_add_event("move_left", ActionBinding::KeyBinding(Key::A));
    map.add_action("move_right", 0.0);
    map.action_add_event("move_right", ActionBinding::KeyBinding(Key::D));
    map.add_action("shoot", 0.0);
    map.action_add_event("shoot", ActionBinding::KeyBinding(Key::F));
    map
}

fn state_with_actions() -> InputState {
    let mut state = InputState::new();
    state.set_input_map(jump_action_map());
    state
}

/// Simulates one full frame: process events → snapshot → (physics reads) → (idle reads) → flush.
/// Returns the snapshot that both physics and idle phases would see.
fn simulate_frame(state: &mut InputState, events: &[InputEvent]) -> gdplatform::input::InputSnapshot {
    for event in events {
        state.process_event(event.clone());
    }
    let snap = state.snapshot();
    // Physics ticks and idle frame all read from this snapshot.
    // At end of frame, flush.
    state.flush_frame();
    snap
}

// ===========================================================================
// 1. Press edge visible in snapshot (both physics and idle see it)
// ===========================================================================

#[test]
fn press_edge_visible_in_snapshot() {
    let mut state = state_with_actions();

    // Frame 1: press Space (jump).
    let snap = simulate_frame(&mut state, &[key_press(Key::Space)]);

    assert!(snap.is_action_pressed("jump"), "jump should be pressed");
    assert!(
        snap.is_action_just_pressed("jump"),
        "jump should be just_pressed on the press frame"
    );
    assert!(
        !snap.is_action_just_released("jump"),
        "jump should NOT be just_released on the press frame"
    );
    assert!(snap.is_key_pressed(Key::Space));
    assert!(snap.is_key_just_pressed(Key::Space));
}

// ===========================================================================
// 2. Edge state cleared after flush (does not leak to next step)
// ===========================================================================

#[test]
fn edge_state_cleared_after_flush() {
    let mut state = state_with_actions();

    // Frame 1: press Space.
    let _snap1 = simulate_frame(&mut state, &[key_press(Key::Space)]);

    // Frame 2: no events — key is held, but edge state should be gone.
    let snap2 = simulate_frame(&mut state, &[]);

    assert!(
        snap2.is_action_pressed("jump"),
        "jump should remain pressed (held key)"
    );
    assert!(
        !snap2.is_action_just_pressed("jump"),
        "just_pressed must NOT carry to next frame"
    );
    assert!(
        !snap2.is_action_just_released("jump"),
        "just_released should not appear — key still held"
    );
    assert!(snap2.is_key_pressed(Key::Space));
    assert!(!snap2.is_key_just_pressed(Key::Space));
}

// ===========================================================================
// 3. Release edge in subsequent step
// ===========================================================================

#[test]
fn release_edge_in_subsequent_step() {
    let mut state = state_with_actions();

    // Frame 1: press.
    let _snap1 = simulate_frame(&mut state, &[key_press(Key::Space)]);

    // Frame 2: release.
    let snap2 = simulate_frame(&mut state, &[key_release(Key::Space)]);

    assert!(
        !snap2.is_action_pressed("jump"),
        "jump should no longer be pressed after release"
    );
    assert!(
        !snap2.is_action_just_pressed("jump"),
        "just_pressed should not fire on release frame"
    );
    assert!(
        snap2.is_action_just_released("jump"),
        "just_released should fire on the release frame"
    );
    assert!(!snap2.is_key_pressed(Key::Space));
    assert!(snap2.is_key_just_released(Key::Space));
}

#[test]
fn release_edge_cleared_frame_after_release() {
    let mut state = state_with_actions();

    // Frame 1: press.
    simulate_frame(&mut state, &[key_press(Key::Space)]);
    // Frame 2: release.
    simulate_frame(&mut state, &[key_release(Key::Space)]);
    // Frame 3: no events — release edge should be gone.
    let snap3 = simulate_frame(&mut state, &[]);

    assert!(!snap3.is_action_pressed("jump"));
    assert!(!snap3.is_action_just_pressed("jump"));
    assert!(
        !snap3.is_action_just_released("jump"),
        "just_released must NOT carry to frame after release"
    );
    assert!(!snap3.is_key_just_released(Key::Space));
}

// ===========================================================================
// 4. Multiple physics ticks see the same snapshot
// ===========================================================================

#[test]
fn multiple_snapshots_within_frame_are_identical() {
    let mut state = state_with_actions();

    // Process press event.
    state.process_event(key_press(Key::Space));

    // Take multiple snapshots (simulating multiple physics ticks reading state).
    let snap1 = state.snapshot();
    let snap2 = state.snapshot();
    let snap3 = state.snapshot();

    // All snapshots must agree.
    for (i, snap) in [&snap1, &snap2, &snap3].iter().enumerate() {
        assert!(
            snap.is_action_pressed("jump"),
            "snapshot {i}: jump should be pressed"
        );
        assert!(
            snap.is_action_just_pressed("jump"),
            "snapshot {i}: jump should be just_pressed"
        );
        assert!(
            !snap.is_action_just_released("jump"),
            "snapshot {i}: jump should not be just_released"
        );
    }

    state.flush_frame();
}

// ===========================================================================
// 5. Hold across multiple steps: is_pressed persists, just_pressed is one-shot
// ===========================================================================

#[test]
fn hold_across_steps_pressed_persists_just_pressed_one_shot() {
    let mut state = state_with_actions();

    // Frame 1: press.
    let snap1 = simulate_frame(&mut state, &[key_press(Key::Space)]);
    assert!(snap1.is_action_pressed("jump"));
    assert!(snap1.is_action_just_pressed("jump"));

    // Frames 2-5: held (no new events).
    for frame in 2..=5 {
        let snap = simulate_frame(&mut state, &[]);
        assert!(
            snap.is_action_pressed("jump"),
            "frame {frame}: jump should remain pressed"
        );
        assert!(
            !snap.is_action_just_pressed("jump"),
            "frame {frame}: just_pressed must be false on held frames"
        );
        assert!(
            !snap.is_action_just_released("jump"),
            "frame {frame}: just_released must be false while held"
        );
    }
}

// ===========================================================================
// 6. Press and release in the same frame
// ===========================================================================

#[test]
fn press_and_release_same_frame() {
    let mut state = state_with_actions();

    // Both press and release in one frame.
    let snap = simulate_frame(&mut state, &[key_press(Key::Space), key_release(Key::Space)]);

    // After press then release in same frame: action not pressed, just_released set.
    assert!(
        !snap.is_action_pressed("jump"),
        "should not be pressed after same-frame release"
    );
    assert!(
        snap.is_action_just_released("jump"),
        "just_released should be set from same-frame release"
    );
    // just_pressed was set then the action was released — key-level edge:
    assert!(
        snap.is_key_just_pressed(Key::Space),
        "key just_pressed should be set (press happened this frame)"
    );
    assert!(
        snap.is_key_just_released(Key::Space),
        "key just_released should also be set (release happened this frame)"
    );
}

// ===========================================================================
// 7. Rapid toggle: press → flush → release → flush → re-press
// ===========================================================================

#[test]
fn rapid_toggle_re_triggers_just_pressed() {
    let mut state = state_with_actions();

    // Frame 1: press.
    let snap1 = simulate_frame(&mut state, &[key_press(Key::Space)]);
    assert!(snap1.is_action_just_pressed("jump"));

    // Frame 2: release.
    let snap2 = simulate_frame(&mut state, &[key_release(Key::Space)]);
    assert!(snap2.is_action_just_released("jump"));
    assert!(!snap2.is_action_pressed("jump"));

    // Frame 3: re-press.
    let snap3 = simulate_frame(&mut state, &[key_press(Key::Space)]);
    assert!(
        snap3.is_action_just_pressed("jump"),
        "re-press must trigger just_pressed again"
    );
    assert!(snap3.is_action_pressed("jump"));
    assert!(!snap3.is_action_just_released("jump"));
}

// ===========================================================================
// 8. Snapshot isolation: snapshot before events differs from snapshot after
// ===========================================================================

#[test]
fn snapshot_before_events_has_no_edges() {
    let mut state = state_with_actions();
    let snap_before = state.snapshot();

    state.process_event(key_press(Key::Space));
    let snap_after = state.snapshot();

    assert!(!snap_before.is_action_pressed("jump"));
    assert!(!snap_before.is_action_just_pressed("jump"));

    assert!(snap_after.is_action_pressed("jump"));
    assert!(snap_after.is_action_just_pressed("jump"));

    state.flush_frame();
}

// ===========================================================================
// 9. Multiple actions: independent edge tracking
// ===========================================================================

#[test]
fn independent_action_edges() {
    let mut state = state_with_actions();

    // Frame 1: press jump only.
    let snap1 = simulate_frame(&mut state, &[key_press(Key::Space)]);
    assert!(snap1.is_action_just_pressed("jump"));
    assert!(!snap1.is_action_just_pressed("move_left"));

    // Frame 2: press move_left, jump still held.
    let snap2 = simulate_frame(&mut state, &[key_press(Key::A)]);
    assert!(!snap2.is_action_just_pressed("jump"), "jump held, not re-pressed");
    assert!(snap2.is_action_pressed("jump"), "jump still held");
    assert!(snap2.is_action_just_pressed("move_left"), "move_left freshly pressed");
    assert!(snap2.is_action_pressed("move_left"));

    // Frame 3: release jump, move_left still held.
    let snap3 = simulate_frame(&mut state, &[key_release(Key::Space)]);
    assert!(snap3.is_action_just_released("jump"));
    assert!(!snap3.is_action_pressed("jump"));
    assert!(snap3.is_action_pressed("move_left"), "move_left still held");
    assert!(!snap3.is_action_just_pressed("move_left"), "move_left not re-pressed");
    assert!(!snap3.is_action_just_released("move_left"), "move_left not released");
}

// ===========================================================================
// 10. Mouse button edge transitions mirror key behavior
// ===========================================================================

#[test]
fn mouse_button_edge_transitions() {
    let mut state = InputState::new();

    // Frame 1: press left mouse button.
    state.process_event(mouse_press(MouseButton::Left));
    let snap1 = state.snapshot();
    assert!(snap1.is_mouse_button_pressed(MouseButton::Left));
    assert!(snap1.is_mouse_button_just_pressed(MouseButton::Left));
    assert!(!snap1.is_mouse_button_just_released(MouseButton::Left));
    state.flush_frame();

    // Frame 2: held.
    let snap2 = state.snapshot();
    assert!(snap2.is_mouse_button_pressed(MouseButton::Left));
    assert!(!snap2.is_mouse_button_just_pressed(MouseButton::Left));
    state.flush_frame();

    // Frame 3: release.
    state.process_event(mouse_release(MouseButton::Left));
    let snap3 = state.snapshot();
    assert!(!snap3.is_mouse_button_pressed(MouseButton::Left));
    assert!(snap3.is_mouse_button_just_released(MouseButton::Left));
    assert!(!snap3.is_mouse_button_just_pressed(MouseButton::Left));
    state.flush_frame();

    // Frame 4: no events — release edge gone.
    let snap4 = state.snapshot();
    assert!(!snap4.is_mouse_button_pressed(MouseButton::Left));
    assert!(!snap4.is_mouse_button_just_released(MouseButton::Left));
}

// ===========================================================================
// 11. MainLoop integration: edge state spans physics + idle phases
// ===========================================================================

#[test]
fn mainloop_edge_spans_physics_and_idle() {
    use gdscene::main_loop::MainLoop;
    use gdscene::scene_tree::SceneTree;

    let tree = SceneTree::new();
    let mut ml = MainLoop::new(tree);
    ml.set_input_map(jump_action_map());

    // Inject press event before step.
    ml.push_event(key_press(Key::Space));

    // Take snapshot from InputState — this is what MainLoop::step() bridges.
    let snap = ml.input_state().snapshot();
    assert!(snap.is_action_pressed("jump"));
    assert!(snap.is_action_just_pressed("jump"));

    // Run one step (contains physics + idle phases).
    let output = ml.step(1.0 / 60.0);
    assert_eq!(output.frame_count, 1);

    // After step, flush has occurred — edge state cleared.
    let snap_after = ml.input_state().snapshot();
    assert!(
        snap_after.is_action_pressed("jump"),
        "key still held after step"
    );
    assert!(
        !snap_after.is_action_just_pressed("jump"),
        "just_pressed cleared by flush_frame after step"
    );
}

#[test]
fn mainloop_release_edge_after_step() {
    use gdscene::main_loop::MainLoop;
    use gdscene::scene_tree::SceneTree;

    let tree = SceneTree::new();
    let mut ml = MainLoop::new(tree);
    ml.set_input_map(jump_action_map());

    // Step 1: press.
    ml.push_event(key_press(Key::Space));
    ml.step(1.0 / 60.0);

    // Step 2: release.
    ml.push_event(key_release(Key::Space));

    let snap_before_step = ml.input_state().snapshot();
    assert!(!snap_before_step.is_action_pressed("jump"));
    assert!(snap_before_step.is_action_just_released("jump"));

    ml.step(1.0 / 60.0);

    // After step 2: release edge flushed.
    let snap_after = ml.input_state().snapshot();
    assert!(!snap_after.is_action_pressed("jump"));
    assert!(!snap_after.is_action_just_released("jump"));
}

// ===========================================================================
// 12. MainLoop: multiple physics ticks share the same input state
// ===========================================================================

#[test]
fn mainloop_multiple_physics_ticks_same_input() {
    use gdscene::main_loop::MainLoop;
    use gdscene::scene_tree::SceneTree;

    let tree = SceneTree::new();
    let mut ml = MainLoop::new(tree);
    ml.set_input_map(jump_action_map());

    // Inject press.
    ml.push_event(key_press(Key::Space));

    // Use a large delta to trigger multiple physics ticks (default 60 TPS).
    // delta = 3/60 → 3 physics ticks.
    let output = ml.step(3.0 / 60.0);
    assert!(
        output.physics_steps >= 3,
        "expected >= 3 physics ticks, got {}",
        output.physics_steps
    );

    // After step, the key is still held but just_pressed is flushed.
    let snap = ml.input_state().snapshot();
    assert!(snap.is_action_pressed("jump"));
    assert!(!snap.is_action_just_pressed("jump"));
}

// ===========================================================================
// 13. Action strength tracks edge transitions
// ===========================================================================

#[test]
fn action_strength_follows_edge_transitions() {
    let mut state = state_with_actions();

    // Frame 1: press.
    let snap1 = simulate_frame(&mut state, &[key_press(Key::Space)]);
    assert_eq!(snap1.get_action_strength("jump"), 1.0);

    // Frame 2: held.
    let snap2 = simulate_frame(&mut state, &[]);
    assert_eq!(snap2.get_action_strength("jump"), 1.0);

    // Frame 3: release.
    let snap3 = simulate_frame(&mut state, &[key_release(Key::Space)]);
    assert_eq!(snap3.get_action_strength("jump"), 0.0);

    // Frame 4: idle.
    let snap4 = simulate_frame(&mut state, &[]);
    assert_eq!(snap4.get_action_strength("jump"), 0.0);
}

// ===========================================================================
// 14. get_axis reflects edge transitions across frames
// ===========================================================================

#[test]
fn get_axis_reflects_edge_transitions() {
    let mut state = state_with_actions();

    // Frame 1: press move_right.
    let snap1 = simulate_frame(&mut state, &[key_press(Key::D)]);
    assert_eq!(
        snap1.get_axis("move_left", "move_right"),
        1.0,
        "right pressed → axis should be +1"
    );

    // Frame 2: also press move_left (opposing).
    let snap2 = simulate_frame(&mut state, &[key_press(Key::A)]);
    assert_eq!(
        snap2.get_axis("move_left", "move_right"),
        0.0,
        "both pressed → axis should cancel to 0"
    );

    // Frame 3: release move_right.
    let snap3 = simulate_frame(&mut state, &[key_release(Key::D)]);
    assert_eq!(
        snap3.get_axis("move_left", "move_right"),
        -1.0,
        "only left pressed → axis should be -1"
    );

    // Frame 4: release move_left.
    let snap4 = simulate_frame(&mut state, &[key_release(Key::A)]);
    assert_eq!(
        snap4.get_axis("move_left", "move_right"),
        0.0,
        "both released → axis should be 0"
    );
}

// ===========================================================================
// 15. Simultaneous press of two different actions in one frame
// ===========================================================================

#[test]
fn simultaneous_press_two_actions() {
    let mut state = state_with_actions();

    let snap = simulate_frame(
        &mut state,
        &[key_press(Key::Space), key_press(Key::F)],
    );

    assert!(snap.is_action_just_pressed("jump"));
    assert!(snap.is_action_just_pressed("shoot"));
    assert!(snap.is_action_pressed("jump"));
    assert!(snap.is_action_pressed("shoot"));
}

#[test]
fn simultaneous_release_two_actions() {
    let mut state = state_with_actions();

    // Frame 1: press both.
    simulate_frame(&mut state, &[key_press(Key::Space), key_press(Key::F)]);

    // Frame 2: release both.
    let snap = simulate_frame(
        &mut state,
        &[key_release(Key::Space), key_release(Key::F)],
    );

    assert!(snap.is_action_just_released("jump"));
    assert!(snap.is_action_just_released("shoot"));
    assert!(!snap.is_action_pressed("jump"));
    assert!(!snap.is_action_pressed("shoot"));
}
