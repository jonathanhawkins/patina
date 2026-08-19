//! GDScript method surface for `NavigationAgent2D` / `NavigationAgent3D`
//! (pat-s3rrv.20).
//!
//! # Why this module exists
//!
//! Godot's navigation agents are **passive**. `doc/classes/NavigationAgent2D.xml`
//! is explicit: "The calculation can be used by the parent node to dynamically
//! move it along the path", and "after setting `target_position`,
//! `get_next_path_position` must be used once every physics frame". The agent
//! inherits `Node`, so it has no transform of its own and moves nothing — the
//! *game* moves the body:
//!
//! ```gdscript
//! velocity = (agent.get_next_path_position() - global_position).normalized() * SPEED
//! move_and_slide()
//! ```
//!
//! Patina registered the agents' properties and signals but **zero methods**, so
//! every line of that idiom raised `UndefinedFunction("ObjectId.get_next_path_position")`.
//! The agent classes were therefore unusable from a script no matter how well the
//! pathfinder underneath worked.
//!
//! # Where the state comes from
//!
//! The backing [`crate::navigation::NavigationAgent2D`] lives in
//! `MainLoop::nav_agents_2d/3d`, which a `SceneTree`-only accessor cannot reach.
//! Rather than thread a `MainLoop` address through the script bindings,
//! `process_navigation` **publishes** each agent's per-frame state onto its node
//! as `__nav_*` companion properties (the same convention `.tscn` flattening uses
//! for `__shape__*` / `__resref__*`), and this module reads them back.
//!
//! That mirrors the engine's actual contract rather than hiding it: these getters
//! report the state as of the last physics frame, which is exactly why Godot
//! documents `get_next_path_position` as a once-per-physics-frame call. Values
//! that are pure geometry ([`distance_to_target`](fn@call_agent_method)) are
//! computed live from the node's current global position instead, so they stay
//! correct between frames.
//!
//! # Not implemented here
//!
//! `get_rid`, `get_navigation_map` and `set_navigation_map` are deliberately
//! absent: Patina's navigation servers have no RID registry to hand out, and
//! returning a fabricated integer would let scripts pass a meaningless handle
//! around. They fall through to the normal unknown-method error.

use crate::navigation::{NavAgentSignal, NavigationAgent2D, NavigationAgent3D};
use crate::node::{Node, NodeId};
use crate::scene_tree::SceneTree;
use gdcore::math::{Vector2, Vector3};
use gdvariant::Variant;

/// Which agent flavour a node is, if either.
#[derive(Clone, Copy, PartialEq, Eq)]
enum AgentKind {
    D2,
    D3,
}

impl AgentKind {
    fn of(node: &Node) -> Option<Self> {
        match node.class_name() {
            "NavigationAgent2D" => Some(Self::D2),
            "NavigationAgent3D" => Some(Self::D3),
            _ => None,
        }
    }
}

/// Companion property names `process_navigation` publishes each frame.
///
/// Prefixed so they cannot collide with a real Godot property and are obviously
/// engine-internal in an inspector dump.
pub mod published {
    /// The next point to steer toward — Godot `get_next_path_position()`.
    ///
    /// Unprefixed because it predates this bead and existing tests read it.
    pub const NEXT_PATH_POSITION: &str = "next_path_position";
    /// Whole current path, as a packed vector array.
    pub const PATH: &str = "__nav_path__";
    /// Index of the waypoint the agent is currently steering toward.
    pub const PATH_INDEX: &str = "__nav_path_index__";
    /// Final reachable point of the current path.
    pub const FINAL_POSITION: &str = "__nav_final_position__";
    /// Whether the agent considers navigation complete.
    pub const FINISHED: &str = "__nav_finished__";
    /// Whether a path to `target_position` exists at all.
    pub const TARGET_REACHABLE: &str = "__nav_target_reachable__";
}

/// Reads `key` off `node` as a bitmask, defaulting to Godot's "layer 1 only".
fn mask_prop(tree: &SceneTree, node: NodeId, key: &str, default: u32) -> u32 {
    match tree.get_node(node).map(|n| n.get_property(key)) {
        Some(Variant::Int(v)) => v as u32,
        _ => default,
    }
}

/// Godot's layer accessors are **1-based** (`layer_number` 1..=32); a script
/// passing 0 or 33 is out of range and Godot ignores it rather than wrapping.
fn layer_bit(args: &[Variant]) -> Option<u32> {
    match args.first() {
        Some(Variant::Int(n)) if (1..=32).contains(n) => Some(1u32 << (n - 1)),
        _ => None,
    }
}

fn arg_bool(args: &[Variant], i: usize) -> bool {
    matches!(args.get(i), Some(Variant::Bool(true)))
}

fn arg_vec2(args: &[Variant], i: usize) -> Option<Vector2> {
    match args.get(i) {
        Some(Variant::Vector2(v)) => Some(*v),
        _ => None,
    }
}

fn arg_vec3(args: &[Variant], i: usize) -> Option<Vector3> {
    match args.get(i) {
        Some(Variant::Vector3(v)) => Some(*v),
        _ => None,
    }
}

/// Dispatches a `NavigationAgent2D`/`NavigationAgent3D` method call.
///
/// Returns `None` when `node` is not an agent or `method` is not part of the
/// surface, so the caller falls through to its normal method table and the
/// usual unknown-method diagnostics are preserved.
pub fn call_agent_method(
    tree: &mut SceneTree,
    node: NodeId,
    method: &str,
    args: &[Variant],
) -> Option<Variant> {
    let kind = AgentKind::of(tree.get_node(node)?)?;

    // ---- setters that write straight back onto the node -------------------
    //
    // `target_position` is a real Godot property, so the setter is a property
    // write and `process_navigation` picks it up on the next tick — the same
    // route a `.tscn`-authored target takes.
    match method {
        "set_target_position" => {
            let value = match kind {
                AgentKind::D2 => arg_vec2(args, 0).map(Variant::Vector2),
                AgentKind::D3 => arg_vec3(args, 0).map(Variant::Vector3),
            };
            if let (Some(value), Some(n)) = (value, tree.get_node_mut(node)) {
                n.set_property("target_position", value);
            }
            return Some(Variant::Nil);
        }
        // Godot distinguishes `velocity` (a request the avoidance solve may
        // override) from `velocity_forced` (bypasses the solve entirely). Both
        // are consumed by the avoidance phase, which already reads the node.
        "set_velocity" | "set_velocity_forced" => {
            let value = match kind {
                AgentKind::D2 => arg_vec2(args, 0).map(Variant::Vector2),
                AgentKind::D3 => arg_vec3(args, 0).map(Variant::Vector3),
            };
            if let (Some(value), Some(n)) = (value, tree.get_node_mut(node)) {
                n.set_property("velocity", value);
                if method == "set_velocity_forced" {
                    n.set_property("velocity_forced", Variant::Bool(true));
                }
            }
            return Some(Variant::Nil);
        }
        _ => {}
    }

    // ---- 1-based layer/mask bit accessors ---------------------------------
    let bit_key = match method {
        "get_avoidance_layer_value" | "set_avoidance_layer_value" => Some("avoidance_layers"),
        "get_avoidance_mask_value" | "set_avoidance_mask_value" => Some("avoidance_mask"),
        "get_navigation_layer_value" | "set_navigation_layer_value" => Some("navigation_layers"),
        _ => None,
    };
    if let Some(key) = bit_key {
        let Some(bit) = layer_bit(args) else {
            // Out-of-range layer number: Godot ignores it. Getters answer false.
            return Some(if method.starts_with("get_") {
                Variant::Bool(false)
            } else {
                Variant::Nil
            });
        };
        let current = mask_prop(tree, node, key, 1);
        if method.starts_with("get_") {
            return Some(Variant::Bool(current & bit != 0));
        }
        let next = if arg_bool(args, 1) {
            current | bit
        } else {
            current & !bit
        };
        if let Some(n) = tree.get_node_mut(node) {
            n.set_property(key, Variant::Int(next as i64));
        }
        return Some(Variant::Nil);
    }

    // ---- getters over the frame's published state -------------------------
    //
    // Read-only from here down, so a shared borrow is enough.
    let n = tree.get_node(node)?;
    let target = n.get_property("target_position");

    // The agent's own global position. Godot's agent has no transform and simply
    // follows its parent body, so this tracks the body — which is what makes the
    // distance/arrival answers move as the game moves the body.
    let here_2d = || crate::node2d::get_global_position(tree, node);
    let here_3d = || crate::node3d::get_global_transform(tree, node).origin;

    let value = match method {
        "get_next_path_position" => match n.get_property(published::NEXT_PATH_POSITION) {
            v @ (Variant::Vector2(_) | Variant::Vector3(_)) => v,
            // Nothing published yet (no physics frame has run). Godot answers
            // with the agent's own position rather than a null, so a script can
            // subtract it on frame zero without a crash.
            _ => match kind {
                AgentKind::D2 => Variant::Vector2(here_2d()),
                AgentKind::D3 => Variant::Vector3(here_3d()),
            },
        },
        "get_target_position" => match (kind, target) {
            (AgentKind::D2, Variant::Vector2(v)) => Variant::Vector2(v),
            (AgentKind::D3, Variant::Vector3(v)) => Variant::Vector3(v),
            (AgentKind::D2, _) => Variant::Vector2(Vector2::ZERO),
            (AgentKind::D3, _) => Variant::Vector3(Vector3::ZERO),
        },
        "is_navigation_finished" => Variant::Bool(matches!(
            n.get_property(published::FINISHED),
            Variant::Bool(true)
        )),
        // Godot's `is_target_reached` asks whether the agent is within
        // `target_desired_distance` of the goal *right now*, which is geometry —
        // not the latched "finished" flag, which stays true once the run ends.
        "is_target_reached" => {
            let desired = match n.get_property("target_desired_distance") {
                Variant::Float(v) => v as f32,
                Variant::Int(v) => v as f32,
                _ => 10.0,
            };
            let distance = match (kind, target) {
                (AgentKind::D2, Variant::Vector2(t)) => (t - here_2d()).length(),
                (AgentKind::D3, Variant::Vector3(t)) => (t - here_3d()).length(),
                _ => f32::INFINITY,
            };
            Variant::Bool(distance <= desired)
        }
        // A target outside every navigable region yields an empty path; the
        // runtime publishes that verdict so the script need not infer it.
        "is_target_reachable" => Variant::Bool(matches!(
            n.get_property(published::TARGET_REACHABLE),
            Variant::Bool(true)
        )),
        "distance_to_target" => {
            let distance = match (kind, target) {
                (AgentKind::D2, Variant::Vector2(t)) => (t - here_2d()).length(),
                (AgentKind::D3, Variant::Vector3(t)) => (t - here_3d()).length(),
                _ => 0.0,
            };
            Variant::Float(distance as f64)
        }
        "get_current_navigation_path" => match n.get_property(published::PATH) {
            v @ (Variant::PackedVector2Array(_) | Variant::PackedVector3Array(_)) => v,
            // An agent with no path answers with an empty array, never Nil, so a
            // script can call `.size()` on the result unconditionally.
            _ => match kind {
                AgentKind::D2 => Variant::PackedVector2Array(Vec::new()),
                AgentKind::D3 => Variant::PackedVector3Array(Vec::new()),
            },
        },
        // Total length of the whole current path, summed over its segments —
        // Godot measures the path itself, not the distance still to walk.
        "get_path_length" => {
            let length = match n.get_property(published::PATH) {
                Variant::PackedVector2Array(p) => {
                    p.windows(2).map(|w| (w[1] - w[0]).length()).sum::<f32>()
                }
                Variant::PackedVector3Array(p) => {
                    p.windows(2).map(|w| (w[1] - w[0]).length()).sum::<f32>()
                }
                _ => 0.0,
            };
            Variant::Float(length as f64)
        }
        "get_current_navigation_path_index" => match n.get_property(published::PATH_INDEX) {
            Variant::Int(i) => Variant::Int(i),
            _ => Variant::Int(0),
        },
        "get_final_position" => match n.get_property(published::FINAL_POSITION) {
            v @ (Variant::Vector2(_) | Variant::Vector3(_)) => v,
            // No path computed yet: the furthest reachable point is where the
            // agent already stands.
            _ => match kind {
                AgentKind::D2 => Variant::Vector2(here_2d()),
                AgentKind::D3 => Variant::Vector3(here_3d()),
            },
        },
        _ => return None,
    };
    Some(value)
}

// ---------------------------------------------------------------------------
// Runtime side: what `process_navigation` calls each tick.
//
// These live here rather than in `main_loop` so the whole agent contract — what
// is published and what is read back — sits in one file and cannot drift apart.
// ---------------------------------------------------------------------------

/// Reads a float-ish property, leaving `current` alone when it is absent.
///
/// A `.tscn` that does not mention a tuning value must keep Godot's default, so
/// "absent" and "authored as 0.0" have to stay distinguishable.
fn tuned(node: &Node, key: &str, current: f32) -> f32 {
    match node.get_property(key) {
        Variant::Float(v) => v as f32,
        Variant::Int(v) => v as f32,
        _ => current,
    }
}

/// Copies the scene-authored tuning values onto the backing agent.
///
/// Before this, `process_navigation` read only `target_position`, so a `.tscn`
/// that set `path_desired_distance` or `target_desired_distance` was silently
/// ignored during path following — the agent used its hardcoded defaults and
/// arrived at a different place than the project asked for.
/// `AvoidanceNodeParams::apply` did sync some of these, but only in the
/// avoidance phase and only when `avoidance_enabled` was true.
pub fn sync_tuning_2d(tree: &SceneTree, node: NodeId, agent: &mut NavigationAgent2D) {
    let Some(n) = tree.get_node(node) else { return };
    agent.path_desired_distance = tuned(n, "path_desired_distance", agent.path_desired_distance);
    agent.target_desired_distance =
        tuned(n, "target_desired_distance", agent.target_desired_distance);
    agent.max_speed = tuned(n, "max_speed", agent.max_speed);
    agent.radius = tuned(n, "radius", agent.radius);
}

/// The 3D twin of [`sync_tuning_2d`].
pub fn sync_tuning_3d(tree: &SceneTree, node: NodeId, agent: &mut NavigationAgent3D) {
    let Some(n) = tree.get_node(node) else { return };
    agent.path_desired_distance = tuned(n, "path_desired_distance", agent.path_desired_distance);
    agent.target_desired_distance =
        tuned(n, "target_desired_distance", agent.target_desired_distance);
    agent.max_speed = tuned(n, "max_speed", agent.max_speed);
    agent.radius = tuned(n, "radius", agent.radius);
    // `height` is the agent cylinder's vertical extent used by the layered
    // avoidance solve. It is NOT `path_height_offset`, which Godot declares
    // separately as a vertical offset applied to the path position — Patina has
    // no field backing that one, so it stays unsynced rather than being folded
    // in here, where it would silently resize the avoidance cylinder instead.
    agent.height = tuned(n, "height", agent.height);
}

/// Publishes the agent's per-frame state onto its node for the getters to read.
///
/// `next_path_position` is published by the caller (it already was, before this
/// bead); everything here is the state that had no representation at all, which
/// is why `get_current_navigation_path` and friends had nothing to answer with.
pub fn publish_state_2d(tree: &mut SceneTree, node: NodeId, agent: &NavigationAgent2D) {
    let finished = agent.is_navigation_finished();
    let path = agent.path.clone();
    // The furthest point the agent can actually reach. With no path there is no
    // reachable point, and the getter falls back to the agent's own position.
    let final_position = path.last().copied();
    let reachable = !path.is_empty();
    let index = agent.path_index as i64;
    let Some(n) = tree.get_node_mut(node) else {
        return;
    };
    n.set_property(published::PATH, Variant::PackedVector2Array(path));
    n.set_property(published::PATH_INDEX, Variant::Int(index));
    n.set_property(published::FINISHED, Variant::Bool(finished));
    n.set_property(published::TARGET_REACHABLE, Variant::Bool(reachable));
    if let Some(p) = final_position {
        n.set_property(published::FINAL_POSITION, Variant::Vector2(p));
    }
}

/// The 3D twin of [`publish_state_2d`].
pub fn publish_state_3d(tree: &mut SceneTree, node: NodeId, agent: &NavigationAgent3D) {
    let finished = agent.is_navigation_finished();
    let path = agent.path.clone();
    let final_position = path.last().copied();
    let reachable = !path.is_empty();
    let index = agent.path_index as i64;
    let Some(n) = tree.get_node_mut(node) else {
        return;
    };
    n.set_property(published::PATH, Variant::PackedVector3Array(path));
    n.set_property(published::PATH_INDEX, Variant::Int(index));
    n.set_property(published::FINISHED, Variant::Bool(finished));
    n.set_property(published::TARGET_REACHABLE, Variant::Bool(reachable));
    if let Some(p) = final_position {
        n.set_property(published::FINAL_POSITION, Variant::Vector3(p));
    }
}

/// Emits the milestone signals the agent computed this frame.
///
/// `NavAgentSignal` has been produced since pat-u3l8c.64 and drained by nobody:
/// `path_changed` and `waypoint_reached` are declared in ClassDB, so
/// `connect("waypoint_reached", …)` succeeded and then never fired.
///
/// **Only those two are emitted here.** `target_reached`, `navigation_finished`
/// and `velocity_computed` already have hand-written emitters in the runtime, so
/// draining them here as well would fire each of them twice per frame —
/// signals are observable, and a doubled one is its own bug.
///
/// Godot passes a `details` Dictionary with `waypoint_reached`; Patina's
/// `NavAgentSignal` carries no payload, so this emits the signal with no
/// arguments. A connected handler that takes the details parameter is therefore
/// not yet satisfiable — reported as remaining scope rather than faked with an
/// empty dictionary.
pub fn drain_and_emit_2d(tree: &mut SceneTree, node: NodeId, agent: &mut NavigationAgent2D) {
    for signal in agent.drain_signals() {
        let name = match signal {
            NavAgentSignal::PathChanged => "path_changed",
            NavAgentSignal::WaypointReached => "waypoint_reached",
            NavAgentSignal::TargetReached
            | NavAgentSignal::NavigationFinished
            | NavAgentSignal::VelocityComputed => continue,
        };
        tree.emit_signal(node, name, &[]);
    }
}
