//! pat-nzgdu.34: served TileMap paint UI wired to the /api/tilemap handlers.
//!
//! The bug this guards: Patina already had server-side tile ops (build real
//! `EditorCommand`s: paint/erase/fill/data/tileset) but *nothing in the served
//! editor exposed them* — `EDITOR_HTML` had no tile palette, no active-tile
//! state, and issued no `/api/tilemap` request.
//!
//! Two guards:
//!  1. Static — the served `EDITOR_HTML` references the tileset + paint
//!     endpoints and defines the palette / active-tile / paint wiring.
//!  2. Functional — boot the real editor server over HTTP with a scene that
//!     contains a `TileMapLayer` (its tile grid pre-seeded, as a loaded scene
//!     with tile data would have), drive `POST /api/tilemap/paint`, then
//!     `GET /api/tilemap/data` and assert the painted cell round-trips through
//!     the live handlers. `GET /api/tilemap/tileset` (the palette's source) is
//!     also probed and must expose tiles.

use gdeditor::editor_server::{EditorServerHandle, EditorState};
use gdeditor::editor_ui::EDITOR_HTML;
use gdscene::node::Node;
use gdscene::tilemap::TileGrid;
use gdscene::SceneTree;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;
use std::time::Duration;

// ---------------------------------------------------------------------------
// (1) Static wiring guard — no server boot.
// ---------------------------------------------------------------------------

#[test]
fn editor_html_wires_tilemap_paint_palette() {
    // The served editor must call both endpoints the palette/paint flow needs.
    assert!(
        EDITOR_HTML.contains("/api/tilemap/tileset"),
        "EDITOR_HTML must GET /api/tilemap/tileset to build the TileSet palette"
    );
    assert!(
        EDITOR_HTML.contains("/api/tilemap/paint"),
        "EDITOR_HTML must POST /api/tilemap/paint to paint the active tile"
    );
    // The palette panel + active-tile state + paint wiring must be present.
    assert!(
        EDITOR_HTML.contains("id=\"tilemap-palette\""),
        "EDITOR_HTML must contain the TileSet palette panel"
    );
    assert!(
        EDITOR_HTML.contains("buildTilePalette"),
        "EDITOR_HTML must define buildTilePalette() to render the palette"
    );
    assert!(
        EDITOR_HTML.contains("activeTileId"),
        "EDITOR_HTML must track the active tile id for painting"
    );
    assert!(
        EDITOR_HTML.contains("paintTileAtEvent"),
        "EDITOR_HTML must wire viewport clicks to a paint action"
    );
}

// ---------------------------------------------------------------------------
// (2) Functional round-trip — boots the real EditorServerHandle over HTTP.
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
        &format!("GET {path} HTTP/1.1\r\nHost: localhost\r\n\r\n"),
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

#[test]
fn painting_a_tilemaplayer_round_trips_through_the_live_handlers() {
    let port = free_port();

    // Scene: root -> TileMapLayer "Tiles". Capture the node id before the move.
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let tiles_node = Node::new("Tiles", "TileMapLayer");
    let tnid = tiles_node.id();
    tree.add_child(root, tiles_node).unwrap();

    // A TileMapLayer loaded from a scene carries a tile grid; seed a small one so
    // the live paint/data handlers (which operate on the node's grid) have
    // something to act on. This mirrors post-scene-load state.
    let mut state = EditorState::new(tree);
    state.tile_grid_store.insert(tnid, TileGrid::new(16, 16));

    let handle = EditorServerHandle::start(port, state);
    thread::sleep(Duration::from_millis(100));

    let raw = tnid.raw();

    // Sanity: the scene exposes the TileMapLayer under the same raw id the
    // /api/tilemap handlers resolve via find_node_by_raw_id.
    let scene = http_get(port, "/api/scene");
    let sv: serde_json::Value =
        serde_json::from_str(extract_body(&scene)).expect("/api/scene should return JSON");
    assert_eq!(
        sv["nodes"]["children"][0]["id"].as_u64(),
        Some(raw),
        "the TileMapLayer should be exposed under its raw id; scene: {sv}"
    );

    // Paint tile 5 at cell (2, 3) through the live handler.
    let paint_body = format!(r#"{{"node_id":{raw},"x":2,"y":3,"tile_id":5}}"#);
    let paint_resp = http_post(port, "/api/tilemap/paint", &paint_body);
    assert!(
        paint_resp.contains("200 OK"),
        "POST /api/tilemap/paint should return 200, got:\n{paint_resp}"
    );

    // Read the grid back and assert the painted cell persisted.
    let data_resp = http_get(port, &format!("/api/tilemap/data?node_id={raw}"));
    assert!(
        data_resp.contains("200 OK"),
        "GET /api/tilemap/data should return 200, got:\n{data_resp}"
    );
    let dv: serde_json::Value =
        serde_json::from_str(extract_body(&data_resp)).expect("/api/tilemap/data should be JSON");
    let w = dv["width"].as_u64().expect("grid json has width") as usize;
    let cells = dv["cells"].as_array().expect("grid json has cells array");
    let idx = 3 * w + 2; // row-major: cells[y * width + x]
    assert_eq!(
        cells[idx].as_i64(),
        Some(5),
        "painted cell (2,3) should read back as tile 5; grid: {dv}"
    );

    // The palette's data source must expose the default tileset's tiles.
    let ts_resp = http_get(port, "/api/tilemap/tileset");
    assert!(
        ts_resp.contains("200 OK"),
        "GET /api/tilemap/tileset should return 200, got:\n{ts_resp}"
    );
    let tv: serde_json::Value =
        serde_json::from_str(extract_body(&ts_resp)).expect("/api/tilemap/tileset should be JSON");
    assert!(
        tv["tiles"]
            .as_array()
            .map(|a| !a.is_empty())
            .unwrap_or(false),
        "the default tileset should expose tiles for the palette; got: {tv}"
    );

    handle.stop();
}
