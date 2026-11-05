//! The editor's edited tree holds instances EXPANDED, told apart from the
//! scene's own nodes by owner, and the saver writes an instance's nodes as
//! override entries (pat-hivyf). Real fixture: the official Godot 3D
//! platformer's `player.tscn`, which instances `player.glb` with Editable
//! Children.

use gdscene::packed_scene::{add_packed_scene_to_tree_for_editing, is_foreign_node, PackedScene};
use gdscene::scene_saver::TscnSaver;
use gdscene::SceneTree;
use gdvariant::Variant;

fn open_player() -> (SceneTree, gdscene::node::NodeId) {
    gdobject::class_db::register_all_classes();
    let game = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../games/platformer_3d");
    gdcore::set_path_roots(Some(game.clone()), None);
    let text = std::fs::read_to_string(game.join("player/player.tscn")).unwrap();
    let scene = PackedScene::from_tscn(&text).unwrap();
    let mut tree = SceneTree::new();
    let r = tree.root_id();
    let root = add_packed_scene_to_tree_for_editing(&mut tree, r, &scene).expect("player.tscn opens for editing");
    (tree, root)
}

#[test]
fn the_glb_instance_is_expanded_marked_and_its_nodes_are_foreign() {
    let (tree, root) = open_player();
    let at = |p: &str| tree.get_node_relative(root, p).unwrap_or_else(|| panic!("no {p}"));
    let inst = tree.get_node(at("Player")).unwrap();
    assert_eq!(inst.class_name(), "Node3D");
    assert_eq!(inst.get_property("_instance_path"), Variant::String("res://player/player.glb".into()));
    assert_eq!(inst.get_property("_editable_children"), Variant::Bool(true));
    assert_eq!(tree.get_node(at("Player/Skeleton/Skeleton3D/Robot")).unwrap().get_property("layers"), Variant::Int(2));

    assert!(!is_foreign_node(&tree, root, at("Player")), "the instance root is the scene's own node");
    assert!(is_foreign_node(&tree, root, at("Player/Skeleton")));
    assert!(is_foreign_node(&tree, root, at("Player/Skeleton/Skeleton3D/Robot")));
    assert!(!is_foreign_node(&tree, root, at("Player/Skeleton/Bullet")), "added by player.tscn");
    assert!(!is_foreign_node(&tree, root, root));
}

#[test]
fn the_saver_writes_foreign_nodes_as_typeless_overrides_and_keeps_editable() {
    let (tree, root) = open_player();
    // The GENERATOR (no file baseline) is what this checks: an opened scene
    // saves as an edit of its file, which keeps these entries verbatim anyway.
    let text = TscnSaver::generate(&tree, root);
    assert!(text.contains("[node name=\"Player\" parent=\".\" instance=ExtResource("), "{text}");
    assert!(text.contains("[node name=\"Robot\" parent=\"Player/Skeleton/Skeleton3D\"]\n"), "no type= on a glTF node:\n{text}");
    assert!(text.contains("[node name=\"Bullet\" type=\"Marker3D\" parent=\"Player/Skeleton\"]"), "{text}");
    assert!(text.contains("\n[editable path=\"Player\"]\n"), "{text}");
    assert!(!text.contains("_editable_children"), "the editor flag is the [editable] section, not a property:\n{text}");

    // And it loads back to the same expanded scene.
    let again = PackedScene::from_tscn(&text).unwrap();
    assert_eq!(again.editable_instances(), ["Player".to_string()]);
    let mut t2 = SceneTree::new();
    let r2 = t2.root_id();
    let root2 = add_packed_scene_to_tree_for_editing(&mut t2, r2, &again).unwrap();
    let robot = t2.get_node_relative(root2, "Player/Skeleton/Skeleton3D/Robot").expect("Robot");
    assert_eq!(t2.get_node(robot).unwrap().class_name(), "MeshInstance3D");
    assert_eq!(t2.get_node(robot).unwrap().get_property("layers"), Variant::Int(2));
}
