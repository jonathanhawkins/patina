//! SceneTree management and main loop integration.
//!
//! The [`SceneTree`] owns all nodes in a flat [`HashMap`] (arena-style
//! allocation) and provides operations for building, querying, and
//! traversing the node hierarchy.

use std::collections::{HashMap, HashSet};

use gdcore::error::{EngineError, EngineResult};
use gdobject::notification::{
    NOTIFICATION_CHILD_ORDER_CHANGED, NOTIFICATION_MOVED_IN_PARENT, NOTIFICATION_PARENTED,
    NOTIFICATION_UNPARENTED,
};
use gdobject::signal::SignalStore;
use gdobject::weak_ref;
use gdplatform::network::{MultiplayerAPI, RPCConfig, RPCMode, TransferMode};
use gdscript_interop::bindings::ScriptInstance;
use gdvariant::Variant;

use crate::animation::{AnimationCallbackMode, AnimationPlayer, AnimationPlayerSignal};
use crate::animation_tree::AnimationTree;
use crate::sprite_frames::{AnimatedSpritePlayback, SpriteFrames};
use crate::lifecycle::LifecycleManager;
use crate::node::{Node, NodeId};
use crate::trace::{EventTrace, TraceEvent, TraceEventType};
use crate::tween::{Tween, TweenId};

/// Stable FNV-1a hash of a node path, used as the RPC `node_path` key on the
/// wire (pat-u3l8c.75). Deterministic across builds so two peers agree on the
/// target of an `rpc()` call.
pub fn rpc_path_hash(path: &str) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for b in path.as_bytes() {
        hash ^= *b as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// A single entry in the deferred call queue ([`SceneTree::call_deferred`]).
///
/// Queued during the frame and executed by [`SceneTree::process_deferred_calls`]
/// after process notifications (Godot MessageQueue order).
#[derive(Debug, Clone)]
pub struct DeferredCall {
    /// Target node.
    pub node_id: NodeId,
    /// Method name (script method, or special `"set"` for property set).
    pub method: String,
    /// Arguments passed to the method.
    pub args: Vec<Variant>,
}

/// One-shot timer created by [`SceneTree::create_timer`] (Godot SceneTreeTimer).
#[derive(Debug, Clone)]
pub struct SceneTreeTimer {
    /// Unique id for this timer.
    pub id: u64,
    /// Seconds remaining until timeout.
    pub time_left: f64,
    /// When true, the timer has fired and should be removed.
    pub done: bool,
    /// Signal name for awaiters (`timeout` or `timeout_<id>`).
    pub signal_name: String,
}

/// Flags controlling what [`SceneTree::duplicate_node`] copies, mirroring
/// Godot's `Node.DUPLICATE_*` constants (pat-nzgdu.138).
///
/// Combine with `|`; the [`Default`] is Godot's own `duplicate()` default
/// (`SIGNALS | GROUPS | SCRIPTS`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DuplicateFlags(u32);

impl DuplicateFlags {
    /// Copy nothing beyond properties and the unique-name flag.
    pub const NONE: Self = Self(0);
    /// Copy signal connections (Godot `DUPLICATE_SIGNALS` = 1).
    pub const SIGNALS: Self = Self(1);
    /// Copy group membership (Godot `DUPLICATE_GROUPS` = 2).
    pub const GROUPS: Self = Self(2);
    /// Copy attached scripts (Godot `DUPLICATE_SCRIPTS` = 4).
    ///
    /// Accepted but **not yet honored** — `ScriptInstance` has no clone.
    pub const SCRIPTS: Self = Self(4);
    /// Duplicate using scene instantiation (Godot `DUPLICATE_USE_INSTANTIATION`
    /// = 8). Accepted but not yet honored.
    pub const USE_INSTANTIATION: Self = Self(8);

    /// Builds flags from Godot's raw bitmask (e.g. from GDScript).
    pub const fn from_bits(bits: u32) -> Self {
        Self(bits)
    }

    /// The raw Godot bitmask.
    pub const fn bits(self) -> u32 {
        self.0
    }

    /// Whether every bit in `other` is set.
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }
}

impl Default for DuplicateFlags {
    /// Godot's `duplicate()` default: `SIGNALS | GROUPS | SCRIPTS`.
    fn default() -> Self {
        Self(1 | 2 | 4)
    }
}

impl std::ops::BitOr for DuplicateFlags {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}

/// The scene tree — an arena that owns every node and maintains the
/// hierarchy.
///
/// A root node is created automatically and is always present.
pub struct SceneTree {
    /// Arena storage: every node in the tree lives here.
    nodes: HashMap<NodeId, Node>,
    /// The ID of the root node (always valid).
    root_id: NodeId,
    /// The root `Window`'s size and content-scale configuration (pat-nzgdu.87).
    root_window: crate::viewport::RootWindow,
    /// Group index: group name -> set of member NodeIds.
    groups: HashMap<String, HashSet<NodeId>>,
    /// Per-node signal stores. Lazily created when a signal is connected.
    signal_stores: HashMap<NodeId, SignalStore>,
    /// Per-node animation players.
    animation_players: HashMap<NodeId, AnimationPlayer>,
    /// Per-node animation trees (Godot `AnimationTree` nodes), ticked each frame
    /// by [`process_animation_trees`](Self::process_animation_trees).
    animation_trees: HashMap<NodeId, AnimationTree>,
    /// Control currently under the mouse, for `mouse_entered`/`mouse_exited`
    /// transitions in the GUI input stage (pat-0q2kx.18).
    hovered_control: Option<NodeId>,
    /// Control that received the most recent mouse press. A release only counts
    /// as a click when it lands on this same control, as in Godot.
    gui_press_target: Option<NodeId>,
    /// The Control holding keyboard focus (Godot's `Viewport::gui.key_focus`).
    /// Clicking a focusable Control grabs it (pat-0q2kx.21).
    focus_owner: Option<NodeId>,
    /// Active tweens: maps TweenId -> (owning NodeId, Tween).
    tweens: HashMap<TweenId, (NodeId, Tween)>,
    /// Per-node script instances. Attached scripts receive lifecycle
    /// callbacks (_ready, _process, etc.) during scene execution.
    scripts: HashMap<NodeId, Box<dyn ScriptInstance>>,
    /// Snapshot of input state for the current frame, used by scripts
    /// that call `Input.is_action_pressed()` etc.
    input_snapshot: Option<crate::scripting::InputSnapshot>,
    /// `Input` mutations issued by scripts this frame — `action_press`,
    /// `set_mouse_mode`, `warp_mouse`, joypad rumble. The tree cannot apply them
    /// itself (the engine `InputState` belongs to the frame owner, `MainLoop`),
    /// so they are queued here and drained once per frame (pat-0q2kx.24).
    pending_input_commands: Vec<crate::scripting::InputCommand>,
    /// Last-known engine mouse mode, mirrored so a script's `Input.get_mouse_mode`
    /// can answer without reaching the `InputState` it cannot see. Refreshed each
    /// frame by the owner and updated eagerly when a script sets it, so a
    /// set-then-get in one frame is consistent.
    input_mouse_mode: i64,
    /// Last-known connected joypad ids, mirrored for `Input.get_connected_joypads`.
    input_connected_joypads: Vec<i64>,
    /// The packed scene that was used to load the current scene (if any).
    /// Stored for `reload_current_scene()`.
    current_packed_scene: Option<crate::packed_scene::PackedScene>,
    /// The NodeId of the current scene root (first child of root after
    /// `change_scene_to_packed` or `change_scene_to_node`).
    current_scene_id: Option<NodeId>,
    /// Nodes marked for deferred deletion via `queue_free()`.
    /// They are removed at the end of the frame by `process_deletions()`.
    pending_deletions: Vec<NodeId>,
    /// Methods queued via [`call_deferred`](Self::call_deferred). Flushed after
    /// the process phase (before deletions), matching Godot's MessageQueue.
    pending_deferred_calls: Vec<DeferredCall>,
    /// Active SceneTreeTimers from [`create_timer`] (pat-z35r2.3).
    scene_timers: Vec<SceneTreeTimer>,
    /// Next timer id.
    next_timer_id: u64,
    /// Global event trace for lifecycle/signal ordering verification.
    event_trace: EventTrace,
    /// Current frame number for trace recording.
    trace_frame: u64,
    /// Whether the scene tree is currently paused.
    paused: bool,
    /// Multiplayer coordinator wired into the tree (pat-u3l8c.75). Polled once
    /// per frame via [`poll_multiplayer`](Self::poll_multiplayer); inbound RPCs
    /// are dispatched to their target node's script.
    multiplayer: MultiplayerAPI,
    /// Method names declared `@rpc(..., "call_local", ...)`, which Godot also
    /// executes on the sending peer (pat-0q2kx.79).
    rpc_call_local: std::collections::HashSet<String>,
    /// A scene queued by [`change_scene_to_file`](Self::change_scene_to_file),
    /// applied at the next [`process_deferred_scene_change`] — Godot defers the
    /// swap to the frame boundary so the calling script's own scene is not
    /// freed mid-execution (pat-nzgdu.85).
    pending_scene: Option<crate::packed_scene::PackedScene>,
    /// Set by [`quit`](Self::quit); the runner/editor polls
    /// [`should_quit`](Self::should_quit) to stop its loop.
    should_quit: bool,
    /// Exit code from the last [`quit`](Self::quit) (Godot's `OS.exit_code`).
    exit_code: i32,
    /// Whether the input event currently being dispatched by
    /// [`push_input`](Self::push_input) has been marked handled. Reset at the
    /// start of each event; set via `set_input_as_handled()` from a script.
    input_handled: bool,
}

impl std::fmt::Debug for SceneTree {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SceneTree")
            .field("nodes", &self.nodes)
            .field("root_id", &self.root_id)
            .field("groups", &self.groups)
            .field("scripts", &format!("({} scripts)", self.scripts.len()))
            .finish()
    }
}

impl SceneTree {
    /// Creates a new scene tree with an empty root node named `"root"`.
    ///
    /// The root is a **`Window`** (pat-nzgdu.87), matching Godot, where
    /// `SceneTree.root` is a `Window` — itself a `Viewport`. That is what makes
    /// `Node.get_viewport()` / `get_window()` resolve for every node in the
    /// tree, and what gives the content-scale ("stretch") pipeline something to
    /// hang off.
    ///
    /// ⚠️ The `Object → Node → Viewport → Window` chain is registered here, not
    /// left to `register_viewport_classes()`: nothing calls that automatically,
    /// so without this line `root.is_class("Node")` would answer **false** in
    /// every test that did not register the viewport classes itself.
    pub fn new() -> Self {
        gdobject::class_db::register_root_window_chain();
        let mut root = Node::new("root", "Window");
        let root_id = root.id();
        // The root node is always inside the tree.
        root.set_inside_tree(true);
        weak_ref::register_object(root_id.object_id());
        let mut nodes = HashMap::new();
        nodes.insert(root_id, root);
        Self {
            nodes,
            root_id,
            groups: HashMap::new(),
            signal_stores: HashMap::new(),
            animation_players: HashMap::new(),
            animation_trees: HashMap::new(),
            hovered_control: None,
            gui_press_target: None,
            focus_owner: None,
            tweens: HashMap::new(),
            scripts: HashMap::new(),
            input_snapshot: None,
            pending_input_commands: Vec::new(),
            input_mouse_mode: 0,
            input_connected_joypads: Vec::new(),
            current_packed_scene: None,
            current_scene_id: None,
            pending_deletions: Vec::new(),
            pending_deferred_calls: Vec::new(),
            scene_timers: Vec::new(),
            next_timer_id: 1,
            event_trace: EventTrace::new(),
            trace_frame: 0,
            paused: false,
            multiplayer: MultiplayerAPI::new(),
            rpc_call_local: std::collections::HashSet::new(),
            pending_scene: None,
            should_quit: false,
            exit_code: 0,
            input_handled: false,
            root_window: crate::viewport::RootWindow::default(),
        }
    }

    /// The root window's size and content-scale ("stretch") configuration
    /// (pat-nzgdu.87).
    ///
    /// Godot stores this on the root `Window` node itself; Patina's nodes are
    /// untyped property bags, so the tree holds it and the root node stands for
    /// it in class terms.
    pub fn root_window(&self) -> &crate::viewport::RootWindow {
        &self.root_window
    }

    /// Mutable access to the root window, for the boot path that installs the
    /// project's `display/window/stretch/*` settings and for window resizes.
    pub fn root_window_mut(&mut self) -> &mut crate::viewport::RootWindow {
        &mut self.root_window
    }

    // -- multiplayer (pat-u3l8c.75) ----------------------------------------

    /// Shared read access to the tree's [`MultiplayerAPI`], mirroring Godot's
    /// `SceneTree.get_multiplayer()`.
    pub fn get_multiplayer(&self) -> &MultiplayerAPI {
        &self.multiplayer
    }

    /// Mutable access to the tree's [`MultiplayerAPI`] (assign a peer, register
    /// RPCs, set authority, initiate `rpc()` calls).
    pub fn get_multiplayer_mut(&mut self) -> &mut MultiplayerAPI {
        &mut self.multiplayer
    }

    /// Polls the multiplayer transport once (call once per frame) and dispatches
    /// every decoded inbound RPC to its target node's script method.
    ///
    /// The wire `node_path` is matched against the [`rpc_path_hash`] of each
    /// live node's path, so a peer that invokes `rpc(rpc_path_hash(path), …)`
    /// reaches the node at that path on the receiver.
    pub fn poll_multiplayer(&mut self) {
        // Non-RPC packets (spawn/sync/etc.) are left for higher layers.
        let _other = self.multiplayer.poll();
        let inbound = self.multiplayer.take_inbound_rpcs();
        for rpc in inbound {
            if let Some(node_id) = self.node_for_rpc_hash(rpc.node_path) {
                self.call_script_with_access(node_id, &rpc.method_name, &rpc.args);
            }
        }
    }

    /// Registers a script's `@rpc` methods with the tree's MultiplayerAPI.
    ///
    /// Godot configures `Node::rpc_config` from the annotation at script load
    /// (`gdscript_parser.cpp`); until this existed, `MultiplayerAPI::rpc` could
    /// only fail with `UnregisteredRPC` because nothing ever registered anything
    /// (pat-0q2kx.79).
    ///
    /// Argument order follows `@rpc("any_peer", "call_local", "reliable", 0)`,
    /// but Godot accepts them in ANY order and omits what it does not need, so
    /// each argument is classified by value rather than by position.
    pub fn register_script_rpcs(&mut self, methods: &[(String, Vec<String>)]) {
        for (name, args) in methods {
            let mut config = RPCConfig {
                // Godot's defaults for a bare `@rpc`.
                mode: RPCMode::Authority,
                transfer_mode: TransferMode::Unreliable,
                channel: 0,
            };
            for arg in args {
                match arg.as_str() {
                    "any_peer" => config.mode = RPCMode::Any,
                    "authority" => config.mode = RPCMode::Authority,
                    "reliable" => config.transfer_mode = TransferMode::Reliable,
                    "unreliable" => config.transfer_mode = TransferMode::Unreliable,
                    // Patina's transport spells Godot's `unreliable_ordered`
                    // as `Ordered`.
                    "unreliable_ordered" => config.transfer_mode = TransferMode::Ordered,
                    // `call_local` / `call_remote` are about local execution, not
                    // transport, and are tracked separately below.
                    "call_local" | "call_remote" => {}
                    other => {
                        // The channel is the only numeric argument.
                        if let Ok(ch) = other.parse::<u8>() {
                            config.channel = ch;
                        }
                    }
                }
            }
            if args.iter().any(|a| a == "call_local") {
                self.rpc_call_local.insert(name.clone());
            }
            self.multiplayer.register_rpc(name.clone(), config);
        }
    }

    /// Whether `method` was declared `@rpc(..., "call_local", ...)`, i.e. Godot
    /// also runs it on the caller.
    pub fn rpc_is_call_local(&self, method: &str) -> bool {
        self.rpc_call_local.contains(method)
    }

    /// Resolves an inbound RPC `node_path` hash to a live node by matching the
    /// [`rpc_path_hash`] of each node's absolute path.
    fn node_for_rpc_hash(&self, hash: u64) -> Option<NodeId> {
        self.nodes
            .keys()
            .copied()
            .find(|&id| self.node_path(id).as_deref().map(rpc_path_hash) == Some(hash))
    }

    /// Create a one-shot [`SceneTreeTimer`] (Godot `SceneTree.create_timer`).
    ///
    /// Returns the timer id. Advance with [`process_timers`]; when finished the
    /// timer is marked `done` and its `signal_name` is available via
    /// [`take_timeout_signals`].
    pub fn create_timer(&mut self, seconds: f64) -> u64 {
        let id = self.next_timer_id;
        self.next_timer_id = self.next_timer_id.saturating_add(1);
        let signal_name = format!("timeout_{id}");
        self.scene_timers.push(SceneTreeTimer {
            id,
            time_left: seconds.max(0.0),
            done: false,
            signal_name,
        });
        id
    }

    /// Look up a live timer by id.
    pub fn get_timer(&self, id: u64) -> Option<&SceneTreeTimer> {
        self.scene_timers.iter().find(|t| t.id == id)
    }

    /// Advance all SceneTreeTimers by `delta` seconds. Completed timers stay
    /// until drained by [`take_timeout_signals`].
    pub fn process_timers(&mut self, delta: f64) {
        for t in &mut self.scene_timers {
            if t.done {
                continue;
            }
            t.time_left -= delta;
            if t.time_left <= 0.0 {
                t.time_left = 0.0;
                t.done = true;
            }
        }
    }

    /// Drain signal names for timers that timed out (for await resume).
    /// Also removes finished timers after reporting.
    pub fn take_timeout_signals(&mut self) -> Vec<String> {
        let mut names = Vec::new();
        let mut keep = Vec::new();
        for t in self.scene_timers.drain(..) {
            if t.done {
                names.push(t.signal_name);
                // Also emit generic "timeout" for the latest style awaiters.
                names.push("timeout".to_string());
            } else {
                keep.push(t);
            }
        }
        self.scene_timers = keep;
        names
    }

    /// Drives GDScript coroutines suspended on `await` for every attached script
    /// once per frame (pat-nzgdu.6). Called from `MainLoop::step` after the
    /// process phase so `await get_tree().create_timer(t).timeout` and
    /// frame-based awaits actually resume at runtime.
    ///
    /// SceneTreeTimers are advanced and drained here exactly once — not once per
    /// script — and their timeout signals wake the parked awaits; each script
    /// then advances its own frame-based awaits.
    pub fn process_script_coroutines(&mut self, delta: f32) {
        // Advance one-shot SceneTreeTimers once and collect fired timeout signals.
        self.process_timers(delta as f64);
        let fired = self.take_timeout_signals();

        // Temporarily remove each script while it runs so a resumed coroutine
        // body can call back into the tree via SceneTreeAccessor without
        // violating borrow rules (mirrors `call_script_with_access`).
        let node_ids: Vec<NodeId> = self.scripts.keys().copied().collect();
        for node_id in node_ids {
            let Some(mut script) = self.scripts.remove(&node_id) else {
                continue;
            };
            let snapshot_clone = self.input_snapshot.clone();
            // SAFETY: same invariant as `call_script_with_access` — the accessor
            // lives only within this frame and is cleared before re-insertion,
            // so no dangling pointer escapes.
            let accessor = if let Some(snapshot) = snapshot_clone {
                unsafe {
                    crate::scripting::SceneTreeAccessor::with_input(
                        self as *mut SceneTree,
                        snapshot,
                    )
                }
            } else {
                unsafe { crate::scripting::SceneTreeAccessor::new(self as *mut SceneTree) }
            };
            script.set_scene_access(Box::new(accessor), node_id.raw());
            for sig in &fired {
                script.notify_await_signal(sig, &[]);
            }
            script.poll_coroutines(delta);
            script.clear_scene_access();

            // Sync any variables a resumed coroutine wrote (e.g. `done = true`)
            // back to node properties, matching `call_script_with_access`.
            let prop_values: Vec<(String, Variant)> = script
                .list_properties()
                .into_iter()
                .filter_map(|info| script.get_property(&info.name).map(|v| (info.name, v)))
                .collect();
            if let Some(node) = self.nodes.get_mut(&node_id) {
                for (name, val) in prop_values {
                    node.set_property(&name, val);
                }
            }

            self.scripts.insert(node_id, script);
        }
    }

    /// Advances all `Timer` nodes in the tree (Godot `Timer`, idle process
    /// callback) (pat-u3l8c.3).
    ///
    /// Handles `autostart`, `paused`, `one_shot`, and `wait_time`, emitting
    /// the `timeout` signal on expiry. Runtime state lives in node
    /// properties so scripts can read `time_left`; `__timer_started` /
    /// `__timer_running` are internal markers. Scripted `start()`/`stop()`
    /// requires node-method dispatch (tracked separately); `autostart`
    /// covers the `.tscn` path.
    pub fn process_timer_nodes(&mut self, delta: f64) {
        let timer_ids: Vec<NodeId> = self
            .all_nodes_in_tree_order()
            .into_iter()
            .filter(|&id| self.get_node(id).is_some_and(|n| n.class_name() == "Timer"))
            .collect();

        for id in timer_ids {
            let Some(node) = self.get_node(id) else {
                continue;
            };
            let as_bool =
                |key: &str| matches!(node.get_property(key), gdvariant::Variant::Bool(true));
            let as_float = |key: &str, default: f64| match node.get_property(key) {
                gdvariant::Variant::Float(f) => f,
                gdvariant::Variant::Int(i) => i as f64,
                _ => default,
            };
            let started = as_bool("__timer_started");
            let mut running = as_bool("__timer_running");
            let mut time_left = as_float("time_left", 0.0);
            let wait_time = as_float("wait_time", 1.0).max(1e-4);
            let one_shot = as_bool("one_shot");
            let paused = as_bool("paused");
            let autostart = as_bool("autostart");

            if !started {
                if autostart {
                    running = true;
                    time_left = wait_time;
                }
                if let Some(node) = self.get_node_mut(id) {
                    node.set_property("__timer_started", gdvariant::Variant::Bool(true));
                    node.set_property("__timer_running", gdvariant::Variant::Bool(running));
                    node.set_property("time_left", gdvariant::Variant::Float(time_left));
                }
            }

            if !running || paused {
                continue;
            }

            time_left -= delta;
            let fired = time_left <= 0.0;
            if fired {
                if one_shot {
                    running = false;
                    time_left = 0.0;
                } else {
                    // Repeating timer: carry the overshoot into the next cycle.
                    time_left += wait_time;
                    if time_left <= 0.0 {
                        time_left = wait_time;
                    }
                }
            }

            if let Some(node) = self.get_node_mut(id) {
                node.set_property("time_left", gdvariant::Variant::Float(time_left));
                node.set_property("__timer_running", gdvariant::Variant::Bool(running));
            }
            if fired {
                self.emit_signal(id, "timeout", &[]);
            }
        }
    }

    /// Returns the ID of the root node.
    pub fn root_id(&self) -> NodeId {
        self.root_id
    }

    /// Returns the ID of the current scene root, if one has been set via
    /// `change_scene_to_packed` or `change_scene_to_node`.
    pub fn current_scene(&self) -> Option<NodeId> {
        self.current_scene_id
    }

    /// Sets the input snapshot for the current frame. Scripts will see this
    /// state when they call `Input.is_action_pressed()` etc.
    pub fn set_input_snapshot(&mut self, snapshot: crate::scripting::InputSnapshot) {
        self.input_snapshot = Some(snapshot);
    }

    /// Clears the input snapshot.
    pub fn clear_input_snapshot(&mut self) {
        self.input_snapshot = None;
    }

    /// Returns `true` if an input snapshot is currently set.
    pub fn has_input_snapshot(&self) -> bool {
        self.input_snapshot.is_some()
    }

    /// Returns a reference to the current input snapshot, if any.
    pub fn input_snapshot(&self) -> Option<&crate::scripting::InputSnapshot> {
        self.input_snapshot.as_ref()
    }

    // -- script-issued Input mutations (pat-0q2kx.24) ------------------------

    /// Queues an `Input` mutation a script asked for, to be applied to the engine
    /// `InputState` by the frame owner. In Godot these calls mutate live engine
    /// state (core/input/input.cpp); the tree cannot, so it records intent.
    pub fn queue_input_command(&mut self, command: crate::scripting::InputCommand) {
        self.pending_input_commands.push(command);
    }

    /// Takes the queued `Input` mutations, leaving the queue empty. Called once
    /// per frame by [`crate::MainLoop::step`] after the script phase.
    pub fn drain_input_commands(&mut self) -> Vec<crate::scripting::InputCommand> {
        std::mem::take(&mut self.pending_input_commands)
    }

    /// Number of `Input` mutations queued but not yet applied.
    pub fn pending_input_command_count(&self) -> usize {
        self.pending_input_commands.len()
    }

    /// Mirrored engine mouse mode, as Godot's `Input.get_mouse_mode` reports it.
    pub fn input_mouse_mode(&self) -> i64 {
        self.input_mouse_mode
    }

    /// Updates the mirrored mouse mode. Called by the frame owner after applying
    /// commands, and eagerly by a script's `set_mouse_mode` so a set-then-get in
    /// the same frame is consistent.
    pub fn set_input_mouse_mode_mirror(&mut self, mode: i64) {
        self.input_mouse_mode = mode;
    }

    /// Mirrored connected joypad ids, for `Input.get_connected_joypads`.
    pub fn input_connected_joypads(&self) -> &[i64] {
        &self.input_connected_joypads
    }

    /// Updates the mirrored joypad list from the engine `InputState`.
    pub fn set_input_connected_joypads_mirror(&mut self, devices: Vec<i64>) {
        self.input_connected_joypads = devices;
    }

    /// Applies a script's `Input.action_press` to the *current* frame's snapshot,
    /// so a later script in the same frame observes it — Godot's `action_press`
    /// is visible to `is_action_pressed` immediately (pat-0q2kx.24).
    pub fn inject_action_press(&mut self, action: &str) {
        if let Some(snapshot) = self.input_snapshot.as_mut() {
            snapshot.inject_action_press(action);
        } else {
            let mut snapshot = crate::scripting::InputSnapshot::default();
            snapshot.inject_action_press(action);
            self.input_snapshot = Some(snapshot);
        }
    }

    /// The `action_release` counterpart of [`inject_action_press`](Self::inject_action_press).
    pub fn inject_action_release(&mut self, action: &str) {
        if let Some(snapshot) = self.input_snapshot.as_mut() {
            snapshot.inject_action_release(action);
        }
    }

    /// Returns a reference to the event trace.
    pub fn event_trace(&self) -> &EventTrace {
        &self.event_trace
    }

    /// Returns a mutable reference to the event trace.
    pub fn event_trace_mut(&mut self) -> &mut EventTrace {
        &mut self.event_trace
    }

    /// Sets the current trace frame counter.
    pub fn set_trace_frame(&mut self, frame: u64) {
        self.trace_frame = frame;
    }

    /// Returns the current trace frame counter.
    pub fn trace_frame(&self) -> u64 {
        self.trace_frame
    }

    /// Returns a reference to a node by ID.
    pub fn get_node(&self, id: NodeId) -> Option<&Node> {
        self.nodes.get(&id)
    }

    /// Returns a mutable reference to a node by ID.
    pub fn get_node_mut(&mut self, id: NodeId) -> Option<&mut Node> {
        self.nodes.get_mut(&id)
    }

    /// Removes a node from the arena by ID **without** detaching it from its
    /// parent or running lifecycle hooks. Use this to extract a freshly
    /// created (unparented) node before passing it to [`add_child`](Self::add_child).
    pub fn take_node(&mut self, id: NodeId) -> Option<Node> {
        self.nodes.remove(&id)
    }

    /// Returns the total number of nodes in the tree (including root).
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    // -- hierarchy manipulation ---------------------------------------------

    /// Adds a node as a child of `parent_id`.
    ///
    /// Returns the [`NodeId`] of the newly inserted node.
    pub fn add_child(&mut self, parent_id: NodeId, mut node: Node) -> EngineResult<NodeId> {
        if !self.nodes.contains_key(&parent_id) {
            return Err(EngineError::NotFound(format!(
                "parent node {parent_id} not found"
            )));
        }

        let child_id = node.id();
        weak_ref::register_object(child_id.object_id());
        node.set_parent(Some(parent_id));
        self.nodes.insert(child_id, node);

        // Update parent's child list.
        if let Some(parent) = self.nodes.get_mut(&parent_id) {
            parent.add_child_id(child_id);
        }

        // NOTIFICATION_PARENTED fires when a node gains a parent.
        self.trace_record(child_id, TraceEventType::Notification, "PARENTED");
        if let Some(child) = self.nodes.get_mut(&child_id) {
            child.receive_notification(NOTIFICATION_PARENTED);
        }

        // NOTIFICATION_CHILD_ORDER_CHANGED fires on the parent when a child is added.
        self.trace_record(
            parent_id,
            TraceEventType::Notification,
            "CHILD_ORDER_CHANGED",
        );
        if let Some(parent) = self.nodes.get_mut(&parent_id) {
            parent.receive_notification(NOTIFICATION_CHILD_ORDER_CHANGED);
        }

        let should_enter_tree = self
            .nodes
            .get(&parent_id)
            .map(|parent| parent.is_inside_tree())
            .unwrap_or(false);
        if should_enter_tree {
            LifecycleManager::enter_tree(self, child_id);
            // Godot's SceneTree.node_added fires when a live node enters the
            // tree (pat-nzgdu.84). Modelled on the root's signal store since
            // Patina keys signals by NodeId and has no separate SceneTree entity.
            let root = self.root_id();
            self.emit_signal(root, "node_added", &[]);
        }

        // Camera3D auto-activation: if this Camera3D has current=true (explicit),
        // deactivate all others. If no Camera3D has current=true, auto-activate this one.
        let is_camera3d = self
            .nodes
            .get(&child_id)
            .map(|n| n.class_name() == "Camera3D")
            .unwrap_or(false);
        if is_camera3d {
            let this_is_current = self
                .nodes
                .get(&child_id)
                .map(|n| n.get_property("current") == Variant::Bool(true))
                .unwrap_or(false);

            if this_is_current {
                // Explicit current=true: deactivate all other Camera3D nodes.
                let others: Vec<NodeId> = self
                    .nodes
                    .iter()
                    .filter(|(id, n)| **id != child_id && n.class_name() == "Camera3D")
                    .map(|(id, _)| *id)
                    .collect();
                for id in others {
                    if let Some(node) = self.nodes.get_mut(&id) {
                        node.set_property("current", Variant::Bool(false));
                    }
                }
            } else {
                // No explicit current: auto-activate if no other Camera3D is current.
                let any_other_current = self.nodes.iter().any(|(id, n)| {
                    *id != child_id
                        && n.class_name() == "Camera3D"
                        && n.get_property("current") == Variant::Bool(true)
                });
                if !any_other_current {
                    if let Some(node) = self.nodes.get_mut(&child_id) {
                        node.set_property("current", Variant::Bool(true));
                    }
                }
            }
        }

        Ok(child_id)
    }

    /// Removes a node (and all its descendants) from the tree.
    ///
    /// Returns the IDs of all removed nodes in depth-first order
    /// (children before parent).
    pub fn remove_node(&mut self, id: NodeId) -> EngineResult<Vec<NodeId>> {
        if id == self.root_id {
            return Err(EngineError::InvalidOperation(
                "cannot remove the root node".into(),
            ));
        }
        if !self.nodes.contains_key(&id) {
            return Err(EngineError::NotFound(format!("node {id} not found")));
        }

        let should_exit_tree = self
            .nodes
            .get(&id)
            .map(|node| node.is_inside_tree())
            .unwrap_or(false);
        if should_exit_tree {
            LifecycleManager::exit_tree(self, id);
        }

        // Collect subtree in depth-first order (children before parent).
        let mut removed = Vec::new();
        self.collect_subtree_bottom_up(id, &mut removed);

        // Detach from parent and fire UNPARENTED.
        if let Some(parent_id) = self.nodes.get(&id).and_then(|n| n.parent()) {
            if let Some(parent) = self.nodes.get_mut(&parent_id) {
                parent.remove_child_id(id);
            }
        }
        self.trace_record(id, TraceEventType::Notification, "UNPARENTED");
        if let Some(node) = self.nodes.get_mut(&id) {
            node.receive_notification(NOTIFICATION_UNPARENTED);
        }

        // Remove all collected nodes from the arena and group index.
        for &nid in &removed {
            if let Some(node) = self.nodes.remove(&nid) {
                weak_ref::unregister_object(nid.object_id());
                for group in node.groups() {
                    if let Some(members) = self.groups.get_mut(group) {
                        members.remove(&nid);
                    }
                }
            }
        }

        Ok(removed)
    }

    /// Reparents a node to a new parent.
    pub fn reparent(&mut self, node_id: NodeId, new_parent_id: NodeId) -> EngineResult<()> {
        if node_id == self.root_id {
            return Err(EngineError::InvalidOperation(
                "cannot reparent the root node".into(),
            ));
        }
        if !self.nodes.contains_key(&node_id) {
            return Err(EngineError::NotFound(format!("node {node_id} not found")));
        }
        if !self.nodes.contains_key(&new_parent_id) {
            return Err(EngineError::NotFound(format!(
                "new parent {new_parent_id} not found"
            )));
        }

        // Reject reparenting a node into itself or one of its own descendants,
        // which would create a cycle and detach the subtree from the scene.
        // Walk up from the proposed new parent; reaching `node_id` means the
        // new parent lives inside node_id's subtree.
        {
            let mut cursor = Some(new_parent_id);
            while let Some(cur) = cursor {
                if cur == node_id {
                    return Err(EngineError::InvalidOperation(
                        "cannot reparent a node into itself or its own descendant".into(),
                    ));
                }
                cursor = self.nodes.get(&cur).and_then(|n| n.parent());
            }
        }

        // Detach from old parent.
        if let Some(old_parent_id) = self.nodes.get(&node_id).and_then(|n| n.parent()) {
            if let Some(old_parent) = self.nodes.get_mut(&old_parent_id) {
                old_parent.remove_child_id(node_id);
            }
        }

        // UNPARENTED fires when detached from old parent.
        self.trace_record(node_id, TraceEventType::Notification, "UNPARENTED");
        if let Some(node) = self.nodes.get_mut(&node_id) {
            node.receive_notification(NOTIFICATION_UNPARENTED);
        }

        // Attach to new parent.
        if let Some(new_parent) = self.nodes.get_mut(&new_parent_id) {
            new_parent.add_child_id(node_id);
        }
        if let Some(node) = self.nodes.get_mut(&node_id) {
            node.set_parent(Some(new_parent_id));
        }

        // PARENTED fires when attached to new parent.
        self.trace_record(node_id, TraceEventType::Notification, "PARENTED");
        if let Some(node) = self.nodes.get_mut(&node_id) {
            node.receive_notification(NOTIFICATION_PARENTED);
        }

        // MOVED_IN_PARENT fires after PARENTED during reparent.
        self.trace_record(node_id, TraceEventType::Notification, "MOVED_IN_PARENT");
        if let Some(node) = self.nodes.get_mut(&node_id) {
            node.receive_notification(NOTIFICATION_MOVED_IN_PARENT);
        }

        // CHILD_ORDER_CHANGED fires on the new parent.
        self.trace_record(
            new_parent_id,
            TraceEventType::Notification,
            "CHILD_ORDER_CHANGED",
        );
        if let Some(parent) = self.nodes.get_mut(&new_parent_id) {
            parent.receive_notification(NOTIFICATION_CHILD_ORDER_CHANGED);
        }

        Ok(())
    }

    // -- child ordering operations -------------------------------------------

    /// Moves a child to a new position within its parent's child list.
    ///
    /// Matches Godot's `Node.move_child(child, to_index)`. Dispatches:
    /// - `NOTIFICATION_MOVED_IN_PARENT` to the moved child
    /// - `NOTIFICATION_CHILD_ORDER_CHANGED` to the parent
    pub fn move_child(
        &mut self,
        parent_id: NodeId,
        child_id: NodeId,
        to_index: usize,
    ) -> EngineResult<()> {
        // Validate parent exists and child is actually a child of parent.
        let parent = self
            .nodes
            .get(&parent_id)
            .ok_or_else(|| EngineError::NotFound(format!("parent node {parent_id} not found")))?;
        let children = parent.children().to_vec();
        let from_index = children
            .iter()
            .position(|&c| c == child_id)
            .ok_or_else(|| {
                EngineError::InvalidOperation(format!(
                    "node {child_id} is not a child of {parent_id}"
                ))
            })?;

        if from_index == to_index {
            return Ok(());
        }

        let clamped = to_index.min(children.len() - 1);

        // Perform the move in the parent's child list.
        if let Some(parent) = self.nodes.get_mut(&parent_id) {
            let list = parent.children_mut();
            let id = list.remove(from_index);
            list.insert(clamped, id);
        }

        // Dispatch MOVED_IN_PARENT to the child.
        self.trace_record(child_id, TraceEventType::Notification, "MOVED_IN_PARENT");
        if let Some(node) = self.nodes.get_mut(&child_id) {
            node.receive_notification(NOTIFICATION_MOVED_IN_PARENT);
        }

        // Dispatch CHILD_ORDER_CHANGED to the parent.
        self.trace_record(
            parent_id,
            TraceEventType::Notification,
            "CHILD_ORDER_CHANGED",
        );
        if let Some(parent) = self.nodes.get_mut(&parent_id) {
            parent.receive_notification(NOTIFICATION_CHILD_ORDER_CHANGED);
        }

        Ok(())
    }

    /// Moves a node to be the last child of its parent (highest index).
    ///
    /// Matches Godot's `Node.raise()`. Equivalent to
    /// `move_child(parent, child, last_index)`.
    pub fn raise(&mut self, node_id: NodeId) -> EngineResult<()> {
        let parent_id = self
            .nodes
            .get(&node_id)
            .and_then(|n| n.parent())
            .ok_or_else(|| {
                EngineError::InvalidOperation(format!("node {node_id} has no parent"))
            })?;
        let child_count = self
            .nodes
            .get(&parent_id)
            .map(|p| p.children().len())
            .unwrap_or(0);
        if child_count == 0 {
            return Ok(());
        }
        self.move_child(parent_id, node_id, child_count - 1)
    }

    /// Moves a node to be the first child of its parent (index 0).
    ///
    /// Matches Godot's implicit "lower" pattern. Equivalent to
    /// `move_child(parent, child, 0)`.
    pub fn lower(&mut self, node_id: NodeId) -> EngineResult<()> {
        let parent_id = self
            .nodes
            .get(&node_id)
            .and_then(|n| n.parent())
            .ok_or_else(|| {
                EngineError::InvalidOperation(format!("node {node_id} has no parent"))
            })?;
        self.move_child(parent_id, node_id, 0)
    }

    // -- path operations ----------------------------------------------------

    /// Computes the absolute path of a node (e.g. `"/root/Main/Player"`).
    pub fn node_path(&self, id: NodeId) -> Option<String> {
        let mut parts = Vec::new();
        let mut current = id;
        loop {
            let node = self.nodes.get(&current)?;
            parts.push(node.name().to_string());
            if let Some(parent_id) = node.parent() {
                current = parent_id;
            } else {
                break;
            }
        }
        parts.reverse();
        Some(format!("/{}", parts.join("/")))
    }

    /// Looks up a node by its absolute path (e.g. `"/root/Main/Player"`).
    ///
    /// The path must start with `"/"` and the first component must match
    /// the root node's name.
    pub fn get_node_by_path(&self, path: &str) -> Option<NodeId> {
        if !path.starts_with('/') {
            return None;
        }
        let parts: Vec<&str> = path[1..].split('/').collect();
        if parts.is_empty() {
            return None;
        }

        let root = self.nodes.get(&self.root_id)?;
        if root.name() != parts[0] {
            return None;
        }

        let mut current_id = self.root_id;
        for &part in &parts[1..] {
            current_id = self.find_child_by_name(current_id, part)?;
        }
        Some(current_id)
    }

    /// Resolves a relative path from a given node (e.g. `"Player/Sprite"`).
    ///
    /// Supports `%UniqueName` syntax: if the **first** segment starts with
    /// `%`, it is resolved via [`get_node_by_unique_name`](Self::get_node_by_unique_name)
    /// within the scene-owner scope. Remaining segments (if any) are resolved
    /// relative to the unique-name result.
    pub fn get_node_relative(&self, from: NodeId, rel_path: &str) -> Option<NodeId> {
        if rel_path.is_empty() {
            return Some(from);
        }

        // Handle %UniqueName paths — first segment starts with '%'.
        if let Some(stripped) = rel_path.strip_prefix('%') {
            let parts: Vec<&str> = stripped.split('/').collect();
            let unique_node = self.get_node_by_unique_name(from, parts[0])?;
            if parts.len() == 1 {
                return Some(unique_node);
            }
            // Resolve remaining segments relative to the unique node.
            let rest = parts[1..].join("/");
            return self.get_node_relative(unique_node, &rest);
        }

        let parts: Vec<&str> = rel_path.split('/').collect();
        let mut current = from;
        for &part in &parts {
            match part {
                ".." => {
                    current = self.nodes.get(&current)?.parent()?;
                }
                "." => { /* stay */ }
                name => {
                    current = self.find_child_by_name(current, name)?;
                }
            }
        }
        Some(current)
    }

    /// Finds a direct child of `parent` whose name matches.
    fn find_child_by_name(&self, parent: NodeId, name: &str) -> Option<NodeId> {
        let parent_node = self.nodes.get(&parent)?;
        for &child_id in parent_node.children() {
            if let Some(child) = self.nodes.get(&child_id) {
                if child.name() == name {
                    return Some(child_id);
                }
            }
        }
        None
    }

    /// Resolves a `%UniqueName` within the scene owner scope.
    ///
    /// In Godot, `get_node("%Foo")` searches the subtree owned by the same
    /// scene root for a node with `unique_name == true` whose name matches
    /// `"Foo"`. The search starts from the owner of `from` (or `from`
    /// itself if it has no owner, i.e. it is a scene root).
    pub fn get_node_by_unique_name(&self, from: NodeId, name: &str) -> Option<NodeId> {
        // Determine the owner scope root.
        let owner_id = self
            .nodes
            .get(&from)
            .and_then(|n| n.owner())
            .unwrap_or(from);

        // Breadth-first search through the owner's subtree.
        self.find_unique_in_subtree(owner_id, name)
    }

    /// Searches a subtree (depth-first) for a node with `unique_name == true`
    /// and the given name.
    fn find_unique_in_subtree(&self, root: NodeId, name: &str) -> Option<NodeId> {
        let mut stack = vec![root];
        while let Some(id) = stack.pop() {
            if let Some(node) = self.nodes.get(&id) {
                if node.is_unique_name() && node.name() == name {
                    return Some(id);
                }
                // Push children in reverse order so left-most is visited first.
                for &child_id in node.children().iter().rev() {
                    stack.push(child_id);
                }
            }
        }
        None
    }

    // -- unique-name-in-owner (`%`) indicator -------------------------------

    /// Toggles the node's "Access as Unique Name" flag (`unique_name_in_owner`
    /// in Godot). When enabled the Scene Tree row shows the `%` badge and the
    /// node becomes reachable via the `%Name` path within its owner. Returns
    /// `true` if the node existed and was updated.
    pub fn set_unique_name_in_owner(&mut self, id: NodeId, unique: bool) -> bool {
        match self.get_node_mut(id) {
            Some(node) => {
                node.set_unique_name(unique);
                true
            }
            None => false,
        }
    }

    /// Returns `true` if `id` is flagged "Access as Unique Name", i.e. whether
    /// its Scene Tree row should show the `%` badge.
    pub fn is_unique_name_in_owner(&self, id: NodeId) -> bool {
        self.get_node(id)
            .map(|n| n.is_unique_name())
            .unwrap_or(false)
    }

    /// Returns `true` if `id`'s unique name collides — i.e. `id` is flagged as
    /// a unique name and another node within the same owner scope is also
    /// flagged with the same name. Unique names must be unique per owner, so a
    /// duplicate is surfaced as a warning on the row. Returns `false` when `id`
    /// is not flagged or its unique name is the only one in scope.
    pub fn unique_name_collision(&self, id: NodeId) -> bool {
        let node = match self.get_node(id) {
            Some(n) if n.is_unique_name() => n,
            _ => return false,
        };
        let name = node.name();
        // Same owner scope the `%Name` lookup uses: the node's owner, or the
        // node itself when it has none (i.e. it is a scene root).
        let owner_id = node.owner().unwrap_or(id);

        let mut matches = 0usize;
        let mut stack = vec![owner_id];
        while let Some(cur) = stack.pop() {
            if let Some(n) = self.nodes.get(&cur) {
                if n.is_unique_name() && n.name() == name {
                    matches += 1;
                    if matches > 1 {
                        return true;
                    }
                }
                for &child in n.children().iter().rev() {
                    stack.push(child);
                }
            }
        }
        false
    }

    // -- group management ---------------------------------------------------

    /// Adds a node to a named group.
    pub fn add_to_group(&mut self, id: NodeId, group: &str) -> EngineResult<()> {
        let node = self
            .nodes
            .get_mut(&id)
            .ok_or_else(|| EngineError::NotFound(format!("node {id} not found")))?;
        node.add_to_group(group);
        self.groups.entry(group.to_owned()).or_default().insert(id);
        Ok(())
    }

    /// Removes a node from a named group.
    pub fn remove_from_group(&mut self, id: NodeId, group: &str) -> EngineResult<()> {
        let node = self
            .nodes
            .get_mut(&id)
            .ok_or_else(|| EngineError::NotFound(format!("node {id} not found")))?;
        node.remove_from_group(group);
        if let Some(members) = self.groups.get_mut(group) {
            members.remove(&id);
        }
        Ok(())
    }

    /// Returns all node IDs in the given group.
    pub fn get_nodes_in_group(&self, group: &str) -> Vec<NodeId> {
        self.groups
            .get(group)
            .map(|s| s.iter().copied().collect())
            .unwrap_or_default()
    }

    /// Returns `true` if `id` is a member of `group`.
    ///
    /// Convenience wrapper for the Scene Tree context-menu group editor, which
    /// queries membership per node rather than scanning a whole group.
    pub fn node_in_group(&self, id: NodeId, group: &str) -> bool {
        self.get_node(id)
            .map(|n| n.is_in_group(group))
            .unwrap_or(false)
    }

    /// Returns the groups `id` belongs to, sorted for stable presentation in
    /// the inspector / context menu. Empty when the node is in no groups.
    pub fn node_groups(&self, id: NodeId) -> Vec<String> {
        let mut groups: Vec<String> = self
            .get_node(id)
            .map(|n| n.groups().iter().cloned().collect())
            .unwrap_or_default();
        groups.sort();
        groups
    }

    /// Returns `true` if `id` belongs to at least one group, i.e. whether its
    /// Scene Tree row should show the group badge.
    pub fn has_group_membership(&self, id: NodeId) -> bool {
        self.get_node(id)
            .map(|n| !n.groups().is_empty())
            .unwrap_or(false)
    }

    // -- editor "group" (children-selection-lock) ---------------------------
    //
    // Distinct from named groups above: the Scene Tree "group" toggle marks a
    // node as a children-selection-lock root via the `_edit_group_` meta
    // (matching Godot). While set, the node shows the group badge and its
    // descendants cannot be picked individually in the viewport — a click on
    // any descendant selects the grouped ancestor instead.

    /// Meta key Godot uses to flag a node as a children-selection-lock root.
    const EDIT_GROUP_META: &'static str = "_edit_group_";

    /// Toggles the editor "group" flag on `id`. When `grouped` is true the
    /// `_edit_group_` meta is set (showing the badge and locking child
    /// selection); when false the meta is removed. Returns `true` if the node
    /// existed and was updated.
    pub fn set_edit_group(&mut self, id: NodeId, grouped: bool) -> bool {
        match self.get_node_mut(id) {
            Some(node) => {
                if grouped {
                    node.set_meta(Self::EDIT_GROUP_META, Variant::Bool(true));
                } else {
                    node.remove_meta(Self::EDIT_GROUP_META);
                }
                true
            }
            None => false,
        }
    }

    /// Returns `true` if `id` is flagged as a children-selection-lock root,
    /// i.e. whether its Scene Tree row should show the group badge.
    pub fn is_edit_group(&self, id: NodeId) -> bool {
        self.get_node(id)
            .map(|n| matches!(n.get_meta(Self::EDIT_GROUP_META), Variant::Bool(true)))
            .unwrap_or(false)
    }

    /// Resolves the node that should actually be selected when `id` is picked
    /// in the viewport, honouring the "group" lock. Walks from `id` to the
    /// root and returns the outermost grouped ancestor (the topmost lock wins,
    /// matching Godot); returns `id` unchanged when no ancestor is grouped.
    pub fn group_selection_target(&self, id: NodeId) -> NodeId {
        let mut target = id;
        let mut current = id;
        while let Some(parent) = self.get_node(current).and_then(|n| n.parent()) {
            if self.is_edit_group(parent) {
                target = parent;
            }
            current = parent;
        }
        target
    }

    // -- traversal helpers --------------------------------------------------

    /// Collects the subtree rooted at `id` in top-down (parent-first) order.
    pub fn collect_subtree_top_down(&self, id: NodeId, out: &mut Vec<NodeId>) {
        out.push(id);
        if let Some(node) = self.nodes.get(&id) {
            for &child in node.children() {
                self.collect_subtree_top_down(child, out);
            }
        }
    }

    /// Collects the subtree rooted at `id` in bottom-up (children-first) order.
    pub fn collect_subtree_bottom_up(&self, id: NodeId, out: &mut Vec<NodeId>) {
        if let Some(node) = self.nodes.get(&id) {
            for &child in node.children() {
                self.collect_subtree_bottom_up(child, out);
            }
        }
        out.push(id);
    }

    /// Returns IDs of all nodes in tree order (depth-first, top-down)
    /// starting from the root.
    pub fn all_nodes_in_tree_order(&self) -> Vec<NodeId> {
        let mut out = Vec::new();
        self.collect_subtree_top_down(self.root_id, &mut out);
        out
    }

    // -- pause state --------------------------------------------------------

    /// Returns whether the scene tree is currently paused.
    pub fn paused(&self) -> bool {
        self.paused
    }

    /// Sets the paused state of the scene tree.
    pub fn set_paused(&mut self, paused: bool) {
        self.paused = paused;
    }

    // -- process ordering and filtering -------------------------------------

    /// Returns IDs of all nodes sorted by process priority (stable sort,
    /// lower priority first), preserving tree order for equal priorities.
    pub fn all_nodes_in_process_order(&self) -> Vec<NodeId> {
        let mut ids = self.all_nodes_in_tree_order();
        ids.sort_by_key(|id| self.nodes.get(id).map_or(0, |n| n.process_priority()));
        ids
    }

    /// Resolves the effective [`ProcessMode`] for a node by walking up the
    /// parent chain until a non-`Inherit` mode is found. If the root node
    /// has `Inherit`, it is treated as `Pausable`.
    pub fn effective_process_mode(&self, node_id: NodeId) -> crate::node::ProcessMode {
        use crate::node::ProcessMode;
        let mut current = Some(node_id);
        while let Some(id) = current {
            if let Some(node) = self.nodes.get(&id) {
                if node.process_mode() != ProcessMode::Inherit {
                    return node.process_mode();
                }
                current = node.parent();
            } else {
                break;
            }
        }
        // Walked to root (or beyond) and everything was Inherit → Pausable.
        ProcessMode::Pausable
    }

    /// Returns `true` if the node should be processed in the current frame,
    /// taking the tree's pause state and the node's effective process mode
    /// into account.
    pub fn should_process_node(&self, node_id: NodeId) -> bool {
        use crate::node::ProcessMode;
        match self.effective_process_mode(node_id) {
            ProcessMode::Disabled => false,
            ProcessMode::Always => true,
            ProcessMode::Pausable => !self.paused,
            ProcessMode::WhenPaused => self.paused,
            ProcessMode::Inherit => {
                // Should never reach here — effective_process_mode resolves it.
                !self.paused
            }
        }
    }

    // -- signal management --------------------------------------------------

    /// Returns a reference to a node's signal store, if it exists.
    pub fn signal_store(&self, id: NodeId) -> Option<&SignalStore> {
        self.signal_stores.get(&id)
    }

    /// Returns all signal stores (node ID -> signal store).
    pub fn signal_stores(&self) -> &HashMap<NodeId, SignalStore> {
        &self.signal_stores
    }

    /// Returns a mutable reference to a node's signal store, creating it
    /// if it doesn't already exist.
    pub fn signal_store_mut(&mut self, id: NodeId) -> &mut SignalStore {
        self.signal_stores.entry(id).or_default()
    }

    /// Renames a node and emits Godot's `SceneTree.node_renamed` signal
    /// (pat-nzgdu.84). Returns `false` if `id` does not exist. A direct
    /// `Node::set_name` bypasses this and does not notify; runtime renames
    /// should route through here so connected scripts see `node_renamed`.
    pub fn set_node_name(&mut self, id: NodeId, name: impl Into<String>) -> bool {
        let Some(node) = self.get_node_mut(id) else {
            return false;
        };
        node.set_name(name);
        let root = self.root_id();
        self.emit_signal(root, "node_renamed", &[]);
        true
    }

    /// Connects a signal on the `source` node. The signal store for
    /// `source` is created lazily if needed.
    pub fn connect_signal(
        &mut self,
        source: NodeId,
        signal_name: &str,
        connection: gdobject::signal::Connection,
    ) {
        self.signal_store_mut(source)
            .connect(signal_name, connection);
    }

    /// Emits a signal on the given node. For each connection:
    /// - If the connection has a callback closure, invoke it directly.
    /// - If the connection has no callback (e.g. from a `.tscn` `[connection]`),
    ///   look up the target node's script and call the named method on it.
    ///
    /// Returns the collected return values from callback connections.
    pub fn emit_signal(
        &mut self,
        source: NodeId,
        signal_name: &str,
        args: &[gdvariant::Variant],
    ) -> Vec<gdvariant::Variant> {
        self.trace_record(source, TraceEventType::SignalEmit, signal_name);

        // Collect connection info to avoid borrow conflicts.
        // Note: we snapshot before emit() so one-shot connections are included.
        // Each entry also carries the connection's `deferred` flag and its
        // resolved argument list (binds appended / unbinds dropped) so the
        // script-method dispatch below honors CONNECT_DEFERRED and Callable
        // bind/unbind (pat-nzgdu.31).
        #[allow(clippy::type_complexity)]
        let connections: Vec<(
            gdcore::id::ObjectId,
            String,
            bool,
            bool,
            Vec<gdvariant::Variant>,
        )> = self
            .signal_stores
            .get(&source)
            .and_then(|store| store.get_signal(signal_name))
            .map(|signal| {
                signal
                    .connections()
                    .iter()
                    .map(|c| {
                        (
                            c.target_id,
                            c.method.clone(),
                            c.has_callback(),
                            c.deferred,
                            c.resolve_args(args),
                        )
                    })
                    .collect()
            })
            .unwrap_or_default();

        // Fire callback-based connections and remove one-shot connections.
        let results = self
            .signal_stores
            .get_mut(&source)
            .map_or_else(Vec::new, |store| store.emit(signal_name, args));

        // For connections without callbacks, dispatch to target node scripts.
        // Deferred connections are queued on the end-of-frame MessageQueue and
        // fire in `process_deferred_calls`; immediate ones fire now. Both use
        // the resolved args so `.bind()`/`.unbind()` payloads arrive correctly.
        for (target_id, method, has_callback, deferred, resolved) in connections {
            if !has_callback {
                let target_node_id = NodeId::from_object_id(target_id);
                if self.scripts.contains_key(&target_node_id) {
                    if deferred {
                        self.call_deferred(target_node_id, &method, &resolved);
                    } else {
                        self.call_script_with_access(target_node_id, &method, &resolved);
                    }
                }
            }
        }

        // Resume any coroutine on the emitting node parked at `await <signal>`,
        // binding the emitted argument(s) as the awaited value so
        // `var v = await my_signal` sees the emit's payload (pat-nzgdu.6).
        if self.scripts.contains_key(&source) {
            self.notify_script_await(source, signal_name, args);
        }
        // …and every OTHER script, because a cross-node `await other.sig` parks
        // in the AWAITING script's interpreter, which the emitter's own host
        // knows nothing about. Object-bound awaits still only wake for their own
        // source, so this is a broadcast of the offer, not of the resume
        // (pat-0q2kx.81).
        //
        // ⚠️ Deliberately outside the `contains_key(&source)` guard above: a
        // scriptless node emitting a signal (a native/engine emit) must still be
        // able to wake somebody else's `await`.
        let others: Vec<NodeId> = self
            .scripts
            .keys()
            .copied()
            .filter(|id| *id != source)
            .collect();
        for id in others {
            self.notify_script_await_from(id, source, signal_name, args);
        }

        results
    }

    // -- get_node_or_null ---------------------------------------------------

    /// Resolves a relative path from `from`, returning `None` instead of
    /// panicking when the path is invalid. This is the non-panicking
    /// counterpart to Godot's `get_node()`.
    pub fn get_node_or_null(&self, from: NodeId, path: &str) -> Option<NodeId> {
        if path.starts_with('/') {
            // Absolute path — delegate to get_node_by_path.
            self.get_node_by_path(path)
        } else {
            self.get_node_relative(from, path)
        }
    }

    // -- get_index ----------------------------------------------------------

    /// Returns the index of `node_id` among its parent's children, or `None`
    /// if the node has no parent or is not found.
    pub fn get_index(&self, node_id: NodeId) -> Option<usize> {
        let node = self.nodes.get(&node_id)?;
        let parent_id = node.parent()?;
        let parent = self.nodes.get(&parent_id)?;
        parent.children().iter().position(|&c| c == node_id)
    }

    // -- duplicate_subtree --------------------------------------------------

    /// Deep-clones a node and all its descendants, assigning fresh IDs.
    ///
    /// Returns the cloned nodes as a flat `Vec` with parent/child
    /// relationships already wired (using the new IDs). The first element
    /// is the clone of `root_id`. The cloned nodes are **not** inserted
    /// into the tree — the caller can add them via [`SceneTree::add_child`].
    /// Duplicates the subtree at `root_id` **into the tree**, honoring Godot's
    /// `Node.DUPLICATE_*` flags, and returns the new subtree's root id
    /// (pat-nzgdu.138).
    ///
    /// The copy is attached as a sibling of `root_id` (under the same parent),
    /// mirroring Godot's `duplicate()`. Properties and the unique-name flag are
    /// always copied; group membership only with
    /// [`DuplicateFlags::GROUPS`], and signal connections only with
    /// [`DuplicateFlags::SIGNALS`].
    ///
    /// Unlike [`duplicate_subtree`](Self::duplicate_subtree) — which returns
    /// detached nodes and therefore cannot carry signals, since connections
    /// live on the tree — this inserts the clones, so it can.
    ///
    /// [`DuplicateFlags::SCRIPTS`] is accepted but **not yet honored**:
    /// `ScriptInstance` is a trait object with no clone, so a script cannot be
    /// copied today (see the bead comment on pat-nzgdu.138).
    ///
    /// # Errors
    ///
    /// Returns an error when `root_id` is unknown, or is the scene tree's root
    /// (which has no parent to duplicate into).
    pub fn duplicate_node(
        &mut self,
        root_id: NodeId,
        flags: DuplicateFlags,
    ) -> EngineResult<NodeId> {
        if root_id == self.root_id {
            return Err(EngineError::InvalidOperation(
                "cannot duplicate the scene tree root".into(),
            ));
        }
        let target_parent = self
            .nodes
            .get(&root_id)
            .ok_or_else(|| EngineError::NotFound(format!("node {root_id} not found")))?
            .parent()
            .ok_or_else(|| {
                EngineError::InvalidOperation(format!("node {root_id} has no parent"))
            })?;

        let mut old_ids = Vec::new();
        self.collect_subtree_top_down(root_id, &mut old_ids);

        let mut old_to_new: HashMap<NodeId, NodeId> = HashMap::new();
        let mut new_root: Option<NodeId> = None;

        for &old_id in &old_ids {
            // Snapshot everything needed before taking a mutable borrow.
            let (clone, old_parent, groups) = {
                let original = self
                    .nodes
                    .get(&old_id)
                    .ok_or_else(|| EngineError::NotFound(format!("node {old_id} disappeared")))?;
                let mut clone = Node::new(original.name(), original.class_name());
                for (key, value) in original.properties() {
                    clone.set_property(key, value.clone());
                }
                clone.set_unique_name(original.is_unique_name());
                let groups: Vec<String> = original.groups().iter().cloned().collect();
                (clone, original.parent(), groups)
            };

            let parent_for_clone = if old_id == root_id {
                target_parent
            } else {
                match old_parent.and_then(|p| old_to_new.get(&p).copied()) {
                    Some(p) => p,
                    // Parent wasn't cloned (can't happen for a top-down walk).
                    None => continue,
                }
            };

            let new_id = self.add_child(parent_for_clone, clone)?;
            // Groups go through the tree so the group index stays in sync —
            // `add_child` does not index them.
            if flags.contains(DuplicateFlags::GROUPS) {
                for group in groups {
                    self.add_to_group(new_id, &group)?;
                }
            }
            old_to_new.insert(old_id, new_id);
            if old_id == root_id {
                new_root = Some(new_id);
            }
        }

        // Signal connections live on the tree, keyed by node.
        if flags.contains(DuplicateFlags::SIGNALS) {
            for (old_id, new_id) in &old_to_new {
                if let Some(store) = self.signal_stores.get(old_id).cloned() {
                    self.signal_stores.insert(*new_id, store);
                }
            }
        }

        new_root.ok_or_else(|| EngineError::NotFound(format!("node {root_id} not found")))
    }

    pub fn duplicate_subtree(&self, root_id: NodeId) -> EngineResult<Vec<Node>> {
        let _source = self
            .nodes
            .get(&root_id)
            .ok_or_else(|| EngineError::NotFound(format!("node {root_id} not found")))?;

        // Collect subtree in top-down order.
        let mut ids = Vec::new();
        self.collect_subtree_top_down(root_id, &mut ids);

        let mut old_to_new: HashMap<NodeId, NodeId> = HashMap::new();
        let mut cloned: Vec<Node> = Vec::with_capacity(ids.len());

        for &old_id in &ids {
            let original = self
                .nodes
                .get(&old_id)
                .ok_or_else(|| EngineError::NotFound(format!("node {old_id} disappeared")))?;

            let mut clone = Node::new(original.name(), original.class_name());
            // Copy properties.
            for (key, value) in original.properties() {
                clone.set_property(key, value.clone());
            }
            // Copy groups.
            for group in original.groups() {
                clone.add_to_group(group.clone());
            }
            // Copy unique_name flag.
            clone.set_unique_name(original.is_unique_name());

            let new_id = clone.id();
            old_to_new.insert(old_id, new_id);

            // Wire parent (skip the root of the subtree — it has no parent
            // in the cloned output).
            if old_id != root_id {
                if let Some(old_parent) = original.parent() {
                    if let Some(&new_parent) = old_to_new.get(&old_parent) {
                        clone.set_parent(Some(new_parent));
                        // Also add as child of the new parent.
                        if let Some(parent_clone) = cloned.iter_mut().find(|n| n.id() == new_parent)
                        {
                            parent_clone.add_child_id(new_id);
                        }
                    }
                }
            }

            cloned.push(clone);
        }

        Ok(cloned)
    }

    // -- animation player management ----------------------------------------

    /// Attaches an [`AnimationPlayer`] to a node.
    pub fn attach_animation_player(
        &mut self,
        node_id: NodeId,
        player: AnimationPlayer,
    ) -> EngineResult<()> {
        if !self.nodes.contains_key(&node_id) {
            return Err(EngineError::NotFound(format!("node {node_id} not found")));
        }
        self.animation_players.insert(node_id, player);
        Ok(())
    }

    /// The loop phase that advances `node_id`'s animation player, read from the
    /// node's `callback_mode_process` property.
    ///
    /// Absent or non-integer means Godot's default, `IDLE` — the same value the
    /// ClassDB registers for `AnimationMixer`.
    pub fn animation_callback_mode(&self, node_id: NodeId) -> AnimationCallbackMode {
        match self
            .nodes
            .get(&node_id)
            .map(|n| n.get_property("callback_mode_process"))
        {
            Some(Variant::Int(v)) => AnimationCallbackMode::from_godot_int(v),
            _ => AnimationCallbackMode::Idle,
        }
    }

    /// Returns a reference to a node's animation player, if attached.
    pub fn get_animation_player(&self, node_id: NodeId) -> Option<&AnimationPlayer> {
        self.animation_players.get(&node_id)
    }

    /// Returns a mutable reference to a node's animation player, if attached.
    pub fn get_animation_player_mut(&mut self, node_id: NodeId) -> Option<&mut AnimationPlayer> {
        self.animation_players.get_mut(&node_id)
    }

    /// Advances the **idle-phase** animation players by `delta` seconds — the
    /// Godot default (`callback_mode_process = IDLE`).
    ///
    /// See [`process_animations_in_mode`](Self::process_animations_in_mode) for
    /// what advancing does; physics-mode players are advanced from the physics
    /// tick instead, and manual-mode players are never advanced by the loop.
    pub fn process_animations(&mut self, delta: f64) {
        self.process_animations_in_mode(delta, AnimationCallbackMode::Idle);
    }

    /// Advances every animation player whose owner node's
    /// `callback_mode_process` selects `mode`, and applies the sampled results
    /// to their nodes: property tracks set node properties, method tracks invoke
    /// the owner node's script method as the playhead crosses each keyframe, and
    /// a configured root-motion track is applied as a transform delta rather
    /// than an absolute property.
    ///
    /// Godot's `AnimationMixer` picks its phase from `callback_mode_process`
    /// (`animation_mixer.cpp`): `PHYSICS` advances inside `_physics_process` so
    /// an animation driving a body stays in lockstep with the solver, `IDLE`
    /// (the default) advances in `_process`, and `MANUAL` never advances on its
    /// own. Every player used to advance in the idle phase regardless
    /// (pat-0q2kx.1).
    pub fn process_animations_in_mode(&mut self, delta: f64, mode: AnimationCallbackMode) {
        // Collect node_ids to avoid borrowing self twice.
        let node_ids: Vec<NodeId> = self
            .animation_players
            .keys()
            .copied()
            .filter(|id| self.animation_callback_mode(*id) == mode)
            .collect();
        for node_id in node_ids {
            // Advance the player and gather everything we need while only the
            // player is borrowed, so we can then mutate nodes and scripts.
            let (values, skip, method_calls, root_motion, root_node, signals, current) =
                match self.animation_players.get_mut(&node_id) {
                    Some(player) => {
                        let prev = player.position();
                        player.advance(delta);
                        let curr = player.position();
                        // (node_path, property_path, value) triples so each track
                        // can target a different node relative to root_node
                        // instead of always writing to the player (pat-u3l8c.94).
                        let values = player.get_current_values_targeted();
                        let skip: std::collections::HashSet<String> =
                            player.non_property_track_paths().into_iter().collect();
                        let method_calls = player.method_calls_in_range(prev, curr);
                        let root_motion = player.consume_root_motion(prev, curr);
                        let root_node = player.root_node.clone();
                        // Playback signals queued by `advance` (and by the
                        // autoplay `play` at scene load) — re-emitted below as
                        // real node signals so `.tscn` `[connection]`s and
                        // script `connect` calls fire (pat-0q2kx.1).
                        let signals = player.drain_signals();
                        // Godot exposes the playing clip as the node's
                        // `current_animation` property; keep it in sync so a
                        // script reading `$AnimationPlayer.current_animation`
                        // sees the truth rather than the .tscn's stale value.
                        let current = player.current_animation().unwrap_or("").to_string();
                        (
                            values,
                            skip,
                            method_calls,
                            root_motion,
                            root_node,
                            signals,
                            current,
                        )
                    }
                    None => continue,
                };

            if let Some(node) = self.nodes.get_mut(&node_id) {
                node.set_property("current_animation", Variant::String(current));
            }

            // Base node that track `node_path`s resolve against: the player's
            // `root_node` from the owner node (default "." = the owner itself,
            // matching Godot's root_node resolution).
            let base_node = if root_node.is_empty() || root_node == "." {
                node_id
            } else {
                self.get_node_or_null(node_id, &root_node)
                    .unwrap_or(node_id)
            };

            // Apply ordinary property tracks to their resolved target nodes,
            // skipping method and root-motion tracks (handled below).
            for (track_node_path, path, value) in values {
                if skip.contains(&path) {
                    continue;
                }
                let target = if track_node_path.is_empty() || track_node_path == "." {
                    base_node
                } else {
                    match self.get_node_or_null(base_node, &track_node_path) {
                        Some(t) => t,
                        None => continue,
                    }
                };
                if let Some(node) = self.nodes.get_mut(&target) {
                    node.set_property(&path, value);
                }
            }

            // Root motion accumulates onto the resolved root node — the node the
            // mixer drives (`root_node` relative to the player owner). Godot
            // applies root motion in the space of the AnimationMixer root, so a
            // non-default `root_node` moves that target rather than the player.
            // With the default root_node "." this is the player's owner node.
            if let Some(node) = self.nodes.get_mut(&base_node) {
                apply_root_motion_delta(node, &root_motion);
            }

            // Dispatch each method-track call to the node named by the track's
            // `node_path`, resolved relative to `base_node` exactly like property
            // tracks. In Godot a method track can invoke a method on any node in
            // the tree, not just the player; an empty/"." path targets the root
            // node (the pre-existing behavior).
            for call in method_calls {
                let target = if call.node_path.is_empty() || call.node_path == "." {
                    base_node
                } else {
                    match self.get_node_or_null(base_node, &call.node_path) {
                        Some(t) => t,
                        None => continue,
                    }
                };
                self.call_script_with_access(target, &call.method, &call.args);
            }

            // Re-emit playback signals on the player node itself, mirroring
            // Godot's `AnimationMixer` signal set, so a GDScript handler
            // connected in the scene (or via `connect`) actually runs. Emitted
            // last so a handler observes the frame's sampled values.
            for signal in signals {
                match signal {
                    AnimationPlayerSignal::AnimationStarted(name) => {
                        self.emit_signal(node_id, "animation_started", &[Variant::String(name)]);
                    }
                    AnimationPlayerSignal::AnimationFinished(name) => {
                        self.emit_signal(node_id, "animation_finished", &[Variant::String(name)]);
                    }
                    AnimationPlayerSignal::AnimationChanged { old, new } => {
                        self.emit_signal(
                            node_id,
                            "animation_changed",
                            &[Variant::String(old), Variant::String(new)],
                        );
                    }
                }
            }
        }
    }

    /// Advances every `AnimatedSprite2D`'s frame by `delta` seconds.
    ///
    /// Godot's `AnimatedSprite2D::_process_animation` walks the node's
    /// `SpriteFrames` at the animation's FPS, wrapping when it loops and
    /// emitting `animation_finished` once when it does not
    /// (`scene/2d/animated_sprite_2d.cpp`). Patina had the state machine
    /// ([`AnimatedSpritePlayback`]) but nothing ever called it — an
    /// `AnimatedSprite2D` sat on frame 0 forever no matter how long the game
    /// ran (pat-0q2kx.74).
    ///
    /// Playback state lives in the node's own property bag (`frame`, `playing`,
    /// `speed_scale` are Godot's; `__sf_time` accumulates sub-frame time) so a
    /// node added mid-run needs no registration step, and the frame index a
    /// script reads back is the one the renderer draws.
    pub fn process_animated_sprites(&mut self, delta: f64) {
        let node_ids: Vec<NodeId> = self.nodes.keys().copied().collect();
        for node_id in node_ids {
            let Some(node) = self.nodes.get(&node_id) else {
                continue;
            };
            if node.class_name() != "AnimatedSprite2D" {
                continue;
            }
            // `playing` defaults to FALSE, matching Godot: a sprite only
            // animates once `play()` — or `autoplay` — has started it. Autoplay
            // fires once, on the first frame the node is processed, and is
            // recorded so stopping a non-looping clip is not undone next frame.
            let mut autostart: Option<String> = None;
            if !matches!(node.get_property("playing"), Variant::Bool(true)) {
                let auto = match node.get_property("autoplay") {
                    Variant::String(s) if !s.is_empty() => s,
                    _ => continue,
                };
                if matches!(node.get_property("__sf_autoplayed"), Variant::Bool(true)) {
                    continue;
                }
                autostart = Some(auto);
            }
            let Some(frames) = sprite_frames_from_node(node) else {
                continue;
            };
            let anim = match autostart.clone() {
                Some(a) => a,
                None => match node.get_property("animation") {
                    Variant::String(s) if !s.is_empty() => s,
                    // Godot's default animation name.
                    _ => "default".to_string(),
                },
            };
            let mut playback = AnimatedSpritePlayback::new(&anim);
            playback.frame = match node.get_property("frame") {
                Variant::Int(i) if i >= 0 => i as usize,
                _ => 0,
            };
            playback.time = match node.get_property("__sf_time") {
                Variant::Float(f) => f as f32,
                _ => 0.0,
            };
            playback.speed_scale = match node.get_property("speed_scale") {
                Variant::Float(f) => f as f32,
                Variant::Int(i) => i as f32,
                _ => 1.0,
            };

            let finished = playback.advance(delta as f32, &frames);

            if let Some(node) = self.nodes.get_mut(&node_id) {
                if let Some(a) = autostart {
                    node.set_property("animation", Variant::String(a));
                    node.set_property("__sf_autoplayed", Variant::Bool(true));
                }
                node.set_property("frame", Variant::Int(playback.frame as i64));
                node.set_property("__sf_time", Variant::Float(playback.time as f64));
                // A non-looping animation stops on its last frame; Godot leaves
                // `playing` false so a later `play()` is what restarts it.
                node.set_property("playing", Variant::Bool(playback.playing));
            }
            if finished {
                self.emit_signal(node_id, "animation_finished", &[]);
            }
        }
    }

    // -- animation tree management ------------------------------------------

    /// Attaches an [`AnimationTree`] to a node (Godot's `AnimationTree` node),
    /// so [`process_animation_trees`](Self::process_animation_trees) drives it
    /// every frame. Mirrors [`attach_animation_player`](Self::attach_animation_player).
    pub fn attach_animation_tree(
        &mut self,
        node_id: NodeId,
        anim_tree: AnimationTree,
    ) -> EngineResult<()> {
        if !self.nodes.contains_key(&node_id) {
            return Err(EngineError::NotFound(format!("node {node_id} not found")));
        }
        self.animation_trees.insert(node_id, anim_tree);
        Ok(())
    }

    /// Returns a reference to a node's animation tree, if attached.
    pub fn get_animation_tree(&self, node_id: NodeId) -> Option<&AnimationTree> {
        self.animation_trees.get(&node_id)
    }

    /// Returns a mutable reference to a node's animation tree, if attached.
    /// Scripts reach this to set `parameters/...` entries at runtime.
    pub fn get_animation_tree_mut(&mut self, node_id: NodeId) -> Option<&mut AnimationTree> {
        self.animation_trees.get_mut(&node_id)
    }

    /// Number of animation trees currently attached.
    pub fn animation_tree_count(&self) -> usize {
        self.animation_trees.len()
    }

    /// Advances every attached [`AnimationTree`] by `delta` seconds and writes
    /// the blended pose onto the nodes its tracks target.
    ///
    /// Each tree's tracks resolve against its
    /// [`root_node`](AnimationTree::root_node) — a path relative to the node the
    /// tree is attached to, defaulting to Godot's `".."` (the tree node's
    /// parent) — and then each track's own node path relative to that. Inactive
    /// trees (`active == false`) are skipped, as in Godot.
    pub fn process_animation_trees(&mut self, delta: f64) {
        // Snapshot the ids so nodes can be mutated while iterating.
        let node_ids: Vec<NodeId> = self.animation_trees.keys().copied().collect();
        for node_id in node_ids {
            let (values, root_node) = match self.animation_trees.get_mut(&node_id) {
                Some(anim_tree) if anim_tree.active => {
                    anim_tree.advance(delta);
                    (
                        anim_tree.get_current_values_targeted(),
                        anim_tree.root_node.clone(),
                    )
                }
                _ => continue,
            };

            let base_node = if root_node.is_empty() || root_node == "." {
                node_id
            } else {
                // A tree whose root_node does not resolve (e.g. the scene root
                // itself, whose ".." is outside the scene) falls back to the
                // owner node rather than dropping the frame's pose.
                self.get_node_or_null(node_id, &root_node)
                    .unwrap_or(node_id)
            };

            for (track_node_path, path, value) in values {
                let target = if track_node_path.is_empty() || track_node_path == "." {
                    base_node
                } else {
                    match self.get_node_or_null(base_node, &track_node_path) {
                        Some(t) => t,
                        None => continue,
                    }
                };
                if let Some(node) = self.nodes.get_mut(&target) {
                    node.set_property(&path, value);
                }
            }
        }
    }

    // -- tween management ---------------------------------------------------

    /// Creates a new tween associated with a node. Returns the [`TweenId`].
    ///
    /// The tween is stored but not yet started — call
    /// [`get_tween_mut`](Self::get_tween_mut) to configure and start it.
    pub fn create_tween(&mut self, node_id: NodeId) -> TweenId {
        let id = TweenId::next();
        self.tweens.insert(id, (node_id, Tween::new()));
        id
    }

    /// Inserts an already-configured tween for the given node.
    pub fn add_tween(&mut self, node_id: NodeId, tween: Tween) -> TweenId {
        let id = TweenId::next();
        self.tweens.insert(id, (node_id, tween));
        id
    }

    /// Returns a mutable reference to a tween by ID.
    pub fn get_tween_mut(&mut self, tween_id: TweenId) -> Option<&mut Tween> {
        self.tweens.get_mut(&tween_id).map(|(_, t)| t)
    }

    /// Returns the number of active tweens.
    pub fn tween_count(&self) -> usize {
        self.tweens.len()
    }

    /// Advances all tweens by `delta` seconds, applies interpolated values
    /// to the corresponding node properties, dispatches `tween_method` /
    /// `tween_callback` to the owning node's script, emits `finished`, and
    /// removes completed non-looping tweens.
    ///
    /// Before pat-0q2kx.4 this only did the property writes: the engine
    /// computed [`method_values`](Tween::method_values) /
    /// [`poll_callbacks`](Tween::poll_callbacks) /
    /// [`take_just_finished`](Tween::take_just_finished) and *nothing consumed
    /// them*, so `tween_callback`, `tween_method` and `finished` silently never
    /// fired — including from Rust.
    pub fn process_tweens(&mut self, delta: f64) {
        let mut completed = Vec::new();
        // Collected while the `self.tweens` borrow is live and dispatched after
        // it ends: `call_script_with_access` / `emit_signal` take `&mut self`,
        // and a script re-entered here may itself create or kill tweens.
        let mut method_calls: Vec<(NodeId, String, Variant)> = Vec::new();
        let mut callbacks: Vec<(NodeId, String)> = Vec::new();
        let mut finished: Vec<(TweenId, NodeId)> = Vec::new();

        let tween_ids: Vec<TweenId> = self.tweens.keys().copied().collect();
        for tween_id in tween_ids {
            if let Some((node_id, tween)) = self.tweens.get_mut(&tween_id) {
                let owner = *node_id;
                let done = tween.advance(delta);

                // Godot's `tween_property(object, ...)` names the target
                // explicitly, so a step may write to a node other than the one
                // that created the tween; `None` keeps the legacy "owning node"
                // behaviour.
                for (target, path, value) in tween.get_current_values_targeted() {
                    let target_id = match target {
                        Some(raw) => NodeId::from_object_id(gdcore::id::ObjectId::from_raw(raw)),
                        None => owner,
                    };
                    if let Some(node) = self.nodes.get_mut(&target_id) {
                        // `"position:x"` addresses the X of a Vector2, not a
                        // property literally named `position:x` — the property
                        // bag is flat, so decompose before writing.
                        match crate::tween::split_property_path(&path) {
                            (prop, Some(sub)) => {
                                let base = node.get_property(prop);
                                if let Some(updated) =
                                    crate::tween::write_subname(&base, sub, &value)
                                {
                                    node.set_property(prop, updated);
                                }
                            }
                            (prop, None) => {
                                node.set_property(prop, value);
                            }
                        }
                    }
                }

                for (method, value) in tween.method_values() {
                    method_calls.push((owner, method, value));
                }
                for method in tween.poll_callbacks() {
                    callbacks.push((owner, method));
                }
                if tween.take_just_finished() {
                    finished.push((tween_id, owner));
                }
                if done {
                    completed.push(tween_id);
                }
            }
        }

        // The `self.tweens` borrow has ended — safe to re-enter scripts.
        for (node_id, method, value) in method_calls {
            self.call_script_with_access(node_id, &method, &[value]);
        }
        for (node_id, method) in callbacks {
            self.call_script_with_access(node_id, &method, &[]);
        }
        for (tween_id, owner) in finished {
            // Godot emits `finished` on the *Tween object*, not on the node, so
            // `tween.finished.connect(cb)` resolves against the tween's own id.
            // Emitting it on the owner instead would spuriously fire an
            // AudioStreamPlayer's real `finished` signal on the same node.
            let tween_as_source =
                NodeId::from_object_id(gdcore::id::ObjectId::from_raw(tween_id.raw()));
            self.emit_signal(tween_as_source, "finished", &[]);
            // `emit_signal` only resumes coroutines parked on the *emitting*
            // node, and a tween has no script — so resume the owner separately
            // for `await tween.finished`. This wakes parked coroutines only; it
            // does not fire the owner's own `finished` connections.
            self.notify_script_await(owner, "finished", &[]);
        }

        for id in completed {
            self.tweens.remove(&id);
        }
    }

    // -- script store -------------------------------------------------------

    /// Attaches a script instance to a node.
    pub fn attach_script(&mut self, node_id: NodeId, mut script: Box<dyn ScriptInstance>) {
        // Godot applies a scene's stored node properties through `Object::set` as
        // it instantiates (packed_scene.cpp `SceneState::instantiate`), which
        // routes each exported value into the script instance and runs that
        // property's setter (gdscript.cpp `GDScriptInstance::set`). The `.tscn`
        // values are already on this node's property bag by the time a script is
        // attached, so replay them into the fresh instance — otherwise
        // `@export var speed := 200.0` with a scene override of `speed = 300.0`
        // would run with 200, and any setter the property declares would never
        // fire (pat-0q2kx.17).
        //
        // Doing it here rather than at each attach site is deliberate: the runner's
        // boot path, the runner's main path and the editor's Play path all funnel
        // through this method, so none of them can forget.
        if let Some(node) = self.nodes.get(&node_id) {
            let overrides: Vec<(String, Variant)> = script
                .list_properties()
                .into_iter()
                .filter(|info| node.has_property(&info.name))
                .map(|info| {
                    let value = node.get_property(&info.name);
                    (info.name, value)
                })
                .collect();
            for (name, value) in overrides {
                script.set_property(&name, value);
            }
        }
        // Godot configures a node's RPCs from the script's `@rpc` annotations at
        // load time. Doing it here covers every attach site — the runner's boot
        // and main paths and the editor's Play path all funnel through this
        // function (pat-0q2kx.79).
        let rpcs = script.rpc_methods();
        if !rpcs.is_empty() {
            self.register_script_rpcs(&rpcs);
        }
        self.scripts.insert(node_id, script);
    }

    /// Detaches the script from a node, if any.
    pub fn detach_script(&mut self, node_id: NodeId) -> Option<Box<dyn ScriptInstance>> {
        self.scripts.remove(&node_id)
    }

    /// Returns `true` if the node has an attached script.
    pub fn has_script(&self, node_id: NodeId) -> bool {
        self.scripts.contains_key(&node_id)
    }

    /// Returns a reference to the node's script instance.
    pub fn get_script(&self, node_id: NodeId) -> Option<&dyn ScriptInstance> {
        self.scripts.get(&node_id).map(|s| s.as_ref())
    }

    /// Returns a mutable reference to the node's script instance.
    pub fn get_script_mut(&mut self, node_id: NodeId) -> Option<&mut Box<dyn ScriptInstance>> {
        self.scripts.get_mut(&node_id)
    }

    // -- configuration-warning indicator ------------------------------------

    /// Returns the node's configuration-warning messages by invoking its
    /// `_get_configuration_warnings` script method. Returns an empty list when
    /// the node has no script, the method is absent, or it reports no warnings.
    /// Drives the Scene Tree warning triangle and its hover tooltip. Accepts
    /// either a `String` array (Godot's `PackedStringArray`) or a single
    /// `String`; empty strings are ignored.
    pub fn node_configuration_warnings(&mut self, node_id: NodeId) -> Vec<String> {
        let script = match self.scripts.get_mut(&node_id) {
            Some(s) => s,
            None => return Vec::new(),
        };
        match script.call_method("_get_configuration_warnings", &[]) {
            Ok(Variant::Array(items)) => items
                .into_iter()
                .filter_map(|v| match v {
                    Variant::String(s) if !s.is_empty() => Some(s),
                    _ => None,
                })
                .collect(),
            Ok(Variant::String(s)) if !s.is_empty() => vec![s],
            _ => Vec::new(),
        }
    }

    /// Returns `true` if `node_id` reports at least one configuration warning,
    /// i.e. whether its Scene Tree row should show the warning triangle.
    pub fn has_configuration_warning(&mut self, node_id: NodeId) -> bool {
        !self.node_configuration_warnings(node_id).is_empty()
    }

    /// Records a trace event for the given node, if tracing is enabled.
    pub fn trace_record(&mut self, node_id: NodeId, event_type: TraceEventType, detail: &str) {
        if self.event_trace.is_enabled() {
            let path = self
                .node_path(node_id)
                .unwrap_or_else(|| format!("<{node_id}>"));
            self.event_trace.record(TraceEvent {
                event_type,
                node_path: path,
                detail: detail.to_string(),
                frame: self.trace_frame,
            });
        }
    }

    // -- script callbacks ---------------------------------------------------

    /// Temporarily extracts a script, gives it scene-tree access via
    /// [`SceneTreeAccessor`], calls the named method, then re-inserts the
    /// script. This allows the running script to call back into the tree
    /// (e.g. `get_node`, `emit_signal`) without violating borrow rules.
    pub(crate) fn call_script_with_access(
        &mut self,
        node_id: NodeId,
        method: &str,
        args: &[Variant],
    ) {
        let _ = self.call_script_method(node_id, method, args);
    }

    /// Calls `method` on the script attached to `node_id` and returns its value.
    ///
    /// This is Godot's `Object::call` reaching a scripted object
    /// (upstream/godot/core/object/object.cpp `Object::callv`): the body runs
    /// synchronously on the **target's** script — not the caller's — the target
    /// can reach back into the tree through [`crate::scripting::SceneTreeAccessor`]
    /// while it runs, and its script variables are synced onto the node
    /// afterwards so the caller can read them as node properties.
    ///
    /// Returns `None` when the node has no script, the script does not define
    /// `method`, or the call raised. Skipping an undefined method is what lets
    /// lifecycle dispatch call `_ready`/`_process` on every node blindly.
    pub fn call_script_method(
        &mut self,
        node_id: NodeId,
        method: &str,
        args: &[Variant],
    ) -> Option<Variant> {
        let mut result = None;
        // The script is taken out of the arena for the duration of the call so a
        // re-entrant call — the target calling back into this node, or into
        // another script that reaches this one — cannot alias it.
        if let Some(mut script) = self.scripts.remove(&node_id) {
            // Skip methods the script doesn't define, matching Godot which only
            // dispatches lifecycle callbacks to scripts that actually override them.
            if !script.has_method(method) {
                self.scripts.insert(node_id, script);
                return None;
            }
            self.trace_record(node_id, TraceEventType::ScriptCall, method);

            let snapshot_clone = self.input_snapshot.clone();
            // SAFETY: We pass `self` as a raw pointer to SceneTreeAccessor. This is
            // safe because the accessor is used only within this call frame — it is
            // created, passed to the script, and cleared (via `clear_scene_access`)
            // before this method returns, so no dangling pointer can escape.
            let accessor = if let Some(snapshot) = snapshot_clone {
                unsafe {
                    crate::scripting::SceneTreeAccessor::with_input(
                        self as *mut SceneTree,
                        snapshot,
                    )
                }
            } else {
                // SAFETY: Same invariant as the branch above — accessor lives only
                // within this call frame and is cleared before return.
                unsafe { crate::scripting::SceneTreeAccessor::new(self as *mut SceneTree) }
            };
            script.set_scene_access(Box::new(accessor), node_id.raw());
            match script.call_method(method, args) {
                Ok(value) => result = Some(value),
                Err(err) => {
                    tracing::debug!("script call {method} on node {node_id} failed: {err}")
                }
            }
            script.clear_scene_access();

            self.trace_record(node_id, TraceEventType::ScriptReturn, method);

            // Sync script variables to node properties so they are visible
            // as first-class node properties (matching Godot behavior).
            // Collect current values first to avoid borrow conflicts.
            let prop_values: Vec<(String, Variant)> = script
                .list_properties()
                .into_iter()
                .filter_map(|info| script.get_property(&info.name).map(|v| (info.name, v)))
                .collect();
            if let Some(node) = self.nodes.get_mut(&node_id) {
                for (name, val) in prop_values {
                    node.set_property(&name, val);
                }
            }

            self.scripts.insert(node_id, script);
        }
        result
    }

    /// Whether `node_id` responds to `method`: its script's methods first, then
    /// its class's registered methods. Mirrors Godot's `Object.has_method`, which
    /// reports script and native methods alike.
    pub fn node_has_method(&self, node_id: NodeId, method: &str) -> bool {
        if let Some(script) = self.scripts.get(&node_id) {
            if script.has_method(method) {
                return true;
            }
        }
        match self.nodes.get(&node_id) {
            Some(node) => gdobject::class_db::class_has_method(node.class_name(), method),
            None => false,
        }
    }

    /// Declared parameter count of `method` on `node_id`'s script, or `None` when
    /// the node has no script or the script does not define `method` (Godot
    /// `Object.get_method_argument_count`).
    pub fn node_method_argument_count(&self, node_id: NodeId, method: &str) -> Option<i64> {
        let script = self.scripts.get(&node_id)?;
        script
            .list_methods()
            .into_iter()
            .find(|info| info.name == method)
            .map(|info| info.argument_names.len() as i64)
    }

    /// Delivers an emitted signal to a coroutine on `node_id` parked at
    /// `await <signal_name>`, binding the emitted argument(s) as the resumed
    /// value (pat-nzgdu.6). Mirrors [`call_script_with_access`]'s borrow-safe
    /// dance: the script is removed while it runs so a resumed body can call
    /// back into the tree via `SceneTreeAccessor` without aliasing `self`.
    fn notify_script_await(&mut self, node_id: NodeId, signal_name: &str, args: &[Variant]) {
        // The emitter is its own source in the single-node case.
        self.notify_script_await_from(node_id, node_id, signal_name, args);
    }

    /// [`notify_script_await`] where the parked coroutine and the EMITTER are
    /// different nodes.
    ///
    /// ⚠️ A coroutine parked at `await other.sig` lives in the AWAITING script's
    /// own interpreter — each `GDScriptNodeInstance` owns its `CoroutineHost` —
    /// so it is not reachable from the emitter's script at all. `source` is
    /// carried separately so the host's `(name, object_id)` match still rejects
    /// a same-named signal from a different object (pat-0q2kx.81).
    fn notify_script_await_from(
        &mut self,
        script_node: NodeId,
        source: NodeId,
        signal_name: &str,
        args: &[Variant],
    ) {
        let node_id = script_node;
        if let Some(mut script) = self.scripts.remove(&node_id) {
            let snapshot_clone = self.input_snapshot.clone();
            // SAFETY: identical invariant to `call_script_with_access` — the
            // accessor lives only within this call frame and is cleared before
            // the script is re-inserted, so no dangling pointer escapes.
            let accessor = if let Some(snapshot) = snapshot_clone {
                unsafe {
                    crate::scripting::SceneTreeAccessor::with_input(
                        self as *mut SceneTree,
                        snapshot,
                    )
                }
            } else {
                unsafe { crate::scripting::SceneTreeAccessor::new(self as *mut SceneTree) }
            };
            script.set_scene_access(Box::new(accessor), node_id.raw());
            // `node_id` is also the emitter here: `emit_signal` passes its own
            // `source` as the node to notify. Carrying it lets an object-bound
            // `await` reject another node's same-named signal (pat-0q2kx.9).
            script.notify_await_signal_from(source.raw(), signal_name, args);
            script.clear_scene_access();

            // Sync any variables a resumed coroutine wrote back to node
            // properties, matching `call_script_with_access`.
            let prop_values: Vec<(String, Variant)> = script
                .list_properties()
                .into_iter()
                .filter_map(|info| script.get_property(&info.name).map(|v| (info.name, v)))
                .collect();
            if let Some(node) = self.nodes.get_mut(&node_id) {
                for (name, val) in prop_values {
                    node.set_property(&name, val);
                }
            }

            self.scripts.insert(node_id, script);
        }
    }

    /// Delivers engine notification `what` (a Godot `NOTIFICATION_*` code) to
    /// the script on `node_id` via its `_notification(what)` virtual, so user
    /// code sees ENTER_TREE/READY/PAUSED/PROCESS/PREDELETE etc. (pat-nzgdu.13).
    /// Mirrors [`call_script_with_access`]'s borrow-safe accessor dance: the
    /// script is removed while it runs so `_notification` can call back into the
    /// tree, then its property writes are synced to node properties.
    pub fn notify_script(&mut self, node_id: NodeId, what: i64) {
        if let Some(mut script) = self.scripts.remove(&node_id) {
            let snapshot_clone = self.input_snapshot.clone();
            // SAFETY: identical invariant to `call_script_with_access` — the
            // accessor lives only within this call frame and is cleared before
            // the script is re-inserted, so no dangling pointer escapes.
            let accessor = if let Some(snapshot) = snapshot_clone {
                unsafe {
                    crate::scripting::SceneTreeAccessor::with_input(
                        self as *mut SceneTree,
                        snapshot,
                    )
                }
            } else {
                unsafe { crate::scripting::SceneTreeAccessor::new(self as *mut SceneTree) }
            };
            script.set_scene_access(Box::new(accessor), node_id.raw());
            script.notification(what);
            script.clear_scene_access();

            // Sync any variables `_notification` wrote back to node properties.
            let prop_values: Vec<(String, Variant)> = script
                .list_properties()
                .into_iter()
                .filter_map(|info| script.get_property(&info.name).map(|v| (info.name, v)))
                .collect();
            if let Some(node) = self.nodes.get_mut(&node_id) {
                for (name, val) in prop_values {
                    node.set_property(&name, val);
                }
            }

            self.scripts.insert(node_id, script);
        }
    }

    /// Calls `_ready()` on the node's script, if present.
    ///
    /// Before `_ready` executes, any `@onready` variables are resolved
    /// by evaluating their default expressions.
    pub fn process_script_ready(&mut self, node_id: NodeId) {
        // Resolve @onready vars before _ready (even if _ready is not defined).
        if let Some(script) = self.scripts.get_mut(&node_id) {
            let _ = script.resolve_onready();
        }
        self.call_script_with_access(node_id, "_ready", &[]);
    }

    /// Calls `_process(delta)` on the node's script, if present.
    pub fn process_script_process(&mut self, node_id: NodeId, delta: f64) {
        self.call_script_with_access(node_id, "_process", &[Variant::Float(delta)]);
    }

    /// Calls `_physics_process(delta)` on the node's script, if present.
    pub fn process_script_physics_process(&mut self, node_id: NodeId, delta: f64) {
        self.call_script_with_access(node_id, "_physics_process", &[Variant::Float(delta)]);
    }

    // -- input propagation (pat-nzgdu.83) -----------------------------------

    /// Returns whether the input event currently being dispatched has been
    /// marked handled (Godot: `Viewport.is_input_handled()`).
    pub fn is_input_handled(&self) -> bool {
        self.input_handled
    }

    /// Marks the input event currently being dispatched as handled, stopping it
    /// from reaching any later propagation stage (Godot:
    /// `Viewport.set_input_as_handled()`).
    pub fn set_input_as_handled(&mut self) {
        self.input_handled = true;
    }

    /// Calls `_input(event)` on the node's script, if present.
    pub fn process_script_input(&mut self, node_id: NodeId, event: &Variant) {
        self.call_script_with_access(node_id, "_input", std::slice::from_ref(event));
    }

    /// Calls `_unhandled_input(event)` on the node's script, if present.
    pub fn process_script_unhandled_input(&mut self, node_id: NodeId, event: &Variant) {
        self.call_script_with_access(node_id, "_unhandled_input", std::slice::from_ref(event));
    }

    /// Routes `event` through Godot's input propagation stages in order:
    /// `_input` → `_shortcut_input` → `_unhandled_key_input` → `_unhandled_input`,
    /// walking nodes in tree order within each stage.
    ///
    /// A script calling `set_input_as_handled()` short-circuits the rest of the
    /// walk, so later stages (and later nodes) never see the event — this is how
    /// a node's `_input` suppresses a sibling's `_unhandled_input`.
    /// `_unhandled_key_input` only runs for key events, matching Godot.
    ///
    /// Scripts that do not define a stage's method are skipped by
    /// [`call_script_with_access`], so no opt-in flag is required for them.
    pub fn push_input(&mut self, event: &gdplatform::input::InputEvent) {
        let payload = input_event_to_variant(event);
        let is_key = matches!(event, gdplatform::input::InputEvent::Key { .. });
        // Each event starts unhandled; `set_input_as_handled()` sets the flag.
        self.input_handled = false;

        let stages: &[&str] = if is_key {
            &[
                "_input",
                "_shortcut_input",
                "_unhandled_key_input",
                "_unhandled_input",
            ]
        } else {
            &["_input", "_shortcut_input", "_unhandled_input"]
        };

        // Godot's `Viewport::push_input` runs a GUI stage between `_input` and
        // the shortcut/unhandled stages (upstream/godot/scene/main/viewport.cpp
        // `_gui_input_event`). Patina had the hit-test math but no runtime call
        // path, so `_gui_input`, `gui_input`, `mouse_entered`/`mouse_exited` and
        // button presses never fired from a real event (pat-0q2kx.18).
        for node_id in self.all_nodes_in_tree_order() {
            if self.input_handled {
                return;
            }
            self.call_script_with_access(node_id, "_input", std::slice::from_ref(&payload));
        }
        if self.input_handled {
            return;
        }

        self.gui_input_stage(event, &payload);
        if self.input_handled {
            return;
        }

        for &stage in &stages[1..] {
            for node_id in self.all_nodes_in_tree_order() {
                if self.input_handled {
                    return;
                }
                self.call_script_with_access(node_id, stage, std::slice::from_ref(&payload));
            }
        }
    }

    /// The GUI stage of [`push_input`](Self::push_input): hit-tests Controls and
    /// delivers the event to the one under the cursor (pat-0q2kx.18).
    ///
    /// Mirrors Godot's `Viewport::_gui_input_event`:
    /// - the topmost Control at the position receives `_gui_input` and emits
    ///   `gui_input`;
    /// - `MOUSE_FILTER_PASS` bubbles the event to ancestors, `STOP` consumes it,
    ///   `IGNORE` is already excluded by the hit test;
    /// - hover transitions emit `mouse_exited` on the old control and
    ///   `mouse_entered` on the new one;
    /// - a press followed by a release **inside the same control** is a click,
    ///   which emits `button_down` / `pressed` / `button_up` on a BaseButton;
    /// - a disabled button consumes the event without emitting anything.
    fn gui_input_stage(&mut self, event: &gdplatform::input::InputEvent, payload: &Variant) {
        use gdplatform::input::InputEvent as Ev;
        let position = match event {
            Ev::MouseButton { position, .. } | Ev::MouseMotion { position, .. } => *position,
            // Only mouse events participate in the GUI stage; keyboard input
            // reaches Controls through focus, which Patina does not model yet.
            _ => return,
        };

        let hit = crate::control::control_at_point(self, position);

        // Hover transitions fire regardless of which control (if any) consumes
        // the event, matching Godot's separate mouse-enter bookkeeping.
        if self.hovered_control != hit {
            if let Some(prev) = self.hovered_control {
                if self.nodes.contains_key(&prev) {
                    self.emit_signal(prev, "mouse_exited", &[]);
                    self.call_script_with_access(prev, "_mouse_exited", &[]);
                }
            }
            if let Some(now) = hit {
                self.emit_signal(now, "mouse_entered", &[]);
                self.call_script_with_access(now, "_mouse_entered", &[]);
            }
            self.hovered_control = hit;
        }

        let Some(hit) = hit else {
            // A press over empty space clears any in-flight press so a later
            // release elsewhere cannot be mistaken for a click.
            if matches!(event, Ev::MouseButton { pressed: true, .. }) {
                self.gui_press_target = None;
            }
            return;
        };

        // Deliver `_gui_input` / `gui_input` up the PASS chain: the hit control
        // first, then each ancestor whose filter is PASS. A STOP control ends
        // the walk (it consumed the event).
        let mut target = Some(hit);
        while let Some(node_id) = target {
            self.call_script_with_access(node_id, "_gui_input", std::slice::from_ref(payload));
            self.emit_signal(node_id, "gui_input", std::slice::from_ref(payload));
            if self.input_handled {
                return;
            }
            match crate::control::get_mouse_filter(self, node_id) {
                crate::control::MouseFilter::Pass => {
                    target = self.nodes.get(&node_id).and_then(|n| n.parent());
                }
                _ => break,
            }
        }

        // Button press/release semantics on the originally hit control.
        if let Ev::MouseButton { pressed, .. } = event {
            let is_button = crate::control::node_is_base_button(self, hit);
            let disabled = crate::control::is_disabled(self, hit);
            if *pressed {
                self.gui_press_target = Some(hit);
                // Godot grabs focus on press for FOCUS_CLICK/FOCUS_ALL controls;
                // clicking a non-focusable one drops focus entirely
                // (pat-0q2kx.21).
                if self.control_takes_click_focus(hit) {
                    self.grab_focus(hit);
                } else {
                    self.release_focus();
                }
                if is_button && !disabled {
                    self.emit_signal(hit, "button_down", &[]);
                }
            } else {
                // A click only counts when press and release land on the same
                // control, as in Godot — dragging off a button cancels it.
                let same = self.gui_press_target == Some(hit);
                self.gui_press_target = None;
                if is_button && !disabled && same {
                    self.emit_signal(hit, "button_up", &[]);
                    self.emit_signal(hit, "pressed", &[]);
                }
            }
        }
    }

    /// The Control currently under the mouse, if any (Godot's viewport hover
    /// bookkeeping). Exposed for tests and tooling.
    pub fn hovered_control(&self) -> Option<NodeId> {
        self.hovered_control
    }

    // -- keyboard focus (pat-0q2kx.21) --------------------------------------

    /// The Control holding keyboard focus (Godot `Viewport.gui_get_focus_owner`).
    pub fn focus_owner(&self) -> Option<NodeId> {
        self.focus_owner
    }

    /// Whether `node_id` currently holds focus (Godot `Control.has_focus`).
    pub fn has_focus(&self, node_id: NodeId) -> bool {
        self.focus_owner == Some(node_id)
    }

    /// Gives `node_id` keyboard focus (Godot `Control.grab_focus`).
    ///
    /// Emits `focus_exited` on the previous owner and `focus_entered` on the new
    /// one. Re-grabbing focus on the node that already holds it is a no-op, so
    /// repeated clicks do not re-fire the signals.
    pub fn grab_focus(&mut self, node_id: NodeId) {
        if self.focus_owner == Some(node_id) || !self.nodes.contains_key(&node_id) {
            return;
        }
        if let Some(prev) = self.focus_owner.take() {
            if self.nodes.contains_key(&prev) {
                self.emit_signal(prev, "focus_exited", &[]);
                self.call_script_with_access(prev, "_focus_exited", &[]);
            }
        }
        self.focus_owner = Some(node_id);
        self.emit_signal(node_id, "focus_entered", &[]);
        self.call_script_with_access(node_id, "_focus_entered", &[]);
    }

    /// Drops keyboard focus, emitting `focus_exited` on the owner if there was
    /// one (Godot `Control.release_focus`).
    pub fn release_focus(&mut self) {
        if let Some(prev) = self.focus_owner.take() {
            if self.nodes.contains_key(&prev) {
                self.emit_signal(prev, "focus_exited", &[]);
                self.call_script_with_access(prev, "_focus_exited", &[]);
            }
        }
    }

    /// Whether clicking `node_id` should grab focus, from Godot's
    /// `Control.focus_mode` (`FOCUS_NONE = 0`, `FOCUS_CLICK = 1`,
    /// `FOCUS_ALL = 2`).
    ///
    /// When the property is absent Patina falls back to class: the interactive
    /// Control families default to `FOCUS_ALL` in Godot, everything else to
    /// `FOCUS_NONE`. Without that fallback a plain `.tscn` Button — which never
    /// serializes the default `focus_mode` — would never take focus.
    fn control_takes_click_focus(&self, node_id: NodeId) -> bool {
        let Some(node) = self.nodes.get(&node_id) else {
            return false;
        };
        match node.get_property("focus_mode") {
            Variant::Int(0) => false,
            Variant::Int(_) => true,
            _ => {
                const FOCUSABLE_BY_DEFAULT: &[&str] = &[
                    "LineEdit",
                    "TextEdit",
                    "CodeEdit",
                    "SpinBox",
                    "OptionButton",
                    "MenuButton",
                    "CheckBox",
                    "CheckButton",
                    "Button",
                    "LinkButton",
                    "TextureButton",
                    "BaseButton",
                    "HSlider",
                    "VSlider",
                    "ItemList",
                    "Tree",
                    "TabBar",
                ];
                node.is_class("BaseButton") || FOCUSABLE_BY_DEFAULT.contains(&node.class_name())
            }
        }
    }

    /// Calls `_enter_tree()` on the node's script, if present.
    pub fn process_script_enter_tree(&mut self, node_id: NodeId) {
        self.call_script_with_access(node_id, "_enter_tree", &[]);
    }

    /// Calls `_exit_tree()` on the node's script, if present.
    pub fn process_script_exit_tree(&mut self, node_id: NodeId) {
        self.call_script_with_access(node_id, "_exit_tree", &[]);
    }

    /// Calls `_process(delta)` on all nodes that have attached scripts.
    pub fn process_all_scripts_process(&mut self, delta: f64) {
        let ids: Vec<NodeId> = self
            .all_nodes_in_process_order()
            .into_iter()
            .filter(|id| self.should_process_node(*id))
            // Godot: `set_process(false)` stops _process independently of
            // process_mode (pat-nzgdu.137).
            .filter(|id| self.is_node_processing(*id))
            .filter(|id| self.scripts.contains_key(id))
            .collect();
        for id in ids {
            self.process_script_process(id, delta);
        }
    }

    /// Calls `_physics_process(delta)` on all nodes that have attached scripts.
    pub fn process_all_scripts_physics_process(&mut self, delta: f64) {
        let ids: Vec<NodeId> = self
            .all_nodes_in_process_order()
            .into_iter()
            .filter(|id| self.should_process_node(*id))
            // `set_physics_process(false)` gates only the physics callback —
            // a node can stop _process and keep _physics_process.
            .filter(|id| self.is_node_physics_processing(*id))
            .filter(|id| self.scripts.contains_key(id))
            .collect();
        for id in ids {
            self.process_script_physics_process(id, delta);
        }
    }

    // -- per-node process toggles (pat-nzgdu.137) ----------------------------

    /// Whether `_process` is enabled on `node_id` (Godot's
    /// `Node.is_processing`). Unknown nodes report `false`.
    pub fn is_node_processing(&self, node_id: NodeId) -> bool {
        self.nodes.get(&node_id).is_some_and(|n| n.is_processing())
    }

    /// Whether `_physics_process` is enabled on `node_id`
    /// (`Node.is_physics_processing`). Unknown nodes report `false`.
    pub fn is_node_physics_processing(&self, node_id: NodeId) -> bool {
        self.nodes
            .get(&node_id)
            .is_some_and(|n| n.is_physics_processing())
    }

    /// Enables/disables `_process` on `node_id` (`Node.set_process`).
    /// Returns whether the node exists.
    pub fn set_node_process(&mut self, node_id: NodeId, enabled: bool) -> bool {
        match self.nodes.get_mut(&node_id) {
            Some(n) => {
                n.set_process(enabled);
                true
            }
            None => false,
        }
    }

    /// Enables/disables `_physics_process` on `node_id`
    /// (`Node.set_physics_process`). Returns whether the node exists.
    pub fn set_node_physics_process(&mut self, node_id: NodeId, enabled: bool) -> bool {
        match self.nodes.get_mut(&node_id) {
            Some(n) => {
                n.set_physics_process(enabled);
                true
            }
            None => false,
        }
    }

    /// Re-arms `_ready` on `node_id` so it runs again on the next tree-enter
    /// (`Node.request_ready`). Returns whether the node exists.
    pub fn request_node_ready(&mut self, node_id: NodeId) -> bool {
        match self.nodes.get_mut(&node_id) {
            Some(n) => {
                n.request_ready();
                true
            }
            None => false,
        }
    }

    // -- process dispatch ----------------------------------------------------

    /// Dispatches [`NOTIFICATION_INTERNAL_PHYSICS_PROCESS`](gdobject::NOTIFICATION_INTERNAL_PHYSICS_PROCESS)
    /// to every node in tree order.
    ///
    /// Called once per fixed-timestep physics tick, **before** user
    /// `NOTIFICATION_PHYSICS_PROCESS`. This matches Godot's per-frame
    /// ordering: internal physics -> user physics -> internal process -> user process.
    pub fn process_internal_physics_frame(&mut self) {
        let ids: Vec<NodeId> = self
            .all_nodes_in_process_order()
            .into_iter()
            .filter(|id| self.should_process_node(*id))
            .collect();
        for id in ids {
            self.trace_record(id, TraceEventType::Notification, "INTERNAL_PHYSICS_PROCESS");
            if let Some(node) = self.nodes.get_mut(&id) {
                node.receive_notification(gdobject::NOTIFICATION_INTERNAL_PHYSICS_PROCESS);
            }
        }
    }

    /// Dispatches [`NOTIFICATION_PHYSICS_PROCESS`](gdobject::NOTIFICATION_PHYSICS_PROCESS)
    /// to every node in tree order.
    ///
    /// Called once per fixed-timestep physics tick, after internal physics processing.
    pub fn process_physics_frame(&mut self) {
        let ids: Vec<NodeId> = self
            .all_nodes_in_process_order()
            .into_iter()
            .filter(|id| self.should_process_node(*id))
            .collect();
        for id in ids {
            self.trace_record(id, TraceEventType::Notification, "PHYSICS_PROCESS");
            if let Some(node) = self.nodes.get_mut(&id) {
                node.receive_notification(gdobject::NOTIFICATION_PHYSICS_PROCESS);
            }
        }
    }

    /// Dispatches [`NOTIFICATION_INTERNAL_PROCESS`](gdobject::NOTIFICATION_INTERNAL_PROCESS)
    /// to every node in tree order.
    ///
    /// Called once per visual frame, **before** user `NOTIFICATION_PROCESS`.
    pub fn process_internal_frame(&mut self) {
        let ids: Vec<NodeId> = self
            .all_nodes_in_process_order()
            .into_iter()
            .filter(|id| self.should_process_node(*id))
            .collect();
        for id in ids {
            self.trace_record(id, TraceEventType::Notification, "INTERNAL_PROCESS");
            if let Some(node) = self.nodes.get_mut(&id) {
                node.receive_notification(gdobject::NOTIFICATION_INTERNAL_PROCESS);
            }
        }
    }

    /// Dispatches [`NOTIFICATION_PROCESS`](gdobject::NOTIFICATION_PROCESS)
    /// to every node in tree order.
    ///
    /// Called once per visual frame, after internal process.
    pub fn process_frame(&mut self) {
        let ids: Vec<NodeId> = self
            .all_nodes_in_process_order()
            .into_iter()
            .filter(|id| self.should_process_node(*id))
            .collect();
        for id in ids {
            self.trace_record(id, TraceEventType::Notification, "PROCESS");
            if let Some(node) = self.nodes.get_mut(&id) {
                node.receive_notification(gdobject::NOTIFICATION_PROCESS);
            }
        }
    }

    /// Dispatches [`NOTIFICATION_PROCESS`](gdobject::NOTIFICATION_PROCESS) to
    /// every node in tree order, interleaving `_process(delta)` script calls
    /// immediately after each node's notification.
    ///
    /// This matches Godot's per-node dispatch: each node receives its PROCESS
    /// notification and has its `_process()` called before the next node is
    /// processed.
    pub fn process_frame_with_scripts(&mut self, delta: f64) {
        self.process_timers(delta);
        let ids: Vec<NodeId> = self
            .all_nodes_in_process_order()
            .into_iter()
            .filter(|id| self.should_process_node(*id))
            .collect();
        for id in ids {
            self.trace_record(id, TraceEventType::Notification, "PROCESS");
            if let Some(node) = self.nodes.get_mut(&id) {
                node.receive_notification(gdobject::NOTIFICATION_PROCESS);
            }
            if self.scripts.contains_key(&id) {
                self.process_script_process(id, delta);
                // Deliver PROCESS to the script's `_notification` virtual too,
                // alongside the named `_process(delta)` callback (pat-nzgdu.13).
                self.notify_script(id, gdobject::NOTIFICATION_PROCESS.code() as i64);
            }
        }
    }

    /// Dispatches [`NOTIFICATION_PHYSICS_PROCESS`](gdobject::NOTIFICATION_PHYSICS_PROCESS) to
    /// every node in tree order, interleaving `_physics_process(delta)` script calls
    /// immediately after each node's notification.
    ///
    /// This matches Godot's per-node dispatch for physics processing.
    pub fn process_physics_frame_with_scripts(&mut self, delta: f64) {
        let ids: Vec<NodeId> = self
            .all_nodes_in_process_order()
            .into_iter()
            .filter(|id| self.should_process_node(*id))
            .collect();
        for id in ids {
            self.trace_record(id, TraceEventType::Notification, "PHYSICS_PROCESS");
            if let Some(node) = self.nodes.get_mut(&id) {
                node.receive_notification(gdobject::NOTIFICATION_PHYSICS_PROCESS);
            }
            if self.scripts.contains_key(&id) {
                self.process_script_physics_process(id, delta);
            }
        }
    }

    // -- collision detection -----------------------------------------------

    /// Runs simple distance-based collision detection on all nodes that
    /// have a `collision_radius` property, updating `_is_colliding`,
    /// `_colliding_with`, and `_off_screen` properties.
    ///
    /// See [`crate::collision`] for full details.
    pub fn process_collisions(&mut self) {
        crate::collision::process_collisions(self);
    }

    /// Same as [`process_collisions`](Self::process_collisions) but with
    /// configurable screen bounds for the off-screen check.
    pub fn process_collisions_with_bounds(&mut self, screen_w: f32, screen_h: f32) {
        crate::collision::process_collisions_with_bounds(self, screen_w, screen_h);
    }

    // -- deferred calls -----------------------------------------------------

    /// Queues a method call for the end of the frame (Godot `call_deferred`).
    ///
    /// Calls are FIFO and run in [`process_deferred_calls`](Self::process_deferred_calls)
    /// after process/collision and before `queue_free` deletions. Nested
    /// `call_deferred` during a flush is appended and processed in the same
    /// flush (Godot MessageQueue re-entrancy), up to a safety cap.
    ///
    /// Special method `"set"` with args `[property_name, value]` sets a node
    /// property (Godot `Object.set` deferred).
    pub fn call_deferred(&mut self, node_id: NodeId, method: &str, args: &[Variant]) {
        self.pending_deferred_calls.push(DeferredCall {
            node_id,
            method: method.to_string(),
            args: args.to_vec(),
        });
    }

    /// Number of deferred calls waiting to be flushed.
    pub fn pending_deferred_count(&self) -> usize {
        self.pending_deferred_calls.len()
    }

    /// Flushes the deferred call queue.
    ///
    /// Each entry either:
    /// - handles `"set"` as a property write on the node, or
    /// - dispatches to the node's attached script via
    ///   [`call_script_with_access`](Self::call_script_with_access).
    ///
    /// Missing nodes or methods are skipped (matching Godot's null-target
    /// tolerance for freed objects). Nested deferred calls enqueued during
    /// this flush run before returning.
    pub fn process_deferred_calls(&mut self) {
        // Cap nested re-queues so a method that always call_deferred's itself
        // cannot hang the frame forever.
        const MAX_FLUSH_ROUNDS: usize = 1024;
        let mut rounds = 0;
        while !self.pending_deferred_calls.is_empty() && rounds < MAX_FLUSH_ROUNDS {
            rounds += 1;
            let batch: Vec<DeferredCall> = self.pending_deferred_calls.drain(..).collect();
            for call in batch {
                if !self.nodes.contains_key(&call.node_id) {
                    continue;
                }
                if call.method == "set" {
                    // Godot Object.set(property, value) deferred form.
                    if call.args.len() >= 2 {
                        if let Variant::String(prop) = &call.args[0] {
                            if let Some(node) = self.nodes.get_mut(&call.node_id) {
                                node.set_property(prop, call.args[1].clone());
                            }
                        }
                    }
                    continue;
                }
                if self.scripts.contains_key(&call.node_id) {
                    self.call_script_with_access(call.node_id, &call.method, &call.args);
                }
            }
        }
    }

    // -- runtime node creation/deletion -------------------------------------

    /// Creates a new node with the given class name and node name.
    ///
    /// The node is inserted into the arena but is **not** attached to any
    /// parent yet. Call [`add_child`](Self::add_child) to place it in the
    /// hierarchy.
    pub fn create_node(&mut self, class_name: &str, name: &str) -> NodeId {
        let mut node = Node::new(name, class_name);
        // Seed the property bag with the class's registered ClassDB defaults,
        // walked base→derived up the inheritance chain, so a freshly created
        // node already carries Godot's class defaults (scale=(1,1), visible=true,
        // …) before any override — reading an unset registered property yields
        // the class default, not Nil (pat-nzgdu.100). An unpopulated ClassDB
        // (empty property list) simply seeds nothing.
        for prop in gdobject::class_db::get_property_list(class_name) {
            node.set_property(&prop.name, prop.default_value);
        }
        let id = node.id();
        self.nodes.insert(id, node);
        id
    }

    /// Marks a node for deferred deletion. The node (and its subtree) will
    /// be removed from the tree when [`process_deletions`](Self::process_deletions)
    /// is called at the end of the frame.
    ///
    /// This is safe to call during `_process` — iterators are not
    /// invalidated because the actual removal is deferred.
    pub fn queue_free(&mut self, node_id: NodeId) {
        if !self.pending_deletions.contains(&node_id) {
            self.pending_deletions.push(node_id);
        }
    }

    /// Removes all nodes that were marked with [`queue_free`](Self::queue_free)
    /// during this frame.
    ///
    /// Call this **after** all `_process` callbacks have run and before
    /// the next frame begins. Scripts attached to deleted nodes are also
    /// removed.
    pub fn process_deletions(&mut self) {
        let to_delete: Vec<NodeId> = self.pending_deletions.drain(..).collect();
        for id in to_delete {
            // Skip nodes that were already removed (e.g. child of an
            // earlier deletion in this batch).
            if !self.nodes.contains_key(&id) {
                continue;
            }

            // Fire EXIT_TREE lifecycle (bottom-up) while scripts are still
            // attached, matching Godot's queue_free() behavior.
            let should_exit = self
                .nodes
                .get(&id)
                .map(|n| n.is_inside_tree())
                .unwrap_or(false);
            if should_exit {
                LifecycleManager::exit_tree(self, id);
            }

            // Fire PREDELETE notification (bottom-up) after EXIT_TREE.
            let mut bottom_up = Vec::new();
            self.collect_subtree_bottom_up(id, &mut bottom_up);
            for &nid in &bottom_up {
                self.trace_record(nid, TraceEventType::Notification, "PREDELETE");
                if let Some(node) = self.nodes.get_mut(&nid) {
                    node.receive_notification(gdobject::NOTIFICATION_PREDELETE);
                }
                // Deliver PREDELETE to the script's `_notification` virtual
                // before the script is removed (pat-nzgdu.13).
                self.notify_script(nid, gdobject::NOTIFICATION_PREDELETE.code() as i64);
            }

            // Now remove scripts and the node itself.
            // exit_tree already marked nodes as outside tree, so
            // remove_node will skip the redundant EXIT_TREE call.
            let mut subtree = Vec::new();
            self.collect_subtree_top_down(id, &mut subtree);
            for &nid in &subtree {
                self.scripts.remove(&nid);
            }
            let _ = self.remove_node(id);
        }
    }

    /// Returns the number of nodes currently pending deletion.
    pub fn pending_deletion_count(&self) -> usize {
        self.pending_deletions.len()
    }

    /// Replaces the current scene by removing all children of the root node
    /// and instancing the given [`PackedScene`] under root.
    ///
    /// This mirrors Godot's `SceneTree.change_scene_to_packed()`.
    pub fn change_scene_to_packed(
        &mut self,
        scene: &crate::packed_scene::PackedScene,
    ) -> EngineResult<NodeId> {
        // Collect direct children of root (snapshot to avoid borrow issues).
        let children: Vec<NodeId> = self
            .get_node(self.root_id)
            .map(|root| root.children().to_vec())
            .unwrap_or_default();

        // Remove each child subtree (this fires exit_tree, unparented, etc.).
        for child_id in children {
            // Also clean up scripts for the subtree.
            let mut subtree = Vec::new();
            self.collect_subtree_top_down(child_id, &mut subtree);
            for &nid in &subtree {
                self.scripts.remove(&nid);
                self.signal_stores.remove(&nid);
                self.animation_players.remove(&nid);
                self.animation_trees.remove(&nid);
            }
            let _ = self.remove_node(child_id);
        }

        // Process any pending deletions.
        self.process_deletions();

        // Remember the packed scene for reload_current_scene().
        self.current_packed_scene = Some(scene.clone());

        // Instance the new scene under root.
        let id = crate::packed_scene::add_packed_scene_to_tree(self, self.root_id, scene)?;
        self.current_scene_id = Some(id);
        Ok(id)
    }

    /// Reloads the current scene from its packed source.
    ///
    /// Returns an error if no packed scene source is stored (e.g. scene was
    /// set via `change_scene_to_node`).
    pub fn reload_current_scene(&mut self) -> EngineResult<NodeId> {
        let packed = self.current_packed_scene.clone().ok_or_else(|| {
            EngineError::InvalidOperation("No packed scene source to reload from".into())
        })?;
        self.change_scene_to_packed(&packed)
    }

    /// Replaces the current scene with a manually constructed node.
    ///
    /// Unlike `change_scene_to_packed`, this clears the packed scene source,
    /// so `reload_current_scene` will fail afterward.
    pub fn change_scene_to_node(&mut self, node: Node) -> EngineResult<NodeId> {
        // Remove existing children of root.
        let children: Vec<NodeId> = self
            .get_node(self.root_id)
            .map(|root| root.children().to_vec())
            .unwrap_or_default();

        for child_id in children {
            let mut subtree = Vec::new();
            self.collect_subtree_top_down(child_id, &mut subtree);
            for &nid in &subtree {
                self.scripts.remove(&nid);
                self.signal_stores.remove(&nid);
                self.animation_players.remove(&nid);
                self.animation_trees.remove(&nid);
            }
            let _ = self.remove_node(child_id);
        }

        self.process_deletions();

        // Clear packed source — reload won't work after this.
        self.current_packed_scene = None;

        let id = self.add_child(self.root_id, node)?;
        self.current_scene_id = Some(id);
        Ok(id)
    }

    /// Queues a scene change from a `res://` (or absolute) path, mirroring
    /// Godot's `SceneTree.change_scene_to_file()`. The `.tscn` is loaded and
    /// parsed immediately (so a missing/invalid path errors here), but the swap
    /// is DEFERRED: it is applied by [`process_deferred_scene_change`] at the
    /// next frame boundary, so a script that calls this is not torn down
    /// mid-run. `MainLoop::step` drives the deferred apply each frame.
    pub fn change_scene_to_file(&mut self, path: &str) -> EngineResult<()> {
        let resolved = gdcore::resolve_path(path)?;
        let source = std::fs::read_to_string(&resolved)?;
        let scene = crate::packed_scene::PackedScene::from_tscn(&source)?;
        self.pending_scene = Some(scene);
        Ok(())
    }

    /// Applies a scene change queued by [`change_scene_to_file`], if any.
    /// Returns the new scene root's id when a swap occurred. Driven once per
    /// frame by `MainLoop::step` for Godot's end-of-frame deferred timing.
    pub fn process_deferred_scene_change(&mut self) -> EngineResult<Option<NodeId>> {
        match self.pending_scene.take() {
            Some(scene) => Ok(Some(self.change_scene_to_packed(&scene)?)),
            None => Ok(None),
        }
    }

    /// Whether a deferred scene change is queued (a `change_scene_to_file` call
    /// is awaiting the next frame boundary).
    pub fn has_pending_scene_change(&self) -> bool {
        self.pending_scene.is_some()
    }

    /// Frees the current scene without loading a replacement, mirroring Godot's
    /// `SceneTree.unload_current_scene()`. Removes every child of root (firing
    /// exit_tree and cleaning up their scripts/signals/animation players) and
    /// clears the current-scene and packed-source state.
    pub fn unload_current_scene(&mut self) {
        let children: Vec<NodeId> = self
            .get_node(self.root_id)
            .map(|root| root.children().to_vec())
            .unwrap_or_default();
        for child_id in children {
            let mut subtree = Vec::new();
            self.collect_subtree_top_down(child_id, &mut subtree);
            for &nid in &subtree {
                self.scripts.remove(&nid);
                self.signal_stores.remove(&nid);
                self.animation_players.remove(&nid);
                self.animation_trees.remove(&nid);
            }
            let _ = self.remove_node(child_id);
        }
        self.process_deletions();
        self.current_packed_scene = None;
        self.current_scene_id = None;
    }

    /// Requests the application quit with `exit_code`, mirroring Godot's
    /// `SceneTree.quit()`. Sets a flag the runner/editor polls via
    /// [`should_quit`](Self::should_quit); it does not itself stop execution.
    pub fn quit(&mut self, exit_code: i32) {
        self.should_quit = true;
        self.exit_code = exit_code;
    }

    /// Whether [`quit`](Self::quit) has been requested.
    pub fn should_quit(&self) -> bool {
        self.should_quit
    }

    /// The exit code set by the last [`quit`](Self::quit) call (default 0).
    pub fn exit_code(&self) -> i32 {
        self.exit_code
    }
}

impl Default for SceneTree {
    fn default() -> Self {
        Self::new()
    }
}

/// Applies an animation root-motion delta to a node's transform by translating
/// its `position` property and adding to the Y component of its `rotation`.
///
/// Root motion moves the owner node itself (matching Godot, where the mixer's
/// parent/character consumes the delta) rather than driving an absolute value,
/// so successive frames accumulate movement.
/// Rebuilds a node's [`SpriteFrames`] from the flat `__sf_*` companion keys the
/// SpriteFrames editor authors (`editor_server.rs`, pat-0q2kx.74).
///
/// Patina's property bag is flat, so the resource is stored as `__sf_anims__`
/// plus `__sf_<anim>_{speed,loop,frames}` rather than as a nested object. Only
/// the frame COUNT matters to playback — which texture each frame draws is the
/// renderer's business.
///
/// Returns `None` when the node declares no animations, so the caller can skip
/// it without allocating.
fn sprite_frames_from_node(node: &Node) -> Option<SpriteFrames> {
    let names = match node.get_property("__sf_anims__") {
        Variant::PackedStringArray(v) if !v.is_empty() => v,
        _ => return None,
    };
    let mut frames = SpriteFrames::new();
    for name in &names {
        let count = match node.get_property(&format!("__sf_{name}_frames")) {
            Variant::PackedStringArray(f) => f.len(),
            _ => 0,
        };
        let speed = match node.get_property(&format!("__sf_{name}_speed")) {
            Variant::Float(f) => f as f32,
            Variant::Int(i) => i as f32,
            // Godot's SpriteFrames default FPS.
            _ => 5.0,
        };
        // Only an explicit `false` turns looping off.
        let loops = !matches!(
            node.get_property(&format!("__sf_{name}_loop")),
            Variant::Bool(false)
        );
        frames.add_animation(name, count, speed, loops);
    }
    Some(frames)
}

fn apply_root_motion_delta(node: &mut Node, delta: &crate::animation::RootMotionDelta) {
    if delta.position != gdcore::math::Vector3::ZERO {
        let cur = match node.get_property("position") {
            Variant::Vector3(v) => v,
            _ => gdcore::math::Vector3::ZERO,
        };
        node.set_property("position", Variant::Vector3(cur + delta.position));
    }
    if delta.rotation_y != 0.0 {
        let cur = match node.get_property("rotation") {
            Variant::Vector3(v) => v,
            _ => gdcore::math::Vector3::ZERO,
        };
        node.set_property(
            "rotation",
            Variant::Vector3(gdcore::math::Vector3::new(
                cur.x,
                cur.y + delta.rotation_y,
                cur.z,
            )),
        );
    }
}

// ───────────────────────────────────────────────────────────────────────────
// pat-nzgdu.86: Node query surface — wildcard find_child/find_children,
// add_sibling, find_parent, is_ancestor_of, and a propagate_call visitor.
// These operate on NodeId over the SceneTree with Godot wildcard semantics.
// (Exposing them to the GDScript bridge is a follow-up; the interpreter's
// singleton dispatch was under concurrent edit at authoring time.)
// ───────────────────────────────────────────────────────────────────────────

/// Converts an [`InputEvent`](gdplatform::input::InputEvent) into the Dictionary
/// a script's input virtual receives as its `event` argument.
///
/// Godot passes a real `InputEvent` object; Patina has no Variant type for one
/// yet, so each event is described by a Dictionary carrying a `type` tag plus
/// that variant's payload (e.g. `{"type": "key", "keycode": "A", "pressed": true}`).
/// Scripts read it with `event["keycode"]`.
fn input_event_to_variant(event: &gdplatform::input::InputEvent) -> Variant {
    use gdplatform::input::InputEvent as Ev;

    let mut d = gdvariant::Dictionary::new();
    match event {
        Ev::Key {
            key,
            pressed,
            shift,
            ctrl,
            alt,
        } => {
            d.insert("type", "key");
            d.insert("keycode", key.name());
            d.insert("pressed", *pressed);
            d.insert("shift", *shift);
            d.insert("ctrl", *ctrl);
            d.insert("alt", *alt);
        }
        Ev::MouseButton {
            button,
            pressed,
            position,
        } => {
            d.insert("type", "mouse_button");
            d.insert("button", format!("{button:?}"));
            d.insert("pressed", *pressed);
            d.insert("position", *position);
        }
        Ev::MouseMotion { position, relative } => {
            d.insert("type", "mouse_motion");
            d.insert("position", *position);
            d.insert("relative", *relative);
        }
        Ev::GamepadButton {
            button,
            pressed,
            gamepad_id,
        } => {
            d.insert("type", "gamepad_button");
            d.insert("button", format!("{button:?}"));
            d.insert("pressed", *pressed);
            d.insert("gamepad_id", *gamepad_id as i64);
        }
        Ev::GamepadAxis {
            axis,
            value,
            gamepad_id,
        } => {
            d.insert("type", "gamepad_axis");
            d.insert("axis", format!("{axis:?}"));
            d.insert("value", *value);
            d.insert("gamepad_id", *gamepad_id as i64);
        }
        Ev::ScreenTouch {
            index,
            position,
            pressed,
        } => {
            d.insert("type", "screen_touch");
            d.insert("index", *index as i64);
            d.insert("position", *position);
            d.insert("pressed", *pressed);
        }
        Ev::ScreenDrag {
            index,
            position,
            relative,
            velocity,
        } => {
            d.insert("type", "screen_drag");
            d.insert("index", *index as i64);
            d.insert("position", *position);
            d.insert("relative", *relative);
            d.insert("velocity", *velocity);
        }
        Ev::Action {
            action,
            pressed,
            strength,
        } => {
            d.insert("type", "action");
            d.insert("action", action.clone());
            d.insert("pressed", *pressed);
            d.insert("strength", *strength);
        }
    }
    Variant::Dictionary(d)
}

/// Godot `String.match`-style wildcard test used by `find_child`/`find_children`:
/// `*` matches any (possibly empty) run of characters, `?` matches exactly one,
/// every other character matches literally. Comparison is over Unicode scalars.
fn wildcard_match(pattern: &str, name: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let s: Vec<char> = name.chars().collect();
    // Classic iterative glob with `*` backtracking.
    let (mut pi, mut si) = (0usize, 0usize);
    let (mut star, mut mark) = (usize::MAX, 0usize);
    while si < s.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == s[si]) {
            pi += 1;
            si += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = pi;
            mark = si;
            pi += 1;
        } else if star != usize::MAX {
            pi = star + 1;
            mark += 1;
            si = mark;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

impl SceneTree {
    /// Depth-first descendants of `id` (excluding `id`), in tree (pre-)order.
    fn collect_subtree_into(&self, id: NodeId, out: &mut Vec<NodeId>) {
        out.push(id);
        let kids: Vec<NodeId> = match self.nodes.get(&id) {
            Some(n) => n.children().to_vec(),
            None => return,
        };
        for c in kids {
            self.collect_subtree_into(c, out);
        }
    }

    /// All descendants of `start` (excluding `start`), depth-first, in tree order.
    fn descendants(&self, start: NodeId) -> Vec<NodeId> {
        let mut out = Vec::new();
        let kids: Vec<NodeId> = self
            .nodes
            .get(&start)
            .map(|n| n.children().to_vec())
            .unwrap_or_default();
        for c in kids {
            self.collect_subtree_into(c, &mut out);
        }
        out
    }

    /// Godot `Node.find_child(pattern, recursive=true)`: the first descendant
    /// whose name matches `pattern` (wildcard). Searches the whole subtree when
    /// `recursive`, otherwise only direct children. Excludes `start` itself.
    pub fn find_child(&self, start: NodeId, pattern: &str, recursive: bool) -> Option<NodeId> {
        let candidates = if recursive {
            self.descendants(start)
        } else {
            self.nodes
                .get(&start)
                .map(|n| n.children().to_vec())
                .unwrap_or_default()
        };
        candidates.into_iter().find(|&id| {
            self.nodes
                .get(&id)
                .map(|n| wildcard_match(pattern, n.name()))
                .unwrap_or(false)
        })
    }

    /// Godot `Node.find_children(pattern, type="", recursive=true)`: every
    /// descendant whose name matches `pattern` and — when `type_name` is `Some`
    /// — whose class name equals it. Excludes `start` itself.
    pub fn find_children(
        &self,
        start: NodeId,
        pattern: &str,
        type_name: Option<&str>,
        recursive: bool,
    ) -> Vec<NodeId> {
        let candidates = if recursive {
            self.descendants(start)
        } else {
            self.nodes
                .get(&start)
                .map(|n| n.children().to_vec())
                .unwrap_or_default()
        };
        candidates
            .into_iter()
            .filter(|&id| match self.nodes.get(&id) {
                Some(n) => {
                    wildcard_match(pattern, n.name())
                        && type_name.map_or(true, |t| n.class_name() == t)
                }
                None => false,
            })
            .collect()
    }

    /// Godot `Node.find_parent(pattern)`: nearest ancestor whose name matches.
    pub fn find_parent(&self, start: NodeId, pattern: &str) -> Option<NodeId> {
        let mut cur = self.nodes.get(&start).and_then(|n| n.parent());
        while let Some(id) = cur {
            let n = self.nodes.get(&id)?;
            if wildcard_match(pattern, n.name()) {
                return Some(id);
            }
            cur = n.parent();
        }
        None
    }

    /// Godot `Node.is_ancestor_of(node)`: true if `descendant` lies in
    /// `ancestor`'s subtree (strictly below it).
    pub fn is_ancestor_of(&self, ancestor: NodeId, descendant: NodeId) -> bool {
        let mut cur = self.nodes.get(&descendant).and_then(|n| n.parent());
        while let Some(id) = cur {
            if id == ancestor {
                return true;
            }
            cur = self.nodes.get(&id).and_then(|n| n.parent());
        }
        false
    }

    /// Godot `Node.add_sibling(node)`: inserts `node` into `sibling`'s parent
    /// immediately after `sibling` in tree order. Returns the new node's id.
    pub fn add_sibling(&mut self, sibling_id: NodeId, node: Node) -> EngineResult<NodeId> {
        let parent_id = self
            .nodes
            .get(&sibling_id)
            .and_then(|n| n.parent())
            .ok_or_else(|| {
                EngineError::InvalidOperation(format!(
                    "node {sibling_id} has no parent to add a sibling under"
                ))
            })?;
        let sibling_index = self.get_index(sibling_id).ok_or_else(|| {
            EngineError::InvalidOperation(format!("node {sibling_id} not found in its parent"))
        })?;
        // add_child appends the new node; move it to right after the sibling.
        let new_id = self.add_child(parent_id, node)?;
        self.move_child(parent_id, new_id, sibling_index + 1)?;
        Ok(new_id)
    }

    /// Godot `Node.propagate_call(method, args, parent_first)` traversal
    /// primitive: visits `start` and every descendant, applying `f`. When
    /// `parent_first` is true the parent is visited before its children
    /// (pre-order), otherwise children first. The GDScript bridge supplies an
    /// `f` that invokes the named script method; exposing it as a generic
    /// visitor keeps the traversal testable without the interpreter.
    pub fn propagate_call<F: FnMut(&mut Node)>(
        &mut self,
        start: NodeId,
        parent_first: bool,
        mut f: F,
    ) {
        let mut order = vec![start];
        order.extend(self.descendants(start));
        if !parent_first {
            order.reverse();
        }
        for id in order {
            if let Some(n) = self.nodes.get_mut(&id) {
                f(n);
            }
        }
    }
}

// ───────────────────────────────────────────────────────────────────────────
// pat-nzgdu.136: SceneTree group broadcasts — call_group / notify_group /
// set_group (+ flags) and get_first_node_in_group / get_node_count_in_group /
// has_group, over the existing group membership index.
// ───────────────────────────────────────────────────────────────────────────

/// Flags for [`SceneTree::call_group_flags`], mirroring Godot's
/// `SceneTree.GroupCallFlags`.
pub const GROUP_CALL_DEFAULT: u32 = 0;
/// Invoke members in reverse tree order.
pub const GROUP_CALL_REVERSE: u32 = 1;
/// Queue the calls until the frame's deferred flush instead of running now.
pub const GROUP_CALL_DEFERRED: u32 = 2;
/// Coalesce identical calls (reserved; group members are already unique).
pub const GROUP_CALL_UNIQUE: u32 = 4;

impl SceneTree {
    /// Group members in depth-first tree order (Godot broadcasts in tree order).
    fn nodes_in_group_ordered(&self, group: &str) -> Vec<NodeId> {
        let members = match self.groups.get(group) {
            Some(m) if !m.is_empty() => m,
            _ => return Vec::new(),
        };
        let root = self.root_id();
        let mut order = vec![root];
        order.extend(self.descendants(root));
        order
            .into_iter()
            .filter(|id| members.contains(id))
            .collect()
    }

    /// Godot `SceneTree.has_group` — whether any node is registered in `group`.
    pub fn has_group(&self, group: &str) -> bool {
        self.groups
            .get(group)
            .map(|m| !m.is_empty())
            .unwrap_or(false)
    }

    /// Godot `SceneTree.get_node_count_in_group`.
    pub fn get_node_count_in_group(&self, group: &str) -> usize {
        self.groups.get(group).map(|m| m.len()).unwrap_or(0)
    }

    /// Godot `SceneTree.get_first_node_in_group` — first member in tree order.
    pub fn get_first_node_in_group(&self, group: &str) -> Option<NodeId> {
        self.nodes_in_group_ordered(group).into_iter().next()
    }

    /// Godot `SceneTree.call_group(group, method, ...args)` — invoke `method` on
    /// every member's attached script, in tree order.
    pub fn call_group(&mut self, group: &str, method: &str, args: &[Variant]) {
        self.call_group_flags(GROUP_CALL_DEFAULT, group, method, args);
    }

    /// Godot `SceneTree.call_group_flags` — like [`call_group`](Self::call_group)
    /// honoring GROUP_CALL_REVERSE (reverse tree order) and GROUP_CALL_DEFERRED
    /// (route through the existing deferred queue, run at the frame's flush).
    pub fn call_group_flags(&mut self, flags: u32, group: &str, method: &str, args: &[Variant]) {
        let mut members = self.nodes_in_group_ordered(group);
        if flags & GROUP_CALL_REVERSE != 0 {
            members.reverse();
        }
        let deferred = flags & GROUP_CALL_DEFERRED != 0;
        for id in members {
            if deferred {
                self.call_deferred(id, method, args);
            } else if self.scripts.contains_key(&id) {
                self.call_script_with_access(id, method, args);
            }
        }
    }

    /// Godot `SceneTree.notify_group(group, what)` — deliver a notification to
    /// every member.
    pub fn notify_group(&mut self, group: &str, what: gdobject::notification::Notification) {
        for id in self.nodes_in_group_ordered(group) {
            if let Some(node) = self.nodes.get_mut(&id) {
                node.receive_notification(what);
            }
        }
    }

    /// Godot `SceneTree.set_group(group, property, value)` — set a property on
    /// every member.
    pub fn set_group(&mut self, group: &str, property: &str, value: Variant) {
        for id in self.nodes_in_group_ordered(group) {
            if let Some(node) = self.nodes.get_mut(&id) {
                node.set_property(property, value.clone());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- pat-nzgdu.138: duplicate() flags ------------------------------------

    /// Builds `parent -> child` where the child is in a group and has a signal
    /// connection, then returns (tree, child_id).
    fn tree_with_connected_grouped_child() -> (SceneTree, NodeId) {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let parent = tree.add_child(root, Node::new("Parent", "Node")).unwrap();
        let child = tree.add_child(parent, Node::new("Child", "Node")).unwrap();
        tree.get_node_mut(child)
            .unwrap()
            .set_property("hp", Variant::Int(7));
        tree.add_to_group(child, "enemies").unwrap();
        tree.connect_signal(
            child,
            "died",
            gdobject::signal::Connection::new(root.object_id(), "on_died"),
        );
        (tree, child)
    }

    /// Number of connections on `node`'s `signal`, or 0 when it has no store.
    fn conn_count(tree: &SceneTree, node: NodeId, signal: &str) -> usize {
        tree.signal_store(node)
            .and_then(|s| s.get_signal(signal))
            .map(|sig| sig.connection_count())
            .unwrap_or(0)
    }

    /// The bead's duplicate clause: `duplicate(DUPLICATE_GROUPS |
    /// DUPLICATE_SIGNALS)` copies group membership AND signal connections.
    #[test]
    fn duplicate_node_flags_copy_groups_and_signals() {
        let (mut tree, child) = tree_with_connected_grouped_child();
        let before = tree.get_nodes_in_group("enemies").len();

        let copy = tree
            .duplicate_node(child, DuplicateFlags::GROUPS | DuplicateFlags::SIGNALS)
            .expect("duplicate should succeed");

        // Properties always come along.
        assert_eq!(
            tree.get_node(copy).unwrap().get_property("hp"),
            Variant::Int(7),
            "properties are always duplicated",
        );
        // Groups: both the node's set AND the tree's group index.
        assert!(
            tree.get_node(copy).unwrap().groups().contains("enemies"),
            "DUPLICATE_GROUPS must copy group membership",
        );
        assert_eq!(
            tree.get_nodes_in_group("enemies").len(),
            before + 1,
            "the tree's group index must include the duplicate",
        );
        // Signals: the connection store rides along.
        assert_eq!(
            conn_count(&tree, copy, "died"),
            conn_count(&tree, child, "died"),
            "DUPLICATE_SIGNALS must copy signal connections",
        );
        assert!(
            conn_count(&tree, copy, "died") > 0,
            "the source must actually have a connection, or this proves nothing",
        );
        // The copy is a sibling of the original.
        assert_eq!(
            tree.get_node(copy).unwrap().parent(),
            tree.get_node(child).unwrap().parent(),
            "a duplicate attaches under the original's parent",
        );
    }

    /// Without the flags, neither groups nor signals are copied — proving the
    /// flags gate rather than everything always coming along.
    #[test]
    fn duplicate_node_without_flags_copies_neither() {
        let (mut tree, child) = tree_with_connected_grouped_child();
        let before = tree.get_nodes_in_group("enemies").len();

        let copy = tree.duplicate_node(child, DuplicateFlags::NONE).unwrap();

        assert_eq!(
            tree.get_node(copy).unwrap().get_property("hp"),
            Variant::Int(7),
            "properties are copied regardless of flags",
        );
        assert!(
            !tree.get_node(copy).unwrap().groups().contains("enemies"),
            "without DUPLICATE_GROUPS the copy joins no groups",
        );
        assert_eq!(
            tree.get_nodes_in_group("enemies").len(),
            before,
            "the group index must not gain the duplicate",
        );
        assert_eq!(
            conn_count(&tree, copy, "died"),
            0,
            "without DUPLICATE_SIGNALS the copy has no connections",
        );
    }

    /// Duplication is recursive: children come along with their own flag-gated
    /// groups.
    #[test]
    fn duplicate_node_copies_descendants() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let parent = tree.add_child(root, Node::new("Parent", "Node")).unwrap();
        let a = tree.add_child(parent, Node::new("A", "Node")).unwrap();
        let _b = tree.add_child(a, Node::new("B", "Node")).unwrap();
        tree.add_to_group(_b, "deep").unwrap();

        let copy = tree.duplicate_node(a, DuplicateFlags::GROUPS).unwrap();

        let mut ids = Vec::new();
        tree.collect_subtree_top_down(copy, &mut ids);
        assert_eq!(ids.len(), 2, "the whole subtree duplicates (A + B)");
        assert_eq!(
            tree.get_nodes_in_group("deep").len(),
            2,
            "a duplicated descendant joins its groups too",
        );
    }

    /// Godot's default `duplicate()` is SIGNALS|GROUPS|SCRIPTS.
    #[test]
    fn duplicate_flags_default_matches_godot() {
        let d = DuplicateFlags::default();
        assert!(d.contains(DuplicateFlags::SIGNALS));
        assert!(d.contains(DuplicateFlags::GROUPS));
        assert!(d.contains(DuplicateFlags::SCRIPTS));
        assert!(!d.contains(DuplicateFlags::USE_INSTANTIATION));
        assert_eq!(d.bits(), 7, "Godot's DUPLICATE default bitmask");
        assert_eq!(DuplicateFlags::from_bits(3).bits(), 3);
    }

    /// The scene tree's root has no parent to duplicate into, so it errors
    /// rather than corrupting the tree.
    #[test]
    fn duplicate_node_rejects_tree_root_and_unknown() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        assert!(tree
            .duplicate_node(root, DuplicateFlags::default())
            .is_err());

        let ghost = NodeId::from_object_id(gdcore::id::ObjectId::from_raw(987_654));
        assert!(tree
            .duplicate_node(ghost, DuplicateFlags::default())
            .is_err());
    }

    // -- pat-nzgdu.137: per-node process toggles gate script callbacks --------

    /// Records which lifecycle callbacks the tree dispatched to this node.
    struct ProcessProbe {
        calls: std::rc::Rc<std::cell::RefCell<Vec<String>>>,
    }

    impl gdscript_interop::bindings::ScriptInstance for ProcessProbe {
        fn call_method(
            &mut self,
            name: &str,
            _args: &[Variant],
        ) -> Result<Variant, gdscript_interop::bindings::ScriptError> {
            self.calls.borrow_mut().push(name.to_string());
            Ok(Variant::Nil)
        }
        fn get_property(&self, _name: &str) -> Option<Variant> {
            None
        }
        fn set_property(&mut self, _name: &str, _value: Variant) -> bool {
            false
        }
        fn list_methods(&self) -> Vec<gdscript_interop::bindings::MethodInfo> {
            Vec::new()
        }
        fn list_properties(&self) -> Vec<gdscript_interop::bindings::ScriptPropertyInfo> {
            Vec::new()
        }
        fn get_script_name(&self) -> &str {
            "ProcessProbe"
        }
        fn has_method(&self, _name: &str) -> bool {
            true
        }
    }

    /// The bead's acceptance, at the tree level: disabling `_process` on a node
    /// stops its `_process` callbacks across subsequent ticks while
    /// `_physics_process` keeps firing, and `request_ready` re-arms `_ready`.
    #[test]
    fn node_process_toggles_gate_callbacks() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let id = tree.add_child(root, Node::new("Probe", "Node")).unwrap();
        let calls = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        tree.attach_script(
            id,
            Box::new(ProcessProbe {
                calls: calls.clone(),
            }),
        );

        // Default: both callbacks dispatch (Godot enables them by default).
        assert!(tree.is_node_processing(id));
        assert!(tree.is_node_physics_processing(id));
        tree.process_all_scripts_process(0.016);
        tree.process_all_scripts_physics_process(0.016);
        assert_eq!(
            calls.borrow().as_slice(),
            ["_process", "_physics_process"],
            "both callbacks dispatch by default",
        );

        // set_process(false) stops _process only.
        calls.borrow_mut().clear();
        assert!(tree.set_node_process(id, false));
        assert!(!tree.is_node_processing(id));
        for _ in 0..3 {
            tree.process_all_scripts_process(0.016);
            tree.process_all_scripts_physics_process(0.016);
        }
        assert_eq!(
            calls.borrow().as_slice(),
            ["_physics_process", "_physics_process", "_physics_process"],
            "set_process(false) must stop _process across ticks while \
             _physics_process keeps running",
        );

        // Re-enabling resumes _process.
        calls.borrow_mut().clear();
        tree.set_node_process(id, true);
        tree.process_all_scripts_process(0.016);
        assert_eq!(
            calls.borrow().as_slice(),
            ["_process"],
            "toggle is reversible"
        );

        // set_physics_process(false) gates the other side independently.
        calls.borrow_mut().clear();
        tree.set_node_physics_process(id, false);
        assert!(!tree.is_node_physics_processing(id));
        tree.process_all_scripts_process(0.016);
        tree.process_all_scripts_physics_process(0.016);
        assert_eq!(
            calls.borrow().as_slice(),
            ["_process"],
            "set_physics_process(false) must stop only _physics_process",
        );

        // request_ready re-arms _ready for the next tree-enter.
        tree.get_node_mut(id).unwrap().set_ready(true);
        assert!(tree.get_node(id).unwrap().is_ready());
        assert!(tree.request_node_ready(id));
        assert!(
            !tree.get_node(id).unwrap().is_ready(),
            "request_ready() must clear the ready flag so _ready runs again",
        );
    }

    /// Toggles are per-node: silencing one node must not silence its siblings.
    #[test]
    fn process_toggles_are_per_node() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let a = tree.add_child(root, Node::new("A", "Node")).unwrap();
        let b = tree.add_child(root, Node::new("B", "Node")).unwrap();
        let a_calls = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let b_calls = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        tree.attach_script(
            a,
            Box::new(ProcessProbe {
                calls: a_calls.clone(),
            }),
        );
        tree.attach_script(
            b,
            Box::new(ProcessProbe {
                calls: b_calls.clone(),
            }),
        );

        tree.set_node_process(a, false);
        tree.process_all_scripts_process(0.016);

        assert!(a_calls.borrow().is_empty(), "A is silenced");
        assert_eq!(
            b_calls.borrow().as_slice(),
            ["_process"],
            "B still processes"
        );
    }

    /// An unknown node reports not-processing rather than panicking, and
    /// setting a toggle on it reports failure.
    #[test]
    fn process_toggles_on_unknown_node_are_safe() {
        let mut tree = SceneTree::new();
        let ghost = NodeId::from_object_id(gdcore::id::ObjectId::from_raw(999_999));
        assert!(!tree.is_node_processing(ghost));
        assert!(!tree.is_node_physics_processing(ghost));
        assert!(!tree.set_node_process(ghost, true));
        assert!(!tree.set_node_physics_process(ghost, true));
        assert!(!tree.request_node_ready(ghost));
    }

    #[test]
    fn new_tree_has_root() {
        let tree = SceneTree::new();
        assert_eq!(tree.node_count(), 1);
        let root = tree.get_node(tree.root_id()).unwrap();
        assert_eq!(root.name(), "root");
    }

    #[test]
    fn add_children_and_lookup_by_path() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let main = Node::new("Main", "Node");
        let main_id = tree.add_child(root, main).unwrap();

        let player = Node::new("Player", "Node2D");
        let player_id = tree.add_child(main_id, player).unwrap();

        assert_eq!(tree.node_count(), 3);

        // Absolute path lookup.
        assert_eq!(tree.get_node_by_path("/root/Main/Player"), Some(player_id));
        assert_eq!(tree.get_node_by_path("/root/Main"), Some(main_id));
        assert_eq!(tree.get_node_by_path("/root"), Some(root));
        assert_eq!(tree.get_node_by_path("/root/Missing"), None);
    }

    #[test]
    fn node_path_computation() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let a = Node::new("A", "Node");
        let a_id = tree.add_child(root, a).unwrap();

        let b = Node::new("B", "Node");
        let b_id = tree.add_child(a_id, b).unwrap();

        assert_eq!(tree.node_path(root).unwrap(), "/root");
        assert_eq!(tree.node_path(a_id).unwrap(), "/root/A");
        assert_eq!(tree.node_path(b_id).unwrap(), "/root/A/B");
    }

    #[test]
    fn relative_path_navigation() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let a = Node::new("A", "Node");
        let a_id = tree.add_child(root, a).unwrap();

        let b = Node::new("B", "Node");
        let b_id = tree.add_child(a_id, b).unwrap();

        assert_eq!(tree.get_node_relative(root, "A/B"), Some(b_id));
        assert_eq!(tree.get_node_relative(b_id, ".."), Some(a_id));
        assert_eq!(tree.get_node_relative(b_id, "../.."), Some(root));
    }

    #[test]
    fn remove_subtree() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let a = Node::new("A", "Node");
        let a_id = tree.add_child(root, a).unwrap();

        let b = Node::new("B", "Node");
        let _b_id = tree.add_child(a_id, b).unwrap();

        assert_eq!(tree.node_count(), 3);

        let removed = tree.remove_node(a_id).unwrap();
        assert_eq!(removed.len(), 2); // B then A
        assert_eq!(tree.node_count(), 1); // only root remains
    }

    #[test]
    fn add_child_to_live_parent_auto_enters_tree() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        tree.get_node_mut(root).unwrap().set_inside_tree(true);

        let child = Node::new("Child", "Node");
        let child_id = tree.add_child(root, child).unwrap();

        let child = tree.get_node(child_id).unwrap();
        assert!(child.is_inside_tree());
        assert!(child.is_ready());
        assert!(child
            .notification_log()
            .contains(&gdobject::NOTIFICATION_ENTER_TREE));
        assert!(child
            .notification_log()
            .contains(&gdobject::NOTIFICATION_READY));
    }

    #[test]
    fn remove_live_subtree_auto_exits_tree_before_removal() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        tree.get_node_mut(root).unwrap().set_inside_tree(true);

        let parent = Node::new("Parent", "Node");
        let parent_id = tree.add_child(root, parent).unwrap();
        let child = Node::new("Child", "Node");
        let child_id = tree.add_child(parent_id, child).unwrap();

        let removed = tree.remove_node(parent_id).unwrap();
        assert_eq!(removed, vec![child_id, parent_id]);
    }

    #[test]
    fn cannot_remove_root() {
        let mut tree = SceneTree::new();
        let result = tree.remove_node(tree.root_id());
        assert!(result.is_err());
    }

    #[test]
    fn reparent_node() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let a = Node::new("A", "Node");
        let a_id = tree.add_child(root, a).unwrap();

        let b = Node::new("B", "Node");
        let b_id = tree.add_child(root, b).unwrap();

        let c = Node::new("C", "Node");
        let c_id = tree.add_child(a_id, c).unwrap();

        // Move C from under A to under B.
        tree.reparent(c_id, b_id).unwrap();

        assert_eq!(tree.get_node(a_id).unwrap().children().len(), 0);
        assert_eq!(tree.get_node(b_id).unwrap().children(), &[c_id]);
        assert_eq!(tree.node_path(c_id).unwrap(), "/root/B/C");
        assert!(tree
            .get_node(c_id)
            .unwrap()
            .notification_log()
            .contains(&gdobject::NOTIFICATION_MOVED_IN_PARENT));
    }

    #[test]
    fn reparent_into_own_descendant_fails() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let a_id = tree.add_child(root, Node::new("A", "Node")).unwrap();
        let b_id = tree.add_child(a_id, Node::new("B", "Node")).unwrap();
        let c_id = tree.add_child(b_id, Node::new("C", "Node")).unwrap();

        // Reparenting A under its own descendant C would create a cycle.
        assert!(tree.reparent(a_id, c_id).is_err());
        // Reparenting a node under itself is also rejected.
        assert!(tree.reparent(a_id, a_id).is_err());

        // The hierarchy is left untouched after the rejected operations.
        assert_eq!(tree.get_node(a_id).unwrap().parent(), Some(root));
        assert_eq!(tree.get_node(b_id).unwrap().parent(), Some(a_id));
        assert_eq!(tree.get_node(c_id).unwrap().parent(), Some(b_id));
    }

    #[test]
    fn group_management() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let a = Node::new("A", "Node");
        let a_id = tree.add_child(root, a).unwrap();

        let b = Node::new("B", "Node");
        let b_id = tree.add_child(root, b).unwrap();

        tree.add_to_group(a_id, "enemies").unwrap();
        tree.add_to_group(b_id, "enemies").unwrap();
        tree.add_to_group(a_id, "visible").unwrap();

        let enemies = tree.get_nodes_in_group("enemies");
        assert_eq!(enemies.len(), 2);
        assert!(enemies.contains(&a_id));
        assert!(enemies.contains(&b_id));

        tree.remove_from_group(a_id, "enemies").unwrap();
        let enemies = tree.get_nodes_in_group("enemies");
        assert_eq!(enemies.len(), 1);
        assert!(enemies.contains(&b_id));
    }

    #[test]
    fn scene_tree_group_membership() {
        // Context-menu group editing: add/remove the selected node from named
        // groups, surface the group badge, and keep group lists consistent.
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let a_id = tree.add_child(root, Node::new("A", "Node")).unwrap();
        let b_id = tree.add_child(root, Node::new("B", "Node")).unwrap();

        // No groups yet → no badge on either row.
        assert!(!tree.has_group_membership(a_id));
        assert!(!tree.has_group_membership(b_id));
        assert!(tree.node_groups(a_id).is_empty());

        // Add A to two groups and B to one (shared with A).
        tree.add_to_group(a_id, "enemies").unwrap();
        tree.add_to_group(a_id, "visible").unwrap();
        tree.add_to_group(b_id, "enemies").unwrap();

        // Membership is recorded and the badge lights up.
        assert!(tree.node_in_group(a_id, "enemies"));
        assert!(tree.node_in_group(a_id, "visible"));
        assert!(tree.has_group_membership(a_id));
        assert!(tree.has_group_membership(b_id));
        // node_groups is the sorted per-node list shown in the menu.
        assert_eq!(tree.node_groups(a_id), vec!["enemies", "visible"]);
        assert_eq!(tree.node_groups(b_id), vec!["enemies"]);

        // Group lists stay consistent across nodes: "enemies" holds A and B.
        let enemies = tree.get_nodes_in_group("enemies");
        assert_eq!(enemies.len(), 2);
        assert!(enemies.contains(&a_id));
        assert!(enemies.contains(&b_id));

        // Remove A from "enemies": its membership clears and the group list
        // drops A, but B (and A's other group) are untouched.
        tree.remove_from_group(a_id, "enemies").unwrap();
        assert!(!tree.node_in_group(a_id, "enemies"));
        assert_eq!(tree.get_nodes_in_group("enemies"), vec![b_id]);
        // A still belongs to "visible", so its badge stays lit.
        assert!(tree.has_group_membership(a_id));
        assert_eq!(tree.node_groups(a_id), vec!["visible"]);

        // Removing A's last group clears the badge.
        tree.remove_from_group(a_id, "visible").unwrap();
        assert!(!tree.has_group_membership(a_id));
        assert!(tree.node_groups(a_id).is_empty());
        // B is unaffected throughout.
        assert!(tree.has_group_membership(b_id));
        assert!(tree.node_in_group(b_id, "enemies"));
    }

    #[test]
    fn scene_tree_group_indicator_locks_child_selection() {
        // The Scene Tree "group" toggle marks a node as a children-selection
        // lock: it sets the `_edit_group_` meta (badge), and picking any
        // descendant in the viewport resolves to the grouped ancestor.
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let parent = tree.add_child(root, Node::new("Parent", "Node2D")).unwrap();
        let child = tree
            .add_child(parent, Node::new("Child", "Node2D"))
            .unwrap();
        let grandchild = tree
            .add_child(child, Node::new("GrandChild", "Node2D"))
            .unwrap();

        // Ungrouped: no badge, and picks resolve to the clicked node itself.
        assert!(!tree.is_edit_group(parent));
        assert_eq!(tree.group_selection_target(child), child);
        assert_eq!(tree.group_selection_target(grandchild), grandchild);

        // Toggle the group button on Parent → sets `_edit_group_` meta + badge.
        assert!(tree.set_edit_group(parent, true));
        assert!(tree.is_edit_group(parent));
        assert!(matches!(
            tree.get_node(parent).unwrap().get_meta("_edit_group_"),
            Variant::Bool(true)
        ));

        // Clicking a grouped child (or deeper descendant) selects the grouped
        // ancestor; clicking the grouped node itself still selects it.
        assert_eq!(tree.group_selection_target(child), parent);
        assert_eq!(tree.group_selection_target(grandchild), parent);
        assert_eq!(tree.group_selection_target(parent), parent);

        // The outermost group wins: also group Child, and a grandchild pick
        // still resolves to the topmost grouped ancestor (Parent).
        assert!(tree.set_edit_group(child, true));
        assert_eq!(tree.group_selection_target(grandchild), parent);

        // Toggling Parent off: Child's group now governs grandchild picks, and
        // Parent loses its badge.
        assert!(tree.set_edit_group(parent, false));
        assert!(!tree.is_edit_group(parent));
        assert_eq!(tree.group_selection_target(grandchild), child);

        // Toggling the last group off restores per-node selection everywhere.
        assert!(tree.set_edit_group(child, false));
        assert_eq!(tree.group_selection_target(grandchild), grandchild);
        assert_eq!(tree.group_selection_target(child), child);
    }

    #[test]
    fn scene_tree_unique_name_indicator_reflects_flag() {
        // The "Access as Unique Name" toggle drives the `%` badge, and a
        // duplicate unique name within the same owner is flagged as a warning.
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let a = tree
            .add_child(root, Node::new("ContainerA", "Node"))
            .unwrap();
        let b = tree
            .add_child(root, Node::new("ContainerB", "Node"))
            .unwrap();
        let foo1 = tree.add_child(a, Node::new("Foo", "Node")).unwrap();
        let foo2 = tree.add_child(b, Node::new("Foo", "Node")).unwrap();
        // Scene nodes are owned by the scene root (the unique-name scope).
        for n in [a, b, foo1, foo2] {
            tree.get_node_mut(n).unwrap().set_owner(Some(root));
        }

        // No `%` badge and no collision until the flag is enabled.
        assert!(!tree.is_unique_name_in_owner(foo1));
        assert!(!tree.unique_name_collision(foo1));

        // Enable "Access as Unique Name" on foo1 → `%` badge shows.
        assert!(tree.set_unique_name_in_owner(foo1, true));
        assert!(tree.is_unique_name_in_owner(foo1));
        // Only one unique "Foo" in the owner → no collision yet.
        assert!(!tree.unique_name_collision(foo1));

        // Enabling the flag on the second "Foo" in the same owner creates a
        // duplicate unique name → flagged as a collision on both rows.
        assert!(tree.set_unique_name_in_owner(foo2, true));
        assert!(tree.is_unique_name_in_owner(foo2));
        assert!(tree.unique_name_collision(foo1));
        assert!(tree.unique_name_collision(foo2));

        // Disabling foo2's flag clears its badge and resolves the collision.
        assert!(tree.set_unique_name_in_owner(foo2, false));
        assert!(!tree.is_unique_name_in_owner(foo2));
        assert!(!tree.unique_name_collision(foo2));
        assert!(!tree.unique_name_collision(foo1));
    }

    /// Minimal script whose `_get_configuration_warnings` returns a fixed list,
    /// standing in for a node script that reports editor warnings.
    struct ConfigWarningScript {
        warnings: Vec<String>,
    }

    impl ScriptInstance for ConfigWarningScript {
        fn call_method(
            &mut self,
            name: &str,
            _args: &[Variant],
        ) -> Result<Variant, gdscript_interop::bindings::ScriptError> {
            if name == "_get_configuration_warnings" {
                Ok(Variant::Array(
                    self.warnings.iter().cloned().map(Variant::String).collect(),
                ))
            } else {
                Err(gdscript_interop::bindings::ScriptError::MethodNotFound(
                    name.to_string(),
                ))
            }
        }
        fn get_property(&self, _name: &str) -> Option<Variant> {
            None
        }
        fn set_property(&mut self, _name: &str, _value: Variant) -> bool {
            false
        }
        fn list_methods(&self) -> Vec<gdscript_interop::bindings::MethodInfo> {
            Vec::new()
        }
        fn list_properties(&self) -> Vec<gdscript_interop::bindings::ScriptPropertyInfo> {
            Vec::new()
        }
        fn get_script_name(&self) -> &str {
            "ConfigWarningScript"
        }
    }

    #[test]
    fn scene_tree_configuration_warning_surfaces_messages() {
        // A node whose `_get_configuration_warnings` returns messages shows the
        // warning triangle, and the messages are exposed for its tooltip.
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let clean = tree.add_child(root, Node::new("Clean", "Node2D")).unwrap();
        let warned = tree
            .add_child(root, Node::new("Broken", "CollisionShape2D"))
            .unwrap();

        // No script → no triangle, no messages.
        assert!(!tree.has_configuration_warning(clean));
        assert!(tree.node_configuration_warnings(clean).is_empty());

        // Attach a script reporting two configuration warnings.
        tree.attach_script(
            warned,
            Box::new(ConfigWarningScript {
                warnings: vec![
                    "A shape resource must be set.".to_string(),
                    "This node has no children.".to_string(),
                ],
            }),
        );

        // The triangle shows and the tooltip lists exactly those messages.
        assert!(tree.has_configuration_warning(warned));
        assert_eq!(
            tree.node_configuration_warnings(warned),
            vec![
                "A shape resource must be set.".to_string(),
                "This node has no children.".to_string(),
            ]
        );

        // A script that reports no warnings shows no triangle.
        tree.attach_script(
            clean,
            Box::new(ConfigWarningScript {
                warnings: Vec::new(),
            }),
        );
        assert!(!tree.has_configuration_warning(clean));
        assert!(tree.node_configuration_warnings(clean).is_empty());
    }

    #[test]
    fn process_frame_dispatches_notification() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let child = Node::new("Child", "Node");
        let child_id = tree.add_child(root, child).unwrap();

        tree.process_frame();

        let root_log = tree.get_node(root).unwrap().notification_log();
        // POSTINITIALIZE + CHILD_ORDER_CHANGED (add_child) + PROCESS (frame)
        assert!(root_log.contains(&gdobject::NOTIFICATION_POSTINITIALIZE));
        assert!(root_log.contains(&gdobject::NOTIFICATION_CHILD_ORDER_CHANGED));
        assert!(root_log.contains(&gdobject::NOTIFICATION_PROCESS));

        let child_log = tree.get_node(child_id).unwrap().notification_log();
        // POSTINITIALIZE + PARENTED + ENTER_TREE + READY + PROCESS
        assert!(child_log.contains(&gdobject::NOTIFICATION_POSTINITIALIZE));
        assert!(child_log.contains(&gdobject::NOTIFICATION_PARENTED));
        assert!(child_log.contains(&gdobject::NOTIFICATION_ENTER_TREE));
        assert!(child_log.contains(&gdobject::NOTIFICATION_READY));
        assert!(child_log.contains(&gdobject::NOTIFICATION_PROCESS));
    }

    #[test]
    fn add_child_to_nonexistent_parent_returns_error() {
        let mut tree = SceneTree::new();
        let fake_parent = crate::node::NodeId::next();
        let child = Node::new("Orphan", "Node");
        let result = tree.add_child(fake_parent, child);
        assert!(result.is_err());
    }

    #[test]
    fn remove_nonexistent_node_returns_error() {
        let mut tree = SceneTree::new();
        let fake_id = crate::node::NodeId::next();
        let result = tree.remove_node(fake_id);
        assert!(result.is_err());
    }

    #[test]
    fn deeply_nested_path_computation() {
        let mut tree = SceneTree::new();
        let mut parent_id = tree.root_id();
        let mut ids = vec![parent_id];

        for i in 0..12 {
            let node = Node::new(format!("L{i}"), "Node");
            let id = tree.add_child(parent_id, node).unwrap();
            ids.push(id);
            parent_id = id;
        }

        let deepest = *ids.last().unwrap();
        let path = tree.node_path(deepest).unwrap();
        assert!(path.starts_with("/root/L0/L1/L2"));
        assert!(path.ends_with("/L11"));

        let parts: Vec<&str> = path[1..].split('/').collect();
        assert_eq!(parts.len(), 13); // root + L0..L11
    }

    #[test]
    fn get_node_by_path_invalid_not_starting_with_slash() {
        let tree = SceneTree::new();
        assert_eq!(tree.get_node_by_path("root"), None);
    }

    #[test]
    fn get_node_by_path_wrong_root_name() {
        let tree = SceneTree::new();
        assert_eq!(tree.get_node_by_path("/wrong_root"), None);
    }

    #[test]
    fn relative_path_self_reference() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let node = Node::new("A", "Node");
        let a_id = tree.add_child(root, node).unwrap();
        assert_eq!(tree.get_node_relative(a_id, ""), Some(a_id));
    }

    #[test]
    fn reparent_root_fails() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let node = Node::new("A", "Node");
        let a_id = tree.add_child(root, node).unwrap();
        let result = tree.reparent(root, a_id);
        assert!(result.is_err());
    }

    #[test]
    fn reparent_nonexistent_node_fails() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let fake = crate::node::NodeId::next();
        assert!(tree.reparent(fake, root).is_err());
    }

    #[test]
    fn reparent_to_nonexistent_parent_fails() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let node = Node::new("A", "Node");
        let a_id = tree.add_child(root, node).unwrap();
        let fake = crate::node::NodeId::next();
        assert!(tree.reparent(a_id, fake).is_err());
    }

    #[test]
    fn physics_frame_dispatches_notification() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let child = Node::new("C", "Node");
        let child_id = tree.add_child(root, child).unwrap();

        tree.process_physics_frame();

        let log = tree.get_node(child_id).unwrap().notification_log();
        // POSTINITIALIZE + PARENTED + ENTER_TREE + READY + PHYSICS_PROCESS
        assert!(log.contains(&gdobject::NOTIFICATION_POSTINITIALIZE));
        assert!(log.contains(&gdobject::NOTIFICATION_PARENTED));
        assert!(log.contains(&gdobject::NOTIFICATION_ENTER_TREE));
        assert!(log.contains(&gdobject::NOTIFICATION_READY));
        assert!(log.contains(&gdobject::NOTIFICATION_PHYSICS_PROCESS));
    }

    #[test]
    fn default_scene_tree() {
        let tree = SceneTree::default();
        assert_eq!(tree.node_count(), 1);
    }

    #[test]
    fn node_path_of_nonexistent_returns_none() {
        let tree = SceneTree::new();
        let fake = crate::node::NodeId::next();
        assert!(tree.node_path(fake).is_none());
    }

    #[test]
    fn get_nodes_in_empty_group() {
        let tree = SceneTree::new();
        assert!(tree.get_nodes_in_group("nonexistent").is_empty());
    }

    #[test]
    fn remove_subtree_cleans_groups() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let node = Node::new("A", "Node");
        let a_id = tree.add_child(root, node).unwrap();
        tree.add_to_group(a_id, "enemies").unwrap();
        assert_eq!(tree.get_nodes_in_group("enemies").len(), 1);

        tree.remove_node(a_id).unwrap();
        assert!(tree.get_nodes_in_group("enemies").is_empty());
    }

    #[test]
    fn unicode_node_names_in_tree() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let node = Node::new("プレイヤー", "Node2D");
        let id = tree.add_child(root, node).unwrap();
        assert_eq!(tree.node_path(id).unwrap(), "/root/プレイヤー");
        assert_eq!(tree.get_node_by_path("/root/プレイヤー"), Some(id));
    }

    #[test]
    fn empty_name_node_in_tree() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let node = Node::new("", "Node");
        let id = tree.add_child(root, node).unwrap();
        assert_eq!(tree.node_path(id).unwrap(), "/root/");
    }

    // -- AnimationPlayer store tests ----------------------------------------

    mod animation_store_tests {
        use super::*;
        use crate::animation::{
            Animation, AnimationPlayer, AnimationTrack, KeyFrame, LoopMode, TrackType,
        };
        use gdcore::math::{Vector2, Vector3};
        use gdvariant::Variant;

        /// Helper: build a tree with root + one Node2D child.
        fn tree_with_node() -> (SceneTree, NodeId) {
            let mut tree = SceneTree::new();
            let root = tree.root_id();
            let node = Node::new("Sprite", "Node2D");
            let id = tree.add_child(root, node).unwrap();
            (tree, id)
        }

        /// Script mock that records every `call_method` invocation, used to
        /// assert method-track dispatch.
        struct RecordingScript {
            calls: std::rc::Rc<std::cell::RefCell<Vec<(String, Vec<Variant>)>>>,
        }

        impl gdscript_interop::bindings::ScriptInstance for RecordingScript {
            fn call_method(
                &mut self,
                name: &str,
                args: &[Variant],
            ) -> Result<Variant, gdscript_interop::bindings::ScriptError> {
                self.calls
                    .borrow_mut()
                    .push((name.to_string(), args.to_vec()));
                Ok(Variant::Nil)
            }
            fn get_property(&self, _name: &str) -> Option<Variant> {
                None
            }
            fn set_property(&mut self, _name: &str, _value: Variant) -> bool {
                false
            }
            fn list_methods(&self) -> Vec<gdscript_interop::bindings::MethodInfo> {
                Vec::new()
            }
            fn list_properties(&self) -> Vec<gdscript_interop::bindings::ScriptPropertyInfo> {
                Vec::new()
            }
            fn get_script_name(&self) -> &str {
                "RecordingScript"
            }
            fn has_method(&self, _name: &str) -> bool {
                true
            }
        }

        #[test]
        fn attach_and_get_animation_player() {
            let (mut tree, id) = tree_with_node();
            let player = AnimationPlayer::new();
            tree.attach_animation_player(id, player).unwrap();
            assert!(tree.get_animation_player(id).is_some());
        }

        #[test]
        fn attach_animation_player_to_nonexistent_node_fails() {
            let mut tree = SceneTree::new();
            let fake = NodeId::next();
            let result = tree.attach_animation_player(fake, AnimationPlayer::new());
            assert!(result.is_err());
        }

        #[test]
        fn get_animation_player_mut_works() {
            let (mut tree, id) = tree_with_node();
            let player = AnimationPlayer::new();
            tree.attach_animation_player(id, player).unwrap();
            let p = tree.get_animation_player_mut(id).unwrap();
            p.speed_scale = 2.0;
            assert_eq!(tree.get_animation_player(id).unwrap().speed_scale, 2.0);
        }

        // -- animation trees (pat-0q2kx.2) ----------------------------------

        /// A scene `Owner -> {Sprite, Tree}` plus a one-track clip on `Sprite:x`,
        /// so a tree attached to `Tree` reaches `Sprite` through root_node "..".
        fn tree_with_animation_tree(
            crossfade_clip: f64,
        ) -> (
            SceneTree,
            NodeId,
            NodeId,
            crate::animation_tree::AnimationTree,
        ) {
            let mut tree = SceneTree::new();
            let root = tree.root_id();
            let owner = tree.add_child(root, Node::new("Owner", "Node2D")).unwrap();
            let sprite = tree
                .add_child(owner, Node::new("Sprite", "Node2D"))
                .unwrap();
            let tree_node = tree
                .add_child(owner, Node::new("Tree", "AnimationTree"))
                .unwrap();

            let mut anim = Animation::new("run", crossfade_clip);
            let mut track = AnimationTrack::with_node("Sprite", "x");
            track.add_keyframe(KeyFrame::linear(0.0, Variant::Float(0.0)));
            track.add_keyframe(KeyFrame::linear(crossfade_clip, Variant::Float(100.0)));
            anim.tracks.push(track);

            let mut anim_tree = crate::animation_tree::AnimationTree::new();
            anim_tree.add_animation(anim);
            anim_tree.set_root(crate::animation_tree::AnimationNode::Animation {
                name: "run".into(),
                position: 0.0,
            });
            (tree, sprite, tree_node, anim_tree)
        }

        #[test]
        fn attach_and_get_animation_tree() {
            let (mut tree, id) = tree_with_node();
            tree.attach_animation_tree(id, crate::animation_tree::AnimationTree::new())
                .unwrap();
            assert!(tree.get_animation_tree(id).is_some());
            assert_eq!(tree.animation_tree_count(), 1);
        }

        #[test]
        fn attach_animation_tree_to_nonexistent_node_fails() {
            let mut tree = SceneTree::new();
            let fake = NodeId::next();
            let result =
                tree.attach_animation_tree(fake, crate::animation_tree::AnimationTree::new());
            assert!(result.is_err());
        }

        #[test]
        fn process_animation_trees_writes_through_root_node_to_track_targets() {
            let (mut tree, sprite, tree_node, anim_tree) = tree_with_animation_tree(1.0);
            tree.attach_animation_tree(tree_node, anim_tree).unwrap();

            tree.process_animation_trees(0.5);

            // root_node ".." resolves to Owner, then the track's "Sprite" path —
            // the value must land on the sprite, not on the AnimationTree node.
            match tree.get_node(sprite).unwrap().get_property("x") {
                Variant::Float(x) => assert!((x - 50.0).abs() < 1.0, "x={x}"),
                other => panic!("Sprite:x not driven by the tree: {other:?}"),
            }
            assert_eq!(
                tree.get_node(tree_node).unwrap().get_property("x"),
                Variant::Nil,
                "the pose must not be written to the AnimationTree node itself"
            );
        }

        #[test]
        fn process_animation_trees_skips_inactive_trees() {
            let (mut tree, sprite, tree_node, mut anim_tree) = tree_with_animation_tree(1.0);
            anim_tree.active = false;
            tree.attach_animation_tree(tree_node, anim_tree).unwrap();

            tree.process_animation_trees(0.5);

            assert_eq!(
                tree.get_node(sprite).unwrap().get_property("x"),
                Variant::Nil,
                "an inactive tree must not write anything"
            );
        }

        #[test]
        fn changing_scenes_drops_attached_animation_trees() {
            let (mut tree, _sprite, tree_node, anim_tree) = tree_with_animation_tree(1.0);
            tree.attach_animation_tree(tree_node, anim_tree).unwrap();
            assert_eq!(tree.animation_tree_count(), 1);

            tree.change_scene_to_node(Node::new("Next", "Node"))
                .unwrap();
            assert_eq!(
                tree.animation_tree_count(),
                0,
                "trees of a freed scene must not keep ticking"
            );
        }

        #[test]
        fn process_animations_applies_float_property() {
            let (mut tree, id) = tree_with_node();

            let mut player = AnimationPlayer::new();
            let mut anim = Animation::new("fade", 1.0);
            let mut track = AnimationTrack::new("opacity");
            track.add_keyframe(KeyFrame::linear(0.0, Variant::Float(1.0)));
            track.add_keyframe(KeyFrame::linear(1.0, Variant::Float(0.0)));
            anim.tracks.push(track);
            player.add_animation(anim);
            player.play("fade");

            tree.attach_animation_player(id, player).unwrap();

            // Advance half-way.
            tree.process_animations(0.5);

            let val = tree.get_node(id).unwrap().get_property("opacity");
            if let Variant::Float(f) = val {
                assert!((f - 0.5).abs() < 1e-4, "got {f}");
            } else {
                panic!("expected Float, got {val:?}");
            }
        }

        #[test]
        fn process_animations_applies_vector2_position() {
            let (mut tree, id) = tree_with_node();

            let mut player = AnimationPlayer::new();
            let mut anim = Animation::new("move", 1.0);
            let mut track = AnimationTrack::new("position");
            track.add_keyframe(KeyFrame::linear(0.0, Variant::Vector2(Vector2::ZERO)));
            track.add_keyframe(KeyFrame::linear(
                1.0,
                Variant::Vector2(Vector2::new(100.0, 200.0)),
            ));
            anim.tracks.push(track);
            player.add_animation(anim);
            player.play("move");

            tree.attach_animation_player(id, player).unwrap();
            tree.process_animations(0.5);

            let val = tree.get_node(id).unwrap().get_property("position");
            if let Variant::Vector2(v) = val {
                assert!((v.x - 50.0).abs() < 1e-3);
                assert!((v.y - 100.0).abs() < 1e-3);
            } else {
                panic!("expected Vector2, got {val:?}");
            }
        }

        #[test]
        fn process_animations_dispatches_method_track() {
            let (mut tree, id) = tree_with_node();
            let calls = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
            tree.attach_script(
                id,
                Box::new(RecordingScript {
                    calls: calls.clone(),
                }),
            );

            let mut player = AnimationPlayer::new();
            let mut anim = Animation::new("fx", 2.0);
            let mut track = AnimationTrack::with_type("", "hit", TrackType::Method);
            track.add_keyframe(KeyFrame::linear(
                1.0,
                Variant::Array(vec![Variant::String("on_footstep".into()), Variant::Int(3)]),
            ));
            anim.tracks.push(track);
            player.add_animation(anim);
            player.play("fx");
            tree.attach_animation_player(id, player).unwrap();

            // Advancing past t=1.0 fires the method track exactly once.
            tree.process_animations(1.5);
            {
                let c = calls.borrow();
                assert_eq!(c.len(), 1, "method track should fire once");
                assert_eq!(c[0].0, "on_footstep");
                assert_eq!(c[0].1, vec![Variant::Int(3)]);
            }

            // Advancing further (without re-crossing the keyframe) does not
            // fire it again.
            tree.process_animations(0.4);
            assert_eq!(calls.borrow().len(), 1, "keyframe must not re-fire");

            // A method track is never written to the node as a property.
            assert_eq!(tree.get_node(id).unwrap().get_property("hit"), Variant::Nil);
        }

        #[test]
        fn process_animations_applies_root_motion_delta() {
            let (mut tree, id) = tree_with_node();
            // Seed a starting position on the node.
            tree.get_node_mut(id)
                .unwrap()
                .set_property("position", Variant::Vector3(Vector3::new(10.0, 0.0, 0.0)));

            let mut player = AnimationPlayer::new();
            let mut anim = Animation::new("walk", 2.0);
            let mut track = AnimationTrack::with_type("", "position", TrackType::Property);
            track.add_keyframe(KeyFrame::linear(0.0, Variant::Vector3(Vector3::ZERO)));
            track.add_keyframe(KeyFrame::linear(
                2.0,
                Variant::Vector3(Vector3::new(4.0, 0.0, 0.0)),
            ));
            anim.tracks.push(track);
            player.add_animation(anim);
            player.play("walk");
            player.root_motion_track = Some("position".to_string());
            tree.attach_animation_player(id, player).unwrap();

            // At t=1.0 the linear track delta is half of (4,0,0) = (2,0,0),
            // added onto the seeded position → (12,0,0). The track is applied
            // as a delta, never as its absolute (2,0,0) value.
            tree.process_animations(1.0);
            let pos = tree.get_node(id).unwrap().get_property("position");
            if let Variant::Vector3(v) = pos {
                assert!(
                    (v.x - 12.0).abs() < 1e-3,
                    "expected root-motion translation, got {v:?}"
                );
                assert!((v.y).abs() < 1e-3 && (v.z).abs() < 1e-3, "got {v:?}");
            } else {
                panic!("expected Vector3, got {pos:?}");
            }
        }

        #[test]
        fn transform_tracks_drive_node3d_trs() {
            use gdcore::math3d::Quaternion;
            let (mut tree, id) = tree_with_node();

            let mut player = AnimationPlayer::new();
            let mut anim = Animation::new("clip", 1.0);

            // Position3D → node "position" (Vector3 lerp).
            let mut pos = AnimationTrack::with_type("", "position", TrackType::Position3D);
            pos.add_keyframe(KeyFrame::linear(0.0, Variant::Vector3(Vector3::ZERO)));
            pos.add_keyframe(KeyFrame::linear(
                1.0,
                Variant::Vector3(Vector3::new(10.0, 0.0, 0.0)),
            ));
            anim.tracks.push(pos);

            // Rotation3D → node "quaternion" (shortest-path slerp).
            let mut rot = AnimationTrack::with_type("", "quaternion", TrackType::Rotation3D);
            rot.add_keyframe(KeyFrame::linear(
                0.0,
                Variant::Quaternion(Quaternion::IDENTITY),
            ));
            rot.add_keyframe(KeyFrame::linear(
                1.0,
                Variant::Quaternion(Quaternion::from_axis_angle(
                    Vector3::new(0.0, 1.0, 0.0),
                    std::f32::consts::FRAC_PI_2,
                )),
            ));
            anim.tracks.push(rot);

            // BlendShape → weight property (float lerp).
            let mut bs = AnimationTrack::with_type("", "blend_shapes/Smile", TrackType::BlendShape);
            bs.add_keyframe(KeyFrame::linear(0.0, Variant::Float(0.0)));
            bs.add_keyframe(KeyFrame::linear(1.0, Variant::Float(1.0)));
            anim.tracks.push(bs);

            player.add_animation(anim);
            player.play("clip");
            tree.attach_animation_player(id, player).unwrap();

            tree.process_animations(0.5);
            let node = tree.get_node(id).unwrap();

            match node.get_property("position") {
                Variant::Vector3(v) => {
                    assert!((v.x - 5.0).abs() < 1e-3, "position drives TRS, got {v:?}")
                }
                other => panic!("expected Vector3 position, got {other:?}"),
            }
            match node.get_property("quaternion") {
                Variant::Quaternion(q) => assert!(
                    (q.length() - 1.0).abs() < 1e-3,
                    "rotation drives TRS (normalized), len={}",
                    q.length()
                ),
                other => panic!("expected Quaternion, got {other:?}"),
            }
            match node.get_property("blend_shapes/Smile") {
                Variant::Float(w) => {
                    assert!((w - 0.5).abs() < 1e-3, "blend-shape weight set, got {w}")
                }
                other => panic!("expected Float weight, got {other:?}"),
            }
        }

        #[test]
        fn multiple_animations_on_different_nodes() {
            let mut tree = SceneTree::new();
            let root = tree.root_id();

            let a = Node::new("A", "Node2D");
            let a_id = tree.add_child(root, a).unwrap();

            let b = Node::new("B", "Node2D");
            let b_id = tree.add_child(root, b).unwrap();

            // Player for A: animate "x"
            let mut pa = AnimationPlayer::new();
            let mut anim_a = Animation::new("go", 1.0);
            let mut track_a = AnimationTrack::new("x");
            track_a.add_keyframe(KeyFrame::linear(0.0, Variant::Float(0.0)));
            track_a.add_keyframe(KeyFrame::linear(1.0, Variant::Float(10.0)));
            anim_a.tracks.push(track_a);
            pa.add_animation(anim_a);
            pa.play("go");
            tree.attach_animation_player(a_id, pa).unwrap();

            // Player for B: animate "y"
            let mut pb = AnimationPlayer::new();
            let mut anim_b = Animation::new("go", 1.0);
            let mut track_b = AnimationTrack::new("y");
            track_b.add_keyframe(KeyFrame::linear(0.0, Variant::Float(100.0)));
            track_b.add_keyframe(KeyFrame::linear(1.0, Variant::Float(200.0)));
            anim_b.tracks.push(track_b);
            pb.add_animation(anim_b);
            pb.play("go");
            tree.attach_animation_player(b_id, pb).unwrap();

            tree.process_animations(0.5);

            if let Variant::Float(f) = tree.get_node(a_id).unwrap().get_property("x") {
                assert!((f - 5.0).abs() < 1e-4);
            } else {
                panic!("A.x not Float");
            }
            if let Variant::Float(f) = tree.get_node(b_id).unwrap().get_property("y") {
                assert!((f - 150.0).abs() < 1e-4);
            } else {
                panic!("B.y not Float");
            }
        }

        #[test]
        fn animation_stops_at_end_no_loop() {
            let (mut tree, id) = tree_with_node();

            let mut player = AnimationPlayer::new();
            let mut anim = Animation::new("once", 1.0);
            let mut track = AnimationTrack::new("val");
            track.add_keyframe(KeyFrame::linear(0.0, Variant::Float(0.0)));
            track.add_keyframe(KeyFrame::linear(1.0, Variant::Float(10.0)));
            anim.tracks.push(track);
            player.add_animation(anim);
            player.play("once");

            tree.attach_animation_player(id, player).unwrap();

            // Advance past end.
            tree.process_animations(2.0);

            assert!(!tree.get_animation_player(id).unwrap().playing);
            if let Variant::Float(f) = tree.get_node(id).unwrap().get_property("val") {
                assert!((f - 10.0).abs() < 1e-4);
            } else {
                panic!("expected Float");
            }
        }

        #[test]
        fn animation_loop_linear_wraps() {
            let (mut tree, id) = tree_with_node();

            let mut player = AnimationPlayer::new();
            let mut anim = Animation::new("loop", 1.0);
            anim.loop_mode = LoopMode::Linear;
            let mut track = AnimationTrack::new("val");
            track.add_keyframe(KeyFrame::linear(0.0, Variant::Float(0.0)));
            track.add_keyframe(KeyFrame::linear(1.0, Variant::Float(10.0)));
            anim.tracks.push(track);
            player.add_animation(anim);
            player.play("loop");

            tree.attach_animation_player(id, player).unwrap();

            // Advance 1.5s — should wrap to 0.5
            tree.process_animations(1.5);

            assert!(tree.get_animation_player(id).unwrap().playing);
            if let Variant::Float(f) = tree.get_node(id).unwrap().get_property("val") {
                assert!((f - 5.0).abs() < 1e-4, "expected ~5.0, got {f}");
            } else {
                panic!("expected Float");
            }
        }

        #[test]
        fn animation_pingpong_reverses() {
            let (mut tree, id) = tree_with_node();

            let mut player = AnimationPlayer::new();
            let mut anim = Animation::new("pp", 1.0);
            anim.loop_mode = LoopMode::PingPong;
            let mut track = AnimationTrack::new("val");
            track.add_keyframe(KeyFrame::linear(0.0, Variant::Float(0.0)));
            track.add_keyframe(KeyFrame::linear(1.0, Variant::Float(10.0)));
            anim.tracks.push(track);
            player.add_animation(anim);
            player.play("pp");

            tree.attach_animation_player(id, player).unwrap();

            // Advance 1.5s — should bounce back to position 0.5
            tree.process_animations(1.5);

            assert!(tree.get_animation_player(id).unwrap().playing);
            if let Variant::Float(f) = tree.get_node(id).unwrap().get_property("val") {
                assert!((f - 5.0).abs() < 1e-4, "expected ~5.0, got {f}");
            } else {
                panic!("expected Float");
            }
        }

        #[test]
        fn animation_multiple_tracks_applied() {
            let (mut tree, id) = tree_with_node();

            let mut player = AnimationPlayer::new();
            let mut anim = Animation::new("multi", 1.0);

            let mut t1 = AnimationTrack::new("x");
            t1.add_keyframe(KeyFrame::linear(0.0, Variant::Float(0.0)));
            t1.add_keyframe(KeyFrame::linear(1.0, Variant::Float(10.0)));
            anim.tracks.push(t1);

            let mut t2 = AnimationTrack::new("y");
            t2.add_keyframe(KeyFrame::linear(0.0, Variant::Float(100.0)));
            t2.add_keyframe(KeyFrame::linear(1.0, Variant::Float(200.0)));
            anim.tracks.push(t2);

            player.add_animation(anim);
            player.play("multi");
            tree.attach_animation_player(id, player).unwrap();

            tree.process_animations(0.5);

            if let Variant::Float(f) = tree.get_node(id).unwrap().get_property("x") {
                assert!((f - 5.0).abs() < 1e-4);
            } else {
                panic!("x not Float");
            }
            if let Variant::Float(f) = tree.get_node(id).unwrap().get_property("y") {
                assert!((f - 150.0).abs() < 1e-4);
            } else {
                panic!("y not Float");
            }
        }

        #[test]
        fn stopped_animation_player_does_not_advance() {
            let (mut tree, id) = tree_with_node();

            let mut player = AnimationPlayer::new();
            let mut anim = Animation::new("a", 1.0);
            let mut track = AnimationTrack::new("val");
            track.add_keyframe(KeyFrame::linear(0.0, Variant::Float(0.0)));
            track.add_keyframe(KeyFrame::linear(1.0, Variant::Float(10.0)));
            anim.tracks.push(track);
            player.add_animation(anim);
            // Don't call play() — player is not playing.
            tree.attach_animation_player(id, player).unwrap();

            tree.process_animations(0.5);

            // Property should not have been set (no current animation values).
            assert_eq!(tree.get_node(id).unwrap().get_property("val"), Variant::Nil);
        }
    }

    // -- Tween store tests --------------------------------------------------

    mod tween_store_tests {
        use super::*;
        use crate::tween::{TweenBuilder, TweenId};
        use gdcore::math::Vector2;
        use gdvariant::Variant;

        fn tree_with_node() -> (SceneTree, NodeId) {
            let mut tree = SceneTree::new();
            let root = tree.root_id();
            let node = Node::new("Player", "Node2D");
            let id = tree.add_child(root, node).unwrap();
            (tree, id)
        }

        #[test]
        fn create_tween_returns_id() {
            let (mut tree, id) = tree_with_node();
            let tid = tree.create_tween(id);
            assert!(tree.get_tween_mut(tid).is_some());
            assert_eq!(tree.tween_count(), 1);
        }

        #[test]
        fn add_tween_with_builder() {
            let (mut tree, id) = tree_with_node();
            let tween = TweenBuilder::new()
                .tween_property("x", Variant::Float(0.0), Variant::Float(100.0), 1.0)
                .build();
            let tid = tree.add_tween(id, tween);
            assert!(tree.get_tween_mut(tid).is_some());
        }

        #[test]
        fn process_tweens_applies_float_property() {
            let (mut tree, id) = tree_with_node();
            let tween = TweenBuilder::new()
                .tween_property("speed", Variant::Float(0.0), Variant::Float(100.0), 1.0)
                .build();
            tree.add_tween(id, tween);

            tree.process_tweens(0.5);

            if let Variant::Float(f) = tree.get_node(id).unwrap().get_property("speed") {
                assert!((f - 50.0).abs() < 1e-3, "got {f}");
            } else {
                panic!("expected Float");
            }
        }

        #[test]
        fn tween_node2d_position_updates() {
            let (mut tree, id) = tree_with_node();
            let tween = TweenBuilder::new()
                .tween_property(
                    "position",
                    Variant::Vector2(Vector2::ZERO),
                    Variant::Vector2(Vector2::new(100.0, 200.0)),
                    1.0,
                )
                .build();
            tree.add_tween(id, tween);

            tree.process_tweens(0.5);

            if let Variant::Vector2(v) = tree.get_node(id).unwrap().get_property("position") {
                assert!((v.x - 50.0).abs() < 1e-3);
                assert!((v.y - 100.0).abs() < 1e-3);
            } else {
                panic!("expected Vector2");
            }
        }

        #[test]
        fn tween_completion_removes_from_store() {
            let (mut tree, id) = tree_with_node();
            let tween = TweenBuilder::new()
                .tween_property("x", Variant::Float(0.0), Variant::Float(10.0), 1.0)
                .build();
            tree.add_tween(id, tween);
            assert_eq!(tree.tween_count(), 1);

            // Advance past completion.
            tree.process_tweens(2.0);
            assert_eq!(tree.tween_count(), 0);
        }

        #[test]
        fn tween_looping_does_not_remove() {
            let (mut tree, id) = tree_with_node();
            let tween = TweenBuilder::new()
                .tween_property("x", Variant::Float(0.0), Variant::Float(10.0), 1.0)
                .set_loops(-1)
                .build();
            tree.add_tween(id, tween);

            tree.process_tweens(5.0);
            assert_eq!(tree.tween_count(), 1, "infinite-loop tween should persist");
        }

        #[test]
        fn tween_finite_loops_removes_after_all() {
            let (mut tree, id) = tree_with_node();
            let tween = TweenBuilder::new()
                .tween_property("x", Variant::Float(0.0), Variant::Float(10.0), 1.0)
                .set_loops(2)
                .build();
            tree.add_tween(id, tween);

            tree.process_tweens(1.0); // First loop done
            assert_eq!(
                tree.tween_count(),
                1,
                "should still be alive after first loop"
            );

            tree.process_tweens(1.0); // Second loop done
            assert_eq!(tree.tween_count(), 0, "should be removed after all loops");
        }

        #[test]
        fn multiple_tweens_on_different_nodes() {
            let mut tree = SceneTree::new();
            let root = tree.root_id();

            let a = Node::new("A", "Node2D");
            let a_id = tree.add_child(root, a).unwrap();

            let b = Node::new("B", "Node2D");
            let b_id = tree.add_child(root, b).unwrap();

            let tw_a = TweenBuilder::new()
                .tween_property("x", Variant::Float(0.0), Variant::Float(10.0), 1.0)
                .build();
            tree.add_tween(a_id, tw_a);

            let tw_b = TweenBuilder::new()
                .tween_property("y", Variant::Float(50.0), Variant::Float(150.0), 1.0)
                .build();
            tree.add_tween(b_id, tw_b);

            tree.process_tweens(0.5);

            if let Variant::Float(f) = tree.get_node(a_id).unwrap().get_property("x") {
                assert!((f - 5.0).abs() < 1e-3);
            } else {
                panic!("A.x not Float");
            }
            if let Variant::Float(f) = tree.get_node(b_id).unwrap().get_property("y") {
                assert!((f - 100.0).abs() < 1e-3);
            } else {
                panic!("B.y not Float");
            }
        }

        #[test]
        fn tween_end_value_applied_on_completion() {
            let (mut tree, id) = tree_with_node();
            let tween = TweenBuilder::new()
                .tween_property("x", Variant::Float(0.0), Variant::Float(42.0), 1.0)
                .build();
            tree.add_tween(id, tween);

            tree.process_tweens(1.0);

            if let Variant::Float(f) = tree.get_node(id).unwrap().get_property("x") {
                assert!((f - 42.0).abs() < 1e-4);
            } else {
                panic!("expected Float");
            }
        }

        #[test]
        fn get_nonexistent_tween_returns_none() {
            let mut tree = SceneTree::new();
            let fake = TweenId::next();
            assert!(tree.get_tween_mut(fake).is_none());
        }
    }

    // -- MainLoop animation/tween integration tests -------------------------

    mod mainloop_integration_tests {
        use crate::animation::{Animation, AnimationPlayer, AnimationTrack, KeyFrame};
        use crate::main_loop::MainLoop;
        use crate::node::Node;
        use crate::scene_tree::SceneTree;
        use crate::tween::TweenBuilder;
        use gdcore::math::Vector2;
        use gdvariant::Variant;

        #[test]
        fn mainloop_advances_animation_player() {
            let mut tree = SceneTree::new();
            let root = tree.root_id();
            let node = Node::new("N", "Node2D");
            let id = tree.add_child(root, node).unwrap();

            let mut player = AnimationPlayer::new();
            let mut anim = Animation::new("slide", 1.0);
            let mut track = AnimationTrack::new("position");
            track.add_keyframe(KeyFrame::linear(0.0, Variant::Vector2(Vector2::ZERO)));
            track.add_keyframe(KeyFrame::linear(
                1.0,
                Variant::Vector2(Vector2::new(60.0, 0.0)),
            ));
            anim.tracks.push(track);
            player.add_animation(anim);
            player.play("slide");
            tree.attach_animation_player(id, player).unwrap();

            let mut ml = MainLoop::new(tree);
            // Run 30 frames at 1/60 each = 0.5s
            ml.run_frames(30, 1.0 / 60.0);

            let pos = ml.tree().get_node(id).unwrap().get_property("position");
            if let Variant::Vector2(v) = pos {
                assert!((v.x - 30.0).abs() < 1.0, "expected ~30, got {}", v.x);
            } else {
                panic!("expected Vector2, got {pos:?}");
            }
        }

        #[test]
        fn mainloop_advances_tween() {
            let mut tree = SceneTree::new();
            let root = tree.root_id();
            let node = Node::new("N", "Node2D");
            let id = tree.add_child(root, node).unwrap();

            let tween = TweenBuilder::new()
                .tween_property(
                    "position",
                    Variant::Vector2(Vector2::ZERO),
                    Variant::Vector2(Vector2::new(60.0, 0.0)),
                    1.0,
                )
                .build();
            tree.add_tween(id, tween);

            let mut ml = MainLoop::new(tree);
            ml.run_frames(30, 1.0 / 60.0);

            let pos = ml.tree().get_node(id).unwrap().get_property("position");
            if let Variant::Vector2(v) = pos {
                assert!((v.x - 30.0).abs() < 1.0, "expected ~30, got {}", v.x);
            } else {
                panic!("expected Vector2, got {pos:?}");
            }
        }

        #[test]
        fn mainloop_tween_completes_and_is_removed() {
            let mut tree = SceneTree::new();
            let root = tree.root_id();
            let node = Node::new("N", "Node2D");
            let id = tree.add_child(root, node).unwrap();

            let tween = TweenBuilder::new()
                .tween_property("x", Variant::Float(0.0), Variant::Float(10.0), 0.5)
                .build();
            tree.add_tween(id, tween);

            let mut ml = MainLoop::new(tree);
            // Run 60 frames at 1/60 = 1.0s (tween is 0.5s)
            ml.run_frames(60, 1.0 / 60.0);

            assert_eq!(ml.tree().tween_count(), 0);
            if let Variant::Float(f) = ml.tree().get_node(id).unwrap().get_property("x") {
                assert!((f - 10.0).abs() < 1e-4);
            } else {
                panic!("expected Float");
            }
        }
    }

    // -- Runtime node creation / deletion tests ----------------------------

    mod runtime_node_tests {
        use super::*;

        #[test]
        fn create_node_adds_to_arena() {
            let mut tree = SceneTree::new();
            let id = tree.create_node("Node2D", "Bullet");
            assert!(tree.get_node(id).is_some());
            let node = tree.get_node(id).unwrap();
            assert_eq!(node.name(), "Bullet");
            assert_eq!(node.class_name(), "Node2D");
            // Not yet parented.
            assert!(node.parent().is_none());
        }

        #[test]
        fn take_node_removes_from_arena() {
            let mut tree = SceneTree::new();
            let id = tree.create_node("Node2D", "Temp");
            assert!(tree.get_node(id).is_some());

            let node = tree.take_node(id);
            assert!(node.is_some());
            assert!(tree.get_node(id).is_none());
        }

        #[test]
        fn create_then_add_child() {
            let mut tree = SceneTree::new();
            let root = tree.root_id();
            tree.get_node_mut(root).unwrap().set_inside_tree(true);

            let id = tree.create_node("Node2D", "Bullet");
            let node = tree.take_node(id).unwrap();
            let added_id = tree.add_child(root, node).unwrap();
            assert_eq!(id, added_id);

            let child = tree.get_node(id).unwrap();
            assert_eq!(child.parent(), Some(root));
            assert!(child.is_inside_tree());
        }

        #[test]
        fn queue_free_defers_deletion() {
            let mut tree = SceneTree::new();
            let root = tree.root_id();

            let n = Node::new("X", "Node");
            let nid = tree.add_child(root, n).unwrap();

            tree.queue_free(nid);
            // Still alive.
            assert!(tree.get_node(nid).is_some());
            assert_eq!(tree.pending_deletion_count(), 1);

            tree.process_deletions();
            assert!(tree.get_node(nid).is_none());
            assert_eq!(tree.node_count(), 1); // Only root.
        }

        #[test]
        fn queue_free_subtree() {
            let mut tree = SceneTree::new();
            let root = tree.root_id();

            let a = Node::new("A", "Node");
            let a_id = tree.add_child(root, a).unwrap();
            let b = Node::new("B", "Node");
            let b_id = tree.add_child(a_id, b).unwrap();

            tree.queue_free(a_id);
            tree.process_deletions();

            assert!(tree.get_node(a_id).is_none());
            assert!(tree.get_node(b_id).is_none());
            assert_eq!(tree.node_count(), 1);
        }

        #[test]
        fn queue_free_already_removed_child_is_safe() {
            let mut tree = SceneTree::new();
            let root = tree.root_id();

            let a = Node::new("A", "Node");
            let a_id = tree.add_child(root, a).unwrap();
            let b = Node::new("B", "Node");
            let b_id = tree.add_child(a_id, b).unwrap();

            // Queue both parent and child for deletion.
            tree.queue_free(b_id);
            tree.queue_free(a_id);
            // Should not panic — b_id is already gone when a_id is removed.
            tree.process_deletions();

            assert!(tree.get_node(a_id).is_none());
            assert!(tree.get_node(b_id).is_none());
        }

        #[test]
        fn process_deletions_clears_pending_list() {
            let mut tree = SceneTree::new();
            let root = tree.root_id();

            let n = Node::new("N", "Node");
            let nid = tree.add_child(root, n).unwrap();

            tree.queue_free(nid);
            tree.process_deletions();
            assert_eq!(tree.pending_deletion_count(), 0);

            // Calling again is safe and does nothing.
            tree.process_deletions();
            assert_eq!(tree.pending_deletion_count(), 0);
        }

        #[test]
        fn call_deferred_queues_until_flush() {
            let mut tree = SceneTree::new();
            let root = tree.root_id();
            let n = tree.add_child(root, Node::new("N", "Node")).unwrap();
            tree.get_node_mut(n)
                .unwrap()
                .set_property("visible", Variant::Bool(true));

            tree.call_deferred(
                n,
                "set",
                &[Variant::String("visible".into()), Variant::Bool(false)],
            );
            assert_eq!(tree.pending_deferred_count(), 1);
            assert_eq!(
                tree.get_node(n).unwrap().get_property("visible"),
                Variant::Bool(true)
            );

            tree.process_deferred_calls();
            assert_eq!(tree.pending_deferred_count(), 0);
            assert_eq!(
                tree.get_node(n).unwrap().get_property("visible"),
                Variant::Bool(false)
            );
        }

        #[test]
        fn call_deferred_fifo_order_for_set() {
            let mut tree = SceneTree::new();
            let root = tree.root_id();
            let n = tree.add_child(root, Node::new("N", "Node")).unwrap();
            tree.call_deferred(
                n,
                "set",
                &[Variant::String("name".into()), Variant::String("A".into())],
            );
            tree.call_deferred(
                n,
                "set",
                &[Variant::String("name".into()), Variant::String("B".into())],
            );
            tree.process_deferred_calls();
            assert_eq!(
                tree.get_node(n).unwrap().get_property("name"),
                Variant::String("B".into()),
                "last deferred set should win (FIFO)"
            );
        }

        #[test]
        fn call_deferred_skips_freed_nodes() {
            let mut tree = SceneTree::new();
            let root = tree.root_id();
            let n = tree.add_child(root, Node::new("N", "Node")).unwrap();
            tree.call_deferred(
                n,
                "set",
                &[Variant::String("visible".into()), Variant::Bool(false)],
            );
            tree.queue_free(n);
            tree.process_deletions();
            // Flush after free — must not panic.
            tree.process_deferred_calls();
            assert_eq!(tree.pending_deferred_count(), 0);
        }

        #[test]
        fn call_deferred_nested_runs_same_flush() {
            // A script method that call_deferred's another set during flush.
            struct NestScript;
            impl ScriptInstance for NestScript {
                fn call_method(
                    &mut self,
                    name: &str,
                    _args: &[Variant],
                ) -> Result<Variant, gdscript_interop::bindings::ScriptError> {
                    if name == "kick" {
                        Ok(Variant::Nil)
                    } else {
                        Err(gdscript_interop::bindings::ScriptError::MethodNotFound(
                            name.to_string(),
                        ))
                    }
                }
                fn get_property(&self, _: &str) -> Option<Variant> {
                    None
                }
                fn set_property(&mut self, _: &str, _: Variant) -> bool {
                    false
                }
                fn list_methods(&self) -> Vec<gdscript_interop::bindings::MethodInfo> {
                    vec![gdscript_interop::bindings::MethodInfo {
                        name: "kick".into(),
                        argument_names: Vec::new(),
                        return_type: gdvariant::variant::VariantType::Nil,
                        flags: gdscript_interop::bindings::MethodFlags::NORMAL,
                    }]
                }
                fn list_properties(&self) -> Vec<gdscript_interop::bindings::ScriptPropertyInfo> {
                    Vec::new()
                }
                fn get_script_name(&self) -> &str {
                    "NestScript"
                }
                fn has_method(&self, name: &str) -> bool {
                    name == "kick"
                }
            }

            let mut tree = SceneTree::new();
            let root = tree.root_id();
            let n = tree.add_child(root, Node::new("N", "Node")).unwrap();
            tree.attach_script(n, Box::new(NestScript));
            tree.call_deferred(n, "kick", &[]);
            // Nested deferred during process: queue a set after kick runs by
            // pre-queuing second call (simulates re-entrant queue growth).
            tree.call_deferred(
                n,
                "set",
                &[Variant::String("visible".into()), Variant::Bool(false)],
            );
            tree.process_deferred_calls();
            assert_eq!(tree.pending_deferred_count(), 0);
            assert_eq!(
                tree.get_node(n).unwrap().get_property("visible"),
                Variant::Bool(false)
            );
        }
    }

    // ── B006: EventTrace integration tests ──────────────────────────────

    mod trace_tests {
        use super::*;
        use crate::lifecycle::LifecycleManager;
        use crate::trace::TraceEventType;

        /// Helper: build a tree with root -> Parent -> (Child1, Child2) and
        /// enable tracing.
        fn build_traced_tree() -> (SceneTree, NodeId, NodeId, NodeId, NodeId) {
            let mut tree = SceneTree::new();
            tree.event_trace_mut().enable();
            let root = tree.root_id();
            let parent = Node::new("Parent", "Node");
            let parent_id = tree.add_child(root, parent).unwrap();
            let c1 = Node::new("Child1", "Node");
            let c1_id = tree.add_child(parent_id, c1).unwrap();
            let c2 = Node::new("Child2", "Node");
            let c2_id = tree.add_child(parent_id, c2).unwrap();
            // Clear trace events from add_child (if any triggered by auto-enter)
            tree.event_trace_mut().clear();
            (tree, root, parent_id, c1_id, c2_id)
        }

        #[test]
        fn trace_enter_tree_top_down_order() {
            let (mut tree, _root, parent_id, _c1_id, _c2_id) = build_traced_tree();
            LifecycleManager::enter_tree(&mut tree, parent_id);

            let events = tree.event_trace().events();
            let enter_events: Vec<_> = events
                .iter()
                .filter(|e| {
                    e.event_type == TraceEventType::Notification && e.detail == "ENTER_TREE"
                })
                .collect();

            // ENTER_TREE should fire parent first, then children (top-down).
            assert_eq!(enter_events.len(), 3);
            assert!(enter_events[0].node_path.ends_with("Parent"));
            assert!(enter_events[1].node_path.ends_with("Child1"));
            assert!(enter_events[2].node_path.ends_with("Child2"));
        }

        #[test]
        fn trace_ready_bottom_up_order() {
            let (mut tree, _root, parent_id, _c1_id, _c2_id) = build_traced_tree();
            LifecycleManager::enter_tree(&mut tree, parent_id);

            let events = tree.event_trace().events();
            let ready_events: Vec<_> = events
                .iter()
                .filter(|e| e.event_type == TraceEventType::Notification && e.detail == "READY")
                .collect();

            // READY should fire children first, then parent (bottom-up).
            assert_eq!(ready_events.len(), 3);
            assert!(ready_events[0].node_path.ends_with("Child1"));
            assert!(ready_events[1].node_path.ends_with("Child2"));
            assert!(ready_events[2].node_path.ends_with("Parent"));
        }

        #[test]
        fn trace_exit_tree_bottom_up_order() {
            let (mut tree, _root, parent_id, _c1_id, _c2_id) = build_traced_tree();
            LifecycleManager::enter_tree(&mut tree, parent_id);
            tree.event_trace_mut().clear();

            LifecycleManager::exit_tree(&mut tree, parent_id);

            let events = tree.event_trace().events();
            let exit_events: Vec<_> = events
                .iter()
                .filter(|e| e.event_type == TraceEventType::Notification && e.detail == "EXIT_TREE")
                .collect();

            // EXIT_TREE fires bottom-up: children first, then parent.
            assert_eq!(exit_events.len(), 3);
            assert!(exit_events[0].node_path.ends_with("Child1"));
            assert!(exit_events[1].node_path.ends_with("Child2"));
            assert!(exit_events[2].node_path.ends_with("Parent"));
        }

        #[test]
        fn trace_enter_then_ready_interleaving() {
            let (mut tree, _root, parent_id, _c1_id, _c2_id) = build_traced_tree();
            LifecycleManager::enter_tree(&mut tree, parent_id);

            let events = tree.event_trace().events();
            let notif_events: Vec<_> = events
                .iter()
                .filter(|e| e.event_type == TraceEventType::Notification)
                .collect();

            // Godot contract: ALL ENTER_TREE first (top-down), then ALL READY (bottom-up).
            // Find boundary: last ENTER_TREE should come before first READY.
            let last_enter = notif_events
                .iter()
                .rposition(|e| e.detail == "ENTER_TREE")
                .unwrap();
            let first_ready = notif_events
                .iter()
                .position(|e| e.detail == "READY")
                .unwrap();
            assert!(
                last_enter < first_ready,
                "All ENTER_TREE must fire before any READY"
            );
        }

        #[test]
        fn trace_process_notification_order() {
            let (mut tree, _root, parent_id, _c1_id, _c2_id) = build_traced_tree();
            LifecycleManager::enter_tree(&mut tree, parent_id);
            tree.event_trace_mut().clear();

            // Run a frame's worth of notification dispatch.
            tree.process_internal_physics_frame();
            tree.process_physics_frame();
            tree.process_internal_frame();
            tree.process_frame();

            let events = tree.event_trace().events();
            let details: Vec<&str> = events
                .iter()
                .filter(|e| e.event_type == TraceEventType::Notification)
                .map(|e| e.detail.as_str())
                .collect();

            // Per Godot contract: internal_physics -> physics -> internal_process -> process.
            // Each dispatches to all nodes in tree order, so we see the pattern repeated.
            // Find the first occurrence of each type.
            let first_int_phys = details
                .iter()
                .position(|d| *d == "INTERNAL_PHYSICS_PROCESS")
                .unwrap();
            let first_phys = details
                .iter()
                .position(|d| *d == "PHYSICS_PROCESS")
                .unwrap();
            let first_int_proc = details
                .iter()
                .position(|d| *d == "INTERNAL_PROCESS")
                .unwrap();
            let first_proc = details.iter().position(|d| *d == "PROCESS").unwrap();

            assert!(first_int_phys < first_phys);
            assert!(first_phys < first_int_proc);
            assert!(first_int_proc < first_proc);
        }

        #[test]
        fn trace_signal_emit_recorded() {
            let (mut tree, _root, parent_id, _c1_id, _c2_id) = build_traced_tree();

            tree.emit_signal(parent_id, "test_signal", &[]);

            let events = tree.event_trace().events();
            let signal_events: Vec<_> = events
                .iter()
                .filter(|e| e.event_type == TraceEventType::SignalEmit)
                .collect();

            assert_eq!(signal_events.len(), 1);
            assert_eq!(signal_events[0].detail, "test_signal");
            assert!(signal_events[0].node_path.ends_with("Parent"));
        }

        #[test]
        fn trace_disabled_records_nothing() {
            let mut tree = SceneTree::new();
            // Trace is disabled by default
            let root = tree.root_id();
            let child = Node::new("A", "Node");
            let child_id = tree.add_child(root, child).unwrap();

            tree.process_frame();
            tree.emit_signal(child_id, "foo", &[]);

            assert!(tree.event_trace().events().is_empty());
        }

        #[test]
        fn trace_frame_number_recorded() {
            let mut tree = SceneTree::new();
            let root = tree.root_id();
            let child = Node::new("A", "Node");
            let _child_id = tree.add_child(root, child).unwrap();

            // Enable tracing after add_child so PARENTED traces don't skew frame counts.
            tree.event_trace_mut().enable();
            tree.set_trace_frame(0);
            tree.process_frame();

            tree.set_trace_frame(1);
            tree.process_frame();

            let events = tree.event_trace().events();
            let frame0: Vec<_> = events.iter().filter(|e| e.frame == 0).collect();
            let frame1: Vec<_> = events.iter().filter(|e| e.frame == 1).collect();

            assert!(!frame0.is_empty());
            assert!(!frame1.is_empty());
            assert_eq!(frame0.len(), frame1.len());
        }

        #[test]
        fn trace_clear_resets() {
            let (mut tree, _root, parent_id, _c1_id, _c2_id) = build_traced_tree();
            LifecycleManager::enter_tree(&mut tree, parent_id);
            assert!(!tree.event_trace().events().is_empty());

            tree.event_trace_mut().clear();
            assert!(tree.event_trace().events().is_empty());
        }

        #[test]
        fn trace_full_lifecycle_global_order() {
            // Verify the complete lifecycle ordering matches Godot:
            // ENTER_TREE(Parent) -> ENTER_TREE(C1) -> ENTER_TREE(C2) ->
            // READY(C1) -> READY(C2) -> READY(Parent)
            let (mut tree, _root, parent_id, _c1_id, _c2_id) = build_traced_tree();
            LifecycleManager::enter_tree(&mut tree, parent_id);

            let events = tree.event_trace().events();
            let notif_events: Vec<_> = events
                .iter()
                .filter(|e| e.event_type == TraceEventType::Notification)
                .map(|e| format!("{}:{}", e.detail, e.node_path.rsplit('/').next().unwrap()))
                .collect();

            assert_eq!(
                notif_events,
                vec![
                    "ENTER_TREE:Parent",
                    "ENTER_TREE:Child1",
                    "ENTER_TREE:Child2",
                    "READY:Child1",
                    "READY:Child2",
                    "READY:Parent",
                ]
            );
        }

        #[test]
        fn trace_deep_tree_ordering() {
            // root -> A -> B -> C
            // ENTER_TREE should be A, B, C. READY should be C, B, A.
            let mut tree = SceneTree::new();
            tree.event_trace_mut().enable();
            let root = tree.root_id();
            let a = Node::new("A", "Node");
            let a_id = tree.add_child(root, a).unwrap();
            let b = Node::new("B", "Node");
            let b_id = tree.add_child(a_id, b).unwrap();
            let c = Node::new("C", "Node");
            let _c_id = tree.add_child(b_id, c).unwrap();
            tree.event_trace_mut().clear();

            LifecycleManager::enter_tree(&mut tree, a_id);

            let events = tree.event_trace().events();
            let names: Vec<_> = events
                .iter()
                .filter(|e| e.event_type == TraceEventType::Notification)
                .map(|e| format!("{}:{}", e.detail, e.node_path.rsplit('/').next().unwrap()))
                .collect();

            assert_eq!(
                names,
                vec![
                    "ENTER_TREE:A",
                    "ENTER_TREE:B",
                    "ENTER_TREE:C",
                    "READY:C",
                    "READY:B",
                    "READY:A",
                ]
            );
        }
    }

    #[test]
    fn create_timer_fires_after_delta() {
        let mut tree = SceneTree::new();
        let id = tree.create_timer(0.25);
        assert!(tree.get_timer(id).is_some());
        tree.process_timers(0.1);
        assert!(!tree.get_timer(id).unwrap().done);
        tree.process_timers(0.2);
        let signals = tree.take_timeout_signals();
        assert!(
            signals
                .iter()
                .any(|s| s == "timeout" || s.starts_with("timeout_")),
            "expected timeout signal, got {signals:?}"
        );
        assert!(tree.get_timer(id).is_none(), "timer drained after fire");
    }
}

#[cfg(test)]
mod multiplayer_tests {
    use super::*;
    use crate::node::Node;
    use gdplatform::network::{
        MockNetwork, MultiplayerAPI, PeerId, RPCConfig, RPCMode, TransferMode,
    };

    /// Script that records every method call, used to assert RPC dispatch.
    struct RecordingScript {
        calls: std::rc::Rc<std::cell::RefCell<Vec<(String, Vec<Variant>)>>>,
    }

    impl gdscript_interop::bindings::ScriptInstance for RecordingScript {
        fn call_method(
            &mut self,
            name: &str,
            args: &[Variant],
        ) -> Result<Variant, gdscript_interop::bindings::ScriptError> {
            self.calls
                .borrow_mut()
                .push((name.to_string(), args.to_vec()));
            Ok(Variant::Nil)
        }
        fn get_property(&self, _name: &str) -> Option<Variant> {
            None
        }
        fn set_property(&mut self, _name: &str, _value: Variant) -> bool {
            false
        }
        fn list_methods(&self) -> Vec<gdscript_interop::bindings::MethodInfo> {
            Vec::new()
        }
        fn list_properties(&self) -> Vec<gdscript_interop::bindings::ScriptPropertyInfo> {
            Vec::new()
        }
        fn get_script_name(&self) -> &str {
            "RecordingScript"
        }
        fn has_method(&self, _name: &str) -> bool {
            true
        }
    }

    fn any_reliable() -> RPCConfig {
        RPCConfig {
            mode: RPCMode::Any,
            transfer_mode: TransferMode::Reliable,
            channel: 0,
        }
    }

    #[test]
    fn scene_tree_exposes_multiplayer() {
        // get_multiplayer() is available and starts peer-less (not a server).
        let mut tree = SceneTree::new();
        assert!(!tree.get_multiplayer().is_server());
        // Polling with no peer is a harmless no-op.
        tree.poll_multiplayer();
    }

    #[test]
    fn two_peers_exchange_rpc_through_scene_tree() {
        // Paired loopback transport: server + client.
        let (server_peer, client_peer) = MockNetwork::create_pair(PeerId(2));

        // Server side: a MultiplayerAPI that initiates the rpc.
        let mut server = MultiplayerAPI::new();
        server.set_multiplayer_peer(Box::new(server_peer));
        server.register_rpc("take_damage", any_reliable());

        // Client side: a SceneTree with a "Player" node whose script records calls.
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let player = tree.add_child(root, Node::new("Player", "Node")).unwrap();
        let calls = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        tree.attach_script(
            player,
            Box::new(RecordingScript {
                calls: calls.clone(),
            }),
        );
        tree.get_multiplayer_mut()
            .set_multiplayer_peer(Box::new(client_peer));

        // Server invokes the rpc targeting the client's Player node path.
        let player_path = tree.node_path(player).unwrap();
        server
            .rpc(
                rpc_path_hash(&player_path),
                "take_damage",
                vec![Variant::Int(7)],
            )
            .unwrap();

        // Client polls: the inbound rpc is decoded and dispatched to the script.
        tree.poll_multiplayer();

        let recorded = calls.borrow();
        assert_eq!(recorded.len(), 1, "player should receive exactly one rpc");
        assert_eq!(recorded[0].0, "take_damage");
        assert_eq!(recorded[0].1, vec![Variant::Int(7)]);
    }

    #[test]
    fn rpc_to_unknown_path_is_dropped() {
        let (server_peer, client_peer) = MockNetwork::create_pair(PeerId(2));
        let mut server = MultiplayerAPI::new();
        server.set_multiplayer_peer(Box::new(server_peer));
        server.register_rpc("noop", any_reliable());

        let mut tree = SceneTree::new();
        tree.get_multiplayer_mut()
            .set_multiplayer_peer(Box::new(client_peer));

        // No node hashes to this path, so the rpc is silently dropped.
        server
            .rpc(rpc_path_hash("root/DoesNotExist"), "noop", vec![])
            .unwrap();
        tree.poll_multiplayer(); // must not panic
    }
}
