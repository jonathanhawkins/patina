//! Base Node type and node tree operations.
//!
//! A [`Node`] is the fundamental building block of the scene tree. Each node
//! has a name, a class, an identity, parent/child relationships, a property
//! bag, and group membership. Nodes are stored in an arena (the
//! [`SceneTree`](crate::scene_tree::SceneTree)) and referenced by
//! lightweight [`NodeId`] handles.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt;

use gdcore::ObjectId;
use gdobject::notification::Notification;
use gdvariant::Variant;

// ---------------------------------------------------------------------------
// NodeId
// ---------------------------------------------------------------------------

/// A lightweight identifier for a node within the scene tree.
///
/// Wraps an [`ObjectId`] so that every node can be uniquely referenced
/// without holding a borrow on the arena.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct NodeId(ObjectId);

impl NodeId {
    /// Creates a new, globally unique `NodeId`.
    pub fn next() -> Self {
        Self(ObjectId::next())
    }

    /// Wraps an existing [`ObjectId`] as a `NodeId`.
    pub fn from_object_id(id: ObjectId) -> Self {
        Self(id)
    }

    /// Returns the underlying [`ObjectId`].
    pub fn object_id(self) -> ObjectId {
        self.0
    }

    /// Returns the raw `u64` value.
    pub fn raw(self) -> u64 {
        self.0.raw()
    }
}

impl fmt::Debug for NodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "NodeId({})", self.0.raw())
    }
}

impl fmt::Display for NodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0.raw())
    }
}

// ---------------------------------------------------------------------------
// ProcessMode
// ---------------------------------------------------------------------------

/// Controls whether a node processes when the scene tree is paused.
///
/// Mirrors Godot's `Node.ProcessMode` enum. The default is [`Inherit`](ProcessMode::Inherit),
/// which resolves by walking up the parent chain. If the root node has `Inherit`,
/// it is treated as [`Pausable`](ProcessMode::Pausable).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ProcessMode {
    /// Use the parent's effective process mode. Root defaults to `Pausable`.
    #[default]
    Inherit,
    /// Processes only when the tree is **not** paused (default behavior).
    Pausable,
    /// Processes **only** when the tree **is** paused.
    WhenPaused,
    /// Always processes, regardless of pause state.
    Always,
    /// Never processes.
    Disabled,
}

// ---------------------------------------------------------------------------
// Node
// ---------------------------------------------------------------------------

/// A scene-tree node, analogous to Godot's `Node` class.
///
/// Nodes form a tree hierarchy managed by a [`SceneTree`](crate::scene_tree::SceneTree).
/// Each node stores its own name, class, parent/child links, a dynamic
/// property bag, group membership, and a notification log.
#[derive(Debug, Clone)]
pub struct Node {
    /// The unique ID of this node.
    id: NodeId,
    /// Human-readable name (e.g. `"Player"`).
    name: String,
    /// The Godot class name (e.g. `"Node2D"`, `"Sprite2D"`).
    class_name: String,
    /// Parent node, or `None` if this is the root / detached.
    parent: Option<NodeId>,
    /// Ordered list of child node IDs.
    children: Vec<NodeId>,
    /// Dynamic property storage (position, texture, etc.).
    properties: HashMap<String, Variant>,
    /// Groups this node belongs to.
    groups: HashSet<String>,
    /// Log of notifications received (for testing / debugging).
    notification_log: Vec<Notification>,
    /// Meta property storage (separate namespace from regular properties).
    meta: BTreeMap<String, Variant>,
    /// The node that "owns" this node in the scene hierarchy.
    /// Usually the scene root that this node was instanced from.
    owner: Option<NodeId>,
    /// Whether this node has a scene-unique name (the `%` prefix in Godot).
    unique_name: bool,
    /// Whether this node is currently inside the active scene tree.
    inside_tree: bool,
    /// Whether this node has completed its ready phase.
    ready: bool,
    /// Process priority — lower values process first. Default 0.
    process_priority: i32,
    /// Controls whether this node processes when the tree is paused.
    process_mode: ProcessMode,
}

impl Node {
    /// Creates a new detached node with the given name and class.
    pub fn new(name: impl Into<String>, class_name: impl Into<String>) -> Self {
        Self {
            id: NodeId::next(),
            name: name.into(),
            class_name: class_name.into(),
            parent: None,
            children: Vec::new(),
            properties: HashMap::new(),
            groups: HashSet::new(),
            notification_log: Vec::new(),
            meta: BTreeMap::new(),
            owner: None,
            unique_name: false,
            inside_tree: false,
            ready: false,
            process_priority: 0,
            process_mode: ProcessMode::default(),
        }
    }

    /// Creates a node with a specific [`NodeId`] (for deserialization / tests).
    pub fn with_id(id: NodeId, name: impl Into<String>, class_name: impl Into<String>) -> Self {
        Self {
            id,
            name: name.into(),
            class_name: class_name.into(),
            parent: None,
            children: Vec::new(),
            properties: HashMap::new(),
            groups: HashSet::new(),
            notification_log: Vec::new(),
            meta: BTreeMap::new(),
            owner: None,
            unique_name: false,
            inside_tree: false,
            ready: false,
            process_priority: 0,
            process_mode: ProcessMode::default(),
        }
    }

    // -- identity -----------------------------------------------------------

    /// Returns the node's unique ID.
    pub fn id(&self) -> NodeId {
        self.id
    }

    /// Returns the node's name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Sets the node's name.
    pub fn set_name(&mut self, name: impl Into<String>) {
        self.name = name.into();
    }

    /// Returns the Godot class name.
    pub fn class_name(&self) -> &str {
        &self.class_name
    }

    // -- hierarchy (low-level, used by SceneTree) ---------------------------

    /// Returns the parent ID, if any.
    pub fn parent(&self) -> Option<NodeId> {
        self.parent
    }

    /// Sets the parent (called by [`SceneTree`](crate::scene_tree::SceneTree)).
    pub(crate) fn set_parent(&mut self, parent: Option<NodeId>) {
        self.parent = parent;
    }

    /// Returns the ordered list of child IDs.
    pub fn children(&self) -> &[NodeId] {
        &self.children
    }

    /// Returns a mutable reference to the ordered list of child IDs.
    pub fn children_mut(&mut self) -> &mut Vec<NodeId> {
        &mut self.children
    }

    /// Appends a child ID (called by [`SceneTree`](crate::scene_tree::SceneTree)).
    pub(crate) fn add_child_id(&mut self, child: NodeId) {
        self.children.push(child);
    }

    /// Removes a child ID, returning `true` if it was present.
    pub(crate) fn remove_child_id(&mut self, child: NodeId) -> bool {
        if let Some(pos) = self.children.iter().position(|&c| c == child) {
            self.children.remove(pos);
            true
        } else {
            false
        }
    }

    // -- properties ---------------------------------------------------------

    /// Sets a property, returning the previous value (or `Nil`).
    pub fn set_property(&mut self, key: &str, value: Variant) -> Variant {
        self.properties
            .insert(key.to_owned(), value)
            .unwrap_or(Variant::Nil)
    }

    /// Gets a property by name, returning `Nil` if absent.
    pub fn get_property(&self, key: &str) -> Variant {
        self.properties.get(key).cloned().unwrap_or(Variant::Nil)
    }

    /// Returns `true` if the property exists.
    pub fn has_property(&self, key: &str) -> bool {
        self.properties.contains_key(key)
    }

    /// Returns an iterator over all properties.
    pub fn properties(&self) -> impl Iterator<Item = (&String, &Variant)> {
        self.properties.iter()
    }

    /// Removes a property by name. Returns the old value, or `Nil` if absent.
    pub fn remove_property(&mut self, key: &str) -> Variant {
        self.properties.remove(key).unwrap_or(Variant::Nil)
    }

    /// Returns a sorted list of property names (matches Godot's
    /// `get_property_list()` returning entries in deterministic order).
    pub fn get_property_list(&self) -> Vec<&str> {
        let mut names: Vec<&str> = self.properties.keys().map(String::as_str).collect();
        names.sort();
        names
    }

    // -- type checking ------------------------------------------------------

    /// Returns `true` if this node's class matches `name`, or if `name` is
    /// an ancestor in the ClassDB inheritance chain.
    ///
    /// Mirrors Godot's `Object.is_class()`.
    pub fn is_class(&self, name: &str) -> bool {
        if self.class_name == name {
            return true;
        }
        gdobject::is_parent_class(&self.class_name, name)
    }

    /// Checks whether the given method is registered for this node's class
    /// (or any ancestor) in the ClassDB.
    pub fn has_method(&self, method_name: &str) -> bool {
        gdobject::class_has_method(&self.class_name, method_name)
    }

    // -- meta properties ----------------------------------------------------

    /// Sets a meta property. Returns the previous value (or `Nil`).
    ///
    /// Meta properties are a separate namespace from regular properties,
    /// used by the editor, plugins, and runtime tagging.
    pub fn set_meta(&mut self, key: &str, value: Variant) -> Variant {
        self.meta
            .insert(key.to_owned(), value)
            .unwrap_or(Variant::Nil)
    }

    /// Gets a meta property by name. Returns `Nil` if absent.
    pub fn get_meta(&self, key: &str) -> Variant {
        self.meta.get(key).cloned().unwrap_or(Variant::Nil)
    }

    /// Returns `true` if the meta property exists.
    pub fn has_meta(&self, key: &str) -> bool {
        self.meta.contains_key(key)
    }

    /// Removes a meta property by name. Returns the old value (or `Nil`).
    pub fn remove_meta(&mut self, key: &str) -> Variant {
        self.meta.remove(key).unwrap_or(Variant::Nil)
    }

    /// Returns the names of all meta properties (sorted, matching Godot).
    pub fn get_meta_list(&self) -> Vec<&str> {
        self.meta.keys().map(String::as_str).collect()
    }

    // -- groups -------------------------------------------------------------

    /// Adds this node to a group.
    pub fn add_to_group(&mut self, group: impl Into<String>) {
        self.groups.insert(group.into());
    }

    /// Removes this node from a group. Returns `true` if it was a member.
    pub fn remove_from_group(&mut self, group: &str) -> bool {
        self.groups.remove(group)
    }

    /// Returns `true` if this node is in the given group.
    pub fn is_in_group(&self, group: &str) -> bool {
        self.groups.contains(group)
    }

    /// Returns all groups this node belongs to.
    pub fn groups(&self) -> &HashSet<String> {
        &self.groups
    }

    // -- owner --------------------------------------------------------------

    /// Returns the owner node ID, if set.
    pub fn owner(&self) -> Option<NodeId> {
        self.owner
    }

    /// Sets the owner node ID.
    pub fn set_owner(&mut self, owner: Option<NodeId>) {
        self.owner = owner;
    }

    // -- unique name --------------------------------------------------------

    /// Returns whether this node has a scene-unique name (`%` prefix).
    pub fn is_unique_name(&self) -> bool {
        self.unique_name
    }

    /// Sets the scene-unique name flag.
    pub fn set_unique_name(&mut self, unique: bool) {
        self.unique_name = unique;
    }

    /// Returns whether this node is currently inside the active scene tree.
    pub fn is_inside_tree(&self) -> bool {
        self.inside_tree
    }

    /// Marks whether this node is currently inside the active scene tree.
    pub(crate) fn set_inside_tree(&mut self, inside_tree: bool) {
        self.inside_tree = inside_tree;
    }

    /// Returns whether this node has completed its ready phase.
    pub fn is_ready(&self) -> bool {
        self.ready
    }

    /// Marks whether this node has completed its ready phase.
    pub(crate) fn set_ready(&mut self, ready: bool) {
        self.ready = ready;
    }

    // -- process priority / mode --------------------------------------------

    /// Returns this node's process priority. Lower values process first.
    pub fn process_priority(&self) -> i32 {
        self.process_priority
    }

    /// Sets this node's process priority. Lower values process first.
    pub fn set_process_priority(&mut self, priority: i32) {
        self.process_priority = priority;
    }

    /// Returns this node's process mode.
    pub fn process_mode(&self) -> ProcessMode {
        self.process_mode
    }

    /// Sets this node's process mode.
    pub fn set_process_mode(&mut self, mode: ProcessMode) {
        self.process_mode = mode;
    }

    // -- notifications ------------------------------------------------------

    /// Records a notification (called by lifecycle manager).
    pub fn receive_notification(&mut self, what: Notification) {
        self.notification_log.push(what);
    }

    /// Returns the notification log for testing / introspection.
    pub fn notification_log(&self) -> &[Notification] {
        &self.notification_log
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn node_creation() {
        let node = Node::new("Player", "Node2D");
        assert_eq!(node.name(), "Player");
        assert_eq!(node.class_name(), "Node2D");
        assert!(node.parent().is_none());
        assert!(node.children().is_empty());
    }

    #[test]
    fn node_unique_ids() {
        let a = Node::new("A", "Node");
        let b = Node::new("B", "Node");
        assert_ne!(a.id(), b.id());
    }

    #[test]
    fn node_properties() {
        let mut node = Node::new("N", "Node");
        assert_eq!(node.get_property("x"), Variant::Nil);

        node.set_property("x", Variant::Int(10));
        assert_eq!(node.get_property("x"), Variant::Int(10));
        assert!(node.has_property("x"));
    }

    #[test]
    fn node_groups() {
        let mut node = Node::new("N", "Node");
        node.add_to_group("enemies");
        assert!(node.is_in_group("enemies"));
        assert!(!node.is_in_group("players"));

        assert!(node.remove_from_group("enemies"));
        assert!(!node.is_in_group("enemies"));
    }

    #[test]
    fn child_id_management() {
        let mut parent = Node::new("Parent", "Node");
        let child_id = NodeId::next();
        parent.add_child_id(child_id);
        assert_eq!(parent.children().len(), 1);
        assert_eq!(parent.children()[0], child_id);

        assert!(parent.remove_child_id(child_id));
        assert!(parent.children().is_empty());
    }

    #[test]
    fn node_with_empty_name() {
        let node = Node::new("", "Node");
        assert_eq!(node.name(), "");
    }

    #[test]
    fn node_with_unicode_name() {
        let node = Node::new("プレイヤー🎮", "Node2D");
        assert_eq!(node.name(), "プレイヤー🎮");
        assert_eq!(node.class_name(), "Node2D");
    }

    #[test]
    fn node_set_name() {
        let mut node = Node::new("Old", "Node");
        node.set_name("New");
        assert_eq!(node.name(), "New");
    }

    #[test]
    fn node_id_display() {
        let id = NodeId::next();
        let display = format!("{id}");
        assert!(!display.is_empty());
    }

    #[test]
    fn node_id_debug() {
        let id = NodeId::next();
        let debug = format!("{id:?}");
        assert!(debug.starts_with("NodeId("));
    }

    #[test]
    fn node_with_id() {
        let id = NodeId::next();
        let node = Node::with_id(id, "Custom", "Sprite2D");
        assert_eq!(node.id(), id);
        assert_eq!(node.name(), "Custom");
    }

    #[test]
    fn remove_nonexistent_child_returns_false() {
        let mut node = Node::new("Parent", "Node");
        assert!(!node.remove_child_id(NodeId::next()));
    }

    #[test]
    fn notification_log_records_in_order() {
        let mut node = Node::new("N", "Node");
        node.receive_notification(gdobject::NOTIFICATION_ENTER_TREE);
        node.receive_notification(gdobject::NOTIFICATION_READY);
        node.receive_notification(gdobject::NOTIFICATION_EXIT_TREE);
        let log = node.notification_log();
        assert_eq!(log.len(), 3);
        assert_eq!(log[0], gdobject::NOTIFICATION_ENTER_TREE);
        assert_eq!(log[1], gdobject::NOTIFICATION_READY);
        assert_eq!(log[2], gdobject::NOTIFICATION_EXIT_TREE);
    }

    #[test]
    fn node_tree_state_flags_default_false() {
        let node = Node::new("N", "Node");
        assert!(!node.is_inside_tree());
        assert!(!node.is_ready());
    }

    #[test]
    fn group_add_twice_is_idempotent() {
        let mut node = Node::new("N", "Node");
        node.add_to_group("enemies");
        node.add_to_group("enemies");
        assert!(node.is_in_group("enemies"));
        assert_eq!(node.groups().len(), 1);
    }

    #[test]
    fn remove_from_nonexistent_group_returns_false() {
        let mut node = Node::new("N", "Node");
        assert!(!node.remove_from_group("nonexistent"));
    }

    #[test]
    fn node_id_from_object_id() {
        let oid = ObjectId::next();
        let nid = NodeId::from_object_id(oid);
        assert_eq!(nid.object_id(), oid);
        assert_eq!(nid.raw(), oid.raw());
    }
}
