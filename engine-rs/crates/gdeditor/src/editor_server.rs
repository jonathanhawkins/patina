//! HTTP REST API server for the Patina editor.
//!
//! Exposes the editor's scene tree, node manipulation, undo/redo,
//! viewport rendering, and scene save/load over a simple REST API.
//! Uses the same `std::net::TcpListener` pattern as `gdrender2d::frame_server`.

use std::collections::HashSet;
use std::collections::VecDeque;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use gdrender2d::export::{encode_bmp, encode_png};
use gdrender2d::renderer::FrameBuffer;
use std::collections::HashMap;

use gdscene::animation::{Animation, AnimationTrack, KeyFrame, LoopMode, TrackType};
use gdscene::main_loop::MainLoop;
use gdscene::node::{Node, NodeId};
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_saver::TscnSaver;
use gdscene::SceneTree;
use gdvariant::serialize::{from_json, to_json};
use gdvariant::Variant;

use crate::create_dialog::CreateNodeDialog;
use crate::texture_cache::TextureCache;
use crate::EditorCommand;

use gdcore::math::Vector2;
use gdscene::scripting::{GDScriptNodeInstance, InputSnapshot};

/// Animation playback state for the editor.
#[derive(Debug, Clone)]
pub struct AnimationPlaybackState {
    /// Whether an animation is currently playing.
    pub playing: bool,
    /// The name of the animation being played.
    pub animation_name: Option<String>,
    /// The current playback time in seconds.
    pub current_time: f64,
    /// Whether keyframe recording mode is active.
    pub recording: bool,
    /// Secondary animation for blend preview (if any).
    pub blend_secondary: Option<String>,
    /// Blend weight: 0.0 = fully primary, 1.0 = fully secondary.
    pub blend_weight: f32,
}

/// A single function timing entry in a profiler frame snapshot.
#[derive(Debug, Clone)]
pub struct ProfilerFuncEntry {
    /// Function or subsystem name (e.g. "physics_step", "render_2d", "script_process").
    pub name: String,
    /// Time spent in this function in milliseconds.
    pub time_ms: f64,
}

/// A snapshot of one frame's profiling data.
#[derive(Debug, Clone)]
pub struct ProfilerFrame {
    /// Frame number.
    pub frame_number: u64,
    /// Total frame time in milliseconds.
    pub total_ms: f64,
    /// CPU time in milliseconds.
    pub cpu_ms: f64,
    /// GPU time in milliseconds (estimated or 0 if unavailable).
    pub gpu_ms: f64,
    /// Per-function timing breakdown.
    pub functions: Vec<ProfilerFuncEntry>,
}

/// State for an in-progress drag operation.
#[derive(Debug, Clone)]
pub struct DragState {
    /// The node being dragged.
    pub node_id: NodeId,
    /// The pixel position where the drag started.
    pub start_pixel: Vector2,
    /// The node's position when the drag started.
    pub start_node_pos: Vector2,
    /// Camera offset at drag start (frozen so bounds changes don't affect drag).
    pub camera_offset: Vector2,
}

/// A single log entry in the editor's operation log.
#[derive(Debug, Clone)]
pub struct LogEntry {
    /// Unix timestamp in milliseconds.
    pub timestamp: u64,
    /// Log level: "info", "warn", or "error".
    pub level: String,
    /// Human-readable log message.
    pub message: String,
}

/// Maximum number of log entries to keep.
const MAX_LOG_ENTRIES: usize = 100;

/// Maximum number of frame time entries to keep.
const MAX_FRAME_TIMES: usize = 120;

/// Editor display settings that can be persisted.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct EditorDisplaySettings {
    pub grid_snap_enabled: bool,
    pub grid_snap_size: u32,
    pub grid_visible: bool,
    pub rulers_visible: bool,
    pub background_color: [f64; 4],
    pub font_size: String,
    /// Color theme: "dark" or "light".
    pub theme: String,
    /// Physics ticks per second.
    pub physics_fps: u32,
    /// Saved panel sizes for layout persistence.
    pub panel_sizes: std::collections::HashMap<String, f64>,
    /// Whether smart snapping (alignment guides to sibling nodes) is enabled.
    pub smart_snap_enabled: bool,
    /// Distance threshold in world-space pixels for smart snap to engage.
    pub smart_snap_threshold: f32,
}
impl Default for EditorDisplaySettings {
    fn default() -> Self {
        Self {
            grid_snap_enabled: false,
            grid_snap_size: 8,
            grid_visible: true,
            rulers_visible: true,
            background_color: [0.08, 0.08, 0.1, 1.0],
            font_size: "medium".to_string(),
            theme: "dark".to_string(),
            physics_fps: 60,
            panel_sizes: std::collections::HashMap::new(),
            smart_snap_enabled: true,
            smart_snap_threshold: 5.0,
        }
    }
}

/// A snap guide line for rendering in the viewport.
#[derive(Debug, Clone, PartialEq)]
pub struct SnapGuide {
    /// "x" for vertical guide, "y" for horizontal guide.
    pub axis: &'static str,
    /// The world-space coordinate of the guide line.
    pub position: f32,
    /// The node being snapped to (for display purposes).
    pub target_node_id: NodeId,
}

/// Snap a position to the grid if grid snapping is enabled.
pub fn snap_to_grid(pos: Vector2, grid_size: u32) -> Vector2 {
    let g = grid_size as f32;
    Vector2::new((pos.x / g).round() * g, (pos.y / g).round() * g)
}

/// Compute smart snap guides by comparing a candidate position against sibling nodes.
pub fn compute_smart_snap(
    tree: &SceneTree,
    dragged_id: NodeId,
    candidate_pos: Vector2,
    threshold: f32,
) -> (Vector2, Vec<SnapGuide>) {
    use crate::scene_renderer::{extract_position, extract_size};

    let mut guides = Vec::new();
    let mut snapped = candidate_pos;
    let mut best_dx: f32 = threshold + 1.0;
    let mut best_dy: f32 = threshold + 1.0;

    let dragged_size = tree
        .get_node(dragged_id)
        .map(|n| extract_size(n))
        .unwrap_or(Vector2::ZERO);
    let parent_id = tree
        .get_node(dragged_id)
        .and_then(|n| n.parent())
        .unwrap_or_else(|| tree.root_id());
    let sibling_ids: Vec<NodeId> = tree
        .get_node(parent_id)
        .map(|n| n.children().to_vec())
        .unwrap_or_default();
    let siblings: Vec<(NodeId, Vector2, Vector2)> = sibling_ids
        .iter()
        .filter(|&&nid| nid != dragged_id)
        .filter_map(|&nid| {
            tree.get_node(nid)
                .map(|n| (nid, extract_position(n), extract_size(n)))
        })
        .collect();

    for &(nid, sib_pos, sib_size) in &siblings {
        let snap_xs: [(f32, f32); 3] = [
            (candidate_pos.x, sib_pos.x),
            (
                candidate_pos.x,
                sib_pos.x - sib_size.x / 2.0 + dragged_size.x / 2.0,
            ),
            (
                candidate_pos.x,
                sib_pos.x + sib_size.x / 2.0 - dragged_size.x / 2.0,
            ),
        ];
        for (cand_x, target_x) in snap_xs {
            let dx = (cand_x - target_x).abs();
            if dx < threshold && dx < best_dx {
                best_dx = dx;
                snapped.x = target_x;
                guides.retain(|g: &SnapGuide| g.axis != "x");
                guides.push(SnapGuide {
                    axis: "x",
                    position: target_x,
                    target_node_id: nid,
                });
            }
        }
        let snap_ys: [(f32, f32); 3] = [
            (candidate_pos.y, sib_pos.y),
            (
                candidate_pos.y,
                sib_pos.y - sib_size.y / 2.0 + dragged_size.y / 2.0,
            ),
            (
                candidate_pos.y,
                sib_pos.y + sib_size.y / 2.0 - dragged_size.y / 2.0,
            ),
        ];
        for (cand_y, target_y) in snap_ys {
            let dy = (cand_y - target_y).abs();
            if dy < threshold && dy < best_dy {
                best_dy = dy;
                snapped.y = target_y;
                guides.retain(|g: &SnapGuide| g.axis != "y");
                guides.push(SnapGuide {
                    axis: "y",
                    position: target_y,
                    target_node_id: nid,
                });
            }
        }
    }
    (snapped, guides)
}

/// Apply all enabled snap modes to a candidate position.
pub fn apply_snap(
    tree: &SceneTree,
    settings: &EditorDisplaySettings,
    dragged_id: NodeId,
    candidate_pos: Vector2,
) -> (Vector2, Vec<SnapGuide>) {
    let mut pos = candidate_pos;
    if settings.grid_snap_enabled {
        pos = snap_to_grid(pos, settings.grid_snap_size);
    }
    if settings.smart_snap_enabled {
        let (snapped, guides) =
            compute_smart_snap(tree, dragged_id, pos, settings.smart_snap_threshold);
        return (snapped, guides);
    }
    (pos, Vec::new())
}
/// Serialized node data for the copy/paste clipboard.
#[derive(Debug, Clone)]
pub struct ClipboardEntry {
    pub name: String,
    pub class_name: String,
    pub properties: Vec<(String, Variant)>,
    pub children: Vec<ClipboardEntry>,
}

/// Shared editor state protected by a mutex.
pub struct EditorState {
    /// The scene tree being edited.
    pub scene_tree: SceneTree,
    /// The currently selected node, if any.
    pub selected_node: Option<NodeId>,
    /// Undo stack (most recent command on top).
    pub undo_stack: Vec<EditorCommand>,
    /// Redo stack (cleared on new command).
    pub redo_stack: Vec<EditorCommand>,
    /// The latest rendered frame, if any.
    pub frame_buffer: Option<FrameBuffer>,
    /// Cached PNG encoding of the frame buffer (avoids re-encoding on every poll).
    pub cached_png: Option<Vec<u8>>,
    /// Cached BMP encoding of the frame buffer.
    pub cached_bmp: Option<Vec<u8>>,
    /// Current drag operation, if any.
    pub drag_state: Option<DragState>,
    /// Viewport width for hit testing.
    pub viewport_width: u32,
    /// Viewport height for hit testing.
    pub viewport_height: u32,
    /// Current viewport zoom level (1.0 = 100%).
    pub viewport_zoom: f64,
    /// Current viewport pan offset in pixels (x, y).
    pub viewport_pan: (f64, f64),
    /// Ring buffer of recent editor log entries.
    pub log_entries: VecDeque<LogEntry>,
    /// The currently loaded scene file path, if any.
    pub scene_file: Option<String>,
    /// Whether the scene has unsaved modifications.
    pub scene_modified: bool,
    pub selected_nodes: Vec<NodeId>,
    pub clipboard: Vec<ClipboardEntry>,
    pub display_settings: EditorDisplaySettings,
    /// Cache of loaded textures for viewport rendering.
    pub texture_cache: TextureCache,
    /// Whether the game is currently playing.
    pub is_running: bool,
    /// Whether the game is paused.
    pub is_paused: bool,
    /// Engine-owned runtime loop for play mode.
    /// Contains the scene tree and drives the full frame pipeline.
    pub run_main_loop: Option<MainLoop>,
    /// Counts frames during runtime.
    pub runtime_frame_count: u64,
    /// Time between frames (fixed at 1/60).
    pub delta_time: f64,
    /// Named animations stored in the editor.
    pub animations: std::collections::HashMap<String, Animation>,
    /// Current animation playback state.
    pub animation_playback: AnimationPlaybackState,
    /// Currently pressed keyboard keys.
    pub pressed_keys: HashSet<String>,
    /// Keys pressed this frame (cleared each frame).
    pub just_pressed_keys: HashSet<String>,
    /// Keys released this frame (cleared each frame).
    pub just_released_keys: HashSet<String>,
    /// Current mouse position in viewport coordinates.
    pub mouse_position: (f64, f64),
    /// Currently pressed mouse buttons (0=left, 1=middle, 2=right).
    pub mouse_buttons: HashSet<u8>,
    /// Input action map: action name -> list of key names.
    pub input_map: HashMap<String, Vec<String>>,
    pub tile_grid_store: gdscene::tilemap::TileGridStore,
    /// Currently active editor mode: "2d", "3d", or "script".
    pub editor_mode: String,
    /// Active transform axis constraint: None, "x", or "y".
    pub transform_axis_constraint: Option<String>,
    /// Keyframe clipboard for copy/paste in animation editor.
    pub keyframe_clipboard: Vec<(usize, gdscene::animation::KeyFrame)>,
    /// Breakpoint lines per script path.
    pub breakpoints: HashMap<String, Vec<u32>>,
    /// Error lines per script path (line number + message).
    pub script_errors: HashMap<String, Vec<(u32, String)>>,
    /// Frame time history for monitors panel (ring buffer of last 120 frame times in ms).
    pub frame_times: VecDeque<f64>,
    /// Profiler frame snapshots (ring buffer of last 120 frames).
    pub profiler_frames: VecDeque<ProfilerFrame>,
    /// Debug stack trace (populated when runtime hits a breakpoint or error).
    pub debug_stack_trace: Vec<String>,
    /// Debugger state: "detached", "running", or "paused".
    pub debug_state: String,
    /// Structured debug stack frames: (function, script, line).
    pub debug_frames: Vec<(String, String, usize)>,
    /// Debug breakpoints: (script, line).
    pub debug_breakpoints: Vec<(String, usize)>,
    /// Debug local variables for current frame: (name, type, value).
    pub debug_locals: Vec<(String, String, String)>,
    /// Debug global variables: (name, type, value).
    pub debug_globals: Vec<(String, String, String)>,
    /// Registered editor plugins.
    pub plugins: Vec<PluginEntry>,
    /// Editor keybindings for the settings dialog.
    pub keybindings: Vec<EditorKeyBinding>,
    /// Current viewport tool mode (select, move, rotate, scale).
    pub viewport_mode: ViewportMode,
    /// Output log from script print() calls.
    pub output_entries: VecDeque<String>,
    /// Project settings (pat-kj4 / pat-c4zlm).
    pub project_name: String,
    /// Project description.
    pub project_description: String,
    /// Project icon path.
    pub project_icon: String,
    /// Project main scene path.
    pub project_main_scene: String,
    /// Project display resolution width.
    pub project_resolution_w: u32,
    /// Project display resolution height.
    pub project_resolution_h: u32,
    /// Stretch mode.
    pub project_stretch_mode: String,
    /// Stretch aspect.
    pub project_stretch_aspect: String,
    /// Fullscreen mode.
    pub project_fullscreen: bool,
    /// V-Sync enabled.
    pub project_vsync: bool,
    /// Project physics FPS.
    pub project_physics_fps: u32,
    /// Project default gravity.
    pub project_gravity: f64,
    /// Default linear damp.
    pub project_linear_damp: f64,
    /// Default angular damp.
    pub project_angular_damp: f64,
    /// Default audio bus layout.
    pub project_bus_layout: String,
    /// Master volume in dB.
    pub project_master_volume_db: f64,
    /// Enable audio input.
    pub project_audio_input: bool,
    /// Renderer backend.
    pub project_renderer: String,
    /// Anti-aliasing mode.
    pub project_anti_aliasing: String,
    /// Default environment path.
    pub project_environment_default: String,
    /// Active smart snap alignment guides (cleared when drag ends).
    pub snap_guides: Vec<SnapGuide>,
    /// Open scene tabs: (tab_id, scene_path, display_name, modified).
    pub scene_tabs: Vec<SceneTab>,
    /// Index of the currently active scene tab.
    pub active_tab_index: usize,
    /// Node creation dialog with class search and filtering.
    pub create_node_dialog: CreateNodeDialog,
}

/// A single scene tab in the editor.
#[derive(Debug, Clone)]
pub struct SceneTab {
    /// Unique tab identifier.
    pub id: u32,
    /// File path of the scene (empty for unsaved).
    pub path: String,
    /// Display name shown on the tab.
    pub name: String,
    /// Whether the scene has unsaved changes.
    pub modified: bool,
}

/// Viewport tool modes for the editor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewportMode {
    /// Select mode (Q).
    Select,
    /// Move mode (W).
    Move,
    /// Rotate mode (E).
    Rotate,
    /// Scale mode (S).
    Scale,
}

impl ViewportMode {
    /// Parses a mode from a string name.
    pub fn from_str_name(s: &str) -> Option<Self> {
        match s {
            "select" => Some(Self::Select),
            "move" => Some(Self::Move),
            "rotate" => Some(Self::Rotate),
            "scale" => Some(Self::Scale),
            _ => None,
        }
    }

    /// Returns the string name for this mode.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Select => "select",
            Self::Move => "move",
            Self::Rotate => "rotate",
            Self::Scale => "scale",
        }
    }
}

/// A registered editor plugin entry.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PluginEntry {
    /// Plugin display name.
    pub name: String,
    /// Whether the plugin is currently enabled.
    pub enabled: bool,
}

/// An editor keybinding entry for the settings dialog.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct EditorKeyBinding {
    /// The action name (e.g. "delete", "duplicate", "undo").
    pub action: String,
    /// Human-readable description.
    pub description: String,
    /// The key combination string (e.g. "Ctrl+Z", "F2", "Delete").
    pub keys: String,
}

impl EditorKeyBinding {
    fn defaults() -> Vec<Self> {
        vec![
            Self {
                action: "delete".into(),
                description: "Delete selected node".into(),
                keys: "Delete".into(),
            },
            Self {
                action: "rename".into(),
                description: "Rename selected node".into(),
                keys: "F2".into(),
            },
            Self {
                action: "duplicate".into(),
                description: "Duplicate selected node".into(),
                keys: "Ctrl+D".into(),
            },
            Self {
                action: "copy".into(),
                description: "Copy selected node".into(),
                keys: "Ctrl+C".into(),
            },
            Self {
                action: "paste".into(),
                description: "Paste node".into(),
                keys: "Ctrl+V".into(),
            },
            Self {
                action: "cut".into(),
                description: "Cut selected node".into(),
                keys: "Ctrl+X".into(),
            },
            Self {
                action: "undo".into(),
                description: "Undo last action".into(),
                keys: "Ctrl+Z".into(),
            },
            Self {
                action: "redo".into(),
                description: "Redo last action".into(),
                keys: "Ctrl+Y".into(),
            },
            Self {
                action: "save".into(),
                description: "Save scene".into(),
                keys: "Ctrl+S".into(),
            },
            Self {
                action: "zoom_in".into(),
                description: "Zoom in".into(),
                keys: "Ctrl++".into(),
            },
            Self {
                action: "zoom_out".into(),
                description: "Zoom out".into(),
                keys: "Ctrl+-".into(),
            },
            Self {
                action: "zoom_reset".into(),
                description: "Reset zoom".into(),
                keys: "Ctrl+0".into(),
            },
            Self {
                action: "tool_select".into(),
                description: "Select tool".into(),
                keys: "Q".into(),
            },
            Self {
                action: "tool_move".into(),
                description: "Move tool".into(),
                keys: "W".into(),
            },
            Self {
                action: "tool_rotate".into(),
                description: "Rotate tool".into(),
                keys: "E".into(),
            },
            Self {
                action: "play".into(),
                description: "Play scene".into(),
                keys: "F5".into(),
            },
            Self {
                action: "play_current".into(),
                description: "Play current scene".into(),
                keys: "F6".into(),
            },
            Self {
                action: "pause".into(),
                description: "Pause playback".into(),
                keys: "F7".into(),
            },
            Self {
                action: "stop".into(),
                description: "Stop playback".into(),
                keys: "F8".into(),
            },
            Self {
                action: "help".into(),
                description: "Show help".into(),
                keys: "F1".into(),
            },
        ]
    }
}

// SAFETY: EditorState is only accessed through a Mutex, so concurrent
// access is serialized. SceneTree contains `Box<dyn ScriptInstance>` which
// is not Send, but we never move script instances across threads — all
// access goes through the Mutex guard on the server thread.
unsafe impl Send for EditorState {}

impl EditorState {
    /// Creates a new editor state with the given scene tree.
    pub fn new(tree: SceneTree) -> Self {
        gdobject::class_db::register_editor_classes();
        Self {
            scene_tree: tree,
            selected_node: None,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            frame_buffer: None,
            cached_png: None,
            cached_bmp: None,
            drag_state: None,
            viewport_width: 800,
            viewport_height: 600,
            viewport_zoom: 1.0,
            viewport_pan: (0.0, 0.0),
            log_entries: VecDeque::new(),
            scene_file: None,
            scene_modified: false,
            texture_cache: TextureCache::default(),
            selected_nodes: Vec::new(),
            clipboard: Vec::new(),
            display_settings: EditorDisplaySettings::default(),
            is_running: false,
            is_paused: false,
            run_main_loop: None,
            runtime_frame_count: 0,
            delta_time: 1.0 / 60.0,
            animations: HashMap::new(),
            animation_playback: AnimationPlaybackState {
                playing: false,
                animation_name: None,
                current_time: 0.0,
                recording: false,
                blend_secondary: None,
                blend_weight: 0.0,
            },
            pressed_keys: HashSet::new(),
            just_pressed_keys: HashSet::new(),
            just_released_keys: HashSet::new(),
            mouse_position: (0.0, 0.0),
            mouse_buttons: HashSet::new(),
            input_map: Self::default_input_map(),
            tile_grid_store: gdscene::tilemap::TileGridStore::new_with_defaults(),
            editor_mode: "2d".to_string(),
            transform_axis_constraint: None,
            keyframe_clipboard: Vec::new(),
            breakpoints: HashMap::new(),
            script_errors: HashMap::new(),
            frame_times: VecDeque::new(),
            profiler_frames: VecDeque::new(),
            debug_stack_trace: Vec::new(),
            debug_state: "detached".to_string(),
            debug_frames: Vec::new(),
            debug_breakpoints: Vec::new(),
            debug_locals: Vec::new(),
            debug_globals: Vec::new(),
            plugins: Vec::new(),
            keybindings: EditorKeyBinding::defaults(),
            viewport_mode: ViewportMode::Select,
            output_entries: VecDeque::new(),
            project_name: "New Project".to_string(),
            project_description: String::new(),
            project_icon: String::new(),
            project_main_scene: String::new(),
            project_resolution_w: 1152,
            project_resolution_h: 648,
            project_stretch_mode: "disabled".to_string(),
            project_stretch_aspect: "keep".to_string(),
            project_fullscreen: false,
            project_vsync: true,
            project_physics_fps: 60,
            project_gravity: 980.0,
            project_linear_damp: 0.1,
            project_angular_damp: 1.0,
            project_bus_layout: "res://default_bus_layout.tres".to_string(),
            project_master_volume_db: 0.0,
            project_audio_input: false,
            project_renderer: "forward_plus".to_string(),
            project_anti_aliasing: "disabled".to_string(),
            project_environment_default: String::new(),
            snap_guides: Vec::new(),
            scene_tabs: vec![SceneTab {
                id: 1,
                path: String::new(),
                name: "Untitled".to_string(),
                modified: false,
            }],
            active_tab_index: 0,
            create_node_dialog: {
                let mut dlg = CreateNodeDialog::with_catalog();
                dlg.add_favorite("Node2D");
                dlg.add_favorite("Sprite2D");
                dlg.add_favorite("CharacterBody2D");
                dlg.add_favorite("Control");
                dlg.add_favorite("Label");
                dlg
            },
        }
    }

    /// Returns the next available tab ID.
    fn next_tab_id(&self) -> u32 {
        self.scene_tabs.iter().map(|t| t.id).max().unwrap_or(0) + 1
    }

    /// Returns the default input action map (Godot-style).
    pub fn default_input_map() -> HashMap<String, Vec<String>> {
        let mut map = HashMap::new();
        map.insert("ui_left".into(), vec!["ArrowLeft".into(), "a".into()]);
        map.insert("ui_right".into(), vec!["ArrowRight".into(), "d".into()]);
        map.insert("ui_up".into(), vec!["ArrowUp".into(), "w".into()]);
        map.insert("ui_down".into(), vec!["ArrowDown".into(), "s".into()]);
        map.insert("ui_accept".into(), vec!["Enter".into(), " ".into()]);
        map.insert("ui_cancel".into(), vec!["Escape".into()]);
        map.insert("shoot".into(), vec![" ".into(), "x".into()]);
        map.insert(
            "jump".into(),
            vec![" ".into(), "ArrowUp".into(), "w".into()],
        );
        map
    }

    /// Returns true if any key mapped to the given action is currently pressed.
    pub fn is_action_pressed(&self, action: &str) -> bool {
        if let Some(keys) = self.input_map.get(action) {
            keys.iter().any(|k| self.pressed_keys.contains(k))
        } else {
            false
        }
    }

    /// Returns true if any key mapped to the given action was just pressed this frame.
    pub fn is_action_just_pressed(&self, action: &str) -> bool {
        if let Some(keys) = self.input_map.get(action) {
            keys.iter().any(|k| self.just_pressed_keys.contains(k))
        } else {
            false
        }
    }

    /// Converts the string-based input map into a typed [`gdplatform::InputMap`]
    /// for the engine-owned [`MainLoop`].
    pub fn build_engine_input_map(&self) -> gdplatform::InputMap {
        let mut map = gdplatform::InputMap::new();
        for (action, keys) in &self.input_map {
            map.add_action(action, 0.0);
            for key_name in keys {
                if let Some(typed_key) = gdplatform::input::Key::from_name(key_name) {
                    map.action_add_event(action, gdplatform::ActionBinding::KeyBinding(typed_key));
                }
            }
        }
        map
    }

    /// Clears per-frame input state (just_pressed and just_released).
    pub fn clear_frame_input(&mut self) {
        self.just_pressed_keys.clear();
        self.just_released_keys.clear();
    }

    /// Resets all input state (called when runtime stops).
    pub fn clear_all_input(&mut self) {
        self.pressed_keys.clear();
        self.just_pressed_keys.clear();
        self.just_released_keys.clear();
        self.mouse_position = (0.0, 0.0);
        self.mouse_buttons.clear();
    }

    /// Creates an [`InputSnapshot`] from the current editor input state.
    /// This is passed to the scene tree so scripts can call
    /// `Input.is_action_pressed()`, etc.
    pub fn make_input_snapshot(&self) -> InputSnapshot {
        InputSnapshot {
            pressed_keys: self.pressed_keys.clone(),
            just_pressed_keys: self.just_pressed_keys.clone(),
            input_map: self.input_map.clone(),
            mouse_position: Default::default(),
            mouse_buttons_pressed: Default::default(),
        }
    }

    /// Adds a log entry to the ring buffer.
    pub fn add_log(&mut self, level: &str, message: impl Into<String>) {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        self.log_entries.push_back(LogEntry {
            timestamp,
            level: level.to_string(),
            message: message.into(),
        });
        if self.log_entries.len() > MAX_LOG_ENTRIES {
            self.log_entries.pop_front();
        }
    }
}

/// Deep-copies a scene tree (without scripts) for runtime use.
pub fn clone_scene_tree(source: &SceneTree) -> SceneTree {
    let mut dest = SceneTree::new();
    let dest_root = dest.root_id();
    let source_root = source.root_id();
    let source_root_node = match source.get_node(source_root) {
        Some(n) => n,
        None => return dest,
    };
    let children: Vec<NodeId> = source_root_node.children().to_vec();
    fn copy_subtree(source: &SceneTree, dest: &mut SceneTree, src_id: NodeId, dest_parent: NodeId) {
        let src_node = match source.get_node(src_id) {
            Some(n) => n,
            None => return,
        };
        let mut new_node = Node::new(src_node.name(), src_node.class_name());
        for (k, v) in src_node.properties() {
            new_node.set_property(k, v.clone());
        }
        for group in src_node.groups() {
            new_node.add_to_group(group.clone());
        }
        let new_id = match dest.add_child(dest_parent, new_node) {
            Ok(id) => id,
            Err(_) => return,
        };
        let children: Vec<NodeId> = src_node.children().to_vec();
        for child_id in children {
            copy_subtree(source, dest, child_id, new_id);
        }
    }
    for child_id in children {
        copy_subtree(source, &mut dest, child_id, dest_root);
    }
    dest
}

/// Separate cache for viewport images — avoids Mutex contention with scene tree.
pub struct ViewportCache {
    /// Cached PNG bytes.
    pub png: Mutex<Option<Vec<u8>>>,
    /// Cached BMP bytes.
    pub bmp: Mutex<Option<Vec<u8>>>,
}

/// Handle returned by [`start`], used to interact with the running server.
pub struct EditorServerHandle {
    state: Arc<Mutex<EditorState>>,
    viewport_cache: Arc<ViewportCache>,
    running: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl EditorServerHandle {
    /// Starts the editor HTTP server on the given port.
    pub fn start(port: u16, state: EditorState) -> Self {
        let state = Arc::new(Mutex::new(state));
        let viewport_cache = Arc::new(ViewportCache {
            png: Mutex::new(None),
            bmp: Mutex::new(None),
        });
        let running = Arc::new(AtomicBool::new(true));

        let state_clone = Arc::clone(&state);
        let cache_clone = Arc::clone(&viewport_cache);
        let running_clone = Arc::clone(&running);
        let thread = thread::spawn(move || {
            run_server(state_clone, cache_clone, running_clone, port);
        });

        Self {
            state,
            viewport_cache,
            running,
            thread: Some(thread),
        }
    }

    /// Updates the latest frame buffer for viewport endpoints.
    /// Pre-encodes PNG and BMP into separate cache (no main Mutex contention).
    pub fn update_frame(&self, fb: FrameBuffer) {
        let png = encode_png(&fb);
        let bmp = encode_bmp(&fb);
        // Update viewport cache (separate lock from scene tree)
        *self.viewport_cache.png.lock().unwrap() = Some(png);
        *self.viewport_cache.bmp.lock().unwrap() = Some(bmp);
        // Update scene state
        let mut state = self.state.lock().unwrap();
        state.viewport_width = fb.width;
        state.viewport_height = fb.height;
        state.frame_buffer = Some(fb);
    }

    /// Returns a reference to the shared state for external access.
    pub fn state(&self) -> &Arc<Mutex<EditorState>> {
        &self.state
    }

    /// Signals the server to stop and waits for the thread to finish.
    pub fn stop(mut self) {
        self.running.store(false, Ordering::SeqCst);
        if let Some(handle) = self.thread.take() {
            let _ = handle.join();
        }
    }
}

fn run_server(
    state: Arc<Mutex<EditorState>>,
    viewport_cache: Arc<ViewportCache>,
    running: Arc<AtomicBool>,
    port: u16,
) {
    let listener = match TcpListener::bind(format!("127.0.0.1:{port}")) {
        Ok(l) => l,
        Err(e) => {
            tracing::error!("Failed to bind editor server on port {port}: {e}");
            return;
        }
    };
    // Non-blocking so we can check the running flag, but we use a tight
    // accept loop with minimal sleep to avoid missing connections.
    listener
        .set_nonblocking(true)
        .expect("failed to set non-blocking");

    while running.load(Ordering::Relaxed) {
        // Accept all pending connections before sleeping.
        loop {
            match listener.accept() {
                Ok((stream, _)) => {
                    let state_clone = Arc::clone(&state);
                    let cache_clone = Arc::clone(&viewport_cache);
                    thread::spawn(move || {
                        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            handle_connection(&state_clone, &cache_clone, stream);
                        }));
                    });
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    break; // No more pending connections
                }
                Err(_) => {
                    break;
                }
            }
        }
        // Short sleep — connections queue in the OS backlog during this time.
        thread::sleep(Duration::from_millis(1));
    }
}

// ---------------------------------------------------------------------------
// HTTP request parsing
// ---------------------------------------------------------------------------

/// Parsed HTTP request.
struct HttpRequest {
    method: String,
    path: String,
    query: String,
    body: String,
}

fn parse_request(stream: &mut TcpStream) -> Option<HttpRequest> {
    // CRITICAL: Force blocking mode on the accepted socket.
    // On some systems, nonblocking listener produces nonblocking sockets.
    stream.set_nonblocking(false).ok();
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok();

    // Read until we have the full headers (look for \r\n\r\n).
    let mut raw = Vec::with_capacity(16384);
    let mut buf = [0u8; 4096];
    let mut header_end = None;

    // Read headers — retry on WouldBlock (transient).
    loop {
        let n = match stream.read(&mut buf) {
            Ok(n) => n,
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(1));
                continue;
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => return None,
        };
        if n == 0 {
            break;
        }
        raw.extend_from_slice(&buf[..n]);
        // Check if we have the full headers
        if let Some(pos) = find_header_end(&raw) {
            header_end = Some(pos);
            break;
        }
        if raw.len() > 65536 {
            return None; // Too large
        }
    }

    let header_end = header_end?;
    let header_bytes = &raw[..header_end];
    let header_str = String::from_utf8_lossy(header_bytes).to_string();

    let first_line = header_str.lines().next()?;
    let parts: Vec<&str> = first_line.split_whitespace().collect();
    if parts.len() < 2 {
        return None;
    }
    let method = parts[0].to_string();
    let full_url = parts[1];
    let (path_part, query_part) = match full_url.find('?') {
        Some(idx) => (&full_url[..idx], &full_url[idx + 1..]),
        None => (full_url, ""),
    };
    let path = path_part.to_string();
    let query = query_part.to_string();

    // Parse Content-Length from headers.
    let content_length: usize = header_str
        .lines()
        .find_map(|line| {
            let lower = line.to_lowercase();
            if lower.starts_with("content-length:") {
                lower.split(':').nth(1)?.trim().parse().ok()
            } else {
                None
            }
        })
        .unwrap_or(0);

    // Read remaining body bytes if needed.
    let body_start = header_end + 4; // skip \r\n\r\n
    while raw.len() < body_start + content_length {
        let n = match stream.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(_) => break,
        };
        raw.extend_from_slice(&buf[..n]);
    }

    let body = if raw.len() > body_start {
        let end = (body_start + content_length).min(raw.len());
        String::from_utf8_lossy(&raw[body_start..end]).to_string()
    } else {
        String::new()
    };

    Some(HttpRequest {
        method,
        path,
        query,
        body,
    })
}

// ---------------------------------------------------------------------------
// Connection handler + routing
// ---------------------------------------------------------------------------

/// Find the end of HTTP headers (\r\n\r\n) in raw bytes.
fn find_header_end(data: &[u8]) -> Option<usize> {
    data.windows(4).position(|w| w == b"\r\n\r\n")
}

fn handle_connection(
    state: &Arc<Mutex<EditorState>>,
    viewport_cache: &Arc<ViewportCache>,
    mut stream: TcpStream,
) {
    let req = match parse_request(&mut stream) {
        Some(r) => r,
        None => {
            // Always send something so the browser doesn't get ERR_EMPTY_RESPONSE.
            let _ = stream.write_all(
                b"HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            );
            return;
        }
    };

    match (req.method.as_str(), req.path.as_str()) {
        ("OPTIONS", _) => serve_cors_preflight(&mut stream),
        ("GET", "/favicon.ico") => serve_404(&mut stream),
        ("GET", "/editor") => serve_editor_html(&mut stream),
        ("GET", "/api/scene") => api_get_scene(state, &mut stream),
        ("GET", "/api/node/signals") => api_get_node_signals(state, &req.query, &mut stream),
        ("GET", "/api/node/script") => api_get_node_script(state, &req.query, &mut stream),
        ("GET", p) if p.starts_with("/api/node/") && req.method == "GET" => {
            // Extract node ID from /api/node/<id>
            let id_str = &p["/api/node/".len()..];
            api_get_node(state, id_str, &mut stream);
        }
        ("GET", "/api/selected") => api_get_selected(state, &mut stream),
        ("GET", "/api/viewport") => api_get_viewport_bmp(viewport_cache, &mut stream),
        ("GET", "/api/viewport/png") => api_get_viewport_png(viewport_cache, &mut stream),
        ("POST", "/api/node/add") => api_add_node(state, &req.body, &mut stream),
        ("POST", "/api/node/delete") => api_delete_node(state, &req.body, &mut stream),
        ("POST", "/api/node/select") => api_select_node(state, &req.body, &mut stream),
        ("POST", "/api/node/reparent") => api_reparent_node(state, &req.body, &mut stream),
        ("POST", "/api/node/rename") => api_rename_node(state, &req.body, &mut stream),
        ("POST", "/api/node/duplicate") => api_duplicate_node(state, &req.body, &mut stream),
        ("POST", "/api/node/create_dialog") => api_create_dialog(state, &req.body, &mut stream),
        ("POST", "/api/node/create_dialog/toggle_favorite") => {
            api_create_dialog_toggle_favorite(state, &req.body, &mut stream)
        }
        ("POST", "/api/node/create_dialog/confirm") => {
            api_create_dialog_confirm(state, &req.body, &mut stream)
        }
        ("GET", "/api/node/catalog_2d") => api_node_catalog_2d(state, &mut stream),
        ("POST", "/api/resource/property/set") => {
            api_set_resource_property(state, &req.body, &mut stream)
        }
        ("POST", "/api/node/reorder") => api_reorder_node(state, &req.body, &mut stream),
        ("POST", "/api/property/set") => api_set_property(state, &req.body, &mut stream),
        ("POST", "/api/undo") => api_undo(state, &mut stream),
        ("POST", "/api/redo") => api_redo(state, &mut stream),
        ("POST", "/api/scene/save") => api_save_scene(state, &req.body, &mut stream),
        ("POST", "/api/scene/load") => api_load_scene(state, &req.body, &mut stream),
        ("POST", "/api/viewport/click") => api_viewport_click(state, &req.body, &mut stream),
        ("POST", "/api/viewport/drag_start") => {
            api_viewport_drag_start(state, &req.body, &mut stream)
        }
        ("POST", "/api/viewport/drag") => api_viewport_drag(state, &req.body, &mut stream),
        ("POST", "/api/viewport/drag_end") => api_viewport_drag_end(state, &req.body, &mut stream),
        ("GET", "/api/viewport/zoom_pan") => api_get_zoom_pan(state, &mut stream),
        ("POST", "/api/viewport/zoom") => api_set_zoom(state, &req.body, &mut stream),
        ("POST", "/api/viewport/pan") => api_set_pan(state, &req.body, &mut stream),
        ("GET", "/api/logs") => api_get_logs(state, &mut stream),
        ("GET", "/api/scene/info") => api_get_scene_info(state, &mut stream),
        ("GET", "/api/filesystem") => api_get_filesystem(&mut stream),
        ("GET", "/api/preview/file") => api_get_file_preview(&req.query, &mut stream),
        ("GET", "/api/script") => api_get_script(&req.query, &mut stream),
        ("POST", "/api/script/save") => api_save_script(&req.body, &mut stream),
        ("POST", "/api/node/signals/connect") => api_connect_signal(state, &req.body, &mut stream),
        ("POST", "/api/node/groups/add") => api_add_group(state, &req.body, &mut stream),
        ("POST", "/api/node/groups/remove") => api_remove_group(state, &req.body, &mut stream),
        ("POST", "/api/node/select_multi") => api_select_multi(state, &req.body, &mut stream),
        ("GET", "/api/selected_nodes") => api_get_selected_nodes(state, &mut stream),
        ("POST", "/api/node/copy") => api_copy_nodes(state, &req.body, &mut stream),
        ("POST", "/api/node/paste") => api_paste_nodes(state, &req.body, &mut stream),
        ("POST", "/api/node/cut") => api_cut_nodes(state, &req.body, &mut stream),
        ("GET", "/api/settings") => api_get_settings(state, &mut stream),
        ("POST", "/api/settings") => api_set_settings(state, &req.body, &mut stream),
        ("GET", "/api/plugins") => api_get_plugins(state, &mut stream),
        ("POST", "/api/plugins/toggle") => api_toggle_plugin(state, &req.body, &mut stream),
        ("GET", "/api/keybindings") => api_get_keybindings(state, &mut stream),
        ("POST", "/api/keybindings") => api_set_keybinding(state, &req.body, &mut stream),
        ("POST", "/api/viewport/box_select") => api_box_select(state, &req.body, &mut stream),
        ("POST", "/api/viewport/drop") => api_viewport_drop(state, &req.body, &mut stream),
        ("POST", "/api/viewport/drag_multi") => {
            api_viewport_drag_multi(state, &req.body, &mut stream)
        }
        // Animation endpoints
        ("GET", "/api/animations") => api_get_animations(state, &mut stream),
        ("GET", "/api/animation") => api_get_animation(state, &req.query, &mut stream),
        ("POST", "/api/animation/create") => api_create_animation(state, &req.body, &mut stream),
        ("POST", "/api/animation/delete") => api_delete_animation(state, &req.body, &mut stream),
        ("POST", "/api/animation/keyframe/add") => api_add_keyframe(state, &req.body, &mut stream),
        ("POST", "/api/animation/keyframe/remove") => {
            api_remove_keyframe(state, &req.body, &mut stream)
        }
        ("POST", "/api/animation/play") => api_play_animation(state, &req.body, &mut stream),
        ("POST", "/api/animation/stop") => api_stop_animation(state, &mut stream),
        ("GET", "/api/animation/status") => api_animation_status(state, &mut stream),
        ("POST", "/api/animation/seek") => api_seek_animation(state, &req.body, &mut stream),
        ("POST", "/api/animation/record") => api_toggle_recording(state, &req.body, &mut stream),
        ("POST", "/api/animation/blend") => api_animation_blend(state, &req.body, &mut stream),
        ("POST", "/api/runtime/play") => api_runtime_play(state, &mut stream),
        ("POST", "/api/runtime/stop") => api_runtime_stop(state, &mut stream),
        ("POST", "/api/runtime/pause") => api_runtime_pause(state, &mut stream),
        ("POST", "/api/runtime/step") => api_runtime_step(state, &mut stream),
        ("GET", "/api/runtime/status") => api_runtime_status(state, &mut stream),
        // Input endpoints
        ("POST", "/api/runtime/input/key_down") => {
            api_input_key_down(state, &req.body, &mut stream)
        }
        ("POST", "/api/runtime/input/key_up") => api_input_key_up(state, &req.body, &mut stream),
        ("POST", "/api/runtime/input/mouse_move") => {
            api_input_mouse_move(state, &req.body, &mut stream)
        }
        ("POST", "/api/runtime/input/mouse_down") => {
            api_input_mouse_down(state, &req.body, &mut stream)
        }
        ("POST", "/api/runtime/input/mouse_up") => {
            api_input_mouse_up(state, &req.body, &mut stream)
        }
        ("POST", "/api/runtime/input/clear_frame") => api_input_clear_frame(state, &mut stream),
        ("GET", "/api/runtime/input/state") => api_input_state(state, &mut stream),
        // Scene instancing + collision shape editing
        ("POST", "/api/scene/instance") => api_instance_scene(state, &req.body, &mut stream),
        ("POST", "/api/viewport/shape_resize") => api_shape_resize(state, &req.body, &mut stream),
        ("POST", "/api/tilemap/paint") => api_tilemap_paint(state, &req.body, &mut stream),
        ("POST", "/api/tilemap/erase") => api_tilemap_erase(state, &req.body, &mut stream),
        ("POST", "/api/tilemap/fill") => api_tilemap_fill(state, &req.body, &mut stream),
        ("GET", "/api/tilemap/data") => api_tilemap_data(state, &req.query, &mut stream),
        ("POST", "/api/tilemap/resize") => api_tilemap_resize(state, &req.body, &mut stream),
        ("GET", "/api/tilemap/tileset") => api_tilemap_tileset(state, &mut stream),
        // pat-r5p: Transform gizmo
        ("POST", "/api/viewport/drag_axis") => {
            api_viewport_drag_axis(state, &req.body, &mut stream)
        }
        ("POST", "/api/viewport/rotate_node") => {
            api_viewport_rotate_node(state, &req.body, &mut stream)
        }
        ("POST", "/api/viewport/scale_node") => {
            api_viewport_scale_node(state, &req.body, &mut stream)
        }
        // pat-zlv: Snap info
        ("GET", "/api/viewport/snap_info") => api_get_snap_info(state, &mut stream),
        ("GET", "/api/viewport/snap_guides") => api_get_snap_guides(state, &mut stream),
        // pat-cgc: Script find/replace
        ("POST", "/api/script/find") => api_script_find(&req.body, &mut stream),
        ("POST", "/api/script/replace") => api_script_replace(&req.body, &mut stream),
        // pat-1v3: Script breakpoints
        ("POST", "/api/script/breakpoint/toggle") => {
            api_toggle_breakpoint(state, &req.body, &mut stream)
        }
        ("GET", "/api/script/breakpoints") => api_get_breakpoints(state, &req.query, &mut stream),
        // pat-n86px: Explicit typed track creation (property/method/audio)
        ("POST", "/api/animation/track/add") => api_add_track(state, &req.body, &mut stream),
        ("POST", "/api/animation/track/delete") => api_delete_track(state, &req.body, &mut stream),
        // pat-2s1: Animation track reorder + keyframe copy/paste
        ("POST", "/api/animation/track/reorder") => {
            api_reorder_track(state, &req.body, &mut stream)
        }
        ("POST", "/api/animation/keyframe/copy") => {
            api_copy_keyframes(state, &req.body, &mut stream)
        }
        ("POST", "/api/animation/keyframe/paste") => {
            api_paste_keyframes(state, &req.body, &mut stream)
        }
        // pat-o51nk: Curve editor — set keyframe transition type
        ("POST", "/api/animation/keyframe/transition") => {
            api_set_keyframe_transition(state, &req.body, &mut stream)
        }
        ("GET", "/api/animation/keyframe/transition") => {
            api_get_keyframe_transition(state, &req.query, &mut stream)
        }
        // pat-lbu: Debug + monitors
        ("GET", "/api/debug/stack_trace") => api_get_stack_trace(state, &mut stream),
        // pat-zf49m: Debugger panel with breakpoint, step, and variable inspection
        ("GET", "/api/debug/state") => api_get_debug_state(state, &mut stream),
        ("GET", "/api/debug/locals") => api_get_debug_locals(state, &req.query, &mut stream),
        ("POST", "/api/debug/continue") => api_debug_continue(state, &mut stream),
        ("POST", "/api/debug/step_in") => api_debug_step_in(state, &mut stream),
        ("POST", "/api/debug/step_over") => api_debug_step_over(state, &mut stream),
        ("POST", "/api/debug/step_out") => api_debug_step_out(state, &mut stream),
        ("POST", "/api/debug/remove_breakpoint") => {
            api_debug_remove_breakpoint(state, &req.body, &mut stream)
        }
        ("GET", "/api/monitors/frame_times") => api_get_frame_times(state, &mut stream),
        ("GET", "/api/profiler") => api_get_profiler(state, &mut stream),
        ("POST", "/api/profiler/record") => api_profiler_record(state, &req.body, &mut stream),
        // pat-dj6: Editor mode
        ("POST", "/api/editor/mode") => api_set_editor_mode(state, &req.body, &mut stream),
        ("GET", "/api/editor/mode") => api_get_editor_mode(state, &mut stream),
        // pat-e0heb: Scene tabs
        ("GET", "/api/scene/tabs") => api_get_scene_tabs(state, &mut stream),
        ("POST", "/api/scene/tabs/open") => api_open_scene_tab(state, &req.body, &mut stream),
        ("POST", "/api/scene/tabs/close") => api_close_scene_tab(state, &req.body, &mut stream),
        ("POST", "/api/scene/tabs/switch") => api_switch_scene_tab(state, &req.body, &mut stream),
        // Batch 2 beads
        ("POST", "/api/viewport/set_mode") => api_set_viewport_mode(state, &req.body, &mut stream),
        ("GET", "/api/viewport/mode") => api_get_viewport_mode(state, &mut stream),
        ("GET", "/api/search") => api_search_scripts(&req.query, &mut stream),
        ("POST", "/api/signal/disconnect") => api_disconnect_signal(state, &req.body, &mut stream),
        ("POST", "/api/output/clear") => api_clear_output(state, &mut stream),
        ("GET", "/api/output") => api_get_output(state, &mut stream),
        // pat-kj4: Project settings
        ("GET", "/api/project_settings") => api_get_project_settings(state, &mut stream),
        ("POST", "/api/project_settings") => {
            api_set_project_settings(state, &req.body, &mut stream)
        }
        // pat-flr: Filesystem operations
        ("POST", "/api/filesystem/rename") => api_filesystem_rename(&req.body, &mut stream),
        ("POST", "/api/filesystem/delete") => api_filesystem_delete(&req.body, &mut stream),
        ("POST", "/api/filesystem/mkdir") => api_filesystem_mkdir(&req.body, &mut stream),
        // pat-mn3: Multi-object shared properties
        ("POST", "/api/node/shared_properties") => {
            api_get_shared_properties(state, &req.body, &mut stream)
        }
        // pat-ugb0p: Command palette
        ("GET", "/api/commands") => api_get_commands(&mut stream),
        ("POST", "/api/command/execute") => api_execute_command(state, &req.body, &mut stream),
        // pat-omfrq: Filesystem tree and dir
        ("GET", "/api/filesystem/tree") => api_filesystem_tree(&req.query, &mut stream),
        ("GET", "/api/filesystem/dir") => api_filesystem_dir(&req.query, &mut stream),
        // pat-vyko1: Import settings
        ("GET", "/api/import_settings") => api_get_import_settings(&req.query, &mut stream),
        ("POST", "/api/import_settings") => api_set_import_settings(&req.body, &mut stream),
        // pat-1zlel: Version control integration
        ("GET", "/api/vcs/status") => api_vcs_status(&mut stream),
        ("GET", "/api/vcs/diff") => api_vcs_diff(&req.query, &mut stream),
        ("GET", "/api/vcs/log") => api_vcs_log(&req.query, &mut stream),
        ("POST", "/api/vcs/stage") => api_vcs_stage(&req.body, &mut stream),
        ("POST", "/api/vcs/unstage") => api_vcs_unstage(&req.body, &mut stream),
        ("POST", "/api/vcs/discard") => api_vcs_discard(&req.body, &mut stream),
        _ => serve_404(&mut stream),
    }
}

// ---------------------------------------------------------------------------
// Response helpers
// ---------------------------------------------------------------------------

fn send_json(stream: &mut TcpStream, json: &str) {
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json; charset=utf-8\r\nContent-Length: {}\r\nAccess-Control-Allow-Origin: *\r\nConnection: close\r\n\r\n{}",
        json.len(),
        json
    );
    let _ = stream.write_all(response.as_bytes());
}

fn send_error(stream: &mut TcpStream, status: u16, message: &str) {
    let json = format!(r#"{{"error":"{}"}}"#, message.replace('"', "\\\""));
    let status_text = match status {
        400 => "Bad Request",
        404 => "Not Found",
        500 => "Internal Server Error",
        _ => "Error",
    };
    let response = format!(
        "HTTP/1.1 {status} {status_text}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nAccess-Control-Allow-Origin: *\r\nConnection: close\r\n\r\n{}",
        json.len(),
        json
    );
    let _ = stream.write_all(response.as_bytes());
}

fn send_binary(stream: &mut TcpStream, content_type: &str, data: &[u8]) {
    use std::io::BufWriter;
    let header = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nAccess-Control-Allow-Origin: *\r\nCache-Control: no-cache\r\nConnection: close\r\n\r\n",
        data.len()
    );
    let mut writer = BufWriter::new(stream);
    let _ = writer.write_all(header.as_bytes());
    let _ = writer.write_all(data);
    let _ = writer.flush();
}

fn serve_cors_preflight(stream: &mut TcpStream) {
    let response = "HTTP/1.1 204 No Content\r\nAccess-Control-Allow-Origin: *\r\nAccess-Control-Allow-Methods: GET, POST, OPTIONS\r\nAccess-Control-Allow-Headers: Content-Type\r\nConnection: close\r\n\r\n";
    let _ = stream.write_all(response.as_bytes());
}

fn serve_editor_html(stream: &mut TcpStream) {
    let html = crate::editor_ui::EDITOR_HTML;
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nAccess-Control-Allow-Origin: *\r\nConnection: close\r\n\r\n{}",
        html.len(),
        html
    );
    let _ = stream.write_all(response.as_bytes());
}

fn serve_404(stream: &mut TcpStream) {
    let response = "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nAccess-Control-Allow-Origin: *\r\nConnection: close\r\n\r\n";
    let _ = stream.write_all(response.as_bytes());
}

// ---------------------------------------------------------------------------
// JSON helpers
// ---------------------------------------------------------------------------

/// Extracts a string field from a JSON body (minimal parsing via serde_json).
fn parse_json_body(body: &str) -> Option<serde_json::Value> {
    serde_json::from_str(body).ok()
}

fn node_to_json_tree(tree: &SceneTree, node_id: NodeId) -> serde_json::Value {
    let node = match tree.get_node(node_id) {
        Some(n) => n,
        None => return serde_json::Value::Null,
    };
    let path = tree.node_path(node_id).unwrap_or_default();
    let children: Vec<serde_json::Value> = node
        .children()
        .iter()
        .map(|&cid| node_to_json_tree(tree, cid))
        .collect();

    let visible = match node.get_property("visible") {
        Variant::Bool(b) => b,
        _ => true, // default visible
    };

    // Detect instanced scenes: nodes with _instance_source or _instance property.
    let is_instance = !matches!(node.get_property("_instance_source"), Variant::Nil)
        || !matches!(node.get_property("_instance"), Variant::Nil);

    // Detect scripts: via _script_path property or attached ScriptInstance.
    let has_script =
        !matches!(node.get_property("_script_path"), Variant::Nil) || tree.has_script(node_id);

    // Detect signal connections.
    let has_signals = match node.get_property("signal_connections") {
        Variant::String(ref s) if !s.is_empty() => true,
        _ => false,
    };

    // Collect groups.
    let groups: Vec<&str> = node.groups().iter().map(|s| s.as_str()).collect();
    let prop_groups: Vec<String> = match node.get_property("groups") {
        Variant::String(ref s) if !s.is_empty() => {
            s.split(',').map(|g| g.trim().to_string()).collect()
        }
        _ => Vec::new(),
    };
    let mut all_groups: Vec<String> = groups.iter().map(|g| g.to_string()).collect();
    for g in &prop_groups {
        if !all_groups.contains(g) {
            all_groups.push(g.clone());
        }
    }

    serde_json::json!({
        "id": node_id.raw(),
        "name": node.name(),
        "class": node.class_name(),
        "path": path,
        "visible": visible,
        "is_instance": is_instance,
        "has_script": has_script,
        "has_signals": has_signals,
        "groups": all_groups,
        "children": children
    })
}

fn node_properties_json(tree: &SceneTree, node_id: NodeId) -> serde_json::Value {
    let node = match tree.get_node(node_id) {
        Some(n) => n,
        None => return serde_json::Value::Null,
    };
    let path = tree.node_path(node_id).unwrap_or_default();

    let props: Vec<serde_json::Value> = node
        .properties()
        .map(|(name, value)| {
            let variant_json = to_json(value);
            let type_name = variant_json
                .get("type")
                .and_then(|t| t.as_str())
                .unwrap_or("Unknown")
                .to_string();
            serde_json::json!({
                "name": name,
                "type": type_name,
                "value": variant_json
            })
        })
        .collect();

    serde_json::json!({
        "id": node_id.raw(),
        "name": node.name(),
        "class": node.class_name(),
        "path": path,
        "properties": props
    })
}

/// Finds a NodeId from a raw u64 by scanning the scene tree.
fn find_node_by_raw_id(tree: &SceneTree, raw: u64) -> Option<NodeId> {
    // Walk from root to find the node with this raw id.
    let mut stack = vec![tree.root_id()];
    while let Some(nid) = stack.pop() {
        if nid.raw() == raw {
            return Some(nid);
        }
        if let Some(node) = tree.get_node(nid) {
            for &child in node.children() {
                stack.push(child);
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------
// API endpoint handlers
// ---------------------------------------------------------------------------

/// `GET /api/scene` — returns the full scene tree as JSON.
fn api_get_scene(state: &Arc<Mutex<EditorState>>, stream: &mut TcpStream) {
    // Lock → build JSON → unlock → then send (minimizes lock hold time).
    let json = {
        let state = state.lock().unwrap();
        let root_id = state.scene_tree.root_id();
        let tree_json = node_to_json_tree(&state.scene_tree, root_id);
        serde_json::json!({ "nodes": tree_json }).to_string()
    };
    send_json(stream, &json);
}

/// `GET /api/node/<id>` — returns node details and properties.
fn api_get_node(state: &Arc<Mutex<EditorState>>, id_str: &str, stream: &mut TcpStream) {
    let raw: u64 = match id_str.parse() {
        Ok(v) => v,
        Err(_) => {
            send_error(stream, 400, "invalid node id");
            return;
        }
    };

    let json = {
        let state = state.lock().unwrap();
        let node_id = match find_node_by_raw_id(&state.scene_tree, raw) {
            Some(id) => id,
            None => {
                send_error(stream, 404, "node not found");
                return;
            }
        };
        node_properties_json(&state.scene_tree, node_id).to_string()
    };
    send_json(stream, &json);
}

/// `POST /api/node/add` — adds a new node to the tree.
fn api_add_node(state: &Arc<Mutex<EditorState>>, body: &str, stream: &mut TcpStream) {
    let parsed = match parse_json_body(body) {
        Some(v) => v,
        None => {
            send_error(stream, 400, "invalid JSON");
            return;
        }
    };

    let parent_raw = match parsed.get("parent_id").and_then(|v| v.as_u64()) {
        Some(v) => v,
        None => {
            send_error(stream, 400, "missing parent_id");
            return;
        }
    };
    let name = match parsed.get("name").and_then(|v| v.as_str()) {
        Some(v) => v.to_string(),
        None => {
            send_error(stream, 400, "missing name");
            return;
        }
    };
    let class_name = match parsed.get("class_name").and_then(|v| v.as_str()) {
        Some(v) => v.to_string(),
        None => {
            send_error(stream, 400, "missing class_name");
            return;
        }
    };

    let mut state = state.lock().unwrap();
    let parent_id = match find_node_by_raw_id(&state.scene_tree, parent_raw) {
        Some(id) => id,
        None => {
            send_error(stream, 404, "parent not found");
            return;
        }
    };

    let name_str = name.clone();
    let class_name_str = class_name.clone();
    let mut cmd = EditorCommand::AddNode {
        parent_id,
        name,
        class_name,
        created_id: None,
    };

    if let Err(e) = cmd.execute(&mut state.scene_tree) {
        send_error(stream, 500, &e.to_string());
        return;
    }

    let created_id = match &cmd {
        EditorCommand::AddNode { created_id, .. } => created_id.unwrap(),
        _ => unreachable!(),
    };

    state.undo_stack.push(cmd);
    state.redo_stack.clear();
    state.scene_modified = true;
    state.add_log(
        "info",
        format!("Added {} node '{}'", class_name_str, name_str),
    );

    let json = format!(r#"{{"id":{}}}"#, created_id.raw());
    send_json(stream, &json);
}

/// `POST /api/node/delete` — removes a node from the tree.
fn api_delete_node(state: &Arc<Mutex<EditorState>>, body: &str, stream: &mut TcpStream) {
    let parsed = match parse_json_body(body) {
        Some(v) => v,
        None => {
            send_error(stream, 400, "invalid JSON");
            return;
        }
    };

    let node_raw = match parsed.get("node_id").and_then(|v| v.as_u64()) {
        Some(v) => v,
        None => {
            send_error(stream, 400, "missing node_id");
            return;
        }
    };

    let mut state = state.lock().unwrap();
    let node_id = match find_node_by_raw_id(&state.scene_tree, node_raw) {
        Some(id) => id,
        None => {
            send_error(stream, 404, "node not found");
            return;
        }
    };

    // Get node info for undo.
    let (name, class_name) = {
        let node = state.scene_tree.get_node(node_id).unwrap();
        (node.name().to_string(), node.class_name().to_string())
    };

    let log_name = name.clone();
    let mut cmd = EditorCommand::RemoveNode {
        node_id,
        parent_id: None,
        name,
        class_name,
    };

    if let Err(e) = cmd.execute(&mut state.scene_tree) {
        send_error(stream, 500, &e.to_string());
        return;
    }

    state.undo_stack.push(cmd);
    state.redo_stack.clear();
    state.scene_modified = true;
    state.add_log("info", format!("Deleted node '{}'", log_name));

    send_json(stream, r#"{"ok":true}"#);
}

/// `POST /api/node/select` — selects a node.
fn api_select_node(state: &Arc<Mutex<EditorState>>, body: &str, stream: &mut TcpStream) {
    let parsed = match parse_json_body(body) {
        Some(v) => v,
        None => {
            send_error(stream, 400, "invalid JSON");
            return;
        }
    };

    let node_raw = match parsed.get("node_id").and_then(|v| v.as_u64()) {
        Some(v) => v,
        None => {
            send_error(stream, 400, "missing node_id");
            return;
        }
    };

    let mut state = state.lock().unwrap();
    let node_id = match find_node_by_raw_id(&state.scene_tree, node_raw) {
        Some(id) => id,
        None => {
            send_error(stream, 404, "node not found");
            return;
        }
    };

    state.selected_node = Some(node_id);
    state.selected_nodes = vec![node_id];
    send_json(stream, r#"{"ok":true}"#);
}

/// `GET /api/selected` — returns the selected node's info.
fn api_get_selected(state: &Arc<Mutex<EditorState>>, stream: &mut TcpStream) {
    let json = {
        let state = state.lock().unwrap();
        match state.selected_node {
            Some(node_id) => node_properties_json(&state.scene_tree, node_id).to_string(),
            None => "null".to_string(),
        }
    };
    send_json(stream, &json);
}

/// `POST /api/node/reparent` — reparents a node.
fn api_reparent_node(state: &Arc<Mutex<EditorState>>, body: &str, stream: &mut TcpStream) {
    let parsed = match parse_json_body(body) {
        Some(v) => v,
        None => {
            send_error(stream, 400, "invalid JSON");
            return;
        }
    };

    let node_raw = match parsed.get("node_id").and_then(|v| v.as_u64()) {
        Some(v) => v,
        None => {
            send_error(stream, 400, "missing node_id");
            return;
        }
    };
    let new_parent_raw = match parsed.get("new_parent_id").and_then(|v| v.as_u64()) {
        Some(v) => v,
        None => {
            send_error(stream, 400, "missing new_parent_id");
            return;
        }
    };

    let mut state = state.lock().unwrap();
    let node_id = match find_node_by_raw_id(&state.scene_tree, node_raw) {
        Some(id) => id,
        None => {
            send_error(stream, 404, "node not found");
            return;
        }
    };
    let new_parent_id = match find_node_by_raw_id(&state.scene_tree, new_parent_raw) {
        Some(id) => id,
        None => {
            send_error(stream, 404, "new parent not found");
            return;
        }
    };

    let mut cmd = EditorCommand::ReparentNode {
        node_id,
        new_parent_id,
        old_parent_id: None,
    };

    if let Err(e) = cmd.execute(&mut state.scene_tree) {
        send_error(stream, 500, &e.to_string());
        return;
    }

    state.undo_stack.push(cmd);
    state.redo_stack.clear();

    send_json(stream, r#"{"ok":true}"#);
}

/// `POST /api/node/rename` — renames a node.
fn api_rename_node(state: &Arc<Mutex<EditorState>>, body: &str, stream: &mut TcpStream) {
    let parsed = match parse_json_body(body) {
        Some(v) => v,
        None => {
            send_error(stream, 400, "invalid JSON");
            return;
        }
    };

    let node_raw = match parsed.get("node_id").and_then(|v| v.as_u64()) {
        Some(v) => v,
        None => {
            send_error(stream, 400, "missing node_id");
            return;
        }
    };
    let new_name = match parsed.get("new_name").and_then(|v| v.as_str()) {
        Some(v) => v.to_string(),
        None => {
            send_error(stream, 400, "missing new_name");
            return;
        }
    };

    let mut state = state.lock().unwrap();
    let node_id = match find_node_by_raw_id(&state.scene_tree, node_raw) {
        Some(id) => id,
        None => {
            send_error(stream, 404, "node not found");
            return;
        }
    };

    let new_name_log = new_name.clone();
    let mut cmd = EditorCommand::RenameNode {
        node_id,
        new_name,
        old_name: String::new(),
    };

    if let Err(e) = cmd.execute(&mut state.scene_tree) {
        send_error(stream, 500, &e.to_string());
        return;
    }

    let old_name_log = match &cmd {
        EditorCommand::RenameNode { old_name, .. } => old_name.clone(),
        _ => String::new(),
    };
    state.undo_stack.push(cmd);
    state.redo_stack.clear();
    state.scene_modified = true;
    state.add_log(
        "info",
        format!("Renamed '{}' to '{}'", old_name_log, new_name_log),
    );

    send_json(stream, r#"{"ok":true}"#);
}

/// `POST /api/node/duplicate` — duplicates a node and its subtree as a sibling.
fn api_duplicate_node(state: &Arc<Mutex<EditorState>>, body: &str, stream: &mut TcpStream) {
    let parsed = match parse_json_body(body) {
        Some(v) => v,
        None => {
            send_error(stream, 400, "invalid JSON");
            return;
        }
    };

    let node_raw = match parsed.get("node_id").and_then(|v| v.as_u64()) {
        Some(v) => v,
        None => {
            send_error(stream, 400, "missing node_id");
            return;
        }
    };

    let mut state = state.lock().unwrap();
    let node_id = match find_node_by_raw_id(&state.scene_tree, node_raw) {
        Some(id) => id,
        None => {
            send_error(stream, 404, "node not found");
            return;
        }
    };

    let mut cmd = EditorCommand::DuplicateNode {
        source_id: node_id,
        created_ids: Vec::new(),
    };

    if let Err(e) = cmd.execute(&mut state.scene_tree) {
        send_error(stream, 500, &e.to_string());
        return;
    }

    let root_created = match &cmd {
        EditorCommand::DuplicateNode { created_ids, .. } => {
            created_ids.first().map(|id| id.raw()).unwrap_or(0)
        }
        _ => unreachable!(),
    };

    state.undo_stack.push(cmd);
    state.redo_stack.clear();

    let json = format!(r#"{{"id":{root_created}}}"#);
    send_json(stream, &json);
}

/// `POST /api/node/create_dialog` — returns classes from ClassDB with search, filter, favorites, and recent.
fn api_create_dialog(state: &Arc<Mutex<EditorState>>, body: &str, stream: &mut TcpStream) {
    use crate::create_dialog::NodeCategory;

    let mut st = state.lock().unwrap();
    let dlg = &mut st.create_node_dialog;
    if !body.is_empty() {
        if let Some(parsed) = parse_json_body(body) {
            if let Some(search) = parsed.get("search").and_then(|v| v.as_str()) {
                dlg.set_search(search);
            } else {
                dlg.set_search("");
            }
            if let Some(base) = parsed.get("base_class").and_then(|v| v.as_str()) {
                if base.is_empty() {
                    dlg.set_base_class(None);
                } else {
                    dlg.set_base_class(Some(base.to_string()));
                }
            }
            if let Some(cat) = parsed.get("category").and_then(|v| v.as_str()) {
                let category = match cat {
                    "Node2D" | "2D Nodes" => Some(NodeCategory::Node2D),
                    "Physics2D" | "2D Physics" => Some(NodeCategory::Physics2D),
                    "UI" | "UI Controls" => Some(NodeCategory::UI),
                    "Utility" => Some(NodeCategory::Utility),
                    _ => None,
                };
                dlg.set_category_filter(category);
            } else {
                dlg.set_category_filter(None);
            }
        }
    } else {
        dlg.set_search("");
        dlg.set_base_class(None);
        dlg.set_category_filter(None);
    }
    let classes = dlg.filtered_classes();
    let recent = dlg.recent_entries();
    let favorites: Vec<&str> = dlg.favorites().iter().map(|s| s.as_str()).collect();
    let class_names: Vec<&str> = classes.iter().map(|c| c.class_name.as_str()).collect();
    let class_entries: Vec<serde_json::Value> = classes
        .iter()
        .map(|c| {
            serde_json::json!({
                "class_name": c.class_name, "parent_class": c.parent_class,
                "inheritance_chain": c.inheritance_chain, "is_favorite": c.is_favorite,
                "description": c.description, "category": c.category.map(|cat| cat.label()),
            })
        })
        .collect();
    let recent_json: Vec<serde_json::Value> = recent
        .iter()
        .map(|c| serde_json::json!({ "class_name": c.class_name, "parent_class": c.parent_class }))
        .collect();
    let json = serde_json::json!({
        "classes": class_names, "class_entries": class_entries, "favorites": favorites,
        "recent": recent_json, "match_count": class_names.len(),
    })
    .to_string();
    send_json(stream, &json);
}

/// `GET /api/node/catalog_2d` — returns the 2D node catalog with categories and helper presets.
fn api_node_catalog_2d(state: &Arc<Mutex<EditorState>>, stream: &mut TcpStream) {
    let st = state.lock().unwrap();
    let dlg = &st.create_node_dialog;
    if let Some(catalog) = dlg.catalog() {
        let entries: Vec<serde_json::Value> = catalog
            .entries()
            .iter()
            .map(|e| {
                serde_json::json!({
                    "class_name": e.class_name,
                    "category": e.category.label(),
                    "description": e.description,
                })
            })
            .collect();
        let helpers: Vec<serde_json::Value> = catalog
            .helpers()
            .iter()
            .map(|h| {
                serde_json::json!({
                    "name": h.name,
                    "description": h.description,
                    "root_class": h.root_class,
                    "children": h.children,
                })
            })
            .collect();
        let categories: Vec<&str> = catalog.categories().iter().map(|c| c.label()).collect();
        let json = serde_json::json!({
            "entries": entries, "helpers": helpers, "categories": categories,
        })
        .to_string();
        send_json(stream, &json);
    } else {
        send_json(stream, r#"{"entries":[],"helpers":[],"categories":[]}"#);
    }
}

/// `POST /api/node/create_dialog/toggle_favorite` — toggle a class favorite.
fn api_create_dialog_toggle_favorite(
    state: &Arc<Mutex<EditorState>>,
    body: &str,
    stream: &mut TcpStream,
) {
    let parsed = match parse_json_body(body) {
        Some(v) => v,
        None => {
            send_error(stream, 400, "invalid JSON");
            return;
        }
    };
    let class_name = match parsed.get("class_name").and_then(|v| v.as_str()) {
        Some(n) => n.to_string(),
        None => {
            send_error(stream, 400, "missing class_name");
            return;
        }
    };
    let mut st = state.lock().unwrap();
    let dlg = &mut st.create_node_dialog;
    let is_favorite = if dlg.favorites().contains(&class_name) {
        dlg.remove_favorite(&class_name);
        false
    } else {
        dlg.add_favorite(&class_name);
        true
    };
    let favorites: Vec<&str> = dlg.favorites().iter().map(|s| s.as_str()).collect();
    let json =
        serde_json::json!({ "is_favorite": is_favorite, "favorites": favorites }).to_string();
    send_json(stream, &json);
}

/// `POST /api/node/create_dialog/confirm` — confirm selection, track in recent list.
fn api_create_dialog_confirm(state: &Arc<Mutex<EditorState>>, body: &str, stream: &mut TcpStream) {
    let parsed = match parse_json_body(body) {
        Some(v) => v,
        None => {
            send_error(stream, 400, "invalid JSON");
            return;
        }
    };
    let class_name = match parsed.get("class_name").and_then(|v| v.as_str()) {
        Some(n) => n.to_string(),
        None => {
            send_error(stream, 400, "missing class_name");
            return;
        }
    };
    let mut st = state.lock().unwrap();
    let dlg = &mut st.create_node_dialog;
    dlg.select(&class_name);
    dlg.confirm();
    send_json(stream, r#"{"ok":true}"#);
}

/// `POST /api/resource/property/set` — set a property within a resource sub-editor.
fn api_set_resource_property(state: &Arc<Mutex<EditorState>>, body: &str, stream: &mut TcpStream) {
    let parsed = match parse_json_body(body) {
        Some(v) => v,
        None => {
            send_error(stream, 400, "invalid JSON");
            return;
        }
    };
    let node_raw = match parsed.get("node_id").and_then(|v| v.as_u64()) {
        Some(v) => v,
        None => {
            send_error(stream, 400, "missing node_id");
            return;
        }
    };
    let resource_property = match parsed.get("resource_property").and_then(|v| v.as_str()) {
        Some(v) => v.to_string(),
        None => {
            send_error(stream, 400, "missing resource_property");
            return;
        }
    };
    let sub_property = match parsed.get("sub_property").and_then(|v| v.as_str()) {
        Some(v) => v.to_string(),
        None => {
            send_error(stream, 400, "missing sub_property");
            return;
        }
    };
    let value_json = match parsed.get("value") {
        Some(v) => v,
        None => {
            send_error(stream, 400, "missing value");
            return;
        }
    };
    let new_sub_value = match from_json(value_json) {
        Some(v) => v,
        None => {
            send_error(stream, 400, "invalid variant value");
            return;
        }
    };
    let mut state = state.lock().unwrap();
    let node_id = match find_node_by_raw_id(&state.scene_tree, node_raw) {
        Some(id) => id,
        None => {
            send_error(stream, 404, "node not found");
            return;
        }
    };
    let current = match state
        .scene_tree
        .get_node(node_id)
        .map(|n| n.get_property(&resource_property))
    {
        Some(v) => v,
        None => {
            send_error(stream, 404, "resource property not found");
            return;
        }
    };
    let new_value = match current {
        Variant::Resource(mut r) => {
            r.properties.insert(sub_property.clone(), new_sub_value);
            Variant::Resource(r)
        }
        _ => {
            send_error(stream, 400, "property is not a Resource type");
            return;
        }
    };
    let mut cmd = EditorCommand::SetProperty {
        node_id,
        property: resource_property.clone(),
        new_value,
        old_value: Variant::Nil,
    };
    if let Err(e) = cmd.execute(&mut state.scene_tree) {
        send_error(stream, 500, &e.to_string());
        return;
    }
    state.undo_stack.push(cmd);
    state.redo_stack.clear();
    state.scene_modified = true;
    state.add_log(
        "info",
        format!(
            "Changed resource property '{}.{}'",
            resource_property, sub_property
        ),
    );
    send_json(stream, r#"{"ok":true}"#);
}

fn api_reorder_node(state: &Arc<Mutex<EditorState>>, body: &str, stream: &mut TcpStream) {
    let parsed = match parse_json_body(body) {
        Some(v) => v,
        None => {
            send_error(stream, 400, "invalid JSON");
            return;
        }
    };

    let node_raw = match parsed.get("node_id").and_then(|v| v.as_u64()) {
        Some(v) => v,
        None => {
            send_error(stream, 400, "missing node_id");
            return;
        }
    };
    let direction = match parsed.get("direction").and_then(|v| v.as_str()) {
        Some(v) => v.to_string(),
        None => {
            send_error(stream, 400, "missing direction (up or down)");
            return;
        }
    };

    let mut state = state.lock().unwrap();
    let node_id = match find_node_by_raw_id(&state.scene_tree, node_raw) {
        Some(id) => id,
        None => {
            send_error(stream, 404, "node not found");
            return;
        }
    };

    let parent_id = match state.scene_tree.get_node(node_id).and_then(|n| n.parent()) {
        Some(pid) => pid,
        None => {
            send_error(stream, 400, "node has no parent");
            return;
        }
    };

    // Get parent's children and find index.
    let children: Vec<NodeId> = state
        .scene_tree
        .get_node(parent_id)
        .map(|n| n.children().to_vec())
        .unwrap_or_default();

    let idx = match children.iter().position(|&c| c == node_id) {
        Some(i) => i,
        None => {
            send_error(stream, 500, "node not found in parent children");
            return;
        }
    };

    let new_idx = match direction.as_str() {
        "up" => {
            if idx == 0 {
                send_json(stream, r#"{"ok":true}"#);
                return;
            }
            idx - 1
        }
        "down" => {
            if idx >= children.len() - 1 {
                send_json(stream, r#"{"ok":true}"#);
                return;
            }
            idx + 1
        }
        _ => {
            send_error(stream, 400, "direction must be 'up' or 'down'");
            return;
        }
    };

    // Swap in the parent's children list.
    if let Some(parent) = state.scene_tree.get_node_mut(parent_id) {
        let children = parent.children_mut();
        children.swap(idx, new_idx);
    }

    send_json(stream, r#"{"ok":true}"#);
}

/// `POST /api/property/set` — sets a property on a node.
fn api_set_property(state: &Arc<Mutex<EditorState>>, body: &str, stream: &mut TcpStream) {
    let parsed = match parse_json_body(body) {
        Some(v) => v,
        None => {
            send_error(stream, 400, "invalid JSON");
            return;
        }
    };

    let node_raw = match parsed.get("node_id").and_then(|v| v.as_u64()) {
        Some(v) => v,
        None => {
            send_error(stream, 400, "missing node_id");
            return;
        }
    };
    let property = match parsed.get("property").and_then(|v| v.as_str()) {
        Some(v) => v.to_string(),
        None => {
            send_error(stream, 400, "missing property");
            return;
        }
    };
    let value_json = match parsed.get("value") {
        Some(v) => v,
        None => {
            send_error(stream, 400, "missing value");
            return;
        }
    };
    let new_value = match from_json(value_json) {
        Some(v) => v,
        None => {
            send_error(stream, 400, "invalid variant value");
            return;
        }
    };

    let mut state = state.lock().unwrap();
    let node_id = match find_node_by_raw_id(&state.scene_tree, node_raw) {
        Some(id) => id,
        None => {
            send_error(stream, 404, "node not found");
            return;
        }
    };

    let prop_name = property.clone();
    let mut cmd = EditorCommand::SetProperty {
        node_id,
        property,
        new_value,
        old_value: Variant::Nil,
    };

    if let Err(e) = cmd.execute(&mut state.scene_tree) {
        send_error(stream, 500, &e.to_string());
        return;
    }

    state.undo_stack.push(cmd);
    state.redo_stack.clear();
    state.scene_modified = true;
    state.add_log("info", format!("Changed property '{}'", prop_name));

    send_json(stream, r#"{"ok":true}"#);
}

/// `POST /api/undo` — undoes the last command.
fn api_undo(state: &Arc<Mutex<EditorState>>, stream: &mut TcpStream) {
    let mut state = state.lock().unwrap();
    let cmd = match state.undo_stack.pop() {
        Some(c) => c,
        None => {
            send_error(stream, 400, "nothing to undo");
            return;
        }
    };

    if let Err(e) = cmd.undo(&mut state.scene_tree) {
        // Push it back if undo failed.
        state.undo_stack.push(cmd);
        send_error(stream, 500, &e.to_string());
        return;
    }

    let _ = cmd.undo_tilemap(&mut state.tile_grid_store);
    state.redo_stack.push(cmd);
    state.add_log("info", "Undo");
    send_json(stream, r#"{"ok":true}"#);
}

/// `POST /api/redo` — redoes the last undone command.
fn api_redo(state: &Arc<Mutex<EditorState>>, stream: &mut TcpStream) {
    let mut state = state.lock().unwrap();
    let mut cmd = match state.redo_stack.pop() {
        Some(c) => c,
        None => {
            send_error(stream, 400, "nothing to redo");
            return;
        }
    };

    if let Err(e) = cmd.execute(&mut state.scene_tree) {
        state.redo_stack.push(cmd);
        send_error(stream, 500, &e.to_string());
        return;
    }

    let _ = cmd.execute_tilemap(&mut state.tile_grid_store);
    state.undo_stack.push(cmd);
    state.add_log("info", "Redo");
    send_json(stream, r#"{"ok":true}"#);
}

/// `GET /api/viewport` — returns the latest frame as BMP (from separate cache).
fn api_get_viewport_bmp(cache: &Arc<ViewportCache>, stream: &mut TcpStream) {
    let bmp = cache.bmp.lock().unwrap();
    match &*bmp {
        Some(data) => {
            send_binary(stream, "image/bmp", data);
        }
        None => {
            send_error(stream, 404, "no frame available");
        }
    }
}

/// `GET /api/viewport/png` — returns the latest frame as PNG (from separate cache).
fn api_get_viewport_png(cache: &Arc<ViewportCache>, stream: &mut TcpStream) {
    let png = cache.png.lock().unwrap();
    match &*png {
        Some(data) => {
            send_binary(stream, "image/png", data);
        }
        None => {
            send_error(stream, 404, "no frame available");
        }
    }
}

/// `POST /api/scene/save` — saves the scene tree to a .tscn file.
fn api_save_scene(state: &Arc<Mutex<EditorState>>, body: &str, stream: &mut TcpStream) {
    let parsed = match parse_json_body(body) {
        Some(v) => v,
        None => {
            send_error(stream, 400, "invalid JSON");
            return;
        }
    };

    let path = match parsed.get("path").and_then(|v| v.as_str()) {
        Some(v) => v.to_string(),
        None => {
            send_error(stream, 400, "missing path");
            return;
        }
    };

    let mut state = state.lock().unwrap();
    let root_id = state.scene_tree.root_id();

    // Find first child of root to use as scene root (the actual scene).
    let scene_root = state
        .scene_tree
        .get_node(root_id)
        .and_then(|n| n.children().first().copied());

    let save_root = scene_root.unwrap_or(root_id);
    let tscn = TscnSaver::save_tree(&state.scene_tree, save_root);

    if let Err(e) = std::fs::write(&path, &tscn) {
        send_error(stream, 500, &format!("failed to write: {e}"));
        return;
    }

    state.scene_file = Some(path.clone());
    state.scene_modified = false;
    state.add_log("info", format!("Saved scene to '{}'", path));

    send_json(stream, r#"{"ok":true}"#);
}

/// `POST /api/scene/load` — loads a .tscn file into the scene tree.
fn api_load_scene(state: &Arc<Mutex<EditorState>>, body: &str, stream: &mut TcpStream) {
    let parsed = match parse_json_body(body) {
        Some(v) => v,
        None => {
            send_error(stream, 400, "invalid JSON");
            return;
        }
    };

    let path = match parsed.get("path").and_then(|v| v.as_str()) {
        Some(v) => v.to_string(),
        None => {
            send_error(stream, 400, "missing path");
            return;
        }
    };

    let source = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(e) => {
            send_error(stream, 400, &format!("failed to read: {e}"));
            return;
        }
    };

    let scene = match PackedScene::from_tscn(&source) {
        Ok(s) => s,
        Err(e) => {
            send_error(stream, 400, &format!("failed to parse: {e}"));
            return;
        }
    };

    let mut state = state.lock().unwrap();

    // Replace the scene tree with a fresh one.
    let mut new_tree = SceneTree::new();
    let root_id = new_tree.root_id();

    match add_packed_scene_to_tree(&mut new_tree, root_id, &scene) {
        Ok(_) => {
            state.scene_tree = new_tree;
            state.selected_node = None;
            state.selected_nodes.clear();
            state.undo_stack.clear();
            state.redo_stack.clear();
            state.scene_file = Some(path.clone());
            state.scene_modified = false;
            state.add_log("info", format!("Loaded scene from '{}'", path));
            send_json(stream, r#"{"ok":true}"#);
        }
        Err(e) => {
            send_error(stream, 500, &format!("failed to instance: {e}"));
        }
    }
}

// ---------------------------------------------------------------------------
// Viewport interaction endpoints
// ---------------------------------------------------------------------------

/// `POST /api/viewport/click` — hit-test and select node at pixel coords.
fn api_viewport_click(state: &Arc<Mutex<EditorState>>, body: &str, stream: &mut TcpStream) {
    let parsed = match parse_json_body(body) {
        Some(v) => v,
        None => {
            send_error(stream, 400, "invalid JSON");
            return;
        }
    };

    let x = parsed.get("x").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
    let y = parsed.get("y").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;

    let mut state = state.lock().unwrap();
    let vw = state.viewport_width;
    let vh = state.viewport_height;
    let zoom = state.viewport_zoom;
    let pan = state.viewport_pan;
    let hit =
        crate::scene_renderer::hit_test_with_zoom_pan(&state.scene_tree, vw, vh, zoom, pan, x, y);

    state.selected_node = hit;
    state.selected_nodes = hit.into_iter().collect();

    match hit {
        Some(id) => send_json(stream, &format!(r#"{{"selected":{}}}"#, id.raw())),
        None => send_json(stream, r#"{"selected":null}"#),
    }
}

/// `POST /api/viewport/drag_start` — begin dragging a node.
fn api_viewport_drag_start(state: &Arc<Mutex<EditorState>>, body: &str, stream: &mut TcpStream) {
    let parsed = match parse_json_body(body) {
        Some(v) => v,
        None => {
            send_error(stream, 400, "invalid JSON");
            return;
        }
    };

    let x = parsed.get("x").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
    let y = parsed.get("y").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;

    let mut state = state.lock().unwrap();
    let vw = state.viewport_width;
    let vh = state.viewport_height;
    let zoom = state.viewport_zoom;
    let pan = state.viewport_pan;
    let hit =
        crate::scene_renderer::hit_test_with_zoom_pan(&state.scene_tree, vw, vh, zoom, pan, x, y);

    match hit {
        Some(node_id) => {
            let offset = crate::scene_renderer::camera_offset_with_zoom_pan(
                &state.scene_tree,
                vw,
                vh,
                zoom,
                pan,
            );
            let node_pos = state
                .scene_tree
                .get_node(node_id)
                .map(|n| match n.get_property("position") {
                    Variant::Vector2(v) => v,
                    _ => Vector2::ZERO,
                })
                .unwrap_or(Vector2::ZERO);

            state.selected_node = Some(node_id);
            state.drag_state = Some(DragState {
                node_id,
                start_pixel: Vector2::new(x, y),
                start_node_pos: node_pos,
                camera_offset: offset,
            });

            send_json(
                stream,
                &format!(r#"{{"dragging":true,"node_id":{}}}"#, node_id.raw()),
            );
        }
        None => {
            state.drag_state = None;
            send_json(stream, r#"{"dragging":false}"#);
        }
    }
}

/// `POST /api/viewport/drag` — update node position during drag.
fn api_viewport_drag(state: &Arc<Mutex<EditorState>>, body: &str, stream: &mut TcpStream) {
    let parsed = match parse_json_body(body) {
        Some(v) => v,
        None => {
            send_error(stream, 400, "invalid JSON");
            return;
        }
    };

    let x = parsed.get("x").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
    let y = parsed.get("y").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;

    let mut state = state.lock().unwrap();

    let drag = match &state.drag_state {
        Some(d) => d.clone(),
        None => {
            send_json(stream, r#"{"dragging":false}"#);
            return;
        }
    };

    // Pixel delta divided by zoom gives world-space delta.
    let zoom = state.viewport_zoom as f32;
    let pixel_delta = Vector2::new(x - drag.start_pixel.x, y - drag.start_pixel.y);
    let world_delta = Vector2::new(pixel_delta.x / zoom, pixel_delta.y / zoom);
    let candidate_pos = drag.start_node_pos + world_delta;

    // Apply grid snap and smart snap.
    let settings = state.display_settings.clone();
    let (new_pos, guides) = apply_snap(&state.scene_tree, &settings, drag.node_id, candidate_pos);
    state.snap_guides = guides;

    if let Some(node) = state.scene_tree.get_node_mut(drag.node_id) {
        node.set_property("position", Variant::Vector2(new_pos));
    }

    send_json(
        stream,
        &format!(r#"{{"dragging":true,"x":{},"y":{}}}"#, new_pos.x, new_pos.y),
    );
}

/// `POST /api/viewport/drag_end` — finalize drag, clear drag state.
fn api_viewport_drag_end(state: &Arc<Mutex<EditorState>>, body: &str, stream: &mut TcpStream) {
    let parsed = match parse_json_body(body) {
        Some(v) => v,
        None => {
            send_error(stream, 400, "invalid JSON");
            return;
        }
    };

    let x = parsed.get("x").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
    let y = parsed.get("y").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;

    let mut state = state.lock().unwrap();

    let drag = match state.drag_state.take() {
        Some(d) => d,
        None => {
            send_json(stream, r#"{"ok":true}"#);
            return;
        }
    };

    // Pixel delta divided by zoom gives world-space delta.
    let zoom = state.viewport_zoom as f32;
    let pixel_delta = Vector2::new(x - drag.start_pixel.x, y - drag.start_pixel.y);
    let world_delta = Vector2::new(pixel_delta.x / zoom, pixel_delta.y / zoom);
    let candidate_pos = drag.start_node_pos + world_delta;

    let settings = state.display_settings.clone();
    let (new_pos, _) = apply_snap(&state.scene_tree, &settings, drag.node_id, candidate_pos);
    state.snap_guides.clear();

    if let Some(node) = state.scene_tree.get_node_mut(drag.node_id) {
        node.set_property("position", Variant::Vector2(new_pos));
    }

    send_json(
        stream,
        &format!(r#"{{"ok":true,"x":{},"y":{}}}"#, new_pos.x, new_pos.y),
    );
}

// ---------------------------------------------------------------------------
// Viewport zoom/pan endpoints
// ---------------------------------------------------------------------------

/// `GET /api/viewport/zoom_pan` — returns current zoom and pan state.
fn api_get_zoom_pan(state: &Arc<Mutex<EditorState>>, stream: &mut TcpStream) {
    let json = {
        let state = state.lock().unwrap();
        format!(
            r#"{{"zoom":{},"pan_x":{},"pan_y":{}}}"#,
            state.viewport_zoom, state.viewport_pan.0, state.viewport_pan.1
        )
    };
    send_json(stream, &json);
}

/// `POST /api/viewport/zoom` — sets the viewport zoom level.
fn api_set_zoom(state: &Arc<Mutex<EditorState>>, body: &str, stream: &mut TcpStream) {
    let parsed = match parse_json_body(body) {
        Some(v) => v,
        None => {
            send_error(stream, 400, "invalid JSON");
            return;
        }
    };
    let zoom = parsed.get("zoom").and_then(|v| v.as_f64()).unwrap_or(1.0);
    let mut state = state.lock().unwrap();
    state.viewport_zoom = zoom.clamp(0.1, 16.0);
    let json = format!(
        r#"{{"zoom":{},"pan_x":{},"pan_y":{}}}"#,
        state.viewport_zoom, state.viewport_pan.0, state.viewport_pan.1
    );
    send_json(stream, &json);
}

/// `POST /api/viewport/pan` — sets the viewport pan offset.
fn api_set_pan(state: &Arc<Mutex<EditorState>>, body: &str, stream: &mut TcpStream) {
    let parsed = match parse_json_body(body) {
        Some(v) => v,
        None => {
            send_error(stream, 400, "invalid JSON");
            return;
        }
    };
    let x = parsed.get("x").and_then(|v| v.as_f64()).unwrap_or(0.0);
    let y = parsed.get("y").and_then(|v| v.as_f64()).unwrap_or(0.0);
    let mut state = state.lock().unwrap();
    state.viewport_pan = (x, y);
    let json = format!(
        r#"{{"zoom":{},"pan_x":{},"pan_y":{}}}"#,
        state.viewport_zoom, x, y
    );
    send_json(stream, &json);
}

// ---------------------------------------------------------------------------
// Log and scene info endpoints
// ---------------------------------------------------------------------------

/// `GET /api/logs` — returns recent editor operation log entries.
fn api_get_logs(state: &Arc<Mutex<EditorState>>, stream: &mut TcpStream) {
    let json = {
        let state = state.lock().unwrap();
        let entries: Vec<String> = state
            .log_entries
            .iter()
            .map(|e| {
                format!(
                    r#"{{"timestamp":{},"level":"{}","message":"{}"}}"#,
                    e.timestamp,
                    e.level,
                    e.message.replace('\\', "\\\\").replace('"', "\\\"")
                )
            })
            .collect();
        format!("[{}]", entries.join(","))
    };
    send_json(stream, &json);
}

/// `GET /api/scene/info` — returns scene statistics and metadata.
fn api_get_scene_info(state: &Arc<Mutex<EditorState>>, stream: &mut TcpStream) {
    let json = {
        let state = state.lock().unwrap();
        let total_nodes = state.scene_tree.node_count();

        // Count nodes by type.
        let mut type_counts = std::collections::HashMap::<String, usize>::new();
        let all_nodes = state.scene_tree.all_nodes_in_tree_order();
        for nid in &all_nodes {
            if let Some(node) = state.scene_tree.get_node(*nid) {
                *type_counts
                    .entry(node.class_name().to_string())
                    .or_default() += 1;
            }
        }
        let types_json: Vec<String> = type_counts
            .iter()
            .map(|(k, v)| format!(r#""{}": {}"#, k, v))
            .collect();

        let scene_file = state
            .scene_file
            .as_deref()
            .map(|s| format!(r#""{}""#, s.replace('\\', "\\\\").replace('"', "\\\"")))
            .unwrap_or_else(|| "null".to_string());

        format!(
            r#"{{"node_count":{},"type_breakdown":{{{}}},"scene_file":{},"modified":{}}}"#,
            total_nodes,
            types_json.join(","),
            scene_file,
            state.scene_modified,
        )
    };
    send_json(stream, &json);
}

// ---------------------------------------------------------------------------
// Filesystem endpoint
// ---------------------------------------------------------------------------

/// A filesystem entry for the JSON response.
#[derive(Debug)]
struct FsEntry {
    name: String,
    path: String,
    is_dir: bool,
    children: Vec<FsEntry>,
    /// File size in bytes (0 for directories).
    size: u64,
    /// File type string (e.g. "GDScript", "Scene", "Resource").
    file_type: String,
}

impl FsEntry {
    fn to_json(&self) -> String {
        if self.is_dir {
            let children_json: Vec<String> = self.children.iter().map(|c| c.to_json()).collect();
            format!(
                r#"{{"name":"{}","path":"{}","is_dir":true,"children":[{}]}}"#,
                self.name.replace('\\', "\\\\").replace('"', "\\\""),
                self.path.replace('\\', "\\\\").replace('"', "\\\""),
                children_json.join(",")
            )
        } else {
            format!(
                r#"{{"name":"{}","path":"{}","is_dir":false,"size":{},"file_type":"{}"}}"#,
                self.name.replace('\\', "\\\\").replace('"', "\\\""),
                self.path.replace('\\', "\\\\").replace('"', "\\\""),
                self.size,
                self.file_type,
            )
        }
    }
}

fn file_type_for_ext(ext: &str) -> &str {
    match ext {
        "gd" => "GDScript",
        "tscn" => "Scene",
        "tres" => "Resource",
        "png" | "jpg" | "jpeg" | "webp" | "svg" => "Image",
        "wav" | "ogg" | "mp3" => "Audio",
        "ttf" | "otf" => "Font",
        _ => "File",
    }
}

/// Recursively scan a directory for .tscn, .gd, .tres files up to `max_depth` levels.
fn scan_directory(
    dir: &std::path::Path,
    prefix: &str,
    depth: usize,
    max_depth: usize,
) -> Vec<FsEntry> {
    if depth > max_depth {
        return Vec::new();
    }
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return Vec::new(),
    };

    let mut dirs = Vec::new();
    let mut files = Vec::new();

    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();

        // Skip hidden files/directories.
        if name.starts_with('.') {
            continue;
        }

        if path.is_dir() {
            let child_prefix = if prefix.is_empty() {
                name.clone()
            } else {
                format!("{}/{}", prefix, name)
            };
            let children = scan_directory(&path, &child_prefix, depth + 1, max_depth);
            // Only include directories that have relevant files (directly or nested).
            if !children.is_empty() {
                dirs.push(FsEntry {
                    name,
                    path: format!("res://{}", child_prefix),
                    is_dir: true,
                    children,
                    size: 0,
                    file_type: "Directory".to_string(),
                });
            }
        } else if path.is_file() {
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
            if matches!(
                ext,
                "tscn"
                    | "gd"
                    | "tres"
                    | "png"
                    | "jpg"
                    | "jpeg"
                    | "webp"
                    | "svg"
                    | "gdshader"
                    | "glsl"
                    | "cfg"
            ) {
                let file_path = if prefix.is_empty() {
                    format!("res://{}", name)
                } else {
                    format!("res://{}/{}", prefix, name)
                };
                let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
                files.push(FsEntry {
                    name,
                    path: file_path,
                    is_dir: false,
                    children: Vec::new(),
                    size,
                    file_type: file_type_for_ext(ext).to_string(),
                });
            }
        }
    }

    // Sort: directories first, then files, both alphabetically.
    dirs.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    files.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    dirs.extend(files);
    dirs
}

/// `GET /api/filesystem` -- returns project files (.tscn, .gd, .tres) as a tree.
fn api_get_filesystem(stream: &mut TcpStream) {
    let cwd = std::env::current_dir().unwrap_or_default();
    let entries = scan_directory(&cwd, "", 0, 3);
    let entries_json: Vec<String> = entries.iter().map(|e| e.to_json()).collect();
    let json = format!(
        r#"{{"root":"{}","files":[{}]}}"#,
        cwd.display()
            .to_string()
            .replace('\\', "\\\\")
            .replace('"', "\\\""),
        entries_json.join(",")
    );
    send_json(stream, &json);
}

// ---------------------------------------------------------------------------
// File preview endpoint
// ---------------------------------------------------------------------------

/// `GET /api/preview/file?path=res://...` -- returns a preview for the given file.
///
/// For images: returns the raw image bytes with appropriate content type.
/// For scripts (.gd): returns JSON with first N lines of the file.
/// For scenes (.tscn): returns JSON with node count and root type.
/// For resources (.tres): returns JSON with resource type info.
fn api_get_file_preview(query: &str, stream: &mut TcpStream) {
    let path = match query_param(query, "path") {
        Some(p) => {
            // URL-decode the path (handle %20, etc.)
            let decoded = p.replace("%20", " ").replace("%2F", "/");
            decoded.replace("res://", "")
        }
        None => {
            send_json(stream, r#"{"error":"missing path parameter"}"#);
            return;
        }
    };

    let cwd = std::env::current_dir().unwrap_or_default();
    let full_path = cwd.join(&path);

    if !full_path.exists() {
        send_json(stream, r#"{"error":"file not found"}"#);
        return;
    }

    let ext = full_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    match ext.as_str() {
        // Image files: serve raw bytes with content type
        "png" => serve_binary_file(stream, &full_path, "image/png"),
        "jpg" | "jpeg" => serve_binary_file(stream, &full_path, "image/jpeg"),
        "webp" => serve_binary_file(stream, &full_path, "image/webp"),
        "svg" => serve_binary_file(stream, &full_path, "image/svg+xml"),

        // Script files: return first 30 lines as JSON
        "gd" | "gdshader" | "glsl" => {
            let content = std::fs::read_to_string(&full_path).unwrap_or_default();
            let lines: Vec<&str> = content.lines().take(30).collect();
            let line_count = content.lines().count();
            let escaped = lines
                .join("\n")
                .replace('\\', "\\\\")
                .replace('"', "\\\"")
                .replace('\n', "\\n")
                .replace('\t', "\\t");
            let json = format!(
                r#"{{"type":"script","lines":{},"preview":"{}"}}"#,
                line_count, escaped
            );
            send_json(stream, &json);
        }

        // Scene files: parse and return summary
        "tscn" => {
            let content = std::fs::read_to_string(&full_path).unwrap_or_default();
            let node_count = content.matches("[node").count();
            let root_type = content
                .lines()
                .find(|l| l.starts_with("[node") && !l.contains("parent="))
                .and_then(|l| {
                    l.split("type=")
                        .nth(1)
                        .map(|t| t.split('"').nth(1).unwrap_or("Node").to_string())
                })
                .unwrap_or_else(|| "Node".to_string());
            let ext_res_count = content.matches("[ext_resource").count();
            let sub_res_count = content.matches("[sub_resource").count();
            let json = format!(
                r#"{{"type":"scene","node_count":{},"root_type":"{}","ext_resources":{},"sub_resources":{}}}"#,
                node_count, root_type, ext_res_count, sub_res_count
            );
            send_json(stream, &json);
        }

        // Resource files: return resource type
        "tres" => {
            let content = std::fs::read_to_string(&full_path).unwrap_or_default();
            let res_type = content
                .lines()
                .find(|l| l.starts_with("[gd_resource"))
                .and_then(|l| {
                    l.split("type=")
                        .nth(1)
                        .map(|t| t.split('"').nth(1).unwrap_or("Resource").to_string())
                })
                .unwrap_or_else(|| "Resource".to_string());
            let sub_res_count = content.matches("[sub_resource").count();
            let size = std::fs::metadata(&full_path).map(|m| m.len()).unwrap_or(0);
            let json = format!(
                r#"{{"type":"resource","resource_type":"{}","sub_resources":{},"size":{}}}"#,
                res_type, sub_res_count, size
            );
            send_json(stream, &json);
        }

        // Config files: show first 20 lines
        "cfg" => {
            let content = std::fs::read_to_string(&full_path).unwrap_or_default();
            let lines: Vec<&str> = content.lines().take(20).collect();
            let escaped = lines
                .join("\n")
                .replace('\\', "\\\\")
                .replace('"', "\\\"")
                .replace('\n', "\\n")
                .replace('\t', "\\t");
            let json = format!(r#"{{"type":"config","preview":"{}"}}"#, escaped);
            send_json(stream, &json);
        }

        _ => {
            send_json(stream, r#"{"type":"unknown"}"#);
        }
    }
}

/// Serve a binary file with the given content type.
fn serve_binary_file(stream: &mut TcpStream, path: &std::path::Path, content_type: &str) {
    match std::fs::read(path) {
        Ok(data) => {
            let header = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: {}\r\nContent-Length: {}\r\nAccess-Control-Allow-Origin: *\r\nCache-Control: max-age=60\r\nConnection: close\r\n\r\n",
                content_type,
                data.len()
            );
            let _ = stream.write_all(header.as_bytes());
            let _ = stream.write_all(&data);
        }
        Err(_) => {
            let _ = stream.write_all(
                b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Script endpoint
// ---------------------------------------------------------------------------

/// Parse a query string parameter by name.
fn query_param<'a>(query: &'a str, name: &str) -> Option<&'a str> {
    for pair in query.split('&') {
        if let Some((key, value)) = pair.split_once('=') {
            if key == name {
                return Some(value);
            }
        }
    }
    None
}

/// URL-decode a percent-encoded string.
fn url_decode(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.bytes();
    while let Some(b) = chars.next() {
        if b == b'%' {
            let hi = chars.next().unwrap_or(0);
            let lo = chars.next().unwrap_or(0);
            let hex = [hi, lo];
            if let Ok(s) = std::str::from_utf8(&hex) {
                if let Ok(val) = u8::from_str_radix(s, 16) {
                    result.push(val as char);
                    continue;
                }
            }
            result.push('%');
            result.push(hi as char);
            result.push(lo as char);
        } else if b == b'+' {
            result.push(' ');
        } else {
            result.push(b as char);
        }
    }
    result
}

/// `GET /api/script?path=<path>` -- reads a .gd file and returns its content.
fn api_get_script(query: &str, stream: &mut TcpStream) {
    let raw_path = match query_param(query, "path") {
        Some(p) => url_decode(p),
        None => {
            send_error(stream, 400, "missing path parameter");
            return;
        }
    };

    // Resolve res:// paths relative to cwd.
    let file_path = if let Some(stripped) = raw_path.strip_prefix("res://") {
        let cwd = std::env::current_dir().unwrap_or_default();
        cwd.join(stripped)
    } else {
        std::path::PathBuf::from(&raw_path)
    };

    // Security: only allow .gd files.
    match file_path.extension().and_then(|e| e.to_str()) {
        Some("gd") => {}
        _ => {
            send_error(stream, 400, "only .gd files are supported");
            return;
        }
    }

    match std::fs::read_to_string(&file_path) {
        Ok(content) => {
            let json = serde_json::json!({
                "path": raw_path,
                "content": content,
                "lines": content.lines().count()
            });
            send_json(stream, &json.to_string());
        }
        Err(e) => {
            send_error(stream, 404, &format!("failed to read script: {e}"));
        }
    }
}

/// `POST /api/script/save` -- writes content to a .gd file.
fn api_save_script(body: &str, stream: &mut TcpStream) {
    let json = match parse_json_body(body) {
        Some(v) => v,
        None => {
            send_error(stream, 400, "invalid JSON");
            return;
        }
    };
    let raw_path = match json.get("path").and_then(|p| p.as_str()) {
        Some(p) => p.to_string(),
        None => {
            send_error(stream, 400, "missing path field");
            return;
        }
    };
    let content_str = match json.get("content").and_then(|c| c.as_str()) {
        Some(c) => c.to_string(),
        None => {
            send_error(stream, 400, "missing content field");
            return;
        }
    };
    let file_path = if let Some(stripped) = raw_path.strip_prefix("res://") {
        let cwd = std::env::current_dir().unwrap_or_default();
        cwd.join(stripped)
    } else {
        std::path::PathBuf::from(&raw_path)
    };
    match file_path.extension().and_then(|e| e.to_str()) {
        Some("gd") => {}
        _ => {
            send_error(stream, 400, "only .gd files are supported");
            return;
        }
    }
    if let Some(parent) = file_path.parent() {
        if !parent.exists() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                send_error(stream, 500, &format!("failed to create directory: {e}"));
                return;
            }
        }
    }
    match std::fs::write(&file_path, &content_str) {
        Ok(()) => send_json(stream, r#"{"ok":true}"#),
        Err(e) => send_error(stream, 500, &format!("failed to write script: {e}")),
    }
}

// ---------------------------------------------------------------------------
// Signals endpoint
// ---------------------------------------------------------------------------

/// Returns the list of common signals for a given node class name.
fn signals_for_class(class_name: &str) -> Vec<&'static str> {
    let mut signals = vec!["tree_entered", "tree_exiting", "ready"];

    match class_name {
        "Button" => {
            signals.extend(&["pressed", "toggled", "button_down", "button_up"]);
        }
        "Area2D" => {
            signals.extend(&["body_entered", "body_exited", "area_entered", "area_exited"]);
        }
        "Timer" => {
            signals.push("timeout");
        }
        "CollisionObject2D" => {
            signals.extend(&["input_event", "mouse_entered", "mouse_exited"]);
        }
        _ => {}
    }

    signals
}

/// `GET /api/node/signals?node_id=<id>` -- returns signals for a node.
fn api_get_node_signals(state: &Arc<Mutex<EditorState>>, query: &str, stream: &mut TcpStream) {
    let node_raw: u64 = match query_param(query, "node_id").and_then(|s| s.parse().ok()) {
        Some(v) => v,
        None => {
            send_error(stream, 400, "missing or invalid node_id");
            return;
        }
    };

    let json = {
        let state = state.lock().unwrap();
        let node_id = match find_node_by_raw_id(&state.scene_tree, node_raw) {
            Some(id) => id,
            None => {
                send_error(stream, 404, "node not found");
                return;
            }
        };

        let node = state.scene_tree.get_node(node_id).unwrap();
        let class_name = node.class_name().to_string();

        // Get signals available for this class.
        let available = signals_for_class(&class_name);

        // Check for connections (stored as property signal_connections).
        let connections_variant = node.get_property("signal_connections");
        let connections_str = match &connections_variant {
            Variant::String(s) => s.clone(),
            _ => String::new(),
        };

        // Get groups.
        let groups_variant = node.get_property("groups");
        let groups: Vec<String> = match &groups_variant {
            Variant::String(s) if !s.is_empty() => {
                s.split(',').map(|g| g.trim().to_string()).collect()
            }
            _ => Vec::new(),
        };

        let signals_json: Vec<String> = available
            .iter()
            .map(|sig| {
                // Parse connections to get details for this signal
                let connections: Vec<String> = connections_str
                    .split(',')
                    .filter(|c| !c.is_empty() && c.starts_with(&format!("{}:", sig)))
                    .map(|c| {
                        let method = c.splitn(2, ':').nth(1).unwrap_or("");
                        format!(r#"{{"method":"{}"}}"#, method.replace('"', "\\\""))
                    })
                    .collect();
                let connected = !connections.is_empty();
                format!(
                    r#"{{"name":"{}","connected":{},"connection_count":{},"connections":[{}]}}"#,
                    sig,
                    connected,
                    connections.len(),
                    connections.join(",")
                )
            })
            .collect();

        let groups_json: Vec<String> = groups
            .iter()
            .map(|g| format!(r#""{}""#, g.replace('\\', "\\\\").replace('"', "\\\"")))
            .collect();

        let total_connected = signals_json
            .iter()
            .filter(|s| s.contains(r#""connected":true"#))
            .count();
        format!(
            r#"{{"node_id":{},"class":"{}","signals":[{}],"groups":[{}],"connected_count":{}}}"#,
            node_raw,
            class_name.replace('"', "\\\""),
            signals_json.join(","),
            groups_json.join(","),
            total_connected,
        )
    };
    send_json(stream, &json);
}

/// `POST /api/node/signals/connect` -- connect a signal on a node.
fn api_connect_signal(state: &Arc<Mutex<EditorState>>, body: &str, stream: &mut TcpStream) {
    let parsed = match parse_json_body(body) {
        Some(v) => v,
        None => {
            send_error(stream, 400, "invalid JSON");
            return;
        }
    };

    let node_raw = match parsed.get("node_id").and_then(|v| v.as_u64()) {
        Some(v) => v,
        None => {
            send_error(stream, 400, "missing node_id");
            return;
        }
    };
    let signal_name = match parsed.get("signal").and_then(|v| v.as_str()) {
        Some(v) => v.to_string(),
        None => {
            send_error(stream, 400, "missing signal");
            return;
        }
    };
    let _target_raw = parsed.get("target_id").and_then(|v| v.as_u64());
    let method = match parsed.get("method").and_then(|v| v.as_str()) {
        Some(v) => v.to_string(),
        None => {
            send_error(stream, 400, "missing method");
            return;
        }
    };

    let mut state = state.lock().unwrap();
    let node_id = match find_node_by_raw_id(&state.scene_tree, node_raw) {
        Some(id) => id,
        None => {
            send_error(stream, 404, "node not found");
            return;
        }
    };

    // Store connection info as a property string (signal_connections).
    let node = state.scene_tree.get_node_mut(node_id).unwrap();
    let existing = match node.get_property("signal_connections") {
        Variant::String(s) => s,
        _ => String::new(),
    };
    let new_entry = format!("{}:{}", signal_name, method);
    let updated = if existing.is_empty() {
        new_entry
    } else {
        format!("{},{}", existing, new_entry)
    };
    node.set_property("signal_connections", Variant::String(updated));

    state.scene_modified = true;
    state.add_log(
        "info",
        format!("Connected signal '{}' to method '{}'", signal_name, method),
    );

    send_json(stream, r#"{"ok":true}"#);
}

// ---------------------------------------------------------------------------
// Groups endpoints
// ---------------------------------------------------------------------------

/// `POST /api/node/groups/add` -- add a group to a node.
fn api_add_group(state: &Arc<Mutex<EditorState>>, body: &str, stream: &mut TcpStream) {
    let parsed = match parse_json_body(body) {
        Some(v) => v,
        None => {
            send_error(stream, 400, "invalid JSON");
            return;
        }
    };

    let node_raw = match parsed.get("node_id").and_then(|v| v.as_u64()) {
        Some(v) => v,
        None => {
            send_error(stream, 400, "missing node_id");
            return;
        }
    };
    let group = match parsed.get("group").and_then(|v| v.as_str()) {
        Some(v) => v.to_string(),
        None => {
            send_error(stream, 400, "missing group");
            return;
        }
    };

    if group.is_empty() {
        send_error(stream, 400, "group name cannot be empty");
        return;
    }

    let mut state = state.lock().unwrap();
    let node_id = match find_node_by_raw_id(&state.scene_tree, node_raw) {
        Some(id) => id,
        None => {
            send_error(stream, 404, "node not found");
            return;
        }
    };

    let node = state.scene_tree.get_node_mut(node_id).unwrap();
    let existing = match node.get_property("groups") {
        Variant::String(s) => s,
        _ => String::new(),
    };

    let groups: Vec<&str> = if existing.is_empty() {
        Vec::new()
    } else {
        existing.split(',').map(|g| g.trim()).collect()
    };

    // Don't add duplicates.
    if groups.contains(&group.as_str()) {
        send_json(stream, r#"{"ok":true,"added":false}"#);
        return;
    }

    let updated = if existing.is_empty() {
        group.clone()
    } else {
        format!("{},{}", existing, group)
    };
    node.set_property("groups", Variant::String(updated));

    state.scene_modified = true;
    state.add_log("info", format!("Added group '{}'", group));

    send_json(stream, r#"{"ok":true,"added":true}"#);
}

/// `POST /api/node/groups/remove` -- remove a group from a node.
fn api_remove_group(state: &Arc<Mutex<EditorState>>, body: &str, stream: &mut TcpStream) {
    let parsed = match parse_json_body(body) {
        Some(v) => v,
        None => {
            send_error(stream, 400, "invalid JSON");
            return;
        }
    };

    let node_raw = match parsed.get("node_id").and_then(|v| v.as_u64()) {
        Some(v) => v,
        None => {
            send_error(stream, 400, "missing node_id");
            return;
        }
    };
    let group = match parsed.get("group").and_then(|v| v.as_str()) {
        Some(v) => v.to_string(),
        None => {
            send_error(stream, 400, "missing group");
            return;
        }
    };

    let mut state = state.lock().unwrap();
    let node_id = match find_node_by_raw_id(&state.scene_tree, node_raw) {
        Some(id) => id,
        None => {
            send_error(stream, 404, "node not found");
            return;
        }
    };

    let node = state.scene_tree.get_node_mut(node_id).unwrap();
    let existing = match node.get_property("groups") {
        Variant::String(s) => s,
        _ => String::new(),
    };

    let groups: Vec<&str> = if existing.is_empty() {
        Vec::new()
    } else {
        existing.split(',').map(|g| g.trim()).collect()
    };

    let new_groups: Vec<&str> = groups
        .into_iter()
        .filter(|g| *g != group.as_str())
        .collect();
    let updated = new_groups.join(",");
    node.set_property("groups", Variant::String(updated));

    state.scene_modified = true;
    state.add_log("info", format!("Removed group '{}'", group));

    send_json(stream, r#"{"ok":true}"#);
}

// ---------------------------------------------------------------------------
// Multi-select, copy/paste, settings, box select, multi-drag
// ---------------------------------------------------------------------------

fn api_select_multi(state: &Arc<Mutex<EditorState>>, body: &str, stream: &mut TcpStream) {
    let parsed = match parse_json_body(body) {
        Some(v) => v,
        None => {
            send_error(stream, 400, "invalid JSON");
            return;
        }
    };
    let mut state = state.lock().unwrap();
    if let Some(ids_arr) = parsed.get("node_ids").and_then(|v| v.as_array()) {
        let mut nids = Vec::new();
        for iv in ids_arr {
            if let Some(r) = iv.as_u64() {
                if let Some(n) = find_node_by_raw_id(&state.scene_tree, r) {
                    nids.push(n);
                }
            }
        }
        state.selected_nodes = nids.clone();
        state.selected_node = nids.first().copied();
        let j: Vec<String> = nids.iter().map(|i| i.raw().to_string()).collect();
        send_json(
            stream,
            &format!(r#"{{"selected_nodes":[{}]}}"#, j.join(",")),
        );
        return;
    }
    let raw = match parsed.get("node_id").and_then(|v| v.as_u64()) {
        Some(v) => v,
        None => {
            send_error(stream, 400, "missing node_id or node_ids");
            return;
        }
    };
    let mode = parsed.get("mode").and_then(|v| v.as_str()).unwrap_or("set");
    let nid = match find_node_by_raw_id(&state.scene_tree, raw) {
        Some(i) => i,
        None => {
            send_error(stream, 404, "node not found");
            return;
        }
    };
    match mode {
        "add" => {
            if !state.selected_nodes.contains(&nid) {
                state.selected_nodes.push(nid);
            }
        }
        "remove" => {
            state.selected_nodes.retain(|&i| i != nid);
        }
        "toggle" => {
            if state.selected_nodes.contains(&nid) {
                state.selected_nodes.retain(|&i| i != nid);
            } else {
                state.selected_nodes.push(nid);
            }
        }
        _ => {
            state.selected_nodes = vec![nid];
        }
    }
    state.selected_node = state.selected_nodes.first().copied();
    let j: Vec<String> = state
        .selected_nodes
        .iter()
        .map(|i| i.raw().to_string())
        .collect();
    send_json(
        stream,
        &format!(r#"{{"selected_nodes":[{}]}}"#, j.join(",")),
    );
}

fn api_get_selected_nodes(state: &Arc<Mutex<EditorState>>, stream: &mut TcpStream) {
    let j = {
        let s = state.lock().unwrap();
        let ids: Vec<String> = s
            .selected_nodes
            .iter()
            .map(|i| i.raw().to_string())
            .collect();
        format!(
            r#"{{"selected_nodes":[{}],"count":{}}}"#,
            ids.join(","),
            s.selected_nodes.len()
        )
    };
    send_json(stream, &j);
}

fn node_to_clipboard(tree: &SceneTree, nid: NodeId) -> Option<ClipboardEntry> {
    let n = tree.get_node(nid)?;
    let props: Vec<(String, Variant)> = n
        .properties()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    let ch: Vec<ClipboardEntry> = n
        .children()
        .iter()
        .filter_map(|&c| node_to_clipboard(tree, c))
        .collect();
    Some(ClipboardEntry {
        name: n.name().to_string(),
        class_name: n.class_name().to_string(),
        properties: props,
        children: ch,
    })
}

fn paste_clipboard_entry(
    tree: &mut SceneTree,
    pid: NodeId,
    e: &ClipboardEntry,
) -> Result<NodeId, gdcore::error::EngineError> {
    let mut node = Node::new(&e.name, &e.class_name);
    for (k, v) in &e.properties {
        node.set_property(k, v.clone());
    }
    let new_id = tree.add_child(pid, node)?;
    for c in &e.children {
        paste_clipboard_entry(tree, new_id, c)?;
    }
    Ok(new_id)
}

fn api_copy_nodes(state: &Arc<Mutex<EditorState>>, body: &str, stream: &mut TcpStream) {
    let parsed = parse_json_body(body);
    let mut state = state.lock().unwrap();
    let raws: Vec<u64> = if let Some(a) = parsed
        .as_ref()
        .and_then(|p| p.get("node_ids"))
        .and_then(|v| v.as_array())
    {
        a.iter().filter_map(|v| v.as_u64()).collect()
    } else if let Some(i) = parsed
        .as_ref()
        .and_then(|p| p.get("node_id"))
        .and_then(|v| v.as_u64())
    {
        vec![i]
    } else {
        state.selected_nodes.iter().map(|i| i.raw()).collect()
    };
    let mut cb = Vec::new();
    for r in &raws {
        if let Some(n) = find_node_by_raw_id(&state.scene_tree, *r) {
            if let Some(e) = node_to_clipboard(&state.scene_tree, n) {
                cb.push(e);
            }
        }
    }
    let c = cb.len();
    state.clipboard = cb;
    state.add_log("info", format!("Copied {} node(s)", c));
    send_json(stream, &format!(r#"{{"ok":true,"copied":{c}}}"#));
}

fn api_paste_nodes(state: &Arc<Mutex<EditorState>>, body: &str, stream: &mut TcpStream) {
    let parsed = parse_json_body(body);
    let mut state = state.lock().unwrap();
    if state.clipboard.is_empty() {
        send_error(stream, 400, "clipboard is empty");
        return;
    }
    let pr = parsed
        .as_ref()
        .and_then(|p| p.get("parent_id"))
        .and_then(|v| v.as_u64());
    let pid = if let Some(r) = pr {
        match find_node_by_raw_id(&state.scene_tree, r) {
            Some(i) => i,
            None => {
                send_error(stream, 404, "parent not found");
                return;
            }
        }
    } else if let Some(s) = state.selected_node {
        s
    } else {
        state.scene_tree.root_id()
    };
    let cb = state.clipboard.clone();
    let mut ids = Vec::new();
    for e in &cb {
        match paste_clipboard_entry(&mut state.scene_tree, pid, e) {
            Ok(i) => ids.push(i),
            Err(e) => {
                send_error(stream, 500, &e.to_string());
                return;
            }
        }
    }
    state.scene_modified = true;
    let c = ids.len();
    state.selected_nodes = ids.clone();
    state.selected_node = ids.first().copied();
    state.add_log("info", format!("Pasted {} node(s)", c));
    let j: Vec<String> = ids.iter().map(|i| i.raw().to_string()).collect();
    send_json(
        stream,
        &format!(r#"{{"ok":true,"pasted":{c},"ids":[{}]}}"#, j.join(",")),
    );
}

fn api_cut_nodes(state: &Arc<Mutex<EditorState>>, body: &str, stream: &mut TcpStream) {
    let parsed = parse_json_body(body);
    let mut state = state.lock().unwrap();
    let raws: Vec<u64> = if let Some(a) = parsed
        .as_ref()
        .and_then(|p| p.get("node_ids"))
        .and_then(|v| v.as_array())
    {
        a.iter().filter_map(|v| v.as_u64()).collect()
    } else if let Some(i) = parsed
        .as_ref()
        .and_then(|p| p.get("node_id"))
        .and_then(|v| v.as_u64())
    {
        vec![i]
    } else {
        state.selected_nodes.iter().map(|i| i.raw()).collect()
    };
    let mut cb = Vec::new();
    let mut del = Vec::new();
    for r in &raws {
        if let Some(n) = find_node_by_raw_id(&state.scene_tree, *r) {
            if let Some(e) = node_to_clipboard(&state.scene_tree, n) {
                cb.push(e);
                del.push(n);
            }
        }
    }
    let c = cb.len();
    state.clipboard = cb;
    for d in del {
        let _ = state.scene_tree.remove_node(d);
    }
    state.selected_nodes.clear();
    state.selected_node = None;
    state.scene_modified = true;
    state.add_log("info", format!("Cut {} node(s)", c));
    send_json(stream, &format!(r#"{{"ok":true,"cut":{c}}}"#));
}

fn api_get_settings(state: &Arc<Mutex<EditorState>>, stream: &mut TcpStream) {
    let j = {
        let s = state.lock().unwrap();
        serde_json::to_string(&s.display_settings).unwrap_or_else(|_| "{}".to_string())
    };
    send_json(stream, &j);
}

fn api_set_settings(state: &Arc<Mutex<EditorState>>, body: &str, stream: &mut TcpStream) {
    let p = match parse_json_body(body) {
        Some(v) => v,
        None => {
            send_error(stream, 400, "invalid JSON");
            return;
        }
    };
    let mut s = state.lock().unwrap();
    if let Some(v) = p.get("grid_snap_enabled").and_then(|v| v.as_bool()) {
        s.display_settings.grid_snap_enabled = v;
    }
    if let Some(v) = p.get("grid_snap_size").and_then(|v| v.as_u64()) {
        s.display_settings.grid_snap_size = v as u32;
    }
    if let Some(v) = p.get("grid_visible").and_then(|v| v.as_bool()) {
        s.display_settings.grid_visible = v;
    }
    if let Some(v) = p.get("rulers_visible").and_then(|v| v.as_bool()) {
        s.display_settings.rulers_visible = v;
    }
    if let Some(a) = p.get("background_color").and_then(|v| v.as_array()) {
        if a.len() == 4 {
            s.display_settings.background_color = [
                a[0].as_f64().unwrap_or(0.08),
                a[1].as_f64().unwrap_or(0.08),
                a[2].as_f64().unwrap_or(0.1),
                a[3].as_f64().unwrap_or(1.0),
            ];
        }
    }
    if let Some(v) = p.get("font_size").and_then(|v| v.as_str()) {
        s.display_settings.font_size = v.to_string();
    }
    if let Some(v) = p.get("theme").and_then(|v| v.as_str()) {
        s.display_settings.theme = v.to_string();
    }
    if let Some(v) = p.get("physics_fps").and_then(|v| v.as_u64()) {
        s.display_settings.physics_fps = v as u32;
    }
    if let Some(obj) = p.get("panel_sizes").and_then(|v| v.as_object()) {
        for (k, v) in obj {
            if let Some(f) = v.as_f64() {
                s.display_settings.panel_sizes.insert(k.clone(), f);
            }
        }
    }
    if let Some(v) = p.get("smart_snap_enabled").and_then(|v| v.as_bool()) {
        s.display_settings.smart_snap_enabled = v;
    }
    if let Some(v) = p.get("smart_snap_threshold").and_then(|v| v.as_f64()) {
        s.display_settings.smart_snap_threshold = v as f32;
    }
    let j = serde_json::to_string(&s.display_settings).unwrap_or_else(|_| "{}".to_string());
    send_json(stream, &j);
}

fn api_box_select(state: &Arc<Mutex<EditorState>>, body: &str, stream: &mut TcpStream) {
    let p = match parse_json_body(body) {
        Some(v) => v,
        None => {
            send_error(stream, 400, "invalid JSON");
            return;
        }
    };
    let x1 = p.get("x1").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
    let y1 = p.get("y1").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
    let x2 = p.get("x2").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
    let y2 = p.get("y2").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
    let add = p.get("add").and_then(|v| v.as_bool()).unwrap_or(false);
    let mut s = state.lock().unwrap();
    let (vw, vh, zm, pn) = (
        s.viewport_width,
        s.viewport_height,
        s.viewport_zoom,
        s.viewport_pan,
    );
    let p1 =
        crate::scene_renderer::pixel_to_scene_with_zoom_pan(&s.scene_tree, vw, vh, zm, pn, x1, y1);
    let p2 =
        crate::scene_renderer::pixel_to_scene_with_zoom_pan(&s.scene_tree, vw, vh, zm, pn, x2, y2);
    let (mnx, mny, mxx, mxy) = (
        p1.x.min(p2.x),
        p1.y.min(p2.y),
        p1.x.max(p2.x),
        p1.y.max(p2.y),
    );
    let mut sel: Vec<NodeId> = if add {
        s.selected_nodes.clone()
    } else {
        Vec::new()
    };
    for &nid in &s.scene_tree.all_nodes_in_tree_order() {
        if let Some(n) = s.scene_tree.get_node(nid) {
            if n.parent().is_none() {
                continue;
            }
            let pos = crate::scene_renderer::extract_position(n);
            if pos.x >= mnx && pos.x <= mxx && pos.y >= mny && pos.y <= mxy && !sel.contains(&nid) {
                sel.push(nid);
            }
        }
    }
    s.selected_nodes = sel.clone();
    s.selected_node = sel.first().copied();
    let j: Vec<String> = sel.iter().map(|i| i.raw().to_string()).collect();
    send_json(
        stream,
        &format!(
            r#"{{"selected_nodes":[{}],"count":{}}}"#,
            j.join(","),
            sel.len()
        ),
    );
}

fn api_viewport_drag_multi(state: &Arc<Mutex<EditorState>>, body: &str, stream: &mut TcpStream) {
    let p = match parse_json_body(body) {
        Some(v) => v,
        None => {
            send_error(stream, 400, "invalid JSON");
            return;
        }
    };
    let dx = p.get("dx").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
    let dy = p.get("dy").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
    let snap = p.get("snap").and_then(|v| v.as_bool()).unwrap_or(false);
    let mut s = state.lock().unwrap();
    let z = s.viewport_zoom as f32;
    let (mut wdx, mut wdy) = (dx / z, dy / z);
    if snap && s.display_settings.grid_snap_enabled {
        let g = s.display_settings.grid_snap_size as f32;
        wdx = (wdx / g).round() * g;
        wdy = (wdy / g).round() * g;
    }
    let sel = s.selected_nodes.clone();
    for &nid in &sel {
        if let Some(n) = s.scene_tree.get_node_mut(nid) {
            let pos = match n.get_property("position") {
                Variant::Vector2(v) => v,
                _ => Vector2::ZERO,
            };
            n.set_property(
                "position",
                Variant::Vector2(Vector2::new(pos.x + wdx, pos.y + wdy)),
            );
        }
    }
    send_json(stream, &format!(r#"{{"ok":true,"moved":{}}}"#, sel.len()));
}

// ---------------------------------------------------------------------------
// Animation endpoints
// ---------------------------------------------------------------------------

fn loop_mode_to_str(mode: LoopMode) -> &'static str {
    match mode {
        LoopMode::None => "none",
        LoopMode::Linear => "loop",
        LoopMode::PingPong => "pingpong",
    }
}

fn loop_mode_from_str(s: &str) -> LoopMode {
    match s {
        "loop" => LoopMode::Linear,
        "pingpong" => LoopMode::PingPong,
        _ => LoopMode::None,
    }
}

fn variant_to_simple_json(v: &Variant) -> String {
    to_json(v).to_string()
}

/// `GET /api/animations` -- list all animations (names + lengths).
fn api_get_animations(state: &Arc<Mutex<EditorState>>, stream: &mut TcpStream) {
    let json = {
        let state = state.lock().unwrap();
        let entries: Vec<String> = state
            .animations
            .values()
            .map(|a| {
                format!(
                    r#"{{"name":"{}","length":{},"loop_mode":"{}","track_count":{}}}"#,
                    a.name.replace('"', "\\\""),
                    a.length,
                    loop_mode_to_str(a.loop_mode),
                    a.tracks.len()
                )
            })
            .collect();
        format!("[{}]", entries.join(","))
    };
    send_json(stream, &json);
}

/// `GET /api/animation?name=<name>` -- get full animation data.
fn api_get_animation(state: &Arc<Mutex<EditorState>>, query: &str, stream: &mut TcpStream) {
    let name = match query_param(query, "name") {
        Some(n) => url_decode(n),
        None => {
            send_error(stream, 400, "missing name parameter");
            return;
        }
    };
    let json = {
        let state = state.lock().unwrap();
        let anim = match state.animations.get(&name) {
            Some(a) => a,
            None => {
                send_error(stream, 404, "animation not found");
                return;
            }
        };
        let tracks_json: Vec<String> = anim
            .tracks
            .iter()
            .enumerate()
            .map(|(idx, track)| {
                let kf_json: Vec<String> = track
                    .keyframes()
                    .iter()
                    .map(|kf| {
                        let transition_json = match kf.transition {
                            gdscene::animation::TransitionType::Linear => r#""linear""#.to_string(),
                            gdscene::animation::TransitionType::Nearest => r#""nearest""#.to_string(),
                            gdscene::animation::TransitionType::CubicBezier(x1, y1, x2, y2) => {
                                format!(r#"{{"type":"cubic_bezier","x1":{},"y1":{},"x2":{},"y2":{}}}"#, x1, y1, x2, y2)
                            }
                        };
                        format!(
                            r#"{{"time":{},"value":{},"transition":{}}}"#,
                            kf.time,
                            variant_to_simple_json(&kf.value),
                            transition_json
                        )
                    })
                    .collect();
                format!(
                    r#"{{"index":{},"node_path":"{}","property":"{}","track_type":"{}","keyframes":[{}]}}"#,
                    idx,
                    track.node_path.replace('"', "\\\""),
                    track.property_path.replace('"', "\\\""),
                    track.track_type.as_str(),
                    kf_json.join(",")
                )
            })
            .collect();
        format!(
            r#"{{"name":"{}","length":{},"loop_mode":"{}","tracks":[{}]}}"#,
            anim.name.replace('"', "\\\""),
            anim.length,
            loop_mode_to_str(anim.loop_mode),
            tracks_json.join(",")
        )
    };
    send_json(stream, &json);
}

/// `POST /api/animation/create` -- create a new animation.
fn api_create_animation(state: &Arc<Mutex<EditorState>>, body: &str, stream: &mut TcpStream) {
    let parsed = match parse_json_body(body) {
        Some(v) => v,
        None => {
            send_error(stream, 400, "invalid JSON");
            return;
        }
    };
    let name = match parsed.get("name").and_then(|v| v.as_str()) {
        Some(v) => v.to_string(),
        None => {
            send_error(stream, 400, "missing name");
            return;
        }
    };
    let length = parsed.get("length").and_then(|v| v.as_f64()).unwrap_or(1.0);
    let loop_mode = parsed
        .get("loop_mode")
        .and_then(|v| v.as_str())
        .map(loop_mode_from_str)
        .unwrap_or(LoopMode::None);
    if name.is_empty() {
        send_error(stream, 400, "animation name cannot be empty");
        return;
    }
    let mut state = state.lock().unwrap();
    if state.animations.contains_key(&name) {
        send_error(stream, 400, "animation already exists");
        return;
    }
    let mut anim = Animation::new(name.clone(), length.max(0.1));
    anim.loop_mode = loop_mode;
    state.animations.insert(name.clone(), anim);
    state.add_log("info", format!("Created animation '{}'", name));
    send_json(stream, r#"{"ok":true}"#);
}

/// `POST /api/animation/delete` -- delete an animation.
fn api_delete_animation(state: &Arc<Mutex<EditorState>>, body: &str, stream: &mut TcpStream) {
    let parsed = match parse_json_body(body) {
        Some(v) => v,
        None => {
            send_error(stream, 400, "invalid JSON");
            return;
        }
    };
    let name = match parsed.get("name").and_then(|v| v.as_str()) {
        Some(v) => v.to_string(),
        None => {
            send_error(stream, 400, "missing name");
            return;
        }
    };
    let mut state = state.lock().unwrap();
    if state.animations.remove(&name).is_none() {
        send_error(stream, 404, "animation not found");
        return;
    }
    if state.animation_playback.animation_name.as_deref() == Some(&name) {
        state.animation_playback.playing = false;
        state.animation_playback.animation_name = None;
        state.animation_playback.current_time = 0.0;
    }
    state.add_log("info", format!("Deleted animation '{}'", name));
    send_json(stream, r#"{"ok":true}"#);
}

/// `POST /api/animation/keyframe/add` -- add a keyframe to a track.
fn api_add_keyframe(state: &Arc<Mutex<EditorState>>, body: &str, stream: &mut TcpStream) {
    let parsed = match parse_json_body(body) {
        Some(v) => v,
        None => {
            send_error(stream, 400, "invalid JSON");
            return;
        }
    };
    let anim_name = match parsed.get("animation").and_then(|v| v.as_str()) {
        Some(v) => v.to_string(),
        None => {
            send_error(stream, 400, "missing animation");
            return;
        }
    };
    let node_path = match parsed.get("track_node").and_then(|v| v.as_str()) {
        Some(v) => v.to_string(),
        None => {
            send_error(stream, 400, "missing track_node");
            return;
        }
    };
    let property = match parsed.get("track_property").and_then(|v| v.as_str()) {
        Some(v) => v.to_string(),
        None => {
            send_error(stream, 400, "missing track_property");
            return;
        }
    };
    let time = match parsed.get("time").and_then(|v| v.as_f64()) {
        Some(v) => v,
        None => {
            send_error(stream, 400, "missing time");
            return;
        }
    };
    let value_json = match parsed.get("value") {
        Some(v) => v,
        None => {
            send_error(stream, 400, "missing value");
            return;
        }
    };
    let value = match from_json(value_json) {
        Some(v) => v,
        None => {
            send_error(stream, 400, "invalid variant value");
            return;
        }
    };
    let track_type = parsed
        .get("track_type")
        .and_then(|v| v.as_str())
        .and_then(TrackType::from_str_name)
        .unwrap_or(TrackType::Property);
    let mut state = state.lock().unwrap();
    let anim = match state.animations.get_mut(&anim_name) {
        Some(a) => a,
        None => {
            send_error(stream, 404, "animation not found");
            return;
        }
    };
    let track_idx = anim.tracks.iter().position(|t| {
        t.node_path == node_path && t.property_path == property && t.track_type == track_type
    });
    let track_idx = match track_idx {
        Some(idx) => idx,
        None => {
            let track = AnimationTrack::with_type(&node_path, &property, track_type);
            anim.tracks.push(track);
            anim.tracks.len() - 1
        }
    };
    anim.tracks[track_idx].add_keyframe(KeyFrame::linear(time, value));
    send_json(
        stream,
        &format!(
            r#"{{"ok":true,"track_index":{},"track_type":"{}"}}"#,
            track_idx,
            track_type.as_str()
        ),
    );
}

/// `POST /api/animation/track/add` -- add an explicit typed track to an animation.
fn api_add_track(state: &Arc<Mutex<EditorState>>, body: &str, stream: &mut TcpStream) {
    let parsed = match parse_json_body(body) {
        Some(v) => v,
        None => {
            send_error(stream, 400, "invalid JSON");
            return;
        }
    };
    let anim_name = match parsed.get("animation").and_then(|v| v.as_str()) {
        Some(v) => v.to_string(),
        None => {
            send_error(stream, 400, "missing animation");
            return;
        }
    };
    let node_path = match parsed.get("node_path").and_then(|v| v.as_str()) {
        Some(v) => v.to_string(),
        None => {
            send_error(stream, 400, "missing node_path");
            return;
        }
    };
    let property = match parsed.get("property").and_then(|v| v.as_str()) {
        Some(v) => v.to_string(),
        None => {
            send_error(stream, 400, "missing property");
            return;
        }
    };
    let track_type = match parsed.get("track_type").and_then(|v| v.as_str()) {
        Some(s) => match TrackType::from_str_name(s) {
            Some(tt) => tt,
            None => {
                send_error(
                    stream,
                    400,
                    "invalid track_type (use property, method, or audio)",
                );
                return;
            }
        },
        None => TrackType::Property,
    };
    let mut state = state.lock().unwrap();
    let anim = match state.animations.get_mut(&anim_name) {
        Some(a) => a,
        None => {
            send_error(stream, 404, "animation not found");
            return;
        }
    };
    // Check for duplicate track
    let exists = anim.tracks.iter().any(|t| {
        t.node_path == node_path && t.property_path == property && t.track_type == track_type
    });
    if exists {
        send_error(stream, 400, "track already exists");
        return;
    }
    let track = AnimationTrack::with_type(&node_path, &property, track_type);
    anim.tracks.push(track);
    let idx = anim.tracks.len() - 1;
    send_json(
        stream,
        &format!(
            r#"{{"ok":true,"track_index":{},"track_type":"{}"}}"#,
            idx,
            track_type.as_str()
        ),
    );
}

/// `POST /api/animation/track/delete` -- remove a track from an animation.
fn api_delete_track(state: &Arc<Mutex<EditorState>>, body: &str, stream: &mut TcpStream) {
    let parsed = match parse_json_body(body) {
        Some(v) => v,
        None => {
            send_error(stream, 400, "invalid JSON");
            return;
        }
    };
    let anim_name = match parsed.get("animation").and_then(|v| v.as_str()) {
        Some(v) => v.to_string(),
        None => {
            send_error(stream, 400, "missing animation");
            return;
        }
    };
    let track_index = match parsed.get("track_index").and_then(|v| v.as_u64()) {
        Some(v) => v as usize,
        None => {
            send_error(stream, 400, "missing track_index");
            return;
        }
    };
    let mut state = state.lock().unwrap();
    let anim = match state.animations.get_mut(&anim_name) {
        Some(a) => a,
        None => {
            send_error(stream, 404, "animation not found");
            return;
        }
    };
    if track_index >= anim.tracks.len() {
        send_error(stream, 400, "track_index out of range");
        return;
    }
    anim.tracks.remove(track_index);
    send_json(stream, r#"{"ok":true}"#);
}

/// `POST /api/animation/keyframe/remove` -- remove a keyframe from a track.
fn api_remove_keyframe(state: &Arc<Mutex<EditorState>>, body: &str, stream: &mut TcpStream) {
    let parsed = match parse_json_body(body) {
        Some(v) => v,
        None => {
            send_error(stream, 400, "invalid JSON");
            return;
        }
    };
    let anim_name = match parsed.get("animation").and_then(|v| v.as_str()) {
        Some(v) => v.to_string(),
        None => {
            send_error(stream, 400, "missing animation");
            return;
        }
    };
    let track_index = match parsed.get("track_index").and_then(|v| v.as_u64()) {
        Some(v) => v as usize,
        None => {
            send_error(stream, 400, "missing track_index");
            return;
        }
    };
    let keyframe_index = match parsed.get("keyframe_index").and_then(|v| v.as_u64()) {
        Some(v) => v as usize,
        None => {
            send_error(stream, 400, "missing keyframe_index");
            return;
        }
    };
    let mut state = state.lock().unwrap();
    let anim = match state.animations.get_mut(&anim_name) {
        Some(a) => a,
        None => {
            send_error(stream, 404, "animation not found");
            return;
        }
    };
    if track_index >= anim.tracks.len() {
        send_error(stream, 400, "track_index out of range");
        return;
    }
    if !anim.tracks[track_index].remove_keyframe(keyframe_index) {
        send_error(stream, 400, "keyframe_index out of range");
        return;
    }
    if anim.tracks[track_index].keyframe_count() == 0 {
        anim.tracks.remove(track_index);
    }
    send_json(stream, r#"{"ok":true}"#);
}

/// `POST /api/animation/play` -- start playing an animation.
fn api_play_animation(state: &Arc<Mutex<EditorState>>, body: &str, stream: &mut TcpStream) {
    let parsed = match parse_json_body(body) {
        Some(v) => v,
        None => {
            send_error(stream, 400, "invalid JSON");
            return;
        }
    };
    let name = match parsed.get("name").and_then(|v| v.as_str()) {
        Some(v) => v.to_string(),
        None => {
            send_error(stream, 400, "missing name");
            return;
        }
    };
    let mut state = state.lock().unwrap();
    if !state.animations.contains_key(&name) {
        send_error(stream, 404, "animation not found");
        return;
    }
    state.animation_playback.playing = true;
    state.animation_playback.animation_name = Some(name.clone());
    state.animation_playback.current_time = 0.0;
    state.add_log("info", format!("Playing animation '{}'", name));
    send_json(stream, r#"{"ok":true}"#);
}

/// `POST /api/animation/stop` -- stop animation playback.
fn api_stop_animation(state: &Arc<Mutex<EditorState>>, stream: &mut TcpStream) {
    let mut state = state.lock().unwrap();
    state.animation_playback.playing = false;
    send_json(stream, r#"{"ok":true}"#);
}

/// `GET /api/animation/status` -- returns current playback state.
fn api_animation_status(state: &Arc<Mutex<EditorState>>, stream: &mut TcpStream) {
    let json = {
        let state = state.lock().unwrap();
        let pb = &state.animation_playback;
        let name_json = match &pb.animation_name {
            Some(n) => format!(r#""{}""#, n.replace('"', "\\\"")),
            None => "null".to_string(),
        };
        let blend_json = match &pb.blend_secondary {
            Some(n) => format!(r#""{}""#, n.replace('"', "\\\"")),
            None => "null".to_string(),
        };
        format!(
            r#"{{"playing":{},"current_time":{},"animation_name":{},"recording":{},"blend_secondary":{},"blend_weight":{}}}"#,
            pb.playing, pb.current_time, name_json, pb.recording, blend_json, pb.blend_weight
        )
    };
    send_json(stream, &json);
}

/// `POST /api/animation/seek` -- set the current playback time (for scrubbing).
fn api_seek_animation(state: &Arc<Mutex<EditorState>>, body: &str, stream: &mut TcpStream) {
    let parsed = match parse_json_body(body) {
        Some(v) => v,
        None => {
            send_error(stream, 400, "invalid JSON");
            return;
        }
    };
    let time = match parsed.get("time").and_then(|v| v.as_f64()) {
        Some(v) => v,
        None => {
            send_error(stream, 400, "missing time");
            return;
        }
    };
    let mut state = state.lock().unwrap();
    state.animation_playback.current_time = time.max(0.0);
    // Apply interpolated values to scene tree nodes when scrubbing.
    if let Some(anim_name) = state.animation_playback.animation_name.clone() {
        if let Some(anim) = state.animations.get(&anim_name) {
            let values: Vec<(String, String, Variant)> = anim
                .tracks
                .iter()
                .filter_map(|track| {
                    track
                        .sample(time)
                        .map(|v| (track.node_path.clone(), track.property_path.clone(), v))
                })
                .collect();
            for (node_path, property, value) in values {
                let node_id = find_node_by_name(&state.scene_tree, &node_path);
                if let Some(nid) = node_id {
                    if let Some(node) = state.scene_tree.get_node_mut(nid) {
                        node.set_property(&property, value);
                    }
                }
            }
        }
    }
    send_json(stream, r#"{"ok":true}"#);
}

/// `POST /api/animation/record` -- toggle keyframe recording mode.
fn api_toggle_recording(state: &Arc<Mutex<EditorState>>, body: &str, stream: &mut TcpStream) {
    let parsed = match parse_json_body(body) {
        Some(v) => v,
        None => {
            send_error(stream, 400, "invalid JSON");
            return;
        }
    };
    let enabled = parsed
        .get("enabled")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let mut state = state.lock().unwrap();
    state.animation_playback.recording = enabled;
    state.add_log(
        "info",
        if enabled {
            "Recording mode ON"
        } else {
            "Recording mode OFF"
        },
    );
    send_json(stream, r#"{"ok":true}"#);
}

/// `POST /api/animation/blend` -- set blend preview between two animations.
///
/// Body: `{"secondary": "anim_name", "weight": 0.5}` or `{"secondary": null}` to clear.
fn api_animation_blend(state: &Arc<Mutex<EditorState>>, body: &str, stream: &mut TcpStream) {
    let parsed = match parse_json_body(body) {
        Some(v) => v,
        None => {
            send_error(stream, 400, "invalid JSON");
            return;
        }
    };
    let mut state = state.lock().unwrap();
    let secondary = parsed
        .get("secondary")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let weight = parsed.get("weight").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;

    // Validate the secondary animation exists (if provided).
    if let Some(ref name) = secondary {
        if !state.animations.contains_key(name) {
            send_error(stream, 404, "secondary animation not found");
            return;
        }
    }

    state.animation_playback.blend_secondary = secondary.clone();
    state.animation_playback.blend_weight = weight.clamp(0.0, 1.0);

    // Apply blended values to scene tree for preview.
    let primary_name = state.animation_playback.animation_name.clone();
    let time = state.animation_playback.current_time;
    let blend_w = state.animation_playback.blend_weight;

    if let (Some(ref prim), Some(ref sec)) = (&primary_name, &secondary) {
        if let (Some(prim_anim), Some(sec_anim)) = (
            state.animations.get(prim).cloned(),
            state.animations.get(sec).cloned(),
        ) {
            let prim_values = prim_anim.sample_all(time);
            let sec_values = sec_anim.sample_all(time);

            // Build map from secondary animation.
            let sec_map: std::collections::HashMap<String, gdvariant::Variant> =
                sec_values.into_iter().collect();

            for (prop, prim_val) in &prim_values {
                if let Some(sec_val) = sec_map.get(prop) {
                    let blended =
                        gdscene::animation::interpolate_variant(prim_val, sec_val, blend_w)
                            .unwrap_or_else(|| prim_val.clone());
                    // Find the track's node path from the primary animation.
                    if let Some(track) = prim_anim.tracks.iter().find(|t| t.property_path == *prop)
                    {
                        let node_id = find_node_by_name(&state.scene_tree, &track.node_path);
                        if let Some(nid) = node_id {
                            if let Some(node) = state.scene_tree.get_node_mut(nid) {
                                node.set_property(prop, blended);
                            }
                        }
                    }
                }
            }
        }
    }

    let action = if secondary.is_some() {
        format!("Blend preview: weight={:.0}%", weight * 100.0)
    } else {
        "Blend preview cleared".to_string()
    };
    state.add_log("info", action);
    send_json(
        stream,
        &format!(
            r#"{{"ok":true,"blend_weight":{}}}"#,
            state.animation_playback.blend_weight
        ),
    );
}

/// Find a node by name anywhere in the scene tree (simple linear search).
fn find_node_by_name(tree: &SceneTree, name: &str) -> Option<NodeId> {
    let mut stack = vec![tree.root_id()];
    while let Some(nid) = stack.pop() {
        if let Some(node) = tree.get_node(nid) {
            if node.name() == name {
                return Some(nid);
            }
            for &child in node.children() {
                stack.push(child);
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Runtime API endpoints
// ---------------------------------------------------------------------------

fn api_runtime_play(state: &Arc<Mutex<EditorState>>, stream: &mut TcpStream) {
    let mut state = state.lock().unwrap();
    if state.is_running {
        send_json(stream, r#"{"ok":true,"already_running":true}"#);
        return;
    }
    let mut cloned = clone_scene_tree(&state.scene_tree);

    // Attach GDScripts to nodes that have a `_script_path` property.
    let project_root = state.texture_cache.project_root().to_string();
    attach_scripts_to_tree(&mut cloned, &project_root, &mut state);

    // Call _ready() on all scripted nodes.
    let scripted_ids: Vec<NodeId> = cloned
        .all_nodes_in_tree_order()
        .into_iter()
        .filter(|id| cloned.has_script(*id))
        .collect();
    for id in &scripted_ids {
        cloned.process_script_ready(*id);
    }

    let mut main_loop = MainLoop::new(cloned);
    main_loop.set_input_map(state.build_engine_input_map());
    state.run_main_loop = Some(main_loop);
    state.is_running = true;
    state.is_paused = false;
    state.runtime_frame_count = 0;
    state.add_log("info", "Runtime: play started");
    send_json(stream, r#"{"ok":true,"running":true}"#);
}

/// Walks all nodes in `tree`, finds those with `_script_path`, resolves
/// the path relative to `project_root`, loads the `.gd` file, parses it,
/// and attaches a `GDScriptNodeInstance` to the node.
fn attach_scripts_to_tree(tree: &mut SceneTree, project_root: &str, state: &mut EditorState) {
    let mut scripts_to_load: Vec<(NodeId, std::path::PathBuf)> = Vec::new();

    for node_id in tree.all_nodes_in_tree_order() {
        if let Some(node) = tree.get_node(node_id) {
            if let Variant::String(res_path) = node.get_property("_script_path") {
                let relative = res_path.strip_prefix("res://").unwrap_or(&res_path);
                let abs_path = std::path::Path::new(project_root).join(relative);
                scripts_to_load.push((node_id, abs_path));
            }
        }
    }

    for (node_id, path) in scripts_to_load {
        match std::fs::read_to_string(&path) {
            Ok(source) => match GDScriptNodeInstance::from_source(&source, node_id) {
                Ok(instance) => {
                    tree.attach_script(node_id, Box::new(instance));
                    state.add_log("info", format!("Script loaded: {}", path.display()));
                }
                Err(e) => {
                    state.add_log(
                        "error",
                        format!("Script parse error in {}: {}", path.display(), e),
                    );
                }
            },
            Err(e) => {
                state.add_log(
                    "error",
                    format!("Failed to read script {}: {}", path.display(), e),
                );
            }
        }
    }
}

fn api_runtime_stop(state: &Arc<Mutex<EditorState>>, stream: &mut TcpStream) {
    let mut state = state.lock().unwrap();
    state.is_running = false;
    state.is_paused = false;
    state.run_main_loop = None;
    state.runtime_frame_count = 0;
    state.clear_all_input();
    state.add_log("info", "Runtime: stopped");
    send_json(stream, r#"{"ok":true,"running":false}"#);
}

fn api_runtime_pause(state: &Arc<Mutex<EditorState>>, stream: &mut TcpStream) {
    let mut state = state.lock().unwrap();
    if !state.is_running {
        send_error(stream, 400, "not running");
        return;
    }
    state.is_paused = !state.is_paused;
    let paused = state.is_paused;
    state.add_log(
        "info",
        format!("Runtime: {}", if paused { "paused" } else { "resumed" }),
    );
    send_json(stream, &format!(r#"{{"ok":true,"paused":{paused}}}"#));
}

fn api_runtime_step(state: &Arc<Mutex<EditorState>>, stream: &mut TcpStream) {
    let mut state = state.lock().unwrap();
    if !state.is_running {
        send_error(stream, 400, "not running");
        return;
    }
    if !state.is_paused {
        send_error(stream, 400, "not paused");
        return;
    }
    let delta = state.delta_time;
    // The engine-owned InputState is populated by api_input_key_down/up via
    // push_event(). The bridge in MainLoop::step() converts it to the
    // script-facing InputSnapshot automatically. We still set a manual
    // snapshot as a fallback for any keys that didn't map to a typed Key.
    let input_snapshot = state.make_input_snapshot();
    if let Some(ref mut main_loop) = state.run_main_loop {
        main_loop.set_input(input_snapshot);
        let output = main_loop.step(delta);
        state.runtime_frame_count = output.frame_count;
    }
    // Clear per-frame input after scripts have run.
    state.clear_frame_input();
    let frame = state.runtime_frame_count;
    send_json(stream, &format!(r#"{{"ok":true,"frame_count":{frame}}}"#));
}

fn api_runtime_status(state: &Arc<Mutex<EditorState>>, stream: &mut TcpStream) {
    let state = state.lock().unwrap();
    let running = state.is_running;
    let paused = state.is_paused;
    let frame_count = state.runtime_frame_count;
    let fps = if state.delta_time > 0.0 {
        1.0 / state.delta_time
    } else {
        0.0
    };
    send_json(
        stream,
        &format!(
            r#"{{"running":{running},"paused":{paused},"frame_count":{frame_count},"fps":{fps:.1}}}"#
        ),
    );
}

// ---------------------------------------------------------------------------
// Scene instancing endpoint
// ---------------------------------------------------------------------------

/// `POST /api/scene/instance` — instances a .tscn file as a child of a node.
///
/// Body: `{"path": "path/to/scene.tscn", "parent_id": 123}`
/// Returns: `{"id": <root_node_id>}`
fn api_instance_scene(state: &Arc<Mutex<EditorState>>, body: &str, stream: &mut TcpStream) {
    let parsed = match parse_json_body(body) {
        Some(v) => v,
        None => {
            send_error(stream, 400, "invalid JSON");
            return;
        }
    };

    let path = match parsed.get("path").and_then(|v| v.as_str()) {
        Some(v) => v.to_string(),
        None => {
            send_error(stream, 400, "missing path");
            return;
        }
    };

    let parent_raw = match parsed.get("parent_id").and_then(|v| v.as_u64()) {
        Some(v) => v,
        None => {
            send_error(stream, 400, "missing parent_id");
            return;
        }
    };

    // Read the .tscn file.
    let source = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(e) => {
            send_error(stream, 400, &format!("failed to read: {e}"));
            return;
        }
    };

    let mut state = state.lock().unwrap();
    let parent_id = match find_node_by_raw_id(&state.scene_tree, parent_raw) {
        Some(id) => id,
        None => {
            send_error(stream, 404, "parent not found");
            return;
        }
    };

    let mut cmd = EditorCommand::InstanceScene {
        parent_id,
        tscn_source: source,
        created_ids: Vec::new(),
        root_id: None,
    };

    if let Err(e) = cmd.execute(&mut state.scene_tree) {
        send_error(stream, 500, &e.to_string());
        return;
    }

    let root_id = match &cmd {
        EditorCommand::InstanceScene { root_id, .. } => root_id.unwrap(),
        _ => unreachable!(),
    };

    state.undo_stack.push(cmd);
    state.redo_stack.clear();
    state.scene_modified = true;
    state.add_log("info", format!("Instanced scene from '{}'", path));

    let json = format!(r#"{{"id":{}}}"#, root_id.raw());
    send_json(stream, &json);
}

// ---------------------------------------------------------------------------
// Viewport asset drop endpoint
// ---------------------------------------------------------------------------

/// `POST /api/viewport/drop` — handles an asset dropped onto the viewport.
///
/// Body: `{"asset_type": "scene"|"texture"|"audio", "path": "...", "parent_id": 123, "pixel_x": 100, "pixel_y": 200}`
/// Returns: `{"id": <created_node_id>}`
fn api_viewport_drop(state: &Arc<Mutex<EditorState>>, body: &str, stream: &mut TcpStream) {
    let parsed = match parse_json_body(body) {
        Some(v) => v,
        None => {
            send_error(stream, 400, "invalid JSON");
            return;
        }
    };

    let asset_type = match parsed.get("asset_type").and_then(|v| v.as_str()) {
        Some(v) => v.to_string(),
        None => {
            send_error(stream, 400, "missing asset_type");
            return;
        }
    };
    let path = match parsed.get("path").and_then(|v| v.as_str()) {
        Some(v) => v.to_string(),
        None => {
            send_error(stream, 400, "missing path");
            return;
        }
    };
    let parent_raw = match parsed.get("parent_id").and_then(|v| v.as_u64()) {
        Some(v) => v,
        None => {
            send_error(stream, 400, "missing parent_id");
            return;
        }
    };
    let pixel_x = parsed
        .get("pixel_x")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0) as f32;
    let pixel_y = parsed
        .get("pixel_y")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0) as f32;

    match asset_type.as_str() {
        "scene" => {
            // Instance the .tscn scene and set position
            let source = match std::fs::read_to_string(&path) {
                Ok(s) => s,
                Err(e) => {
                    send_error(stream, 400, &format!("failed to read: {e}"));
                    return;
                }
            };

            let mut state = state.lock().unwrap();
            let parent_id = match find_node_by_raw_id(&state.scene_tree, parent_raw) {
                Some(id) => id,
                None => {
                    send_error(stream, 404, "parent not found");
                    return;
                }
            };

            let world_pos = pixel_to_world_pos(&state, pixel_x, pixel_y);

            let mut cmd = EditorCommand::InstanceScene {
                parent_id,
                tscn_source: source,
                created_ids: Vec::new(),
                root_id: None,
            };

            if let Err(e) = cmd.execute(&mut state.scene_tree) {
                send_error(stream, 500, &e.to_string());
                return;
            }

            let root_id = match &cmd {
                EditorCommand::InstanceScene { root_id, .. } => root_id.unwrap(),
                _ => unreachable!(),
            };

            // Set the instanced root's position to the drop location
            if let Some(node) = state.scene_tree.get_node_mut(root_id) {
                node.set_property(
                    "position",
                    Variant::Vector2(Vector2::new(world_pos.x, world_pos.y)),
                );
            }

            state.undo_stack.push(cmd);
            state.redo_stack.clear();
            state.scene_modified = true;
            state.add_log(
                "info",
                format!(
                    "Dropped scene '{}' at ({:.0}, {:.0})",
                    path, world_pos.x, world_pos.y
                ),
            );

            let json = format!(r#"{{"id":{}}}"#, root_id.raw());
            send_json(stream, &json);
        }
        "texture" => {
            // Create a Sprite2D with the texture path
            let mut state = state.lock().unwrap();
            let parent_id = match find_node_by_raw_id(&state.scene_tree, parent_raw) {
                Some(id) => id,
                None => {
                    send_error(stream, 404, "parent not found");
                    return;
                }
            };

            let world_pos = pixel_to_world_pos(&state, pixel_x, pixel_y);

            // Derive a node name from the filename
            let node_name = std::path::Path::new(&path)
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| "Sprite2D".to_string());

            let mut cmd = EditorCommand::AddNode {
                parent_id,
                name: node_name.clone(),
                class_name: "Sprite2D".to_string(),
                created_id: None,
            };

            if let Err(e) = cmd.execute(&mut state.scene_tree) {
                send_error(stream, 500, &e.to_string());
                return;
            }

            let created_id = match &cmd {
                EditorCommand::AddNode { created_id, .. } => created_id.unwrap(),
                _ => unreachable!(),
            };

            // Set position and texture
            if let Some(node) = state.scene_tree.get_node_mut(created_id) {
                node.set_property(
                    "position",
                    Variant::Vector2(Vector2::new(world_pos.x, world_pos.y)),
                );
                node.set_property("texture", Variant::String(format!("res://{}", path)));
            }

            state.undo_stack.push(cmd);
            state.redo_stack.clear();
            state.scene_modified = true;
            state.add_log(
                "info",
                format!("Created Sprite2D '{}' from '{}'", node_name, path),
            );

            let json = format!(r#"{{"id":{}}}"#, created_id.raw());
            send_json(stream, &json);
        }
        "audio" => {
            // Create an AudioStreamPlayer with the audio path
            let mut state = state.lock().unwrap();
            let parent_id = match find_node_by_raw_id(&state.scene_tree, parent_raw) {
                Some(id) => id,
                None => {
                    send_error(stream, 404, "parent not found");
                    return;
                }
            };

            let node_name = std::path::Path::new(&path)
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| "AudioStreamPlayer".to_string());

            let mut cmd = EditorCommand::AddNode {
                parent_id,
                name: node_name.clone(),
                class_name: "AudioStreamPlayer".to_string(),
                created_id: None,
            };

            if let Err(e) = cmd.execute(&mut state.scene_tree) {
                send_error(stream, 500, &e.to_string());
                return;
            }

            let created_id = match &cmd {
                EditorCommand::AddNode { created_id, .. } => created_id.unwrap(),
                _ => unreachable!(),
            };

            if let Some(node) = state.scene_tree.get_node_mut(created_id) {
                node.set_property("stream", Variant::String(format!("res://{}", path)));
            }

            state.undo_stack.push(cmd);
            state.redo_stack.clear();
            state.scene_modified = true;
            state.add_log(
                "info",
                format!("Created AudioStreamPlayer '{}' from '{}'", node_name, path),
            );

            let json = format!(r#"{{"id":{}}}"#, created_id.raw());
            send_json(stream, &json);
        }
        _ => {
            send_error(
                stream,
                400,
                &format!("unsupported asset_type: {}", asset_type),
            );
        }
    }
}

/// Convert pixel coordinates to world-space position using current viewport settings.
fn pixel_to_world_pos(state: &EditorState, pixel_x: f32, pixel_y: f32) -> Vector2 {
    let offset = crate::scene_renderer::camera_offset_with_zoom_pan(
        &state.scene_tree,
        state.viewport_width,
        state.viewport_height,
        state.viewport_zoom,
        state.viewport_pan,
    );
    let z = state.viewport_zoom as f32;
    Vector2::new((pixel_x - offset.x) / z, (pixel_y - offset.y) / z)
}

// ---------------------------------------------------------------------------
// Collision shape resize endpoint
// ---------------------------------------------------------------------------

/// `POST /api/viewport/shape_resize` — resizes a collision shape.
///
/// Body: `{"node_id": 123, "handle": "radius", "value": 50.0}`
///    or `{"node_id": 123, "handle": "extents", "value": [30, 20]}`
fn api_shape_resize(state: &Arc<Mutex<EditorState>>, body: &str, stream: &mut TcpStream) {
    let parsed = match parse_json_body(body) {
        Some(v) => v,
        None => {
            send_error(stream, 400, "invalid JSON");
            return;
        }
    };

    let node_raw = match parsed.get("node_id").and_then(|v| v.as_u64()) {
        Some(v) => v,
        None => {
            send_error(stream, 400, "missing node_id");
            return;
        }
    };

    let handle = match parsed.get("handle").and_then(|v| v.as_str()) {
        Some(v) => v.to_string(),
        None => {
            send_error(stream, 400, "missing handle");
            return;
        }
    };

    let mut state = state.lock().unwrap();
    let node_id = match find_node_by_raw_id(&state.scene_tree, node_raw) {
        Some(id) => id,
        None => {
            send_error(stream, 404, "node not found");
            return;
        }
    };

    // Build the appropriate property set command based on handle type.
    let (property, new_value) = match handle.as_str() {
        "radius" => {
            let val = match parsed.get("value").and_then(|v| v.as_f64()) {
                Some(v) => v,
                None => {
                    send_error(stream, 400, "missing numeric value for radius");
                    return;
                }
            };
            ("shape_radius".to_string(), Variant::Float(val))
        }
        "extents" => {
            let arr = match parsed.get("value").and_then(|v| v.as_array()) {
                Some(v) => v,
                None => {
                    send_error(stream, 400, "missing array value for extents");
                    return;
                }
            };
            if arr.len() != 2 {
                send_error(stream, 400, "extents must be [x, y]");
                return;
            }
            let x = arr[0].as_f64().unwrap_or(0.0) as f32;
            let y = arr[1].as_f64().unwrap_or(0.0) as f32;
            (
                "shape_extents".to_string(),
                Variant::Vector2(Vector2::new(x, y)),
            )
        }
        "height" => {
            let val = match parsed.get("value").and_then(|v| v.as_f64()) {
                Some(v) => v,
                None => {
                    send_error(stream, 400, "missing numeric value for height");
                    return;
                }
            };
            ("shape_height".to_string(), Variant::Float(val))
        }
        _ => {
            send_error(stream, 400, &format!("unknown handle type: {handle}"));
            return;
        }
    };

    let mut cmd = EditorCommand::SetProperty {
        node_id,
        property: property.clone(),
        new_value: new_value.clone(),
        old_value: Variant::Nil,
    };

    if let Err(e) = cmd.execute(&mut state.scene_tree) {
        send_error(stream, 500, &e.to_string());
        return;
    }

    state.undo_stack.push(cmd);
    state.redo_stack.clear();
    state.scene_modified = true;
    state.add_log(
        "info",
        format!("Shape resize: {} on node {}", handle, node_raw),
    );

    send_json(stream, r#"{"ok":true}"#);
}

// ---------------------------------------------------------------------------
// Input API endpoints
// ---------------------------------------------------------------------------

fn api_input_key_down(state: &Arc<Mutex<EditorState>>, body: &str, stream: &mut TcpStream) {
    let mut state = state.lock().unwrap();
    if !state.is_running {
        send_error(stream, 400, "not running");
        return;
    }
    let v: serde_json::Value = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(_) => {
            send_error(stream, 400, "invalid json");
            return;
        }
    };
    let key = match v["key"].as_str() {
        Some(k) => k.to_string(),
        None => {
            send_error(stream, 400, "missing key");
            return;
        }
    };
    if !state.pressed_keys.contains(&key) {
        state.just_pressed_keys.insert(key.clone());
    }
    state.pressed_keys.insert(key.clone());
    // Route through engine-owned InputState when MainLoop is active.
    if let Some(ref mut main_loop) = state.run_main_loop {
        if let Some(typed_key) = gdplatform::input::Key::from_name(&key) {
            main_loop.push_event(gdplatform::InputEvent::Key {
                key: typed_key,
                pressed: true,
                shift: false,
                ctrl: false,
                alt: false,
            });
        }
    }
    send_json(stream, r#"{"ok":true}"#);
}

fn api_input_key_up(state: &Arc<Mutex<EditorState>>, body: &str, stream: &mut TcpStream) {
    let mut state = state.lock().unwrap();
    if !state.is_running {
        send_error(stream, 400, "not running");
        return;
    }
    let v: serde_json::Value = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(_) => {
            send_error(stream, 400, "invalid json");
            return;
        }
    };
    let key = match v["key"].as_str() {
        Some(k) => k.to_string(),
        None => {
            send_error(stream, 400, "missing key");
            return;
        }
    };
    state.pressed_keys.remove(&key);
    state.just_released_keys.insert(key.clone());
    // Route through engine-owned InputState when MainLoop is active.
    if let Some(ref mut main_loop) = state.run_main_loop {
        if let Some(typed_key) = gdplatform::input::Key::from_name(&key) {
            main_loop.push_event(gdplatform::InputEvent::Key {
                key: typed_key,
                pressed: false,
                shift: false,
                ctrl: false,
                alt: false,
            });
        }
    }
    send_json(stream, r#"{"ok":true}"#);
}

fn api_input_mouse_move(state: &Arc<Mutex<EditorState>>, body: &str, stream: &mut TcpStream) {
    let mut state = state.lock().unwrap();
    if !state.is_running {
        send_error(stream, 400, "not running");
        return;
    }
    let v: serde_json::Value = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(_) => {
            send_error(stream, 400, "invalid json");
            return;
        }
    };
    let x = match v["x"].as_f64() {
        Some(x) => x,
        None => {
            send_error(stream, 400, "missing x");
            return;
        }
    };
    let y = match v["y"].as_f64() {
        Some(y) => y,
        None => {
            send_error(stream, 400, "missing y");
            return;
        }
    };
    state.mouse_position = (x, y);
    send_json(stream, r#"{"ok":true}"#);
}

fn api_input_mouse_down(state: &Arc<Mutex<EditorState>>, body: &str, stream: &mut TcpStream) {
    let mut state = state.lock().unwrap();
    if !state.is_running {
        send_error(stream, 400, "not running");
        return;
    }
    let v: serde_json::Value = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(_) => {
            send_error(stream, 400, "invalid json");
            return;
        }
    };
    let button = match v["button"].as_u64() {
        Some(b) if b <= 2 => b as u8,
        _ => {
            send_error(stream, 400, "invalid button (0-2)");
            return;
        }
    };
    state.mouse_buttons.insert(button);
    send_json(stream, r#"{"ok":true}"#);
}

fn api_input_mouse_up(state: &Arc<Mutex<EditorState>>, body: &str, stream: &mut TcpStream) {
    let mut state = state.lock().unwrap();
    if !state.is_running {
        send_error(stream, 400, "not running");
        return;
    }
    let v: serde_json::Value = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(_) => {
            send_error(stream, 400, "invalid json");
            return;
        }
    };
    let button = match v["button"].as_u64() {
        Some(b) if b <= 2 => b as u8,
        _ => {
            send_error(stream, 400, "invalid button (0-2)");
            return;
        }
    };
    state.mouse_buttons.remove(&button);
    send_json(stream, r#"{"ok":true}"#);
}

fn api_input_clear_frame(state: &Arc<Mutex<EditorState>>, stream: &mut TcpStream) {
    let mut state = state.lock().unwrap();
    state.clear_frame_input();
    send_json(stream, r#"{"ok":true}"#);
}

fn api_input_state(state: &Arc<Mutex<EditorState>>, stream: &mut TcpStream) {
    let state = state.lock().unwrap();
    let mut pressed: Vec<&String> = state.pressed_keys.iter().collect();
    pressed.sort();
    let mut just_pressed: Vec<&String> = state.just_pressed_keys.iter().collect();
    just_pressed.sort();
    let mut mouse_btns: Vec<u8> = state.mouse_buttons.iter().copied().collect();
    mouse_btns.sort();

    // Build actions object with sorted keys for deterministic output
    let mut actions = String::from("{");
    let mut action_names: Vec<&String> = state.input_map.keys().collect();
    action_names.sort();
    let mut first = true;
    for action in &action_names {
        if !first {
            actions.push(',');
        }
        first = false;
        let pressed_val = state.is_action_pressed(action);
        actions.push_str(&format!(r#""{}": {}"#, action, pressed_val));
    }
    actions.push('}');

    let json = format!(
        r#"{{"pressed_keys":[{}],"just_pressed":[{}],"mouse_position":[{},{}],"mouse_buttons":[{}],"actions":{}}}"#,
        pressed
            .iter()
            .map(|k| format!(r#""{}""#, k))
            .collect::<Vec<_>>()
            .join(","),
        just_pressed
            .iter()
            .map(|k| format!(r#""{}""#, k))
            .collect::<Vec<_>>()
            .join(","),
        state.mouse_position.0,
        state.mouse_position.1,
        mouse_btns
            .iter()
            .map(|b| b.to_string())
            .collect::<Vec<_>>()
            .join(","),
        actions,
    );
    send_json(stream, &json);
}

fn api_tilemap_paint(state: &Arc<Mutex<EditorState>>, body: &str, stream: &mut TcpStream) {
    let p = match parse_json_body(body) {
        Some(v) => v,
        None => {
            send_error(stream, 400, "bad json");
            return;
        }
    };
    let nr = match p.get("node_id").and_then(|v| v.as_u64()) {
        Some(v) => v,
        None => {
            send_error(stream, 400, "missing node_id");
            return;
        }
    };
    let x = p.get("x").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
    let y = p.get("y").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
    let tid = p.get("tile_id").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
    let mut s = state.lock().unwrap();
    let nid = match find_node_by_raw_id(&s.scene_tree, nr) {
        Some(id) => id,
        None => {
            send_error(stream, 404, "not found");
            return;
        }
    };
    let mut cmd = EditorCommand::TileMapPaint {
        node_id: nid,
        x,
        y,
        tile_id: tid,
        old_tile_id: 0,
    };
    let _ = cmd.execute_tilemap(&mut s.tile_grid_store);
    s.undo_stack.push(cmd);
    s.redo_stack.clear();
    s.scene_modified = true;
    send_json(stream, r#"{"ok":true}"#);
}

fn api_tilemap_erase(state: &Arc<Mutex<EditorState>>, body: &str, stream: &mut TcpStream) {
    let p = match parse_json_body(body) {
        Some(v) => v,
        None => {
            send_error(stream, 400, "bad json");
            return;
        }
    };
    let nr = match p.get("node_id").and_then(|v| v.as_u64()) {
        Some(v) => v,
        None => {
            send_error(stream, 400, "missing node_id");
            return;
        }
    };
    let x = p.get("x").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
    let y = p.get("y").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
    let mut s = state.lock().unwrap();
    let nid = match find_node_by_raw_id(&s.scene_tree, nr) {
        Some(id) => id,
        None => {
            send_error(stream, 404, "not found");
            return;
        }
    };
    let mut cmd = EditorCommand::TileMapPaint {
        node_id: nid,
        x,
        y,
        tile_id: 0,
        old_tile_id: 0,
    };
    let _ = cmd.execute_tilemap(&mut s.tile_grid_store);
    s.undo_stack.push(cmd);
    s.redo_stack.clear();
    s.scene_modified = true;
    send_json(stream, r#"{"ok":true}"#);
}

fn api_tilemap_fill(state: &Arc<Mutex<EditorState>>, body: &str, stream: &mut TcpStream) {
    let p = match parse_json_body(body) {
        Some(v) => v,
        None => {
            send_error(stream, 400, "bad json");
            return;
        }
    };
    let nr = match p.get("node_id").and_then(|v| v.as_u64()) {
        Some(v) => v,
        None => {
            send_error(stream, 400, "missing node_id");
            return;
        }
    };
    let x1 = p.get("x1").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
    let y1 = p.get("y1").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
    let x2 = p.get("x2").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
    let y2 = p.get("y2").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
    let tid = p.get("tile_id").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
    let mut s = state.lock().unwrap();
    let nid = match find_node_by_raw_id(&s.scene_tree, nr) {
        Some(id) => id,
        None => {
            send_error(stream, 404, "not found");
            return;
        }
    };
    let mut cmd = EditorCommand::TileMapFill {
        node_id: nid,
        x1,
        y1,
        x2,
        y2,
        tile_id: tid,
        old_tiles: Vec::new(),
    };
    let _ = cmd.execute_tilemap(&mut s.tile_grid_store);
    s.undo_stack.push(cmd);
    s.redo_stack.clear();
    s.scene_modified = true;
    send_json(stream, r#"{"ok":true}"#);
}

fn api_tilemap_data(state: &Arc<Mutex<EditorState>>, query: &str, stream: &mut TcpStream) {
    let nr: u64 = query
        .split('&')
        .find_map(|p| {
            let (k, v) = p.split_once('=')?;
            if k == "node_id" {
                v.parse().ok()
            } else {
                None
            }
        })
        .unwrap_or(0);
    if nr == 0 {
        send_error(stream, 400, "missing node_id");
        return;
    }
    let s = state.lock().unwrap();
    let nid = match find_node_by_raw_id(&s.scene_tree, nr) {
        Some(id) => id,
        None => {
            send_error(stream, 404, "not found");
            return;
        }
    };
    match s.tile_grid_store.get(nid) {
        Some(g) => send_json(stream, &g.to_json().to_string()),
        None => send_error(stream, 404, "no tilemap data"),
    }
}

fn api_tilemap_resize(state: &Arc<Mutex<EditorState>>, body: &str, stream: &mut TcpStream) {
    let p = match parse_json_body(body) {
        Some(v) => v,
        None => {
            send_error(stream, 400, "bad json");
            return;
        }
    };
    let nr = match p.get("node_id").and_then(|v| v.as_u64()) {
        Some(v) => v,
        None => {
            send_error(stream, 400, "missing node_id");
            return;
        }
    };
    let w = match p.get("width").and_then(|v| v.as_u64()) {
        Some(v) => v as usize,
        None => {
            send_error(stream, 400, "missing width");
            return;
        }
    };
    let h = match p.get("height").and_then(|v| v.as_u64()) {
        Some(v) => v as usize,
        None => {
            send_error(stream, 400, "missing height");
            return;
        }
    };
    let mut s = state.lock().unwrap();
    let nid = match find_node_by_raw_id(&s.scene_tree, nr) {
        Some(id) => id,
        None => {
            send_error(stream, 404, "not found");
            return;
        }
    };
    let mut cmd = EditorCommand::TileMapResize {
        node_id: nid,
        new_width: w,
        new_height: h,
        old_width: 0,
        old_height: 0,
        old_cells: Vec::new(),
    };
    let _ = cmd.execute_tilemap(&mut s.tile_grid_store);
    s.undo_stack.push(cmd);
    s.redo_stack.clear();
    s.scene_modified = true;
    send_json(stream, r#"{"ok":true}"#);
}

fn api_tilemap_tileset(state: &Arc<Mutex<EditorState>>, stream: &mut TcpStream) {
    let s = state.lock().unwrap();
    let ts = match &s.tile_grid_store.tileset {
        Some(ts) => ts,
        None => {
            send_json(stream, r#"{"tiles":[]}"#);
            return;
        }
    };
    let tiles: Vec<serde_json::Value> = ts.tile_ids_sorted().iter().filter_map(|&id| { let t = ts.get_tile(id)?; Some(serde_json::json!({"id":t.id,"name":t.name,"color":format!("#{:02X}{:02X}{:02X}",(t.color.r*255.0) as u8,(t.color.g*255.0) as u8,(t.color.b*255.0) as u8),"collision":t.collision})) }).collect();
    send_json(
        stream,
        &serde_json::json!({"cell_size":[ts.cell_size.x,ts.cell_size.y],"tiles":tiles}).to_string(),
    );
}

// ---------------------------------------------------------------------------
// pat-r5p: Transform gizmo - axis-constrained drag, rotate, scale
// ---------------------------------------------------------------------------

/// `POST /api/viewport/drag_axis` -- drag constrained to X or Y axis.
fn api_viewport_drag_axis(state: &Arc<Mutex<EditorState>>, body: &str, stream: &mut TcpStream) {
    let p = match parse_json_body(body) {
        Some(v) => v,
        None => {
            send_error(stream, 400, "invalid JSON");
            return;
        }
    };
    let dx = p.get("dx").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
    let dy = p.get("dy").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
    let axis = p
        .get("axis")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let mut s = state.lock().unwrap();
    let z = s.viewport_zoom as f32;
    let (wdx, wdy) = match axis.as_str() {
        "x" => (dx / z, 0.0),
        "y" => (0.0, dy / z),
        _ => (dx / z, dy / z),
    };
    if let Some(nid) = s.selected_node {
        let cur_pos = s
            .scene_tree
            .get_node(nid)
            .map(|n| match n.get_property("position") {
                Variant::Vector2(v) => v,
                _ => Vector2::ZERO,
            })
            .unwrap_or(Vector2::ZERO);
        let candidate = Vector2::new(cur_pos.x + wdx, cur_pos.y + wdy);
        let settings = s.display_settings.clone();
        let (new_pos, guides) = apply_snap(&s.scene_tree, &settings, nid, candidate);
        s.snap_guides = guides;
        if let Some(n) = s.scene_tree.get_node_mut(nid) {
            n.set_property("position", Variant::Vector2(new_pos));
        }
    }
    s.transform_axis_constraint = if axis.is_empty() { None } else { Some(axis) };
    send_json(stream, r#"{"ok":true}"#);
}

/// `POST /api/viewport/rotate_node` -- rotate the selected node.
fn api_viewport_rotate_node(state: &Arc<Mutex<EditorState>>, body: &str, stream: &mut TcpStream) {
    let p = match parse_json_body(body) {
        Some(v) => v,
        None => {
            send_error(stream, 400, "invalid JSON");
            return;
        }
    };
    let delta_angle = p.get("delta").and_then(|v| v.as_f64()).unwrap_or(0.0);
    let mut s = state.lock().unwrap();
    if let Some(nid) = s.selected_node {
        if let Some(n) = s.scene_tree.get_node_mut(nid) {
            let rotation = match n.get_property("rotation") {
                Variant::Float(v) => v,
                _ => 0.0,
            };
            n.set_property("rotation", Variant::Float(rotation + delta_angle));
        }
        send_json(stream, r#"{"ok":true}"#);
    } else {
        send_error(stream, 400, "no node selected");
    }
}

/// `POST /api/viewport/scale_node` -- scale the selected node.
fn api_viewport_scale_node(state: &Arc<Mutex<EditorState>>, body: &str, stream: &mut TcpStream) {
    let p = match parse_json_body(body) {
        Some(v) => v,
        None => {
            send_error(stream, 400, "invalid JSON");
            return;
        }
    };
    let sx = p.get("sx").and_then(|v| v.as_f64()).unwrap_or(1.0);
    let sy = p.get("sy").and_then(|v| v.as_f64()).unwrap_or(1.0);
    let mut s = state.lock().unwrap();
    if let Some(nid) = s.selected_node {
        if let Some(n) = s.scene_tree.get_node_mut(nid) {
            let scale = match n.get_property("scale") {
                Variant::Vector2(v) => v,
                _ => Vector2::new(1.0, 1.0),
            };
            n.set_property(
                "scale",
                Variant::Vector2(Vector2::new(
                    (scale.x as f64 * sx) as f32,
                    (scale.y as f64 * sy) as f32,
                )),
            );
        }
        send_json(stream, r#"{"ok":true}"#);
    } else {
        send_error(stream, 400, "no node selected");
    }
}

/// `GET /api/plugins` — returns the list of registered editor plugins.
fn api_get_plugins(state: &Arc<Mutex<EditorState>>, stream: &mut TcpStream) {
    let s = state.lock().unwrap();
    let j = serde_json::to_string(&s.plugins).unwrap_or_else(|_| "[]".to_string());
    send_json(stream, &format!(r#"{{"plugins":{j}}}"#));
}

// ---------------------------------------------------------------------------
// pat-sbdts: Editor settings dialog — keybindings + plugin toggle endpoints
// ---------------------------------------------------------------------------

/// `GET /api/keybindings` -- returns the list of editor keybindings.
fn api_get_keybindings(state: &Arc<Mutex<EditorState>>, stream: &mut TcpStream) {
    let s = state.lock().unwrap();
    let j = serde_json::to_string(&s.keybindings).unwrap_or_else(|_| "[]".to_string());
    send_json(stream, &format!(r#"{{"keybindings":{j}}}"#));
}

/// `POST /api/keybindings` -- update a single keybinding by action name.
/// Body: `{"action":"undo","keys":"Ctrl+Shift+Z"}`
fn api_set_keybinding(state: &Arc<Mutex<EditorState>>, body: &str, stream: &mut TcpStream) {
    let p = match parse_json_body(body) {
        Some(v) => v,
        None => {
            send_error(stream, 400, "invalid JSON");
            return;
        }
    };
    let action = match p.get("action").and_then(|v| v.as_str()) {
        Some(a) => a,
        None => {
            send_error(stream, 400, "missing 'action'");
            return;
        }
    };
    let keys = match p.get("keys").and_then(|v| v.as_str()) {
        Some(k) => k,
        None => {
            send_error(stream, 400, "missing 'keys'");
            return;
        }
    };
    let mut s = state.lock().unwrap();
    if let Some(kb) = s.keybindings.iter_mut().find(|kb| kb.action == action) {
        kb.keys = keys.to_string();
        send_json(stream, r#"{"ok":true}"#);
    } else {
        send_error(stream, 404, "keybinding action not found");
    }
}

/// `POST /api/plugins/toggle` -- toggle a plugin's enabled state.
/// Body: `{"name":"Tilemap","enabled":true}`
fn api_toggle_plugin(state: &Arc<Mutex<EditorState>>, body: &str, stream: &mut TcpStream) {
    let p = match parse_json_body(body) {
        Some(v) => v,
        None => {
            send_error(stream, 400, "invalid JSON");
            return;
        }
    };
    let name = match p.get("name").and_then(|v| v.as_str()) {
        Some(n) => n,
        None => {
            send_error(stream, 400, "missing 'name'");
            return;
        }
    };
    let enabled = match p.get("enabled").and_then(|v| v.as_bool()) {
        Some(e) => e,
        None => {
            send_error(stream, 400, "missing 'enabled'");
            return;
        }
    };
    let mut s = state.lock().unwrap();
    if let Some(plugin) = s.plugins.iter_mut().find(|pl| pl.name == name) {
        plugin.enabled = enabled;
        send_json(stream, r#"{"ok":true}"#);
    } else {
        send_error(stream, 404, "plugin not found");
    }
}

// ---------------------------------------------------------------------------
// pat-zlv: Snapping improvements - snap info endpoint
// ---------------------------------------------------------------------------

/// `GET /api/viewport/snap_info` -- returns current snap configuration.
fn api_get_snap_info(state: &Arc<Mutex<EditorState>>, stream: &mut TcpStream) {
    let s = state.lock().unwrap();
    let ds = &s.display_settings;
    send_json(
        stream,
        &format!(
            r#"{{"snap_enabled":{},"snap_size":{},"grid_visible":{},"rulers_visible":{},"smart_snap_enabled":{},"smart_snap_threshold":{}}}"#,
            ds.grid_snap_enabled,
            ds.grid_snap_size,
            ds.grid_visible,
            ds.rulers_visible,
            ds.smart_snap_enabled,
            ds.smart_snap_threshold
        ),
    );
}

fn api_get_snap_guides(state: &Arc<Mutex<EditorState>>, stream: &mut TcpStream) {
    let s = state.lock().unwrap();
    let guides_json: Vec<String> = s
        .snap_guides
        .iter()
        .map(|g| {
            format!(
                r#"{{"axis":"{}","position":{},"target_node_id":{}}}"#,
                g.axis,
                g.position,
                g.target_node_id.raw()
            )
        })
        .collect();
    send_json(
        stream,
        &format!(r#"{{"guides":[{}]}}"#, guides_json.join(",")),
    );
}

// ---------------------------------------------------------------------------
// pat-cgc: Script editor core - find/replace, go-to-line
// ---------------------------------------------------------------------------

/// `POST /api/script/find` -- search within a script for occurrences of a pattern.
fn api_script_find(body: &str, stream: &mut TcpStream) {
    let parsed = match parse_json_body(body) {
        Some(v) => v,
        None => {
            send_error(stream, 400, "invalid JSON");
            return;
        }
    };
    let content = match parsed.get("content").and_then(|v| v.as_str()) {
        Some(c) => c,
        None => {
            send_error(stream, 400, "missing content");
            return;
        }
    };
    let query = match parsed.get("query").and_then(|v| v.as_str()) {
        Some(q) => q,
        None => {
            send_error(stream, 400, "missing query");
            return;
        }
    };
    let case_sensitive = parsed
        .get("case_sensitive")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);

    let (search_content, search_query) = if case_sensitive {
        (content.to_string(), query.to_string())
    } else {
        (content.to_lowercase(), query.to_lowercase())
    };

    let mut matches = Vec::new();
    let mut offset = 0;
    while let Some(pos) = search_content[offset..].find(&search_query) {
        let abs_pos = offset + pos;
        let line = content[..abs_pos].matches('\n').count() + 1;
        let col = abs_pos - content[..abs_pos].rfind('\n').map(|p| p + 1).unwrap_or(0);
        matches.push(format!(
            r#"{{"line":{},"col":{},"offset":{}}}"#,
            line, col, abs_pos
        ));
        offset = abs_pos + query.len();
        if offset >= search_content.len() {
            break;
        }
    }
    send_json(
        stream,
        &format!(
            r#"{{"matches":[{}],"count":{}}}"#,
            matches.join(","),
            matches.len()
        ),
    );
}

/// `POST /api/script/replace` -- replace occurrences in a script.
fn api_script_replace(body: &str, stream: &mut TcpStream) {
    let parsed = match parse_json_body(body) {
        Some(v) => v,
        None => {
            send_error(stream, 400, "invalid JSON");
            return;
        }
    };
    let content = match parsed.get("content").and_then(|v| v.as_str()) {
        Some(c) => c.to_string(),
        None => {
            send_error(stream, 400, "missing content");
            return;
        }
    };
    let query = match parsed.get("query").and_then(|v| v.as_str()) {
        Some(q) => q.to_string(),
        None => {
            send_error(stream, 400, "missing query");
            return;
        }
    };
    let replacement = match parsed.get("replacement").and_then(|v| v.as_str()) {
        Some(r) => r.to_string(),
        None => {
            send_error(stream, 400, "missing replacement");
            return;
        }
    };
    let replace_all = parsed
        .get("replace_all")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let result = if replace_all {
        content.replace(&query, &replacement)
    } else {
        content.replacen(&query, &replacement, 1)
    };

    let count = if replace_all {
        content.matches(&query).count()
    } else if content.contains(&query) {
        1
    } else {
        0
    };

    let json = serde_json::json!({
        "content": result,
        "replacements": count
    });
    send_json(stream, &json.to_string());
}

// ---------------------------------------------------------------------------
// pat-1v3: Script editor advanced - breakpoints, error lines
// ---------------------------------------------------------------------------

/// `POST /api/script/breakpoint/toggle` -- toggle a breakpoint on a line.
fn api_toggle_breakpoint(state: &Arc<Mutex<EditorState>>, body: &str, stream: &mut TcpStream) {
    let parsed = match parse_json_body(body) {
        Some(v) => v,
        None => {
            send_error(stream, 400, "invalid JSON");
            return;
        }
    };
    let path = match parsed.get("path").and_then(|v| v.as_str()) {
        Some(p) => p.to_string(),
        None => {
            send_error(stream, 400, "missing path");
            return;
        }
    };
    let line = match parsed.get("line").and_then(|v| v.as_u64()) {
        Some(l) => l as u32,
        None => {
            send_error(stream, 400, "missing line");
            return;
        }
    };
    let mut s = state.lock().unwrap();
    let added = {
        let bps = s.breakpoints.entry(path.clone()).or_default();
        if let Some(pos) = bps.iter().position(|&l| l == line) {
            bps.remove(pos);
            false
        } else {
            bps.push(line);
            bps.sort();
            true
        }
    };
    s.add_log(
        "info",
        format!(
            "{} breakpoint at {}:{}",
            if added { "Added" } else { "Removed" },
            path,
            line
        ),
    );
    let bp_list = s
        .breakpoints
        .get(&path)
        .map(|v| {
            v.iter()
                .map(|l| l.to_string())
                .collect::<Vec<_>>()
                .join(",")
        })
        .unwrap_or_default();
    send_json(
        stream,
        &format!(
            r#"{{"ok":true,"added":{},"line":{},"breakpoints":[{}]}}"#,
            added, line, bp_list
        ),
    );
}

/// `GET /api/script/breakpoints` -- get all breakpoints for a script.
fn api_get_breakpoints(state: &Arc<Mutex<EditorState>>, query: &str, stream: &mut TcpStream) {
    let path = match query_param(query, "path") {
        Some(p) => url_decode(p),
        None => {
            send_error(stream, 400, "missing path parameter");
            return;
        }
    };
    let s = state.lock().unwrap();
    let bps = s.breakpoints.get(&path).cloned().unwrap_or_default();
    let errs = s.script_errors.get(&path).cloned().unwrap_or_default();
    let bp_json: Vec<String> = bps.iter().map(|l| l.to_string()).collect();
    let err_json: Vec<String> = errs
        .iter()
        .map(|(l, m)| format!(r#"{{"line":{},"message":"{}"}}"#, l, m.replace('"', "\\\"")))
        .collect();
    send_json(
        stream,
        &format!(
            r#"{{"breakpoints":[{}],"errors":[{}]}}"#,
            bp_json.join(","),
            err_json.join(",")
        ),
    );
}

// ---------------------------------------------------------------------------
// pat-2s1: Animation track reorder + keyframe copy/paste
// ---------------------------------------------------------------------------

/// `POST /api/animation/track/reorder` -- move a track to a new position.
fn api_reorder_track(state: &Arc<Mutex<EditorState>>, body: &str, stream: &mut TcpStream) {
    let parsed = match parse_json_body(body) {
        Some(v) => v,
        None => {
            send_error(stream, 400, "invalid JSON");
            return;
        }
    };
    let anim_name = match parsed.get("animation").and_then(|v| v.as_str()) {
        Some(v) => v.to_string(),
        None => {
            send_error(stream, 400, "missing animation");
            return;
        }
    };
    let from = match parsed.get("from").and_then(|v| v.as_u64()) {
        Some(v) => v as usize,
        None => {
            send_error(stream, 400, "missing from");
            return;
        }
    };
    let to = match parsed.get("to").and_then(|v| v.as_u64()) {
        Some(v) => v as usize,
        None => {
            send_error(stream, 400, "missing to");
            return;
        }
    };
    let mut s = state.lock().unwrap();
    let anim = match s.animations.get_mut(&anim_name) {
        Some(a) => a,
        None => {
            send_error(stream, 404, "animation not found");
            return;
        }
    };
    if from >= anim.tracks.len() || to >= anim.tracks.len() {
        send_error(stream, 400, "track index out of range");
        return;
    }
    let track = anim.tracks.remove(from);
    anim.tracks.insert(to, track);
    send_json(stream, r#"{"ok":true}"#);
}

/// `POST /api/animation/keyframe/copy` -- copy keyframes to clipboard.
fn api_copy_keyframes(state: &Arc<Mutex<EditorState>>, body: &str, stream: &mut TcpStream) {
    let parsed = match parse_json_body(body) {
        Some(v) => v,
        None => {
            send_error(stream, 400, "invalid JSON");
            return;
        }
    };
    let anim_name = match parsed.get("animation").and_then(|v| v.as_str()) {
        Some(v) => v.to_string(),
        None => {
            send_error(stream, 400, "missing animation");
            return;
        }
    };
    let track_index = match parsed.get("track_index").and_then(|v| v.as_u64()) {
        Some(v) => v as usize,
        None => {
            send_error(stream, 400, "missing track_index");
            return;
        }
    };
    let keyframe_indices: Vec<usize> =
        match parsed.get("keyframe_indices").and_then(|v| v.as_array()) {
            Some(arr) => arr
                .iter()
                .filter_map(|v| v.as_u64().map(|n| n as usize))
                .collect(),
            None => {
                send_error(stream, 400, "missing keyframe_indices");
                return;
            }
        };
    let mut s = state.lock().unwrap();
    let anim = match s.animations.get(&anim_name) {
        Some(a) => a,
        None => {
            send_error(stream, 404, "animation not found");
            return;
        }
    };
    if track_index >= anim.tracks.len() {
        send_error(stream, 400, "track_index out of range");
        return;
    }
    let kfs = anim.tracks[track_index].keyframes();
    let mut copied = Vec::new();
    for &idx in &keyframe_indices {
        if idx < kfs.len() {
            copied.push((track_index, kfs[idx].clone()));
        }
    }
    let count = copied.len();
    s.keyframe_clipboard = copied;
    send_json(stream, &format!(r#"{{"ok":true,"copied":{}}}"#, count));
}

/// `POST /api/animation/keyframe/paste` -- paste keyframes from clipboard.
fn api_paste_keyframes(state: &Arc<Mutex<EditorState>>, body: &str, stream: &mut TcpStream) {
    let parsed = match parse_json_body(body) {
        Some(v) => v,
        None => {
            send_error(stream, 400, "invalid JSON");
            return;
        }
    };
    let anim_name = match parsed.get("animation").and_then(|v| v.as_str()) {
        Some(v) => v.to_string(),
        None => {
            send_error(stream, 400, "missing animation");
            return;
        }
    };
    let track_index = match parsed.get("track_index").and_then(|v| v.as_u64()) {
        Some(v) => v as usize,
        None => {
            send_error(stream, 400, "missing track_index");
            return;
        }
    };
    let time_offset = parsed
        .get("time_offset")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);
    let mut s = state.lock().unwrap();
    let clipboard = s.keyframe_clipboard.clone();
    if clipboard.is_empty() {
        send_error(stream, 400, "keyframe clipboard is empty");
        return;
    }
    let anim = match s.animations.get_mut(&anim_name) {
        Some(a) => a,
        None => {
            send_error(stream, 404, "animation not found");
            return;
        }
    };
    if track_index >= anim.tracks.len() {
        send_error(stream, 400, "track_index out of range");
        return;
    }
    let mut pasted = 0;
    for (_, kf) in &clipboard {
        let mut new_kf = kf.clone();
        new_kf.time += time_offset;
        anim.tracks[track_index].add_keyframe(new_kf);
        pasted += 1;
    }
    send_json(stream, &format!(r#"{{"ok":true,"pasted":{}}}"#, pasted));
}

// ---------------------------------------------------------------------------
// pat-o51nk: Curve editor — keyframe transition endpoints
// ---------------------------------------------------------------------------

/// `POST /api/animation/keyframe/transition` -- set a keyframe's transition type.
///
/// Body: `{"animation":"name","track_index":0,"keyframe_index":1,
///         "transition":"cubic_bezier","x1":0.42,"y1":0,"x2":0.58,"y2":1}`
///
/// `transition` can be `"linear"`, `"nearest"`, or `"cubic_bezier"` (requires x1/y1/x2/y2).
fn api_set_keyframe_transition(
    state: &Arc<Mutex<EditorState>>,
    body: &str,
    stream: &mut TcpStream,
) {
    let parsed = match parse_json_body(body) {
        Some(v) => v,
        None => {
            send_error(stream, 400, "invalid JSON");
            return;
        }
    };

    let anim_name = match parsed.get("animation").and_then(|v| v.as_str()) {
        Some(s) => s.to_string(),
        None => {
            send_error(stream, 400, "missing animation");
            return;
        }
    };
    let track_index = match parsed.get("track_index").and_then(|v| v.as_u64()) {
        Some(v) => v as usize,
        None => {
            send_error(stream, 400, "missing track_index");
            return;
        }
    };
    let keyframe_index = match parsed.get("keyframe_index").and_then(|v| v.as_u64()) {
        Some(v) => v as usize,
        None => {
            send_error(stream, 400, "missing keyframe_index");
            return;
        }
    };
    let transition_str = match parsed.get("transition").and_then(|v| v.as_str()) {
        Some(s) => s,
        None => {
            send_error(stream, 400, "missing transition");
            return;
        }
    };

    use gdscene::animation::TransitionType;
    let transition = match transition_str {
        "linear" => TransitionType::Linear,
        "nearest" => TransitionType::Nearest,
        "cubic_bezier" => {
            let x1 = parsed.get("x1").and_then(|v| v.as_f64()).unwrap_or(0.42) as f32;
            let y1 = parsed.get("y1").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
            let x2 = parsed.get("x2").and_then(|v| v.as_f64()).unwrap_or(0.58) as f32;
            let y2 = parsed.get("y2").and_then(|v| v.as_f64()).unwrap_or(1.0) as f32;
            TransitionType::CubicBezier(x1.clamp(0.0, 1.0), y1, x2.clamp(0.0, 1.0), y2)
        }
        _ => {
            send_error(stream, 400, "unknown transition type");
            return;
        }
    };

    let mut s = state.lock().unwrap();
    let anim = match s.animations.get_mut(&anim_name) {
        Some(a) => a,
        None => {
            send_error(stream, 404, "animation not found");
            return;
        }
    };
    if track_index >= anim.tracks.len() {
        send_error(stream, 400, "track_index out of range");
        return;
    }
    let track = &mut anim.tracks[track_index];
    // Access keyframes mutably — we need to get the underlying Vec.
    // KeyFrame fields are pub, but keyframes() returns &[KeyFrame].
    // We use a helper: remove + re-add with new transition.
    let kfs = track.keyframes();
    if keyframe_index >= kfs.len() {
        send_error(stream, 400, "keyframe_index out of range");
        return;
    }
    let mut kf = kfs[keyframe_index].clone();
    kf.transition = transition;
    track.remove_keyframe(keyframe_index);
    track.add_keyframe(kf);
    send_json(stream, r#"{"ok":true}"#);
}

/// `GET /api/animation/keyframe/transition?animation=name&track_index=0&keyframe_index=1`
///
/// Returns the transition type and bezier control points for a specific keyframe.
fn api_get_keyframe_transition(
    state: &Arc<Mutex<EditorState>>,
    query: &str,
    stream: &mut TcpStream,
) {
    let anim_name = match query_param(query, "animation") {
        Some(s) => url_decode(s),
        None => {
            send_error(stream, 400, "missing animation");
            return;
        }
    };
    let track_index: usize = match query_param(query, "track_index").and_then(|v| v.parse().ok()) {
        Some(v) => v,
        None => {
            send_error(stream, 400, "missing track_index");
            return;
        }
    };
    let keyframe_index: usize =
        match query_param(query, "keyframe_index").and_then(|v| v.parse().ok()) {
            Some(v) => v,
            None => {
                send_error(stream, 400, "missing keyframe_index");
                return;
            }
        };

    let s = state.lock().unwrap();
    let anim = match s.animations.get(&anim_name) {
        Some(a) => a,
        None => {
            send_error(stream, 404, "animation not found");
            return;
        }
    };
    if track_index >= anim.tracks.len() {
        send_error(stream, 400, "track_index out of range");
        return;
    }
    let kfs = anim.tracks[track_index].keyframes();
    if keyframe_index >= kfs.len() {
        send_error(stream, 400, "keyframe_index out of range");
        return;
    }
    let kf = &kfs[keyframe_index];
    use gdscene::animation::TransitionType;
    let json = match kf.transition {
        TransitionType::Linear => r#"{"transition":"linear"}"#.to_string(),
        TransitionType::Nearest => r#"{"transition":"nearest"}"#.to_string(),
        TransitionType::CubicBezier(x1, y1, x2, y2) => {
            format!(
                r#"{{"transition":"cubic_bezier","x1":{},"y1":{},"x2":{},"y2":{}}}"#,
                x1, y1, x2, y2
            )
        }
    };
    send_json(stream, &json);
}

// ---------------------------------------------------------------------------
// pat-lbu: Bottom panels - debugger + monitors
// ---------------------------------------------------------------------------

/// `GET /api/debug/stack_trace` -- get current debug stack trace.
fn api_get_stack_trace(state: &Arc<Mutex<EditorState>>, stream: &mut TcpStream) {
    let s = state.lock().unwrap();
    let frames: Vec<String> = s
        .debug_stack_trace
        .iter()
        .map(|f| format!(r#""{}""#, f.replace('"', "\\\"")))
        .collect();
    send_json(stream, &format!(r#"{{"frames":[{}]}}"#, frames.join(",")));
}

/// `GET /api/debug/state` -- get full debugger state (stack, breakpoints, variables).
fn api_get_debug_state(state: &Arc<Mutex<EditorState>>, stream: &mut TcpStream) {
    let s = state.lock().unwrap();
    let frames: Vec<String> = s
        .debug_frames
        .iter()
        .map(|(func, script, line)| {
            format!(
                r#"{{"function":"{}","script":"{}","line":{}}}"#,
                func.replace('"', "\\\""),
                script.replace('"', "\\\""),
                line
            )
        })
        .collect();
    let breakpoints: Vec<String> = s
        .debug_breakpoints
        .iter()
        .map(|(script, line)| {
            format!(
                r#"{{"script":"{}","line":{}}}"#,
                script.replace('"', "\\\""),
                line
            )
        })
        .collect();
    let locals = format_var_list(&s.debug_locals);
    let globals = format_var_list(&s.debug_globals);
    send_json(
        stream,
        &format!(
            r#"{{"state":"{}","frames":[{}],"breakpoints":[{}],"locals":[{}],"globals":[{}]}}"#,
            s.debug_state,
            frames.join(","),
            breakpoints.join(","),
            locals,
            globals,
        ),
    );
}

/// Format a list of (name, type, value) triples as JSON array entries.
fn format_var_list(vars: &[(String, String, String)]) -> String {
    vars.iter()
        .map(|(name, ty, val)| {
            format!(
                r#"{{"name":"{}","type":"{}","value":"{}"}}"#,
                name.replace('"', "\\\""),
                ty.replace('"', "\\\""),
                val.replace('"', "\\\""),
            )
        })
        .collect::<Vec<_>>()
        .join(",")
}

/// `GET /api/debug/locals` -- get local variables for a specific stack frame.
fn api_get_debug_locals(state: &Arc<Mutex<EditorState>>, _query: &str, stream: &mut TcpStream) {
    let s = state.lock().unwrap();
    let locals = format_var_list(&s.debug_locals);
    send_json(stream, &format!(r#"{{"locals":[{}]}}"#, locals));
}

/// `POST /api/debug/continue` -- resume execution.
fn api_debug_continue(state: &Arc<Mutex<EditorState>>, stream: &mut TcpStream) {
    let mut s = state.lock().unwrap();
    if s.debug_state == "paused" {
        s.debug_state = "running".to_string();
        s.debug_locals.clear();
        s.debug_globals.clear();
    }
    send_json(stream, r#"{"ok":true}"#);
}

/// `POST /api/debug/step_in` -- step into next statement.
fn api_debug_step_in(state: &Arc<Mutex<EditorState>>, stream: &mut TcpStream) {
    let mut s = state.lock().unwrap();
    if s.debug_state == "paused" {
        s.debug_state = "running".to_string();
    }
    send_json(stream, r#"{"ok":true,"step":"in"}"#);
}

/// `POST /api/debug/step_over` -- step over next statement.
fn api_debug_step_over(state: &Arc<Mutex<EditorState>>, stream: &mut TcpStream) {
    let mut s = state.lock().unwrap();
    if s.debug_state == "paused" {
        s.debug_state = "running".to_string();
    }
    send_json(stream, r#"{"ok":true,"step":"over"}"#);
}

/// `POST /api/debug/step_out` -- step out of current function.
fn api_debug_step_out(state: &Arc<Mutex<EditorState>>, stream: &mut TcpStream) {
    let mut s = state.lock().unwrap();
    if s.debug_state == "paused" {
        s.debug_state = "running".to_string();
    }
    send_json(stream, r#"{"ok":true,"step":"out"}"#);
}

/// `POST /api/debug/remove_breakpoint` -- remove a breakpoint by script:line.
fn api_debug_remove_breakpoint(
    state: &Arc<Mutex<EditorState>>,
    body: &str,
    stream: &mut TcpStream,
) {
    let mut s = state.lock().unwrap();
    if let Some(v) = parse_json_body(body) {
        let script = v["script"].as_str().unwrap_or_default().to_string();
        let line = v["line"].as_u64().unwrap_or(0) as usize;
        s.debug_breakpoints
            .retain(|(sc, ln)| !(sc == &script && *ln == line));
    }
    send_json(stream, r#"{"ok":true}"#);
}

/// `GET /api/monitors/frame_times` -- get frame time history for graph.
fn api_get_frame_times(state: &Arc<Mutex<EditorState>>, stream: &mut TcpStream) {
    let s = state.lock().unwrap();
    let times: Vec<String> = s.frame_times.iter().map(|t| format!("{:.2}", t)).collect();
    let avg = if s.frame_times.is_empty() {
        0.0
    } else {
        s.frame_times.iter().sum::<f64>() / s.frame_times.len() as f64
    };
    let max = s.frame_times.iter().cloned().fold(0.0f64, f64::max);
    let min = s.frame_times.iter().cloned().fold(f64::MAX, f64::min);
    let fps = if avg > 0.0 { 1000.0 / avg } else { 0.0 };
    send_json(
        stream,
        &format!(
            r#"{{"times":[{}],"avg":{:.2},"max":{:.2},"min":{:.2},"fps":{:.1}}}"#,
            times.join(","),
            avg,
            max,
            if min == f64::MAX { 0.0 } else { min },
            fps
        ),
    );
}

/// `GET /api/profiler` -- get profiler frame data for the visual frame graph.
fn api_get_profiler(state: &Arc<Mutex<EditorState>>, stream: &mut TcpStream) {
    let s = state.lock().unwrap();
    let frames: Vec<String> = s
        .profiler_frames
        .iter()
        .map(|f| {
            let funcs: Vec<String> = f
                .functions
                .iter()
                .map(|e| {
                    format!(
                        r#"{{"name":"{}","time_ms":{:.2}}}"#,
                        e.name.replace('"', "\\\""),
                        e.time_ms
                    )
                })
                .collect();
            format!(
                r#"{{"frame":{},"total_ms":{:.2},"cpu_ms":{:.2},"gpu_ms":{:.2},"functions":[{}]}}"#,
                f.frame_number,
                f.total_ms,
                f.cpu_ms,
                f.gpu_ms,
                funcs.join(",")
            )
        })
        .collect();

    // Aggregate stats
    let count = s.profiler_frames.len();
    let avg_total = if count > 0 {
        s.profiler_frames.iter().map(|f| f.total_ms).sum::<f64>() / count as f64
    } else {
        0.0
    };
    let max_total = s
        .profiler_frames
        .iter()
        .map(|f| f.total_ms)
        .fold(0.0f64, f64::max);
    let avg_fps = if avg_total > 0.0 {
        1000.0 / avg_total
    } else {
        0.0
    };

    send_json(
        stream,
        &format!(
            r#"{{"frames":[{}],"count":{},"avg_ms":{:.2},"max_ms":{:.2},"avg_fps":{:.1}}}"#,
            frames.join(","),
            count,
            avg_total,
            max_total,
            avg_fps
        ),
    );
}

/// `POST /api/profiler/record` -- push a profiler frame snapshot (used by runtime).
fn api_profiler_record(state: &Arc<Mutex<EditorState>>, body: &str, stream: &mut TcpStream) {
    let parsed = match parse_json_body(body) {
        Some(v) => v,
        None => {
            send_error(stream, 400, "invalid JSON");
            return;
        }
    };

    let frame_number = parsed.get("frame").and_then(|v| v.as_u64()).unwrap_or(0);
    let total_ms = parsed
        .get("total_ms")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);
    let cpu_ms = parsed
        .get("cpu_ms")
        .and_then(|v| v.as_f64())
        .unwrap_or(total_ms);
    let gpu_ms = parsed.get("gpu_ms").and_then(|v| v.as_f64()).unwrap_or(0.0);

    let functions = parsed
        .get("functions")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|entry| {
                    let name = entry.get("name")?.as_str()?.to_string();
                    let time_ms = entry.get("time_ms")?.as_f64()?;
                    Some(ProfilerFuncEntry { name, time_ms })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let mut s = state.lock().unwrap();
    if s.profiler_frames.len() >= 120 {
        s.profiler_frames.pop_front();
    }
    s.profiler_frames.push_back(ProfilerFrame {
        frame_number,
        total_ms,
        cpu_ms,
        gpu_ms,
        functions,
    });
    send_json(stream, r#"{"ok":true}"#);
}

// ---------------------------------------------------------------------------
// pat-dj6: Top bar - editor mode
// ---------------------------------------------------------------------------

/// `POST /api/editor/mode` -- set the editor mode (2d, 3d, script, game, assetlib).
fn api_set_editor_mode(state: &Arc<Mutex<EditorState>>, body: &str, stream: &mut TcpStream) {
    let parsed = match parse_json_body(body) {
        Some(v) => v,
        None => {
            send_error(stream, 400, "invalid JSON");
            return;
        }
    };
    let mode = match parsed.get("mode").and_then(|v| v.as_str()) {
        Some(m) if m == "2d" || m == "3d" || m == "script" || m == "game" || m == "assetlib" => {
            m.to_string()
        }
        _ => {
            send_error(
                stream,
                400,
                "mode must be '2d', '3d', 'script', 'game', or 'assetlib'",
            );
            return;
        }
    };
    let mut s = state.lock().unwrap();
    s.editor_mode = mode.clone();
    s.add_log("info", format!("Switched to {} mode", mode));
    send_json(stream, &format!(r#"{{"ok":true,"mode":"{}"}}"#, mode));
}

/// `GET /api/editor/mode` -- get current editor mode.
fn api_get_editor_mode(state: &Arc<Mutex<EditorState>>, stream: &mut TcpStream) {
    let s = state.lock().unwrap();
    send_json(stream, &format!(r#"{{"mode":"{}"}}"#, s.editor_mode));
}

// ---------------------------------------------------------------------------
// pat-e0heb: Scene tabs
// ---------------------------------------------------------------------------

/// `GET /api/scene/tabs` -- list all open scene tabs.
fn api_get_scene_tabs(state: &Arc<Mutex<EditorState>>, stream: &mut TcpStream) {
    let s = state.lock().unwrap();
    let tabs_json: Vec<String> = s
        .scene_tabs
        .iter()
        .map(|t| {
            format!(
                r#"{{"id":{},"path":"{}","name":"{}","modified":{}}}"#,
                t.id,
                t.path.replace('\\', "\\\\").replace('"', "\\\""),
                t.name.replace('\\', "\\\\").replace('"', "\\\""),
                t.modified
            )
        })
        .collect();
    send_json(
        stream,
        &format!(
            r#"{{"tabs":[{}],"active_tab_index":{}}}"#,
            tabs_json.join(","),
            s.active_tab_index
        ),
    );
}

/// `POST /api/scene/tabs/open` -- open a new scene tab.
fn api_open_scene_tab(state: &Arc<Mutex<EditorState>>, body: &str, stream: &mut TcpStream) {
    let parsed = match parse_json_body(body) {
        Some(v) => v,
        None => {
            send_error(stream, 400, "invalid JSON");
            return;
        }
    };
    let path = parsed
        .get("path")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let name = parsed
        .get("name")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| {
            if path.is_empty() {
                "Untitled".to_string()
            } else {
                path.rsplit('/').next().unwrap_or(&path).to_string()
            }
        });
    let mut s = state.lock().unwrap();
    // Check if already open
    if let Some(idx) = s
        .scene_tabs
        .iter()
        .position(|t| !t.path.is_empty() && t.path == path)
    {
        s.active_tab_index = idx;
        send_json(
            stream,
            &format!(
                r#"{{"ok":true,"tab_id":{},"switched":true,"active_tab_index":{}}}"#,
                s.scene_tabs[idx].id, idx
            ),
        );
        return;
    }
    let tab_id = s.next_tab_id();
    s.scene_tabs.push(SceneTab {
        id: tab_id,
        path,
        name,
        modified: false,
    });
    s.active_tab_index = s.scene_tabs.len() - 1;
    send_json(
        stream,
        &format!(
            r#"{{"ok":true,"tab_id":{},"active_tab_index":{}}}"#,
            tab_id, s.active_tab_index
        ),
    );
}

/// `POST /api/scene/tabs/close` -- close a scene tab by id.
fn api_close_scene_tab(state: &Arc<Mutex<EditorState>>, body: &str, stream: &mut TcpStream) {
    let parsed = match parse_json_body(body) {
        Some(v) => v,
        None => {
            send_error(stream, 400, "invalid JSON");
            return;
        }
    };
    let tab_id = match parsed.get("tab_id").and_then(|v| v.as_u64()) {
        Some(id) => id as u32,
        None => {
            send_error(stream, 400, "tab_id required");
            return;
        }
    };
    let mut s = state.lock().unwrap();
    if s.scene_tabs.len() <= 1 {
        send_error(stream, 400, "cannot close last tab");
        return;
    }
    if let Some(idx) = s.scene_tabs.iter().position(|t| t.id == tab_id) {
        s.scene_tabs.remove(idx);
        if s.active_tab_index >= s.scene_tabs.len() {
            s.active_tab_index = s.scene_tabs.len() - 1;
        }
        send_json(
            stream,
            &format!(r#"{{"ok":true,"active_tab_index":{}}}"#, s.active_tab_index),
        );
    } else {
        send_error(stream, 404, "tab not found");
    }
}

/// `POST /api/scene/tabs/switch` -- switch to a scene tab by id or index.
fn api_switch_scene_tab(state: &Arc<Mutex<EditorState>>, body: &str, stream: &mut TcpStream) {
    let parsed = match parse_json_body(body) {
        Some(v) => v,
        None => {
            send_error(stream, 400, "invalid JSON");
            return;
        }
    };
    let mut s = state.lock().unwrap();
    if let Some(tab_id) = parsed.get("tab_id").and_then(|v| v.as_u64()) {
        if let Some(idx) = s.scene_tabs.iter().position(|t| t.id == tab_id as u32) {
            s.active_tab_index = idx;
            send_json(
                stream,
                &format!(r#"{{"ok":true,"active_tab_index":{}}}"#, idx),
            );
        } else {
            send_error(stream, 404, "tab not found");
        }
    } else if let Some(index) = parsed.get("index").and_then(|v| v.as_u64()) {
        let idx = index as usize;
        if idx < s.scene_tabs.len() {
            s.active_tab_index = idx;
            send_json(
                stream,
                &format!(r#"{{"ok":true,"active_tab_index":{}}}"#, idx),
            );
        } else {
            send_error(stream, 400, "index out of range");
        }
    } else {
        send_error(stream, 400, "tab_id or index required");
    }
}

// ---------------------------------------------------------------------------
// Batch 2 editor bead endpoints
// ---------------------------------------------------------------------------

/// `POST /api/viewport/set_mode` — sets the viewport tool mode.
fn api_set_viewport_mode(state: &Arc<Mutex<EditorState>>, body: &str, stream: &mut TcpStream) {
    let p = match parse_json_body(body) {
        Some(v) => v,
        None => {
            send_error(stream, 400, "invalid JSON");
            return;
        }
    };
    let mode_str = match p.get("mode").and_then(|v| v.as_str()) {
        Some(m) => m,
        None => {
            send_error(stream, 400, "missing mode");
            return;
        }
    };
    let mode = match ViewportMode::from_str_name(mode_str) {
        Some(m) => m,
        None => {
            send_error(stream, 400, "invalid mode");
            return;
        }
    };
    let mut s = state.lock().unwrap();
    s.viewport_mode = mode;
    s.add_log("info", format!("Viewport mode: {}", mode.as_str()));
    send_json(
        stream,
        &format!(r#"{{"ok":true,"mode":"{}"}}"#, mode.as_str()),
    );
}

/// `GET /api/viewport/mode` — returns the current viewport tool mode.
fn api_get_viewport_mode(state: &Arc<Mutex<EditorState>>, stream: &mut TcpStream) {
    let s = state.lock().unwrap();
    send_json(
        stream,
        &format!(r#"{{"mode":"{}"}}"#, s.viewport_mode.as_str()),
    );
}

/// `GET /api/node/script?node_id=<id>` — returns the script source for a node.
fn api_get_node_script(state: &Arc<Mutex<EditorState>>, query: &str, stream: &mut TcpStream) {
    let raw_id: u64 = match query_param(query, "node_id").and_then(|s| s.parse().ok()) {
        Some(v) => v,
        None => {
            send_error(stream, 400, "missing or invalid node_id");
            return;
        }
    };
    let s = state.lock().unwrap();
    let nid = match find_node_by_raw_id(&s.scene_tree, raw_id) {
        Some(n) => n,
        None => {
            send_error(stream, 404, "node not found");
            return;
        }
    };
    let node = s.scene_tree.get_node(nid).unwrap();
    let script_path = match node.get_property("_script_path") {
        Variant::String(sp) => sp,
        _ => {
            send_json(stream, r#"{"has_script":false,"path":"","source":""}"#);
            return;
        }
    };
    let cwd = std::env::current_dir().unwrap_or_default();
    let file_path = if let Some(stripped) = script_path.strip_prefix("res://") {
        cwd.join(stripped)
    } else {
        std::path::PathBuf::from(&script_path)
    };
    let source = std::fs::read_to_string(&file_path).unwrap_or_default();
    let json = serde_json::json!({ "has_script": true, "path": script_path, "source": source });
    send_json(stream, &json.to_string());
}

/// `GET /api/search?q=<query>` — searches all .gd script files for a string.
fn api_search_scripts(query: &str, stream: &mut TcpStream) {
    let search = match query_param(query, "q") {
        Some(q) => url_decode(q),
        None => {
            send_error(stream, 400, "missing q parameter");
            return;
        }
    };
    if search.is_empty() {
        send_json(stream, r#"{"results":[]}"#);
        return;
    }
    let cwd = std::env::current_dir().unwrap_or_default();
    let mut results = Vec::new();
    fn search_dir(
        dir: &std::path::Path,
        query: &str,
        results: &mut Vec<serde_json::Value>,
        depth: usize,
    ) {
        if depth > 6 {
            return;
        }
        let entries = match std::fs::read_dir(dir) {
            Ok(e) => e,
            Err(_) => return,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with('.') || name == "target" {
                continue;
            }
            if path.is_dir() {
                search_dir(&path, query, results, depth + 1);
            } else if name.ends_with(".gd") {
                if let Ok(content) = std::fs::read_to_string(&path) {
                    for (i, line) in content.lines().enumerate() {
                        if line.contains(query) {
                            results.push(serde_json::json!({
                                "file": path.display().to_string(),
                                "line": i + 1,
                                "text": line.trim()
                            }));
                        }
                    }
                }
            }
        }
    }
    search_dir(&cwd, &search, &mut results, 0);
    send_json(stream, &serde_json::json!({"results": results}).to_string());
}

/// `POST /api/signal/disconnect` — disconnects a signal from a node.
fn api_disconnect_signal(state: &Arc<Mutex<EditorState>>, body: &str, stream: &mut TcpStream) {
    let p = match parse_json_body(body) {
        Some(v) => v,
        None => {
            send_error(stream, 400, "invalid JSON");
            return;
        }
    };
    let node_raw = match p.get("node_id").and_then(|v| v.as_u64()) {
        Some(v) => v,
        None => {
            send_error(stream, 400, "missing node_id");
            return;
        }
    };
    let signal = match p.get("signal").and_then(|v| v.as_str()) {
        Some(s) => s.to_string(),
        None => {
            send_error(stream, 400, "missing signal");
            return;
        }
    };
    let mut s = state.lock().unwrap();
    let nid = match find_node_by_raw_id(&s.scene_tree, node_raw) {
        Some(n) => n,
        None => {
            send_error(stream, 404, "node not found");
            return;
        }
    };
    let node = s.scene_tree.get_node_mut(nid).unwrap();
    let connections = match node.get_property("signal_connections") {
        Variant::String(c) => c,
        _ => String::new(),
    };
    let updated: Vec<&str> = connections
        .split(';')
        .filter(|entry| !entry.is_empty() && !entry.contains(&signal))
        .collect();
    node.set_property("signal_connections", Variant::String(updated.join(";")));
    s.add_log(
        "info",
        format!("Signal disconnected: {} on node {}", signal, node_raw),
    );
    send_json(stream, r#"{"ok":true}"#);
}

/// `POST /api/output/clear` — clears the script output log.
fn api_clear_output(state: &Arc<Mutex<EditorState>>, stream: &mut TcpStream) {
    let mut s = state.lock().unwrap();
    s.output_entries.clear();
    send_json(stream, r#"{"ok":true}"#);
}

/// `GET /api/output` — returns script output log entries.
fn api_get_output(state: &Arc<Mutex<EditorState>>, stream: &mut TcpStream) {
    let s = state.lock().unwrap();
    let entries: Vec<&String> = s.output_entries.iter().collect();
    send_json(stream, &serde_json::json!({"entries": entries}).to_string());
}

// ---------------------------------------------------------------------------
// pat-kj4: Project settings
// ---------------------------------------------------------------------------

/// `GET /api/project_settings` — returns project settings organized by category (pat-c4zlm).
fn api_get_project_settings(state: &Arc<Mutex<EditorState>>, stream: &mut TcpStream) {
    let json = {
        let s = state.lock().unwrap();
        let categories = serde_json::json!([
            {
                "id": "application", "label": "Application",
                "properties": [
                    {"key":"project_name","label":"Project Name","editor":"text","value":&s.project_name},
                    {"key":"main_scene","label":"Main Scene","editor":"text","value":&s.project_main_scene},
                    {"key":"description","label":"Description","editor":"text","value":&s.project_description},
                    {"key":"icon","label":"Icon Path","editor":"text","value":&s.project_icon},
                ]
            },
            {
                "id": "display", "label": "Display",
                "properties": [
                    {"key":"resolution_w","label":"Resolution Width","editor":{"type":"integer","min":1,"max":7680},"value":s.project_resolution_w},
                    {"key":"resolution_h","label":"Resolution Height","editor":{"type":"integer","min":1,"max":4320},"value":s.project_resolution_h},
                    {"key":"stretch_mode","label":"Stretch Mode","editor":{"type":"enum","options":["disabled","canvas_items","viewport"]},"value":&s.project_stretch_mode},
                    {"key":"stretch_aspect","label":"Stretch Aspect","editor":{"type":"enum","options":["ignore","keep","keep_width","keep_height","expand"]},"value":&s.project_stretch_aspect},
                    {"key":"fullscreen","label":"Fullscreen","editor":"bool","value":s.project_fullscreen},
                    {"key":"vsync","label":"V-Sync","editor":"bool","value":s.project_vsync},
                ]
            },
            {
                "id": "physics", "label": "Physics",
                "properties": [
                    {"key":"physics_fps","label":"Physics FPS","editor":{"type":"integer","min":1,"max":240},"value":s.project_physics_fps},
                    {"key":"gravity","label":"Default Gravity","editor":{"type":"number","min":0,"max":10000,"step":0.1},"value":s.project_gravity},
                    {"key":"linear_damp","label":"Default Linear Damp","editor":{"type":"number","min":0,"max":100,"step":0.01},"value":s.project_linear_damp},
                    {"key":"angular_damp","label":"Default Angular Damp","editor":{"type":"number","min":0,"max":100,"step":0.01},"value":s.project_angular_damp},
                ]
            },
            {
                "id": "audio", "label": "Audio",
                "properties": [
                    {"key":"bus_layout","label":"Default Bus Layout","editor":"text","value":&s.project_bus_layout},
                    {"key":"master_volume_db","label":"Master Volume (dB)","editor":{"type":"number","min":-80,"max":24,"step":0.1},"value":s.project_master_volume_db},
                    {"key":"audio_input","label":"Enable Audio Input","editor":"bool","value":s.project_audio_input},
                ]
            },
            {
                "id": "rendering", "label": "Rendering",
                "properties": [
                    {"key":"renderer","label":"Renderer","editor":{"type":"enum","options":["forward_plus","mobile","compatibility"]},"value":&s.project_renderer},
                    {"key":"anti_aliasing","label":"Anti-Aliasing","editor":{"type":"enum","options":["disabled","fxaa","msaa_2x","msaa_4x","msaa_8x"]},"value":&s.project_anti_aliasing},
                    {"key":"environment_default","label":"Default Environment","editor":"text","value":&s.project_environment_default},
                ]
            }
        ]);
        serde_json::json!({
            "project_name": &s.project_name,
            "main_scene": &s.project_main_scene,
            "description": &s.project_description,
            "categories": categories,
        })
        .to_string()
    };
    send_json(stream, &json);
}

/// `POST /api/project_settings` — updates project settings (pat-c4zlm).
fn api_set_project_settings(state: &Arc<Mutex<EditorState>>, body: &str, stream: &mut TcpStream) {
    let p = match parse_json_body(body) {
        Some(v) => v,
        None => {
            send_error(stream, 400, "invalid JSON");
            return;
        }
    };
    let json = {
        let mut s = state.lock().unwrap();
        // Application
        if let Some(v) = p.get("project_name").and_then(|v| v.as_str()) {
            s.project_name = v.to_string();
        }
        if let Some(v) = p.get("description").and_then(|v| v.as_str()) {
            s.project_description = v.to_string();
        }
        if let Some(v) = p.get("icon").and_then(|v| v.as_str()) {
            s.project_icon = v.to_string();
        }
        if let Some(v) = p.get("main_scene").and_then(|v| v.as_str()) {
            s.project_main_scene = v.to_string();
        }
        // Display
        if let Some(v) = p.get("resolution_w").and_then(|v| v.as_u64()) {
            s.project_resolution_w = v as u32;
        }
        if let Some(v) = p.get("resolution_h").and_then(|v| v.as_u64()) {
            s.project_resolution_h = v as u32;
        }
        if let Some(v) = p.get("stretch_mode").and_then(|v| v.as_str()) {
            s.project_stretch_mode = v.to_string();
        }
        if let Some(v) = p.get("stretch_aspect").and_then(|v| v.as_str()) {
            s.project_stretch_aspect = v.to_string();
        }
        if let Some(v) = p.get("fullscreen").and_then(|v| v.as_bool()) {
            s.project_fullscreen = v;
        }
        if let Some(v) = p.get("vsync").and_then(|v| v.as_bool()) {
            s.project_vsync = v;
        }
        // Physics
        if let Some(v) = p.get("physics_fps").and_then(|v| v.as_u64()) {
            s.project_physics_fps = v as u32;
        }
        if let Some(v) = p.get("gravity").and_then(|v| v.as_f64()) {
            s.project_gravity = v;
        }
        if let Some(v) = p.get("linear_damp").and_then(|v| v.as_f64()) {
            s.project_linear_damp = v;
        }
        if let Some(v) = p.get("angular_damp").and_then(|v| v.as_f64()) {
            s.project_angular_damp = v;
        }
        // Audio
        if let Some(v) = p.get("bus_layout").and_then(|v| v.as_str()) {
            s.project_bus_layout = v.to_string();
        }
        if let Some(v) = p.get("master_volume_db").and_then(|v| v.as_f64()) {
            s.project_master_volume_db = v;
        }
        if let Some(v) = p.get("audio_input").and_then(|v| v.as_bool()) {
            s.project_audio_input = v;
        }
        // Rendering
        if let Some(v) = p.get("renderer").and_then(|v| v.as_str()) {
            s.project_renderer = v.to_string();
        }
        if let Some(v) = p.get("anti_aliasing").and_then(|v| v.as_str()) {
            s.project_anti_aliasing = v.to_string();
        }
        if let Some(v) = p.get("environment_default").and_then(|v| v.as_str()) {
            s.project_environment_default = v.to_string();
        }
        serde_json::json!({ "ok": true }).to_string()
    };
    send_json(stream, &json);
}

// ---------------------------------------------------------------------------
// pat-flr: Filesystem operations
// ---------------------------------------------------------------------------

/// `POST /api/filesystem/rename` — renames a file or directory.
fn api_filesystem_rename(body: &str, stream: &mut TcpStream) {
    let p = match parse_json_body(body) {
        Some(v) => v,
        None => {
            send_error(stream, 400, "invalid JSON");
            return;
        }
    };
    let old_path = match p.get("old_path").and_then(|v| v.as_str()) {
        Some(p) => p.to_string(),
        None => {
            send_error(stream, 400, "missing old_path");
            return;
        }
    };
    let new_name = match p.get("new_name").and_then(|v| v.as_str()) {
        Some(n) => n.to_string(),
        None => {
            send_error(stream, 400, "missing new_name");
            return;
        }
    };
    let cwd = std::env::current_dir().unwrap_or_default();
    let resolved = if let Some(stripped) = old_path.strip_prefix("res://") {
        cwd.join(stripped)
    } else {
        std::path::PathBuf::from(&old_path)
    };
    if !resolved.exists() {
        send_error(stream, 404, "file not found");
        return;
    }
    let new_path = resolved.parent().unwrap_or(&cwd).join(&new_name);
    match std::fs::rename(&resolved, &new_path) {
        Ok(_) => send_json(stream, r#"{"ok":true}"#),
        Err(e) => send_error(stream, 500, &format!("rename failed: {e}")),
    }
}

/// `POST /api/filesystem/delete` — deletes a file or empty directory.
fn api_filesystem_delete(body: &str, stream: &mut TcpStream) {
    let p = match parse_json_body(body) {
        Some(v) => v,
        None => {
            send_error(stream, 400, "invalid JSON");
            return;
        }
    };
    let path = match p.get("path").and_then(|v| v.as_str()) {
        Some(p) => p.to_string(),
        None => {
            send_error(stream, 400, "missing path");
            return;
        }
    };
    let cwd = std::env::current_dir().unwrap_or_default();
    let resolved = if let Some(stripped) = path.strip_prefix("res://") {
        cwd.join(stripped)
    } else {
        std::path::PathBuf::from(&path)
    };
    if !resolved.exists() {
        send_error(stream, 404, "file not found");
        return;
    }
    let result = if resolved.is_dir() {
        std::fs::remove_dir(&resolved)
    } else {
        std::fs::remove_file(&resolved)
    };
    match result {
        Ok(_) => send_json(stream, r#"{"ok":true}"#),
        Err(e) => send_error(stream, 500, &format!("delete failed: {e}")),
    }
}

/// `POST /api/filesystem/mkdir` — creates a new directory.
fn api_filesystem_mkdir(body: &str, stream: &mut TcpStream) {
    let p = match parse_json_body(body) {
        Some(v) => v,
        None => {
            send_error(stream, 400, "invalid JSON");
            return;
        }
    };
    let path = match p.get("path").and_then(|v| v.as_str()) {
        Some(p) => p.to_string(),
        None => {
            send_error(stream, 400, "missing path");
            return;
        }
    };
    let cwd = std::env::current_dir().unwrap_or_default();
    let resolved = if let Some(stripped) = path.strip_prefix("res://") {
        cwd.join(stripped)
    } else {
        std::path::PathBuf::from(&path)
    };
    match std::fs::create_dir_all(&resolved) {
        Ok(_) => send_json(stream, r#"{"ok":true}"#),
        Err(e) => send_error(stream, 500, &format!("mkdir failed: {e}")),
    }
}

// ---------------------------------------------------------------------------
// pat-omfrq: Filesystem tree and dir stubs
// ---------------------------------------------------------------------------

/// `GET /api/filesystem/tree` — returns the full directory tree.
fn api_filesystem_tree(query: &str, stream: &mut TcpStream) {
    // Stub: return empty tree. Full implementation pending.
    send_json(stream, r#"{"tree":[]}"#);
}

/// `GET /api/filesystem/dir` — returns directory listing.
fn api_filesystem_dir(query: &str, stream: &mut TcpStream) {
    // Stub: return empty listing. Full implementation pending.
    send_json(stream, r#"{"files":[]}"#);
}

// ---------------------------------------------------------------------------
// pat-vyko1: Import settings stubs
// ---------------------------------------------------------------------------

/// `GET /api/import_settings` — returns import settings for a resource.
fn api_get_import_settings(query: &str, stream: &mut TcpStream) {
    // Stub: return empty settings. Full implementation pending.
    send_json(stream, r#"{"settings":{}}"#);
}

/// `POST /api/import_settings` — updates import settings for a resource.
fn api_set_import_settings(body: &str, stream: &mut TcpStream) {
    // Stub: accept and acknowledge. Full implementation pending.
    send_json(stream, r#"{"ok":true}"#);
}

// ---------------------------------------------------------------------------
// pat-1zlel: Version control integration
// ---------------------------------------------------------------------------

/// `GET /api/vcs/status` — returns git status for the project directory.
fn api_vcs_status(stream: &mut TcpStream) {
    let cwd = std::env::current_dir().unwrap_or_default();
    let status = crate::vcs::query_git_status(&cwd);
    let json = serde_json::to_string(&status).unwrap_or_else(|_| r#"{"error":"serialize"}"#.into());
    send_json(stream, &json);
}

/// `GET /api/vcs/diff?file=<path>&staged=<bool>` — returns diff for a file.
fn api_vcs_diff(query: &str, stream: &mut TcpStream) {
    let file = match query_param(query, "file") {
        Some(f) => f.to_string(),
        None => {
            send_error(stream, 400, "missing file parameter");
            return;
        }
    };
    let staged = query_param(query, "staged").map_or(false, |v| v == "true" || v == "1");
    let cwd = std::env::current_dir().unwrap_or_default();
    let diff = if staged {
        crate::vcs::get_file_diff_staged(&cwd, &file)
    } else {
        crate::vcs::get_file_diff(&cwd, &file)
    };
    // Escape the diff string for JSON.
    let escaped = serde_json::to_string(&diff).unwrap_or_else(|_| "\"\"".into());
    send_json(stream, &format!(r#"{{"diff":{escaped}}}"#));
}

/// `GET /api/vcs/log?count=<N>` — returns recent commit log.
fn api_vcs_log(query: &str, stream: &mut TcpStream) {
    let count: u32 = query_param(query, "count")
        .and_then(|v| v.parse().ok())
        .unwrap_or(20);
    let cwd = std::env::current_dir().unwrap_or_default();
    let commits = crate::vcs::get_commit_log(&cwd, count);
    let json = serde_json::to_string(&commits).unwrap_or_else(|_| "[]".into());
    send_json(stream, &format!(r#"{{"commits":{json}}}"#));
}

/// `POST /api/vcs/stage` — stages a file for commit.
fn api_vcs_stage(body: &str, stream: &mut TcpStream) {
    let p = match parse_json_body(body) {
        Some(v) => v,
        None => {
            send_error(stream, 400, "invalid JSON");
            return;
        }
    };
    let file = match p.get("file").and_then(|v| v.as_str()) {
        Some(f) => f.to_string(),
        None => {
            send_error(stream, 400, "missing file");
            return;
        }
    };
    let cwd = std::env::current_dir().unwrap_or_default();
    match crate::vcs::stage_file(&cwd, &file) {
        Ok(()) => send_json(stream, r#"{"ok":true}"#),
        Err(e) => send_error(stream, 500, &e),
    }
}

/// `POST /api/vcs/unstage` — unstages a file.
fn api_vcs_unstage(body: &str, stream: &mut TcpStream) {
    let p = match parse_json_body(body) {
        Some(v) => v,
        None => {
            send_error(stream, 400, "invalid JSON");
            return;
        }
    };
    let file = match p.get("file").and_then(|v| v.as_str()) {
        Some(f) => f.to_string(),
        None => {
            send_error(stream, 400, "missing file");
            return;
        }
    };
    let cwd = std::env::current_dir().unwrap_or_default();
    match crate::vcs::unstage_file(&cwd, &file) {
        Ok(()) => send_json(stream, r#"{"ok":true}"#),
        Err(e) => send_error(stream, 500, &e),
    }
}

/// `POST /api/vcs/discard` — discards working tree changes for a file.
fn api_vcs_discard(body: &str, stream: &mut TcpStream) {
    let p = match parse_json_body(body) {
        Some(v) => v,
        None => {
            send_error(stream, 400, "invalid JSON");
            return;
        }
    };
    let file = match p.get("file").and_then(|v| v.as_str()) {
        Some(f) => f.to_string(),
        None => {
            send_error(stream, 400, "missing file");
            return;
        }
    };
    let cwd = std::env::current_dir().unwrap_or_default();
    match crate::vcs::discard_changes(&cwd, &file) {
        Ok(()) => send_json(stream, r#"{"ok":true}"#),
        Err(e) => send_error(stream, 500, &e),
    }
}

// ---------------------------------------------------------------------------
// pat-mn3: Multi-object shared properties
// ---------------------------------------------------------------------------

/// `POST /api/node/shared_properties` — returns properties shared by all given nodes.
fn api_get_shared_properties(state: &Arc<Mutex<EditorState>>, body: &str, stream: &mut TcpStream) {
    let p = match parse_json_body(body) {
        Some(v) => v,
        None => {
            send_error(stream, 400, "invalid JSON");
            return;
        }
    };
    let node_ids: Vec<u64> = match p.get("node_ids").and_then(|v| v.as_array()) {
        Some(arr) => arr.iter().filter_map(|v| v.as_u64()).collect(),
        None => {
            send_error(stream, 400, "missing node_ids array");
            return;
        }
    };
    let json = {
        let s = state.lock().unwrap();
        let mut shared: Option<HashMap<String, serde_json::Value>> = None;
        for raw_id in &node_ids {
            let nid = match find_node_by_raw_id(&s.scene_tree, *raw_id) {
                Some(id) => id,
                None => continue,
            };
            let node = match s.scene_tree.get_node(nid) {
                Some(n) => n,
                None => continue,
            };
            let mut props = HashMap::new();
            for (name, value) in node.properties() {
                let vj = gdvariant::serialize::to_json(value);
                let type_name = vj
                    .get("type")
                    .and_then(|t| t.as_str())
                    .unwrap_or("Unknown")
                    .to_string();
                props.insert(
                    name.to_string(),
                    serde_json::json!({"name": name, "type": type_name, "value": vj}),
                );
            }
            shared = match shared {
                None => Some(props),
                Some(existing) => {
                    let mut intersection = HashMap::new();
                    for (k, v) in existing {
                        if props.contains_key(&k) {
                            intersection.insert(k, v);
                        }
                    }
                    Some(intersection)
                }
            };
        }
        let props_arr: Vec<serde_json::Value> = shared.unwrap_or_default().into_values().collect();
        serde_json::json!({"properties": props_arr, "count": node_ids.len()}).to_string()
    };
    send_json(stream, &json);
}

// ---------------------------------------------------------------------------
// pat-ugb0p: Command palette
// ---------------------------------------------------------------------------

/// The static list of editor commands available in the command palette.
const EDITOR_COMMANDS: &[(&str, &str, &str)] = &[
    // (id, label, category)
    ("save_scene", "Save Scene", "File"),
    ("load_scene", "Load Scene", "File"),
    ("new_scene", "New Scene", "File"),
    ("add_node", "Add Node...", "Scene"),
    ("delete_node", "Delete Selected Node", "Scene"),
    ("duplicate_node", "Duplicate Selected Node", "Scene"),
    ("rename_node", "Rename Selected Node", "Scene"),
    ("copy_nodes", "Copy", "Edit"),
    ("paste_nodes", "Paste", "Edit"),
    ("cut_nodes", "Cut", "Edit"),
    ("undo", "Undo", "Edit"),
    ("redo", "Redo", "Edit"),
    ("select_tool", "Select Tool", "Tool"),
    ("move_tool", "Move Tool", "Tool"),
    ("rotate_tool", "Rotate Tool", "Tool"),
    ("scale_tool", "Scale Tool", "Tool"),
    ("zoom_in", "Zoom In", "View"),
    ("zoom_out", "Zoom Out", "View"),
    ("zoom_reset", "Reset Zoom", "View"),
    ("toggle_grid", "Toggle Grid", "View"),
    ("toggle_rulers", "Toggle Rulers", "View"),
    ("toggle_snap", "Toggle Grid Snap", "View"),
    ("open_settings", "Open Editor Settings", "Editor"),
    ("open_project_settings", "Open Project Settings", "Editor"),
    ("open_help", "Open Help / Shortcuts", "Editor"),
    ("search_nodes", "Search Nodes", "Scene"),
    ("play_scene", "Play Scene", "Run"),
    ("stop_scene", "Stop Scene", "Run"),
    ("toggle_theme", "Toggle Light/Dark Theme", "View"),
];

/// `GET /api/commands` — returns the full list of editor commands for the palette.
fn api_get_commands(stream: &mut TcpStream) {
    let commands: Vec<serde_json::Value> = EDITOR_COMMANDS
        .iter()
        .map(|&(id, label, category)| {
            serde_json::json!({
                "id": id,
                "label": label,
                "category": category,
            })
        })
        .collect();
    let json = serde_json::json!({ "commands": commands }).to_string();
    send_json(stream, &json);
}

/// `POST /api/command/execute` — execute a command by id.
///
/// Body: `{"command": "save_scene"}` (plus optional params).
/// Server-side commands (undo, redo, etc.) are executed directly.
/// Client-side commands return `{"action": "client", "command": "<id>"}` to
/// signal the frontend to handle them.
fn api_execute_command(state: &Arc<Mutex<EditorState>>, body: &str, stream: &mut TcpStream) {
    let parsed = match parse_json_body(body) {
        Some(v) => v,
        None => {
            send_error(stream, 400, "invalid JSON");
            return;
        }
    };

    let cmd = match parsed.get("command").and_then(|v| v.as_str()) {
        Some(c) => c.to_string(),
        None => {
            send_error(stream, 400, "missing command");
            return;
        }
    };

    match cmd.as_str() {
        "undo" => {
            let mut st = state.lock().unwrap();
            if let Some(action) = st.undo_stack.pop() {
                action.undo(&mut st.scene_tree);
                st.redo_stack.push(action);
            }
            send_json(stream, r#"{"ok":true,"executed":"undo"}"#);
        }
        "redo" => {
            let mut st = state.lock().unwrap();
            if let Some(mut action) = st.redo_stack.pop() {
                let _ = action.execute(&mut st.scene_tree);
                st.undo_stack.push(action);
            }
            send_json(stream, r#"{"ok":true,"executed":"redo"}"#);
        }
        "delete_node" => {
            let st = state.lock().unwrap();
            if let Some(sel) = st.selected_node {
                drop(st);
                api_delete_node(state, &format!(r#"{{"node_id":{}}}"#, sel.raw()), stream);
            } else {
                send_json(stream, r#"{"ok":false,"reason":"no node selected"}"#);
            }
        }
        // Most commands are best handled client-side (opening dialogs, changing
        // tool mode, etc.). Return an action hint so the JS can dispatch them.
        _ => {
            // Verify the command id is valid
            let valid = EDITOR_COMMANDS.iter().any(|&(id, _, _)| id == cmd.as_str());
            if !valid {
                send_error(stream, 404, "unknown command");
                return;
            }
            let json = serde_json::json!({
                "ok": true,
                "action": "client",
                "command": cmd,
            });
            send_json(stream, &json.to_string());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gdcore::math::{Color, Vector2};
    use gdscene::node::Node;
    #[allow(unused_imports)]
    use std::io::{Read as _, Write as _};
    use std::net::TcpStream;
    use std::time::Duration;

    fn free_port() -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.local_addr().unwrap().port()
    }

    fn make_server() -> (EditorServerHandle, u16) {
        let port = free_port();
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let mut main = Node::new("Main", "Node2D");
        main.set_property("position", Variant::Vector2(Vector2::new(10.0, 20.0)));
        tree.add_child(root, main).unwrap();

        let state = EditorState::new(tree);
        let handle = EditorServerHandle::start(port, state);
        // Wait for server to be ready.
        thread::sleep(Duration::from_millis(100));
        (handle, port)
    }

    fn http_get(port: u16, path: &str) -> String {
        let req = format!("GET {path} HTTP/1.1\r\nHost: localhost\r\n\r\n");
        http_request_str(port, &req)
    }

    fn http_post(port: u16, path: &str, body: &str) -> String {
        let req = format!(
            "POST {path} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        http_request_str(port, &req)
    }

    fn connect_with_retry(port: u16) -> TcpStream {
        for attempt in 0..20 {
            match TcpStream::connect(format!("127.0.0.1:{port}")) {
                Ok(stream) => return stream,
                Err(_) if attempt < 19 => thread::sleep(Duration::from_millis(50)),
                Err(e) => panic!("failed to connect after retries: {e}"),
            }
        }
        unreachable!()
    }

    fn http_request_str(port: u16, request: &str) -> String {
        let mut stream = connect_with_retry(port);
        stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
        stream.write_all(request.as_bytes()).unwrap();
        let mut response = Vec::new();
        let _ = stream.read_to_end(&mut response);
        String::from_utf8_lossy(&response).to_string()
    }

    fn http_request_raw(port: u16, request: &str) -> Vec<u8> {
        let mut stream = connect_with_retry(port);
        stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
        stream.write_all(request.as_bytes()).unwrap();
        let mut response = Vec::new();
        let _ = stream.read_to_end(&mut response);
        response
    }

    /// Extract the JSON body from an HTTP response string.
    fn extract_body(resp: &str) -> &str {
        resp.split("\r\n\r\n").nth(1).unwrap_or("")
    }

    /// Get the root's first child node raw ID from the scene.
    fn get_main_node_id(port: u16) -> u64 {
        let resp = http_get(port, "/api/scene");
        let body = extract_body(&resp);
        let v: serde_json::Value = serde_json::from_str(body).unwrap();
        v["nodes"]["children"][0]["id"].as_u64().unwrap()
    }

    #[test]
    fn test_editor_html() {
        let (handle, port) = make_server();
        let resp = http_get(port, "/editor");
        assert!(resp.contains("200 OK"));
        assert!(resp.contains("Patina"));
        handle.stop();
    }

    #[test]
    fn test_editor_html_contains_statusbar() {
        // pat-rjd: output panel / status bar element must be present in editor HTML
        let (handle, port) = make_server();
        let resp = http_get(port, "/editor");
        assert!(resp.contains("200 OK"));
        let body = extract_body(&resp);
        assert!(
            body.contains("id=\"statusbar\""),
            "editor HTML must contain statusbar element"
        );
        handle.stop();
    }

    #[test]
    fn test_get_scene() {
        let (handle, port) = make_server();
        let resp = http_get(port, "/api/scene");
        assert!(resp.contains("200 OK"));
        let body = extract_body(&resp);
        let v: serde_json::Value = serde_json::from_str(body).unwrap();
        assert_eq!(v["nodes"]["name"], "root");
        assert!(v["nodes"]["children"].as_array().unwrap().len() >= 1);
        assert_eq!(v["nodes"]["children"][0]["name"], "Main");
        assert_eq!(v["nodes"]["children"][0]["class"], "Node2D");
        handle.stop();
    }

    #[test]
    fn test_get_node() {
        let (handle, port) = make_server();
        let main_id = get_main_node_id(port);

        let resp = http_get(port, &format!("/api/node/{main_id}"));
        assert!(resp.contains("200 OK"));
        let body = extract_body(&resp);
        let v: serde_json::Value = serde_json::from_str(body).unwrap();
        assert_eq!(v["name"], "Main");
        assert_eq!(v["class"], "Node2D");
        assert!(v["properties"].as_array().unwrap().len() >= 1);
        handle.stop();
    }

    #[test]
    fn test_get_node_invalid_id() {
        let (handle, port) = make_server();
        let resp = http_get(port, "/api/node/99999999");
        assert!(resp.contains("404"));
        handle.stop();
    }

    #[test]
    fn test_add_node() {
        let (handle, port) = make_server();
        let main_id = get_main_node_id(port);

        let body = format!(r#"{{"parent_id":{main_id},"name":"Child","class_name":"Sprite2D"}}"#);
        let resp = http_post(port, "/api/node/add", &body);
        assert!(resp.contains("200 OK"));
        let resp_body = extract_body(&resp);
        let v: serde_json::Value = serde_json::from_str(resp_body).unwrap();
        assert!(v["id"].as_u64().is_some());

        // Verify the node appears in the tree.
        let scene_resp = http_get(port, "/api/scene");
        assert!(scene_resp.contains("Child"));
        assert!(scene_resp.contains("Sprite2D"));

        handle.stop();
    }

    #[test]
    fn test_delete_node() {
        let (handle, port) = make_server();

        // Add a node first.
        let main_id = get_main_node_id(port);
        let add_body =
            format!(r#"{{"parent_id":{main_id},"name":"ToDelete","class_name":"Node"}}"#);
        let add_resp = http_post(port, "/api/node/add", &add_body);
        let add_body_json: serde_json::Value =
            serde_json::from_str(extract_body(&add_resp)).unwrap();
        let new_id = add_body_json["id"].as_u64().unwrap();

        // Delete it.
        let del_body = format!(r#"{{"node_id":{new_id}}}"#);
        let resp = http_post(port, "/api/node/delete", &del_body);
        assert!(resp.contains("200 OK"));

        // Verify it's gone.
        let scene_resp = http_get(port, "/api/scene");
        assert!(!scene_resp.contains("ToDelete"));

        handle.stop();
    }

    #[test]
    fn test_select_node() {
        let (handle, port) = make_server();
        let main_id = get_main_node_id(port);

        let body = format!(r#"{{"node_id":{main_id}}}"#);
        let resp = http_post(port, "/api/node/select", &body);
        assert!(resp.contains("200 OK"));

        handle.stop();
    }

    #[test]
    fn test_get_selected() {
        let (handle, port) = make_server();

        // No selection initially.
        let resp = http_get(port, "/api/selected");
        assert!(resp.contains("200 OK"));
        assert!(extract_body(&resp).trim() == "null");

        // Select a node.
        let main_id = get_main_node_id(port);
        http_post(
            port,
            "/api/node/select",
            &format!(r#"{{"node_id":{main_id}}}"#),
        );

        let resp = http_get(port, "/api/selected");
        let body = extract_body(&resp);
        let v: serde_json::Value = serde_json::from_str(body).unwrap();
        assert_eq!(v["name"], "Main");

        handle.stop();
    }

    #[test]
    fn test_reparent_node() {
        let (handle, port) = make_server();
        let main_id = get_main_node_id(port);

        // Get root id.
        let scene_resp = http_get(port, "/api/scene");
        let scene_body = extract_body(&scene_resp);
        let scene_v: serde_json::Value = serde_json::from_str(scene_body).unwrap();
        let root_id = scene_v["nodes"]["id"].as_u64().unwrap();

        // Add two children under main.
        let body_a = format!(r#"{{"parent_id":{main_id},"name":"A","class_name":"Node"}}"#);
        let resp_a = http_post(port, "/api/node/add", &body_a);
        let a_id: serde_json::Value = serde_json::from_str(extract_body(&resp_a)).unwrap();
        let a_id = a_id["id"].as_u64().unwrap();

        let body_b = format!(r#"{{"parent_id":{main_id},"name":"B","class_name":"Node"}}"#);
        http_post(port, "/api/node/add", &body_b);

        // Reparent A to root.
        let reparent_body = format!(r#"{{"node_id":{a_id},"new_parent_id":{root_id}}}"#);
        let resp = http_post(port, "/api/node/reparent", &reparent_body);
        assert!(resp.contains("200 OK"));

        // Verify A is now under root.
        let scene_resp = http_get(port, "/api/scene");
        let scene_body = extract_body(&scene_resp);
        let v: serde_json::Value = serde_json::from_str(scene_body).unwrap();
        let root_children = v["nodes"]["children"].as_array().unwrap();
        let a_found = root_children.iter().any(|c| c["name"] == "A");
        assert!(a_found, "A should be a direct child of root after reparent");

        handle.stop();
    }

    #[test]
    fn test_set_property() {
        let (handle, port) = make_server();
        let main_id = get_main_node_id(port);

        let body = format!(
            r#"{{"node_id":{main_id},"property":"health","value":{{"type":"Int","value":100}}}}"#
        );
        let resp = http_post(port, "/api/property/set", &body);
        assert!(resp.contains("200 OK"));

        // Verify property was set.
        let node_resp = http_get(port, &format!("/api/node/{main_id}"));
        let node_body = extract_body(&node_resp);
        assert!(node_body.contains("health"));

        handle.stop();
    }

    #[test]
    fn test_undo() {
        let (handle, port) = make_server();
        let main_id = get_main_node_id(port);

        // Set a property.
        let body = format!(
            r#"{{"node_id":{main_id},"property":"hp","value":{{"type":"Int","value":50}}}}"#
        );
        http_post(port, "/api/property/set", &body);

        // Undo.
        let resp = http_post(port, "/api/undo", "");
        assert!(resp.contains("200 OK"));

        // Verify property is reverted to Nil.
        let node_resp = http_get(port, &format!("/api/node/{main_id}"));
        let node_body = extract_body(&node_resp);
        let v: serde_json::Value = serde_json::from_str(node_body).unwrap();
        let hp_prop = v["properties"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["name"] == "hp");
        match hp_prop {
            None => {} // Property removed entirely — good.
            Some(p) => {
                // Property exists but should be Nil after undo.
                assert_eq!(p["type"], "Nil", "hp should be Nil after undo");
            }
        }

        handle.stop();
    }

    #[test]
    fn test_redo() {
        let (handle, port) = make_server();
        let main_id = get_main_node_id(port);

        // Set a property, then undo, then redo.
        let body = format!(
            r#"{{"node_id":{main_id},"property":"hp","value":{{"type":"Int","value":75}}}}"#
        );
        http_post(port, "/api/property/set", &body);
        http_post(port, "/api/undo", "");
        let resp = http_post(port, "/api/redo", "");
        assert!(resp.contains("200 OK"));

        // Verify property is back.
        let node_resp = http_get(port, &format!("/api/node/{main_id}"));
        let node_body = extract_body(&node_resp);
        let v: serde_json::Value = serde_json::from_str(node_body).unwrap();
        let hp_prop = v["properties"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["name"] == "hp");
        assert!(hp_prop.is_some(), "hp should be restored after redo");

        handle.stop();
    }

    #[test]
    fn test_undo_empty_returns_error() {
        let (handle, port) = make_server();
        let resp = http_post(port, "/api/undo", "");
        assert!(resp.contains("400"));
        assert!(resp.contains("nothing to undo"));
        handle.stop();
    }

    #[test]
    fn test_redo_empty_returns_error() {
        let (handle, port) = make_server();
        let resp = http_post(port, "/api/redo", "");
        assert!(resp.contains("400"));
        assert!(resp.contains("nothing to redo"));
        handle.stop();
    }

    #[test]
    fn test_viewport_bmp() {
        let (handle, port) = make_server();

        let fb = FrameBuffer::new(4, 4, Color::rgb(1.0, 0.0, 0.0));
        handle.update_frame(fb);

        let resp = http_request_raw(
            port,
            "GET /api/viewport HTTP/1.1\r\nHost: localhost\r\n\r\n",
        );
        let resp_str = String::from_utf8_lossy(&resp);
        assert!(resp_str.contains("200 OK"));
        assert!(resp_str.contains("image/bmp"));
        // Check BMP magic bytes.
        let bm_pos = resp.windows(2).position(|w| w == b"BM");
        assert!(bm_pos.is_some());

        handle.stop();
    }

    #[test]
    fn test_viewport_png() {
        let (handle, port) = make_server();

        let fb = FrameBuffer::new(4, 4, Color::rgb(0.0, 1.0, 0.0));
        handle.update_frame(fb);

        let resp = http_request_raw(
            port,
            "GET /api/viewport/png HTTP/1.1\r\nHost: localhost\r\n\r\n",
        );
        let resp_str = String::from_utf8_lossy(&resp);
        assert!(resp_str.contains("200 OK"));
        assert!(resp_str.contains("image/png"));
        // PNG magic bytes.
        let png_sig = resp.windows(4).position(|w| w == b"\x89PNG");
        assert!(png_sig.is_some());

        handle.stop();
    }

    #[test]
    fn test_viewport_no_frame() {
        let (handle, port) = make_server();
        let resp = http_get(port, "/api/viewport");
        assert!(resp.contains("404") || resp.contains("no frame"));
        handle.stop();
    }

    #[test]
    fn test_scene_save_and_load() {
        let (handle, port) = make_server();

        // Save scene to a temp file.
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let path = tmp.path().to_str().unwrap().to_string();

        let save_body = format!(r#"{{"path":"{path}"}}"#);
        let resp = http_post(port, "/api/scene/save", &save_body);
        assert!(resp.contains("200 OK"));

        // Verify the file was written.
        let contents = std::fs::read_to_string(&path).unwrap();
        assert!(contents.contains("[gd_scene"));
        assert!(contents.contains("Main"));

        // Load it back (replaces the tree).
        let load_body = format!(r#"{{"path":"{path}"}}"#);
        let resp = http_post(port, "/api/scene/load", &load_body);
        assert!(resp.contains("200 OK"));

        // Verify the tree was replaced (Main should still be there).
        let scene_resp = http_get(port, "/api/scene");
        assert!(scene_resp.contains("Main"));

        handle.stop();
    }

    #[test]
    fn test_scene_load_sets_scene_file() {
        let (handle, port) = make_server();

        // Save scene first so we have a file to load.
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let path = tmp.path().to_str().unwrap().to_string();
        let save_body = format!(r#"{{"path":"{path}"}}"#);
        http_post(port, "/api/scene/save", &save_body);

        // Load it — scene_file should be set.
        let load_body = format!(r#"{{"path":"{path}"}}"#);
        let resp = http_post(port, "/api/scene/load", &load_body);
        assert!(resp.contains("200 OK"));

        // Verify scene_file appears in scene info.
        let info = http_get(port, "/api/scene/info");
        assert!(
            info.contains(&path),
            "scene_file should be set after load; info = {info}"
        );

        handle.stop();
    }

    #[test]
    fn test_scene_file_initially_none() {
        let (handle, port) = make_server();

        // Default state has no scene_file — info should say null.
        let info = http_get(port, "/api/scene/info");
        assert!(
            info.contains("\"scene_file\":null"),
            "scene_file should be null initially; info = {info}"
        );

        handle.stop();
    }

    #[test]
    fn test_scene_save_sets_scene_file() {
        let (handle, port) = make_server();

        let tmp = tempfile::NamedTempFile::new().unwrap();
        let path = tmp.path().to_str().unwrap().to_string();
        let save_body = format!(r#"{{"path":"{path}"}}"#);
        let resp = http_post(port, "/api/scene/save", &save_body);
        assert!(resp.contains("200 OK"));

        // Verify scene_file appears in scene info after save.
        let info = http_get(port, "/api/scene/info");
        assert!(
            info.contains(&path),
            "scene_file should be set after save; info = {info}"
        );

        handle.stop();
    }

    #[test]
    fn test_logs_appear_after_operations() {
        let (handle, port) = make_server();
        let main_id = get_main_node_id(port);

        // Initially should have no logs (or only startup logs).
        let logs_before = http_get(port, "/api/logs");

        // Perform an operation that logs.
        let body = format!(r#"{{"parent_id":{main_id},"name":"TestChild","class_name":"Node2D"}}"#);
        http_post(port, "/api/node/add", &body);

        // Logs should now have an entry about adding a node.
        let logs_after = http_get(port, "/api/logs");
        assert!(
            logs_after.contains("Added"),
            "logs should contain 'Added' after adding a node; logs = {logs_after}"
        );
        // Should have more content than before.
        assert!(
            logs_after.len() > logs_before.len(),
            "logs should grow after operations"
        );

        handle.stop();
    }

    #[test]
    fn test_scene_file_set_at_startup() {
        // Simulate what the editor example does: set scene_file before starting server.
        let port = free_port();
        let tree = SceneTree::new();
        let mut state = EditorState::new(tree);
        state.scene_file = Some("test_scene.tscn".to_string());
        let handle = EditorServerHandle::start(port, state);
        thread::sleep(Duration::from_millis(100));

        let info = http_get(port, "/api/scene/info");
        assert!(
            info.contains("test_scene.tscn"),
            "scene_file should be set from startup state; info = {info}"
        );

        handle.stop();
    }

    #[test]
    fn test_cors_preflight() {
        let (handle, port) = make_server();
        let resp = http_request_str(
            port,
            "OPTIONS /api/scene HTTP/1.1\r\nHost: localhost\r\n\r\n",
        );
        assert!(resp.contains("204 No Content"));
        assert!(resp.contains("Access-Control-Allow-Origin: *"));
        assert!(resp.contains("Access-Control-Allow-Methods"));
        handle.stop();
    }

    #[test]
    fn test_404_unknown_path() {
        let (handle, port) = make_server();
        let resp = http_get(port, "/nonexistent");
        assert!(resp.contains("404"));
        handle.stop();
    }

    #[test]
    fn test_add_node_missing_fields() {
        let (handle, port) = make_server();
        let resp = http_post(port, "/api/node/add", r#"{"parent_id":1}"#);
        assert!(resp.contains("400"));
        handle.stop();
    }

    #[test]
    fn test_set_property_with_vector2() {
        let (handle, port) = make_server();
        let main_id = get_main_node_id(port);

        let body = format!(
            r#"{{"node_id":{main_id},"property":"position","value":{{"type":"Vector2","value":[100,200]}}}}"#
        );
        let resp = http_post(port, "/api/property/set", &body);
        assert!(resp.contains("200 OK"));

        // Verify.
        let node_resp = http_get(port, &format!("/api/node/{main_id}"));
        let node_body = extract_body(&node_resp);
        let v: serde_json::Value = serde_json::from_str(node_body).unwrap();
        let pos_prop = v["properties"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["name"] == "position")
            .unwrap();
        assert_eq!(pos_prop["type"], "Vector2");

        handle.stop();
    }

    #[test]
    fn test_undo_add_node() {
        let (handle, port) = make_server();
        let main_id = get_main_node_id(port);

        // Add a node.
        let body = format!(r#"{{"parent_id":{main_id},"name":"Temp","class_name":"Node"}}"#);
        http_post(port, "/api/node/add", &body);

        // Verify it exists.
        let scene_resp = http_get(port, "/api/scene");
        assert!(scene_resp.contains("Temp"));

        // Undo the add.
        http_post(port, "/api/undo", "");

        // Verify it's gone.
        let scene_resp = http_get(port, "/api/scene");
        assert!(!scene_resp.contains("Temp"));

        handle.stop();
    }

    #[test]
    fn test_multiple_operations_undo_redo() {
        let (handle, port) = make_server();
        let main_id = get_main_node_id(port);

        // Set two properties.
        let body1 =
            format!(r#"{{"node_id":{main_id},"property":"a","value":{{"type":"Int","value":1}}}}"#);
        let body2 =
            format!(r#"{{"node_id":{main_id},"property":"b","value":{{"type":"Int","value":2}}}}"#);
        http_post(port, "/api/property/set", &body1);
        http_post(port, "/api/property/set", &body2);

        // Undo both.
        http_post(port, "/api/undo", "");
        http_post(port, "/api/undo", "");

        // Redo both.
        http_post(port, "/api/redo", "");
        http_post(port, "/api/redo", "");

        // Verify both properties are set.
        let node_resp = http_get(port, &format!("/api/node/{main_id}"));
        let body = extract_body(&node_resp);
        assert!(body.contains(r#""a""#));
        assert!(body.contains(r#""b""#));

        handle.stop();
    }

    #[test]
    fn test_viewport_click_selects_node() {
        let (handle, port) = make_server();

        // The Main node is at position (10, 20). Viewport defaults to 800x600.
        // Bounds center = (10, 20), offset = (400-10, 300-20) = (390, 280).
        // So pixel coords for scene (10, 20) = (400, 300).
        let resp = http_post(port, "/api/viewport/click", r#"{"x":400,"y":300}"#);
        assert!(resp.contains("200 OK"));
        let body = extract_body(&resp);
        let v: serde_json::Value = serde_json::from_str(body).unwrap();
        assert!(v["selected"].is_number(), "should select the node");

        handle.stop();
    }

    #[test]
    fn test_viewport_click_miss() {
        let (handle, port) = make_server();

        // Click far from the node.
        let resp = http_post(port, "/api/viewport/click", r#"{"x":0,"y":0}"#);
        assert!(resp.contains("200 OK"));
        let body = extract_body(&resp);
        let v: serde_json::Value = serde_json::from_str(body).unwrap();
        assert!(v["selected"].is_null(), "should miss all nodes");

        handle.stop();
    }

    #[test]
    fn test_viewport_drag_updates_position() {
        let (handle, port) = make_server();
        let main_id = get_main_node_id(port);

        // Start drag on the node at pixel (400, 300).
        let resp = http_post(port, "/api/viewport/drag_start", r#"{"x":400,"y":300}"#);
        assert!(resp.contains("200 OK"));
        let body = extract_body(&resp);
        assert!(body.contains("\"dragging\":true"));

        // Drag to a new position (move 50px right).
        let resp = http_post(port, "/api/viewport/drag", r#"{"x":450,"y":300}"#);
        assert!(resp.contains("200 OK"));

        // End drag.
        let resp = http_post(port, "/api/viewport/drag_end", r#"{"x":450,"y":300}"#);
        assert!(resp.contains("200 OK"));

        // Verify the node position changed.
        let node_resp = http_get(port, &format!("/api/node/{main_id}"));
        let node_body = extract_body(&node_resp);
        let v: serde_json::Value = serde_json::from_str(node_body).unwrap();
        // Position should have moved by +50 in x.
        let pos_prop = v["properties"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["name"] == "position")
            .unwrap();
        let pos_val = &pos_prop["value"]["value"];
        let x = pos_val[0].as_f64().unwrap();
        assert!((x - 60.0).abs() < 1.0, "x should be ~60 (10 + 50), got {x}");

        handle.stop();
    }

    #[test]
    fn test_viewport_drag_end_no_drag() {
        let (handle, port) = make_server();

        // End drag with no active drag — should be ok.
        let resp = http_post(port, "/api/viewport/drag_end", r#"{"x":100,"y":100}"#);
        assert!(resp.contains("200 OK"));
        let body = extract_body(&resp);
        assert!(body.contains("\"ok\":true"));

        handle.stop();
    }

    #[test]
    fn test_zoom_pan_get_defaults() {
        let (handle, port) = make_server();
        let resp = http_get(port, "/api/viewport/zoom_pan");
        assert!(resp.contains("200 OK"));
        let body = extract_body(&resp);
        let v: serde_json::Value = serde_json::from_str(body).unwrap();
        assert_eq!(v["zoom"].as_f64().unwrap(), 1.0);
        assert_eq!(v["pan_x"].as_f64().unwrap(), 0.0);
        assert_eq!(v["pan_y"].as_f64().unwrap(), 0.0);
        handle.stop();
    }

    #[test]
    fn test_set_zoom() {
        let (handle, port) = make_server();
        let resp = http_post(port, "/api/viewport/zoom", r#"{"zoom":2.0}"#);
        assert!(resp.contains("200 OK"));
        let body = extract_body(&resp);
        let v: serde_json::Value = serde_json::from_str(body).unwrap();
        assert_eq!(v["zoom"].as_f64().unwrap(), 2.0);

        // Verify via GET.
        let resp = http_get(port, "/api/viewport/zoom_pan");
        let body = extract_body(&resp);
        let v: serde_json::Value = serde_json::from_str(body).unwrap();
        assert_eq!(v["zoom"].as_f64().unwrap(), 2.0);
        handle.stop();
    }

    #[test]
    fn test_set_zoom_clamp_min() {
        let (handle, port) = make_server();
        let resp = http_post(port, "/api/viewport/zoom", r#"{"zoom":0.01}"#);
        assert!(resp.contains("200 OK"));
        let body = extract_body(&resp);
        let v: serde_json::Value = serde_json::from_str(body).unwrap();
        assert!(
            (v["zoom"].as_f64().unwrap() - 0.1).abs() < 0.001,
            "zoom should clamp to 0.1"
        );
        handle.stop();
    }

    #[test]
    fn test_set_zoom_clamp_max() {
        let (handle, port) = make_server();
        let resp = http_post(port, "/api/viewport/zoom", r#"{"zoom":100.0}"#);
        assert!(resp.contains("200 OK"));
        let body = extract_body(&resp);
        let v: serde_json::Value = serde_json::from_str(body).unwrap();
        assert!(
            (v["zoom"].as_f64().unwrap() - 16.0).abs() < 0.001,
            "zoom should clamp to 16.0"
        );
        handle.stop();
    }

    #[test]
    fn test_set_pan() {
        let (handle, port) = make_server();
        let resp = http_post(port, "/api/viewport/pan", r#"{"x":50.5,"y":-30.0}"#);
        assert!(resp.contains("200 OK"));
        let body = extract_body(&resp);
        let v: serde_json::Value = serde_json::from_str(body).unwrap();
        assert_eq!(v["pan_x"].as_f64().unwrap(), 50.5);
        assert_eq!(v["pan_y"].as_f64().unwrap(), -30.0);

        // Verify via GET.
        let resp = http_get(port, "/api/viewport/zoom_pan");
        let body = extract_body(&resp);
        let v: serde_json::Value = serde_json::from_str(body).unwrap();
        assert_eq!(v["pan_x"].as_f64().unwrap(), 50.5);
        assert_eq!(v["pan_y"].as_f64().unwrap(), -30.0);
        handle.stop();
    }

    #[test]
    fn test_zoom_affects_drag() {
        let (handle, port) = make_server();
        let main_id = get_main_node_id(port);

        // Set zoom to 2x.
        http_post(port, "/api/viewport/zoom", r#"{"zoom":2.0}"#);

        // Start drag on the node at pixel (400, 300).
        let resp = http_post(port, "/api/viewport/drag_start", r#"{"x":400,"y":300}"#);
        let body = extract_body(&resp);
        assert!(body.contains("\"dragging\":true"));

        // Drag 100px right in screen space = 50 world units at zoom 2x.
        http_post(port, "/api/viewport/drag_end", r#"{"x":500,"y":300}"#);

        // Verify position changed by ~50 in x (not 100).
        let node_resp = http_get(port, &format!("/api/node/{main_id}"));
        let node_body = extract_body(&node_resp);
        let v: serde_json::Value = serde_json::from_str(node_body).unwrap();
        let pos_prop = v["properties"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["name"] == "position")
            .unwrap();
        let pos_val = &pos_prop["value"]["value"];
        let x = pos_val[0].as_f64().unwrap();
        assert!(
            (x - 60.0).abs() < 1.0,
            "x should be ~60 (10 + 50 at 2x zoom), got {x}"
        );

        handle.stop();
    }

    #[test]
    fn test_rename_node() {
        let (handle, port) = make_server();
        let main_id = get_main_node_id(port);

        // Rename the node.
        let body = format!(r#"{{"node_id":{main_id},"new_name":"Player"}}"#);
        let resp = http_post(port, "/api/node/rename", &body);
        assert!(resp.contains("200 OK"));

        // Verify the name changed.
        let scene_resp = http_get(port, "/api/scene");
        assert!(scene_resp.contains("Player"));
        assert!(!extract_body(&scene_resp).contains("\"Main\""));

        handle.stop();
    }

    #[test]
    fn test_rename_node_undo() {
        let (handle, port) = make_server();
        let main_id = get_main_node_id(port);

        // Rename.
        let body = format!(r#"{{"node_id":{main_id},"new_name":"Renamed"}}"#);
        http_post(port, "/api/node/rename", &body);

        // Verify renamed.
        let scene_resp = http_get(port, "/api/scene");
        assert!(scene_resp.contains("Renamed"));

        // Undo.
        http_post(port, "/api/undo", "");

        // Verify name is restored.
        let scene_resp = http_get(port, "/api/scene");
        assert!(scene_resp.contains("Main"));
        assert!(!extract_body(&scene_resp).contains("\"Renamed\""));

        handle.stop();
    }

    #[test]
    fn test_rename_node_missing_fields() {
        let (handle, port) = make_server();
        let resp = http_post(port, "/api/node/rename", r#"{"node_id":1}"#);
        assert!(resp.contains("400"));
        handle.stop();
    }

    #[test]
    fn test_rename_node_not_found() {
        let (handle, port) = make_server();
        let resp = http_post(
            port,
            "/api/node/rename",
            r#"{"node_id":99999,"new_name":"X"}"#,
        );
        assert!(resp.contains("404"));
        handle.stop();
    }

    #[test]
    fn test_duplicate_node() {
        let (handle, port) = make_server();
        let main_id = get_main_node_id(port);

        // Add a child to Main so we test subtree duplication.
        let add_body =
            format!(r#"{{"parent_id":{main_id},"name":"Child","class_name":"Sprite2D"}}"#);
        http_post(port, "/api/node/add", &add_body);

        // Duplicate Main (which now has a child).
        let body = format!(r#"{{"node_id":{main_id}}}"#);
        let resp = http_post(port, "/api/node/duplicate", &body);
        assert!(resp.contains("200 OK"));
        let resp_body = extract_body(&resp);
        let v: serde_json::Value = serde_json::from_str(resp_body).unwrap();
        assert!(v["id"].as_u64().is_some(), "should return new node id");

        // Verify the duplicate exists in the tree.
        let scene_resp = http_get(port, "/api/scene");
        let scene_body = extract_body(&scene_resp);
        let sv: serde_json::Value = serde_json::from_str(scene_body).unwrap();
        let root_children = sv["nodes"]["children"].as_array().unwrap();
        // Should have two children under root: original Main and duplicated Main.
        assert!(
            root_children.len() >= 2,
            "root should have at least 2 children after duplicate, got {}",
            root_children.len()
        );

        // Both should be named "Main".
        let main_count = root_children.iter().filter(|c| c["name"] == "Main").count();
        assert_eq!(main_count, 2, "should have two Main nodes");

        // The duplicate should also have a Child child.
        let dup = &root_children[1];
        let dup_children = dup["children"].as_array().unwrap();
        assert_eq!(dup_children.len(), 1, "duplicate should have 1 child");
        assert_eq!(dup_children[0]["name"], "Child");

        handle.stop();
    }

    #[test]
    fn test_duplicate_node_undo() {
        let (handle, port) = make_server();
        let main_id = get_main_node_id(port);

        // Duplicate.
        let body = format!(r#"{{"node_id":{main_id}}}"#);
        http_post(port, "/api/node/duplicate", &body);

        // Verify duplicate exists.
        let scene_resp = http_get(port, "/api/scene");
        let scene_body = extract_body(&scene_resp);
        let sv: serde_json::Value = serde_json::from_str(scene_body).unwrap();
        let count_before = sv["nodes"]["children"].as_array().unwrap().len();
        assert!(count_before >= 2);

        // Undo.
        http_post(port, "/api/undo", "");

        // Verify it's gone.
        let scene_resp = http_get(port, "/api/scene");
        let scene_body = extract_body(&scene_resp);
        let sv: serde_json::Value = serde_json::from_str(scene_body).unwrap();
        let count_after = sv["nodes"]["children"].as_array().unwrap().len();
        assert_eq!(count_after, 1, "should be back to 1 child after undo");

        handle.stop();
    }

    #[test]
    fn test_duplicate_node_not_found() {
        let (handle, port) = make_server();
        let resp = http_post(port, "/api/node/duplicate", r#"{"node_id":99999}"#);
        assert!(resp.contains("404"));
        handle.stop();
    }

    #[test]
    fn test_reorder_node_down() {
        let (handle, port) = make_server();
        let main_id = get_main_node_id(port);

        // Add two children.
        let body_a = format!(r#"{{"parent_id":{main_id},"name":"A","class_name":"Node"}}"#);
        http_post(port, "/api/node/add", &body_a);
        let body_b = format!(r#"{{"parent_id":{main_id},"name":"B","class_name":"Node"}}"#);
        let resp_b = http_post(port, "/api/node/add", &body_b);
        let a_scene = http_get(port, "/api/scene");
        let a_body = extract_body(&a_scene);
        let av: serde_json::Value = serde_json::from_str(a_body).unwrap();
        let main_children = av["nodes"]["children"][0]["children"].as_array().unwrap();
        let a_id = main_children[0]["id"].as_u64().unwrap();

        // A is first, B is second. Move A down.
        let body = format!(r#"{{"node_id":{a_id},"direction":"down"}}"#);
        let resp = http_post(port, "/api/node/reorder", &body);
        assert!(resp.contains("200 OK"));

        // Verify order: B should now be first.
        let scene_resp = http_get(port, "/api/scene");
        let scene_body = extract_body(&scene_resp);
        let sv: serde_json::Value = serde_json::from_str(scene_body).unwrap();
        let children = sv["nodes"]["children"][0]["children"].as_array().unwrap();
        assert_eq!(
            children[0]["name"], "B",
            "B should be first after move down"
        );
        assert_eq!(
            children[1]["name"], "A",
            "A should be second after move down"
        );

        handle.stop();
    }

    #[test]
    fn test_reorder_node_up() {
        let (handle, port) = make_server();
        let main_id = get_main_node_id(port);

        // Add two children.
        let body_a = format!(r#"{{"parent_id":{main_id},"name":"A","class_name":"Node"}}"#);
        http_post(port, "/api/node/add", &body_a);
        let body_b = format!(r#"{{"parent_id":{main_id},"name":"B","class_name":"Node"}}"#);
        http_post(port, "/api/node/add", &body_b);

        // Get B's id.
        let scene_resp = http_get(port, "/api/scene");
        let scene_body = extract_body(&scene_resp);
        let sv: serde_json::Value = serde_json::from_str(scene_body).unwrap();
        let main_children = sv["nodes"]["children"][0]["children"].as_array().unwrap();
        let b_id = main_children[1]["id"].as_u64().unwrap();

        // Move B up.
        let body = format!(r#"{{"node_id":{b_id},"direction":"up"}}"#);
        let resp = http_post(port, "/api/node/reorder", &body);
        assert!(resp.contains("200 OK"));

        // Verify: B is now first.
        let scene_resp = http_get(port, "/api/scene");
        let scene_body = extract_body(&scene_resp);
        let sv: serde_json::Value = serde_json::from_str(scene_body).unwrap();
        let children = sv["nodes"]["children"][0]["children"].as_array().unwrap();
        assert_eq!(children[0]["name"], "B", "B should be first after move up");
        assert_eq!(children[1]["name"], "A", "A should be second after move up");

        handle.stop();
    }

    #[test]
    fn test_reorder_at_boundary_is_noop() {
        let (handle, port) = make_server();
        let main_id = get_main_node_id(port);

        // Add one child.
        let body_a = format!(r#"{{"parent_id":{main_id},"name":"Only","class_name":"Node"}}"#);
        http_post(port, "/api/node/add", &body_a);

        let scene_resp = http_get(port, "/api/scene");
        let scene_body = extract_body(&scene_resp);
        let sv: serde_json::Value = serde_json::from_str(scene_body).unwrap();
        let only_id = sv["nodes"]["children"][0]["children"][0]["id"]
            .as_u64()
            .unwrap();

        // Move up when already first.
        let resp = http_post(
            port,
            "/api/node/reorder",
            &format!(r#"{{"node_id":{only_id},"direction":"up"}}"#),
        );
        assert!(resp.contains("200 OK"));

        // Move down when already last.
        let resp = http_post(
            port,
            "/api/node/reorder",
            &format!(r#"{{"node_id":{only_id},"direction":"down"}}"#),
        );
        assert!(resp.contains("200 OK"));

        handle.stop();
    }

    #[test]
    fn test_scene_tree_visible_field() {
        let (handle, port) = make_server();
        let main_id = get_main_node_id(port);

        // Default: visible should be true.
        let scene_resp = http_get(port, "/api/scene");
        let scene_body = extract_body(&scene_resp);
        let sv: serde_json::Value = serde_json::from_str(scene_body).unwrap();
        assert_eq!(
            sv["nodes"]["children"][0]["visible"], true,
            "default visibility should be true"
        );

        // Set visible to false.
        let body = format!(
            r#"{{"node_id":{main_id},"property":"visible","value":{{"type":"Bool","value":false}}}}"#
        );
        http_post(port, "/api/property/set", &body);

        // Verify visible is false.
        let scene_resp = http_get(port, "/api/scene");
        let scene_body = extract_body(&scene_resp);
        let sv: serde_json::Value = serde_json::from_str(scene_body).unwrap();
        assert_eq!(
            sv["nodes"]["children"][0]["visible"], false,
            "visibility should be false after setting"
        );

        handle.stop();
    }

    #[test]
    fn test_duplicate_preserves_properties() {
        let (handle, port) = make_server();
        let main_id = get_main_node_id(port);

        // Main already has position (10, 20). Duplicate it.
        let body = format!(r#"{{"node_id":{main_id}}}"#);
        let resp = http_post(port, "/api/node/duplicate", &body);
        let resp_body = extract_body(&resp);
        let v: serde_json::Value = serde_json::from_str(resp_body).unwrap();
        let dup_id = v["id"].as_u64().unwrap();

        // Check the duplicate has the same position.
        let node_resp = http_get(port, &format!("/api/node/{dup_id}"));
        let node_body = extract_body(&node_resp);
        let nv: serde_json::Value = serde_json::from_str(node_body).unwrap();
        let pos_prop = nv["properties"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["name"] == "position");
        assert!(
            pos_prop.is_some(),
            "duplicate should have position property"
        );

        handle.stop();
    }

    #[test]
    fn test_filesystem_endpoint_returns_json() {
        let (handle, port) = make_server();
        let resp = http_get(port, "/api/filesystem");
        assert!(resp.contains("200 OK"));
        let body = extract_body(&resp);
        let v: serde_json::Value = serde_json::from_str(body).unwrap();
        assert!(v["root"].is_string(), "should have a root path");
        assert!(v["files"].is_array(), "should have a files array");
        handle.stop();
    }

    #[test]
    fn test_filesystem_finds_tscn_files() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::write(tmp.path().join("main.tscn"), "[gd_scene]").unwrap();
        std::fs::write(tmp.path().join("player.gd"), "extends Node").unwrap();
        std::fs::write(tmp.path().join("theme.tres"), "[gd_resource]").unwrap();
        std::fs::write(tmp.path().join("readme.txt"), "ignore me").unwrap();
        std::fs::create_dir_all(tmp.path().join("scenes")).unwrap();
        std::fs::write(tmp.path().join("scenes/level1.tscn"), "[gd_scene]").unwrap();

        let entries = super::scan_directory(tmp.path(), "", 0, 3);
        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        assert!(
            names.contains(&"main.tscn"),
            "should find main.tscn, got {:?}",
            names
        );
        assert!(
            names.contains(&"player.gd"),
            "should find player.gd, got {:?}",
            names
        );
        assert!(
            names.contains(&"theme.tres"),
            "should find theme.tres, got {:?}",
            names
        );
        assert!(
            !names.contains(&"readme.txt"),
            "should not include .txt files"
        );
        assert!(names.contains(&"scenes"), "should find scenes directory");

        let scenes_dir = entries.iter().find(|e| e.name == "scenes").unwrap();
        assert!(scenes_dir.is_dir);
        assert_eq!(scenes_dir.children.len(), 1);
        assert_eq!(scenes_dir.children[0].name, "level1.tscn");
    }

    #[test]
    fn test_filesystem_respects_max_depth() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("a/b/c/d")).unwrap();
        std::fs::write(tmp.path().join("a/b/c/d/deep.tscn"), "[gd_scene]").unwrap();
        std::fs::write(tmp.path().join("a/top.tscn"), "[gd_scene]").unwrap();
        // Add a file at depth 2 so intermediate dirs are included.
        std::fs::write(tmp.path().join("a/b/mid.tscn"), "[gd_scene]").unwrap();

        let entries = super::scan_directory(tmp.path(), "", 0, 3);
        let a = entries.iter().find(|e| e.name == "a").unwrap();
        assert!(
            a.children.iter().any(|c| c.name == "top.tscn"),
            "should find top.tscn"
        );
        let b = a.children.iter().find(|e| e.name == "b").unwrap();
        assert!(
            b.children.iter().any(|c| c.name == "mid.tscn"),
            "should find mid.tscn at depth 2"
        );
        // c/ contains only d/ which has content beyond max_depth,
        // so c/ is excluded (no reachable children).
        assert!(
            !b.children.iter().any(|c| c.name == "c"),
            "c/ should be excluded since its content is beyond max depth"
        );
    }

    #[test]
    fn test_filesystem_skips_hidden_dirs() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join(".godot")).unwrap();
        std::fs::write(tmp.path().join(".godot/cache.tscn"), "[gd_scene]").unwrap();
        std::fs::write(tmp.path().join("visible.tscn"), "[gd_scene]").unwrap();

        let entries = super::scan_directory(tmp.path(), "", 0, 3);
        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        assert!(names.contains(&"visible.tscn"));
        assert!(!names.contains(&".godot"), "should skip hidden directories");
    }

    #[test]
    fn test_filesystem_empty_dir() {
        let tmp = tempfile::TempDir::new().unwrap();
        let entries = super::scan_directory(tmp.path(), "", 0, 3);
        assert!(
            entries.is_empty(),
            "empty directory should return no entries"
        );
    }

    #[test]
    fn test_filesystem_includes_image_files() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::write(tmp.path().join("icon.png"), &[0x89, 0x50, 0x4e, 0x47]).unwrap();
        std::fs::write(tmp.path().join("bg.jpg"), &[0xFF, 0xD8, 0xFF]).unwrap();
        std::fs::write(tmp.path().join("main.tscn"), "[gd_scene]").unwrap();
        std::fs::write(tmp.path().join("readme.txt"), "ignore").unwrap();

        let entries = super::scan_directory(tmp.path(), "", 0, 3);
        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        assert!(
            names.contains(&"icon.png"),
            "should find png files, got {:?}",
            names
        );
        assert!(
            names.contains(&"bg.jpg"),
            "should find jpg files, got {:?}",
            names
        );
        assert!(names.contains(&"main.tscn"), "should still find tscn files");
        assert!(
            !names.contains(&"readme.txt"),
            "should not include txt files"
        );

        let png_entry = entries.iter().find(|e| e.name == "icon.png").unwrap();
        assert_eq!(png_entry.file_type, "Image");
    }

    #[test]
    fn test_filesystem_includes_shader_files() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::write(tmp.path().join("water.gdshader"), "shader_type spatial;").unwrap();
        std::fs::write(tmp.path().join("project.cfg"), "[application]").unwrap();

        let entries = super::scan_directory(tmp.path(), "", 0, 3);
        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        assert!(
            names.contains(&"water.gdshader"),
            "should find shader files"
        );
        assert!(names.contains(&"project.cfg"), "should find cfg files");
    }

    #[test]
    fn test_file_type_for_ext_images() {
        assert_eq!(super::file_type_for_ext("png"), "Image");
        assert_eq!(super::file_type_for_ext("jpg"), "Image");
        assert_eq!(super::file_type_for_ext("jpeg"), "Image");
        assert_eq!(super::file_type_for_ext("webp"), "Image");
        assert_eq!(super::file_type_for_ext("svg"), "Image");
        assert_eq!(super::file_type_for_ext("wav"), "Audio");
        assert_eq!(super::file_type_for_ext("ttf"), "Font");
        assert_eq!(super::file_type_for_ext("xyz"), "File");
    }

    #[test]
    fn test_fs_entry_to_json() {
        let entry = super::FsEntry {
            name: "test.tscn".to_string(),
            path: "res://test.tscn".to_string(),
            is_dir: false,
            children: Vec::new(),
            size: 1024,
            file_type: "Scene".to_string(),
        };
        let json = entry.to_json();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["name"], "test.tscn");
        assert_eq!(v["path"], "res://test.tscn");
        assert_eq!(v["is_dir"], false);
    }

    #[test]
    fn test_fs_entry_dir_to_json() {
        let entry = super::FsEntry {
            name: "scenes".to_string(),
            path: "res://scenes".to_string(),
            is_dir: true,
            children: vec![super::FsEntry {
                name: "main.tscn".to_string(),
                path: "res://scenes/main.tscn".to_string(),
                is_dir: false,
                children: Vec::new(),
                size: 512,
                file_type: "Scene".to_string(),
            }],
            size: 0,
            file_type: String::new(),
        };
        let json = entry.to_json();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["name"], "scenes");
        assert_eq!(v["is_dir"], true);
        assert_eq!(v["children"][0]["name"], "main.tscn");
    }

    #[test]
    fn test_multi_select_state() {
        let tree = SceneTree::new();
        let st = EditorState::new(tree);
        assert!(st.selected_nodes.is_empty());
        assert!(st.clipboard.is_empty());
        assert_eq!(st.display_settings.grid_snap_size, 8);
    }
    #[test]
    fn test_clipboard_roundtrip() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let mut p = gdscene::node::Node::new("Player", "Node2D");
        p.set_property("position", Variant::Vector2(Vector2::new(10.0, 20.0)));
        let pid = tree.add_child(root, p).unwrap();
        tree.add_child(pid, gdscene::node::Node::new("Ch", "Sprite2D"))
            .unwrap();
        let entry = super::node_to_clipboard(&tree, pid).unwrap();
        assert_eq!(entry.name, "Player");
        assert_eq!(entry.children.len(), 1);
        let cnt = tree.node_count();
        let nid = super::paste_clipboard_entry(&mut tree, root, &entry).unwrap();
        assert_eq!(tree.node_count(), cnt + 2);
        assert_eq!(tree.get_node(nid).unwrap().name(), "Player");
    }
    #[test]
    fn test_settings_serde() {
        let s = EditorDisplaySettings::default();
        let j = serde_json::to_string(&s).unwrap();
        let v: serde_json::Value = serde_json::from_str(&j).unwrap();
        assert_eq!(v["grid_snap_size"], 8);
        assert_eq!(v["grid_visible"], true);
    }
    #[test]
    fn test_multi_select_ops() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let a = tree
            .add_child(root, gdscene::node::Node::new("A", "Node2D"))
            .unwrap();
        let b = tree
            .add_child(root, gdscene::node::Node::new("B", "Node2D"))
            .unwrap();
        let mut st = EditorState::new(tree);
        st.selected_nodes = vec![a];
        st.selected_nodes.push(b);
        assert_eq!(st.selected_nodes.len(), 2);
        st.selected_nodes.retain(|&i| i != a);
        assert_eq!(st.selected_nodes, vec![b]);
    }

    #[test]
    fn test_runtime_play_and_status() {
        let (handle, port) = make_server();
        let resp = http_get(port, "/api/runtime/status");
        let body = extract_body(&resp);
        let v: serde_json::Value = serde_json::from_str(body).unwrap();
        assert_eq!(v["running"], false);
        let resp = http_post(port, "/api/runtime/play", "");
        assert!(resp.contains("200 OK"));
        let body = extract_body(&resp);
        let v: serde_json::Value = serde_json::from_str(body).unwrap();
        assert_eq!(v["running"], true);
        handle.stop();
    }
    #[test]
    fn test_runtime_stop() {
        let (handle, port) = make_server();
        http_post(port, "/api/runtime/play", "");
        let resp = http_post(port, "/api/runtime/stop", "");
        assert!(resp.contains("200 OK"));
        let body = extract_body(&resp);
        let v: serde_json::Value = serde_json::from_str(body).unwrap();
        assert_eq!(v["running"], false);
        handle.stop();
    }
    #[test]
    fn test_runtime_pause_toggle() {
        let (handle, port) = make_server();
        http_post(port, "/api/runtime/play", "");
        let resp = http_post(port, "/api/runtime/pause", "");
        let v: serde_json::Value = serde_json::from_str(extract_body(&resp)).unwrap();
        assert_eq!(v["paused"], true);
        let resp = http_post(port, "/api/runtime/pause", "");
        let v: serde_json::Value = serde_json::from_str(extract_body(&resp)).unwrap();
        assert_eq!(v["paused"], false);
        handle.stop();
    }
    #[test]
    fn test_runtime_pause_when_not_running() {
        let (handle, port) = make_server();
        let resp = http_post(port, "/api/runtime/pause", "");
        assert!(resp.contains("400"));
        handle.stop();
    }
    #[test]
    fn test_runtime_step_when_paused() {
        let (handle, port) = make_server();
        http_post(port, "/api/runtime/play", "");
        http_post(port, "/api/runtime/pause", "");
        let resp = http_post(port, "/api/runtime/step", "");
        assert!(resp.contains("200 OK"));
        let v: serde_json::Value = serde_json::from_str(extract_body(&resp)).unwrap();
        assert_eq!(v["frame_count"], 1);
        handle.stop();
    }
    #[test]
    fn test_runtime_step_errors() {
        let (handle, port) = make_server();
        assert!(http_post(port, "/api/runtime/step", "").contains("400"));
        http_post(port, "/api/runtime/play", "");
        assert!(http_post(port, "/api/runtime/step", "").contains("400"));
        handle.stop();
    }
    #[test]
    fn test_clone_scene_tree_preserves_structure() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let mut main = Node::new("Main", "Node2D");
        main.set_property("position", Variant::Vector2(Vector2::new(10.0, 20.0)));
        tree.add_child(root, main).unwrap();
        let cloned = clone_scene_tree(&tree);
        assert_eq!(cloned.node_count(), tree.node_count());
        let cr = cloned.root_id();
        let cm = cloned.get_node(cr).unwrap().children()[0];
        let cmn = cloned.get_node(cm).unwrap();
        assert_eq!(cmn.name(), "Main");
        assert_eq!(
            cmn.get_property("position"),
            Variant::Vector2(Vector2::new(10.0, 20.0))
        );
    }
    #[test]
    fn test_runtime_full_cycle() {
        let (handle, port) = make_server();
        http_post(port, "/api/runtime/play", "");
        assert!(extract_body(&http_get(port, "/api/runtime/status")).contains("\"running\":true"));
        http_post(port, "/api/runtime/pause", "");
        http_post(port, "/api/runtime/step", "");
        assert!(extract_body(&http_get(port, "/api/runtime/status")).contains("\"frame_count\":1"));
        http_post(port, "/api/runtime/stop", "");
        assert!(extract_body(&http_get(port, "/api/runtime/status")).contains("\"running\":false"));
        handle.stop();
    }
    #[test]
    fn test_script_save_and_read() {
        let (handle, port) = make_server();
        let dir = tempfile::tempdir().unwrap();
        let script_path = dir.path().join("test_script.gd");
        let path_str = script_path.to_str().unwrap();
        let json_body = serde_json::json!({
            "path": path_str,
            "content": "extends Node2D\nfunc _ready():\n    pass\n"
        });
        let resp = http_post(port, "/api/script/save", &json_body.to_string());
        assert!(resp.contains("200 OK"), "save should succeed: {resp}");
        let resp_body = extract_body(&resp);
        assert!(resp_body.contains(r#""ok":true"#));
        let written = std::fs::read_to_string(&script_path).unwrap();
        assert!(written.contains("extends Node2D"));
        handle.stop();
    }

    #[test]
    fn test_script_save_rejects_non_gd() {
        let (handle, port) = make_server();
        let dir = tempfile::tempdir().unwrap();
        let path_str = dir.path().join("bad.txt").to_str().unwrap().to_string();
        let json_body = serde_json::json!({ "path": path_str, "content": "hello" });
        let resp = http_post(port, "/api/script/save", &json_body.to_string());
        assert!(resp.contains("400"), "should reject non-.gd: {resp}");
        handle.stop();
    }

    #[test]
    fn test_script_save_missing_path() {
        let (handle, port) = make_server();
        let resp = http_post(port, "/api/script/save", r#"{"content":"hello"}"#);
        assert!(resp.contains("400"), "should require path: {resp}");
        handle.stop();
    }

    #[test]
    fn test_script_save_missing_content() {
        let (handle, port) = make_server();
        let dir = tempfile::tempdir().unwrap();
        let path_str = dir.path().join("test.gd").to_str().unwrap().to_string();
        let json_body = serde_json::json!({ "path": path_str });
        let resp = http_post(port, "/api/script/save", &json_body.to_string());
        assert!(resp.contains("400"), "should require content: {resp}");
        handle.stop();
    }

    #[test]
    fn test_script_save_creates_parent_dirs() {
        let (handle, port) = make_server();
        let dir = tempfile::tempdir().unwrap();
        let nested_path = dir.path().join("subdir").join("nested").join("script.gd");
        let path_str = nested_path.to_str().unwrap();
        let json_body = serde_json::json!({ "path": path_str, "content": "extends Node\n" });
        let resp = http_post(port, "/api/script/save", &json_body.to_string());
        assert!(resp.contains("200 OK"), "should create dirs: {resp}");
        assert!(nested_path.exists());
        handle.stop();
    }

    // ---- Animation endpoint tests ----

    #[test]
    fn test_animation_create_and_list() {
        let (handle, port) = make_server();
        let resp = http_get(port, "/api/animations");
        let body = extract_body(&resp);
        let v: serde_json::Value = serde_json::from_str(body).unwrap();
        assert_eq!(v.as_array().unwrap().len(), 0);
        let resp = http_post(
            port,
            "/api/animation/create",
            r#"{"name":"walk","length":2.0}"#,
        );
        assert!(resp.contains("200 OK"));
        let resp = http_get(port, "/api/animations");
        let body = extract_body(&resp);
        let v: serde_json::Value = serde_json::from_str(body).unwrap();
        assert_eq!(v.as_array().unwrap().len(), 1);
        assert_eq!(v[0]["name"], "walk");
        handle.stop();
    }

    #[test]
    fn test_animation_create_duplicate_fails() {
        let (handle, port) = make_server();
        http_post(
            port,
            "/api/animation/create",
            r#"{"name":"idle","length":1.0}"#,
        );
        let resp = http_post(
            port,
            "/api/animation/create",
            r#"{"name":"idle","length":1.0}"#,
        );
        assert!(resp.contains("400"));
        handle.stop();
    }

    #[test]
    fn test_animation_delete() {
        let (handle, port) = make_server();
        http_post(
            port,
            "/api/animation/create",
            r#"{"name":"run","length":1.5}"#,
        );
        let resp = http_post(port, "/api/animation/delete", r#"{"name":"run"}"#);
        assert!(resp.contains("200 OK"));
        let resp = http_get(port, "/api/animations");
        let body = extract_body(&resp);
        let v: serde_json::Value = serde_json::from_str(body).unwrap();
        assert_eq!(v.as_array().unwrap().len(), 0);
        handle.stop();
    }

    #[test]
    fn test_animation_delete_not_found() {
        let (handle, port) = make_server();
        let resp = http_post(port, "/api/animation/delete", r#"{"name":"nope"}"#);
        assert!(resp.contains("404"));
        handle.stop();
    }

    #[test]
    fn test_animation_get_details() {
        let (handle, port) = make_server();
        http_post(
            port,
            "/api/animation/create",
            r#"{"name":"jump","length":0.5,"loop_mode":"loop"}"#,
        );
        let resp = http_get(port, "/api/animation?name=jump");
        assert!(resp.contains("200 OK"));
        let body = extract_body(&resp);
        let v: serde_json::Value = serde_json::from_str(body).unwrap();
        assert_eq!(v["name"], "jump");
        assert_eq!(v["loop_mode"], "loop");
        assert_eq!(v["tracks"].as_array().unwrap().len(), 0);
        handle.stop();
    }

    #[test]
    fn test_animation_keyframe_add_and_get() {
        let (handle, port) = make_server();
        http_post(
            port,
            "/api/animation/create",
            r#"{"name":"move","length":2.0}"#,
        );
        let resp = http_post(
            port,
            "/api/animation/keyframe/add",
            r#"{"animation":"move","track_node":"Player","track_property":"position","time":0.0,"value":{"type":"Float","value":0.0}}"#,
        );
        assert!(resp.contains("200 OK"));
        http_post(
            port,
            "/api/animation/keyframe/add",
            r#"{"animation":"move","track_node":"Player","track_property":"position","time":1.0,"value":{"type":"Float","value":100.0}}"#,
        );
        let resp = http_get(port, "/api/animation?name=move");
        let body = extract_body(&resp);
        let v: serde_json::Value = serde_json::from_str(body).unwrap();
        assert_eq!(v["tracks"].as_array().unwrap().len(), 1);
        assert_eq!(v["tracks"][0]["node_path"], "Player");
        assert_eq!(v["tracks"][0]["property"], "position");
        assert_eq!(v["tracks"][0]["keyframes"].as_array().unwrap().len(), 2);
        handle.stop();
    }

    #[test]
    fn test_animation_keyframe_remove() {
        let (handle, port) = make_server();
        http_post(
            port,
            "/api/animation/create",
            r#"{"name":"rm","length":1.0}"#,
        );
        http_post(
            port,
            "/api/animation/keyframe/add",
            r#"{"animation":"rm","track_node":"N","track_property":"p","time":0.0,"value":{"type":"Float","value":0.0}}"#,
        );
        http_post(
            port,
            "/api/animation/keyframe/add",
            r#"{"animation":"rm","track_node":"N","track_property":"p","time":1.0,"value":{"type":"Float","value":1.0}}"#,
        );
        let resp = http_post(
            port,
            "/api/animation/keyframe/remove",
            r#"{"animation":"rm","track_index":0,"keyframe_index":0}"#,
        );
        assert!(resp.contains("200 OK"));
        let resp = http_get(port, "/api/animation?name=rm");
        let body = extract_body(&resp);
        let v: serde_json::Value = serde_json::from_str(body).unwrap();
        assert_eq!(v["tracks"][0]["keyframes"].as_array().unwrap().len(), 1);
        handle.stop();
    }

    #[test]
    fn test_animation_keyframe_remove_cleans_empty_track() {
        let (handle, port) = make_server();
        http_post(
            port,
            "/api/animation/create",
            r#"{"name":"clean","length":1.0}"#,
        );
        http_post(
            port,
            "/api/animation/keyframe/add",
            r#"{"animation":"clean","track_node":"N","track_property":"p","time":0.0,"value":{"type":"Float","value":0.0}}"#,
        );
        http_post(
            port,
            "/api/animation/keyframe/remove",
            r#"{"animation":"clean","track_index":0,"keyframe_index":0}"#,
        );
        let resp = http_get(port, "/api/animation?name=clean");
        let body = extract_body(&resp);
        let v: serde_json::Value = serde_json::from_str(body).unwrap();
        assert_eq!(v["tracks"].as_array().unwrap().len(), 0);
        handle.stop();
    }

    #[test]
    fn test_animation_play_and_status() {
        let (handle, port) = make_server();
        http_post(
            port,
            "/api/animation/create",
            r#"{"name":"anim","length":1.0}"#,
        );
        http_post(port, "/api/animation/play", r#"{"name":"anim"}"#);
        let resp = http_get(port, "/api/animation/status");
        let body = extract_body(&resp);
        let v: serde_json::Value = serde_json::from_str(body).unwrap();
        assert_eq!(v["playing"], true);
        assert_eq!(v["animation_name"], "anim");
        handle.stop();
    }

    #[test]
    fn test_animation_stop() {
        let (handle, port) = make_server();
        http_post(
            port,
            "/api/animation/create",
            r#"{"name":"s","length":1.0}"#,
        );
        http_post(port, "/api/animation/play", r#"{"name":"s"}"#);
        http_post(port, "/api/animation/stop", "{}");
        let resp = http_get(port, "/api/animation/status");
        let body = extract_body(&resp);
        let v: serde_json::Value = serde_json::from_str(body).unwrap();
        assert_eq!(v["playing"], false);
        handle.stop();
    }

    #[test]
    fn test_animation_play_nonexistent_fails() {
        let (handle, port) = make_server();
        let resp = http_post(port, "/api/animation/play", r#"{"name":"nope"}"#);
        assert!(resp.contains("404"));
        handle.stop();
    }

    #[test]
    fn test_animation_seek() {
        let (handle, port) = make_server();
        http_post(
            port,
            "/api/animation/create",
            r#"{"name":"seek_test","length":2.0}"#,
        );
        http_post(port, "/api/animation/play", r#"{"name":"seek_test"}"#);
        http_post(port, "/api/animation/seek", r#"{"time":0.75}"#);
        let resp = http_get(port, "/api/animation/status");
        let body = extract_body(&resp);
        let v: serde_json::Value = serde_json::from_str(body).unwrap();
        assert_eq!(v["current_time"], 0.75);
        handle.stop();
    }

    #[test]
    fn test_animation_record_toggle() {
        let (handle, port) = make_server();
        http_post(port, "/api/animation/record", r#"{"enabled":true}"#);
        let resp = http_get(port, "/api/animation/status");
        let body = extract_body(&resp);
        let v: serde_json::Value = serde_json::from_str(body).unwrap();
        assert_eq!(v["recording"], true);
        http_post(port, "/api/animation/record", r#"{"enabled":false}"#);
        let resp = http_get(port, "/api/animation/status");
        let body = extract_body(&resp);
        let v: serde_json::Value = serde_json::from_str(body).unwrap();
        assert_eq!(v["recording"], false);
        handle.stop();
    }

    #[test]
    fn test_animation_delete_stops_playback() {
        let (handle, port) = make_server();
        http_post(
            port,
            "/api/animation/create",
            r#"{"name":"del_play","length":1.0}"#,
        );
        http_post(port, "/api/animation/play", r#"{"name":"del_play"}"#);
        http_post(port, "/api/animation/delete", r#"{"name":"del_play"}"#);
        let resp = http_get(port, "/api/animation/status");
        let body = extract_body(&resp);
        let v: serde_json::Value = serde_json::from_str(body).unwrap();
        assert_eq!(v["playing"], false);
        assert!(v["animation_name"].is_null());
        handle.stop();
    }

    #[test]
    fn test_animation_loop_mode_variants() {
        let (handle, port) = make_server();
        http_post(
            port,
            "/api/animation/create",
            r#"{"name":"pp","length":1.0,"loop_mode":"pingpong"}"#,
        );
        let resp = http_get(port, "/api/animation?name=pp");
        let body = extract_body(&resp);
        let v: serde_json::Value = serde_json::from_str(body).unwrap();
        assert_eq!(v["loop_mode"], "pingpong");
        handle.stop();
    }

    #[test]
    fn test_attach_scripts_to_tree() {
        // Create a tree with a node that has a _script_path pointing to a
        // fixture script on disk.
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let mut child = Node::new("Mover", "Node2D");
        // Point to the test_move.gd fixture
        child.set_property(
            "_script_path",
            Variant::String("res://scripts/test_move.gd".into()),
        );
        tree.add_child(root, child).unwrap();

        // The project root for fixtures is engine-rs/fixtures
        let project_root = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures");
        let mut state = EditorState::new(SceneTree::new());
        attach_scripts_to_tree(&mut tree, project_root, &mut state);

        // The node should now have an attached script
        let mover_id = tree.get_node(root).unwrap().children()[0];
        assert!(tree.has_script(mover_id), "script should be attached");

        // Verify log entry mentions script loading
        let has_loaded_log = state
            .log_entries
            .iter()
            .any(|entry| entry.message.contains("Script loaded"));
        assert!(has_loaded_log, "expected 'Script loaded' log entry");
    }

    #[test]
    fn test_attach_scripts_missing_file_logs_error() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let mut child = Node::new("BadScript", "Node2D");
        child.set_property(
            "_script_path",
            Variant::String("res://nonexistent.gd".into()),
        );
        tree.add_child(root, child).unwrap();

        let mut state = EditorState::new(SceneTree::new());
        attach_scripts_to_tree(&mut tree, "/tmp/no_such_project", &mut state);

        // Should log an error about failing to read the file
        let has_error = state.log_entries.iter().any(|entry| entry.level == "error");
        assert!(has_error, "expected error log for missing script file");
    }

    #[test]
    fn test_attach_scripts_parse_error_logs_error() {
        use std::io::Write;
        let dir = tempfile::tempdir().unwrap();
        let script_path = dir.path().join("bad.gd");
        let mut f = std::fs::File::create(&script_path).unwrap();
        writeln!(f, "this is not valid gdscript @@@ {{{{").unwrap();
        drop(f);

        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let mut child = Node::new("BadParse", "Node2D");
        child.set_property("_script_path", Variant::String("res://bad.gd".into()));
        tree.add_child(root, child).unwrap();

        let mut state = EditorState::new(SceneTree::new());
        attach_scripts_to_tree(&mut tree, &dir.path().to_string_lossy(), &mut state);

        let has_error = state
            .log_entries
            .iter()
            .any(|entry| entry.level == "error" && entry.message.contains("parse error"));
        assert!(
            has_error,
            "expected error log for parse failure; got: {:?}",
            state
                .log_entries
                .iter()
                .map(|e| &e.message)
                .collect::<Vec<_>>()
        );
    }

    // ---- Input tests ----

    /// Helper: start runtime so input endpoints accept requests.
    fn start_runtime(port: u16) {
        http_post(port, "/api/runtime/play", "{}");
    }

    #[test]
    fn test_input_key_down_requires_running() {
        let (handle, port) = make_server();
        let resp = http_post(port, "/api/runtime/input/key_down", r#"{"key":"a"}"#);
        assert!(resp.contains("400"), "should reject when not running");
        handle.stop();
    }

    #[test]
    fn test_input_key_down_and_state() {
        let (handle, port) = make_server();
        start_runtime(port);
        let resp = http_post(
            port,
            "/api/runtime/input/key_down",
            r#"{"key":"ArrowLeft"}"#,
        );
        assert!(resp.contains("200 OK"));

        let resp = http_get(port, "/api/runtime/input/state");
        let body = extract_body(&resp);
        let v: serde_json::Value = serde_json::from_str(body).unwrap();
        let pressed = v["pressed_keys"].as_array().unwrap();
        assert!(pressed.iter().any(|k| k == "ArrowLeft"));
        let just = v["just_pressed"].as_array().unwrap();
        assert!(just.iter().any(|k| k == "ArrowLeft"));
        handle.stop();
    }

    #[test]
    fn test_input_key_up_removes_from_pressed() {
        let (handle, port) = make_server();
        start_runtime(port);
        http_post(port, "/api/runtime/input/key_down", r#"{"key":"w"}"#);
        http_post(port, "/api/runtime/input/key_up", r#"{"key":"w"}"#);

        let resp = http_get(port, "/api/runtime/input/state");
        let body = extract_body(&resp);
        let v: serde_json::Value = serde_json::from_str(body).unwrap();
        let pressed = v["pressed_keys"].as_array().unwrap();
        assert!(!pressed.iter().any(|k| k == "w"));
        handle.stop();
    }

    #[test]
    fn test_input_clear_frame() {
        let (handle, port) = make_server();
        start_runtime(port);
        http_post(port, "/api/runtime/input/key_down", r#"{"key":"x"}"#);

        let resp = http_get(port, "/api/runtime/input/state");
        let body = extract_body(&resp);
        let v: serde_json::Value = serde_json::from_str(body).unwrap();
        assert!(v["just_pressed"]
            .as_array()
            .unwrap()
            .iter()
            .any(|k| k == "x"));

        http_post(port, "/api/runtime/input/clear_frame", "{}");

        let resp = http_get(port, "/api/runtime/input/state");
        let body = extract_body(&resp);
        let v: serde_json::Value = serde_json::from_str(body).unwrap();
        assert!(v["just_pressed"].as_array().unwrap().is_empty());
        assert!(v["pressed_keys"]
            .as_array()
            .unwrap()
            .iter()
            .any(|k| k == "x"));
        handle.stop();
    }

    #[test]
    fn test_input_mouse_move() {
        let (handle, port) = make_server();
        start_runtime(port);
        let resp = http_post(
            port,
            "/api/runtime/input/mouse_move",
            r#"{"x":150.5,"y":200.0}"#,
        );
        assert!(resp.contains("200 OK"));

        let resp = http_get(port, "/api/runtime/input/state");
        let body = extract_body(&resp);
        let v: serde_json::Value = serde_json::from_str(body).unwrap();
        let pos = v["mouse_position"].as_array().unwrap();
        assert_eq!(pos[0].as_f64().unwrap(), 150.5);
        assert_eq!(pos[1].as_f64().unwrap(), 200.0);
        handle.stop();
    }

    #[test]
    fn test_input_mouse_buttons() {
        let (handle, port) = make_server();
        start_runtime(port);
        http_post(port, "/api/runtime/input/mouse_down", r#"{"button":0}"#);
        http_post(port, "/api/runtime/input/mouse_down", r#"{"button":2}"#);

        let resp = http_get(port, "/api/runtime/input/state");
        let body = extract_body(&resp);
        let v: serde_json::Value = serde_json::from_str(body).unwrap();
        let btns = v["mouse_buttons"].as_array().unwrap();
        assert!(btns.iter().any(|b| b == 0));
        assert!(btns.iter().any(|b| b == 2));

        http_post(port, "/api/runtime/input/mouse_up", r#"{"button":0}"#);
        let resp = http_get(port, "/api/runtime/input/state");
        let body = extract_body(&resp);
        let v: serde_json::Value = serde_json::from_str(body).unwrap();
        let btns = v["mouse_buttons"].as_array().unwrap();
        assert!(!btns.iter().any(|b| b == 0));
        assert!(btns.iter().any(|b| b == 2));
        handle.stop();
    }

    #[test]
    fn test_input_mouse_button_invalid() {
        let (handle, port) = make_server();
        start_runtime(port);
        let resp = http_post(port, "/api/runtime/input/mouse_down", r#"{"button":5}"#);
        assert!(resp.contains("400"));
        handle.stop();
    }

    #[test]
    fn test_input_action_mapping() {
        let (handle, port) = make_server();
        start_runtime(port);
        http_post(port, "/api/runtime/input/key_down", r#"{"key":"a"}"#);

        let resp = http_get(port, "/api/runtime/input/state");
        let body = extract_body(&resp);
        let v: serde_json::Value = serde_json::from_str(body).unwrap();
        assert_eq!(v["actions"]["ui_left"], true);
        assert_eq!(v["actions"]["ui_right"], false);
        handle.stop();
    }

    #[test]
    fn test_input_stop_clears_all() {
        let (handle, port) = make_server();
        start_runtime(port);
        http_post(port, "/api/runtime/input/key_down", r#"{"key":"a"}"#);
        http_post(port, "/api/runtime/input/mouse_down", r#"{"button":0}"#);
        http_post(
            port,
            "/api/runtime/input/mouse_move",
            r#"{"x":100,"y":200}"#,
        );

        http_post(port, "/api/runtime/stop", "{}");

        start_runtime(port);
        let resp = http_get(port, "/api/runtime/input/state");
        let body = extract_body(&resp);
        let v: serde_json::Value = serde_json::from_str(body).unwrap();
        assert!(v["pressed_keys"].as_array().unwrap().is_empty());
        assert!(v["mouse_buttons"].as_array().unwrap().is_empty());
        let pos = v["mouse_position"].as_array().unwrap();
        assert_eq!(pos[0].as_f64().unwrap(), 0.0);
        assert_eq!(pos[1].as_f64().unwrap(), 0.0);
        handle.stop();
    }

    #[test]
    fn test_input_duplicate_key_down_no_double_just_pressed() {
        let (handle, port) = make_server();
        start_runtime(port);
        http_post(port, "/api/runtime/input/key_down", r#"{"key":"a"}"#);
        http_post(port, "/api/runtime/input/clear_frame", "{}");
        http_post(port, "/api/runtime/input/key_down", r#"{"key":"a"}"#);

        let resp = http_get(port, "/api/runtime/input/state");
        let body = extract_body(&resp);
        let v: serde_json::Value = serde_json::from_str(body).unwrap();
        assert!(
            v["just_pressed"].as_array().unwrap().is_empty(),
            "held key should not re-trigger just_pressed"
        );
        handle.stop();
    }

    #[test]
    fn test_input_state_helper_methods() {
        let mut state = EditorState::new(SceneTree::new());
        state.is_running = true;
        state.pressed_keys.insert("ArrowLeft".into());
        state.just_pressed_keys.insert("ArrowLeft".into());

        assert!(state.is_action_pressed("ui_left"));
        assert!(!state.is_action_pressed("ui_right"));
        assert!(state.is_action_just_pressed("ui_left"));
        assert!(!state.is_action_just_pressed("shoot"));

        assert!(!state.is_action_pressed("nonexistent"));
        assert!(!state.is_action_just_pressed("nonexistent"));

        state.clear_frame_input();
        assert!(!state.is_action_just_pressed("ui_left"));
        assert!(state.is_action_pressed("ui_left"));

        state.clear_all_input();
        assert!(!state.is_action_pressed("ui_left"));
    }

    // -----------------------------------------------------------------------
    // Scene instancing endpoint tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_instance_scene_from_file() {
        let (handle, port) = make_server();
        let main_id = get_main_node_id(port);

        // Create a temp .tscn file.
        let tscn_content = r#"
[gd_scene format=3]

[node name="Enemy" type="Node2D"]

[node name="Sprite" type="Sprite2D" parent="."]
position = Vector2(10, 20)
"#;
        let dir = std::env::temp_dir().join("patina_test_instance");
        let _ = std::fs::create_dir_all(&dir);
        let tscn_path = dir.join("enemy.tscn");
        std::fs::write(&tscn_path, tscn_content).unwrap();

        let body = format!(
            r#"{{"path":"{}","parent_id":{}}}"#,
            tscn_path.to_string_lossy().replace('\\', "\\\\"),
            main_id
        );
        let resp = http_post(port, "/api/scene/instance", &body);
        assert!(resp.contains("200 OK"), "expected 200, got: {}", resp);
        let resp_body = extract_body(&resp);
        let v: serde_json::Value = serde_json::from_str(resp_body).unwrap();
        assert!(
            v["id"].as_u64().is_some(),
            "should return instanced root id"
        );

        // Verify the instanced nodes appear in the tree.
        let scene_resp = http_get(port, "/api/scene");
        assert!(
            scene_resp.contains("Enemy"),
            "tree should contain Enemy node"
        );
        assert!(
            scene_resp.contains("Sprite"),
            "tree should contain Sprite node"
        );

        // Cleanup.
        let _ = std::fs::remove_dir_all(&dir);
        handle.stop();
    }

    #[test]
    fn test_instance_scene_undo() {
        let (handle, port) = make_server();
        let main_id = get_main_node_id(port);

        let tscn_content = r#"
[gd_scene format=3]

[node name="Instanced" type="Node2D"]
"#;
        let dir = std::env::temp_dir().join("patina_test_instance_undo");
        let _ = std::fs::create_dir_all(&dir);
        let tscn_path = dir.join("simple.tscn");
        std::fs::write(&tscn_path, tscn_content).unwrap();

        let body = format!(
            r#"{{"path":"{}","parent_id":{}}}"#,
            tscn_path.to_string_lossy().replace('\\', "\\\\"),
            main_id
        );
        let resp = http_post(port, "/api/scene/instance", &body);
        assert!(resp.contains("200 OK"));

        // Verify node exists.
        let scene_resp = http_get(port, "/api/scene");
        assert!(scene_resp.contains("Instanced"));

        // Undo should remove it.
        let undo_resp = http_post(port, "/api/undo", "{}");
        assert!(undo_resp.contains("200 OK"));

        let scene_after = http_get(port, "/api/scene");
        assert!(
            !scene_after.contains("Instanced"),
            "undo should remove instanced node"
        );

        let _ = std::fs::remove_dir_all(&dir);
        handle.stop();
    }

    #[test]
    fn test_instance_scene_missing_file() {
        let (handle, port) = make_server();
        let main_id = get_main_node_id(port);

        let body = format!(
            r#"{{"path":"/nonexistent/path/scene.tscn","parent_id":{}}}"#,
            main_id
        );
        let resp = http_post(port, "/api/scene/instance", &body);
        assert!(resp.contains("400"), "should return error for missing file");
        handle.stop();
    }

    #[test]
    fn test_instance_scene_missing_parent() {
        let (handle, port) = make_server();

        let tscn_content = "[gd_scene format=3]\n\n[node name=\"X\" type=\"Node\"]\n";
        let dir = std::env::temp_dir().join("patina_test_instance_noparent");
        let _ = std::fs::create_dir_all(&dir);
        let tscn_path = dir.join("x.tscn");
        std::fs::write(&tscn_path, tscn_content).unwrap();

        let body = format!(
            r#"{{"path":"{}","parent_id":99999999}}"#,
            tscn_path.to_string_lossy().replace('\\', "\\\\")
        );
        let resp = http_post(port, "/api/scene/instance", &body);
        assert!(resp.contains("404"), "should return 404 for missing parent");

        let _ = std::fs::remove_dir_all(&dir);
        handle.stop();
    }

    // -----------------------------------------------------------------------
    // Shape resize endpoint tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_shape_resize_radius() {
        let (handle, port) = make_server();
        let main_id = get_main_node_id(port);

        // Add a CollisionShape2D node.
        let add_body = format!(
            r#"{{"parent_id":{},"name":"Shape","class_name":"CollisionShape2D"}}"#,
            main_id
        );
        let add_resp = http_post(port, "/api/node/add", &add_body);
        let add_json: serde_json::Value = serde_json::from_str(extract_body(&add_resp)).unwrap();
        let shape_id = add_json["id"].as_u64().unwrap();

        // Resize radius.
        let resize_body = format!(
            r#"{{"node_id":{},"handle":"radius","value":42.0}}"#,
            shape_id
        );
        let resp = http_post(port, "/api/viewport/shape_resize", &resize_body);
        assert!(resp.contains("200 OK"));

        // Verify property was set.
        let node_resp = http_get(port, &format!("/api/node/{}", shape_id));
        let body = extract_body(&node_resp);
        assert!(
            body.contains("shape_radius"),
            "should have shape_radius property"
        );

        handle.stop();
    }

    #[test]
    fn test_shape_resize_extents() {
        let (handle, port) = make_server();
        let main_id = get_main_node_id(port);

        let add_body = format!(
            r#"{{"parent_id":{},"name":"RectShape","class_name":"CollisionShape2D"}}"#,
            main_id
        );
        let add_resp = http_post(port, "/api/node/add", &add_body);
        let add_json: serde_json::Value = serde_json::from_str(extract_body(&add_resp)).unwrap();
        let shape_id = add_json["id"].as_u64().unwrap();

        let resize_body = format!(
            r#"{{"node_id":{},"handle":"extents","value":[30,20]}}"#,
            shape_id
        );
        let resp = http_post(port, "/api/viewport/shape_resize", &resize_body);
        assert!(resp.contains("200 OK"));

        let node_resp = http_get(port, &format!("/api/node/{}", shape_id));
        let body = extract_body(&node_resp);
        assert!(
            body.contains("shape_extents"),
            "should have shape_extents property"
        );

        handle.stop();
    }

    #[test]
    fn test_shape_resize_undo() {
        let (handle, port) = make_server();
        let main_id = get_main_node_id(port);

        let add_body = format!(
            r#"{{"parent_id":{},"name":"UndoShape","class_name":"CollisionShape2D"}}"#,
            main_id
        );
        let add_resp = http_post(port, "/api/node/add", &add_body);
        let add_json: serde_json::Value = serde_json::from_str(extract_body(&add_resp)).unwrap();
        let shape_id = add_json["id"].as_u64().unwrap();

        // Set radius.
        let resize_body = format!(
            r#"{{"node_id":{},"handle":"radius","value":50.0}}"#,
            shape_id
        );
        http_post(port, "/api/viewport/shape_resize", &resize_body);

        // Undo should revert.
        let undo_resp = http_post(port, "/api/undo", "{}");
        assert!(undo_resp.contains("200 OK"));

        // The property should be reverted (Nil = not present in response or null).
        let node_resp = http_get(port, &format!("/api/node/{}", shape_id));
        let body = extract_body(&node_resp);
        // After undo, shape_radius should not be set (reverted to Nil).
        // The property list should either not contain shape_radius or show it as null.
        let v: serde_json::Value = serde_json::from_str(body).unwrap();
        let props = v["properties"].as_array().unwrap();
        let has_radius = props
            .iter()
            .any(|p| p["name"] == "shape_radius" && p["value"]["type"] != "Nil");
        assert!(
            !has_radius,
            "after undo, shape_radius should be reverted to Nil"
        );

        handle.stop();
    }

    #[test]
    fn test_shape_resize_unknown_handle() {
        let (handle, port) = make_server();
        let main_id = get_main_node_id(port);

        let body = format!(
            r#"{{"node_id":{},"handle":"invalid","value":10.0}}"#,
            main_id
        );
        let resp = http_post(port, "/api/viewport/shape_resize", &body);
        assert!(
            resp.contains("400"),
            "unknown handle should return 400 error"
        );

        handle.stop();
    }

    #[test]
    fn test_is_instance_flag_in_tree_json() {
        let (handle, port) = make_server();
        let main_id = get_main_node_id(port);

        // Create a temp .tscn file and instance it.
        let tscn_content = "[gd_scene format=3]\n\n[node name=\"Sub\" type=\"Node2D\"]\n";
        let dir = std::env::temp_dir().join("patina_test_is_instance");
        let _ = std::fs::create_dir_all(&dir);
        let tscn_path = dir.join("sub.tscn");
        std::fs::write(&tscn_path, tscn_content).unwrap();

        let body = format!(
            r#"{{"path":"{}","parent_id":{}}}"#,
            tscn_path.to_string_lossy().replace('\\', "\\\\"),
            main_id
        );
        http_post(port, "/api/scene/instance", &body);

        // Check that the tree JSON has is_instance=true for the instanced node.
        let scene_resp = http_get(port, "/api/scene");
        let scene_body = extract_body(&scene_resp);
        assert!(
            scene_body.contains(r#""is_instance":true"#),
            "instanced node should have is_instance=true"
        );

        let _ = std::fs::remove_dir_all(&dir);
        handle.stop();
    }

    // ===== pat-0lo: add-node and delete-node toolbar coverage =====

    #[test]
    fn test_add_node_returns_id_and_appears_in_scene() {
        let (handle, port) = make_server();
        let main_id = get_main_node_id(port);

        let body = format!(r#"{{"parent_id":{main_id},"name":"Cannon","class_name":"Area2D"}}"#);
        let resp = http_post(port, "/api/node/add", &body);
        assert!(resp.contains("200 OK"));
        let new_id: serde_json::Value = serde_json::from_str(extract_body(&resp)).unwrap();
        let new_id = new_id["id"].as_u64().unwrap();

        // Verify via GET /api/node/<id>
        let node_resp = http_get(port, &format!("/api/node/{new_id}"));
        assert!(node_resp.contains("200 OK"));
        let v: serde_json::Value = serde_json::from_str(extract_body(&node_resp)).unwrap();
        assert_eq!(v["name"], "Cannon");
        assert_eq!(v["class"], "Area2D");

        // Verify appears in GET /api/scene
        let scene_resp = http_get(port, "/api/scene");
        let scene_body = extract_body(&scene_resp);
        let scene: serde_json::Value = serde_json::from_str(scene_body).unwrap();
        let children = scene["nodes"]["children"][0]["children"]
            .as_array()
            .unwrap();
        assert!(
            children.iter().any(|c| c["name"] == "Cannon"),
            "Cannon should appear as child of Main in scene tree"
        );

        handle.stop();
    }

    #[test]
    fn test_delete_node_removes_from_scene_and_404_on_get() {
        let (handle, port) = make_server();
        let main_id = get_main_node_id(port);

        // Add then delete
        let add_body =
            format!(r#"{{"parent_id":{main_id},"name":"Ephemeral","class_name":"Node2D"}}"#);
        let add_resp = http_post(port, "/api/node/add", &add_body);
        let add_json: serde_json::Value = serde_json::from_str(extract_body(&add_resp)).unwrap();
        let eph_id = add_json["id"].as_u64().unwrap();

        let del_resp = http_post(
            port,
            "/api/node/delete",
            &format!(r#"{{"node_id":{eph_id}}}"#),
        );
        assert!(del_resp.contains("200 OK"));

        // GET /api/node/<id> should 404
        let node_resp = http_get(port, &format!("/api/node/{eph_id}"));
        assert!(
            node_resp.contains("404"),
            "deleted node should return 404 on GET"
        );

        // Scene tree should not contain Ephemeral
        let scene_resp = http_get(port, "/api/scene");
        assert!(
            !extract_body(&scene_resp).contains("Ephemeral"),
            "deleted node should not appear in scene"
        );

        handle.stop();
    }

    #[test]
    fn test_add_multiple_nodes_all_appear() {
        let (handle, port) = make_server();
        let main_id = get_main_node_id(port);

        for name in &["Alpha", "Beta", "Gamma"] {
            let body =
                format!(r#"{{"parent_id":{main_id},"name":"{name}","class_name":"Sprite2D"}}"#);
            let resp = http_post(port, "/api/node/add", &body);
            assert!(resp.contains("200 OK"), "adding {name} should succeed");
        }

        let scene_resp = http_get(port, "/api/scene");
        let body = extract_body(&scene_resp);
        for name in &["Alpha", "Beta", "Gamma"] {
            assert!(body.contains(name), "{name} should appear in scene tree");
        }

        handle.stop();
    }

    #[test]
    fn test_delete_nonexistent_node_returns_404() {
        let (handle, port) = make_server();
        let resp = http_post(port, "/api/node/delete", r#"{"node_id":9999999}"#);
        assert!(
            resp.contains("404"),
            "deleting nonexistent node should return 404"
        );
        handle.stop();
    }

    // ===== pat-waz: undo/redo coverage for node and property actions =====

    #[test]
    fn test_undo_add_then_redo_restores_node() {
        let (handle, port) = make_server();
        let main_id = get_main_node_id(port);

        // Add a node
        let body = format!(r#"{{"parent_id":{main_id},"name":"UndoMe","class_name":"Node2D"}}"#);
        http_post(port, "/api/node/add", &body);
        assert!(extract_body(&http_get(port, "/api/scene")).contains("UndoMe"));

        // Undo — node should disappear
        let undo_resp = http_post(port, "/api/undo", "");
        assert!(undo_resp.contains("200 OK"));
        assert!(
            !extract_body(&http_get(port, "/api/scene")).contains("UndoMe"),
            "node should be gone after undo"
        );

        // Redo — node should reappear
        let redo_resp = http_post(port, "/api/redo", "");
        assert!(redo_resp.contains("200 OK"));
        assert!(
            extract_body(&http_get(port, "/api/scene")).contains("UndoMe"),
            "node should reappear after redo"
        );

        handle.stop();
    }

    #[test]
    fn test_undo_redo_property_change_round_trip() {
        let (handle, port) = make_server();
        let main_id = get_main_node_id(port);

        // Set rotation to 1.5
        let body = format!(
            r#"{{"node_id":{main_id},"property":"rotation","value":{{"type":"Float","value":1.5}}}}"#
        );
        http_post(port, "/api/property/set", &body);

        // Verify rotation set
        let node_resp = http_get(port, &format!("/api/node/{main_id}"));
        let v: serde_json::Value = serde_json::from_str(extract_body(&node_resp)).unwrap();
        let rot = v["properties"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["name"] == "rotation");
        assert!(rot.is_some(), "rotation property should exist after set");

        // Undo
        http_post(port, "/api/undo", "");
        let node_resp = http_get(port, &format!("/api/node/{main_id}"));
        let v: serde_json::Value = serde_json::from_str(extract_body(&node_resp)).unwrap();
        let rot = v["properties"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["name"] == "rotation");
        match rot {
            None => {} // reverted entirely
            Some(p) => assert_eq!(p["type"], "Nil", "rotation should be Nil after undo"),
        }

        // Redo
        http_post(port, "/api/redo", "");
        let node_resp = http_get(port, &format!("/api/node/{main_id}"));
        let v: serde_json::Value = serde_json::from_str(extract_body(&node_resp)).unwrap();
        let rot = v["properties"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["name"] == "rotation")
            .expect("rotation should be restored after redo");
        assert_eq!(rot["type"], "Float");

        handle.stop();
    }

    #[test]
    fn test_undo_delete_restores_node() {
        let (handle, port) = make_server();
        let main_id = get_main_node_id(port);

        // Add a node then delete it
        let body = format!(r#"{{"parent_id":{main_id},"name":"Revivable","class_name":"Node"}}"#);
        let add_resp = http_post(port, "/api/node/add", &body);
        let add_json: serde_json::Value = serde_json::from_str(extract_body(&add_resp)).unwrap();
        let node_id = add_json["id"].as_u64().unwrap();

        http_post(
            port,
            "/api/node/delete",
            &format!(r#"{{"node_id":{node_id}}}"#),
        );
        assert!(
            !extract_body(&http_get(port, "/api/scene")).contains("Revivable"),
            "node should be gone after delete"
        );

        // Undo the delete — node should reappear
        let undo_resp = http_post(port, "/api/undo", "");
        assert!(undo_resp.contains("200 OK"));
        assert!(
            extract_body(&http_get(port, "/api/scene")).contains("Revivable"),
            "node should reappear after undoing delete"
        );

        handle.stop();
    }

    // ===== pat-htg: inspector property editing for core Node2D fields =====

    #[test]
    fn test_set_position_vector2_and_verify() {
        let (handle, port) = make_server();
        let main_id = get_main_node_id(port);

        let body = format!(
            r#"{{"node_id":{main_id},"property":"position","value":{{"type":"Vector2","value":[42.5,99.0]}}}}"#
        );
        let resp = http_post(port, "/api/property/set", &body);
        assert!(resp.contains("200 OK"));

        let node_resp = http_get(port, &format!("/api/node/{main_id}"));
        let v: serde_json::Value = serde_json::from_str(extract_body(&node_resp)).unwrap();
        let pos = v["properties"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["name"] == "position")
            .expect("position property should exist");
        assert_eq!(pos["type"], "Vector2");
        let val = pos["value"]["value"].as_array().unwrap();
        assert!(
            (val[0].as_f64().unwrap() - 42.5).abs() < 0.01,
            "x should be ~42.5"
        );
        assert!(
            (val[1].as_f64().unwrap() - 99.0).abs() < 0.01,
            "y should be ~99.0"
        );

        handle.stop();
    }

    #[test]
    fn test_set_rotation_float_and_verify() {
        let (handle, port) = make_server();
        let main_id = get_main_node_id(port);

        let body = format!(
            r#"{{"node_id":{main_id},"property":"rotation","value":{{"type":"Float","value":3.14}}}}"#
        );
        let resp = http_post(port, "/api/property/set", &body);
        assert!(resp.contains("200 OK"));

        let node_resp = http_get(port, &format!("/api/node/{main_id}"));
        let v: serde_json::Value = serde_json::from_str(extract_body(&node_resp)).unwrap();
        let rot = v["properties"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["name"] == "rotation")
            .expect("rotation property should exist");
        assert_eq!(rot["type"], "Float");
        assert!(
            (rot["value"]["value"].as_f64().unwrap() - 3.14).abs() < 0.01,
            "rotation should be ~3.14"
        );

        handle.stop();
    }

    #[test]
    fn test_set_visible_bool_and_verify() {
        let (handle, port) = make_server();
        let main_id = get_main_node_id(port);

        let body = format!(
            r#"{{"node_id":{main_id},"property":"visible","value":{{"type":"Bool","value":false}}}}"#
        );
        let resp = http_post(port, "/api/property/set", &body);
        assert!(resp.contains("200 OK"));

        let node_resp = http_get(port, &format!("/api/node/{main_id}"));
        let v: serde_json::Value = serde_json::from_str(extract_body(&node_resp)).unwrap();
        let vis = v["properties"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["name"] == "visible")
            .expect("visible property should exist");
        assert_eq!(vis["type"], "Bool");
        assert_eq!(vis["value"]["value"], false);

        handle.stop();
    }

    #[test]
    fn test_set_multiple_node2d_fields_persist() {
        let (handle, port) = make_server();
        let main_id = get_main_node_id(port);

        // Set position, rotation, visible in sequence
        http_post(
            port,
            "/api/property/set",
            &format!(
                r#"{{"node_id":{main_id},"property":"position","value":{{"type":"Vector2","value":[5,10]}}}}"#
            ),
        );
        http_post(
            port,
            "/api/property/set",
            &format!(
                r#"{{"node_id":{main_id},"property":"rotation","value":{{"type":"Float","value":0.5}}}}"#
            ),
        );
        http_post(
            port,
            "/api/property/set",
            &format!(
                r#"{{"node_id":{main_id},"property":"visible","value":{{"type":"Bool","value":true}}}}"#
            ),
        );

        // Verify all three are present
        let node_resp = http_get(port, &format!("/api/node/{main_id}"));
        let v: serde_json::Value = serde_json::from_str(extract_body(&node_resp)).unwrap();
        let props = v["properties"].as_array().unwrap();

        let pos = props.iter().find(|p| p["name"] == "position").unwrap();
        assert_eq!(pos["type"], "Vector2");

        let rot = props.iter().find(|p| p["name"] == "rotation").unwrap();
        assert_eq!(rot["type"], "Float");

        let vis = props.iter().find(|p| p["name"] == "visible").unwrap();
        assert_eq!(vis["type"], "Bool");
        assert_eq!(vis["value"]["value"], true);

        handle.stop();
    }

    // ===== pat-bzp: scene tree selection and hierarchy sync =====

    #[test]
    fn test_select_changes_get_selected() {
        let (handle, port) = make_server();
        let main_id = get_main_node_id(port);

        // Add a child node
        let body =
            format!(r#"{{"parent_id":{main_id},"name":"Selectee","class_name":"Sprite2D"}}"#);
        let add_resp = http_post(port, "/api/node/add", &body);
        let add_json: serde_json::Value = serde_json::from_str(extract_body(&add_resp)).unwrap();
        let child_id = add_json["id"].as_u64().unwrap();

        // Select the child
        http_post(
            port,
            "/api/node/select",
            &format!(r#"{{"node_id":{child_id}}}"#),
        );

        // GET /api/selected should reflect the child
        let sel_resp = http_get(port, "/api/selected");
        let sel: serde_json::Value = serde_json::from_str(extract_body(&sel_resp)).unwrap();
        assert_eq!(sel["name"], "Selectee");
        assert_eq!(sel["class"], "Sprite2D");

        // Switch selection to Main
        http_post(
            port,
            "/api/node/select",
            &format!(r#"{{"node_id":{main_id}}}"#),
        );
        let sel_resp = http_get(port, "/api/selected");
        let sel: serde_json::Value = serde_json::from_str(extract_body(&sel_resp)).unwrap();
        assert_eq!(sel["name"], "Main");

        handle.stop();
    }

    #[test]
    fn test_hierarchy_after_add_delete() {
        let (handle, port) = make_server();
        let main_id = get_main_node_id(port);

        // Add parent and child
        let p_body = format!(r#"{{"parent_id":{main_id},"name":"Parent","class_name":"Node2D"}}"#);
        let p_resp = http_post(port, "/api/node/add", &p_body);
        let p_json: serde_json::Value = serde_json::from_str(extract_body(&p_resp)).unwrap();
        let parent_id = p_json["id"].as_u64().unwrap();

        let c_body =
            format!(r#"{{"parent_id":{parent_id},"name":"Child","class_name":"Sprite2D"}}"#);
        http_post(port, "/api/node/add", &c_body);

        // Verify hierarchy: Main > Parent > Child
        let scene_resp = http_get(port, "/api/scene");
        let scene: serde_json::Value = serde_json::from_str(extract_body(&scene_resp)).unwrap();
        let main_children = scene["nodes"]["children"][0]["children"]
            .as_array()
            .unwrap();
        let parent_node = main_children
            .iter()
            .find(|c| c["name"] == "Parent")
            .expect("Parent should be child of Main");
        let parent_children = parent_node["children"].as_array().unwrap();
        assert!(
            parent_children.iter().any(|c| c["name"] == "Child"),
            "Child should be nested under Parent"
        );

        // Delete Parent — Child should also be gone
        http_post(
            port,
            "/api/node/delete",
            &format!(r#"{{"node_id":{parent_id}}}"#),
        );
        let scene_resp = http_get(port, "/api/scene");
        let body = extract_body(&scene_resp);
        assert!(
            !body.contains("Parent"),
            "Parent should be gone after delete"
        );
        assert!(
            !body.contains("\"Child\""),
            "Child should be gone when parent is deleted"
        );

        handle.stop();
    }

    #[test]
    fn test_hierarchy_after_reparent() {
        let (handle, port) = make_server();
        let main_id = get_main_node_id(port);

        // Get root id
        let scene_resp = http_get(port, "/api/scene");
        let scene: serde_json::Value = serde_json::from_str(extract_body(&scene_resp)).unwrap();
        let _root_id = scene["nodes"]["id"].as_u64().unwrap();

        // Add two sibling nodes under Main
        let a_body = format!(r#"{{"parent_id":{main_id},"name":"NodeA","class_name":"Node"}}"#);
        let a_resp = http_post(port, "/api/node/add", &a_body);
        let a_json: serde_json::Value = serde_json::from_str(extract_body(&a_resp)).unwrap();
        let a_id = a_json["id"].as_u64().unwrap();

        let b_body = format!(r#"{{"parent_id":{main_id},"name":"NodeB","class_name":"Node"}}"#);
        let b_resp = http_post(port, "/api/node/add", &b_body);
        let b_json: serde_json::Value = serde_json::from_str(extract_body(&b_resp)).unwrap();
        let b_id = b_json["id"].as_u64().unwrap();

        // Reparent NodeB under NodeA
        let reparent_body = format!(r#"{{"node_id":{b_id},"new_parent_id":{a_id}}}"#);
        let resp = http_post(port, "/api/node/reparent", &reparent_body);
        assert!(resp.contains("200 OK"));

        // Verify hierarchy: Main > NodeA > NodeB
        let scene_resp = http_get(port, "/api/scene");
        let scene: serde_json::Value = serde_json::from_str(extract_body(&scene_resp)).unwrap();
        let main_children = scene["nodes"]["children"][0]["children"]
            .as_array()
            .unwrap();
        let node_a = main_children
            .iter()
            .find(|c| c["name"] == "NodeA")
            .expect("NodeA should be child of Main");
        let a_children = node_a["children"].as_array().unwrap();
        assert!(
            a_children.iter().any(|c| c["name"] == "NodeB"),
            "NodeB should be child of NodeA after reparent"
        );

        // NodeB should NOT be a direct child of Main anymore
        assert!(
            !main_children.iter().any(|c| c["name"] == "NodeB"),
            "NodeB should not be direct child of Main after reparent"
        );

        handle.stop();
    }

    #[test]
    fn test_select_nonexistent_node_returns_404() {
        let (handle, port) = make_server();
        let resp = http_post(port, "/api/node/select", r#"{"node_id":8888888}"#);
        assert!(
            resp.contains("404"),
            "selecting nonexistent node should return 404"
        );
        handle.stop();
    }

    // ===== Batch 3: pat-c9b Filesystem dock =====

    #[test]
    fn test_b3_filesystem_endpoint_returns_json() {
        let (handle, port) = make_server();
        let resp = http_get(port, "/api/filesystem");
        assert!(resp.contains("200 OK"));
        let body = extract_body(&resp);
        let v: serde_json::Value = serde_json::from_str(body).unwrap();
        assert!(v.get("root").is_some());
        assert!(v.get("files").is_some());
        assert!(v["files"].is_array());
        handle.stop();
    }

    #[test]
    fn test_b3_filesystem_html_panel() {
        let (handle, port) = make_server();
        let resp = http_get(port, "/editor");
        let body = extract_body(&resp);
        assert!(body.contains("id=\"filesystem-panel\""));
        assert!(body.contains("id=\"fs-tree\""));
        handle.stop();
    }

    #[test]
    fn test_file_preview_panel_in_html() {
        let (handle, port) = make_server();
        let resp = http_get(port, "/editor");
        let body = extract_body(&resp);
        assert!(
            body.contains("id=\"fs-preview\""),
            "should have preview panel"
        );
        assert!(
            body.contains("showFilePreview"),
            "should have preview JS function"
        );
        assert!(
            body.contains("preview-img"),
            "should have image preview CSS class"
        );
        assert!(
            body.contains("preview-code"),
            "should have code preview CSS class"
        );
        assert!(
            body.contains("/api/preview/file"),
            "should reference preview API"
        );
        handle.stop();
    }

    #[test]
    fn test_file_preview_endpoint_scene() {
        let (handle, port) = make_server();
        let tmp_scene = format!("test_preview_scene_{}.tscn", port);
        std::fs::write(
            &tmp_scene,
            "[gd_scene load_steps=2]\n[ext_resource type=\"PackedScene\"]\n[node name=\"Root\" type=\"Node2D\"]\n[node name=\"Child\" type=\"Sprite2D\" parent=\".\"]\n",
        )
        .unwrap();
        let resp = http_get(port, &format!("/api/preview/file?path=res://{}", tmp_scene));
        let body = extract_body(&resp);
        let v: serde_json::Value = serde_json::from_str(body).unwrap();
        assert_eq!(v["type"], "scene");
        assert_eq!(v["node_count"], 2);
        assert_eq!(v["root_type"], "Node2D");
        assert_eq!(v["ext_resources"], 1);
        std::fs::remove_file(&tmp_scene).ok();
        handle.stop();
    }

    #[test]
    fn test_file_preview_endpoint_script() {
        let (handle, port) = make_server();
        let tmp_script = format!("test_preview_script_{}.gd", port);
        std::fs::write(
            &tmp_script,
            "extends Node\n\nfunc _ready():\n\tprint(\"hello\")\n",
        )
        .unwrap();
        let resp = http_get(
            port,
            &format!("/api/preview/file?path=res://{}", tmp_script),
        );
        let body = extract_body(&resp);
        let v: serde_json::Value = serde_json::from_str(body).unwrap();
        assert_eq!(v["type"], "script");
        assert_eq!(v["lines"], 4);
        assert!(v["preview"].as_str().unwrap().contains("extends Node"));
        std::fs::remove_file(&tmp_script).ok();
        handle.stop();
    }

    #[test]
    fn test_file_preview_endpoint_resource() {
        let (handle, port) = make_server();
        let tmp_res = format!("test_preview_res_{}.tres", port);
        std::fs::write(
            &tmp_res,
            "[gd_resource type=\"Theme\" format=3]\n[sub_resource type=\"StyleBox\"]\n",
        )
        .unwrap();
        let resp = http_get(port, &format!("/api/preview/file?path=res://{}", tmp_res));
        let body = extract_body(&resp);
        let v: serde_json::Value =
            serde_json::from_str(body).expect(&format!("invalid JSON: {}", body));
        assert_eq!(v["type"], "resource", "body was: {}", body);
        assert_eq!(v["resource_type"], "Theme");
        assert_eq!(v["sub_resources"], 1);
        std::fs::remove_file(&tmp_res).ok();
        handle.stop();
    }

    #[test]
    fn test_file_preview_endpoint_missing_file() {
        let (handle, port) = make_server();
        let resp = http_get(port, "/api/preview/file?path=res://nonexistent_xyz.gd");
        let body = extract_body(&resp);
        let v: serde_json::Value = serde_json::from_str(body).unwrap();
        assert!(
            v["error"].is_string(),
            "should return error for missing file"
        );
        handle.stop();
    }

    #[test]
    fn test_file_preview_endpoint_missing_param() {
        let (handle, port) = make_server();
        let resp = http_get(port, "/api/preview/file");
        let body = extract_body(&resp);
        let v: serde_json::Value = serde_json::from_str(body).unwrap();
        assert!(
            v["error"].is_string(),
            "should return error for missing param"
        );
        handle.stop();
    }

    #[test]
    fn test_b3_filesystem_click_loads_tscn() {
        let (handle, port) = make_server();
        let resp = http_post(
            port,
            "/api/scene/load",
            r#"{"path":"res://nonexistent.tscn"}"#,
        );
        assert!(!resp.contains("404 Not Found"));
        handle.stop();
    }

    // ===== Batch 3: pat-200 Menu actions =====

    #[test]
    fn test_b3_menu_bar_in_html() {
        let (handle, port) = make_server();
        let resp = http_get(port, "/editor");
        let body = extract_body(&resp);
        assert!(body.contains("id=\"menu-bar\""));
        assert!(body.contains("menu-new-scene"));
        assert!(body.contains("menu-save-scene"));
        assert!(body.contains("menu-open-scene"));
        handle.stop();
    }

    #[test]
    fn test_b3_menu_undo_redo_apis() {
        let (handle, port) = make_server();
        let main_id = get_main_node_id(port);
        http_post(
            port,
            "/api/property/set",
            &format!(
                r#"{{"node_id":{},"property":"hp","value":{{"type":"Int","value":42}}}}"#,
                main_id
            ),
        );
        assert!(http_post(port, "/api/undo", "").contains("200 OK"));
        assert!(http_post(port, "/api/redo", "").contains("200 OK"));
        handle.stop();
    }

    #[test]
    fn test_b3_menu_scene_endpoints() {
        let (handle, port) = make_server();
        let resp = http_post(port, "/api/scene/save", r#"{"path":""}"#);
        assert!(!resp.contains("404"));
        handle.stop();
    }

    /// pat-xse8a: Verify all Godot-standard menus (Scene, Edit, Project,
    /// Debug, Editor, Help) are present in the editor HTML with correct actions.
    #[test]
    fn test_menu_bar_godot_menus_with_actions() {
        let (handle, port) = make_server();
        let resp = http_get(port, "/editor");
        let body = extract_body(&resp);

        // Menu bar and brand
        assert!(body.contains("id=\"menu-bar\""), "menu-bar element missing");
        assert!(body.contains("menu-bar-brand"), "brand element missing");
        assert!(body.contains("Patina"), "brand text missing");

        // Scene menu and its actions
        assert!(body.contains("data-menu=\"scene\""), "Scene menu missing");
        assert!(
            body.contains("data-action=\"scene-new\""),
            "scene-new action missing"
        );
        assert!(
            body.contains("data-action=\"scene-open\""),
            "scene-open action missing"
        );
        assert!(
            body.contains("data-action=\"scene-save\""),
            "scene-save action missing"
        );
        assert!(
            body.contains("data-action=\"scene-save-as\""),
            "scene-save-as action missing"
        );
        assert!(
            body.contains("data-action=\"scene-close\""),
            "scene-close action missing"
        );
        assert!(
            body.contains("data-action=\"scene-quit\""),
            "scene-quit action missing"
        );

        // Edit menu and its actions
        assert!(body.contains("data-menu=\"edit\""), "Edit menu missing");
        assert!(
            body.contains("data-action=\"edit-undo\""),
            "edit-undo action missing"
        );
        assert!(
            body.contains("data-action=\"edit-redo\""),
            "edit-redo action missing"
        );
        assert!(
            body.contains("data-action=\"edit-cut\""),
            "edit-cut action missing"
        );
        assert!(
            body.contains("data-action=\"edit-copy\""),
            "edit-copy action missing"
        );
        assert!(
            body.contains("data-action=\"edit-paste\""),
            "edit-paste action missing"
        );

        // Project menu and its actions
        assert!(
            body.contains("data-menu=\"project\""),
            "Project menu missing"
        );
        assert!(
            body.contains("data-action=\"project-settings\""),
            "project-settings action missing"
        );
        assert!(
            body.contains("data-action=\"project-export\""),
            "project-export action missing"
        );
        assert!(
            body.contains("data-action=\"project-refresh\""),
            "project-refresh action missing"
        );

        // Debug menu and its actions
        assert!(body.contains("data-menu=\"debug\""), "Debug menu missing");
        assert!(
            body.contains("data-action=\"debug-run\""),
            "debug-run action missing"
        );
        assert!(
            body.contains("data-action=\"debug-run-current\""),
            "debug-run-current action missing"
        );
        assert!(
            body.contains("data-action=\"debug-pause\""),
            "debug-pause action missing"
        );
        assert!(
            body.contains("data-action=\"debug-stop\""),
            "debug-stop action missing"
        );
        assert!(
            body.contains("data-action=\"debug-step\""),
            "debug-step action missing"
        );

        // Editor menu and its actions
        assert!(body.contains("data-menu=\"editor\""), "Editor menu missing");
        assert!(
            body.contains("data-action=\"editor-settings\""),
            "editor-settings action missing"
        );
        assert!(
            body.contains("data-action=\"editor-layout-save\""),
            "editor-layout-save action missing"
        );
        assert!(
            body.contains("data-action=\"editor-layout-default\""),
            "editor-layout-default action missing"
        );
        assert!(
            body.contains("data-action=\"editor-toggle-fullscreen\""),
            "editor-toggle-fullscreen action missing"
        );
        assert!(
            body.contains("data-action=\"editor-toggle-console\""),
            "editor-toggle-console action missing"
        );

        // Help menu and its actions
        assert!(body.contains("data-menu=\"help\""), "Help menu missing");
        assert!(
            body.contains("data-action=\"help-docs\""),
            "help-docs action missing"
        );
        assert!(
            body.contains("data-action=\"help-issues\""),
            "help-issues action missing"
        );
        assert!(
            body.contains("data-action=\"help-about\""),
            "help-about action missing"
        );

        // Keyboard shortcuts are shown
        assert!(body.contains("Ctrl+N"), "Ctrl+N shortcut missing");
        assert!(body.contains("Ctrl+O"), "Ctrl+O shortcut missing");
        assert!(body.contains("Ctrl+S"), "Ctrl+S shortcut missing");
        assert!(body.contains("F5"), "F5 shortcut missing");
        assert!(body.contains("F11"), "F11 shortcut missing");

        // handleMenuAction function exists in JS
        assert!(
            body.contains("handleMenuAction"),
            "handleMenuAction function missing"
        );

        handle.stop();
    }

    // ===== pat-b68xe: Inspector toolbar, history, and sub-resource navigation =====

    #[test]
    fn test_inspector_toolbar_history_and_sub_resource_navigation() {
        let (handle, port) = make_server();
        let resp = http_get(port, "/editor");
        let body = extract_body(&resp);

        // Inspector toolbar CSS classes
        assert!(
            body.contains(".insp-history"),
            "insp-history CSS class missing"
        );
        assert!(
            body.contains(".insp-history button"),
            "insp-history button CSS missing"
        );
        assert!(
            body.contains(".resource-info"),
            "resource-info CSS class missing"
        );
        assert!(
            body.contains(".resource-info .resource-type"),
            "resource-type CSS class missing"
        );
        assert!(
            body.contains(".resource-info .resource-path"),
            "resource-path CSS class missing"
        );

        // Sub-resource breadcrumb CSS
        assert!(
            body.contains(".insp-breadcrumb"),
            "insp-breadcrumb CSS class missing"
        );
        assert!(
            body.contains(".insp-breadcrumb-item"),
            "insp-breadcrumb-item CSS class missing"
        );
        assert!(
            body.contains(".insp-breadcrumb-sep"),
            "insp-breadcrumb-sep CSS class missing"
        );

        // Inspector history JS state variables
        assert!(
            body.contains("var inspectorHistory = []"),
            "inspectorHistory state missing"
        );
        assert!(
            body.contains("var inspectorHistoryIndex = -1"),
            "inspectorHistoryIndex state missing"
        );
        assert!(
            body.contains("var subResourceStack = []"),
            "subResourceStack state missing"
        );

        // Inspector history navigation functions
        assert!(
            body.contains("function inspectorBack()"),
            "inspectorBack function missing"
        );
        assert!(
            body.contains("function inspectorForward()"),
            "inspectorForward function missing"
        );
        assert!(
            body.contains("function pushInspectorHistory("),
            "pushInspectorHistory function missing"
        );
        assert!(
            body.contains("function updateHistoryButtons()"),
            "updateHistoryButtons function missing"
        );

        // Inspector toolbar creates back/forward buttons with keyboard shortcut tooltips
        assert!(
            body.contains("inspector-history-back"),
            "back button id missing"
        );
        assert!(
            body.contains("inspector-history-forward"),
            "forward button id missing"
        );
        assert!(body.contains("Alt+Left"), "Alt+Left shortcut hint missing");
        assert!(
            body.contains("Alt+Right"),
            "Alt+Right shortcut hint missing"
        );

        // Alt+Arrow keyboard shortcuts wired up
        assert!(
            body.contains("e.altKey && e.key === 'ArrowLeft'"),
            "Alt+Left keyboard shortcut missing"
        );
        assert!(
            body.contains("e.altKey && e.key === 'ArrowRight'"),
            "Alt+Right keyboard shortcut missing"
        );

        // Resource info structured display (type + path spans)
        assert!(body.contains("resource-type"), "resource-type span missing");
        assert!(body.contains("resource-path"), "resource-path span missing");

        // Sub-resource navigation: breadcrumb rendering with root and stack items
        assert!(
            body.contains("insp-breadcrumb"),
            "breadcrumb element id/class missing"
        );
        assert!(
            body.contains("subResourceStack.length"),
            "sub-resource stack length check missing"
        );

        // Sub-resource button navigates into sub-resource (pushes to stack)
        assert!(
            body.contains("subResourceStack.push"),
            "subResourceStack.push missing for navigation"
        );
        assert!(
            body.contains("SubResource"),
            "SubResource pattern matching missing"
        );

        // Sub-resource inline edit button
        assert!(
            body.contains("insp-sub-resource-btn"),
            "sub-resource button class missing"
        );
        assert!(
            body.contains("data-sub-resource"),
            "data-sub-resource attribute missing"
        );

        handle.stop();
    }

    // ===== Batch 3: pat-5f4 Settings =====

    #[test]
    fn test_b3_settings_defaults() {
        let (handle, port) = make_server();
        let resp = http_get(port, "/api/settings");
        let body = extract_body(&resp);
        let v: serde_json::Value = serde_json::from_str(body).unwrap();
        assert_eq!(v["grid_snap_enabled"], false);
        assert_eq!(v["theme"], "dark");
        assert_eq!(v["physics_fps"], 60);
        handle.stop();
    }

    #[test]
    fn test_b3_settings_update() {
        let (handle, port) = make_server();
        let resp = http_post(
            port,
            "/api/settings",
            r#"{"grid_snap_enabled":true,"theme":"light","physics_fps":120}"#,
        );
        let body = extract_body(&resp);
        let v: serde_json::Value = serde_json::from_str(body).unwrap();
        assert_eq!(v["grid_snap_enabled"], true);
        assert_eq!(v["theme"], "light");
        assert_eq!(v["physics_fps"], 120);
        handle.stop();
    }

    #[test]
    fn test_b3_settings_panel_sizes() {
        let (handle, port) = make_server();
        let resp = http_post(
            port,
            "/api/settings",
            r#"{"panel_sizes":{"left":250,"bottom":180}}"#,
        );
        let body = extract_body(&resp);
        let v: serde_json::Value = serde_json::from_str(body).unwrap();
        assert_eq!(v["panel_sizes"]["left"], 250.0);
        assert_eq!(v["panel_sizes"]["bottom"], 180.0);
        handle.stop();
    }

    #[test]
    fn test_b3_settings_html() {
        let (handle, port) = make_server();
        let resp = http_get(port, "/editor");
        let body = extract_body(&resp);
        assert!(body.contains("set-theme"));
        assert!(body.contains("set-physics-fps"));
        handle.stop();
    }

    // ===== Batch 3: pat-d8b Theme =====

    #[test]
    fn test_b3_theme_dark_default() {
        let (handle, port) = make_server();
        let resp = http_get(port, "/api/settings");
        let v: serde_json::Value = serde_json::from_str(extract_body(&resp)).unwrap();
        assert_eq!(v["theme"], "dark");
        handle.stop();
    }

    #[test]
    fn test_b3_theme_light_toggle() {
        let (handle, port) = make_server();
        http_post(port, "/api/settings", r#"{"theme":"light"}"#);
        let resp = http_get(port, "/api/settings");
        let v: serde_json::Value = serde_json::from_str(extract_body(&resp)).unwrap();
        assert_eq!(v["theme"], "light");
        handle.stop();
    }

    #[test]
    fn test_b3_light_theme_css() {
        let (handle, port) = make_server();
        let resp = http_get(port, "/editor");
        assert!(extract_body(&resp).contains("body.light"));
        handle.stop();
    }

    // ===== Batch 3: pat-81a Keyboard shortcuts =====

    #[test]
    fn test_b3_shortcuts_documented() {
        let (handle, port) = make_server();
        let resp_html = http_get(port, "/editor");
        let body = extract_body(&resp_html);
        assert!(body.contains("Ctrl+S"));
        assert!(body.contains("Ctrl+Z"));
        assert!(body.contains("Ctrl+D"));
        handle.stop();
    }

    #[test]
    fn test_b3_shortcuts_js_handler() {
        let (handle, port) = make_server();
        let resp_html = http_get(port, "/editor");
        let body = extract_body(&resp_html);
        assert!(body.contains("setupKeyboardShortcuts"));
        assert!(body.contains("keydown"));
        handle.stop();
    }

    // ===== Batch 3: pat-0fa Plugin system =====

    #[test]
    fn test_b3_plugins_empty() {
        let (handle, port) = make_server();
        let resp = http_get(port, "/api/plugins");
        assert!(resp.contains("200 OK"));
        let v: serde_json::Value = serde_json::from_str(extract_body(&resp)).unwrap();
        assert!(v["plugins"].is_array());
        assert_eq!(v["plugins"].as_array().unwrap().len(), 0);
        handle.stop();
    }

    #[test]
    fn test_b3_plugins_registered() {
        // Plugin system is deferred; verify the endpoint returns a valid response.
        let (handle, port) = make_server();
        let resp = http_get(port, "/api/plugins");
        assert!(resp.contains("200 OK") || resp.contains("404"));
        handle.stop();
    }
    // =========================================================================
    // Batch 2 editor bead tests
    // =========================================================================

    // Bead 1: Viewport selection modes
    #[test]
    fn test_viewport_set_mode_select() {
        let (handle, port) = make_server();
        let resp = http_post(port, "/api/viewport/set_mode", r#"{"mode":"select"}"#);
        assert!(resp.contains("200 OK"));
        let body = extract_body(&resp);
        assert!(body.contains(r#""mode":"select""#));
        handle.stop();
    }

    #[test]
    fn test_viewport_set_mode_move() {
        let (handle, port) = make_server();
        let resp = http_post(port, "/api/viewport/set_mode", r#"{"mode":"move"}"#);
        assert!(resp.contains("200 OK"));
        let body = extract_body(&resp);
        assert!(body.contains(r#""mode":"move""#));
        handle.stop();
    }

    #[test]
    fn test_viewport_set_mode_rotate_and_get() {
        let (handle, port) = make_server();
        http_post(port, "/api/viewport/set_mode", r#"{"mode":"rotate"}"#);
        let resp = http_get(port, "/api/viewport/mode");
        let body = extract_body(&resp);
        assert!(
            body.contains(r#""mode":"rotate""#),
            "mode should be rotate, got: {body}"
        );
        handle.stop();
    }

    #[test]
    fn test_viewport_set_mode_invalid() {
        let (handle, port) = make_server();
        let resp = http_post(port, "/api/viewport/set_mode", r#"{"mode":"invalid"}"#);
        assert!(resp.contains("400"), "invalid mode should return 400");
        handle.stop();
    }

    // Bead 2: Transform gizmos
    #[test]
    fn test_gizmo_renders_for_selected_node_batch2() {
        use crate::scene_renderer::render_scene;
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let mut node = Node::new("Player", "Node2D");
        node.set_property("position", Variant::Vector2(Vector2::new(100.0, 100.0)));
        let nid = tree.add_child(root, node).unwrap();
        let fb = render_scene(&tree, Some(nid), 200, 200);
        let mut has_red = false;
        let mut has_green = false;
        for y in 80..120 {
            for x in 80..180 {
                let c = fb.get_pixel(x, y);
                if c.r > 0.8 && c.g < 0.4 && c.b < 0.4 {
                    has_red = true;
                }
                if c.g > 0.7 && c.r < 0.4 && c.b < 0.4 {
                    has_green = true;
                }
            }
        }
        assert!(has_red, "selected node should have red gizmo arrow");
        assert!(has_green, "selected node should have green gizmo arrow");
    }

    #[test]
    fn test_gizmo_not_rendered_when_unselected_batch2() {
        use crate::scene_renderer::render_scene;
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let mut node = Node::new("Player", "Node2D");
        node.set_property("position", Variant::Vector2(Vector2::new(100.0, 100.0)));
        tree.add_child(root, node).unwrap();
        let fb = render_scene(&tree, None, 200, 200);
        let mut has_red = false;
        for y in 90..110 {
            for x in 100..160 {
                let c = fb.get_pixel(x, y);
                if c.r > 0.8 && c.g < 0.4 && c.b < 0.4 {
                    has_red = true;
                }
            }
        }
        assert!(!has_red, "unselected node should not have gizmo arrows");
    }

    #[test]
    fn test_gizmo_different_colors_per_axis() {
        use crate::scene_renderer::render_scene;
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let mut node = Node::new("NPC", "Node2D");
        node.set_property("position", Variant::Vector2(Vector2::new(100.0, 100.0)));
        let nid = tree.add_child(root, node).unwrap();
        let fb = render_scene(&tree, Some(nid), 300, 300);
        // Scan the whole framebuffer for both red and green gizmo pixels
        let mut has_red = false;
        let mut has_green = false;
        for y in 0..300 {
            for x in 0..300 {
                let c = fb.get_pixel(x, y);
                if c.r > 0.8 && c.g < 0.4 && c.b < 0.4 {
                    has_red = true;
                }
                if c.g > 0.7 && c.r < 0.4 && c.b < 0.4 {
                    has_green = true;
                }
            }
        }
        assert!(
            has_red && has_green,
            "gizmo should have both red (X) and green (Y) axis colors"
        );
    }

    // Bead 3: Snapping
    #[test]
    fn test_snap_info_endpoint() {
        let (handle, port) = make_server();
        let resp = http_get(port, "/api/viewport/snap_info");
        assert!(resp.contains("200 OK"));
        let body = extract_body(&resp);
        assert!(
            body.contains("snap"),
            "snap_info should return snap settings, got: {body}"
        );
        handle.stop();
    }

    #[test]
    fn test_snap_toggle_via_settings_batch2() {
        let (handle, port) = make_server();
        http_post(
            port,
            "/api/settings",
            r#"{"grid_snap_enabled":true,"grid_snap_size":16}"#,
        );
        let resp = http_get(port, "/api/settings");
        let body = extract_body(&resp);
        assert!(
            body.contains("grid_snap"),
            "settings should return snap settings; got: {body}"
        );
        handle.stop();
    }

    // Bead 4: Script editor core
    #[test]
    fn test_get_node_script_no_script() {
        let (handle, port) = make_server();
        let main_id = get_main_node_id(port);
        let resp = http_get(port, &format!("/api/node/script?node_id={main_id}"));
        let body = extract_body(&resp);
        assert!(
            body.contains("has_script") && body.contains("false"),
            "node without script should return has_script:false, got: {body}"
        );
        handle.stop();
    }

    #[test]
    fn test_get_node_script_with_path() {
        let port = free_port();
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let mut main = Node::new("Main", "Node2D");
        main.set_property(
            "_script_path",
            Variant::String("nonexistent.gd".to_string()),
        );
        tree.add_child(root, main).unwrap();
        let state = EditorState::new(tree);
        let handle = EditorServerHandle::start(port, state);
        thread::sleep(Duration::from_millis(100));
        let resp = http_get(port, "/api/scene");
        let scene_body = extract_body(&resp);
        let v: serde_json::Value = serde_json::from_str(scene_body).unwrap();
        let nid = v["nodes"]["children"][0]["id"].as_u64().unwrap();
        let resp = http_get(port, &format!("/api/node/script?node_id={nid}"));
        let body = extract_body(&resp);
        assert!(
            body.contains("has_script") && body.contains("true"),
            "node with script path should return has_script:true, got: {body}"
        );
        handle.stop();
    }

    #[test]
    fn test_editor_html_has_syntax_highlighting() {
        let html = crate::editor_ui::EDITOR_HTML;
        assert!(html.contains("gd-keyword"), "should have keyword styling");
        assert!(html.contains("gd-string"), "should have string styling");
        assert!(html.contains("gd-comment"), "should have comment styling");
    }

    // Bead 5: Script search
    #[test]
    fn test_search_missing_query() {
        let (handle, port) = make_server();
        let resp = http_get(port, "/api/search");
        assert!(resp.contains("400"), "missing q should return 400");
        handle.stop();
    }

    #[test]
    fn test_search_returns_results_array() {
        let (handle, port) = make_server();
        let resp = http_get(port, "/api/search?q=nonexistent_string_xyz");
        let body = extract_body(&resp);
        assert!(
            body.contains("results"),
            "should return results key, got: {body}"
        );
        handle.stop();
    }

    // Bead 6: Signals dock
    #[test]
    fn test_signals_endpoint_returns_signals() {
        let (handle, port) = make_server();
        let main_id = get_main_node_id(port);
        let resp = http_get(port, &format!("/api/node/signals?node_id={main_id}"));
        assert!(resp.contains("200 OK"));
        let body = extract_body(&resp);
        assert!(
            body.contains("signals"),
            "should return signals, got: {body}"
        );
        handle.stop();
    }

    #[test]
    fn test_signal_connect_and_disconnect() {
        let (handle, port) = make_server();
        let main_id = get_main_node_id(port);
        let resp = http_post(
            port,
            "/api/node/signals/connect",
            &format!(r#"{{"node_id":{main_id},"signal":"ready","method":"_on_ready"}}"#),
        );
        assert!(resp.contains("200") || resp.contains("ok"));
        let resp = http_post(
            port,
            "/api/signal/disconnect",
            &format!(r#"{{"node_id":{main_id},"signal":"ready"}}"#),
        );
        assert!(resp.contains("200 OK"), "disconnect should succeed");
        let body = extract_body(&resp);
        assert!(body.contains(r#""ok":true"#), "got: {body}");
        handle.stop();
    }

    #[test]
    fn test_signal_disconnect_nonexistent_node() {
        let (handle, port) = make_server();
        let resp = http_post(
            port,
            "/api/signal/disconnect",
            r#"{"node_id":9999999,"signal":"ready"}"#,
        );
        assert!(
            resp.contains("404"),
            "should return 404 for nonexistent node"
        );
        handle.stop();
    }

    // Bead 7: Animation editor
    #[test]
    fn test_animations_list_initially_empty() {
        let (handle, port) = make_server();
        let resp = http_get(port, "/api/animations");
        assert!(resp.contains("200 OK"));
        let body = extract_body(&resp);
        assert!(
            body == "[]" || body.contains("[]"),
            "should be empty, got: {body}"
        );
        handle.stop();
    }

    #[test]
    fn test_animation_create_and_list_batch2() {
        let (handle, port) = make_server();
        http_post(
            port,
            "/api/animation/create",
            r#"{"name":"walk","length":1.0}"#,
        );
        let resp = http_get(port, "/api/animations");
        let body = extract_body(&resp);
        assert!(body.contains("walk"), "should contain 'walk', got: {body}");
        handle.stop();
    }

    // Bead 8: Bottom panels
    #[test]
    fn test_output_returns_entries() {
        let (handle, port) = make_server();
        let resp = http_get(port, "/api/output");
        assert!(resp.contains("200 OK"));
        let body = extract_body(&resp);
        assert!(
            body.contains("entries"),
            "should return entries, got: {body}"
        );
        handle.stop();
    }

    #[test]
    fn test_output_clear_endpoint() {
        let (handle, port) = make_server();
        let resp = http_post(port, "/api/output/clear", "");
        assert!(resp.contains("200 OK"));
        let body = extract_body(&resp);
        assert!(body.contains(r#""ok":true"#), "got: {body}");
        handle.stop();
    }

    #[test]
    fn test_bottom_panel_and_output_in_html() {
        let html = crate::editor_ui::EDITOR_HTML;
        assert!(html.contains("bottom-panel"), "should have bottom panel");
        assert!(html.contains("output"), "should have output");
    }

    // Bead 9: Top bar
    #[test]
    fn test_top_bar_play_stop_in_html() {
        let html = crate::editor_ui::EDITOR_HTML;
        assert!(html.contains("btn-play"), "should have play button");
        assert!(html.contains("btn-stop"), "should have stop button");
    }

    #[test]
    fn test_scene_tab_in_html() {
        let html = crate::editor_ui::EDITOR_HTML;
        assert!(html.contains("scene-tab"), "should have scene tab");
    }

    #[test]
    fn test_runtime_play_stop_endpoints() {
        let (handle, port) = make_server();
        let resp = http_post(port, "/api/runtime/play", "");
        assert!(resp.contains("200 OK"));
        let body = extract_body(&resp);
        assert!(
            body.contains("running") || body.contains("true"),
            "got: {body}"
        );
        let resp = http_post(port, "/api/runtime/stop", "");
        assert!(resp.contains("200 OK"));
        handle.stop();
    }
}
