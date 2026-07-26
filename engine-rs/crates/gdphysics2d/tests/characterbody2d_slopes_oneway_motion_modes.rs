//! pat-0q2kx.113: CharacterBody2D respects obstacle rotation and does not tunnel.
//!
//! Two defects in `move_and_slide`, both invisible in slow, axis-aligned scenes:
//!
//! 1. **Obstacle rotation was discarded.** Every obstacle was collided against
//!    via `Transform2D::translated(body.position)`, dropping
//!    `PhysicsBody2D::rotation`. A rotated ramp therefore behaved like a flat
//!    box: the contact normal came back axis-aligned, so a steep slope was
//!    classified as floor and a shallow one as wall.
//! 2. **The slide loop tested overlap only at the target position.** At 600 px/s
//!    over one 1/60 s frame the body advances 10 px per step, so a thin platform
//!    can sit entirely between the start and target positions, register no
//!    overlap, and be passed straight through.
//!
//! Both are asserted through the public `move_and_slide_delta`, so a regression
//! in either shows up as a behaviour change rather than a silent geometry bug.

use gdcore::math::Vector2;
use gdphysics2d::body::{BodyId, BodyType, PhysicsBody2D};
use gdphysics2d::character::{CharacterBody2D, MotionMode, PlatformOnLeave};
use gdphysics2d::shape::Shape2D;

const DT: f32 = 1.0 / 60.0;

fn wall(position: Vector2, half_extents: Vector2, rotation: f32) -> PhysicsBody2D {
    let mut b = PhysicsBody2D::new(
        BodyId(1),
        BodyType::Static,
        position,
        Shape2D::Rectangle { half_extents },
        0.0,
    );
    b.rotation = rotation;
    b
}

/// **The acceptance test** for the two defects this change fixes.
#[test]
fn characterbody2d_slopes_oneway_motion_modes() {
    // -- (1) a fast body does not tunnel through a thin platform -------------
    //
    // The platform is 1 unit thick, spanning x ∈ [7.5, 8.5]. The body starts at
    // x = 0 with radius 0.5 and moves 600 px/s for one frame = 10 units, so its
    // *target* (x = 10, spanning 9.5..10.5) does not overlap the platform at
    // all. A target-only overlap test sees nothing and lets it through; a swept
    // one stops it.
    let mut body = CharacterBody2D::new(Vector2::ZERO, Shape2D::Circle { radius: 0.5 });
    let obstacles = vec![wall(Vector2::new(8.0, 0.0), Vector2::new(0.5, 50.0), 0.0)];
    let refs: Vec<&PhysicsBody2D> = obstacles.iter().collect();
    body.move_and_slide_delta(Vector2::new(600.0, 0.0), DT, &refs);

    assert!(
        body.position.x < 8.0,
        "a body moving 600 px/s must be stopped by a 1-unit-thick platform at \
         x = 8, not tunnel past it; ended at x = {}",
        body.position.x
    );

    // -- (2) obstacle rotation is honored when classifying a surface ---------
    //
    // A rectangle rotated 60° presents faces 60° from horizontal — steeper than
    // the default 45° `floor_max_angle`, so landing on it is a WALL contact, not
    // a floor one. If the rotation were dropped the same body would be a flat
    // box with an upward normal and would classify as floor, so this assertion
    // is exactly the difference between honoring rotation and discarding it.
    let steep = 60.0_f32.to_radians();
    let mut body = CharacterBody2D::new(Vector2::new(0.0, -3.0), Shape2D::Circle { radius: 0.5 });
    let obstacles = vec![wall(Vector2::ZERO, Vector2::new(6.0, 0.5), steep)];
    let refs: Vec<&PhysicsBody2D> = obstacles.iter().collect();
    // Drive downward onto it for several frames so contact is established.
    for _ in 0..8 {
        body.move_and_slide_delta(Vector2::new(0.0, 120.0), DT, &refs);
    }

    assert!(
        !body.is_on_floor(),
        "a surface rotated 60° is steeper than floor_max_angle (45°) and must \
         NOT classify as floor; reporting floor here means the obstacle's \
         rotation was discarded and it was treated as an axis-aligned box"
    );

    // -- (3) one-way platform: up THROUGH it, land ON it ---------------------
    //
    // The platform's `one_way_direction` is up, so upward motion passes through
    // and downward motion is blocked. Both halves are asserted: passthrough
    // alone would also be satisfied by "never collide".
    let mut platform = wall(Vector2::new(0.0, 0.0), Vector2::new(6.0, 0.25), 0.0);
    platform.one_way_collision = true;
    platform.one_way_direction = Vector2::new(0.0, -1.0);
    let obstacles = vec![platform];
    let refs: Vec<&PhysicsBody2D> = obstacles.iter().collect();

    // Jumping upward from below passes through.
    let mut body = CharacterBody2D::new(Vector2::new(0.0, 3.0), Shape2D::Circle { radius: 0.5 });
    for _ in 0..8 {
        body.move_and_slide_delta(Vector2::new(0.0, -120.0), DT, &refs);
    }
    assert!(
        body.position.y < 0.0,
        "a body moving UP through a one-way platform must pass through it; \
         stopped at y = {}",
        body.position.y
    );

    // Falling onto it from above is blocked.
    let mut body = CharacterBody2D::new(Vector2::new(0.0, -3.0), Shape2D::Circle { radius: 0.5 });
    for _ in 0..8 {
        body.move_and_slide_delta(Vector2::new(0.0, 120.0), DT, &refs);
    }
    assert!(
        body.position.y < 0.0,
        "a body falling DOWN onto a one-way platform must land on it, not fall \
         through; ended at y = {}",
        body.position.y
    );

    // -- (4) floating motion mode reports no floor state ---------------------
    let mut body = CharacterBody2D::new(Vector2::new(0.0, -3.0), Shape2D::Circle { radius: 0.5 });
    body.motion_mode = MotionMode::Floating;
    let flat = vec![wall(Vector2::ZERO, Vector2::new(6.0, 0.5), 0.0)];
    let refs: Vec<&PhysicsBody2D> = flat.iter().collect();
    for _ in 0..8 {
        body.move_and_slide_delta(Vector2::new(0.0, 120.0), DT, &refs);
    }
    assert!(
        !body.is_on_floor() && !body.is_on_wall() && !body.is_on_ceiling(),
        "MOTION_MODE_FLOATING privileges no up direction, so nothing may be \
         classified as floor/wall/ceiling"
    );

    // -- (4b) platform_on_leave = DO_NOTHING drops inherited velocity --------
    let mut body = CharacterBody2D::new(Vector2::ZERO, Shape2D::Circle { radius: 0.5 });
    body.platform_on_leave = PlatformOnLeave::DoNothing;
    body.platform_velocity = Vector2::new(200.0, 0.0);
    let none: Vec<PhysicsBody2D> = Vec::new();
    let refs: Vec<&PhysicsBody2D> = none.iter().collect();
    body.move_and_slide_delta(Vector2::ZERO, DT, &refs);
    assert!(
        body.position.x.abs() < 1e-4,
        "platform_on_leave = DO_NOTHING must inherit no platform velocity; the \
         body drifted to x = {}",
        body.position.x
    );
}

/// The mirror of (4b): the default ADD_VELOCITY *does* carry the platform's
/// momentum, so "never inherit" cannot pass the acceptance case.
#[test]
fn add_velocity_still_inherits_platform_momentum() {
    let mut body = CharacterBody2D::new(Vector2::ZERO, Shape2D::Circle { radius: 0.5 });
    body.platform_on_leave = PlatformOnLeave::AddVelocity;
    body.platform_velocity = Vector2::new(200.0, 0.0);
    let none: Vec<PhysicsBody2D> = Vec::new();
    let refs: Vec<&PhysicsBody2D> = none.iter().collect();
    body.move_and_slide_delta(Vector2::ZERO, DT, &refs);
    assert!(
        body.position.x > 1e-3,
        "the default ADD_VELOCITY must carry platform momentum, got x = {}",
        body.position.x
    );
}

/// Grounded mode (the default) still classifies a flat surface as floor — the
/// floating change must not disable ordinary platforming.
#[test]
fn grounded_mode_still_classifies_floor() {
    let mut body = CharacterBody2D::new(Vector2::new(0.0, -3.0), Shape2D::Circle { radius: 0.5 });
    assert_eq!(
        body.motion_mode,
        MotionMode::Grounded,
        "grounded is default"
    );
    let flat = vec![wall(Vector2::ZERO, Vector2::new(6.0, 0.5), 0.0)];
    let refs: Vec<&PhysicsBody2D> = flat.iter().collect();
    for _ in 0..8 {
        body.move_and_slide_delta(Vector2::new(0.0, 120.0), DT, &refs);
    }
    assert!(body.is_on_floor(), "grounded mode must still report floor");
}

/// The mirror of clause (2): the *same* rectangle left unrotated is a floor.
/// Without this, "never report floor" would pass the acceptance case.
#[test]
fn an_unrotated_surface_still_classifies_as_floor() {
    let mut body = CharacterBody2D::new(Vector2::new(0.0, -3.0), Shape2D::Circle { radius: 0.5 });
    let obstacles = vec![wall(Vector2::ZERO, Vector2::new(6.0, 0.5), 0.0)];
    let refs: Vec<&PhysicsBody2D> = obstacles.iter().collect();
    for _ in 0..8 {
        body.move_and_slide_delta(Vector2::new(0.0, 120.0), DT, &refs);
    }

    assert!(
        body.is_on_floor(),
        "a flat, unrotated surface must still classify as floor — the rotation \
         fix must not break the ordinary case"
    );
}

/// A shallow rotation stays walkable: 30° is under the 45° limit, so it is
/// floor. Together with the 60° case this pins the classification to the real
/// angle rather than to "rotated at all".
#[test]
fn a_shallow_slope_is_still_floor() {
    let shallow = 30.0_f32.to_radians();
    let mut body = CharacterBody2D::new(Vector2::new(0.0, -3.0), Shape2D::Circle { radius: 0.5 });
    let obstacles = vec![wall(Vector2::ZERO, Vector2::new(6.0, 0.5), shallow)];
    let refs: Vec<&PhysicsBody2D> = obstacles.iter().collect();
    for _ in 0..8 {
        body.move_and_slide_delta(Vector2::new(0.0, 120.0), DT, &refs);
    }

    assert!(
        body.is_on_floor(),
        "a 30° slope is under floor_max_angle and must be walkable"
    );
}

/// Slow motion is unaffected by substepping: a body that already fits inside one
/// step must land exactly where it did before, so the sweep cannot introduce
/// drift into ordinary movement.
#[test]
fn slow_motion_is_unchanged_by_sweeping() {
    let mut body = CharacterBody2D::new(Vector2::ZERO, Shape2D::Circle { radius: 5.0 });
    let obstacles: Vec<PhysicsBody2D> = Vec::new();
    let refs: Vec<&PhysicsBody2D> = obstacles.iter().collect();
    body.move_and_slide_delta(Vector2::new(60.0, 0.0), DT, &refs);

    assert!(
        (body.position.x - 1.0).abs() < 1e-4,
        "60 px/s for 1/60 s is exactly 1 unit; substepping must not change \
         unobstructed movement, got x = {}",
        body.position.x
    );
}

/// Unobstructed fast motion still completes in full — the sweep must stop the
/// body only when something is actually in the way.
#[test]
fn fast_motion_with_no_obstacle_completes() {
    let mut body = CharacterBody2D::new(Vector2::ZERO, Shape2D::Circle { radius: 0.5 });
    let obstacles: Vec<PhysicsBody2D> = Vec::new();
    let refs: Vec<&PhysicsBody2D> = obstacles.iter().collect();
    body.move_and_slide_delta(Vector2::new(600.0, 0.0), DT, &refs);

    assert!(
        (body.position.x - 10.0).abs() < 1e-3,
        "with nothing in the way the full 10 units must be travelled, got x = {}",
        body.position.x
    );
}
