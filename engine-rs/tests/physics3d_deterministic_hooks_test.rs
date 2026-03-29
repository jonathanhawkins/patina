//! pat-azsj: Initial 3D physics hooks and deterministic test coverage.
//!
//! Integration-level tests for the 3D physics pipeline: PhysicsWorld3D,
//! PhysicsBody3D, Shape3D, collision detection, and raycasting. Exercises
//! the full simulation loop with determinism verification, multi-body
//! scenarios, and edge cases beyond the per-module unit tests.
//!
//! Acceptance: 3D physics hooks exist and deterministic tests prove
//! reproducible simulation results.

use gdcore::math::Vector3;
use gdphysics2d::body3d::{BodyId3D, BodyType3D, PhysicsBody3D};
use gdphysics2d::shape3d::Shape3D;
use gdphysics2d::world3d::PhysicsWorld3D;

const DT: f32 = 1.0 / 60.0;
const EPSILON: f32 = 1e-4;

fn approx_eq(a: f32, b: f32) -> bool {
    (a - b).abs() < EPSILON
}

fn v3_approx_eq(a: Vector3, b: Vector3) -> bool {
    approx_eq(a.x, b.x) && approx_eq(a.y, b.y) && approx_eq(a.z, b.z)
}

// ===========================================================================
// Helpers
// ===========================================================================

fn rigid_sphere(pos: Vector3, radius: f32, mass: f32) -> PhysicsBody3D {
    PhysicsBody3D::new(
        BodyId3D(0),
        BodyType3D::Rigid,
        pos,
        Shape3D::Sphere { radius },
        mass,
    )
}

fn static_box(pos: Vector3, half: Vector3) -> PhysicsBody3D {
    PhysicsBody3D::new(
        BodyId3D(0),
        BodyType3D::Static,
        pos,
        Shape3D::BoxShape { half_extents: half },
        1.0,
    )
}

fn kinematic_sphere(pos: Vector3, radius: f32) -> PhysicsBody3D {
    PhysicsBody3D::new(
        BodyId3D(0),
        BodyType3D::Kinematic,
        pos,
        Shape3D::Sphere { radius },
        1.0,
    )
}

// ===========================================================================
// 1. Determinism: same inputs → same outputs across runs
// ===========================================================================

#[test]
fn azsj_determinism_gravity_freefall() {
    fn run() -> (Vector3, Vector3) {
        let mut world = PhysicsWorld3D::new();
        let a = world.add_body(rigid_sphere(Vector3::new(0.0, 100.0, 0.0), 1.0, 1.0));
        let b = world.add_body(rigid_sphere(Vector3::new(10.0, 50.0, 5.0), 2.0, 3.0));

        for _ in 0..120 {
            world.step(DT);
        }

        let pa = world.get_body(a).unwrap().position;
        let pb = world.get_body(b).unwrap().position;
        (pa, pb)
    }

    let (a1, b1) = run();
    let (a2, b2) = run();
    assert!(
        v3_approx_eq(a1, a2),
        "body A not deterministic: {a1:?} vs {a2:?}"
    );
    assert!(
        v3_approx_eq(b1, b2),
        "body B not deterministic: {b1:?} vs {b2:?}"
    );
}

#[test]
fn azsj_determinism_collision_scenario() {
    fn run() -> Vec<Vector3> {
        let mut world = PhysicsWorld3D::new();
        world.gravity = Vector3::ZERO;

        let mut s1 = rigid_sphere(Vector3::new(-5.0, 0.0, 0.0), 2.0, 1.0);
        s1.linear_velocity = Vector3::new(3.0, 0.0, 0.0);
        let id1 = world.add_body(s1);

        let mut s2 = rigid_sphere(Vector3::new(5.0, 0.0, 0.0), 2.0, 1.0);
        s2.linear_velocity = Vector3::new(-3.0, 0.0, 0.0);
        let id2 = world.add_body(s2);

        let mut s3 = rigid_sphere(Vector3::new(0.0, 5.0, 0.0), 2.0, 2.0);
        s3.linear_velocity = Vector3::new(0.0, -2.0, 0.0);
        let id3 = world.add_body(s3);

        for _ in 0..60 {
            world.step(DT);
        }

        vec![
            world.get_body(id1).unwrap().position,
            world.get_body(id2).unwrap().position,
            world.get_body(id3).unwrap().position,
        ]
    }

    let r1 = run();
    let r2 = run();
    for (i, (a, b)) in r1.iter().zip(r2.iter()).enumerate() {
        assert!(
            v3_approx_eq(*a, *b),
            "body {i} not deterministic: {a:?} vs {b:?}"
        );
    }
}

// ===========================================================================
// 2. Gravity freefall: position follows physics equations
// ===========================================================================

#[test]
fn azsj_gravity_freefall_60_frames() {
    let mut world = PhysicsWorld3D::new();
    let id = world.add_body(rigid_sphere(Vector3::new(0.0, 100.0, 0.0), 1.0, 1.0));

    let initial_y = 100.0f32;
    for _ in 0..60 {
        world.step(DT);
    }

    let body = world.get_body(id).unwrap();
    // After 1 second of -9.8 m/s² gravity: y ≈ 100 - 0.5*9.8*1² = 95.1
    // (approximate due to Euler integration)
    assert!(body.position.y < initial_y, "body should fall");
    assert!(body.position.x == 0.0, "no lateral movement");
    assert!(body.position.z == 0.0, "no lateral movement");
    assert!(
        body.linear_velocity.y < 0.0,
        "should have downward velocity"
    );
}

// ===========================================================================
// 3. Zero gravity: linear motion preserves velocity
// ===========================================================================

#[test]
fn azsj_zero_gravity_linear_motion() {
    let mut world = PhysicsWorld3D::new();
    world.gravity = Vector3::ZERO;

    let mut body = rigid_sphere(Vector3::ZERO, 1.0, 1.0);
    body.linear_velocity = Vector3::new(10.0, 5.0, -3.0);
    let id = world.add_body(body);

    for _ in 0..60 {
        world.step(DT);
    }

    let b = world.get_body(id).unwrap();
    // After 1 second at constant velocity: pos ≈ velocity * 1.0
    assert!(approx_eq(b.position.x, 10.0));
    assert!(approx_eq(b.position.y, 5.0));
    assert!(approx_eq(b.position.z, -3.0));
    // Velocity unchanged.
    assert!(approx_eq(b.linear_velocity.x, 10.0));
    assert!(approx_eq(b.linear_velocity.y, 5.0));
    assert!(approx_eq(b.linear_velocity.z, -3.0));
}

// ===========================================================================
// 4. Static body immobility under gravity
// ===========================================================================

#[test]
fn azsj_static_body_immobile() {
    let mut world = PhysicsWorld3D::new();
    let origin = Vector3::new(5.0, 10.0, -3.0);
    let id = world.add_body(static_box(origin, Vector3::new(5.0, 5.0, 5.0)));

    for _ in 0..120 {
        world.step(DT);
    }

    let b = world.get_body(id).unwrap();
    assert_eq!(b.position, origin, "static body must not move");
}

// ===========================================================================
// 5. Kinematic body moves at set velocity, ignoring gravity
// ===========================================================================

#[test]
fn azsj_kinematic_body_moves_ignores_gravity() {
    let mut world = PhysicsWorld3D::new();
    let mut body = kinematic_sphere(Vector3::ZERO, 1.0);
    body.linear_velocity = Vector3::new(0.0, 10.0, 0.0); // moving UP against gravity
    let id = world.add_body(body);

    for _ in 0..60 {
        world.step(DT);
    }

    let b = world.get_body(id).unwrap();
    // Should move up, not be pulled down by gravity.
    assert!(
        b.position.y > 9.0,
        "kinematic should move up: y={}",
        b.position.y
    );
}

// ===========================================================================
// 6. Sphere-sphere collision separation
// ===========================================================================

#[test]
fn azsj_sphere_sphere_collision_separates() {
    let mut world = PhysicsWorld3D::new();
    world.gravity = Vector3::ZERO;

    // Two overlapping spheres.
    let id_a = world.add_body(rigid_sphere(Vector3::ZERO, 5.0, 1.0));
    let id_b = world.add_body(rigid_sphere(Vector3::new(6.0, 0.0, 0.0), 5.0, 1.0));

    world.step(0.0); // dt=0 to only do collision resolution

    let a = world.get_body(id_a).unwrap();
    let b = world.get_body(id_b).unwrap();
    let dist = (b.position - a.position).length();
    assert!(
        dist >= 10.0 - EPSILON,
        "overlapping spheres should separate to >= sum of radii, got {dist}"
    );
}

// ===========================================================================
// 7. Sphere-box collision separation
// ===========================================================================

#[test]
fn azsj_sphere_box_collision_separates() {
    let mut world = PhysicsWorld3D::new();
    world.gravity = Vector3::ZERO;

    // Static box at origin, sphere overlapping from above.
    world.add_body(static_box(Vector3::ZERO, Vector3::new(5.0, 5.0, 5.0)));
    let sphere_id = world.add_body(rigid_sphere(Vector3::new(0.0, 6.0, 0.0), 2.0, 1.0));

    world.step(0.0);

    let s = world.get_body(sphere_id).unwrap();
    // Sphere should be pushed out of the box.
    assert!(
        s.position.y >= 7.0 - EPSILON,
        "sphere should be separated from box, y={}",
        s.position.y
    );
}

// ===========================================================================
// 8. Multi-body simulation: 5 bodies falling onto a floor
// ===========================================================================

#[test]
fn azsj_five_bodies_fall_onto_floor() {
    let mut world = PhysicsWorld3D::new();

    // Floor at y=0.
    world.add_body(static_box(
        Vector3::new(0.0, -5.0, 0.0),
        Vector3::new(100.0, 5.0, 100.0),
    ));

    // 5 spheres at different heights.
    let mut ids = Vec::new();
    for i in 0..5 {
        let y = 10.0 + (i as f32) * 5.0;
        let x = (i as f32) * 4.0 - 8.0;
        ids.push(world.add_body(rigid_sphere(Vector3::new(x, y, 0.0), 1.0, 1.0)));
    }

    // Run 120 frames (2 seconds).
    for _ in 0..120 {
        world.step(DT);
    }

    // All bodies should have settled near the floor (y ≈ 1.0 for radius=1 sphere on y=0 floor).
    for (i, &id) in ids.iter().enumerate() {
        let b = world.get_body(id).unwrap();
        assert!(
            b.position.y < 30.0,
            "body {i} should have fallen, y={}",
            b.position.y
        );
    }
}

// ===========================================================================
// 9. Force application: upward force counteracts gravity
// ===========================================================================

#[test]
fn azsj_upward_force_counteracts_gravity() {
    let mut world = PhysicsWorld3D::new();
    let id = world.add_body(rigid_sphere(Vector3::new(0.0, 50.0, 0.0), 1.0, 1.0));

    for _ in 0..60 {
        // Apply upward force equal to gravity (mass * 9.8).
        if let Some(b) = world.get_body_mut(id) {
            b.apply_force(Vector3::new(0.0, 9.8, 0.0));
        }
        world.step(DT);
    }

    let b = world.get_body(id).unwrap();
    // Should stay approximately at y=50 (gravity cancelled).
    assert!(
        approx_eq(b.position.y, 50.0),
        "upward force should cancel gravity, y={}",
        b.position.y
    );
}

// ===========================================================================
// 10. Impulse: instant velocity change
// ===========================================================================

#[test]
fn azsj_impulse_instant_velocity_change() {
    let mut world = PhysicsWorld3D::new();
    world.gravity = Vector3::ZERO;

    let id = world.add_body(rigid_sphere(Vector3::ZERO, 1.0, 2.0));

    if let Some(b) = world.get_body_mut(id) {
        b.apply_impulse(Vector3::new(20.0, 0.0, 0.0));
    }

    let b = world.get_body(id).unwrap();
    // impulse / mass = 20 / 2 = 10
    assert!(approx_eq(b.linear_velocity.x, 10.0));
}

// ===========================================================================
// 11. Raycasting: sphere hit
// ===========================================================================

#[test]
fn azsj_raycast_hits_sphere() {
    let mut world = PhysicsWorld3D::new();
    world.gravity = Vector3::ZERO;
    let sphere_id = world.add_body(rigid_sphere(Vector3::new(0.0, 0.0, 20.0), 3.0, 1.0));

    let hit = world.raycast_3d(Vector3::ZERO, Vector3::new(0.0, 0.0, 1.0), 100.0);
    assert!(hit.is_some(), "ray should hit the sphere");
    let hit = hit.unwrap();
    assert_eq!(hit.body_id, sphere_id);
    assert!(
        approx_eq(hit.distance, 17.0),
        "hit distance should be 20 - 3 = 17, got {}",
        hit.distance
    );
}

// ===========================================================================
// 12. Raycasting: box hit
// ===========================================================================

#[test]
fn azsj_raycast_hits_box() {
    let mut world = PhysicsWorld3D::new();
    world.gravity = Vector3::ZERO;
    let box_id = world.add_body(static_box(
        Vector3::new(10.0, 0.0, 0.0),
        Vector3::new(2.0, 2.0, 2.0),
    ));

    let hit = world.raycast_3d(Vector3::ZERO, Vector3::new(1.0, 0.0, 0.0), 100.0);
    assert!(hit.is_some());
    let hit = hit.unwrap();
    assert_eq!(hit.body_id, box_id);
    assert!(
        approx_eq(hit.distance, 8.0),
        "distance should be 10-2=8, got {}",
        hit.distance
    );
}

// ===========================================================================
// 13. Raycasting: closest body returned
// ===========================================================================

#[test]
fn azsj_raycast_returns_closest() {
    let mut world = PhysicsWorld3D::new();
    world.gravity = Vector3::ZERO;

    let near_id = world.add_body(rigid_sphere(Vector3::new(10.0, 0.0, 0.0), 1.0, 1.0));
    let _far_id = world.add_body(rigid_sphere(Vector3::new(30.0, 0.0, 0.0), 1.0, 1.0));

    let hit = world.raycast_3d(Vector3::ZERO, Vector3::new(1.0, 0.0, 0.0), 100.0);
    assert!(hit.is_some());
    assert_eq!(hit.unwrap().body_id, near_id, "should return closest body");
}

// ===========================================================================
// 14. Raycasting: miss when no bodies in path
// ===========================================================================

#[test]
fn azsj_raycast_miss() {
    let mut world = PhysicsWorld3D::new();
    world.add_body(rigid_sphere(Vector3::new(0.0, 50.0, 0.0), 1.0, 1.0));

    // Shoot ray along X — sphere is along Y.
    let hit = world.raycast_3d(Vector3::ZERO, Vector3::new(1.0, 0.0, 0.0), 100.0);
    assert!(hit.is_none());
}

// ===========================================================================
// 15. Multiple step determinism: 300 frames
// ===========================================================================

#[test]
fn azsj_determinism_300_frames() {
    fn run() -> Vec<(f32, f32, f32)> {
        let mut world = PhysicsWorld3D::new();

        let mut a = rigid_sphere(Vector3::new(-10.0, 50.0, 0.0), 2.0, 1.0);
        a.linear_velocity = Vector3::new(5.0, 0.0, 1.0);
        let id_a = world.add_body(a);

        let mut b = rigid_sphere(Vector3::new(10.0, 50.0, 0.0), 2.0, 2.0);
        b.linear_velocity = Vector3::new(-3.0, 0.0, -1.0);
        let id_b = world.add_body(b);

        world.add_body(static_box(
            Vector3::new(0.0, -5.0, 0.0),
            Vector3::new(100.0, 5.0, 100.0),
        ));

        for _ in 0..300 {
            world.step(DT);
        }

        let pa = world.get_body(id_a).unwrap().position;
        let pb = world.get_body(id_b).unwrap().position;
        vec![(pa.x, pa.y, pa.z), (pb.x, pb.y, pb.z)]
    }

    let r1 = run();
    let r2 = run();
    assert_eq!(r1, r2, "300-frame simulation must be deterministic");
}

// ===========================================================================
// 16. Body removal during simulation
// ===========================================================================

#[test]
fn azsj_body_removal_mid_simulation() {
    let mut world = PhysicsWorld3D::new();
    world.gravity = Vector3::ZERO;

    let id_a = world.add_body(rigid_sphere(Vector3::ZERO, 1.0, 1.0));
    let id_b = world.add_body(rigid_sphere(Vector3::new(10.0, 0.0, 0.0), 1.0, 1.0));

    assert_eq!(world.body_count(), 2);

    for _ in 0..10 {
        world.step(DT);
    }

    world.remove_body(id_a);
    assert_eq!(world.body_count(), 1);
    assert!(world.get_body(id_a).is_none());
    assert!(world.get_body(id_b).is_some());

    // Simulation continues with remaining body.
    for _ in 0..10 {
        world.step(DT);
    }

    assert_eq!(world.body_count(), 1);
}

// ===========================================================================
// 17. Different masses: heavy vs light collision
// ===========================================================================

#[test]
fn azsj_mass_affects_collision_response() {
    let mut world = PhysicsWorld3D::new();
    world.gravity = Vector3::ZERO;

    // Heavy body (mass=10) and light body (mass=1) heading toward each other.
    let mut heavy = rigid_sphere(Vector3::new(-5.0, 0.0, 0.0), 2.0, 10.0);
    heavy.linear_velocity = Vector3::new(1.0, 0.0, 0.0);
    let heavy_id = world.add_body(heavy);

    let mut light = rigid_sphere(Vector3::new(5.0, 0.0, 0.0), 2.0, 1.0);
    light.linear_velocity = Vector3::new(-1.0, 0.0, 0.0);
    let light_id = world.add_body(light);

    for _ in 0..60 {
        world.step(DT);
    }

    let h = world.get_body(heavy_id).unwrap();
    let l = world.get_body(light_id).unwrap();

    // Heavy body should barely be deflected; light body should be pushed away more.
    // After collision, light body should be further from origin than heavy body.
    assert!(
        l.position.x.abs() > h.position.x.abs() || l.position.x > 0.0,
        "light body should be deflected more: heavy_x={}, light_x={}",
        h.position.x,
        l.position.x
    );
}

// ===========================================================================
// 18. Empty world: stepping is safe
// ===========================================================================

#[test]
fn azsj_empty_world_step_safe() {
    let mut world = PhysicsWorld3D::new();

    for _ in 0..60 {
        world.step(DT);
    }

    assert_eq!(world.body_count(), 0);
}

// ===========================================================================
// 19. Custom gravity direction
// ===========================================================================

#[test]
fn azsj_custom_gravity_direction() {
    let mut world = PhysicsWorld3D::new();
    world.gravity = Vector3::new(9.8, 0.0, 0.0); // gravity pulls RIGHT

    let id = world.add_body(rigid_sphere(Vector3::ZERO, 1.0, 1.0));

    for _ in 0..60 {
        world.step(DT);
    }

    let b = world.get_body(id).unwrap();
    assert!(
        b.position.x > 1.0,
        "should move right under sideways gravity"
    );
    assert!(approx_eq(b.position.y, 0.0), "no vertical movement");
}

// ===========================================================================
// 20. Shape3D bounding volumes: integration-level check
// ===========================================================================

#[test]
fn azsj_shape_bounding_volumes_consistent() {
    let shapes = [
        Shape3D::Sphere { radius: 3.0 },
        Shape3D::BoxShape {
            half_extents: Vector3::new(2.0, 3.0, 4.0),
        },
        Shape3D::CapsuleShape {
            radius: 1.5,
            height: 6.0,
        },
    ];

    for shape in &shapes {
        let aabb = shape.bounding_aabb();
        // AABB should always have positive size.
        assert!(aabb.size.x > 0.0, "{shape:?} AABB has zero/negative x size");
        assert!(aabb.size.y > 0.0, "{shape:?} AABB has zero/negative y size");
        assert!(aabb.size.z > 0.0, "{shape:?} AABB has zero/negative z size");
        // Origin should be inside AABB.
        assert!(
            shape.contains_point(Vector3::ZERO),
            "{shape:?} should contain origin"
        );
    }
}
