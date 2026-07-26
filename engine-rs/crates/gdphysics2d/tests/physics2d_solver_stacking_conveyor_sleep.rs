//! pat-0q2kx.116: the 2D contact solver, sleeping lifecycle and conveyor
//! surfaces.
//!
//! Four things existed in `gdphysics2d` and were consulted by nothing:
//!
//! - **Sleeping.** `sleeping` was only ever set by hand in unit tests. Nothing
//!   put a body to sleep and — more seriously — **nothing anywhere cleared it**,
//!   so a body that ever slept could never be woken by gameplay again.
//! - **`constant_linear_velocity`.** Godot's conveyor property: the body does
//!   not move, but its surface drags whatever rests on it. It was stored and
//!   read by no solver code, so a conveyor was inert.
//! - **`constraint_iterations`.** A field on the world, used for joints only.
//!   Contacts were resolved once per pair, from a single point, with no impulse
//!   accumulation, so a stack of boxes could never converge.
//! - **The `physics/2d/*` project settings.** `default_linear_damp` and the
//!   sleep thresholds were never parsed, so a project that authored them
//!   simulated as if it had not.
//!
//! Godot ground truth: `modules/godot_physics_2d/godot_body_pair_2d.cpp`
//! (impulse solver, combine rules) and `godot_step_2d.cpp` (the
//! `integrate_forces → solve → integrate_velocities` order the stacking clause
//! depends on).

use gdcore::math::Vector2;
use gdphysics2d::body::{BodyId, BodyType, PhysicsBody2D};
use gdphysics2d::shape::Shape2D;
use gdphysics2d::world::{PhysicsWorld2D, SpaceParams2D};

const DT: f32 = 1.0 / 60.0;
const GRAVITY: Vector2 = Vector2 { x: 0.0, y: 980.0 };

fn dynamic_box(id: u64, position: Vector2) -> PhysicsBody2D {
    PhysicsBody2D::new(
        BodyId(id),
        BodyType::Rigid,
        position,
        Shape2D::Rectangle {
            half_extents: Vector2::new(16.0, 16.0),
        },
        1.0,
    )
}

/// A floor plus `count` 32 px boxes stacked exactly touching on top of it.
///
/// `can_sleep` is off throughout: a settled stack would otherwise fall asleep
/// and hold still for free, which would let a solver that cannot actually
/// support a stack pass the stability check.
fn stack_world(count: usize) -> (PhysicsWorld2D, Vec<BodyId>) {
    let mut world = PhysicsWorld2D::new();
    world.set_gravity(GRAVITY);

    let mut floor = PhysicsBody2D::new(
        BodyId(0),
        BodyType::Static,
        Vector2::new(0.0, 100.0),
        Shape2D::Rectangle {
            half_extents: Vector2::new(300.0, 16.0),
        },
        0.0,
    );
    floor.friction = 1.0;
    world.add_body(floor);

    // Floor top surface is y = 84; each box is 32 px tall.
    let mut ids = Vec::with_capacity(count);
    for i in 0..count {
        let mut body = dynamic_box(i as u64 + 1, Vector2::new(0.0, 68.0 - 32.0 * i as f32));
        body.can_sleep = false;
        body.friction = 1.0;
        ids.push(world.add_body(body));
    }
    (world, ids)
}

/// **The acceptance test.** Stacking stability, the conveyor drag, and the
/// sleep lifecycle, in that order.
#[test]
fn physics2d_solver_stacking_conveyor_sleep() {
    // -- (1) a three-box stack settles and then holds -----------------------
    //
    // Resolving one pair at a time re-opens the contact below every time the
    // contact above is solved, so the old path made a stack shiver and sink
    // without bound. Let it settle first, then measure: "rest" is where the
    // stack ends up, not where it was authored.
    let (mut world, ids) = stack_world(3);
    for _ in 0..120 {
        world.step(DT);
    }
    let settled: Vec<Vector2> = ids
        .iter()
        .map(|id| world.get_body(*id).unwrap().position)
        .collect();

    let mut worst_drift = 0.0_f32;
    for _ in 0..300 {
        world.step(DT);
        for (i, id) in ids.iter().enumerate() {
            let drift = (world.get_body(*id).unwrap().position - settled[i]).length();
            worst_drift = worst_drift.max(drift);
        }
    }
    assert!(
        worst_drift < 1.0,
        "a settled three-box stack must stay within 1 px over 300 ticks; the \
         worst box moved {worst_drift} px"
    );

    // And it must have settled somewhere sane rather than sunk through the
    // floor while "not drifting".
    let bottom = settled[0].y;
    assert!(
        (bottom - 68.0).abs() < 3.0,
        "the bottom box should rest on the floor near y = 68, settled at {bottom}"
    );
    assert!(
        settled[1].y < bottom - 28.0 && settled[2].y < settled[1].y - 28.0,
        "the stack must still be stacked, not collapsed into one another: {settled:?}"
    );

    // -- (3) an idle body sleeps only after time_before_sleep ---------------
    let mut body = dynamic_box(1, Vector2::ZERO);
    assert!(body.can_sleep, "bodies may sleep by default");
    assert!(!body.sleeping, "a fresh body is awake");

    // Well short of the 0.5 s default: still awake.
    for _ in 0..12 {
        body.integrate(DT, Vector2::ZERO);
    }
    assert!(
        !body.sleeping,
        "0.2 s of stillness is under time_before_sleep (0.5 s); sleeping here \
         means the countdown is being skipped"
    );

    // Past it: asleep.
    for _ in 0..30 {
        body.integrate(DT, Vector2::ZERO);
    }
    assert!(
        body.sleeping,
        "a body still for longer than time_before_sleep must fall asleep"
    );

    // -- and an impulse wakes it -------------------------------------------
    body.apply_central_impulse(Vector2::new(50.0, 0.0));
    assert!(
        !body.sleeping,
        "apply_central_impulse must wake the body — before this nothing in the \
         engine ever cleared `sleeping`, so a slept body was stuck forever"
    );
    body.integrate(DT, Vector2::ZERO);
    assert!(
        body.position.x > 0.0,
        "a woken body must actually move again, got x = {}",
        body.position.x
    );

    // -- (2) a conveyor drags a resting body --------------------------------
    //
    // The platform does NOT move; only its surface does. A solver reading the
    // rigid-body velocities alone sees a stationary platform and drags nothing.
    // Driven through the real `PhysicsWorld2D::step`, because "the world drags
    // the box" is the claim — a hand-rolled loop over the narrow phase would
    // prove something the engine never actually runs.
    let mut world = PhysicsWorld2D::new();
    world.set_gravity(GRAVITY);

    let mut belt = PhysicsBody2D::new(
        BodyId(10),
        BodyType::Static,
        Vector2::new(0.0, 100.0),
        Shape2D::Rectangle {
            half_extents: Vector2::new(200.0, 16.0),
        },
        0.0,
    );
    belt.constant_linear_velocity = Vector2::new(100.0, 0.0);
    belt.friction = 1.0;
    let belt_id = world.add_body(belt);

    let mut carried = dynamic_box(11, Vector2::new(0.0, 68.0));
    carried.friction = 1.0;
    carried.can_sleep = false;
    // Rotation locked so this measures the clause under test — does the surface
    // drag the box — and not the separate question of how far a block tips when
    // friction pushes on its base. `lock_rotation` is Godot's own property, not
    // a test-only escape hatch.
    carried.lock_rotation = true;
    let carried_id = world.add_body(carried);

    let before_x = world.get_body(carried_id).unwrap().position.x;
    for _ in 0..60 {
        world.step(DT);
    }
    let after = world.get_body(carried_id).unwrap().position;

    assert!(
        after.x > before_x + 10.0,
        "a box resting on a conveyor (constant_linear_velocity = +x) must be \
         dragged along it; x went {before_x} -> {}",
        after.x
    );
    assert_eq!(
        world.get_body(belt_id).unwrap().position,
        Vector2::new(0.0, 100.0),
        "the conveyor itself must NOT move — only its surface does"
    );
}

/// The `physics/2d/default_linear_damp` clause: the space-wide default slows a
/// body that has no damping of its own.
///
/// The value was stored on `PhysicsBody2D::linear_damp` and defaulted to zero,
/// and nothing ever read a project-wide default into it — so a `project.godot`
/// asking for damping got none.
#[test]
fn space_default_linear_damp_slows_a_gravity_free_body() {
    let coast = |damp: f32| {
        let mut world = PhysicsWorld2D::new();
        world.set_gravity(Vector2::ZERO);
        world.set_space_params(SpaceParams2D {
            default_linear_damp: damp,
            ..SpaceParams2D::default()
        });
        let mut body = dynamic_box(1, Vector2::ZERO);
        body.can_sleep = false;
        body.linear_velocity = Vector2::new(200.0, 0.0);
        let id = world.add_body(body);
        for _ in 0..60 {
            world.step(DT);
        }
        world.get_body(id).unwrap().linear_velocity.x
    };

    let undamped = coast(0.0);
    let damped = coast(1.0);
    assert!(
        (undamped - 200.0).abs() < 1e-3,
        "with no gravity and no damping the body must coast unchanged, got {undamped}"
    );
    assert!(
        damped < undamped * 0.6,
        "`default_linear_damp = 1.0` must visibly slow the body over a second: \
         {damped} against an undamped {undamped}"
    );
    assert!(
        damped > 0.0,
        "damping decays the velocity, it does not reverse it; got {damped}"
    );
}

/// The space stamps its sleep settings onto the bodies in it, including ones
/// registered before boot finished reading `project.godot`.
#[test]
fn space_params_reach_bodies_already_in_the_world() {
    let mut world = PhysicsWorld2D::new();
    let id = world.add_body(dynamic_box(1, Vector2::ZERO));
    world.set_space_params(SpaceParams2D {
        time_before_sleep: 2.0,
        sleep_threshold_linear: 42.0,
        ..SpaceParams2D::default()
    });
    let body = world.get_body(id).unwrap();
    assert!((body.time_before_sleep - 2.0).abs() < 1e-6);
    assert!((body.sleep_threshold_linear - 42.0).abs() < 1e-6);
}

/// The mirror of the sleep clause: a body that keeps moving never sleeps. This
/// is the case that "sleep as soon as speed dips below the threshold" gets
/// wrong — a ball at the apex of its arc has near-zero velocity for one instant
/// and would freeze in mid-air.
#[test]
fn a_moving_body_never_sleeps() {
    let mut body = dynamic_box(2, Vector2::ZERO);
    // Thrown upward under gravity: velocity passes through zero at the apex.
    body.linear_velocity = Vector2::new(0.0, -300.0);
    let mut slept = false;
    for _ in 0..120 {
        body.integrate(DT, GRAVITY);
        if body.sleeping {
            slept = true;
            break;
        }
    }
    assert!(
        !slept,
        "a body in free flight must never sleep, even as its velocity passes \
         through zero at the apex — that instant is not sustained stillness"
    );
}

/// `can_sleep = false` opts out entirely (Godot `RigidBody2D.can_sleep`).
#[test]
fn can_sleep_false_never_sleeps() {
    let mut body = dynamic_box(3, Vector2::ZERO);
    body.can_sleep = false;
    for _ in 0..120 {
        body.integrate(DT, Vector2::ZERO);
    }
    assert!(
        !body.sleeping,
        "can_sleep = false must keep the body awake indefinitely"
    );
}

/// A longer `time_before_sleep` genuinely delays sleeping, proving the value is
/// read rather than a hard-coded constant.
#[test]
fn time_before_sleep_is_honored() {
    let mut quick = dynamic_box(4, Vector2::ZERO);
    quick.time_before_sleep = 0.1;
    let mut slow = dynamic_box(5, Vector2::ZERO);
    slow.time_before_sleep = 2.0;

    for _ in 0..30 {
        quick.integrate(DT, Vector2::ZERO);
        slow.integrate(DT, Vector2::ZERO);
    }
    assert!(quick.sleeping, "0.1 s threshold must have elapsed by 0.5 s");
    assert!(
        !slow.sleeping,
        "a 2 s threshold must NOT have elapsed by 0.5 s — the field is being \
         ignored in favour of a constant"
    );
}

/// A sleeping body does not integrate, so gravity cannot drag it downward.
#[test]
fn a_sleeping_body_holds_position() {
    let mut body = dynamic_box(6, Vector2::ZERO);
    for _ in 0..60 {
        body.integrate(DT, Vector2::ZERO);
    }
    assert!(body.sleeping);

    let resting = body.position;
    for _ in 0..60 {
        body.integrate(DT, GRAVITY);
    }
    assert_eq!(
        body.position, resting,
        "a sleeping body must not fall under gravity"
    );
}
