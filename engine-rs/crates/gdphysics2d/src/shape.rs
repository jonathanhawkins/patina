//! Collision shape definitions.
//!
//! Provides 2D collision shapes for the physics engine: circles, rectangles,
//! line segments, and capsules. Each shape can compute its axis-aligned
//! bounding box and test point containment.

use gdcore::math::{Rect2, Vector2};

/// A 2D collision shape.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Shape2D {
    /// A circle centered at the origin with the given radius.
    Circle { radius: f32 },
    /// An axis-aligned rectangle defined by half-extents from the origin.
    Rectangle { half_extents: Vector2 },
    /// A line segment between two local points.
    Segment { a: Vector2, b: Vector2 },
    /// A capsule oriented vertically (along Y), defined by radius and total height.
    Capsule { radius: f32, height: f32 },
}

impl Shape2D {
    /// Computes the axis-aligned bounding rectangle for broad-phase collision.
    ///
    /// The returned `Rect2` is in local space (centered on the shape origin).
    pub fn bounding_rect(&self) -> Rect2 {
        match *self {
            Shape2D::Circle { radius } => Rect2::new(
                Vector2::new(-radius, -radius),
                Vector2::new(radius * 2.0, radius * 2.0),
            ),
            Shape2D::Rectangle { half_extents } => Rect2::new(
                Vector2::new(-half_extents.x, -half_extents.y),
                Vector2::new(half_extents.x * 2.0, half_extents.y * 2.0),
            ),
            Shape2D::Segment { a, b } => {
                let min_x = a.x.min(b.x);
                let min_y = a.y.min(b.y);
                let max_x = a.x.max(b.x);
                let max_y = a.y.max(b.y);
                Rect2::new(
                    Vector2::new(min_x, min_y),
                    Vector2::new(max_x - min_x, max_y - min_y),
                )
            }
            Shape2D::Capsule { radius, height } => {
                let half_height = height / 2.0;
                Rect2::new(
                    Vector2::new(-radius, -half_height),
                    Vector2::new(radius * 2.0, height),
                )
            }
        }
    }

    /// Tests whether a point (in local shape space) is contained within the shape.
    pub fn contains_point(&self, point: Vector2) -> bool {
        match *self {
            Shape2D::Circle { radius } => point.length_squared() <= radius * radius,
            Shape2D::Rectangle { half_extents } => {
                point.x.abs() <= half_extents.x && point.y.abs() <= half_extents.y
            }
            Shape2D::Segment { a, b } => {
                // A segment has zero area; use a small tolerance for point-on-segment.
                let ab = b - a;
                let ap = point - a;
                let t = if ab.length_squared() < 1e-12 {
                    0.0
                } else {
                    ap.dot(ab) / ab.length_squared()
                };
                if t < 0.0 || t > 1.0 {
                    return false;
                }
                let closest = a + ab * t;
                (point - closest).length_squared() < 1e-6
            }
            Shape2D::Capsule { radius, height } => {
                let half_h = height / 2.0 - radius;
                // Clamp the y component to the capsule's line segment
                let clamped_y = point.y.clamp(-half_h.max(0.0), half_h.max(0.0));
                let closest = Vector2::new(0.0, clamped_y);
                (point - closest).length_squared() <= radius * radius
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EPSILON: f32 = 1e-5;

    fn approx_eq(a: f32, b: f32) -> bool {
        (a - b).abs() < EPSILON
    }

    // --- AABB tests ---

    #[test]
    fn circle_bounding_rect() {
        let shape = Shape2D::Circle { radius: 5.0 };
        let r = shape.bounding_rect();
        assert!(approx_eq(r.position.x, -5.0));
        assert!(approx_eq(r.position.y, -5.0));
        assert!(approx_eq(r.size.x, 10.0));
        assert!(approx_eq(r.size.y, 10.0));
    }

    #[test]
    fn rectangle_bounding_rect() {
        let shape = Shape2D::Rectangle {
            half_extents: Vector2::new(3.0, 4.0),
        };
        let r = shape.bounding_rect();
        assert!(approx_eq(r.position.x, -3.0));
        assert!(approx_eq(r.position.y, -4.0));
        assert!(approx_eq(r.size.x, 6.0));
        assert!(approx_eq(r.size.y, 8.0));
    }

    #[test]
    fn segment_bounding_rect() {
        let shape = Shape2D::Segment {
            a: Vector2::new(-2.0, 1.0),
            b: Vector2::new(3.0, -4.0),
        };
        let r = shape.bounding_rect();
        assert!(approx_eq(r.position.x, -2.0));
        assert!(approx_eq(r.position.y, -4.0));
        assert!(approx_eq(r.size.x, 5.0));
        assert!(approx_eq(r.size.y, 5.0));
    }

    #[test]
    fn capsule_bounding_rect() {
        let shape = Shape2D::Capsule {
            radius: 2.0,
            height: 10.0,
        };
        let r = shape.bounding_rect();
        assert!(approx_eq(r.position.x, -2.0));
        assert!(approx_eq(r.position.y, -5.0));
        assert!(approx_eq(r.size.x, 4.0));
        assert!(approx_eq(r.size.y, 10.0));
    }

    // --- Point containment tests ---

    #[test]
    fn circle_contains_point() {
        let shape = Shape2D::Circle { radius: 5.0 };
        assert!(shape.contains_point(Vector2::ZERO));
        assert!(shape.contains_point(Vector2::new(3.0, 4.0))); // exactly on boundary
        assert!(!shape.contains_point(Vector2::new(4.0, 4.0))); // outside
    }

    #[test]
    fn rectangle_contains_point() {
        let shape = Shape2D::Rectangle {
            half_extents: Vector2::new(3.0, 4.0),
        };
        assert!(shape.contains_point(Vector2::ZERO));
        assert!(shape.contains_point(Vector2::new(3.0, 4.0))); // on boundary
        assert!(!shape.contains_point(Vector2::new(3.1, 0.0))); // outside
    }

    #[test]
    fn zero_radius_circle_bounding_rect() {
        let shape = Shape2D::Circle { radius: 0.0 };
        let r = shape.bounding_rect();
        assert!(approx_eq(r.size.x, 0.0));
        assert!(approx_eq(r.size.y, 0.0));
    }

    #[test]
    fn zero_size_rectangle_contains_origin() {
        let shape = Shape2D::Rectangle {
            half_extents: Vector2::ZERO,
        };
        // The origin point itself should be "on the boundary" (<=)
        assert!(shape.contains_point(Vector2::ZERO));
        assert!(!shape.contains_point(Vector2::new(0.1, 0.0)));
    }
}
