//! App-level e2e test for Stop tearing down the instance and restoring the
//! editable editor state (pat-niu1j.3).
//!
//! Methodology: the editor-parity gate requires an app-level test, not a unit
//! test on an isolated model. This boots the real server
//! (`EditorServerHandle::start`), drives it over HTTP the way a user does —
//! pick an editing mode, Play (`POST /api/runtime/play`), then Stop
//! (`POST /api/runtime/stop`) — and asserts that Stop both tears down the run
//! (`GET /api/runtime/status` reports not-running) and restores the editor to
//! the SAME editing view it was on before Play (`GET /api/editor/main_view`),
//! rather than leaving the editor stranded on the game view. Because the
//! tree/inspector/viewport mutation routes are gated on `is_running`, dropping
//! it on Stop is what re-enables editing; the test asserts that observable
//! `running=false` plus the restored non-game view.
//!
//! Deliberately a SINGLE `#[test]`: nextest launches one process per test, and
//! booting servers from several concurrently-launched processes thrashes under
//! the host's code-sign/AMFI launch path. Harness mirrors
//! `editor_play_e2e_test.rs` (raw-TCP HTTP); integration-test crates can't share
//! helpers, so they're duplicated here.

use gdeditor::editor_server::{EditorServerHandle, EditorState};
use gdscene::node::Node;
use gdscene::SceneTree;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;
use std::time::Duration;

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn make_server() -> (EditorServerHandle, u16) {
    let port = free_port();
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    tree.add_child(root, Node::new("Main", "Node2D")).unwrap();
    let state = EditorState::new(tree);
    let handle = EditorServerHandle::start(port, state);
    thread::sleep(Duration::from_millis(100));
    (handle, port)
}

fn connect_with_retry(port: u16) -> TcpStream {
    for attempt in 0..50 {
        match TcpStream::connect(format!("127.0.0.1:{port}")) {
            Ok(s) => return s,
            Err(_) if attempt < 49 => thread::sleep(Duration::from_millis(100)),
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
        &format!("GET {path} HTTP/1.1\r\nHost: localhost\r\n\r\n"),
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

fn extract_body(resp: &str) -> &str {
    resp.split("\r\n\r\n").nth(1).unwrap_or("")
}

fn main_view(port: u16) -> serde_json::Value {
    let resp = http_get(port, "/api/editor/main_view");
    assert!(resp.contains("200 OK"), "main_view ok: {resp}");
    serde_json::from_str(extract_body(&resp)).unwrap_or_else(|e| panic!("main_view json: {e}"))
}

fn status(port: u16) -> serde_json::Value {
    let resp = http_get(port, "/api/runtime/status");
    assert!(resp.contains("200 OK"), "status ok: {resp}");
    serde_json::from_str(extract_body(&resp)).unwrap_or_else(|e| panic!("status json: {e}"))
}

/// Boots the real editor server and asserts that Stop tears down the running
/// instance and restores the editable editor state: after editing in 3D mode,
/// Play switches to the game view and runs, and Stop returns the editor to the
/// 3D editing view (not the game view) with the runtime no longer running
/// (so editing is unlocked again) — pat-niu1j.3.
#[test]
fn editor_stop_restores_editable() {
    let (_handle, port) = make_server();

    // Pick a non-default editing view so we can prove Stop restores THIS mode,
    // not merely the 2D default.
    let set = http_post(port, "/api/editor/mode", r#"{"mode":"3d"}"#);
    assert!(set.contains("200 OK"), "set 3d ok: {set}");
    let editing = main_view(port);
    assert_eq!(editing["mode"], "3d", "editing in 3d mode: {editing}");
    assert_eq!(
        editing["component_id"], "spatial_editor",
        "3d mode shows the spatial editor: {editing}"
    );
    assert_eq!(status(port)["running"], false, "not running while editing");

    // Play switches to the game view and runs (edit-lock engaged).
    let play = http_post(port, "/api/runtime/play", "{}");
    assert!(play.contains("200 OK"), "play ok: {play}");
    let running = main_view(port);
    assert_eq!(
        running["component_id"], "game_view",
        "Play shows the game view: {running}"
    );
    assert_eq!(status(port)["running"], true, "running after play");

    // Stop tears down the run and restores the prior (3D) editing view.
    let stop = http_post(port, "/api/runtime/stop", "{}");
    assert!(stop.contains("200 OK"), "stop ok: {stop}");
    let stop_body: serde_json::Value =
        serde_json::from_str(extract_body(&stop)).unwrap_or_else(|e| panic!("stop json: {e}"));
    assert_eq!(
        stop_body["running"], false,
        "stop reports not running: {stop_body}"
    );
    assert_eq!(
        stop_body["editor_mode"], "3d",
        "stop restores the prior editing mode: {stop_body}"
    );

    // The runtime is fully torn down and editing is unlocked again.
    let after = status(port);
    assert_eq!(
        after["running"], false,
        "runtime not running after stop: {after}"
    );
    assert_eq!(
        after["paused"], false,
        "runtime not paused after stop: {after}"
    );

    // The served central view is back on the 3D editor — not stranded on the
    // game view.
    let restored = main_view(port);
    assert_eq!(
        restored["mode"], "3d",
        "editor restored to 3d mode: {restored}"
    );
    assert_eq!(
        restored["component_id"], "spatial_editor",
        "central view restored to the spatial editor: {restored}"
    );
    assert_ne!(
        restored["component_id"], "game_view",
        "editor is not stranded on the game view after stop: {restored}"
    );
}
