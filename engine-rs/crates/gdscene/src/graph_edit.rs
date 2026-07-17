//! GraphEdit family — Godot's node-graph GUI (pat-nzgdu.109).
//!
//! Ports Godot's `GraphElement` → `GraphNode` / `GraphFrame` hierarchy and the
//! `GraphEdit` container that owns the connection list.
//!
//! The connection model mirrors `scene/gui/graph_edit.cpp`:
//!
//! - A [`GraphNode`] exposes numbered **slots**; each slot may enable a left
//!   (input) side and/or a right (output) side, each with its own port *type*.
//! - **Port indices are not slot indices.** Input port `N` is the `N`-th slot
//!   with an enabled left side, in ascending slot order (likewise outputs on
//!   the right). Godot indexes connections by port, so a disabled slot simply
//!   does not consume a port number.
//! - Two ports may connect when their types match, or when the ordered pair was
//!   whitelisted via [`GraphEdit::add_valid_connection_type`] — Godot's rule for
//!   "allow the connection between two *different* port types".
//!
//! Drawing and mouse interaction are deliberately not implemented here: this is
//! the data model that the render and input layers consume. See the module
//! tests for the behaviour this guarantees.

use std::collections::{HashMap, HashSet};

use gdcore::math::{Color, Vector2};

/// Why a [`GraphEdit`] connection operation failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GraphEditError {
    /// No node is registered under this name.
    UnknownNode(String),
    /// The node has no enabled port with this index on the requested side.
    InvalidPort {
        /// Name of the node queried.
        node: String,
        /// The port index that does not exist.
        port: u32,
    },
    /// The port types are not compatible and were not whitelisted.
    IncompatibleTypes {
        /// Type of the originating (output) port.
        from_type: i32,
        /// Type of the destination (input) port.
        to_type: i32,
    },
    /// This exact connection already exists.
    AlreadyConnected,
    /// The connection to disconnect does not exist.
    NotConnected,
}

// ---------------------------------------------------------------------------
// GraphElement
// ---------------------------------------------------------------------------

/// Common state of anything placed on a [`GraphEdit`] (Godot `GraphElement`).
#[derive(Debug, Clone, PartialEq)]
pub struct GraphElement {
    /// Position within the graph, before zoom/scroll (`position_offset`).
    pub position_offset: Vector2,
    /// Size of the element in graph space.
    pub size: Vector2,
    /// Whether the user may resize it.
    pub resizable: bool,
    /// Whether the user may drag it.
    pub draggable: bool,
    /// Whether the user may select it.
    pub selectable: bool,
    /// Current selection state.
    pub selected: bool,
}

impl Default for GraphElement {
    fn default() -> Self {
        Self {
            position_offset: Vector2::ZERO,
            size: Vector2::new(100.0, 100.0),
            resizable: false,
            draggable: true,
            selectable: true,
            selected: false,
        }
    }
}

// ---------------------------------------------------------------------------
// GraphNode
// ---------------------------------------------------------------------------

/// One numbered slot on a [`GraphNode`] (Godot `GraphNode::set_slot`).
#[derive(Debug, Clone, PartialEq)]
pub struct GraphNodeSlot {
    /// Whether the left (input) side is enabled.
    pub enable_left: bool,
    /// Port type of the left side.
    pub type_left: i32,
    /// Draw colour of the left port.
    pub color_left: Color,
    /// Whether the right (output) side is enabled.
    pub enable_right: bool,
    /// Port type of the right side.
    pub type_right: i32,
    /// Draw colour of the right port.
    pub color_right: Color,
}

impl Default for GraphNodeSlot {
    fn default() -> Self {
        Self {
            enable_left: false,
            type_left: 0,
            color_left: Color::WHITE,
            enable_right: false,
            type_right: 0,
            color_right: Color::WHITE,
        }
    }
}

/// A titled, slotted node on a [`GraphEdit`] (Godot `GraphNode`).
#[derive(Debug, Clone, PartialEq)]
pub struct GraphNode {
    /// Placement/selection state.
    pub element: GraphElement,
    /// Text shown in the title bar.
    pub title: String,
    slots: HashMap<u32, GraphNodeSlot>,
}

impl GraphNode {
    /// Creates a node with the given title and no slots.
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            element: GraphElement::default(),
            title: title.into(),
            slots: HashMap::new(),
        }
    }

    /// Configures slot `idx` (Godot `set_slot`).
    #[allow(clippy::too_many_arguments)]
    pub fn set_slot(
        &mut self,
        idx: u32,
        enable_left: bool,
        type_left: i32,
        color_left: Color,
        enable_right: bool,
        type_right: i32,
        color_right: Color,
    ) {
        self.slots.insert(
            idx,
            GraphNodeSlot {
                enable_left,
                type_left,
                color_left,
                enable_right,
                type_right,
                color_right,
            },
        );
    }

    /// Enables a simple input slot of `type_left` at `idx`.
    pub fn set_input_slot(&mut self, idx: u32, type_left: i32) {
        let slot = self.slots.entry(idx).or_default();
        slot.enable_left = true;
        slot.type_left = type_left;
    }

    /// Enables a simple output slot of `type_right` at `idx`.
    pub fn set_output_slot(&mut self, idx: u32, type_right: i32) {
        let slot = self.slots.entry(idx).or_default();
        slot.enable_right = true;
        slot.type_right = type_right;
    }

    /// Returns slot `idx`, if configured.
    pub fn get_slot(&self, idx: u32) -> Option<&GraphNodeSlot> {
        self.slots.get(&idx)
    }

    /// Removes every slot (Godot `clear_all_slots`).
    pub fn clear_all_slots(&mut self) {
        self.slots.clear();
    }

    /// Slot indices whose left side is enabled, in port order.
    fn input_slots(&self) -> Vec<u32> {
        let mut idx: Vec<u32> = self
            .slots
            .iter()
            .filter(|(_, s)| s.enable_left)
            .map(|(i, _)| *i)
            .collect();
        idx.sort_unstable();
        idx
    }

    /// Slot indices whose right side is enabled, in port order.
    fn output_slots(&self) -> Vec<u32> {
        let mut idx: Vec<u32> = self
            .slots
            .iter()
            .filter(|(_, s)| s.enable_right)
            .map(|(i, _)| *i)
            .collect();
        idx.sort_unstable();
        idx
    }

    /// Number of enabled input ports (Godot `get_input_port_count`).
    pub fn get_input_port_count(&self) -> usize {
        self.input_slots().len()
    }

    /// Number of enabled output ports (Godot `get_output_port_count`).
    pub fn get_output_port_count(&self) -> usize {
        self.output_slots().len()
    }

    /// Type of input port `port`, or `None` if there is no such port.
    pub fn get_input_port_type(&self, port: u32) -> Option<i32> {
        let slot = *self.input_slots().get(port as usize)?;
        Some(self.slots[&slot].type_left)
    }

    /// Type of output port `port`, or `None` if there is no such port.
    pub fn get_output_port_type(&self, port: u32) -> Option<i32> {
        let slot = *self.output_slots().get(port as usize)?;
        Some(self.slots[&slot].type_right)
    }

    /// Slot index backing input port `port` (Godot `get_input_port_slot`).
    pub fn get_input_port_slot(&self, port: u32) -> Option<u32> {
        self.input_slots().get(port as usize).copied()
    }

    /// Slot index backing output port `port` (Godot `get_output_port_slot`).
    pub fn get_output_port_slot(&self, port: u32) -> Option<u32> {
        self.output_slots().get(port as usize).copied()
    }
}

// ---------------------------------------------------------------------------
// GraphFrame
// ---------------------------------------------------------------------------

/// A grouping frame that other elements attach to (Godot `GraphFrame`).
#[derive(Debug, Clone, PartialEq)]
pub struct GraphFrame {
    /// Placement/selection state.
    pub element: GraphElement,
    /// Text shown in the frame's title bar.
    pub title: String,
    /// Tint applied when [`tint_color_enabled`](Self::tint_color_enabled).
    pub tint_color: Color,
    /// Whether the tint is drawn.
    pub tint_color_enabled: bool,
    /// Whether the frame shrinks to fit its attached elements.
    pub autoshrink_enabled: bool,
    attached: HashSet<String>,
}

impl GraphFrame {
    /// Creates an empty frame with the given title.
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            element: GraphElement::default(),
            title: title.into(),
            tint_color: Color::WHITE,
            tint_color_enabled: true,
            autoshrink_enabled: true,
            attached: HashSet::new(),
        }
    }

    /// Attaches a node (by name) to this frame.
    pub fn attach(&mut self, node: impl Into<String>) {
        self.attached.insert(node.into());
    }

    /// Detaches a node; returns `true` if it was attached.
    pub fn detach(&mut self, node: &str) -> bool {
        self.attached.remove(node)
    }

    /// Whether `node` is attached to this frame.
    pub fn is_attached(&self, node: &str) -> bool {
        self.attached.contains(node)
    }

    /// Number of attached elements.
    pub fn attached_count(&self) -> usize {
        self.attached.len()
    }
}

// ---------------------------------------------------------------------------
// GraphEdit
// ---------------------------------------------------------------------------

/// One edge in a [`GraphEdit`] (an entry of Godot's `get_connection_list`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphConnection {
    /// Name of the originating node.
    pub from_node: String,
    /// Output port index on `from_node`.
    pub from_port: u32,
    /// Name of the destination node.
    pub to_node: String,
    /// Input port index on `to_node`.
    pub to_port: u32,
}

/// The node-graph container that owns nodes and their connections
/// (Godot `GraphEdit`).
#[derive(Debug, Clone, Default)]
pub struct GraphEdit {
    nodes: HashMap<String, GraphNode>,
    node_order: Vec<String>,
    frames: HashMap<String, GraphFrame>,
    connections: Vec<GraphConnection>,
    valid_connection_types: HashSet<(i32, i32)>,
    /// Current zoom factor.
    pub zoom: f32,
    /// Whether the background grid is drawn.
    pub show_grid: bool,
    /// Whether the minimap is shown.
    pub minimap_enabled: bool,
    /// Whether right-clicking a connection disconnects it.
    pub right_disconnects: bool,
}

impl GraphEdit {
    /// Creates an empty graph with Godot's defaults.
    pub fn new() -> Self {
        Self {
            zoom: 1.0,
            show_grid: true,
            minimap_enabled: true,
            ..Default::default()
        }
    }

    // -- nodes ------------------------------------------------------------

    /// Adds (or replaces) a node under `name`.
    pub fn add_node(&mut self, name: impl Into<String>, node: GraphNode) {
        let name = name.into();
        if !self.nodes.contains_key(&name) {
            self.node_order.push(name.clone());
        }
        self.nodes.insert(name, node);
    }

    /// Removes a node and every connection touching it.
    ///
    /// Returns `true` if the node existed.
    pub fn remove_node(&mut self, name: &str) -> bool {
        if self.nodes.remove(name).is_none() {
            return false;
        }
        self.node_order.retain(|n| n != name);
        self.connections
            .retain(|c| c.from_node != name && c.to_node != name);
        true
    }

    /// Returns the node registered under `name`.
    pub fn get_node(&self, name: &str) -> Option<&GraphNode> {
        self.nodes.get(name)
    }

    /// Returns the node registered under `name`, mutably.
    pub fn get_node_mut(&mut self, name: &str) -> Option<&mut GraphNode> {
        self.nodes.get_mut(name)
    }

    /// Node names, in insertion order.
    pub fn node_names(&self) -> &[String] {
        &self.node_order
    }

    /// Adds (or replaces) a grouping frame under `name`.
    pub fn add_frame(&mut self, name: impl Into<String>, frame: GraphFrame) {
        self.frames.insert(name.into(), frame);
    }

    /// Returns the frame registered under `name`.
    pub fn get_frame(&self, name: &str) -> Option<&GraphFrame> {
        self.frames.get(name)
    }

    // -- connection types -------------------------------------------------

    /// Whitelists connecting an output of `from_type` to an input of `to_type`
    /// (Godot `add_valid_connection_type`). Equal types are always valid and
    /// need no whitelisting.
    pub fn add_valid_connection_type(&mut self, from_type: i32, to_type: i32) {
        self.valid_connection_types.insert((from_type, to_type));
    }

    /// Removes a whitelisted type pair (Godot `remove_valid_connection_type`).
    pub fn remove_valid_connection_type(&mut self, from_type: i32, to_type: i32) {
        self.valid_connection_types.remove(&(from_type, to_type));
    }

    /// Whether an output of `from_type` may drive an input of `to_type`.
    ///
    /// Matching types are always valid; differing types only when whitelisted.
    pub fn is_valid_connection_type(&self, from_type: i32, to_type: i32) -> bool {
        from_type == to_type || self.valid_connection_types.contains(&(from_type, to_type))
    }

    // -- connections ------------------------------------------------------

    /// Connects an output port to an input port (Godot `connect_node`).
    ///
    /// Rejects unknown nodes, ports that do not exist (or are disabled),
    /// incompatible port types, and exact duplicates.
    pub fn connect_node(
        &mut self,
        from_node: &str,
        from_port: u32,
        to_node: &str,
        to_port: u32,
    ) -> Result<(), GraphEditError> {
        let from = self
            .nodes
            .get(from_node)
            .ok_or_else(|| GraphEditError::UnknownNode(from_node.to_string()))?;
        let from_type =
            from.get_output_port_type(from_port)
                .ok_or_else(|| GraphEditError::InvalidPort {
                    node: from_node.to_string(),
                    port: from_port,
                })?;

        let to = self
            .nodes
            .get(to_node)
            .ok_or_else(|| GraphEditError::UnknownNode(to_node.to_string()))?;
        let to_type =
            to.get_input_port_type(to_port)
                .ok_or_else(|| GraphEditError::InvalidPort {
                    node: to_node.to_string(),
                    port: to_port,
                })?;

        if !self.is_valid_connection_type(from_type, to_type) {
            return Err(GraphEditError::IncompatibleTypes { from_type, to_type });
        }
        if self.is_node_connected(from_node, from_port, to_node, to_port) {
            return Err(GraphEditError::AlreadyConnected);
        }

        self.connections.push(GraphConnection {
            from_node: from_node.to_string(),
            from_port,
            to_node: to_node.to_string(),
            to_port,
        });
        Ok(())
    }

    /// Removes a connection (Godot `disconnect_node`).
    pub fn disconnect_node(
        &mut self,
        from_node: &str,
        from_port: u32,
        to_node: &str,
        to_port: u32,
    ) -> Result<(), GraphEditError> {
        let before = self.connections.len();
        self.connections.retain(|c| {
            !(c.from_node == from_node
                && c.from_port == from_port
                && c.to_node == to_node
                && c.to_port == to_port)
        });
        if self.connections.len() == before {
            return Err(GraphEditError::NotConnected);
        }
        Ok(())
    }

    /// Whether this exact connection exists (Godot `is_node_connected`).
    pub fn is_node_connected(
        &self,
        from_node: &str,
        from_port: u32,
        to_node: &str,
        to_port: u32,
    ) -> bool {
        self.connections.iter().any(|c| {
            c.from_node == from_node
                && c.from_port == from_port
                && c.to_node == to_node
                && c.to_port == to_port
        })
    }

    /// Every connection, in creation order (Godot `get_connection_list`).
    pub fn get_connection_list(&self) -> &[GraphConnection] {
        &self.connections
    }

    /// Removes every connection (Godot `clear_connections`).
    pub fn clear_connections(&mut self) {
        self.connections.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `out_node` has one output of `out_type`; `in_node` one input of `in_type`.
    fn two_node_graph(out_type: i32, in_type: i32) -> GraphEdit {
        let mut graph = GraphEdit::new();
        let mut a = GraphNode::new("A");
        a.set_output_slot(0, out_type);
        let mut b = GraphNode::new("B");
        b.set_input_slot(0, in_type);
        graph.add_node("a", a);
        graph.add_node("b", b);
        graph
    }

    #[test]
    fn connect_node_records_the_connection() {
        let mut graph = two_node_graph(0, 0);
        assert!(graph.get_connection_list().is_empty());

        graph
            .connect_node("a", 0, "b", 0)
            .expect("same types connect");

        let list = graph.get_connection_list();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].from_node, "a");
        assert_eq!(list[0].from_port, 0);
        assert_eq!(list[0].to_node, "b");
        assert_eq!(list[0].to_port, 0);
        assert!(graph.is_node_connected("a", 0, "b", 0));
    }

    #[test]
    fn mismatched_port_types_are_rejected() {
        let mut graph = two_node_graph(1, 2);
        assert_eq!(
            graph.connect_node("a", 0, "b", 0),
            Err(GraphEditError::IncompatibleTypes {
                from_type: 1,
                to_type: 2
            })
        );
        assert!(
            graph.get_connection_list().is_empty(),
            "a rejected connection must not be recorded"
        );
    }

    #[test]
    fn whitelisting_a_type_pair_allows_the_connection() {
        let mut graph = two_node_graph(1, 2);
        assert!(!graph.is_valid_connection_type(1, 2));

        graph.add_valid_connection_type(1, 2);
        assert!(graph.is_valid_connection_type(1, 2));
        graph
            .connect_node("a", 0, "b", 0)
            .expect("whitelisted pair");
        assert_eq!(graph.get_connection_list().len(), 1);

        // The whitelist is directional, as in Godot.
        assert!(!graph.is_valid_connection_type(2, 1));

        graph.remove_valid_connection_type(1, 2);
        assert!(!graph.is_valid_connection_type(1, 2));
    }

    #[test]
    fn equal_types_never_need_whitelisting() {
        let graph = GraphEdit::new();
        assert!(graph.is_valid_connection_type(7, 7));
        assert!(!graph.is_valid_connection_type(7, 8));
    }

    #[test]
    fn unknown_nodes_and_ports_are_rejected() {
        let mut graph = two_node_graph(0, 0);
        assert_eq!(
            graph.connect_node("nope", 0, "b", 0),
            Err(GraphEditError::UnknownNode("nope".to_string()))
        );
        assert_eq!(
            graph.connect_node("a", 0, "nope", 0),
            Err(GraphEditError::UnknownNode("nope".to_string()))
        );
        // Port 5 does not exist on "a" (it has a single output port).
        assert_eq!(
            graph.connect_node("a", 5, "b", 0),
            Err(GraphEditError::InvalidPort {
                node: "a".to_string(),
                port: 5
            })
        );
        // "a" has no *input* port, so it cannot be a destination.
        assert_eq!(
            graph.connect_node("a", 0, "a", 0),
            Err(GraphEditError::InvalidPort {
                node: "a".to_string(),
                port: 0
            })
        );
    }

    #[test]
    fn duplicate_connections_are_rejected() {
        let mut graph = two_node_graph(0, 0);
        graph.connect_node("a", 0, "b", 0).unwrap();
        assert_eq!(
            graph.connect_node("a", 0, "b", 0),
            Err(GraphEditError::AlreadyConnected)
        );
        assert_eq!(graph.get_connection_list().len(), 1);
    }

    #[test]
    fn disconnect_removes_only_the_named_edge() {
        let mut graph = two_node_graph(0, 0);
        graph.get_node_mut("b").unwrap().set_input_slot(1, 0);
        graph.connect_node("a", 0, "b", 0).unwrap();
        graph.connect_node("a", 0, "b", 1).unwrap();
        assert_eq!(graph.get_connection_list().len(), 2);

        graph.disconnect_node("a", 0, "b", 0).unwrap();
        assert_eq!(graph.get_connection_list().len(), 1);
        assert!(!graph.is_node_connected("a", 0, "b", 0));
        assert!(graph.is_node_connected("a", 0, "b", 1));

        assert_eq!(
            graph.disconnect_node("a", 0, "b", 0),
            Err(GraphEditError::NotConnected)
        );
    }

    #[test]
    fn removing_a_node_drops_its_connections() {
        let mut graph = two_node_graph(0, 0);
        graph.connect_node("a", 0, "b", 0).unwrap();
        assert!(graph.remove_node("b"));
        assert!(
            graph.get_connection_list().is_empty(),
            "connections to a removed node must not linger"
        );
        assert!(!graph.remove_node("b"), "removing twice reports false");
        assert_eq!(graph.node_names(), &["a".to_string()][..]);
    }

    #[test]
    fn clear_connections_empties_the_list() {
        let mut graph = two_node_graph(0, 0);
        graph.connect_node("a", 0, "b", 0).unwrap();
        graph.clear_connections();
        assert!(graph.get_connection_list().is_empty());
    }

    #[test]
    fn port_indices_skip_disabled_slots() {
        // Slots 0 and 2 carry outputs; slot 1 does not. Output port 1 is
        // therefore slot 2 — port indices are not slot indices.
        let mut node = GraphNode::new("N");
        node.set_output_slot(0, 10);
        node.set_input_slot(1, 99);
        node.set_output_slot(2, 20);

        assert_eq!(node.get_output_port_count(), 2);
        assert_eq!(node.get_input_port_count(), 1);
        assert_eq!(node.get_output_port_type(0), Some(10));
        assert_eq!(node.get_output_port_type(1), Some(20));
        assert_eq!(node.get_output_port_slot(1), Some(2));
        assert_eq!(node.get_output_port_type(2), None);
        assert_eq!(node.get_input_port_type(0), Some(99));
        assert_eq!(node.get_input_port_slot(0), Some(1));
    }

    #[test]
    fn set_slot_configures_both_sides() {
        let mut node = GraphNode::new("N");
        node.set_slot(0, true, 3, Color::WHITE, true, 4, Color::BLACK);
        let slot = node.get_slot(0).unwrap();
        assert!(slot.enable_left && slot.enable_right);
        assert_eq!(slot.type_left, 3);
        assert_eq!(slot.type_right, 4);
        assert_eq!(node.get_input_port_type(0), Some(3));
        assert_eq!(node.get_output_port_type(0), Some(4));

        node.clear_all_slots();
        assert_eq!(node.get_input_port_count(), 0);
        assert_eq!(node.get_output_port_count(), 0);
    }

    #[test]
    fn graph_frame_tracks_attached_elements() {
        let mut frame = GraphFrame::new("Group");
        assert_eq!(frame.attached_count(), 0);
        frame.attach("a");
        frame.attach("b");
        frame.attach("a"); // idempotent
        assert_eq!(frame.attached_count(), 2);
        assert!(frame.is_attached("a"));
        assert!(frame.detach("a"));
        assert!(!frame.is_attached("a"));
        assert!(!frame.detach("a"));
    }

    #[test]
    fn graph_edit_and_element_defaults_match_godot() {
        let graph = GraphEdit::new();
        assert_eq!(graph.zoom, 1.0);
        assert!(graph.show_grid);
        assert!(!graph.right_disconnects);

        let el = GraphElement::default();
        assert!(el.draggable && el.selectable);
        assert!(!el.resizable && !el.selected);
        assert_eq!(el.position_offset, Vector2::ZERO);
    }
}
