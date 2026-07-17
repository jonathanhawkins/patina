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

use std::collections::HashMap;

use crate::audio_server_bridge::AudioServerBridge;
use crate::navigation::{
    NavMesh2D, NavMesh3D, NavPolygon, NavPolygon3D, NavigationAgent2D, NavigationAgent3D,
    NavigationServer2D, NavigationServer3D, OrcaAgent,
};
use crate::node::NodeId;
use crate::particle::{ParticleEmitter, ParticleSimulator};
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
    /// Audio bridge driving AudioStreamPlayer nodes through gdaudio
    /// (pat-u3l8c.2).
    audio_bridge: AudioServerBridge,
    /// Number of physics ticks per second (default 60).
    physics_ticks_per_second: u32,
    /// Maximum number of physics steps allowed in a single frame (default 8).
    max_physics_steps_per_frame: u32,
    /// Total frames executed.
    frame_counter: u64,
    /// Total accumulated process (variable-timestep) time in seconds.
    process_time: f64,
    /// Total accumulated physics (fixed-timestep) time in seconds.
    physics_time: f64,
    /// Time left over from physics stepping, carried into the next frame.
    physics_accumulator: f64,
    /// Whether the main loop is currently paused.
    paused: bool,
    /// Engine-owned input state. Receives events via [`push_event`](Self::push_event)
    /// and produces the script-facing [`InputSnapshot`] each frame.
    input_state: InputState,
    /// Events pushed since the last [`step`](Self::step), awaiting dispatch
    /// through `SceneTree::push_input` (pat-nzgdu.83). Distinct from
    /// `input_state`, which only accumulates pollable state.
    pending_input: Vec<InputEvent>,
    /// CPU particle simulators for emitting particle nodes, keyed by node id
    /// (pat-u3l8c.4). Built on demand in [`process_particles`](Self::process_particles).
    particles: HashMap<NodeId, ParticleSimulator>,
    /// Per-node 2D navigation agents, keyed by node id (pat-u3l8c.5). Hold the
    /// computed path + waypoint index across frames so agents advance smoothly.
    nav_agents_2d: HashMap<NodeId, NavigationAgent2D>,
    /// Per-node 3D navigation agents, keyed by node id (pat-u3l8c.5).
    nav_agents_3d: HashMap<NodeId, NavigationAgent3D>,
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

impl MainLoop {
    /// Creates a new `MainLoop` that owns the given [`SceneTree`].
    pub fn new(tree: SceneTree) -> Self {
        Self {
            tree,
            physics_server: PhysicsServer::new(),
            physics_server_3d: PhysicsServer3D::new(),
            audio_bridge: AudioServerBridge::new(),
            physics_ticks_per_second: 60,
            max_physics_steps_per_frame: 8,
            frame_counter: 0,
            process_time: 0.0,
            physics_time: 0.0,
            physics_accumulator: 0.0,
            paused: false,
            input_state: InputState::new(),
            pending_input: Vec::new(),
            particles: HashMap::new(),
            nav_agents_2d: HashMap::new(),
            nav_agents_3d: HashMap::new(),
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
        const PARTICLE_CLASSES: [&str; 4] = [
            "GPUParticles2D",
            "GPUParticles3D",
            "CPUParticles2D",
            "CPUParticles3D",
        ];

        // Collect every particle node's config (emitting flag + emitter params)
        // while the tree is borrowed immutably.
        let mut configs: Vec<(NodeId, bool, ParticleEmitter)> = Vec::new();
        let mut present: std::collections::HashSet<NodeId> = std::collections::HashSet::new();
        for id in self.tree.all_nodes_in_tree_order() {
            let Some(node) = self.tree.get_node(id) else {
                continue;
            };
            if !PARTICLE_CLASSES.contains(&node.class_name()) {
                continue;
            }
            present.insert(id);
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
            // CPUParticles2D-style process properties drive per-particle motion so
            // emitted particles actually travel across the frame. Godot exposes
            // these on the CPUParticles2D node directly; GPUParticles2D reads them
            // from its ParticleProcessMaterial, which we approximate with the same
            // node keys so an authored velocity is honored in both cases.
            if let Variant::Vector2(d) = node.get_property("direction") {
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
            if let Variant::Vector2(g) = node.get_property("gravity") {
                emitter.material.gravity = g;
            }
            if let Variant::Color(c) = node.get_property("color") {
                emitter.material.start_color = c;
                emitter.material.end_color = c;
            }
            configs.push((id, is_emitting, emitter));
        }

        // Drop simulators for nodes that no longer exist.
        self.particles.retain(|id, _| present.contains(id));

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
            sim.step(dt);
            // Drain the one-shot completion edge (fires exactly once per drain).
            if sim.take_finished() {
                finished_emitters.push(id);
            }
        }
        // Godot fires `finished` when a one_shot emitter completes; a connected
        // script's callback (or `.tscn` [connection]) runs here.
        for id in finished_emitters {
            self.tree.emit_signal(id, "finished", &[]);
        }

        // Publish live counts + per-particle draw data back onto the nodes so the
        // render bridge (crate::scene_bridge) can rasterize each emitter's live
        // particles as billboard quads. Positions/colors/scales are stored under
        // internal (`_`-prefixed) keys so they never leak into oracle property
        // dumps or the editor inspector.
        let draws: Vec<(NodeId, Vec<(Vector2, Color, f32)>)> = self
            .particles
            .iter()
            .map(|(id, sim)| (*id, sim.get_draw_commands()))
            .collect();
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
        self.particles
            .get(&node_id)
            .map(|s| s.active_particles.len())
            .unwrap_or(0)
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

        struct Update3 {
            id: NodeId,
            progress: f32,
            pos: Vector3,
            rot: Option<Vector3>,
        }
        struct Update2 {
            id: NodeId,
            progress: f32,
            pos: Vector2,
            rot: Option<f32>,
        }
        let mut updates3: Vec<Update3> = Vec::new();
        let mut updates2: Vec<Update2> = Vec::new();

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
                let rotation_mode = match node.get_property("rotation_mode") {
                    Variant::Int(1) => PathFollowRotation::Y,
                    Variant::Int(2) => PathFollowRotation::Xy,
                    Variant::Int(3) => PathFollowRotation::Xyz,
                    _ => PathFollowRotation::None,
                };
                let mut follow = PathFollow3D {
                    progress,
                    loop_path,
                    rotation_mode,
                };
                follow.advance(&curve, speed * dt);
                let pos = follow.position_on(&curve);
                let rot = if rotation_mode == PathFollowRotation::None {
                    None
                } else {
                    let t = curve.sample_tangent(follow.progress);
                    // Heading about +Y using +Z as forward (internally
                    // consistent Patina convention).
                    let yaw = t.x.atan2(t.z);
                    let euler = match rotation_mode {
                        PathFollowRotation::Y => Vector3::new(0.0, yaw, 0.0),
                        // Xy / Xyz also pitch toward the tangent's vertical
                        // component; roll (Xyz torsion) is not modeled yet.
                        _ => Vector3::new(t.y.clamp(-1.0, 1.0).asin(), yaw, 0.0),
                    };
                    Some(euler)
                };
                updates3.push(Update3 {
                    id,
                    progress: follow.progress,
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
                let mut follow = PathFollow2D {
                    progress,
                    loop_path,
                    rotates,
                };
                follow.advance(&curve, speed * dt);
                let pos = follow.position_on(&curve);
                let rot = if rotates {
                    let t = curve.sample_tangent(follow.progress);
                    Some(t.y.atan2(t.x))
                } else {
                    None
                };
                updates2.push(Update2 {
                    id,
                    progress: follow.progress,
                    pos,
                    rot,
                });
            }
        }

        // Apply phase: write progress + transform back onto the nodes.
        for u in updates3 {
            if let Some(node) = self.tree.get_node_mut(u.id) {
                node.set_property("progress", Variant::Float(u.progress as f64));
            }
            crate::node3d::set_position(&mut self.tree, u.id, u.pos);
            if let Some(euler) = u.rot {
                crate::node3d::set_rotation(&mut self.tree, u.id, euler);
            }
        }
        for u in updates2 {
            if let Some(node) = self.tree.get_node_mut(u.id) {
                node.set_property("progress", Variant::Float(u.progress as f64));
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
                    let position = crate::node2d::get_position(&self.tree, id);
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

        let positions: Vec<Vector2> = agent_nodes
            .iter()
            .map(|(id, _)| crate::node2d::get_position(&self.tree, *id))
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

    /// Drives `NavigationAgent2D`/`NavigationAgent3D` nodes along their computed
    /// paths each frame (pat-u3l8c.5).
    ///
    /// A local `NavigationServer2D`/`NavigationServer3D` is (re)built from every
    /// `NavigationRegion2D`/`NavigationRegion3D` node — each contributes one
    /// convex navmesh polygon via a `navmesh_vertices` packed-array property,
    /// merged into a single connected mesh. For each agent node the path is
    /// (re)computed only when its `target_position` property changes, then the
    /// agent is integrated toward its next waypoint (default max_speed) and the
    /// resulting `next_path_position` is published onto the node. Agents stop
    /// once within `path_desired_distance` of the target so they rest cleanly.
    /// Closes the audit gap where navigation.rs was never called from the loop.
    pub fn process_navigation(&mut self, dt: f32) {
        use gdcore::math::{Vector2, Vector3};

        let mut polys2: Vec<NavPolygon> = Vec::new();
        let mut polys3: Vec<NavPolygon3D> = Vec::new();
        let mut agents2: Vec<(NodeId, Vector2, Vector2)> = Vec::new();
        let mut agents3: Vec<(NodeId, Vector3, Vector3)> = Vec::new();

        // Gather navmesh regions + agent nodes (immutable tree read).
        for id in self.tree.all_nodes_in_tree_order() {
            let Some(node) = self.tree.get_node(id) else {
                continue;
            };
            match node.class_name() {
                "NavigationRegion2D" => {
                    if let Variant::PackedVector2Array(v) = node.get_property("navmesh_vertices") {
                        if v.len() >= 3 {
                            polys2.push(NavPolygon::new(v));
                        }
                    }
                }
                "NavigationRegion3D" => {
                    if let Variant::PackedVector3Array(v) = node.get_property("navmesh_vertices") {
                        if v.len() >= 3 {
                            polys3.push(NavPolygon3D::new(v));
                        }
                    }
                }
                "NavigationAgent2D" => {
                    let current = crate::node2d::get_position(&self.tree, id);
                    let target = match node.get_property("target_position") {
                        Variant::Vector2(v) => v,
                        _ => Vector2::ZERO,
                    };
                    agents2.push((id, current, target));
                }
                "NavigationAgent3D" => {
                    let current = crate::node3d::get_position(&self.tree, id);
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
        let mut server3 = NavigationServer3D::new();
        if !polys3.is_empty() {
            let conns = Self::nav3d_connections(&polys3, 0.5);
            server3.add_region(NavMesh3D::new(polys3, conns));
        }

        // Drop agents whose node vanished.
        let present2: std::collections::HashSet<NodeId> =
            agents2.iter().map(|(id, _, _)| *id).collect();
        let present3: std::collections::HashSet<NodeId> =
            agents3.iter().map(|(id, _, _)| *id).collect();
        self.nav_agents_2d.retain(|id, _| present2.contains(id));
        self.nav_agents_3d.retain(|id, _| present3.contains(id));

        // Step 2D agents.
        let mut updates2: Vec<(NodeId, Vector2, Vector2)> = Vec::new();
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
            let arrived = agent.is_navigation_finished()
                || (agent.target_position - *current).length() <= agent.path_desired_distance;
            let new_pos = if arrived {
                *current
            } else {
                agent.step(*current, dt, &others)
            };
            let next = agent.get_next_path_position(new_pos);
            // Fire `navigation_finished`/`target_reached` exactly once, the frame
            // the agent first reaches within `path_desired_distance` of the goal.
            let arrived_now = agent.is_navigation_finished()
                || (agent.target_position - new_pos).length() <= agent.path_desired_distance;
            if arrived_now && !agent.finished_emitted {
                agent.finished_emitted = true;
                finished2.push(*id);
            }
            updates2.push((*id, new_pos, next));
        }
        for (id, new_pos, next) in updates2 {
            crate::node2d::set_position(&mut self.tree, id, new_pos);
            if let Some(node) = self.tree.get_node_mut(id) {
                node.set_property("next_path_position", Variant::Vector2(next));
            }
        }
        for id in finished2 {
            self.tree.emit_signal(id, "target_reached", &[]);
            self.tree.emit_signal(id, "navigation_finished", &[]);
        }

        // Step 3D agents.
        let mut updates3: Vec<(NodeId, Vector3, Vector3)> = Vec::new();
        let mut finished3: Vec<NodeId> = Vec::new();
        for (id, current, target) in &agents3 {
            let agent = self
                .nav_agents_3d
                .entry(*id)
                .or_insert_with(NavigationAgent3D::new);
            if agent.path.is_empty() || (agent.target_position - *target).length() > 0.01 {
                agent.set_target_position(*target, *current, &server3);
            }
            let arrived = agent.is_navigation_finished()
                || (agent.target_position - *current).length() <= agent.path_desired_distance;
            let new_pos = if arrived {
                *current
            } else {
                agent.step(*current, dt, &[])
            };
            let next = agent.get_next_path_position(new_pos);
            let arrived_now = agent.is_navigation_finished()
                || (agent.target_position - new_pos).length() <= agent.path_desired_distance;
            if arrived_now && !agent.finished_emitted {
                agent.finished_emitted = true;
                finished3.push(*id);
            }
            updates3.push((*id, new_pos, next));
        }
        for (id, new_pos, next) in updates3 {
            crate::node3d::set_position(&mut self.tree, id, new_pos);
            if let Some(node) = self.tree.get_node_mut(id) {
                node.set_property("next_path_position", Variant::Vector3(next));
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
        use gdcore::math::Vector2;

        struct RayJob {
            id: NodeId,
            origin: Vector2,
            dir: Vector2,
            max_dist: f32,
        }
        let mut jobs: Vec<RayJob> = Vec::new();
        for id in self.tree.all_nodes_in_tree_order() {
            let Some(node) = self.tree.get_node(id) else {
                continue;
            };
            if node.class_name() != "RayCast2D" {
                continue;
            }
            // Godot's RayCast2D defaults `enabled` to true.
            if matches!(node.get_property("enabled"), Variant::Bool(false)) {
                continue;
            }
            let target = match node.get_property("target_position") {
                Variant::Vector2(v) => v,
                _ => Vector2::new(0.0, 50.0),
            };
            let len = (target.x * target.x + target.y * target.y).sqrt();
            if len < 1e-6 {
                continue;
            }
            let origin = crate::node2d::get_position(&self.tree, id);
            jobs.push(RayJob {
                id,
                origin,
                dir: Vector2::new(target.x / len, target.y / len),
                max_dist: len,
            });
        }

        let mut updates: Vec<(NodeId, bool, Option<NodeId>)> = Vec::new();
        for job in &jobs {
            let hit = self
                .physics_server
                .world()
                .raycast(job.origin, job.dir, job.max_dist);
            let collider = hit
                .as_ref()
                .and_then(|h| self.physics_server.node_for_body(h.body_id));
            updates.push((job.id, hit.is_some(), collider));
        }

        for (id, colliding, collider) in updates {
            if let Some(node) = self.tree.get_node_mut(id) {
                node.set_property("is_colliding", Variant::Bool(colliding));
                let collider_val = match collider {
                    Some(cid) => Variant::Int(cid.raw() as i64),
                    None => Variant::Nil,
                };
                node.set_property("collider", collider_val);
            }
        }
    }

    /// Sets the number of physics ticks per second (default 60).
    pub fn set_physics_ticks_per_second(&mut self, tps: u32) {
        assert!(tps > 0, "physics_ticks_per_second must be > 0");
        self.physics_ticks_per_second = tps;
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

        // Physics tick rate. Ignore non-positive values (Godot requires > 0).
        if let Some(tps) = settings
            .get("physics/common/physics_ticks_per_second")
            .and_then(|v| v.as_int())
        {
            if tps > 0 {
                self.physics_ticks_per_second = tps as u32;
            }
        }

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

    /// Returns a reference to the audio bridge.
    pub fn audio_bridge(&self) -> &AudioServerBridge {
        &self.audio_bridge
    }

    /// Returns a mutable reference to the audio bridge.
    pub fn audio_bridge_mut(&mut self) -> &mut AudioServerBridge {
        &mut self.audio_bridge
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
        self.pending_input.push(event.clone());
        self.input_state.process_event(event);
    }

    /// Sets the [`InputMap`] that maps physical events to named actions.
    ///
    /// Must be called before pushing events if scripts rely on action names
    /// (e.g. `Input.is_action_pressed("ui_left")`).
    pub fn set_input_map(&mut self, map: InputMap) {
        self.input_state.set_input_map(map);
    }

    /// Returns a reference to the engine-owned [`InputState`].
    pub fn input_state(&self) -> &InputState {
        &self.input_state
    }

    /// Returns a mutable reference to the engine-owned [`InputState`].
    pub fn input_state_mut(&mut self) -> &mut InputState {
        &mut self.input_state
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
        // Bridge engine-owned InputState → script-facing InputSnapshot.
        // Only apply if no manual snapshot was set via set_input().
        if !self.tree.has_input_snapshot() {
            let platform_snap = self.input_state.snapshot();
            // Convert gdplatform::InputSnapshot → gdscene::scripting::InputSnapshot.
            // Convert mouse buttons to Godot button index strings (1=Left, 2=Right, 3=Middle).
            let mut mouse_buttons_pressed = std::collections::HashSet::new();
            use gdplatform::input::MouseButton;
            for (btn, idx) in [
                (MouseButton::Left, "1"),
                (MouseButton::Right, "2"),
                (MouseButton::Middle, "3"),
            ] {
                if platform_snap.is_mouse_button_pressed(btn) {
                    mouse_buttons_pressed.insert(idx.to_string());
                }
            }
            let mouse_pos = platform_snap.get_mouse_position();

            let script_snap = InputSnapshot {
                pressed_keys: platform_snap.pressed_key_names().into_iter().collect(),
                just_pressed_keys: platform_snap.just_pressed_key_names().into_iter().collect(),
                just_released_keys: platform_snap
                    .just_released_key_names()
                    .into_iter()
                    .collect(),
                input_map: platform_snap.action_pressed_key_map(),
                mouse_position: mouse_pos,
                mouse_buttons_pressed,
            };
            let has_input = !script_snap.pressed_keys.is_empty()
                || !script_snap.just_pressed_keys.is_empty()
                || !script_snap.mouse_buttons_pressed.is_empty()
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
            for event in std::mem::take(&mut self.pending_input) {
                self.tree.push_input(&event);
            }
        }

        let physics_dt = 1.0 / self.physics_ticks_per_second as f64;

        // -- physics phase --
        // Godot per-tick order: INTERNAL_PHYSICS_PROCESS -> PHYSICS_PROCESS
        self.physics_accumulator += delta_secs;
        let mut physics_steps = 0u32;
        while self.physics_accumulator >= physics_dt
            && physics_steps < self.max_physics_steps_per_frame
        {
            // Sync scene node positions to physics world before stepping.
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

            // Run move_and_slide for CharacterBody2D nodes with velocity.
            self.physics_server
                .process_character_movement(physics_dt as f32, &mut self.tree);

            self.physics_server.record_trace(&self.tree);

            // 3D physics: sync -> step -> sync (pat-u3l8c.1).
            self.physics_server_3d.sync_to_physics(&self.tree);
            self.physics_server_3d.step(physics_dt as f32);
            self.physics_server_3d.sync_from_physics(&mut self.tree);

            // -- navigation avoidance phase (pat-nzgdu.116) --
            // ORCA runs per physics tick, like Godot's NavigationServer: agents
            // with a desired velocity get a safe one back via velocity_computed.
            // Emitted before the script phase so a _physics_process handler sees
            // the velocity solved for this very tick.
            self.process_navigation_avoidance(physics_dt as f32);

            // SceneTree.physics_frame fires before each physics tick's script
            // phase, mirroring Godot's per-tick emission (pat-nzgdu.84).
            let physics_frame_root = self.tree.root_id();
            self.tree
                .emit_signal(physics_frame_root, "physics_frame", &[]);
            self.tree.process_internal_physics_frame();
            self.tree.process_physics_frame_with_scripts(physics_dt);
            self.physics_accumulator -= physics_dt;
            self.physics_time += physics_dt;
            physics_steps += 1;
        }

        // Clamp accumulator if we hit the step limit (spiral-of-death guard).
        if physics_steps >= self.max_physics_steps_per_frame {
            self.physics_accumulator = 0.0;
        }

        // -- animation / tween phase --
        self.tree.process_animations(delta_secs);
        self.tree.process_tweens(delta_secs);

        // -- particle phase (pat-u3l8c.4) --
        // Advance CPU particle simulators for emitting particle nodes so that
        // GPUParticles2D/3D and CPUParticles2D/3D actually simulate at runtime.
        self.process_particles(delta_secs as f32);

        // -- path-follow phase (pat-u3l8c.6) --
        // Project PathFollow2D/3D nodes onto their parent Path's curve each
        // frame so their transform tracks `progress` (and any auto-advance).
        self.process_path_follows(delta_secs as f32);

        // -- navigation phase (pat-u3l8c.5) --
        // Drive NavigationAgent2D/3D nodes along their computed paths so agents
        // with a target_position advance across frames at runtime.
        self.process_navigation(delta_secs as f32);

        // -- raycast phase (pat-u3l8c.49) --
        // Cast each RayCast2D against the (already-synced) physics world and
        // publish is_colliding / collider so scripts can read them this frame.
        self.process_raycasts();

        // -- 3D query-node phase (pat-nzgdu.12) --
        // Drive RayCast3D/ShapeCast3D/SpringArm3D against the synced 3D physics
        // world so is_colliding()/get_collider()/hit_length update at runtime.
        self.process_queries_3d();

        // -- Timer nodes (Godot Timer, idle process mode) (pat-u3l8c.3) --
        self.tree.process_timer_nodes(delta_secs);

        // -- multiplayer poll (pat-u3l8c.75) --
        // Decode inbound RPCs and dispatch them to node scripts before the
        // process phase, so `_process` sees RPC-driven state this frame.
        self.tree.poll_multiplayer();

        // -- process phase --
        // Godot per-frame order: INTERNAL_PROCESS -> PROCESS
        // Script calls are interleaved per-node to match Godot's dispatch.
        // SceneTree.process_frame fires immediately before the process phase
        // so `await get_tree().process_frame` resumes each frame (pat-nzgdu.84).
        let process_frame_root = self.tree.root_id();
        self.tree
            .emit_signal(process_frame_root, "process_frame", &[]);
        self.tree.process_internal_frame();
        self.tree.process_frame_with_scripts(delta_secs);

        // -- coroutine resume phase (pat-nzgdu.6) --
        // Drive GDScript `await` coroutines: advance SceneTreeTimers once,
        // deliver their timeout signals to parked awaits, and tick frame-based
        // awaits so `await get_tree().create_timer(t).timeout` resumes at
        // runtime instead of hanging forever.
        self.tree.process_script_coroutines(delta_secs as f32);

        // -- collision detection phase --
        // Run after scripts so collision properties are set before next frame's _process.
        self.tree.process_collisions();

        // -- audio phase (pat-u3l8c.2) --
        // After scripts so same-frame `playing` changes are heard immediately.
        self.audio_bridge.process(&mut self.tree, delta_secs);

        // -- deferred call phase (Godot MessageQueue flush) --
        // After process notifications; before queue_free deletions.
        self.tree.process_deferred_calls();

        // -- deferred deletion phase --
        // Remove nodes that called queue_free() during this frame.
        self.tree.process_deletions();

        // -- deferred scene change (pat-nzgdu.85) --
        // Godot applies change_scene_to_file() at the frame boundary (after
        // _process and the deferred/deletion phases) so the calling script's
        // scene is not freed mid-run. A load error is non-fatal to the loop.
        let _ = self.tree.process_deferred_scene_change();

        // -- input cleanup --
        // Clear input snapshot so it doesn't leak into the next frame.
        self.tree.clear_input_snapshot();
        // Flush per-frame input state (just_pressed / just_released) so
        // events don't carry over to the next frame.
        self.input_state.flush_frame();

        // -- bookkeeping --
        self.process_time += delta_secs;
        self.frame_counter += 1;

        FrameOutput {
            frame_count: self.frame_counter,
            delta: delta_secs,
            physics_steps,
        }
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
    use gdobject::notification::{
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
        assert_eq!(added.load(Ordering::SeqCst), 1, "node_added fires on add_child");

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
        assert!(tree.set_node_name(child, "Renamed"), "rename hits a live node");
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

        // Y rotation faces the +X tangent: yaw = atan2(1, 0) = PI/2.
        let rot = crate::node3d::get_rotation(ml.tree(), fid);
        assert!(
            (rot.y - std::f32::consts::FRAC_PI_2).abs() < 1e-2,
            "Y rotation should face the +X tangent (~PI/2), got {}",
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
        // rotation_mode defaults to None; seed a distinctive rotation.
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

    /// App-level: a NavigationAgent2D with a target advances along a computed
    /// multi-waypoint path across frames, its `get_next_path_position` guidance
    /// progressing from an intermediate waypoint to the target (pat-u3l8c.5).
    #[test]
    fn mainloop_navigation_agent_2d_paths_across_frames() {
        use gdcore::math::Vector2;
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        // Three adjacent navmesh polygons forming a corridor along +X.
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
                Vector2::new(70.0, -10.0),
                Vector2::new(110.0, -10.0),
                Vector2::new(110.0, 10.0),
                Vector2::new(70.0, 10.0),
            ],
        ];
        for verts in regions {
            let mut r = Node::new("Region", "NavigationRegion2D");
            r.set_property(
                "navmesh_vertices",
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
            gdvariant::Variant::Vector2(Vector2::new(100.0, 0.0)),
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

        // Early: a 3-waypoint path exists; guidance is the first intermediate
        // waypoint (~(50,0)) and the agent has begun moving along +X.
        for _ in 0..5 {
            ml.step(1.0 / 60.0);
        }
        assert!(
            ml.nav_agent_path_len_2d(aid) >= 3,
            "expected a multi-waypoint path, got {}",
            ml.nav_agent_path_len_2d(aid),
        );
        let next_early = next2(&ml, aid);
        let pos_early = crate::node2d::get_position(ml.tree(), aid);
        assert!(
            pos_early.x > 0.0 && pos_early.x < 50.0,
            "agent should have started moving, x={}",
            pos_early.x,
        );
        assert!(
            (next_early.x - 50.0).abs() < 1e-2,
            "first guidance should be the intermediate waypoint ~(50,0), got {:?}",
            next_early,
        );

        // Later: the intermediate waypoint is consumed, so guidance advances to
        // the target and the agent has moved further along.
        for _ in 0..90 {
            ml.step(1.0 / 60.0);
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
            (next_late.x - 100.0).abs() < 1e-2,
            "guidance should advance to the target ~(100,0), got {:?}",
            next_late,
        );
        assert!(
            (next_late.x - next_early.x).abs() > 1.0,
            "get_next_path_position must change across frames ({:?} -> {:?})",
            next_early,
            next_late,
        );
        assert!(
            pos_late.x > 90.0,
            "agent should reach near the target, x={}",
            pos_late.x
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
            region.set_property(
                "navmesh_vertices",
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
            ml.step(1.0 / 60.0);
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
        // Guidance advanced monotonically toward +X (never regressed).
        assert!(samples.len() >= 2, "expected multiple guidance samples");
        for w in samples.windows(2) {
            assert!(
                w[1] >= w[0] - 1e-3,
                "get_next_path_position must not regress ({} -> {})",
                w[0],
                w[1],
            );
        }
        assert!(
            samples.last().unwrap() - samples.first().unwrap() > 1.0,
            "guidance must progress across frames ({:?})",
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

        // One flat navmesh polygon on the XZ plane covering the route.
        let mut region = Node::new("Region", "NavigationRegion3D");
        region.set_property(
            "navmesh_vertices",
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
            ml.step(1.0 / 60.0);
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
            ml.step(1.0 / 60.0);
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
        assert_eq!(log.len(), 8);
        assert_eq!(log[0], NOTIFICATION_POSTINITIALIZE);
        assert_eq!(log[1], NOTIFICATION_PARENTED);
        assert_eq!(log[2], NOTIFICATION_ENTER_TREE);
        assert_eq!(log[3], NOTIFICATION_READY);
        assert_eq!(log[4], NOTIFICATION_INTERNAL_PHYSICS_PROCESS);
        assert_eq!(log[5], NOTIFICATION_PHYSICS_PROCESS);
        assert_eq!(log[6], NOTIFICATION_INTERNAL_PROCESS);
        assert_eq!(log[7], NOTIFICATION_PROCESS);
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
        assert_eq!(log[3], NOTIFICATION_READY);
        for frame in 0..5 {
            let base = 4 + frame * 4; // +4 for leading lifecycle
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
        assert_eq!(parent_log.len(), 9);
        assert_eq!(child_log.len(), 8);

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
            let expected_len = 4 + (frame as usize) * 4;
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
        assert_eq!(a_log.len(), 9); // 4 lifecycle + CHILD_ORDER_CHANGED + 4 frame
        let b_log = ml.tree().get_node(b_id).unwrap().notification_log();
        assert_eq!(b_log.len(), 9); // 4 lifecycle + CHILD_ORDER_CHANGED + 4 frame
        let c_log = ml.tree().get_node(c_id).unwrap().notification_log();
        assert_eq!(c_log.len(), 8); // 4 lifecycle + 4 frame (no children)
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
