//! pat-s3rrv.203 — normal-mapped and specular 2D canvas shading.
//!
//! Ported from `upstream/godot/drivers/gles3/shaders/canvas.glsl`:
//! `light_normal_compute` (:420), the normal decode (:646), and the positional
//! light vector (:810).
//!
//! 🔑 **A light with no normal map is indistinguishable from a light with a
//! flat one.** Every normal is `(0, 0, 1)`, `N·L` collapses to a constant over
//! the surface, and `height` — the whole point of the feature — multiplies out.
//! So the assertions here are differential across the *surface*: the same light
//! and the same pixel, shaded through two different normals, must disagree.
//! Anything that asserts a single pixel is "lit" passes on the flat fallback.

use gdcore::math::{Color, Vector2, Vector3};
use gdrender2d::light2d::{
    decode_normal_map, light_normal_compute, light_vector_to, Light2D, SpecularShininess,
};

const WHITE: Color = Color {
    r: 1.0,
    g: 1.0,
    b: 1.0,
    a: 1.0,
};

/// A normal-map texel encoding a surface facing left (`-X`).
///
/// `r = 0` decodes to `x = -1`; `g = 0.5` decodes to `y = 0` — see
/// [`the_green_channel_is_inverted`] for why `0.5` is the one green value that
/// cannot detect a sign error.
fn facing_left() -> Color {
    Color::new(0.0, 0.5, 1.0, 1.0)
}

fn facing_right() -> Color {
    Color::new(1.0, 0.5, 1.0, 1.0)
}

fn flat() -> Color {
    Color::new(0.5, 0.5, 1.0, 1.0)
}

/// The bead's rig: a point light 100px to the LEFT of the pixel, 32px off the
/// plane.
fn left_light() -> Light2D {
    let mut light = Light2D::point(Vector2::new(-100.0, 0.0), WHITE, 1.0, 0.0);
    light.height = 32.0;
    light
}

fn luma(c: Color) -> f32 {
    (c.r + c.g + c.b) / 3.0
}

/// Shades one pixel at the origin through `texel`'s normal.
fn shade(texel: Color) -> Color {
    let light = left_light();
    let l = light_vector_to(&light, Vector2::new(0.0, 0.0));
    light_normal_compute(l, decode_normal_map(texel), WHITE, WHITE, None)
}

/// A flat normal map decodes to the geometric normal, so shading through it is
/// the unlit-surface case.
#[test]
fn a_flat_texel_decodes_to_the_geometric_normal() {
    let n = decode_normal_map(flat());
    assert!(
        n.x.abs() < 1e-6 && n.y.abs() < 1e-6 && (n.z - 1.0).abs() < 1e-6,
        "0.5/0.5 must decode to (0, 0, 1); got {n:?}"
    );
}

/// ⚠️ **The green channel is inverted.** Godot decodes
/// `normal.xy = texel.xy * vec2(2, -2) - vec2(1, -1)`, so `y = 1 - 2g`, because
/// canvas Y points down while normal maps are authored Y-up.
///
/// 🔑 This test exists because **the bead's own acceptance cannot catch this
/// bug.** That acceptance varies the surface only along X (left-facing vs
/// right-facing pixels), and every X-varying texel holds `g = 0.5`, which
/// decodes to `y = 0` under either sign. A whole feature can ship with its
/// vertical axis mirrored and the acceptance stays green.
#[test]
fn the_green_channel_is_inverted() {
    let up = decode_normal_map(Color::new(0.5, 1.0, 1.0, 1.0));
    assert!(
        (up.y + 1.0).abs() < 1e-6,
        "g=1.0 must decode to y=-1 (Godot's `y = 1 - 2g`); y=+1 means the \
         canvas Y-flip was dropped and every normal map is mirrored \
         vertically. got {up:?}"
    );
}

/// **The bead's acceptance, at the shading layer.** A surface facing the light
/// is brighter than one facing away from it.
#[test]
fn left_facing_pixels_are_brighter_than_right_facing_ones() {
    let left = luma(shade(facing_left()));
    let right = luma(shade(facing_right()));

    assert!(
        left > right + 0.5,
        "a left-facing pixel under a light to the LEFT must be markedly \
         brighter than a right-facing one; got left={left} right={right}"
    );
    assert!(
        right.abs() < 1e-6,
        "a pixel facing directly away from the light receives nothing: N·L is \
         negative and clamps to zero. got {right}"
    );
}

/// The control the assertion above needs: the same light over a FLAT surface is
/// uniform, so the contrast really came from the normal map.
///
/// Without this, `left > right` could just as well be measuring a horizontal
/// distance falloff.
#[test]
fn a_flat_surface_under_the_same_light_is_uniform() {
    let a = luma(shade(flat()));
    let b = luma(shade(flat()));
    assert!(
        (a - b).abs() < 1e-6,
        "a flat normal map has no per-pixel variation to produce; got {a} vs {b}"
    );
    assert!(
        a > 0.0,
        "the flat case must still be LIT — if it is zero, the contrast above is \
         'lit vs unlit' rather than 'normal-mapped relief' and proves nothing. \
         got {a}"
    );
}

/// 🔑 **`height` is what makes the light three-dimensional.** At Godot's default
/// height of 0 the light lies *in* the canvas plane, so its vector is purely
/// horizontal and `N·L` against a flat normal is exactly zero.
///
/// This is the differential that proves `height` is read at all: the same rig
/// with height 32 lights the flat surface, and with height 0 it cannot.
#[test]
fn a_zero_height_light_grazes_a_flat_surface() {
    let mut light = left_light();
    light.height = 0.0;
    let l = light_vector_to(&light, Vector2::new(0.0, 0.0));
    let lit = light_normal_compute(l, decode_normal_map(flat()), WHITE, WHITE, None);

    assert!(
        luma(lit).abs() < 1e-6,
        "height 0 puts the light in the plane, so a flat surface catches none \
         of it; a non-zero value here means `height` never reached the light \
         vector. got {}",
        luma(lit)
    );
    assert!(
        luma(shade(flat())) > 0.0,
        "…while the SAME rig at height 32 must light it, or this test is \
         passing because nothing is lit at all"
    );
}

/// The light vector is a unit vector, and at height == distance it sits at 45°
/// to the plane — the calibration Godot's own `height` documentation gives
/// ("if the height is 100, then it will illuminate an object 100 pixels away at
/// a 45° angle to the plane").
#[test]
fn the_light_vector_matches_godots_documented_45_degree_case() {
    let mut light = Light2D::point(Vector2::new(-100.0, 0.0), WHITE, 1.0, 0.0);
    light.height = 100.0;
    let l = light_vector_to(&light, Vector2::new(0.0, 0.0));

    assert!(
        (l.length() - 1.0).abs() < 1e-5,
        "the light vector must be normalised; got {l:?}"
    );
    let expected = 1.0 / 2.0_f32.sqrt();
    assert!(
        (l.z - expected).abs() < 1e-5 && (l.x + expected).abs() < 1e-5,
        "height 100 at distance 100 is a 45° elevation, so z and -x are both \
         1/sqrt(2); got {l:?}"
    );
}

/// Specular adds energy on top of the diffuse term rather than replacing it, so
/// a pixel aimed at the light is never dimmer for having a specular map.
#[test]
fn specular_adds_to_the_diffuse_term() {
    let light = left_light();
    let l = light_vector_to(&light, Vector2::new(0.0, 0.0));
    let normal = decode_normal_map(facing_left());

    let diffuse_only = light_normal_compute(l, normal, WHITE, WHITE, None);
    let with_spec =
        light_normal_compute(l, normal, WHITE, WHITE, Some(SpecularShininess::default()));

    assert!(
        luma(with_spec) >= luma(diffuse_only) - 1e-6,
        "the Blinn term is additive: `spec * light * s + light * base * N·L`. \
         got diffuse={} with_spec={}",
        luma(diffuse_only),
        luma(with_spec)
    );
}

/// A normal pointing straight at the viewer under a light directly overhead is
/// fully lit — the sanity anchor for the decode/compose pair.
#[test]
fn a_normal_aimed_at_the_light_is_fully_lit() {
    let straight_up = Vector3::new(0.0, 0.0, 1.0);
    let lit = light_normal_compute(straight_up, straight_up, WHITE, WHITE, None);
    assert!(
        (luma(lit) - 1.0).abs() < 1e-6,
        "N·L is 1 when the normal and the light vector coincide; got {}",
        luma(lit)
    );
}
