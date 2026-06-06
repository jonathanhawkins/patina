//! pat-didnj: Optimistic concurrency control on `PATCH /api/node/patch`.
//!
//! Acceptance: a node has a monotonic version number; a PATCH that supplies a
//! stale version returns HTTP 409; a fresh version succeeds.

use gdeditor::editor_server::{EditorServerHandle, EditorState};
use gdscene::node::Node;
use gdscene::SceneTree;

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;
use std::time::Duration;

fn free_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.local_addr().unwrap().port()
}

fn fixture_state() -> EditorState {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    tree.add_child(root, Node::new("Main", "Node2D")).unwrap();
    EditorState::new(tree)
}

fn connect_with_retry(port: u16) -> TcpStream {
    for _ in 0..40 {
        if let Ok(s) = TcpStream::connect(format!("127.0.0.1:{port}")) {
            return s;
        }
        thread::sleep(Duration::from_millis(25));
    }
    panic!("could not connect to editor server on port {port}");
}

fn http_raw(port: u16, request: &str) -> String {
    let mut stream = connect_with_retry(port);
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    stream.write_all(request.as_bytes()).unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).ok();
    response
}

fn status_code(response: &str) -> u16 {
    let first = response.lines().next().unwrap_or("");
    let parts: Vec<&str> = first.split_whitespace().collect();
    parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(0)
}

fn response_body(response: &str) -> &str {
    response.split("\r\n\r\n").nth(1).unwrap_or("")
}

fn http_get(port: u16, path: &str) -> String {
    let req = format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n");
    http_raw(port, &req)
}

fn http_patch(port: u16, path: &str, body: &str) -> String {
    let req = format!(
        "PATCH {path} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {len}\r\nConnection: close\r\n\r\n{body}",
        len = body.len(),
    );
    http_raw(port, &req)
}

fn child_node_id(port: u16) -> u64 {
    let resp = http_get(port, "/api/scene");
    let body = response_body(&resp);
    let v: serde_json::Value = serde_json::from_str(body)
        .unwrap_or_else(|e| panic!("/api/scene must return JSON, got: {body}\n{e}"));
    // /api/scene returns { "nodes": <root-node-object> } where the root has
    // a "children" array; the fixture seeds one child under the root.
    let root = &v["nodes"];
    let children = root["children"]
        .as_array()
        .expect("root must have children array");
    assert!(!children.is_empty(), "fixture must have at least one child");
    children[0]["id"]
        .as_u64()
        .expect("child must expose a u64 id")
}

#[test]
fn editor_concurrent_edit_conflict_test() {
    let port = free_port();
    let handle = EditorServerHandle::start(port, fixture_state());
    thread::sleep(Duration::from_millis(120));

    let node_id = child_node_id(port);

    // 1. First PATCH supplies the initial monotonic version (0) and must
    //    succeed. The response reports the new version 1.
    let body_v0 = format!(
        r#"{{"node_id":{node_id},"version":0,"property":"test_counter","value":{{"type":"Int","value":1}}}}"#
    );
    let first = http_patch(port, "/api/node/patch", &body_v0);
    assert_eq!(
        status_code(&first),
        200,
        "fresh PATCH (v=0) must succeed\n--- response ---\n{first}"
    );
    let first_body = response_body(&first);
    let first_json: serde_json::Value = serde_json::from_str(first_body)
        .unwrap_or_else(|e| panic!("PATCH response must be JSON, got: {first_body}\n{e}"));
    assert_eq!(
        first_json["version"].as_u64(),
        Some(1),
        "version must bump to 1 after first successful PATCH; body={first_body}"
    );

    // 2. Replaying the same PATCH (still version=0) is now stale. The server
    //    must reject it with HTTP 409 Conflict.
    let second = http_patch(port, "/api/node/patch", &body_v0);
    assert_eq!(
        status_code(&second),
        409,
        "stale PATCH (v=0 after v=1 landed) must return 409\n--- response ---\n{second}"
    );

    // 3. A PATCH supplying the current version (1) succeeds and bumps the
    //    version to 2 — proving the counter is monotonic.
    let body_v1 = format!(
        r#"{{"node_id":{node_id},"version":1,"property":"test_counter","value":{{"type":"Int","value":2}}}}"#
    );
    let third = http_patch(port, "/api/node/patch", &body_v1);
    assert_eq!(
        status_code(&third),
        200,
        "fresh PATCH (v=1) must succeed\n--- response ---\n{third}"
    );
    let third_body = response_body(&third);
    let third_json: serde_json::Value = serde_json::from_str(third_body)
        .unwrap_or_else(|e| panic!("PATCH response must be JSON, got: {third_body}\n{e}"));
    assert_eq!(
        third_json["version"].as_u64(),
        Some(2),
        "version must bump to 2 after second successful PATCH; body={third_body}"
    );

    // Sanity: another stale PATCH at v=1 is rejected, confirming the
    // counter doesn't drift backward and the 409 path isn't a fluke.
    let stale_again = http_patch(port, "/api/node/patch", &body_v1);
    assert_eq!(
        status_code(&stale_again),
        409,
        "second stale PATCH must also return 409\n--- response ---\n{stale_again}"
    );

    handle.stop();
}
