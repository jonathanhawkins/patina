//! pat-s3rrv.15 acceptance: the browser's real modifier state reaches the game.
//!
//! Godot's web platform layer builds an `InputEventKey` from a KeyboardEvent,
//! carrying `shiftKey`/`ctrlKey`/`altKey`/`metaKey` into the
//! `InputEventWithModifiers` flags
//! (upstream/godot/platform/web/js/libs/library_godot_input.js,
//! display_server_web.cpp). That modifier state is what makes Ctrl+C, Ctrl+Z and
//! Shift+Tab work inside a running game.
//!
//! Patina's editor Play flow dropped it in **three** independent places, each of
//! which alone was enough to make every modifier-bound `ui_*` action dead:
//!
//! 1. the served client posted only `{key: e.key}`, so the modifiers never left
//!    the browser (`editor_ui.rs`);
//! 2. the server built the event with `shift: false, ctrl: false, alt: false`
//!    hard-coded (`editor_server.rs` `api_input_key_down`/`_up`);
//! 3. `build_engine_input_map` never called `install_builtin_ui_actions`, so
//!    `ui_copy` had no `KeyWithModifiers` binding to match against at all; and
//! 4. the editor set a manual `InputSnapshot` every frame, and
//!    `MainLoop::step` runs its engine→script input bridge only
//!    `if !has_input_snapshot()` — so scripts saw a string-keyed snapshot with
//!    no notion of modifiers, whatever the engine had resolved.
//!
//! ⚠️ This asserts through a **script in the running scene**, not through
//! `/api/runtime/input/state`. That endpoint answers from
//! `EditorState::is_action_pressed`, which walks the editor's own string map and
//! never reaches `KeyCombo::modifiers_satisfied_by` — it reports `ui_copy`
//! identically with and without Ctrl, so a test written against it passes on
//! completely broken plumbing.

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
    let root_path = dir.path().to_string_lossy().to_string();
    state.project_root = Some(root_path.clone());
    state.texture_cache.set_project_root(root_path);

    let port = free_port();
    let handle = EditorServerHandle::start(port, state);
    thread::sleep(Duration::from_millis(100));
    (handle, port, dir)
}

/// ⚠️ `/api/runtime/step` single-steps a **paused** runtime and 400s otherwise;
/// an unpaused runtime is advanced by a background driver instead. Pausing is
/// what makes `step` legal and these assertions deterministic.
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
const COUNTER_GD: &str = "\
extends Node2D
var seen_ui_copy = 0
var frames = 0
func _process(delta):
    self.frames = self.frames + 1
    if Input.is_action_just_pressed(\"ui_copy\"):
        self.seen_ui_copy = self.seen_ui_copy + 1
";

/// **The acceptance test.**
#[test]
fn browser_modifier_keys_reach_running_game() {
    let (handle, port, _dir) = boot_with_script(COUNTER_GD);
    play_paused(port);

    // Baseline: the script is attached and running, and has seen nothing yet.
    step(port);
    assert!(
        script_int(port, "frames") > 0,
        "the script must actually be running — 0 frames means it never attached"
    );
    assert_eq!(
        script_int(port, "seen_ui_copy"),
        0,
        "no key pressed yet, so ui_copy must not have fired"
    );

    // -- Ctrl+C: the modifier arrives and the builtin combo resolves ----------
    assert!(
        http_post(
            port,
            "/api/runtime/input/key_down",
            r#"{"key":"c","code":"KeyC","ctrl":true}"#,
        )
        .contains("200 OK"),
        "key_down accepted while running"
    );
    step(port);
    assert_eq!(
        script_int(port, "seen_ui_copy"),
        1,
        "Ctrl+C must resolve the builtin ui_copy action inside the running game"
    );

    // -- a repeat keydown is an echo: it must NOT re-fire just_pressed --------
    assert!(
        http_post(
            port,
            "/api/runtime/input/key_down",
            r#"{"key":"c","code":"KeyC","ctrl":true,"repeat":true}"#,
        )
        .contains("200 OK"),
        "repeat key_down accepted"
    );
    step(port);
    assert_eq!(
        script_int(port, "seen_ui_copy"),
        1,
        "a repeat (echo) keydown must not re-fire just_pressed"
    );

    http_post(
        port,
        "/api/runtime/input/key_up",
        r#"{"key":"c","code":"KeyC","ctrl":true}"#,
    );
    step(port);

    // -- the same key WITHOUT ctrl must not fire it ---------------------------
    // The discriminator: identical key, identical path, only the modifier bit
    // differs. Before this bead the server hard-coded every modifier to false,
    // so this case and the one above were indistinguishable.
    assert!(
        http_post(
            port,
            "/api/runtime/input/key_down",
            r#"{"key":"c","code":"KeyC"}"#,
        )
        .contains("200 OK"),
        "plain c key_down accepted"
    );
    step(port);
    assert_eq!(
        script_int(port, "seen_ui_copy"),
        1,
        "plain C (no Ctrl) must NOT resolve ui_copy — if this equals 2 the \
         modifier test is not being applied at all"
    );

    handle.stop();
}

/// The served client must actually forward the modifier fields; the server can
/// only read what the browser posts.
#[test]
fn served_editor_html_posts_browser_modifier_fields() {
    let html = gdeditor::editor_ui::EDITOR_HTML;
    for field in ["shiftKey", "ctrlKey", "altKey", "metaKey"] {
        assert!(
            html.contains(field),
            "the served editor client must forward `{field}` on key events"
        );
    }
    // The physical code and the echo flag ride along on the same POST — Godot
    // maps them to physical_keycode and InputEventKey.echo.
    assert!(
        html.contains("e.code"),
        "the client must post the physical `code` alongside the logical key"
    );
    assert!(
        html.contains("e.repeat"),
        "the client must post the browser's `repeat` (echo) flag"
    );
}
