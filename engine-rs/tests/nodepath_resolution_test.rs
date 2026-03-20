//! pat-b2h: NodePath resolution fixtures for absolute, relative, and unique paths.
//!
//! Validates that SceneTree path resolution works for:
//! 1. Absolute paths (/root/Player)
//! 2. Relative paths (../Sibling, Player/Sprite)
//! 3. Unique name paths (%Player) — documented gap
//! 4. Missing/invalid paths return None

use gdscene::node::Node;
use gdscene::scene_tree::SceneTree;

// ===========================================================================
// Helper: builds a tree shaped like:
//   root
//   ├── Player (Node2D)
//   │   ├── Sprite (Sprite2D)
//   │   └── CollisionShape (CollisionShape2D)
//   ├── Enemy (Node2D)
//   │   ├── Sprite (Sprite2D)
//   │   └── AI (Node)
//   └── UI (Control)
//       ├── HealthBar (ProgressBar) [unique]
//       └── Container (VBoxContainer)
//           └── ScoreLabel (Label) [unique]
// ===========================================================================

struct TestTree {
    tree: SceneTree,
    root: gdscene::NodeId,
    player: gdscene::NodeId,
    player_sprite: gdscene::NodeId,
    player_collision: gdscene::NodeId,
    enemy: gdscene::NodeId,
    enemy_sprite: gdscene::NodeId,
    enemy_ai: gdscene::NodeId,
    ui: gdscene::NodeId,
    health_bar: gdscene::NodeId,
    container: gdscene::NodeId,
    score_label: gdscene::NodeId,
}

fn build_test_tree() -> TestTree {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    // Player subtree
    let player = tree.add_child(root, Node::new("Player", "Node2D")).unwrap();
    let player_sprite = tree
        .add_child(player, Node::new("Sprite", "Sprite2D"))
        .unwrap();
    let player_collision = tree
        .add_child(player, Node::new("CollisionShape", "CollisionShape2D"))
        .unwrap();

    // Enemy subtree
    let enemy = tree.add_child(root, Node::new("Enemy", "Node2D")).unwrap();
    let enemy_sprite = tree
        .add_child(enemy, Node::new("Sprite", "Sprite2D"))
        .unwrap();
    let enemy_ai = tree.add_child(enemy, Node::new("AI", "Node")).unwrap();

    // UI subtree
    let ui = tree.add_child(root, Node::new("UI", "Control")).unwrap();

    let mut health_bar_node = Node::new("HealthBar", "ProgressBar");
    health_bar_node.set_unique_name(true);
    let health_bar = tree.add_child(ui, health_bar_node).unwrap();

    let container = tree
        .add_child(ui, Node::new("Container", "VBoxContainer"))
        .unwrap();

    let mut score_label_node = Node::new("ScoreLabel", "Label");
    score_label_node.set_unique_name(true);
    let score_label = tree.add_child(container, score_label_node).unwrap();

    TestTree {
        tree,
        root,
        player,
        player_sprite,
        player_collision,
        enemy,
        enemy_sprite,
        enemy_ai,
        ui,
        health_bar,
        container,
        score_label,
    }
}

// ===========================================================================
// 1. Absolute path resolution (/root/...)
// ===========================================================================

#[test]
fn b2h_absolute_path_to_root() {
    let t = build_test_tree();
    let found = t.tree.get_node_by_path("/root").unwrap();
    assert_eq!(found, t.root);
}

#[test]
fn b2h_absolute_path_to_direct_child() {
    let t = build_test_tree();
    assert_eq!(t.tree.get_node_by_path("/root/Player").unwrap(), t.player);
    assert_eq!(t.tree.get_node_by_path("/root/Enemy").unwrap(), t.enemy);
    assert_eq!(t.tree.get_node_by_path("/root/UI").unwrap(), t.ui);
}

#[test]
fn b2h_absolute_path_to_nested_child() {
    let t = build_test_tree();
    assert_eq!(
        t.tree.get_node_by_path("/root/Player/Sprite").unwrap(),
        t.player_sprite
    );
    assert_eq!(
        t.tree
            .get_node_by_path("/root/Player/CollisionShape")
            .unwrap(),
        t.player_collision
    );
    assert_eq!(
        t.tree.get_node_by_path("/root/Enemy/AI").unwrap(),
        t.enemy_ai
    );
}

#[test]
fn b2h_absolute_path_deeply_nested() {
    let t = build_test_tree();
    assert_eq!(
        t.tree
            .get_node_by_path("/root/UI/Container/ScoreLabel")
            .unwrap(),
        t.score_label
    );
}

#[test]
fn b2h_absolute_path_missing_returns_none() {
    let t = build_test_tree();
    assert!(t.tree.get_node_by_path("/root/NonExistent").is_none());
    assert!(t.tree.get_node_by_path("/root/Player/Missing").is_none());
    assert!(t
        .tree
        .get_node_by_path("/root/UI/Container/Missing")
        .is_none());
}

#[test]
fn b2h_absolute_path_wrong_root_name() {
    let t = build_test_tree();
    assert!(t.tree.get_node_by_path("/wrong_root/Player").is_none());
}

#[test]
fn b2h_absolute_path_not_starting_with_slash() {
    let t = build_test_tree();
    assert!(t.tree.get_node_by_path("root/Player").is_none());
}

// ===========================================================================
// 2. Relative path resolution (., .., Name, A/B)
// ===========================================================================

#[test]
fn b2h_relative_dot_returns_self() {
    let t = build_test_tree();
    assert_eq!(t.tree.get_node_relative(t.player, ".").unwrap(), t.player);
}

#[test]
fn b2h_relative_empty_returns_self() {
    let t = build_test_tree();
    assert_eq!(t.tree.get_node_relative(t.player, "").unwrap(), t.player);
}

#[test]
fn b2h_relative_dotdot_returns_parent() {
    let t = build_test_tree();
    assert_eq!(
        t.tree.get_node_relative(t.player_sprite, "..").unwrap(),
        t.player
    );
    assert_eq!(t.tree.get_node_relative(t.player, "..").unwrap(), t.root);
}

#[test]
fn b2h_relative_child_by_name() {
    let t = build_test_tree();
    assert_eq!(
        t.tree.get_node_relative(t.player, "Sprite").unwrap(),
        t.player_sprite
    );
    assert_eq!(t.tree.get_node_relative(t.enemy, "AI").unwrap(), t.enemy_ai);
}

#[test]
fn b2h_relative_multi_segment_path() {
    let t = build_test_tree();
    assert_eq!(
        t.tree
            .get_node_relative(t.ui, "Container/ScoreLabel")
            .unwrap(),
        t.score_label
    );
}

#[test]
fn b2h_relative_sibling_via_dotdot() {
    let t = build_test_tree();
    // From Player/Sprite, go to Player/CollisionShape via ../CollisionShape
    assert_eq!(
        t.tree
            .get_node_relative(t.player_sprite, "../CollisionShape")
            .unwrap(),
        t.player_collision
    );
}

#[test]
fn b2h_relative_uncle_via_double_dotdot() {
    let t = build_test_tree();
    // From Player/Sprite, go to Enemy via ../../Enemy
    assert_eq!(
        t.tree
            .get_node_relative(t.player_sprite, "../../Enemy")
            .unwrap(),
        t.enemy
    );
}

#[test]
fn b2h_relative_cousin_via_ancestor() {
    let t = build_test_tree();
    // From Player/Sprite, navigate to Enemy/AI via ../../Enemy/AI
    assert_eq!(
        t.tree
            .get_node_relative(t.player_sprite, "../../Enemy/AI")
            .unwrap(),
        t.enemy_ai
    );
}

#[test]
fn b2h_relative_missing_child_returns_none() {
    let t = build_test_tree();
    assert!(t.tree.get_node_relative(t.player, "Nonexistent").is_none());
}

#[test]
fn b2h_relative_dotdot_past_root_returns_none() {
    let t = build_test_tree();
    assert!(t.tree.get_node_relative(t.root, "..").is_none());
}

// ===========================================================================
// 3. get_node_or_null: routes absolute vs relative
// ===========================================================================

#[test]
fn b2h_get_node_or_null_absolute() {
    let t = build_test_tree();
    assert_eq!(
        t.tree.get_node_or_null(t.player, "/root/Enemy").unwrap(),
        t.enemy
    );
}

#[test]
fn b2h_get_node_or_null_relative() {
    let t = build_test_tree();
    assert_eq!(
        t.tree.get_node_or_null(t.player, "Sprite").unwrap(),
        t.player_sprite
    );
}

#[test]
fn b2h_get_node_or_null_missing_returns_none() {
    let t = build_test_tree();
    assert!(t.tree.get_node_or_null(t.player, "/root/Missing").is_none());
    assert!(t.tree.get_node_or_null(t.player, "Missing").is_none());
}

// ===========================================================================
// 4. Unique name resolution (%Name) — documented gap
// ===========================================================================

#[test]
fn b2h_unique_name_flag_preserved_in_tree() {
    let t = build_test_tree();
    let health_bar = t.tree.get_node(t.health_bar).unwrap();
    assert!(
        health_bar.is_unique_name(),
        "HealthBar should have unique_name flag set"
    );
    let score_label = t.tree.get_node(t.score_label).unwrap();
    assert!(
        score_label.is_unique_name(),
        "ScoreLabel should have unique_name flag set"
    );
    // Container should NOT be unique
    let container = t.tree.get_node(t.container).unwrap();
    assert!(!container.is_unique_name());
}

#[test]
fn b2h_unique_name_accessible_via_normal_path() {
    let t = build_test_tree();
    // Unique-name nodes are still accessible via normal child lookup.
    assert_eq!(
        t.tree.get_node_relative(t.ui, "HealthBar").unwrap(),
        t.health_bar
    );
    assert_eq!(
        t.tree
            .get_node_relative(t.ui, "Container/ScoreLabel")
            .unwrap(),
        t.score_label
    );
}

#[test]
fn b2h_unique_name_percent_prefix_not_yet_resolved() {
    let t = build_test_tree();
    // In Godot, %HealthBar resolves to the unique-name node from any point
    // within the scene owner. This is NOT yet implemented in Patina.
    // Verify the gap: %HealthBar should return None since find_child_by_name
    // looks for literal "%HealthBar" child name, which doesn't exist.
    let result = t.tree.get_node_relative(t.ui, "%HealthBar");
    assert!(
        result.is_none(),
        "KNOWN GAP: %%Name unique-name resolution is not yet implemented. \
         Godot resolves %%Name by searching for unique-named nodes within the scene owner."
    );
}

// ===========================================================================
// 5. node_path() — computes absolute path from NodeId
// ===========================================================================

#[test]
fn b2h_node_path_root() {
    let t = build_test_tree();
    assert_eq!(t.tree.node_path(t.root).unwrap(), "/root");
}

#[test]
fn b2h_node_path_direct_child() {
    let t = build_test_tree();
    assert_eq!(t.tree.node_path(t.player).unwrap(), "/root/Player");
}

#[test]
fn b2h_node_path_deeply_nested() {
    let t = build_test_tree();
    assert_eq!(
        t.tree.node_path(t.score_label).unwrap(),
        "/root/UI/Container/ScoreLabel"
    );
}

#[test]
fn b2h_node_path_nonexistent_returns_none() {
    let t = build_test_tree();
    let bogus = gdscene::NodeId::next();
    assert!(t.tree.node_path(bogus).is_none());
}

// ===========================================================================
// 6. Same-name children under different parents resolve independently
// ===========================================================================

#[test]
fn b2h_same_name_children_different_parents() {
    let t = build_test_tree();
    // Both Player and Enemy have a child named "Sprite"
    let player_sprite = t.tree.get_node_relative(t.player, "Sprite").unwrap();
    let enemy_sprite = t.tree.get_node_relative(t.enemy, "Sprite").unwrap();

    assert_ne!(player_sprite, enemy_sprite);
    assert_eq!(player_sprite, t.player_sprite);
    assert_eq!(enemy_sprite, t.enemy_sprite);
}

// ===========================================================================
// 7. PackedScene: paths resolve correctly after instancing
// ===========================================================================

#[test]
fn b2h_tscn_instanced_paths_resolve() {
    use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};

    let tscn = r#"[gd_scene format=3]

[node name="Level" type="Node2D"]

[node name="Player" type="CharacterBody2D" parent="."]

[node name="Sprite" type="Sprite2D" parent="Player"]

[node name="Camera" type="Camera2D" parent="Player"]
"#;
    let scene = PackedScene::from_tscn(tscn).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let level_id = add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();

    // Absolute path
    assert!(tree.get_node_by_path("/root/Level/Player/Sprite").is_some());
    assert!(tree.get_node_by_path("/root/Level/Player/Camera").is_some());

    // Relative from level
    let player = tree.get_node_relative(level_id, "Player").unwrap();
    let sprite = tree.get_node_relative(player, "Sprite").unwrap();
    let camera = tree.get_node_relative(player, "Camera").unwrap();

    // Sibling navigation
    assert_eq!(tree.get_node_relative(sprite, "../Camera").unwrap(), camera);
    assert_eq!(tree.get_node_relative(camera, "../Sprite").unwrap(), sprite);
}
