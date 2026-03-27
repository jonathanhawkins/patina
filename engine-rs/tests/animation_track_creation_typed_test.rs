//! Tests for animation editor track creation with property, method, and audio types (pat-n86px).
//!
//! Verifies that the editor API supports creating typed animation tracks,
//! that track types are correctly persisted and returned, and that keyframe
//! operations work for all three track types.

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

    let mut player = Node::new("Player", "Node2D");
    player.set_property("position", Variant::Vector2(Vector2::new(0.0, 0.0)));
    tree.add_child(root, player).unwrap();

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

// --- Explicit track/add endpoint ---

#[test]
fn add_property_track_explicitly() {
    let (handle, port) = make_test_server();

    http_post(port, "/api/animation/create", r#"{"name":"Walk","length":1.0}"#);

    let resp = http_post(
        port,
        "/api/animation/track/add",
        r#"{"animation":"Walk","node_path":"Player","property":"position","track_type":"property"}"#,
    );
    assert!(resp.contains("200 OK"), "Add property track should succeed");
    let body = extract_body(&resp);
    let result: serde_json::Value = serde_json::from_str(body).unwrap();
    assert_eq!(result["track_type"], "property");

    // Verify via GET
    let resp = http_get(port, "/api/animation?name=Walk");
    let body = extract_body(&resp);
    let anim: serde_json::Value = serde_json::from_str(body).unwrap();
    let tracks = anim["tracks"].as_array().unwrap();
    assert_eq!(tracks.len(), 1);
    assert_eq!(tracks[0]["track_type"], "property");
    assert_eq!(tracks[0]["node_path"], "Player");
    assert_eq!(tracks[0]["property"], "position");

    handle.stop();
}

#[test]
fn add_method_track_explicitly() {
    let (handle, port) = make_test_server();

    http_post(port, "/api/animation/create", r#"{"name":"Effects","length":2.0}"#);

    let resp = http_post(
        port,
        "/api/animation/track/add",
        r#"{"animation":"Effects","node_path":"Player","property":"play_effect","track_type":"method"}"#,
    );
    assert!(resp.contains("200 OK"), "Add method track should succeed");
    let body = extract_body(&resp);
    let result: serde_json::Value = serde_json::from_str(body).unwrap();
    assert_eq!(result["track_type"], "method");

    let resp = http_get(port, "/api/animation?name=Effects");
    let body = extract_body(&resp);
    let anim: serde_json::Value = serde_json::from_str(body).unwrap();
    let tracks = anim["tracks"].as_array().unwrap();
    assert_eq!(tracks.len(), 1);
    assert_eq!(tracks[0]["track_type"], "method");

    handle.stop();
}

#[test]
fn add_audio_track_explicitly() {
    let (handle, port) = make_test_server();

    http_post(port, "/api/animation/create", r#"{"name":"Soundtrack","length":5.0}"#);

    let resp = http_post(
        port,
        "/api/animation/track/add",
        r#"{"animation":"Soundtrack","node_path":"AudioPlayer","property":"footstep","track_type":"audio"}"#,
    );
    assert!(resp.contains("200 OK"), "Add audio track should succeed");
    let body = extract_body(&resp);
    let result: serde_json::Value = serde_json::from_str(body).unwrap();
    assert_eq!(result["track_type"], "audio");

    let resp = http_get(port, "/api/animation?name=Soundtrack");
    let body = extract_body(&resp);
    let anim: serde_json::Value = serde_json::from_str(body).unwrap();
    let tracks = anim["tracks"].as_array().unwrap();
    assert_eq!(tracks.len(), 1);
    assert_eq!(tracks[0]["track_type"], "audio");

    handle.stop();
}

#[test]
fn duplicate_track_rejected() {
    let (handle, port) = make_test_server();

    http_post(port, "/api/animation/create", r#"{"name":"Dup","length":1.0}"#);

    http_post(
        port,
        "/api/animation/track/add",
        r#"{"animation":"Dup","node_path":"Player","property":"position","track_type":"property"}"#,
    );

    // Same track again should fail
    let resp = http_post(
        port,
        "/api/animation/track/add",
        r#"{"animation":"Dup","node_path":"Player","property":"position","track_type":"property"}"#,
    );
    assert!(resp.contains("400"), "Duplicate track should be rejected");

    handle.stop();
}

#[test]
fn same_node_property_different_types_allowed() {
    let (handle, port) = make_test_server();

    http_post(port, "/api/animation/create", r#"{"name":"Multi","length":1.0}"#);

    // Property track
    let resp = http_post(
        port,
        "/api/animation/track/add",
        r#"{"animation":"Multi","node_path":"Player","property":"position","track_type":"property"}"#,
    );
    assert!(resp.contains("200 OK"));

    // Method track with same node+property name but different type
    let resp = http_post(
        port,
        "/api/animation/track/add",
        r#"{"animation":"Multi","node_path":"Player","property":"position","track_type":"method"}"#,
    );
    assert!(resp.contains("200 OK"), "Different track_type should be allowed");

    let resp = http_get(port, "/api/animation?name=Multi");
    let body = extract_body(&resp);
    let anim: serde_json::Value = serde_json::from_str(body).unwrap();
    let tracks = anim["tracks"].as_array().unwrap();
    assert_eq!(tracks.len(), 2);

    handle.stop();
}

#[test]
fn invalid_track_type_rejected() {
    let (handle, port) = make_test_server();

    http_post(port, "/api/animation/create", r#"{"name":"Bad","length":1.0}"#);

    let resp = http_post(
        port,
        "/api/animation/track/add",
        r#"{"animation":"Bad","node_path":"Player","property":"pos","track_type":"banana"}"#,
    );
    assert!(resp.contains("400"), "Invalid track_type should be rejected");

    handle.stop();
}

// --- Keyframe add with track_type ---

#[test]
fn add_keyframe_with_method_track_type() {
    let (handle, port) = make_test_server();

    http_post(port, "/api/animation/create", r#"{"name":"MethodAnim","length":2.0}"#);

    // Add keyframe with track_type=method — should auto-create method track
    let resp = http_post(
        port,
        "/api/animation/keyframe/add",
        r#"{"animation":"MethodAnim","track_node":"Player","track_property":"explode","track_type":"method","time":0.5,"value":{"type":"Array","value":[]}}"#,
    );
    assert!(resp.contains("200 OK"));
    let body = extract_body(&resp);
    let result: serde_json::Value = serde_json::from_str(body).unwrap();
    assert_eq!(result["track_type"], "method");

    let resp = http_get(port, "/api/animation?name=MethodAnim");
    let body = extract_body(&resp);
    let anim: serde_json::Value = serde_json::from_str(body).unwrap();
    let tracks = anim["tracks"].as_array().unwrap();
    assert_eq!(tracks.len(), 1);
    assert_eq!(tracks[0]["track_type"], "method");
    assert_eq!(tracks[0]["keyframes"].as_array().unwrap().len(), 1);

    handle.stop();
}

#[test]
fn add_keyframe_with_audio_track_type() {
    let (handle, port) = make_test_server();

    http_post(port, "/api/animation/create", r#"{"name":"AudioAnim","length":3.0}"#);

    // Add keyframe with track_type=audio
    let resp = http_post(
        port,
        "/api/animation/keyframe/add",
        r#"{"animation":"AudioAnim","track_node":"SFXPlayer","track_property":"sfx","track_type":"audio","time":1.0,"value":{"type":"String","value":"res://sounds/hit.wav"}}"#,
    );
    assert!(resp.contains("200 OK"));
    let body = extract_body(&resp);
    let result: serde_json::Value = serde_json::from_str(body).unwrap();
    assert_eq!(result["track_type"], "audio");

    let resp = http_get(port, "/api/animation?name=AudioAnim");
    let body = extract_body(&resp);
    let anim: serde_json::Value = serde_json::from_str(body).unwrap();
    let tracks = anim["tracks"].as_array().unwrap();
    assert_eq!(tracks.len(), 1);
    assert_eq!(tracks[0]["track_type"], "audio");

    handle.stop();
}

#[test]
fn add_keyframe_defaults_to_property_track() {
    let (handle, port) = make_test_server();

    http_post(port, "/api/animation/create", r#"{"name":"Default","length":1.0}"#);

    // No track_type specified — should default to property
    let resp = http_post(
        port,
        "/api/animation/keyframe/add",
        r#"{"animation":"Default","track_node":"Player","track_property":"position","time":0.0,"value":{"type":"Vector2","value":[0,0]}}"#,
    );
    assert!(resp.contains("200 OK"));

    let resp = http_get(port, "/api/animation?name=Default");
    let body = extract_body(&resp);
    let anim: serde_json::Value = serde_json::from_str(body).unwrap();
    let tracks = anim["tracks"].as_array().unwrap();
    assert_eq!(tracks[0]["track_type"], "property");

    handle.stop();
}

#[test]
fn mixed_track_types_in_one_animation() {
    let (handle, port) = make_test_server();

    http_post(port, "/api/animation/create", r#"{"name":"Full","length":3.0}"#);

    // Property track
    http_post(
        port,
        "/api/animation/keyframe/add",
        r#"{"animation":"Full","track_node":"Player","track_property":"position","track_type":"property","time":0.0,"value":{"type":"Vector2","value":[0,0]}}"#,
    );

    // Method track
    http_post(
        port,
        "/api/animation/keyframe/add",
        r#"{"animation":"Full","track_node":"Player","track_property":"jump","track_type":"method","time":1.0,"value":{"type":"Array","value":[]}}"#,
    );

    // Audio track
    http_post(
        port,
        "/api/animation/keyframe/add",
        r#"{"animation":"Full","track_node":"AudioPlayer","track_property":"bgm","track_type":"audio","time":0.0,"value":{"type":"String","value":"res://music/theme.ogg"}}"#,
    );

    let resp = http_get(port, "/api/animation?name=Full");
    let body = extract_body(&resp);
    let anim: serde_json::Value = serde_json::from_str(body).unwrap();
    let tracks = anim["tracks"].as_array().unwrap();
    assert_eq!(tracks.len(), 3, "Should have property, method, and audio tracks");

    let types: Vec<&str> = tracks.iter().map(|t| t["track_type"].as_str().unwrap()).collect();
    assert!(types.contains(&"property"));
    assert!(types.contains(&"method"));
    assert!(types.contains(&"audio"));

    handle.stop();
}

#[test]
fn track_add_to_nonexistent_animation_fails() {
    let (handle, port) = make_test_server();

    let resp = http_post(
        port,
        "/api/animation/track/add",
        r#"{"animation":"Ghost","node_path":"Player","property":"pos","track_type":"property"}"#,
    );
    assert!(resp.contains("404"), "Should fail for nonexistent animation");

    handle.stop();
}

#[test]
fn track_add_missing_fields_rejected() {
    let (handle, port) = make_test_server();

    http_post(port, "/api/animation/create", r#"{"name":"Fields","length":1.0}"#);

    // Missing node_path
    let resp = http_post(
        port,
        "/api/animation/track/add",
        r#"{"animation":"Fields","property":"pos"}"#,
    );
    assert!(resp.contains("400"), "Missing node_path should fail");

    // Missing property
    let resp = http_post(
        port,
        "/api/animation/track/add",
        r#"{"animation":"Fields","node_path":"Player"}"#,
    );
    assert!(resp.contains("400"), "Missing property should fail");

    handle.stop();
}
