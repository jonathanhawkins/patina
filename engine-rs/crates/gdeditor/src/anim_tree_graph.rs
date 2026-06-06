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
}

/// A directed link (blend-tree connection or state transition).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Link {
    /// Source node.
    pub from: NodeId,
    /// Destination node.
    pub to: NodeId,
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
        });
        id
    }

    /// The number of nodes.
    pub fn node_count(&self) -> usize {
        self.nodes.len()
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

    /// Creates a state transition. Returns false if either node is missing, the
    /// endpoints are equal, or the transition already exists.
    pub fn add_transition(&mut self, from: NodeId, to: NodeId) -> bool {
        self.add_link(from, to, true)
    }

    /// Removes a state transition. Returns whether it existed.
    pub fn remove_transition(&mut self, from: NodeId, to: NodeId) -> bool {
        remove_link(&mut self.transitions, from, to)
    }

    /// Whether a transition `from -> to` exists.
    pub fn has_transition(&self, from: NodeId, to: NodeId) -> bool {
        self.transitions.iter().any(|l| l.from == from && l.to == to)
    }

    /// The state transitions.
    pub fn transitions(&self) -> &[Link] {
        &self.transitions
    }

    fn add_link(&mut self, from: NodeId, to: NodeId, transition: bool) -> bool {
        if from == to
            || self.get_node(from).is_none()
            || self.get_node(to).is_none()
        {
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
        list.push(Link { from, to });
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
}
