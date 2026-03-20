//! Maintenance-only smoke tests for the editor server.
//!
//! These tests verify that the editor server starts, serves basic endpoints,
//! and handles core round-trip operations without crashing. They are stability
//! checks, not feature tests. See AGENTS.md "Editor Feature Gate" for policy.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;
use std::time::Duration;

use gdcore::math::Vector2;
use gdeditor::editor_server::{EditorServerHandle, EditorState};
use gdscene::node::Node;
use gdscene::SceneTree;
use gdvariant::Variant;

fn free_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.local_addr().unwrap().port()
}

fn make_smoke_server() -> (EditorServerHandle, u16) {
    let port = free_port();
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let mut world = Node::new("World", "Node");
    world.set_property("name", Variant::String("World".into()));
    let world_id = tree.add_child(root, world).unwrap();

    let mut player = Node::new("Player", "Node2D");
    player.set_property("position", Variant::Vector2(Vector2::new(10.0, 20.0)));
    tree.add_child(world_id, player).unwrap();

    let state = EditorState::new(tree);
    let handle = EditorServerHandle::start(port, state);
    thread::sleep(Duration::from_millis(300));
    (handle, port)
}

fn http_get(port: u16, path: &str) -> String {
    let req = format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n");
    http_request(port, &req)
}

fn http_post(port: u16, path: &str, body: &str) -> String {
    let req = format!(
        "POST {path} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    http_request(port, &req)
}

fn http_request(port: u16, request: &str) -> String {
    let mut stream = TcpStream::connect(format!("127.0.0.1:{port}")).expect("failed to connect");
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
    stream.write_all(request.as_bytes()).unwrap();
    let mut response = Vec::new();
    let _ = stream.read_to_end(&mut response);
    String::from_utf8_lossy(&response).to_string()
}

fn extract_body(resp: &str) -> &str {
    resp.split("\r\n\r\n").nth(1).unwrap_or("")
}

// --- Smoke tests ---

#[test]
fn smoke_server_starts_and_stops() {
    let (handle, port) = make_smoke_server();
    // Server should accept a connection
    let stream = TcpStream::connect(format!("127.0.0.1:{port}"));
    assert!(stream.is_ok(), "Server should accept TCP connections");
    handle.stop();
}

#[test]
fn smoke_editor_serves_html() {
    let (handle, port) = make_smoke_server();
    let resp = http_get(port, "/editor");
    assert!(resp.contains("200 OK"), "GET /editor should return 200");
    assert!(resp.contains("text/html"), "Should serve HTML content type");
    assert!(resp.contains("Patina"), "HTML should contain 'Patina'");
    handle.stop();
}

#[test]
fn smoke_api_scene_returns_json() {
    let (handle, port) = make_smoke_server();
    let resp = http_get(port, "/api/scene");
    assert!(resp.contains("200 OK"), "GET /api/scene should return 200");
    let body = extract_body(&resp);
    let v: serde_json::Value = serde_json::from_str(body).expect("Response should be valid JSON");
    assert!(v["nodes"].is_object(), "Should have a nodes object");
    assert_eq!(
        v["nodes"]["name"], "root",
        "Root node should be named 'root'"
    );
    handle.stop();
}

#[test]
fn smoke_add_delete_node_round_trip() {
    let (handle, port) = make_smoke_server();

    // Get the World node ID
    let scene_resp = http_get(port, "/api/scene");
    let body = extract_body(&scene_resp);
    let v: serde_json::Value = serde_json::from_str(body).unwrap();
    let world_id = v["nodes"]["children"][0]["id"]
        .as_u64()
        .expect("World node ID");

    // Add a node
    let add_body = format!(r#"{{"parent_id":{world_id},"name":"SmokeNode","class_name":"Node"}}"#);
    let add_resp = http_post(port, "/api/node/add", &add_body);
    assert!(add_resp.contains("200 OK"), "Add node should succeed");
    let add_json: serde_json::Value = serde_json::from_str(extract_body(&add_resp)).unwrap();
    let new_id = add_json["id"].as_u64().expect("New node should have an ID");

    // Verify it exists
    let scene_resp = http_get(port, "/api/scene");
    assert!(
        scene_resp.contains("SmokeNode"),
        "Added node should appear in scene"
    );

    // Delete it
    let del_body = format!(r#"{{"node_id":{new_id}}}"#);
    let del_resp = http_post(port, "/api/node/delete", &del_body);
    assert!(del_resp.contains("200 OK"), "Delete node should succeed");

    // Verify it's gone
    let scene_resp = http_get(port, "/api/scene");
    assert!(
        !scene_resp.contains("SmokeNode"),
        "Deleted node should be gone"
    );

    handle.stop();
}

#[test]
fn smoke_undo_redo_works() {
    let (handle, port) = make_smoke_server();

    // Get World node ID
    let scene_resp = http_get(port, "/api/scene");
    let body = extract_body(&scene_resp);
    let v: serde_json::Value = serde_json::from_str(body).unwrap();
    let world_id = v["nodes"]["children"][0]["id"].as_u64().unwrap();

    // Add a node (creates an undo-able action)
    let add_body = format!(r#"{{"parent_id":{world_id},"name":"UndoMe","class_name":"Node"}}"#);
    http_post(port, "/api/node/add", &add_body);
    assert!(http_get(port, "/api/scene").contains("UndoMe"));

    // Undo
    let undo_resp = http_post(port, "/api/undo", "");
    assert!(undo_resp.contains("200 OK"), "Undo should succeed");
    assert!(
        !http_get(port, "/api/scene").contains("UndoMe"),
        "Node should be undone"
    );

    // Redo
    let redo_resp = http_post(port, "/api/redo", "");
    assert!(redo_resp.contains("200 OK"), "Redo should succeed");
    assert!(
        http_get(port, "/api/scene").contains("UndoMe"),
        "Node should be redone"
    );

    handle.stop();
}

#[test]
fn smoke_save_load_works() {
    let (handle, port) = make_smoke_server();

    let tmp = tempfile::NamedTempFile::new().unwrap();
    let path = tmp.path().to_str().unwrap().to_string();

    // Save
    let save_resp = http_post(port, "/api/scene/save", &format!(r#"{{"path":"{path}"}}"#));
    assert!(save_resp.contains("200 OK"), "Save should succeed");

    let contents = std::fs::read_to_string(&path).unwrap();
    assert!(
        contents.contains("[gd_scene"),
        "Saved file should be a .tscn"
    );

    // Load
    let load_resp = http_post(port, "/api/scene/load", &format!(r#"{{"path":"{path}"}}"#));
    assert!(load_resp.contains("200 OK"), "Load should succeed");

    // Scene should still have World
    let scene_resp = http_get(port, "/api/scene");
    assert!(
        scene_resp.contains("World"),
        "Loaded scene should contain World"
    );

    handle.stop();
}
