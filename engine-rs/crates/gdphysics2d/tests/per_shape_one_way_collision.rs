//! pat-s3rrv.25: `one_way_collision` is a property of the SHAPE, not the body.
//!
//! Godot puts `one_way_collision` on the `CollisionShape2D` child
//! (`upstream/godot/scene/2d/physics/collision_shape_2d.cpp`), so one
//! `CollisionObject2D` can own a solid collider and a one-way collider at the
//! same time — the shape every platformer character has: a solid torso and a
//! one-way foot sensor.
//!
//! Patina lifted the flag onto `PhysicsBody2D`, which made those two states
//! mutually exclusive. The multi-shape migration moved the *control flow* per
//! shape (both `world.rs`'s pair loop and `character.rs`'s sweeps iterate
//! `shape_owners()`), but the flag itself stayed body-wide, so every collider on
//! a body still had to agree.
//!
//! Both tests below drive ONE body carrying two colliders and assert that the
//! two halves behave DIFFERENTLY under the same motion. That is the assertion a
//! body-wide flag cannot pass in either direction: set it and both halves let
//! the mover through, clear it and both block.

use gdcore::math::{Transform2D, Vector2};
use gdphysics2d::body::{BodyId, BodyType, PhysicsBody2D, ShapeOwner2D};
use gdphysics2d::character::CharacterBody2D;
use gdphysics2d::shape::Shape2D;
use gdphysics2d::world::PhysicsWorld2D;

const DT: f32 = 1.0 / 60.0;

/// Half-width and half-height of each of the platform's two colliders.
const HALF: Vector2 = Vector2::new(25.0, 5.0);

/// A single static body spanning x ∈ [-50, 50] out of TWO colliders:
///
/// * owner 0 — the primary shape, x ∈ [-50, 0], **solid**;
/// * owner 1 — an extra shape, x ∈ [0, 50], **one-way** with its blocking face
///   pointing up.
///
/// Both sit at y ∈ [-5, 5]. A mover travelling up under the left half must be
/// blocked; the same mover under the right half must pass through.
fn split_platform() -> PhysicsBody2D {
    let mut b = PhysicsBody2D::new(
        BodyId(1),
        BodyType::Static,
        Vector2::ZERO,
        Shape2D::Rectangle { half_extents: HALF },
        0.0,
    );
    // Owner 0: solid. The body-level flag is the PRIMARY shape's flag, and it
    // stays false — which is exactly what makes this test meaningful.
    b.one_way_collision = false;
    b.shape_transform = Transform2D::translated(Vector2::new(-25.0, 0.0));
    // Owner 1: one-way, blocking face up (-Y is up in Godot's 2D coordinates).
    b.extra_shapes = vec![ShapeOwner2D {
        shape: Shape2D::Rectangle { half_extents: HALF },
        transform: Transform2D::translated(Vector2::new(25.0, 0.0)),
        one_way_collision: true,
        one_way_direction: Vector2::new(0.0, -1.0),
    }];
    b
}

/// A rigid circle at `position` travelling straight up at 10 px/s.
fn riser(id: u64, position: Vector2) -> PhysicsBody2D {
    let mut b = PhysicsBody2D::new(
        BodyId(id),
        BodyType::Rigid,
        position,
        Shape2D::Circle { radius: 5.0 },
        1.0,
    );
    b.linear_velocity = Vector2::new(0.0, -10.0);
    b
}

/// **The acceptance test**: `world.rs`'s pair loop reads the OWNER's one-way
/// flags, so one body is solid on one collider and permeable on another.
#[test]
fn body_vs_body_one_way_is_per_shape() {
    let mut world = PhysicsWorld2D::new();

    // Both risers overlap the platform's y band by 2 px and are moving up, so
    // the only thing that can distinguish them is WHICH collider they are under.
    let solid_side = world.add_body(riser(2, Vector2::new(-25.0, 8.0)));
    let one_way_side = world.add_body(riser(3, Vector2::new(25.0, 8.0)));
    world.add_body(split_platform());

    let events = world.step(0.0);

    let touched = |id: BodyId| events.iter().any(|e| e.body_a == id || e.body_b == id);

    assert!(
        touched(solid_side),
        "the platform's SOLID collider must still block a body rising into it — \
         no contact was reported for the mover at x = -25. Events: {events:?}",
    );
    assert!(
        !touched(one_way_side),
        "the platform's ONE-WAY collider must let the same rising body through — \
         a contact was reported for the mover at x = +25, which means the \
         one-way flag is being read off the BODY (where it is false) instead of \
         off the shape owner. Events: {events:?}",
    );
}

/// The same split platform seen by `CharacterBody2D::move_and_slide`, which
/// filters one-way shapes inside `deepest_hit_against` rather than dropping the
/// whole obstacle body.
#[test]
fn character_sweeps_pass_only_the_one_way_collider() {
    // 3600 px/s over one 1/60 s frame = 60 px of upward motion, enough to clear
    // the 10 px-thick platform entirely.
    const UP: Vector2 = Vector2::new(0.0, -3600.0);

    let obstacles = vec![split_platform()];
    let refs: Vec<&PhysicsBody2D> = obstacles.iter().collect();

    // Under the SOLID half: blocked below the platform's lower face (y = 5),
    // so the circle's centre never gets above y = 5 + radius = 10.
    let mut blocked =
        CharacterBody2D::new(Vector2::new(-25.0, 30.0), Shape2D::Circle { radius: 5.0 });
    blocked.move_and_slide_delta(UP, DT, &refs);
    assert!(
        blocked.position.y > 5.0,
        "a character rising into the platform's SOLID collider must be stopped \
         below it; ended at y = {}",
        blocked.position.y,
    );

    // Under the ONE-WAY half: straight through, ending above the platform.
    let mut through =
        CharacterBody2D::new(Vector2::new(25.0, 30.0), Shape2D::Circle { radius: 5.0 });
    through.move_and_slide_delta(UP, DT, &refs);
    assert!(
        through.position.y < -5.0,
        "a character rising into the platform's ONE-WAY collider must pass \
         through it; ended at y = {} (a body-wide flag would have stopped it \
         here exactly as it stopped the one at x = -25)",
        through.position.y,
    );

    // ...and the one-way collider is still SOLID from its front face: dropping
    // onto it lands. Without this half, "passes through" could be satisfied by a
    // collider that had simply stopped colliding at all.
    let mut lands =
        CharacterBody2D::new(Vector2::new(25.0, -30.0), Shape2D::Circle { radius: 5.0 });
    lands.move_and_slide_delta(-UP, DT, &refs);
    assert!(
        lands.position.y < -5.0,
        "the one-way collider must still block a character falling onto it from \
         above; it fell to y = {}",
        lands.position.y,
    );
}
