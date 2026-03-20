//! pat-xm0: Oracle-backed fixture for unique-name NodePath behavior.
//!
//! Loads `fixtures/scenes/unique_name_resolution.tscn` through Patina's
//! PackedScene parser, instances it, and verifies that `get_node("%Foo")`
//! resolves correctly. The expected behavior matches Godot 4.x where
//! `%Name` searches the owner scope for a node with `unique_name_in_owner`.

use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;

fn load_fixture() -> (SceneTree, gdscene::NodeId) {
    let tscn_src = std::fs::read_to_string("../fixtures/scenes/unique_name_resolution.tscn")
        .expect("fixture file should exist");
    let scene = PackedScene::from_tscn(&tscn_src).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let scene_root = add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();
    (tree, scene_root)
}

// ===========================================================================
// 1. %HealthBar resolves from scene root
// ===========================================================================

#[test]
fn xm0_fixture_health_bar_resolves() {
    let (tree, scene_root) = load_fixture();

    let hb = tree
        .get_node_relative(scene_root, "%HealthBar")
        .expect("%HealthBar should resolve within owner scope");
    assert_eq!(tree.get_node(hb).unwrap().name(), "HealthBar");
    assert!(tree.get_node(hb).unwrap().is_unique_name());
    assert_eq!(tree.get_node(hb).unwrap().class_name(), "ProgressBar");
}

// ===========================================================================
// 2. %ScoreLabel resolves (nested under Panel)
// ===========================================================================

#[test]
fn xm0_fixture_score_label_resolves() {
    let (tree, scene_root) = load_fixture();

    let sl = tree
        .get_node_relative(scene_root, "%ScoreLabel")
        .expect("%ScoreLabel should resolve even though nested under Panel");
    assert_eq!(tree.get_node(sl).unwrap().name(), "ScoreLabel");
    assert_eq!(tree.get_node(sl).unwrap().class_name(), "Label");

    // Also resolvable from a sibling node
    let hb = tree.get_node_relative(scene_root, "%HealthBar").unwrap();
    assert_eq!(tree.get_node_relative(hb, "%ScoreLabel").unwrap(), sl);
}

// ===========================================================================
// 3. %StatusIcon resolves (nested under Container)
// ===========================================================================

#[test]
fn xm0_fixture_status_icon_resolves() {
    let (tree, scene_root) = load_fixture();

    let si = tree
        .get_node_relative(scene_root, "%StatusIcon")
        .expect("%StatusIcon should resolve");
    assert_eq!(tree.get_node(si).unwrap().name(), "StatusIcon");
    assert_eq!(tree.get_node(si).unwrap().class_name(), "TextureRect");
}

// ===========================================================================
// 4. Normal paths still work alongside %UniqueName
// ===========================================================================

#[test]
fn xm0_fixture_normal_paths_still_work() {
    let (tree, scene_root) = load_fixture();

    // Normal child access
    let panel = tree.get_node_relative(scene_root, "Panel").unwrap();
    assert_eq!(tree.get_node(panel).unwrap().name(), "Panel");

    // Normal nested path
    let sl_via_path = tree
        .get_node_relative(scene_root, "Panel/ScoreLabel")
        .unwrap();
    let sl_via_unique = tree.get_node_relative(scene_root, "%ScoreLabel").unwrap();
    assert_eq!(
        sl_via_path, sl_via_unique,
        "both paths should reach the same node"
    );
}

// ===========================================================================
// 5. Non-unique node not resolved via %
// ===========================================================================

#[test]
fn xm0_fixture_nonunique_not_resolved_via_percent() {
    let (tree, scene_root) = load_fixture();

    // Panel is NOT unique — %Panel should return None
    assert!(
        tree.get_node_relative(scene_root, "%Panel").is_none(),
        "Panel is not a unique node, %Panel should not resolve"
    );
    // Container is NOT unique
    assert!(tree.get_node_relative(scene_root, "%Container").is_none());
}
