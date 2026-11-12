//! Pure shading-math helpers for `StandardMaterial3D` channels (pat-u3l8c.42).
//!
//! Self-contained `Vector2`/`Vector3` functions for three of the advanced
//! material channels: **triplanar** projection blend weights, **height/parallax**
//! UV offset, and **distance fade** alpha. These are the CPU-side reference
//! implementations (and unit-testable oracle) for the corresponding shader
//! built-ins; the shader/material struct wiring lives elsewhere.
//!
//! # Scope / remaining work (pat-u3l8c.42)
//!
//! This delivers only the triplanar / parallax / distance-fade math. The rest
//! of the pack — AO, clearcoat, anisotropy, refraction, rim, backlight, detail
//! blending, proximity fade against the depth buffer, billboard material modes,
//! and threading UV2 through the render path — is not implemented here.

use gdcore::math::{Vector2, Vector3};

const EPS: f32 = 1e-6;

/// Triplanar blend weights for a surface `normal`.
///
/// Returns per-axis weights (x/y/z) that sum to `1.0`, biased toward the axis
/// the normal points along; higher `sharpness` concentrates the blend on the
/// dominant axis. Matches Godot's `pow(abs(NORMAL), vec3(sharpness))`
/// normalized.
pub fn triplanar_weights(normal: Vector3, sharpness: f32) -> Vector3 {
    let s = sharpness.max(0.0);
    let wx = normal.x.abs().powf(s);
    let wy = normal.y.abs().powf(s);
    let wz = normal.z.abs().powf(s);
    let sum = wx + wy + wz;
    if sum > EPS {
        Vector3::new(wx / sum, wy / sum, wz / sum)
    } else {
        // Degenerate normal: split evenly.
        Vector3::new(1.0 / 3.0, 1.0 / 3.0, 1.0 / 3.0)
    }
}

/// Height-map parallax UV offset (parallax mapping).
///
/// `view_tangent` is the view direction in tangent space (x/y along the UV
/// axes, z along the surface normal). Returns the offset UV, shifted along the
/// view direction proportionally to `height * scale`. A head-on view
/// (`view_tangent.z == 1`, x/y == 0) leaves the UV unchanged. Matches Godot's
/// `UV -= (VIEW_TANGENT.xy / VIEW_TANGENT.z) * (height * scale)`.
pub fn parallax_offset_uv(uv: Vector2, view_tangent: Vector3, height: f32, scale: f32) -> Vector2 {
    let z = if view_tangent.z.abs() < EPS {
        EPS
    } else {
        view_tangent.z
    };
    let shift = Vector2::new(view_tangent.x, view_tangent.y) * (height * scale / z);
    uv - shift
}

/// Distance-fade alpha for a fragment at camera `distance`.
///
/// Returns `1.0` when nearer than `fade_min`, `0.0` when farther than
/// `fade_max`, and a linear ramp between (an object that fades out with
/// distance). If the range is degenerate it hard-cuts at `fade_max`.
pub fn distance_fade(distance: f32, fade_min: f32, fade_max: f32) -> f32 {
    if (fade_max - fade_min).abs() < EPS {
        return if distance >= fade_max { 0.0 } else { 1.0 };
    }
    1.0 - ((distance - fade_min) / (fade_max - fade_min)).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-5
    }

    #[test]
    fn triplanar_axis_aligned_normal_selects_one_axis() {
        let w = triplanar_weights(Vector3::new(0.0, 1.0, 0.0), 4.0);
        assert!(
            approx(w.x, 0.0) && approx(w.y, 1.0) && approx(w.z, 0.0),
            "{w:?}"
        );
    }

    #[test]
    fn triplanar_weights_sum_to_one() {
        for n in [
            Vector3::new(0.3, 0.6, -0.7),
            Vector3::new(-1.0, 0.2, 0.1),
            Vector3::new(0.5, 0.5, 0.5),
        ] {
            for sharp in [0.5, 1.0, 4.0, 8.0] {
                let w = triplanar_weights(n, sharp);
                assert!(approx(w.x + w.y + w.z, 1.0), "sum {w:?} @ {sharp}");
                assert!(w.x >= 0.0 && w.y >= 0.0 && w.z >= 0.0);
            }
        }
    }

    #[test]
    fn triplanar_equal_normal_equal_weights() {
        let w = triplanar_weights(Vector3::new(1.0, 1.0, 1.0), 1.0);
        assert!(approx(w.x, 1.0 / 3.0) && approx(w.y, 1.0 / 3.0) && approx(w.z, 1.0 / 3.0));
    }

    #[test]
    fn triplanar_higher_sharpness_concentrates_dominant_axis() {
        let n = Vector3::new(0.6, 0.8, 0.0);
        let low = triplanar_weights(n, 1.0);
        let high = triplanar_weights(n, 8.0);
        // Y dominates; more sharpness → larger Y weight.
        assert!(high.y > low.y, "high.y {} low.y {}", high.y, low.y);
    }

    #[test]
    fn triplanar_degenerate_normal_splits_evenly() {
        let w = triplanar_weights(Vector3::new(0.0, 0.0, 0.0), 4.0);
        assert!(approx(w.x, 1.0 / 3.0) && approx(w.y, 1.0 / 3.0) && approx(w.z, 1.0 / 3.0));
    }

    #[test]
    fn parallax_head_on_leaves_uv_unchanged() {
        let uv = Vector2::new(0.5, 0.5);
        let out = parallax_offset_uv(uv, Vector3::new(0.0, 0.0, 1.0), 0.2, 0.05);
        assert!(approx(out.x, 0.5) && approx(out.y, 0.5), "{out:?}");
    }

    #[test]
    fn parallax_tilted_view_shifts_uv() {
        // View tilted along +X: UV shifts by -(x/z)*(h*scale) in X.
        let uv = Vector2::new(0.5, 0.5);
        let out = parallax_offset_uv(uv, Vector3::new(0.5, 0.0, 1.0), 0.4, 0.1);
        // shift.x = 0.5/1.0 * (0.4*0.1) = 0.02 -> uv.x - 0.02
        assert!(approx(out.x, 0.48), "x {}", out.x);
        assert!(approx(out.y, 0.5), "y {}", out.y);
    }

    #[test]
    fn parallax_scales_with_height() {
        let uv = Vector2::new(0.0, 0.0);
        let a = parallax_offset_uv(uv, Vector3::new(1.0, 0.0, 1.0), 0.1, 1.0);
        let b = parallax_offset_uv(uv, Vector3::new(1.0, 0.0, 1.0), 0.2, 1.0);
        // Double height → double shift magnitude.
        assert!(approx(b.x, 2.0 * a.x), "a {} b {}", a.x, b.x);
    }

    #[test]
    fn distance_fade_ramps_from_one_to_zero() {
        assert!(approx(distance_fade(5.0, 10.0, 20.0), 1.0), "near");
        assert!(approx(distance_fade(15.0, 10.0, 20.0), 0.5), "mid");
        assert!(approx(distance_fade(25.0, 10.0, 20.0), 0.0), "far");
        assert!(approx(distance_fade(10.0, 10.0, 20.0), 1.0), "at min");
        assert!(approx(distance_fade(20.0, 10.0, 20.0), 0.0), "at max");
    }

    #[test]
    fn distance_fade_degenerate_range_hard_cuts() {
        assert!(approx(distance_fade(5.0, 10.0, 10.0), 1.0));
        assert!(approx(distance_fade(10.0, 10.0, 10.0), 0.0));
        assert!(approx(distance_fade(15.0, 10.0, 10.0), 0.0));
    }
}
