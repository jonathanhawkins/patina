//! pat-s3rrv.36 (slice 2): the 2D render path evaluates the real GDShader
//! language, producing pixels the line matcher could never produce.
//!
//! Before this, `gdserver2d::shader::parse_fragment_body` was a five-shape line
//! matcher that silently skipped any statement it could not classify. The repo's
//! own `fixtures/shaders/outline.gdshader` — local variables, `max()`, four
//! `texture()` taps, `TEXTURE_PIXEL_SIZE` — yielded ZERO instructions, so
//! `draw_texture_rect_shaded` was an exact passthrough and the sprite rendered
//! unshaded with no diagnostic.
//!
//! This drives the actual renderer entry point, not the evaluator in isolation:
//! `parse_fragment_body` → `draw_texture_rect_shaded` → framebuffer pixels.

use std::collections::HashMap;

use gdcore::math::{Color, Rect2, Vector2};
use gdrender2d::draw::draw_texture_rect_shaded;
use gdrender2d::{FrameBuffer, Texture2D};
use gdserver2d::shader::parse_fragment_body;
use gdvariant::variant::Variant;

const W: u32 = 16;
const H: u32 = 16;

fn outline_source() -> String {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/shaders/outline.gdshader"
    );
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("fixture must exist at {path}: {e}"))
}

/// An 8×8 texture: an opaque red 4×4 block in the middle, transparent around it.
/// The outline shader should ring that block.
fn blob_texture() -> Texture2D {
    let mut pixels = vec![Color::new(0.0, 0.0, 0.0, 0.0); 64];
    for y in 2..6 {
        for x in 2..6 {
            pixels[y * 8 + x] = Color::new(1.0, 0.0, 0.0, 1.0);
        }
    }
    Texture2D {
        width: 8,
        height: 8,
        pixels,
    }
}

fn uniforms() -> HashMap<String, Variant> {
    let mut u = HashMap::new();
    // Green outline so it is unmistakable against the red sprite.
    u.insert(
        "outline_color".to_string(),
        Variant::Color(Color::new(0.0, 1.0, 0.0, 1.0)),
    );
    u.insert("outline_width".to_string(), Variant::Float(1.0));
    u
}

fn render_with(source: &str) -> FrameBuffer {
    let instructions = parse_fragment_body(source);
    let mut fb = FrameBuffer::new(W, H);
    fb.clear(Color::new(0.0, 0.0, 0.0, 1.0));
    draw_texture_rect_shaded(
        &mut fb,
        &blob_texture(),
        Rect2::new(Vector2::new(0.0, 0.0), Vector2::new(W as f32, H as f32)),
        Color::WHITE,
        &instructions,
        &uniforms(),
        0.0,
    );
    fb
}

/// Counts pixels that are predominantly green — the outline colour.
fn green_pixels(fb: &FrameBuffer) -> usize {
    let mut n = 0;
    for y in 0..H {
        for x in 0..W {
            let c = fb.get_pixel(x, y);
            if c.g > 0.5 && c.r < 0.5 {
                n += 1;
            }
        }
    }
    n
}

/// The fixture now yields instructions at all. Before this slice the line
/// matcher returned an empty vec, which is the whole bug — an empty list makes
/// `draw_texture_rect_shaded` a passthrough.
#[test]
fn outline_fixture_yields_instructions() {
    let instructions = parse_fragment_body(&outline_source());
    assert!(
        !instructions.is_empty(),
        "the shipped outline fixture must produce instructions; an empty list \
         is exactly the silent no-op this bead is about"
    );
}

/// **The wiring proof.** Rendering through the real draw entry point produces
/// the green ring.
#[test]
fn outline_shader_draws_a_ring_through_the_render_path() {
    let shaded = render_with(&outline_source());
    let ring = green_pixels(&shaded);
    assert!(
        ring > 0,
        "the outline shader must paint outline-coloured pixels around the \
         opaque blob; 0 means the fragment body is still being skipped or the \
         sampler never reached texture(TEXTURE, UV)"
    );

    // Fixture guard: an EMPTY instruction list over the same texture must NOT
    // produce green. Without this, a bug that tinted everything green would
    // satisfy the assertion above.
    let mut plain = FrameBuffer::new(W, H);
    plain.clear(Color::new(0.0, 0.0, 0.0, 1.0));
    draw_texture_rect_shaded(
        &mut plain,
        &blob_texture(),
        Rect2::new(Vector2::new(0.0, 0.0), Vector2::new(W as f32, H as f32)),
        Color::WHITE,
        &[],
        &uniforms(),
        0.0,
    );
    assert_eq!(
        green_pixels(&plain),
        0,
        "an unshaded draw of the same sprite has no outline colour in it, so \
         the green above came from the shader and not from the texture"
    );
}

/// The sprite's own opaque area keeps its red — the shader rings the blob, it
/// does not repaint it. This is what distinguishes "the shader ran" from "the
/// shader ran and produced the right thing".
#[test]
fn the_sprite_interior_is_not_repainted() {
    let shaded = render_with(&outline_source());
    let mut red = 0;
    for y in 0..H {
        for x in 0..W {
            let c = shaded.get_pixel(x, y);
            if c.r > 0.5 && c.g < 0.5 {
                red += 1;
            }
        }
    }
    assert!(
        red > 0,
        "the blob's interior must stay the sprite's red; 0 red pixels means the \
         shader overwrote the whole sprite with the outline colour"
    );
}

/// A shader the line matcher DOES understand keeps its existing path untouched.
/// The AST is a fallback for sources the matcher cannot represent, so this
/// guards against the routing change altering shaders that already worked.
#[test]
fn a_matcher_shaped_shader_still_uses_the_matcher() {
    let simple = "shader_type canvas_item;\nvoid fragment() {\n\tCOLOR.a = 0.5;\n}\n";
    let instructions = parse_fragment_body(simple);
    assert!(
        !instructions.is_empty(),
        "the matcher still handles COLOR.a = <float>"
    );
    assert!(
        !matches!(
            instructions.first(),
            Some(gdserver2d::shader::FragmentInstruction::Program(_))
        ),
        "a source the line matcher understands must NOT be rerouted through the \
         AST — that path is a fallback, so shaders that render correctly today \
         keep their exact behaviour"
    );
}
