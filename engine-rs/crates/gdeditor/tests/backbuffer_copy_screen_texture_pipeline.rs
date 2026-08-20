//! pat-s3rrv.202 — a `BackBufferCopy` snapshot reaches a canvas_item shader
//! through `hint_screen_texture` / `SCREEN_TEXTURE`.
//!
//! Godot ground truth: `scene/2d/back_buffer_copy.cpp`,
//! `RenderingServer.canvas_item_set_copy_to_backbuffer`, and the canvas_item
//! `hint_screen_texture` / `SCREEN_TEXTURE` + `SCREEN_UV` built-ins. This is how
//! 2D distortion, blur and glass effects are built.
//!
//! # What was missing
//!
//! pat-s3rrv.183 already built most of this: `Render2DKind::BackBufferCopy`
//! exists, the rasterizer snapshots the framebuffer into a screen texture and
//! honours `copy_mode = 0`, and the snapshot is handed to the shaded draw.
//!
//! ⚠️ The gap was the NAME. Godot allows two spellings for a back-buffer read —
//! the legacy built-in `SCREEN_TEXTURE`, and a user-named
//! `uniform sampler2D screen_tex : hint_screen_texture`. The AST evaluator hands
//! the sampler the LITERAL name, and only the legacy spelling was matched. A
//! hint-declared uniform fell through to the item's OWN texture, so a sprite
//! sampled itself — which still produces plausible pixels and reads as a
//! working screen effect. That is why this needs a pixel test and not a
//! "does it compile" one.

use gdcore::math::{Color, Vector2};
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

/// `[ColorRect(red) -> BackBufferCopy(copy_mode) -> Sprite2D(shader)]`.
///
/// The sprite's own texture is WHITE, so any red in its pixels can only have
/// come from sampling the back buffer — a sprite that sampled itself stays
/// white, which is exactly the failure this test exists to catch.
///
/// ⚠️ Draw ORDER carries the meaning: the ColorRect must be painted before the
/// BackBufferCopy runs, and the sprite after it. Godot snapshots at the copy
/// node's position in the draw order, so moving it would change what is
/// captured.
fn scene(copy_mode: i64) -> String {
    format!(
        r#"[gd_scene load_steps=3 format=3]

[sub_resource type="Shader" id="Shader_1"]
code = "shader_type canvas_item; uniform sampler2D screen_tex : hint_screen_texture; void fragment() {{ COLOR = texture(screen_tex, SCREEN_UV + vec2(0.05, 0.0)); }}"

[sub_resource type="ShaderMaterial" id="ShaderMaterial_1"]
shader = SubResource("Shader_1")

[node name="Main" type="Node2D"]

[node name="Backdrop" type="ColorRect" parent="."]
offset_left = 0.0
offset_top = 0.0
size = Vector2(64, 64)
color = Color(1, 0, 0, 1)

[node name="Copy" type="BackBufferCopy" parent="."]
copy_mode = {copy_mode}

[node name="Shaded" type="Sprite2D" parent="."]
position = Vector2(32, 32)
scale = Vector2(8, 8)
material = SubResource("ShaderMaterial_1")
"#
    )
}

fn render(copy_mode: i64) -> FrameBuffer {
    // ⚠️ Without a populated ClassDB there is no class chain, so
    // `Node::is_class("CanvasItem")` is false for every node and
    // `collect_canvas_items` filters the whole scene out — which reads like the
    // shader was not carried rather than like nothing was collected.
    gdobject::class_db::register_2d_classes();

    let mut textures = TextureCache::new(".");
    textures.insert("res://white.png", Texture2D::solid(8, 8, Color::WHITE));

    let tscn = scene(copy_mode);
    let packed = PackedScene::from_tscn(&tscn).expect("scene parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let scene_root = add_packed_scene_to_tree(&mut tree, root, &packed).expect("scene instances");

    // Point the sprite at the registered white texture, so the `.tscn` text
    // stays about the shader under test.
    let sprite = tree
        .all_nodes_in_tree_order()
        .into_iter()
        .find(|id| {
            tree.get_node(*id)
                .map(|n| n.name() == "Shaded")
                .unwrap_or(false)
        })
        .expect("the shaded sprite");
    let _ = scene_root;
    tree.get_node_mut(sprite)
        .expect("sprite node")
        .set_property("texture", Variant::String("res://white.png".into()));

    let items = build_runtime_render_list(&tree);
    rasterize_render_list(&items, W, H, BG, Some(&mut textures))
}

/// The pixel at the centre of the frame, where the shaded sprite is.
fn centre(fb: &FrameBuffer) -> Color {
    fb.pixels[(H / 2 * W + W / 2) as usize]
}

/// 🔑 **THE ACCEPTANCE.** With `copy_mode = 2` (VIEWPORT) the shader samples the
/// captured red backdrop; with `copy_mode = 0` (DISABLED) it has no back buffer
/// to read.
#[test]
fn backbuffer_copy_screen_texture_pipeline() {
    let copied = render(2);
    let c = centre(&copied);

    // The whole backdrop is red, so a 5% SCREEN_UV shift still lands on red —
    // the shift proves the sampler read the BUFFER rather than the sprite.
    assert!(
        c.r > 0.5 && c.g < 0.3 && c.b < 0.3,
        "with copy_mode = VIEWPORT the sprite must show the captured RED \
         backdrop, got {c:?}. A white pixel means the sampler fell through to \
         the sprite's own white texture — which is what happens when a \
         `hint_screen_texture` uniform is not recognised and only the legacy \
         `SCREEN_TEXTURE` spelling is matched"
    );

    // ⚠️ The control. Without it, "the pixel is red" would pass just as happily
    // against a build that painted the backdrop straight through the sprite,
    // with no shader or back buffer involved at all.
    let disabled = render(0);
    let d = centre(&disabled);
    assert!(
        d.r <= 0.5 || d.g > 0.3,
        "with copy_mode = DISABLED there is no back buffer, so the sprite must \
         NOT come out red like the copied case, got {d:?}"
    );
    assert_ne!(
        (c.r > 0.5 && c.g < 0.3),
        (d.r > 0.5 && d.g < 0.3),
        "copy_mode must be what changes the result; identical output for \
         VIEWPORT and DISABLED means the snapshot is not gating anything"
    );
}

/// The legacy `SCREEN_TEXTURE` spelling keeps working.
///
/// ⚠️ Both spellings are asserted because the fix widened a match that
/// previously accepted only this one. A change that swapped hint support IN and
/// legacy support OUT would pass the acceptance above and silently break every
/// shader already written against `SCREEN_TEXTURE`.
#[test]
fn the_legacy_screen_texture_spelling_still_resolves() {
    gdobject::class_db::register_2d_classes();
    let mut textures = TextureCache::new(".");
    textures.insert("res://white.png", Texture2D::solid(8, 8, Color::WHITE));

    let tscn = r#"[gd_scene load_steps=3 format=3]

[sub_resource type="Shader" id="Shader_1"]
code = "shader_type canvas_item; void fragment() { COLOR = texture(SCREEN_TEXTURE, SCREEN_UV); }"

[sub_resource type="ShaderMaterial" id="ShaderMaterial_1"]
shader = SubResource("Shader_1")

[node name="Main" type="Node2D"]

[node name="Backdrop" type="ColorRect" parent="."]
offset_left = 0.0
offset_top = 0.0
size = Vector2(64, 64)
color = Color(0, 1, 0, 1)

[node name="Copy" type="BackBufferCopy" parent="."]
copy_mode = 2

[node name="Shaded" type="Sprite2D" parent="."]
position = Vector2(32, 32)
scale = Vector2(8, 8)
material = SubResource("ShaderMaterial_1")
"#;

    let packed = PackedScene::from_tscn(tscn).expect("scene parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &packed).expect("scene instances");
    let sprite = tree
        .all_nodes_in_tree_order()
        .into_iter()
        .find(|id| {
            tree.get_node(*id)
                .map(|n| n.name() == "Shaded")
                .unwrap_or(false)
        })
        .expect("the shaded sprite");
    tree.get_node_mut(sprite)
        .expect("sprite node")
        .set_property("texture", Variant::String("res://white.png".into()));

    let items = build_runtime_render_list(&tree);
    let fb = rasterize_render_list(&items, W, H, BG, Some(&mut textures));
    let c = centre(&fb);
    assert!(
        c.g > 0.5 && c.r < 0.3,
        "the legacy SCREEN_TEXTURE built-in must still read the back buffer \
         (green backdrop), got {c:?}"
    );
}

/// ⚠️ A `hint_screen_texture` shader is no longer reported as unsupported.
///
/// The entry claimed the feature was missing while the CPU path implemented it
/// end to end — the inverse of the usual defect, and just as misleading: it
/// sends the next reader to reimplement something that already runs.
#[test]
fn screen_texture_is_no_longer_reported_unsupported() {
    let src = "shader_type canvas_item; uniform sampler2D screen_tex : hint_screen_texture; \
               void fragment() { COLOR = texture(screen_tex, SCREEN_UV); }";
    let missing =
        gdserver2d::shader::unsupported_features(src, gdserver2d::shader::ShaderType::CanvasItem);
    assert!(
        !missing.iter().any(|f| f.contains("screen-buffer sampling")),
        "screen-buffer sampling is implemented on the canvas path and must not \
         be advertised as unsupported, got {missing:?}"
    );

    // And the hint-carrying uniform is discoverable by name.
    let names = gdserver2d::shader::screen_texture_uniforms(src);
    assert_eq!(
        names,
        vec!["screen_tex".to_string()],
        "the uniform declared with `: hint_screen_texture` must be listed, or \
         the renderer cannot tell it apart from an ordinary sampler2D"
    );
}
