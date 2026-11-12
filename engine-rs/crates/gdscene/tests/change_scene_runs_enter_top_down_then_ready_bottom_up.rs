//! A scene reached through `change_scene_to_file` must start the way Godot
//! starts any instanced subtree: one `add_child` of the fully built scene, so
//! `_enter_tree` runs TOP-DOWN over the whole subtree and only then `_ready`
//! runs BOTTOM-UP — children before their parent (`Node::_propagate_enter_tree`
//! then `Node::_propagate_ready`, scene/main/node.cpp). A parent's `_ready`
//! therefore always sees children that have already entered and readied, which
//! is what `$Child.value` in a root `_ready` relies on.
//!
//! Each node must also receive ENTER_TREE / READY exactly ONCE.

use gdscene::main_loop::MainLoop;
use gdscene::node::Node;
use gdscene::SceneTree;

const ROOT_GD: &str = "extends Node\nfunc _enter_tree():\n\tprint(\"enter Root children=\", get_child_count())\nfunc _ready():\n\tprint(\"ready Root sees=\", $Child.value)\n";
const CHILD_GD: &str = "extends Node\nvar value = \"unset\"\nfunc _enter_tree():\n\tprint(\"enter Child\")\nfunc _ready():\n\tvalue = \"child-ready-ran\"\n\tprint(\"ready Child\")\n";
const B_TSCN: &str = "[gd_scene load_steps=3 format=3]\n\n[ext_resource type=\"Script\" path=\"res://b_root.gd\" id=\"1\"]\n[ext_resource type=\"Script\" path=\"res://b_child.gd\" id=\"2\"]\n\n[node name=\"B\" type=\"Node\"]\nscript = ExtResource(\"1\")\n\n[node name=\"Child\" type=\"Node\" parent=\".\"]\nscript = ExtResource(\"2\")\n";

#[test]
fn change_scene_runs_enter_top_down_then_ready_bottom_up_once_each() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("b_root.gd"), ROOT_GD).unwrap();
    std::fs::write(dir.path().join("b_child.gd"), CHILD_GD).unwrap();
    std::fs::write(dir.path().join("b.tscn"), B_TSCN).unwrap();
    gdcore::set_path_roots(Some(dir.path().to_path_buf()), None);
    gdobject::class_db::register_all_classes();

    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let old = tree.add_child(root, Node::new("Old", "Node")).unwrap();
    tree.set_current_scene(old, None);
    let mut ml = MainLoop::new(tree);
    ml.tree_mut().drain_script_output();
    ml.tree_mut()
        .change_scene_to_file("res://b.tscn")
        .expect("queued");
    ml.step(1.0 / 60.0);

    let out = ml.tree_mut().drain_script_output();
    assert!(out.errors.is_empty(), "script errors: {:?}", out.errors);
    assert_eq!(
        out.prints,
        vec![
            "enter Root children=1".to_string(),
            "enter Child".to_string(),
            "ready Child".to_string(),
            "ready Root sees=child-ready-ran".to_string(),
        ],
        "Godot order: _enter_tree top-down, then _ready bottom-up, once each"
    );

    let b = ml.tree().current_scene().expect("B is current");
    let child = ml.tree().get_node_or_null(b, "Child").unwrap();
    for id in [b, child] {
        let log = ml.tree().get_node(id).unwrap().notification_log();
        let count = |code: i32| log.iter().filter(|n| n.code() == code).count();
        assert_eq!(count(10), 1, "ENTER_TREE once: {log:?}");
        assert_eq!(count(13), 1, "READY once: {log:?}");
    }
}
