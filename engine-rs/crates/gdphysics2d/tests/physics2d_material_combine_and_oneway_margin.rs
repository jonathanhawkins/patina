//! pat-s3rrv.99: `PhysicsMaterial.rough` / `.absorbent` combine modes and
//! `CollisionShape2D.one_way_collision_margin`.
//!
//! ⚠️ The bead's prose describes `rough` as switching friction to
//! MULTIPLICATION (0.5 and 0.5 combining to 0.25). The vendored source does not
//! do that, and neither does the bead's own "Godot ground truth" line, which
//! quotes the real formulas. Godot 4.6 has exactly two combine functions and
//! neither has a multiplicative branch (`godot_body_pair_2d.cpp:242-248`):
//!
//! ```text
//! combine_bounce(A, B)   = CLAMP(A.bounce + B.bounce, 0, 1)
//! combine_friction(A, B) = abs(MIN(A.friction, B.friction))
//! ```
//!
//! The modes are carried as a SIGN, not as a branch: `PhysicsMaterial` stores
//! `computed_friction() { return rough ? -friction : friction; }` and
//! `computed_bounce() { return absorbent ? -bounce : bounce; }`
//! (`physics_material.h:42-67`). Reading that sign back out through the two
//! formulas above is what the modes MEAN:
//!
//! * `abs(MIN(..))` — a negative friction is the minimum of the pair, so its
//!   magnitude wins. A rough surface therefore IMPOSES its friction instead of
//!   the usual "slipperiest side wins".
//! * `CLAMP(A + B, 0, 1)` — a negative bounce subtracts, so an absorbent
//!   surface deadens a bouncy one.
//!
//! 🔑 The asymmetric fixtures below are deliberate. With both sides at 0.5 the
//! rough and non-rough results are IDENTICAL (`abs(MIN(-0.5, 0.5)) == 0.5 ==
//! MIN(0.5, 0.5)`), so the acceptance criterion as literally written — two
//! bodies at friction 0.5 each — cannot distinguish the modes under Godot's
//! real rules and would pass against a stub. Every case here picks values where
//! the two models give different answers.

use gdcore::math::{Transform2D, Vector2};
use gdphysics2d::body::{BodyId, BodyType, PhysicsBody2D, ShapeOwner2D, DEFAULT_ONE_WAY_MARGIN};
use gdphysics2d::shape::Shape2D;
use gdphysics2d::solver::{combine_bounce, combine_friction};
use gdphysics2d::world::PhysicsWorld2D;

const DT: f32 = 1.0 / 60.0;

/// Godot's `computed_friction()` / `computed_bounce()`: the mode is the sign.
fn computed(value: f32, mode: bool) -> f32 {
    if mode {
        -value
    } else {
        value
    }
}

#[test]
fn rough_makes_a_surface_impose_its_friction_instead_of_the_minimum() {
    // A slippery crate (0.2) on a rough road (0.9). Without `rough`, Godot's
    // "slipperiest side wins" gives 0.2. With it, the road's 0.9 is imposed.
    let slippery = 0.2_f32;
    let road = 0.9_f32;

    let plain = combine_friction(slippery, computed(road, false));
    let rough = combine_friction(slippery, computed(road, true));

    assert!(
        (plain - 0.2).abs() < 1e-6,
        "without `rough` the pair takes the MINIMUM friction: expected 0.2, got {plain}"
    );
    assert!(
        (rough - 0.9).abs() < 1e-6,
        "`rough` negates the stored friction, so `abs(MIN(..))` returns the \
         rough surface's own magnitude: expected 0.9, got {rough}"
    );
    assert!(
        rough > plain,
        "the whole point of `rough` is that the pair ends up grippier: \
         rough {rough} must exceed plain {plain}"
    );

    // ⚠️ The acceptance criterion's own fixture, recorded as a NEGATIVE result:
    // at equal friction the flag cannot be observed at all. A test written to
    // the bead's wording would assert 0.25 here and fail against correct code.
    let equal_plain = combine_friction(0.5, computed(0.5, false));
    let equal_rough = combine_friction(0.5, computed(0.5, true));
    assert!(
        (equal_plain - equal_rough).abs() < 1e-6,
        "at equal friction `rough` is INDISTINGUISHABLE under Godot's rules \
         (plain {equal_plain}, rough {equal_rough}); if these ever differ, the \
         combine rule has been changed to something Godot does not do"
    );
}

#[test]
fn absorbent_subtracts_bounce_instead_of_adding_it() {
    let ball = 0.5_f32;
    let surface = 0.8_f32;

    let plain = combine_bounce(ball, computed(surface, false));
    let absorbent = combine_bounce(ball, computed(surface, true));

    assert!(
        (plain - 1.0).abs() < 1e-6,
        "0.5 + 0.8 clamps to a fully elastic 1.0, got {plain}"
    );
    assert!(
        absorbent < plain,
        "`absorbent` must DEADEN the pair: got {absorbent}, which is not below \
         the non-absorbent {plain}"
    );
    assert!(
        (absorbent - 0.0).abs() < 1e-6,
        "0.5 + (-0.8) = -0.3, clamped to 0 — an absorbent surface swallows a \
         bouncier ball entirely; got {absorbent}"
    );
}

/// The acceptance criterion's simulation form: a body dropped on an absorbent
/// surface must reach a lower apex than on a non-absorbent one of equal bounce.
#[test]
fn a_body_bounces_lower_off_an_absorbent_surface() {
    fn apex_after_bounce(absorbent: bool) -> f32 {
        let mut world = PhysicsWorld2D::new();

        let mut ground = PhysicsBody2D::new(
            BodyId(1),
            BodyType::Static,
            Vector2::new(0.0, 100.0),
            Shape2D::Rectangle {
                half_extents: Vector2::new(500.0, 10.0),
            },
            0.0,
        );
        ground.bounce = computed(0.8, absorbent);
        ground.friction = 0.0;
        world.add_body(ground);

        let mut ball = PhysicsBody2D::new(
            BodyId(2),
            BodyType::Rigid,
            Vector2::new(0.0, 0.0),
            Shape2D::Circle { radius: 10.0 },
            1.0,
        );
        ball.bounce = 0.5;
        ball.friction = 0.0;
        let ball_id = world.add_body(ball);

        // Fall, hit, and rebound. The apex is the highest point (smallest y,
        // since +Y is down) reached after the first contact.
        let mut highest = f32::MAX;
        let mut has_touched = false;
        for _ in 0..400 {
            world.step(DT);
            let y = world.get_body(ball_id).map(|b| b.position.y).unwrap_or(0.0);
            // ⚠️ NOT `y > 80.0`. The ball's resting centre is exactly y=80
            // (ground top 90, radius 10), so that threshold sits ON the
            // coincidence point and would flip on rounding. 75 is unambiguously
            // "has arrived at the ground" and unambiguously below the start.
            if y > 75.0 {
                has_touched = true;
            }
            if has_touched {
                highest = highest.min(y);
            }
        }
        assert!(has_touched, "fixture: the ball never reached the ground");
        highest
    }

    let plain = apex_after_bounce(false);
    let absorbed = apex_after_bounce(true);

    // Smaller y is higher, so the absorbent run must NOT climb as high.
    assert!(
        absorbed > plain,
        "a body dropped on an `absorbent` surface must rebound LOWER than on a \
         non-absorbent one of the same bounce magnitude: absorbent apex y={absorbed} \
         should be below (greater y than) the plain apex y={plain}"
    );
}

/// `one_way_collision_margin`: the deepest penetration at which a one-way shape
/// still blocks. Past it the mover has effectively passed through.
#[test]
fn one_way_margin_decides_whether_a_deep_contact_still_blocks() {
    /// Drops a fast body onto a one-way platform whose margin is `margin`, and
    /// reports whether it ended up below the platform (clipped through).
    fn fell_through(margin: f32) -> bool {
        let mut world = PhysicsWorld2D::new();

        let mut platform = PhysicsBody2D::new(
            BodyId(1),
            BodyType::Static,
            Vector2::new(0.0, 200.0),
            Shape2D::Rectangle {
                half_extents: Vector2::new(200.0, 5.0),
            },
            0.0,
        );
        platform.one_way_collision = true;
        // Blocking face up: +Y is down in Godot 2D, so "up" is -Y.
        platform.one_way_direction = Vector2::new(0.0, -1.0);
        platform.one_way_margin = margin;
        world.add_body(platform);

        let mut faller = PhysicsBody2D::new(
            BodyId(2),
            BodyType::Rigid,
            Vector2::new(0.0, 150.0),
            Shape2D::Circle { radius: 8.0 },
            1.0,
        );
        // ⚠️ 300 px/s (5 px per 1/60s step), NOT the 1800 this first used.
        // At 1800 the body advances 30 px per step against a 10 px-thick
        // platform and TUNNELS clean past it — no frame ever sees an overlap,
        // so it "fell through" no matter what the margin said. That measures
        // the absence of continuous collision detection, not this property.
        //
        // At 5 px/step the first contact lands ~3 px deep: deeper than a
        // rejecting margin of 0, shallower than a permissive one, which is
        // exactly the gap the margin is supposed to decide.
        faller.linear_velocity = Vector2::new(0.0, 300.0);
        // Gravity off so the step stays a constant 5 px and the contact depth
        // is a property of the fixture rather than of how long it fell.
        faller.gravity_scale = 0.0;
        faller.bounce = 0.0;
        let faller_id = world.add_body(faller);

        for _ in 0..120 {
            world.step(DT);
        }
        let y = world
            .get_body(faller_id)
            .map(|b| b.position.y)
            .unwrap_or(0.0);
        y > 260.0
    }

    // A generous margin keeps the deep contact valid, so the platform blocks.
    let blocked = !fell_through(400.0);
    // A zero margin rejects any penetrating contact, so the body clips through.
    let clipped = fell_through(0.0);

    assert!(
        blocked,
        "a one-way platform with a permissive `one_way_collision_margin` must \
         still block a body whose first contact is already a few px deep; it \
         fell through instead"
    );
    assert!(
        clipped,
        "with `one_way_collision_margin` 0 every penetrating contact is \
         rejected, so the fast body must clip through; it was blocked"
    );
}

#[test]
fn the_one_way_margin_default_matches_godot() {
    assert!(
        (DEFAULT_ONE_WAY_MARGIN - 1.0).abs() < 1e-6,
        "Godot's `CollisionShape2D.one_way_collision_margin` defaults to 1.0 px"
    );

    // A shape built through the solid() constructor is two-way, so the margin
    // must not accidentally gate a normal collider.
    let owner = ShapeOwner2D::solid(
        Shape2D::Circle { radius: 4.0 },
        Transform2D::translated(Vector2::ZERO),
    );
    assert!(
        !owner.one_way_collision,
        "a solid owner must stay two-way, else the margin gate would apply to it"
    );
}
