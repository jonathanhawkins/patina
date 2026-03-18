//! SceneTree management and main loop integration.
//!
//! The [`SceneTree`] owns all nodes in a flat [`HashMap`] (arena-style
//! allocation) and provides operations for building, querying, and
//! traversing the node hierarchy.

use std::collections::{HashMap, HashSet};

use gdcore::error::{EngineError, EngineResult};
use gdobject::signal::SignalStore;
use gdscript_interop::bindings::ScriptInstance;
use gdvariant::Variant;

use crate::animation::AnimationPlayer;
use crate::node::{Node, NodeId};
use crate::tween::{Tween, TweenId};

/// The scene tree — an arena that owns every node and maintains the
/// hierarchy.
///
/// A root node is created automatically and is always present.
pub struct SceneTree {
    /// Arena storage: every node in the tree lives here.
    nodes: HashMap<NodeId, Node>,
    /// The ID of the root node (always valid).
    root_id: NodeId,
    /// Group index: group name -> set of member NodeIds.
    groups: HashMap<String, HashSet<NodeId>>,
    /// Per-node signal stores. Lazily created when a signal is connected.
    signal_stores: HashMap<NodeId, SignalStore>,
    /// Per-node animation players.
    animation_players: HashMap<NodeId, AnimationPlayer>,
    /// Active tweens: maps TweenId -> (owning NodeId, Tween).
    tweens: HashMap<TweenId, (NodeId, Tween)>,
    /// Per-node script instances. Attached scripts receive lifecycle
    /// callbacks (_ready, _process, etc.) during scene execution.
    scripts: HashMap<NodeId, Box<dyn ScriptInstance>>,
}

impl std::fmt::Debug for SceneTree {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SceneTree")
            .field("nodes", &self.nodes)
            .field("root_id", &self.root_id)
            .field("groups", &self.groups)
            .field("scripts", &format!("({} scripts)", self.scripts.len()))
            .finish()
    }
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
            animation_players: HashMap::new(),
            tweens: HashMap::new(),
            scripts: HashMap::new(),
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
        self.groups.entry(group.to_owned()).or_default().insert(id);
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

    /// Returns all signal stores (node ID -> signal store).
    pub fn signal_stores(&self) -> &HashMap<NodeId, SignalStore> {
        &self.signal_stores
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
        self.signal_store_mut(source)
            .connect(signal_name, connection);
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

    // -- get_node_or_null ---------------------------------------------------

    /// Resolves a relative path from `from`, returning `None` instead of
    /// panicking when the path is invalid. This is the non-panicking
    /// counterpart to Godot's `get_node()`.
    pub fn get_node_or_null(&self, from: NodeId, path: &str) -> Option<NodeId> {
        if path.starts_with('/') {
            // Absolute path — delegate to get_node_by_path.
            self.get_node_by_path(path)
        } else {
            self.get_node_relative(from, path)
        }
    }

    // -- get_index ----------------------------------------------------------

    /// Returns the index of `node_id` among its parent's children, or `None`
    /// if the node has no parent or is not found.
    pub fn get_index(&self, node_id: NodeId) -> Option<usize> {
        let node = self.nodes.get(&node_id)?;
        let parent_id = node.parent()?;
        let parent = self.nodes.get(&parent_id)?;
        parent.children().iter().position(|&c| c == node_id)
    }

    // -- duplicate_subtree --------------------------------------------------

    /// Deep-clones a node and all its descendants, assigning fresh IDs.
    ///
    /// Returns the cloned nodes as a flat `Vec` with parent/child
    /// relationships already wired (using the new IDs). The first element
    /// is the clone of `root_id`. The cloned nodes are **not** inserted
    /// into the tree — the caller can add them via [`SceneTree::add_child`].
    pub fn duplicate_subtree(&self, root_id: NodeId) -> EngineResult<Vec<Node>> {
        let _source = self
            .nodes
            .get(&root_id)
            .ok_or_else(|| EngineError::NotFound(format!("node {root_id} not found")))?;

        // Collect subtree in top-down order.
        let mut ids = Vec::new();
        self.collect_subtree_top_down(root_id, &mut ids);

        let mut old_to_new: HashMap<NodeId, NodeId> = HashMap::new();
        let mut cloned: Vec<Node> = Vec::with_capacity(ids.len());

        for &old_id in &ids {
            let original = self
                .nodes
                .get(&old_id)
                .ok_or_else(|| EngineError::NotFound(format!("node {old_id} disappeared")))?;

            let mut clone = Node::new(original.name(), original.class_name());
            // Copy properties.
            for (key, value) in original.properties() {
                clone.set_property(key, value.clone());
            }
            // Copy groups.
            for group in original.groups() {
                clone.add_to_group(group.clone());
            }
            // Copy unique_name flag.
            clone.set_unique_name(original.is_unique_name());

            let new_id = clone.id();
            old_to_new.insert(old_id, new_id);

            // Wire parent (skip the root of the subtree — it has no parent
            // in the cloned output).
            if old_id != root_id {
                if let Some(old_parent) = original.parent() {
                    if let Some(&new_parent) = old_to_new.get(&old_parent) {
                        clone.set_parent(Some(new_parent));
                        // Also add as child of the new parent.
                        if let Some(parent_clone) = cloned.iter_mut().find(|n| n.id() == new_parent)
                        {
                            parent_clone.add_child_id(new_id);
                        }
                    }
                }
            }

            cloned.push(clone);
        }

        Ok(cloned)
    }

    // -- animation player management ----------------------------------------

    /// Attaches an [`AnimationPlayer`] to a node.
    pub fn attach_animation_player(
        &mut self,
        node_id: NodeId,
        player: AnimationPlayer,
    ) -> EngineResult<()> {
        if !self.nodes.contains_key(&node_id) {
            return Err(EngineError::NotFound(format!("node {node_id} not found")));
        }
        self.animation_players.insert(node_id, player);
        Ok(())
    }

    /// Returns a reference to a node's animation player, if attached.
    pub fn get_animation_player(&self, node_id: NodeId) -> Option<&AnimationPlayer> {
        self.animation_players.get(&node_id)
    }

    /// Returns a mutable reference to a node's animation player, if attached.
    pub fn get_animation_player_mut(&mut self, node_id: NodeId) -> Option<&mut AnimationPlayer> {
        self.animation_players.get_mut(&node_id)
    }

    /// Advances all animation players by `delta` seconds and applies sampled
    /// values to the corresponding node properties.
    pub fn process_animations(&mut self, delta: f64) {
        // Collect node_ids to avoid borrowing self twice.
        let node_ids: Vec<NodeId> = self.animation_players.keys().copied().collect();
        for node_id in node_ids {
            if let Some(player) = self.animation_players.get_mut(&node_id) {
                player.advance(delta);
                let values = player.get_current_values();
                for (path, value) in values {
                    if let Some(node) = self.nodes.get_mut(&node_id) {
                        node.set_property(&path, value);
                    }
                }
            }
        }
    }

    // -- tween management ---------------------------------------------------

    /// Creates a new tween associated with a node. Returns the [`TweenId`].
    ///
    /// The tween is stored but not yet started — call
    /// [`get_tween_mut`](Self::get_tween_mut) to configure and start it.
    pub fn create_tween(&mut self, node_id: NodeId) -> TweenId {
        let id = TweenId::next();
        self.tweens.insert(id, (node_id, Tween::new()));
        id
    }

    /// Inserts an already-configured tween for the given node.
    pub fn add_tween(&mut self, node_id: NodeId, tween: Tween) -> TweenId {
        let id = TweenId::next();
        self.tweens.insert(id, (node_id, tween));
        id
    }

    /// Returns a mutable reference to a tween by ID.
    pub fn get_tween_mut(&mut self, tween_id: TweenId) -> Option<&mut Tween> {
        self.tweens.get_mut(&tween_id).map(|(_, t)| t)
    }

    /// Returns the number of active tweens.
    pub fn tween_count(&self) -> usize {
        self.tweens.len()
    }

    /// Advances all tweens by `delta` seconds, applies interpolated values
    /// to the corresponding node properties, and removes completed
    /// non-looping tweens.
    pub fn process_tweens(&mut self, delta: f64) {
        let mut completed = Vec::new();
        let tween_ids: Vec<TweenId> = self.tweens.keys().copied().collect();
        for tween_id in tween_ids {
            if let Some((node_id, tween)) = self.tweens.get_mut(&tween_id) {
                let node_id = *node_id;
                let done = tween.advance(delta);
                let values = tween.get_current_values();
                for (path, value) in values {
                    if let Some(node) = self.nodes.get_mut(&node_id) {
                        node.set_property(&path, value);
                    }
                }
                if done {
                    completed.push(tween_id);
                }
            }
        }
        for id in completed {
            self.tweens.remove(&id);
        }
    }

    // -- script store -------------------------------------------------------

    /// Attaches a script instance to a node.
    pub fn attach_script(&mut self, node_id: NodeId, script: Box<dyn ScriptInstance>) {
        self.scripts.insert(node_id, script);
    }

    /// Detaches the script from a node, if any.
    pub fn detach_script(&mut self, node_id: NodeId) -> Option<Box<dyn ScriptInstance>> {
        self.scripts.remove(&node_id)
    }

    /// Returns `true` if the node has an attached script.
    pub fn has_script(&self, node_id: NodeId) -> bool {
        self.scripts.contains_key(&node_id)
    }

    /// Returns a reference to the node's script instance.
    pub fn get_script(&self, node_id: NodeId) -> Option<&dyn ScriptInstance> {
        self.scripts.get(&node_id).map(|s| s.as_ref())
    }

    /// Returns a mutable reference to the node's script instance.
    pub fn get_script_mut(&mut self, node_id: NodeId) -> Option<&mut Box<dyn ScriptInstance>> {
        self.scripts.get_mut(&node_id)
    }

    // -- script callbacks ---------------------------------------------------

    /// Calls `_ready()` on the node's script, if present.
    pub fn process_script_ready(&mut self, node_id: NodeId) {
        if let Some(script) = self.scripts.get_mut(&node_id) {
            // Ignore MethodNotFound — the script may not define _ready.
            let _ = script.call_method("_ready", &[]);
        }
    }

    /// Calls `_process(delta)` on the node's script, if present.
    pub fn process_script_process(&mut self, node_id: NodeId, delta: f64) {
        if let Some(script) = self.scripts.get_mut(&node_id) {
            let _ = script.call_method("_process", &[Variant::Float(delta)]);
        }
    }

    /// Calls `_physics_process(delta)` on the node's script, if present.
    pub fn process_script_physics_process(&mut self, node_id: NodeId, delta: f64) {
        if let Some(script) = self.scripts.get_mut(&node_id) {
            let _ = script.call_method("_physics_process", &[Variant::Float(delta)]);
        }
    }

    /// Calls `_enter_tree()` on the node's script, if present.
    pub fn process_script_enter_tree(&mut self, node_id: NodeId) {
        if let Some(script) = self.scripts.get_mut(&node_id) {
            let _ = script.call_method("_enter_tree", &[]);
        }
    }

    /// Calls `_exit_tree()` on the node's script, if present.
    pub fn process_script_exit_tree(&mut self, node_id: NodeId) {
        if let Some(script) = self.scripts.get_mut(&node_id) {
            let _ = script.call_method("_exit_tree", &[]);
        }
    }

    /// Calls `_process(delta)` on all nodes that have attached scripts.
    pub fn process_all_scripts_process(&mut self, delta: f64) {
        let ids: Vec<NodeId> = self.scripts.keys().copied().collect();
        for id in ids {
            self.process_script_process(id, delta);
        }
    }

    /// Calls `_physics_process(delta)` on all nodes that have attached scripts.
    pub fn process_all_scripts_physics_process(&mut self, delta: f64) {
        let ids: Vec<NodeId> = self.scripts.keys().copied().collect();
        for id in ids {
            self.process_script_physics_process(id, delta);
        }
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
        assert_eq!(tree.get_node_by_path("/root/Main/Player"), Some(player_id));
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

    // -- AnimationPlayer store tests ----------------------------------------

    mod animation_store_tests {
        use super::*;
        use crate::animation::{Animation, AnimationPlayer, AnimationTrack, KeyFrame, LoopMode};
        use gdcore::math::Vector2;
        use gdvariant::Variant;

        /// Helper: build a tree with root + one Node2D child.
        fn tree_with_node() -> (SceneTree, NodeId) {
            let mut tree = SceneTree::new();
            let root = tree.root_id();
            let node = Node::new("Sprite", "Node2D");
            let id = tree.add_child(root, node).unwrap();
            (tree, id)
        }

        #[test]
        fn attach_and_get_animation_player() {
            let (mut tree, id) = tree_with_node();
            let player = AnimationPlayer::new();
            tree.attach_animation_player(id, player).unwrap();
            assert!(tree.get_animation_player(id).is_some());
        }

        #[test]
        fn attach_animation_player_to_nonexistent_node_fails() {
            let mut tree = SceneTree::new();
            let fake = NodeId::next();
            let result = tree.attach_animation_player(fake, AnimationPlayer::new());
            assert!(result.is_err());
        }

        #[test]
        fn get_animation_player_mut_works() {
            let (mut tree, id) = tree_with_node();
            let player = AnimationPlayer::new();
            tree.attach_animation_player(id, player).unwrap();
            let p = tree.get_animation_player_mut(id).unwrap();
            p.speed_scale = 2.0;
            assert_eq!(tree.get_animation_player(id).unwrap().speed_scale, 2.0);
        }

        #[test]
        fn process_animations_applies_float_property() {
            let (mut tree, id) = tree_with_node();

            let mut player = AnimationPlayer::new();
            let mut anim = Animation::new("fade", 1.0);
            let mut track = AnimationTrack::new("opacity");
            track.add_keyframe(KeyFrame::linear(0.0, Variant::Float(1.0)));
            track.add_keyframe(KeyFrame::linear(1.0, Variant::Float(0.0)));
            anim.tracks.push(track);
            player.add_animation(anim);
            player.play("fade");

            tree.attach_animation_player(id, player).unwrap();

            // Advance half-way.
            tree.process_animations(0.5);

            let val = tree.get_node(id).unwrap().get_property("opacity");
            if let Variant::Float(f) = val {
                assert!((f - 0.5).abs() < 1e-4, "got {f}");
            } else {
                panic!("expected Float, got {val:?}");
            }
        }

        #[test]
        fn process_animations_applies_vector2_position() {
            let (mut tree, id) = tree_with_node();

            let mut player = AnimationPlayer::new();
            let mut anim = Animation::new("move", 1.0);
            let mut track = AnimationTrack::new("position");
            track.add_keyframe(KeyFrame::linear(0.0, Variant::Vector2(Vector2::ZERO)));
            track.add_keyframe(KeyFrame::linear(
                1.0,
                Variant::Vector2(Vector2::new(100.0, 200.0)),
            ));
            anim.tracks.push(track);
            player.add_animation(anim);
            player.play("move");

            tree.attach_animation_player(id, player).unwrap();
            tree.process_animations(0.5);

            let val = tree.get_node(id).unwrap().get_property("position");
            if let Variant::Vector2(v) = val {
                assert!((v.x - 50.0).abs() < 1e-3);
                assert!((v.y - 100.0).abs() < 1e-3);
            } else {
                panic!("expected Vector2, got {val:?}");
            }
        }

        #[test]
        fn multiple_animations_on_different_nodes() {
            let mut tree = SceneTree::new();
            let root = tree.root_id();

            let a = Node::new("A", "Node2D");
            let a_id = tree.add_child(root, a).unwrap();

            let b = Node::new("B", "Node2D");
            let b_id = tree.add_child(root, b).unwrap();

            // Player for A: animate "x"
            let mut pa = AnimationPlayer::new();
            let mut anim_a = Animation::new("go", 1.0);
            let mut track_a = AnimationTrack::new("x");
            track_a.add_keyframe(KeyFrame::linear(0.0, Variant::Float(0.0)));
            track_a.add_keyframe(KeyFrame::linear(1.0, Variant::Float(10.0)));
            anim_a.tracks.push(track_a);
            pa.add_animation(anim_a);
            pa.play("go");
            tree.attach_animation_player(a_id, pa).unwrap();

            // Player for B: animate "y"
            let mut pb = AnimationPlayer::new();
            let mut anim_b = Animation::new("go", 1.0);
            let mut track_b = AnimationTrack::new("y");
            track_b.add_keyframe(KeyFrame::linear(0.0, Variant::Float(100.0)));
            track_b.add_keyframe(KeyFrame::linear(1.0, Variant::Float(200.0)));
            anim_b.tracks.push(track_b);
            pb.add_animation(anim_b);
            pb.play("go");
            tree.attach_animation_player(b_id, pb).unwrap();

            tree.process_animations(0.5);

            if let Variant::Float(f) = tree.get_node(a_id).unwrap().get_property("x") {
                assert!((f - 5.0).abs() < 1e-4);
            } else {
                panic!("A.x not Float");
            }
            if let Variant::Float(f) = tree.get_node(b_id).unwrap().get_property("y") {
                assert!((f - 150.0).abs() < 1e-4);
            } else {
                panic!("B.y not Float");
            }
        }

        #[test]
        fn animation_stops_at_end_no_loop() {
            let (mut tree, id) = tree_with_node();

            let mut player = AnimationPlayer::new();
            let mut anim = Animation::new("once", 1.0);
            let mut track = AnimationTrack::new("val");
            track.add_keyframe(KeyFrame::linear(0.0, Variant::Float(0.0)));
            track.add_keyframe(KeyFrame::linear(1.0, Variant::Float(10.0)));
            anim.tracks.push(track);
            player.add_animation(anim);
            player.play("once");

            tree.attach_animation_player(id, player).unwrap();

            // Advance past end.
            tree.process_animations(2.0);

            assert!(!tree.get_animation_player(id).unwrap().playing);
            if let Variant::Float(f) = tree.get_node(id).unwrap().get_property("val") {
                assert!((f - 10.0).abs() < 1e-4);
            } else {
                panic!("expected Float");
            }
        }

        #[test]
        fn animation_loop_linear_wraps() {
            let (mut tree, id) = tree_with_node();

            let mut player = AnimationPlayer::new();
            let mut anim = Animation::new("loop", 1.0);
            anim.loop_mode = LoopMode::Linear;
            let mut track = AnimationTrack::new("val");
            track.add_keyframe(KeyFrame::linear(0.0, Variant::Float(0.0)));
            track.add_keyframe(KeyFrame::linear(1.0, Variant::Float(10.0)));
            anim.tracks.push(track);
            player.add_animation(anim);
            player.play("loop");

            tree.attach_animation_player(id, player).unwrap();

            // Advance 1.5s — should wrap to 0.5
            tree.process_animations(1.5);

            assert!(tree.get_animation_player(id).unwrap().playing);
            if let Variant::Float(f) = tree.get_node(id).unwrap().get_property("val") {
                assert!((f - 5.0).abs() < 1e-4, "expected ~5.0, got {f}");
            } else {
                panic!("expected Float");
            }
        }

        #[test]
        fn animation_pingpong_reverses() {
            let (mut tree, id) = tree_with_node();

            let mut player = AnimationPlayer::new();
            let mut anim = Animation::new("pp", 1.0);
            anim.loop_mode = LoopMode::PingPong;
            let mut track = AnimationTrack::new("val");
            track.add_keyframe(KeyFrame::linear(0.0, Variant::Float(0.0)));
            track.add_keyframe(KeyFrame::linear(1.0, Variant::Float(10.0)));
            anim.tracks.push(track);
            player.add_animation(anim);
            player.play("pp");

            tree.attach_animation_player(id, player).unwrap();

            // Advance 1.5s — should bounce back to position 0.5
            tree.process_animations(1.5);

            assert!(tree.get_animation_player(id).unwrap().playing);
            if let Variant::Float(f) = tree.get_node(id).unwrap().get_property("val") {
                assert!((f - 5.0).abs() < 1e-4, "expected ~5.0, got {f}");
            } else {
                panic!("expected Float");
            }
        }

        #[test]
        fn animation_multiple_tracks_applied() {
            let (mut tree, id) = tree_with_node();

            let mut player = AnimationPlayer::new();
            let mut anim = Animation::new("multi", 1.0);

            let mut t1 = AnimationTrack::new("x");
            t1.add_keyframe(KeyFrame::linear(0.0, Variant::Float(0.0)));
            t1.add_keyframe(KeyFrame::linear(1.0, Variant::Float(10.0)));
            anim.tracks.push(t1);

            let mut t2 = AnimationTrack::new("y");
            t2.add_keyframe(KeyFrame::linear(0.0, Variant::Float(100.0)));
            t2.add_keyframe(KeyFrame::linear(1.0, Variant::Float(200.0)));
            anim.tracks.push(t2);

            player.add_animation(anim);
            player.play("multi");
            tree.attach_animation_player(id, player).unwrap();

            tree.process_animations(0.5);

            if let Variant::Float(f) = tree.get_node(id).unwrap().get_property("x") {
                assert!((f - 5.0).abs() < 1e-4);
            } else {
                panic!("x not Float");
            }
            if let Variant::Float(f) = tree.get_node(id).unwrap().get_property("y") {
                assert!((f - 150.0).abs() < 1e-4);
            } else {
                panic!("y not Float");
            }
        }

        #[test]
        fn stopped_animation_player_does_not_advance() {
            let (mut tree, id) = tree_with_node();

            let mut player = AnimationPlayer::new();
            let mut anim = Animation::new("a", 1.0);
            let mut track = AnimationTrack::new("val");
            track.add_keyframe(KeyFrame::linear(0.0, Variant::Float(0.0)));
            track.add_keyframe(KeyFrame::linear(1.0, Variant::Float(10.0)));
            anim.tracks.push(track);
            player.add_animation(anim);
            // Don't call play() — player is not playing.
            tree.attach_animation_player(id, player).unwrap();

            tree.process_animations(0.5);

            // Property should not have been set (no current animation values).
            assert_eq!(tree.get_node(id).unwrap().get_property("val"), Variant::Nil);
        }
    }

    // -- Tween store tests --------------------------------------------------

    mod tween_store_tests {
        use super::*;
        use crate::tween::{TweenBuilder, TweenId};
        use gdcore::math::Vector2;
        use gdvariant::Variant;

        fn tree_with_node() -> (SceneTree, NodeId) {
            let mut tree = SceneTree::new();
            let root = tree.root_id();
            let node = Node::new("Player", "Node2D");
            let id = tree.add_child(root, node).unwrap();
            (tree, id)
        }

        #[test]
        fn create_tween_returns_id() {
            let (mut tree, id) = tree_with_node();
            let tid = tree.create_tween(id);
            assert!(tree.get_tween_mut(tid).is_some());
            assert_eq!(tree.tween_count(), 1);
        }

        #[test]
        fn add_tween_with_builder() {
            let (mut tree, id) = tree_with_node();
            let tween = TweenBuilder::new()
                .tween_property("x", Variant::Float(0.0), Variant::Float(100.0), 1.0)
                .build();
            let tid = tree.add_tween(id, tween);
            assert!(tree.get_tween_mut(tid).is_some());
        }

        #[test]
        fn process_tweens_applies_float_property() {
            let (mut tree, id) = tree_with_node();
            let tween = TweenBuilder::new()
                .tween_property("speed", Variant::Float(0.0), Variant::Float(100.0), 1.0)
                .build();
            tree.add_tween(id, tween);

            tree.process_tweens(0.5);

            if let Variant::Float(f) = tree.get_node(id).unwrap().get_property("speed") {
                assert!((f - 50.0).abs() < 1e-3, "got {f}");
            } else {
                panic!("expected Float");
            }
        }

        #[test]
        fn tween_node2d_position_updates() {
            let (mut tree, id) = tree_with_node();
            let tween = TweenBuilder::new()
                .tween_property(
                    "position",
                    Variant::Vector2(Vector2::ZERO),
                    Variant::Vector2(Vector2::new(100.0, 200.0)),
                    1.0,
                )
                .build();
            tree.add_tween(id, tween);

            tree.process_tweens(0.5);

            if let Variant::Vector2(v) = tree.get_node(id).unwrap().get_property("position") {
                assert!((v.x - 50.0).abs() < 1e-3);
                assert!((v.y - 100.0).abs() < 1e-3);
            } else {
                panic!("expected Vector2");
            }
        }

        #[test]
        fn tween_completion_removes_from_store() {
            let (mut tree, id) = tree_with_node();
            let tween = TweenBuilder::new()
                .tween_property("x", Variant::Float(0.0), Variant::Float(10.0), 1.0)
                .build();
            tree.add_tween(id, tween);
            assert_eq!(tree.tween_count(), 1);

            // Advance past completion.
            tree.process_tweens(2.0);
            assert_eq!(tree.tween_count(), 0);
        }

        #[test]
        fn tween_looping_does_not_remove() {
            let (mut tree, id) = tree_with_node();
            let tween = TweenBuilder::new()
                .tween_property("x", Variant::Float(0.0), Variant::Float(10.0), 1.0)
                .set_loops(-1)
                .build();
            tree.add_tween(id, tween);

            tree.process_tweens(5.0);
            assert_eq!(tree.tween_count(), 1, "infinite-loop tween should persist");
        }

        #[test]
        fn tween_finite_loops_removes_after_all() {
            let (mut tree, id) = tree_with_node();
            let tween = TweenBuilder::new()
                .tween_property("x", Variant::Float(0.0), Variant::Float(10.0), 1.0)
                .set_loops(2)
                .build();
            tree.add_tween(id, tween);

            tree.process_tweens(1.0); // First loop done
            assert_eq!(
                tree.tween_count(),
                1,
                "should still be alive after first loop"
            );

            tree.process_tweens(1.0); // Second loop done
            assert_eq!(tree.tween_count(), 0, "should be removed after all loops");
        }

        #[test]
        fn multiple_tweens_on_different_nodes() {
            let mut tree = SceneTree::new();
            let root = tree.root_id();

            let a = Node::new("A", "Node2D");
            let a_id = tree.add_child(root, a).unwrap();

            let b = Node::new("B", "Node2D");
            let b_id = tree.add_child(root, b).unwrap();

            let tw_a = TweenBuilder::new()
                .tween_property("x", Variant::Float(0.0), Variant::Float(10.0), 1.0)
                .build();
            tree.add_tween(a_id, tw_a);

            let tw_b = TweenBuilder::new()
                .tween_property("y", Variant::Float(50.0), Variant::Float(150.0), 1.0)
                .build();
            tree.add_tween(b_id, tw_b);

            tree.process_tweens(0.5);

            if let Variant::Float(f) = tree.get_node(a_id).unwrap().get_property("x") {
                assert!((f - 5.0).abs() < 1e-3);
            } else {
                panic!("A.x not Float");
            }
            if let Variant::Float(f) = tree.get_node(b_id).unwrap().get_property("y") {
                assert!((f - 100.0).abs() < 1e-3);
            } else {
                panic!("B.y not Float");
            }
        }

        #[test]
        fn tween_end_value_applied_on_completion() {
            let (mut tree, id) = tree_with_node();
            let tween = TweenBuilder::new()
                .tween_property("x", Variant::Float(0.0), Variant::Float(42.0), 1.0)
                .build();
            tree.add_tween(id, tween);

            tree.process_tweens(1.0);

            if let Variant::Float(f) = tree.get_node(id).unwrap().get_property("x") {
                assert!((f - 42.0).abs() < 1e-4);
            } else {
                panic!("expected Float");
            }
        }

        #[test]
        fn get_nonexistent_tween_returns_none() {
            let mut tree = SceneTree::new();
            let fake = TweenId::next();
            assert!(tree.get_tween_mut(fake).is_none());
        }
    }

    // -- MainLoop animation/tween integration tests -------------------------

    mod mainloop_integration_tests {
        use crate::animation::{Animation, AnimationPlayer, AnimationTrack, KeyFrame};
        use crate::main_loop::MainLoop;
        use crate::node::Node;
        use crate::scene_tree::SceneTree;
        use crate::tween::TweenBuilder;
        use gdcore::math::Vector2;
        use gdvariant::Variant;

        #[test]
        fn mainloop_advances_animation_player() {
            let mut tree = SceneTree::new();
            let root = tree.root_id();
            let node = Node::new("N", "Node2D");
            let id = tree.add_child(root, node).unwrap();

            let mut player = AnimationPlayer::new();
            let mut anim = Animation::new("slide", 1.0);
            let mut track = AnimationTrack::new("position");
            track.add_keyframe(KeyFrame::linear(0.0, Variant::Vector2(Vector2::ZERO)));
            track.add_keyframe(KeyFrame::linear(
                1.0,
                Variant::Vector2(Vector2::new(60.0, 0.0)),
            ));
            anim.tracks.push(track);
            player.add_animation(anim);
            player.play("slide");
            tree.attach_animation_player(id, player).unwrap();

            let mut ml = MainLoop::new(tree);
            // Run 30 frames at 1/60 each = 0.5s
            ml.run_frames(30, 1.0 / 60.0);

            let pos = ml.tree().get_node(id).unwrap().get_property("position");
            if let Variant::Vector2(v) = pos {
                assert!((v.x - 30.0).abs() < 1.0, "expected ~30, got {}", v.x);
            } else {
                panic!("expected Vector2, got {pos:?}");
            }
        }

        #[test]
        fn mainloop_advances_tween() {
            let mut tree = SceneTree::new();
            let root = tree.root_id();
            let node = Node::new("N", "Node2D");
            let id = tree.add_child(root, node).unwrap();

            let tween = TweenBuilder::new()
                .tween_property(
                    "position",
                    Variant::Vector2(Vector2::ZERO),
                    Variant::Vector2(Vector2::new(60.0, 0.0)),
                    1.0,
                )
                .build();
            tree.add_tween(id, tween);

            let mut ml = MainLoop::new(tree);
            ml.run_frames(30, 1.0 / 60.0);

            let pos = ml.tree().get_node(id).unwrap().get_property("position");
            if let Variant::Vector2(v) = pos {
                assert!((v.x - 30.0).abs() < 1.0, "expected ~30, got {}", v.x);
            } else {
                panic!("expected Vector2, got {pos:?}");
            }
        }

        #[test]
        fn mainloop_tween_completes_and_is_removed() {
            let mut tree = SceneTree::new();
            let root = tree.root_id();
            let node = Node::new("N", "Node2D");
            let id = tree.add_child(root, node).unwrap();

            let tween = TweenBuilder::new()
                .tween_property("x", Variant::Float(0.0), Variant::Float(10.0), 0.5)
                .build();
            tree.add_tween(id, tween);

            let mut ml = MainLoop::new(tree);
            // Run 60 frames at 1/60 = 1.0s (tween is 0.5s)
            ml.run_frames(60, 1.0 / 60.0);

            assert_eq!(ml.tree().tween_count(), 0);
            if let Variant::Float(f) = ml.tree().get_node(id).unwrap().get_property("x") {
                assert!((f - 10.0).abs() < 1e-4);
            } else {
                panic!("expected Float");
            }
        }
    }
}
