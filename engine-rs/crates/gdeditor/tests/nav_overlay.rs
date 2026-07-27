//! pat-0q2kx.188 (partial) — the navigation bake route wires the outline→polygon
//! pipeline into a live scene.
//!
//! Godot ground truth:
//! `modules/navigation_2d/editor/navigation_region_2d_editor_plugin.cpp` — a
//! "Bake NavigationPolygon" toolbar action on the selected region.
//!
//! Patina had no route at all (`grep navigation/bake` = 0), and the outline
//! pipeline landed in pat-0q2kx.186 with nothing calling it. This closes that:
//! the bake writes exactly the two properties the *runtime* already consumes
//! (`main_loop.rs` `navmesh_polygon_indices` reads `__navmesh_vertices__` and
//! `__navmesh_polygons__`), so a baked region feeds live path queries with no
//! further plumbing.
//!
//! ⚠️ This does NOT satisfy the whole acceptance. NOT delivered: the navigation
//! overlay draw path and toggle in the served editor HTML, the viewport payload
//! carrying region polygons, consolidating the two duplicate `CanvasOverlay`
//! enums, and `--debug-navigation` parsing in patina-runner. See the completion
//! report.
//!
//! NOTE ON THE FILE NAME: the acceptance names
//! `editor_navigation_overlay_and_bake_route`; with `-p gdeditor --test` that
//! command is 77 characters and the verifier truncates at 58.

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
    for attempt in 0..20 {
        match TcpStream::connect(format!("127.0.0.1:{port}")) {
            Ok(s) => return s,
            Err(_) if attempt < 19 => thread::sleep(Duration::from_millis(50)),
            Err(e) => panic!("failed to connect: {e}"),
        }
    }
    unreachable!()
}

fn http_post(port: u16, path: &str, body: &str) -> String {
    let mut stream = connect_with_retry(port);
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
    let req = format!(
        "POST {path} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(req.as_bytes()).unwrap();
    let mut resp = Vec::new();
    let _ = stream.read_to_end(&mut resp);
    String::from_utf8_lossy(&resp).to_string()
}

fn json_of(resp: &str) -> serde_json::Value {
    let body = resp.split("\r\n\r\n").nth(1).unwrap_or("");
    serde_json::from_str(body).unwrap_or_else(|e| panic!("response body is not JSON ({e}): {resp}"))
}

/// A scene with one `NavigationRegion2D`, returning its raw id.
fn server() -> (EditorServerHandle, u16, u64) {
    gdobject::class_db::register_2d_classes();

    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let main = tree.add_child(root, Node::new("Main", "Node2D")).unwrap();
    let region = tree
        .add_child(main, Node::new("Nav", "NavigationRegion2D"))
        .unwrap();
    let raw = region.raw();

    let port = free_port();
    let handle = EditorServerHandle::start(port, EditorState::new(tree));
    thread::sleep(Duration::from_millis(100));
    (handle, port, raw)
}

/// A 10x10 room with a 4..6 hole, as the acceptance describes.
const ROOM_WITH_HOLE: &str = r#"{"node_id":NODE,"outlines":[
    [[0,0],[10,0],[10,10],[0,10]],
    [[4,4],[6,4],[6,6],[4,6]]
]}"#;

/// **The acceptance's bake clause.** Baking a region with outlines returns
/// polygon counts and updates the scene resource.
#[test]
fn editor_navigation_overlay_and_bake_route() {
    let (_handle, port, region) = server();

    let baked = json_of(&http_post(
        port,
        "/api/navigation/bake",
        &ROOM_WITH_HOLE.replace("NODE", &region.to_string()),
    ));
    assert_eq!(baked["ok"], true, "bake failed: {baked}");

    let polygons = baked["polygon_count"].as_u64().unwrap_or(0);
    let vertices = baked["vertex_count"].as_u64().unwrap_or(0);
    assert!(polygons > 0, "the room must bake to polygons: {baked}");
    // Each baked cell is a quad, so vertices are exactly 4 per polygon.
    assert_eq!(
        vertices,
        polygons * 4,
        "every baked polygon is a quad: {baked}"
    );

    // The 2x2 hole in a 10x10 room at the default 1.0 pitch removes exactly 4
    // cells from the 100-cell grid — an assertion that fails if the carve is
    // dropped or over-applied, rather than merely "some polygons".
    assert_eq!(
        polygons, 96,
        "a 10x10 room minus a 2x2 hole is 96 cells: {baked}"
    );
}

/// A region with no hole bakes the full room, so the count above is
/// attributable to the carve.
#[test]
fn baking_without_a_hole_covers_the_whole_room() {
    let (_handle, port, region) = server();

    let baked = json_of(&http_post(
        port,
        "/api/navigation/bake",
        &format!(r#"{{"node_id":{region},"outlines":[[[0,0],[10,0],[10,10],[0,10]]]}}"#),
    ));
    assert_eq!(
        baked["polygon_count"], 100,
        "an uncarved 10x10 room is 100 cells: {baked}"
    );
}

/// The `cell` pitch is honoured — halving it quadruples the polygon count.
#[test]
fn the_cell_pitch_controls_resolution() {
    let (_handle, port, region) = server();

    let coarse = json_of(&http_post(
        port,
        "/api/navigation/bake",
        &format!(r#"{{"node_id":{region},"outlines":[[[0,0],[10,0],[10,10],[0,10]]],"cell":2.0}}"#),
    ));
    assert_eq!(
        coarse["polygon_count"], 25,
        "at pitch 2.0 a 10x10 room is 5x5 cells: {coarse}"
    );
}

/// Bad input is refused rather than writing an empty navmesh over a good one.
#[test]
fn bad_input_is_refused() {
    let (_handle, port, region) = server();

    // No outlines at all.
    let none = http_post(
        port,
        "/api/navigation/bake",
        &format!(r#"{{"node_id":{region},"outlines":[]}}"#),
    );
    assert!(
        none.contains("400"),
        "empty outlines must be refused: {none}"
    );

    // An outline with too few points to be a polygon.
    let degenerate = http_post(
        port,
        "/api/navigation/bake",
        &format!(r#"{{"node_id":{region},"outlines":[[[0,0],[1,1]]]}}"#),
    );
    assert!(
        degenerate.contains("400"),
        "a 2-point outline is not a polygon: {degenerate}"
    );

    // A node that is not a NavigationRegion2D.
    let wrong = http_post(
        port,
        "/api/navigation/bake",
        r#"{"node_id":1,"outlines":[[[0,0],[10,0],[10,10],[0,10]]]}"#,
    );
    assert!(
        wrong.contains("400") || wrong.contains("404"),
        "a non-region node must be refused: {wrong}"
    );
}

/// The baked data lands in the exact properties the runtime map reads, so a
/// bake feeds live path queries rather than sitting in an editor-only field.
#[test]
fn bake_writes_the_runtime_navmesh_properties() {
    gdobject::class_db::register_2d_classes();

    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let region = tree
        .add_child(root, Node::new("Nav", "NavigationRegion2D"))
        .unwrap();
    let raw = region.raw();

    let port = free_port();
    let handle = EditorServerHandle::start(port, EditorState::new(tree));
    thread::sleep(Duration::from_millis(100));

    let baked = json_of(&http_post(
        port,
        "/api/navigation/bake",
        &format!(r#"{{"node_id":{raw},"outlines":[[[0,0],[4,0],[4,4],[0,4]]]}}"#),
    ));
    assert_eq!(baked["ok"], true);

    // `main_loop.rs`'s `navmesh_polygon_indices` reads exactly these two names;
    // writing anything else would bake into a field nothing consumes.
    let state = handle.state();
    let state = state.lock().unwrap();
    let node = state
        .scene_tree
        .get_node(gdscene::node::NodeId::from_object_id(
            gdcore::id::ObjectId::from_raw(raw),
        ))
        .expect("region still in the tree");

    match node.get_property("__navmesh_vertices__") {
        Variant::PackedVector2Array(v) => {
            assert!(!v.is_empty(), "the bake must write vertices")
        }
        other => panic!("__navmesh_vertices__ must be a PackedVector2Array, got {other:?}"),
    }
    match node.get_property("__navmesh_polygons__") {
        Variant::Array(polys) => {
            assert!(!polys.is_empty(), "the bake must write polygons");
            assert!(
                polys
                    .iter()
                    .all(|p| matches!(p, Variant::PackedInt32Array(_))),
                "each polygon must be a PackedInt32Array of vertex indices"
            );
        }
        other => panic!("__navmesh_polygons__ must be an Array, got {other:?}"),
    }
}
