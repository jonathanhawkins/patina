//! **AnimationTree graph editor** (pat-la1lc).
//!
//! Models the AnimationTree's editable graph: nodes (state machine, blend
//! space, blend nodes, animations) that can be added, parameterized, and
//! connected in the blend tree, plus transitions between states in a state
//! machine. Removing a node also drops the connections and transitions that
//! touch it.

use std::collections::BTreeMap;

use gdscene::animation_tree::{
    AnimationLeafOptions, AnimationNode, BlendSpace1D, BlendSpace2D, StateMachine, StateTransition,
};

/// The kind of an AnimationTree node.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeKind {
    /// A state-machine node.
    StateMachine,
    /// A 1D blend-space node (`AnimationNodeBlendSpace1D`).
    ///
    /// pat-s3rrv.1: 1D and 2D are SEPARATE classes in Godot, not one class with
    /// a dimension flag, and their blend points differ in shape — a 1D point's
    /// position is a scalar and a 2D point's is a Vector2. A single `BlendSpace`
    /// variant could not say which a node was, so the point editor could not
    /// know how many components to read.
    BlendSpace1D,
    /// A 2D blend-space node (`AnimationNodeBlendSpace2D`).
    BlendSpace2D,
    /// A two-input blend node.
    Blend2,
    /// An animation leaf node.
    Animation,
}

/// A stable node id.
pub type NodeId = u64;

/// Grid width used when laying out a machine loaded from a scene
/// (pat-s3rrv.1) — see [`AnimTreeGraph::from_state_machine`].
const LOAD_COLUMNS: usize = 4;
/// Horizontal spacing of that grid, in editor pixels.
const LOAD_COLUMN_WIDTH: f64 = 220.0;
/// Vertical spacing of that grid, in editor pixels.
const LOAD_ROW_HEIGHT: f64 = 140.0;

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

    /// Builds an editor graph from a runtime state machine (pat-s3rrv.1).
    ///
    /// The inverse of [`to_state_machine`](Self::to_state_machine), and the
    /// reason it matters: without it the editor opens a scene that already has
    /// an `AnimationTree` and shows an EMPTY graph. Applying that empty graph —
    /// now that `POST /api/animation/tree/apply` exists — would silently
    /// replace the real machine with nothing.
    ///
    /// ⚠️ States are laid out in NAME order, with the machine's `current` state
    /// first. The runtime keeps them in a `HashMap`, so iterating it directly
    /// would lay the graph out differently on every open; and because
    /// `to_state_machine` reads the starting state off the FIRST node, map
    /// order would also hand back a machine that starts somewhere else. Sorting
    /// makes the round trip an identity instead of a shuffle.
    ///
    /// ⚠️ Not everything survives a re-apply. A nested `StateMachine`, a
    /// `Blend2` and the other composite nodes are labelled with their real
    /// [`NodeKind`] so the editor shows what is actually in the scene, but the
    /// editor has no surface for their children — converting such a graph back
    /// flattens them to a leaf. Showing them as plain animations would hide
    /// that; showing them truthfully at least lets a user see what they are
    /// about to lose.
    pub fn from_state_machine(machine: &StateMachine) -> Self {
        let mut graph = Self::new();

        let mut names: Vec<&String> = machine.states.keys().collect();
        names.sort();
        // Stable, so the current state leads and the rest stay in name order.
        names.sort_by_key(|n| *n != &machine.current);

        for (i, name) in names.into_iter().enumerate() {
            let node = machine.states.get(name).expect("key came from the map");
            let id = graph.add_node(name.clone(), Self::graph_kind(node));
            // ⚠️ Every node would otherwise sit at the origin, drawing the whole
            // machine as one stack the user cannot pull apart. The runtime
            // `StateMachine` carries no coordinates — Godot keeps them on the
            // resource as `states/<name>/position`, which our type does not
            // model yet — so the load lays out a grid. Moving a node persists
            // in the graph as usual; only the FIRST open is auto-arranged.
            graph.move_node(
                id,
                (i % LOAD_COLUMNS) as f64 * LOAD_COLUMN_WIDTH,
                (i / LOAD_COLUMNS) as f64 * LOAD_ROW_HEIGHT,
            );
            Self::store_leaf_clip(&mut graph, id, name, node);
            Self::store_blend_points(&mut graph, id, node);
        }

        for t in &machine.transitions {
            let (Some(from), Some(to)) =
                (graph.node_id_by_name(&t.from), graph.node_id_by_name(&t.to))
            else {
                // An edge naming a state the machine does not hold describes
                // nothing the editor could draw; dropping it beats inventing
                // the missing state.
                continue;
            };
            graph.add_transition_with_xfade(from, to, t.crossfade);
        }
        graph
    }

    /// The editor kind that names a runtime node.
    fn graph_kind(node: &AnimationNode) -> NodeKind {
        match node {
            AnimationNode::BlendSpace1D(_) => NodeKind::BlendSpace1D,
            AnimationNode::BlendSpace2D(_) => NodeKind::BlendSpace2D,
            AnimationNode::StateMachine(_) => NodeKind::StateMachine,
            AnimationNode::Blend2 { .. } => NodeKind::Blend2,
            // Every other runtime node plays or composes clips and has no
            // editor kind of its own; `Animation` is the closest honest label.
            _ => NodeKind::Animation,
        }
    }

    /// Records a leaf's clip name when it differs from the state name.
    ///
    /// A state key and the clip it plays are independent in Godot — a `walk`
    /// state may play `walk_cycle`. [`runtime_node`](Self::runtime_node) reads
    /// this key back, so loading and re-applying a machine keeps the clip
    /// instead of renaming it after its state.
    fn store_leaf_clip(graph: &mut Self, id: NodeId, state: &str, node: &AnimationNode) {
        if let AnimationNode::Animation { name, .. } = node {
            if name != state {
                graph.set_param(id, "animation", name.clone());
            }
        }
    }

    /// Writes a blend space's points back into `params` under the same
    /// `blend_point_N/*` names the param route and the point editor use, so a
    /// loaded graph is indistinguishable from an authored one — and so
    /// [`blend_points`](Self::blend_points) reads them straight back.
    fn store_blend_points(graph: &mut Self, id: NodeId, node: &AnimationNode) {
        match node {
            AnimationNode::BlendSpace1D(space) => {
                for (i, (pos, animation)) in space.points().iter().enumerate() {
                    graph.set_param(id, format!("blend_point_{i}/pos"), pos.to_string());
                    graph.set_param(id, format!("blend_point_{i}/node"), animation.clone());
                }
            }
            AnimationNode::BlendSpace2D(space) => {
                for (i, (pos, animation)) in space.points().iter().enumerate() {
                    graph.set_param(
                        id,
                        format!("blend_point_{i}/pos"),
                        format!("{},{}", pos.x, pos.y),
                    );
                    graph.set_param(id, format!("blend_point_{i}/node"), animation.clone());
                }
            }
            _ => {}
        }
    }

    /// A blend space's authored points, in index order.
    ///
    /// ⚠️ They live in [`Node::params`] under Godot's own indexed
    /// resource-property names — `blend_point_N/pos` and `blend_point_N/node` —
    /// because that is what `POST /api/animation/tree/node/param` writes and
    /// what the point editor in `editor_ui.rs` edits. Reading anywhere else
    /// yields a blend space with no points for every graph a user actually
    /// authored (pat-s3rrv.1).
    ///
    /// `pos` is a scalar in 1D and an `"x,y"` pair in 2D; the `y` is simply 0.0
    /// for the 1D case, so one reader serves both. A point missing its `node`
    /// is skipped rather than defaulted: a blend point with no animation
    /// samples nothing, and inventing a clip name would be worse than omitting
    /// the point.
    fn blend_points(node: &Node) -> Vec<(gdcore::math::Vector2, &str)> {
        let mut out: Vec<(usize, gdcore::math::Vector2, &str)> = Vec::new();
        for (key, value) in &node.params {
            let Some(rest) = key.strip_prefix("blend_point_") else {
                continue;
            };
            let Some((index, field)) = rest.split_once('/') else {
                continue;
            };
            if field != "pos" {
                continue;
            }
            let Ok(index) = index.parse::<usize>() else {
                continue;
            };
            let mut parts = value.split(',').map(|p| p.trim().parse::<f32>());
            let x = match parts.next() {
                Some(Ok(v)) => v,
                _ => continue,
            };
            let y = match parts.next() {
                Some(Ok(v)) => v,
                _ => 0.0,
            };
            let Some(animation) = node.params.get(&format!("blend_point_{index}/node")) else {
                continue;
            };
            out.push((index, gdcore::math::Vector2::new(x, y), animation.as_str()));
        }
        // Index order, not map order: the points describe a 1D ramp whose
        // ordering is meaningful.
        out.sort_by_key(|(i, _, _)| *i);
        out.into_iter().map(|(_, p, a)| (p, a)).collect()
    }

    /// Builds the runtime [`StateMachine`](gdscene::animation_tree::StateMachine)
    /// this graph describes (pat-s3rrv.1).
    ///
    /// # Why this exists
    ///
    /// The editor's graph was pure in-memory state: 16 route call sites mutate
    /// it, one GET reads it back, and nothing ever turned it into something the
    /// runtime could play. Every state, transition and blend point authored in
    /// the served editor evaporated — the routes returned 200 the whole time.
    ///
    /// Each graph node becomes a state, keyed by name, and each transition link
    /// becomes a [`StateTransition`] carrying its `xfade`. Node kinds map onto
    /// the runtime node they name: an [`NodeKind::Animation`] state plays the
    /// clip its name refers to, and a blend space becomes a real
    /// `BlendSpace1D`/`2D` carrying the points authored on it.
    ///
    /// ⚠️ `current` is the FIRST node added, not an arbitrary map entry. States
    /// live in a `HashMap`, so taking "some" key would start a different state
    /// on every run — the same class of nondeterminism the saver avoids by
    /// sorting. Godot starts at the state wired to the graph's Start node; until
    /// the editor models one, insertion order is the closest stable answer.
    pub fn to_state_machine(&self) -> StateMachine {
        let mut sm = StateMachine::new();

        for node in &self.nodes {
            sm.states.insert(node.name.clone(), self.runtime_node(node));
        }
        if let Some(first) = self.nodes.first() {
            sm.current = first.name.clone();
        }

        for link in &self.transitions {
            let (Some(from), Some(to)) = (self.get_node(link.from), self.get_node(link.to)) else {
                continue;
            };
            // ⚠️ `StateTransition` has no `new`; it derives its Godot defaults
            // through `Default` (animation_node_state_machine.h:53-63), so the
            // rest of the edge — switch_mode, advance_mode, priority — keeps
            // upstream's values rather than zeroes.
            sm.transitions.push(StateTransition {
                from: from.name.clone(),
                to: to.name.clone(),
                crossfade: link.xfade,
                ..StateTransition::default()
            });
        }
        sm
    }

    /// The runtime node one graph node describes.
    fn runtime_node(&self, node: &Node) -> AnimationNode {
        match node.kind {
            NodeKind::BlendSpace1D => {
                let mut bs = BlendSpace1D::new();
                for (pos, animation) in Self::blend_points(node) {
                    bs.add_point(pos.x, animation);
                }
                AnimationNode::BlendSpace1D(bs)
            }
            NodeKind::BlendSpace2D => {
                let mut bs = BlendSpace2D::new();
                for (pos, animation) in Self::blend_points(node) {
                    bs.add_point(pos, animation);
                }
                AnimationNode::BlendSpace2D(bs)
            }
            // Everything else plays the clip its name refers to. A StateMachine
            // node inside the graph is a STATE of this machine rather than a
            // nested machine: the editor has no nesting surface yet, and
            // inventing an empty nested machine here would produce a state that
            // silently plays nothing.
            // The clip is the `animation` param when one is set, and the node
            // name otherwise (pat-s3rrv.1). A state key and the clip it plays
            // are independent in Godot — a `walk` state may play `walk_cycle` —
            // and before this the name was the ONLY way to say which clip, so
            // loading such a machine and applying it renamed the clip.
            _ => AnimationNode::Animation {
                name: node
                    .params
                    .get("animation")
                    .cloned()
                    .unwrap_or_else(|| node.name.clone()),
                position: 0.0,
                opts: AnimationLeafOptions::default(),
            },
        }
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
    /// pat-s3rrv.1: the authored graph becomes a runtime state machine.
    ///
    /// Everything the served editor authored was in-memory only — 16 route call
    /// sites mutated `EditorState.anim_tree`, one GET read it back, and nothing
    /// ever built an `AnimationNode` from it. The routes returned 200 the whole
    /// time.
    #[test]
    fn the_graph_becomes_a_runtime_state_machine() {
        let mut g = AnimTreeGraph::new();
        let idle = g.add_node("idle", NodeKind::Animation);
        let run = g.add_node("run", NodeKind::Animation);
        assert!(g.add_transition_with_xfade(idle, run, 0.25));

        let sm = g.to_state_machine();

        assert_eq!(sm.states.len(), 2);
        assert!(sm.states.contains_key("idle") && sm.states.contains_key("run"));
        assert_eq!(sm.transitions.len(), 1);
        assert_eq!(sm.transitions[0].from, "idle");
        assert_eq!(sm.transitions[0].to, "run");
        assert_eq!(
            sm.transitions[0].crossfade, 0.25,
            "the authored xfade must survive the conversion"
        );
    }

    /// ⚠️ `current` is the FIRST node added, not an arbitrary map entry.
    ///
    /// States live in a `HashMap`, so taking "some" key would start a different
    /// state on each run. Asserting it explicitly is what makes the choice a
    /// contract rather than an accident of iteration order.
    #[test]
    fn the_first_authored_state_is_the_starting_one() {
        let mut g = AnimTreeGraph::new();
        for name in ["first", "second", "third", "fourth"] {
            g.add_node(name, NodeKind::Animation);
        }
        assert_eq!(g.to_state_machine().current, "first");
    }

    /// A blend space becomes a real blend space carrying its points — the whole
    /// point of the model work, since the runtime evaluator
    /// (`sample_blend_space_2d`) has always been able to consume them.
    #[test]
    fn blend_points_reach_the_runtime_blend_space() {
        let mut g = AnimTreeGraph::new();
        let bs = g.add_node("Move", NodeKind::BlendSpace2D);
        // ⚠️ Authored through `params` with Godot's own indexed key names —
        // exactly what POST /api/animation/tree/node/param writes. Any other
        // representation would convert to a blend space with no points.
        g.set_param(bs, "blend_point_0/pos", "-1,0");
        g.set_param(bs, "blend_point_0/node", "left");
        g.set_param(bs, "blend_point_1/pos", "1,0");
        g.set_param(bs, "blend_point_1/node", "right");

        let sm = g.to_state_machine();
        match sm.states.get("Move").expect("the blend space is a state") {
            AnimationNode::BlendSpace2D(space) => {
                assert_eq!(space.points().len(), 2, "both authored points carry over");
                assert_eq!(space.points()[0].1, "left");
                assert_eq!(space.points()[1].0.x, 1.0);
            }
            other => panic!("expected a BlendSpace2D, got {other:?}"),
        }
    }

    /// A 1D blend space keeps its scalar positions.
    #[test]
    fn a_1d_blend_space_carries_scalar_points() {
        let mut g = AnimTreeGraph::new();
        let bs = g.add_node("Speed", NodeKind::BlendSpace1D);
        // A 1D point's `pos` is a bare scalar, not an "x,y" pair.
        g.set_param(bs, "blend_point_0/pos", "2.0");
        g.set_param(bs, "blend_point_0/node", "run");

        let sm = g.to_state_machine();
        match sm.states.get("Speed").expect("state") {
            AnimationNode::BlendSpace1D(space) => {
                assert_eq!(space.points().len(), 1);
                assert_eq!(space.points()[0].0, 2.0);
                assert_eq!(space.points()[0].1, "run");
            }
            other => panic!("expected a BlendSpace1D, got {other:?}"),
        }
    }

    /// ⚠️ A point whose `node` is missing is SKIPPED, not defaulted: a blend
    /// point with no animation samples nothing, and inventing a clip name would
    /// hide the authoring mistake behind a silent wrong result.
    #[test]
    fn a_point_without_an_animation_is_skipped() {
        let mut g = AnimTreeGraph::new();
        let bs = g.add_node("Speed", NodeKind::BlendSpace1D);
        g.set_param(bs, "blend_point_0/pos", "0.0");
        g.set_param(bs, "blend_point_0/node", "idle");
        g.set_param(bs, "blend_point_1/pos", "1.0"); // no `node`

        let sm = g.to_state_machine();
        match sm.states.get("Speed").expect("state") {
            AnimationNode::BlendSpace1D(space) => assert_eq!(space.points().len(), 1),
            other => panic!("expected a BlendSpace1D, got {other:?}"),
        }
    }

    /// Points come back in INDEX order, not map order — a 1D ramp's ordering is
    /// meaningful, and `params` is a BTreeMap keyed by string, where
    /// `blend_point_10` sorts before `blend_point_2`.
    #[test]
    fn points_are_ordered_by_index_not_by_key() {
        let mut g = AnimTreeGraph::new();
        let bs = g.add_node("Speed", NodeKind::BlendSpace1D);
        for (i, (pos, clip)) in [("0.0", "a"), ("1.0", "b"), ("2.0", "c")]
            .iter()
            .enumerate()
        {
            g.set_param(bs, format!("blend_point_{i}/pos"), *pos);
            g.set_param(bs, format!("blend_point_{i}/node"), *clip);
        }
        g.set_param(bs, "blend_point_10/pos", "10.0");
        g.set_param(bs, "blend_point_10/node", "last");

        let sm = g.to_state_machine();
        match sm.states.get("Speed").expect("state") {
            AnimationNode::BlendSpace1D(space) => {
                let names: Vec<&str> = space.points().iter().map(|p| p.1.as_str()).collect();
                assert_eq!(names, vec!["a", "b", "c", "last"]);
            }
            other => panic!("expected a BlendSpace1D, got {other:?}"),
        }
    }

    use super::*;

    /// Builds a machine the way a scene would hold one, so the tests below
    /// exercise the load direction rather than a graph they just authored.
    fn leaf(name: &str) -> AnimationNode {
        AnimationNode::Animation {
            name: name.to_string(),
            position: 0.0,
            opts: AnimationLeafOptions::default(),
        }
    }

    /// pat-s3rrv.1: a scene's existing machine becomes the editor's graph.
    ///
    /// Nothing built an `AnimTreeGraph` from an `AnimationNode` tree, so opening
    /// a project with an existing `AnimationTree` showed an empty canvas — and
    /// once `POST /api/animation/tree/apply` landed, saving that empty canvas
    /// replaced the real machine with nothing.
    #[test]
    fn a_runtime_machine_becomes_an_editor_graph() {
        let mut sm = StateMachine::new();
        sm.states.insert("idle".into(), leaf("idle"));
        sm.states.insert("run".into(), leaf("run"));
        sm.current = "idle".into();
        sm.transitions.push(StateTransition {
            from: "idle".into(),
            to: "run".into(),
            crossfade: 0.25,
            ..StateTransition::default()
        });

        let g = AnimTreeGraph::from_state_machine(&sm);

        assert_eq!(g.node_count(), 2, "every state is a node the user can see");
        let idle = g.node_id_by_name("idle").expect("idle loaded");
        let run = g.node_id_by_name("run").expect("run loaded");
        assert!(g.has_transition(idle, run), "the edge loaded too");
        assert_eq!(
            g.transition_xfade(idle, run),
            Some(0.25),
            "the crossfade is authored data, not a runtime detail"
        );
    }

    /// The round trip is an identity, which is what makes the apply route safe:
    /// opening a scene and saving it back without touching anything must leave
    /// the machine exactly as it was.
    #[test]
    fn loading_then_applying_leaves_the_machine_unchanged() {
        let mut before = StateMachine::new();
        for name in ["walk", "idle", "run"] {
            before.states.insert(name.into(), leaf(name));
        }
        before.current = "idle".into();
        before.transitions.push(StateTransition {
            from: "idle".into(),
            to: "run".into(),
            crossfade: 0.5,
            ..StateTransition::default()
        });

        let after = AnimTreeGraph::from_state_machine(&before).to_state_machine();

        assert_eq!(after.states.len(), 3, "no state was dropped");
        assert!(["walk", "idle", "run"]
            .iter()
            .all(|n| after.states.contains_key(*n)));
        // ⚠️ `to_state_machine` takes the FIRST node as the starting state, so
        // this only holds because the load sorts `current` to the front. With
        // `HashMap` order the machine would come back starting somewhere else.
        assert_eq!(
            after.current, "idle",
            "the starting state survives the round trip"
        );
        assert_eq!(after.transitions.len(), 1);
        assert_eq!(after.transitions[0].crossfade, 0.5);
    }

    /// Two identical machines must load to identically ordered graphs.
    ///
    /// Each `HashMap` carries its own random hash seed, so iterating
    /// `states` directly gives a different node order per map — the graph would
    /// be laid out differently every time a scene was opened.
    #[test]
    fn the_loaded_layout_does_not_depend_on_map_order() {
        let build = || {
            let mut sm = StateMachine::new();
            for name in ["delta", "alpha", "charlie", "bravo", "echo"] {
                sm.states.insert(name.into(), leaf(name));
            }
            sm.current = "charlie".into();
            sm
        };

        let names = |g: &AnimTreeGraph| -> Vec<String> {
            g.nodes().iter().map(|n| n.name.clone()).collect()
        };
        let first = names(&AnimTreeGraph::from_state_machine(&build()));

        assert_eq!(
            first,
            vec!["charlie", "alpha", "bravo", "delta", "echo"],
            "the current state leads, the rest are in name order"
        );
        assert_eq!(
            first,
            names(&AnimTreeGraph::from_state_machine(&build())),
            "a second load of the same machine lays out the same way"
        );
    }

    /// A blend space comes back with its points in `params`, under the same
    /// keys the param route writes — so the point editor can edit a loaded
    /// blend space, not just one authored in this session.
    #[test]
    fn a_blend_space_state_reloads_its_points() {
        let mut space = BlendSpace2D::new();
        space.add_point(gdcore::math::Vector2::new(-1.0, 0.0), "left");
        space.add_point(gdcore::math::Vector2::new(1.0, 0.5), "right");
        let mut sm = StateMachine::new();
        sm.states
            .insert("Move".into(), AnimationNode::BlendSpace2D(space));

        let g = AnimTreeGraph::from_state_machine(&sm);
        let id = g.node_id_by_name("Move").expect("the blend space loaded");

        assert_eq!(g.get_node(id).map(|n| n.kind), Some(NodeKind::BlendSpace2D));
        assert_eq!(g.param(id, "blend_point_0/pos"), Some("-1,0"));
        assert_eq!(g.param(id, "blend_point_0/node"), Some("left"));
        assert_eq!(g.param(id, "blend_point_1/pos"), Some("1,0.5"));
        assert_eq!(g.param(id, "blend_point_1/node"), Some("right"));

        // And they survive going back — the points must round-trip through the
        // param representation, not just be readable in it.
        match AnimTreeGraph::from_state_machine(&sm)
            .to_state_machine()
            .states
            .remove("Move")
        {
            Some(AnimationNode::BlendSpace2D(back)) => {
                assert_eq!(back.points().len(), 2);
                assert_eq!(back.points()[1].1, "right");
                assert_eq!(back.points()[1].0.y, 0.5);
            }
            other => panic!("expected a BlendSpace2D, got {other:?}"),
        }
    }

    /// A 1D blend space's scalar positions survive the same way.
    #[test]
    fn a_1d_blend_space_state_reloads_its_points() {
        let mut space = BlendSpace1D::new();
        space.add_point(-1.0, "slow");
        space.add_point(0.5, "fast");
        let mut sm = StateMachine::new();
        sm.states
            .insert("Speed".into(), AnimationNode::BlendSpace1D(space));

        let g = AnimTreeGraph::from_state_machine(&sm);
        let id = g.node_id_by_name("Speed").expect("the blend space loaded");

        assert_eq!(g.get_node(id).map(|n| n.kind), Some(NodeKind::BlendSpace1D));
        assert_eq!(
            g.param(id, "blend_point_1/pos"),
            Some("0.5"),
            "a 1D position is a scalar, not an `x,y` pair"
        );
        assert_eq!(g.param(id, "blend_point_1/node"), Some("fast"));
    }

    /// A state and the clip it plays are independent in Godot. Before
    /// pat-s3rrv.1 the node name was the only way to name a clip, so loading a
    /// machine whose clip differed from its state and applying it back RENAMED
    /// the clip.
    #[test]
    fn a_clip_that_differs_from_its_state_is_not_renamed() {
        let mut sm = StateMachine::new();
        sm.states.insert("walk".into(), leaf("walk_cycle"));

        let g = AnimTreeGraph::from_state_machine(&sm);
        let id = g.node_id_by_name("walk").expect("the state loaded");
        assert_eq!(
            g.param(id, "animation"),
            Some("walk_cycle"),
            "the clip is preserved where the param route can edit it"
        );

        match g.to_state_machine().states.remove("walk") {
            Some(AnimationNode::Animation { name, .. }) => assert_eq!(name, "walk_cycle"),
            other => panic!("expected an Animation leaf, got {other:?}"),
        }
    }

    /// A state whose clip matches its name carries no redundant param — an
    /// authored graph and a loaded one must look the same, or the editor shows
    /// a key the user never typed.
    #[test]
    fn a_clip_matching_its_state_adds_no_param() {
        let mut sm = StateMachine::new();
        sm.states.insert("idle".into(), leaf("idle"));

        let g = AnimTreeGraph::from_state_machine(&sm);
        let id = g.node_id_by_name("idle").expect("the state loaded");
        assert_eq!(g.param(id, "animation"), None);
    }

    /// A transition naming a state the machine does not hold is dropped rather
    /// than conjuring the missing state — a node the scene has no state for
    /// would be applied back as a real one.
    #[test]
    fn an_edge_to_a_missing_state_is_dropped() {
        let mut sm = StateMachine::new();
        sm.states.insert("idle".into(), leaf("idle"));
        sm.transitions.push(StateTransition {
            from: "idle".into(),
            to: "ghost".into(),
            ..StateTransition::default()
        });

        let g = AnimTreeGraph::from_state_machine(&sm);

        assert_eq!(g.node_count(), 1, "no node is invented for `ghost`");
        assert!(g.transitions().is_empty(), "and the dangling edge is gone");
    }

    /// A loaded machine is laid out on a grid rather than stacked at the
    /// origin. `add_node` places every node at (0,0), so without this the whole
    /// machine draws as one pile the user cannot separate.
    #[test]
    fn a_loaded_machine_is_laid_out_not_stacked() {
        let mut sm = StateMachine::new();
        for name in ["a", "b", "c", "d", "e"] {
            sm.states.insert(name.into(), leaf(name));
        }
        sm.current = "a".into();

        let g = AnimTreeGraph::from_state_machine(&sm);
        let at = |name: &str| {
            let n = g
                .get_node(g.node_id_by_name(name).expect("loaded"))
                .unwrap();
            (n.x, n.y)
        };

        assert_eq!(at("a"), (0.0, 0.0), "the current state anchors the grid");
        assert_eq!(at("b"), (220.0, 0.0));
        assert_eq!(at("e"), (0.0, 140.0), "the fifth node wraps to a new row");
        let mut seen: Vec<(u64, u64)> =
            g.nodes().iter().map(|n| (n.x as u64, n.y as u64)).collect();
        seen.sort();
        seen.dedup();
        assert_eq!(seen.len(), 5, "no two nodes share a position");
    }

    /// An empty machine loads to an empty graph without panicking: a scene may
    /// hold an `AnimationTree` whose root machine has no states yet.
    #[test]
    fn an_empty_machine_loads_to_an_empty_graph() {
        let g = AnimTreeGraph::from_state_machine(&StateMachine::new());
        assert_eq!(g.node_count(), 0);
        assert!(g.transitions().is_empty());
    }

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
