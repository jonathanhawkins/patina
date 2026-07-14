//! Game-view runtime toolbar drives the running game (pat-j9yqh).
//!
//! Godot's GameView toolbar acts on the embedded running game
//! (game_view_plugin.cpp): Suspend pauses the SceneTree, Next Frame advances a
//! frame, the speed control sets/resets the time scale, Input/2D/3D switch the
//! interaction mode, Hide Selection toggles selection visibility, Camera
//! Override drives the in-game camera, and Mute silences audio. This drives the
//! assembled server's runtime control endpoints and asserts the status
//! reflects each change.

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

fn status(port: u16) -> serde_json::Value {
    body_json(&http_get(port, "/api/runtime/status"))
}

#[test]
fn game_view_toolbar_controls_running_game() {
    let (handle, port) = make_server();

    // Defaults.
    let s0 = status(port);
    assert_eq!(s0["suspended"], serde_json::json!(false));
    assert_eq!(s0["time_scale"], serde_json::json!(1.0));
    assert_eq!(s0["input_mode"], serde_json::json!("input"));

    // Start the game so the runtime is live.
    assert!(http_post(port, "/api/runtime/play", "{}").contains("200 OK"));

    // Suspend pauses the SceneTree.
    assert_eq!(body_json(&http_post(port, "/api/runtime/suspend", r#"{"suspended":true}"#))["suspended"], serde_json::json!(true));
    let ss = status(port);
    assert_eq!(ss["suspended"], serde_json::json!(true));
    assert_eq!(ss["paused"], serde_json::json!(true), "suspend pauses the SceneTree");

    // Speed control changes / resets the time scale.
    assert_eq!(body_json(&http_post(port, "/api/runtime/time_scale", r#"{"scale":2.0}"#))["time_scale"], serde_json::json!(2.0));
    assert_eq!(status(port)["time_scale"], serde_json::json!(2.0));
    assert_eq!(body_json(&http_post(port, "/api/runtime/time_scale", "{}"))["time_scale"], serde_json::json!(1.0), "reset returns to 1.0");

    // Input / 2D / 3D switch the interaction mode.
    assert_eq!(body_json(&http_post(port, "/api/runtime/input_mode", r#"{"mode":"2d"}"#))["input_mode"], serde_json::json!("2d"));
    assert_eq!(status(port)["input_mode"], serde_json::json!("2d"));

    // Hide Selection, Camera Override, Mute act on the session.
    assert_eq!(body_json(&http_post(port, "/api/runtime/hide_selection", r#"{"hidden":true}"#))["hide_selection"], serde_json::json!(true));
    assert_eq!(body_json(&http_post(port, "/api/runtime/camera_override", r#"{"enabled":true}"#))["camera_override"], serde_json::json!(true));
    assert_eq!(body_json(&http_post(port, "/api/runtime/mute", r#"{"muted":true}"#))["muted"], serde_json::json!(true));
    let sf = status(port);
    assert_eq!(sf["hide_selection"], serde_json::json!(true));
    assert_eq!(sf["camera_override"], serde_json::json!(true));
    assert_eq!(sf["muted"], serde_json::json!(true));

    // Next Frame advances a frame (the step endpoint accepts the request).
    assert!(http_post(port, "/api/runtime/step", "{}").contains("200 OK"));

    // The served toolbar wires the buttons to the runtime endpoints.
    let html = http_get(port, "/editor");
    for needle in [
        "id=\"game-suspend\"", "id=\"game-speed\"", "id=\"game-reset-speed\"",
        "id=\"game-mute\"", "id=\"game-camera-override\"", "id=\"game-hide-selection\"",
        "/api/runtime/suspend", "/api/runtime/time_scale", "/api/runtime/mute",
        "/api/runtime/input_mode", "/api/runtime/camera_override", "/api/runtime/hide_selection",
    ] {
        assert!(html.contains(needle), "game toolbar missing `{needle}`");
    }

    handle.stop();
}
