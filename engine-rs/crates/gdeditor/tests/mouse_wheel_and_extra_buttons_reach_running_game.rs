//! pat-s3rrv.76 acceptance: wheel notches and thumb buttons reach the running
//! game through the browser input endpoints.
//!
//! Two gaps, both silent:
//!
//! * there was no wheel endpoint at all — `EDITOR_HTML`'s `wheel` listener only
//!   drives editor viewport zoom, so a notch could never become a
//!   `MOUSE_BUTTON_WHEEL_*` event for the game;
//! * `mouse_down` validated `button <= 2` and answered "invalid button (0-2)",
//!   so the thumb buttons were refused at the door. Mapping DOM 3/4 in
//!   `push_mouse_button_event` was necessary and NOT sufficient — the endpoint
//!   rejected them first.
//!
//! ⚠️ The script asserts on the `@GlobalScope` names rather than 5 and 8. A test
//! written against the literals would still pass if the indices were wrong, and
//! the index IS the contract a project depends on.

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

fn script_float(port: u16, name: &str) -> f64 {
    let node = runtime_node(port);
    property_value(&node, name)
        .and_then(|v| v.as_f64())
        .unwrap_or_else(|| panic!("script var `{name}` missing from /api/runtime/node: {node}"))
}

/// Counts `ui_copy` presses. `ui_copy` is a **builtin** action (Ctrl+C) — the
/// scene declares no input map of its own, which is the point: Godot seeds the
/// builtins before any project action is read.
const WATCHER_GD: &str = "\
extends Node2D
var frames = 0
var wheel_down = 0
var wheel_up = 0
var xbutton1 = 0
var xbutton2 = 0
func _process(delta):
    self.frames = self.frames + 1
func _input(event):
    if event is InputEventMouseButton and event.pressed:
        if event.button_index == MOUSE_BUTTON_WHEEL_DOWN:
            self.wheel_down = self.wheel_down + 1
        if event.button_index == MOUSE_BUTTON_WHEEL_UP:
            self.wheel_up = self.wheel_up + 1
        if event.button_index == MOUSE_BUTTON_XBUTTON1:
            self.xbutton1 = self.xbutton1 + 1
        if event.button_index == MOUSE_BUTTON_XBUTTON2:
            self.xbutton2 = self.xbutton2 + 1
";

#[test]
fn mouse_wheel_and_extra_buttons_reach_running_game() {
    let (handle, port, _dir) = boot_with_script(WATCHER_GD);
    play_paused(port);

    // The script must actually be running, or every zero below is meaningless.
    step(port);
    assert!(
        script_int(port, "frames") > 0,
        "the script must be running — 0 frames means it never attached"
    );
    assert_eq!(script_int(port, "wheel_down"), 0, "nothing scrolled yet");

    // -- a wheel notch DOWN ---------------------------------------------------
    assert!(
        http_post(port, "/api/runtime/input/mouse_wheel", r#"{"delta_y":1.0}"#,).contains("200 OK"),
        "the wheel endpoint must accept a notch while running"
    );
    step(port);
    assert_eq!(
        script_int(port, "wheel_down"),
        1,
        "`_input` must observe button_index == MOUSE_BUTTON_WHEEL_DOWN. A 0 \
         means the notch never became a game event — the state this bead found"
    );
    assert_eq!(
        script_int(port, "wheel_up"),
        0,
        "a positive deltaY is a scroll DOWN in the DOM; counting it as WHEEL_UP \
         would invert every scroll in the engine and still look like it worked"
    );

    // -- a wheel notch UP -----------------------------------------------------
    assert!(http_post(
        port,
        "/api/runtime/input/mouse_wheel",
        r#"{"delta_y":-1.0}"#,
    )
    .contains("200 OK"));
    step(port);
    assert_eq!(
        script_int(port, "wheel_up"),
        1,
        "negative deltaY is WHEEL_UP"
    );
    assert_eq!(
        script_int(port, "wheel_down"),
        1,
        "and it must not also count as another notch down"
    );

    // -- the thumb buttons ----------------------------------------------------
    // ⚠️ This is the pair that proves the endpoint AND the mapping. Before this
    // bead the POST was refused with "invalid button (0-2)", so the DOM 3/4
    // mapping alone would have changed nothing observable.
    assert!(
        http_post(port, "/api/runtime/input/mouse_down", r#"{"button":3}"#).contains("200 OK"),
        "DOM button 3 (back) must be accepted, not rejected as out of range"
    );
    step(port);
    assert_eq!(
        script_int(port, "xbutton1"),
        1,
        "DOM 3 is Godot's XBUTTON1 (8) — the browser's names are the reverse of \
         Godot's numbering"
    );

    assert!(
        http_post(port, "/api/runtime/input/mouse_down", r#"{"button":4}"#).contains("200 OK"),
        "DOM button 4 (forward) must be accepted"
    );
    step(port);
    assert_eq!(script_int(port, "xbutton2"), 1, "DOM 4 is XBUTTON2 (9)");

    // A genuinely out-of-range button is still refused, so widening the cap did
    // not turn the validator off.
    assert!(
        http_post(port, "/api/runtime/input/mouse_down", r#"{"button":9}"#).contains("400"),
        "button 9 is not a DOM button and must still be rejected"
    );

    drop(handle);
}

// ---------------------------------------------------------------------------
// Acceptance clause 1, second half: the notch must SCROLL something
// ---------------------------------------------------------------------------

/// Boots a scene whose `Main` owns a `ScrollContainer` with over-tall content.
///
/// ⚠️ The container's extent is written BOTH as `size` and as anchors+offsets.
/// `scroll_under_cursor` reads the stored `size`, but any render pass runs
/// `resolve_control_layout`, which recomputes `size` from the anchor/offset pair
/// — so a fixture that sets only one of them measures 200x100 or 0x0 depending
/// on whether a frame happened to draw, and the scroll step silently changes
/// with it.
///
/// Content height comes from the child's `custom_minimum_size`: `max_scroll` is
/// `content - view`, and `scroll_content_size` takes each child's effective
/// minimum. A child without one leaves the content exactly the view's size, so
/// there is nothing to scroll and the wheel legitimately does nothing.
fn boot_with_scroll_container() -> (EditorServerHandle, u16, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("tempdir");

    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let main = tree
        .add_child(root, Node::new("Main", "Node2D"))
        .expect("add main");

    let scroll = tree
        .add_child(main, Node::new("Scroll", "ScrollContainer"))
        .expect("add scroll");
    gdscene::control::set_position(&mut tree, scroll, gdcore::math::Vector2::new(0.0, 0.0));
    gdscene::control::set_size(&mut tree, scroll, gdcore::math::Vector2::new(200.0, 100.0));
    gdscene::control::apply_anchor_preset(
        &mut tree,
        scroll,
        gdscene::control::AnchorPreset::TopLeft,
    );
    gdscene::control::set_offset_right(&mut tree, scroll, 200.0);
    gdscene::control::set_offset_bottom(&mut tree, scroll, 100.0);

    // Seed the offset so the property exists before anything scrolls — reading
    // an absent property would fail as a confusing "no scroll_vertical" panic
    // rather than as the assertion that actually matters.
    gdscene::control::set_scroll(&mut tree, scroll, gdcore::math::Vector2::new(0.0, 0.0));

    let content = tree
        .add_child(scroll, Node::new("Content", "Control"))
        .expect("add content");
    gdscene::control::set_custom_minimum_size(
        &mut tree,
        content,
        gdcore::math::Vector2::new(200.0, 400.0),
    );

    let mut state = EditorState::new(tree);
    let root_path = dir.path().to_string_lossy().to_string();
    state.project_root = Some(root_path.clone());
    state.texture_cache.set_project_root(root_path);

    let port = free_port();
    let handle = EditorServerHandle::start(port, state);
    thread::sleep(Duration::from_millis(100));
    (handle, port, dir)
}

fn scroll_vertical(port: u16) -> f64 {
    let node = json_body(&http_get(port, "/api/runtime/node?path=/root/Main/Scroll"));
    property_value(&node, "scroll_vertical")
        .and_then(|v| v.as_f64())
        .unwrap_or_else(|| panic!("no scroll_vertical on the container: {node}"))
}

/// 🔑 A wheel notch advances a `ScrollContainer`'s offset in the running game.
///
/// The existing test proves a notch reaches a script as `MOUSE_BUTTON_WHEEL_DOWN`.
/// This proves it reaches the ENGINE — the half of acceptance clause 1 that was
/// still open. They are different failures: the event can arrive perfectly and
/// still scroll nothing if the GUI stage never finds an enclosing container.
///
/// ⚠️ The cursor is moved first on purpose. `api_input_mouse_wheel` takes its
/// position from `state.mouse_position`, NOT from the request body — a wheel
/// POST without a preceding move lands wherever the pointer was last left,
/// which for a fresh server is the origin, and the notch would hit nothing.
#[test]
fn a_wheel_notch_scrolls_a_scrollcontainer_in_the_running_game() {
    let (handle, port, _dir) = boot_with_scroll_container();
    play_paused(port);

    http_post(port, "/api/runtime/input/mouse_move", r#"{"x":50,"y":50}"#);
    let before = scroll_vertical(port);
    assert_eq!(before, 0.0, "the container starts unscrolled");

    assert!(
        http_post(port, "/api/runtime/input/mouse_wheel", r#"{"delta_y":1.0}"#).contains("200 OK"),
        "the wheel endpoint must accept a downward notch"
    );
    step(port);

    // Godot moves `page / PAGE_DIVISOR` per notch, floored at the range's step;
    // here the page IS the 100px view, so one notch is 12.5 — derived from the
    // fixture rather than copied out of the implementation.
    let after = scroll_vertical(port);
    assert!(
        (after - 12.5).abs() < 1e-3,
        "one wheel-down notch must advance the offset by view/8 = 12.5, got {after}"
    );

    drop(handle);
}

/// ⚠️ Wheeling somewhere else does NOT scroll the container.
///
/// Without this, the test above passes for an implementation that scrolls the
/// first ScrollContainer it can find regardless of where the pointer is — which
/// is exactly what "walk up from the hit node" is there to prevent.
#[test]
fn a_wheel_notch_away_from_the_container_scrolls_nothing() {
    let (handle, port, _dir) = boot_with_scroll_container();
    play_paused(port);

    // Well outside the 200x100 container.
    http_post(
        port,
        "/api/runtime/input/mouse_move",
        r#"{"x":900,"y":900}"#,
    );
    http_post(port, "/api/runtime/input/mouse_wheel", r#"{"delta_y":1.0}"#);
    step(port);

    assert_eq!(
        scroll_vertical(port),
        0.0,
        "a notch outside the container must leave it alone"
    );

    drop(handle);
}

/// Acceptance clause 3: the served page registers a `wheel` listener.
///
/// ⚠️ The literal clause is satisfied by ANY wheel listener, and one has existed
/// all along — it just zoomed the editor viewport and never told the game. So
/// the second assertion is the one that means anything: the listener must also
/// POST the notch to the runtime. Checking only the first would have passed
/// before this bead, against a page where scrolling a running game did nothing.
#[test]
fn the_served_page_forwards_the_wheel_to_the_running_game() {
    assert!(
        EDITOR_HTML.contains("addEventListener('wheel'"),
        "the served page must register a wheel listener"
    );
    assert!(
        EDITOR_HTML.contains("/api/runtime/input/mouse_wheel"),
        "the wheel listener must forward the notch to the running game; without \
         it the page only zooms the editor viewport"
    );
}

// ---------------------------------------------------------------------------
// Acceptance clause 2 — double_click on the second press inside the window
// ---------------------------------------------------------------------------

/// Records every mouse PRESS and which of them reported `double_click`.
///
/// ⚠️ Guarded on `button_index == 1` (Godot's `MOUSE_BUTTON_LEFT`) rather than
/// on `pressed`: `_input` receives every event, and a field a non-mouse event
/// does not carry reads back as `Nil`. Counting presses and recording WHICH one
/// was the double is what makes the assertion meaningful — a bare
/// `self.saw_double = event.double_click` would be overwritten by the next
/// event and could pass while reporting the wrong press.
const DOUBLE_CLICK_GD: &str = "\
extends Node2D
var presses = 0
var double_on_press = -1
func _input(event):
    if event.button_index == 1:
        if event.pressed:
            self.presses = self.presses + 1
            if event.double_click:
                self.double_on_press = self.presses
";

/// **Acceptance clause 2.** Two left-button presses inside the double-click
/// window: the FIRST must not be a double, the SECOND must be.
///
/// The window is `DOUBLE_TAP_FRAMES` (15) of `trace_frame`, and `push_input`
/// computes the flag from `last_mouse_press_frame` — the same mechanism the
/// touch path uses for `double_tap`, which is why the two agree. Stepping once
/// between the presses puts them one frame apart, well inside it.
#[test]
fn a_second_press_inside_the_window_reports_double_click() {
    let (handle, port, _dir) = boot_with_script(DOUBLE_CLICK_GD);
    play_paused(port);

    assert!(http_post(port, "/api/runtime/input/mouse_down", r#"{"button":0}"#).contains("200 OK"));
    step(port);
    assert_eq!(script_int(port, "presses"), 1, "the first press arrived");
    assert_eq!(
        script_int(port, "double_on_press"),
        -1,
        "⚠️ the FIRST press must never be a double — with no prior press there \
         is nothing to be a double OF, and a mechanism that reported true here \
         would make the second assertion pass for the wrong reason"
    );

    assert!(http_post(port, "/api/runtime/input/mouse_down", r#"{"button":0}"#).contains("200 OK"));
    step(port);

    assert_eq!(
        script_int(port, "presses"),
        2,
        "both presses reached _input"
    );
    assert_eq!(
        script_int(port, "double_on_press"),
        2,
        "the SECOND press must carry double_click == true"
    );

    drop(handle);
}

/// ⚠️ The window is per-BUTTON. A left press followed by a right press is two
/// different buttons and must not report a double — `last_mouse_press_frame` is
/// keyed by the Godot button index precisely so a fast alternation between
/// buttons cannot masquerade as a double-click.
#[test]
fn a_press_of_a_different_button_is_not_a_double_click() {
    let (handle, port, _dir) = boot_with_script(DOUBLE_CLICK_GD);
    play_paused(port);

    // Right button first (index 2), then left (index 1) — the script only
    // records the left one, which must be its FIRST press and not a double.
    assert!(http_post(port, "/api/runtime/input/mouse_down", r#"{"button":2}"#).contains("200 OK"));
    step(port);
    assert!(http_post(port, "/api/runtime/input/mouse_down", r#"{"button":0}"#).contains("200 OK"));
    step(port);

    assert_eq!(
        script_int(port, "presses"),
        1,
        "only the left press is counted"
    );
    assert_eq!(
        script_int(port, "double_on_press"),
        -1,
        "a different button in between must not make this a double-click"
    );

    drop(handle);
}

// ---------------------------------------------------------------------------
// Pen fields — pressure / tilt / pen_inverted
// ---------------------------------------------------------------------------

/// `if event is InputEventMouseMotion:` is the guard the existing motion tests
/// use; a bare field read would fire on every event and see `Nil`.
const PEN_GD: &str = "\
extends Node2D
var pressure = 0.0
var tilt_x = 0.0
var tilt_y = 0.0
var inverted = 0
func _input(event):
    if event is InputEventMouseMotion:
        self.pressure = event.pressure
        self.tilt_x = event.tilt.x
        self.tilt_y = event.tilt.y
        if event.pen_inverted:
            self.inverted = 1
";

/// A stylus sample reaches `_input` as Godot-shaped pressure/tilt/pen_inverted.
///
/// ⚠️ These are CARRIED data, not derived — `input_event_to_variant` hardcoded
/// `pressure: 0.0`, `tilt: ZERO`, `pen_inverted: false` for every motion event,
/// so a stylus was indistinguishable from a mouse no matter what the browser
/// reported. The values now ride a `PenState` side channel on `SceneTree`,
/// mirroring how `double_click` avoided adding a field to an enum variant with
/// dozens of construction sites.
#[test]
fn a_stylus_pointer_delivers_pressure_and_tilt_to_input() {
    let (handle, port, _dir) = boot_with_script(PEN_GD);
    play_paused(port);

    // Baseline: the script is running and nothing has reported pressure yet.
    assert!(http_post(
        port,
        "/api/runtime/input/mouse_move",
        r#"{"x":10.0,"y":10.0}"#
    )
    .contains("200 OK"));
    step(port);
    assert_eq!(
        script_float(port, "pressure"),
        0.0,
        "no stylus data has been sent yet"
    );

    // A pen sample. Godot's ranges: pressure 0..1, tilt -1..1 on both axes
    // (doc/classes/InputEventMouseMotion.xml).
    assert!(http_post(
        port,
        "/api/runtime/input/mouse_move",
        r#"{"x":20.0,"y":20.0,"pressure":0.75,"tilt_x":0.5,"tilt_y":-0.25,"pen_inverted":true}"#,
    )
    .contains("200 OK"));
    step(port);
    assert!(
        (script_float(port, "pressure") - 0.75).abs() < 1e-6,
        "`_input` must see the reported pressure, got {}",
        script_float(port, "pressure")
    );
    assert!(
        (script_float(port, "tilt_x") - 0.5).abs() < 1e-6,
        "tilt.x must survive the trip, got {}",
        script_float(port, "tilt_x")
    );
    assert!(
        (script_float(port, "tilt_y") + 0.25).abs() < 1e-6,
        "⚠️ tilt.y must keep its SIGN — negative is tilt away from the user, \
         and an abs() or a swapped axis reads plausible but wrong; got {}",
        script_float(port, "tilt_y")
    );
    assert_eq!(
        script_int(port, "inverted"),
        1,
        "the eraser end must report pen_inverted"
    );

    // 🔑 STICKY. A browser sends pen data on pointer events only, so an ordinary
    // move that omits it must NOT reset pressure to 0 — a script would read that
    // as the pen being lifted.
    assert!(http_post(
        port,
        "/api/runtime/input/mouse_move",
        r#"{"x":30.0,"y":30.0}"#
    )
    .contains("200 OK"));
    step(port);
    assert!(
        (script_float(port, "pressure") - 0.75).abs() < 1e-6,
        "a move carrying no pen data must leave the last sample standing, got {}",
        script_float(port, "pressure")
    );

    drop(handle);
}

/// The served page must actually collect the stylus data.
///
/// ⚠️ A `mousemove` listener cannot: `MouseEvent` has no `pressure`. The page
/// caches the sample from a `pointermove` listener and merges it into the
/// existing POST, rather than posting from both — two listeners would send two
/// mouse_move events for one motion.
#[test]
fn the_served_page_collects_stylus_data_from_pointer_events() {
    let html = gdeditor::editor_ui::EDITOR_HTML;
    assert!(
        html.contains("addEventListener('pointermove'"),
        "the page must listen for pointermove; mousemove carries no pressure"
    );
    assert!(
        html.contains("gameInputWithPen"),
        "the cached stylus sample must be merged into the mouse_move payload, \
         or the listener collects data that never leaves the browser"
    );
}
