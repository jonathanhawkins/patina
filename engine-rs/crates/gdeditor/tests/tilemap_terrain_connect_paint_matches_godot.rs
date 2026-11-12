//! pat-mgixk.112 acceptance: terrain sets and Connect-mode terrain painting in
//! the served editor, plus TileMapPattern capture and stamping.
//!
//! # Why the assertions look the way they do
//!
//! The terrain solver already lives in `gdscene::tilemap` — `TerrainMode`,
//! `TerrainSet`/`TerrainType` with bitmask→tile maps, `compute_bitmask`,
//! `resolve_terrain_tile` and `paint_terrain`, which *is* Connect mode (it
//! assigns the terrain, then recomputes the bitmask for the painted cells **and
//! every affected neighbour**). What was missing is the editor: `grep -ci
//! terrain` was 0 in `editor_server.rs`, `editor_ui.rs` and
//! `tilemap_editor.rs`. So this test drives the new routes, not the solver.
//!
//! ## The invariant, and why it is not vacuous
//!
//! `MatchSides` numbers its neighbours bit0=Top, bit1=Right, bit2=Bottom,
//! bit3=Left (`NEIGHBOR_OFFSETS_4_SIDES`). Painting a 3x3 block with one
//! terrain therefore produces these bitmasks — computed from that offset table
//! before this test was written:
//!
//! ```text
//!   (0,0)= 6   (1,0)=14   (2,0)=12
//!   (0,1)= 7   (1,1)=15   (2,1)=13
//!   (0,2)= 3   (1,2)=11   (2,2)= 9
//! ```
//!
//! Nine distinct masks, with the centre at 15 (`0b1111`) — the "full-centre
//! tile" the bead describes. 🔑 The fixture registers each mask against the
//! atlas cell at the *same position*, so a correct Connect paint must resolve
//! **cell (x, y) to atlas (x, y)** for all nine. That identity is the whole
//! assertion.
//!
//! ⚠️ It has to be the identity and not "a tile came back", because
//! `resolve_terrain_tile` falls back to the best *partial* match: a wrong bit
//! order or a wrong neighbour set still returns *some* tile. The centre is also
//! pinned to (1,1) and an edge pinned to **not** (1,1), so a resolver stuck on
//! a single entry fails rather than passing nine times over.

use gdeditor::editor_server::{EditorServerHandle, EditorState};
use gdscene::node::Node;
use gdscene::tilemap::TileGrid;
use gdscene::SceneTree;
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

fn ok(resp: &str, what: &str) -> serde_json::Value {
    assert!(
        resp.contains("200 OK"),
        "{what} should return 200, got:\n{resp}"
    );
    serde_json::from_str(extract_body(resp))
        .unwrap_or_else(|e| panic!("{what} should be JSON: {e}"))
}

/// The nine `MatchSides` bitmasks of a 3x3 block, by cell.
///
/// Derived from `NEIGHBOR_OFFSETS_4_SIDES` (T, R, B, L = bits 0..3), not
/// guessed: a cell's bit is set when the neighbour in that direction carries
/// the same terrain, so an interior cell of the block has all four.
const BLOCK_MASKS: [[u8; 3]; 3] = [
    [6, 14, 12], // y = 0
    [7, 15, 13], // y = 1
    [3, 11, 9],  // y = 2
];

#[test]
fn tilemap_terrain_connect_paint_matches_godot() {
    let port = free_port();

    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let tiles = Node::new("Tiles", "TileMapLayer");
    let tnid = tiles.id();
    tree.add_child(root, tiles).unwrap();

    let mut state = EditorState::new(tree);
    state.tile_grid_store.insert(tnid, TileGrid::new(16, 16));

    let handle = EditorServerHandle::start(port, state);
    thread::sleep(Duration::from_millis(100));
    let raw = tnid.raw();

    // -- (1) define terrain set 0 in Match Sides mode -----------------------
    //
    // ⚠️ Mode is named, not defaulted. `TerrainMode`'s own default is
    // `MatchCornersAndSides`, an 8-bit mask, so leaving it off would make every
    // expected bitmask below wrong while still producing plausible tiles.
    let set_resp = http_post(
        port,
        "/api/tileset/terrain_set/add",
        r#"{"mode":"match_sides"}"#,
    );
    let sv = ok(&set_resp, "POST /api/tileset/terrain_set/add");
    let set_id = sv["terrain_set"].as_i64().unwrap_or(0);

    let terrain_resp = http_post(
        port,
        "/api/tileset/terrain/add",
        &format!(r#"{{"terrain_set":{set_id},"id":0,"name":"grass"}}"#),
    );
    ok(&terrain_resp, "POST /api/tileset/terrain/add");

    // -- (2) paint the peering bits of a 3x3 autotile atlas -----------------
    //
    // Each bitmask is registered against the atlas cell at the same position,
    // which is what makes the identity below assertable.
    for y in 0..3i32 {
        for x in 0..3i32 {
            let mask = BLOCK_MASKS[y as usize][x as usize];
            let body = format!(
                r#"{{"terrain_set":{set_id},"terrain":0,"bitmask":{mask},"source_id":1,"atlas_coords":[{x},{y}]}}"#
            );
            let r = http_post(port, "/api/tileset/tile/peering", &body);
            assert!(
                r.contains("200 OK"),
                "POST /api/tileset/tile/peering for mask {mask} should return 200, got:\n{r}"
            );
        }
    }

    // -- (3) Connect-paint the 3x3 block ------------------------------------
    let cells: Vec<String> = (0..3)
        .flat_map(|y| (0..3).map(move |x| format!("[{x},{y}]")))
        .collect();
    let paint_body = format!(
        r#"{{"node_id":{raw},"terrain_set":{set_id},"terrain":0,"cells":[{}]}}"#,
        cells.join(",")
    );
    let paint_resp = http_post(port, "/api/tilemap/terrain_paint", &paint_body);
    let pv = ok(&paint_resp, "POST /api/tilemap/terrain_paint");

    // The response reports what it resolved, so a silent no-op is visible as a
    // count rather than only as nine wrong cells later.
    assert_eq!(
        pv["cells"].as_array().map(|a| a.len()),
        Some(9),
        "Connect-painting a 3x3 block must resolve all nine cells; got: {pv}"
    );

    // -- (4) THE INVARIANT: cell (x, y) resolved to atlas (x, y) ------------
    let data = ok(
        &http_get(port, &format!("/api/tilemap/data?node_id={raw}")),
        "GET /api/tilemap/data",
    );
    let w = data["width"].as_u64().expect("grid json has width") as usize;
    let grid = data["cells"].as_array().expect("grid json has cells array");
    // 🔑 `atlas_coords` is decoded from the node's own `tile_map_data`
    // property, not read out of an editor-side cache — so this map arriving
    // correctly IS the round trip the bead asks for, and it is the half the
    // solver being right cannot fake.
    //
    // ⚠️ `TileGrid` is a flat `Vec<i32>` of SOURCE ids with nowhere to put
    // atlas coords, and `writeback_tile_grid` built every cell as
    // `TileData::new(src)`. Until that used `TileData::with_atlas`, the
    // coordinates autotiling computes were dropped on write and the layer
    // reloaded as nine copies of one tile — a defect no assertion on the
    // solver's return value would have caught.
    let atlas = data["atlas_coords"]
        .as_object()
        .expect("grid json must report per-cell atlas coords, decoded from tile_map_data");

    let mut centre_atlas = None;
    for y in 0..3usize {
        for x in 0..3usize {
            assert_eq!(
                grid[y * w + x].as_i64(),
                Some(1),
                "cell ({x},{y}) must carry source id 1 after the terrain paint; grid: {data}"
            );
            let got = atlas
                .get(&format!("{x},{y}"))
                .and_then(|v| v.as_array())
                .map(|a| (a[0].as_i64().unwrap_or(-1), a[1].as_i64().unwrap_or(-1)))
                .unwrap_or_else(|| panic!("cell ({x},{y}) must report atlas coords; got: {data}"));
            assert_eq!(
                got,
                (x as i64, y as i64),
                "🔑 cell ({x},{y}) has MatchSides bitmask {} and must resolve to the \
                 atlas cell registered for it, ({x},{y}); got {got:?}. A different \
                 coordinate means the bit order, the neighbour set, or the \
                 dirty-neighbour sweep is wrong — `resolve_terrain_tile` falls back \
                 to a partial match, so a wrong mask still returns a tile",
                BLOCK_MASKS[y][x]
            );
            if (x, y) == (1, 1) {
                centre_atlas = Some(got);
            }
        }
    }
    assert_eq!(
        centre_atlas,
        Some((1, 1)),
        "the centre cell's bitmask is 15 (all four sides matching), so it must \
         take the full-centre tile"
    );
    // The control: an edge must NOT be the centre tile. A resolver that
    // answered one entry for everything would satisfy every assertion above.
    let edge = atlas
        .get("0,0")
        .and_then(|v| v.as_array())
        .map(|a| (a[0].as_i64().unwrap_or(-1), a[1].as_i64().unwrap_or(-1)));
    assert_ne!(
        edge,
        Some((1, 1)),
        "the corner cell has bitmask 6, not 15, and must not take the centre tile"
    );

    // -- (5) patterns: capture 2x2, stamp it elsewhere ----------------------
    let create = ok(
        &http_post(
            port,
            "/api/tilemap/pattern/create",
            &format!(r#"{{"node_id":{raw},"rect":[0,0,2,2]}}"#),
        ),
        "POST /api/tilemap/pattern/create",
    );
    let pattern = create["pattern"].as_i64().unwrap_or(0);
    assert_eq!(
        create["size"]
            .as_array()
            .map(|a| (a[0].as_i64().unwrap_or(-1), a[1].as_i64().unwrap_or(-1))),
        Some((2, 2)),
        "a pattern captured from a 2x2 rect must report size 2x2; got: {create}"
    );

    ok(
        &http_post(
            port,
            "/api/tilemap/pattern/paint",
            &format!(r#"{{"node_id":{raw},"pattern":{pattern},"at":[8,8]}}"#),
        ),
        "POST /api/tilemap/pattern/paint",
    );

    let after = ok(
        &http_get(port, &format!("/api/tilemap/data?node_id={raw}")),
        "GET /api/tilemap/data after the stamp",
    );
    let grid2 = after["cells"].as_array().expect("cells array");
    let atlas2 = after["atlas_coords"].as_object().expect("atlas map");
    // 🔑 The stamped cells must match the SOURCE cells, atlas coords included —
    // a pattern that carried only source ids would put the right tile id at the
    // destination with the wrong picture in it.
    for y in 0..2usize {
        for x in 0..2usize {
            assert_eq!(
                grid2[(8 + y) * w + (8 + x)].as_i64(),
                Some(1),
                "stamped cell ({},{}) must carry the source id from ({x},{y})",
                8 + x,
                8 + y
            );
            assert_eq!(
                atlas2
                    .get(&format!("{},{}", 8 + x, 8 + y))
                    .and_then(|v| v.as_array())
                    .map(|a| (a[0].as_i64().unwrap_or(-1), a[1].as_i64().unwrap_or(-1))),
                Some((x as i64, y as i64)),
                "stamped cell ({},{}) must carry the ATLAS coords of the pattern \
                 cell it came from, ({x},{y}) — a pattern holding only source ids \
                 would stamp the right tile id with the wrong atlas picture",
                8 + x,
                8 + y
            );
        }
    }

    handle.stop();
}

/// Edge: a plain paint over a terrain-resolved cell must not keep that cell's
/// terrain atlas coords — the painted tile is the plain tool's, at atlas (0,0).
#[test]
fn plain_paint_over_a_terrain_cell_drops_its_atlas_coords() {
    let port = free_port();
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let tiles = Node::new("Tiles", "TileMapLayer");
    let tnid = tiles.id();
    tree.add_child(root, tiles).unwrap();
    let mut state = EditorState::new(tree);
    state.tile_grid_store.insert(tnid, TileGrid::new(8, 8));
    let handle = EditorServerHandle::start(port, state);
    thread::sleep(Duration::from_millis(100));
    let raw = tnid.raw();

    let set = ok(
        &http_post(
            port,
            "/api/tileset/terrain_set/add",
            r#"{"mode":"match_sides"}"#,
        ),
        "set",
    );
    let set_id = set["terrain_set"].as_i64().unwrap();
    ok(
        &http_post(
            port,
            "/api/tileset/terrain/add",
            &format!(r#"{{"terrain_set":{set_id},"id":0}}"#),
        ),
        "terrain",
    );
    // A lone cell has mask 0; map it to atlas (2,1).
    ok(
        &http_post(
            port,
            "/api/tileset/tile/peering",
            &format!(
                r#"{{"terrain_set":{set_id},"terrain":0,"bitmask":0,"source_id":1,"atlas_coords":[2,1]}}"#
            ),
        ),
        "peering",
    );
    ok(
        &http_post(
            port,
            "/api/tilemap/terrain_paint",
            &format!(r#"{{"node_id":{raw},"terrain_set":{set_id},"terrain":0,"cells":[[3,3]]}}"#),
        ),
        "paint",
    );
    let data = ok(
        &http_get(port, &format!("/api/tilemap/data?node_id={raw}")),
        "data",
    );
    assert_eq!(
        data["atlas_coords"]["3,3"],
        serde_json::json!([2, 1]),
        "terrain placed (2,1)"
    );

    // Plain paint of a DIFFERENT source over it.
    ok(
        &http_post(
            port,
            "/api/tilemap/paint",
            &format!(r#"{{"node_id":{raw},"x":3,"y":3,"tile_id":2}}"#),
        ),
        "plain paint",
    );
    let data = ok(
        &http_get(port, &format!("/api/tilemap/data?node_id={raw}")),
        "data",
    );
    assert_eq!(
        data["atlas_coords"]["3,3"],
        serde_json::json!([0, 0]),
        "stale terrain atlas must not survive: {data}"
    );
    handle.stop();
}
