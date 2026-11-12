//! End-to-end guard for `Scene > Close Scene` against the real editor server
//! (Godot `EditorNode::_scene_tab_closed` / `_remove_edited_scene`).
//!
//! Covers the multi-tab focus + scene-swap behavior that the parity test's
//! last-tab case doesn't: closing the active tab moves focus to the tab on its
//! left and displays that scene (loaded from disk); closing a background tab
//! keeps the active scene focused, its index shifting left.

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

fn json(resp: &str) -> serde_json::Value {
    serde_json::from_str(extract_body(resp)).unwrap_or_else(|e| panic!("json: {e}: {resp}"))
}

fn tabs(port: u16) -> serde_json::Value {
    json(&http_get(port, "/api/scene/tabs"))
}

/// Absolute path to a workspace scene fixture. Integration tests run with CWD =
/// the crate dir, so a workspace-relative path wouldn't resolve; the server
/// loads tabs by the exact stored path, so an absolute one is CWD-independent.
fn fixture(name: &str) -> String {
    format!(
        "{}/../../fixtures/scenes/{name}",
        env!("CARGO_MANIFEST_DIR")
    )
}

/// Opens `path` in a new tab and returns its tab id (also becomes active).
fn open_tab(port: u16, path: &str) -> u64 {
    let resp = http_post(
        port,
        "/api/scene/tabs/open",
        &format!(r#"{{"path":"{path}"}}"#),
    );
    assert!(resp.contains("200 OK"), "open {path} ok: {resp}");
    json(&resp)["tab_id"].as_u64().unwrap()
}

fn close_tab(port: u16, tab_id: u64) -> serde_json::Value {
    let resp = http_post(
        port,
        "/api/scene/tabs/close",
        &format!(r#"{{"tab_id":{tab_id}}}"#),
    );
    assert!(resp.contains("200 OK"), "close {tab_id} ok: {resp}");
    json(&resp)
}

/// Closing the ACTIVE tab moves focus to the tab on its left and shows that
/// tab's scene, loaded from disk.
#[test]
fn closing_active_tab_focuses_left_neighbor_and_loads_its_scene() {
    let (handle, port) = make_server();
    let shooter = fixture("space_shooter.tscn");
    open_tab(port, &shooter); // idx 1
    let demo = open_tab(port, &fixture("demo_2d.tscn")); // idx 2, active

    let before = tabs(port);
    assert_eq!(before["tabs"].as_array().unwrap().len(), 3);
    assert_eq!(before["active_tab_index"].as_u64(), Some(2));

    // Close the active tab (demo_2d).
    let closed = close_tab(port, demo);
    assert_eq!(
        closed["active_tab_index"].as_u64(),
        Some(1),
        "focus moves to the left neighbor (space_shooter): {closed}"
    );

    let after = tabs(port);
    let arr = after["tabs"].as_array().unwrap();
    assert_eq!(arr.len(), 2, "one tab removed: {after}");
    assert_eq!(
        arr[1]["path"].as_str(),
        Some(shooter.as_str()),
        "the now-active tab is space_shooter: {after}"
    );

    // The displayed scene actually swapped to space_shooter, loaded from disk.
    let scene = json(&http_get(port, "/api/scene"));
    let kids = scene["nodes"]["children"].as_array().unwrap();
    assert_eq!(
        kids.first().and_then(|n| n["name"].as_str()),
        Some("SpaceShooter"),
        "the space_shooter root is now shown: {scene}"
    );
    let info = json(&http_get(port, "/api/scene/info"));
    assert_eq!(
        info["scene_file"].as_str(),
        Some(shooter.as_str()),
        "the active scene file tracks the reopened tab: {info}"
    );
    handle.stop();
}

/// Closing a BACKGROUND tab keeps the active scene focused; only its index
/// shifts (Godot leaves the edited scene untouched).
#[test]
fn closing_background_tab_keeps_active_scene() {
    let (handle, port) = make_server();
    let first = tabs(port)["tabs"][0]["id"].as_u64().unwrap(); // idx 0
    open_tab(port, &fixture("space_shooter.tscn")); // idx 1
    let demo = fixture("demo_2d.tscn");
    open_tab(port, &demo); // idx 2, active

    // Close the first (background) tab.
    let closed = close_tab(port, first);
    assert_eq!(
        closed["active_tab_index"].as_u64(),
        Some(1),
        "the active scene stays focused, its index shifts 2 -> 1: {closed}"
    );
    let after = tabs(port);
    let arr = after["tabs"].as_array().unwrap();
    assert_eq!(arr.len(), 2, "one tab removed: {after}");
    assert_eq!(
        arr[1]["path"].as_str(),
        Some(demo.as_str()),
        "the still-active tab is demo_2d: {after}"
    );
    handle.stop();
}
