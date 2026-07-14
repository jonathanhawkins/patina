//! PackedScene instancing ownership and unique-name tests (pat-ooe).
//!
//! Validates that instanced scenes integrate correctly into a SceneTree:
//! ownership, parent references, property independence, unique names,
//! and nested instancing.

use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;
use gdvariant::Variant;

// ===========================================================================
// Fixture scenes
// ===========================================================================

const PLAYER_SCENE: &str = r#"[gd_scene format=3 uid="uid://player"]

[node name="Player" type="Node2D"]
position = Vector2(100, 200)
speed = 300

[node name="Sprite" type="Sprite2D" parent="."]
visible = true

[node name="CollisionShape" type="CollisionShape2D" parent="."]
"#;

const ENEMY_SCENE: &str = r#"[gd_scene format=3 uid="uid://enemy"]

[node name="Enemy" type="Node2D"]
position = Vector2(400, 100)
health = 50

[node name="Sprite" type="Sprite2D" parent="."]

[node name="AI" type="Node" parent="."]
"#;

const UNIQUE_NAME_SCENE: &str = r#"[gd_scene format=3]

[node name="UI" type="Control"]

[node name="%HealthBar" type="ProgressBar" parent="."]
value = 100

[node name="%ScoreLabel" type="Label" parent="."]

[node name="Container" type="VBoxContainer" parent="."]

[node name="%ItemList" type="ItemList" parent="Container"]
"#;

// ===========================================================================
// 1. Instance root becomes child of parent
// ===========================================================================

#[test]
fn instance_root_becomes_child_of_parent() {
    let scene = PackedScene::from_tscn(PLAYER_SCENE).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let player_id = add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();

    // Player should be a child of root.
    let root_node = tree.get_node(root).unwrap();
    assert!(
        root_node.children().contains(&player_id),
        "instanced root should be child of parent"
    );

    // Player's parent should be root.
    let player_node = tree.get_node(player_id).unwrap();
    assert_eq!(player_node.parent(), Some(root));
    assert_eq!(player_node.name(), "Player");
}

#[test]
fn instance_children_have_correct_parents() {
    let scene = PackedScene::from_tscn(PLAYER_SCENE).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let player_id = add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();

    // Player should have 2 children: Sprite and CollisionShape.
    let player_node = tree.get_node(player_id).unwrap();
    assert_eq!(player_node.children().len(), 2);

    // Find Sprite and CollisionShape.
    let sprite_id = tree.get_node_relative(player_id, "Sprite").unwrap();
    let collision_id = tree.get_node_relative(player_id, "CollisionShape").unwrap();

    assert_eq!(tree.get_node(sprite_id).unwrap().parent(), Some(player_id));
    assert_eq!(
        tree.get_node(collision_id).unwrap().parent(),
        Some(player_id)
    );
}

// ===========================================================================
// 2. Instance same scene twice → independent copies
// ===========================================================================

#[test]
fn two_instances_are_independent() {
    let scene = PackedScene::from_tscn(PLAYER_SCENE).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let player1_id = add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();
    let player2_id = add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();

    // Different node IDs.
    assert_ne!(player1_id, player2_id);

    // Both are children of root.
    let root_node = tree.get_node(root).unwrap();
    assert!(root_node.children().contains(&player1_id));
    assert!(root_node.children().contains(&player2_id));
}

#[test]
fn modifying_one_instance_does_not_affect_other() {
    let scene = PackedScene::from_tscn(PLAYER_SCENE).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let player1_id = add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();
    let player2_id = add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();

    // Modify player1's speed.
    tree.get_node_mut(player1_id)
        .unwrap()
        .set_property("speed", Variant::Int(999));

    // player2 should still have original speed.
    assert_eq!(
        tree.get_node(player2_id).unwrap().get_property("speed"),
        Variant::Int(300),
        "modifying one instance should not affect the other"
    );

    // player1 should have the new speed.
    assert_eq!(
        tree.get_node(player1_id).unwrap().get_property("speed"),
        Variant::Int(999)
    );
}

#[test]
fn two_instances_have_independent_children() {
    let scene = PackedScene::from_tscn(PLAYER_SCENE).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let p1 = add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();
    let p2 = add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();

    let sprite1 = tree.get_node_relative(p1, "Sprite").unwrap();
    let sprite2 = tree.get_node_relative(p2, "Sprite").unwrap();

    // Different nodes.
    assert_ne!(sprite1, sprite2);

    // Each Sprite's parent is its own Player instance.
    assert_eq!(tree.get_node(sprite1).unwrap().parent(), Some(p1));
    assert_eq!(tree.get_node(sprite2).unwrap().parent(), Some(p2));
}

// ===========================================================================
// 3. Unique names (% prefix)
// ===========================================================================

#[test]
fn unique_name_flag_preserved_on_instance() {
    let scene = PackedScene::from_tscn(UNIQUE_NAME_SCENE).unwrap();
    let nodes = scene.instance().unwrap();

    // HealthBar should have unique_name flag.
    let health_bar = nodes.iter().find(|n| n.name() == "HealthBar").unwrap();
    assert!(
        health_bar.is_unique_name(),
        "HealthBar should have unique_name flag"
    );

    let score_label = nodes.iter().find(|n| n.name() == "ScoreLabel").unwrap();
    assert!(score_label.is_unique_name());

    let item_list = nodes.iter().find(|n| n.name() == "ItemList").unwrap();
    assert!(item_list.is_unique_name());

    // Container should NOT have unique_name flag.
    let container = nodes.iter().find(|n| n.name() == "Container").unwrap();
    assert!(!container.is_unique_name());
}

#[test]
fn unique_name_preserved_in_tree() {
    let scene = PackedScene::from_tscn(UNIQUE_NAME_SCENE).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let ui_id = add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();

    // HealthBar is accessible via normal path.
    let health_bar_id = tree.get_node_relative(ui_id, "HealthBar").unwrap();
    let health_bar = tree.get_node(health_bar_id).unwrap();
    assert!(health_bar.is_unique_name());
    assert_eq!(health_bar.get_property("value"), Variant::Int(100));

    // ItemList is nested under Container.
    let item_list_id = tree.get_node_relative(ui_id, "Container/ItemList").unwrap();
    let item_list = tree.get_node(item_list_id).unwrap();
    assert!(item_list.is_unique_name());
}

// ===========================================================================
// 4. Nested instancing: scene with instance reference
// ===========================================================================

#[test]
fn instance_with_ext_resource_instance_attribute() {
    // Scene that references another scene via instance=ExtResource("id").
    let tscn = r#"[gd_scene format=3]

[ext_resource type="PackedScene" path="res://enemy.tscn" id="1_enemy"]

[node name="Level" type="Node2D"]

[node name="Spawn1" type="Node2D" parent="." instance=ExtResource("1_enemy")]
"#;
    let scene = PackedScene::from_tscn(tscn).unwrap();
    assert_eq!(scene.node_count(), 2);

    let nodes = scene.instance().unwrap();
    let spawn = &nodes[1];
    assert_eq!(spawn.name(), "Spawn1");
    // Instance attribute stored as _instance property.
    assert_eq!(
        spawn.get_property("_instance"),
        Variant::String("ExtResource(\"1_enemy\")".into())
    );
}

#[test]
fn two_different_scenes_instanced_under_same_parent() {
    let player = PackedScene::from_tscn(PLAYER_SCENE).unwrap();
    let enemy = PackedScene::from_tscn(ENEMY_SCENE).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let player_id = add_packed_scene_to_tree(&mut tree, root, &player).unwrap();
    let enemy_id = add_packed_scene_to_tree(&mut tree, root, &enemy).unwrap();

    assert_ne!(player_id, enemy_id);

    let player_node = tree.get_node(player_id).unwrap();
    let enemy_node = tree.get_node(enemy_id).unwrap();

    assert_eq!(player_node.name(), "Player");
    assert_eq!(enemy_node.name(), "Enemy");

    // Both have Sprite children but they're independent.
    let player_sprite = tree.get_node_relative(player_id, "Sprite").unwrap();
    let enemy_sprite = tree.get_node_relative(enemy_id, "Sprite").unwrap();
    assert_ne!(player_sprite, enemy_sprite);
}

// ===========================================================================
// 5. Properties persist independently per instance
// ===========================================================================

#[test]
fn instanced_properties_match_scene() {
    let scene = PackedScene::from_tscn(PLAYER_SCENE).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let player_id = add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();

    let player = tree.get_node(player_id).unwrap();
    assert_eq!(
        player.get_property("position"),
        Variant::Vector2(gdcore::math::Vector2::new(100.0, 200.0))
    );
    assert_eq!(player.get_property("speed"), Variant::Int(300));
}

#[test]
fn properties_independent_across_three_instances() {
    let scene = PackedScene::from_tscn(ENEMY_SCENE).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let e1 = add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();
    let e2 = add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();
    let e3 = add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();

    // Modify each enemy's health differently.
    tree.get_node_mut(e1)
        .unwrap()
        .set_property("health", Variant::Int(10));
    tree.get_node_mut(e2)
        .unwrap()
        .set_property("health", Variant::Int(25));
    // e3 keeps default.

    assert_eq!(
        tree.get_node(e1).unwrap().get_property("health"),
        Variant::Int(10)
    );
    assert_eq!(
        tree.get_node(e2).unwrap().get_property("health"),
        Variant::Int(25)
    );
    assert_eq!(
        tree.get_node(e3).unwrap().get_property("health"),
        Variant::Int(50)
    );
}

// ===========================================================================
// 6. Instanced scene children have correct parent() references
// ===========================================================================

#[test]
fn all_children_parent_refs_correct() {
    let scene = PackedScene::from_tscn(PLAYER_SCENE).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let player_id = add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();

    // Walk all nodes under player and verify parent references.
    let player = tree.get_node(player_id).unwrap();
    for &child_id in player.children() {
        let child = tree.get_node(child_id).unwrap();
        assert_eq!(
            child.parent(),
            Some(player_id),
            "child '{}' should have Player as parent",
            child.name()
        );
    }
}

#[test]
fn deeply_nested_scene_parent_refs() {
    let tscn = r#"[gd_scene format=3]

[node name="Root" type="Node"]

[node name="A" type="Node2D" parent="."]

[node name="B" type="Node2D" parent="A"]

[node name="C" type="Sprite2D" parent="A/B"]
"#;
    let scene = PackedScene::from_tscn(tscn).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let scene_root = add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();

    let a_id = tree.get_node_relative(scene_root, "A").unwrap();
    let b_id = tree.get_node_relative(scene_root, "A/B").unwrap();
    let c_id = tree.get_node_relative(scene_root, "A/B/C").unwrap();

    assert_eq!(tree.get_node(a_id).unwrap().parent(), Some(scene_root));
    assert_eq!(tree.get_node(b_id).unwrap().parent(), Some(a_id));
    assert_eq!(tree.get_node(c_id).unwrap().parent(), Some(b_id));
}

#[test]
fn instance_root_is_not_its_own_parent() {
    let scene = PackedScene::from_tscn(PLAYER_SCENE).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let player_id = add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();

    let player = tree.get_node(player_id).unwrap();
    assert_ne!(
        player.parent(),
        Some(player_id),
        "instance root should not be its own parent"
    );
    assert_eq!(player.parent(), Some(root));
}

// ===========================================================================
// 7. Multiple instances under nested parents
// ===========================================================================

#[test]
fn instance_under_instanced_child() {
    let parent_scene = PackedScene::from_tscn(
        r#"[gd_scene format=3]

[node name="World" type="Node2D"]

[node name="SpawnPoint" type="Node2D" parent="."]
"#,
    )
    .unwrap();

    let child_scene = PackedScene::from_tscn(ENEMY_SCENE).unwrap();

    let mut tree = SceneTree::new();
    let root = tree.root_id();

    // Instance the world.
    let world_id = add_packed_scene_to_tree(&mut tree, root, &parent_scene).unwrap();
    let spawn_id = tree.get_node_relative(world_id, "SpawnPoint").unwrap();

    // Instance an enemy under SpawnPoint.
    let enemy_id = add_packed_scene_to_tree(&mut tree, spawn_id, &child_scene).unwrap();

    // Enemy should be child of SpawnPoint.
    assert_eq!(tree.get_node(enemy_id).unwrap().parent(), Some(spawn_id));
    assert!(tree
        .get_node(spawn_id)
        .unwrap()
        .children()
        .contains(&enemy_id));

    // Full path traversal should work.
    let found = tree
        .get_node_relative(world_id, "SpawnPoint/Enemy")
        .unwrap();
    assert_eq!(found, enemy_id);
}
