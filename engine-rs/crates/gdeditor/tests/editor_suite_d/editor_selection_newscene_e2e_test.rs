//! App-level editor-flow criteria, driven over HTTP (pat-e66em).
//!
//! These drive the assembled `editor_server` the way the browser does — POSTing
//! to the real endpoints and asserting the server state changed — rather than
//! poking the in-process model. Two sibling criteria:
//!   * selection-populates-inspector-app — selecting a node makes the inspector
//!     endpoints report that node (catches the selection bug pat-yl0fg).
//!   * new-scene-creates-scene-app — New Scene adds a second tab + a fresh scene.
//! (The third, mode-switch-renders-view-app, lives in editor_mode_e2e_test.rs.)

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
    let world = tree.add_child(root, Node::new("World", "Node2D")).unwrap();
    tree.add_child(world, Node::new("Hero", "Sprite2D")).unwrap();
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

/// selection-populates-inspector-app: POST /api/node/select makes the inspector
/// endpoints report the selected node's identity + properties.
#[test]
fn selection_populates_inspector_app() {
    let (handle, port) = make_server();

    // Find the "World" node's id from the live scene.
    let scene = body_json(&http_get(port, "/api/scene"));
    let world = &scene["nodes"]["children"][0];
    assert_eq!(world["name"], serde_json::json!("World"));
    let world_id = world["id"].as_u64().expect("node id");

    // Before selecting, the inspector is not showing World.
    let before = body_json(&http_get(port, "/api/inspector/header"));
    assert_ne!(before["name"], serde_json::json!("World"), "World not selected yet");

    // Select it over HTTP.
    assert!(http_post(port, "/api/node/select", &format!(r#"{{"node_id":{world_id}}}"#)).contains("200 OK"));

    // The inspector header now reports the selected node.
    let header = body_json(&http_get(port, "/api/inspector/header"));
    assert_eq!(header["visible"], serde_json::json!(true), "inspector visible after selection");
    assert_eq!(header["name"], serde_json::json!("World"));
    assert_eq!(header["class"], serde_json::json!("Node2D"));

    // /api/selected reflects the selection, and the node endpoint returns props.
    let node = body_json(&http_get(port, &format!("/api/node/{world_id}")));
    assert_eq!(node["name"], serde_json::json!("World"));
    assert_eq!(node["class"], serde_json::json!("Node2D"));

    // Selecting a different node re-populates the inspector (the pat-yl0fg bug:
    // selection didn't flow to the inspector).
    let hero_id = world["children"][0]["id"].as_u64().expect("hero id");
    assert!(http_post(port, "/api/node/select", &format!(r#"{{"node_id":{hero_id}}}"#)).contains("200 OK"));
    let header2 = body_json(&http_get(port, "/api/inspector/header"));
    assert_eq!(header2["name"], serde_json::json!("Hero"), "inspector follows the new selection");
    assert_eq!(header2["class"], serde_json::json!("Sprite2D"));

    handle.stop();
}

/// new-scene-creates-scene-app: Scene > New Scene adds a second tab and swaps in
/// a fresh, empty scene.
#[test]
fn new_scene_creates_scene_app() {
    let (handle, port) = make_server();

    // One tab, and the scene has our World/Hero content.
    let tabs0 = body_json(&http_get(port, "/api/scene/tabs"));
    assert_eq!(tabs0["tabs"].as_array().unwrap().len(), 1, "starts with one tab");
    let scene0 = body_json(&http_get(port, "/api/scene"));
    assert!(!scene0["nodes"]["children"].as_array().unwrap().is_empty(), "scene has content");

    // Create a new scene over HTTP.
    assert!(http_post(port, "/api/scene/new", "{}").contains("200 OK"));

    // A second tab exists and is active; the live scene is now a fresh empty root.
    let tabs1 = body_json(&http_get(port, "/api/scene/tabs"));
    assert_eq!(tabs1["tabs"].as_array().unwrap().len(), 2, "a second scene tab was added: {tabs1}");
    let scene1 = body_json(&http_get(port, "/api/scene"));
    assert!(
        scene1["nodes"]["children"].as_array().map(|c| c.is_empty()).unwrap_or(true),
        "the new scene is a fresh, empty root"
    );

    handle.stop();
}
