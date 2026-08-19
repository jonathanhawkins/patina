//! pat-s3rrv.36 acceptance: the real GDShader front end drives scene pixels.
//!
//! The bead's wiring requirement is explicit that a unit test on `shader_ast`
//! alone does not close it: the language must reach pixels through
//! `.tscn` → `PackedScene::from_tscn` → `add_packed_scene_to_tree` → the shipped
//! canvas path.
//!
//! # Why this file exists
//!
//! `gdserver2d`'s `parse_fragment_body` was a five-shape line matcher that
//! silently skipped any statement it could not classify. A `fragment()` body
//! with a local variable, an `if`, or a `max()` call yielded ZERO instructions,
//! which made the shaded draw an exact passthrough — the sprite rendered
//! unshaded with no diagnostic. This drives shaders the matcher cannot express
//! and asserts they change pixels.
//!
//! # Structure
//!
//! The `.tscn` shape and the render call sequence are copied from
//! `canvas_item_shader_scene_pixels.rs`, which already proves this exact path
//! carries an authored ShaderMaterial to pixels. Reusing a reference test that
//! pins the same layer means a failure here is about the shader LANGUAGE, not
//! about scene loading or the render list.

use gdcore::math::Color;
use gdeditor::texture_cache::TextureCache;
use gdeditor::viewport_2d_render::{build_runtime_render_list, rasterize_render_list};
use gdrender2d::texture::Texture2D;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;

const SIZE: u32 = 8;

/// A one-Sprite2D scene carrying an inline shader. Mirrors the fixture in
/// `canvas_item_shader_scene_pixels.rs`.
fn sprite_scene_tscn(code: &str) -> String {
    format!(
        r#"[gd_scene load_steps=3 format=3]

[sub_resource type="Shader" id="Shader_1"]
code = "{code}"

[sub_resource type="ShaderMaterial" id="ShaderMaterial_1"]
shader = SubResource("Shader_1")

[node name="Shaded" type="Sprite2D"]
texture = "tex"
position = Vector2(4, 4)
material = SubResource("ShaderMaterial_1")
"#
    )
}

/// Instances `tscn` and renders it through the shipped canvas path.
///
/// ⚠️ **Sample only the upper-left quadrant.** The sprite is centred (Godot's
/// default) over an 8x8 texture, and the covered region is its own extent — the
/// authored `position` is not what places it in the frame here. A sample at
/// (4,4) comes back as the BLACK clear colour, which reads like "the shader
/// computed black" and is really "no texel was drawn there". The sibling
/// `canvas_item_shader_scene_pixels.rs` samples (0..3, 2) for exactly this
/// reason; assertions here match it so the shading claim never rests on the
/// transform half of the pipeline.
fn render(tscn: &str) -> Vec<Color> {
    // ⚠️ Without a populated ClassDB there is no class chain, so
    // `Node::is_class("CanvasItem")` is false for every node and
    // `collect_canvas_items` filters the whole scene out and returns `[]`
    // (pat-s3rrv.28).
    gdobject::class_db::register_2d_classes();
    let scene = PackedScene::from_tscn(tscn).expect("scene must parse");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &scene).expect("scene must instance");

    let mut textures = TextureCache::new(".");
    textures.insert("tex", Texture2D::solid(SIZE, SIZE, Color::WHITE));

    let items = build_runtime_render_list(&tree);
    let frame = rasterize_render_list(&items, SIZE, SIZE, Color::BLACK, Some(&mut textures));
    frame.pixels
}

fn pixel(px: &[Color], x: u32, y: u32) -> Color {
    px[(y * SIZE + x) as usize]
}

fn close(a: Color, b: Color) -> bool {
    (a.r - b.r).abs() < 0.05 && (a.g - b.g).abs() < 0.05 && (a.b - b.b).abs() < 0.05
}

/// **The acceptance test.** A `fragment()` body using a LOCAL VARIABLE and an
/// `if` — neither expressible in the old line matcher — changes scene pixels.
///
/// The sprite's texture is solid white, so any colour other than white or the
/// black clear proves the shader ran and produced its own value.
#[test]
fn shader_language_frontend_drives_scene_pixels() {
    // ⚠️ The threshold is MEASURED, not assumed.
    //
    // This previously hardcoded 0.75 and sampled columns 0 and 3, on the stated
    // assumption that UV.x runs ~0.56..0.94 across the covered region. Where the
    // sprite lands is exactly the half of the pipeline this bead is NOT about —
    // the sibling gradient test in `canvas_item_shader_scene_pixels.rs` avoids
    // depending on it for the same reason — and any change to the transform
    // silently turns the branch assertion into "both samples fell on the same
    // side of the threshold": a shader failure that is really a fixture failure.
    //
    // So render a UV ramp first, learn the real span, and put the threshold in
    // the middle of it. GREEN marks coverage, because an uncovered texel and a
    // covered texel at UV.x ≈ 0 are indistinguishable in the red channel alone.
    let probe = render(&sprite_scene_tscn(
        "shader_type canvas_item; void fragment() { COLOR.rgb = vec3(UV.x, 1.0, 0.0); }",
    ));
    let row = 2;
    let covered: Vec<(u32, f32)> = (0..SIZE)
        .map(|x| (x, pixel(&probe, x, row)))
        .filter(|(_, c)| c.g > 0.5)
        .map(|(x, c)| (x, c.r))
        .collect();
    assert!(
        covered.len() >= 2,
        "the UV probe must cover at least two texels in row {row} for there to be \
         a span to split; got {covered:?}. That is a coverage/fixture failure, \
         not a shader one"
    );

    let lo = covered
        .iter()
        .map(|(_, u)| *u)
        .fold(f32::INFINITY, f32::min);
    let hi = covered
        .iter()
        .map(|(_, u)| *u)
        .fold(f32::NEG_INFINITY, f32::max);
    assert!(
        hi - lo > 0.05,
        "UV.x must vary across the covered row (got {lo}..{hi}); a flat ramp means \
         UV never reached the shader at all"
    );
    let threshold = (lo + hi) * 0.5;
    let (left_x, _) = *covered
        .iter()
        .find(|(_, u)| *u < threshold)
        .expect("a covered texel below the midpoint");
    let (right_x, _) = *covered
        .iter()
        .find(|(_, u)| *u > threshold)
        .expect("a covered texel above the midpoint");

    // A local, a branch on UV, and a swizzle assignment. The old line matcher
    // recognised none of these and would have produced zero instructions.
    let code = format!(
        "shader_type canvas_item; void fragment() {{ vec3 tint = vec3(0.0, 0.0, 1.0); \
         if (UV.x > {threshold:.4}) {{ tint = vec3(1.0, 0.0, 0.0); }} COLOR.rgb = tint; }}"
    );
    let shaded = render(&sprite_scene_tscn(&code));
    let left = pixel(&shaded, left_x, row);
    let right = pixel(&shaded, right_x, row);

    let blue = Color::rgb(0.0, 0.0, 1.0);
    let red = Color::rgb(1.0, 0.0, 0.0);
    assert!(
        close(left, blue),
        "column {left_x} measured UV.x below {threshold:.4}, so the else-arm's blue \
         must reach it; got {left:?}. White means the shader body was skipped \
         entirely — the line-matcher behaviour this bead replaces"
    );
    assert!(
        close(right, red),
        "column {right_x} measured UV.x above {threshold:.4}, so the if-arm's red \
         must reach it; got {right:?}"
    );
    assert!(
        !close(left, right),
        "the two arms must differ — identical halves mean the `if` never ran"
    );
}

/// A `max()` call and arithmetic on a local — the other shapes the matcher
/// could not represent.
#[test]
fn builtin_calls_and_locals_reach_scene_pixels() {
    let code = "shader_type canvas_item; void fragment() { float a = 0.25; \
                float b = max(a, 0.75); COLOR.rgb = vec3(b, 0.0, 0.0); }";
    let shaded = render(&sprite_scene_tscn(code));
    // (2,2), not (4,4): see `render`'s note — the sprite covers the upper-left
    // quadrant whether or not the authored `position` is applied, and (4,4) is
    // outside it. This shader's output is uniform, so any covered texel proves
    // the claim.
    let got = pixel(&shaded, 2, 2);
    assert!(
        close(got, Color::rgb(0.75, 0.0, 0.0)),
        "max(0.25, 0.75) must reach the pixel as 0.75 red; got {got:?}. \
         Pure black would mean the texel is not covered at all, not that the \
         shader computed black."
    );
}

/// The control that makes the assertions above meaningful: the SAME scene with
/// no material renders the plain white texel, so the colours above are the
/// shader's doing and not an artefact of the fixture or the clear colour.
#[test]
fn without_a_shader_the_sprite_is_unshaded() {
    const PLAIN: &str = r#"[gd_scene format=3]

[node name="Plain" type="Sprite2D"]
texture = "tex"
position = Vector2(4, 4)
"#;
    let plain = render(PLAIN);
    let got = pixel(&plain, 2, 2);
    assert!(
        close(got, Color::rgb(1.0, 1.0, 1.0)),
        "an unshaded sprite must render its plain white texel; got {got:?}. If \
         this is not white, the fixture — not the shader — is producing the \
         colours asserted above."
    );
}
