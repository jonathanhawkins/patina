//! pat-s3rrv.28 acceptance — an authored `CanvasItemMaterial`, a `.gdshader` on
//! a `CanvasItem`, and `texture_filter` all change the pixels of the **Play**
//! frame.
//!
//! Godot has exactly one canvas draw path: every item composites through its
//! `CanvasItemMaterial.blend_mode`, runs its `ShaderMaterial`'s canvas_item
//! `fragment()`, and samples every texture through a `texture_filter` /
//! `texture_repeat` resolved up the parent chain
//! (`servers/rendering/renderer_canvas_render.cpp`).
//!
//! Patina had two 2D pipelines and the wrong one was live. The feature-complete
//! `gdrender2d::SoftwareRenderer::rasterize_item` honoured all of it, but its
//! only feeder — `gdscene::scene_bridge` — had zero production callers and has
//! since been deleted. The served Play loop goes through
//! `gdeditor::viewport_2d_render`, whose `Render2DItem` carried no material,
//! filter or repeat field at all: an authored CanvasItemMaterial composited as
//! plain Mix, a `.gdshader` on a Sprite2D did nothing, and `texture_filter =
//! LINEAR` was inert however the scene set it.
//!
//! Every assertion here drives the shipped path end to end — `.tscn` text →
//! `PackedScene::from_tscn` → `add_packed_scene_to_tree` →
//! `build_runtime_render_list` → `rasterize_render_list` — and reads pixels. No
//! test-only shims and no assertions on the render list's fields: a field that
//! is populated but never consumed is exactly the failure this bead exists to
//! close.

use gdcore::math::Color;
use gdeditor::texture_cache::TextureCache;
use gdeditor::viewport_2d_render::{build_runtime_render_list, rasterize_render_list};
use gdrender2d::renderer::FrameBuffer;
use gdrender2d::texture::Texture2D;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;
use gdvariant::Variant;

const W: u32 = 64;
const H: u32 = 64;
const BG: Color = Color::new(0.0, 0.0, 0.0, 1.0);

/// Instances `tscn` and rasterizes it exactly as the Play loop does.
///
/// `textures` is the same `TextureCache` the served path resolves sprite
/// textures through, so a test texture is registered rather than injected by a
/// size closure.
fn render(tscn: &str, textures: Option<&mut TextureCache>) -> FrameBuffer {
    // ⚠️ Without a populated ClassDB there is no class chain, so
    // `Node::is_class("CanvasItem")` is false for every node and
    // `collect_canvas_items` filters the whole scene out and returns `[]` —
    // which reads like the material was not carried rather than like the scene
    // was never collected. Any test that loads a `.tscn` and expects the canvas
    // walk to see its nodes needs this.
    gdobject::class_db::register_2d_classes();
    let scene = PackedScene::from_tscn(tscn).expect("scene parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &scene).expect("scene instances");
    let items = build_runtime_render_list(&tree);
    rasterize_render_list(&items, W, H, BG, textures)
}

// ---------------------------------------------------------------------------
// (1) CanvasItemMaterial.blend_mode
// ---------------------------------------------------------------------------

/// Two overlapping half-grey `ColorRect`s. The `blend_mode` of the top one is
/// the ONLY difference between the two scenes this builds.
fn overlap_scene(blend_mode: Option<i32>) -> String {
    let (sub_resource, material_line) = match blend_mode {
        Some(mode) => (
            format!(
                "\n[sub_resource type=\"CanvasItemMaterial\" id=\"CanvasItemMaterial_1\"]\nblend_mode = {mode}\n"
            ),
            "material = SubResource(\"CanvasItemMaterial_1\")",
        ),
        None => (String::new(), ""),
    };
    format!(
        r#"[gd_scene load_steps=2 format=3]
{sub_resource}
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
"#
    )
}

// ---------------------------------------------------------------------------
// (2) a canvas_item shader on a Sprite2D
// ---------------------------------------------------------------------------

/// A single `Sprite2D` over a **white** texture, magnified to a 32x32 quad
/// centred in the frame, carrying `code` as its canvas_item shader.
///
/// The texture is deliberately white so any non-white pixel can only have come
/// from the shader — an unwired material leaves a white sprite, which is
/// precisely the bug.
fn shaded_sprite_scene(code: &str) -> String {
    format!(
        r#"[gd_scene load_steps=3 format=3]

[sub_resource type="Shader" id="Shader_1"]
code = "{code}"

[sub_resource type="ShaderMaterial" id="ShaderMaterial_1"]
shader = SubResource("Shader_1")

[node name="Shaded" type="Sprite2D"]
position = Vector2(32, 32)
scale = Vector2(4, 4)
material = SubResource("ShaderMaterial_1")
"#
    )
}

/// An 8x8 all-white texture, and the cache the sprite scenes resolve it through.
fn white_texture_cache() -> TextureCache {
    let mut textures = TextureCache::new(".");
    textures.insert("res://white.png", Texture2D::solid(8, 8, Color::WHITE));
    textures
}

/// Renders a `Sprite2D` scene, pointing the sprite at the registered texture.
///
/// The `texture` property is set after instancing (rather than in the `.tscn`)
/// so the scene text stays about the property under test.
fn render_sprite(tscn: &str, textures: &mut TextureCache) -> FrameBuffer {
    // ⚠️ Without a populated ClassDB there is no class chain, so
    // `Node::is_class("CanvasItem")` is false for every node and
    // `collect_canvas_items` filters the whole scene out and returns `[]` —
    // which reads like the material was not carried rather than like the scene
    // was never collected. Any test that loads a `.tscn` and expects the canvas
    // walk to see its nodes needs this.
    gdobject::class_db::register_2d_classes();
    let scene = PackedScene::from_tscn(tscn).expect("scene parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let scene_root = add_packed_scene_to_tree(&mut tree, root, &scene).expect("scene instances");
    // The scene root IS the sprite in these one-node scenes.
    tree.get_node_mut(scene_root)
        .expect("sprite node")
        .set_property("texture", Variant::String("res://white.png".into()));
    let items = build_runtime_render_list(&tree);
    rasterize_render_list(&items, W, H, BG, Some(textures))
}

// ---------------------------------------------------------------------------
// (3) texture_filter
// ---------------------------------------------------------------------------

/// A 2x2 black/white checker. Magnified, NEAREST keeps hard texel edges while
/// LINEAR produces intermediate greys across the boundary.
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

/// The checker on a `Sprite2D` blown up to a 32x32 quad centred in the frame.
///
/// ⚠️ The magnification is the whole point: a `Sprite2D` draws at its texture's
/// pixel size, so an unscaled 2x2 checker covers four pixels and no filter can
/// be told from any other. `scale = Vector2(16, 16)` puts the texel boundary
/// across the middle of the frame where the difference is visible.
fn checker_scene(filter: i32) -> String {
    format!(
        r#"[gd_scene format=3]

[node name="Sprite" type="Sprite2D"]
position = Vector2(32, 32)
scale = Vector2(16, 16)
texture_filter = {filter}
"#
    )
}

/// Renders the checker scene with the sprite pointed at the checker texture.
fn render_checker(filter: i32) -> FrameBuffer {
    // ⚠️ Without a populated ClassDB there is no class chain, so
    // `Node::is_class("CanvasItem")` is false for every node and
    // `collect_canvas_items` filters the whole scene out and returns `[]` —
    // which reads like the material was not carried rather than like the scene
    // was never collected. Any test that loads a `.tscn` and expects the canvas
    // walk to see its nodes needs this.
    gdobject::class_db::register_2d_classes();
    let scene = PackedScene::from_tscn(&checker_scene(filter)).expect("scene parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let scene_root = add_packed_scene_to_tree(&mut tree, root, &scene).expect("scene instances");
    tree.get_node_mut(scene_root)
        .expect("sprite node")
        .set_property("texture", Variant::String("res://checker.png".into()));
    let mut textures = TextureCache::new(".");
    textures.insert("res://checker.png", checker());
    let items = build_runtime_render_list(&tree);
    rasterize_render_list(&items, W, H, BG, Some(&mut textures))
}

/// The row through the middle of the magnified checker: it crosses the texel
/// boundary, so it is where interpolation shows up if it happens at all.
const CHECKER_ROW: u32 = 32;

/// Whether any pixel on `y` is a mid-grey — the signature of bilinear
/// interpolation across a hard black/white texel boundary. The clear colour is
/// pure black and both texels are pure black or white, so nothing else in the
/// frame can produce one.
fn has_intermediate(fb: &FrameBuffer, y: u32) -> bool {
    (0..W).any(|x| {
        let c = fb.get_pixel(x, y);
        c.r > 0.15 && c.r < 0.85
    })
}

// ---------------------------------------------------------------------------
// The acceptance test
// ---------------------------------------------------------------------------

/// **The acceptance test** for pat-s3rrv.28: material state reaches the Play
/// rasterizer's pixels.
///
/// Three independent claims, each with its own control so a pass cannot be a
/// coincidence of the clear colour:
///
/// 1. `blend_mode = 1` (ADD) composites brighter than the MIX baseline;
/// 2. a canvas_item shader writing `COLOR.rgb = vec3(0, 1, 0)` paints a white
///    sprite green;
/// 3. `texture_filter = 2` (LINEAR) produces intermediate pixels across a
///    magnified texel edge where `1` (NEAREST) produces only source values.
#[test]
fn canvas_item_material_and_sampling_in_play_frame() {
    // -- (1) CanvasItemMaterial.blend_mode ---------------------------------
    let mix = render(&overlap_scene(None), None);
    let add = render(&overlap_scene(Some(1)), None);

    let mix_px = mix.get_pixel(20, 20);
    let add_px = add.get_pixel(20, 20);
    // Baseline: opaque grey over opaque grey is still grey. If this drifts the
    // comparison below means nothing.
    assert!(
        (mix_px.r - 0.5).abs() < 0.05,
        "the MIX baseline must stay at the source grey, got {mix_px:?}",
    );
    assert!(
        add_px.r > mix_px.r + 0.3,
        "blend_mode = ADD must composite brighter than MIX: add {add_px:?} vs \
         mix {mix_px:?}. Equal values mean the CanvasItemMaterial never reached \
         the rasterizer.",
    );
    assert!(
        add_px.r > 0.9,
        "0.5 + 0.5 must saturate toward 1.0, got {add_px:?}",
    );

    // -- (2) a canvas_item ShaderMaterial ----------------------------------
    let mut textures = white_texture_cache();
    let shaded = render_sprite(
        &shaded_sprite_scene(
            "shader_type canvas_item; void fragment() { COLOR.rgb = vec3(0.0, 1.0, 0.0); }",
        ),
        &mut textures,
    );
    let green = shaded.get_pixel(32, 32);
    assert!(
        green.g > 0.9 && green.r < 0.1 && green.b < 0.1,
        "the authored shader must paint the sprite green, not the white texel — \
         got {green:?}. White here means the ShaderMaterial never reached the \
         rasterizer.",
    );

    // The control: the same scene with no material renders the plain white
    // texel, so the green above is the shader's doing and not the sprite
    // failing to draw.
    let plain = render_sprite(
        r#"[gd_scene format=3]

[node name="Plain" type="Sprite2D"]
position = Vector2(32, 32)
scale = Vector2(4, 4)
"#,
        &mut textures,
    );
    let unshaded = plain.get_pixel(32, 32);
    assert!(
        unshaded.r > 0.9 && unshaded.g > 0.9 && unshaded.b > 0.9,
        "an unshaded sprite must render its white texel, got {unshaded:?}",
    );

    // -- (3) texture_filter -------------------------------------------------
    let linear = render_checker(2);
    let nearest = render_checker(1);
    assert!(
        has_intermediate(&linear, CHECKER_ROW),
        "texture_filter = LINEAR must produce interpolated greys across the \
         magnified texel edge; the row was entirely source values, which is \
         what a hardcoded nearest sample gives",
    );
    assert!(
        !has_intermediate(&nearest, CHECKER_ROW),
        "texture_filter = NEAREST must keep hard texel edges — every pixel on \
         the row should be fully black or fully white",
    );
}
