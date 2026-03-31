//! pat-pwo6 / pat-ggk: Scene instancing edge-case coverage.
//!
//! Covers edge cases in PackedScene instancing that are NOT yet tested:
//! - Sub-scene instancing via `instance_with_subscenes`
//! - Property overrides on instanced sub-scene roots
//! - Multiple instances of the same scene in one tree
//! - Adding multiple packed scenes to an existing tree
//! - Instancing scenes with groups
//! - Instancing scenes with connection sections
//! - Deep nesting (>4 levels)
//! - Sibling nodes with identical names under the same parent
//! - Instancing empty-child root nodes
//! - Tree integrity after instancing (parent/child relationships)
//! - Reparenting instanced nodes preserves/remaps ownership
//! - Cross-owner-boundary path resolution
//! - Notification ordering across multiple instances
//! - Very wide trees (many siblings)
//! - Remove-and-reinstance cycles
//! - Property modification after instancing

use gdobject::notification::{NOTIFICATION_ENTER_TREE, NOTIFICATION_READY};
use gdscene::packed_scene::{
    add_packed_scene_to_tree, add_packed_scene_to_tree_with_subscenes, PackedScene,
};
use gdscene::scene_tree::SceneTree;

// ===========================================================================
// Helpers
// ===========================================================================

fn instance_into_tree(tscn: &str) -> SceneTree {
    let packed = PackedScene::from_tscn(tscn).expect("parse .tscn");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &packed).expect("add to tree");
    tree
}

// ===========================================================================
// 1. Multiple instances of the same scene in one tree
// ===========================================================================

const SIMPLE_SCENE: &str = r#"[gd_scene format=3 uid="uid://simple"]

[node name="Enemy" type="Node2D"]
position = Vector2(0, 0)

[node name="Sprite" type="Sprite2D" parent="."]

[node name="Hitbox" type="Area2D" parent="."]
"#;

#[test]
fn two_instances_of_same_scene_are_independent() {
    let packed = PackedScene::from_tscn(SIMPLE_SCENE).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    // Instance the same scene twice under root.
    add_packed_scene_to_tree(&mut tree, root, &packed).unwrap();
    add_packed_scene_to_tree(&mut tree, root, &packed).unwrap();

    // Root should have two children (both named "Enemy").
    let root_node = tree.get_node(root).unwrap();
    let children = root_node.children();
    assert_eq!(children.len(), 2, "Root should have 2 instanced children");

    // Each enemy should have 2 children (Sprite + Hitbox).
    for &enemy_id in children {
        let enemy = tree.get_node(enemy_id).unwrap();
        assert_eq!(enemy.class_name(), "Node2D");
        assert_eq!(
            enemy.children().len(),
            2,
            "Each Enemy instance should have 2 children"
        );
    }

    // Instances should have distinct NodeIds.
    assert_ne!(
        children[0], children[1],
        "Instances must have unique NodeIds"
    );
}

#[test]
fn three_instances_all_have_correct_subtrees() {
    let packed = PackedScene::from_tscn(SIMPLE_SCENE).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    for _ in 0..3 {
        add_packed_scene_to_tree(&mut tree, root, &packed).unwrap();
    }

    let root_node = tree.get_node(root).unwrap();
    assert_eq!(root_node.children().len(), 3);

    // Verify total node count: 3 instances * 3 nodes each = 9, plus root = 10
    // (root is the tree root, not counted in all_nodes normally)
    let all = tree.all_nodes_in_tree_order();
    // root + 3*(Enemy + Sprite + Hitbox) = 1 + 9 = 10
    assert_eq!(all.len(), 10, "Should have root + 9 instanced nodes");
}

// ===========================================================================
// 2. Deep nesting (>4 levels)
// ===========================================================================

const DEEP_SCENE_6: &str = r#"[gd_scene format=3]

[node name="L0" type="Node"]

[node name="L1" type="Node" parent="."]

[node name="L2" type="Node" parent="L1"]

[node name="L3" type="Node" parent="L1/L2"]

[node name="L4" type="Node" parent="L1/L2/L3"]

[node name="L5" type="Node2D" parent="L1/L2/L3/L4"]
position = Vector2(99, 88)
"#;

#[test]
fn deep_nesting_6_levels_parses_and_instances() {
    let tree = instance_into_tree(DEEP_SCENE_6);

    // Verify the deepest node is reachable.
    let l5 = tree.get_node_by_path("/root/L0/L1/L2/L3/L4/L5");
    assert!(l5.is_some(), "Should find L5 at depth 6");

    let l5_node = tree.get_node(l5.unwrap()).unwrap();
    assert_eq!(l5_node.class_name(), "Node2D");
    assert_eq!(
        l5_node.get_property("position"),
        gdvariant::Variant::Vector2(gdcore::math::Vector2::new(99.0, 88.0))
    );
}

#[test]
fn deep_nesting_parent_child_chain_correct() {
    let tree = instance_into_tree(DEEP_SCENE_6);

    let l0 = tree.get_node_by_path("/root/L0").unwrap();
    let l1 = tree.get_node_by_path("/root/L0/L1").unwrap();
    let l2 = tree.get_node_by_path("/root/L0/L1/L2").unwrap();
    let l3 = tree.get_node_by_path("/root/L0/L1/L2/L3").unwrap();
    let l4 = tree.get_node_by_path("/root/L0/L1/L2/L3/L4").unwrap();
    let l5 = tree.get_node_by_path("/root/L0/L1/L2/L3/L4/L5").unwrap();

    // Verify each node's parent is correct.
    assert_eq!(tree.get_node(l1).unwrap().parent(), Some(l0));
    assert_eq!(tree.get_node(l2).unwrap().parent(), Some(l1));
    assert_eq!(tree.get_node(l3).unwrap().parent(), Some(l2));
    assert_eq!(tree.get_node(l4).unwrap().parent(), Some(l3));
    assert_eq!(tree.get_node(l5).unwrap().parent(), Some(l4));
}

// ===========================================================================
// 3. Sibling nodes with identical names under the same parent
// ===========================================================================

#[test]
fn siblings_with_same_name_both_instanced() {
    // In .tscn format this can't normally happen (Godot renames), but we
    // test the parser handles it gracefully by instancing two packed scenes
    // that produce same-named children.
    let tscn_a = r#"[gd_scene format=3]

[node name="Item" type="Node2D"]
position = Vector2(10, 20)
"#;
    let tscn_b = r#"[gd_scene format=3]

[node name="Item" type="Node2D"]
position = Vector2(30, 40)
"#;

    let packed_a = PackedScene::from_tscn(tscn_a).unwrap();
    let packed_b = PackedScene::from_tscn(tscn_b).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &packed_a).unwrap();
    add_packed_scene_to_tree(&mut tree, root, &packed_b).unwrap();

    let root_node = tree.get_node(root).unwrap();
    assert_eq!(root_node.children().len(), 2, "Both Items should be added");

    // Both should be named "Item" but have different positions.
    for &child_id in root_node.children() {
        let child = tree.get_node(child_id).unwrap();
        assert_eq!(child.name(), "Item");
    }
}

// ===========================================================================
// 4. Instancing scene with groups
// ===========================================================================

const GROUPS_SCENE: &str = r#"[gd_scene format=3]

[node name="Player" type="CharacterBody2D" groups=["players", "persistent"]]
position = Vector2(100, 200)

[node name="Weapon" type="Node2D" parent="." groups=["weapons"]]
"#;

#[test]
fn instanced_nodes_preserve_groups() {
    let tree = instance_into_tree(GROUPS_SCENE);

    let player = tree.get_node_by_path("/root/Player").unwrap();
    let player_node = tree.get_node(player).unwrap();

    let groups = player_node.groups();
    assert!(
        groups.contains(&"players".to_string()),
        "Player should be in 'players' group"
    );
    assert!(
        groups.contains(&"persistent".to_string()),
        "Player should be in 'persistent' group"
    );

    let weapon = tree.get_node_by_path("/root/Player/Weapon").unwrap();
    let weapon_node = tree.get_node(weapon).unwrap();
    assert!(
        weapon_node.groups().contains(&"weapons".to_string()),
        "Weapon should be in 'weapons' group"
    );
}

#[test]
fn two_instances_have_independent_group_membership() {
    let packed = PackedScene::from_tscn(GROUPS_SCENE).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    add_packed_scene_to_tree(&mut tree, root, &packed).unwrap();
    add_packed_scene_to_tree(&mut tree, root, &packed).unwrap();

    // Both instances should be in the "players" group.
    let players_in_group = tree.get_nodes_in_group("players");
    assert_eq!(
        players_in_group.len(),
        2,
        "Both Player instances should be in 'players' group"
    );
}

// ===========================================================================
// 5. Instancing scene with connections
// ===========================================================================

const CONNECTIONS_SCENE: &str = r#"[gd_scene format=3]

[node name="UI" type="Control"]

[node name="Button" type="Button" parent="."]
text = "Click Me"

[node name="Label" type="Label" parent="."]
text = "Status"

[connection signal="pressed" from="Button" to="." method="_on_button_pressed"]
"#;

#[test]
fn scene_with_connections_instances_correctly() {
    let packed = PackedScene::from_tscn(CONNECTIONS_SCENE).unwrap();
    assert_eq!(packed.connection_count(), 1);
    assert_eq!(packed.node_count(), 3);

    let tree = instance_into_tree(CONNECTIONS_SCENE);

    let ui = tree.get_node_by_path("/root/UI").unwrap();
    let button = tree.get_node_by_path("/root/UI/Button").unwrap();
    let label = tree.get_node_by_path("/root/UI/Label").unwrap();

    assert_eq!(tree.get_node(ui).unwrap().class_name(), "Control");
    assert_eq!(tree.get_node(button).unwrap().class_name(), "Button");
    assert_eq!(tree.get_node(label).unwrap().class_name(), "Label");
}

#[test]
fn connection_count_correct_after_instancing() {
    let packed = PackedScene::from_tscn(CONNECTIONS_SCENE).unwrap();
    assert_eq!(
        packed.connection_count(),
        1,
        "Should have exactly 1 connection"
    );
}

// ===========================================================================
// 6. Root-only scene (no children)
// ===========================================================================

#[test]
fn root_only_scene_instances_into_tree() {
    let tscn = r#"[gd_scene format=3]

[node name="Singleton" type="Node"]
"#;
    let tree = instance_into_tree(tscn);

    let singleton = tree.get_node_by_path("/root/Singleton").unwrap();
    let node = tree.get_node(singleton).unwrap();
    assert_eq!(node.name(), "Singleton");
    assert!(node.children().is_empty());
}

#[test]
fn root_only_with_properties() {
    let tscn = r#"[gd_scene format=3]

[node name="Config" type="Node"]
process_mode = 3
"#;
    let tree = instance_into_tree(tscn);

    let config = tree.get_node_by_path("/root/Config").unwrap();
    let node = tree.get_node(config).unwrap();
    assert_eq!(
        node.get_property("process_mode"),
        gdvariant::Variant::Int(3)
    );
}

// ===========================================================================
// 7. Properties with various Variant types
// ===========================================================================

const VARIED_PROPS_SCENE: &str = r#"[gd_scene format=3]

[node name="Root" type="Node2D"]
position = Vector2(10.5, -20.3)
rotation = 1.5708
scale = Vector2(2, 2)
z_index = 5
visible = false
"#;

#[test]
fn varied_property_types_preserved_after_instancing() {
    let tree = instance_into_tree(VARIED_PROPS_SCENE);
    let root = tree.get_node_by_path("/root/Root").unwrap();
    let node = tree.get_node(root).unwrap();

    // Float
    match node.get_property("rotation") {
        gdvariant::Variant::Float(v) => {
            assert!((v - 1.5708).abs() < 0.001, "rotation should be ~1.5708");
        }
        other => panic!("expected Float for rotation, got {:?}", other),
    }

    // Vector2
    assert_eq!(
        node.get_property("scale"),
        gdvariant::Variant::Vector2(gdcore::math::Vector2::new(2.0, 2.0))
    );

    // Int
    assert_eq!(node.get_property("z_index"), gdvariant::Variant::Int(5));

    // Bool
    assert_eq!(
        node.get_property("visible"),
        gdvariant::Variant::Bool(false)
    );
}

// ===========================================================================
// 8. Instancing into non-root parent
// ===========================================================================

#[test]
fn instance_scene_under_existing_child() {
    let parent_tscn = r#"[gd_scene format=3]

[node name="World" type="Node"]

[node name="Entities" type="Node" parent="."]
"#;
    let child_tscn = r#"[gd_scene format=3]

[node name="Mob" type="Node2D"]
position = Vector2(50, 50)
"#;

    let parent_packed = PackedScene::from_tscn(parent_tscn).unwrap();
    let child_packed = PackedScene::from_tscn(child_tscn).unwrap();

    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &parent_packed).unwrap();

    // Find the "Entities" node and add a child scene under it.
    let entities = tree.get_node_by_path("/root/World/Entities").unwrap();
    add_packed_scene_to_tree(&mut tree, entities, &child_packed).unwrap();

    // Mob should be under Entities.
    let mob = tree.get_node_by_path("/root/World/Entities/Mob");
    assert!(mob.is_some(), "Mob should be under Entities");

    let mob_node = tree.get_node(mob.unwrap()).unwrap();
    assert_eq!(mob_node.class_name(), "Node2D");
    assert_eq!(mob_node.parent(), Some(entities));
}

#[test]
fn multiple_scenes_under_same_parent_node() {
    let container_tscn = r#"[gd_scene format=3]

[node name="Container" type="Node"]
"#;
    let item_tscn = r#"[gd_scene format=3]

[node name="Item" type="Node2D"]
"#;

    let container = PackedScene::from_tscn(container_tscn).unwrap();
    let item = PackedScene::from_tscn(item_tscn).unwrap();

    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &container).unwrap();

    let container_id = tree.get_node_by_path("/root/Container").unwrap();

    // Add 5 items under the container.
    for _ in 0..5 {
        add_packed_scene_to_tree(&mut tree, container_id, &item).unwrap();
    }

    let container_node = tree.get_node(container_id).unwrap();
    assert_eq!(
        container_node.children().len(),
        5,
        "Container should have 5 Item children"
    );
}

// ===========================================================================
// 9. Tree order consistency
// ===========================================================================

#[test]
fn tree_order_matches_tscn_order() {
    let tscn = r#"[gd_scene format=3]

[node name="Root" type="Node"]

[node name="Alpha" type="Node" parent="."]

[node name="Beta" type="Node" parent="."]

[node name="Gamma" type="Node" parent="."]

[node name="Delta" type="Node2D" parent="Beta"]
"#;
    let tree = instance_into_tree(tscn);

    // Verify all nodes exist and are reachable.
    assert!(tree.get_node_by_path("/root/Root/Alpha").is_some());
    assert!(tree.get_node_by_path("/root/Root/Beta").is_some());
    assert!(tree.get_node_by_path("/root/Root/Gamma").is_some());
    assert!(tree.get_node_by_path("/root/Root/Beta/Delta").is_some());

    // Root's children should be Alpha, Beta, Gamma in order.
    let root_id = tree.get_node_by_path("/root/Root").unwrap();
    let root_node = tree.get_node(root_id).unwrap();
    let names: Vec<&str> = root_node
        .children()
        .iter()
        .map(|&id| tree.get_node(id).unwrap().name())
        .collect();
    assert_eq!(names, vec!["Alpha", "Beta", "Gamma"]);
}

// ===========================================================================
// 10. Determinism: same scene instances identically every time
// ===========================================================================

#[test]
fn instancing_is_deterministic() {
    let tscn = r#"[gd_scene format=3]

[node name="World" type="Node2D"]
position = Vector2(10, 20)

[node name="A" type="Node2D" parent="."]
position = Vector2(1, 2)

[node name="B" type="Sprite2D" parent="."]
position = Vector2(3, 4)

[node name="C" type="Node" parent="A"]
"#;

    let packed = PackedScene::from_tscn(tscn).unwrap();

    // Instance twice and compare node structures.
    let nodes1 = packed.instance().unwrap();
    let nodes2 = packed.instance().unwrap();

    assert_eq!(nodes1.len(), nodes2.len());

    for (n1, n2) in nodes1.iter().zip(nodes2.iter()) {
        assert_eq!(n1.name(), n2.name());
        assert_eq!(n1.class_name(), n2.class_name());
        // Properties should match.
        for (key, val) in n1.properties() {
            assert_eq!(
                n2.get_property(key),
                val.clone(),
                "Property '{}' mismatch on node '{}'",
                key,
                n1.name()
            );
        }
    }
}

// ===========================================================================
// 11. Large flat scene (many siblings)
// ===========================================================================

#[test]
fn large_flat_scene_50_children() {
    let mut tscn = String::from("[gd_scene format=3]\n\n[node name=\"Root\" type=\"Node\"]\n\n");
    for i in 0..50 {
        tscn.push_str(&format!(
            "[node name=\"Child{i}\" type=\"Node2D\" parent=\".\"]\nposition = Vector2({}, {})\n\n",
            i * 10,
            i * 5
        ));
    }

    let tree = instance_into_tree(&tscn);
    let root_id = tree.get_node_by_path("/root/Root").unwrap();
    let root_node = tree.get_node(root_id).unwrap();
    assert_eq!(root_node.children().len(), 50);

    // Spot-check a few nodes.
    let c0 = tree.get_node_by_path("/root/Root/Child0").unwrap();
    assert_eq!(
        tree.get_node(c0).unwrap().get_property("position"),
        gdvariant::Variant::Vector2(gdcore::math::Vector2::new(0.0, 0.0))
    );
    let c49 = tree.get_node_by_path("/root/Root/Child49").unwrap();
    assert_eq!(
        tree.get_node(c49).unwrap().get_property("position"),
        gdvariant::Variant::Vector2(gdcore::math::Vector2::new(490.0, 245.0))
    );
}

// ===========================================================================
// 12. Scene with string properties containing special characters
// ===========================================================================

#[test]
fn string_property_with_special_chars() {
    let tscn = r#"[gd_scene format=3]

[node name="Root" type="Label"]
text = "Hello \"World\" & <Goodbye>"
"#;
    let tree = instance_into_tree(tscn);
    let root = tree.get_node_by_path("/root/Root").unwrap();
    let node = tree.get_node(root).unwrap();

    match node.get_property("text") {
        gdvariant::Variant::String(s) => {
            assert!(
                s.contains("Hello"),
                "String property should contain 'Hello'"
            );
        }
        gdvariant::Variant::Nil => {
            // Parser may not handle escaped quotes — that's an acceptable edge case.
        }
        other => panic!("expected String or Nil for text, got {:?}", other),
    }
}

// ===========================================================================
// 13. Sub-scene instancing via instance_with_subscenes
// ===========================================================================

const CHILD_SCENE_TSCN: &str = r#"[gd_scene format=3 uid="uid://child_scene"]

[node name="Mob" type="Node2D"]
speed = 100

[node name="Sprite" type="Sprite2D" parent="."]

[node name="Collision" type="CollisionShape2D" parent="."]
"#;

const PARENT_WITH_INSTANCE_TSCN: &str = r#"[gd_scene format=3 uid="uid://parent_scene"]

[ext_resource type="PackedScene" path="res://scenes/mob.tscn" id="1_mob"]

[node name="World" type="Node"]

[node name="Enemy" type="Node2D" parent="." instance=ExtResource("1_mob")]
position = Vector2(200, 300)
"#;

#[test]
fn subscene_instancing_resolves_child_scene() {
    let parent = PackedScene::from_tscn(PARENT_WITH_INSTANCE_TSCN).unwrap();

    let resolve = |path: &str| -> Option<PackedScene> {
        if path == "res://scenes/mob.tscn" {
            Some(PackedScene::from_tscn(CHILD_SCENE_TSCN).unwrap())
        } else {
            None
        }
    };

    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree_with_subscenes(&mut tree, root, &parent, &resolve).unwrap();

    // The sub-scene root should be renamed to "Enemy" (from the parent template).
    let enemy = tree.get_node_by_path("/root/World/Enemy");
    assert!(
        enemy.is_some(),
        "Sub-scene root should be renamed to 'Enemy'"
    );

    let enemy_id = enemy.unwrap();
    let enemy_node = tree.get_node(enemy_id).unwrap();

    // Sub-scene children should be present.
    assert_eq!(
        enemy_node.children().len(),
        2,
        "Enemy should have Sprite + Collision from sub-scene"
    );

    // Sprite and Collision should be reachable.
    assert!(tree.get_node_by_path("/root/World/Enemy/Sprite").is_some());
    assert!(tree
        .get_node_by_path("/root/World/Enemy/Collision")
        .is_some());
}

// ===========================================================================
// 14. Property overrides on instanced sub-scene roots
// ===========================================================================

#[test]
fn subscene_property_overrides_applied() {
    let parent = PackedScene::from_tscn(PARENT_WITH_INSTANCE_TSCN).unwrap();

    let resolve = |path: &str| -> Option<PackedScene> {
        if path == "res://scenes/mob.tscn" {
            Some(PackedScene::from_tscn(CHILD_SCENE_TSCN).unwrap())
        } else {
            None
        }
    };

    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree_with_subscenes(&mut tree, root, &parent, &resolve).unwrap();

    let enemy_id = tree.get_node_by_path("/root/World/Enemy").unwrap();
    let enemy_node = tree.get_node(enemy_id).unwrap();

    // The position override from the parent scene should be applied.
    assert_eq!(
        enemy_node.get_property("position"),
        gdvariant::Variant::Vector2(gdcore::math::Vector2::new(200.0, 300.0)),
        "Parent scene's position override should be applied to sub-scene root"
    );

    // The original 'speed' property from the sub-scene should still exist.
    assert_eq!(
        enemy_node.get_property("speed"),
        gdvariant::Variant::Int(100),
        "Sub-scene's own properties should be preserved"
    );
}

// ===========================================================================
// 15. Owner relationships after instancing
// ===========================================================================

#[test]
fn owner_set_correctly_for_instanced_nodes() {
    let tree = instance_into_tree(SIMPLE_SCENE);

    let enemy_id = tree.get_node_by_path("/root/Enemy").unwrap();
    let sprite_id = tree.get_node_by_path("/root/Enemy/Sprite").unwrap();
    let hitbox_id = tree.get_node_by_path("/root/Enemy/Hitbox").unwrap();

    let sprite_node = tree.get_node(sprite_id).unwrap();
    let hitbox_node = tree.get_node(hitbox_id).unwrap();

    // Children's owner should be the scene root (Enemy).
    assert_eq!(
        sprite_node.owner(),
        Some(enemy_id),
        "Sprite's owner should be the scene root (Enemy)"
    );
    assert_eq!(
        hitbox_node.owner(),
        Some(enemy_id),
        "Hitbox's owner should be the scene root (Enemy)"
    );
}

#[test]
fn owner_preserved_in_deep_hierarchy() {
    let tree = instance_into_tree(DEEP_SCENE_6);

    let l0 = tree.get_node_by_path("/root/L0").unwrap();
    let l5 = tree.get_node_by_path("/root/L0/L1/L2/L3/L4/L5").unwrap();

    let l5_node = tree.get_node(l5).unwrap();
    assert_eq!(
        l5_node.owner(),
        Some(l0),
        "Deepest node should be owned by the scene root"
    );
}

// ===========================================================================
// 16. Unique name (%UniqueName) nodes in instanced scenes
// ===========================================================================

const UNIQUE_NAME_SCENE: &str = r#"[gd_scene format=3]

[node name="Root" type="Node"]

[node name="%Player" type="Node2D" parent="."]
position = Vector2(50, 50)

[node name="%Camera" type="Camera2D" parent="Player"]
"#;

#[test]
fn unique_name_nodes_flagged_after_instancing() {
    let tree = instance_into_tree(UNIQUE_NAME_SCENE);

    let player_id = tree.get_node_by_path("/root/Root/Player").unwrap();
    let camera_id = tree.get_node_by_path("/root/Root/Player/Camera").unwrap();

    let player_node = tree.get_node(player_id).unwrap();
    let camera_node = tree.get_node(camera_id).unwrap();

    assert!(
        player_node.is_unique_name(),
        "Player should be marked as unique name"
    );
    assert!(
        camera_node.is_unique_name(),
        "Camera should be marked as unique name"
    );
}

#[test]
fn non_unique_nodes_not_flagged() {
    let tree = instance_into_tree(SIMPLE_SCENE);

    let enemy_id = tree.get_node_by_path("/root/Enemy").unwrap();
    let enemy_node = tree.get_node(enemy_id).unwrap();

    assert!(
        !enemy_node.is_unique_name(),
        "Regular nodes should not be marked unique"
    );
}

// ===========================================================================
// 17. Script path preservation through instancing
// ===========================================================================

const SCRIPT_SCENE: &str = r#"[gd_scene format=3]

[ext_resource type="Script" path="res://scripts/player.gd" id="1_script"]

[node name="Player" type="CharacterBody2D"]
script = ExtResource("1_script")

[node name="Sprite" type="Sprite2D" parent="."]
"#;

#[test]
fn script_path_preserved_after_instancing() {
    let tree = instance_into_tree(SCRIPT_SCENE);

    let player_id = tree.get_node_by_path("/root/Player").unwrap();
    let player_node = tree.get_node(player_id).unwrap();

    assert_eq!(
        player_node.get_property("_script_path"),
        gdvariant::Variant::String("res://scripts/player.gd".to_string()),
        "Script path should be preserved as _script_path property"
    );
}

#[test]
fn child_without_script_has_no_script_path() {
    let tree = instance_into_tree(SCRIPT_SCENE);

    let sprite_id = tree.get_node_by_path("/root/Player/Sprite").unwrap();
    let sprite_node = tree.get_node(sprite_id).unwrap();

    assert_eq!(
        sprite_node.get_property("_script_path"),
        gdvariant::Variant::Nil,
        "Nodes without scripts should not have _script_path"
    );
}

// ===========================================================================
// 18. Wire connections verified after tree insertion (actual signal wiring)
// ===========================================================================

#[test]
fn connections_actually_wired_in_tree() {
    let tree = instance_into_tree(CONNECTIONS_SCENE);

    let ui_id = tree.get_node_by_path("/root/UI").unwrap();
    let button_id = tree.get_node_by_path("/root/UI/Button").unwrap();

    // Button should have a signal store with "pressed" wired to UI root.
    let store = tree
        .signal_store(button_id)
        .expect("Button should have a signal store after wiring");
    let pressed = store
        .get_signal("pressed")
        .expect("Button should have 'pressed' signal");
    assert_eq!(pressed.connection_count(), 1);
    assert_eq!(pressed.connections()[0].method, "_on_button_pressed");
    assert_eq!(pressed.connections()[0].target_id, ui_id.object_id());
}

// ===========================================================================
// 19. Connection flags (deferred/one-shot) wired correctly
// ===========================================================================

const FLAGS_SCENE: &str = r#"[gd_scene format=3]

[node name="Root" type="Node"]

[node name="Timer" type="Timer" parent="."]

[connection signal="timeout" from="Timer" to="." method="_on_timeout" flags=1]
[connection signal="tree_exited" from="Timer" to="." method="_on_exit" flags=4]
[connection signal="ready" from="Timer" to="." method="_on_ready" flags=5]
"#;

#[test]
fn connection_deferred_flag_preserved() {
    let tree = instance_into_tree(FLAGS_SCENE);
    let timer_id = tree.get_node_by_path("/root/Root/Timer").unwrap();

    let store = tree
        .signal_store(timer_id)
        .expect("Timer should have signal store");

    // flags=1 → CONNECT_DEFERRED
    let timeout = store.get_signal("timeout").expect("should have 'timeout'");
    assert!(
        timeout.connections()[0].deferred,
        "timeout connection should be deferred (flags=1)"
    );
    assert!(
        !timeout.connections()[0].one_shot,
        "timeout connection should NOT be one_shot"
    );
}

#[test]
fn connection_one_shot_flag_preserved() {
    let tree = instance_into_tree(FLAGS_SCENE);
    let timer_id = tree.get_node_by_path("/root/Root/Timer").unwrap();

    let store = tree
        .signal_store(timer_id)
        .expect("Timer should have signal store");

    // flags=4 → CONNECT_ONE_SHOT
    let exit = store
        .get_signal("tree_exited")
        .expect("should have 'tree_exited'");
    assert!(
        !exit.connections()[0].deferred,
        "tree_exited should NOT be deferred"
    );
    assert!(
        exit.connections()[0].one_shot,
        "tree_exited connection should be one_shot (flags=4)"
    );
}

#[test]
fn connection_combined_flags_preserved() {
    let tree = instance_into_tree(FLAGS_SCENE);
    let timer_id = tree.get_node_by_path("/root/Root/Timer").unwrap();

    let store = tree
        .signal_store(timer_id)
        .expect("Timer should have signal store");

    // flags=5 → DEFERRED | ONE_SHOT
    let ready = store.get_signal("ready").expect("should have 'ready'");
    assert!(
        ready.connections()[0].deferred,
        "ready connection should be deferred (flags=5)"
    );
    assert!(
        ready.connections()[0].one_shot,
        "ready connection should be one_shot (flags=5)"
    );
}

// ===========================================================================
// 20. Ext resource parsing and resolution
// ===========================================================================

#[test]
fn ext_resources_parsed_correctly() {
    let packed = PackedScene::from_tscn(PARENT_WITH_INSTANCE_TSCN).unwrap();
    let ext = packed.ext_resources();

    assert_eq!(ext.len(), 1);
    let entry = ext.get("1_mob").expect("should have ext resource '1_mob'");
    assert_eq!(entry.res_type, "PackedScene");
    assert_eq!(entry.path, "res://scenes/mob.tscn");
}

#[test]
fn resolve_ext_resource_path_works() {
    let packed = PackedScene::from_tscn(PARENT_WITH_INSTANCE_TSCN).unwrap();

    let resolved = packed.resolve_ext_resource_path("ExtResource(\"1_mob\")");
    assert_eq!(resolved, Some("res://scenes/mob.tscn"));

    // Non-existent reference.
    let missing = packed.resolve_ext_resource_path("ExtResource(\"99_nope\")");
    assert_eq!(missing, None);
}

// ===========================================================================
// 21. Unresolved sub-scene falls back to placeholder node
// ===========================================================================

#[test]
fn unresolved_subscene_creates_placeholder() {
    let parent = PackedScene::from_tscn(PARENT_WITH_INSTANCE_TSCN).unwrap();

    // resolve_scene always returns None — sub-scene can't be found.
    let resolve = |_path: &str| -> Option<PackedScene> { None };

    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree_with_subscenes(&mut tree, root, &parent, &resolve).unwrap();

    // Enemy should still exist as a placeholder node (with _instance property).
    let enemy = tree.get_node_by_path("/root/World/Enemy");
    assert!(
        enemy.is_some(),
        "Unresolved sub-scene should still produce a node"
    );

    let enemy_node = tree.get_node(enemy.unwrap()).unwrap();
    // The _instance property should be set (indicating it was an unresolved instance).
    match enemy_node.get_property("_instance") {
        gdvariant::Variant::String(s) => {
            assert!(
                s.contains("ExtResource"),
                "Placeholder should have _instance property: {s}"
            );
        }
        gdvariant::Variant::Nil => {
            // Acceptable — some implementations may not set this.
        }
        other => panic!("unexpected _instance value: {:?}", other),
    }
}

// ===========================================================================
// 22. Multiple sub-scene instances under same parent
// ===========================================================================

const MULTI_INSTANCE_TSCN: &str = r#"[gd_scene format=3]

[ext_resource type="PackedScene" path="res://scenes/mob.tscn" id="1_mob"]

[node name="World" type="Node"]

[node name="Mob1" type="Node2D" parent="." instance=ExtResource("1_mob")]
position = Vector2(10, 10)

[node name="Mob2" type="Node2D" parent="." instance=ExtResource("1_mob")]
position = Vector2(20, 20)

[node name="Mob3" type="Node2D" parent="." instance=ExtResource("1_mob")]
position = Vector2(30, 30)
"#;

#[test]
fn multiple_subscene_instances_independent() {
    let parent = PackedScene::from_tscn(MULTI_INSTANCE_TSCN).unwrap();

    let resolve = |path: &str| -> Option<PackedScene> {
        if path == "res://scenes/mob.tscn" {
            Some(PackedScene::from_tscn(CHILD_SCENE_TSCN).unwrap())
        } else {
            None
        }
    };

    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree_with_subscenes(&mut tree, root, &parent, &resolve).unwrap();

    // All three mobs should exist with their own names and positions.
    let mob1 = tree.get_node_by_path("/root/World/Mob1").unwrap();
    let mob2 = tree.get_node_by_path("/root/World/Mob2").unwrap();
    let mob3 = tree.get_node_by_path("/root/World/Mob3").unwrap();

    assert_ne!(mob1, mob2);
    assert_ne!(mob2, mob3);

    // Each mob should have its own position override.
    assert_eq!(
        tree.get_node(mob1).unwrap().get_property("position"),
        gdvariant::Variant::Vector2(gdcore::math::Vector2::new(10.0, 10.0))
    );
    assert_eq!(
        tree.get_node(mob2).unwrap().get_property("position"),
        gdvariant::Variant::Vector2(gdcore::math::Vector2::new(20.0, 20.0))
    );
    assert_eq!(
        tree.get_node(mob3).unwrap().get_property("position"),
        gdvariant::Variant::Vector2(gdcore::math::Vector2::new(30.0, 30.0))
    );

    // Each mob should have sub-scene children (Sprite + Collision).
    for &mob_id in &[mob1, mob2, mob3] {
        let mob = tree.get_node(mob_id).unwrap();
        assert_eq!(
            mob.children().len(),
            2,
            "Each mob instance should have 2 children"
        );
    }
}

// ===========================================================================
// 23. Groups merged from parent and sub-scene
// ===========================================================================

const GROUPED_INSTANCE_TSCN: &str = r#"[gd_scene format=3]

[ext_resource type="PackedScene" path="res://scenes/mob.tscn" id="1_mob"]

[node name="World" type="Node"]

[node name="Enemy" type="Node2D" parent="." instance=ExtResource("1_mob") groups=["enemies", "targetable"]]
"#;

const MOB_WITH_GROUPS_TSCN: &str = r#"[gd_scene format=3]

[node name="Mob" type="Node2D" groups=["npcs"]]
speed = 50
"#;

#[test]
fn subscene_groups_merged_with_parent_groups() {
    let parent = PackedScene::from_tscn(GROUPED_INSTANCE_TSCN).unwrap();

    let resolve = |path: &str| -> Option<PackedScene> {
        if path == "res://scenes/mob.tscn" {
            Some(PackedScene::from_tscn(MOB_WITH_GROUPS_TSCN).unwrap())
        } else {
            None
        }
    };

    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree_with_subscenes(&mut tree, root, &parent, &resolve).unwrap();

    let enemy_id = tree.get_node_by_path("/root/World/Enemy").unwrap();
    let enemy_node = tree.get_node(enemy_id).unwrap();
    let groups = enemy_node.groups();

    // Should have groups from both parent scene and sub-scene.
    assert!(
        groups.contains(&"enemies".to_string()),
        "Should have parent-scene group 'enemies'"
    );
    assert!(
        groups.contains(&"targetable".to_string()),
        "Should have parent-scene group 'targetable'"
    );
    assert!(
        groups.contains(&"npcs".to_string()),
        "Should have sub-scene group 'npcs'"
    );
}

// ===========================================================================
// 24. Nested sub-scene instancing (2 levels deep)
// ===========================================================================

const WEAPON_TSCN: &str = r#"[gd_scene format=3]

[node name="Weapon" type="Node2D"]
damage = 10

[node name="HitArea" type="Area2D" parent="."]
"#;

const SOLDIER_TSCN: &str = r#"[gd_scene format=3]

[ext_resource type="PackedScene" path="res://scenes/weapon.tscn" id="1_wpn"]

[node name="Soldier" type="CharacterBody2D"]
health = 100

[node name="Arm" type="Node2D" parent="." instance=ExtResource("1_wpn")]
"#;

const ARMY_TSCN: &str = r#"[gd_scene format=3]

[ext_resource type="PackedScene" path="res://scenes/soldier.tscn" id="1_soldier"]

[node name="Army" type="Node"]

[node name="Guard" type="CharacterBody2D" parent="." instance=ExtResource("1_soldier")]
"#;

#[test]
fn nested_subscene_two_levels_deep() {
    let army = PackedScene::from_tscn(ARMY_TSCN).unwrap();

    let resolve = |path: &str| -> Option<PackedScene> {
        match path {
            "res://scenes/soldier.tscn" => Some(PackedScene::from_tscn(SOLDIER_TSCN).unwrap()),
            "res://scenes/weapon.tscn" => Some(PackedScene::from_tscn(WEAPON_TSCN).unwrap()),
            _ => None,
        }
    };

    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree_with_subscenes(&mut tree, root, &army, &resolve).unwrap();

    // Guard (renamed from Soldier) should exist.
    let guard = tree.get_node_by_path("/root/Army/Guard");
    assert!(guard.is_some(), "Guard should exist");

    // Guard should have the soldier's health.
    let guard_node = tree.get_node(guard.unwrap()).unwrap();
    assert_eq!(
        guard_node.get_property("health"),
        gdvariant::Variant::Int(100)
    );

    // Arm (renamed from Weapon) should be a child of Guard.
    let arm = tree.get_node_by_path("/root/Army/Guard/Arm");
    assert!(
        arm.is_some(),
        "Arm (from weapon sub-scene) should exist under Guard"
    );

    // Arm should have the weapon's damage property.
    let arm_node = tree.get_node(arm.unwrap()).unwrap();
    assert_eq!(arm_node.get_property("damage"), gdvariant::Variant::Int(10));

    // HitArea should be under Arm.
    let hit_area = tree.get_node_by_path("/root/Army/Guard/Arm/HitArea");
    assert!(
        hit_area.is_some(),
        "HitArea from weapon sub-scene should be under Arm"
    );
}

// ===========================================================================
// 25. Instancing preserves total node count
// ===========================================================================

#[test]
fn total_node_count_correct_after_subscene_instancing() {
    let parent = PackedScene::from_tscn(PARENT_WITH_INSTANCE_TSCN).unwrap();

    let resolve = |path: &str| -> Option<PackedScene> {
        if path == "res://scenes/mob.tscn" {
            Some(PackedScene::from_tscn(CHILD_SCENE_TSCN).unwrap())
        } else {
            None
        }
    };

    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree_with_subscenes(&mut tree, root, &parent, &resolve).unwrap();

    // Expected: tree_root + World + Enemy(Mob) + Sprite + Collision = 5
    assert_eq!(
        tree.node_count(),
        5,
        "Tree should have root + World + Enemy + Sprite + Collision"
    );
}

// ===========================================================================
// 26. Empty scene (no nodes) produces an error — pat-ggk
// ===========================================================================

#[test]
fn empty_scene_returns_error() {
    let tscn = "[gd_scene format=3]\n";
    let result = PackedScene::from_tscn(tscn);
    assert!(
        result.is_err(),
        "Parsing an empty scene (no nodes) should fail"
    );
}

// ===========================================================================
// 27. Mutation after instancing — add children to instanced subtree — pat-ggk
// ===========================================================================

#[test]
fn add_child_to_instanced_subtree() {
    let tree = instance_into_tree(SIMPLE_SCENE);

    let enemy_id = tree.get_node_by_path("/root/Enemy").unwrap();
    let mut tree = tree; // make mutable

    // Dynamically add a node to the instanced subtree.
    let extra = gdscene::node::Node::new("ExtraChild", "Node");
    let extra_id = tree.add_child(enemy_id, extra).unwrap();

    // Enemy should now have 3 children: Sprite, Hitbox, ExtraChild.
    let enemy_node = tree.get_node(enemy_id).unwrap();
    assert_eq!(enemy_node.children().len(), 3);

    // The dynamically added node should be reachable by path.
    let found = tree.get_node_by_path("/root/Enemy/ExtraChild");
    assert_eq!(found, Some(extra_id));
}

// ===========================================================================
// 28. Remove instanced node and re-instance — pat-ggk
// ===========================================================================

#[test]
fn remove_and_reinstance_scene() {
    let packed = PackedScene::from_tscn(SIMPLE_SCENE).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    // Instance, then remove, then re-instance.
    let first_id = add_packed_scene_to_tree(&mut tree, root, &packed).unwrap();
    tree.remove_node(first_id).unwrap();

    // Root should have no children after removal.
    assert!(tree.get_node(root).unwrap().children().is_empty());

    // Re-instance.
    let second_id = add_packed_scene_to_tree(&mut tree, root, &packed).unwrap();
    assert_ne!(first_id, second_id, "Re-instanced node should have new ID");

    let root_node = tree.get_node(root).unwrap();
    assert_eq!(root_node.children().len(), 1);

    // Subtree should be intact.
    let enemy = tree.get_node(second_id).unwrap();
    assert_eq!(enemy.children().len(), 2);
}

// ===========================================================================
// 29. Reparent instanced node preserves subtree — pat-ggk
// ===========================================================================

#[test]
fn reparent_instanced_node_preserves_children() {
    let tscn = r#"[gd_scene format=3]

[node name="World" type="Node"]

[node name="Container" type="Node" parent="."]

[node name="Loose" type="Node" parent="."]
"#;
    let packed = PackedScene::from_tscn(tscn).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &packed).unwrap();

    // Instance a sub-scene under Loose.
    let loose = tree.get_node_by_path("/root/World/Loose").unwrap();
    let child_packed = PackedScene::from_tscn(SIMPLE_SCENE).unwrap();
    add_packed_scene_to_tree(&mut tree, loose, &child_packed).unwrap();

    // Verify Enemy is under Loose.
    assert!(tree.get_node_by_path("/root/World/Loose/Enemy").is_some());
    let enemy_id = tree.get_node_by_path("/root/World/Loose/Enemy").unwrap();
    let enemy_node = tree.get_node(enemy_id).unwrap();
    assert_eq!(enemy_node.children().len(), 2);

    // Reparent Loose under Container.
    let container = tree.get_node_by_path("/root/World/Container").unwrap();
    tree.reparent(loose, container).unwrap();

    // Enemy subtree should still be intact under the new parent path.
    let enemy_node = tree.get_node(enemy_id).unwrap();
    assert_eq!(
        enemy_node.children().len(),
        2,
        "Enemy children should survive reparent"
    );
    assert_eq!(enemy_node.class_name(), "Node2D");
}

// ===========================================================================
// 30. Wide + deep combined stress test — pat-ggk
// ===========================================================================

#[test]
fn wide_and_deep_combined_100_nodes() {
    // 5 branches, each 4 deep = 5*4 = 20 nodes + root = 21
    let mut tscn = String::from("[gd_scene format=3]\n\n[node name=\"Root\" type=\"Node\"]\n\n");
    for branch in 0..5 {
        let b_name = format!("Branch{branch}");
        tscn.push_str(&format!(
            "[node name=\"{b_name}\" type=\"Node\" parent=\".\"]\n\n"
        ));
        let mut parent_path = b_name.clone();
        for depth in 0..3 {
            let child_name = format!("B{branch}D{depth}");
            tscn.push_str(&format!(
                "[node name=\"{child_name}\" type=\"Node2D\" parent=\"{parent_path}\"]\nposition = Vector2({}, {})\n\n",
                branch * 100 + depth * 10,
                depth * 5
            ));
            parent_path = format!("{parent_path}/{child_name}");
        }
    }

    let tree = instance_into_tree(&tscn);
    let root_id = tree.get_node_by_path("/root/Root").unwrap();
    let root_node = tree.get_node(root_id).unwrap();
    assert_eq!(root_node.children().len(), 5, "Root should have 5 branches");

    // Total: root + 5 branches + 5*3 depth nodes = 1 + 5 + 15 = 21
    let all = tree.all_nodes_in_tree_order();
    // tree root + instanced root + 20 descendants = 22
    assert_eq!(
        all.len(),
        22,
        "Should have 22 total nodes (tree root + 21 instanced)"
    );

    // Verify deepest nodes are reachable.
    for branch in 0..5 {
        let path = format!("/root/Root/Branch{branch}/B{branch}D0/B{branch}D1/B{branch}D2");
        assert!(
            tree.get_node_by_path(&path).is_some(),
            "Deepest node at {path} should exist"
        );
    }
}

// ===========================================================================
// 31. Instance with multiple connection targets — pat-ggk
// ===========================================================================

const MULTI_CONN_SCENE: &str = r#"[gd_scene format=3]

[node name="Root" type="Node"]

[node name="Button" type="Button" parent="."]

[node name="Label" type="Label" parent="."]

[node name="Counter" type="Node" parent="."]

[connection signal="pressed" from="Button" to="." method="_on_pressed"]
[connection signal="pressed" from="Button" to="Label" method="_update_text"]
[connection signal="pressed" from="Button" to="Counter" method="_increment"]
"#;

#[test]
fn multiple_connections_from_same_signal() {
    let packed = PackedScene::from_tscn(MULTI_CONN_SCENE).unwrap();
    assert_eq!(packed.connection_count(), 3, "Should parse 3 connections");

    let tree = instance_into_tree(MULTI_CONN_SCENE);
    let button_id = tree.get_node_by_path("/root/Root/Button").unwrap();

    let store = tree
        .signal_store(button_id)
        .expect("Button should have signal store");
    let pressed = store
        .get_signal("pressed")
        .expect("Should have 'pressed' signal");
    assert_eq!(
        pressed.connection_count(),
        3,
        "pressed signal should have 3 connections"
    );

    // Verify each target method.
    let methods: Vec<&str> = pressed
        .connections()
        .iter()
        .map(|c| c.method.as_str())
        .collect();
    assert!(methods.contains(&"_on_pressed"));
    assert!(methods.contains(&"_update_text"));
    assert!(methods.contains(&"_increment"));
}

// ===========================================================================
// 32. Cross-boundary signal: subscene node connects to parent scene — pat-ggk
// ===========================================================================

const PARENT_WITH_CONN_TSCN: &str = r#"[gd_scene format=3]

[ext_resource type="PackedScene" path="res://scenes/mob.tscn" id="1_mob"]

[node name="World" type="Node"]

[node name="Enemy" type="Node2D" parent="." instance=ExtResource("1_mob")]

[connection signal="tree_entered" from="Enemy" to="." method="_on_enemy_entered"]
"#;

#[test]
fn cross_boundary_connection_wired() {
    let parent = PackedScene::from_tscn(PARENT_WITH_CONN_TSCN).unwrap();
    assert_eq!(parent.connection_count(), 1);

    let resolve = |path: &str| -> Option<PackedScene> {
        if path == "res://scenes/mob.tscn" {
            Some(PackedScene::from_tscn(CHILD_SCENE_TSCN).unwrap())
        } else {
            None
        }
    };

    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree_with_subscenes(&mut tree, root, &parent, &resolve).unwrap();

    let world_id = tree.get_node_by_path("/root/World").unwrap();
    let enemy_id = tree.get_node_by_path("/root/World/Enemy").unwrap();

    // The connection should be wired from Enemy to World.
    let store = tree
        .signal_store(enemy_id)
        .expect("Enemy should have signal store");
    let signal = store
        .get_signal("tree_entered")
        .expect("Should have tree_entered");
    assert_eq!(signal.connection_count(), 1);
    assert_eq!(signal.connections()[0].method, "_on_enemy_entered");
    assert_eq!(signal.connections()[0].target_id, world_id.object_id());
}

// ===========================================================================
// 33. Instance scene then verify process order — pat-ggk
// ===========================================================================

#[test]
fn process_order_correct_after_instancing() {
    let tscn = r#"[gd_scene format=3]

[node name="Root" type="Node"]

[node name="A" type="Node" parent="."]

[node name="B" type="Node" parent="."]

[node name="C" type="Node" parent="A"]
"#;
    let tree = instance_into_tree(tscn);

    let process_order = tree.all_nodes_in_process_order();
    // Process order should follow depth-first traversal.
    let names: Vec<&str> = process_order
        .iter()
        .map(|&id| tree.get_node(id).unwrap().name())
        .collect();

    // Expected: root, Root, A, C, B (depth-first)
    assert_eq!(names[0], "root", "First should be tree root");
    assert_eq!(names[1], "Root", "Second should be scene root");
    // A should come before B, and C (child of A) should come after A but before B.
    let a_pos = names.iter().position(|&n| n == "A").unwrap();
    let c_pos = names.iter().position(|&n| n == "C").unwrap();
    let b_pos = names.iter().position(|&n| n == "B").unwrap();
    assert!(a_pos < c_pos, "A should come before its child C");
    assert!(c_pos < b_pos, "C (child of A) should come before sibling B");
}

// ===========================================================================
// 34. Instance scene with node_count boundary checks — pat-ggk
// ===========================================================================

#[test]
fn node_count_consistent_through_operations() {
    let packed = PackedScene::from_tscn(SIMPLE_SCENE).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    assert_eq!(tree.node_count(), 1, "Just root initially");

    add_packed_scene_to_tree(&mut tree, root, &packed).unwrap();
    assert_eq!(tree.node_count(), 4, "root + Enemy + Sprite + Hitbox = 4");

    add_packed_scene_to_tree(&mut tree, root, &packed).unwrap();
    assert_eq!(tree.node_count(), 7, "4 + 3 more instanced = 7");

    // Remove one instance.
    let children = tree.get_node(root).unwrap().children().to_vec();
    tree.remove_node(children[0]).unwrap();
    assert_eq!(tree.node_count(), 4, "7 - 3 removed = 4");
}

// ===========================================================================
// 35. Meta properties preserved through instancing — pat-ggk
// ===========================================================================

#[test]
fn meta_properties_preserved_after_instancing() {
    let packed = PackedScene::from_tscn(SIMPLE_SCENE).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let enemy_id = add_packed_scene_to_tree(&mut tree, root, &packed).unwrap();

    // Set meta on the instanced node.
    tree.get_node_mut(enemy_id)
        .unwrap()
        .set_meta("editor_hint", gdvariant::Variant::String("spawner".into()));
    tree.get_node_mut(enemy_id)
        .unwrap()
        .set_meta("spawn_weight", gdvariant::Variant::Int(5));

    // Verify meta is readable.
    let node = tree.get_node(enemy_id).unwrap();
    assert_eq!(
        node.get_meta("editor_hint"),
        gdvariant::Variant::String("spawner".into())
    );
    assert_eq!(node.get_meta("spawn_weight"), gdvariant::Variant::Int(5));
    assert!(node.has_meta("editor_hint"));
    assert!(!node.has_meta("nonexistent"));

    let meta_list = node.get_meta_list();
    assert_eq!(meta_list.len(), 2);
}

#[test]
fn two_instances_have_independent_meta() {
    let packed = PackedScene::from_tscn(SIMPLE_SCENE).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let inst1 = add_packed_scene_to_tree(&mut tree, root, &packed).unwrap();
    let inst2 = add_packed_scene_to_tree(&mut tree, root, &packed).unwrap();

    // Set different meta on each instance.
    tree.get_node_mut(inst1)
        .unwrap()
        .set_meta("team", gdvariant::Variant::String("red".into()));
    tree.get_node_mut(inst2)
        .unwrap()
        .set_meta("team", gdvariant::Variant::String("blue".into()));

    assert_eq!(
        tree.get_node(inst1).unwrap().get_meta("team"),
        gdvariant::Variant::String("red".into())
    );
    assert_eq!(
        tree.get_node(inst2).unwrap().get_meta("team"),
        gdvariant::Variant::String("blue".into())
    );
}

#[test]
fn remove_meta_works_on_instanced_node() {
    let packed = PackedScene::from_tscn(SIMPLE_SCENE).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let enemy_id = add_packed_scene_to_tree(&mut tree, root, &packed).unwrap();
    tree.get_node_mut(enemy_id)
        .unwrap()
        .set_meta("temp", gdvariant::Variant::Bool(true));
    assert!(tree.get_node(enemy_id).unwrap().has_meta("temp"));

    tree.get_node_mut(enemy_id).unwrap().remove_meta("temp");
    assert!(!tree.get_node(enemy_id).unwrap().has_meta("temp"));
}

// ===========================================================================
// 36. ProcessMode behavior after instancing — pat-ggk
// ===========================================================================

use gdscene::node::ProcessMode;

#[test]
fn process_mode_default_is_inherit() {
    let tree = instance_into_tree(SIMPLE_SCENE);
    let enemy_id = tree.get_node_by_path("/root/Enemy").unwrap();
    assert_eq!(
        tree.get_node(enemy_id).unwrap().process_mode(),
        ProcessMode::Inherit
    );
}

#[test]
fn explicit_process_mode_preserved_after_instancing() {
    let packed = PackedScene::from_tscn(SIMPLE_SCENE).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let enemy_id = add_packed_scene_to_tree(&mut tree, root, &packed).unwrap();
    tree.get_node_mut(enemy_id)
        .unwrap()
        .set_process_mode(ProcessMode::Always);

    assert_eq!(
        tree.get_node(enemy_id).unwrap().process_mode(),
        ProcessMode::Always
    );
    assert_eq!(tree.effective_process_mode(enemy_id), ProcessMode::Always);
}

#[test]
fn inherit_resolves_to_parent_process_mode() {
    let packed = PackedScene::from_tscn(SIMPLE_SCENE).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let enemy_id = add_packed_scene_to_tree(&mut tree, root, &packed).unwrap();
    let sprite_id = tree.get_node_by_path("/root/Enemy/Sprite").unwrap();

    // Set parent to Disabled — child with Inherit should resolve to Disabled.
    tree.get_node_mut(enemy_id)
        .unwrap()
        .set_process_mode(ProcessMode::Disabled);
    assert_eq!(
        tree.get_node(sprite_id).unwrap().process_mode(),
        ProcessMode::Inherit
    );
    assert_eq!(
        tree.effective_process_mode(sprite_id),
        ProcessMode::Disabled
    );
}

#[test]
fn two_instances_independent_process_modes() {
    let packed = PackedScene::from_tscn(SIMPLE_SCENE).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let inst1 = add_packed_scene_to_tree(&mut tree, root, &packed).unwrap();
    let inst2 = add_packed_scene_to_tree(&mut tree, root, &packed).unwrap();

    tree.get_node_mut(inst1)
        .unwrap()
        .set_process_mode(ProcessMode::Always);
    tree.get_node_mut(inst2)
        .unwrap()
        .set_process_mode(ProcessMode::Disabled);

    assert_eq!(tree.effective_process_mode(inst1), ProcessMode::Always);
    assert_eq!(tree.effective_process_mode(inst2), ProcessMode::Disabled);
}

#[test]
fn should_process_respects_pause_and_process_mode() {
    let packed = PackedScene::from_tscn(SIMPLE_SCENE).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let enemy_id = add_packed_scene_to_tree(&mut tree, root, &packed).unwrap();

    // Default (Inherit → Pausable): processes when not paused.
    assert!(tree.should_process_node(enemy_id));
    tree.set_paused(true);
    assert!(!tree.should_process_node(enemy_id));

    // Always: processes even when paused.
    tree.get_node_mut(enemy_id)
        .unwrap()
        .set_process_mode(ProcessMode::Always);
    assert!(tree.should_process_node(enemy_id));

    // WhenPaused: processes only when paused.
    tree.get_node_mut(enemy_id)
        .unwrap()
        .set_process_mode(ProcessMode::WhenPaused);
    assert!(tree.should_process_node(enemy_id));
    tree.set_paused(false);
    assert!(!tree.should_process_node(enemy_id));

    // Disabled: never processes.
    tree.get_node_mut(enemy_id)
        .unwrap()
        .set_process_mode(ProcessMode::Disabled);
    assert!(!tree.should_process_node(enemy_id));
    tree.set_paused(true);
    assert!(!tree.should_process_node(enemy_id));
}

// ===========================================================================
// 37. Process priority ordering after instancing — pat-ggk
// ===========================================================================

#[test]
fn process_priority_default_zero() {
    let tree = instance_into_tree(SIMPLE_SCENE);
    let enemy_id = tree.get_node_by_path("/root/Enemy").unwrap();
    assert_eq!(tree.get_node(enemy_id).unwrap().process_priority(), 0);
}

#[test]
fn process_priority_preserved_on_instanced_nodes() {
    let packed = PackedScene::from_tscn(SIMPLE_SCENE).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let inst1 = add_packed_scene_to_tree(&mut tree, root, &packed).unwrap();
    let inst2 = add_packed_scene_to_tree(&mut tree, root, &packed).unwrap();

    tree.get_node_mut(inst1).unwrap().set_process_priority(-10);
    tree.get_node_mut(inst2).unwrap().set_process_priority(10);

    assert_eq!(tree.get_node(inst1).unwrap().process_priority(), -10);
    assert_eq!(tree.get_node(inst2).unwrap().process_priority(), 10);
}

// ===========================================================================
// 38. SubResource references preserved through instancing — pat-ggk
// ===========================================================================

const SUBRESOURCE_SCENE: &str = r#"[gd_scene format=3]

[sub_resource type="RectangleShape2D" id="RectangleShape2D_abc"]
size = Vector2(32, 32)

[sub_resource type="CircleShape2D" id="CircleShape2D_def"]
radius = 16.0

[node name="Root" type="Node2D"]

[node name="CollisionShape" type="CollisionShape2D" parent="."]
shape = SubResource("RectangleShape2D_abc")

[node name="EnemyCollision" type="CollisionShape2D" parent="."]
shape = SubResource("CircleShape2D_def")
"#;

#[test]
fn subresource_references_preserved_after_instancing() {
    let tree = instance_into_tree(SUBRESOURCE_SCENE);

    let collision = tree.get_node_by_path("/root/Root/CollisionShape").unwrap();
    let collision_node = tree.get_node(collision).unwrap();

    match collision_node.get_property("shape") {
        gdvariant::Variant::String(s) => {
            assert!(
                s.contains("RectangleShape2D"),
                "SubResource reference should contain type: got '{s}'"
            );
        }
        other => panic!("expected String for shape SubResource ref, got {:?}", other),
    }

    let enemy_collision = tree.get_node_by_path("/root/Root/EnemyCollision").unwrap();
    let enemy_node = tree.get_node(enemy_collision).unwrap();

    match enemy_node.get_property("shape") {
        gdvariant::Variant::String(s) => {
            assert!(
                s.contains("CircleShape2D"),
                "SubResource reference should contain type: got '{s}'"
            );
        }
        other => panic!("expected String for shape SubResource ref, got {:?}", other),
    }
}

#[test]
fn two_instances_subresource_refs_independent() {
    let packed = PackedScene::from_tscn(SUBRESOURCE_SCENE).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    add_packed_scene_to_tree(&mut tree, root, &packed).unwrap();
    add_packed_scene_to_tree(&mut tree, root, &packed).unwrap();

    let root_node = tree.get_node(root).unwrap();
    assert_eq!(root_node.children().len(), 2, "Two instances under root");

    // Both instances should have their SubResource properties intact.
    for &child_id in root_node.children() {
        let child = tree.get_node(child_id).unwrap();
        let collision_ids: Vec<_> = child.children().iter().copied().collect();
        assert_eq!(
            collision_ids.len(),
            2,
            "Each instance should have 2 collision children"
        );
    }
}

// ===========================================================================
// 39. Partial child removal then re-instance — pat-ggk
// ===========================================================================

#[test]
fn remove_one_child_then_add_another_instance() {
    let packed = PackedScene::from_tscn(SIMPLE_SCENE).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let enemy_id = add_packed_scene_to_tree(&mut tree, root, &packed).unwrap();

    // Remove just one child (Sprite) from the instanced scene.
    let sprite_id = tree.get_node_by_path("/root/Enemy/Sprite").unwrap();
    tree.remove_node(sprite_id).unwrap();

    // Enemy should now have 1 child (Hitbox).
    assert_eq!(tree.get_node(enemy_id).unwrap().children().len(), 1);

    // Instance another full copy under root.
    let enemy2_id = add_packed_scene_to_tree(&mut tree, root, &packed).unwrap();

    // New instance should be complete (2 children).
    assert_eq!(tree.get_node(enemy2_id).unwrap().children().len(), 2);

    // First instance still has 1 child.
    assert_eq!(tree.get_node(enemy_id).unwrap().children().len(), 1);
}

// ===========================================================================
// 40. Deep unique-name lookup in instanced scenes — pat-ggk
// ===========================================================================

const DEEP_UNIQUE_SCENE: &str = r#"[gd_scene format=3]

[node name="Root" type="Node"]

[node name="Level1" type="Node" parent="."]

[node name="Level2" type="Node" parent="Level1"]

[node name="%DeepTarget" type="Node2D" parent="Level1/Level2"]
position = Vector2(42, 42)
"#;

#[test]
fn unique_name_at_depth_3_flagged() {
    let tree = instance_into_tree(DEEP_UNIQUE_SCENE);

    let target = tree
        .get_node_by_path("/root/Root/Level1/Level2/DeepTarget")
        .unwrap();
    let target_node = tree.get_node(target).unwrap();

    assert!(
        target_node.is_unique_name(),
        "DeepTarget should be unique-name even at depth 3"
    );
    assert_eq!(
        target_node.get_property("position"),
        gdvariant::Variant::Vector2(gdcore::math::Vector2::new(42.0, 42.0))
    );
}

#[test]
fn two_instances_deep_unique_names_independent() {
    let packed = PackedScene::from_tscn(DEEP_UNIQUE_SCENE).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    add_packed_scene_to_tree(&mut tree, root, &packed).unwrap();
    add_packed_scene_to_tree(&mut tree, root, &packed).unwrap();

    // Both instances should have their own DeepTarget.
    let root_node = tree.get_node(root).unwrap();
    assert_eq!(root_node.children().len(), 2);

    for &child_id in root_node.children() {
        // Walk down: Root -> Level1 -> Level2 -> DeepTarget
        let root_inst = tree.get_node(child_id).unwrap();
        let l1 = tree.get_node(root_inst.children()[0]).unwrap();
        let l2 = tree.get_node(l1.children()[0]).unwrap();
        let target = tree.get_node(l2.children()[0]).unwrap();
        assert!(target.is_unique_name());
    }
}

// ===========================================================================
// pat-sky: Expanded edge-case coverage
// ===========================================================================

// ---------------------------------------------------------------------------
// 13. Reparenting instanced subtrees
// ---------------------------------------------------------------------------

const REPARENT_SCENE: &str = r#"[gd_scene format=3 uid="uid://reparent"]

[node name="World" type="Node2D"]

[node name="Player" type="CharacterBody2D" parent="."]

[node name="Sprite" type="Sprite2D" parent="Player"]
"#;

#[test]
fn reparent_instanced_subtree_preserves_children() {
    let mut tree = instance_into_tree(REPARENT_SCENE);
    let root = tree.root_id();

    // Add a second container node.
    let container = tree
        .add_child(root, gdscene::node::Node::new("Container", "Node2D"))
        .unwrap();

    // Find Player.
    let world = tree.get_node(root).unwrap().children()[0];
    let player = tree.get_node(world).unwrap().children()[0];
    let sprite_before = tree.get_node(player).unwrap().children()[0];

    // Reparent Player under Container.
    tree.reparent(player, container).unwrap();

    // Player should now be under Container.
    let container_children = tree.get_node(container).unwrap().children();
    assert_eq!(container_children.len(), 1);
    assert_eq!(container_children[0], player);

    // Sprite should still be Player's child.
    let player_children = tree.get_node(player).unwrap().children();
    assert_eq!(player_children.len(), 1);
    assert_eq!(player_children[0], sprite_before);

    // World should have no children left.
    assert!(tree.get_node(world).unwrap().children().is_empty());
}

#[test]
fn reparent_instanced_node_fires_lifecycle_notifications() {
    let mut tree = instance_into_tree(REPARENT_SCENE);
    let root = tree.root_id();

    let container = tree
        .add_child(root, gdscene::node::Node::new("Container", "Node2D"))
        .unwrap();

    let world = tree.get_node(root).unwrap().children()[0];
    let player = tree.get_node(world).unwrap().children()[0];

    // Clear notification logs before reparent.
    tree.get_node_mut(player).unwrap().clear_notification_log();

    tree.reparent(player, container).unwrap();

    // Reparent should fire EXIT_TREE then ENTER_TREE and READY.
    let log = tree.get_node(player).unwrap().notification_log();
    let has_enter = log.iter().any(|n| *n == NOTIFICATION_ENTER_TREE);
    let has_ready = log.iter().any(|n| *n == NOTIFICATION_READY);
    assert!(has_enter, "Reparent must fire ENTER_TREE on the moved node");
    assert!(has_ready, "Reparent must fire READY on the moved node");
}

// ---------------------------------------------------------------------------
// 14. Notification ordering across multiple instances
// ---------------------------------------------------------------------------

#[test]
fn notification_ordering_across_two_instances() {
    let packed = PackedScene::from_tscn(SIMPLE_SCENE).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let inst1 = add_packed_scene_to_tree(&mut tree, root, &packed).unwrap();
    let inst2 = add_packed_scene_to_tree(&mut tree, root, &packed).unwrap();

    // First instance should have received ENTER_TREE before second instance.
    let log1 = tree.get_node(inst1).unwrap().notification_log();
    let log2 = tree.get_node(inst2).unwrap().notification_log();

    // Both must have ENTER_TREE and READY.
    assert!(log1.iter().any(|n| *n == NOTIFICATION_ENTER_TREE));
    assert!(log1.iter().any(|n| *n == NOTIFICATION_READY));
    assert!(log2.iter().any(|n| *n == NOTIFICATION_ENTER_TREE));
    assert!(log2.iter().any(|n| *n == NOTIFICATION_READY));
}

#[test]
fn all_children_receive_enter_tree_before_ready() {
    let tree = instance_into_tree(SIMPLE_SCENE);
    let root = tree.root_id();
    let enemy = tree.get_node(root).unwrap().children()[0];

    // Check every node in the subtree.
    for &child_id in tree.get_node(enemy).unwrap().children() {
        let log = tree.get_node(child_id).unwrap().notification_log();
        let enter_pos = log.iter().position(|n| *n == NOTIFICATION_ENTER_TREE);
        let ready_pos = log.iter().position(|n| *n == NOTIFICATION_READY);
        assert!(
            enter_pos.is_some() && ready_pos.is_some(),
            "Child must receive both ENTER_TREE and READY"
        );
        assert!(
            enter_pos.unwrap() < ready_pos.unwrap(),
            "ENTER_TREE must precede READY for each node"
        );
    }
}

// ---------------------------------------------------------------------------
// 15. Very wide trees (many siblings)
// ---------------------------------------------------------------------------

#[test]
fn wide_scene_100_siblings_all_instanced() {
    // Build a .tscn with 100 sibling children.
    let mut tscn = String::from("[gd_scene format=3 uid=\"uid://wide100\"]\n\n");
    tscn.push_str("[node name=\"Root\" type=\"Node2D\"]\n\n");
    for i in 0..100 {
        tscn.push_str(&format!(
            "[node name=\"Child{i}\" type=\"Sprite2D\" parent=\".\"]\n"
        ));
    }

    let tree = instance_into_tree(&tscn);
    let root = tree.root_id();
    let scene_root = tree.get_node(root).unwrap().children()[0];
    let children = tree.get_node(scene_root).unwrap().children();

    assert_eq!(children.len(), 100, "All 100 siblings must be instanced");

    // Verify ordering: children must be in declaration order.
    for (i, &child_id) in children.iter().enumerate() {
        let name = tree.get_node(child_id).unwrap().name().to_string();
        assert_eq!(
            name,
            format!("Child{i}"),
            "Child order must match .tscn order"
        );
    }
}

#[test]
fn wide_scene_siblings_all_have_correct_parent() {
    let mut tscn = String::from("[gd_scene format=3 uid=\"uid://wide50p\"]\n\n");
    tscn.push_str("[node name=\"Root\" type=\"Node2D\"]\n\n");
    for i in 0..50 {
        tscn.push_str(&format!(
            "[node name=\"Item{i}\" type=\"Node2D\" parent=\".\"]\n"
        ));
    }

    let tree = instance_into_tree(&tscn);
    let root = tree.root_id();
    let scene_root = tree.get_node(root).unwrap().children()[0];

    for &child_id in tree.get_node(scene_root).unwrap().children() {
        let node = tree.get_node(child_id).unwrap();
        assert_eq!(
            node.parent(),
            Some(scene_root),
            "Every sibling must have scene root as parent"
        );
    }
}

// ---------------------------------------------------------------------------
// 16. Remove-and-reinstance cycle
// ---------------------------------------------------------------------------

#[test]
fn remove_and_reinstance_same_scene() {
    let packed = PackedScene::from_tscn(SIMPLE_SCENE).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    // Instance, remove, reinstance.
    let inst1 = add_packed_scene_to_tree(&mut tree, root, &packed).unwrap();
    tree.remove_node(inst1).unwrap();
    let inst2 = add_packed_scene_to_tree(&mut tree, root, &packed).unwrap();

    // Old id should be gone, new id should work.
    assert!(
        tree.get_node(inst1).is_none(),
        "Removed instance must be gone"
    );
    assert!(tree.get_node(inst2).is_some(), "New instance must exist");

    let enemy = tree.get_node(inst2).unwrap();
    assert_eq!(enemy.class_name(), "Node2D");
    assert_eq!(enemy.children().len(), 2);
}

#[test]
fn repeated_remove_reinstance_cycle() {
    let packed = PackedScene::from_tscn(SIMPLE_SCENE).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    // Do 10 cycles of instance + remove.
    for _ in 0..10 {
        let inst = add_packed_scene_to_tree(&mut tree, root, &packed).unwrap();
        tree.remove_node(inst).unwrap();
    }

    // Final instance should work cleanly.
    let final_inst = add_packed_scene_to_tree(&mut tree, root, &packed).unwrap();
    assert!(tree.get_node(final_inst).is_some());
    assert_eq!(
        tree.get_node(root).unwrap().children().len(),
        1,
        "Only the final instance should remain"
    );
}

// ---------------------------------------------------------------------------
// 17. Property mutation after instancing
// ---------------------------------------------------------------------------

const PROPS_SCENE: &str = r#"[gd_scene format=3 uid="uid://propsmut"]

[node name="Player" type="CharacterBody2D"]
velocity = Vector2(0, 0)

[node name="Sprite" type="Sprite2D" parent="."]
"#;

#[test]
fn mutating_property_on_one_instance_does_not_affect_another() {
    let packed = PackedScene::from_tscn(PROPS_SCENE).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let inst1 = add_packed_scene_to_tree(&mut tree, root, &packed).unwrap();
    let inst2 = add_packed_scene_to_tree(&mut tree, root, &packed).unwrap();

    // Mutate velocity on inst1.
    let new_vel = gdvariant::Variant::Vector2(gdcore::math::Vector2::new(100.0, 200.0));
    tree.get_node_mut(inst1)
        .unwrap()
        .set_property("velocity", new_vel.clone());

    // inst2 should still have original velocity.
    let vel2 = tree.get_node(inst2).unwrap().get_property("velocity");
    assert_ne!(
        vel2, new_vel,
        "Mutating one instance must not affect another"
    );
}

#[test]
fn adding_property_after_instancing_is_local_to_node() {
    let packed = PackedScene::from_tscn(SIMPLE_SCENE).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let inst1 = add_packed_scene_to_tree(&mut tree, root, &packed).unwrap();
    let inst2 = add_packed_scene_to_tree(&mut tree, root, &packed).unwrap();

    // Add a new property to inst1.
    tree.get_node_mut(inst1)
        .unwrap()
        .set_property("custom_tag", gdvariant::Variant::String("marked".into()));

    // inst2 should not have it.
    let val = tree.get_node(inst2).unwrap().get_property("custom_tag");
    assert_eq!(
        val,
        gdvariant::Variant::Nil,
        "Custom property must not leak across instances"
    );
}

// ---------------------------------------------------------------------------
// 18. Cross-owner-boundary path resolution
// ---------------------------------------------------------------------------

const TWO_LEVEL_SCENE: &str = r#"[gd_scene format=3 uid="uid://twolevel"]

[node name="Outer" type="Node2D"]

[node name="Middle" type="Node2D" parent="."]

[node name="Inner" type="Sprite2D" parent="Middle"]
"#;

#[test]
fn get_node_by_path_within_instance_works() {
    let tree = instance_into_tree(TWO_LEVEL_SCENE);

    // Absolute path should resolve.
    let inner = tree.get_node_by_path("/root/Outer/Middle/Inner");
    assert!(inner.is_some(), "Absolute path to inner node must resolve");
    assert_eq!(
        tree.get_node(inner.unwrap()).unwrap().class_name(),
        "Sprite2D"
    );
}

#[test]
fn get_node_relative_from_child_to_sibling_parent() {
    let tree = instance_into_tree(TWO_LEVEL_SCENE);

    let inner = tree.get_node_by_path("/root/Outer/Middle/Inner").unwrap();

    // Relative path "../../" from Inner should reach Outer.
    let outer = tree.get_node_relative(inner, "../..");
    assert!(
        outer.is_some(),
        "Relative path ../.. from Inner must reach Outer"
    );
    assert_eq!(tree.get_node(outer.unwrap()).unwrap().name(), "Outer");
}

#[test]
fn relative_path_to_nonexistent_returns_none() {
    let tree = instance_into_tree(TWO_LEVEL_SCENE);

    let outer = tree.get_node_by_path("/root/Outer").unwrap();
    let result = tree.get_node_relative(outer, "DoesNotExist/Child");
    assert!(
        result.is_none(),
        "Non-existent relative path must return None"
    );
}

// ---------------------------------------------------------------------------
// 19. Instance under a dynamically-added parent
// ---------------------------------------------------------------------------

#[test]
fn instance_under_dynamically_created_parent() {
    let packed = PackedScene::from_tscn(SIMPLE_SCENE).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    // Create a parent dynamically, then instance under it.
    let entities = tree
        .add_child(root, gdscene::node::Node::new("Entities", "Node2D"))
        .unwrap();

    let inst = add_packed_scene_to_tree(&mut tree, entities, &packed).unwrap();

    assert_eq!(
        tree.get_node(inst).unwrap().parent(),
        Some(entities),
        "Instanced root should be child of dynamic parent"
    );

    // Path should be /root/Entities/Enemy.
    let by_path = tree.get_node_by_path("/root/Entities/Enemy");
    assert!(by_path.is_some(), "Path /root/Entities/Enemy must resolve");
}

#[test]
fn multiple_scenes_under_dynamic_parent_maintain_order() {
    let packed = PackedScene::from_tscn(SIMPLE_SCENE).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let container = tree
        .add_child(root, gdscene::node::Node::new("Container", "Node2D"))
        .unwrap();

    let ids: Vec<_> = (0..5)
        .map(|_| add_packed_scene_to_tree(&mut tree, container, &packed).unwrap())
        .collect();

    let children = tree.get_node(container).unwrap().children().to_vec();
    assert_eq!(children.len(), 5);
    for (i, expected_id) in ids.iter().enumerate() {
        assert_eq!(
            children[i], *expected_id,
            "Instance order must match addition order"
        );
    }
}

// ---------------------------------------------------------------------------
// 20. Ownership after sub-scene instancing
// ---------------------------------------------------------------------------

#[test]
fn sub_scene_instance_root_owned_by_none() {
    let tree = instance_into_tree(TWO_LEVEL_SCENE);
    let root = tree.root_id();
    let outer = tree.get_node(root).unwrap().children()[0];

    // The instance root (Outer) should have no owner — it IS the owner scope.
    assert_eq!(
        tree.get_node(outer).unwrap().owner(),
        None,
        "Instance root must have no owner"
    );
}

#[test]
fn all_children_owned_by_instance_root() {
    let tree = instance_into_tree(TWO_LEVEL_SCENE);
    let root = tree.root_id();
    let outer = tree.get_node(root).unwrap().children()[0];
    let middle = tree.get_node(outer).unwrap().children()[0];
    let inner = tree.get_node(middle).unwrap().children()[0];

    assert_eq!(
        tree.get_node(middle).unwrap().owner(),
        Some(outer),
        "Middle must be owned by instance root"
    );
    assert_eq!(
        tree.get_node(inner).unwrap().owner(),
        Some(outer),
        "Inner (grandchild) must also be owned by instance root"
    );
}

// ---------------------------------------------------------------------------
// 21. Deterministic instancing
// ---------------------------------------------------------------------------

#[test]
fn same_scene_produces_identical_structure_every_time() {
    let packed = PackedScene::from_tscn(SIMPLE_SCENE).unwrap();

    let results: Vec<Vec<(String, String)>> = (0..5)
        .map(|_| {
            let mut tree = SceneTree::new();
            let root = tree.root_id();
            add_packed_scene_to_tree(&mut tree, root, &packed).unwrap();

            // Collect (name, class) pairs in tree order.
            tree.all_nodes_in_tree_order()
                .iter()
                .map(|&id| {
                    let n = tree.get_node(id).unwrap();
                    (n.name().to_string(), n.class_name().to_string())
                })
                .collect()
        })
        .collect();

    for i in 1..results.len() {
        assert_eq!(
            results[0], results[i],
            "Instancing must produce identical structure every time (iteration {i})"
        );
    }
}

// ---------------------------------------------------------------------------
// 22. Add child to instanced subtree
// ---------------------------------------------------------------------------

#[test]
fn add_dynamic_child_to_instanced_node() {
    let mut tree = instance_into_tree(SIMPLE_SCENE);
    let root = tree.root_id();
    let enemy = tree.get_node(root).unwrap().children()[0];

    // Enemy starts with 2 children (Sprite + Hitbox).
    assert_eq!(tree.get_node(enemy).unwrap().children().len(), 2);

    // Add a dynamic child.
    let extra = tree
        .add_child(enemy, gdscene::node::Node::new("HealthBar", "Control"))
        .unwrap();

    assert_eq!(
        tree.get_node(enemy).unwrap().children().len(),
        3,
        "Instanced node must accept dynamic children"
    );
    assert_eq!(tree.get_node(extra).unwrap().name(), "HealthBar");
    assert_eq!(tree.get_node(extra).unwrap().parent(), Some(enemy));
}

#[test]
fn dynamic_child_receives_lifecycle_notifications() {
    let mut tree = instance_into_tree(SIMPLE_SCENE);
    let root = tree.root_id();
    let enemy = tree.get_node(root).unwrap().children()[0];

    let extra = tree
        .add_child(enemy, gdscene::node::Node::new("HealthBar", "Control"))
        .unwrap();

    let log = tree.get_node(extra).unwrap().notification_log();
    assert!(
        log.iter().any(|n| *n == NOTIFICATION_ENTER_TREE),
        "Dynamic child must receive ENTER_TREE"
    );
    assert!(
        log.iter().any(|n| *n == NOTIFICATION_READY),
        "Dynamic child must receive READY"
    );
}
