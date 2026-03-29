//! pat-fz8r: Cover keyboard action snapshots through engine input API.
//!
//! Focused on the InputSnapshot contract: every query method on InputSnapshot
//! must faithfully reflect the InputState at the moment `snapshot()` was called.
//! Tests exercise the full pipeline: key event → InputState → snapshot → query,
//! with emphasis on snapshot isolation, multi-frame correctness, and edge cases
//! not covered by pat-3a0a (base) or pat-qhfn (extended).
//!
//! Acceptance: action press/release snapshots match expected runtime state.

use gdplatform::input::{ActionBinding, InputEvent, InputMap, InputState, Key};

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

fn wasd_action_map() -> InputMap {
    let mut map = InputMap::new();
    map.add_action("move_left", 0.0);
    map.action_add_event("move_left", ActionBinding::KeyBinding(Key::A));
    map.add_action("move_right", 0.0);
    map.action_add_event("move_right", ActionBinding::KeyBinding(Key::D));
    map.add_action("move_up", 0.0);
    map.action_add_event("move_up", ActionBinding::KeyBinding(Key::W));
    map.add_action("move_down", 0.0);
    map.action_add_event("move_down", ActionBinding::KeyBinding(Key::S));
    map.add_action("jump", 0.0);
    map.action_add_event("jump", ActionBinding::KeyBinding(Key::Space));
    map.add_action("interact", 0.0);
    map.action_add_event("interact", ActionBinding::KeyBinding(Key::E));
    map
}

fn state_with_wasd() -> InputState {
    let mut state = InputState::new();
    state.set_input_map(wasd_action_map());
    state
}

// ===========================================================================
// 1. Snapshot of empty state has no actions pressed
// ===========================================================================

#[test]
fn fz8r_empty_snapshot_no_actions() {
    let state = state_with_wasd();
    let snap = state.snapshot();

    assert!(!snap.is_action_pressed("move_left"));
    assert!(!snap.is_action_pressed("jump"));
    assert!(!snap.is_action_just_pressed("jump"));
    assert!(!snap.is_action_just_released("jump"));
    assert_eq!(snap.get_action_strength("jump"), 0.0);
    assert_eq!(snap.get_axis("move_left", "move_right"), 0.0);

    let vec = snap.get_vector("move_left", "move_right", "move_up", "move_down");
    assert_eq!(vec.x, 0.0);
    assert_eq!(vec.y, 0.0);
}

// ===========================================================================
// 2. Snapshot captures just_pressed for action on press frame
// ===========================================================================

#[test]
fn fz8r_snapshot_captures_just_pressed() {
    let mut state = state_with_wasd();
    state.process_event(key_press(Key::Space));

    let snap = state.snapshot();
    assert!(snap.is_action_pressed("jump"));
    assert!(snap.is_action_just_pressed("jump"));
    assert!(!snap.is_action_just_released("jump"));
    assert_eq!(snap.get_action_strength("jump"), 1.0);
}

// ===========================================================================
// 3. Snapshot after flush loses just_pressed but retains pressed
// ===========================================================================

#[test]
fn fz8r_snapshot_after_flush_loses_just_pressed() {
    let mut state = state_with_wasd();
    state.process_event(key_press(Key::Space));
    state.flush_frame();

    let snap = state.snapshot();
    assert!(snap.is_action_pressed("jump"), "held key stays pressed");
    assert!(
        !snap.is_action_just_pressed("jump"),
        "just_pressed cleared by flush"
    );
    assert_eq!(snap.get_action_strength("jump"), 1.0);
}

// ===========================================================================
// 4. Snapshot captures just_released on release frame
// ===========================================================================

#[test]
fn fz8r_snapshot_captures_just_released() {
    let mut state = state_with_wasd();
    state.process_event(key_press(Key::E));
    state.flush_frame();

    state.process_event(key_release(Key::E));
    let snap = state.snapshot();

    assert!(!snap.is_action_pressed("interact"));
    assert!(!snap.is_action_just_pressed("interact"));
    assert!(snap.is_action_just_released("interact"));
    assert_eq!(snap.get_action_strength("interact"), 0.0);
}

// ===========================================================================
// 5. Snapshot clone is independent
// ===========================================================================

#[test]
fn fz8r_snapshot_clone_independent() {
    let mut state = state_with_wasd();
    state.process_event(key_press(Key::A));

    let snap1 = state.snapshot();
    let snap2 = snap1.clone();

    // Mutate state — neither snapshot should be affected.
    state.process_event(key_release(Key::A));
    state.flush_frame();

    assert!(snap1.is_action_pressed("move_left"));
    assert!(snap2.is_action_pressed("move_left"));
    assert!(snap1.is_action_just_pressed("move_left"));
    assert!(snap2.is_action_just_pressed("move_left"));
}

// ===========================================================================
// 6. Two snapshots at different frames diverge
// ===========================================================================

#[test]
fn fz8r_snapshots_diverge_across_frames() {
    let mut state = state_with_wasd();
    state.process_event(key_press(Key::W));

    let snap_frame1 = state.snapshot();
    state.flush_frame();

    let snap_frame2 = state.snapshot();

    // Frame 1: just_pressed is true.
    assert!(snap_frame1.is_action_just_pressed("move_up"));
    // Frame 2: just_pressed is cleared but pressed remains.
    assert!(!snap_frame2.is_action_just_pressed("move_up"));
    assert!(snap_frame2.is_action_pressed("move_up"));
}

// ===========================================================================
// 7. Snapshot get_axis reflects single direction press
// ===========================================================================

#[test]
fn fz8r_snapshot_get_axis_single_direction() {
    let mut state = state_with_wasd();
    state.process_event(key_press(Key::D));

    let snap = state.snapshot();
    let axis = snap.get_axis("move_left", "move_right");
    assert_eq!(axis, 1.0, "right only → positive axis");
}

// ===========================================================================
// 8. Snapshot get_axis opposing directions cancel
// ===========================================================================

#[test]
fn fz8r_snapshot_get_axis_opposing_cancel() {
    let mut state = state_with_wasd();
    state.process_event(key_press(Key::A));
    state.process_event(key_press(Key::D));

    let snap = state.snapshot();
    let axis = snap.get_axis("move_left", "move_right");
    assert_eq!(axis, 0.0, "left + right → zero");
}

// ===========================================================================
// 9. Snapshot get_vector diagonal is normalized
// ===========================================================================

#[test]
fn fz8r_snapshot_get_vector_diagonal_normalized() {
    let mut state = state_with_wasd();
    state.process_event(key_press(Key::D));
    state.process_event(key_press(Key::S));

    let snap = state.snapshot();
    let vec = snap.get_vector("move_left", "move_right", "move_up", "move_down");

    let len = (vec.x * vec.x + vec.y * vec.y).sqrt();
    assert!(
        (len - 1.0).abs() < 0.01,
        "diagonal vector should be normalized to ~1.0, got {len}"
    );
    assert!(vec.x > 0.0, "right component positive");
    assert!(vec.y > 0.0, "down component positive");
}

// ===========================================================================
// 10. Snapshot get_vector single axis not normalized
// ===========================================================================

#[test]
fn fz8r_snapshot_get_vector_single_axis_unit() {
    let mut state = state_with_wasd();
    state.process_event(key_press(Key::W));

    let snap = state.snapshot();
    let vec = snap.get_vector("move_left", "move_right", "move_up", "move_down");

    assert_eq!(vec.x, 0.0);
    assert_eq!(vec.y, -1.0, "up only → y = -1");
}

// ===========================================================================
// 11. Snapshot key state mirrors action state for mapped keys
// ===========================================================================

#[test]
fn fz8r_snapshot_key_and_action_consistent() {
    let mut state = state_with_wasd();
    state.process_event(key_press(Key::Space));

    let snap = state.snapshot();
    assert!(snap.is_key_pressed(Key::Space));
    assert!(snap.is_key_just_pressed(Key::Space));
    assert!(snap.is_action_pressed("jump"));
    assert!(snap.is_action_just_pressed("jump"));
}

// ===========================================================================
// 12. Snapshot key release mirrors action release
// ===========================================================================

#[test]
fn fz8r_snapshot_key_and_action_release_consistent() {
    let mut state = state_with_wasd();
    state.process_event(key_press(Key::Space));
    state.flush_frame();
    state.process_event(key_release(Key::Space));

    let snap = state.snapshot();
    assert!(!snap.is_key_pressed(Key::Space));
    assert!(snap.is_key_just_released(Key::Space));
    assert!(!snap.is_action_pressed("jump"));
    assert!(snap.is_action_just_released("jump"));
}

// ===========================================================================
// 13. Unmapped key appears in snapshot key state but not actions
// ===========================================================================

#[test]
fn fz8r_snapshot_unmapped_key_no_action() {
    let mut state = state_with_wasd();
    state.process_event(key_press(Key::F1));

    let snap = state.snapshot();
    assert!(snap.is_key_pressed(Key::F1));
    assert!(snap.is_key_just_pressed(Key::F1));
    // No action mapped to F1.
    assert!(!snap.is_action_pressed("jump"));
    assert!(!snap.is_action_pressed("move_left"));
}

// ===========================================================================
// 14. Querying nonexistent action returns false/zero
// ===========================================================================

#[test]
fn fz8r_snapshot_nonexistent_action_safe() {
    let state = state_with_wasd();
    let snap = state.snapshot();

    assert!(!snap.is_action_pressed("nonexistent"));
    assert!(!snap.is_action_just_pressed("nonexistent"));
    assert!(!snap.is_action_just_released("nonexistent"));
    assert_eq!(snap.get_action_strength("nonexistent"), 0.0);
    assert_eq!(snap.get_axis("nonexistent", "also_nonexistent"), 0.0);
}

// ===========================================================================
// 15. Multi-action simultaneous press all visible in snapshot
// ===========================================================================

#[test]
fn fz8r_snapshot_multiple_simultaneous_actions() {
    let mut state = state_with_wasd();
    state.process_event(key_press(Key::W));
    state.process_event(key_press(Key::A));
    state.process_event(key_press(Key::Space));

    let snap = state.snapshot();
    assert!(snap.is_action_pressed("move_up"));
    assert!(snap.is_action_pressed("move_left"));
    assert!(snap.is_action_pressed("jump"));
    assert!(!snap.is_action_pressed("move_right"));
    assert!(!snap.is_action_pressed("move_down"));
    assert!(!snap.is_action_pressed("interact"));

    assert!(snap.is_action_just_pressed("move_up"));
    assert!(snap.is_action_just_pressed("move_left"));
    assert!(snap.is_action_just_pressed("jump"));
}

// ===========================================================================
// 16. Snapshot pressed_key_names includes all held keys
// ===========================================================================

#[test]
fn fz8r_snapshot_pressed_key_names_complete() {
    let mut state = state_with_wasd();
    state.process_event(key_press(Key::W));
    state.process_event(key_press(Key::D));

    let snap = state.snapshot();
    let names = snap.pressed_key_names();

    assert!(names.contains(&"W".to_string()));
    assert!(names.contains(&"D".to_string()));
    assert_eq!(names.len(), 2);
}

// ===========================================================================
// 17. Snapshot just_pressed_key_names matches just pressed
// ===========================================================================

#[test]
fn fz8r_snapshot_just_pressed_key_names_transient() {
    let mut state = state_with_wasd();
    state.process_event(key_press(Key::A));
    state.process_event(key_press(Key::S));

    let snap1 = state.snapshot();
    let jp1 = snap1.just_pressed_key_names();
    assert_eq!(jp1.len(), 2);
    assert!(jp1.contains(&"A".to_string()));
    assert!(jp1.contains(&"S".to_string()));

    state.flush_frame();
    let snap2 = state.snapshot();
    let jp2 = snap2.just_pressed_key_names();
    assert_eq!(jp2.len(), 0, "just_pressed_key_names empty after flush");
}

// ===========================================================================
// 18. Snapshot action_pressed_key_map entries match pressed actions
// ===========================================================================

#[test]
fn fz8r_snapshot_action_pressed_key_map_entries() {
    let mut state = state_with_wasd();
    state.process_event(key_press(Key::W));
    state.process_event(key_press(Key::Space));

    let snap = state.snapshot();
    let map = snap.action_pressed_key_map();

    assert!(map.contains_key("move_up"), "move_up should be in map");
    assert!(map.contains_key("jump"), "jump should be in map");
    assert!(!map.contains_key("move_left"), "move_left not pressed");
}

// ===========================================================================
// 19. Press-release-press across frames: just_pressed re-triggers
// ===========================================================================

#[test]
fn fz8r_press_release_press_retriggers_just_pressed() {
    let mut state = state_with_wasd();

    // Frame 1: press
    state.process_event(key_press(Key::Space));
    let snap1 = state.snapshot();
    assert!(snap1.is_action_just_pressed("jump"));
    state.flush_frame();

    // Frame 2: release
    state.process_event(key_release(Key::Space));
    state.flush_frame();

    // Frame 3: press again
    state.process_event(key_press(Key::Space));
    let snap3 = state.snapshot();
    assert!(
        snap3.is_action_just_pressed("jump"),
        "re-press after release should re-trigger just_pressed"
    );
    assert!(snap3.is_action_pressed("jump"));
}

// ===========================================================================
// 20. Snapshot action strength transitions through lifecycle
// ===========================================================================

#[test]
fn fz8r_snapshot_action_strength_lifecycle() {
    let mut state = state_with_wasd();

    // Not pressed → 0.0
    assert_eq!(state.snapshot().get_action_strength("jump"), 0.0);

    // Pressed → 1.0
    state.process_event(key_press(Key::Space));
    assert_eq!(state.snapshot().get_action_strength("jump"), 1.0);

    // Still held after flush → 1.0
    state.flush_frame();
    assert_eq!(state.snapshot().get_action_strength("jump"), 1.0);

    // Released → 0.0
    state.process_event(key_release(Key::Space));
    assert_eq!(state.snapshot().get_action_strength("jump"), 0.0);
}
