//! pat-s3rrv.27: the editor's Play session actually runs a debugger.
//!
//! Patina had a complete `EngineDebugger` script surface — 23/23 methods
//! dispatched — attached to nothing. `engine_debugger_attach()`,
//! `drain_engine_debugger_messages()` and `take_engine_debugger_break()` had no
//! call sites outside one interpreter test, so during editor Play
//! `EngineDebugger.is_active()` was permanently false, `debug()` was inert, and
//! `send_message()` pushed onto a thread-local `Vec` nothing ever read.
//!
//! # Why this test drives the real HTTP routes
//!
//! The wiring hazard here is *threading*, and only an end-to-end run exposes it.
//! `EngineDebugger` and its message buffer live in `thread_local!`s, while the
//! editor runs scripts on two different threads:
//!
//! - `_ready` is dispatched by `build_play_tree`, inline on the connection
//!   thread serving `POST /api/runtime/play`;
//! - `_process` runs later, from the driver thread or a `/api/runtime/step`
//!   request.
//!
//! So an implementation that attaches once — or drains only per frame — compiles,
//! looks wired, and still reports `is_active() == false` to `_ready` while
//! stranding its messages on a thread nobody drains. Each assertion below pins
//! one of those threads specifically.

use gdeditor::editor_server::{EditorServerHandle, EditorState};
use gdscene::node::Node;
use gdscene::SceneTree;
use gdvariant::Variant;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;
use std::time::Duration;

/// `_ready` sends only when the debugger says it is active, so the message's
/// presence *is* the proof that `is_active()` was true on the connection
/// thread — no separate readback of a script variable needed.
///
/// `_process` sends on its first frame only, from the other thread, and raises a
/// `debug()` break there too.
const DEBUG_GD: &str = "\
extends Node2D
var frames = 0
func _ready():
    if EngineDebugger.is_active():
        EngineDebugger.send_message(\"ready_cap\", [1, 2])
func _process(delta):
    self.frames = self.frames + 1
    if self.frames == 1:
        if EngineDebugger.is_active():
            EngineDebugger.send_message(\"process_cap\", [7])
        EngineDebugger.debug(true, false)
";

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn connect_with_retry(port: u16) -> TcpStream {
    for attempt in 0..20 {
        match TcpStream::connect(format!("127.0.0.1:{port}")) {
            Ok(s) => return s,
            Err(_) if attempt < 19 => thread::sleep(Duration::from_millis(50)),
            Err(e) => panic!("failed to connect: {e}"),
        }
    }
    unreachable!()
}

fn http_request_str(port: u16, request: &str) -> String {
    let mut stream = connect_with_retry(port);
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
    stream.write_all(request.as_bytes()).unwrap();
    let mut resp = Vec::new();
    let _ = stream.read_to_end(&mut resp);
    String::from_utf8_lossy(&resp).to_string()
}

fn http_get(port: u16, path: &str) -> String {
    http_request_str(
        port,
        &format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n"),
    )
}

fn http_post(port: u16, path: &str, body: &str) -> String {
    http_request_str(
        port,
        &format!(
            "POST {path} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        ),
    )
}

fn body_of(resp: &str) -> &str {
    resp.split("\r\n\r\n").nth(1).unwrap_or("")
}

fn json_of(resp: &str) -> serde_json::Value {
    serde_json::from_str(body_of(resp))
        .unwrap_or_else(|e| panic!("response body is not JSON ({e}): {resp}"))
}

/// A one-node scene whose script is a real `.gd` file on disk.
///
/// ⚠️ BOTH roots must be set. `attach_scripts_to_tree` resolves `_script_path`
/// against `texture_cache.project_root()`, not `state.project_root`; setting
/// only the latter attaches no script at all, and every assertion here would
/// fail as "no messages" rather than "the script never loaded".
fn server() -> (EditorServerHandle, u16, tempfile::TempDir) {
    gdobject::class_db::register_2d_classes();

    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("dbg.gd"), DEBUG_GD).expect("write script");

    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let mut main = Node::new("Main", "Node2D");
    main.set_property("_script_path", Variant::String("res://dbg.gd".to_string()));
    tree.add_child(root, main).unwrap();

    let mut state = EditorState::new(tree);
    let root_path = dir.path().to_string_lossy().to_string();
    state.project_root = Some(root_path.clone());
    state.texture_cache.set_project_root(root_path);

    let port = free_port();
    let handle = EditorServerHandle::start(port, state);
    thread::sleep(Duration::from_millis(100));
    (handle, port, dir)
}

/// **The acceptance test.**
#[test]
fn engine_debugger_attaches_to_editor_play_session() {
    let (handle, port, _dir) = server();

    // -- before Play there is no session, so no debugger ----------------------
    let idle = json_of(&http_get(port, "/api/debug/state"));
    assert_eq!(
        idle["attached"], false,
        "a debugger must not be attached before the game runs: {idle}"
    );

    // -- Play -----------------------------------------------------------------
    let played = json_of(&http_post(port, "/api/runtime/play", "{}"));
    assert_eq!(played["ok"], true, "play failed: {played}");

    let after_play = json_of(&http_get(port, "/api/debug/state"));
    assert_eq!(
        after_play["attached"], true,
        "Godot installs a debugger for the whole Play session: {after_play}"
    );
    // ⚠️ Do NOT assert `state == "running"` here. The auto-driver thread starts
    // advancing frames the moment Play returns, so by the time this GET is
    // served `_process` may already have run, called `debug()`, and legitimately
    // moved the session to "paused". Asserting the transient value races the
    // driver and fails intermittently while the feature is working correctly.
    // What is stable is that Play left the session attached and NOT detached.
    assert_ne!(
        after_play["state"], "detached",
        "Play must leave the debugger session live: {after_play}"
    );

    // -- Link 1: `_ready` saw an ACTIVE debugger, on the connection thread -----
    //
    // The script only sends when `is_active()` is true, so finding this message
    // proves both halves: the debugger was live during `build_play_tree`, and
    // the drain ran on that same thread. Draining only per-frame strands it.
    let messages = after_play["messages"]
        .as_array()
        .unwrap_or_else(|| panic!("debug state must carry a messages array: {after_play}"));
    let ready_msg = messages
        .iter()
        .find(|m| m["name"] == "ready_cap")
        .unwrap_or_else(|| {
            panic!(
                "`_ready` must observe is_active()==true and its send_message must reach the \
                 editor; got {messages:?}. Empty here means the debugger was attached only on \
                 the stepping thread, or the message was drained on the wrong one."
            )
        });
    assert_eq!(
        ready_msg["args"],
        serde_json::json!(["1", "2"]),
        "the capture's arguments must survive: {ready_msg}"
    );

    // -- Link 2: the other thread works too ------------------------------------
    //
    // `_process` runs from the frame-advance path, a different thread from the
    // one that served Play. A fix that only handled `_ready` passes link 1 and
    // fails here.
    //
    // ⚠️ `/api/runtime/step` is the MANUAL step and rejects with 400 "not
    // paused" unless the session is paused first — stepping a free-running game
    // would race the driver thread. Without the pause the step is a no-op and
    // this link fails looking exactly like "the drain lost the message".
    let paused = json_of(&http_post(port, "/api/runtime/pause", ""));
    assert_eq!(
        paused["paused"], true,
        "the route TOGGLES, so assert the resulting state rather than just `ok` — \
         landing on `false` here would silently re-break the step below: {paused}"
    );
    let stepped_resp = json_of(&http_post(port, "/api/runtime/step", ""));
    assert_eq!(
        stepped_resp["ok"], true,
        "manual step rejected — the frame never advanced, so any message \
         assertion below would be meaningless: {stepped_resp}"
    );
    let after_step = json_of(&http_get(port, "/api/debug/state"));
    let stepped = after_step["messages"].as_array().unwrap();
    assert!(
        stepped.iter().any(|m| m["name"] == "process_cap"),
        "a send_message from `_process` must reach the editor after a frame step; got {stepped:?}"
    );
    assert!(
        stepped.iter().any(|m| m["name"] == "ready_cap"),
        "and the earlier capture must not be lost when the next drain runs: {stepped:?}"
    );

    // -- Link 3: `debug()` is no longer inert ----------------------------------
    assert_eq!(
        after_step["state"], "paused",
        "EngineDebugger.debug() must put the session into its paused state: {after_step}"
    );
    let errors = json_of(&http_get(port, "/api/debug/errors"));
    let text = errors.to_string();
    assert!(
        text.contains("broke"),
        "the break must be recorded where the editor can show it: {errors}"
    );

    // -- Link 3b: the break is a real, resumable suspension --------------------
    //
    // `debug()` publishes the session's live pause (so the runtime driver
    // stops stepping, as Godot's debug loop suspends the game), and the
    // editor's Continue must release it. A `debug()` break has no parked
    // frame to wake, so Continue drops the pause rather than leaving the game
    // frozen forever.
    let again = json_of(&http_get(port, "/api/debug/state"));
    assert_eq!(
        again["state"], "paused",
        "the debug() pause must survive a second read, not flip back to running: {again}"
    );
    let cont = json_of(&http_post(port, "/api/debug/continue", ""));
    assert_eq!(
        cont["ok"], true,
        "continue must find the live pause: {cont}"
    );
    assert_eq!(
        cont["paused"], false,
        "Continue must resume a debug() break (it has no parked frame): {cont}"
    );
    let resumed = json_of(&http_get(port, "/api/debug/state"));
    assert_eq!(
        resumed["state"], "running",
        "after Continue the session reads as running again: {resumed}"
    );

    // -- Link 4: the debugger belongs to the session ---------------------------
    let stopped = json_of(&http_post(port, "/api/runtime/stop", ""));
    assert_eq!(stopped["ok"], true, "stop failed: {stopped}");
    let after_stop = json_of(&http_get(port, "/api/debug/state"));
    assert_eq!(
        after_stop["attached"], false,
        "is_active() must report false once the game is not running: {after_stop}"
    );
    assert_eq!(
        after_stop["state"], "detached",
        "the panel must return to detached after Stop: {after_stop}"
    );

    drop(handle);
}
