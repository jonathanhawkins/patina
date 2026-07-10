//! Camera post-processing: glow (bloom), tone mapping, and the final
//! linear→sRGB encode.
//!
//! This is a CPU port of Godot 4.6's `tonemap.glsl` (plus the glow generation
//! in `copy.glsl`). It takes a **linear scene-referred HDR** pixel buffer and
//! collapses it to a display-referred sRGB `[0, 1]` image, exactly as Godot's
//! tonemap pass does:
//!
//! ```text
//! color *= exposure
//! if glow (non-softlight):  color = apply_glow(color, gather_glow * intensity)
//! color = tonemap(color)                      // filmic / reinhard / aces
//! if glow (softlight):      color = apply_glow(color, tonemap(gather_glow * intensity))
//! color = linear_to_srgb(color)
//! ```
//!
//! The software and GPU editor render paths both call [`apply_camera_post`] on
//! their linear HDR buffer, so the two backends produce byte-identical display
//! images (the GPU path reads its float target back and runs this same code).
//!
//! Glow is a faithful port of Godot's algorithm — HDR threshold with bleed
//! ramp + luminance cap, a mip pyramid of separable 5-tap gaussian blurs, and
//! a per-level weighted gather — rather than a pixel-exact replica of Godot's
//! bilinear-tap downsample, so bright regions bloom the same way without
//! requiring the GPU's exact sampling.

use gdcore::math::{linear_to_srgb_channel, Color};
use gdserver3d::environment::{Environment3D, GlowBlendMode, ToneMapper};

/// Applies the camera post-process to a **linear HDR** pixel buffer in place,
/// converting it to display sRGB in `[0, 1]`. Alpha is preserved.
///
/// Mirrors Godot 4.6's `tonemap.glsl` `main()`. When the environment's tone
/// mapper is [`ToneMapper::Linear`] and glow is disabled, this is just the
/// final `linear_to_srgb` encode (with exposure), so a scene authored in sRGB
/// and converted to linear on input round-trips unchanged.
pub fn apply_camera_post(pixels: &mut [Color], width: u32, height: u32, env: &Environment3D) {
    let exposure = env.tonemap_exposure;
    let white = env.tonemap_white.max(1e-4);
    let intensity = env.glow_intensity;

    // Generate the glow buffer once (linear HDR, full resolution) if enabled.
    // Early-out when nothing can bleed into glow: with `glow_bloom == 0`, only
    // pixels whose exposed luminance exceeds the HDR threshold contribute, so a
    // scene with no HDR highlights (e.g. the default preview sun at exactly 1.0)
    // produces an all-black glow — skip the whole pyramid in that case.
    let glow = if env.glow_enabled && glow_has_active_level(env) && scene_can_bloom(pixels, env, exposure)
    {
        Some(generate_glow(pixels, width, height, env, exposure))
    } else {
        None
    };
    let pre_tonemap_glow = env.glow_blend_mode != GlowBlendMode::Softlight;

    for (i, px) in pixels.iter_mut().enumerate() {
        let mut c = [px.r * exposure, px.g * exposure, px.b * exposure];

        // Pre-tonemap glow (every blend mode except soft light).
        if let (Some(g), true) = (glow.as_ref(), pre_tonemap_glow) {
            let gi = [g[i][0] * intensity, g[i][1] * intensity, g[i][2] * intensity];
            c = blend_glow(c, gi, env.glow_blend_mode, white, intensity);
        }

        // Tone map linear HDR → linear LDR.
        c = tonemap(c, env.tone_mapper, white);

        // Post-tonemap glow (soft light only: it must run after tonemapping to
        // avoid a discontinuity at 1.0).
        if let (Some(g), false) = (glow.as_ref(), pre_tonemap_glow) {
            let mut gi = [g[i][0] * intensity, g[i][1] * intensity, g[i][2] * intensity];
            gi = tonemap(gi, env.tone_mapper, white);
            c = blend_glow(c, gi, GlowBlendMode::Softlight, white, intensity);
        }

        // Linear → sRGB for display.
        px.r = linear_to_srgb_channel(c[0]).clamp(0.0, 1.0);
        px.g = linear_to_srgb_channel(c[1]).clamp(0.0, 1.0);
        px.b = linear_to_srgb_channel(c[2]).clamp(0.0, 1.0);
    }
}

// ── Tone mapping (ported from tonemap.glsl) ─────────────────────────────────

/// Tone maps a linear HDR color. `Linear` is the identity (used by the
/// low-level default environment); the others match Godot's operators with the
/// white reference folded in so `white` maps to display 1.0.
pub fn tonemap(c: [f32; 3], mode: ToneMapper, white: f32) -> [f32; 3] {
    if mode == ToneMapper::Linear {
        return c;
    }
    // Godot clamps negatives before the tonemap curve (negative lights).
    let c = [c[0].max(0.0), c[1].max(0.0), c[2].max(0.0)];
    match mode {
        ToneMapper::Linear => c,
        ToneMapper::Reinhard => [
            tonemap_reinhard(c[0], white),
            tonemap_reinhard(c[1], white),
            tonemap_reinhard(c[2], white),
        ],
        ToneMapper::Filmic => [
            tonemap_filmic(c[0], white),
            tonemap_filmic(c[1], white),
            tonemap_filmic(c[2], white),
        ],
        ToneMapper::Aces => tonemap_aces(c, white),
    }
}

/// Extended Reinhard (Godot's `tonemap_reinhard`).
fn tonemap_reinhard(color: f32, white: f32) -> f32 {
    let white_squared = white * white;
    let white_squared_color = white_squared * color;
    (white_squared_color + color * color) / (white_squared_color + white_squared)
}

/// Uncharted-style filmic curve (Godot's `tonemap_filmic`), with the
/// `exposure_bias = 2.0` constants baked in and divided by the tonemapped
/// white point so `white` → 1.0.
fn tonemap_filmic(color: f32, white: f32) -> f32 {
    filmic_curve(color) / filmic_curve(white)
}

#[inline]
fn filmic_curve(x: f32) -> f32 {
    // exposure_bias = 2.0 baked into the constants (matches the shader / the
    // C++ white-point calc in environment_storage.cpp).
    const A: f32 = 0.22 * 4.0; // 0.88
    const B: f32 = 0.30 * 2.0; // 0.60
    const C: f32 = 0.10;
    const D: f32 = 0.20;
    const E: f32 = 0.01;
    const F: f32 = 0.30;
    ((x * (A * x + C * B) + D * E) / (x * (A * x + B) + D * F)) - E / F
}

/// ACES filmic (Godot's `tonemap_aces`), RRT+ODT fit with `exposure_bias = 1.8`.
fn tonemap_aces(color: [f32; 3], white: f32) -> [f32; 3] {
    const EB: f32 = 1.8;
    const A: f32 = 0.0245786;
    const B: f32 = 0.000090537;
    const C: f32 = 0.983729;
    const D: f32 = 0.432951;
    const E: f32 = 0.238081;
    // GLSL evaluates `color * mat3(col0, col1, col2)` (row-vector × column-major
    // matrix) as `out[j] = dot(color, col_j)`. Bias baked into `rgb_to_rrt`.
    let rrt = |c: [f32; 3]| {
        [
            EB * (0.59719 * c[0] + 0.35458 * c[1] + 0.04823 * c[2]),
            EB * (0.07600 * c[0] + 0.90834 * c[1] + 0.01566 * c[2]),
            EB * (0.02840 * c[0] + 0.13383 * c[1] + 0.83777 * c[2]),
        ]
    };
    let odt = |c: [f32; 3]| {
        [
            1.60475 * c[0] - 0.53108 * c[1] - 0.07367 * c[2],
            -0.10208 * c[0] + 1.10813 * c[1] - 0.00605 * c[2],
            -0.00327 * c[0] - 0.07276 * c[1] + 1.07602 * c[2],
        ]
    };
    let fit = |c: [f32; 3]| {
        let mapped = |v: f32| (v * (v + A) - B) / (v * (C * v + D) + E);
        [mapped(c[0]), mapped(c[1]), mapped(c[2])]
    };
    let white_tonemapped = {
        let w = white * EB;
        (w * (w + A) - B) / (w * (C * w + D) + E)
    };
    let out = odt(fit(rrt(color)));
    [
        out[0] / white_tonemapped,
        out[1] / white_tonemapped,
        out[2] / white_tonemapped,
    ]
}

// ── Glow blending (ported from tonemap.glsl `apply_glow`) ────────────────────

/// Composites `glow` onto `color` per the selected blend mode. `Mix` lerps by
/// the glow intensity (Godot handles it inline in `main()`, not in
/// `apply_glow`).
fn blend_glow(
    color: [f32; 3],
    glow: [f32; 3],
    mode: GlowBlendMode,
    white: f32,
    intensity: f32,
) -> [f32; 3] {
    match mode {
        GlowBlendMode::Additive => [
            color[0] + glow[0],
            color[1] + glow[1],
            color[2] + glow[2],
        ],
        GlowBlendMode::Screen => {
            // Glow clamped to [0, white], then screen-blended.
            let g = [
                glow[0].clamp(0.0, white),
                glow[1].clamp(0.0, white),
                glow[2].clamp(0.0, white),
            ];
            [
                color[0] + g[0] - (color[0] * g[0] / white),
                color[1] + g[1] - (color[1] * g[1] / white),
                color[2] + g[2] - (color[2] * g[2] / white),
            ]
        }
        GlowBlendMode::Softlight => {
            let g = [
                glow[0].clamp(0.0, 1.0),
                glow[1].clamp(0.0, 1.0),
                glow[2].clamp(0.0, 1.0),
            ];
            [
                softlight_channel(color[0], g[0]),
                softlight_channel(color[1], g[1]),
                softlight_channel(color[2], g[2]),
            ]
        }
        GlowBlendMode::Replace => glow,
        GlowBlendMode::Mix => [
            color[0] * (1.0 - intensity) + glow[0],
            color[1] * (1.0 - intensity) + glow[1],
            color[2] * (1.0 - intensity) + glow[2],
        ],
    }
}

#[inline]
fn softlight_channel(c: f32, g: f32) -> f32 {
    if c > 1.0 {
        c
    } else {
        let soft = if c <= 0.25 {
            ((16.0 * c - 12.0) * c + 4.0) * c
        } else {
            c.sqrt()
        };
        c + g * (soft - c)
    }
}

// ── Glow generation (ported from copy.glsl MODE_GLOW) ────────────────────────

/// Godot's separable 5-tap glow blur kernel (center + 4 symmetric taps).
const GLOW_KERNEL: [f32; 5] = [0.2024, 0.1790, 0.1240, 0.0672, 0.0285];

/// Returns whether any glow level has a non-negligible weight.
fn glow_has_active_level(env: &Environment3D) -> bool {
    env.glow_levels.iter().any(|&w| w > 0.0001)
}

/// Cheap pre-check: could any pixel contribute to glow? `glow_bloom > 0` leaks
/// the whole image, so it always can; otherwise a pixel contributes only if its
/// exposed luminance exceeds the bleed threshold. Lets the common no-HDR scene
/// skip the glow pyramid entirely.
fn scene_can_bloom(pixels: &[Color], env: &Environment3D, exposure: f32) -> bool {
    if env.glow_bloom > 0.0001 {
        return true;
    }
    let threshold = env.glow_hdr_bleed_threshold;
    pixels
        .iter()
        .any(|c| (c.r.max(c.g).max(c.b)) * exposure > threshold)
}

/// A single glow mip level: a downsampled+blurred linear HDR image.
struct GlowLevel {
    px: Vec<[f32; 3]>,
    w: u32,
    h: u32,
}

/// Builds the glow mip pyramid and gathers it back to a full-resolution glow
/// buffer, one `[r, g, b]` per scene pixel (pre-intensity, pre-blend). Mirrors
/// Godot: level 0 is a half-res thresholded blur of the scene, each subsequent
/// level a half-res blur of the one above, and the gather is a per-level
/// weighted bilinear sample.
fn generate_glow(
    scene: &[Color],
    w: u32,
    h: u32,
    env: &Environment3D,
    exposure: f32,
) -> Vec<[f32; 3]> {
    // Highest active level index determines how deep the pyramid must go.
    let max_level = env
        .glow_levels
        .iter()
        .enumerate()
        .rev()
        .find(|(_, &wt)| wt > 0.0001)
        .map(|(i, _)| i)
        .unwrap_or(0);

    let mut levels: Vec<GlowLevel> = Vec::with_capacity(max_level + 1);

    // Level 0: threshold + blur, downsampled from the scene.
    let scene_rgb: Vec<[f32; 3]> = scene.iter().map(|c| [c.r, c.g, c.b]).collect();
    let (mut cur, mut cw, mut ch) = downsample_box(&scene_rgb, w, h);
    apply_glow_threshold(&mut cur, env, exposure);
    cur = gaussian_blur(&cur, cw, ch);
    levels.push(GlowLevel { px: cur.clone(), w: cw, h: ch });

    // Levels 1..=max_level: plain downsample + blur of the previous level.
    for _ in 1..=max_level {
        let (mut next, nw, nh) = downsample_box(&levels.last().unwrap().px, cw, ch);
        for p in next.iter_mut() {
            p[0] *= env.glow_strength;
            p[1] *= env.glow_strength;
            p[2] *= env.glow_strength;
        }
        next = gaussian_blur(&next, nw, nh);
        cw = nw;
        ch = nh;
        levels.push(GlowLevel { px: next, w: cw, h: ch });
    }

    // Gather: weighted bilinear sample of each active level at the pixel's uv.
    let mut out = vec![[0.0f32; 3]; (w * h) as usize];
    for y in 0..h {
        let v = (y as f32 + 0.5) / h as f32;
        for x in 0..w {
            let u = (x as f32 + 0.5) / w as f32;
            let mut acc = [0.0f32; 3];
            for (i, level) in levels.iter().enumerate() {
                let weight = env.glow_levels[i];
                if weight <= 0.0001 {
                    continue;
                }
                let s = sample_bilinear(&level.px, level.w, level.h, u, v);
                acc[0] += s[0] * weight;
                acc[1] += s[1] * weight;
                acc[2] += s[2] * weight;
            }
            out[(y * w + x) as usize] = acc;
        }
    }
    out
}

/// Halves resolution by averaging each 2×2 source block. Dimensions floor at 1.
fn downsample_box(src: &[[f32; 3]], w: u32, h: u32) -> (Vec<[f32; 3]>, u32, u32) {
    let dw = (w / 2).max(1);
    let dh = (h / 2).max(1);
    let mut out = vec![[0.0f32; 3]; (dw * dh) as usize];
    for y in 0..dh {
        for x in 0..dw {
            let sx0 = (x * 2).min(w - 1);
            let sy0 = (y * 2).min(h - 1);
            let sx1 = (x * 2 + 1).min(w - 1);
            let sy1 = (y * 2 + 1).min(h - 1);
            let mut acc = [0.0f32; 3];
            for &(sx, sy) in &[(sx0, sy0), (sx1, sy0), (sx0, sy1), (sx1, sy1)] {
                let p = src[(sy * w + sx) as usize];
                acc[0] += p[0];
                acc[1] += p[1];
                acc[2] += p[2];
            }
            out[(y * dw + x) as usize] = [acc[0] * 0.25, acc[1] * 0.25, acc[2] * 0.25];
        }
    }
    (out, dw, dh)
}

/// The glow HDR threshold + bleed + luminance cap (Godot's FIRST_PASS block).
/// Pixels below the threshold contribute nothing; the smoothstep ramps in over
/// `bleed_scale`, `glow_bloom` leaks the whole image, and the luminance cap
/// clamps fireflies.
fn apply_glow_threshold(px: &mut [[f32; 3]], env: &Environment3D, exposure: f32) {
    let threshold = env.glow_hdr_bleed_threshold;
    let scale = env.glow_hdr_bleed_scale;
    let cap = env.glow_hdr_luminance_cap;
    let bloom = env.glow_bloom;
    for p in px.iter_mut() {
        let c = [p[0] * exposure, p[1] * exposure, p[2] * exposure];
        let luminance = c[0].max(c[1]).max(c[2]);
        let feedback = smoothstep(threshold, threshold + scale, luminance).max(bloom);
        p[0] = (c[0] * feedback).min(cap);
        p[1] = (c[1] * feedback).min(cap);
        p[2] = (c[2] * feedback).min(cap);
    }
}

/// Separable gaussian blur using Godot's 5-tap glow kernel (radius 4, symmetric).
fn gaussian_blur(src: &[[f32; 3]], w: u32, h: u32) -> Vec<[f32; 3]> {
    let mut tmp = vec![[0.0f32; 3]; src.len()];
    // Horizontal.
    for y in 0..h {
        for x in 0..w {
            let mut acc = [0.0f32; 3];
            for k in -4i32..=4 {
                let sx = (x as i32 + k).clamp(0, w as i32 - 1) as u32;
                let weight = GLOW_KERNEL[k.unsigned_abs() as usize];
                let p = src[(y * w + sx) as usize];
                acc[0] += p[0] * weight;
                acc[1] += p[1] * weight;
                acc[2] += p[2] * weight;
            }
            tmp[(y * w + x) as usize] = acc;
        }
    }
    // Vertical.
    let mut out = vec![[0.0f32; 3]; src.len()];
    for y in 0..h {
        for x in 0..w {
            let mut acc = [0.0f32; 3];
            for k in -4i32..=4 {
                let sy = (y as i32 + k).clamp(0, h as i32 - 1) as u32;
                let weight = GLOW_KERNEL[k.unsigned_abs() as usize];
                let p = tmp[(sy * w + x) as usize];
                acc[0] += p[0] * weight;
                acc[1] += p[1] * weight;
                acc[2] += p[2] * weight;
            }
            out[(y * w + x) as usize] = acc;
        }
    }
    out
}

/// Bilinear sample of a level buffer at normalized `uv` (pixel-center convention).
fn sample_bilinear(px: &[[f32; 3]], w: u32, h: u32, u: f32, v: f32) -> [f32; 3] {
    let fx = (u * w as f32 - 0.5).clamp(0.0, (w - 1) as f32);
    let fy = (v * h as f32 - 0.5).clamp(0.0, (h - 1) as f32);
    let x0 = fx.floor() as u32;
    let y0 = fy.floor() as u32;
    let x1 = (x0 + 1).min(w - 1);
    let y1 = (y0 + 1).min(h - 1);
    let tx = fx - x0 as f32;
    let ty = fy - y0 as f32;
    let p00 = px[(y0 * w + x0) as usize];
    let p10 = px[(y0 * w + x1) as usize];
    let p01 = px[(y1 * w + x0) as usize];
    let p11 = px[(y1 * w + x1) as usize];
    let mut out = [0.0f32; 3];
    for c in 0..3 {
        let top = p00[c] + (p10[c] - p00[c]) * tx;
        let bot = p01[c] + (p11[c] - p01[c]) * tx;
        out[c] = top + (bot - top) * ty;
    }
    out
}

/// GLSL `smoothstep`.
#[inline]
fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    if edge0 == edge1 {
        return if x < edge0 { 0.0 } else { 1.0 };
    }
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[cfg(test)]
mod tests {
    use super::*;
    use gdserver3d::environment::Environment3D;

    /// Godot's filmic curve with white=1.0 maps mid values on a known rolloff:
    /// filmic(1.0) = 1.0 (white), and it's monotonic and < linear for x<1.
    #[test]
    fn filmic_matches_godot_reference() {
        // filmic_curve(1.0) = 0.578355 (the white divisor); filmic(1.0) = 1.0.
        assert!((filmic_curve(1.0) - 0.578_355).abs() < 1e-4);
        assert!((tonemap_filmic(1.0, 1.0) - 1.0).abs() < 1e-4);
        // filmic(0) = 0.
        assert!(tonemap_filmic(0.0, 1.0).abs() < 1e-5);
        // Monotonic increasing.
        assert!(tonemap_filmic(0.2, 1.0) < tonemap_filmic(0.5, 1.0));
        assert!(tonemap_filmic(0.5, 1.0) < tonemap_filmic(0.9, 1.0));
        // With white=1.0 the normalized curve lifts midtones: 0.5 → ~0.694.
        assert!((tonemap_filmic(0.5, 1.0) - 0.6936).abs() < 2e-3, "got {}", tonemap_filmic(0.5, 1.0));
        // The highlight rolloff: a bright HDR input exceeds white before the
        // final clamp (it will be clamped to 1.0 on sRGB encode).
        assert!(tonemap_filmic(4.0, 1.0) > 1.0);
    }

    #[test]
    fn reinhard_and_aces_map_white_to_one() {
        assert!((tonemap_reinhard(1.0, 1.0) - 1.0).abs() < 1e-4);
        // ACES white maps to ~1.0 by construction.
        let w = tonemap_aces([1.0, 1.0, 1.0], 1.0);
        assert!((w[0] - 1.0).abs() < 2e-2, "aces white {:?}", w);
    }

    #[test]
    fn linear_tonemapper_is_identity() {
        let c = [0.3, 1.5, 4.0];
        assert_eq!(tonemap(c, ToneMapper::Linear, 1.0), c);
    }

    #[test]
    fn screen_glow_never_exceeds_white() {
        // Screen blend with full glow can't push a mid value past white.
        let out = blend_glow([0.5, 0.5, 0.5], [10.0, 10.0, 10.0], GlowBlendMode::Screen, 1.0, 0.3);
        for ch in out {
            assert!(ch <= 1.0 + 1e-6, "screen blend exceeded white: {ch}");
            assert!(ch >= 0.5, "screen blend darkened the base: {ch}");
        }
    }

    #[test]
    fn additive_glow_adds() {
        let out = blend_glow([0.2, 0.2, 0.2], [0.3, 0.0, 0.0], GlowBlendMode::Additive, 1.0, 0.3);
        assert!((out[0] - 0.5).abs() < 1e-6);
        assert!((out[1] - 0.2).abs() < 1e-6);
    }

    /// With the default environment (Linear tonemap, glow off), post-processing
    /// is just linear→sRGB — so an sRGB color converted to linear on input and
    /// back out here round-trips to itself.
    #[test]
    fn default_env_is_srgb_encode_only() {
        use gdcore::math::Color;
        let env = Environment3D::default();
        // A linear value; after linear→srgb it becomes the sRGB encoding.
        let mut px = vec![Color::new(0.2140, 0.2140, 0.2140, 1.0)];
        apply_camera_post(&mut px, 1, 1, &env);
        // linear 0.214 → sRGB ~0.5.
        assert!((px[0].r - 0.5).abs() < 2e-3, "got {}", px[0].r);
        assert_eq!(px[0].a, 1.0);
    }

    /// A bright HDR sun (linear value well above the glow threshold) blooms into
    /// its neighbours; a dim scene at exactly the threshold does not.
    #[test]
    fn glow_blooms_hdr_but_not_threshold() {
        use gdcore::math::Color;
        let mut env = Environment3D::default();
        env.glow_enabled = true;
        env.tone_mapper = ToneMapper::Linear;

        // 16×16 buffer, single very bright center pixel (HDR 20 > cap 12 → capped).
        let (w, h) = (16u32, 16u32);
        let mut px = vec![Color::new(0.0, 0.0, 0.0, 1.0); (w * h) as usize];
        let center = (8 * w + 8) as usize;
        px[center] = Color::new(20.0, 20.0, 20.0, 1.0);
        apply_camera_post(&mut px, w, h, &env);
        // A neighbour a few pixels away should now be non-black (bloomed).
        let neighbour = (8 * w + 11) as usize;
        assert!(
            px[neighbour].r > 0.02,
            "HDR sun must bloom into neighbours, got {}",
            px[neighbour].r
        );

        // Now a scene whose brightest pixel is exactly at the threshold (1.0):
        // smoothstep(1,3,1)=0 → no bloom (matches Godot).
        let mut px2 = vec![Color::new(0.0, 0.0, 0.0, 1.0); (w * h) as usize];
        px2[center] = Color::new(1.0, 1.0, 1.0, 1.0);
        apply_camera_post(&mut px2, w, h, &env);
        assert!(
            px2[neighbour].r < 1e-4,
            "a pixel at the glow threshold must not bloom, got {}",
            px2[neighbour].r
        );
    }

    #[test]
    fn disabled_glow_leaves_neighbours_black() {
        use gdcore::math::Color;
        let mut env = Environment3D::default();
        env.glow_enabled = false;
        let (w, h) = (8u32, 8u32);
        let mut px = vec![Color::new(0.0, 0.0, 0.0, 1.0); (w * h) as usize];
        px[(4 * w + 4) as usize] = Color::new(50.0, 50.0, 50.0, 1.0);
        apply_camera_post(&mut px, w, h, &env);
        assert!(px[(4 * w + 7) as usize].r < 1e-6);
    }

    #[test]
    fn smoothstep_endpoints() {
        assert_eq!(smoothstep(1.0, 3.0, 0.5), 0.0);
        assert_eq!(smoothstep(1.0, 3.0, 4.0), 1.0);
        assert!((smoothstep(1.0, 3.0, 2.0) - 0.5).abs() < 1e-6);
    }
}
