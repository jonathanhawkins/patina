//! **AnimationTree graph editor** (pat-la1lc).
//!
//! Models the AnimationTree's editable graph: nodes (state machine, blend
//! space, blend nodes, animations) that can be added, parameterized, and
//! connected in the blend tree, plus transitions between states in a state
//! machine. Removing a node also drops the connections and transitions that
//! touch it.

use std::collections::BTreeMap;

/// The kind of an AnimationTree node.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeKind {
    /// A state-machine node.
    StateMachine,
    /// A blend-space node.
    BlendSpace,
    /// A two-input blend node.
    Blend2,
    /// An animation leaf node.
    Animation,
}

/// A stable node id.
pub type NodeId = u64;

/// A node in the AnimationTree graph.
#[derive(Debug, Clone, PartialEq)]
pub struct Node {
    /// Stable id.
    pub id: NodeId,
    /// Display name.
    pub name: String,
    /// Node kind.
    pub kind: NodeKind,
    /// Editable parameters.
    pub params: BTreeMap<String, String>,
    /// Graph-canvas position in editor pixels (pat-0q2kx.166).
    ///
    /// Typed rather than stuffed into [`params`](Self::params) because the canvas
    /// reads these every frame and a stringly-typed coordinate would have to be
    /// re-parsed each time — and a malformed one would fail silently. Godot keeps
    /// the equivalent on the `AnimationNodeStateMachine` resource.
    pub x: f64,
    /// Graph-canvas Y position in editor pixels.
    pub y: f64,
}

/// A directed link (blend-tree connection or state transition).
///
/// `Eq` is deliberately **not** derived: `xfade` is an `f64`, and no container
/// here needs `Link` as a hash/BTree key — every use is a `Vec<Link>` or a
/// `&[Link]`. `PartialEq` is kept for assertions.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Link {
    /// Source node.
    pub from: NodeId,
    /// Destination node.
    pub to: NodeId,
    /// Crossfade time in seconds for a state transition — Godot's
    /// `AnimationNodeStateMachineTransition.xfade_time`, spelled `xfade_time` in
    /// a `.tscn` (pat-0q2kx.166). Always `0.0` for a blend-tree connection, which
    /// has no such concept, and `0.0` is also Godot's transition default.
    pub xfade: f64,
}

/// An editable AnimationTree graph.
#[derive(Debug, Clone, Default)]
pub struct AnimTreeGraph {
    nodes: Vec<Node>,
    connections: Vec<Link>,
    transitions: Vec<Link>,
    next_id: NodeId,
}

impl AnimTreeGraph {
    /// Creates an empty graph.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a node, returning its id.
    pub fn add_node(&mut self, name: impl Into<String>, kind: NodeKind) -> NodeId {
        let id = self.next_id;
        self.next_id += 1;
        self.nodes.push(Node {
            id,
            name: name.into(),
            kind,
            params: BTreeMap::new(),
            // Origin until the canvas moves it; Godot likewise starts a new state
            // at the drop point rather than a computed layout.
            x: 0.0,
            y: 0.0,
        });
        id
    }

    /// Moves a node on the graph canvas. Returns whether the node existed —
    /// a missing id is a 404, not a silent no-op (pat-0q2kx.166).
    pub fn move_node(&mut self, id: NodeId, x: f64, y: f64) -> bool {
        match self.nodes.iter_mut().find(|n| n.id == id) {
            Some(n) => {
                n.x = x;
                n.y = y;
                true
            }
            None => false,
        }
    }

    /// The number of nodes.
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// All nodes, in insertion order (the order the editor graph lists them).
    pub fn nodes(&self) -> &[Node] {
        &self.nodes
    }

    /// Finds a node id by its display name (the editor addresses states by
    /// name, since that is what the served graph shows).
    pub fn node_id_by_name(&self, name: &str) -> Option<NodeId> {
        self.nodes.iter().find(|n| n.name == name).map(|n| n.id)
    }

    /// A node by id.
    pub fn get_node(&self, id: NodeId) -> Option<&Node> {
        self.nodes.iter().find(|n| n.id == id)
    }

    /// Removes a node and any connections/transitions touching it. Returns
    /// whether it existed.
    pub fn remove_node(&mut self, id: NodeId) -> bool {
        let before = self.nodes.len();
        self.nodes.retain(|n| n.id != id);
        if self.nodes.len() == before {
            return false;
        }
        self.connections.retain(|l| l.from != id && l.to != id);
        self.transitions.retain(|l| l.from != id && l.to != id);
        true
    }

    /// Sets a parameter on a node. Returns whether the node exists.
    pub fn set_param(
        &mut self,
        id: NodeId,
        key: impl Into<String>,
        value: impl Into<String>,
    ) -> bool {
        match self.nodes.iter_mut().find(|n| n.id == id) {
            Some(n) => {
                n.params.insert(key.into(), value.into());
                true
            }
            None => false,
        }
    }

    /// Reads a node parameter.
    pub fn param(&self, id: NodeId, key: &str) -> Option<&str> {
        self.get_node(id)?.params.get(key).map(String::as_str)
    }

    /// Connects two nodes in the blend tree. Returns false if either node is
    /// missing, the endpoints are equal, or the connection already exists.
    pub fn connect(&mut self, from: NodeId, to: NodeId) -> bool {
        self.add_link(from, to, false)
    }

    /// Removes a blend-tree connection. Returns whether it existed.
    pub fn disconnect(&mut self, from: NodeId, to: NodeId) -> bool {
        remove_link(&mut self.connections, from, to)
    }

    /// The blend-tree connections.
    pub fn connections(&self) -> &[Link] {
        &self.connections
    }

    /// Creates a state transition with Godot's default crossfade (`0.0`).
    /// Returns false if either node is missing, the endpoints are equal, or the
    /// transition already exists.
    pub fn add_transition(&mut self, from: NodeId, to: NodeId) -> bool {
        self.add_transition_with_xfade(from, to, 0.0)
    }

    /// Creates a state transition carrying an explicit crossfade time in seconds
    /// (pat-0q2kx.166). The 2-argument [`add_transition`] is kept as the default
    /// case so existing callers need no change.
    pub fn add_transition_with_xfade(&mut self, from: NodeId, to: NodeId, xfade: f64) -> bool {
        self.add_link_with_xfade(from, to, true, xfade)
    }

    /// Updates an existing transition's crossfade time. Returns whether the
    /// transition existed — the caller reports a 404 rather than silently
    /// creating one, so a typo'd endpoint pair cannot masquerade as success.
    pub fn set_transition_xfade(&mut self, from: NodeId, to: NodeId, xfade: f64) -> bool {
        match self
            .transitions
            .iter_mut()
            .find(|l| l.from == from && l.to == to)
        {
            Some(link) => {
                link.xfade = xfade;
                true
            }
            None => false,
        }
    }

    /// The crossfade time of a transition, if it exists.
    pub fn transition_xfade(&self, from: NodeId, to: NodeId) -> Option<f64> {
        self.transitions
            .iter()
            .find(|l| l.from == from && l.to == to)
            .map(|l| l.xfade)
    }

    /// Removes a state transition. Returns whether it existed.
    pub fn remove_transition(&mut self, from: NodeId, to: NodeId) -> bool {
        remove_link(&mut self.transitions, from, to)
    }

    /// Whether a transition `from -> to` exists.
    pub fn has_transition(&self, from: NodeId, to: NodeId) -> bool {
        self.transitions
            .iter()
            .any(|l| l.from == from && l.to == to)
    }

    /// The state transitions.
    pub fn transitions(&self) -> &[Link] {
        &self.transitions
    }

    fn add_link(&mut self, from: NodeId, to: NodeId, transition: bool) -> bool {
        self.add_link_with_xfade(from, to, transition, 0.0)
    }

    fn add_link_with_xfade(
        &mut self,
        from: NodeId,
        to: NodeId,
        transition: bool,
        xfade: f64,
    ) -> bool {
        if from == to || self.get_node(from).is_none() || self.get_node(to).is_none() {
            return false;
        }
        let list = if transition {
            &mut self.transitions
        } else {
            &mut self.connections
        };
        if list.iter().any(|l| l.from == from && l.to == to) {
            return false;
        }
        list.push(Link { from, to, xfade });
        true
    }
}

fn remove_link(list: &mut Vec<Link>, from: NodeId, to: NodeId) -> bool {
    let before = list.len();
    list.retain(|l| !(l.from == from && l.to == to));
    list.len() != before
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Acceptance (pat-la1lc): the AnimationTree graph lets nodes be added,
    /// connected, and parameterized, and transitions between states are created
    /// and removed.
    #[test]
    fn anim_tree_graph_edit() {
        let mut g = AnimTreeGraph::new();

        // Add a mix of node kinds.
        let _sm = g.add_node("StateMachine", NodeKind::StateMachine);
        let idle = g.add_node("idle", NodeKind::Animation);
        let run = g.add_node("run", NodeKind::Animation);
        let blend = g.add_node("blend", NodeKind::Blend2);
        assert_eq!(g.node_count(), 4);
        assert_eq!(g.get_node(blend).unwrap().kind, NodeKind::Blend2);

        // Parameterize a node.
        assert!(g.set_param(blend, "blend_amount", "0.5"));
        assert_eq!(g.param(blend, "blend_amount"), Some("0.5"));
        assert!(!g.set_param(999, "x", "y")); // missing node

        // Connect nodes in the blend tree.
        assert!(g.connect(idle, blend));
        assert!(g.connect(run, blend));
        assert_eq!(g.connections().len(), 2);
        // Duplicate / self / unknown connections are rejected.
        assert!(!g.connect(idle, blend));
        assert!(!g.connect(blend, blend));
        assert!(!g.connect(idle, 999));
        // Disconnect one.
        assert!(g.disconnect(run, blend));
        assert_eq!(g.connections().len(), 1);

        // Create transitions between states.
        assert!(g.add_transition(idle, run));
        assert!(g.add_transition(run, idle));
        assert_eq!(g.transitions().len(), 2);
        assert!(g.has_transition(idle, run));
        assert!(!g.add_transition(idle, run)); // duplicate
                                               // Remove one.
        assert!(g.remove_transition(run, idle));
        assert_eq!(g.transitions().len(), 1);
        assert!(!g.remove_transition(run, idle)); // already gone

        // Removing a node drops its connections and transitions.
        assert!(g.remove_node(idle));
        assert!(g.get_node(idle).is_none());
        assert_eq!(g.node_count(), 3);
        assert_eq!(g.connections().len(), 0); // idle -> blend removed
        assert_eq!(g.transitions().len(), 0); // idle -> run removed
        assert!(!g.remove_node(idle)); // already gone
    }

    // -- transition crossfade (pat-0q2kx.166) -------------------------------
    //
    // `.166`'s acceptance criterion requires a transition to carry an xfade of
    // 0.3 and for the served graph to report it. Before this, `Link` had no
    // crossfade field at all and `xfade` appeared nowhere in `editor_server.rs`,
    // so the criterion could not be met however the UI was wired.

    #[test]
    fn transition_carries_an_explicit_crossfade() {
        let mut g = AnimTreeGraph::new();
        let idle = g.add_node("Idle", NodeKind::Animation);
        let run = g.add_node("Run", NodeKind::Animation);

        assert!(g.add_transition_with_xfade(idle, run, 0.3));
        assert_eq!(g.transition_xfade(idle, run), Some(0.3));
        assert_eq!(
            g.transitions()[0].xfade,
            0.3,
            "the stored Link must carry the crossfade the caller asked for"
        );
    }

    /// The 2-argument form must keep Godot's default rather than inventing one:
    /// `xfade_time` defaults to `0.0`, i.e. an instant switch.
    #[test]
    fn plain_add_transition_defaults_to_godot_zero_crossfade() {
        let mut g = AnimTreeGraph::new();
        let a = g.add_node("A", NodeKind::Animation);
        let b = g.add_node("B", NodeKind::Animation);

        assert!(g.add_transition(a, b));
        assert_eq!(g.transition_xfade(a, b), Some(0.0));
    }

    /// A blend-tree connection has no crossfade concept, so it must stay `0.0`
    /// rather than inherit whatever a transition happened to set.
    #[test]
    fn blend_tree_connection_has_no_crossfade() {
        let mut g = AnimTreeGraph::new();
        let a = g.add_node("A", NodeKind::Animation);
        let b = g.add_node("Blend", NodeKind::Blend2);

        assert!(g.connect(a, b));
        assert_eq!(g.connections()[0].xfade, 0.0);
    }

    /// Canvas positions must persist per node and start at the origin, which is
    /// what `state/move` + the `x`/`y` fields in the served graph exist for.
    #[test]
    fn nodes_carry_a_persisted_canvas_position() {
        let mut g = AnimTreeGraph::new();
        let idle = g.add_node("Idle", NodeKind::Animation);
        let run = g.add_node("Run", NodeKind::Animation);

        // A fresh node sits at the origin rather than an unset/NaN position.
        assert_eq!(
            (g.get_node(idle).unwrap().x, g.get_node(idle).unwrap().y),
            (0.0, 0.0)
        );

        assert!(g.move_node(idle, 120.0, 40.0));
        let n = g.get_node(idle).unwrap();
        assert_eq!((n.x, n.y), (120.0, 40.0));

        // Moving one node must not disturb another.
        let other = g.get_node(run).unwrap();
        assert_eq!((other.x, other.y), (0.0, 0.0));
    }

    /// Moving a node that does not exist must fail rather than no-op silently, so
    /// the route can answer 404 instead of a misleading success.
    #[test]
    fn moving_a_missing_node_fails() {
        let mut g = AnimTreeGraph::new();
        let a = g.add_node("A", NodeKind::Animation);
        assert!(!g.move_node(a + 999, 5.0, 5.0));
    }

    /// Editing an absent transition must fail rather than silently create one —
    /// otherwise a typo'd endpoint pair would report success and quietly add a
    /// transition the user never asked for.
    #[test]
    fn setting_crossfade_on_a_missing_transition_fails() {
        let mut g = AnimTreeGraph::new();
        let a = g.add_node("A", NodeKind::Animation);
        let b = g.add_node("B", NodeKind::Animation);

        assert!(!g.set_transition_xfade(a, b, 0.5), "no such transition");
        assert_eq!(g.transitions().len(), 0, "and none may be created");
        assert_eq!(g.transition_xfade(a, b), None);

        // Once it exists, the edit lands.
        assert!(g.add_transition(a, b));
        assert!(g.set_transition_xfade(a, b, 0.5));
        assert_eq!(g.transition_xfade(a, b), Some(0.5));
    }
}
