//! Project Settings persistence + live Input Map (pat-db37w).
//!
//! Godot's Project Settings dialog persists changed settings and the Input Map
//! tab edits the project's input actions live. This drives the assembled
//! `EditorServerHandle`: POST /api/project_settings persists a changed setting
//! (read back via GET), and /api/input_map lists, adds, updates, and removes
//! input actions.

use gdeditor::editor_server::{EditorServerHandle, EditorState};
use gdscene::node::Node;
use gdscene::SceneTree;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;
use std::time::Duration;

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
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
    http(port, &format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n"))
}

fn http_post(port: u16, path: &str, body: &str) -> String {
    http(port, &format!("POST {path} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()))
}

fn body_json(resp: &str) -> serde_json::Value {
    assert!(resp.contains("200 OK"), "expected 200, got:\n{resp}");
    serde_json::from_str(resp.split("\r\n\r\n").nth(1).unwrap_or("")).expect("valid JSON")
}

#[test]
fn project_settings_and_input_map_live() {
    let (handle, port) = make_server();

    // --- Project Settings persist a changed setting ---
    let set = http_post(port, "/api/project_settings", r#"{"project_name":"PersistCheck123"}"#);
    assert!(set.contains("200 OK"), "POST project_settings ok");
    let after = http_get(port, "/api/project_settings");
    assert!(
        after.contains("PersistCheck123"),
        "GET project_settings reflects the persisted project name"
    );

    // --- Input Map is live (model serializes actions as an object by name) ---
    // Defaults are present.
    let initial = body_json(&http_get(port, "/api/input_map"));
    assert!(initial["actions"]["ui_left"].is_object(), "default actions present");

    // Add an action bound to a key (events carry {Key: "f"}).
    assert!(http_post(port, "/api/input_map", r#"{"action":"fire","keys":["f"]}"#).contains("200 OK"));
    let fire = body_json(&http_get(port, "/api/input_map"));
    let events = fire["actions"]["fire"]["events"].as_array().expect("fire action added with events");
    assert_eq!(events.len(), 1);
    assert_eq!(events[0]["Key"], serde_json::json!("f"));

    // Update its keys.
    assert!(http_post(port, "/api/input_map", r#"{"action":"fire","keys":["f","g"]}"#).contains("200 OK"));
    let upd = body_json(&http_get(port, "/api/input_map"));
    assert_eq!(upd["actions"]["fire"]["events"].as_array().unwrap().len(), 2, "keys updated");

    // Remove it.
    assert!(http_post(port, "/api/input_map", r#"{"action":"fire","remove":true}"#).contains("200 OK"));
    let final_map = body_json(&http_get(port, "/api/input_map"));
    assert!(final_map["actions"]["fire"].is_null(), "fire action removed");

    handle.stop();
}
