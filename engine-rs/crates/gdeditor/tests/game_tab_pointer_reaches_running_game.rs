//! A game played in the editor's Game tab must receive clicks. Found importing
//! the official Godot demo "Dodge the Creeps": its Start button could not be
//! pressed, because the page forwarded pointer input only from the EDIT
//! viewport (`#viewport-container`), which is hidden (0x0) while the Game tab
//! shows the game on `#game-screen-img`. The page sent no `mouse_down` at all.
//!
//! Two halves:
//! - the server: a press carries its own position, so a click that arrives
//!   with no `mouse_move` before it (a tap, a synthetic click) lands where it
//!   happened, preceded by the motion a real pointer would have made;
//! - the page: the pointer listeners are registered on the Game tab's surface
//!   too, and the image cannot be native-dragged (which swallows `mouseup`).

use gdeditor::editor_server::{EditorServerHandle, EditorState};
use gdeditor::editor_ui::EDITOR_HTML;
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

fn http(port: u16, request: &str) -> String {
    let mut stream = (0..40)
        .find_map(|_| {
            TcpStream::connect(format!("127.0.0.1:{port}"))
                .ok()
                .or_else(|| {
                    thread::sleep(Duration::from_millis(50));
                    None
                })
        })
        .expect("connect");
    stream.write_all(request.as_bytes()).unwrap();
    let mut resp = String::new();
    stream.read_to_string(&mut resp).unwrap();
    resp
}

fn post(port: u16, path: &str, body: &str) -> String {
    http(
        port,
        &format!(
            "POST {path} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        ),
    )
}

fn get(port: u16, path: &str) -> String {
    http(
        port,
        &format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n"),
    )
}

/// The script variable `name` on `/root/Main`, as `/api/runtime/node` reports it.
fn script_var(port: u16, name: &str) -> serde_json::Value {
    let resp = get(port, "/api/runtime/node?path=/root/Main");
    let body: serde_json::Value =
        serde_json::from_str(resp.split("\r\n\r\n").nth(1).unwrap_or("")).unwrap();
    body["properties"]
        .as_array()
        .and_then(|props| {
            props
                .iter()
                .find(|p| p["name"].as_str() == Some(name))
                .map(|p| p["value"]["value"].clone())
        })
        .unwrap_or_else(|| panic!("script var `{name}` missing: {body}"))
}

const WATCHER_GD: &str = "\
extends Node2D
var frames = 0
var press_x = -1.0
var press_y = -1.0
var release_x = -1.0
var moved_before_press = false
var saw_motion = false
func _process(delta):
    self.frames = self.frames + 1
func _input(event):
    if event is InputEventMouseMotion:
        self.saw_motion = true
    if event is InputEventMouseButton and event.button_index == MOUSE_BUTTON_LEFT:
        if event.pressed:
            self.press_x = event.position.x
            self.press_y = event.position.y
            self.moved_before_press = self.saw_motion
        else:
            self.release_x = event.position.x
";

#[test]
fn a_press_with_coordinates_and_no_prior_move_lands_where_it_happened() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("main.gd"), WATCHER_GD).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let mut main = Node::new("Main", "Node2D");
    main.set_property("_script_path", Variant::String("main.gd".into()));
    tree.add_child(root, main).unwrap();
    let mut state = EditorState::new(tree);
    let root_path = dir.path().to_string_lossy().to_string();
    state.project_root = Some(root_path.clone());
    state.texture_cache.set_project_root(root_path);
    let port = free_port();
    let handle = EditorServerHandle::start(port, state);
    thread::sleep(Duration::from_millis(100));

    assert!(post(port, "/api/runtime/play", "{}").contains("200 OK"));
    post(port, "/api/runtime/pause", "");
    post(port, "/api/runtime/step", "");
    assert!(
        script_var(port, "frames").as_i64().unwrap_or(0) > 0,
        "the script must be running, or every value below is meaningless"
    );

    // No mouse_move at all: the press alone says where it happened.
    assert!(post(port, "/api/runtime/input/mouse_down", r#"{"button":0,"x":123,"y":45}"#)
        .contains("200 OK"));
    post(port, "/api/runtime/step", "");
    assert_eq!(script_var(port, "press_x").as_f64(), Some(123.0));
    assert_eq!(script_var(port, "press_y").as_f64(), Some(45.0));
    assert_eq!(
        script_var(port, "moved_before_press").as_bool(),
        Some(true),
        "a pointer that moved to the press point is reported as a motion first"
    );

    assert!(post(port, "/api/runtime/input/mouse_up", r#"{"button":0,"x":130,"y":45}"#)
        .contains("200 OK"));
    post(port, "/api/runtime/step", "");
    assert_eq!(script_var(port, "release_x").as_f64(), Some(130.0));

    // Without coordinates the last position stands (the edit viewport path,
    // and any older client).
    assert!(post(port, "/api/runtime/input/mouse_down", r#"{"button":0}"#).contains("200 OK"));
    post(port, "/api/runtime/step", "");
    assert_eq!(script_var(port, "press_x").as_f64(), Some(130.0));

    handle.stop();
}

#[test]
fn the_game_tab_surface_forwards_pointer_input() {
    // The listeners go on BOTH surfaces that show the running game.
    assert!(
        EDITOR_HTML.contains("var gameInputSurfaces = [vp, gameScreenImg].filter(Boolean);"),
        "pointer input must be registered on the Game tab's #game-screen-img"
    );
    for event in ["mousemove", "mousedown", "mouseup", "touchstart"] {
        assert!(
            EDITOR_HTML.contains(&format!("s.addEventListener('{event}'")),
            "`{event}` must be registered per surface"
        );
    }
    // A press sends its own position.
    assert!(EDITOR_HTML.contains("api('POST', '/api/runtime/input/mouse_down', gameButtonBody(e));"));
    // A natively draggable <img> starts a drag on press-and-move and the
    // browser then never delivers the `mouseup`.
    assert!(EDITOR_HTML.contains(r#"<img id="game-screen-img" alt="game" draggable="false""#));
}

#[test]
fn the_game_tab_surface_forwards_the_wheel_with_its_position() {
    // The Game tab's screen registers a wheel listener of its own (the edit
    // viewport's wheel goes through its zoom handler), and posts the position
    // in the SAME request so the notch cannot race a separate motion post.
    assert!(EDITOR_HTML.contains("gameScreenImg.addEventListener('wheel', gameInputWheelHandler"));
    assert!(EDITOR_HTML.contains(
        "api('POST', '/api/runtime/input/mouse_wheel', { delta_y: e.deltaY || 0, delta_x: e.deltaX || 0, x: wp.x, y: wp.y });"
    ));
}

const WHEEL_GD: &str = "\
extends Node2D
var frames = 0
var wheel_x = -1.0
func _process(delta):
    self.frames = self.frames + 1
func _input(event):
    if event is InputEventMouseButton and event.pressed and event.button_index == MOUSE_BUTTON_WHEEL_DOWN:
        self.wheel_x = event.position.x
";

#[test]
fn a_wheel_notch_with_a_position_lands_there() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("main.gd"), WHEEL_GD).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let mut main = Node::new("Main", "Node2D");
    main.set_property("_script_path", Variant::String("main.gd".into()));
    tree.add_child(root, main).unwrap();
    let mut state = EditorState::new(tree);
    let root_path = dir.path().to_string_lossy().to_string();
    state.project_root = Some(root_path.clone());
    state.texture_cache.set_project_root(root_path);
    let port = free_port();
    let handle = EditorServerHandle::start(port, state);
    thread::sleep(Duration::from_millis(100));
    assert!(post(port, "/api/runtime/play", "{}").contains("200 OK"));
    post(port, "/api/runtime/pause", "");
    post(port, "/api/runtime/step", "");
    assert!(post(port, "/api/runtime/input/mouse_wheel", r#"{"delta_y":1.0,"x":77,"y":5}"#)
        .contains("200 OK"));
    post(port, "/api/runtime/step", "");
    assert_eq!(script_var(port, "wheel_x").as_f64(), Some(77.0));
    handle.stop();
}

fn status(port: u16) -> serde_json::Value {
    let resp = get(port, "/api/runtime/status");
    serde_json::from_str(resp.split("\r\n\r\n").nth(1).unwrap_or("")).unwrap()
}

/// Play opens a sound device only when the request asks (the editor's Play
/// buttons send `audio: true`; an agent's run and this suite do not), and the
/// Game view's mute reaches the running game. Found importing Dodge the
/// Creeps: editor Play attached no output at all, so every game was silent,
/// and the mute flag was stored but never applied.
#[test]
fn play_audio_is_opt_in_and_mute_reaches_the_running_game() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    tree.add_child(root, Node::new("Main", "Node")).unwrap();
    let port = free_port();
    let handle = EditorServerHandle::start(port, EditorState::new(tree));
    thread::sleep(Duration::from_millis(100));

    assert!(post(port, "/api/runtime/play", "{}").contains("200 OK"));
    let s = status(port);
    assert_eq!(s["audio_output"], false, "no `audio` request, no device: {s}");
    assert_eq!(s["audio_muted"], false);

    assert!(post(port, "/api/runtime/mute", r#"{"muted":true}"#).contains("200 OK"));
    assert_eq!(status(port)["audio_muted"], true, "mute is applied to the running game");
    post(port, "/api/runtime/mute", r#"{"muted":false}"#);
    assert_eq!(status(port)["audio_muted"], false);

    // A mute set before Play carries into the next run.
    post(port, "/api/runtime/stop", "");
    post(port, "/api/runtime/mute", r#"{"muted":true}"#);
    assert!(post(port, "/api/runtime/play", "{}").contains("200 OK"));
    assert_eq!(status(port)["audio_muted"], true);
    handle.stop();

    // The page's Play buttons ask for sound.
    assert!(EDITOR_HTML.contains("api('POST', '/api/runtime/play', { movie: movieMakerEnabled, audio: true })"));
}
