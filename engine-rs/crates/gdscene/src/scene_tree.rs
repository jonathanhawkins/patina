//! SceneTree management and main loop integration.
//!
//! The [`SceneTree`] owns all nodes in a flat [`HashMap`] (arena-style
//! allocation) and provides operations for building, querying, and
//! traversing the node hierarchy.

use std::collections::{HashMap, HashSet};

use gdcore::error::{EngineError, EngineResult};
use gdobject::notification::{
    NOTIFICATION_CHILD_ORDER_CHANGED, NOTIFICATION_MOVED_IN_PARENT, NOTIFICATION_PARENTED,
    NOTIFICATION_UNPARENTED,
};
use gdobject::signal::SignalStore;
use gdscript_interop::bindings::ScriptInstance;
use gdvariant::Variant;

use crate::animation::AnimationPlayer;
use crate::lifecycle::LifecycleManager;
use crate::node::{Node, NodeId};
use crate::trace::{EventTrace, TraceEvent, TraceEventType};
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
    /// Snapshot of input state for the current frame, used by scripts
    /// that call `Input.is_action_pressed()` etc.
    input_snapshot: Option<crate::scripting::InputSnapshot>,
    /// Nodes marked for deferred deletion via `queue_free()`.
    /// They are removed at the end of the frame by `process_deletions()`.
    pending_deletions: Vec<NodeId>,
    /// Global event trace for lifecycle/signal ordering verification.
    event_trace: EventTrace,
    /// Current frame number for trace recording.
    trace_frame: u64,
    /// Whether the scene tree is currently paused.
    paused: bool,
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
            input_snapshot: None,
            pending_deletions: Vec::new(),
            event_trace: EventTrace::new(),
            trace_frame: 0,
            paused: false,
        }
    }

    /// Returns the ID of the root node.
    pub fn root_id(&self) -> NodeId {
        self.root_id
    }

    /// Sets the input snapshot for the current frame. Scripts will see this
    /// state when they call `Input.is_action_pressed()` etc.
    pub fn set_input_snapshot(&mut self, snapshot: crate::scripting::InputSnapshot) {
        self.input_snapshot = Some(snapshot);
    }

    /// Clears the input snapshot.
    pub fn clear_input_snapshot(&mut self) {
        self.input_snapshot = None;
    }

    /// Returns `true` if an input snapshot is currently set.
    pub fn has_input_snapshot(&self) -> bool {
        self.input_snapshot.is_some()
    }

    /// Returns a reference to the current input snapshot, if any.
    pub fn input_snapshot(&self) -> Option<&crate::scripting::InputSnapshot> {
        self.input_snapshot.as_ref()
    }

    /// Returns a reference to the event trace.
    pub fn event_trace(&self) -> &EventTrace {
        &self.event_trace
    }

    /// Returns a mutable reference to the event trace.
    pub fn event_trace_mut(&mut self) -> &mut EventTrace {
        &mut self.event_trace
    }

    /// Sets the current trace frame counter.
    pub fn set_trace_frame(&mut self, frame: u64) {
        self.trace_frame = frame;
    }

    /// Returns the current trace frame counter.
    pub fn trace_frame(&self) -> u64 {
        self.trace_frame
    }

    /// Returns a reference to a node by ID.
    pub fn get_node(&self, id: NodeId) -> Option<&Node> {
        self.nodes.get(&id)
    }

    /// Returns a mutable reference to a node by ID.
    pub fn get_node_mut(&mut self, id: NodeId) -> Option<&mut Node> {
        self.nodes.get_mut(&id)
    }

    /// Removes a node from the arena by ID **without** detaching it from its
    /// parent or running lifecycle hooks. Use this to extract a freshly
    /// created (unparented) node before passing it to [`add_child`](Self::add_child).
    pub fn take_node(&mut self, id: NodeId) -> Option<Node> {
        self.nodes.remove(&id)
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

        // NOTIFICATION_PARENTED fires when a node gains a parent.
        self.trace_record(child_id, TraceEventType::Notification, "PARENTED");
        if let Some(child) = self.nodes.get_mut(&child_id) {
            child.receive_notification(NOTIFICATION_PARENTED);
        }

        // NOTIFICATION_CHILD_ORDER_CHANGED fires on the parent when a child is added.
        self.trace_record(
            parent_id,
            TraceEventType::Notification,
            "CHILD_ORDER_CHANGED",
        );
        if let Some(parent) = self.nodes.get_mut(&parent_id) {
            parent.receive_notification(NOTIFICATION_CHILD_ORDER_CHANGED);
        }

        let should_enter_tree = self
            .nodes
            .get(&parent_id)
            .map(|parent| parent.is_inside_tree())
            .unwrap_or(false);
        if should_enter_tree {
            LifecycleManager::enter_tree(self, child_id);
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

        let should_exit_tree = self
            .nodes
            .get(&id)
            .map(|node| node.is_inside_tree())
            .unwrap_or(false);
        if should_exit_tree {
            LifecycleManager::exit_tree(self, id);
        }

        // Collect subtree in depth-first order (children before parent).
        let mut removed = Vec::new();
        self.collect_subtree_bottom_up(id, &mut removed);

        // Detach from parent and fire UNPARENTED.
        if let Some(parent_id) = self.nodes.get(&id).and_then(|n| n.parent()) {
            if let Some(parent) = self.nodes.get_mut(&parent_id) {
                parent.remove_child_id(id);
            }
        }
        self.trace_record(id, TraceEventType::Notification, "UNPARENTED");
        if let Some(node) = self.nodes.get_mut(&id) {
            node.receive_notification(NOTIFICATION_UNPARENTED);
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

        // UNPARENTED fires when detached from old parent.
        self.trace_record(node_id, TraceEventType::Notification, "UNPARENTED");
        if let Some(node) = self.nodes.get_mut(&node_id) {
            node.receive_notification(NOTIFICATION_UNPARENTED);
        }

        // Attach to new parent.
        if let Some(new_parent) = self.nodes.get_mut(&new_parent_id) {
            new_parent.add_child_id(node_id);
        }
        if let Some(node) = self.nodes.get_mut(&node_id) {
            node.set_parent(Some(new_parent_id));
        }

        // PARENTED fires when attached to new parent.
        self.trace_record(node_id, TraceEventType::Notification, "PARENTED");
        if let Some(node) = self.nodes.get_mut(&node_id) {
            node.receive_notification(NOTIFICATION_PARENTED);
        }

        // MOVED_IN_PARENT fires after PARENTED during reparent.
        self.trace_record(node_id, TraceEventType::Notification, "MOVED_IN_PARENT");
        if let Some(node) = self.nodes.get_mut(&node_id) {
            node.receive_notification(NOTIFICATION_MOVED_IN_PARENT);
        }

        // CHILD_ORDER_CHANGED fires on the new parent.
        self.trace_record(
            new_parent_id,
            TraceEventType::Notification,
            "CHILD_ORDER_CHANGED",
        );
        if let Some(parent) = self.nodes.get_mut(&new_parent_id) {
            parent.receive_notification(NOTIFICATION_CHILD_ORDER_CHANGED);
        }

        Ok(())
    }

    // -- child ordering operations -------------------------------------------

    /// Moves a child to a new position within its parent's child list.
    ///
    /// Matches Godot's `Node.move_child(child, to_index)`. Dispatches:
    /// - `NOTIFICATION_MOVED_IN_PARENT` to the moved child
    /// - `NOTIFICATION_CHILD_ORDER_CHANGED` to the parent
    pub fn move_child(
        &mut self,
        parent_id: NodeId,
        child_id: NodeId,
        to_index: usize,
    ) -> EngineResult<()> {
        // Validate parent exists and child is actually a child of parent.
        let parent = self
            .nodes
            .get(&parent_id)
            .ok_or_else(|| EngineError::NotFound(format!("parent node {parent_id} not found")))?;
        let children = parent.children().to_vec();
        let from_index = children
            .iter()
            .position(|&c| c == child_id)
            .ok_or_else(|| {
                EngineError::InvalidOperation(format!(
                    "node {child_id} is not a child of {parent_id}"
                ))
            })?;

        if from_index == to_index {
            return Ok(());
        }

        let clamped = to_index.min(children.len() - 1);

        // Perform the move in the parent's child list.
        if let Some(parent) = self.nodes.get_mut(&parent_id) {
            let list = parent.children_mut();
            let id = list.remove(from_index);
            list.insert(clamped, id);
        }

        // Dispatch MOVED_IN_PARENT to the child.
        self.trace_record(child_id, TraceEventType::Notification, "MOVED_IN_PARENT");
        if let Some(node) = self.nodes.get_mut(&child_id) {
            node.receive_notification(NOTIFICATION_MOVED_IN_PARENT);
        }

        // Dispatch CHILD_ORDER_CHANGED to the parent.
        self.trace_record(
            parent_id,
            TraceEventType::Notification,
            "CHILD_ORDER_CHANGED",
        );
        if let Some(parent) = self.nodes.get_mut(&parent_id) {
            parent.receive_notification(NOTIFICATION_CHILD_ORDER_CHANGED);
        }

        Ok(())
    }

    /// Moves a node to be the last child of its parent (highest index).
    ///
    /// Matches Godot's `Node.raise()`. Equivalent to
    /// `move_child(parent, child, last_index)`.
    pub fn raise(&mut self, node_id: NodeId) -> EngineResult<()> {
        let parent_id = self
            .nodes
            .get(&node_id)
            .and_then(|n| n.parent())
            .ok_or_else(|| {
                EngineError::InvalidOperation(format!("node {node_id} has no parent"))
            })?;
        let child_count = self
            .nodes
            .get(&parent_id)
            .map(|p| p.children().len())
            .unwrap_or(0);
        if child_count == 0 {
            return Ok(());
        }
        self.move_child(parent_id, node_id, child_count - 1)
    }

    /// Moves a node to be the first child of its parent (index 0).
    ///
    /// Matches Godot's implicit "lower" pattern. Equivalent to
    /// `move_child(parent, child, 0)`.
    pub fn lower(&mut self, node_id: NodeId) -> EngineResult<()> {
        let parent_id = self
            .nodes
            .get(&node_id)
            .and_then(|n| n.parent())
            .ok_or_else(|| {
                EngineError::InvalidOperation(format!("node {node_id} has no parent"))
            })?;
        self.move_child(parent_id, node_id, 0)
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
    ///
    /// Supports `%UniqueName` syntax: if the **first** segment starts with
    /// `%`, it is resolved via [`get_node_by_unique_name`](Self::get_node_by_unique_name)
    /// within the scene-owner scope. Remaining segments (if any) are resolved
    /// relative to the unique-name result.
    pub fn get_node_relative(&self, from: NodeId, rel_path: &str) -> Option<NodeId> {
        if rel_path.is_empty() {
            return Some(from);
        }

        // Handle %UniqueName paths — first segment starts with '%'.
        if let Some(stripped) = rel_path.strip_prefix('%') {
            let parts: Vec<&str> = stripped.split('/').collect();
            let unique_node = self.get_node_by_unique_name(from, parts[0])?;
            if parts.len() == 1 {
                return Some(unique_node);
            }
            // Resolve remaining segments relative to the unique node.
            let rest = parts[1..].join("/");
            return self.get_node_relative(unique_node, &rest);
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

    /// Resolves a `%UniqueName` within the scene owner scope.
    ///
    /// In Godot, `get_node("%Foo")` searches the subtree owned by the same
    /// scene root for a node with `unique_name == true` whose name matches
    /// `"Foo"`. The search starts from the owner of `from` (or `from`
    /// itself if it has no owner, i.e. it is a scene root).
    pub fn get_node_by_unique_name(&self, from: NodeId, name: &str) -> Option<NodeId> {
        // Determine the owner scope root.
        let owner_id = self
            .nodes
            .get(&from)
            .and_then(|n| n.owner())
            .unwrap_or(from);

        // Breadth-first search through the owner's subtree.
        self.find_unique_in_subtree(owner_id, name)
    }

    /// Searches a subtree (depth-first) for a node with `unique_name == true`
    /// and the given name.
    fn find_unique_in_subtree(&self, root: NodeId, name: &str) -> Option<NodeId> {
        let mut stack = vec![root];
        while let Some(id) = stack.pop() {
            if let Some(node) = self.nodes.get(&id) {
                if node.is_unique_name() && node.name() == name {
                    return Some(id);
                }
                // Push children in reverse order so left-most is visited first.
                for &child_id in node.children().iter().rev() {
                    stack.push(child_id);
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

    // -- pause state --------------------------------------------------------

    /// Returns whether the scene tree is currently paused.
    pub fn paused(&self) -> bool {
        self.paused
    }

    /// Sets the paused state of the scene tree.
    pub fn set_paused(&mut self, paused: bool) {
        self.paused = paused;
    }

    // -- process ordering and filtering -------------------------------------

    /// Returns IDs of all nodes sorted by process priority (stable sort,
    /// lower priority first), preserving tree order for equal priorities.
    pub fn all_nodes_in_process_order(&self) -> Vec<NodeId> {
        let mut ids = self.all_nodes_in_tree_order();
        ids.sort_by_key(|id| self.nodes.get(id).map_or(0, |n| n.process_priority()));
        ids
    }

    /// Resolves the effective [`ProcessMode`] for a node by walking up the
    /// parent chain until a non-`Inherit` mode is found. If the root node
    /// has `Inherit`, it is treated as `Pausable`.
    pub fn effective_process_mode(&self, node_id: NodeId) -> crate::node::ProcessMode {
        use crate::node::ProcessMode;
        let mut current = Some(node_id);
        while let Some(id) = current {
            if let Some(node) = self.nodes.get(&id) {
                if node.process_mode() != ProcessMode::Inherit {
                    return node.process_mode();
                }
                current = node.parent();
            } else {
                break;
            }
        }
        // Walked to root (or beyond) and everything was Inherit → Pausable.
        ProcessMode::Pausable
    }

    /// Returns `true` if the node should be processed in the current frame,
    /// taking the tree's pause state and the node's effective process mode
    /// into account.
    pub fn should_process_node(&self, node_id: NodeId) -> bool {
        use crate::node::ProcessMode;
        match self.effective_process_mode(node_id) {
            ProcessMode::Disabled => false,
            ProcessMode::Always => true,
            ProcessMode::Pausable => !self.paused,
            ProcessMode::WhenPaused => self.paused,
            ProcessMode::Inherit => {
                // Should never reach here — effective_process_mode resolves it.
                !self.paused
            }
        }
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

    /// Emits a signal on the given node. For each connection:
    /// - If the connection has a callback closure, invoke it directly.
    /// - If the connection has no callback (e.g. from a `.tscn` `[connection]`),
    ///   look up the target node's script and call the named method on it.
    ///
    /// Returns the collected return values from callback connections.
    pub fn emit_signal(
        &mut self,
        source: NodeId,
        signal_name: &str,
        args: &[gdvariant::Variant],
    ) -> Vec<gdvariant::Variant> {
        self.trace_record(source, TraceEventType::SignalEmit, signal_name);

        // Collect connection info to avoid borrow conflicts.
        // Note: we snapshot before emit() so one-shot connections are included.
        let connections: Vec<(gdcore::id::ObjectId, String, bool)> = self
            .signal_stores
            .get(&source)
            .and_then(|store| store.get_signal(signal_name))
            .map(|signal| {
                signal
                    .connections()
                    .iter()
                    .map(|c| (c.target_id, c.method.clone(), c.has_callback()))
                    .collect()
            })
            .unwrap_or_default();

        // Fire callback-based connections and remove one-shot connections.
        let results = self
            .signal_stores
            .get_mut(&source)
            .map_or_else(Vec::new, |store| store.emit(signal_name, args));

        // For connections without callbacks, dispatch to target node scripts.
        for (target_id, method, has_callback) in connections {
            if !has_callback {
                let target_node_id = NodeId::from_object_id(target_id);
                if self.scripts.contains_key(&target_node_id) {
                    self.call_script_with_access(target_node_id, &method, args);
                }
            }
        }

        results
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

    /// Records a trace event for the given node, if tracing is enabled.
    pub fn trace_record(&mut self, node_id: NodeId, event_type: TraceEventType, detail: &str) {
        if self.event_trace.is_enabled() {
            let path = self
                .node_path(node_id)
                .unwrap_or_else(|| format!("<{node_id}>"));
            self.event_trace.record(TraceEvent {
                event_type,
                node_path: path,
                detail: detail.to_string(),
                frame: self.trace_frame,
            });
        }
    }

    // -- script callbacks ---------------------------------------------------

    /// Temporarily extracts a script, gives it scene-tree access via
    /// [`SceneTreeAccessor`], calls the named method, then re-inserts the
    /// script. This allows the running script to call back into the tree
    /// (e.g. `get_node`, `emit_signal`) without violating borrow rules.
    fn call_script_with_access(&mut self, node_id: NodeId, method: &str, args: &[Variant]) {
        if let Some(mut script) = self.scripts.remove(&node_id) {
            // Skip methods the script doesn't define, matching Godot which only
            // dispatches lifecycle callbacks to scripts that actually override them.
            if !script.has_method(method) {
                self.scripts.insert(node_id, script);
                return;
            }
            self.trace_record(node_id, TraceEventType::ScriptCall, method);

            let snapshot_clone = self.input_snapshot.clone();
            let accessor = if let Some(snapshot) = snapshot_clone {
                unsafe {
                    crate::scripting::SceneTreeAccessor::with_input(
                        self as *mut SceneTree,
                        snapshot,
                    )
                }
            } else {
                unsafe { crate::scripting::SceneTreeAccessor::new(self as *mut SceneTree) }
            };
            script.set_scene_access(Box::new(accessor), node_id.raw());
            let _ = script.call_method(method, args);
            script.clear_scene_access();

            self.trace_record(node_id, TraceEventType::ScriptReturn, method);

            // Sync script variables to node properties so they are visible
            // as first-class node properties (matching Godot behavior).
            // Collect current values first to avoid borrow conflicts.
            let prop_values: Vec<(String, Variant)> = script
                .list_properties()
                .into_iter()
                .filter_map(|info| script.get_property(&info.name).map(|v| (info.name, v)))
                .collect();
            if let Some(node) = self.nodes.get_mut(&node_id) {
                for (name, val) in prop_values {
                    node.set_property(&name, val);
                }
            }

            self.scripts.insert(node_id, script);
        }
    }

    /// Calls `_ready()` on the node's script, if present.
    pub fn process_script_ready(&mut self, node_id: NodeId) {
        self.call_script_with_access(node_id, "_ready", &[]);
    }

    /// Calls `_process(delta)` on the node's script, if present.
    pub fn process_script_process(&mut self, node_id: NodeId, delta: f64) {
        self.call_script_with_access(node_id, "_process", &[Variant::Float(delta)]);
    }

    /// Calls `_physics_process(delta)` on the node's script, if present.
    pub fn process_script_physics_process(&mut self, node_id: NodeId, delta: f64) {
        self.call_script_with_access(node_id, "_physics_process", &[Variant::Float(delta)]);
    }

    /// Calls `_enter_tree()` on the node's script, if present.
    pub fn process_script_enter_tree(&mut self, node_id: NodeId) {
        self.call_script_with_access(node_id, "_enter_tree", &[]);
    }

    /// Calls `_exit_tree()` on the node's script, if present.
    pub fn process_script_exit_tree(&mut self, node_id: NodeId) {
        self.call_script_with_access(node_id, "_exit_tree", &[]);
    }

    /// Calls `_process(delta)` on all nodes that have attached scripts.
    pub fn process_all_scripts_process(&mut self, delta: f64) {
        let ids: Vec<NodeId> = self
            .all_nodes_in_process_order()
            .into_iter()
            .filter(|id| self.should_process_node(*id))
            .filter(|id| self.scripts.contains_key(id))
            .collect();
        for id in ids {
            self.process_script_process(id, delta);
        }
    }

    /// Calls `_physics_process(delta)` on all nodes that have attached scripts.
    pub fn process_all_scripts_physics_process(&mut self, delta: f64) {
        let ids: Vec<NodeId> = self
            .all_nodes_in_process_order()
            .into_iter()
            .filter(|id| self.should_process_node(*id))
            .filter(|id| self.scripts.contains_key(id))
            .collect();
        for id in ids {
            self.process_script_physics_process(id, delta);
        }
    }

    // -- process dispatch ----------------------------------------------------

    /// Dispatches [`NOTIFICATION_INTERNAL_PHYSICS_PROCESS`](gdobject::NOTIFICATION_INTERNAL_PHYSICS_PROCESS)
    /// to every node in tree order.
    ///
    /// Called once per fixed-timestep physics tick, **before** user
    /// `NOTIFICATION_PHYSICS_PROCESS`. This matches Godot's per-frame
    /// ordering: internal physics -> user physics -> internal process -> user process.
    pub fn process_internal_physics_frame(&mut self) {
        let ids: Vec<NodeId> = self
            .all_nodes_in_process_order()
            .into_iter()
            .filter(|id| self.should_process_node(*id))
            .collect();
        for id in ids {
            self.trace_record(id, TraceEventType::Notification, "INTERNAL_PHYSICS_PROCESS");
            if let Some(node) = self.nodes.get_mut(&id) {
                node.receive_notification(gdobject::NOTIFICATION_INTERNAL_PHYSICS_PROCESS);
            }
        }
    }

    /// Dispatches [`NOTIFICATION_PHYSICS_PROCESS`](gdobject::NOTIFICATION_PHYSICS_PROCESS)
    /// to every node in tree order.
    ///
    /// Called once per fixed-timestep physics tick, after internal physics processing.
    pub fn process_physics_frame(&mut self) {
        let ids: Vec<NodeId> = self
            .all_nodes_in_process_order()
            .into_iter()
            .filter(|id| self.should_process_node(*id))
            .collect();
        for id in ids {
            self.trace_record(id, TraceEventType::Notification, "PHYSICS_PROCESS");
            if let Some(node) = self.nodes.get_mut(&id) {
                node.receive_notification(gdobject::NOTIFICATION_PHYSICS_PROCESS);
            }
        }
    }

    /// Dispatches [`NOTIFICATION_INTERNAL_PROCESS`](gdobject::NOTIFICATION_INTERNAL_PROCESS)
    /// to every node in tree order.
    ///
    /// Called once per visual frame, **before** user `NOTIFICATION_PROCESS`.
    pub fn process_internal_frame(&mut self) {
        let ids: Vec<NodeId> = self
            .all_nodes_in_process_order()
            .into_iter()
            .filter(|id| self.should_process_node(*id))
            .collect();
        for id in ids {
            self.trace_record(id, TraceEventType::Notification, "INTERNAL_PROCESS");
            if let Some(node) = self.nodes.get_mut(&id) {
                node.receive_notification(gdobject::NOTIFICATION_INTERNAL_PROCESS);
            }
        }
    }

    /// Dispatches [`NOTIFICATION_PROCESS`](gdobject::NOTIFICATION_PROCESS)
    /// to every node in tree order.
    ///
    /// Called once per visual frame, after internal process.
    pub fn process_frame(&mut self) {
        let ids: Vec<NodeId> = self
            .all_nodes_in_process_order()
            .into_iter()
            .filter(|id| self.should_process_node(*id))
            .collect();
        for id in ids {
            self.trace_record(id, TraceEventType::Notification, "PROCESS");
            if let Some(node) = self.nodes.get_mut(&id) {
                node.receive_notification(gdobject::NOTIFICATION_PROCESS);
            }
        }
    }

    /// Dispatches [`NOTIFICATION_PROCESS`](gdobject::NOTIFICATION_PROCESS) to
    /// every node in tree order, interleaving `_process(delta)` script calls
    /// immediately after each node's notification.
    ///
    /// This matches Godot's per-node dispatch: each node receives its PROCESS
    /// notification and has its `_process()` called before the next node is
    /// processed.
    pub fn process_frame_with_scripts(&mut self, delta: f64) {
        let ids: Vec<NodeId> = self
            .all_nodes_in_process_order()
            .into_iter()
            .filter(|id| self.should_process_node(*id))
            .collect();
        for id in ids {
            self.trace_record(id, TraceEventType::Notification, "PROCESS");
            if let Some(node) = self.nodes.get_mut(&id) {
                node.receive_notification(gdobject::NOTIFICATION_PROCESS);
            }
            if self.scripts.contains_key(&id) {
                self.process_script_process(id, delta);
            }
        }
    }

    /// Dispatches [`NOTIFICATION_PHYSICS_PROCESS`](gdobject::NOTIFICATION_PHYSICS_PROCESS) to
    /// every node in tree order, interleaving `_physics_process(delta)` script calls
    /// immediately after each node's notification.
    ///
    /// This matches Godot's per-node dispatch for physics processing.
    pub fn process_physics_frame_with_scripts(&mut self, delta: f64) {
        let ids: Vec<NodeId> = self
            .all_nodes_in_process_order()
            .into_iter()
            .filter(|id| self.should_process_node(*id))
            .collect();
        for id in ids {
            self.trace_record(id, TraceEventType::Notification, "PHYSICS_PROCESS");
            if let Some(node) = self.nodes.get_mut(&id) {
                node.receive_notification(gdobject::NOTIFICATION_PHYSICS_PROCESS);
            }
            if self.scripts.contains_key(&id) {
                self.process_script_physics_process(id, delta);
            }
        }
    }

    // -- collision detection -----------------------------------------------

    /// Runs simple distance-based collision detection on all nodes that
    /// have a `collision_radius` property, updating `_is_colliding`,
    /// `_colliding_with`, and `_off_screen` properties.
    ///
    /// See [`crate::collision`] for full details.
    pub fn process_collisions(&mut self) {
        crate::collision::process_collisions(self);
    }

    /// Same as [`process_collisions`](Self::process_collisions) but with
    /// configurable screen bounds for the off-screen check.
    pub fn process_collisions_with_bounds(&mut self, screen_w: f32, screen_h: f32) {
        crate::collision::process_collisions_with_bounds(self, screen_w, screen_h);
    }

    // -- deferred calls (stub) ----------------------------------------------

    /// Stub for Godot's `call_deferred()` mechanism.
    ///
    /// In Godot, deferred calls are queued during the current frame and
    /// executed at the end of the frame (after all process callbacks).
    /// This is a placeholder -- deferred call queuing and execution is not
    /// yet implemented.
    ///
    /// TODO: Implement a deferred call queue that collects callables during
    /// the frame and flushes them after the process phase in `MainLoop::step()`.
    pub fn call_deferred(&mut self, _node_id: NodeId, _method: &str, _args: &[Variant]) {
        // Stub: deferred calls are not yet queued or executed.
        // When implemented, this should push onto a Vec<DeferredCall> that
        // MainLoop::step() flushes after the process phase.
    }

    // -- runtime node creation/deletion -------------------------------------

    /// Creates a new node with the given class name and node name.
    ///
    /// The node is inserted into the arena but is **not** attached to any
    /// parent yet. Call [`add_child`](Self::add_child) to place it in the
    /// hierarchy.
    pub fn create_node(&mut self, class_name: &str, name: &str) -> NodeId {
        let node = Node::new(name, class_name);
        let id = node.id();
        self.nodes.insert(id, node);
        id
    }

    /// Marks a node for deferred deletion. The node (and its subtree) will
    /// be removed from the tree when [`process_deletions`](Self::process_deletions)
    /// is called at the end of the frame.
    ///
    /// This is safe to call during `_process` — iterators are not
    /// invalidated because the actual removal is deferred.
    pub fn queue_free(&mut self, node_id: NodeId) {
        if !self.pending_deletions.contains(&node_id) {
            self.pending_deletions.push(node_id);
        }
    }

    /// Removes all nodes that were marked with [`queue_free`](Self::queue_free)
    /// during this frame.
    ///
    /// Call this **after** all `_process` callbacks have run and before
    /// the next frame begins. Scripts attached to deleted nodes are also
    /// removed.
    pub fn process_deletions(&mut self) {
        let to_delete: Vec<NodeId> = self.pending_deletions.drain(..).collect();
        for id in to_delete {
            // Skip nodes that were already removed (e.g. child of an
            // earlier deletion in this batch).
            if !self.nodes.contains_key(&id) {
                continue;
            }

            // Fire EXIT_TREE lifecycle (bottom-up) while scripts are still
            // attached, matching Godot's queue_free() behavior.
            let should_exit = self
                .nodes
                .get(&id)
                .map(|n| n.is_inside_tree())
                .unwrap_or(false);
            if should_exit {
                LifecycleManager::exit_tree(self, id);
            }

            // Fire PREDELETE notification (bottom-up) after EXIT_TREE.
            let mut bottom_up = Vec::new();
            self.collect_subtree_bottom_up(id, &mut bottom_up);
            for &nid in &bottom_up {
                self.trace_record(nid, TraceEventType::Notification, "PREDELETE");
                if let Some(node) = self.nodes.get_mut(&nid) {
                    node.receive_notification(gdobject::NOTIFICATION_PREDELETE);
                }
            }

            // Now remove scripts and the node itself.
            // exit_tree already marked nodes as outside tree, so
            // remove_node will skip the redundant EXIT_TREE call.
            let mut subtree = Vec::new();
            self.collect_subtree_top_down(id, &mut subtree);
            for &nid in &subtree {
                self.scripts.remove(&nid);
            }
            let _ = self.remove_node(id);
        }
    }

    /// Returns the number of nodes currently pending deletion.
    pub fn pending_deletion_count(&self) -> usize {
        self.pending_deletions.len()
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
    fn add_child_to_live_parent_auto_enters_tree() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        tree.get_node_mut(root).unwrap().set_inside_tree(true);

        let child = Node::new("Child", "Node");
        let child_id = tree.add_child(root, child).unwrap();

        let child = tree.get_node(child_id).unwrap();
        assert!(child.is_inside_tree());
        assert!(child.is_ready());
        assert!(child
            .notification_log()
            .contains(&gdobject::NOTIFICATION_ENTER_TREE));
        assert!(child
            .notification_log()
            .contains(&gdobject::NOTIFICATION_READY));
    }

    #[test]
    fn remove_live_subtree_auto_exits_tree_before_removal() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        tree.get_node_mut(root).unwrap().set_inside_tree(true);

        let parent = Node::new("Parent", "Node");
        let parent_id = tree.add_child(root, parent).unwrap();
        let child = Node::new("Child", "Node");
        let child_id = tree.add_child(parent_id, child).unwrap();

        let removed = tree.remove_node(parent_id).unwrap();
        assert_eq!(removed, vec![child_id, parent_id]);
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
        assert!(tree
            .get_node(c_id)
            .unwrap()
            .notification_log()
            .contains(&gdobject::NOTIFICATION_MOVED_IN_PARENT));
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
        // CHILD_ORDER_CHANGED from add_child + PROCESS from process_frame
        assert_eq!(root_log.len(), 2);
        assert_eq!(root_log[0], gdobject::NOTIFICATION_CHILD_ORDER_CHANGED);
        assert_eq!(root_log[1], gdobject::NOTIFICATION_PROCESS);

        let child_log = tree.get_node(child_id).unwrap().notification_log();
        // PARENTED from add_child + PROCESS from process_frame
        assert_eq!(child_log.len(), 2);
        assert_eq!(child_log[0], gdobject::NOTIFICATION_PARENTED);
        assert_eq!(child_log[1], gdobject::NOTIFICATION_PROCESS);
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
        // PARENTED from add_child + PHYSICS_PROCESS from process_physics_frame
        assert_eq!(log.len(), 2);
        assert_eq!(log[0], gdobject::NOTIFICATION_PARENTED);
        assert_eq!(log[1], gdobject::NOTIFICATION_PHYSICS_PROCESS);
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

    // -- Runtime node creation / deletion tests ----------------------------

    mod runtime_node_tests {
        use super::*;

        #[test]
        fn create_node_adds_to_arena() {
            let mut tree = SceneTree::new();
            let id = tree.create_node("Node2D", "Bullet");
            assert!(tree.get_node(id).is_some());
            let node = tree.get_node(id).unwrap();
            assert_eq!(node.name(), "Bullet");
            assert_eq!(node.class_name(), "Node2D");
            // Not yet parented.
            assert!(node.parent().is_none());
        }

        #[test]
        fn take_node_removes_from_arena() {
            let mut tree = SceneTree::new();
            let id = tree.create_node("Node2D", "Temp");
            assert!(tree.get_node(id).is_some());

            let node = tree.take_node(id);
            assert!(node.is_some());
            assert!(tree.get_node(id).is_none());
        }

        #[test]
        fn create_then_add_child() {
            let mut tree = SceneTree::new();
            let root = tree.root_id();
            tree.get_node_mut(root).unwrap().set_inside_tree(true);

            let id = tree.create_node("Node2D", "Bullet");
            let node = tree.take_node(id).unwrap();
            let added_id = tree.add_child(root, node).unwrap();
            assert_eq!(id, added_id);

            let child = tree.get_node(id).unwrap();
            assert_eq!(child.parent(), Some(root));
            assert!(child.is_inside_tree());
        }

        #[test]
        fn queue_free_defers_deletion() {
            let mut tree = SceneTree::new();
            let root = tree.root_id();

            let n = Node::new("X", "Node");
            let nid = tree.add_child(root, n).unwrap();

            tree.queue_free(nid);
            // Still alive.
            assert!(tree.get_node(nid).is_some());
            assert_eq!(tree.pending_deletion_count(), 1);

            tree.process_deletions();
            assert!(tree.get_node(nid).is_none());
            assert_eq!(tree.node_count(), 1); // Only root.
        }

        #[test]
        fn queue_free_subtree() {
            let mut tree = SceneTree::new();
            let root = tree.root_id();

            let a = Node::new("A", "Node");
            let a_id = tree.add_child(root, a).unwrap();
            let b = Node::new("B", "Node");
            let b_id = tree.add_child(a_id, b).unwrap();

            tree.queue_free(a_id);
            tree.process_deletions();

            assert!(tree.get_node(a_id).is_none());
            assert!(tree.get_node(b_id).is_none());
            assert_eq!(tree.node_count(), 1);
        }

        #[test]
        fn queue_free_already_removed_child_is_safe() {
            let mut tree = SceneTree::new();
            let root = tree.root_id();

            let a = Node::new("A", "Node");
            let a_id = tree.add_child(root, a).unwrap();
            let b = Node::new("B", "Node");
            let b_id = tree.add_child(a_id, b).unwrap();

            // Queue both parent and child for deletion.
            tree.queue_free(b_id);
            tree.queue_free(a_id);
            // Should not panic — b_id is already gone when a_id is removed.
            tree.process_deletions();

            assert!(tree.get_node(a_id).is_none());
            assert!(tree.get_node(b_id).is_none());
        }

        #[test]
        fn process_deletions_clears_pending_list() {
            let mut tree = SceneTree::new();
            let root = tree.root_id();

            let n = Node::new("N", "Node");
            let nid = tree.add_child(root, n).unwrap();

            tree.queue_free(nid);
            tree.process_deletions();
            assert_eq!(tree.pending_deletion_count(), 0);

            // Calling again is safe and does nothing.
            tree.process_deletions();
            assert_eq!(tree.pending_deletion_count(), 0);
        }
    }

    // ── B006: EventTrace integration tests ──────────────────────────────

    mod trace_tests {
        use super::*;
        use crate::lifecycle::LifecycleManager;
        use crate::trace::TraceEventType;

        /// Helper: build a tree with root -> Parent -> (Child1, Child2) and
        /// enable tracing.
        fn build_traced_tree() -> (SceneTree, NodeId, NodeId, NodeId, NodeId) {
            let mut tree = SceneTree::new();
            tree.event_trace_mut().enable();
            let root = tree.root_id();
            let parent = Node::new("Parent", "Node");
            let parent_id = tree.add_child(root, parent).unwrap();
            let c1 = Node::new("Child1", "Node");
            let c1_id = tree.add_child(parent_id, c1).unwrap();
            let c2 = Node::new("Child2", "Node");
            let c2_id = tree.add_child(parent_id, c2).unwrap();
            // Clear trace events from add_child (if any triggered by auto-enter)
            tree.event_trace_mut().clear();
            (tree, root, parent_id, c1_id, c2_id)
        }

        #[test]
        fn trace_enter_tree_top_down_order() {
            let (mut tree, _root, parent_id, _c1_id, _c2_id) = build_traced_tree();
            LifecycleManager::enter_tree(&mut tree, parent_id);

            let events = tree.event_trace().events();
            let enter_events: Vec<_> = events
                .iter()
                .filter(|e| {
                    e.event_type == TraceEventType::Notification && e.detail == "ENTER_TREE"
                })
                .collect();

            // ENTER_TREE should fire parent first, then children (top-down).
            assert_eq!(enter_events.len(), 3);
            assert!(enter_events[0].node_path.ends_with("Parent"));
            assert!(enter_events[1].node_path.ends_with("Child1"));
            assert!(enter_events[2].node_path.ends_with("Child2"));
        }

        #[test]
        fn trace_ready_bottom_up_order() {
            let (mut tree, _root, parent_id, _c1_id, _c2_id) = build_traced_tree();
            LifecycleManager::enter_tree(&mut tree, parent_id);

            let events = tree.event_trace().events();
            let ready_events: Vec<_> = events
                .iter()
                .filter(|e| e.event_type == TraceEventType::Notification && e.detail == "READY")
                .collect();

            // READY should fire children first, then parent (bottom-up).
            assert_eq!(ready_events.len(), 3);
            assert!(ready_events[0].node_path.ends_with("Child1"));
            assert!(ready_events[1].node_path.ends_with("Child2"));
            assert!(ready_events[2].node_path.ends_with("Parent"));
        }

        #[test]
        fn trace_exit_tree_bottom_up_order() {
            let (mut tree, _root, parent_id, _c1_id, _c2_id) = build_traced_tree();
            LifecycleManager::enter_tree(&mut tree, parent_id);
            tree.event_trace_mut().clear();

            LifecycleManager::exit_tree(&mut tree, parent_id);

            let events = tree.event_trace().events();
            let exit_events: Vec<_> = events
                .iter()
                .filter(|e| e.event_type == TraceEventType::Notification && e.detail == "EXIT_TREE")
                .collect();

            // EXIT_TREE fires bottom-up: children first, then parent.
            assert_eq!(exit_events.len(), 3);
            assert!(exit_events[0].node_path.ends_with("Child1"));
            assert!(exit_events[1].node_path.ends_with("Child2"));
            assert!(exit_events[2].node_path.ends_with("Parent"));
        }

        #[test]
        fn trace_enter_then_ready_interleaving() {
            let (mut tree, _root, parent_id, _c1_id, _c2_id) = build_traced_tree();
            LifecycleManager::enter_tree(&mut tree, parent_id);

            let events = tree.event_trace().events();
            let notif_events: Vec<_> = events
                .iter()
                .filter(|e| e.event_type == TraceEventType::Notification)
                .collect();

            // Godot contract: ALL ENTER_TREE first (top-down), then ALL READY (bottom-up).
            // Find boundary: last ENTER_TREE should come before first READY.
            let last_enter = notif_events
                .iter()
                .rposition(|e| e.detail == "ENTER_TREE")
                .unwrap();
            let first_ready = notif_events
                .iter()
                .position(|e| e.detail == "READY")
                .unwrap();
            assert!(
                last_enter < first_ready,
                "All ENTER_TREE must fire before any READY"
            );
        }

        #[test]
        fn trace_process_notification_order() {
            let (mut tree, _root, parent_id, _c1_id, _c2_id) = build_traced_tree();
            LifecycleManager::enter_tree(&mut tree, parent_id);
            tree.event_trace_mut().clear();

            // Run a frame's worth of notification dispatch.
            tree.process_internal_physics_frame();
            tree.process_physics_frame();
            tree.process_internal_frame();
            tree.process_frame();

            let events = tree.event_trace().events();
            let details: Vec<&str> = events
                .iter()
                .filter(|e| e.event_type == TraceEventType::Notification)
                .map(|e| e.detail.as_str())
                .collect();

            // Per Godot contract: internal_physics -> physics -> internal_process -> process.
            // Each dispatches to all nodes in tree order, so we see the pattern repeated.
            // Find the first occurrence of each type.
            let first_int_phys = details
                .iter()
                .position(|d| *d == "INTERNAL_PHYSICS_PROCESS")
                .unwrap();
            let first_phys = details
                .iter()
                .position(|d| *d == "PHYSICS_PROCESS")
                .unwrap();
            let first_int_proc = details
                .iter()
                .position(|d| *d == "INTERNAL_PROCESS")
                .unwrap();
            let first_proc = details.iter().position(|d| *d == "PROCESS").unwrap();

            assert!(first_int_phys < first_phys);
            assert!(first_phys < first_int_proc);
            assert!(first_int_proc < first_proc);
        }

        #[test]
        fn trace_signal_emit_recorded() {
            let (mut tree, _root, parent_id, _c1_id, _c2_id) = build_traced_tree();

            tree.emit_signal(parent_id, "test_signal", &[]);

            let events = tree.event_trace().events();
            let signal_events: Vec<_> = events
                .iter()
                .filter(|e| e.event_type == TraceEventType::SignalEmit)
                .collect();

            assert_eq!(signal_events.len(), 1);
            assert_eq!(signal_events[0].detail, "test_signal");
            assert!(signal_events[0].node_path.ends_with("Parent"));
        }

        #[test]
        fn trace_disabled_records_nothing() {
            let mut tree = SceneTree::new();
            // Trace is disabled by default
            let root = tree.root_id();
            let child = Node::new("A", "Node");
            let child_id = tree.add_child(root, child).unwrap();

            tree.process_frame();
            tree.emit_signal(child_id, "foo", &[]);

            assert!(tree.event_trace().events().is_empty());
        }

        #[test]
        fn trace_frame_number_recorded() {
            let mut tree = SceneTree::new();
            let root = tree.root_id();
            let child = Node::new("A", "Node");
            let _child_id = tree.add_child(root, child).unwrap();

            // Enable tracing after add_child so PARENTED traces don't skew frame counts.
            tree.event_trace_mut().enable();
            tree.set_trace_frame(0);
            tree.process_frame();

            tree.set_trace_frame(1);
            tree.process_frame();

            let events = tree.event_trace().events();
            let frame0: Vec<_> = events.iter().filter(|e| e.frame == 0).collect();
            let frame1: Vec<_> = events.iter().filter(|e| e.frame == 1).collect();

            assert!(!frame0.is_empty());
            assert!(!frame1.is_empty());
            assert_eq!(frame0.len(), frame1.len());
        }

        #[test]
        fn trace_clear_resets() {
            let (mut tree, _root, parent_id, _c1_id, _c2_id) = build_traced_tree();
            LifecycleManager::enter_tree(&mut tree, parent_id);
            assert!(!tree.event_trace().events().is_empty());

            tree.event_trace_mut().clear();
            assert!(tree.event_trace().events().is_empty());
        }

        #[test]
        fn trace_full_lifecycle_global_order() {
            // Verify the complete lifecycle ordering matches Godot:
            // ENTER_TREE(Parent) -> ENTER_TREE(C1) -> ENTER_TREE(C2) ->
            // READY(C1) -> READY(C2) -> READY(Parent)
            let (mut tree, _root, parent_id, _c1_id, _c2_id) = build_traced_tree();
            LifecycleManager::enter_tree(&mut tree, parent_id);

            let events = tree.event_trace().events();
            let notif_events: Vec<_> = events
                .iter()
                .filter(|e| e.event_type == TraceEventType::Notification)
                .map(|e| format!("{}:{}", e.detail, e.node_path.rsplit('/').next().unwrap()))
                .collect();

            assert_eq!(
                notif_events,
                vec![
                    "ENTER_TREE:Parent",
                    "ENTER_TREE:Child1",
                    "ENTER_TREE:Child2",
                    "READY:Child1",
                    "READY:Child2",
                    "READY:Parent",
                ]
            );
        }

        #[test]
        fn trace_deep_tree_ordering() {
            // root -> A -> B -> C
            // ENTER_TREE should be A, B, C. READY should be C, B, A.
            let mut tree = SceneTree::new();
            tree.event_trace_mut().enable();
            let root = tree.root_id();
            let a = Node::new("A", "Node");
            let a_id = tree.add_child(root, a).unwrap();
            let b = Node::new("B", "Node");
            let b_id = tree.add_child(a_id, b).unwrap();
            let c = Node::new("C", "Node");
            let _c_id = tree.add_child(b_id, c).unwrap();
            tree.event_trace_mut().clear();

            LifecycleManager::enter_tree(&mut tree, a_id);

            let events = tree.event_trace().events();
            let names: Vec<_> = events
                .iter()
                .filter(|e| e.event_type == TraceEventType::Notification)
                .map(|e| format!("{}:{}", e.detail, e.node_path.rsplit('/').next().unwrap()))
                .collect();

            assert_eq!(
                names,
                vec![
                    "ENTER_TREE:A",
                    "ENTER_TREE:B",
                    "ENTER_TREE:C",
                    "READY:C",
                    "READY:B",
                    "READY:A",
                ]
            );
        }
    }
}
