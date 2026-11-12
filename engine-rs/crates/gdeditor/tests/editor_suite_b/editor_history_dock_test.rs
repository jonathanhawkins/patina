//! App-level tests for the editor History dock (Godot parity).
//!
//! Patina was missing the History window that Godot docks beside FileSystem in
//! the bottom-left: a newest-first list of undo/redo actions with "The
//! Beginning" anchored at the bottom, the current version highlighted, Scene/
//! Global filter checkboxes, and click-to-jump navigation. These tests boot the
//! real `EditorServerHandle` over HTTP and assert:
//!   1. the served `/editor` UI actually ships the History dock + its wiring,
//!   2. `GET /api/history` mirrors the live undo/redo timeline (current marker,
//!      newest-first order, the Beginning anchor), and
//!   3. `POST /api/history/goto` jumps the *edited scene* to any version.
//! The harness mirrors editor_capabilities_routes_test.rs so this file is
//! self-contained.

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

fn extract_body(resp: &str) -> &str {
    resp.split("\r\n\r\n").nth(1).unwrap_or("")
}

fn get_main_node_id(port: u16) -> u64 {
    let resp = http_get(port, "/api/scene");
    let v: serde_json::Value = serde_json::from_str(extract_body(&resp)).unwrap();
    v["nodes"]["children"][0]["id"].as_u64().unwrap()
}

/// Number of children under the "Main" node (root's first child). Adding a node
/// grows this; undoing back to the Beginning shrinks it to 0.
fn main_child_count(port: u16) -> usize {
    let resp = http_get(port, "/api/scene");
    let v: serde_json::Value = serde_json::from_str(extract_body(&resp)).unwrap();
    v["nodes"]["children"][0]["children"]
        .as_array()
        .map(|a| a.len())
        .unwrap_or(0)
}

fn get_history(port: u16) -> serde_json::Value {
    let resp = http_get(port, "/api/history");
    assert!(
        resp.contains("200 OK"),
        "GET /api/history should return 200, got:\n{resp}"
    );
    serde_json::from_str(extract_body(&resp)).expect("history body should be valid JSON")
}

/// Adds a Node2D child named `name` under `parent`, returning the new node id.
fn add_child(port: u16, parent: u64, name: &str) -> u64 {
    let body = format!(r#"{{"parent_id":{parent},"name":"{name}","class_name":"Node2D"}}"#);
    let resp = http_post(port, "/api/node/add", &body);
    assert!(resp.contains("200 OK"), "node/add should 200, got:\n{resp}");
    let v: serde_json::Value = serde_json::from_str(extract_body(&resp)).unwrap();
    v["id"].as_u64().expect("node/add returns new id")
}

/// (1) The served editor must actually ship the History dock and its wiring —
/// the tab, the dock container, the Scene/Global filters, and the JS that talks
/// to /api/history. This is what was missing.
#[test]
fn served_editor_ships_history_dock() {
    let (handle, port) = make_server();
    let html = http_get(port, "/editor");

    for needle in [
        "id=\"history-dock-tab\"", // the FileSystem-tabbed History tab
        "id=\"history-dock\"",     // dock content container
        "id=\"history-scene\"",    // Scene filter checkbox
        "id=\"history-global\"",   // Global filter checkbox
        "id=\"history-list\"",     // the entry list
        "refreshHistoryDock",      // fetches /api/history
        "function gotoHistory",    // click-to-jump handler
        "/api/history",            // talks to the backend route
    ] {
        assert!(
            html.contains(needle),
            "served /editor is missing History dock marker `{needle}`"
        );
    }
    handle.stop();
}

/// (2) GET /api/history mirrors the live undo/redo timeline: it always anchors
/// "The Beginning" (version 0), advances `current` as actions are applied, and
/// orders entries newest-first with exactly one `current` entry.
#[test]
fn history_reflects_undo_redo_timeline() {
    let (handle, port) = make_server();
    let main = get_main_node_id(port);

    // Fresh document: only "The Beginning", and it is the current version.
    let h0 = get_history(port);
    assert_eq!(h0["current"].as_u64(), Some(0), "fresh doc is at version 0");
    let e0 = h0["entries"].as_array().unwrap();
    assert_eq!(e0.len(), 1, "fresh history has only the Beginning anchor");
    assert_eq!(e0[0]["label"].as_str(), Some("The Beginning"));
    assert_eq!(e0[0]["version"].as_u64(), Some(0));
    assert_eq!(e0[0]["current"].as_bool(), Some(true));

    // Apply two actions.
    add_child(port, main, "Alpha");
    add_child(port, main, "Beta");

    let h2 = get_history(port);
    assert_eq!(h2["current"].as_u64(), Some(2), "two actions => version 2");
    let entries = h2["entries"].as_array().unwrap();
    // Newest-first: [Beta action, Alpha action, The Beginning].
    assert_eq!(entries.len(), 3, "two actions + Beginning");
    assert!(
        entries[0]["label"].as_str().unwrap().contains("Beta"),
        "newest action listed first, got: {}",
        entries[0]["label"]
    );
    assert_eq!(
        entries[0]["current"].as_bool(),
        Some(true),
        "latest action is the current version"
    );
    assert!(
        entries[1]["label"].as_str().unwrap().contains("Alpha"),
        "older action listed below the newest"
    );
    assert_eq!(
        entries.last().unwrap()["label"].as_str(),
        Some("The Beginning"),
        "Beginning anchored at the bottom"
    );
    // Exactly one entry is the current version; none of the applied actions are
    // "future" (nothing has been undone yet).
    let current_count = entries.iter().filter(|e| e["current"] == true).count();
    assert_eq!(current_count, 1, "exactly one current version");
    assert!(
        entries.iter().all(|e| e["future"] == false),
        "nothing is in the future before any undo"
    );
    handle.stop();
}

/// After undoing, the undone action becomes a "future" (redoable) entry and the
/// current marker moves down the timeline.
#[test]
fn history_marks_future_after_undo() {
    let (handle, port) = make_server();
    let main = get_main_node_id(port);
    add_child(port, main, "Alpha");
    add_child(port, main, "Beta");

    // Undo once: Beta is now a future action, current is version 1 (Alpha).
    assert!(http_post(port, "/api/undo", "").contains("200 OK"));

    let h = get_history(port);
    assert_eq!(h["current"].as_u64(), Some(1), "undo moves current to v1");
    let entries = h["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 3, "undo keeps the redoable action listed");
    // Newest-first still: Beta (future) on top, Alpha (current), Beginning.
    assert!(entries[0]["label"].as_str().unwrap().contains("Beta"));
    assert_eq!(
        entries[0]["future"].as_bool(),
        Some(true),
        "the undone action is shown as a dimmed future entry"
    );
    assert_eq!(entries[0]["current"].as_bool(), Some(false));
    assert!(entries[1]["label"].as_str().unwrap().contains("Alpha"));
    assert_eq!(entries[1]["current"].as_bool(), Some(true));
    handle.stop();
}

/// (3) POST /api/history/goto jumps the *edited scene* to any version by
/// undoing/redoing the right number of steps — the dock's click-to-jump.
#[test]
fn history_goto_jumps_edit_state() {
    let (handle, port) = make_server();
    let main = get_main_node_id(port);
    assert_eq!(main_child_count(port), 0, "Main starts empty");

    add_child(port, main, "Alpha");
    add_child(port, main, "Beta");
    assert_eq!(main_child_count(port), 2, "two children added");

    // Jump all the way back to the Beginning: both adds are undone.
    let r0 = http_post(port, "/api/history/goto", r#"{"version":0}"#);
    assert!(r0.contains("200 OK"), "goto v0 should 200, got:\n{r0}");
    assert_eq!(
        main_child_count(port),
        0,
        "goto Beginning removes both nodes"
    );
    assert_eq!(get_history(port)["current"].as_u64(), Some(0));

    // Jump forward to the latest version: both adds are redone.
    let r2 = http_post(port, "/api/history/goto", r#"{"version":2}"#);
    assert!(r2.contains("200 OK"), "goto v2 should 200, got:\n{r2}");
    assert_eq!(main_child_count(port), 2, "goto latest redoes both nodes");
    assert_eq!(get_history(port)["current"].as_u64(), Some(2));

    // Jump to the midpoint: exactly one node present.
    let r1 = http_post(port, "/api/history/goto", r#"{"version":1}"#);
    assert!(r1.contains("200 OK"), "goto v1 should 200, got:\n{r1}");
    assert_eq!(main_child_count(port), 1, "goto v1 leaves one node");
    assert_eq!(get_history(port)["current"].as_u64(), Some(1));
    handle.stop();
}

/// goto is robust against bad input: an out-of-range version clamps to the
/// latest valid version rather than erroring or corrupting state, and a missing
/// `version` field is a clean 400.
#[test]
fn history_goto_clamps_and_validates() {
    let (handle, port) = make_server();
    let main = get_main_node_id(port);
    add_child(port, main, "Alpha");

    // Way past the end clamps to the latest version (1) — no panic, scene intact.
    let big = http_post(port, "/api/history/goto", r#"{"version":999}"#);
    assert!(
        big.contains("200 OK"),
        "out-of-range goto should 200, got:\n{big}"
    );
    assert_eq!(main_child_count(port), 1, "clamped goto keeps the node");
    assert_eq!(get_history(port)["current"].as_u64(), Some(1));

    // Missing the version field is a clean client error, not a 500.
    let bad = http_post(port, "/api/history/goto", r#"{}"#);
    assert!(
        bad.contains("400"),
        "goto without a version should be a 400, got:\n{bad}"
    );
    handle.stop();
}
