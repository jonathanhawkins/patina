//! Build [`AnimationTree`] graphs out of parsed `.tscn` data (pat-0q2kx.2).
//!
//! Godot stores an animation graph as a chain of resources: an `AnimationTree`
//! node points at its `tree_root` (an `AnimationNodeStateMachine`,
//! `AnimationNodeBlendTree`, `AnimationNodeBlendSpace1D/2D`, …), each of which
//! names its children through further `SubResource` references
//! (`states/<name>/node`, `blend_point_N/node`, `nodes/<name>/node` +
//! `node_connections`). The tree resolves clips by name against the
//! `AnimationPlayer` at `anim_player`, and scripts steer it by setting
//! `parameters/...` entries
//! (upstream/godot/scene/animation/animation_node_state_machine.cpp,
//! animation_blend_tree.cpp, animation_blend_space_1d/2d.cpp).
//!
//! Patina already had the graph evaluator ([`crate::animation_tree`]) and now has
//! the runtime store ([`SceneTree::attach_animation_tree`] /
//! [`SceneTree::process_animation_trees`]); this module is the missing step that
//! turns a loaded scene's `AnimationTree` node into a running graph.
//! [`attach_scene_animation_trees`] runs from the scene-instancing helpers in
//! [`crate::packed_scene`], right after
//! [`crate::animation_tscn::attach_scene_animation_players`] so the clip library
//! is already available.
//!
//! Supported `tree_root` types: `AnimationNodeAnimation`,
//! `AnimationNodeStateMachine`, `AnimationNodeBlendSpace1D`,
//! `AnimationNodeBlendSpace2D`, and `AnimationNodeBlendTree` wiring
//! `AnimationNodeBlend2` / `AnimationNodeBlend3` / `AnimationNodeOneShot`. The
//! remaining blend-tree node types (Add2/3, Sub2, TimeScale, TimeSeek,
//! Transition, and track filters) are pat-0q2kx.53; an unsupported node resolves
//! to `None` and its tree is left unattached rather than half-built.

use std::collections::HashMap;

use gdcore::math::Vector2;
use gdvariant::Variant;

use crate::animation::{Animation, LoopMode};
use crate::animation_tree::{
    AnimationLeafOptions, AnimationNode, AnimationPlayMode, AnimationTree, BlendSpace1D,
    BlendSpace2D, BlendSpaceBlendMode, OneShot, OneShotMixMode, StateMachine, StateTransition,
    SwitchMode, TransitionAdvanceMode,
};
use crate::node::NodeId;
use crate::packed_scene::PackedScene;
use crate::scene_tree::SceneTree;

/// Godot's implicit state-machine entry pseudo-state. A transition out of it
/// selects the starting state rather than describing a real edge.
const START_STATE: &str = "Start";
/// Godot's implicit state-machine exit pseudo-state.
const END_STATE: &str = "End";
/// The implicit output node of an `AnimationNodeBlendTree`.
const OUTPUT_NODE: &str = "output";

// ---------------------------------------------------------------------------
// Scene wiring
// ---------------------------------------------------------------------------

/// Walks the subtree rooted at `root_id` and attaches an [`AnimationTree`] to
/// every `AnimationTree` node the scene declared, so
/// [`SceneTree::process_animation_trees`] drives it from the next frame.
///
/// Returns how many trees were attached. A node whose `tree_root` is missing or
/// uses a graph node that is not supported yet is skipped (it would animate
/// nothing anyway).
pub fn attach_scene_animation_trees(
    tree: &mut SceneTree,
    root_id: NodeId,
    scene: &PackedScene,
) -> usize {
    let mut subtree = Vec::new();
    tree.collect_subtree_top_down(root_id, &mut subtree);

    // Build first (immutable reads of nodes and their attached players), attach
    // second, so the tree is never borrowed mutably mid-read.
    let mut built = Vec::new();
    for &node_id in &subtree {
        let Some(node) = tree.get_node(node_id) else {
            continue;
        };
        if node.class_name() != "AnimationTree" {
            continue;
        }
        let props: HashMap<String, Variant> = node
            .properties()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        let animations = resolve_clip_library(tree, node_id, &props, scene);
        if let Some(anim_tree) = build_animation_tree(scene, &props, animations) {
            built.push((node_id, anim_tree));
        }
    }

    let attached = built.len();
    for (node_id, anim_tree) in built {
        // The ids come from the walk above, so this cannot fail; a stale id
        // would only mean that tree is skipped.
        let _ = tree.attach_animation_tree(node_id, anim_tree);
    }
    attached
}

/// Resolves the clip library an `AnimationTree` node plays from: the
/// `AnimationPlayer` named by its `anim_player` path.
///
/// Prefers the runtime [`AnimationPlayer`](crate::animation::AnimationPlayer)
/// already attached to that node (built from its `libraries` at scene load), so
/// both nodes share one library. Falls back to building the library from the
/// player node's own properties, which covers a tree loaded without the player
/// wiring having run.
fn resolve_clip_library(
    tree: &SceneTree,
    node_id: NodeId,
    props: &HashMap<String, Variant>,
    scene: &PackedScene,
) -> HashMap<String, Animation> {
    let anim_player = node_path_string(props.get("anim_player"));
    if anim_player.is_empty() {
        return HashMap::new();
    }
    let Some(player_id) = tree.get_node_or_null(node_id, &anim_player) else {
        tracing::debug!("AnimationTree anim_player path '{anim_player}' does not resolve");
        return HashMap::new();
    };
    if let Some(player) = tree.get_animation_player(player_id) {
        return player.animations.clone();
    }
    let player_props: HashMap<String, Variant> = match tree.get_node(player_id) {
        Some(node) => node
            .properties()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect(),
        None => return HashMap::new(),
    };
    crate::animation_tscn::build_animation_player(scene, &player_props)
        .map(|player| player.animations)
        .unwrap_or_default()
}

/// Builds an [`AnimationTree`] from an `AnimationTree` node's `.tscn` properties
/// and a resolved clip library.
///
/// Returns `None` when the node has no `tree_root`, or when that root uses a
/// graph node that is not supported yet — an inert tree is not attached.
pub fn build_animation_tree(
    scene: &PackedScene,
    props: &HashMap<String, Variant>,
    animations: HashMap<String, Animation>,
) -> Option<AnimationTree> {
    let root_id = sub_resource_id(props.get("tree_root"))?;
    let root = build_animation_node(scene, &root_id, &animations)?;

    let mut anim_tree = AnimationTree::new();
    anim_tree.animations = animations;
    anim_tree.set_root(root);

    // Godot `AnimationMixer.active` defaults to true (doc/classes/AnimationMixer.xml).
    anim_tree.active = match props.get("active") {
        Some(Variant::Bool(b)) => *b,
        Some(Variant::Int(i)) => *i != 0,
        _ => true,
    };
    let root_node = node_path_string(props.get("root_node"));
    if !root_node.is_empty() {
        anim_tree.root_node = root_node;
    }
    anim_tree.anim_player = node_path_string(props.get("anim_player"));

    // `parameters/...` node properties are the tree's initial parameter values —
    // conditions, blend amounts, blend positions, one-shot requests. Godot writes
    // every non-default parameter into the scene this way.
    for (key, value) in props {
        if key.starts_with("parameters/") {
            anim_tree.params.insert(key.clone(), value.clone());
        }
    }

    Some(anim_tree)
}

// ---------------------------------------------------------------------------
// Graph node conversion
// ---------------------------------------------------------------------------

/// Converts the inline sub-resource `sub_id` into an [`AnimationNode`].
///
/// `animations` is consulted only for clip lengths (a one-shot's hold time);
/// clips themselves are resolved by name at evaluation time.
pub fn build_animation_node(
    scene: &PackedScene,
    sub_id: &str,
    animations: &HashMap<String, Animation>,
) -> Option<AnimationNode> {
    let class = scene.sub_resource_type(sub_id)?.to_string();
    let empty = HashMap::new();
    let props = scene.sub_resource_props(sub_id).unwrap_or(&empty);

    match class.as_str() {
        "AnimationNodeAnimation" => {
            let name = string_of(props.get("animation"))?;
            // Godot's leaf options (pat-s3rrv.45): the play direction and the
            // custom timeline. Every one of these was dropped at load, so a
            // `play_mode = 1` leaf played forwards and a custom timeline was
            // ignored entirely.
            let mut opts = AnimationLeafOptions::default();
            if let Some(v) = int_of(props.get("play_mode")) {
                opts.play_mode = AnimationPlayMode::from_godot_int(v);
            }
            if let Some(Variant::Bool(b)) = props.get("use_custom_timeline") {
                opts.use_custom_timeline = *b;
            }
            if let Some(v) = float_of(props.get("timeline_length")) {
                opts.timeline_length = v as f64;
            }
            if let Some(Variant::Bool(b)) = props.get("stretch_time_scale") {
                opts.stretch_time_scale = *b;
            }
            if let Some(v) = float_of(props.get("start_offset")) {
                opts.start_offset = v as f64;
            }
            if let Some(v) = int_of(props.get("loop_mode")) {
                opts.loop_mode = loop_mode_from_godot_int(v);
            }
            Some(AnimationNode::Animation {
                name,
                position: 0.0,
                opts,
            })
        }
        "AnimationNodeStateMachine" => build_state_machine(scene, props, animations),
        "AnimationNodeBlendSpace1D" => build_blend_space_1d(scene, props),
        "AnimationNodeBlendSpace2D" => build_blend_space_2d(scene, props),
        "AnimationNodeBlendTree" => build_blend_tree(scene, props, animations),
        other => {
            tracing::debug!(
                "AnimationTree: graph node type '{other}' is not supported yet (pat-0q2kx.53)"
            );
            None
        }
    }
}

/// Builds a [`StateMachine`] from `states/<name>/node` entries and the flat
/// `transitions` array (triples of `from`, `to`, transition sub-resource).
fn build_state_machine(
    scene: &PackedScene,
    props: &HashMap<String, Variant>,
    animations: &HashMap<String, Animation>,
) -> Option<AnimationNode> {
    let mut sm = StateMachine::new();

    // States. Godot's implicit Start/End pseudo-states carry only a `position`,
    // so keying off `/node` skips them naturally.
    let mut names: Vec<&String> = props.keys().collect();
    names.sort(); // deterministic build order
    for key in names {
        let Some(state) = key
            .strip_prefix("states/")
            .and_then(|rest| rest.strip_suffix("/node"))
        else {
            continue;
        };
        let Some(child_id) = sub_resource_id(props.get(key)) else {
            continue;
        };
        match build_animation_node(scene, &child_id, animations) {
            // Wrapping in `Named` makes the state's own parameters addressable as
            // `parameters/<state>/...`, exactly as Godot names them.
            Some(node) => {
                sm.states.insert(state.to_string(), named(state, node));
            }
            None => tracing::debug!("AnimationTree: state '{state}' has an unsupported node"),
        }
    }
    if sm.states.is_empty() {
        return None;
    }

    // Transitions: [from, to, SubResource(transition), ...].
    let mut start_target = None;
    if let Some(Variant::Array(items)) = props.get("transitions") {
        for triple in items.to_vec().chunks(3) {
            if triple.len() < 3 {
                break;
            }
            let (Some(from), Some(to)) = (string_of(Some(&triple[0])), string_of(Some(&triple[1])))
            else {
                continue;
            };
            // A Start edge picks the initial state; an End edge exits a nested
            // machine (pat-0q2kx.53). Neither is a real state-to-state edge.
            if from == START_STATE {
                start_target = Some(to);
                continue;
            }
            if to == END_STATE || from == END_STATE || to == START_STATE {
                continue;
            }
            // Every field of `AnimationNodeStateMachineTransition`
            // (pat-s3rrv.1). Only `xfade_time` and `advance_condition` used to
            // be read, so a scene's switch mode, advance mode, priority and
            // reset flag were all discarded — and `advance_mode` in particular
            // decides whether an edge fires on its own at all.
            let mut edge = StateTransition {
                from,
                to,
                ..Default::default()
            };
            if let Some(t_props) = sub_resource_id(Some(&triple[2]))
                .and_then(|id| scene.sub_resource_props(&id).cloned())
            {
                edge.crossfade = float_of(t_props.get("xfade_time")).unwrap_or(0.0) as f64;
                edge.condition = string_of(t_props.get("advance_condition")).unwrap_or_default();
                edge.advance_expression =
                    string_of(t_props.get("advance_expression")).unwrap_or_default();
                if let Some(v) = int_of(t_props.get("switch_mode")) {
                    edge.switch_mode = SwitchMode::from_godot_int(v);
                }
                if let Some(v) = int_of(t_props.get("advance_mode")) {
                    edge.advance_mode = TransitionAdvanceMode::from_godot_int(v);
                }
                if let Some(v) = int_of(t_props.get("priority")) {
                    edge.priority = v as i32;
                }
                if let Some(Variant::Bool(b)) = t_props.get("reset") {
                    edge.reset = *b;
                }
                if let Some(Variant::Bool(b)) = t_props.get("break_loop_at_end") {
                    edge.break_loop_at_end = *b;
                }
                edge.xfade_curve = sub_resource_id(t_props.get("xfade_curve"));
            }
            sm.transitions.push(edge);
        }
    }

    // Initial state: the Start edge's target, else the first state by name so a
    // machine without an explicit entry edge still plays something deterministic.
    sm.current = match start_target {
        Some(target) if sm.states.contains_key(&target) => target,
        _ => {
            let mut keys: Vec<&String> = sm.states.keys().collect();
            keys.sort();
            keys[0].clone()
        }
    };

    Some(AnimationNode::StateMachine(sm))
}

/// Builds a [`BlendSpace1D`] from `blend_point_N/node` + `blend_point_N/pos`.
fn build_blend_space_1d(
    scene: &PackedScene,
    props: &HashMap<String, Variant>,
) -> Option<AnimationNode> {
    let mut space = BlendSpace1D::new();
    for (index, clip) in blend_point_clips(scene, props).into_iter() {
        let pos = float_of(props.get(&format!("blend_point_{index}/pos"))).unwrap_or(0.0);
        space.add_point(pos, clip);
    }
    if space.points().is_empty() {
        return None;
    }

    // The 1D parameter surface (pat-s3rrv.45), previously dropped exactly as
    // the 2D one was. `blend_mode` is the one that changes what a frame
    // samples; the bounds and the grid step are editor properties that must
    // still survive the load.
    if let Some(v) = float_of(props.get("min_space")) {
        space.min_space = v;
    }
    if let Some(v) = float_of(props.get("max_space")) {
        space.max_space = v;
    }
    if let Some(v) = float_of(props.get("snap")) {
        space.snap = v;
    }
    if let Some(Variant::Bool(b)) = props.get("sync") {
        space.sync = *b;
    }
    if let Some(v) = int_of(props.get("blend_mode")) {
        space.blend_mode = BlendSpaceBlendMode::from_godot_int(v);
    }

    Some(AnimationNode::BlendSpace1D(space))
}

/// Builds a [`BlendSpace2D`] from `blend_point_N/node` + `blend_point_N/pos`.
fn build_blend_space_2d(
    scene: &PackedScene,
    props: &HashMap<String, Variant>,
) -> Option<AnimationNode> {
    let mut space = BlendSpace2D::new();
    for (index, clip) in blend_point_clips(scene, props).into_iter() {
        let pos = match props.get(&format!("blend_point_{index}/pos")) {
            Some(Variant::Vector2(v)) => *v,
            _ => Vector2::ZERO,
        };
        space.add_point(pos, clip);
    }
    if space.points().is_empty() {
        return None;
    }

    // The blend-space parameter surface (pat-s3rrv.45). Each of these was
    // previously dropped, which is why a 9-point space snapped between clips:
    // without `triangles` there was nothing to blend barycentrically over.
    if let Some(Variant::Vector2(v)) = props.get("min_space") {
        space.min_space = *v;
    }
    if let Some(Variant::Vector2(v)) = props.get("max_space") {
        space.max_space = *v;
    }
    if let Some(Variant::Vector2(v)) = props.get("snap") {
        space.snap = *v;
    }
    if let Some(Variant::Bool(b)) = props.get("auto_triangles") {
        space.auto_triangles = *b;
    }
    if let Some(Variant::Bool(b)) = props.get("sync") {
        space.sync = *b;
    }
    if let Some(v) = int_of(props.get("blend_mode")) {
        space.blend_mode = BlendSpaceBlendMode::from_godot_int(v);
    }
    // `triangles` is a FLAT PackedInt32Array of point-index triples, not a list
    // of triples — a trailing partial group is ignored rather than read past.
    if let Some(tri) = props.get("triangles") {
        for chunk in packed_ints(tri).chunks_exact(3) {
            space.add_triangle(chunk[0] as usize, chunk[1] as usize, chunk[2] as usize);
        }
    }

    Some(AnimationNode::BlendSpace2D(space))
}

/// Collects `(point_index, clip_name)` for a blend space's points, in index
/// order. Patina's blend spaces hold clip names, so a point whose child is a
/// nested graph node (rather than an `AnimationNodeAnimation`) is skipped.
fn blend_point_clips(
    scene: &PackedScene,
    props: &HashMap<String, Variant>,
) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    // Godot writes points contiguously from 0; stop at the first gap.
    for index in 0.. {
        let key = format!("blend_point_{index}/node");
        let Some(child_id) = sub_resource_id(props.get(&key)) else {
            break;
        };
        match scene.sub_resource_type(&child_id) {
            Some("AnimationNodeAnimation") => {
                let clip = scene
                    .sub_resource_props(&child_id)
                    .and_then(|p| string_of(p.get("animation")));
                match clip {
                    Some(clip) => out.push((index, clip)),
                    None => tracing::debug!("blend point {index} names no animation"),
                }
            }
            other => tracing::debug!(
                "blend point {index} holds a nested '{}' node, which blend spaces do not support yet",
                other.unwrap_or("?")
            ),
        }
    }
    out
}

/// Builds the graph feeding a blend tree's `output` node from `nodes/<name>/node`
/// plus the flat `node_connections` array.
///
/// `node_connections` holds triples of `(destination node, destination input
/// index, source node)` — see `AnimationNodeBlendTree::_set` in
/// upstream/godot/scene/animation/animation_blend_tree.cpp.
fn build_blend_tree(
    scene: &PackedScene,
    props: &HashMap<String, Variant>,
    animations: &HashMap<String, Animation>,
) -> Option<AnimationNode> {
    // name -> sub-resource id
    let mut nodes: HashMap<String, String> = HashMap::new();
    for (key, value) in props {
        if let Some(name) = key
            .strip_prefix("nodes/")
            .and_then(|rest| rest.strip_suffix("/node"))
        {
            if let Some(id) = sub_resource_id(Some(value)) {
                nodes.insert(name.to_string(), id);
            }
        }
    }

    // (destination node, input index) -> source node
    let mut inputs: HashMap<(String, usize), String> = HashMap::new();
    if let Some(Variant::Array(items)) = props.get("node_connections") {
        for triple in items.to_vec().chunks(3) {
            if triple.len() < 3 {
                break;
            }
            let (Some(dst), Some(src)) = (string_of(Some(&triple[0])), string_of(Some(&triple[2])))
            else {
                continue;
            };
            let Some(index) = int_of(Some(&triple[1])) else {
                continue;
            };
            inputs.insert((dst, index as usize), src);
        }
    }

    let source = inputs.get(&(OUTPUT_NODE.to_string(), 0))?;
    build_blend_tree_node(scene, source, &nodes, &inputs, animations, 0)
}

/// Maximum blend-tree depth walked while resolving connections. Godot forbids
/// cycles; this is a cheap guard so a hand-edited scene cannot recurse forever.
const MAX_BLEND_TREE_DEPTH: usize = 32;

/// Resolves one named blend-tree node and, recursively, whatever feeds its
/// inputs. Returns `None` if the node type is unsupported or an input is missing.
fn build_blend_tree_node(
    scene: &PackedScene,
    name: &str,
    nodes: &HashMap<String, String>,
    inputs: &HashMap<(String, usize), String>,
    animations: &HashMap<String, Animation>,
    depth: usize,
) -> Option<AnimationNode> {
    if depth > MAX_BLEND_TREE_DEPTH {
        tracing::warn!("AnimationTree: blend tree nesting exceeded {MAX_BLEND_TREE_DEPTH}");
        return None;
    }
    let sub_id = nodes.get(name)?;
    let class = scene.sub_resource_type(sub_id)?.to_string();
    let empty = HashMap::new();
    let props = scene.sub_resource_props(sub_id).unwrap_or(&empty);

    // Resolves the node wired into input `index` of this node.
    let input = |index: usize| -> Option<AnimationNode> {
        let src = inputs.get(&(name.to_string(), index))?;
        build_blend_tree_node(scene, src, nodes, inputs, animations, depth + 1)
    };

    let node = match class.as_str() {
        // Blend2 inputs: 0 = "in", 1 = "blend" (animation_blend_tree.cpp).
        "AnimationNodeBlend2" => AnimationNode::Blend2 {
            a: Box::new(input(0)?),
            b: Box::new(input(1)?),
            weight: 0.0,
        },
        // Blend3 inputs: 0 = "-blend", 1 = "in", 2 = "+blend".
        "AnimationNodeBlend3" => AnimationNode::Blend3 {
            a: Box::new(input(0)?),
            b: Box::new(input(1)?),
            c: Box::new(input(2)?),
            weight: 0.0,
        },
        // OneShot inputs: 0 = "in" (base), 1 = "shot".
        "AnimationNodeOneShot" => {
            let base = input(0)?;
            let shot = input(1)?;
            let hold = clip_length(&shot, animations);
            let mut one_shot = OneShot::new(base, shot)
                .with_fades(
                    float_of(props.get("fadein_time")).unwrap_or(0.0) as f64,
                    float_of(props.get("fadeout_time")).unwrap_or(0.0) as f64,
                )
                .with_shot_length(hold);
            // The rest of Godot's one-shot surface (pat-s3rrv.45). Each of
            // these was parsed by nothing, so a scene's `mix_mode = 1` shot
            // blended instead of adding and an `autorestart` shot fired once.
            if let Some(v) = int_of(props.get("mix_mode")) {
                one_shot.mix_mode = OneShotMixMode::from_godot_int(v);
            }
            if let Some(Variant::Bool(b)) = props.get("autorestart") {
                one_shot.autorestart = *b;
            }
            if let Some(v) = float_of(props.get("autorestart_delay")) {
                one_shot.autorestart_delay = v as f64;
            }
            if let Some(v) = float_of(props.get("autorestart_random_delay")) {
                one_shot.autorestart_random_delay = v as f64;
            }
            if let Some(Variant::Bool(b)) = props.get("break_loop_at_end") {
                one_shot.break_loop_at_end = *b;
            }
            AnimationNode::OneShot(one_shot)
        }
        // Nested containers and leaves reuse the generic conversion.
        _ => build_animation_node(scene, sub_id, animations)?,
    };

    // The name makes this node's parameters addressable as
    // `parameters/<name>/blend_amount` / `.../request`, as Godot names them.
    Some(named(name, node))
}

/// Length of the clip an [`AnimationNode::Animation`] leaf plays, used to size a
/// one-shot's hold time. Falls back to 1 second for a non-leaf or unknown clip.
fn clip_length(node: &AnimationNode, animations: &HashMap<String, Animation>) -> f64 {
    match node {
        AnimationNode::Animation { name, .. } => {
            animations.get(name).map(|a| a.length).unwrap_or(1.0)
        }
        AnimationNode::Named { inner, .. } => clip_length(inner, animations),
        _ => 1.0,
    }
}

/// Wraps `node` so its `parameters/<name>/...` entries reach it.
fn named(name: &str, node: AnimationNode) -> AnimationNode {
    AnimationNode::Named {
        name: name.to_string(),
        inner: Box::new(node),
    }
}

// ---------------------------------------------------------------------------
// Variant helpers
// ---------------------------------------------------------------------------

/// Extracts the id from a parsed `SubResource("id")` reference, which the `.tscn`
/// value parser stores as `Variant::String("SubResource:id")`.
fn sub_resource_id(value: Option<&Variant>) -> Option<String> {
    match value {
        Some(Variant::String(s)) => s.strip_prefix("SubResource:").map(String::from),
        _ => None,
    }
}

/// Reads a `String`/`StringName` value (Godot writes state and node names as
/// `&"name"` string names, and animation names likewise).
fn string_of(value: Option<&Variant>) -> Option<String> {
    match value {
        Some(Variant::String(s)) if !s.is_empty() => Some(s.clone()),
        Some(Variant::StringName(s)) if !s.as_str().is_empty() => Some(s.as_str().to_string()),
        _ => None,
    }
}

/// Reads a numeric value as `f32`.
fn float_of(value: Option<&Variant>) -> Option<f32> {
    match value {
        Some(Variant::Float(f)) => Some(*f as f32),
        Some(Variant::Int(i)) => Some(*i as f32),
        _ => None,
    }
}

/// Reads an integer value (a connection's input index).
/// Reads a flat integer sequence out of any shape Godot writes for a packed
/// int property — `PackedInt32Array`, `PackedInt64Array`, or a plain `Array` of
/// numbers. An unrelated Variant yields an empty list, so a malformed
/// `triangles` entry drops the triangulation rather than panicking.
fn packed_ints(value: &Variant) -> Vec<i64> {
    match value {
        Variant::PackedInt32Array(a) => a.iter().map(|i| *i as i64).collect(),
        Variant::PackedInt64Array(a) => a.clone(),
        Variant::Array(items) => items
            .to_vec()
            .iter()
            .filter_map(|v| match v {
                Variant::Int(i) => Some(*i),
                Variant::Float(f) => Some(*f as i64),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// Godot's `Animation.LoopMode` integer (`0 = NONE, 1 = LINEAR, 2 = PINGPONG`),
/// as an `AnimationNodeAnimation`'s custom timeline serializes it.
fn loop_mode_from_godot_int(v: i64) -> LoopMode {
    match v {
        1 => LoopMode::Linear,
        2 => LoopMode::PingPong,
        _ => LoopMode::None,
    }
}

fn int_of(value: Option<&Variant>) -> Option<i64> {
    match value {
        Some(Variant::Int(i)) => Some(*i),
        Some(Variant::Float(f)) => Some(*f as i64),
        _ => None,
    }
}

/// Flattens a `NodePath("a/b")` (or plain string) property to `"a/b"`, which is
/// what [`SceneTree::get_node_or_null`] takes. Empty when absent.
fn node_path_string(value: Option<&Variant>) -> String {
    match value {
        Some(Variant::NodePath(np)) => {
            let names: Vec<&str> = (0..np.get_name_count())
                .filter_map(|i| np.get_name(i))
                .collect();
            let joined = names.join("/");
            if np.is_absolute() {
                format!("/{joined}")
            } else {
                joined
            }
        }
        Some(Variant::String(s)) => s.clone(),
        Some(Variant::StringName(s)) => s.as_str().to_string(),
        _ => String::new(),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Parses `body` as a scene. A `[node]` section is appended because the
    /// `.tscn` parser rejects a file with no nodes.
    fn scene(body: &str) -> PackedScene {
        let source = format!("[gd_scene format=3]\n\n{body}\n[node name=\"Root\" type=\"Node\"]\n");
        PackedScene::from_tscn(&source).expect("scene parses")
    }

    fn no_clips() -> HashMap<String, Animation> {
        HashMap::new()
    }

    /// Unwraps the `Named` wrapper the importer puts around every graph node it
    /// names, returning the node inside.
    fn inner(node: &AnimationNode) -> &AnimationNode {
        match node {
            AnimationNode::Named { inner, .. } => inner,
            other => other,
        }
    }

    fn name_of(node: &AnimationNode) -> Option<&str> {
        match node {
            AnimationNode::Named { name, .. } => Some(name.as_str()),
            _ => None,
        }
    }

    const STATE_MACHINE: &str = r#"[sub_resource type="AnimationNodeAnimation" id="Anim_idle"]
animation = &"idle"

[sub_resource type="AnimationNodeAnimation" id="Anim_run"]
animation = &"run"

[sub_resource type="AnimationNodeStateMachineTransition" id="T_start"]
xfade_time = 0.0

[sub_resource type="AnimationNodeStateMachineTransition" id="T_run"]
xfade_time = 0.25
advance_condition = &"run"

[sub_resource type="AnimationNodeStateMachine" id="SM"]
states/idle/node = SubResource("Anim_idle")
states/idle/position = Vector2(300, 100)
states/run/node = SubResource("Anim_run")
transitions = ["Start", "idle", SubResource("T_start"), "idle", "run", SubResource("T_run")]
"#;

    #[test]
    fn state_machine_from_tscn_sub_resources() {
        let scene = scene(STATE_MACHINE);
        let node = build_animation_node(&scene, "SM", &no_clips()).expect("state machine builds");

        let AnimationNode::StateMachine(sm) = node else {
            panic!("expected a state machine, got {node:?}");
        };

        // Both declared states are present, each named so `parameters/<state>/...`
        // can address it, and each resolving to its clip leaf.
        assert_eq!(sm.states.len(), 2, "states: {:?}", sm.states.keys());
        let idle = sm.states.get("idle").expect("idle state");
        assert_eq!(name_of(idle), Some("idle"));
        match inner(idle) {
            AnimationNode::Animation { name, .. } => assert_eq!(name, "idle"),
            other => panic!("idle state is not an animation leaf: {other:?}"),
        }

        // The Start edge selects the entry state and is not kept as a real edge.
        assert_eq!(sm.current, "idle");
        assert_eq!(sm.transitions.len(), 1, "{:?}", sm.transitions);
        let t = &sm.transitions[0];
        assert_eq!((t.from.as_str(), t.to.as_str()), ("idle", "run"));
        assert_eq!(
            t.condition, "run",
            "advance_condition becomes the condition"
        );
        assert!(
            (t.crossfade - 0.25).abs() < 1e-6,
            "xfade_time becomes the crossfade: {}",
            t.crossfade
        );
    }

    #[test]
    fn state_machine_accepts_string_name_transitions() {
        // Godot writes state names in `transitions` as StringNames (`&"idle"`).
        let scene = scene(
            r#"[sub_resource type="AnimationNodeAnimation" id="A"]
animation = &"idle"

[sub_resource type="AnimationNodeAnimation" id="B"]
animation = &"run"

[sub_resource type="AnimationNodeStateMachineTransition" id="T"]
xfade_time = 0.5
advance_condition = &"go"

[sub_resource type="AnimationNodeStateMachine" id="SM"]
states/idle/node = SubResource("A")
states/run/node = SubResource("B")
transitions = [&"Start", &"idle", SubResource("T"), &"idle", &"run", SubResource("T")]
"#,
        );
        let node = build_animation_node(&scene, "SM", &no_clips()).expect("state machine builds");
        let AnimationNode::StateMachine(sm) = node else {
            panic!("expected a state machine");
        };
        assert_eq!(sm.current, "idle");
        assert_eq!(sm.transitions.len(), 1);
        assert_eq!(sm.transitions[0].condition, "go");
    }

    #[test]
    fn state_machine_without_start_edge_picks_a_deterministic_state() {
        let scene = scene(
            r#"[sub_resource type="AnimationNodeAnimation" id="A"]
animation = &"walk"

[sub_resource type="AnimationNodeAnimation" id="B"]
animation = &"zzz"

[sub_resource type="AnimationNodeStateMachine" id="SM"]
states/walk/node = SubResource("A")
states/zzz/node = SubResource("B")
"#,
        );
        let node = build_animation_node(&scene, "SM", &no_clips()).expect("builds");
        let AnimationNode::StateMachine(sm) = node else {
            panic!("expected a state machine");
        };
        assert_eq!(sm.current, "walk", "first state by name, not HashMap order");
    }

    #[test]
    fn blend_space_1d_points_from_tscn() {
        let scene = scene(
            r#"[sub_resource type="AnimationNodeAnimation" id="A"]
animation = &"idle"

[sub_resource type="AnimationNodeAnimation" id="B"]
animation = &"run"

[sub_resource type="AnimationNodeBlendSpace1D" id="BS"]
blend_point_0/node = SubResource("A")
blend_point_0/pos = 0.0
blend_point_1/node = SubResource("B")
blend_point_1/pos = 1.0
min_space = -1.0
max_space = 1.0
"#,
        );
        let node = build_animation_node(&scene, "BS", &no_clips()).expect("blend space builds");
        let AnimationNode::BlendSpace1D(space) = node else {
            panic!("expected a 1D blend space, got {node:?}");
        };
        assert_eq!(
            space.points().to_vec(),
            vec![(0.0, "idle".to_string()), (1.0, "run".to_string())]
        );
    }

    #[test]
    fn blend_space_2d_points_from_tscn() {
        let scene = scene(
            r#"[sub_resource type="AnimationNodeAnimation" id="A"]
animation = &"idle"

[sub_resource type="AnimationNodeBlendSpace2D" id="BS"]
blend_point_0/node = SubResource("A")
blend_point_0/pos = Vector2(0.5, -0.25)
"#,
        );
        let node = build_animation_node(&scene, "BS", &no_clips()).expect("blend space builds");
        let AnimationNode::BlendSpace2D(space) = node else {
            panic!("expected a 2D blend space, got {node:?}");
        };
        assert_eq!(space.points().len(), 1);
        assert_eq!(space.points()[0].1, "idle");
        assert!((space.points()[0].0.x - 0.5).abs() < 1e-6);
        assert!((space.points()[0].0.y + 0.25).abs() < 1e-6);
    }

    #[test]
    fn blend_tree_wires_blend2_inputs_from_node_connections() {
        // `node_connections` triples are (destination, input index, source).
        let scene = scene(
            r#"[sub_resource type="AnimationNodeAnimation" id="A"]
animation = &"idle"

[sub_resource type="AnimationNodeAnimation" id="B"]
animation = &"run"

[sub_resource type="AnimationNodeBlend2" id="Mix"]

[sub_resource type="AnimationNodeBlendTree" id="BT"]
nodes/idle/node = SubResource("A")
nodes/run/node = SubResource("B")
nodes/mix/node = SubResource("Mix")
node_connections = ["output", 0, "mix", "mix", 0, "idle", "mix", 1, "run"]
"#,
        );
        let node = build_animation_node(&scene, "BT", &no_clips()).expect("blend tree builds");
        // The graph feeding `output` is the named `mix` node.
        assert_eq!(name_of(&node), Some("mix"));
        let AnimationNode::Blend2 { a, b, weight } = inner(&node) else {
            panic!("expected Blend2 at the output, got {node:?}");
        };
        assert_eq!(
            *weight, 0.0,
            "blend_amount comes from parameters at runtime"
        );
        // Input 0 is Godot's "in", input 1 its "blend".
        match inner(a) {
            AnimationNode::Animation { name, .. } => assert_eq!(name, "idle"),
            other => panic!("input 0 is not the idle clip: {other:?}"),
        }
        match inner(b) {
            AnimationNode::Animation { name, .. } => assert_eq!(name, "run"),
            other => panic!("input 1 is not the run clip: {other:?}"),
        }
    }

    #[test]
    fn blend_tree_one_shot_reads_fades_and_shot_length() {
        let scene = scene(
            r#"[sub_resource type="AnimationNodeAnimation" id="A"]
animation = &"idle"

[sub_resource type="AnimationNodeAnimation" id="B"]
animation = &"hit"

[sub_resource type="AnimationNodeOneShot" id="Shot"]
fadein_time = 0.1
fadeout_time = 0.2

[sub_resource type="AnimationNodeBlendTree" id="BT"]
nodes/idle/node = SubResource("A")
nodes/hit/node = SubResource("B")
nodes/shot/node = SubResource("Shot")
node_connections = ["output", 0, "shot", "shot", 0, "idle", "shot", 1, "hit"]
"#,
        );
        // The shot's hold time comes from the clip it plays.
        let mut clips = HashMap::new();
        clips.insert("hit".to_string(), Animation::new("hit", 0.75));

        let node = build_animation_node(&scene, "BT", &clips).expect("blend tree builds");
        let AnimationNode::OneShot(one_shot) = inner(&node) else {
            panic!("expected a one-shot at the output, got {node:?}");
        };
        assert!((one_shot.fade_in - 0.1).abs() < 1e-6);
        assert!((one_shot.fade_out - 0.2).abs() < 1e-6);
        assert!(
            (one_shot.shot_length - 0.75).abs() < 1e-6,
            "shot length should follow the shot clip: {}",
            one_shot.shot_length
        );
    }

    #[test]
    fn unsupported_graph_node_builds_nothing() {
        // TimeScale and friends are pat-0q2kx.53; a tree is left unattached
        // rather than half-built.
        let scene = scene(
            r#"[sub_resource type="AnimationNodeTimeScale" id="TS"]
"#,
        );
        assert!(build_animation_node(&scene, "TS", &no_clips()).is_none());
        assert!(build_animation_node(&scene, "missing_id", &no_clips()).is_none());
    }

    #[test]
    fn build_animation_tree_reads_active_root_node_and_parameters() {
        let scene = scene(STATE_MACHINE);
        let mut props = HashMap::new();
        props.insert(
            "tree_root".to_string(),
            Variant::String("SubResource:SM".into()),
        );
        props.insert("active".to_string(), Variant::Bool(false));
        props.insert(
            "anim_player".to_string(),
            Variant::NodePath(gdcore::node_path::NodePath::new("../AnimationPlayer")),
        );
        props.insert("parameters/conditions/run".to_string(), Variant::Bool(true));

        let anim_tree = build_animation_tree(&scene, &props, no_clips()).expect("tree builds");
        assert!(!anim_tree.active, "active = false must be honored");
        assert_eq!(anim_tree.anim_player, "../AnimationPlayer");
        assert_eq!(
            anim_tree.root_node, "..",
            "Godot's AnimationMixer.root_node default"
        );
        assert_eq!(
            anim_tree.get_parameter("parameters/conditions/run"),
            Some(Variant::Bool(true)),
            "scene parameters seed the tree's parameter map"
        );
    }

    #[test]
    fn build_animation_tree_without_tree_root_is_none() {
        let scene = scene(STATE_MACHINE);
        assert!(build_animation_tree(&scene, &HashMap::new(), no_clips()).is_none());
    }

    #[test]
    fn scene_load_attaches_a_tree_and_resolves_clips_from_the_player() {
        // End-to-end through the scene-instancing path: the AnimationTree node
        // gets a graph, and its clip library comes from the AnimationPlayer at
        // `anim_player`.
        let source = format!(
            r#"[gd_scene format=3]

{STATE_MACHINE}
[sub_resource type="Animation" id="Anim_clip_idle"]
resource_name = "idle"
length = 1.0
tracks/0/type = "value"
tracks/0/path = NodePath("Sprite:x")
tracks/0/keys = {{"times": PackedFloat32Array(0, 1), "values": [0.0, 5.0]}}

[sub_resource type="AnimationLibrary" id="Lib"]
_data = {{"idle": SubResource("Anim_clip_idle")}}

[node name="Root" type="Node2D"]

[node name="Sprite" type="Node2D" parent="."]
x = 0.0

[node name="AnimationPlayer" type="AnimationPlayer" parent="."]
libraries = {{"": SubResource("Lib")}}

[node name="Tree" type="AnimationTree" parent="."]
tree_root = SubResource("SM")
anim_player = NodePath("../AnimationPlayer")
"#
        );
        let scene = PackedScene::from_tscn(&source).expect("scene parses");
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let scene_root = crate::packed_scene::add_packed_scene_to_tree(&mut tree, root, &scene)
            .expect("instance");

        let tree_node = tree
            .get_node_or_null(scene_root, "Tree")
            .expect("AnimationTree node");
        let attached = tree
            .get_animation_tree(tree_node)
            .expect("scene load attaches the tree");
        assert!(attached.active, "active defaults to true");
        assert!(
            attached.animations.contains_key("idle"),
            "clips resolved from the player: {:?}",
            attached.animations.keys().collect::<Vec<_>>()
        );
        assert_eq!(tree.animation_tree_count(), 1);
    }
}
