//! pat-g9k: Keyboard action coverage through engine-owned input API.
//!
//! Proves that `MainLoop::push_event()` + `set_input_map()` drives the full
//! scene→script pipeline end-to-end. Uses real GDScript scripts and fixtures,
//! not the manual `set_input()` path.

use gdplatform::input::{ActionBinding, InputEvent, InputMap, Key};
use gdscene::main_loop::MainLoop;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scripting::GDScriptNodeInstance;
use gdscene::SceneTree;
use gdvariant::Variant;

const SCENE_SOURCE: &str = include_str!("../fixtures/scenes/space_shooter.tscn");
const PLAYER_SCRIPT: &str = include_str!("../fixtures/scripts/player.gd");
const SPAWNER_SCRIPT: &str = include_str!("../fixtures/scripts/enemy_spawner.gd");

const DT: f64 = 1.0 / 60.0;

/// Builds a default InputMap with all standard Godot-like action bindings.
fn default_engine_input_map() -> InputMap {
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

/// Builds a MainLoop from the space_shooter scene with scripts attached and
/// engine-owned InputMap configured.
fn build_space_shooter_with_engine_input() -> (MainLoop, gdscene::NodeId) {
    let packed = PackedScene::from_tscn(SCENE_SOURCE).expect("failed to parse scene");

    let mut tree = SceneTree::new();
    let root_id = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root_id, &packed).expect("failed to instance scene");

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

    // Run lifecycle: enter_tree + ready.
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
    main_loop.set_input_map(default_engine_input_map());
    (main_loop, player_id.expect("Player node not found"))
}

fn get_position(main_loop: &MainLoop, node_id: gdscene::NodeId) -> (f32, f32) {
    let node = main_loop.tree().get_node(node_id).unwrap();
    match node.get_property("position") {
        Variant::Vector2(v) => (v.x, v.y),
        other => panic!("expected Vector2 position, got {other:?}"),
    }
}

fn key_event(key: Key, pressed: bool) -> InputEvent {
    InputEvent::Key {
        key,
        pressed,
        shift: false,
        ctrl: false,
        alt: false,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[test]
fn push_event_moves_player_right() {
    let (mut main_loop, player_id) = build_space_shooter_with_engine_input();
    let start_x = get_position(&main_loop, player_id).0;

    // Push right key via engine-owned InputState (not set_input).
    main_loop.push_event(key_event(Key::Right, true));
    for _ in 0..60 {
        main_loop.step(DT);
    }

    let (end_x, _) = get_position(&main_loop, player_id);
    let expected_move = 200.0 * DT as f32 * 60.0;
    let actual_move = end_x - start_x;
    assert!(
        actual_move > expected_move * 0.9,
        "player should move right ~{expected_move}px via push_event, moved {actual_move}px"
    );
}

#[test]
fn push_event_moves_player_left() {
    let (mut main_loop, player_id) = build_space_shooter_with_engine_input();
    let start_x = get_position(&main_loop, player_id).0;

    main_loop.push_event(key_event(Key::Left, true));
    for _ in 0..60 {
        main_loop.step(DT);
    }

    let actual_move = start_x - get_position(&main_loop, player_id).0;
    assert!(
        actual_move > 100.0,
        "player should move left via push_event, moved {actual_move}px"
    );
}

#[test]
fn push_event_moves_player_up() {
    let (mut main_loop, player_id) = build_space_shooter_with_engine_input();
    let start_y = get_position(&main_loop, player_id).1;

    main_loop.push_event(key_event(Key::Up, true));
    for _ in 0..60 {
        main_loop.step(DT);
    }

    let end_y = get_position(&main_loop, player_id).1;
    assert!(
        end_y < start_y - 100.0,
        "player should move up via push_event: {start_y} -> {end_y}"
    );
}

#[test]
fn push_event_moves_player_down() {
    let (mut main_loop, player_id) = build_space_shooter_with_engine_input();
    let start_y = get_position(&main_loop, player_id).1;

    main_loop.push_event(key_event(Key::Down, true));
    for _ in 0..60 {
        main_loop.step(DT);
    }

    let end_y = get_position(&main_loop, player_id).1;
    // Clamped to 480, start at 400, speed=200, 1s => 600 but clamped to 480.
    assert!(
        end_y > start_y,
        "player should move down via push_event: {start_y} -> {end_y}"
    );
}

#[test]
fn push_event_shoot_triggers_cooldown() {
    let (mut main_loop, player_id) = build_space_shooter_with_engine_input();

    // Push space (shoot action).
    main_loop.push_event(key_event(Key::Space, true));
    main_loop.step(DT);

    // After one frame with shoot pressed, shoot_cooldown should be set to 0.3.
    let node = main_loop.tree().get_node(player_id).unwrap();
    let cooldown = match node.get_property("shoot_cooldown") {
        Variant::Float(v) => v,
        Variant::Int(v) => v as f64,
        other => panic!("unexpected shoot_cooldown type: {other:?}"),
    };
    assert!(
        (cooldown - 0.3).abs() < 0.05,
        "shoot should set cooldown to ~0.3, got {cooldown}"
    );
}

#[test]
fn all_action_names_resolve_through_input_map() {
    let (mut main_loop, _) = build_space_shooter_with_engine_input();

    // Verify each action is recognized by InputState after push_event.
    let actions_and_keys: &[(&str, Key)] = &[
        ("ui_left", Key::Left),
        ("ui_right", Key::Right),
        ("ui_up", Key::Up),
        ("ui_down", Key::Down),
        ("ui_accept", Key::Enter),
        ("shoot", Key::Space),
        ("jump", Key::Space),
    ];

    for &(action, key) in actions_and_keys {
        // Press the key.
        main_loop.push_event(key_event(key, true));
        assert!(
            main_loop.input_state().is_action_pressed(action),
            "action '{action}' should be pressed after push_event({key:?})"
        );
        // Release the key.
        main_loop.push_event(key_event(key, false));
        assert!(
            !main_loop.input_state().is_action_pressed(action),
            "action '{action}' should not be pressed after release"
        );
    }
}

#[test]
fn is_action_just_pressed_true_only_first_frame() {
    let (mut main_loop, _) = build_space_shooter_with_engine_input();

    // Frame 1: press right — just_pressed should be true.
    main_loop.push_event(key_event(Key::Right, true));
    assert!(main_loop.input_state().is_action_just_pressed("ui_right"));

    main_loop.step(DT);

    // After step, flush_frame clears just_pressed.
    assert!(
        !main_loop.input_state().is_action_just_pressed("ui_right"),
        "just_pressed should be false after first step"
    );
    // But action should still be pressed (key held).
    assert!(main_loop.input_state().is_action_pressed("ui_right"));

    // Frame 2: no new events — just_pressed stays false.
    main_loop.step(DT);
    assert!(
        !main_loop.input_state().is_action_just_pressed("ui_right"),
        "just_pressed should remain false on subsequent frames"
    );
    assert!(main_loop.input_state().is_action_pressed("ui_right"));
}

#[test]
fn get_vector_returns_correct_direction_via_push_event() {
    // Use a custom script that calls Input.get_vector() to verify normalized direction.
    let get_vector_script = r#"extends Node2D

var dir_x = 0.0
var dir_y = 0.0

func _process(delta):
    var dir = Input.get_vector("ui_left", "ui_right", "ui_up", "ui_down")
    self.dir_x = dir.x
    self.dir_y = dir.y
"#;

    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let node = gdscene::node::Node::new("Mover", "Node2D");
    let node_id = tree.add_child(root, node).unwrap();

    let instance = GDScriptNodeInstance::from_source(get_vector_script, node_id)
        .expect("failed to parse get_vector script");
    tree.attach_script(node_id, Box::new(instance));
    tree.process_script_enter_tree(node_id);
    tree.process_script_ready(node_id);

    let mut main_loop = MainLoop::new(tree);
    main_loop.set_input_map(default_engine_input_map());

    // Press right only → dir should be (1, 0).
    main_loop.push_event(key_event(Key::Right, true));
    main_loop.step(DT);

    let node = main_loop.tree().get_node(node_id).unwrap();
    let dir_x = match node.get_property("dir_x") {
        Variant::Float(v) => v,
        Variant::Int(v) => v as f64,
        other => panic!("unexpected dir_x: {other:?}"),
    };
    let dir_y = match node.get_property("dir_y") {
        Variant::Float(v) => v,
        Variant::Int(v) => v as f64,
        other => panic!("unexpected dir_y: {other:?}"),
    };
    assert!(
        (dir_x - 1.0).abs() < 0.01,
        "get_vector x should be 1.0 with right pressed, got {dir_x}"
    );
    assert!(
        dir_y.abs() < 0.01,
        "get_vector y should be 0.0 with only right pressed, got {dir_y}"
    );

    // Release right, press right + up → dir should be normalized (~0.707, ~-0.707).
    main_loop.push_event(key_event(Key::Right, false));
    main_loop.push_event(key_event(Key::Right, true));
    main_loop.push_event(key_event(Key::Up, true));
    main_loop.step(DT);

    let node = main_loop.tree().get_node(node_id).unwrap();
    let dir_x = match node.get_property("dir_x") {
        Variant::Float(v) => v,
        Variant::Int(v) => v as f64,
        other => panic!("unexpected dir_x: {other:?}"),
    };
    let dir_y = match node.get_property("dir_y") {
        Variant::Float(v) => v,
        Variant::Int(v) => v as f64,
        other => panic!("unexpected dir_y: {other:?}"),
    };
    let expected = 1.0 / (2.0_f64).sqrt();
    assert!(
        (dir_x - expected).abs() < 0.02,
        "get_vector x should be ~{expected} with right+up, got {dir_x}"
    );
    assert!(
        (dir_y + expected).abs() < 0.02,
        "get_vector y should be ~-{expected} with right+up, got {dir_y}"
    );
}

#[test]
fn push_event_diagonal_movement() {
    let (mut main_loop, player_id) = build_space_shooter_with_engine_input();
    let (start_x, start_y) = get_position(&main_loop, player_id);

    // Press right + up simultaneously via push_event.
    main_loop.push_event(key_event(Key::Right, true));
    main_loop.push_event(key_event(Key::Up, true));
    for _ in 0..30 {
        main_loop.step(DT);
    }

    let (end_x, end_y) = get_position(&main_loop, player_id);
    assert!(
        end_x > start_x,
        "player should move right: {start_x} -> {end_x}"
    );
    assert!(
        end_y < start_y,
        "player should move up: {start_y} -> {end_y}"
    );
}

#[test]
fn push_event_release_stops_movement() {
    let (mut main_loop, player_id) = build_space_shooter_with_engine_input();

    // Press right for 10 frames.
    main_loop.push_event(key_event(Key::Right, true));
    for _ in 0..10 {
        main_loop.step(DT);
    }
    let pos_after_press = get_position(&main_loop, player_id).0;

    // Release right, run 10 more frames.
    main_loop.push_event(key_event(Key::Right, false));
    for _ in 0..10 {
        main_loop.step(DT);
    }
    let pos_after_release = get_position(&main_loop, player_id).0;

    // Player should have stopped after release.
    assert!(
        (pos_after_press - pos_after_release).abs() < 0.01,
        "player should stop after key release: {pos_after_press} vs {pos_after_release}"
    );
}
