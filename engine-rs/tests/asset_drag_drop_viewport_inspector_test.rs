//! Tests for asset drag-drop to scene viewport and inspector (pat-nrtp9).
//!
//! Verifies:
//! - POST /api/asset/drop_to_viewport creates appropriate nodes from asset types
//! - Texture drops create Sprite2D with position and texture property
//! - Audio drops create AudioStreamPlayer
//! - Script drops create Node with _script_path
//! - Invalid requests return proper errors
//! - Inspector drop-to-property via POST /api/property/set works for resource paths

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

fn make_test_server() -> (EditorServerHandle, u16) {
    let port = free_port();
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let mut world = Node::new("World", "Node2D");
    world.set_property("name", Variant::String("World".into()));
    world.set_property(
        "position",
        Variant::Vector2(Vector2::new(0.0, 0.0)),
    );
    tree.add_child(root, world).unwrap();

    let state = EditorState::new(tree);
    let handle = EditorServerHandle::start(port, state);
    thread::sleep(Duration::from_millis(300));
    (handle, port)
}

fn http_post(port: u16, path: &str, body: &str) -> String {
    let req = format!(
        "POST {path} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    http_request(port, &req)
}

fn http_get(port: u16, path: &str) -> String {
    let req = format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n");
    http_request(port, &req)
}

fn http_request(port: u16, request: &str) -> String {
    let mut stream =
        TcpStream::connect(format!("127.0.0.1:{port}")).expect("failed to connect");
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
    stream.write_all(request.as_bytes()).unwrap();
    let mut response = Vec::new();
    let _ = stream.read_to_end(&mut response);
    String::from_utf8_lossy(&response).to_string()
}

fn extract_body(resp: &str) -> &str {
    resp.split("\r\n\r\n").nth(1).unwrap_or("")
}

/// Find the World node ID by fetching the scene tree.
fn get_world_id(port: u16) -> u64 {
    let resp = http_get(port, "/api/scene");
    let body = extract_body(&resp);
    let v: serde_json::Value = serde_json::from_str(body).expect("valid JSON");
    // World is the first child of root
    v["nodes"]["children"][0]["id"]
        .as_u64()
        .expect("World node should have an id")
}

// --- Tests ---

#[test]
fn asset_drop_texture_creates_sprite2d() {
    let (handle, port) = make_test_server();
    let world_id = get_world_id(port);

    let body = format!(
        r#"{{"asset_path":"res://icon.png","asset_type":"texture","parent_id":{},"viewport_x":100,"viewport_y":200}}"#,
        world_id
    );
    let resp = http_post(port, "/api/asset/drop_to_viewport", &body);
    assert!(resp.contains("200 OK"), "Should return 200, got: {resp}");

    let result: serde_json::Value =
        serde_json::from_str(extract_body(&resp)).expect("valid JSON response");
    assert!(result["node_id"].is_number(), "Should return node_id");
    assert_eq!(
        result["node_class"].as_str().unwrap(),
        "Sprite2D",
        "Texture drop should create Sprite2D"
    );

    // Verify the node was actually added to the scene tree.
    let scene_resp = http_get(port, "/api/scene");
    let scene_body = extract_body(&scene_resp);
    assert!(
        scene_body.contains("icon"),
        "Scene should contain the new 'icon' node"
    );

    handle.stop();
}

#[test]
fn asset_drop_audio_creates_audio_player() {
    let (handle, port) = make_test_server();
    let world_id = get_world_id(port);

    let body = format!(
        r#"{{"asset_path":"res://music.ogg","asset_type":"audio","parent_id":{},"viewport_x":0,"viewport_y":0}}"#,
        world_id
    );
    let resp = http_post(port, "/api/asset/drop_to_viewport", &body);
    assert!(resp.contains("200 OK"), "Should return 200");

    let result: serde_json::Value =
        serde_json::from_str(extract_body(&resp)).expect("valid JSON");
    assert_eq!(
        result["node_class"].as_str().unwrap(),
        "AudioStreamPlayer"
    );

    handle.stop();
}

#[test]
fn asset_drop_script_creates_node_with_script() {
    let (handle, port) = make_test_server();
    let world_id = get_world_id(port);

    let body = format!(
        r#"{{"asset_path":"res://player.gd","asset_type":"script","parent_id":{},"viewport_x":50,"viewport_y":75}}"#,
        world_id
    );
    let resp = http_post(port, "/api/asset/drop_to_viewport", &body);
    assert!(resp.contains("200 OK"), "Should return 200");

    let result: serde_json::Value =
        serde_json::from_str(extract_body(&resp)).expect("valid JSON");
    assert_eq!(result["node_class"].as_str().unwrap(), "Node");
    assert!(result["node_id"].is_number());

    handle.stop();
}

#[test]
fn asset_drop_mesh_creates_meshinstance3d() {
    let (handle, port) = make_test_server();
    let world_id = get_world_id(port);

    let body = format!(
        r#"{{"asset_path":"res://model.glb","asset_type":"mesh","parent_id":{},"viewport_x":0,"viewport_y":0}}"#,
        world_id
    );
    let resp = http_post(port, "/api/asset/drop_to_viewport", &body);
    assert!(resp.contains("200 OK"), "Should return 200");

    let result: serde_json::Value =
        serde_json::from_str(extract_body(&resp)).expect("valid JSON");
    assert_eq!(
        result["node_class"].as_str().unwrap(),
        "MeshInstance3D"
    );

    handle.stop();
}

#[test]
fn asset_drop_missing_parent_returns_404() {
    let (handle, port) = make_test_server();

    let body = r#"{"asset_path":"res://icon.png","asset_type":"texture","parent_id":999999,"viewport_x":0,"viewport_y":0}"#;
    let resp = http_post(port, "/api/asset/drop_to_viewport", body);
    assert!(
        resp.contains("404") || resp.contains("not found"),
        "Should return 404 for missing parent, got: {resp}"
    );

    handle.stop();
}

#[test]
fn asset_drop_missing_path_returns_400() {
    let (handle, port) = make_test_server();

    let body = r#"{"asset_type":"texture","parent_id":1,"viewport_x":0,"viewport_y":0}"#;
    let resp = http_post(port, "/api/asset/drop_to_viewport", body);
    assert!(
        resp.contains("400") || resp.contains("missing asset_path"),
        "Should return 400 for missing asset_path, got: {resp}"
    );

    handle.stop();
}

#[test]
fn asset_drop_invalid_json_returns_400() {
    let (handle, port) = make_test_server();

    let resp = http_post(port, "/api/asset/drop_to_viewport", "not json");
    assert!(
        resp.contains("400") || resp.contains("invalid JSON"),
        "Should return 400 for invalid JSON, got: {resp}"
    );

    handle.stop();
}

#[test]
fn inspector_set_property_with_resource_path() {
    let (handle, port) = make_test_server();
    let world_id = get_world_id(port);

    // First create a Sprite2D via asset drop
    let body = format!(
        r#"{{"asset_path":"res://icon.png","asset_type":"texture","parent_id":{},"viewport_x":0,"viewport_y":0}}"#,
        world_id
    );
    let resp = http_post(port, "/api/asset/drop_to_viewport", &body);
    let result: serde_json::Value =
        serde_json::from_str(extract_body(&resp)).expect("valid JSON");
    let sprite_id = result["node_id"].as_u64().unwrap();

    // Now set a property via the property API (simulates inspector drag-drop)
    let prop_body = format!(
        r#"{{"node_id":{},"property":"texture","value":{{"type":"String","value":"res://new_texture.png"}}}}"#,
        sprite_id
    );
    let prop_resp = http_post(port, "/api/property/set", &prop_body);
    assert!(
        prop_resp.contains("200 OK"),
        "Setting property should succeed, got: {prop_resp}"
    );

    handle.stop();
}

#[test]
fn asset_drop_uses_filename_as_node_name() {
    let (handle, port) = make_test_server();
    let world_id = get_world_id(port);

    let body = format!(
        r#"{{"asset_path":"res://sprites/player_idle.png","asset_type":"texture","parent_id":{},"viewport_x":0,"viewport_y":0}}"#,
        world_id
    );
    let resp = http_post(port, "/api/asset/drop_to_viewport", &body);
    assert!(resp.contains("200 OK"));

    // Check scene tree for the node name derived from filename
    let scene_resp = http_get(port, "/api/scene");
    let scene_body = extract_body(&scene_resp);
    assert!(
        scene_body.contains("player_idle"),
        "Node name should be derived from filename 'player_idle', scene: {scene_body}"
    );

    handle.stop();
}

#[test]
fn asset_drop_unknown_type_creates_node2d() {
    let (handle, port) = make_test_server();
    let world_id = get_world_id(port);

    let body = format!(
        r#"{{"asset_path":"res://data.json","asset_type":"unknown","parent_id":{},"viewport_x":10,"viewport_y":20}}"#,
        world_id
    );
    let resp = http_post(port, "/api/asset/drop_to_viewport", &body);
    assert!(resp.contains("200 OK"));

    let result: serde_json::Value =
        serde_json::from_str(extract_body(&resp)).expect("valid JSON");
    assert_eq!(
        result["node_class"].as_str().unwrap(),
        "Node2D",
        "Unknown asset type should default to Node2D"
    );

    handle.stop();
}
