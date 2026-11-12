//! pat-nzgdu.170: served inspector Back/Forward + object-history navigation.
//!
//! The server handlers (/api/inspector/back|forward|history|history/select) and
//! the EditorSelectionHistory state already existed and passed tests, but the
//! served inspector never called them. This guards:
//!  1. Static — EDITOR_HTML now references the back + history endpoints and
//!     defines the navigation wiring.
//!  2. Functional — selecting node A then node B and posting /api/inspector/back
//!     returns focus to A (the handler's reported `selected` object), driven
//!     through the real select + back routes.

use gdeditor::editor_server::{EditorServerHandle, EditorState};
use gdeditor::editor_ui::EDITOR_HTML;
use gdscene::node::Node;
use gdscene::SceneTree;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;
use std::time::Duration;

// ---------------------------------------------------------------------------
// (1) Static wiring guard.
// ---------------------------------------------------------------------------

#[test]
fn editor_html_wires_inspector_history_nav() {
    assert!(
        EDITOR_HTML.contains("/api/inspector/back"),
        "EDITOR_HTML must POST /api/inspector/back"
    );
    assert!(
        EDITOR_HTML.contains("/api/inspector/history"),
        "EDITOR_HTML must GET /api/inspector/history for the object-history dropdown"
    );
    assert!(
        EDITOR_HTML.contains("inspectorNav"),
        "EDITOR_HTML must wire the Back/Forward navigation"
    );
}

// ---------------------------------------------------------------------------
// (2) Functional select -> select -> back round-trip.
// ---------------------------------------------------------------------------

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

fn http_post(port: u16, path: &str, body: &str) -> String {
    let mut stream = connect_with_retry(port);
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
    let req = format!(
        "POST {path} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(req.as_bytes()).unwrap();
    let mut resp = Vec::new();
    let _ = stream.read_to_end(&mut resp);
    String::from_utf8_lossy(&resp).to_string()
}

fn extract_body(resp: &str) -> &str {
    resp.split("\r\n\r\n").nth(1).unwrap_or("")
}

#[test]
fn inspector_back_returns_to_previous_object() {
    let port = free_port();

    // Scene: root -> A, B.
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let a = tree.add_child(root, Node::new("A", "Node2D")).unwrap();
    let b = tree.add_child(root, Node::new("B", "Node2D")).unwrap();

    let handle = EditorServerHandle::start(port, EditorState::new(tree));
    thread::sleep(Duration::from_millis(100));

    // Inspect A, then B — each POST /api/node/select pushes to the history.
    let sa = http_post(
        port,
        "/api/node/select",
        &format!(r#"{{"node_id":{}}}"#, a.raw()),
    );
    assert!(sa.contains("200 OK"), "select A should 200, got:\n{sa}");
    let sb = http_post(
        port,
        "/api/node/select",
        &format!(r#"{{"node_id":{}}}"#, b.raw()),
    );
    assert!(sb.contains("200 OK"), "select B should 200, got:\n{sb}");

    // Back -> the current inspected object returns to A.
    let back = http_post(port, "/api/inspector/back", "");
    assert!(back.contains("200 OK"), "back should 200, got:\n{back}");
    let v: serde_json::Value =
        serde_json::from_str(extract_body(&back)).expect("back response should be JSON");
    assert_eq!(
        v["selected"].as_u64(),
        Some(a.raw()),
        "Back after inspecting A then B must return to A; response: {v}"
    );
    assert_eq!(
        v["can_forward"].as_bool(),
        Some(true),
        "Forward should be available after going Back"
    );

    handle.stop();
}
