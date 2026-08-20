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
use crate::lifecycle::LifecycleManager;
use crate::node::{Node, NodeId};
use crate::sprite_frames::{AnimatedSpritePlayback, SpriteFrames};
use crate::trace::{EventTrace, TraceEvent, TraceEventType};
use crate::tween::{Tween, TweenId, TweenProcessMode};

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

/// Reads a `.tscn` path-ish property as a plain string (pat-s3rrv.17).
///
/// `spawn_path` / `root_path` are `NodePath` in Godot and a `.tscn` writes them
/// as `NodePath("../Foo")`, but a hand-built scene or a test may set the same
/// property as a bare `String` — both spellings mean the same path, so both are
/// accepted. Anything else (notably `Nil`, which is what an unset property reads
/// back as) yields `None` so the caller can fall back to a default rather than
/// registering an empty path that silently matches nothing.
/// The properties a `MultiplayerSynchronizer` replicates (pat-s3rrv.17).
///
/// Prefers the flattened `SceneReplicationConfig` (`__replication__properties/N/*`,
/// what the Godot editor actually exports) and falls back to the hand-authored
/// `_sync_properties` array that hand-written test scenes use.
///
/// An empty result means nothing syncs. `poll_outbound` already returns early on
/// an empty snapshot, so an unconfigured synchronizer stays inert rather than
/// guessing at `position`.
fn replication_properties_from_node(node: &crate::node::Node) -> Vec<String> {
    replication_paths_from_node(node, "sync")
}

/// The properties a `MultiplayerSynchronizer` sends as SPAWN state
/// (pat-s3rrv.17).
///
/// ⚠️ Godot's `SceneReplicationConfig` defaults BOTH flags to true
/// (`scene_replication_config.h:51`), so an entry that does not spell `spawn`
/// out is spawn state — the same default the sync reader uses. Defaulting to
/// false here would make every editor-authored config send nothing at spawn
/// while looking correctly configured.
///
/// No `_sync_properties` fallback: that array is the hand-authored SYNC list,
/// and treating it as spawn state too would send a snapshot no config asked for.
fn spawn_properties_from_node(node: &crate::node::Node) -> Vec<String> {
    replication_paths_from_node(node, "spawn")
}

/// The properties a `MultiplayerSynchronizer` replicates ONLY on change
/// (`SceneReplicationConfig` REPLICATION_MODE_ON_CHANGE — Godot's `watch_props`).
///
/// ⚠️ Disjoint from [`replication_properties_from_node`] by construction: sync is
/// mode ALWAYS, watch is mode ON_CHANGE, and a property has exactly one mode.
fn watch_properties_from_node(node: &crate::node::Node) -> Vec<String> {
    replication_paths_from_node(node, "watch")
}

fn replication_paths_from_node(node: &crate::node::Node, flag_name: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    // Godot numbers the entries densely from 0, so stop at the first gap rather
    // than scanning an arbitrary range.
    for index in 0.. {
        let path = match node.get_property(&format!("__replication__properties/{index}/path")) {
            Variant::Nil => break,
            other => match variant_as_path_string(&other) {
                Some(p) if !p.is_empty() => p,
                _ => break,
            },
        };
        let flag = |name: &str, default: bool| -> bool {
            match node.get_property(&format!("__replication__properties/{index}/{name}")) {
                Variant::Bool(b) => b,
                _ => default,
            }
        };
        // Godot's `ReplicationMode`, transcribed positionally from
        // `modules/multiplayer/scene_replication_config.h`:
        // NEVER 0, ALWAYS 1, ON_CHANGE 2. Absent means the default, ALWAYS.
        let mode = match node.get_property(&format!(
            "__replication__properties/{index}/replication_mode"
        )) {
            Variant::Int(i) => i,
            _ => 1,
        };
        // 🔑 `sync` and `watch` are two VIEWS over the single `mode` field, not
        // independent flags — `_update()` in scene_replication_config.cpp routes
        // ALWAYS into `sync_props` and ON_CHANGE into `watch_props`, so a path is
        // never in both.
        //
        // ⚠️ ON_CHANGE used to fall through this check into the SYNC list, which
        // meant a watched property was retransmitted every tick — precisely the
        // behaviour the mode exists to avoid.
        let include = match flag_name {
            "sync" => mode == 1 && flag("sync", true),
            // Upstream has no `watch` bool; membership is the mode alone.
            "watch" => mode == 2,
            // 🔑 `spawn` is gated on the flag ALONE. Upstream's `_update()`
            // pushes into `spawn_props` on `prop.spawn` with no reference to
            // `mode` at all — only the sync/watch routing below it consults the
            // mode. So a `spawn = true, replication_mode = 0` property IS spawn
            // state: it travels once with the spawn packet and never syncs
            // afterwards, which is a perfectly ordinary way to author immutable
            // per-instance data.
            //
            // ⚠️ This used to also require `mode != 0`, so exactly that
            // combination was dropped from the spawn state and the value never
            // reached the client at all.
            "spawn" => flag("spawn", true),
            // No other flag name is used today; kept conservative rather than
            // unreachable so a future caller cannot silently get spawn's rule.
            _ => mode != 0 && flag(flag_name, true),
        };
        if !include {
            continue;
        }
        // ⚠️ A config path is `<node>:<property>`, e.g. `.:position`. Split from
        // the RIGHT: only the last colon separates the property, and a NodePath
        // may legitimately contain others.
        let Some((node_part, prop)) = path.rsplit_once(':') else {
            continue;
        };
        if prop.is_empty() {
            continue;
        }
        // `.` and the empty node part both mean the synchronizer's own root, and
        // those are stored as a BARE property name — that keeps the common case
        // a plain `get_property` and keeps the same shape as the
        // `_sync_properties` fallback below.
        //
        // A path targeting a CHILD (`Sprite2D:modulate`) is kept VERBATIM,
        // colon and all. Both the outbound snapshot and the inbound apply split
        // it again and resolve the subpath relative to the synchronizer's root,
        // so the key means the same thing on either peer and no wire-format
        // change is needed — the packet already carries arbitrary
        // `(String, Variant)` pairs.
        //
        // ⚠️ This used to `continue` here, silently dropping every child path.
        // Godot's editor authors configs with them constantly, so a real
        // exported .tscn replicated nothing while looking correctly configured.
        if node_part.is_empty() || node_part == "." {
            out.push(prop.to_string());
        } else {
            out.push(format!("{node_part}:{prop}"));
        }
    }
    if !out.is_empty() || flag_name != "sync" {
        return out;
    }
    match node.get_property("_sync_properties") {
        Variant::Array(items) => items
            .iter()
            .filter_map(|v| match v {
                Variant::String(s) if !s.is_empty() => Some(s.clone()),
                Variant::StringName(s) if !s.as_str().is_empty() => Some(s.as_str().to_string()),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

fn variant_as_path_string(value: &Variant) -> Option<String> {
    match value {
        Variant::NodePath(p) => Some(p.to_string()),
        Variant::String(s) if !s.is_empty() => Some(s.clone()),
        Variant::StringName(s) if !s.as_str().is_empty() => Some(s.as_str().to_string()),
        _ => None,
    }
}

/// Scene-replication state owned by a [`SceneTree`] (pat-s3rrv.17).
///
/// Godot keeps this inside `SceneReplicationInterface`, registered per node via
/// `MultiplayerAPI::object_configuration_add`. Grouped into one struct here so
/// adding replication touches a single `SceneTree` field rather than four.
///
/// `registered` is what makes registration idempotent: `register_replication_nodes`
/// re-scans the tree whenever nodes may have been added, and a
/// `MultiplayerSynchronizer` must not lose its accumulated property values (or
/// its `last_sync_ms` interval clock) just because the scan ran again.
#[derive(Debug, Default)]
struct ReplicationState {
    /// Applies inbound spawn/sync packets to the live tree.
    replicator: crate::multiplayer::MultiplayerReplicator,
    /// `MultiplayerSpawner` node -> its replication config.
    spawners: HashMap<NodeId, gdplatform::network::MultiplayerSpawner>,
    /// `MultiplayerSynchronizer` node -> its replication config.
    synchronizers: HashMap<NodeId, gdplatform::network::MultiplayerSynchronizer>,
    /// Nodes already registered, so a re-scan does not reset their state.
    registered: HashSet<NodeId>,
    /// Children already announced under each spawner's `spawn_path`.
    ///
    /// The outbound spawn pass diffs the live children against this set, so a
    /// node is announced exactly once no matter how many frames it lives for.
    announced: HashMap<NodeId, HashSet<NodeId>>,
    /// Announced child -> the `net_id` it was announced under (pat-s3rrv.17).
    ///
    /// The spawn pass mints a `net_id` per child and previously discarded it.
    /// Despawn needs it: the receiver keys its instantiated nodes by `net_id`,
    /// so "this child is gone" cannot be expressed without the id it arrived
    /// under.
    spawn_net_ids: HashMap<NodeId, u32>,
    /// `MultiplayerSynchronizer` node -> its `add_visibility_filter` callables
    /// (pat-s3rrv.17).
    ///
    /// ⚠️ These live HERE rather than beside the rest of the visibility state on
    /// gdplatform's synchronizer, and not by preference. A filter is a
    /// `Callable` that must be INVOKED, and gdplatform depends only on gdcore +
    /// gdvariant — it has no route to a script interpreter, so it physically
    /// cannot run one. Only gdscene can.
    ///
    /// The evaluated verdict is written back into the synchronizer's per-peer
    /// visibility map, so `poll_outbound`'s existing `is_visible_to` check stays
    /// the single place visibility is actually enforced.
    visibility_filters: HashMap<NodeId, Vec<Variant>>,
    /// Last computed filter verdict per `(synchronizer, peer)` (pat-s3rrv.17).
    ///
    /// ⚠️ Filters are evaluated ONLY in the outbound pass, never on demand from
    /// `is_visible_to`. `call_script_method` takes the script OUT of the arena
    /// for the duration of a call, so a script asking `is_visible_to()` from
    /// `_process` is already checked out — a re-entrant call to a filter on that
    /// same node finds no script and returns `None`. Treating that as a filter
    /// failure would hide every peer whenever a script asked the question.
    ///
    /// So the outbound pass (which runs outside any script call, where the
    /// filter genuinely can run) records its verdict here, and queries read the
    /// cache. A query therefore reflects the previous frame's evaluation, which
    /// is the honest answer: it is the verdict the send path is acting on.
    filter_verdicts: HashMap<(NodeId, u32), bool>,
    /// Monotonic milliseconds accumulated from the frame delta.
    ///
    /// `MultiplayerSynchronizer::poll_outbound` compares this against its
    /// `last_sync_ms` to honour `replication_interval`. A frame counter would
    /// not do: the interval is declared in seconds, so it has to be measured in
    /// wall time the way Godot's `SceneReplicationInterface` does.
    clock_ms: u64,
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
    /// Godot `create_timer(..., process_always)`, default `true`.
    ///
    /// When false the timer stops while the tree is paused — the flag exists so
    /// a pause menu's own timers can keep running while gameplay ones freeze
    /// (`SceneTree::process_timers` skips them when `paused`).
    pub process_always: bool,
    /// Godot `create_timer(..., process_in_physics)`, default `false`.
    ///
    /// Advances on the fixed physics tick instead of the idle frame, so its
    /// resolution follows `physics_ticks_per_second` rather than the render rate.
    pub process_in_physics: bool,
    /// Godot `create_timer(..., ignore_time_scale)`, default `false`.
    ///
    /// Uses the driver's real delta, so `Engine.time_scale = 0.5` does not
    /// stretch it — what a UI timeout during bullet-time needs.
    pub ignore_time_scale: bool,
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
/// The open popup menu and where it is on screen (pat-s3rrv.72).
///
/// Holds the MODEL rather than pointing at the node, so activation reads the
/// same items the hit-test measured even if the node's `items` property is
/// rewritten while the menu is open.
#[derive(Debug, Clone)]
struct OpenPopup {
    /// The control that opened it — a MenuButton or OptionButton. Receives
    /// `item_selected` when the owner is an OptionButton.
    owner: NodeId,
    /// The PopupMenu node itself, when one exists as a child. `id_pressed` and
    /// `index_pressed` are emitted here, as Godot emits them on the PopupMenu.
    popup_node: Option<NodeId>,
    menu: crate::popup::PopupMenu,
    /// Top-left corner in the same space `gui_input_stage` receives positions.
    origin: gdcore::math::Vector2,
    width: f32,
    /// The row under the cursor, for the draw pass to highlight.
    hovered: Option<usize>,
    /// Which MenuBar title this menu belongs to, when a bar opened it.
    ///
    /// `None` for a MenuButton/OptionButton, which have no strip. Kept so the
    /// strip can highlight the open title and so hover-to-switch knows which
    /// title it is already showing.
    bar_title: Option<usize>,
}

pub struct SceneTree {
    /// Arena storage: every node in the tree lives here.
    nodes: HashMap<NodeId, Node>,
    /// The ID of the root node (always valid).
    root_id: NodeId,
    /// Seconds elapsed in the running loop, published for shader `TIME`
    /// (pat-s3rrv.122).
    ///
    /// 🔑 Lives on the tree because the render adapter's per-frame entry
    /// (`render_frame`) takes only `&SceneTree`. Threading a parameter instead
    /// would change a signature `gdeditor` also calls — and gdeditor does not
    /// currently build, so that edit could not be verified.
    shader_time: f32,
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
    /// Per-node graph models (Godot `GraphEdit`), holding the node set and
    /// connection list a script manipulates via `connect_node` /
    /// `disconnect_node` (pat-0q2kx.244).
    ///
    /// `graph_edit.rs` was a complete 732-line model with NO owner anywhere, so
    /// nothing could reach it. It lives on the tree rather than the frame loop
    /// because the connection list is authored state that must survive whether
    /// or not the scene is ticking — and because the scripting accessor reaches
    /// the tree, not `MainLoop`.
    graph_edits: HashMap<NodeId, crate::graph_edit::GraphEdit>,
    /// Per-node video players (Godot `VideoStreamPlayer`), advanced each frame
    /// by [`process_video_players`](Self::process_video_players).
    ///
    /// The playback model in [`crate::video_stream`] was complete but had no
    /// caller outside its own tests, so a `VideoStreamPlayer` in a played scene
    /// never advanced and never finished (pat-0q2kx.182).
    video_players: HashMap<NodeId, crate::video_stream::VideoStreamPlayer>,
    /// Control currently under the mouse, for `mouse_entered`/`mouse_exited`
    /// transitions in the GUI input stage (pat-0q2kx.18).
    /// The popup menu currently open, if any (pat-s3rrv.72).
    ///
    /// Godot's `PopupMenu` is a Popup<-Window: while it is open it draws above
    /// everything and GRABS input, so the control underneath must not see the
    /// click that dismisses it.
    open_popup: Option<OpenPopup>,
    hovered_control: Option<NodeId>,
    /// Control that received the most recent mouse press. A release only counts
    /// as a click when it lands on this same control, as in Godot.
    gui_press_target: Option<NodeId>,
    /// Godot's `Viewport::gui.mouse_focus` (viewport.cpp:1916): the control that
    /// captured the pointer on press. While it is set, motion and the release
    /// are routed HERE rather than re-hit-tested (viewport.cpp:2070-2075) —
    /// which is what makes button drag-off/drag-back, and every draggable
    /// Control, work at all (pat-s3rrv.12).
    gui_mouse_focus: Option<NodeId>,
    /// Per-node `HTTPRequest` state (pat-s3rrv.78), created lazily the first
    /// time a script calls `request()` on an HTTPRequest node.
    http_requests: HashMap<NodeId, crate::http_request_node::HttpRequestNode>,
    /// An in-flight scrollbar grabber drag (Godot's `ScrollBar::drag`), holding
    /// where the grab started so the handle stays under the pointer.
    gui_scrollbar_drag: Option<(NodeId, crate::range_controls::ScrollBarDrag)>,
    /// An in-flight SplitContainer divider drag: `(container, grab position
    /// along the split axis, the container's `split_offset` when grabbed)`
    /// (pat-s3rrv.173).
    ///
    /// ⚠️ The offset at grab time is stored rather than applying per-motion
    /// deltas. `drag_split` CLAMPS, so accumulating deltas would desynchronise
    /// the divider from the pointer the moment a drag ran into the clamp: the
    /// pointer keeps moving, the offset does not, and the divider then lags by
    /// however far the user overshot on the way back.
    gui_split_drag: Option<(NodeId, f32, f32)>,
    /// The `[url=…]` run the pointer is currently inside: its label, the run's
    /// index and the payload.
    ///
    /// The payload is kept because `meta_hover_ended` reports the run being
    /// LEFT, which is no longer under the cursor by the time it is emitted.
    hovered_meta: Option<(NodeId, usize, String)>,
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
    /// Address of the [`crate::physics_server::PhysicsServer`] driving this
    /// frame, or 0 when no server is bound — the route that lets a script reach
    /// the `PhysicsServer2D` singleton (pat-0q2kx.200).
    ///
    /// Stored as a `usize` rather than a `*mut` deliberately: `SceneTree` lives
    /// inside an `Arc<Mutex<EditorState>>` that the editor server shares across
    /// HTTP threads, so it must stay `Send`, and a raw-pointer field would
    /// silently make it `!Send` and break every `gdeditor` handler. The
    /// pointer is only ever installed for the duration of a script phase by
    /// [`SceneTree::set_script_physics_server`].
    script_physics: usize,
    /// The 3D twin of [`script_physics`](Self::script_physics) (pat-s3rrv.107).
    ///
    /// Separate address rather than an enum: the two servers are different
    /// types and a script can legitimately query both in one frame.
    script_physics_3d: usize,
    /// Address of the `AudioServer` scripts reach through the `AudioServer`
    /// singleton, installed for the duration of a script phase
    /// (pat-s3rrv.6). A `usize` for the same `Send` reason as
    /// [`Self::script_physics`]; see [`SceneTree::set_script_audio_server`].
    script_audio: usize,
    /// `SceneMultiplayer.set_auth_callback(cb)` — the script Callable consulted
    /// when a peer authenticates (pat-s3rrv.18). Held on the tree rather than
    /// in `MultiplayerAPI` because only the scene layer can invoke a script
    /// Callable; `gdplatform` has no interpreter.
    multiplayer_auth_callback: Variant,
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
    /// Mirrored default cursor shape (Godot `CURSOR_*`; 0 = `CURSOR_ARROW`),
    /// the cursor counterpart of `input_mouse_mode` (pat-0q2kx.90).
    cursor_shape: i64,
    /// Last-known connected joypad ids, mirrored for `Input.get_connected_joypads`.
    input_connected_joypads: Vec<i64>,
    /// Mirror of the engine `InputMap`: action name → number of bound events
    /// (pat-0q2kx.89). Enough to answer `InputMap.has_action` / `get_actions` /
    /// `action_has_event` from the script phase, which cannot reach the real
    /// map; the frame owner refreshes it, and a script's own mutation updates
    /// it eagerly so add-then-query in one frame is consistent.
    input_map_events: std::collections::HashMap<String, usize>,
    /// Action name → its real bindings (pat-s3rrv.14).
    ///
    /// Kept alongside the count mirror rather than replacing it: the counts are
    /// authoritative for actions a script added at runtime through
    /// `InputMap.add_action`, which have no engine bindings yet, and losing that
    /// distinction would make `has_action` disagree with `get_actions`.
    input_map_bindings: std::collections::HashMap<String, Vec<gdplatform::input::ActionBinding>>,
    /// The packed scene that was used to load the current scene (if any).
    /// Stored for `reload_current_scene()`.
    current_packed_scene: Option<crate::packed_scene::PackedScene>,
    /// The NodeId of the current scene root (first child of root after
    /// `change_scene_to_packed` or `change_scene_to_node`).
    current_scene_id: Option<NodeId>,
    /// Nodes marked for deferred deletion via `queue_free()`.
    /// They are removed at the end of the frame by `process_deletions()`.
    /// URIs a `LinkButton` press asked the host to open (pat-s3rrv.166).
    ///
    /// 🔑 Recorded rather than performed. `gdplatform::os::shell_open` really
    /// spawns `open` / `xdg-open` / `cmd start`, and gdscene performs no
    /// platform side effects anywhere else — a layout/input pass that launched
    /// a browser would fire on every test run that clicks one. The host drains
    /// these and decides; the same shape as `pending_input_commands` below.
    pending_shell_open: Vec<String>,
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
    /// Frame on which each mouse button was last pressed, keyed by Godot button
    /// index — the state `InputEventMouseButton.double_click` is computed from
    /// (pat-s3rrv.76).
    ///
    /// Godot's platform layers flag the SECOND press, so the window is measured
    /// against the previous press of the SAME button: chording left then right
    /// is not a double click.
    last_mouse_press_frame: std::collections::HashMap<i64, u64>,
    /// Stylus data from the most recent pointer event, applied to the next
    /// `InputEventMouseMotion` (pat-s3rrv.76). See [`PenState`].
    pen: PenState,
    /// The `double_click` flag [`push_input`](Self::push_input) computed for the
    /// event it is dispatching right now (pat-s3rrv.76).
    ///
    /// Physics picking needs this value but runs outside `push_input` — it needs
    /// the physics world, which the tree does not own — so the flag is mirrored
    /// here rather than recomputed. Recomputing would be wrong, not merely
    /// duplicated: the window bookkeeping is stateful (`last_mouse_press_frame`
    /// is written as it is read), so a second pass over the same event would see
    /// the press it just recorded and report every click as a double.
    last_double_click: bool,
    /// The modifier keys held as of the most recent key event (pat-s3rrv.14).
    ///
    /// ⚠️ Mouse and gesture events are `InputEventWithModifiers` in Godot
    /// (`doc/classes/InputEventMouse.xml:2`), so `event.shift_pressed` on a
    /// click is real state there. Patina's `InputEvent::MouseButton` carries no
    /// modifier fields, and adding them is the same 71-construction-site change
    /// `double_click` above declined — so the state is mirrored here instead,
    /// from the flags every key event already carries.
    ///
    /// Without this every mouse and gesture event reported all four modifiers as
    /// `false`, so **no script could implement shift-click or ctrl-click** —
    /// multi-select, add-to-selection, and every other modifier-qualified click.
    last_modifiers: (bool, bool, bool),
    /// Whether the scene tree is currently paused.
    paused: bool,
    /// Multiplayer coordinator wired into the tree (pat-u3l8c.75). Polled once
    /// per frame via [`poll_multiplayer`](Self::poll_multiplayer); inbound RPCs
    /// are dispatched to their target node's script.
    multiplayer: MultiplayerAPI,
    /// Scene-replication state for the `MultiplayerSpawner` /
    /// `MultiplayerSynchronizer` nodes in this tree (pat-s3rrv.17).
    ///
    /// Godot registers each of these nodes with the MultiplayerAPI as it enters
    /// the tree (`object_configuration_add`, multiplayer_spawner.cpp
    /// `_notification(NOTIFICATION_POST_ENTER_TREE)`); this is the equivalent
    /// registry, keyed by the node that owns each config so a poll can find the
    /// live node again. Populated by
    /// [`register_replication_nodes`](Self::register_replication_nodes).
    replication: ReplicationState,
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
    /// CanvasItems whose `_draw` must re-run before the next frame is built —
    /// Godot's `CanvasItem::queue_redraw` pending set (pat-0q2kx.82).
    redraw_queued: HashSet<NodeId>,
    /// The last command list each scripted `_draw` produced. Godot keeps a
    /// CanvasItem's commands alive until the item is redrawn, which is why this
    /// survives frames instead of being rebuilt by the render walk.
    custom_draw_commands: HashMap<NodeId, Vec<gdserver2d::canvas::DrawCommand>>,
    /// What each control's SCRIPTED resource drew on the last pre-pass
    /// (pat-s3rrv.160). Kept apart from `custom_draw_commands` because a
    /// control can have both its own `_draw` and a scripted stylebox, and
    /// merging them would make the two indistinguishable — and double-draw the
    /// node the moment one of them re-ran.
    scripted_resource_commands: HashMap<NodeId, Vec<gdserver2d::canvas::DrawCommand>>,
    /// Set only while a `_draw` body is running: the `draw_*` builtins append
    /// here, standing in for Godot's "drawing is only valid inside `_draw`"
    /// rule (a `draw_rect` outside `_draw` is silently dropped, as it is there).
    draw_capture: Option<Vec<gdserver2d::canvas::DrawCommand>>,
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
            shader_time: 0.0,
            nodes,
            root_id,
            groups: HashMap::new(),
            signal_stores: HashMap::new(),
            animation_players: HashMap::new(),
            animation_trees: HashMap::new(),
            graph_edits: HashMap::new(),
            video_players: HashMap::new(),
            open_popup: None,
            hovered_control: None,
            gui_press_target: None,
            gui_mouse_focus: None,
            http_requests: HashMap::new(),
            gui_scrollbar_drag: None,
            gui_split_drag: None,
            hovered_meta: None,
            focus_owner: None,
            tweens: HashMap::new(),
            scripts: HashMap::new(),
            input_snapshot: None,
            script_physics: 0,
            script_physics_3d: 0,
            script_audio: 0,
            multiplayer_auth_callback: Variant::Nil,
            pending_input_commands: Vec::new(),
            input_mouse_mode: 0,
            cursor_shape: 0,
            input_connected_joypads: Vec::new(),
            input_map_events: std::collections::HashMap::new(),
            input_map_bindings: std::collections::HashMap::new(),
            current_packed_scene: None,
            current_scene_id: None,
            pending_shell_open: Vec::new(),
            pending_deletions: Vec::new(),
            pending_deferred_calls: Vec::new(),
            scene_timers: Vec::new(),
            next_timer_id: 1,
            event_trace: EventTrace::new(),
            trace_frame: 0,
            last_mouse_press_frame: std::collections::HashMap::new(),
            pen: PenState::default(),
            last_double_click: false,
            last_modifiers: (false, false, false),
            paused: false,
            multiplayer: MultiplayerAPI::new(),
            replication: ReplicationState::default(),
            rpc_call_local: std::collections::HashSet::new(),
            pending_scene: None,
            should_quit: false,
            exit_code: 0,
            input_handled: false,
            root_window: crate::viewport::RootWindow::default(),
            redraw_queued: HashSet::new(),
            custom_draw_commands: HashMap::new(),
            scripted_resource_commands: HashMap::new(),
            draw_capture: None,
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
    ///
    /// Call [`publish_root_window_state`](Self::publish_root_window_state)
    /// afterwards so a script's `get_window().content_scale_mode` sees the
    /// change — this accessor writes the Rust struct only.
    pub fn root_window_mut(&mut self) -> &mut crate::viewport::RootWindow {
        &mut self.root_window
    }

    /// Records the stylus data the next `InputEventMouseMotion` should carry
    /// (pat-s3rrv.76).
    ///
    /// The host calls this from its pointer-event route immediately before
    /// pushing the motion event, so `_input` sees a Godot-shaped
    /// `pressure`/`tilt`/`pen_inverted` instead of the hardcoded zeros that
    /// made every stylus look like a mouse.
    ///
    /// ⚠️ STICKY, by design. A browser sends `pressure` on every pointer event,
    /// so the value persists until the next one replaces it — clearing it per
    /// event would report 0 for any motion that arrived without fresh data,
    /// which reads as "pen lifted" rather than "unchanged".
    pub fn set_pen_state(&mut self, pen: PenState) {
        self.pen = pen;
    }

    /// The stylus data currently applied to motion events.
    pub fn pen_state(&self) -> PenState {
        self.pen
    }

    /// The `double_click` flag computed for the event most recently passed to
    /// [`push_input`](Self::push_input).
    ///
    /// ⚠️ Only meaningful while that event is still being dispatched. It is read
    /// by the physics-picking stage, which `MainLoop::step` runs immediately
    /// after `push_input` within the same loop iteration, so the value belongs
    /// to the event picking is handling. Reading it anywhere else reports the
    /// previous event's flag.
    pub fn last_double_click(&self) -> bool {
        self.last_double_click
    }

    /// The `(shift, ctrl, alt)` state carried by the most recent key event.
    ///
    /// Read by the picking stages, which build their own payload for the same
    /// event `push_input` is dispatching — the same mirroring reason as
    /// [`last_double_click`](Self::last_double_click).
    pub fn last_modifiers(&self) -> (bool, bool, bool) {
        self.last_modifiers
    }

    /// Mirrors the root window's content-scale state onto the root NODE's
    /// property bag (pat-0q2kx.141).
    ///
    /// `Window`'s `content_scale_*` properties were declared in ClassDB but
    /// never populated from the booted project, so `get_window().content_scale_mode`
    /// answered the registration's default `0` inside a project that plainly
    /// sets `canvas_items`. The scaling itself worked — only the script-visible
    /// half was missing, which is the sort of gap that reads as "implemented"
    /// right up until a script asks.
    ///
    /// Enum values are Godot's `Window.ContentScaleMode` /
    /// `ContentScaleAspect` / `ContentScaleStretch` ordinals.
    pub fn publish_root_window_state(&mut self) {
        use gdcore::content_scale::{ContentScaleAspect, ContentScaleMode, ContentScaleStretch};
        let cfg = self.root_window.config;
        let size = self.root_window.size;
        let mode = match cfg.mode {
            ContentScaleMode::Disabled => 0,
            ContentScaleMode::CanvasItems => 1,
            ContentScaleMode::Viewport => 2,
        };
        let aspect = match cfg.aspect {
            ContentScaleAspect::Ignore => 0,
            ContentScaleAspect::Keep => 1,
            ContentScaleAspect::KeepWidth => 2,
            ContentScaleAspect::KeepHeight => 3,
            ContentScaleAspect::Expand => 4,
        };
        let stretch = match cfg.stretch {
            ContentScaleStretch::Fractional => 0,
            ContentScaleStretch::Integer => 1,
        };
        let root = self.root_id();
        if let Some(node) = self.get_node_mut(root) {
            node.set_property("content_scale_mode", Variant::Int(mode));
            node.set_property("content_scale_aspect", Variant::Int(aspect));
            node.set_property("content_scale_stretch", Variant::Int(stretch));
            node.set_property("content_scale_factor", Variant::Float(cfg.factor as f64));
            node.set_property("content_scale_size", Variant::Vector2(cfg.base_size));
            node.set_property("size", Variant::Vector2(size));
        }
    }

    /// Resizes the root window and, when the size actually changed, republishes
    /// the state and emits `size_changed` on the root (pat-0q2kx.141).
    ///
    /// Godot's `Viewport` emits `size_changed` whenever the viewport's size
    /// changes, which is how a game re-lays-out on a window resize. Nothing in
    /// Patina emitted it — the signal name appeared NOWHERE in the workspace —
    /// so a script connecting to it waited forever.
    ///
    /// Returns whether the size changed. A resize to the same size is silent,
    /// as in Godot.
    /// Publishes the loop's elapsed seconds for shader `TIME` (pat-s3rrv.122).
    ///
    /// `MainLoop::step` sets this each frame; the render adapter reads it and
    /// hands it to the fragment shader. Before this, `CustomFragmentShader`'s
    /// `set_time` had NO production caller anywhere — `TIME` evaluated to 0.0
    /// in every real render on every backend, so an animated shader was static.
    pub fn set_shader_time(&mut self, seconds: f32) {
        self.shader_time = seconds;
    }

    /// Elapsed seconds published for shader `TIME`.
    pub fn shader_time(&self) -> f32 {
        self.shader_time
    }

    pub fn resize_root_window(&mut self, size: gdcore::math::Vector2) -> bool {
        if self.root_window.size == size {
            return false;
        }
        self.root_window.size = size;
        self.publish_root_window_state();
        let root = self.root_id();
        self.emit_signal(root, "size_changed", &[]);
        true
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

    /// Registers every `MultiplayerSpawner` / `MultiplayerSynchronizer` node in
    /// the tree with this tree's replication state (pat-s3rrv.17).
    ///
    /// Godot does this per node as it enters the tree
    /// (`MultiplayerAPI::object_configuration_add`, called from
    /// `_notification(NOTIFICATION_POST_ENTER_TREE)` in multiplayer_spawner.cpp
    /// / multiplayer_synchronizer.cpp). Patina instantiates these classes from a
    /// `.tscn` as plain `Node`s with a property bag, so the equivalent is a scan
    /// that reads those properties into the gdplatform config objects.
    ///
    /// Idempotent: a node already registered is skipped, so re-scanning never
    /// discards a synchronizer's accumulated property values or resets its
    /// `last_sync_ms` interval clock. Safe to call every frame.
    pub fn register_replication_nodes(&mut self) {
        use gdplatform::network::{MultiplayerSpawner, MultiplayerSynchronizer};

        // Read-then-write in two phases: the scan borrows the node arena
        // immutably, and inserting into `self.replication` needs it mutably.
        enum Pending {
            Spawner {
                spawn_path: String,
                spawn_limit: u32,
                scenes: Vec<String>,
            },
            Synchronizer {
                root_path: String,
                interval_ms: u32,
                properties: Vec<String>,
                /// REPLICATION_MODE_ON_CHANGE paths — disjoint from `properties`.
                watch_properties: Vec<String>,
                public_visibility: bool,
            },
        }

        let mut pending: Vec<(NodeId, Pending)> = Vec::new();
        for id in self.all_nodes_in_tree_order() {
            if self.replication.registered.contains(&id) {
                continue;
            }
            let Some(node) = self.get_node(id) else {
                continue;
            };
            match node.class_name() {
                "MultiplayerSpawner" => {
                    // `_spawnable_scenes` is how the `.tscn` carries the
                    // `spawnable_scenes` array; a single `spawn_path` scene is
                    // also accepted so a minimal scene needs no array.
                    let scenes = match node.get_property("_spawnable_scenes") {
                        Variant::Array(items) => items
                            .iter()
                            .filter_map(variant_as_path_string)
                            .collect::<Vec<_>>(),
                        other => variant_as_path_string(&other).into_iter().collect(),
                    };
                    pending.push((
                        id,
                        Pending::Spawner {
                            spawn_path: variant_as_path_string(&node.get_property("spawn_path"))
                                .unwrap_or_default(),
                            spawn_limit: match node.get_property("spawn_limit") {
                                Variant::Int(i) if i > 0 => i as u32,
                                _ => 0,
                            },
                            scenes,
                        },
                    ));
                }
                "MultiplayerSynchronizer" => {
                    // Godot stores `replication_interval` in **seconds**;
                    // gdplatform's synchronizer counts milliseconds.
                    let interval_ms = match node.get_property("replication_interval") {
                        Variant::Float(f) if f > 0.0 => (f * 1000.0) as u32,
                        Variant::Int(i) if i > 0 => (i as u32).saturating_mul(1000),
                        _ => 0,
                    };
                    // Which properties replicate. Godot keeps this in a
                    // `SceneReplicationConfig` resource; until that is loadable
                    // the `.tscn` carries the same list as
                    // `_sync_properties = ["position", …]`. An empty list means
                    // nothing syncs — `poll_outbound` already returns early on
                    // an empty snapshot, so a synchronizer with no config is
                    // inert rather than guessing at `position`.
                    let properties = replication_properties_from_node(node);
                    let watch_properties = watch_properties_from_node(node);
                    pending.push((
                        id,
                        Pending::Synchronizer {
                            root_path: variant_as_path_string(&node.get_property("root_path"))
                                .unwrap_or_default(),
                            interval_ms,
                            properties,
                            watch_properties,
                            // Godot's `public_visibility` defaults to TRUE
                            // (doc_classes/MultiplayerSynchronizer.xml), which
                            // is also what `MultiplayerSynchronizer::new` and
                            // the ClassDB registration use — so an absent or
                            // non-bool value means "visible to everyone".
                            //
                            // ⚠️ Until this was read, the visibility system was
                            // enforced but unreachable: `poll_outbound` already
                            // gates every send on `is_visible_to`, yet nothing
                            // in the scene layer ever set it, so a `.tscn`
                            // carrying `public_visibility = false` replicated to
                            // every peer regardless.
                            public_visibility: match node.get_property("public_visibility") {
                                Variant::Bool(b) => b,
                                _ => true,
                            },
                        },
                    ));
                }
                _ => {}
            }
        }

        for (id, cfg) in pending {
            match cfg {
                Pending::Spawner {
                    spawn_path,
                    spawn_limit,
                    scenes,
                } => {
                    let mut spawner = MultiplayerSpawner::new(spawn_limit);
                    spawner.set_spawn_path(spawn_path);
                    for scene in scenes {
                        spawner.add_spawnable_scene(scene);
                    }
                    self.replication.spawners.insert(id, spawner);
                }
                Pending::Synchronizer {
                    root_path,
                    interval_ms,
                    properties,
                    watch_properties,
                    public_visibility,
                } => {
                    let mut sync = MultiplayerSynchronizer::new(interval_ms);
                    sync.set_root_path(root_path);
                    for name in properties {
                        sync.add_property(name);
                    }
                    for name in watch_properties {
                        sync.add_watch_property(name);
                    }
                    sync.set_visibility_public(public_visibility);
                    self.replication.synchronizers.insert(id, sync);
                }
            }
            self.replication.registered.insert(id);
        }
    }

    /// Applies decoded inbound replication frames to this tree (pat-s3rrv.17).
    ///
    /// Godot's `SceneReplicationInterface` instantiates the spawned scene under
    /// the remote spawner's `spawn_path` and writes synchronized properties onto
    /// the live node; this is the same, with the `.tscn` fetched through the
    /// real resource loader rather than a caller-supplied closure.
    /// Removes the node a peer despawned (pat-s3rrv.17).
    ///
    /// Also drops it from `announced`, because the receiver marked it announced
    /// at birth to stop the echo cascade — leaving it there would keep a freed
    /// NodeId in the diff set, and NodeIds are reused.
    fn apply_replication_despawn(&mut self, net_id: u32) {
        let Some(node_id) = self.replication.replicator.forget(net_id) else {
            return;
        };
        for seen in self.replication.announced.values_mut() {
            seen.remove(&node_id);
        }
        self.replication.spawn_net_ids.remove(&node_id);
        let _ = self.remove_node(node_id);
    }

    fn apply_replication_packets(
        &mut self,
        spawns: Vec<(
            gdplatform::network::PeerId,
            gdplatform::network::SpawnPacket,
        )>,
        syncs: Vec<gdplatform::network::SyncPacket>,
    ) {
        // The replicator lives inside `self`, and its methods take `&mut SceneTree`
        // — so it has to come out for the duration of the call or the two
        // mutable borrows overlap. `MultiplayerReplicator: Default`, so the
        // placeholder left behind is an empty map, and it is put back below
        // (including on the early-return-free paths) so the `net_id -> NodeId`
        // mapping survives across frames.
        let mut replicator = std::mem::take(&mut self.replication.replicator);

        // Inbound spawns land under the mirrored `spawn_path`. Godot resolves
        // that path on the *receiving* peer's own spawner node, so a client
        // whose scene puts the spawner elsewhere still parents correctly.
        let owner = self.replication_spawn_owner();
        let spawn_parent = owner.map(|(_, parent)| parent).unwrap_or(self.root_id);
        for (from, spawn) in spawns {
            // 🔑 Only the spawner's authority may spawn on this peer. Godot:
            // `ERR_FAIL_COND_V(p_from != spawner->get_multiplayer_authority(),
            // ERR_UNAUTHORIZED)` (scene_replication_interface.cpp:577).
            //
            // ⚠️ Godot does NOT replicate authority — I assumed it did when I
            // recorded this item, and the C++ says otherwise. Both peers already
            // agree on the spawner's authority because the spawner is part of
            // the shared scene; what was missing is the receiver ENFORCING it.
            //
            // ⚠️ Only enforced when the receiver actually resolved a spawner.
            // With none, the node lands at the scene root through the
            // `spawn_parent` fallback above — already degenerate, and rejecting
            // there would change the
            // behaviour of trees that have no spawner at all.
            if let Some((spawner_id, _)) = owner {
                if i64::from(from.0) != self.get_multiplayer_authority(spawner_id) {
                    continue;
                }
            }
            // `Option` is `#[must_use]`; nothing needs the new id any more now
            // that the announced-at-birth bookkeeping is gone.
            let _ = replicator.apply_spawn(self, spawn_parent, &spawn, |path| {
                gdcore::file_access::FileAccess::read_file_string(path).ok()
            });
        }
        for sync in syncs {
            replicator.apply_sync(self, &sync);
        }

        self.replication.replicator = replicator;

        // ⚠️ The "mark inbound spawns announced-at-birth" workaround that used
        // to live here is GONE (pat-s3rrv.17 item 4), and the gate above is what
        // replaced it. It stopped an echo cascade by suppressing the symptom: a
        // received child looked "new" to the outbound diff, which announced it
        // back to the sender, which applied and re-announced it — one node per
        // frame on both trees, forever.
        //
        // The rule now expresses itself. An accepted spawn came from the
        // spawner's authority, and a peer never receives its own packets, so the
        // receiver is by construction NOT that authority — which means
        // `poll_replication_spawns` skips the spawner entirely and there is
        // nothing to echo. Removing the workaround BEFORE the gate existed would
        // have traded a suppressed symptom for a live echo, which is why the
        // ordering mattered.
    }

    /// Peers a replication frame should be addressed to (pat-s3rrv.17).
    ///
    /// ⚠️ Falls back to [`PeerId::SERVER`] when the peer list is empty, which is
    /// the normal case rather than an edge case: `MultiplayerAPI::peers` is
    /// populated from `PeerConnected` signals, and a peer installed directly
    /// with `set_multiplayer_peer` — how the engine, the editor and every test
    /// attach one — never runs that handshake. Gating replication on a
    /// non-empty list therefore sent *nothing, ever*. The RPC path already had
    /// this right: it addresses `authority_map.get(..).unwrap_or(PeerId::SERVER)`
    /// (network.rs:358-362) rather than consulting `get_peers`.
    fn replication_targets(&self) -> Vec<gdplatform::network::PeerId> {
        use gdplatform::network::PeerId;

        // `get_peers` answers Godot's signed peer ids; `PeerId` is unsigned, and
        // a non-positive id is not a real peer.
        let peers: Vec<PeerId> = self
            .multiplayer
            .get_peers()
            .into_iter()
            .filter(|id| *id > 0)
            .map(|id| PeerId(id as u32))
            .collect();
        if peers.is_empty() {
            vec![PeerId::SERVER]
        } else {
            peers
        }
    }

    /// Emits SPAWN frames for children added under a `MultiplayerSpawner`'s
    /// `spawn_path` since the last poll (pat-s3rrv.17).
    ///
    /// Godot's `SceneReplicationInterface` watches the spawn path and replicates
    /// each new child to every peer; this is the same diff, driven from the
    /// SceneTree poll.
    /// Announces children that have LEFT a spawner's `spawn_path` (pat-s3rrv.17).
    ///
    /// The inverse of the spawn diff: `announced` minus the live children is the
    /// set that was replicated and is now gone. Without this a host that frees a
    /// spawned node leaves every client's copy on screen permanently — the node
    /// simply stops receiving sync frames, which looks like a frozen entity
    /// rather than a removed one.
    fn poll_replication_despawns(&mut self) {
        use gdplatform::network::{encode_despawn_packet, Packet, TransferMode};

        if self.replication.spawners.is_empty() {
            return;
        }

        // Phase 1 — collect (spawner, gone child) pairs without holding a borrow
        // into `self.replication` across the sends below.
        let mut gone: Vec<(NodeId, NodeId)> = Vec::new();
        // Spawners whose own node has left the tree, kept so their registry
        // entries can be pruned once the sends below are done.
        let mut freed_spawners: Vec<NodeId> = Vec::new();
        for (spawner_id, announced) in &self.replication.announced {
            let Some(spawner) = self.replication.spawners.get(spawner_id) else {
                continue;
            };
            // Same rule as the spawn diff: a non-authority peer must not retract
            // a node it never announced.
            if !self.is_multiplayer_authority(*spawner_id) {
                continue;
            }
            // ⚠️ Godot despawns a spawner's replicated nodes when the SPAWNER
            // leaves the tree, not only when a child leaves `spawn_path`.
            // Without this branch the `spawn_path` lookup below cannot resolve
            // and simply `continue`s, so everything the spawner had announced
            // lives forever on the client.
            //
            // The distinction from that skip is the whole point: a freed
            // SPAWNER means "retract everything it announced", whereas a
            // spawn_path that will not resolve means "I cannot tell" — and
            // conflating the two would despawn the world on a client whose
            // scene merely resolves the path differently.
            //
            // Authority is deliberately still read FIRST, and it answers
            // correctly for a node that is already gone: `get_multiplayer_authority`
            // walks to the missing node, breaks, and falls back to SERVER — so
            // the host retracts and a client does not.
            if self.get_node(*spawner_id).is_none() {
                gone.extend(announced.iter().map(|child| (*spawner_id, *child)));
                freed_spawners.push(*spawner_id);
                continue;
            }
            let path = spawner.get_spawn_path();
            let parent = if path.is_empty() {
                Some(*spawner_id)
            } else {
                self.get_node_or_null(*spawner_id, path)
            };
            // ⚠️ A missing parent is NOT an empty child list. If the spawn_path
            // itself has been freed, treating it as "no children" would despawn
            // every node under it — including on a client whose scene simply
            // resolves the path differently. Skip instead.
            let Some(parent) = parent else {
                continue;
            };
            let Some(parent_node) = self.get_node(parent) else {
                continue;
            };
            let live = parent_node.children();
            for child in announced {
                if !live.contains(child) {
                    gone.push((*spawner_id, *child));
                }
            }
        }
        // ⚠️ `freed_spawners` is checked too: a spawner can leave the tree
        // having announced nothing, and returning early there would leak its
        // registry entry forever.
        if gone.is_empty() && freed_spawners.is_empty() {
            return;
        }

        let peers = self.replication_targets();
        let sender = self.multiplayer.peer_mut().map(|p| p.get_unique_id());
        for (spawner_id, child) in gone {
            // Forget it regardless of whether a packet goes out, so a child that
            // was announced without a net_id (a spawner at its limit) cannot be
            // rescanned every frame forever.
            let net_id = self.replication.spawn_net_ids.remove(&child);
            if let Some(seen) = self.replication.announced.get_mut(&spawner_id) {
                seen.remove(&child);
            }
            let (Some(net_id), Some(sender)) = (net_id, sender) else {
                continue;
            };
            let encoded = encode_despawn_packet(net_id);
            if let Some(peer) = self.multiplayer.peer_mut() {
                for to in &peers {
                    let _ = peer.send_packet(
                        *to,
                        Packet {
                            sender,
                            channel: 0,
                            data: encoded.clone(),
                            transfer_mode: TransferMode::Reliable,
                        },
                    );
                }
            }
        }

        // Prune the freed spawners themselves. Registration only ever INSERTS
        // (it is guarded by `registered.contains`), so without this a freed
        // spawner is rescanned every frame forever — and because `NodeId`s can
        // be reused, a stale entry could later attach to an unrelated node that
        // happens to land on the same id.
        for spawner_id in freed_spawners {
            self.replication.spawners.remove(&spawner_id);
            self.replication.announced.remove(&spawner_id);
            self.replication.registered.remove(&spawner_id);
        }
    }

    fn poll_replication_spawns(&mut self) {
        use gdplatform::network::{encode_spawn_packet, Packet, TransferMode};

        if self.replication.spawners.is_empty() {
            return;
        }
        let peers = self.replication_targets();

        // Phase 1 — diff live children against what has already been announced.
        let mut pending: Vec<(NodeId, Vec<NodeId>)> = Vec::new();
        for (spawner_id, spawner) in &self.replication.spawners {
            // Only the spawner's authority replicates it (pat-s3rrv.17). Godot's
            // rule — and here it also REPLACES a workaround: the receiver marks
            // inbound spawns announced-at-birth purely to stop an echo cascade,
            // which suppressed the symptom without expressing the rule.
            //
            // ⚠️ Deliberately NOT applied to `replication_spawn_owner`, which
            // has an identical loop header: that one resolves where INBOUND
            // spawns are parented, and the receiver is by definition not the
            // authority. Gating it would drop every arriving node.
            if !self.is_multiplayer_authority(*spawner_id) {
                continue;
            }
            let path = spawner.get_spawn_path();
            let parent = if path.is_empty() {
                Some(*spawner_id)
            } else {
                self.get_node_or_null(*spawner_id, path)
            };
            let Some(parent) = parent else {
                continue;
            };
            let Some(parent_node) = self.get_node(parent) else {
                continue;
            };
            let known = self.replication.announced.get(spawner_id);
            let fresh: Vec<NodeId> = parent_node
                .children()
                .iter()
                .copied()
                .filter(|child| known.map(|k| !k.contains(child)).unwrap_or(true))
                .collect();
            if !fresh.is_empty() {
                pending.push((*spawner_id, fresh));
            }
        }
        if pending.is_empty() {
            return;
        }

        // Phase 2 — announce each new child exactly once, to every peer.
        //
        // ⚠️ Deliberately NOT `spawn_and_notify` per peer: that calls `spawn()`
        // internally, which allocates a fresh `net_id` every time, so one child
        // would arrive at N peers under N different ids and later SYNC frames
        // would address a node the receiver never mapped. Spawn once, then fan
        // the same encoded packet out.
        let sender = self.multiplayer.peer_mut().map(|p| p.get_unique_id());
        let Some(sender) = sender else {
            return;
        };
        for (spawner_id, children) in pending {
            for child in children {
                // Resolved in its own statement so the `spawners` borrow ends
                // before `announced` is touched below — nesting that mutation
                // inside a `match` on `spawners.get_mut(..)` would hold two
                // borrows into `self.replication` at once.
                // ⚠️ Gathered BEFORE the `spawners` borrow below, and from the
                // scene rather than the spawner: property values live on nodes,
                // which gdplatform cannot reach. Godot splits it the same way —
                // the spawner allocates the id, and
                // `SceneReplicationInterface::_make_spawn_packet` collects the
                // state.
                let spawn_state = self.spawn_state_for_subtree(child);
                let request = self
                    .replication
                    .spawners
                    .get_mut(&spawner_id)
                    .and_then(|spawner| spawner.spawn_with_properties("", spawn_state).ok());
                let Some(request) = request else {
                    // Spawn limit reached, or no spawnable scene registered:
                    // still mark the child announced so a failing spawner does
                    // not retry it on every frame forever.
                    self.replication
                        .announced
                        .entry(spawner_id)
                        .or_default()
                        .insert(child);
                    continue;
                };
                let encoded = encode_spawn_packet(&request);
                let net_id = request.net_id;
                // Bind any synchronizer inside the spawned subtree to this
                // spawn's `net_id`.
                //
                // ⚠️ Without this the whole sync path is dead on arrival: a
                // `MultiplayerSynchronizer` starts at `net_id` 0, so its SYNC
                // frames address 0, while the receiver's replicator mapped the
                // node under the spawn's id — `apply_sync` would look up 0,
                // find nothing, and silently drop every update. Godot gets this
                // for free because the synchronizer *is* part of the spawned
                // scene and shares its object id.
                let mut subtree = Vec::new();
                self.collect_subtree_top_down(child, &mut subtree);
                for id in subtree {
                    if let Some(sync) = self.replication.synchronizers.get_mut(&id) {
                        sync.set_net_id(net_id);
                    }
                }

                if let Some(peer) = self.multiplayer.peer_mut() {
                    for to in &peers {
                        let _ = peer.send_packet(
                            *to,
                            Packet {
                                sender,
                                channel: 0,
                                data: encoded.clone(),
                                transfer_mode: TransferMode::Reliable,
                            },
                        );
                    }
                }
                self.replication.spawn_net_ids.insert(child, net_id);
                self.replication
                    .announced
                    .entry(spawner_id)
                    .or_default()
                    .insert(child);
            }
        }
    }

    /// Emits SYNC frames for every registered `MultiplayerSynchronizer` whose
    /// `replication_interval` has elapsed (pat-s3rrv.17).
    ///
    /// The spawn-state values for every synchronizer inside a freshly spawned
    /// subtree (pat-s3rrv.17).
    ///
    /// `SceneReplicationConfig::spawn_properties()` recorded which properties
    /// are spawn-flagged and **nothing ever sent them** — the config had exactly
    /// one caller repo-wide and it was a test assertion. So a spawned node
    /// arrived on remote peers with its scene defaults, and only started
    /// matching once the first periodic SYNC frame landed.
    ///
    /// ⚠️ Key resolution is deliberately identical to the sync snapshot: a key
    /// carrying a colon targets a CHILD of the synchronizer's root
    /// (`Sprite2D:modulate`), a bare key is the root's own. Resolving them
    /// differently here would make the same config line mean two things
    /// depending on which packet carried it.
    fn spawn_state_for_subtree(&self, root_of_spawn: NodeId) -> Vec<(String, Variant)> {
        let mut subtree = Vec::new();
        self.collect_subtree_top_down(root_of_spawn, &mut subtree);
        let mut out: Vec<(String, Variant)> = Vec::new();
        for sync_id in subtree {
            let Some(sync_node) = self.get_node(sync_id) else {
                continue;
            };
            if sync_node.class_name() != "MultiplayerSynchronizer" {
                continue;
            }
            let keys = spawn_properties_from_node(sync_node);
            if keys.is_empty() {
                continue;
            }
            // Godot's `root_path` defaults to ".." — the synchronizer replicates
            // its parent, not itself.
            let root_path =
                variant_as_path_string(&sync_node.get_property("root_path")).unwrap_or_default();
            let root = if root_path.is_empty() {
                Some(sync_id)
            } else {
                self.get_node_or_null(sync_id, &root_path)
            };
            let Some(root) = root else {
                continue;
            };
            let Some(root_node) = self.get_node(root) else {
                continue;
            };
            for key in keys {
                let value = match key.rsplit_once(':') {
                    Some((subpath, prop)) => {
                        let Some(child) = self.get_node_or_null(root, subpath) else {
                            continue;
                        };
                        let Some(child_node) = self.get_node(child) else {
                            continue;
                        };
                        child_node.get_property(prop)
                    }
                    None => root_node.get_property(&key),
                };
                // An unset property reads back `Nil`; sending it would clobber a
                // real value on the receiving peer — the same rule the sync
                // snapshot applies.
                if !value.is_nil() {
                    out.push((key, value));
                }
            }
        }
        out
    }

    /// Mirrors Godot's `SceneReplicationInterface::_send_sync`: snapshot the
    /// configured properties off the live node, hand them to the synchronizer,
    /// and let it decide (on `replication_interval`) whether this tick actually
    /// sends. Values travel as binary Variants, so a `Vector3` or `Color`
    /// survives the trip.
    fn poll_replication_outbound(&mut self) {
        if self.replication.synchronizers.is_empty() {
            return;
        }
        let peers = self.replication_targets();

        // Phase 1 — read the live values. Borrows the node arena immutably, so
        // it cannot be fused with the write-back below.
        let mut snapshots: Vec<(NodeId, Vec<(String, Variant)>)> = Vec::new();
        for (id, sync) in &self.replication.synchronizers {
            // ⚠️ Both lists, not just sync: a synchronizer configured entirely
            // with REPLICATION_MODE_ON_CHANGE properties has an empty sync list
            // and would otherwise be skipped before its values were ever read.
            if sync.properties_to_sync.is_empty() && sync.properties_to_watch.is_empty() {
                continue;
            }
            // Godot's `root_path` defaults to ".." — the synchronizer replicates
            // its *parent*, not itself.
            let root_path = sync.get_root_path();
            let root = if root_path.is_empty() {
                Some(*id)
            } else {
                self.get_node_or_null(*id, root_path)
            };
            let Some(root) = root else {
                continue;
            };
            let Some(node) = self.get_node(root) else {
                continue;
            };
            // A key carrying a colon targets a CHILD of the root
            // (`Sprite2D:modulate`); resolve it relative to the root and read
            // the property off that node. A bare key is the root's own.
            //
            // Written as a plain loop rather than a `filter_map` closure: the
            // child lookup needs `&self` while `node` is already borrowed from
            // it, and keeping both as ordinary shared borrows in a loop body is
            // the shape the surrounding code already proves compiles.
            let mut values: Vec<(String, Variant)> = Vec::new();
            // 🔑 Watched properties are read here as well. Change detection
            // compares the live value against the last one SENT, so a watched
            // property whose value was never sampled looks permanently absent and
            // never travels at all.
            for key in sync
                .properties_to_sync
                .iter()
                .chain(sync.properties_to_watch.iter())
            {
                let value = match key.rsplit_once(':') {
                    Some((subpath, prop)) => {
                        let Some(child) = self.get_node_or_null(root, subpath) else {
                            continue;
                        };
                        let Some(child_node) = self.get_node(child) else {
                            continue;
                        };
                        child_node.get_property(prop)
                    }
                    None => node.get_property(key),
                };
                // An unset property reads back `Nil`; replicating that would
                // clobber a real value on the receiving peer.
                if !value.is_nil() {
                    values.push((key.clone(), value));
                }
            }
            if !values.is_empty() {
                snapshots.push((*id, values));
            }
        }

        // Phase 2 — write the snapshot into each synchronizer.
        for (id, values) in snapshots {
            if let Some(sync) = self.replication.synchronizers.get_mut(&id) {
                for (name, value) in values {
                    sync.set_property_value(&name, value);
                }
            }
        }

        // Phase 2b — decide which (synchronizer, peer) pairs the visibility
        // filters veto this frame (pat-s3rrv.17).
        //
        // ⚠️ The verdict is NOT written into the synchronizer's per-peer
        // visibility map. Godot evaluates filters inside `is_visible_to` at
        // QUERY time (multiplayer_synchronizer.cpp:193-207) and leaves
        // `peer_visibility` alone, so an explicit `set_visibility_for` survives
        // a filter pass. Storing the verdict would silently overwrite it.
        //
        // Effective visibility is therefore `filters AND stored`, which is
        // exactly Godot's formula: the veto set below suppresses the send, and
        // `poll_outbound`'s existing `is_visible_to` still applies the stored
        // public/explicit state.
        let mut vetoed: HashSet<(NodeId, u32)> = HashSet::new();
        if !self.replication.visibility_filters.is_empty() {
            self.replication.filter_verdicts.clear();
            let sync_ids: Vec<NodeId> = self
                .replication
                .visibility_filters
                .keys()
                .copied()
                .filter(|id| self.replication.synchronizers.contains_key(id))
                .collect();
            let peer_ids: Vec<u32> = peers.iter().map(|p| p.0).collect();
            for sync_id in sync_ids {
                for &peer_id in &peer_ids {
                    let allowed = self.visibility_filters_allow(sync_id, peer_id);
                    self.replication
                        .filter_verdicts
                        .insert((sync_id, peer_id), allowed);
                    if !allowed {
                        vetoed.insert((sync_id, peer_id));
                    }
                }
            }
        }

        // Phase 3 — send. `multiplayer` and `replication` are disjoint fields,
        // so both can be borrowed mutably here.
        let now_ms = self.replication.clock_ms;
        let Some(peer) = self.multiplayer.peer_mut() else {
            return;
        };
        for (sync_id, sync) in self.replication.synchronizers.iter_mut() {
            for to in &peers {
                // A filter veto suppresses the send without touching the stored
                // visibility, so the two conditions AND rather than one
                // overwriting the other.
                if vetoed.contains(&(*sync_id, to.0)) {
                    continue;
                }
                // `force = false`: the synchronizer applies its own interval.
                let _ = sync.poll_outbound(peer, *to, now_ms, false);
            }
        }
    }

    /// The spawner that owns inbound spawns and the node they parent under:
    /// the first registered `MultiplayerSpawner` whose `spawn_path` resolves.
    ///
    /// Returns the spawner's own id too, because the caller must mark what it
    /// applies as already-announced FOR THAT SPAWNER — see
    /// `apply_replication_packets`. `None` when the scene declares no spawner
    /// or no path resolves; the caller then parents at the tree root.
    fn replication_spawn_owner(&self) -> Option<(NodeId, NodeId)> {
        for (spawner_id, spawner) in &self.replication.spawners {
            let path = spawner.get_spawn_path();
            if path.is_empty() {
                continue;
            }
            if let Some(target) = self.get_node_or_null(*spawner_id, path) {
                return Some((*spawner_id, target));
            }
        }
        None
    }

    /// Polls the multiplayer transport once (call once per frame) and dispatches
    /// every decoded inbound RPC to its target node's script method.
    ///
    /// The wire `node_path` is matched against the [`rpc_path_hash`] of each
    /// live node's path, so a peer that invokes `rpc(rpc_path_hash(path), …)`
    /// reaches the node at that path on the receiver.
    pub fn poll_multiplayer(&mut self) {
        self.poll_multiplayer_with_delta(0.0);
    }

    /// [`poll_multiplayer`](Self::poll_multiplayer) with the frame delta, which
    /// additionally ages the authentication timers (pat-0q2kx.94).
    ///
    /// Re-emits every queued [`MultiplayerEvent`](gdplatform::network::MultiplayerEvent)
    /// as the identically-named Godot signal on the scene root, which is where
    /// `multiplayer.peer_connected.connect(...)` binds. Before this, the API
    /// tracked peers with no signal mechanism at all, so a script could not
    /// learn that anyone had joined.
    /// Invokes the `SceneMultiplayer` auth callback for a peer that is
    /// authenticating (pat-s3rrv.18).
    ///
    /// Godot's signature is `func _auth(id: int, data: PackedByteArray)`. The
    /// payload is empty when the peer sent none — Godot passes the bytes it
    /// received, which for a peer that simply connected is nothing.
    ///
    /// Only `Callable(object, "method")` is dispatchable here: a lambda's body
    /// lives in the interpreter, and the tree has no way to run one outside a
    /// script call frame. A lambda callback is therefore ignored rather than
    /// silently reported as run.
    fn run_multiplayer_auth_callback(&mut self, peer_id: i32) {
        let callback = self.multiplayer_auth_callback.clone();
        let Variant::Callable(callable) = callback else {
            return;
        };
        let gdvariant::CallableRef::Method { target_id, method } = callable.inner_callable() else {
            return;
        };
        let (target_id, method) = (*target_id, method.clone());
        let args = vec![
            Variant::Int(i64::from(peer_id)),
            Variant::PackedByteArray(Vec::new()),
        ];
        let node = NodeId::from_object_id(gdcore::id::ObjectId::from_raw(target_id));
        let _ = self.call_script_method(node, &method, &args);
    }

    pub fn poll_multiplayer_with_delta(&mut self, delta: f32) {
        use gdplatform::network::MultiplayerEvent;

        // Age the auth timers before polling, so a peer that ran out this frame
        // fails in the same drain rather than one frame late.
        self.multiplayer.poll_auth(delta);
        // Scene replication: SPAWN/SYNC frames (pat-s3rrv.17).
        //
        // These used to be bound to `_other` and dropped on the floor every
        // frame with the comment "left for higher layers" — but no higher layer
        // existed: `MultiplayerReplicator` had zero callers outside its own
        // tests, so a spawn or sync that arrived over the wire reached nothing.
        //
        // ⚠️ Registration runs BEFORE the inbound drain, not after. Godot
        // registers a spawner with the MultiplayerAPI on
        // `NOTIFICATION_POST_ENTER_TREE`, long before any packet is processed,
        // and the order matters here for a concrete reason:
        // `replication_spawn_owner` resolves the mirrored parent by walking
        // the REGISTERED spawners. On a receiver's very first poll that map is
        // still empty, so the lookup fell through to its `self.root_id`
        // fallback and parented the spawned node at the scene ROOT instead of
        // under `spawn_path`. `multiplayer.poll()` drains the inbox, so the
        // frame that got this wrong was also the only frame that ever saw the
        // packet — the node existed, at the wrong path, and every later frame
        // had nothing left to apply.
        self.register_replication_nodes();
        let other = self.multiplayer.poll();
        let mut spawns = Vec::new();
        let mut syncs = Vec::new();
        let mut despawns: Vec<u32> = Vec::new();
        for packet in &other {
            // Spawn first: both decoders are tagged by a leading kind byte, so
            // a frame can only match one, and trying spawn first mirrors the
            // order the two arrive in for a newly replicated node.
            if let Some(spawn) = gdplatform::network::decode_spawn_packet(&packet.data) {
                // The SENDER travels with the packet: authorizing a spawn needs
                // to know who sent it (pat-s3rrv.17 item 5).
                spawns.push((packet.sender, spawn));
            } else if let Some(sync) = gdplatform::network::decode_sync_packet(&packet.data) {
                syncs.push(sync);
            } else if let Some(net_id) = gdplatform::network::decode_despawn_packet(&packet.data) {
                despawns.push(net_id);
            }
        }
        if !spawns.is_empty() || !syncs.is_empty() {
            self.apply_replication_packets(spawns, syncs);
        }
        // After the spawns, so a frame carrying both applies them in the order
        // they were produced rather than deleting a node that arrives later in
        // the same drain.
        for net_id in despawns {
            self.apply_replication_despawn(net_id);
        }
        // Outbound replication runs after inbound so a value that arrived this
        // frame is not immediately echoed back to its sender.
        self.replication.clock_ms = self
            .replication
            .clock_ms
            .saturating_add((delta * 1000.0).max(0.0) as u64);
        // (registration already ran above, before the inbound drain)
        self.poll_replication_spawns();
        // After spawns so a node added and removed within one frame is announced
        // before it is retracted, and before outbound sync so a removed node
        // does not also emit a property frame on its way out.
        self.poll_replication_despawns();
        self.poll_replication_outbound();

        let root = self.root_id;
        for event in self.multiplayer.take_events() {
            // Godot runs `SceneMultiplayer`'s auth callback as the peer starts
            // authenticating — BEFORE `peer_connected`, which only fires once
            // the script accepts with `complete_auth` (pat-s3rrv.18). Invoked
            // here rather than at the signal layer because the callback is a
            // decision point, not a notification: `set_auth_callback` had no
            // effect at all before this.
            if let MultiplayerEvent::PeerAuthenticating(id) = event {
                self.run_multiplayer_auth_callback(id);
            }
            let (signal, args): (&str, Vec<Variant>) = match event {
                MultiplayerEvent::PeerConnected(id) => {
                    ("peer_connected", vec![Variant::Int(id as i64)])
                }
                MultiplayerEvent::PeerDisconnected(id) => {
                    ("peer_disconnected", vec![Variant::Int(id as i64)])
                }
                MultiplayerEvent::ConnectedToServer => ("connected_to_server", Vec::new()),
                MultiplayerEvent::ConnectionFailed => ("connection_failed", Vec::new()),
                MultiplayerEvent::ServerDisconnected => ("server_disconnected", Vec::new()),
                MultiplayerEvent::PeerPacket { id, packet } => (
                    "peer_packet",
                    vec![Variant::Int(id as i64), Variant::PackedByteArray(packet)],
                ),
                MultiplayerEvent::PeerAuthenticating(id) => {
                    ("peer_authenticating", vec![Variant::Int(id as i64)])
                }
                MultiplayerEvent::PeerAuthenticationFailed(id) => {
                    ("peer_authentication_failed", vec![Variant::Int(id as i64)])
                }
            };
            self.emit_signal(root, signal, &args);
        }

        let inbound = self.multiplayer.take_inbound_rpcs();
        for rpc in inbound {
            if let Some(node_id) = self.node_for_rpc_hash(rpc.node_path) {
                // Godot's `get_remote_sender_id()` answers for the duration of
                // the handler, which is the only window in which "who called
                // me?" has an answer.
                self.multiplayer.set_remote_sender_id(rpc.sender_id);
                self.call_script_with_access(node_id, &rpc.method_name, &rpc.args);
                self.multiplayer
                    .set_remote_sender_id(gdplatform::network::PeerId(0));
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
    /// Godot's defaults: `process_always = true`, everything else false.
    pub fn create_timer(&mut self, seconds: f64) -> u64 {
        self.create_timer_with_flags(seconds, true, false, false)
    }

    /// `SceneTree.create_timer(delay, process_always, process_in_physics,
    /// ignore_time_scale)` (pat-s3rrv.35).
    ///
    /// Kept separate from [`create_timer`](Self::create_timer) rather than
    /// widening that signature: it has callers across three crates, and Rust has
    /// no default arguments, so a widened signature would have to be edited in
    /// at every one of them for no gain.
    pub fn create_timer_with_flags(
        &mut self,
        seconds: f64,
        process_always: bool,
        process_in_physics: bool,
        ignore_time_scale: bool,
    ) -> u64 {
        let id = self.next_timer_id;
        self.next_timer_id = self.next_timer_id.saturating_add(1);
        let signal_name = format!("timeout_{id}");
        self.scene_timers.push(SceneTreeTimer {
            id,
            time_left: seconds.max(0.0),
            done: false,
            signal_name,
            process_always,
            process_in_physics,
            ignore_time_scale,
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
        self.process_timers_phase(delta, delta, false);
    }

    /// Advances the timers belonging to one phase of the frame (pat-s3rrv.35).
    ///
    /// Mirrors Godot's `SceneTree::process_timers(delta, physics_frame)`: a
    /// timer is advanced by the idle pass or by the physics tick, never both,
    /// selected by its `process_in_physics` flag. `delta` is the scaled frame
    /// delta; `unscaled_delta` is the driver's real one, used for timers created
    /// with `ignore_time_scale`.
    ///
    /// Timers without `process_always` are frozen while the tree is paused.
    pub fn process_timers_phase(&mut self, delta: f64, unscaled_delta: f64, in_physics: bool) {
        let paused = self.paused;
        for t in &mut self.scene_timers {
            if t.done || t.process_in_physics != in_physics {
                continue;
            }
            if paused && !t.process_always {
                continue;
            }
            t.time_left -= if t.ignore_time_scale {
                unscaled_delta
            } else {
                delta
            };
            if t.time_left <= 0.0 {
                t.time_left = 0.0;
                t.done = true;
            }
        }
    }

    /// Drain signal names for timers that timed out (for await resume).
    /// Also removes finished timers after reporting.
    /// Records a URI for the host to open (pat-s3rrv.166).
    pub fn request_shell_open(&mut self, uri: impl Into<String>) {
        self.pending_shell_open.push(uri.into());
    }

    /// Drains the pending `shell_open` URIs.
    ///
    /// Draining is what makes a second press observable as a second request
    /// rather than folding into the first.
    pub fn take_shell_open_requests(&mut self) -> Vec<String> {
        std::mem::take(&mut self.pending_shell_open)
    }

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
        self.process_script_coroutines_unscaled(delta, delta as f64);
    }

    /// [`process_script_coroutines`](Self::process_script_coroutines) with the
    /// driver's real delta alongside the scaled one (pat-s3rrv.35).
    ///
    /// Only `ignore_time_scale` timers read `unscaled_delta`; everything else
    /// uses `delta`. `MainLoop::step` calls this form because it is the only
    /// caller that knows both, and the plain form forwards `delta` for both so
    /// no existing caller changes behaviour.
    pub fn process_script_coroutines_unscaled(&mut self, delta: f32, unscaled_delta: f64) {
        // The single idle-phase advance for SceneTreeTimers. Physics-phase
        // timers are advanced by the caller's fixed-step loop instead.
        self.process_timers_phase(delta as f64, unscaled_delta, false);
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

    /// Binds `server` as the physics server that scripts reach through the
    /// `PhysicsServer2D` singleton for the duration of a script phase
    /// (pat-0q2kx.200).
    ///
    /// The caller must call [`SceneTree::clear_script_physics_server`] before
    /// `server` can move or drop. `MainLoop` owns the tree and the server as
    /// sibling fields and brackets the script phase with these two calls, so the
    /// address cannot dangle. With nothing bound, `PhysicsServer2D` calls report
    /// failure rather than reaching a stale pointer.
    pub fn set_script_physics_server(&mut self, server: &mut crate::physics_server::PhysicsServer) {
        self.script_physics = server as *mut crate::physics_server::PhysicsServer as usize;
    }

    /// Unbinds the script-facing physics server. Always call this before the
    /// bound server moves or drops.
    pub fn clear_script_physics_server(&mut self) {
        self.script_physics = 0;
    }

    /// The bound script-facing physics server address, or 0 when none is bound.
    pub(crate) fn script_physics_addr(&self) -> usize {
        self.script_physics
    }

    /// Binds the 3D physics server scripts reach through
    /// `get_world_3d().direct_space_state` for the duration of a script phase
    /// (pat-s3rrv.107). The 3D twin of `set_script_physics_server`.
    pub fn set_script_physics_server_3d(
        &mut self,
        server: &mut crate::physics_server_3d::PhysicsServer3D,
    ) {
        self.script_physics_3d = server as *mut crate::physics_server_3d::PhysicsServer3D as usize;
    }

    /// Unbinds the script-facing 3D physics server.
    ///
    /// ⚠️ ALWAYS call this before the bound server can move or drop — it is the
    /// half of the pair that matters. A missed BIND merely leaves the address at
    /// 0, which reads as "no server" and answers queries emptily; a missed
    /// CLEAR leaves a dangling address.
    pub fn clear_script_physics_server_3d(&mut self) {
        self.script_physics_3d = 0;
    }

    /// The bound script-facing 3D physics server address, or 0 when none.
    pub(crate) fn script_physics_3d_addr(&self) -> usize {
        self.script_physics_3d
    }

    /// Binds `server` as the audio server that scripts reach through the
    /// `AudioServer` singleton for the duration of a script phase
    /// (pat-s3rrv.6).
    ///
    /// Same contract as [`SceneTree::set_script_physics_server`]: the caller
    /// must call [`SceneTree::clear_script_audio_server`] before `server` can
    /// move or drop. `MainLoop` owns the tree and the `AudioServerBridge` as
    /// sibling fields and brackets each script phase with these two calls, so
    /// the address cannot dangle. With nothing bound, `AudioServer` calls
    /// report failure rather than reaching a stale pointer.
    pub fn set_script_audio_server(&mut self, server: &mut gdaudio::AudioServer) {
        self.script_audio = server as *mut gdaudio::AudioServer as usize;
    }

    /// Unbinds the script-facing audio server. Always call this before the
    /// bound server moves or drops.
    pub fn clear_script_audio_server(&mut self) {
        self.script_audio = 0;
    }

    /// The bound script-facing audio server address, or 0 when none is bound.
    pub(crate) fn script_audio_addr(&self) -> usize {
        self.script_audio
    }

    /// The `SceneMultiplayer` auth callback a script registered, or `Nil`.
    pub fn multiplayer_auth_callback(&self) -> Variant {
        self.multiplayer_auth_callback.clone()
    }

    /// Sets (or clears, with `Nil`) the auth callback (pat-s3rrv.18).
    pub fn set_multiplayer_auth_callback(&mut self, callback: Variant) {
        self.multiplayer_auth_callback = callback;
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

    /// Mirrored default cursor shape, as Godot's
    /// `Input.get_current_cursor_shape()` reports it (pat-0q2kx.90). `0` is
    /// `CURSOR_ARROW`, Godot's default.
    pub fn cursor_shape(&self) -> i64 {
        self.cursor_shape
    }

    /// Updates the mirrored cursor shape — the cursor counterpart of
    /// [`set_input_mouse_mode_mirror`](Self::set_input_mouse_mode_mirror), and
    /// mirrored for the same reason: the command is applied by the frame owner,
    /// but a script reads the shape back immediately.
    pub fn set_cursor_shape_mirror(&mut self, shape: i64) {
        self.cursor_shape = shape;
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

    // -- InputMap mirror (pat-0q2kx.89) -------------------------------------

    /// Whether the mirrored `InputMap` has `action` (`InputMap.has_action`).
    pub fn input_map_has_action(&self, action: &str) -> bool {
        self.input_map_events.contains_key(action)
    }

    /// Mirrored action names, sorted (`InputMap.get_actions`).
    pub fn input_map_actions(&self) -> Vec<String> {
        let mut names: Vec<String> = self.input_map_events.keys().cloned().collect();
        names.sort();
        names
    }

    /// How many events the mirror records for `action`.
    pub fn input_map_action_event_count(&self, action: &str) -> usize {
        self.input_map_events.get(action).copied().unwrap_or(0)
    }

    /// Replaces the whole mirror from the engine map. Called by the frame owner
    /// after applying queued commands.
    pub fn set_input_map_mirror(&mut self, actions: std::collections::HashMap<String, usize>) {
        self.input_map_events = actions;
    }

    /// The bindings mirrored for `action` (pat-s3rrv.14).
    ///
    /// Empty for an action that exists but has no engine binding — which is a
    /// real state, not an error: `InputMap.add_action` creates exactly that.
    pub fn input_map_action_bindings(&self, action: &str) -> &[gdplatform::input::ActionBinding] {
        self.input_map_bindings
            .get(action)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    /// Replaces the binding mirror from the engine map (pat-s3rrv.14). Called by
    /// the frame owner beside [`Self::set_input_map_mirror`].
    pub fn set_input_map_binding_mirror(
        &mut self,
        bindings: std::collections::HashMap<String, Vec<gdplatform::input::ActionBinding>>,
    ) {
        self.input_map_bindings = bindings;
    }

    /// Records a script's `InputMap.add_action`. An existing action keeps its
    /// event count, matching Godot: re-adding a live action is a no-op there.
    pub fn input_map_mirror_add_action(&mut self, action: &str) {
        self.input_map_events.entry(action.to_string()).or_insert(0);
    }

    /// Records a script's `InputMap.erase_action`.
    pub fn input_map_mirror_erase_action(&mut self, action: &str) {
        self.input_map_events.remove(action);
    }

    /// Records a script's `InputMap.action_add_event`. Only counts against an
    /// action that exists, mirroring `InputMap::action_add_event`'s guard.
    pub fn input_map_mirror_add_event(&mut self, action: &str) {
        if let Some(count) = self.input_map_events.get_mut(action) {
            *count += 1;
        }
    }

    /// Records a script's `InputMap.action_erase_event`.
    pub fn input_map_mirror_erase_event(&mut self, action: &str) {
        if let Some(count) = self.input_map_events.get_mut(action) {
            *count = count.saturating_sub(1);
        }
    }

    /// Records a script's `InputMap.action_erase_events`.
    pub fn input_map_mirror_erase_events(&mut self, action: &str) {
        if let Some(count) = self.input_map_events.get_mut(action) {
            *count = 0;
        }
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
            self.forget_replication_entries(nid);
        }

        Ok(removed)
    }

    /// Drops every replication registry entry belonging to a removed node
    /// (pat-s3rrv.17 item 6).
    ///
    /// `remove_node` cleaned the arena, the weak-ref table and the group index
    /// and left the replication maps alone, so a freed `MultiplayerSynchronizer`
    /// or `MultiplayerSpawner` kept its entry forever. A game that spawns and
    /// frees units continuously grew `synchronizers` without bound and walked
    /// the dead entries on every outbound pass.
    ///
    /// ⚠️ Not a correctness bug today, and it is worth being precise about why:
    /// `NodeId` wraps a monotonic `AtomicU64` (`gdcore/src/id.rs:19`), so ids are
    /// never recycled and a later node cannot inherit a dead synchronizer's
    /// config. Were ids pooled, this leak would be a silent misconfiguration
    /// rather than only a leak.
    ///
    /// `announced` is cleaned from BOTH sides: a removed spawner drops its whole
    /// set, and a removed child is dropped from whichever spawner announced it —
    /// otherwise the child ids accumulate in a live spawner's set forever.
    fn forget_replication_entries(&mut self, nid: NodeId) {
        self.replication.spawners.remove(&nid);
        self.replication.synchronizers.remove(&nid);
        self.replication.registered.remove(&nid);
        self.replication.visibility_filters.remove(&nid);
        self.replication.announced.remove(&nid);
        self.replication.spawn_net_ids.remove(&nid);
        for announced in self.replication.announced.values_mut() {
            announced.remove(&nid);
        }
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
    /// Godot `Node.get_multiplayer_authority()` (pat-s3rrv.17).
    ///
    /// Stored per node under `multiplayer_authority`. Absent means INHERIT: walk
    /// up to the nearest ancestor that declares one, defaulting to the server —
    /// what Godot seeds every node with, and why a scene that never mentions
    /// authority still behaves server-authoritative.
    pub fn get_multiplayer_authority(&self, node_id: NodeId) -> i64 {
        let mut cursor = Some(node_id);
        while let Some(id) = cursor {
            let Some(node) = self.get_node(id) else {
                break;
            };
            if let Variant::Int(v) = node.get_property("multiplayer_authority") {
                return v;
            }
            cursor = node.parent();
        }
        i64::from(gdplatform::network::PeerId::SERVER.0)
    }

    /// Godot `Node.set_multiplayer_authority(id, recursive = true)`.
    ///
    /// ⚠️ Recursive is Godot's DEFAULT and is not a convenience: authority is
    /// read per node, so setting it on a spawned root alone leaves every child
    /// inheriting whatever the ancestor chain says — which for a node parented
    /// under the scene root is the server, not the new owner.
    pub fn set_multiplayer_authority(&mut self, node_id: NodeId, authority: i64, recursive: bool) {
        let mut targets = Vec::new();
        if recursive {
            self.collect_subtree_top_down(node_id, &mut targets);
        } else {
            targets.push(node_id);
        }
        for id in targets {
            if let Some(node) = self.get_node_mut(id) {
                node.set_property("multiplayer_authority", Variant::Int(authority));
            }
        }
    }

    /// Whether THIS peer is the authority for `node_id`.
    ///
    /// With no peer set the answer is `true`: a single-player tree owns
    /// everything, and gating replication on `false` there would silently
    /// disable the spawner in every offline test.
    pub fn is_multiplayer_authority(&self, node_id: NodeId) -> bool {
        let Some(peer) = self.multiplayer.peer() else {
            return true;
        };
        i64::from(peer.get_unique_id().0) == self.get_multiplayer_authority(node_id)
    }

    /// Godot `MultiplayerSynchronizer.set_visibility_for(peer, visible)`
    /// (pat-s3rrv.17).
    ///
    /// ⚠️ Peer `0` is NOT a peer id. Godot documents it as meaning "update
    /// `public_visibility` instead" (doc_classes/MultiplayerSynchronizer.xml).
    /// Treating it as an ordinary id would store an override for a peer that can
    /// never connect, leaving the real default untouched — the call would look
    /// like it worked and change nothing.
    ///
    /// Returns `false` when `node_id` is not a registered synchronizer, so
    /// calling it on the wrong node is a visible no-op rather than a silent one.
    pub fn synchronizer_set_visibility_for(
        &mut self,
        node_id: NodeId,
        peer: i64,
        visible: bool,
    ) -> bool {
        let Some(sync) = self.replication.synchronizers.get_mut(&node_id) else {
            return false;
        };
        if peer == 0 {
            sync.set_visibility_public(visible);
            return true;
        }
        // ⚠️ `PeerId` wraps a **u32**. `peer as u32` on a negative id would wrap
        // it to a huge number and store an override that no real peer can ever
        // match — a silent no-op that looks like a successful call. Reject it.
        let Ok(peer_id) = u32::try_from(peer) else {
            return false;
        };
        sync.set_visibility_for(gdplatform::network::PeerId(peer_id), visible);
        true
    }

    /// Godot `MultiplayerSynchronizer.add_visibility_filter(callable)`
    /// (pat-s3rrv.17).
    ///
    /// Returns `false` for a node that is not a registered synchronizer, or for
    /// a non-`Callable` argument — storing a non-callable would defer the
    /// failure to the outbound pass, where it would look like a filter that
    /// simply never fires.
    pub fn synchronizer_add_visibility_filter(&mut self, node_id: NodeId, filter: Variant) -> bool {
        if !self.replication.synchronizers.contains_key(&node_id) {
            return false;
        }
        if !matches!(filter, Variant::Callable(_)) {
            return false;
        }
        let filters = self
            .replication
            .visibility_filters
            .entry(node_id)
            .or_default();
        // ⚠️ Deduped, because Godot stores these in a `HashSet<Callable>`
        // (`multiplayer_synchronizer.h:59`) and `add_visibility_filter` is an
        // `insert` (`:211`). Pushing unconditionally into a `Vec` makes a filter
        // added twice RUN twice on every outbound pass — which is invisible for
        // a pure predicate and wrong the moment one has a side effect or is
        // expensive, and it also makes a single `remove` leave a copy behind.
        if !filters.contains(&filter) {
            filters.push(filter);
        }
        true
    }

    /// Godot `MultiplayerSynchronizer.remove_visibility_filter(callable)`
    /// (pat-s3rrv.17).
    ///
    /// Returns `true` when a filter was removed. Godot's `erase` on a
    /// `HashSet` is silent about a filter that was never added, so a `false`
    /// here means "nothing matched", not "the call was rejected" — the two
    /// rejection cases (unknown node, non-Callable) also return `false`, which
    /// mirrors `synchronizer_add_visibility_filter`.
    ///
    /// ⚠️ Equality is `CallableRef`'s own impl (`gdvariant/src/variant.rs:127`),
    /// which compares a `Method` by `target_id` + `method` name — so the
    /// `Callable(self, "_deny")` a script writes here matches the one it passed
    /// to `add_visibility_filter`, even though the two are separate values.
    ///
    /// ⚠️ **A lambda filter used to be unremovable, and now is not**
    /// (pat-s3rrv.67). `CallableRef`'s `PartialEq` ended `_ => false` with the
    /// note "lambdas are never equal", so a script doing
    /// `var f = func(peer): return false` / `add_visibility_filter(f)` /
    /// `remove_visibility_filter(f)` silently kept filtering forever, because
    /// `retain` compares by equality and nothing ever matched. Lambdas now
    /// compare by the identity of their body `Arc`, which is exactly Godot's
    /// rule — *"Lambda callables are only compared by reference"*
    /// (`gdscript_lambda_callable.cpp:37-40`). The script must hand back the
    /// SAME Callable it added, as it must in Godot; a freshly written
    /// `func(peer): return false` is a different lambda and removes nothing.
    pub fn synchronizer_remove_visibility_filter(
        &mut self,
        node_id: NodeId,
        filter: Variant,
    ) -> bool {
        if !self.replication.synchronizers.contains_key(&node_id) {
            return false;
        }
        if !matches!(filter, Variant::Callable(_)) {
            return false;
        }
        let Some(filters) = self.replication.visibility_filters.get_mut(&node_id) else {
            return false;
        };
        let before = filters.len();
        filters.retain(|f| f != &filter);
        let removed = filters.len() != before;
        // Drop the entry entirely once empty: `process_replication` gates the
        // whole visibility pass on `visibility_filters.is_empty()` (:1376), so
        // an empty-but-present Vec would keep a per-tick walk alive for a
        // synchronizer that no longer filters anything.
        if filters.is_empty() {
            self.replication.visibility_filters.remove(&node_id);
        }
        removed
    }

    /// Godot `MultiplayerSynchronizer.is_visible_to(peer)` (pat-s3rrv.17).
    ///
    /// A node that is not a registered synchronizer answers `true`: Godot's
    /// default is public visibility, and `false` would read as "deliberately
    /// hidden" for a node that simply is not a synchronizer at all.
    pub fn synchronizer_is_visible_to(&self, node_id: NodeId, peer: i64) -> bool {
        if !self.replication.synchronizers.contains_key(&node_id) {
            return true;
        }
        let Ok(peer_id) = u32::try_from(peer) else {
            // A malformed id can be neither filtered nor overridden; answer with
            // whatever the public default currently is.
            return self
                .replication
                .synchronizers
                .get(&node_id)
                .map(|s| s.is_visibility_public())
                .unwrap_or(true);
        };
        // ⚠️ Godot checks the FILTERS first and returns on the first veto,
        // before consulting `peer_visibility` at all
        // (multiplayer_synchronizer.cpp:193-207). Reading only the stored state
        // here would report a peer as visible that the outbound pass then
        // refuses to send to — two answers to the same question.
        //
        // The verdict is READ FROM CACHE rather than recomputed: see
        // `filter_verdicts`. Evaluating here would re-enter a script that is
        // already checked out of the arena and answer `None` for every filter.
        // An absent entry means "not evaluated yet", which is not a veto.
        if !self
            .replication
            .filter_verdicts
            .get(&(node_id, peer_id))
            .copied()
            .unwrap_or(true)
        {
            return false;
        }
        self.replication
            .synchronizers
            .get(&node_id)
            .map(|s| s.is_visible_to(gdplatform::network::PeerId(peer_id)))
            .unwrap_or(true)
    }

    /// Runs every `add_visibility_filter` Callable on `sync_id`, returning
    /// whether they all permit `peer_id` (pat-s3rrv.17).
    ///
    /// Mirrors `MultiplayerSynchronizer::is_visible_to`: the filters are ANDed
    /// and the first `false` wins.
    ///
    /// ⚠️ Anything that is not a genuine `true` HIDES the peer. Godot's
    /// `ERR_FAIL_COND_V(err != CALL_OK || ret.get_type() != BOOL, false)` fails
    /// CLOSED, so a filter that errors, names a missing method, or returns a
    /// non-bool must not be read as permission.
    fn visibility_filters_allow(&mut self, sync_id: NodeId, peer_id: u32) -> bool {
        let Some(filters) = self.replication.visibility_filters.get(&sync_id) else {
            return true;
        };
        if filters.is_empty() {
            return true;
        }
        // Resolve the targets first: invoking a script needs `&mut self`, which
        // the borrow on `visibility_filters` above rules out.
        let mut calls: Vec<(NodeId, String)> = Vec::new();
        for filter in filters {
            let Variant::Callable(callable) = filter else {
                continue;
            };
            let gdvariant::CallableRef::Method { target_id, method } = callable.inner_callable()
            else {
                continue;
            };
            // `target_id` is documented "0 for self", and self is the
            // synchronizer the filter was registered on. Converting 0 straight
            // to a NodeId yields a node that does not exist, so the filter would
            // be stored, evaluated and silently never run.
            let target = if *target_id == 0 {
                sync_id
            } else {
                NodeId::from_object_id(gdcore::id::ObjectId::from_raw(*target_id))
            };
            calls.push((target, method.clone()));
        }
        for (target, method) in calls {
            let ret = self.call_script_method(target, &method, &[Variant::Int(i64::from(peer_id))]);
            if !matches!(ret, Some(Variant::Bool(true))) {
                return false;
            }
        }
        true
    }

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

    /// IDs sorted by **physics**-process priority (pat-0q2kx.221).
    ///
    /// ⚠️ Godot orders `_physics_process` by `process_physics_priority`, a
    /// property independent of `process_priority`. Physics dispatch used the
    /// idle-process ordering here, so the two could never differ — a node could
    /// not run late in `_process` and early in `_physics_process`, which is the
    /// entire point of Godot having two properties.
    ///
    /// Stable, like its idle twin, so equal priorities keep tree order.
    pub fn all_nodes_in_physics_process_order(&self) -> Vec<NodeId> {
        let mut ids = self.all_nodes_in_tree_order();
        ids.sort_by_key(|id| {
            self.nodes
                .get(id)
                .map_or(0, |n| n.process_physics_priority())
        });
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

    /// Attaches a `VideoStreamPlayer` to `node_id` (pat-0q2kx.182).
    pub fn add_video_player(
        &mut self,
        node_id: NodeId,
        player: crate::video_stream::VideoStreamPlayer,
    ) {
        self.video_players.insert(node_id, player);
    }

    /// Returns a node's video player, if attached.
    pub fn get_video_player(
        &self,
        node_id: NodeId,
    ) -> Option<&crate::video_stream::VideoStreamPlayer> {
        self.video_players.get(&node_id)
    }

    /// Returns a mutable reference to a node's video player, if attached.
    pub fn get_video_player_mut(
        &mut self,
        node_id: NodeId,
    ) -> Option<&mut crate::video_stream::VideoStreamPlayer> {
        self.video_players.get_mut(&node_id)
    }

    /// Advances every playing `VideoStreamPlayer` by `delta`, emitting Godot's
    /// `finished` signal for each stream that ends this tick.
    ///
    /// `VideoStreamPlayer::advance` already clears `playing` at the end of a
    /// non-looping stream, so "finished" is detected by the playing flag going
    /// false across the call rather than by re-deriving the end condition here —
    /// which keeps the looping rule in one place.
    ///
    /// A paused player is skipped by `advance` itself, so a paused video does
    /// not creep forward.
    pub fn process_video_players(&mut self, delta: f64) {
        // Collect first: emitting a signal takes `&mut self`, so the map cannot
        // stay borrowed across the emit.
        let mut finished: Vec<NodeId> = Vec::new();
        for (node_id, player) in self.video_players.iter_mut() {
            if !player.is_playing() {
                continue;
            }
            player.advance(delta);
            if !player.is_playing() {
                finished.push(*node_id);
            }
        }
        for node_id in finished {
            self.emit_signal(node_id, "finished", &[]);
        }
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
            // Gathered inside the player borrow below, but kept out of the
            // returned tuple: widening that tuple would re-indent the whole
            // match arm, and this file carries other agents' uncommitted work.
            let mut clip_starts: Vec<(String, String)> = Vec::new();
            let (values, skip, method_calls, audio_keys, root_motion, root_node, signals, current) =
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
                        // Animation-track keys crossed this frame; started on
                        // the nested players below (pat-s3rrv.44).
                        clip_starts = player.animation_clip_starts_in_range(prev, curr);
                        // Audio keys crossed this frame; applied below like
                        // method tracks (pat-0q2kx.157).
                        let audio_keys = audio_keys_in_range(player, prev, curr);
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
                            audio_keys,
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
                    // `path` may address a COMPONENT of a compound property
                    // (`position:x`) — Godot's bézier tracks always do, and a
                    // value track may. A flat `set_property` would create a
                    // literal `"position:x"` entry that nothing reads
                    // (pat-s3rrv.44).
                    set_animated_property_path(node, &path, value);
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

            // Start each crossed animation-track clip on the nested
            // `AnimationPlayer` the track names, resolved relative to
            // `base_node` exactly like property and method tracks
            // (pat-s3rrv.44). This is how a "cutscene" clip drives a character's
            // own player rather than duplicating its tracks.
            for (track_node_path, clip) in clip_starts {
                let target = if track_node_path.is_empty() || track_node_path == "." {
                    base_node
                } else {
                    match self.get_node_or_null(base_node, &track_node_path) {
                        Some(t) => t,
                        None => continue,
                    }
                };
                // Self-targeting would recurse into the player currently being
                // advanced, restarting it every frame it crosses its own key.
                if target == node_id {
                    continue;
                }
                if let Some(sub) = self.animation_players.get_mut(&target) {
                    // Godot's reserved `[stop]` clip name halts the nested
                    // player instead of naming a clip to play.
                    if clip == "[stop]" {
                        sub.stop();
                    } else if sub.has_animation(&clip) {
                        sub.play(&clip);
                    }
                }
            }

            // Fire audio-track keys at the `AudioStreamPlayer` named by the
            // track, resolved relative to `base_node` exactly like method and
            // property tracks (pat-0q2kx.157). Point it at the keyed stream,
            // seek to the key's `start_offset`, and start it. `MainLoop::step`
            // runs the animation phase before the audio phase, so the voice is
            // audible on the very frame the key is crossed.
            for (track_node_path, stream, start_offset) in audio_keys {
                let target = if track_node_path.is_empty() || track_node_path == "." {
                    base_node
                } else {
                    match self.get_node_or_null(base_node, &track_node_path) {
                        Some(t) => t,
                        None => continue,
                    }
                };
                if let Some(node) = self.nodes.get_mut(&target) {
                    node.set_property("stream", Variant::String(stream));
                    // The bridge prefers a non-empty `__extres__stream`
                    // companion over `stream`, so a scene-authored stream would
                    // otherwise win over the key. Clear it so the keyed stream
                    // is what plays.
                    node.set_property("__extres__stream", Variant::Nil);
                    if start_offset > 0.0 {
                        node.set_property("seek_request", Variant::Float(start_offset));
                    }
                    node.set_property("playing", Variant::Bool(true));
                }
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

    /// Advances every `AnimatedSprite2D`/`AnimatedSprite3D` frame by `delta` seconds.
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
            // ⚠️ AnimatedSprite3D drives from the SAME state, so it shares
            // this loop rather than getting a parallel one. Godot gives it the
            // identical `animation` / `autoplay` / `frame` / `frame_progress` /
            // `speed_scale` / `sprite_frames` members
            // (`doc/classes/AnimatedSprite3D.xml`) and an `_process_animation`
            // mirroring the 2D one. The `texture` published below then reaches
            // the 3D viewport by exactly the route it already reaches the
            // canvas — which is the same argument the comment there makes
            // against a second parallel path (pat-s3rrv.92).
            if !matches!(node.class_name(), "AnimatedSprite2D" | "AnimatedSprite3D") {
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

            let previous_frame = playback.frame;
            // ⚠️ The previously SELECTED clip comes from a companion the last
            // tick published, not from `playback`. `playback` is rebuilt from
            // the node's `animation` property at the top of every tick, and
            // `advance` never changes it — so any before/after comparison around
            // the call is always equal and `animation_changed` could never fire.
            // The switch happens BETWEEN ticks, so that is where it must be seen.
            let previous_animation = match node.get_property("__sf_last_animation") {
                Variant::String(s) => s,
                _ => String::new(),
            };
            let animation_switched =
                !previous_animation.is_empty() && previous_animation != playback.animation;
            let outcome = playback.advance(delta as f32, &frames);
            let finished = outcome.finished;
            let frame_changed = playback.frame != previous_frame;
            // The texture the current frame shows. Publishing it as the node's
            // own `texture` is what makes an AnimatedSprite2D actually draw
            // (pat-0q2kx.100): the 2D render path — the viewport's
            // `render_kind_for` — resolves
            // a sprite's pixels from that property, so the animation reaches the
            // canvas through the same route a plain Sprite2D does rather than a
            // second, parallel one.
            let frame_texture = frames
                .frame_texture(&playback.animation, playback.frame)
                .map(str::to_string);

            if let Some(node) = self.nodes.get_mut(&node_id) {
                if let Some(a) = autostart {
                    node.set_property("animation", Variant::String(a));
                    node.set_property("__sf_autoplayed", Variant::Bool(true));
                }
                node.set_property("frame", Variant::Int(playback.frame as i64));
                node.set_property("__sf_time", Variant::Float(playback.time as f64));
                // Remembered for the next tick's `animation_changed` comparison.
                node.set_property(
                    "__sf_last_animation",
                    Variant::String(playback.animation.clone()),
                );
                // Godot exposes the sub-frame position as `frame_progress`, a
                // 0..1 fraction of the current frame rather than raw seconds.
                let progress = match frames.animation(&playback.animation) {
                    Some(a) => {
                        let hold = a.frame_seconds(playback.frame);
                        if hold > 0.0 {
                            // ⚠️ A reversed clip's `time` runs in `(-hold, 0]`, so the
                            // raw ratio is NEGATIVE and the clamp alone would pin
                            // `frame_progress` at 0.0 for the whole of backwards
                            // playback — the feature this property exists to
                            // describe. Godot keeps it a 0..1 fraction in both
                            // directions and starts a `from_end` play at 1.0
                            // (`animated_sprite_2d.cpp:520-522`), which is exactly
                            // what `1.0 + ratio` gives at `time == 0`.
                            let ratio = playback.time / hold;
                            if playback.speed_scale < 0.0 {
                                (1.0 + ratio).clamp(0.0, 1.0)
                            } else {
                                ratio.clamp(0.0, 1.0)
                            }
                        } else {
                            0.0
                        }
                    }
                    None => 0.0,
                };
                node.set_property("frame_progress", Variant::Float(progress as f64));
                if let Some(texture) = frame_texture {
                    node.set_property("texture", Variant::String(texture));
                }
                // A non-looping animation stops on its last frame; Godot leaves
                // `playing` false so a later `play()` is what restarts it.
                node.set_property("playing", Variant::Bool(playback.playing));
            }
            if frame_changed {
                self.emit_signal(node_id, "frame_changed", &[]);
            }
            // A looping clip wrapped: Godot emits this per wrap, and it is
            // mutually exclusive with `animation_finished` — a clip that loops
            // never finishes (`animated_sprite_2d.cpp:45-46`, `:69-70`).
            if outcome.looped {
                self.emit_signal(node_id, "animation_looped", &[]);
            }
            // The selected clip changed since the last tick. Compared against
            // the value the NODE carried, so a script assigning `animation`
            // between frames is what the comparison sees.
            if animation_switched {
                self.emit_signal(node_id, "animation_changed", &[]);
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

    /// A node's graph model, creating an empty one on first use
    /// (pat-0q2kx.244).
    ///
    /// Lazy rather than created at `add_child` time: a GraphEdit with no
    /// connections is indistinguishable from an absent one, so allocating for
    /// every node that merely *might* be a GraphEdit would cost memory for
    /// nothing. The first `connect_node` is what makes it real.
    pub fn graph_edit_mut(&mut self, node_id: NodeId) -> &mut crate::graph_edit::GraphEdit {
        self.graph_edits.entry(node_id).or_default()
    }

    /// A node's graph model if one has been created; `None` before the first
    /// mutation. Callers that only read must not force one into existence.
    pub fn get_graph_edit(&self, node_id: NodeId) -> Option<&crate::graph_edit::GraphEdit> {
        self.graph_edits.get(&node_id)
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
        self.process_animation_trees_in_mode(delta, AnimationCallbackMode::Idle);
    }

    /// Advances every `AnimationTree` whose owner node's `callback_mode_process`
    /// selects `mode` (pat-s3rrv.23).
    ///
    /// `AnimationTree` extends `AnimationMixer`, which is where
    /// `callback_mode_process` lives — a tree honours it exactly as an
    /// `AnimationPlayer` does (`scene/animation/animation_mixer.cpp`). Patina
    /// filtered players by mode but ran trees unconditionally in the idle
    /// phase, so a tree set to `PHYSICS` advanced on the wrong clock: its pose
    /// was written once per rendered frame instead of once per physics tick, and
    /// anything reading a bone or property it drives saw values that drifted
    /// with the frame rate rather than tracking the simulation.
    ///
    /// Kept as a separate entry point rather than a flag, mirroring
    /// [`process_animations_in_mode`](Self::process_animations_in_mode), so both
    /// families are driven the same way from `MainLoop`.
    pub fn process_animation_trees_in_mode(&mut self, delta: f64, mode: AnimationCallbackMode) {
        // Snapshot the ids so nodes can be mutated while iterating.
        let node_ids: Vec<NodeId> = self
            .animation_trees
            .keys()
            .copied()
            .filter(|id| self.animation_callback_mode(*id) == mode)
            .collect();
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
                    // `path` may address a COMPONENT of a compound property
                    // (`position:x`) — Godot's bézier tracks always do, and a
                    // value track may. A flat `set_property` would create a
                    // literal `"position:x"` entry that nothing reads
                    // (pat-s3rrv.44).
                    set_animated_property_path(node, &path, value);
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
        self.process_tweens_in_mode(delta, delta, TweenProcessMode::Idle);
    }

    /// The node that created a tween — the default `bind_node` target and the
    /// node a step with no explicit `target` writes to.
    pub fn tween_owner(&self, tween_id: TweenId) -> Option<NodeId> {
        self.tweens.get(&tween_id).map(|(owner, _)| *owner)
    }

    /// Removes a tween from the tree and hands it back, so it can be nested
    /// inside another tween as a `tween_subtween` step (pat-s3rrv.46). The id
    /// is dead afterwards, exactly as in Godot, where the parent takes
    /// ownership of the subtween.
    pub fn take_tween(&mut self, tween_id: TweenId) -> Option<Tween> {
        self.tweens.remove(&tween_id).map(|(_, t)| t)
    }

    /// The mode-aware half of [`process_tweens`](Self::process_tweens): advances
    /// only the tweens whose [`TweenProcessMode`] matches this phase and whose
    /// [`Tween::can_process`] says they survive the tree's current pause state
    /// (pat-s3rrv.46).
    ///
    /// Before this, `can_process` had no production caller at all and the whole
    /// phase sat inside `MainLoop`'s blanket `if !paused` guard, so a
    /// `TWEEN_PAUSE_PROCESS` pause-menu tween froze with the game and a
    /// `TWEEN_PROCESS_PHYSICS` tween ran at render rate.
    ///
    /// `unscaled_delta` is the same frame's delta without `Engine.time_scale`
    /// applied; a tween with `ignore_time_scale` set advances on that one.
    pub fn process_tweens_in_mode(
        &mut self,
        delta: f64,
        unscaled_delta: f64,
        mode: TweenProcessMode,
    ) {
        let mut completed = Vec::new();
        // Collected while the `self.tweens` borrow is live and dispatched after
        // it ends: `call_script_with_access` / `emit_signal` take `&mut self`,
        // and a script re-entered here may itself create or kill tweens.
        let mut method_calls: Vec<(NodeId, String, Variant)> = Vec::new();
        let mut callbacks: Vec<(NodeId, String)> = Vec::new();
        let mut finished: Vec<(TweenId, NodeId)> = Vec::new();
        // `step_finished(idx)` / `loop_finished(count)`: declared in ClassDB
        // since the Tween family was registered and never emitted until
        // pat-s3rrv.46.
        let mut step_finished: Vec<(TweenId, i64)> = Vec::new();
        let mut loop_finished: Vec<(TweenId, i64)> = Vec::new();

        let paused = self.paused;
        let tween_ids: Vec<TweenId> = self.tweens.keys().copied().collect();
        for tween_id in tween_ids {
            // Phase + pause gate, resolved before the mutable borrow because
            // `should_process_node` needs `&self` too. Godot's `bind_node`
            // defaults to the node that created the tween, which is exactly the
            // owner recorded here, so `Bound` always has a node to follow.
            let (in_phase, allowed, bound_gone) = match self.tweens.get(&tween_id) {
                Some((owner, tween)) => {
                    let bound = tween
                        .bound_node
                        .map(|raw| NodeId::from_object_id(gdcore::id::ObjectId::from_raw(raw)))
                        .unwrap_or(*owner);
                    // Only an EXPLICIT `bind_node` target counts as "gone": the
                    // owner is absent in plenty of Rust-side fixtures that add a
                    // tween without a node, and killing those would change
                    // long-standing behaviour.
                    let gone = tween.bound_node.is_some() && self.nodes.get(&bound).is_none();
                    (
                        tween.process_mode == mode,
                        tween.can_process(paused, Some(self.should_process_node(bound))),
                        gone,
                    )
                }
                None => continue,
            };
            // Godot's `bind_node` kills the tween with its node.
            if bound_gone {
                completed.push(tween_id);
                continue;
            }
            if !in_phase || !allowed {
                continue;
            }
            if let Some((node_id, tween)) = self.tweens.get_mut(&tween_id) {
                let owner = *node_id;
                let step = if tween.ignore_time_scale {
                    unscaled_delta
                } else {
                    delta
                };
                let done = tween.advance(step);

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
                for idx in tween.take_step_finished() {
                    step_finished.push((tween_id, idx));
                }
                for count in tween.take_loop_finished() {
                    loop_finished.push((tween_id, count));
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
        for (tween_id, idx) in step_finished {
            let tween_as_source =
                NodeId::from_object_id(gdcore::id::ObjectId::from_raw(tween_id.raw()));
            self.emit_signal(tween_as_source, "step_finished", &[Variant::Int(idx)]);
        }
        for (tween_id, count) in loop_finished {
            let tween_as_source =
                NodeId::from_object_id(gdcore::id::ObjectId::from_raw(tween_id.raw()));
            self.emit_signal(tween_as_source, "loop_finished", &[Variant::Int(count)]);
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
        // A CanvasItem that overrides `_draw` is drawn once without anyone
        // calling `queue_redraw()`: Godot queues the first redraw when the item
        // enters the canvas (canvas_item.cpp NOTIFICATION_ENTER_CANVAS). Without
        // this, a script whose `_draw` is its whole visual output would render
        // nothing until something happened to invalidate it (pat-0q2kx.82).
        if script.has_method("_draw") {
            self.redraw_queued.insert(node_id);
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
    /// Sets a node property and emits whatever change signal Godot emits for
    /// it (pat-s3rrv.167).
    ///
    /// Writing straight to `Node::set_property` stores the value and tells
    /// nobody; a `NinePatchRect` swapping its texture has to notify whatever is
    /// drawing it. Use this whenever a property is assigned on a node that is
    /// already in the tree.
    ///
    /// ⚠️ **No-op assignments emit nothing.** Godot fires the signal on a
    /// CHANGE, so assigning the same texture twice is one emission, not two —
    /// that is the "exactly once" a listener counts on, and re-emitting would
    /// make any redraw-on-change handler loop on itself.
    ///
    /// Whether a signal exists is asked of ClassDB rather than a hardcoded list
    /// of classes, so a class gaining `texture_changed` gets the behaviour
    /// without a second place needing to learn about it.
    pub fn set_node_property_notifying(
        &mut self,
        node_id: NodeId,
        key: &str,
        value: Variant,
    ) -> bool {
        let Some(node) = self.nodes.get(&node_id) else {
            return false;
        };
        if node.get_property(key) == value {
            return false;
        }
        let class = node.class_name().to_string();
        if let Some(node) = self.nodes.get_mut(&node_id) {
            node.set_property(key, value);
        }
        // `texture` is the only property with a dedicated change signal in
        // Godot's Texture2D-holding controls; others notify through `changed`
        // on the resource itself, which this engine does not model yet.
        if key == "texture" && gdobject::class_db::class_has_signal(&class, "texture_changed") {
            self.emit_signal(node_id, "texture_changed", &[]);
        }
        true
    }

    /// Runs every scripted `StyleBox` in the tree and caches what it drew
    /// (pat-s3rrv.160).
    ///
    /// # Why a pre-pass
    ///
    /// Running a script needs `&mut self`, and the control render path takes
    /// the tree immutably by design. This mirrors what already happens for a
    /// node's own `_draw`: [`Self::flush_pending_redraws`] runs first and the
    /// renderer then reads [`Self::custom_draw_commands`]. Threading mutability
    /// through `draw_control` instead would push `&mut` into every caller of
    /// `control_draw_items`, including the editor viewport.
    ///
    /// The script path reaches here as the `__resscript__<prop>` companion the
    /// loader records, because render time holds the tree but not the
    /// `PackedScene` the sub-resource bodies live in.
    pub fn flush_scripted_resources(&mut self, rects: &[(NodeId, gdcore::math::Rect2)]) {
        for (node_id, rect) in rects {
            let Some(node) = self.nodes.get(node_id) else {
                continue;
            };
            // Two kinds of scripted resource draw through this path, and the
            // slot names vary by control (`panel`, `normal`, `pressed`, …), so
            // this keys off the companion rather than fixed property names.
            //
            // ⚠️ A StyleBox and a Texture2D draw through DIFFERENT virtuals —
            // `_draw(canvas_item, rect)` against `_draw_rect(canvas_item, rect,
            // tile, modulate, transpose)` — so the method and its arity are
            // chosen per slot. Calling one with the other's signature does not
            // fail loudly; the interpreter simply finds no such method and the
            // resource silently draws nothing.
            let scripts: Vec<(String, bool)> = node
                .properties()
                .filter_map(|(key, value)| {
                    let prop = key.strip_prefix("__resscript__")?;
                    let is_stylebox = prop.starts_with("theme_override_styles/");
                    if !is_stylebox && prop != "texture" {
                        return None;
                    }
                    match value {
                        Variant::String(path) => Some((path.clone(), is_stylebox)),
                        _ => None,
                    }
                })
                .collect();

            let mut drawn = Vec::new();
            for (path, is_stylebox) in scripts {
                let Ok(source) = gdcore::file_access::FileAccess::read_file_string(&path) else {
                    continue;
                };
                let Ok(script) =
                    crate::scripting::GDScriptNodeInstance::from_source(&source, *node_id)
                else {
                    continue;
                };
                // Godot passes the canvas item as an RID, which has no
                // equivalent a script can use here, so it arrives as Nil; the
                // rect is what these virtuals actually need, and the draw calls
                // route through the same accessor either way.
                if is_stylebox {
                    drawn.extend(self.draw_with_external_script(
                        *node_id,
                        Box::new(script),
                        "_draw",
                        &[Variant::Nil, Variant::Rect2(*rect)],
                    ));
                } else {
                    // A texture reports its OWN size through `_get_width` /
                    // `_get_height`, and that is the rect it draws into — not
                    // the control's. That is the whole point of a procedural
                    // texture: a 64x32 texture in a larger rect still draws
                    // 64x32. Falling back to the control's rect when the script
                    // does not override them keeps a partial script usable.
                    let mut script = script;
                    // `_get_width`/`_get_height` are declared to return int,
                    // but a script may compute one and hand back a float; both
                    // are accepted rather than silently falling back to the
                    // control's size, which would look like the override never
                    // ran.
                    let dimension = |v: Option<Variant>| match v {
                        Some(Variant::Int(i)) => Some(i as f32),
                        Some(Variant::Float(f)) => Some(f as f32),
                        _ => None,
                    };
                    let w = dimension(script.call_method("_get_width", &[]).ok())
                        .unwrap_or(rect.size.x);
                    let h = dimension(script.call_method("_get_height", &[]).ok())
                        .unwrap_or(rect.size.y);
                    let sized =
                        gdcore::math::Rect2::new(rect.position, gdcore::math::Vector2::new(w, h));
                    drawn.extend(self.draw_with_external_script(
                        *node_id,
                        Box::new(script),
                        "_draw_rect",
                        &[
                            Variant::Nil,
                            Variant::Rect2(sized),
                            Variant::Bool(false),
                            Variant::Color(gdcore::math::Color::WHITE),
                            Variant::Bool(false),
                        ],
                    ));
                }
            }
            if drawn.is_empty() {
                self.scripted_resource_commands.remove(node_id);
            } else {
                self.scripted_resource_commands.insert(*node_id, drawn);
            }
        }
    }

    /// What `node_id`'s scripted StyleBox drew on the last pre-pass.
    pub fn scripted_resource_commands(
        &self,
        node_id: NodeId,
    ) -> &[gdserver2d::canvas::DrawCommand] {
        self.scripted_resource_commands
            .get(&node_id)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    /// Runs an external script's drawing method and returns what it drew
    /// (pat-s3rrv.160).
    ///
    /// A scripted `StyleBox._draw` paints through the same `draw_rect` /
    /// `draw_circle` API a node's `_draw` uses, so the commands land in the
    /// tree's capture buffer rather than being returned. This brackets the call
    /// exactly as [`Self::flush_pending_redraws`] does — including saving and
    /// restoring any outer capture, so a nested call cannot clobber the frame
    /// already being collected — and hands back the commands instead of
    /// recording them against the node, because a stylebox's output belongs to
    /// the control that asked for it, not to the node's own `_draw`.
    pub fn draw_with_external_script(
        &mut self,
        context_node: NodeId,
        script: Box<dyn ScriptInstance>,
        method: &str,
        args: &[Variant],
    ) -> Vec<gdserver2d::canvas::DrawCommand> {
        let outer = self.draw_capture.take();
        self.draw_capture = Some(Vec::new());
        self.call_external_script_method(context_node, script, method, args);
        let drawn = self.draw_capture.take().unwrap_or_default();
        self.draw_capture = outer;
        drawn
    }

    /// Calls `method` on a script instance the tree does not own, in the
    /// context of `context_node` (pat-s3rrv.160).
    ///
    /// This is how a scripted **resource** runs engine-facing code: Godot's
    /// `StyleBox._draw(canvas_item, rect)` draws into the canvas item that owns
    /// it, so the stylebox's script has to run as though it were that node —
    /// its `draw_circle`/`draw_rect` calls must reach the same accessor and
    /// land in the same per-node capture a `_draw` override would.
    ///
    /// Implemented by installing the instance under `context_node` for the
    /// duration and restoring whatever was there, rather than duplicating
    /// [`Self::call_script_method`]'s accessor setup and command capture — a
    /// second copy of that would drift.
    ///
    /// ⚠️ The node keeps its own script afterwards, but the call's variable
    /// sync-back still writes the RESOURCE's vars onto the node's properties.
    /// That is visible to anything reading them; a stylebox's fields are not a
    /// control's fields. Callers that care should read the draw commands, which
    /// is what this exists for, and not the node's properties.
    pub fn call_external_script_method(
        &mut self,
        context_node: NodeId,
        script: Box<dyn ScriptInstance>,
        method: &str,
        args: &[Variant],
    ) -> Option<Variant> {
        let displaced = self.scripts.insert(context_node, script);
        let result = self.call_script_method(context_node, method, args);
        // Restore precisely: a node that had NO script must not end up owning
        // the resource's one, which would make the resource's methods answer
        // for the node from here on.
        match displaced {
            Some(original) => {
                self.scripts.insert(context_node, original);
            }
            None => {
                self.scripts.remove(&context_node);
            }
        }
        result
    }

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

    // -- custom drawing: `queue_redraw` / `_draw` (pat-0q2kx.82) ------------

    /// Marks `node_id` as needing its `_draw` re-run, Godot's
    /// `CanvasItem::queue_redraw` (upstream/godot/scene/main/canvas_item.cpp).
    ///
    /// Queuing is idempotent within a frame and never runs `_draw` inline:
    /// Godot defers the redraw to the next draw pass so a script may call
    /// `queue_redraw()` from `_ready`, from `_process`, or from inside `_draw`
    /// itself without recursing.
    pub fn queue_redraw(&mut self, node_id: NodeId) {
        self.redraw_queued.insert(node_id);
    }

    /// Whether `node_id` has a redraw pending (Godot
    /// `CanvasItem.is_queued_for_redraw`).
    pub fn is_queued_for_redraw(&self, node_id: NodeId) -> bool {
        self.redraw_queued.contains(&node_id)
    }

    /// Runs `_draw` on every node with a pending redraw and stores the commands
    /// it emitted — the NOTIFICATION_DRAW half of Godot's redraw cycle.
    ///
    /// Call this before building a frame; the canvas render list folds the
    /// stored commands into each node's render items. A node whose script has no
    /// `_draw` still clears its pending flag (and its old commands), matching
    /// Godot where a redraw with no `_draw` override empties the item.
    pub fn flush_pending_redraws(&mut self) {
        if self.redraw_queued.is_empty() {
            return;
        }
        // Drain rather than iterate: `_draw` may call `queue_redraw()` again
        // (Godot's animation idiom), and that must land on the *next* pass, not
        // spin here forever.
        let pending: Vec<NodeId> = self.redraw_queued.drain().collect();
        for node_id in pending {
            if !self.nodes.contains_key(&node_id) {
                self.custom_draw_commands.remove(&node_id);
                continue;
            }
            // A nested flush would clobber the outer capture; `_draw` bodies do
            // not re-enter this, but a script reaching into the tree could.
            let outer = self.draw_capture.take();
            self.draw_capture = Some(Vec::new());
            self.call_script_with_access(node_id, "_draw", &[]);
            let commands = self.draw_capture.take().unwrap_or_default();
            self.draw_capture = outer;
            if commands.is_empty() {
                self.custom_draw_commands.remove(&node_id);
            } else {
                self.custom_draw_commands.insert(node_id, commands);
            }
        }
    }

    /// The draw commands `node_id`'s `_draw` produced on its last redraw.
    pub fn custom_draw_commands(&self, node_id: NodeId) -> &[gdserver2d::canvas::DrawCommand] {
        self.custom_draw_commands
            .get(&node_id)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    /// Appends a command from a `draw_*` builtin. Outside a `_draw` body there
    /// is no capture buffer and the command is dropped, as Godot drops draw
    /// calls made outside `_draw`.
    pub(crate) fn push_draw_command(&mut self, command: gdserver2d::canvas::DrawCommand) {
        if let Some(buffer) = self.draw_capture.as_mut() {
            buffer.push(command);
        }
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
        use gdplatform::input::InputEvent as Ev;
        // `double_click` is computed HERE rather than carried on the event:
        // `InputEvent::MouseButton` has 71 construction sites across six crates,
        // so adding a field to it is a far larger change than this flag is worth
        // (pat-s3rrv.76). The bookkeeping is the same shape as the touch path's
        // `would_be_double_tap`, reusing its window so the two agree.
        let double_click = match event {
            Ev::MouseButton {
                button,
                pressed: true,
                ..
            } => {
                let idx = button.to_godot_index();
                let now = self.trace_frame;
                let is_double = match self.last_mouse_press_frame.get(&idx) {
                    Some(&last) => now.saturating_sub(last) <= gdplatform::input::DOUBLE_TAP_FRAMES,
                    None => false,
                };
                self.last_mouse_press_frame.insert(idx, now);
                is_double
            }
            _ => false,
        };
        // Mirror it for the picking stage, which runs after this call for the
        // SAME event (`MainLoop::step`) and cannot recompute it safely.
        self.last_double_click = double_click;
        // ⚠️ Updated from KEY events, and read by the mouse and gesture arms.
        // Every key event carries the modifier state the platform saw — the web
        // bridge reads `shiftKey`/`ctrlKey`/`altKey` off the browser event
        // (`gdplatform/src/input.rs:2634`) — and a modifier key press is itself a
        // key event carrying its own flag, so holding Shift and clicking with no
        // other key in between still updates this.
        if let Ev::Key {
            shift, ctrl, alt, ..
        } = event
        {
            self.last_modifiers = (*shift, *ctrl, *alt);
        }
        let payload = input_event_to_variant(event, double_click, self.pen, self.last_modifiers);
        // Each event starts unhandled; `set_input_as_handled()` sets the flag.
        self.input_handled = false;

        self.call_input_stage("_input", &payload);
        if self.input_handled {
            return;
        }

        // Godot's `Viewport::push_input` runs a GUI stage between `_input` and
        // the shortcut/unhandled stages (upstream/godot/scene/main/viewport.cpp
        // `_gui_input_event`). Patina had the hit-test math but no runtime call
        // path, so `_gui_input`, `gui_input`, `mouse_entered`/`mouse_exited` and
        // button presses never fired from a real event (pat-0q2kx.18).
        self.gui_input_stage(event, &payload);
        if self.input_handled {
            return;
        }

        // The remaining stages are gated by event type, not run for everything
        // (pat-0q2kx.91). Godot's `Viewport::_push_unhandled_input_internal`
        // (viewport.cpp:3558-3576) admits only Key/Shortcut/JoypadButton to
        // `_shortcut_input`, and only Key to `_unhandled_key_input` — the latter
        // explicitly so mouse motion, the highest-frequency event there is,
        // never walks the tree for a keyboard handler. Running `_shortcut_input`
        // for every event made a shortcut handler fire on mouse movement.
        if matches!(event, Ev::Key { .. } | Ev::GamepadButton { .. }) {
            self.call_input_stage("_shortcut_input", &payload);
            if self.input_handled {
                return;
            }
            // Built-in BaseButton accelerators run AFTER script
            // `_shortcut_input` handlers, so a script can consume the key
            // first, exactly as Godot orders them (pat-s3rrv.175). This is a
            // no-op tree walk on any scene that declares no Shortcut, since it
            // only reacts to nodes carrying `__shortcut__events`.
            crate::shortcut::dispatch_shortcuts(self, event);
            if self.input_handled {
                return;
            }
        }
        if matches!(event, Ev::Key { .. }) {
            self.call_input_stage("_unhandled_key_input", &payload);
            if self.input_handled {
                return;
            }
        }
        self.call_input_stage("_unhandled_input", &payload);
    }

    /// Runs one input stage over the tree, in Godot's order and with Godot's
    /// filtering (pat-0q2kx.91).
    ///
    /// Mirrors `SceneTree::_call_input_pause`
    /// (upstream/godot/scene/main/scene_tree.cpp:1435-1447):
    /// * **Reverse order.** Godot walks the stage's node group backwards, so the
    ///   node added last — the deepest, most recently entered — sees the event
    ///   first and can consume it before its ancestors. Patina's group analogue
    ///   is tree order, so this reverses that. Forward iteration gave a parent
    ///   first refusal over its own children, the opposite of Godot.
    /// * **`can_process` gating.** A node whose `process_mode` disables it under
    ///   the tree's current pause state is skipped, which is what stops a paused
    ///   scene from reacting to input.
    /// * **Early out.** The handled flag is re-checked before every node, so
    ///   `set_input_as_handled()` stops the walk mid-stage rather than only
    ///   between stages.
    fn call_input_stage(&mut self, stage: &str, payload: &Variant) {
        for node_id in self.all_nodes_in_tree_order().into_iter().rev() {
            if self.input_handled {
                return;
            }
            if !self.should_process_node(node_id) {
                continue;
            }
            // `should_process_node` answers the *pause* question
            // (`process_mode`); this answers whether the node wants input at
            // all. Godot's `_call_input_pause` checks `is_processing_input()`
            // too, so a node that called `set_process_input(false)` receives
            // nothing even while the tree runs normally (pat-0q2kx.138).
            if !self
                .get_node(node_id)
                .is_some_and(|n| n.is_processing_input())
            {
                continue;
            }
            self.call_script_with_access(node_id, stage, std::slice::from_ref(payload));
        }
    }

    /// Rewrites a mouse payload's `position` into `node_id`'s local space,
    /// mirroring Godot's `Viewport::_gui_input_event` localizer
    /// (`viewport.cpp:2078-2088`).
    ///
    /// A script's `_gui_input` must see coordinates relative to its own control:
    /// a control at (100, 50) clicked at viewport (120, 60) reads (20, 10).
    ///
    /// `global_position` is deliberately left in viewport space — that is what
    /// Godot's `InputEventMouse.global_position` means, and localizing both
    /// would make the two properties indistinguishable. A payload carrying
    /// neither (a key event) passes through untouched.
    fn localize_gui_payload(&self, node_id: NodeId, payload: &Variant) -> Variant {
        let origin = crate::control::get_global_position(self, node_id);
        match payload {
            Variant::Resource(res) => {
                let mut out = res.clone();
                if let Some(Variant::Vector2(p)) = out.properties.get("position") {
                    let local = gdcore::math::Vector2::new(p.x - origin.x, p.y - origin.y);
                    out.properties
                        .insert("position".to_string(), Variant::Vector2(local));
                }
                Variant::Resource(out)
            }
            other => other.clone(),
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
    /// - a disabled button consumes the event without emitting anything;
    /// - key events skip the hit test entirely and go to the focus owner
    ///   ([`gui_key_stage`](Self::gui_key_stage), pat-0q2kx.84).
    /// The open popup's geometry and model, for the draw pass (pat-s3rrv.72).
    ///
    /// Returns the SAME model the hit-test measures, so the rows drawn and the
    /// rows clickable cannot drift apart.
    pub(crate) fn open_popup_draw(
        &self,
    ) -> Option<(
        gdcore::math::Vector2,
        f32,
        &crate::popup::PopupMenu,
        Option<usize>,
    )> {
        self.open_popup
            .as_ref()
            .map(|o| (o.origin, o.width, &o.menu, o.hovered))
    }

    /// Emits `meta_hover_started` / `meta_hover_ended` as the pointer moves in
    /// and out of `[url=…]` runs (pat-s3rrv.74).
    ///
    /// ⚠️ `meta_hover_ended` carries the payload of the run being **left**, not
    /// the one being entered (`rich_text_label.cpp:3055-3072`) — which is why
    /// the payload is stored rather than re-derived: by the time the signal
    /// fires, that run is no longer under the cursor.
    ///
    /// Identity is (label, run index) rather than the payload string, mirroring
    /// Godot's comparison of the meta ITEM. Two adjacent `[url=same]` runs are
    /// two links there, and merging them by payload would swallow the crossing
    /// between them.
    fn update_meta_hover(&mut self, hit: Option<NodeId>, position: gdcore::math::Vector2) {
        let now: Option<(NodeId, usize, String)> = hit
            .filter(|&id| {
                self.get_node(id)
                    .map(|n| n.class_name() == "RichTextLabel")
                    .unwrap_or(false)
            })
            .and_then(|id| {
                let rect = gdcore::math::Rect2::new(
                    self.control_global_position(id),
                    crate::control::get_size(self, id),
                );
                crate::control_render::richtext_meta_regions(
                    self,
                    id,
                    crate::control_render::RICHTEXT_DEFAULT_FONT_SIZE,
                    rect,
                )
                .into_iter()
                .enumerate()
                .find(|(_, (r, _))| r.contains_point(position))
                .map(|(idx, (_, meta))| (id, idx, meta))
            });

        // Moving WITHIN one run is not a transition, so it emits nothing —
        // otherwise every motion sample over a link would re-fire the pair.
        let unchanged = match (&self.hovered_meta, &now) {
            (Some((a, i, _)), Some((b, j, _))) => a == b && i == j,
            (None, None) => true,
            _ => false,
        };
        if unchanged {
            return;
        }

        // Ended before started, so a pointer sliding straight from one link to
        // the next reports leaving the first as well as entering the second.
        if let Some((node, _, left)) = self.hovered_meta.take() {
            if self.nodes.contains_key(&node) {
                self.emit_signal(node, "meta_hover_ended", &[Variant::String(left)]);
            }
        }
        if let Some((node, idx, meta)) = now {
            self.emit_signal(node, "meta_hover_started", &[Variant::String(meta.clone())]);
            self.hovered_meta = Some((node, idx, meta));
        }
    }

    /// The `HTTPRequest` state for `node_id`, created on first use.
    ///
    /// Lazy because a `.tscn` can carry any number of HTTPRequest nodes that a
    /// scene never actually uses; spinning up transport state for each at load
    /// time would allocate threads for requests nobody makes.
    pub fn http_request_for(
        &mut self,
        node_id: NodeId,
    ) -> &mut crate::http_request_node::HttpRequestNode {
        self.http_requests.entry(node_id).or_default()
    }

    /// Whether `node_id` already has HTTP state (without creating any).
    pub fn has_http_request(&self, node_id: NodeId) -> bool {
        self.http_requests.contains_key(&node_id)
    }

    /// Polls every in-flight `HTTPRequest` and emits `request_completed` for the
    /// ones that finished (pat-s3rrv.78).
    ///
    /// Godot: `request_completed(result, response_code, headers, body)` — the
    /// headers are a `PackedStringArray` of `"Name: Value"` lines and the body a
    /// `PackedByteArray`, which is what a script's handler destructures.
    ///
    /// ⚠️ The payloads are drained BEFORE any signal is emitted. A handler is
    /// script code that can add or free nodes — including another HTTPRequest —
    /// and mutating `http_requests` while iterating it would be the classic
    /// "emit inside the loop over the collection you are about to change".
    pub fn process_http_requests(&mut self) {
        let ready: Vec<(NodeId, Vec<gdplatform::http_request::RequestCompleted>)> = self
            .http_requests
            .iter()
            .map(|(id, req)| (*id, req.drain_completed()))
            .filter(|(_, done)| !done.is_empty())
            .collect();

        for (node_id, done) in ready {
            if !self.nodes.contains_key(&node_id) {
                // The node was freed while the request was in flight; Godot
                // drops the callback rather than emitting into a dead object.
                continue;
            }
            // Read once per node, before any handler runs: a handler is
            // script code that could set `download_file` mid-drain, and Godot
            // reads the property at completion time.
            let delivery = self
                .nodes
                .get(&node_id)
                .map(crate::http_request_node::delivery_settings)
                .unwrap_or_default();
            for payload in done {
                // `body_size_limit` / `download_file` are node properties the
                // transport knows nothing about, so they are applied here.
                let payload = crate::http_request_node::deliver(payload, &delivery);
                self.emit_signal(
                    node_id,
                    "request_completed",
                    &[
                        Variant::Int(payload.result as i64),
                        Variant::Int(payload.response_code as i64),
                        Variant::PackedStringArray(payload.headers),
                        Variant::PackedByteArray(payload.body),
                    ],
                );
            }
        }
    }

    /// Screen-space top-left of a control, accumulating ancestor offsets the
    /// same way hit-testing does (pat-s3rrv.72).
    fn control_global_position(&self, node: NodeId) -> gdcore::math::Vector2 {
        let mut pos = crate::control::get_position(self, node);
        let mut cur = self.nodes.get(&node).and_then(|n| n.parent());
        while let Some(p) = cur {
            pos = pos + crate::control::get_position(self, p);
            cur = self.nodes.get(&p).and_then(|n| n.parent());
        }
        pos
    }

    /// Opens the popup owned by `owner`, anchored under it (pat-s3rrv.72).
    ///
    /// A MenuButton owns a `PopupMenu` CHILD; an OptionButton carries the items
    /// on itself. Both end up as the same model, so activation has one path.
    fn open_popup_for(&mut self, owner: NodeId) {
        let children: Vec<NodeId> = self
            .nodes
            .get(&owner)
            .map(|n| n.children().to_vec())
            .unwrap_or_default();
        let popup_node = children.into_iter().find(|&c| {
            self.get_node(c)
                .map(|n| n.class_name() == "PopupMenu")
                .unwrap_or(false)
        });
        let menu = crate::item_views::load_popup_menu(self, popup_node.unwrap_or(owner));
        // An empty menu opens nothing: Godot shows no panel, and opening one
        // would make the popup modal with no way to dismiss it by choosing.
        if menu.item_count() == 0 {
            return;
        }
        let pos = self.control_global_position(owner);
        let size = crate::control::get_size(self, owner);
        self.open_popup = Some(OpenPopup {
            owner,
            popup_node,
            menu,
            // Below the button, which is where Godot drops a MenuButton's menu.
            origin: gdcore::math::Vector2::new(pos.x, pos.y + size.y),
            width: size.x.max(1.0),
            hovered: None,
            bar_title: None,
        });
    }

    /// A `MenuBar`'s titles and which one is currently open, for the draw pass
    /// (pat-s3rrv.72).
    ///
    /// Returns the SAME titles `menu_bar_title_under` maps clicks against, so
    /// the strip drawn and the strip clickable cannot drift apart.
    pub(crate) fn menu_bar_strip(&self, bar: NodeId) -> (Vec<String>, Option<usize>) {
        let open = self
            .open_popup
            .as_ref()
            .filter(|o| o.owner == bar)
            .and_then(|o| o.bar_title);
        (self.menu_bar_titles(bar), open)
    }

    /// The `PopupMenu` children of a `MenuBar`, in tree order — one per title.
    fn menu_bar_popups(&self, bar: NodeId) -> Vec<NodeId> {
        self.nodes
            .get(&bar)
            .map(|n| n.children().to_vec())
            .unwrap_or_default()
            .into_iter()
            .filter(|&c| {
                self.get_node(c)
                    .map(|n| n.class_name() == "PopupMenu")
                    .unwrap_or(false)
            })
            .collect()
    }

    /// The strip's titles: a child's `title` property when set, else its node
    /// name — Godot's `MenuBar` labels each menu that way (`menu_bar.cpp`).
    fn menu_bar_titles(&self, bar: NodeId) -> Vec<String> {
        self.menu_bar_popups(bar)
            .into_iter()
            .map(|c| {
                let node = self.get_node(c);
                let titled = node.map(|n| n.get_property("title")).and_then(|v| match v {
                    Variant::String(t) if !t.is_empty() => Some(t),
                    _ => None,
                });
                titled.unwrap_or_else(|| node.map(|n| n.name().to_string()).unwrap_or_default())
            })
            .collect()
    }

    /// The title index under a global `position`, or `None` when the pointer is
    /// off the strip.
    fn menu_bar_title_under(&self, bar: NodeId, position: gdcore::math::Vector2) -> Option<usize> {
        let is_bar = self
            .get_node(bar)
            .map(|n| n.class_name() == "MenuBar")
            .unwrap_or(false);
        if !is_bar {
            return None;
        }
        let origin = self.control_global_position(bar);
        let local_y = position.y - origin.y;
        if local_y < 0.0 || local_y >= crate::menu::MenuBarModel::TITLE_HEIGHT {
            return None;
        }
        crate::menu::MenuBarModel::new(self.menu_bar_titles(bar)).title_at(position.x - origin.x)
    }

    /// Opens the `index`-th menu of `bar`, anchored under its own title.
    fn open_menu_bar_title(&mut self, bar: NodeId, index: usize) {
        let popups = self.menu_bar_popups(bar);
        let Some(&popup_node) = popups.get(index) else {
            return;
        };
        let menu = crate::item_views::load_popup_menu(self, popup_node);
        if menu.item_count() == 0 {
            return;
        }
        let model = crate::menu::MenuBarModel::new(self.menu_bar_titles(bar));
        let origin = self.control_global_position(bar);
        // The panel is as wide as its widest row, floored at the title it hangs
        // from — measured with the SAME synthetic advance the strip uses, so a
        // menu never renders narrower than the title that opened it.
        let width = (0..menu.item_count())
            .filter_map(|i| menu.get_item(i))
            .map(|it| crate::menu::MenuBarModel::title_width(&it.text))
            .fold(0.0f32, f32::max)
            .max(
                model
                    .titles()
                    .get(index)
                    .map(|t| crate::menu::MenuBarModel::title_width(t))
                    .unwrap_or(0.0),
            );
        self.open_popup = Some(OpenPopup {
            owner: bar,
            popup_node: Some(popup_node),
            menu,
            // Under its own title, not under the bar's left edge.
            origin: gdcore::math::Vector2::new(
                origin.x + model.title_x(index),
                origin.y + crate::menu::MenuBarModel::TITLE_HEIGHT,
            ),
            width: width.max(1.0),
            hovered: None,
            bar_title: Some(index),
        });
    }

    /// Routes a mouse event to the open popup; returns whether it consumed it.
    ///
    /// ⚠️ Runs BEFORE `control_at_point`. A `PopupMenu` is a Popup<-Window and
    /// is MODAL in Godot: while open it takes every mouse event, so a click
    /// inside its rect must never reach the control underneath and a click
    /// outside dismisses it rather than pressing whatever it landed on.
    fn popup_input_stage(
        &mut self,
        event: &gdplatform::input::InputEvent,
        position: gdcore::math::Vector2,
    ) -> bool {
        use gdplatform::input::InputEvent as Ev;
        // Everything the decision needs, read while the borrow is alive so the
        // mutating half below is free to take `&mut self`.
        let Some((inside, idx, activatable, owner, popup_node, item_text, bar_title)) =
            self.open_popup.as_ref().map(|open| {
                let o = open.origin;
                let h = open.menu.height();
                let inside = position.x >= o.x
                    && position.x < o.x + open.width
                    && position.y >= o.y
                    && position.y < o.y + h;
                let idx = if inside {
                    open.menu.index_at(position.y - o.y)
                } else {
                    None
                };
                // Read-only: `activate_item` toggles a checkbox as a side
                // effect, so asking WITH it would flip state on a row that may
                // not even be activatable.
                let activatable = idx.is_some_and(|i| open.menu.is_activatable(i));
                let text = idx
                    .and_then(|i| open.menu.get_item(i))
                    .map(|it| it.text.clone())
                    .unwrap_or_default();
                (
                    inside,
                    idx,
                    activatable,
                    open.owner,
                    open.popup_node,
                    text,
                    open.bar_title,
                )
            })
        else {
            return false;
        };

        match event {
            Ev::MouseMotion { .. } => {
                // Godot's MenuBar switches menus on hover WHILE one is open —
                // the behaviour every desktop menu bar has, and the reason
                // `MenuBarModel` keeps "open" and "hovered" apart. Moving across
                // a CLOSED bar must open nothing, which is why this lives here,
                // inside the already-open branch, rather than in the hover
                // bookkeeping of the normal stage.
                if !inside && bar_title.is_some() {
                    if let Some(next) = self.menu_bar_title_under(owner, position) {
                        if Some(next) != bar_title {
                            self.open_menu_bar_title(owner, next);
                            return true;
                        }
                    }
                }
                if let Some(o) = self.open_popup.as_mut() {
                    o.hovered = idx;
                }
                true
            }
            // The press is swallowed and the ACTIVATION happens on release,
            // matching Godot: pressing and dragging off a row must not fire it.
            Ev::MouseButton { pressed: true, .. } => true,
            Ev::MouseButton { pressed: false, .. } => {
                if !inside {
                    // Godot's autohide — dismiss without activating anything.
                    self.open_popup = None;
                    return true;
                }
                let Some(idx) = idx else { return true };
                if !activatable {
                    // A disabled row or separator keeps the menu open and emits
                    // nothing, rather than closing as if it had been chosen.
                    return true;
                }
                let id = self
                    .open_popup
                    .as_mut()
                    .and_then(|o| o.menu.activate_item(idx));
                self.open_popup = None;
                if let Some(id) = id {
                    // Godot emits both on the PopupMenu itself.
                    if let Some(pn) = popup_node {
                        self.emit_signal(pn, "id_pressed", &[Variant::Int(id)]);
                        self.emit_signal(pn, "index_pressed", &[Variant::Int(idx as i64)]);
                    }
                    let is_option = self
                        .get_node(owner)
                        .map(|n| n.class_name() == "OptionButton")
                        .unwrap_or(false);
                    if is_option {
                        if let Some(n) = self.get_node_mut(owner) {
                            n.set_property("selected", Variant::Int(idx as i64));
                            // The button shows the chosen entry, which is the
                            // visible half of `select()`.
                            n.set_property("text", Variant::String(item_text));
                        }
                        self.emit_signal(owner, "item_selected", &[Variant::Int(idx as i64)]);
                    }
                }
                true
            }
            _ => false,
        }
    }

    /// The nearest `SubViewportContainer` at or above `node`, if any
    /// (pat-s3rrv.155).
    ///
    /// Walks the ancestor chain because the hit is the control INSIDE the
    /// sub-viewport, not the container itself — `hit_test_control` descends
    /// through the container to map the click into sub-viewport space, so by
    /// the time we have a hit the container is an ancestor.
    ///
    /// ⚠️ Matches on the class name as well as ClassDB, mirroring
    /// `control::is_subviewport_container`: bare unit trees never populate the
    /// inheritance table, and a ClassDB-only check would silently answer `false`
    /// for every one of them.
    fn enclosing_subviewport_container(&self, node: NodeId) -> Option<NodeId> {
        let mut cur = Some(node);
        while let Some(id) = cur {
            let n = self.get_node(id)?;
            if n.class_name() == "SubViewportContainer" || n.is_class("SubViewportContainer") {
                return Some(id);
            }
            cur = n.parent();
        }
        None
    }

    fn gui_input_stage(&mut self, event: &gdplatform::input::InputEvent, payload: &Variant) {
        use gdplatform::input::InputEvent as Ev;
        let position = match event {
            Ev::MouseButton { position, .. } | Ev::MouseMotion { position, .. } => *position,
            // Godot's GUI stage routes key events by focus, not by position
            // (`Viewport::gui.key_focus`), so they take the other branch.
            Ev::Key { .. } => {
                self.gui_key_stage(event, payload);
                return;
            }
            _ => return,
        };

        // pat-s3rrv.72: the open popup is modal and gets first refusal, BEFORE
        // any hit-test — that is what stops a click inside it reaching the
        // control underneath.
        if self.popup_input_stage(event, position) {
            return;
        }

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

        // `[url=…]` hover (pat-s3rrv.74). Placed beside the mouse_entered
        // bookkeeping and BEFORE pointer capture on purpose: hover follows the
        // real cursor, so a link stops being hovered the moment the pointer
        // leaves it even mid-drag.
        if matches!(event, Ev::MouseMotion { .. }) {
            self.update_meta_hover(hit, position);
        }

        // Pointer capture (pat-s3rrv.12). Godot's viewport routes every motion
        // and the release to the control that took `gui.mouse_focus` on press,
        // instead of re-hit-testing under the cursor — otherwise dragging off a
        // button silently re-targets whatever is underneath, and no Control can
        // implement a drag at all. Hover bookkeeping above still follows the
        // real cursor, as it does in Godot.
        let captured = match event {
            Ev::MouseMotion { .. } | Ev::MouseButton { pressed: false, .. } => self.gui_mouse_focus,
            _ => None,
        };
        let hit = captured.filter(|id| self.nodes.contains_key(id)).or(hit);

        let Some(hit) = hit else {
            // A press over empty space clears any in-flight press so a later
            // release elsewhere cannot be mistaken for a click.
            if matches!(event, Ev::MouseButton { pressed: true, .. }) {
                self.gui_press_target = None;
                self.gui_mouse_focus = None;
            }
            return;
        };

        // `SubViewportContainer._propagate_input_event(event) -> bool` decides
        // whether an event is allowed to reach the hosted sub-scene
        // (pat-s3rrv.155). Godot asks the container before forwarding; a script
        // returning `false` makes the container swallow the event.
        //
        // ⚠️ Asked HERE rather than inside the hit test. `hit_test_control`
        // already descends through a SubViewportContainer into its children —
        // it has to, to map the click into sub-viewport space — but it takes
        // `&SceneTree`, and calling a script needs `&mut`. This stage is the
        // first point after the hit that holds `&mut self`.
        //
        // ⚠️ `call_script_method` returns `None` when the script does not define
        // the method, so an unscripted container (and a scripted one that does
        // not override it) is untouched — only an explicit `false` vetoes. A
        // script returning `true`, or nothing, forwards as before.
        if let Some(container) = self.enclosing_subviewport_container(hit) {
            if matches!(
                self.call_script_method(
                    container,
                    "_propagate_input_event",
                    std::slice::from_ref(payload)
                ),
                Some(Variant::Bool(false))
            ) {
                return;
            }
        }

        // Deliver `_gui_input` / `gui_input` up the PASS chain: the hit control
        // first, then each ancestor whose filter is PASS. A STOP control ends
        // the walk (it consumed the event).
        let mut target = Some(hit);
        while let Some(node_id) = target {
            // Godot hands `_gui_input` an event localized to the *receiving*
            // control (`localizer.xform(mpos)`, viewport.cpp:2078-2088), so the
            // payload is rebuilt per delivery rather than shared: the same click
            // reads different coordinates at each level of the PASS chain
            // (pat-s3rrv.13). Patina forwarded the one viewport-space payload,
            // so every `event.position` a script saw was off by the control's
            // own offset.
            let local = self.localize_gui_payload(node_id, payload);
            self.call_script_with_access(node_id, "_gui_input", std::slice::from_ref(&local));
            self.emit_signal(node_id, "gui_input", std::slice::from_ref(&local));
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

        // A click on a `[url=…]` run emits `meta_clicked(payload)`
        // (pat-s3rrv.74) — Godot's `RichTextLabel` signal, and the reason the
        // parser carries a meta per run at all.
        //
        // On RELEASE, like every other click action here: pressing and dragging
        // off a link must not follow it.
        if matches!(event, Ev::MouseButton { pressed: false, .. }) {
            let is_rich = self
                .get_node(hit)
                .map(|n| n.class_name() == "RichTextLabel")
                .unwrap_or(false);
            if is_rich {
                let rect = gdcore::math::Rect2::new(
                    self.control_global_position(hit),
                    crate::control::get_size(self, hit),
                );
                let clicked = crate::control_render::richtext_meta_regions(
                    self,
                    hit,
                    crate::control_render::RICHTEXT_DEFAULT_FONT_SIZE,
                    rect,
                )
                .into_iter()
                .find(|(r, _)| r.contains_point(position))
                .map(|(_, meta)| meta);
                if let Some(meta) = clicked {
                    self.emit_signal(hit, "meta_clicked", &[Variant::String(meta)]);
                    self.input_handled = true;
                }
            }
        }

        // A press on a MenuButton or OptionButton opens its popup
        // (pat-s3rrv.72). Guarded on nothing being open already: the modal
        // stage above consumes clicks while a popup is up, so reaching here
        // means the press really is a fresh open.
        if matches!(event, Ev::MouseButton { pressed: true, .. }) && self.open_popup.is_none() {
            let opens_popup = self
                .get_node(hit)
                .map(|n| matches!(n.class_name(), "MenuButton" | "OptionButton"))
                .unwrap_or(false);
            if opens_popup {
                self.open_popup_for(hit);
            } else if let Some(title) = self.menu_bar_title_under(hit, position) {
                // A MenuBar owns one PopupMenu per title, so which menu opens
                // depends on WHERE along the strip the press landed —
                // `open_popup_for` would always take the first child.
                self.open_menu_bar_title(hit, title);
            }
        }

        // A press on a TabContainer's header strip switches tabs and emits
        // `tab_changed(idx)`, as Godot's TabBar does on click (pat-0q2kx.83).
        // The strip is only reachable when no child covers it, which is exactly
        // the region `arrange_container` reserves above the current page.
        if matches!(event, Ev::MouseButton { pressed: true, .. }) {
            let is_tab_container = self
                .get_node(hit)
                .map(|n| n.class_name() == "TabContainer")
                .unwrap_or(false);
            if is_tab_container {
                if let Some(idx) = crate::control::tab_at_point(self, hit, position) {
                    if crate::control::get_current_tab(self, hit) != idx {
                        crate::control::set_current_tab(self, hit, idx as i64);
                        self.emit_signal(hit, "tab_changed", &[Variant::Int(idx as i64)]);
                    }
                }
            }
        }

        // Button press/release semantics on the originally hit control.
        // Wheel events scroll the nearest enclosing ScrollContainer rather than
        // acting as a click (pat-0q2kx.86). Godot routes them to the container's
        // internal scrollbars via `ScrollContainer::gui_input`, so the wheel
        // works anywhere over the container, not just over the bar itself.
        if let Ev::MouseButton {
            button:
                gdplatform::input::MouseButton::WheelUp
                | gdplatform::input::MouseButton::WheelDown
                | gdplatform::input::MouseButton::WheelLeft
                | gdplatform::input::MouseButton::WheelRight,
            pressed: true,
            ..
        } = event
        {
            // `down` is really "the positive direction": down for a vertical
            // wheel, right for a horizontal one (pat-s3rrv.173).
            let down = matches!(
                event,
                Ev::MouseButton {
                    button: gdplatform::input::MouseButton::WheelDown
                        | gdplatform::input::MouseButton::WheelRight,
                    ..
                }
            );
            let horizontal = matches!(
                event,
                Ev::MouseButton {
                    button: gdplatform::input::MouseButton::WheelLeft
                        | gdplatform::input::MouseButton::WheelRight,
                    ..
                }
            );
            // A standalone Slider/ScrollBar takes the notch itself, before the
            // enclosing-container walk below: `scroll_under_cursor` only looks
            // for a ScrollContainer, so a bare HScrollBar in a scene was
            // wheel-deaf even though it is exactly the control a wheel belongs
            // to. Godot gives the event to the control under the cursor, and the
            // container case only exists because a container FORWARDS to its own
            // internal bars (`ScrollContainer::gui_input`).
            //
            // ⚠️ Vertical wheels only. `wheel_delta` knows a direction but not an
            // axis, so feeding a horizontal notch to it would make a VSlider or
            // VScrollBar respond to WheelLeft/WheelRight — which Godot does not
            // do, and which no user expects from a sideways scroll.
            if let Some(delta) =
                crate::range_controls::wheel_delta(self, hit, down).filter(|_| !horizontal)
            {
                let target = crate::control::get_range_value(self, hit) + delta;
                if crate::control::set_range_value(self, hit, target) {
                    // Re-read: the stored value is snapped and clamped, so it is
                    // what the signal must carry.
                    let now = crate::control::get_range_value(self, hit);
                    self.emit_signal(hit, "value_changed", &[Variant::Float(now)]);
                }
                // Consumed either way — a notch at the end of the range is still
                // the scrollbar's notch, and must not fall through to a
                // container behind it and scroll something else.
                self.input_handled = true;
                return;
            }
            if self.scroll_under_cursor(hit, down, horizontal) {
                self.input_handled = true;
            }
            return;
        }

        // Range controls (Slider/ScrollBar) jump to the pointer on the press and
        // keep tracking it while captured — that second half is what makes a
        // drag a drag. Godot's `Slider::gui_input` acts on the button-down edge
        // and then on every motion while `grab.active`, so clicking the track
        // moves the value immediately and the following motion continues from
        // there (upstream/godot/scene/gui/slider.cpp).
        //
        // A SplitContainer divider drag, checked BEFORE the range controls: the
        // divider sits in the gap between two panes, so nothing else wants that
        // press, and a drag in flight must keep winning motion even when the
        // pointer wanders over a child (pat-s3rrv.173).
        let split_press = match event {
            Ev::MouseButton {
                pressed: true,
                button: gdplatform::input::MouseButton::Left,
                ..
            } => Some(true),
            // ⚠️ Keyed on the drag being in flight, NOT on `gui_mouse_focus`
            // matching `hit` the way the range controls below are. The divider
            // is a gap between two panes, so the captured node is the CONTAINER
            // while `hit` may be either child — comparing them would end the
            // drag the moment the pointer crossed into a pane. Capture is still
            // taken on press (see `drag_split_divider`); it is what keeps this
            // stage running at all once the cursor leaves the container.
            Ev::MouseMotion { .. } if self.gui_split_drag.is_some() => Some(false),
            _ => None,
        };
        if let Some(is_press) = split_press {
            if self.drag_split_divider(hit, position, is_press) {
                self.input_handled = true;
                return;
            }
        }

        // Placed AFTER the wheel block above, which returns for wheel events: a
        // wheel notch over a scrollbar must scroll, not be read as a drag to the
        // cursor's position. Only the left button drags, as in `slider.cpp` and
        // `scroll_bar.cpp`.
        let range_press = match event {
            Ev::MouseButton {
                pressed: true,
                button: gdplatform::input::MouseButton::Left,
                ..
            } => Some(true),
            // Captured, so this fires even when the pointer has left the rect —
            // dragging past the end and back is a real gesture.
            Ev::MouseMotion { .. } if self.gui_mouse_focus == Some(hit) => Some(false),
            _ => None,
        };
        if let Some(is_press) = range_press {
            if let Some(value) = self.drag_range_control(hit, position, is_press) {
                // `value_changed` fires ONLY on a real change: `drag_range_control`
                // returns `None` when the snapped value is unchanged, mirroring
                // `Range::set_value` (upstream/godot/scene/gui/range.cpp). Emitting
                // per motion sample instead would flood every connected callback
                // during a single drag.
                self.emit_signal(hit, "value_changed", &[Variant::Float(value)]);
                // The GUI consumed the event, so it does not also fall through to
                // physics picking — the same reason a button stops a click from
                // reaching the world behind it.
                self.input_handled = true;
            }
        }

        // pat-s3rrv.172: a ColorPicker tracks the pointer exactly as a slider
        // does — the press picks immediately and every captured motion keeps
        // picking, which is what makes a drag across the SV square a continuous
        // gesture rather than a series of jumps.
        //
        // Placed HERE, before the BaseButton block below, for the same reason
        // the range controls sit after the wheel block: a press on a picker must
        // be consumed as a colour pick, not fall through and be read as a click.
        // It deliberately does NOT return — falling through is what lets the
        // block below set `gui_mouse_focus`, and without that capture the
        // motions that follow would never route back here.
        let picker_press = match event {
            Ev::MouseButton {
                pressed: true,
                button: gdplatform::input::MouseButton::Left,
                ..
            } => Some(true),
            // Captured, so the drag survives the pointer leaving the square —
            // Godot keeps picking while the button is held and clamps at the
            // edges, which is what `sv_uv_at` / `hue_t_at` do.
            Ev::MouseMotion { .. } if self.gui_mouse_focus == Some(hit) => Some(false),
            _ => None,
        };
        if picker_press.is_some() {
            if let Some(color) = self.drag_color_picker(hit, position) {
                // 🔑 Emitted ONLY on a real change. `drag_color_picker` returns
                // `None` when the model reports the colour unchanged, mirroring
                // `drag_range_control` above — and `ColorPickerModel` was written
                // to answer exactly that question, because Godot does not
                // re-emit for a click landing on the value already selected.
                // Emitting per motion sample would flood every handler connected
                // during one drag.
                self.emit_signal(hit, "color_changed", &[Variant::Color(color)]);
                // 🔑 A picker opened BY a ColorPickerButton also reports through
                // the button, which is what makes the button's swatch follow the
                // colour. Upstream wires this at construction —
                // `picker->connect("color_changed", ..._color_changed)` inside
                // `_update_picker()` — and the handler assigns, redraws and
                // re-emits. Without it the picker moves and the swatch behind it
                // does not.
                if let Some(button) = self.color_picker_host(hit) {
                    if let Some(n) = self.get_node_mut(button) {
                        n.set_property("color", Variant::Color(color));
                    }
                    self.emit_signal(button, "color_changed", &[Variant::Color(color)]);
                }
                self.input_handled = true;
            }
        }

        if let Ev::MouseButton {
            pressed, button, ..
        } = event
        {
            let is_button = crate::control::node_is_base_button(self, hit);
            let disabled = crate::control::is_disabled(self, hit);
            // Godot numbers mouse buttons from 1 and builds `button_mask` as
            // `1 << (index - 1)`; a button outside the mask is ignored ENTIRELY
            // — no `button_down`, no press bookkeeping — which is why a
            // right-click on a default Button does nothing (pat-s3rrv.12).
            let button_index = match button {
                gdplatform::input::MouseButton::Left => 1,
                gdplatform::input::MouseButton::Right => 2,
                gdplatform::input::MouseButton::Middle => 3,
                _ => 0,
            };
            if is_button && !crate::control::button_mask_accepts(self, hit, button_index) {
                return;
            }
            if *pressed {
                self.gui_press_target = Some(hit);
                self.gui_mouse_focus = Some(hit);
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
                    // ACTION_MODE_BUTTON_PRESS fires on the DOWN edge instead of
                    // the release (`base_button.cpp::on_action_event`), so a
                    // button configured that way acts before the mouse comes up.
                    if crate::control::button_action_mode(self, hit)
                        == crate::control::ACTION_MODE_BUTTON_PRESS
                    {
                        crate::control::fire_button_action(self, hit);
                        // Godot clears the press attempt after a press-mode
                        // action so the coming release cannot fire it twice.
                        self.gui_press_target = None;
                    }
                }
                // Item views act on the press, not the release: Godot's
                // ItemList/TabBar `gui_input` handlers run under
                // `mb->is_pressed()` (pat-0q2kx.85).
                if !disabled {
                    crate::item_views::item_view_click(self, hit, position, false);
                    // A FoldableContainer folds on the PRESS too
                    // (`foldable_container.cpp:242-247` gates on
                    // `b->is_pressed()`), so it belongs here rather than beside
                    // `fire_button_action` in the release branch. The call is a
                    // no-op unless `hit` really is a FoldableContainer and the
                    // point really is inside its title bar (pat-s3rrv.165).
                    crate::control::foldable_title_press(self, hit, position);
                }
            } else {
                // A click only counts when press and release land on the same
                // control, as in Godot — dragging off a button cancels it.
                //
                // With the pointer captured, `hit` IS the pressed control even
                // when the cursor left its rect, so "did the release happen
                // inside?" is a separate geometric question — Godot's
                // `pressing_inside`. `keep_pressed_outside` waives it.
                let same = self.gui_press_target == Some(hit);
                let inside = crate::control::control_at_point(self, position) == Some(hit)
                    || crate::control::keeps_pressed_outside(self, hit);
                self.gui_press_target = None;
                self.gui_mouse_focus = None;
                // The grabber is let go on release, as `ScrollBar::gui_input`
                // clears `drag.active`; otherwise the next stray motion would
                // resume a drag the user already finished.
                self.gui_scrollbar_drag = None;
                self.gui_split_drag = None;
                if is_button && !disabled && same {
                    // `button_up` reports the physical release and fires even
                    // when the pointer wandered off; only the ACTION is gated on
                    // still being inside.
                    self.emit_signal(hit, "button_up", &[]);
                    if inside
                        && crate::control::button_action_mode(self, hit)
                            != crate::control::ACTION_MODE_BUTTON_PRESS
                    {
                        crate::control::fire_button_action(self, hit);
                    }
                }
            }
        }
    }

    /// Moves a Slider/ScrollBar to the pointer and returns its new value **only
    /// when it changed**, so the caller emits `value_changed` exactly as often
    /// as Godot does.
    ///
    /// Returns `None` for any control that is not a range control:
    /// `drag_slider_to_local` resolves the axis from the class name and gives up
    /// when there is none, so routing every hit control through here is safe.
    fn drag_range_control(
        &mut self,
        node_id: NodeId,
        position: gdcore::math::Vector2,
        is_press: bool,
    ) -> Option<f64> {
        use crate::range_controls::ScrollBarPress;
        // The drag math is expressed in the control's own space, the same space
        // the render arm draws the grabber in.
        let origin = crate::control::get_global_position(self, node_id);
        let local = gdcore::math::Vector2::new(position.x - origin.x, position.y - origin.y);

        let is_scrollbar = self
            .get_node(node_id)
            .map(|n| matches!(n.class_name(), "HScrollBar" | "VScrollBar"))
            .unwrap_or(false);
        if !is_scrollbar {
            // A slider jumps to the cursor: `Slider::gui_input` reads the value
            // straight off the click position.
            return crate::range_controls::drag_slider_to_local(self, node_id, local);
        }

        // ⚠️ A scrollbar does NOT jump to the cursor — see `scrollbar_press`.
        if is_press {
            match crate::range_controls::scrollbar_press(self, node_id, local)? {
                ScrollBarPress::Grab(drag) => {
                    // Seizing the grabber changes nothing by itself; the value
                    // moves only once the pointer does. That is what keeps the
                    // handle under the finger that grabbed it.
                    self.gui_scrollbar_drag = Some((node_id, drag));
                    None
                }
                ScrollBarPress::Page(amount) => {
                    self.gui_scrollbar_drag = None;
                    crate::range_controls::scrollbar_page(self, node_id, amount)
                }
            }
        } else {
            let (owner, drag) = self.gui_scrollbar_drag?;
            // A drag belongs to the bar it started on, so motion never leaks
            // into a different scrollbar the pointer happens to cross.
            if owner != node_id {
                return None;
            }
            crate::range_controls::scrollbar_drag_to(self, node_id, local, drag)
        }
    }

    /// How wide the grabbable band around a divider is, in pixels, when the
    /// container's `separation` is narrower than that.
    ///
    /// ⚠️ Load-bearing. `get_separation` returns **0** for a container with no
    /// `theme_override_constants/separation`, which is the default, so the
    /// divider is a zero-width line and would be literally impossible to grab.
    /// Godot avoids this with a themed dragger; this is the stand-in.
    const SPLIT_DRAG_GRAB_PX: f32 = 6.0;

    /// The SplitContainer at or above `from` whose divider band contains
    /// `position`, with its split axis. `None` when the press was not on a
    /// divider.
    ///
    /// ⚠️ The divider's position is read back off the LAID-OUT first child
    /// rather than recomputed. `arrange_split` places child 0 with exactly the
    /// span the divider starts after, so reading it keeps hit-testing and layout
    /// as one rule; re-deriving it here would duplicate the clamp arithmetic and
    /// the two would drift the first time either changed.
    fn split_divider_under(
        &self,
        from: NodeId,
        position: gdcore::math::Vector2,
    ) -> Option<(NodeId, bool)> {
        let mut cursor = Some(from);
        while let Some(id) = cursor {
            let class = self
                .nodes
                .get(&id)
                .map(|n| n.class_name().to_string())
                .unwrap_or_default();
            if matches!(
                class.as_str(),
                "SplitContainer" | "HSplitContainer" | "VSplitContainer"
            ) {
                let vertical = class == "VSplitContainer";
                let rect = crate::control::get_global_rect(self, id);
                let local = position - rect.position;
                let (along, across) = if vertical {
                    (local.y, local.x)
                } else {
                    (local.x, local.y)
                };
                let across_extent = if vertical { rect.size.x } else { rect.size.y };
                if across >= 0.0 && across <= across_extent {
                    let children: Vec<NodeId> = self
                        .nodes
                        .get(&id)
                        .map(|n| n.children().to_vec())
                        .unwrap_or_default();
                    if let Some(&first) = children.first() {
                        let first_size = crate::control::get_size(self, first);
                        let start = if vertical { first_size.y } else { first_size.x };
                        let sep = crate::control::get_separation(self, id) as f32;
                        let band = sep.max(Self::SPLIT_DRAG_GRAB_PX);
                        // Centre the grab band on the gap when the band is wider
                        // than the separation, so a hairline divider is grabbable
                        // from either side rather than only from below/right.
                        let pad = (band - sep).max(0.0) * 0.5;
                        if along >= start - pad && along <= start + sep + pad {
                            return Some((id, vertical));
                        }
                    }
                }
            }
            cursor = self.nodes.get(&id).and_then(|n| n.parent());
        }
        None
    }

    /// The `ColorPickerButton` that hosts `picker`, if one does (pat-s3rrv.172).
    ///
    /// ⚠️ The PARENT only, not an ancestor search. A ColorPicker dropped into a
    /// VBox that happens to sit inside some button-like control is not hosted by
    /// it, and reporting its colour through that control would invent a
    /// relationship the scene never declared. A hosted picker is always a direct
    /// child, because `control::color_picker_button_pressed` is what created it.
    fn color_picker_host(&self, picker: NodeId) -> Option<NodeId> {
        let parent = self.get_node(picker)?.parent()?;
        (self.get_node(parent)?.class_name() == "ColorPickerButton").then_some(parent)
    }

    /// Maps a pointer position onto a `ColorPicker`'s model and writes the
    /// resulting colour back onto the node (pat-s3rrv.172).
    ///
    /// Returns the new colour only when it actually changed, which is what gates
    /// `color_changed`.
    ///
    /// 🔑 The geometry comes from `color_picker::picker_layout` — the SAME
    /// function the draw arm uses — so a click always resolves to the colour
    /// that was painted under it. Duplicating the sub-rect arithmetic here is
    /// the obvious shortcut, and it is exactly how a picker ends up visually
    /// correct and one bar-width out on every click.
    fn drag_color_picker(
        &mut self,
        node_id: NodeId,
        position: gdcore::math::Vector2,
    ) -> Option<gdcore::math::Color> {
        let is_picker = self
            .get_node(node_id)
            .map(|n| n.class_name() == "ColorPicker")
            .unwrap_or(false);
        if !is_picker {
            return None;
        }

        // ⚠️ GLOBAL space, matching what the draw arm passes `picker_layout`
        // (`control_render.rs:571` builds `Rect2::new(global_pos, size)`). The
        // sub-rects come back in whatever space the rect went in, so mixing
        // local and global here would hit-test a picker as though it sat at the
        // viewport origin — correct at (0, 0) and wrong everywhere else.
        //
        // 🔑 `get_global_rect` rather than composing it by hand: its own doc says
        // "anything mapping a click onto sub-parts of a control (rows, tabs)
        // must measure from here", which is precisely this.
        let rect = crate::control::get_global_rect(self, node_id);

        let edit_alpha = !matches!(
            self.get_node(node_id).map(|n| n.get_property("edit_alpha")),
            Some(Variant::Bool(false))
        );
        let layout = crate::color_picker::picker_layout(rect, edit_alpha);
        let part = crate::color_picker::part_at(&layout, position)?;

        let current = match self.get_node(node_id).map(|n| n.get_property("color")) {
            Some(Variant::Color(c)) => c,
            _ => gdcore::math::Color::WHITE,
        };
        let mut model = crate::color_picker::ColorPickerModel::from_color(current);

        let changed = match part {
            crate::color_picker::PickerPart::Sv => {
                let (u, v) = crate::color_picker::sv_uv_at(&layout, position)?;
                model.pick_sv_rect(u, v)
            }
            crate::color_picker::PickerPart::Hue => {
                let t = crate::color_picker::hue_t_at(&layout, position)?;
                model.pick_hue_bar(t)
            }
            crate::color_picker::PickerPart::Alpha => {
                // The alpha bar runs left-to-right, unlike the vertical hue bar.
                let bar = layout.alpha?;
                let a = ((position.x - bar.position.x) / bar.size.x).clamp(0.0, 1.0);
                let before = model.alpha;
                model.alpha = a;
                model.alpha != before
            }
        };
        if !changed {
            return None;
        }

        let color = model.color();
        if let Some(n) = self.get_node_mut(node_id) {
            n.set_property("color", Variant::Color(color));
        }
        Some(color)
    }

    /// Begins or continues a divider drag. Returns whether the event was for a
    /// divider at all.
    ///
    /// The offset arithmetic and the `dragged` signal both live in
    /// `control::drag_split`, which already existed with no caller — this is
    /// only the input half.
    fn drag_split_divider(
        &mut self,
        hit: NodeId,
        position: gdcore::math::Vector2,
        is_press: bool,
    ) -> bool {
        if is_press {
            let Some((container, vertical)) = self.split_divider_under(hit, position) else {
                return false;
            };
            let grab = if vertical { position.y } else { position.x };
            let offset = crate::control::get_split_offset(self, container);
            self.gui_split_drag = Some((container, grab, offset));
            // ⚠️ Capture the pointer, exactly as the button path does at
            // `:6204`. Without this the drag dies the moment the cursor leaves
            // the container: `gui_input_stage` resolves `hit` with
            // `control_at_point`, and a motion outside every control makes it
            // return BEFORE reaching the split block. The press branch that
            // normally sets `gui_mouse_focus` is below this one and never runs,
            // because a divider press returns early.
            //
            // Dragging past the edge and back is the whole point of a divider,
            // so this is not an edge case — it is the common gesture.
            self.gui_mouse_focus = Some(container);
            return true;
        }

        let Some((container, grab, offset_at_press)) = self.gui_split_drag else {
            return false;
        };
        if !self.nodes.contains_key(&container) {
            self.gui_split_drag = None;
            return false;
        }
        let vertical = self
            .nodes
            .get(&container)
            .map(|n| n.class_name() == "VSplitContainer")
            .unwrap_or(false);
        let now = if vertical { position.y } else { position.x };
        let total = crate::control::get_size(self, container);
        if crate::control::drag_split(self, container, offset_at_press + (now - grab), total) {
            // ⚠️ `drag_split` only writes `split_offset` and emits `dragged`; it
            // does not re-lay-out. Godot spells that `queue_sort()`, and this
            // stage re-sorts inline the way the scroll-wheel branch does — a
            // moved divider whose panes have not been re-arranged is a divider
            // that visibly does not move (pat-s3rrv.165 hit the same thing).
            crate::control::arrange_container(self, container, total);
        }
        true
    }

    /// Scrolls the nearest ScrollContainer at or above `from` by one wheel step
    /// (pat-0q2kx.86). Returns whether anything actually scrolled, so a wheel
    /// over non-scrolling content still reaches gameplay code.
    ///
    /// The offset is clamped to the container's scrollable range, matching
    /// Godot's scrollbars — wheeling past the end parks at the last band
    /// instead of scrolling the content out of sight.
    fn scroll_under_cursor(&mut self, from: NodeId, down: bool, horizontal: bool) -> bool {
        let mut cursor = Some(from);
        while let Some(id) = cursor {
            let is_scroll = self
                .nodes
                .get(&id)
                .map(|n| n.class_name() == "ScrollContainer")
                .unwrap_or(false);
            if is_scroll {
                // `vertical_scroll_mode = SCROLL_MODE_DISABLED` (0) takes this
                // container out of the wheel's path entirely (pat-s3rrv.173).
                //
                // ⚠️ DISABLED is the ONLY mode that blocks scrolling —
                // `scroll_container.cpp:540-544` gates on
                // `!= SCROLL_MODE_DISABLED`. `SHOW_NEVER` (3) merely hides the
                // bar and still scrolls on the wheel; treating it as "no
                // scrolling" is the easy misreading of the enum's name.
                //
                // ⚠️ `continue`, not `return false`: a disabled container has
                // not consumed the notch, so an enclosing ScrollContainer must
                // still get it. Stopping the walk here would make a disabled
                // inner panel silently swallow its parent's scrolling.
                let mode_key = if horizontal {
                    "horizontal_scroll_mode"
                } else {
                    "vertical_scroll_mode"
                };
                let vmode = self
                    .nodes
                    .get(&id)
                    .map(|n| match n.get_property(mode_key) {
                        Variant::Int(i) => i,
                        _ => 1,
                    })
                    .unwrap_or(1);
                if vmode == 0 {
                    cursor = self.nodes.get(&id).and_then(|n| n.parent());
                    continue;
                }

                let view = crate::control::get_size(self, id);
                let limit = crate::control::max_scroll(self, id, view);
                let current = crate::control::get_scroll(self, id);
                // Godot: one wheel notch moves `page / PAGE_DIVISOR`, floored at
                // the range's `step` of 1 — `ScrollBar::gui_input`, with
                // PAGE_DIVISOR = 8 (upstream/godot/scene/gui/scroll_bar.cpp:84).
                // `page` is the viewport extent, so a taller container scrolls
                // proportionally further per notch.
                //
                // `scroll_vertical_custom_step` overrides that when set —
                // `custom_step >= 0 ? custom_step : <default>`
                // (`scroll_bar.cpp:112`, reached through
                // `ScrollContainer::set_vertical_custom_step`).
                //
                // ⚠️ `>= 0`, not `> 0`. The "unset" value is spelled **-1.0**,
                // and a custom step of exactly 0 is a real setting meaning "the
                // wheel moves nothing" — folding it in with unset would silently
                // restore default scrolling to a container that asked for none.
                let step_key = if horizontal {
                    "scroll_horizontal_custom_step"
                } else {
                    "scroll_vertical_custom_step"
                };
                let custom = self
                    .nodes
                    .get(&id)
                    .map(|n| match n.get_property(step_key) {
                        Variant::Float(f) => f as f32,
                        Variant::Int(i) => i as f32,
                        _ => -1.0,
                    })
                    .unwrap_or(-1.0);
                // The extent along the axis being scrolled: a horizontal notch
                // is sized by the container's WIDTH, not its height.
                let extent = if horizontal { view.x } else { view.y };
                let magnitude = if custom >= 0.0 {
                    custom
                } else {
                    (extent / 8.0).max(1.0)
                };
                let step = if down { magnitude } else { -magnitude };
                let (from_value, ceiling) = if horizontal {
                    (current.x, limit.x)
                } else {
                    (current.y, limit.y)
                };
                let next = (from_value + step).clamp(0.0, ceiling);
                if next == from_value {
                    return false;
                }
                let scrolled = if horizontal {
                    gdcore::math::Vector2::new(next, current.y)
                } else {
                    gdcore::math::Vector2::new(current.x, next)
                };
                crate::control::set_scroll(self, id, scrolled);
                // Re-run the container's sort so the content child's offset
                // reflects the new value before the next frame is drawn.
                crate::control::arrange_container(self, id, view);
                return true;
            }
            cursor = self.nodes.get(&id).and_then(|n| n.parent());
        }
        false
    }

    /// The keyboard half of the GUI stage (pat-0q2kx.84): key events reach the
    /// focus owner, not whatever the mouse happens to be over.
    ///
    /// Mirrors Godot's `Viewport::_gui_input_event` for keys:
    /// 1. the focus owner gets `_gui_input` / `gui_input` first, and a script
    ///    calling `set_input_as_handled()` there consumes the event;
    /// 2. otherwise the built-in UI actions run — Tab / Shift-Tab walk the focus
    ///    ring, the arrows walk `focus_neighbor_*`, and `ui_accept` operates the
    ///    focused BaseButton.
    ///
    /// The `SceneTree` holds no `InputMap` (that lives on `MainLoop`), so the
    /// action names are matched through the default bindings
    /// `InputMap::install_builtin_ui_actions` installs: Tab = `ui_focus_next`,
    /// Shift-Tab = `ui_focus_prev`, arrows = `ui_left`/`ui_up`/`ui_right`/
    /// `ui_down`, Enter and Space = `ui_accept`.
    fn gui_key_stage(&mut self, event: &gdplatform::input::InputEvent, payload: &Variant) {
        use gdplatform::input::{InputEvent as Ev, Key};
        let Ev::Key {
            key,
            pressed,
            shift,
            ctrl,
            ..
        } = event
        else {
            return;
        };

        // A control with focus sees every key event, press and release alike,
        // before the viewport interprets any of them as navigation.
        if let Some(owner) = self.focus_owner {
            if self.nodes.contains_key(&owner) {
                self.call_script_with_access(owner, "_gui_input", std::slice::from_ref(payload));
                self.emit_signal(owner, "gui_input", std::slice::from_ref(payload));
                if self.input_handled {
                    return;
                }
            }
        }

        // A focused LineEdit/TextEdit turns key events into text edits
        // (pat-0q2kx.87). This runs BEFORE the button and navigation arms
        // below, mirroring Godot, where `LineEdit::gui_input` handles the event
        // inside the focused control and calls `accept_event()`: while a text
        // field has focus, Left/Right move the caret rather than walking the
        // focus ring, and Enter submits rather than activating a button.
        if let Some(owner) = self.focus_owner {
            if crate::line_edit::handle_key(self, owner, *key, *pressed, *shift, *ctrl) {
                self.input_handled = true;
                return;
            }
        }

        // `ui_accept` is the one action a BaseButton consumes itself, and like a
        // mouse click it fires `pressed` on RELEASE — Godot's default
        // `ACTION_MODE_BUTTON_RELEASE` (upstream/godot/scene/gui/base_button.cpp
        // `on_action_event`). Handle it before the press-only navigation keys so
        // the release half is not dropped.
        if matches!(key, Key::Enter | Key::Space) {
            if let Some(owner) = self.focus_owner {
                if crate::control::node_is_base_button(self, owner)
                    && !crate::control::is_disabled(self, owner)
                {
                    // The action itself runs through the same path a click takes,
                    // so a keyboard-activated CheckBox toggles and emits
                    // `toggled` exactly like a clicked one (pat-s3rrv.12) — and
                    // an ACTION_MODE_BUTTON_PRESS button fires on the down edge
                    // here too.
                    let press_mode = crate::control::button_action_mode(self, owner)
                        == crate::control::ACTION_MODE_BUTTON_PRESS;
                    if *pressed {
                        self.emit_signal(owner, "button_down", &[]);
                        if press_mode {
                            crate::control::fire_button_action(self, owner);
                        }
                    } else {
                        self.emit_signal(owner, "button_up", &[]);
                        if !press_mode {
                            crate::control::fire_button_action(self, owner);
                        }
                    }
                    self.input_handled = true;
                }
            }
            return;
        }

        // Navigation acts on the press; the matching release is not a second
        // move.
        if !*pressed {
            return;
        }

        // A focused Slider consumes the arrow along its OWN axis and moves by
        // `step` (upstream/godot/scene/gui/slider.cpp `gui_input`).
        //
        // ⚠️ The axis check is not a detail. Godot returns WITHOUT
        // `accept_event()` when the key does not match the orientation, so
        // Left/Right on a VSlider still walks the focus ring instead of being
        // swallowed by a control that cannot act on it. Consuming every arrow
        // would strand keyboard users on the first slider in the ring.
        if let Some(owner) = self.focus_owner {
            if let Some(delta) = self.range_key_delta(owner, *key) {
                let target = crate::control::get_range_value(self, owner) + delta;
                if crate::control::set_range_value(self, owner, target) {
                    // Re-read rather than reporting `target`: `set_range_value`
                    // snaps to `step` and clamps to [min, max - page], so the
                    // stored value is what the signal must carry.
                    let now = crate::control::get_range_value(self, owner);
                    self.emit_signal(owner, "value_changed", &[Variant::Float(now)]);
                }
                // Accepted whenever the axis matched, even when the value was
                // already at the end of its range: the slider still owns the key,
                // so focus must not move out from under it.
                self.input_handled = true;
                return;
            }
        }

        let moved = match key {
            Key::Tab if *shift => self.focus_prev(),
            Key::Tab => self.focus_next(),
            Key::Left => self.focus_neighbor(crate::control::FocusSide::Left),
            Key::Up => self.focus_neighbor(crate::control::FocusSide::Top),
            Key::Right => self.focus_neighbor(crate::control::FocusSide::Right),
            Key::Down => self.focus_neighbor(crate::control::FocusSide::Bottom),
            _ => return,
        };

        // Tab is always consumed by the GUI stage in Godot even when the ring
        // has nowhere to go; an arrow key with no neighbor falls through to
        // `_unhandled_input` so gameplay can still read it.
        if moved || matches!(key, Key::Tab) {
            self.input_handled = true;
        }
    }

    /// The value delta an arrow key applies to a focused range control, or
    /// `None` when the key is off the control's axis — or it is not a range
    /// control at all, which is what keeps this out of every other control's way.
    fn range_key_delta(&self, node_id: NodeId, key: gdplatform::input::Key) -> Option<f64> {
        use crate::range_controls::SliderAxis;
        use gdplatform::input::Key;
        let class = self.get_node(node_id)?.class_name().to_string();
        let axis = crate::range_controls::slider_axis(&class)?;
        // `Slider::gui_input` returns immediately when `editable` is false. An
        // absent property means editable, since that is the registered default.
        if matches!(
            self.get_node(node_id).map(|n| n.get_property("editable")),
            Some(Variant::Bool(false))
        ) {
            return None;
        }
        let step = crate::control::get_range_step(self, node_id);
        match (axis, key) {
            (SliderAxis::Horizontal, Key::Right) => Some(step),
            (SliderAxis::Horizontal, Key::Left) => Some(-step),
            // Up INCREASES on a vertical slider: its maximum is at the top, the
            // same inversion `ratio_at_local_point` applies to the drag.
            (SliderAxis::Vertical, Key::Up) => Some(step),
            (SliderAxis::Vertical, Key::Down) => Some(-step),
            _ => None,
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
        // `ScrollContainer.follow_focus` scrolls a newly focused descendant into
        // view (`scroll_container.cpp:311-312`), which is what makes keyboard
        // navigation through a long scrolled list usable at all.
        self.follow_focus_to(node_id);
    }

    /// Scrolls the nearest `follow_focus` ScrollContainer so `node_id` is
    /// visible, if there is one (pat-s3rrv.173).
    ///
    /// Port of `ScrollContainer::ensure_control_visible`
    /// (`scroll_container.cpp:322-339`), without the rotation and scrollbar
    /// margin terms — Patina's scroll containers have neither.
    fn follow_focus_to(&mut self, node_id: NodeId) {
        let mut cursor = self.nodes.get(&node_id).and_then(|n| n.parent());
        while let Some(id) = cursor {
            let is_scroll = self
                .nodes
                .get(&id)
                .map(|n| n.class_name() == "ScrollContainer")
                .unwrap_or(false);
            if is_scroll {
                let follows = self
                    .nodes
                    .get(&id)
                    .map(|n| matches!(n.get_property("follow_focus"), Variant::Bool(true)))
                    .unwrap_or(false);
                if !follows {
                    // ⚠️ Godot's default is FALSE, so a container that has not
                    // opted in must not move. Keep walking: an outer container
                    // may still follow.
                    cursor = self.nodes.get(&id).and_then(|n| n.parent());
                    continue;
                }
                let view = crate::control::get_size(self, id);
                let container_origin = crate::control::get_global_rect(self, id).position;
                let child = crate::control::get_global_rect(self, node_id);
                // The focused control's rect in the container's own space. This
                // already reflects the current scroll, because the content child
                // is positioned by it — the same thing Godot's `other_in_this`
                // gives, which is why the result is applied as a DELTA below.
                let top_left = child.position - container_origin;

                // `MAX(MIN(near, 0), far - view)` — scroll up when the control
                // starts above the view, down when it ends below it, and not at
                // all when it already fits (`scroll_container.cpp:334-335`).
                let axis =
                    |near: f32, extent: f32, view: f32| near.min(0.0).max(near + extent - view);
                let diff = gdcore::math::Vector2::new(
                    axis(top_left.x, child.size.x, view.x),
                    axis(top_left.y, child.size.y, view.y),
                );
                if diff.x == 0.0 && diff.y == 0.0 {
                    return;
                }
                let limit = crate::control::max_scroll(self, id, view);
                let current = crate::control::get_scroll(self, id);
                let next = gdcore::math::Vector2::new(
                    (current.x + diff.x).clamp(0.0, limit.x),
                    (current.y + diff.y).clamp(0.0, limit.y),
                );
                crate::control::set_scroll(self, id, next);
                // Re-sort so the content child's offset reflects the new scroll,
                // as the wheel path does.
                crate::control::arrange_container(self, id, view);
                return;
            }
            cursor = self.nodes.get(&id).and_then(|n| n.parent());
        }
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
    /// Both `FOCUS_CLICK` and `FOCUS_ALL` take click focus; only `FOCUS_ALL`
    /// joins the Tab ring. The class-default fallback (a `.tscn` Button never
    /// serializes its default `focus_mode`) lives in
    /// [`control::resolved_focus_mode`](crate::control::resolved_focus_mode) so
    /// click focus and keyboard traversal cannot drift apart.
    fn control_takes_click_focus(&self, node_id: NodeId) -> bool {
        crate::control::resolved_focus_mode(self, node_id) != crate::control::FocusMode::None
    }

    // -- focus traversal: Tab / Shift-Tab / arrows (pat-0q2kx.84) -----------

    /// Moves focus to [`find_next_valid_focus`](crate::control::find_next_valid_focus)
    /// — Godot's `ui_focus_next` (Tab). With nothing focused yet, focus lands on
    /// the first keyboard-focusable Control, as pressing Tab into a fresh window
    /// does in Godot.
    ///
    /// Returns whether focus actually moved, so the caller can decide if the
    /// event was consumed.
    pub fn focus_next(&mut self) -> bool {
        let target = match self.focus_owner {
            Some(current) => crate::control::find_next_valid_focus(self, current),
            None => crate::control::first_keyboard_focusable(self),
        };
        self.move_focus_to(target)
    }

    /// Moves focus to [`find_prev_valid_focus`](crate::control::find_prev_valid_focus)
    /// — Godot's `ui_focus_prev` (Shift-Tab).
    pub fn focus_prev(&mut self) -> bool {
        let target = match self.focus_owner {
            Some(current) => crate::control::find_prev_valid_focus(self, current),
            None => crate::control::first_keyboard_focusable(self),
        };
        self.move_focus_to(target)
    }

    /// Moves focus to the neighbor on `side` — Godot's
    /// `ui_left`/`ui_up`/`ui_right`/`ui_down` arrow navigation. Does nothing
    /// (and reports `false`) when nothing holds focus or no neighbor exists.
    pub fn focus_neighbor(&mut self, side: crate::control::FocusSide) -> bool {
        let Some(current) = self.focus_owner else {
            return false;
        };
        let target = crate::control::find_valid_focus_neighbor(self, current, side);
        self.move_focus_to(target)
    }

    /// Grabs focus for `target` when the walk found one, reporting whether
    /// focus actually changed hands.
    fn move_focus_to(&mut self, target: Option<NodeId>) -> bool {
        match target {
            Some(next) if self.focus_owner != Some(next) => {
                self.grab_focus(next);
                true
            }
            _ => false,
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
            // ⚠️ The PHYSICS ordering, not the idle one (pat-0q2kx.221).
            .all_nodes_in_physics_process_order()
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
    /// Enables/disables input delivery to `node_id` (Godot
    /// `Node.set_process_input`) — pat-0q2kx.138. Returns `false` for an
    /// unknown node.
    pub fn set_node_process_input(&mut self, node_id: NodeId, enabled: bool) -> bool {
        match self.nodes.get_mut(&node_id) {
            Some(n) => {
                n.set_process_input(enabled);
                true
            }
            None => false,
        }
    }

    /// Whether the input stages deliver to `node_id`
    /// (Godot `Node.is_processing_input`).
    pub fn is_node_processing_input(&self, node_id: NodeId) -> bool {
        self.nodes
            .get(&node_id)
            .is_some_and(|n| n.is_processing_input())
    }

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
        // NO `process_timers` here (pat-s3rrv.35). `MainLoop::step` calls both
        // this and `process_script_coroutines`, which also advances timers, so
        // every SceneTreeTimer lost 2×delta per frame and
        // `await create_timer(1.0).timeout` fired after ~0.5 s. The coroutine
        // pass is the single owner because it is the one that then drains
        // `take_timeout_signals` to wake the parked awaits.
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
pub(crate) fn sprite_frames_from_node(node: &Node) -> Option<SpriteFrames> {
    let names = match node.get_property("__sf_anims__") {
        Variant::PackedStringArray(v) if !v.is_empty() => v,
        _ => return None,
    };
    let mut frames = SpriteFrames::new();
    for name in &names {
        // `__sf_<name>_frames` holds the resolved texture path of each frame, in
        // play order. This used to be read for its *length* only and rebuilt as
        // that many textureless frames — which is why playback could advance
        // `frame` correctly while the node never had a texture to draw
        // (pat-0q2kx.100). The paths are the whole point of the array.
        let entries: Vec<crate::sprite_frames::SpriteFrameEntry> =
            match node.get_property(&format!("__sf_{name}_frames")) {
                Variant::PackedStringArray(paths) => paths
                    .iter()
                    .map(|p| crate::sprite_frames::SpriteFrameEntry::new(p.as_str()))
                    .collect(),
                _ => Vec::new(),
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
        frames.add_animation_frames(name, entries, speed, loops);
    }
    Some(frames)
}

/// Audio-track keys the playhead crossed between `prev` and `curr`, as
/// `(node_path, stream_path, start_offset)` triples (pat-0q2kx.157).
///
/// `TrackType::Audio` parsed and round-tripped but was never applied: the
/// advance path handled only Method and property tracks, so a keyed stream
/// stayed silent forever. Crossing semantics mirror
/// [`AnimationPlayer::method_calls_in_range`] exactly — `(prev, curr]` forward,
/// `[curr, prev)` reversed — so a key fires once as the playhead passes it and
/// once again only if the animation loops back over it.
///
/// This lives here rather than beside `method_calls_in_range` in `animation.rs`
/// because everything it needs is already public (`animations`, `tracks`,
/// `node_path`, `track_type`, `keyframes`) and that file was held under an
/// exclusive reservation by another agent when this landed. Moving it next to
/// its sibling later is a pure relocation with no call-site changes.
fn audio_keys_in_range(
    player: &crate::animation::AnimationPlayer,
    prev: f64,
    curr: f64,
) -> Vec<(String, String, f64)> {
    let Some(name) = player.current_animation() else {
        return Vec::new();
    };
    let Some(anim) = player.animations.get(name) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for track in &anim.tracks {
        if track.track_type != crate::animation::TrackType::Audio {
            continue;
        }
        for kf in track.keyframes() {
            let crossed = if curr >= prev {
                kf.time > prev && kf.time <= curr
            } else {
                kf.time < prev && kf.time >= curr
            };
            if !crossed {
                continue;
            }
            // Godot's audio keys are `{stream, start_offset, end_offset}` dicts
            // (see the `TrackType::Audio` doc in `animation.rs`). A key without
            // a usable stream is skipped rather than starting a silent voice.
            let Variant::Dictionary(d) = &kf.value else {
                continue;
            };
            let stream = match d.get("stream") {
                Some(Variant::String(s)) if !s.is_empty() => s.clone(),
                _ => continue,
            };
            let start_offset = match d.get("start_offset") {
                Some(Variant::Float(f)) => *f,
                Some(Variant::Int(i)) => *i as f64,
                _ => 0.0,
            };
            out.push((track.node_path.clone(), stream, start_offset));
        }
    }
    out
}

/// Writes an animated value into `node`, decomposing a Godot property path that
/// addresses a COMPONENT of a compound property (`position:x`) (pat-s3rrv.44).
///
/// The node's property bag is flat, so a bare `set_property("position:x", ..)`
/// would insert a key nothing ever reads — the animation would run and the
/// sprite would not move. Bézier tracks always key a scalar this way, and value
/// tracks may.
///
/// This reuses the tween layer's `split_property_path`/`write_subname` rather
/// than re-deriving the component table: the two paths must agree on what
/// `position:x` means, and a second implementation is a second thing to drift.
/// A subname that names no component of the base value (or a base that is still
/// `Nil` because nothing seeded it) leaves the property untouched, which is the
/// same conservative choice the tween path makes.
fn set_animated_property_path(node: &mut Node, path: &str, value: Variant) {
    match crate::tween::split_property_path(path) {
        (prop, Some(sub)) => {
            let base = node.get_property(prop);
            if let Some(updated) = crate::tween::write_subname(&base, sub, &value) {
                node.set_property(prop, updated);
            }
        }
        (prop, None) => {
            node.set_property(prop, value);
        }
    }
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

/// Converts an [`InputEvent`](gdplatform::input::InputEvent) into the
/// `InputEvent` **object** a script's input virtual receives (pat-s3rrv.14).
///
/// Godot's `Viewport::push_input` delivers a RefCounted `InputEvent` subclass,
/// and every ported project is written against that: `if event is
/// InputEventKey and event.keycode == KEY_W`. Patina used to hand scripts a
/// plain Dictionary with ad-hoc keys and the WRONG TYPES — `keycode` was the
/// name string `"W"` where Godot's is the int `KEY_W == 87`, and `button` was
/// the Rust debug string `"Left"` where Godot exposes `button_index ==
/// MOUSE_BUTTON_LEFT == 1`. No real Godot code compares against either, so
/// every ported `_input` body was silently dead.
///
/// Each event now becomes a `Variant::Resource` carrying the Godot CLASS NAME
/// and Godot PROPERTY NAMES/TYPES. That is what makes both halves work with no
/// new Variant kind: the interpreter's `is` operator already answers from
/// `ResourceRef::class_name`, and member access already reads
/// `ResourceRef::properties`.
fn input_event_object(class_name: &str, props: Vec<(&str, Variant)>) -> Variant {
    let mut properties = std::collections::HashMap::new();
    for (k, v) in props {
        properties.insert(k.to_string(), v);
    }
    // BUILD FIX (BlackSnow, not the author of this fn): `Variant::Resource`
    // holds a `Box<ResourceRef>` (gdvariant/src/variant.rs:272), so this needs
    // the `Box::new`. Without it the whole `gdscene` lib failed E0308 and every
    // gdscene bead in the verify lane FAILed behind it. Reverting this is fine
    // if the boxing moves — the shape of the fix is all that matters here.
    Variant::Resource(Box::new(gdvariant::ResourceRef {
        // Input events are synthesized, not loaded from disk; Godot's own
        // `InputEvent.resource_path` is likewise empty.
        path: String::new(),
        class_name: class_name.to_string(),
        properties,
    }))
}

/// The four `InputEventWithModifiers` flags every key/mouse event carries.
///
/// `meta_pressed` has no source in Patina's event enum yet, so it is reported
/// as `false` rather than omitted — a script reading `event.meta_pressed` must
/// get a bool, not `Nil`.
fn modifier_props(shift: bool, ctrl: bool, alt: bool) -> Vec<(&'static str, Variant)> {
    vec![
        ("alt_pressed", Variant::Bool(alt)),
        ("shift_pressed", Variant::Bool(shift)),
        ("ctrl_pressed", Variant::Bool(ctrl)),
        ("meta_pressed", Variant::Bool(false)),
    ]
}

/// The stylus data the last pointer event carried (pat-s3rrv.76).
///
/// ⚠️ CARRIED, NOT DERIVED — which is why this is a side channel and not a
/// computation like `double_click`. Pressure and tilt only exist if the browser
/// reports them, so they have to travel from the pointer event to the script.
///
/// They ride HERE rather than on `InputEvent::MouseMotion` because that variant
/// has 33 construction sites across six crates; the same objection kept
/// `double_click` off `InputEvent::MouseButton` (74 sites), so this follows the
/// precedent already set in `push_input` rather than inventing a second shape.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PenState {
    /// `0.0`..=`1.0`; Godot reports `1.0` for a plain mouse press and `0.0`
    /// otherwise (`InputEventMouseMotion.pressure`).
    pub pressure: f32,
    /// X/Y tilt in `-1.0`..=`1.0` (`InputEventMouseMotion.tilt`).
    pub tilt: gdcore::math::Vector2,
    /// Whether the eraser end of the stylus is in use.
    pub inverted: bool,
}

impl Default for PenState {
    fn default() -> Self {
        Self {
            pressure: 0.0,
            tilt: gdcore::math::Vector2::ZERO,
            inverted: false,
        }
    }
}

pub(crate) fn input_event_to_variant(
    event: &gdplatform::input::InputEvent,
    double_click: bool,
    pen: PenState,
    modifiers: (bool, bool, bool),
) -> Variant {
    use gdplatform::input::InputEvent as Ev;

    match event {
        Ev::Key {
            key,
            pressed,
            shift,
            ctrl,
            alt,
        } => {
            let code = key.to_godot_keycode();
            let mut props = vec![
                ("pressed", Variant::Bool(*pressed)),
                ("keycode", Variant::Int(code)),
                // Patina has one key identity, so the physical key and the
                // label agree with the keycode. Godot separates them for
                // non-QWERTY layouts; reporting the same value keeps every
                // comparison a script can write true rather than Nil.
                ("physical_keycode", Variant::Int(code)),
                ("key_label", Variant::Int(code)),
                ("unicode", Variant::Int(0)),
                ("location", Variant::Int(0)),
                ("echo", Variant::Bool(false)),
                ("device", Variant::Int(0)),
            ];
            props.extend(modifier_props(*shift, *ctrl, *alt));
            input_event_object("InputEventKey", props)
        }
        Ev::MouseButton {
            button,
            pressed,
            position,
        } => {
            let mut props = vec![
                ("button_index", Variant::Int(button.to_godot_index())),
                ("pressed", Variant::Bool(*pressed)),
                ("canceled", Variant::Bool(false)),
                ("double_click", Variant::Bool(double_click)),
                ("factor", Variant::Float(1.0)),
                ("position", Variant::Vector2(*position)),
                ("global_position", Variant::Vector2(*position)),
                (
                    "button_mask",
                    Variant::Int(if *pressed { button.to_godot_mask() } else { 0 }),
                ),
                ("device", Variant::Int(0)),
            ];
            props.extend(modifier_props(modifiers.0, modifiers.1, modifiers.2));
            input_event_object("InputEventMouseButton", props)
        }
        Ev::MouseMotion { position, relative } => {
            let mut props = vec![
                ("position", Variant::Vector2(*position)),
                ("global_position", Variant::Vector2(*position)),
                ("relative", Variant::Vector2(*relative)),
                ("screen_relative", Variant::Vector2(*relative)),
                ("velocity", Variant::Vector2(gdcore::math::Vector2::ZERO)),
                (
                    "screen_velocity",
                    Variant::Vector2(gdcore::math::Vector2::ZERO),
                ),
                ("pressure", Variant::Float(f64::from(pen.pressure))),
                ("tilt", Variant::Vector2(pen.tilt)),
                ("pen_inverted", Variant::Bool(pen.inverted)),
                ("button_mask", Variant::Int(0)),
                ("device", Variant::Int(0)),
            ];
            props.extend(modifier_props(modifiers.0, modifiers.1, modifiers.2));
            input_event_object("InputEventMouseMotion", props)
        }
        Ev::GamepadButton {
            button,
            pressed,
            gamepad_id,
        } => input_event_object(
            "InputEventJoypadButton",
            vec![
                // BUILD FIX (BlackSnow, not the author): `GamepadButton::to_godot_index`
                // returns `Option<i64>`, unlike the same-named `MouseButton` method a
                // few arms up which returns a bare `i64` — easy to conflate, and it was
                // E0308 here. `None` occurs only for `LeftTrigger`/`RightTrigger`, which
                // Godot models as analog AXES rather than buttons, so `-1` is not an
                // invented sentinel: it is Godot's own `JOY_BUTTON_INVALID`.
                (
                    "button_index",
                    Variant::Int(button.to_godot_index().unwrap_or(-1)),
                ),
                ("pressed", Variant::Bool(*pressed)),
                ("pressure", Variant::Float(0.0)),
                ("device", Variant::Int(*gamepad_id as i64)),
            ],
        ),
        Ev::GamepadAxis {
            axis,
            value,
            gamepad_id,
        } => input_event_object(
            "InputEventJoypadMotion",
            vec![
                ("axis", Variant::Int(axis.to_godot_index())),
                ("axis_value", Variant::Float(f64::from(*value))),
                ("device", Variant::Int(*gamepad_id as i64)),
            ],
        ),
        Ev::ScreenTouch {
            index,
            position,
            pressed,
            double_tap,
            canceled,
        } => input_event_object(
            "InputEventScreenTouch",
            vec![
                ("index", Variant::Int(*index as i64)),
                ("position", Variant::Vector2(*position)),
                ("pressed", Variant::Bool(*pressed)),
                ("double_tap", Variant::Bool(*double_tap)),
                ("canceled", Variant::Bool(*canceled)),
                ("device", Variant::Int(0)),
            ],
        ),
        Ev::ScreenDrag {
            index,
            position,
            relative,
            velocity,
            pressure,
            tilt,
        } => input_event_object(
            "InputEventScreenDrag",
            vec![
                ("index", Variant::Int(*index as i64)),
                ("position", Variant::Vector2(*position)),
                ("relative", Variant::Vector2(*relative)),
                ("screen_relative", Variant::Vector2(*relative)),
                ("velocity", Variant::Vector2(*velocity)),
                ("screen_velocity", Variant::Vector2(*velocity)),
                ("pressure", Variant::Float(f64::from(*pressure))),
                ("tilt", Variant::Vector2(*tilt)),
                ("pen_inverted", Variant::Bool(false)),
                ("device", Variant::Int(0)),
            ],
        ),
        // Gestures (pat-s3rrv.14). These three classes were registered in
        // ClassDB (`class_db.rs:22707-22736`) with nothing able to produce one,
        // so `event is InputEventPanGesture` type-checked and could never be
        // true — a dead script branch rather than an error.
        //
        // `InputEventGesture` derives from `InputEventWithModifiers`, so the
        // modifier props are emitted (as `false`) rather than omitted: leaving
        // them out makes `event.shift_pressed` read Nil where Godot gives a bool.
        Ev::MagnifyGesture { position, factor } => {
            let mut props = vec![
                ("position", Variant::Vector2(*position)),
                ("factor", Variant::Float(f64::from(*factor))),
                ("device", Variant::Int(0)),
            ];
            props.extend(modifier_props(modifiers.0, modifiers.1, modifiers.2));
            input_event_object("InputEventMagnifyGesture", props)
        }
        Ev::PanGesture { position, delta } => {
            let mut props = vec![
                ("position", Variant::Vector2(*position)),
                ("delta", Variant::Vector2(*delta)),
                ("device", Variant::Int(0)),
            ];
            props.extend(modifier_props(modifiers.0, modifiers.1, modifiers.2));
            input_event_object("InputEventPanGesture", props)
        }
        Ev::Action {
            action,
            pressed,
            strength,
        } => input_event_object(
            "InputEventAction",
            vec![
                ("action", Variant::String(action.clone())),
                ("pressed", Variant::Bool(*pressed)),
                ("strength", Variant::Float(f64::from(*strength))),
                ("event_index", Variant::Int(-1)),
                ("device", Variant::Int(0)),
            ],
        ),
        // Godot `InputEventMIDI` (pat-0q2kx.246). Every field is present on
        // every message, matching Godot's single concrete class — a script
        // switches on `message` first, exactly as it would there.
        Ev::Midi {
            channel,
            message,
            pitch,
            velocity,
            controller_number,
            controller_value,
            instrument,
            pressure,
            device,
        } => input_event_object(
            "InputEventMIDI",
            vec![
                ("channel", Variant::Int(i64::from(*channel))),
                // The @GlobalScope `MIDI_MESSAGE_*` integer, so a script can
                // write `event.message == MIDI_MESSAGE_NOTE_ON`.
                ("message", Variant::Int(i64::from(*message))),
                ("pitch", Variant::Int(i64::from(*pitch))),
                ("velocity", Variant::Int(i64::from(*velocity))),
                (
                    "controller_number",
                    Variant::Int(i64::from(*controller_number)),
                ),
                (
                    "controller_value",
                    Variant::Int(i64::from(*controller_value)),
                ),
                ("instrument", Variant::Int(i64::from(*instrument))),
                ("pressure", Variant::Int(i64::from(*pressure))),
                ("device", Variant::Int(i64::from(*device))),
            ],
        ),
    }
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

    // -- Node API completeness (pat-0q2kx.221) -------------------------------

    /// Delivers `what` to `start` and every descendant, parent-first
    /// (Godot `Node.propagate_notification`).
    ///
    /// ⚠️ Distinct from the existing [`propagate_call`](Self::propagate_call),
    /// which runs a Rust closure over the nodes. This one reaches **scripts**:
    /// each node's `_notification(what)` fires, which is what a scene actually
    /// observes.
    ///
    /// The traversal order is captured up front, so a `_notification` handler
    /// that adds or removes nodes cannot make the walk skip or revisit — Godot
    /// has the same snapshot semantics.
    pub fn propagate_notification(&mut self, start: NodeId, what: i64) {
        let mut order = vec![start];
        order.extend(self.descendants(start));
        for id in order {
            // Record on the node even when it has no script, so a Rust-side
            // observer (and `notification_log`) sees the same delivery a
            // scripted node would.
            if let Some(node) = self.nodes.get_mut(&id) {
                node.receive_notification(gdobject::notification::Notification::new(what as i32));
            }
            self.notify_script(id, what);
        }
    }

    /// Replaces `old` with `new_node` in the tree, moving `old`'s children onto
    /// it (Godot `Node.replace_by(node, keep_groups)`).
    ///
    /// `new_node` must already be in the tree — Godot's signature takes a node,
    /// and an orphan has nothing to re-root children onto. `old` is removed.
    /// Returns `false` when either id is unknown or `old` is the root (which has
    /// no parent to splice into).
    ///
    /// ⚠️ The replacement takes `old`'s **index** among its siblings, not the
    /// end of the child list. Draw order and `get_index()` both key off that, so
    /// appending instead would silently reorder a scene.
    pub fn replace_by(&mut self, old: NodeId, new_node: NodeId, keep_groups: bool) -> bool {
        let Some(parent) = self.nodes.get(&old).and_then(|n| n.parent()) else {
            return false;
        };
        if !self.nodes.contains_key(&new_node) || old == new_node {
            return false;
        }
        let index = match self.nodes.get(&parent) {
            Some(p) => match p.children().iter().position(|c| *c == old) {
                Some(i) => i,
                None => return false,
            },
            None => return false,
        };
        let children: Vec<NodeId> = match self.nodes.get(&old) {
            Some(n) => n.children().to_vec(),
            None => return false,
        };
        let groups: Vec<String> = if keep_groups {
            self.nodes
                .get(&old)
                .map(|n| n.groups().iter().cloned().collect())
                .unwrap_or_default()
        } else {
            Vec::new()
        };

        // Re-parent the children first, while `old` still owns them.
        for child in children {
            if let Some(c) = self.nodes.get_mut(&child) {
                c.set_parent(Some(new_node));
            }
            if let Some(n) = self.nodes.get_mut(&new_node) {
                if !n.children().contains(&child) {
                    n.children_mut().push(child);
                }
            }
        }
        if let Some(n) = self.nodes.get_mut(&old) {
            n.children_mut().clear();
        }
        for group in groups {
            let _ = self.add_to_group(new_node, &group);
        }

        // Splice `new_node` into `old`'s slot, then drop `old`. Detaching the
        // replacement from wherever it was first keeps the child list free of
        // duplicates when it was already a sibling.
        if let Some(prev_parent) = self.nodes.get(&new_node).and_then(|n| n.parent()) {
            if let Some(p) = self.nodes.get_mut(&prev_parent) {
                p.children_mut().retain(|c| *c != new_node);
            }
        }
        if let Some(p) = self.nodes.get_mut(&parent) {
            p.children_mut().retain(|c| *c != new_node);
            let at = index.min(p.children().len());
            p.children_mut().insert(at, new_node);
            p.children_mut().retain(|c| *c != old);
        }
        if let Some(n) = self.nodes.get_mut(&new_node) {
            n.set_parent(Some(parent));
        }
        self.nodes.remove(&old);
        true
    }

    /// Resolves a `NodePath` with a `:subname` chain into the node it names and
    /// the property value at that subname (Godot
    /// `Node.get_node_and_resource`).
    ///
    /// `"Sprite:texture"` yields the `Sprite` node and its `texture` value;
    /// a path with no `:` yields the node and `Variant::Nil`, which is what
    /// Godot returns when the path names no resource.
    ///
    /// ⚠️ Only the FIRST subname is resolved. Godot returns the remaining path
    /// as a third element for deeper chains (`"Sprite:texture:size"`); that
    /// third element is not modelled here — see the completion report.
    pub fn get_node_and_resource(&self, from: NodeId, path: &str) -> Option<(NodeId, Variant)> {
        let (node_path, subname) = match path.split_once(':') {
            Some((n, s)) => (n, Some(s)),
            None => (path, None),
        };
        // An empty node part means "this node", as `":texture"` does in Godot.
        let target = if node_path.is_empty() {
            from
        } else {
            self.get_node_or_null(from, node_path)?
        };
        let value = match subname {
            // Only the first component; a deeper chain resolves its head.
            Some(s) => {
                let first = s.split(':').next().unwrap_or(s);
                self.nodes
                    .get(&target)
                    .map(|n| n.get_property(first))
                    .unwrap_or(Variant::Nil)
            }
            None => Variant::Nil,
        };
        Some((target, value))
    }

    /// The relative `NodePath` from `from` to `to` (Godot `Node.get_path_to`).
    ///
    /// `".."` for each level up to the common ancestor, then the names down to
    /// the target: a sibling is `"../Other"`, a descendant is `"Child/Grand"`,
    /// and a node's path to itself is `"."`.
    ///
    /// `None` when the two are in different trees — Godot errors there, and a
    /// silently wrong path would be worse than no path.
    pub fn get_path_to(&self, from: NodeId, to: NodeId) -> Option<String> {
        if from == to {
            return Some(".".to_string());
        }
        // Root-to-self chains, root first.
        let chain = |mut id: NodeId| -> Vec<NodeId> {
            let mut out = vec![id];
            while let Some(p) = self.nodes.get(&id).and_then(|n| n.parent()) {
                out.push(p);
                id = p;
            }
            out.reverse();
            out
        };
        let (a, b) = (chain(from), chain(to));
        if a.first() != b.first() {
            return None;
        }
        // Longest shared prefix ends at the common ancestor.
        let shared = a.iter().zip(b.iter()).take_while(|(x, y)| x == y).count();
        let mut parts: Vec<String> = vec!["..".to_string(); a.len() - shared];
        for id in &b[shared..] {
            parts.push(
                self.nodes
                    .get(id)
                    .map(|n| n.name().to_string())
                    .unwrap_or_default(),
            );
        }
        Some(parts.join("/"))
    }

    /// Whether `a` comes after `b` in tree order (Godot `Node.is_greater_than`).
    ///
    /// "Tree order" is the depth-first walk the scene uses for processing and
    /// drawing, so this answers "does `a` run after `b`". Unknown nodes are
    /// `false` rather than a panic, matching the rest of this API.
    pub fn is_greater_than(&self, a: NodeId, b: NodeId) -> bool {
        let order = self.all_nodes_in_tree_order();
        let (ia, ib) = (
            order.iter().position(|n| *n == a),
            order.iter().position(|n| *n == b),
        );
        match (ia, ib) {
            (Some(x), Some(y)) => x > y,
            _ => false,
        }
    }

    /// The subtree as newline-separated paths relative to `root`
    /// (Godot `Node.get_tree_string`).
    ///
    /// The root itself is `"."`, matching [`Self::get_path_to`], so every line
    /// is a path you can feed straight back to `get_node`.
    pub fn get_tree_string(&self, root: NodeId) -> String {
        let mut out = vec![".".to_string()];
        for id in self.descendants(root) {
            if let Some(p) = self.get_path_to(root, id) {
                out.push(p);
            }
        }
        out.join("\n")
    }

    /// The subtree drawn with box characters (Godot
    /// `Node.get_tree_string_pretty`).
    ///
    /// ⚠️ The prefix bookkeeping is the whole job: a child of a *last* sibling
    /// continues under blank space, a child of any other sibling continues under
    /// `"│  "`. Getting that wrong is what makes hand-rolled tree printers show
    /// stray trailing pipes.
    pub fn get_tree_string_pretty(&self, root: NodeId) -> String {
        let mut out = String::new();
        let name = self
            .nodes
            .get(&root)
            .map(|n| n.name().to_string())
            .unwrap_or_default();
        out.push_str(&name);
        out.push('\n');
        self.pretty_children(root, "", &mut out);
        out
    }

    fn pretty_children(&self, parent: NodeId, prefix: &str, out: &mut String) {
        let children: Vec<NodeId> = match self.nodes.get(&parent) {
            Some(n) => n.children().to_vec(),
            None => return,
        };
        let last = children.len().saturating_sub(1);
        for (i, child) in children.iter().enumerate() {
            let is_last = i == last;
            out.push_str(prefix);
            out.push_str(if is_last { "┗╸" } else { "┣╸" });
            out.push_str(
                &self
                    .nodes
                    .get(child)
                    .map(|n| n.name().to_string())
                    .unwrap_or_default(),
            );
            out.push('\n');
            let next = format!("{prefix}{}", if is_last { "  " } else { "┃ " });
            self.pretty_children(*child, &next, out);
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
                opts: crate::animation_tree::AnimationLeafOptions::default(),
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

        /// pat-s3rrv.23: an AnimationTree honours `callback_mode_process`.
        ///
        /// `AnimationTree` extends `AnimationMixer`, where the property lives,
        /// so a tree picks its phase exactly as an `AnimationPlayer` does.
        /// Patina filtered players by mode and ran trees unconditionally in the
        /// idle phase — a tree set to PHYSICS sampled once per rendered frame
        /// instead of once per tick, so anything it drove drifted with the
        /// frame rate instead of tracking the simulation.
        ///
        /// ⚠️ Asserted in BOTH directions. Only checking that a PHYSICS tree
        /// advances in the physics phase would still pass if the filter were
        /// missing entirely and every tree ran in every phase — which is the
        /// bug. The idle-phase assertion below is what makes the filter real.
        #[test]
        fn animation_trees_respect_their_callback_mode() {
            use crate::animation::AnimationCallbackMode;

            // A tree left at the default (IDLE) advances in the idle phase…
            let (mut tree, sprite, tree_node, anim_tree) = tree_with_animation_tree(1.0);
            tree.attach_animation_tree(tree_node, anim_tree).unwrap();
            tree.process_animation_trees_in_mode(0.5, AnimationCallbackMode::Idle);
            let idle_x = match tree.get_node(sprite).unwrap().get_property("x") {
                Variant::Float(x) => x,
                other => panic!("Sprite:x not driven: {other:?}"),
            };
            assert!(
                idle_x > 1.0,
                "a default-mode tree must advance in the IDLE phase, x={idle_x}",
            );

            // …and does NOT advance in the physics phase.
            let (mut tree, sprite, tree_node, anim_tree) = tree_with_animation_tree(1.0);
            tree.attach_animation_tree(tree_node, anim_tree).unwrap();
            tree.process_animation_trees_in_mode(0.5, AnimationCallbackMode::Physics);
            assert!(
                !matches!(
                    tree.get_node(sprite).unwrap().get_property("x"),
                    Variant::Float(x) if x > 1.0
                ),
                "an IDLE tree must not advance in the PHYSICS phase — if it \
                 does, the mode filter is absent and every tree runs in every \
                 phase, which is the bug this pins",
            );

            // A tree marked PHYSICS is the mirror image.
            let (mut tree, sprite, tree_node, anim_tree) = tree_with_animation_tree(1.0);
            tree.attach_animation_tree(tree_node, anim_tree).unwrap();
            if let Some(n) = tree.get_node_mut(tree_node) {
                // AnimationCallbackMode::Physics == 0 in Godot's enum
                // (ANIMATION_CALLBACK_MODE_PROCESS_PHYSICS).
                n.set_property("callback_mode_process", Variant::Int(0));
            }
            tree.process_animation_trees_in_mode(0.5, AnimationCallbackMode::Physics);
            match tree.get_node(sprite).unwrap().get_property("x") {
                Variant::Float(x) => assert!(
                    x > 1.0,
                    "a PHYSICS tree must advance in the physics phase, x={x}",
                ),
                other => panic!("Sprite:x not driven: {other:?}"),
            }
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
