//! SceneTree management and main loop integration.
//!
//! The [`SceneTree`] owns all nodes in a flat [`HashMap`] (arena-style
//! allocation) and provides operations for building, querying, and
//! traversing the node hierarchy.

use std::collections::{HashMap, HashSet};

use gdcore::error::{EngineError, EngineResult};
use gdobject::signal::SignalStore;

use crate::node::{Node, NodeId};

/// The scene tree — an arena that owns every node and maintains the
/// hierarchy.
///
/// A root node is created automatically and is always present.
#[derive(Debug)]
pub struct SceneTree {
    /// Arena storage: every node in the tree lives here.
    nodes: HashMap<NodeId, Node>,
    /// The ID of the root node (always valid).
    root_id: NodeId,
    /// Group index: group name -> set of member NodeIds.
    groups: HashMap<String, HashSet<NodeId>>,
    /// Per-node signal stores. Lazily created when a signal is connected.
    signal_stores: HashMap<NodeId, SignalStore>,
}

impl SceneTree {
    /// Creates a new scene tree with an empty root node named `"root"`.
    pub fn new() -> Self {
        let root = Node::new("root", "Node");
        let root_id = root.id();
        let mut nodes = HashMap::new();
        nodes.insert(root_id, root);
        Self {
            nodes,
            root_id,
            groups: HashMap::new(),
            signal_stores: HashMap::new(),
        }
    }

    /// Returns the ID of the root node.
    pub fn root_id(&self) -> NodeId {
        self.root_id
    }

    /// Returns a reference to a node by ID.
    pub fn get_node(&self, id: NodeId) -> Option<&Node> {
        self.nodes.get(&id)
    }

    /// Returns a mutable reference to a node by ID.
    pub fn get_node_mut(&mut self, id: NodeId) -> Option<&mut Node> {
        self.nodes.get_mut(&id)
    }

    /// Returns the total number of nodes in the tree (including root).
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    // -- hierarchy manipulation ---------------------------------------------

    /// Adds a node as a child of `parent_id`.
    ///
    /// Returns the [`NodeId`] of the newly inserted node.
    pub fn add_child(&mut self, parent_id: NodeId, mut node: Node) -> EngineResult<NodeId> {
        if !self.nodes.contains_key(&parent_id) {
            return Err(EngineError::NotFound(format!(
                "parent node {parent_id} not found"
            )));
        }

        let child_id = node.id();
        node.set_parent(Some(parent_id));
        self.nodes.insert(child_id, node);

        // Update parent's child list.
        if let Some(parent) = self.nodes.get_mut(&parent_id) {
            parent.add_child_id(child_id);
        }

        Ok(child_id)
    }

    /// Removes a node (and all its descendants) from the tree.
    ///
    /// Returns the IDs of all removed nodes in depth-first order
    /// (children before parent).
    pub fn remove_node(&mut self, id: NodeId) -> EngineResult<Vec<NodeId>> {
        if id == self.root_id {
            return Err(EngineError::InvalidOperation(
                "cannot remove the root node".into(),
            ));
        }
        if !self.nodes.contains_key(&id) {
            return Err(EngineError::NotFound(format!("node {id} not found")));
        }

        // Collect subtree in depth-first order (children before parent).
        let mut removed = Vec::new();
        self.collect_subtree_bottom_up(id, &mut removed);

        // Detach from parent.
        if let Some(parent_id) = self.nodes.get(&id).and_then(|n| n.parent()) {
            if let Some(parent) = self.nodes.get_mut(&parent_id) {
                parent.remove_child_id(id);
            }
        }

        // Remove all collected nodes from the arena and group index.
        for &nid in &removed {
            if let Some(node) = self.nodes.remove(&nid) {
                for group in node.groups() {
                    if let Some(members) = self.groups.get_mut(group) {
                        members.remove(&nid);
                    }
                }
            }
        }

        Ok(removed)
    }

    /// Reparents a node to a new parent.
    pub fn reparent(&mut self, node_id: NodeId, new_parent_id: NodeId) -> EngineResult<()> {
        if node_id == self.root_id {
            return Err(EngineError::InvalidOperation(
                "cannot reparent the root node".into(),
            ));
        }
        if !self.nodes.contains_key(&node_id) {
            return Err(EngineError::NotFound(format!("node {node_id} not found")));
        }
        if !self.nodes.contains_key(&new_parent_id) {
            return Err(EngineError::NotFound(format!(
                "new parent {new_parent_id} not found"
            )));
        }

        // Detach from old parent.
        if let Some(old_parent_id) = self.nodes.get(&node_id).and_then(|n| n.parent()) {
            if let Some(old_parent) = self.nodes.get_mut(&old_parent_id) {
                old_parent.remove_child_id(node_id);
            }
        }

        // Attach to new parent.
        if let Some(new_parent) = self.nodes.get_mut(&new_parent_id) {
            new_parent.add_child_id(node_id);
        }
        if let Some(node) = self.nodes.get_mut(&node_id) {
            node.set_parent(Some(new_parent_id));
        }

        Ok(())
    }

    // -- path operations ----------------------------------------------------

    /// Computes the absolute path of a node (e.g. `"/root/Main/Player"`).
    pub fn node_path(&self, id: NodeId) -> Option<String> {
        let mut parts = Vec::new();
        let mut current = id;
        loop {
            let node = self.nodes.get(&current)?;
            parts.push(node.name().to_string());
            if let Some(parent_id) = node.parent() {
                current = parent_id;
            } else {
                break;
            }
        }
        parts.reverse();
        Some(format!("/{}", parts.join("/")))
    }

    /// Looks up a node by its absolute path (e.g. `"/root/Main/Player"`).
    ///
    /// The path must start with `"/"` and the first component must match
    /// the root node's name.
    pub fn get_node_by_path(&self, path: &str) -> Option<NodeId> {
        if !path.starts_with('/') {
            return None;
        }
        let parts: Vec<&str> = path[1..].split('/').collect();
        if parts.is_empty() {
            return None;
        }

        let root = self.nodes.get(&self.root_id)?;
        if root.name() != parts[0] {
            return None;
        }

        let mut current_id = self.root_id;
        for &part in &parts[1..] {
            current_id = self.find_child_by_name(current_id, part)?;
        }
        Some(current_id)
    }

    /// Resolves a relative path from a given node (e.g. `"Player/Sprite"`).
    pub fn get_node_relative(&self, from: NodeId, rel_path: &str) -> Option<NodeId> {
        if rel_path.is_empty() {
            return Some(from);
        }
        let parts: Vec<&str> = rel_path.split('/').collect();
        let mut current = from;
        for &part in &parts {
            match part {
                ".." => {
                    current = self.nodes.get(&current)?.parent()?;
                }
                "." => { /* stay */ }
                name => {
                    current = self.find_child_by_name(current, name)?;
                }
            }
        }
        Some(current)
    }

    /// Finds a direct child of `parent` whose name matches.
    fn find_child_by_name(&self, parent: NodeId, name: &str) -> Option<NodeId> {
        let parent_node = self.nodes.get(&parent)?;
        for &child_id in parent_node.children() {
            if let Some(child) = self.nodes.get(&child_id) {
                if child.name() == name {
                    return Some(child_id);
                }
            }
        }
        None
    }

    // -- group management ---------------------------------------------------

    /// Adds a node to a named group.
    pub fn add_to_group(&mut self, id: NodeId, group: &str) -> EngineResult<()> {
        let node = self
            .nodes
            .get_mut(&id)
            .ok_or_else(|| EngineError::NotFound(format!("node {id} not found")))?;
        node.add_to_group(group);
        self.groups
            .entry(group.to_owned())
            .or_default()
            .insert(id);
        Ok(())
    }

    /// Removes a node from a named group.
    pub fn remove_from_group(&mut self, id: NodeId, group: &str) -> EngineResult<()> {
        let node = self
            .nodes
            .get_mut(&id)
            .ok_or_else(|| EngineError::NotFound(format!("node {id} not found")))?;
        node.remove_from_group(group);
        if let Some(members) = self.groups.get_mut(group) {
            members.remove(&id);
        }
        Ok(())
    }

    /// Returns all node IDs in the given group.
    pub fn get_nodes_in_group(&self, group: &str) -> Vec<NodeId> {
        self.groups
            .get(group)
            .map(|s| s.iter().copied().collect())
            .unwrap_or_default()
    }

    // -- traversal helpers --------------------------------------------------

    /// Collects the subtree rooted at `id` in top-down (parent-first) order.
    pub fn collect_subtree_top_down(&self, id: NodeId, out: &mut Vec<NodeId>) {
        out.push(id);
        if let Some(node) = self.nodes.get(&id) {
            for &child in node.children() {
                self.collect_subtree_top_down(child, out);
            }
        }
    }

    /// Collects the subtree rooted at `id` in bottom-up (children-first) order.
    pub fn collect_subtree_bottom_up(&self, id: NodeId, out: &mut Vec<NodeId>) {
        if let Some(node) = self.nodes.get(&id) {
            for &child in node.children() {
                self.collect_subtree_bottom_up(child, out);
            }
        }
        out.push(id);
    }

    /// Returns IDs of all nodes in tree order (depth-first, top-down)
    /// starting from the root.
    pub fn all_nodes_in_tree_order(&self) -> Vec<NodeId> {
        let mut out = Vec::new();
        self.collect_subtree_top_down(self.root_id, &mut out);
        out
    }

    // -- signal management --------------------------------------------------

    /// Returns a reference to a node's signal store, if it exists.
    pub fn signal_store(&self, id: NodeId) -> Option<&SignalStore> {
        self.signal_stores.get(&id)
    }

    /// Returns a mutable reference to a node's signal store, creating it
    /// if it doesn't already exist.
    pub fn signal_store_mut(&mut self, id: NodeId) -> &mut SignalStore {
        self.signal_stores.entry(id).or_default()
    }

    /// Connects a signal on the `source` node. The signal store for
    /// `source` is created lazily if needed.
    pub fn connect_signal(
        &mut self,
        source: NodeId,
        signal_name: &str,
        connection: gdobject::signal::Connection,
    ) {
        self.signal_store_mut(source).connect(signal_name, connection);
    }

    /// Emits a signal on the given node, returning the collected return
    /// values from all connected callbacks.
    pub fn emit_signal(
        &self,
        source: NodeId,
        signal_name: &str,
        args: &[gdvariant::Variant],
    ) -> Vec<gdvariant::Variant> {
        self.signal_stores
            .get(&source)
            .map_or_else(Vec::new, |store| store.emit(signal_name, args))
    }

    // -- process stub -------------------------------------------------------

    /// Dispatches [`NOTIFICATION_PROCESS`](gdobject::NOTIFICATION_PROCESS)
    /// to every node in tree order.
    ///
    /// This is a simplified stub — a real implementation would track
    /// `delta` time and respect pause mode.
    pub fn process_frame(&mut self) {
        let ids = self.all_nodes_in_tree_order();
        for id in ids {
            if let Some(node) = self.nodes.get_mut(&id) {
                node.receive_notification(gdobject::NOTIFICATION_PROCESS);
            }
        }
    }

    /// Dispatches [`NOTIFICATION_PHYSICS_PROCESS`](gdobject::NOTIFICATION_PHYSICS_PROCESS)
    /// to every node in tree order.
    ///
    /// Called once per fixed-timestep physics tick.
    pub fn process_physics_frame(&mut self) {
        let ids = self.all_nodes_in_tree_order();
        for id in ids {
            if let Some(node) = self.nodes.get_mut(&id) {
                node.receive_notification(gdobject::NOTIFICATION_PHYSICS_PROCESS);
            }
        }
    }
}

impl Default for SceneTree {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_tree_has_root() {
        let tree = SceneTree::new();
        assert_eq!(tree.node_count(), 1);
        let root = tree.get_node(tree.root_id()).unwrap();
        assert_eq!(root.name(), "root");
    }

    #[test]
    fn add_children_and_lookup_by_path() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let main = Node::new("Main", "Node");
        let main_id = tree.add_child(root, main).unwrap();

        let player = Node::new("Player", "Node2D");
        let player_id = tree.add_child(main_id, player).unwrap();

        assert_eq!(tree.node_count(), 3);

        // Absolute path lookup.
        assert_eq!(
            tree.get_node_by_path("/root/Main/Player"),
            Some(player_id)
        );
        assert_eq!(tree.get_node_by_path("/root/Main"), Some(main_id));
        assert_eq!(tree.get_node_by_path("/root"), Some(root));
        assert_eq!(tree.get_node_by_path("/root/Missing"), None);
    }

    #[test]
    fn node_path_computation() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let a = Node::new("A", "Node");
        let a_id = tree.add_child(root, a).unwrap();

        let b = Node::new("B", "Node");
        let b_id = tree.add_child(a_id, b).unwrap();

        assert_eq!(tree.node_path(root).unwrap(), "/root");
        assert_eq!(tree.node_path(a_id).unwrap(), "/root/A");
        assert_eq!(tree.node_path(b_id).unwrap(), "/root/A/B");
    }

    #[test]
    fn relative_path_navigation() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let a = Node::new("A", "Node");
        let a_id = tree.add_child(root, a).unwrap();

        let b = Node::new("B", "Node");
        let b_id = tree.add_child(a_id, b).unwrap();

        assert_eq!(tree.get_node_relative(root, "A/B"), Some(b_id));
        assert_eq!(tree.get_node_relative(b_id, ".."), Some(a_id));
        assert_eq!(tree.get_node_relative(b_id, "../.."), Some(root));
    }

    #[test]
    fn remove_subtree() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let a = Node::new("A", "Node");
        let a_id = tree.add_child(root, a).unwrap();

        let b = Node::new("B", "Node");
        let _b_id = tree.add_child(a_id, b).unwrap();

        assert_eq!(tree.node_count(), 3);

        let removed = tree.remove_node(a_id).unwrap();
        assert_eq!(removed.len(), 2); // B then A
        assert_eq!(tree.node_count(), 1); // only root remains
    }

    #[test]
    fn cannot_remove_root() {
        let mut tree = SceneTree::new();
        let result = tree.remove_node(tree.root_id());
        assert!(result.is_err());
    }

    #[test]
    fn reparent_node() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let a = Node::new("A", "Node");
        let a_id = tree.add_child(root, a).unwrap();

        let b = Node::new("B", "Node");
        let b_id = tree.add_child(root, b).unwrap();

        let c = Node::new("C", "Node");
        let c_id = tree.add_child(a_id, c).unwrap();

        // Move C from under A to under B.
        tree.reparent(c_id, b_id).unwrap();

        assert_eq!(tree.get_node(a_id).unwrap().children().len(), 0);
        assert_eq!(tree.get_node(b_id).unwrap().children(), &[c_id]);
        assert_eq!(tree.node_path(c_id).unwrap(), "/root/B/C");
    }

    #[test]
    fn group_management() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let a = Node::new("A", "Node");
        let a_id = tree.add_child(root, a).unwrap();

        let b = Node::new("B", "Node");
        let b_id = tree.add_child(root, b).unwrap();

        tree.add_to_group(a_id, "enemies").unwrap();
        tree.add_to_group(b_id, "enemies").unwrap();
        tree.add_to_group(a_id, "visible").unwrap();

        let enemies = tree.get_nodes_in_group("enemies");
        assert_eq!(enemies.len(), 2);
        assert!(enemies.contains(&a_id));
        assert!(enemies.contains(&b_id));

        tree.remove_from_group(a_id, "enemies").unwrap();
        let enemies = tree.get_nodes_in_group("enemies");
        assert_eq!(enemies.len(), 1);
        assert!(enemies.contains(&b_id));
    }

    #[test]
    fn process_frame_dispatches_notification() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let child = Node::new("Child", "Node");
        let child_id = tree.add_child(root, child).unwrap();

        tree.process_frame();

        let root_log = tree.get_node(root).unwrap().notification_log();
        assert_eq!(root_log.len(), 1);
        assert_eq!(root_log[0], gdobject::NOTIFICATION_PROCESS);

        let child_log = tree.get_node(child_id).unwrap().notification_log();
        assert_eq!(child_log.len(), 1);
        assert_eq!(child_log[0], gdobject::NOTIFICATION_PROCESS);
    }

    #[test]
    fn add_child_to_nonexistent_parent_returns_error() {
        let mut tree = SceneTree::new();
        let fake_parent = crate::node::NodeId::next();
        let child = Node::new("Orphan", "Node");
        let result = tree.add_child(fake_parent, child);
        assert!(result.is_err());
    }

    #[test]
    fn remove_nonexistent_node_returns_error() {
        let mut tree = SceneTree::new();
        let fake_id = crate::node::NodeId::next();
        let result = tree.remove_node(fake_id);
        assert!(result.is_err());
    }

    #[test]
    fn deeply_nested_path_computation() {
        let mut tree = SceneTree::new();
        let mut parent_id = tree.root_id();
        let mut ids = vec![parent_id];

        for i in 0..12 {
            let node = Node::new(format!("L{i}"), "Node");
            let id = tree.add_child(parent_id, node).unwrap();
            ids.push(id);
            parent_id = id;
        }

        let deepest = *ids.last().unwrap();
        let path = tree.node_path(deepest).unwrap();
        assert!(path.starts_with("/root/L0/L1/L2"));
        assert!(path.ends_with("/L11"));

        let parts: Vec<&str> = path[1..].split('/').collect();
        assert_eq!(parts.len(), 13); // root + L0..L11
    }

    #[test]
    fn get_node_by_path_invalid_not_starting_with_slash() {
        let tree = SceneTree::new();
        assert_eq!(tree.get_node_by_path("root"), None);
    }

    #[test]
    fn get_node_by_path_wrong_root_name() {
        let tree = SceneTree::new();
        assert_eq!(tree.get_node_by_path("/wrong_root"), None);
    }

    #[test]
    fn relative_path_self_reference() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let node = Node::new("A", "Node");
        let a_id = tree.add_child(root, node).unwrap();
        assert_eq!(tree.get_node_relative(a_id, ""), Some(a_id));
    }

    #[test]
    fn reparent_root_fails() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let node = Node::new("A", "Node");
        let a_id = tree.add_child(root, node).unwrap();
        let result = tree.reparent(root, a_id);
        assert!(result.is_err());
    }

    #[test]
    fn reparent_nonexistent_node_fails() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let fake = crate::node::NodeId::next();
        assert!(tree.reparent(fake, root).is_err());
    }

    #[test]
    fn reparent_to_nonexistent_parent_fails() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let node = Node::new("A", "Node");
        let a_id = tree.add_child(root, node).unwrap();
        let fake = crate::node::NodeId::next();
        assert!(tree.reparent(a_id, fake).is_err());
    }

    #[test]
    fn physics_frame_dispatches_notification() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let child = Node::new("C", "Node");
        let child_id = tree.add_child(root, child).unwrap();

        tree.process_physics_frame();

        let log = tree.get_node(child_id).unwrap().notification_log();
        assert_eq!(log.len(), 1);
        assert_eq!(log[0], gdobject::NOTIFICATION_PHYSICS_PROCESS);
    }

    #[test]
    fn default_scene_tree() {
        let tree = SceneTree::default();
        assert_eq!(tree.node_count(), 1);
    }

    #[test]
    fn node_path_of_nonexistent_returns_none() {
        let tree = SceneTree::new();
        let fake = crate::node::NodeId::next();
        assert!(tree.node_path(fake).is_none());
    }

    #[test]
    fn get_nodes_in_empty_group() {
        let tree = SceneTree::new();
        assert!(tree.get_nodes_in_group("nonexistent").is_empty());
    }

    #[test]
    fn remove_subtree_cleans_groups() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let node = Node::new("A", "Node");
        let a_id = tree.add_child(root, node).unwrap();
        tree.add_to_group(a_id, "enemies").unwrap();
        assert_eq!(tree.get_nodes_in_group("enemies").len(), 1);

        tree.remove_node(a_id).unwrap();
        assert!(tree.get_nodes_in_group("enemies").is_empty());
    }

    #[test]
    fn unicode_node_names_in_tree() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let node = Node::new("プレイヤー", "Node2D");
        let id = tree.add_child(root, node).unwrap();
        assert_eq!(tree.node_path(id).unwrap(), "/root/プレイヤー");
        assert_eq!(tree.get_node_by_path("/root/プレイヤー"), Some(id));
    }

    #[test]
    fn empty_name_node_in_tree() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let node = Node::new("", "Node");
        let id = tree.add_child(root, node).unwrap();
        assert_eq!(tree.node_path(id).unwrap(), "/root/");
    }
}
