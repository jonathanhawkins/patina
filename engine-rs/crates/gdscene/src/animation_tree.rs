//! AnimationTree — graph-based animation evaluation.
//!
//! Mirrors Godot's [`AnimationTree`] system: a tree of [`AnimationNode`]s
//! produces sampled property values from an [`Animation`] library. Supported
//! node types are [`AnimationNode::Animation`], [`AnimationNode::Blend2`],
//! [`AnimationNode::Blend3`], [`AnimationNode::OneShot`],
//! [`AnimationNode::BlendSpace1D`], [`AnimationNode::BlendSpace2D`],
//! [`AnimationNode::StateMachine`], and [`AnimationNode::Named`] (a naming
//! wrapper that makes a node's `parameters/<name>/...` entries addressable).
//!
//! A tree stored in the [`SceneTree`] is advanced every frame by
//! [`SceneTree::process_animation_trees`]; graphs are built from a `.tscn` by
//! [`crate::animation_tree_tscn`].

use std::collections::HashMap;

use gdcore::math::Vector2;
use gdvariant::Variant;

use crate::animation::{interpolate_variant, Animation};
use crate::node::NodeId;
use crate::scene_tree::SceneTree;

/// A node in the animation graph.
#[derive(Debug, Clone)]
pub enum AnimationNode {
    /// Plays a single animation by name.
    Animation { name: String, position: f64 },
    /// Blends two child nodes by a scalar weight in `[0,1]`.
    Blend2 {
        a: Box<AnimationNode>,
        b: Box<AnimationNode>,
        weight: f32,
    },
    /// Three-way blend: `weight` in `[-1, 1]` selects between `a` (at -1),
    /// `b` (at 0), and `c` (at +1). Matches Godot's `AnimationNodeBlend3`.
    Blend3 {
        a: Box<AnimationNode>,
        b: Box<AnimationNode>,
        c: Box<AnimationNode>,
        weight: f32,
    },
    /// Plays a shot child once over `base` when `trigger_param` rises from
    /// false to true. Fades in over `fade_in`, holds for `shot_length`, then
    /// fades out over `fade_out`. Mirrors Godot's `AnimationNodeOneShot`.
    OneShot(OneShot),
    /// 1D blend space — selects/blends among points along a single axis.
    BlendSpace1D(BlendSpace1D),
    /// 2D blend space — bilinear/triangular blend over 2D positions.
    BlendSpace2D(BlendSpace2D),
    /// State machine that plays one animation at a time and crossfades
    /// between states when transitions fire.
    StateMachine(StateMachine),
    /// A named pass-through wrapper around `inner`, carrying the graph-node name
    /// the node has in a `.tscn`/`.tres` (a state name, or a blend-tree node
    /// name). The name is what makes Godot's `parameters/<name>/...` entries
    /// addressable: during [`AnimationTree::advance`] the tree copies
    /// `parameters/<name>/blend_amount`, `parameters/<name>/blend_position`, and
    /// `parameters/<name>/request` onto the wrapped node before evaluating it.
    /// Sampling is transparent — a `Named` samples exactly as `inner` does.
    Named {
        /// The graph node's name, as written in the scene/resource file.
        name: String,
        /// The wrapped node.
        inner: Box<AnimationNode>,
    },
}

/// A 1D blend space. Points are sorted by `position` on insert.
#[derive(Debug, Clone, Default)]
pub struct BlendSpace1D {
    points: Vec<(f32, String)>,
    pub blend_position: f32,
    /// Shared playback clock (seconds), advanced by the tree each frame so the
    /// blended clips play over time rather than being frozen at t=0.
    pub elapsed: f64,
}

impl BlendSpace1D {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_point(&mut self, position: f32, animation: impl Into<String>) {
        self.points.push((position, animation.into()));
        self.points
            .sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    }

    pub fn points(&self) -> &[(f32, String)] {
        &self.points
    }
}

/// A 2D blend space — uses bilinear interpolation over a grid-aligned set
/// of four points, or nearest-neighbor for any other configuration.
#[derive(Debug, Clone, Default)]
pub struct BlendSpace2D {
    points: Vec<(Vector2, String)>,
    pub blend_position: Vector2,
    /// Shared playback clock (seconds), advanced by the tree each frame so the
    /// blended clips play over time rather than being frozen at t=0.
    pub elapsed: f64,
}

impl BlendSpace2D {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_point(&mut self, position: Vector2, animation: impl Into<String>) {
        self.points.push((position, animation.into()));
    }

    pub fn points(&self) -> &[(Vector2, String)] {
        &self.points
    }
}

/// OneShot node — plays `shot` on top of `base` when triggered.
#[derive(Debug, Clone)]
pub struct OneShot {
    pub base: Box<AnimationNode>,
    pub shot: Box<AnimationNode>,
    pub fade_in: f64,
    pub fade_out: f64,
    pub shot_length: f64,
    pub trigger_param: String,
    pub elapsed: f64,
    pub active: bool,
    prev_trigger: bool,
    /// Last `parameters/<name>/request` value seen through a wrapping
    /// [`AnimationNode::Named`], so a request only acts on its rising edge
    /// (Godot clears the request after processing it; we latch instead).
    prev_request: i64,
}

/// Godot `AnimationNodeOneShot.ONE_SHOT_REQUEST_NONE`.
pub const ONE_SHOT_REQUEST_NONE: i64 = 0;
/// Godot `AnimationNodeOneShot.ONE_SHOT_REQUEST_FIRE`.
pub const ONE_SHOT_REQUEST_FIRE: i64 = 1;
/// Godot `AnimationNodeOneShot.ONE_SHOT_REQUEST_ABORT`.
pub const ONE_SHOT_REQUEST_ABORT: i64 = 2;
/// Godot `AnimationNodeOneShot.ONE_SHOT_REQUEST_FADE_OUT`.
pub const ONE_SHOT_REQUEST_FADE_OUT: i64 = 3;

impl OneShot {
    pub fn new(base: AnimationNode, shot: AnimationNode) -> Self {
        Self {
            base: Box::new(base),
            shot: Box::new(shot),
            fade_in: 0.0,
            fade_out: 0.0,
            shot_length: 0.0,
            trigger_param: String::new(),
            elapsed: 0.0,
            active: false,
            prev_trigger: false,
            prev_request: ONE_SHOT_REQUEST_NONE,
        }
    }

    /// Starts the shot from the beginning (Godot `ONE_SHOT_REQUEST_FIRE`).
    pub fn fire(&mut self) {
        reset_node_time(&mut self.shot);
        self.active = true;
        self.elapsed = 0.0;
    }

    /// Stops the shot immediately, leaving only `base` (Godot
    /// `ONE_SHOT_REQUEST_ABORT`).
    pub fn abort(&mut self) {
        self.active = false;
        self.elapsed = 0.0;
    }

    /// Jumps to the start of the fade-out phase (Godot
    /// `ONE_SHOT_REQUEST_FADE_OUT`). A no-op when the shot is not active.
    pub fn fade_out_now(&mut self) {
        if self.active {
            self.elapsed = self.elapsed.max(self.fade_in + self.shot_length);
        }
    }

    pub fn with_fades(mut self, fade_in: f64, fade_out: f64) -> Self {
        self.fade_in = fade_in.max(0.0);
        self.fade_out = fade_out.max(0.0);
        self
    }

    pub fn with_shot_length(mut self, shot_length: f64) -> Self {
        self.shot_length = shot_length.max(0.0);
        self
    }

    pub fn with_trigger(mut self, param: impl Into<String>) -> Self {
        self.trigger_param = param.into();
        self
    }

    pub fn total_duration(&self) -> f64 {
        self.fade_in + self.shot_length + self.fade_out
    }

    /// Blend factor applied to `shot` at the current elapsed time.
    fn blend_factor(&self) -> f32 {
        if !self.active {
            return 0.0;
        }
        let e = self.elapsed;
        if e < self.fade_in {
            return if self.fade_in > 0.0 {
                (e / self.fade_in) as f32
            } else {
                1.0
            };
        }
        let hold_end = self.fade_in + self.shot_length;
        if e < hold_end {
            return 1.0;
        }
        let fade_out_end = hold_end + self.fade_out;
        if e < fade_out_end {
            return if self.fade_out > 0.0 {
                (1.0 - (e - hold_end) / self.fade_out) as f32
            } else {
                0.0
            };
        }
        0.0
    }
}

/// A state-machine node. Each state maps to a child `AnimationNode`.
#[derive(Debug, Clone, Default)]
pub struct StateMachine {
    pub states: HashMap<String, AnimationNode>,
    pub transitions: Vec<StateTransition>,
    pub current: String,
    pub elapsed: f64,
    transition: Option<ActiveTransition>,
    /// Remaining states of a queued [`travel`](StateMachine::travel) path — the
    /// ordered hops after `current`. Each `advance` takes the next hop
    /// (crossfading) once any active transition completes; empty when not
    /// travelling.
    travel_queue: Vec<String>,
}

impl StateMachine {
    /// Create an empty state machine. Provided for consistency with the other
    /// animation-node constructors (`BlendSpace1D::new`, `OneShot::new`, …);
    /// equivalent to `StateMachine::default()`.
    pub fn new() -> Self {
        Self::default()
    }

    /// Computes the shortest transition path from `current` to `target` via BFS
    /// over the declared (directional) transitions, returning the ordered list
    /// of states to walk **after** `current`.
    ///
    /// Returns `Some(vec![])` when already at `target`, and `None` when
    /// `target` is unknown or unreachable through the transition graph.
    pub fn travel_path(&self, target: &str) -> Option<Vec<String>> {
        if target == self.current {
            return Some(Vec::new());
        }
        if !self.states.contains_key(target) {
            return None;
        }
        // BFS; `pred` maps each discovered state to the state we reached it from.
        let mut pred: HashMap<&str, &str> = HashMap::new();
        let mut queue: std::collections::VecDeque<&str> = std::collections::VecDeque::new();
        queue.push_back(self.current.as_str());
        while let Some(state) = queue.pop_front() {
            for t in &self.transitions {
                if t.from == state && t.to != self.current && !pred.contains_key(t.to.as_str()) {
                    pred.insert(t.to.as_str(), state);
                    if t.to == target {
                        // Reconstruct target -> current, then reverse.
                        let mut path = vec![target.to_string()];
                        let mut cur = target;
                        while let Some(&prev) = pred.get(cur) {
                            if prev == self.current {
                                break;
                            }
                            path.push(prev.to_string());
                            cur = prev;
                        }
                        path.reverse();
                        return Some(path);
                    }
                    queue.push_back(t.to.as_str());
                }
            }
        }
        None
    }

    /// Queues a travel to `target` (Godot `AnimationNodeStateMachinePlayback::travel`).
    /// Subsequent `advance` calls crossfade along the shortest transition path,
    /// using each hop's declared crossfade. Returns `true` if a path was found
    /// (or already at `target`), `false` if `target` is unreachable.
    pub fn travel(&mut self, target: &str) -> bool {
        match self.travel_path(target) {
            Some(path) => {
                self.travel_queue = path;
                true
            }
            None => false,
        }
    }

    /// `true` while a queued travel path is still being walked.
    pub fn is_travelling(&self) -> bool {
        !self.travel_queue.is_empty()
    }
}

/// A declarative transition between two states.
#[derive(Debug, Clone)]
pub struct StateTransition {
    pub from: String,
    pub to: String,
    /// Name of a bool parameter controlling the transition.
    pub condition: String,
    /// Crossfade duration in seconds. 0.0 = instant snap.
    pub crossfade: f64,
}

#[derive(Debug, Clone)]
struct ActiveTransition {
    from_state: String,
    /// Snapshot of the from-state node at transition start (used for sampling).
    from_node: Box<AnimationNode>,
    elapsed: f64,
    duration: f64,
}

/// Root container that evaluates a graph of [`AnimationNode`]s.
#[derive(Debug, Clone)]
pub struct AnimationTree {
    pub animations: HashMap<String, Animation>,
    pub root: Option<AnimationNode>,
    pub parameters: HashMap<String, bool>,
    /// Godot-style `parameters/...` map keyed by string path (e.g.
    /// `"parameters/blend/blend_amount"`), holding arbitrary [`Variant`]
    /// values — the `AnimationTree.set()/get()` parameter surface.
    ///
    /// This map **drives evaluation**: `parameters/conditions/<name>` resolves a
    /// transition condition (Godot stores an `advance_condition` as the
    /// `conditions/<name>` parameter), and `parameters/<node>/blend_amount`,
    /// `parameters/<node>/blend_position`, `parameters/<node>/request` drive any
    /// graph node wrapped in [`AnimationNode::Named`]. The boolean `parameters`
    /// map above remains supported for hand-built trees; an entry here wins over
    /// it for the same condition name.
    pub params: HashMap<String, Variant>,
    pub active: bool,
    /// Godot `AnimationMixer.root_node` — the node that track paths resolve
    /// against, relative to the node the tree is attached to. Default `".."`
    /// (the tree node's parent), matching Godot's documented default.
    pub root_node: String,
    /// Godot `AnimationTree.anim_player` — the `AnimationPlayer` this tree pulls
    /// its clip library from, as a node path relative to the tree node. Empty by
    /// default (Godot's `NodePath("")`); recorded for round-tripping and for the
    /// scene-load wiring pass.
    pub anim_player: String,
}

impl AnimationTree {
    pub fn new() -> Self {
        Self {
            animations: HashMap::new(),
            root: None,
            parameters: HashMap::new(),
            params: HashMap::new(),
            active: true,
            root_node: "..".to_string(),
            anim_player: String::new(),
        }
    }

    pub fn add_animation(&mut self, anim: Animation) {
        self.animations.insert(anim.name.clone(), anim);
    }

    pub fn set_root(&mut self, node: AnimationNode) {
        self.root = Some(node);
    }

    /// Set a boolean parameter used as a transition condition.
    pub fn set_param(&mut self, name: impl Into<String>, value: bool) {
        self.parameters.insert(name.into(), value);
    }

    /// Sets a Godot-style tree parameter by path (e.g.
    /// `"parameters/blend/blend_amount"`) to an arbitrary [`Variant`].
    /// Mirrors Godot's `AnimationTree.set("parameters/...", value)`.
    ///
    /// The value is consulted by the next [`advance`](Self::advance): see
    /// [`params`](Self::params) for the recognised paths.
    pub fn set_parameter(&mut self, path: impl Into<String>, value: Variant) {
        self.params.insert(path.into(), value);
    }

    /// Returns a clone of the raw [`Variant`] stored at `path`, or `None` if
    /// no parameter has been set there. Mirrors `AnimationTree.get("parameters/...")`.
    pub fn get_parameter(&self, path: &str) -> Option<Variant> {
        self.params.get(path).cloned()
    }

    /// Returns the parameter at `path` as an `f32` when it is numeric —
    /// [`Variant::Float`] (cast `f64`→`f32`) or [`Variant::Int`]
    /// (cast `i64`→`f32`) — and `None` for any other type or a missing key.
    pub fn get_parameter_float(&self, path: &str) -> Option<f32> {
        match self.params.get(path) {
            Some(Variant::Float(f)) => Some(*f as f32),
            Some(Variant::Int(i)) => Some(*i as f32),
            _ => None,
        }
    }

    /// Returns the parameter at `path` as a [`Vector2`] when it is stored as a
    /// [`Variant::Vector2`], and `None` for any other type or a missing key.
    pub fn get_parameter_vector2(&self, path: &str) -> Option<Vector2> {
        match self.params.get(path) {
            Some(Variant::Vector2(v)) => Some(*v),
            _ => None,
        }
    }

    /// Returns the parameter at `path` as an `i64` when it is stored as a
    /// [`Variant::Int`], and `None` for any other type or a missing key.
    pub fn get_parameter_int(&self, path: &str) -> Option<i64> {
        match self.params.get(path) {
            Some(Variant::Int(i)) => Some(*i),
            _ => None,
        }
    }

    /// Advances all time-dependent nodes by `delta` seconds, resolving
    /// transition conditions and blend parameters from both parameter maps.
    pub fn advance(&mut self, delta: f64) {
        if !self.active || delta == 0.0 {
            return;
        }
        let mut root = match self.root.take() {
            Some(r) => r,
            None => return,
        };
        let params = EvalParams {
            bools: &self.parameters,
            vars: &self.params,
        };
        // The root node is unnamed in Godot, so its own parameters live directly
        // at `parameters/<param>` (e.g. `parameters/blend_position` for a root
        // blend space). Named children are handled inside `advance_node`.
        apply_named_params("", &mut root, params);
        advance_node(&mut root, delta, &self.animations, params);
        self.root = Some(root);
    }

    /// Samples the current values of the graph as `(property_path, value)` pairs.
    ///
    /// Track node paths are dropped — every value is reported by property alone.
    /// Use [`get_current_values_targeted`](Self::get_current_values_targeted)
    /// when the caller needs to resolve each value's target node.
    pub fn get_current_values(&self) -> Vec<(String, Variant)> {
        self.get_current_values_targeted()
            .into_iter()
            .map(|(_, prop, value)| (prop, value))
            .collect()
    }

    /// Samples the current values of the graph as
    /// `(node_path, property_path, value)` triples, where `node_path` is the
    /// track's path relative to [`root_node`](Self::root_node) (empty or `"."`
    /// means `root_node` itself). Mirrors
    /// [`AnimationPlayer::get_current_values_targeted`](crate::animation::AnimationPlayer::get_current_values_targeted)
    /// so a tree can drive several nodes, as Godot's mixer does.
    pub fn get_current_values_targeted(&self) -> Vec<(String, String, Variant)> {
        match &self.root {
            Some(root) => sample_node(root, &self.animations)
                .into_iter()
                .map(|((node_path, prop), value)| (node_path, prop, value))
                .collect(),
            None => Vec::new(),
        }
    }
}

impl Default for AnimationTree {
    fn default() -> Self {
        Self::new()
    }
}

/// Drives an [`AnimationTree`] for one frame and applies the resulting pose to a
/// node in the scene: advances all clocks (state machines, blend-space clocks,
/// one-shots, animation leaves) by `delta`, samples the blended property values,
/// and writes each `(property_path, value)` onto `node_id`.
///
/// Every sampled value is written to `node_id` itself, ignoring track node
/// paths. For the scene-driven path — a tree stored in the [`SceneTree`] and
/// ticked every frame, with each track resolved against the tree's
/// [`root_node`](AnimationTree::root_node) — see
/// [`SceneTree::attach_animation_tree`] and
/// [`SceneTree::process_animation_trees`].
pub fn advance_and_apply(
    tree: &mut AnimationTree,
    scene: &mut SceneTree,
    node_id: NodeId,
    delta: f64,
) {
    tree.advance(delta);
    let values = tree.get_current_values();
    if let Some(node) = scene.get_node_mut(node_id) {
        for (path, value) in values {
            node.set_property(&path, value);
        }
    }
}

// ---------------------------------------------------------------------------
// Evaluation
// ---------------------------------------------------------------------------

/// A sampled value's target: `(node_path, property_path)`. `node_path` is the
/// track's node path relative to the mixer's `root_node` (empty or `"."` means
/// `root_node` itself). Values blend by this whole key, so two clips only blend
/// where they drive the same property **of the same node**.
type TrackKey = (String, String);

/// The parameter sources consulted while evaluating a graph: the legacy boolean
/// condition map and the Godot-style `parameters/...` [`Variant`] map.
#[derive(Clone, Copy)]
struct EvalParams<'a> {
    bools: &'a HashMap<String, bool>,
    vars: &'a HashMap<String, Variant>,
}

impl EvalParams<'_> {
    /// Resolves a transition condition / one-shot trigger by name. Godot stores a
    /// transition's `advance_condition` as the `conditions/<name>` parameter, so
    /// the tree-level path is `parameters/conditions/<name>`; that entry wins
    /// over the legacy boolean map, which hand-built trees still use. An empty
    /// name is never satisfied (an unconditional edge only fires via `travel`).
    fn condition(&self, name: &str) -> bool {
        if name.is_empty() {
            return false;
        }
        if let Some(v) = variant_bool(self.vars.get(&format!("parameters/conditions/{name}"))) {
            return v;
        }
        if let Some(v) = self.bools.get(name) {
            return *v;
        }
        variant_bool(self.vars.get(name)).unwrap_or(false)
    }

    /// Numeric parameter at `path` (`Float` or `Int`), as `f32`.
    fn float(&self, path: &str) -> Option<f32> {
        match self.vars.get(path) {
            Some(Variant::Float(f)) => Some(*f as f32),
            Some(Variant::Int(i)) => Some(*i as f32),
            _ => None,
        }
    }

    /// [`Vector2`] parameter at `path`.
    fn vector2(&self, path: &str) -> Option<Vector2> {
        match self.vars.get(path) {
            Some(Variant::Vector2(v)) => Some(*v),
            _ => None,
        }
    }

    /// Integer parameter at `path`. A `Bool` coerces (`true` = 1) so a script
    /// that sets a one-shot request with `true` still fires it.
    fn int(&self, path: &str) -> Option<i64> {
        match self.vars.get(path) {
            Some(Variant::Int(i)) => Some(*i),
            Some(Variant::Bool(b)) => Some(*b as i64),
            _ => None,
        }
    }
}

/// Reads a [`Variant`] as a bool, accepting Godot's numeric truthiness
/// (`Int`/`Float` non-zero) as well as `Bool`.
fn variant_bool(value: Option<&Variant>) -> Option<bool> {
    match value {
        Some(Variant::Bool(b)) => Some(*b),
        Some(Variant::Int(i)) => Some(*i != 0),
        Some(Variant::Float(f)) => Some(*f != 0.0),
        _ => None,
    }
}

/// The Godot parameter path for `param` on the graph node called `name`. The
/// root node is unnamed, so its parameters sit directly under `parameters/`.
fn param_path(name: &str, param: &str) -> String {
    if name.is_empty() {
        format!("parameters/{param}")
    } else {
        format!("parameters/{name}/{param}")
    }
}

/// Copies the `parameters/<name>/...` entries addressed to a
/// [`AnimationNode::Named`] wrapper onto the node it wraps, before that node is
/// advanced. Mirrors how Godot's graph nodes read their own parameters.
fn apply_named_params(name: &str, inner: &mut AnimationNode, params: EvalParams<'_>) {
    match inner {
        AnimationNode::Blend2 { weight, .. } | AnimationNode::Blend3 { weight, .. } => {
            if let Some(w) = params.float(&param_path(name, "blend_amount")) {
                *weight = w;
            }
        }
        AnimationNode::BlendSpace1D(bs) => {
            if let Some(p) = params.float(&param_path(name, "blend_position")) {
                bs.blend_position = p;
            }
        }
        AnimationNode::BlendSpace2D(bs) => {
            if let Some(p) = params.vector2(&param_path(name, "blend_position")) {
                bs.blend_position = p;
            }
        }
        AnimationNode::OneShot(one_shot) => {
            let request = params
                .int(&param_path(name, "request"))
                .unwrap_or(ONE_SHOT_REQUEST_NONE);
            // Act on the rising edge only: Godot clears the request once it has
            // been processed, while our map keeps whatever the script last set.
            if request != one_shot.prev_request {
                match request {
                    ONE_SHOT_REQUEST_FIRE => one_shot.fire(),
                    ONE_SHOT_REQUEST_ABORT => one_shot.abort(),
                    ONE_SHOT_REQUEST_FADE_OUT => one_shot.fade_out_now(),
                    _ => {}
                }
            }
            one_shot.prev_request = request;
        }
        AnimationNode::Animation { .. }
        | AnimationNode::StateMachine(_)
        | AnimationNode::Named { .. } => {}
    }
}

fn advance_node(
    node: &mut AnimationNode,
    delta: f64,
    library: &HashMap<String, Animation>,
    params: EvalParams<'_>,
) {
    match node {
        AnimationNode::Animation { name, position } => {
            if let Some(anim) = library.get(name) {
                let length = anim.length.max(f64::EPSILON);
                *position = (*position + delta).rem_euclid(length);
            }
        }
        AnimationNode::Blend2 { a, b, .. } => {
            advance_node(a, delta, library, params);
            advance_node(b, delta, library, params);
        }
        AnimationNode::Blend3 { a, b, c, .. } => {
            advance_node(a, delta, library, params);
            advance_node(b, delta, library, params);
            advance_node(c, delta, library, params);
        }
        AnimationNode::OneShot(one_shot) => advance_one_shot(one_shot, delta, library, params),
        AnimationNode::BlendSpace1D(bs) => {
            // Advance the shared playback clock so the blended clips play over
            // time (each clip loops independently at sample time).
            bs.elapsed += delta;
        }
        AnimationNode::BlendSpace2D(bs) => {
            bs.elapsed += delta;
        }
        AnimationNode::StateMachine(sm) => advance_state_machine(sm, delta, library, params),
        AnimationNode::Named { name, inner } => {
            // Parameters land on the wrapped node before it advances, so a value
            // set this frame takes effect on this frame.
            apply_named_params(name, inner, params);
            advance_node(inner, delta, library, params);
        }
    }
}

fn advance_state_machine(
    sm: &mut StateMachine,
    delta: f64,
    library: &HashMap<String, Animation>,
    params: EvalParams<'_>,
) {
    sm.elapsed += delta;

    // If no active transition, evaluate outgoing transitions before advancing
    // so a transition triggered this tick receives the full delta of progress.
    if sm.transition.is_none() {
        if !sm.travel_queue.is_empty() {
            // Travel: take the next queued hop regardless of transition
            // conditions, using the declared crossfade for that edge (if any).
            let next = sm.travel_queue.remove(0);
            let crossfade = sm
                .transitions
                .iter()
                .find(|t| t.from == sm.current && t.to == next)
                .map(|t| t.crossfade)
                .unwrap_or(0.0);
            if let Some(from_node) = sm.states.get(&sm.current).cloned() {
                let prev_state = std::mem::replace(&mut sm.current, next);
                if let Some(node) = sm.states.get_mut(&sm.current) {
                    reset_node_time(node);
                }
                if crossfade > 0.0 {
                    sm.transition = Some(ActiveTransition {
                        from_state: prev_state,
                        from_node: Box::new(from_node),
                        elapsed: 0.0,
                        duration: crossfade,
                    });
                }
            }
        } else {
            let triggered = sm
                .transitions
                .iter()
                .find(|t| t.from == sm.current && params.condition(&t.condition))
                .cloned();

            if let Some(t) = triggered {
                if let Some(from_node) = sm.states.get(&sm.current).cloned() {
                    let prev_state = std::mem::replace(&mut sm.current, t.to.clone());
                    // Reset the incoming state to start from zero.
                    if let Some(node) = sm.states.get_mut(&sm.current) {
                        reset_node_time(node);
                    }
                    if t.crossfade > 0.0 {
                        sm.transition = Some(ActiveTransition {
                            from_state: prev_state,
                            from_node: Box::new(from_node),
                            elapsed: 0.0,
                            duration: t.crossfade,
                        });
                    }
                }
            }
        }
    }

    // Advance active transition if any.
    if let Some(mut t) = sm.transition.take() {
        t.elapsed += delta;
        advance_node(&mut t.from_node, delta, library, params);
        if t.elapsed < t.duration {
            sm.transition = Some(t);
        }
    }

    // Advance the current state's node.
    if let Some(node) = sm.states.get_mut(&sm.current) {
        advance_node(node, delta, library, params);
    }
}

fn reset_node_time(node: &mut AnimationNode) {
    match node {
        AnimationNode::Animation { position, .. } => *position = 0.0,
        AnimationNode::Blend2 { a, b, .. } => {
            reset_node_time(a);
            reset_node_time(b);
        }
        AnimationNode::Blend3 { a, b, c, .. } => {
            reset_node_time(a);
            reset_node_time(b);
            reset_node_time(c);
        }
        AnimationNode::OneShot(one_shot) => {
            reset_node_time(&mut one_shot.base);
            reset_node_time(&mut one_shot.shot);
            one_shot.elapsed = 0.0;
            one_shot.active = false;
            one_shot.prev_trigger = false;
        }
        AnimationNode::StateMachine(sm) => {
            sm.elapsed = 0.0;
            sm.transition = None;
        }
        AnimationNode::Named { inner, .. } => reset_node_time(inner),
        AnimationNode::BlendSpace1D(_) | AnimationNode::BlendSpace2D(_) => {}
    }
}

fn advance_one_shot(
    one_shot: &mut OneShot,
    delta: f64,
    library: &HashMap<String, Animation>,
    params: EvalParams<'_>,
) {
    let trigger = if one_shot.trigger_param.is_empty() {
        false
    } else {
        params.condition(&one_shot.trigger_param)
    };

    if !one_shot.active && trigger && !one_shot.prev_trigger {
        one_shot.active = true;
        one_shot.elapsed = 0.0;
        reset_node_time(&mut one_shot.shot);
    }
    one_shot.prev_trigger = trigger;

    advance_node(&mut one_shot.base, delta, library, params);
    if one_shot.active {
        one_shot.elapsed += delta;
        advance_node(&mut one_shot.shot, delta, library, params);
        if one_shot.elapsed >= one_shot.total_duration() {
            one_shot.active = false;
            one_shot.elapsed = 0.0;
        }
    }
}

fn sample_node(
    node: &AnimationNode,
    library: &HashMap<String, Animation>,
) -> Vec<(TrackKey, Variant)> {
    match node {
        AnimationNode::Animation { name, position } => match library.get(name) {
            Some(anim) => sample_clip_targeted(anim, *position),
            None => Vec::new(),
        },
        AnimationNode::Blend2 { a, b, weight } => {
            let va = sample_node(a, library);
            let vb = sample_node(b, library);
            blend_value_lists(&va, &vb, *weight)
        }
        AnimationNode::Blend3 { a, b, c, weight } => {
            let w = weight.clamp(-1.0, 1.0);
            let vb = sample_node(b, library);
            if w < 0.0 {
                let va = sample_node(a, library);
                blend_value_lists(&va, &vb, 1.0 + w)
            } else if w > 0.0 {
                let vc = sample_node(c, library);
                blend_value_lists(&vb, &vc, w)
            } else {
                vb
            }
        }
        AnimationNode::OneShot(one_shot) => {
            let base = sample_node(&one_shot.base, library);
            if !one_shot.active {
                return base;
            }
            let shot = sample_node(&one_shot.shot, library);
            blend_value_lists(&base, &shot, one_shot.blend_factor())
        }
        AnimationNode::BlendSpace1D(space) => sample_blend_space_1d(space, library),
        AnimationNode::BlendSpace2D(space) => sample_blend_space_2d(space, library),
        AnimationNode::StateMachine(sm) => sample_state_machine(sm, library),
        // A name is metadata for parameter addressing only — it does not change
        // what the wrapped node produces.
        AnimationNode::Named { inner, .. } => sample_node(inner, library),
    }
}

fn sample_state_machine(
    sm: &StateMachine,
    library: &HashMap<String, Animation>,
) -> Vec<(TrackKey, Variant)> {
    let current = match sm.states.get(&sm.current) {
        Some(n) => n,
        None => return Vec::new(),
    };
    let to_values = sample_node(current, library);

    let t = match &sm.transition {
        Some(t) => t,
        None => return to_values,
    };
    if t.duration <= 0.0 {
        return to_values;
    }
    let from_values = sample_node(&t.from_node, library);
    let factor = (t.elapsed / t.duration).clamp(0.0, 1.0) as f32;
    blend_value_lists(&from_values, &to_values, factor)
}

fn sample_blend_space_1d(
    space: &BlendSpace1D,
    library: &HashMap<String, Animation>,
) -> Vec<(TrackKey, Variant)> {
    if space.points.is_empty() {
        return Vec::new();
    }
    let x = space.blend_position;

    // Clamp to ends — the single active clip still plays over time.
    if x <= space.points[0].0 {
        return sample_clip_at(library, &space.points[0].1, space.elapsed);
    }
    if x >= space.points[space.points.len() - 1].0 {
        return sample_clip_at(
            library,
            &space.points[space.points.len() - 1].1,
            space.elapsed,
        );
    }

    // Find the bracketing pair.
    for win in space.points.windows(2) {
        let (ax, a_name) = &win[0];
        let (bx, b_name) = &win[1];
        if x >= *ax && x <= *bx {
            let span = (bx - ax).max(f32::EPSILON);
            let t = (x - ax) / span;
            let anim_a = match library.get(a_name) {
                Some(a) => a,
                None => return Vec::new(),
            };
            let anim_b = match library.get(b_name) {
                Some(a) => a,
                None => return Vec::new(),
            };
            // Sample each clip at the blend space's shared playback clock,
            // looped to that clip's own length, so the blended clips play over
            // time rather than being frozen at t=0.
            let ta = space.elapsed.rem_euclid(anim_a.length.max(f64::EPSILON));
            let tb = space.elapsed.rem_euclid(anim_b.length.max(f64::EPSILON));
            return blend_clips_targeted(anim_a, anim_b, ta, tb, t);
        }
    }
    Vec::new()
}

fn sample_blend_space_2d(
    space: &BlendSpace2D,
    library: &HashMap<String, Animation>,
) -> Vec<(TrackKey, Variant)> {
    if space.points.is_empty() {
        return Vec::new();
    }
    // Bilinear interpolation over an axis-aligned grid of four points; fall
    // back to nearest neighbor otherwise.
    if space.points.len() == 4 {
        if let Some(values) = bilinear_sample(space, library) {
            return values;
        }
    }
    nearest_point(space, library)
}

fn bilinear_sample(
    space: &BlendSpace2D,
    library: &HashMap<String, Animation>,
) -> Option<Vec<(TrackKey, Variant)>> {
    let mut xs: Vec<f32> = space.points.iter().map(|(p, _)| p.x).collect();
    xs.sort_by(|a, b| a.partial_cmp(b).unwrap());
    xs.dedup();
    let mut ys: Vec<f32> = space.points.iter().map(|(p, _)| p.y).collect();
    ys.sort_by(|a, b| a.partial_cmp(b).unwrap());
    ys.dedup();
    if xs.len() != 2 || ys.len() != 2 {
        return None;
    }
    let (x0, x1) = (xs[0], xs[1]);
    let (y0, y1) = (ys[0], ys[1]);

    let find = |px: f32, py: f32| -> Option<&str> {
        space
            .points
            .iter()
            .find(|(p, _)| (p.x - px).abs() < 1e-5 && (p.y - py).abs() < 1e-5)
            .map(|(_, n)| n.as_str())
    };
    let n00 = find(x0, y0)?;
    let n10 = find(x1, y0)?;
    let n01 = find(x0, y1)?;
    let n11 = find(x1, y1)?;

    let bx = ((space.blend_position.x - x0) / (x1 - x0)).clamp(0.0, 1.0);
    let by = ((space.blend_position.y - y0) / (y1 - y0)).clamp(0.0, 1.0);

    let a00 = library.get(n00)?;
    let a10 = library.get(n10)?;
    let a01 = library.get(n01)?;
    let a11 = library.get(n11)?;

    let bottom = blend_clips_targeted(a00, a10, 0.0, 0.0, bx);
    let top = blend_clips_targeted(a01, a11, 0.0, 0.0, bx);
    Some(blend_value_lists(&bottom, &top, by))
}

fn nearest_point(
    space: &BlendSpace2D,
    library: &HashMap<String, Animation>,
) -> Vec<(TrackKey, Variant)> {
    let mut best: Option<(&str, f32)> = None;
    for (pos, name) in &space.points {
        let d = (*pos - space.blend_position).length_squared();
        if best.map(|(_, bd)| d < bd).unwrap_or(true) {
            best = Some((name.as_str(), d));
        }
    }
    match best.and_then(|(n, _)| library.get(n)) {
        Some(anim) => sample_clip_targeted(anim, 0.0),
        None => Vec::new(),
    }
}

/// Samples every track of `anim` at `time`, keyed by `(node_path, property)` so
/// blending pairs values per target node and the caller can resolve each one.
fn sample_clip_targeted(anim: &Animation, time: f64) -> Vec<(TrackKey, Variant)> {
    anim.sample_all_targeted(time)
        .into_iter()
        .map(|(node_path, prop, value)| ((node_path, prop), value))
        .collect()
}

/// Blends two clips sampled at their own times, weighting `b` by `weight`.
/// The targeted counterpart of [`crate::animation::blend_animations`].
fn blend_clips_targeted(
    a: &Animation,
    b: &Animation,
    ta: f64,
    tb: f64,
    weight: f32,
) -> Vec<(TrackKey, Variant)> {
    let va = sample_clip_targeted(a, ta);
    let vb = sample_clip_targeted(b, tb);
    blend_value_lists(&va, &vb, weight)
}

/// Samples a single clip from `library` at `elapsed` seconds, looped to the
/// clip's own length. Used by blend spaces so a clamped (single-clip) position
/// still plays over time rather than being frozen at t=0.
fn sample_clip_at(
    library: &HashMap<String, Animation>,
    name: &str,
    elapsed: f64,
) -> Vec<(TrackKey, Variant)> {
    match library.get(name) {
        Some(a) => sample_clip_targeted(a, elapsed.rem_euclid(a.length.max(f64::EPSILON))),
        None => Vec::new(),
    }
}

fn blend_value_lists(
    a: &[(TrackKey, Variant)],
    b: &[(TrackKey, Variant)],
    weight: f32,
) -> Vec<(TrackKey, Variant)> {
    let w = weight.clamp(0.0, 1.0);
    if w <= 0.0 {
        return a.to_vec();
    }
    if w >= 1.0 {
        return b.to_vec();
    }
    let b_map: HashMap<&TrackKey, &Variant> = b.iter().map(|(k, v)| (k, v)).collect();
    let mut seen = std::collections::HashSet::new();
    let mut result = Vec::with_capacity(a.len().max(b.len()));
    for (prop, va) in a {
        seen.insert(prop);
        if let Some(vb) = b_map.get(prop) {
            let blended = interpolate_variant(va, vb, w).unwrap_or_else(|| {
                if w < 0.5 {
                    va.clone()
                } else {
                    (*vb).clone()
                }
            });
            result.push((prop.clone(), blended));
        } else {
            result.push((prop.clone(), va.clone()));
        }
    }
    for (prop, vb) in b {
        if !seen.contains(prop) {
            result.push((prop.clone(), vb.clone()));
        }
    }
    result
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::animation::{AnimationTrack, KeyFrame};

    fn simple_anim(name: &str, value: f64) -> Animation {
        let mut anim = Animation::new(name, 1.0);
        let mut track = AnimationTrack::new("x");
        track.add_keyframe(KeyFrame::linear(0.0, Variant::Float(value)));
        track.add_keyframe(KeyFrame::linear(1.0, Variant::Float(value)));
        anim.tracks.push(track);
        anim
    }

    fn float_of(values: &[(String, Variant)], prop: &str) -> f64 {
        for (p, v) in values {
            if p == prop {
                if let Variant::Float(f) = v {
                    return *f;
                }
            }
        }
        panic!("no float for {prop}");
    }

    #[test]
    fn tree_empty_produces_no_values() {
        let tree = AnimationTree::new();
        assert!(tree.get_current_values().is_empty());
    }

    #[test]
    fn tree_animation_leaf_samples_library_clip() {
        let mut tree = AnimationTree::new();
        tree.add_animation(simple_anim("idle", 3.0));
        tree.set_root(AnimationNode::Animation {
            name: "idle".into(),
            position: 0.0,
        });
        assert_eq!(float_of(&tree.get_current_values(), "x"), 3.0);
    }

    fn ramp_anim(name: &str, v0: f64, v1: f64) -> Animation {
        let mut anim = Animation::new(name, 1.0);
        let mut track = AnimationTrack::new("x");
        track.add_keyframe(KeyFrame::linear(0.0, Variant::Float(v0)));
        track.add_keyframe(KeyFrame::linear(1.0, Variant::Float(v1)));
        anim.tracks.push(track);
        anim
    }

    #[test]
    fn blend_space_1d_blends_by_position_and_plays_over_time() {
        let mut tree = AnimationTree::new();
        // Two ramp clips: A goes 0->10, B goes 100->110 over 1s.
        tree.add_animation(ramp_anim("a", 0.0, 10.0));
        tree.add_animation(ramp_anim("b", 100.0, 110.0));

        let mut bs = BlendSpace1D::new();
        bs.add_point(0.0, "a");
        bs.add_point(1.0, "b");
        bs.blend_position = 0.5; // midway between the two clips
        tree.set_root(AnimationNode::BlendSpace1D(bs));

        // Blend by position: midway between clip A (start 0) and clip B (start 100).
        let v0 = float_of(&tree.get_current_values(), "x");
        assert!(
            (40.0..=60.0).contains(&v0),
            "blend-by-position at start: v0={v0}"
        );

        // Over time: advancing the tree plays both clips forward (they ramp up),
        // so the blended value increases — previously it was frozen at t=0.
        tree.advance(0.5);
        let v_mid = float_of(&tree.get_current_values(), "x");
        assert!(
            v_mid > v0 + 1.0,
            "blend-space clips must play over time: v0={v0} v_mid={v_mid}"
        );
    }

    #[test]
    fn state_machine_drives_node_property_over_time() {
        use crate::node::Node;

        let mut scene = SceneTree::new();
        let root = scene.root_id();
        let node = scene.add_child(root, Node::new("N", "Node")).unwrap();

        // A state machine whose current state plays a clip that ramps "x" 0->42.
        let mut tree = AnimationTree::new();
        tree.add_animation(ramp_anim("run", 0.0, 42.0));
        let mut sm = StateMachine::new();
        sm.states.insert(
            "run".into(),
            AnimationNode::Animation {
                name: "run".into(),
                position: 0.0,
            },
        );
        sm.current = "run".into();
        tree.set_root(AnimationNode::StateMachine(sm));

        // Drive the tree at runtime and apply its pose to the node.
        advance_and_apply(&mut tree, &mut scene, node, 0.5);

        // The state machine drove the node's "x" property over time
        // (0 -> 42 over 1s, so ~21 at 0.5s).
        match scene.get_node(node).unwrap().get_property("x") {
            Variant::Float(x) => assert!((x - 21.0).abs() < 1.0, "x={x}"),
            other => panic!("x not driven by AnimationTree: {other:?}"),
        }
    }

    fn anim_state(name: &str) -> AnimationNode {
        AnimationNode::Animation {
            name: name.into(),
            position: 0.0,
        }
    }

    fn edge(from: &str, to: &str) -> StateTransition {
        StateTransition {
            from: from.into(),
            to: to.into(),
            condition: String::new(),
            crossfade: 0.0,
        }
    }

    #[test]
    fn travel_path_finds_shortest_route() {
        let mut sm = StateMachine::new();
        for s in ["idle", "walk", "run", "jump"] {
            sm.states.insert(s.into(), anim_state(s));
        }
        sm.current = "idle".into();
        // idle -> walk -> run ; idle -> jump ; (no direct idle -> run)
        sm.transitions.push(edge("idle", "walk"));
        sm.transitions.push(edge("walk", "run"));
        sm.transitions.push(edge("idle", "jump"));

        assert_eq!(
            sm.travel_path("run"),
            Some(vec!["walk".to_string(), "run".to_string()])
        );
        assert_eq!(sm.travel_path("jump"), Some(vec!["jump".to_string()]));
        assert_eq!(sm.travel_path("idle"), Some(vec![])); // already there
        assert_eq!(sm.travel_path("nope"), None); // unknown state
    }

    #[test]
    fn travel_unreachable_returns_none() {
        let mut sm = StateMachine::new();
        for s in ["a", "b", "island"] {
            sm.states.insert(s.into(), anim_state(s));
        }
        sm.current = "a".into();
        sm.transitions.push(edge("a", "b"));
        // "island" has no inbound transition.
        assert_eq!(sm.travel_path("island"), None);
        assert!(!sm.travel("island"));
        assert!(!sm.is_travelling());
    }

    #[test]
    fn travel_walks_the_graph_path_via_advance() {
        // idle -> walk -> run with instant hops; travel("run") reaches "run" by
        // walking through "walk" across successive advance() calls.
        let mut tree = AnimationTree::new();
        for s in ["idle", "walk", "run"] {
            tree.add_animation(ramp_anim(s, 0.0, 1.0));
        }
        let mut sm = StateMachine::new();
        for s in ["idle", "walk", "run"] {
            sm.states.insert(s.into(), anim_state(s));
        }
        sm.current = "idle".into();
        sm.transitions.push(edge("idle", "walk"));
        sm.transitions.push(edge("walk", "run"));

        assert!(sm.travel("run"));
        assert!(sm.is_travelling());
        tree.set_root(AnimationNode::StateMachine(sm));

        tree.advance(0.1); // idle -> walk
        tree.advance(0.1); // walk -> run

        match &tree.root {
            Some(AnimationNode::StateMachine(sm)) => {
                assert_eq!(sm.current, "run");
                assert!(!sm.is_travelling(), "travel queue drained on arrival");
            }
            _ => panic!("root is not a state machine"),
        }
    }

    // -- Godot-style `parameters/...` Variant parameter map -----------------

    #[test]
    fn param_map_set_get_float() {
        let mut tree = AnimationTree::new();
        tree.set_parameter("parameters/blend/blend_amount", Variant::Float(0.25));
        let f = tree
            .get_parameter_float("parameters/blend/blend_amount")
            .expect("float param present");
        assert!((f - 0.25).abs() < 1e-6, "f={f}");
        assert_eq!(
            tree.get_parameter("parameters/blend/blend_amount"),
            Some(Variant::Float(0.25))
        );
    }

    #[test]
    fn param_map_set_get_vector2() {
        let mut tree = AnimationTree::new();
        let pos = Vector2::new(0.5, -0.25);
        tree.set_parameter("parameters/blend_position", Variant::Vector2(pos));
        assert_eq!(
            tree.get_parameter_vector2("parameters/blend_position"),
            Some(pos)
        );
        assert_eq!(
            tree.get_parameter("parameters/blend_position"),
            Some(Variant::Vector2(pos))
        );
    }

    #[test]
    fn param_map_int_and_type_mismatch() {
        let mut tree = AnimationTree::new();
        tree.set_parameter("parameters/OneShot/request", Variant::Int(3));
        assert_eq!(
            tree.get_parameter_int("parameters/OneShot/request"),
            Some(3)
        );
        // Wrong-type accessor on an Int param returns None.
        assert_eq!(
            tree.get_parameter_vector2("parameters/OneShot/request"),
            None
        );
    }

    #[test]
    fn param_map_missing_returns_none() {
        let tree = AnimationTree::new();
        assert_eq!(tree.get_parameter("nope"), None);
        assert_eq!(tree.get_parameter_float("nope"), None);
        assert_eq!(tree.get_parameter_vector2("nope"), None);
        assert_eq!(tree.get_parameter_int("nope"), None);
    }

    #[test]
    fn param_map_float_getter_accepts_int() {
        let mut tree = AnimationTree::new();
        tree.set_parameter("parameters/count", Variant::Int(2));
        let f = tree
            .get_parameter_float("parameters/count")
            .expect("int coerces to float");
        assert!((f - 2.0).abs() < 1e-6, "f={f}");
    }

    // -- the `parameters/...` map drives evaluation (pat-0q2kx.2) ------------

    fn state_of(tree: &AnimationTree) -> String {
        match &tree.root {
            Some(AnimationNode::StateMachine(sm)) => sm.current.clone(),
            other => panic!("root is not a state machine: {other:?}"),
        }
    }

    fn two_state_machine() -> AnimationTree {
        let mut tree = AnimationTree::new();
        tree.add_animation(ramp_anim("idle", 0.0, 1.0));
        tree.add_animation(ramp_anim("run", 0.0, 100.0));
        let mut sm = StateMachine::new();
        sm.states.insert("idle".into(), anim_state("idle"));
        sm.states.insert("run".into(), anim_state("run"));
        sm.current = "idle".into();
        sm.transitions.push(StateTransition {
            from: "idle".into(),
            to: "run".into(),
            condition: "run".into(),
            crossfade: 0.0,
        });
        tree.set_root(AnimationNode::StateMachine(sm));
        tree
    }

    #[test]
    fn condition_parameter_drives_state_transition() {
        let mut tree = two_state_machine();

        // Unset condition: the machine holds its state.
        tree.advance(0.1);
        assert_eq!(state_of(&tree), "idle");

        // Godot addresses a transition's `advance_condition` as the tree
        // parameter `parameters/conditions/<name>`.
        tree.set_parameter("parameters/conditions/run", Variant::Bool(true));
        tree.advance(0.1);
        assert_eq!(state_of(&tree), "run", "condition parameter fires the edge");
    }

    #[test]
    fn condition_parameter_wins_over_stale_bool_map() {
        let mut tree = two_state_machine();
        // A hand-built tree may have set the legacy boolean map; the Godot-style
        // parameter is authoritative when both are present.
        tree.set_param("run", false);
        tree.set_parameter("parameters/conditions/run", Variant::Bool(true));
        tree.advance(0.1);
        assert_eq!(state_of(&tree), "run");
    }

    #[test]
    fn legacy_bool_param_still_drives_transition() {
        let mut tree = two_state_machine();
        tree.set_param("run", true);
        tree.advance(0.1);
        assert_eq!(state_of(&tree), "run", "set_param must keep working");
    }

    #[test]
    fn named_blend_position_parameter_drives_blend_space() {
        let mut tree = AnimationTree::new();
        tree.add_animation(simple_anim("a", 0.0));
        tree.add_animation(simple_anim("b", 100.0));
        let mut bs = BlendSpace1D::new();
        bs.add_point(0.0, "a");
        bs.add_point(1.0, "b");
        tree.set_root(AnimationNode::Named {
            name: "space".into(),
            inner: Box::new(AnimationNode::BlendSpace1D(bs)),
        });

        // Position 0 (the default) samples clip "a" alone.
        tree.advance(0.1);
        assert!(float_of(&tree.get_current_values(), "x") < 1.0);

        // `parameters/<node>/blend_position` moves it fully onto clip "b".
        tree.set_parameter("parameters/space/blend_position", Variant::Float(1.0));
        tree.advance(0.1);
        let x = float_of(&tree.get_current_values(), "x");
        assert!((x - 100.0).abs() < 1e-3, "blend position ignored: x={x}");
    }

    #[test]
    fn root_blend_position_parameter_has_no_node_prefix() {
        // Godot's root graph node is unnamed, so its parameters live directly
        // under `parameters/` — here `parameters/blend_position`.
        let mut tree = AnimationTree::new();
        tree.add_animation(simple_anim("a", 0.0));
        tree.add_animation(simple_anim("b", 100.0));
        let mut bs = BlendSpace1D::new();
        bs.add_point(0.0, "a");
        bs.add_point(1.0, "b");
        tree.set_root(AnimationNode::BlendSpace1D(bs));

        tree.set_parameter("parameters/blend_position", Variant::Float(1.0));
        tree.advance(0.1);
        let x = float_of(&tree.get_current_values(), "x");
        assert!(
            (x - 100.0).abs() < 1e-3,
            "root blend position ignored: x={x}"
        );
    }

    #[test]
    fn one_shot_request_parameter_fires_once() {
        let mut tree = AnimationTree::new();
        tree.add_animation(simple_anim("base", 0.0));
        tree.add_animation(simple_anim("hit", 50.0));
        let one_shot = OneShot::new(
            AnimationNode::Animation {
                name: "base".into(),
                position: 0.0,
            },
            AnimationNode::Animation {
                name: "hit".into(),
                position: 0.0,
            },
        )
        .with_shot_length(0.5);
        tree.set_root(AnimationNode::Named {
            name: "OneShot".into(),
            inner: Box::new(AnimationNode::OneShot(one_shot)),
        });

        // Idle: only the base pose.
        tree.advance(0.1);
        assert!(float_of(&tree.get_current_values(), "x").abs() < 1e-3);

        // ONE_SHOT_REQUEST_FIRE starts the shot, which then owns the pose.
        tree.set_parameter(
            "parameters/OneShot/request",
            Variant::Int(ONE_SHOT_REQUEST_FIRE),
        );
        tree.advance(0.1);
        let x = float_of(&tree.get_current_values(), "x");
        assert!((x - 50.0).abs() < 1e-3, "one-shot request ignored: x={x}");

        // The request latches: once the shot has run its course it does not
        // restart while the same request value is still in the map.
        tree.advance(0.5);
        tree.advance(0.1);
        let x = float_of(&tree.get_current_values(), "x");
        assert!(
            x.abs() < 1e-3,
            "one-shot re-fired from a stale request: x={x}"
        );
    }

    #[test]
    fn one_shot_abort_request_stops_the_shot() {
        let mut tree = AnimationTree::new();
        tree.add_animation(simple_anim("base", 0.0));
        tree.add_animation(simple_anim("hit", 50.0));
        tree.set_root(AnimationNode::Named {
            name: "OneShot".into(),
            inner: Box::new(AnimationNode::OneShot(
                OneShot::new(
                    AnimationNode::Animation {
                        name: "base".into(),
                        position: 0.0,
                    },
                    AnimationNode::Animation {
                        name: "hit".into(),
                        position: 0.0,
                    },
                )
                .with_shot_length(10.0),
            )),
        });
        tree.set_parameter(
            "parameters/OneShot/request",
            Variant::Int(ONE_SHOT_REQUEST_FIRE),
        );
        tree.advance(0.1);
        assert!((float_of(&tree.get_current_values(), "x") - 50.0).abs() < 1e-3);

        tree.set_parameter(
            "parameters/OneShot/request",
            Variant::Int(ONE_SHOT_REQUEST_ABORT),
        );
        tree.advance(0.1);
        assert!(
            float_of(&tree.get_current_values(), "x").abs() < 1e-3,
            "abort must drop back to the base pose"
        );
    }

    #[test]
    fn targeted_values_keep_each_track_node_path() {
        let mut anim = Animation::new("walk", 1.0);
        let mut track = AnimationTrack::with_node("Sprite", "x");
        track.add_keyframe(KeyFrame::linear(0.0, Variant::Float(7.0)));
        track.add_keyframe(KeyFrame::linear(1.0, Variant::Float(7.0)));
        anim.tracks.push(track);

        let mut tree = AnimationTree::new();
        tree.add_animation(anim);
        tree.set_root(AnimationNode::Animation {
            name: "walk".into(),
            position: 0.0,
        });

        let targeted = tree.get_current_values_targeted();
        assert_eq!(
            targeted,
            vec![("Sprite".to_string(), "x".to_string(), Variant::Float(7.0))],
            "a track's node path must survive sampling"
        );
        // The property-only view stays as it was for existing callers.
        assert_eq!(
            tree.get_current_values(),
            vec![("x".to_string(), Variant::Float(7.0))]
        );
    }
}
