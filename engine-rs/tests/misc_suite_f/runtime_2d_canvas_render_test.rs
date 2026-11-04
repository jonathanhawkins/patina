//! App-level framebuffer test for the 2D node → CanvasItem runtime pipeline
//! (pat-u3l8c.23).
//!
//! Before this pipeline, pressing **Play** on a 2D scene showed the editor's
//! class-glyph preview — a grid, rulers, gizmos, and blue placeholder icons for
//! sprites — instead of the pixels the game actually draws. The runtime canvas
//! rasterizer (`gdeditor::viewport_2d_render`) walks the scene tree and paints
//! the real `CanvasItem` visuals: textured `Sprite2D`s, filled `ColorRect`s, and
//! `Label` text, composited by `(canvas_layer, z_index)` order — exactly what the
//! Play tab / runtime framebuffer shows.
//!
//! This test drives that rasterizer end to end over a mixed scene and asserts on
//! the resulting framebuffer pixels: real colors appear where the nodes are,
//! `z_index` orders overlapping items within a layer, a `CanvasLayer` draws over
//! the default canvas regardless of `z_index`, markers/cameras paint nothing at
//! runtime, and no editor chrome (grid/rulers) is drawn.

use gdcore::math::{Color, Vector2};
use gdeditor::viewport_2d_render::{
    build_runtime_render_list, rasterize_render_list, RUNTIME_CLEAR_COLOR,
};
use gdrender2d::renderer::FrameBuffer;
use gdrender2d::texture::Texture2D;
use gdscene::node::Node;
use gdscene::{node2d, SceneTree};
use gdvariant::Variant;

const RED: Color = Color::new(1.0, 0.0, 0.0, 1.0);
const BLUE: Color = Color::new(0.0, 0.0, 1.0, 1.0);
const GREEN: Color = Color::new(0.0, 1.0, 0.0, 1.0);
const MAGENTA: Color = Color::new(1.0, 0.0, 1.0, 1.0);
const WHITE: Color = Color::new(1.0, 1.0, 1.0, 1.0);

/// Quantizes a color to 8-bit RGB for tolerant comparison.
fn q(c: Color) -> (u8, u8, u8) {
    (
        (c.r * 255.0).round() as u8,
        (c.g * 255.0).round() as u8,
        (c.b * 255.0).round() as u8,
    )
}

/// Asserts the pixel at `(x, y)` matches `expected` (8-bit RGB), with a helpful
/// message naming what should be there.
fn assert_pixel(fb: &FrameBuffer, x: u32, y: u32, expected: Color, what: &str) {
    let got = fb.get_pixel(x, y);
    assert_eq!(
        q(got),
        q(expected),
        "pixel ({x},{y}) should be {what}: expected {:?}, got {:?}",
        q(expected),
        q(got)
    );
}

/// Sets a node's raw `Vector2` property (position/size).
fn set_v2(tree: &mut SceneTree, id: gdscene::NodeId, key: &str, v: Vector2) {
    tree.get_node_mut(id)
        .unwrap()
        .set_property(key, Variant::Vector2(v));
}

/// Adds a positioned, sized, colored `ColorRect` under `parent`, at draw order
/// `z`.
fn add_color_rect(
    tree: &mut SceneTree,
    parent: gdscene::NodeId,
    name: &str,
    pos: Vector2,
    size: Vector2,
    color: Color,
    z: i64,
) -> gdscene::NodeId {
    let id = tree.add_child(parent, Node::new(name, "ColorRect")).unwrap();
    set_v2(tree, id, "position", pos);
    set_v2(tree, id, "size", size);
    let n = tree.get_node_mut(id).unwrap();
    n.set_property("color", Variant::Color(color));
    n.set_property("z_index", Variant::Int(z));
    id
}

/// Renders a mixed 2D scene through the runtime canvas rasterizer and asserts
/// every acceptance property of pat-u3l8c.23 on the resulting framebuffer.
#[test]
fn runtime_canvas_paints_real_sprite_rect_and_label_pixels() {
    gdobject::class_db::register_2d_classes();

    let (w, h) = (200u32, 200u32);
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let main = tree.add_child(root, Node::new("Main", "Node2D")).unwrap();

    // --- Sprite2D with a real (inserted) texture -------------------------
    // An 8×8 solid-magenta texture, centered at (40,40) → covers (36,36)-(44,44).
    let hero = tree.add_child(main, Node::new("Hero", "Sprite2D")).unwrap();
    node2d::set_position(&mut tree, hero, Vector2::new(40.0, 40.0));
    node2d::set_texture_path(&mut tree, hero, "res://hero.png");

    // --- Two overlapping ColorRects: z_index picks the winner ------------
    // Red at (70,70)-(110,110) z=0, Blue at (90,90)-(130,130) z=1. In the
    // overlap (90,90)-(110,110) Blue (higher z) wins.
    add_color_rect(
        &mut tree,
        main,
        "Red",
        Vector2::new(70.0, 70.0),
        Vector2::new(40.0, 40.0),
        RED,
        0,
    );
    add_color_rect(
        &mut tree,
        main,
        "Blue",
        Vector2::new(90.0, 90.0),
        Vector2::new(40.0, 40.0),
        BLUE,
        1,
    );

    // --- Label text ------------------------------------------------------
    // White "HI" at (10,10); the bitmap font paints lit glyph pixels there.
    let label = tree.add_child(main, Node::new("Score", "Label")).unwrap();
    set_v2(&mut tree, label, "position", Vector2::new(10.0, 10.0));
    tree.get_node_mut(label)
        .unwrap()
        .set_property("text", Variant::String("HI".to_string()));

    // --- CanvasLayer beats z_index --------------------------------------
    // A default-canvas red rect with the maximum z_index (999) at (80,140),
    // and a green rect on CanvasLayer(layer=2) with the minimum z_index
    // (-100) at (60,120). Despite the huge z gap, the CanvasLayer draws on
    // top in the overlap (80,140)-(110,170).
    add_color_rect(
        &mut tree,
        main,
        "WorldHigh",
        Vector2::new(80.0, 140.0),
        Vector2::new(50.0, 50.0),
        RED,
        999,
    );
    let ui_layer = tree.add_child(main, Node::new("UI", "CanvasLayer")).unwrap();
    tree.get_node_mut(ui_layer)
        .unwrap()
        .set_property("layer", Variant::Int(2));
    add_color_rect(
        &mut tree,
        ui_layer,
        "Hud",
        Vector2::new(60.0, 120.0),
        Vector2::new(50.0, 50.0),
        GREEN,
        -100,
    );

    // --- A marker node (empty Node2D) paints nothing at runtime ----------
    let empty = tree.add_child(main, Node::new("Empty", "Node2D")).unwrap();
    node2d::set_position(&mut tree, empty, Vector2::new(180.0, 10.0));

    // Insert the sprite's texture into the cache so the runtime path blits
    // real pixels (the editor's Play loop uses this same texture cache).
    let mut cache = gdeditor::texture_cache::TextureCache::default();
    cache.insert("res://hero.png", Texture2D::solid(8, 8, MAGENTA));

    // Render through the exact runtime path: build the ordered render list,
    // then rasterize it with the texture cache.
    let items = build_runtime_render_list(&tree);
    let fb = rasterize_render_list(&items, w, h, RUNTIME_CLEAR_COLOR, Some(&mut cache));

    assert_eq!((fb.width, fb.height), (w, h), "frame is the requested size");

    // Sprite2D: real texture pixels at its centered position.
    assert_pixel(&fb, 40, 40, MAGENTA, "the Sprite2D's blitted texture");

    // ColorRect fills.
    assert_pixel(&fb, 75, 75, RED, "the Red ColorRect (no overlap)");
    assert_pixel(&fb, 120, 120, BLUE, "the Blue ColorRect (no overlap)");
    // z_index ordering: Blue (z=1) over Red (z=0) in the overlap.
    assert_pixel(&fb, 100, 100, BLUE, "Blue over Red in the z-overlap");

    // CanvasLayer ordering beats z_index: Hud (layer 2, z=-100) over WorldHigh
    // (layer 0, z=999) in the overlap.
    assert_pixel(&fb, 90, 150, GREEN, "the CanvasLayer HUD over the world rect");
    assert_pixel(&fb, 120, 180, RED, "the WorldHigh rect where the HUD does not cover");
    assert_pixel(&fb, 65, 125, GREEN, "the HUD rect where the world rect does not cover");

    // Label: some lit glyph pixels appear in the label's region (10..40, 10..30).
    //
    // ⚠️ NEAR-white, not exactly white. Runtime text is anti-aliased TTF since
    // pat-s3rrv.37 (it was a 5x7 bitmap that painted solid 255s); a 16px stem
    // peaks around 242 of 255. Grey over the 77-grey clear colour is still
    // unmistakably glyph coverage, and with no text drawn the count is 0.
    let label_pixels = (10u32..40)
        .flat_map(|x| (10u32..30).map(move |y| (x, y)))
        .filter(|&(x, y)| {
            let (r, g, b) = q(fb.get_pixel(x, y));
            r >= 200 && r == g && g == b
        })
        .count();
    assert!(
        label_pixels > 0,
        "the Label paints white glyph pixels (found {label_pixels})"
    );

    // A marker/empty node paints nothing at runtime — that pixel stays clear.
    assert_pixel(
        &fb,
        180,
        10,
        RUNTIME_CLEAR_COLOR,
        "clear (empty Node2D markers are not drawn at runtime)",
    );

    // No editor chrome: the top-left corner has no grid line or ruler, just the
    // runtime clear color.
    assert_pixel(&fb, 2, 2, RUNTIME_CLEAR_COLOR, "clear (no editor grid/rulers)");
}

/// The render list orders items by `(canvas_layer, z_index)` — the exact draw
/// order the rasterizer composites in — so a `CanvasLayer` child always sorts
/// after (on top of) default-canvas nodes regardless of `z_index`.
#[test]
fn runtime_render_list_orders_by_layer_then_z() {
    gdobject::class_db::register_2d_classes();

    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let main = tree.add_child(root, Node::new("Main", "Node2D")).unwrap();

    let low_z = add_color_rect(
        &mut tree,
        main,
        "LowZ",
        Vector2::new(0.0, 0.0),
        Vector2::new(10.0, 10.0),
        RED,
        -5,
    );
    let high_z = add_color_rect(
        &mut tree,
        main,
        "HighZ",
        Vector2::new(0.0, 0.0),
        Vector2::new(10.0, 10.0),
        BLUE,
        900,
    );
    let layer = tree.add_child(main, Node::new("UI", "CanvasLayer")).unwrap();
    tree.get_node_mut(layer)
        .unwrap()
        .set_property("layer", Variant::Int(1));
    let hud = add_color_rect(
        &mut tree,
        layer,
        "Hud",
        Vector2::new(0.0, 0.0),
        Vector2::new(10.0, 10.0),
        GREEN,
        -999,
    );

    let items = build_runtime_render_list(&tree);
    let order: Vec<u64> = items.iter().map(|i| i.node_id).collect();

    let pos = |id: gdscene::NodeId| order.iter().position(|&n| n == id.raw()).unwrap();

    // Within the default canvas, low z sorts before high z.
    assert!(pos(low_z) < pos(high_z), "z_index orders within a layer");
    // The CanvasLayer child sorts last (on top) despite its very low z_index.
    assert!(
        pos(hud) > pos(high_z),
        "a CanvasLayer draws over the default canvas regardless of z_index"
    );

    // Every reported item carries its resolved canvas layer.
    let hud_item = items.iter().find(|i| i.node_id == hud.raw()).unwrap();
    assert_eq!(hud_item.canvas_layer, 1, "HUD resolves to CanvasLayer 1");
    let world_item = items.iter().find(|i| i.node_id == high_z.raw()).unwrap();
    assert_eq!(world_item.canvas_layer, 0, "default-canvas nodes are layer 0");
}
