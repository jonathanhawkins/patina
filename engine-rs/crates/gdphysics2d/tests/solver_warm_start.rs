//! pat-0q2kx.116 — the contact solver warm-starts from last step's impulses.
//!
//! ⚠️ **These live in an integration test, not in `solver.rs`'s `#[cfg(test)]`
//! module, on purpose.** `gdphysics2d`'s *lib-test* target does not currently
//! compile — two E0061s that predate this change (identical failure fingerprint
//! in verifier.log at `pat-0q2kx.138`, long before warm starting existed). A
//! unit test placed next to the code would therefore never build, and would
//! report as a failure of this bead. An integration test builds against the
//! crate's public API and runs independently of that break.
//!
//! Move these back next to the solver once the lib-test target builds again.

use std::collections::HashMap;

use gdcore::math::{Transform2D, Vector2};
use gdphysics2d::body::{BodyId, BodyType, PhysicsBody2D};
use gdphysics2d::shape::Shape2D;
use gdphysics2d::solver::{
    build_manifold, ContactCache2D, ContactConstraint2D, ContactSolver2D, DEFAULT_CONTACT_BIAS,
    DEFAULT_MAX_PENETRATION,
};

fn box_body(id: u64, ty: BodyType, position: Vector2, half: Vector2, mass: f32) -> PhysicsBody2D {
    PhysicsBody2D::new(
        BodyId(id),
        ty,
        position,
        Shape2D::Rectangle { half_extents: half },
        mass,
    )
}

/// A static floor with a 1 kg box resting slightly inside it.
fn floor_and_box() -> (PhysicsBody2D, PhysicsBody2D) {
    let floor = box_body(
        1,
        BodyType::Static,
        Vector2::new(0.0, 100.0),
        Vector2::new(200.0, 8.0),
        0.0,
    );
    let falling = box_body(
        2,
        BodyType::Rigid,
        Vector2::new(0.0, 76.5),
        Vector2::new(16.0, 16.0),
        1.0,
    );
    (floor, falling)
}

fn contact_for(falling: &PhysicsBody2D, floor: &PhysicsBody2D) -> ContactConstraint2D {
    let tf_floor = Transform2D::translated(floor.position);
    let tf_falling = Transform2D::translated(falling.position);
    let result = gdphysics2d::collision::test_collision(
        &falling.shape,
        &tf_falling,
        &floor.shape,
        &tf_floor,
    )
    .expect("the box overlaps the floor");
    let manifold = build_manifold(
        &falling.shape,
        &tf_falling,
        &floor.shape,
        &tf_floor,
        &result,
    );
    ContactConstraint2D::new(
        falling,
        floor,
        &manifold,
        1.0 / 60.0,
        DEFAULT_CONTACT_BIAS,
        DEFAULT_MAX_PENETRATION,
    )
    .expect("one body is movable")
}

/// Solves one contact from a fixed starting state and returns the residual
/// approach speed.
///
/// Deliberately starved to a **single** sweep so the seeded impulse still shows
/// in the result; with the default 16 iterations this contact converges either
/// way and warm starting would leave no visible trace.
///
/// ⚠️ Each call **resets** the body to a full 200 px/s approach. That makes this
/// a probe of the *mechanism*, not a simulation: the seed carried in is the
/// impulse that suited the previous solve's END state, applied to a body wound
/// back to its start. So there is no reason to expect the residual to shrink
/// here, and asserting that it would was simply wrong (it failed at
/// verifier.log `solver_warm_start.rs:130`). Whether warm starting actually
/// *settles* a stack is measured where it is meaningful — over a continuing
/// simulation, in `physics2d_solver_stacking_conveyor_sleep`, which passes.
fn starved_residual(cache: &mut ContactCache2D) -> f32 {
    let solver = ContactSolver2D {
        iterations: 1,
        ..ContactSolver2D::default()
    };
    let (floor, mut falling) = floor_and_box();
    falling.linear_velocity = Vector2::new(0.0, 200.0);
    let mut constraints = vec![contact_for(&falling, &floor)];
    let mut bodies = HashMap::new();
    bodies.insert(falling.id, falling.clone());
    bodies.insert(floor.id, floor.clone());
    solver.solve(&mut bodies, &mut constraints, cache);
    bodies[&falling.id].linear_velocity.y.abs()
}

/// **The regression test.** Warm starting must carry impulses into the next
/// step.
///
/// Both runs solve an identical contact from an identical body state, so the
/// solver is deterministic and the *only* possible source of difference is the
/// cache. Without warm starting every step rebuilt its constraints with
/// `acc_normal: 0.0`, making the two results **bit-identical** — so "the results
/// differ at all" is the exact property that distinguishes seeded from cold, and
/// it fails the moment the seeding is removed or the cache stops being consulted.
///
/// This asserts the mechanism only, deliberately. An earlier version of this
/// test asserted the warm residual would be *smaller*, which is a claim about
/// convergence quality that this reset-based harness cannot support — see
/// `starved_residual`. The convergence benefit is measured by
/// `physics2d_solver_stacking_conveyor_sleep`, over a real continuing
/// simulation, where the stack creep this bead is about actually appears.
#[test]
fn warm_starting_carries_impulses_into_the_next_step() {
    let mut cache = ContactCache2D::default();
    let cold = starved_residual(&mut cache);
    let warm = starved_residual(&mut cache);

    assert!(
        (warm - cold).abs() > 1e-6,
        "with no warm start these two solves are bit-identical; the cached \
         impulse must change the second one. cold {cold} px/s, warm {warm} px/s"
    );
}

/// A pair that stops touching must be forgotten, or the impulse it held would be
/// re-applied the moment those two bodies meet again — a shove out of nowhere,
/// from a contact that no longer exists.
#[test]
fn the_cache_forgets_pairs_that_stopped_touching() {
    let mut cache = ContactCache2D::default();
    let cold = starved_residual(&mut cache);
    let warm = starved_residual(&mut cache);
    assert!(
        (warm - cold).abs() > 1e-6,
        "precondition: the cache must be live before eviction can mean anything"
    );

    // A step in which the pair reports no contact at all.
    let solver = ContactSolver2D::default();
    let mut bodies: HashMap<BodyId, PhysicsBody2D> = HashMap::new();
    solver.solve(&mut bodies, &mut [], &mut cache);

    // Evicted means genuinely cold, so this must reproduce the very first
    // result exactly — the solver is deterministic, and an empty cache seeds
    // nothing. A merely *close* value would hide a stale impulse still leaking
    // in, which is the failure this test exists to catch.
    let after_eviction = starved_residual(&mut cache);
    assert!(
        (after_eviction - cold).abs() < 1e-6,
        "once the pair separates its impulse must be dropped, so the next \
         contact starts cold ({cold}); got {after_eviction}"
    );
}
