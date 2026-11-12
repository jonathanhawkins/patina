//! pat-0q2kx.12 acceptance: every mutating editor op class creates a real undo
//! step.
//!
//! Godot ground truth: `editor/editor_undo_redo_manager.cpp` — every editor
//! action (transform drags, structural ops, connections, animation edits)
//! registers an undo step, so Ctrl+Z always walks back exactly one action.
//!
//! Patina's `undo_stack` covered ~17 routes; the ones exercised here covered
//! none. A viewport drag, a 3D gizmo move, a reorder, a paste, a group add, a
//! signal connect and a keyframe add were all **permanent**.
//!
//! These tests drive the **real HTTP server**, and each asserts *state
//! equality against a pre-op snapshot* rather than merely "a history entry
//! appeared". That distinction matters: `/api/viewport/drag` writes `position`
//! live during a drag, so a command that captured `old_value` at drag_end would
//! record the last mid-drag frame — producing an undo step that shows up in the
//! history and restores nothing. Only a state-equality assertion catches it.

use gdcore::math::Vector2;
use gdeditor::editor_server::{EditorServerHandle, EditorState};
use gdscene::node::Node;
use gdscene::SceneTree;
use gdvariant::Variant;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;
use std::time::Duration;

// ---------------------------------------------------------------------------
// HTTP harness (mirrors editor_export_presets_e2e_test.rs)
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

/// A scene with a root and two children, so reorder/paste have something to
/// work with. Returns the handle (kept alive by the caller) and the port.
///
/// Alpha and Beta sit 100 units either side of the origin so the viewport
/// hit-test can tell them apart: `/api/viewport/drag_start` picks its target by
/// hit-testing the click pixel (it ignores any `node_id` in the body), and a
/// plain `Node2D` only has a 15px hit radius. Overlapping them at the origin
/// would make the drag grab an arbitrary one.
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

/// Begins a viewport drag on `expect_node` by clicking its rendered pixel, and
/// fails loudly if the hit-test grabbed something else (or nothing).
///
/// The default viewport is 800x600 at zoom 1 with no pan, and the camera centres
/// the scene bounds — which are symmetric about the origin here — so world
/// (x, y) renders at pixel (400 + x, 300 + y).
fn drag_start_on(port: u16, expect_node: u64, world_x: f32, world_y: f32) -> (f32, f32) {
    let px = 400.0 + world_x;
    let py = 300.0 + world_y;
    let started = json_of(&http_post(
        port,
        "/api/viewport/drag_start",
        &format!(r#"{{"x":{px},"y":{py}}}"#),
    ));
    assert_eq!(
        started["dragging"], true,
        "drag_start must hit a node at pixel ({px}, {py}); got {started}"
    );
    assert_eq!(
        started["node_id"].as_u64(),
        Some(expect_node),
        "drag_start hit the wrong node: {started}"
    );
    (px, py)
}

/// Whole-scene snapshot — names, order and nesting. Structural undo must
/// restore this byte for byte.
fn scene_snapshot(port: u16) -> String {
    json_of(&http_get(port, "/api/scene"))["nodes"].to_string()
}

/// Finds a node's raw id by name anywhere in the scene JSON.
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

/// A single node's JSON (properties included) — the snapshot for property ops.
fn node_json(port: u16, id: u64) -> serde_json::Value {
    json_of(&http_get(port, &format!("/api/node/{id}")))
}

fn undo(port: u16) -> serde_json::Value {
    json_of(&http_post(port, "/api/undo", "{}"))
}

fn redo(port: u16) -> serde_json::Value {
    json_of(&http_post(port, "/api/redo", "{}"))
}

fn history_labels(port: u16) -> Vec<String> {
    json_of(&http_get(port, "/api/history"))["entries"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|e| e["label"].as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// The acceptance test: one round-trip per op class.
// ---------------------------------------------------------------------------

#[test]
fn editor_undo_coverage_e2e() {
    let (_h, port) = server();
    let alpha = node_id(port, "Alpha");

    // -- (1) 2D viewport drag ------------------------------------------------
    // The regression this guards: `/api/viewport/drag` writes position live, so
    // capturing `old_value` at drag_end would record the mid-drag value and undo
    // would restore the drag's final frame instead of the starting point.
    let before_pos = node_json(port, alpha);
    let (px, py) = drag_start_on(port, alpha, -100.0, 0.0);
    http_post(
        port,
        "/api/viewport/drag",
        &format!(r#"{{"x":{},"y":{}}}"#, px + 40.0, py + 20.0),
    );
    http_post(
        port,
        "/api/viewport/drag",
        &format!(r#"{{"x":{},"y":{}}}"#, px + 80.0, py + 60.0),
    );
    let moved = json_of(&http_post(
        port,
        "/api/viewport/drag_end",
        &format!(r#"{{"x":{},"y":{}}}"#, px + 80.0, py + 60.0),
    ));
    assert_eq!(moved["ok"], true, "drag_end should succeed");
    let after_pos = node_json(port, alpha);
    assert_ne!(
        before_pos["properties"], after_pos["properties"],
        "the drag must actually move the node"
    );

    assert_eq!(undo(port)["ok"], true, "undo after drag");
    assert_eq!(
        node_json(port, alpha)["properties"],
        before_pos["properties"],
        "undo must restore the PRE-DRAG position, not the last mid-drag frame"
    );
    assert_eq!(redo(port)["ok"], true, "redo after drag");
    assert_eq!(
        node_json(port, alpha)["properties"],
        after_pos["properties"],
        "redo must reapply the drag"
    );
    undo(port); // leave the scene as we found it

    // -- (2) node reorder ----------------------------------------------------
    let before_tree = scene_snapshot(port);
    let beta = node_id(port, "Beta");
    let r = json_of(&http_post(
        port,
        "/api/node/reorder",
        &format!(r#"{{"node_id":{beta},"direction":"up"}}"#),
    ));
    assert_eq!(r["ok"], true, "reorder should succeed");
    let after_tree = scene_snapshot(port);
    assert_ne!(before_tree, after_tree, "reorder must change sibling order");

    assert_eq!(undo(port)["ok"], true, "undo after reorder");
    assert_eq!(
        scene_snapshot(port),
        before_tree,
        "undo must restore the original sibling order"
    );
    assert_eq!(redo(port)["ok"], true, "redo after reorder");
    assert_eq!(
        scene_snapshot(port),
        after_tree,
        "redo must reapply reorder"
    );
    undo(port);

    // -- (3) group add -------------------------------------------------------
    // Group membership is reported in the SCENE json (`"groups"` per node), not
    // in `/api/node/{id}`'s property list, so snapshot the tree here.
    let before_groups = scene_snapshot(port);
    let g = json_of(&http_post(
        port,
        "/api/node/groups/add",
        &format!(r#"{{"node_id":{alpha},"group":"enemies"}}"#),
    ));
    assert_eq!(g["ok"], true, "group add should succeed");
    let after_groups = scene_snapshot(port);
    assert_ne!(
        before_groups, after_groups,
        "group add must change the reported groups"
    );
    assert!(
        after_groups.contains("enemies"),
        "the new group should be listed: {after_groups}"
    );

    assert_eq!(undo(port)["ok"], true, "undo after group add");
    assert_eq!(
        scene_snapshot(port),
        before_groups,
        "undo must remove the group again"
    );
    assert_eq!(redo(port)["ok"], true, "redo after group add");
    assert_eq!(scene_snapshot(port), after_groups, "redo must re-add");
    undo(port);

    // -- (4) signal connect --------------------------------------------------
    // The scene JSON carries a `has_signals` flag per node, which flips on the
    // first connection — a guaranteed observable, unlike relying on
    // `signal_connections` surfacing in `/api/node/{id}`'s property list.
    let before_sig = scene_snapshot(port);
    let main_for_signal = node_id(port, "Main");
    http_post(
        port,
        "/api/node/signals/connect",
        &format!(
            r#"{{"node_id":{alpha},"signal":"pressed","method":"_on_pressed","target_id":{main_for_signal}}}"#
        ),
    );
    let after_sig = scene_snapshot(port);
    // Asserted, not guarded: an `if changed {...}` wrapper would let a wrong
    // payload key skip the whole round-trip and still report success.
    assert_ne!(
        before_sig, after_sig,
        "connecting a signal must record it on the node"
    );
    assert_eq!(undo(port)["ok"], true, "undo after signal connect");
    assert_eq!(
        scene_snapshot(port),
        before_sig,
        "undo must drop the connection entry"
    );
    assert_eq!(redo(port)["ok"], true, "redo after signal connect");
    assert_eq!(scene_snapshot(port), after_sig, "redo must reconnect");
    undo(port);

    // -- (5) paste -----------------------------------------------------------
    let before_paste = scene_snapshot(port);
    http_post(
        port,
        "/api/node/copy",
        &format!(r#"{{"node_ids":[{alpha}]}}"#),
    );
    let main = node_id(port, "Main");
    let p = json_of(&http_post(
        port,
        "/api/node/paste",
        &format!(r#"{{"parent_id":{main}}}"#),
    ));
    assert_eq!(p["ok"], true, "paste should succeed");
    let after_paste = scene_snapshot(port);
    assert_ne!(before_paste, after_paste, "paste must add nodes");

    assert_eq!(undo(port)["ok"], true, "undo after paste");
    assert_eq!(
        scene_snapshot(port),
        before_paste,
        "undo must remove every pasted node"
    );
    assert_eq!(redo(port)["ok"], true, "redo after paste");
    assert_eq!(
        scene_snapshot(port),
        after_paste,
        "redo must re-paste with the same names (uniquification included)"
    );
    undo(port);

    // -- (6) animation keyframe add -----------------------------------------
    http_post(port, "/api/animation/create", r#"{"name":"walk"}"#);
    let before_anim = body_of(&http_get(port, "/api/animation?name=walk")).to_string();
    // NOTE the capitalised `"Float"` type tag — `gdvariant::serialize::from_json`
    // matches exactly, and a lowercase tag makes the route reject the body with
    // a 400. This is asserted rather than guarded on purpose: an `if ok {...}`
    // wrapper here would turn a malformed request into a silent skip.
    let k = json_of(&http_post(
        port,
        "/api/animation/keyframe/add",
        r#"{"animation":"walk","track_node":"Alpha","track_property":"position","time":0.5,"value":{"type":"Float","value":3.0}}"#,
    ));
    assert_eq!(k["ok"], true, "keyframe add should succeed, got {k}");
    let after_anim = body_of(&http_get(port, "/api/animation?name=walk")).to_string();
    assert_ne!(before_anim, after_anim, "keyframe add must change the clip");
    assert_eq!(undo(port)["ok"], true, "undo after keyframe add");
    assert_eq!(
        body_of(&http_get(port, "/api/animation?name=walk")),
        before_anim,
        "undo must restore the clip exactly"
    );
    assert_eq!(redo(port)["ok"], true, "redo after keyframe add");
    assert_eq!(
        body_of(&http_get(port, "/api/animation?name=walk")),
        after_anim,
        "redo must reapply the keyframe"
    );
    undo(port);

    // -- (7) the History dock lists the steps -------------------------------
    let labels = history_labels(port);
    assert!(
        labels.len() > 1,
        "the history should list the actions performed, got {labels:?}"
    );
}

/// A drag that ends where it began still round-trips cleanly — the guard
/// against an undo step that "restores" a stale mid-drag value.
#[test]
fn undo_after_drag_restores_exact_start_position() {
    let (_h, port) = server();
    let alpha = node_id(port, "Alpha");
    let start = node_json(port, alpha)["properties"].clone();

    let (px, py) = drag_start_on(port, alpha, -100.0, 0.0);
    // Several intermediate frames, each writing `position`.
    for step in 1..=5 {
        http_post(
            port,
            "/api/viewport/drag",
            &format!(
                r#"{{"x":{},"y":{}}}"#,
                px + (step as f32) * 13.0,
                py + (step as f32) * 7.0
            ),
        );
    }
    http_post(
        port,
        "/api/viewport/drag_end",
        &format!(r#"{{"x":{},"y":{}}}"#, px + 65.0, py + 35.0),
    );

    assert_eq!(undo(port)["ok"], true);
    assert_eq!(
        node_json(port, alpha)["properties"],
        start,
        "a single undo must return the node to where the drag started, \
         regardless of how many intermediate drag frames were written"
    );
}
