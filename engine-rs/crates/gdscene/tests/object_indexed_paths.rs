//! pat-0q2kx.162 clause 1 — `set_indexed` / `get_indexed` from a real script.
//!
//! The interpreter arms live on the `ObjectId` branch of `call_method_on`, but the
//! `self.method()` branch **returns early** before reaching it (`interpreter.rs`,
//! the `Expr::SelfRef` case). So `node.set_indexed(...)` worked while
//! `self.set_indexed(...)` — the form the acceptance criterion actually uses —
//! resolved as neither a builtin nor a user function and failed.
//!
//! The unit tests next to the helpers cover the subname resolution itself. These
//! cover the part unit tests cannot: that a running script reaches the arms at
//! all, through both receiver forms.

use gdcore::math::Vector2;
use gdscene::node::Node;
use gdscene::scene_tree::SceneTree;
use gdscene::GDScriptNodeInstance;
use gdscript_interop::bindings::ScriptInstance;
use gdvariant::Variant;

/// `Root(Node2D)` with a `Child(Node2D)` sibling to address explicitly.
fn tree_with_script(src: &str) -> (SceneTree, gdscene::node::NodeId, gdscene::node::NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let main = tree
        .add_child(root, Node::new("Main", "Node2D"))
        .expect("main added");
    let child = tree
        .add_child(main, Node::new("Child", "Node2D"))
        .expect("child added");
    if let Some(n) = tree.get_node_mut(main) {
        n.set_property("position", Variant::Vector2(Vector2::new(1.0, 2.0)));
    }
    if let Some(n) = tree.get_node_mut(child) {
        n.set_property("position", Variant::Vector2(Vector2::new(0.0, 0.0)));
    }
    let script = GDScriptNodeInstance::from_source(src, main).expect("script parses");
    tree.attach_script(main, Box::new(script));
    (tree, main, child)
}

fn pos(tree: &SceneTree, id: gdscene::node::NodeId) -> Vector2 {
    match tree.get_node(id).map(|n| n.get_property("position")) {
        Some(Variant::Vector2(v)) => v,
        other => panic!("expected a Vector2 position, got {other:?}"),
    }
}

/// **`self.set_indexed` must write a sub-field.** This is the form the bead's
/// acceptance criterion uses, and the one the `SelfRef` early-return broke.
#[test]
fn self_set_indexed_writes_a_component() {
    let (mut tree, main, _child) = tree_with_script(
        "extends Node2D\nfunc run():\n    self.set_indexed(\"position:x\", 5.0)\n",
    );

    tree.get_script_mut(main)
        .expect("script attached")
        .call_method("run", &[])
        .expect("self.set_indexed must not raise");

    let p = pos(&tree, main);
    assert_eq!(p.x, 5.0, "position:x must be written through self");
    assert_eq!(p.y, 2.0, "the sibling component must survive the write");
}

/// `self.get_indexed` must read the same sub-field back.
#[test]
fn self_get_indexed_reads_a_component() {
    let (mut tree, main, _child) = tree_with_script(
        "extends Node2D\nvar got = -1.0\nfunc run():\n    self.set_indexed(\"position:x\", 7.5)\n    self.got = self.get_indexed(\"position:x\")\n",
    );

    tree.get_script_mut(main)
        .expect("script attached")
        .call_method("run", &[])
        .expect("self.get_indexed must not raise");

    let got = tree
        .get_script(main)
        .unwrap()
        .list_properties()
        .into_iter()
        .find(|p| p.name == "got")
        .map(|p| p.default_value);
    assert_eq!(
        got,
        Some(Variant::Float(7.5)),
        "get_indexed must read back what set_indexed wrote, got {got:?}"
    );
}

/// The explicit-receiver form must keep working — it is the path that already
/// resolved, and the `self` route must not have disturbed it.
#[test]
fn node_receiver_set_indexed_still_works() {
    let (mut tree, _main, child) = tree_with_script(
        "extends Node2D\nfunc run():\n    var c = get_node(\"Child\")\n    c.set_indexed(\"position:y\", 3.0)\n",
    );

    tree.get_script_mut(_main)
        .expect("script attached")
        .call_method("run", &[])
        .expect("node.set_indexed must not raise");

    let p = pos(&tree, child);
    assert_eq!(p.y, 3.0, "position:y must be written on the child");
    assert_eq!(p.x, 0.0, "the sibling component must survive");
}

/// A path with no `:` chain degrades to a plain property write, which is what
/// Godot's `set_indexed` does for a bare property name.
#[test]
fn set_indexed_without_a_chain_is_a_plain_property_write() {
    let (mut tree, main, _child) = tree_with_script(
        "extends Node2D\nfunc run():\n    self.set_indexed(\"position\", Vector2(9.0, 8.0))\n",
    );

    tree.get_script_mut(main)
        .expect("script attached")
        .call_method("run", &[])
        .expect("plain set_indexed must not raise");

    let p = pos(&tree, main);
    assert_eq!(
        (p.x, p.y),
        (9.0, 8.0),
        "a chainless path writes the property"
    );
}
