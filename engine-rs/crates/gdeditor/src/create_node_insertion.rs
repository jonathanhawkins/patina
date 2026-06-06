//! Where a newly created node lands in the scene tree.
//!
//! When the user confirms node creation, the editor has to decide *where* the
//! node goes:
//!
//! - If the scene is **empty**, the new node becomes the **scene root**.
//! - Otherwise it is inserted as a **child of the currently selected** node (or,
//!   when nothing is selected, as a child of the root).
//!
//! In every case the freshly created node is **focused** (selected) in the scene
//! tree so the user can keep building from it.
//!
//! This module models just enough of a scene tree to make that placement logic
//! testable in isolation from the full editor scene graph.

/// How a created node was placed in the tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodePlacement {
    /// The scene was empty; the node became the scene root.
    Root,
    /// The node was inserted as a child of the node with this id.
    ChildOf(usize),
}

/// The result of creating a node: its new id and where it was placed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NodeInsertion {
    /// Id of the newly created node.
    pub id: usize,
    /// Where it landed.
    pub placement: NodePlacement,
}

#[derive(Debug, Clone)]
struct NodeRec {
    name: String,
    parent: Option<usize>,
    children: Vec<usize>,
}

/// A minimal scene tree used to model node-creation placement and focus.
#[derive(Debug, Clone, Default)]
pub struct SceneTreeModel {
    nodes: Vec<NodeRec>,
    root: Option<usize>,
    /// The currently selected/focused node.
    selected: Option<usize>,
}

impl SceneTreeModel {
    /// Creates an empty scene (no root, nothing selected).
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether the scene has no nodes yet.
    pub fn is_empty(&self) -> bool {
        self.root.is_none()
    }

    /// Total number of nodes in the scene.
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// The scene root, if any.
    pub fn root(&self) -> Option<usize> {
        self.root
    }

    /// The currently focused (selected) node, if any.
    pub fn focused(&self) -> Option<usize> {
        self.selected
    }

    /// Selects (focuses) a node.
    pub fn select(&mut self, id: usize) {
        if id < self.nodes.len() {
            self.selected = Some(id);
        }
    }

    /// Clears the current selection.
    pub fn clear_selection(&mut self) {
        self.selected = None;
    }

    /// The parent of a node, or `None` for the root / unknown id.
    pub fn parent_of(&self, id: usize) -> Option<usize> {
        self.nodes.get(id).and_then(|n| n.parent)
    }

    /// The children of a node, in insertion order.
    pub fn children_of(&self, id: usize) -> &[usize] {
        match self.nodes.get(id) {
            Some(n) => &n.children,
            None => &[],
        }
    }

    /// A node's name.
    pub fn name(&self, id: usize) -> Option<&str> {
        self.nodes.get(id).map(|n| n.name.as_str())
    }

    /// Confirms creation of a node named `name` and places it per the editor
    /// rules: root when the scene is empty, otherwise a child of the current
    /// selection (or the root when nothing is selected). The new node is
    /// focused. Returns its id and placement.
    pub fn create_node(&mut self, name: impl Into<String>) -> NodeInsertion {
        let id = self.nodes.len();

        if self.is_empty() {
            // Empty scene: the new node becomes the root.
            self.nodes.push(NodeRec {
                name: name.into(),
                parent: None,
                children: Vec::new(),
            });
            self.root = Some(id);
            self.selected = Some(id);
            return NodeInsertion {
                id,
                placement: NodePlacement::Root,
            };
        }

        // Non-empty: parent is the selection, falling back to the root.
        let parent = self.selected.or(self.root).expect("non-empty scene has a root");
        self.nodes.push(NodeRec {
            name: name.into(),
            parent: Some(parent),
            children: Vec::new(),
        });
        self.nodes[parent].children.push(id);
        self.selected = Some(id);
        NodeInsertion {
            id,
            placement: NodePlacement::ChildOf(parent),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Acceptance (pat-bkami): confirming creation inserts the node as a child
    /// of the active selection, focuses it, and makes it the scene root when the
    /// scene is empty.
    #[test]
    fn create_node_child_of_selection() {
        let mut tree = SceneTreeModel::new();
        assert!(tree.is_empty());
        assert!(tree.focused().is_none());

        // Empty scene: the first node becomes the root and is focused.
        let root = tree.create_node("Main");
        assert_eq!(root.placement, NodePlacement::Root);
        assert_eq!(tree.root(), Some(root.id));
        assert_eq!(tree.focused(), Some(root.id), "new root is focused");
        assert_eq!(tree.parent_of(root.id), None);
        assert!(!tree.is_empty());

        // With the root selected, a new node is inserted as its child and
        // becomes the focus.
        let child = tree.create_node("Player");
        assert_eq!(child.placement, NodePlacement::ChildOf(root.id));
        assert_eq!(tree.parent_of(child.id), Some(root.id));
        assert_eq!(tree.children_of(root.id), &[child.id]);
        assert_eq!(tree.focused(), Some(child.id), "new child is focused");

        // Because creation re-focuses, the next node nests under the previous.
        let grandchild = tree.create_node("Sprite");
        assert_eq!(grandchild.placement, NodePlacement::ChildOf(child.id));
        assert_eq!(tree.parent_of(grandchild.id), Some(child.id));

        // Selecting a different node targets creation there.
        tree.select(root.id);
        let sibling = tree.create_node("Camera");
        assert_eq!(sibling.placement, NodePlacement::ChildOf(root.id));
        assert_eq!(tree.parent_of(sibling.id), Some(root.id));
        // Root now has two direct children: the first child and the sibling.
        assert_eq!(tree.children_of(root.id), &[child.id, sibling.id]);
    }

    /// With nothing selected but a non-empty scene, creation falls back to the
    /// root as parent.
    #[test]
    fn create_with_no_selection_falls_back_to_root() {
        let mut tree = SceneTreeModel::new();
        let root = tree.create_node("Main");
        tree.clear_selection();
        assert!(tree.focused().is_none());

        let inserted = tree.create_node("Loose");
        assert_eq!(inserted.placement, NodePlacement::ChildOf(root.id));
        assert_eq!(tree.parent_of(inserted.id), Some(root.id));
        assert_eq!(tree.focused(), Some(inserted.id));
    }
}
