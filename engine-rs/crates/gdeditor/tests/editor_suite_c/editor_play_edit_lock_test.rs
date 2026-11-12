//! Play-mode edit-lock (pat-niu1j.2).
//!
//! Godot makes the editor read-only while the scene is running ("Stop the scene
//! to edit"). This drives the assembled server: a scene-mutating route succeeds
//! while editing, is rejected (edit-locked) once Play starts, and becomes
//! writable again after Stop.

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
    tree.add_child(root, Node::new("World", "Node2D")).unwrap();
    let handle = EditorServerHandle::start(port, EditorState::new(tree));
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

fn http(port: u16, req: &str) -> String {
    let mut stream = connect_with_retry(port);
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
    stream.write_all(req.as_bytes()).unwrap();
    let mut resp = Vec::new();
    let _ = stream.read_to_end(&mut resp);
    String::from_utf8_lossy(&resp).to_string()
}

fn http_get(port: u16, path: &str) -> String {
    http(
        port,
        &format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n"),
    )
}

fn http_post(port: u16, path: &str, body: &str) -> String {
    http(port, &format!("POST {path} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()))
}

fn first_child_id(port: u16) -> u64 {
    let scene: serde_json::Value = serde_json::from_str(
        http_get(port, "/api/scene")
            .split("\r\n\r\n")
            .nth(1)
            .unwrap_or(""),
    )
    .unwrap();
    scene["nodes"]["children"][0]["id"]
        .as_u64()
        .expect("a child node id")
}

#[test]
fn editor_play_edit_lock() {
    let (handle, port) = make_server();
    let node = first_child_id(port);

    // While editing, a tree-mutating route succeeds.
    let before = http_post(
        port,
        "/api/node/groups/add",
        &format!(r#"{{"node_id":{node},"group":"editable"}}"#),
    );
    assert!(
        before.contains("200 OK"),
        "mutation allowed while editing, got:\n{before}"
    );

    // Play starts the runtime.
    assert!(http_post(port, "/api/runtime/play", "{}").contains("200 OK"));

    // Now the same mutation is rejected — the editor is read-only while running.
    let locked = http_post(
        port,
        "/api/node/groups/add",
        &format!(r#"{{"node_id":{node},"group":"while_running"}}"#),
    );
    assert!(
        !locked.contains("200 OK"),
        "mutation must be blocked while running, got:\n{locked}"
    );
    assert!(
        locked.contains("edit_locked")
            || locked.contains("read-only")
            || locked.contains("running"),
        "rejection should explain the play-mode edit-lock, got:\n{locked}"
    );

    // Stop lifts the lock — the route is writable again.
    assert!(http_post(port, "/api/runtime/stop", "{}").contains("200 OK"));
    let after = http_post(
        port,
        "/api/node/groups/add",
        &format!(r#"{{"node_id":{node},"group":"after_stop"}}"#),
    );
    assert!(
        after.contains("200 OK"),
        "mutation allowed again after Stop, got:\n{after}"
    );

    handle.stop();
}
