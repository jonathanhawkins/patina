//! Acceptance test for pat-s3rrv.10: per-node 3D gizmo handles reach the
//! **served** editor.
//!
//! Godot draws a per-node gizmo on the selected 3D node and dragging one of its
//! handles edits the node's SHAPE rather than its transform
//! (`editor/scene/3d/gizmos/collision_shape_3d_gizmo_plugin.cpp` resizes a
//! `BoxShape3D` through `Gizmo3DHelper`; `light_3d_gizmo_plugin.cpp` drags an
//! omni range).
//!
//! Patina had the parity-exact math and no way to reach it. `viewport_3d_handles.rs`
//! (`box_get_handles` / `box_set_handle` / `box_handle_drag_commands`) had ZERO
//! non-test callers, the only handle endpoint (`/api/viewport/shape_handles`)
//! hard-returned `editable:false` for anything but `CollisionShape2D`, and no
//! `/api/viewport/3d/handle*` route existed at all. The previous test named
//! `viewport3d_gizmo_handles_e2e` calls the module directly and never opens a
//! socket, so it stayed green while the served editor had no handles whatsoever.
//!
//! This test drives the real `editor_server` over TCP, exactly like the other
//! `_e2e` suites, and asserts the whole chain:
//!
//! ```text
//! GET /api/viewport/3d/handles -> screen-projected handle dots
//!   -> POST handle/begin (hit) -> handle/update (drag) -> handle/end (undo step)
//!   -> __shape__size + re-centred position -> /api/undo restores -> /api/scene/save persists
//! ```

use gdcore::math::Vector3;
use gdeditor::editor_server::{EditorServerHandle, EditorState};
use gdscene::SceneTree;
use gdvariant::Variant;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;
use std::time::Duration;

// ---------------------------------------------------------------------------
// HTTP harness (mirrors editor_undo_coverage_e2e.rs)
// ---------------------------------------------------------------------------

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn connect_with_retry(port: u16) -> TcpStream {
    for attempt in 0..20 {
        match TcpStream::connect(format!("127.0.0.1:{port}")) {
            Ok(s) => return s,
            Err(_) if attempt < 19 => thread::sleep(Duration::from_millis(50)),
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

fn body_of(resp: &str) -> &str {
    resp.split("\r\n\r\n").nth(1).unwrap_or("")
}

fn json_of(resp: &str) -> serde_json::Value {
    serde_json::from_str(body_of(resp))
        .unwrap_or_else(|e| panic!("response body is not JSON ({e}): {resp}"))
}

// ---------------------------------------------------------------------------
// Scene
// ---------------------------------------------------------------------------

/// The scene the bead names, as real `.tscn` text: a `CollisionShape3D` holding
/// a `BoxShape3D` of size (2,2,2) at the origin, plus an `OmniLight3D`.
///
/// Parsed rather than hand-built on purpose. The loader flattens an inline
/// sub-resource onto the node as `__shape__class` / `__shape__size` **and**
/// records `__resref__shape = "BoxShape3D::Box_1"`; the saver keys the
/// `[sub_resource]` body it rebuilds off that `__resref__` companion
/// (`scene_saver.rs::collect_sub_resource_bodies`). A node assembled by hand
/// with only the `__shape__*` cache would resize fine and then save nothing,
/// so the persistence clause below would pass or fail for the wrong reason.
const SCENE_TSCN: &str = r#"[gd_scene load_steps=2 format=3]

[sub_resource type="BoxShape3D" id="Box_1"]
size = Vector3(2, 2, 2)

[node name="Main" type="Node3D"]

[node name="Box" type="CollisionShape3D" parent="."]
shape = SubResource("Box_1")

[node name="Omni" type="OmniLight3D" parent="."]
omni_range = 5.0
"#;

/// Boots the served editor on the parsed scene. Returns the handle (kept alive
/// by the caller), the port, and the raw ids of Main / Box / Omni.
fn server() -> (EditorServerHandle, u16, u64, u64, u64) {
    let scene = gdscene::packed_scene::PackedScene::from_tscn(SCENE_TSCN).expect("parse tscn");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let main = gdscene::packed_scene::add_packed_scene_to_tree(&mut tree, root, &scene)
        .expect("instance scene");

    let child = |tree: &SceneTree, name: &str| -> u64 {
        tree.get_node(main)
            .expect("scene root")
            .children()
            .iter()
            .copied()
            .find(|&c| tree.get_node(c).map(|n| n.name() == name).unwrap_or(false))
            .unwrap_or_else(|| panic!("scene should contain a node named {name}"))
            .raw()
    };
    let raw_shape = child(&tree, "Box");
    let raw_light = child(&tree, "Omni");
    let raw_main = main.raw();

    // The box must have arrived with its geometry flattened, or every assertion
    // below would be measuring a default rather than the authored scene.
    assert_eq!(
        tree.get_node(
            tree.get_node(main)
                .unwrap()
                .children()
                .iter()
                .copied()
                .find(|&c| tree.get_node(c).map(|n| n.name() == "Box").unwrap_or(false))
                .unwrap()
        )
        .unwrap()
        .get_property("__shape__size"),
        Variant::Vector3(Vector3::new(2.0, 2.0, 2.0)),
        "the loader must flatten the BoxShape3D sub-resource onto the node",
    );

    let port = free_port();
    let handle = EditorServerHandle::start(port, EditorState::new(tree));
    thread::sleep(Duration::from_millis(100));
    (handle, port, raw_main, raw_shape, raw_light)
}

fn select(port: u16, node: u64) {
    let resp = http_post(
        port,
        "/api/node/select",
        &format!(r#"{{"node_id":{node}}}"#),
    );
    assert!(resp.contains("200 OK"), "select failed: {resp}");
}

/// The node's current `__shape__size` and `position`, read back over HTTP so the
/// assertions see what the served editor actually holds.
///
/// `/api/selected` returns `properties` as an ARRAY of inspector entries, each
/// wrapping its value as `{"type":"Vector3","value":[x,y,z]}` — hence the
/// find-by-name and the double `value` hop.
fn shape_state(port: u16, node: u64) -> (Vec<f64>, Vec<f64>) {
    let sel = json_of(&http_get(port, "/api/selected"));
    let props = sel["properties"]
        .as_array()
        .unwrap_or_else(|| panic!("/api/selected should list properties for {node}: {sel}"))
        .clone();
    let entry = |key: &str| -> serde_json::Value {
        props
            .iter()
            .find(|p| p["name"].as_str() == Some(key))
            .unwrap_or_else(|| panic!("{key} should be an inspector property on {node}: {sel}"))
            .clone()
    };
    let components = |v: &serde_json::Value| -> Option<Vec<f64>> {
        Some(
            v["value"]["value"]
                .as_array()?
                .iter()
                .map(|c| c.as_f64().unwrap_or(f64::NAN))
                .collect(),
        )
    };
    let size_entry = entry("__shape__size");
    let size = components(&size_entry)
        .unwrap_or_else(|| panic!("__shape__size should be a Vector3: {size_entry}"));
    // An unset `position` reports as the class default (or Nil once undo has
    // restored "no stored value"), and both mean the origin.
    let pos = components(&entry("position")).unwrap_or_else(|| vec![0.0, 0.0, 0.0]);
    (size, pos)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// **The acceptance test.** Selecting the `CollisionShape3D` in the served
/// editor exposes draggable handles; dragging the +X face grows the box and
/// re-centres the node by exactly half the growth (Godot's `box_set_handle`);
/// `/api/undo` restores the original size and position; and saving writes the
/// new size into the sub-resource.
#[test]
fn editor_3d_gizmo_handles_served() {
    let (_h, port, _main, box_node, _light) = server();
    select(port, box_node);

    // (a) The served editor offers handles for this node at all — the state
    //     before this bead was `editable:false` for every 3D class.
    let handles = json_of(&http_get(
        port,
        &format!("/api/viewport/3d/handles?node_id={box_node}"),
    ));
    assert_eq!(
        handles["editable"], true,
        "a CollisionShape3D holding a BoxShape3D must expose handles: {handles}"
    );
    let list = handles["handles"]
        .as_array()
        .expect("handles array")
        .clone();
    assert_eq!(
        list.len(),
        6,
        "a BoxShape3D has one handle per face (Godot's box_get_handles): {handles}"
    );

    // The +X face is handle id 0 and -X is id 1 (Godot's id packing). The screen
    // vector between them IS the +X direction in the current camera's view, so
    // the drag below is camera-agnostic: no test-side projection math to drift.
    let hx = |i: usize, k: &str| list[i][k].as_f64().expect("handle coord") as f32;
    let (h0x, h0y) = (hx(0, "x"), hx(0, "y"));
    let (h1x, h1y) = (hx(1, "x"), hx(1, "y"));
    let (dx, dy) = (h0x - h1x, h0y - h1y);
    let len = (dx * dx + dy * dy).sqrt();
    assert!(
        len > 1.0,
        "the +X and -X handles must project to distinct pixels, got \
         ({h0x},{h0y}) and ({h1x},{h1y})"
    );
    // Drag well past the +X face along that screen direction.
    let (tx, ty) = (h0x + dx / len * 60.0, h0y + dy / len * 60.0);

    // (b) Grab the +X handle where the server said it is.
    let began = json_of(&http_post(
        port,
        "/api/viewport/3d/handle/begin",
        &format!(r#"{{"node_id":{box_node},"handle":0,"x":{h0x},"y":{h0y}}}"#),
    ));
    assert_eq!(
        began["dragging"], true,
        "begin must grab handle 0 at its own projected pixel: {began}"
    );

    // (c) Drag, then commit.
    let resp = http_post(
        port,
        "/api/viewport/3d/handle/update",
        &format!(r#"{{"x":{tx},"y":{ty}}}"#),
    );
    assert!(resp.contains("200 OK"), "handle/update failed: {resp}");
    let resp = http_post(port, "/api/viewport/3d/handle/end", "{}");
    assert!(resp.contains("200 OK"), "handle/end failed: {resp}");

    let (size, pos) = shape_state(port, box_node);
    assert!(
        size[0] > 2.0,
        "dragging the +X face outward must grow the box on X: size={size:?}"
    );
    assert!(
        (size[1] - 2.0).abs() < 1e-3 && (size[2] - 2.0).abs() < 1e-3,
        "only the dragged axis may change: size={size:?}"
    );
    // Godot's non-symmetric resize moves ONE face and re-centres the node so the
    // opposite face stays put: the origin shifts by exactly half the growth.
    // This pins the actual formula rather than merely "something got bigger".
    let expected_shift = (size[0] - 2.0) / 2.0;
    assert!(
        (pos[0] - expected_shift).abs() < 1e-3,
        "the node must re-centre by half the growth (expected x={expected_shift}, \
         got {pos:?} for size {size:?}) — the -X face has to stay where it was"
    );

    // (d) One drag is one undo step, and it restores the pre-drag state.
    let resp = http_post(port, "/api/undo", "{}");
    assert!(resp.contains("200 OK"), "undo failed: {resp}");
    let (size, pos) = shape_state(port, box_node);
    assert!(
        (size[0] - 2.0).abs() < 1e-3,
        "undo must restore the original size 2, got {size:?}"
    );
    assert!(
        pos[0].abs() < 1e-3,
        "undo must restore the original position, got {pos:?}"
    );

    // (e) Redo the drag and save: the new size reaches the .tscn sub-resource.
    let resp = http_post(port, "/api/redo", "{}");
    assert!(resp.contains("200 OK"), "redo failed: {resp}");
    let (size, _) = shape_state(port, box_node);
    let tmp = std::env::temp_dir().join(format!("patina_gizmo3d_{}.tscn", std::process::id()));
    let path = tmp.to_str().unwrap().to_string();
    let resp = http_post(port, "/api/scene/save", &format!(r#"{{"path":"{path}"}}"#));
    assert!(resp.contains("200 OK"), "save failed: {resp}");
    let text = std::fs::read_to_string(&tmp).expect("saved scene");
    assert!(
        text.contains("[sub_resource type=\"BoxShape3D\""),
        "the saved scene must still carry the box sub-resource:\n{text}"
    );
    // Assert by ROUND TRIP rather than by matching the float's text: the saver's
    // number formatting is not part of this bead's contract, but "reload the
    // saved scene and get the dragged size back" is.
    let reloaded =
        gdscene::packed_scene::PackedScene::from_tscn(&text).expect("re-parse the saved scene");
    let mut tree2 = SceneTree::new();
    let root2 = tree2.root_id();
    let main2 = gdscene::packed_scene::add_packed_scene_to_tree(&mut tree2, root2, &reloaded)
        .expect("re-instance the saved scene");
    let box2 = tree2
        .get_node(main2)
        .expect("saved scene root")
        .children()
        .iter()
        .copied()
        .find(|&c| {
            tree2
                .get_node(c)
                .map(|n| n.name() == "Box")
                .unwrap_or(false)
        })
        .expect("saved scene keeps the Box node");
    match tree2
        .get_node(box2)
        .expect("box node")
        .get_property("__shape__size")
    {
        Variant::Vector3(v) => assert!(
            (f64::from(v.x) - size[0]).abs() < 1e-3,
            "the dragged size {} must survive the save/reload round trip, got {}:\n{text}",
            size[0],
            v.x,
        ),
        other => {
            panic!("the reloaded box must carry its size sub-resource, got {other:?}:\n{text}")
        }
    }
    std::fs::remove_file(&tmp).ok();
}

/// Alt (symmetric resize) is the other half of Godot's box handle behavior: the
/// box grows about its own centre and the node does NOT move.
#[test]
fn editor_3d_box_handle_symmetric_drag_keeps_the_node_put() {
    let (_h, port, _main, box_node, _light) = server();
    select(port, box_node);

    let handles = json_of(&http_get(
        port,
        &format!("/api/viewport/3d/handles?node_id={box_node}"),
    ));
    let list = handles["handles"].as_array().expect("handles").clone();
    let hx = |i: usize, k: &str| list[i][k].as_f64().expect("coord") as f32;
    let (h0x, h0y) = (hx(0, "x"), hx(0, "y"));
    let (dx, dy) = (h0x - hx(1, "x"), h0y - hx(1, "y"));
    let len = (dx * dx + dy * dy).sqrt();

    http_post(
        port,
        "/api/viewport/3d/handle/begin",
        &format!(r#"{{"node_id":{box_node},"handle":0,"x":{h0x},"y":{h0y}}}"#),
    );
    http_post(
        port,
        "/api/viewport/3d/handle/update",
        &format!(
            r#"{{"x":{},"y":{},"symmetric":true}}"#,
            h0x + dx / len * 60.0,
            h0y + dy / len * 60.0
        ),
    );
    http_post(port, "/api/viewport/3d/handle/end", "{}");

    let (size, pos) = shape_state(port, box_node);
    assert!(
        size[0] > 2.0,
        "a symmetric drag still resizes: size={size:?}"
    );
    assert!(
        pos[0].abs() < 1e-3,
        "a symmetric (Alt) resize grows about the centre, so the node must not \
         move: got {pos:?}"
    );
}

/// An `OmniLight3D` exposes its range handle — the second node class the bead
/// names, proving the route is not box-only.
#[test]
fn editor_3d_omni_light_exposes_a_range_handle() {
    let (_h, port, _main, _box_node, light) = server();
    select(port, light);

    let handles = json_of(&http_get(
        port,
        &format!("/api/viewport/3d/handles?node_id={light}"),
    ));
    assert_eq!(
        handles["editable"], true,
        "an OmniLight3D must expose its range handle: {handles}"
    );
    let list = handles["handles"].as_array().expect("handles array");
    assert_eq!(list.len(), 1, "omni range is a single handle: {handles}");
    assert_eq!(
        list[0]["name"].as_str(),
        Some("range"),
        "the handle names the property it edits: {handles}"
    );
}

/// A class with no node gizmo answers cleanly rather than erroring, the way the
/// 2D route reports `editable:false`.
#[test]
fn editor_3d_handles_report_not_editable_for_a_plain_node() {
    let (_h, port, main, _box_node, _light) = server();
    // "Main" is a plain Node3D with no shape and no light.
    let handles = json_of(&http_get(
        port,
        &format!("/api/viewport/3d/handles?node_id={main}"),
    ));
    assert_eq!(
        handles["editable"], false,
        "a plain Node3D has no node gizmo: {handles}"
    );
    assert_eq!(
        handles["handles"].as_array().map(|a| a.len()),
        Some(0),
        "and no handles: {handles}"
    );
}

/// **The wiring clause.** A server route with no client caller does not count:
/// the served HTML must actually fetch the handle endpoints.
#[test]
fn editor_html_fetches_the_3d_handle_endpoints() {
    let html = gdeditor::editor_ui::EDITOR_HTML;
    assert!(
        html.contains("/api/viewport/3d/handles"),
        "EDITOR_HTML must fetch the handle list for the 3D selection"
    );
    assert!(
        html.contains("/api/viewport/3d/handle/begin"),
        "EDITOR_HTML must post the handle drag begin"
    );
    assert!(
        html.contains("/api/viewport/3d/handle/update"),
        "EDITOR_HTML must post the handle drag update"
    );
    assert!(
        html.contains("/api/viewport/3d/handle/end"),
        "EDITOR_HTML must post the handle drag end"
    );
}
