//! CameraAttributes (exposure, DOF) + MSAA resolve (pat-u3l8c.38).
//!
//! camera3d.rs was projection-only with no camera attributes, and there was no
//! MSAA resolve. This module provides the pure post/resolve primitives:
//! exposure scaling, box-filter MSAA resolve, and depth-of-field blur amount
//! from a [`CameraAttributes`] resource.

use gdcore::math::Color;

/// Godot `CameraAttributesPractical`-style camera post settings.
#[derive(Debug, Clone, Copy)]
pub struct CameraAttributes {
    /// Exposure multiplier applied to rendered color.
    pub exposure: f32,
    /// Whether depth-of-field blur is active.
    pub dof_enabled: bool,
    /// Depth kept perfectly in focus.
    pub focus_distance: f32,
    /// Half-width of the in-focus band around `focus_distance`.
    pub focus_range: f32,
    /// Maximum blur amount reached outside the focus band.
    pub blur_amount: f32,
}

impl Default for CameraAttributes {
    fn default() -> Self {
        Self {
            exposure: 1.0,
            dof_enabled: false,
            focus_distance: 10.0,
            focus_range: 2.0,
            blur_amount: 1.0,
        }
    }
}

/// Scales `color` by `exposure`, clamping RGB to `[0, 1]` (alpha unchanged).
pub fn apply_exposure(color: Color, exposure: f32) -> Color {
    Color::new(
        (color.r * exposure).clamp(0.0, 1.0),
        (color.g * exposure).clamp(0.0, 1.0),
        (color.b * exposure).clamp(0.0, 1.0),
        color.a,
    )
}

/// Resolves MSAA sub-samples into one color via a box (average) filter.
///
/// Returns transparent black for no samples.
pub fn resolve_msaa(samples: &[Color]) -> Color {
    if samples.is_empty() {
        return Color::new(0.0, 0.0, 0.0, 0.0);
    }
    let n = samples.len() as f32;
    let (mut r, mut g, mut b, mut a) = (0.0f32, 0.0f32, 0.0f32, 0.0f32);
    for s in samples {
        r += s.r;
        g += s.g;
        b += s.b;
        a += s.a;
    }
    Color::new(r / n, g / n, b / n, a / n)
}

/// Depth-of-field blur amount in `[0, blur_amount]` for a fragment at `depth`.
///
/// Fragments within `focus_range` of `focus_distance` are sharp (0); beyond
/// that the blur ramps linearly over one more `focus_range` to the maximum.
pub fn dof_blur(attrs: &CameraAttributes, depth: f32) -> f32 {
    if !attrs.dof_enabled {
        return 0.0;
    }
    let dist = (depth - attrs.focus_distance).abs();
    if dist <= attrs.focus_range {
        return 0.0;
    }
    let over = dist - attrs.focus_range;
    let t = (over / attrs.focus_range.max(1e-6)).min(1.0);
    t * attrs.blur_amount
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-4
    }

    #[test]
    fn exposure_scales_and_clamps() {
        let up = apply_exposure(Color::new(0.5, 0.5, 0.5, 1.0), 2.0);
        assert!(approx(up.r, 1.0) && approx(up.a, 1.0)); // clamped, alpha kept
        let down = apply_exposure(Color::new(0.5, 0.5, 0.5, 1.0), 0.5);
        assert!(approx(down.r, 0.25));
    }

    #[test]
    fn msaa_resolves_to_average() {
        let black = Color::new(0.0, 0.0, 0.0, 1.0);
        let white = Color::new(1.0, 1.0, 1.0, 1.0);
        let out = resolve_msaa(&[black, white, black, white]);
        assert!(approx(out.r, 0.5) && approx(out.a, 1.0));
    }

    #[test]
    fn msaa_empty_is_transparent() {
        assert!(approx(resolve_msaa(&[]).a, 0.0));
    }

    #[test]
    fn dof_disabled_is_sharp() {
        let attrs = CameraAttributes::default(); // dof_enabled = false
        assert!(approx(dof_blur(&attrs, 100.0), 0.0));
    }

    #[test]
    fn dof_in_focus_band_is_sharp() {
        let attrs = CameraAttributes {
            dof_enabled: true,
            focus_distance: 10.0,
            focus_range: 2.0,
            blur_amount: 1.0,
            ..Default::default()
        };
        assert!(approx(dof_blur(&attrs, 10.0), 0.0)); // exact focus
        assert!(approx(dof_blur(&attrs, 11.5), 0.0)); // within band
    }

    #[test]
    fn dof_ramps_outside_focus_band() {
        let attrs = CameraAttributes {
            dof_enabled: true,
            focus_distance: 10.0,
            focus_range: 2.0,
            blur_amount: 1.0,
            ..Default::default()
        };
        // depth 13: dist 3, over 1, t = 1/2 = 0.5.
        assert!(approx(dof_blur(&attrs, 13.0), 0.5));
        // depth 14: dist 4, over 2, t clamps to 1 -> full blur.
        assert!(approx(dof_blur(&attrs, 14.0), 1.0));
        assert!(approx(dof_blur(&attrs, 100.0), 1.0)); // far still clamped
    }
}
