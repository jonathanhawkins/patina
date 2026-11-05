//! pat-2ifdx / pat-1unro — solver regressions the official Godot 3D platformer
//! exposed once its GridMap level had collision. Each case is the smallest
//! world that shows one of them; `gdscene/tests/platformer_3d_level_collision.rs`
//! drives the real game against Godot's own numbers.

use gdcore::math::Vector3;
use gdcore::math3d::Quaternion;
use gdphysics3d::body::{BodyId3D, BodyType3D, PhysicsBody3D};
use gdphysics3d::character::CharacterBody3D;
use gdphysics3d::collision::test_collision_rot;
use gdphysics3d::shape::Shape3D;
use gdphysics3d::world::PhysicsWorld3D;

fn floor_box() -> PhysicsBody3D {
    // Top face at y = 0.
    PhysicsBody3D::new(
        BodyId3D(0),
        BodyType3D::Static,
        Vector3::new(0.0, -1.0, 0.0),
        Shape3D::BoxShape {
            half_extents: Vector3::new(50.0, 1.0, 50.0),
        },
        0.0,
    )
}

/// A character walking on a floor moves by its motion ONCE.
///
/// A floor contact used to be resolved by moving to the full target, pushing
/// out along the normal (already the slide) — and then applying the slid
/// remainder a second time, so a grounded character covered twice the
/// distance its velocity asked for. The platformer's player walked at 12 m/s
/// with `velocity` reading its MAX_SPEED of 6.
#[test]
fn a_grounded_character_moves_its_motion_once() {
    let floor = floor_box();
    let bodies = [&floor];
    let capsule = Shape3D::CapsuleShape {
        radius: 0.4,
        height: 2.1,
    };
    let mut ch = CharacterBody3D::new(Vector3::new(0.0, 0.001, 0.0), capsule);
    // The capsule sits on its feet: its centre is half its height up.
    ch.shape_offset = Vector3::new(0.0, 1.05, 0.0);
    // Settle onto the floor.
    for _ in 0..4 {
        ch.move_and_slide(Vector3::new(0.0, -0.0015, 0.0), &bodies);
    }
    assert!(ch.is_on_floor());
    let start = ch.position;
    // 0.05 m along X per tick, pressing into the floor like gravity does.
    for _ in 0..10 {
        let out = ch.move_and_slide(Vector3::new(0.05, -0.0015, 0.0), &bodies);
        // Godot resets the gravity accumulation on the floor
        // (`velocity.slide(up_direction)`), keeping the walk.
        assert_eq!(out.y, 0.0, "grounded: no velocity into the floor survives");
        assert!((out.x - 0.05).abs() < 1e-6, "the walk is kept: {out:?}");
    }
    let travelled = ch.position.x - start.x;
    assert!(
        (travelled - 0.5).abs() < 1e-3,
        "10 ticks of 0.05 m must cover 0.5 m, covered {travelled}"
    );
    assert!(
        (ch.position.y - 0.001).abs() < 1e-3,
        "stands at the floor plus safe_margin: {}",
        ch.position.y
    );
}

/// The shape offset is honoured: a capsule raised by half its height stands
/// with the body ORIGIN on the floor, as the platformer's player does in
/// Godot (`position.y == -3.998966` on a floor whose top is -4).
#[test]
fn a_raised_capsule_stands_with_its_origin_on_the_floor() {
    let floor = floor_box();
    let bodies = [&floor];
    let mut ch = CharacterBody3D::new(
        Vector3::new(0.0, 0.5, 0.0),
        Shape3D::CapsuleShape {
            radius: 0.4,
            height: 2.1,
        },
    );
    ch.shape_offset = Vector3::new(0.0, 1.05, 0.0);
    for _ in 0..60 {
        ch.move_and_slide(Vector3::new(0.0, -0.05, 0.0), &bodies);
    }
    assert!(ch.is_on_floor());
    assert!(
        ch.position.y.abs() < 0.002,
        "origin at the floor top, got {}",
        ch.position.y
    );
}

/// A rotation-locked rigid body touching the floor off-centre has its
/// approaching velocity fully cancelled.
///
/// A locked axis is infinitely stiff, so it must not appear in the contact's
/// effective mass. Budgeting part of the impulse for a rotation the lock then
/// discards left most of the downward velocity in place every tick — the
/// platformer's enemies rested with `linear_velocity.y` near -2 where Godot
/// reads -0.18.
#[test]
fn a_rotation_locked_body_on_the_floor_loses_its_downward_velocity() {
    let mut world = PhysicsWorld3D::new();
    world.gravity = Vector3::ZERO;
    world.add_body(floor_box());
    // A sphere hung off to one side of the body origin, just touching down.
    let mut body = PhysicsBody3D::new(
        BodyId3D(0),
        BodyType3D::Rigid,
        Vector3::new(0.0, 0.5, 0.0),
        Shape3D::Sphere { radius: 0.5 },
        1.0,
    );
    body.shape_offset = Vector3::new(0.6, 0.0, 0.0);
    body.axis_lock_angular = [true; 3];
    body.can_sleep = false;
    body.linear_velocity = Vector3::new(0.0, -0.5, 0.0);
    let id = world.add_body(body);
    world.step(1.0 / 120.0);
    let v = world.get_body(id).unwrap().linear_velocity;
    assert!(
        v.y.abs() < 1e-4,
        "the contact must stop the approach completely, v = {v:?}"
    );
}

/// `lock_rotation` behaves like all three axis locks for the contact solver.
#[test]
fn lock_rotation_has_no_inverse_inertia() {
    let mut body = PhysicsBody3D::new(
        BodyId3D(1),
        BodyType3D::Rigid,
        Vector3::ZERO,
        Shape3D::Sphere { radius: 0.5 },
        1.0,
    );
    let torque = Vector3::new(1.0, 2.0, 3.0);
    assert!(body.apply_inverse_inertia(torque).length() > 0.0);
    body.lock_rotation = true;
    assert_eq!(body.apply_inverse_inertia(torque), Vector3::ZERO);
    body.lock_rotation = false;
    body.axis_lock_angular = [true, false, true];
    let w = body.apply_inverse_inertia(torque);
    assert_eq!((w.x, w.z), (0.0, 0.0));
    assert!(w.y != 0.0);
}

/// The exact sphere-vs-convex solver: a sphere resting 1.5 mm into the flat
/// top of a convex slab reports 1.5 mm straight up, both argument orders
/// agree, and past the side the closest feature is the top edge. (The EPA
/// path it replaced reported 109 mm along a tilted normal on the platformer's
/// real `floor_edge` tile; that case is pinned by the enemy test in
/// `gdscene/tests/platformer_3d_level_collision.rs`, which goes red without
/// this solver. This simpler slab is a correctness check of the solver.)
#[test]
fn sphere_on_a_convex_slab_reports_the_true_depth() {
    // A 2 x 0.2 x 2 slab whose top face is y = 0, offset so its centre is not
    // the origin, as an authored tile hull is.
    let mut points = Vec::new();
    for x in [-1.0, 1.0] {
        for y in [-0.2, 0.0] {
            for z in [-1.0, 1.0] {
                points.push(Vector3::new(x, y, z));
            }
        }
    }
    let slab = Shape3D::ConvexPolygonShape { points };
    let sphere = Shape3D::Sphere { radius: 0.68 };
    // Over the slab's very edge, 1.5 mm down — the tile-seam case.
    let centre = Vector3::new(-0.95, 0.68 - 0.0015, 0.2);
    let r = test_collision_rot(
        centre,
        Quaternion::IDENTITY,
        &sphere,
        Vector3::ZERO,
        Quaternion::IDENTITY,
        &slab,
    );
    assert!(r.colliding);
    assert!((r.depth - 0.0015).abs() < 1e-4, "depth {}", r.depth);
    // A → B: from the sphere down into the slab.
    assert!((r.normal - Vector3::new(0.0, -1.0, 0.0)).length() < 1e-3, "{:?}", r.normal);

    let flipped = test_collision_rot(
        Vector3::ZERO,
        Quaternion::IDENTITY,
        &slab,
        centre,
        Quaternion::IDENTITY,
        &sphere,
    );
    assert!((flipped.depth - 0.0015).abs() < 1e-4);
    assert!((flipped.normal - Vector3::new(0.0, 1.0, 0.0)).length() < 1e-3);

    // Clear of the slab: no contact.
    let apart = test_collision_rot(
        Vector3::new(-0.95, 0.7, 0.2),
        Quaternion::IDENTITY,
        &sphere,
        Vector3::ZERO,
        Quaternion::IDENTITY,
        &slab,
    );
    assert!(!apart.colliding);

    // Past the slab's side, the closest feature is the top EDGE: depth is the
    // radius minus the distance to that edge, along the diagonal.
    let off_edge = Vector3::new(-1.3, 0.3, 0.0);
    let e = test_collision_rot(
        off_edge,
        Quaternion::IDENTITY,
        &sphere,
        Vector3::ZERO,
        Quaternion::IDENTITY,
        &slab,
    );
    let to_edge = (off_edge - Vector3::new(-1.0, 0.0, 0.0)).length();
    assert!((e.depth - (0.68 - to_edge)).abs() < 1e-4, "edge depth {}", e.depth);
}
