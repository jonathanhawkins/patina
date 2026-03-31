//! pat-3gs3o: 3D fixture corpus — coverage for audio_integration,
//! navigation_integration, and ui_complex scenes.
//!
//! These three fixtures had .tscn scene files and oracle outputs but were
//! missing golden JSON files and integration test coverage.
//!
//! Validates:
//!   1. Scene loading and tree structure correctness
//!   2. Golden JSON parity (node paths match)
//!   3. Frame stepping without panics (MainLoop)
//!   4. Scene transitions across all three fixtures
//!   5. Deterministic loading (two loads produce identical trees)

use gdscene::main_loop::MainLoop;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::SceneTree;

const DT: f64 = 1.0 / 60.0;

// ---------------------------------------------------------------------------
// Fixture sources
// ---------------------------------------------------------------------------

const AUDIO_INTEGRATION_TSCN: &str = include_str!("../../fixtures/scenes/audio_integration.tscn");
const NAVIGATION_INTEGRATION_TSCN: &str =
    include_str!("../../fixtures/scenes/navigation_integration.tscn");
const UI_COMPLEX_TSCN: &str = include_str!("../../fixtures/scenes/ui_complex.tscn");

const AUDIO_INTEGRATION_GOLDEN: &str =
    include_str!("../../fixtures/golden/scenes/audio_integration.json");
const NAVIGATION_INTEGRATION_GOLDEN: &str =
    include_str!("../../fixtures/golden/scenes/navigation_integration.json");
const UI_COMPLEX_GOLDEN: &str = include_str!("../../fixtures/golden/scenes/ui_complex.json");

fn parse_scene(tscn: &str) -> PackedScene {
    PackedScene::from_tscn(tscn).expect("failed to parse scene")
}

fn load_scene(tscn: &str) -> (SceneTree, gdscene::node::NodeId) {
    let packed = parse_scene(tscn);
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let scene_root = add_packed_scene_to_tree(&mut tree, root, &packed).unwrap();
    (tree, scene_root)
}

/// Collect all node paths from a SceneTree (DFS).
fn collect_paths(tree: &SceneTree) -> Vec<String> {
    let mut paths = Vec::new();
    fn walk(tree: &SceneTree, id: gdscene::node::NodeId, paths: &mut Vec<String>) {
        if let Some(path) = tree.node_path(id) {
            paths.push(path);
        }
        if let Some(node) = tree.get_node(id) {
            for &child in node.children() {
                walk(tree, child, paths);
            }
        }
    }
    walk(tree, tree.root_id(), &mut paths);
    paths
}

/// Parse golden JSON and extract all node paths.
fn golden_paths(json_str: &str) -> Vec<String> {
    let val: serde_json::Value = serde_json::from_str(json_str).expect("invalid golden JSON");
    let mut paths = Vec::new();
    fn walk_json(node: &serde_json::Value, paths: &mut Vec<String>) {
        if let Some(path) = node.get("path").and_then(|p| p.as_str()) {
            paths.push(path.to_string());
        }
        if let Some(children) = node.get("children").and_then(|c| c.as_array()) {
            for child in children {
                walk_json(child, paths);
            }
        }
    }
    if let Some(nodes) = val.get("nodes").and_then(|n| n.as_array()) {
        for node in nodes {
            walk_json(node, &mut paths);
        }
    }
    paths
}

fn assert_golden_paths_match(fixture_name: &str, tscn: &str, golden_json: &str) {
    let (tree, _) = load_scene(tscn);
    let actual = collect_paths(&tree);
    let expected = golden_paths(golden_json);

    let actual_filtered: Vec<&String> = actual.iter().filter(|p| *p != "/root").collect();

    assert_eq!(
        actual_filtered.len(),
        expected.len(),
        "{fixture_name}: node count mismatch — actual {} vs golden {}",
        actual_filtered.len(),
        expected.len()
    );

    for (a, e) in actual_filtered.iter().zip(expected.iter()) {
        assert_eq!(
            *a, e,
            "{fixture_name}: path mismatch — actual '{a}' vs golden '{e}'"
        );
    }
}

// ===========================================================================
// 1. Audio integration — CharacterBody2D + AudioStreamPlayer2D nodes
// ===========================================================================

#[test]
fn corpus_audio_integration_scene_loads_all_audio_nodes() {
    let (tree, scene_root) = load_scene(AUDIO_INTEGRATION_TSCN);
    let node = tree.get_node(scene_root).unwrap();
    assert_eq!(node.name(), "AudioWorld");
    assert_eq!(node.class_name(), "Node2D");

    let expected = [
        "/root/AudioWorld/Player",
        "/root/AudioWorld/Player/PlayerCollision",
        "/root/AudioWorld/Player/FootstepAudio",
        "/root/AudioWorld/BGM",
        "/root/AudioWorld/SFXEmitter",
        "/root/AudioWorld/Enemy",
        "/root/AudioWorld/Enemy/EnemyCollision",
        "/root/AudioWorld/Enemy/EnemyAudio",
    ];
    for path in &expected {
        assert!(
            tree.get_node_by_path(path).is_some(),
            "node should exist at {path}"
        );
    }
}

#[test]
fn corpus_audio_integration_steps_frames_without_panic() {
    let (tree, _) = load_scene(AUDIO_INTEGRATION_TSCN);
    let mut main_loop = MainLoop::new(tree);
    main_loop.run_frames(60, DT);
    assert_eq!(main_loop.frame_count(), 60);
}

// ===========================================================================
// 2. Navigation integration — CharacterBody2D + NavigationAgent2D + Marker2D
// ===========================================================================

#[test]
fn corpus_navigation_integration_scene_loads_all_nav_nodes() {
    let (tree, scene_root) = load_scene(NAVIGATION_INTEGRATION_TSCN);
    let node = tree.get_node(scene_root).unwrap();
    assert_eq!(node.name(), "NavWorld");
    assert_eq!(node.class_name(), "Node2D");

    let expected = [
        "/root/NavWorld/Player",
        "/root/NavWorld/Player/PlayerCollision",
        "/root/NavWorld/Player/PlayerNavAgent",
        "/root/NavWorld/Enemy1",
        "/root/NavWorld/Enemy1/Enemy1NavAgent",
        "/root/NavWorld/Enemy2",
        "/root/NavWorld/Enemy2/Enemy2NavAgent",
        "/root/NavWorld/Patrol",
        "/root/NavWorld/Patrol/Waypoint1",
        "/root/NavWorld/Patrol/Waypoint2",
        "/root/NavWorld/Patrol/Waypoint3",
        "/root/NavWorld/Patrol/Waypoint4",
    ];
    for path in &expected {
        assert!(
            tree.get_node_by_path(path).is_some(),
            "node should exist at {path}"
        );
    }
}

#[test]
fn corpus_navigation_integration_steps_frames_without_panic() {
    let (tree, _) = load_scene(NAVIGATION_INTEGRATION_TSCN);
    let mut main_loop = MainLoop::new(tree);
    main_loop.run_frames(60, DT);
    assert_eq!(main_loop.frame_count(), 60);
}

// ===========================================================================
// 3. UI complex — Control hierarchy with HUD, sidebar, dialog
// ===========================================================================

#[test]
fn corpus_ui_complex_scene_loads_full_hud_hierarchy() {
    let (tree, scene_root) = load_scene(UI_COMPLEX_TSCN);
    let node = tree.get_node(scene_root).unwrap();
    assert_eq!(node.name(), "UIRoot");
    assert_eq!(node.class_name(), "Control");

    let expected = [
        "/root/UIRoot/Header",
        "/root/UIRoot/Header/Title",
        "/root/UIRoot/Header/ScoreLabel",
        "/root/UIRoot/Header/HealthLabel",
        "/root/UIRoot/Sidebar",
        "/root/UIRoot/Sidebar/InventoryLabel",
        "/root/UIRoot/Sidebar/Slot1",
        "/root/UIRoot/Sidebar/Slot2",
        "/root/UIRoot/Sidebar/Slot3",
        "/root/UIRoot/DialogBox",
        "/root/UIRoot/DialogBox/SpeakerName",
        "/root/UIRoot/DialogBox/DialogText",
        "/root/UIRoot/DialogBox/NextButton",
        "/root/UIRoot/DialogBox/SkipButton",
    ];
    for path in &expected {
        assert!(
            tree.get_node_by_path(path).is_some(),
            "node should exist at {path}"
        );
    }
}

#[test]
fn corpus_ui_complex_steps_frames_without_panic() {
    let (tree, _) = load_scene(UI_COMPLEX_TSCN);
    let mut main_loop = MainLoop::new(tree);
    main_loop.run_frames(60, DT);
    assert_eq!(main_loop.frame_count(), 60);
}

// ===========================================================================
// 4. Golden parity — parsed tree vs golden JSON
// ===========================================================================

#[test]
fn corpus_golden_parity_audio_integration() {
    assert_golden_paths_match(
        "audio_integration",
        AUDIO_INTEGRATION_TSCN,
        AUDIO_INTEGRATION_GOLDEN,
    );
}

#[test]
fn corpus_golden_parity_navigation_integration() {
    assert_golden_paths_match(
        "navigation_integration",
        NAVIGATION_INTEGRATION_TSCN,
        NAVIGATION_INTEGRATION_GOLDEN,
    );
}

#[test]
fn corpus_golden_parity_ui_complex() {
    assert_golden_paths_match("ui_complex", UI_COMPLEX_TSCN, UI_COMPLEX_GOLDEN);
}

// ===========================================================================
// 5. Scene transitions across all three fixtures
// ===========================================================================

#[test]
fn corpus_scene_transition_through_all_corpus_fixtures() {
    let scenes: &[(&str, &str, &str)] = &[
        ("audio_integration", AUDIO_INTEGRATION_TSCN, "AudioWorld"),
        (
            "navigation_integration",
            NAVIGATION_INTEGRATION_TSCN,
            "NavWorld",
        ),
        ("ui_complex", UI_COMPLEX_TSCN, "UIRoot"),
    ];

    let (tree, _) = load_scene(scenes[0].1);
    let mut main_loop = MainLoop::new(tree);
    main_loop.run_frames(20, DT);

    for &(name, tscn, expected_root) in &scenes[1..] {
        let packed = parse_scene(tscn);
        let new_root_id = main_loop
            .tree_mut()
            .change_scene_to_packed(&packed)
            .unwrap();

        let node = main_loop.tree().get_node(new_root_id).unwrap();
        assert_eq!(
            node.name(),
            expected_root,
            "scene '{name}' root should be '{expected_root}'"
        );

        main_loop.run_frames(20, DT);
    }

    assert_eq!(main_loop.frame_count(), 60);
}

// ===========================================================================
// 6. Determinism: two identical loads produce identical trees
// ===========================================================================

#[test]
fn corpus_determinism_loads_identically() {
    let fixtures = [
        AUDIO_INTEGRATION_TSCN,
        NAVIGATION_INTEGRATION_TSCN,
        UI_COMPLEX_TSCN,
    ];

    for tscn in &fixtures {
        let paths_a = collect_paths(&load_scene(tscn).0);
        let paths_b = collect_paths(&load_scene(tscn).0);
        assert_eq!(
            paths_a, paths_b,
            "two loads of the same fixture should produce identical trees"
        );
    }
}
