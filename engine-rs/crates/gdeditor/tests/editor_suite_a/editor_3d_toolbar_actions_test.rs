//! App-level behavior tests for the 3D spatial-editor toolbar endpoints and the
//! viewport auto-resize, plus the render effects the toolbar toggles drive.
//!
//! Covers the endpoints added for Godot Node3DEditor toolbar parity:
//!   - `POST /api/viewport/resize`            (browser-reported panel size)
//!   - `POST /api/viewport/3d/view_flag`      (sun/env/grid/origin/local/snap)
//!   - `POST /api/viewport/3d/snap_config`    (Configure Snap increments)
//!   - `POST /api/viewport/3d/sun_config`     (Sun & Environment settings)
//!   - `POST /api/viewport/3d/lock` `.../group` (selection lock/group)
//!   - `POST /api/viewport/3d/snap_to_floor`  (drop selection to y=0)
//!   - `POST /api/viewport/3d/translate_selection` (Transform Dialog)
//!   - `POST /api/viewport/3d/ground_point`   (Ruler measurement)
//! and the renderer honoring the sun/environment preview toggles.

use gdeditor::editor_server::{EditorServerHandle, EditorState};
use gdeditor::viewport_3d::Viewport3D;
use gdeditor::viewport_3d_render::render_editor_3d_frame;
use gdscene::node::Node;
use gdscene::{node3d, SceneTree};
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

/// A 3D scene with a single MeshInstance3D placed above the floor so
/// snap-to-floor and translate have a real spatial target.
fn build_3d_scene() -> (SceneTree, u64) {
    gdobject::class_db::register_3d_classes();
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let crate_id = tree
        .add_child(root, Node::new("Crate", "MeshInstance3D"))
        .unwrap();
    node3d::set_position(&mut tree, crate_id, gdcore::math::Vector3::new(2.0, 5.0, -1.0));
    (tree, crate_id.raw())
}

fn start() -> (EditorServerHandle, u16, u64) {
    let (tree, crate_raw) = build_3d_scene();
    let port = free_port();
    let handle = EditorServerHandle::start(port, EditorState::new(tree));
    thread::sleep(Duration::from_millis(100));
    (handle, port, crate_raw)
}

fn http_post(port: u16, path: &str, body: &str) -> String {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
    let req = format!(
        "POST {path} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(req.as_bytes()).unwrap();
    let mut buf = Vec::new();
    let _ = stream.read_to_end(&mut buf);
    String::from_utf8_lossy(&buf).to_string()
}

fn body(resp: &str) -> String {
    resp.split("\r\n\r\n").nth(1).unwrap_or("").to_string()
}

/// The browser reports the real panel size so the server renders at the panel's
/// aspect (the full-panel fix); the size is clamped to [64, 2048] per axis.
#[test]
fn resize_sets_and_clamps_render_size() {
    let (handle, port, _) = start();
    let r = body(&http_post(port, "/api/viewport/resize", r#"{"w":1280,"h":720}"#));
    assert!(r.contains(r#""w":1280"#) && r.contains(r#""h":720"#), "resize echo: {r}");
    // Over-large requests clamp to 2048; tiny ones clamp up to 64.
    let r2 = body(&http_post(port, "/api/viewport/resize", r#"{"w":9000,"h":10}"#));
    assert!(r2.contains(r#""w":2048"#), "width should clamp to 2048: {r2}");
    assert!(r2.contains(r#""h":64"#), "height should clamp to 64: {r2}");
    handle.stop();
}

/// Each boolean view/preview flag toggles in the 3D state. `value` is honored
/// when present and the flag is flipped when omitted.
#[test]
fn view_flag_toggles_each_flag() {
    let (handle, port, _) = start();
    // Defaults: sun/environment/grid/origin on, local/snap off.
    for (flag, off) in [
        ("sun", r#""sun":false"#),
        ("environment", r#""environ":false"#),
        ("grid", r#""grid":false"#),
        ("origin", r#""origin":false"#),
    ] {
        let r = body(&http_post(
            port,
            "/api/viewport/3d/view_flag",
            &format!(r#"{{"flag":"{flag}","value":false}}"#),
        ));
        assert!(r.contains(off), "flag {flag} should be off: {r}");
    }
    // local_space / snap default off → flipping with no value turns them on.
    let r = body(&http_post(port, "/api/viewport/3d/view_flag", r#"{"flag":"local_space"}"#));
    assert!(r.contains(r#""local":true"#), "local flip: {r}");
    let r = body(&http_post(port, "/api/viewport/3d/view_flag", r#"{"flag":"snap"}"#));
    assert!(r.contains(r#""snap":true"#), "snap flip: {r}");
    handle.stop();
}

/// Configure Snap sets the transform increments returned in the 3D state.
#[test]
fn snap_config_sets_increments() {
    let (handle, port, _) = start();
    let r = body(&http_post(
        port,
        "/api/viewport/3d/snap_config",
        r#"{"translate":2.5,"rotate":30,"scale":0.25}"#,
    ));
    assert!(r.contains(r#""snap_t":2.5"#), "translate step: {r}");
    assert!(r.contains(r#""snap_r":30"#), "rotate step: {r}");
    assert!(r.contains(r#""snap_s":0.25"#), "scale step: {r}");
    handle.stop();
}

/// Lock / group act on the current selection, setting Godot's edit meta.
#[test]
fn lock_and_group_act_on_selection() {
    let (handle, port, crate_raw) = start();
    http_post(port, "/api/node/select", &format!(r#"{{"node_id":{crate_raw}}}"#));
    let r = body(&http_post(port, "/api/viewport/3d/lock", r#"{"locked":true}"#));
    assert!(r.contains(r#""count":1"#) && r.contains(r#""locked":true"#), "lock: {r}");
    let r = body(&http_post(port, "/api/viewport/3d/group", r#"{"grouped":true}"#));
    assert!(r.contains(r#""count":1"#) && r.contains(r#""grouped":true"#), "group: {r}");
    handle.stop();
}

/// Snap Object to Floor drops the selected spatial node to y = 0.
#[test]
fn snap_to_floor_moves_selection() {
    let (handle, port, crate_raw) = start();
    http_post(port, "/api/node/select", &format!(r#"{{"node_id":{crate_raw}}}"#));
    let r = body(&http_post(port, "/api/viewport/3d/snap_to_floor", "{}"));
    assert!(r.contains(r#""moved":1"#), "snap_to_floor should move 1 node: {r}");
    handle.stop();
}

/// The Transform Dialog's relative translate moves the selection.
#[test]
fn translate_selection_moves() {
    let (handle, port, crate_raw) = start();
    http_post(port, "/api/node/select", &format!(r#"{{"node_id":{crate_raw}}}"#));
    let r = body(&http_post(
        port,
        "/api/viewport/3d/translate_selection",
        r#"{"dx":1,"dy":2,"dz":3}"#,
    ));
    assert!(r.contains(r#""moved":1"#), "translate should move 1 node: {r}");
    handle.stop();
}

/// The Ruler's ground-point query hits the y = 0 plane for a ray pointing at the
/// floor (the default editor camera looks down at the origin).
#[test]
fn ground_point_hits_floor() {
    let (handle, port, _) = start();
    // Centre of an 800x600 viewport — the default camera frames the origin.
    let r = body(&http_post(port, "/api/viewport/3d/ground_point", r#"{"x":400,"y":300}"#));
    assert!(r.contains(r#""hit":true"#), "centre ray should hit the floor: {r}");
    handle.stop();
}

/// Unknown view flags are rejected rather than silently ignored.
#[test]
fn view_flag_rejects_unknown() {
    let (handle, port, _) = start();
    let resp = http_post(port, "/api/viewport/3d/view_flag", r#"{"flag":"bogus"}"#);
    assert!(resp.starts_with("HTTP/1.1 400"), "unknown flag should 400: {resp}");
    handle.stop();
}

/// A 3D scene with a mesh at the origin, so it is centred and clearly visible
/// in the default editor camera framing for the render-toggle tests.
fn build_centered_3d_scene() -> SceneTree {
    gdobject::class_db::register_3d_classes();
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let id = tree
        .add_child(root, Node::new("Crate", "MeshInstance3D"))
        .unwrap();
    node3d::set_position(&mut tree, id, gdcore::math::Vector3::new(0.0, 0.0, 0.0));
    tree
}

/// Count pixels that differ (beyond a small epsilon) between two frames.
fn count_diff(a: &gdrender2d::renderer::FrameBuffer, b: &gdrender2d::renderer::FrameBuffer) -> usize {
    a.pixels
        .iter()
        .zip(b.pixels.iter())
        .filter(|(p, q)| (p.r - q.r).abs() + (p.g - q.g).abs() + (p.b - q.b).abs() > 0.02)
        .count()
}

/// The renderer honors the Environment preview toggle: many background pixels
/// change between the sky-gradient frame and the flat-backdrop frame.
#[test]
fn environment_toggle_changes_background() {
    let tree = build_centered_3d_scene();
    let mut vp = Viewport3D::new(160, 120);

    vp.environment.enabled = true;
    vp.environment.sky_visible = true;
    let sky = render_editor_3d_frame(&vp, &tree);

    vp.environment.enabled = false;
    let flat = render_editor_3d_frame(&vp, &tree);

    // The sky fills most of the upper half of the frame, so the difference is
    // large — far more than incidental anti-aliasing noise.
    assert!(
        count_diff(&sky, &flat) > 100,
        "sky-on and sky-off backgrounds should differ across many pixels"
    );
}

/// The renderer honors the Sun preview toggle: turning the sun off dims the
/// directional light, changing the shaded mesh pixels.
#[test]
fn sun_toggle_changes_frame() {
    let tree = build_centered_3d_scene();
    let mut vp = Viewport3D::new(160, 120);
    // Flat backdrop both times so only the mesh lighting differs.
    vp.environment.enabled = false;

    vp.environment.sun_enabled = true;
    let lit = render_editor_3d_frame(&vp, &tree);
    vp.environment.sun_enabled = false;
    let unlit = render_editor_3d_frame(&vp, &tree);

    assert!(
        count_diff(&lit, &unlit) > 5,
        "sun on/off should change the shaded mesh pixels"
    );
}
