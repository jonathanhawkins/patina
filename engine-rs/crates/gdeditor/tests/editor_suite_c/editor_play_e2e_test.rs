//! App-level e2e test for Play launching the current scene into a game view
//! (pat-niu1j.1).
//!
//! Methodology: the editor-parity gate requires an app-level test, not a unit
//! test on an isolated model struct. A `PlayCustomScene`-style model can pass
//! in isolation while Play never actually swaps the central view in the running
//! editor. This boots the real server (`EditorServerHandle::start`), drives it
//! over HTTP the way a user does — `POST /api/runtime/play` — and asserts that
//! the SERVED central view (`GET /api/editor/main_view`) becomes the game view,
//! and that the runtime reports the scene is running. Before Play it asserts the
//! editor is on the 2D viewport, so the switch is observable, not assumed.
//!
//! Deliberately a SINGLE `#[test]`: nextest launches one process per test, and
//! booting servers from several concurrently-launched processes thrashes under
//! the host's code-sign/AMFI launch path. Harness mirrors
//! `editor_mode_e2e_test.rs` (raw-TCP HTTP); integration-test crates can't share
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

/// Boots the real editor server and asserts that pressing Play
/// (`POST /api/runtime/play`) launches the current scene into the game view:
/// the runtime reports running, and the served central view swaps from the 2D
/// viewport to the `game_view` component (pat-niu1j.1).
#[test]
fn editor_play_launches_scene_game_view() {
    let (_handle, port) = make_server();

    // Before Play: the editor shows the 2D canvas viewport. This makes the
    // subsequent switch observable rather than assumed.
    let before = main_view(port);
    assert_eq!(
        before["is_2d_viewport"], true,
        "editor starts on the 2D viewport: {before}"
    );
    assert_eq!(before["mode"], "2d", "editor starts in 2d mode: {before}");

    // Press Play.
    let play = http_post(port, "/api/runtime/play", "{}");
    assert!(play.contains("200 OK"), "play ok: {play}");
    let play_body: serde_json::Value =
        serde_json::from_str(extract_body(&play)).unwrap_or_else(|e| panic!("play json: {e}"));
    assert_eq!(
        play_body["running"], true,
        "play reports running: {play_body}"
    );

    // The runtime is actually running the scene.
    let status = http_get(port, "/api/runtime/status");
    assert!(status.contains("200 OK"), "status ok: {status}");
    let status_body: serde_json::Value =
        serde_json::from_str(extract_body(&status)).unwrap_or_else(|e| panic!("status json: {e}"));
    assert_eq!(
        status_body["running"], true,
        "runtime reports the scene running: {status_body}"
    );
    assert_eq!(
        status_body["paused"], false,
        "a freshly-launched scene is not paused: {status_body}"
    );

    // After Play: the served central view is the game view, not the 2D viewport.
    let after = main_view(port);
    assert_eq!(
        after["mode"], "game",
        "Play switches the editor to game mode: {after}"
    );
    assert_eq!(
        after["component_id"], "game_view",
        "Play launches the scene into the game view: {after}"
    );
    assert_eq!(
        after["is_2d_viewport"], false,
        "the game view is not the 2D viewport: {after}"
    );
}
