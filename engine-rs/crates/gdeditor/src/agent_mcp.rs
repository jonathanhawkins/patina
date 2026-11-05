//! The agent interface: an MCP (Model Context Protocol) server on the editor.
//!
//! `POST /mcp` speaks JSON-RPC 2.0 — MCP's "Streamable HTTP" transport in its
//! JSON-response form — on the same port as the browser editor. An AI client
//! and a human therefore share one live session: an edit made through a tool
//! shows up in the open editor, and a game the human plays in the browser
//! leaves its output in the same log the agent reads.
//!
//! The tools are shaped for an agent's loop rather than for clicks:
//!
//! * nodes are addressed by **path** and values written in **Godot literal
//!   syntax** (`Vector2(10, 20)`), exactly as a `.tscn` spells them;
//! * one call is one experiment — `play` boots the scene paused, steps N frames
//!   against a scripted input timeline, probes state, captures frames, and
//!   reports what the scripts printed and which calls failed, frame by frame;
//! * results are compact text, not JSON dumps, because every byte returned is
//!   context the agent pays for.
//!
//! Mutations go through this server's own `/api` routes over loopback, so they
//! keep each route's validation, undo entry and edit-lock rule, and the browser
//! refreshes exactly as if a human had made them. Reads, frame stepping and
//! rendering run in-process under the state lock, released between frames.

use std::collections::VecDeque;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use gdscene::node::NodeId;
use gdscene::SceneTree;
use gdvariant::Variant;
use serde_json::{json, Value};

use crate::editor_server::{EditorState, ViewportCache};
use gdcore::content_scale::{ContentScaleAspect, ContentScaleConfig, ContentScaleMode};

/// The protocol revision this server implements; an `initialize` asking for
/// another supported revision gets that one echoed back.
pub const PROTOCOL_VERSION: &str = "2025-06-18";
const SUPPORTED_PROTOCOLS: &[&str] = &["2025-06-18", "2025-03-26", "2024-11-05"];

/// Entries [`GameLog`] keeps before dropping the oldest.
const GAME_LOG_CAP: usize = 5000;
/// Longest single `play`/`step` call: ten minutes of game time at 60 fps.
const MAX_FRAMES_PER_CALL: u64 = 36_000;
/// Frames one call may capture; each is an image the client must ingest.
const MAX_SCREENSHOTS: usize = 8;
/// Most game-log lines a `play`/`step` report quotes before summarising.
const REPORT_LOG_LINES: usize = 200;
/// Nodes a `tree` listing prints before eliding the rest, unless the call
/// passes its own `budget`.
const TREE_NODE_BUDGET: usize = 400;
/// The most nodes one `tree` call may ask for.
const TREE_NODE_BUDGET_MAX: usize = 100_000;
/// Characters of property text one tree line carries.
const TREE_PROPS_CHARS: usize = 240;

const INSTRUCTIONS: &str = "Patina is a Rust reimplementation of Godot 4.6: projects, .tscn scenes and \
GDScript work as in Godot. Loop: `status` to orient → `write_script` / `edit` / `scene` to build → \
`play` with an input timeline, probes and screenshots to test → fix → repeat. Paths are relative to \
the open scene's root (`Player/Sprite`, `.` = root); `/root/...` is absolute. Values use Godot \
literal syntax (`Vector2(10, 20)`, `Color(1, 0, 0)`, `true`); a sub-resource is \
`{\"new\": \"RectangleShape2D\", \"size\": \"Vector2(32, 32)\"}`. `play` runs headless and \
frame-exact, faster than real time: frames are numbered from 1 within each call, a print is tagged \
with the frame that produced it, and failed script calls are reported as errors. Edits are refused \
while the game runs — `stop` first. Save with `scene {action: save}`.";

// ---------------------------------------------------------------------------
// Per-process key for loopback calls
// ---------------------------------------------------------------------------

/// The key this process stamps on its own loopback `/api` calls
/// (`X-Patina-Agent`). Random per process, so no outside client can claim the
/// rate-limit exemption it grants.
pub fn agent_key() -> &'static str {
    static KEY: OnceLock<String> = OnceLock::new();
    KEY.get_or_init(|| {
        use std::hash::{BuildHasher, Hasher};
        let mut a = std::collections::hash_map::RandomState::new().build_hasher();
        a.write_u128(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos(),
        );
        a.write_u32(std::process::id());
        let first = a.finish();
        let mut b = std::collections::hash_map::RandomState::new().build_hasher();
        b.write_u64(first);
        format!("{first:016x}{:016x}", b.finish())
    })
}

// ---------------------------------------------------------------------------
// Game log
// ---------------------------------------------------------------------------

/// What a [`GameLogEntry`] records.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GameLogKind {
    /// A `print` family line.
    Print,
    /// `push_error`/`push_warning`/`printerr`, or a script call that failed.
    Error,
}

/// One line of a running game's output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameLogEntry {
    /// Monotonic across runs, so a client can resume with `since`.
    pub seq: u64,
    /// The game frame that produced the line (0 = during `_ready` at boot).
    pub frame: u64,
    pub kind: GameLogKind,
    pub text: String,
}

/// The running game's prints and errors, frame-tagged — see
/// [`EditorState::game_log`].
#[derive(Debug, Default)]
pub struct GameLog {
    entries: VecDeque<GameLogEntry>,
    next_seq: u64,
}

impl GameLog {
    pub fn push(&mut self, frame: u64, kind: GameLogKind, text: String) {
        if self.entries.len() >= GAME_LOG_CAP {
            self.entries.pop_front();
        }
        self.entries.push_back(GameLogEntry {
            seq: self.next_seq,
            frame,
            kind,
            text,
        });
        self.next_seq += 1;
    }

    /// Forgets the previous run's lines. The sequence keeps counting, so a
    /// `since` taken before the restart stays meaningful.
    pub fn start_run(&mut self) {
        self.entries.clear();
    }

    /// The sequence number the next line will get.
    pub fn next_seq(&self) -> u64 {
        self.next_seq
    }

    /// Lines with `seq >= since`, oldest first.
    pub fn since(&self, since: u64) -> impl Iterator<Item = &GameLogEntry> {
        self.entries.iter().filter(move |e| e.seq >= since)
    }
}

// ---------------------------------------------------------------------------
// Transport
// ---------------------------------------------------------------------------

/// Answers `GET`/`DELETE /mcp`. The transport allows a server to offer no
/// server-initiated stream and no session teardown, which is this server.
pub fn reject_non_post(stream: &mut TcpStream) {
    write_http(
        stream,
        "405 Method Not Allowed",
        Some("application/json"),
        &json!({"error": "POST JSON-RPC requests to /mcp"}).to_string(),
        &[("Allow", "POST")],
    );
}

/// Whether a request's `Origin` names this machine. The MCP transport requires
/// servers to validate `Origin`: a browser on any web page can POST a
/// "simple" (text/plain, preflight-free) request to `127.0.0.1`, and with no
/// editor token set nothing else stops that page from writing scripts into the
/// project and running them. DNS rebinding is covered too — a rebound host
/// still sends its own name as the Origin.
pub fn origin_is_local(origin: &str) -> bool {
    let Some(rest) = origin
        .strip_prefix("http://")
        .or_else(|| origin.strip_prefix("https://"))
    else {
        return false;
    };
    let host = if rest.starts_with('[') {
        rest.split_inclusive(']').next().unwrap_or("")
    } else {
        rest.split([':', '/']).next().unwrap_or("")
    };
    matches!(host, "localhost" | "127.0.0.1" | "[::1]")
}

/// `POST /mcp` — one JSON-RPC message in, one response out.
pub fn handle_mcp(
    state: &Arc<Mutex<EditorState>>,
    cache: &Arc<ViewportCache>,
    body: &str,
    stream: &mut TcpStream,
    auth_token: Option<&str>,
    origin: Option<&str>,
) {
    // Non-browser clients (Claude Code, the CLI shim) send no Origin at all.
    if let Some(origin) = origin.filter(|o| !origin_is_local(o)) {
        write_http(
            stream,
            "403 Forbidden",
            Some("application/json"),
            &json!({"error": format!("cross-origin request from {origin} refused")}).to_string(),
            &[],
        );
        return;
    }
    let message: Value = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(e) => {
            write_rpc(
                stream,
                &rpc_error(Value::Null, -32700, &format!("parse error: {e}")),
            );
            return;
        }
    };
    if message.is_array() {
        write_rpc(
            stream,
            &rpc_error(Value::Null, -32600, "JSON-RPC batches are not supported"),
        );
        return;
    }
    // A notification carries no id and gets no response body.
    let Some(id) = message.get("id").cloned() else {
        write_http(stream, "202 Accepted", None, "", &[]);
        return;
    };
    let method = message.get("method").and_then(Value::as_str).unwrap_or("");
    let params = message.get("params").cloned().unwrap_or_else(|| json!({}));
    let ctx = Ctx {
        state: Arc::clone(state),
        cache: Arc::clone(cache),
        api: Api {
            port: stream.local_addr().map(|a| a.port()).unwrap_or(0),
            auth: auth_token.map(str::to_string),
        },
    };
    let response = match method {
        "initialize" => rpc_result(id, initialize_result(&params)),
        "ping" => rpc_result(id, json!({})),
        "tools/list" => rpc_result(id, json!({ "tools": tool_definitions() })),
        "tools/call" => rpc_result(id, call_tool(&ctx, &params)),
        other => rpc_error(id, -32601, &format!("method not found: {other}")),
    };
    write_rpc(stream, &response);
}

fn initialize_result(params: &Value) -> Value {
    let asked = params
        .get("protocolVersion")
        .and_then(Value::as_str)
        .unwrap_or(PROTOCOL_VERSION);
    let version = if SUPPORTED_PROTOCOLS.contains(&asked) {
        asked
    } else {
        PROTOCOL_VERSION
    };
    json!({
        "protocolVersion": version,
        "capabilities": { "tools": { "listChanged": false } },
        "serverInfo": { "name": "patina-editor", "version": env!("CARGO_PKG_VERSION") },
        "instructions": INSTRUCTIONS,
    })
}

fn rpc_result(id: Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

fn rpc_error(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

fn write_rpc(stream: &mut TcpStream, value: &Value) {
    write_http(
        stream,
        "200 OK",
        Some("application/json"),
        &value.to_string(),
        &[],
    );
}

fn write_http(
    stream: &mut TcpStream,
    status: &str,
    content_type: Option<&str>,
    body: &str,
    extra: &[(&str, &str)],
) {
    let mut head = format!("HTTP/1.1 {status}\r\n");
    if let Some(ct) = content_type {
        head.push_str(&format!("Content-Type: {ct}\r\n"));
    }
    for (name, value) in extra {
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    head.push_str(&format!(
        "Content-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    ));
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(body.as_bytes());
}

// ---------------------------------------------------------------------------
// Loopback client for this server's own /api routes
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct Ctx {
    state: Arc<Mutex<EditorState>>,
    /// The frames published to the browser, and who is pulling them.
    cache: Arc<ViewportCache>,
    api: Api,
}

#[derive(Clone)]
struct Api {
    port: u16,
    auth: Option<String>,
}

impl Api {
    /// One request to `/api` on this server; returns the status and raw body.
    fn request(
        &self,
        method: &str,
        path: &str,
        body: Option<&Value>,
    ) -> Result<(u16, Vec<u8>), String> {
        let body = body.map(Value::to_string).unwrap_or_default();
        let mut stream = TcpStream::connect(("127.0.0.1", self.port))
            .map_err(|e| format!("loopback connect to :{} failed: {e}", self.port))?;
        let _ = stream.set_read_timeout(Some(Duration::from_secs(120)));
        let mut head = format!(
            "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\n\
             Content-Length: {}\r\nConnection: close\r\nX-Patina-Agent: {}\r\n",
            body.len(),
            agent_key()
        );
        if let Some(token) = &self.auth {
            head.push_str(&format!("Authorization: Bearer {token}\r\n"));
        }
        head.push_str("\r\n");
        stream
            .write_all(head.as_bytes())
            .and_then(|_| stream.write_all(body.as_bytes()))
            .map_err(|e| format!("loopback write failed: {e}"))?;
        let mut raw = Vec::new();
        stream
            .read_to_end(&mut raw)
            .map_err(|e| format!("loopback read failed: {e}"))?;
        let split = raw
            .windows(4)
            .position(|w| w == b"\r\n\r\n")
            .ok_or_else(|| "loopback response had no header terminator".to_string())?;
        let status = std::str::from_utf8(&raw[..split])
            .ok()
            .and_then(|h| h.split_whitespace().nth(1))
            .and_then(|code| code.parse::<u16>().ok())
            .ok_or_else(|| "loopback response had no status code".to_string())?;
        Ok((status, raw[split + 4..].to_vec()))
    }

    /// A JSON call that must answer 200; anything else becomes the route's own
    /// error message.
    fn call(&self, method: &str, path: &str, body: Option<Value>) -> Result<Value, String> {
        let (status, bytes) = self.request(method, path, body.as_ref())?;
        let text = String::from_utf8_lossy(&bytes);
        let parsed: Option<Value> = serde_json::from_str(&text).ok();
        if status != 200 {
            return Err(format!(
                "{method} {path} → {status}: {}",
                error_message(parsed.as_ref(), &text)
            ));
        }
        Ok(parsed.unwrap_or(Value::Null))
    }

    fn post(&self, path: &str, body: Value) -> Result<Value, String> {
        self.call("POST", path, Some(body))
    }
}

/// The message out of either error envelope the routes use
/// (`{"error":{"message":..}}` or `{"error":".."}`), else the raw body.
fn error_message(parsed: Option<&Value>, raw: &str) -> String {
    if let Some(v) = parsed {
        if let Some(m) = v.pointer("/error/message").and_then(Value::as_str) {
            return m.to_string();
        }
        if let Some(m) = v.get("error").and_then(Value::as_str) {
            return m.to_string();
        }
    }
    raw.chars().take(500).collect()
}

// ---------------------------------------------------------------------------
// Tool table
// ---------------------------------------------------------------------------

fn tool_definitions() -> Vec<Value> {
    let input_item = json!({
        "type": "object",
        "description": "One input on the timeline. Exactly one of `action` (an InputMap action, e.g. \"jump\"), `key` (a key name, e.g. \"Space\", \"A\", \"Escape\") or `click` ([x, y] in the game's own coordinates — its base resolution, as probes report positions; presses the left mouse button there). Timing: `at` = the frame it starts (default 1); `frames` = how many frames it is held (default 1), or `to` = last held frame (inclusive).",
        "properties": {
            "action": {"type": "string"}, "key": {"type": "string"},
            "click": {"type": "array", "items": {"type": "number"}},
            "at": {"type": "integer"}, "frames": {"type": "integer"}, "to": {"type": "integer"}
        }
    });
    let probe_item = json!({
        "type": "object",
        "description": "Read node properties (engine properties or script variables) after frame `at` (0 = before the first frame), or after every `every`-th frame.",
        "properties": {
            "node": {"type": "string"}, "props": {"type": "array", "items": {"type": "string"}},
            "at": {"type": "integer"}, "every": {"type": "integer"}
        },
        "required": ["node", "props"]
    });
    let timeline = |what: &str| {
        json!({
            "type": "object",
            "properties": {
                "frames": {"type": "integer", "description": format!("Frames to {what} (default 60 = 1 s at 60 fps).")},
                "inputs": {"type": "array", "items": input_item.clone()},
                "probes": {"type": "array", "items": probe_item.clone()},
                "screenshots": {"type": "array", "items": {"type": "integer"}, "description": "Frames to capture (0 = before stepping). At most 8."},
                "width": {"type": "integer"}, "height": {"type": "integer"},
                "keep_running": {"type": "boolean", "description": "Leave the game paused at the last frame instead of stopping it (play only; continue with `step`)."},
                "audio": {"type": "boolean", "description": "Open the real audio device for this run (play only), so `audio` can measure what it sounds like. Off by default."}
            }
        })
    };
    vec![
        tool("status", "Orientation: project, open scene, game state, input actions, autoloads, project files and the most recent game errors.", json!({"type": "object", "properties": {}})),
        tool("tree", "The open scene (or, with running=true, the live game tree) as an indented outline: name (Class) [script] <instanced scene> {groups} and every non-default property in Godot syntax.",
            json!({"type": "object", "properties": {
                "node": {"type": "string", "description": "Subtree to list (default: scene root)."},
                "running": {"type": "boolean"}, "depth": {"type": "integer"},
                "budget": {"type": "integer", "description": "Most nodes to print (default 400, max 100000). The listing ends by saying how many it omitted."},
                "props": {"type": "boolean", "description": "Include property values (default true)."}
            }})),
        tool("inspect", "One node in detail: non-default properties, script variables with live values, groups and children. `all` adds default-valued properties.",
            json!({"type": "object", "properties": {
                "node": {"type": "string"}, "running": {"type": "boolean"}, "all": {"type": "boolean"}
            }, "required": ["node"]})),
        tool("edit", "Apply a batch of scene edits in order, each through the editor's own undoable route; returns the resulting outline. Ops: {op:\"add\", type, name, parent?, props?} · {op:\"set\", node, props} · {op:\"delete\", node} · {op:\"rename\", node, name} · {op:\"move\", node, parent} · {op:\"instance\", scene:\"res://x.tscn\", parent?, name?, props?} · {op:\"script\", node, path} · {op:\"connect\", from, signal, to, method} · {op:\"group\", node, add|remove}. Prop values: Godot literals as strings, JSON numbers/bools, \"res://...\" to load a resource, or {\"new\": Class, ...props} for a sub-resource. Stops at the first failing op; earlier ops stay applied (undo with api POST /api/undo).",
            json!({"type": "object", "properties": {
                "ops": {"type": "array", "items": {"type": "object"}},
                "show_tree": {"type": "boolean", "description": "Return the scene outline after the edit (default true)."}
            }, "required": ["ops"]})),
        tool("scene", "Open, create or save the edited scene. new: writes a scene file with one root node and opens it. open: loads a .tscn into the editor. save: writes the open scene (to `path`, default its own file).",
            json!({"type": "object", "properties": {
                "action": {"type": "string", "enum": ["new", "open", "save"]},
                "path": {"type": "string"}, "root_type": {"type": "string"}, "root_name": {"type": "string"}
            }, "required": ["action"]})),
        tool("write_script", "Write a GDScript file and check it: reports syntax errors with line/column and the static checker's findings. Optionally attach it to a node of the open scene.",
            json!({"type": "object", "properties": {
                "path": {"type": "string", "description": "res:// path, e.g. res://player.gd"},
                "source": {"type": "string"},
                "attach_to": {"type": "string", "description": "Node path to attach the script to."}
            }, "required": ["path", "source"]})),
        tool("play", "Boot the open scene headless and frame-exact, step it against an input timeline, and report: prints and errors tagged by frame, probe values, screenshots. Stops the game afterwards unless keep_running.", timeline("run")),
        tool("step", "Advance the paused running game (after play with keep_running, or a game the human paused) by N more frames, with the same inputs/probes/screenshots as play.", timeline("advance")),
        tool("stop", "Stop the running game.", json!({"type": "object", "properties": {}})),
        tool("screenshot", "Render the game view: the running game's current frame, or the open scene as the game would first draw it when nothing runs. view=\"player\" instead returns the exact frame the browser's Game view was last sent — what a person playing in the editor sees, at the panel's size — so a picture that only looks wrong in the browser shows up.",
            json!({"type": "object", "properties": {
                "width": {"type": "integer"}, "height": {"type": "integer"},
                "view": {"type": "string", "enum": ["game", "player"], "description": "game (default): render the game at the project's window size. player: the frame the browser was last sent."}
            }})),
        tool("diagnose", "Why does Play feel slow or look wrong in the browser? Measured, not configured: game frames stepped per second, frames rendered and ms each, frames the browser actually received, requests the rate limiter refused, the build profile, the running window's stretch against project.godot, and held keys — then findings naming the bottleneck. Run it while a human plays.",
            json!({"type": "object", "properties": {}})),
        tool("audio", "What the audio DEVICE actually played, measured — the ear for an agent that cannot hear. Device queue counters (underruns = audible gaps, priming waits, queue depth, clipped samples) plus an analysis of the last `secs` of real device output: clicks (discontinuities, with timestamps and severity), gaps (dropouts), clipping, peak/RMS. `save` writes that capture as a WAV for offline listening. Needs a Play started with audio (the browser's Play button, or `play` with audio:true).",
            json!({"type": "object", "properties": {
                "secs": {"type": "number", "description": "How much recent device output to analyse (0.1-10, default 5)."},
                "save": {"type": "string", "description": "Optional path to write the capture to as 16-bit WAV."}
            }})),
        tool("log", "The game's output (prints and errors, frame-tagged) since a sequence number — e.g. after a human played in the browser.",
            json!({"type": "object", "properties": {"since": {"type": "integer"}, "limit": {"type": "integer"}}})),
        tool("class_info", "What Patina's ClassDB registers for a class: inheritance chain, its own properties with defaults, methods and signals. Use it before relying on an API.",
            json!({"type": "object", "properties": {"class": {"type": "string"}}, "required": ["class"]})),
        tool("api", "Escape hatch: call any editor HTTP route (GET /api/capabilities lists them). Returns the raw response.",
            json!({"type": "object", "properties": {
                "method": {"type": "string"}, "path": {"type": "string"}, "body": {"type": "object"}
            }, "required": ["method", "path"]})),
    ]
}

fn tool(name: &str, description: &str, schema: Value) -> Value {
    json!({ "name": name, "description": description, "inputSchema": schema })
}

/// A tool's successful result: text plus any images, in order.
struct Output {
    content: Vec<Value>,
}

impl Output {
    fn text(text: impl Into<String>) -> Self {
        Self {
            content: vec![json!({"type": "text", "text": text.into()})],
        }
    }

    fn image(&mut self, png: &[u8]) {
        self.content.push(json!({
            "type": "image",
            "data": crate::editor_server::base64_encode(png),
            "mimeType": "image/png",
        }));
    }
}

fn call_tool(ctx: &Ctx, params: &Value) -> Value {
    let name = params.get("name").and_then(Value::as_str).unwrap_or("");
    let args = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let result = match name {
        "status" => tool_status(ctx),
        "tree" => tool_tree(ctx, &args),
        "inspect" => tool_inspect(ctx, &args),
        "edit" => tool_edit(ctx, &args),
        "scene" => tool_scene(ctx, &args),
        "write_script" => tool_write_script(ctx, &args),
        "play" => tool_play(ctx, &args),
        "step" => tool_step(ctx, &args),
        "stop" => tool_stop(ctx),
        "screenshot" => tool_screenshot(ctx, &args),
        "diagnose" => tool_diagnose(ctx),
        "audio" => tool_audio(ctx, &args),
        "log" => tool_log(ctx, &args),
        "class_info" => tool_class_info(&args),
        "api" => tool_api(ctx, &args),
        other => Err(format!("unknown tool `{other}`")),
    };
    match result {
        Ok(out) => json!({ "content": out.content, "isError": false }),
        Err(message) => json!({
            "content": [{"type": "text", "text": message}],
            "isError": true,
        }),
    }
}

fn arg_str<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key).and_then(Value::as_str)
}

fn arg_bool(args: &Value, key: &str, default: bool) -> bool {
    args.get(key).and_then(Value::as_bool).unwrap_or(default)
}

fn arg_u64(args: &Value, key: &str) -> Option<u64> {
    args.get(key).and_then(Value::as_u64)
}

// ---------------------------------------------------------------------------
// Paths, values and formatting
// ---------------------------------------------------------------------------

/// The edited scene's root: the tree's current scene, else root's first child.
fn edit_scene_root(tree: &SceneTree) -> Option<NodeId> {
    tree.current_scene().or_else(|| {
        tree.get_node(tree.root_id())
            .and_then(|root| root.children().first().copied())
    })
}

/// The running game's scene root. Autoloads are also children of `/root`
/// and come first, so the fallback is the LAST child.
fn running_scene_root(tree: &SceneTree) -> Option<NodeId> {
    tree.current_scene().or_else(|| {
        tree.get_node(tree.root_id())
            .and_then(|root| root.children().last().copied())
    })
}

/// Resolves an agent-facing node path: `.`/empty = scene root, `/root/...` or
/// `/...` absolute, otherwise relative to the scene root (a leading segment
/// naming the root itself is accepted too).
fn resolve(tree: &SceneTree, scene_root: Option<NodeId>, path: &str) -> Result<NodeId, String> {
    let path = path.trim();
    if let Some(abs) = path.strip_prefix('/') {
        let full = if abs == "root" || abs.starts_with("root/") {
            format!("/{abs}")
        } else {
            format!("/root/{abs}")
        };
        return tree
            .get_node_by_path(&full)
            .ok_or_else(|| format!("no node at `{full}`"));
    }
    let root = scene_root.ok_or_else(|| "no scene is open".to_string())?;
    if path.is_empty() || path == "." {
        return Ok(root);
    }
    if let Some(id) = tree.get_node_or_null(root, path) {
        return Ok(id);
    }
    let root_name = tree
        .get_node(root)
        .map(|n| n.name().to_string())
        .unwrap_or_default();
    if path == root_name {
        return Ok(root);
    }
    if let Some(rest) = path.strip_prefix(&format!("{root_name}/")) {
        if let Some(id) = tree.get_node_or_null(root, rest) {
            return Ok(id);
        }
    }
    // Name the nearest existing ancestor's children so a typo is one look away.
    let mut parent = root;
    let mut parent_label = ".".to_string();
    for segment in path.split('/') {
        match tree.get_node_or_null(parent, segment) {
            Some(id) => {
                parent = id;
                parent_label = tree.node_path(id).unwrap_or_default();
            }
            None => break,
        }
    }
    let children: Vec<String> = tree
        .get_node(parent)
        .map(|n| {
            n.children()
                .iter()
                .filter_map(|c| tree.get_node(*c).map(|c| c.name().to_string()))
                .collect()
        })
        .unwrap_or_default();
    Err(format!(
        "no node `{path}` (relative to scene root `{root_name}`); `{parent_label}` has children: [{}]",
        children.join(", ")
    ))
}

/// The property's registered default for `class`, when ClassDB has one —
/// its variant type decides how a string value is read.
fn registered_default(class: &str, property: &str) -> Option<Variant> {
    // The MERGED list, so a subclass's default override (Label's
    // `mouse_filter`) wins over the base's value — the own-members view
    // (`get_class_info`) deliberately omits overrides.
    if let Some(p) = gdobject::class_db::get_property_list(class)
        .into_iter()
        .find(|p| p.name == property)
    {
        return Some(p.default_value);
    }
    let mut current = class.to_string();
    for _ in 0..32 {
        let info = gdobject::class_db::get_class_info(&current)?;
        if let Some(p) = info.properties.iter().find(|p| p.name == property) {
            return Some(p.default_value.clone());
        }
        if info.parent_class.is_empty() {
            return None;
        }
        current = info.parent_class;
    }
    None
}

/// Converts an agent-supplied value into the typed JSON `/api/property/set`
/// takes. Strings are read as Godot literals unless the property is a string
/// type, where the text is used verbatim (so `text = "1.5"` stays a String).
fn value_to_typed_json(value: &Value, string_property: bool) -> Result<Value, String> {
    let variant = json_to_variant(value, string_property)?;
    Ok(gdvariant::serialize::to_json(&variant))
}

fn json_to_variant(value: &Value, string_property: bool) -> Result<Variant, String> {
    Ok(match value {
        Value::Null => Variant::Nil,
        Value::Bool(b) => Variant::Bool(*b),
        Value::Number(n) => match n.as_i64() {
            Some(i) if !n.is_f64() => Variant::Int(i),
            _ => Variant::Float(n.as_f64().unwrap_or(0.0)),
        },
        Value::String(s) => {
            if string_property && !(s.starts_with('"') && s.ends_with('"') && s.len() >= 2) {
                Variant::String(s.clone())
            } else {
                gdresource::loader::parse_variant_value(s)
                    .unwrap_or_else(|_| Variant::String(s.clone()))
            }
        }
        Value::Array(items) => {
            let converted: Result<Vec<Variant>, String> =
                items.iter().map(|v| json_to_variant(v, false)).collect();
            Variant::Array(converted?.into())
        }
        Value::Object(map) => {
            if map.contains_key("type") {
                return gdvariant::serialize::from_json(value)
                    .ok_or_else(|| format!("not a typed variant: {value}"));
            }
            return Err(format!(
                "object values must be a typed variant ({{\"type\":..,\"value\":..}}) or, in `props`, a sub-resource {{\"new\": Class, ...}}: {value}"
            ));
        }
    })
}

fn fmt_variant(v: &Variant) -> String {
    gdscene::scene_saver::format_variant_value(v)
}

/// `(name, Godot-syntax value)` for every non-default schema property of a
/// node, plus — with `extras` — every other stored property that is not
/// editor-internal (script variables, metadata, runtime state).
fn node_props(tree: &SceneTree, id: NodeId, all: bool, extras: bool) -> Vec<(String, String)> {
    let json = crate::editor_server::node_properties_json(tree, id, None);
    let mut out = Vec::new();
    let Some(props) = json.get("properties").and_then(Value::as_array) else {
        return out;
    };
    for entry in props {
        let name = entry.get("name").and_then(Value::as_str).unwrap_or("");
        if name.is_empty() || name.starts_with('_') || name == "name" || name == "script" {
            continue;
        }
        let from_schema = !entry
            .get("type_name")
            .and_then(Value::as_str)
            .unwrap_or("")
            .is_empty();
        let is_default = entry
            .get("is_default")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if from_schema {
            if is_default && !all {
                continue;
            }
        } else if !extras {
            continue;
        }
        let value = entry
            .get("value")
            .and_then(gdvariant::serialize::from_json)
            .map(|v| fmt_variant(&v))
            .unwrap_or_else(|| entry.get("value").map(Value::to_string).unwrap_or_default());
        out.push((name.to_string(), value));
    }
    out
}

fn node_script_path(tree: &SceneTree, id: NodeId) -> Option<String> {
    let node = tree.get_node(id)?;
    match node.get_property("_script_path") {
        Variant::String(s) if !s.is_empty() => Some(s),
        _ => match node.get_property("script") {
            Variant::String(s) if s.starts_with("res://") => Some(s),
            // A live script with no path property (an autoload): ask it.
            _ => tree.get_script(id).map(|script| {
                script
                    .script_file_path()
                    .map(str::to_string)
                    .unwrap_or_else(|| "script".to_string())
            }),
        },
    }
}

fn node_instance_path(tree: &SceneTree, id: NodeId) -> Option<String> {
    let node = tree.get_node(id)?;
    for key in ["_instance_source", "_instance_path"] {
        if let Variant::String(s) = node.get_property(key) {
            if !s.is_empty() {
                return Some(s);
            }
        }
    }
    None
}

fn node_header(tree: &SceneTree, id: NodeId) -> String {
    let Some(node) = tree.get_node(id) else {
        return "<missing>".to_string();
    };
    let mut line = format!("{} ({})", node.name(), node.class_name());
    if let Some(script) = node_script_path(tree, id) {
        line.push_str(&format!(" [{script}]"));
    }
    if let Some(inst) = node_instance_path(tree, id) {
        line.push_str(&format!(" <{inst}>"));
    }
    let mut groups: Vec<&String> = node
        .groups()
        .iter()
        .filter(|g| !g.starts_with('_'))
        .collect();
    if !groups.is_empty() {
        groups.sort();
        let names: Vec<&str> = groups.iter().map(|g| g.as_str()).collect();
        line.push_str(&format!(" {{{}}}", names.join(", ")));
    }
    line
}

fn format_tree(
    tree: &SceneTree,
    id: NodeId,
    depth: usize,
    max_depth: usize,
    with_props: bool,
    budget: &mut usize,
    out: &mut String,
) {
    if *budget == 0 {
        return;
    }
    *budget -= 1;
    let mut line = format!("{}{}", "  ".repeat(depth), node_header(tree, id));
    if with_props {
        let props = node_props(tree, id, false, false);
        if !props.is_empty() {
            let mut text = props
                .iter()
                .map(|(k, v)| format!("{k}={v}"))
                .collect::<Vec<_>>()
                .join(" ");
            if text.chars().count() > TREE_PROPS_CHARS {
                text = text.chars().take(TREE_PROPS_CHARS).collect::<String>() + "…";
            }
            line.push(' ');
            line.push_str(&text);
        }
    }
    out.push_str(&line);
    out.push('\n');
    let children: Vec<NodeId> = tree
        .get_node(id)
        .map(|n| n.children().to_vec())
        .unwrap_or_default();
    if depth >= max_depth {
        if !children.is_empty() {
            out.push_str(&format!(
                "{}… {} children (raise depth)\n",
                "  ".repeat(depth + 1),
                children.len()
            ));
        }
        return;
    }
    for child in children {
        if *budget == 0 {
            out.push_str(&format!(
                "{}… (node budget reached)\n",
                "  ".repeat(depth + 1)
            ));
            return;
        }
        format_tree(tree, child, depth + 1, max_depth, with_props, budget, out);
    }
}

fn outline(tree: &SceneTree, root: NodeId, max_depth: usize, with_props: bool) -> String {
    outline_with_budget(tree, root, max_depth, with_props, TREE_NODE_BUDGET)
}

/// [`outline`] printing at most `budget` nodes, then saying how many of the
/// subtree's nodes (within `max_depth`) it left out and how to see them
/// (pat-rbyty). A silent cut at 400 hid most of a 626-node game.
fn outline_with_budget(
    tree: &SceneTree,
    root: NodeId,
    max_depth: usize,
    with_props: bool,
    budget: usize,
) -> String {
    fn count(tree: &SceneTree, id: NodeId, depth: usize, max_depth: usize) -> usize {
        let children = tree.get_node(id).map(|n| n.children().to_vec()).unwrap_or_default();
        1 + if depth >= max_depth {
            0
        } else {
            children.iter().map(|&c| count(tree, c, depth + 1, max_depth)).sum()
        }
    }
    let mut out = String::new();
    let mut remaining = budget;
    format_tree(tree, root, 0, max_depth, with_props, &mut remaining, &mut out);
    let printed = budget - remaining;
    let total = count(tree, root, 0, max_depth);
    if printed < total {
        out.push_str(&format!(
            "… {} of {total} nodes omitted (budget {budget}): pass a larger `budget`, or list a subtree with `node`\n",
            total - printed
        ));
    }
    out
}

/// `res://x` → `<project>/x`; a relative path joins the project root; an
/// absolute path is kept.
fn to_disk_path(project_root: Option<&str>, path: &str) -> String {
    let root = project_root.unwrap_or(".");
    if let Some(rel) = path.strip_prefix("res://") {
        format!("{}/{}", root.trim_end_matches('/'), rel)
    } else if path.starts_with('/') {
        path.to_string()
    } else {
        format!("{}/{}", root.trim_end_matches('/'), path)
    }
}

/// `<project>/x` → `res://x` when inside the project.
fn to_res_path(project_root: Option<&str>, disk: &str) -> String {
    if disk.starts_with("res://") {
        return disk.to_string();
    }
    if let Some(root) = project_root {
        let root = root.trim_end_matches('/');
        if let Some(rel) = disk.strip_prefix(&format!("{root}/")) {
            return format!("res://{rel}");
        }
    }
    disk.to_string()
}

// ---------------------------------------------------------------------------
// status / tree / inspect / log / class_info / api
// ---------------------------------------------------------------------------

fn tool_status(ctx: &Ctx) -> Result<Output, String> {
    let s = ctx.state.lock().unwrap();
    let root = s.project_root.clone();
    let mut out = String::new();
    let setting = |key: &str| {
        s.project_settings
            .get(key)
            .and_then(|v| v.as_string().map(str::to_string))
    };
    out.push_str(&format!(
        "project: {}  name: {}  main_scene: {}\n",
        root.as_deref()
            .unwrap_or("(none — api POST /api/project/open {\"path\": dir})"),
        setting("application/config/name").unwrap_or_else(|| s.project_name.clone()),
        setting("application/run/main_scene").unwrap_or_else(|| "(unset)".to_string()),
    ));
    let scene_root = edit_scene_root(&s.scene_tree);
    let scene_label = s
        .scene_file
        .as_deref()
        .map(|f| to_res_path(root.as_deref(), &to_disk_path(root.as_deref(), f)))
        .unwrap_or_else(|| "(unsaved)".to_string());
    out.push_str(&format!(
        "scene: {}{}  root: {}  nodes: {}\n",
        scene_label,
        if s.scene_modified { " (modified)" } else { "" },
        scene_root
            .map(|r| node_header(&s.scene_tree, r))
            .unwrap_or_else(|| "(none)".to_string()),
        scene_root
            .map(|r| count_subtree(&s.scene_tree, r))
            .unwrap_or(0),
    ));
    out.push_str(&if s.is_running {
        format!(
            "game: running{} at frame {}\n",
            if s.is_paused { " (paused)" } else { "" },
            s.runtime_frame_count
        )
    } else {
        "game: stopped\n".to_string()
    });
    let map = s.build_play_input_map();
    let mut actions: Vec<&String> = map.actions().filter(|a| !a.starts_with("ui_")).collect();
    actions.sort();
    out.push_str(&format!(
        "input actions: {} (+ built-in ui_*)\n",
        if actions.is_empty() {
            "(none declared)".to_string()
        } else {
            actions
                .iter()
                .map(|a| a.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        }
    ));
    let autoloads: Vec<String> = s
        .project_settings
        .iter()
        .filter_map(|(k, _)| k.strip_prefix("autoload/").map(str::to_string))
        .collect();
    if !autoloads.is_empty() {
        out.push_str(&format!("autoloads: {}\n", autoloads.join(", ")));
    }
    let errors: Vec<&GameLogEntry> = s
        .game_log
        .since(0)
        .filter(|e| e.kind == GameLogKind::Error)
        .collect();
    if !errors.is_empty() {
        out.push_str(&format!("last run errors ({}):\n", errors.len()));
        for e in errors.iter().rev().take(5).rev() {
            out.push_str(&format!("  f{} {}\n", e.frame, e.text));
        }
    }
    drop(s);
    if let Some(root) = root.as_deref() {
        out.push_str(&project_files(root));
    }
    Ok(Output::text(out))
}

fn count_subtree(tree: &SceneTree, id: NodeId) -> usize {
    1 + tree
        .get_node(id)
        .map(|n| n.children().iter().map(|c| count_subtree(tree, *c)).sum())
        .unwrap_or(0)
}

/// The project's scenes, scripts and other files, as `res://` paths.
fn project_files(root: &str) -> String {
    let mut scenes = Vec::new();
    let mut scripts = Vec::new();
    let mut other = Vec::new();
    let mut stack = vec![std::path::PathBuf::from(root)];
    let mut visited = 0usize;
    while let Some(dir) = stack.pop() {
        let Ok(read) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in read.flatten() {
            visited += 1;
            if visited > 5000 {
                break;
            }
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with('.') || name.ends_with(".import") || name.ends_with(".uid") {
                continue;
            }
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            let res = to_res_path(Some(root), &path.to_string_lossy());
            if name.ends_with(".tscn") || name.ends_with(".scn") {
                scenes.push(res);
            } else if name.ends_with(".gd") {
                scripts.push(res);
            } else if name != "project.godot" {
                other.push(res);
            }
        }
    }
    let mut text = String::new();
    for (label, list) in [
        ("scenes", &mut scenes),
        ("scripts", &mut scripts),
        ("other files", &mut other),
    ] {
        list.sort();
        let shown: Vec<&str> = list.iter().take(40).map(String::as_str).collect();
        let more = list.len().saturating_sub(shown.len());
        text.push_str(&format!(
            "{label} ({}): {}{}\n",
            list.len(),
            shown.join(" "),
            if more > 0 {
                format!(" … +{more}")
            } else {
                String::new()
            }
        ));
    }
    text
}

fn tool_tree(ctx: &Ctx, args: &Value) -> Result<Output, String> {
    let running = arg_bool(args, "running", false);
    let depth = arg_u64(args, "depth").unwrap_or(64) as usize;
    let with_props = arg_bool(args, "props", true);
    let node = arg_str(args, "node").unwrap_or(".");
    let s = ctx.state.lock().unwrap();
    let tree = if running {
        s.run_main_loop
            .as_ref()
            .ok_or_else(|| {
                "the game is not running (use play with keep_running, or running=false)".to_string()
            })?
            .tree()
    } else {
        &s.scene_tree
    };
    let root = if running {
        running_scene_root(tree)
    } else {
        edit_scene_root(tree)
    };
    let start = resolve(tree, root, node)?;
    let budget = arg_u64(args, "budget")
        .map(|b| (b as usize).clamp(1, TREE_NODE_BUDGET_MAX))
        .unwrap_or(TREE_NODE_BUDGET);
    Ok(Output::text(outline_with_budget(tree, start, depth, with_props, budget)))
}

fn tool_inspect(ctx: &Ctx, args: &Value) -> Result<Output, String> {
    let path = arg_str(args, "node").ok_or("`node` is required")?;
    let running = arg_bool(args, "running", false);
    let all = arg_bool(args, "all", false);
    let s = ctx.state.lock().unwrap();
    let tree = if running {
        s.run_main_loop
            .as_ref()
            .ok_or_else(|| "the game is not running".to_string())?
            .tree()
    } else {
        &s.scene_tree
    };
    let root = if running {
        running_scene_root(tree)
    } else {
        edit_scene_root(tree)
    };
    let id = resolve(tree, root, path)?;
    let mut out = format!(
        "{}  {}\n",
        tree.node_path(id).unwrap_or_default(),
        node_header(tree, id)
    );
    let props = node_props(tree, id, all, false);
    out.push_str(if all {
        "properties:\n"
    } else {
        "properties (non-default):\n"
    });
    if props.is_empty() {
        out.push_str("  (all defaults)\n");
    }
    for (k, v) in &props {
        out.push_str(&format!("  {k} = {v}\n"));
    }
    if let Some(script) = tree.get_script(id) {
        let vars = script.list_properties();
        if !vars.is_empty() {
            out.push_str("script variables:\n");
            for info in vars {
                let value = script
                    .get_property(&info.name)
                    .map(|v| fmt_variant(&v))
                    .unwrap_or_else(|| "?".to_string());
                out.push_str(&format!("  {} = {}\n", info.name, value));
            }
        }
    }
    if let Some(node) = tree.get_node(id) {
        let kids: Vec<String> = node
            .children()
            .iter()
            .filter_map(|c| {
                tree.get_node(*c)
                    .map(|n| format!("{} ({})", n.name(), n.class_name()))
            })
            .collect();
        out.push_str(&format!(
            "children: {}\n",
            if kids.is_empty() {
                "(none)".to_string()
            } else {
                kids.join(", ")
            }
        ));
    }
    Ok(Output::text(out))
}

fn tool_log(ctx: &Ctx, args: &Value) -> Result<Output, String> {
    let since = arg_u64(args, "since").unwrap_or(0);
    let limit = arg_u64(args, "limit").unwrap_or(REPORT_LOG_LINES as u64) as usize;
    let s = ctx.state.lock().unwrap();
    let entries: Vec<&GameLogEntry> = s.game_log.since(since).collect();
    let mut out = format!(
        "{} line(s) since #{since}; next since = {}\n",
        entries.len(),
        s.game_log.next_seq()
    );
    let skip = entries.len().saturating_sub(limit);
    if skip > 0 {
        out.push_str(&format!("… {skip} older line(s) omitted\n"));
    }
    for e in entries.iter().skip(skip) {
        out.push_str(&format_log_line(e));
    }
    Ok(Output::text(out))
}

/// The report's output section: lines in order of first appearance, with
/// every repeat of an identical line folded into its first occurrence as
/// `×N (fA–fB)`. A script failing every frame otherwise buried everything else
/// under hundreds of copies of one error.
fn collapse_log(entries: &[&GameLogEntry], max_lines: usize) -> String {
    let mut order: Vec<(GameLogKind, &str)> = Vec::new();
    let mut seen: std::collections::HashMap<(GameLogKind, &str), (u64, u64, usize)> =
        std::collections::HashMap::new();
    for e in entries {
        let key = (e.kind, e.text.as_str());
        let slot = seen.entry(key).or_insert_with(|| {
            order.push(key);
            (e.frame, e.frame, 0)
        });
        slot.1 = e.frame;
        slot.2 += 1;
    }
    let mut out = String::new();
    for (i, key) in order.iter().enumerate() {
        if i == max_lines {
            out.push_str(&format!(
                "  … {} more distinct line(s) (use `log`)\n",
                order.len() - max_lines
            ));
            break;
        }
        let (first, last, count) = seen[key];
        let tag = match key.0 {
            GameLogKind::Print => "",
            GameLogKind::Error => "ERROR ",
        };
        if count == 1 {
            out.push_str(&format!("  f{first} {tag}{}\n", key.1));
        } else {
            out.push_str(&format!("  f{first}–f{last} ×{count} {tag}{}\n", key.1));
        }
    }
    out
}

fn format_log_line(e: &GameLogEntry) -> String {
    match e.kind {
        GameLogKind::Print => format!("  f{} {}\n", e.frame, e.text),
        GameLogKind::Error => format!("  f{} ERROR {}\n", e.frame, e.text),
    }
}

fn tool_class_info(args: &Value) -> Result<Output, String> {
    let class = arg_str(args, "class").ok_or("`class` is required")?;
    let info = gdobject::class_db::get_class_info(class)
        .ok_or_else(|| format!("`{class}` is not registered in Patina's ClassDB"))?;
    let mut chain = vec![info.class_name.clone()];
    let mut parent = info.parent_class.clone();
    while !parent.is_empty() && chain.len() < 32 {
        chain.push(parent.clone());
        parent = gdobject::class_db::get_class_info(&parent)
            .map(|p| p.parent_class)
            .unwrap_or_default();
    }
    let mut out = format!("{}\n", chain.join(" < "));
    out.push_str(&format!("properties ({}):\n", info.properties.len()));
    for p in &info.properties {
        out.push_str(&format!(
            "  {} = {}\n",
            p.name,
            fmt_variant(&p.default_value)
        ));
    }
    let mut methods: Vec<&str> = info.methods.iter().map(|m| m.name.as_str()).collect();
    methods.sort_unstable();
    out.push_str(&format!(
        "methods ({}): {}\n",
        methods.len(),
        methods.join(", ")
    ));
    let signals: Vec<String> = info
        .signals
        .iter()
        .map(|sig| format!("{}({})", sig.name, sig.arguments.join(", ")))
        .collect();
    out.push_str(&format!(
        "signals ({}): {}\n",
        signals.len(),
        signals.join(", ")
    ));
    out.push_str("(own members only — query a parent class for inherited ones)\n");
    Ok(Output::text(out))
}

fn tool_api(ctx: &Ctx, args: &Value) -> Result<Output, String> {
    let method = arg_str(args, "method")
        .ok_or("`method` is required")?
        .to_uppercase();
    let path = arg_str(args, "path").ok_or("`path` is required")?;
    if !path.starts_with("/api/") {
        return Err("`path` must start with /api/".to_string());
    }
    let (status, bytes) = ctx.api.request(&method, path, args.get("body"))?;
    if bytes.starts_with(b"\x89PNG") {
        let mut out = Output::text(format!("{status} image/png ({} bytes)", bytes.len()));
        out.image(&bytes);
        return Ok(out);
    }
    let mut text = String::from_utf8_lossy(&bytes).to_string();
    if text.len() > 60_000 {
        let cut = (0..=60_000)
            .rev()
            .find(|i| text.is_char_boundary(*i))
            .unwrap_or(0);
        text.truncate(cut);
        text.push_str("\n… (truncated)");
    }
    Ok(Output::text(format!("{status}\n{text}")))
}

// ---------------------------------------------------------------------------
// edit / scene / write_script
// ---------------------------------------------------------------------------

fn refuse_while_running(ctx: &Ctx) -> Result<(), String> {
    if ctx.state.lock().unwrap().is_running {
        return Err(
            "the game is running and the editor scene is read-only during play — call `stop` first"
                .to_string(),
        );
    }
    Ok(())
}

/// Resolves `path` in the edit tree to the raw id the routes take.
fn edit_node_id(ctx: &Ctx, path: &str) -> Result<u64, String> {
    let s = ctx.state.lock().unwrap();
    let root = edit_scene_root(&s.scene_tree);
    resolve(&s.scene_tree, root, path).map(|id| id.raw())
}

fn edit_node_class(ctx: &Ctx, raw: u64) -> String {
    let s = ctx.state.lock().unwrap();
    s.scene_tree
        .all_nodes_in_tree_order()
        .into_iter()
        .find(|id| id.raw() == raw)
        .and_then(|id| {
            s.scene_tree
                .get_node(id)
                .map(|n| n.class_name().to_string())
        })
        .unwrap_or_default()
}

fn tool_edit(ctx: &Ctx, args: &Value) -> Result<Output, String> {
    refuse_while_running(ctx)?;
    let ops = args
        .get("ops")
        .and_then(Value::as_array)
        .ok_or("`ops` must be an array of edit operations")?;
    let mut log = String::new();
    for (index, op) in ops.iter().enumerate() {
        match apply_op(ctx, op) {
            Ok(line) => log.push_str(&format!("✓ {line}\n")),
            Err(e) => {
                log.push_str(&format!("✗ op {} {}: {e}\n", index + 1, op));
                log.push_str(&format!(
                    "stopped: {} of {} op(s) applied (earlier ops stay; undo with api POST /api/undo)\n",
                    index,
                    ops.len()
                ));
                return Err(log);
            }
        }
    }
    if arg_bool(args, "show_tree", true) {
        let s = ctx.state.lock().unwrap();
        if let Some(root) = edit_scene_root(&s.scene_tree) {
            log.push('\n');
            log.push_str(&outline(&s.scene_tree, root, 64, true));
        }
    }
    Ok(Output::text(log))
}

fn op_field<'a>(op: &'a Value, key: &str) -> Result<&'a str, String> {
    op.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("missing `{key}`"))
}

fn apply_op(ctx: &Ctx, op: &Value) -> Result<String, String> {
    let kind = op_field(op, "op")?;
    match kind {
        "add" => {
            let class = op_field(op, "type")?;
            let name = op.get("name").and_then(Value::as_str).unwrap_or(class);
            let parent_path = op.get("parent").and_then(Value::as_str).unwrap_or(".");
            let parent = edit_node_id(ctx, parent_path)?;
            let created = ctx.api.post(
                "/api/node/add",
                json!({"parent_id": parent, "name": name, "class_name": class}),
            )?;
            let id = created
                .get("id")
                .and_then(Value::as_u64)
                .ok_or_else(|| format!("/api/node/add returned no id: {created}"))?;
            let actual = node_name_by_raw(ctx, id);
            if let Some(props) = op.get("props") {
                set_props(ctx, id, props)?;
            }
            let renamed = if actual != name {
                format!(" (named `{actual}` — `{name}` was taken)")
            } else {
                String::new()
            };
            Ok(format!("add {actual} ({class}) under {parent_path}{renamed}"))
        }
        "set" => {
            let path = op_field(op, "node")?;
            let id = edit_node_id(ctx, path)?;
            let props = op.get("props").ok_or("missing `props`")?;
            set_props(ctx, id, props)?;
            Ok(format!("set {path}: {}", summarize_keys(props)))
        }
        "delete" => {
            let path = op_field(op, "node")?;
            let id = edit_node_id(ctx, path)?;
            ctx.api.post("/api/node/delete", json!({"node_id": id}))?;
            Ok(format!("delete {path}"))
        }
        "rename" => {
            let path = op_field(op, "node")?;
            let name = op_field(op, "name")?;
            let id = edit_node_id(ctx, path)?;
            ctx.api.post("/api/node/rename", json!({"node_id": id, "new_name": name}))?;
            Ok(format!("rename {path} → {name}"))
        }
        "move" => {
            let path = op_field(op, "node")?;
            let parent_path = op_field(op, "parent")?;
            let id = edit_node_id(ctx, path)?;
            let parent = edit_node_id(ctx, parent_path)?;
            ctx.api.post(
                "/api/node/reparent",
                json!({"node_id": id, "new_parent_id": parent, "keep_transform": op.get("keep_transform").and_then(Value::as_bool).unwrap_or(false)}),
            )?;
            Ok(format!("move {path} under {parent_path}"))
        }
        "instance" => {
            let scene = op_field(op, "scene")?;
            let parent_path = op.get("parent").and_then(Value::as_str).unwrap_or(".");
            let parent = edit_node_id(ctx, parent_path)?;
            let before = child_ids(ctx, parent);
            let res = ctx.api.post(
                "/api/scene/instance",
                json!({"parent_id": parent, "path": scene}),
            )?;
            let id = res
                .get("id")
                .and_then(Value::as_u64)
                .or_else(|| child_ids(ctx, parent).into_iter().find(|c| !before.contains(c)))
                .ok_or_else(|| format!("/api/scene/instance created no node: {res}"))?;
            if let Some(name) = op.get("name").and_then(Value::as_str) {
                ctx.api.post("/api/node/rename", json!({"node_id": id, "new_name": name}))?;
            }
            if let Some(props) = op.get("props") {
                set_props(ctx, id, props)?;
            }
            Ok(format!("instance {scene} as {} under {parent_path}", node_name_by_raw(ctx, id)))
        }
        "script" => {
            let path = op_field(op, "node")?;
            let id = edit_node_id(ctx, path)?;
            match op.get("path").and_then(Value::as_str) {
                Some(script) => {
                    ctx.api.post(
                        "/api/property/set",
                        json!({"node_id": id, "property": "script", "value": {"type": "String", "value": script}}),
                    )?;
                    Ok(format!("script {path} = {script}"))
                }
                None => {
                    ctx.api.post("/api/node/detach_script", json!({"node_id": id}))?;
                    Ok(format!("detach script from {path}"))
                }
            }
        }
        "connect" => {
            let from = op_field(op, "from")?;
            let signal = op_field(op, "signal")?;
            let to = op_field(op, "to")?;
            let method = op_field(op, "method")?;
            let from_id = edit_node_id(ctx, from)?;
            let to_id = edit_node_id(ctx, to)?;
            let mut body = json!({"node_id": from_id, "signal": signal, "target_id": to_id, "method": method});
            for key in ["binds", "deferred", "one_shot"] {
                if let Some(v) = op.get(key) {
                    body[key] = v.clone();
                }
            }
            ctx.api.post("/api/node/signals/connect", body)?;
            Ok(format!("connect {from}.{signal} → {to}.{method}"))
        }
        "group" => {
            let path = op_field(op, "node")?;
            let id = edit_node_id(ctx, path)?;
            if let Some(group) = op.get("add").and_then(Value::as_str) {
                ctx.api.post("/api/node/groups/add", json!({"node_id": id, "group": group}))?;
                Ok(format!("group {path} += {group}"))
            } else if let Some(group) = op.get("remove").and_then(Value::as_str) {
                ctx.api.post("/api/node/groups/remove", json!({"node_id": id, "group": group}))?;
                Ok(format!("group {path} -= {group}"))
            } else {
                Err("group needs `add` or `remove`".to_string())
            }
        }
        other => Err(format!(
            "unknown op `{other}` (add, set, delete, rename, move, instance, script, connect, group)"
        )),
    }
}

fn summarize_keys(props: &Value) -> String {
    props
        .as_object()
        .map(|m| m.keys().cloned().collect::<Vec<_>>().join(", "))
        .unwrap_or_default()
}

fn node_name_by_raw(ctx: &Ctx, raw: u64) -> String {
    let s = ctx.state.lock().unwrap();
    s.scene_tree
        .all_nodes_in_tree_order()
        .into_iter()
        .find(|id| id.raw() == raw)
        .and_then(|id| s.scene_tree.get_node(id).map(|n| n.name().to_string()))
        .unwrap_or_default()
}

fn child_ids(ctx: &Ctx, parent_raw: u64) -> Vec<u64> {
    let s = ctx.state.lock().unwrap();
    s.scene_tree
        .all_nodes_in_tree_order()
        .into_iter()
        .find(|id| id.raw() == parent_raw)
        .and_then(|id| {
            s.scene_tree
                .get_node(id)
                .map(|n| n.children().iter().map(|c| c.raw()).collect())
        })
        .unwrap_or_default()
}

/// Sets each `props` entry on a node through the matching route: a
/// `{"new": Class}` object makes a sub-resource, a `res://` string on a
/// resource-typed property loads that resource, `script` attaches a script,
/// anything else is a plain property write.
fn set_props(ctx: &Ctx, node_raw: u64, props: &Value) -> Result<(), String> {
    let map = props.as_object().ok_or("`props` must be an object")?;
    let class = edit_node_class(ctx, node_raw);
    for (name, value) in map {
        let default = registered_default(&class, name);
        if let Some(obj) = value.as_object().filter(|o| o.contains_key("new")) {
            let res_class = obj
                .get("new")
                .and_then(Value::as_str)
                .ok_or("`new` must be a class name")?;
            ctx.api
                .post(
                    "/api/property/resource/new",
                    json!({"node_id": node_raw, "property": name, "class_name": res_class}),
                )
                .map_err(|e| format!("{name} = new {res_class}: {e}"))?;
            for (sub, sub_value) in obj.iter().filter(|(k, _)| k.as_str() != "new") {
                let sub_default = registered_default(res_class, sub);
                let typed = value_to_typed_json(
                    sub_value,
                    matches!(sub_default, Some(Variant::String(_))),
                )?;
                ctx.api.post(
                    "/api/resource/property/set",
                    json!({"node_id": node_raw, "resource_property": name, "sub_property": sub, "value": typed}),
                )
                .map_err(|e| format!("{name}.{sub}: {e}"))?;
            }
            continue;
        }
        let is_resource_slot = matches!(
            default,
            Some(Variant::Nil) | Some(Variant::ObjectId(_)) | Some(Variant::Resource(_)) | None
        );
        if let Some(path) = value.as_str().filter(|p| p.starts_with("res://")) {
            if name == "script" {
                ctx.api.post(
                    "/api/property/set",
                    json!({"node_id": node_raw, "property": "script", "value": {"type": "String", "value": path}}),
                )?;
                continue;
            }
            if is_resource_slot {
                ctx.api
                    .post(
                        "/api/property/resource/assign",
                        json!({"node_id": node_raw, "property": name, "path": path}),
                    )
                    .map_err(|e| format!("{name} = {path}: {e}"))?;
                continue;
            }
        }
        let string_typed = matches!(
            default,
            Some(Variant::String(_)) | Some(Variant::StringName(_)) | Some(Variant::NodePath(_))
        );
        let typed = value_to_typed_json(value, string_typed).map_err(|e| format!("{name}: {e}"))?;
        ctx.api
            .post(
                "/api/property/set",
                json!({"node_id": node_raw, "property": name, "value": typed}),
            )
            .map_err(|e| format!("{name}: {e}"))?;
    }
    Ok(())
}

fn tool_scene(ctx: &Ctx, args: &Value) -> Result<Output, String> {
    let action = arg_str(args, "action").ok_or("`action` is required (new, open, save)")?;
    let project_root = ctx.state.lock().unwrap().project_root.clone();
    match action {
        "new" => {
            refuse_while_running(ctx)?;
            let path = arg_str(args, "path").ok_or("`path` is required")?;
            let root_type = arg_str(args, "root_type").unwrap_or("Node2D");
            let default_name = path
                .rsplit('/')
                .next()
                .and_then(|f| f.strip_suffix(".tscn"))
                .map(pascal_case)
                .unwrap_or_else(|| root_type.to_string());
            let root_name = arg_str(args, "root_name")
                .map(str::to_string)
                .unwrap_or(default_name);
            let disk = to_disk_path(project_root.as_deref(), path);
            if std::path::Path::new(&disk).exists() {
                return Err(format!(
                    "{path} already exists — open it, or pick another path"
                ));
            }
            if let Some(dir) = std::path::Path::new(&disk).parent() {
                std::fs::create_dir_all(dir)
                    .map_err(|e| format!("mkdir {}: {e}", dir.display()))?;
            }
            std::fs::write(
                &disk,
                format!(
                    "[gd_scene format=3]\n\n[node name=\"{root_name}\" type=\"{root_type}\"]\n"
                ),
            )
            .map_err(|e| format!("write {disk}: {e}"))?;
            ctx.api.post("/api/scene/load", json!({"path": disk}))?;
            Ok(Output::text(format!(
                "created and opened {path} (root {root_name}: {root_type})"
            )))
        }
        "open" => {
            refuse_while_running(ctx)?;
            let path = arg_str(args, "path").ok_or("`path` is required")?;
            let disk = to_disk_path(project_root.as_deref(), path);
            ctx.api.post("/api/scene/load", json!({"path": disk}))?;
            let s = ctx.state.lock().unwrap();
            let tree = edit_scene_root(&s.scene_tree)
                .map(|r| outline(&s.scene_tree, r, 64, true))
                .unwrap_or_default();
            Ok(Output::text(format!("opened {path}\n\n{tree}")))
        }
        "save" => {
            let current = ctx.state.lock().unwrap().scene_file.clone();
            let target = match arg_str(args, "path") {
                Some(p) => to_disk_path(project_root.as_deref(), p),
                None => current
                    .map(|f| to_disk_path(project_root.as_deref(), &f))
                    .ok_or("the scene has never been saved — give a `path`")?,
            };
            ctx.api.post("/api/scene/save", json!({"path": target}))?;
            Ok(Output::text(format!(
                "saved {}",
                to_res_path(project_root.as_deref(), &target)
            )))
        }
        other => Err(format!("unknown action `{other}` (new, open, save)")),
    }
}

fn pascal_case(stem: &str) -> String {
    stem.split(|c: char| c == '_' || c == '-' || c == ' ')
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut chars = w.chars();
            chars
                .next()
                .map(|c| c.to_uppercase().collect::<String>() + chars.as_str())
                .unwrap_or_default()
        })
        .collect()
}

/// Syntax and static-check findings for a GDScript source, one per line.
pub fn script_diagnostics(source: &str) -> Vec<String> {
    use gdscript_interop::parser::ParseError;
    use gdscript_interop::type_checker::{type_check, TypeCheckError};
    match type_check(source) {
        Ok(errors) => errors
            .into_iter()
            .map(|e| format!("warning (static check): {}", e.message))
            .collect(),
        Err(TypeCheckError::Parse(ParseError::UnexpectedToken {
            token,
            expected,
            line,
            col,
            source_line,
        })) => {
            let mut msg =
                format!("error: line {line}, col {col}: unexpected {token}, expected {expected}");
            if let Some(text) = source_line {
                msg.push_str(&format!(
                    "\n    {text}\n    {}^",
                    " ".repeat(col.saturating_sub(1))
                ));
            }
            vec![msg]
        }
        Err(e) => vec![format!("error: {e}")],
    }
}

fn tool_write_script(ctx: &Ctx, args: &Value) -> Result<Output, String> {
    let path = arg_str(args, "path").ok_or("`path` is required")?;
    let source = arg_str(args, "source").ok_or("`source` is required")?;
    if !path.ends_with(".gd") {
        return Err("`path` must end in .gd".to_string());
    }
    let res = if path.starts_with("res://") {
        path.to_string()
    } else {
        format!("res://{}", path.trim_start_matches('/'))
    };
    ctx.api
        .post("/api/script/save", json!({"path": res, "content": source}))?;
    let diagnostics = script_diagnostics(source);
    let mut out = format!("saved {res} ({} lines)", source.lines().count());
    if diagnostics.is_empty() {
        out.push_str(" — no errors\n");
    } else {
        out.push('\n');
        for d in &diagnostics {
            out.push_str(d);
            out.push('\n');
        }
    }
    if let Some(node) = arg_str(args, "attach_to") {
        refuse_while_running(ctx)?;
        let id = edit_node_id(ctx, node)?;
        ctx.api.post(
            "/api/property/set",
            json!({"node_id": id, "property": "script", "value": {"type": "String", "value": res}}),
        )?;
        out.push_str(&format!("attached to {node}\n"));
    }
    Ok(Output::text(out))
}

// ---------------------------------------------------------------------------
// play / step / stop / screenshot
// ---------------------------------------------------------------------------

enum InputKind {
    Action(String),
    Key(String),
    Click(f64, f64),
}

struct TimedInput {
    kind: InputKind,
    /// First frame (1-based, relative to the call) the input is held.
    start: u64,
    /// Last frame it is held, inclusive.
    end: u64,
}

#[derive(Clone)]
struct Probe {
    node: String,
    props: Vec<String>,
    at: Option<u64>,
    every: Option<u64>,
}

impl Probe {
    fn due(&self, frame: u64) -> bool {
        self.at == Some(frame)
            || self
                .every
                .is_some_and(|n| n > 0 && frame > 0 && frame % n == 0)
    }
}

fn parse_inputs(args: &Value, known_actions: &[String]) -> Result<Vec<TimedInput>, String> {
    let mut out = Vec::new();
    for (i, entry) in args
        .get("inputs")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .enumerate()
    {
        let start = entry.get("at").and_then(Value::as_u64).unwrap_or(1).max(1);
        let end = match (
            entry.get("to").and_then(Value::as_u64),
            entry.get("frames").and_then(Value::as_u64),
        ) {
            (Some(to), _) => to.max(start),
            (None, Some(n)) => start + n.max(1) - 1,
            (None, None) => start,
        };
        let kind = if let Some(action) = entry.get("action").and_then(Value::as_str) {
            if !known_actions.iter().any(|a| a == action) {
                return Err(format!(
                    "input {}: unknown action `{action}`; the project declares: {}",
                    i + 1,
                    known_actions.join(", ")
                ));
            }
            InputKind::Action(action.to_string())
        } else if let Some(key) = entry.get("key").and_then(Value::as_str) {
            // Rejected here, like an unknown action: the input route drops a
            // name it cannot resolve without a word, so a playtest holding
            // "Lft" for 40 frames reported a clean run in which nothing moved.
            if gdplatform::input::Key::from_name(key).is_none() {
                return Err(format!(
                    "input {}: unknown key `{key}` — use a Godot or DOM key name \
                     (\"A\", \"Space\", \"Enter\", \"Left\"/\"ArrowLeft\", \"Escape\", \"F1\")",
                    i + 1
                ));
            }
            InputKind::Key(key.to_string())
        } else if let Some(click) = entry.get("click").and_then(Value::as_array) {
            let x = click
                .first()
                .and_then(Value::as_f64)
                .ok_or("click needs [x, y]")?;
            let y = click
                .get(1)
                .and_then(Value::as_f64)
                .ok_or("click needs [x, y]")?;
            InputKind::Click(x, y)
        } else {
            return Err(format!(
                "input {}: needs `action`, `key` or `click`: {entry}",
                i + 1
            ));
        };
        out.push(TimedInput { kind, start, end });
    }
    Ok(out)
}

fn parse_probes(args: &Value) -> Result<Vec<Probe>, String> {
    let mut out = Vec::new();
    for entry in args
        .get("probes")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let node = entry
            .get("node")
            .and_then(Value::as_str)
            .ok_or("probe needs `node`")?;
        let props = entry
            .get("props")
            .and_then(Value::as_array)
            .ok_or("probe needs `props`")?
            .iter()
            .filter_map(|p| p.as_str().map(str::to_string))
            .collect();
        let at = entry.get("at").and_then(Value::as_u64);
        let every = entry.get("every").and_then(Value::as_u64);
        out.push(Probe {
            node: node.to_string(),
            props,
            at: if at.is_none() && every.is_none() {
                Some(u64::MAX)
            } else {
                at
            },
            every,
        });
    }
    Ok(out)
}

/// Reads a probe: engine property first, then the script variable of that
/// name — the live value, never ClassDB's declared default.
fn read_probe(tree: &SceneTree, probe: &Probe) -> String {
    let id = match resolve(tree, running_scene_root(tree), &probe.node) {
        Ok(id) => id,
        Err(e) => return format!("{}: {e}", probe.node),
    };
    let node = match tree.get_node(id) {
        Some(n) => n,
        None => return format!("{}: <freed>", probe.node),
    };
    let values: Vec<String> = probe
        .props
        .iter()
        .map(|prop| {
            // A property the node never stored reads as ClassDB's default, as it
            // would in Godot, rather than as null.
            let value = match tree.get_script(id).and_then(|s| s.get_property(prop)) {
                Some(v) => v,
                None => match node.get_property(prop) {
                    // `global_position` is DERIVED in Godot (the parent chain,
                    // `top_level` included), never stored, so it read null here
                    // and a `--watch <camera>:global_position` compared nothing.
                    Variant::Nil if prop == "global_position" => {
                        let class = node.class_name();
                        if gdobject::class_db::is_parent_class(class, "Node3D") {
                            Variant::Vector3(gdscene::node3d::get_global_transform(tree, id).origin)
                        } else if gdobject::class_db::is_parent_class(class, "Node2D") {
                            Variant::Vector2(gdscene::node2d::get_global_position(tree, id))
                        } else {
                            Variant::Nil
                        }
                    }
                    Variant::Nil => {
                        registered_default(node.class_name(), prop).unwrap_or(Variant::Nil)
                    }
                    v => v,
                },
            };
            format!("{prop}={}", fmt_variant(&value))
        })
        .collect();
    format!("{}: {}", probe.node, values.join(" "))
}

/// Godot's base window size from the project, else Godot's default.
fn project_view_size(s: &EditorState) -> (u32, u32) {
    let settings = s
        .run_project_settings
        .as_ref()
        .unwrap_or(&s.project_settings);
    let read = |key: &str| match settings.get(key) {
        Some(gdcore::project_settings::SettingsValue::Int(i)) if *i > 0 => Some(*i as u32),
        _ => None,
    };
    (
        read("display/window/size/viewport_width").unwrap_or(1152),
        read("display/window/size/viewport_height").unwrap_or(648),
    )
}

/// Renders the running game's frame (or, with nothing running, the edited
/// scene as the game would draw it) through the same textured 2D path the
/// editor's Play view uses.
fn render_game_png(s: &mut EditorState, width: u32, height: u32) -> Result<Vec<u8>, String> {
    let st = &mut *s;
    let settings = st
        .run_project_settings
        .clone()
        .unwrap_or_else(|| st.project_settings.clone());
    let tree = match st.run_main_loop.as_mut() {
        Some(ml) => ml.tree_mut(),
        None => &mut st.scene_tree,
    };
    if crate::scene_renderer::scene_has_3d_content(tree) {
        // The Play view's own 3D entry point (pat-p3p1m), so a screenshot is
        // the frame the editor's Game tab draws for this tree: scene camera,
        // game environment, HUD composited over it. Without it a 3D game had
        // no frame-exact picture at all, so nothing could be diffed against
        // Godot's (pat-xggzs).
        let clear = settings.default_clear_color();
        let img = crate::play_3d::prepare_play_3d_frame(
            tree,
            &st.editor_viewport_3d,
            st.runtime_camera_override,
            clear,
            width,
            height,
        )
        .render();
        return Ok(gdrender2d::export::encode_png_bgra(&img));
    }
    let fb = crate::viewport_2d_render::render_play_frame(
        tree,
        width,
        height,
        &mut st.texture_cache,
        &settings,
    );
    Ok(gdrender2d::export::encode_png(&fb))
}

fn tool_screenshot(ctx: &Ctx, args: &Value) -> Result<Output, String> {
    if arg_str(args, "view") == Some("player") {
        return player_view(ctx);
    }
    let mut s = ctx.state.lock().unwrap();
    let (w, h) = project_view_size(&s);
    let width = arg_u64(args, "width")
        .map(|v| v as u32)
        .unwrap_or(w)
        .clamp(16, 4096);
    let height = arg_u64(args, "height")
        .map(|v| v as u32)
        .unwrap_or(h)
        .clamp(16, 4096);
    let label = if s.is_running {
        format!("game frame {}", s.runtime_frame_count)
    } else {
        "edited scene (not running)".to_string()
    };
    let png = render_game_png(&mut s, width, height)?;
    drop(s);
    let mut out = Output::text(format!("{label}, {width}x{height}"));
    out.image(&png);
    Ok(out)
}

/// The exact frame the browser's Game view was last sent. The plain
/// screenshot renders the game afresh at the project's window size, so it can
/// look right while the browser shows something else — the game drawn 1:1 in
/// a corner of a larger panel, say.
fn player_view(ctx: &Ctx) -> Result<Output, String> {
    let (status, png) = ctx.api.request("GET", "/api/viewport/png", None)?;
    if status != 200 || png.len() < 24 || &png[1..4] != b"PNG" {
        return Err("no frame has been published to the browser view yet: the editor's render loop (the `editor` example) draws it, and a bare server has none".to_string());
    }
    let width = u32::from_be_bytes([png[16], png[17], png[18], png[19]]);
    let height = u32::from_be_bytes([png[20], png[21], png[22], png[23]]);
    let mut out = Output::text(format!(
        "player view: the frame the browser was last sent, {width}x{height} (frame #{})",
        ctx.cache.frame_seq()
    ));
    out.image(&png);
    Ok(out)
}

/// Everything `diagnose` measures, read at one moment.
#[derive(Debug, Clone, Default)]
struct Pipeline {
    debug_build: bool,
    running: bool,
    paused: bool,
    frame: u64,
    target_fps: f64,
    sim_fps: usize,
    render_fps: usize,
    render_ms: f64,
    encode_ms: f64,
    frame_size: (u32, u32),
    served_fps: usize,
    waiting: usize,
    last_served: Option<Duration>,
    /// Refused requests in the last 10 s, per route.
    refused: Vec<(String, usize)>,
    /// The running game's root window: its size and stretch.
    window: Option<(gdcore::math::Vector2, ContentScaleConfig)>,
    /// The stretch project.godot asks for.
    project_stretch: ContentScaleConfig,
    held_keys: Vec<String>,
    fallback_key: Option<String>,
}

fn pipeline(ctx: &Ctx) -> Pipeline {
    let (served_fps, last_served) = ctx.cache.served_per_second();
    let waiting = ctx.cache.waiting_viewers();
    let s = ctx.state.lock().unwrap();
    let settings = s
        .run_project_settings
        .as_ref()
        .unwrap_or(&s.project_settings);
    let mut held_keys: Vec<String> = s.pressed_keys.iter().cloned().collect();
    held_keys.sort();
    Pipeline {
        debug_build: crate::editor_server::build_profile() == "debug",
        running: s.is_running,
        paused: s.is_paused,
        frame: s.runtime_frame_count,
        target_fps: if s.delta_time > 0.0 {
            1.0 / s.delta_time
        } else {
            60.0
        },
        sim_fps: s.frame_stats.sim_fps(),
        render_fps: s.frame_stats.render_fps(),
        render_ms: s.frame_stats.render_ms(),
        encode_ms: crate::editor_server::PERF_LAST_ENCODE_US
            .load(std::sync::atomic::Ordering::Relaxed) as f64
            / 1000.0,
        frame_size: (s.viewport_width, s.viewport_height),
        served_fps,
        waiting,
        last_served,
        refused: s.frame_stats.refused_within(Duration::from_secs(10)),
        window: s.run_main_loop.as_ref().map(|ml| {
            let w = ml.tree().root_window();
            (w.size, w.config)
        }),
        project_stretch: settings.content_scale_config(),
        held_keys,
        fallback_key: s.input_fallback_key().map(str::to_string),
    }
}

/// `canvas_items/keep at 640x360`, or `disabled`, as project.godot spells it.
fn describe_stretch(c: &ContentScaleConfig) -> String {
    let mode = match c.mode {
        ContentScaleMode::Disabled => return "disabled".to_string(),
        ContentScaleMode::CanvasItems => "canvas_items",
        ContentScaleMode::Viewport => "viewport",
    };
    let aspect = match c.aspect {
        ContentScaleAspect::Ignore => "ignore",
        ContentScaleAspect::Keep => "keep",
        ContentScaleAspect::KeepWidth => "keep_width",
        ContentScaleAspect::KeepHeight => "keep_height",
        ContentScaleAspect::Expand => "expand",
    };
    format!(
        "{mode}/{aspect} at {}x{}",
        c.base_size.x as u32, c.base_size.y as u32
    )
}

fn frame_bytes(p: &Pipeline) -> String {
    let bytes = p.frame_size.0 as f64 * p.frame_size.1 as f64 * 4.0 + 54.0;
    if bytes >= 1024.0 * 1024.0 {
        format!("{:.1} MB", bytes / (1024.0 * 1024.0))
    } else {
        format!("{:.0} KB", bytes / 1024.0)
    }
}

/// What the measurements say is wrong, most likely cause first.
fn findings(p: &Pipeline) -> Vec<String> {
    let mut out = Vec::new();
    let target = p.target_fps.round() as usize;
    // Rates are over the last second, so judge them after a full second.
    let measured = p.running && !p.paused && p.frame >= target as u64;
    if p.running && p.debug_build && p.render_ms > 8.0 {
        out.push(format!(
            "debug build: {:.1} ms to render a frame. A release editor renders ~10x faster — judge frame rate on one (scripts/editor-bg.sh start --release).",
            p.render_ms
        ));
    }
    if measured && p.sim_fps + 3 < target {
        out.push(format!(
            "game-bound: {} of {target} game frames stepped last second — one frame's scripts and physics (or the render holding the editor lock) take longer than {:.1} ms.",
            p.sim_fps,
            1000.0 / p.target_fps
        ));
    }
    if measured && p.render_fps + 3 < p.sim_fps.min(target) {
        out.push(format!(
            "render-bound: {} of {} game frames rendered, {:.1} ms each ({:.1} ms of it encoding) at {}x{}.",
            p.render_fps,
            p.sim_fps.min(target),
            p.render_ms,
            p.encode_ms,
            p.frame_size.0,
            p.frame_size.1
        ));
    }
    if measured && p.render_fps > 0 {
        if p.served_fps == 0 {
            out.push("no viewer is pulling frames: the Game view is not open, its tab is hidden, or the page predates the frame pump (reload it).".to_string());
        } else if p.served_fps * 10 < p.render_fps * 8 {
            out.push(format!(
                "stream-bound: the browser received {} of {} rendered frames/s — the page fetches on a timer, requests are refused, or decoding {} per frame is too slow.",
                p.served_fps,
                p.render_fps,
                frame_bytes(p)
            ));
        }
    }
    if !p.refused.is_empty() {
        let total: usize = p.refused.iter().map(|(_, n)| n).sum();
        let routes: Vec<String> = p
            .refused
            .iter()
            .map(|(route, n)| format!("{route} ×{n}"))
            .collect();
        out.push(format!(
            "rate limiter refused {total} request(s) in the last 10 s ({}): a refused frame or key request looks like a frozen view or a dropped key.",
            routes.join(", ")
        ));
    }
    if let Some((size, running)) = &p.window {
        let project = &p.project_stretch;
        if running.mode == ContentScaleMode::Disabled && project.mode != ContentScaleMode::Disabled
        {
            out.push(format!(
                "stretch ignored: project.godot asks for {}, but the running window has stretching off, so the game draws 1:1 in a corner of the {}x{} view.",
                describe_stretch(project),
                size.x as u32,
                size.y as u32
            ));
        } else if running.mode != project.mode
            || running.aspect != project.aspect
            || running.base_size != project.base_size
        {
            out.push(format!(
                "stretch differs: the running window uses {} but project.godot asks for {}.",
                describe_stretch(running),
                describe_stretch(project)
            ));
        }
    }
    if let Some(key) = &p.fallback_key {
        out.push(format!(
            "input fallback: `{key}` is held and has no engine keycode, so scripts see the editor's built-in key map instead of the project's actions — controls look dead until it is released (a key whose keyup the OS swallowed?)."
        ));
    }
    out
}

fn tool_diagnose(ctx: &Ctx) -> Result<Output, String> {
    let p = pipeline(ctx);
    let mut out = String::new();
    out.push_str(&format!(
        "build: {}\n",
        if p.debug_build { "debug" } else { "release" }
    ));
    let target = p.target_fps.round() as usize;
    if p.running {
        out.push_str(&format!(
            "game: running{} at frame {} — {} of {target} frames/s stepped\n",
            if p.paused { " (paused)" } else { "" },
            p.frame,
            p.sim_fps
        ));
    } else {
        out.push_str("game: stopped\n");
    }
    out.push_str(&format!(
        "render: {} frames/s, {:.1} ms each (encode {:.1} ms) at {}x{}, {} per frame\n",
        p.render_fps,
        p.render_ms,
        p.encode_ms,
        p.frame_size.0,
        p.frame_size.1,
        frame_bytes(&p)
    ));
    out.push_str(&format!(
        "viewers: {} frames/s served, {} waiting for the next frame, {}\n",
        p.served_fps,
        p.waiting,
        match p.last_served {
            Some(age) => format!("last one {} ms ago", age.as_millis()),
            None => "none served yet".to_string(),
        }
    ));
    out.push_str(&format!(
        "refused (last 10 s): {}\n",
        if p.refused.is_empty() {
            "none".to_string()
        } else {
            p.refused
                .iter()
                .map(|(route, n)| format!("{route} ×{n}"))
                .collect::<Vec<_>>()
                .join(", ")
        }
    ));
    match &p.window {
        Some((size, running)) => out.push_str(&format!(
            "window: {}x{}, stretch {} · project.godot: {}\n",
            size.x as u32,
            size.y as u32,
            describe_stretch(running),
            describe_stretch(&p.project_stretch)
        )),
        None => out.push_str(&format!(
            "window: (no game running) · project.godot: {}\n",
            describe_stretch(&p.project_stretch)
        )),
    }
    out.push_str(&format!(
        "input: {}{}\n",
        if p.held_keys.is_empty() {
            "no keys held".to_string()
        } else {
            format!("held {}", p.held_keys.join(", "))
        },
        match &p.fallback_key {
            Some(key) => format!(" — fallback snapshot in force (`{key}`)"),
            None => String::new(),
        }
    ));
    let found = findings(&p);
    if found.is_empty() {
        out.push_str("findings: none\n");
    } else {
        out.push_str("findings:\n");
        for f in found {
            out.push_str(&format!("- {f}\n"));
        }
    }
    Ok(Output::text(out))
}

fn tool_stop(ctx: &Ctx) -> Result<Output, String> {
    if !ctx.state.lock().unwrap().is_running {
        return Ok(Output::text("not running"));
    }
    ctx.api.post("/api/runtime/stop", json!({}))?;
    Ok(Output::text("stopped"))
}

fn tool_play(ctx: &Ctx, args: &Value) -> Result<Output, String> {
    if ctx.state.lock().unwrap().is_running {
        ctx.api.post("/api/runtime/stop", json!({}))?;
    }
    // Taken before Play: booting runs `_ready`, and its prints are frame 0.
    let log_start = ctx.state.lock().unwrap().game_log.next_seq();
    // A fixed seed: an agent's playtest is frame-exact, and that includes the
    // random stream — the same timeline must replay the same game. (A human's
    // Play is randomized at startup, as Godot's is.)
    // `audio: true` opens the real output device, so `audio` can measure what
    // the game sounds like; off by default, so a playtest stays silent.
    let started = ctx.api.post(
        "/api/runtime/play",
        json!({"paused": true, "seed": 42, "audio": arg_bool(args, "audio", false)}),
    )?;
    if started.get("ok").and_then(Value::as_bool) == Some(false) {
        return Err(format!("play refused: {started}"));
    }
    let keep_running = arg_bool(args, "keep_running", false);
    let result = frames_on_game_thread(ctx, args, "play", Some(log_start));
    if !keep_running && ctx.state.lock().unwrap().is_running {
        let _ = ctx.api.post("/api/runtime/stop", json!({}));
    }
    let mut out = result?;
    if let Some(Value::Object(first)) = out.content.first_mut() {
        if let Some(Value::String(text)) = first.get_mut("text") {
            text.push_str(if keep_running {
                "game left paused — continue with `step`, `tree running=true`, `inspect running=true`, or `stop`\n"
            } else {
                "game stopped\n"
            });
        }
    }
    Ok(out)
}

fn tool_audio(ctx: &Ctx, args: &Value) -> Result<Output, String> {
    let secs = args.get("secs").and_then(Value::as_f64).unwrap_or(5.0);
    let v = ctx.api.call("GET", &format!("/api/runtime/audio?secs={secs}"), None)?;
    if v["device"].is_null() {
        return Err(v["error"]
            .as_str()
            .unwrap_or("no audio device is open")
            .to_string());
    }
    let d = &v["device"];
    let a = &v["analysis"];
    let n = |x: &Value| x.as_f64().unwrap_or(0.0);
    let mut text = format!(
        "device: {} Hz x{} · callbacks {} · underruns {} · priming waits {} · queue {} frames (max {}) · dropped {} · clipped {}\n\
         last {:.1}s: peak {:.3} · rms {:.3} · clicks {} · gaps {} · clipped {}\n",
        n(&d["sample_rate"]), n(&d["channels"]), n(&d["callbacks"]), n(&d["underruns"]),
        n(&d["priming_callbacks"]), n(&d["queued_frames"]), n(&d["max_queued_frames"]),
        n(&d["dropped_samples"]), n(&d["clipped_samples"]),
        n(&a["seconds"]), n(&a["peak"]), n(&a["rms"]), n(&a["click_count"]),
        n(&a["gap_count"]), n(&a["clipped_samples"]),
    );
    for c in a["clicks"].as_array().into_iter().flatten().take(12) {
        text.push_str(&format!("  click at {:.3}s ({:.0}x local curvature)\n", n(&c["t"]), n(&c["severity"])));
    }
    for g in a["gaps"].as_array().into_iter().flatten().take(12) {
        text.push_str(&format!("  gap at {:.3}s ({} frames silent)\n", n(&g["t"]), n(&g["frames"])));
    }
    // Findings, so the numbers read as a diagnosis.
    if n(&d["underruns"]) > 0.0 {
        text.push_str("finding: the device ran dry mid-callback — the game is not feeding audio fast/steadily enough (gaps are audible)\n");
    }
    if n(&a["clipped_samples"]) > 0.0 || n(&a["peak"]) > 0.99 {
        text.push_str("finding: output is clipping — the mix exceeds full scale (harsh distortion on loud passages)\n");
    }
    // ⚠️ Music has sharp transients of its own: Dodge the Creeps' source OGG
    // shows 12-45x "clicks" (drum hits) every few hundred ms. A real seam or
    // cut is orders of magnitude sharper (a dropout measured 36000x), so only
    // a jump past HARD_CLICK is a finding; the rest is listed for comparison.
    const HARD_CLICK: f64 = 200.0;
    let hard = a["clicks"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|c| n(&c["severity"]) > HARD_CLICK)
        .count();
    if hard > 0 && n(&d["underruns"]) == 0.0 {
        text.push_str(&format!("finding: {hard} hard discontinuities (>{HARD_CLICK}x) with no underruns — the MIX itself jumps (resampling, loop seam, block seam)\n"));
    } else if n(&a["click_count"]) > 0.0 {
        text.push_str("note: only soft clicks (<200x) — consistent with the music's own transients; compare against the source with gdaudio::diagnostics::analyze\n");
    }
    if let Some(path) = arg_str(args, "save") {
        let (status, wav) = ctx.api.request("GET", &format!("/api/runtime/audio?secs={secs}&wav=1"), None)?;
        if status != 200 {
            return Err(format!("capture failed: HTTP {status}"));
        }
        std::fs::write(path, &wav).map_err(|e| format!("cannot write {path}: {e}"))?;
        text.push_str(&format!("saved {} bytes to {path}\n", wav.len()));
    }
    Ok(Output::text(text))
}

fn tool_step(ctx: &Ctx, args: &Value) -> Result<Output, String> {
    {
        let mut s = ctx.state.lock().unwrap();
        if !s.is_running {
            return Err(
                "the game is not running — use `play` (with keep_running to keep it)".to_string(),
            );
        }
        s.is_paused = true;
    }
    frames_on_game_thread(ctx, args, "step", None)
}

/// [`run_frames`] on the runtime thread, where all game code runs (see
/// `editor_server::run_on_game_thread`): stepping here, on the MCP connection
/// thread, would put the game's frames on a different thread from its
/// `_ready`, and every object a script created there would be gone.
fn frames_on_game_thread(
    ctx: &Ctx,
    args: &Value,
    verb: &'static str,
    log_from: Option<u64>,
) -> Result<Output, String> {
    let ctx2 = ctx.clone();
    let args = args.clone();
    crate::editor_server::run_on_game_thread(&ctx.state, move |_| {
        run_frames(&ctx2, &args, verb, log_from)
    })
}

/// The frame loop shared by `play` and `step`: the game must already be
/// running and paused.
fn run_frames(
    ctx: &Ctx,
    args: &Value,
    verb: &str,
    log_from: Option<u64>,
) -> Result<Output, String> {
    let frames = arg_u64(args, "frames")
        .unwrap_or(60)
        .min(MAX_FRAMES_PER_CALL);
    let (known_actions, base_frame, log_start, view) = {
        let s = ctx.state.lock().unwrap();
        let map = s.build_play_input_map();
        let mut actions: Vec<String> = map.actions().cloned().collect();
        actions.sort();
        let log_start = log_from.unwrap_or_else(|| s.game_log.next_seq());
        (
            actions,
            s.runtime_frame_count,
            log_start,
            project_view_size(&s),
        )
    };
    let inputs = parse_inputs(args, &known_actions)?;
    let probes = parse_probes(args)?;
    let mut shots: Vec<u64> = args
        .get("screenshots")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_u64)
        .map(|f| f.min(frames))
        .collect();
    shots.sort_unstable();
    shots.dedup();
    if shots.len() > MAX_SCREENSHOTS {
        return Err(format!("at most {MAX_SCREENSHOTS} screenshots per call"));
    }
    let width = arg_u64(args, "width")
        .map(|v| v as u32)
        .unwrap_or(view.0)
        .clamp(16, 4096);
    let height = arg_u64(args, "height")
        .map(|v| v as u32)
        .unwrap_or(view.1)
        .clamp(16, 4096);

    let wall = Instant::now();
    let mut probe_lines: Vec<String> = Vec::new();
    let mut images: Vec<(u64, Vec<u8>)> = Vec::new();
    let mut ended_early: Option<String> = None;
    let mut stepped = 0u64;

    // Frame 0: before anything steps.
    {
        let mut s = ctx.state.lock().unwrap();
        capture(
            &mut s,
            0,
            base_frame,
            &probes,
            &shots,
            width,
            height,
            &mut probe_lines,
            &mut images,
        );
    }
    // What is held right now, per input, so the cleanup below releases exactly
    // that however the loop ends (a quit, a breakpoint, a failed press).
    let mut held = vec![false; inputs.len()];
    let mut input_failure: Option<String> = None;
    'frames: for frame in 1..=frames {
        // Key and click inputs go through the same routes the browser's
        // keyboard and mouse use, so the game sees exactly what a player's
        // would. Actions are pressed directly on the engine's InputState.
        for (i, input) in inputs.iter().enumerate() {
            if input.start == frame {
                if let Err(e) = press(ctx, input, true) {
                    input_failure = Some(format!(
                        "input {} failed at frame {}: {e}",
                        i + 1,
                        base_frame + frame
                    ));
                    break 'frames;
                }
                held[i] = true;
            }
        }
        {
            let mut s = ctx.state.lock().unwrap();
            if !s.is_running {
                ended_early = Some(format!(
                    "the game stopped itself before frame {}",
                    base_frame + frame
                ));
                break;
            }
            // The runtime driver's own guard (pat-mgixk.14): a frame parked
            // at a breakpoint must not be stepped past, or every further step
            // parks another suspended frame and the session never converges.
            if gdscript_interop::debug_session::is_paused() || s.runtime_suspended {
                ended_early = Some(format!(
                    "the game is {} at frame {} — stepping stopped",
                    if s.runtime_suspended {
                        "suspended"
                    } else {
                        "paused in the debugger"
                    },
                    base_frame + stepped
                ));
                break;
            }
            // 🔑 Probes read the tree at the deferred-call flush, where the
            // Godot reference capture reads it (a `call_deferred` from its
            // last-priority `_process`), not after the whole step: a queued
            // scene change and queued frees land AFTER that point in Godot
            // (pat-la27y). See `MainLoop::set_deferred_flush_observer`.
            let flushed = install_flush_probes(&mut s, frame, base_frame, &probes);
            crate::editor_server::advance_running_main_loop(&mut s);
            let flushed = take_flush_probes(&mut s, flushed);
            stepped += 1;
            if !s.is_running {
                ended_early = Some(format!("the game quit during frame {}", base_frame + frame));
                break;
            }
            let read_after: &[Probe] = match flushed {
                Some(lines) => {
                    probe_lines.extend(lines);
                    &[]
                }
                None => &probes,
            };
            capture(
                &mut s,
                frame,
                base_frame,
                read_after,
                &shots,
                width,
                height,
                &mut probe_lines,
                &mut images,
            );
        }
        for (i, input) in inputs.iter().enumerate() {
            if input.end == frame && held[i] {
                held[i] = false;
                if let Err(e) = press(ctx, input, false) {
                    input_failure = Some(format!(
                        "input {} failed at frame {}: {e}",
                        i + 1,
                        base_frame + frame
                    ));
                    break 'frames;
                }
            }
        }
    }
    // Release anything still held so the next call starts clean.
    for (i, input) in inputs.iter().enumerate() {
        if held[i] {
            let _ = press(ctx, input, false);
        }
    }
    if let Some(why) = input_failure {
        ended_early = Some(why);
    }
    // Probes with no frame given read the final state.
    if probes.iter().any(|p| p.at == Some(u64::MAX)) {
        let s = ctx.state.lock().unwrap();
        if let Some(ml) = s.run_main_loop.as_ref() {
            for p in probes.iter().filter(|p| p.at == Some(u64::MAX)) {
                probe_lines.push(format!(
                    "f{} {}",
                    base_frame + stepped,
                    read_probe(ml.tree(), p)
                ));
            }
        }
    }

    let s = ctx.state.lock().unwrap();
    let entries: Vec<&GameLogEntry> = s.game_log.since(log_start).collect();
    let errors = entries
        .iter()
        .filter(|e| e.kind == GameLogKind::Error)
        .count();
    let prints = entries.len() - errors;
    let mut text = format!(
        "{verb}: {} frame(s) (f{}→f{}, {:.2}s game time) in {} ms · {prints} print(s), {errors} error(s)\n",
        stepped,
        base_frame,
        base_frame + stepped,
        stepped as f64 * s.delta_time,
        wall.elapsed().as_millis(),
    );
    if let Some(why) = &ended_early {
        text.push_str(&format!("⚠ {why}\n"));
    }
    if !entries.is_empty() {
        text.push_str("output:\n");
        text.push_str(&collapse_log(&entries, REPORT_LOG_LINES));
    }
    if !probe_lines.is_empty() {
        text.push_str("probes:\n");
        for line in &probe_lines {
            text.push_str(&format!("  {line}\n"));
        }
    }
    if !images.is_empty() {
        let frames: Vec<String> = images.iter().map(|(f, _)| format!("f{f}")).collect();
        text.push_str(&format!(
            "screenshots: {} ({width}x{height}), in order below\n",
            frames.join(", ")
        ));
    }
    drop(s);
    if ended_early
        .as_deref()
        .is_some_and(|why| why.starts_with("input "))
    {
        return Err(text);
    }
    let mut out = Output::text(text);
    for (_, png) in &images {
        out.image(png);
    }
    Ok(out)
}

#[allow(clippy::too_many_arguments)]
/// What [`install_flush_probes`] hands [`take_flush_probes`]: the lines the
/// observer wrote, or `None` when no probe is due this frame.
type FlushProbes = Option<std::sync::Arc<std::sync::Mutex<Option<Vec<String>>>>>;

/// Installs a deferred-flush observer on the running loop that reads every
/// probe due at `frame` (pat-la27y). Returns the slot it writes into.
fn install_flush_probes(
    s: &mut EditorState,
    frame: u64,
    base_frame: u64,
    probes: &[Probe],
) -> FlushProbes {
    let due: Vec<Probe> = probes.iter().filter(|p| p.due(frame)).cloned().collect();
    if due.is_empty() {
        return None;
    }
    let ml = s.run_main_loop.as_mut()?;
    let slot = std::sync::Arc::new(std::sync::Mutex::new(None));
    let out = slot.clone();
    ml.set_deferred_flush_observer(Some(Box::new(move |tree: &SceneTree| {
        // The LAST flush of the step wins, should one advance run several.
        let lines = due
            .iter()
            .map(|p| format!("f{} {}", base_frame + frame, read_probe(tree, p)))
            .collect();
        *out.lock().unwrap() = Some(lines);
    })));
    Some(slot)
}

/// Removes the observer [`install_flush_probes`] set and returns what it read,
/// or `None` (read after the step instead) when the step never reached a
/// flush.
fn take_flush_probes(s: &mut EditorState, slot: FlushProbes) -> Option<Vec<String>> {
    let slot = slot?;
    if let Some(ml) = s.run_main_loop.as_mut() {
        ml.set_deferred_flush_observer(None);
    }
    let lines = slot.lock().unwrap().take();
    lines
}

fn capture(
    s: &mut EditorState,
    frame: u64,
    base_frame: u64,
    probes: &[Probe],
    shots: &[u64],
    width: u32,
    height: u32,
    probe_lines: &mut Vec<String>,
    images: &mut Vec<(u64, Vec<u8>)>,
) {
    if let Some(ml) = s.run_main_loop.as_ref() {
        for p in probes.iter().filter(|p| p.due(frame)) {
            probe_lines.push(format!(
                "f{} {}",
                base_frame + frame,
                read_probe(ml.tree(), p)
            ));
        }
    }
    if shots.contains(&frame) {
        match render_game_png(s, width, height) {
            Ok(png) => images.push((base_frame + frame, png)),
            Err(e) => probe_lines.push(format!("f{} screenshot failed: {e}", base_frame + frame)),
        }
    }
}

/// `MouseEvent.button` for the primary button, the numbering the editor's
/// `/api/runtime/input/mouse_*` routes take.
const DOM_LEFT_BUTTON: u8 = 0;

fn press(ctx: &Ctx, input: &TimedInput, down: bool) -> Result<(), String> {
    match &input.kind {
        InputKind::Action(action) => {
            let mut s = ctx.state.lock().unwrap();
            if let Some(ml) = s.run_main_loop.as_mut() {
                if down {
                    ml.input_state_mut().action_press(action, 1.0);
                } else {
                    ml.input_state_mut().action_release(action);
                }
            }
            Ok(())
        }
        InputKind::Key(key) => {
            let route = if down {
                "/api/runtime/input/key_down"
            } else {
                "/api/runtime/input/key_up"
            };
            ctx.api.post(route, json!({"key": key})).map(|_| ())
        }
        // 🔑 The input routes take the browser's DOM button numbering (0 =
        // left, 1 = MIDDLE, 2 = right). This sent `1` — Godot's number for the
        // left button — so every agent "click" was a middle click, which a
        // Button ignores: a playtest could never press a game's Start button.
        // The position rides in the press and the release themselves.
        //
        // The point is in the GAME's coordinates — the space its probes, node
        // positions and scripts use — and is mapped through the running
        // window's stretch to the window pixel the input routes expect. Sent
        // raw, a click aimed at a 480x720 game's Start button landed elsewhere
        // whenever the window was any other size (the editor's Game panel).
        InputKind::Click(x, y) => {
            let route = if down {
                "/api/runtime/input/mouse_down"
            } else {
                "/api/runtime/input/mouse_up"
            };
            let (wx, wy) = {
                let s = ctx.state.lock().unwrap();
                match s.run_main_loop.as_ref() {
                    Some(ml) => {
                        let p = ml
                            .tree()
                            .root_window()
                            .stretch_transform()
                            .xform(gdcore::math::Vector2::new(*x as f32, *y as f32));
                        (p.x as f64, p.y as f64)
                    }
                    None => (*x as f64, *y as f64),
                }
            };
            ctx.api
                .post(route, json!({"button": DOM_LEFT_BUTTON, "x": wx, "y": wy}))
                .map(|_| ())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A game in its second second of play, rendered and watched at full rate.
    fn live() -> Pipeline {
        Pipeline {
            running: true,
            frame: 600,
            target_fps: 60.0,
            sim_fps: 60,
            render_fps: 60,
            render_ms: 2.5,
            served_fps: 58,
            frame_size: (995, 843),
            ..Pipeline::default()
        }
    }

    fn canvas_items_640x360() -> ContentScaleConfig {
        ContentScaleConfig {
            mode: ContentScaleMode::CanvasItems,
            aspect: ContentScaleAspect::Keep,
            base_size: gdcore::math::Vector2::new(640.0, 360.0),
            ..ContentScaleConfig::default()
        }
    }

    /// pat-d4ghb: `global_position` is derived, never stored, so a probe of it
    /// read `null` and `--watch <camera>:global_position` compared nothing.
    /// It must follow the parent chain — and stop at a `top_level` node.
    #[test]
    fn a_global_position_probe_composes_the_parent_chain() {
        gdobject::class_db::register_all_classes();
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let mut parent = gdscene::node::Node::new("Game", "Node3D");
        parent.set_property(
            "position",
            Variant::Vector3(gdcore::math::Vector3::new(10.0, 0.0, 0.0)),
        );
        let parent = tree.add_child(root, parent).unwrap();
        let mut child = gdscene::node::Node::new("Cam", "Camera3D");
        child.set_property(
            "position",
            Variant::Vector3(gdcore::math::Vector3::new(1.0, 2.0, 3.0)),
        );
        tree.add_child(parent, child).unwrap();
        let probe = Probe {
            node: "/root/Game/Cam".into(),
            props: vec!["global_position".into()],
            at: None,
            every: None,
        };
        assert_eq!(
            read_probe(&tree, &probe),
            "/root/Game/Cam: global_position=Vector3(11, 2, 3)"
        );
    }

    #[test]
    fn a_pipeline_that_keeps_up_has_no_findings() {
        assert_eq!(findings(&live()), Vec::<String>::new());
    }

    /// The lag a person reported in the browser: frames fetched on a 200 ms
    /// timer, a PNG deflated per frame, a debug build. Each is named from what
    /// was measured.
    #[test]
    fn each_bottleneck_is_named_from_its_measurement() {
        let polled = Pipeline {
            served_fps: 5,
            render_fps: 14,
            render_ms: 40.0,
            debug_build: true,
            ..live()
        };
        let found = findings(&polled).join("\n");
        assert!(found.contains("debug build: 40.0 ms"), "{found}");
        assert!(found.contains("render-bound: 14 of 60"), "{found}");
        assert!(
            found.contains("stream-bound: the browser received 5 of 14"),
            "{found}"
        );

        let slow_game = Pipeline {
            sim_fps: 30,
            render_fps: 30,
            served_fps: 30,
            ..live()
        };
        let found = findings(&slow_game).join("\n");
        assert!(found.contains("game-bound: 30 of 60"), "{found}");
        assert!(
            !found.contains("render-bound"),
            "the render keeps up with the game it is given: {found}"
        );

        let unwatched = Pipeline {
            served_fps: 0,
            ..live()
        };
        assert!(findings(&unwatched)
            .join("\n")
            .contains("no viewer is pulling frames"));
    }

    #[test]
    fn rates_are_not_judged_before_a_full_second_of_unpaused_play() {
        let warming_up = Pipeline {
            frame: 20,
            sim_fps: 20,
            render_fps: 20,
            served_fps: 0,
            ..live()
        };
        assert_eq!(findings(&warming_up), Vec::<String>::new());
        let paused = Pipeline {
            paused: true,
            sim_fps: 0,
            render_fps: 0,
            served_fps: 0,
            ..live()
        };
        assert_eq!(findings(&paused), Vec::<String>::new());
    }

    #[test]
    fn a_window_that_ignores_the_projects_stretch_is_named() {
        let view = gdcore::math::Vector2::new(995.0, 843.0);
        let ignored = Pipeline {
            window: Some((view, ContentScaleConfig::default())),
            project_stretch: canvas_items_640x360(),
            ..live()
        };
        let found = findings(&ignored).join("\n");
        assert!(
            found.contains("stretch ignored: project.godot asks for canvas_items/keep at 640x360"),
            "{found}"
        );
        assert!(found.contains("a corner of the 995x843 view"), "{found}");

        let honoured = Pipeline {
            window: Some((view, canvas_items_640x360())),
            project_stretch: canvas_items_640x360(),
            ..live()
        };
        assert_eq!(findings(&honoured), Vec::<String>::new());

        let expanded = ContentScaleConfig {
            aspect: ContentScaleAspect::Expand,
            ..canvas_items_640x360()
        };
        let differs = Pipeline {
            window: Some((view, expanded)),
            project_stretch: canvas_items_640x360(),
            ..live()
        };
        assert!(findings(&differs)
            .join("\n")
            .contains("stretch differs: the running window uses canvas_items/expand at 640x360"));
    }

    #[test]
    fn refused_requests_and_a_fallback_key_are_named() {
        let p = Pipeline {
            refused: vec![
                ("/api/viewport".to_string(), 37),
                ("/api/runtime/input/key_down".to_string(), 2),
            ],
            fallback_key: Some("§".to_string()),
            ..live()
        };
        let found = findings(&p).join("\n");
        assert!(
            found.contains("refused 39 request(s) in the last 10 s (/api/viewport ×37, /api/runtime/input/key_down ×2)"),
            "{found}"
        );
        assert!(found.contains("input fallback: `§` is held"), "{found}");
    }

    #[test]
    fn game_log_resumes_by_sequence_across_runs() {
        let mut log = GameLog::default();
        log.push(0, GameLogKind::Print, "a".into());
        log.push(3, GameLogKind::Error, "b".into());
        let mark = log.next_seq();
        log.start_run();
        log.push(1, GameLogKind::Print, "c".into());
        let after: Vec<&str> = log.since(mark).map(|e| e.text.as_str()).collect();
        assert_eq!(
            after,
            ["c"],
            "a `since` taken before a restart must not replay the old run"
        );
        assert_eq!(
            log.since(0).count(),
            1,
            "start_run forgets the previous run"
        );
    }

    #[test]
    fn game_log_is_capped() {
        let mut log = GameLog::default();
        for i in 0..(GAME_LOG_CAP + 10) {
            log.push(i as u64, GameLogKind::Print, i.to_string());
        }
        assert_eq!(log.since(0).count(), GAME_LOG_CAP);
        assert_eq!(
            log.since(0).next().unwrap().text,
            "10",
            "the oldest lines go first"
        );
    }

    #[test]
    fn string_values_are_godot_literals_unless_the_property_is_a_string() {
        assert_eq!(
            json_to_variant(&json!("Vector2(1, 2)"), false).unwrap(),
            Variant::Vector2(gdcore::math::Vector2::new(1.0, 2.0))
        );
        assert_eq!(
            json_to_variant(&json!("1.5"), true).unwrap(),
            Variant::String("1.5".into()),
            "Label.text = \"1.5\" must stay text"
        );
        assert_eq!(
            json_to_variant(&json!("\"quoted\""), true).unwrap(),
            Variant::String("quoted".into()),
            "a quoted literal on a string property is unquoted"
        );
        assert_eq!(json_to_variant(&json!(3), false).unwrap(), Variant::Int(3));
        assert_eq!(
            json_to_variant(&json!(3.0), false).unwrap(),
            Variant::Float(3.0)
        );
        assert_eq!(
            json_to_variant(&json!("not a literal"), false).unwrap(),
            Variant::String("not a literal".into()),
            "unparseable text falls back to a String"
        );
    }

    #[test]
    fn diagnostics_locate_a_syntax_error() {
        let diags = script_diagnostics("extends Node\nfunc _ready(:\n\tpass\n");
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert!(diags[0].contains("line 2"), "{diags:?}");
        assert!(script_diagnostics("extends Node\nfunc _ready():\n\tpass\n").is_empty());
    }

    #[test]
    fn repeated_lines_collapse_with_count_and_frame_range() {
        let mut log = GameLog::default();
        log.push(0, GameLogKind::Print, "ready".into());
        for f in 1..=5 {
            log.push(f, GameLogKind::Error, "boom".into());
        }
        log.push(3, GameLogKind::Print, "once".into());
        let entries: Vec<&GameLogEntry> = log.since(0).collect();
        let text = collapse_log(&entries, 10);
        assert_eq!(text, "  f0 ready\n  f1–f5 ×5 ERROR boom\n  f3 once\n");
        assert!(collapse_log(&entries, 1).contains("2 more distinct"));
    }

    #[test]
    fn resource_paths_map_both_ways() {
        assert_eq!(to_disk_path(Some("/p"), "res://a/b.tscn"), "/p/a/b.tscn");
        assert_eq!(to_disk_path(Some("/p/"), "main.tscn"), "/p/main.tscn");
        assert_eq!(to_disk_path(Some("/p"), "/abs/x.tscn"), "/abs/x.tscn");
        assert_eq!(to_res_path(Some("/p"), "/p/a/b.tscn"), "res://a/b.tscn");
        assert_eq!(
            to_res_path(Some("/p"), "/elsewhere/b.tscn"),
            "/elsewhere/b.tscn"
        );
        assert_eq!(pascal_case("level_one"), "LevelOne");
    }
}
