//! 3D collision detection and resolution.
//!
//! Provides narrow-phase collision tests between pairs of 3D shapes,
//! returning contact information (normal, depth, contact point) when
//! shapes overlap. Also provides body separation utilities.

use gdcore::math::Vector3;

use crate::body3d::PhysicsBody3D;
use crate::shape3d::Shape3D;

/// The result of a narrow-phase collision test between two 3D shapes.
#[derive(Debug, Clone, Copy)]
pub struct CollisionResult3D {
    /// Whether the shapes are overlapping.
    pub colliding: bool,
    /// The collision normal pointing from shape B toward shape A.
    pub normal: Vector3,
    /// The penetration depth along the normal.
    pub depth: f32,
    /// The approximate contact point in world space.
    pub point: Vector3,
}

impl CollisionResult3D {
    /// A "no collision" sentinel value.
    pub const NONE: Self = Self {
        colliding: false,
        normal: Vector3::ZERO,
        depth: 0.0,
        point: Vector3::ZERO,
    };
}

/// Tests for collision between two 3D shapes at given positions.
///
/// Supports sphere-sphere, sphere-box (AABB), and box-box (AABB) pairs.
/// Unsupported shape pairs return `None`.
pub fn test_collision_3d(
    shape_a: &Shape3D,
    pos_a: Vector3,
    shape_b: &Shape3D,
    pos_b: Vector3,
) -> Option<CollisionResult3D> {
    match (shape_a, shape_b) {
        (Shape3D::Sphere { radius: ra }, Shape3D::Sphere { radius: rb }) => {
            Some(sphere_sphere(pos_a, *ra, pos_b, *rb))
        }
        (Shape3D::Sphere { radius }, Shape3D::BoxShape { half_extents }) => {
            let mut result = sphere_box(pos_a, *radius, pos_b, *half_extents);
            // sphere_box returns normal pointing from box toward sphere.
            // When sphere is A, we flip to maintain B→A convention.
            result.normal = -result.normal;
            Some(result)
        }
        (Shape3D::BoxShape { half_extents }, Shape3D::Sphere { radius }) => {
            Some(sphere_box(pos_b, *radius, pos_a, *half_extents))
        }
        (
            Shape3D::BoxShape {
                half_extents: he_a,
            },
            Shape3D::BoxShape {
                half_extents: he_b,
            },
        ) => Some(box_box(pos_a, *he_a, pos_b, *he_b)),
        _ => None, // Capsule pairs not yet implemented
    }
}

/// Sphere vs sphere collision test.
fn sphere_sphere(pos_a: Vector3, ra: f32, pos_b: Vector3, rb: f32) -> CollisionResult3D {
    let diff = pos_b - pos_a;
    let dist_sq = diff.length_squared();
    let sum_r = ra + rb;

    if dist_sq > sum_r * sum_r {
        return CollisionResult3D::NONE;
    }

    let dist = dist_sq.sqrt();
    if dist < 1e-10 {
        return CollisionResult3D {
            colliding: true,
            normal: Vector3::new(1.0, 0.0, 0.0),
            depth: sum_r,
            point: pos_a,
        };
    }

    let normal = diff * (1.0 / dist);
    let depth = sum_r - dist;
    let point = pos_a + normal * (ra - depth * 0.5);

    CollisionResult3D {
        colliding: true,
        normal,
        depth,
        point,
    }
}

/// Sphere vs axis-aligned box collision test.
fn sphere_box(
    sphere_pos: Vector3,
    radius: f32,
    box_pos: Vector3,
    half_extents: Vector3,
) -> CollisionResult3D {
    let local = sphere_pos - box_pos;
    let clamped = Vector3::new(
        local.x.clamp(-half_extents.x, half_extents.x),
        local.y.clamp(-half_extents.y, half_extents.y),
        local.z.clamp(-half_extents.z, half_extents.z),
    );
    let closest_world = box_pos + clamped;
    let diff = sphere_pos - closest_world;
    let dist_sq = diff.length_squared();

    if dist_sq > radius * radius {
        return CollisionResult3D::NONE;
    }

    let dist = dist_sq.sqrt();
    if dist < 1e-10 {
        // Sphere center is inside the box — find the closest face
        let dx = half_extents.x - local.x.abs();
        let dy = half_extents.y - local.y.abs();
        let dz = half_extents.z - local.z.abs();

        if dx <= dy && dx <= dz {
            let sign = if local.x >= 0.0 { 1.0 } else { -1.0 };
            return CollisionResult3D {
                colliding: true,
                normal: Vector3::new(sign, 0.0, 0.0),
                depth: dx + radius,
                point: Vector3::new(box_pos.x + half_extents.x * sign, sphere_pos.y, sphere_pos.z),
            };
        } else if dy <= dz {
            let sign = if local.y >= 0.0 { 1.0 } else { -1.0 };
            return CollisionResult3D {
                colliding: true,
                normal: Vector3::new(0.0, sign, 0.0),
                depth: dy + radius,
                point: Vector3::new(sphere_pos.x, box_pos.y + half_extents.y * sign, sphere_pos.z),
            };
        } else {
            let sign = if local.z >= 0.0 { 1.0 } else { -1.0 };
            return CollisionResult3D {
                colliding: true,
                normal: Vector3::new(0.0, 0.0, sign),
                depth: dz + radius,
                point: Vector3::new(sphere_pos.x, sphere_pos.y, box_pos.z + half_extents.z * sign),
            };
        }
    }

    let normal = diff * (1.0 / dist);
    let depth = radius - dist;

    CollisionResult3D {
        colliding: true,
        normal,
        depth,
        point: closest_world,
    }
}

/// Axis-aligned box vs box collision test.
fn box_box(
    pos_a: Vector3,
    he_a: Vector3,
    pos_b: Vector3,
    he_b: Vector3,
) -> CollisionResult3D {
    let diff = pos_b - pos_a;
    let overlap_x = he_a.x + he_b.x - diff.x.abs();
    let overlap_y = he_a.y + he_b.y - diff.y.abs();
    let overlap_z = he_a.z + he_b.z - diff.z.abs();

    if overlap_x <= 0.0 || overlap_y <= 0.0 || overlap_z <= 0.0 {
        return CollisionResult3D::NONE;
    }

    // Choose the axis with the smallest overlap for separation
    if overlap_x <= overlap_y && overlap_x <= overlap_z {
        let sign = if diff.x >= 0.0 { 1.0 } else { -1.0 };
        CollisionResult3D {
            colliding: true,
            normal: Vector3::new(sign, 0.0, 0.0),
            depth: overlap_x,
            point: Vector3::new(
                pos_a.x + he_a.x * sign,
                pos_a.y + diff.y * 0.5,
                pos_a.z + diff.z * 0.5,
            ),
        }
    } else if overlap_y <= overlap_z {
        let sign = if diff.y >= 0.0 { 1.0 } else { -1.0 };
        CollisionResult3D {
            colliding: true,
            normal: Vector3::new(0.0, sign, 0.0),
            depth: overlap_y,
            point: Vector3::new(
                pos_a.x + diff.x * 0.5,
                pos_a.y + he_a.y * sign,
                pos_a.z + diff.z * 0.5,
            ),
        }
    } else {
        let sign = if diff.z >= 0.0 { 1.0 } else { -1.0 };
        CollisionResult3D {
            colliding: true,
            normal: Vector3::new(0.0, 0.0, sign),
            depth: overlap_z,
            point: Vector3::new(
                pos_a.x + diff.x * 0.5,
                pos_a.y + diff.y * 0.5,
                pos_a.z + he_a.z * sign,
            ),
        }
    }
}

/// Separates two 3D bodies based on a collision result.
///
/// Distributes the separation proportionally by inverse mass. Static bodies
/// do not move.
pub fn separate_bodies_3d(
    a: &mut PhysicsBody3D,
    b: &mut PhysicsBody3D,
    result: &CollisionResult3D,
) {
    if !result.colliding || result.depth <= 0.0 {
        return;
    }

    let inv_a = a.inverse_mass();
    let inv_b = b.inverse_mass();
    let total_inv = inv_a + inv_b;

    if total_inv <= 0.0 {
        return;
    }

    let separation = result.normal * result.depth;
    a.position = a.position - separation * (inv_a / total_inv);
    b.position = b.position + separation * (inv_b / total_inv);

    // Simple velocity resolution with restitution
    let relative_vel = b.linear_velocity - a.linear_velocity;
    let vel_along_normal = relative_vel.dot(result.normal);

    if vel_along_normal > 0.0 {
        return; // Bodies are already separating
    }

    let restitution = a.bounce.min(b.bounce);
    let impulse_magnitude = -(1.0 + restitution) * vel_along_normal / total_inv;
    let impulse = result.normal * impulse_magnitude;

    a.linear_velocity = a.linear_velocity - impulse * inv_a;
    b.linear_velocity = b.linear_velocity + impulse * inv_b;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::body3d::{BodyId3D, BodyType3D};

    const EPSILON: f32 = 1e-4;

    fn approx_eq(a: f32, b: f32) -> bool {
        (a - b).abs() < EPSILON
    }

    #[test]
    fn sphere_sphere_overlapping() {
        let result = test_collision_3d(
            &Shape3D::Sphere { radius: 5.0 },
            Vector3::new(0.0, 0.0, 0.0),
            &Shape3D::Sphere { radius: 5.0 },
            Vector3::new(8.0, 0.0, 0.0),
        )
        .unwrap();
        assert!(result.colliding);
        assert!(approx_eq(result.depth, 2.0));
        assert!(approx_eq(result.normal.x, 1.0));
    }

    #[test]
    fn sphere_sphere_separated() {
        let result = test_collision_3d(
            &Shape3D::Sphere { radius: 5.0 },
            Vector3::ZERO,
            &Shape3D::Sphere { radius: 5.0 },
            Vector3::new(11.0, 0.0, 0.0),
        )
        .unwrap();
        assert!(!result.colliding);
    }

    #[test]
    fn sphere_sphere_touching() {
        let result = test_collision_3d(
            &Shape3D::Sphere { radius: 5.0 },
            Vector3::ZERO,
            &Shape3D::Sphere { radius: 5.0 },
            Vector3::new(10.0, 0.0, 0.0),
        )
        .unwrap();
        assert!(result.colliding);
        assert!(approx_eq(result.depth, 0.0));
    }

    #[test]
    fn sphere_box_collision() {
        let result = test_collision_3d(
            &Shape3D::Sphere { radius: 2.0 },
            Vector3::new(4.0, 0.0, 0.0),
            &Shape3D::BoxShape {
                half_extents: Vector3::new(3.0, 3.0, 3.0),
            },
            Vector3::ZERO,
        )
        .unwrap();
        assert!(result.colliding);
        assert!(approx_eq(result.depth, 1.0));
    }

    #[test]
    fn sphere_box_no_collision() {
        let result = test_collision_3d(
            &Shape3D::Sphere { radius: 1.0 },
            Vector3::new(10.0, 0.0, 0.0),
            &Shape3D::BoxShape {
                half_extents: Vector3::new(3.0, 3.0, 3.0),
            },
            Vector3::ZERO,
        )
        .unwrap();
        assert!(!result.colliding);
    }

    #[test]
    fn box_box_overlapping() {
        let result = test_collision_3d(
            &Shape3D::BoxShape {
                half_extents: Vector3::new(3.0, 3.0, 3.0),
            },
            Vector3::ZERO,
            &Shape3D::BoxShape {
                half_extents: Vector3::new(3.0, 3.0, 3.0),
            },
            Vector3::new(4.0, 0.0, 0.0),
        )
        .unwrap();
        assert!(result.colliding);
        assert!(approx_eq(result.depth, 2.0));
    }

    #[test]
    fn box_box_separated() {
        let result = test_collision_3d(
            &Shape3D::BoxShape {
                half_extents: Vector3::new(2.0, 2.0, 2.0),
            },
            Vector3::ZERO,
            &Shape3D::BoxShape {
                half_extents: Vector3::new(2.0, 2.0, 2.0),
            },
            Vector3::new(10.0, 0.0, 0.0),
        )
        .unwrap();
        assert!(!result.colliding);
    }

    #[test]
    fn unsupported_pair_returns_none() {
        let result = test_collision_3d(
            &Shape3D::CapsuleShape {
                radius: 1.0,
                height: 4.0,
            },
            Vector3::ZERO,
            &Shape3D::Sphere { radius: 1.0 },
            Vector3::new(1.0, 0.0, 0.0),
        );
        assert!(result.is_none());
    }

    #[test]
    fn separate_bodies_pushes_apart() {
        let mut a = PhysicsBody3D::new(
            BodyId3D(1),
            BodyType3D::Rigid,
            Vector3::ZERO,
            Shape3D::Sphere { radius: 5.0 },
            1.0,
        );
        let mut b = PhysicsBody3D::new(
            BodyId3D(2),
            BodyType3D::Rigid,
            Vector3::new(8.0, 0.0, 0.0),
            Shape3D::Sphere { radius: 5.0 },
            1.0,
        );
        let result = test_collision_3d(&a.shape, a.position, &b.shape, b.position).unwrap();
        separate_bodies_3d(&mut a, &mut b, &result);
        let dist = (b.position - a.position).length();
        assert!(
            dist >= 10.0 - EPSILON,
            "Bodies should be separated: dist = {dist}"
        );
    }

    #[test]
    fn static_body_not_moved_by_separation() {
        let mut a = PhysicsBody3D::new(
            BodyId3D(1),
            BodyType3D::Static,
            Vector3::ZERO,
            Shape3D::BoxShape {
                half_extents: Vector3::new(5.0, 5.0, 5.0),
            },
            1.0,
        );
        let mut b = PhysicsBody3D::new(
            BodyId3D(2),
            BodyType3D::Rigid,
            Vector3::new(6.0, 0.0, 0.0),
            Shape3D::Sphere { radius: 2.0 },
            1.0,
        );
        let result = test_collision_3d(&a.shape, a.position, &b.shape, b.position).unwrap();
        separate_bodies_3d(&mut a, &mut b, &result);
        assert_eq!(a.position, Vector3::ZERO, "Static body must not move");
        assert!(b.position.x > 6.0, "Rigid body should be pushed away");
    }
}
