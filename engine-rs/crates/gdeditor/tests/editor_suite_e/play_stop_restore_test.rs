//! pat-niu1j.3 — Stop tears down the run instance and restores editable state.
//!
//! Sub-task of pat-niu1j (Play / Stop / Pause run workflow with edit-lock).
//! Verifies the full play -> stop cycle: starting a run builds the engine-owned
//! `run_main_loop` instance and marks the editor non-editable; stopping tears
//! that instance down, resets the runtime counters, flips the status back to
//! `editable: true`, and leaves the editable scene tree mutable again.

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

fn status_json(port: u16) -> serde_json::Value {
    let resp = http_get(port, "/api/runtime/status");
    serde_json::from_str(extract_body(&resp)).unwrap()
}

fn main_node_id(port: u16) -> u64 {
    let resp = http_get(port, "/api/scene");
    let v: serde_json::Value = serde_json::from_str(extract_body(&resp)).unwrap();
    v["nodes"]["children"][0]["id"].as_u64().unwrap()
}

/// Full play -> stop cycle: play builds the run instance and locks editing;
/// stop tears the instance down and restores the editable state.
#[test]
fn editor_stop_restores_editable() {
    let (handle, port) = make_server();

    // Baseline: not running, no run instance, editor is editable.
    let st = status_json(port);
    assert_eq!(st["running"], false, "should not be running at startup");
    assert!(
        handle.state().lock().unwrap().run_main_loop.is_none(),
        "no run instance should exist before play"
    );

    // Play: builds the engine-owned run loop and locks editing.
    let resp = http_post(port, "/api/runtime/play", "");
    assert!(resp.contains("200 OK"), "play should succeed: {resp}");
    let st = status_json(port);
    assert_eq!(st["running"], true, "should be running after play");
    assert!(
        handle.state().lock().unwrap().run_main_loop.is_some(),
        "play should build the run instance"
    );

    // Advance a frame so there is runtime state to tear down.
    http_post(port, "/api/runtime/pause", "");
    http_post(port, "/api/runtime/step", "");
    assert!(
        handle.state().lock().unwrap().runtime_frame_count > 0,
        "stepping a paused run should advance the frame counter"
    );

    // Stop: tears the instance down and restores the editable state.
    let resp = http_post(port, "/api/runtime/stop", "");
    assert!(resp.contains("200 OK"), "stop should succeed: {resp}");
    let body: serde_json::Value = serde_json::from_str(extract_body(&resp)).unwrap();
    assert_eq!(body["running"], false, "stop response should report stopped");

    // Status reflects the restored, editable, idle editor.
    let st = status_json(port);
    assert_eq!(st["running"], false, "should not be running after stop");
    assert_eq!(st["paused"], false, "pause state should clear on stop");
    assert_eq!(st["frame_count"], 0, "frame counter should reset on stop");

    // The run instance is fully torn down.
    {
        let guard = handle.state().lock().unwrap();
        assert!(
            guard.run_main_loop.is_none(),
            "stop should tear down the run instance"
        );
        assert!(!guard.is_running, "is_running should be false after stop");
        assert!(!guard.is_paused, "is_paused should be false after stop");
    }

    // Stepping is rejected because the instance is gone, not merely paused.
    let resp = http_post(port, "/api/runtime/step", "");
    assert!(
        resp.contains("400"),
        "stepping after teardown should be rejected: {resp}"
    );

    // Editing is restored: a mutation against the editable scene tree succeeds.
    let id = main_node_id(port);
    let resp = http_post(
        port,
        "/api/node/rename",
        &format!(r#"{{"node_id":{id},"new_name":"Restored"}}"#),
    );
    assert!(
        resp.contains("200 OK"),
        "editing should be restored after stop: {resp}"
    );
    let v: serde_json::Value = serde_json::from_str(extract_body(&resp)).unwrap();
    assert_eq!(v["ok"], true, "rename should succeed after stop");

    // The edit landed on the live editable tree.
    let resp = http_get(port, "/api/scene");
    let scene: serde_json::Value = serde_json::from_str(extract_body(&resp)).unwrap();
    assert_eq!(
        scene["nodes"]["children"][0]["name"], "Restored",
        "renamed node should be visible in the editable scene tree"
    );

    handle.stop();
}
