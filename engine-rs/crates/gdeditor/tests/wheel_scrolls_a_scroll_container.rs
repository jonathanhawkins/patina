//! pat-s3rrv.76 — a wheel notch scrolls a ScrollContainer in the running game.
//!
//! The sibling acceptance test proves a notch reaches `_input` with
//! `MOUSE_BUTTON_WHEEL_DOWN`. This proves the other half of the same clause:
//! that the notch actually moves a container.
//!
//! ⚠️ The container's `size` is set EXPLICITLY rather than authored with anchors
//! and offsets, and that is load-bearing. Nothing in the Play path resolves
//! Control layout — `advance_running_main_loop` only calls `MainLoop::step`, and
//! `step` never touches `resolve_control_layout`. Only the EDITOR's viewport
//! render reaches it (`viewport_2d_render` -> `control_draw_items` ->
//! `render_node`). An anchors-and-offsets container in a stepped runtime is
//! therefore 0x0, which makes it invisible to the hit test AND gives
//! `max_scroll` a zero limit — the wheel would appear "not wired" when the real
//! cause is layout. Setting `size` here keeps this test about the wheel.
//!
//! Content taller than the view comes from the child's `custom_minimum_size`,
//! which is what `scroll_content_size` reads.

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
    stream.read_to_string(&mut resp).unwrap();
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

fn extract_body(resp: &str) -> &str {
    resp.split("\r\n\r\n").nth(1).unwrap_or("")
}

fn json_body(resp: &str) -> serde_json::Value {
    serde_json::from_str(extract_body(resp)).unwrap_or(serde_json::Value::Null)
}

/// Boots the real editor server over a one-node scene carrying `source`.
///
/// ⚠️ BOTH roots are required and setting only the first is silent: the PLAY
/// path resolves `_script_path` against `texture_cache.project_root()`, and
/// without it no script attaches at all — surfacing far downstream as every
/// script variable being *absent* rather than "the script never loaded".
fn boot_with_scroll_container() -> (EditorServerHandle, u16, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("tempdir");
    let script_rel = "main.gd";
    std::fs::write(dir.path().join(script_rel), TICKER_GD).expect("write script");

    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let mut main = Node::new("Main", "Node2D");
    main.set_property("_script_path", Variant::String(script_rel.to_string()));
    let main_id = tree.add_child(root, main).unwrap();

    // The container, sized explicitly — see the module comment for why.
    let mut scroll = Node::new("Scroll", "ScrollContainer");
    scroll.set_property(
        "position",
        Variant::Vector2(gdcore::math::Vector2::new(0.0, 0.0)),
    );
    scroll.set_property(
        "size",
        Variant::Vector2(gdcore::math::Vector2::new(100.0, 100.0)),
    );
    let scroll_id = tree.add_child(main_id, scroll).unwrap();

    // Content taller than the view, so there is somewhere to scroll TO. Without
    // this `max_scroll` is zero and the notch is correctly a no-op, which would
    // read as "the wheel is not wired".
    let mut content = Node::new("Content", "Control");
    content.set_property(
        "custom_minimum_size",
        Variant::Vector2(gdcore::math::Vector2::new(100.0, 500.0)),
    );
    tree.add_child(scroll_id, content).unwrap();

    let mut state = EditorState::new(tree);
    let root_path = dir.path().to_string_lossy().to_string();
    state.project_root = Some(root_path.clone());
    state.texture_cache.set_project_root(root_path);

    let port = free_port();
    let handle = EditorServerHandle::start(port, state);
    thread::sleep(Duration::from_millis(100));
    (handle, port, dir)
}

fn play_paused(port: u16) {
    assert!(
        http_post(port, "/api/runtime/play", "{}").contains("200 OK"),
        "the runtime must start"
    );
    http_post(port, "/api/runtime/pause", "");
}

fn step(port: u16) {
    assert!(
        http_post(port, "/api/runtime/step", "").contains("200 OK"),
        "stepping a paused runtime must succeed"
    );
}

fn runtime_node(port: u16) -> serde_json::Value {
    json_body(&http_get(port, "/api/runtime/node?path=/root/Main"))
}

/// ⚠️ The variable name is a **value**, not a key, and `value` is doubled.
/// Walking the JSON for a key named `seen_ui_copy` matches nothing, ever.
fn property_value(v: &serde_json::Value, name: &str) -> Option<serde_json::Value> {
    v.get("properties")?
        .as_array()?
        .iter()
        .find(|p| p.get("name").and_then(|n| n.as_str()) == Some(name))?
        .get("value")?
        .get("value")
        .cloned()
}

fn script_int(port: u16, name: &str) -> i64 {
    let node = runtime_node(port);
    property_value(&node, name)
        .and_then(|v| v.as_i64())
        .unwrap_or_else(|| panic!("script var `{name}` missing from /api/runtime/node: {node}"))
}

/// Counts `ui_copy` presses. `ui_copy` is a **builtin** action (Ctrl+C) — the
/// scene declares no input map of its own, which is the point: Godot seeds the
/// builtins before any project action is read.

fn scroll_vertical(port: u16) -> f64 {
    let node = json_body(&http_get(port, "/api/runtime/node?path=/root/Main/Scroll"));
    property_value(&node, "scroll_vertical")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0)
}

const TICKER_GD: &str = "\
extends Node2D
var frames = 0
func _process(delta):
    self.frames = self.frames + 1
";

#[test]
fn a_wheel_notch_scrolls_a_scroll_container_in_the_running_game() {
    let (handle, port, _dir) = boot_with_scroll_container();
    play_paused(port);
    step(port);

    assert!(
        script_int(port, "frames") > 0,
        "the runtime must be running before the wheel means anything"
    );
    assert_eq!(
        scroll_vertical(port),
        0.0,
        "the container starts unscrolled, or the delta below proves nothing"
    );

    // The cursor must be INSIDE the container: the wheel is delivered to the
    // control under the pointer, and the pointer starts at (0, 0).
    assert!(
        http_post(
            port,
            "/api/runtime/input/mouse_move",
            r#"{"x":50.0,"y":50.0}"#,
        )
        .contains("200 OK"),
        "mouse_move must be accepted while running"
    );
    step(port);

    assert!(
        http_post(port, "/api/runtime/input/mouse_wheel", r#"{"delta_y":1.0}"#).contains("200 OK"),
        "the wheel endpoint must accept a notch"
    );
    step(port);

    let after = scroll_vertical(port);
    assert!(
        after > 0.0,
        "a wheel-down notch over the container must advance scroll_vertical, \
         got {after}. A 0 means the notch reached the game but nothing scrolled \
         — check that the container was HIT (its size is set explicitly here \
         because the Play path never resolves Control layout) and that \
         max_scroll is non-zero (the child's custom_minimum_size is what makes \
         the content taller than the view)"
    );

    drop(handle);
}
