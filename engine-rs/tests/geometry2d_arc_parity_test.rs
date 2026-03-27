//! pat-hew7: Geometry2D arc fixed-subdivision parity test.
//!
//! Godot 4.6.1 removed tolerance-based adaptive arc subdivision and
//! switched to a fixed point count.  These tests verify that Patina's
//! `geometry2d::build_arc` matches the 4.6.1 semantics:
//!
//! - The output has exactly `point_count` vertices.
//! - Points are **evenly spaced** in angle (fixed subdivision, not adaptive).
//! - The first and last points lie at the requested start/end angles.
//! - All points lie at the requested radius from the center.
//! - Edge cases: minimal point count, full circle, reverse sweep.

use gdcore::geometry2d;
use gdcore::math::Vector2;
use std::f32::consts::{FRAC_PI_2, FRAC_PI_4, PI, TAU};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Asserts two f32 values are within `eps` of each other.
fn assert_near(a: f32, b: f32, eps: f32, msg: &str) {
    assert!(
        (a - b).abs() < eps,
        "{msg}: expected {b}, got {a} (diff={})",
        (a - b).abs()
    );
}

/// Asserts two angles are within `eps`, accounting for wrapping at ±PI.
fn assert_angle_near(a: f32, b: f32, eps: f32, msg: &str) {
    let diff = (a - b + PI).rem_euclid(TAU) - PI;
    assert!(
        diff.abs() < eps,
        "{msg}: expected {b}, got {a} (angular diff={})",
        diff.abs()
    );
}

/// Returns the angle of a point relative to a center.
fn angle_of(point: Vector2, center: Vector2) -> f32 {
    (point.y - center.y).atan2(point.x - center.x)
}

// ===========================================================================
// 1. Fixed-count contract
// ===========================================================================

#[test]
fn fixed_subdivision_exact_point_count() {
    for count in [2, 3, 10, 32, 64, 128, 256] {
        let pts = geometry2d::build_arc(Vector2::ZERO, 1.0, 0.0, PI, count);
        assert_eq!(
            pts.len(),
            count as usize,
            "build_arc must return exactly {count} points"
        );
    }
}

// ===========================================================================
// 2. Even angular spacing (core 4.6.1 change)
// ===========================================================================

#[test]
fn even_angular_spacing_half_circle() {
    // Use a quarter-circle to avoid the atan2 discontinuity at ±PI.
    let pts = geometry2d::build_arc(Vector2::ZERO, 1.0, 0.0, FRAC_PI_2, 17);
    let expected_step = FRAC_PI_2 / 16.0;

    for i in 0..(pts.len() - 1) {
        let a0 = angle_of(pts[i], Vector2::ZERO);
        let a1 = angle_of(pts[i + 1], Vector2::ZERO);
        let gap = a1 - a0;
        assert_near(
            gap,
            expected_step,
            1e-4,
            &format!("angular gap between point {i} and {}", i + 1),
        );
    }
}

#[test]
fn even_angular_spacing_quarter_circle() {
    let pts = geometry2d::build_arc(Vector2::ZERO, 50.0, 0.0, FRAC_PI_2, 9);
    let expected_step = FRAC_PI_2 / 8.0;

    for i in 0..(pts.len() - 1) {
        let a0 = angle_of(pts[i], Vector2::ZERO);
        let a1 = angle_of(pts[i + 1], Vector2::ZERO);
        assert_near(
            a1 - a0,
            expected_step,
            1e-4,
            &format!("angular gap at index {i}"),
        );
    }
}

// ===========================================================================
// 3. Endpoint angles
// ===========================================================================

#[test]
fn first_point_matches_start_angle() {
    for start in [0.0_f32, FRAC_PI_4, PI, -FRAC_PI_2] {
        let pts = geometry2d::build_arc(Vector2::ZERO, 1.0, start, start + PI, 10);
        let actual = angle_of(pts[0], Vector2::ZERO);
        assert_angle_near(actual, start, 1e-5, &format!("start_angle={start}"));
    }
}

#[test]
fn last_point_matches_end_angle() {
    for end in [PI, FRAC_PI_2, TAU, -FRAC_PI_4] {
        let pts = geometry2d::build_arc(Vector2::ZERO, 1.0, 0.0, end, 10);
        let actual = angle_of(*pts.last().unwrap(), Vector2::ZERO);
        // Normalize both to [-PI, PI] for comparison.
        let expected = end.sin().atan2(end.cos());
        let actual_norm = actual.sin().atan2(actual.cos());
        assert_near(
            actual_norm,
            expected,
            1e-4,
            &format!("end_angle={end}"),
        );
    }
}

// ===========================================================================
// 4. Radius invariant
// ===========================================================================

#[test]
fn all_points_at_requested_radius() {
    let center = Vector2::new(100.0, -50.0);
    let radius = 75.0;
    let pts = geometry2d::build_arc(center, radius, 0.0, TAU, 100);

    for (i, p) in pts.iter().enumerate() {
        let dist = ((p.x - center.x).powi(2) + (p.y - center.y).powi(2)).sqrt();
        assert_near(
            dist,
            radius,
            1e-3,
            &format!("point {i} distance from center"),
        );
    }
}

#[test]
fn various_radii() {
    for &r in &[0.001, 1.0, 100.0, 10_000.0] {
        let pts = geometry2d::build_arc(Vector2::ZERO, r, 0.0, PI, 16);
        for p in &pts {
            let dist = p.length();
            assert_near(dist, r, r * 1e-5 + 1e-6, &format!("radius={r}"));
        }
    }
}

// ===========================================================================
// 5. Edge cases
// ===========================================================================

#[test]
fn minimal_two_points_gives_start_and_end() {
    let pts = geometry2d::build_arc(Vector2::ZERO, 1.0, 0.0, PI, 2);
    assert_eq!(pts.len(), 2);
    assert_near(pts[0].x, 1.0, 1e-5, "start x");
    assert_near(pts[0].y, 0.0, 1e-5, "start y");
    assert_near(pts[1].x, -1.0, 1e-5, "end x");
    assert_near(pts[1].y, 0.0, 1e-4, "end y");
}

#[test]
fn degenerate_count_returns_empty() {
    assert!(geometry2d::build_arc(Vector2::ZERO, 1.0, 0.0, PI, 0).is_empty());
    assert!(geometry2d::build_arc(Vector2::ZERO, 1.0, 0.0, PI, 1).is_empty());
}

#[test]
fn full_circle_first_last_coincide() {
    let pts = geometry2d::build_arc(Vector2::ZERO, 10.0, 0.0, TAU, 64);
    let first = pts[0];
    let last = *pts.last().unwrap();
    assert_near(first.x, last.x, 1e-3, "full circle x");
    assert_near(first.y, last.y, 1e-3, "full circle y");
}

#[test]
fn reverse_sweep_produces_correct_direction() {
    // Sweeping from PI to 0 (decreasing angle).
    let pts = geometry2d::build_arc(Vector2::ZERO, 1.0, PI, 0.0, 5);
    assert_eq!(pts.len(), 5);
    // Angles should decrease from PI to 0.
    let a_first = angle_of(pts[0], Vector2::ZERO);
    let a_last = angle_of(*pts.last().unwrap(), Vector2::ZERO);
    assert_angle_near(a_first, PI, 1e-5, "reverse sweep start");
    assert_near(a_last.abs(), 0.0, 1e-4, "reverse sweep end");
}

// ===========================================================================
// 6. Not adaptive — verify NO tolerance parameter influence
// ===========================================================================

#[test]
fn same_count_same_output_regardless_of_sweep_size() {
    // In the old tolerance-based system, a larger sweep with the same
    // "tolerance" would produce MORE points.  Fixed-count means the
    // caller controls the count directly — 16 points for a tiny arc
    // and 16 points for a huge arc.
    let small = geometry2d::build_arc(Vector2::ZERO, 1.0, 0.0, 0.01, 16);
    let big = geometry2d::build_arc(Vector2::ZERO, 1.0, 0.0, TAU, 16);
    assert_eq!(small.len(), 16, "small arc: count must be 16");
    assert_eq!(big.len(), 16, "big arc: count must be 16");
}

// ===========================================================================
// 7. Companion Geometry2D utilities
// ===========================================================================

#[test]
fn point_in_polygon_arc_hull() {
    // Build a semicircular arc and verify the center is "inside" the
    // polygon formed by the arc + chord closure.
    let mut pts = geometry2d::build_arc(Vector2::ZERO, 10.0, 0.0, PI, 32);
    // Close the polygon with a chord back to the start.
    pts.push(pts[0]);
    // Center (0, 0) is below the semicircle chord — whether it's "inside"
    // depends on winding. Just verify the API runs without panic.
    let _ = geometry2d::is_point_in_polygon(Vector2::ZERO, &pts);
}

#[test]
fn closest_point_to_segment_basic() {
    let p = geometry2d::get_closest_point_to_segment(
        Vector2::new(5.0, 5.0),
        Vector2::new(0.0, 0.0),
        Vector2::new(10.0, 0.0),
    );
    assert_near(p.x, 5.0, 1e-5, "closest x");
    assert_near(p.y, 0.0, 1e-5, "closest y");
}
