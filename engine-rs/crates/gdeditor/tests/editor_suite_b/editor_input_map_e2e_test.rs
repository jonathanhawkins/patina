//! App-level e2e test for the Project Settings > Input Map tab (pat-db37w).
//!
//! `crates/gdeditor/src/input_map.rs` defines a complete `InputMap` model with
//! its own lib unit tests — but those pass even if the model is never wired
//! into the running editor. This test exercises the input map the way the
//! Input Map dialog does: by booting the *real* editor server and editing the
//! map over HTTP (`GET`/`POST /api/input_map`), then proving the edit is
//! honored by the live runtime (`is_action_pressed` via
//! `/api/runtime/input/state`) rather than living only in an isolated struct.

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

fn boot_editor() -> (EditorServerHandle, u16) {
    let port = free_port();
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let main = Node::new("Main", "Node2D");
    tree.add_child(root, main).unwrap();
    let state = EditorState::new(tree);
    let handle = EditorServerHandle::start(port, state);
    thread::sleep(Duration::from_millis(100));
    (handle, port)
}

fn connect_with_retry(port: u16) -> TcpStream {
    for attempt in 0..40 {
        match TcpStream::connect(format!("127.0.0.1:{port}")) {
            Ok(s) => return s,
            Err(_) if attempt < 39 => thread::sleep(Duration::from_millis(50)),
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

fn json_body(resp: &str) -> serde_json::Value {
    serde_json::from_str(extract_body(resp))
        .unwrap_or_else(|e| panic!("response body is not JSON ({e}): {resp}"))
}

/// Returns the list of action names present in an `/api/input_map` payload.
fn action_names(map: &serde_json::Value) -> Vec<String> {
    map["actions"]
        .as_object()
        .map(|o| o.keys().cloned().collect())
        .unwrap_or_default()
}

/// Adding an action over HTTP edits the *live* editor's input map: it reads
/// back via `GET /api/input_map`, and the bound key resolves the action at
/// runtime through the editor's own `is_action_pressed`.
#[test]
fn input_map_edit_is_live_in_running_editor() {
    let (handle, port) = boot_editor();

    // The Input Map tab loads the current map. The default map is non-empty,
    // and "jump" is not one of the defaults.
    let initial = http_get(port, "/api/input_map");
    assert!(initial.contains("200 OK"), "GET /api/input_map ok: {initial}");
    let initial = json_body(&initial);
    assert!(
        !action_names(&initial).iter().any(|a| a == "jump"),
        "jump is not a default action: {initial}"
    );

    // Add a "jump" action bound to Space, the way the dialog does.
    let added = http_post(port, "/api/input_map", r#"{"action":"jump","keys":["Space"]}"#);
    assert!(added.contains("200 OK"), "POST add action ok: {added}");
    let added = json_body(&added);
    assert!(
        added["actions"]["jump"]["events"]
            .as_array()
            .map(|evs| !evs.is_empty())
            .unwrap_or(false),
        "added response shows jump with a bound event: {added}"
    );

    // The edit persists in the running server (not just the POST response).
    let reread = json_body(&http_get(port, "/api/input_map"));
    assert!(
        action_names(&reread).iter().any(|a| a == "jump"),
        "jump persists after re-reading the live map: {reread}"
    );

    // Prove the runtime honors the new binding: start the game runtime, press
    // Space, and the "jump" action reports pressed via is_action_pressed.
    assert!(
        http_post(port, "/api/runtime/play", "{}").contains("200 OK"),
        "runtime starts"
    );
    assert!(
        http_post(port, "/api/runtime/input/key_down", r#"{"key":"Space"}"#).contains("200 OK"),
        "key_down accepted while running"
    );
    let state = json_body(&http_get(port, "/api/runtime/input/state"));
    assert_eq!(
        state["actions"]["jump"], true,
        "the live runtime resolves the edited action: {state}"
    );

    handle.stop();
}

/// Removing an action over HTTP also takes effect in the live editor.
#[test]
fn input_map_remove_action_is_live() {
    let (handle, port) = boot_editor();

    http_post(port, "/api/input_map", r#"{"action":"sprint","keys":["Shift"]}"#);
    let with = json_body(&http_get(port, "/api/input_map"));
    assert!(
        action_names(&with).iter().any(|a| a == "sprint"),
        "sprint was added: {with}"
    );

    let removed = http_post(port, "/api/input_map", r#"{"action":"sprint","remove":true}"#);
    assert!(removed.contains("200 OK"), "POST remove ok: {removed}");

    let without = json_body(&http_get(port, "/api/input_map"));
    assert!(
        !action_names(&without).iter().any(|a| a == "sprint"),
        "sprint is gone from the live map: {without}"
    );

    handle.stop();
}

/// A malformed edit (no action name) is rejected without corrupting the map.
#[test]
fn input_map_rejects_missing_action() {
    let (handle, port) = boot_editor();

    let before = json_body(&http_get(port, "/api/input_map"));
    let resp = http_post(port, "/api/input_map", r#"{"keys":["Space"]}"#);
    assert!(resp.contains("400"), "missing action rejected: {resp}");

    let after = json_body(&http_get(port, "/api/input_map"));
    assert_eq!(
        action_names(&before).len(),
        action_names(&after).len(),
        "rejected edit left the map unchanged"
    );

    handle.stop();
}
