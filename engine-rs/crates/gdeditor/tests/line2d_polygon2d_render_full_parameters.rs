//! pat-s3rrv.85 — a `.tscn`'s Line2D/Polygon2D parameters reach the rasterizer.
//!
//! The whole production mapping used to be `Polygon2D -> {points, color}` and
//! `Line2D -> {points, width, default_color}`. `gradient`, `width_curve`,
//! `begin_cap_mode`, `joint_mode`, `texture` and `uv` were read nowhere, even
//! though `polygon_raster::draw_polyline_shaded` had accepted per-vertex widths
//! and colours since pat-0q2kx.191 (with zero non-test callers) and
//! `fill_polygon_textured` already sampled a texture through UVs.
//!
//! Godot ground truth: `scene/2d/line_2d.cpp` + `line_builder.cpp`,
//! `scene/2d/polygon_2d.cpp`, `scene/resources/curve.cpp`,
//! `scene/resources/gradient.h`.
//!
//! Each clause is paired with a CONTROL that must come out the other way, so a
//! renderer that simply drew a thick line everywhere could not pass.

use gdcore::math::Color;
use gdeditor::texture_cache::TextureCache;
use gdeditor::viewport_2d_render::{
    build_runtime_render_list, rasterize_render_list, Render2DKind,
};
use gdrender2d::renderer::FrameBuffer;
use gdrender2d::texture::Texture2D;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;

const BG: Color = Color::new(0.0, 0.0, 0.0, 1.0);
const W: u32 = 128;
const H: u32 = 80;

/// The gradient and width-curve sub-resources both clauses of the Line2D case
/// share: red at the start, blue at the end; width scaling 1.0 -> 0.25.
const LINE_RESOURCES: &str = r#"[gd_scene format=3]

[sub_resource type="Gradient" id="Gradient_1"]
offsets = PackedFloat32Array(0, 1)
colors = PackedColorArray(1, 0, 0, 1, 0, 0, 1, 1)

[sub_resource type="Curve" id="Curve_1"]
_data = [Vector2(0, 1), 0.0, 0.0, 0, 0, Vector2(1, 0.25), 0.0, 0.0, 0, 0]

[node name="World" type="Node2D"]
"#;

fn load(tscn: &str) -> SceneTree {
    // ClassDB starts empty in every nextest process, and the canvas walk admits
    // a node only if `is_class("CanvasItem")` resolves — without this the scene
    // collects to `[]`, which reads like the mapping dropped the node.
    gdobject::class_db::register_2d_classes();
    let scene = PackedScene::from_tscn(tscn).expect("scene parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &scene).expect("scene instances");
    tree
}

fn render(
    tscn: &str,
    textures: &mut TextureCache,
) -> (Vec<gdeditor::viewport_2d_render::Render2DItem>, FrameBuffer) {
    let tree = load(tscn);
    let items = build_runtime_render_list(&tree);
    let fb = rasterize_render_list(&items, W, H, BG, Some(textures));
    (items, fb)
}

/// Number of non-background pixels in the vertical column `x` — the stroke's
/// thickness where it crosses that column.
fn column_thickness(fb: &FrameBuffer, x: u32) -> usize {
    (0..H).filter(|y| fb.get_pixel(x, *y) != BG).count()
}

/// A 16x16 texture split into four differently coloured quadrants, so sampling
/// through real UVs yields several distinct texels rather than one flat colour.
fn quad_texture() -> Texture2D {
    let (w, h) = (16u32, 16u32);
    let mut pixels = Vec::with_capacity((w * h) as usize);
    for y in 0..h {
        for x in 0..w {
            pixels.push(match (x < 8, y < 8) {
                (true, true) => Color::new(1.0, 0.0, 0.0, 1.0),
                (false, true) => Color::new(0.0, 0.0, 1.0, 1.0),
                (true, false) => Color::new(1.0, 1.0, 1.0, 1.0),
                (false, false) => Color::new(1.0, 1.0, 0.0, 1.0),
            });
        }
    }
    Texture2D {
        width: w,
        height: h,
        pixels,
    }
}

#[test]
fn line2d_polygon2d_render_full_parameters() {
    gradient_and_width_curve_shade_the_stroke();
    a_plain_line2d_still_takes_the_uniform_path();
    round_begin_cap_lights_pixels_a_none_cap_does_not();
    polygon2d_samples_its_texture_through_uvs();
}

/// Clause 1: the gradient tints along the stroke and the width curve ramps it.
fn gradient_and_width_curve_shade_the_stroke() {
    let scene = format!(
        r#"{LINE_RESOURCES}
[node name="Stroke" type="Line2D" parent="."]
points = PackedVector2Array(20, 40, 100, 40)
width = 16.0
default_color = Color(1, 1, 1, 1)
gradient = SubResource("Gradient_1")
width_curve = SubResource("Curve_1")
"#
    );
    let mut textures = TextureCache::new(".");
    let (items, fb) = render(&scene, &mut textures);

    // (a) The properties reach the render item as per-vertex arrays.
    let shaded = items
        .iter()
        .find_map(|i| match &i.kind {
            Render2DKind::LineShaded { widths, colors, .. } => Some((widths.clone(), colors.clone())),
            _ => None,
        })
        .unwrap_or_else(|| {
            panic!("a Line2D with a gradient and a width curve must produce a LineShaded item, got {items:?}")
        });
    let (widths, colors) = shaded;
    assert_eq!(widths.len(), 2, "one width per point");
    assert!(
        (widths[0] - 16.0).abs() < 0.5 && (widths[1] - 4.0).abs() < 0.5,
        "the width curve must scale `width` 1.0 -> 0.25 along the stroke, got {widths:?}"
    );
    assert!(
        colors[0].r > 0.9 && colors[0].b < 0.1,
        "the first vertex takes the gradient's first stop (red), got {:?}",
        colors[0]
    );
    assert!(
        colors[1].b > 0.9 && colors[1].r < 0.1,
        "the last vertex takes the gradient's last stop (blue), got {:?}",
        colors[1]
    );

    // (b) …and it reaches PIXELS, which is what the bead is about.
    let near_start = fb.get_pixel(24, 40);
    let near_end = fb.get_pixel(96, 40);
    assert!(
        near_start.r > near_start.b,
        "the pixel near the first point must be red-dominant, got {near_start:?}"
    );
    assert!(
        near_end.b > near_end.r,
        "the pixel near the last point must be blue-dominant, got {near_end:?}"
    );

    // (c) The stroke is measurably thicker at the ramp's high end.
    let thick = column_thickness(&fb, 24);
    let thin = column_thickness(&fb, 96);
    assert!(
        thick > thin + 4,
        "the width curve must make the stroke measurably thicker at the start: \
         {thick}px at x=24 vs {thin}px at x=96"
    );
}

/// Control for clause 1: without a gradient or curve the Line2D must still take
/// the original uniform path. Otherwise "everything is LineShaded" would pass
/// the assertions above for the wrong reason.
fn a_plain_line2d_still_takes_the_uniform_path() {
    let scene = r#"[gd_scene format=3]

[node name="World" type="Node2D"]

[node name="Stroke" type="Line2D" parent="."]
points = PackedVector2Array(20, 40, 100, 40)
width = 16.0
default_color = Color(1, 1, 1, 1)
"#;
    let mut textures = TextureCache::new(".");
    let (items, fb) = render(scene, &mut textures);
    assert!(
        items
            .iter()
            .any(|i| matches!(i.kind, Render2DKind::Line { .. })),
        "a plain Line2D must still produce the uniform Line item, got {items:?}"
    );
    let a = column_thickness(&fb, 24);
    let b = column_thickness(&fb, 96);
    assert!(
        (a as i32 - b as i32).abs() <= 1,
        "a Line2D with no width curve must be the same thickness at both ends, \
         got {a}px and {b}px"
    );
}

/// Clause 3: `begin_cap_mode = ROUND` lights pixels before the first point.
fn round_begin_cap_lights_pixels_a_none_cap_does_not() {
    let body = r#"
[node name="Stroke" type="Line2D" parent="."]
points = PackedVector2Array(20, 40, 100, 40)
width = 16.0
default_color = Color(1, 1, 1, 1)
gradient = SubResource("Gradient_1")
width_curve = SubResource("Curve_1")
"#;
    // Pixels strictly LEFT of the first point (x = 20) — only a cap puts ink there.
    let beyond =
        |fb: &FrameBuffer| -> usize { (0..20u32).map(|x| column_thickness(fb, x)).sum::<usize>() };

    let mut textures = TextureCache::new(".");
    let (_, flat) = render(&format!("{LINE_RESOURCES}{body}"), &mut textures);
    let (_, round) = render(
        &format!("{LINE_RESOURCES}{body}begin_cap_mode = 2\n"),
        &mut textures,
    );

    assert_eq!(
        beyond(&flat),
        0,
        "a default (NONE) cap must not draw past the first point"
    );
    assert!(
        beyond(&round) > 0,
        "begin_cap_mode = ROUND must light pixels beyond the first point"
    );
}

/// Clause 2: a textured Polygon2D samples the texture through its UVs.
fn polygon2d_samples_its_texture_through_uvs() {
    // UVs are authored in TEXTURE PIXELS and span the whole 16x16 image, so the
    // fill crosses all four quadrants (`polygon_2d.cpp:232` divides by the
    // texture size). A uv array that stayed unnormalized would clamp to a single
    // edge texel and show one flat colour.
    let scene = r#"[gd_scene format=3]

[node name="World" type="Node2D"]

[node name="Poly" type="Polygon2D" parent="."]
polygon = PackedVector2Array(20, 10, 100, 10, 100, 70, 20, 70)
uv = PackedVector2Array(0, 0, 16, 0, 16, 16, 0, 16)
texture = "res://tex.png"
color = Color(0, 1, 0, 1)
"#;
    let mut textures = TextureCache::new(".");
    textures.insert("res://tex.png", quad_texture());
    let (items, fb) = render(scene, &mut textures);

    assert!(
        items
            .iter()
            .any(|i| matches!(i.kind, Render2DKind::PolygonTextured { .. })),
        "a Polygon2D with a texture and uv must produce a PolygonTextured item, got {items:?}"
    );

    let fill = Color::new(0.0, 1.0, 0.0, 1.0);
    let mut seen: Vec<Color> = Vec::new();
    for y in 12..68 {
        for x in 22..98 {
            let p = fb.get_pixel(x, y);
            if p != BG && p != fill && !seen.iter().any(|c| *c == p) {
                seen.push(p);
            }
        }
    }
    assert!(
        seen.len() >= 2,
        "the polygon must show at least two distinct non-fill texel values, got {seen:?}"
    );
    assert!(
        !seen.iter().any(|c| *c == fill),
        "a textured polygon must not fall back to its flat fill colour"
    );
}
