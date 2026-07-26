//! pat-0q2kx.131 — `CanvasItemMaterial` blend modes and `texture_filter` /
//! `texture_repeat` reach the composited Play pixels.
//!
//! Godot composites each canvas item per its `CanvasItemMaterial.blend_mode`
//! (MIX/ADD/SUB/MUL/PREMULT_ALPHA) and samples every texture per
//! `texture_filter`/`texture_repeat`, both resolved up the parent chain to the
//! viewport defaults (`canvas_item.cpp`, `canvas_item_material.cpp`).
//!
//! Patina had the math and none of the wiring: `gdrender2d::canvas_material`'s
//! `blend` and `sample_texture` had **zero production callers** — only their own
//! `#[cfg(test)]` cases — `CanvasItemMaterial` was not registered in the
//! ClassDB, and `packed_scene`'s node-level `material` pass filtered to
//! `ShaderMaterial`, so a `.tscn`-authored CanvasItemMaterial was dropped at
//! load. Every item composited as plain Mix and every texture sampled
//! nearest-clamped.
//!
//! These drive the real `.tscn` -> `PackedScene` -> `build_canvas_viewport` ->
//! `SoftwareRenderer` path and assert on pixels.

use gdcore::math::{Color, Vector2};
use gdrender2d::renderer::{FrameBuffer, SoftwareRenderer};
use gdrender2d::texture::Texture2D;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_bridge::build_canvas_viewport;
use gdscene::scene_tree::SceneTree;

const W: u32 = 64;
const H: u32 = 64;
const BG: Color = Color::new(0.0, 0.0, 0.0, 1.0);

/// Two overlapping half-grey `ColorRect`s. The top one's blend mode is the only
/// thing the two scenes below differ by.
fn overlap_scene(top_material: &str) -> String {
    format!(
        r#"[gd_scene load_steps=2 format=3]
{top_material}
[node name="Main" type="Node2D"]

[node name="Bottom" type="ColorRect" parent="."]
offset_left = 0.0
offset_top = 0.0
size = Vector2(40, 40)
color = Color(0.5, 0.5, 0.5, 1)

[node name="Top" type="ColorRect" parent="."]
offset_left = 0.0
offset_top = 0.0
size = Vector2(40, 40)
color = Color(0.5, 0.5, 0.5, 1)
{material_line}
"#,
        top_material = top_material,
        material_line = if top_material.is_empty() {
            ""
        } else {
            "material = SubResource(\"CanvasItemMaterial_1\")"
        }
    )
}

const ADD_MATERIAL: &str = r#"
[sub_resource type="CanvasItemMaterial" id="CanvasItemMaterial_1"]
blend_mode = 1
"#;

fn render_tscn(src: &str) -> FrameBuffer {
    let scene = PackedScene::from_tscn(src).expect("scene parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &scene).expect("scene instances");
    let no_textures = |_p: &str| -> Option<Vector2> { None };
    let viewport = build_canvas_viewport(&tree, W, H, BG, &no_textures);
    let mut renderer = SoftwareRenderer::new();
    gdrender2d::test_adapter::capture_frame(&mut renderer, &viewport)
}

/// **The acceptance test.** Two overlapping half-grey rects composite brighter
/// with `blend_mode = ADD` than with the MIX baseline.
#[test]
fn canvas_item_material_blend_in_play() {
    let mix = render_tscn(&overlap_scene(""));
    let add = render_tscn(&overlap_scene(ADD_MATERIAL));

    let mix_px = mix.get_pixel(20, 20);
    let add_px = add.get_pixel(20, 20);

    // Baseline: opaque grey over opaque grey is still grey.
    assert!(
        (mix_px.r - 0.5).abs() < 0.05,
        "MIX baseline should stay at the source grey, got {mix_px:?}",
    );

    // ADD accumulates: 0.5 over 0.5 clamps toward 1.0. This is the assertion the
    // old code could not satisfy — `blend` had no caller, so the material was
    // inert and this pixel was identical to the MIX one.
    assert!(
        add_px.r > mix_px.r + 0.3,
        "ADD must composite brighter than MIX: add {add_px:?} vs mix {mix_px:?}",
    );
    assert!(
        add_px.r > 0.9,
        "0.5 + 0.5 should saturate toward 1.0, got {add_px:?}",
    );
}

// ---------------------------------------------------------------------------
// texture_filter inheritance
// ---------------------------------------------------------------------------

/// A 2×2 black/white checker — magnified, NEAREST keeps hard texel edges while
/// LINEAR produces intermediate greys.
fn checker() -> Texture2D {
    Texture2D {
        width: 2,
        height: 2,
        pixels: vec![
            Color::new(0.0, 0.0, 0.0, 1.0),
            Color::new(1.0, 1.0, 1.0, 1.0),
            Color::new(1.0, 1.0, 1.0, 1.0),
            Color::new(0.0, 0.0, 0.0, 1.0),
        ],
    }
}

/// `filter` values are Godot's `CanvasItem.TextureFilter`: 0 = PARENT_NODE,
/// 1 = NEAREST, 2 = LINEAR.
fn checker_scene(parent_filter: i32, child_filter: i32) -> String {
    format!(
        r#"[gd_scene load_steps=2 format=3]

[node name="Main" type="Node2D"]
texture_filter = {parent_filter}

[node name="Sprite" type="Sprite2D" parent="."]
position = Vector2(0, 0)
texture_filter = {child_filter}
"#
    )
}

/// Renders a scene whose `Sprite2D` shows the 2×2 checker stretched to 32×32.
fn render_checker(src: &str) -> FrameBuffer {
    let scene = PackedScene::from_tscn(src).expect("scene parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let scene_root = add_packed_scene_to_tree(&mut tree, root, &scene).expect("scene instances");
    let sprite = tree
        .get_node_or_null(scene_root, "Sprite")
        .expect("Sprite instanced");
    // Point the sprite at a texture the renderer knows, and give the bridge its
    // size so the destination rect is the magnified 32×32.
    tree.get_node_mut(sprite).unwrap().set_property(
        "texture",
        gdvariant::Variant::String("res://checker.png".into()),
    );
    let sizes = |p: &str| -> Option<Vector2> {
        (p == "res://checker.png").then_some(Vector2::new(32.0, 32.0))
    };
    let viewport = build_canvas_viewport(&tree, W, H, BG, &sizes);
    let mut renderer = SoftwareRenderer::new();
    renderer.register_texture("res://checker.png", checker());
    gdrender2d::test_adapter::capture_frame(&mut renderer, &viewport)
}

/// Whether any pixel on the row is a mid-grey — the signature of bilinear
/// interpolation across a hard black/white texel boundary.
fn has_intermediate(fb: &FrameBuffer, y: u32) -> bool {
    (0..W).any(|x| {
        let c = fb.get_pixel(x, y);
        c.r > 0.15 && c.r < 0.85
    })
}

/// A child left on `FILTER_PARENT_NODE` samples with the parent's LINEAR, and
/// the same scene with the parent on NEAREST has no interpolated pixels.
#[test]
fn texture_filter_resolves_through_parent_node() {
    // Parent LINEAR (2), child PARENT_NODE (0) -> the child must sample LINEAR.
    let inherited_linear = render_checker(&checker_scene(2, 0));
    // Parent NEAREST (1), child PARENT_NODE (0) -> hard edges.
    let inherited_nearest = render_checker(&checker_scene(1, 0));

    let y = 8;
    assert!(
        has_intermediate(&inherited_linear, y),
        "LINEAR inherited from the parent must produce interpolated greys",
    );
    assert!(
        !has_intermediate(&inherited_nearest, y),
        "NEAREST inherited from the parent must keep hard texel edges — every \
         pixel should be fully black or fully white",
    );
}

/// An explicit child filter overrides the parent's, rather than the walk always
/// stopping at the ancestor.
#[test]
fn explicit_child_filter_beats_the_parent() {
    // Parent LINEAR, child explicitly NEAREST -> hard edges win.
    let child_nearest = render_checker(&checker_scene(2, 1));
    assert!(
        !has_intermediate(&child_nearest, 8),
        "an explicit NEAREST on the child must override the parent's LINEAR",
    );
}
