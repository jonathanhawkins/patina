//! App-level tests for pat-3dnav: the 3D spatial editor's camera navigation.
//!
//! Before this, the editor's 3D viewport rebuilt a fresh camera every frame, so
//! there were no endpoints to orbit/pan/zoom it and nothing to accumulate — the
//! "3D side" was a static, non-interactive render. These tests prove the server
//! now keeps a persistent 3D editor camera and that the orbit/pan/zoom/reset
//! endpoints mutate it (and persist across requests), matching Godot's 3D view.

use gdeditor::editor_server::{EditorServerHandle, EditorState};
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

fn build_scene() -> SceneTree {
    gdobject::class_db::register_3d_classes();
    gdobject::class_db::register_2d_classes();
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let crate_id = tree
        .add_child(root, Node::new("Crate", "MeshInstance3D"))
        .unwrap();
    node3d::set_position(&mut tree, crate_id, gdcore::math::Vector3::new(1.0, 0.5, -2.0));
    tree
}

fn http_request(port: u16, req: &str) -> String {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
    stream.write_all(req.as_bytes()).unwrap();
    let mut buf = Vec::new();
    let _ = stream.read_to_end(&mut buf);
    String::from_utf8_lossy(&buf).to_string()
}

fn http_get(port: u16, path: &str) -> String {
    http_request(
        port,
        &format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n"),
    )
}

fn http_post(port: u16, path: &str, body: &str) -> String {
    http_request(
        port,
        &format!(
            "POST {path} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        ),
    )
}

/// Extracts a numeric JSON field value (`"key":<number>`) from a response body.
fn field(resp: &str, key: &str) -> f64 {
    let needle = format!("\"{key}\":");
    let start = resp
        .find(&needle)
        .unwrap_or_else(|| panic!("field {key} missing in: {resp}"))
        + needle.len();
    let rest = &resp[start..];
    let end = rest
        .find(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-' || c == 'e' || c == 'E'))
        .unwrap_or(rest.len());
    rest[..end]
        .parse::<f64>()
        .unwrap_or_else(|_| panic!("field {key} not a number: {:?}", &rest[..end]))
}

fn start() -> (EditorServerHandle, u16) {
    let state = EditorState::new(build_scene());
    let port = free_port();
    let handle = EditorServerHandle::start(port, state);
    thread::sleep(Duration::from_millis(100));
    (handle, port)
}

/// The default 3D editor camera reports the expected 3/4 "floor" framing.
#[test]
fn editor_3d_default_camera_framing() {
    let (handle, port) = start();
    let s = http_get(port, "/api/viewport/3d/state");
    assert!((field(&s, "distance") - 14.0).abs() < 1e-3, "distance: {s}");
    assert!((field(&s, "yaw") - 0.6).abs() < 1e-3, "yaw: {s}");
    assert!((field(&s, "pitch") - (-0.5)).abs() < 1e-3, "pitch: {s}");
    handle.stop();
}

/// Orbiting rotates the camera (yaw + pitch) around the focus point.
#[test]
fn editor_3d_orbit_rotates_camera() {
    let (handle, port) = start();
    let before = http_get(port, "/api/viewport/3d/state");
    let resp = http_post(port, "/api/viewport/3d/orbit", r#"{"dx":60,"dy":25}"#);
    assert!(resp.starts_with("HTTP/1.1 200"), "{resp}");
    assert!(
        field(&resp, "yaw") > field(&before, "yaw"),
        "orbit dx>0 should increase yaw"
    );
    assert!(
        field(&resp, "pitch") > field(&before, "pitch"),
        "orbit dy>0 should increase pitch"
    );
    handle.stop();
}

/// Zooming in dollies the camera closer (smaller distance); out moves it back.
#[test]
fn editor_3d_zoom_changes_distance() {
    let (handle, port) = start();
    let zin = http_post(port, "/api/viewport/3d/zoom", r#"{"delta":1.0}"#);
    assert!(field(&zin, "distance") < 14.0, "zoom in should shrink distance: {zin}");
    let zout = http_post(port, "/api/viewport/3d/zoom", r#"{"delta":-3.0}"#);
    assert!(
        field(&zout, "distance") > field(&zin, "distance"),
        "zoom out should grow distance"
    );
    handle.stop();
}

/// Panning slides the focus point.
#[test]
fn editor_3d_pan_moves_focus() {
    let (handle, port) = start();
    let before = http_get(port, "/api/viewport/3d/state");
    let resp = http_post(port, "/api/viewport/3d/pan", r#"{"dx":40,"dy":15}"#);
    let moved = (field(&resp, "focus_x") - field(&before, "focus_x")).abs()
        + (field(&resp, "focus_y") - field(&before, "focus_y")).abs()
        + (field(&resp, "focus_z") - field(&before, "focus_z")).abs();
    assert!(moved > 1e-4, "pan should move the focus point: {resp}");
    handle.stop();
}

/// Godot's right-mouse free-look: `fly/begin` enters the mode, `fly/look` turns
/// the camera in place (changes yaw without orbiting), and `fly/move` flies it
/// (moves the focus point); `fly/end` exits.
#[test]
fn editor_3d_freelook_turns_and_flies() {
    let (handle, port) = start();
    assert!(http_post(port, "/api/viewport/3d/fly/begin", "{}").starts_with("HTTP/1.1 200"));

    let before = http_get(port, "/api/viewport/3d/state");
    let look = http_post(port, "/api/viewport/3d/fly/look", r#"{"dx":60,"dy":0}"#);
    assert!(
        (field(&look, "yaw") - field(&before, "yaw")).abs() > 1e-3,
        "fly/look should turn the camera (yaw): {look}"
    );

    let mid = http_get(port, "/api/viewport/3d/state");
    let mv = http_post(port, "/api/viewport/3d/fly/move", r#"{"x":0,"y":0,"z":1}"#);
    let moved = (field(&mv, "focus_x") - field(&mid, "focus_x")).abs()
        + (field(&mv, "focus_y") - field(&mid, "focus_y")).abs()
        + (field(&mv, "focus_z") - field(&mid, "focus_z")).abs();
    assert!(moved > 1e-4, "fly/move should fly the camera: {mv}");

    assert!(http_post(port, "/api/viewport/3d/fly/end", "{}").starts_with("HTTP/1.1 200"));
    handle.stop();
}

/// Navigation accumulates across separate HTTP requests (persistent camera).
#[test]
fn editor_3d_nav_persists_across_requests() {
    let (handle, port) = start();
    let a = http_post(port, "/api/viewport/3d/orbit", r#"{"dx":30,"dy":0}"#);
    let b = http_post(port, "/api/viewport/3d/orbit", r#"{"dx":30,"dy":0}"#);
    let yaw_a = field(&a, "yaw");
    let yaw_b = field(&b, "yaw");
    assert!(
        yaw_b > yaw_a,
        "second orbit must build on the first (persistent state): {yaw_a} !< {yaw_b}"
    );
    // And a read-only GET reflects the accumulated state.
    let s = http_get(port, "/api/viewport/3d/state");
    assert!((field(&s, "yaw") - yaw_b).abs() < 1e-3, "GET state should match last orbit: {s}");
    handle.stop();
}

/// Dragging the move gizmo's X axis translates the selected node along +X.
#[test]
fn editor_3d_move_gizmo_drags_node() {
    gdobject::class_db::register_3d_classes();
    gdobject::class_db::register_2d_classes();
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let n = tree
        .add_child(root, Node::new("Box", "MeshInstance3D"))
        .unwrap();
    node3d::set_position(&mut tree, n, gdcore::math::Vector3::ZERO);

    let state = EditorState::new(tree);
    let port = free_port();
    let handle = EditorServerHandle::start(port, state);
    thread::sleep(Duration::from_millis(100));

    // Select the node at the origin, then switch to the Move tool.
    let s = http_post(port, "/api/viewport/3d/select", r#"{"x":320,"y":240}"#);
    assert!(s.contains(&format!("\"selected\":{}", n.raw())), "{s}");
    http_post(port, "/api/viewport/3d/gizmo_mode", r#"{"mode":"move"}"#);

    // Project the X-axis arrow tip with a viewport matching the server's default,
    // press there to grab the X axis, then drag further along +X.
    let vp = gdeditor::editor_server::default_editor_viewport_3d();
    let len = gdeditor::viewport_3d_render::gizmo_arrow_length(&vp);
    let (tx, ty) = vp.world_to_screen(gdcore::math::Vector3::new(len, 0.0, 0.0));
    let begin = http_post(
        port,
        "/api/viewport/3d/gizmo/begin",
        &format!(r#"{{"x":{tx},"y":{ty}}}"#),
    );
    assert!(begin.contains("\"dragging\":true"), "should grab a gizmo axis: {begin}");
    assert!(begin.contains("\"axis\":\"x\""), "should grab the X axis: {begin}");

    let (tx2, ty2) = vp.world_to_screen(gdcore::math::Vector3::new(len * 2.5, 0.0, 0.0));
    let upd = http_post(
        port,
        "/api/viewport/3d/gizmo/update",
        &format!(r#"{{"x":{tx2},"y":{ty2}}}"#),
    );
    assert!(field(&upd, "x") > 0.5, "node should move along +X: {upd}");
    // The move stays on the grabbed axis (Y/Z barely change).
    assert!(field(&upd, "y").abs() < 0.5 && field(&upd, "z").abs() < 0.5, "move is axis-locked: {upd}");
    http_post(port, "/api/viewport/3d/gizmo/end", "");
    handle.stop();
}

/// Parses the `"selected_nodes":[..]` id array from a select response.
fn id_array(resp: &str) -> Vec<u64> {
    let needle = "\"selected_nodes\":[";
    let start = resp.find(needle).expect("selected_nodes present") + needle.len();
    let end = resp[start..].find(']').unwrap() + start;
    resp[start..end]
        .split(',')
        .filter(|s| !s.trim().is_empty())
        .map(|s| s.trim().parse().unwrap())
        .collect()
}

/// Shift/Ctrl-click adds (and toggles) nodes in the 3D selection.
#[test]
fn editor_3d_shift_click_multi_select() {
    gdobject::class_db::register_3d_classes();
    gdobject::class_db::register_2d_classes();
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let a = tree.add_child(root, Node::new("A", "MeshInstance3D")).unwrap();
    let b = tree.add_child(root, Node::new("B", "MeshInstance3D")).unwrap();
    node3d::set_position(&mut tree, a, gdcore::math::Vector3::ZERO);
    node3d::set_position(&mut tree, b, gdcore::math::Vector3::new(4.0, 0.0, 0.0));

    let state = EditorState::new(tree);
    let port = free_port();
    let handle = EditorServerHandle::start(port, state);
    thread::sleep(Duration::from_millis(100));

    let vp = gdeditor::editor_server::default_editor_viewport_3d();
    let (ax, ay) = vp.world_to_screen(gdcore::math::Vector3::ZERO);
    let (bx, by) = vp.world_to_screen(gdcore::math::Vector3::new(4.0, 0.0, 0.0));

    // Plain click selects A only.
    let r1 = http_post(port, "/api/viewport/3d/select", &format!(r#"{{"x":{ax},"y":{ay}}}"#));
    assert_eq!(id_array(&r1), vec![a.raw()], "plain click selects only A: {r1}");
    // Shift-add B → both selected.
    let r2 = http_post(
        port,
        "/api/viewport/3d/select",
        &format!(r#"{{"x":{bx},"y":{by},"add":true}}"#),
    );
    let mut got = id_array(&r2);
    got.sort_unstable();
    let mut want = vec![a.raw(), b.raw()];
    want.sort_unstable();
    assert_eq!(got, want, "shift-add selects both A and B: {r2}");
    // Add-click B again toggles it off → only A.
    let r3 = http_post(
        port,
        "/api/viewport/3d/select",
        &format!(r#"{{"x":{bx},"y":{by},"add":true}}"#),
    );
    assert_eq!(id_array(&r3), vec![a.raw()], "toggling B off leaves only A: {r3}");
    handle.stop();
}

/// Frame-selection (F) moves the camera focus to the selected node and pulls in.
#[test]
fn editor_3d_frame_focuses_selection() {
    gdobject::class_db::register_3d_classes();
    gdobject::class_db::register_2d_classes();
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let n = tree
        .add_child(root, Node::new("Box", "MeshInstance3D"))
        .unwrap();
    node3d::set_position(&mut tree, n, gdcore::math::Vector3::new(4.0, 0.0, 0.0));
    let state = EditorState::new(tree);
    let port = free_port();
    let handle = EditorServerHandle::start(port, state);
    thread::sleep(Duration::from_millis(100));

    let vp = gdeditor::editor_server::default_editor_viewport_3d();
    let (sx, sy) = vp.world_to_screen(gdcore::math::Vector3::new(4.0, 0.0, 0.0));
    let sel = http_post(
        port,
        "/api/viewport/3d/select",
        &format!(r#"{{"x":{sx},"y":{sy}}}"#),
    );
    assert!(sel.contains(&format!("\"selected\":{}", n.raw())), "select the node: {sel}");
    let f = http_post(port, "/api/viewport/3d/frame", "");
    assert!((field(&f, "focus_x") - 4.0).abs() < 0.1, "focus moves to the node: {f}");
    assert!((field(&f, "distance") - 5.0).abs() < 0.1, "distance set to a fit: {f}");
    handle.stop();
}

/// View presets snap the camera to top / front / right orientations.
#[test]
fn editor_3d_view_presets_snap_camera() {
    let (handle, port) = start();
    let top = http_post(port, "/api/viewport/3d/view_preset", r#"{"preset":"top"}"#);
    assert!(field(&top, "pitch") < -1.0, "top view looks down: {top}");
    let front = http_post(port, "/api/viewport/3d/view_preset", r#"{"preset":"front"}"#);
    assert!(
        field(&front, "yaw").abs() < 1e-3 && field(&front, "pitch").abs() < 1e-3,
        "front view is axis-aligned: {front}"
    );
    let right = http_post(port, "/api/viewport/3d/view_preset", r#"{"preset":"right"}"#);
    assert!(
        (field(&right, "yaw") - std::f64::consts::FRAC_PI_2).abs() < 1e-3,
        "right view yaw is 90°: {right}"
    );
    handle.stop();
}

/// Sets up a server with a single MeshInstance3D at the origin, selected, in the
/// given gizmo mode. Returns (handle, port, node-raw-id) and a viewport matching
/// the server's default camera for projecting gizmo handle positions.
fn start_gizmo(mode: &str) -> (EditorServerHandle, u16, u64, gdeditor::viewport_3d::Viewport3D) {
    gdobject::class_db::register_3d_classes();
    gdobject::class_db::register_2d_classes();
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let n = tree
        .add_child(root, Node::new("Box", "MeshInstance3D"))
        .unwrap();
    node3d::set_position(&mut tree, n, gdcore::math::Vector3::ZERO);
    let state = EditorState::new(tree);
    let port = free_port();
    let handle = EditorServerHandle::start(port, state);
    thread::sleep(Duration::from_millis(100));
    http_post(port, "/api/viewport/3d/select", r#"{"x":320,"y":240}"#);
    http_post(
        port,
        "/api/viewport/3d/gizmo_mode",
        &format!(r#"{{"mode":"{mode}"}}"#),
    );
    (handle, port, n.raw(), gdeditor::editor_server::default_editor_viewport_3d())
}

/// Moving a nested node's gizmo applies the parent transform exactly once: the
/// child's resulting WORLD position reflects the drag, not parent-doubled coords.
#[test]
fn editor_3d_gizmo_move_respects_parent_transform() {
    gdobject::class_db::register_3d_classes();
    gdobject::class_db::register_2d_classes();
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let parent = tree.add_child(root, Node::new("Parent", "Node3D")).unwrap();
    node3d::set_position(&mut tree, parent, gdcore::math::Vector3::new(5.0, 0.0, 0.0));
    let child = tree
        .add_child(parent, Node::new("Child", "MeshInstance3D"))
        .unwrap();
    // Local (0,2,0) under a parent at (5,0,0) → world (5,2,0).
    node3d::set_position(&mut tree, child, gdcore::math::Vector3::new(0.0, 2.0, 0.0));

    let state = EditorState::new(tree);
    let port = free_port();
    let handle = EditorServerHandle::start(port, state);
    thread::sleep(Duration::from_millis(100));

    let vp = gdeditor::editor_server::default_editor_viewport_3d();
    let cw = gdcore::math::Vector3::new(5.0, 2.0, 0.0);
    let (sx, sy) = vp.world_to_screen(cw);
    let sel = http_post(port, "/api/viewport/3d/select", &format!(r#"{{"x":{sx},"y":{sy}}}"#));
    assert!(sel.contains(&format!("\"selected\":{}", child.raw())), "select child: {sel}");
    http_post(port, "/api/viewport/3d/gizmo_mode", r#"{"mode":"move"}"#);

    let len = gdeditor::viewport_3d_render::gizmo_arrow_length(&vp);
    let (bx, by) = vp.world_to_screen(gdcore::math::Vector3::new(cw.x + len, cw.y, cw.z));
    let begin = http_post(
        port,
        "/api/viewport/3d/gizmo/begin",
        &format!(r#"{{"x":{bx},"y":{by}}}"#),
    );
    assert!(begin.contains("\"axis\":\"x\""), "grab X axis: {begin}");
    let (ux, uy) = vp.world_to_screen(gdcore::math::Vector3::new(cw.x + len + 2.0, cw.y, cw.z));
    http_post(
        port,
        "/api/viewport/3d/gizmo/update",
        &format!(r#"{{"x":{ux},"y":{uy}}}"#),
    );
    http_post(port, "/api/viewport/3d/gizmo/end", "");

    // World X should land around 7 (5 + ~2), NOT ~12 (the parent-doubled bug).
    let world = {
        let s = handle.state().lock().unwrap();
        gdscene::node3d::get_global_transform(&s.scene_tree, child).origin
    };
    assert!(
        world.x > 6.0 && world.x < 8.5,
        "child world X should move ~+2 to ~7, not double the parent: {world:?}"
    );
    assert!(
        (world.y - 2.0).abs() < 0.6 && world.z.abs() < 0.6,
        "other axes stay put: {world:?}"
    );
    handle.stop();
}

/// With snap on (Ctrl-drag), a move-gizmo translation lands on whole-unit steps.
#[test]
fn editor_3d_gizmo_snap_rounds_translation() {
    let (handle, port, _n, vp) = start_gizmo("move");
    let len = gdeditor::viewport_3d_render::gizmo_arrow_length(&vp);
    let (tx, ty) = vp.world_to_screen(gdcore::math::Vector3::new(len, 0.0, 0.0));
    let begin = http_post(
        port,
        "/api/viewport/3d/gizmo/begin",
        &format!(r#"{{"x":{tx},"y":{ty},"snap":true}}"#),
    );
    assert!(begin.contains("\"dragging\":true"), "should grab the X axis: {begin}");
    let (tx2, ty2) = vp.world_to_screen(gdcore::math::Vector3::new(len + 2.7, 0.0, 0.0));
    let upd = http_post(
        port,
        "/api/viewport/3d/gizmo/update",
        &format!(r#"{{"x":{tx2},"y":{ty2}}}"#),
    );
    let x = field(&upd, "x");
    assert!((x - x.round()).abs() < 1e-3, "snapped X should be a whole unit, got {x}: {upd}");
    assert!(x >= 1.0, "should have moved at least one unit: {upd}");
    http_post(port, "/api/viewport/3d/gizmo/end", "");
    handle.stop();
}

/// Dragging the move gizmo's XY plane handle translates the node within that
/// plane (X and Y move; Z stays put).
#[test]
fn editor_3d_move_gizmo_plane_drag() {
    let (handle, port, _n, vp) = start_gizmo("move");
    let len = gdeditor::viewport_3d_render::gizmo_arrow_length(&vp);
    let pf = len * 0.3;
    let (px, py) = vp.world_to_screen(gdcore::math::Vector3::new(pf, pf, 0.0));
    let begin = http_post(
        port,
        "/api/viewport/3d/gizmo/begin",
        &format!(r#"{{"x":{px},"y":{py}}}"#),
    );
    assert!(begin.contains("\"dragging\":true"), "should grab a plane handle: {begin}");
    assert!(begin.contains("\"axis\":\"xy\""), "should grab the XY plane: {begin}");
    let (px2, py2) = vp.world_to_screen(gdcore::math::Vector3::new(pf + 2.0, pf, 0.0));
    let upd = http_post(
        port,
        "/api/viewport/3d/gizmo/update",
        &format!(r#"{{"x":{px2},"y":{py2}}}"#),
    );
    assert!(field(&upd, "x") > 1.0, "node should move in X within the plane: {upd}");
    assert!(field(&upd, "z").abs() < 0.5, "plane drag keeps Z fixed: {upd}");
    http_post(port, "/api/viewport/3d/gizmo/end", "");
    handle.stop();
}

/// Dragging a rotate ring rotates the selected node (the update applies a
/// non-trivial rotation result).
#[test]
fn editor_3d_rotate_gizmo_rotates_node() {
    let (handle, port, _n, vp) = start_gizmo("rotate");
    let len = gdeditor::viewport_3d_render::gizmo_arrow_length(&vp);
    // A point only on the Y ring (XZ plane) at 45°.
    let p = gdcore::math::Vector3::new(len * 0.707, 0.0, len * 0.707);
    let (px, py) = vp.world_to_screen(p);
    let begin = http_post(
        port,
        "/api/viewport/3d/gizmo/begin",
        &format!(r#"{{"x":{px},"y":{py}}}"#),
    );
    assert!(begin.contains("\"dragging\":true"), "should grab a rotate ring: {begin}");
    // Drag to a different screen angle around the center → non-zero rotation.
    let upd = http_post(port, "/api/viewport/3d/gizmo/update", r#"{"x":320,"y":120}"#);
    assert!(upd.contains("\"angle\""), "rotate update should apply an angle: {upd}");
    http_post(port, "/api/viewport/3d/gizmo/end", "");
    handle.stop();
}

/// Dragging a scale handle scales the selected node (factor grows as the cursor
/// moves away from the gizmo center).
#[test]
fn editor_3d_scale_gizmo_scales_node() {
    let (handle, port, _n, vp) = start_gizmo("scale");
    let len = gdeditor::viewport_3d_render::gizmo_arrow_length(&vp);
    let (px, py) = vp.world_to_screen(gdcore::math::Vector3::new(len, 0.0, 0.0));
    let begin = http_post(
        port,
        "/api/viewport/3d/gizmo/begin",
        &format!(r#"{{"x":{px},"y":{py}}}"#),
    );
    assert!(begin.contains("\"dragging\":true"), "should grab a scale handle: {begin}");
    // Drag further out along the X handle → factor > 1.
    let (px2, py2) = vp.world_to_screen(gdcore::math::Vector3::new(len * 2.0, 0.0, 0.0));
    let upd = http_post(
        port,
        "/api/viewport/3d/gizmo/update",
        &format!(r#"{{"x":{px2},"y":{py2}}}"#),
    );
    assert!(field(&upd, "factor") > 1.0, "scale update should grow the factor: {upd}");
    http_post(port, "/api/viewport/3d/gizmo/end", "");
    handle.stop();
}

/// A click in the 3D viewport raycast-selects the Node3D under the cursor and
/// makes it the editor's selection; a click on empty space clears it.
#[test]
fn editor_3d_click_selects_node3d() {
    gdobject::class_db::register_3d_classes();
    gdobject::class_db::register_2d_classes();
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let n = tree
        .add_child(root, Node::new("Center", "MeshInstance3D"))
        .unwrap();
    // At the origin = the camera's focus, so it projects to the screen center.
    node3d::set_position(&mut tree, n, gdcore::math::Vector3::ZERO);

    let state = EditorState::new(tree);
    let port = free_port();
    let handle = EditorServerHandle::start(port, state);
    thread::sleep(Duration::from_millis(100));

    // Default 640x480 viewport with focus at the origin → origin is at (320,240).
    let hit = http_post(port, "/api/viewport/3d/select", r#"{"x":320,"y":240}"#);
    assert!(hit.starts_with("HTTP/1.1 200"), "{hit}");
    assert!(
        hit.contains(&format!("\"selected\":{}", n.raw())),
        "click at screen center should select the origin node: {hit}"
    );

    // A click in the far corner hits nothing and clears the selection.
    let miss = http_post(port, "/api/viewport/3d/select", r#"{"x":3,"y":3}"#);
    assert!(
        miss.contains("\"selected\":null"),
        "corner click should select nothing: {miss}"
    );
    handle.stop();
}

/// Parses a `"key":[a,b,c]` numeric array from a response body.
fn vec3_field(resp: &str, key: &str) -> [f64; 3] {
    let needle = format!("\"{key}\":[");
    let start = resp
        .find(&needle)
        .unwrap_or_else(|| panic!("array {key} missing in: {resp}"))
        + needle.len();
    let end = resp[start..].find(']').unwrap() + start;
    let nums: Vec<f64> = resp[start..end]
        .split(',')
        .map(|s| s.trim().parse::<f64>().unwrap())
        .collect();
    [nums[0], nums[1], nums[2]]
}

/// The 3D state exposes the camera's view basis (world axes in view space) so the
/// frontend can draw the navigation gizmo. Each basis vector must be unit length.
#[test]
fn editor_3d_state_has_orthonormal_view_basis() {
    let (handle, port) = start();
    let s = http_get(port, "/api/viewport/3d/state");
    for key in ["bx", "by", "bz"] {
        let v = vec3_field(&s, key);
        let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
        assert!((len - 1.0).abs() < 1e-3, "{key} not unit length ({len}): {s}");
    }
    handle.stop();
}

/// Reset restores the default framing after the camera has been moved.
#[test]
fn editor_3d_reset_restores_default() {
    let (handle, port) = start();
    http_post(port, "/api/viewport/3d/orbit", r#"{"dx":200,"dy":80}"#);
    http_post(port, "/api/viewport/3d/zoom", r#"{"delta":2.0}"#);
    let r = http_post(port, "/api/viewport/3d/reset", "");
    assert!((field(&r, "distance") - 14.0).abs() < 1e-3, "reset distance: {r}");
    assert!((field(&r, "yaw") - 0.6).abs() < 1e-3, "reset yaw: {r}");
    assert!((field(&r, "pitch") - (-0.5)).abs() < 1e-3, "reset pitch: {r}");
    handle.stop();
}

/// Godot parity: `/api/scene` recommends which main-screen editor to open for a
/// scene, mirroring Godot selecting the main plugin whose `handles()` matches the
/// scene root — `Node3DEditorPlugin` handles `Node3D`, `CanvasItemEditorPlugin`
/// handles `CanvasItem`. This is what lands a freshly opened 3D scene in the
/// spatial editor (where right-drag freelook / middle-orbit / wheel-zoom apply)
/// instead of leaving the user on the 2D canvas. Covers a 3D root, a 2D root, and
/// a bare-`Node` root that defers to its dominant descendant type.
#[test]
fn editor_scene_recommends_matching_main_view() {
    gdobject::class_db::register_3d_classes();
    gdobject::class_db::register_2d_classes();

    // (a) 3D-rooted scene (MeshInstance3D under root) → recommends "3d".
    let (h3, p3) = start();
    let s3 = http_get(p3, "/api/scene");
    assert!(
        s3.contains("\"recommended_view\":\"3d\""),
        "3D-rooted scene should recommend the 3D editor: {s3}"
    );
    h3.stop();

    // (b) 2D-rooted scene (Node2D — a CanvasItem — under root) → recommends "2d".
    let mut t2 = SceneTree::new();
    let r2 = t2.root_id();
    t2.add_child(r2, Node::new("Sprite", "Node2D")).unwrap();
    let p2 = free_port();
    let h2 = EditorServerHandle::start(p2, EditorState::new(t2));
    thread::sleep(Duration::from_millis(100));
    let s2 = http_get(p2, "/api/scene");
    assert!(
        s2.contains("\"recommended_view\":\"2d\""),
        "2D-rooted scene should recommend the 2D editor: {s2}"
    );
    h2.stop();

    // (c) Bare `Node` root with a 3D child → no clear root type, so it falls back
    // to the dominant descendant type (Godot's `_find_node_types` counting) → "3d".
    let mut tn = SceneTree::new();
    let rn = tn.root_id();
    let level = tn.add_child(rn, Node::new("Level", "Node")).unwrap();
    tn.add_child(level, Node::new("Box", "MeshInstance3D")).unwrap();
    let pn = free_port();
    let hn = EditorServerHandle::start(pn, EditorState::new(tn));
    thread::sleep(Duration::from_millis(100));
    let sn = http_get(pn, "/api/scene");
    assert!(
        sn.contains("\"recommended_view\":\"3d\""),
        "bare-Node root with a 3D child should recommend the 3D editor: {sn}"
    );
    hn.stop();
}

/// Godot parity for free-look (right-mouse) flight speed:
/// - the mouse wheel scales `fly_speed` while free-look is active (`fly/speed`),
/// - a `fly/move` carries a per-event `speed` multiplier so Shift flies ×3 faster
///   and Alt ×0.333 slower,
/// - a degenerate scale factor can't stick the speed at zero (it resets).
#[test]
fn editor_3d_freelook_speed_controls() {
    // Distance the focus point travelled between two `/state` snapshots.
    fn moved(a: &str, b: &str) -> f64 {
        let dx = field(b, "focus_x") - field(a, "focus_x");
        let dy = field(b, "focus_y") - field(a, "focus_y");
        let dz = field(b, "focus_z") - field(a, "focus_z");
        (dx * dx + dy * dy + dz * dz).sqrt()
    }

    let (handle, port) = start();
    http_post(port, "/api/viewport/3d/fly/begin", "{}");

    // Wheel up (factor > 1) speeds up free-look; wheel down (factor < 1) slows it.
    let base = field(&http_get(port, "/api/viewport/3d/state"), "fly_speed");
    let faster = http_post(port, "/api/viewport/3d/fly/speed", r#"{"factor":1.08}"#);
    assert!(field(&faster, "fly_speed") > base, "wheel-up should speed up freelook: {faster}");
    let slower = http_post(port, "/api/viewport/3d/fly/speed", r#"{"factor":0.5}"#);
    assert!(
        field(&slower, "fly_speed") < field(&faster, "fly_speed"),
        "wheel-down should slow freelook back down: {slower}"
    );

    // At a fixed speed, a Shift-boosted move (×3) travels much farther than ×1.
    let p0 = http_get(port, "/api/viewport/3d/state");
    let plain = http_post(port, "/api/viewport/3d/fly/move", r#"{"x":0,"y":0,"z":1,"speed":1.0}"#);
    let d_plain = moved(&p0, &plain);
    let p1 = http_get(port, "/api/viewport/3d/state");
    let boosted = http_post(port, "/api/viewport/3d/fly/move", r#"{"x":0,"y":0,"z":1,"speed":3.0}"#);
    let d_boost = moved(&p1, &boosted);
    assert!(
        d_boost > d_plain * 2.0,
        "Shift (×3) should fly much farther than ×1: {d_plain} vs {d_boost}"
    );

    // A zero factor would otherwise stick the speed at 0; it resets to default.
    let reset = http_post(port, "/api/viewport/3d/fly/speed", r#"{"factor":0.0}"#);
    assert!(
        (field(&reset, "fly_speed") - 5.0).abs() < 1e-3,
        "a degenerate (zero) factor resets fly speed to the default: {reset}"
    );
    handle.stop();
}
