//! Integration tests for the unified 3D render and physics comparison tooling.
//!
//! Exercises the full comparison pipeline: scene tree comparison, physics trace
//! comparison, render framebuffer comparison, and the unified `FixtureParityReport3D`
//! and `AggregateParityReport3D` across multiple simulated fixtures.

use gdcore::compare3d::{
    compare_physics_traces, compare_scene_trees, AggregateParityReport3D, DimensionVerdict,
    FixtureParityReport3D, PhysicsTraceEntry3D, RenderCompareResult3D, SceneTreeEntry,
};
use gdcore::math::Vector3;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn physics_entry(name: &str, frame: u64, x: f32, y: f32, z: f32) -> PhysicsTraceEntry3D {
    PhysicsTraceEntry3D::new(name, frame, Vector3::new(x, y, z), Vector3::ZERO, 0.0)
}

fn make_scene_tree(nodes: &[(&str, &str)]) -> Vec<SceneTreeEntry> {
    nodes
        .iter()
        .map(|(path, class)| SceneTreeEntry::new(path, class))
        .collect()
}

fn make_render_result(matching: u64, total: u64) -> RenderCompareResult3D {
    RenderCompareResult3D {
        matching_pixels: matching,
        total_pixels: total,
        max_diff: if matching == total { 0.0 } else { 0.05 },
        avg_diff: if matching == total { 0.0 } else { 0.01 },
        width: 64,
        height: 64,
    }
}

// ---------------------------------------------------------------------------
// Scene Tree Comparison Integration
// ---------------------------------------------------------------------------

#[test]
fn scene_tree_comparison_full_match() {
    let godot_tree = make_scene_tree(&[
        ("root", "Node3D"),
        ("root/Camera3D", "Camera3D"),
        ("root/MeshInstance3D", "MeshInstance3D"),
        ("root/DirectionalLight3D", "DirectionalLight3D"),
        ("root/StaticBody3D", "StaticBody3D"),
    ]);
    let patina_tree = godot_tree.clone();
    let result = compare_scene_trees(&godot_tree, &patina_tree);
    assert!(result.is_exact_match());
    assert_eq!(result.matching_nodes, 5);
}

#[test]
fn scene_tree_comparison_partial_mismatch() {
    let godot_tree = make_scene_tree(&[
        ("root", "Node3D"),
        ("root/Camera3D", "Camera3D"),
        ("root/Mesh", "MeshInstance3D"),
        ("root/Light", "DirectionalLight3D"),
    ]);
    // Patina is missing the light and has an extra node
    let patina_tree = make_scene_tree(&[
        ("root", "Node3D"),
        ("root/Camera3D", "Camera3D"),
        ("root/Mesh", "MeshInstance3D"),
        ("root/ExtraNode", "Sprite3D"),
    ]);
    let result = compare_scene_trees(&godot_tree, &patina_tree);
    assert!(!result.is_exact_match());
    assert_eq!(result.matching_nodes, 3);
    // 1 missing (Light) + 1 extra (ExtraNode)
    assert_eq!(result.mismatches.len(), 2);
    // Match ratio is 3/4 = 0.75
    assert!((result.match_ratio() - 0.75).abs() < 0.01);
}

// ---------------------------------------------------------------------------
// Physics Trace Comparison Integration
// ---------------------------------------------------------------------------

#[test]
fn physics_freefall_deterministic() {
    // Simulate two identical freefall traces (gravity = 9.8, dt=1/60)
    let trace: Vec<PhysicsTraceEntry3D> = (0..30)
        .map(|f| {
            let t = f as f32 / 60.0;
            let y = -0.5 * 9.8 * t * t;
            physics_entry("Ball", f, 0.0, y, 0.0)
        })
        .collect();

    let result = compare_physics_traces(&trace, &trace, 0.0, 0.0);
    assert!(result.is_exact_match());
    assert_eq!(result.total_entries, 30);
}

#[test]
fn physics_trace_with_drift() {
    // Expected: clean freefall
    let expected: Vec<PhysicsTraceEntry3D> = (0..10)
        .map(|f| {
            let t = f as f32 / 60.0;
            physics_entry("Ball", f, 0.0, -0.5 * 9.8 * t * t, 0.0)
        })
        .collect();

    // Actual: slight X drift (simulating numerical divergence)
    let actual: Vec<PhysicsTraceEntry3D> = (0..10)
        .map(|f| {
            let t = f as f32 / 60.0;
            let drift = f as f32 * 0.001;
            physics_entry("Ball", f, drift, -0.5 * 9.8 * t * t, 0.0)
        })
        .collect();

    // With tight tolerance, some should mismatch
    let strict = compare_physics_traces(&expected, &actual, 0.001, 0.01);
    assert!(strict.matching_entries < 10);

    // With lenient tolerance, all should match
    let lenient = compare_physics_traces(&expected, &actual, 0.1, 0.1);
    assert!(lenient.is_exact_match());
}

// ---------------------------------------------------------------------------
// Unified Fixture Report Integration
// ---------------------------------------------------------------------------

#[test]
fn fixture_report_all_dimensions_pass() {
    let physics = compare_physics_traces(
        &[physics_entry("A", 0, 0.0, 0.0, 0.0)],
        &[physics_entry("A", 0, 0.0, 0.0, 0.0)],
        0.01,
        0.01,
    );
    let render = make_render_result(100, 100);
    let scene_tree = compare_scene_trees(
        &make_scene_tree(&[("root", "Node3D"), ("root/Cam", "Camera3D")]),
        &make_scene_tree(&[("root", "Node3D"), ("root/Cam", "Camera3D")]),
    );

    let report = FixtureParityReport3D::new("minimal_3d")
        .with_physics(physics)
        .with_render(render)
        .with_scene_tree(scene_tree);

    assert_eq!(report.physics_verdict(), DimensionVerdict::Pass);
    assert_eq!(report.render_verdict(), DimensionVerdict::Pass);
    assert_eq!(report.scene_tree_verdict(), DimensionVerdict::Pass);
    assert_eq!(report.overall_verdict(), DimensionVerdict::Pass);

    // Text report should contain all sections
    let text = report.render_text();
    assert!(text.contains("Physics Trace Parity"));
    assert!(text.contains("Render Parity"));
    assert!(text.contains("Scene Tree Parity"));
    assert!(text.contains("Overall: PASS"));

    // JSON report should be well-formed
    let json = report.render_json();
    assert!(json.contains("\"fixture\": \"minimal_3d\""));
    assert!(json.contains("\"overall_verdict\": \"PASS\""));
}

#[test]
fn fixture_report_mixed_dimensions() {
    // Physics: pass
    let physics = compare_physics_traces(
        &[physics_entry("A", 0, 0.0, 0.0, 0.0)],
        &[physics_entry("A", 0, 0.0, 0.0, 0.0)],
        0.01,
        0.01,
    );
    // Render: partial (80% match)
    let render = make_render_result(80, 100);
    // Scene tree: exact match
    let scene_tree = compare_scene_trees(
        &make_scene_tree(&[("root", "Node3D")]),
        &make_scene_tree(&[("root", "Node3D")]),
    );

    let report = FixtureParityReport3D::new("mixed")
        .with_physics(physics)
        .with_render(render)
        .with_scene_tree(scene_tree);

    assert_eq!(report.physics_verdict(), DimensionVerdict::Pass);
    assert_eq!(report.render_verdict(), DimensionVerdict::Partial);
    assert_eq!(report.scene_tree_verdict(), DimensionVerdict::Pass);
    // Overall should be Partial because render is Partial
    assert_eq!(report.overall_verdict(), DimensionVerdict::Partial);
}

// ---------------------------------------------------------------------------
// Aggregate Report Integration
// ---------------------------------------------------------------------------

#[test]
fn aggregate_report_across_fixtures() {
    let mut agg = AggregateParityReport3D::new();

    // Fixture 1: all pass
    let physics1 = compare_physics_traces(
        &[physics_entry("Ball", 0, 0.0, 0.0, 0.0)],
        &[physics_entry("Ball", 0, 0.0, 0.0, 0.0)],
        0.01,
        0.01,
    );
    agg.add(
        FixtureParityReport3D::new("minimal_3d")
            .with_physics(physics1)
            .with_render(make_render_result(100, 100))
            .with_scene_tree(compare_scene_trees(
                &make_scene_tree(&[("root", "Node3D")]),
                &make_scene_tree(&[("root", "Node3D")]),
            )),
    );

    // Fixture 2: physics fail
    let physics2 = compare_physics_traces(
        &[physics_entry("Ball", 0, 0.0, 0.0, 0.0)],
        &[physics_entry("Ball", 0, 99.0, 99.0, 99.0)],
        0.01,
        0.01,
    );
    agg.add(FixtureParityReport3D::new("physics_fail").with_physics(physics2));

    // Fixture 3: render-only pass
    agg.add(FixtureParityReport3D::new("render_only").with_render(make_render_result(96, 100)));

    assert_eq!(agg.fixture_count(), 3);
    assert!(!agg.all_pass());

    let (pass, _partial, fail, _skipped) = agg.verdict_counts();
    assert_eq!(pass, 2); // minimal_3d + render_only
    assert_eq!(fail, 1); // physics_fail

    // Text report should list all fixtures
    let text = agg.render_text();
    assert!(text.contains("minimal_3d"));
    assert!(text.contains("physics_fail"));
    assert!(text.contains("render_only"));
    assert!(text.contains("Total fixtures: 3"));

    // JSON report structure
    let json = agg.render_json();
    assert!(json.contains("\"fixture_count\": 3"));
    assert!(json.contains("\"all_pass\": false"));
}

#[test]
fn aggregate_report_all_pass_gate() {
    let mut agg = AggregateParityReport3D::new();

    for name in &["scene_a", "scene_b", "scene_c"] {
        let physics = compare_physics_traces(
            &[physics_entry("Ball", 0, 0.0, 0.0, 0.0)],
            &[physics_entry("Ball", 0, 0.0, 0.0, 0.0)],
            0.01,
            0.01,
        );
        agg.add(
            FixtureParityReport3D::new(name)
                .with_physics(physics)
                .with_render(make_render_result(100, 100)),
        );
    }

    assert!(agg.all_pass(), "CI gate: all fixtures must pass");
    let json = agg.render_json();
    assert!(json.contains("\"all_pass\": true"));
}

// ---------------------------------------------------------------------------
// Report output validation
// ---------------------------------------------------------------------------

#[test]
fn json_round_trip_fixture_report() {
    let report = FixtureParityReport3D::new("test")
        .with_physics(compare_physics_traces(
            &[physics_entry("A", 0, 1.0, 2.0, 3.0)],
            &[physics_entry("A", 0, 1.0, 2.0, 3.0)],
            0.01,
            0.01,
        ))
        .with_render(make_render_result(95, 100))
        .with_scene_tree(compare_scene_trees(
            &make_scene_tree(&[("root", "Node3D"), ("root/Cam", "Camera3D")]),
            &make_scene_tree(&[("root", "Node3D"), ("root/Cam", "Camera3D")]),
        ));

    let json = report.render_json();

    // Verify key fields are present and well-formed
    assert!(json.contains("\"fixture\": \"test\""));
    assert!(json.contains("\"match_ratio\":"));
    assert!(json.contains("\"total_entries\": 1"));
    assert!(json.contains("\"matching_pixels\": 95"));
    assert!(json.contains("\"matching_nodes\": 2"));
    assert!(json.contains("\"verdict\":"));

    // Verify it starts and ends with braces (valid JSON structure)
    let trimmed = json.trim();
    assert!(trimmed.starts_with('{'));
    assert!(trimmed.ends_with('}'));
}

#[test]
fn text_report_human_readable() {
    let report = FixtureParityReport3D::new("hierarchy_3d")
        .with_physics(compare_physics_traces(
            &[physics_entry("A", 0, 0.0, 0.0, 0.0)],
            &[physics_entry("A", 0, 0.01, 0.0, 0.0)],
            0.1,
            0.1,
        ))
        .with_scene_tree(compare_scene_trees(
            &make_scene_tree(&[
                ("root", "Node3D"),
                ("root/Player", "CharacterBody3D"),
                ("root/Player/Mesh", "MeshInstance3D"),
            ]),
            &make_scene_tree(&[
                ("root", "Node3D"),
                ("root/Player", "CharacterBody3D"),
                ("root/Player/Mesh", "MeshInstance3D"),
            ]),
        ));

    let text = report.render_text();
    assert!(text.contains("hierarchy_3d"));
    assert!(text.contains("100.0%"));
    assert!(text.contains("3/3 matched"));
}
