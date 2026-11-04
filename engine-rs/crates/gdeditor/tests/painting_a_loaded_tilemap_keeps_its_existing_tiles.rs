//! Painting one tile on a TileMapLayer loaded from a scene must not touch the
//! rest of the layer. The editor's dense `TileGrid` started EMPTY for such a
//! layer, and the write-back rebuilt `tile_map_data` from that grid alone — so
//! the first brush stroke erased the whole level, and every cell's atlas
//! coords. Cells the grid cannot represent at all (negative coordinates, and
//! TileSet source 0, which the grid uses for "empty") were lost too. Found
//! painting the platformer demo's level in the browser editor (341 cells, x
//! down to -12, 14 cells on source 0).

use gdcore::math::Vector2i;
use gdeditor::editor_server::{EditorServerHandle, EditorState};
use gdscene::node::Node;
use gdscene::tilemap::{decode_tile_map_data, encode_tile_map_data, TileData};
use gdscene::SceneTree;
use gdvariant::Variant;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::time::Duration;

fn post(port: u16, path: &str, body: &str) -> String {
    let mut s = TcpStream::connect(format!("127.0.0.1:{port}")).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(5))).ok();
    let req = format!(
        "POST {path} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    s.write_all(req.as_bytes()).unwrap();
    let mut out = String::new();
    let _ = s.read_to_string(&mut out);
    out
}

#[test]
fn one_paint_keeps_every_existing_cell_including_negative_and_source_zero() {
    let mut authored = HashMap::new();
    authored.insert(Vector2i::new(-1, -1), TileData::with_atlas(3, Vector2i::new(2, 1), 0));
    authored.insert(Vector2i::new(0, 0), TileData::with_atlas(0, Vector2i::new(1, 1), 0));
    authored.insert(Vector2i::new(2, 2), TileData::with_atlas(5, Vector2i::new(3, 0), 1));

    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let mut layer = Node::new("Layer", "TileMapLayer");
    layer.set_property("tile_map_data", Variant::PackedByteArray(encode_tile_map_data(&authored)));
    let id = layer.id();
    tree.add_child(root, layer).unwrap();
    let port = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let handle = EditorServerHandle::start(port, EditorState::new(tree));
    std::thread::sleep(Duration::from_millis(100));

    let r = post(port, "/api/tilemap/paint", &format!(r#"{{"node_id":{},"x":4,"y":4,"tile_id":2}}"#, id.raw()));
    assert!(r.contains("200 OK"), "paint failed: {r}");

    let state = handle.state();
    let s = state.lock().unwrap();
    let Variant::PackedByteArray(bytes) = s.scene_tree.get_node(id).unwrap().get_property("tile_map_data") else {
        panic!("tile_map_data missing")
    };
    let cells = decode_tile_map_data(&bytes);
    for (c, want) in &authored {
        let got = cells.get(c).unwrap_or_else(|| panic!("authored cell {c:?} was erased by one paint: {cells:?}"));
        assert_eq!(
            (got.source_id, got.atlas_coords, got.alternative_id),
            (want.source_id, want.atlas_coords, want.alternative_id),
            "cell {c:?} must be unchanged"
        );
    }
    assert_eq!(cells.get(&Vector2i::new(4, 4)).map(|d| d.source_id), Some(2), "the painted cell");
    assert_eq!(cells.len(), 4);
    drop(s);
    handle.stop();
}

fn get(port: u16, path: &str) -> serde_json::Value {
    let mut s = TcpStream::connect(format!("127.0.0.1:{port}")).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(5))).ok();
    s.write_all(format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n").as_bytes()).unwrap();
    let mut out = String::new();
    let _ = s.read_to_string(&mut out);
    serde_json::from_str(out.split("\r\n\r\n").nth(1).unwrap_or("")).expect("json")
}

/// The viewport maps a click to a cell with the palette's `cell_size`; it must
/// be the selected layer's TileSet `tile_size` (the platformer demo uses 64px),
/// not the palette's 16px default — or every click paints four cells away.
#[test]
fn the_palette_cell_size_is_the_selected_layers_tile_size() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let mut layer = Node::new("Layer", "TileMapLayer");
    layer.set_property(gdscene::tilemap::TILE_SET_TILE_SIZE, Variant::Vector2i(Vector2i::new(64, 64)));
    let id = layer.id();
    tree.add_child(root, layer).unwrap();
    let port = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let handle = EditorServerHandle::start(port, EditorState::new(tree));
    std::thread::sleep(Duration::from_millis(100));

    // Edge: nothing selected → the palette's own default.
    assert_eq!(get(port, "/api/tilemap/tileset")["cell_size"], serde_json::json!([16.0, 16.0]));
    post(port, "/api/node/select", &format!(r#"{{"node_id":{}}}"#, id.raw()));
    assert_eq!(get(port, "/api/tilemap/tileset")["cell_size"], serde_json::json!([64.0, 64.0]));
    handle.stop();
}

/// The same cell size through the REAL load path. The platformer demo keeps
/// its TileSet in `res://level/tileset.tres`, which the scene parser reads
/// through gdcore's `res://` root — and the editor only set that root at Play,
/// so a scene opened for EDITING kept `tile_set = "ExtResource(\"1\")"`
/// unresolved: no atlas sources, the 16px default tile size, and every
/// browser click painting four cells from the cursor. The test above writes
/// the tile size onto the node itself, so it never reached this.
#[test]
fn a_scene_opened_for_editing_resolves_its_external_tileset() {
    let project = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../games/platformer");
    let project = project.canonicalize().expect("games/platformer fixture");
    let port = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let handle = EditorServerHandle::start(port, EditorState::new(SceneTree::new()));
    std::thread::sleep(Duration::from_millis(100));

    let open = post(port, "/api/project/open", &format!(r#"{{"path":"{}"}}"#, project.display()));
    assert!(open.contains("200 OK"), "project open failed: {open}");
    let load = post(
        port,
        "/api/scene/load",
        &format!(r#"{{"path":"{}"}}"#, project.join("level/level.tscn").display()),
    );
    assert!(load.contains("200 OK"), "scene load failed: {load}");

    let id = {
        let state = handle.state();
        let s = state.lock().unwrap();
        let ids = s.scene_tree.all_nodes_in_tree_order();
        let id = ids
            .into_iter()
            .find(|&n| s.scene_tree.get_node(n).is_some_and(|n| n.class_name() == "TileMapLayer"))
            .expect("level.tscn has a TileMapLayer");
        assert_eq!(
            s.scene_tree.get_node(id).unwrap().get_property(gdscene::tilemap::TILE_SET_TILE_SIZE),
            Variant::Vector2i(Vector2i::new(64, 64)),
            "the external tileset.tres must be flattened onto the layer at edit load"
        );
        id
    };
    post(port, "/api/node/select", &format!(r#"{{"node_id":{}}}"#, id.raw()));
    assert_eq!(get(port, "/api/tilemap/tileset")["cell_size"], serde_json::json!([64.0, 64.0]));
    let src = get(port, &format!("/api/tileset/source?node_id={}", id.raw()));
    assert!(
        src["sources"].as_array().is_some_and(|a| !a.is_empty()),
        "the TileSet panel must list the .tres atlas sources: {src}"
    );
    handle.stop();
}

/// The edit view must SHOW the level's tiles. The platformer demo instances its
/// ParallaxBackground (a CanvasLayer on layer -100) as a sub-scene, which the
/// editor draws as a preview — appended AFTER the one (layer, z) sort, so it
/// was drawn over everything and turning the whole TileMapLayer off changed
/// zero pixels of the edit frame. Differential: the same frame with the layer
/// enabled and disabled; the no-preview frame is the control that proves the
/// layer draws at all.
#[test]
fn the_edit_view_draws_the_level_tiles_in_front_of_an_instanced_parallax_background() {
    let project = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../games/platformer")
        .canonicalize()
        .expect("games/platformer fixture");
    let root = project.display().to_string();
    // As the editor server does at start: the canvas pass asks ClassDB which
    // nodes are CanvasItems, and an empty one draws nothing at all.
    gdobject::class_db::register_all_classes();
    gdeditor::editor_server::apply_project_path_roots(&root);
    let text = std::fs::read_to_string(project.join("level/level.tscn")).unwrap();
    let scene = gdscene::packed_scene::PackedScene::from_tscn(&text).unwrap();
    let mut tree = SceneTree::new();
    let tree_root = tree.root_id();
    gdscene::packed_scene::add_packed_scene_to_tree(&mut tree, tree_root, &scene).unwrap();
    let layer = tree
        .all_nodes_in_tree_order()
        .into_iter()
        .find(|&n| tree.get_node(n).is_some_and(|n| n.class_name() == "TileMapLayer"))
        .unwrap();
    let mut tc = gdeditor::texture_cache::TextureCache::new(&root);
    let mut frame = |tree: &mut SceneTree, previews: bool| {
        if previews {
            gdeditor::scene_renderer::render_scene_with_instance_previews(
                tree, &root, &mut tc, None, 962, 681, 1.0, (0.0, 0.0),
            )
        } else {
            gdeditor::scene_renderer::render_scene_with_textures(tree, None, 962, 681, 1.0, (0.0, 0.0), &mut tc)
        }
    };
    let (with_previews_on, without_on) = (frame(&mut tree, true), frame(&mut tree, false));
    tree.get_node_mut(layer).unwrap().set_property("enabled", Variant::Bool(false));
    let (with_previews_off, without_off) = (frame(&mut tree, true), frame(&mut tree, false));
    let diff = |a: &gdrender2d::FrameBuffer, b: &gdrender2d::FrameBuffer| {
        a.pixels.iter().zip(&b.pixels).filter(|(p, q)| p != q).count()
    };
    let control = diff(&without_on, &without_off);
    assert!(control > 10_000, "control: the layer must draw without previews ({control} px)");
    let shown = diff(&with_previews_on, &with_previews_off);
    // Measured 97,065 px without previews; the parallax sky is behind, so
    // nearly all of them must survive. Before the fix this was exactly 0.
    assert!(
        shown * 10 >= control * 9,
        "the instanced ParallaxBackground (layer -100) must draw BEHIND the tiles: \
         toggling the layer changed {shown} px with previews vs {control} px without"
    );
}
