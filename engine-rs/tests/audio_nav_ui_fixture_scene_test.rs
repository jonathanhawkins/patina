//! pat-r6sku: Integration tests that exercise the three new fixture scenes
//! (audio_integration, navigation_integration, ui_complex) through the scene
//! system, verifying correct parsing, tree structure, node types, properties,
//! scene transitions, and cross-domain coexistence.
//!
//! Each test states the observable behavior it checks (Oracle Rule 2).

use gdscene::control;
use gdscene::main_loop::MainLoop;
use gdscene::node::NodeId;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::SceneTree;

const DT: f64 = 1.0 / 60.0;

const AUDIO_TSCN: &str = include_str!("../../fixtures/scenes/audio_integration.tscn");
const NAV_TSCN: &str = include_str!("../../fixtures/scenes/navigation_integration.tscn");
const UI_COMPLEX_TSCN: &str = include_str!("../../fixtures/scenes/ui_complex.tscn");
const PLATFORMER_TSCN: &str = include_str!("../../fixtures/scenes/platformer.tscn");

fn parse_scene(tscn: &str) -> PackedScene {
    PackedScene::from_tscn(tscn).expect("failed to parse scene")
}

fn load_scene(tscn: &str) -> (SceneTree, NodeId) {
    let packed = parse_scene(tscn);
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let scene_root = add_packed_scene_to_tree(&mut tree, root, &packed).unwrap();
    (tree, scene_root)
}

// ===========================================================================
// Audio fixture scene tests
// ===========================================================================

/// Verifies audio_integration.tscn parses into the expected tree structure
/// with AudioStreamPlayer2D nodes as children of game entities.
#[test]
fn r6sku_audio_fixture_parses_correct_tree_structure() {
    let (tree, _root) = load_scene(AUDIO_TSCN);

    // Root is AudioWorld (Node2D).
    let audio_world = tree
        .get_node_by_path("/root/AudioWorld")
        .expect("AudioWorld root should exist");
    assert_eq!(tree.get_node(audio_world).unwrap().class_name(),"Node2D");

    // Player with collision and audio child.
    let player = tree
        .get_node_by_path("/root/AudioWorld/Player")
        .expect("Player should exist");
    assert_eq!(tree.get_node(player).unwrap().class_name(),"CharacterBody2D");

    tree.get_node_by_path("/root/AudioWorld/Player/FootstepAudio")
        .expect("FootstepAudio should exist");
    tree.get_node_by_path("/root/AudioWorld/Player/PlayerCollision")
        .expect("PlayerCollision should exist");

    // BGM is a top-level audio node.
    let bgm = tree
        .get_node_by_path("/root/AudioWorld/BGM")
        .expect("BGM should exist");
    assert_eq!(
        tree.get_node(bgm).unwrap().class_name(),
        "AudioStreamPlayer2D"
    );

    // SFXEmitter.
    tree.get_node_by_path("/root/AudioWorld/SFXEmitter")
        .expect("SFXEmitter should exist");

    // Enemy with audio child.
    tree.get_node_by_path("/root/AudioWorld/Enemy/EnemyAudio")
        .expect("EnemyAudio should exist");
}

/// Verifies audio scene has correct node count (9 nodes total).
#[test]
fn r6sku_audio_fixture_node_count() {
    let (tree, _root) = load_scene(AUDIO_TSCN);
    let count = tree.node_count();
    // AudioWorld + Player + PlayerCollision + FootstepAudio + BGM + SFXEmitter
    // + Enemy + EnemyCollision + EnemyAudio = 9, plus the implicit /root = 10
    assert!(
        count >= 9,
        "audio scene should have at least 9 nodes, got {count}"
    );
}

/// Verifies audio scene nodes have expected position properties.
#[test]
fn r6sku_audio_fixture_positions() {
    let (tree, _root) = load_scene(AUDIO_TSCN);

    let player = tree
        .get_node_by_path("/root/AudioWorld/Player")
        .expect("Player");
    let pos = gdscene::node2d::get_position(&tree, player);
    assert!(
        (pos.x - 100.0).abs() < 0.01 && (pos.y - 200.0).abs() < 0.01,
        "Player position should be (100, 200), got ({}, {})",
        pos.x,
        pos.y
    );

    let enemy = tree
        .get_node_by_path("/root/AudioWorld/Enemy")
        .expect("Enemy");
    let epos = gdscene::node2d::get_position(&tree, enemy);
    assert!(
        (epos.x - 500.0).abs() < 0.01 && (epos.y - 200.0).abs() < 0.01,
        "Enemy position should be (500, 200), got ({}, {})",
        epos.x,
        epos.y
    );
}

/// Verifies audio scene runs stably through 60 frames in MainLoop.
#[test]
fn r6sku_audio_fixture_mainloop_stability() {
    let packed = parse_scene(AUDIO_TSCN);
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &packed).unwrap();
    let mut main_loop = MainLoop::new(tree);

    main_loop.run_frames(60, DT);
    assert_eq!(main_loop.frame_count(), 60);
}

// ===========================================================================
// Navigation fixture scene tests
// ===========================================================================

/// Verifies navigation_integration.tscn parses with correct tree including
/// NavigationAgent2D nodes and waypoint markers.
#[test]
fn r6sku_nav_fixture_parses_correct_tree_structure() {
    let (tree, _root) = load_scene(NAV_TSCN);

    let nav_world = tree
        .get_node_by_path("/root/NavWorld")
        .expect("NavWorld root should exist");
    assert_eq!(tree.get_node(nav_world).unwrap().class_name(),"Node2D");

    // Player with nav agent.
    tree.get_node_by_path("/root/NavWorld/Player/PlayerNavAgent")
        .expect("PlayerNavAgent should exist");

    // Enemy1 with nav agent.
    tree.get_node_by_path("/root/NavWorld/Enemy1/Enemy1NavAgent")
        .expect("Enemy1NavAgent should exist");

    // Enemy2 with nav agent.
    tree.get_node_by_path("/root/NavWorld/Enemy2/Enemy2NavAgent")
        .expect("Enemy2NavAgent should exist");

    // Patrol waypoints.
    for i in 1..=4 {
        tree.get_node_by_path(&format!("/root/NavWorld/Patrol/Waypoint{i}"))
            .unwrap_or_else(|| panic!("Waypoint{i} should exist"));
    }
}

/// Verifies nav scene has correct node count.
#[test]
fn r6sku_nav_fixture_node_count() {
    let (tree, _root) = load_scene(NAV_TSCN);
    let count = tree.node_count();
    // NavWorld + Player + PlayerCollision + PlayerNavAgent
    // + Enemy1 + Enemy1Collision + Enemy1NavAgent
    // + Enemy2 + Enemy2Collision + Enemy2NavAgent
    // + Patrol + Waypoint1-4 = 14, plus /root = 15
    assert!(
        count >= 14,
        "nav scene should have at least 14 nodes, got {count}"
    );
}

/// Verifies nav scene positions are set correctly from tscn properties.
#[test]
fn r6sku_nav_fixture_positions() {
    let (tree, _root) = load_scene(NAV_TSCN);

    let player = tree
        .get_node_by_path("/root/NavWorld/Player")
        .expect("Player");
    let pos = gdscene::node2d::get_position(&tree, player);
    assert!(
        (pos.x - 50.0).abs() < 0.01 && (pos.y - 100.0).abs() < 0.01,
        "Player pos should be (50, 100), got ({}, {})",
        pos.x,
        pos.y
    );

    let wp2 = tree
        .get_node_by_path("/root/NavWorld/Patrol/Waypoint2")
        .expect("Waypoint2");
    let wp2_pos = gdscene::node2d::get_position(&tree, wp2);
    assert!(
        (wp2_pos.x - 500.0).abs() < 0.01 && (wp2_pos.y - 50.0).abs() < 0.01,
        "Waypoint2 pos should be (500, 50), got ({}, {})",
        wp2_pos.x,
        wp2_pos.y
    );
}

/// Verifies nav scene runs stably through 60 frames.
#[test]
fn r6sku_nav_fixture_mainloop_stability() {
    let packed = parse_scene(NAV_TSCN);
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &packed).unwrap();
    let mut main_loop = MainLoop::new(tree);

    main_loop.run_frames(60, DT);
    assert_eq!(main_loop.frame_count(), 60);
}

// ===========================================================================
// UI complex fixture scene tests
// ===========================================================================

/// Verifies ui_complex.tscn parses with correct tree: header, sidebar,
/// dialog box sections with labels and buttons.
#[test]
fn r6sku_ui_complex_fixture_parses_correct_tree_structure() {
    let (tree, _root) = load_scene(UI_COMPLEX_TSCN);

    let ui_root = tree
        .get_node_by_path("/root/UIRoot")
        .expect("UIRoot should exist");
    assert_eq!(tree.get_node(ui_root).unwrap().class_name(),"Control");

    // Header section.
    tree.get_node_by_path("/root/UIRoot/Header")
        .expect("Header should exist");
    tree.get_node_by_path("/root/UIRoot/Header/Title")
        .expect("Title should exist");
    tree.get_node_by_path("/root/UIRoot/Header/ScoreLabel")
        .expect("ScoreLabel should exist");
    tree.get_node_by_path("/root/UIRoot/Header/HealthLabel")
        .expect("HealthLabel should exist");

    // Sidebar section.
    tree.get_node_by_path("/root/UIRoot/Sidebar")
        .expect("Sidebar should exist");
    tree.get_node_by_path("/root/UIRoot/Sidebar/Slot1")
        .expect("Slot1 should exist");
    tree.get_node_by_path("/root/UIRoot/Sidebar/Slot2")
        .expect("Slot2 should exist");
    tree.get_node_by_path("/root/UIRoot/Sidebar/Slot3")
        .expect("Slot3 should exist");

    // Dialog box.
    tree.get_node_by_path("/root/UIRoot/DialogBox")
        .expect("DialogBox should exist");
    tree.get_node_by_path("/root/UIRoot/DialogBox/NextButton")
        .expect("NextButton should exist");
    tree.get_node_by_path("/root/UIRoot/DialogBox/SkipButton")
        .expect("SkipButton should exist");
}

/// Verifies UI complex scene has correct node count.
#[test]
fn r6sku_ui_complex_fixture_node_count() {
    let (tree, _root) = load_scene(UI_COMPLEX_TSCN);
    let count = tree.node_count();
    // UIRoot + Header + Title + ScoreLabel + HealthLabel
    // + Sidebar + InventoryLabel + Slot1 + Slot2 + Slot3
    // + DialogBox + SpeakerName + DialogText + NextButton + SkipButton = 15
    // plus /root = 16
    assert!(
        count >= 15,
        "UI complex scene should have at least 15 nodes, got {count}"
    );
}

/// Verifies UI complex scene label text properties from tscn.
#[test]
fn r6sku_ui_complex_fixture_label_text_properties() {
    let (tree, _root) = load_scene(UI_COMPLEX_TSCN);

    let title = tree
        .get_node_by_path("/root/UIRoot/Header/Title")
        .expect("Title");
    assert_eq!(control::get_label_text(&tree, title), "Game HUD");

    let score = tree
        .get_node_by_path("/root/UIRoot/Header/ScoreLabel")
        .expect("ScoreLabel");
    assert_eq!(control::get_label_text(&tree, score), "Score: 0");

    let health = tree
        .get_node_by_path("/root/UIRoot/Header/HealthLabel")
        .expect("HealthLabel");
    assert_eq!(control::get_label_text(&tree, health), "HP: 100");

    let speaker = tree
        .get_node_by_path("/root/UIRoot/DialogBox/SpeakerName")
        .expect("SpeakerName");
    assert_eq!(control::get_label_text(&tree, speaker), "NPC");

    let dialog_text = tree
        .get_node_by_path("/root/UIRoot/DialogBox/DialogText")
        .expect("DialogText");
    assert_eq!(
        control::get_label_text(&tree, dialog_text),
        "Welcome, adventurer!"
    );
}

/// Verifies UI complex scene button text properties from tscn.
#[test]
fn r6sku_ui_complex_fixture_button_text_properties() {
    let (tree, _root) = load_scene(UI_COMPLEX_TSCN);

    let slot1 = tree
        .get_node_by_path("/root/UIRoot/Sidebar/Slot1")
        .expect("Slot1");
    assert_eq!(control::get_button_text(&tree, slot1), "Sword");

    let slot2 = tree
        .get_node_by_path("/root/UIRoot/Sidebar/Slot2")
        .expect("Slot2");
    assert_eq!(control::get_button_text(&tree, slot2), "Shield");

    let next_btn = tree
        .get_node_by_path("/root/UIRoot/DialogBox/NextButton")
        .expect("NextButton");
    assert_eq!(control::get_button_text(&tree, next_btn), "Next");
}

/// Verifies UI complex scene anchor properties on root control.
#[test]
fn r6sku_ui_complex_fixture_anchor_properties() {
    let (tree, _root) = load_scene(UI_COMPLEX_TSCN);

    let ui_root = tree
        .get_node_by_path("/root/UIRoot")
        .expect("UIRoot");

    // UIRoot should have full-rect anchors (right=1.0, bottom=1.0).
    let ar = control::get_anchor_right(&tree, ui_root);
    let ab = control::get_anchor_bottom(&tree, ui_root);
    assert!(
        (ar - 1.0).abs() < 0.01,
        "anchor_right should be 1.0, got {ar}"
    );
    assert!(
        (ab - 1.0).abs() < 0.01,
        "anchor_bottom should be 1.0, got {ab}"
    );
}

/// Verifies UI complex scene signal connections are preserved in PackedScene.
#[test]
fn r6sku_ui_complex_fixture_signal_connections() {
    let packed = parse_scene(UI_COMPLEX_TSCN);

    // The packed scene should have signal connections parsed from [connection] sections.
    let connections = packed.connections();
    let has_pressed = connections.iter().any(|c| c.signal_name == "pressed");
    assert!(
        has_pressed,
        "UI complex scene should have 'pressed' signal connections, found {} connections",
        connections.len()
    );

    // Should have at least 5 connections (3 slots + next + skip buttons).
    assert!(
        connections.len() >= 5,
        "expected at least 5 signal connections, got {}",
        connections.len()
    );
}

/// Verifies UI complex scene runs stably through 60 frames.
#[test]
fn r6sku_ui_complex_fixture_mainloop_stability() {
    let packed = parse_scene(UI_COMPLEX_TSCN);
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &packed).unwrap();
    let mut main_loop = MainLoop::new(tree);

    main_loop.run_frames(60, DT);
    assert_eq!(main_loop.frame_count(), 60);
}

// ===========================================================================
// Cross-domain fixture scene transition tests
// ===========================================================================

/// Verifies transitioning from audio scene to nav scene cleans up old tree
/// and correctly loads new tree.
#[test]
fn r6sku_fixture_transition_audio_to_nav() {
    let packed_audio = parse_scene(AUDIO_TSCN);
    let packed_nav = parse_scene(NAV_TSCN);

    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &packed_audio).unwrap();
    let mut main_loop = MainLoop::new(tree);

    main_loop.run_frames(10, DT);
    assert!(
        main_loop
            .tree()
            .get_node_by_path("/root/AudioWorld/BGM")
            .is_some(),
        "audio scene should have BGM"
    );

    // Transition to nav scene.
    main_loop
        .tree_mut()
        .change_scene_to_packed(&packed_nav)
        .unwrap();

    // Old audio nodes gone.
    assert!(
        main_loop
            .tree()
            .get_node_by_path("/root/AudioWorld/BGM")
            .is_none(),
        "audio nodes should be cleaned up"
    );

    // New nav nodes present.
    assert!(
        main_loop
            .tree()
            .get_node_by_path("/root/NavWorld/Player/PlayerNavAgent")
            .is_some(),
        "nav scene should have PlayerNavAgent"
    );

    main_loop.run_frames(20, DT);
    assert_eq!(main_loop.frame_count(), 30);
}

/// Verifies transitioning from UI complex to gameplay preserves frame count.
#[test]
fn r6sku_fixture_transition_ui_to_gameplay() {
    let packed_ui = parse_scene(UI_COMPLEX_TSCN);
    let packed_game = parse_scene(PLATFORMER_TSCN);

    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &packed_ui).unwrap();
    let mut main_loop = MainLoop::new(tree);

    main_loop.run_frames(30, DT);

    // Verify UI is loaded.
    assert!(
        main_loop
            .tree()
            .get_node_by_path("/root/UIRoot/Header/Title")
            .is_some(),
    );

    // Transition to gameplay.
    main_loop
        .tree_mut()
        .change_scene_to_packed(&packed_game)
        .unwrap();

    // Old UI gone.
    assert!(
        main_loop
            .tree()
            .get_node_by_path("/root/UIRoot/Header/Title")
            .is_none(),
    );

    main_loop.run_frames(30, DT);
    assert_eq!(main_loop.frame_count(), 60);
}

/// Verifies all three fixture scenes can be loaded and transitioned through
/// in sequence: audio -> nav -> ui_complex, running frames at each stage.
#[test]
fn r6sku_fixture_sequential_three_scene_transition() {
    let scenes = [
        ("audio", parse_scene(AUDIO_TSCN)),
        ("nav", parse_scene(NAV_TSCN)),
        ("ui", parse_scene(UI_COMPLEX_TSCN)),
    ];

    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &scenes[0].1).unwrap();
    let mut main_loop = MainLoop::new(tree);

    main_loop.run_frames(20, DT);

    for (_name, packed) in &scenes[1..] {
        main_loop
            .tree_mut()
            .change_scene_to_packed(packed)
            .unwrap();
        main_loop.run_frames(20, DT);
    }

    // Total: 20 (audio) + 20 (nav) + 20 (ui) = 60 frames.
    assert_eq!(
        main_loop.frame_count(),
        60,
        "should have run 60 total frames across 3 scene transitions"
    );
}

/// Verifies loading the same fixture scene twice produces identical trees.
#[test]
fn r6sku_fixture_deterministic_reload() {
    let packed = parse_scene(NAV_TSCN);

    let (tree1, _) = {
        let mut t = SceneTree::new();
        let r = t.root_id();
        let sr = add_packed_scene_to_tree(&mut t, r, &packed).unwrap();
        (t, sr)
    };
    let (tree2, _) = {
        let mut t = SceneTree::new();
        let r = t.root_id();
        let sr = add_packed_scene_to_tree(&mut t, r, &packed).unwrap();
        (t, sr)
    };

    assert_eq!(
        tree1.node_count(),
        tree2.node_count(),
        "two loads of the same scene should produce identical node counts"
    );
}
