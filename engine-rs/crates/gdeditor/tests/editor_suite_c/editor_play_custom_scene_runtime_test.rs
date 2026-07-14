//! Play a custom scene, not the main one (pat-niu1j.5).
//!
//! Godot's "Play Custom Scene" loads an arbitrary scene file and runs it. This
//! drives the assembled server: save a scene to disk, swap the live scene to a
//! fresh one, then load the saved scene back by path and Play it — the runtime
//! runs the loaded (custom) scene's content, distinct from the swapped-in scene.

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
    // The "custom" scene's distinctive content.
    let world = tree.add_child(root, Node::new("CustomLevel", "Node2D")).unwrap();
    tree.add_child(world, Node::new("CustomHero", "Sprite2D")).unwrap();
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

fn json(resp: &str) -> serde_json::Value {
    assert!(resp.contains("200 OK"), "expected 200, got:\n{resp}");
    serde_json::from_str(resp.split("\r\n\r\n").nth(1).unwrap_or("")).expect("valid JSON")
}

/// Does the live scene contain a node with this name (anywhere)?
fn scene_has(port: u16, name: &str) -> bool {
    http_get(port, "/api/scene").contains(&format!("\"name\":\"{name}\""))
}

#[test]
fn editor_play_custom_scene_runtime() {
    let (handle, port) = make_server();

    // Persist the custom scene to a real .tscn on disk.
    let tmp = tempfile::Builder::new().suffix(".tscn").tempfile().unwrap();
    let custom_path = tmp.path().to_str().unwrap().to_string();
    assert!(http_post(port, "/api/scene/save", &format!(r#"{{"path":"{custom_path}"}}"#)).contains("200 OK"));
    assert!(scene_has(port, "CustomLevel"), "custom scene authored");

    // Swap the live editor scene to a fresh, empty one (the "main" scene) so the
    // custom content is gone.
    assert!(http_post(port, "/api/scene/new", "{}").contains("200 OK"));
    assert!(!scene_has(port, "CustomLevel"), "fresh scene has no custom content");

    // Play Custom Scene: load the saved scene back by path, then Play it.
    assert!(http_post(port, "/api/scene/load", &format!(r#"{{"path":"{custom_path}"}}"#)).contains("200 OK"));
    assert!(scene_has(port, "CustomLevel"), "custom scene loaded by path");
    assert!(scene_has(port, "CustomHero"), "custom scene's child loaded");

    let play = json(&http_post(port, "/api/runtime/play", "{}"));
    assert_eq!(play["running"], serde_json::json!(true), "the custom scene is running");

    // The runtime is running the custom scene (its content is live).
    let status = json(&http_get(port, "/api/runtime/status"));
    assert_eq!(status["running"], serde_json::json!(true));
    assert!(scene_has(port, "CustomLevel"), "the running scene is the custom one");

    // Stop tears it down.
    assert!(http_post(port, "/api/runtime/stop", "{}").contains("200 OK"));
    assert_eq!(json(&http_get(port, "/api/runtime/status"))["running"], serde_json::json!(false));

    handle.stop();
}
