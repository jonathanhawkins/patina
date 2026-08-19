//! Engine performance monitors — the registry behind Godot's `Performance`
//! singleton (pat-0q2kx.208).
//!
//! Godot ground truth: `upstream/godot/main/performance.h` (the `Monitor` enum)
//! and `main/performance.cpp` (the monitor name table, which carries a
//! `static_assert(std_size(names) == MONITOR_MAX)` pinning the two together).
//!
//! `Performance.get_monitor(Monitor)` reads a live engine counter, and
//! `add_custom_monitor(name, Callable)` registers a user monitor the editor
//! graphs. Patina had neither: no dispatch arm in the interpreter, no ClassDB
//! registration, and the editor's monitors endpoint invented numbers from
//! class-name heuristics.
//!
//! This module is the storage layer those pieces share. It deliberately knows
//! nothing about scripting: a custom monitor is a plain `Fn() -> f64`, so the
//! interpreter can wrap a `Callable` without `gdcore` depending on it. Godot
//! evaluates custom monitors at *read* time, and keeping the callback (rather
//! than a cached value) is what preserves that.
//!
//! ## Ownership
//!
//! [`PerformanceRegistry`] is an ordinary owned value; [`global`] is the
//! process-wide instance the engine feeds. Tests should construct their own
//! registry — global state shared across concurrently running tests is a
//! flakiness source, and nothing here requires the singleton.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

// Variant names, so the 59-entry table below stays readable.
use Monitor::*;

/// Number of built-in monitors. Mirrors Godot's `Performance::MONITOR_MAX`.
///
/// Godot's header guards the `NAVIGATION_3D_*` block behind `_3D_DISABLED`;
/// this count matches a standard build with 3D enabled, which is the
/// configuration Patina targets.
pub const MONITOR_MAX: usize = 59;

/// A built-in engine monitor.
///
/// Discriminants are Godot's, in header order — scripts pass these as plain
/// integers (`Performance.get_monitor(Performance.OBJECT_NODE_COUNT)`), so the
/// numbering is load-bearing API, not an implementation detail.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(usize)]
pub enum Monitor {
    TimeFps = 0,
    TimeProcess,
    TimePhysicsProcess,
    TimeNavigationProcess,
    MemoryStatic,
    MemoryStaticMax,
    MemoryMessageBufferMax,
    ObjectCount,
    ObjectResourceCount,
    ObjectNodeCount,
    ObjectOrphanNodeCount,
    RenderTotalObjectsInFrame,
    RenderTotalPrimitivesInFrame,
    RenderTotalDrawCallsInFrame,
    RenderVideoMemUsed,
    RenderTextureMemUsed,
    RenderBufferMemUsed,
    Physics2dActiveObjects,
    Physics2dCollisionPairs,
    Physics2dIslandCount,
    Physics3dActiveObjects,
    Physics3dCollisionPairs,
    Physics3dIslandCount,
    AudioOutputLatency,
    NavigationActiveMaps,
    NavigationRegionCount,
    NavigationAgentCount,
    NavigationLinkCount,
    NavigationPolygonCount,
    NavigationEdgeCount,
    NavigationEdgeMergeCount,
    NavigationEdgeConnectionCount,
    NavigationEdgeFreeCount,
    NavigationObstacleCount,
    PipelineCompilationsCanvas,
    PipelineCompilationsMesh,
    PipelineCompilationsSurface,
    PipelineCompilationsDraw,
    PipelineCompilationsSpecialization,
    Navigation2dActiveMaps,
    Navigation2dRegionCount,
    Navigation2dAgentCount,
    Navigation2dLinkCount,
    Navigation2dPolygonCount,
    Navigation2dEdgeCount,
    Navigation2dEdgeMergeCount,
    Navigation2dEdgeConnectionCount,
    Navigation2dEdgeFreeCount,
    Navigation2dObstacleCount,
    Navigation3dActiveMaps,
    Navigation3dRegionCount,
    Navigation3dAgentCount,
    Navigation3dLinkCount,
    Navigation3dPolygonCount,
    Navigation3dEdgeCount,
    Navigation3dEdgeMergeCount,
    Navigation3dEdgeConnectionCount,
    Navigation3dEdgeFreeCount,
    Navigation3dObstacleCount,
}

/// Every monitor in Godot's declaration order. Index equals the discriminant.
pub const ALL_MONITORS: [Monitor; MONITOR_MAX] = [
    TimeFps,
    TimeProcess,
    TimePhysicsProcess,
    TimeNavigationProcess,
    MemoryStatic,
    MemoryStaticMax,
    MemoryMessageBufferMax,
    ObjectCount,
    ObjectResourceCount,
    ObjectNodeCount,
    ObjectOrphanNodeCount,
    RenderTotalObjectsInFrame,
    RenderTotalPrimitivesInFrame,
    RenderTotalDrawCallsInFrame,
    RenderVideoMemUsed,
    RenderTextureMemUsed,
    RenderBufferMemUsed,
    Physics2dActiveObjects,
    Physics2dCollisionPairs,
    Physics2dIslandCount,
    Physics3dActiveObjects,
    Physics3dCollisionPairs,
    Physics3dIslandCount,
    AudioOutputLatency,
    NavigationActiveMaps,
    NavigationRegionCount,
    NavigationAgentCount,
    NavigationLinkCount,
    NavigationPolygonCount,
    NavigationEdgeCount,
    NavigationEdgeMergeCount,
    NavigationEdgeConnectionCount,
    NavigationEdgeFreeCount,
    NavigationObstacleCount,
    PipelineCompilationsCanvas,
    PipelineCompilationsMesh,
    PipelineCompilationsSurface,
    PipelineCompilationsDraw,
    PipelineCompilationsSpecialization,
    Navigation2dActiveMaps,
    Navigation2dRegionCount,
    Navigation2dAgentCount,
    Navigation2dLinkCount,
    Navigation2dPolygonCount,
    Navigation2dEdgeCount,
    Navigation2dEdgeMergeCount,
    Navigation2dEdgeConnectionCount,
    Navigation2dEdgeFreeCount,
    Navigation2dObstacleCount,
    Navigation3dActiveMaps,
    Navigation3dRegionCount,
    Navigation3dAgentCount,
    Navigation3dLinkCount,
    Navigation3dPolygonCount,
    Navigation3dEdgeCount,
    Navigation3dEdgeMergeCount,
    Navigation3dEdgeConnectionCount,
    Navigation3dEdgeFreeCount,
    Navigation3dObstacleCount,
];

/// Godot's monitor names, in enum order (`performance.cpp`). These are the
/// strings the editor's profiler shows and `get_monitor_name` returns.
const MONITOR_NAMES: [&str; MONITOR_MAX] = [
    "time/fps",
    "time/process",
    "time/physics_process",
    "time/navigation_process",
    "memory/static",
    "memory/static_max",
    "memory/msg_buf_max",
    "object/objects",
    "object/resources",
    "object/nodes",
    "object/orphan_nodes",
    "raster/total_objects_drawn",
    "raster/total_primitives_drawn",
    "raster/total_draw_calls",
    "video/video_mem",
    "video/texture_mem",
    "video/buffer_mem",
    "physics_2d/active_objects",
    "physics_2d/collision_pairs",
    "physics_2d/islands",
    "physics_3d/active_objects",
    "physics_3d/collision_pairs",
    "physics_3d/islands",
    "audio/driver/output_latency",
    "navigation/active_maps",
    "navigation/regions",
    "navigation/agents",
    "navigation/links",
    "navigation/polygons",
    "navigation/edges",
    "navigation/edges_merged",
    "navigation/edges_connected",
    "navigation/edges_free",
    "navigation/obstacles",
    "pipeline/compilations_canvas",
    "pipeline/compilations_mesh",
    "pipeline/compilations_surface",
    "pipeline/compilations_draw",
    "pipeline/compilations_specialization",
    "navigation_2d/active_maps",
    "navigation_2d/regions",
    "navigation_2d/agents",
    "navigation_2d/links",
    "navigation_2d/polygons",
    "navigation_2d/edges",
    "navigation_2d/edges_merged",
    "navigation_2d/edges_connected",
    "navigation_2d/edges_free",
    "navigation_2d/obstacles",
    "navigation_3d/active_maps",
    "navigation_3d/regions",
    "navigation_3d/agents",
    "navigation_3d/links",
    "navigation_3d/polygons",
    "navigation_3d/edges",
    "navigation_3d/edges_merged",
    "navigation_3d/edges_connected",
    "navigation_3d/edges_free",
    "navigation_3d/obstacles",
];

impl Monitor {
    /// The monitor's index, which is the integer scripts pass.
    pub fn to_index(self) -> usize {
        self as usize
    }

    /// Resolves the integer a script passed. Returns `None` for out-of-range
    /// values rather than clamping — `get_monitor(999)` is a script bug, and
    /// silently reading `time/fps` instead would hide it.
    pub fn from_index(index: i64) -> Option<Self> {
        if index < 0 {
            return None;
        }
        ALL_MONITORS.get(index as usize).copied()
    }

    /// Godot's name for this monitor, e.g. `"object/nodes"`.
    pub fn name(self) -> &'static str {
        MONITOR_NAMES[self as usize]
    }
}

/// A user-registered monitor. Evaluated on read, matching Godot.
type CustomMonitorFn = Box<dyn Fn() -> f64 + Send + Sync>;

/// Storage for the built-in monitors plus any custom ones.
pub struct PerformanceRegistry {
    values: [f64; MONITOR_MAX],
    custom: HashMap<String, CustomMonitorFn>,
    /// Registration order, which is the order `get_custom_monitor_names`
    /// reports. A `HashMap` alone would return them in an arbitrary order that
    /// changes between runs.
    custom_order: Vec<String>,
}

impl std::fmt::Debug for PerformanceRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The callbacks are not printable, so report the shape instead.
        f.debug_struct("PerformanceRegistry")
            .field("custom_monitors", &self.custom_order)
            .finish_non_exhaustive()
    }
}

impl Default for PerformanceRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl PerformanceRegistry {
    pub fn new() -> Self {
        Self {
            values: [0.0; MONITOR_MAX],
            custom: HashMap::new(),
            custom_order: Vec::new(),
        }
    }

    /// Writes a built-in monitor. Called by whichever subsystem owns the
    /// counter — `MainLoop` for the timing monitors, `SceneTree` for the node
    /// counts, and so on.
    pub fn set(&mut self, monitor: Monitor, value: f64) {
        self.values[monitor.to_index()] = value;
    }

    /// Adds to a built-in monitor, for counters tracked incrementally.
    pub fn add(&mut self, monitor: Monitor, delta: f64) {
        self.values[monitor.to_index()] += delta;
    }

    pub fn get(&self, monitor: Monitor) -> f64 {
        self.values[monitor.to_index()]
    }

    /// Registers a custom monitor. Returns `false` if the name is already
    /// taken, matching Godot, which errors rather than replacing.
    pub fn add_custom_monitor(&mut self, name: &str, callback: CustomMonitorFn) -> bool {
        if self.custom.contains_key(name) {
            return false;
        }
        self.custom.insert(name.to_string(), callback);
        self.custom_order.push(name.to_string());
        true
    }

    pub fn remove_custom_monitor(&mut self, name: &str) -> bool {
        if self.custom.remove(name).is_none() {
            return false;
        }
        self.custom_order.retain(|n| n != name);
        true
    }

    pub fn has_custom_monitor(&self, name: &str) -> bool {
        self.custom.contains_key(name)
    }

    /// Evaluates a custom monitor. `None` if no such monitor is registered —
    /// distinct from a monitor that legitimately returns `0.0`.
    pub fn get_custom_monitor(&self, name: &str) -> Option<f64> {
        self.custom.get(name).map(|f| f())
    }

    /// Custom monitor names in registration order.
    pub fn custom_monitor_names(&self) -> Vec<String> {
        self.custom_order.clone()
    }

    /// Clears every built-in counter and custom monitor. Intended for the start
    /// of a play session, so numbers from a previous run cannot leak into a new
    /// one.
    pub fn reset(&mut self) {
        self.values = [0.0; MONITOR_MAX];
        self.custom.clear();
        self.custom_order.clear();
    }
}

/// Turns a frame loop's **cumulative** clocks into the per-frame timing
/// monitors (pat-0q2kx.208, stage 2).
///
/// This exists because the obvious wiring is wrong. `MainLoop::process_time`
/// and `physics_time` are running totals — "the sum of all `delta_secs` values
/// passed to `step`" — while Godot's `TIME_PROCESS` / `TIME_PHYSICS_PROCESS`
/// report the time of the **last frame**. Assigning the totals straight into
/// the monitors would produce a number that climbs forever and looks plausible
/// on a graph, which is the kind of wrong that survives review.
///
/// `TIME_FPS` is likewise a trailing-second frame count in Godot
/// (`Engine::get_frames_per_second`), not `1.0 / delta` of the newest frame.
///
/// Fidelity caveat, stated rather than papered over: the totals accumulate
/// wall-clock frame deltas, so the difference is *frame time*, not time spent
/// inside `_process`. Reporting genuine process-vs-physics split needs the
/// frame loop to time those phases separately.
#[derive(Debug, Default)]
pub struct FrameMonitorFeed {
    last_process_total: f64,
    last_physics_total: f64,
    /// Cumulative-process-time stamp of each frame in the trailing second.
    recent_frames: std::collections::VecDeque<f64>,
}

impl FrameMonitorFeed {
    pub fn new() -> Self {
        Self::default()
    }

    /// Records one frame and writes `TIME_FPS`, `TIME_PROCESS` and
    /// `TIME_PHYSICS_PROCESS`. Both arguments are the loop's cumulative totals.
    pub fn record_frame(
        &mut self,
        registry: &mut PerformanceRegistry,
        process_total: f64,
        physics_total: f64,
    ) {
        // `max(0.0)` guards a restarted loop whose totals went backwards; a
        // negative "duration" would otherwise reach the profiler.
        let process_delta = (process_total - self.last_process_total).max(0.0);
        let physics_delta = (physics_total - self.last_physics_total).max(0.0);
        self.last_process_total = process_total;
        self.last_physics_total = physics_total;

        registry.set(Monitor::TimeProcess, process_delta);
        registry.set(Monitor::TimePhysicsProcess, physics_delta);

        self.recent_frames.push_back(process_total);
        while self
            .recent_frames
            .front()
            .is_some_and(|&t| t < process_total - 1.0)
        {
            self.recent_frames.pop_front();
        }
        registry.set(Monitor::TimeFps, self.recent_frames.len() as f64);
    }

    /// Frames recorded in the trailing second.
    pub fn fps(&self) -> f64 {
        self.recent_frames.len() as f64
    }

    /// Forgets accumulated timing, for a fresh play session.
    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

/// The process-wide registry the running engine feeds and scripts read.
///
/// Prefer an owned [`PerformanceRegistry`] in tests: this instance is shared by
/// every test in a binary that touches it.
pub fn global() -> &'static Mutex<PerformanceRegistry> {
    static GLOBAL: OnceLock<Mutex<PerformanceRegistry>> = OnceLock::new();
    GLOBAL.get_or_init(|| Mutex::new(PerformanceRegistry::new()))
}

/// The `Performance.MONITOR_*` constant names a script writes, in Godot's
/// declaration order, so index equals the discriminant of [`ALL_MONITORS`].
///
/// ⚠️ Transcribed from `upstream/godot/doc/classes/Performance.xml`, **not**
/// derived from the Rust variant names or from [`MONITOR_NAMES`]. Both
/// mechanical derivations are wrong, in different places:
///
/// - a CamelCase split of `Physics2dActiveObjects` gives
///   `PHYSICS2D_ACTIVE_OBJECTS`, but Godot spells it
///   `PHYSICS_2D_ACTIVE_OBJECTS` (likewise every `NAVIGATION_2D_*` /
///   `NAVIGATION_3D_*` member);
/// - upper-casing the monitor string `memory/msg_buf_max` gives
///   `MEMORY_MSG_BUF_MAX`, but the constant is `MEMORY_MESSAGE_BUFFER_MAX`.
///
/// A script using the documented Godot spelling would miss under either.
pub const MONITOR_CONST_NAMES: [&str; MONITOR_MAX] = [
    "TIME_FPS",
    "TIME_PROCESS",
    "TIME_PHYSICS_PROCESS",
    "TIME_NAVIGATION_PROCESS",
    "MEMORY_STATIC",
    "MEMORY_STATIC_MAX",
    "MEMORY_MESSAGE_BUFFER_MAX",
    "OBJECT_COUNT",
    "OBJECT_RESOURCE_COUNT",
    "OBJECT_NODE_COUNT",
    "OBJECT_ORPHAN_NODE_COUNT",
    "RENDER_TOTAL_OBJECTS_IN_FRAME",
    "RENDER_TOTAL_PRIMITIVES_IN_FRAME",
    "RENDER_TOTAL_DRAW_CALLS_IN_FRAME",
    "RENDER_VIDEO_MEM_USED",
    "RENDER_TEXTURE_MEM_USED",
    "RENDER_BUFFER_MEM_USED",
    "PHYSICS_2D_ACTIVE_OBJECTS",
    "PHYSICS_2D_COLLISION_PAIRS",
    "PHYSICS_2D_ISLAND_COUNT",
    "PHYSICS_3D_ACTIVE_OBJECTS",
    "PHYSICS_3D_COLLISION_PAIRS",
    "PHYSICS_3D_ISLAND_COUNT",
    "AUDIO_OUTPUT_LATENCY",
    "NAVIGATION_ACTIVE_MAPS",
    "NAVIGATION_REGION_COUNT",
    "NAVIGATION_AGENT_COUNT",
    "NAVIGATION_LINK_COUNT",
    "NAVIGATION_POLYGON_COUNT",
    "NAVIGATION_EDGE_COUNT",
    "NAVIGATION_EDGE_MERGE_COUNT",
    "NAVIGATION_EDGE_CONNECTION_COUNT",
    "NAVIGATION_EDGE_FREE_COUNT",
    "NAVIGATION_OBSTACLE_COUNT",
    "PIPELINE_COMPILATIONS_CANVAS",
    "PIPELINE_COMPILATIONS_MESH",
    "PIPELINE_COMPILATIONS_SURFACE",
    "PIPELINE_COMPILATIONS_DRAW",
    "PIPELINE_COMPILATIONS_SPECIALIZATION",
    "NAVIGATION_2D_ACTIVE_MAPS",
    "NAVIGATION_2D_REGION_COUNT",
    "NAVIGATION_2D_AGENT_COUNT",
    "NAVIGATION_2D_LINK_COUNT",
    "NAVIGATION_2D_POLYGON_COUNT",
    "NAVIGATION_2D_EDGE_COUNT",
    "NAVIGATION_2D_EDGE_MERGE_COUNT",
    "NAVIGATION_2D_EDGE_CONNECTION_COUNT",
    "NAVIGATION_2D_EDGE_FREE_COUNT",
    "NAVIGATION_2D_OBSTACLE_COUNT",
    "NAVIGATION_3D_ACTIVE_MAPS",
    "NAVIGATION_3D_REGION_COUNT",
    "NAVIGATION_3D_AGENT_COUNT",
    "NAVIGATION_3D_LINK_COUNT",
    "NAVIGATION_3D_POLYGON_COUNT",
    "NAVIGATION_3D_EDGE_COUNT",
    "NAVIGATION_3D_EDGE_MERGE_COUNT",
    "NAVIGATION_3D_EDGE_CONNECTION_COUNT",
    "NAVIGATION_3D_EDGE_FREE_COUNT",
    "NAVIGATION_3D_OBSTACLE_COUNT",
];

/// Resolves a `Performance.TIME_FPS`-style constant name to its monitor.
pub fn monitor_from_const_name(name: &str) -> Option<Monitor> {
    MONITOR_CONST_NAMES
        .iter()
        .position(|n| *n == name)
        .map(|i| ALL_MONITORS[i])
}

/// Resolves the integer a script passes to `get_monitor` back to a monitor.
///
/// Returns `None` for an out-of-range id rather than clamping, so a typo'd
/// monitor reads as "no such monitor" instead of silently reporting `TIME_FPS`.
pub fn monitor_from_index(index: i64) -> Option<Monitor> {
    usize::try_from(index)
        .ok()
        .and_then(|i| ALL_MONITORS.get(i).copied())
}
