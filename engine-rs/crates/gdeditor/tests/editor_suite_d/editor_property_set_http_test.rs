//! Regression test for pat-d55cu.
//!
//! The bead reported that `POST /api/property/set` returned `{"ok":true}` but
//! silently applied nothing: the value never changed, the scene dirty flag
//! never toggled, and `POST /api/undo` reported "nothing to undo" (no undo
//! entry was pushed). This is a full app-level HTTP roundtrip that boots the
//! real editor server and pins the correct behavior so the silent-drop class
//! of regression can never return unnoticed.
//!
//! Harness (free_port / make_server / http_get / http_post / extract_body) is
//! copied verbatim from crates/gdeditor/tests/editor_parity_test.rs so this
//! file is self-contained.

use gdeditor::editor_server::{EditorServerHandle, EditorState};
use gdscene::node::Node;
use gdscene::SceneTree;
use gdvariant::Variant;
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
    let mut main = Node::new("Main", "Node2D");
    main.set_property(
        "position",
        Variant::Vector2(gdcore::math::Vector2::new(10.0, 20.0)),
    );
    tree.add_child(root, main).unwrap();
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

fn get_main_node_id(port: u16) -> u64 {
    let resp = http_get(port, "/api/scene");
    let v: serde_json::Value = serde_json::from_str(extract_body(&resp)).unwrap();
    v["nodes"]["children"][0]["id"].as_u64().unwrap()
}

/// Reads the `position` Vector2 of a node via `GET /api/node/<id>` and returns
/// it as `(x, y)`. Panics if the property is missing or not a Vector2 — those
/// are exactly the silent-drop symptoms this test exists to catch.
fn read_position(port: u16, node_id: u64) -> (f64, f64) {
    let resp = http_get(port, &format!("/api/node/{node_id}"));
    let v: serde_json::Value = serde_json::from_str(extract_body(&resp)).unwrap();
    let pos = v["properties"]
        .as_array()
        .expect("node has a properties array")
        .iter()
        .find(|p| p["name"] == "position")
        .expect("position property must exist on the node");
    assert_eq!(pos["type"], "Vector2", "position must be a Vector2");
    let arr = pos["value"]["value"]
        .as_array()
        .expect("Vector2 value is a 2-element array");
    (
        arr[0].as_f64().expect("x component is a number"),
        arr[1].as_f64().expect("y component is a number"),
    )
}

/// Reads the `modified` (scene-dirty) flag from `GET /api/scene/info`.
fn read_modified(port: u16) -> bool {
    let resp = http_get(port, "/api/scene/info");
    let v: serde_json::Value = serde_json::from_str(extract_body(&resp)).unwrap();
    v["modified"]
        .as_bool()
        .expect("scene/info exposes a boolean `modified` flag")
}

/// pat-d55cu: setting a property over HTTP with the documented typed-object
/// body must (a) actually mutate the live scene, (b) flip the scene-dirty
/// flag, and (c) push an undo entry that restores the prior value.
#[test]
fn editor_property_set_http_applies_and_undoes() {
    let (handle, port) = make_server();
    let node_id = get_main_node_id(port);

    // Precondition: the demo node starts at (10, 20) and the scene is clean.
    assert_eq!(
        read_position(port, node_id),
        (10.0, 20.0),
        "fixture precondition: position starts at (10, 20)"
    );
    assert!(
        !read_modified(port),
        "fixture precondition: a freshly loaded scene is not modified"
    );

    // Apply: set position via the documented `{type, value}` variant body
    // (the exact shape from the bead repro).
    let body = format!(
        r#"{{"node_id":{node_id},"property":"position","value":{{"type":"Vector2","value":[123,456]}}}}"#
    );
    let set_resp = http_post(port, "/api/property/set", &body);
    assert!(
        set_resp.contains("200 OK"),
        "property/set should succeed; got: {set_resp}"
    );

    // (a) The mutation must be visible in the live scene — not a silent no-op.
    assert_eq!(
        read_position(port, node_id),
        (123.0, 456.0),
        "property/set returned ok but the value did not change in the scene (silent drop)"
    );

    // (b) The scene-dirty flag must toggle on after a real mutation.
    assert!(
        read_modified(port),
        "property/set returned ok but the scene was never marked modified"
    );

    // (c) An undo entry must have been pushed: undo succeeds (not "nothing to
    // undo") and restores the original value.
    let undo_resp = http_post(port, "/api/undo", "");
    assert!(
        undo_resp.contains("200 OK"),
        "undo after property/set should succeed, meaning an undo entry was pushed; got: {undo_resp}"
    );
    assert!(
        !undo_resp.contains("nothing to undo"),
        "property/set pushed no undo entry; got: {undo_resp}"
    );
    assert_eq!(
        read_position(port, node_id),
        (10.0, 20.0),
        "undo did not restore the original position"
    );

    handle.stop();
}