//! Editor dock panels and layout management.
//!
//! Provides the [`DockPanel`] trait and concrete implementations for
//! the scene tree dock and property dock, mirroring Godot's editor
//! layout panels.

use gdscene::node::NodeId;
use gdscene::SceneTree;

use crate::inspector::InspectorPanel;

/// A named dock panel in the editor UI.
///
/// Each dock has a title and can refresh its contents from the scene tree.
pub trait DockPanel {
    /// Returns the display title of this dock.
    fn title(&self) -> &str;

    /// Refreshes the dock's internal state from the current scene tree.
    fn refresh(&mut self, tree: &SceneTree);
}

/// An entry in the scene tree dock, representing a node in the hierarchy.
#[derive(Debug, Clone)]
pub struct SceneTreeEntry {
    /// The node's ID.
    pub id: NodeId,
    /// The node's display name.
    pub name: String,
    /// The node's class name.
    pub class_name: String,
    /// The absolute path of this node.
    pub path: String,
    /// Indentation depth (0 for root).
    pub depth: usize,
}

/// A dock panel showing the scene tree node hierarchy.
///
/// Displays nodes as a flat list with indentation to convey depth,
/// similar to Godot's Scene dock.
#[derive(Debug)]
pub struct SceneTreeDock {
    /// Flattened tree entries, in depth-first order.
    entries: Vec<SceneTreeEntry>,
}

impl SceneTreeDock {
    /// Creates an empty scene tree dock.
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    /// Returns the current list of tree entries.
    pub fn entries(&self) -> &[SceneTreeEntry] {
        &self.entries
    }

    /// Finds an entry by node ID.
    pub fn find_entry(&self, id: NodeId) -> Option<&SceneTreeEntry> {
        self.entries.iter().find(|e| e.id == id)
    }

    /// Collects entries recursively from the scene tree.
    fn collect_entries(tree: &SceneTree, id: NodeId, depth: usize, out: &mut Vec<SceneTreeEntry>) {
        let node = match tree.get_node(id) {
            Some(n) => n,
            None => return,
        };
        let path = tree.node_path(id).unwrap_or_default();
        let children: Vec<NodeId> = node.children().to_vec();
        out.push(SceneTreeEntry {
            id,
            name: node.name().to_string(),
            class_name: node.class_name().to_string(),
            path,
            depth,
        });
        for child_id in children {
            Self::collect_entries(tree, child_id, depth + 1, out);
        }
    }
}

impl Default for SceneTreeDock {
    fn default() -> Self {
        Self::new()
    }
}

impl DockPanel for SceneTreeDock {
    fn title(&self) -> &str {
        "Scene"
    }

    fn refresh(&mut self, tree: &SceneTree) {
        self.entries.clear();
        Self::collect_entries(tree, tree.root_id(), 0, &mut self.entries);
        tracing::debug!("SceneTreeDock refreshed: {} entries", self.entries.len());
    }
}

/// A dock panel wrapping the property inspector.
///
/// Delegates to an [`InspectorPanel`] and displays the properties of
/// the currently inspected node.
#[derive(Debug)]
pub struct PropertyDock {
    /// The underlying inspector.
    inspector: InspectorPanel,
}

impl PropertyDock {
    /// Creates a new property dock.
    pub fn new() -> Self {
        Self {
            inspector: InspectorPanel::new(),
        }
    }

    /// Returns a reference to the underlying inspector.
    pub fn inspector(&self) -> &InspectorPanel {
        &self.inspector
    }

    /// Returns a mutable reference to the underlying inspector.
    pub fn inspector_mut(&mut self) -> &mut InspectorPanel {
        &mut self.inspector
    }
}

impl Default for PropertyDock {
    fn default() -> Self {
        Self::new()
    }
}

impl DockPanel for PropertyDock {
    fn title(&self) -> &str {
        "Inspector"
    }

    fn refresh(&mut self, _tree: &SceneTree) {
        // The inspector reads properties on demand — nothing to cache.
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gdscene::node::Node;

    fn make_tree() -> SceneTree {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let main = Node::new("Main", "Node");
        let main_id = tree.add_child(root, main).unwrap();
        let player = Node::new("Player", "Node2D");
        tree.add_child(main_id, player).unwrap();
        let enemy = Node::new("Enemy", "Sprite2D");
        tree.add_child(main_id, enemy).unwrap();
        tree
    }

    #[test]
    fn scene_tree_dock_refresh() {
        let tree = make_tree();
        let mut dock = SceneTreeDock::new();
        dock.refresh(&tree);

        assert_eq!(dock.entries().len(), 4); // root, Main, Player, Enemy
        assert_eq!(dock.entries()[0].name, "root");
        assert_eq!(dock.entries()[0].depth, 0);
        assert_eq!(dock.entries()[1].name, "Main");
        assert_eq!(dock.entries()[1].depth, 1);
        assert_eq!(dock.entries()[2].name, "Player");
        assert_eq!(dock.entries()[2].depth, 2);
    }

    #[test]
    fn scene_tree_dock_paths() {
        let tree = make_tree();
        let mut dock = SceneTreeDock::new();
        dock.refresh(&tree);

        assert_eq!(dock.entries()[0].path, "/root");
        assert_eq!(dock.entries()[2].path, "/root/Main/Player");
    }

    #[test]
    fn scene_tree_dock_title() {
        let dock = SceneTreeDock::new();
        assert_eq!(dock.title(), "Scene");
    }

    #[test]
    fn property_dock_title() {
        let dock = PropertyDock::new();
        assert_eq!(dock.title(), "Inspector");
    }

    #[test]
    fn find_entry_by_id() {
        let tree = make_tree();
        let mut dock = SceneTreeDock::new();
        dock.refresh(&tree);

        let root_id = tree.root_id();
        let entry = dock.find_entry(root_id).unwrap();
        assert_eq!(entry.name, "root");
    }

    #[test]
    fn find_nonexistent_entry() {
        let dock = SceneTreeDock::new();
        assert!(dock.find_entry(NodeId::next()).is_none());
    }
}
