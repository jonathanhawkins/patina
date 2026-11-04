//! A node's `groups=[...]` in a .tscn must put it in the tree's group index,
//! so `get_nodes_in_group` / `call_group` reach it — Godot registers a node's
//! groups as it enters (`Node::_propagate_enter_tree`, scene/main/node.cpp).
//!
//! Found on Dodge the Creeps: `new_game()` clears the previous round with
//! `get_tree().call_group("mobs", "queue_free")`, and `mob.tscn` declares
//! `groups=["mobs"]`. Only `SceneTree::add_to_group` indexed groups, so the
//! call reached no mob and old mobs survived the restart.

use gdscene::packed_scene::PackedScene;
use gdscene::SceneTree;

const LEVEL: &str = r#"[gd_scene format=3]

[node name="Level" type="Node2D"]

[node name="MobA" type="Node2D" parent="." groups=["mobs"]]

[node name="MobB" type="Node2D" parent="." groups=["mobs", "flying"]]

[node name="Player" type="Node2D" parent="."]
"#;

#[test]
fn scene_file_groups_are_indexed_and_reached_by_call_group() {
    gdobject::class_db::register_all_classes();
    let scene = PackedScene::from_tscn(LEVEL).unwrap();
    let mut tree = SceneTree::new();
    let level = tree.change_scene_to_packed(&scene).unwrap();

    assert_eq!(tree.get_node_count_in_group("mobs"), 2);
    assert_eq!(tree.get_node_count_in_group("flying"), 1);
    assert!(tree.has_group("mobs"));

    tree.call_group("mobs", "queue_free", &[]);
    tree.process_deletions();

    assert!(tree.get_node_or_null(level, "MobA").is_none(), "MobA freed");
    assert!(tree.get_node_or_null(level, "MobB").is_none(), "MobB freed");
    assert!(tree.get_node_or_null(level, "Player").is_some(), "non-members untouched");
    assert_eq!(
        tree.get_node_count_in_group("mobs"),
        0,
        "freed members leave the index"
    );
}
