//! pat-s3rrv.203 — a `CanvasTexture`'s normal map shades a Play frame.
//!
//! ⚠️ **The obvious form of this test passes on an unimplemented feature.** The
//! bead's wording is "brighter on its left-facing pixels than its right-facing
//! ones, with the light offset to the left" — but a light to the left is also
//! *nearer* the left-hand pixels, so plain distance falloff already satisfies
//! that comparison with no normal mapping whatsoever.
//!
//! 🔑 So the load-bearing assertions here compare **the same pixel across two
//! scenes** that are identical except for the normal map's contents. Distance,
//! light, sprite and albedo are equal by construction, so any difference is
//! attributable to the normal alone. The within-frame left/right comparison is
//! still asserted, but it is the weaker of the two and could never stand alone.

use gdcore::math::Color;
use gdeditor::texture_cache::TextureCache;
use gdeditor::viewport_2d_render::render_runtime_canvas;
use gdobject::class_db::register_all_classes;
use gdrender2d::texture::Texture2D;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;

const W: u32 = 128;
const H: u32 = 128;
const CLEAR: Color = Color {
    r: 0.0,
    g: 0.0,
    b: 0.0,
    a: 1.0,
};

/// A 64x64 Sprite2D at the centre of the frame with a CanvasTexture, and a
/// PointLight2D off to its left at the bead's height of 32.
const SCENE: &str = r#"[gd_scene load_steps=4 format=3]

[ext_resource type="Texture2D" path="res://wall_d.png" id="1_d"]
[ext_resource type="Texture2D" path="res://wall_n.png" id="1_n"]

[sub_resource type="CanvasTexture" id="CanvasTexture_1"]
diffuse_texture = ExtResource("1_d")
normal_texture = ExtResource("1_n")

[node name="Root" type="Node2D"]

[node name="Wall" type="Sprite2D" parent="."]
position = Vector2(64, 64)
texture = SubResource("CanvasTexture_1")

[node name="Lamp" type="PointLight2D" parent="."]
position = Vector2(0, 64)
height = 32.0
energy = 4.0
"#;

/// A plain white diffuse map, so every brightness difference in the frame comes
/// from lighting rather than from albedo.
fn diffuse() -> Texture2D {
    Texture2D::solid(64, 64, Color::new(1.0, 1.0, 1.0, 1.0))
}

/// A hemispherical normal map: the left half faces `-X`, the right half `+X`.
///
/// `g = 0.5` throughout, which decodes to `y = 0` — see
/// `gdrender2d`'s `the_green_channel_is_inverted` for why that means this
/// fixture cannot detect a Y-flip, and why a separate test pins it.
fn hemispherical_normal() -> Texture2D {
    let mut tex = Texture2D::solid(64, 64, Color::new(0.5, 0.5, 1.0, 1.0));
    for y in 0..64 {
        for x in 0..64 {
            let r = if x < 32 { 0.0 } else { 1.0 };
            tex.pixels[(y * 64 + x) as usize] = Color::new(r, 0.5, 1.0, 1.0);
        }
    }
    tex
}

/// The control fixture: every texel faces the viewer, so the surface is flat.
fn flat_normal() -> Texture2D {
    Texture2D::solid(64, 64, Color::new(0.5, 0.5, 1.0, 1.0))
}

fn render(normal: Texture2D) -> Vec<Color> {
    // ⚠️ REQUIRED, and its absence fails as an empty frame rather than an error.
    // `collect_canvas_items` filters on `node.is_class("CanvasItem")`, which
    // walks the ClassDB inheritance chain — with ClassDB unpopulated a Sprite2D
    // is not a CanvasItem, the render list comes back empty, and every
    // brightness assertion compares black to black.
    register_all_classes();

    let scene = PackedScene::from_tscn(SCENE).expect("scene parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &scene).expect("scene instances");

    // Injected rather than loaded from disk: `TextureCache::insert` keeps the
    // fixture in the test instead of requiring a PNG on a project root.
    let mut cache = TextureCache::new(".");
    cache.insert("res://wall_d.png", diffuse());
    cache.insert("res://wall_n.png", normal);

    render_runtime_canvas(&tree, W, H, CLEAR, Some(&mut cache)).pixels
}

fn luma(c: Color) -> f32 {
    (c.r + c.g + c.b) / 3.0
}

/// Screen-space columns the sprite actually covers on the centre scanline,
/// discovered rather than assumed, so the test does not depend on Sprite2D's
/// centring convention.
fn covered_columns(px: &[Color]) -> Vec<u32> {
    let y = H / 2;
    (0..W)
        .filter(|x| luma(px[(y * W + x) as usize]) > 1e-6)
        .collect()
}

/// The sprite must actually draw. 🔑 Its `texture` property is
/// `SubResource:CanvasTexture_1`, which names a resource and not a file — if the
/// diffuse channel is not resolved, nothing is drawn and every brightness
/// assertion below would compare black to black and pass.
#[test]
fn the_canvas_textures_diffuse_channel_is_what_gets_drawn() {
    let px = render(flat_normal());
    let covered = covered_columns(&px);
    assert!(
        covered.len() > 16,
        "the sprite must render its CanvasTexture's diffuse channel; {} lit \
         columns means `texture = SubResource(...)` never resolved to a path",
        covered.len()
    );
}

/// **The assertion that cannot be explained by falloff.** The same pixel, the
/// same light, the same distance — only the normal map differs.
#[test]
fn a_right_facing_normal_darkens_a_pixel_a_flat_one_leaves_lit() {
    let bumpy = render(hemispherical_normal());
    let flat = render(flat_normal());
    let covered = covered_columns(&flat);
    assert!(!covered.is_empty(), "the sprite must render");

    // A column well inside the right-facing half.
    let x = covered[covered.len() * 3 / 4];
    let idx = ((H / 2) * W + x) as usize;

    assert!(
        luma(flat[idx]) > 0.0,
        "the control must be lit at x={x}, or the comparison below is \
         black-vs-black; got {}",
        luma(flat[idx])
    );
    // ⚠️ The threshold accounts for an AMBIENT FLOOR. `CanvasAmbient::default()`
    // is 0.15 and `composite_light` in Mix mode is `b + c*(1-b)`, so a pixel
    // receiving NO light still reads 0.15 rather than 0. The light's energy is
    // raised in the fixture so its contribution dominates that floor; without
    // that, a correct implementation still fails this comparison.
    assert!(
        luma(bumpy[idx]) < luma(flat[idx]) * 0.6,
        "a pixel whose normal faces AWAY from the light must be markedly \
         darker than the same pixel with a flat normal — same position, same \
         distance, so falloff is identical and only the normal differs. \
         Values near 0.15 are the ambient floor, not darkness. \
         got bumpy={} flat={}",
        luma(bumpy[idx]),
        luma(flat[idx])
    );
}

/// The bead's literal wording, asserted second because on its own it is
/// satisfied by distance falloff alone.
#[test]
fn left_facing_pixels_are_brighter_than_right_facing_ones() {
    let px = render(hemispherical_normal());
    let covered = covered_columns(&px);
    assert!(covered.len() > 4, "the sprite must render");

    let left = covered[covered.len() / 4];
    let right = covered[covered.len() * 3 / 4];
    let left_luma = luma(px[((H / 2) * W + left) as usize]);
    let right_luma = luma(px[((H / 2) * W + right) as usize]);

    assert!(
        left_luma > right_luma,
        "with the light to the left, left-facing pixels must be brighter; got \
         left={left_luma} at x={left}, right={right_luma} at x={right}"
    );
}

/// The bead's own control: "a flat texture under the same light is uniform".
///
/// ⚠️ Uniform *in the normal's contribution*, not in absolute value — the light
/// is a point light 64px away, so its distance falloff still varies across the
/// sprite. What must hold is that the flat surface's variation is far smaller
/// than the normal-mapped one's, which is what isolates the relief.
#[test]
fn a_flat_normal_map_under_the_same_light_has_far_less_variation() {
    let bumpy = render(hemispherical_normal());
    let flat = render(flat_normal());
    let covered = covered_columns(&flat);
    assert!(covered.len() > 4, "the sprite must render");

    let l = covered[covered.len() / 4];
    let r = covered[covered.len() * 3 / 4];
    let spread = |px: &[Color]| {
        (luma(px[((H / 2) * W + l) as usize]) - luma(px[((H / 2) * W + r) as usize])).abs()
    };

    assert!(
        spread(&bumpy) > spread(&flat) * 2.0,
        "the normal-mapped sprite must show markedly more left-to-right \
         variation than the flat one; comparable spreads mean the frame is \
         only showing distance falloff and the normal map never reached the \
         light pass. got bumpy={} flat={}",
        spread(&bumpy),
        spread(&flat)
    );
}
