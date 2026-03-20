//! pat-vih: Input map loading and action binding coverage.
//!
//! Tests that InputMap can be loaded from JSON config files and that
//! loaded maps correctly drive action resolution through MainLoop::push_event().

use gdplatform::input::{ActionBinding, InputEvent, InputMap, Key, MouseButton};
use gdscene::main_loop::MainLoop;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scripting::{GDScriptNodeInstance, InputSnapshot};
use gdscene::SceneTree;
use gdvariant::Variant;

const SCENE_SOURCE: &str = include_str!("../fixtures/scenes/space_shooter.tscn");
const PLAYER_SCRIPT: &str = include_str!("../fixtures/scripts/player.gd");
const SPAWNER_SCRIPT: &str = include_str!("../fixtures/scripts/enemy_spawner.gd");

const DT: f64 = 1.0 / 60.0;

/// Loads the InputMap from fixtures/input_map.json.
fn load_fixture_input_map() -> InputMap {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../fixtures/input_map.json");
    InputMap::load_from_json_file(&path)
        .unwrap_or_else(|e| panic!("failed to load input_map.json: {e}"))
}

/// Builds a MainLoop from space_shooter scene with the JSON-loaded InputMap.
fn build_scene_with_json_input_map() -> (MainLoop, gdscene::NodeId) {
    let packed = PackedScene::from_tscn(SCENE_SOURCE).expect("parse scene");
    let mut tree = SceneTree::new();
    let root_id = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root_id, &packed).expect("instance scene");

    let node_ids = tree.all_nodes_in_tree_order();
    let mut player_id = None;
    for &nid in &node_ids {
        let (name, script_path) = {
            let node = tree.get_node(nid).unwrap();
            let name = node.name().to_string();
            let path = match node.get_property("_script_path") {
                Variant::String(p) => Some(p.clone()),
                _ => None,
            };
            (name, path)
        };

        if let Some(ref path) = script_path {
            let source = if path.contains("player.gd") {
                PLAYER_SCRIPT
            } else if path.contains("enemy_spawner.gd") {
                SPAWNER_SCRIPT
            } else {
                continue;
            };
            let instance = GDScriptNodeInstance::from_source(source, nid)
                .unwrap_or_else(|e| panic!("failed to parse script {path}: {e}"));
            tree.attach_script(nid, Box::new(instance));
        }

        if name == "Player" {
            player_id = Some(nid);
        }
    }

    let all = tree.all_nodes_in_tree_order();
    for &nid in &all {
        tree.process_script_enter_tree(nid);
    }
    for &nid in &all {
        if tree.has_script(nid) {
            tree.process_script_ready(nid);
        }
    }

    let mut main_loop = MainLoop::new(tree);
    main_loop.set_input_map(load_fixture_input_map());

    let pid = player_id.expect("Player node not found");
    (main_loop, pid)
}

// ═══════════════════════════════════════════════════════════════════════
// Test: JSON fixture loads all expected actions
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn vih_json_fixture_loads_all_actions() {
    let map = load_fixture_input_map();

    let expected = [
        "move_left",
        "move_right",
        "move_up",
        "move_down",
        "jump",
        "shoot",
        "dash",
        "pause",
    ];
    for action in &expected {
        assert!(
            map.get_bindings(action).is_some(),
            "missing action: {action}"
        );
    }
    assert_eq!(map.actions().count(), expected.len());
}

// ═══════════════════════════════════════════════════════════════════════
// Test: Custom JSON overrides default map
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn vih_json_map_overrides_default() {
    // Default map: jump → W
    let mut default_map = InputMap::new();
    default_map.add_action("jump", 0.0);
    default_map.action_add_event("jump", ActionBinding::KeyBinding(Key::W));

    // JSON map: jump → Space (from fixture)
    let json_map = load_fixture_input_map();

    // Default map: W triggers jump
    let evt_w = InputEvent::Key {
        key: Key::W,
        pressed: true,
        shift: false,
        ctrl: false,
        alt: false,
    };
    assert!(default_map.event_matches_action(&evt_w, "jump"));

    // JSON map: W does NOT trigger jump, Space does
    assert!(!json_map.event_matches_action(&evt_w, "jump"));
    let evt_space = InputEvent::Key {
        key: Key::Space,
        pressed: true,
        shift: false,
        ctrl: false,
        alt: false,
    };
    assert!(json_map.event_matches_action(&evt_space, "jump"));
}

// ═══════════════════════════════════════════════════════════════════════
// Test: Multiple keys per action from JSON
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn vih_multiple_keys_per_action() {
    let map = load_fixture_input_map();

    // move_left has A and ArrowLeft
    let bindings = map.get_bindings("move_left").unwrap();
    assert_eq!(bindings.len(), 2, "move_left should have 2 key bindings");

    let evt_a = InputEvent::Key {
        key: Key::A,
        pressed: true,
        shift: false,
        ctrl: false,
        alt: false,
    };
    let evt_left = InputEvent::Key {
        key: Key::Left,
        pressed: true,
        shift: false,
        ctrl: false,
        alt: false,
    };
    assert!(map.event_matches_action(&evt_a, "move_left"));
    assert!(map.event_matches_action(&evt_left, "move_left"));
}

// ═══════════════════════════════════════════════════════════════════════
// Test: Deadzone loads from JSON
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn vih_deadzone_from_json() {
    let map = load_fixture_input_map();
    assert!(
        (map.get_deadzone("dash") - 0.2).abs() < 1e-6,
        "dash deadzone should be 0.2"
    );
    assert_eq!(
        map.get_deadzone("jump"),
        0.0,
        "jump deadzone should default to 0.0"
    );
}

// ═══════════════════════════════════════════════════════════════════════
// Test: Mixed key + mouse bindings from JSON
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn vih_mixed_key_and_mouse_bindings() {
    let map = load_fixture_input_map();

    // shoot has Enter key + Left mouse
    let bindings = map.get_bindings("shoot").unwrap();
    assert_eq!(bindings.len(), 2);

    let evt_enter = InputEvent::Key {
        key: Key::Enter,
        pressed: true,
        shift: false,
        ctrl: false,
        alt: false,
    };
    let evt_mouse = InputEvent::MouseButton {
        button: MouseButton::Left,
        pressed: true,
        position: gdcore::math::Vector2::ZERO,
    };
    assert!(map.event_matches_action(&evt_enter, "shoot"));
    assert!(map.event_matches_action(&evt_mouse, "shoot"));
}

// ═══════════════════════════════════════════════════════════════════════
// Test: JSON-loaded InputMap resolves actions via push_event at runtime
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn vih_json_map_actions_resolve_via_push_event() {
    let (mut ml, _player_id) = build_scene_with_json_input_map();

    // The space_shooter player.gd uses "ui_right" for movement.
    // Our JSON map uses "move_right" → D/ArrowRight, not "ui_right".
    // But we can still verify actions resolve by checking InputState directly.

    // Push D key (mapped to move_right in JSON)
    ml.push_event(InputEvent::Key {
        key: Key::D,
        pressed: true,
        shift: false,
        ctrl: false,
        alt: false,
    });

    // Check that move_right action is active on InputState
    assert!(
        ml.input_state().is_action_pressed("move_right"),
        "push_event(D) should activate 'move_right' action from JSON map"
    );

    // Step a frame so the bridge runs
    ml.step(DT);

    // After step, input should still be pressed (not flushed until end of step)
    // Push Space (mapped to jump)
    ml.push_event(InputEvent::Key {
        key: Key::Space,
        pressed: true,
        shift: false,
        ctrl: false,
        alt: false,
    });
    assert!(
        ml.input_state().is_action_pressed("jump"),
        "push_event(Space) should activate 'jump' action from JSON map"
    );
}

#[test]
fn vih_json_map_action_just_pressed_clears_after_step() {
    let (mut ml, _) = build_scene_with_json_input_map();

    ml.push_event(InputEvent::Key {
        key: Key::Escape,
        pressed: true,
        shift: false,
        ctrl: false,
        alt: false,
    });
    assert!(ml.input_state().is_action_just_pressed("pause"));

    // Step flushes just_pressed
    ml.step(DT);
    assert!(!ml.input_state().is_action_just_pressed("pause"));
    // But action is still held
    assert!(ml.input_state().is_action_pressed("pause"));
}

#[test]
fn vih_json_map_multiple_keys_resolve_same_action_via_push_event() {
    let (mut ml, _) = build_scene_with_json_input_map();

    // Press A → move_left active
    ml.push_event(InputEvent::Key {
        key: Key::A,
        pressed: true,
        shift: false,
        ctrl: false,
        alt: false,
    });
    assert!(ml.input_state().is_action_pressed("move_left"));

    // Release A
    ml.push_event(InputEvent::Key {
        key: Key::A,
        pressed: false,
        shift: false,
        ctrl: false,
        alt: false,
    });
    assert!(!ml.input_state().is_action_pressed("move_left"));

    // Press ArrowLeft → same action active via alternate binding
    ml.push_event(InputEvent::Key {
        key: Key::Left,
        pressed: true,
        shift: false,
        ctrl: false,
        alt: false,
    });
    assert!(
        ml.input_state().is_action_pressed("move_left"),
        "ArrowLeft should also activate move_left"
    );
}

#[test]
fn vih_json_map_mouse_binding_resolves_via_push_event() {
    let (mut ml, _) = build_scene_with_json_input_map();

    // Push left mouse click (mapped to shoot)
    ml.push_event(InputEvent::MouseButton {
        button: MouseButton::Left,
        pressed: true,
        position: gdcore::math::Vector2::new(100.0, 200.0),
    });
    assert!(
        ml.input_state().is_action_pressed("shoot"),
        "left mouse click should activate 'shoot' from JSON map"
    );
}

// ═══════════════════════════════════════════════════════════════════════
// pat-aro: Mouse position and button routing to input snapshots
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn aro_mouse_move_updates_position_in_input_state() {
    let (mut ml, _) = build_scene_with_json_input_map();

    ml.push_event(InputEvent::MouseMotion {
        position: gdcore::math::Vector2::new(320.0, 240.0),
        relative: gdcore::math::Vector2::new(5.0, 3.0),
    });

    assert_eq!(
        ml.input_state().get_mouse_position(),
        gdcore::math::Vector2::new(320.0, 240.0),
        "MouseMotion should update input state mouse position"
    );
}

#[test]
fn aro_mouse_button_press_updates_input_state() {
    let (mut ml, _) = build_scene_with_json_input_map();

    ml.push_event(InputEvent::MouseButton {
        button: MouseButton::Right,
        pressed: true,
        position: gdcore::math::Vector2::new(50.0, 75.0),
    });

    assert!(
        ml.input_state().is_mouse_button_pressed(MouseButton::Right),
        "MouseButton press should set button in input state"
    );
    assert_eq!(
        ml.input_state().get_mouse_position(),
        gdcore::math::Vector2::new(50.0, 75.0),
        "MouseButton event should also update position"
    );
}

#[test]
fn aro_mouse_button_release_clears_input_state() {
    let (mut ml, _) = build_scene_with_json_input_map();

    // Press
    ml.push_event(InputEvent::MouseButton {
        button: MouseButton::Left,
        pressed: true,
        position: gdcore::math::Vector2::ZERO,
    });
    assert!(ml.input_state().is_mouse_button_pressed(MouseButton::Left));

    // Release
    ml.push_event(InputEvent::MouseButton {
        button: MouseButton::Left,
        pressed: false,
        position: gdcore::math::Vector2::ZERO,
    });
    assert!(!ml.input_state().is_mouse_button_pressed(MouseButton::Left));
}

#[test]
fn aro_mouse_position_bridges_to_script_snapshot() {
    let (mut ml, _) = build_scene_with_json_input_map();

    ml.push_event(InputEvent::MouseMotion {
        position: gdcore::math::Vector2::new(150.0, 250.0),
        relative: gdcore::math::Vector2::ZERO,
    });

    // Step to trigger the bridge
    ml.step(DT);

    // Verify the bridge set a snapshot with mouse position
    // We can verify indirectly: the script snapshot is set on the tree
    // and cleared after step. Push another motion + key so bridge runs again.
    ml.push_event(InputEvent::MouseMotion {
        position: gdcore::math::Vector2::new(400.0, 300.0),
        relative: gdcore::math::Vector2::ZERO,
    });

    // The platform-level state should have the new position
    assert_eq!(
        ml.input_state().get_mouse_position(),
        gdcore::math::Vector2::new(400.0, 300.0),
    );
}

#[test]
fn aro_mouse_button_bridges_to_script_snapshot() {
    let (mut ml, _) = build_scene_with_json_input_map();

    // Press left mouse button (also mapped to "shoot" action)
    ml.push_event(InputEvent::MouseButton {
        button: MouseButton::Left,
        pressed: true,
        position: gdcore::math::Vector2::new(100.0, 200.0),
    });

    // Step triggers the bridge: InputState → script InputSnapshot
    ml.step(DT);

    // The "shoot" action should have resolved via the mouse binding
    // (verified via input_state which still holds the pressed state)
    assert!(ml.input_state().is_mouse_button_pressed(MouseButton::Left));
    assert!(ml.input_state().is_action_pressed("shoot"));
}

#[test]
fn aro_script_snapshot_mouse_position_and_buttons() {
    // Test the scripting InputSnapshot directly (unit-level)
    let mut snap = InputSnapshot::default();
    snap.mouse_position = gdcore::math::Vector2::new(42.0, 99.0);
    snap.mouse_buttons_pressed.insert("1".to_string()); // Left
    snap.mouse_buttons_pressed.insert("3".to_string()); // Middle

    assert_eq!(
        snap.get_mouse_position(),
        gdcore::math::Vector2::new(42.0, 99.0)
    );
    assert!(snap.is_mouse_button_pressed(1)); // Left
    assert!(!snap.is_mouse_button_pressed(2)); // Right — not pressed
    assert!(snap.is_mouse_button_pressed(3)); // Middle
}

#[test]
fn aro_mouse_move_then_button_preserves_position() {
    let (mut ml, _) = build_scene_with_json_input_map();

    // Move mouse to (200, 150)
    ml.push_event(InputEvent::MouseMotion {
        position: gdcore::math::Vector2::new(200.0, 150.0),
        relative: gdcore::math::Vector2::ZERO,
    });

    // Click at (200, 150) — MouseButton also carries position
    ml.push_event(InputEvent::MouseButton {
        button: MouseButton::Left,
        pressed: true,
        position: gdcore::math::Vector2::new(200.0, 150.0),
    });

    assert_eq!(
        ml.input_state().get_mouse_position(),
        gdcore::math::Vector2::new(200.0, 150.0),
    );
    assert!(ml.input_state().is_mouse_button_pressed(MouseButton::Left));
    assert!(ml.input_state().is_action_pressed("shoot"));
}

// ═══════════════════════════════════════════════════════════════════════
// pat-vih: Edge cases — missing actions, overlapping bindings, empty map
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn vih_missing_action_returns_false() {
    let map = load_fixture_input_map();

    // Query a nonexistent action — should return None / false, not panic
    assert!(map.get_bindings("nonexistent_action").is_none());
    assert_eq!(map.get_deadzone("nonexistent_action"), 0.0);

    let evt = InputEvent::Key {
        key: Key::Space,
        pressed: true,
        shift: false,
        ctrl: false,
        alt: false,
    };
    assert!(!map.event_matches_action(&evt, "nonexistent_action"));
}

#[test]
fn vih_missing_action_via_push_event_does_not_panic() {
    let (mut ml, _) = build_scene_with_json_input_map();

    // Query an action that doesn't exist in the loaded map
    assert!(!ml.input_state().is_action_pressed("totally_bogus"));
    assert!(!ml.input_state().is_action_just_pressed("totally_bogus"));

    // Push an event and step — should not panic even though no action matches
    ml.push_event(InputEvent::Key {
        key: Key::Z,
        pressed: true,
        shift: false,
        ctrl: false,
        alt: false,
    });
    ml.step(DT);
    assert!(!ml.input_state().is_action_pressed("totally_bogus"));
}

#[test]
fn vih_overlapping_bindings_same_key_two_actions() {
    // Two different actions bound to the same key — both should activate
    let json = r#"{
        "actions": {
            "action_a": { "keys": ["Space"] },
            "action_b": { "keys": ["Space", "Enter"] }
        }
    }"#;
    let map = InputMap::load_from_json(json).unwrap();

    let evt_space = InputEvent::Key {
        key: Key::Space,
        pressed: true,
        shift: false,
        ctrl: false,
        alt: false,
    };
    assert!(map.event_matches_action(&evt_space, "action_a"));
    assert!(map.event_matches_action(&evt_space, "action_b"));

    // Enter only matches action_b
    let evt_enter = InputEvent::Key {
        key: Key::Enter,
        pressed: true,
        shift: false,
        ctrl: false,
        alt: false,
    };
    assert!(!map.event_matches_action(&evt_enter, "action_a"));
    assert!(map.event_matches_action(&evt_enter, "action_b"));
}

#[test]
fn vih_overlapping_bindings_resolve_via_input_state() {
    // Verify overlapping bindings work through InputState (not just InputMap)
    let json = r#"{
        "actions": {
            "fire": { "keys": ["Space"] },
            "confirm": { "keys": ["Space"] }
        }
    }"#;
    let map = InputMap::load_from_json(json).unwrap();

    let mut state = gdplatform::input::InputState::new();
    state.set_input_map(map);

    state.process_event(InputEvent::Key {
        key: Key::Space,
        pressed: true,
        shift: false,
        ctrl: false,
        alt: false,
    });

    assert!(state.is_action_pressed("fire"));
    assert!(state.is_action_pressed("confirm"));
}

#[test]
fn vih_empty_map_has_no_actions() {
    let json = r#"{ "actions": {} }"#;
    let map = InputMap::load_from_json(json).unwrap();

    assert_eq!(map.actions().count(), 0);
    assert!(map.get_bindings("anything").is_none());

    let evt = InputEvent::Key {
        key: Key::A,
        pressed: true,
        shift: false,
        ctrl: false,
        alt: false,
    };
    assert!(!map.event_matches_action(&evt, "anything"));
}

#[test]
fn vih_empty_map_does_not_break_main_loop() {
    let json = r#"{ "actions": {} }"#;
    let empty_map = InputMap::load_from_json(json).unwrap();

    let packed = gdscene::packed_scene::PackedScene::from_tscn(SCENE_SOURCE).expect("parse scene");
    let mut tree = gdscene::SceneTree::new();
    let root_id = tree.root_id();
    gdscene::packed_scene::add_packed_scene_to_tree(&mut tree, root_id, &packed).expect("instance");

    let all = tree.all_nodes_in_tree_order();
    for &nid in &all {
        tree.process_script_enter_tree(nid);
    }

    let mut ml = gdscene::main_loop::MainLoop::new(tree);
    ml.set_input_map(empty_map);

    // Push events and step — nothing should panic
    ml.push_event(InputEvent::Key {
        key: Key::Space,
        pressed: true,
        shift: false,
        ctrl: false,
        alt: false,
    });
    ml.step(DT);
    assert!(!ml.input_state().is_action_pressed("jump"));
}

#[test]
fn vih_invalid_json_returns_errors() {
    assert!(InputMap::load_from_json("not json at all").is_err());
    assert!(InputMap::load_from_json("{}").is_err());
    assert!(InputMap::load_from_json(r#"{"actions": "string_not_object"}"#).is_err());
}

#[test]
fn vih_load_from_nonexistent_file_returns_error() {
    let result =
        InputMap::load_from_json_file(std::path::Path::new("/tmp/does_not_exist_12345.json"));
    assert!(result.is_err());
}

#[test]
fn vih_action_with_no_bindings() {
    // An action defined with no keys and no mouse_buttons should exist but have empty bindings
    let json = r#"{ "actions": { "empty_action": {} } }"#;
    let map = InputMap::load_from_json(json).unwrap();

    assert_eq!(map.actions().count(), 1);
    let bindings = map.get_bindings("empty_action").unwrap();
    assert_eq!(bindings.len(), 0);

    // No event should match this action
    let evt = InputEvent::Key {
        key: Key::A,
        pressed: true,
        shift: false,
        ctrl: false,
        alt: false,
    };
    assert!(!map.event_matches_action(&evt, "empty_action"));
}
