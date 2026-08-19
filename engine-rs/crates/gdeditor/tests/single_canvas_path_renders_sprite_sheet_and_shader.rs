//! Acceptance test for pat-s3rrv.22: there is now exactly ONE canvas path, and
//! it handles the cases the deleted bridge silently got wrong.
//!
//! Patina had two divergent scene→canvas renderers. `gdscene::scene_bridge`
//! documented itself as THE bridge and had no production caller; the shipped
//! path is `gdeditor::viewport_2d_render`. They had already drifted: the bridge's
//! `sprite_commands` ignored `region_enabled` / `region_rect` / `hframes` /
//! `vframes` / `frame` / `flip_h` / `flip_v`, all of which the shipped
//! `sprite_sheet_for_node` supports. Five tests "proved" 2D features end to end
//! through the bridge and therefore proved nothing about what ships.
//!
//! The bridge is gone (`rg scene_bridge` returns no hits). This pins the
//! behaviour that only the surviving path ever had:
//!
//! - a sprite-sheet frame + horizontal flip, which the bridge dropped entirely;
//! - a script `_draw()`, whose commands the bridge was the ONLY consumer of —
//!   they now reach the shipped render list;
//! - a `CanvasItem` ShaderMaterial, which must still reach pixels.

use gdcore::math::{Color, Vector2, Vector2i};
use gdeditor::texture_cache::TextureCache;
use gdeditor::viewport_2d_render::{
    build_runtime_render_list, rasterize_render_list, sprite_sheet_for_node, Render2DKind,
};
use gdrender2d::texture::Texture2D;
use gdscene::node::Node;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;
use gdscene::GDScriptNodeInstance;
use gdvariant::Variant;

const RED: Color = Color::new(1.0, 0.0, 0.0, 1.0);
const GREEN: Color = Color::new(0.0, 1.0, 0.0, 1.0);
const BLUE: Color = Color::new(0.0, 0.0, 1.0, 1.0);
const WHITE: Color = Color::new(1.0, 1.0, 1.0, 1.0);
const BG: Color = Color::new(0.0, 0.0, 0.0, 1.0);

/// A 4×1-cell sheet, 4px per cell: columns red, green, BLUE, white.
///
/// `frame = 2` must therefore sample the BLUE column — a distinct colour per
/// cell is what makes "the right sub-rect" observable in pixels rather than
/// inferred from fields.
fn sheet_texture() -> Texture2D {
    let (w, h) = (16u32, 4u32);
    let cols = [RED, GREEN, BLUE, WHITE];
    let mut pixels = Vec::with_capacity((w * h) as usize);
    for _y in 0..h {
        for x in 0..w {
            pixels.push(cols[(x / 4) as usize]);
        }
    }
    Texture2D {
        width: w,
        height: h,
        pixels,
    }
}

fn load(tscn: &str) -> SceneTree {
    let scene = PackedScene::from_tscn(tscn).expect("scene parses");
    // ⚠️ Without a populated ClassDB there is no class chain, so
    // `Node::is_class("CanvasItem")` is false for every node and the canvas
    // walk filters the whole scene out and returns `[]` — which reads like the
    // collapsed path lost the item rather than like the scene was never
    // collected at all (pat-s3rrv.22).
    gdobject::class_db::register_2d_classes();
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &scene).expect("scene instances");
    tree
}

/// **The acceptance test.** A `.tscn` sprite sheet with `hframes = 4,
/// frame = 2, flip_h = true` samples the right cell and mirrors it — the exact
/// case the deleted bridge dropped.
#[test]
fn single_canvas_path_renders_sprite_sheet_and_shader() {
    const SCENE: &str = r#"[gd_scene format=3]

[node name="World" type="Node2D"]

[node name="Sprite" type="Sprite2D" parent="."]
position = Vector2(16, 8)
texture = "res://sheet.png"
hframes = 4
frame = 2
flip_h = true
centered = false
"#;
    let tree = load(SCENE);
    let items = build_runtime_render_list(&tree);

    // (a) The sheet fields survive into the render item. The deleted bridge had
    //     no representation for any of them.
    let sprite = items
        .iter()
        .find(|i| matches!(i.kind, Render2DKind::Sprite { .. }))
        .unwrap_or_else(|| panic!("the Sprite2D must produce a Sprite item, got {items:?}"));
    match &sprite.kind {
        Render2DKind::Sprite {
            texture,
            flip_h,
            sheet,
            ..
        } => {
            assert_eq!(
                texture, "res://sheet.png",
                "the texture path carries through"
            );
            assert!(*flip_h, "flip_h must reach the render item");
            assert_eq!(sheet.hframes, 4, "hframes must reach the render item");
            assert_eq!(sheet.frame, 2, "frame must reach the render item");
        }
        other => panic!("expected a Sprite, got {other:?}"),
    }

    // (b) …and it samples the right CELL in pixels, not just in fields.
    let mut textures = TextureCache::new(".");
    textures.insert("res://sheet.png", sheet_texture());
    let fb = rasterize_render_list(&items, 32, 16, BG, Some(&mut textures));

    // The sprite is uncentred at (16,8) and each cell is 4×4, so the drawn cell
    // occupies x∈[16,20), y∈[8,12).
    let drawn = fb.get_pixel(18, 10);
    assert!(
        drawn.b > 0.5 && drawn.r < 0.5 && drawn.g < 0.5,
        "frame = 2 of a 4-frame sheet must sample the BLUE cell, got {drawn:?} — \
         a path that ignores `frame` draws the red first cell instead"
    );

    // 🔑 …and the flip must not MOVE the quad (pat-s3rrv.22).
    //
    // Godot flips a Sprite2D by negating `dst_rect.size`, which the canvas
    // normalises back to a positive rect plus a FLIP_H flag — the texture
    // sampling mirrors, the geometry does not. Reflecting about the node
    // origin instead sends this uncentred cell to x∈[12,16), where the
    // assertion above reads the clear colour and looks like "`frame` was
    // ignored". Sampling the far side is what tells the two apart, and it is
    // only observable because `centered = false`: a centred rect is symmetric
    // about the origin, so both behaviours agree and neither test fails.
    let mirrored_side = fb.get_pixel(14, 10);
    assert!(
        mirrored_side.r < 0.5 && mirrored_side.g < 0.5 && mirrored_side.b < 0.5,
        "x = 14 is outside the sprite's rect, so it must still be the clear \
         colour; got {mirrored_side:?} — the flip reflected the quad about the \
         node origin instead of mirroring the texture in place"
    );
}

/// A script `_draw()` reaches pixels through the one remaining path. Its
/// commands' only consumer used to be the deleted bridge.
#[test]
fn script_draw_reaches_pixels_on_the_shipped_path() {
    // ⚠️ Without a populated ClassDB there is no class chain, so
    // `Node::is_class("CanvasItem")` is false for every node and the canvas
    // walk filters the whole scene out and returns `[]` — which reads like the
    // collapsed path lost the item rather than like the scene was never
    // collected at all (pat-s3rrv.22).
    gdobject::class_db::register_2d_classes();
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let painter = tree
        .add_child(root, Node::new("Painter", "Node2D"))
        .unwrap();
    if let Some(n) = tree.get_node_mut(painter) {
        n.set_property("position", Variant::Vector2(Vector2::new(4.0, 4.0)));
    }
    tree.attach_script(
        painter,
        Box::new(
            GDScriptNodeInstance::from_source(
                "\
extends Node2D
func _ready():
    queue_redraw()
func _draw():
    draw_rect(Rect2(0, 0, 8, 8), Color(0, 1, 0, 1), true)
",
                painter,
            )
            .expect("script parses"),
        ),
    );
    let _ = tree.call_script_method(painter, "_ready", &[]);
    tree.flush_pending_redraws();

    let items = build_runtime_render_list(&tree);
    assert!(
        items
            .iter()
            .any(|i| matches!(i.kind, Render2DKind::ColorRect { .. })),
        "the _draw rect must reach the shipped render list, got {items:?}"
    );

    let fb = rasterize_render_list(&items, 24, 24, BG, None);
    let inside = fb.get_pixel(8, 8);
    assert!(
        inside.g > 0.5 && inside.r < 0.5,
        "a script `_draw()` must reach PIXELS on the one canvas path, got \
         {inside:?} — before this bead its commands had no consumer at all"
    );
}

/// A `CanvasItem` ShaderMaterial still reaches pixels on the surviving path.
#[test]
fn canvas_item_shader_material_still_reaches_pixels() {
    const SCENE: &str = r#"[gd_scene load_steps=2 format=3]

[sub_resource type="ShaderMaterial" id="Mat_1"]

[node name="World" type="Node2D"]

[node name="Rect" type="ColorRect" parent="."]
position = Vector2(2, 2)
size = Vector2(8, 8)
color = Color(1, 0, 0, 1)
material = SubResource("Mat_1")
"#;
    let tree = load(SCENE);
    let items = build_runtime_render_list(&tree);
    assert!(
        !items.is_empty(),
        "a materialled ColorRect must still produce a render item"
    );

    let fb = rasterize_render_list(&items, 16, 16, BG, None);
    let inside = fb.get_pixel(5, 5);
    assert!(
        inside.r > 0.5,
        "a CanvasItem carrying a ShaderMaterial must still reach pixels, got \
         {inside:?}"
    );
}

// ---------------------------------------------------------------------------
// `frame` vs `frame_coords` precedence (pat-s3rrv.22)
// ---------------------------------------------------------------------------

/// A `Sprite2D` exactly as the scene loader hands one over: EVERY ClassDB
/// default already in the bag, then the authored values on top.
fn sprite_with_class_defaults(authored: &[(&str, Variant)]) -> Node {
    gdobject::class_db::register_2d_classes();
    let mut node = Node::new("Sprite", "Sprite2D");
    // The same seeding `packed_scene::node_with_class_defaults` does — base
    // → derived, so a derived class's default wins.
    for prop in gdobject::class_db::get_property_list("Sprite2D") {
        node.set_property(&prop.name, prop.default_value);
    }
    for (key, value) in authored {
        node.set_property(key, value.clone());
    }
    node
}

/// **The precedence regression.** A seeded default `frame_coords` must NOT beat
/// an authored `frame`.
///
/// This is the bug the acceptance test caught the moment its ClassDB was
/// populated: `packed_scene` seeds every class default onto the node, so
/// `frame_coords` is ALWAYS present as `Vector2i(0, 0)` once
/// `register_2d_classes()` has run — which the editor and runner both do at
/// startup. Reading it as "the author chose cell (0,0)" pinned every sprite
/// sheet in every real project to frame 0, and looked like sheets being ignored
/// rather than like a precedence bug.
#[test]
fn a_seeded_default_frame_coords_does_not_override_an_authored_frame() {
    let node =
        sprite_with_class_defaults(&[("hframes", Variant::Int(4)), ("frame", Variant::Int(2))]);
    // Fixture check: the seeded default really is present, or this test is
    // asserting against a node that never had the problem.
    assert_eq!(
        node.get_property("frame_coords"),
        Variant::Vector2i(Vector2i::ZERO),
        "fixture: ClassDB seeding must have put a default frame_coords on the node",
    );

    let sheet = sprite_sheet_for_node(&node);
    assert_eq!(
        sheet.frame, 2,
        "an authored `frame` must survive the seeded default `frame_coords`",
    );
    assert_eq!(sheet.hframes, 4);
}

/// The other half: an ACTUALLY authored `frame_coords` still wins, so the fix
/// above is a precedence rule rather than a removal of the feature.
#[test]
fn an_authored_frame_coords_still_selects_the_cell() {
    let node = sprite_with_class_defaults(&[
        ("hframes", Variant::Int(4)),
        ("frame_coords", Variant::Vector2i(Vector2i { x: 3, y: 1 })),
    ]);
    let sheet = sprite_sheet_for_node(&node);
    assert_eq!(
        sheet.frame, 7,
        "frame_coords (col 3, row 1) of a 4-wide sheet is row*hframes+col = 7",
    );
}

/// Neither authored: cell 0, which is what both views agree on.
#[test]
fn no_authored_cell_is_frame_zero() {
    let node = sprite_with_class_defaults(&[("hframes", Variant::Int(4))]);
    assert_eq!(sprite_sheet_for_node(&node).frame, 0);
}
