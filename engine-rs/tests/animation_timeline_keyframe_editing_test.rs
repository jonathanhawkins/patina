//! Tests for animation editor timeline with keyframe editing (pat-8zg44).
//!
//! Verifies the animation CRUD lifecycle, keyframe manipulation, playback
//! control, and seek/scrub operations via the editor HTTP API.

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

fn extract_body(resp: &str) -> &str {
    resp.split("\r\n\r\n").nth(1).unwrap_or("")
}

// --- Animation CRUD Tests ---

#[test]
fn create_animation_and_list() {
    let (handle, port) = make_test_server();

    // Initially no animations
    let resp = http_get(port, "/api/animations");
    let body = extract_body(&resp);
    let list: Vec<serde_json::Value> = serde_json::from_str(body).unwrap();
    assert!(list.is_empty(), "Should start with no animations");

    // Create an animation
    let resp = http_post(
        port,
        "/api/animation/create",
        r#"{"name":"Walk","length":1.5,"loop_mode":"loop"}"#,
    );
    assert!(resp.contains("200 OK"), "Create should succeed");

    // List should now contain it
    let resp = http_get(port, "/api/animations");
    let body = extract_body(&resp);
    let list: Vec<serde_json::Value> = serde_json::from_str(body).unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0]["name"], "Walk");
    assert_eq!(list[0]["length"], 1.5);
    assert_eq!(list[0]["loop_mode"], "loop");

    handle.stop();
}

#[test]
fn delete_animation() {
    let (handle, port) = make_test_server();

    http_post(
        port,
        "/api/animation/create",
        r#"{"name":"Idle","length":2.0}"#,
    );

    let resp = http_post(port, "/api/animation/delete", r#"{"name":"Idle"}"#);
    assert!(resp.contains("200 OK"), "Delete should succeed");

    let resp = http_get(port, "/api/animations");
    let body = extract_body(&resp);
    let list: Vec<serde_json::Value> = serde_json::from_str(body).unwrap();
    assert!(list.is_empty(), "Animation should be deleted");

    handle.stop();
}

#[test]
fn get_animation_details() {
    let (handle, port) = make_test_server();

    http_post(
        port,
        "/api/animation/create",
        r#"{"name":"Run","length":0.8,"loop_mode":"pingpong"}"#,
    );

    let resp = http_get(port, "/api/animation?name=Run");
    assert!(resp.contains("200 OK"));
    let body = extract_body(&resp);
    let anim: serde_json::Value = serde_json::from_str(body).unwrap();
    assert_eq!(anim["name"], "Run");
    assert_eq!(anim["length"], 0.8);
    assert_eq!(anim["loop_mode"], "pingpong");
    assert!(anim["tracks"].is_array());

    handle.stop();
}

// --- Keyframe Editing Tests ---

#[test]
fn add_keyframe_creates_track_and_keyframe() {
    let (handle, port) = make_test_server();

    http_post(
        port,
        "/api/animation/create",
        r#"{"name":"Move","length":2.0}"#,
    );

    // Add a keyframe — this auto-creates the track
    let resp = http_post(
        port,
        "/api/animation/keyframe/add",
        r#"{"animation":"Move","track_node":"Player","track_property":"position","time":0.0,"value":{"type":"Vector2","value":[0,0]}}"#,
    );
    assert!(resp.contains("200 OK"), "Add keyframe should succeed");

    // Add another keyframe on same track
    let resp = http_post(
        port,
        "/api/animation/keyframe/add",
        r#"{"animation":"Move","track_node":"Player","track_property":"position","time":1.0,"value":{"type":"Vector2","value":[100,50]}}"#,
    );
    assert!(resp.contains("200 OK"));

    // Verify tracks and keyframes
    let resp = http_get(port, "/api/animation?name=Move");
    let body = extract_body(&resp);
    let anim: serde_json::Value = serde_json::from_str(body).unwrap();
    let tracks = anim["tracks"].as_array().unwrap();
    assert_eq!(tracks.len(), 1, "Should have one track");
    assert_eq!(tracks[0]["node_path"], "Player");
    assert_eq!(tracks[0]["property"], "position");
    let kfs = tracks[0]["keyframes"].as_array().unwrap();
    assert_eq!(kfs.len(), 2, "Should have two keyframes");
    assert_eq!(kfs[0]["time"], 0.0);
    assert_eq!(kfs[1]["time"], 1.0);

    handle.stop();
}

#[test]
fn remove_keyframe() {
    let (handle, port) = make_test_server();

    http_post(
        port,
        "/api/animation/create",
        r#"{"name":"Fade","length":1.0}"#,
    );
    http_post(
        port,
        "/api/animation/keyframe/add",
        r#"{"animation":"Fade","track_node":"Player","track_property":"modulate","time":0.0,"value":{"type":"Float","value":1.0}}"#,
    );
    http_post(
        port,
        "/api/animation/keyframe/add",
        r#"{"animation":"Fade","track_node":"Player","track_property":"modulate","time":0.5,"value":{"type":"Float","value":0.5}}"#,
    );

    // Remove first keyframe (index 0)
    let resp = http_post(
        port,
        "/api/animation/keyframe/remove",
        r#"{"animation":"Fade","track_index":0,"keyframe_index":0}"#,
    );
    assert!(resp.contains("200 OK"), "Remove keyframe should succeed");

    // Verify only one keyframe remains
    let resp = http_get(port, "/api/animation?name=Fade");
    let body = extract_body(&resp);
    let anim: serde_json::Value = serde_json::from_str(body).unwrap();
    let tracks = anim["tracks"].as_array().unwrap();
    assert_eq!(tracks.len(), 1);
    let kfs = tracks[0]["keyframes"].as_array().unwrap();
    assert_eq!(kfs.len(), 1, "Should have one keyframe after removal");
    assert_eq!(kfs[0]["time"], 0.5);

    handle.stop();
}

#[test]
fn multiple_tracks_on_same_animation() {
    let (handle, port) = make_test_server();

    http_post(
        port,
        "/api/animation/create",
        r#"{"name":"Complex","length":2.0}"#,
    );

    // Position track
    http_post(
        port,
        "/api/animation/keyframe/add",
        r#"{"animation":"Complex","track_node":"Player","track_property":"position","time":0.0,"value":{"type":"Vector2","value":[0,0]}}"#,
    );

    // Rotation track
    http_post(
        port,
        "/api/animation/keyframe/add",
        r#"{"animation":"Complex","track_node":"Player","track_property":"rotation","time":0.0,"value":{"type":"Float","value":0.0}}"#,
    );

    // Scale track
    http_post(
        port,
        "/api/animation/keyframe/add",
        r#"{"animation":"Complex","track_node":"Player","track_property":"scale","time":0.5,"value":{"type":"Vector2","value":[1,1]}}"#,
    );

    let resp = http_get(port, "/api/animation?name=Complex");
    let body = extract_body(&resp);
    let anim: serde_json::Value = serde_json::from_str(body).unwrap();
    let tracks = anim["tracks"].as_array().unwrap();
    assert_eq!(tracks.len(), 3, "Should have three separate tracks");

    handle.stop();
}

// --- Playback Control Tests ---

#[test]
fn play_and_stop_animation() {
    let (handle, port) = make_test_server();

    http_post(
        port,
        "/api/animation/create",
        r#"{"name":"Bounce","length":1.0}"#,
    );

    // Play
    let resp = http_post(
        port,
        "/api/animation/play",
        r#"{"name":"Bounce"}"#,
    );
    assert!(resp.contains("200 OK"), "Play should succeed");

    // Check status
    let resp = http_get(port, "/api/animation/status");
    let body = extract_body(&resp);
    let status: serde_json::Value = serde_json::from_str(body).unwrap();
    assert_eq!(status["playing"], true);
    assert_eq!(status["animation_name"], "Bounce");

    // Stop
    let resp = http_post(port, "/api/animation/stop", "");
    assert!(resp.contains("200 OK"), "Stop should succeed");

    // Check stopped
    let resp = http_get(port, "/api/animation/status");
    let body = extract_body(&resp);
    let status: serde_json::Value = serde_json::from_str(body).unwrap();
    assert_eq!(status["playing"], false);

    handle.stop();
}

#[test]
fn seek_animation() {
    let (handle, port) = make_test_server();

    http_post(
        port,
        "/api/animation/create",
        r#"{"name":"Slide","length":2.0}"#,
    );
    http_post(
        port,
        "/api/animation/keyframe/add",
        r#"{"animation":"Slide","track_node":"Player","track_property":"position","time":0.0,"value":{"type":"Vector2","value":[0,0]}}"#,
    );
    http_post(
        port,
        "/api/animation/keyframe/add",
        r#"{"animation":"Slide","track_node":"Player","track_property":"position","time":2.0,"value":{"type":"Vector2","value":[200,0]}}"#,
    );

    // Seek to midpoint
    let resp = http_post(
        port,
        "/api/animation/seek",
        r#"{"time":1.0}"#,
    );
    assert!(resp.contains("200 OK"), "Seek should succeed");

    handle.stop();
}

#[test]
fn recording_mode_toggle() {
    let (handle, port) = make_test_server();

    // Toggle recording on
    let resp = http_post(port, "/api/animation/record", r#"{"enabled":true}"#);
    assert!(resp.contains("200 OK"));

    let resp = http_get(port, "/api/animation/status");
    let body = extract_body(&resp);
    let status: serde_json::Value = serde_json::from_str(body).unwrap();
    assert_eq!(status["recording"], true);

    // Toggle recording off
    let resp = http_post(port, "/api/animation/record", r#"{"enabled":false}"#);
    assert!(resp.contains("200 OK"));

    let resp = http_get(port, "/api/animation/status");
    let body = extract_body(&resp);
    let status: serde_json::Value = serde_json::from_str(body).unwrap();
    assert_eq!(status["recording"], false);

    handle.stop();
}

// --- Error Handling Tests ---

#[test]
fn get_nonexistent_animation_returns_404() {
    let (handle, port) = make_test_server();

    let resp = http_get(port, "/api/animation?name=DoesNotExist");
    assert!(
        resp.contains("404") || resp.contains("not found"),
        "Should return 404 for missing animation"
    );

    handle.stop();
}

#[test]
fn add_keyframe_to_nonexistent_animation_fails() {
    let (handle, port) = make_test_server();

    let resp = http_post(
        port,
        "/api/animation/keyframe/add",
        r#"{"animation":"NoSuch","track_node":"Player","track_property":"position","time":0.0,"value":{"type":"Float","value":0}}"#,
    );
    assert!(
        resp.contains("404") || resp.contains("not found"),
        "Should fail for nonexistent animation"
    );

    handle.stop();
}

#[test]
fn editor_html_contains_animation_panel() {
    let (handle, port) = make_test_server();

    let resp = http_get(port, "/editor");
    assert!(resp.contains("200 OK"));
    assert!(
        resp.contains("animation-panel"),
        "Editor HTML should contain animation panel"
    );
    assert!(
        resp.contains("anim-timeline-canvas"),
        "Should contain timeline canvas"
    );
    assert!(
        resp.contains("anim-new-btn"),
        "Should contain new animation button"
    );
    assert!(
        resp.contains("anim-play-btn"),
        "Should contain play button"
    );
    assert!(
        resp.contains("renderTimeline"),
        "Should contain timeline rendering code"
    );
    assert!(
        resp.contains("loadAnimation"),
        "Should contain animation loading code"
    );
    assert!(
        resp.contains("renderAnimTracks"),
        "Should contain track rendering code"
    );

    handle.stop();
}
