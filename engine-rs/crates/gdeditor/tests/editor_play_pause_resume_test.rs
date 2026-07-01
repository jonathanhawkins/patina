//! App-level e2e test for pausing and resuming the running scene (pat-niu1j.4).
//!
//! Methodology: the editor-parity gate requires an app-level test, not a unit
//! test on an isolated model. This boots the real server
//! (`EditorServerHandle::start`), drives it over HTTP the way a user does —
//! Play (`POST /api/runtime/play`), then Pause/Resume (`POST /api/runtime/pause`)
//! — and asserts the SERVED runtime status (`GET /api/runtime/status`) reflects
//! the paused state: a freshly-launched scene runs unpaused, the first pause
//! freezes it (`paused=true`) while it stays running, and a second pause resumes
//! it (`paused=false`). Pausing a scene that is not running is rejected.
//!
//! Deliberately a SINGLE `#[test]`: nextest launches one process per test, and
//! booting servers from several concurrently-launched processes thrashes under
//! the host's code-sign/AMFI launch path. Harness mirrors
//! `editor_play_e2e_test.rs` / `editor_mode_e2e_test.rs` (raw-TCP HTTP);
//! integration-test crates can't share helpers, so they're duplicated here.

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

fn status(port: u16) -> serde_json::Value {
    let resp = http_get(port, "/api/runtime/status");
    assert!(resp.contains("200 OK"), "status ok: {resp}");
    serde_json::from_str(extract_body(&resp)).unwrap_or_else(|e| panic!("status json: {e}"))
}

/// Boots the real editor server and exercises the pause/resume loop over HTTP:
/// Play launches the scene unpaused, the first Pause freezes it while it stays
/// running, and a second Pause resumes it. Pausing a not-running scene is
/// rejected (pat-niu1j.4).
#[test]
fn editor_play_pause_resume() {
    let (_handle, port) = make_server();

    // Pausing before Play is rejected — there's no running scene to pause.
    let early = http_post(port, "/api/runtime/pause", "{}");
    assert!(
        early.contains("400"),
        "pause before play is rejected: {early}"
    );

    // Play launches the scene; it runs and is not paused.
    let play = http_post(port, "/api/runtime/play", "{}");
    assert!(play.contains("200 OK"), "play ok: {play}");
    let after_play = status(port);
    assert_eq!(after_play["running"], true, "scene runs after play: {after_play}");
    assert_eq!(
        after_play["paused"], false,
        "a freshly-launched scene is not paused: {after_play}"
    );

    // First pause freezes the running scene.
    let pause = http_post(port, "/api/runtime/pause", "{}");
    assert!(pause.contains("200 OK"), "pause ok: {pause}");
    let pause_body: serde_json::Value =
        serde_json::from_str(extract_body(&pause)).unwrap_or_else(|e| panic!("pause json: {e}"));
    assert_eq!(pause_body["paused"], true, "pause reports paused: {pause_body}");
    let paused = status(port);
    assert_eq!(paused["paused"], true, "status reports paused: {paused}");
    assert_eq!(
        paused["running"], true,
        "a paused scene is still running (frozen, not stopped): {paused}"
    );

    // Second pause resumes the scene.
    let resume = http_post(port, "/api/runtime/pause", "{}");
    assert!(resume.contains("200 OK"), "resume ok: {resume}");
    let resume_body: serde_json::Value =
        serde_json::from_str(extract_body(&resume)).unwrap_or_else(|e| panic!("resume json: {e}"));
    assert_eq!(
        resume_body["paused"], false,
        "second pause resumes (paused=false): {resume_body}"
    );
    let resumed = status(port);
    assert_eq!(resumed["paused"], false, "status reports resumed: {resumed}");
    assert_eq!(resumed["running"], true, "scene still running after resume: {resumed}");
}
