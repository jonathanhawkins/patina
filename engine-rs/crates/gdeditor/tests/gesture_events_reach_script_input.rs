//! pat-s3rrv.234 acceptance: a browser pinch/pan reaches a running game.
//!
//! # The gap this closes
//!
//! Almost everything was already here and the bead's own survey is out of date:
//! `InputEvent::MagnifyGesture` / `PanGesture` exist in `gdplatform`, all three
//! classes are registered in ClassDB, and `scene_tree.rs` already builds the
//! script-facing `InputEventMagnifyGesture` / `InputEventPanGesture` objects.
//!
//! **What was missing was the producer.** There was no
//! `/api/runtime/input/gesture` route and no listener in the served client, so a
//! pinch on the game view originated nothing — a complete delivery path with
//! nothing at the top of it.
//!
//! ⚠️ **Why the client half matters as much as the route.** Outside Safari there
//! is no pinch event at all: browsers report a trackpad pinch as a `wheel` with
//! `ctrlKey` set. A route with no listener is as inert as a listener with no
//! route, so this file asserts the served HTML carries both the synthesis and
//! the Safari `gesturechange` path.

use gdeditor::editor_server::{EditorServerHandle, EditorState};
use gdscene::node::Node;
use gdscene::SceneTree;
use gdvariant::Variant;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;
use std::time::Duration;

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn connect_with_retry(port: u16) -> TcpStream {
    for attempt in 0..40 {
        match TcpStream::connect(format!("127.0.0.1:{port}")) {
            Ok(s) => return s,
            Err(_) if attempt < 39 => thread::sleep(Duration::from_millis(50)),
            Err(e) => panic!("failed to connect: {e}"),
        }
    }
    unreachable!()
}

fn http_request_str(port: u16, request: &str) -> String {
    let mut stream = connect_with_retry(port);
    stream.write_all(request.as_bytes()).unwrap();
    let mut resp = String::new();
    let _ = stream.read_to_string(&mut resp);
    resp
}

fn http_get(port: u16, path: &str) -> String {
    http_request_str(
        port,
        &format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n"),
    )
}

fn http_post(port: u16, path: &str, body: &str) -> String {
    http_request_str(
        port,
        &format!(
            "POST {path} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        ),
    )
}

fn json_body(resp: &str) -> serde_json::Value {
    serde_json::from_str(resp.split("\r\n\r\n").nth(1).unwrap_or(""))
        .unwrap_or(serde_json::Value::Null)
}

/// ⚠️ BOTH project roots are required and setting only one is silent: the PLAY
/// path resolves `_script_path` through `texture_cache.project_root()`, and
/// without it no script attaches — which surfaces as every script variable
/// being absent rather than as "the script never loaded".
fn boot() -> (EditorServerHandle, u16, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("tempdir");
    // Each field is recorded from a DIFFERENT part of the event, so a builder
    // that produced the right class with empty contents still fails. The hit
    // counters make "arrived once" separable from "arrived at all".
    std::fs::write(
        dir.path().join("main.gd"),
        "extends Node2D\n\
         var magnify_hits = 0\n\
         var magnify_factor = 0.0\n\
         var magnify_x = 0.0\n\
         var magnify_y = 0.0\n\
         var pan_hits = 0\n\
         var pan_dx = 0.0\n\
         var pan_dy = 0.0\n\
         func _input(event):\n\
         \x20   if event is InputEventMagnifyGesture:\n\
         \x20       self.magnify_hits = self.magnify_hits + 1\n\
         \x20       self.magnify_factor = event.factor\n\
         \x20       self.magnify_x = event.position.x\n\
         \x20       self.magnify_y = event.position.y\n\
         \x20   if event is InputEventPanGesture:\n\
         \x20       self.pan_hits = self.pan_hits + 1\n\
         \x20       self.pan_dx = event.delta.x\n\
         \x20       self.pan_dy = event.delta.y\n",
    )
    .expect("write main.gd");

    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let mut main = Node::new("Main", "Node2D");
    main.set_property("_script_path", Variant::String("main.gd".to_string()));
    tree.add_child(root, main).unwrap();

    let mut state = EditorState::new(tree);
    let root_path = dir.path().to_string_lossy().to_string();
    state.project_root = Some(root_path.clone());
    state.texture_cache.set_project_root(root_path);

    let port = free_port();
    let handle = EditorServerHandle::start(port, state);
    thread::sleep(Duration::from_millis(100));
    (handle, port, dir)
}

/// ⚠️ `/api/runtime/step` single-steps a **paused** runtime and 400s otherwise.
/// Pausing is what makes stepping legal and these assertions deterministic.
fn play_paused(port: u16) {
    assert!(
        http_post(port, "/api/runtime/play", "{}").contains("200 OK"),
        "the runtime must start"
    );
    http_post(port, "/api/runtime/pause", "");
}

/// A script variable off `/api/runtime/node`.
///
/// ⚠️ The payload nests the value twice — `properties[] -> value -> value` —
/// because each entry carries the variant's type beside its contents. Reading
/// one level yields the wrapper object, which compares unequal to every number
/// and makes a passing assertion impossible to write by accident.
fn script_var(port: u16, path: &str, name: &str) -> Option<f64> {
    let node = json_body(&http_get(port, &format!("/api/runtime/node?path={path}")));
    node.get("properties")?
        .as_array()?
        .iter()
        .find(|p| p.get("name").and_then(|n| n.as_str()) == Some(name))?
        .get("value")?
        .get("value")?
        .as_f64()
}

// ---------------------------------------------------------------------------
// The bead's acceptance test
// ---------------------------------------------------------------------------

/// **The acceptance case.** A posted magnify arrives at `_input` as an
/// `InputEventMagnifyGesture` carrying the posted factor and position; a posted
/// pan arrives as an `InputEventPanGesture` carrying the posted delta.
#[test]
fn gesture_events_reach_script_input() {
    let (handle, port, _dir) = boot();
    play_paused(port);

    // -- magnify ------------------------------------------------------------
    let resp = http_post(
        port,
        "/api/runtime/input/gesture",
        r#"{"kind":"magnify","x":30,"y":40,"factor":1.25}"#,
    );
    assert!(
        resp.contains("200 OK"),
        "POST /api/runtime/input/gesture must exist and accept a magnify:\n{resp}"
    );
    assert!(
        resp.contains(r#""events":1"#),
        "one gesture must produce one event:\n{resp}"
    );
    http_post(port, "/api/runtime/step", "");

    assert_eq!(
        script_var(port, "/root/Main", "magnify_hits"),
        Some(1.0),
        "_input must observe exactly one InputEventMagnifyGesture — 0 means the \
         event never reached the script, and the `is` test is what proves the \
         class, not merely that something arrived"
    );
    // The posted value, not just a non-zero one. `factor` defaults to 1.0, so
    // asserting "not zero" would pass on an event that dropped the payload.
    assert_eq!(
        script_var(port, "/root/Main", "magnify_factor"),
        Some(1.25),
        "the posted factor must survive to the script"
    );
    // Position is asserted on BOTH axes: 30 and 40 are distinct, so a builder
    // that swapped them, or wrote one into both, fails here.
    assert_eq!(
        script_var(port, "/root/Main", "magnify_x"),
        Some(30.0),
        "position.x must be the posted x"
    );
    assert_eq!(
        script_var(port, "/root/Main", "magnify_y"),
        Some(40.0),
        "position.y must be the posted y"
    );
    // The control for the assertions above: a magnify must not also register as
    // a pan. Without this, a builder that produced the same object for both
    // kinds would satisfy every assertion in this test.
    assert_eq!(
        script_var(port, "/root/Main", "pan_hits"),
        Some(0.0),
        "a magnify must not arrive as a pan as well"
    );

    // -- pan ----------------------------------------------------------------
    let resp = http_post(
        port,
        "/api/runtime/input/gesture",
        r#"{"kind":"pan","x":10,"y":12,"dx":-4,"dy":7}"#,
    );
    assert!(
        resp.contains("200 OK"),
        "POST /api/runtime/input/gesture must accept a pan:\n{resp}"
    );
    http_post(port, "/api/runtime/step", "");

    assert_eq!(
        script_var(port, "/root/Main", "pan_hits"),
        Some(1.0),
        "_input must observe exactly one InputEventPanGesture"
    );
    // Negative on one axis and positive on the other, so a sign flip or an
    // abs() anywhere in the path is visible.
    assert_eq!(
        script_var(port, "/root/Main", "pan_dx"),
        Some(-4.0),
        "the posted delta.x must survive, sign included"
    );
    assert_eq!(
        script_var(port, "/root/Main", "pan_dy"),
        Some(7.0),
        "the posted delta.y must survive"
    );
    // And the magnify counter must not have moved again.
    assert_eq!(
        script_var(port, "/root/Main", "magnify_hits"),
        Some(1.0),
        "a pan must not arrive as a magnify as well"
    );

    drop(handle);
}

/// A malformed frame is refused rather than silently pushed as a zero gesture.
///
/// A `factor` of 0 is not a no-op — it is "scale to nothing" — so the route
/// must not invent one from a body it could not read.
#[test]
fn gesture_events_reach_script_input_rejects_an_unknown_kind() {
    let (handle, port, _dir) = boot();
    play_paused(port);

    let resp = http_post(
        port,
        "/api/runtime/input/gesture",
        r#"{"kind":"twist","x":1,"y":2}"#,
    );
    assert!(
        !resp.contains("200 OK"),
        "an unknown gesture kind must be refused:\n{resp}"
    );

    http_post(port, "/api/runtime/step", "");
    assert_eq!(
        script_var(port, "/root/Main", "magnify_hits"),
        Some(0.0),
        "a refused frame must not reach the script"
    );

    drop(handle);
}

/// The client half. A route with no listener is as inert as a listener with no
/// route, and outside Safari a pinch IS a `wheel` with `ctrlKey` — so the
/// synthesis in the served page is part of the acceptance, not decoration.
#[test]
fn gesture_events_reach_script_input_client_synthesizes_from_wheel() {
    let html = gdeditor::editor_ui::EDITOR_HTML;

    assert!(
        html.contains("/api/runtime/input/gesture"),
        "the served client must post to the gesture route"
    );
    assert!(
        html.contains("e.ctrlKey"),
        "a trackpad pinch arrives as a wheel with ctrlKey; without that branch \
         no non-Safari browser can ever produce a magnify"
    );
    assert!(
        html.contains("'magnify'") && html.contains("'pan'"),
        "both gesture kinds must be reachable from the client"
    );
    assert!(
        html.contains("gesturechange"),
        "Safari does not set ctrlKey on a pinch, so it needs its own listener"
    );
}
