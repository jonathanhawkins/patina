//! Collision detection and resolution.
//!
//! Provides narrow-phase collision tests between pairs of 2D shapes,
//! returning contact information (normal, depth, contact point) when
//! shapes overlap. Also provides body separation utilities.

use gdcore::math::{Transform2D, Vector2};

use crate::body::PhysicsBody2D;
use crate::shape::Shape2D;

/// The result of a narrow-phase collision test between two shapes.
#[derive(Debug, Clone, Copy)]
pub struct CollisionResult {
    /// Whether the shapes are overlapping.
    pub colliding: bool,
    /// The collision normal pointing from shape B toward shape A.
    pub normal: Vector2,
    /// The penetration depth along the normal.
    pub depth: f32,
    /// The approximate contact point in world space.
    pub point: Vector2,
}

impl CollisionResult {
    /// A "no collision" sentinel value.
    pub const NONE: Self = Self {
        colliding: false,
        normal: Vector2::ZERO,
        depth: 0.0,
        point: Vector2::ZERO,
    };
}

/// Tests for collision between two shapes, each positioned by a transform.
///
/// Currently supports circle-circle, circle-rect (AABB), and rect-rect (AABB)
/// pairs. Unsupported shape pairs return `None`.
pub fn test_collision(
    shape_a: &Shape2D,
    transform_a: &Transform2D,
    shape_b: &Shape2D,
    transform_b: &Transform2D,
) -> Option<CollisionResult> {
    match (shape_a, shape_b) {
        (Shape2D::Circle { radius: ra }, Shape2D::Circle { radius: rb }) => {
            Some(circle_circle(transform_a.origin, *ra, transform_b.origin, *rb))
        }
        (Shape2D::Circle { radius }, Shape2D::Rectangle { half_extents }) => {
            // circle_rect returns normal pointing from rect toward circle (B→A).
            // Our convention: normal from A to B, so flip it.
            let mut result =
                circle_rect(transform_a.origin, *radius, transform_b.origin, *half_extents);
            result.normal = -result.normal;
            Some(result)
        }
        (Shape2D::Rectangle { half_extents }, Shape2D::Circle { radius }) => {
            // circle_rect returns normal pointing from rect toward circle.
            // Here rect=A, circle=B, so normal points A→B — already correct.
            Some(circle_rect(transform_b.origin, *radius, transform_a.origin, *half_extents))
        }
        (
            Shape2D::Rectangle {
                half_extents: he_a,
            },
            Shape2D::Rectangle {
                half_extents: he_b,
            },
        ) => Some(rect_rect(transform_a.origin, *he_a, transform_b.origin, *he_b)),
        _ => None, // Unsupported shape pair
    }
}

/// Circle vs circle collision test.
fn circle_circle(pos_a: Vector2, ra: f32, pos_b: Vector2, rb: f32) -> CollisionResult {
    let diff = pos_b - pos_a;
    let dist_sq = diff.length_squared();
    let sum_r = ra + rb;

    if dist_sq > sum_r * sum_r {
        return CollisionResult::NONE;
    }

    let dist = dist_sq.sqrt();
    if dist < 1e-10 {
        // Circles are coincident — pick an arbitrary normal
        return CollisionResult {
            colliding: true,
            normal: Vector2::new(1.0, 0.0),
            depth: sum_r,
            point: pos_a,
        };
    }

    let normal = diff * (1.0 / dist);
    let depth = sum_r - dist;
    let point = pos_a + normal * (ra - depth * 0.5);

    CollisionResult {
        colliding: true,
        normal,
        depth,
        point,
    }
}

/// Circle vs axis-aligned rectangle collision test.
fn circle_rect(
    circle_pos: Vector2,
    radius: f32,
    rect_pos: Vector2,
    half_extents: Vector2,
) -> CollisionResult {
    // Find the closest point on the AABB to the circle center
    let local = circle_pos - rect_pos;
    let clamped = Vector2::new(
        local.x.clamp(-half_extents.x, half_extents.x),
        local.y.clamp(-half_extents.y, half_extents.y),
    );
    let closest_world = rect_pos + clamped;
    let diff = circle_pos - closest_world;
    let dist_sq = diff.length_squared();

    if dist_sq > radius * radius {
        return CollisionResult::NONE;
    }

    let dist = dist_sq.sqrt();
    if dist < 1e-10 {
        // Circle center is inside the rectangle — find the closest edge
        let dx = half_extents.x - local.x.abs();
        let dy = half_extents.y - local.y.abs();
        if dx < dy {
            let sign = if local.x >= 0.0 { 1.0 } else { -1.0 };
            return CollisionResult {
                colliding: true,
                normal: Vector2::new(sign, 0.0),
                depth: dx + radius,
                point: Vector2::new(rect_pos.x + half_extents.x * sign, circle_pos.y),
            };
        } else {
            let sign = if local.y >= 0.0 { 1.0 } else { -1.0 };
            return CollisionResult {
                colliding: true,
                normal: Vector2::new(0.0, sign),
                depth: dy + radius,
                point: Vector2::new(circle_pos.x, rect_pos.y + half_extents.y * sign),
            };
        }
    }

    let normal = diff * (1.0 / dist);
    let depth = radius - dist;

    CollisionResult {
        colliding: true,
        normal,
        depth,
        point: closest_world,
    }
}

/// Axis-aligned rectangle vs rectangle collision test.
fn rect_rect(
    pos_a: Vector2,
    he_a: Vector2,
    pos_b: Vector2,
    he_b: Vector2,
) -> CollisionResult {
    let diff = pos_b - pos_a;
    let overlap_x = he_a.x + he_b.x - diff.x.abs();
    let overlap_y = he_a.y + he_b.y - diff.y.abs();

    if overlap_x <= 0.0 || overlap_y <= 0.0 {
        return CollisionResult::NONE;
    }

    // Choose the axis with the smallest overlap for separation
    if overlap_x < overlap_y {
        let sign = if diff.x >= 0.0 { 1.0 } else { -1.0 };
        CollisionResult {
            colliding: true,
            normal: Vector2::new(sign, 0.0),
            depth: overlap_x,
            point: Vector2::new(
                pos_a.x + he_a.x * sign,
                pos_a.y + diff.y * 0.5,
            ),
        }
    } else {
        let sign = if diff.y >= 0.0 { 1.0 } else { -1.0 };
        CollisionResult {
            colliding: true,
            normal: Vector2::new(0.0, sign),
            depth: overlap_y,
            point: Vector2::new(
                pos_a.x + diff.x * 0.5,
                pos_a.y + he_a.y * sign,
            ),
        }
    }
}

/// Separates two bodies based on a collision result.
///
/// Distributes the separation proportionally by inverse mass. Static bodies
/// do not move.
pub fn separate_bodies(a: &mut PhysicsBody2D, b: &mut PhysicsBody2D, result: &CollisionResult) {
    if !result.colliding || result.depth <= 0.0 {
        return;
    }

    let inv_a = a.inverse_mass();
    let inv_b = b.inverse_mass();
    let total_inv = inv_a + inv_b;

    if total_inv <= 0.0 {
        return; // Both immovable
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

    const EPSILON: f32 = 1e-4;

    fn approx_eq(a: f32, b: f32) -> bool {
        (a - b).abs() < EPSILON
    }

    #[test]
    fn circle_circle_overlapping() {
        let result = test_collision(
            &Shape2D::Circle { radius: 5.0 },
            &Transform2D::translated(Vector2::new(0.0, 0.0)),
            &Shape2D::Circle { radius: 5.0 },
            &Transform2D::translated(Vector2::new(8.0, 0.0)),
        )
        .unwrap();
        assert!(result.colliding);
        assert!(approx_eq(result.depth, 2.0));
        assert!(approx_eq(result.normal.x, 1.0));
        assert!(approx_eq(result.normal.y, 0.0));
    }

    #[test]
    fn circle_circle_touching() {
        let result = test_collision(
            &Shape2D::Circle { radius: 5.0 },
            &Transform2D::translated(Vector2::new(0.0, 0.0)),
            &Shape2D::Circle { radius: 5.0 },
            &Transform2D::translated(Vector2::new(10.0, 0.0)),
        )
        .unwrap();
        // Exactly touching: depth == 0, still "colliding" (<=)
        assert!(result.colliding);
        assert!(approx_eq(result.depth, 0.0));
    }

    #[test]
    fn circle_circle_separated() {
        let result = test_collision(
            &Shape2D::Circle { radius: 5.0 },
            &Transform2D::translated(Vector2::new(0.0, 0.0)),
            &Shape2D::Circle { radius: 5.0 },
            &Transform2D::translated(Vector2::new(11.0, 0.0)),
        )
        .unwrap();
        assert!(!result.colliding);
    }

    #[test]
    fn circle_rect_collision() {
        let result = test_collision(
            &Shape2D::Circle { radius: 2.0 },
            &Transform2D::translated(Vector2::new(4.0, 0.0)),
            &Shape2D::Rectangle {
                half_extents: Vector2::new(3.0, 3.0),
            },
            &Transform2D::translated(Vector2::new(0.0, 0.0)),
        )
        .unwrap();
        assert!(result.colliding);
        // Circle center at 4, rect edge at 3 => gap = 1, circle radius = 2 => depth = 1
        assert!(approx_eq(result.depth, 1.0));
        // Normal points from A (circle) toward B (rect), so negative x
        assert!(approx_eq(result.normal.x, -1.0));
    }

    #[test]
    fn circle_rect_no_collision() {
        let result = test_collision(
            &Shape2D::Circle { radius: 1.0 },
            &Transform2D::translated(Vector2::new(10.0, 0.0)),
            &Shape2D::Rectangle {
                half_extents: Vector2::new(3.0, 3.0),
            },
            &Transform2D::translated(Vector2::new(0.0, 0.0)),
        )
        .unwrap();
        assert!(!result.colliding);
    }

    #[test]
    fn rect_rect_overlapping() {
        let result = test_collision(
            &Shape2D::Rectangle {
                half_extents: Vector2::new(3.0, 3.0),
            },
            &Transform2D::translated(Vector2::new(0.0, 0.0)),
            &Shape2D::Rectangle {
                half_extents: Vector2::new(3.0, 3.0),
            },
            &Transform2D::translated(Vector2::new(4.0, 0.0)),
        )
        .unwrap();
        assert!(result.colliding);
        assert!(approx_eq(result.depth, 2.0));
    }

    #[test]
    fn rect_rect_separated() {
        let result = test_collision(
            &Shape2D::Rectangle {
                half_extents: Vector2::new(2.0, 2.0),
            },
            &Transform2D::translated(Vector2::new(0.0, 0.0)),
            &Shape2D::Rectangle {
                half_extents: Vector2::new(2.0, 2.0),
            },
            &Transform2D::translated(Vector2::new(10.0, 0.0)),
        )
        .unwrap();
        assert!(!result.colliding);
    }

    #[test]
    fn unsupported_pair_returns_none() {
        let result = test_collision(
            &Shape2D::Segment {
                a: Vector2::ZERO,
                b: Vector2::new(1.0, 0.0),
            },
            &Transform2D::IDENTITY,
            &Shape2D::Circle { radius: 1.0 },
            &Transform2D::IDENTITY,
        );
        assert!(result.is_none());
    }
}
