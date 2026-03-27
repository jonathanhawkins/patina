//! Framebuffer comparison utilities for 3D golden-image testing.
//!
//! Provides pixel-level and depth-buffer diffing between two
//! [`FrameBuffer3D`]s, reporting matching pixel counts, per-pixel color
//! distance, depth agreement, and summary statistics.

use crate::renderer::FrameBuffer3D;

/// Result of comparing two 3D framebuffers pixel-by-pixel.
#[derive(Debug, Clone)]
pub struct DiffResult3D {
    /// Number of color pixels that match within the tolerance.
    pub matching_pixels: u64,
    /// Total number of pixels compared.
    pub total_pixels: u64,
    /// Maximum Euclidean color distance observed across all pixels.
    pub max_color_diff: f64,
    /// Average Euclidean color distance across all pixels.
    pub avg_color_diff: f64,
    /// Number of depth values that match within the depth tolerance.
    pub matching_depth: u64,
    /// Maximum absolute depth difference observed.
    pub max_depth_diff: f64,
    /// Average absolute depth difference.
    pub avg_depth_diff: f64,
}

impl DiffResult3D {
    /// Returns the fraction of matching color pixels (0.0 to 1.0).
    pub fn color_match_ratio(&self) -> f64 {
        if self.total_pixels == 0 {
            return 1.0;
        }
        self.matching_pixels as f64 / self.total_pixels as f64
    }

    /// Returns the fraction of matching depth values (0.0 to 1.0).
    pub fn depth_match_ratio(&self) -> f64 {
        if self.total_pixels == 0 {
            return 1.0;
        }
        self.matching_depth as f64 / self.total_pixels as f64
    }

    /// Returns `true` if all color pixels match within the tolerance.
    pub fn is_exact_color_match(&self) -> bool {
        self.matching_pixels == self.total_pixels
    }

    /// Returns `true` if all depth values match within the tolerance.
    pub fn is_exact_depth_match(&self) -> bool {
        self.matching_depth == self.total_pixels
    }
}

/// Compares two 3D framebuffers pixel-by-pixel using Euclidean color distance
/// and absolute depth difference.
///
/// Two color pixels are considered "matching" if their Euclidean RGB distance
/// is less than or equal to `color_tolerance`. Two depth values are considered
/// "matching" if their absolute difference is less than or equal to `depth_tolerance`.
///
/// # Panics
///
/// Panics if the framebuffers have different dimensions.
pub fn compare_framebuffers_3d(
    a: &FrameBuffer3D,
    b: &FrameBuffer3D,
    color_tolerance: f64,
    depth_tolerance: f64,
) -> DiffResult3D {
    assert_eq!(
        (a.width, a.height),
        (b.width, b.height),
        "framebuffer dimensions must match: ({}, {}) vs ({}, {})",
        a.width, a.height, b.width, b.height,
    );

    let total = (a.width as u64) * (a.height as u64);
    if total == 0 {
        return DiffResult3D {
            matching_pixels: 0,
            total_pixels: 0,
            max_color_diff: 0.0,
            avg_color_diff: 0.0,
            matching_depth: 0,
            max_depth_diff: 0.0,
            avg_depth_diff: 0.0,
        };
    }

    let mut color_matching = 0u64;
    let mut max_color = 0.0f64;
    let mut sum_color = 0.0f64;

    for (pa, pb) in a.pixels.iter().zip(b.pixels.iter()) {
        let dr = (pa.r - pb.r) as f64;
        let dg = (pa.g - pb.g) as f64;
        let db = (pa.b - pb.b) as f64;
        let dist = (dr * dr + dg * dg + db * db).sqrt();

        if dist <= color_tolerance {
            color_matching += 1;
        }
        if dist > max_color {
            max_color = dist;
        }
        sum_color += dist;
    }

    let mut depth_matching = 0u64;
    let mut max_depth = 0.0f64;
    let mut sum_depth = 0.0f64;

    for (da, db) in a.depth.iter().zip(b.depth.iter()) {
        let diff = (*da - *db).abs() as f64;
        if diff <= depth_tolerance {
            depth_matching += 1;
        }
        if diff > max_depth {
            max_depth = diff;
        }
        sum_depth += diff;
    }

    DiffResult3D {
        matching_pixels: color_matching,
        total_pixels: total,
        max_color_diff: max_color,
        avg_color_diff: sum_color / total as f64,
        matching_depth: depth_matching,
        max_depth_diff: max_depth,
        avg_depth_diff: sum_depth / total as f64,
    }
}

/// Generates a visual diff framebuffer highlighting pixel differences.
///
/// Matching pixels are rendered in grayscale (from framebuffer `a`).
/// Differing pixels are highlighted in red, with intensity proportional
/// to the color distance.
///
/// # Panics
///
/// Panics if the framebuffers have different dimensions.
pub fn diff_image_3d(a: &FrameBuffer3D, b: &FrameBuffer3D) -> FrameBuffer3D {
    use gdcore::math::Color;

    assert_eq!(
        (a.width, a.height),
        (b.width, b.height),
        "framebuffer dimensions must match for diff_image_3d",
    );

    let mut out = FrameBuffer3D::new(a.width, a.height, Color::BLACK);

    for (i, (pa, pb)) in a.pixels.iter().zip(b.pixels.iter()).enumerate() {
        let dr = (pa.r - pb.r) as f64;
        let dg = (pa.g - pb.g) as f64;
        let db = (pa.b - pb.b) as f64;
        let dist = (dr * dr + dg * dg + db * db).sqrt();

        let color = if dist < 0.001 {
            let luma = 0.299 * pa.r + 0.587 * pa.g + 0.114 * pa.b;
            Color::new(luma, luma, luma, 1.0)
        } else {
            let intensity = (dist / 1.732).min(1.0) as f32;
            Color::new(1.0, 0.0, 0.0, intensity.max(0.3))
        };

        out.pixels[i] = color;
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use gdcore::math::Color;

    #[test]
    fn identical_framebuffers_exact_match() {
        let a = FrameBuffer3D::new(4, 4, Color::rgb(0.5, 0.3, 0.7));
        let b = a.clone();
        let result = compare_framebuffers_3d(&a, &b, 0.0, 0.0);
        assert!(result.is_exact_color_match());
        assert!(result.is_exact_depth_match());
        assert_eq!(result.total_pixels, 16);
    }

    #[test]
    fn completely_different_framebuffers() {
        let a = FrameBuffer3D::new(2, 2, Color::BLACK);
        let b = FrameBuffer3D::new(2, 2, Color::WHITE);
        let result = compare_framebuffers_3d(&a, &b, 0.0, 0.0);
        assert_eq!(result.matching_pixels, 0);
        assert!((result.max_color_diff - 3.0_f64.sqrt()).abs() < 0.001);
    }

    #[test]
    fn color_tolerance_allows_near_matches() {
        let a = FrameBuffer3D::new(1, 1, Color::rgb(0.5, 0.5, 0.5));
        let b = FrameBuffer3D::new(1, 1, Color::rgb(0.51, 0.49, 0.5));
        let strict = compare_framebuffers_3d(&a, &b, 0.0, 1.0);
        assert_eq!(strict.matching_pixels, 0);
        let lenient = compare_framebuffers_3d(&a, &b, 0.02, 1.0);
        assert_eq!(lenient.matching_pixels, 1);
    }

    #[test]
    fn depth_comparison_works() {
        let mut a = FrameBuffer3D::new(2, 2, Color::BLACK);
        let b = FrameBuffer3D::new(2, 2, Color::BLACK);
        a.depth[0] = 0.5;
        // b.depth[0] is 1.0 (default)
        let result = compare_framebuffers_3d(&a, &b, 1.0, 0.1);
        // 3 match (depth diff 0.0), 1 doesn't (diff 0.5)
        assert_eq!(result.matching_depth, 3);
        assert!((result.max_depth_diff - 0.5).abs() < 0.001);
    }

    #[test]
    fn color_match_ratio_calculation() {
        let mut a = FrameBuffer3D::new(2, 2, Color::BLACK);
        let b = FrameBuffer3D::new(2, 2, Color::BLACK);
        a.pixels[0] = Color::WHITE;
        let result = compare_framebuffers_3d(&a, &b, 0.0, 1.0);
        assert!((result.color_match_ratio() - 0.75).abs() < 0.001);
    }

    #[test]
    fn zero_size_framebuffers() {
        let a = FrameBuffer3D::new(0, 0, Color::BLACK);
        let b = FrameBuffer3D::new(0, 0, Color::BLACK);
        let result = compare_framebuffers_3d(&a, &b, 0.0, 0.0);
        assert_eq!(result.total_pixels, 0);
        assert_eq!(result.color_match_ratio(), 1.0);
        assert_eq!(result.depth_match_ratio(), 1.0);
    }

    #[test]
    #[should_panic(expected = "framebuffer dimensions must match")]
    fn mismatched_dimensions_panics() {
        let a = FrameBuffer3D::new(4, 4, Color::BLACK);
        let b = FrameBuffer3D::new(8, 8, Color::BLACK);
        compare_framebuffers_3d(&a, &b, 0.0, 0.0);
    }

    #[test]
    fn diff_image_identical_is_grayscale() {
        let a = FrameBuffer3D::new(2, 2, Color::rgb(1.0, 0.0, 0.0));
        let b = a.clone();
        let diff = diff_image_3d(&a, &b);
        let p = diff.pixels[0];
        assert!((p.r - 0.299).abs() < 0.01);
        assert!((p.g - 0.299).abs() < 0.01);
        assert!((p.b - 0.299).abs() < 0.01);
    }

    #[test]
    fn diff_image_different_shows_red() {
        let a = FrameBuffer3D::new(1, 1, Color::BLACK);
        let b = FrameBuffer3D::new(1, 1, Color::WHITE);
        let diff = diff_image_3d(&a, &b);
        let p = diff.pixels[0];
        assert!(p.r > 0.0);
        assert!(p.g < 0.01);
        assert!(p.b < 0.01);
    }
}
