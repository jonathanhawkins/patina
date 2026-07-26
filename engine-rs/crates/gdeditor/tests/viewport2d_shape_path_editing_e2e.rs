//! pat-0q2kx.77: CollisionShape2D resize handles and Path2D/Polygon2D point
//! editing, end to end over HTTP.
//!
//! Godot draws draggable handles on a selected CollisionShape2D and a full point
//! editor on Path2D/Polygon2D/Line2D
//! (`editor/scene/2d/physics/collision_shape_2d_editor_plugin.cpp`,
//! `path_2d_editor_plugin.cpp`, `abstract_polygon_2d_editor.cpp`).
//!
//! Patina had `api_shape_resize` implemented, undo-integrated and unit-tested
//! with ZERO client callers, and no point editing at all. Two deeper faults sat
//! underneath: the resize wrote `shape_extents`, a name nothing reads, and the
//! `.tscn` writer emitted `[sub_resource]` headers with an EMPTY body — so a
//! shape's authored geometry was dropped on every save regardless.

use gdeditor::editor_server::{EditorServerHandle, EditorState};
use gdscene::node::Node;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_saver::TscnSaver;
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

fn json_of(resp: &str) -> serde_json::Value {
    serde_json::from_str(resp.split("\r\n\r\n").nth(1).unwrap_or(""))
        .unwrap_or_else(|e| panic!("response body is not JSON ({e}): {resp}"))
}

/// A scene loaded from real `.tscn` text, so the CollisionShape2D carries the
/// `__resref__shape` / `__shape__*` companions a loaded scene actually has —
/// hand-building the node would skip the very path under test.
const SCENE: &str = r#"[gd_scene load_steps=2 format=3]

[sub_resource type="RectangleShape2D" id="Rect_1"]
size = Vector2(20, 20)

[node name="Main" type="Node2D"]

[node name="Body" type="CollisionShape2D" parent="."]
shape = SubResource("Rect_1")

[node name="Path" type="Path2D" parent="."]

[node name="Poly" type="Polygon2D" parent="."]
"#;

fn server() -> (EditorServerHandle, u16, u64, u64, u64) {
    gdobject::class_db::register_2d_classes();

    let scene = PackedScene::from_tscn(SCENE).expect("fixture scene parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let scene_root = add_packed_scene_to_tree(&mut tree, root, &scene).expect("instantiates");

    let mut ids = Vec::new();
    tree.collect_subtree_top_down(scene_root, &mut ids);
    let find = |name: &str| -> u64 {
        ids.iter()
            .find(|id| {
                tree.get_node(**id)
                    .map(|n| n.name() == name)
                    .unwrap_or(false)
            })
            .unwrap_or_else(|| panic!("node {name} missing"))
            .raw()
    };
    let (body, path, poly) = (find("Body"), find("Path"), find("Poly"));

    let port = free_port();
    let handle = EditorServerHandle::start(port, EditorState::new(tree));
    thread::sleep(Duration::from_millis(100));
    (handle, port, body, path, poly)
}

/// **The acceptance test.**
#[test]
fn viewport2d_shape_path_editing_e2e() {
    let (handle, port, body, path, _poly) = server();

    // -- the shape reports draggable handles ----------------------------------
    let handles = json_of(&http_get(
        port,
        &format!("/api/viewport/shape_handles?node_id={body}"),
    ));
    assert_eq!(handles["editable"], true, "handles unavailable: {handles}");
    assert_eq!(
        handles["shape"], "RectangleShape2D",
        "the shape class comes from the loaded sub-resource: {handles}"
    );
    assert!(
        !handles["handles"].as_array().unwrap().is_empty(),
        "a rectangle must expose at least one resize handle: {handles}"
    );

    // -- drag a handle ---------------------------------------------------------
    let resized = json_of(&http_post(
        port,
        "/api/viewport/shape_resize",
        &format!(r#"{{"node_id":{body},"handle":"extents","value":[64,48]}}"#),
    ));
    assert_eq!(resized["ok"], true, "resize failed: {resized}");

    // -- the SAVED .tscn sub_resource carries the new size --------------------
    //
    // ⚠️ This is the clause that exposed two bugs at once: the resize used to
    // write `shape_extents`, which nothing reads, and the writer emitted
    // `[sub_resource ...]` with no body at all, so even a correct write was
    // dropped on save.
    let saved = {
        let state = handle.state();
        let state = state.lock().unwrap();
        let root = state.scene_tree.root_id();
        TscnSaver::save_tree(&state.scene_tree, root)
    };
    assert!(
        saved.contains(r#"[sub_resource type="RectangleShape2D""#),
        "the shape sub-resource must still be declared:\n{saved}"
    );
    assert!(
        saved.contains("size = Vector2(64, 48)"),
        "the resized extents must reach the saved sub_resource body:\n{saved}"
    );

    // …and it re-parses, so the change is not merely textual.
    let reparsed = PackedScene::from_tscn(&saved)
        .unwrap_or_else(|e| panic!("saved scene must re-parse ({e:?}):\n{saved}"));
    let mut back = SceneTree::new();
    let broot = back.root_id();
    let scene_root = add_packed_scene_to_tree(&mut back, broot, &reparsed).expect("instantiates");
    let mut ids = Vec::new();
    back.collect_subtree_top_down(scene_root, &mut ids);
    let reloaded_body = ids
        .iter()
        .find(|id| back.get_node(**id).map(|n| n.name() == "Body").unwrap_or(false))
        .copied()
        .expect("Body survives the round trip");
    assert_eq!(
        back.get_node(reloaded_body).map(|n| n.get_property("__shape__size")),
        Some(Variant::Vector2(gdcore::math::Vector2::new(64.0, 48.0))),
        "the reloaded shape must carry the resized geometry"
    );

    // -- undo restores the original size --------------------------------------
    let undone = json_of(&http_post(port, "/api/undo", "{}"));
    assert_eq!(undone["ok"], true, "undo failed: {undone}");
    {
        let state = handle.state();
        let state = state.lock().unwrap();
        let nid = state
            .scene_tree
            .get_node_by_path("/root/Main/Body")
            .expect("body exists");
        assert_eq!(
            state.scene_tree.get_node(nid).map(|n| n.get_property("__shape__size")),
            Some(Variant::Vector2(gdcore::math::Vector2::new(20.0, 20.0))),
            "undo must restore the size the scene was authored with"
        );
    }

    // -- Path2D: add two points ------------------------------------------------
    for (x, y) in [(10.0, 10.0), (90.0, 40.0)] {
        let added = json_of(&http_post(
            port,
            "/api/path2d/point/add",
            &format!(r#"{{"node_id":{path},"x":{x},"y":{y}}}"#),
        ));
        assert_eq!(added["ok"], true, "point add failed: {added}");
    }

    let points = json_of(&http_get(port, &format!("/api/viewport/points?node_id={path}")));
    assert_eq!(points["editable"], true, "Path2D must be point-editable");
    assert_eq!(
        points["property"], "__curve__points",
        "a Path2D's points live on its flattened Curve2D companion"
    );
    let list = points["points"].as_array().expect("points array");
    assert_eq!(list.len(), 2, "the curve has two points: {points}");
    assert_eq!(list[0][0], 10.0);
    assert_eq!(list[1][0], 90.0);

    // -- the viewport render includes the path --------------------------------
    //
    // ⚠️ Asserted against the render list rather than the PNG bytes: a pixel
    // threshold would pass on any yellow smudge, while this names the node.
    {
        let state = handle.state();
        let state = state.lock().unwrap();
        let items = gdeditor::viewport_2d_render::collect_canvas_items(&state.scene_tree);
        let drawn = items.iter().any(|i| {
            matches!(
                &i.kind,
                gdeditor::viewport_2d_render::Render2DKind::Line { points, .. } if points.len() == 2
            )
        });
        assert!(
            drawn,
            "the Path2D must render as a 2-point polyline, not a bare marker"
        );
    }

    // -- undo removes the last point ------------------------------------------
    http_post(port, "/api/undo", "{}");
    let after = json_of(&http_get(port, &format!("/api/viewport/points?node_id={path}")));
    assert_eq!(
        after["points"].as_array().map(|p| p.len()),
        Some(1),
        "undo drops the last-added point, not the whole curve: {after}"
    );
}

/// One route set serves all three classes, and refuses nodes that have no point
/// list rather than inventing a property on them.
#[test]
fn point_routes_cover_polygon_and_reject_other_classes() {
    let (handle, port, body, _path, poly) = server();

    // Polygon2D edits its own `polygon` property.
    for (x, y) in [(0.0, 0.0), (10.0, 0.0), (10.0, 10.0)] {
        let added = json_of(&http_post(
            port,
            "/api/polygon2d/vertex/add",
            &format!(r#"{{"node_id":{poly},"x":{x},"y":{y}}}"#),
        ));
        assert_eq!(added["ok"], true, "vertex add failed: {added}");
    }
    let points = json_of(&http_get(port, &format!("/api/viewport/points?node_id={poly}")));
    assert_eq!(
        points["property"], "polygon",
        "a Polygon2D holds its vertices directly: {points}"
    );
    assert_eq!(points["points"].as_array().map(|p| p.len()), Some(3));

    // Moving and removing address the list by index.
    let moved = json_of(&http_post(
        port,
        "/api/polygon2d/vertex/move",
        &format!(r#"{{"node_id":{poly},"index":1,"x":55,"y":5}}"#),
    ));
    assert_eq!(moved["ok"], true, "vertex move failed: {moved}");
    let after_move = json_of(&http_get(port, &format!("/api/viewport/points?node_id={poly}")));
    assert_eq!(after_move["points"][1][0], 55.0, "the moved vertex updated");

    let removed = json_of(&http_post(
        port,
        "/api/polygon2d/vertex/remove",
        &format!(r#"{{"node_id":{poly},"index":0}}"#),
    ));
    assert_eq!(removed["count"], 2, "one vertex removed: {removed}");

    // An out-of-range index is a 404, not a silent no-op.
    let bad = http_post(
        port,
        "/api/polygon2d/vertex/remove",
        &format!(r#"{{"node_id":{poly},"index":99}}"#),
    );
    assert!(bad.contains("404"), "an out-of-range index must fail: {bad}");

    // ⚠️ A CollisionShape2D has no point list; the route must refuse rather than
    // writing a `polygon` property onto a node that has no such concept.
    let wrong = http_post(
        port,
        "/api/polygon2d/vertex/add",
        &format!(r#"{{"node_id":{body},"x":1,"y":1}}"#),
    );
    assert!(
        wrong.contains("400"),
        "a class with no point list must be refused: {wrong}"
    );

    handle.stop();
}

/// The bead's client half: these routes had no callers at all before this.
#[test]
fn sub_editor_handles_are_wired_in_editor_html() {
    let html = gdeditor::editor_ui::EDITOR_HTML;

    for route in [
        "/api/viewport/shape_handles?node_id=",
        "/api/viewport/points?node_id=",
        "/api/viewport/shape_resize",
        "/api/path2d/point/add",
        "/api/path2d/point/move",
        "/api/path2d/point/remove",
    ] {
        assert!(html.contains(route), "no client calls {route}");
    }
    assert!(
        html.contains("shape-handle-overlay"),
        "handles need an overlay to be drawn into"
    );
    assert!(
        html.contains("function refreshSubEditorHandles"),
        "handles must be rebuilt when the selection changes"
    );
    assert!(
        html.contains("refreshSubEditorHandles(); // pat-0q2kx.77"),
        "…and fetchSelected must actually call it"
    );
    assert!(
        html.contains("function subEditorDragMove"),
        "dragging a handle must post the new geometry"
    );
}
