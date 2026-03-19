//! Framebuffer comparison utilities for golden-image testing.
//!
//! Provides pixel-level diffing between two [`FrameBuffer`]s, reporting
//! matching pixel counts, per-pixel color distance, and summary statistics.

use crate::renderer::FrameBuffer;

/// Result of comparing two framebuffers pixel-by-pixel.
#[derive(Debug, Clone)]
pub struct DiffResult {
    /// Number of pixels that match within the tolerance.
    pub matching_pixels: u64,
    /// Total number of pixels compared.
    pub total_pixels: u64,
    /// Maximum Euclidean color distance observed across all pixels.
    pub max_diff: f64,
    /// Average Euclidean color distance across all pixels.
    pub avg_diff: f64,
}

impl DiffResult {
    /// Returns the fraction of matching pixels (0.0 to 1.0).
    pub fn match_ratio(&self) -> f64 {
        if self.total_pixels == 0 {
            return 1.0;
        }
        self.matching_pixels as f64 / self.total_pixels as f64
    }

    /// Returns `true` if all pixels match within the tolerance.
    pub fn is_exact_match(&self) -> bool {
        self.matching_pixels == self.total_pixels
    }
}

/// Compares two framebuffers pixel-by-pixel using Euclidean color distance.
///
/// Two pixels are considered "matching" if their Euclidean RGB distance
/// is less than or equal to `tolerance`. Color channels are in the 0.0–1.0
/// range, so the maximum possible distance is `sqrt(3) ≈ 1.732`.
///
/// # Panics
///
/// Panics if the framebuffers have different dimensions.
pub fn compare_framebuffers(a: &FrameBuffer, b: &FrameBuffer, tolerance: f64) -> DiffResult {
    assert_eq!(
        (a.width, a.height),
        (b.width, b.height),
        "framebuffer dimensions must match: ({}, {}) vs ({}, {})",
        a.width,
        a.height,
        b.width,
        b.height,
    );

    let total = (a.width as u64) * (a.height as u64);
    if total == 0 {
        return DiffResult {
            matching_pixels: 0,
            total_pixels: 0,
            max_diff: 0.0,
            avg_diff: 0.0,
        };
    }

    let mut matching = 0u64;
    let mut max_diff = 0.0f64;
    let mut sum_diff = 0.0f64;

    for (pa, pb) in a.pixels.iter().zip(b.pixels.iter()) {
        let dr = (pa.r - pb.r) as f64;
        let dg = (pa.g - pb.g) as f64;
        let db = (pa.b - pb.b) as f64;
        let dist = (dr * dr + dg * dg + db * db).sqrt();

        if dist <= tolerance {
            matching += 1;
        }
        if dist > max_diff {
            max_diff = dist;
        }
        sum_diff += dist;
    }

    DiffResult {
        matching_pixels: matching,
        total_pixels: total,
        max_diff,
        avg_diff: sum_diff / total as f64,
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
pub fn diff_image(a: &FrameBuffer, b: &FrameBuffer) -> FrameBuffer {
    use gdcore::math::Color;

    assert_eq!(
        (a.width, a.height),
        (b.width, b.height),
        "framebuffer dimensions must match for diff_image",
    );

    let mut out = FrameBuffer::new(a.width, a.height, Color::BLACK);

    for (i, (pa, pb)) in a.pixels.iter().zip(b.pixels.iter()).enumerate() {
        let dr = (pa.r - pb.r) as f64;
        let dg = (pa.g - pb.g) as f64;
        let db = (pa.b - pb.b) as f64;
        let dist = (dr * dr + dg * dg + db * db).sqrt();

        let color = if dist < 0.001 {
            // Match — render as grayscale.
            let luma = 0.299 * pa.r + 0.587 * pa.g + 0.114 * pa.b;
            Color::new(luma, luma, luma, 1.0)
        } else {
            // Diff — render in red proportional to distance.
            let intensity = (dist / 1.732).min(1.0) as f32; // normalize by max possible dist
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
        let a = FrameBuffer::new(4, 4, Color::rgb(0.5, 0.3, 0.7));
        let b = a.clone();
        let result = compare_framebuffers(&a, &b, 0.0);
        assert!(result.is_exact_match());
        assert_eq!(result.total_pixels, 16);
        assert_eq!(result.matching_pixels, 16);
        assert!(result.max_diff < f64::EPSILON);
        assert!(result.avg_diff < f64::EPSILON);
    }

    #[test]
    fn completely_different_framebuffers() {
        let a = FrameBuffer::new(2, 2, Color::BLACK);
        let b = FrameBuffer::new(2, 2, Color::WHITE);
        let result = compare_framebuffers(&a, &b, 0.0);
        assert_eq!(result.matching_pixels, 0);
        assert_eq!(result.total_pixels, 4);
        // sqrt(1^2 + 1^2 + 1^2) = sqrt(3) ≈ 1.732
        assert!((result.max_diff - 3.0_f64.sqrt()).abs() < 0.001);
    }

    #[test]
    fn tolerance_allows_near_matches() {
        let a = FrameBuffer::new(1, 1, Color::rgb(0.5, 0.5, 0.5));
        let b = FrameBuffer::new(1, 1, Color::rgb(0.51, 0.49, 0.5));
        // Distance is very small.
        let strict = compare_framebuffers(&a, &b, 0.0);
        assert_eq!(strict.matching_pixels, 0);
        let lenient = compare_framebuffers(&a, &b, 0.02);
        assert_eq!(lenient.matching_pixels, 1);
    }

    #[test]
    fn match_ratio_calculation() {
        let mut a = FrameBuffer::new(2, 2, Color::BLACK);
        let b = FrameBuffer::new(2, 2, Color::BLACK);
        // Change one pixel.
        a.set_pixel(0, 0, Color::WHITE);
        let result = compare_framebuffers(&a, &b, 0.0);
        assert!((result.match_ratio() - 0.75).abs() < 0.001);
    }

    #[test]
    fn zero_size_framebuffers() {
        let a = FrameBuffer::new(0, 0, Color::BLACK);
        let b = FrameBuffer::new(0, 0, Color::BLACK);
        let result = compare_framebuffers(&a, &b, 0.0);
        assert_eq!(result.total_pixels, 0);
        assert_eq!(result.match_ratio(), 1.0);
    }

    #[test]
    #[should_panic(expected = "framebuffer dimensions must match")]
    fn mismatched_dimensions_panics() {
        let a = FrameBuffer::new(4, 4, Color::BLACK);
        let b = FrameBuffer::new(8, 8, Color::BLACK);
        compare_framebuffers(&a, &b, 0.0);
    }

    #[test]
    fn diff_image_identical_is_grayscale() {
        let a = FrameBuffer::new(2, 2, Color::rgb(1.0, 0.0, 0.0));
        let b = a.clone();
        let diff = diff_image(&a, &b);
        // Red (1,0,0) -> luma = 0.299
        let p = diff.get_pixel(0, 0);
        assert!((p.r - 0.299).abs() < 0.01);
        assert!((p.g - 0.299).abs() < 0.01);
        assert!((p.b - 0.299).abs() < 0.01);
    }

    #[test]
    fn diff_image_different_shows_red() {
        let a = FrameBuffer::new(1, 1, Color::BLACK);
        let b = FrameBuffer::new(1, 1, Color::WHITE);
        let diff = diff_image(&a, &b);
        let p = diff.get_pixel(0, 0);
        assert!(p.r > 0.0, "diff should show red for different pixels");
        assert!(p.g < 0.01, "diff green channel should be zero");
        assert!(p.b < 0.01, "diff blue channel should be zero");
    }

    #[test]
    fn avg_diff_correct_for_single_changed_pixel() {
        let mut a = FrameBuffer::new(2, 1, Color::BLACK);
        let b = FrameBuffer::new(2, 1, Color::BLACK);
        a.set_pixel(0, 0, Color::rgb(1.0, 0.0, 0.0)); // distance = 1.0
        let result = compare_framebuffers(&a, &b, 0.0);
        assert!((result.avg_diff - 0.5).abs() < 0.001); // (1.0 + 0.0) / 2
        assert!((result.max_diff - 1.0).abs() < 0.001);
    }
}
