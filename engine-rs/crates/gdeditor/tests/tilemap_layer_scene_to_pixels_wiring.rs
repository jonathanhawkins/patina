//! pat-0q2kx.108 acceptance: a Godot-authored `TileMapLayer` actually shows its
//! tiles.
//!
//! Every piece existed and nothing joined them up. `decode_tile_map_data`
//! (gdscene/src/tilemap.rs) had no caller, there was no `TileSet` sub-resource
//! parse at all, and `viewport_2d_render::render_kind_for` had no TileMap arm —
//! so a `.tscn` with a painted layer loaded as an empty shell and drew as an
//! invisible marker. `PackedByteArray("<base64>")`, the form Godot 4 actually
//! writes `tile_map_data` in, did not even parse.
//!
//! This drives the real chain — `.tscn` text → `PackedScene` → `SceneTree` →
//! runtime canvas — and asserts on the pixels, so "the tiles are on screen" is
//! checked by what was drawn rather than by the intermediate structs.

use gdcore::math::{Color, Vector2i};
use gdeditor::texture_cache::TextureCache;
use gdeditor::viewport_2d_render::{
    build_runtime_render_list, render_runtime_canvas, Render2DKind,
};
use gdrender2d::renderer::FrameBuffer;
use gdrender2d::texture::Texture2D;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_saver::TscnSaver;
use gdscene::{NodeId, SceneTree};
use gdvariant::Variant;

const W: u32 = 64;
const H: u32 = 64;
const CLEAR: Color = Color {
    r: 0.0,
    g: 0.0,
    b: 0.0,
    a: 1.0,
};
const RED: Color = Color {
    r: 1.0,
    g: 0.0,
    b: 0.0,
    a: 1.0,
};
const GREEN: Color = Color {
    r: 0.0,
    g: 1.0,
    b: 0.0,
    a: 1.0,
};

/// `tile_map_data` for cells (0,0) → atlas (0,0) and (3,2) → atlas (1,0), both
/// on source 0, base64-encoded exactly as Godot's text writer emits it
/// (`variant_parser.cpp`, the non-compat `PackedByteArray("…")` branch).
///
/// Decoded: a `u16` format header then 12-byte
/// `[i16 x][i16 y][u16 source][u16 atlas_x][u16 atlas_y][u16 alternative]`
/// records — `00 00 | 0,0,0,0,0,0 | 3,2,0,1,0,0`.
const TILE_MAP_DATA_B64: &str = "AAAAAAAAAAAAAAAAAAADAAIAAAABAAAAAAA=";

/// The same cell records as raw bytes, for the round-trip comparison.
const TILE_MAP_DATA_BYTES: [u8; 26] = [
    0, 0, // format version
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, // cell (0,0) → source 0, atlas (0,0)
    3, 0, 2, 0, 0, 0, 1, 0, 0, 0, 0, 0, // cell (3,2) → source 0, atlas (1,0)
];

/// A Godot-shaped scene: an atlas texture, a `TileSetAtlasSource` over it, a
/// `TileSet` referencing that source under id 0, and a layer painted with two
/// cells.
fn scene_source() -> String {
    format!(
        r#"[gd_scene load_steps=4 format=3]

[ext_resource type="Texture2D" path="res://tiles.png" id="1_tex"]

[sub_resource type="TileSetAtlasSource" id="TileSetAtlasSource_a"]
texture = ExtResource("1_tex")
texture_region_size = Vector2i(16, 16)
0:0/0 = 0
1:0/0 = 0

[sub_resource type="TileSet" id="TileSet_b"]
tile_size = Vector2i(16, 16)
sources/0 = SubResource("TileSetAtlasSource_a")

[node name="World" type="Node2D"]

[node name="Ground" type="TileMapLayer" parent="."]
tile_map_data = PackedByteArray("{TILE_MAP_DATA_B64}")
tile_set = SubResource("TileSet_b")
"#
    )
}

/// A 32x16 atlas of two 16x16 tiles: atlas (0,0) is red, atlas (1,0) is green.
/// Distinct colours are what make "the right region was sampled" checkable.
fn atlas_texture() -> Texture2D {
    let (w, h) = (32u32, 16u32);
    let mut pixels = Vec::with_capacity((w * h) as usize);
    for _y in 0..h {
        for x in 0..w {
            pixels.push(if x < 16 { RED } else { GREEN });
        }
    }
    Texture2D {
        width: w,
        height: h,
        pixels,
    }
}

/// Loads [`scene_source`] into a tree and returns it with the layer's node id.
fn load_scene() -> (SceneTree, NodeId) {
    gdobject::class_db::register_2d_classes();
    let scene = PackedScene::from_tscn(&scene_source()).expect("scene parses");
    let mut tree = SceneTree::new();
    let root_id = tree.root_id();
    let scene_root = add_packed_scene_to_tree(&mut tree, root_id, &scene).expect("scene instances");
    let layer = *tree
        .find_children(scene_root, "*", Some("TileMapLayer"), true)
        .first()
        .expect("the TileMapLayer node is in the tree");
    (tree, layer)
}

fn render(tree: &SceneTree) -> FrameBuffer {
    let mut cache = TextureCache::new(".");
    cache.insert("res://tiles.png", atlas_texture());
    render_runtime_canvas(tree, W, H, CLEAR, Some(&mut cache))
}

fn pixel(fb: &FrameBuffer, x: u32, y: u32) -> Color {
    fb.pixels[(y * fb.width + x) as usize]
}

fn approx(a: Color, b: Color) -> bool {
    (a.r - b.r).abs() < 0.02 && (a.g - b.g).abs() < 0.02 && (a.b - b.b).abs() < 0.02
}

// ---------------------------------------------------------------------------
// The bead's acceptance test
// ---------------------------------------------------------------------------

/// **The wiring test.** A `.tscn` TileSet atlas plus a two-cell
/// `tile_map_data` puts those two atlas regions on the runtime canvas at
/// (0,0) and (48,32), reports two used cells, and re-saves byte-identical
/// `tile_map_data`.
#[test]
fn tilemap_layer_scene_to_pixels_wiring() {
    let (tree, layer) = load_scene();

    // -- the layer really loaded its cells ----------------------------------
    let items = build_runtime_render_list(&tree);
    let item = items
        .iter()
        .find(|i| i.node_id == layer.raw())
        .expect("the TileMapLayer is in the render list");
    let cells = match &item.kind {
        Render2DKind::TileMap { cells } => cells,
        other => panic!(
            "a painted TileMapLayer must render as tiles, not {other:?} — \
             falling through to Marker is the bug this bead fixes"
        ),
    };
    assert_eq!(
        cells.len(),
        2,
        "get_used_cells-equivalent state must report the two painted cells"
    );

    // -- and drew them at the right pixels ----------------------------------
    let fb = render(&tree);

    // Cell (0,0) with 16x16 tiles occupies [0,16) x [0,16).
    assert!(
        approx(pixel(&fb, 0, 0), RED),
        "cell (0,0) must draw atlas tile (0,0) — red — at pixel (0,0); got {:?}",
        pixel(&fb, 0, 0)
    );
    assert!(
        approx(pixel(&fb, 15, 15), RED),
        "the whole 16x16 cell must be covered, not just its first pixel; got {:?}",
        pixel(&fb, 15, 15)
    );

    // Cell (3,2) lands at (3*16, 2*16) = (48, 32).
    assert!(
        approx(pixel(&fb, 48, 32), GREEN),
        "cell (3,2) must draw atlas tile (1,0) — green — at pixel (48,32); got {:?}",
        pixel(&fb, 48, 32)
    );
    assert!(
        approx(pixel(&fb, 63, 47), GREEN),
        "cell (3,2) must cover its full 16x16 extent; got {:?}",
        pixel(&fb, 63, 47)
    );

    // The decisive half: each cell samples its OWN atlas region. Blitting the
    // whole 32x16 atlas per cell would put green inside cell (0,0) and red
    // inside cell (3,2), and both centre-pixel checks above would still pass.
    assert!(
        !approx(pixel(&fb, 15, 8), GREEN),
        "cell (0,0) must not contain the neighbouring atlas tile's colour — \
         the atlas region, not the whole texture, is what gets drawn"
    );
    assert!(
        !approx(pixel(&fb, 48, 40), RED),
        "cell (3,2) must not contain atlas tile (0,0)'s colour"
    );

    // And nothing is painted where no cell was: (32,32) is between the two.
    assert!(
        approx(pixel(&fb, 32, 32), CLEAR),
        "an unpainted cell must stay clear; got {:?}",
        pixel(&fb, 32, 32)
    );

    // -- saving re-emits the same tile_map_data -----------------------------
    let saved = TscnSaver::save_tree(&tree, layer);
    let reloaded = PackedScene::from_tscn(&saved).expect("the saved layer parses back");
    let nodes = reloaded.instance().expect("the saved layer instances");
    assert_eq!(
        nodes[0].get_property("tile_map_data"),
        Variant::PackedByteArray(TILE_MAP_DATA_BYTES.to_vec()),
        "a save/load round-trip must preserve tile_map_data exactly"
    );
    assert!(
        !saved.contains("__tile_set__"),
        "the flattened tile-set cache is an internal companion and must never \
         be written out as a scene property"
    );
}

/// Control: the same scene with `enabled = false` draws nothing. Without this,
/// the pixel assertions above could pass for a renderer that ignores the layer
/// state entirely.
#[test]
fn disabled_layer_draws_no_tiles() {
    let (mut tree, layer) = load_scene();
    tree.get_node_mut(layer)
        .unwrap()
        .set_property("enabled", Variant::Bool(false));

    let fb = render(&tree);
    assert!(
        approx(pixel(&fb, 0, 0), CLEAR) && approx(pixel(&fb, 48, 32), CLEAR),
        "TileMapLayer.enabled = false must draw no cells at all"
    );
}

/// A layer whose `tile_set` names a source id its cells do not use draws
/// nothing, rather than panicking or drawing tile (0,0) for everything.
#[test]
fn cells_referencing_an_unknown_source_are_skipped() {
    let (mut tree, layer) = load_scene();
    // Repaint both cells onto source 7, which the tile set never declares.
    let mut bytes = TILE_MAP_DATA_BYTES;
    bytes[6] = 7;
    bytes[18] = 7;
    tree.get_node_mut(layer)
        .unwrap()
        .set_property("tile_map_data", Variant::PackedByteArray(bytes.to_vec()));

    let fb = render(&tree);
    assert!(
        approx(pixel(&fb, 0, 0), CLEAR),
        "a cell whose source_id names no atlas source must be skipped; got {:?}",
        pixel(&fb, 0, 0)
    );
}

/// The atlas grid honours `margins` and `separation`, so a padded atlas — the
/// common case for an imported tilesheet — samples the right region.
#[test]
fn atlas_margins_and_separation_offset_the_region() {
    use gdscene::tilemap::TileSetAtlasSource;

    let mut source = TileSetAtlasSource::new("res://tiles.png", Vector2i::new(16, 16));
    source.margins = Vector2i::new(2, 3);
    source.separation = Vector2i::new(4, 5);

    // Godot: margins + coords * (region + separation).
    let region = source.tile_region(Vector2i::new(2, 1));
    assert_eq!(region.position.x, 2.0 + 2.0 * (16.0 + 4.0));
    assert_eq!(region.position.y, 3.0 + 1.0 * (16.0 + 5.0));
    assert_eq!(region.size.x, 16.0);
    assert_eq!(region.size.y, 16.0);

    // The default atlas is a tight grid.
    let tight = TileSetAtlasSource::new("res://tiles.png", Vector2i::new(16, 16));
    let region = tight.tile_region(Vector2i::new(1, 0));
    assert_eq!(region.position.x, 16.0);
    assert_eq!(region.position.y, 0.0);
}
