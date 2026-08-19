//! pat-s3rrv.88 — a GDScript can drive its own TileMapLayer.
//!
//! Every cell operation had a Rust implementation in `gdscene::tilemap` and
//! **no binding at all**: `rg '"set_cell"' crates/gdscript-interop/src
//! crates/gdscene/src/scripting.rs` returned nothing, so the only callers were
//! the editor's `/api/tilemap/paint` route and integration tests. Procedural
//! generation — the node's dominant use in Godot — was impossible.
//!
//! ⚠️ Those existing functions take a `TileMapStore`, which is an EDITOR
//! structure that nothing in the running engine ever builds. At runtime the
//! source of truth is the node's `tile_map_data` property: the render path
//! (`viewport_2d_render::tilemap_quads`) and the tile-physics pass
//! (`physics_server`) both decode it directly. The bridge therefore mutates that
//! property, which is what makes a scripted edit reach pixels AND collision on
//! the next frame with no invalidation step.
//!
//! Godot ground truth: `upstream/godot/scene/2d/tile_map_layer.cpp`.
//!
//! Geometry: `tile_size` is 16, so cell (2,3) covers `x ∈ [32,48)`,
//! `y ∈ [48,64)` and `map_to_local` reports its CENTRE, `(40, 56)`.

use gdcore::math::{Color, Vector2, Vector2i};
use gdeditor::texture_cache::TextureCache;
use gdeditor::viewport_2d_render::render_runtime_canvas;
use gdrender2d::renderer::FrameBuffer;
use gdrender2d::texture::Texture2D;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::{GDScriptNodeInstance, MainLoop, NodeId, SceneTree};
use gdvariant::Variant;

const W: u32 = 64;
const H: u32 = 64;
const CLEAR: Color = Color::new(0.0, 0.0, 0.0, 1.0);
const RED: Color = Color::new(1.0, 0.0, 0.0, 1.0);

/// An empty layer: no `tile_map_data` at all, so every cell the test observes
/// was painted by the script rather than authored.
const SCENE: &str = r#"[gd_scene load_steps=4 format=3]

[ext_resource type="Texture2D" path="res://tiles.png" id="1_tex"]

[sub_resource type="TileSetAtlasSource" id="Atlas_1"]
texture = ExtResource("1_tex")
texture_region_size = Vector2i(16, 16)
0:0/0 = 0
0:0/0/physics_layer_0/polygon_0/points = PackedVector2Array(-8, -8, 8, -8, 8, 8, -8, 8)

[sub_resource type="TileSet" id="TileSet_1"]
tile_size = Vector2i(16, 16)
physics_layer_0/collision_layer = 1
sources/0 = SubResource("Atlas_1")

[node name="World" type="Node2D"]

[node name="Ground" type="TileMapLayer" parent="."]
tile_set = SubResource("TileSet_1")
"#;

/// Paints in `_ready`, reads back every frame, and erases on the second frame.
const PAINTER_GD: &str = "\
extends TileMapLayer
var used = []
var local = Vector2()
var round_trip = Vector2i(0, 0)
var source = -99
var ticks = 0
func _ready():
    set_cell(Vector2i(2, 3), 0, Vector2i(0, 0))
func _process(delta):
    self.ticks = self.ticks + 1
    if self.ticks == 2:
        erase_cell(Vector2i(2, 3))
    self.used = get_used_cells()
    self.local = map_to_local(Vector2i(2, 3))
    self.round_trip = local_to_map(self.local)
    self.source = get_cell_source_id(Vector2i(2, 3))
";

/// A 16x16 all-red atlas, so a drawn tile is unmistakable against the clear.
fn atlas_texture() -> Texture2D {
    Texture2D {
        width: 16,
        height: 16,
        pixels: vec![RED; 16 * 16],
    }
}

fn script_var(ml: &MainLoop, node: NodeId, name: &str) -> Option<Variant> {
    ml.tree()
        .get_script(node)?
        .list_properties()
        .into_iter()
        .find(|p| p.name == name)
        .map(|p| p.default_value)
}

fn render(tree: &SceneTree) -> FrameBuffer {
    let mut cache = TextureCache::new(".");
    cache.insert("res://tiles.png", atlas_texture());
    render_runtime_canvas(tree, W, H, CLEAR, Some(&mut cache))
}

fn pixel(fb: &FrameBuffer, x: u32, y: u32) -> Color {
    fb.pixels[(y * fb.width + x) as usize]
}

#[test]
fn tilemap_layer_script_api_mutates_scene() {
    gdobject::class_db::register_2d_classes();
    let scene = PackedScene::from_tscn(SCENE).expect("scene parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let scene_root = add_packed_scene_to_tree(&mut tree, root, &scene).expect("scene instances");
    let layer = *tree
        .find_children(scene_root, "*", Some("TileMapLayer"), true)
        .first()
        .expect("the TileMapLayer node is in the tree");
    tree.attach_script(
        layer,
        Box::new(GDScriptNodeInstance::from_source(PAINTER_GD, layer).unwrap()),
    );

    // The layer starts genuinely empty — otherwise "the script painted it" would
    // be indistinguishable from an authored cell.
    let before = render(&tree);
    assert_eq!(
        pixel(&before, 40, 56),
        CLEAR,
        "the layer must start empty, with nothing drawn at cell (2,3)"
    );

    let mut ml = MainLoop::new(tree);
    // ⚠️ `MainLoop::step` alone does NOT fire `_ready` for a script attached
    // after construction, so without this the paint never happens and every
    // assertion below reads an empty layer.
    ml.tree_mut().process_script_ready(layer);
    ml.step(1.0 / 60.0);

    assert_eq!(
        script_var(&ml, layer, "ticks"),
        Some(Variant::Int(1)),
        "_process must have run once"
    );

    // -- the script's own view of the map ------------------------------------
    assert_eq!(
        script_var(&ml, layer, "used"),
        Some(Variant::Array(vec![Variant::Vector2i(Vector2i::new(2, 3))])),
        "get_used_cells() must report exactly the cell _ready painted"
    );
    assert_eq!(
        script_var(&ml, layer, "source"),
        Some(Variant::Int(0)),
        "get_cell_source_id() must report the source the script set"
    );
    // `map_to_local` returns the cell's CENTRE: (2 + 0.5) * 16, (3 + 0.5) * 16.
    assert_eq!(
        script_var(&ml, layer, "local"),
        Some(Variant::Vector2(Vector2::new(40.0, 56.0))),
        "map_to_local() must return the cell centre"
    );
    assert_eq!(
        script_var(&ml, layer, "round_trip"),
        Some(Variant::Vector2i(Vector2i::new(2, 3))),
        "local_to_map(map_to_local(c)) must round-trip to c"
    );

    // -- …and it reached PIXELS ---------------------------------------------
    let painted = render(ml.tree());
    assert_eq!(
        pixel(&painted, 40, 56),
        RED,
        "the scripted set_cell must draw the tile at the cell's centre"
    );

    // -- …and COLLISION -----------------------------------------------------
    let bodies_painted = ml.physics_server().world().body_count();
    assert!(
        bodies_painted >= 1,
        "the painted tile must gain a static collision body, got {bodies_painted} bodies"
    );

    // -- a subsequent erase_cell removes the quad AND the body ---------------
    ml.step(1.0 / 60.0);
    assert_eq!(
        script_var(&ml, layer, "ticks"),
        Some(Variant::Int(2)),
        "the second frame must have run — otherwise the erase never happened"
    );
    assert_eq!(
        script_var(&ml, layer, "used"),
        Some(Variant::Array(Vec::new())),
        "after erase_cell the layer must report no used cells"
    );
    assert_eq!(
        script_var(&ml, layer, "source"),
        Some(Variant::Int(-1)),
        "an erased cell reports Godot's INVALID_SOURCE (-1), not 0"
    );

    // The quad goes immediately: the render path decodes `tile_map_data` afresh
    // every frame, so it has no state to invalidate.
    let erased = render(ml.tree());
    assert_eq!(
        pixel(&erased, 40, 56),
        CLEAR,
        "erase_cell must remove the drawn quad"
    );

    // Collision lags by one tick BY CONSTRUCTION: the tilemap refresh runs with
    // the rest of the physics registration, ahead of the script phase, so an
    // edit made inside `_process` is seen on the following tick. Godot defers
    // the same work to `update_internals` at the end of the frame. Stepping once
    // more is what a running game does anyway.
    ml.step(1.0 / 60.0);
    assert_eq!(
        ml.physics_server().world().body_count(),
        0,
        "erase_cell must free the tile's static collision body"
    );
}
