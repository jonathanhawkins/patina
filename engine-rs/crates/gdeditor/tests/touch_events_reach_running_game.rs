//! pat-s3rrv.77 acceptance: browser touch reaches a running game.
//!
//! # The gap this closes
//!
//! `InputEvent::ScreenTouch`/`ScreenDrag`, per-finger tracking, Godot's
//! first-finger `mouse_from_touch_index` rule and the double-tap window all
//! existed and were tested — with **no producer**. The served client had zero
//! touch listeners, the server had no touch route, and the two
//! `input_devices/pointing/emulate_*` settings were declared and read by nobody.
//! A phone or tablet could not play a Patina game.
//!
//! ⚠️ **`InputState`'s emulation was not enough on its own, and that is the
//! subtle half.** `emulate_mouse_from_touch_event` applies the emulation *to
//! InputState* — `mouse_position`, `mouse_pressed`. The GUI stage consumes
//! `InputEvent`s, not `InputState`, so a touch could make
//! `Input.is_mouse_button_pressed()` true while every Control in the scene
//! stayed inert. Godot emits a real `InputEventMouseButton`; `MainLoop` now
//! synthesizes one, which is what lets a touch reach a Button — and what lets
//! turning the setting off suppress it while the `ScreenTouch` still arrives.

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

/// A Control node with an explicit rect. Both the position/size pair and the
/// anchor offsets are set: the hit test reads the former, the render pass
/// re-derives the rect from the latter.
fn control(name: &str, class: &str, x: f32, y: f32, w: f32, h: f32) -> Node {
    let mut n = Node::new(name, class);
    n.set_property(
        "position",
        Variant::Vector2(gdcore::math::Vector2::new(x, y)),
    );
    n.set_property("size", Variant::Vector2(gdcore::math::Vector2::new(w, h)));
    n.set_property("offset_left", Variant::Float(x as f64));
    n.set_property("offset_top", Variant::Float(y as f64));
    n.set_property("offset_right", Variant::Float((x + w) as f64));
    n.set_property("offset_bottom", Variant::Float((y + h) as f64));
    n
}

/// `Main` (records `_input`) → `Btn`, a Button that records `_gui_input`.
///
/// ⚠️ BOTH project roots are required and setting only one is silent: the PLAY
/// path resolves `_script_path` through `texture_cache.project_root()`, and
/// without it no script attaches — surfacing as every script variable being
/// absent rather than "the script never loaded".
fn boot() -> (EditorServerHandle, u16, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("tempdir");
    // ⚠️ `_input` runs for EVERY event, and the emulated mouse arrives right
    // after the touch — so an unguarded `self.x = event.index` would be
    // overwritten by the mouse event. The guard is what makes the recording
    // survive, and it doubles as the `index == 0` assertion.
    std::fs::write(
        dir.path().join("main.gd"),
        "extends Node2D\n\
         var touch_index = -1\n\
         var touch_pressed = 0\n\
         func _input(event):\n\
         \x20   if event.index == 0:\n\
         \x20       self.touch_index = 0\n\
         \x20       if event.pressed:\n\
         \x20           self.touch_pressed = 1\n",
    )
    .expect("write main.gd");
    // The Button records the GUI stage reaching it. That is the observable the
    // `emulate_mouse_from_touch` setting gates: a touch only reaches a Control
    // through the synthesized mouse event.
    std::fs::write(
        dir.path().join("btn.gd"),
        "extends Button\n\
         var gui_hits = 0\n\
         func _gui_input(event):\n\
         \x20   self.gui_hits = self.gui_hits + 1\n",
    )
    .expect("write btn.gd");

    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let mut main = Node::new("Main", "Node2D");
    main.set_property("_script_path", Variant::String("main.gd".to_string()));
    let main_id = tree.add_child(root, main).unwrap();
    let mut btn = control("Btn", "Button", 0.0, 0.0, 80.0, 40.0);
    btn.set_property("_script_path", Variant::String("btn.gd".to_string()));
    tree.add_child(main_id, btn).unwrap();

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

fn touch(port: u16, phase: &str, x: f32, y: f32) -> String {
    http_post(
        port,
        "/api/runtime/input/touch",
        &format!(
            r#"{{"phase":"{phase}","touches":[{{"identifier":0,"x":{x},"y":{y},"force":0}}]}}"#
        ),
    )
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

/// **The acceptance case.** A touchstart on the Button's rect reaches the
/// Button through mouse emulation AND arrives at `_input` as an
/// `InputEventScreenTouch` with `index == 0`, `pressed == true`.
#[test]
fn touch_events_reach_running_game() {
    let (handle, port, _dir) = boot();
    play_paused(port);

    let resp = touch(port, "start", 20.0, 20.0);
    assert!(
        resp.contains("200 OK"),
        "POST /api/runtime/input/touch must exist and accept a touchstart:\n{resp}"
    );
    assert!(
        resp.contains(r#""events":1"#),
        "one changed touch must produce one event:\n{resp}"
    );
    http_post(port, "/api/runtime/step", "");

    assert_eq!(
        script_var(port, "/root/Main", "touch_index"),
        Some(0.0),
        "the script's _input must observe an InputEventScreenTouch with index 0"
    );
    assert_eq!(
        script_var(port, "/root/Main", "touch_pressed"),
        Some(1.0),
        "…and pressed == true"
    );
    assert_eq!(
        script_var(port, "/root/Main/Btn", "gui_hits"),
        Some(1.0),
        "the touch must reach the Button through the emulated mouse event — 0 \
         means InputState was updated but no EVENT was synthesized, which is \
         exactly the gap this bead closes"
    );

    drop(handle);
}

/// ⚠️ With `emulate_mouse_from_touch = false` the Button must NOT be touched
/// while the `ScreenTouch` still arrives. This is the half that proves the
/// emulation is a real synthesized event rather than the GUI stage being taught
/// to accept touches directly — routing `ScreenTouch` into `gui_input_stage`
/// would press the Button here.
#[test]
fn disabling_mouse_emulation_suppresses_the_button_but_not_the_touch() {
    let (handle, port, _dir) = boot();

    let set = http_post(
        port,
        "/api/project_settings",
        r#"{"emulate_mouse_from_touch":false}"#,
    );
    assert!(
        set.contains("200 OK"),
        "the setting must be accepted:\n{set}"
    );

    // Applied at PLAY, so the setting has to be in place before the runtime
    // boots — which is also when Godot reads it.
    play_paused(port);
    touch(port, "start", 20.0, 20.0);
    http_post(port, "/api/runtime/step", "");

    assert_eq!(
        script_var(port, "/root/Main", "touch_index"),
        Some(0.0),
        "the ScreenTouch still reaches _input"
    );
    assert_eq!(
        script_var(port, "/root/Main/Btn", "gui_hits"),
        Some(0.0),
        "but with emulation off nothing must reach the Button"
    );

    drop(handle);
}

/// The served client registers the listeners. Without them the route above has
/// no producer and a real device still cannot play.
#[test]
fn the_served_editor_registers_touch_listeners() {
    let html = gdeditor::editor_ui::EDITOR_HTML;

    for phase in ["touchstart", "touchmove", "touchend", "touchcancel"] {
        assert!(
            html.contains(&format!("addEventListener('{phase}'")),
            "EDITOR_HTML must register a {phase} listener"
        );
    }
    assert!(
        html.contains("/api/runtime/input/touch"),
        "…and post them to the touch route"
    );
    // ⚠️ passive:false is load-bearing. Left passive — the browser default for
    // touchstart/touchmove — preventDefault() is a silent no-op, so the page
    // scrolls under the game and the browser also synthesizes delayed mouse
    // events, making every tap arrive twice.
    assert!(
        html.contains("{ passive: false }"),
        "the touch listeners must be registered non-passive"
    );
    // The lifted finger, not every finger still down.
    assert!(
        html.contains("changedTouches"),
        "the client must post changedTouches, not touches"
    );
}
