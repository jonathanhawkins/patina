//! 3D collision shape definitions.

use gdcore::math::Vector3;
use gdcore::math3d::Aabb;

/// A 3D collision shape.
#[derive(Debug, Clone, PartialEq)]
pub enum Shape3D {
    /// A sphere centered at the origin with the given radius.
    Sphere { radius: f32 },
    /// An axis-aligned box defined by half-extents from the origin.
    BoxShape { half_extents: Vector3 },
    /// A capsule oriented vertically (along Y), defined by radius and total height.
    CapsuleShape { radius: f32, height: f32 },
    /// A cylinder oriented vertically (along Y), defined by radius and total height.
    CylinderShape { radius: f32, height: f32 },
    /// A convex polygon defined by a set of 3D vertices (convex hull).
    ConvexPolygonShape { points: Vec<Vector3> },
    /// A concave polygon (trimesh) defined by triangle faces.
    ConcavePolygonShape { faces: Vec<Vector3> },
    /// An infinite plane defined by a normal and distance from origin.
    WorldBoundaryShape { normal: Vector3, distance: f32 },
}

impl Shape3D {
    /// Computes the axis-aligned bounding box for broad-phase collision.
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
            Shape3D::CylinderShape { radius, height } => {
                let half_height = height / 2.0;
                Aabb::new(
                    Vector3::new(-radius, -half_height, -radius),
                    Vector3::new(radius * 2.0, height, radius * 2.0),
                )
            }
            Shape3D::ConvexPolygonShape { ref points } => {
                if points.is_empty() {
                    return Aabb::new(Vector3::ZERO, Vector3::ZERO);
                }
                let mut min = points[0];
                let mut max = points[0];
                for p in &points[1..] {
                    min.x = min.x.min(p.x);
                    min.y = min.y.min(p.y);
                    min.z = min.z.min(p.z);
                    max.x = max.x.max(p.x);
                    max.y = max.y.max(p.y);
                    max.z = max.z.max(p.z);
                }
                Aabb::new(min, Vector3::new(max.x - min.x, max.y - min.y, max.z - min.z))
            }
            Shape3D::ConcavePolygonShape { ref faces } => {
                if faces.is_empty() {
                    return Aabb::new(Vector3::ZERO, Vector3::ZERO);
                }
                let mut min = faces[0];
                let mut max = faces[0];
                for p in &faces[1..] {
                    min.x = min.x.min(p.x);
                    min.y = min.y.min(p.y);
                    min.z = min.z.min(p.z);
                    max.x = max.x.max(p.x);
                    max.y = max.y.max(p.y);
                    max.z = max.z.max(p.z);
                }
                Aabb::new(min, Vector3::new(max.x - min.x, max.y - min.y, max.z - min.z))
            }
            Shape3D::WorldBoundaryShape { .. } => {
                // Infinite plane — return a very large AABB as a sentinel.
                let big = 1e6_f32;
                Aabb::new(
                    Vector3::new(-big, -big, -big),
                    Vector3::new(big * 2.0, big * 2.0, big * 2.0),
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
            Shape3D::CylinderShape { radius, height } => {
                let half_h = height / 2.0;
                if point.y.abs() > half_h {
                    return false;
                }
                let xz_dist_sq = point.x * point.x + point.z * point.z;
                xz_dist_sq <= radius * radius
            }
            Shape3D::ConvexPolygonShape { .. } => {
                // Approximate: check against bounding AABB.
                let aabb = self.bounding_aabb();
                point.x >= aabb.position.x
                    && point.x <= aabb.position.x + aabb.size.x
                    && point.y >= aabb.position.y
                    && point.y <= aabb.position.y + aabb.size.y
                    && point.z >= aabb.position.z
                    && point.z <= aabb.position.z + aabb.size.z
            }
            Shape3D::ConcavePolygonShape { .. } => {
                // Approximate: check against bounding AABB.
                let aabb = self.bounding_aabb();
                point.x >= aabb.position.x
                    && point.x <= aabb.position.x + aabb.size.x
                    && point.y >= aabb.position.y
                    && point.y <= aabb.position.y + aabb.size.y
                    && point.z >= aabb.position.z
                    && point.z <= aabb.position.z + aabb.size.z
            }
            Shape3D::WorldBoundaryShape { normal, distance } => {
                let dot = normal.x * point.x + normal.y * point.y + normal.z * point.z;
                dot <= distance
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sphere_aabb() {
        let shape = Shape3D::Sphere { radius: 2.0 };
        let aabb = shape.bounding_aabb();
        assert_eq!(aabb.position, Vector3::new(-2.0, -2.0, -2.0));
        assert_eq!(aabb.size, Vector3::new(4.0, 4.0, 4.0));
    }

    #[test]
    fn box_aabb() {
        let shape = Shape3D::BoxShape {
            half_extents: Vector3::new(1.0, 2.0, 3.0),
        };
        let aabb = shape.bounding_aabb();
        assert_eq!(aabb.position, Vector3::new(-1.0, -2.0, -3.0));
        assert_eq!(aabb.size, Vector3::new(2.0, 4.0, 6.0));
    }

    #[test]
    fn capsule_aabb() {
        let shape = Shape3D::CapsuleShape { radius: 1.0, height: 4.0 };
        let aabb = shape.bounding_aabb();
        assert_eq!(aabb.position, Vector3::new(-1.0, -2.0, -1.0));
        assert_eq!(aabb.size, Vector3::new(2.0, 4.0, 2.0));
    }

    #[test]
    fn sphere_contains_origin() {
        let shape = Shape3D::Sphere { radius: 1.0 };
        assert!(shape.contains_point(Vector3::ZERO));
    }

    #[test]
    fn sphere_excludes_outside() {
        let shape = Shape3D::Sphere { radius: 1.0 };
        assert!(!shape.contains_point(Vector3::new(2.0, 0.0, 0.0)));
    }

    #[test]
    fn box_contains_interior() {
        let shape = Shape3D::BoxShape {
            half_extents: Vector3::new(1.0, 1.0, 1.0),
        };
        assert!(shape.contains_point(Vector3::new(0.5, 0.5, 0.5)));
    }

    #[test]
    fn box_excludes_exterior() {
        let shape = Shape3D::BoxShape {
            half_extents: Vector3::new(1.0, 1.0, 1.0),
        };
        assert!(!shape.contains_point(Vector3::new(1.5, 0.0, 0.0)));
    }

    #[test]
    fn capsule_contains_center() {
        let shape = Shape3D::CapsuleShape { radius: 1.0, height: 4.0 };
        assert!(shape.contains_point(Vector3::ZERO));
    }

    #[test]
    fn capsule_contains_top_cap() {
        let shape = Shape3D::CapsuleShape { radius: 1.0, height: 4.0 };
        // Top hemisphere center at y=1.0 (half_height - radius = 2.0 - 1.0 = 1.0)
        assert!(shape.contains_point(Vector3::new(0.0, 1.5, 0.0)));
    }
}
