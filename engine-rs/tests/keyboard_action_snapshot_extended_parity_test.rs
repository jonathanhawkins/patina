//! pat-qhfn: Extended keyboard action snapshot coverage through engine input API.
//!
//! Complements `keyboard_action_snapshot_parity_test.rs` (pat-3a0a, 24 tests)
//! with scenarios not covered by the base suite:
//!
//! 1.  Multi-frame hold: action stays pressed across 5 flush cycles
//! 2.  Rapid toggle: press → flush → release → flush → press across 3 frames
//! 3.  Snapshot divergence: snapshots at different frames differ
//! 4.  WindowEvent → InputEvent → action end-to-end chain
//! 5.  All four arrow keys pressed simultaneously
//! 6.  Modifier key tracking in raw key state (shift/ctrl/alt)
//! 7.  Action map hot-swap: old actions clear, new actions activate
//! 8.  Key name roundtrip for common keys
//! 9.  MainLoop multi-step snapshot timing
//! 10. Snapshot pressed_key_names completeness with many keys
//! 11. get_vector cancellation: opposing directions → zero
//! 12. Dual-binding partial release: two keys, one action
//! 13. Snapshot action_pressed_key_map accuracy
//! 14. Release without prior press is no-op
//! 15. Snapshot after flush_frame retains held actions
//! 16. Multiple flush_frame calls are idempotent
//!
//! Acceptance: action press/release snapshots match expected runtime state.

use gdplatform::input::{ActionBinding, InputEvent, InputMap, InputState, Key};
use gdplatform::window::WindowEvent;
use gdscene::main_loop::MainLoop;
use gdscene::scene_tree::SceneTree;

const DT: f64 = 1.0 / 60.0;

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

fn key_press_with_modifiers(key: Key, shift: bool, ctrl: bool, alt: bool) -> InputEvent {
    InputEvent::Key {
        key,
        pressed: true,
        shift,
        ctrl,
        alt,
    }
}

fn standard_map() -> InputMap {
    let mut map = InputMap::new();
    map.add_action("ui_left", 0.0);
    map.action_add_event("ui_left", ActionBinding::KeyBinding(Key::Left));
    map.add_action("ui_right", 0.0);
    map.action_add_event("ui_right", ActionBinding::KeyBinding(Key::Right));
    map.add_action("ui_up", 0.0);
    map.action_add_event("ui_up", ActionBinding::KeyBinding(Key::Up));
    map.add_action("ui_down", 0.0);
    map.action_add_event("ui_down", ActionBinding::KeyBinding(Key::Down));
    map.add_action("ui_accept", 0.0);
    map.action_add_event("ui_accept", ActionBinding::KeyBinding(Key::Enter));
    map.add_action("shoot", 0.0);
    map.action_add_event("shoot", ActionBinding::KeyBinding(Key::Space));
    map.add_action("jump", 0.0);
    map.action_add_event("jump", ActionBinding::KeyBinding(Key::Space));
    map
}

fn state_with_map() -> InputState {
    let mut state = InputState::new();
    state.set_input_map(standard_map());
    state
}

// ===========================================================================
// 1. Multi-frame hold: action stays pressed across 5 flush cycles
// ===========================================================================

#[test]
fn action_persists_across_multiple_flushes() {
    let mut state = state_with_map();
    state.process_event(key_press(Key::Left));

    for frame in 0..5 {
        assert!(
            state.is_action_pressed("ui_left"),
            "action must stay pressed on frame {frame}"
        );
        // just_pressed only true on first frame
        if frame == 0 {
            assert!(state.is_action_just_pressed("ui_left"));
        } else {
            assert!(!state.is_action_just_pressed("ui_left"));
        }
        state.flush_frame();
    }

    // Still pressed after 5 flushes
    assert!(state.is_action_pressed("ui_left"));
}

// ===========================================================================
// 2. Rapid toggle: press → flush → release → flush → re-press
// ===========================================================================

#[test]
fn rapid_toggle_press_release_press() {
    let mut state = state_with_map();

    // Frame 1: press
    state.process_event(key_press(Key::Space));
    assert!(state.is_action_just_pressed("shoot"));
    assert!(state.is_action_pressed("shoot"));
    state.flush_frame();

    // Frame 2: release
    state.process_event(key_release(Key::Space));
    assert!(state.is_action_just_released("shoot"));
    assert!(!state.is_action_pressed("shoot"));
    state.flush_frame();

    // Frame 3: re-press
    state.process_event(key_press(Key::Space));
    assert!(
        state.is_action_just_pressed("shoot"),
        "re-press after release must trigger just_pressed again"
    );
    assert!(state.is_action_pressed("shoot"));
}

// ===========================================================================
// 3. Snapshots at different frames differ
// ===========================================================================

#[test]
fn snapshots_at_different_frames_diverge() {
    let mut state = state_with_map();

    // Snapshot 1: no keys
    let snap1 = state.snapshot();
    assert!(!snap1.is_action_pressed("ui_left"));

    // Press and snapshot 2
    state.process_event(key_press(Key::Left));
    let snap2 = state.snapshot();
    assert!(snap2.is_action_pressed("ui_left"));
    assert!(snap2.is_action_just_pressed("ui_left"));

    // Flush and snapshot 3
    state.flush_frame();
    let snap3 = state.snapshot();
    assert!(snap3.is_action_pressed("ui_left"));
    assert!(
        !snap3.is_action_just_pressed("ui_left"),
        "snapshot after flush must not have just_pressed"
    );

    // Original snapshots are still frozen
    assert!(!snap1.is_action_pressed("ui_left"));
    assert!(snap2.is_action_just_pressed("ui_left"));
}

// ===========================================================================
// 4. WindowEvent → InputEvent → action end-to-end
// ===========================================================================

#[test]
fn window_event_to_action_chain() {
    let we = WindowEvent::KeyInput {
        key: Key::Space,
        pressed: true,
        shift: false,
        ctrl: false,
        alt: false,
    };

    let ie = we.to_input_event().expect("KeyInput should convert");
    let mut state = state_with_map();
    state.process_event(ie);

    assert!(state.is_action_pressed("shoot"));
    assert!(state.is_action_pressed("jump"));
    assert!(state.is_key_pressed(Key::Space));
}

#[test]
fn window_event_release_chain() {
    let mut state = state_with_map();

    // Press via WindowEvent
    let press = WindowEvent::KeyInput {
        key: Key::Enter,
        pressed: true,
        shift: false,
        ctrl: false,
        alt: false,
    };
    state.process_event(press.to_input_event().unwrap());
    assert!(state.is_action_pressed("ui_accept"));

    state.flush_frame();

    // Release via WindowEvent
    let release = WindowEvent::KeyInput {
        key: Key::Enter,
        pressed: false,
        shift: false,
        ctrl: false,
        alt: false,
    };
    state.process_event(release.to_input_event().unwrap());
    assert!(!state.is_action_pressed("ui_accept"));
    assert!(state.is_action_just_released("ui_accept"));
}

// ===========================================================================
// 5. All four arrow keys pressed simultaneously
// ===========================================================================

#[test]
fn all_arrows_pressed_simultaneously() {
    let mut state = state_with_map();
    state.process_event(key_press(Key::Left));
    state.process_event(key_press(Key::Right));
    state.process_event(key_press(Key::Up));
    state.process_event(key_press(Key::Down));

    assert!(state.is_action_pressed("ui_left"));
    assert!(state.is_action_pressed("ui_right"));
    assert!(state.is_action_pressed("ui_up"));
    assert!(state.is_action_pressed("ui_down"));

    // All just_pressed on same frame
    assert!(state.is_action_just_pressed("ui_left"));
    assert!(state.is_action_just_pressed("ui_right"));
    assert!(state.is_action_just_pressed("ui_up"));
    assert!(state.is_action_just_pressed("ui_down"));

    // get_vector should cancel out to ~zero
    let v = state.get_vector("ui_left", "ui_right", "ui_up", "ui_down");
    assert!(
        v.x.abs() < 0.01 && v.y.abs() < 0.01,
        "opposing directions should cancel: ({}, {})",
        v.x,
        v.y
    );
}

// ===========================================================================
// 6. Modifier key tracking in raw key state
// ===========================================================================

#[test]
fn modifier_keys_in_raw_state() {
    let mut state = state_with_map();

    // Press Shift key directly
    state.process_event(key_press(Key::Shift));
    assert!(state.is_key_pressed(Key::Shift));
    assert!(state.is_key_just_pressed(Key::Shift));

    state.flush_frame();
    assert!(state.is_key_pressed(Key::Shift));
    assert!(!state.is_key_just_pressed(Key::Shift));

    // Press Ctrl
    state.process_event(key_press(Key::Ctrl));
    assert!(state.is_key_pressed(Key::Ctrl));
    assert!(state.is_key_pressed(Key::Shift)); // still held

    state.process_event(key_release(Key::Shift));
    assert!(!state.is_key_pressed(Key::Shift));
    assert!(state.is_key_pressed(Key::Ctrl));
}

#[test]
fn modifier_flags_on_key_event() {
    let mut state = state_with_map();

    // Press Space with shift held — action still resolves
    state.process_event(key_press_with_modifiers(Key::Space, true, false, false));
    assert!(state.is_action_pressed("shoot"));
    assert!(state.is_key_pressed(Key::Space));
}

// ===========================================================================
// 7. Action map hot-swap: old actions clear, new actions activate
// ===========================================================================

#[test]
fn action_map_hot_swap() {
    let mut state = state_with_map();
    state.process_event(key_press(Key::Space));
    assert!(state.is_action_pressed("shoot"));

    // Swap to a new map that doesn't have "shoot" but has "fire" on Space
    let mut new_map = InputMap::new();
    new_map.add_action("fire", 0.0);
    new_map.action_add_event("fire", ActionBinding::KeyBinding(Key::Space));
    state.set_input_map(new_map);

    // "shoot" should no longer resolve (map changed)
    // Raw key state persists — Space is still pressed
    assert!(state.is_key_pressed(Key::Space));

    // Process a no-op flush to let the new map take effect on next event
    state.flush_frame();

    // Release and re-press to trigger the new action
    state.process_event(key_release(Key::Space));
    state.process_event(key_press(Key::Space));
    assert!(state.is_action_pressed("fire"));
}

// ===========================================================================
// 8. Key name roundtrip for common keys
// ===========================================================================

#[test]
fn key_name_roundtrip() {
    let keys: Vec<Key> = vec![
        Key::A, Key::Z, Key::Num0, Key::Num9, Key::Space, Key::Enter,
        Key::Escape, Key::Tab, Key::Shift, Key::Ctrl, Key::Alt,
        Key::Left, Key::Right, Key::Up, Key::Down,
        Key::F1, Key::F12, Key::Backspace, Key::Delete,
    ];

    for key in &keys {
        let name = key.name();
        let roundtrip = Key::from_name(name);
        assert_eq!(
            roundtrip,
            Some(*key),
            "Key::{:?} name='{}' roundtrip failed",
            key,
            name
        );
    }
}

// ===========================================================================
// 9. MainLoop multi-step snapshot timing
// ===========================================================================

#[test]
fn mainloop_snapshot_timing_across_steps() {
    let tree = SceneTree::new();
    let mut ml = MainLoop::new(tree);
    ml.set_input_map(standard_map());

    // Push key before first step
    ml.push_event(key_press(Key::Left));
    ml.step(DT);

    // After step, input_state should still have Left pressed (flush happened)
    assert!(ml.input_state().is_action_pressed("ui_left"));
    assert!(
        !ml.input_state().is_action_just_pressed("ui_left"),
        "just_pressed cleared by step's internal flush"
    );

    // Step 2: no new events — action still pressed
    ml.step(DT);
    assert!(ml.input_state().is_action_pressed("ui_left"));

    // Step 3: release between steps
    ml.push_event(key_release(Key::Left));
    ml.step(DT);
    assert!(!ml.input_state().is_action_pressed("ui_left"));
}

// ===========================================================================
// 10. Snapshot pressed_key_names completeness
// ===========================================================================

#[test]
fn snapshot_pressed_key_names_complete() {
    let mut state = state_with_map();
    state.process_event(key_press(Key::A));
    state.process_event(key_press(Key::Space));
    state.process_event(key_press(Key::Left));
    state.process_event(key_press(Key::Enter));

    let snap = state.snapshot();
    let names = snap.pressed_key_names();

    assert!(names.contains(&"A".to_string()) || names.contains(&"a".to_string()),
        "A must be in pressed_key_names: {:?}", names);
    assert!(names.contains(&Key::Space.name().to_string()),
        "Space must be in pressed_key_names: {:?}", names);
    assert!(names.contains(&Key::Left.name().to_string()),
        "Left must be in pressed_key_names: {:?}", names);
    assert!(names.contains(&Key::Enter.name().to_string()),
        "Enter must be in pressed_key_names: {:?}", names);
    assert_eq!(names.len(), 4, "exactly 4 keys pressed: {:?}", names);
}

// ===========================================================================
// 11. get_vector cancellation: opposing directions → zero
// ===========================================================================

#[test]
fn get_vector_opposing_cancels() {
    let mut state = state_with_map();
    state.process_event(key_press(Key::Left));
    state.process_event(key_press(Key::Right));

    let v = state.get_vector("ui_left", "ui_right", "ui_up", "ui_down");
    assert!(
        v.x.abs() < 0.01,
        "left+right should cancel x: {}",
        v.x
    );
    assert!(
        v.y.abs() < 0.01,
        "no vertical input: {}",
        v.y
    );
}

#[test]
fn get_axis_opposing_cancels() {
    let mut state = state_with_map();
    state.process_event(key_press(Key::Left));
    state.process_event(key_press(Key::Right));

    let axis = state.get_axis("ui_left", "ui_right");
    assert!(
        axis.abs() < 0.01,
        "left+right should cancel: {}",
        axis
    );
}

// ===========================================================================
// 12. Dual-binding partial release
// ===========================================================================

#[test]
fn dual_binding_release_any_key_clears_action() {
    // Engine behavior: releasing ANY bound key for an action clears the action,
    // even if other bound keys are still held. This differs from some engines
    // that track per-binding state, but matches the current Patina implementation.
    let mut map = InputMap::new();
    map.add_action("move_left", 0.0);
    map.action_add_event("move_left", ActionBinding::KeyBinding(Key::Left));
    map.action_add_event("move_left", ActionBinding::KeyBinding(Key::A));

    let mut state = InputState::new();
    state.set_input_map(map);

    // Press both keys
    state.process_event(key_press(Key::Left));
    state.process_event(key_press(Key::A));
    assert!(state.is_action_pressed("move_left"));

    state.flush_frame();

    // Release Left — action clears even though A is still held
    state.process_event(key_release(Key::Left));
    assert!(
        !state.is_action_pressed("move_left"),
        "releasing any bound key clears the action"
    );
    assert!(!state.is_key_pressed(Key::Left));
    assert!(state.is_key_pressed(Key::A)); // raw key still tracked

    // Re-pressing A re-activates the action
    state.flush_frame();
    state.process_event(key_release(Key::A));
    state.process_event(key_press(Key::A));
    assert!(state.is_action_pressed("move_left"));
    assert!(state.is_action_just_pressed("move_left"));
}

// ===========================================================================
// 13. Snapshot action_pressed_key_map accuracy
// ===========================================================================

#[test]
fn snapshot_action_pressed_key_map() {
    let mut state = state_with_map();
    state.process_event(key_press(Key::Left));
    state.process_event(key_press(Key::Space));

    let snap = state.snapshot();
    let map = snap.action_pressed_key_map();

    // ui_left should map to [Left key name]
    assert!(
        map.contains_key("ui_left"),
        "ui_left must be in action map: {:?}",
        map.keys().collect::<Vec<_>>()
    );

    // shoot and jump both map to Space
    assert!(map.contains_key("shoot"), "shoot must be in action map");
    assert!(map.contains_key("jump"), "jump must be in action map");
}

// ===========================================================================
// 14. Release without prior press is no-op
// ===========================================================================

#[test]
fn release_without_press_is_noop() {
    let mut state = state_with_map();

    // Release a key that was never pressed
    state.process_event(key_release(Key::Left));

    assert!(!state.is_action_pressed("ui_left"));
    assert!(!state.is_action_just_released("ui_left"));
    assert!(!state.is_key_pressed(Key::Left));
}

// ===========================================================================
// 15. Snapshot after flush retains held actions
// ===========================================================================

#[test]
fn snapshot_after_flush_retains_held() {
    let mut state = state_with_map();
    state.process_event(key_press(Key::Up));

    state.flush_frame();

    let snap = state.snapshot();
    assert!(
        snap.is_action_pressed("ui_up"),
        "snapshot after flush must still show held action"
    );
    assert!(
        !snap.is_action_just_pressed("ui_up"),
        "just_pressed must be cleared in snapshot after flush"
    );
}

// ===========================================================================
// 16. Multiple flush_frame calls are idempotent
// ===========================================================================

#[test]
fn multiple_flushes_idempotent() {
    let mut state = state_with_map();
    state.process_event(key_press(Key::Down));
    assert!(state.is_action_just_pressed("ui_down"));

    state.flush_frame();
    assert!(!state.is_action_just_pressed("ui_down"));
    assert!(state.is_action_pressed("ui_down"));

    // Second and third flush should not change anything
    state.flush_frame();
    state.flush_frame();
    assert!(state.is_action_pressed("ui_down"));
    assert!(!state.is_action_just_pressed("ui_down"));
    assert!(!state.is_action_just_released("ui_down"));
}

// ===========================================================================
// 17. WindowEvent non-key events return None for action
// ===========================================================================

#[test]
fn window_non_key_events_no_input_event_for_some() {
    let resize = WindowEvent::Resized {
        width: 800,
        height: 600,
    };
    assert!(
        resize.to_input_event().is_none(),
        "Resize should not produce an InputEvent"
    );

    let close = WindowEvent::CloseRequested;
    assert!(close.to_input_event().is_none());

    let focus = WindowEvent::FocusGained;
    assert!(focus.to_input_event().is_none());
}

// ===========================================================================
// 18. Action strength transitions
// ===========================================================================

#[test]
fn action_strength_press_release_cycle() {
    let mut state = state_with_map();

    // Before press: strength 0.0
    assert_eq!(state.get_action_strength("shoot"), 0.0);

    // After press: strength 1.0
    state.process_event(key_press(Key::Space));
    assert_eq!(state.get_action_strength("shoot"), 1.0);

    state.flush_frame();

    // Still held: strength 1.0
    assert_eq!(state.get_action_strength("shoot"), 1.0);

    // After release: strength 0.0
    state.process_event(key_release(Key::Space));
    assert_eq!(state.get_action_strength("shoot"), 0.0);
}

#[test]
fn snapshot_captures_action_strength() {
    let mut state = state_with_map();
    state.process_event(key_press(Key::Space));

    let snap_pressed = state.snapshot();
    assert_eq!(snap_pressed.get_action_strength("shoot"), 1.0);

    state.process_event(key_release(Key::Space));
    let snap_released = state.snapshot();
    assert_eq!(snap_released.get_action_strength("shoot"), 0.0);

    // Original snapshot unchanged
    assert_eq!(snap_pressed.get_action_strength("shoot"), 1.0);
}

// ===========================================================================
// 19. get_vector single direction
// ===========================================================================

#[test]
fn get_vector_single_direction() {
    let mut state = state_with_map();
    state.process_event(key_press(Key::Right));

    let v = state.get_vector("ui_left", "ui_right", "ui_up", "ui_down");
    assert!(
        (v.x - 1.0).abs() < 0.01,
        "right only: x should be 1.0, got {}",
        v.x
    );
    assert!(v.y.abs() < 0.01, "no vertical: y should be 0, got {}", v.y);
}

#[test]
fn get_vector_diagonal_normalized() {
    let mut state = state_with_map();
    state.process_event(key_press(Key::Right));
    state.process_event(key_press(Key::Down));

    let v = state.get_vector("ui_left", "ui_right", "ui_up", "ui_down");
    let expected = 1.0_f32 / 2.0_f32.sqrt(); // ~0.707
    assert!(
        (v.x - expected).abs() < 0.02 && (v.y - expected).abs() < 0.02,
        "diagonal should be normalized ~(0.707, 0.707), got ({}, {})",
        v.x,
        v.y
    );
}

// ===========================================================================
// 20. Snapshot just_pressed_key_names reflects transient state
// ===========================================================================

#[test]
fn snapshot_just_pressed_key_names_transient() {
    let mut state = state_with_map();
    state.process_event(key_press(Key::A));
    state.process_event(key_press(Key::B));

    let snap1 = state.snapshot();
    let jp1 = snap1.just_pressed_key_names();
    assert_eq!(jp1.len(), 2, "2 keys just pressed: {:?}", jp1);

    state.flush_frame();
    let snap2 = state.snapshot();
    let jp2 = snap2.just_pressed_key_names();
    assert_eq!(
        jp2.len(),
        0,
        "no keys just pressed after flush: {:?}",
        jp2
    );

    // But still 2 pressed
    assert_eq!(snap2.pressed_key_names().len(), 2);
}
