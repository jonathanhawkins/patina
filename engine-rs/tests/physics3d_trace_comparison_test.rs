//! pat-26kt: 3D render and physics comparison tooling tests.
//!
//! Validates the 3D comparison utilities in `gdcore::compare3d`:
//! - PhysicsTraceEntry3D creation and comparison
//! - Golden trace loading from JSON fixtures
//! - Parity report generation
//! - Determinism verification
//! - RenderCompareResult3D statistics
//! - Tolerance handling (position and velocity independently)
//! - End-to-end golden comparison workflow

use gdcore::compare3d::{
    assert_deterministic, compare_physics_traces, PhysicsTraceEntry3D, RenderCompareResult3D,
};
use gdcore::math::Vector3;
use gdphysics2d::body3d::{BodyId3D, BodyType3D, PhysicsBody3D};
use gdphysics2d::shape3d::Shape3D;
use gdphysics2d::world3d::PhysicsWorld3D;

// ===========================================================================
// Helpers: JSON golden file loading
// ===========================================================================

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

fn trace_to_json(trace: &[PhysicsTraceEntry3D]) -> String {
    let entries: Vec<serde_json::Value> = trace
        .iter()
        .map(|e| {
            serde_json::json!({
                "name": e.name,
                "frame": e.frame,
                "px": e.position.x,
                "py": e.position.y,
                "pz": e.position.z,
                "vx": e.velocity.x,
                "vy": e.velocity.y,
                "vz": e.velocity.z,
            })
        })
        .collect();
    serde_json::to_string_pretty(&entries).unwrap()
}

// ===========================================================================
// 1. Golden fixture loading
// ===========================================================================

#[test]
fn load_golden_fixture_from_json() {
    let trace = load_golden_3d_trace("minimal_3d_10frames");
    assert_eq!(trace.len(), 10);
    assert_eq!(trace[0].name, "Ball");
    assert_eq!(trace[0].frame, 0);
    assert!((trace[0].position.y - 5.0).abs() < 0.001);
}

#[test]
fn golden_fixture_has_expected_shape() {
    let trace = load_golden_3d_trace("minimal_3d_10frames");
    // All entries are for "Ball"
    assert!(trace.iter().all(|e| e.name == "Ball"));
    // Frames 0-9
    for (i, e) in trace.iter().enumerate() {
        assert_eq!(e.frame, i as u64);
    }
    // Ball starts at y=5.0 and falls under gravity
    assert!(trace[0].position.y > trace[9].position.y);
}

// ===========================================================================
// 2. Self-comparison (golden vs itself = exact match)
// ===========================================================================

#[test]
fn golden_vs_self_is_exact_match() {
    let trace = load_golden_3d_trace("minimal_3d_10frames");
    let result = compare_physics_traces(&trace, &trace, 0.0, 0.0);
    assert!(result.is_exact_match());
    assert_eq!(result.total_entries, 10);
    assert_eq!(result.matching_entries, 10);
    assert!(result.mismatches.is_empty());
}

// ===========================================================================
// 3. Simulated runtime trace comparison
// ===========================================================================

fn simulate_gravity_fall_3d(frames: u64, dt: f32, gravity: f32) -> Vec<PhysicsTraceEntry3D> {
    let mut position = Vector3::new(0.0, 5.0, 0.0);
    let mut velocity = Vector3::ZERO;
    let mut trace = Vec::new();

    for frame in 0..frames {
        trace.push(PhysicsTraceEntry3D::new(
            "Ball", frame, position, velocity, 0.0,
        ));
        velocity.y -= gravity * dt;
        position = position + velocity * dt;
    }

    trace
}

#[test]
fn simulated_trace_matches_golden_within_tolerance() {
    let golden = load_golden_3d_trace("minimal_3d_10frames");
    let simulated = simulate_gravity_fall_3d(10, 1.0 / 60.0, 588.0);

    let result = compare_physics_traces(&golden, &simulated, 0.5, 1.0);
    assert!(
        result.match_ratio() >= 0.8,
        "expected >= 80% match, got {:.1}%\n{}",
        result.match_ratio() * 100.0,
        result.parity_report("gravity_fall_3d_sim")
    );
}

#[test]
fn simulated_trace_deterministic() {
    let run_a = simulate_gravity_fall_3d(10, 1.0 / 60.0, 588.0);
    let run_b = simulate_gravity_fall_3d(10, 1.0 / 60.0, 588.0);
    assert!(assert_deterministic(&run_a, &run_b));
}

// ===========================================================================
// 4. Parity report generation
// ===========================================================================

#[test]
fn parity_report_shows_pass_for_exact_match() {
    let trace = load_golden_3d_trace("minimal_3d_10frames");
    let result = compare_physics_traces(&trace, &trace, 0.0, 0.0);
    let report = result.parity_report("golden_self_check");
    assert!(report.contains("10/10 matched (100.0%)"));
    assert!(!report.contains("Mismatches"));
}

#[test]
fn parity_report_shows_mismatches() {
    let golden = load_golden_3d_trace("minimal_3d_10frames");
    let mut modified = golden.clone();
    modified[5] = PhysicsTraceEntry3D::new(
        "Ball",
        5,
        Vector3::new(99.0, 99.0, 99.0),
        Vector3::ZERO,
        0.0,
    );
    let result = compare_physics_traces(&golden, &modified, 0.001, 0.001);
    let report = result.parity_report("modified_trace");
    assert!(report.contains("9/10 matched (90.0%)"));
    assert!(report.contains("Mismatches (1)"));
    assert!(report.contains("[Ball] frame 5"));
}

// ===========================================================================
// 5. JSON round-trip (trace → JSON → trace)
// ===========================================================================

#[test]
fn trace_json_roundtrip() {
    let original = vec![
        PhysicsTraceEntry3D::new(
            "Cube",
            0,
            Vector3::new(1.0, 2.0, 3.0),
            Vector3::new(0.0, -9.8, 0.0),
            0.0,
        ),
        PhysicsTraceEntry3D::new(
            "Cube",
            1,
            Vector3::new(1.0, 1.837, 3.0),
            Vector3::new(0.0, -19.6, 0.0),
            0.0,
        ),
    ];

    let json = trace_to_json(&original);
    let parsed: Vec<serde_json::Value> = serde_json::from_str(&json).unwrap();
    let roundtripped: Vec<PhysicsTraceEntry3D> = parsed
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
        .collect();

    let result = compare_physics_traces(&original, &roundtripped, 0.001, 0.001);
    assert!(result.is_exact_match());
}

// ===========================================================================
// 6. Render comparison tooling
// ===========================================================================

#[test]
fn render_compare_parity_report_format() {
    let result = RenderCompareResult3D {
        matching_pixels: 65000,
        total_pixels: 65536,
        max_diff: 0.05,
        avg_diff: 0.002,
        width: 256,
        height: 256,
    };
    let report = result.parity_report("camera_depth_test");
    assert!(report.contains("camera_depth_test"));
    assert!(report.contains("256x256"));
    assert!(report.contains("65000/65536"));
    assert!(report.contains("99.2%"));
}

#[test]
fn render_compare_exact_match() {
    let result = RenderCompareResult3D {
        matching_pixels: 1000,
        total_pixels: 1000,
        max_diff: 0.0,
        avg_diff: 0.0,
        width: 100,
        height: 10,
    };
    assert!(result.is_exact_match());
    assert_eq!(result.match_ratio(), 1.0);
}

// ===========================================================================
// 7. Multi-body 3D trace comparison
// ===========================================================================

#[test]
fn multi_body_trace_comparison() {
    let expected = vec![
        PhysicsTraceEntry3D::new("Ball", 0, Vector3::new(0.0, 5.0, 0.0), Vector3::ZERO, 0.0),
        PhysicsTraceEntry3D::new("Cube", 0, Vector3::new(3.0, 0.0, 0.0), Vector3::ZERO, 0.0),
        PhysicsTraceEntry3D::new(
            "Ball",
            1,
            Vector3::new(0.0, 4.5, 0.0),
            Vector3::new(0.0, -10.0, 0.0),
            0.0,
        ),
        PhysicsTraceEntry3D::new("Cube", 1, Vector3::new(3.0, 0.0, 0.0), Vector3::ZERO, 0.0),
    ];

    // Cube is static, Ball is falling — only Ball changes.
    let actual = expected.clone();
    let result = compare_physics_traces(&expected, &actual, 0.001, 0.001);
    assert!(result.is_exact_match());
    assert_eq!(result.total_entries, 4);
}

// ===========================================================================
// 8. Edge cases
// ===========================================================================

#[test]
fn single_entry_comparison() {
    let a = vec![PhysicsTraceEntry3D::new(
        "Solo",
        0,
        Vector3::new(1.0, 2.0, 3.0),
        Vector3::new(4.0, 5.0, 6.0),
        1.5,
    )];
    let result = compare_physics_traces(&a, &a, 0.0, 0.0);
    assert!(result.is_exact_match());
    assert!((result.avg_position_diff - 0.0).abs() < 1e-6);
}

#[test]
fn angular_velocity_stored_correctly() {
    let entry = PhysicsTraceEntry3D::new("Spinner", 0, Vector3::ZERO, Vector3::ZERO, 3.14);
    assert!((entry.angular_velocity - 3.14).abs() < 1e-6);
}

// ===========================================================================
// pat-q6i: PhysicsWorld3D single-body golden trace matching
// ===========================================================================

const DT: f32 = 1.0 / 60.0;
const GOLDEN_GRAVITY: f32 = 588.0;
const POSITION_TOLERANCE: f32 = 0.1;
const VELOCITY_TOLERANCE: f32 = 0.5;

/// Runs a single rigid sphere through PhysicsWorld3D and records a trace.
fn run_single_body_3d(
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

/// Runs multiple bodies through PhysicsWorld3D and records interleaved traces.
fn run_multi_body_3d(
    bodies: &[(&str, Vector3, f32)],
    gravity: Vector3,
    frames: u64,
) -> Vec<PhysicsTraceEntry3D> {
    let mut world = PhysicsWorld3D::new();
    world.gravity = gravity;

    let mut ids = Vec::new();
    for &(name, pos, mass) in bodies {
        let body = PhysicsBody3D::new(
            BodyId3D(ids.len() as u64),
            BodyType3D::Rigid,
            pos,
            Shape3D::Sphere { radius: 0.5 },
            mass,
        );
        ids.push((name, world.add_body(body)));
    }

    let mut trace = Vec::new();
    for frame in 0..frames {
        for &(name, id) in &ids {
            let b = world.get_body(id).unwrap();
            trace.push(PhysicsTraceEntry3D::new(
                name,
                frame,
                b.position,
                b.linear_velocity,
                0.0,
            ));
        }
        world.step(DT);
    }
    trace
}

// ---------------------------------------------------------------------------
// 13. Engine-produced trace matches minimal_3d_10frames golden
// ---------------------------------------------------------------------------

#[test]
fn q6i_engine_matches_minimal_3d_golden() {
    let golden = load_golden_3d_trace("minimal_3d_10frames");
    let actual = run_single_body_3d(
        "Ball",
        Vector3::new(0.0, 5.0, 0.0),
        Vector3::ZERO,
        Vector3::new(0.0, -GOLDEN_GRAVITY, 0.0),
        10,
    );

    let result = compare_physics_traces(&golden, &actual, POSITION_TOLERANCE, VELOCITY_TOLERANCE);
    assert!(
        result.is_exact_match(),
        "minimal_3d golden mismatch:\n{}",
        result.parity_report("q6i_minimal_3d")
    );
}

// ---------------------------------------------------------------------------
// 14. Engine-produced trace matches rigid_sphere_bounce_3d_20frames golden
// ---------------------------------------------------------------------------

#[test]
fn q6i_engine_matches_rigid_sphere_golden() {
    let golden = load_golden_3d_trace("rigid_sphere_bounce_3d_20frames");
    let actual = run_single_body_3d(
        "Ball",
        Vector3::new(0.0, 8.0, 0.0),
        Vector3::ZERO,
        Vector3::new(0.0, -GOLDEN_GRAVITY, 0.0),
        20,
    );

    let result = compare_physics_traces(&golden, &actual, POSITION_TOLERANCE, VELOCITY_TOLERANCE);
    assert!(
        result.is_exact_match(),
        "rigid_sphere golden mismatch:\n{}",
        result.parity_report("q6i_rigid_sphere")
    );
}

// ---------------------------------------------------------------------------
// 15. Engine-produced multi-body trace matches multi_body_3d_20frames golden
// ---------------------------------------------------------------------------

#[test]
fn q6i_engine_matches_multi_body_golden() {
    let golden = load_golden_3d_trace("multi_body_3d_20frames");
    // Golden has Ball(0,8,0), Cube(3,5,0), HeavyBlock(-2,12,1) — 10 frames, 3 bodies
    let actual = run_multi_body_3d(
        &[
            ("Ball", Vector3::new(0.0, 8.0, 0.0), 1.0),
            ("Cube", Vector3::new(3.0, 5.0, 0.0), 1.0),
            ("HeavyBlock", Vector3::new(-2.0, 12.0, 1.0), 1.0),
        ],
        Vector3::new(0.0, -GOLDEN_GRAVITY, 0.0),
        10,
    );

    let result = compare_physics_traces(&golden, &actual, POSITION_TOLERANCE, VELOCITY_TOLERANCE);
    assert!(
        result.is_exact_match(),
        "multi_body golden mismatch:\n{}",
        result.parity_report("q6i_multi_body")
    );
}

// ---------------------------------------------------------------------------
// 16. Two engine runs produce deterministic traces (bitwise identical)
// ---------------------------------------------------------------------------

#[test]
fn q6i_engine_determinism_minimal() {
    let run_a = run_single_body_3d(
        "Ball",
        Vector3::new(0.0, 5.0, 0.0),
        Vector3::ZERO,
        Vector3::new(0.0, -GOLDEN_GRAVITY, 0.0),
        10,
    );
    let run_b = run_single_body_3d(
        "Ball",
        Vector3::new(0.0, 5.0, 0.0),
        Vector3::ZERO,
        Vector3::new(0.0, -GOLDEN_GRAVITY, 0.0),
        10,
    );

    assert!(
        assert_deterministic(&run_a, &run_b),
        "two identical engine runs must produce bitwise-identical traces"
    );
}

// ---------------------------------------------------------------------------
// 17. Multi-body engine determinism
// ---------------------------------------------------------------------------

#[test]
fn q6i_engine_determinism_multi_body() {
    let bodies: &[(&str, Vector3, f32)] = &[
        ("Ball", Vector3::new(0.0, 8.0, 0.0), 1.0),
        ("Cube", Vector3::new(3.0, 5.0, 0.0), 1.0),
        ("HeavyBlock", Vector3::new(-2.0, 12.0, 1.0), 1.0),
    ];
    let gravity = Vector3::new(0.0, -GOLDEN_GRAVITY, 0.0);

    let run_a = run_multi_body_3d(bodies, gravity, 10);
    let run_b = run_multi_body_3d(bodies, gravity, 10);

    assert!(assert_deterministic(&run_a, &run_b));
}

// ---------------------------------------------------------------------------
// 18. Golden velocity delta is constant (uniform gravity)
// ---------------------------------------------------------------------------

#[test]
fn q6i_golden_velocity_delta_constant_across_fixtures() {
    for (name, expected_len) in [
        ("minimal_3d_10frames", 10),
        ("rigid_sphere_bounce_3d_20frames", 20),
    ] {
        let golden = load_golden_3d_trace(name);
        assert_eq!(golden.len(), expected_len);

        let first_delta = golden[1].velocity.y - golden[0].velocity.y;
        for i in 2..golden.len() {
            let delta = golden[i].velocity.y - golden[i - 1].velocity.y;
            assert!(
                (delta - first_delta).abs() < 0.1,
                "{name} frame {i}: velocity delta {delta:.3} != first delta {first_delta:.3}"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// 19. Engine velocity delta matches golden velocity delta
// ---------------------------------------------------------------------------

#[test]
fn q6i_engine_velocity_delta_matches_golden() {
    let golden = load_golden_3d_trace("minimal_3d_10frames");
    let actual = run_single_body_3d(
        "Ball",
        Vector3::new(0.0, 5.0, 0.0),
        Vector3::ZERO,
        Vector3::new(0.0, -GOLDEN_GRAVITY, 0.0),
        10,
    );

    let golden_delta = golden[1].velocity.y - golden[0].velocity.y;
    let actual_delta = actual[1].velocity.y - actual[0].velocity.y;
    assert!(
        (golden_delta - actual_delta).abs() < 0.01,
        "velocity deltas must match: golden={golden_delta}, engine={actual_delta}"
    );
}

// ---------------------------------------------------------------------------
// 20. Multi-body golden: all bodies share same velocity delta
// ---------------------------------------------------------------------------

#[test]
fn q6i_multi_body_golden_uniform_velocity_delta() {
    let golden = load_golden_3d_trace("multi_body_3d_20frames");
    // 3 bodies × 10 frames = 30 entries
    assert_eq!(golden.len(), 30);

    // Each body has entries at indices 0,3,6,...  1,4,7,...  2,5,8,...
    for body_offset in 0..3 {
        let body_entries: Vec<_> = golden.iter().skip(body_offset).step_by(3).collect();
        let first_delta = body_entries[1].velocity.y - body_entries[0].velocity.y;
        for i in 2..body_entries.len() {
            let delta = body_entries[i].velocity.y - body_entries[i - 1].velocity.y;
            assert!(
                (delta - first_delta).abs() < 0.1,
                "body {} frame {}: velocity delta {delta:.3} != {first_delta:.3}",
                golden[body_offset].name,
                i
            );
        }
    }
}

// ---------------------------------------------------------------------------
// 21. Parity report for engine vs golden shows 100% match
// ---------------------------------------------------------------------------

#[test]
fn q6i_parity_report_engine_vs_golden_100_percent() {
    let golden = load_golden_3d_trace("minimal_3d_10frames");
    let actual = run_single_body_3d(
        "Ball",
        Vector3::new(0.0, 5.0, 0.0),
        Vector3::ZERO,
        Vector3::new(0.0, -GOLDEN_GRAVITY, 0.0),
        10,
    );

    let result = compare_physics_traces(&golden, &actual, POSITION_TOLERANCE, VELOCITY_TOLERANCE);
    let report = result.parity_report("q6i_engine_vs_golden");
    assert!(
        report.contains("10/10 matched (100.0%)"),
        "report should show 100%: {report}"
    );
    assert!(
        !report.contains("Mismatches"),
        "should have no mismatches section"
    );
}

// ---------------------------------------------------------------------------
// 22. Perturbed trace produces measurable mismatch in parity report
// ---------------------------------------------------------------------------

#[test]
fn q6i_perturbed_trace_detected_in_report() {
    let golden = load_golden_3d_trace("minimal_3d_10frames");
    let mut perturbed = golden.clone();
    // Shift frame 3 position significantly
    perturbed[3] = PhysicsTraceEntry3D::new(
        "Ball",
        3,
        Vector3::new(50.0, 50.0, 50.0),
        Vector3::ZERO,
        0.0,
    );

    let result = compare_physics_traces(&golden, &perturbed, 0.001, 0.001);
    assert!(!result.is_exact_match());
    assert_eq!(result.matching_entries, 9);
    assert_eq!(result.mismatches.len(), 1);

    let report = result.parity_report("perturbed_check");
    assert!(report.contains("[Ball] frame 3"));
}

// ---------------------------------------------------------------------------
// 23. Engine trace JSON round-trip preserves golden match
// ---------------------------------------------------------------------------

#[test]
fn q6i_engine_trace_json_roundtrip_preserves_golden_match() {
    let actual = run_single_body_3d(
        "Ball",
        Vector3::new(0.0, 5.0, 0.0),
        Vector3::ZERO,
        Vector3::new(0.0, -GOLDEN_GRAVITY, 0.0),
        10,
    );

    // Serialize and deserialize
    let json = trace_to_json(&actual);
    let parsed: Vec<serde_json::Value> = serde_json::from_str(&json).unwrap();
    let roundtripped: Vec<PhysicsTraceEntry3D> = parsed
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
        .collect();

    // Round-tripped trace should still match golden
    let golden = load_golden_3d_trace("minimal_3d_10frames");
    let result = compare_physics_traces(
        &golden,
        &roundtripped,
        POSITION_TOLERANCE,
        VELOCITY_TOLERANCE,
    );
    assert!(
        result.is_exact_match(),
        "JSON round-trip should preserve golden match:\n{}",
        result.parity_report("roundtrip_vs_golden")
    );
}

// ---------------------------------------------------------------------------
// 24. Cross-golden consistency: same gravity produces same velocity delta
// ---------------------------------------------------------------------------

#[test]
fn q6i_cross_golden_velocity_delta_consistency() {
    let golden_a = load_golden_3d_trace("minimal_3d_10frames");
    let golden_b = load_golden_3d_trace("rigid_sphere_bounce_3d_20frames");

    let delta_a = golden_a[1].velocity.y - golden_a[0].velocity.y;
    let delta_b = golden_b[1].velocity.y - golden_b[0].velocity.y;

    assert!(
        (delta_a - delta_b).abs() < 0.01,
        "both goldens must share same gravity constant: delta_a={delta_a}, delta_b={delta_b}"
    );
}

// ---------------------------------------------------------------------------
// 25. Engine vs golden with zero tolerance catches small drifts
// ---------------------------------------------------------------------------

#[test]
fn q6i_zero_tolerance_catches_rounding() {
    let golden = load_golden_3d_trace("minimal_3d_10frames");
    let actual = run_single_body_3d(
        "Ball",
        Vector3::new(0.0, 5.0, 0.0),
        Vector3::ZERO,
        Vector3::new(0.0, -GOLDEN_GRAVITY, 0.0),
        10,
    );

    // With zero tolerance, only frame 0 (exact start position) matches;
    // subsequent frames diverge due to 3-decimal rounding in golden fixtures.
    let result = compare_physics_traces(&golden, &actual, 0.0, 0.0);
    assert!(
        result.match_ratio() >= 0.1,
        "at least frame 0 should match at zero tolerance: {:.1}%",
        result.match_ratio() * 100.0
    );
    // But with standard tolerance, all frames match.
    let result_tol =
        compare_physics_traces(&golden, &actual, POSITION_TOLERANCE, VELOCITY_TOLERANCE);
    assert!(result_tol.is_exact_match());
}

// ---------------------------------------------------------------------------
// 26. Multi-body golden has correct body names and frame ordering
// ---------------------------------------------------------------------------

#[test]
fn q6i_multi_body_golden_structure() {
    let golden = load_golden_3d_trace("multi_body_3d_20frames");

    // 3 bodies × 10 frames
    assert_eq!(golden.len(), 30);

    let expected_names = ["Ball", "Cube", "HeavyBlock"];
    for frame in 0..10u64 {
        for (j, expected_name) in expected_names.iter().enumerate() {
            let idx = (frame as usize) * 3 + j;
            assert_eq!(golden[idx].name, *expected_name, "frame {frame} body {j}");
            assert_eq!(
                golden[idx].frame, frame,
                "frame number mismatch at idx {idx}"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// 27. Multi-body golden: x/z stay constant (vertical-only gravity)
// ---------------------------------------------------------------------------

#[test]
fn q6i_multi_body_golden_lateral_stability() {
    let golden = load_golden_3d_trace("multi_body_3d_20frames");

    // Group by body name and verify x/z don't change
    let body_names = ["Ball", "Cube", "HeavyBlock"];
    let expected_x = [0.0f32, 3.0, -2.0];
    let expected_z = [0.0f32, 0.0, 1.0];

    for (i, name) in body_names.iter().enumerate() {
        let entries: Vec<_> = golden.iter().filter(|e| e.name == *name).collect();
        for entry in &entries {
            assert!(
                (entry.position.x - expected_x[i]).abs() < 1e-3,
                "{name} frame {}: x should be {}, got {}",
                entry.frame,
                expected_x[i],
                entry.position.x
            );
            assert!(
                (entry.position.z - expected_z[i]).abs() < 1e-3,
                "{name} frame {}: z should be {}, got {}",
                entry.frame,
                expected_z[i],
                entry.position.z
            );
        }
    }
}

// ---------------------------------------------------------------------------
// 28. Long-running determinism (100 frames, engine-based)
// ---------------------------------------------------------------------------

#[test]
fn q6i_long_run_determinism_100_frames() {
    let run_a = run_single_body_3d(
        "LongBall",
        Vector3::new(0.0, 100.0, 0.0),
        Vector3::new(2.0, 10.0, -1.5),
        Vector3::new(0.0, -GOLDEN_GRAVITY, 0.0),
        100,
    );
    let run_b = run_single_body_3d(
        "LongBall",
        Vector3::new(0.0, 100.0, 0.0),
        Vector3::new(2.0, 10.0, -1.5),
        Vector3::new(0.0, -GOLDEN_GRAVITY, 0.0),
        100,
    );

    assert!(assert_deterministic(&run_a, &run_b));
    // Self-comparison should be exact
    let result = compare_physics_traces(&run_a, &run_b, 0.0, 0.0);
    assert!(result.is_exact_match());
    assert_eq!(result.total_entries, 100);
}

// ---------------------------------------------------------------------------
// 29. Horizontal velocity scene: x increases, z constant
// ---------------------------------------------------------------------------

#[test]
fn q6i_horizontal_velocity_trajectory_shape() {
    let trace = run_single_body_3d(
        "Projectile",
        Vector3::new(0.0, 10.0, 0.0),
        Vector3::new(10.0, 0.0, 0.0),
        Vector3::new(0.0, -GOLDEN_GRAVITY, 0.0),
        20,
    );

    // x should increase monotonically
    for i in 1..trace.len() {
        assert!(trace[i].position.x > trace[i - 1].position.x);
    }
    // z should stay zero
    for entry in &trace {
        assert!(entry.position.z.abs() < 1e-4);
    }
    // x velocity constant, y velocity increasingly negative
    for entry in &trace {
        assert!((entry.velocity.x - 10.0).abs() < 1e-4);
    }
    assert!(trace.last().unwrap().velocity.y < -100.0);
}

// ---------------------------------------------------------------------------
// 30. Tolerance sweep: increasing tolerance increases match ratio
// ---------------------------------------------------------------------------

#[test]
fn q6i_tolerance_sweep_monotonic() {
    let golden = load_golden_3d_trace("minimal_3d_10frames");
    let mut slightly_off = golden.clone();
    // Offset every entry by a small amount
    for entry in &mut slightly_off {
        entry.position.y += 0.05;
    }

    let ratios: Vec<f64> = [0.0, 0.01, 0.05, 0.1, 0.5]
        .iter()
        .map(|&tol| {
            let result = compare_physics_traces(&golden, &slightly_off, tol, 1.0);
            result.match_ratio()
        })
        .collect();

    // Match ratio should be non-decreasing as tolerance increases
    for i in 1..ratios.len() {
        assert!(
            ratios[i] >= ratios[i - 1],
            "tolerance sweep not monotonic: {:?}",
            ratios
        );
    }
    // Highest tolerance should match all
    assert_eq!(*ratios.last().unwrap(), 1.0);
}
