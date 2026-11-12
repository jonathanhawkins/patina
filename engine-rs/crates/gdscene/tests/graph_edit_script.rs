//! pat-0q2kx.244 (partial) — `GraphEdit`'s connection model gets an owner.
//!
//! Godot ground truth: `scene/gui/graph_edit.cpp` / `graph_node.cpp`.
//!
//! `gdscene/src/graph_edit.rs` was a complete 732-line model — slot/port
//! typing, duplicate rejection, validity rules — with NO owner anywhere in the
//! engine, so nothing could reach any of it. This gives it per-node storage on
//! the SceneTree, which is what the scripting accessor can reach.
//!
//! ⚠️ SCOPE: storage plus the script-reachable path. The bead also wants a draw
//! arm in control_render.rs and drag-to-connect emitting `connection_request`;
//! neither is delivered, so `graph_edit_renders_and_connects` does not exist.

use gdcore::math::Color;
use gdscene::graph_edit::{GraphEdit, GraphNode};
use gdscene::node::Node;
use gdscene::scene_tree::SceneTree;

/// A node with one output on the right.
fn source(title: &str) -> GraphNode {
    let mut n = GraphNode::new(title);
    n.set_slot(0, false, 0, Color::WHITE, true, 0, Color::WHITE);
    n
}

/// A node with one input on the left.
fn sink(title: &str) -> GraphNode {
    let mut n = GraphNode::new(title);
    n.set_slot(0, true, 0, Color::WHITE, false, 0, Color::WHITE);
    n
}

fn tree_with_graph() -> (SceneTree, gdscene::NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let g = tree
        .add_child(root, Node::new("Graph", "GraphEdit"))
        .unwrap();
    (tree, g)
}

/// The model now has an owner, created lazily and kept per node.
#[test]
fn the_tree_owns_a_per_node_graph_model() {
    let (mut tree, g) = tree_with_graph();

    // A pure query must not allocate one.
    assert!(
        tree.get_graph_edit(g).is_none(),
        "a node that has never been mutated should carry no model"
    );

    tree.graph_edit_mut(g).add_node("a", source("A"));
    assert!(
        tree.get_graph_edit(g).is_some(),
        "the first mutation creates it"
    );

    // A second GraphEdit gets its OWN model. One shared instance would make
    // every GraphEdit in a scene alias the same connections.
    let root = tree.root_id();
    let h = tree
        .add_child(root, Node::new("Graph2", "GraphEdit"))
        .unwrap();
    tree.graph_edit_mut(h).add_node("z", source("Z"));

    assert!(tree.get_graph_edit(g).unwrap().get_node("a").is_some());
    assert!(
        tree.get_graph_edit(h).unwrap().get_node("a").is_none(),
        "the second graph must not see the first graph's node"
    );
}

/// A connection made through the tree survives and reads back intact.
#[test]
fn connections_persist_through_the_tree() {
    let (mut tree, g) = tree_with_graph();
    {
        let graph = tree.graph_edit_mut(g);
        graph.add_node("a", source("A"));
        graph.add_node("b", sink("B"));
        graph
            .connect_node("a", 0, "b", 0)
            .expect("valid connection");
    }

    let list = tree.get_graph_edit(g).unwrap().get_connection_list();
    assert_eq!(list.len(), 1, "the connection survived the borrow");
    assert_eq!(list[0].from_node, "a");
    assert_eq!(list[0].from_port, 0);
    assert_eq!(list[0].to_node, "b");
    assert_eq!(list[0].to_port, 0);
}

/// The model's own rules still apply through this path — adding storage must
/// not bypass the validation that makes the model worth having.
#[test]
fn the_models_validation_still_applies() {
    let (mut tree, g) = tree_with_graph();
    let graph = tree.graph_edit_mut(g);
    graph.add_node("a", source("A"));
    graph.add_node("b", sink("B"));

    // Unknown destination.
    assert!(
        graph.connect_node("a", 0, "nope", 0).is_err(),
        "connecting to an unregistered node must fail"
    );
    // A port the slot does not provide — "b" has an input at 0, not 3.
    assert!(
        graph.connect_node("a", 0, "b", 3).is_err(),
        "an out-of-range port must fail rather than silently connect"
    );
    // Wrong direction: "b" has no OUTPUT, so it cannot be a source.
    assert!(
        graph.connect_node("b", 0, "a", 0).is_err(),
        "a node with no output port cannot originate a connection"
    );

    graph
        .connect_node("a", 0, "b", 0)
        .expect("the valid one works");
    graph.disconnect_node("a", 0, "b", 0).expect("disconnects");
    assert!(graph.get_connection_list().is_empty());
}

/// A tree-owned model behaves exactly like a standalone one, so the storage
/// change contributed no behaviour of its own.
#[test]
fn tree_owned_matches_standalone() {
    let mut standalone = GraphEdit::default();
    standalone.add_node("a", source("A"));
    standalone.add_node("b", sink("B"));
    standalone.connect_node("a", 0, "b", 0).unwrap();

    let (mut tree, g) = tree_with_graph();
    let owned = tree.graph_edit_mut(g);
    owned.add_node("a", source("A"));
    owned.add_node("b", sink("B"));
    owned.connect_node("a", 0, "b", 0).unwrap();

    assert_eq!(owned.get_connection_list().len(), 1);
    assert_eq!(
        owned.get_connection_list()[0].from_node,
        standalone.get_connection_list()[0].from_node
    );
}
