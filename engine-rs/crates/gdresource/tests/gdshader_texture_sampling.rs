//! pat-s3rrv.36 (slice 1): the AST evaluator can sample textures.
//!
//! `shader_ast.rs` is a complete GLSL-ES-3-subset parser and evaluator that the
//! engine never calls — the renderers use line-matchers that silently skip any
//! statement they cannot classify. The blocker for wiring the evaluator in was
//! that it had no `texture()` at all: its own module doc deferred sampling to
//! "a caller-supplied sampler" that did not exist yet. Every real 2D shader
//! samples, so nothing could be routed through it.
//!
//! This pins the sampler hook against the repo's own fixture,
//! `fixtures/shaders/outline.gdshader`, which the audit found renders as an
//! unshaded sprite because it yields ZERO instructions through the line-matcher.

use std::collections::HashMap;

use gdresource::shader_ast::{eval_function, parse_shader, Sampler, ShaderValue, StageContext};

/// The shipped fixture, read from disk rather than duplicated inline — a copy
/// could drift from the file the renderers actually load.
fn outline_source() -> String {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/shaders/outline.gdshader"
    );
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("fixture must exist at {path}: {e}"))
}

/// A 1×1-ish stub texture: opaque red in the left half, transparent in the
/// right. Sampling outside [0,1] clamps, as a CLAMP-mode sampler2D does.
fn half_opaque_sampler() -> Sampler {
    Sampler::new(|name, uv| {
        assert_eq!(name, "TEXTURE", "the fixture samples TEXTURE");
        if uv[0] <= 0.5 {
            [1.0, 0.0, 0.0, 1.0]
        } else {
            [0.0, 0.0, 0.0, 0.0]
        }
    })
}

fn ctx_at(uv: [f32; 2]) -> StageContext {
    let mut uniforms = HashMap::new();
    uniforms.insert(
        "outline_color".to_string(),
        ShaderValue::Vec4([0.0, 1.0, 0.0, 1.0]),
    );
    uniforms.insert("outline_width".to_string(), ShaderValue::Float(1.0));

    let mut builtins = HashMap::new();
    builtins.insert("UV".to_string(), ShaderValue::Vec2(uv));
    // One "pixel" is a quarter of the sprite, so a neighbour tap actually
    // crosses the opaque/transparent edge — with a tiny value every tap would
    // land back on the same texel and the outline could never appear.
    builtins.insert(
        "TEXTURE_PIXEL_SIZE".to_string(),
        ShaderValue::Vec2([0.25, 0.25]),
    );
    builtins.insert("COLOR".to_string(), ShaderValue::Vec4([0.0, 0.0, 0.0, 0.0]));

    StageContext::new(uniforms, builtins).with_sampler(half_opaque_sampler())
}

fn run(uv: [f32; 2]) -> [f32; 4] {
    let program = parse_shader(&outline_source()).expect("the shipped fixture must parse");
    let mut ctx = ctx_at(uv);
    eval_function(&program, "fragment", &mut ctx).expect("fragment() must evaluate");
    match ctx.builtins.get("COLOR") {
        Some(ShaderValue::Vec4(c)) => *c,
        other => panic!("fragment() must write COLOR as a vec4, got {other:?}"),
    }
}

/// The fixture parses at all — it uses local variables, `max()`, `mix()`,
/// swizzles and four `texture()` taps, none of which the line-matcher handles.
#[test]
fn outline_fixture_parses_and_evaluates() {
    let program = parse_shader(&outline_source()).expect("fixture parses");
    assert_eq!(program.shader_type.as_deref(), Some("canvas_item"));
    assert!(
        program.function("fragment").is_some(),
        "the fixture declares fragment()"
    );
    assert!(
        program.uniforms.iter().any(|u| u == "outline_color"),
        "uniforms must be collected, got {:?}",
        program.uniforms
    );
}

/// Inside the opaque area the sprite's own colour wins: `mix(outline, col, col.a)`
/// with `col.a == 1` is `col`, and alpha stays 1.
#[test]
fn opaque_interior_keeps_the_sprite_colour() {
    let c = run([0.25, 0.5]);
    assert!(
        c[0] > 0.9 && c[1] < 0.1,
        "interior must stay the texture's red, got {c:?}"
    );
    assert!(
        (c[3] - 1.0).abs() < 1e-6,
        "interior stays opaque, got {c:?}"
    );
}

/// **The point of the fixture.** Just outside the opaque edge the sprite itself
/// is transparent, but a neighbour tap hits opacity — so `max()` lifts alpha and
/// the pixel becomes the outline colour. This is the ring the bead's acceptance
/// looks for, evaluated here at the language level.
#[test]
fn transparent_pixel_next_to_opacity_becomes_the_outline() {
    // u = 0.6 is transparent; the -x tap at 0.6-0.25 = 0.35 is opaque.
    let c = run([0.6, 0.5]);
    assert!(
        c[3] > 0.9,
        "a transparent pixel adjacent to opacity must be lifted to the outline \
         by max() over the neighbour taps, got alpha {} in {c:?}",
        c[3]
    );
    assert!(
        c[1] > 0.9 && c[0] < 0.1,
        "and it must take the outline uniform's green, not the sprite's red — \
         mix(outline_color, col, col.a) with col.a == 0 is the outline colour. \
         Got {c:?}"
    );
}

/// Far from any opacity every tap misses, so alpha stays 0 and no ring appears.
/// Without this the previous test would also pass if `texture()` returned a
/// constant opaque value for everything.
#[test]
fn transparent_pixel_far_from_opacity_stays_clear() {
    let c = run([1.0, 0.5]);
    assert!(
        c[3] < 1e-6,
        "no neighbour tap reaches opacity from u=1.0 with a 0.25 step, so alpha \
         must stay 0; got {c:?}"
    );
}

/// With no sampler attached a shader that samples still runs, reading
/// transparent black — the same thing an unbound texture unit yields.
#[test]
fn sampling_without_a_sampler_is_transparent_black_not_an_error() {
    let program = parse_shader(&outline_source()).expect("fixture parses");
    let mut uniforms = HashMap::new();
    uniforms.insert(
        "outline_color".to_string(),
        ShaderValue::Vec4([0.0, 1.0, 0.0, 1.0]),
    );
    uniforms.insert("outline_width".to_string(), ShaderValue::Float(1.0));
    let mut builtins = HashMap::new();
    builtins.insert("UV".to_string(), ShaderValue::Vec2([0.5, 0.5]));
    builtins.insert(
        "TEXTURE_PIXEL_SIZE".to_string(),
        ShaderValue::Vec2([0.25, 0.25]),
    );
    builtins.insert("COLOR".to_string(), ShaderValue::Vec4([0.0, 0.0, 0.0, 0.0]));

    // No `.with_sampler(..)`.
    let mut ctx = StageContext::new(uniforms, builtins);
    eval_function(&program, "fragment", &mut ctx)
        .expect("a shader that samples must still run without a sampler attached");
    match ctx.builtins.get("COLOR") {
        Some(ShaderValue::Vec4(c)) => assert!(
            c[3] < 1e-6,
            "every tap reads transparent black, so alpha stays 0; got {c:?}"
        ),
        other => panic!("expected a vec4 COLOR, got {other:?}"),
    }
}
