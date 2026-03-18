//! 3D collision shape definitions.
//!
//! Provides 3D collision shapes for the physics engine: spheres, boxes,
//! and capsules. Each shape can compute its axis-aligned bounding box
//! and test point containment.

use gdcore::math::Vector3;
use gdcore::math3d::Aabb;

/// A 3D collision shape.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Shape3D {
    /// A sphere centered at the origin with the given radius.
    Sphere { radius: f32 },
    /// An axis-aligned box defined by half-extents from the origin.
    BoxShape { half_extents: Vector3 },
    /// A capsule oriented vertically (along Y), defined by radius and total height.
    CapsuleShape { radius: f32, height: f32 },
}

impl Shape3D {
    /// Computes the axis-aligned bounding box for broad-phase collision.
    ///
    /// The returned `Aabb` is in local space (centered on the shape origin).
    pub fn bounding_aabb(&self) -> Aabb {
        match *self {
            Shape3D::Sphere { radius } => Aabb::new(
                Vector3::new(-radius, -radius, -radius),
                Vector3::new(radius * 2.0, radius * 2.0, radius * 2.0),
            ),
            Shape3D::BoxShape { half_extents } => Aabb::new(
                Vector3::new(-half_extents.x, -half_extents.y, -half_extents.z),
                Vector3::new(
                    half_extents.x * 2.0,
                    half_extents.y * 2.0,
                    half_extents.z * 2.0,
                ),
            ),
            Shape3D::CapsuleShape { radius, height } => {
                let half_height = height / 2.0;
                Aabb::new(
                    Vector3::new(-radius, -half_height, -radius),
                    Vector3::new(radius * 2.0, height, radius * 2.0),
                )
            }
        }
    }

    /// Tests whether a point (in local shape space) is contained within the shape.
    pub fn contains_point(&self, point: Vector3) -> bool {
        match *self {
            Shape3D::Sphere { radius } => point.length_squared() <= radius * radius,
            Shape3D::BoxShape { half_extents } => {
                point.x.abs() <= half_extents.x
                    && point.y.abs() <= half_extents.y
                    && point.z.abs() <= half_extents.z
            }
            Shape3D::CapsuleShape { radius, height } => {
                let half_h = (height / 2.0 - radius).max(0.0);
                let clamped_y = point.y.clamp(-half_h, half_h);
                let closest = Vector3::new(0.0, clamped_y, 0.0);
                let diff = point - closest;
                diff.length_squared() <= radius * radius
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

    #[test]
    fn sphere_bounding_aabb() {
        let shape = Shape3D::Sphere { radius: 5.0 };
        let aabb = shape.bounding_aabb();
        assert!(approx_eq(aabb.position.x, -5.0));
        assert!(approx_eq(aabb.position.y, -5.0));
        assert!(approx_eq(aabb.position.z, -5.0));
        assert!(approx_eq(aabb.size.x, 10.0));
        assert!(approx_eq(aabb.size.y, 10.0));
        assert!(approx_eq(aabb.size.z, 10.0));
    }

    #[test]
    fn box_bounding_aabb() {
        let shape = Shape3D::BoxShape {
            half_extents: Vector3::new(3.0, 4.0, 5.0),
        };
        let aabb = shape.bounding_aabb();
        assert!(approx_eq(aabb.position.x, -3.0));
        assert!(approx_eq(aabb.position.y, -4.0));
        assert!(approx_eq(aabb.position.z, -5.0));
        assert!(approx_eq(aabb.size.x, 6.0));
        assert!(approx_eq(aabb.size.y, 8.0));
        assert!(approx_eq(aabb.size.z, 10.0));
    }

    #[test]
    fn capsule_bounding_aabb() {
        let shape = Shape3D::CapsuleShape {
            radius: 2.0,
            height: 10.0,
        };
        let aabb = shape.bounding_aabb();
        assert!(approx_eq(aabb.position.x, -2.0));
        assert!(approx_eq(aabb.position.y, -5.0));
        assert!(approx_eq(aabb.position.z, -2.0));
        assert!(approx_eq(aabb.size.x, 4.0));
        assert!(approx_eq(aabb.size.y, 10.0));
        assert!(approx_eq(aabb.size.z, 4.0));
    }

    #[test]
    fn sphere_contains_point() {
        let shape = Shape3D::Sphere { radius: 5.0 };
        assert!(shape.contains_point(Vector3::ZERO));
        assert!(shape.contains_point(Vector3::new(3.0, 4.0, 0.0))); // on boundary
        assert!(!shape.contains_point(Vector3::new(3.0, 4.0, 1.0))); // outside
    }

    #[test]
    fn box_contains_point() {
        let shape = Shape3D::BoxShape {
            half_extents: Vector3::new(3.0, 4.0, 5.0),
        };
        assert!(shape.contains_point(Vector3::ZERO));
        assert!(shape.contains_point(Vector3::new(3.0, 4.0, 5.0))); // on boundary
        assert!(!shape.contains_point(Vector3::new(3.1, 0.0, 0.0))); // outside
    }

    #[test]
    fn capsule_contains_point() {
        let shape = Shape3D::CapsuleShape {
            radius: 2.0,
            height: 10.0,
        };
        assert!(shape.contains_point(Vector3::ZERO));
        // Point at top cap center
        assert!(shape.contains_point(Vector3::new(0.0, 5.0, 0.0)));
        // Point outside
        assert!(!shape.contains_point(Vector3::new(3.0, 0.0, 0.0)));
    }

    #[test]
    fn zero_radius_sphere() {
        let shape = Shape3D::Sphere { radius: 0.0 };
        let aabb = shape.bounding_aabb();
        assert!(approx_eq(aabb.size.x, 0.0));
        assert!(shape.contains_point(Vector3::ZERO));
        assert!(!shape.contains_point(Vector3::new(0.1, 0.0, 0.0)));
    }
}
