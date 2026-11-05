//! A `TileMapLayer` whose `tile_set` is an EXTERNAL `.tres` (`tile_set =
//! ExtResource("1")`) — how real projects store a TileSet, and how the official
//! 2D Platformer demo does. Only the inline `SubResource` form used to be
//! flattened onto the node, so such a layer drew nothing and collided with
//! nothing: the demo's player fell through the level forever.

use gdscene::packed_scene::PackedScene;
use gdscene::physics_server::PhysicsServer;
use gdscene::tilemap::{encode_tile_map_data, TileData};
use gdscene::SceneTree;
use gdcore::math::Vector2i;
use std::collections::HashMap;

/// Serializes the tests in this binary that touch the res:// path roots (pat-hukbz).
///
/// That state is process-global. Nextest gives each test its own process, but
/// plain `cargo test` runs the tests as threads in ONE process, where they
/// overwrote each other's state mid-test. Bind the guard for the whole test.
/// Poison-tolerant, so one failing test does not fail the rest.
#[must_use = "bind it for the whole test: `let _guard = serial();`"]
fn serial() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

const TILESET_TRES: &str = r#"[gd_resource type="TileSet" format=3]

[ext_resource type="Texture2D" path="res://tiles.png" id="1"]

[sub_resource type="TileSetAtlasSource" id="TileSetAtlasSource_a"]
texture = ExtResource("1")
texture_region_size = Vector2i(16, 16)
0:0/0 = 0
0:0/0/physics_layer_0/polygon_0/points = PackedVector2Array(-8, -8, 8, -8, 8, 8, -8, 8)

[resource]
tile_size = Vector2i(32, 32)
physics_layer_0/collision_layer = 16
physics_layer_0/collision_mask = 3
sources/0 = SubResource("TileSetAtlasSource_a")
"#;

#[test]
fn a_tile_map_layer_with_an_external_tile_set_draws_and_collides() {
    let _guard = serial();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::write(root.join("project.godot"), "config_version=5\n").unwrap();
    image::RgbaImage::from_pixel(16, 16, image::Rgba([0, 200, 0, 255]))
        .save(root.join("tiles.png"))
        .unwrap();
    std::fs::write(root.join("tileset.tres"), TILESET_TRES).unwrap();

    let mut cells = HashMap::new();
    for x in 0..3 {
        let mut cell = TileData::new(0);
        cell.atlas_coords = Vector2i::new(0, 0);
        cells.insert(Vector2i::new(x, 0), cell);
    }
    let bytes = encode_tile_map_data(&cells);
    let list: Vec<String> = bytes.iter().map(|b| b.to_string()).collect();
    let level = format!(
        "[gd_scene format=3]\n\n\
         [ext_resource type=\"TileSet\" path=\"res://tileset.tres\" id=\"1\"]\n\n\
         [node name=\"Level\" type=\"Node2D\"]\n\n\
         [node name=\"TileMapLayer\" type=\"TileMapLayer\" parent=\".\"]\n\
         tile_map_data = PackedByteArray({})\n\
         tile_set = ExtResource(\"1\")\n",
        list.join(", ")
    );
    std::fs::write(root.join("level.tscn"), &level).unwrap();

    gdcore::set_path_roots(Some(root.to_path_buf()), None);
    gdobject::class_db::register_all_classes();
    let mut tree = SceneTree::new();
    let scene = PackedScene::from_tscn(&level).unwrap();
    let level_id = tree.change_scene_to_packed(&scene).unwrap();
    let layer = tree.get_node_or_null(level_id, "TileMapLayer").unwrap();

    let quads = gdscene::canvas_render::tilemap_quads(&tree, layer);
    assert_eq!(quads.len(), 3, "one quad per painted cell");
    assert_eq!(quads[0].texture, "res://tiles.png", "the atlas texture from the .tres");

    let mut physics = PhysicsServer::new();
    physics.register_bodies(&tree);
    assert!(
        physics.world().bodies().count() >= 1,
        "the tiles' physics polygons register static collision"
    );
    // On the TileSet physics layer's bits, not a hard-coded layer 1: the
    // official 2D Platformer demo's tiles are on 16, so its player (mask 30)
    // fell straight through a level of layer-1 bodies.
    for b in physics.world().bodies() {
        assert_eq!((b.collision_layer, b.collision_mask), (16, 3), "tile body layer/mask");
    }
    // Each tile body sits where its cell is DRAWN. With a non-default 32 px
    // tile set, physics used to fall back to 16 px cells and put the level's
    // collision at half (for 64 px: a quarter) of its drawn position.
    let mut body_x: Vec<f32> = physics.world().bodies().map(|b| b.position.x).collect();
    body_x.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let mut quad_x: Vec<f32> = quads
        .iter()
        .map(|q| q.dest.position.x + q.dest.size.x / 2.0)
        .collect();
    quad_x.sort_by(|a, b| a.partial_cmp(b).unwrap());
    assert_eq!(body_x, quad_x, "tile bodies at the drawn cells' centres");
}

/// Alternative tiles: a cell painted with alternative 1 (`flip_h`) draws
/// mirrored and collides with ITS OWN polygon, not whichever alternative's
/// polygon a hash map happened to visit last.
const ALT_TILESET_TRES: &str = r#"[gd_resource type="TileSet" format=3]

[ext_resource type="Texture2D" path="res://tiles.png" id="1"]

[sub_resource type="TileSetAtlasSource" id="TileSetAtlasSource_a"]
texture = ExtResource("1")
texture_region_size = Vector2i(16, 16)
0:0/next_alternative_id = 3
0:0/0 = 0
0:0/0/physics_layer_0/polygon_0/points = PackedVector2Array(-8, -2, 8, -2, 8, 8, -8, 8)
0:0/1 = 1
0:0/1/flip_h = true
0:0/1/physics_layer_0/polygon_0/points = PackedVector2Array(8, -2, -8, -2, -8, 8, 8, 8)
0:0/2 = 2
0:0/2/flip_v = true
0:0/2/physics_layer_0/polygon_0/points = PackedVector2Array(-8, 2, 8, 2, 8, -8, -8, -8)

[resource]
tile_size = Vector2i(16, 16)
physics_layer_0/collision_layer = 1
sources/0 = SubResource("TileSetAtlasSource_a")
"#;

#[test]
fn alternative_tiles_keep_their_own_orientation_and_polygon() {
    let _guard = serial();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::write(root.join("project.godot"), "config_version=5\n").unwrap();
    image::RgbaImage::from_pixel(16, 16, image::Rgba([255, 255, 255, 255]))
        .save(root.join("tiles.png"))
        .unwrap();
    std::fs::write(root.join("tileset.tres"), ALT_TILESET_TRES).unwrap();

    let mut cells = HashMap::new();
    for (x, alt) in [(0, 0), (1, 1), (2, 2)] {
        let mut cell = TileData::new(0);
        cell.atlas_coords = Vector2i::new(0, 0);
        cell.alternative_id = alt;
        cells.insert(Vector2i::new(x, 0), cell);
    }
    let list: Vec<String> = encode_tile_map_data(&cells).iter().map(|b| b.to_string()).collect();
    let level = format!(
        "[gd_scene format=3]\n\n\
         [ext_resource type=\"TileSet\" path=\"res://tileset.tres\" id=\"1\"]\n\n\
         [node name=\"Level\" type=\"Node2D\"]\n\n\
         [node name=\"TileMapLayer\" type=\"TileMapLayer\" parent=\".\"]\n\
         tile_map_data = PackedByteArray({})\n\
         tile_set = ExtResource(\"1\")\n",
        list.join(", ")
    );
    gdcore::set_path_roots(Some(root.to_path_buf()), None);
    gdobject::class_db::register_all_classes();
    let mut tree = SceneTree::new();
    let level_id = tree
        .change_scene_to_packed(&PackedScene::from_tscn(&level).unwrap())
        .unwrap();
    let layer = tree.get_node_or_null(level_id, "TileMapLayer").unwrap();

    // Orientation: alt 0 unflipped, alt 1 mirrored in x, alt 2 mirrored in y.
    let quads = gdscene::canvas_render::tilemap_quads(&tree, layer);
    assert_eq!(quads.len(), 3);
    let by_flags: Vec<i64> = quads.iter().map(|q| q.flags).collect();
    assert_eq!(
        by_flags,
        vec![0, gdscene::tilemap::TILE_FLIP_H, gdscene::tilemap::TILE_FLIP_V]
    );
    let q1 = &quads[1];
    let left_edge_mid = gdcore::math::Vector2::new(q1.dest.position.x, q1.dest.position.y + 8.0);
    let mapped = q1.orientation().xform(left_edge_mid);
    assert!(
        (mapped.x - (q1.dest.position.x + 16.0)).abs() < 1e-4 && (mapped.y - left_edge_mid.y).abs() < 1e-4,
        "flip_h sends the texture's left edge to the cell's right edge: {mapped:?}"
    );

    // Each alternative keeps its own polygon.
    let node = tree.get_node(layer).unwrap();
    let poly = |key: String| match node.get_property(&key) {
        gdvariant::Variant::PackedVector2Array(p) => p,
        other => panic!("{key}: {other:?}"),
    };
    let base = poly(gdscene::tilemap::tile_physics_points_key(0, Vector2i::new(0, 0)));
    let alt2 = poly(gdscene::tilemap::tile_physics_points_alt_key(0, Vector2i::new(0, 0), 2));
    assert_eq!(base[0], gdcore::math::Vector2::new(-8.0, -2.0), "the default tile's own polygon");
    assert_eq!(alt2[0], gdcore::math::Vector2::new(-8.0, 2.0), "alternative 2 keeps its flipped polygon");
}

#[test]
fn transpose_swaps_the_cell_axes() {
    use gdcore::math::{Rect2, Vector2};
    let q = gdscene::canvas_render::TileQuad {
        texture: String::new(),
        dest: Rect2::new(Vector2::ZERO, Vector2::new(16.0, 16.0)),
        src: Rect2::new(Vector2::ZERO, Vector2::new(16.0, 16.0)),
        flags: gdscene::tilemap::TILE_TRANSPOSE,
    };
    // Texture top-right corner (16, 0) lands at the cell's bottom-left (0, 16).
    let p = q.orientation().xform(Vector2::new(16.0, 0.0));
    assert!((p.x - 0.0).abs() < 1e-4 && (p.y - 16.0).abs() < 1e-4, "{p:?}");
}
