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
use gdscene::node::NodeId;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_saver::TscnSaver;
use gdscene::SceneTree;
use gdvariant::serialize::{from_json, to_json};
use gdvariant::Variant;

use crate::EditorCommand;

use gdcore::math::Vector2;

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
    let path = parts[1].split('?').next().unwrap_or(parts[1]).to_string();

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

    Some(HttpRequest { method, path, body })
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
}
