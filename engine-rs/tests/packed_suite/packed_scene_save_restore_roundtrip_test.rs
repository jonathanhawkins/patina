//! pat-pdf: PackedScene save/restore roundtrip.
//!
//! Parse .tscn -> instance into SceneTree -> save back to .tscn ->
//! re-parse and verify structure, names, types, and properties survive.

use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;
use gdscene::TscnSaver;

#[test]
fn roundtrip_preserves_node_count_names_types() {
    let source = r#"[gd_scene format=3]

[node name="World" type="Node2D"]
position = Vector2(10, 20)

[node name="Player" type="CharacterBody2D" parent="."]
position = Vector2(100, 200)

[node name="Sprite" type="Sprite2D" parent="Player"]
position = Vector2(0, -16)
"#;

    let scene1 = PackedScene::from_tscn(source).unwrap();
    assert_eq!(scene1.node_count(), 3);

    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let scene_root = add_packed_scene_to_tree(&mut tree, root, &scene1).unwrap();

    let saved = TscnSaver::save_tree(&tree, scene_root);

    let scene2 = PackedScene::from_tscn(&saved).unwrap();
    assert_eq!(
        scene2.node_count(),
        scene1.node_count(),
        "roundtrip must preserve node count"
    );

    let nodes2 = scene2.instance().unwrap();
    let names: Vec<&str> = nodes2.iter().map(|n| n.name()).collect();
    assert_eq!(names, vec!["World", "Player", "Sprite"]);

    let types: Vec<&str> = nodes2.iter().map(|n| n.class_name()).collect();
    assert_eq!(types, vec!["Node2D", "CharacterBody2D", "Sprite2D"]);
}

#[test]
fn roundtrip_preserves_root_properties() {
    let source = r#"[gd_scene format=3]

[node name="Root" type="Node2D"]
position = Vector2(42, 99)
"#;

    let scene1 = PackedScene::from_tscn(source).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let scene_root = add_packed_scene_to_tree(&mut tree, root, &scene1).unwrap();

    let saved = TscnSaver::save_tree(&tree, scene_root);
    let scene2 = PackedScene::from_tscn(&saved).unwrap();
    let nodes = scene2.instance().unwrap();

    let pos = nodes[0].get_property("position");
    assert!(
        !matches!(pos, gdvariant::Variant::Nil),
        "roundtrip must preserve properties, got Nil"
    );
}

#[test]
fn roundtrip_single_node() {
    let source = r#"[gd_scene format=3]

[node name="Solo" type="Node"]
"#;

    let scene1 = PackedScene::from_tscn(source).unwrap();
    assert_eq!(scene1.node_count(), 1);

    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let scene_root = add_packed_scene_to_tree(&mut tree, root, &scene1).unwrap();

    let saved = TscnSaver::save_tree(&tree, scene_root);
    let scene2 = PackedScene::from_tscn(&saved).unwrap();
    assert_eq!(scene2.node_count(), 1);

    let nodes = scene2.instance().unwrap();
    assert_eq!(nodes[0].name(), "Solo");
    assert_eq!(nodes[0].class_name(), "Node");
}

#[test]
fn roundtrip_deep_hierarchy() {
    let source = r#"[gd_scene format=3]

[node name="A" type="Node"]

[node name="B" type="Node" parent="."]

[node name="C" type="Node" parent="B"]

[node name="D" type="Node" parent="B/C"]
"#;

    let scene1 = PackedScene::from_tscn(source).unwrap();
    assert_eq!(scene1.node_count(), 4);

    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let scene_root = add_packed_scene_to_tree(&mut tree, root, &scene1).unwrap();

    let saved = TscnSaver::save_tree(&tree, scene_root);
    let scene2 = PackedScene::from_tscn(&saved).unwrap();
    assert_eq!(scene2.node_count(), 4);

    let nodes = scene2.instance().unwrap();
    let names: Vec<&str> = nodes.iter().map(|n| n.name()).collect();
    assert_eq!(names, vec!["A", "B", "C", "D"]);
}

#[test]
fn roundtrip_preserves_child_properties() {
    let source = r#"[gd_scene format=3]

[node name="Root" type="Node2D"]

[node name="Child" type="Sprite2D" parent="."]
position = Vector2(55, 66)
"#;

    let scene1 = PackedScene::from_tscn(source).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let scene_root = add_packed_scene_to_tree(&mut tree, root, &scene1).unwrap();

    let saved = TscnSaver::save_tree(&tree, scene_root);
    let scene2 = PackedScene::from_tscn(&saved).unwrap();
    let nodes = scene2.instance().unwrap();

    let child = nodes.iter().find(|n| n.name() == "Child").unwrap();
    let pos = child.get_property("position");
    match pos {
        gdvariant::Variant::Vector2(v) => {
            assert!(
                (v.x - 55.0).abs() < 0.001 && (v.y - 66.0).abs() < 0.001,
                "child position should be (55, 66), got ({}, {})",
                v.x,
                v.y
            );
        }
        other => panic!("position should be Vector2, got {:?}", other),
    }
}
