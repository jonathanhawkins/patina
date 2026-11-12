//! pat-u3l8c.54 — Physics3D narrow-phase matrix, exercised at the world level.
//!
//! Before this bead the 3D narrow phase only matched sphere/box combinations:
//! capsules (the default character shape) never collided, `WorldBoundary`
//! planes never collided so bodies fell through the ground, and boxes collided
//! as world-axis AABBs ignoring rotation.
//!
//! These tests drive the full `PhysicsWorld3D` simulation so the fixes are
//! verified end-to-end (bodies actually rest on floors), plus a handful of
//! direct narrow-phase assertions for rotation-aware pairs.

use gdcore::math::Vector3;
use gdcore::math3d::Quaternion;
use gdphysics3d::body::{BodyId3D, BodyType3D, PhysicsBody3D};
use gdphysics3d::collision::{self, test_collision_rot};
use gdphysics3d::shape::Shape3D;
use gdphysics3d::world::PhysicsWorld3D;

fn rigid(pos: Vector3, shape: Shape3D) -> PhysicsBody3D {
    let mut b = PhysicsBody3D::new(BodyId3D(0), BodyType3D::Rigid, pos, shape, 1.0);
    b.can_sleep = false;
    b
}

fn static_body(pos: Vector3, shape: Shape3D) -> PhysicsBody3D {
    PhysicsBody3D::new(BodyId3D(0), BodyType3D::Static, pos, shape, 0.0)
}

fn settle(world: &mut PhysicsWorld3D, steps: usize) {
    for _ in 0..steps {
        world.step(1.0 / 60.0);
    }
}

// ── World-level: capsule rests on a box floor ──────────────────────────────

#[test]
fn capsule_character_stands_on_box_floor() {
    let mut world = PhysicsWorld3D::new();

    // Static box floor: top surface at y = 0.5.
    let floor = static_body(
        Vector3::ZERO,
        Shape3D::BoxShape {
            half_extents: Vector3::new(10.0, 0.5, 10.0),
        },
    );
    world.add_body(floor);

    // Rigid capsule (r=0.5, h=2 ⇒ lowest point 1.0 below its center) dropped
    // from above. It should come to rest with its center near y = 1.5 and must
    // NOT fall through the floor.
    let cap_id = world.add_body(rigid(
        Vector3::new(0.0, 4.0, 0.0),
        Shape3D::CapsuleShape {
            radius: 0.5,
            height: 2.0,
        },
    ));

    settle(&mut world, 240);

    let y = world.get_body(cap_id).unwrap().position.y;
    assert!(
        y > 1.4,
        "capsule should rest on the floor (y≈1.5), did not fall through; got y={y}"
    );
    assert!(
        y < 1.7,
        "capsule should settle onto the floor, not float; got y={y}"
    );
}

// ── World-level: WorldBoundary plane is solid ground ───────────────────────

#[test]
fn sphere_lands_on_worldboundary_plane() {
    let mut world = PhysicsWorld3D::new();

    // Ground plane: solid half-space is y <= 0 (normal +Y, distance 0).
    let ground = static_body(
        Vector3::ZERO,
        Shape3D::WorldBoundaryShape {
            normal: Vector3::new(0.0, 1.0, 0.0),
            distance: 0.0,
        },
    );
    world.add_body(ground);

    // Sphere of radius 1 dropped from y=5 should rest with its center at y≈1.
    let ball_id = world.add_body(rigid(
        Vector3::new(0.0, 5.0, 0.0),
        Shape3D::Sphere { radius: 1.0 },
    ));

    settle(&mut world, 300);

    let y = world.get_body(ball_id).unwrap().position.y;
    assert!(
        y > 0.85,
        "sphere must not fall through the WorldBoundary ground; got y={y}"
    );
    assert!(
        y < 1.2,
        "sphere should rest on the ground plane (y≈1.0); got y={y}"
    );
}

#[test]
fn box_lands_on_worldboundary_plane() {
    let mut world = PhysicsWorld3D::new();
    world.add_body(static_body(
        Vector3::ZERO,
        Shape3D::WorldBoundaryShape {
            normal: Vector3::new(0.0, 1.0, 0.0),
            distance: 0.0,
        },
    ));
    let box_id = world.add_body(rigid(
        Vector3::new(0.0, 5.0, 0.0),
        Shape3D::BoxShape {
            half_extents: Vector3::new(1.0, 1.0, 1.0),
        },
    ));

    settle(&mut world, 300);

    let y = world.get_body(box_id).unwrap().position.y;
    assert!(y > 0.85, "box must not fall through the plane; got y={y}");
    assert!(y < 1.2, "box should rest at y≈1.0; got y={y}");
}

// ── Direct narrow-phase: rotation-aware pairs ──────────────────────────────

#[test]
fn rotated_crate_collides_when_axis_aligned_would_not() {
    let a = Shape3D::BoxShape {
        half_extents: Vector3::new(1.0, 1.0, 1.0),
    };
    let b = Shape3D::BoxShape {
        half_extents: Vector3::new(1.0, 1.0, 1.0),
    };
    let yaw45 = Quaternion::from_axis_angle(Vector3::new(0.0, 1.0, 0.0), 45f32.to_radians());

    // At x=2.2 the axis-aligned boxes are clear (half-widths sum to 2.0), but a
    // 45°-yawed box reaches sqrt(2)≈1.414 toward A, so the corner pokes in.
    let hit = test_collision_rot(
        Vector3::ZERO,
        Quaternion::IDENTITY,
        &a,
        Vector3::new(2.2, 0.0, 0.0),
        yaw45,
        &b,
    );
    assert!(hit.colliding, "rotated crate corner should collide");
    assert!(hit.depth > 0.0);

    // Sanity: identical placement without rotation does NOT collide.
    let axis_aligned = test_collision_rot(
        Vector3::ZERO,
        Quaternion::IDENTITY,
        &a,
        Vector3::new(2.2, 0.0, 0.0),
        Quaternion::IDENTITY,
        &b,
    );
    assert!(
        !axis_aligned.colliding,
        "axis-aligned boxes at x=2.2 should be clear"
    );
}

#[test]
fn capsule_vs_capsule_and_cylinder_pairs_resolve() {
    // Two overlapping vertical capsules collide (previously returned NONE).
    let cap = Shape3D::CapsuleShape {
        radius: 0.5,
        height: 2.0,
    };
    let cc = collision::test_collision(Vector3::ZERO, &cap, Vector3::new(0.8, 0.0, 0.0), &cap);
    assert!(cc.colliding, "overlapping capsules should collide");
    assert!(
        cc.normal.x > 0.8,
        "capsule→capsule normal +X, got {:?}",
        cc.normal
    );

    // Cylinder vs cylinder (routed through GJK/EPA) collides when overlapping.
    let cyl = Shape3D::CylinderShape {
        radius: 1.0,
        height: 2.0,
    };
    let yy = collision::test_collision(Vector3::ZERO, &cyl, Vector3::new(1.5, 0.0, 0.0), &cyl);
    assert!(yy.colliding, "overlapping cylinders should collide");

    // Capsule vs cylinder (mixed GJK/EPA pair) collides when overlapping.
    let mixed = collision::test_collision(Vector3::ZERO, &cap, Vector3::new(1.0, 0.0, 0.0), &cyl);
    assert!(
        mixed.colliding,
        "overlapping capsule/cylinder should collide"
    );
}

#[test]
fn tilted_capsule_on_plane_collides() {
    // A capsule tilted 30° still reports collision with the ground plane when
    // its lower cap dips below y=0.
    let plane = Shape3D::WorldBoundaryShape {
        normal: Vector3::new(0.0, 1.0, 0.0),
        distance: 0.0,
    };
    let cap = Shape3D::CapsuleShape {
        radius: 0.5,
        height: 2.0,
    };
    let tilt = Quaternion::from_axis_angle(Vector3::new(0.0, 0.0, 1.0), 30f32.to_radians());
    // Center at y=0.4: the lower cap reaches below the plane regardless of tilt.
    let hit = test_collision_rot(
        Vector3::ZERO,
        Quaternion::IDENTITY,
        &plane,
        Vector3::new(0.0, 0.4, 0.0),
        tilt,
        &cap,
    );
    assert!(
        hit.colliding,
        "tilted capsule should touch the ground plane"
    );
    assert!(
        hit.normal.y > 0.9,
        "plane→capsule normal +Y, got {:?}",
        hit.normal
    );
}

/// A box resting OFF-centre on a wide floor stays flat: its contact must sit
/// under the box, not half-way to the floor's centre (which torqued it over).
#[test]
fn a_box_resting_off_centre_on_a_wide_floor_stays_flat() {
    let mut world = PhysicsWorld3D::new();
    world.add_body(static_body(
        Vector3::ZERO,
        Shape3D::BoxShape {
            half_extents: Vector3::new(10.0, 0.5, 10.0),
        },
    ));
    let id = world.add_body(rigid(
        Vector3::new(6.0, 1.6, -4.0),
        Shape3D::BoxShape {
            half_extents: Vector3::new(0.5, 0.5, 0.5),
        },
    ));
    settle(&mut world, 180);
    let b = world.get_body(id).unwrap();
    assert!(
        (b.position.y - 1.0).abs() < 0.1,
        "rests on the floor; y={}",
        b.position.y
    );
    assert!(
        b.rotation.w.abs() > 0.999,
        "must not tip: rot={:?}",
        b.rotation
    );
    assert!(
        (b.position.x - 6.0).abs() < 0.1 && (b.position.z + 4.0).abs() < 0.1,
        "must not slide: {:?}",
        b.position
    );
}

/// Edge: a box whose centre hangs past a ledge still tips — the contact is
/// clamped onto the ledge's top face, off the box's centre of mass.
#[test]
fn a_box_hanging_past_a_ledge_tips_over() {
    let mut world = PhysicsWorld3D::new();
    // Ledge top face spans x in [-2, 2].
    world.add_body(static_body(
        Vector3::ZERO,
        Shape3D::BoxShape {
            half_extents: Vector3::new(2.0, 0.5, 2.0),
        },
    ));
    let id = world.add_body(rigid(
        Vector3::new(2.3, 1.05, 0.0),
        Shape3D::BoxShape {
            half_extents: Vector3::new(0.5, 0.5, 0.5),
        },
    ));
    settle(&mut world, 60);
    let b = world.get_body(id).unwrap();
    assert!(
        b.rotation.w.abs() < 0.995,
        "an overhanging box must rotate: rot={:?}",
        b.rotation
    );
}
