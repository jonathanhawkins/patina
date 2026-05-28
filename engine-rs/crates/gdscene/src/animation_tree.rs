//! AnimationTree — graph-based animation evaluation.
//!
//! Mirrors Godot's [`AnimationTree`] system: a tree of [`AnimationNode`]s
//! produces sampled property values from an [`Animation`] library. Supported
//! node types are [`AnimationNode::Animation`], [`AnimationNode::Blend2`],
//! [`AnimationNode::Blend3`], [`AnimationNode::OneShot`],
//! [`AnimationNode::BlendSpace1D`], [`AnimationNode::BlendSpace2D`], and
//! [`AnimationNode::StateMachine`].

use std::collections::HashMap;

use gdcore::math::Vector2;
use gdvariant::Variant;

use crate::animation::{blend_animations, interpolate_variant, Animation};

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
}

/// A 1D blend space. Points are sorted by `position` on insert.
#[derive(Debug, Clone, Default)]
pub struct BlendSpace1D {
    points: Vec<(f32, String)>,
    pub blend_position: f32,
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
}

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
    pub active: bool,
}

impl AnimationTree {
    pub fn new() -> Self {
        Self {
            animations: HashMap::new(),
            root: None,
            parameters: HashMap::new(),
            active: true,
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

    /// Advances all time-dependent nodes by `delta` seconds.
    pub fn advance(&mut self, delta: f64) {
        if !self.active || delta == 0.0 {
            return;
        }
        let mut root = match self.root.take() {
            Some(r) => r,
            None => return,
        };
        advance_node(&mut root, delta, &self.animations, &self.parameters);
        self.root = Some(root);
    }

    /// Samples the current values of the graph.
    pub fn get_current_values(&self) -> Vec<(String, Variant)> {
        match &self.root {
            Some(root) => sample_node(root, &self.animations),
            None => Vec::new(),
        }
    }
}

impl Default for AnimationTree {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Evaluation
// ---------------------------------------------------------------------------

fn advance_node(
    node: &mut AnimationNode,
    delta: f64,
    library: &HashMap<String, Animation>,
    params: &HashMap<String, bool>,
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
        AnimationNode::BlendSpace1D(_) | AnimationNode::BlendSpace2D(_) => {
            // Blend spaces are stateless — their animations loop based on a
            // shared clock driven by the tree. We intentionally do not track
            // per-point time here; sampling uses the tree-level elapsed value
            // stored on the nodes' underlying Animation library (loops).
        }
        AnimationNode::StateMachine(sm) => advance_state_machine(sm, delta, library, params),
    }
}

fn advance_state_machine(
    sm: &mut StateMachine,
    delta: f64,
    library: &HashMap<String, Animation>,
    params: &HashMap<String, bool>,
) {
    sm.elapsed += delta;

    // If no active transition, evaluate outgoing transitions before advancing
    // so a transition triggered this tick receives the full delta of progress.
    if sm.transition.is_none() {
        let triggered = sm
            .transitions
            .iter()
            .find(|t| t.from == sm.current && params.get(&t.condition).copied().unwrap_or(false))
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
        AnimationNode::BlendSpace1D(_) | AnimationNode::BlendSpace2D(_) => {}
    }
}

fn advance_one_shot(
    one_shot: &mut OneShot,
    delta: f64,
    library: &HashMap<String, Animation>,
    params: &HashMap<String, bool>,
) {
    let trigger = if one_shot.trigger_param.is_empty() {
        false
    } else {
        params.get(&one_shot.trigger_param).copied().unwrap_or(false)
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
) -> Vec<(String, Variant)> {
    match node {
        AnimationNode::Animation { name, position } => match library.get(name) {
            Some(anim) => anim.sample_all(*position),
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
    }
}

fn sample_state_machine(
    sm: &StateMachine,
    library: &HashMap<String, Animation>,
) -> Vec<(String, Variant)> {
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
) -> Vec<(String, Variant)> {
    if space.points.is_empty() {
        return Vec::new();
    }
    let x = space.blend_position;

    // Clamp to ends.
    if x <= space.points[0].0 {
        return sample_library(library, &space.points[0].1);
    }
    if x >= space.points[space.points.len() - 1].0 {
        return sample_library(library, &space.points[space.points.len() - 1].1);
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
            // Use position 0.0 for both — blend spaces sample the clips' t=0
            // pose unless the tree has an external clock. This matches the
            // minimal parity surface; higher-fidelity phasing is future work.
            return blend_animations(anim_a, anim_b, 0.0, 0.0, t);
        }
    }
    Vec::new()
}

fn sample_blend_space_2d(
    space: &BlendSpace2D,
    library: &HashMap<String, Animation>,
) -> Vec<(String, Variant)> {
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
) -> Option<Vec<(String, Variant)>> {
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

    let bottom = blend_animations(a00, a10, 0.0, 0.0, bx);
    let top = blend_animations(a01, a11, 0.0, 0.0, bx);
    Some(blend_value_lists(&bottom, &top, by))
}

fn nearest_point(
    space: &BlendSpace2D,
    library: &HashMap<String, Animation>,
) -> Vec<(String, Variant)> {
    let mut best: Option<(&str, f32)> = None;
    for (pos, name) in &space.points {
        let d = (*pos - space.blend_position).length_squared();
        if best.map(|(_, bd)| d < bd).unwrap_or(true) {
            best = Some((name.as_str(), d));
        }
    }
    match best.and_then(|(n, _)| library.get(n)) {
        Some(anim) => anim.sample_all(0.0),
        None => Vec::new(),
    }
}

fn sample_library(library: &HashMap<String, Animation>, name: &str) -> Vec<(String, Variant)> {
    match library.get(name) {
        Some(a) => a.sample_all(0.0),
        None => Vec::new(),
    }
}

fn blend_value_lists(
    a: &[(String, Variant)],
    b: &[(String, Variant)],
    weight: f32,
) -> Vec<(String, Variant)> {
    let w = weight.clamp(0.0, 1.0);
    if w <= 0.0 {
        return a.to_vec();
    }
    if w >= 1.0 {
        return b.to_vec();
    }
    let b_map: HashMap<&str, &Variant> = b.iter().map(|(k, v)| (k.as_str(), v)).collect();
    let mut seen = std::collections::HashSet::new();
    let mut result = Vec::with_capacity(a.len().max(b.len()));
    for (prop, va) in a {
        seen.insert(prop.as_str());
        if let Some(vb) = b_map.get(prop.as_str()) {
            let blended = interpolate_variant(va, vb, w)
                .unwrap_or_else(|| if w < 0.5 { va.clone() } else { (*vb).clone() });
            result.push((prop.clone(), blended));
        } else {
            result.push((prop.clone(), va.clone()));
        }
    }
    for (prop, vb) in b {
        if !seen.contains(prop.as_str()) {
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
}
