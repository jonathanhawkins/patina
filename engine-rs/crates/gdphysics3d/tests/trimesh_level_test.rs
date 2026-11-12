//! pat-u3l8c.56 — level-geometry integration test.
//!
//! Drives the full `PhysicsWorld3D` simulation to prove that a static
//! `ConcavePolygonShape` trimesh ("level geometry") stops falling bodies:
//! before this bead concave shapes had empty geometry and no narrow phase, so
//! dynamic bodies fell straight through the ground.

use gdcore::math::Vector3;
use gdphysics3d::body::{BodyId3D, BodyType3D, PhysicsBody3D};
use gdphysics3d::shape::Shape3D;
use gdphysics3d::world::PhysicsWorld3D;

/// A large flat trimesh floor at y=0 (a 20×20 quad as two triangles).
fn floor_faces() -> Vec<Vector3> {
    let v0 = Vector3::new(-10.0, 0.0, -10.0);
    let v1 = Vector3::new(10.0, 0.0, -10.0);
    let v2 = Vector3::new(10.0, 0.0, 10.0);
    let v3 = Vector3::new(-10.0, 0.0, 10.0);
    vec![v0, v1, v2, v0, v2, v3]
}

fn static_floor() -> PhysicsBody3D {
    PhysicsBody3D::new(
        BodyId3D(0),
        BodyType3D::Static,
        Vector3::ZERO,
        Shape3D::ConcavePolygonShape {
            faces: floor_faces(),
            backface_collision: false,
        },
        0.0,
    )
}

#[test]
fn falling_sphere_rests_on_trimesh_floor() {
    let mut world = PhysicsWorld3D::new();
    world.gravity = Vector3::new(0.0, -9.8, 0.0);
    world.add_body(static_floor());

    let mut ball = PhysicsBody3D::new(
        BodyId3D(0),
        BodyType3D::Rigid,
        Vector3::new(0.0, 3.0, 0.0),
        Shape3D::Sphere { radius: 0.5 },
        1.0,
    );
    ball.can_sleep = false;
    let ball_id = world.add_body(ball);

    for _ in 0..300 {
        world.step(1.0 / 60.0);
    }

    let y = world.get_body(ball_id).unwrap().position.y;
    assert!(
        y > 0.2,
        "sphere must not fall through the trimesh floor, got y={y}"
    );
    assert!(
        y < 1.2,
        "sphere should settle on the floor near its radius, got y={y}"
    );
}

#[test]
fn falling_capsule_rests_on_trimesh_floor() {
    let mut world = PhysicsWorld3D::new();
    world.gravity = Vector3::new(0.0, -9.8, 0.0);
    world.add_body(static_floor());

    let mut capsule = PhysicsBody3D::new(
        BodyId3D(0),
        BodyType3D::Rigid,
        Vector3::new(0.0, 4.0, 0.0),
        Shape3D::CapsuleShape {
            radius: 0.5,
            height: 2.0,
        },
        1.0,
    );
    capsule.can_sleep = false;
    let capsule_id = world.add_body(capsule);

    for _ in 0..300 {
        world.step(1.0 / 60.0);
    }

    let y = world.get_body(capsule_id).unwrap().position.y;
    // The capsule half-length + radius keeps its centre well above the floor.
    assert!(
        y > 0.2,
        "capsule must not fall through the trimesh floor, got y={y}"
    );
}
