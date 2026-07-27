//! pat-0q2kx.32 — `intersect_ray` against a **rotated** body.
//!
//! Godot ground truth: `servers/physics_2d/godot_space_2d.cpp` intersects a ray
//! in each shape's own space, so a rotated collider is hit as it is drawn.
//! `world.rs::raycast_shape` implements that by transforming the ray through
//! `body_transform(position, rotation).affine_inverse()` before testing, then
//! mapping the hit point and normal back out.
//!
//! ⚠️ **That rotation handling had no test.** `world.rs`'s in-crate suite covers
//! `raycast_hits_circle` / `_capsule` / `_segment`, mask semantics, exclusions,
//! `hit_from_inside`, max-distance and closest-body — but not one of them rotates
//! a body before casting, and a **circle is rotation-invariant**, so the shape
//! most covered is precisely the one that cannot detect a rotation bug.
//! `step_narrowphase_is_rotation_aware` covers the *narrowphase*, a different
//! code path.
//!
//! The geometry below is chosen so a rotation-blind implementation does not
//! merely lose precision — it returns the **opposite answer** on both cases:
//!
//! ```text
//!   wall: Rectangle half_extents (10, 100), at the origin
//!     unrotated -> spans x ∈ [-10, 10],  y ∈ [-100, 100]   (tall and thin)
//!     rotated 90° -> spans x ∈ [-100, 100], y ∈ [-10, 10]   (wide and flat)
//!
//!   ray V : vertical   at x = 50   -> MISS unrotated, HIT rotated
//!   ray H : horizontal at y = 50   -> HIT unrotated, MISS rotated
//! ```
//!
//! Both rays stay outside the box at their origin in every case, so nothing here
//! depends on `hit_from_inside`.

use gdcore::math::Vector2;
use gdphysics2d::body::{BodyId, BodyType, PhysicsBody2D};
use gdphysics2d::shape::Shape2D;
use gdphysics2d::world::{PhysicsWorld2D, RayQuery2D};

/// A tall, thin static wall at the origin, rotated by `rotation` radians.
///
/// The extents are deliberately asymmetric — a square would be indistinguishable
/// under a quarter turn and could not fail either assertion.
fn world_with_wall(rotation: f32) -> PhysicsWorld2D {
    let mut world = PhysicsWorld2D::new();
    let mut wall = PhysicsBody2D::new(
        BodyId(1),
        BodyType::Static,
        Vector2::ZERO,
        Shape2D::Rectangle {
            half_extents: Vector2::new(10.0, 100.0),
        },
        0.0,
    );
    wall.rotation = rotation;
    world.add_body(wall);
    world
}

/// Vertical ray down the line `x = 50`, from y=50 to y=-50.
fn ray_v() -> RayQuery2D {
    RayQuery2D::new(Vector2::new(50.0, 50.0), Vector2::new(50.0, -50.0))
}

/// Horizontal ray along `y = 50`, from x=150 to x=-150.
fn ray_h() -> RayQuery2D {
    RayQuery2D::new(Vector2::new(150.0, 50.0), Vector2::new(-150.0, 50.0))
}

/// **The acceptance case.** A quarter turn swaps which ray can reach the wall.
#[test]
fn rotated_body_is_raycast_where_it_is_drawn() {
    let quarter = std::f32::consts::FRAC_PI_2;

    // -- unrotated: the tall thin wall is reachable across, not down ----------
    let flat = world_with_wall(0.0);
    assert!(
        flat.intersect_ray(&ray_v()).is_none(),
        "x=50 is far outside an unrotated wall spanning x ∈ [-10, 10]"
    );
    let across = flat
        .intersect_ray(&ray_h())
        .expect("y=50 lies inside the unrotated wall's y ∈ [-100, 100], so this must hit");
    assert!(
        (across.point.x - 10.0).abs() < 0.5,
        "the ray travels -X and must stop on the near face at x=10, got {:?}",
        across.point
    );
    assert!(
        across.normal.x > 0.5,
        "the surface normal must face back along the ray (+X), got {:?}",
        across.normal
    );

    // -- rotated 90°: exactly the opposite pair ------------------------------
    let turned = world_with_wall(quarter);
    assert!(
        turned.intersect_ray(&ray_h()).is_none(),
        "after a quarter turn the wall spans y ∈ [-10, 10]; a ray at y=50 must \
         now miss. A rotation-blind cast still reports the unrotated hit here — \
         this is the assertion that fails without the inverse transform"
    );
    let down = turned
        .intersect_ray(&ray_v())
        .expect("the turned wall spans x ∈ [-100, 100], so a ray at x=50 must now hit it");
    assert!(
        (down.point.y - 10.0).abs() < 0.5,
        "travelling -Y, the ray must stop on the turned wall's near face at \
         y=10, got {:?}",
        down.point
    );
    assert!(
        (down.point.x - 50.0).abs() < 0.5,
        "the hit stays on the ray's own line x=50, got {:?}",
        down.point
    );
    assert!(
        down.normal.y > 0.5,
        "the normal must face back up the ray (+Y), got {:?}",
        down.normal
    );
}

/// A half turn maps the box onto itself, so results must be identical to
/// unrotated — the control that keeps the test above from passing for an
/// implementation that merely perturbs the ray whenever rotation is non-zero.
#[test]
fn half_turn_is_indistinguishable_for_a_symmetric_box() {
    let flat = world_with_wall(0.0);
    let half = world_with_wall(std::f32::consts::PI);

    assert!(
        half.intersect_ray(&ray_v()).is_none(),
        "a 180° turn leaves the box's extents unchanged, so x=50 still misses"
    );

    let a = flat.intersect_ray(&ray_h()).expect("unrotated hit");
    let b = half.intersect_ray(&ray_h()).expect("half-turned hit");
    assert!(
        (a.point.x - b.point.x).abs() < 0.5 && (a.point.y - b.point.y).abs() < 0.5,
        "a symmetric box hit at the same place before and after a half turn: \
         {:?} vs {:?}",
        a.point,
        b.point
    );
    assert!(
        (a.distance - b.distance).abs() < 0.5,
        "and at the same distance: {} vs {}",
        a.distance,
        b.distance
    );
}

/// The hit distance is measured in world space. `body_transform` is rigid
/// (rotation + translation, no scale), so a distance computed in the body's
/// local space must survive the round trip unchanged — if a scale ever crept
/// into that transform, this is what would catch it.
#[test]
fn distance_survives_the_local_space_round_trip() {
    let turned = world_with_wall(std::f32::consts::FRAC_PI_2);
    let hit = turned.intersect_ray(&ray_v()).expect("hit");
    // Ray starts at y=50 and stops at y=10 → 40 units travelled.
    assert!(
        (hit.distance - 40.0).abs() < 0.5,
        "expected ~40 units from (50,50) to (50,10), got {}",
        hit.distance
    );
    assert!(
        (hit.normal.x * hit.normal.x + hit.normal.y * hit.normal.y - 1.0).abs() < 1e-3,
        "the normal must come back unit-length through the inverse transform, \
         got {:?}",
        hit.normal
    );
}
