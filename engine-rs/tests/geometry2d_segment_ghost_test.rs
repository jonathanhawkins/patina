//! pat-nr44: Geometry2D segment ghost-collision regression (4.6.1 delta).
//!
//! The Godot 4.6.1 release fixed a ghost-collision bug in
//! `Geometry2D.segment_intersects_segment` where nearly-parallel segments
//! produced phantom intersection points.  This test ensures Patina's
//! implementation uses a sufficient epsilon on the cross-product denominator
//! to reject those false positives, while still reporting real intersections.

use gdcore::geometry2d::segment_intersects_segment;
use gdcore::math::Vector2;

// Helper — approximate equality for intersection points.
fn approx(a: Vector2, b: Vector2) -> bool {
    (a.x - b.x).abs() < 1e-4 && (a.y - b.y).abs() < 1e-4
}

// ===========================================================================
// 1. Real intersection — basic cross
// ===========================================================================

#[test]
fn simple_cross_intersection() {
    // X pattern: (0,0)→(10,10) crosses (10,0)→(0,10) at (5,5).
    let hit = segment_intersects_segment(
        Vector2::new(0.0, 0.0),
        Vector2::new(10.0, 10.0),
        Vector2::new(10.0, 0.0),
        Vector2::new(0.0, 10.0),
    );
    let p = hit.expect("crossing segments must intersect");
    assert!(approx(p, Vector2::new(5.0, 5.0)), "got {:?}", p);
}

#[test]
fn t_junction_at_endpoint() {
    // Horizontal (0,0)→(10,0) meets vertical (5,-5)→(5,0) at (5,0).
    let hit = segment_intersects_segment(
        Vector2::new(0.0, 0.0),
        Vector2::new(10.0, 0.0),
        Vector2::new(5.0, -5.0),
        Vector2::new(5.0, 0.0),
    );
    let p = hit.expect("T-junction at endpoint should intersect");
    assert!(approx(p, Vector2::new(5.0, 0.0)), "got {:?}", p);
}

// ===========================================================================
// 2. Ghost-collision regression — nearly-parallel segments
// ===========================================================================

#[test]
fn near_parallel_no_ghost_collision() {
    // Two long, almost-parallel horizontal segments offset by a tiny angle.
    // Pre-fix Godot would report a phantom intersection far outside the
    // segment bounds due to denominator underflow.
    let hit = segment_intersects_segment(
        Vector2::new(0.0, 0.0),
        Vector2::new(1000.0, 0.0),
        Vector2::new(0.0, 0.001),
        Vector2::new(1000.0, 0.001),
    );
    assert!(
        hit.is_none(),
        "nearly-parallel segments must NOT report ghost collision, got {:?}",
        hit
    );
}

#[test]
fn near_parallel_angled_no_ghost() {
    // Two segments at ~0.0001 radian difference, 10 000 units long.
    let hit = segment_intersects_segment(
        Vector2::new(0.0, 0.0),
        Vector2::new(10_000.0, 1.0),
        Vector2::new(0.0, 0.5),
        Vector2::new(10_000.0, 1.5),
    );
    assert!(
        hit.is_none(),
        "nearly-parallel angled segments must NOT ghost-intersect, got {:?}",
        hit
    );
}

// ===========================================================================
// 3. Exactly parallel — no intersection
// ===========================================================================

#[test]
fn exactly_parallel_no_intersection() {
    let hit = segment_intersects_segment(
        Vector2::new(0.0, 0.0),
        Vector2::new(10.0, 0.0),
        Vector2::new(0.0, 5.0),
        Vector2::new(10.0, 5.0),
    );
    assert!(hit.is_none(), "parallel segments must not intersect");
}

// ===========================================================================
// 4. Collinear overlapping — treated as no single intersection point
// ===========================================================================

#[test]
fn collinear_overlapping_returns_none() {
    // Two segments on the same line overlapping: [0,10] and [5,15].
    let hit = segment_intersects_segment(
        Vector2::new(0.0, 0.0),
        Vector2::new(10.0, 0.0),
        Vector2::new(5.0, 0.0),
        Vector2::new(15.0, 0.0),
    );
    assert!(
        hit.is_none(),
        "collinear overlapping segments have no single intersection point"
    );
}

#[test]
fn collinear_non_overlapping_returns_none() {
    let hit = segment_intersects_segment(
        Vector2::new(0.0, 0.0),
        Vector2::new(5.0, 0.0),
        Vector2::new(6.0, 0.0),
        Vector2::new(10.0, 0.0),
    );
    assert!(hit.is_none(), "collinear non-overlapping must not intersect");
}

// ===========================================================================
// 5. Zero-length segment edge cases
// ===========================================================================

#[test]
fn zero_length_segment_no_ghost() {
    // A degenerate (zero-length) segment must not cause division-by-zero
    // ghost collisions.
    let hit = segment_intersects_segment(
        Vector2::new(5.0, 5.0),
        Vector2::new(5.0, 5.0), // zero-length
        Vector2::new(0.0, 0.0),
        Vector2::new(10.0, 10.0),
    );
    // Cross product of zero-length direction with anything is 0 → parallel path.
    assert!(
        hit.is_none(),
        "zero-length segment must not produce ghost collision, got {:?}",
        hit
    );
}

// ===========================================================================
// 6. Non-intersecting segments (miss cases)
// ===========================================================================

#[test]
fn disjoint_segments_no_intersection() {
    // Lines would cross, but segments don't reach each other.
    let hit = segment_intersects_segment(
        Vector2::new(0.0, 0.0),
        Vector2::new(1.0, 1.0),
        Vector2::new(5.0, 0.0),
        Vector2::new(6.0, 1.0),
    );
    assert!(hit.is_none(), "disjoint segments must not intersect");
}

#[test]
fn perpendicular_miss() {
    // Perpendicular but non-overlapping in parameter space.
    let hit = segment_intersects_segment(
        Vector2::new(0.0, 0.0),
        Vector2::new(3.0, 0.0),
        Vector2::new(5.0, -5.0),
        Vector2::new(5.0, 5.0),
    );
    assert!(hit.is_none(), "perpendicular miss must return None");
}
