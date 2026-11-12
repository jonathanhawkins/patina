//! pat-0q2kx.205 (partial): behavioural coverage of the EXISTING vehicle sim.
//!
//! ⚠️ The bead says "Patina has the ClassDB surface only" and "Zero simulation",
//! and asks for a *new* `vehicle.rs` in `gdphysics3d`. That premise is wrong:
//! `gdphysics2d/src/vehicle3d.rs` is a 373-line raycast-style vehicle with
//! suspension spring/damper, engine force, brake, steering and lateral tire
//! friction. The audit's grep covered `gdscene/src`, `gdphysics3d/src`,
//! `gdscript-interop/src` and `patina-runner/src` — not `gdphysics2d` — so it
//! reported the sim as absent rather than as unreachable. Writing a second one
//! would have duplicated working code.
//!
//! What is actually true: the sim has **no non-test caller**, and it grounds
//! against a flat `ground_y: f32` rather than querying world bodies. So the
//! bead's remaining work is *wiring plus a ray-query ground*, not a rewrite.
//!
//! These tests pin the four behaviours the acceptance criterion names, at
//! library level against the flat ground. They are NOT the acceptance test
//! `physics3d_vehicle_raycast_sim`, which needs a `.tscn` and scene wiring.

use gdphysics2d::vehicle3d::{VehicleBody3D, VehicleBodyId3D, VehicleWheel3D};

use gdcore::math::Vector3;

const GROUND: f32 = 0.0;
const DT: f32 = 1.0 / 60.0;
const GRAVITY: Vector3 = Vector3::new(0.0, -9.8, 0.0);
const MASS: f32 = 100.0;

/// Half-height of the chassis box. The sim clamps the chassis to
/// `ground_y + half_extents.y` as a floor of last resort, so this value is what
/// "resting on its collision box" means — the suspension tests below are only
/// meaningful because the spring settles the chassis well above it.
const HALF_HEIGHT: f32 = 0.4;

/// Four wheels mounted below the chassis centre, at the corners.
fn car() -> VehicleBody3D {
    let mut v = VehicleBody3D::new(
        VehicleBodyId3D(1),
        Vector3::new(0.0, 1.5, 0.0),
        Vector3::new(0.8, HALF_HEIGHT, 1.6),
        MASS,
    );
    for (x, z) in [(-0.7, 1.4), (0.7, 1.4), (-0.7, -1.4), (0.7, -1.4)] {
        let mut w = VehicleWheel3D::new(Vector3::new(x, -0.2, z));
        // Front wheels steer; all four drive.
        w.steerable = z > 0.0;
        v.add_wheel(w);
    }
    v
}

fn settle(v: &mut VehicleBody3D, ticks: usize) {
    for _ in 0..ticks {
        v.step(DT, GRAVITY, GROUND);
    }
}

/// Dropped onto the floor, the car comes to rest on its suspension — not on its
/// collision box, and not still falling.
#[test]
fn dropped_car_settles_on_its_suspension() {
    let mut v = car();
    settle(&mut v, 240);

    assert!(
        v.wheels.iter().all(|w| w.in_contact),
        "every wheel must report ground contact once settled"
    );

    let y = v.position().y;

    // The distinction the acceptance criterion draws: held up by springs, not
    // sitting on the box. The clamp in `step` would also produce a "stable"
    // height, so a bare "it stopped falling" assertion cannot tell the two
    // apart — this is the assertion that can.
    assert!(
        y > GROUND + HALF_HEIGHT + 0.05,
        "the chassis must rest on its suspension, clearly above the \
         collision-box clamp at {} (got {y})",
        GROUND + HALF_HEIGHT
    );

    // Springs compress under load, so it must also sit BELOW the fully
    // extended height. Together these bracket a real equilibrium.
    let extended = GROUND + 0.4 + 0.3 + 0.2; // rest_length + radius - attachment.y
    assert!(
        y < extended,
        "the suspension must be compressed under the car's weight, so the \
         chassis sits below the fully extended height {extended} (got {y})"
    );

    assert!(
        v.wheels.iter().all(|w| w.compression > 0.0),
        "a loaded suspension must report non-zero compression"
    );

    // Actually at rest, not drifting through.
    let before = v.position().y;
    settle(&mut v, 60);
    assert!(
        (v.position().y - before).abs() < 0.01,
        "the settled height must be stable across another second \
         ({before} -> {})",
        v.position().y
    );
}

/// `engine_force` accelerates the car along its forward axis.
#[test]
fn engine_force_accelerates_the_car() {
    let mut v = car();
    settle(&mut v, 120);

    let forward = v.forward();
    let start = v.linear_velocity().dot(forward);

    v.engine_force = 200.0;
    settle(&mut v, 120);

    let speed = v.linear_velocity().dot(v.forward());
    // Total drive force is `engine_force` (split per driven wheel, then applied
    // once per driven wheel), so 200 N on 100 kg is 2 m/s^2 -> ~4 m/s over two
    // seconds. The criterion's 2 m/s bar leaves room for suspension losses.
    assert!(
        speed > 2.0,
        "engine_force = 200 must accelerate the car past 2 m/s within 120 \
         ticks (start {start}, got {speed})"
    );
}

/// A car with no engine force does not accelerate. Without this, "it moved"
/// proves nothing about `engine_force`.
#[test]
fn a_coasting_car_does_not_accelerate() {
    let mut v = car();
    settle(&mut v, 240);

    let speed = v.linear_velocity().dot(v.forward());
    assert!(
        speed.abs() < 0.1,
        "a car with engine_force = 0 must not drive itself (got {speed})"
    );
}

/// Steering rotates the heading, and only while the car is moving — the
/// bicycle model's yaw rate is proportional to forward speed.
#[test]
fn steering_curves_the_path_only_when_moving() {
    // Stationary: steering must not spin the car on the spot.
    let mut parked = car();
    settle(&mut parked, 120);
    let heading_before = parked.forward();
    parked.set_steering(0.3);
    settle(&mut parked, 120);
    let dot = parked.forward().dot(heading_before).clamp(-1.0, 1.0);
    assert!(
        dot > 0.999,
        "a stationary car must not yaw when steered — the yaw rate is \
         proportional to forward speed (heading dot {dot})"
    );

    // Driving: the same steering input curves the path.
    let mut driving = car();
    settle(&mut driving, 120);
    driving.engine_force = 200.0;
    settle(&mut driving, 60);

    let heading_before = driving.forward();
    driving.set_steering(0.3);
    settle(&mut driving, 120);

    let dot = driving.forward().dot(heading_before).clamp(-1.0, 1.0);
    assert!(
        dot < 0.99,
        "steering 0.3 must measurably rotate a moving car's heading \
         (heading dot {dot}, 1.0 = unchanged)"
    );
}

/// `set_steering` clamps to `max_steer` rather than accepting any angle.
#[test]
fn steering_is_clamped_to_max_steer() {
    let mut v = car();
    v.set_steering(10.0);
    assert!(
        (v.steering - v.max_steer).abs() < 1e-6,
        "over-range steering must clamp to max_steer {} (got {})",
        v.max_steer,
        v.steering
    );
    v.set_steering(-10.0);
    assert!(
        (v.steering + v.max_steer).abs() < 1e-6,
        "negative over-range steering must clamp to -max_steer (got {})",
        v.steering
    );
}

/// Braking brings a moving car to a stop without throwing it into reverse.
#[test]
fn brake_stops_the_car_without_reversing() {
    let mut v = car();
    settle(&mut v, 120);
    v.engine_force = 200.0;
    settle(&mut v, 120);

    let moving = v.linear_velocity().dot(v.forward());
    assert!(
        moving > 2.0,
        "precondition: the car is rolling (got {moving})"
    );

    v.engine_force = 0.0;
    v.brake = 5000.0;
    settle(&mut v, 120);

    let stopped = v.linear_velocity().dot(v.forward());
    assert!(
        stopped.abs() < 0.5,
        "braking must bring the car to rest (got {stopped})"
    );
    // The brake is capped so it cannot push the car backwards — a naive
    // opposing force would overshoot into reverse and oscillate.
    assert!(
        stopped > -0.5,
        "the brake must not reverse the car (got {stopped})"
    );
}
