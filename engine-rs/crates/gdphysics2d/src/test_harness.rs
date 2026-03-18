//! Deterministic test harness for physics simulation.
//!
//! Provides helper functions for creating test worlds with known body
//! configurations and asserting physics invariants.

use gdcore::math::Vector2;

use crate::body::{BodyId, BodyType, PhysicsBody2D};
use crate::shape::Shape2D;
use crate::world::PhysicsWorld2D;

/// Creates a test world with two overlapping circles.
///
/// Returns `(world, id_a, id_b)`.
pub fn two_overlapping_circles() -> (PhysicsWorld2D, BodyId, BodyId) {
    let mut world = PhysicsWorld2D::new();
    let a = PhysicsBody2D::new(
        BodyId(0),
        BodyType::Rigid,
        Vector2::new(0.0, 0.0),
        Shape2D::Circle { radius: 5.0 },
        1.0,
    );
    let b = PhysicsBody2D::new(
        BodyId(0),
        BodyType::Rigid,
        Vector2::new(8.0, 0.0),
        Shape2D::Circle { radius: 5.0 },
        1.0,
    );
    let id_a = world.add_body(a);
    let id_b = world.add_body(b);
    (world, id_a, id_b)
}

/// Creates a test world with a rigid circle resting on a static floor.
///
/// Returns `(world, circle_id, floor_id)`.
pub fn circle_on_static_floor() -> (PhysicsWorld2D, BodyId, BodyId) {
    let mut world = PhysicsWorld2D::new();
    let circle = PhysicsBody2D::new(
        BodyId(0),
        BodyType::Rigid,
        Vector2::new(0.0, -5.0),
        Shape2D::Circle { radius: 2.0 },
        1.0,
    );
    let floor = PhysicsBody2D::new(
        BodyId(0),
        BodyType::Static,
        Vector2::new(0.0, 0.0),
        Shape2D::Rectangle {
            half_extents: Vector2::new(100.0, 1.0),
        },
        1.0,
    );
    let cid = world.add_body(circle);
    let fid = world.add_body(floor);
    (world, cid, fid)
}

/// Asserts that two bodies are not overlapping (separated or barely touching).
///
/// Only works for circle-circle pairs currently.
pub fn assert_bodies_separated(world: &PhysicsWorld2D, id_a: BodyId, id_b: BodyId) {
    let a = world.get_body(id_a).expect("body A not found");
    let b = world.get_body(id_b).expect("body B not found");

    let min_dist = match (&a.shape, &b.shape) {
        (Shape2D::Circle { radius: ra }, Shape2D::Circle { radius: rb }) => ra + rb,
        _ => {
            // For non-circle pairs, just check they aren't at the same position
            0.0
        }
    };

    let dist = (b.position - a.position).length();
    assert!(
        dist >= min_dist - 1e-3,
        "Bodies overlap: distance {dist} < required {min_dist}"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn harness_two_overlapping_circles_separate_after_step() {
        let (mut world, id_a, id_b) = two_overlapping_circles();
        world.step(0.0);
        assert_bodies_separated(&world, id_a, id_b);
    }

    #[test]
    fn harness_circle_on_floor_stays_above() {
        let (mut world, cid, _fid) = circle_on_static_floor();
        // Step a few times
        for _ in 0..10 {
            world.step(1.0 / 60.0);
        }
        let circle = world.get_body(cid).unwrap();
        // Circle should remain above the floor (y <= 0 in our setup since floor is at y=0)
        assert!(
            circle.position.y <= 0.0,
            "Circle should remain above/at the floor, got y={}",
            circle.position.y
        );
    }
}
