//! B013: Measured 2D vertical-slice integration test.
//!
//! Loads `space_shooter.tscn`, attaches real GDScript scripts, feeds
//! deterministic input through `MainLoop::set_input()` + `step()`, and
//! verifies the full engine-owned pipeline: scene → scripts → input →
//! physics → process → render.
//!
//! This is the capstone test proving B012's engine-owned runtime flow
//! works end-to-end with real fixture content.

use std::collections::{HashMap, HashSet};

use gdscene::main_loop::MainLoop;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scripting::{GDScriptNodeInstance, InputSnapshot};
use gdscene::SceneTree;
use gdvariant::Variant;

const SCENE_SOURCE: &str = include_str!("../fixtures/scenes/space_shooter.tscn");
const PLAYER_SCRIPT: &str = include_str!("../fixtures/scripts/player.gd");
const SPAWNER_SCRIPT: &str = include_str!("../fixtures/scripts/enemy_spawner.gd");

const DT: f64 = 1.0 / 60.0;
const FRAMES: u64 = 60;

/// Default Godot input map matching the editor's defaults.
fn default_input_map() -> HashMap<String, Vec<String>> {
    let mut map = HashMap::new();
    map.insert("ui_left".into(), vec!["ArrowLeft".into()]);
    map.insert("ui_right".into(), vec!["ArrowRight".into()]);
    map.insert("ui_up".into(), vec!["ArrowUp".into()]);
    map.insert("ui_down".into(), vec!["ArrowDown".into()]);
    map.insert("shoot".into(), vec![" ".into()]);
    map
}

/// Builds a MainLoop from the space_shooter scene with scripts attached.
fn build_space_shooter() -> (MainLoop, gdscene::NodeId) {
    let packed = PackedScene::from_tscn(SCENE_SOURCE).expect("failed to parse scene");

    let mut tree = SceneTree::new();
    let root_id = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root_id, &packed).expect("failed to instance scene");

    // Find and attach scripts.
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

    let main_loop = MainLoop::new(tree);
    (main_loop, player_id.expect("Player node not found"))
}

/// Creates an InputSnapshot with the given actions pressed.
fn make_input(actions: &[&str], input_map: &HashMap<String, Vec<String>>) -> InputSnapshot {
    let mut pressed_keys = HashSet::new();
    let mut just_pressed = HashSet::new();
    for &action in actions {
        if let Some(keys) = input_map.get(action) {
            for key in keys {
                pressed_keys.insert(key.clone());
                just_pressed.insert(key.clone());
            }
        }
    }
    InputSnapshot {
        pressed_keys,
        just_pressed_keys: just_pressed,
        input_map: input_map.clone(),
    }
}

fn get_position(main_loop: &MainLoop, node_id: gdscene::NodeId) -> (f32, f32) {
    let node = main_loop.tree().get_node(node_id).unwrap();
    match node.get_property("position") {
        Variant::Vector2(v) => (v.x, v.y),
        other => panic!("expected Vector2 position, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[test]
fn vertical_slice_scene_loads_with_correct_structure() {
    let (main_loop, _player_id) = build_space_shooter();
    let tree = main_loop.tree();

    // SpaceShooter root + Background + Player + EnemySpawner + ScoreLabel = 5
    // plus the SceneTree root = 6
    assert_eq!(tree.node_count(), 6);
}

#[test]
fn vertical_slice_player_starts_at_expected_position() {
    let (main_loop, player_id) = build_space_shooter();
    let (x, y) = get_position(&main_loop, player_id);
    assert!((x - 320.0).abs() < 0.01, "player start x={x}, expected 320");
    assert!((y - 400.0).abs() < 0.01, "player start y={y}, expected 400");
}

#[test]
fn vertical_slice_60_frames_no_input() {
    let (mut main_loop, player_id) = build_space_shooter();

    // Run 60 frames with no input — player should stay put.
    for _ in 0..FRAMES {
        main_loop.step(DT);
    }

    assert_eq!(main_loop.frame_count(), FRAMES);
    let (x, y) = get_position(&main_loop, player_id);
    assert!(
        (x - 320.0).abs() < 0.01,
        "player should not move without input, x={x}"
    );
    assert!(
        (y - 400.0).abs() < 0.01,
        "player should not move without input, y={y}"
    );
}

#[test]
fn vertical_slice_player_moves_right_with_input() {
    let (mut main_loop, player_id) = build_space_shooter();
    let input_map = default_input_map();

    let start_x = get_position(&main_loop, player_id).0;

    // Hold right for 60 frames.
    for _ in 0..FRAMES {
        let snapshot = make_input(&["ui_right"], &input_map);
        main_loop.set_input(snapshot);
        main_loop.step(DT);
    }

    assert_eq!(main_loop.frame_count(), FRAMES);
    let (end_x, _) = get_position(&main_loop, player_id);

    // Player speed=200, delta=1/60, 60 frames => ~200 pixels right.
    let expected_move = 200.0 * DT as f32 * FRAMES as f32;
    let actual_move = end_x - start_x;
    assert!(
        actual_move > expected_move * 0.9,
        "player should move right ~{expected_move}px, moved {actual_move}px"
    );
}

#[test]
fn vertical_slice_player_moves_left_with_input() {
    let (mut main_loop, player_id) = build_space_shooter();
    let input_map = default_input_map();

    let start_x = get_position(&main_loop, player_id).0;

    // Hold left for 60 frames.
    for _ in 0..FRAMES {
        let snapshot = make_input(&["ui_left"], &input_map);
        main_loop.set_input(snapshot);
        main_loop.step(DT);
    }

    assert_eq!(main_loop.frame_count(), FRAMES);
    let (end_x, _) = get_position(&main_loop, player_id);

    // Player starts at x=320 with speed=200: 320 - 200*1 = 120.
    // But clamped to 0..640, so should be around 120.
    let actual_move = start_x - end_x;
    assert!(
        actual_move > 100.0,
        "player should move left significantly, moved {actual_move}px"
    );
}

#[test]
fn vertical_slice_player_clamped_to_viewport() {
    let (mut main_loop, player_id) = build_space_shooter();
    let input_map = default_input_map();

    // Hold left for 300 frames — should clamp at x=0.
    for _ in 0..300 {
        let snapshot = make_input(&["ui_left"], &input_map);
        main_loop.set_input(snapshot);
        main_loop.step(DT);
    }

    let (x, _) = get_position(&main_loop, player_id);
    assert!(
        x >= 0.0 && x <= 1.0,
        "player x should be clamped at left edge, got {x}"
    );
}

#[test]
fn vertical_slice_frame_output_matches() {
    let (mut main_loop, _) = build_space_shooter();

    let mut last_frame = 0u64;
    for i in 1..=FRAMES {
        let output = main_loop.step(DT);
        assert_eq!(output.frame_count, i, "frame_count mismatch at step {i}");
        assert_eq!(output.delta, DT);
        last_frame = output.frame_count;
    }
    assert_eq!(last_frame, FRAMES);
    assert_eq!(main_loop.frame_count(), FRAMES);
}

#[test]
fn vertical_slice_enemy_spawner_timer_advances() {
    let (mut main_loop, _) = build_space_shooter();

    // Run 60 frames (1 second). The spawner has spawn_interval=2.0,
    // so spawn_timer should be ~1.0 after 60 frames at 1/60.
    for _ in 0..FRAMES {
        main_loop.step(DT);
    }

    // Find the EnemySpawner node.
    let tree = main_loop.tree();
    let spawner_id = tree
        .all_nodes_in_tree_order()
        .into_iter()
        .find(|&id| {
            tree.get_node(id)
                .map(|n| n.name() == "EnemySpawner")
                .unwrap_or(false)
        })
        .expect("EnemySpawner not found");

    let node = tree.get_node(spawner_id).unwrap();
    match node.get_property("spawn_timer") {
        Variant::Float(t) => {
            assert!(
                (t - 1.0).abs() < 0.1,
                "spawn_timer should be ~1.0 after 60 frames, got {t}"
            );
        }
        Variant::Int(t) => {
            // Script may accumulate as int if started at 0
            assert!(
                (t as f64 - 1.0).abs() < 0.1,
                "spawn_timer should be ~1.0, got {t}"
            );
        }
        other => panic!("unexpected spawn_timer type: {other:?}"),
    }
}

#[test]
fn vertical_slice_diagonal_movement() {
    let (mut main_loop, player_id) = build_space_shooter();
    let input_map = default_input_map();

    let (start_x, start_y) = get_position(&main_loop, player_id);

    // Hold right + up for 30 frames.
    for _ in 0..30 {
        let snapshot = make_input(&["ui_right", "ui_up"], &input_map);
        main_loop.set_input(snapshot);
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
fn vertical_slice_deterministic() {
    let input_map = default_input_map();

    let run = || {
        let (mut main_loop, player_id) = build_space_shooter();
        // Deterministic input sequence: right for 20 frames, up for 20, nothing for 20.
        for frame in 0..FRAMES {
            let actions: &[&str] = if frame < 20 {
                &["ui_right"]
            } else if frame < 40 {
                &["ui_up"]
            } else {
                &[]
            };
            if !actions.is_empty() {
                main_loop.set_input(make_input(actions, &input_map));
            }
            main_loop.step(DT);
        }
        get_position(&main_loop, player_id)
    };

    let (x1, y1) = run();
    let (x2, y2) = run();
    assert_eq!(x1, x2, "determinism: x positions must match");
    assert_eq!(y1, y2, "determinism: y positions must match");
}

#[test]
fn vertical_slice_input_does_not_persist() {
    let (mut main_loop, player_id) = build_space_shooter();
    let input_map = default_input_map();

    // Frame 1: press right.
    main_loop.set_input(make_input(&["ui_right"], &input_map));
    main_loop.step(DT);
    let after_one = get_position(&main_loop, player_id).0;

    // Frame 2-60: no input set. Player should stop moving.
    for _ in 1..FRAMES {
        main_loop.step(DT);
    }
    let after_rest = get_position(&main_loop, player_id).0;

    // Player moved one frame's worth (~3.33px) then stopped.
    let first_move = after_one - 320.0;
    let total_move = after_rest - 320.0;
    assert!(
        (first_move - total_move).abs() < 0.01,
        "input should not persist: first_move={first_move}, total_move={total_move}"
    );
}
