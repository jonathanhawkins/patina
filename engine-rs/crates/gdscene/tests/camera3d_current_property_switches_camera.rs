//! `camera.current = true` switches the viewport's camera, as Godot's
//! `Camera3D::set_current(true)` → `make_current()` does: exactly one Camera3D
//! is current, so the previous one is cleared. The write used to set only the
//! new camera's flag; the first camera (auto-current on entering) stayed
//! current too and, being first in tree order, kept rendering.

use gdscene::node::Node;
use gdscene::{GDScriptNodeInstance, SceneTree};
use gdvariant::Variant;

#[test]
fn setting_current_true_from_a_script_clears_the_previous_camera() {
    gdobject::class_db::register_all_classes();
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let a = tree.add_child(root, Node::new("CamA", "Camera3D")).unwrap();
    let b = tree.add_child(root, Node::new("CamB", "Camera3D")).unwrap();
    assert_eq!(tree.get_node(a).unwrap().get_property("current"), Variant::Bool(true), "first camera auto-current");

    let s = tree.add_child(root, Node::new("Switcher", "Node")).unwrap();
    let src = "extends Node\nfunc _ready():\n\tget_node(\"../CamB\").current = true\n";
    tree.attach_script(s, Box::new(GDScriptNodeInstance::from_source(src, s).unwrap()));
    gdscene::LifecycleManager::enter_tree(&mut tree, s);

    assert_eq!(tree.get_node(b).unwrap().get_property("current"), Variant::Bool(true));
    assert_eq!(
        tree.get_node(a).unwrap().get_property("current"),
        Variant::Bool(false),
        "one current camera per viewport: CamA is cleared"
    );
}
