//! pat-0q2kx.90: mouse capture + cursor shape reach the browser game view.
//!
//! Godot's web build turns `MOUSE_MODE_CAPTURED` into `canvas.requestPointerLock()`,
//! after which motion is reported as `movementX/movementY` with the position
//! frozen, and turns the cursor shape into a CSS `cursor` keyword
//! (upstream/godot/platform/web/display_server_web.cpp).
//!
//! Patina had every piece and no pipeline: the mouse mode was a stored enum,
//! `gdplatform::cursor::CursorManager` was dead code with no owner anywhere in
//! the engine, `mouse_mode` never appeared in any served payload, and the client
//! always posted absolute coordinates. So a script calling
//! `Input.set_mouse_mode(MOUSE_MODE_CAPTURED)` during Play changed nothing a
//! player could observe.
//!
//! These drive the real path end to end — a GDScript in a played scene, the live
//! HTTP handlers, and the served client's own wiring — so "the enum is stored
//! but nothing carries it" fails.

use gdeditor::editor_server::{EditorServerHandle, EditorState};
use gdeditor::editor_ui::EDITOR_HTML;
use gdscene::node::Node;
use gdscene::SceneTree;
use gdvariant::Variant;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;
use std::time::Duration;

// ---------------------------------------------------------------------------
// HTTP harness
// ---------------------------------------------------------------------------

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
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
    stream.write_all(request.as_bytes()).unwrap();
    let mut resp = Vec::new();
    let _ = stream.read_to_end(&mut resp);
    String::from_utf8_lossy(&resp).to_string()
}

fn http_get(port: u16, path: &str) -> String {
    http_request_str(
        port,
        &format!("GET {path} HTTP/1.1\r\nHost: localhost\r\n\r\n"),
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
    serde_json::from_str(extract_body(resp))
        .unwrap_or_else(|e| panic!("expected JSON body, got {resp:?}: {e}"))
}

fn input_state(port: u16) -> serde_json::Value {
    json_body(&http_get(port, "/api/runtime/input/state"))
}

/// Boots the real editor server on a scene whose `Main` node runs `source`.
///
/// The script is written into a temp project root and named by `_script_path`,
/// which is how `build_play_tree` finds and attaches it — the same route a
/// `.tscn`-loaded script takes, rather than a hand-attached instance that would
/// bypass the Play path entirely.
fn boot_with_script(source: &str) -> (EditorServerHandle, u16, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("tempdir");
    let script_rel = "main.gd";
    std::fs::write(dir.path().join(script_rel), source).expect("write script");

    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let mut main = Node::new("Main", "Node2D");
    main.set_property("_script_path", Variant::String(script_rel.to_string()));
    tree.add_child(root, main).unwrap();

    let mut state = EditorState::new(tree);
    // BOTH roots, deliberately. The Play path resolves `_script_path` through
    // `state.texture_cache.project_root()` (editor_server.rs, where
    // `build_play_tree` computes the root it hands to `attach_scripts_to_tree`),
    // NOT through `state.project_root` — that one serves the filesystem routes.
    // Setting only the latter leaves the texture cache's root empty, so the
    // script file is never found, `attach_scripts_to_tree` logs "Failed to read
    // script", no script is attached, and `_ready` never runs. Every assertion
    // below would then fail on an unset `seen_mode`/`mouse_mode` while looking
    // like a mouse-mode bug rather than a fixture bug.
    state.project_root = Some(dir.path().to_string_lossy().to_string());
    state
        .texture_cache
        .set_project_root(dir.path().to_string_lossy().to_string());

    let port = free_port();
    let handle = EditorServerHandle::start(port, state);
    thread::sleep(Duration::from_millis(100));
    (handle, port, dir)
}

/// Starts the runtime and takes manual control of the clock.
///
/// ⚠️ `/api/runtime/step` single-steps a **paused** runtime and 400s with
/// "not paused" otherwise; a running-but-unpaused runtime is advanced by a
/// background auto-driver instead. Pausing first is both what makes `step`
/// legal and what makes these tests deterministic rather than racing that
/// driver — the same play → pause → step sequence the Movie Maker tests use.
fn play_paused(port: u16) {
    assert!(
        http_post(port, "/api/runtime/play", "{}").contains("200 OK"),
        "the runtime must start"
    );
    http_post(port, "/api/runtime/pause", "");
}

/// Advances exactly one frame of the paused runtime.
fn step(port: u16) {
    assert!(
        http_post(port, "/api/runtime/step", "").contains("200 OK"),
        "stepping a paused runtime must succeed"
    );
}

// ---------------------------------------------------------------------------
// (1) The acceptance path, end to end over the live handlers.
// ---------------------------------------------------------------------------

#[test]
fn mouse_mode_pointer_lock_pipeline() {
    // `_ready` captures the mouse and asks for a pointing-hand cursor;
    // `_process` records what `Input` reports back, so the second half of the
    // acceptance ("a second script reads CAPTURED") is observable.
    let (_handle, port, _dir) = boot_with_script(
        "\
extends Node2D
var seen_mode = -1
var seen_shape = -1
var rel_x = 0.0
var rel_y = 0.0
func _ready():
    Input.set_mouse_mode(MOUSE_MODE_CAPTURED)
    Input.set_default_cursor_shape(CURSOR_POINTING_HAND)
func _process(delta):
    self.seen_mode = Input.get_mouse_mode()
    self.seen_shape = Input.get_current_cursor_shape()
func _input(event):
    self.rel_x = event.relative.x
    self.rel_y = event.relative.y
",
    );

    play_paused(port);
    // `_ready` runs as the play tree is built; this frame is what drains the
    // queued Input command onto the engine state.
    step(port);

    // -- the state payload reports CAPTURED (the client contract) -----------
    let st = input_state(port);
    assert_eq!(
        st["mouse_mode"].as_i64(),
        Some(2),
        "after a script sets MOUSE_MODE_CAPTURED the input/state response must \
         report mouse_mode 2 — this is what the client watches to call \
         requestPointerLock(); payload: {st}"
    );

    // -- and the cursor shape, as the CSS keyword the client applies ---------
    assert_eq!(
        st["cursor_shape"].as_i64(),
        Some(2),
        "set_default_cursor_shape(CURSOR_POINTING_HAND) must reach the payload; \
         payload: {st}"
    );
    assert_eq!(
        st["cursor_css"].as_str(),
        Some("pointer"),
        "CURSOR_POINTING_HAND is CSS `pointer` (display_server_web.cpp \
         godot2dom_cursor); payload: {st}"
    );

    // -- relative motion, position frozen ------------------------------------
    let before = input_state(port);
    let pos_before = before["mouse_position"].clone();

    assert!(
        http_post(port, "/api/runtime/input/mouse_move", r#"{"dx":7,"dy":-3}"#).contains("200 OK"),
        "the client must be able to post relative motion while captured"
    );
    step(port);

    let after = input_state(port);
    assert_eq!(
        after["mouse_position"], pos_before,
        "while captured the pointer position must not move — Godot freezes it \
         and reports only `relative`; before={pos_before} after={}",
        after["mouse_position"]
    );

    // -- the motion reached the script as MouseMotion.relative ---------------
    //
    // A script's variables are synced onto its node after each call, so the live
    // runtime node is where a script's own view of the frame is observable.
    let props = json_body(&http_get(port, "/api/runtime/node?path=/root/Main"));
    assert_eq!(
        find_script_var(&props, "rel_x"),
        Some(7.0),
        "the posted dx must reach `_input` as MouseMotion.relative.x; \
         runtime node: {props}"
    );
    assert_eq!(
        find_script_var(&props, "rel_y"),
        Some(-3.0),
        "the posted dy must reach `_input` as MouseMotion.relative.y"
    );

    // -- a script reading Input back sees CAPTURED ---------------------------
    assert_eq!(
        find_script_var(&props, "seen_mode"),
        Some(2.0),
        "Input.get_mouse_mode() must report CAPTURED to a script; runtime \
         node: {props}"
    );
    assert_eq!(
        find_script_var(&props, "seen_shape"),
        Some(2.0),
        "Input.get_current_cursor_shape() must report the shape a script set"
    );
}

/// Digs a script variable out of whatever shape `/api/runtime/state` returns.
///
/// Written as a search rather than a fixed path because this test is about the
/// input pipeline, not about the runtime-state schema — pinning the schema here
/// would make an unrelated payload change fail this bead's test.
fn find_script_var(v: &serde_json::Value, name: &str) -> Option<f64> {
    match v {
        serde_json::Value::Object(map) => {
            for (k, val) in map {
                if k == name {
                    if let Some(n) = val.as_f64() {
                        return Some(n);
                    }
                }
                if let Some(found) = find_script_var(val, name) {
                    return Some(found);
                }
            }
            None
        }
        serde_json::Value::Array(items) => items.iter().find_map(|i| find_script_var(i, name)),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// (2) Absolute motion still works when NOT captured.
// ---------------------------------------------------------------------------

/// The freeze must be conditional on capture. Without this, "always freeze"
/// would pass the acceptance test above while breaking every ordinary mouse
/// move in the editor's game view.
#[test]
fn uncaptured_motion_still_moves_the_pointer() {
    let (_handle, port, _dir) = boot_with_script("extends Node2D\n");
    play_paused(port);
    step(port);

    let st = input_state(port);
    assert_eq!(
        st["mouse_mode"].as_i64(),
        Some(0),
        "a scene that never touches the mouse mode stays VISIBLE: {st}"
    );
    assert_eq!(
        st["cursor_css"].as_str(),
        Some("default"),
        "the default cursor shape is CSS `default`: {st}"
    );

    assert!(
        http_post(port, "/api/runtime/input/mouse_move", r#"{"x":40,"y":25}"#).contains("200 OK")
    );
    let after = input_state(port);
    assert_eq!(
        after["mouse_position"][0].as_f64(),
        Some(40.0),
        "an uncaptured absolute move must move the pointer: {after}"
    );
    assert_eq!(after["mouse_position"][1].as_f64(), Some(25.0));
}

// ---------------------------------------------------------------------------
// (3) The client reports the REAL lock state back.
// ---------------------------------------------------------------------------

/// A `requestPointerLock()` can be refused or dropped by the browser without the
/// engine hearing about it. The client posts the actual state to
/// `/api/runtime/input/mouse_mode`; if the engine ignored that it would keep
/// freezing the pointer forever after a lock it no longer holds.
#[test]
fn client_can_report_the_actual_pointer_lock_state() {
    let (_handle, port, _dir) = boot_with_script(
        "\
extends Node2D
func _ready():
    Input.set_mouse_mode(MOUSE_MODE_CAPTURED)
",
    );
    play_paused(port);
    step(port);
    assert_eq!(input_state(port)["mouse_mode"].as_i64(), Some(2));

    // The browser dropped the lock (Esc); the client reports VISIBLE.
    assert!(
        http_post(port, "/api/runtime/input/mouse_mode", r#"{"mouse_mode":0}"#).contains("200 OK"),
        "the client must be able to report the real lock state back"
    );

    let st = input_state(port);
    assert_eq!(
        st["mouse_mode"].as_i64(),
        Some(0),
        "the engine must accept the client's correction: {st}"
    );

    // And motion moves the pointer again, rather than staying frozen.
    assert!(
        http_post(port, "/api/runtime/input/mouse_move", r#"{"x":12,"y":9}"#).contains("200 OK")
    );
    assert_eq!(
        input_state(port)["mouse_position"][0].as_f64(),
        Some(12.0),
        "after the lock is released the pointer must move again"
    );

    // A nonsense mode is rejected rather than silently coerced.
    assert!(
        http_post(
            port,
            "/api/runtime/input/mouse_mode",
            r#"{"mouse_mode":99}"#
        )
        .contains("400"),
        "an out-of-range mouse mode must be rejected"
    );
}

// ---------------------------------------------------------------------------
// (4) Static guard — the served client actually does the browser half.
// ---------------------------------------------------------------------------

/// The server half is useless if the served page never requests the lock. This
/// is the guard that "0 hits for requestPointerLock in editor_ui.rs" — the
/// original finding — cannot come back.
#[test]
fn served_client_wires_pointer_lock_and_css_cursor() {
    for needle in [
        "requestPointerLock",
        "exitPointerLock",
        "pointerLockElement",
        "pointerlockchange",
        // Relative motion while locked.
        "movementX",
        "movementY",
        // The engine-driven state the client applies.
        "applyMouseModeAndCursor",
        "mouse_mode",
        "cursor_css",
        // Reporting the real state back.
        "/api/runtime/input/mouse_mode",
    ] {
        assert!(
            EDITOR_HTML.contains(needle),
            "the served editor must wire `{needle}` for the pointer-lock / \
             cursor pipeline"
        );
    }
}
