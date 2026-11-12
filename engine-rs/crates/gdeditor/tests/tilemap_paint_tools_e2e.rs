//! pat-0q2kx.75: the full TileMap paint toolset over HTTP.
//!
//! Godot's tiles editor offers paint (drag), line, rect, bucket, picker and
//! eraser, commits each gesture as ONE undo action, and stores the result on the
//! layer's `tile_map_data` property so it survives a save
//! (`editor/scene/2d/tiles/tile_map_layer_editor.cpp`).
//!
//! Patina had paint and rect as routes with no client caller, no line or bucket
//! at all, and a `tile_grid_store` that was never populated from a scene nor
//! written back to it — so painting into an opened `.tscn` was discarded twice
//! over: once because the grid did not exist, and again because nothing
//! persisted it.

use gdeditor::editor_server::{EditorServerHandle, EditorState};
use gdscene::node::Node;
use gdscene::scene_saver::TscnSaver;
use gdscene::tilemap::TileGrid;
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

/// A `Main -> Tiles` scene with an 8x8 grid already registered, so the tests
/// exercise the routes rather than grid bootstrapping.
fn server() -> (EditorServerHandle, u16, u64) {
    gdobject::class_db::register_2d_classes();

    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let main = tree.add_child(root, Node::new("Main", "Node2D")).unwrap();
    let layer = tree
        .add_child(main, Node::new("Tiles", "TileMapLayer"))
        .unwrap();
    let raw = layer.raw();

    let port = free_port();
    let mut state = EditorState::new(tree);
    state.tile_grid_store.insert(layer, TileGrid::new(8, 8));
    let handle = EditorServerHandle::start(port, state);
    thread::sleep(Duration::from_millis(100));
    (handle, port, raw)
}

/// Reads the whole grid back through the public route.
fn cells(port: u16, raw: u64) -> Vec<i64> {
    let data = json_of(&http_get(port, &format!("/api/tilemap/data?node_id={raw}")));
    data["cells"]
        .as_array()
        .unwrap_or_else(|| panic!("no cells in {data}"))
        .iter()
        .map(|v| v.as_i64().unwrap_or(0))
        .collect()
}

fn cell_at(port: u16, raw: u64, x: usize, y: usize) -> i64 {
    let data = json_of(&http_get(port, &format!("/api/tilemap/data?node_id={raw}")));
    let w = data["width"].as_u64().unwrap() as usize;
    data["cells"].as_array().unwrap()[y * w + x]
        .as_i64()
        .unwrap_or(0)
}

/// **The acceptance test.**
#[test]
fn tilemap_paint_tools_e2e() {
    let (handle, port, raw) = server();

    // -- bucket-fill an empty 8x8 region, then undo ---------------------------
    let filled = json_of(&http_post(
        port,
        "/api/tilemap/fill",
        &format!(r#"{{"node_id":{raw},"mode":"bucket","x":0,"y":0,"tile_id":3}}"#),
    ));
    assert_eq!(filled["ok"], true, "bucket fill failed: {filled}");
    assert_eq!(
        filled["cells"], 64,
        "an empty 8x8 grid floods to all 64 cells: {filled}"
    );
    assert!(
        cells(port, raw).iter().all(|&c| c == 3),
        "every cell must carry the filled tile"
    );

    // ⚠️ ONE undo, not 64. The flood is a `Group`, so it reverses as a unit.
    let undone = json_of(&http_post(port, "/api/undo", "{}"));
    assert_eq!(undone["ok"], true, "undo failed: {undone}");
    assert!(
        cells(port, raw).iter().all(|&c| c == 0),
        "a single undo must restore the whole flood, not one cell of it"
    );

    // -- erase removes a painted cell -----------------------------------------
    http_post(
        port,
        "/api/tilemap/paint",
        &format!(r#"{{"node_id":{raw},"x":2,"y":3,"tile_id":5}}"#),
    );
    assert_eq!(cell_at(port, raw, 2, 3), 5, "the cell was painted");
    let erased = json_of(&http_post(
        port,
        "/api/tilemap/erase",
        &format!(r#"{{"node_id":{raw},"x":2,"y":3}}"#),
    ));
    assert_eq!(erased["ok"], true, "erase failed: {erased}");
    assert_eq!(cell_at(port, raw, 2, 3), 0, "erase must clear the cell");

    // -- a rect drag is ONE history entry -------------------------------------
    let depth_before = {
        let state = handle.state();
        let state = state.lock().unwrap();
        state.undo_stack.len()
    };
    let rect = json_of(&http_post(
        port,
        "/api/tilemap/fill",
        &format!(r#"{{"node_id":{raw},"x1":1,"y1":1,"x2":4,"y2":2,"tile_id":7}}"#),
    ));
    assert_eq!(rect["ok"], true, "rect fill failed: {rect}");
    {
        let state = handle.state();
        let state = state.lock().unwrap();
        assert_eq!(
            state.undo_stack.len() - depth_before,
            1,
            "a rect drag must push exactly ONE history entry, not one per cell"
        );
    }
    // 4 wide x 2 tall = 8 cells.
    assert_eq!(cell_at(port, raw, 1, 1), 7);
    assert_eq!(cell_at(port, raw, 4, 2), 7);
    assert_eq!(cell_at(port, raw, 5, 2), 0, "the rect must not overrun");

    // -- a line drag is also ONE entry, and is connected -----------------------
    let line = json_of(&http_post(
        port,
        "/api/tilemap/line",
        &format!(r#"{{"node_id":{raw},"x1":0,"y1":6,"x2":3,"y2":7,"tile_id":8}}"#),
    ));
    assert_eq!(line["ok"], true, "line failed: {line}");
    assert_eq!(
        line["cells"], 4,
        "a 4-wide Bresenham run covers 4 cells: {line}"
    );
    assert_eq!(cell_at(port, raw, 0, 6), 8, "the line includes its start");
    assert_eq!(cell_at(port, raw, 3, 7), 8, "…and its end");

    // -- the cells round-trip through the saved .tscn --------------------------
    //
    // ⚠️ This is the clause the whole feature hangs on: `tile_map_data` is
    // Godot's own PackedByteArray format, so the existing writer emits it and
    // the existing loader parses it — a Patina-only property would "persist"
    // just as well and be unreadable by Godot.
    let saved = {
        let state = handle.state();
        let state = state.lock().unwrap();
        let root = state.scene_tree.root_id();
        TscnSaver::save_tree(&state.scene_tree, root)
    };
    assert!(
        saved.contains("tile_map_data = PackedByteArray("),
        "painted cells must be written to the layer's tile_map_data:\n{saved}"
    );

    let scene = gdscene::packed_scene::PackedScene::from_tscn(&saved)
        .unwrap_or_else(|e| panic!("saved scene must re-parse ({e:?}):\n{saved}"));
    let mut reloaded = SceneTree::new();
    let rroot = reloaded.root_id();
    let scene_root = gdscene::packed_scene::add_packed_scene_to_tree(&mut reloaded, rroot, &scene)
        .expect("scene instantiates");

    let mut ids = Vec::new();
    reloaded.collect_subtree_top_down(scene_root, &mut ids);
    let tiles = ids
        .into_iter()
        .find(|id| {
            reloaded
                .get_node(*id)
                .map(|n| n.name() == "Tiles")
                .unwrap_or(false)
        })
        .expect("the layer survives the round trip");

    let bytes = match reloaded
        .get_node(tiles)
        .map(|n| n.get_property("tile_map_data"))
    {
        Some(Variant::PackedByteArray(b)) => b,
        other => panic!("tile_map_data should be a PackedByteArray, got {other:?}"),
    };
    let grid = TileGrid::from_tile_map_data(&bytes, 8, 8);
    assert_eq!(grid.get(1, 1), Some(7), "the rect survives the save/load");
    assert_eq!(grid.get(4, 2), Some(7));
    assert_eq!(grid.get(0, 6), Some(8), "the line survives too");
    assert_eq!(
        grid.get(2, 3),
        Some(0),
        "an erased cell must stay erased — a present record would resurrect it"
    );
}

/// The tools are client-side, so the bead's toolbar clause is asserted against
/// the served document: routes with no caller were the exact failure here, with
/// erase and fill tested server-side and unreachable from the editor.
#[test]
fn tilemap_tools_are_wired_in_editor_html() {
    let html = gdeditor::editor_ui::EDITOR_HTML;

    for tool in ["paint", "erase", "bucket", "rect", "line", "picker"] {
        assert!(
            html.contains(&format!(r#"data-tool="{tool}""#)),
            "the {tool} tool needs a toolbar button"
        );
    }

    // Every route the toolbar depends on has a caller.
    for route in [
        "/api/tilemap/paint",
        "/api/tilemap/erase",
        "/api/tilemap/fill",
        "/api/tilemap/line",
        "/api/tilemap/data?node_id=",
    ] {
        assert!(html.contains(route), "no client calls {route}");
    }

    assert!(
        html.contains("mode: 'bucket'"),
        "the bucket tool must ask the fill route for a flood, not a rectangle"
    );
    // Drag painting and the deferred rect/line commit.
    assert!(
        html.contains("tileDragAnchor") && html.contains("tileStrokeSeen"),
        "a held drag must paint each newly-entered cell once"
    );
    assert!(
        html.contains("function tileToolUp"),
        "rect and line must commit on release so the drag is one action"
    );
    // Flip/rotate ride on Godot's alternative-tile transform bits.
    assert!(
        html.contains("TILE_FLIP_H") && html.contains("TILE_TRANSPOSE"),
        "flip/rotate must set the alternative-tile transform flags"
    );
    assert!(
        html.contains("wireTileTools(); // pat-0q2kx.75"),
        "the toolbar must be wired during editor boot"
    );
}

/// Flip/rotate ride on Godot's alternative-tile id, so they have to reach the
/// saved bytes — a toolbar that posted them into a server that dropped them
/// would look wired and change nothing.
#[test]
fn flip_and_rotate_persist_as_alternative_tile_ids() {
    let (handle, port, raw) = server();

    // Godot's transform bits: FLIP_H = 1, FLIP_V = 2, TRANSPOSE = 4.
    let flipped = json_of(&http_post(
        port,
        "/api/tilemap/paint",
        &format!(r#"{{"node_id":{raw},"x":1,"y":1,"tile_id":4,"alternative":3}}"#),
    ));
    assert_eq!(flipped["ok"], true, "paint failed: {flipped}");

    // A plain paint alongside it, to prove the bits are per-cell.
    http_post(
        port,
        "/api/tilemap/paint",
        &format!(r#"{{"node_id":{raw},"x":2,"y":1,"tile_id":4}}"#),
    );

    let bytes = {
        let state = handle.state();
        let state = state.lock().unwrap();
        let nid = state
            .scene_tree
            .get_node_by_path("/root/Main/Tiles")
            .expect("layer exists");
        match state
            .scene_tree
            .get_node(nid)
            .map(|n| n.get_property("tile_map_data"))
        {
            Some(Variant::PackedByteArray(b)) => b,
            other => panic!("expected PackedByteArray, got {other:?}"),
        }
    };

    let cells = gdscene::tilemap::decode_tile_map_data(&bytes);
    let flipped_cell = cells
        .get(&gdcore::math::Vector2i::new(1, 1))
        .expect("the flipped cell is in the saved data");
    assert_eq!(
        flipped_cell.alternative_id, 3,
        "H+V flip must survive into tile_map_data"
    );
    assert_eq!(flipped_cell.source_id, 4, "…without disturbing the tile id");

    let plain = cells
        .get(&gdcore::math::Vector2i::new(2, 1))
        .expect("the plain cell is in the saved data");
    assert_eq!(
        plain.alternative_id, 0,
        "the transform must be per-cell, not sticky across paints"
    );

    // Erasing sheds the transform, so repainting does not inherit it.
    http_post(
        port,
        "/api/tilemap/erase",
        &format!(r#"{{"node_id":{raw},"x":1,"y":1}}"#),
    );
    http_post(
        port,
        "/api/tilemap/paint",
        &format!(r#"{{"node_id":{raw},"x":1,"y":1,"tile_id":4}}"#),
    );
    let bytes2 = {
        let state = handle.state();
        let state = state.lock().unwrap();
        let nid = state
            .scene_tree
            .get_node_by_path("/root/Main/Tiles")
            .expect("layer exists");
        match state
            .scene_tree
            .get_node(nid)
            .map(|n| n.get_property("tile_map_data"))
        {
            Some(Variant::PackedByteArray(b)) => b,
            other => panic!("expected PackedByteArray, got {other:?}"),
        }
    };
    assert_eq!(
        gdscene::tilemap::decode_tile_map_data(&bytes2)
            .get(&gdcore::math::Vector2i::new(1, 1))
            .map(|c| c.alternative_id),
        Some(0),
        "a repainted cell must not inherit the erased tile's transform"
    );
}
