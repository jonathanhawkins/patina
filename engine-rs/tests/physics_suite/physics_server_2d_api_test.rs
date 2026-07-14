//! pat-9fep: PhysicsServer2D API surface — body_create, body_set_state, body_get_state.

use gdcore::math::Vector2;
use gdphysics2d::body::{BodyId, BodyType, PhysicsBody2D};
use gdphysics2d::shape::Shape2D;
use gdphysics2d::world::PhysicsWorld2D;

#[test]
fn body_create_and_add() {
    let mut world = PhysicsWorld2D::new();
    let body = PhysicsBody2D::new(
        BodyId(0),
        BodyType::Rigid,
        Vector2::ZERO,
        Shape2D::Circle { radius: 5.0 },
        1.0,
    );
    let id = world.add_body(body);
    assert!(world.get_body(id).is_some());
}

#[test]
fn body_get_state_position() {
    let mut world = PhysicsWorld2D::new();
    let body = PhysicsBody2D::new(
        BodyId(0),
        BodyType::Rigid,
        Vector2::new(10.0, 20.0),
        Shape2D::Circle { radius: 1.0 },
        1.0,
    );
    let id = world.add_body(body);
    let b = world.get_body(id).unwrap();
    assert!((b.position.x - 10.0).abs() < 0.001);
    assert!((b.position.y - 20.0).abs() < 0.001);
}

#[test]
fn body_set_state_velocity() {
    let mut world = PhysicsWorld2D::new();
    let body = PhysicsBody2D::new(
        BodyId(0),
        BodyType::Rigid,
        Vector2::ZERO,
        Shape2D::Circle { radius: 1.0 },
        1.0,
    );
    let id = world.add_body(body);
    world.get_body_mut(id).unwrap().linear_velocity = Vector2::new(100.0, 0.0);

    world.step(1.0 / 60.0);
    let b = world.get_body(id).unwrap();
    assert!(b.position.x > 0.0, "body should have moved");
}

#[test]
fn body_types_static_kinematic_rigid() {
    let mut world = PhysicsWorld2D::new();
    let shape = Shape2D::Circle { radius: 1.0 };

    let s = world.add_body(PhysicsBody2D::new(
        BodyId(0),
        BodyType::Static,
        Vector2::ZERO,
        shape,
        1.0,
    ));
    let k = world.add_body(PhysicsBody2D::new(
        BodyId(0),
        BodyType::Kinematic,
        Vector2::new(5.0, 0.0),
        shape,
        1.0,
    ));
    let r = world.add_body(PhysicsBody2D::new(
        BodyId(0),
        BodyType::Rigid,
        Vector2::new(10.0, 0.0),
        shape,
        1.0,
    ));

    assert_eq!(world.get_body(s).unwrap().body_type, BodyType::Static);
    assert_eq!(world.get_body(k).unwrap().body_type, BodyType::Kinematic);
    assert_eq!(world.get_body(r).unwrap().body_type, BodyType::Rigid);
}

#[test]
fn body_count_tracks_additions() {
    let mut world = PhysicsWorld2D::new();
    assert_eq!(world.body_count(), 0);

    let shape = Shape2D::Circle { radius: 1.0 };
    world.add_body(PhysicsBody2D::new(
        BodyId(0),
        BodyType::Rigid,
        Vector2::ZERO,
        shape,
        1.0,
    ));
    assert_eq!(world.body_count(), 1);

    world.add_body(PhysicsBody2D::new(
        BodyId(0),
        BodyType::Static,
        Vector2::ZERO,
        shape,
        1.0,
    ));
    assert_eq!(world.body_count(), 2);
}

#[test]
fn step_advances_rigid_body() {
    let mut world = PhysicsWorld2D::new();
    let mut body = PhysicsBody2D::new(
        BodyId(0),
        BodyType::Rigid,
        Vector2::ZERO,
        Shape2D::Circle { radius: 1.0 },
        1.0,
    );
    body.linear_velocity = Vector2::new(60.0, 0.0);
    let id = world.add_body(body);

    world.step(1.0); // 1 second
    let b = world.get_body(id).unwrap();
    assert!(
        (b.position.x - 60.0).abs() < 1.0,
        "should move ~60 units in 1s at v=60"
    );
}
