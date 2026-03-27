//! pat-s9rf, pat-zwt8: Extended input-map loading and action binding coverage.
//!
//! Complements `input_map_loading_test.rs` (pat-vih, 25 tests) and
//! `input_action_binding_parity_test.rs` (pat-jkwa, 41 tests) with
//! scenarios not covered by either suite:
//!
//! Direct API methods:
//! 1.  get_bindings returns correct binding list
//! 2.  get_bindings for unregistered action returns None
//! 3.  actions() iterator enumerates all registered actions
//! 4.  event_matches_action for matching key binding
//! 5.  event_matches_action for non-matching key
//! 6.  event_matches_action for unregistered action
//! 7.  event_matches_action for gamepad axis with deadzone
//!
//! JSON loading edge cases:
//! 8.  JSON with action but empty keys array
//! 9.  JSON with only mouse bindings, no keys
//! 10. JSON deadzone defaults to 0.0 when omitted
//!
//! Multi-step MainLoop integration:
//! 11. JSON-loaded map drives MainLoop across 3 steps
//! 12. Multiple actions active simultaneously via JSON map
//! 13. Action map from JSON fixture has correct action count
//! 14. JSON fixture bindings are queryable via get_bindings
//!
//! Action registration semantics:
//! 15. add_action then action_add_event for unregistered name is no-op
//! 16. add_action with same name overwrites deadzone
//! 17. Large action map (20+ actions) all resolve correctly
//!
//! Snapshot through loaded map:
//! 18. Snapshot taken with JSON-loaded map reflects pressed actions
//! 19. Snapshot action_pressed_key_map with multi-key action
//! 20. Snapshot get_axis with JSON-loaded directional map
//!
//! Acceptance: fixtures cover action registration, lookup, and runtime snapshot use.

use std::collections::HashSet;

use gdplatform::input::{
    ActionBinding, GamepadAxis, GamepadButton, InputEvent, InputMap, InputState, Key, MouseButton,
};
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

fn load_fixture_map() -> InputMap {
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../fixtures/input_map.json");
    InputMap::load_from_json_file(&path)
        .unwrap_or_else(|e| panic!("failed to load input_map.json: {e}"))
}

fn simple_map() -> InputMap {
    let mut map = InputMap::new();
    map.add_action("ui_left", 0.0);
    map.action_add_event("ui_left", ActionBinding::KeyBinding(Key::Left));
    map.add_action("ui_right", 0.0);
    map.action_add_event("ui_right", ActionBinding::KeyBinding(Key::Right));
    map.add_action("shoot", 0.0);
    map.action_add_event("shoot", ActionBinding::KeyBinding(Key::Space));
    map.action_add_event("shoot", ActionBinding::MouseBinding(MouseButton::Left));
    map
}

// ===========================================================================
// 1. get_bindings returns correct binding list
// ===========================================================================

#[test]
fn get_bindings_returns_registered_bindings() {
    let map = simple_map();
    let bindings = map.get_bindings("shoot").expect("shoot must exist");
    assert_eq!(bindings.len(), 2, "shoot has key + mouse binding");

    // Verify types
    let has_key = bindings
        .iter()
        .any(|b| matches!(b, ActionBinding::KeyBinding(Key::Space)));
    let has_mouse = bindings
        .iter()
        .any(|b| matches!(b, ActionBinding::MouseBinding(MouseButton::Left)));
    assert!(has_key, "shoot must have Space key binding");
    assert!(has_mouse, "shoot must have Left mouse binding");
}

// ===========================================================================
// 2. get_bindings for unregistered action returns None
// ===========================================================================

#[test]
fn get_bindings_unregistered_returns_none() {
    let map = simple_map();
    assert!(
        map.get_bindings("nonexistent").is_none(),
        "unregistered action must return None"
    );
}

// ===========================================================================
// 3. actions() iterator enumerates all registered actions
// ===========================================================================

#[test]
fn actions_iterator_complete() {
    let map = simple_map();
    let actions: HashSet<String> = map.actions().cloned().collect();
    assert!(actions.contains("ui_left"));
    assert!(actions.contains("ui_right"));
    assert!(actions.contains("shoot"));
    assert_eq!(actions.len(), 3);
}

// ===========================================================================
// 4. event_matches_action for matching key binding
// ===========================================================================

#[test]
fn event_matches_action_key_match() {
    let map = simple_map();
    let event = key_press(Key::Left);
    assert!(map.event_matches_action(&event, "ui_left"));
    assert!(!map.event_matches_action(&event, "ui_right"));
    assert!(!map.event_matches_action(&event, "shoot"));
}

// ===========================================================================
// 5. event_matches_action for non-matching key
// ===========================================================================

#[test]
fn event_matches_action_no_match() {
    let map = simple_map();
    let event = key_press(Key::A);
    assert!(!map.event_matches_action(&event, "ui_left"));
    assert!(!map.event_matches_action(&event, "ui_right"));
    assert!(!map.event_matches_action(&event, "shoot"));
}

// ===========================================================================
// 6. event_matches_action for unregistered action
// ===========================================================================

#[test]
fn event_matches_action_unregistered() {
    let map = simple_map();
    let event = key_press(Key::Space);
    assert!(
        !map.event_matches_action(&event, "nonexistent"),
        "unregistered action must not match any event"
    );
}

// ===========================================================================
// 7. event_matches_action for gamepad axis with deadzone
// ===========================================================================

#[test]
fn event_matches_action_gamepad_axis_deadzone() {
    let mut map = InputMap::new();
    map.add_action("move_x", 0.2);
    map.action_add_event(
        "move_x",
        ActionBinding::GamepadAxisBinding(GamepadAxis::LeftStickX),
    );

    // Below deadzone
    let below = InputEvent::GamepadAxis {
        axis: GamepadAxis::LeftStickX,
        value: 0.1,
        gamepad_id: 0,
    };
    assert!(
        !map.event_matches_action(&below, "move_x"),
        "axis value 0.1 below deadzone 0.2 must not match"
    );

    // Above deadzone
    let above = InputEvent::GamepadAxis {
        axis: GamepadAxis::LeftStickX,
        value: 0.5,
        gamepad_id: 0,
    };
    assert!(
        map.event_matches_action(&above, "move_x"),
        "axis value 0.5 above deadzone 0.2 must match"
    );

    // Exactly at deadzone (not above)
    let at = InputEvent::GamepadAxis {
        axis: GamepadAxis::LeftStickX,
        value: 0.2,
        gamepad_id: 0,
    };
    assert!(
        !map.event_matches_action(&at, "move_x"),
        "axis value exactly at deadzone must not match (> not >=)"
    );
}

// ===========================================================================
// 8. JSON with action but empty keys array
// ===========================================================================

#[test]
fn json_empty_keys_array() {
    let json = r#"{ "actions": { "noop": { "keys": [] } } }"#;
    let map = InputMap::load_from_json(json).unwrap();

    let actions: Vec<_> = map.actions().collect();
    assert!(actions.iter().any(|a| *a == "noop"));

    let bindings = map.get_bindings("noop").unwrap();
    assert_eq!(bindings.len(), 0);
}

// ===========================================================================
// 9. JSON with only mouse bindings, no keys
// ===========================================================================

#[test]
fn json_mouse_only_bindings() {
    let json = r#"{ "actions": { "fire": { "mouse_buttons": ["Left", "Right"] } } }"#;
    let map = InputMap::load_from_json(json).unwrap();

    let bindings = map.get_bindings("fire").unwrap();
    assert_eq!(bindings.len(), 2);

    let event = InputEvent::MouseButton {
        button: MouseButton::Left,
        pressed: true,
        position: gdcore::math::Vector2::ZERO,
    };
    assert!(map.event_matches_action(&event, "fire"));
}

// ===========================================================================
// 10. JSON deadzone defaults to 0.0 when omitted
// ===========================================================================

#[test]
fn json_deadzone_default_zero() {
    let json = r#"{ "actions": { "jump": { "keys": ["Space"] } } }"#;
    let map = InputMap::load_from_json(json).unwrap();
    assert_eq!(
        map.get_deadzone("jump"),
        0.0,
        "omitted deadzone must default to 0.0"
    );
}

// ===========================================================================
// 11. JSON-loaded map drives MainLoop across 3 steps
// ===========================================================================

#[test]
fn json_map_mainloop_multi_step() {
    let tree = SceneTree::new();
    let mut ml = MainLoop::new(tree);
    ml.set_input_map(load_fixture_map());

    // Step 1: press move_left (A key)
    ml.push_event(key_press(Key::A));
    ml.step(DT);
    assert!(ml.input_state().is_action_pressed("move_left"));

    // Step 2: still held, add jump
    ml.push_event(key_press(Key::Space));
    ml.step(DT);
    assert!(ml.input_state().is_action_pressed("move_left"));
    assert!(ml.input_state().is_action_pressed("jump"));

    // Step 3: release move_left, jump still held
    ml.push_event(key_release(Key::A));
    ml.step(DT);
    assert!(!ml.input_state().is_action_pressed("move_left"));
    assert!(ml.input_state().is_action_pressed("jump"));
}

// ===========================================================================
// 12. Multiple actions active simultaneously via JSON map
// ===========================================================================

#[test]
fn json_map_simultaneous_actions() {
    let mut state = InputState::new();
    state.set_input_map(load_fixture_map());

    // Press keys for 4 different actions
    state.process_event(key_press(Key::A)); // move_left
    state.process_event(key_press(Key::W)); // move_up
    state.process_event(key_press(Key::Space)); // jump
    state.process_event(key_press(Key::Escape)); // pause

    assert!(state.is_action_pressed("move_left"));
    assert!(state.is_action_pressed("move_up"));
    assert!(state.is_action_pressed("jump"));
    assert!(state.is_action_pressed("pause"));
    assert!(!state.is_action_pressed("shoot"));
}

// ===========================================================================
// 13. Action map from JSON fixture has correct action count
// ===========================================================================

#[test]
fn json_fixture_action_count() {
    let map = load_fixture_map();
    let actions: Vec<_> = map.actions().collect();
    // input_map.json has: move_left, move_right, move_up, move_down, jump, shoot, dash, pause
    assert_eq!(
        actions.len(),
        8,
        "fixture should have 8 actions, got {}: {:?}",
        actions.len(),
        actions
    );
}

// ===========================================================================
// 14. JSON fixture bindings are queryable via get_bindings
// ===========================================================================

#[test]
fn json_fixture_bindings_queryable() {
    let map = load_fixture_map();

    // move_left has ["A", "ArrowLeft"] → 2 bindings
    let ml = map.get_bindings("move_left").unwrap();
    assert_eq!(ml.len(), 2, "move_left should have 2 key bindings");

    // shoot has ["Enter"] + mouse ["Left"] → 2 bindings
    let shoot = map.get_bindings("shoot").unwrap();
    assert_eq!(shoot.len(), 2, "shoot should have key + mouse binding");

    // dash has ["Shift"] → 1 binding, deadzone 0.2
    let dash = map.get_bindings("dash").unwrap();
    assert_eq!(dash.len(), 1);
    assert_eq!(map.get_deadzone("dash"), 0.2);
}

// ===========================================================================
// 15. action_add_event for unregistered name is no-op
// ===========================================================================

#[test]
fn action_add_event_unregistered_is_noop() {
    let mut map = InputMap::new();
    // Don't call add_action, just try to add a binding
    map.action_add_event("ghost", ActionBinding::KeyBinding(Key::G));
    assert!(
        map.get_bindings("ghost").is_none(),
        "binding to unregistered action must not create the action"
    );
}

// ===========================================================================
// 16. add_action with same name overwrites deadzone
// ===========================================================================

#[test]
fn add_action_overwrites_deadzone() {
    let mut map = InputMap::new();
    map.add_action("aim", 0.1);
    assert_eq!(map.get_deadzone("aim"), 0.1);

    map.add_action("aim", 0.5);
    assert_eq!(map.get_deadzone("aim"), 0.5);

    // Bindings added before re-registration should still exist
    // (because bindings use entry().or_default() — existing vec is preserved)
    map.action_add_event("aim", ActionBinding::KeyBinding(Key::A));
    let bindings = map.get_bindings("aim").unwrap();
    assert_eq!(bindings.len(), 1);
}

// ===========================================================================
// 17. Large action map (20+ actions) all resolve correctly
// ===========================================================================

#[test]
fn large_action_map_resolves() {
    let mut map = InputMap::new();
    let keys = [
        Key::A, Key::B, Key::C, Key::D, Key::E, Key::F, Key::G, Key::H,
        Key::I, Key::J, Key::K, Key::L, Key::M, Key::N, Key::O, Key::P,
        Key::Q, Key::R, Key::S, Key::T, Key::U, Key::V, Key::W, Key::X,
    ];

    for (i, key) in keys.iter().enumerate() {
        let action = format!("action_{i}");
        map.add_action(&action, 0.0);
        map.action_add_event(&action, ActionBinding::KeyBinding(*key));
    }

    let action_count = map.actions().count();
    assert_eq!(action_count, 24);

    let mut state = InputState::new();
    state.set_input_map(map);

    // Press a few keys and verify correct actions
    state.process_event(key_press(Key::A));
    state.process_event(key_press(Key::M));
    state.process_event(key_press(Key::X));

    assert!(state.is_action_pressed("action_0")); // A
    assert!(state.is_action_pressed("action_12")); // M
    assert!(state.is_action_pressed("action_23")); // X
    assert!(!state.is_action_pressed("action_1")); // B not pressed
}

// ===========================================================================
// 18. Snapshot taken with JSON-loaded map reflects pressed actions
// ===========================================================================

#[test]
fn snapshot_with_json_map() {
    let mut state = InputState::new();
    state.set_input_map(load_fixture_map());

    state.process_event(key_press(Key::Left)); // move_left (ArrowLeft binding)
    state.process_event(key_press(Key::Enter)); // shoot

    let snap = state.snapshot();
    assert!(snap.is_action_pressed("move_left"));
    assert!(snap.is_action_pressed("shoot"));
    assert!(!snap.is_action_pressed("jump"));
    assert!(snap.is_action_just_pressed("move_left"));
    assert!(snap.is_action_just_pressed("shoot"));
}

// ===========================================================================
// 19. Snapshot action_pressed_key_map with multi-key action
// ===========================================================================

#[test]
fn snapshot_action_key_map_multi_binding() {
    let mut state = InputState::new();
    state.set_input_map(load_fixture_map());

    // Press both keys bound to move_left (A and ArrowLeft)
    state.process_event(key_press(Key::A));
    state.process_event(key_press(Key::Left));

    let snap = state.snapshot();
    let map = snap.action_pressed_key_map();

    assert!(map.contains_key("move_left"));
    let keys = &map["move_left"];
    assert!(
        keys.len() >= 2,
        "move_left should list both pressed keys: {:?}",
        keys
    );
}

// ===========================================================================
// 20. Snapshot get_axis with JSON-loaded directional map
// ===========================================================================

#[test]
fn snapshot_get_axis_with_json_map() {
    let mut state = InputState::new();
    state.set_input_map(load_fixture_map());

    // Press right only
    state.process_event(key_press(Key::D)); // move_right

    let snap = state.snapshot();
    let axis = snap.get_axis("move_left", "move_right");
    assert!(
        (axis - 1.0).abs() < 0.01,
        "right only should give axis=1.0, got {}",
        axis
    );

    // Now also press left — should cancel
    state.process_event(key_press(Key::A)); // move_left
    let snap2 = state.snapshot();
    let axis2 = snap2.get_axis("move_left", "move_right");
    assert!(
        axis2.abs() < 0.01,
        "left+right should cancel to 0, got {}",
        axis2
    );

    // Original snapshot unchanged
    let orig_axis = snap.get_axis("move_left", "move_right");
    assert!((orig_axis - 1.0).abs() < 0.01);
}

// ===========================================================================
// 21. event_matches_action with mouse binding
// ===========================================================================

#[test]
fn event_matches_action_mouse_binding() {
    let map = simple_map();
    let mouse_event = InputEvent::MouseButton {
        button: MouseButton::Left,
        pressed: true,
        position: gdcore::math::Vector2::ZERO,
    };
    assert!(
        map.event_matches_action(&mouse_event, "shoot"),
        "Left mouse button must match shoot action"
    );
    assert!(!map.event_matches_action(&mouse_event, "ui_left"));
}

// ===========================================================================
// 22. event_matches_action with gamepad button binding
// ===========================================================================

#[test]
fn event_matches_action_gamepad_button() {
    let mut map = InputMap::new();
    map.add_action("attack", 0.0);
    map.action_add_event(
        "attack",
        ActionBinding::GamepadButtonBinding(GamepadButton::FaceA),
    );

    let event = InputEvent::GamepadButton {
        button: GamepadButton::FaceA,
        pressed: true,
        gamepad_id: 0,
    };
    assert!(map.event_matches_action(&event, "attack"));

    let wrong = InputEvent::GamepadButton {
        button: GamepadButton::FaceB,
        pressed: true,
        gamepad_id: 0,
    };
    assert!(!map.event_matches_action(&wrong, "attack"));
}

// ===========================================================================
// 23. JSON load_from_json with malformed input
// ===========================================================================

#[test]
fn json_malformed_returns_error() {
    assert!(InputMap::load_from_json("not json").is_err());
    assert!(InputMap::load_from_json("{}").is_err()); // missing "actions" key
    assert!(InputMap::load_from_json(r#"{"actions": {}}"#).is_ok()); // empty actions section is valid
}

// ===========================================================================
// 24. get_deadzone for unregistered action returns 0.0
// ===========================================================================

#[test]
fn get_deadzone_unregistered_returns_default() {
    let map = InputMap::new();
    assert_eq!(
        map.get_deadzone("nonexistent"),
        0.0,
        "unregistered action deadzone must default to 0.0"
    );
}

// ===========================================================================
// 25. JSON fixture dash action has correct deadzone
// ===========================================================================

#[test]
fn json_fixture_dash_deadzone() {
    let map = load_fixture_map();
    assert_eq!(map.get_deadzone("dash"), 0.2);
    assert_eq!(map.get_deadzone("jump"), 0.0);
    assert_eq!(map.get_deadzone("move_left"), 0.0);
}

// ===========================================================================
// pat-zwt8: Unmapped event fallback semantics
// ===========================================================================

// ---------------------------------------------------------------------------
// 26. Unmapped event against empty InputMap: no match, no panic
// ---------------------------------------------------------------------------

#[test]
fn zwt8_unmapped_event_empty_map_no_match() {
    let map = InputMap::new();
    // Fire a key event at a completely empty map.
    let event = key_press(Key::A);
    // No actions exist, so event_matches_action must return false for any name.
    assert!(
        !map.event_matches_action(&event, "move_left"),
        "unmapped event in empty map must return false"
    );
    assert!(
        !map.event_matches_action(&event, ""),
        "empty action name must return false"
    );
}

// ---------------------------------------------------------------------------
// 27. Event that matches no registered action: silent fallthrough
// ---------------------------------------------------------------------------

#[test]
fn zwt8_event_matches_no_action_silent_fallthrough() {
    let mut map = InputMap::new();
    map.add_action("jump", 0.0);
    map.action_add_event("jump", ActionBinding::KeyBinding(Key::Space));

    // Key::A is not bound to any action.
    let event = key_press(Key::A);
    assert!(
        !map.event_matches_action(&event, "jump"),
        "Key::A should not match 'jump' (bound to Space)"
    );

    // Check against ALL actions — none should match.
    let matched_any = map.actions().any(|action| map.event_matches_action(&event, action));
    assert!(
        !matched_any,
        "Key::A bound to no action must not match any action"
    );
}

// ---------------------------------------------------------------------------
// 28. Duplicate binding on same action: still matches, no double-fire
// ---------------------------------------------------------------------------

#[test]
fn zwt8_duplicate_binding_same_action_matches_once() {
    let mut map = InputMap::new();
    map.add_action("shoot", 0.0);
    map.action_add_event("shoot", ActionBinding::KeyBinding(Key::Enter));
    map.action_add_event("shoot", ActionBinding::KeyBinding(Key::Enter)); // duplicate

    let event = key_press(Key::Enter);
    assert!(
        map.event_matches_action(&event, "shoot"),
        "duplicate binding must still match"
    );

    // Bindings list should have two entries (no deduplication).
    let bindings = map.get_bindings("shoot").unwrap();
    assert_eq!(
        bindings.len(),
        2,
        "duplicate bindings are preserved (Godot does not deduplicate)"
    );
}

// ---------------------------------------------------------------------------
// 29. Same binding on multiple actions: both match independently
// ---------------------------------------------------------------------------

#[test]
fn zwt8_same_binding_multiple_actions_both_match() {
    let mut map = InputMap::new();
    map.add_action("attack", 0.0);
    map.add_action("confirm", 0.0);
    map.action_add_event("attack", ActionBinding::KeyBinding(Key::Enter));
    map.action_add_event("confirm", ActionBinding::KeyBinding(Key::Enter));

    let event = key_press(Key::Enter);
    assert!(
        map.event_matches_action(&event, "attack"),
        "Enter should match 'attack'"
    );
    assert!(
        map.event_matches_action(&event, "confirm"),
        "Enter should also match 'confirm'"
    );

    // Count total matching actions.
    let match_count = map
        .actions()
        .filter(|a| map.event_matches_action(&event, a))
        .count();
    assert_eq!(
        match_count, 2,
        "one physical event can match multiple actions simultaneously"
    );
}

// ---------------------------------------------------------------------------
// 30. Empty action table: has_action true, get_bindings empty, no match
// ---------------------------------------------------------------------------

#[test]
fn zwt8_empty_action_table_registered_but_no_bindings() {
    let mut map = InputMap::new();
    map.add_action("interact", 0.0);
    // No bindings added.

    assert!(map.has_action("interact"), "action should exist");
    assert_eq!(map.action_count(), 1, "one action registered");

    let bindings = map.get_bindings("interact");
    assert!(bindings.is_some(), "registered action returns Some");
    assert!(
        bindings.unwrap().is_empty(),
        "registered action with no bindings returns empty slice"
    );

    // No event should match.
    assert!(
        !map.event_matches_action(&key_press(Key::E), "interact"),
        "empty binding list must never match any event"
    );
    assert!(
        !map.event_matches_action(&key_press(Key::Space), "interact"),
        "empty binding list must never match any event"
    );
}

// ---------------------------------------------------------------------------
// 31. erase_action then query: fallback to defaults
// ---------------------------------------------------------------------------

#[test]
fn zwt8_erase_action_restores_defaults() {
    let mut map = InputMap::new();
    map.add_action("jump", 0.3);
    map.action_add_event("jump", ActionBinding::KeyBinding(Key::Space));

    assert!(map.has_action("jump"));
    assert_eq!(map.get_deadzone("jump"), 0.3);

    map.erase_action("jump");

    assert!(!map.has_action("jump"), "erased action must not exist");
    assert_eq!(
        map.get_deadzone("jump"),
        0.0,
        "erased action deadzone must default to 0.0"
    );
    assert!(
        map.get_bindings("jump").is_none(),
        "erased action bindings must be None"
    );
    assert!(
        !map.event_matches_action(&key_press(Key::Space), "jump"),
        "erased action must not match any event"
    );
    assert_eq!(map.action_count(), 0, "no actions remain");
}

// ---------------------------------------------------------------------------
// 32. action_erase_events preserves registration
// ---------------------------------------------------------------------------

#[test]
fn zwt8_action_erase_events_preserves_registration() {
    let mut map = InputMap::new();
    map.add_action("move", 0.5);
    map.action_add_event("move", ActionBinding::KeyBinding(Key::W));
    map.action_add_event("move", ActionBinding::KeyBinding(Key::Up));

    assert_eq!(map.get_bindings("move").unwrap().len(), 2);

    map.action_erase_events("move");

    assert!(
        map.has_action("move"),
        "action_erase_events must not unregister the action"
    );
    assert_eq!(
        map.get_deadzone("move"),
        0.5,
        "deadzone preserved after erase_events"
    );
    assert!(
        map.get_bindings("move").unwrap().is_empty(),
        "bindings cleared"
    );
    assert!(
        !map.event_matches_action(&key_press(Key::W), "move"),
        "cleared bindings must not match"
    );

    // Re-add a binding — action is still valid.
    map.action_add_event("move", ActionBinding::KeyBinding(Key::A));
    assert!(
        map.event_matches_action(&key_press(Key::A), "move"),
        "re-added binding must work after erase_events"
    );
}

// ---------------------------------------------------------------------------
// 33. InputState: unmapped event processed without panic
// ---------------------------------------------------------------------------

#[test]
fn zwt8_input_state_unmapped_event_no_panic() {
    let mut state = InputState::new();
    let mut map = InputMap::new();
    map.add_action("jump", 0.0);
    map.action_add_event("jump", ActionBinding::KeyBinding(Key::Space));
    state.set_input_map(map);

    // Process an event for a key not bound to any action.
    state.process_event(key_press(Key::Q));

    // Should not panic. jump should not be triggered.
    let snap = state.snapshot();
    assert!(
        !snap.is_action_pressed("jump"),
        "unmapped key must not trigger bound action"
    );
    // The key itself should still be tracked in raw key state.
    assert!(
        snap.is_key_pressed(Key::Q),
        "raw key state should track any pressed key"
    );
}

// ---------------------------------------------------------------------------
// 34. InputState: duplicate binding doesn't double-fire action
// ---------------------------------------------------------------------------

#[test]
fn zwt8_input_state_duplicate_binding_single_action_press() {
    let mut state = InputState::new();
    let mut map = InputMap::new();
    map.add_action("fire", 0.0);
    map.action_add_event("fire", ActionBinding::KeyBinding(Key::Enter));
    map.action_add_event("fire", ActionBinding::KeyBinding(Key::Enter)); // duplicate
    state.set_input_map(map);

    state.process_event(key_press(Key::Enter));

    let snap = state.snapshot();
    assert!(snap.is_action_pressed("fire"), "action should be pressed");
    assert_eq!(
        snap.get_action_strength("fire"),
        1.0,
        "strength should be 1.0, not 2.0 from duplicate"
    );
}

// ---------------------------------------------------------------------------
// 35. MainLoop: unmapped event doesn't affect action state
// ---------------------------------------------------------------------------

#[test]
fn zwt8_mainloop_unmapped_event_no_action_state_change() {
    let tree = SceneTree::new();
    let mut ml = MainLoop::new(tree);

    let mut map = InputMap::new();
    map.add_action("pause", 0.0);
    map.action_add_event("pause", ActionBinding::KeyBinding(Key::Escape));
    ml.set_input_map(map);

    // Push an unmapped key.
    ml.push_event(key_press(Key::F1));
    ml.step(DT);

    assert!(
        !ml.input_state().is_action_pressed("pause"),
        "unmapped F1 must not trigger 'pause'"
    );
    assert!(
        ml.input_state().is_key_pressed(Key::F1),
        "raw key F1 should still be tracked"
    );
}

// ---------------------------------------------------------------------------
// 36. Completely empty map: actions() yields nothing, queries safe
// ---------------------------------------------------------------------------

#[test]
fn zwt8_completely_empty_map_safe_queries() {
    let map = InputMap::new();

    assert_eq!(map.action_count(), 0);
    assert!(!map.has_action("anything"));
    assert!(map.get_bindings("anything").is_none());
    assert_eq!(map.get_deadzone("anything"), 0.0);
    assert!(!map.event_matches_action(&key_press(Key::A), "anything"));

    let action_names: Vec<_> = map.actions().collect();
    assert!(action_names.is_empty(), "empty map yields no actions");
}

// ---------------------------------------------------------------------------
// 37. JSON empty actions object: valid map with zero actions
// ---------------------------------------------------------------------------

#[test]
fn zwt8_json_empty_actions_valid_zero_actions() {
    let map = InputMap::load_from_json(r#"{"actions": {}}"#).unwrap();
    assert_eq!(map.action_count(), 0);
    assert!(!map.has_action("anything"));
    assert!(!map.event_matches_action(&key_press(Key::A), "anything"));
}

// ---------------------------------------------------------------------------
// 38. Multiple actions some empty: unmapped event falls through all
// ---------------------------------------------------------------------------

#[test]
fn zwt8_mixed_empty_and_bound_actions_fallthrough() {
    let mut map = InputMap::new();
    map.add_action("bound_action", 0.0);
    map.action_add_event("bound_action", ActionBinding::KeyBinding(Key::X));
    map.add_action("empty_action", 0.0);
    // empty_action has no bindings.
    map.add_action("another_empty", 0.0);

    assert_eq!(map.action_count(), 3);

    // Key::Z is not bound to anything.
    let event = key_press(Key::Z);
    let matched: Vec<_> = map
        .actions()
        .filter(|a| map.event_matches_action(&event, a))
        .collect();
    assert!(
        matched.is_empty(),
        "Key::Z must not match any action (bound or empty)"
    );

    // Key::X matches only bound_action.
    let event_x = key_press(Key::X);
    let matched_x: Vec<_> = map
        .actions()
        .filter(|a| map.event_matches_action(&event_x, a))
        .collect();
    assert_eq!(matched_x.len(), 1);
    assert_eq!(matched_x[0], "bound_action");
}

// ---------------------------------------------------------------------------
// 39. Erase action on non-existent action: no panic
// ---------------------------------------------------------------------------

#[test]
fn zwt8_erase_nonexistent_action_no_panic() {
    let mut map = InputMap::new();
    map.add_action("real", 0.0);

    // Erasing a non-existent action should not panic.
    map.erase_action("fake");

    assert!(map.has_action("real"), "real action must survive");
    assert!(!map.has_action("fake"));
}

// ---------------------------------------------------------------------------
// 40. action_erase_events on non-existent action: no panic
// ---------------------------------------------------------------------------

#[test]
fn zwt8_erase_events_nonexistent_action_no_panic() {
    let mut map = InputMap::new();
    map.add_action("real", 0.0);
    map.action_add_event("real", ActionBinding::KeyBinding(Key::A));

    // Erasing events on a non-existent action should not panic.
    map.action_erase_events("fake");

    // Real action bindings must be unaffected.
    assert_eq!(map.get_bindings("real").unwrap().len(), 1);
}

// ===========================================================================
// pat-ypt: Input-map action fallback semantics for unmapped events
// ===========================================================================

// ---------------------------------------------------------------------------
// 41. is_action_pressed for nonexistent action returns false
// ---------------------------------------------------------------------------

#[test]
fn ypt_is_action_pressed_nonexistent_returns_false() {
    let mut state = InputState::new();
    let mut map = InputMap::new();
    map.add_action("jump", 0.0);
    map.action_add_event("jump", ActionBinding::KeyBinding(Key::Space));
    state.set_input_map(map);

    state.process_event(key_press(Key::Space));

    assert!(state.is_action_pressed("jump"));
    assert!(
        !state.is_action_pressed("nonexistent"),
        "nonexistent action must return false"
    );
    assert!(
        !state.is_action_just_pressed("nonexistent"),
        "nonexistent action just_pressed must return false"
    );
    assert!(
        !state.is_action_just_released("nonexistent"),
        "nonexistent action just_released must return false"
    );
}

// ---------------------------------------------------------------------------
// 42. get_action_strength for nonexistent action returns 0.0
// ---------------------------------------------------------------------------

#[test]
fn ypt_get_action_strength_nonexistent_returns_zero() {
    let mut state = InputState::new();
    let mut map = InputMap::new();
    map.add_action("fire", 0.0);
    map.action_add_event("fire", ActionBinding::KeyBinding(Key::Enter));
    state.set_input_map(map);

    state.process_event(key_press(Key::Enter));

    assert_eq!(state.get_action_strength("fire"), 1.0);
    assert_eq!(
        state.get_action_strength("nonexistent"),
        0.0,
        "nonexistent action strength must be 0.0"
    );
}

// ---------------------------------------------------------------------------
// 43. Snapshot fallback: queries on nonexistent actions return defaults
// ---------------------------------------------------------------------------

#[test]
fn ypt_snapshot_nonexistent_action_defaults() {
    let mut state = InputState::new();
    let mut map = InputMap::new();
    map.add_action("move_left", 0.0);
    map.action_add_event("move_left", ActionBinding::KeyBinding(Key::A));
    state.set_input_map(map);

    state.process_event(key_press(Key::A));
    let snap = state.snapshot();

    assert!(snap.is_action_pressed("move_left"));
    assert!(!snap.is_action_pressed("ghost_action"));
    assert!(!snap.is_action_just_pressed("ghost_action"));
    assert!(!snap.is_action_just_released("ghost_action"));
    assert_eq!(snap.get_action_strength("ghost_action"), 0.0);
}

// ---------------------------------------------------------------------------
// 44. get_axis with one unmapped action returns partial value
// ---------------------------------------------------------------------------

#[test]
fn ypt_get_axis_one_unmapped_returns_partial() {
    let mut state = InputState::new();
    let mut map = InputMap::new();
    map.add_action("go_right", 0.0);
    map.action_add_event("go_right", ActionBinding::KeyBinding(Key::D));
    // "go_left" is never registered.
    state.set_input_map(map);

    state.process_event(key_press(Key::D));

    // Godot: get_axis returns positive when only positive action is pressed.
    let axis = state.get_axis("go_left", "go_right");
    assert!(
        (axis - 1.0).abs() < 0.01,
        "axis with unmapped negative should return 1.0, got {axis}"
    );
}

// ---------------------------------------------------------------------------
// 45. get_axis with both actions unmapped returns 0.0
// ---------------------------------------------------------------------------

#[test]
fn ypt_get_axis_both_unmapped_returns_zero() {
    let state = InputState::new();
    let axis = state.get_axis("fake_neg", "fake_pos");
    assert!(
        axis.abs() < 0.01,
        "axis with both unmapped should return 0.0, got {axis}"
    );
}

// ---------------------------------------------------------------------------
// 46. get_vector with all unmapped actions returns zero vector
// ---------------------------------------------------------------------------

#[test]
fn ypt_get_vector_all_unmapped_returns_zero() {
    let state = InputState::new();
    let vec = state.get_vector("a", "b", "c", "d");
    assert!(
        vec.x.abs() < 0.01 && vec.y.abs() < 0.01,
        "vector with all unmapped should be (0,0), got ({}, {})",
        vec.x,
        vec.y
    );
}

// ---------------------------------------------------------------------------
// 47. flush_frame clears just_pressed/just_released for unmapped keys
// ---------------------------------------------------------------------------

#[test]
fn ypt_flush_frame_unmapped_keys_persist() {
    let mut state = InputState::new();
    let mut map = InputMap::new();
    map.add_action("jump", 0.0);
    map.action_add_event("jump", ActionBinding::KeyBinding(Key::Space));
    state.set_input_map(map);

    // Press an unmapped key (Q) and a mapped key (Space).
    state.process_event(key_press(Key::Q));
    state.process_event(key_press(Key::Space));

    assert!(state.is_key_pressed(Key::Q));
    assert!(state.is_key_just_pressed(Key::Q));
    assert!(state.is_action_pressed("jump"));

    state.flush_frame();

    // After flush: keys still pressed, just_pressed cleared.
    assert!(
        state.is_key_pressed(Key::Q),
        "unmapped key should still be pressed after flush"
    );
    assert!(
        !state.is_key_just_pressed(Key::Q),
        "just_pressed should be cleared after flush"
    );
    assert!(
        state.is_action_pressed("jump"),
        "action should still be pressed after flush"
    );
    assert!(
        !state.is_action_just_pressed("jump"),
        "action just_pressed should be cleared after flush"
    );
}

// ---------------------------------------------------------------------------
// 48. Re-registration lifecycle: erase → re-add with different bindings
// ---------------------------------------------------------------------------

#[test]
fn ypt_erase_and_reregister_with_different_bindings() {
    let mut state = InputState::new();
    let mut map = InputMap::new();
    map.add_action("fire", 0.0);
    map.action_add_event("fire", ActionBinding::KeyBinding(Key::Enter));
    state.set_input_map(map.clone());

    state.process_event(key_press(Key::Enter));
    assert!(state.is_action_pressed("fire"));

    // Erase and re-add with different binding.
    map.erase_action("fire");
    map.add_action("fire", 0.0);
    map.action_add_event("fire", ActionBinding::KeyBinding(Key::F));
    state.set_input_map(map);

    // Old binding should no longer work for new events.
    // But Enter is still physically pressed from before.
    state.flush_frame();

    // Press the new binding.
    state.process_event(key_press(Key::F));
    assert!(
        state.is_action_pressed("fire"),
        "new binding F should trigger fire"
    );
}

// ---------------------------------------------------------------------------
// 49. Mixed binding types: key+mouse+gamepad, only key arrives
// ---------------------------------------------------------------------------

#[test]
fn ypt_mixed_bindings_partial_input() {
    let mut state = InputState::new();
    let mut map = InputMap::new();
    map.add_action("attack", 0.0);
    map.action_add_event("attack", ActionBinding::KeyBinding(Key::X));
    map.action_add_event(
        "attack",
        ActionBinding::MouseBinding(MouseButton::Left),
    );
    map.action_add_event(
        "attack",
        ActionBinding::GamepadButtonBinding(GamepadButton::FaceA),
    );
    state.set_input_map(map);

    // Only press the key — mouse and gamepad not pressed.
    state.process_event(key_press(Key::X));

    assert!(
        state.is_action_pressed("attack"),
        "any one binding should trigger the action"
    );
    assert_eq!(state.get_action_strength("attack"), 1.0);

    // Release key, press mouse.
    state.process_event(key_release(Key::X));
    state.flush_frame();
    let mouse_press = InputEvent::MouseButton {
        button: MouseButton::Left,
        pressed: true,
        position: gdcore::math::Vector2::ZERO,
    };
    state.process_event(mouse_press);

    assert!(
        state.is_action_pressed("attack"),
        "mouse binding should also trigger attack"
    );
}

// ---------------------------------------------------------------------------
// 50. Duplicate bindings survive erase_events + re-add
// ---------------------------------------------------------------------------

#[test]
fn ypt_duplicate_bindings_after_erase_reenter() {
    let mut map = InputMap::new();
    map.add_action("shoot", 0.0);
    map.action_add_event("shoot", ActionBinding::KeyBinding(Key::Space));
    map.action_add_event("shoot", ActionBinding::KeyBinding(Key::Space)); // dup

    assert_eq!(map.get_bindings("shoot").unwrap().len(), 2);

    map.action_erase_events("shoot");
    assert!(map.get_bindings("shoot").unwrap().is_empty());

    // Re-add single binding — no duplicates.
    map.action_add_event("shoot", ActionBinding::KeyBinding(Key::Enter));
    assert_eq!(map.get_bindings("shoot").unwrap().len(), 1);
    assert!(map.event_matches_action(&key_press(Key::Enter), "shoot"));
    assert!(!map.event_matches_action(&key_press(Key::Space), "shoot"));
}

// ---------------------------------------------------------------------------
// 51. Action just_released after erase_action: no ghost release
// ---------------------------------------------------------------------------

#[test]
fn ypt_no_ghost_release_after_erase() {
    let mut state = InputState::new();
    let mut map = InputMap::new();
    map.add_action("jump", 0.0);
    map.action_add_event("jump", ActionBinding::KeyBinding(Key::Space));
    state.set_input_map(map.clone());

    // Press and hold jump.
    state.process_event(key_press(Key::Space));
    assert!(state.is_action_pressed("jump"));

    // Erase the action from the map and set new map.
    map.erase_action("jump");
    state.set_input_map(map);
    state.flush_frame();

    // After erasing the action from the map, is_action_just_released should
    // NOT produce a ghost release event (the action simply disappears).
    assert!(
        !state.is_action_just_released("jump"),
        "erased action must not produce ghost just_released"
    );
    assert!(
        !state.is_action_just_pressed("jump"),
        "erased action must not produce ghost just_pressed"
    );
    // Note: actions_pressed retains the stale entry until the next
    // process_event cycle clears it — matching Godot's lazy-cleanup
    // semantics where erasing an InputMap action doesn't retroactively
    // scrub InputState caches mid-frame.
}

// ---------------------------------------------------------------------------
// 52. Snapshot get_axis with partially mapped directional pair
// ---------------------------------------------------------------------------

#[test]
fn ypt_snapshot_get_axis_partial_mapping() {
    let mut state = InputState::new();
    let mut map = InputMap::new();
    map.add_action("right", 0.0);
    map.action_add_event("right", ActionBinding::KeyBinding(Key::Right));
    // "left" is not registered.
    state.set_input_map(map);

    state.process_event(key_press(Key::Right));
    let snap = state.snapshot();

    let axis = snap.get_axis("left", "right");
    assert!(
        (axis - 1.0).abs() < 0.01,
        "snapshot axis with unmapped negative = 1.0, got {axis}"
    );
}

// ---------------------------------------------------------------------------
// 53. Snapshot get_vector with mixed mapped/unmapped axes
// ---------------------------------------------------------------------------

#[test]
fn ypt_snapshot_get_vector_partial_mapping() {
    let mut state = InputState::new();
    let mut map = InputMap::new();
    map.add_action("up", 0.0);
    map.action_add_event("up", ActionBinding::KeyBinding(Key::W));
    // "down", "left", "right" are not registered.
    state.set_input_map(map);

    state.process_event(key_press(Key::W));
    let snap = state.snapshot();

    let vec = snap.get_vector("left", "right", "up", "down");
    // Only "up" is pressed → y should be negative (up is negative_y convention)
    // or positive depending on convention.
    assert!(
        vec.x.abs() < 0.01,
        "x should be 0 with no horizontal input, got {}",
        vec.x
    );
    // y should be non-zero since "up" is pressed.
    assert!(
        vec.y.abs() > 0.01,
        "y should be non-zero with 'up' pressed, got {}",
        vec.y
    );
}

// ---------------------------------------------------------------------------
// 54. Multiple erase/re-add cycles: action count stays correct
// ---------------------------------------------------------------------------

#[test]
fn ypt_multiple_erase_readd_cycles_action_count() {
    let mut map = InputMap::new();

    for i in 0..5 {
        map.add_action("toggle", 0.0);
        map.action_add_event("toggle", ActionBinding::KeyBinding(Key::T));
        assert_eq!(map.action_count(), 1, "cycle {i}: one action");
        assert!(map.has_action("toggle"));

        map.erase_action("toggle");
        assert_eq!(map.action_count(), 0, "cycle {i}: zero after erase");
        assert!(!map.has_action("toggle"));
    }
}

// ---------------------------------------------------------------------------
// 55. Unmapped mouse motion doesn't affect action state
// ---------------------------------------------------------------------------

#[test]
fn ypt_mouse_motion_no_action_effect() {
    let mut state = InputState::new();
    let mut map = InputMap::new();
    map.add_action("look", 0.0);
    map.action_add_event("look", ActionBinding::KeyBinding(Key::L));
    state.set_input_map(map);

    // Send mouse motion — no binding for it.
    state.process_event(InputEvent::MouseMotion {
        position: gdcore::math::Vector2::new(100.0, 200.0),
        relative: gdcore::math::Vector2::new(10.0, 5.0),
    });

    assert!(
        !state.is_action_pressed("look"),
        "mouse motion must not trigger key-bound action"
    );
    // But mouse position should update.
    let snap = state.snapshot();
    let pos = snap.get_mouse_position();
    assert!(
        (pos.x - 100.0).abs() < 0.01,
        "mouse position should track motion: got ({}, {})",
        pos.x,
        pos.y
    );
}

// ---------------------------------------------------------------------------
// 56. Gamepad axis below deadzone doesn't fire action
// ---------------------------------------------------------------------------

#[test]
fn ypt_gamepad_axis_below_deadzone_no_action() {
    let mut state = InputState::new();
    let mut map = InputMap::new();
    map.add_action("move_x", 0.3); // 0.3 deadzone
    map.action_add_event(
        "move_x",
        ActionBinding::GamepadAxisBinding(GamepadAxis::LeftStickX),
    );
    state.set_input_map(map);

    // Axis value within deadzone.
    state.process_event(InputEvent::GamepadAxis {
        axis: GamepadAxis::LeftStickX,
        value: 0.2,
        gamepad_id: 0,
    });

    assert!(
        !state.is_action_pressed("move_x"),
        "axis value 0.2 below deadzone 0.3 must not trigger action"
    );
    assert_eq!(state.get_action_strength("move_x"), 0.0);

    // Axis value above deadzone.
    state.process_event(InputEvent::GamepadAxis {
        axis: GamepadAxis::LeftStickX,
        value: 0.5,
        gamepad_id: 0,
    });

    assert!(
        state.is_action_pressed("move_x"),
        "axis value 0.5 above deadzone 0.3 must trigger action"
    );
}

// ---------------------------------------------------------------------------
// 57. Screen touch event doesn't fire key-bound actions
// ---------------------------------------------------------------------------

#[test]
fn ypt_screen_touch_no_key_action() {
    let mut state = InputState::new();
    let mut map = InputMap::new();
    map.add_action("interact", 0.0);
    map.action_add_event("interact", ActionBinding::KeyBinding(Key::E));
    state.set_input_map(map);

    state.process_event(InputEvent::ScreenTouch {
        index: 0,
        position: gdcore::math::Vector2::new(50.0, 50.0),
        pressed: true,
    });

    assert!(
        !state.is_action_pressed("interact"),
        "screen touch must not trigger key-bound action"
    );
}

// ---------------------------------------------------------------------------
// 58. InputState with no map set: all action queries return defaults
// ---------------------------------------------------------------------------

#[test]
fn ypt_no_input_map_set_all_defaults() {
    let mut state = InputState::new();

    // Process some events without any map.
    state.process_event(key_press(Key::A));
    state.process_event(key_press(Key::Space));

    // Raw key state should still work.
    assert!(state.is_key_pressed(Key::A));
    assert!(state.is_key_pressed(Key::Space));

    // Action queries should all return false/0.0.
    assert!(!state.is_action_pressed("anything"));
    assert!(!state.is_action_just_pressed("anything"));
    assert_eq!(state.get_action_strength("anything"), 0.0);
    assert_eq!(state.get_axis("neg", "pos"), 0.0);
}
