//! Shapes exactly TOUCHING collide — Godot's SAT separates only on a strict gap
//! (`godot_collision_solver_2d_sat.cpp` `test_axis`), and `test_collision`
//! already reports depth 0 as colliding. The world's pair loop required
//! `depth > 0`, so a body authored resting on a floor fell into it for a step
//! before any normal impulse or friction applied.

use gdcore::math::Vector2;
use gdphysics2d::body::{BodyId, BodyType, PhysicsBody2D};
use gdphysics2d::shape::Shape2D;
use gdphysics2d::world::PhysicsWorld2D;

#[test]
fn a_body_starting_exactly_on_a_floor_is_in_contact_on_the_first_step() {
    let mut world = PhysicsWorld2D::new();
    // Circle bottom at y = 5, floor top at y = 5: a zero gap.
    let mut ball = PhysicsBody2D::new(
        BodyId(0),
        BodyType::Rigid,
        Vector2::new(0.0, 0.0),
        Shape2D::Circle { radius: 5.0 },
        1.0,
    );
    ball.linear_velocity = Vector2::new(0.0, 50.0);
    let ball_id = world.add_body(ball);
    world.add_body(PhysicsBody2D::new(
        BodyId(0),
        BodyType::Static,
        Vector2::new(0.0, 10.0),
        Shape2D::Rectangle {
            half_extents: Vector2::new(500.0, 5.0),
        },
        1.0,
    ));

    let events = world.step(1.0 / 60.0);
    assert_eq!(
        events.len(),
        1,
        "the touching pair is a contact on step one"
    );
    let ball = world.get_body(ball_id).unwrap();
    assert!(
        ball.linear_velocity.y.abs() < 1e-3,
        "the approach is stopped on the first step, not after sinking: {:?}",
        ball.linear_velocity
    );
    assert!(
        ball.position.y <= 0.0 + 1e-3,
        "it did not sink into the floor: {:?}",
        ball.position
    );
}
