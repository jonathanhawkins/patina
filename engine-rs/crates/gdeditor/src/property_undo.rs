//! Reversible undo/redo for inspector property edits, with scene-dirty tracking.
//!
//! Committing a property change from the inspector records a reversible entry:
//! undo restores the prior value and redo reapplies it, and the owning scene is
//! marked modified. Unlike the string-based [`crate::undo_redo`] model, this
//! history stores the actual `Variant` values so edits round-trip exactly.

use gdscene::node::NodeId;
use gdscene::SceneTree;
use gdvariant::Variant;

/// A single reversible property edit.
#[derive(Debug, Clone)]
struct PropertyEdit {
    node_id: NodeId,
    property: String,
    old_value: Variant,
    new_value: Variant,
}

/// Tracks reversible inspector property edits and whether the owning scene has
/// unsaved modifications.
#[derive(Debug, Default)]
pub struct PropertyEditHistory {
    undo: Vec<PropertyEdit>,
    redo: Vec<PropertyEdit>,
    dirty: bool,
}

impl PropertyEditHistory {
    /// Creates an empty history with a clean (unmodified) scene.
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether the scene has unsaved modifications.
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// Whether there is an edit available to undo.
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    /// Whether there is an undone edit available to redo.
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Commits a property change: writes `new_value` to `property` on `node_id`,
    /// records a reversible undo entry (discarding any redo history), and marks
    /// the scene modified. Returns `false` if the node does not exist.
    pub fn commit(
        &mut self,
        tree: &mut SceneTree,
        node_id: NodeId,
        property: &str,
        new_value: Variant,
    ) -> bool {
        let node = match tree.get_node_mut(node_id) {
            Some(n) => n,
            None => return false,
        };
        let old_value = node.get_property(property);
        node.set_property(property, new_value.clone());
        self.undo.push(PropertyEdit {
            node_id,
            property: property.to_string(),
            old_value,
            new_value,
        });
        self.redo.clear();
        self.dirty = true;
        true
    }

    /// Undoes the most recent edit, restoring the prior value on the node and
    /// moving the edit onto the redo stack. Returns `false` if there is nothing
    /// to undo.
    pub fn undo(&mut self, tree: &mut SceneTree) -> bool {
        let edit = match self.undo.pop() {
            Some(e) => e,
            None => return false,
        };
        if let Some(node) = tree.get_node_mut(edit.node_id) {
            node.set_property(&edit.property, edit.old_value.clone());
        }
        self.redo.push(edit);
        self.dirty = true;
        true
    }

    /// Redoes the most recently undone edit, reapplying its value on the node.
    /// Returns `false` if there is nothing to redo.
    pub fn redo(&mut self, tree: &mut SceneTree) -> bool {
        let edit = match self.redo.pop() {
            Some(e) => e,
            None => return false,
        };
        if let Some(node) = tree.get_node_mut(edit.node_id) {
            node.set_property(&edit.property, edit.new_value.clone());
        }
        self.undo.push(edit);
        self.dirty = true;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gdscene::node::Node;

    /// Acceptance (pat-27bwu): committing a property change records an undo
    /// entry that restores the prior value on undo and reapplies it on redo,
    /// and the owning scene is marked modified.
    #[test]
    fn inspector_property_undo_redo_dirty() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let id = tree.add_child(root, Node::new("Node", "Node2D")).unwrap();
        tree.get_node_mut(id)
            .unwrap()
            .set_property("z_index", Variant::Int(0));

        let mut history = PropertyEditHistory::new();
        assert!(!history.is_dirty(), "a fresh scene starts unmodified");
        assert!(!history.can_undo());

        // Commit a property change: it applies to the node and marks dirty.
        assert!(history.commit(&mut tree, id, "z_index", Variant::Int(7)));
        assert_eq!(
            tree.get_node(id).unwrap().get_property("z_index"),
            Variant::Int(7)
        );
        assert!(
            history.is_dirty(),
            "committing a property change marks the scene modified"
        );
        assert!(history.can_undo());
        assert!(!history.can_redo());

        // Undo restores the prior value.
        assert!(history.undo(&mut tree));
        assert_eq!(
            tree.get_node(id).unwrap().get_property("z_index"),
            Variant::Int(0),
            "undo restores the prior value"
        );
        assert!(history.can_redo());

        // Redo reapplies the value.
        assert!(history.redo(&mut tree));
        assert_eq!(
            tree.get_node(id).unwrap().get_property("z_index"),
            Variant::Int(7),
            "redo reapplies the value"
        );
    }

    #[test]
    fn commit_clears_redo_and_handles_missing_node() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let id = tree.add_child(root, Node::new("Node", "Node2D")).unwrap();

        let mut history = PropertyEditHistory::new();
        history.commit(&mut tree, id, "value", Variant::Int(1));
        history.undo(&mut tree);
        assert!(history.can_redo());

        // A fresh commit discards the redo history.
        history.commit(&mut tree, id, "value", Variant::Int(2));
        assert!(!history.can_redo(), "a new commit clears the redo stack");

        // Committing to a missing node is a no-op.
        let missing = tree.add_child(root, Node::new("Tmp", "Node")).unwrap();
        let _ = tree.remove_node(missing);
        assert!(!history.commit(&mut tree, missing, "value", Variant::Int(9)));
    }
}
