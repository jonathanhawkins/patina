//! pat-s3rrv.65 — the TileSet atlas-source editor, served and persisted.
//!
//! # The gap
//!
//! Patina could *paint* tiles but not *define* them. The only tileset endpoint,
//! `GET /api/tilemap/tileset`, reports a flat `{id,name,color,collision}` list
//! from the editor's own `TileGridStore` — a colour palette that has nothing to
//! do with the `TileSet` resource on the selected `TileMapLayer`. No route
//! created an atlas source, and no route set per-tile collision, navigation or
//! custom data. Loading a real Godot project's TileSet therefore could not
//! round-trip through the editor: you could move its tiles around and never
//! change what a tile *was*.
//!
//! # What this pins
//!
//! The full loop, because each half is uninteresting alone: the editor writes
//! the `__tile_set__*` / `__tile_physics_*` companions, they survive a save to
//! `.tscn`, and reloading that file gives the runtime tile collision it already
//! knew how to consume. A test that only checked the HTTP response would pass
//! on an endpoint that stored the value under a name nothing reads.

use gdcore::math::{Vector2, Vector2i};
use gdeditor::editor_server::{EditorServerHandle, EditorState};
use gdeditor::editor_ui::EDITOR_HTML;
use gdscene::node::{Node, NodeId};
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::tilemap::{
    decode_tile_map_data, encode_tile_map_data, tile_navigation_polygons, tile_occluder_polygons,
    tile_physics_one_way_key, TileData, TILE_NAV_POLYGON, TILE_NAV_SOURCES, TILE_OCCLUDER_POLYGON,
    TILE_OCCLUDER_SOURCES, TILE_PHYSICS_SOURCES, TILE_SET_SOURCE_IDS,
};
use gdscene::{MainLoop, SceneTree};
use gdvariant::Variant;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;
use std::time::Duration;

const TILE: i32 = 16;
const SOURCE: i32 = 0;
/// The row of cells painted below the falling body.
const GROUND_ROW: i32 = 1;

// ---------------------------------------------------------------------------
// (1) Static wiring guard — the served editor must actually call the routes.
// ---------------------------------------------------------------------------

#[test]
fn editor_html_wires_the_tileset_editor() {
    // The acceptance clause names this one explicitly: without it the routes
    // exist but nothing in the served editor reaches them, which is the state
    // every other TileSet capability was already in.
    assert!(
        EDITOR_HTML.contains("/api/tileset/tile"),
        "EDITOR_HTML must call /api/tileset/tile to read and write per-tile data"
    );
    assert!(
        EDITOR_HTML.contains("/api/tileset/source"),
        "EDITOR_HTML must call /api/tileset/source to assign an atlas texture"
    );
    assert!(
        EDITOR_HTML.contains("id=\"tileset-editor\""),
        "EDITOR_HTML must contain the TileSet editor panel"
    );
    // The two authoring actions the bead's acceptance walks through.
    assert!(
        EDITOR_HTML.contains("tileset-assign-atlas"),
        "the panel must offer assigning an atlas texture to a source"
    );
    assert!(
        EDITOR_HTML.contains("tileset-full-collision"),
        "the panel must offer setting a tile's collision"
    );
    // ⚠️ A CHECKBOX, not a button. Navigation is the one tile-data control that
    // has to be reversible from the panel: `tile_navigation_polygons` keys off
    // the source list, so a one-way "Nav" button would leave every tile ever
    // marked walkable walkable forever, through every later save.
    assert!(
        EDITOR_HTML.contains("id=\"tileset-navigation\" type=\"checkbox\""),
        "the panel must offer toggling a tile's navigation, in both directions"
    );
    // ⚠️ Occlusion is a CHECKBOX for the same reason navigation is, and the
    // reason is not symmetry: `tile_occluder_polygons` keys off the source
    // list, so a one-way "Occl" button would leave every tile ever marked
    // shadow-casting marked forever, through every later save. The route
    // landed in an earlier pass with no control at all, so the saver and
    // loader could round-trip occlusion that no editor action could author
    // (pat-s3rrv.65).
    assert!(
        EDITOR_HTML.contains("id=\"tileset-occlusion\" type=\"checkbox\""),
        "the panel must offer toggling a tile's occlusion, in both directions"
    );
    assert!(
        EDITOR_HTML.contains("occlusion: !!occl.checked"),
        "the occlusion checkbox must POST its state, not merely exist — a \
         control wired to nothing is the same defect as no control"
    );
}

// ---------------------------------------------------------------------------
// HTTP plumbing (same shape as editor_tilemap_paint_wired_test).
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

fn json_of(resp: &str) -> serde_json::Value {
    serde_json::from_str(extract_body(resp))
        .unwrap_or_else(|e| panic!("expected JSON, got {e}:\n{resp}"))
}

/// A scene whose `TileMapLayer` already has cells painted — the realistic
/// starting point, since you paint with a source before defining what its
/// tiles are.
fn painted_scene() -> (SceneTree, NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let mut painted: HashMap<Vector2i, TileData> = HashMap::new();
    for x in 0..3 {
        painted.insert(Vector2i::new(x, GROUND_ROW), TileData::new(SOURCE));
    }

    let mut layer = Node::new("Layer", "TileMapLayer");
    layer.set_property(
        "tile_map_data",
        Variant::PackedByteArray(encode_tile_map_data(&painted)),
    );
    layer.set_property("tile_size", Variant::Vector2i(Vector2i::new(TILE, TILE)));
    let layer_id = tree.add_child(root, layer).unwrap();
    (tree, layer_id)
}

// ---------------------------------------------------------------------------
// (2) The acceptance test.
// ---------------------------------------------------------------------------

#[test]
fn editor_tileset_atlas_tiledata_served() {
    let port = free_port();
    let (tree, layer_id) = painted_scene();
    let raw = layer_id.raw();
    let handle = EditorServerHandle::start(port, EditorState::new(tree));
    thread::sleep(Duration::from_millis(100));

    // -- assign an atlas source ---------------------------------------------
    let resp = http_post(
        port,
        "/api/tileset/source",
        &format!(
            r#"{{"node_id":{raw},"source_id":{SOURCE},"texture":"res://atlas.png","region_size":[16,16],"tile_size":[16,16]}}"#
        ),
    );
    assert!(
        resp.contains("200 OK"),
        "POST /api/tileset/source should return 200, got:\n{resp}"
    );

    let sources = json_of(&http_get(
        port,
        &format!("/api/tileset/source?node_id={raw}"),
    ));
    assert_eq!(
        sources["sources"][0]["texture"].as_str(),
        Some("res://atlas.png"),
        "the atlas source must read back; got {sources}"
    );
    assert_eq!(
        sources["sources"][0]["region_size"][0].as_i64(),
        Some(16),
        "the atlas region size must round-trip; got {sources}"
    );

    // -- set collision on tile (0, 0) ---------------------------------------
    let resp = http_post(
        port,
        "/api/tileset/tile/data",
        &format!(
            r#"{{"node_id":{raw},"source_id":{SOURCE},"atlas":[0,0],"full_cell_collision":true,"one_way":false,"custom_data":{{"damage":"5"}}}}"#
        ),
    );
    assert!(
        resp.contains("200 OK"),
        "POST /api/tileset/tile/data should return 200, got:\n{resp}"
    );

    let tiles = json_of(&http_get(
        port,
        &format!("/api/tileset/tile?node_id={raw}&source_id={SOURCE}"),
    ));
    let tile = &tiles["tiles"][0];
    assert_eq!(
        tile["atlas"].as_array().map(|a| a.len()),
        Some(2),
        "the edited tile must be reported back; got {tiles}"
    );
    // ⚠️ Points are tile-local and CENTRED. `[0, 0]` as the first point would
    // mean corner-origin authoring, which still produces a collider — offset
    // half a tile down and right — and so looks very nearly correct.
    assert_eq!(
        tile["collision"][0][0].as_f64(),
        Some(-8.0),
        "a full-cell polygon on a 16px tile starts at -8, not 0; got {tiles}"
    );
    assert_eq!(
        tiles["custom_data_layers"][0].as_str(),
        Some("damage"),
        "setting a value must declare its custom data layer, or \
         TileData.get_custom_data(name) can never find it; got {tiles}"
    );

    // -- the edit reaches the saved scene text ------------------------------
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("tileset_scene.tscn");
    let save = http_post(
        port,
        "/api/scene/save",
        &format!(r#"{{"path":"{}"}}"#, path.to_string_lossy()),
    );
    assert!(
        save.contains("200 OK"),
        "POST /api/scene/save should return 200, got:\n{save}"
    );
    let saved = std::fs::read_to_string(&path).expect("the saved scene must exist");

    // ⚠️ The saved file must carry Godot's REAL TileSet shape, not the editor's
    // flat companions. `__tile_set__*` and `__tile_physics_*` are an internal
    // cache under names Godot has never heard of; `is_internal_prop` drops
    // them, and writing them verbatim would produce a file Godot cannot parse.
    // What has to survive is the `[sub_resource]` pair that
    // `packed_scene`'s reader already consumes.
    for needle in [
        "[sub_resource type=\"TileSet\"",
        "[sub_resource type=\"TileSetAtlasSource\"",
        // The tile must be declared before its data, or every `0:0/0/...` key
        // below is orphaned on load.
        "0:0/0 = 0",
        "0:0/0/physics_layer_0/polygon_0/points = PackedVector2Array(",
        // Godot only reads a polygon when its physics layer exists — without
        // this line the collision is written and then ignored, which is the
        // "wired but inert" failure expressed in the file format.
        "physics_layer_0/collision_layer = 1",
        "sources/0 = SubResource(",
        "tile_set = SubResource(",
        "res://atlas.png",
    ] {
        assert!(
            saved.contains(needle),
            "the saved scene must carry `{needle}` — an editor-authored TileSet \
             has no pre-existing SubResource reference to fall back on, so if \
             the saver does not rebuild one the edit is simply dropped.\n{saved}"
        );
    }
    // The flat companions must NOT leak into the file.
    assert!(
        !saved.contains("__tile_physics_"),
        "raw `__tile_physics_*` companions must not reach the .tscn — they are \
         property names Godot cannot parse, and they duplicate the \
         TileSetAtlasSource keys written above.\n{saved}"
    );
    assert!(
        !saved.contains("__tile_set__"),
        "raw `__tile_set__*` cache keys must not reach the .tscn.\n{saved}"
    );

    drop(handle);

    // -- and the runtime honours it -----------------------------------------
    //
    // Reload the file the editor just wrote, rather than hand-building the same
    // properties: only the round trip proves the editor and the runtime agree
    // on the names.
    let scene = PackedScene::from_tscn(&saved).expect("the saved scene must parse");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &scene).expect("the saved scene must instance");

    // ⚠️ THE STEP THAT MAKES IT REAL. `tile_collision_infos` returns nothing at
    // all while `__tile_physics_sources__` is empty, so a perfectly authored
    // polygon produces NO body and the tile reads as "collision does not work".
    let layer = tree
        .find_child(root, "Layer", true)
        .expect("the reloaded scene must still hold the Layer");
    assert!(
        matches!(
            tree.get_node(layer).unwrap().get_property(TILE_PHYSICS_SOURCES),
            Variant::PackedInt32Array(ref v) if v.contains(&SOURCE)
        ),
        "setting tile collision must register the source in {TILE_PHYSICS_SOURCES}; \
         without it the polygon is stored, saved and reloaded but generates no body"
    );
    assert!(
        matches!(
            tree.get_node(layer).unwrap().get_property(TILE_SET_SOURCE_IDS),
            Variant::PackedInt32Array(ref v) if v.contains(&SOURCE)
        ),
        "assigning an atlas must register the source in {TILE_SET_SOURCE_IDS}, \
         which is the list tile_set_from_properties iterates"
    );
    assert!(
        matches!(
            tree.get_node(layer)
                .unwrap()
                .get_property(&tile_physics_one_way_key(SOURCE, Vector2i::ZERO)),
            Variant::Bool(false)
        ),
        "the one-way flag must survive the save as a real bool"
    );

    // A body dropped onto the painted row must land on it.
    let mut walker = Node::new("Walker", "CharacterBody2D");
    walker.set_property("position", Variant::Vector2(Vector2::new(24.0, 0.0)));
    walker.set_property("velocity", Variant::Vector2(Vector2::new(0.0, 200.0)));
    let walker_id = tree.add_child(root, walker).unwrap();
    let mut shape = Node::new("WalkerShape", "CollisionShape2D");
    shape.set_property("shape", Variant::String("RectangleShape2D".to_owned()));
    shape.set_property("size", Variant::Vector2(Vector2::new(16.0, 16.0)));
    tree.add_child(walker_id, shape).unwrap();

    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();
    for _ in 0..60 {
        ml.step(1.0 / 60.0);
    }
    let y = gdscene::node2d::get_position(ml.tree(), walker_id).y;
    // One second of 200 px/s would reach y ≈ 200 unobstructed; the tile row's
    // top surface is at y = 16.
    assert!(
        y < 40.0,
        "the body must come to rest on the tile row it was given collision for, \
         but fell to y = {y}. Falling straight through means the authored \
         collision never became a body — check {TILE_PHYSICS_SOURCES} first, \
         since an empty list short-circuits tile_collision_infos before it ever \
         looks at the polygon."
    );
}

// ---------------------------------------------------------------------------
// (3) Navigation, authored through the same route.
// ---------------------------------------------------------------------------

/// The navigation half of the TileData surface, end to end: the route sets it,
/// the saver writes Godot's keys, the loader reads them back, and the runtime
/// produces a walkable region per painted cell.
///
/// Navigation was the piece stage 1 explicitly did not deliver — the companions
/// existed and were filtered out of the file, but nothing authored them and
/// nothing wrote them into the TileSet, so enabling navigation on a tile was
/// dropped on save.
#[test]
fn editor_tileset_tile_navigation_served() {
    let port = free_port();
    let (tree, layer_id) = painted_scene();
    let raw = layer_id.raw();
    let handle = EditorServerHandle::start(port, EditorState::new(tree));
    thread::sleep(Duration::from_millis(100));

    let resp = http_post(
        port,
        "/api/tileset/source",
        &format!(
            r#"{{"node_id":{raw},"source_id":{SOURCE},"texture":"res://atlas.png","region_size":[16,16],"tile_size":[16,16]}}"#
        ),
    );
    assert!(resp.contains("200 OK"), "atlas source POST failed:\n{resp}");

    // ⚠️ The polygon is tile-local from the TOP-LEFT corner — the left half of
    // a 16x16 tile. Collision polygons on the same route are CENTRED; sending
    // centred points here would place the walkable strip half a tile off.
    let resp = http_post(
        port,
        "/api/tileset/tile/data",
        &format!(
            r#"{{"node_id":{raw},"source_id":{SOURCE},"atlas":[0,0],"navigation":true,"navigation_polygon":[[0,0],[8,0],[8,16],[0,16]]}}"#
        ),
    );
    assert!(
        resp.contains("200 OK"),
        "POST /api/tileset/tile/data with navigation should return 200, got:\n{resp}"
    );

    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("tileset_nav_scene.tscn");
    let save = http_post(
        port,
        "/api/scene/save",
        &format!(r#"{{"path":"{}"}}"#, path.to_string_lossy()),
    );
    assert!(save.contains("200 OK"), "scene save failed:\n{save}");
    let saved = std::fs::read_to_string(&path).expect("the saved scene must exist");

    for needle in [
        // Godot only reads a tile's polygon when the layer exists on the
        // TileSet — the navigation twin of `physics_layer_0/collision_layer`.
        "navigation_layer_0/layers = 1",
        // ⚠️ A SubResource reference, not an inline array: Godot's TileData
        // holds a NavigationPolygon RESOURCE, and a raw array converts to a
        // null Ref — accepted without error, geometry dropped.
        "0:0/0/navigation_layer_0/polygon = SubResource(",
        "[sub_resource type=\"NavigationPolygon\"",
    ] {
        assert!(
            saved.contains(needle),
            "the saved scene must carry `{needle}`; without it the authored \
             navigation layer is dropped on save.\n{saved}"
        );
    }
    assert!(
        !saved.contains("__tile_nav_"),
        "the flat navigation companions must not reach the .tscn.\n{saved}"
    );

    drop(handle);

    // -- reload, and check the runtime honours it ---------------------------
    let scene = PackedScene::from_tscn(&saved).expect("the saved scene must parse");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &scene).expect("the saved scene must instance");
    let layer = tree
        .find_child(root, "Layer", true)
        .expect("the reloaded scene must still hold the Layer");
    let node = tree.get_node(layer).expect("layer node");

    // ⚠️ THE STEP THAT MAKES IT REAL, the navigation twin of the physics source
    // registration: `tile_navigation_polygons` returns NOTHING while the source
    // list is empty, so a perfectly authored polygon yields no walkable region
    // at all.
    assert!(
        matches!(
            node.get_property(TILE_NAV_SOURCES),
            Variant::PackedInt32Array(ref v) if v.contains(&SOURCE)
        ),
        "enabling navigation must register the source in {TILE_NAV_SOURCES}"
    );

    // Back in TOP-LEFT coordinates, not the centred form the file holds.
    assert_eq!(
        node.get_property(TILE_NAV_POLYGON),
        Variant::PackedVector2Array(vec![
            Vector2::new(0.0, 0.0),
            Vector2::new(8.0, 0.0),
            Vector2::new(8.0, 16.0),
            Vector2::new(0.0, 16.0),
        ]),
        "the polygon must survive the round trip in the editor's own coordinates"
    );

    let cells = match node.get_property("tile_map_data") {
        Variant::PackedByteArray(bytes) => decode_tile_map_data(&bytes),
        other => panic!("the painted cells must survive the save, got {other:?}"),
    };
    let regions = tile_navigation_polygons(
        &cells,
        Vector2i::new(TILE, TILE),
        &[SOURCE],
        &[
            Vector2::new(0.0, 0.0),
            Vector2::new(8.0, 0.0),
            Vector2::new(8.0, 16.0),
            Vector2::new(0.0, 16.0),
        ],
        Vector2::ZERO,
    );
    assert_eq!(
        regions.len(),
        3,
        "one walkable region per painted cell on the ground row"
    );
    // The first region sits on cell (0, GROUND_ROW): its left edge is the cell's
    // own left edge, and it is half a tile wide because that is what was
    // authored.
    assert_eq!(
        regions[0][0],
        Vector2::new(0.0, GROUND_ROW as f32 * TILE as f32)
    );
    assert_eq!(
        regions[0][1],
        Vector2::new(8.0, GROUND_ROW as f32 * TILE as f32)
    );
}

// ---------------------------------------------------------------------------

/// The occlusion half of the TileData surface, end to end.
///
/// The saver and the loader have handled tile occlusion since stage 1 — but the
/// route did not, so a scene could *carry* occlusion that no editor action could
/// author. `grep -c occluder editor_server.rs` was 0 while the same grep over
/// `scene_saver.rs` was 10 (pat-s3rrv.65).
#[test]
fn editor_tileset_tile_occlusion_served() {
    let port = free_port();
    let (tree, layer_id) = painted_scene();
    let raw = layer_id.raw();
    let handle = EditorServerHandle::start(port, EditorState::new(tree));
    thread::sleep(Duration::from_millis(100));

    let resp = http_post(
        port,
        "/api/tileset/source",
        &format!(
            r#"{{"node_id":{raw},"source_id":{SOURCE},"texture":"res://atlas.png","region_size":[16,16],"tile_size":[16,16]}}"#
        ),
    );
    assert!(resp.contains("200 OK"), "atlas source POST failed:\n{resp}");

    // ⚠️ Top-left relative, like the navigation polygon and UNLIKE the collision
    // points on this same route, which are tile-centred. `tilemap.rs` documents
    // the occluder as the navigation twin deliberately; sending centred points
    // casts the shadow half a tile off.
    let resp = http_post(
        port,
        "/api/tileset/tile/data",
        &format!(
            r#"{{"node_id":{raw},"source_id":{SOURCE},"atlas":[0,0],"occlusion":true,"occluder_polygon":[[0,0],[8,0],[8,16],[0,16]]}}"#
        ),
    );
    assert!(
        resp.contains("200 OK"),
        "POST /api/tileset/tile/data with occlusion should return 200, got:\n{resp}"
    );

    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("tileset_occlusion_scene.tscn");
    let save = http_post(
        port,
        "/api/scene/save",
        &format!(r#"{{"path":"{}"}}"#, path.to_string_lossy()),
    );
    assert!(save.contains("200 OK"), "scene save failed:\n{save}");
    let saved = std::fs::read_to_string(&path).expect("the saved scene must exist");

    for needle in [
        // Godot reads a tile's occluder only when the layer exists on the
        // TileSet — the occlusion twin of `navigation_layer_0/layers`.
        "occlusion_layer_0/light_mask = 1",
        // ⚠️ THREE components, not two: Godot's occlusion layer holds a LIST of
        // polygons per layer (`polygon_0/polygon`) where navigation carries
        // exactly one. Writing the navigation shape here parses and drops the
        // occluder.
        "0:0/0/occlusion_layer_0/polygon_0/polygon = SubResource(",
        // A SubResource reference to a real resource type, not an inline array.
        "[sub_resource type=\"OccluderPolygon2D\"",
    ] {
        assert!(
            saved.contains(needle),
            "the saved scene must carry `{needle}`; without it the authored \
             occlusion layer is dropped on save.\n{saved}"
        );
    }
    assert!(
        !saved.contains("__tile_occluder_"),
        "the flat occluder companions must not reach the .tscn.\n{saved}"
    );

    drop(handle);

    // -- reload, and check the runtime honours it ---------------------------
    let scene = PackedScene::from_tscn(&saved).expect("the saved scene must parse");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &scene).expect("the saved scene must instance");
    let layer = tree
        .find_child(root, "Layer", true)
        .expect("the reloaded scene must still hold the Layer");
    let node = tree.get_node(layer).expect("layer node");

    // 🔑 The step that makes it real: `tile_occluder_polygons` returns NOTHING
    // while the source list is empty, so a perfectly authored polygon casts no
    // shadow at all — saved, reloaded and inert.
    assert!(
        matches!(
            node.get_property(TILE_OCCLUDER_SOURCES),
            Variant::PackedInt32Array(ref v) if v.contains(&SOURCE)
        ),
        "enabling occlusion must register the source in {TILE_OCCLUDER_SOURCES}"
    );

    // Back in TOP-LEFT coordinates, not the centred form the file holds.
    let local = match node.get_property(TILE_OCCLUDER_POLYGON) {
        Variant::PackedVector2Array(v) => v,
        other => panic!("the reloaded layer must carry an occluder polygon, got {other:?}"),
    };
    assert_eq!(
        local,
        vec![
            Vector2::new(0.0, 0.0),
            Vector2::new(8.0, 0.0),
            Vector2::new(8.0, 16.0),
            Vector2::new(0.0, 16.0),
        ],
        "the polygon must come back in the top-left form it was authored in; a \
         half-tile shift means the centred/top-left rebasing was applied once \
         too often or not at all",
    );

    // And the runtime emits one occluder per painted, occlusion-enabled cell.
    // ⚠️ `tile_map_data` is a PackedByteArray, not an int array — copied from
    // the navigation test's own read rather than assumed.
    let cells = match node.get_property("tile_map_data") {
        Variant::PackedByteArray(bytes) => decode_tile_map_data(&bytes),
        other => panic!("the painted cells must survive the save, got {other:?}"),
    };
    let sources = match node.get_property(TILE_OCCLUDER_SOURCES) {
        Variant::PackedInt32Array(v) => v,
        _ => Vec::new(),
    };
    let polys = tile_occluder_polygons(&cells, Vector2i::new(TILE, TILE), &sources, &local);
    assert!(
        !polys.is_empty(),
        "every painted occlusion-enabled cell must produce an occluder polygon",
    );
}
