//! 3D collision detection.

use gdcore::math::Vector3;

use crate::shape::Shape3D;

/// Result of a collision test between two 3D shapes.
#[derive(Debug, Clone, Copy)]
pub struct CollisionResult3D {
    /// Whether the shapes are overlapping.
    pub colliding: bool,
    /// The collision normal (from A to B).
    pub normal: Vector3,
    /// The penetration depth.
    pub depth: f32,
}

impl CollisionResult3D {
    /// No collision.
    pub const NONE: Self = Self {
        colliding: false,
        normal: Vector3::ZERO,
        depth: 0.0,
    };
}

/// Tests for collision between two spheres at the given positions.
pub fn test_sphere_sphere(
    pos_a: Vector3,
    radius_a: f32,
    pos_b: Vector3,
    radius_b: f32,
) -> CollisionResult3D {
    let diff = pos_b - pos_a;
    let dist_sq = diff.length_squared();
    let min_dist = radius_a + radius_b;

    if dist_sq >= min_dist * min_dist || dist_sq < f32::EPSILON {
        return CollisionResult3D::NONE;
    }

    let dist = dist_sq.sqrt();
    CollisionResult3D {
        colliding: true,
        normal: diff * (1.0 / dist),
        depth: min_dist - dist,
    }
}

/// Tests for collision between a sphere and an axis-aligned box.
pub fn test_sphere_box(
    sphere_pos: Vector3,
    radius: f32,
    box_pos: Vector3,
    half_extents: Vector3,
) -> CollisionResult3D {
    // Find the closest point on the box to the sphere center.
    let local = sphere_pos - box_pos;
    let clamped = Vector3::new(
        local.x.clamp(-half_extents.x, half_extents.x),
        local.y.clamp(-half_extents.y, half_extents.y),
        local.z.clamp(-half_extents.z, half_extents.z),
    );
    let diff = local - clamped;
    let dist_sq = diff.length_squared();

    if dist_sq < f32::EPSILON {
        // Sphere center is inside the box — find the axis of minimum penetration.
        // Normal must point from A (sphere) toward B (box center), i.e. opposite
        // to the push-out direction.
        let pen_x = half_extents.x - local.x.abs();
        let pen_y = half_extents.y - local.y.abs();
        let pen_z = half_extents.z - local.z.abs();

        if pen_x <= pen_y && pen_x <= pen_z {
            let sign = if local.x >= 0.0 { -1.0 } else { 1.0 };
            return CollisionResult3D {
                colliding: true,
                normal: Vector3::new(sign, 0.0, 0.0),
                depth: pen_x + radius,
            };
        } else if pen_y <= pen_z {
            let sign = if local.y >= 0.0 { -1.0 } else { 1.0 };
            return CollisionResult3D {
                colliding: true,
                normal: Vector3::new(0.0, sign, 0.0),
                depth: pen_y + radius,
            };
        } else {
            let sign = if local.z >= 0.0 { -1.0 } else { 1.0 };
            return CollisionResult3D {
                colliding: true,
                normal: Vector3::new(0.0, 0.0, sign),
                depth: pen_z + radius,
            };
        }
    }

    if dist_sq >= radius * radius {
        return CollisionResult3D::NONE;
    }

    let dist = dist_sq.sqrt();
    CollisionResult3D {
        colliding: true,
        normal: diff * (1.0 / dist),
        depth: radius - dist,
    }
}

/// Tests for collision between two axis-aligned boxes.
pub fn test_box_box(
    pos_a: Vector3,
    half_a: Vector3,
    pos_b: Vector3,
    half_b: Vector3,
) -> CollisionResult3D {
    let diff = pos_b - pos_a;
    let overlap_x = (half_a.x + half_b.x) - diff.x.abs();
    let overlap_y = (half_a.y + half_b.y) - diff.y.abs();
    let overlap_z = (half_a.z + half_b.z) - diff.z.abs();

    if overlap_x <= 0.0 || overlap_y <= 0.0 || overlap_z <= 0.0 {
        return CollisionResult3D::NONE;
    }

    // Resolve along the axis with minimum overlap (least penetration).
    let (depth, mut normal) = if overlap_x <= overlap_y && overlap_x <= overlap_z {
        (overlap_x, Vector3::new(1.0, 0.0, 0.0))
    } else if overlap_y <= overlap_z {
        (overlap_y, Vector3::new(0.0, 1.0, 0.0))
    } else {
        (overlap_z, Vector3::new(0.0, 0.0, 1.0))
    };

    // Normal should point from A to B.
    if diff.x < 0.0 && normal.x > 0.0 {
        normal.x = -1.0;
    }
    if diff.y < 0.0 && normal.y > 0.0 {
        normal.y = -1.0;
    }
    if diff.z < 0.0 && normal.z > 0.0 {
        normal.z = -1.0;
    }

    CollisionResult3D {
        colliding: true,
        normal,
        depth,
    }
}

/// Tests for collision between two shapes at the given positions.
pub fn test_collision(
    pos_a: Vector3,
    shape_a: &Shape3D,
    pos_b: Vector3,
    shape_b: &Shape3D,
) -> CollisionResult3D {
    match (shape_a, shape_b) {
        (Shape3D::Sphere { radius: ra }, Shape3D::Sphere { radius: rb }) => {
            test_sphere_sphere(pos_a, *ra, pos_b, *rb)
        }
        (Shape3D::Sphere { radius }, Shape3D::BoxShape { half_extents }) => {
            let result = test_sphere_box(pos_a, *radius, pos_b, *half_extents);
            // test_sphere_box returns normal from box toward sphere (B→A).
            // test_collision convention is A→B, so flip.
            CollisionResult3D {
                normal: result.normal * -1.0,
                ..result
            }
        }
        (Shape3D::BoxShape { half_extents }, Shape3D::Sphere { radius }) => {
            // Call with sphere=B, box=A. Result normal points from A(box)→B(sphere).
            // That matches the A→B convention, so no flip needed.
            test_sphere_box(pos_b, *radius, pos_a, *half_extents)
        }
        (Shape3D::BoxShape { half_extents: ha }, Shape3D::BoxShape { half_extents: hb }) => {
            test_box_box(pos_a, *ha, pos_b, *hb)
        }
        _ => CollisionResult3D::NONE,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sphere_sphere_overlap() {
        let result = test_sphere_sphere(Vector3::ZERO, 1.0, Vector3::new(1.5, 0.0, 0.0), 1.0);
        assert!(result.colliding);
        assert!((result.depth - 0.5).abs() < 1e-5);
        assert!((result.normal.x - 1.0).abs() < 1e-5);
    }

    #[test]
    fn sphere_sphere_no_overlap() {
        let result = test_sphere_sphere(Vector3::ZERO, 1.0, Vector3::new(3.0, 0.0, 0.0), 1.0);
        assert!(!result.colliding);
    }

    #[test]
    fn sphere_sphere_touching() {
        let result = test_sphere_sphere(Vector3::ZERO, 1.0, Vector3::new(2.0, 0.0, 0.0), 1.0);
        assert!(!result.colliding, "exactly touching should not collide");
    }

    #[test]
    fn test_collision_dispatch() {
        let a = Shape3D::Sphere { radius: 1.0 };
        let b = Shape3D::Sphere { radius: 1.0 };
        let result = test_collision(Vector3::ZERO, &a, Vector3::new(1.0, 0.0, 0.0), &b);
        assert!(result.colliding);
    }

    #[test]
    fn box_sphere_overlap_at_center() {
        // Sphere centered inside box should collide.
        let a = Shape3D::BoxShape {
            half_extents: Vector3::new(1.0, 1.0, 1.0),
        };
        let b = Shape3D::Sphere { radius: 1.0 };
        let result = test_collision(Vector3::ZERO, &a, Vector3::ZERO, &b);
        assert!(result.colliding, "sphere inside box should collide");
        assert!(result.depth > 0.0);
    }

    #[test]
    fn box_box_overlap() {
        let a = Shape3D::BoxShape {
            half_extents: Vector3::new(1.0, 1.0, 1.0),
        };
        let b = Shape3D::BoxShape {
            half_extents: Vector3::new(1.0, 1.0, 1.0),
        };
        let result = test_collision(Vector3::ZERO, &a, Vector3::new(1.5, 0.0, 0.0), &b);
        assert!(result.colliding);
        assert!((result.depth - 0.5).abs() < 1e-5);
    }

    #[test]
    fn box_box_no_overlap() {
        let a = Shape3D::BoxShape {
            half_extents: Vector3::new(1.0, 1.0, 1.0),
        };
        let b = Shape3D::BoxShape {
            half_extents: Vector3::new(1.0, 1.0, 1.0),
        };
        let result = test_collision(Vector3::ZERO, &a, Vector3::new(3.0, 0.0, 0.0), &b);
        assert!(!result.colliding);
    }

    #[test]
    fn sphere_inside_box_detects_collision() {
        let result = test_sphere_box(
            Vector3::new(0.5, 0.0, 0.0),
            0.5,
            Vector3::ZERO,
            Vector3::new(2.0, 2.0, 2.0),
        );
        assert!(result.colliding, "sphere inside box should collide");
        assert!(result.depth > 0.0);
    }
}
