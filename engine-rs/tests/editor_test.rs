//! Integration tests for the Patina editor server and web UI.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;
use std::time::Duration;

use gdcore::math::{Color, Vector2};
use gdeditor::editor_server::{EditorServerHandle, EditorState};
use gdrender2d::renderer::FrameBuffer;
use gdscene::node::Node;
use gdscene::SceneTree;
use gdvariant::Variant;

fn free_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.local_addr().unwrap().port()
}

fn make_test_server() -> (EditorServerHandle, u16) {
    let port = free_port();
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let mut world = Node::new("World", "Node");
    world.set_property("name", Variant::String("World".into()));
    let world_id = tree.add_child(root, world).unwrap();

    let mut player = Node::new("Player", "Node2D");
    player.set_property("position", Variant::Vector2(Vector2::new(100.0, 200.0)));
    player.set_property("rotation", Variant::Float(0.0));
    player.set_property("visible", Variant::Bool(true));
    tree.add_child(world_id, player).unwrap();

    let enemy = Node::new("Enemy", "Node2D");
    tree.add_child(world_id, enemy).unwrap();

    let ground = Node::new("Ground", "Node2D");
    tree.add_child(world_id, ground).unwrap();

    let state = EditorState::new(tree);
    let handle = EditorServerHandle::start(port, state);
    thread::sleep(Duration::from_millis(300));
    (handle, port)
}

fn http_get(port: u16, path: &str) -> String {
    let req = format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n");
    http_request_str(port, &req)
}

fn http_post(port: u16, path: &str, body: &str) -> String {
    let req = format!(
        "POST {path} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    http_request_str(port, &req)
}

fn http_request_str(port: u16, request: &str) -> String {
    let mut stream = TcpStream::connect(format!("127.0.0.1:{port}")).expect("failed to connect");
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
    stream.write_all(request.as_bytes()).unwrap();
    let mut response = Vec::new();
    let _ = stream.read_to_end(&mut response);
    String::from_utf8_lossy(&response).to_string()
}

fn http_request_raw(port: u16, request: &str) -> Vec<u8> {
    let mut stream = TcpStream::connect(format!("127.0.0.1:{port}")).expect("failed to connect");
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
    stream.write_all(request.as_bytes()).unwrap();
    let mut response = Vec::new();
    let _ = stream.read_to_end(&mut response);
    response
}

fn extract_body(resp: &str) -> &str {
    resp.split("\r\n\r\n").nth(1).unwrap_or("")
}

fn get_world_node_id(port: u16) -> u64 {
    let resp = http_get(port, "/api/scene");
    let body = extract_body(&resp);
    let v: serde_json::Value = serde_json::from_str(body).unwrap();
    v["nodes"]["children"][0]["id"].as_u64().unwrap()
}

fn get_player_node_id(port: u16) -> u64 {
    let resp = http_get(port, "/api/scene");
    let body = extract_body(&resp);
    let v: serde_json::Value = serde_json::from_str(body).expect("scene JSON parse failed");
    let world = &v["nodes"]["children"][0];
    assert!(
        world["name"].is_string(),
        "World node not found in scene: {v}"
    );
    let player = &world["children"][0];
    assert!(
        player["name"].is_string(),
        "Player node not found under World: {world}"
    );
    player["id"].as_u64().expect("Player node missing id")
}

// --- Tests ---

#[test]
fn editor_html_contains_patina() {
    let (handle, port) = make_test_server();
    let resp = http_get(port, "/editor");
    assert!(resp.contains("200 OK"));
    assert!(
        resp.contains("Patina"),
        "Editor HTML should contain 'Patina'"
    );
    assert!(resp.contains("text/html"));
    handle.stop();
}

#[test]
fn editor_html_contains_ui_elements() {
    let (handle, port) = make_test_server();
    let resp = http_get(port, "/editor");
    assert!(
        resp.contains("scene-tree"),
        "Should contain scene tree panel"
    );
    assert!(resp.contains("inspector"), "Should contain inspector panel");
    assert!(resp.contains("viewport"), "Should contain viewport panel");
    assert!(resp.contains("toolbar"), "Should contain toolbar");
    handle.stop();
}

#[test]
fn api_scene_returns_json_with_nodes() {
    let (handle, port) = make_test_server();
    let resp = http_get(port, "/api/scene");
    assert!(resp.contains("200 OK"));
    let body = extract_body(&resp);
    let v: serde_json::Value = serde_json::from_str(body).unwrap();
    assert_eq!(v["nodes"]["name"], "root");
    let children = v["nodes"]["children"].as_array().unwrap();
    assert!(!children.is_empty());
    assert_eq!(children[0]["name"], "World");
    // World should have Player, Enemy, Ground
    let world_children = children[0]["children"].as_array().unwrap();
    assert_eq!(world_children.len(), 3);
    assert_eq!(world_children[0]["name"], "Player");
    assert_eq!(world_children[1]["name"], "Enemy");
    assert_eq!(world_children[2]["name"], "Ground");
    handle.stop();
}

#[test]
fn select_then_get_selected() {
    let (handle, port) = make_test_server();
    let player_id = get_player_node_id(port);

    // Select the player
    let resp = http_post(
        port,
        "/api/node/select",
        &format!(r#"{{"node_id":{player_id}}}"#),
    );
    assert!(resp.contains("200 OK"));

    // Get selected should return the player
    let resp = http_get(port, "/api/selected");
    let body = extract_body(&resp);
    let v: serde_json::Value = serde_json::from_str(body).unwrap();
    assert_eq!(v["name"], "Player");
    assert_eq!(v["class"], "Node2D");
    assert!(v["properties"].as_array().unwrap().len() >= 1);
    handle.stop();
}

#[test]
fn set_property_then_verify() {
    let (handle, port) = make_test_server();
    let player_id = get_player_node_id(port);

    // Set a new property
    let body = format!(
        r#"{{"node_id":{player_id},"property":"health","value":{{"type":"Int","value":100}}}}"#
    );
    let resp = http_post(port, "/api/property/set", &body);
    assert!(resp.contains("200 OK"));

    // Verify via GET /api/node/<id>
    let resp = http_get(port, &format!("/api/node/{player_id}"));
    let body = extract_body(&resp);
    let v: serde_json::Value = serde_json::from_str(body).unwrap();
    let health = v["properties"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["name"] == "health")
        .expect("health property should exist");
    assert_eq!(health["type"], "Int");
    handle.stop();
}

#[test]
fn add_node_then_verify_in_scene() {
    let (handle, port) = make_test_server();
    let world_id = get_world_node_id(port);

    let body = format!(r#"{{"parent_id":{world_id},"name":"NewSprite","class_name":"Sprite2D"}}"#);
    let resp = http_post(port, "/api/node/add", &body);
    assert!(resp.contains("200 OK"));
    let resp_body: serde_json::Value = serde_json::from_str(extract_body(&resp)).unwrap();
    assert!(resp_body["id"].as_u64().is_some());

    // Verify the node appears in the scene tree
    let scene_resp = http_get(port, "/api/scene");
    assert!(scene_resp.contains("NewSprite"));
    assert!(scene_resp.contains("Sprite2D"));
    handle.stop();
}

#[test]
fn undo_reverses_add_node() {
    let (handle, port) = make_test_server();
    let world_id = get_world_node_id(port);

    // Add a node
    let body = format!(r#"{{"parent_id":{world_id},"name":"TempNode","class_name":"Node"}}"#);
    http_post(port, "/api/node/add", &body);

    // Verify it exists
    let scene_resp = http_get(port, "/api/scene");
    assert!(scene_resp.contains("TempNode"));

    // Undo
    let resp = http_post(port, "/api/undo", "");
    assert!(resp.contains("200 OK"));

    // Verify it's gone
    let scene_resp = http_get(port, "/api/scene");
    assert!(!scene_resp.contains("TempNode"));
    handle.stop();
}

#[test]
fn redo_restores_undone_action() {
    let (handle, port) = make_test_server();
    let player_id = get_player_node_id(port);

    // Set property
    let body = format!(
        r#"{{"node_id":{player_id},"property":"speed","value":{{"type":"Float","value":5.5}}}}"#
    );
    http_post(port, "/api/property/set", &body);

    // Undo
    http_post(port, "/api/undo", "");

    // Redo
    let resp = http_post(port, "/api/redo", "");
    assert!(resp.contains("200 OK"));

    // Verify property is back
    let node_resp = http_get(port, &format!("/api/node/{player_id}"));
    let body = extract_body(&node_resp);
    assert!(body.contains("speed"));
    handle.stop();
}

#[test]
fn delete_node_removes_from_tree() {
    let (handle, port) = make_test_server();
    let world_id = get_world_node_id(port);

    // Add then delete
    let add_body = format!(r#"{{"parent_id":{world_id},"name":"ToDelete","class_name":"Node"}}"#);
    let add_resp = http_post(port, "/api/node/add", &add_body);
    let add_json: serde_json::Value = serde_json::from_str(extract_body(&add_resp)).unwrap();
    let new_id = add_json["id"].as_u64().unwrap();

    let del_body = format!(r#"{{"node_id":{new_id}}}"#);
    let resp = http_post(port, "/api/node/delete", &del_body);
    assert!(resp.contains("200 OK"));

    let scene_resp = http_get(port, "/api/scene");
    assert!(!scene_resp.contains("ToDelete"));
    handle.stop();
}

#[test]
fn scene_save_writes_file() {
    let (handle, port) = make_test_server();

    let tmp = tempfile::NamedTempFile::new().unwrap();
    let path = tmp.path().to_str().unwrap().to_string();

    let save_body = format!(r#"{{"path":"{path}"}}"#);
    let resp = http_post(port, "/api/scene/save", &save_body);
    assert!(resp.contains("200 OK"));

    let contents = std::fs::read_to_string(&path).unwrap();
    assert!(contents.contains("[gd_scene"));
    assert!(contents.contains("World"));
    handle.stop();
}

#[test]
fn scene_load_replaces_tree() {
    let (handle, port) = make_test_server();

    // Save current scene
    let tmp = tempfile::NamedTempFile::new().unwrap();
    let path = tmp.path().to_str().unwrap().to_string();
    http_post(port, "/api/scene/save", &format!(r#"{{"path":"{path}"}}"#));

    // Load it back
    let resp = http_post(port, "/api/scene/load", &format!(r#"{{"path":"{path}"}}"#));
    assert!(resp.contains("200 OK"));

    // Tree should still have World
    let scene_resp = http_get(port, "/api/scene");
    assert!(scene_resp.contains("World"));
    handle.stop();
}

#[test]
fn viewport_returns_valid_bmp() {
    let (handle, port) = make_test_server();

    // Upload a frame
    let fb = FrameBuffer::new(8, 8, Color::rgb(0.5, 0.2, 0.8));
    handle.update_frame(fb);

    let resp = http_request_raw(
        port,
        "GET /api/viewport HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
    );
    let resp_str = String::from_utf8_lossy(&resp);
    assert!(resp_str.contains("200 OK"));
    assert!(resp_str.contains("image/bmp"));

    // Verify BMP magic bytes
    let bm_pos = resp.windows(2).position(|w| w == b"BM");
    assert!(bm_pos.is_some(), "Response should contain BMP header");
    handle.stop();
}

#[test]
fn viewport_no_frame_returns_error() {
    let (handle, port) = make_test_server();
    let resp = http_get(port, "/api/viewport");
    assert!(resp.contains("404") || resp.contains("no frame"));
    handle.stop();
}

#[test]
fn set_vector2_property() {
    let (handle, port) = make_test_server();
    let player_id = get_player_node_id(port);

    let body = format!(
        r#"{{"node_id":{player_id},"property":"position","value":{{"type":"Vector2","value":[300,400]}}}}"#
    );
    let resp = http_post(port, "/api/property/set", &body);
    assert!(resp.contains("200 OK"));

    let node_resp = http_get(port, &format!("/api/node/{player_id}"));
    let body = extract_body(&node_resp);
    let v: serde_json::Value = serde_json::from_str(body).unwrap();
    let pos = v["properties"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["name"] == "position")
        .unwrap();
    assert_eq!(pos["type"], "Vector2");
    handle.stop();
}

#[test]
fn undo_empty_returns_error() {
    let (handle, port) = make_test_server();
    let resp = http_post(port, "/api/undo", "");
    assert!(resp.contains("400"));
    assert!(resp.contains("nothing to undo"));
    handle.stop();
}

#[test]
fn cors_preflight_supported() {
    let (handle, port) = make_test_server();
    let resp = http_request_str(
        port,
        "OPTIONS /api/scene HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
    );
    assert!(resp.contains("204 No Content"));
    assert!(resp.contains("Access-Control-Allow-Origin: *"));
    handle.stop();
}

#[test]
fn unknown_path_returns_404() {
    let (handle, port) = make_test_server();
    let resp = http_get(port, "/nonexistent");
    assert!(resp.contains("404"));
    handle.stop();
}

#[test]
fn get_node_by_id() {
    let (handle, port) = make_test_server();
    let player_id = get_player_node_id(port);

    let resp = http_get(port, &format!("/api/node/{player_id}"));
    assert!(resp.contains("200 OK"));
    let body = extract_body(&resp);
    let v: serde_json::Value = serde_json::from_str(body).unwrap();
    assert_eq!(v["name"], "Player");
    assert_eq!(v["class"], "Node2D");
    handle.stop();
}
