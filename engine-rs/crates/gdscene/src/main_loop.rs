//! Main frame loop for deterministic scene execution.
//!
//! The [`MainLoop`] drives the scene tree through a Godot-compatible frame
//! loop with fixed-timestep physics and variable-timestep process callbacks.
//!
//! # Frame model
//!
//! Each call to [`MainLoop::step`] represents one visual frame:
//!
//! 1. **Physics phase** — the physics time accumulator is advanced by `delta`.
//!    While the accumulator holds at least one fixed-timestep tick, the tree
//!    receives [`NOTIFICATION_PHYSICS_PROCESS`](gdobject::NOTIFICATION_PHYSICS_PROCESS).
//!    At most `max_physics_steps_per_frame` ticks run per frame to avoid a
//!    spiral of death.
//! 2. **Process phase** — the tree receives
//!    [`NOTIFICATION_PROCESS`](gdobject::NOTIFICATION_PROCESS) exactly once.
//! 3. Frame counter and elapsed-time accumulators are updated.

use std::collections::{HashMap, HashSet};

use crate::audio_server_bridge::AudioServerBridge;
use crate::navigation::{
    navmesh_polygon_indices, NavMesh2D, NavMesh3D, NavPolygon, NavPolygon3D, NavigationAgent2D,
    NavigationAgent3D, NavigationServer2D, NavigationServer3D, OrcaAgent,
};
use crate::node::NodeId;
use crate::particle::{ParticleEmitter, ParticleSimulator, SubEmitterMode};

/// One resolved `sub_emitter` link for the current frame (pat-0q2kx.233).
///
/// Built while the scene tree is borrowed immutably, then consumed after the
/// particle step loop: the parent and child simulators are separate entries in
/// `MainLoop::particles`, so the parent→child hop cannot be done under a single
/// mutable borrow.
struct SubEmitterChain {
    /// The emitter whose particles trigger sub-emission.
    parent: NodeId,
    /// The particle node named by the parent's `sub_emitter` NodePath.
    child: NodeId,
    /// Which particle event fires the burst.
    mode: SubEmitterMode,
    /// Particles spawned per dying parent particle (`AT_END`).
    amount_at_end: u32,
    /// Particles spawned per newborn parent particle (`AT_START`).
    amount_at_start: u32,
    /// `CONSTANT` rate in emissions per second per live parent particle.
    frequency: f32,
    /// Whether a child inherits the parent particle's velocity.
    keep_velocity: bool,
}
use crate::path3d::{Curve2D, Curve3D, PathFollow2D, PathFollow3D, PathFollowRotation};
use crate::physics_server::PhysicsServer;
use crate::physics_server_3d::PhysicsServer3D;
use crate::scene_tree::SceneTree;
use crate::scripting::InputSnapshot;
use crate::trace::{TraceEvent, TraceEventType};
use gdobject::notification::{NOTIFICATION_PAUSED, NOTIFICATION_UNPAUSED};
use gdplatform::input::{InputEvent, InputMap, InputState};
use gdvariant::Variant;

/// Output returned by [`MainLoop::step`] after each frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FrameOutput {
    /// The frame number that was just completed (1-indexed after first step).
    pub frame_count: u64,
    /// The delta time that was used for this frame.
    pub delta: f64,
    /// Number of physics ticks that ran during this frame.
    pub physics_steps: u32,
    /// The cursor shape the currently hovered `Control` asks for
    /// (pat-0q2kx.175).
    ///
    /// Reported per frame so the host shell can set its CSS cursor — the shape is
    /// a property of whatever is under the mouse, so it has to be recomputed as
    /// the hover moves rather than pushed on change. Falls back to
    /// [`CursorShape::Arrow`] when nothing is hovered.
    pub cursor_shape: crate::control::CursorShape,
}

/// A snapshot of a single node's properties at a given frame.
#[derive(Debug, Clone, PartialEq)]
pub struct NodeSnapshot {
    /// The path of the node in the scene tree.
    pub path: String,
    /// The class name of the node.
    pub class_name: String,
    /// Property values at the end of this frame.
    pub properties: HashMap<String, Variant>,
}

/// A record of everything that happened during a single frame.
///
/// This captures the full per-frame evolution data needed to validate
/// Godot-compatible frame processing semantics: which notifications fired,
/// which scripts ran, property values at end-of-frame, and physics tick counts.
#[derive(Debug, Clone, PartialEq)]
pub struct FrameRecord {
    /// The frame number (1-indexed).
    pub frame_number: u64,
    /// The delta time used for this frame.
    pub delta: f64,
    /// Number of physics ticks that ran during this frame.
    pub physics_ticks: u32,
    /// Cumulative process time after this frame.
    pub cumulative_process_time: f64,
    /// Cumulative physics time after this frame.
    pub cumulative_physics_time: f64,
    /// Whether the loop was paused during this frame.
    pub paused: bool,
    /// Trace events that occurred during this frame (notifications, script calls, etc.).
    pub events: Vec<TraceEvent>,
    /// Per-node property snapshots at the end of this frame.
    pub node_snapshots: Vec<NodeSnapshot>,
}

/// A complete frame-by-frame trace of scene execution.
///
/// Use [`MainLoop::run_frames_traced`] to produce this trace. Each entry
/// captures the full state evolution at that frame, enabling frame-by-frame
/// property evolution validation rather than only end-state checks.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FrameTrace {
    /// One [`FrameRecord`] per frame, in order.
    pub frames: Vec<FrameRecord>,
}

impl FrameTrace {
    /// Returns the total number of recorded frames.
    pub fn len(&self) -> usize {
        self.frames.len()
    }

    /// Returns true if no frames were recorded.
    pub fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }

    /// Returns notification details from all events across all frames, in order.
    pub fn all_notification_details(&self) -> Vec<&str> {
        self.frames
            .iter()
            .flat_map(|f| {
                f.events.iter().filter_map(|e| {
                    if e.event_type == TraceEventType::Notification {
                        Some(e.detail.as_str())
                    } else {
                        None
                    }
                })
            })
            .collect()
    }

    /// Returns events for a specific frame (0-indexed into the frames vec).
    pub fn frame_events(&self, index: usize) -> &[TraceEvent] {
        &self.frames[index].events
    }

    /// Returns the total number of physics ticks across all frames.
    pub fn total_physics_ticks(&self) -> u32 {
        self.frames.iter().map(|f| f.physics_ticks).sum()
    }
}

/// The avoidance parameters read off a `NavigationAgent2D` node for one tick
/// (pat-nzgdu.116).
///
/// Each field is `Option`: a property the scene never set leaves the agent's
/// existing value untouched, so a node that only sets `avoidance_enabled` keeps
/// Godot's documented defaults instead of being zeroed.
#[derive(Debug, Clone, Copy, Default)]
struct AvoidanceNodeParams {
    /// The desired velocity (Godot's `velocity`), defaulting to zero.
    velocity: gdcore::math::Vector2,
    radius: Option<f32>,
    max_speed: Option<f32>,
    neighbor_distance: Option<f32>,
    max_neighbors: Option<usize>,
    time_horizon_agents: Option<f32>,
    time_horizon_obstacles: Option<f32>,
    avoidance_layers: Option<u32>,
    avoidance_mask: Option<u32>,
    avoidance_priority: Option<f32>,
}

impl AvoidanceNodeParams {
    /// Reads the avoidance property surface off a node.
    fn read(node: &crate::node::Node) -> Self {
        /// Godot exposes these as floats, but a scene (or a `.tscn` round-trip)
        /// may hand back an int for a whole number.
        fn number(node: &crate::node::Node, name: &str) -> Option<f32> {
            match node.get_property(name) {
                Variant::Float(f) => Some(f as f32),
                Variant::Int(i) => Some(i as f32),
                _ => None,
            }
        }
        fn integer(node: &crate::node::Node, name: &str) -> Option<i64> {
            match node.get_property(name) {
                Variant::Int(i) => Some(i),
                Variant::Float(f) => Some(f as i64),
                _ => None,
            }
        }

        Self {
            velocity: match node.get_property("velocity") {
                Variant::Vector2(v) => v,
                _ => gdcore::math::Vector2::ZERO,
            },
            radius: number(node, "radius"),
            max_speed: number(node, "max_speed"),
            neighbor_distance: number(node, "neighbor_distance"),
            max_neighbors: integer(node, "max_neighbors").map(|i| i.max(0) as usize),
            time_horizon_agents: number(node, "time_horizon_agents"),
            time_horizon_obstacles: number(node, "time_horizon_obstacles"),
            avoidance_layers: integer(node, "avoidance_layers").map(|i| i as u32),
            avoidance_mask: integer(node, "avoidance_mask").map(|i| i as u32),
            avoidance_priority: number(node, "avoidance_priority"),
        }
    }

    /// Applies the read parameters onto `agent`, leaving unset ones alone.
    fn apply(&self, agent: &mut NavigationAgent2D) {
        // Gathering already filtered on the node's `avoidance_enabled`; the
        // backing agent must agree or the solve would pass the velocity through.
        agent.avoidance_enabled = true;
        agent.velocity = self.velocity;
        if let Some(v) = self.radius {
            agent.radius = v;
        }
        if let Some(v) = self.max_speed {
            agent.max_speed = v;
        }
        if let Some(v) = self.neighbor_distance {
            agent.neighbor_distance = v;
        }
        if let Some(v) = self.max_neighbors {
            agent.max_neighbors = v;
        }
        if let Some(v) = self.time_horizon_agents {
            agent.time_horizon_agents = v;
        }
        if let Some(v) = self.time_horizon_obstacles {
            agent.time_horizon_obstacles = v;
        }
        if let Some(v) = self.avoidance_layers {
            agent.avoidance_layers = v;
        }
        if let Some(v) = self.avoidance_mask {
            agent.avoidance_mask = v;
        }
        if let Some(v) = self.avoidance_priority {
            agent.avoidance_priority = v;
        }
    }
}

/// The `NavigationAgent3D` avoidance property surface (pat-0q2kx.97).
///
/// Separate from [`AvoidanceNodeParams`] rather than generic over the vector
/// type, because the 3D node carries three properties the 2D one has no
/// counterpart for — `height`, `use_3d_avoidance` and `keep_y_velocity` — and
/// they are exactly the ones that decide which solve runs.
struct AvoidanceNodeParams3D {
    velocity: gdcore::math::Vector3,
    radius: Option<f32>,
    height: Option<f32>,
    max_speed: Option<f32>,
    neighbor_distance: Option<f32>,
    max_neighbors: Option<usize>,
    time_horizon_agents: Option<f32>,
    time_horizon_obstacles: Option<f32>,
    avoidance_layers: Option<u32>,
    avoidance_mask: Option<u32>,
    avoidance_priority: Option<f32>,
    use_3d_avoidance: Option<bool>,
    keep_y_velocity: Option<bool>,
}

impl AvoidanceNodeParams3D {
    fn read(node: &crate::node::Node) -> Self {
        fn number(node: &crate::node::Node, name: &str) -> Option<f32> {
            match node.get_property(name) {
                Variant::Float(f) => Some(f as f32),
                Variant::Int(i) => Some(i as f32),
                _ => None,
            }
        }
        fn integer(node: &crate::node::Node, name: &str) -> Option<i64> {
            match node.get_property(name) {
                Variant::Int(i) => Some(i),
                Variant::Float(f) => Some(f as i64),
                _ => None,
            }
        }
        fn boolean(node: &crate::node::Node, name: &str) -> Option<bool> {
            match node.get_property(name) {
                Variant::Bool(b) => Some(b),
                _ => None,
            }
        }

        Self {
            velocity: match node.get_property("velocity") {
                Variant::Vector3(v) => v,
                _ => gdcore::math::Vector3::ZERO,
            },
            radius: number(node, "radius"),
            height: number(node, "height"),
            max_speed: number(node, "max_speed"),
            neighbor_distance: number(node, "neighbor_distance"),
            max_neighbors: integer(node, "max_neighbors").map(|i| i.max(0) as usize),
            time_horizon_agents: number(node, "time_horizon_agents"),
            time_horizon_obstacles: number(node, "time_horizon_obstacles"),
            avoidance_layers: integer(node, "avoidance_layers").map(|i| i as u32),
            avoidance_mask: integer(node, "avoidance_mask").map(|i| i as u32),
            avoidance_priority: number(node, "avoidance_priority"),
            use_3d_avoidance: boolean(node, "use_3d_avoidance"),
            keep_y_velocity: boolean(node, "keep_y_velocity"),
        }
    }

    fn apply(&self, agent: &mut NavigationAgent3D) {
        // Gathering already filtered on `avoidance_enabled`; the backing agent
        // must agree or the solve passes the velocity straight through.
        agent.avoidance_enabled = true;
        agent.velocity = self.velocity;
        if let Some(v) = self.radius {
            agent.radius = v;
        }
        if let Some(v) = self.height {
            agent.height = v;
        }
        if let Some(v) = self.max_speed {
            agent.max_speed = v;
        }
        if let Some(v) = self.neighbor_distance {
            agent.neighbor_distance = v;
        }
        if let Some(v) = self.max_neighbors {
            agent.max_neighbors = v;
        }
        if let Some(v) = self.time_horizon_agents {
            agent.time_horizon_agents = v;
        }
        if let Some(v) = self.time_horizon_obstacles {
            agent.time_horizon_obstacles = v;
        }
        if let Some(v) = self.avoidance_layers {
            agent.avoidance_layers = v;
        }
        if let Some(v) = self.avoidance_mask {
            agent.avoidance_mask = v;
        }
        if let Some(v) = self.avoidance_priority {
            agent.avoidance_priority = v;
        }
        if let Some(v) = self.use_3d_avoidance {
            agent.use_3d_avoidance = v;
        }
        if let Some(v) = self.keep_y_velocity {
            agent.keep_y_velocity = v;
        }
    }
}

/// Godot's particle nodes default `emitting` to true (pat-0q2kx.154).
fn emitting_flag(node: &crate::node::Node) -> bool {
    !matches!(node.get_property("emitting"), Variant::Bool(false))
}

/// Builds a [`ParticleEmitter3D`](crate::particle3d::ParticleEmitter3D) from a
/// `GPUParticles3D`/`CPUParticles3D` node's authored properties
/// (pat-0q2kx.154).
///
/// Reads `direction` and `gravity` as **Vector3**. The 2D path this replaced
/// matched only `Variant::Vector2`, so a 3D node's authored gravity fell
/// through and the emitter silently kept its default — the particles fell the
/// wrong way (or not at all) with no error anywhere.
fn read_emitter_3d(node: &crate::node::Node) -> crate::particle3d::ParticleEmitter3D {
    use gdcore::math::Vector3;
    let mut emitter = crate::particle3d::ParticleEmitter3D::default();
    if let Variant::Int(a) = node.get_property("amount") {
        if a > 0 {
            emitter.amount = a as u32;
        }
    }
    if let Some(l) = prop_f32(node.get_property("lifetime")) {
        if l > 0.0 {
            emitter.lifetime = l;
        }
    }
    if let Variant::Bool(o) = node.get_property("one_shot") {
        emitter.one_shot = o;
    }
    if let Some(e) = prop_f32(node.get_property("explosiveness")) {
        emitter.explosiveness = e;
    }
    if let Some(sp) = prop_f32(node.get_property("speed_scale")) {
        emitter.speed_scale = sp;
    }
    if let Variant::Vector3(d) = node.get_property("direction") {
        if d != Vector3::ZERO {
            emitter.material.direction = d;
        }
    }
    if let Variant::Vector3(g) = node.get_property("gravity") {
        emitter.material.gravity = g;
    }
    if let Some(sp) = prop_f32(node.get_property("spread")) {
        emitter.material.spread = sp;
    }
    if let Some(v) = prop_f32(node.get_property("initial_velocity_min")) {
        emitter.material.initial_velocity_min = v;
    }
    if let Some(v) = prop_f32(node.get_property("initial_velocity_max")) {
        emitter.material.initial_velocity_max = v;
    }
    if let Some(v) = prop_f32(node.get_property("scale_min")) {
        emitter.material.scale_min = v;
    }
    if let Some(v) = prop_f32(node.get_property("scale_max")) {
        emitter.material.scale_max = v;
    }
    // The authored `scale_curve` scales each particle over its lifetime; without
    // it every particle held its spawn size for its whole life (pat-s3rrv.119).
    if let Some(curve) = scale_curve_from_samples(node) {
        emitter.material.scale_curve = curve;
    }
    // The authored `color_ramp` wins when the scene has one: it is what makes a
    // particle's colour change over its lifetime, which a flat `color` cannot
    // express (pat-s3rrv.116).
    if let Some(curve) = color_ramp_curve(node) {
        emitter.material.color_curve = curve;
    } else if let Variant::Color(c) = node.get_property("color") {
        // `ColorCurve` is a gradient (`keys: Vec<(f32, Color)>`), not a
        // start/end pair — a flat authored `color` is the degenerate gradient.
        emitter.material.color_curve = crate::particle3d::ColorCurve::linear(c, c);
    }

    // ParticleProcessMaterial collision/attractor parameters (pat-s3rrv.216).
    // The .tscn flattener lands these on the node alongside `gravity`/`spread`,
    // so they are read exactly like every other material parameter here.
    if let Variant::Bool(enabled) = node.get_property("attractor_interaction_enabled") {
        emitter.material.attractor_interaction_enabled = enabled;
    }
    emitter.material.collision_mode = crate::particle3d::ParticleCollisionMode::from_godot(
        match node.get_property("collision_mode") {
            Variant::Int(v) => v,
            Variant::Float(f) => f as i64,
            _ => 0,
        },
    );
    if let Some(v) = prop_f32(node.get_property("collision_friction")) {
        emitter.material.collision_friction = v;
    }
    if let Some(v) = prop_f32(node.get_property("collision_bounce")) {
        emitter.material.collision_bounce = v;
    }
    if let Variant::Bool(v) = node.get_property("collision_use_scale") {
        emitter.material.collision_use_scale = v;
    }
    // Godot's collision radius comes from the emitter's `collision_base_size`.
    if let Some(v) = prop_f32(node.get_property("collision_base_size")) {
        emitter.material.particle_size = v;
    }
    if let Variant::Int(v) = node.get_property("visibility_layer") {
        emitter.material.cull_mask = v as u32;
    }
    emitter
}

/// Collects every `GPUParticlesAttractor*3D` and `GPUParticlesCollision*3D` in
/// the tree into the frame's [`ParticleField3D`] (pat-s3rrv.216).
///
/// Stage 1 covers the analytic Box and Sphere shapes; `VectorField3D`,
/// `HeightField3D` and `SDF3D` need baked textures and are follow-ups. An
/// unrecognised subclass is skipped rather than defaulted, so a follow-up shape
/// cannot silently behave like a box.
fn read_particle_field_3d(tree: &SceneTree) -> crate::particle3d::ParticleField3D {
    use crate::particle3d::{
        ParticleAttractor3D, ParticleCollider3D, ParticleField3D, ParticleVolume3D,
    };
    use gdcore::math::Vector3;

    let mut field = ParticleField3D::empty();
    for id in tree.all_nodes_in_tree_order() {
        let Some(node) = tree.get_node(id) else {
            continue;
        };
        let class = node.class_name();
        let is_attractor = class.starts_with("GPUParticlesAttractor");
        let is_collider = class.starts_with("GPUParticlesCollision");
        if !is_attractor && !is_collider {
            continue;
        }

        // Godot's defaults: size Vector3(2, 2, 2) and radius 1.0.
        let volume = if class.ends_with("Box3D") {
            let size = match node.get_property("size") {
                Variant::Vector3(s) => s,
                _ => Vector3::new(2.0, 2.0, 2.0),
            };
            ParticleVolume3D::Box {
                half_extents: size * 0.5,
            }
        } else if class.ends_with("Sphere3D") {
            ParticleVolume3D::Sphere {
                radius: prop_f32(node.get_property("radius")).unwrap_or(1.0),
            }
        } else {
            continue;
        };

        let transform = crate::node3d::get_global_transform(tree, id);
        let cull_mask = match node.get_property("cull_mask") {
            Variant::Int(v) => v as u32,
            _ => 0xFFFF_FFFF,
        };

        if is_attractor {
            field.attractors.push(ParticleAttractor3D {
                transform,
                volume,
                strength: prop_f32(node.get_property("strength")).unwrap_or(1.0),
                attenuation: prop_f32(node.get_property("attenuation")).unwrap_or(1.0),
                directionality: prop_f32(node.get_property("directionality")).unwrap_or(0.0),
                cull_mask,
            });
        } else {
            field.colliders.push(ParticleCollider3D {
                transform,
                volume,
                cull_mask,
            });
        }
    }
    field
}

/// Builds a [`ColorCurve`](crate::particle3d::ColorCurve) from the
/// `color_ramp` gradient `PackedScene` flattened onto the node.
///
/// Returns `None` when the scene authored no ramp, so the caller keeps its flat
/// `color` behaviour.
/// Builds a [`ScaleCurve`](crate::particle3d::ScaleCurve) from the `scale_curve`
/// samples `PackedScene` baked onto the node.
///
/// The samples are evenly spaced over the curve's domain — that is what
/// `bake_curve_samples` produces, and the same array shape Line2D's
/// `width_curve` uses — so the key times are `i / (n - 1)`.
///
/// Returns `None` when the scene authored no curve, leaving the emitter's
/// default (a flat 1.0) in place.
fn scale_curve_from_samples(node: &crate::node::Node) -> Option<crate::particle3d::ScaleCurve> {
    let samples: Vec<f32> = match node.get_property("__scale_curve_samples") {
        Variant::PackedFloat32Array(v) => v,
        Variant::PackedFloat64Array(v) => v.into_iter().map(|f| f as f32).collect(),
        _ => return None,
    };
    // ⚠️ A single sample has no span to spread over, and `i / (n - 1)` would
    // divide by zero. It is still a meaningful curve — a constant — so it maps
    // to one key rather than being refused.
    let keys = match samples.len() {
        0 => return None,
        1 => vec![(0.0, samples[0])],
        n => samples
            .into_iter()
            .enumerate()
            .map(|(i, v)| (i as f32 / (n - 1) as f32, v))
            .collect(),
    };
    Some(crate::particle3d::ScaleCurve { keys })
}

fn color_ramp_curve(node: &crate::node::Node) -> Option<crate::particle3d::ColorCurve> {
    let offsets: Vec<f32> = match node.get_property("__color_ramp_offsets") {
        Variant::PackedFloat32Array(v) => v,
        Variant::PackedFloat64Array(v) => v.into_iter().map(|f| f as f32).collect(),
        _ => return None,
    };
    let colors: Vec<gdcore::math::Color> = match node.get_property("__color_ramp_colors") {
        Variant::PackedColorArray(v) => v,
        _ => return None,
    };
    // ⚠️ Godot stores the stops as two PARALLEL arrays, so they are coupled by
    // their count: zipping a mismatched pair would silently drop the tail and
    // shift every colour onto the wrong offset. A malformed bag is refused
    // outright rather than half-read.
    if offsets.is_empty() || offsets.len() != colors.len() {
        return None;
    }
    let mut keys: Vec<(f32, gdcore::math::Color)> = offsets.into_iter().zip(colors).collect();
    // `ColorCurve::sample` walks the keys in order and short-circuits on the
    // first and last, so unsorted stops would read as a truncated gradient.
    // Godot keeps its stops sorted; a hand-written .tscn need not.
    keys.sort_by(|a, b| a.0.total_cmp(&b.0));
    Some(crate::particle3d::ColorCurve { keys })
}

/// Drives a [`SceneTree`] through a deterministic frame loop.
///
/// # Example
///
/// ```
/// use gdscene::main_loop::MainLoop;
/// use gdscene::scene_tree::SceneTree;
///
/// let tree = SceneTree::new();
/// let mut main_loop = MainLoop::new(tree);
/// main_loop.step(1.0 / 60.0);
/// assert_eq!(main_loop.frame_count(), 1);
/// ```
#[derive(Debug)]
pub struct MainLoop {
    /// The scene tree being driven.
    tree: SceneTree,
    /// Physics server bridging scene nodes to gdphysics2d.
    physics_server: PhysicsServer,
    /// Physics server bridging scene nodes to gdphysics3d (pat-u3l8c.1).
    physics_server_3d: PhysicsServer3D,
    /// Players whose voice an `Area2D` audio-bus override is currently holding
    /// off their own bus (pat-s3rrv.190).
    ///
    /// ⚠️ Needed because the redirect is applied to the LIVE VOICE
    /// (`AudioServer::redirect_stream_bus`), which nothing resets — the
    /// per-frame `sync_player_node` pass only re-stamps the *player's*
    /// configured bus, not the voice's. Without remembering who was overridden,
    /// a player walking out of a reverb zone would keep the reverb bus forever.
    area2d_bus_overridden: HashSet<NodeId>,
    /// Audio bridge driving AudioStreamPlayer nodes through gdaudio
    /// (pat-u3l8c.2).
    audio_bridge: AudioServerBridge,
    /// Number of physics ticks per second (default 60).
    physics_ticks_per_second: u32,
    /// Maximum number of physics steps allowed in a single frame (default 8).
    max_physics_steps_per_frame: u32,
    /// Total frames executed.
    frame_counter: u64,
    /// Total physics ticks executed, backing `Engine.get_physics_frames()`
    /// (pat-0q2kx.121).
    physics_tick_counter: u64,
    /// Last observed generation of script-side `Engine.physics_ticks_per_second`
    /// writes. A change means a script set the tick rate and this loop must
    /// adopt it (pat-0q2kx.121).
    engine_physics_tps_gen: u64,
    /// Total accumulated process (variable-timestep) time in seconds.
    process_time: f64,
    /// Total accumulated physics (fixed-timestep) time in seconds.
    physics_time: f64,
    /// Feeds `time/fps` and the process/physics timings into the process-wide
    /// [`gdcore::performance`] registry each frame (pat-s3rrv.109).
    performance_feed: gdcore::performance::FrameMonitorFeed,
    /// Time left over from physics stepping, carried into the next frame.
    physics_accumulator: f64,
    /// Double-buffered physics transforms for render-tick interpolation
    /// (pat-0q2kx.219).
    physics_interpolation: crate::physics_interpolation::PhysicsInterpolationState,
    /// The same, for 3D bodies (pat-0q2kx.250). Held separately from the 2D
    /// buffers because 3D blends rotation with `slerp` on a quaternion rather
    /// than along a scalar shortest arc.
    physics_interpolation_3d: crate::physics_interpolation_3d::PhysicsInterpolation3DState,
    /// `SceneTree.physics_interpolation_enabled`, from
    /// `physics/common/physics_interpolation`. Off by default, as in Godot.
    physics_interpolation_enabled: bool,
    /// Whether the main loop is currently paused.
    paused: bool,
    /// Engine-owned input state. Receives events via [`push_event`](Self::push_event)
    /// and produces the script-facing [`InputSnapshot`] each frame.
    input_state: InputState,
    /// The map the host installed at boot, kept so
    /// `InputMap.load_from_project_settings()` can restore it (pat-s3rrv.14).
    ///
    /// `None` when no host ever called [`MainLoop::set_input_map`] — a reload
    /// then does nothing, which is the honest answer: there are no project
    /// declarations to return to.
    project_input_map: Option<InputMap>,
    /// Engine-owned cursor state (pat-0q2kx.90). `gdplatform::cursor` had the
    /// full shape/custom-cursor model with green unit tests but **no owner** —
    /// nothing in the engine or the editor ever constructed one, so a script's
    /// cursor change had nowhere to land and the browser view had nothing to
    /// read. It lives here beside `input_state` because both are applied from
    /// the same drained command queue.
    cursor: gdplatform::cursor::CursorManager,
    /// Events pushed since the last [`step`](Self::step), awaiting dispatch
    /// through `SceneTree::push_input` (pat-nzgdu.83). Distinct from
    /// `input_state`, which only accumulates pollable state.
    pending_input: Vec<InputEvent>,
    /// CPU particle simulators for emitting particle nodes, keyed by node id
    /// (pat-u3l8c.4). Built on demand in [`process_particles`](Self::process_particles).
    particles: HashMap<NodeId, ParticleSimulator>,
    /// 3D particle simulators, keyed by node (pat-0q2kx.154).
    ///
    /// Separate from `particles` because `GPUParticles3D`/`CPUParticles3D` used
    /// to route through the 2D simulator, whose property reads match
    /// `Variant::Vector2` — so an authored `Vector3` gravity never matched and
    /// the node silently kept 2D defaults.
    particles_3d: HashMap<NodeId, crate::particle3d::ParticleSimulator3D>,
    /// Per-node 2D navigation agents, keyed by node id (pat-u3l8c.5). Hold the
    /// computed path + waypoint index across frames so agents advance smoothly.
    nav_agents_2d: HashMap<NodeId, NavigationAgent2D>,
    /// Per-node 3D navigation agents, keyed by node id (pat-u3l8c.5).
    nav_agents_3d: HashMap<NodeId, NavigationAgent3D>,
    /// `CollisionObject2D`s the pointer was inside on the previous pick pass
    /// (pat-0q2kx.199).
    ///
    /// Hover is a *transition*, so it cannot be derived from one frame's hit
    /// set alone — `mouse_entered`/`mouse_exited` fire only on the difference
    /// against this set. Godot keeps the same state on the viewport
    /// (`Viewport::_process_picking`'s `physics_2d_mouseover`).
    hovered_pickables: std::collections::HashSet<NodeId>,
    /// `CollisionObject3D`s the pointer was over on the previous 3D pick pass —
    /// the 3D twin of [`Self::hovered_pickables`] (pat-0q2kx.202). Godot keeps
    /// these separately too (`physics_object_over` vs `physics_2d_mouseover`):
    /// a pointer can be over a 2D Control *and* a 3D body at once, and merging
    /// the sets would make either one's exit cancel the other's enter.
    hovered_pickables_3d: std::collections::HashSet<NodeId>,
    /// Size of the surface mouse positions are expressed in, used to unproject
    /// a screen point through the current `Camera3D` (pat-0q2kx.202).
    ///
    /// Defaults to Godot's own `display/window/size/viewport_{width,height}`
    /// defaults, 1152x648. A host rendering at another size must call
    /// [`Self::set_viewport_size`], or every pick ray is aimed at the wrong
    /// part of the frustum — silently, since a wrong ray still hits *something*
    /// often enough to look plausible.
    viewport_size: (u32, u32),
    /// The active movie-capture session, if any (pat-s3rrv.262).
    movie_capture: Option<MovieCapture>,
}

/// An in-flight `MovieWriter` capture session (pat-s3rrv.262).
///
/// Godot turns this on with `--write-movie <path>`; the web target has no such
/// CLI, so a host starts it through [`MainLoop::start_movie_capture`] instead.
///
/// ⚠️ **The pixels are published by the host, not rendered here.** `MainLoop`
/// simulates — it has no renderer and `FrameOutput` carries no image — which
/// mirrors Godot, where the writer is fed by the rendering server rather than
/// by the main loop. The host calls [`MainLoop::set_movie_frame_pixels`] with
/// whatever it just drew; until it does, the frame is opaque black at the
/// capture size, so `_write_frame` still receives a correctly-sized image
/// rather than a null.
#[derive(Debug, Clone)]
struct MovieCapture {
    /// Path offered to `_handles_file` and passed to `_write_begin`.
    base_path: String,
    /// Frame size reported to `_write_begin` and used to size the image.
    size: (i64, i64),
    /// Frame rate reported to `_write_begin`.
    fps: i64,
    /// The writer chosen by `_handles_file`, resolved once at begin.
    ///
    /// `None` until the first frame. If no registered writer accepts the path,
    /// it stays `None` and nothing is driven — Godot likewise writes no movie
    /// when no writer handles the extension.
    writer: Option<NodeId>,
    /// Whether `_write_begin` has already run, so it runs exactly once.
    begun: bool,
    /// The most recent frame published by the host.
    pixels: Vec<gdcore::math::Color>,
}

/// Reads a numeric node property as `f32`, accepting either a `Float` or an
/// `Int` variant (`.tscn` stores whole numbers like `speed_scale = 2` as ints).
/// Returns `None` for any other variant so callers keep their existing default.
fn prop_f32(v: Variant) -> Option<f32> {
    match v {
        Variant::Float(f) => Some(f as f32),
        Variant::Int(i) => Some(i as f32),
        _ => None,
    }
}

/// Reads a [`gdcore::ProjectSettings`] entry as `f32`, accepting an
/// int-spelled value too — a `project.godot` writes `time_before_sleep=1`
/// without a decimal point (pat-0q2kx.116).
///
/// `None` when the key is absent, so a caller can tell "the project said 0.0"
/// apart from "the project said nothing" and keep its own default.
fn float_setting(settings: &gdcore::ProjectSettings, key: &str) -> Option<f32> {
    settings.get(key)?.as_float().map(|f| f as f32)
}

/// Godot's `NavigationRegion2D/3D.enabled` — a disabled region contributes
/// nothing to the navigation map. Absent means enabled, matching Godot's
/// default of `true` (pat-0q2kx.26).
fn region_enabled(node: &crate::node::Node) -> bool {
    match node.get_property("enabled") {
        Variant::Bool(b) => b,
        _ => true,
    }
}

/// A navigation region's polygons, as index lists into its vertex array.
///
/// Godot stores `polygons` as an array of `PackedInt32Array` index lists, so one
/// region is several polygons — that is what lets a path cross a concave or
/// multi-part region instead of cutting straight through it.
///
/// A resource carrying vertices but no `polygons` entry is treated as a single
/// polygon over all of them, which is what a hand-authored one-face mesh looks
/// like. Out-of-range indices are dropped rather than panicking: a malformed
/// `.tscn` should cost that polygon, not the frame.
impl MainLoop {
    /// Creates a new `MainLoop` that owns the given [`SceneTree`].
    pub fn new(tree: SceneTree) -> Self {
        // Godot starts the OS tick clock at engine init.
        gdplatform::os::start_clock();
        // NOTE: the `Engine` singleton state lives in process statics (scripts
        // can only reach the interpreter) and is deliberately NOT reset here —
        // in Godot the singleton outlives the main loop, and a script that set
        // `Engine.time_scale` in `_ready` runs before the loop is constructed
        // in several boot paths. Tests that need a clean slate call
        // `gdscript_interop::reset_engine_singleton_state()` (pat-0q2kx.121).
        Self {
            tree,
            physics_server: PhysicsServer::new(),
            physics_server_3d: PhysicsServer3D::new(),
            area2d_bus_overridden: HashSet::new(),
            audio_bridge: AudioServerBridge::new(),
            physics_ticks_per_second: 60,
            max_physics_steps_per_frame: 8,
            frame_counter: 0,
            physics_tick_counter: 0,
            engine_physics_tps_gen: gdscript_interop::engine_physics_ticks_per_second_generation(),
            process_time: 0.0,
            physics_time: 0.0,
            performance_feed: gdcore::performance::FrameMonitorFeed::new(),
            physics_accumulator: 0.0,
            physics_interpolation: crate::physics_interpolation::PhysicsInterpolationState::new(),
            physics_interpolation_3d:
                crate::physics_interpolation_3d::PhysicsInterpolation3DState::new(),
            physics_interpolation_enabled: false,
            paused: false,
            input_state: InputState::new(),
            project_input_map: None,
            cursor: gdplatform::cursor::CursorManager::new(),
            pending_input: Vec::new(),
            particles: HashMap::new(),
            particles_3d: HashMap::new(),
            nav_agents_2d: HashMap::new(),
            nav_agents_3d: HashMap::new(),
            hovered_pickables: std::collections::HashSet::new(),
            hovered_pickables_3d: std::collections::HashSet::new(),
            viewport_size: (1152, 648),
            movie_capture: None,
        }
    }

    /// Advances CPU particle simulators for every emitting particle node.
    ///
    /// `GPUParticles2D`/`GPUParticles3D`/`CPUParticles2D`/`CPUParticles3D`
    /// nodes each get (or reuse) a [`ParticleSimulator`] keyed by node id,
    /// configured from the node's `amount`/`lifetime`/`one_shot` properties and
    /// stepped by `dt`. The resulting live-particle count is written back to the
    /// node's `particle_count` property. Simulators for nodes that vanished or
    /// stopped emitting are dropped once their remaining particles die out.
    pub fn process_particles(&mut self, dt: f32) {
        use gdcore::math::{Color, Vector2};
        const PARTICLE_CLASSES: [&str; 2] = ["GPUParticles2D", "CPUParticles2D"];
        // 3D emitters go to the 3D simulator instead (pat-0q2kx.154). They used
        // to be in the list above, which meant they ran the 2D simulator and
        // read `direction`/`gravity` as `Variant::Vector2` — a `Vector3` never
        // matched, so an authored 3D gravity silently kept the 2D default.
        const PARTICLE_CLASSES_3D: [&str; 2] = ["GPUParticles3D", "CPUParticles3D"];

        // Collect every particle node's config (emitting flag + emitter params)
        // while the tree is borrowed immutably.
        let mut configs: Vec<(NodeId, bool, ParticleEmitter)> = Vec::new();
        let mut configs_3d: Vec<(NodeId, bool, crate::particle3d::ParticleEmitter3D)> = Vec::new();
        let mut present: std::collections::HashSet<NodeId> = std::collections::HashSet::new();
        let mut present_3d: std::collections::HashSet<NodeId> = std::collections::HashSet::new();
        // Resolved parent→child sub-emitter links for this frame (pat-0q2kx.233).
        let mut sub_chains: Vec<SubEmitterChain> = Vec::new();
        for id in self.tree.all_nodes_in_tree_order() {
            let Some(node) = self.tree.get_node(id) else {
                continue;
            };
            if PARTICLE_CLASSES_3D.contains(&node.class_name()) {
                present_3d.insert(id);
                let mut emitter_3d = read_emitter_3d(node);
                // The emitter's transform THIS frame, stamped onto every
                // particle spawned this frame — the 3D twin of the 2D line
                // below (pat-mgixk.70). `read_emitter_3d` takes only the node,
                // so it cannot resolve a global transform itself.
                emitter_3d.emitter_transform = crate::node3d::get_global_transform(&self.tree, id);
                configs_3d.push((id, emitting_flag(node), emitter_3d));
                continue;
            }
            if !PARTICLE_CLASSES.contains(&node.class_name()) {
                continue;
            }
            present.insert(id);
            // -- sub_emitter chain (pat-0q2kx.233) --
            // `sub_emitter` is a NodePath on the emitter node; the trigger mode
            // and amounts come from its ParticleProcessMaterial, which the .tscn
            // flattener lands on the node alongside `gravity`/`spread`/… so they
            // are read the same way as every other material parameter here.
            {
                let mode =
                    SubEmitterMode::from_godot_int(match node.get_property("sub_emitter_mode") {
                        Variant::Int(v) => v,
                        Variant::Float(f) => f as i64,
                        _ => 0,
                    });
                let path = match node.get_property("sub_emitter") {
                    Variant::NodePath(p) => p.to_string(),
                    Variant::String(s) => s,
                    _ => String::new(),
                };
                // A mode without a target (or a target that does not resolve) is
                // inert rather than an error, matching Godot.
                if mode != SubEmitterMode::Disabled && !path.is_empty() {
                    if let Some(child) = self.tree.get_node_or_null(id, &path) {
                        let int_prop = |key: &str, default: u32| -> u32 {
                            match node.get_property(key) {
                                Variant::Int(v) if v >= 0 => v as u32,
                                Variant::Float(f) if f >= 0.0 => f as u32,
                                _ => default,
                            }
                        };
                        sub_chains.push(SubEmitterChain {
                            parent: id,
                            child,
                            mode,
                            // Godot's ParticleProcessMaterial defaults.
                            amount_at_end: int_prop("sub_emitter_amount_at_end", 1),
                            amount_at_start: int_prop("sub_emitter_amount_at_start", 1),
                            frequency: match node.get_property("sub_emitter_frequency") {
                                Variant::Float(f) if f > 0.0 => f as f32,
                                Variant::Int(v) if v > 0 => v as f32,
                                _ => 4.0,
                            },
                            keep_velocity: matches!(
                                node.get_property("sub_emitter_keep_velocity"),
                                Variant::Bool(true)
                            ),
                        });
                    }
                }
            }
            // Godot's particle nodes default `emitting` to true.
            let is_emitting = !matches!(node.get_property("emitting"), Variant::Bool(false));
            let mut emitter = ParticleEmitter::default();
            if let Variant::Int(a) = node.get_property("amount") {
                if a > 0 {
                    emitter.amount = a as u32;
                }
            }
            if let Variant::Float(l) = node.get_property("lifetime") {
                if l > 0.0 {
                    emitter.lifetime = l as f32;
                }
            }
            emitter.one_shot = matches!(node.get_property("one_shot"), Variant::Bool(true));
            // pat-nzgdu.145: node-level draw order (0=Index / 1=Lifetime /
            // 2=ReverseLifetime) and fixed simulation rate. The emitter carried
            // these fields, but the node's properties were never read into it, so
            // setting them had no scene-level effect until now.
            if let Variant::Int(o) = node.get_property("draw_order") {
                emitter.draw_order = crate::particle::ParticleDrawOrder::from_godot_int(o);
            }
            if let Variant::Int(f) = node.get_property("fixed_fps") {
                if f >= 0 {
                    emitter.fixed_fps = f as u32;
                }
            }
            if let Some(e) = prop_f32(node.get_property("explosiveness")) {
                emitter.explosiveness = e.clamp(0.0, 1.0);
            }
            if let Some(s) = prop_f32(node.get_property("speed_scale")) {
                emitter.speed_scale = s;
            }
            emitter.local_coords = matches!(node.get_property("local_coords"), Variant::Bool(true));
            // The emitter's transform *this* frame, stamped onto every particle
            // spawned this frame so a `local_coords = false` cloud can stay in
            // global space once the emitter moves on (pat-0q2kx.51).
            emitter.emitter_transform = crate::node2d::get_global_transform(&self.tree, id);
            // CPUParticles2D-style process properties drive per-particle motion so
            // emitted particles actually travel across the frame. Godot exposes
            // these on the CPUParticles2D node directly; GPUParticles2D reads them
            // from its ParticleProcessMaterial, which we approximate with the same
            // node keys so an authored velocity is honored in both cases.
            // 🔑 Vector3 AND Vector2 (pat-mgixk.67). Godot's
            // `ParticleProcessMaterial.direction` and `.gravity` are Vector3 for
            // the 2D emitter as well as the 3D one
            // (`doc/classes/ParticleProcessMaterial.xml`), and `packed_scene`
            // flattens them onto the node UNCHANGED as `Variant::Vector3`
            // (pinned by `particle_process_material_flattens.rs`). This block
            // matched only `Vector2`, so on every editor-authored GPUParticles2D
            // the match silently fell through and the emitter used Patina's
            // defaults — gravity ZERO and direction UP. The 2D emitter takes
            // x,y, as Godot's does.
            //
            // Vector2 is still accepted: that is the CPUParticles2D spelling,
            // where these live on the node itself.
            let authored_direction = match node.get_property("direction") {
                Variant::Vector2(d) => Some(d),
                Variant::Vector3(d) => Some(Vector2::new(d.x, d.y)),
                _ => None,
            };
            if let Some(d) = authored_direction {
                if d != Vector2::ZERO {
                    emitter.material.direction = d;
                }
            }
            if let Some(sp) = prop_f32(node.get_property("spread")) {
                emitter.material.spread = sp;
            }
            if let Some(v) = prop_f32(node.get_property("initial_velocity_min")) {
                emitter.material.initial_velocity_min = v;
            }
            if let Some(v) = prop_f32(node.get_property("initial_velocity_max")) {
                emitter.material.initial_velocity_max = v;
            }
            // As above. ⚠️ No zero-guard here, deliberately matching the
            // previous code: an authored gravity of (0, 0) is a real value —
            // "this emitter is weightless" — and skipping it would leave
            // Patina's default in place instead.
            let authored_gravity = match node.get_property("gravity") {
                Variant::Vector2(g) => Some(g),
                Variant::Vector3(g) => Some(Vector2::new(g.x, g.y)),
                _ => None,
            };
            if let Some(g) = authored_gravity {
                emitter.material.gravity = g;
            }
            // Per-particle scale, flattened from the material's
            // `scale_min`/`scale_max`. Read only by the 3D emitter before this,
            // so a 2D emitter authoring scale 3 drew at 1 (pat-mgixk.67).
            if let Some(v) = prop_f32(node.get_property("scale_min")) {
                emitter.material.scale_min = v;
            }
            if let Some(v) = prop_f32(node.get_property("scale_max")) {
                emitter.material.scale_max = v;
            }
            // The `color_ramp` GradientTexture1D, flattened to
            // `__color_ramp_offsets` / `__color_ramp_colors`. Reusing the same
            // helper `read_emitter_3d` uses, so the two emitters cannot disagree
            // about what a ramp means.
            emitter.color_curve = color_ramp_curve(node);
            // ...and `scale_curve`, flattened to `__scale_curve_samples` by the
            // same pass, read with the same helper as the 3D emitter.
            emitter.scale_curve = scale_curve_from_samples(node);
            if let Variant::Color(c) = node.get_property("color") {
                emitter.material.start_color = c;
                emitter.material.end_color = c;
            }
            // 🔑 SDF collision parameters (pat-mgixk.69). The canvas SDF was
            // already built every frame below and `step_with_sdf` already called
            // with it — but this block never read `collision_mode`, so the
            // material kept its `Disabled` default and `resolve_sdf_collisions`
            // returned immediately. The field was rasterized and thrown away on
            // every frame of every scene.
            //
            // Read exactly the way `read_emitter_3d` reads them
            // (`main_loop.rs`, the `ParticleProcessMaterial` block): the `.tscn`
            // flattener lands these on the node alongside `gravity`/`spread`, and
            // `ParticleCollisionMode` is one shared enum for 2D and 3D.
            emitter.material.collision_mode = crate::particle3d::ParticleCollisionMode::from_godot(
                match node.get_property("collision_mode") {
                    Variant::Int(v) => v,
                    Variant::Float(f) => f as i64,
                    _ => 0,
                },
            );
            if let Some(v) = prop_f32(node.get_property("collision_friction")) {
                emitter.material.collision_friction = v;
            }
            if let Some(v) = prop_f32(node.get_property("collision_bounce")) {
                emitter.material.collision_bounce = v;
            }
            // Godot puts `collision_base_size` on the GPUParticles2D node, not on
            // the material; the 3D reader lands it on the material too
            // (as `particle_size`), so both paths keep the radius where the
            // simulator can see it.
            if let Some(v) = prop_f32(node.get_property("collision_base_size")) {
                emitter.material.collision_base_size = v;
            }
            configs.push((id, is_emitting, emitter));
        }

        // Drop simulators for nodes that no longer exist.
        self.particles.retain(|id, _| present.contains(id));

        // pat-s3rrv.250: build the canvas SDF once for the frame, from every
        // occluder marked `sdf_collision`. Godot rasterizes these into a distance
        // texture that GPUParticles2D collision samples; before this nothing
        // produced a field at all, so 2D particles passed through level geometry.
        //
        // Built BEFORE the loop, not per-simulator: it is one field per frame and
        // rebuilding it per emitter would be O(emitters) identical rasterizations.
        let canvas_sdf = {
            let occluders: Vec<(Vec<Vector2>, bool)> =
                crate::light2d_scene::collect_occluders_2d(&self.tree)
                    .into_iter()
                    .filter(|o| o.sdf_collision)
                    .map(|o| (o.points, o.closed))
                    .collect();
            if occluders.is_empty() {
                None
            } else {
                let (vw, vh) = self.viewport_size;
                let frame = Vector2::new(vw as f32, vh as f32);
                let root = self.tree.root_id();
                // ⚠️ These properties are ENUM INDICES, not multipliers.
                // `Viewport.sdf_oversize` defaults to 1, which is 120% — NOT 1.0x —
                // and `sdf_scale` defaults to 1, which is 50%. Passing the raw int
                // through as a factor would quietly build a 100%/100% field and look
                // entirely reasonable doing it.
                let oversize = match self
                    .tree
                    .get_node(root)
                    .map(|n| n.get_property("sdf_oversize"))
                {
                    Some(Variant::Int(i)) => gdrender2d::sdf2d::sdf_oversize_factor(i),
                    _ => gdrender2d::sdf2d::sdf_oversize_factor(1),
                };
                let scale = match self
                    .tree
                    .get_node(root)
                    .map(|n| n.get_property("sdf_scale"))
                {
                    Some(Variant::Int(i)) => gdrender2d::sdf2d::sdf_scale_factor(i),
                    _ => gdrender2d::sdf2d::sdf_scale_factor(1),
                };
                Some(gdrender2d::sdf2d::CanvasSdf::generate(
                    frame, oversize, scale, &occluders,
                ))
            }
        };

        // Step every particle node's simulator: emitting nodes spawn new
        // particles, non-emitting nodes just age their remaining particles out.
        // pat-nzgdu.145: nodes whose one_shot burst drains this frame get their
        // Godot `finished` signal emitted after the loop (emitting inside would
        // conflict with the `self.particles` borrow the loop holds).
        let mut finished_emitters: Vec<NodeId> = Vec::new();
        for (id, is_emitting, emitter) in configs {
            let sim = self
                .particles
                .entry(id)
                .or_insert_with(|| ParticleSimulator::new(emitter.clone()));
            // Sync config from the node each frame. Preserve one-shot emitters
            // that have already fired their full burst (don't re-enable them).
            let one_shot_finished = emitter.one_shot && sim.total_emitted >= emitter.amount as u64;
            let emitting = is_emitting && !one_shot_finished;
            // Replace the config wholesale (amount/lifetime/one_shot + material +
            // explosiveness/speed_scale/local_coords) so authored motion params
            // reach new particles, then restore the computed emitting flag.
            sim.emitter = emitter;
            sim.emitter.emitting = emitting;
            // The sub-emitter trigger has to be on the simulator before it steps,
            // since the AT_START/AT_END/CONSTANT events are recorded inside the
            // step (pat-0q2kx.233).
            let chain = sub_chains.iter().find(|c| c.parent == id);
            sim.sub_emitter_mode = chain.map_or(SubEmitterMode::Disabled, |c| c.mode);
            if let Some(c) = chain {
                sim.sub_emitter_frequency = c.frequency;
            }
            match &canvas_sdf {
                Some(sdf) => sim.step_with_sdf(dt, sdf),
                None => sim.step(dt),
            }
            // Drain the one-shot completion edge (fires exactly once per drain).
            if sim.take_finished() {
                finished_emitters.push(id);
            }
        }

        // -- sub-emitter hop (pat-0q2kx.233) --
        // Two passes, because `self.particles` is a map and parent and child are
        // separate entries: collect first, then inject. Draining the parent also
        // clears its queue, so an unresolvable child cannot accumulate a backlog
        // that fires later.
        //
        // The child's own `emitting` flag is deliberately ignored — Godot drives a
        // sub-emitter entirely from its parent, so a child left `emitting = false`
        // (the normal way to author one) still receives particles.
        let mut injections: Vec<(NodeId, Vec<(Vector2, Option<Vector2>)>)> = Vec::new();
        for chain in &sub_chains {
            let Some(parent_sim) = self.particles.get_mut(&chain.parent) else {
                continue;
            };
            let events = parent_sim.drain_sub_emissions();
            if events.is_empty() {
                continue;
            }
            // How many children each trigger spawns. CONSTANT emits one per
            // trigger; the burst modes use their authored amounts.
            let per_event = match chain.mode {
                SubEmitterMode::AtEnd => chain.amount_at_end,
                SubEmitterMode::AtStart => chain.amount_at_start,
                SubEmitterMode::Constant => 1,
                // Disabled/AtCollision record no events, so this is unreachable
                // in practice; 0 keeps it inert rather than guessing.
                _ => 0,
            };
            // A parent particle's position is in the PARENT emitter's space, and
            // the child simulates in its own. Godot converts through global
            // space, and so must this — otherwise a child node authored at the
            // origin (the normal case) would spawn its particles offset by the
            // parent's position, which looks like the feature working until you
            // move either node.
            let to_child = crate::node2d::get_global_transform(&self.tree, chain.child)
                .affine_inverse()
                * crate::node2d::get_global_transform(&self.tree, chain.parent);
            let mut spawns = Vec::new();
            for ev in events {
                let position = to_child.xform(ev.position);
                // Velocity is a direction: rotate/scale it but do not translate.
                let velocity = chain
                    .keep_velocity
                    .then(|| to_child.basis_xform(ev.velocity));
                for _ in 0..per_event {
                    spawns.push((position, velocity));
                }
            }
            if !spawns.is_empty() {
                injections.push((chain.child, spawns));
            }
        }
        for (child, spawns) in injections {
            // Only an existing simulator is fed: the child must itself be a
            // particle node that this pass already registered.
            if let Some(child_sim) = self.particles.get_mut(&child) {
                for (position, velocity) in spawns {
                    child_sim.inject_particle(position, velocity);
                }
            }
        }
        // Godot fires `finished` when a one_shot emitter completes; a connected
        // script's callback (or `.tscn` [connection]) runs here.
        for id in finished_emitters {
            self.tree.emit_signal(id, "finished", &[]);
        }

        // Publish live counts + per-particle draw data back onto the nodes so the
        // canvas render list can rasterize each emitter's live
        // particles as billboard quads. Positions/colors/scales are stored under
        // internal (`_`-prefixed) keys so they never leak into oracle property
        // dumps or the editor inspector.
        // Positions are published in the emitter's *current* local space, which
        // is what every renderer draws through the node's global transform. For
        // `local_coords = false` (Godot's default) that means undoing the
        // emitter's current transform on top of each particle's spawn transform,
        // so already-spawned particles hold their global position while the
        // emitter moves away from them (pat-0q2kx.51).
        let draws: Vec<(NodeId, Vec<(Vector2, Color, f32)>)> = self
            .particles
            .iter()
            .map(|(id, sim)| {
                let global = crate::node2d::get_global_transform(&self.tree, *id);
                (*id, sim.draw_commands_in_local_space(global))
            })
            .collect();
        // --- 3D simulators (pat-0q2kx.154) ---
        self.particles_3d.retain(|id, _| present_3d.contains(id));
        // Collected rather than emitted inside the loop: the loop holds a
        // mutable borrow of `self.particles_3d`, and `emit_signal` needs
        // `self.tree`. The 2D pass above is split for the same reason.
        let mut finished_emitters_3d: Vec<NodeId> = Vec::new();
        // The frame's attractors and colliders, collected once and shared by
        // every 3D emitter (pat-s3rrv.216). Built before the loop because the
        // loop borrows `self.particles_3d` mutably, so `&self.tree` cannot be
        // read inside it.
        let particle_field_3d = read_particle_field_3d(&self.tree);
        for (id, is_emitting, emitter) in configs_3d {
            let sim = self
                .particles_3d
                .entry(id)
                .or_insert_with(|| crate::particle3d::ParticleSimulator3D::new(emitter.clone()));
            // Re-read the authored config each frame so a property edited at
            // runtime takes effect, matching how the 2D path behaves.
            //
            // ⚠️ A drained one-shot must NOT be re-armed from the node. The
            // node's `emitting` property stays `true` for the whole scene —
            // nothing writes it back — so assigning it straight through resets
            // `emitting` every frame, and `is_complete()` (which requires
            // `!emitting`) can never become true. The simulator's own step sets
            // the flag false when the burst drains; this preserves that
            // decision. The 2D pass has computed the same suppression since
            // pat-nzgdu.145 (pat-s3rrv.111).
            let one_shot_finished = emitter.one_shot && sim.total_emitted >= emitter.amount as u64;
            let emitting = is_emitting && !one_shot_finished;
            sim.emitter = emitter;
            sim.emitter.emitting = emitting;
            sim.step_with_field(dt, &particle_field_3d);
            // Drain the one-shot completion edge (fires exactly once per drain).
            if sim.take_finished() {
                finished_emitters_3d.push(id);
            }
        }
        // Godot fires `finished` when a one_shot emitter completes. The 3D
        // simulator has carried `is_complete()` since pat-0q2kx.154 and nothing
        // ever read it, so a CPUParticles3D never emitted the signal its class
        // has always declared (pat-s3rrv.111).
        for id in finished_emitters_3d {
            self.tree.emit_signal(id, "finished", &[]);
        }
        let positions_3d: Vec<(NodeId, Vec<gdcore::math::Vector3>, Vec<Color>)> = self
            .particles_3d
            .iter()
            .map(|(id, sim)| {
                (
                    *id,
                    sim.active_particles.iter().map(|p| p.position).collect(),
                    sim.active_particles.iter().map(|p| p.color).collect(),
                )
            })
            .collect();
        for (id, positions, colors) in positions_3d {
            if let Some(node) = self.tree.get_node_mut(id) {
                node.set_property("particle_count", Variant::Int(positions.len() as i64));
                node.set_property(
                    "_particle_positions",
                    Variant::PackedVector3Array(positions),
                );
                node.set_property("_particle_colors", Variant::PackedColorArray(colors));
            }
        }

        for (id, cmds) in draws {
            if let Some(node) = self.tree.get_node_mut(id) {
                node.set_property("particle_count", Variant::Int(cmds.len() as i64));
                let positions: Vec<Vector2> = cmds.iter().map(|(p, _, _)| *p).collect();
                let colors: Vec<Color> = cmds.iter().map(|(_, c, _)| *c).collect();
                let scales: Vec<f32> = cmds.iter().map(|(_, _, s)| *s).collect();
                node.set_property(
                    "_particle_positions",
                    Variant::PackedVector2Array(positions),
                );
                node.set_property("_particle_colors", Variant::PackedColorArray(colors));
                node.set_property("_particle_scales", Variant::PackedFloat32Array(scales));
            }
        }
    }

    /// Returns the number of live particles currently simulated for `node_id`
    /// (0 if the node has no active particle simulator).
    pub fn particle_count(&self, node_id: NodeId) -> usize {
        if let Some(sim) = self.particles.get(&node_id) {
            return sim.active_particles.len();
        }
        // A 3D emitter lives in its own simulator (pat-0q2kx.154).
        self.particles_3d
            .get(&node_id)
            .map(|s| s.active_particles.len())
            .unwrap_or(0)
    }

    /// Live 3D particle positions for `node_id`, in world space
    /// (pat-0q2kx.154). Empty when the node has no 3D simulator.
    pub fn particle_positions_3d(&self, node_id: NodeId) -> Vec<gdcore::math::Vector3> {
        self.particles_3d
            .get(&node_id)
            .map(|s| s.active_particles.iter().map(|p| p.position).collect())
            .unwrap_or_default()
    }

    /// Drives `PathFollow2D`/`PathFollow3D` nodes along their parent Path's
    /// curve each frame (pat-u3l8c.6).
    ///
    /// The parent `Path2D`/`Path3D` exposes its baked polyline via a
    /// `curve_points` property (`PackedVector2Array` / `PackedVector3Array`).
    /// Each follow node's `progress` is advanced by `path_speed * dt` (a Patina
    /// convenience — Godot drives `progress` from animation/script) then
    /// projected onto the curve, updating the node's `position` and, per its
    /// rotation mode (`rotation_mode` for 3D, `rotates` for 2D), its rotation.
    /// Closes the audit gap where `PathFollow3D::advance`/`position_on` were
    /// never called from the loop and PathFollow2D was defaults-only.
    pub fn process_path_follows(&mut self, dt: f32) {
        use gdcore::math::{Vector2, Vector3};
        // PathFollow3D's tilt is a rotation about the tangent, which has to be
        // composed with the posture basis rather than added to an euler.
        use gdcore::math3d::{Basis, Quaternion};

        /// Shadow of the `progress` this loop last published, used to tell an
        /// external write apart from its own (pat-0q2kx.104). Underscore-
        /// prefixed like the engine's other internal node keys
        /// (`_move_and_slide_requested`).
        const PF_PUBLISHED_PROGRESS: &str = "_pathfollow_published_progress";
        /// Shadow of the last published `progress_ratio`.
        const PF_PUBLISHED_RATIO: &str = "_pathfollow_published_ratio";

        struct Update3 {
            id: NodeId,
            progress: f32,
            /// `progress / baked_length`, republished every tick — the 3D
            /// branch used to publish only `progress`, so a script reading
            /// `progress_ratio` off a PathFollow3D got `Nil` for ever.
            progress_ratio: f32,
            pos: Vector3,
            rot: Option<Vector3>,
        }
        struct Update2 {
            id: NodeId,
            progress: f32,
            /// `progress / baked_length`, republished every tick so the two
            /// views of the position never drift apart (pat-0q2kx.104).
            progress_ratio: f32,
            pos: Vector2,
            rot: Option<f32>,
        }
        let mut updates3: Vec<Update3> = Vec::new();
        let mut updates2: Vec<Update2> = Vec::new();

        /// A numeric node property as `f32`, defaulting to `0.0` when absent —
        /// Godot's default for both `h_offset` and `v_offset`. Local because
        /// the gather loop holds `&Node` directly; the crate's other
        /// `float_prop` helpers all take `(tree, id, key)` and would need a
        /// second borrow of the tree here.
        fn float_property(node: &crate::node::Node, name: &str) -> f32 {
            match node.get_property(name) {
                Variant::Float(f) => f as f32,
                Variant::Int(i) => i as f32,
                _ => 0.0,
            }
        }

        // Gather phase: read follow nodes + their parent curves (immutable).
        for id in self.tree.all_nodes_in_tree_order() {
            let Some(node) = self.tree.get_node(id) else {
                continue;
            };
            let is3 = node.class_name() == "PathFollow3D";
            let is2 = node.class_name() == "PathFollow2D";
            if !is3 && !is2 {
                continue;
            }
            let Some(parent_id) = node.parent() else {
                continue;
            };
            let Some(parent) = self.tree.get_node(parent_id) else {
                continue;
            };

            let progress = match node.get_property("progress") {
                Variant::Float(f) => f as f32,
                Variant::Int(i) => i as f32,
                _ => 0.0,
            };
            let speed = match node.get_property("path_speed") {
                Variant::Float(f) => f as f32,
                Variant::Int(i) => i as f32,
                _ => 0.0,
            };
            // Godot's PathFollow defaults `loop` to true.
            let loop_path = !matches!(node.get_property("loop"), Variant::Bool(false));

            if is3 {
                if parent.class_name() != "Path3D" {
                    continue;
                }
                let pts = match parent.get_property("curve_points") {
                    Variant::PackedVector3Array(v) => v,
                    _ => continue,
                };
                if pts.len() < 2 {
                    continue;
                }
                let mut curve = Curve3D::new();
                for p in &pts {
                    curve.add_point(*p);
                }
                // Baked index-for-index with `curve_points` by the loader, so a
                // length mismatch means one of the two is stale; drop the tilts
                // rather than rotate by an angle belonging to another point.
                if let Variant::PackedFloat32Array(t) = parent.get_property("curve_tilts") {
                    if t.len() == pts.len() {
                        curve.tilts = t;
                    }
                }
                if let Variant::PackedVector3Array(u) = parent.get_property("curve_up_vectors") {
                    if u.len() == pts.len() {
                        curve.up_vectors = u;
                    }
                }
                // The AUTHORED control points, in Godot's (in, out, position)
                // triple order (pat-s3rrv.91 item 5). Unlike the three arrays
                // above these are NOT index-aligned with the polyline and must
                // not be length-checked against it — there are a handful of
                // control points behind hundreds of baked samples. The guard is
                // that the array divides into whole triples; a partial one would
                // silently drop a handle and straighten the last segment.
                if let Variant::PackedVector3Array(cp) = parent.get_property("curve_control_points")
                {
                    if !cp.is_empty() && cp.len() % 3 == 0 {
                        curve.control = cp
                            .chunks_exact(3)
                            .map(|t| crate::bezier_bake::CurvePoint3 {
                                in_handle: t[0],
                                out_handle: t[1],
                                position: t[2],
                            })
                            .collect();
                    }
                }
                curve.closed = matches!(parent.get_property("curve_closed"), Variant::Bool(true));
                let baked_len = curve.baked_length();

                // The same reconciliation the 2D branch documents at length:
                // Godot has ONE value with two views (`progress_ratio ==
                // progress / baked_length`), Patina stores two properties, so
                // whichever no longer matches its shadow is the one the outside
                // world wrote. `progress` wins ties. The 3D branch published
                // neither shadow before, so writing `progress_ratio` from a
                // script did nothing at all here.
                let last_progress = float_property(node, PF_PUBLISHED_PROGRESS);
                let last_ratio = float_property(node, PF_PUBLISHED_RATIO);
                let progress = if (progress - last_progress).abs() > 1e-5 {
                    progress
                } else if baked_len > 1e-6 {
                    match node.get_property("progress_ratio") {
                        Variant::Float(_) | Variant::Int(_) => {
                            let ratio = float_property(node, "progress_ratio");
                            if (ratio - last_ratio).abs() > 1e-5 {
                                ratio * baked_len
                            } else {
                                progress
                            }
                        }
                        _ => progress,
                    }
                } else {
                    progress
                };

                // ⚠️ Godot's default is ROTATION_XYZ (3), NOT none
                // (`doc/classes/PathFollow3D.xml:41`), so an absent or
                // out-of-range property has to land on XYZ. The old `_ =>
                // None` arm meant a follower authored without an explicit mode
                // — the common case — did not rotate at all, and mode 4
                // (ORIENTED) silently fell into the same hole.
                let rotation_mode = match node.get_property("rotation_mode") {
                    Variant::Int(0) => PathFollowRotation::None,
                    Variant::Int(1) => PathFollowRotation::Y,
                    Variant::Int(2) => PathFollowRotation::Xy,
                    Variant::Int(4) => PathFollowRotation::Oriented,
                    _ => PathFollowRotation::Xyz,
                };
                let mut follow = PathFollow3D {
                    progress,
                    loop_path,
                    rotation_mode,
                };
                follow.advance(&curve, speed * dt);

                // Godot samples with `sample_baked(progress, cubic)`
                // (`path_3d.cpp:286, 289`). ⚠️ `cubic_interp` DEFAULTS TO TRUE
                // (`class_db.rs:11798`, `doc/classes/PathFollow3D.xml`), so an
                // absent property means CUBIC — reading it as false-by-default
                // would silently downgrade every scene that never set it, and
                // the two agree exactly on straight paths, so most fixtures
                // would never notice.
                let cubic = !matches!(node.get_property("cubic_interp"), Variant::Bool(false));
                let mut pos = if cubic {
                    curve.sample_baked_cubic(follow.progress)
                } else {
                    follow.position_on(&curve)
                };

                // Godot builds the frame with **-Z along the tangent**
                // (`sample_baked_with_rotation`, then `correct_posture` locks
                // axes in YXZ euler order — `path_3d.cpp:290-305, 371-400`).
                //
                // 🔑 The yaw here used to be `atan2(t.x, t.z)`, a "+Z is
                // forward" convention this file described as "internally
                // consistent". It is EXACTLY π away from Godot's on every
                // tangent, so every follower faced backwards along its own
                // path — and no test could see it, because the only
                // PathFollow3D coverage asserted position. The values below are
                // the YXZ euler of `Basis::looking_at(tangent)`, derived
                // analytically and checked against Godot's `get_euler` on eight
                // tangents.
                let tangent = curve.sample_tangent(follow.progress);
                // The posture frame: -Z along the tangent, +Y from the curve's
                // own baked UP VECTOR (world up when none was baked, which is
                // Godot's fallback and reproduces the flat-curve case exactly).
                //
                // ⚠️ Built as a frame rather than as `(pitch, yaw)` because the
                // up vector carries ROLL. The analytic two-angle form this
                // replaces assumed roll == 0, which is true only while up is
                // world up — i.e. on flat curves, where both forms agree.
                let up = curve.sample_baked_up_vector(follow.progress);
                let frame = looking_at_basis(tangent, up);
                // ROTATION_ORIENTED is Godot's `Basis::looking_at(tangent)` with
                // the DEFAULT up, not the baked one (`path_3d.cpp:381`), so it
                // is built separately rather than reusing the frame above.
                let euler = frame.to_euler();
                let (pitch, yaw, roll) = (euler.x, euler.y, euler.z);
                // Roll stays 0. It comes from the curve's baked UP VECTORS and
                // per-point tilt, neither of which is baked yet (the loader
                // documents skipping `tilts`). ⚠️ Y and XY are EXACT regardless
                // — `correct_posture` zeroes euler[2] for both — so only XYZ
                // and ORIENTED are approximate, and only in roll.
                // `correct_posture` locks axes on the YXZ euler of the frame
                // (`path_3d.cpp:371-400`): Y keeps only yaw, XY drops roll, XYZ
                // keeps everything. ⚠️ Y and XY are therefore EXACT whether or
                // not up vectors were baked — the up vector only ever shows up
                // in the roll term, which both of them zero.
                let mut rot = match rotation_mode {
                    PathFollowRotation::None => None,
                    PathFollowRotation::Y => Some(Vector3::new(0.0, yaw, 0.0)),
                    PathFollowRotation::Xy => Some(Vector3::new(pitch, yaw, 0.0)),
                    PathFollowRotation::Oriented => {
                        Some(looking_at_basis(tangent, Vector3::new(0.0, 1.0, 0.0)).to_euler())
                    }
                    PathFollowRotation::Xyz => Some(Vector3::new(pitch, yaw, roll)),
                };

                // `use_model_front` — Godot's "switch Z+ and Z-"
                // (`path_3d.cpp:293-296`):
                //
                //     if (use_model_front) {
                //         t.basis *= Basis::from_scale(Vector3(-1.0, 1.0, -1.0));
                //     }
                //
                // For a model authored facing +Z instead of Godot's -Z. Default
                // is FALSE (`doc/classes/PathFollow3D.xml`), unlike
                // `tilt_enabled`, so an absent property reads as off.
                //
                // ⚠️ ORDER IS LOAD-BEARING: after the rotation mode built the
                // basis and *before* the tilt twist. Godot puts it exactly
                // there, and it is not commutative with the twist — tilt
                // left-multiplies about the tangent while this right-multiplies
                // about the follower's own axes, so swapping them rolls the
                // model the wrong way on any tilted path.
                //
                // ⚠️ POST-multiplying by `diag(-1, 1, -1)` negates the basis's X
                // and Z COLUMNS, which is all this does. `det = +1`, so it stays
                // a rotation (a 180° turn about the frame's own Y) and survives
                // the round trip through euler angles; a reflection would not.
                //
                // ⚠️ Inside the `Some` arm on purpose: ROTATION_NONE publishes no
                // euler, and Godot's flip sits in the `else` of
                // `rotation_mode == ROTATION_NONE` too — there is no frame to
                // flip.
                if matches!(node.get_property("use_model_front"), Variant::Bool(true)) {
                    if let Some(euler) = rot {
                        let mut basis = Basis::from_euler(euler);
                        basis.x = -basis.x;
                        basis.z = -basis.z;
                        rot = Some(basis.to_euler());
                    }
                }

                // Godot applies tilt as a twist about the TANGENT, *after*
                // `correct_posture` and on the LEFT of the basis
                // (`path_3d.cpp:299-305`):
                //
                //     const Basis twist(tangent, tilt);
                //     t.basis = twist * t.basis;
                //
                // ⚠️ Left-multiplied, so it rotates the already-oriented frame
                // about the path rather than adding to the follower's own roll;
                // multiplying on the right would tilt about the follower's local
                // Z and drift as the path turns.
                //
                // ⚠️ `tilt_enabled` defaults to TRUE
                // (`doc/classes/PathFollow3D.xml`), so an absent property must
                // NOT read as off. ROTATION_NONE has no frame to twist, which is
                // Godot's behaviour too: the tilt branch sits inside the `else`
                // of `rotation_mode == ROTATION_NONE`.
                let tilt_enabled =
                    !matches!(node.get_property("tilt_enabled"), Variant::Bool(false));
                if tilt_enabled {
                    if let Some(euler) = rot {
                        let tilt = curve.sample_baked_tilt(follow.progress);
                        if tilt != 0.0 {
                            let twist =
                                Basis::from_quaternion(Quaternion::from_axis_angle(tangent, tilt));
                            rot = Some((twist * Basis::from_euler(euler)).to_euler());
                        }
                    }
                }

                // `h_offset`/`v_offset` are Godot's
                // `t.translate_local(Vector3(h_offset, v_offset, 0))`
                // (`path_3d.cpp:310`) — a shift along the frame's OWN x and y,
                // applied AFTER the posture correction, so a row of followers
                // stays perpendicular to the path instead of sliding along it.
                // With ROTATION_NONE the basis is the identity and the two
                // become plain world x/y, which is also Godot's behaviour.
                let h_offset = float_property(node, "h_offset");
                let v_offset = float_property(node, "v_offset");
                if h_offset != 0.0 || v_offset != 0.0 {
                    // Columns 0 and 1 of the FINAL basis — built from the euler
                    // that was actually published, so the tilt above is included.
                    // ⚠️ Derived through `Basis::from_euler` rather than from
                    // `(pitch, yaw)` analytically: once tilt can put a non-zero
                    // roll in there, a two-term expression that assumed roll == 0
                    // silently offsets in the wrong direction.
                    // `ROTATION_NONE` publishes no euler and gets the identity,
                    // which is Godot's `translate_local` against an unrotated
                    // transform.
                    let basis = Basis::from_euler(rot.unwrap_or(Vector3::ZERO));
                    pos = pos + basis.x * h_offset + basis.y * v_offset;
                }

                updates3.push(Update3 {
                    id,
                    progress: follow.progress,
                    progress_ratio: if baked_len > 1e-6 {
                        follow.progress / baked_len
                    } else {
                        0.0
                    },
                    pos,
                    rot,
                });
            } else {
                if parent.class_name() != "Path2D" {
                    continue;
                }
                let pts = match parent.get_property("curve_points") {
                    Variant::PackedVector2Array(v) => v,
                    _ => continue,
                };
                if pts.len() < 2 {
                    continue;
                }
                let mut curve = Curve2D::default();
                for p in &pts {
                    curve.add_point(*p);
                }
                let rotates = !matches!(node.get_property("rotates"), Variant::Bool(false));
                let baked_len = curve.baked_length();

                // `progress` and `progress_ratio` are two views of ONE value in
                // Godot (`progress_ratio == progress / baked_length`), so it
                // never has to decide between them. Patina stores them as two
                // separate node properties, which means this loop does — and
                // "whichever disagrees with the other wins" is not decidable
                // from their values alone.
                //
                // Resolved by remembering what the previous tick published: the
                // property that no longer matches its shadow is the one the
                // outside world (script, animation track, `.tscn`) wrote.
                // `progress` wins ties, since it is what `advance` operates on.
                //
                // ⚠️ Without the shadows, writing `progress` directly is undone
                // on the next tick by the stale ratio published before it.
                let last_progress = float_property(node, PF_PUBLISHED_PROGRESS);
                let last_ratio = float_property(node, PF_PUBLISHED_RATIO);
                let ratio_prop = node.get_property("progress_ratio");
                let progress = if (progress - last_progress).abs() > 1e-5 {
                    // `progress` was written externally: it wins.
                    progress
                } else if baked_len > 1e-6 {
                    match ratio_prop {
                        Variant::Float(_) | Variant::Int(_) => {
                            let ratio = float_property(node, "progress_ratio");
                            if (ratio - last_ratio).abs() > 1e-5 {
                                // `progress_ratio` was written externally.
                                ratio * baked_len
                            } else {
                                progress
                            }
                        }
                        _ => progress,
                    }
                } else {
                    progress
                };

                let mut follow = PathFollow2D {
                    progress,
                    loop_path,
                    rotates,
                };
                follow.advance(&curve, speed * dt);
                // Godot samples with `curve->sample_baked(progress, cubic_interp)`
                // (`path_2d.cpp`, `PathFollow2D::_update_transform`).
                //
                // ⚠️ `cubic_interp` DEFAULTS TO TRUE (`PathFollow2D.xml`, and
                // `classdb_2d_node_property_tables_match_godot.rs` asserts the
                // Bool(true) default), so an ABSENT property means CUBIC.
                // Reading it as false-by-default would silently downgrade every
                // scene that never set it — and the two agree exactly on a
                // straight path, so most fixtures would never notice. This is
                // the same wiring the 3D branch already uses.
                let cubic = !matches!(node.get_property("cubic_interp"), Variant::Bool(false));
                let mut pos = if cubic {
                    curve.sample_baked_cubic(follow.progress)
                } else {
                    follow.position_on(&curve)
                };
                let tangent = curve.sample_tangent(follow.progress);

                // `h_offset`/`v_offset` shift the sample off the curve.
                // Godot applies them in the *rotated* frame when `rotates` is
                // on (`path_2d.cpp`: `t.translate_local(Vector2(h, v))` on a
                // transform whose basis is tangent/normal) and in plain x/y
                // when it is off — so a spaced-out row of followers stays
                // perpendicular to the path rather than sliding along it.
                let h_offset = float_property(node, "h_offset");
                let v_offset = float_property(node, "v_offset");
                if h_offset != 0.0 || v_offset != 0.0 {
                    pos = if rotates {
                        let normal = Vector2::new(-tangent.y, tangent.x);
                        pos + tangent * h_offset + normal * v_offset
                    } else {
                        Vector2::new(pos.x + h_offset, pos.y + v_offset)
                    };
                }

                let rot = if rotates {
                    Some(tangent.y.atan2(tangent.x))
                } else {
                    None
                };
                updates2.push(Update2 {
                    id,
                    progress: follow.progress,
                    progress_ratio: if baked_len > 1e-6 {
                        follow.progress / baked_len
                    } else {
                        0.0
                    },
                    pos,
                    rot,
                });
            }
        }

        // Apply phase: write progress + transform back onto the nodes.
        for u in updates3 {
            if let Some(node) = self.tree.get_node_mut(u.id) {
                node.set_property("progress", Variant::Float(u.progress as f64));
                // Published unconditionally and with its shadows, exactly as
                // the 2D branch below does — otherwise the reconciliation above
                // has nothing to compare against and `progress_ratio` reads
                // `Nil` until something else writes it.
                node.set_property("progress_ratio", Variant::Float(u.progress_ratio as f64));
                node.set_property(PF_PUBLISHED_PROGRESS, Variant::Float(u.progress as f64));
                node.set_property(PF_PUBLISHED_RATIO, Variant::Float(u.progress_ratio as f64));
            }
            crate::node3d::set_position(&mut self.tree, u.id, u.pos);
            if let Some(euler) = u.rot {
                crate::node3d::set_rotation(&mut self.tree, u.id, euler);
            }
        }
        for u in updates2 {
            if let Some(node) = self.tree.get_node_mut(u.id) {
                node.set_property("progress", Variant::Float(u.progress as f64));
                // Published unconditionally, not only when it changed: a
                // getter that reads `progress_ratio` before it is ever written
                // gets `Nil` rather than the real position.
                node.set_property("progress_ratio", Variant::Float(u.progress_ratio as f64));
                // Shadows of what was just published, so the next tick can tell
                // which of the two an external writer touched. Written last and
                // always, so they can never disagree with the pair above.
                node.set_property(PF_PUBLISHED_PROGRESS, Variant::Float(u.progress as f64));
                node.set_property(PF_PUBLISHED_RATIO, Variant::Float(u.progress_ratio as f64));
            }
            crate::node2d::set_position(&mut self.tree, u.id, u.pos);
            if let Some(r) = u.rot {
                crate::node2d::set_rotation(&mut self.tree, u.id, r);
            }
        }
    }

    /// Runs the ORCA avoidance simulation over avoidance-enabled
    /// `NavigationAgent2D` nodes and delivers `velocity_computed(safe_velocity)`
    /// (pat-nzgdu.116).
    ///
    /// This follows Godot's avoidance contract: the server does **not** move the
    /// body. An agent whose node carries a desired `velocity` receives a
    /// collision-free `safe_velocity` — published onto the node as a property and
    /// passed as the `velocity_computed` argument — and the game moves in
    /// response. Runs on the physics tick before the physics-frame script phase,
    /// so a `_physics_process` handler reads the value solved for that same tick.
    ///
    /// `NavigationObstacle2D` nodes join the solve as non-reciprocal obstacles:
    /// they never yield, so agents shoulder the whole correction around them.
    pub fn process_navigation_avoidance(&mut self, dt: f32) {
        use gdcore::math::Vector2;

        // The 3D pass runs first because the 2D gather below returns early when
        // there are no 2D agents — and a 3D-only scene is exactly that case, so
        // hanging the 3D call off the end of this function would skip it in the
        // one situation it matters most (pat-0q2kx.97).
        self.process_navigation_avoidance_3d(dt);

        // -- gather (immutable tree read) --
        let mut agent_nodes: Vec<(NodeId, AvoidanceNodeParams)> = Vec::new();
        let mut obstacles: Vec<OrcaAgent> = Vec::new();

        for id in self.tree.all_nodes_in_tree_order() {
            let Some(node) = self.tree.get_node(id) else {
                continue;
            };
            match node.class_name() {
                "NavigationAgent2D" => {
                    if !matches!(node.get_property("avoidance_enabled"), Variant::Bool(true)) {
                        continue;
                    }
                    agent_nodes.push((id, AvoidanceNodeParams::read(node)));
                }
                "NavigationObstacle2D" => {
                    let radius = match node.get_property("radius") {
                        Variant::Float(f) => f as f32,
                        _ => 0.0,
                    };
                    if radius <= 0.0 {
                        continue;
                    }
                    // GLOBAL, not local (pat-mgixk.27). Godot's
                    // `NavigationObstacle2D` submits `get_global_position()`
                    // (`scene/2d/navigation/navigation_obstacle_2d.cpp`), and an
                    // obstacle is normally a CHILD of the body it guards, so its
                    // own `position` is an offset within the parent — or absent
                    // entirely, which `get_position` reports as ZERO.
                    let position = crate::node2d::get_global_position(&self.tree, id);
                    // max_speed is irrelevant for something that never moves
                    // under its own avoidance solve.
                    let mut obstacle = OrcaAgent::new(position, radius, 0.0);
                    if let Variant::Vector2(v) = node.get_property("velocity") {
                        obstacle.velocity = v;
                    }
                    if let Variant::Int(layers) = node.get_property("avoidance_layers") {
                        obstacle.avoidance_layers = layers as u32;
                    }
                    obstacles.push(obstacle);
                }
                _ => {}
            }
        }

        if agent_nodes.is_empty() {
            return;
        }

        // ⚠️ GLOBAL, not local (pat-mgixk.27). Godot's `NavigationAgent2D`
        // feeds the solver its PARENT's position every physics frame —
        // `agent_set_position(agent, agent_parent->get_global_position())`
        // (`scene/2d/navigation/navigation_agent_2d.cpp`,
        // NOTIFICATION_INTERNAL_PHYSICS_PROCESS).
        //
        // An agent is a plain `Node` child of a CharacterBody2D, so it has no
        // `position` property of its own and `node2d::get_position` returned
        // ZERO for every one of them: ORCA saw every agent stacked at the origin
        // and `velocity_computed` came back as garbage. `process_navigation`
        // already read the global position (see its 2D arm); only this
        // avoidance pass was left behind, so the two disagreed.
        //
        // It stayed hidden because the avoidance tests set `position` directly
        // on the agent node — a property a Godot agent never carries.
        let positions: Vec<Vector2> = agent_nodes
            .iter()
            .map(|(id, _)| crate::node2d::get_global_position(&self.tree, *id))
            .collect();

        // -- sync node properties onto the agents, then snapshot solver views --
        // Every view is taken BEFORE any solve, so each agent negotiates against
        // its neighbors' state at the top of this tick rather than against
        // whatever the earlier agents in iteration order were just given. Solving
        // against half-updated neighbors makes the result depend on tree order.
        let mut views: Vec<OrcaAgent> = Vec::with_capacity(agent_nodes.len());
        for ((id, params), position) in agent_nodes.iter().zip(&positions) {
            let agent = self
                .nav_agents_2d
                .entry(*id)
                .or_insert_with(NavigationAgent2D::new);
            params.apply(agent);
            views.push(agent.orca_agent(*position));
        }

        // -- solve --
        let mut results: Vec<(NodeId, Vector2)> = Vec::with_capacity(agent_nodes.len());
        for (index, (id, params)) in agent_nodes.iter().enumerate() {
            let neighbors: Vec<OrcaAgent> = views
                .iter()
                .enumerate()
                .filter(|(other, _)| *other != index)
                .map(|(_, view)| *view)
                .collect();
            let Some(agent) = self.nav_agents_2d.get(id) else {
                continue;
            };
            let safe = agent.compute_safe_velocity_against(
                positions[index],
                params.velocity,
                &neighbors,
                &obstacles,
                dt,
            );
            results.push((*id, safe));
        }

        // -- publish + signal --
        for (id, safe) in results {
            if let Some(agent) = self.nav_agents_2d.get_mut(&id) {
                agent.safe_velocity = safe;
            }
            if let Some(node) = self.tree.get_node_mut(id) {
                node.set_property("safe_velocity", Variant::Vector2(safe));
            }
            self.tree
                .emit_signal(id, "velocity_computed", &[Variant::Vector2(safe)]);
        }
    }

    /// The 3D half of the avoidance tick (pat-0q2kx.97).
    ///
    /// Godot runs RVO for 3D agents every physics frame in one of two modes
    /// (`doc/classes/NavigationAgent3D.xml`): the default 2D-layered solve on
    /// the XZ plane, where cylinders interact only when their vertical extents
    /// overlap, and full 3D spheres under `use_3d_avoidance`. Patina had both
    /// solvers but no caller — `process_navigation_avoidance` matched only the
    /// 2D classes, so every 3D agent path-stepped with an empty neighbour list
    /// and `velocity_computed` never fired for one.
    ///
    /// Structured exactly like the 2D pass above: gather, snapshot every view
    /// *before* any solve, then publish. Snapshotting first is what keeps the
    /// result independent of tree order — an agent must negotiate against its
    /// neighbours' state at the top of the tick, not against whatever the
    /// earlier agents were just assigned.
    fn process_navigation_avoidance_3d(&mut self, dt: f32) {
        use crate::navigation::NavigationObstacle3D;
        use gdcore::math::Vector3;

        // -- gather (immutable tree read) --
        let mut agent_nodes: Vec<(NodeId, AvoidanceNodeParams3D)> = Vec::new();
        let mut obstacles: Vec<NavigationObstacle3D> = Vec::new();

        for id in self.tree.all_nodes_in_tree_order() {
            let Some(node) = self.tree.get_node(id) else {
                continue;
            };
            match node.class_name() {
                "NavigationAgent3D" => {
                    if !matches!(node.get_property("avoidance_enabled"), Variant::Bool(true)) {
                        continue;
                    }
                    agent_nodes.push((id, AvoidanceNodeParams3D::read(node)));
                }
                "NavigationObstacle3D" => {
                    let radius = match node.get_property("radius") {
                        Variant::Float(f) => f as f32,
                        Variant::Int(i) => i as f32,
                        _ => 0.0,
                    };
                    if radius <= 0.0 {
                        continue;
                    }
                    let height = match node.get_property("height") {
                        Variant::Float(f) => f as f32,
                        Variant::Int(i) => i as f32,
                        // Godot's obstacle height defaults to 1.0.
                        _ => 1.0,
                    };
                    // GLOBAL, not local — see the 2D twin (pat-mgixk.27).
                    // `navigation_obstacle_3d.cpp` submits `get_global_position()`.
                    let position = crate::node3d::get_global_transform(&self.tree, id).origin;
                    let mut obstacle = NavigationObstacle3D::new(position, radius, height);
                    if let Variant::Vector3(v) = node.get_property("velocity") {
                        obstacle.velocity = v;
                    }
                    if let Variant::Int(layers) = node.get_property("avoidance_layers") {
                        obstacle.avoidance_layers = layers as u32;
                    }
                    obstacles.push(obstacle);
                }
                _ => {}
            }
        }

        if agent_nodes.is_empty() {
            return;
        }

        // GLOBAL, not local — see the 2D twin above (pat-mgixk.27).
        // `navigation_agent_3d.cpp` calls
        // `agent_set_position(agent, agent_parent->get_global_position())`.
        let positions: Vec<Vector3> = agent_nodes
            .iter()
            .map(|(id, _)| crate::node3d::get_global_transform(&self.tree, *id).origin)
            .collect();

        // -- sync node properties onto the agents, then snapshot solver views --
        // Each view carries the position and height the layered mode needs to
        // decide whether this pair overlaps vertically at all.
        let mut views: Vec<(Vector3, f32, crate::navigation::OrcaAgent)> =
            Vec::with_capacity(agent_nodes.len());
        for ((id, params), position) in agent_nodes.iter().zip(&positions) {
            let agent = self
                .nav_agents_3d
                .entry(*id)
                .or_insert_with(NavigationAgent3D::new);
            params.apply(agent);
            views.push((*position, agent.height, agent.orca_agent_xz(*position)));
        }

        // -- solve --
        let mut results: Vec<(NodeId, Vector3)> = Vec::with_capacity(agent_nodes.len());
        for (index, (id, _)) in agent_nodes.iter().enumerate() {
            let neighbors: Vec<(Vector3, f32, crate::navigation::OrcaAgent)> = views
                .iter()
                .enumerate()
                .filter(|(other, _)| *other != index)
                .map(|(_, view)| *view)
                .collect();
            let Some(agent) = self.nav_agents_3d.get(id) else {
                continue;
            };
            let safe = agent.solve_safe_velocity(positions[index], &neighbors, &obstacles, dt);
            results.push((*id, safe));
        }

        // -- publish + signal --
        for (id, safe) in results {
            if let Some(agent) = self.nav_agents_3d.get_mut(&id) {
                agent.safe_velocity = safe;
            }
            if let Some(node) = self.tree.get_node_mut(id) {
                node.set_property("safe_velocity", Variant::Vector3(safe));
            }
            self.tree
                .emit_signal(id, "velocity_computed", &[Variant::Vector3(safe)]);
        }
    }

    /// Drives `NavigationAgent2D`/`NavigationAgent3D` nodes along their computed
    /// paths each frame (pat-u3l8c.5).
    ///
    /// A local `NavigationServer2D`/`NavigationServer3D` is (re)built from every
    /// enabled `NavigationRegion2D`/`NavigationRegion3D` node — each contributes
    /// the polygons of its `NavigationPolygon`/`NavigationMesh` resource, which
    /// `PackedScene` resolves off the `.tscn` onto `__navmesh_vertices__` /
    /// `__navmesh_polygons__` (pat-0q2kx.26). A region is a *set* of indexed
    /// polygons, not one convex blob, so concave and multi-part regions route
    /// correctly. For each agent node the path is
    /// (re)computed only when its `target_position` property changes, then the
    /// agent is integrated toward its next waypoint (default max_speed) and the
    /// resulting `next_path_position` is published onto the node. Agents stop
    /// once within `target_desired_distance` of the target so they rest cleanly.
    /// Closes the audit gap where navigation.rs was never called from the loop.
    pub fn process_navigation(&mut self, dt: f32) {
        use gdcore::math::{Vector2, Vector3};

        let mut polys2: Vec<NavPolygon> = Vec::new();
        let mut polys3: Vec<NavPolygon3D> = Vec::new();
        let mut agents2: Vec<(NodeId, Vector2, Vector2)> = Vec::new();
        let mut agents3: Vec<(NodeId, Vector3, Vector3)> = Vec::new();
        // Off-mesh connections gathered from NavigationLink2D/3D nodes
        // (pat-0q2kx.98). The link-aware search already existed on both servers
        // but nothing ever filled these, so `links` was permanently empty and a
        // scene whose regions are joined only by a link could not path at all.
        // pat-s3rrv.180: obstacles never reached the query. `add_obstacle` had
        // no runtime caller at all, so the detour logic in
        // `navigation::NavigationServer2D` was dead code in a shipped engine.
        let mut obstacles2: Vec<crate::navigation::NavigationObstacle2D> = Vec::new();
        let mut links2: Vec<crate::navigation::NavigationLink2D> = Vec::new();
        let mut links3: Vec<crate::navigation::NavigationLink3D> = Vec::new();

        // Gather navmesh regions + agent nodes (immutable tree read).
        for id in self.tree.all_nodes_in_tree_order() {
            let Some(node) = self.tree.get_node(id) else {
                continue;
            };
            match node.class_name() {
                // A region contributes the polygons of its NavigationPolygon /
                // NavigationMesh resource, resolved from the .tscn by
                // `PackedScene` onto `__navmesh_vertices__` / `__navmesh_polygons__`
                // (pat-0q2kx.26). Godot uploads these on ENTER_TREE via
                // `region_set_navigation_polygon`, and a region is a *set* of
                // indexed polygons — reading one flat vertex array as a single
                // convex blob (what this did before) cannot represent an L-shape
                // or any concave region, and made cross-region paths impossible.
                "NavigationRegion2D" => {
                    if !region_enabled(node) {
                        continue;
                    }
                    let origin = crate::node2d::get_position(&self.tree, id);
                    if let Variant::PackedVector2Array(verts) =
                        node.get_property("__navmesh_vertices__")
                    {
                        for indices in navmesh_polygon_indices(node, verts.len()) {
                            let poly: Vec<Vector2> =
                                indices.iter().map(|&i| verts[i] + origin).collect();
                            if poly.len() >= 3 {
                                polys2.push(NavPolygon::new(poly));
                            }
                        }
                    }
                }
                // Every painted, navigation-enabled cell of a tile layer is its
                // own region (pat-0q2kx.99), exactly as Godot's
                // `TileMapLayer::_navigation_update` emits one region RID per
                // tile. `tilemap.rs` had zero navigation references, so
                // TileSet-authored navigation — a standard 2D Godot pattern —
                // produced nothing at runtime.
                "TileMapLayer" | "TileMap" => {
                    if !region_enabled(node) {
                        continue;
                    }
                    let nav_sources = match node.get_property(crate::tilemap::TILE_NAV_SOURCES) {
                        Variant::PackedInt32Array(ids) => ids,
                        _ => Vec::new(),
                    };
                    if nav_sources.is_empty() {
                        continue;
                    }
                    let tile_local = match node.get_property(crate::tilemap::TILE_NAV_POLYGON) {
                        Variant::PackedVector2Array(v) => v,
                        _ => Vec::new(),
                    };
                    let tile_size = match node.get_property("tile_size") {
                        Variant::Vector2i(v) => v,
                        Variant::Vector2(v) => gdcore::math::Vector2i::new(v.x as i32, v.y as i32),
                        // Godot's TileSet default.
                        _ => gdcore::math::Vector2i::new(16, 16),
                    };
                    let cells = match node.get_property("tile_map_data") {
                        Variant::PackedByteArray(bytes) => {
                            crate::tilemap::decode_tile_map_data(&bytes)
                        }
                        _ => Default::default(),
                    };
                    let origin = crate::node2d::get_global_position(&self.tree, id);
                    for poly in crate::tilemap::tile_navigation_polygons(
                        &cells,
                        tile_size,
                        &nav_sources,
                        &tile_local,
                        origin,
                    ) {
                        polys2.push(NavPolygon::new(poly));
                    }
                }
                "NavigationRegion3D" => {
                    if !region_enabled(node) {
                        continue;
                    }
                    let origin = crate::node3d::get_position(&self.tree, id);
                    if let Variant::PackedVector3Array(verts) =
                        node.get_property("__navmesh_vertices__")
                    {
                        for indices in navmesh_polygon_indices(node, verts.len()) {
                            let poly: Vec<Vector3> =
                                indices.iter().map(|&i| verts[i] + origin).collect();
                            if poly.len() >= 3 {
                                polys3.push(NavPolygon3D::new(poly));
                            }
                        }
                    }
                }
                // 🔑 Gated on `avoidance_enabled`, which defaults to TRUE — not on
                // `affect_navigation_mesh`, which defaults to false. The two
                // belong to different Godot subsystems and picking the wrong one
                // makes every plain obstacle inert: `affect_navigation_mesh`
                // discards source geometry inside the obstacle's `vertices`
                // during BAKING, while `radius` is the avoidance shape
                // (`doc/classes/NavigationObstacle2D.xml`). This detour is
                // radius-based, so `avoidance_enabled` is its gate.
                "NavigationObstacle2D" => {
                    let radius = match node.get_property("radius") {
                        Variant::Float(f) => f as f32,
                        Variant::Int(i) => i as f32,
                        _ => 0.0,
                    };
                    let enabled =
                        !matches!(node.get_property("avoidance_enabled"), Variant::Bool(false));
                    // A zero radius is Godot's default and blocks nothing; adding
                    // it would put a degenerate circle on the map that every
                    // segment test still has to consider.
                    if enabled && radius > 0.0 {
                        let origin = crate::node2d::get_global_position(&self.tree, id);
                        obstacles2
                            .push(crate::navigation::NavigationObstacle2D::new(origin, radius));
                    }
                }
                // A link is an off-mesh connection between two points on the map
                // (pat-0q2kx.98). Godot's `get_global_start_position()` is the
                // node's global transform applied to `start_position`, so the
                // endpoints are read relative to the node rather than as world
                // coordinates — moving the link node moves both ends.
                "NavigationLink2D" => {
                    if !matches!(node.get_property("enabled"), Variant::Bool(false)) {
                        let origin = crate::node2d::get_global_position(&self.tree, id);
                        let read = |key: &str| match node.get_property(key) {
                            Variant::Vector2(v) => v,
                            _ => Vector2::ZERO,
                        };
                        let mut link = crate::navigation::NavigationLink2D::new(
                            origin + read("start_position"),
                            origin + read("end_position"),
                        );
                        link.bidirectional =
                            !matches!(node.get_property("bidirectional"), Variant::Bool(false));
                        links2.push(link);
                    }
                }
                "NavigationLink3D" => {
                    if !matches!(node.get_property("enabled"), Variant::Bool(false)) {
                        let xform = crate::node3d::get_global_transform(&self.tree, id);
                        let read = |key: &str| match node.get_property(key) {
                            Variant::Vector3(v) => v,
                            _ => Vector3::ZERO,
                        };
                        let mut link = crate::navigation::NavigationLink3D::new(
                            xform.xform(read("start_position")),
                            xform.xform(read("end_position")),
                        );
                        link.bidirectional =
                            !matches!(node.get_property("bidirectional"), Variant::Bool(false));
                        links3.push(link);
                    }
                }
                "NavigationAgent2D" => {
                    // GLOBAL, not local (pat-s3rrv.20). Godot's agent has no
                    // transform of its own and simply rides its parent body, so
                    // the global position is what makes path progress follow the
                    // body. Reading the LOCAL position pinned every agent at its
                    // authored offset — `(0,0)` for the standard "agent is a
                    // child of the body" setup — which computed the path from the
                    // world origin and handed `get_next_path_position` a value
                    // that never changed, so no waypoint was ever consumed.
                    //
                    // This was masked while the agent wrote its own simulated
                    // position back onto itself: that write moved the local
                    // position each frame. Removing the self-write made the
                    // latent bug reachable.
                    let current = crate::node2d::get_global_position(&self.tree, id);
                    let target = match node.get_property("target_position") {
                        Variant::Vector2(v) => v,
                        _ => Vector2::ZERO,
                    };
                    agents2.push((id, current, target));
                }
                "NavigationAgent3D" => {
                    // GLOBAL, not local — see the 2D twin above (pat-s3rrv.20).
                    let current = crate::node3d::get_global_transform(&self.tree, id).origin;
                    let target = match node.get_property("target_position") {
                        Variant::Vector3(v) => v,
                        _ => Vector3::ZERO,
                    };
                    agents3.push((id, current, target));
                }
                _ => {}
            }
        }

        let mut server2 = NavigationServer2D::new();
        if !polys2.is_empty() {
            server2.add_region(NavMesh2D::new(polys2, 0.5));
        }
        // After the region, because an obstacle is a modifier on the map the
        // region defines — `NavigationServer2D::find_path` consults them while
        // walking the polygons it just registered.
        for obstacle in obstacles2 {
            server2.add_obstacle(obstacle);
        }
        let mut server3 = NavigationServer3D::new();
        if !polys3.is_empty() {
            let conns = Self::nav3d_connections(&polys3, 0.5);
            server3.add_region(NavMesh3D::new(polys3, conns));
        }
        // Links go on after the regions: both servers resolve a link's endpoints
        // to the nearest polygon of the already-registered geometry
        // (pat-0q2kx.98).
        for link in links2 {
            server2.add_link(link);
        }
        for link in links3 {
            server3.add_link(link);
        }

        // Drop agents whose node vanished.
        let present2: std::collections::HashSet<NodeId> =
            agents2.iter().map(|(id, _, _)| *id).collect();
        let present3: std::collections::HashSet<NodeId> =
            agents3.iter().map(|(id, _, _)| *id).collect();
        self.nav_agents_2d.retain(|id, _| present2.contains(id));
        self.nav_agents_3d.retain(|id, _| present3.contains(id));

        // Step 2D agents.
        // `(agent node, next_path_position)` — the agent's own transform is no
        // longer written, so no position rides along (pat-s3rrv.20).
        let mut updates2: Vec<(NodeId, Vector2)> = Vec::new();
        // Nodes whose agent reached its goal this frame (emit signals once).
        let mut finished2: Vec<NodeId> = Vec::new();
        // Neighbor snapshot for path-following avoidance (pat-nzgdu.116). This
        // was previously a hardcoded empty slice, which left avoidance inert at
        // runtime no matter how an agent was configured: no agent was ever told
        // another existed. Radii come from each agent's own value, defaulting
        // for nodes not yet stepped.
        let neighbor_radii: Vec<(NodeId, Vector2, f32)> = agents2
            .iter()
            .map(|(id, current, _)| {
                let radius = self
                    .nav_agents_2d
                    .get(id)
                    .map(|agent| agent.radius)
                    .unwrap_or_else(|| NavigationAgent2D::new().radius);
                (*id, *current, radius)
            })
            .collect();
        // Copy the scene-authored tuning onto each backing agent before it is
        // stepped (pat-s3rrv.20). `process_navigation` read only
        // `target_position`, so a .tscn setting `path_desired_distance` or
        // `target_desired_distance` was inert during path following —
        // `AvoidanceNodeParams::apply` synced some of these, but only in the
        // avoidance phase and only when `avoidance_enabled` was true.
        //
        // A separate pass, not inside the loop below: that loop holds `&mut` on
        // `nav_agents_2d` via `entry()`, which cannot coexist with the
        // `&self.tree` needed to read the node's properties.
        for (id, _, _) in &agents2 {
            let mut agent = self
                .nav_agents_2d
                .remove(id)
                .unwrap_or_else(NavigationAgent2D::new);
            crate::navigation_agents::sync_tuning_2d(&self.tree, *id, &mut agent);
            self.nav_agents_2d.insert(*id, agent);
        }
        for (id, current, target) in &agents2 {
            // Every other agent this frame — an agent must not avoid itself.
            let others: Vec<(Vector2, f32)> = neighbor_radii
                .iter()
                .filter(|(other_id, _, _)| other_id != id)
                .map(|(_, position, radius)| (*position, *radius))
                .collect();
            let agent = self
                .nav_agents_2d
                .entry(*id)
                .or_insert_with(NavigationAgent2D::new);
            // Recompute the path only when the target changes (or none exists),
            // preserving waypoint progress across frames.
            if agent.path.is_empty() || (agent.target_position - *target).length() > 0.01 {
                agent.set_target_position(*target, *current, &server2);
            }
            // ⚠️ The agent is PASSIVE (pat-s3rrv.20). Godot's NavigationAgent2D
            // never moves anything: it is a child of the body, it follows the
            // body's transform, and it only *publishes* where to go next. The
            // game moves the body —
            //   `velocity = (agent.get_next_path_position() - global_position) …`
            //   `move_and_slide()`
            // — which is precisely what this bead's acceptance drives.
            //
            // This used to run `agent.step(..)`, an internal simulation of the
            // agent walking its own path, and then write that simulated point
            // back onto the agent node with `node2d::set_position`. Two things
            // broke: the agent node teleported away from its authored position
            // (and away from its parent body), and `next_path_position` was
            // computed from the *simulated* point rather than the real one — so
            // it advanced on its own schedule regardless of whether the body
            // had actually moved.
            //
            // `get_next_path_position` is already the whole mechanism: it
            // consumes waypoints against the position handed to it. Handing it
            // the agent's real position makes progress follow the body, which
            // is Godot's behaviour.
            let next = agent.get_next_path_position(*current);
            // Fire `navigation_finished`/`target_reached` exactly once, the frame
            // the agent first reaches within `target_desired_distance` of the goal.
            let arrived_now = agent.is_navigation_finished()
                || (agent.target_position - *current).length() <= agent.target_desired_distance;
            if arrived_now && !agent.finished_emitted {
                agent.finished_emitted = true;
                finished2.push(*id);
            }
            updates2.push((*id, next));
        }
        for (id, next) in updates2 {
            // Publish only. The agent node's own transform is left alone.
            if let Some(node) = self.tree.get_node_mut(id) {
                node.set_property("next_path_position", Variant::Vector2(next));
            }
        }
        // Publish the rest of the frame's agent state for the script-facing
        // getters, and fire the milestone signals NavAgentSignal has produced
        // since pat-u3l8c.64 with no consumer (pat-s3rrv.20). remove/insert
        // because both calls need `&mut self.tree` while the agent is borrowed.
        for (id, _, _) in &agents2 {
            if let Some(mut agent) = self.nav_agents_2d.remove(id) {
                crate::navigation_agents::publish_state_2d(&mut self.tree, *id, &agent);
                crate::navigation_agents::drain_and_emit_2d(&mut self.tree, *id, &mut agent);
                self.nav_agents_2d.insert(*id, agent);
            }
        }
        for id in finished2 {
            self.tree.emit_signal(id, "target_reached", &[]);
            self.tree.emit_signal(id, "navigation_finished", &[]);
        }

        // Step 3D agents.
        // `(agent node, next_path_position)` — see the 2D twin (pat-s3rrv.20).
        let mut updates3: Vec<(NodeId, Vector3)> = Vec::new();
        let mut finished3: Vec<NodeId> = Vec::new();
        // 3D twin of the tuning sync above (pat-s3rrv.20).
        for (id, _, _) in &agents3 {
            let mut agent = self
                .nav_agents_3d
                .remove(id)
                .unwrap_or_else(NavigationAgent3D::new);
            crate::navigation_agents::sync_tuning_3d(&self.tree, *id, &mut agent);
            self.nav_agents_3d.insert(*id, agent);
        }
        for (id, current, target) in &agents3 {
            let agent = self
                .nav_agents_3d
                .entry(*id)
                .or_insert_with(NavigationAgent3D::new);
            if agent.path.is_empty() || (agent.target_position - *target).length() > 0.01 {
                agent.set_target_position(*target, *current, &server3);
            }
            // Passive agent, same as the 2D path above (pat-s3rrv.20): no
            // self-simulation and no write to the agent's own transform. The
            // body moves; `get_next_path_position` consumes waypoints against
            // the agent's real position.
            let next = agent.get_next_path_position(*current);
            let arrived_now = agent.is_navigation_finished()
                || (agent.target_position - *current).length() <= agent.target_desired_distance;
            if arrived_now && !agent.finished_emitted {
                agent.finished_emitted = true;
                finished3.push(*id);
            }
            updates3.push((*id, next));
        }
        for (id, next) in updates3 {
            // Publish only. The agent node's own transform is left alone.
            if let Some(node) = self.tree.get_node_mut(id) {
                node.set_property("next_path_position", Variant::Vector3(next));
            }
        }
        // 3D twin of the publish pass above. There is no signal drain here:
        // `NavigationAgent3D` has no `pending_signals` field at all, so it
        // produces no milestones to emit (reported as remaining scope rather
        // than papered over with a silent no-op).
        for (id, _, _) in &agents3 {
            if let Some(agent) = self.nav_agents_3d.remove(id) {
                crate::navigation_agents::publish_state_3d(&mut self.tree, *id, &agent);
                self.nav_agents_3d.insert(*id, agent);
            }
        }
        for id in finished3 {
            self.tree.emit_signal(id, "target_reached", &[]);
            self.tree.emit_signal(id, "navigation_finished", &[]);
        }
    }

    /// Shared-edge adjacency for 3D navmesh polygons (connected when they share
    /// >= 2 vertices within `tol`), mirroring NavMesh2D's auto-connection since
    /// `NavMesh3D::new` takes explicit connections.
    fn nav3d_connections(polys: &[NavPolygon3D], tol: f32) -> Vec<Vec<usize>> {
        let n = polys.len();
        let tol_sq = tol * tol;
        let mut conns = vec![Vec::new(); n];
        for i in 0..n {
            for j in (i + 1)..n {
                let mut shared = 0;
                let mut connected = false;
                'outer: for va in &polys[i].vertices {
                    for vb in &polys[j].vertices {
                        if (*va - *vb).length_squared() < tol_sq {
                            shared += 1;
                            if shared >= 2 {
                                connected = true;
                                break 'outer;
                            }
                        }
                    }
                }
                if connected {
                    conns[i].push(j);
                    conns[j].push(i);
                }
            }
        }
        conns
    }

    /// Number of path waypoints currently held by a 2D navigation agent node
    /// (0 if it has no active agent) (pat-u3l8c.5).
    pub fn nav_agent_path_len_2d(&self, node_id: NodeId) -> usize {
        self.nav_agents_2d
            .get(&node_id)
            .map(|a| a.path.len())
            .unwrap_or(0)
    }

    /// Number of path waypoints currently held by a 3D navigation agent node.
    pub fn nav_agent_path_len_3d(&self, node_id: NodeId) -> usize {
        self.nav_agents_3d
            .get(&node_id)
            .map(|a| a.path.len())
            .unwrap_or(0)
    }

    /// Drives every `RayCast3D` / `ShapeCast3D` / `SpringArm3D` node each frame:
    /// re-runs its query against the (already-synced) 3D physics world and
    /// publishes the `_hit_*` result properties that scripts read (pat-nzgdu.12).
    ///
    /// Godot updates these query nodes every physics frame; before this nothing
    /// called the (already-implemented) `force_*_update` / `process` functions, so
    /// `is_colliding()` never became true at runtime without a manual force call.
    /// Each query self-guards on the node's `enabled` flag, so disabled nodes are
    /// cleared. Runs after the physics phase so the world reflects this frame's
    /// body positions.
    pub fn process_queries_3d(&mut self) {
        // Collect ids first (immutable tree read), then update (needs &mut tree).
        let mut raycasts: Vec<NodeId> = Vec::new();
        let mut shapecasts: Vec<NodeId> = Vec::new();
        let mut springarms: Vec<NodeId> = Vec::new();
        for id in self.tree.all_nodes_in_tree_order() {
            let Some(node) = self.tree.get_node(id) else {
                continue;
            };
            match node.class_name() {
                "RayCast3D" => raycasts.push(id),
                "ShapeCast3D" => shapecasts.push(id),
                "SpringArm3D" => springarms.push(id),
                _ => {}
            }
        }
        for id in raycasts {
            crate::raycast3d::force_raycast_update(&mut self.tree, &self.physics_server_3d, id);
        }
        for id in shapecasts {
            crate::shapecast3d::force_shapecast_update(&mut self.tree, &self.physics_server_3d, id);
        }
        for id in springarms {
            crate::springarm3d::process(&mut self.tree, &self.physics_server_3d, id);
        }
    }

    /// Casts every enabled `RayCast2D` node against the 2D physics world and
    /// publishes `is_colliding` (Bool) and `collider` (the hit node's id, or Nil)
    /// back onto the node (pat-u3l8c.49).
    ///
    /// The ray starts at the node's position and extends along its
    /// `target_position` (Godot default `(0, 50)`); it must run after the
    /// physics phase so the world reflects this frame's body positions.
    pub fn process_raycasts(&mut self) {
        // Collect ids first (immutable tree read), then update (needs &mut
        // tree) — the same shape as `process_queries_3d`.
        //
        // The per-node work lives in `raycast2d::force_raycast_update` so the
        // scripted `force_raycast_update()` and this per-frame pass run the
        // identical query. This loop previously inlined a cast from the node's
        // LOCAL position along the raw `target_position`, using the unfiltered
        // `world.raycast`, publishing property names the script bridge does not
        // read (pat-0q2kx.32).
        let mut raycasts: Vec<NodeId> = Vec::new();
        for id in self.tree.all_nodes_in_tree_order() {
            let Some(node) = self.tree.get_node(id) else {
                continue;
            };
            if node.class_name() == "RayCast2D" {
                raycasts.push(id);
            }
        }
        for id in raycasts {
            crate::raycast2d::force_raycast_update(&mut self.tree, &self.physics_server, id);
        }
    }

    /// Sweeps every `ShapeCast2D` (pat-s3rrv.87).
    ///
    /// Godot updates a ShapeCast2D under
    /// `NOTIFICATION_INTERNAL_PHYSICS_PROCESS`, so this sits beside
    /// [`Self::process_raycasts`] and shares its shape: collect ids from an
    /// immutable read, then drive each one through the same entry point the
    /// scripted `force_shapecast_update()` calls, so a per-frame sweep and an
    /// on-demand one can never disagree.
    pub fn process_shapecasts(&mut self) {
        let mut casts: Vec<NodeId> = Vec::new();
        for id in self.tree.all_nodes_in_tree_order() {
            let Some(node) = self.tree.get_node(id) else {
                continue;
            };
            if node.class_name() == "ShapeCast2D" {
                casts.push(id);
            }
        }
        for id in casts {
            crate::shapecast2d::force_shapecast_update(&mut self.tree, &self.physics_server, id);
        }
    }

    /// Sets the number of physics ticks per second (default 60).
    pub fn set_physics_ticks_per_second(&mut self, tps: u32) {
        assert!(tps > 0, "physics_ticks_per_second must be > 0");
        self.physics_ticks_per_second = tps;
        // Mirror to the Engine singleton so `Engine.physics_ticks_per_second`
        // answers the rate this loop actually runs at (pat-0q2kx.121).
        gdscript_interop::publish_engine_physics_ticks_per_second(tps as i64);
    }

    /// Returns the configured physics ticks per second.
    pub fn physics_ticks_per_second(&self) -> u32 {
        self.physics_ticks_per_second
    }

    /// Applies physics-related [`ProjectSettings`] to this main loop at boot,
    /// mirroring how Godot configures `Engine.physics_ticks_per_second` and the
    /// `PhysicsServer2D`/`PhysicsServer3D` space gravity from `project.godot`
    /// (pat-nzgdu.15).
    ///
    /// Honored keys (falling back to Godot defaults when absent):
    /// - `physics/common/physics_ticks_per_second`
    /// - `physics/2d/default_gravity` × `physics/2d/default_gravity_vector`
    ///   (default direction `(0, 1)`, i.e. downward in 2D screen space)
    /// - `physics/3d/default_gravity` × `physics/3d/default_gravity_vector`
    ///   (default direction `(0, -1, 0)`)
    ///
    /// This is the boot-path wiring the runner uses so a project that sets
    /// 30 ticks or 490 gravity actually simulates at those values — no manual
    /// `set_physics_ticks_per_second`/`set_gravity` call required.
    pub fn configure_from_project_settings(&mut self, settings: &gdcore::ProjectSettings) {
        use gdcore::{SettingsValue, Vector2, Vector3};

        // The root window's stretch (`display/window/stretch/*`). Idempotent,
        // so a host that already installed it before building its tree (as it
        // must, for `_ready`) loses nothing by this second call.
        crate::project_runtime::install_root_window(&mut self.tree, settings);

        // Audio: `audio/driver/mix_rate` and the two global panning strengths
        // (pat-mgixk.88). `AudioServer::from_project_settings` existed with NO
        // caller, so every project mixed at 44100 however it was configured —
        // audibly wrong pitch on a 48 kHz project, since the resample step is
        // `src_rate / output_rate`.
        //
        // ⚠️ Done through the bridge's own `configure_from_project_settings`,
        // which RETUNES the live server instead of replacing it. Replacing it
        // would discard a bus layout `bootstrap.rs` may already have loaded in
        // the same boot, which is why this does not have to be ordered against
        // that load.
        self.audio_bridge.configure_from_project_settings(settings);

        // Physics tick rate. Ignore non-positive values (Godot requires > 0).
        if let Some(tps) = settings
            .get("physics/common/physics_ticks_per_second")
            .and_then(|v| v.as_int())
        {
            if tps > 0 {
                self.physics_ticks_per_second = tps as u32;
            }
        }
        // `SceneTree.physics_interpolation_enabled` (pat-0q2kx.219). Godot
        // defaults this off, so a project that never opts in renders exactly as
        // before — this whole feature is inert until the setting flips.
        if let Some(on) = settings
            .get("physics/common/physics_interpolation")
            .and_then(|v| v.as_bool())
        {
            self.set_physics_interpolation_enabled(on);
        }

        // `Engine.get_physics_ticks_per_second()` must report the booted rate,
        // not the 60 default, once project.godot has been applied
        // (pat-0q2kx.121).
        gdscript_interop::publish_engine_physics_ticks_per_second(
            self.physics_ticks_per_second as i64,
        );

        // 2D gravity = scalar magnitude × direction vector.
        let g2 = settings
            .get("physics/2d/default_gravity")
            .and_then(|v| v.as_float())
            .unwrap_or(980.0) as f32;
        let dir2 = match settings.get("physics/2d/default_gravity_vector") {
            Some(SettingsValue::Vector2(x, y)) => Vector2::new(*x as f32, *y as f32),
            _ => Vector2::new(0.0, 1.0),
        };
        self.physics_server
            .set_gravity(Vector2::new(dir2.x * g2, dir2.y * g2));

        // 3D gravity = scalar magnitude × direction vector.
        let g3 = settings
            .get("physics/3d/default_gravity")
            .and_then(|v| v.as_float())
            .unwrap_or(9.8) as f32;
        let dir3 = match settings.get("physics/3d/default_gravity_vector") {
            Some(SettingsValue::Vector3(x, y, z)) => Vector3::new(*x as f32, *y as f32, *z as f32),
            _ => Vector3::new(0.0, -1.0, 0.0),
        };
        self.physics_server_3d
            .set_gravity(Vector3::new(dir3.x * g3, dir3.y * g3, dir3.z * g3));

        // 2D space + solver parameters (pat-0q2kx.116). These were stored on the
        // body and the world and read from nowhere — a project that authored
        // `default_linear_damp = 1.0` simulated as if it had not.
        //
        // ⚠️ Absent keys fall back to *Patina's* zero-effect defaults, not
        // Godot's (`default_linear_damp` 0.1, `default_angular_damp` 1.0).
        // Applying Godot's implicit damping to every project-booted 2D world is
        // a real parity gap, but it is also a behaviour change for every scene
        // that never asked for it, so it wants its own bead rather than riding
        // along here.
        let mut space = gdphysics2d::world::SpaceParams2D::default();
        if let Some(v) = float_setting(settings, "physics/2d/default_linear_damp") {
            space.default_linear_damp = v;
        }
        if let Some(v) = float_setting(settings, "physics/2d/default_angular_damp") {
            space.default_angular_damp = v;
        }
        if let Some(v) = float_setting(settings, "physics/2d/sleep_threshold_linear") {
            space.sleep_threshold_linear = v;
        }
        if let Some(v) = float_setting(settings, "physics/2d/sleep_threshold_angular") {
            space.sleep_threshold_angular = v;
        }
        if let Some(v) = float_setting(settings, "physics/2d/time_before_sleep") {
            space.time_before_sleep = v;
        }
        self.physics_server.set_space_params(space);

        let iterations = settings
            .get("physics/2d/solver/solver_iterations")
            .and_then(|v| v.as_int())
            .filter(|n| *n > 0)
            .map(|n| n as usize)
            .unwrap_or(gdphysics2d::solver::DEFAULT_SOLVER_ITERATIONS);
        let contact_bias = float_setting(settings, "physics/2d/solver/default_contact_bias")
            .unwrap_or(gdphysics2d::solver::DEFAULT_CONTACT_BIAS);
        self.physics_server
            .set_solver_params(iterations, contact_bias);
    }

    /// Sets the maximum number of physics steps per frame (default 8).
    pub fn set_max_physics_steps_per_frame(&mut self, max_steps: u32) {
        assert!(max_steps > 0, "max_physics_steps_per_frame must be > 0");
        self.max_physics_steps_per_frame = max_steps;
    }

    /// Returns the maximum physics steps per frame.
    pub fn max_physics_steps_per_frame(&self) -> u32 {
        self.max_physics_steps_per_frame
    }

    /// Returns whether the loop is currently paused.
    pub fn paused(&self) -> bool {
        self.paused
    }

    /// Sets the paused state and dispatches pause transition notifications.
    ///
    /// Matches Godot's behavior: when `SceneTree.paused` changes, all nodes
    /// in tree order receive `NOTIFICATION_PAUSED` or `NOTIFICATION_UNPAUSED`.
    pub fn set_paused(&mut self, paused: bool) {
        if self.paused == paused {
            return;
        }
        self.paused = paused;
        self.tree.set_paused(paused);
        let (notification, detail) = if paused {
            (NOTIFICATION_PAUSED, "PAUSED")
        } else {
            (NOTIFICATION_UNPAUSED, "UNPAUSED")
        };
        let ids = self.tree.all_nodes_in_tree_order();
        for id in ids {
            self.tree
                .trace_record(id, crate::trace::TraceEventType::Notification, detail);
            if let Some(node) = self.tree.get_node_mut(id) {
                node.receive_notification(notification);
            }
            // Deliver PAUSED/UNPAUSED to the script's `_notification` virtual
            // (pat-nzgdu.13).
            self.tree.notify_script(id, notification.code() as i64);
        }
    }

    /// Returns a reference to the owned [`SceneTree`].
    pub fn tree(&self) -> &SceneTree {
        &self.tree
    }

    /// Returns a mutable reference to the owned [`SceneTree`].
    pub fn tree_mut(&mut self) -> &mut SceneTree {
        &mut self.tree
    }

    /// Returns a reference to the physics server.
    pub fn physics_server(&self) -> &PhysicsServer {
        &self.physics_server
    }

    /// Returns a mutable reference to the physics server.
    pub fn physics_server_mut(&mut self) -> &mut PhysicsServer {
        &mut self.physics_server
    }

    /// Returns a reference to the 3D physics server.
    pub fn physics_server_3d(&self) -> &PhysicsServer3D {
        &self.physics_server_3d
    }

    /// Returns a mutable reference to the 3D physics server.
    pub fn physics_server_3d_mut(&mut self) -> &mut PhysicsServer3D {
        &mut self.physics_server_3d
    }

    /// Whether render-tick physics interpolation is on
    /// (`SceneTree.physics_interpolation_enabled`).
    pub fn physics_interpolation_enabled(&self) -> bool {
        self.physics_interpolation_enabled
    }

    /// Toggles render-tick physics interpolation.
    ///
    /// Turning it off clears the buffers: stale snapshots from an earlier
    /// enabled period would otherwise blend a node from wherever it used to be
    /// the moment it were re-enabled.
    pub fn set_physics_interpolation_enabled(&mut self, enabled: bool) {
        self.physics_interpolation_enabled = enabled;
        if !enabled {
            self.physics_interpolation.clear();
            self.physics_interpolation_3d.clear();
        }
    }

    /// Godot's `Node.reset_physics_interpolation()` — call after teleporting a
    /// body so the next frame renders it where it now is, instead of streaking
    /// it through the gap it never travelled.
    pub fn reset_physics_interpolation(&mut self, node_id: NodeId) {
        self.physics_interpolation.reset_node(node_id);
        // Godot's `reset_physics_interpolation()` is one call on `Node`, not one
        // per dimension, so both buffers are reset. Resetting only the matching
        // one would require the caller to know which server owns the node —
        // and would silently smear a 3D teleport (pat-0q2kx.250).
        self.physics_interpolation_3d.reset_node(node_id);
    }

    /// The interpolation buffers, for tests and for a render path that wants
    /// the blended transform.
    pub fn physics_interpolation(
        &self,
    ) -> &crate::physics_interpolation::PhysicsInterpolationState {
        &self.physics_interpolation
    }

    /// The 3D interpolation buffers (pat-0q2kx.250).
    pub fn physics_interpolation_3d(
        &self,
    ) -> &crate::physics_interpolation_3d::PhysicsInterpolation3DState {
        &self.physics_interpolation_3d
    }

    /// Godot's `PhysicalBoneSimulator3D.physical_bones_start_simulation()`
    /// (pat-0q2kx.203): hands the simulator's bones to the physics solver.
    ///
    /// Lives on `MainLoop` rather than being called as
    /// `physics_server_3d_mut().start_ragdoll_simulation(tree_mut(), ..)`
    /// because that borrows `self` mutably twice; here the server and the tree
    /// are disjoint fields.
    /// `only` names the bones to simulate; empty simulates all of them.
    pub fn physical_bones_start_simulation(&mut self, simulator: NodeId, only: &[String]) {
        self.physics_server_3d
            .start_ragdoll_simulation(&mut self.tree, simulator, only);
    }

    /// Godot's `physical_bones_stop_simulation()`: returns the bones to
    /// animation control.
    pub fn physical_bones_stop_simulation(&mut self, simulator: NodeId) {
        self.physics_server_3d
            .stop_ragdoll_simulation(&mut self.tree, simulator);
    }

    /// Returns a reference to the audio bridge.
    /// Reroutes every live positional voice standing inside an `Area3D` whose
    /// `audio_bus_override` is on to that area's `audio_bus_name`
    /// (pat-s3rrv.102).
    ///
    /// ⚠️ `AudioServer::redirect_stream_bus` shipped with the audio server,
    /// documented as being "for Godot's Area3D/Area2D audio-bus override", and
    /// had no caller in the entire workspace — the effect existed and was
    /// unreachable from a scene. This sweep is what connects the two.
    ///
    /// A player NOT inside an overriding area is left alone rather than being
    /// forced back to Master: its bus is whatever the node itself asked for,
    /// and stamping Master over that would break a player that legitimately
    /// selected its own bus.
    fn apply_area3d_audio_bus_overrides(&mut self) {
        // Nothing to do before any area is registered — the common case for a
        // scene with no Area3D at all, and it keeps the per-frame cost at one
        // emptiness check rather than a tree walk.
        if self.physics_server_3d.area_count() == 0 {
            return;
        }
        for node_id in self.audio_bridge.player_nodes() {
            let point = crate::node3d::get_global_transform(&self.tree, node_id).origin;
            if let Some(bus) = self.physics_server_3d.audio_bus_at(&self.tree, point) {
                self.audio_bridge.redirect_node_bus(node_id, &bus);
            }
        }
    }

    /// The 2D twin of [`Self::apply_area3d_audio_bus_overrides`]: reroutes every
    /// live `AudioStreamPlayer2D` standing inside an `Area2D` whose
    /// `audio_bus_override` is on to that area's `audio_bus_name`
    /// (pat-s3rrv.190).
    ///
    /// ⚠️ Filters on the node's class, which the 3D sweep does not do.
    /// `player_nodes()` returns EVERY tracked player — plain, 2D and 3D — and
    /// asking `node2d::get_global_position` for a 3D player yields a position
    /// anyway, so an unfiltered sweep would let a 2D area capture a 3D voice
    /// whose 2D position happens to land inside it.
    fn apply_area2d_audio_bus_overrides(&mut self) {
        // Nothing to do before any area is registered AND with nobody currently
        // overridden — the common case for a scene with no Area2D at all. The
        // second half matters: if the last area is removed while a player is
        // still overridden, that player must still be restored.
        if self.physics_server.area_count() == 0 && self.area2d_bus_overridden.is_empty() {
            return;
        }
        for node_id in self.audio_bridge.player_nodes() {
            let is_2d = self
                .tree
                .get_node(node_id)
                .map(|n| n.class_name() == "AudioStreamPlayer2D")
                .unwrap_or(false);
            if !is_2d {
                continue;
            }
            let point = crate::node2d::get_global_position(&self.tree, node_id);
            match self.physics_server.audio_bus_at(&self.tree, point) {
                Some(bus) => {
                    self.audio_bridge.redirect_node_bus(node_id, &bus);
                    self.area2d_bus_overridden.insert(node_id);
                }
                None => {
                    // ⚠️ Restore only players THIS sweep moved. Redirecting every
                    // player outside every area would stamp its own `bus` over a
                    // voice some other system legitimately routed elsewhere — and
                    // it is why the restore is gated on the remembered set rather
                    // than applied unconditionally.
                    if self.area2d_bus_overridden.remove(&node_id) {
                        let own = match self.tree.get_node(node_id).map(|n| n.get_property("bus")) {
                            Some(Variant::String(b)) if !b.is_empty() => b,
                            // Godot's default bus for a player that names none.
                            _ => "Master".to_string(),
                        };
                        self.audio_bridge.redirect_node_bus(node_id, &own);
                    }
                }
            }
        }
    }

    pub fn audio_bridge(&self) -> &AudioServerBridge {
        &self.audio_bridge
    }

    /// Returns a mutable reference to the audio bridge.
    pub fn audio_bridge_mut(&mut self) -> &mut AudioServerBridge {
        &mut self.audio_bridge
    }

    /// Runs `node`'s enter-tree/`_ready` lifecycle with the script-facing
    /// servers bound (pat-s3rrv.6).
    ///
    /// In Godot the servers are global singletons, so `_ready()` reaches them
    /// exactly like `_process()` does — `AudioServer.set_bus_volume_db(...)` in
    /// `_ready` is the standard options-menu idiom. Patina binds the servers by
    /// address for the duration of a script phase, and `_ready` runs through
    /// `LifecycleManager::enter_tree`, which is driven by the *host* rather
    /// than by [`Self::step`] — so a bare `enter_tree` leaves those singletons
    /// unbound and the call silently no-ops. Hosts that want Godot's behavior
    /// call this instead of `LifecycleManager::enter_tree` directly.
    ///
    /// The bracket is as tight as the per-phase ones: `&mut self` means the
    /// bridge cannot move while the address is installed.
    pub fn enter_tree_with_scripts(&mut self, node: NodeId) {
        self.bind_script_servers();
        crate::lifecycle::LifecycleManager::enter_tree(&mut self.tree, node);
        self.unbind_script_servers();
    }

    /// Binds the physics/audio servers (and physics delta) that script
    /// callbacks reach through singletons, for a lifecycle pass run outside
    /// the frame's own process phases. Pair with [`unbind_script_servers`].
    fn bind_script_servers(&mut self) {
        // The physics TICK, not a frame delta: Godot's `move_and_slide()` uses
        // `get_physics_process_delta_time()` whichever callback calls it
        // (pat-mgixk.37), and `_ready` has no delta of its own at all.
        let physics_delta = 1.0 / self.physics_ticks_per_second as f64;
        self.tree.set_script_physics_delta(physics_delta);
        self.tree
            .set_script_physics_server(&mut self.physics_server);
        self.tree
            .set_script_physics_server_3d(&mut self.physics_server_3d);
        self.tree
            .set_script_audio_server(self.audio_bridge.server_mut());
    }

    fn unbind_script_servers(&mut self) {
        self.tree.clear_script_audio_server();
        self.tree.clear_script_physics_server();
        self.tree.clear_script_physics_server_3d();
    }

    /// Registers physics bodies from the scene tree.
    ///
    /// Call this after loading a scene to populate the physics world.
    pub fn register_physics_bodies(&mut self) {
        self.physics_server.register_bodies(&self.tree);
    }

    /// Sets the input snapshot for the current frame.
    ///
    /// Scripts running during this frame's `_process` and `_physics_process`
    /// callbacks will see this input state when they query
    /// `Input.is_action_pressed()` etc.
    ///
    /// The snapshot is automatically cleared at the end of [`step`](Self::step),
    /// so callers must set it before each step.
    pub fn set_input(&mut self, snapshot: InputSnapshot) {
        self.tree.set_input_snapshot(snapshot);
    }

    /// Clears the current input snapshot.
    pub fn clear_input(&mut self) {
        self.tree.clear_input_snapshot();
    }

    /// Pushes a raw input event into the engine-owned [`InputState`].
    ///
    /// Events are accumulated between frames. When [`step`](Self::step) runs,
    /// the current `InputState` is converted into a script-facing
    /// [`InputSnapshot`] so scripts can query `Input.is_action_pressed()` etc.
    ///
    /// The event is also queued for dispatch to script input virtuals
    /// (`_input` / `_unhandled_input` / ...) at the next `step`.
    ///
    /// Per-frame state (just_pressed / just_released) is flushed automatically
    /// at the end of each step.
    pub fn push_event(&mut self, event: InputEvent) {
        // Window pixels -> canvas coordinates, FIRST, so everything downstream
        // (the GUI hit test, `_input`, `InputState`, the touch/mouse emulation)
        // sees the same space the renderer drew in (pat-mgixk.25).
        let event = self.map_event_to_canvas(event);
        // ⚠️ Decided BEFORE `process_event`, which mutates the very state the
        // decision reads: processing a touch release clears
        // `mouse_from_touch_index`, so asking afterwards can never tell that
        // this finger was the one driving the mouse.
        let emulated = self.emulated_mouse_event(&event);

        self.pending_input.push(event.clone());
        self.input_state.process_event(event);

        if let Some(synth) = emulated {
            // ⚠️ Queued for dispatch ONLY — deliberately not fed back through
            // `process_event`. `InputState::emulate_mouse_from_touch_event` has
            // already applied the state change, and re-processing would both
            // double-count the press and re-enter `emulate_touch_from_mouse`,
            // which is how a touch/mouse emulation pair loops.
            self.pending_input.push(synth);
        }
    }

    /// Maps a pointer event from window pixels into canvas coordinates.
    ///
    /// Godot transforms every pointer event by the inverse of the window's
    /// content-scale transform before `_input` or the GUI sees it
    /// (`scene/main/viewport.cpp` `Viewport::push_input`), which is what makes a
    /// click land on what was drawn. Patina DREW through
    /// `RootWindow::stretch_transform` but never mapped input back:
    /// `window_to_canvas` carried the comment "Input MUST go through this" and
    /// had no caller outside its own tests, so the moment a project enabled
    /// stretching every click missed by the stretch factor (pat-mgixk.25).
    ///
    /// Positions go through the full affine inverse; `relative`, `velocity` and
    /// `delta` are DELTAS and take the basis only, because adding the
    /// transform's translation to a difference of two points would offset it by
    /// the letterbox margin twice over.
    ///
    /// A project with stretching off has an identity `window_transform`, so this
    /// is a no-op there — but it early-outs anyway rather than running four
    /// matrix ops per event on the common path.
    fn map_event_to_canvas(&self, event: InputEvent) -> InputEvent {
        let window = self.tree.root_window();
        if !window.is_stretching() {
            return event;
        }
        let inverse = window.content_scale().window_transform.affine_inverse();
        let point = |p: gdcore::math::Vector2| inverse.xform(p);
        let delta = |v: gdcore::math::Vector2| inverse.basis_xform(v);
        match event {
            InputEvent::MouseButton {
                button,
                pressed,
                position,
            } => InputEvent::MouseButton {
                button,
                pressed,
                position: point(position),
            },
            InputEvent::MouseMotion { position, relative } => InputEvent::MouseMotion {
                position: point(position),
                relative: delta(relative),
            },
            InputEvent::ScreenTouch {
                index,
                position,
                pressed,
                double_tap,
                canceled,
            } => InputEvent::ScreenTouch {
                index,
                position: point(position),
                pressed,
                double_tap,
                canceled,
            },
            InputEvent::ScreenDrag {
                index,
                position,
                relative,
                velocity,
                pressure,
                tilt,
            } => InputEvent::ScreenDrag {
                index,
                position: point(position),
                relative: delta(relative),
                velocity: delta(velocity),
                pressure,
                // Tilt is a direction in -1..=1 per axis, not a screen
                // measurement, so a stretch must not touch it.
                tilt,
            },
            InputEvent::MagnifyGesture { position, factor } => InputEvent::MagnifyGesture {
                position: point(position),
                // `factor` is a ratio; scaling it would compound the stretch.
                factor,
            },
            InputEvent::PanGesture { position, delta: d } => InputEvent::PanGesture {
                position: point(position),
                delta: delta(d),
            },
            // Keys, gamepads and MIDI carry no screen coordinate.
            other => other,
        }
    }

    /// The mouse event Godot's `emulate_mouse_from_touch` would produce for
    /// `event`, or `None` (pat-s3rrv.77).
    ///
    /// # Why this exists at all
    ///
    /// `InputState` already emulates — but only into ITSELF. The GUI stage
    /// consumes events, so without a synthesized event a touch could set
    /// `Input.is_mouse_button_pressed()` while every Button in the scene stayed
    /// inert. Godot emits a real `InputEventMouseButton` here, and scripts'
    /// `_gui_input` receive exactly that.
    ///
    /// ⚠️ Only the FIRST finger down translates, and only that same finger
    /// releases the emulated button — Godot's `mouse_from_touch_index` rule. A
    /// second finger must not move the cursor or the pointer jumps between
    /// fingers mid-gesture.
    fn emulated_mouse_event(&self, event: &InputEvent) -> Option<InputEvent> {
        if !self.input_state.is_emulating_mouse_from_touch() {
            return None;
        }
        let driving = self.input_state.mouse_from_touch_index();
        match event {
            InputEvent::ScreenTouch {
                index,
                position,
                pressed,
                ..
            } => {
                let translates = if *pressed {
                    // The first finger down claims the mouse; later ones do not.
                    driving.is_none()
                } else {
                    driving == Some(*index)
                };
                translates.then(|| InputEvent::MouseButton {
                    button: gdplatform::input::MouseButton::Left,
                    pressed: *pressed,
                    position: *position,
                })
            }
            InputEvent::ScreenDrag {
                index, position, ..
            } => (driving == Some(*index)).then(|| InputEvent::MouseMotion {
                position: *position,
                // ⚠️ `relative` is the delta since the finger's last known
                // position, not since the last mouse move: the emulated cursor
                // teleports to wherever the finger went down, and reporting that
                // jump as motion would fling anything reading `relative`.
                relative: *position - self.input_state.touch_position(*index).unwrap_or(*position),
            }),
            _ => None,
        }
    }

    /// Godot's physics object picking — the final input stage, after
    /// `_unhandled_input` (pat-0q2kx.138).
    ///
    /// `Viewport::_process_picking` queries the 2D physics space at the cursor
    /// and calls `_input_event(viewport, event, shape_idx)` on every
    /// `CollisionObject2D` under it whose `input_pickable` is set
    /// (upstream/godot/scene/main/viewport.cpp). Patina had the point query and
    /// the `input_pickable` property but nothing joined them, so a click never
    /// reached an Area2D.
    ///
    /// Skipped entirely once the event is handled: an earlier stage — most
    /// often the GUI stage consuming a click on a Control — takes the event out
    /// of circulation, which is exactly how a button stops a click from also
    /// hitting the world behind it.
    fn physics_pick_input(&mut self, event: &InputEvent) {
        let position = match event {
            InputEvent::MouseButton { position, .. } => *position,
            InputEvent::MouseMotion { position, .. } => *position,
            // Picking is a pointer-space query; keys have no cursor position.
            _ => return,
        };
        if self.tree.is_input_handled() {
            return;
        }

        // Resolve the hit bodies to their scene nodes first, so the physics
        // borrow is released before any script runs (a script reached through
        // `_input_event` can mutate the tree).
        // Paired with the index of the collider actually struck, which is
        // Godot's `shape_idx` (pat-s3rrv.25).
        let hits_with_shape: Vec<(NodeId, usize)> = self
            .physics_server
            .colliders_at_point_with_shape(position)
            .into_iter()
            .filter(|(nid, _)| {
                // Godot's default is `input_pickable = true`, so a collider that
                // never mentions the property still picks.
                self.tree.get_node(*nid).is_some_and(|n| {
                    !matches!(n.get_property("input_pickable"), Variant::Bool(false))
                })
            })
            .collect();
        let hits: Vec<NodeId> = hits_with_shape.iter().map(|(nid, _)| *nid).collect();

        // -- hover transitions (pat-0q2kx.199) --
        // Resolved before dispatch on purpose: enter/exit are notifications, not
        // consumable input. Were they emitted inside the loop below, a script
        // that handles the click would trip the `is_input_handled` return and
        // strand the pointer as permanently "inside" a collider it has left.
        let current: std::collections::HashSet<NodeId> = hits.iter().copied().collect();
        let exited: Vec<NodeId> = self
            .hovered_pickables
            .difference(&current)
            .copied()
            .collect();
        let entered: Vec<NodeId> = current
            .difference(&self.hovered_pickables)
            .copied()
            .collect();
        self.hovered_pickables = current;

        // Exits precede entries, matching Godot: a pointer sliding from one
        // collider onto an overlapping one releases the old before taking the
        // new, so a script tracking "what am I over" never sees two at once.
        //
        // Both halves of Godot's contract fire here (pat-s3rrv.25). Patina used
        // to emit only the SIGNALS, so `_mouse_enter`/`_mouse_exit` — the
        // virtuals `CollisionObject2D.xml` documents, and the names an author
        // reading those docs writes — did nothing at all. The near-miss made it
        // hard to see: gdscene does dispatch `_mouse_entered`/`_mouse_exited`,
        // but only on Controls (`scene_tree.rs` GUI stage) and those are signal
        // names with an underscore glued on, not these virtuals. So the exact
        // names the documentation gives were the ones that were dead.
        //
        // `shape_idx` is 0 for the same reason it is 0 in the picking dispatch
        // below: a Patina body has exactly one shape.
        // The shape/object pair nests the way Godot nests it: leaving reports the
        // shape before the object, entering reports the object before the shape.
        // The signal emission keeps its existing position so nothing that
        // already depends on `mouse_entered`/`mouse_exited` ordering shifts.
        // `shape_idx` for the collider actually under the pointer
        // (pat-s3rrv.25). On EXIT the node is no longer in `hits_with_shape` —
        // it has just left — so 0 is the honest answer there rather than a
        // stale index from wherever the pointer used to be.
        let shape_at = |nid: NodeId| -> i64 {
            hits_with_shape
                .iter()
                .find(|(hit, _)| *hit == nid)
                .map(|(_, idx)| *idx as i64)
                .unwrap_or(0)
        };
        for nid in exited {
            self.tree.emit_signal(nid, "mouse_exited", &[]);
            let _ = self.tree.call_script_method(
                nid,
                "_mouse_shape_exit",
                &[Variant::Int(shape_at(nid))],
            );
            let _ = self.tree.call_script_method(nid, "_mouse_exit", &[]);
        }
        for nid in entered {
            self.tree.emit_signal(nid, "mouse_entered", &[]);
            let _ = self.tree.call_script_method(nid, "_mouse_enter", &[]);
            let _ = self.tree.call_script_method(
                nid,
                "_mouse_shape_enter",
                &[Variant::Int(shape_at(nid))],
            );
        }

        // The REAL flag, not `false` (pat-s3rrv.76). Godot delivers a double
        // click to a collider's `_input_event` exactly as it does to `_input`;
        // hardcoding `false` meant a collider could never see one.
        //
        // Safe to read here, and the ordering is the whole argument: `step`
        // dispatches `push_input(event)` and then `physics_pick_input(event)`
        // for the SAME event inside one loop iteration, so the flag was computed
        // from this event microseconds earlier and cannot be a stale read from
        // the previous press. Picking running last is Godot's order too
        // (`viewport.cpp:3464-3580`). If picking is ever hoisted above
        // `push_input`, this silently reports the PREVIOUS event's flag — move
        // the mirror, do not reorder the calls.
        let payload = crate::scene_tree::input_event_to_variant(
            event,
            self.tree.last_double_click(),
            self.tree.pen_state(),
            self.tree.last_modifiers(),
        );
        for nid in hits {
            if self.tree.is_input_handled() {
                return;
            }
            // Godot delivers picking to the `input_event` signal as well as the
            // virtual method. Only the method was wired, so the ordinary case —
            // a scene connecting `input_event` in the editor, with no script on
            // the collider at all — received nothing.
            //
            // Godot's signature is `(viewport, event, shape_idx)` and Patina used
            // to send the event alone (pat-s3rrv.25). That is not a missing
            // extra: the interpreter REJECTS an arity mismatch outright
            // (`interpreter.rs`: `args.len() > func.params.len()` is a
            // `TypeError`) and `call_script_method` swallows the raise, so a
            // handler written to Godot's documented signature — the one every
            // doc and tutorial shows — silently never ran at all.
            //
            // Both arguments the old comment here called unreproducible are in
            // fact available:
            //   * `viewport` is `Node.get_viewport()`, already implemented in
            //     `crate::viewport::get_viewport`, which walks to the nearest
            //     enclosing `Viewport` and falls back to the root `Window`. A
            //     collider inside a `SubViewport` correctly gets that.
            //   * `shape_idx` is `0`, and that is a *value*, not a placeholder:
            //     `PhysicsBody2D` holds exactly one shape (`body.rs`: `pub shape:
            //     Shape2D`) and `find_collision_shape` keeps only the first
            //     collider child, so index 0 is the only shape any Patina body
            //     has. When multi-shape bodies land, `colliders_at_point` has to
            //     start returning the struck index and this becomes a real
            //     lookup — but sending nothing today does not make that future
            //     change any easier, it just breaks every handler in the present.
            let viewport =
                Variant::ObjectId(crate::viewport::get_viewport(&self.tree, nid).object_id());
            let shape_idx = hits_with_shape
                .iter()
                .find(|(hit, _)| *hit == nid)
                .map(|(_, idx)| *idx as i64)
                .unwrap_or(0);
            let args = [viewport, payload.clone(), Variant::Int(shape_idx)];
            self.tree.emit_signal(nid, "input_event", &args);
            let _ = self.tree.call_script_method(nid, "_input_event", &args);
        }
    }

    /// Sets the surface size mouse positions are measured in, so 3D picking can
    /// unproject a screen point through the current `Camera3D` (pat-0q2kx.202).
    /// Sets the surface size, and resizes the root window with it
    /// (pat-s3rrv.120).
    ///
    /// 🔑 **One entry point for both.** This used to store only the pick-ray
    /// tuple, while `SceneTree::resize_root_window` — which recomputes the
    /// content scale, republishes the mirrored properties and emits
    /// `size_changed` — had no caller outside tests. So after a resize the
    /// stretch transform, `get_viewport().get_visible_rect()` and Control
    /// anchoring all stayed at the boot size, and the two sizes could disagree
    /// with each other.
    ///
    /// ⚠️ Safe to call every frame: `resize_root_window` returns early when the
    /// size is unchanged, so `size_changed` fires on a real resize and not once
    /// per frame. That early-out is what makes the frame loop a legitimate
    /// caller — Godot delivers this from `NOTIFICATION_WM_SIZE_CHANGED`, an
    /// event, so a per-frame re-emit would be a parity bug, not just noise.
    pub fn set_viewport_size(&mut self, width: u32, height: u32) {
        let (w, h) = (width.max(1), height.max(1));
        self.viewport_size = (w, h);
        self.tree
            .resize_root_window(gdcore::math::Vector2::new(w as f32, h as f32));
    }

    /// The surface size used for 3D pick rays.
    pub fn viewport_size(&self) -> (u32, u32) {
        self.viewport_size
    }

    /// Casts the pointer through the current `Camera3D` into the 3D physics
    /// world and dispatches `input_event` / `mouse_entered` / `mouse_exited` to
    /// the `CollisionObject3D` it hits (pat-0q2kx.202).
    ///
    /// `Viewport::_process_picking` (upstream/godot/scene/main/viewport.cpp:750)
    /// unprojects each unhandled mouse event through the current camera, raycasts
    /// the 3D space, and dispatches to the hit object when its
    /// `input_ray_pickable` is set.
    ///
    /// Every piece existed and none of them were joined: `project_ray_origin` /
    /// `project_ray_normal` (`camera3d.rs:854`/`:885`) had **no callers at all**,
    /// `PhysicsRayQuery3D` was only reached from `RayCast3D` nodes, and
    /// `input_ray_pickable` appeared solely as an inspector schema string. So a
    /// click never reached a 3D body no matter how the scene was authored.
    ///
    /// This is deliberately the same shape as [`Self::physics_pick_input`] — the
    /// 2D twin two functions up — including the ordering rules, which are load
    /// bearing and were worked out there.
    fn physics_pick_input_3d(&mut self, event: &InputEvent) {
        let position = match event {
            InputEvent::MouseButton { position, .. } => *position,
            InputEvent::MouseMotion { position, .. } => *position,
            // Picking is a pointer-space query; keys have no cursor position.
            _ => return,
        };
        if self.tree.is_input_handled() {
            return;
        }
        // No current camera means no frustum to unproject through. Godot skips
        // 3D picking entirely in that case rather than guessing a view.
        let Some(camera) = crate::camera3d::current_camera3d(&self.tree) else {
            return;
        };
        // ⚠️ This stage runs BEFORE the physics phase, so the 3D world reflects
        // the *previous* frame. On the very first `step` of a fresh scene it is
        // still empty — the 3D server registers lazily inside `sync_to_physics`
        // — so a click delivered on frame 1 hits nothing. Godot picks inside the
        // physics frame instead, against an already-updated space. Matching the
        // 2D twin's placement was chosen over matching Godot's, because the
        // handled-state this stage reads is what lets a Control swallow a click
        // before the world sees it.

        let (vw, vh) = self.viewport_size;
        let from = crate::camera3d::project_ray_origin(&self.tree, camera, position, vw, vh);
        let dir = crate::camera3d::project_ray_normal(&self.tree, camera, position, vw, vh);
        // Godot casts to the camera's far plane, so a body beyond the view
        // distance is not pickable even though it is in front of the camera.
        let far = crate::node3d::get_far(&self.tree, camera) as f32;
        let to = from + dir * far;

        let mut query = gdphysics3d::PhysicsRayQuery3D::new(from, to);
        // Picking ignores collision layers — Godot gates it on
        // `input_ray_pickable`, not on the mask — so every layer stays in.
        query.collide_with_bodies = true;
        query.collide_with_areas = true;

        // Resolve to a scene node and release the physics borrow before any
        // script runs: a script reached through `input_event` can mutate the
        // tree.
        let hit = query.intersect(self.physics_server_3d.world().bodies());
        let hit = hit.and_then(|h| {
            let nid = self.physics_server_3d.node_for_body(h.body_id)?;
            // ⚠️ Godot's default is `input_ray_pickable = true`, so a body that
            // never mentions the property still picks. Only an explicit `false`
            // opts out.
            let pickable = self.tree.get_node(nid).is_some_and(|n| {
                !matches!(n.get_property("input_ray_pickable"), Variant::Bool(false))
            });
            pickable.then_some((nid, h.point, h.normal))
        });

        // -- hover transitions --
        // Resolved before dispatch, for the reason the 2D twin documents:
        // enter/exit are notifications, not consumable input. Emitting them
        // inside the dispatch below would let a script that handles the click
        // trip the `is_input_handled` return and strand the pointer as
        // permanently "over" a body it has left.
        let current: std::collections::HashSet<NodeId> =
            hit.iter().map(|(nid, _, _)| *nid).collect();
        let exited: Vec<NodeId> = self
            .hovered_pickables_3d
            .difference(&current)
            .copied()
            .collect();
        let entered: Vec<NodeId> = current
            .difference(&self.hovered_pickables_3d)
            .copied()
            .collect();
        self.hovered_pickables_3d = current;

        // Exits precede entries, matching Godot and the 2D twin.
        //
        // ⚠️ `CollisionObject3D` has only `_mouse_enter`/`_mouse_exit` — no
        // `_mouse_shape_enter`/`_mouse_shape_exit`. That asymmetry with the 2D
        // twin above is Godot's, not an omission here: `CollisionObject3D.xml`
        // documents two mouse virtuals where `CollisionObject2D.xml` documents
        // four. Adding the shape pair in 3D would invent API (pat-s3rrv.25).
        for nid in exited {
            self.tree.emit_signal(nid, "mouse_exited", &[]);
            let _ = self.tree.call_script_method(nid, "_mouse_exit", &[]);
        }
        for nid in entered {
            self.tree.emit_signal(nid, "mouse_entered", &[]);
            let _ = self.tree.call_script_method(nid, "_mouse_enter", &[]);
        }

        let Some((nid, point, normal)) = hit else {
            return;
        };
        if self.tree.is_input_handled() {
            return;
        }

        // Godot's signature is
        // `input_event(camera, event, event_position, normal, shape_idx)` — all
        // five, as of pat-s3rrv.25.
        //
        // `shape_idx` used to be omitted here because the ray result carries a
        // `BodyId3D` rather than which of that body's shapes was struck.
        // `PhysicsBody3D` holds exactly one shape (`body.rs`: `pub shape:
        // Shape3D`), so `0` is the only index any 3D body has — a real value,
        // not a placeholder.
        //
        // ⚠️ The 2D twin no longer sends a constant: 2D bodies and areas gained
        // a collider list (pat-s3rrv.25) and picking reports the struck index.
        // 3D stays `0` because `PhysicsBody3D` has no such list *yet* — when it
        // gains one, this line becomes wrong in exactly the way the 2D one was. Sending four
        // arguments to a five-parameter handler is not a partial delivery: the
        // interpreter rejects the arity outright and the raise is swallowed, so
        // a handler with Godot's documented signature never ran.
        // See the 2D picking site above for why reading the mirrored flag here
        // is ordering-safe; the 3D pass runs later still in the same iteration.
        let payload = crate::scene_tree::input_event_to_variant(
            event,
            self.tree.last_double_click(),
            self.tree.pen_state(),
            self.tree.last_modifiers(),
        );
        let args = [
            Variant::ObjectId(camera.object_id()),
            payload.clone(),
            Variant::Vector3(point),
            Variant::Vector3(normal),
            Variant::Int(0),
        ];
        self.tree.emit_signal(nid, "input_event", &args);
        let _ = self.tree.call_script_method(nid, "_input_event", &args);
    }

    /// Sets the [`InputMap`] that maps physical events to named actions.
    ///
    /// Must be called before pushing events if scripts rely on action names
    /// (e.g. `Input.is_action_pressed("ui_left")`).
    pub fn set_input_map(&mut self, map: InputMap) {
        // Snapshot what the host installed. `load_from_project_settings()` is a
        // reset, so it needs the pre-runtime baseline to return to; taking it
        // here means any host that installs a map gets the behaviour, without a
        // second "and also register the defaults" call to forget.
        self.project_input_map = Some(map.clone());
        self.input_state.set_input_map(map);
    }

    /// Applies the `Input` mutations scripts queued this frame to the engine
    /// [`InputState`], then refreshes the tree's read mirrors (pat-0q2kx.24).
    ///
    /// Godot's `Input` mutators act on live engine state; here the script phase
    /// records intent on the tree and this drains it, so `action_press` is real
    /// engine state from the next frame on — while remaining visible to later
    /// scripts *this* frame through the injected snapshot.
    fn apply_script_input_commands(&mut self) {
        use crate::scripting::InputCommand;
        use gdplatform::input::MouseMode;

        for command in self.tree.drain_input_commands() {
            match command {
                InputCommand::SetMouseMode(mode) => {
                    let mode = match mode {
                        1 => MouseMode::Hidden,
                        2 => MouseMode::Captured,
                        3 => MouseMode::Confined,
                        4 => MouseMode::ConfinedHidden,
                        // Godot treats any other value as VISIBLE.
                        _ => MouseMode::Visible,
                    };
                    self.input_state.set_mouse_mode(mode);
                }
                InputCommand::WarpMouse { x, y } => {
                    self.input_state
                        .warp_mouse(gdcore::math::Vector2::new(x, y));
                }
                InputCommand::ActionPress { action, strength } => {
                    self.input_state.action_press(&action, strength);
                }
                InputCommand::ActionRelease { action } => {
                    self.input_state.action_release(&action);
                }
                InputCommand::StartJoyVibration {
                    device,
                    weak,
                    strong,
                    duration,
                } => {
                    if device >= 0 {
                        self.input_state
                            .start_joy_vibration(device as u32, weak, strong, duration);
                    }
                }
                InputCommand::StopJoyVibration { device } => {
                    if device >= 0 {
                        self.input_state.stop_joy_vibration(device as u32);
                    }
                }
                // Cursor shape (pat-0q2kx.90). An out-of-range shape is ignored
                // rather than clamped: Godot's `set_default_cursor_shape` is
                // `ERR_FAIL_INDEX`-guarded (display_server_web.cpp:500), so a
                // bad value leaves the cursor alone instead of snapping it to
                // ARROW.
                InputCommand::SetDefaultCursorShape(shape) => {
                    if let Some(shape) = u8::try_from(shape)
                        .ok()
                        .and_then(gdplatform::cursor::CursorShape::from_u8)
                    {
                        self.cursor.set_cursor_shape(shape);
                        self.tree.set_cursor_shape_mirror(shape as i64);
                    }
                }
                InputCommand::SetCustomMouseCursor {
                    image_path,
                    shape,
                    hotspot_x,
                    hotspot_y,
                } => {
                    if let Some(shape) = u8::try_from(shape)
                        .ok()
                        .and_then(gdplatform::cursor::CursorShape::from_u8)
                    {
                        if image_path.is_empty() {
                            // Godot clears the override when passed a null image.
                            self.cursor.clear_custom_cursor(shape);
                        } else {
                            self.cursor.set_custom_cursor_path(
                                shape,
                                image_path,
                                hotspot_x.max(0.0) as u32,
                                hotspot_y.max(0.0) as u32,
                            );
                        }
                    }
                }
                // InputMap rebinding (pat-0q2kx.89). These land on the engine
                // map before the next event is pushed, so an action a script
                // created this frame resolves from the very next input.
                InputCommand::InputMapAddAction { action, deadzone } => {
                    self.input_state
                        .input_map_mut()
                        .add_action(action, deadzone);
                }
                InputCommand::InputMapEraseAction { action } => {
                    self.input_state.input_map_mut().erase_action(&action);
                }
                InputCommand::InputMapSetDeadzone { action, deadzone } => {
                    self.input_state
                        .input_map_mut()
                        .action_set_deadzone(&action, deadzone);
                }
                InputCommand::InputMapAddEvent { action, binding } => {
                    self.input_state
                        .input_map_mut()
                        .action_add_event(action, binding);
                }
                InputCommand::InputMapEraseEvent { action, binding } => {
                    self.input_state
                        .input_map_mut()
                        .action_erase_event(&action, &binding);
                }
                InputCommand::InputMapEraseEvents { action } => {
                    self.input_state
                        .input_map_mut()
                        .action_erase_events(&action);
                }
                InputCommand::InputMapLoadFromProjectSettings => {
                    // Replace, do not merge: the whole point of the call is to
                    // discard runtime rebinds. Cloned because the baseline has
                    // to survive a second reload.
                    if let Some(baseline) = self.project_input_map.clone() {
                        self.input_state.set_input_map(baseline);
                    }
                }
            }
        }

        // Mirror engine truth back for the next frame's reads.
        let mode = match self.input_state.mouse_mode() {
            MouseMode::Visible => 0,
            MouseMode::Hidden => 1,
            MouseMode::Captured => 2,
            MouseMode::Confined => 3,
            MouseMode::ConfinedHidden => 4,
        };
        self.tree.set_input_mouse_mode_mirror(mode);
        let joypads: Vec<i64> = self
            .input_state
            .connected_joypads()
            .into_iter()
            .map(|d| d as i64)
            .collect();
        self.tree.set_input_connected_joypads_mirror(joypads);
        self.tree
            .set_input_map_mirror(self.input_state.input_map_action_event_counts());
        // pat-s3rrv.14: the bindings themselves, not just their count — an
        // event cannot answer `is_action_pressed("ui_accept")` without knowing
        // what "ui_accept" is bound to.
        self.tree
            .set_input_map_binding_mirror(self.input_state.input_map_action_bindings());
    }

    /// Returns a reference to the engine-owned [`InputState`].
    pub fn input_state(&self) -> &InputState {
        &self.input_state
    }

    /// Returns a mutable reference to the engine-owned [`InputState`].
    pub fn input_state_mut(&mut self) -> &mut InputState {
        &mut self.input_state
    }

    /// The engine-owned [`CursorManager`](gdplatform::cursor::CursorManager)
    /// (pat-0q2kx.90). The editor server reads the active shape out of this to
    /// put it in the input-state payload the browser game view polls.
    pub fn cursor(&self) -> &gdplatform::cursor::CursorManager {
        &self.cursor
    }

    /// Mutable access to the cursor state, for a host applying a change that
    /// did not come from a script.
    pub fn cursor_mut(&mut self) -> &mut gdplatform::cursor::CursorManager {
        &mut self.cursor
    }

    /// Advances one frame by `delta_secs` seconds.
    ///
    /// 1. Accumulates physics time and runs fixed-timestep physics ticks
    ///    (dispatching `NOTIFICATION_PHYSICS_PROCESS`), up to
    ///    `max_physics_steps_per_frame`.
    /// 2. Dispatches `NOTIFICATION_PROCESS` once (variable timestep).
    /// 3. Increments the frame counter and elapsed time accumulators.
    ///
    /// Returns a [`FrameOutput`] describing what happened during this frame.
    pub fn step(&mut self, delta_secs: f64) -> FrameOutput {
        // -- JavaScriptBridge callbacks (pat-mgixk.41) --
        // Runs the callbacks the browser invoked since the last frame, on the
        // main thread at a frame boundary. A `create_callback` closure cannot
        // call straight back into the interpreter — it fires while that
        // interpreter is already borrowed further up the stack — so the wasm
        // side queues and this drains.
        //
        // FIRST in the frame, so a callback that moves a node or sets a flag is
        // visible to BOTH the physics and the process phases of the same frame
        // rather than landing a frame late. The loop itself is in `scripting`:
        // it needs nothing from `MainLoop` but the tree.
        crate::scripting::pump_javascript_callbacks(&mut self.tree);

        // -- Engine singleton sync (pat-0q2kx.121) --
        // `Engine.physics_ticks_per_second = N` from a script has to land on the
        // loop that owns the fixed step. The generation counter distinguishes a
        // script write from the value this loop publishes itself, so two loops
        // sharing the process statics never fight over the tick rate.
        let script_tps_gen = gdscript_interop::engine_physics_ticks_per_second_generation();
        if script_tps_gen != self.engine_physics_tps_gen {
            self.engine_physics_tps_gen = script_tps_gen;
            let tps = gdscript_interop::engine_physics_ticks_per_second();
            if tps > 0 {
                self.physics_ticks_per_second = tps as u32;
            }
        } else {
            gdscript_interop::publish_engine_physics_ticks_per_second(
                self.physics_ticks_per_second as i64,
            );
        }
        gdscript_interop::set_engine_max_physics_steps_per_frame(
            self.max_physics_steps_per_frame as i64,
        );

        // The real (unscaled) delta the driver reported. `Engine.time_scale`
        // scales the simulation, not the wall clock, so the measured FPS and
        // pixel-per-second mouse velocity are computed from this one.
        let real_delta_secs = delta_secs;
        if real_delta_secs > 0.0 {
            gdscript_interop::set_engine_frames_per_second(1.0 / real_delta_secs);
        }
        // Godot's `Main::iteration` multiplies the frame delta by `time_scale`
        // before both the process and physics phases, which is what makes
        // slow-motion slow physics down too.
        let delta_secs = delta_secs * gdscript_interop::engine_time_scale();

        // Bridge engine-owned InputState → script-facing InputSnapshot.
        // Only apply if no manual snapshot was set via set_input().
        if !self.tree.has_input_snapshot() {
            let platform_snap = self.input_state.snapshot();
            // Convert gdplatform::InputSnapshot → gdscene::scripting::InputSnapshot.
            // Convert mouse buttons to Godot button index strings
            // (1=Left, 2=Right, 3=Middle, 8=XBUTTON1, 9=XBUTTON2).
            //
            // ⚠️ The thumb buttons were MISSING here (pat-s3rrv.76), and that
            // made two script-facing APIs silently wrong rather than absent:
            // `Input.is_mouse_button_pressed(MOUSE_BUTTON_XBUTTON1)` answered
            // false while the button was held, and `Input.get_mouse_button_mask`
            // — which derives from that same query — could never set
            // MOUSE_BUTTON_MASK_MB_XBUTTON1/2 (128/256). Widening the mask loop
            // WITHOUT this would have changed nothing: the bits it ORs in are
            // read from this set.
            //
            // Wheel buttons (4..=7) are deliberately absent. Godot's
            // `MouseButtonMask` (core/input/input_enums.h:142-149) names only
            // LEFT/RIGHT/MIDDLE/MB_XBUTTON1/MB_XBUTTON2, and a wheel notch is
            // pressed and released within one batch, so it is never held at the
            // moment this per-frame snapshot is taken.
            let mut mouse_buttons_pressed = std::collections::HashSet::new();
            use gdplatform::input::MouseButton;
            for (btn, idx) in [
                (MouseButton::Left, "1"),
                (MouseButton::Right, "2"),
                (MouseButton::Middle, "3"),
                (MouseButton::XButton1, "8"),
                (MouseButton::XButton2, "9"),
            ] {
                if platform_snap.is_mouse_button_pressed(btn) {
                    mouse_buttons_pressed.insert(idx.to_string());
                }
            }
            let mouse_pos = platform_snap.get_mouse_position();

            // Gamepad state, keyed by Godot's JoyButton/JoyAxis numbering so a
            // script's `JOY_BUTTON_*` / `JOY_AXIS_*` constant matches
            // (pat-0q2kx.88). Buttons Godot models as axes (the analog
            // triggers) have no JoyButton and are dropped here.
            let mut gamepad_buttons_pressed = std::collections::HashSet::new();
            for (device, button) in platform_snap.pressed_gamepad_buttons() {
                if let Some(index) = button.to_godot_index() {
                    gamepad_buttons_pressed.insert((device as i64, index));
                }
            }
            let mut gamepad_axis_values = std::collections::HashMap::new();
            for ((device, axis), value) in platform_snap.gamepad_axis_values() {
                gamepad_axis_values.insert((device as i64, axis.to_godot_index()), value);
            }

            let script_snap = InputSnapshot {
                pressed_keys: platform_snap.pressed_key_names().into_iter().collect(),
                just_pressed_keys: platform_snap.just_pressed_key_names().into_iter().collect(),
                just_released_keys: platform_snap
                    .just_released_key_names()
                    .into_iter()
                    .collect(),
                input_map: platform_snap.action_pressed_key_map(),
                mouse_position: mouse_pos,
                // Godot reports mouse velocity in pixels/second, and the delta
                // is only known here — `InputState` just accumulates the motion
                // (pat-0q2kx.92).
                mouse_velocity: if real_delta_secs > 0.0 {
                    let motion = self.input_state.mouse_motion_this_frame();
                    gdcore::math::Vector2::new(
                        motion.x / real_delta_secs as f32,
                        motion.y / real_delta_secs as f32,
                    )
                } else {
                    gdcore::math::Vector2::ZERO
                },
                mouse_buttons_pressed,
                // The engine's own action state, so an action bound to a
                // gamepad button reaches scripts — the key-name `input_map`
                // above can only ever answer for keyboard bindings.
                actions_pressed: platform_snap.pressed_actions(),
                actions_just_pressed: platform_snap.just_pressed_actions(),
                actions_just_released: platform_snap.just_released_actions(),
                gamepad_buttons_pressed,
                gamepad_axis_values,
                // Carry the analog pair across the bridge (pat-s3rrv.14).
                // `InputState` computes both correctly and had ZERO readers:
                // the script-facing snapshot never carried them, so the
                // `SceneAccess` digital default answered every query.
                //
                // Only held actions are stored. Both gdplatform accessors gate
                // on `actions_pressed` before consulting their maps, so an
                // absent entry and an unpressed action mean the same thing, and
                // the override's digital fallback reproduces it exactly.
                action_strengths: platform_snap
                    .pressed_actions()
                    .into_iter()
                    .map(|a| {
                        let pair = (
                            platform_snap.get_action_strength(&a),
                            platform_snap.get_action_raw_strength(&a),
                        );
                        (a, pair)
                    })
                    .collect(),
            };
            // A gamepad-only frame used to produce NO snapshot at all, so a pad
            // could never be seen from a script however well the transport
            // worked (pat-0q2kx.88).
            let has_input = !script_snap.pressed_keys.is_empty()
                || !script_snap.just_pressed_keys.is_empty()
                || !script_snap.mouse_buttons_pressed.is_empty()
                || !script_snap.actions_pressed.is_empty()
                || !script_snap.actions_just_pressed.is_empty()
                || !script_snap.actions_just_released.is_empty()
                || !script_snap.gamepad_buttons_pressed.is_empty()
                || !script_snap.gamepad_axis_values.is_empty()
                || mouse_pos != gdcore::math::Vector2::ZERO;
            if has_input {
                self.tree.set_input_snapshot(script_snap);
            }
        }

        // Sync trace frame counter so trace events have the correct frame.
        self.tree.set_trace_frame(self.frame_counter);

        // -- input phase (pat-nzgdu.83) --
        // Dispatch each event queued since the last step through the tree's
        // staged propagation (_input -> _shortcut_input -> _unhandled_key_input
        // -> _unhandled_input) before the physics and process phases, so a
        // script reacting to an event does so on the same frame it arrived.
        if !self.pending_input.is_empty() {
            // 🔑 A Control's rect is resolved by LAYOUT, and layout only ran in
            // the render pass. Pointer hit-testing therefore used whatever
            // rect the last render wrote — none at all in a run that steps
            // without rendering (the agent interface's frame-exact `play`), or
            // one from a different window size: an anchored Start button
            // could not be clicked. Godot lays Controls out in the scene tree,
            // independent of drawing; this resolves it before the GUI routes a
            // pointer event, with the render's own pass so the two agree.
            // Only when a pointer event is pending, so a frame with no clicks
            // costs nothing extra.
            let has_pointer = self.pending_input.iter().any(|e| {
                matches!(
                    e,
                    InputEvent::MouseButton { .. }
                        | InputEvent::MouseMotion { .. }
                        | InputEvent::ScreenTouch { .. }
                        | InputEvent::ScreenDrag { .. }
                )
            });
            if has_pointer {
                let window = *self.tree.root_window();
                let size = crate::canvas_render::canvas_size_of(&window);
                let root = self.tree.root_id();
                let _ =
                    crate::canvas_render::build_control_render_items(&mut self.tree, root, size);
            }
            for event in std::mem::take(&mut self.pending_input) {
                self.tree.push_input(&event);
                // Physics object picking is Godot's LAST input stage, after
                // `_unhandled_input` (viewport.cpp:3464-3580) — and it lives
                // here rather than in `push_input` because it needs the physics
                // world, which the scene tree does not own (pat-0q2kx.138).
                self.physics_pick_input(&event);
                // 3D picking runs in the same stage and AFTER the 2D pass, so a
                // 2D collider that handles the click stops it reaching the world
                // behind it — matching Godot, where the 2D pass is dispatched
                // first and each dispatch can mark the event handled
                // (pat-0q2kx.202).
                self.physics_pick_input_3d(&event);

                // -- touch-button phase (pat-0q2kx.190) --
                // TouchScreenButton hit-tests the pointer against its shape (or
                // texture) and injects its `action`, so an on-screen control
                // drives the same actions a key binding would. The edits come
                // back rather than being applied in place because the routing
                // holds the tree borrow while `input_state` is a sibling field.
                for edit in crate::behavioral_2d::route_touch_screen_buttons(&mut self.tree, &event)
                {
                    if edit.pressed {
                        self.input_state.action_press(&edit.action, 1.0);
                    } else {
                        self.input_state.action_release(&edit.action);
                    }
                }
            }
        }

        let physics_dt = 1.0 / self.physics_ticks_per_second as f64;

        // Whether the tree is paused, sampled once so every phase below agrees
        // (pat-0q2kx.140). Godot's `SceneTree::set_pause`
        // (scene/main/scene_tree.cpp:1111-1140) calls
        // `PhysicsServer2D/3D::set_active(!paused)`, so a paused game does not
        // step physics at all — the whole phase is skipped rather than each body
        // being filtered. Before this, a paused Patina game kept simulating
        // everything except script callbacks: bodies fell, timers counted down,
        // animations and tweens advanced.
        let paused = self.tree.paused();

        // -- physics phase --
        // Godot per-tick order: INTERNAL_PHYSICS_PROCESS -> PHYSICS_PROCESS
        // While paused the accumulator is still fed, so unpausing does not
        // suddenly run a burst of catch-up ticks for the paused interval — it is
        // drained below instead.
        //
        // 🔑 Paused, the tick STILL iterates — Godot's `Main::iteration` runs
        // `physics_process` every fixed step whatever the pause state, and
        // `set_pause` only calls `PhysicsServer2D/3D::set_active(false)`. So
        // `physics_frame` fires, `Engine.get_physics_frames()` advances, and a
        // `PROCESS_MODE_ALWAYS` / `WHEN_PAUSED` node's `_physics_process` runs
        // (the script, timer and tween phases filter per node). What stops is
        // every server/solver/mixer pass, guarded by `simulate`. The whole
        // loop used to be skipped, so an "always" node froze under pause.
        let simulate = !paused;
        self.physics_accumulator += delta_secs;
        let mut physics_steps = 0u32;
        // `Engine.is_in_physics_frame()` is true for the whole fixed-step
        // section, so a script called from `_physics_process` can tell which
        // callback it is running in (pat-0q2kx.121).
        gdscript_interop::set_engine_in_physics_frame(true);
        while self.physics_accumulator >= physics_dt
            && physics_steps < self.max_physics_steps_per_frame
        {
            // -- registration pass (pat-0q2kx.201) --
            // The 2D solver phase now runs at the END of the tick, after the
            // script phase, so `sync_to_physics` — which is what adds a newly
            // entered node to the world and drops a freed one — no longer runs
            // before this tick's query phase. Registering here keeps a
            // scene-loaded body visible to the RayCast2D/ShapeCast2D pass and
            // to a `_physics_process` handler on the VERY FIRST tick, which is
            // the one tick the previous tick's sync cannot cover.
            //
            // Registration only: `sync_to_physics` also applies `constant_force`
            // and drains the one-shot `_pending_*` properties, so calling the
            // whole thing twice in a tick would double every constant force.
            if simulate {
                self.physics_server.sync_registrations(&self.tree);

                // 3D physics: sync -> step -> sync (pat-u3l8c.1).
                // `_integrate_forces` runs before the sync for the same reason the
                // 2D pass does: the sync is what carries a rigid body's velocity
                // into the solver, so a script's write has to be on the node by
                // then (pat-s3rrv.94).
                // Servers bound for the callback (pat-1unro): `get_gravity()`
                // inside `_integrate_forces` reads the 3D server, and unbound it
                // fell back to the built-in 9.8 — so the platformer's enemies
                // fell at 9.8 while the project (and the player) used 22.
                self.bind_script_servers();
                crate::integrate_forces::run_integrate_forces_3d(
                    &mut self.tree,
                    self.physics_server_3d.gravity(),
                    physics_dt as f32,
                );
                self.unbind_script_servers();
                self.physics_server_3d.sync_to_physics(&self.tree);
                self.physics_server_3d.step(physics_dt as f32);
                self.physics_server_3d.sync_from_physics(&mut self.tree);

                // -- ragdoll writeback (pat-0q2kx.203) --
                // Simulated PhysicalBone3D bodies push their transforms onto the
                // Skeleton3D bone poses, so the skinned mesh follows the ragdoll.
                // After sync_from_physics so it writes THIS tick's solved bodies.
                self.physics_server_3d
                    .write_back_ragdoll_poses(&mut self.tree);

                // Drive CharacterBody3D nodes, mirroring the 2D character pass
                // above. Runs AFTER step/sync so the sweep tests against this
                // tick's body positions. Nothing called the (already complete)
                // 3D move_and_slide before this, so a CharacterBody3D in a scene
                // never moved and `is_on_floor()` was always false (pat-0q2kx.36).
                crate::character3d::process_character_movement(
                    &mut self.tree,
                    &self.physics_server_3d,
                    physics_dt as f32,
                );

                // -- navigation avoidance phase (pat-nzgdu.116) --
                // ORCA runs per physics tick, like Godot's NavigationServer: agents
                // with a desired velocity get a safe one back via velocity_computed.
                // Emitted before the script phase so a _physics_process handler sees
                // the velocity solved for this very tick.
                self.process_navigation_avoidance(physics_dt as f32);

                // Animation players whose `callback_mode_process` is PHYSICS advance
                // here rather than in the idle phase below, so an animation driving a
                // body samples once per tick in lockstep with the solver — Godot's
                // `AnimationMixer::_physics_process` (pat-0q2kx.1). Before the script
                // phase, so a `_physics_process` handler reads this tick's pose.
                self.tree.process_animations_in_mode(
                    physics_dt,
                    crate::animation::AnimationCallbackMode::Physics,
                );
                // pat-s3rrv.23: AnimationTree honours `callback_mode_process` too —
                // it extends AnimationMixer, where the property lives. Trees used to
                // advance unconditionally in the idle phase below, so a tree set to
                // PHYSICS sampled once per rendered frame instead of once per tick.
                // After the players, matching the idle phase's ordering, so a tree's
                // blended pose still wins where both write the same property.
                self.tree.process_animation_trees_in_mode(
                    physics_dt,
                    crate::animation::AnimationCallbackMode::Physics,
                );
            } // end `if simulate` (servers, solvers, mixers)

            // `TWEEN_PROCESS_PHYSICS` tweens advance here — once per fixed tick,
            // in lockstep with the solver — instead of at render rate in the
            // idle phase below (pat-s3rrv.46). Both deltas are `physics_dt` for
            // the same reason the timer phase below passes it twice: the fixed
            // step is not scaled, so there is no separate unscaled step to hand
            // an `ignore_time_scale` tween here.
            //
            // Runs paused too: `can_process` filters each tween by the pause
            // state, so a `TWEEN_PROCESS_PHYSICS` + `TWEEN_PAUSE_PROCESS`
            // tween keeps advancing under pause, as in Godot.
            self.tree.process_tweens_in_mode(
                physics_dt,
                physics_dt,
                crate::tween::TweenProcessMode::Physics,
            );

            // SceneTree.physics_frame fires before each physics tick's script
            // phase, mirroring Godot's per-tick emission (pat-nzgdu.84).
            let physics_frame_root = self.tree.root_id();
            self.tree
                .emit_signal(physics_frame_root, "physics_frame", &[]);
            self.tree.process_internal_physics_frame();
            // -- raycast phase (pat-u3l8c.49, moved here by pat-0q2kx.32) --
            // Godot's RayCast2D updates under
            // `NOTIFICATION_INTERNAL_PHYSICS_PROCESS`
            // (`scene/2d/physics/ray_cast_2d.cpp`: `set_physics_process_internal`
            // / the `INTERNAL_PHYSICS_PROCESS` arm), so the cast is already
            // published by the time any `_physics_process` runs. This used to sit
            // in the idle section *after* the script phase, which made
            // `is_colliding()` read false for the whole first frame — the exact
            // thing this bead is about — and ran it once per frame rather than
            // once per physics tick.
            if simulate {
                self.process_raycasts();
                self.process_shapecasts();
                // The 3D twin, moved into the tick for the same reason
                // (pat-s3rrv.106). `process_queries_3d` drives RayCast3D,
                // ShapeCast3D and SpringArm3D, and its only caller was the per-FRAME
                // idle block AFTER this script phase — so a `_physics_process`
                // handler read `is_colliding()` from the PREVIOUS frame, and false
                // for the whole first one. Godot updates these under
                // `NOTIFICATION_INTERNAL_PHYSICS_PROCESS`, i.e. before any script
                // runs, exactly as the 2D pair above now do.
                //
                // ⚠️ It also ran once per FRAME rather than once per physics TICK.
                // With several fixed steps in one frame, every tick after the first
                // saw a stale cast.
                self.process_queries_3d();
            }
            // Bind the physics server for the script phase so a
            // `_physics_process` handler can drive `PhysicsServer2D` directly
            // (pat-0q2kx.200). Bracketed tightly around the phase: the tree
            // holds only the address, so it must not outlive this statement, and
            // the next thing to touch the server is the following tick's step.
            // What a script-phase `move_and_slide()` moves by (pat-mgixk.37).
            // Recorded with the server address, so the two always describe the
            // same tick.
            self.tree.set_script_physics_delta(physics_dt);
            self.tree
                .set_script_physics_server(&mut self.physics_server);
            self.tree
                .set_script_physics_server_3d(&mut self.physics_server_3d);
            // Same bracket for the audio server: `AudioServer.set_bus_volume_db`
            // from a `_physics_process` handler is ordinary Godot (pat-s3rrv.6).
            self.tree
                .set_script_audio_server(self.audio_bridge.server_mut());
            self.tree.process_physics_frame_with_scripts(physics_dt);
            self.tree.clear_script_audio_server();
            self.tree.clear_script_physics_server();
            self.tree.clear_script_physics_server_3d();
            // -- 2D solver phase (pat-0q2kx.201) --
            // Godot's `Main::iteration` runs the scene tree's physics process —
            // every `_physics_process`, and so every `move_and_slide`, impulse
            // and velocity write a script makes — BEFORE
            // `PhysicsServer2D::step` (`main.cpp`: physics_process, then
            // end_sync + step). Stepping first, as this loop used to, left a
            // scripted velocity sitting on the node until the NEXT tick: one
            // tick of latency on every scripted body, and physics traces that
            // could not match Godot's step for step.
            //
            // So the whole 2D phase — forces, sync, step, state publish,
            // signals and character movement — now runs here, at the end of the
            // tick, reading what the script phase above just wrote.
            //
            // ⚠️ A scene with no scripts is unaffected: the step still happens
            // exactly once per tick with the same inputs, so the recorded
            // traces are unchanged. Only a scripted body moves, and it moves
            // one tick EARLIER — which is the bug.
            // Sync scene node positions to physics world before stepping.
            // A script may have painted or erased cells since the last tick
            // (pat-s3rrv.88). Tile collision is keyed off `tile_map_data`, which
            // only `register_bodies` used to re-read — so a scripted `set_cell`
            // showed up in pixels but never in physics. Fingerprinted per layer,
            // so an unchanged map costs one comparison.
            if simulate {
                self.physics_server.refresh_tilemap_bodies(&self.tree);
                // `_integrate_forces` runs BEFORE the sync, not after: the script's
                // writes to the state land on the node, and `sync_to_physics` is
                // what carries a rigid body's velocity and pending forces into the
                // solver. Running it after would leave those writes on the node
                // until the next tick — one frame late (pat-s3rrv.94).
                // The server is passed in so the state can report the SOLVER's
                // inverse inertia rather than a recomputation (pat-mgixk.38).
                // `tree` and `physics_server` are disjoint fields, so the mutable
                // and shared borrows do not overlap.
                let gravity_2d = self.physics_server.gravity();
                crate::integrate_forces::run_integrate_forces_2d(
                    &mut self.tree,
                    &self.physics_server,
                    gravity_2d,
                    physics_dt as f32,
                );
                self.physics_server.sync_to_physics(&mut self.tree);
                self.physics_server.step_physics(physics_dt as f32);
                self.physics_server.sync_from_physics(&mut self.tree);

                // Emit body_entered / body_exited signals on Area2D nodes.
                self.physics_server.process_overlap_signals(&mut self.tree);

                // Emit area_entered / area_exited signals between overlapping Area2D
                // nodes (pat-u3l8c.51).
                self.physics_server
                    .process_area_overlap_signals(&mut self.tree);

                // Emit body_entered / body_exited signals on contact-monitoring
                // RigidBody2D nodes (contact_monitor + max_contacts_reported).
                self.physics_server
                    .process_body_contact_signals(&mut self.tree);

                // Publish this step's contacts for `_integrate_forces(state)` to
                // report (pat-s3rrv.94).
                //
                // ⚠️ Separate from the signal pass above, which reports enter/exit
                // EDGES and skips persisting contacts — a body resting on a floor
                // persists, so it would otherwise be visible for one frame only.
                //
                // The `_integrate_forces` callback runs BEFORE the step, so what a
                // script reads is the previous step's contact set. That is Godot's
                // behaviour too: the state reports the last solve, not one that has
                // not happened yet.
                self.physics_server.publish_body_contacts(&mut self.tree);

                // Run move_and_slide for CharacterBody2D nodes with velocity.
                self.physics_server
                    .process_character_movement(physics_dt as f32, &mut self.tree);

                self.physics_server.record_trace(&self.tree);
                // -- interpolation capture (pat-0q2kx.219) --
                // End of the tick: record the transforms this tick settled on,
                // rolling the previous ones back a slot. Runs after the script
                // phase so a `_physics_process` that moved a body is included —
                // the rendered blend must interpolate what actually happened.
                if self.physics_interpolation_enabled {
                    self.physics_interpolation.capture(&self.tree);
                    // The 3D bodies settle in the same tick (pat-0q2kx.250), so
                    // they are captured here too rather than on a separate cadence
                    // — a second capture point would drift out of step with the 2D
                    // one and publish blends from different ticks.
                    self.physics_interpolation_3d.capture(&self.tree);
                }
            } // end `if simulate` (2D solver, contacts, interpolation)

            // SceneTreeTimers created with `process_in_physics = true` advance
            // here, on the fixed tick, and are skipped by the idle pass
            // (pat-s3rrv.35).
            //
            // Both deltas are `physics_dt`: the physics step is the fixed
            // `1.0 / physics_ticks_per_second` and is NOT scaled — time_scale
            // changes how many ticks run, not how long each one is (the
            // accumulator above is fed the scaled delta). So there is no
            // separate unscaled step to hand an `ignore_time_scale` timer here,
            // and passing one would be inventing a number.
            self.tree.process_timers_phase(physics_dt, physics_dt, true);

            // Flush the deferred queue at the END OF EVERY PHYSICS TICK, not
            // just once per frame (pat-mgixk.10).
            //
            // Godot's SceneTree flushes the MessageQueue after `_physics_process`
            // as well as after `_process` (scene/main/scene_tree.cpp:647, :704,
            // :711). Patina flushed only after the idle pass, so with more than
            // one tick per frame — which is the normal case at 60 physics ticks
            // against a slower frame rate — a `call_deferred` issued from
            // `_physics_process` was not applied until AFTER every remaining
            // tick of that frame had already run. A body that deferred its own
            // `queue_free` kept colliding for the rest of the frame, and the
            // canonical `get_parent().call_deferred("add_child", n)` from a
            // physics callback could not be seen by the next tick.
            //
            // ⚠️ Inside the loop, after the tick's own work. Flushing before it
            // would apply the PREVIOUS tick's deferrals in the middle of this
            // one's; flushing after the loop is what it already did.
            self.tree.process_deferred_calls();

            self.physics_accumulator -= physics_dt;
            self.physics_time += physics_dt;
            physics_steps += 1;
            self.physics_tick_counter += 1;
            // Publish inside the loop so a script running in `_physics_process`
            // reads the tick it is currently in, not last frame's count.
            gdscript_interop::set_engine_frame_counters(
                self.frame_counter,
                self.physics_tick_counter,
                self.frame_counter,
            );
        }
        gdscript_interop::set_engine_in_physics_frame(false);

        // -- interpolation publish (pat-0q2kx.219) --
        // How far this render frame sits past the last completed tick. With
        // physics at 20 tps and 60 fps rendering, two frames in three do no
        // physics at all — those are exactly the frames that used to show a
        // stale transform and now show a blended one.
        if self.physics_interpolation_enabled {
            let fraction = (self.physics_accumulator / physics_dt) as f32;
            self.physics_interpolation
                .publish(&mut self.tree, fraction, true);
            // Same fraction for 3D (pat-0q2kx.250): both dimensions advance on
            // the one accumulator, so recomputing it per dimension could only
            // introduce a discrepancy.
            self.physics_interpolation_3d
                .publish(&mut self.tree, fraction, true);
        }

        // Clamp accumulator if we hit the step limit (spiral-of-death guard).
        if physics_steps >= self.max_physics_steps_per_frame {
            self.physics_accumulator = 0.0;
        }

        // -- animation / tween phase --
        // Idle-mode players only; PHYSICS players already advanced in the tick
        // loop above and MANUAL players are never advanced by the loop.
        //
        // Every simulation phase from here to the timer phase is skipped while
        // the tree is paused (pat-0q2kx.140). ⚠️ Godot filters these *per node*
        // by `process_mode`, so an `Always` AnimationPlayer keeps playing under
        // pause; this whole-phase skip does not model that yet — see the note on
        // `paused` above and the bead's follow-up.
        if !paused {
            self.tree.process_animations(delta_secs);
            // AnimationTree nodes advance in the same phase as the players they pull
            // clips from, after them so a tree's blended pose wins where both write
            // the same property (pat-0q2kx.2).
            self.tree.process_animation_trees(delta_secs);
            // AnimatedSprite2D advances its own frame counter in the idle phase,
            // independent of any AnimationPlayer (pat-0q2kx.74).
            self.tree.process_animated_sprites(delta_secs);
            // VideoStreamPlayer advances in the same idle phase and emits
            // `finished` at the end of a non-looping stream. Inside the `paused`
            // guard on purpose: Godot's SceneTree pause stops video with
            // everything else (pat-0q2kx.182).
            self.tree.process_video_players(delta_secs);
            self.tree.process_tweens_in_mode(
                delta_secs,
                real_delta_secs,
                crate::tween::TweenProcessMode::Idle,
            );

            // -- XR node phase (pat-s3rrv.219) --
            // Copy each XRNode3D's tracker pose onto its transform before the
            // skeleton phase, so a SkeletonModifier3D targeting a controller
            // sees this frame's pose rather than last frame's — Godot delivers
            // poses ahead of NOTIFICATION_UPDATE_SKELETON for the same reason.
            //
            // `tracking_changed` is emitted out here rather than inside the walk
            // because a connected script may touch the tree, which the walk
            // holds mutably.
            for id in crate::xr_nodes::update_xr_nodes(&mut self.tree) {
                self.tree.emit_signal(id, "tracking_changed", &[]);
            }

            // -- skeleton phase (pat-0q2kx.3) --
            // Godot's NOTIFICATION_UPDATE_SKELETON: after the mixer has written
            // poses, each Skeleton3D routes its 3D bone tracks onto bone poses, runs
            // its SkeletonModifier3D stack (IK layers on top of animation), then
            // snaps BoneAttachment3D children to their bone. The render bridge reads
            // the resulting poses to skin bound MeshInstance3D meshes.
            crate::skeleton3d::process_skeletons_with_delta(&mut self.tree, delta_secs);
            // pat-s3rrv.185: the 2D bone phase, immediately after the 3D one.
            // Both write poses that the render pass reads later in the same
            // frame, so a skeleton posed here is skinned before anything draws.
            crate::skeleton2d::process_skeletons_2d(&mut self.tree);

            // -- particle phase (pat-u3l8c.4) --
            // Advance CPU particle simulators for emitting particle nodes so that
            // GPUParticles2D/3D and CPUParticles2D/3D actually simulate at runtime.
            self.process_particles(delta_secs as f32);

            // -- path-follow phase (pat-u3l8c.6) --
            // Project PathFollow2D/3D nodes onto their parent Path's curve each
            // frame so their transform tracks `progress` (and any auto-advance).
            self.process_path_follows(delta_secs as f32);

            // -- behavioral 2D nodes phase (pat-0q2kx.190) --
            // RemoteTransform2D copies its transform onto its remote target,
            // then the visibility notifiers test their rects against the
            // camera-visible rect and emit screen_entered/screen_exited. The
            // remote push runs first so a notifier moved by one this frame is
            // tested at the position it actually ended up in.
            crate::behavioral_2d::process_remote_transforms(&mut self.tree);
            // -- Camera2D scroll integration (pat-s3rrv.84) --
            // Godot's `Camera2D::_update_scroll` runs on the process
            // notification, not in the renderer, and keeps the resolved centre
            // on the node between frames. Drag margins measure their dead zone
            // from that centre and position smoothing eases from it, so both
            // are identities without it. Runs after the remote push (a
            // RemoteTransform2D may drive the camera) and before the notifiers,
            // so they test against the centre this frame actually renders.
            crate::behavioral_2d::process_camera2d_scroll(&mut self.tree, delta_secs as f32);
            // -- Parallax phase (pat-s3rrv.86) --
            // Immediately after the camera scroll, because it reads the centre
            // that pass just resolved: a smoothed or drag-held camera must
            // parallax against the view it actually renders, not the target it
            // is still easing toward. Before the notifiers for the same reason
            // the camera pass is — a layer moved here is tested where it ends up.
            crate::parallax_runtime::process_parallax(&mut self.tree, delta_secs as f32);
            crate::behavioral_2d::process_visibility_notifiers(&mut self.tree);
            // Materializes is_pressed on buttons no input event has reached, so
            // an untouched button reads false rather than Nil. Idempotent, so it
            // cannot clobber a press routed earlier in this same frame.
            crate::behavioral_2d::process_touch_screen_buttons(&mut self.tree);

            // -- RemoteTransform3D phase (pat-0q2kx.197) --
            // The 3D sibling of the pass above: forwards each RemoteTransform3D's
            // transform onto its remote_path target. `remote_target_transform`
            // held the per-channel masking all along but had no non-test caller.
            crate::node3d::process_remote_transforms_3d(&mut self.tree);

            // -- 3D visibility phase (pat-0q2kx.198) --
            // Frustum-cull each VisibleOnScreenNotifier3D's AABB against the
            // current Camera3D and emit screen_entered/screen_exited on the
            // transition. Runs after the remote-transform pass so a notifier
            // moved by one this frame is tested where it actually ended up.
            crate::visibility_3d::process_visibility_notifiers_3d(&mut self.tree);

            // -- navigation phase (pat-u3l8c.5) --
            // Drive NavigationAgent2D/3D nodes along their computed paths so agents
            // with a target_position advance across frames at runtime.
            self.process_navigation(delta_secs as f32);

            // -- 3D query-node phase --
            // MOVED into the physics tick by pat-s3rrv.106, exactly as the 2D
            // pair was by pat-0q2kx.32. Running it here, after the script phase,
            // meant `_physics_process` always read the PREVIOUS frame's cast.
            // Godot updates these under `NOTIFICATION_INTERNAL_PHYSICS_PROCESS`,
            // so there is no idle-phase update to keep: the 2D twins have none
            // either, and leaving one here would recompute every cast a second
            // time per frame for no observable gain.

            // -- Timer nodes (Godot Timer, idle process mode) (pat-u3l8c.3) --
            // A default-process_mode Timer stops while paused, which is why Godot's
            // pause menus use `Always` timers (pat-0q2kx.140).
            self.tree.process_timer_nodes(delta_secs);
            // pat-s3rrv.78: HTTPRequest nodes poll their transport on the same
            // per-frame pass as timers, so `request_completed` reaches a script
            // with no Rust-side driving — which is the whole wiring requirement.
            self.tree.process_http_requests();
        } else {
            // The one phase that does NOT stop dead at pause (pat-s3rrv.46).
            // Godot decides per tween: `TWEEN_PAUSE_PROCESS` keeps a pause-menu
            // animation moving, and `TWEEN_PAUSE_BOUND` follows the bound node,
            // so an `Always` node's tween runs while a `Pausable` one's does
            // not. `process_tweens_in_mode` applies `Tween::can_process` per
            // tween, so this call advances exactly the survivors and is a no-op
            // for everything else. It sits in the `else` arm so the unpaused
            // frame's phase ORDER is untouched.
            self.tree.process_tweens_in_mode(
                delta_secs,
                real_delta_secs,
                crate::tween::TweenProcessMode::Idle,
            );
        } // end of the `if !paused` simulation block (pat-0q2kx.140)
          //
          // The raycast and 3D-query phases sit inside this block too. That is
          // deliberate: they publish `is_colliding`/`get_collider` from the
          // physics world, and while paused that world does not change, so
          // re-publishing would burn work to write identical values. Godot
          // reaches the same place via each RayCast's own `process_mode`.

        // -- multiplayer poll (pat-u3l8c.75) --
        // Decode inbound RPCs and dispatch them to node scripts before the
        // process phase, so `_process` sees RPC-driven state this frame.
        // The delta is passed so SceneMultiplayer's authentication timers age
        // with the frame (pat-0q2kx.94) — without a real delta here, an
        // unauthenticated peer would never time out.
        self.tree.poll_multiplayer_with_delta(delta_secs as f32);

        // -- process phase --
        // Godot per-frame order: INTERNAL_PROCESS -> PROCESS
        // Script calls are interleaved per-node to match Godot's dispatch.
        // SceneTree.process_frame fires immediately before the process phase
        // so `await get_tree().process_frame` resumes each frame (pat-nzgdu.84).
        // Godot increments the process-frame counter at the *end* of the
        // iteration, so a script in `_process` on the first frame reads 0
        // (pat-0q2kx.121).
        gdscript_interop::set_engine_frame_counters(
            self.frame_counter,
            self.physics_tick_counter,
            self.frame_counter,
        );
        let process_frame_root = self.tree.root_id();
        self.tree
            .emit_signal(process_frame_root, "process_frame", &[]);
        self.tree.process_internal_frame();
        // Bound here as well as around the physics phase: `PhysicsServer2D` calls
        // from `_process` are ordinary Godot, and leaving the idle phase unbound
        // would hand those scripts an invalid RID with nothing to explain why
        // (pat-0q2kx.200).
        // Deliberately the physics TICK and not `delta_secs`: Godot's
        // `move_and_slide()` reads `get_physics_process_delta_time()` even when
        // a `_process` handler calls it, so using the frame delta here would
        // make the same script move at a different speed out of `_process`
        // (pat-mgixk.37).
        let physics_delta = 1.0 / self.physics_ticks_per_second as f64;
        self.tree.set_script_physics_delta(physics_delta);
        self.tree
            .set_script_physics_server(&mut self.physics_server);
        self.tree
            .set_script_physics_server_3d(&mut self.physics_server_3d);
        // Likewise for `AudioServer`: a `_process` handler ducking a bus or
        // polling a peak meter is the common case, and the mix that reads the
        // result happens later in this same frame (pat-s3rrv.6).
        self.tree
            .set_script_audio_server(self.audio_bridge.server_mut());
        self.tree.process_frame_with_scripts(delta_secs);
        self.tree.clear_script_audio_server();
        self.tree.clear_script_physics_server();
        self.tree.clear_script_physics_server_3d();

        // -- coroutine resume phase (pat-nzgdu.6) --
        // Drive GDScript `await` coroutines: advance SceneTreeTimers once,
        // deliver their timeout signals to parked awaits, and tick frame-based
        // awaits so `await get_tree().create_timer(t).timeout` resumes at
        // runtime instead of hanging forever.
        // `real_delta_secs` is the driver's unscaled delta, which only
        // `ignore_time_scale` timers consume (pat-s3rrv.35).
        self.tree
            .process_script_coroutines_unscaled(delta_secs as f32, real_delta_secs);

        // -- Input singleton mutations (pat-0q2kx.24) --
        // Scripts cannot reach the engine `InputState` (this loop owns it), so
        // `Input.action_press`, `set_mouse_mode`, `warp_mouse` and joypad rumble
        // are queued on the tree during the script phase and applied here, in
        // issue order. The tree's mirrors are then refreshed so a script's
        // `get_mouse_mode`/`get_connected_joypads` reads engine truth next frame.
        self.apply_script_input_commands();

        // -- collision detection phase --
        // Run after scripts so collision properties are set before next frame's _process.
        self.tree.process_collisions();

        // -- Area3D audio bus override (pat-s3rrv.102) --
        // BEFORE the audio phase mixes: a voice rerouted after the mix would be
        // heard on the old bus for one frame, which is exactly the kind of
        // one-frame lag that makes a reverb zone feel wrong on entry.
        self.apply_area3d_audio_bus_overrides();
        self.apply_area2d_audio_bus_overrides();

        // -- audio phase (pat-u3l8c.2) --
        // After scripts so same-frame `playing` changes are heard immediately.
        self.audio_bridge.process(&mut self.tree, delta_secs);

        // -- deferred call phase (Godot MessageQueue flush) --
        // After process notifications; before queue_free deletions.
        self.tree.process_deferred_calls();

        // -- deferred deletion phase --
        // Remove nodes that called queue_free() during this frame.
        self.tree.process_deletions();

        // -- draw phase (pat-0q2kx.82) --
        // Godot's redraw pass: every CanvasItem whose `queue_redraw()` was
        // called runs `_draw` and refills its command list, which the render
        // bridge then folds into the frame. It runs after deletions so a node
        // freed this frame is never asked to draw, and after the script phase so
        // a `queue_redraw()` issued from `_process` is honored this same frame
        // rather than one frame late.
        self.tree.flush_pending_redraws();

        // -- deferred scene change (pat-nzgdu.85) --
        // Godot applies change_scene_to_file() at the frame boundary (after
        // _process and the deferred/deletion phases) so the calling script's
        // scene is not freed mid-run. A load error is non-fatal to the loop.
        //
        // 🔑 The new scene's SCRIPTS are attached and its lifecycle run here
        // (pat-mgixk.62). This phase used to swap the tree and stop: the scene
        // arrived with `_script_path` recorded and no live script, so a game
        // that changed scenes got a visibly-correct tree in which nothing
        // ran — no `_ready`, no `_process`. The boot path had an attach loop
        // and the scene-change path had none.
        //
        // ⚠️ It happens HERE rather than inside `change_scene_to_packed`
        // because `_ready` must run through `enter_tree_with_scripts`, which
        // binds the physics and audio servers for the duration of the call.
        // `SceneTree` has no access to those; only the host loop does. Running
        // `LifecycleManager::enter_tree` without that bracket leaves the
        // singletons unbound and the callbacks silently no-op.
        // Bound before the match so the mutable borrow of `self.tree` provably
        // ends here, leaving the arm free to take it again.
        // 🔑 A PACKED scene is built detached, its scripts attached in the
        // `prepare` hook, and only then added — so its one lifecycle pass
        // reaches the scripts in Godot's order (ENTER top-down, READY
        // bottom-up) and no node is entered twice. The servers are bound
        // around it for the same reason `enter_tree_with_scripts` binds them.
        if self.tree.pending_scene_change_is_packed() {
            self.bind_script_servers();
            let _ = self
                .tree
                .process_deferred_scene_change_prepared(&mut |tree, root| {
                    crate::scripting::attach_subtree_scripts_from_res(tree, root);
                });
            self.unbind_script_servers();
        }
        let swapped = self.tree.process_deferred_scene_change();
        match swapped {
            Ok(Some(new_root)) => {
                // The shared helper from pat-mgixk.61, which reads through
                // `FileAccess` so a scene inside an exported `.pck` works too,
                // and skips any node that already carries a script.
                crate::scripting::attach_subtree_scripts_from_res(&mut self.tree, new_root);
                // `_enter_tree` then `_ready`, once, for the whole new subtree.
                self.enter_tree_with_scripts(new_root);
            }
            // `Ok(None)` is "nothing was queued", or an `unload` that
            // deliberately loaded nothing — neither has a subtree to start.
            // An `Err` is a bad scene path, which stays non-fatal to the loop.
            Ok(None) | Err(_) => {}
        }

        // -- input cleanup --
        // Clear input snapshot so it doesn't leak into the next frame.
        self.tree.clear_input_snapshot();
        // Flush per-frame input state (just_pressed / just_released) so
        // events don't carry over to the next frame.
        self.input_state.flush_frame();

        // -- bookkeeping --
        self.process_time += delta_secs;
        // Publish elapsed seconds for shader `TIME` (pat-s3rrv.122). The render
        // adapter reads this off the tree; without it `CustomFragmentShader`'s
        // clock stayed at 0.0 and every animated shader rendered static.
        self.tree.set_shader_time(self.process_time as f32);

        // -- performance monitors (pat-s3rrv.109) --
        // Fed at the END of the frame, after the deferred and free queues have
        // drained, so `object/node_count` reports the tree a script would see
        // on its next frame rather than a count that includes nodes this frame
        // already freed.
        //
        // `record_frame` takes CUMULATIVE totals and windows `time/fps` over
        // the trailing 1.0 of the first one, so it must be handed elapsed
        // seconds — passing a per-frame delta would make every frame land in
        // the same window and report the whole run as one second.
        {
            let mut registry = gdcore::performance::global().lock().unwrap();
            self.performance_feed
                .record_frame(&mut registry, self.process_time, self.physics_time);
            registry.set(
                gdcore::performance::Monitor::ObjectNodeCount,
                self.tree.node_count() as f64,
            );
        }

        self.frame_counter += 1;
        gdscript_interop::set_engine_frame_counters(
            self.frame_counter,
            self.physics_tick_counter,
            self.frame_counter,
        );

        // Resolved from the hovered control each frame; no hover means the
        // default arrow, which is also what an unhovered viewport shows.
        let cursor_shape = match self.tree.hovered_control() {
            Some(id) => crate::control::get_cursor_shape(&self.tree, id),
            None => crate::control::CursorShape::Arrow,
        };

        // Movie capture runs last, once the frame is fully simulated, so a
        // writer sees the same state a renderer would have drawn.
        self.drive_movie_capture();

        FrameOutput {
            frame_count: self.frame_counter,
            delta: delta_secs,
            physics_steps,
            cursor_shape,
        }
    }

    // -- movie capture (pat-s3rrv.262) --------------------------------------

    /// Starts a `MovieWriter` capture session targeting `base_path`.
    ///
    /// Godot enters this mode via `--write-movie <path>`; there is no such CLI
    /// on the web target, so a host calls this instead. The writer is not chosen
    /// here — selection needs `_handles_file`, which is a script call, so it
    /// happens on the first captured frame.
    pub fn start_movie_capture(
        &mut self,
        base_path: impl Into<String>,
        size: (i64, i64),
        fps: i64,
    ) {
        let (w, h) = (size.0.max(0), size.1.max(0));
        self.movie_capture = Some(MovieCapture {
            base_path: base_path.into(),
            size: (w, h),
            fps,
            writer: None,
            begun: false,
            // Opaque black rather than transparent: an unpublished frame should
            // look like an unlit frame, not like a hole.
            pixels: vec![
                gdcore::math::Color::new(0.0, 0.0, 0.0, 1.0);
                (w as usize).saturating_mul(h as usize)
            ],
        });
    }

    /// Publishes the pixels the host just rendered, used by the next
    /// `_write_frame`.
    ///
    /// Ignored when no capture is running, so a host may call it unconditionally.
    pub fn set_movie_frame_pixels(&mut self, pixels: Vec<gdcore::math::Color>) {
        if let Some(cap) = self.movie_capture.as_mut() {
            cap.pixels = pixels;
        }
    }

    /// Whether a capture session is running.
    pub fn movie_capture_active(&self) -> bool {
        self.movie_capture.is_some()
    }

    /// Ends the session, calling `_write_end` on the writer that was driving it.
    ///
    /// Returns whether a writer was actually closed — false when no capture was
    /// running, or when one was but no registered writer ever accepted the path.
    pub fn finish_movie_capture(&mut self) -> bool {
        let Some(cap) = self.movie_capture.take() else {
            return false;
        };
        let Some(nid) = cap.writer else {
            return false;
        };
        let _ = self.tree.call_script_method(nid, "_write_end", &[]);
        true
    }

    /// The first registered writer whose `_handles_file` accepts `path`.
    ///
    /// Registration order, like Godot: the first acceptor wins and the rest are
    /// never asked.
    fn select_movie_writer(&mut self, path: &str) -> Option<NodeId> {
        for writer in gdobject::movie_writer::instance_writers() {
            // A name-only registration from Rust has no instance to call back
            // into, so it cannot handle a file.
            let Some(raw) = writer.object_id else {
                continue;
            };
            let nid = NodeId::from_object_id(gdcore::id::ObjectId::from_raw(raw));
            let handled = self.tree.call_script_method(
                nid,
                "_handles_file",
                &[Variant::String(path.to_string())],
            );
            if matches!(handled, Some(Variant::Bool(true))) {
                return Some(nid);
            }
        }
        None
    }

    /// Drives `_write_begin` once and `_write_frame` once per frame.
    ///
    /// ⚠️ `_write_frame`'s second parameter is Godot's `audio_frame_block`,
    /// typed `const void*` in the docs — a raw pointer, which means the virtual
    /// is overridable only from C++/GDExtension there. A GDScript writer cannot
    /// receive it, so `Nil` is passed and the arity still matches the six-virtual
    /// surface ClassDB declares. The image half, which is what a script can
    /// actually use, is real.
    fn drive_movie_capture(&mut self) {
        // Copied out before any script call: the calls need `&mut self.tree`
        // while a borrow of `self.movie_capture` would still be live.
        let Some((base_path, size, fps, writer, begun)) = self
            .movie_capture
            .as_ref()
            .map(|c| (c.base_path.clone(), c.size, c.fps, c.writer, c.begun))
        else {
            return;
        };

        let writer = if begun {
            writer
        } else {
            let chosen = self.select_movie_writer(&base_path);
            if let Some(nid) = chosen {
                let args = [
                    Variant::Vector2i(gdcore::math::Vector2i::new(size.0 as i32, size.1 as i32)),
                    Variant::Int(fps),
                    Variant::String(base_path.clone()),
                ];
                let _ = self.tree.call_script_method(nid, "_write_begin", &args);
            }
            // Recorded even when nothing accepted the path, so `_handles_file`
            // is asked once per session rather than once per frame.
            if let Some(cap) = self.movie_capture.as_mut() {
                cap.writer = chosen;
                cap.begun = true;
            }
            chosen
        };

        let Some(nid) = writer else {
            return;
        };

        let pixels = self
            .movie_capture
            .as_ref()
            .map(|c| c.pixels.clone())
            .unwrap_or_default();
        let image =
            gdscript_interop::interpreter::new_image_handle_from_pixels(size.0, size.1, pixels);
        let _ = self.tree.call_script_method(
            nid,
            "_write_frame",
            &[Variant::ObjectId(image), Variant::Nil],
        );
    }

    /// Runs exactly `n` frames, each with the given `delta` seconds.
    ///
    /// This is the primary entry point for deterministic testing — the same
    /// `n` and `delta` values always produce the same results.
    pub fn run_frames(&mut self, n: u64, delta: f64) {
        for _ in 0..n {
            self.step(delta);
        }
    }

    /// Advances one frame and returns a [`FrameRecord`] capturing per-frame
    /// trace events and end-of-frame node properties.
    ///
    /// This enables frame-by-frame property evolution validation. The event
    /// trace is automatically enabled for the duration of the step and the
    /// events for this frame are captured into the returned record.
    pub fn step_traced(&mut self, delta_secs: f64) -> FrameRecord {
        // Enable tracing for this frame.
        let was_enabled = self.tree.event_trace().is_enabled();
        self.tree.event_trace_mut().enable();
        let events_before = self.tree.event_trace().events().len();

        let output = self.step(delta_secs);

        // Capture events that occurred during this frame.
        let all_events = self.tree.event_trace().events();
        let frame_events: Vec<TraceEvent> = all_events[events_before..].to_vec();

        // Capture node property snapshots.
        let node_snapshots = self.snapshot_nodes();

        // Restore previous trace state.
        if !was_enabled {
            self.tree.event_trace_mut().disable();
        }

        FrameRecord {
            frame_number: output.frame_count,
            delta: output.delta,
            physics_ticks: output.physics_steps,
            cumulative_process_time: self.process_time,
            cumulative_physics_time: self.physics_time,
            paused: self.paused,
            events: frame_events,
            node_snapshots,
        }
    }

    /// Runs `n` frames with the given `delta` and returns a [`FrameTrace`]
    /// capturing per-frame evolution data.
    ///
    /// This is the primary entry point for validating frame-by-frame property
    /// evolution against Godot contracts.
    pub fn run_frames_traced(&mut self, n: u64, delta: f64) -> FrameTrace {
        let mut trace = FrameTrace::default();
        for _ in 0..n {
            trace.frames.push(self.step_traced(delta));
        }
        trace
    }

    /// Captures a snapshot of all nodes' properties at the current moment.
    fn snapshot_nodes(&self) -> Vec<NodeSnapshot> {
        let ids = self.tree.all_nodes_in_tree_order();
        let mut snapshots = Vec::with_capacity(ids.len());
        for id in ids {
            if let Some(node) = self.tree.get_node(id) {
                let path = self.tree.node_path(id).unwrap_or_else(|| format!("<{id}>"));
                let mut properties = HashMap::new();
                for (k, v) in node.properties() {
                    properties.insert(k.clone(), v.clone());
                }
                snapshots.push(NodeSnapshot {
                    path,
                    class_name: node.class_name().to_string(),
                    properties,
                });
            }
        }
        snapshots
    }

    /// Runs one frame using a [`PlatformBackend`] for event sourcing.
    ///
    /// 1. Drains window events from the backend and pushes input events
    ///    into the engine-owned [`InputState`].
    /// 2. Calls [`step`](Self::step) with the given `delta_secs`.
    /// 3. Signals `end_frame()` on the backend.
    ///
    /// Returns the [`FrameOutput`] from the step.
    ///
    /// Callback-driven backends (e.g. winit) should call this inside their
    /// per-frame callback. Pull-based backends can use [`run`](Self::run).
    pub fn run_frame(
        &mut self,
        backend: &mut dyn gdplatform::PlatformBackend,
        delta_secs: f64,
    ) -> FrameOutput {
        // Drain platform events into engine-owned input.
        for event in backend.poll_events() {
            if let Some(input_event) = event.to_input_event() {
                self.push_event(input_event);
            }
        }

        let output = self.step(delta_secs);
        backend.end_frame();
        output
    }

    /// Runs a pull-based frame loop until the backend signals quit.
    ///
    /// Each iteration calls [`run_frame`](Self::run_frame) with the given
    /// fixed `delta_secs`. This is the single entry point for headless
    /// runners and batch simulations.
    ///
    /// For callback-driven platforms (e.g. winit), use `run_frame` directly
    /// inside the platform's event callback instead.
    pub fn run(&mut self, backend: &mut dyn gdplatform::PlatformBackend, delta_secs: f64) {
        while !backend.should_quit() {
            self.run_frame(backend, delta_secs);
        }
    }

    /// Returns the total number of frames executed so far.
    pub fn frame_count(&self) -> u64 {
        self.frame_counter
    }

    /// Returns the total elapsed physics time in seconds.
    ///
    /// This only advances in fixed-timestep increments, so it may not
    /// exactly match wall-clock time.
    pub fn physics_time(&self) -> f64 {
        self.physics_time
    }

    /// Returns the total elapsed process time in seconds.
    ///
    /// This is the sum of all `delta_secs` values passed to [`step`](Self::step).
    pub fn process_time(&self) -> f64 {
        self.process_time
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node::Node;

    /// pat-s3rrv.116: the flattened `color_ramp` becomes the emitter's
    /// `ColorCurve`, so a particle's colour changes across its lifetime.
    ///
    /// This is the half `packed_scene`'s flattening feeds. It lives here rather
    /// than in `tests/` because `read_emitter_3d` and `color_ramp_curve` are
    /// private — the emitter has no public colour accessor to observe from
    /// outside, and adding one purely to be observed would be the wrong trade.
    mod color_ramp {
        use super::*;
        use gdcore::math::Color;

        fn node_with_ramp(offsets: Vec<f32>, colors: Vec<Color>) -> Node {
            let mut n = Node::new("Emitter", "GPUParticles3D");
            n.set_property("__color_ramp_offsets", Variant::PackedFloat32Array(offsets));
            n.set_property("__color_ramp_colors", Variant::PackedColorArray(colors));
            n
        }

        const RED: Color = Color {
            r: 1.0,
            g: 0.0,
            b: 0.0,
            a: 1.0,
        };
        const BLUE: Color = Color {
            r: 0.0,
            g: 0.0,
            b: 1.0,
            a: 1.0,
        };

        /// **The bead's acceptance shape**: colour at 10% of life differs from
        /// colour at 90%.
        #[test]
        fn a_ramp_makes_the_colour_change_over_lifetime() {
            let node = node_with_ramp(vec![0.0, 1.0], vec![RED, BLUE]);
            let emitter = read_emitter_3d(&node);
            let early = emitter.material.color_curve.sample(0.1);
            let late = emitter.material.color_curve.sample(0.9);

            assert!(
                early.r > late.r && late.b > early.b,
                "expected a red-to-blue fade, got {early:?} then {late:?}"
            );
            // ⚠️ Not just "different": the old behaviour was
            // `ColorCurve::linear(c, c)`, a gradient whose two stops are EQUAL.
            // That samples to one colour everywhere, so the inequality above is
            // exactly what used to be impossible.
            assert!(
                (early.r - late.r).abs() > 0.1,
                "a degenerate one-colour ramp would make these equal"
            );
        }

        /// ⚠️ The two stop arrays are PARALLEL — coupled by their count. Zipping
        /// a mismatched pair would drop the tail and shift every colour onto the
        /// wrong offset, so a malformed bag is refused outright and the emitter
        /// keeps its flat-colour path.
        #[test]
        fn mismatched_stop_arrays_are_refused_rather_than_zipped_short() {
            let node = node_with_ramp(vec![0.0, 0.5, 1.0], vec![RED, BLUE]);
            assert!(color_ramp_curve(&node).is_none());
        }

        /// Godot keeps its stops sorted; a hand-written scene need not.
        /// `ColorCurve::sample` short-circuits on the first and last key, so
        /// unsorted stops would read as a truncated gradient.
        #[test]
        fn unsorted_stops_are_sorted_before_sampling() {
            let node = node_with_ramp(vec![1.0, 0.0], vec![BLUE, RED]);
            let curve = color_ramp_curve(&node).expect("a valid ramp");
            assert_eq!(curve.keys[0].0, 0.0, "keys must ascend");
            assert!(
                curve.sample(0.0).r > curve.sample(1.0).r,
                "red is still the start after sorting"
            );
        }

        /// The negative control: with no ramp the emitter keeps the flat colour,
        /// which is what every existing particle scene relies on.
        #[test]
        fn without_a_ramp_the_flat_colour_still_wins() {
            let mut n = Node::new("Emitter", "GPUParticles3D");
            n.set_property("color", Variant::Color(RED));
            assert!(color_ramp_curve(&n).is_none());

            let emitter = read_emitter_3d(&n);
            let c = emitter.material.color_curve.sample(0.5);
            assert_eq!((c.r, c.b), (1.0, 0.0), "flat red across the lifetime");
        }
    }

    /// pat-s3rrv.119: the `scale_curve` half. Its own module so the
    /// `color_ramp` sibling keeps the exact test-name prefix pat-s3rrv.116
    /// was verified with.
    mod authored_scale_curve {
        use super::*;

        /// pat-s3rrv.119: the baked `scale_curve` samples become the emitter's
        /// `ScaleCurve`, so a particle changes size across its lifetime.
        #[test]
        fn a_scale_curve_shrinks_the_particle_over_lifetime() {
            let mut n = Node::new("Emitter", "GPUParticles3D");
            n.set_property(
                "__scale_curve_samples",
                Variant::PackedFloat32Array(vec![1.0, 0.75, 0.5, 0.25]),
            );
            let emitter = read_emitter_3d(&n);
            let curve = &emitter.material.scale_curve;

            // ⚠️ The samples are evenly spaced over the domain, so four of them
            // are keyed at 0, 1/3, 2/3 and 1 — not at 0..3. Keying by index
            // would put every sample beyond t=1 and `sample` would clamp to the
            // first, reading as "the curve does nothing".
            assert!((curve.sample(0.0) - 1.0).abs() < 1e-4);
            assert!((curve.sample(1.0) - 0.25).abs() < 1e-4);
            assert!(
                curve.sample(0.0) > curve.sample(1.0),
                "the default is a flat 1.0, so a descending sample proves the \
                 authored curve landed"
            );
        }

        /// ⚠️ A single sample has no span to spread over and `i / (n - 1)` would
        /// divide by zero. It is still a meaningful curve — a constant — so it
        /// maps to one key rather than being refused or producing NaN.
        #[test]
        fn a_single_sample_curve_is_a_constant_not_a_division_by_zero() {
            let mut n = Node::new("Emitter", "GPUParticles3D");
            n.set_property(
                "__scale_curve_samples",
                Variant::PackedFloat32Array(vec![0.5]),
            );
            let curve = scale_curve_from_samples(&n).expect("one sample is a valid curve");
            assert_eq!(curve.keys.len(), 1);
            assert!(curve.sample(0.0).is_finite() && curve.sample(1.0).is_finite());
            assert!((curve.sample(0.7) - 0.5).abs() < 1e-4);
        }

        /// With no curve the emitter keeps its default, which every existing
        /// particle scene relies on.
        #[test]
        fn without_a_scale_curve_the_default_survives() {
            let n = Node::new("Emitter", "GPUParticles3D");
            assert!(scale_curve_from_samples(&n).is_none());
            let emitter = read_emitter_3d(&n);
            assert!((emitter.material.scale_curve.sample(0.5) - 1.0).abs() < 1e-4);
        }
    }
    use gdobject::notification::{
        NOTIFICATION_POST_ENTER_TREE,
        NOTIFICATION_ENTER_TREE, NOTIFICATION_INTERNAL_PHYSICS_PROCESS,
        NOTIFICATION_INTERNAL_PROCESS, NOTIFICATION_PARENTED, NOTIFICATION_PHYSICS_PROCESS,
        NOTIFICATION_POSTINITIALIZE, NOTIFICATION_PROCESS, NOTIFICATION_READY,
    };

    /// pat-nzgdu.84: SceneTree/Node lifecycle + per-frame signals fire. A
    /// handler on the tree's `process_frame`/`physics_frame` receives a callback
    /// when the scene is stepped through MainLoop; `node_added` fires on
    /// add_child, `node_renamed` on set_node_name, and `tree_entered` when a
    /// node enters the tree. All signals ride the existing per-node SignalStore.
    #[test]
    fn scene_tree_and_node_signals_fire_via_main_loop() {
        use gdcore::id::ObjectId;
        use gdobject::signal::Connection;
        use gdvariant::Variant;
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;

        // A recording connection: bumps `cell` each time its signal fires. The
        // dummy target id is unused because the connection carries a callback.
        fn recorder(cell: &Arc<AtomicUsize>, tag: u64) -> Connection {
            let c = cell.clone();
            Connection::with_callback(ObjectId::from_raw(tag), "on_signal", move |_args| {
                c.fetch_add(1, Ordering::SeqCst);
                Variant::Nil
            })
        }

        let mut tree = SceneTree::new();
        let root = tree.root_id();

        // node_added fires when a live node enters the tree.
        let added = Arc::new(AtomicUsize::new(0));
        tree.connect_signal(root, "node_added", recorder(&added, 1));
        let child = tree.add_child(root, Node::new("Child", "Node")).unwrap();
        assert_eq!(
            added.load(Ordering::SeqCst),
            1,
            "node_added fires on add_child"
        );

        // tree_entered fires on the node as it enters the tree. The child is
        // already in-tree, so re-run the enter phase to observe the emission.
        let entered = Arc::new(AtomicUsize::new(0));
        tree.connect_signal(child, "tree_entered", recorder(&entered, 2));
        crate::lifecycle::LifecycleManager::enter_tree(&mut tree, child);
        assert!(
            entered.load(Ordering::SeqCst) >= 1,
            "tree_entered fires when the node enters the tree",
        );

        // node_renamed fires on a SceneTree-level rename.
        let renamed = Arc::new(AtomicUsize::new(0));
        tree.connect_signal(root, "node_renamed", recorder(&renamed, 3));
        assert!(
            tree.set_node_name(child, "Renamed"),
            "rename hits a live node"
        );
        assert_eq!(
            renamed.load(Ordering::SeqCst),
            1,
            "node_renamed fires on set_node_name",
        );

        // process_frame + physics_frame fire when stepped through MainLoop.
        let process = Arc::new(AtomicUsize::new(0));
        let physics = Arc::new(AtomicUsize::new(0));
        tree.connect_signal(root, "process_frame", recorder(&process, 4));
        tree.connect_signal(root, "physics_frame", recorder(&physics, 5));
        let mut main_loop = MainLoop::new(tree);
        // A 0.1s delta guarantees at least one physics tick at any normal tps.
        main_loop.step(0.1);
        assert!(
            process.load(Ordering::SeqCst) >= 1,
            "process_frame fires once per MainLoop step",
        );
        assert!(
            physics.load(Ordering::SeqCst) >= 1,
            "physics_frame fires for the physics tick(s) in the step",
        );
    }

    /// Helper: build a MainLoop with a tree containing root + one child.
    fn make_loop_with_child() -> (MainLoop, crate::node::NodeId) {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let child = Node::new("Child", "Node");
        let child_id = tree.add_child(root, child).unwrap();
        (MainLoop::new(tree), child_id)
    }

    /// App-level: a RigidBody3D in the tree must fall under gravity when the
    /// game is driven only through `MainLoop::step` (pat-u3l8c.1).
    #[test]
    fn physics_3d_rigid_body_falls_via_main_loop() {
        use gdcore::math::Vector3;

        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let body = Node::new("Ball", "RigidBody3D");
        let body_id = tree.add_child(root, body).unwrap();
        crate::node3d::set_position(&mut tree, body_id, Vector3::new(0.0, 10.0, 0.0));

        let mut ml = MainLoop::new(tree);
        for _ in 0..30 {
            ml.step(1.0 / 60.0);
        }

        assert!(ml.physics_server_3d().body_count() >= 1);
        let pos = crate::node3d::get_position(ml.tree(), body_id);
        assert!(
            pos.y < 9.9,
            "RigidBody3D should fall via MainLoop::step, got y={}",
            pos.y,
        );
    }

    /// App-level: an emitting particle node advances its CPU simulator every
    /// frame when driven only through `MainLoop::step`, accumulating live
    /// particles and publishing the count back onto the node (pat-u3l8c.4).
    #[test]
    fn mainloop_emitting_particle_node_advances_and_count_positive() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let mut node = Node::new("Sparks", "GPUParticles2D");
        node.set_property("emitting", gdvariant::Variant::Bool(true));
        node.set_property("amount", gdvariant::Variant::Int(64));
        node.set_property("lifetime", gdvariant::Variant::Float(2.0));
        let pid = tree.add_child(root, node).unwrap();

        let mut ml = MainLoop::new(tree);
        // No live particles before stepping.
        assert_eq!(ml.particle_count(pid), 0);
        for _ in 0..60 {
            ml.step(1.0 / 60.0);
        }

        // The simulator must have emitted and be tracking live particles.
        let live = ml.particle_count(pid);
        assert!(
            live > 0,
            "emitting particle node should have live particles after stepping, got {}",
            live,
        );

        // And the live count must be published back onto the node so game code
        // / the editor can observe it.
        match ml
            .tree()
            .get_node(pid)
            .map(|n| n.get_property("particle_count"))
        {
            Some(gdvariant::Variant::Int(c)) => assert!(
                c > 0,
                "particle_count property should be published > 0, got {}",
                c,
            ),
            other => panic!("expected particle_count Int property, got {:?}", other),
        }
    }

    /// App-level: a particle node with `emitting = false` produces no live
    /// particles when stepped through `MainLoop::step` (pat-u3l8c.4).
    #[test]
    fn mainloop_non_emitting_particle_node_stays_empty() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let mut node = Node::new("Idle", "CPUParticles2D");
        node.set_property("emitting", gdvariant::Variant::Bool(false));
        node.set_property("amount", gdvariant::Variant::Int(32));
        node.set_property("lifetime", gdvariant::Variant::Float(2.0));
        let pid = tree.add_child(root, node).unwrap();

        let mut ml = MainLoop::new(tree);
        for _ in 0..60 {
            ml.step(1.0 / 60.0);
        }

        assert_eq!(
            ml.particle_count(pid),
            0,
            "non-emitting particle node must not accumulate particles",
        );
    }

    /// 🔑 `cubic_interp` changes the sampled position, and DEFAULTS TO TRUE
    /// (pat-s3rrv.91 item 2).
    ///
    /// ⚠️ The fixture must be genuinely CURVED. Catmull-Rom through collinear
    /// points reproduces the straight line exactly, so on any of the straight
    /// fixtures the sibling tests use, cubic and linear agree to the bit and
    /// this test would pass while reading nothing at all.
    ///
    /// ⚠️ The second assertion is the one that catches the likelier bug: Godot's
    /// default is TRUE, so an absent property must sample CUBIC. Reading the
    /// property as false-by-default is the natural Rust reflex
    /// (`matches!(.., Bool(true))`) and would silently downgrade every scene
    /// that never set it.
    #[test]
    fn mainloop_pathfollow3d_cubic_interp_defaults_on_and_changes_the_sample() {
        use gdcore::math::Vector3;

        fn sampled(cubic: Option<bool>) -> Vector3 {
            let mut tree = SceneTree::new();
            let root = tree.root_id();

            let mut path = Node::new("Path", "Path3D");
            path.set_property(
                "curve_points",
                gdvariant::Variant::PackedVector3Array(vec![
                    Vector3::new(0.0, 0.0, 0.0),
                    Vector3::new(1.0, 0.0, 0.0),
                    Vector3::new(2.0, 1.0, 0.0),
                    Vector3::new(3.0, 3.0, 0.0),
                ]),
            );
            let path_id = tree.add_child(root, path).unwrap();

            let mut follow = Node::new("Follow", "PathFollow3D");
            follow.set_property("progress", gdvariant::Variant::Float(1.5));
            if let Some(c) = cubic {
                follow.set_property("cubic_interp", gdvariant::Variant::Bool(c));
            }
            let fid = tree.add_child(path_id, follow).unwrap();

            let mut ml = MainLoop::new(tree);
            ml.step(1.0 / 60.0);
            crate::node3d::get_position(ml.tree(), fid)
        }

        let linear = sampled(Some(false));
        let cubic = sampled(Some(true));
        let default = sampled(None);

        assert!(
            (cubic - linear).length() > 1e-3,
            "cubic sampling must differ from linear on a curved path: {cubic:?} \
             vs {linear:?}. Equal means cubic_interp is being ignored — or the \
             fixture went straight",
        );
        assert!(
            (default - cubic).length() < 1e-6,
            "an absent cubic_interp must sample CUBIC, since Godot's default is \
             true; got {default:?}, cubic is {cubic:?} and linear {linear:?}",
        );
    }

    /// 🔑 `use_model_front` reverses which way the follower faces
    /// (pat-s3rrv.91 item 3).
    ///
    /// Godot's models face -Z; a model authored facing +Z sets this flag and
    /// Godot answers with `t.basis *= Basis::from_scale(-1, 1, -1)`
    /// (`path_3d.cpp:293-296`). Negating the X and Z columns is a 180° turn
    /// about the frame's own Y, so the forward axis reverses exactly.
    ///
    /// ⚠️ Asserted as `flipped == -plain` rather than against a hard-coded
    /// direction: that holds whatever convention the surrounding rotation modes
    /// use, so this test pins the FLIP and cannot accidentally re-assert the
    /// yaw convention its siblings already cover.
    #[test]
    fn mainloop_pathfollow3d_use_model_front_reverses_facing() {
        use gdcore::math::Vector3;
        use gdcore::math3d::Basis;

        fn forward(model_front: bool) -> Vector3 {
            let mut tree = SceneTree::new();
            let root = tree.root_id();

            let mut path = Node::new("Path", "Path3D");
            path.set_property(
                "curve_points",
                gdvariant::Variant::PackedVector3Array(vec![
                    Vector3::new(0.0, 0.0, 0.0),
                    Vector3::new(10.0, 0.0, 0.0),
                ]),
            );
            let path_id = tree.add_child(root, path).unwrap();

            let mut follow = Node::new("Follow", "PathFollow3D");
            follow.set_property("progress", gdvariant::Variant::Float(5.0));
            // ROTATION_XYZ — there must be a frame to flip; ROTATION_NONE
            // publishes no euler and Godot skips the flip there too.
            follow.set_property("rotation_mode", gdvariant::Variant::Int(3));
            if model_front {
                follow.set_property("use_model_front", gdvariant::Variant::Bool(true));
            }
            let fid = tree.add_child(path_id, follow).unwrap();

            let mut ml = MainLoop::new(tree);
            ml.step(1.0 / 60.0);
            // Godot's forward is -Z of the basis.
            -Basis::from_euler(crate::node3d::get_rotation(ml.tree(), fid)).z
        }

        let plain = forward(false);
        let flipped = forward(true);

        assert!(
            plain.length() > 0.5,
            "the control must have a real facing to reverse, got {plain:?}",
        );
        let sum = plain + flipped;
        assert!(
            sum.length() < 1e-3,
            "use_model_front must reverse the facing exactly: {plain:?} vs \
             {flipped:?} (their sum should cancel, got {sum:?}). An unchanged \
             value means the property is still being ignored",
        );
    }

    /// App-level: a PathFollow3D child projects onto its parent Path3D curve so
    /// its transform tracks `progress` as it changes, including Y-mode rotation
    /// that faces the direction of travel (pat-u3l8c.6).
    #[test]
    fn mainloop_pathfollow3d_projects_progress_with_rotation() {
        use gdcore::math::Vector3;
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        // Straight path from the origin along +X, arc length 10.
        let mut path = Node::new("Path", "Path3D");
        path.set_property(
            "curve_points",
            gdvariant::Variant::PackedVector3Array(vec![
                Vector3::new(0.0, 0.0, 0.0),
                Vector3::new(10.0, 0.0, 0.0),
            ]),
        );
        let path_id = tree.add_child(root, path).unwrap();

        let mut follow = Node::new("Follow", "PathFollow3D");
        follow.set_property("progress", gdvariant::Variant::Float(0.0));
        follow.set_property("rotation_mode", gdvariant::Variant::Int(1)); // Y
        let fid = tree.add_child(path_id, follow).unwrap();

        let mut ml = MainLoop::new(tree);
        ml.step(1.0 / 60.0);
        let p0 = crate::node3d::get_position(ml.tree(), fid);
        assert!(
            p0.x.abs() < 1e-2,
            "progress 0 should place follow at curve start, got x={}",
            p0.x,
        );

        // Change progress; the projected transform must track it next frame.
        ml.tree_mut()
            .get_node_mut(fid)
            .unwrap()
            .set_property("progress", gdvariant::Variant::Float(5.0));
        ml.step(1.0 / 60.0);
        let p1 = crate::node3d::get_position(ml.tree(), fid);
        assert!(
            (p1.x - 5.0).abs() < 1e-2,
            "follow transform should track progress to x=5, got x={}",
            p1.x,
        );

        // Y rotation faces the +X tangent with Godot's convention: **-Z** along
        // the tangent, i.e. `yaw = atan2(-t.x, -t.z) = -PI/2`.
        //
        // ⚠️ THIS ASSERTED +PI/2 UNTIL pat-s3rrv.91, and +PI/2 was the bug. The
        // driver used `atan2(t.x, t.z)` — a "+Z is forward" convention it
        // described as "internally consistent" — which is exactly PI away from
        // Godot's on every tangent, so every follower faced backwards along its
        // own path. The sign here moved because the CODE was corrected, not
        // because this fixture was wrong about the tangent.
        let rot = crate::node3d::get_rotation(ml.tree(), fid);
        assert!(
            (rot.y + std::f32::consts::FRAC_PI_2).abs() < 1e-2,
            "Y rotation should put -Z along the +X tangent (~-PI/2), got {}",
            rot.y,
        );
    }

    /// App-level: `path_speed` auto-advances a PathFollow3D along its parent
    /// curve every frame via `MainLoop::step` (exercises the previously-unused
    /// `PathFollow3D::advance`) (pat-u3l8c.6).
    #[test]
    fn mainloop_pathfollow3d_auto_advances_with_path_speed() {
        use gdcore::math::Vector3;
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let mut path = Node::new("Path", "Path3D");
        path.set_property(
            "curve_points",
            gdvariant::Variant::PackedVector3Array(vec![
                Vector3::new(0.0, 0.0, 0.0),
                Vector3::new(10.0, 0.0, 0.0),
            ]),
        );
        let path_id = tree.add_child(root, path).unwrap();

        let mut follow = Node::new("Follow", "PathFollow3D");
        follow.set_property("path_speed", gdvariant::Variant::Float(60.0)); // units/sec
        follow.set_property("loop", gdvariant::Variant::Bool(false));
        // ⚠️ LINEAR sampling, stated explicitly (pat-s3rrv.91 item 2). This test
        // is about `path_speed` advancing PROGRESS and uses position only as a
        // proxy for it, so it needs the proxy to be linear in arc length.
        //
        // `cubic_interp` now defaults to TRUE, matching Godot, and cubic
        // sampling is NOT linear in arc length on an END segment: Godot clamps
        // the Catmull-Rom neighbours there (`pre = r[idx]`, `post = r[idx+1]`),
        // which reduces to `A + (B-A)(0.5w + 1.5w² - w³)`. This fixture's baked
        // array is a single 2-point segment, so the whole curve IS that end
        // segment and `w = 0.1` samples 0.064 rather than 0.1 — x = 0.64.
        //
        // A real scene never looks like this: the loader bakes 16 samples per
        // segment, so the clamped ends are a sixteenth of the curve and the
        // deviation is small. The sibling test at `progress = 5.0` survives
        // untouched because `w = 0.5` is the one interior point where the
        // clamped form and the linear form agree exactly.
        follow.set_property("cubic_interp", gdvariant::Variant::Bool(false));
        let fid = tree.add_child(path_id, follow).unwrap();

        let mut ml = MainLoop::new(tree);
        ml.step(1.0 / 60.0); // advances 60 * (1/60) = 1 unit
        let p1 = crate::node3d::get_position(ml.tree(), fid);
        assert!(
            (p1.x - 1.0).abs() < 1e-2,
            "after one frame follow should be ~1 unit along, got x={}",
            p1.x,
        );

        ml.step(1.0 / 60.0); // advances another unit
        let p2 = crate::node3d::get_position(ml.tree(), fid);
        assert!(
            p2.x > p1.x + 0.5,
            "follow should keep moving along the path each frame ({} -> {})",
            p1.x,
            p2.x,
        );
    }

    /// App-level: a PathFollow2D child tracks its parent Path2D curve and
    /// rotates to face travel when `rotates` is set (pat-u3l8c.6).
    #[test]
    fn mainloop_pathfollow2d_follows_curve_and_rotates() {
        use gdcore::math::Vector2;
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        // Straight path from the origin along +Y, arc length 10.
        let mut path = Node::new("Path", "Path2D");
        path.set_property(
            "curve_points",
            gdvariant::Variant::PackedVector2Array(vec![
                Vector2::new(0.0, 0.0),
                Vector2::new(0.0, 10.0),
            ]),
        );
        let path_id = tree.add_child(root, path).unwrap();

        let mut follow = Node::new("Follow", "PathFollow2D");
        follow.set_property("progress", gdvariant::Variant::Float(0.0));
        follow.set_property("rotates", gdvariant::Variant::Bool(true));
        let fid = tree.add_child(path_id, follow).unwrap();

        let mut ml = MainLoop::new(tree);
        ml.step(1.0 / 60.0);
        let p0 = crate::node2d::get_position(ml.tree(), fid);
        assert!(
            p0.length() < 1e-2,
            "progress 0 should be at curve start, got {:?}",
            p0
        );

        ml.tree_mut()
            .get_node_mut(fid)
            .unwrap()
            .set_property("progress", gdvariant::Variant::Float(5.0));
        ml.step(1.0 / 60.0);
        let p1 = crate::node2d::get_position(ml.tree(), fid);
        assert!(
            (p1.y - 5.0).abs() < 1e-2 && p1.x.abs() < 1e-2,
            "follow should track progress to (0,5), got {:?}",
            p1,
        );

        // Rotation faces the +Y tangent: atan2(1, 0) = PI/2.
        let rot = crate::node2d::get_rotation(ml.tree(), fid);
        assert!(
            (rot - std::f32::consts::FRAC_PI_2).abs() < 1e-2,
            "2D rotation should face the +Y tangent (~PI/2), got {}",
            rot,
        );
    }

    /// App-level: rotation mode `None` positions the PathFollow3D child but
    /// leaves its rotation untouched (pat-u3l8c.6).
    #[test]
    fn mainloop_pathfollow3d_rotation_none_leaves_rotation() {
        use gdcore::math::Vector3;
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let mut path = Node::new("Path", "Path3D");
        path.set_property(
            "curve_points",
            gdvariant::Variant::PackedVector3Array(vec![
                Vector3::new(0.0, 0.0, 0.0),
                Vector3::new(10.0, 0.0, 0.0),
            ]),
        );
        let path_id = tree.add_child(root, path).unwrap();

        let mut follow = Node::new("Follow", "PathFollow3D");
        follow.set_property("progress", gdvariant::Variant::Float(5.0));
        // ⚠️ ROTATION_NONE is now set EXPLICITLY. This used to rely on it being
        // the default, which it is not: Godot's default `rotation_mode` is
        // ROTATION_XYZ (3) (`doc/classes/PathFollow3D.xml`), and the driver's
        // old `_ => None` fallback meant a follower authored without the
        // property — the common case — did not rotate at all (pat-s3rrv.91).
        // The companion test below pins that corrected default, so the two
        // together cover what this one alone used to claim.
        follow.set_property("rotation_mode", gdvariant::Variant::Int(0));
        // Seed a distinctive rotation, which NONE must leave alone.
        follow.set_property(
            "rotation",
            gdvariant::Variant::Vector3(Vector3::new(0.5, 0.5, 0.5)),
        );
        let fid = tree.add_child(path_id, follow).unwrap();

        let mut ml = MainLoop::new(tree);
        ml.step(1.0 / 60.0);

        // Position still projects onto the curve...
        let p = crate::node3d::get_position(ml.tree(), fid);
        assert!(
            (p.x - 5.0).abs() < 1e-2,
            "None mode still positions, got x={}",
            p.x
        );
        // ...but rotation is left as seeded.
        let rot = crate::node3d::get_rotation(ml.tree(), fid);
        assert!(
            (rot.x - 0.5).abs() < 1e-6 && (rot.y - 0.5).abs() < 1e-6 && (rot.z - 0.5).abs() < 1e-6,
            "rotation mode None must not modify rotation, got {:?}",
            rot,
        );
    }

    /// 🔑 An ABSENT `rotation_mode` means ROTATION_XYZ, not NONE.
    ///
    /// Godot documents the default as `3` (`doc/classes/PathFollow3D.xml`). The
    /// driver used to fall through to NONE, so a follower authored without the
    /// property never rotated — and the test above could not see it, because it
    /// relied on that same wrong default to express "NONE leaves rotation
    /// alone". Splitting the two is what makes either meaningful.
    #[test]
    fn mainloop_pathfollow3d_absent_rotation_mode_defaults_to_xyz() {
        use gdcore::math::Vector3;
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let mut path = Node::new("Path", "Path3D");
        path.set_property(
            "curve_points",
            gdvariant::Variant::PackedVector3Array(vec![
                Vector3::new(0.0, 0.0, 0.0),
                Vector3::new(10.0, 0.0, 0.0),
            ]),
        );
        let path_id = tree.add_child(root, path).unwrap();

        // No `rotation_mode` at all, and a seeded rotation that must be replaced.
        let mut follow = Node::new("Follow", "PathFollow3D");
        follow.set_property("progress", gdvariant::Variant::Float(5.0));
        follow.set_property(
            "rotation",
            gdvariant::Variant::Vector3(Vector3::new(0.5, 0.5, 0.5)),
        );
        let fid = tree.add_child(path_id, follow).unwrap();

        let mut ml = MainLoop::new(tree);
        ml.step(1.0 / 60.0);

        let rot = crate::node3d::get_rotation(ml.tree(), fid);
        assert!(
            (rot.y + std::f32::consts::FRAC_PI_2).abs() < 1e-2,
            "an absent rotation_mode must orient like XYZ (-Z along the +X \
             tangent, yaw ~-PI/2), got {:?}",
            rot,
        );
    }

    /// One frame of a game that steers by its agent, as Godot projects do:
    /// the engine ticks, then "the body's script" moves the agent node toward
    /// the published `next_path_position` at `speed` units/s.
    ///
    /// 🔑 The NavigationAgent is PASSIVE in Godot (pat-s3rrv.20) — it never
    /// moves anything. These tests were written for an agent that walked its
    /// own path, so after that fix nothing moved it and they waited forever.
    fn step_steering_2d(ml: &mut MainLoop, id: crate::node::NodeId, speed: f32) {
        let dt = 1.0 / 60.0;
        ml.step(dt as f64);
        let next = match ml
            .tree()
            .get_node(id)
            .map(|n| n.get_property("next_path_position"))
        {
            Some(gdvariant::Variant::Vector2(v)) => v,
            _ => return,
        };
        let pos = crate::node2d::get_position(ml.tree(), id);
        let to = next - pos;
        let step = (speed * dt).min(to.length());
        if step > 0.0 {
            let moved = pos + to.normalized() * step;
            crate::node2d::set_position(ml.tree_mut(), id, moved);
        }
    }

    /// The 3D twin of [`step_steering_2d`].
    fn step_steering_3d(ml: &mut MainLoop, id: crate::node::NodeId, speed: f32) {
        let dt = 1.0 / 60.0;
        ml.step(dt as f64);
        let next = match ml
            .tree()
            .get_node(id)
            .map(|n| n.get_property("next_path_position"))
        {
            Some(gdvariant::Variant::Vector3(v)) => v,
            _ => return,
        };
        let pos = crate::node3d::get_position(ml.tree(), id);
        let to = next - pos;
        let step = (speed * dt).min(to.length());
        if step > 0.0 {
            let moved = pos + to.normalized() * step;
            crate::node3d::set_position(ml.tree_mut(), id, moved);
        }
    }

    /// App-level: a NavigationAgent2D with a target advances along a computed
    /// multi-waypoint path across frames, its `get_next_path_position` guidance
    /// progressing from an intermediate waypoint to the target (pat-u3l8c.5).
    #[test]
    fn mainloop_navigation_agent_2d_paths_across_frames() {
        use gdcore::math::Vector2;
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        // Three adjacent navmesh polygons forming an L: along +X, then up +Y.
        //
        // ⚠️ Not a straight corridor any more: Godot's default
        // `path_postprocessing` (CORRIDORFUNNEL) pulls a straight corridor
        // taut to `[start, end]`, so it has no intermediate waypoint. The old
        // (50,0) expectation was a polygon CENTROID, which only the
        // pre-funnel pathing produced.
        let regions = [
            vec![
                Vector2::new(-10.0, -10.0),
                Vector2::new(30.0, -10.0),
                Vector2::new(30.0, 10.0),
                Vector2::new(-10.0, 10.0),
            ],
            vec![
                Vector2::new(30.0, -10.0),
                Vector2::new(70.0, -10.0),
                Vector2::new(70.0, 10.0),
                Vector2::new(30.0, 10.0),
            ],
            vec![
                Vector2::new(30.0, 10.0),
                Vector2::new(70.0, 10.0),
                Vector2::new(70.0, 50.0),
                Vector2::new(30.0, 50.0),
            ],
        ];
        for verts in regions {
            let mut r = Node::new("Region", "NavigationRegion2D");
            r.set_property(
                // `__navmesh_vertices__`, not `navmesh_vertices`: pat-0q2kx.26
                // moved region geometry onto the dunder key that `PackedScene`
                // resolves the NavigationPolygon/NavigationMesh resource onto,
                // and `process_navigation` reads only that key (:1437, :1498).
                // These hand-built nodes were left on the old name, so every
                // region contributed zero polygons and the agent's path length
                // was 0 rather than the asserted 3.
                "__navmesh_vertices__",
                gdvariant::Variant::PackedVector2Array(verts),
            );
            tree.add_child(root, r).unwrap();
        }

        let mut agent = Node::new("Agent", "NavigationAgent2D");
        agent.set_property(
            "position",
            gdvariant::Variant::Vector2(Vector2::new(0.0, 0.0)),
        );
        agent.set_property(
            "target_position",
            gdvariant::Variant::Vector2(Vector2::new(50.0, 40.0)),
        );
        let aid = tree.add_child(root, agent).unwrap();

        let mut ml = MainLoop::new(tree);

        // Read helper for the published `next_path_position` property.
        fn next2(ml: &MainLoop, id: crate::node::NodeId) -> gdcore::math::Vector2 {
            match ml
                .tree()
                .get_node(id)
                .map(|n| n.get_property("next_path_position"))
            {
                Some(gdvariant::Variant::Vector2(v)) => v,
                other => panic!("expected next_path_position Vector2, got {:?}", other),
            }
        }

        // Early: a 3-waypoint path exists; guidance is the inner corner
        // (30,10) and the steered body has begun moving.
        for _ in 0..5 {
            step_steering_2d(&mut ml, aid, 100.0);
        }
        assert!(
            ml.nav_agent_path_len_2d(aid) >= 3,
            "expected a multi-waypoint path, got {}",
            ml.nav_agent_path_len_2d(aid),
        );
        let next_early = next2(&ml, aid);
        let pos_early = crate::node2d::get_position(ml.tree(), aid);
        assert!(
            pos_early.x > 0.0 && pos_early.x < 30.0,
            "agent should have started moving, x={}",
            pos_early.x,
        );
        assert!(
            (next_early - Vector2::new(30.0, 10.0)).length() < 1e-2,
            "first guidance should be the inner corner ~(30,10), got {:?}",
            next_early,
        );

        // Later: the intermediate waypoint is consumed, so guidance advances to
        // the target and the agent has moved further along.
        for _ in 0..90 {
            step_steering_2d(&mut ml, aid, 100.0);
        }
        let next_late = next2(&ml, aid);
        let pos_late = crate::node2d::get_position(ml.tree(), aid);
        assert!(
            pos_late.x > pos_early.x,
            "agent should keep advancing ({} -> {})",
            pos_early.x,
            pos_late.x,
        );
        assert!(
            (next_late - Vector2::new(50.0, 40.0)).length() < 1e-2,
            "guidance should advance to the target ~(50,40), got {:?}",
            next_late,
        );
        assert!(
            (next_late - next_early).length() > 1.0,
            "get_next_path_position must change across frames ({:?} -> {:?})",
            next_early,
            next_late,
        );
        assert!(
            (pos_late - Vector2::new(50.0, 40.0)).length() < 10.0,
            "agent should reach near the target, at {:?}",
            pos_late
        );
    }

    /// pat-nzgdu.10 acceptance: a scene with a NavigationRegion2D + a
    /// NavigationAgent2D, its `target_position` set, produces a
    /// monotonically-advancing `get_next_path_position` sequence and emits
    /// `navigation_finished` exactly once when the MainLoop ticks the agent to
    /// the goal — driven by `MainLoop::step`, not a direct struct call.
    #[test]
    fn navigation_agent_runtime_paths_from_scene() {
        use gdcore::math::Vector2;
        use gdobject::signal::Connection;
        use std::sync::atomic::{AtomicU32, Ordering};
        use std::sync::Arc;

        let mut tree = SceneTree::new();
        let root = tree.root_id();

        // Three adjacent navmesh polygons forming a corridor along +X, so the
        // path carries an intermediate waypoint and guidance advances 50 -> 100.
        for verts in [
            vec![
                Vector2::new(-10.0, -10.0),
                Vector2::new(30.0, -10.0),
                Vector2::new(30.0, 10.0),
                Vector2::new(-10.0, 10.0),
            ],
            vec![
                Vector2::new(30.0, -10.0),
                Vector2::new(70.0, -10.0),
                Vector2::new(70.0, 10.0),
                Vector2::new(30.0, 10.0),
            ],
            vec![
                Vector2::new(70.0, -10.0),
                Vector2::new(110.0, -10.0),
                Vector2::new(110.0, 10.0),
                Vector2::new(70.0, 10.0),
            ],
        ] {
            let mut region = Node::new("Region", "NavigationRegion2D");
            // Dunder key — see the note in
            // `mainloop_navigation_agent_2d_paths_across_frames`.
            region.set_property(
                "__navmesh_vertices__",
                gdvariant::Variant::PackedVector2Array(verts),
            );
            tree.add_child(root, region).unwrap();
        }

        let mut agent = Node::new("Agent", "NavigationAgent2D");
        agent.set_property(
            "position",
            gdvariant::Variant::Vector2(Vector2::new(0.0, 0.0)),
        );
        agent.set_property(
            "target_position",
            gdvariant::Variant::Vector2(Vector2::new(100.0, 0.0)),
        );
        let aid = tree.add_child(root, agent).unwrap();

        // Count `navigation_finished` emissions via a callback connection.
        let finished = Arc::new(AtomicU32::new(0));
        let finished_cb = finished.clone();
        tree.connect_signal(
            aid,
            "navigation_finished",
            Connection::with_callback(aid.object_id(), "_on_nav_finished", move |_| {
                finished_cb.fetch_add(1, Ordering::SeqCst);
                gdvariant::Variant::Nil
            }),
        );

        let mut ml = MainLoop::new(tree);

        fn next_x(ml: &MainLoop, id: crate::node::NodeId) -> f32 {
            match ml
                .tree()
                .get_node(id)
                .map(|n| n.get_property("next_path_position"))
            {
                Some(gdvariant::Variant::Vector2(v)) => v.x,
                other => panic!("expected next_path_position Vector2, got {other:?}"),
            }
        }

        // Tick to the goal, sampling guidance to confirm monotonic advancement.
        let mut samples: Vec<f32> = Vec::new();
        for i in 0..200 {
            step_steering_2d(&mut ml, aid, 100.0);
            if i % 10 == 0 {
                samples.push(next_x(&ml, aid));
            }
            if finished.load(Ordering::SeqCst) > 0 {
                break;
            }
        }

        // navigation_finished fired exactly once at the goal.
        assert_eq!(
            finished.load(Ordering::SeqCst),
            1,
            "navigation_finished must fire exactly once",
        );
        // The agent reached near the target.
        let pos = crate::node2d::get_position(ml.tree(), aid);
        assert!(
            pos.x > 90.0,
            "agent should reach near the target, x={}",
            pos.x
        );
        // Guidance never regressed.
        assert!(samples.len() >= 2, "expected multiple guidance samples");
        for w in samples.windows(2) {
            assert!(
                w[1] >= w[0] - 1e-3,
                "get_next_path_position must not regress ({} -> {})",
                w[0],
                w[1],
            );
        }
        // A straight corridor funnels to `[start, target]` (Godot's default
        // CORRIDORFUNNEL), so the guidance IS the target from the first frame
        // — there is no interior waypoint for it to progress through.
        assert!(
            samples.iter().all(|x| (x - 100.0).abs() < 1e-3),
            "a straight corridor's guidance is the target throughout ({:?})",
            samples,
        );
    }

    /// App-level: a NavigationAgent3D advances toward its target across frames
    /// on a flat navmesh, coming to rest near the target (pat-u3l8c.5).
    #[test]
    fn mainloop_navigation_agent_3d_advances_to_target() {
        use gdcore::math::Vector3;
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        // One flat navmesh polygon on the XZ plane covering the route. Dunder
        // key — see the note in
        // `mainloop_navigation_agent_2d_paths_across_frames`.
        let mut region = Node::new("Region", "NavigationRegion3D");
        region.set_property(
            "__navmesh_vertices__",
            gdvariant::Variant::PackedVector3Array(vec![
                Vector3::new(-10.0, 0.0, -10.0),
                Vector3::new(110.0, 0.0, -10.0),
                Vector3::new(110.0, 0.0, 10.0),
                Vector3::new(-10.0, 0.0, 10.0),
            ]),
        );
        tree.add_child(root, region).unwrap();

        let mut agent = Node::new("Agent", "NavigationAgent3D");
        agent.set_property(
            "position",
            gdvariant::Variant::Vector3(Vector3::new(0.0, 0.0, 0.0)),
        );
        agent.set_property(
            "target_position",
            gdvariant::Variant::Vector3(Vector3::new(50.0, 0.0, 0.0)),
        );
        let aid = tree.add_child(root, agent).unwrap();

        let mut ml = MainLoop::new(tree);

        let start = crate::node3d::get_position(ml.tree(), aid);
        for _ in 0..30 {
            step_steering_3d(&mut ml, aid, 10.0);
        }
        let mid = crate::node3d::get_position(ml.tree(), aid);
        assert!(
            ml.nav_agent_path_len_3d(aid) >= 2,
            "expected a computed path, got {}",
            ml.nav_agent_path_len_3d(aid),
        );
        assert!(
            mid.x > start.x + 1.0,
            "3D agent should advance toward the target ({} -> {})",
            start.x,
            mid.x,
        );

        // Enough frames (3D default max_speed is 10) to arrive.
        for _ in 0..600 {
            step_steering_3d(&mut ml, aid, 10.0);
        }
        let end = crate::node3d::get_position(ml.tree(), aid);
        assert!(
            (end.x - 50.0).abs() < 2.0,
            "3D agent should come to rest near the target x=50, got {}",
            end.x,
        );
    }

    /// App-level: a RayCast2D detects a body in its path via `MainLoop::step`,
    /// reporting `is_colliding` and the collider node (pat-u3l8c.49).
    #[test]
    fn mainloop_raycast2d_detects_body_and_collider() {
        use gdcore::math::Vector2;
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        // Static wall body centered at x=100 (a 10x200 box → left edge at x=95).
        let mut wall = Node::new("Wall", "StaticBody2D");
        wall.set_property("position", Variant::Vector2(Vector2::new(100.0, 0.0)));
        let wall_id = tree.add_child(root, wall).unwrap();
        let mut ws = Node::new("Shape", "CollisionShape2D");
        ws.set_property("size", Variant::Vector2(Vector2::new(10.0, 200.0)));
        tree.add_child(wall_id, ws).unwrap();

        // RayCast2D at the origin pointing right, long enough to reach the wall.
        let mut ray = Node::new("Ray", "RayCast2D");
        ray.set_property("position", Variant::Vector2(Vector2::new(0.0, 0.0)));
        ray.set_property(
            "target_position",
            Variant::Vector2(Vector2::new(150.0, 0.0)),
        );
        let ray_id = tree.add_child(root, ray).unwrap();

        let mut ml = MainLoop::new(tree);
        ml.register_physics_bodies();
        ml.step(1.0 / 60.0);

        let colliding = ml
            .tree()
            .get_node(ray_id)
            .map(|n| n.get_property("is_colliding"));
        assert!(
            matches!(colliding, Some(Variant::Bool(true))),
            "RayCast2D should hit the wall, got {:?}",
            colliding,
        );
        match ml
            .tree()
            .get_node(ray_id)
            .map(|n| n.get_property("collider"))
        {
            Some(Variant::Int(raw)) => {
                assert_eq!(
                    raw as u64,
                    wall_id.raw(),
                    "collider should be the wall node"
                )
            }
            other => panic!("expected collider Int, got {:?}", other),
        }
    }

    /// App-level: a RayCast2D with nothing in its path reports not colliding.
    #[test]
    fn mainloop_raycast2d_misses_empty_path() {
        use gdcore::math::Vector2;
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let mut ray = Node::new("Ray", "RayCast2D");
        ray.set_property("target_position", Variant::Vector2(Vector2::new(0.0, 50.0)));
        let ray_id = tree.add_child(root, ray).unwrap();

        let mut ml = MainLoop::new(tree);
        ml.register_physics_bodies();
        ml.step(1.0 / 60.0);

        let colliding = ml
            .tree()
            .get_node(ray_id)
            .map(|n| n.get_property("is_colliding"));
        assert!(
            matches!(colliding, Some(Variant::Bool(false))),
            "empty-scene ray should not collide, got {:?}",
            colliding,
        );
    }

    /// Helper: build a MainLoop containing a Timer node with the given
    /// properties and a counter incremented on each `timeout` emission.
    fn make_timer_loop(
        wait_time: f64,
        one_shot: bool,
        autostart: bool,
        paused: bool,
    ) -> (MainLoop, std::sync::Arc<std::sync::atomic::AtomicU32>) {
        use gdobject::signal::Connection;

        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let mut timer = Node::new("Timer", "Timer");
        timer.set_property("wait_time", gdvariant::Variant::Float(wait_time));
        timer.set_property("one_shot", gdvariant::Variant::Bool(one_shot));
        timer.set_property("autostart", gdvariant::Variant::Bool(autostart));
        timer.set_property("paused", gdvariant::Variant::Bool(paused));
        let timer_id = tree.add_child(root, timer).unwrap();

        let fired = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
        let fired_in_cb = fired.clone();
        tree.connect_signal(
            timer_id,
            "timeout",
            Connection::with_callback(timer_id.object_id(), "on_timeout", move |_args| {
                fired_in_cb.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                gdvariant::Variant::Nil
            }),
        );

        (MainLoop::new(tree), fired)
    }

    /// App-level: an autostarted one-shot Timer emits `timeout` exactly once
    /// (pat-u3l8c.3).
    #[test]
    fn timer_one_shot_fires_timeout_once_via_main_loop() {
        let (mut ml, fired) = make_timer_loop(0.05, true, true, false);
        for _ in 0..60 {
            ml.step(1.0 / 60.0);
        }
        assert_eq!(fired.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    /// App-level: a repeating Timer keeps emitting `timeout` (pat-u3l8c.3).
    #[test]
    fn timer_repeating_fires_timeout_repeatedly_via_main_loop() {
        let (mut ml, fired) = make_timer_loop(0.05, false, true, false);
        for _ in 0..60 {
            ml.step(1.0 / 60.0);
        }
        let count = fired.load(std::sync::atomic::Ordering::SeqCst);
        assert!(
            count >= 15,
            "repeating timer should fire ~20 times, got {count}"
        );
    }

    /// App-level: a paused Timer never fires; without autostart it stays
    /// stopped (pat-u3l8c.3).
    #[test]
    fn timer_paused_or_stopped_does_not_fire() {
        let (mut ml, fired) = make_timer_loop(0.05, false, true, true);
        for _ in 0..30 {
            ml.step(1.0 / 60.0);
        }
        assert_eq!(fired.load(std::sync::atomic::Ordering::SeqCst), 0);

        let (mut ml2, fired2) = make_timer_loop(0.05, false, false, false);
        for _ in 0..30 {
            ml2.step(1.0 / 60.0);
        }
        assert_eq!(fired2.load(std::sync::atomic::Ordering::SeqCst), 0);
    }

    /// App-level: an autoplaying AudioStreamPlayer produces non-silent mixed
    /// audio when the game is driven only through `MainLoop::step`
    /// (pat-u3l8c.2).
    #[test]
    fn audio_autoplay_mixes_via_main_loop() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let mut sfx = Node::new("Sfx", "AudioStreamPlayer");
        sfx.set_property("autoplay", gdvariant::Variant::Bool(true));
        let sfx_id = tree.add_child(root, sfx).unwrap();

        let mut ml = MainLoop::new(tree);
        let samples: Vec<f32> = (0..44100usize)
            .flat_map(|i| {
                let s = (i as f32 * 2.0 * std::f32::consts::PI * 440.0 / 44100.0).sin() * 0.5;
                [s, s]
            })
            .collect();
        ml.audio_bridge_mut().set_stream_buffer(
            sfx_id,
            gdaudio::AudioBuffer {
                samples,
                sample_rate: 44100,
                channels: 2,
            },
        );

        for _ in 0..3 {
            ml.step(1.0 / 60.0);
        }

        assert!(
            ml.audio_bridge().last_mix().iter().any(|s| s.abs() > 0.01),
            "autoplayed AudioStreamPlayer should mix non-silent samples via MainLoop",
        );
    }

    /// Timer exposes a decreasing `time_left` property while running
    /// (pat-u3l8c.3).
    #[test]
    fn timer_time_left_counts_down() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let mut timer = Node::new("Timer", "Timer");
        timer.set_property("wait_time", gdvariant::Variant::Float(10.0));
        timer.set_property("autostart", gdvariant::Variant::Bool(true));
        let timer_id = tree.add_child(root, timer).unwrap();

        let mut ml = MainLoop::new(tree);
        ml.step(1.0 / 60.0);

        let time_left = match ml
            .tree()
            .get_node(timer_id)
            .unwrap()
            .get_property("time_left")
        {
            gdvariant::Variant::Float(f) => f,
            other => panic!("expected Float time_left, got {other:?}"),
        };
        assert!(
            time_left > 9.0 && time_left < 10.0,
            "time_left should have started counting down, got {time_left}",
        );
    }

    #[test]
    fn single_frame_increments_counter() {
        let (mut ml, _) = make_loop_with_child();
        assert_eq!(ml.frame_count(), 0);
        ml.step(1.0 / 60.0);
        assert_eq!(ml.frame_count(), 1);
    }

    #[test]
    fn ten_frames_increments_counter() {
        let (mut ml, _) = make_loop_with_child();
        ml.run_frames(10, 1.0 / 60.0);
        assert_eq!(ml.frame_count(), 10);
    }

    #[test]
    fn physics_process_fires_with_fixed_timestep() {
        let (mut ml, child_id) = make_loop_with_child();
        // At 60 TPS with delta = 1/60, each frame should produce exactly
        // one physics tick.
        ml.run_frames(5, 1.0 / 60.0);

        let log = ml.tree().get_node(child_id).unwrap().notification_log();
        let physics_count = log
            .iter()
            .filter(|&&n| n == NOTIFICATION_PHYSICS_PROCESS)
            .count();
        assert_eq!(physics_count, 5, "expected 5 physics ticks");
    }

    #[test]
    fn process_notification_dispatched_each_frame() {
        let (mut ml, child_id) = make_loop_with_child();
        ml.run_frames(3, 1.0 / 60.0);

        let log = ml.tree().get_node(child_id).unwrap().notification_log();
        let process_count = log.iter().filter(|&&n| n == NOTIFICATION_PROCESS).count();
        assert_eq!(process_count, 3, "expected 3 process calls");
    }

    #[test]
    fn delta_time_values_are_correct() {
        let (mut ml, _) = make_loop_with_child();
        let delta = 1.0 / 60.0;
        ml.run_frames(60, delta);

        // Process time should be ~1 second.
        let expected_process = 60.0 * delta;
        assert!(
            (ml.process_time() - expected_process).abs() < 1e-10,
            "process_time {} != expected {}",
            ml.process_time(),
            expected_process
        );

        // Physics time should also be ~1 second (60 ticks at 1/60).
        let expected_physics = 60.0 * (1.0 / 60.0);
        assert!(
            (ml.physics_time() - expected_physics).abs() < 1e-10,
            "physics_time {} != expected {}",
            ml.physics_time(),
            expected_physics
        );
    }

    #[test]
    fn deterministic_same_inputs_same_counts() {
        // Run two independent MainLoops with identical parameters and
        // verify they produce identical state.
        let run = || {
            let (mut ml, child_id) = make_loop_with_child();
            ml.run_frames(100, 1.0 / 60.0);
            let log = ml
                .tree()
                .get_node(child_id)
                .unwrap()
                .notification_log()
                .to_vec();
            (ml.frame_count(), ml.physics_time(), ml.process_time(), log)
        };

        let (fc1, pt1, prt1, log1) = run();
        let (fc2, pt2, prt2, log2) = run();

        assert_eq!(fc1, fc2);
        assert_eq!(pt1, pt2);
        assert_eq!(prt1, prt2);
        assert_eq!(log1, log2);
    }

    #[test]
    fn large_delta_caps_physics_steps() {
        let (mut ml, child_id) = make_loop_with_child();
        ml.set_max_physics_steps_per_frame(4);
        // A 1-second delta at 60 TPS would need 60 physics steps,
        // but the cap limits it to 4.
        ml.step(1.0);

        let log = ml.tree().get_node(child_id).unwrap().notification_log();
        let physics_count = log
            .iter()
            .filter(|&&n| n == NOTIFICATION_PHYSICS_PROCESS)
            .count();
        assert_eq!(physics_count, 4, "expected physics steps capped at 4");
    }

    #[test]
    fn notification_order_godot_contract() {
        let (mut ml, child_id) = make_loop_with_child();
        ml.step(1.0 / 60.0);

        let log = ml.tree().get_node(child_id).unwrap().notification_log();
        // POSTINITIALIZE + PARENTED + ENTER_TREE + READY from add_child to live root,
        // then per frame: INTERNAL_PHYSICS -> PHYSICS -> INTERNAL_PROCESS -> PROCESS
        assert_eq!(log.len(), 9);
        assert_eq!(log[0], NOTIFICATION_POSTINITIALIZE);
        assert_eq!(log[1], NOTIFICATION_PARENTED);
        assert_eq!(log[2], NOTIFICATION_ENTER_TREE);
        assert_eq!(log[3], NOTIFICATION_POST_ENTER_TREE);
        assert_eq!(log[4], NOTIFICATION_READY);
        assert_eq!(log[5], NOTIFICATION_INTERNAL_PHYSICS_PROCESS);
        assert_eq!(log[6], NOTIFICATION_PHYSICS_PROCESS);
        assert_eq!(log[7], NOTIFICATION_INTERNAL_PROCESS);
        assert_eq!(log[8], NOTIFICATION_PROCESS);
    }

    #[test]
    fn step_with_delta_zero() {
        let (mut ml, child_id) = make_loop_with_child();
        ml.step(0.0);

        assert_eq!(ml.frame_count(), 1);
        assert_eq!(ml.process_time(), 0.0);

        // With delta=0, no physics ticks should fire, but process phase fires
        let log = ml.tree().get_node(child_id).unwrap().notification_log();
        let physics_count = log
            .iter()
            .filter(|&&n| n == NOTIFICATION_PHYSICS_PROCESS)
            .count();
        let internal_physics_count = log
            .iter()
            .filter(|&&n| n == NOTIFICATION_INTERNAL_PHYSICS_PROCESS)
            .count();
        let process_count = log.iter().filter(|&&n| n == NOTIFICATION_PROCESS).count();
        let internal_process_count = log
            .iter()
            .filter(|&&n| n == NOTIFICATION_INTERNAL_PROCESS)
            .count();
        assert_eq!(physics_count, 0);
        assert_eq!(internal_physics_count, 0);
        assert_eq!(process_count, 1);
        assert_eq!(internal_process_count, 1);
    }

    #[test]
    fn step_with_very_small_delta() {
        let (mut ml, child_id) = make_loop_with_child();
        // Much smaller than physics timestep (1/60)
        ml.step(1e-10);

        assert_eq!(ml.frame_count(), 1);

        let log = ml.tree().get_node(child_id).unwrap().notification_log();
        let physics_count = log
            .iter()
            .filter(|&&n| n == NOTIFICATION_PHYSICS_PROCESS)
            .count();
        let process_count = log.iter().filter(|&&n| n == NOTIFICATION_PROCESS).count();
        // Delta too small for any physics step
        assert_eq!(physics_count, 0);
        assert_eq!(process_count, 1);
    }

    #[test]
    fn run_frames_zero() {
        let (mut ml, child_id) = make_loop_with_child();
        ml.run_frames(0, 1.0 / 60.0);

        assert_eq!(ml.frame_count(), 0);
        let log = ml.tree().get_node(child_id).unwrap().notification_log();
        // POSTINITIALIZE + PARENTED + ENTER_TREE + READY from add_child, no frame notifications.
        assert_eq!(
            log,
            &[
                NOTIFICATION_POSTINITIALIZE,
                NOTIFICATION_PARENTED,
                NOTIFICATION_ENTER_TREE,
                NOTIFICATION_POST_ENTER_TREE,
                NOTIFICATION_READY,
            ]
        );
    }

    #[test]
    fn physics_ticks_per_second_accessor() {
        let (ml, _) = make_loop_with_child();
        assert_eq!(ml.physics_ticks_per_second(), 60);
    }

    #[test]
    fn max_physics_steps_per_frame_accessor() {
        let (ml, _) = make_loop_with_child();
        assert_eq!(ml.max_physics_steps_per_frame(), 8);
    }

    #[test]
    fn paused_accessor_defaults_false() {
        let (ml, _) = make_loop_with_child();
        assert!(!ml.paused());
    }

    #[test]
    fn paused_step_skips_process_and_physics_notifications() {
        let (mut ml, child_id) = make_loop_with_child();
        ml.set_paused(true);
        ml.step(1.0 / 60.0);

        let log = ml.tree().get_node(child_id).unwrap().notification_log();
        assert!(log.contains(&NOTIFICATION_PAUSED));
        assert!(!log.contains(&NOTIFICATION_PROCESS));
        assert!(!log.contains(&NOTIFICATION_PHYSICS_PROCESS));
        assert_eq!(ml.frame_count(), 1);
        // Physics still runs when paused (Always-mode nodes may need it);
        // default Inherit (= Pausable) nodes are filtered at the per-node level.
        assert!(ml.physics_time() > 0.0);
    }

    #[test]
    fn unpausing_restores_frame_processing() {
        let (mut ml, child_id) = make_loop_with_child();
        ml.set_paused(true);
        ml.step(1.0 / 60.0);
        ml.set_paused(false);
        ml.step(1.0 / 60.0);

        let log = ml.tree().get_node(child_id).unwrap().notification_log();
        assert!(log.contains(&NOTIFICATION_PAUSED));
        assert!(log.contains(&NOTIFICATION_UNPAUSED));
        assert!(log.contains(&NOTIFICATION_PROCESS));
        assert!(log.contains(&NOTIFICATION_PHYSICS_PROCESS));
    }

    #[test]
    fn set_physics_ticks_per_second() {
        let (mut ml, _) = make_loop_with_child();
        ml.set_physics_ticks_per_second(120);
        assert_eq!(ml.physics_ticks_per_second(), 120);
    }

    #[test]
    fn configure_from_project_settings_honors_tick_rate_and_gravity() {
        use gdcore::{ProjectSettings, SettingsValue};

        let mut settings = ProjectSettings::new();
        settings.set(
            "physics/common/physics_ticks_per_second",
            SettingsValue::Int(30),
        );
        settings.set("physics/2d/default_gravity", SettingsValue::Float(490.0));
        settings.set("physics/3d/default_gravity", SettingsValue::Float(4.9));

        let (mut ml, _) = make_loop_with_child();
        ml.configure_from_project_settings(&settings);

        assert_eq!(ml.physics_ticks_per_second(), 30);
        // 2D default direction is (0, 1) (downward); magnitude honored.
        let g2 = ml.physics_server().gravity();
        assert_eq!(g2.x, 0.0);
        assert!((g2.y - 490.0).abs() < 1e-4, "2D gravity y = {}", g2.y);
        // 3D default direction is (0, -1, 0).
        let g3 = ml.physics_server_3d().gravity();
        assert_eq!(g3.x, 0.0);
        assert!((g3.y + 4.9).abs() < 1e-4, "3D gravity y = {}", g3.y);
        assert_eq!(g3.z, 0.0);
        // The 3D world (not merely the server's cached field) must honor it.
        assert!((ml.physics_server_3d().world().gravity.y + 4.9).abs() < 1e-4);
    }

    #[test]
    fn configure_from_project_settings_honors_explicit_gravity_vector() {
        use gdcore::{ProjectSettings, SettingsValue};

        let mut settings = ProjectSettings::new();
        // Sideways 2D gravity: magnitude 100 along direction (1, 0).
        settings.set("physics/2d/default_gravity", SettingsValue::Float(100.0));
        settings.set(
            "physics/2d/default_gravity_vector",
            SettingsValue::Vector2(1.0, 0.0),
        );

        let (mut ml, _) = make_loop_with_child();
        ml.configure_from_project_settings(&settings);

        let g2 = ml.physics_server().gravity();
        assert!((g2.x - 100.0).abs() < 1e-4, "2D gravity x = {}", g2.x);
        assert_eq!(g2.y, 0.0);
    }

    #[test]
    fn configure_from_project_settings_ignores_nonpositive_tick_rate() {
        use gdcore::{ProjectSettings, SettingsValue};

        let mut settings = ProjectSettings::new();
        settings.set(
            "physics/common/physics_ticks_per_second",
            SettingsValue::Int(0),
        );

        let (mut ml, _) = make_loop_with_child();
        ml.configure_from_project_settings(&settings);
        // Non-positive tick rate is rejected; the default 60 is preserved.
        assert_eq!(ml.physics_ticks_per_second(), 60);
    }

    #[test]
    #[should_panic(expected = "physics_ticks_per_second must be > 0")]
    fn set_physics_ticks_per_second_zero_panics() {
        let (mut ml, _) = make_loop_with_child();
        ml.set_physics_ticks_per_second(0);
    }

    #[test]
    #[should_panic(expected = "max_physics_steps_per_frame must be > 0")]
    fn set_max_physics_steps_zero_panics() {
        let (mut ml, _) = make_loop_with_child();
        ml.set_max_physics_steps_per_frame(0);
    }

    #[test]
    fn tree_mut_access() {
        let (mut ml, _) = make_loop_with_child();
        let child = Node::new("Extra", "Node");
        let root = ml.tree().root_id();
        ml.tree_mut().add_child(root, child).unwrap();
        assert_eq!(ml.tree().node_count(), 3); // root + Child + Extra
    }

    #[test]
    fn multiple_physics_steps_in_one_frame() {
        let (mut ml, child_id) = make_loop_with_child();
        // With delta = 2/60, at 60 TPS we should get 2 physics steps
        ml.step(2.0 / 60.0);

        let log = ml.tree().get_node(child_id).unwrap().notification_log();
        let physics_count = log
            .iter()
            .filter(|&&n| n == NOTIFICATION_PHYSICS_PROCESS)
            .count();
        assert_eq!(physics_count, 2);
    }

    #[test]
    fn accumulator_carries_over_between_frames() {
        let (mut ml, child_id) = make_loop_with_child();
        // At 60 TPS, physics dt = 1/60 ~= 0.01667
        // Step with half the physics dt twice — accumulator should carry over
        let half_dt = 0.5 / 60.0;
        ml.step(half_dt);
        ml.step(half_dt);

        let log = ml.tree().get_node(child_id).unwrap().notification_log();
        let physics_count = log
            .iter()
            .filter(|&&n| n == NOTIFICATION_PHYSICS_PROCESS)
            .count();
        // The first step accumulates half, second step accumulates to full => 1 physics step
        assert_eq!(physics_count, 1);
        assert_eq!(ml.frame_count(), 2);
    }

    // ── Bead B007: Godot frame-contract tests ───────────────────────────

    /// Helper: builds a tree with root -> Parent -> Child hierarchy.
    fn make_loop_with_parent_child() -> (MainLoop, crate::node::NodeId, crate::node::NodeId) {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let parent = Node::new("Parent", "Node");
        let parent_id = tree.add_child(root, parent).unwrap();
        let child = Node::new("Child", "Node");
        let child_id = tree.add_child(parent_id, child).unwrap();
        (MainLoop::new(tree), parent_id, child_id)
    }

    #[test]
    fn godot_four_notification_order_per_frame() {
        // Godot contract: each frame with one physics tick fires exactly
        // INTERNAL_PHYSICS -> PHYSICS -> INTERNAL_PROCESS -> PROCESS
        let (mut ml, child_id) = make_loop_with_child();
        ml.step(1.0 / 60.0);

        let log = ml.tree().get_node(child_id).unwrap().notification_log();
        assert_eq!(
            log,
            &[
                NOTIFICATION_POSTINITIALIZE,
                NOTIFICATION_PARENTED,
                NOTIFICATION_ENTER_TREE,
                NOTIFICATION_POST_ENTER_TREE,
                NOTIFICATION_READY,
                NOTIFICATION_INTERNAL_PHYSICS_PROCESS,
                NOTIFICATION_PHYSICS_PROCESS,
                NOTIFICATION_INTERNAL_PROCESS,
                NOTIFICATION_PROCESS,
            ]
        );
    }

    #[test]
    fn process_fires_after_physics_process_every_frame() {
        let (mut ml, child_id) = make_loop_with_child();
        ml.run_frames(5, 1.0 / 60.0);

        let log = ml.tree().get_node(child_id).unwrap().notification_log();
        // First 4 entries from add_child: POSTINITIALIZE + PARENTED + ENTER_TREE + READY.
        // Then per-frame notifications repeat every 4 notifications.
        assert_eq!(log[0], NOTIFICATION_POSTINITIALIZE);
        assert_eq!(log[1], NOTIFICATION_PARENTED);
        assert_eq!(log[2], NOTIFICATION_ENTER_TREE);
        assert_eq!(log[3], NOTIFICATION_POST_ENTER_TREE);
        assert_eq!(log[4], NOTIFICATION_READY);
        for frame in 0..5 {
            // +5 for the leading lifecycle (POST_ENTER_TREE is Godot's too).
            let base = 5 + frame * 4;
            assert_eq!(
                log[base], NOTIFICATION_INTERNAL_PHYSICS_PROCESS,
                "frame {frame}: expected INTERNAL_PHYSICS_PROCESS at pos {base}"
            );
            assert_eq!(
                log[base + 1],
                NOTIFICATION_PHYSICS_PROCESS,
                "frame {frame}: expected PHYSICS_PROCESS at pos {}",
                base + 1
            );
            assert_eq!(
                log[base + 2],
                NOTIFICATION_INTERNAL_PROCESS,
                "frame {frame}: expected INTERNAL_PROCESS at pos {}",
                base + 2
            );
            assert_eq!(
                log[base + 3],
                NOTIFICATION_PROCESS,
                "frame {frame}: expected PROCESS at pos {}",
                base + 3
            );
        }
    }

    #[test]
    fn parent_processes_before_child_in_tree_order() {
        // Godot processes parent before child (depth-first, top-down).
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let parent = Node::new("Parent", "Node");
        let parent_id = tree.add_child(root, parent).unwrap();
        let child = Node::new("Child", "Node");
        let child_id = tree.add_child(parent_id, child).unwrap();

        let mut ml = MainLoop::new(tree);
        ml.step(1.0 / 60.0);

        // Both nodes should receive the same sequence. Since tree order is
        // root -> Parent -> Child, Parent receives each notification before Child.
        let parent_log = ml.tree().get_node(parent_id).unwrap().notification_log();
        let child_log = ml.tree().get_node(child_id).unwrap().notification_log();

        // Parent: POSTINITIALIZE + PARENTED + ENTER_TREE + READY + CHILD_ORDER_CHANGED + 4 frame = 9
        // Child: POSTINITIALIZE + PARENTED + ENTER_TREE + READY + 4 frame = 8
        // +1 each for NOTIFICATION_POST_ENTER_TREE (Godot `_propagate_ready`).
        assert_eq!(parent_log.len(), 10);
        assert_eq!(child_log.len(), 9);

        // Verify tree order via all_nodes_in_tree_order.
        let order = ml.tree().all_nodes_in_tree_order();
        let parent_pos = order.iter().position(|&id| id == parent_id).unwrap();
        let child_pos = order.iter().position(|&id| id == child_id).unwrap();
        assert!(
            parent_pos < child_pos,
            "parent (pos {parent_pos}) must be processed before child (pos {child_pos})"
        );
    }

    #[test]
    fn physics_delta_is_fixed_timestep() {
        // Physics delta should always be 1/physics_ticks_per_second,
        // regardless of the frame delta passed to step().
        let (mut ml, _) = make_loop_with_child();
        let physics_dt = 1.0 / ml.physics_ticks_per_second() as f64;

        // Run with non-standard delta (e.g. 2/60 => 2 physics ticks per frame).
        ml.step(2.0 / 60.0);

        // Each physics tick should advance physics_time by exactly physics_dt.
        assert!(
            (ml.physics_time() - 2.0 * physics_dt).abs() < 1e-12,
            "physics_time {} != expected {}",
            ml.physics_time(),
            2.0 * physics_dt
        );
    }

    #[test]
    fn process_delta_is_variable_timestep() {
        // Process time should exactly track the delta passed to step().
        let (mut ml, _) = make_loop_with_child();
        let variable_delta = 0.05; // 50ms — not aligned to physics tick
        ml.step(variable_delta);

        assert!(
            (ml.process_time() - variable_delta).abs() < 1e-12,
            "process_time {} != expected {}",
            ml.process_time(),
            variable_delta
        );
    }

    #[test]
    fn ten_frame_property_evolution_trace() {
        // Run 10 frames and verify per-frame notification accumulation is correct
        // at each step — not just at the end.
        let (mut ml, child_id) = make_loop_with_child();
        let delta = 1.0 / 60.0;

        for frame in 1..=10u64 {
            ml.step(delta);

            assert_eq!(ml.frame_count(), frame);

            let log = ml.tree().get_node(child_id).unwrap().notification_log();
            // 4 lifecycle from add_child (POSTINITIALIZE + PARENTED + ENTER_TREE + READY)
            // + each frame adds 4 notifications.
            // 5 lifecycle: POST_ENTER_TREE follows ENTER_TREE, as in Godot.
            let expected_len = 5 + (frame as usize) * 4;
            assert_eq!(
                log.len(),
                expected_len,
                "frame {frame}: expected {expected_len} notifications, got {}",
                log.len()
            );

            // Physics process count = frame number
            let physics_count = log
                .iter()
                .filter(|&&n| n == NOTIFICATION_PHYSICS_PROCESS)
                .count();
            assert_eq!(
                physics_count, frame as usize,
                "frame {frame}: physics_count"
            );

            // Process count = frame number
            let process_count = log.iter().filter(|&&n| n == NOTIFICATION_PROCESS).count();
            assert_eq!(
                process_count, frame as usize,
                "frame {frame}: process_count"
            );

            // Cumulative time checks.
            let expected_process_time = frame as f64 * delta;
            assert!(
                (ml.process_time() - expected_process_time).abs() < 1e-10,
                "frame {frame}: process_time {} != {expected_process_time}",
                ml.process_time()
            );
        }
    }

    #[test]
    fn physics_accumulator_non_integer_frame_ratio() {
        // Run at 30 FPS with 60 TPS physics — each frame's delta (1/30)
        // holds exactly 2 physics ticks (1/30 / 1/60 = 2). No accumulator
        // remainder.
        let (mut ml, child_id) = make_loop_with_child();
        ml.run_frames(5, 1.0 / 30.0);

        let log = ml.tree().get_node(child_id).unwrap().notification_log();
        let physics_count = log
            .iter()
            .filter(|&&n| n == NOTIFICATION_PHYSICS_PROCESS)
            .count();
        assert_eq!(physics_count, 10, "5 frames * 2 physics ticks = 10");
        assert_eq!(ml.frame_count(), 5);
    }

    #[test]
    fn physics_accumulator_fractional_ratio() {
        // 50 FPS rendering with 60 TPS physics: delta=1/50=0.02, physics_dt=1/60≈0.01667
        // Frame 1: acc=0.02, 1 tick (acc=0.02-0.01667=0.00333)
        // Frame 2: acc=0.02333, 1 tick (acc=0.00667)
        // Frame 3: acc=0.02667, 1 tick (acc=0.01)
        // Frame 4: acc=0.03, 1 tick (acc=0.01333)
        // Frame 5: acc=0.03333, 2 ticks (acc=0.03333-0.03333=0.0)
        // Total: 6 physics ticks in 5 frames
        let (mut ml, child_id) = make_loop_with_child();
        ml.run_frames(5, 1.0 / 50.0);

        let log = ml.tree().get_node(child_id).unwrap().notification_log();
        let physics_count = log
            .iter()
            .filter(|&&n| n == NOTIFICATION_PHYSICS_PROCESS)
            .count();
        assert_eq!(
            physics_count, 6,
            "5 frames at 50fps with 60tps should give 6 physics ticks"
        );
    }

    #[test]
    fn frame_count_matches_run_frames() {
        let (mut ml, _) = make_loop_with_child();
        ml.run_frames(42, 1.0 / 60.0);
        assert_eq!(ml.frame_count(), 42);
    }

    #[test]
    fn frame_count_matches_manual_steps() {
        let (mut ml, _) = make_loop_with_child();
        for _ in 0..17 {
            ml.step(1.0 / 60.0);
        }
        assert_eq!(ml.frame_count(), 17);
    }

    #[test]
    fn internal_notifications_also_in_tree_order() {
        // Internal notifications must also respect parent-before-child order.
        let (mut ml, parent_id, child_id) = make_loop_with_parent_child();
        ml.step(1.0 / 60.0);

        // Verify both parent and child get internal notifications.
        let parent_log = ml.tree().get_node(parent_id).unwrap().notification_log();
        let child_log = ml.tree().get_node(child_id).unwrap().notification_log();

        assert!(parent_log.contains(&NOTIFICATION_INTERNAL_PHYSICS_PROCESS));
        assert!(parent_log.contains(&NOTIFICATION_INTERNAL_PROCESS));
        assert!(child_log.contains(&NOTIFICATION_INTERNAL_PHYSICS_PROCESS));
        assert!(child_log.contains(&NOTIFICATION_INTERNAL_PROCESS));
    }

    #[test]
    fn high_tps_caps_at_max_steps() {
        // With 1000 TPS, delta=1/60 would need ~16 physics ticks, but
        // max_physics_steps_per_frame (default 8) caps it.
        let (mut ml, child_id) = make_loop_with_child();
        ml.set_physics_ticks_per_second(1000);
        ml.step(1.0 / 60.0);

        let log = ml.tree().get_node(child_id).unwrap().notification_log();
        let physics_count = log
            .iter()
            .filter(|&&n| n == NOTIFICATION_PHYSICS_PROCESS)
            .count();
        assert_eq!(physics_count, 8, "capped at max_physics_steps_per_frame");
    }

    #[test]
    fn high_tps_uncapped_when_limit_raised() {
        // With limit raised to 20, 1000 TPS at 1/60 delta gives 16 ticks.
        let (mut ml, child_id) = make_loop_with_child();
        ml.set_physics_ticks_per_second(1000);
        ml.set_max_physics_steps_per_frame(20);
        ml.step(1.0 / 60.0);

        let log = ml.tree().get_node(child_id).unwrap().notification_log();
        let physics_count = log
            .iter()
            .filter(|&&n| n == NOTIFICATION_PHYSICS_PROCESS)
            .count();
        assert_eq!(physics_count, 16, "floor(1/60 / 1/1000) = 16");
    }

    #[test]
    fn internal_physics_count_matches_user_physics_count() {
        // Each physics tick should dispatch both internal and user physics.
        let (mut ml, child_id) = make_loop_with_child();
        ml.run_frames(7, 1.0 / 60.0);

        let log = ml.tree().get_node(child_id).unwrap().notification_log();
        let internal_phys = log
            .iter()
            .filter(|&&n| n == NOTIFICATION_INTERNAL_PHYSICS_PROCESS)
            .count();
        let user_phys = log
            .iter()
            .filter(|&&n| n == NOTIFICATION_PHYSICS_PROCESS)
            .count();
        assert_eq!(
            internal_phys, user_phys,
            "internal and user physics counts must match"
        );
    }

    #[test]
    fn internal_process_count_matches_user_process_count() {
        // Each frame dispatches both internal and user process exactly once.
        let (mut ml, child_id) = make_loop_with_child();
        ml.run_frames(7, 1.0 / 60.0);

        let log = ml.tree().get_node(child_id).unwrap().notification_log();
        let internal_proc = log
            .iter()
            .filter(|&&n| n == NOTIFICATION_INTERNAL_PROCESS)
            .count();
        let user_proc = log.iter().filter(|&&n| n == NOTIFICATION_PROCESS).count();
        assert_eq!(internal_proc, 7);
        assert_eq!(user_proc, 7);
        assert_eq!(internal_proc, user_proc);
    }

    #[test]
    fn process_count_equals_frame_count() {
        let (mut ml, child_id) = make_loop_with_child();
        ml.run_frames(20, 1.0 / 60.0);

        let log = ml.tree().get_node(child_id).unwrap().notification_log();
        let process_count = log.iter().filter(|&&n| n == NOTIFICATION_PROCESS).count();
        assert_eq!(process_count as u64, ml.frame_count());
    }

    #[test]
    fn deep_tree_processes_in_correct_order() {
        // Build root -> A -> B -> C and verify tree-order processing.
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let a = Node::new("A", "Node");
        let a_id = tree.add_child(root, a).unwrap();
        let b = Node::new("B", "Node");
        let b_id = tree.add_child(a_id, b).unwrap();
        let c = Node::new("C", "Node");
        let c_id = tree.add_child(b_id, c).unwrap();

        let mut ml = MainLoop::new(tree);
        ml.step(1.0 / 60.0);

        let order = ml.tree().all_nodes_in_tree_order();
        let a_pos = order.iter().position(|&id| id == a_id).unwrap();
        let b_pos = order.iter().position(|&id| id == b_id).unwrap();
        let c_pos = order.iter().position(|&id| id == c_id).unwrap();
        assert!(a_pos < b_pos, "A before B");
        assert!(b_pos < c_pos, "B before C");

        // Each node gets POSTINITIALIZE + PARENTED + ENTER_TREE + READY + possibly
        // CHILD_ORDER_CHANGED + 4 frame notifications.
        // A and B each have one child added, so they get CHILD_ORDER_CHANGED too.
        let a_log = ml.tree().get_node(a_id).unwrap().notification_log();
        // 5 lifecycle (incl. POST_ENTER_TREE) + CHILD_ORDER_CHANGED + 4 frame
        assert_eq!(a_log.len(), 10);
        let b_log = ml.tree().get_node(b_id).unwrap().notification_log();
        assert_eq!(b_log.len(), 10);
        let c_log = ml.tree().get_node(c_id).unwrap().notification_log();
        assert_eq!(c_log.len(), 9); // 5 lifecycle + 4 frame (no children)
    }

    #[test]
    fn sibling_order_preserved() {
        // root -> A, root -> B: A should process before B (insertion order).
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let a = Node::new("A", "Node");
        let a_id = tree.add_child(root, a).unwrap();
        let b = Node::new("B", "Node");
        let b_id = tree.add_child(root, b).unwrap();

        let ml = MainLoop::new(tree);
        let order = ml.tree().all_nodes_in_tree_order();
        let a_pos = order.iter().position(|&id| id == a_id).unwrap();
        let b_pos = order.iter().position(|&id| id == b_id).unwrap();
        assert!(a_pos < b_pos, "sibling A before sibling B");
    }

    #[test]
    fn call_deferred_queues_without_panic() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        tree.call_deferred(root, "some_method", &[]);
        assert_eq!(tree.pending_deferred_count(), 1);
        tree.process_deferred_calls();
        assert_eq!(tree.pending_deferred_count(), 0);
    }

    #[test]
    fn call_deferred_set_runs_after_step() {
        use gdvariant::Variant;
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let child = tree
            .add_child(root, crate::node::Node::new("C", "Node2D"))
            .unwrap();
        // Property starts true (Node default visible).
        tree.get_node_mut(child)
            .unwrap()
            .set_property("visible", Variant::Bool(true));

        tree.call_deferred(
            child,
            "set",
            &[Variant::String("visible".into()), Variant::Bool(false)],
        );
        // Not applied yet.
        assert_eq!(
            tree.get_node(child).unwrap().get_property("visible"),
            Variant::Bool(true)
        );

        let mut ml = MainLoop::new(tree);
        ml.step(1.0 / 60.0);
        assert_eq!(
            ml.tree().get_node(child).unwrap().get_property("visible"),
            Variant::Bool(false),
            "call_deferred set must apply after MainLoop::step"
        );
    }

    // ── B012: Engine-owned input routing tests ────────────────────────────

    #[test]
    fn set_input_makes_snapshot_available_on_tree() {
        let (mut ml, _) = make_loop_with_child();
        let mut snapshot = InputSnapshot::default();
        snapshot.pressed_keys.insert("ArrowLeft".to_string());

        ml.set_input(snapshot);
        assert!(ml.tree().has_input_snapshot());
        let snap = ml.tree().input_snapshot().unwrap();
        assert!(snap.is_key_pressed("ArrowLeft"));
    }

    #[test]
    fn step_clears_input_snapshot() {
        let (mut ml, _) = make_loop_with_child();
        let mut snapshot = InputSnapshot::default();
        snapshot.pressed_keys.insert("Space".to_string());

        ml.set_input(snapshot);
        assert!(ml.tree().has_input_snapshot());

        ml.step(1.0 / 60.0);

        // Input should be cleared after step completes.
        assert!(
            !ml.tree().has_input_snapshot(),
            "input snapshot must be cleared after step()"
        );
    }

    #[test]
    fn clear_input_removes_snapshot() {
        let (mut ml, _) = make_loop_with_child();
        let snapshot = InputSnapshot::default();
        ml.set_input(snapshot);
        assert!(ml.tree().has_input_snapshot());

        ml.clear_input();
        assert!(!ml.tree().has_input_snapshot());
    }

    #[test]
    fn input_does_not_leak_across_frames() {
        let (mut ml, _) = make_loop_with_child();

        // Frame 1: set input and step.
        let mut snap1 = InputSnapshot::default();
        snap1.pressed_keys.insert("a".to_string());
        ml.set_input(snap1);
        ml.step(1.0 / 60.0);

        // Frame 2: step without setting input — should have no snapshot.
        assert!(!ml.tree().has_input_snapshot());
        ml.step(1.0 / 60.0);
        assert!(!ml.tree().has_input_snapshot());
    }

    #[test]
    fn input_with_action_map_flows_through() {
        let (mut ml, _) = make_loop_with_child();
        let mut snapshot = InputSnapshot::default();
        snapshot.pressed_keys.insert("ArrowRight".to_string());
        snapshot
            .input_map
            .insert("move_right".to_string(), vec!["ArrowRight".to_string()]);

        ml.set_input(snapshot);
        let snap = ml.tree().input_snapshot().unwrap();
        assert!(snap.is_action_pressed("move_right"));
        assert!(!snap.is_action_pressed("move_left"));
    }

    #[test]
    fn paused_step_still_clears_input() {
        let (mut ml, _) = make_loop_with_child();
        ml.set_paused(true);

        let mut snapshot = InputSnapshot::default();
        snapshot.pressed_keys.insert("Escape".to_string());
        ml.set_input(snapshot);
        assert!(ml.tree().has_input_snapshot());

        ml.step(1.0 / 60.0);

        // Even when paused, input should be cleared to prevent stale state.
        assert!(
            !ml.tree().has_input_snapshot(),
            "paused step must still clear input"
        );
    }

    // ── B012: FrameOutput return tests ────────────────────────────────────

    #[test]
    fn step_returns_frame_output() {
        let (mut ml, _) = make_loop_with_child();
        let delta = 1.0 / 60.0;
        let output = ml.step(delta);

        assert_eq!(output.frame_count, 1);
        assert_eq!(output.delta, delta);
        assert_eq!(output.physics_steps, 1); // 1/60 delta at 60 TPS = 1 tick
    }

    #[test]
    fn frame_output_increments_across_frames() {
        let (mut ml, _) = make_loop_with_child();
        let delta = 1.0 / 60.0;

        for expected in 1..=5u64 {
            let output = ml.step(delta);
            assert_eq!(output.frame_count, expected);
            assert_eq!(output.delta, delta);
        }
    }

    #[test]
    fn frame_output_reports_multiple_physics_steps() {
        let (mut ml, _) = make_loop_with_child();
        // 2/60 delta at 60 TPS = 2 physics ticks
        let output = ml.step(2.0 / 60.0);

        assert_eq!(output.physics_steps, 2);
        assert_eq!(output.frame_count, 1);
    }

    #[test]
    fn frame_output_physics_runs_when_paused() {
        let (mut ml, _) = make_loop_with_child();
        ml.set_paused(true);

        let output = ml.step(1.0 / 60.0);
        assert_eq!(output.frame_count, 1);
        // Physics still runs when paused; per-node filtering handles pause mode.
        assert!(output.physics_steps >= 1);
    }

    #[test]
    fn frame_output_zero_physics_with_tiny_delta() {
        let (mut ml, _) = make_loop_with_child();
        let output = ml.step(1e-10);

        assert_eq!(output.frame_count, 1);
        assert_eq!(output.physics_steps, 0);
    }

    // ── pat-isw: Engine-owned InputState → script bridge tests ──────────

    #[test]
    fn push_event_bridges_to_script_snapshot() {
        let (mut ml, _) = make_loop_with_child();

        // Push a key press event into the engine-owned InputState.
        ml.push_event(InputEvent::Key {
            key: gdplatform::input::Key::Right,
            pressed: true,
            shift: false,
            ctrl: false,
            alt: false,
        });

        // step() should bridge InputState → script InputSnapshot.
        ml.step(1.0 / 60.0);

        // After step, input is cleared — but the bridge happened during step.
        // Verify by checking frame executed without panic. The real proof is
        // the next test which checks the snapshot mid-bridge.
        assert_eq!(ml.frame_count(), 1);
    }

    #[test]
    fn push_event_snapshot_visible_on_tree_during_step() {
        // Verify the bridge produces a script-facing snapshot by checking
        // that has_input_snapshot is true after push_event + before step clears it.
        let (mut ml, _) = make_loop_with_child();

        ml.push_event(InputEvent::Key {
            key: gdplatform::input::Key::Left,
            pressed: true,
            shift: false,
            ctrl: false,
            alt: false,
        });

        // Before step, no manual snapshot was set, but InputState has data.
        assert!(!ml.tree().has_input_snapshot(), "no snapshot before step");

        // step() will bridge and then clear. To verify the bridge happened,
        // we check that InputState was flushed (just_pressed cleared).
        ml.step(1.0 / 60.0);
        assert!(
            !ml.input_state()
                .is_key_just_pressed(gdplatform::input::Key::Left),
            "just_pressed should be flushed after step"
        );
        // But key should still be pressed (not released).
        assert!(
            ml.input_state()
                .is_key_pressed(gdplatform::input::Key::Left),
            "key should remain pressed until release event"
        );
    }

    #[test]
    fn push_event_does_not_override_manual_set_input() {
        let (mut ml, _) = make_loop_with_child();

        // Push an event into InputState.
        ml.push_event(InputEvent::Key {
            key: gdplatform::input::Key::Right,
            pressed: true,
            shift: false,
            ctrl: false,
            alt: false,
        });

        // Also set a manual snapshot (this takes priority).
        let mut manual = InputSnapshot::default();
        manual.pressed_keys.insert("Space".to_string());
        ml.set_input(manual);

        // step() should use the manual snapshot, not bridge from InputState.
        ml.step(1.0 / 60.0);

        // Frame ran successfully — the manual snapshot was used.
        assert_eq!(ml.frame_count(), 1);
    }

    #[test]
    fn push_event_with_action_map_bridges_actions() {
        let (mut ml, _) = make_loop_with_child();

        // Set up an input map: "ui_right" → Key::Right.
        let mut map = gdplatform::input::InputMap::new();
        map.add_action("ui_right", 0.0);
        map.action_add_event(
            "ui_right",
            gdplatform::input::ActionBinding::KeyBinding(gdplatform::input::Key::Right),
        );
        ml.set_input_map(map);

        // Push the key event — InputState should resolve the action.
        ml.push_event(InputEvent::Key {
            key: gdplatform::input::Key::Right,
            pressed: true,
            shift: false,
            ctrl: false,
            alt: false,
        });

        assert!(ml.input_state().is_action_pressed("ui_right"));

        // After step, flush_frame clears just_pressed but action remains pressed.
        ml.step(1.0 / 60.0);
        assert!(ml.input_state().is_action_pressed("ui_right"));
        assert!(!ml.input_state().is_action_just_pressed("ui_right"));
    }

    #[test]
    fn input_state_flush_clears_just_pressed_after_step() {
        let (mut ml, _) = make_loop_with_child();

        ml.push_event(InputEvent::Key {
            key: gdplatform::input::Key::Space,
            pressed: true,
            shift: false,
            ctrl: false,
            alt: false,
        });

        assert!(ml
            .input_state()
            .is_key_just_pressed(gdplatform::input::Key::Space));

        ml.step(1.0 / 60.0);

        // just_pressed should be flushed, but key stays pressed.
        assert!(!ml
            .input_state()
            .is_key_just_pressed(gdplatform::input::Key::Space));
        assert!(ml
            .input_state()
            .is_key_pressed(gdplatform::input::Key::Space));
    }

    #[test]
    fn paused_step_flushes_input_state() {
        let (mut ml, _) = make_loop_with_child();
        ml.set_paused(true);

        ml.push_event(InputEvent::Key {
            key: gdplatform::input::Key::A,
            pressed: true,
            shift: false,
            ctrl: false,
            alt: false,
        });

        assert!(ml
            .input_state()
            .is_key_just_pressed(gdplatform::input::Key::A));

        ml.step(1.0 / 60.0);

        // Even when paused, flush_frame should clear just_pressed.
        assert!(!ml
            .input_state()
            .is_key_just_pressed(gdplatform::input::Key::A));
        assert!(ml.input_state().is_key_pressed(gdplatform::input::Key::A));
    }

    // --- PlatformBackend integration ---

    #[test]
    fn run_frame_drains_backend_events_into_input() {
        let (mut ml, _) = make_loop_with_child();

        use gdplatform::input::{ActionBinding, Key};
        let mut input_map = gdplatform::input::InputMap::new();
        input_map.add_action("jump", 0.0);
        input_map.action_add_event("jump", ActionBinding::KeyBinding(Key::Space));
        ml.set_input_map(input_map);

        let mut backend = gdplatform::HeadlessPlatform::new(640, 480);
        backend.push_event(gdplatform::WindowEvent::KeyInput {
            key: Key::Space,
            pressed: true,
            shift: false,
            ctrl: false,
            alt: false,
        });

        let output = ml.run_frame(&mut backend, 1.0 / 60.0);
        assert_eq!(output.frame_count, 1);
        assert_eq!(backend.frames_run(), 1);
        // Input was flushed by step(), so just_pressed is cleared.
        assert!(!ml.input_state().is_key_just_pressed(Key::Space));
    }

    #[test]
    fn run_respects_max_frames() {
        use gdplatform::PlatformBackend;
        let (mut ml, _) = make_loop_with_child();
        let mut backend = gdplatform::HeadlessPlatform::new(640, 480).with_max_frames(10);

        ml.run(&mut backend, 1.0 / 60.0);

        assert_eq!(ml.frame_count(), 10);
        assert_eq!(backend.frames_run(), 10);
        assert!(backend.should_quit());
    }

    #[test]
    fn run_stops_on_quit_request() {
        let (mut ml, _) = make_loop_with_child();
        let mut backend = gdplatform::HeadlessPlatform::new(640, 480);
        backend.request_quit();

        ml.run(&mut backend, 1.0 / 60.0);

        assert_eq!(ml.frame_count(), 0);
    }

    #[test]
    fn run_frame_ignores_non_input_events() {
        let (mut ml, _) = make_loop_with_child();
        let mut backend = gdplatform::HeadlessPlatform::new(640, 480);
        backend.push_event(gdplatform::WindowEvent::FocusGained);
        backend.push_event(gdplatform::WindowEvent::Resized {
            width: 800,
            height: 600,
        });

        let output = ml.run_frame(&mut backend, 1.0 / 60.0);
        assert_eq!(output.frame_count, 1);
        // No input events were pushed, so no keys are pressed.
        assert!(!ml
            .input_state()
            .is_key_pressed(gdplatform::input::Key::Space));
    }

    // -------------------------------------------------------------------
    // #159 — Area2D signal dispatch through MainLoop
    // -------------------------------------------------------------------

    #[test]
    fn mainloop_emits_body_entered_signal_on_area2d() {
        use gdobject::signal::Connection;
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;

        let mut tree = SceneTree::new();
        let root = tree.root_id();

        // RigidBody2D overlapping an Area2D
        let mut body = Node::new("Body", "RigidBody2D");
        body.set_property(
            "position",
            Variant::Vector2(gdcore::math::Vector2::new(5.0, 0.0)),
        );
        let body_nid = tree.add_child(root, body).unwrap();
        let mut s = Node::new("Shape", "CollisionShape2D");
        s.set_property("radius", Variant::Float(2.0));
        tree.add_child(body_nid, s).unwrap();

        let mut area = Node::new("Zone", "Area2D");
        area.set_property(
            "position",
            Variant::Vector2(gdcore::math::Vector2::new(5.0, 0.0)),
        );
        let area_nid = tree.add_child(root, area).unwrap();
        let mut sa = Node::new("Shape", "CollisionShape2D");
        sa.set_property("radius", Variant::Float(10.0));
        tree.add_child(area_nid, sa).unwrap();

        let entered_count = Arc::new(AtomicUsize::new(0));
        let cnt = entered_count.clone();
        tree.connect_signal(
            area_nid,
            "body_entered",
            Connection::with_callback(area_nid.object_id(), "on_body_entered", move |_args| {
                cnt.fetch_add(1, Ordering::SeqCst);
                Variant::Nil
            }),
        );

        let mut ml = MainLoop::new(tree);
        ml.register_physics_bodies();
        ml.step(1.0 / 60.0);

        assert_eq!(
            entered_count.load(Ordering::SeqCst),
            1,
            "body_entered should fire once through MainLoop"
        );
    }

    // -------------------------------------------------------------------
    // #162 — CharacterBody2D move_and_slide through MainLoop
    // -------------------------------------------------------------------

    #[test]
    fn mainloop_moves_character_body_with_velocity() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let mut char_node = Node::new("Player", "CharacterBody2D");
        char_node.set_property(
            "position",
            Variant::Vector2(gdcore::math::Vector2::new(0.0, 0.0)),
        );
        char_node.set_property(
            "velocity",
            Variant::Vector2(gdcore::math::Vector2::new(100.0, 0.0)),
        );
        let char_id = tree.add_child(root, char_node).unwrap();
        let mut s = Node::new("Shape", "CollisionShape2D");
        s.set_property("radius", Variant::Float(8.0));
        tree.add_child(char_id, s).unwrap();

        let mut ml = MainLoop::new(tree);
        ml.register_physics_bodies();
        ml.step(1.0 / 60.0);

        let pos = ml
            .tree()
            .get_node(char_id)
            .map(|n| n.get_property("position"))
            .unwrap_or(Variant::Nil);
        match pos {
            Variant::Vector2(v) => assert!(
                v.x > 0.0,
                "CharacterBody2D should move right via MainLoop, got x={:.1}",
                v.x
            ),
            _ => panic!("expected Vector2 position"),
        }
    }

    #[test]
    fn mainloop_character_body_stops_at_wall() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        // velocity=600 px/s, dt=1/60 → displacement=10px per tick.
        // Wall at x=12 with half_extent=5 → left edge at x=7.
        // Character radius=4, so collision when center+4 >= 7, i.e., center >= 3.
        let mut char_node = Node::new("Player", "CharacterBody2D");
        char_node.set_property(
            "position",
            Variant::Vector2(gdcore::math::Vector2::new(0.0, 0.0)),
        );
        char_node.set_property(
            "velocity",
            Variant::Vector2(gdcore::math::Vector2::new(600.0, 0.0)),
        );
        let char_id = tree.add_child(root, char_node).unwrap();
        let mut s = Node::new("Shape", "CollisionShape2D");
        s.set_property("radius", Variant::Float(4.0));
        tree.add_child(char_id, s).unwrap();

        let mut wall_node = Node::new("Wall", "StaticBody2D");
        wall_node.set_property(
            "position",
            Variant::Vector2(gdcore::math::Vector2::new(12.0, 0.0)),
        );
        let wall_id = tree.add_child(root, wall_node).unwrap();
        let mut ws = Node::new("Shape", "CollisionShape2D");
        ws.set_property(
            "size",
            Variant::Vector2(gdcore::math::Vector2::new(10.0, 200.0)),
        );
        tree.add_child(wall_id, ws).unwrap();

        let mut ml = MainLoop::new(tree);
        ml.register_physics_bodies();
        ml.step(1.0 / 60.0);

        let pos = ml
            .tree()
            .get_node(char_id)
            .map(|n| n.get_property("position"))
            .unwrap_or(Variant::Nil);
        match pos {
            Variant::Vector2(v) => assert!(
                v.x < 12.0,
                "CharacterBody2D should stop before wall, got x={:.1}",
                v.x
            ),
            _ => panic!("expected Vector2 position"),
        }
    }

    // -------------------------------------------------------------------
    // pat-u3l8c.48 — app-level platformer regression: velocity stays in
    // pixels-per-second across many MainLoop frames, and a grounded body
    // rides a floor while preserving horizontal speed.
    //
    // The original bug wrote the per-tick displacement (velocity * dt) back
    // into `velocity`, so speed collapsed toward zero after the first frame.
    // A single-step test can't catch that — only iterating the MainLoop over
    // many physics ticks exposes the decay.
    // -------------------------------------------------------------------

    fn char_velocity(ml: &MainLoop, id: crate::node::NodeId) -> gdcore::math::Vector2 {
        match ml
            .tree()
            .get_node(id)
            .map(|n| n.get_property("velocity"))
            .unwrap_or(Variant::Nil)
        {
            Variant::Vector2(v) => v,
            _ => panic!("expected Vector2 velocity"),
        }
    }

    fn char_position(ml: &MainLoop, id: crate::node::NodeId) -> gdcore::math::Vector2 {
        match ml
            .tree()
            .get_node(id)
            .map(|n| n.get_property("position"))
            .unwrap_or(Variant::Nil)
        {
            Variant::Vector2(v) => v,
            _ => panic!("expected Vector2 position"),
        }
    }

    #[test]
    fn mainloop_character_velocity_stays_px_per_second_over_many_frames() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let mut char_node = Node::new("Player", "CharacterBody2D");
        char_node.set_property(
            "position",
            Variant::Vector2(gdcore::math::Vector2::new(0.0, 0.0)),
        );
        char_node.set_property(
            "velocity",
            Variant::Vector2(gdcore::math::Vector2::new(120.0, 0.0)),
        );
        let char_id = tree.add_child(root, char_node).unwrap();
        let mut s = Node::new("Shape", "CollisionShape2D");
        s.set_property("radius", Variant::Float(8.0));
        tree.add_child(char_id, s).unwrap();

        let mut ml = MainLoop::new(tree);
        ml.register_physics_bodies();

        // 30 physics ticks at 60 Hz = 0.5 s of travel at 120 px/s = ~60 px.
        for _ in 0..30 {
            ml.step(1.0 / 60.0);
        }

        let vel = char_velocity(&ml, char_id);
        let pos = char_position(&ml, char_id);

        // The core regression: velocity must remain in px/s (≈120), NOT decay
        // toward the per-tick displacement (~2 px) as the original bug did.
        assert!(
            (vel.x - 120.0).abs() < 1.0,
            "velocity must stay px/s across frames, got {:.3} (bug would collapse to ~2)",
            vel.x
        );
        // And the travelled distance must reflect an undiminished speed.
        assert!(
            pos.x > 50.0,
            "should travel ~60px in 30 frames at 120 px/s, got x={:.2} (bug asymptotes near ~2)",
            pos.x
        );
    }

    #[test]
    fn mainloop_platformer_rides_floor_preserving_horizontal_speed() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        // Wide static floor: center y=20, size (2000, 20) => top edge at y=10.
        let mut ground = Node::new("Ground", "StaticBody2D");
        ground.set_property(
            "position",
            Variant::Vector2(gdcore::math::Vector2::new(0.0, 20.0)),
        );
        let ground_id = tree.add_child(root, ground).unwrap();
        let mut gs = Node::new("Shape", "CollisionShape2D");
        gs.set_property(
            "size",
            Variant::Vector2(gdcore::math::Vector2::new(2000.0, 20.0)),
        );
        tree.add_child(ground_id, gs).unwrap();

        // Player circle radius 8. Start at y=2.5 so it overlaps the floor top
        // (bottom edge at y=10.5) and is grounded on frame 1. Move right (120)
        // with a small downward bias (40) to stay pressed onto the floor.
        let mut char_node = Node::new("Player", "CharacterBody2D");
        char_node.set_property(
            "position",
            Variant::Vector2(gdcore::math::Vector2::new(0.0, 2.5)),
        );
        char_node.set_property(
            "velocity",
            Variant::Vector2(gdcore::math::Vector2::new(120.0, 40.0)),
        );
        char_node.set_property("floor_snap_length", Variant::Float(4.0));
        let char_id = tree.add_child(root, char_node).unwrap();
        let mut s = Node::new("Shape", "CollisionShape2D");
        s.set_property("radius", Variant::Float(8.0));
        tree.add_child(char_id, s).unwrap();

        let mut ml = MainLoop::new(tree);
        ml.register_physics_bodies();

        for _ in 0..30 {
            ml.step(1.0 / 60.0);
        }

        let vel = char_velocity(&ml, char_id);
        let pos = char_position(&ml, char_id);
        let on_floor = match ml
            .tree()
            .get_node(char_id)
            .map(|n| n.get_property("is_on_floor"))
            .unwrap_or(Variant::Nil)
        {
            Variant::Bool(b) => b,
            _ => false,
        };

        // Grounded: floor snap keeps the body glued to the floor top (center
        // y≈2) rather than sinking or drifting off.
        assert!(
            on_floor,
            "body should ride the floor, got is_on_floor=false"
        );
        assert!(
            (pos.y - 2.0).abs() < 1.0,
            "body should rest on floor top (center y≈2), got y={:.2}",
            pos.y
        );
        // Horizontal speed is preserved while grounded (the floor slide zeroes
        // only the downward component).
        assert!(
            vel.x > 100.0,
            "horizontal speed should be preserved on the floor, got vx={:.2}",
            vel.x
        );
        assert!(
            pos.x > 40.0,
            "grounded body should travel horizontally, got x={:.2}",
            pos.x
        );
    }
}

/// Godot's `Basis::looking_at(target, up)` — a frame whose **-Z** column is
/// `target` (`core/math/basis.cpp:1033-1053`).
///
/// ⚠️ Godot negates the target: `v_z = -target`, so the FORWARD axis is -Z, not
/// +Z. That single negation is the whole "which way does a PathFollow3D face"
/// question, and getting it backwards leaves every follower travelling
/// rear-first along its own path.
///
/// Falls back to a perpendicular when `up` is colinear with the target, which is
/// Godot's `get_any_perpendicular` branch — without it the cross product is zero
/// and the frame collapses to NaN.
///
/// `pub(crate)` so `curve3d_query::sample_baked_with_rotation` builds the SAME
/// frame this driver does. Two independent "point -Z along the tangent"
/// helpers would be free to disagree about the negation above, and a curve
/// method that disagreed with the follower driving it is worse than not having
/// the method.
pub(crate) fn looking_at_basis(
    target: gdcore::math::Vector3,
    up: gdcore::math::Vector3,
) -> gdcore::math3d::Basis {
    let v_z = -target.normalized();
    let mut v_x = up.cross(v_z);
    if v_x.length() < 1e-6 {
        let alt = if v_z.x.abs() < 0.9 {
            gdcore::math::Vector3::new(1.0, 0.0, 0.0)
        } else {
            gdcore::math::Vector3::new(0.0, 1.0, 0.0)
        };
        v_x = alt.cross(v_z);
    }
    let v_x = v_x.normalized();
    let v_y = v_z.cross(v_x);
    gdcore::math3d::Basis {
        x: v_x,
        y: v_y,
        z: v_z,
    }
}
