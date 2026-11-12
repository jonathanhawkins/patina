//! `Control.layout_mode` is a computed read in Godot (`Control::_get_layout_mode`,
//! control.cpp:938): UNCONTROLLED (3) without a parent Control, CONTAINER (2)
//! under a Container, ANCHORS (1) when the stored mode is anchors and the
//! anchors leave the top-left preset, else POSITION (0). Patina stored nothing
//! and returned `Nil` for every read.

use gdscene::control::layout_mode;
use gdscene::node::Node;
use gdscene::{GDScriptNodeInstance, SceneTree};
use gdvariant::Variant;

fn tree_with_controls() -> (SceneTree, [gdscene::node::NodeId; 5]) {
    gdobject::class_db::register_all_classes();
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let ui = tree.add_child(root, Node::new("UI", "Control")).unwrap();
    let plain = tree.add_child(ui, Node::new("Plain", "ColorRect")).unwrap();
    let vbox = tree
        .add_child(ui, Node::new("Box", "VBoxContainer"))
        .unwrap();
    let in_box = tree.add_child(vbox, Node::new("Btn", "Button")).unwrap();
    let mut anchored = Node::new("Anchored", "Panel");
    anchored.set_property("layout_mode", Variant::Int(1));
    anchored.set_property("anchor_right", Variant::Float(1.0));
    anchored.set_property("anchor_bottom", Variant::Float(1.0));
    let anchored = tree.add_child(ui, anchored).unwrap();
    (tree, [ui, plain, vbox, in_box, anchored])
}

#[test]
fn layout_mode_follows_the_parent_kind() {
    let (tree, [ui, plain, vbox, in_box, anchored]) = tree_with_controls();
    assert_eq!(
        layout_mode(&tree, ui),
        3,
        "parent is the root Window: UNCONTROLLED"
    );
    assert_eq!(
        layout_mode(&tree, plain),
        0,
        "plain Control parent: POSITION"
    );
    assert_eq!(layout_mode(&tree, vbox), 0);
    assert_eq!(layout_mode(&tree, in_box), 2, "Container parent: CONTAINER");
    assert_eq!(
        layout_mode(&tree, anchored),
        1,
        "full-rect anchors: ANCHORS"
    );
}

#[test]
fn stored_anchors_mode_at_top_left_anchors_still_reads_anchors() {
    // Edge: stored ANCHORS with all anchors at 0 is the TOP_LEFT preset, so
    // Godot falls through and returns the stored mode (control.cpp:953).
    let (mut tree, [ui, ..]) = tree_with_controls();
    let mut n = Node::new("TopLeft", "Label");
    n.set_property("layout_mode", Variant::Int(1));
    let n = tree.add_child(ui, n).unwrap();
    assert_eq!(layout_mode(&tree, n), 1);
    // A stored CONTAINER/UNCONTROLLED under a plain Control falls back to POSITION.
    let mut m = Node::new("Stale", "Label");
    m.set_property("layout_mode", Variant::Int(2));
    let m = tree.add_child(ui, m).unwrap();
    assert_eq!(layout_mode(&tree, m), 0);
}

#[test]
fn a_script_reads_the_computed_layout_mode() {
    let (mut tree, [_, _, vbox, ..]) = tree_with_controls();
    let probe = tree.add_child(vbox, Node::new("Probe", "Label")).unwrap();
    let src = "extends Label\nfunc _ready():\n\tset_meta(\"mode\", layout_mode)\n";
    tree.attach_script(
        probe,
        Box::new(GDScriptNodeInstance::from_source(src, probe).unwrap()),
    );
    gdscene::LifecycleManager::enter_tree(&mut tree, probe);
    assert_eq!(
        tree.get_node(probe).unwrap().get_meta("mode"),
        Variant::Int(2),
        "`layout_mode` from script must be CONTAINER, not Nil"
    );
}
