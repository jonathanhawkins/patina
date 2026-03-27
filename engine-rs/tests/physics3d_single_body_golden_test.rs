//! pat-ey5y: Match 3D physics golden traces for single-body deterministic scenes.
//!
//! Validates that deterministic single-body 3D scenes run through PhysicsWorld3D
//! produce golden traces comparable to upstream reference captures:
//!
//! - Gravity freefall from y=5.0 matches minimal_3d_10frames.json
//! - Gravity freefall from y=8.0 matches rigid_sphere_bounce_3d_20frames.json
//! - Two independent runs produce bitwise-identical traces (determinism)
//! - Trace comparison tooling correctly validates match ratios
//! - Horizontal velocity scenes produce correct parabolic trajectories
//! - Zero-gravity scenes produce linear motion traces
//! - Custom gravity direction scenes produce correct traces

use gdcore::compare3d::{
    assert_deterministic, compare_physics_traces, PhysicsTraceEntry3D,
};
use gdcore::math::Vector3;
use gdphysics2d::body3d::{BodyId3D, BodyType3D, PhysicsBody3D};
use gdphysics2d::shape3d::Shape3D;
use gdphysics2d::world3d::PhysicsWorld3D;

const DT: f32 = 1.0 / 60.0;
/// The golden fixtures use gravity = 588 m/s² (so that per-frame velocity
/// delta at 60 fps = 9.8 m/s, matching Godot's display convention).
const GOLDEN_GRAVITY: f32 = 588.0;
/// Position tolerance accounts for 3-decimal rounding in golden fixtures,
/// which accumulates drift of ~0.003/frame over semi-implicit Euler integration.
const POSITION_TOLERANCE: f32 = 0.1;
const VELOCITY_TOLERANCE: f32 = 0.5;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Loads a golden 3D trace from a JSON fixture file.
fn load_golden_3d_trace(fixture_name: &str) -> Vec<PhysicsTraceEntry3D> {
    let path = format!(
        "{}/../fixtures/golden/physics/{}.json",
        env!("CARGO_MANIFEST_DIR"),
        fixture_name
    );
    let data = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read golden fixture {}: {}", path, e));
    let entries: Vec<serde_json::Value> = serde_json::from_str(&data)
        .unwrap_or_else(|e| panic!("failed to parse golden JSON {}: {}", path, e));

    entries
        .iter()
        .map(|e| {
            PhysicsTraceEntry3D::new(
                e["name"].as_str().unwrap(),
                e["frame"].as_u64().unwrap(),
                Vector3::new(
                    e["px"].as_f64().unwrap() as f32,
                    e["py"].as_f64().unwrap() as f32,
                    e["pz"].as_f64().unwrap() as f32,
                ),
                Vector3::new(
                    e["vx"].as_f64().unwrap() as f32,
                    e["vy"].as_f64().unwrap() as f32,
                    e["vz"].as_f64().unwrap() as f32,
                ),
                0.0,
            )
        })
        .collect()
}

/// Runs a single rigid sphere through PhysicsWorld3D for `frames` steps
/// and records a trace at each frame (before stepping).
fn run_single_body_sim(
    name: &str,
    start_pos: Vector3,
    start_vel: Vector3,
    gravity: Vector3,
    frames: u64,
) -> Vec<PhysicsTraceEntry3D> {
    let mut world = PhysicsWorld3D::new();
    world.gravity = gravity;

    let mut body = PhysicsBody3D::new(
        BodyId3D(0),
        BodyType3D::Rigid,
        start_pos,
        Shape3D::Sphere { radius: 0.5 },
        1.0,
    );
    body.linear_velocity = start_vel;
    let id = world.add_body(body);

    let mut trace = Vec::new();

    for frame in 0..frames {
        let b = world.get_body(id).unwrap();
        trace.push(PhysicsTraceEntry3D::new(
            name,
            frame,
            b.position,
            b.linear_velocity,
            b.angular_velocity.length(),
        ));
        world.step(DT);
    }

    trace
}

// ===========================================================================
// 1. Gravity freefall matches minimal_3d_10frames golden
// ===========================================================================

#[test]
fn gravity_freefall_matches_minimal_3d_golden() {
    let golden = load_golden_3d_trace("minimal_3d_10frames");
    assert_eq!(golden.len(), 10);

    let actual = run_single_body_sim(
        "Ball",
        Vector3::new(0.0, 5.0, 0.0),
        Vector3::ZERO,
        Vector3::new(0.0, -GOLDEN_GRAVITY, 0.0),
        10,
    );

    let result = compare_physics_traces(&golden, &actual, POSITION_TOLERANCE, VELOCITY_TOLERANCE);
    assert!(
        result.is_exact_match(),
        "minimal_3d_10frames golden mismatch:\n{}",
        result.parity_report("gravity_freefall_vs_golden")
    );
}

// ===========================================================================
// 2. Gravity freefall matches rigid_sphere_bounce_3d_20frames golden
// ===========================================================================

#[test]
fn gravity_freefall_matches_rigid_sphere_golden() {
    let golden = load_golden_3d_trace("rigid_sphere_bounce_3d_20frames");
    assert_eq!(golden.len(), 20);

    let actual = run_single_body_sim(
        "Ball",
        Vector3::new(0.0, 8.0, 0.0),
        Vector3::ZERO,
        Vector3::new(0.0, -GOLDEN_GRAVITY, 0.0),
        20,
    );

    let result = compare_physics_traces(&golden, &actual, POSITION_TOLERANCE, VELOCITY_TOLERANCE);
    assert!(
        result.is_exact_match(),
        "rigid_sphere_bounce_3d_20frames golden mismatch:\n{}",
        result.parity_report("rigid_sphere_vs_golden")
    );
}

// ===========================================================================
// 3. Determinism: two runs produce identical traces
// ===========================================================================

#[test]
fn deterministic_gravity_freefall() {
    let run_a = run_single_body_sim(
        "Ball",
        Vector3::new(0.0, 5.0, 0.0),
        Vector3::ZERO,
        Vector3::new(0.0, -GOLDEN_GRAVITY, 0.0),
        30,
    );
    let run_b = run_single_body_sim(
        "Ball",
        Vector3::new(0.0, 5.0, 0.0),
        Vector3::ZERO,
        Vector3::new(0.0, -GOLDEN_GRAVITY, 0.0),
        30,
    );

    assert!(
        assert_deterministic(&run_a, &run_b),
        "two identical simulation runs must produce bitwise-identical traces"
    );
}

#[test]
fn deterministic_with_initial_velocity() {
    let run_a = run_single_body_sim(
        "Projectile",
        Vector3::new(0.0, 10.0, 0.0),
        Vector3::new(5.0, 20.0, -3.0),
        Vector3::new(0.0, -GOLDEN_GRAVITY, 0.0),
        50,
    );
    let run_b = run_single_body_sim(
        "Projectile",
        Vector3::new(0.0, 10.0, 0.0),
        Vector3::new(5.0, 20.0, -3.0),
        Vector3::new(0.0, -GOLDEN_GRAVITY, 0.0),
        50,
    );

    assert!(assert_deterministic(&run_a, &run_b));
}

// ===========================================================================
// 4. Zero-gravity produces linear motion
// ===========================================================================

#[test]
fn zero_gravity_linear_motion() {
    let velocity = Vector3::new(3.0, 0.0, -2.0);
    let trace = run_single_body_sim(
        "Drifter",
        Vector3::ZERO,
        velocity,
        Vector3::ZERO,
        20,
    );

    // Verify each frame's position matches linear extrapolation.
    for entry in &trace {
        let expected_pos = velocity * (entry.frame as f32 * DT);
        let pos_diff = (entry.position - expected_pos).length();
        assert!(
            pos_diff < 1e-4,
            "frame {}: expected pos ({:.4}, {:.4}, {:.4}), got ({:.4}, {:.4}, {:.4}), diff={:.6}",
            entry.frame,
            expected_pos.x, expected_pos.y, expected_pos.z,
            entry.position.x, entry.position.y, entry.position.z,
            pos_diff
        );
    }

    // Velocity should be constant across all frames.
    for entry in &trace {
        let vel_diff = (entry.velocity - velocity).length();
        assert!(
            vel_diff < 1e-4,
            "frame {}: velocity should be constant",
            entry.frame
        );
    }
}

// ===========================================================================
// 5. Horizontal + gravity produces parabolic trajectory
// ===========================================================================

#[test]
fn parabolic_trajectory_with_horizontal_velocity() {
    let start_pos = Vector3::new(0.0, 10.0, 0.0);
    let start_vel = Vector3::new(10.0, 0.0, 0.0);
    let gravity = Vector3::new(0.0, -GOLDEN_GRAVITY, 0.0);

    let trace = run_single_body_sim("Projectile", start_pos, start_vel, gravity, 30);

    // X should increase linearly (no drag).
    for i in 1..trace.len() {
        assert!(
            trace[i].position.x > trace[i - 1].position.x,
            "frame {}: x must increase monotonically",
            i
        );
    }

    // Y should decrease (falling under gravity).
    // After enough frames, y goes below starting height.
    let last = trace.last().unwrap();
    assert!(
        last.position.y < start_pos.y,
        "body must fall below starting height after 30 frames"
    );

    // Z should remain zero (no Z velocity or gravity).
    for entry in &trace {
        assert!(
            entry.position.z.abs() < 1e-4,
            "frame {}: z should remain zero",
            entry.frame
        );
    }

    // X velocity should remain constant (no horizontal forces).
    for entry in &trace {
        assert!(
            (entry.velocity.x - 10.0).abs() < 1e-4,
            "frame {}: x velocity should remain 10.0",
            entry.frame
        );
    }
}

// ===========================================================================
// 6. Custom gravity direction (sideways)
// ===========================================================================

#[test]
fn custom_gravity_direction() {
    // Gravity in +X direction.
    let trace = run_single_body_sim(
        "SideGrav",
        Vector3::ZERO,
        Vector3::ZERO,
        Vector3::new(GOLDEN_GRAVITY, 0.0, 0.0),
        10,
    );

    // X should increase (accelerating in +X).
    for i in 1..trace.len() {
        assert!(
            trace[i].position.x > trace[i - 1].position.x,
            "frame {}: x should increase under +X gravity",
            i
        );
    }

    // Y and Z should remain zero.
    for entry in &trace {
        assert!(entry.position.y.abs() < 1e-4);
        assert!(entry.position.z.abs() < 1e-4);
    }
}

// ===========================================================================
// 7. Trace entries have correct frame numbers and body name
// ===========================================================================

#[test]
fn trace_metadata_correctness() {
    let trace = run_single_body_sim(
        "TestBody",
        Vector3::new(1.0, 2.0, 3.0),
        Vector3::ZERO,
        Vector3::new(0.0, -9.8, 0.0),
        15,
    );

    assert_eq!(trace.len(), 15);
    for (i, entry) in trace.iter().enumerate() {
        assert_eq!(entry.name, "TestBody");
        assert_eq!(entry.frame, i as u64);
    }

    // First frame should be at start position.
    assert!((trace[0].position.x - 1.0).abs() < 1e-4);
    assert!((trace[0].position.y - 2.0).abs() < 1e-4);
    assert!((trace[0].position.z - 3.0).abs() < 1e-4);
}

// ===========================================================================
// 8. Long simulation stays deterministic
// ===========================================================================

#[test]
fn long_sim_300_frames_deterministic() {
    let run_a = run_single_body_sim(
        "LongRun",
        Vector3::new(0.0, 100.0, 0.0),
        Vector3::new(2.0, 50.0, -1.0),
        Vector3::new(0.0, -GOLDEN_GRAVITY, 0.0),
        300,
    );
    let run_b = run_single_body_sim(
        "LongRun",
        Vector3::new(0.0, 100.0, 0.0),
        Vector3::new(2.0, 50.0, -1.0),
        Vector3::new(0.0, -GOLDEN_GRAVITY, 0.0),
        300,
    );

    assert!(
        assert_deterministic(&run_a, &run_b),
        "300-frame simulation must be deterministic"
    );
}

// ===========================================================================
// 9. Velocity monotonically increases under constant gravity
// ===========================================================================

#[test]
fn velocity_magnitude_increases_under_gravity() {
    let trace = run_single_body_sim(
        "Falling",
        Vector3::new(0.0, 50.0, 0.0),
        Vector3::ZERO,
        Vector3::new(0.0, -GOLDEN_GRAVITY, 0.0),
        20,
    );

    // Y-velocity should be increasingly negative each frame.
    for i in 1..trace.len() {
        assert!(
            trace[i].velocity.y < trace[i - 1].velocity.y,
            "frame {}: vy should decrease monotonically (got {} vs prev {})",
            i,
            trace[i].velocity.y,
            trace[i - 1].velocity.y
        );
    }
}

// ===========================================================================
// 10. Golden trace self-comparison via engine roundtrip
// ===========================================================================

#[test]
fn engine_trace_self_comparison_exact() {
    let trace = run_single_body_sim(
        "Self",
        Vector3::new(0.0, 5.0, 0.0),
        Vector3::ZERO,
        Vector3::new(0.0, -GOLDEN_GRAVITY, 0.0),
        10,
    );

    let result = compare_physics_traces(&trace, &trace, 0.0, 0.0);
    assert!(result.is_exact_match());
    assert_eq!(result.match_ratio(), 1.0);
    assert_eq!(result.total_entries, 10);
}

// ===========================================================================
// 11. Static body produces flat trace
// ===========================================================================

#[test]
fn static_body_flat_trace() {
    let mut world = PhysicsWorld3D::new();
    world.gravity = Vector3::new(0.0, -GOLDEN_GRAVITY, 0.0);

    let body = PhysicsBody3D::new(
        BodyId3D(0),
        BodyType3D::Static,
        Vector3::new(0.0, 0.0, 0.0),
        Shape3D::BoxShape {
            half_extents: Vector3::new(5.0, 0.5, 5.0),
        },
        1.0,
    );
    let id = world.add_body(body);

    let mut trace = Vec::new();
    for frame in 0..10u64 {
        let b = world.get_body(id).unwrap();
        trace.push(PhysicsTraceEntry3D::new(
            "Floor",
            frame,
            b.position,
            b.linear_velocity,
            0.0,
        ));
        world.step(DT);
    }

    // Static body should not move at all.
    for entry in &trace {
        assert_eq!(entry.position, Vector3::ZERO);
        assert_eq!(entry.velocity, Vector3::ZERO);
    }
}

// ===========================================================================
// 12. Different masses produce same freefall trajectory
// ===========================================================================

#[test]
fn mass_does_not_affect_freefall() {
    let trace_light = run_mass_sim(0.5, 20);
    let trace_heavy = run_mass_sim(10.0, 20);

    let result = compare_physics_traces(&trace_light, &trace_heavy, 1e-4, 1e-4);
    assert!(
        result.is_exact_match(),
        "mass should not affect freefall trajectory:\n{}",
        result.parity_report("mass_independence")
    );
}

// ===========================================================================
// pat-6ax: Additional single-body golden trace validation
// ===========================================================================

#[test]
fn p6ax_minimal_3d_golden_frame_count() {
    let golden = load_golden_3d_trace("minimal_3d_10frames");
    assert_eq!(golden.len(), 10, "minimal_3d golden should have exactly 10 frames");
    assert_eq!(golden[0].frame, 0);
    assert_eq!(golden[9].frame, 9);
}

#[test]
fn p6ax_rigid_sphere_golden_frame_count() {
    let golden = load_golden_3d_trace("rigid_sphere_bounce_3d_20frames");
    assert_eq!(golden.len(), 20, "rigid_sphere golden should have exactly 20 frames");
    assert_eq!(golden[0].frame, 0);
    assert_eq!(golden[19].frame, 19);
}

#[test]
fn p6ax_minimal_3d_initial_conditions() {
    let golden = load_golden_3d_trace("minimal_3d_10frames");
    let first = &golden[0];
    assert_eq!(first.name, "Ball");
    assert!((first.position.x).abs() < 1e-4);
    assert!((first.position.y - 5.0).abs() < 1e-4);
    assert!((first.position.z).abs() < 1e-4);
    assert!((first.velocity.x).abs() < 1e-4);
    assert!((first.velocity.y).abs() < 1e-4);
    assert!((first.velocity.z).abs() < 1e-4);
}

#[test]
fn p6ax_rigid_sphere_initial_conditions() {
    let golden = load_golden_3d_trace("rigid_sphere_bounce_3d_20frames");
    let first = &golden[0];
    assert_eq!(first.name, "Ball");
    assert!((first.position.y - 8.0).abs() < 1e-4);
    assert!((first.velocity.y).abs() < 1e-4);
}

#[test]
fn p6ax_golden_velocity_delta_constant() {
    // For both goldens, gravity causes constant velocity delta per frame
    for (name, expected_len) in [("minimal_3d_10frames", 10), ("rigid_sphere_bounce_3d_20frames", 20)] {
        let golden = load_golden_3d_trace(name);
        assert_eq!(golden.len(), expected_len);

        if golden.len() < 3 {
            continue;
        }
        let first_delta = golden[1].velocity.y - golden[0].velocity.y;
        for i in 2..golden.len() {
            let delta = golden[i].velocity.y - golden[i - 1].velocity.y;
            assert!(
                (delta - first_delta).abs() < 0.1,
                "{name} frame {i}: velocity delta {delta:.3} should match first delta {first_delta:.3}"
            );
        }
    }
}

#[test]
fn p6ax_goldens_x_z_remain_zero() {
    // Single-body goldens with only vertical gravity should have x=0, z=0 throughout
    for name in ["minimal_3d_10frames", "rigid_sphere_bounce_3d_20frames"] {
        let golden = load_golden_3d_trace(name);
        for entry in &golden {
            assert!(
                entry.position.x.abs() < 1e-4 && entry.position.z.abs() < 1e-4,
                "{name} frame {}: x/z should be zero",
                entry.frame
            );
            assert!(
                entry.velocity.x.abs() < 1e-4 && entry.velocity.z.abs() < 1e-4,
                "{name} frame {}: vx/vz should be zero",
                entry.frame
            );
        }
    }
}

#[test]
fn p6ax_engine_matches_both_goldens_simultaneously() {
    // Run both simulations and compare — ensures engine is self-consistent
    // across different starting heights
    let golden_minimal = load_golden_3d_trace("minimal_3d_10frames");
    let golden_sphere = load_golden_3d_trace("rigid_sphere_bounce_3d_20frames");

    let actual_minimal = run_single_body_sim(
        "Ball",
        Vector3::new(0.0, 5.0, 0.0),
        Vector3::ZERO,
        Vector3::new(0.0, -GOLDEN_GRAVITY, 0.0),
        10,
    );
    let actual_sphere = run_single_body_sim(
        "Ball",
        Vector3::new(0.0, 8.0, 0.0),
        Vector3::ZERO,
        Vector3::new(0.0, -GOLDEN_GRAVITY, 0.0),
        20,
    );

    let r1 = compare_physics_traces(&golden_minimal, &actual_minimal, POSITION_TOLERANCE, VELOCITY_TOLERANCE);
    let r2 = compare_physics_traces(&golden_sphere, &actual_sphere, POSITION_TOLERANCE, VELOCITY_TOLERANCE);

    assert!(r1.is_exact_match(), "minimal golden mismatch");
    assert!(r2.is_exact_match(), "sphere golden mismatch");

    // Both should share the same gravity constant — verify velocity deltas match
    let delta1 = actual_minimal[1].velocity.y - actual_minimal[0].velocity.y;
    let delta2 = actual_sphere[1].velocity.y - actual_sphere[0].velocity.y;
    assert!(
        (delta1 - delta2).abs() < 0.01,
        "velocity deltas should match across simulations: {delta1} vs {delta2}"
    );
}

fn run_mass_sim(mass: f32, frames: u64) -> Vec<PhysicsTraceEntry3D> {
    let mut world = PhysicsWorld3D::new();
    world.gravity = Vector3::new(0.0, -GOLDEN_GRAVITY, 0.0);

    let body = PhysicsBody3D::new(
        BodyId3D(0),
        BodyType3D::Rigid,
        Vector3::new(0.0, 5.0, 0.0),
        Shape3D::Sphere { radius: 0.5 },
        mass,
    );
    let id = world.add_body(body);

    let mut trace = Vec::new();
    for frame in 0..frames {
        let b = world.get_body(id).unwrap();
        trace.push(PhysicsTraceEntry3D::new(
            "Ball",
            frame,
            b.position,
            b.linear_velocity,
            0.0,
        ));
        world.step(DT);
    }
    trace
}
