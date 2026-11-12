//! pat-0q2kx.48 acceptance: a `ShaderMaterial` authored in a `.tscn` on a
//! CanvasItem actually runs when the scene is rendered.
//!
//! The whole 2D shader stack existed and was unit-tested — the `gdserver2d`
//! compiler, `CanvasItem::set_material`, and `rasterize_item`'s material branch —
//! but **nothing called it**. `grep canvas_item_set_material` found exactly one
//! hit, its own definition. Two links were missing: `packed_scene` flattened a
//! ShaderMaterial only for the 3D `material_override`/`surface_material_override`
//! slots, never the CanvasItem `material` property, and the scene-to-render-list
//! walk — the sole place a `SceneTree` becomes a render list — built every
//! `CanvasItem` without a material. So an authored 2D shader was dropped at load and, even if
//! it had survived, never reached the rasterizer.
//!
//! These drive the real path: `.tscn` text → `PackedScene::from_tscn` →
//! `add_packed_scene_to_tree` → `viewport_2d_render` (the ONE shipped canvas path) → pixels.
//! No test-only shims, and the shader forms used here are the ones
//! `gdrender2d/tests/canvas_item_shader.rs` already proves the interpreter
//! supports, so a failure here means the *wiring* broke, not the shader language.

use gdcore::math::{Color, Vector2};
use gdeditor::texture_cache::TextureCache;
use gdeditor::viewport_2d_render::{build_runtime_render_list, rasterize_render_list};
use gdrender2d::texture::Texture2D;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;
use gdvariant::Variant;

const SIZE: u32 = 8;

/// A one-Sprite2D scene whose `material` is a ShaderMaterial over an inline
/// `[sub_resource type="Shader"]`, with `params` spliced into the material.
///
/// `texture = "tex"` is a bare string because `node2d::get_texture_path` reads the
/// `texture` property as a plain path; the renderer registers the same key.
fn sprite_scene_tscn(code: &str, params: &str) -> String {
    format!(
        r#"[gd_scene load_steps=3 format=3]

[sub_resource type="Shader" id="Shader_1"]
code = "{code}"

[sub_resource type="ShaderMaterial" id="ShaderMaterial_1"]
shader = SubResource("Shader_1")
{params}

[node name="Shaded" type="Sprite2D"]
texture = "tex"
position = Vector2(4, 4)
material = SubResource("ShaderMaterial_1")
"#
    )
}

/// Instances `tscn` into a tree and renders it, returning the frame pixels.
///
/// The sprite is centered (Godot's default) over an 8x8 texture. Assertions
/// sample the upper-left quadrant, which the sprite covers whether or not the
/// authored `position` is applied — the shading claim should not rest on the
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

    // The shipped path resolves textures through a TextureCache rather than a
    // size closure + renderer registry (pat-s3rrv.22).
    let mut textures = TextureCache::new(".");
    textures.insert("tex", Texture2D::solid(SIZE, SIZE, Color::WHITE));

    let items = build_runtime_render_list(&tree);
    let frame = rasterize_render_list(&items, SIZE, SIZE, Color::BLACK, Some(&mut textures));
    assert_eq!(frame.pixels.len(), (SIZE * SIZE) as usize, "frame extent");
    frame.pixels
}

fn pixel(pixels: &[Color], x: u32, y: u32) -> Color {
    pixels[(y * SIZE + x) as usize]
}

fn close(a: Color, b: Color) -> bool {
    (a.r - b.r).abs() < 0.02 && (a.g - b.g).abs() < 0.02 && (a.b - b.b).abs() < 0.02
}

/// **The acceptance test.** A `.tscn`-authored canvas_item shader paints the
/// sprite its shader's colour rather than the texture's, and editing the
/// `shader_parameter` changes the pixels.
///
/// The texture is deliberately WHITE, so any non-white result can only have come
/// from the shader — an unwired material would leave a white sprite.
#[test]
fn canvas_item_shader_scene_pixels() {
    let code = "shader_type canvas_item; uniform vec4 tint; void fragment() { COLOR = tint; }";

    let green = render(&sprite_scene_tscn(
        code,
        "shader_parameter/tint = Color(0, 1, 0, 1)",
    ));
    let center = pixel(&green, 2, 2);
    assert!(
        close(center, Color::rgb(0.0, 1.0, 0.0)),
        "the authored shader must paint the sprite green, not the white texel — \
         got {center:?}. White here means the ShaderMaterial never reached the \
         rasterizer.",
    );

    // Same scene, one parameter changed: the pixels must follow it. This is what
    // separates "a shader ran" from "the material's parameters are live".
    let blue = render(&sprite_scene_tscn(
        code,
        "shader_parameter/tint = Color(0, 0, 1, 1)",
    ));
    let blue_center = pixel(&blue, 2, 2);
    assert!(
        close(blue_center, Color::rgb(0.0, 0.0, 1.0)),
        "changing shader_parameter/tint must change the pixels — got {blue_center:?}",
    );
    assert!(
        !close(center, blue_center),
        "the two parameter values must not render identically",
    );
}

/// A scene with no material renders the plain texel — the control that makes the
/// acceptance assertion meaningful rather than a coincidence of the clear colour.
#[test]
fn sprite_without_material_renders_unshaded() {
    let tscn = r#"[gd_scene format=3]

[node name="Plain" type="Sprite2D"]
texture = "tex"
position = Vector2(4, 4)
"#;
    let center = pixel(&render(tscn), 2, 2);
    assert!(
        close(center, Color::WHITE),
        "an unshaded sprite must render its white texel, got {center:?}",
    );
}

/// The `UV` builtin is fed real per-fragment coordinates through the scene path,
/// so a gradient shader renders a gradient — a single flat colour would pass the
/// acceptance test above but still be wrong.
#[test]
fn scene_shader_uv_gradient_varies_across_the_sprite() {
    let code = "shader_type canvas_item; void fragment() { COLOR.a = UV.x; }";
    let pixels = render(&sprite_scene_tscn(code, ""));

    // Composited over black, alpha ramping with UV.x brightens left→right.
    let left = pixel(&pixels, 0, 2);
    let right = pixel(&pixels, 3, 2);
    // A threshold, not an exact value: the sampled UVs depend on where the
    // sprite lands, and the claim under test is only that UV.x *varies* across
    // it. A flat colour — the failure this guards — gives a difference of 0.
    assert!(
        right.r > left.r + 0.2,
        "UV.x must ramp across the sprite: left {left:?} vs right {right:?}",
    );
}

/// A `ColorRect`'s ShaderMaterial reaches the node as flat properties — the
/// `packed_scene` half of this bead — even though its pixels are not yet shaded.
///
/// The canvas walk sets the material on every CanvasItem it builds, ColorRect
/// included, but `rasterize_item` consults the shader only for
/// `DrawCommand::DrawTextureRect` (gdrender2d/src/renderer.rs:416); `DrawRect`,
/// which is what `color_rect_commands` emits, has no shader branch at all. So
/// this asserts the wiring that exists and deliberately does NOT assert shaded
/// ColorRect pixels — that needs a shaded fill path in `gdrender2d` and is
/// reported as a follow-up rather than silently claimed here.
#[test]
fn color_rect_shader_material_flattens_onto_the_node() {
    let tscn = r#"[gd_scene load_steps=3 format=3]

[sub_resource type="Shader" id="Shader_1"]
code = "shader_type canvas_item; void fragment() { COLOR = vec4(UV.x, 0.0, 0.0, 1.0); }"

[sub_resource type="ShaderMaterial" id="ShaderMaterial_1"]
shader = SubResource("Shader_1")
shader_parameter/strength = 0.25

[node name="Rect" type="ColorRect"]
material = SubResource("ShaderMaterial_1")
"#;
    // ⚠️ Without a populated ClassDB there is no class chain, so
    // `Node::is_class("CanvasItem")` is false for every node and
    // `collect_canvas_items` filters the whole scene out and returns `[]`
    // (pat-s3rrv.28).
    gdobject::class_db::register_2d_classes();
    let scene = PackedScene::from_tscn(tscn).expect("scene must parse");
    let nodes = scene.instance().expect("scene must instance");
    let rect = &nodes[0];
    assert_eq!(rect.name(), "Rect");

    match rect.get_property("shader_code") {
        Variant::String(code) => assert!(
            code.contains("shader_type canvas_item"),
            "the CanvasItem `material` property must flatten its shader source \
             onto the node; got {code}",
        ),
        other => panic!(
            "expected shader_code on the ColorRect — the flattening pass only \
             matched the 3D override slots before this bead; got {other:?}"
        ),
    }
    assert_eq!(
        rect.get_property("shader_parameter/strength"),
        Variant::Float(0.25),
        "shader parameters must travel with the shader",
    );
}
