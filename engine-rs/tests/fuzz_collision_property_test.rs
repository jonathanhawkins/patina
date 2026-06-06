//! Property tests for the 2D collision detection system.
//!
//! Tests cover collision symmetry, numeric edge cases, separation
//! correctness, and shape bounding-rect consistency.

use gdcore::math::{Transform2D, Vector2};
use gdphysics2d::collision::{test_collision, CollisionResult};
use gdphysics2d::shape::Shape2D;
use proptest::prelude::*;

// ---------------------------------------------------------------------------
// Strategies for generating valid shapes and transforms
// ---------------------------------------------------------------------------

fn arb_positive_f32() -> impl Strategy<Value = f32> {
    0.01f32..1000.0
}

fn arb_position() -> impl Strategy<Value = Vector2> {
    (-500.0f32..500.0, -500.0f32..500.0).prop_map(|(x, y)| Vector2::new(x, y))
}

fn arb_circle() -> impl Strategy<Value = Shape2D> {
    arb_positive_f32().prop_map(|r| Shape2D::Circle { radius: r })
}

fn arb_rectangle() -> impl Strategy<Value = Shape2D> {
    (arb_positive_f32(), arb_positive_f32()).prop_map(|(w, h)| Shape2D::Rectangle {
        half_extents: Vector2::new(w, h),
    })
}

fn arb_shape() -> impl Strategy<Value = Shape2D> {
    prop_oneof![arb_circle(), arb_rectangle(),]
}

// ---------------------------------------------------------------------------
// Collision symmetry: test_collision(A, B) colliding iff test_collision(B, A) colliding
// ---------------------------------------------------------------------------

proptest! {
    /// Collision detection is symmetric: if A collides with B, then B collides with A.
    #[test]
    fn collision_symmetry(
        shape_a in arb_shape(),
        pos_a in arb_position(),
        shape_b in arb_shape(),
        pos_b in arb_position(),
    ) {
        let ta = Transform2D::translated(pos_a);
        let tb = Transform2D::translated(pos_b);

        let result_ab = test_collision(&shape_a, &ta, &shape_b, &tb);
        let result_ba = test_collision(&shape_b, &tb, &shape_a, &ta);

        match (result_ab, result_ba) {
            (Some(ab), Some(ba)) => {
                prop_assert_eq!(ab.colliding, ba.colliding,
                    "Symmetry violation: A-B colliding={} but B-A colliding={}",
                    ab.colliding, ba.colliding);
                if ab.colliding {
                    // Depths should be approximately equal
                    prop_assert!((ab.depth - ba.depth).abs() < 0.01,
                        "Depth mismatch: A-B depth={} vs B-A depth={}",
                        ab.depth, ba.depth);
                }
            }
            (None, None) => {} // Both unsupported, that's fine
            (a, b) => prop_assert!(false,
                "Asymmetric support: A-B={:?}, B-A={:?}", a, b),
        }
    }
}

// ---------------------------------------------------------------------------
// Circle-circle specific properties
// ---------------------------------------------------------------------------

proptest! {
    /// Two circles collide iff distance between centers < sum of radii.
    #[test]
    fn circle_circle_correctness(
        ra in 0.1f32..100.0,
        rb in 0.1f32..100.0,
        pos_a in arb_position(),
        pos_b in arb_position(),
    ) {
        let ta = Transform2D::translated(pos_a);
        let tb = Transform2D::translated(pos_b);
        let sa = Shape2D::Circle { radius: ra };
        let sb = Shape2D::Circle { radius: rb };

        let result = test_collision(&sa, &ta, &sb, &tb).unwrap();
        let dist = (pos_b - pos_a).length();
        let sum_r = ra + rb;

        if dist > sum_r + 0.001 {
            prop_assert!(!result.colliding,
                "Expected no collision: dist={dist}, sum_r={sum_r}");
        }
        if dist < sum_r - 0.001 {
            prop_assert!(result.colliding,
                "Expected collision: dist={dist}, sum_r={sum_r}");
        }
    }

    /// Coincident circles (same position) produce a valid result.
    #[test]
    fn coincident_circles_valid(
        r in 0.1f32..100.0,
        pos in arb_position(),
    ) {
        let t = Transform2D::translated(pos);
        let s = Shape2D::Circle { radius: r };

        let result = test_collision(&s, &t, &s, &t).unwrap();
        prop_assert!(result.colliding, "Coincident circles should collide");
        prop_assert!(result.depth > 0.0, "Depth should be positive");
        // Normal should be unit length (or close to it)
        let normal_len = result.normal.length();
        prop_assert!((normal_len - 1.0).abs() < 0.01,
            "Normal should be unit length, got {normal_len}");
    }
}

// ---------------------------------------------------------------------------
// Rectangle-rectangle specific properties
// ---------------------------------------------------------------------------

proptest! {
    /// Two rects with the same center always collide.
    #[test]
    fn rect_same_center_collides(
        he_a in (0.1f32..100.0, 0.1f32..100.0),
        pos in arb_position(),
    ) {
        let t = Transform2D::translated(pos);
        let s = Shape2D::Rectangle {
            half_extents: Vector2::new(he_a.0, he_a.1),
        };

        let result = test_collision(&s, &t, &s, &t).unwrap();
        prop_assert!(result.colliding, "Same-position rects should collide");
    }

    /// Two rects far apart do not collide.
    #[test]
    fn rect_far_apart_no_collision(
        he in 1.0f32..10.0,
        gap in 1.0f32..100.0,
    ) {
        let sa = Shape2D::Rectangle {
            half_extents: Vector2::new(he, he),
        };
        let sb = Shape2D::Rectangle {
            half_extents: Vector2::new(he, he),
        };
        // Place them far apart on the x-axis
        let ta = Transform2D::translated(Vector2::new(0.0, 0.0));
        let tb = Transform2D::translated(Vector2::new(he * 2.0 + gap, 0.0));

        let result = test_collision(&sa, &ta, &sb, &tb).unwrap();
        prop_assert!(!result.colliding,
            "Rects separated by gap={gap} should not collide");
    }
}

// ---------------------------------------------------------------------------
// Collision result invariants
// ---------------------------------------------------------------------------

proptest! {
    /// When colliding, depth is always positive.
    #[test]
    fn collision_depth_positive(
        shape_a in arb_shape(),
        shape_b in arb_shape(),
    ) {
        // Place shapes at same position to guarantee collision
        let t = Transform2D::translated(Vector2::ZERO);

        if let Some(result) = test_collision(&shape_a, &t, &shape_b, &t) {
            if result.colliding {
                prop_assert!(result.depth > 0.0,
                    "Collision depth should be positive, got {}", result.depth);
            }
        }
    }

    /// When colliding, normal is approximately unit length.
    #[test]
    fn collision_normal_unit_length(
        shape_a in arb_shape(),
        shape_b in arb_shape(),
    ) {
        let t = Transform2D::translated(Vector2::ZERO);

        if let Some(result) = test_collision(&shape_a, &t, &shape_b, &t) {
            if result.colliding {
                let len = result.normal.length();
                prop_assert!((len - 1.0).abs() < 0.01,
                    "Normal should be unit length, got {len}");
            }
        }
    }
}

// ---------------------------------------------------------------------------
// CollisionResult::NONE sentinel
// ---------------------------------------------------------------------------

#[test]
fn collision_none_sentinel() {
    let none = CollisionResult::NONE;
    assert!(!none.colliding);
    assert_eq!(none.depth, 0.0);
    assert_eq!(none.normal, Vector2::ZERO);
    assert_eq!(none.point, Vector2::ZERO);
}

// ---------------------------------------------------------------------------
// Shape bounding rect properties
// ---------------------------------------------------------------------------

proptest! {
    /// Bounding rect dimensions are non-negative for all valid shapes.
    #[test]
    fn bounding_rect_non_negative(shape in arb_shape()) {
        let rect = shape.bounding_rect();
        prop_assert!(rect.size.x >= 0.0,
            "Bounding rect width should be non-negative, got {}", rect.size.x);
        prop_assert!(rect.size.y >= 0.0,
            "Bounding rect height should be non-negative, got {}", rect.size.y);
    }

    /// Shape origin is inside its own bounding rect.
    #[test]
    fn origin_inside_bounding_rect(shape in arb_shape()) {
        let rect = shape.bounding_rect();
        prop_assert!(
            rect.position.x <= 0.0 && 0.0 <= rect.position.x + rect.size.x,
            "Origin x=0 not inside bounding rect [{}, {}]",
            rect.position.x, rect.position.x + rect.size.x
        );
        prop_assert!(
            rect.position.y <= 0.0 && 0.0 <= rect.position.y + rect.size.y,
            "Origin y=0 not inside bounding rect [{}, {}]",
            rect.position.y, rect.position.y + rect.size.y
        );
    }

    /// Points contained by the shape are within its bounding rect.
    #[test]
    fn contained_point_inside_bounding(
        radius in 1.0f32..50.0,
        angle in 0.0f32..std::f32::consts::TAU,
        fraction in 0.0f32..0.99,
    ) {
        let shape = Shape2D::Circle { radius };
        let point = Vector2::new(
            angle.cos() * radius * fraction,
            angle.sin() * radius * fraction,
        );
        prop_assert!(shape.contains_point(point),
            "Point should be inside circle");

        let rect = shape.bounding_rect();
        prop_assert!(
            point.x >= rect.position.x && point.x <= rect.position.x + rect.size.x,
            "Contained point x={} outside bounding rect", point.x
        );
        prop_assert!(
            point.y >= rect.position.y && point.y <= rect.position.y + rect.size.y,
            "Contained point y={} outside bounding rect", point.y
        );
    }
}

// ---------------------------------------------------------------------------
// Zero-size / degenerate shape edge cases
// ---------------------------------------------------------------------------

#[test]
fn zero_radius_circle_collision() {
    let s = Shape2D::Circle { radius: 0.0 };
    let t = Transform2D::translated(Vector2::ZERO);
    // Two zero-radius circles at the same point
    let result = test_collision(&s, &t, &s, &t).unwrap();
    // Coincident: depth = sum_r = 0, so this is an edge case
    // Just verify it doesn't panic
    let _ = result;
}

#[test]
fn zero_extents_rectangle_collision() {
    let s = Shape2D::Rectangle {
        half_extents: Vector2::ZERO,
    };
    let t = Transform2D::translated(Vector2::ZERO);
    let result = test_collision(&s, &t, &s, &t).unwrap();
    // Zero-size rects at same position: overlap_x = 0, overlap_y = 0
    // The <= 0 check means no collision, which is acceptable
    let _ = result;
}
