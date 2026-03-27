//! Tests for animation track creation for property, method, and audio tracks (pat-n86px).
//!
//! Verifies that:
//! - Property, method, and audio tracks can be created via the API
//! - Track types are correctly stored and returned
//! - Duplicate tracks are rejected
//! - Tracks can be deleted
//! - Keyframes can be added to each track type

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
    player.set_property(
        "position",
        Variant::Vector2(Vector2::new(0.0, 0.0)),
    );
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
    let mut stream =
        TcpStream::connect(format!("127.0.0.1:{port}")).expect("failed to connect");
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
    stream.write_all(request.as_bytes()).unwrap();
    let mut response = Vec::new();
    let _ = stream.read_to_end(&mut response);
    String::from_utf8_lossy(&response).to_string()
}

fn extract_body(response: &str) -> &str {
    response.split("\r\n\r\n").nth(1).unwrap_or("")
}

fn response_ok(response: &str) -> bool {
    response.contains("200 OK")
}

#[test]
fn test_create_property_track() {
    let (_handle, port) = make_test_server();

    // Create animation first
    let resp = http_post(port, "/api/animation/create", r#"{"name":"walk","length":1.0}"#);
    assert!(response_ok(&resp), "failed to create animation: {resp}");

    // Add a property track
    let resp = http_post(
        port,
        "/api/animation/track/add",
        r#"{"animation":"walk","node_path":"Player","property":"position","track_type":"property"}"#,
    );
    assert!(response_ok(&resp), "failed to add property track: {resp}");
    let body = extract_body(&resp);
    assert!(body.contains(r#""ok":true"#), "expected ok: {body}");
    assert!(body.contains(r#""track_type":"property""#), "expected property type: {body}");
    assert!(body.contains(r#""track_index":0"#), "expected index 0: {body}");

    // Verify track appears in animation data
    let resp = http_get(port, "/api/animation?name=walk");
    let body = extract_body(&resp);
    assert!(body.contains(r#""track_type":"property""#), "track_type not in animation data: {body}");
    assert!(body.contains(r#""node_path":"Player""#), "node_path not in animation data: {body}");
    assert!(body.contains(r#""property":"position""#), "property not in animation data: {body}");
}

#[test]
fn test_create_method_track() {
    let (_handle, port) = make_test_server();

    let resp = http_post(port, "/api/animation/create", r#"{"name":"attack","length":0.5}"#);
    assert!(response_ok(&resp));

    let resp = http_post(
        port,
        "/api/animation/track/add",
        r#"{"animation":"attack","node_path":"Player","property":"jump","track_type":"method"}"#,
    );
    assert!(response_ok(&resp), "failed to add method track: {resp}");
    let body = extract_body(&resp);
    assert!(body.contains(r#""track_type":"method""#), "expected method type: {body}");

    // Verify in animation data
    let resp = http_get(port, "/api/animation?name=attack");
    let body = extract_body(&resp);
    assert!(body.contains(r#""track_type":"method""#), "method track not stored: {body}");
}

#[test]
fn test_create_audio_track() {
    let (_handle, port) = make_test_server();

    let resp = http_post(port, "/api/animation/create", r#"{"name":"sfx","length":2.0}"#);
    assert!(response_ok(&resp));

    let resp = http_post(
        port,
        "/api/animation/track/add",
        r#"{"animation":"sfx","node_path":"Player","property":"footstep","track_type":"audio"}"#,
    );
    assert!(response_ok(&resp), "failed to add audio track: {resp}");
    let body = extract_body(&resp);
    assert!(body.contains(r#""track_type":"audio""#), "expected audio type: {body}");

    // Verify in animation data
    let resp = http_get(port, "/api/animation?name=sfx");
    let body = extract_body(&resp);
    assert!(body.contains(r#""track_type":"audio""#), "audio track not stored: {body}");
}

#[test]
fn test_duplicate_track_rejected() {
    let (_handle, port) = make_test_server();

    let resp = http_post(port, "/api/animation/create", r#"{"name":"idle","length":1.0}"#);
    assert!(response_ok(&resp));

    // Add track once
    let resp = http_post(
        port,
        "/api/animation/track/add",
        r#"{"animation":"idle","node_path":"Player","property":"position","track_type":"property"}"#,
    );
    assert!(response_ok(&resp));

    // Try to add same track again — should fail
    let resp = http_post(
        port,
        "/api/animation/track/add",
        r#"{"animation":"idle","node_path":"Player","property":"position","track_type":"property"}"#,
    );
    assert!(resp.contains("400"), "duplicate track should be rejected: {resp}");
    assert!(extract_body(&resp).contains("already exists"), "should say already exists: {resp}");
}

#[test]
fn test_same_node_different_track_types() {
    let (_handle, port) = make_test_server();

    let resp = http_post(port, "/api/animation/create", r#"{"name":"multi","length":1.0}"#);
    assert!(response_ok(&resp));

    // Property track
    let resp = http_post(
        port,
        "/api/animation/track/add",
        r#"{"animation":"multi","node_path":"Player","property":"position","track_type":"property"}"#,
    );
    assert!(response_ok(&resp));

    // Method track with same node — different type, should succeed
    let resp = http_post(
        port,
        "/api/animation/track/add",
        r#"{"animation":"multi","node_path":"Player","property":"position","track_type":"method"}"#,
    );
    assert!(response_ok(&resp), "different track type should be allowed: {resp}");

    // Audio track with same node — should also succeed
    let resp = http_post(
        port,
        "/api/animation/track/add",
        r#"{"animation":"multi","node_path":"Player","property":"position","track_type":"audio"}"#,
    );
    assert!(response_ok(&resp), "audio track should be allowed: {resp}");

    // Verify all three tracks present
    let resp = http_get(port, "/api/animation?name=multi");
    let body = extract_body(&resp);
    assert!(body.contains(r#""track_type":"property""#));
    assert!(body.contains(r#""track_type":"method""#));
    assert!(body.contains(r#""track_type":"audio""#));
}

#[test]
fn test_delete_track() {
    let (_handle, port) = make_test_server();

    let resp = http_post(port, "/api/animation/create", r#"{"name":"del","length":1.0}"#);
    assert!(response_ok(&resp));

    // Add two tracks
    http_post(
        port,
        "/api/animation/track/add",
        r#"{"animation":"del","node_path":"Player","property":"position","track_type":"property"}"#,
    );
    http_post(
        port,
        "/api/animation/track/add",
        r#"{"animation":"del","node_path":"Player","property":"rotation","track_type":"property"}"#,
    );

    // Verify 2 tracks
    let resp = http_get(port, "/api/animation?name=del");
    let body = extract_body(&resp);
    assert!(body.contains(r#""property":"position""#));
    assert!(body.contains(r#""property":"rotation""#));

    // Delete first track
    let resp = http_post(
        port,
        "/api/animation/track/delete",
        r#"{"animation":"del","track_index":0}"#,
    );
    assert!(response_ok(&resp), "failed to delete track: {resp}");

    // Verify only rotation track remains
    let resp = http_get(port, "/api/animation?name=del");
    let body = extract_body(&resp);
    assert!(!body.contains(r#""property":"position""#), "position track should be gone");
    assert!(body.contains(r#""property":"rotation""#), "rotation track should remain");
}

#[test]
fn test_delete_track_invalid_index() {
    let (_handle, port) = make_test_server();

    let resp = http_post(port, "/api/animation/create", r#"{"name":"err","length":1.0}"#);
    assert!(response_ok(&resp));

    // Delete with out-of-range index
    let resp = http_post(
        port,
        "/api/animation/track/delete",
        r#"{"animation":"err","track_index":99}"#,
    );
    assert!(resp.contains("400"), "out-of-range index should fail: {resp}");
}

#[test]
fn test_delete_track_missing_animation() {
    let (_handle, port) = make_test_server();

    let resp = http_post(
        port,
        "/api/animation/track/delete",
        r#"{"animation":"nonexistent","track_index":0}"#,
    );
    assert!(resp.contains("404"), "missing animation should be 404: {resp}");
}

#[test]
fn test_add_keyframe_to_method_track() {
    let (_handle, port) = make_test_server();

    let resp = http_post(port, "/api/animation/create", r#"{"name":"meth","length":1.0}"#);
    assert!(response_ok(&resp));

    // Create method track
    let resp = http_post(
        port,
        "/api/animation/track/add",
        r#"{"animation":"meth","node_path":"Player","property":"jump","track_type":"method"}"#,
    );
    assert!(response_ok(&resp));

    // Add keyframe to method track
    let resp = http_post(
        port,
        "/api/animation/keyframe/add",
        r#"{"animation":"meth","track_node":"Player","track_property":"jump","time":0.5,"value":{"type":"Array","value":[{"type":"String","value":"jump"},{"type":"Float","value":100}]}}"#,
    );
    assert!(response_ok(&resp), "failed to add keyframe to method track: {resp}");

    // Verify keyframe in animation data
    let resp = http_get(port, "/api/animation?name=meth");
    let body = extract_body(&resp);
    assert!(body.contains("0.5"), "keyframe time should be in data: {body}");
}

#[test]
fn test_add_keyframe_to_audio_track() {
    let (_handle, port) = make_test_server();

    let resp = http_post(port, "/api/animation/create", r#"{"name":"aud","length":2.0}"#);
    assert!(response_ok(&resp));

    // Create audio track
    let resp = http_post(
        port,
        "/api/animation/track/add",
        r#"{"animation":"aud","node_path":"Player","property":"sfx","track_type":"audio"}"#,
    );
    assert!(response_ok(&resp));

    // Add keyframe with audio dict value
    let resp = http_post(
        port,
        "/api/animation/keyframe/add",
        r#"{"animation":"aud","track_node":"Player","track_property":"sfx","time":0.0,"value":{"type":"String","value":"res://sounds/step.wav"}}"#,
    );
    assert!(response_ok(&resp), "failed to add keyframe to audio track: {resp}");
}

#[test]
fn test_default_track_type_is_property() {
    let (_handle, port) = make_test_server();

    let resp = http_post(port, "/api/animation/create", r#"{"name":"def","length":1.0}"#);
    assert!(response_ok(&resp));

    // Omit track_type — should default to property
    let resp = http_post(
        port,
        "/api/animation/track/add",
        r#"{"animation":"def","node_path":"Player","property":"scale"}"#,
    );
    assert!(response_ok(&resp), "default track type should work: {resp}");
    let body = extract_body(&resp);
    assert!(body.contains(r#""track_type":"property""#), "default should be property: {body}");
}

#[test]
fn test_invalid_track_type_rejected() {
    let (_handle, port) = make_test_server();

    let resp = http_post(port, "/api/animation/create", r#"{"name":"inv","length":1.0}"#);
    assert!(response_ok(&resp));

    let resp = http_post(
        port,
        "/api/animation/track/add",
        r#"{"animation":"inv","node_path":"Player","property":"x","track_type":"bezier"}"#,
    );
    assert!(resp.contains("400"), "invalid track type should fail: {resp}");
}
