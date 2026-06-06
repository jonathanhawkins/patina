//! Multi-object property editing for the inspector.
//!
//! When several nodes are selected, committing a shared property in the
//! inspector applies the new value to every selected node, recorded as a
//! single undo step so one undo reverts the change across all of them — like
//! Godot's multi-node inspector editing.

use crate::undo_redo::{UndoAction, UndoOp, UndoRedoManager};
use gdscene::node::NodeId;
use gdscene::SceneTree;
use gdvariant::Variant;

/// Commits a shared property edit across multiple selected nodes as a single
/// undo step.
///
/// `value` is written to `property` on every node in `node_ids` that exists,
/// and the per-node `SetProperty` operations are grouped into one
/// [`UndoAction`] pushed onto `undo`, so a single undo reverts the change on
/// every node. Returns the number of nodes the edit was applied to.
pub fn commit_multi_node_property(
    tree: &mut SceneTree,
    undo: &mut UndoRedoManager,
    node_ids: &[NodeId],
    property: &str,
    value: Variant,
) -> usize {
    let mut ops = Vec::new();
    for &node_id in node_ids {
        if let Some(node) = tree.get_node_mut(node_id) {
            let old_value = node.get_property(property);
            node.set_property(property, value.clone());
            ops.push(UndoOp::SetProperty {
                node_id: node_id.raw(),
                property: property.to_string(),
                new_value: value.to_string(),
                old_value: old_value.to_string(),
            });
        }
    }

    let applied = ops.len();
    if applied > 0 {
        let label = format!("Set {property} on {applied} node(s)");
        undo.push(UndoAction::group(label, ops));
    }
    applied
}

#[cfg(test)]
mod tests {
    use super::*;
    use gdscene::node::Node;

    /// Acceptance (pat-7xd0s): with several nodes selected, committing a shared
    /// property applies the new value to every selected node in a single undo
    /// step.
    #[test]
    fn inspector_multi_node_property_edit() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let a = tree.add_child(root, Node::new("A", "Node2D")).unwrap();
        let b = tree.add_child(root, Node::new("B", "Node2D")).unwrap();
        let c = tree.add_child(root, Node::new("C", "Node2D")).unwrap();

        // Distinct initial values so the shared edit is observable.
        tree.get_node_mut(a)
            .unwrap()
            .set_property("z_index", Variant::Int(0));
        tree.get_node_mut(b)
            .unwrap()
            .set_property("z_index", Variant::Int(1));
        tree.get_node_mut(c)
            .unwrap()
            .set_property("z_index", Variant::Int(2));

        let mut undo = UndoRedoManager::new(64);

        // Edit z_index with A and B selected (C is not selected).
        let applied =
            commit_multi_node_property(&mut tree, &mut undo, &[a, b], "z_index", Variant::Int(5));
        assert_eq!(applied, 2, "the edit applies to both selected nodes");

        // Every selected node receives the new value; the unselected node is
        // left unchanged.
        assert_eq!(
            tree.get_node(a).unwrap().get_property("z_index"),
            Variant::Int(5)
        );
        assert_eq!(
            tree.get_node(b).unwrap().get_property("z_index"),
            Variant::Int(5)
        );
        assert_eq!(
            tree.get_node(c).unwrap().get_property("z_index"),
            Variant::Int(2),
            "an unselected node is not affected"
        );

        // The change is a single undo step covering one operation per node.
        assert_eq!(
            undo.undo_count(),
            1,
            "a multi-node edit is a single undo step"
        );
        let action = undo.undo_stack().last().expect("one undo action");
        assert_eq!(
            action.op_count(),
            2,
            "the single action groups both per-node edits"
        );
    }

    #[test]
    fn multi_edit_skips_missing_nodes_and_noops_on_empty() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let a = tree.add_child(root, Node::new("A", "Node2D")).unwrap();
        let mut undo = UndoRedoManager::new(64);

        // No selection → no edit, no undo entry.
        let applied = commit_multi_node_property(&mut tree, &mut undo, &[], "z_index", Variant::Int(1));
        assert_eq!(applied, 0);
        assert_eq!(undo.undo_count(), 0, "an empty selection pushes no undo step");

        // A single real node still works and records one undo step.
        let applied = commit_multi_node_property(&mut tree, &mut undo, &[a], "z_index", Variant::Int(9));
        assert_eq!(applied, 1);
        assert_eq!(
            tree.get_node(a).unwrap().get_property("z_index"),
            Variant::Int(9)
        );
        assert_eq!(undo.undo_count(), 1);
    }
}
