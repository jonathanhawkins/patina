//! pat-s3rrv.206: a metal shows the probe's reflection INSTEAD of its diffuse.
//!
//! # The defect this pins
//!
//! `LambertFragmentShader` gained an image-based specular term so a `.tscn`
//! `StandardMaterial3D` (which takes the Lambert path, not the PBR one) could
//! see a captured `ReflectionProbe` at all. But that term was *added* on top of
//! a full Lambert diffuse without the energy split `pbr_shade` already makes:
//!
//! ```text
//! diffuse_* += env.* * f0[*] * sharpness;   // and diffuse_* was never scaled
//! ```
//!
//! For the mirror case the acceptance exercises — white albedo, `metallic = 1`,
//! `roughness = 0`, red capture — that yields `(W + 1, W, W)` for a lit white
//! level `W`. The pixel is red-*tinted*, never red-*dominant*, so a strict
//! red-dominance assertion sees **zero** reflected pixels on a surface that is
//! plainly reflecting. Metals have no diffuse; the reflection must replace it.
//!
//! ⚠️ These live in `tests/` rather than the module's own `#[cfg(test)]` block
//! so they can be named as a gate on their own target, and in a NEW file rather
//! than in `reflection_probe_capture_render.rs` because that file was being
//! edited by another agent at the time.

use gdcore::math::{Color, Vector3};
use gdrender3d::shader::{
    FragmentInput, FragmentShader, LambertFragmentShader, MaterialTextures, ShaderUniforms,
};
use gdserver3d::reflection_probe::{ProbeReflection, ReflectionCubemap};
use std::sync::Arc;

/// The lit white level every case starts from, via `ambient * albedo`.
///
/// Deliberately well above the `0.538` where a `(W + 1, W, W)` pixel would
/// still scrape past a `g < r * 0.35` test: at `0.8` the unfixed shader is
/// unambiguously pink, so these assertions cannot pass on the old behaviour by
/// a rounding accident.
const W: f32 = 0.8;

/// A probe whose capture is pure red in every direction.
///
/// Uniform fill, so the exact reflected direction is irrelevant and these tests
/// pin the energy split rather than the cubemap's face selection — which
/// `sample_direction` already has its own coverage for.
fn red_probe() -> ProbeReflection {
    ProbeReflection {
        cubemap: Arc::new(ReflectionCubemap::new(2, Color::new(1.0, 0.0, 0.0, 1.0))),
        box_min: Vector3::new(-10.0, -10.0, -10.0),
        box_max: Vector3::new(10.0, 10.0, 10.0),
        origin: Vector3::ZERO,
        // Raw reflection direction: no parallax correction to reason about.
        box_projection: false,
        intensity: 1.0,
        // A hard edge, so `blend_weight` is 1.0 anywhere inside the box.
        blend_distance: 0.0,
    }
}

/// Shades one fragment of a white surface under `ambient = W`, with no lights.
///
/// Neither `FragmentInput` nor `ShaderUniforms` derives `Default`, so both are
/// built in full.
fn shade(metallic: f32, roughness: f32, probe: Option<ProbeReflection>) -> Color {
    let input = FragmentInput {
        world_position: Vector3::ZERO,
        world_normal: Vector3::new(0.0, 1.0, 0.0),
        uv: [0.5, 0.5],
        depth: 0.0,
    };
    let uniforms = ShaderUniforms {
        model_matrix: [[0.0; 4]; 4],
        view_matrix: [[0.0; 4]; 4],
        projection_matrix: [[0.0; 4]; 4],
        albedo: Color::new(1.0, 1.0, 1.0, 1.0),
        emission: Color::new(0.0, 0.0, 0.0, 1.0),
        ambient: Color::new(W, W, W, 1.0),
        roughness,
        metallic,
        // Directly above the fragment, so the reflected ray is well-defined.
        camera_position: Vector3::new(0.0, 1.0, 0.0),
        probe,
        layers: 1,
        lights: Vec::new(),
        directional_shadow_maps: Vec::new(),
        omni_shadow_cubemaps: Vec::new(),
        textures: MaterialTextures::default(),
    };
    LambertFragmentShader.process(&input, &uniforms)
}

/// 🔑 **THE ACCEPTANCE.** A mirror takes the capture and loses its diffuse.
#[test]
fn a_metal_under_a_probe_shows_the_reflection_instead_of_its_diffuse() {
    let got = shade(1.0, 0.0, Some(red_probe()));

    // The green and blue channels are the whole point: the capture is pure red,
    // so anything left in g/b is Lambert diffuse that a metal should not have.
    // Before the fix these were both W = 0.8.
    assert!(
        got.g < 0.01 && got.b < 0.01,
        "a metal has no diffuse, so a pure-red capture must leave g and b at \
         zero; got {got:?}. Non-zero g/b here is the white Lambert term \
         surviving underneath the reflection, which makes the pixel pink \
         rather than red"
    );
    assert!(
        (got.r - 1.0).abs() < 0.01,
        "the red channel must be the captured red at full strength for a \
         metallic = 1.0, roughness = 0.0 mirror; got {got:?}"
    );
}

/// CONTROL: a dielectric keeps its diffuse.
///
/// Without this, the fix above would also pass if it had suppressed the diffuse
/// for *every* surface under a probe, which would black out every non-metal in
/// a probe's box.
#[test]
fn a_dielectric_under_a_probe_keeps_its_diffuse() {
    let got = shade(0.0, 0.0, Some(red_probe()));

    assert!(
        (got.g - W).abs() < 0.01 && (got.b - W).abs() < 0.01,
        "at metallic = 0.0 the energy split must be a no-op and the Lambert \
         diffuse must survive intact; got {got:?}, expected g and b near {W}"
    );
    // F0 for a dielectric is 0.04, so it takes a faint red rim, not the wall.
    assert!(
        got.r > W && got.r < W + 0.2,
        "a dielectric still takes a weak Schlick-F0 reflection on top of its \
         diffuse; got {got:?}"
    );
}

/// CONTROL: a rough metal is untouched, because the block never runs.
///
/// `roughness = 1.0` is the DEFAULT material, so this is the guarantee that
/// ordinary scenes did not move. A build that applied the energy split
/// unconditionally would black this out.
#[test]
fn a_rough_metal_under_a_probe_is_unchanged() {
    let got = shade(1.0, 1.0, Some(red_probe()));

    assert!(
        (got.r - W).abs() < 0.01 && (got.g - W).abs() < 0.01 && (got.b - W).abs() < 0.01,
        "at roughness = 1.0 sharpness is exactly 0, so neither the reflection \
         nor the energy split may apply; got {got:?}, expected {W} on every \
         channel"
    );
}

/// CONTROL: with no probe, a mirror is byte-identical to before the feature.
///
/// This is the property the `sharpness > 0.0` guard was written to protect, and
/// the energy split is scoped inside the same branch so it inherits it.
#[test]
fn a_metal_with_no_probe_is_unchanged() {
    let got = shade(1.0, 0.0, None);

    assert!(
        (got.r - W).abs() < 0.01 && (got.g - W).abs() < 0.01 && (got.b - W).abs() < 0.01,
        "with no probe the Lambert path must be exactly what it always was; \
         got {got:?}, expected {W} on every channel"
    );
}
