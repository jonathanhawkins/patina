//! pat-rk3md: `Idempotency-Key` header dedupe on state-mutating endpoints.
//!
//! Acceptance: a POST with `Idempotency-Key` returns the cached response and
//! does not re-apply the mutation when sent twice within the cache window.

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

fn http_post_idem(port: u16, path: &str, body: &str, key: Option<&str>) -> String {
    let header = match key {
        Some(k) => format!("Idempotency-Key: {k}\r\n"),
        None => String::new(),
    };
    let req = format!(
        "POST {path} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {len}\r\n{header}Connection: close\r\n\r\n{body}",
        len = body.len(),
    );
    http_raw(port, &req)
}

/// Count scene nodes by GETting /api/scene and inspecting the JSON. The
/// concrete shape varies but every Patina scene reply carries a `nodes` array.
fn scene_node_count(port: u16) -> usize {
    let resp = http_get(port, "/api/scene");
    let body = response_body(&resp);
    let v: serde_json::Value = serde_json::from_str(body)
        .unwrap_or_else(|e| panic!("/api/scene must return JSON, got: {body}\n{e}"));
    v["nodes"].as_array().map(|a| a.len()).unwrap_or(0)
}

#[test]
fn post_with_idempotency_key_replays_response_and_runs_handler_once() {
    let port = free_port();
    let handle = EditorServerHandle::start(port, fixture_state());
    thread::sleep(Duration::from_millis(120));

    let initial = scene_node_count(port);

    // First POST: adds a child to the root (parent_id=0). The handler should
    // run, the response is cached for this Idempotency-Key.
    let body = r#"{"parent_id":0,"name":"Spawned","class":"Node2D"}"#;
    let first = http_post_idem(port, "/api/node/add", body, Some("idem-test-1"));
    assert_eq!(
        status_code(&first),
        200,
        "first POST should succeed\n--- response ---\n{first}"
    );
    let after_first = scene_node_count(port);
    assert!(
        after_first > initial,
        "first POST must mutate state: before={initial} after={after_first}"
    );

    let first_body = response_body(&first).to_string();

    // Second POST with the same Idempotency-Key. The handler must NOT run a
    // second time, so the node count must stay the same as after the first
    // call. The response body should match the cached first response.
    let second = http_post_idem(port, "/api/node/add", body, Some("idem-test-1"));
    assert_eq!(
        status_code(&second),
        200,
        "second POST replay should be 200\n--- response ---\n{second}"
    );
    let after_second = scene_node_count(port);
    assert_eq!(
        after_first, after_second,
        "duplicate POST with same Idempotency-Key must NOT re-apply the mutation; \
         before={after_first} after={after_second}"
    );
    let second_body = response_body(&second);
    assert_eq!(
        first_body, second_body,
        "replay body must equal the cached original body"
    );

    // Sanity: a POST with a *different* Idempotency-Key DOES run the handler.
    let third = http_post_idem(port, "/api/node/add", body, Some("idem-test-2"));
    assert_eq!(status_code(&third), 200);
    let after_third = scene_node_count(port);
    assert!(
        after_third > after_second,
        "different Idempotency-Key must run handler again: before={after_second} after={after_third}"
    );

    handle.stop();
}
