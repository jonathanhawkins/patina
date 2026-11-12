//! pat-mgixk.24 acceptance — editor Play runs the PROJECT's `[input]` actions.
//!
//! `InputMap::load_from_project_godot` (gdplatform) is the real parser: joypad
//! buttons, joypad motion with a signed axis, mouse buttons, modifier combos
//! and a per-action deadzone. It had exactly ONE caller, in `patina-runner`.
//! Editor Play used `EditorState::build_engine_input_map`, built from a
//! hard-coded `default_input_map()` that bound string key names only, with
//! deadzone 0, added `a`/`d`/`w`/`s` to the `ui_*` actions and invented two
//! actions (`shoot`, `jump`) no project declares.
//!
//! So a project's gamepad, mouse and modifier bindings did nothing in Play and
//! only started working after an export — the worst shape for this class of
//! bug, because the editor is where you test the controls.
//!
//! ⚠️ Every assertion goes through a SCRIPT in the running scene, never through
//! `/api/runtime/input/state`. That endpoint answers from
//! `EditorState::is_action_pressed`, which walks the editor's own string map and
//! never reaches `KeyCombo::modifiers_satisfied_by` — it reports a
//! modifier-bound action identically with and without the modifier, so a test
//! written against it passes on completely broken plumbing. The sibling
//! `browser_modifier_keys_reach_running_game.rs` records the same trap.

use gdeditor::editor_server::{EditorServerHandle, EditorState};
use gdscene::node::Node;
use gdscene::SceneTree;
use gdvariant::Variant;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;
use std::time::Duration;

/// A project that binds `fire` to joypad button 0 and `save` to Ctrl+S, written
/// the way Godot serialises them.
///
/// ⚠️ `fire` deliberately omits `deadzone`, which is the other half of this
/// bead: the parser defaulted to 0.5 where Godot's `DEFAULT_DEADZONE` is 0.2.
/// That value is not observable over HTTP, so it is asserted in
/// `gdplatform::input`'s own tests
/// (`project_action_without_a_deadzone_takes_godots_default`); what this file
/// proves is that the action arrives at all.
///
/// ⚠️ Neither action is a `ui_*` name, and the project declares no `ui_left` —
/// so `ui_left` must come from the builtins `load_from_project_godot` installs,
/// with Godot's binding and NOT the `a` the editor used to add.
const PROJECT_GODOT: &str = r#"config_version=5

[application]

config/name="Input Map Fixture"

[input]

fire={
"events": [Object(InputEventJoypadButton,"resource_local_to_scene":false,"resource_name":"","device":-1,"button_index":0,"pressure":0.0,"pressed":false,"script":null)
]
}
save={
"deadzone": 0.5,
"events": [Object(InputEventKey,"resource_local_to_scene":false,"resource_name":"","device":-1,"window_id":0,"alt_pressed":false,"shift_pressed":false,"ctrl_pressed":true,"meta_pressed":false,"pressed":false,"keycode":0,"physical_keycode":83,"key_label":0,"unicode":0,"location":0,"echo":false,"script":null)
]
}
"#;

/// Counts the frames each action is held. Counters rather than booleans so a
/// "was it ever pressed" answer cannot be confused with "is it pressed now",
/// and so a stuck action is visible as a count that keeps climbing.
const COUNTER_GD: &str = "\
extends Node2D
var frames = 0
var fire_frames = 0
var save_frames = 0
var left_frames = 0
func _process(delta):
    self.frames = self.frames + 1
    if Input.is_action_pressed(\"fire\"):
        self.fire_frames = self.fire_frames + 1
    if Input.is_action_pressed(\"save\"):
        self.save_frames = self.save_frames + 1
    if Input.is_action_pressed(\"ui_left\"):
        self.left_frames = self.left_frames + 1
";

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

fn json_body(resp: &str) -> serde_json::Value {
    serde_json::from_str(resp.split("\r\n\r\n").nth(1).unwrap_or(""))
        .unwrap_or(serde_json::Value::Null)
}

/// Boots the real editor server over a one-node scene whose script is on disk,
/// in a project directory that also carries [`PROJECT_GODOT`].
///
/// ⚠️ BOTH roots are required and setting only the first is silent: the PLAY
/// path resolves `_script_path` against `texture_cache.project_root()`, and
/// without it no script attaches at all — which surfaces as every script
/// variable being ABSENT rather than "the script never loaded".
fn boot(port: u16) -> (EditorServerHandle, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("main.gd"), COUNTER_GD).expect("write script");
    std::fs::write(dir.path().join("project.godot"), PROJECT_GODOT).expect("write project");

    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let mut main = Node::new("Main", "Node2D");
    main.set_property("_script_path", Variant::String("main.gd".to_string()));
    tree.add_child(root, main).unwrap();

    let mut state = EditorState::new(tree);
    let root_path = dir.path().to_string_lossy().to_string();
    state.project_root = Some(root_path.clone());
    state.texture_cache.set_project_root(root_path);

    let handle = EditorServerHandle::start(port, state);
    thread::sleep(Duration::from_millis(100));
    (handle, dir)
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

/// ⚠️ The variable name is a **value**, not a key, and `value` is doubled.
/// Walking the JSON for a key named `fire_frames` matches nothing, ever.
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
    let node = json_body(&http_get(port, "/api/runtime/node?path=/root/Main"));
    property_value(&node, name)
        .and_then(|v| v.as_i64())
        .unwrap_or_else(|| panic!("script var `{name}` missing from /api/runtime/node: {node}"))
}

/// **The acceptance test.**
#[test]
fn editor_play_project_input_map_test() {
    let port = free_port();
    let (handle, _dir) = boot(port);

    play_paused(port);
    step(port);
    assert!(
        script_int(port, "frames") >= 1,
        "the script must be attached and running, or every assertion below is \
         vacuous"
    );
    assert_eq!(script_int(port, "fire_frames"), 0, "nothing is pressed yet");

    // -- (a) a project JOYPAD binding reaches the game -----------------------
    //
    // Button 0 is Godot's `button_index: 0` and the browser's standard-gamepad
    // index 0; both resolve to `GamepadButton::FaceA`.
    assert!(
        http_post(
            port,
            "/api/runtime/input/gamepad",
            r#"{"device":0,"buttons":[true],"axes":[0.0,0.0]}"#
        )
        .contains("200 OK"),
        "gamepad poll accepted while running"
    );
    step(port);
    assert!(
        script_int(port, "fire_frames") >= 1,
        "a project action bound to InputEventJoypadButton 0 must be pressed by \
         gamepad button 0. Zero here is the whole bug: the Play map was built \
         from the editor's string key list, which has no joypad bindings at all"
    );

    // -- (b) a modifier-bound project action needs its modifier --------------
    assert!(
        http_post(port, "/api/runtime/input/key_down", r#"{"key":"s"}"#).contains("200 OK"),
        "key_down accepted"
    );
    step(port);
    assert_eq!(
        script_int(port, "save_frames"),
        0,
        "`save` is bound to Ctrl+S, so plain S must NOT trigger it. A non-zero \
         count means the binding lost its modifier and became a bare key — \
         which is exactly what the old string map could express"
    );

    http_post(port, "/api/runtime/input/key_up", r#"{"key":"s"}"#);
    step(port);
    assert!(
        http_post(
            port,
            "/api/runtime/input/key_down",
            r#"{"key":"s","ctrl":true}"#
        )
        .contains("200 OK"),
        "ctrl+s key_down accepted"
    );
    step(port);
    assert!(
        script_int(port, "save_frames") >= 1,
        "...and Ctrl+S must trigger it"
    );
    http_post(
        port,
        "/api/runtime/input/key_up",
        r#"{"key":"s","ctrl":true}"#,
    );
    step(port);

    // -- (c) ui_left is the BUILTIN binding, not the editor's invented one ---
    let left_before = script_int(port, "left_frames");
    assert!(
        http_post(port, "/api/runtime/input/key_down", r#"{"key":"a"}"#).contains("200 OK"),
        "key_down a accepted"
    );
    step(port);
    assert_eq!(
        script_int(port, "left_frames"),
        left_before,
        "`ui_left` must not be bound to A. Godot binds it to Left/joypad dpad; \
         the editor's hard-coded map added `a`, so a game testing `ui_left` \
         responded to A in Play and not in an export"
    );
    http_post(port, "/api/runtime/input/key_up", r#"{"key":"a"}"#);
    step(port);

    // ...and the real binding still works, so (c) is not passing merely
    // because `ui_left` is dead.
    assert!(
        http_post(
            port,
            "/api/runtime/input/key_down",
            r#"{"key":"ArrowLeft"}"#
        )
        .contains("200 OK"),
        "key_down ArrowLeft accepted"
    );
    step(port);
    assert!(
        script_int(port, "left_frames") > left_before,
        "`ui_left` must still fire on its BUILTIN binding — otherwise (c) \
         passes on an action that resolves nothing at all"
    );

    drop(handle);
}
