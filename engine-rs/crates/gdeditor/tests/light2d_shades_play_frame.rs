//! pat-0q2kx.130 acceptance: scene 2D lights actually shade the Play frame, with
//! blend modes, coloured shadows and per-item cull masks.
//!
//! The lighting maths existed and almost none of it was reachable. `blend_mode`
//! was collected and then ignored — `apply_lights_to_framebuffer` accumulated a
//! single multiplier, so `composite_light` had only test callers and a `SUB`
//! light brightened like any other. Shadows were binary: an occluded pixel got
//! `continue`, so `shadow_color` had nowhere to apply. And the light pass was
//! handed a hardcoded item mask of `1` for the whole frame, so
//! `range_item_cull_mask` could not exclude anything.
//!
//! These drive `render_runtime_canvas` — the real Play path — from a `.tscn` and
//! assert on pixels.

use gdcore::math::Color;
use gdeditor::texture_cache::TextureCache;
use gdeditor::viewport_2d_render::render_runtime_canvas;
use gdrender2d::renderer::FrameBuffer;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::SceneTree;

const W: u32 = 80;
const H: u32 = 80;
const CLEAR: Color = Color {
    r: 0.0,
    g: 0.0,
    b: 0.0,
    a: 1.0,
};

/// A dark `CanvasModulate`, a white 60x60 `ColorRect` at the origin, a red
/// `PointLight2D` at its centre, and an occluder wall between the light and the
/// rect's lower-left corner.
///
/// The `CanvasModulate` is a uniform final multiply, so it scales every reading
/// equally and leaves the relative comparisons below intact — it is here because
/// a lit scene that is *not* already at full brightness is the case a light
/// actually has to show up against.
///
/// `blend_mode`: Godot's enum is ADD=0, SUB=1, MIX=2.
fn scene_source(blend_mode: i32, light_cull_mask: i32, rect_light_mask: i32) -> String {
    format!(
        r#"[gd_scene format=3]

[node name="World" type="Node2D"]

[node name="Dim" type="CanvasModulate" parent="."]
color = Color(0.6, 0.6, 0.6, 1)

[node name="Rect" type="ColorRect" parent="."]
position = Vector2(0, 0)
size = Vector2(60, 60)
color = Color(1, 1, 1, 1)
light_mask = {rect_light_mask}

[node name="Lamp" type="PointLight2D" parent="."]
position = Vector2(30, 20)
color = Color(1, 0, 0, 1)
energy = 1.0
range = 200.0
texture = "res://light.png"
texture_scale = 1.0
blend_mode = {blend_mode}
shadow_enabled = true
shadow_color = Color(0.2, 0, 0, 0.5)
range_item_cull_mask = {light_cull_mask}
shadow_item_cull_mask = 1

[node name="Wall" type="LightOccluder2D" parent="."]
polygon = PackedVector2Array(0, 40, 30, 40)
light_mask = 1
"#
    )
}

fn render(source: &str) -> FrameBuffer {
    gdobject::class_db::register_2d_classes();
    let scene = PackedScene::from_tscn(source).expect("scene parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &scene).expect("scene instances");
    let mut cache = TextureCache::new(".");
    render_runtime_canvas(&tree, W, H, CLEAR, Some(&mut cache))
}

fn pixel(fb: &FrameBuffer, x: u32, y: u32) -> Color {
    fb.pixels[(y * fb.width + x) as usize]
}

/// Luminance, for "is this brighter overall" comparisons that do not hinge on a
/// single channel.
fn luma(c: Color) -> f32 {
    0.2126 * c.r + 0.7152 * c.g + 0.0722 * c.b
}

// ---------------------------------------------------------------------------
// The bead's acceptance test
// ---------------------------------------------------------------------------

/// **The acceptance test.** A lit region, a coloured (not black) shadow behind
/// the occluder, `SUB` darkening instead of brightening, and a cull-mask miss
/// leaving the rect untouched.
#[test]
fn light2d_shades_play_frame() {
    // MIX (2) — Godot's default blend mode. Light and rect share layer 1.
    let fb = render(&scene_source(2, 1, 1));

    // (a) Near the light the rect is bright and red-tinted: the light is pure
    // red, so red must dominate green/blue.
    let lit = pixel(&fb, 30, 25);
    assert!(
        lit.r > 0.5,
        "pixels near the light must be brightened, got {lit:?}"
    );
    assert!(
        lit.r > lit.g + 0.2 && lit.r > lit.b + 0.2,
        "a red light must tint the lit region red, got {lit:?}"
    );

    // (b) Behind the occluder the pixel is darker than the lit side, but NOT
    // fully unlit: shadow_color has alpha 0.5, so half the light still passes.
    // A binary shadow would make this exactly the ambient floor.
    let shadowed = pixel(&fb, 15, 50);
    assert!(
        luma(shadowed) < luma(lit),
        "the occluded region must be darker than the lit side (shadow {shadowed:?} vs lit {lit:?})"
    );
    assert!(
        shadowed.r > 0.05,
        "shadow_color with alpha 0.5 must let light through — a fully black \
         shadow means shadow_color was ignored, got {shadowed:?}"
    );

    // (c) SUB (1) darkens where MIX brightened. Same scene otherwise, so the
    // only difference is the blend mode being routed through composite_light.
    let sub = render(&scene_source(1, 1, 1));
    let sub_lit = pixel(&sub, 30, 25);
    assert!(
        luma(sub_lit) < luma(lit),
        "blend_mode=SUB must darken where MIX brightened (sub {sub_lit:?} vs mix {lit:?})"
    );

    // (d) A light whose range_item_cull_mask misses the rect's light_mask leaves
    // the rect alone. Light on layer 2, rect on layer 1.
    let culled = render(&scene_source(2, 2, 1));
    let culled_px = pixel(&culled, 30, 25);
    assert!(
        culled_px.r < lit.r - 0.2,
        "a light whose range_item_cull_mask misses the item's light_mask must \
         not brighten it (culled {culled_px:?} vs lit {lit:?})"
    );
    // And the reverse: matching masks on a non-default layer DO interact, so the
    // check above is about the mask and not about layer 2 being special.
    let matched = render(&scene_source(2, 2, 2));
    let matched_px = pixel(&matched, 30, 25);
    assert!(
        matched_px.r > culled_px.r + 0.2,
        "the same light and item both on layer 2 must interact (matched \
         {matched_px:?} vs culled {culled_px:?})"
    );
}

/// `editor_only` lights are authoring aids and must contribute nothing to a
/// running frame.
#[test]
fn editor_only_light_does_not_shade_the_play_frame() {
    let lit = render(&scene_source(2, 1, 1));
    let editor_only = render(&scene_source(2, 1, 1).replace(
        "shadow_enabled = true",
        "shadow_enabled = true\neditor_only = true",
    ));
    assert!(
        luma(pixel(&editor_only, 30, 25)) < luma(pixel(&lit, 30, 25)),
        "an editor_only light must not light the Play frame",
    );
}

/// A shadow with an opaque `shadow_color` blocks the light completely — Godot's
/// default, and the behaviour the old binary `continue` produced. This pins that
/// the alpha is really doing the work in the acceptance test above.
#[test]
fn opaque_shadow_color_blocks_the_light_entirely() {
    let translucent = render(&scene_source(2, 1, 1));
    let opaque = render(&scene_source(2, 1, 1).replace(
        "shadow_color = Color(0.2, 0, 0, 0.5)",
        "shadow_color = Color(0, 0, 0, 1)",
    ));
    let t = pixel(&translucent, 15, 50);
    let o = pixel(&opaque, 15, 50);
    assert!(
        o.r < t.r,
        "an opaque black shadow_color must be darker than a half-alpha one \
         (opaque {o:?} vs translucent {t:?})"
    );
}
