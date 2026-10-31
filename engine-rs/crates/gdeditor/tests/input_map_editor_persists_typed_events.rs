//! pat-mgixk.16 acceptance — the Input Map editor captures TYPED events and a
//! deadzone, and persists them to `project.godot [input]` through the served
//! editor.
//!
//! Godot's Input Map dialog captures a real `InputEventKey` (with modifiers and
//! a logical/physical keycode), `InputEventJoypadButton`, `InputEventMouseButton`
//! and a per-action deadzone, and writes them into `project.godot`'s `[input]`
//! section (`core/input/input_map.cpp` `load_from_project_settings`).
//!
//! Patina's editor stopped at `EditorState::input_map`, an in-memory
//! `HashMap<String, Vec<String>>`. So `POST /api/input_map` could only express a
//! bare key NAME — no modifiers, no physical keycode, no joypad index, and
//! nowhere at all to put a deadzone — and it wrote nothing to disk, so the edit
//! died with the process.
//!
//! Entry point, per the bead's Wiring requirement: the real editor booted with
//! `EditorServerHandle::start` on a temp project directory and driven over HTTP,
//! as `editor_play_input_map.rs` and `anim_tree_apply_reaches_the_runtime.rs` do.
//! Nothing here constructs `input_map::InputMap` directly — that layer is
//! already covered by `input_map_project_bridge.rs`.
//!
//! ⚠️ The Play half of the acceptance ("a script in Play sees
//! `InputMap.has_action`") is NOT re-asserted here, deliberately.
//! `EditorState::build_play_input_map` already treats the project's `[input]`
//! as the source of truth and is covered by the existing green
//! `editor_play_input_map.rs` (pat-mgixk.24). The link THIS bead adds is getting
//! the typed data *into* `project.godot` in the first place — which is what the
//! on-disk and second-editor assertions below prove. Duplicating a green test
//! would assert someone else's code and tell us nothing new.

use gdeditor::editor_server::{EditorServerHandle, EditorState};
use gdscene::SceneTree;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;
use std::time::Duration;

/// A project that ALREADY declares a mouse binding, so the test can prove an
/// unrelated pre-existing event is not rewritten or dropped. Written in the
/// exact form `gdresource::project::serialize_input_action` emits, so it
/// round-trips through the parser rather than testing a shape Godot never
/// writes.
const PROJECT_GODOT: &str = r#"[application]

config/name="InputMapEditor"

[input]

fire={
"deadzone": 0.5,
"events": [Object(InputEventMouseButton,"resource_local_to_scene":false,"resource_name":"","device":-1,"window_id":0,"alt_pressed":false,"shift_pressed":false,"ctrl_pressed":false,"meta_pressed":false,"button_mask":0,"position":Vector2(0, 0),"global_position":Vector2(0, 0),"factor":1.0,"button_index":1,"canceled":false,"pressed":false,"double_click":false,"script":null)]
}
"#;

/// `Space` as a PHYSICAL keycode, plus joypad button 0, at deadzone 0.3 — the
/// three things the old string map could not represent. Events are encoded the
/// way `InputMap::to_json` emits them, so a client can GET, edit and POST back
/// without a second format.
const JUMP_BODY: &str = r#"{"action":"jump","deadzone":0.3,"events":[
  {"KeyEvent":{"keycode":0,"physical_keycode":32,"ctrl":false,"shift":false,"alt":false,"meta":false}},
  {"JoyButton":0}
]}"#;

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .expect("bind an ephemeral port")
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

/// Boots the real editor server rooted at `dir`.
///
/// ⚠️ BOTH roots are set, as `editor_play_input_map.rs` documents: the editor
/// resolves project-relative paths through `texture_cache.project_root()` as
/// well as `project_root`, and setting only the first fails silently.
fn boot_at(port: u16, dir: &std::path::Path) -> EditorServerHandle {
    let mut state = EditorState::new(SceneTree::new());
    let root = dir.to_string_lossy().to_string();
    state.project_root = Some(root.clone());
    state.texture_cache.set_project_root(root);
    let handle = EditorServerHandle::start(port, state);
    thread::sleep(Duration::from_millis(100));
    handle
}

#[test]
fn input_map_editor_persists_typed_events() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("project.godot"), PROJECT_GODOT).expect("write project");

    let port = free_port();
    let _handle = boot_at(port, dir.path());

    // -- the edit, over HTTP ------------------------------------------------
    let resp = http_post(port, "/api/input_map", JUMP_BODY);
    assert!(
        resp.contains("200 OK"),
        "POST /api/input_map must accept typed events and a deadzone. A 400 \
         means an event failed to deserialize. Response: {resp}"
    );

    // -- 🔑 it reached project.godot on disk --------------------------------
    //
    // This is the assertion the bead exists for. Before the fix the handler
    // wrote NOTHING to disk — verified by there being no `project.godot`,
    // `set_input_action`, `save` or `write` anywhere in its body.
    let on_disk = std::fs::read_to_string(dir.path().join("project.godot"))
        .expect("project.godot is still readable");

    assert!(
        on_disk.contains("jump="),
        "the action must be written into the [input] section. project.godot:\n{on_disk}"
    );
    assert!(
        on_disk.contains("\"deadzone\": 0.3"),
        "the per-action deadzone must persist — the old string map had nowhere \
         to put it at all. project.godot:\n{on_disk}"
    );
    assert!(
        on_disk.contains("Object(InputEventKey") && on_disk.contains("\"physical_keycode\":32"),
        "the key binding must persist as a typed InputEventKey carrying the \
         PHYSICAL keycode 32, not as the key name \"Space\". project.godot:\n{on_disk}"
    );
    assert!(
        on_disk.contains("Object(InputEventJoypadButton") && on_disk.contains("\"button_index\":0"),
        "the joypad binding must persist as a typed InputEventJoypadButton. \
         project.godot:\n{on_disk}"
    );

    // -- the pre-existing binding is untouched ------------------------------
    //
    // ⚠️ Only the edited action is written through, so an entry the editor did
    // not touch — and a class it may not even model — must survive verbatim.
    assert!(
        on_disk.contains("fire=") && on_disk.contains("Object(InputEventMouseButton"),
        "the pre-existing `fire` mouse binding must stay an \
         InputEventMouseButton. project.godot:\n{on_disk}"
    );

    // -- GET returns the typed events ---------------------------------------
    //
    // Previously GET rebuilt the model from the LOSSY string map, so it could
    // not report a physical keycode or a deadzone even immediately after a POST.
    let got = http_get(port, "/api/input_map");
    assert!(
        got.contains("physical_keycode") && got.contains("32"),
        "GET /api/input_map must return the typed key event. Response: {got}"
    );
    assert!(
        got.contains("0.3"),
        "GET /api/input_map must report the action's deadzone. Response: {got}"
    );

    // -- a second editor on the same directory sees it ----------------------
    //
    // The point of persisting: this editor has its own fresh EditorState and
    // never saw the POST, so anything it reports came off disk.
    let port2 = free_port();
    let _handle2 = boot_at(port2, dir.path());
    let second = http_get(port2, "/api/input_map");
    assert!(
        second.contains("jump"),
        "a second editor booted on the same project directory must see `jump` \
         — if it does not, the edit never left the first editor's memory. \
         Response: {second}"
    );
    assert!(
        second.contains("physical_keycode"),
        "and it must see the TYPED event, not a downgraded key name. \
         Response: {second}"
    );
}
