//! pat-414k3: `draw_ordered_children` reads the parent's own child list
//! instead of walking the whole tree once per parent, and must return exactly
//! what the walk returned: the children, in tree order, after reparenting,
//! reordering and removal.
//!
//! The walk made the HUD pass quadratic in the tree size. A 3D game's tree is
//! large (the official 3D platformer has 600+ nodes), and the pass runs under
//! the editor-state lock on every Play frame.

use gdscene::canvas_order::draw_ordered_children;
use gdscene::node::Node;
use gdscene::{NodeId, SceneTree};

/// The definition `draw_ordered_children` used to compute directly.
fn by_tree_walk(tree: &SceneTree, parent: NodeId) -> Vec<NodeId> {
    tree.all_nodes_in_tree_order()
        .into_iter()
        .filter(|&id| tree.get_node(id).and_then(|n| n.parent()) == Some(parent))
        .collect()
}

fn assert_all_match(tree: &SceneTree) {
    for id in tree.all_nodes_in_tree_order() {
        assert_eq!(
            draw_ordered_children(tree, id),
            by_tree_walk(tree, id),
            "children of {id:?}"
        );
    }
}

#[test]
fn draw_ordered_children_match_the_tree_walk_through_edits() {
    gdobject::class_db::register_all_classes();
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let mut ids = vec![root];
    // A ragged tree of CanvasItems, three levels deep.
    for i in 0..60 {
        let parent = ids[(i * 7) % ids.len()];
        let class = if i % 3 == 0 { "Control" } else { "Node2D" };
        let id = tree
            .add_child(parent, Node::new(&format!("N{i}"), class))
            .unwrap();
        ids.push(id);
    }
    assert_all_match(&tree);

    // Reorder, reparent and remove, checking after each.
    let some_parent = ids[1];
    let kids = draw_ordered_children(&tree, some_parent);
    assert!(
        kids.len() >= 2,
        "the fixture must give node 1 several children"
    );
    tree.move_child(some_parent, kids[kids.len() - 1], 0)
        .unwrap();
    assert_all_match(&tree);
    tree.reparent(ids[10], ids[30]).unwrap();
    assert_all_match(&tree);
    tree.remove_node(ids[5]).unwrap();
    assert_all_match(&tree);
}
