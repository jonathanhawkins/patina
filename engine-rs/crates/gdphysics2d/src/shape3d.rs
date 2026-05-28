//! 3D collision shape definitions.
//!
//! Provides 3D collision shapes for the physics engine: spheres, boxes,
//! capsules, and extruded 2D polygons (CollisionPolygon3D). Each shape can
//! compute its axis-aligned bounding box and test point containment.

use gdcore::math::{Vector2, Vector3};
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
    /// A 2D polygon extruded along the local Z axis to form a prism.
    ///
    /// Mirrors Godot's `CollisionPolygon3D`: the `vertices` describe a polygon
    /// in the local XY plane (centered at the origin), and `depth` is the total
    /// extrusion length along Z (the prism spans `[-depth/2, depth/2]` in Z).
    CollisionPolygon3D { vertices: Vec<Vector2>, depth: f32 },
}

impl Shape3D {
    /// Computes the axis-aligned bounding box for broad-phase collision.
    ///
    /// The returned `Aabb` is in local space (centered on the shape origin).
    pub fn bounding_aabb(&self) -> Aabb {
        match self {
            &Shape3D::Sphere { radius } => Aabb::new(
                Vector3::new(-radius, -radius, -radius),
                Vector3::new(radius * 2.0, radius * 2.0, radius * 2.0),
            ),
            &Shape3D::BoxShape { half_extents } => Aabb::new(
                Vector3::new(-half_extents.x, -half_extents.y, -half_extents.z),
                Vector3::new(
                    half_extents.x * 2.0,
                    half_extents.y * 2.0,
                    half_extents.z * 2.0,
                ),
            ),
            &Shape3D::CapsuleShape { radius, height } => {
                let half_height = height / 2.0;
                Aabb::new(
                    Vector3::new(-radius, -half_height, -radius),
                    Vector3::new(radius * 2.0, height, radius * 2.0),
                )
            }
            Shape3D::CollisionPolygon3D { vertices, depth } => {
                let depth = *depth;
                let half_depth = depth / 2.0;
                if vertices.is_empty() {
                    return Aabb::new(
                        Vector3::new(0.0, 0.0, -half_depth),
                        Vector3::new(0.0, 0.0, depth),
                    );
                }
                let mut min_x = vertices[0].x;
                let mut max_x = vertices[0].x;
                let mut min_y = vertices[0].y;
                let mut max_y = vertices[0].y;
                for v in &vertices[1..] {
                    min_x = min_x.min(v.x);
                    max_x = max_x.max(v.x);
                    min_y = min_y.min(v.y);
                    max_y = max_y.max(v.y);
                }
                Aabb::new(
                    Vector3::new(min_x, min_y, -half_depth),
                    Vector3::new(max_x - min_x, max_y - min_y, depth),
                )
            }
        }
    }

    /// Tests whether a point (in local shape space) is contained within the shape.
    pub fn contains_point(&self, point: Vector3) -> bool {
        match self {
            &Shape3D::Sphere { radius } => point.length_squared() <= radius * radius,
            &Shape3D::BoxShape { half_extents } => {
                point.x.abs() <= half_extents.x
                    && point.y.abs() <= half_extents.y
                    && point.z.abs() <= half_extents.z
            }
            &Shape3D::CapsuleShape { radius, height } => {
                let half_h = (height / 2.0 - radius).max(0.0);
                let clamped_y = point.y.clamp(-half_h, half_h);
                let closest = Vector3::new(0.0, clamped_y, 0.0);
                let diff = point - closest;
                diff.length_squared() <= radius * radius
            }
            Shape3D::CollisionPolygon3D { vertices, depth } => {
                let half_depth = depth / 2.0;
                if point.z < -half_depth || point.z > half_depth {
                    return false;
                }
                point_in_polygon_2d(vertices, Vector2::new(point.x, point.y))
            }
        }
    }
}

/// Even-odd rule point-in-polygon test for a 2D polygon.
///
/// Points exactly on an edge are treated as inside. Returns `false` for
/// polygons with fewer than three vertices.
fn point_in_polygon_2d(vertices: &[Vector2], point: Vector2) -> bool {
    if vertices.len() < 3 {
        return false;
    }
    let mut inside = false;
    let mut j = vertices.len() - 1;
    for i in 0..vertices.len() {
        let vi = vertices[i];
        let vj = vertices[j];
        // Exact boundary check: on the edge between vi and vj?
        if on_segment_2d(vi, vj, point) {
            return true;
        }
        let crosses_y = (vi.y > point.y) != (vj.y > point.y);
        if crosses_y {
            let x_intersect = (vj.x - vi.x) * (point.y - vi.y) / (vj.y - vi.y) + vi.x;
            if point.x < x_intersect {
                inside = !inside;
            }
        }
        j = i;
    }
    inside
}

/// Returns `true` if `p` lies on the closed segment `a..b` (with a small epsilon).
fn on_segment_2d(a: Vector2, b: Vector2, p: Vector2) -> bool {
    const EPS: f32 = 1e-5;
    let ab = Vector2::new(b.x - a.x, b.y - a.y);
    let ap = Vector2::new(p.x - a.x, p.y - a.y);
    let cross = ab.x * ap.y - ab.y * ap.x;
    if cross.abs() > EPS * (ab.x.abs() + ab.y.abs() + 1.0) {
        return false;
    }
    let dot = ab.x * ap.x + ab.y * ap.y;
    let len_sq = ab.x * ab.x + ab.y * ab.y;
    dot >= -EPS && dot <= len_sq + EPS
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

    fn unit_square_polygon() -> Vec<Vector2> {
        vec![
            Vector2::new(-1.0, -1.0),
            Vector2::new(1.0, -1.0),
            Vector2::new(1.0, 1.0),
            Vector2::new(-1.0, 1.0),
        ]
    }

    #[test]
    fn collision_polygon3d_bounding_aabb_square_prism() {
        let shape = Shape3D::CollisionPolygon3D {
            vertices: unit_square_polygon(),
            depth: 4.0,
        };
        let aabb = shape.bounding_aabb();
        assert!(approx_eq(aabb.position.x, -1.0));
        assert!(approx_eq(aabb.position.y, -1.0));
        assert!(approx_eq(aabb.position.z, -2.0));
        assert!(approx_eq(aabb.size.x, 2.0));
        assert!(approx_eq(aabb.size.y, 2.0));
        assert!(approx_eq(aabb.size.z, 4.0));
    }

    #[test]
    fn collision_polygon3d_bounding_aabb_triangle_prism() {
        let shape = Shape3D::CollisionPolygon3D {
            vertices: vec![
                Vector2::new(0.0, 0.0),
                Vector2::new(4.0, 0.0),
                Vector2::new(2.0, 3.0),
            ],
            depth: 2.0,
        };
        let aabb = shape.bounding_aabb();
        assert!(approx_eq(aabb.position.x, 0.0));
        assert!(approx_eq(aabb.position.y, 0.0));
        assert!(approx_eq(aabb.position.z, -1.0));
        assert!(approx_eq(aabb.size.x, 4.0));
        assert!(approx_eq(aabb.size.y, 3.0));
        assert!(approx_eq(aabb.size.z, 2.0));
    }

    #[test]
    fn collision_polygon3d_contains_interior_point() {
        let shape = Shape3D::CollisionPolygon3D {
            vertices: unit_square_polygon(),
            depth: 2.0,
        };
        assert!(shape.contains_point(Vector3::ZERO));
        assert!(shape.contains_point(Vector3::new(0.5, 0.5, 0.5)));
    }

    #[test]
    fn collision_polygon3d_rejects_outside_xy() {
        let shape = Shape3D::CollisionPolygon3D {
            vertices: unit_square_polygon(),
            depth: 2.0,
        };
        assert!(!shape.contains_point(Vector3::new(2.0, 0.0, 0.0)));
        assert!(!shape.contains_point(Vector3::new(0.0, 2.0, 0.0)));
    }

    #[test]
    fn collision_polygon3d_rejects_outside_depth() {
        let shape = Shape3D::CollisionPolygon3D {
            vertices: unit_square_polygon(),
            depth: 2.0,
        };
        assert!(!shape.contains_point(Vector3::new(0.0, 0.0, 2.0)));
        assert!(!shape.contains_point(Vector3::new(0.0, 0.0, -2.0)));
    }

    #[test]
    fn collision_polygon3d_edge_point_is_contained() {
        let shape = Shape3D::CollisionPolygon3D {
            vertices: unit_square_polygon(),
            depth: 2.0,
        };
        // On the +X edge.
        assert!(shape.contains_point(Vector3::new(1.0, 0.0, 0.0)));
        // Corner of prism.
        assert!(shape.contains_point(Vector3::new(1.0, 1.0, 1.0)));
    }

    #[test]
    fn collision_polygon3d_concave_polygon_point_containment() {
        // Non-convex "L" polygon:
        //  (0,0)-(2,0)-(2,1)-(1,1)-(1,2)-(0,2)
        let shape = Shape3D::CollisionPolygon3D {
            vertices: vec![
                Vector2::new(0.0, 0.0),
                Vector2::new(2.0, 0.0),
                Vector2::new(2.0, 1.0),
                Vector2::new(1.0, 1.0),
                Vector2::new(1.0, 2.0),
                Vector2::new(0.0, 2.0),
            ],
            depth: 1.0,
        };
        // Inside the horizontal arm.
        assert!(shape.contains_point(Vector3::new(1.5, 0.5, 0.0)));
        // Inside the vertical arm.
        assert!(shape.contains_point(Vector3::new(0.5, 1.5, 0.0)));
        // In the concave notch — outside the polygon.
        assert!(!shape.contains_point(Vector3::new(1.5, 1.5, 0.0)));
    }

    #[test]
    fn collision_polygon3d_empty_vertices_aabb_zero_xy() {
        let shape = Shape3D::CollisionPolygon3D {
            vertices: vec![],
            depth: 3.0,
        };
        let aabb = shape.bounding_aabb();
        assert!(approx_eq(aabb.size.x, 0.0));
        assert!(approx_eq(aabb.size.y, 0.0));
        assert!(approx_eq(aabb.size.z, 3.0));
    }
}
