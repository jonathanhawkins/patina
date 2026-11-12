//! pat-s3rrv.208 — a `NoiseTexture2D` over a `FastNoiseLite` renders generated
//! pixels through the ordinary 2D draw path.
//!
//! `gdresource::noise` has implemented FastNoiseLite since pat-nzgdu.219 and had
//! **no caller anywhere in the workspace** — the orphaned-module pattern. The
//! editor meanwhile offered "NoiseTexture2D" in its Texture2D picker, so the
//! user could pick a class nothing could construct.
//!
//! 🔑 **Two hops, and the second is the one that breaks.** The Sprite2D names a
//! NoiseTexture2D, whose own `noise` property names a FastNoiseLite in a
//! separate `[sub_resource]` section. Resolving only the first hop yields a
//! texture with no generator — a blank image, which looks exactly like a
//! texture that failed to load.

use gdcore::math::Color;
use gdeditor::texture_cache::TextureCache;
use gdeditor::viewport_2d_render::render_runtime_canvas;
use gdobject::class_db::register_all_classes;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;

const W: u32 = 96;
const H: u32 = 96;
const CLEAR: Color = Color {
    r: 0.0,
    g: 0.0,
    b: 0.0,
    a: 1.0,
};

/// A Sprite2D over a NoiseTexture2D over a FastNoiseLite.
///
/// ⚠️ `noise_type = 3` is PERLIN, stated explicitly. Godot's DEFAULT is 1
/// (SIMPLEX_SMOOTH), which this engine does not implement — it would be
/// substituted, and a fixture resting on a substitution would be measuring the
/// fallback rather than the feature.
///
/// `frequency` is raised well above Godot's 0.01 default so that adjacent
/// pixels differ visibly across a 64px sprite; at 0.01 a 64px window samples a
/// span of 0.64 in noise space and comes out nearly flat, which would make
/// "the pixels vary" fail for a reason that has nothing to do with wiring.
fn scene(seed: i64, frequency: &str) -> String {
    format!(
        r#"[gd_scene load_steps=3 format=3]

[sub_resource type="FastNoiseLite" id="FastNoiseLite_1"]
noise_type = 3
seed = {seed}
frequency = {frequency}

[sub_resource type="NoiseTexture2D" id="NoiseTexture2D_1"]
width = 64
height = 64
noise = SubResource("FastNoiseLite_1")

[node name="Root" type="Node2D"]

[node name="Patch" type="Sprite2D" parent="."]
position = Vector2(48, 48)
texture = SubResource("NoiseTexture2D_1")
"#
    )
}

fn render(seed: i64, frequency: &str) -> Vec<Color> {
    // Required: `collect_canvas_items` filters on `is_class("CanvasItem")`,
    // which walks the ClassDB chain. Without this the render list is empty and
    // every assertion below compares black to black.
    register_all_classes();

    let src = scene(seed, frequency);
    let parsed = PackedScene::from_tscn(&src).expect("scene parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &parsed).expect("scene instances");

    let mut cache = TextureCache::new(".");
    render_runtime_canvas(&tree, W, H, CLEAR, Some(&mut cache)).pixels
}

fn luma(c: Color) -> f32 {
    (c.r + c.g + c.b) / 3.0
}

/// Pixels the sprite actually covers, discovered rather than assumed.
fn lit(px: &[Color]) -> Vec<f32> {
    px.iter().map(|c| luma(*c)).filter(|l| *l > 1e-6).collect()
}

/// The sprite draws at all. 🔑 Its `texture` is `SubResource:NoiseTexture2D_1`,
/// which names no file — if the generated texture never reaches the cache, the
/// sprite draws nothing and every variance assertion below passes vacuously on
/// an empty set.
#[test]
fn the_generated_texture_reaches_the_draw_path() {
    let px = render(7, "0.15");
    assert!(
        lit(&px).len() > 256,
        "the sprite must cover a meaningful area; {} lit pixels means the \
         `noise://` key was never baked into the cache and nothing was drawn",
        lit(&px).len()
    );
}

/// **The acceptance: the pixels VARY.** A noise field that renders as a flat
/// block is the failure this bead exists to prevent — it is what a placeholder
/// texture looks like.
#[test]
fn the_rendered_noise_is_not_uniform() {
    let values = lit(&render(7, "0.15"));
    assert!(!values.is_empty(), "the sprite must render");

    let min = values.iter().cloned().fold(f32::INFINITY, f32::min);
    let max = values.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
    assert!(
        max - min > 0.05,
        "generated noise must vary across the sprite; a spread of {} means a \
         flat fill, which is exactly what a placeholder or an ungenerated \
         texture produces",
        max - min
    );
}

/// **Deterministic in seed.** Same seed twice is identical; a different seed is
/// a different field.
///
/// ⚠️ Both halves are needed. Equality alone passes if the renderer ignores the
/// noise entirely and draws a constant, and difference alone passes if the
/// output is random per run rather than seeded.
#[test]
fn the_field_is_deterministic_and_seed_dependent() {
    let a = render(7, "0.15");
    let again = render(7, "0.15");
    let other = render(99, "0.15");

    assert_eq!(
        a, again,
        "the same seed must produce byte-identical pixels, or the bake is not \
         deterministic and no golden test of a noise scene can ever be stable"
    );
    assert!(
        a != other,
        "a different `seed` must produce a different field; identical output \
         means `seed` never reached the generator"
    );
}

/// `frequency` reaches the generator too — the other half of the acceptance's
/// "changes deterministically with seed/frequency".
#[test]
fn frequency_changes_the_field() {
    let coarse = render(7, "0.05");
    let fine = render(7, "0.30");
    assert!(
        coarse != fine,
        "changing `frequency` must change the rendered field; identical output \
         means the property was parsed but never applied"
    );
}

/// The control for the variance test: a sprite with NO noise resource must not
/// somehow produce a varying field.
///
/// 🔑 This is what makes "the pixels vary" mean "the noise arrived" rather than
/// "something in the pipeline happens to be non-uniform" — a gradient, a
/// lighting pass, or an uninitialised buffer would all satisfy the variance
/// assertion on their own.
#[test]
fn a_sprite_with_no_noise_resource_does_not_render_a_field() {
    register_all_classes();
    const PLAIN: &str = r#"[gd_scene load_steps=2 format=3]

[sub_resource type="NoiseTexture2D" id="NoiseTexture2D_2"]
width = 64
height = 64

[node name="Root" type="Node2D"]

[node name="Patch" type="Sprite2D" parent="."]
position = Vector2(48, 48)
texture = SubResource("NoiseTexture2D_2")
"#;
    let parsed = PackedScene::from_tscn(PLAIN).expect("scene parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &parsed).expect("scene instances");
    let mut cache = TextureCache::new(".");
    let px = render_runtime_canvas(&tree, W, H, CLEAR, Some(&mut cache)).pixels;

    let values = lit(&px);
    let spread = if values.is_empty() {
        0.0
    } else {
        values.iter().cloned().fold(f32::NEG_INFINITY, f32::max)
            - values.iter().cloned().fold(f32::INFINITY, f32::min)
    };
    assert!(
        spread < 0.05,
        "a NoiseTexture2D with no `noise` resource has nothing to generate \
         from, so it must not produce a varying field; a spread of {spread} \
         means the variance test above could pass without the generator"
    );
}
