//! pat-0q2kx.15 acceptance: 2D viewport drags go through the **active tool**.
//!
//! Godot ground truth: `editor/scene/canvas_item_editor_plugin.cpp` — the canvas
//! editor applies the selected tool during a drag (TOOL_ROTATE orbits the pivot,
//! TOOL_SCALE scales, Shift constrains to an axis, and dragging one node of a
//! multi-selection moves them all).
//!
//! Patina's server already had `rotate_node` / `scale_node` / `drag_axis` /
//! `drag_multi` with tests, but the **client was hardcoded to move**: every 2D
//! mousedown/mousemove/mouseup posted `/api/viewport/drag_start|drag|drag_end`
//! regardless of `currentToolMode`, so rotate and scale were unreachable from
//! the UI and a multi-selection drag moved only one node. This test covers both
//! halves of that gap: the served HTML must *call* the tool routes, and the
//! routes must *do* the right thing to the scene.
//!
//! The HTTP harness mirrors `editor_undo_coverage_e2e.rs`.

use gdcore::math::Vector2;
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

fn find_id(v: &serde_json::Value, name: &str) -> Option<u64> {
    if v["name"].as_str() == Some(name) {
        return v["id"].as_u64();
    }
    for c in v["children"].as_array()?.iter() {
        if let Some(id) = find_id(c, name) {
            return Some(id);
        }
    }
    None
}

fn node_id(port: u16, name: &str) -> u64 {
    let scene = json_of(&http_get(port, "/api/scene"));
    find_id(&scene["nodes"], name)
        .unwrap_or_else(|| panic!("node '{name}' not found in {}", scene["nodes"]))
}

fn node_json(port: u16, id: u64) -> serde_json::Value {
    json_of(&http_get(port, &format!("/api/node/{id}")))
}

/// The inner value of one inspector property.
///
/// ⚠️ `/api/node/<id>` returns `properties` as an **array of entries**, not a map
/// keyed by name — each entry is
/// `{name, type, type_name, value: {type, value}, hint, hint_string, is_default}`
/// (`property_entry`, editor_server.rs:3276). So a property is found by scanning
/// for `name`, and the payload is one level deeper, under `value.value`.
fn prop_value(port: u16, id: u64, prop: &str) -> serde_json::Value {
    let n = node_json(port, id);
    let props: &Vec<serde_json::Value> = match n["properties"].as_array() {
        Some(a) => a,
        None => panic!("node {id} has no properties array"),
    };
    for entry in props {
        if entry["name"].as_str() == Some(prop) {
            return entry["value"]["value"].clone();
        }
    }
    panic!("node {id} has no property '{prop}'");
}

/// A scalar property (`rotation` — radians, despite the inspector's
/// `radians_as_degrees` display hint).
fn prop_f64(port: u16, id: u64, prop: &str) -> f64 {
    let v = prop_value(port, id, prop);
    match v.as_f64() {
        Some(f) => f,
        None => panic!("node {id} '{prop}' is not a number: {v}"),
    }
}

/// A `Vector2` property, serialised as `{"type":"Vector2","value":[x,y]}`.
fn prop_xy(port: u16, id: u64, prop: &str) -> (f64, f64) {
    let v = prop_value(port, id, prop);
    let comps: Vec<f64> = match v.as_array() {
        Some(a) => a.iter().filter_map(|c| c.as_f64()).collect(),
        None => panic!("node {id} '{prop}' is not a Vector2 array: {v}"),
    };
    assert!(
        comps.len() >= 2,
        "node {id} '{prop}' needs 2 numeric components, got {v}"
    );
    (comps[0], comps[1])
}

/// Alpha and Beta sit 100 units either side of the origin so the viewport
/// hit-test can tell them apart — `/api/viewport/drag_start` picks its target by
/// hit-testing the click pixel, and a plain `Node2D` has only a small hit radius.
fn server() -> (EditorServerHandle, u16) {
    let port = free_port();
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let main = tree.add_child(root, Node::new("Main", "Node2D")).unwrap();
    let mut alpha = Node::new("Alpha", "Node2D");
    alpha.set_property("position", Variant::Vector2(Vector2::new(-100.0, 0.0)));
    let mut beta = Node::new("Beta", "Node2D");
    beta.set_property("position", Variant::Vector2(Vector2::new(100.0, 0.0)));
    tree.add_child(main, alpha).unwrap();
    tree.add_child(main, beta).unwrap();
    let handle = EditorServerHandle::start(port, EditorState::new(tree));
    thread::sleep(Duration::from_millis(100));
    (handle, port)
}

/// Selects a node the way a browser does — by clicking its rendered pixel. The
/// default viewport is 800x600 at zoom 1 centred on the scene bounds (symmetric
/// here), so world (x, y) renders at pixel (400 + x, 300 + y).
fn select_by_click(port: u16, expect: u64, world_x: f32, world_y: f32) {
    let px = 400.0 + world_x;
    let py = 300.0 + world_y;
    let started = json_of(&http_post(
        port,
        "/api/viewport/drag_start",
        &format!(r#"{{"x":{px},"y":{py}}}"#),
    ));
    assert_eq!(
        started["node_id"].as_u64(),
        Some(expect),
        "drag_start must grab the intended node: {started}"
    );
}

/// **The acceptance test.**
#[test]
fn viewport2d_tool_drag_e2e() {
    let (_h, port) = server();
    let alpha = node_id(port, "Alpha");
    let beta = node_id(port, "Beta");

    // ── 1. Rotate mode ────────────────────────────────────────────────────
    select_by_click(port, alpha, -100.0, 0.0);
    let rot_before = prop_f64(port, alpha, "rotation");
    // The browser sends one increment per mousemove; send a few, as a real drag
    // would, to prove they accumulate rather than overwrite.
    for _ in 0..3 {
        http_post(port, "/api/viewport/rotate_node", r#"{"delta":0.25}"#);
    }
    http_post(port, "/api/viewport/drag_end", r#"{"x":300.0,"y":300.0}"#);
    let rot_after = prop_f64(port, alpha, "rotation");
    assert!(
        (rot_after - (rot_before + 0.75)).abs() < 1e-4,
        "three 0.25 increments must accumulate: {rot_before} -> {rot_after}"
    );

    // Survives a scene save — the acceptance's persistence clause.
    let save_path = std::env::temp_dir().join(format!(
        "patina_0q2kx15_{}_{}.tscn",
        std::process::id(),
        port
    ));
    let saved = json_of(&http_post(
        port,
        "/api/scene/save",
        &format!(r#"{{"path":"{}"}}"#, save_path.to_string_lossy()),
    ));
    assert_ne!(
        saved["error"].as_str(),
        Some("missing path"),
        "save must accept the path: {saved}"
    );
    let text = std::fs::read_to_string(&save_path).expect("scene file written");
    assert!(
        text.contains("rotation"),
        "the rotated node's rotation must be serialised into the .tscn:\n{text}"
    );
    let _ = std::fs::remove_file(&save_path);

    // ── 2. Scale mode ─────────────────────────────────────────────────────
    let (sx_before, sy_before) = prop_xy(port, alpha, "scale");
    http_post(port, "/api/viewport/scale_node", r#"{"sx":2.0,"sy":2.0}"#);
    let (sx_after, sy_after) = prop_xy(port, alpha, "scale");
    assert!(
        (sx_after - sx_before * 2.0).abs() < 1e-4 && (sy_after - sy_before * 2.0).abs() < 1e-4,
        "scale_node multiplies: ({sx_before},{sy_before}) -> ({sx_after},{sy_after})"
    );

    // ── 3. Axis-locked move ───────────────────────────────────────────────
    let (ax_before, ay_before) = prop_xy(port, alpha, "position");
    http_post(
        port,
        "/api/viewport/drag_axis",
        r#"{"dx":40.0,"dy":40.0,"axis":"x"}"#,
    );
    let (ax_after, ay_after) = prop_xy(port, alpha, "position");
    assert!(
        (ax_after - ax_before).abs() > 1e-4,
        "axis=x must move X: {ax_before} -> {ax_after}"
    );
    assert!(
        (ay_after - ay_before).abs() < 1e-4,
        "axis=x must NOT move Y: {ay_before} -> {ay_after}"
    );

    // ── 4. Multi-node drag ────────────────────────────────────────────────
    http_post(
        port,
        "/api/node/select_multi",
        &format!(r#"{{"node_ids":[{alpha},{beta}]}}"#),
    );
    let a0 = prop_xy(port, alpha, "position");
    let b0 = prop_xy(port, beta, "position");
    http_post(
        port,
        "/api/viewport/drag_multi",
        r#"{"dx":25.0,"dy":15.0,"snap":false}"#,
    );
    let a1 = prop_xy(port, alpha, "position");
    let b1 = prop_xy(port, beta, "position");
    assert!(
        (a1.0 - a0.0).abs() > 1e-4 && (b1.0 - b0.0).abs() > 1e-4,
        "drag_multi must move BOTH selected nodes: Alpha {a0:?}->{a1:?}, Beta {b0:?}->{b1:?}"
    );
    // They move together, not independently.
    assert!(
        ((a1.0 - a0.0) - (b1.0 - b0.0)).abs() < 1e-4
            && ((a1.1 - a0.1) - (b1.1 - b0.1)).abs() < 1e-4,
        "both nodes must move by the same delta: Alpha {a0:?}->{a1:?}, Beta {b0:?}->{b1:?}"
    );

    // ── 5. The client actually calls these routes ─────────────────────────
    //
    // The half that was missing: the endpoints all worked before this bead, but
    // no client code path reached them. Asserting on EDITOR_HTML is what makes
    // this a wiring test rather than another server-side unit test.
    //
    // ⚠️ Assert on the CONSTANT, not on `GET /`. `EDITOR_HTML` is ~741 KB, and
    // reading that back over the socket truncates against the read timeout — the
    // fetched body silently lost the script block, so this assertion failed
    // while the wiring was present. The other wired-panel tests in this crate
    // (editor_theme_resource_editor_wired_test, editor_vcs_panel_wired_test)
    // check the constant for the same reason.
    let html = EDITOR_HTML;
    for route in [
        "/api/viewport/rotate_node",
        "/api/viewport/scale_node",
        "/api/viewport/drag_axis",
        "/api/viewport/drag_multi",
    ] {
        assert!(
            html.contains(route),
            "served editor HTML must contain a client call to {route}"
        );
    }
    // …and that it branches on the tool rather than always moving.
    assert!(
        html.contains("currentToolMode === 'rotate'")
            && html.contains("currentToolMode === 'scale'"),
        "the 2D pointer handler must branch on currentToolMode"
    );
    // Not vacuous: the original move route is still there for select mode.
    assert!(html.contains("/api/viewport/drag_start"));
}
