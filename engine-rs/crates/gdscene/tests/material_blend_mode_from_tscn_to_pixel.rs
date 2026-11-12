//! pat-s3rrv.30 — `BaseMaterial3D.blend_mode` reaches the composited pixel.
//!
//! The bead's headline is "wire transparency, blend, cull and depth-draw modes
//! from .tscn into both render pipelines". Three of the four had landed;
//! **blend was still entirely absent** — `grep blend_mode` across
//! `gdserver3d/src`, `gdrender3d/src` and `render_server_3d.rs` matched only
//! `Environment.glow_blend_mode`, which is a different property on a different
//! class. `StandardMaterial3D` had no field, `standard_material_from_props`
//! read four keys and not this one, and the rasterizer hardcoded source-over.
//!
//! So every additive and multiply material — which is how essentially every
//! glow, flame and shadow-decal effect is authored — composited as a plain
//! alpha blend, and looked merely *wrong* rather than broken.
//!
//! ⚠️ The equations are transcribed from the GL blend state Godot sets in
//! `drivers/gles3/rasterizer_scene_gles3.cpp:3255-3285`, not inferred from the
//! mode names. The names are actively misleading — see the `Sub` and `Mul`
//! tests below.

use gdcore::math::Color;
use gdrender3d::render_modes::blend_pixel;
use gdscene::render_server_3d::standard_material_from_props;
use gdserver3d::material::BlendMode;
use gdvariant::Variant;
use std::collections::HashMap;

fn props(pairs: &[(&str, Variant)]) -> HashMap<String, Variant> {
    pairs
        .iter()
        .map(|(k, v)| ((*k).to_string(), v.clone()))
        .collect()
}

fn approx(a: Color, b: Color) -> bool {
    (a.r - b.r).abs() < 1e-5
        && (a.g - b.g).abs() < 1e-5
        && (a.b - b.b).abs() < 1e-5
        && (a.a - b.a).abs() < 1e-5
}

/// **The acceptance test**: a `.tscn` `blend_mode` reaches the pixel.
#[test]
fn material_blend_mode_from_tscn_to_pixel() {
    // Godot's BaseMaterial3D.BlendMode ints (material.h:228-232).
    for (value, expected) in [
        (0, BlendMode::Mix),
        (1, BlendMode::Add),
        (2, BlendMode::Sub),
        (3, BlendMode::Mul),
        (4, BlendMode::PremultAlpha),
    ] {
        let mat = standard_material_from_props(&props(&[("blend_mode", Variant::Int(value))]));
        assert_eq!(
            mat.blend_mode, expected,
            "`blend_mode = {value}` in a .tscn must parse to {expected:?}",
        );

        // ⚠️ The conversion to `Material3D` is where this would silently die:
        // `to_material3d` ends in `..Material3D::default()`, so a field that is
        // not mapped EXPLICITLY resets to Mix — and both the parser and the
        // renderer would still look correct in isolation.
        assert_eq!(
            mat.to_material3d().blend_mode,
            expected,
            "`to_material3d` must carry {expected:?} through; its \
             `..Material3D::default()` tail silently resets anything unmapped",
        );
    }

    // A material that says nothing keeps Godot's default.
    assert_eq!(
        standard_material_from_props(&props(&[])).blend_mode,
        BlendMode::Mix,
        "an unspecified blend_mode is Mix",
    );
}

/// `Mix` reproduces the source-over the rasterizer used to hardcode
/// (pat-s3rrv.30).
///
/// This is the regression guard for the wiring: every pre-existing
/// transparency assertion in the 3D suite goes through this equation, so if it
/// changed, those tests would be measuring something new.
#[test]
fn mix_is_exactly_the_source_over_it_replaced() {
    let src = Color::new(1.0, 0.0, 0.0, 0.5);
    let dst = Color::new(0.0, 0.0, 1.0, 1.0);
    assert!(
        approx(
            blend_pixel(BlendMode::Mix, src, dst),
            Color::new(0.5, 0.0, 0.5, 1.0)
        ),
        "half-alpha red over opaque blue meets in the middle: {:?}",
        blend_pixel(BlendMode::Mix, src, dst),
    );
}

/// `Add` scales the source by its own alpha (pat-s3rrv.30).
///
/// Godot sets `glBlendFunc(GL_SRC_ALPHA, GL_ONE)` in the transparent pass, so a
/// fully transparent additive fragment contributes **nothing** — not its full
/// colour. Treating `Add` as `src + dst` makes invisible geometry glow.
#[test]
fn add_scales_the_source_by_alpha() {
    let dst = Color::new(0.25, 0.25, 0.25, 1.0);

    let half = blend_pixel(BlendMode::Add, Color::new(1.0, 0.0, 0.0, 0.5), dst);
    assert!(
        approx(half, Color::new(0.75, 0.25, 0.25, 1.0)),
        "0.5-alpha red added to 0.25 grey = 0.5*1 + 0.25: got {half:?}",
    );

    let invisible = blend_pixel(BlendMode::Add, Color::new(1.0, 1.0, 1.0, 0.0), dst);
    assert!(
        approx(invisible, dst),
        "a zero-alpha additive fragment must leave the destination untouched, \
         got {invisible:?} — plain `src + dst` would blow this out to white",
    );
}

/// `Sub` REVERSE-subtracts (pat-s3rrv.30).
///
/// ⚠️ Godot selects `GL_FUNC_REVERSE_SUBTRACT`, i.e. `dst - src`. Reading the
/// name as "subtract the destination" inverts the result on every pixel, and
/// against a mid-grey background the two are hard to tell apart by eye.
#[test]
fn sub_subtracts_the_source_from_the_destination() {
    let got = blend_pixel(
        BlendMode::Sub,
        Color::new(0.5, 0.0, 0.0, 1.0),
        Color::new(0.75, 0.75, 0.75, 1.0),
    );
    assert!(
        approx(got, Color::new(0.25, 0.75, 0.75, 0.0)),
        "dst - src*a = 0.75 - 0.5 on red only: got {got:?}. (0.0, …) would mean \
         src - dst, the wrong direction. ⚠️ The ALPHA is 0.0, not 1.0: Godot \
         uses plain `glBlendFunc`, so the same reverse-subtract applies to the \
         alpha channel (1 - 1*1), and assuming the destination keeps its alpha \
         — as `Mul` does via glBlendFuncSeparate — is the easy mistake here",
    );

    // Clamped at zero rather than wrapping.
    let floored = blend_pixel(
        BlendMode::Sub,
        Color::new(1.0, 1.0, 1.0, 1.0),
        Color::new(0.25, 0.25, 0.25, 1.0),
    );
    assert!(
        approx(floored, Color::new(0.0, 0.0, 0.0, 0.0)),
        "{floored:?}"
    );
}

/// `Mul` modulates by the destination and leaves its alpha alone
/// (pat-s3rrv.30).
///
/// `glBlendFuncSeparate(GL_DST_COLOR, GL_ZERO, GL_ZERO, GL_ONE)`: the colour is
/// `src * dst` — a modulate, not a mix — and the destination keeps its own
/// alpha.
#[test]
fn mul_modulates_by_the_destination() {
    let got = blend_pixel(
        BlendMode::Mul,
        Color::new(0.5, 1.0, 0.0, 0.25),
        Color::new(0.8, 0.5, 1.0, 1.0),
    );
    assert!(
        approx(got, Color::new(0.4, 0.5, 0.0, 1.0)),
        "src * dst per channel, destination alpha preserved: got {got:?}. Note \
         the source alpha 0.25 does NOT scale the result — Mul ignores it",
    );
}

/// `PremultAlpha` adds the source unscaled (pat-s3rrv.30).
///
/// `glBlendFunc(GL_ONE, GL_ONE_MINUS_SRC_ALPHA)`: the source is assumed to
/// carry premultiplied alpha already, so scaling it again would darken every
/// such material.
#[test]
fn premult_alpha_adds_the_source_unscaled() {
    let got = blend_pixel(
        BlendMode::PremultAlpha,
        Color::new(0.4, 0.0, 0.0, 0.5),
        Color::new(0.0, 0.0, 0.8, 1.0),
    );
    assert!(
        approx(got, Color::new(0.4, 0.0, 0.4, 1.0)),
        "src + dst*(1-a): got {got:?}. If the source were scaled by its alpha \
         the red would be 0.2, which is the signature of treating this as Mix",
    );
}
