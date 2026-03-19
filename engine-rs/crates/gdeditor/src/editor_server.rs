//! HTTP REST API server for the Patina editor.
//!
//! Exposes the editor's scene tree, node manipulation, undo/redo,
//! viewport rendering, and scene save/load over a simple REST API.
//! Uses the same `std::net::TcpListener` pattern as `gdrender2d::frame_server`.

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

use gdscene::animation::{Animation, AnimationTrack, KeyFrame, LoopMode};
use gdscene::node::{Node, NodeId};
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_saver::TscnSaver;
use gdscene::SceneTree;
use gdvariant::serialize::{from_json, to_json};
use gdvariant::Variant;

use crate::texture_cache::TextureCache;
use crate::EditorCommand;

use gdcore::math::Vector2;

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

/// Editor display settings that can be persisted.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct EditorDisplaySettings {
    pub grid_snap_enabled: bool,
    pub grid_snap_size: u32,
    pub grid_visible: bool,
    pub rulers_visible: bool,
    pub background_color: [f64; 4],
    pub font_size: String,
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
        }
    }
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
    /// A separate copy of the scene tree for running.
    pub run_scene_tree: Option<SceneTree>,
    /// Counts frames during runtime.
    pub runtime_frame_count: u64,
    /// Time between frames (fixed at 1/60).
    pub delta_time: f64,
    /// Named animations stored in the editor.
    pub animations: std::collections::HashMap<String, Animation>,
    /// Current animation playback state.
    pub animation_playback: AnimationPlaybackState,
}

// SAFETY: EditorState is only accessed through a Mutex, so concurrent
// access is serialized. SceneTree contains `Box<dyn ScriptInstance>` which
// is not Send, but we never move script instances across threads — all
// access goes through the Mutex guard on the server thread.
unsafe impl Send for EditorState {}

impl EditorState {
    /// Creates a new editor state with the given scene tree.
    pub fn new(tree: SceneTree) -> Self {
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
            run_scene_tree: None,
            runtime_frame_count: 0,
            delta_time: 1.0 / 60.0,
            animations: HashMap::new(),
            animation_playback: AnimationPlaybackState {
                playing: false,
                animation_name: None,
                current_time: 0.0,
                recording: false,
            },
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
        ("POST", "/api/viewport/box_select") => api_box_select(state, &req.body, &mut stream),
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
        ("POST", "/api/runtime/play") => api_runtime_play(state, &mut stream),
        ("POST", "/api/runtime/stop") => api_runtime_stop(state, &mut stream),
        ("POST", "/api/runtime/pause") => api_runtime_pause(state, &mut stream),
        ("POST", "/api/runtime/step") => api_runtime_step(state, &mut stream),
        ("GET", "/api/runtime/status") => api_runtime_status(state, &mut stream),
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

    serde_json::json!({
        "id": node_id.raw(),
        "name": node.name(),
        "class": node.class_name(),
        "path": path,
        "visible": visible,
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

/// `POST /api/node/reorder` — reorders a node within its parent's children.
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
    let new_pos = drag.start_node_pos + world_delta;

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
    let new_pos = drag.start_node_pos + world_delta;

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
                r#"{{"name":"{}","path":"{}","is_dir":false}}"#,
                self.name.replace('\\', "\\\\").replace('"', "\\\""),
                self.path.replace('\\', "\\\\").replace('"', "\\\""),
            )
        }
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
                });
            }
        } else if path.is_file() {
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
            if matches!(ext, "tscn" | "gd" | "tres") {
                let file_path = if prefix.is_empty() {
                    format!("res://{}", name)
                } else {
                    format!("res://{}/{}", prefix, name)
                };
                files.push(FsEntry {
                    name,
                    path: file_path,
                    is_dir: false,
                    children: Vec::new(),
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
                let connected = connections_str.contains(sig);
                format!(r#"{{"name":"{}","connected":{}}}"#, sig, connected,)
            })
            .collect();

        let groups_json: Vec<String> = groups
            .iter()
            .map(|g| format!(r#""{}""#, g.replace('\\', "\\\\").replace('"', "\\\"")))
            .collect();

        format!(
            r#"{{"node_id":{},"class":"{}","signals":[{}],"groups":[{}]}}"#,
            node_raw,
            class_name.replace('"', "\\\""),
            signals_json.join(","),
            groups_json.join(","),
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
                        format!(
                            r#"{{"time":{},"value":{}}}"#,
                            kf.time,
                            variant_to_simple_json(&kf.value)
                        )
                    })
                    .collect();
                format!(
                    r#"{{"index":{},"node_path":"{}","property":"{}","keyframes":[{}]}}"#,
                    idx,
                    track.node_path.replace('"', "\\\""),
                    track.property_path.replace('"', "\\\""),
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
    let mut state = state.lock().unwrap();
    let anim = match state.animations.get_mut(&anim_name) {
        Some(a) => a,
        None => {
            send_error(stream, 404, "animation not found");
            return;
        }
    };
    let track_idx = anim
        .tracks
        .iter()
        .position(|t| t.node_path == node_path && t.property_path == property);
    let track_idx = match track_idx {
        Some(idx) => idx,
        None => {
            let track = AnimationTrack::with_node(&node_path, &property);
            anim.tracks.push(track);
            anim.tracks.len() - 1
        }
    };
    anim.tracks[track_idx].add_keyframe(KeyFrame::linear(time, value));
    send_json(
        stream,
        &format!(r#"{{"ok":true,"track_index":{}}}"#, track_idx),
    );
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
        format!(
            r#"{{"playing":{},"current_time":{},"animation_name":{},"recording":{}}}"#,
            pb.playing, pb.current_time, name_json, pb.recording
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
    let cloned = clone_scene_tree(&state.scene_tree);
    state.run_scene_tree = Some(cloned);
    state.is_running = true;
    state.is_paused = false;
    state.runtime_frame_count = 0;
    state.add_log("info", "Runtime: play started");
    send_json(stream, r#"{"ok":true,"running":true}"#);
}

fn api_runtime_stop(state: &Arc<Mutex<EditorState>>, stream: &mut TcpStream) {
    let mut state = state.lock().unwrap();
    state.is_running = false;
    state.is_paused = false;
    state.run_scene_tree = None;
    state.runtime_frame_count = 0;
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
    if let Some(ref mut tree) = state.run_scene_tree {
        let ids = tree.all_nodes_in_tree_order();
        for id in &ids {
            let velocity = {
                let node = match tree.get_node(*id) {
                    Some(n) => n,
                    None => continue,
                };
                match node.get_property("velocity") {
                    Variant::Vector2(v) => Some(v),
                    _ => None,
                }
            };
            if let Some(vel) = velocity {
                let new_pos = {
                    let node = tree.get_node(*id).unwrap();
                    match node.get_property("position") {
                        Variant::Vector2(pos) => Variant::Vector2(gdcore::math::Vector2::new(
                            pos.x + vel.x * delta as f32,
                            pos.y + vel.y * delta as f32,
                        )),
                        _ => continue,
                    }
                };
                if let Some(node) = tree.get_node_mut(*id) {
                    node.set_property("position", new_pos);
                }
            }
        }
        tree.process_animations(delta);
        tree.process_tweens(delta);
        tree.process_all_scripts_process(delta);
        tree.process_frame();
    }
    state.runtime_frame_count += 1;
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

    fn http_request_str(port: u16, request: &str) -> String {
        let mut stream =
            TcpStream::connect(format!("127.0.0.1:{port}")).expect("failed to connect");
        stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
        stream.write_all(request.as_bytes()).unwrap();
        let mut response = Vec::new();
        let _ = stream.read_to_end(&mut response);
        String::from_utf8_lossy(&response).to_string()
    }

    fn http_request_raw(port: u16, request: &str) -> Vec<u8> {
        let mut stream =
            TcpStream::connect(format!("127.0.0.1:{port}")).expect("failed to connect");
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
    fn test_fs_entry_to_json() {
        let entry = super::FsEntry {
            name: "test.tscn".to_string(),
            path: "res://test.tscn".to_string(),
            is_dir: false,
            children: Vec::new(),
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
            }],
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
}
