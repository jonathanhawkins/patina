//! Canvas-item (2D) fragment shader execution in the software draw path
//! (pat-u3l8c.45).
//!
//! Before this bead the CPU `canvas_item` fragment interpreter in
//! `gdserver2d::shader` (`parse_fragment_body` / `execute_fragment`) existed but
//! was never invoked by any render path — a `ShaderMaterial` attached to a
//! sprite had zero visible effect. These tests drive a `ShaderMaterial` through
//! the real `SoftwareRenderer` render path and assert the framebuffer pixels
//! change, exercising the `COLOR`, `UV`, and `TEXTURE` (sampled-texel-as-COLOR)
//! builtins plus material uniform-parameter overrides.

use gdcore::math::{Color, Rect2, Vector2};
use gdrender2d::renderer::SoftwareRenderer;
use gdrender2d::test_adapter::{assert_pixel_color, capture_frame};
use gdrender2d::texture::Texture2D;
use gdserver2d::canvas::{CanvasItem, CanvasItemId, DrawCommand};
use gdserver2d::shader::{Shader, ShaderMaterial, ShaderType};
use gdserver2d::viewport::Viewport;
use gdvariant::variant::Variant;

/// Builds a `canvas_item` shader material from GDShader `fragment()` source.
fn material(source: &str) -> ShaderMaterial {
    let mut m = ShaderMaterial::new();
    m.shader = Some(Shader::new(ShaderType::CanvasItem, source));
    m
}

/// Renders a single full-viewport textured sprite, optionally with a material,
/// into a `size × size` black framebuffer and returns it.
fn render_sprite(
    size: u32,
    texture: Texture2D,
    modulate: Color,
    mat: Option<ShaderMaterial>,
) -> gdrender2d::renderer::FrameBuffer {
    let mut renderer = SoftwareRenderer::new();
    renderer.register_texture("tex", texture);
    let mut vp = Viewport::new(size, size, Color::BLACK);

    let mut item = CanvasItem::new(CanvasItemId(1));
    item.commands.push(DrawCommand::DrawTextureRect {
        texture_path: "tex".to_string(),
        rect: Rect2::new(Vector2::new(0.0, 0.0), Vector2::new(size as f32, size as f32)),
        modulate,
    });
    item.material = mat;
    vp.add_canvas_item(item);

    capture_frame(&mut renderer, &vp)
}

/// Baseline: a textured sprite with no material renders the texel unchanged.
/// This is the control the shaded cases are measured against.
#[test]
fn sprite_without_material_renders_texel() {
    let fb = render_sprite(4, Texture2D::solid(4, 4, Color::WHITE), Color::WHITE, None);
    assert_pixel_color(&fb, 2, 2, Color::WHITE, 0.02);
}

/// `COLOR.rgb *= 0.5` — the incoming `COLOR` (white texel) is halved, so the
/// sprite renders mid-gray. Exercises the `COLOR` builtin + `MultiplyColorRgb`.
#[test]
fn shader_tint_halves_color() {
    let src = "shader_type canvas_item;\nvoid fragment() {\n    COLOR.rgb *= 0.5;\n}\n";
    let fb = render_sprite(
        4,
        Texture2D::solid(4, 4, Color::WHITE),
        Color::WHITE,
        Some(material(src)),
    );
    assert_pixel_color(&fb, 2, 2, Color::new(0.5, 0.5, 0.5, 1.0), 0.02);
}

/// `COLOR.a = UV.x` — output alpha ramps with the horizontal UV coordinate.
/// Composited over black, the sprite brightness increases left→right, proving
/// the `UV` builtin is fed real per-fragment coordinates across the rect.
#[test]
fn shader_uv_builtin_drives_horizontal_gradient() {
    let src = "shader_type canvas_item;\nvoid fragment() {\n    COLOR.a = UV.x;\n}\n";
    let size = 8;
    let fb = render_sprite(
        size,
        Texture2D::solid(size, size, Color::WHITE),
        Color::WHITE,
        Some(material(src)),
    );

    let left = fb.get_pixel(0, 4);
    let right = fb.get_pixel(size - 1, 4);
    // UV.x at the left column ≈ 0 → alpha ≈ 0 → stays near black.
    assert!(
        left.r < 0.2,
        "left column should be dark, got r={}",
        left.r
    );
    // UV.x at the right column is high → brighter than the left.
    assert!(
        right.r > left.r + 0.5,
        "right column ({}) should be much brighter than left ({})",
        right.r,
        left.r
    );
}

/// `COLOR = tint;` with a `uniform vec4 tint` overridden via the material's
/// runtime parameters. Proves shader-parameter overrides reach the interpreter.
#[test]
fn shader_uniform_parameter_override_recolors_sprite() {
    let src =
        "shader_type canvas_item;\nuniform vec4 tint;\nvoid fragment() {\n    COLOR = tint;\n}\n";
    let mut mat = material(src);
    mat.set_shader_parameter("tint", Variant::Color(Color::rgb(0.0, 1.0, 0.0)));

    let fb = render_sprite(
        4,
        Texture2D::solid(4, 4, Color::WHITE),
        Color::WHITE,
        Some(mat),
    );
    // White texel is entirely replaced by the green uniform.
    assert_pixel_color(&fb, 2, 2, Color::rgb(0.0, 1.0, 0.0), 0.02);
}

/// A material whose `fragment()` body has no recognized statements is a pure
/// passthrough: `COLOR` starts as `texture(TEXTURE, UV) * MODULATE`, so a red
/// texel survives unchanged. Confirms the sampled `TEXTURE` feeds `COLOR`.
#[test]
fn empty_fragment_passes_texture_through_as_color() {
    let src = "shader_type canvas_item;\nvoid fragment() {\n    // no-op\n}\n";
    let fb = render_sprite(
        4,
        Texture2D::solid(4, 4, Color::rgb(1.0, 0.0, 0.0)),
        Color::WHITE,
        Some(material(src)),
    );
    assert_pixel_color(&fb, 2, 2, Color::rgb(1.0, 0.0, 0.0), 0.02);
}
