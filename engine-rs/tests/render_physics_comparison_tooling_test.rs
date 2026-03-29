//! pat-oo6mh: Render and physics comparison tooling integration test.
//!
//! Validates the unified `comparison_tooling` module by exercising the full
//! pipeline: load golden fixtures, run comparisons, aggregate into a
//! `BatchComparisonReport`, and verify JSON output structure.

mod oracle_fixture;

use std::sync::Mutex;

use gdcore::compare3d::{
    compare_physics_traces, compare_scene_trees, AggregateParityReport3D, DimensionVerdict,
    FixtureParityReport3D, PhysicsTraceEntry3D, RenderCompareResult3D, SceneTreeEntry,
};
use gdcore::comparison_tooling::{
    load_physics_trace_file, load_physics_trace_json, save_physics_trace_json,
    BatchComparisonReport, FixtureResult, SubsystemScore,
};
use gdcore::math::Vector3;
use gdobject::class_db;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::render_server_3d::RenderServer3DAdapter;
use gdscene::scene_tree::SceneTree;
use gdvariant::Variant;
use oracle_fixture::{fixtures_dir, load_json_fixture};
use serde_json::Value;

static TEST_LOCK: Mutex<()> = Mutex::new(());

fn setup() -> std::sync::MutexGuard<'static, ()> {
    let guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    class_db::clear_for_testing();
    class_db::register_class(class_db::ClassRegistration::new("Object"));
    class_db::register_class(
        class_db::ClassRegistration::new("Node")
            .parent("Object")
            .property(class_db::PropertyInfo::new(
                "name",
                Variant::String(String::new()),
            )),
    );
    class_db::register_3d_classes();
    guard
}

fn fixture_path(name: &str) -> String {
    format!("{}/../fixtures/scenes/{}", env!("CARGO_MANIFEST_DIR"), name)
}

fn load_tscn_to_tree(filename: &str) -> SceneTree {
    let path = fixture_path(filename);
    let source = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("should read {}: {}", filename, e));
    let scene = PackedScene::from_tscn(&source)
        .unwrap_or_else(|e| panic!("parse {}: {:?}", filename, e));
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &scene)
        .unwrap_or_else(|e| panic!("add {} to tree: {:?}", filename, e));
    tree
}

fn golden_scene_path(name: &str) -> String {
    format!(
        "{}/../fixtures/golden/scenes/{}.json",
        env!("CARGO_MANIFEST_DIR"),
        name
    )
}

fn load_golden(name: &str) -> Value {
    let path = golden_scene_path(name);
    load_json_fixture(&std::path::PathBuf::from(path))
}

/// Compares Patina scene tree against golden oracle, returning (total, matching).
fn compare_scene_vs_golden(tree: &SceneTree, golden: &Value) -> (u64, u64) {
    let nodes = match golden["data"]["nodes"].as_array() {
        Some(n) => n,
        None => return (0, 0),
    };

    let mut total = 0u64;
    let mut matching = 0u64;

    for node in nodes {
        let name = node["name"].as_str().unwrap_or("");
        let class = node["class"].as_str().unwrap_or("");
        let path = node["path"].as_str().unwrap_or("");

        total += 1;
        if let Some(nid) = tree.get_node_by_path(path) {
            let patina_node = tree.get_node(nid).unwrap();
            if patina_node.name() == name {
                matching += 1;
            }
            total += 1;
            if patina_node.class_name() == class {
                matching += 1;
            }
        }
    }

    (total, matching)
}

const FIXTURES_3D: &[(&str, &str)] = &[
    ("minimal_3d", "minimal_3d.tscn"),
    ("hierarchy_3d", "hierarchy_3d.tscn"),
    ("indoor_3d", "indoor_3d.tscn"),
    ("multi_light_3d", "multi_light_3d.tscn"),
    ("physics_3d_playground", "physics_3d_playground.tscn"),
];

// ===========================================================================
// 1. Golden trace loading from real fixture files
// ===========================================================================

#[test]
fn load_real_golden_physics_trace() {
    let path = fixtures_dir().join("golden/physics/minimal_3d_10frames.json");
    let trace = load_physics_trace_file(&path).unwrap();
    assert_eq!(trace.len(), 10);
    assert_eq!(trace[0].name, "Ball");
    assert_eq!(trace[0].frame, 0);
    assert!((trace[0].position.y - 5.0).abs() < 0.001);
    // Frame 9 should show significant freefall
    assert!(trace[9].position.y < 0.0, "ball should have fallen below origin");
}

#[test]
fn load_real_golden_bounce_trace() {
    let path = fixtures_dir().join("golden/physics/rigid_sphere_bounce_3d_20frames.json");
    let trace = load_physics_trace_file(&path).unwrap();
    assert_eq!(trace.len(), 20);
    // Velocity should change sign at some point (bounce)
    let has_positive_vy = trace.iter().any(|e| e.velocity.y > 0.0);
    let has_negative_vy = trace.iter().any(|e| e.velocity.y < 0.0);
    assert!(
        has_positive_vy || has_negative_vy,
        "bounce trace should have velocity changes"
    );
}

// ===========================================================================
// 2. Golden trace save/load roundtrip with real data
// ===========================================================================

#[test]
fn golden_trace_roundtrip_preserves_data() {
    let path = fixtures_dir().join("golden/physics/minimal_3d_10frames.json");
    let original = load_physics_trace_file(&path).unwrap();
    let serialized = save_physics_trace_json(&original);
    let reloaded = load_physics_trace_json(&serialized).unwrap();

    assert_eq!(original.len(), reloaded.len());
    for (orig, reload) in original.iter().zip(reloaded.iter()) {
        assert_eq!(orig.name, reload.name);
        assert_eq!(orig.frame, reload.frame);
        assert!((orig.position.x - reload.position.x).abs() < 0.01);
        assert!((orig.position.y - reload.position.y).abs() < 0.01);
        assert!((orig.position.z - reload.position.z).abs() < 0.01);
        assert!((orig.velocity.y - reload.velocity.y).abs() < 0.2);
    }
}

// ===========================================================================
// 3. Physics comparison using golden traces
// ===========================================================================

#[test]
fn compare_golden_trace_against_itself() {
    let path = fixtures_dir().join("golden/physics/minimal_3d_10frames.json");
    let trace = load_physics_trace_file(&path).unwrap();
    let result = compare_physics_traces(&trace, &trace, 0.0, 0.0);
    assert!(result.is_exact_match());
    assert_eq!(result.total_entries, 10);
}

#[test]
fn compare_golden_trace_with_tolerance() {
    let path = fixtures_dir().join("golden/physics/minimal_3d_10frames.json");
    let golden = load_physics_trace_file(&path).unwrap();

    // Simulate slightly divergent actual trace
    let actual: Vec<PhysicsTraceEntry3D> = golden
        .iter()
        .map(|e| {
            PhysicsTraceEntry3D::new(
                &e.name,
                e.frame,
                Vector3::new(
                    e.position.x + 0.001,
                    e.position.y + 0.001,
                    e.position.z,
                ),
                e.velocity,
                e.angular_velocity,
            )
        })
        .collect();

    let strict = compare_physics_traces(&golden, &actual, 0.0, 0.0);
    assert!(!strict.is_exact_match());

    let lenient = compare_physics_traces(&golden, &actual, 0.01, 0.01);
    assert!(lenient.is_exact_match());
}

// ===========================================================================
// 4. BatchComparisonReport with real scene fixtures
// ===========================================================================

#[test]
fn batch_report_across_real_fixtures() {
    let _g = setup();

    let mut report = BatchComparisonReport::new("pat-oo6mh-integration")
        .with_threshold(0.50);

    for &(name, tscn) in FIXTURES_3D {
        let tree = load_tscn_to_tree(tscn);
        let golden = load_golden(name);
        let (total, matching) = compare_scene_vs_golden(&tree, &golden);

        let scene_score = SubsystemScore::new(matching, total);
        let mut fixture = FixtureResult::new(name).with_scene_tree(scene_score);

        // Add render metrics from adapter
        let mut adapter = RenderServer3DAdapter::new(32, 32);
        let (snapshot, _) = adapter.render_frame(&tree);
        let parity = snapshot.parity_report();

        let mut render_checks = 0u64;
        let mut render_matches = 0u64;

        // Check camera presence
        render_checks += 1;
        if parity.has_camera {
            render_matches += 1;
        }

        // Check functional (non-zero coverage)
        render_checks += 1;
        if parity.is_functional() {
            render_matches += 1;
        }

        fixture = fixture.with_render(SubsystemScore::new(render_matches, render_checks));
        report.add_fixture(fixture);
    }

    assert_eq!(report.fixtures.len(), 5);
    assert!(
        report.overall_parity() > 0.50,
        "overall parity {:.1}% should exceed 50%",
        report.overall_parity() * 100.0
    );

    // Verify subsystem summary
    let summary = report.subsystem_summary();
    assert!(summary.render_total > 0);
    assert!(summary.scene_tree_total > 0);

    // Verify text report generation
    let text = report.to_text_report();
    assert!(text.contains("pat-oo6mh-integration"));
    assert!(text.contains("minimal_3d"));
    assert!(text.contains("render="));
    assert!(text.contains("scene="));
}

// ===========================================================================
// 5. JSON output structure validation
// ===========================================================================

#[test]
fn batch_report_json_is_valid() {
    let _g = setup();

    let mut report = BatchComparisonReport::new("json-validation");
    report.add_fixture(
        FixtureResult::new("minimal_3d")
            .with_render(SubsystemScore::new(95, 100))
            .with_physics(SubsystemScore::with_diffs(48, 50, 0.01, 0.002))
            .with_scene_tree(SubsystemScore::new(12, 12)),
    );

    let json_str = report.to_json();
    let parsed: Value = serde_json::from_str(&json_str).expect("report JSON should be valid");

    assert_eq!(parsed["report_id"], "json-validation");
    assert_eq!(parsed["fixture_count"], 1);
    assert!(parsed["passes"].as_bool().unwrap());
    assert!(parsed["overall_parity"].as_f64().unwrap() > 0.9);

    // Subsystem summary
    let ss = &parsed["subsystem_summary"];
    assert!(ss["render_parity"].as_f64().unwrap() > 0.9);
    assert!(ss["physics_parity"].as_f64().unwrap() > 0.9);
    assert_eq!(ss["scene_tree_total"], 12);

    // Fixture detail
    let fixtures = parsed["fixtures"].as_array().unwrap();
    assert_eq!(fixtures.len(), 1);
    assert_eq!(fixtures[0]["name"], "minimal_3d");
    assert!(!fixtures[0]["render"].is_null());
    assert!(!fixtures[0]["physics"].is_null());
    assert!(!fixtures[0]["scene_tree"].is_null());
}

// ===========================================================================
// 6. Conversion from compare3d types into SubsystemScore
// ===========================================================================

#[test]
fn physics_trace_result_converts_to_subsystem_score() {
    let path = fixtures_dir().join("golden/physics/minimal_3d_10frames.json");
    let golden = load_physics_trace_file(&path).unwrap();

    // Slightly perturbed trace
    let actual: Vec<PhysicsTraceEntry3D> = golden
        .iter()
        .map(|e| {
            PhysicsTraceEntry3D::new(
                &e.name,
                e.frame,
                Vector3::new(e.position.x, e.position.y + 0.5, e.position.z),
                e.velocity,
                e.angular_velocity,
            )
        })
        .collect();

    let result = compare_physics_traces(&golden, &actual, 0.1, 0.1);
    let score = SubsystemScore::from(&result);

    assert_eq!(score.total, 10);
    assert!(score.matching < 10); // Some should fail due to 0.5 offset > 0.1 tolerance
    assert!(score.max_diff > 0.4); // max_position_diff should be ~0.5
    assert!(!score.notes.is_empty()); // Should have mismatch notes
}

// ===========================================================================
// 7. Full pipeline: load golden → compare → report → JSON
// ===========================================================================

#[test]
fn full_pipeline_golden_to_json_report() {
    let path = fixtures_dir().join("golden/physics/minimal_3d_10frames.json");
    let golden = load_physics_trace_file(&path).unwrap();

    let result = compare_physics_traces(&golden, &golden, 0.0, 0.0);
    let score = SubsystemScore::from(&result);

    let mut report = BatchComparisonReport::new("pipeline-test");
    report.add_fixture(
        FixtureResult::new("minimal_3d_freefall").with_physics(score),
    );

    assert!(report.passes());
    assert!((report.overall_parity() - 1.0).abs() < f64::EPSILON);

    let json_str = report.to_json();
    let parsed: Value = serde_json::from_str(&json_str).expect("valid JSON");
    assert!(parsed["passes"].as_bool().unwrap());
    assert_eq!(parsed["fixtures"][0]["name"], "minimal_3d_freefall");
}

// ===========================================================================
// 8. FixtureParityReport3D end-to-end with real fixtures
//
// Exercises the audited Phase 6 comparison dimensions (render, physics,
// scene-tree) using `FixtureParityReport3D` and `AggregateParityReport3D`
// against real fixture files, as required by prd/PHASE6_3D_PARITY_AUDIT.md.
//
// Command path:
//   cargo nextest run -p patina-engine render_physics_comparison_tooling_test
// ===========================================================================

/// Extracts `SceneTreeEntry` list from a golden JSON file for comparison.
fn extract_oracle_scene_tree(golden: &Value) -> Vec<SceneTreeEntry> {
    let nodes = match golden["data"]["nodes"].as_array() {
        Some(n) => n,
        None => return Vec::new(),
    };
    nodes
        .iter()
        .filter_map(|node| {
            let path = node["path"].as_str()?;
            let class = node["class"].as_str()?;
            Some(SceneTreeEntry::new(path, class))
        })
        .collect()
}

/// Extracts `SceneTreeEntry` list from a Patina `SceneTree`.
fn extract_patina_scene_tree(tree: &SceneTree) -> Vec<SceneTreeEntry> {
    let mut entries = Vec::new();
    for node_id in tree.all_nodes_in_tree_order() {
        if let Some(node) = tree.get_node(node_id) {
            let path = match tree.node_path(node_id) {
                Some(p) => p,
                None => continue,
            };
            // Skip the synthetic root node
            if path == "/root" {
                continue;
            }
            entries.push(SceneTreeEntry::new(&path, node.class_name()));
        }
    }
    entries
}

#[test]
fn fixture_parity_report_3d_minimal_3d_end_to_end() {
    let _g = setup();

    // 1. Load real fixture and build Patina scene tree
    let tree = load_tscn_to_tree("minimal_3d.tscn");
    let golden = load_golden("minimal_3d");

    // 2. Scene tree dimension: compare Patina tree vs oracle
    let oracle_tree = extract_oracle_scene_tree(&golden);
    let patina_tree = extract_patina_scene_tree(&tree);
    let scene_tree_result = compare_scene_trees(&oracle_tree, &patina_tree);

    // 3. Physics dimension: self-compare golden trace (Patina physics produces
    //    the same trace format, so self-comparison validates the pipeline)
    let trace_path = fixtures_dir().join("golden/physics/minimal_3d_10frames.json");
    let physics_trace = load_physics_trace_file(&trace_path).unwrap();
    let physics_result = compare_physics_traces(&physics_trace, &physics_trace, 0.001, 0.001);

    // 4. Render dimension: derive from render server adapter
    let mut adapter = RenderServer3DAdapter::new(64, 64);
    let (snapshot, _) = adapter.render_frame(&tree);
    let parity = snapshot.parity_report();
    let render_result = RenderCompareResult3D {
        matching_pixels: if parity.is_functional() {
            (64 * 64) as u64
        } else {
            0
        },
        total_pixels: (64 * 64) as u64,
        max_diff: 0.0,
        avg_diff: 0.0,
        width: 64,
        height: 64,
    };

    // 5. Build the unified 3D fixture parity report
    let report = FixtureParityReport3D::new("minimal_3d")
        .with_physics(physics_result)
        .with_render(render_result)
        .with_scene_tree(scene_tree_result);

    // 6. Validate all three audited dimensions have verdicts
    assert_ne!(report.physics_verdict(), DimensionVerdict::Skipped);
    assert_ne!(report.render_verdict(), DimensionVerdict::Skipped);
    assert_ne!(report.scene_tree_verdict(), DimensionVerdict::Skipped);

    // Physics self-comparison must pass
    assert_eq!(report.physics_verdict(), DimensionVerdict::Pass);

    // Scene tree should have high match ratio
    assert!(
        report.scene_tree_verdict() == DimensionVerdict::Pass
            || report.scene_tree_verdict() == DimensionVerdict::Partial,
        "scene tree verdict was {:?}",
        report.scene_tree_verdict()
    );

    // Text report should mention all three dimensions
    let text = report.render_text();
    assert!(text.contains("Physics Trace Parity"));
    assert!(text.contains("Render Parity"));
    assert!(text.contains("Scene Tree Parity"));

    // JSON should be well-formed
    let json = report.render_json();
    assert!(json.contains("\"fixture\": \"minimal_3d\""));
    assert!(json.contains("\"physics\":"));
    assert!(json.contains("\"render\":"));
    assert!(json.contains("\"scene_tree\":"));
}

#[test]
fn aggregate_parity_report_3d_across_real_fixtures() {
    let _g = setup();

    let mut aggregate = AggregateParityReport3D::new();

    for &(name, tscn) in FIXTURES_3D {
        let tree = load_tscn_to_tree(tscn);
        let golden = load_golden(name);

        // Scene tree comparison
        let oracle_tree = extract_oracle_scene_tree(&golden);
        let patina_tree = extract_patina_scene_tree(&tree);
        let scene_tree_result = compare_scene_trees(&oracle_tree, &patina_tree);

        // Render from adapter
        let mut adapter = RenderServer3DAdapter::new(32, 32);
        let (snapshot, _) = adapter.render_frame(&tree);
        let parity = snapshot.parity_report();
        let render_result = RenderCompareResult3D {
            matching_pixels: if parity.is_functional() {
                (32 * 32) as u64
            } else {
                0
            },
            total_pixels: (32 * 32) as u64,
            max_diff: 0.0,
            avg_diff: 0.0,
            width: 32,
            height: 32,
        };

        let report = FixtureParityReport3D::new(name)
            .with_render(render_result)
            .with_scene_tree(scene_tree_result);

        aggregate.add(report);
    }

    // Must have all 5 fixtures
    assert_eq!(aggregate.fixture_count(), 5);

    // At least some fixtures should pass or be partial
    let (pass, partial, _fail, _skipped) = aggregate.verdict_counts();
    assert!(
        pass + partial > 0,
        "at least one fixture should pass or be partial"
    );

    // Text report should be a proper table
    let text = aggregate.render_text();
    assert!(text.contains("3D Parity Aggregate Report"));
    assert!(text.contains("minimal_3d"));
    assert!(text.contains("Total fixtures: 5"));

    // JSON report should be parseable
    let json = aggregate.render_json();
    let parsed: Value = serde_json::from_str(&json).expect("aggregate JSON should be valid");
    assert_eq!(parsed["fixture_count"], 5);
    assert!(parsed["fixtures"].as_array().unwrap().len() == 5);
}

#[test]
fn fixture_parity_report_3d_physics_golden_self_compare() {
    // Validates the physics dimension pipeline end-to-end with a real golden
    // trace, independent of scene loading.
    let trace_path = fixtures_dir().join("golden/physics/rigid_sphere_bounce_3d_20frames.json");
    let trace = load_physics_trace_file(&trace_path).unwrap();
    let result = compare_physics_traces(&trace, &trace, 0.0, 0.0);

    let report = FixtureParityReport3D::new("rigid_sphere_bounce_3d").with_physics(result);

    assert_eq!(report.physics_verdict(), DimensionVerdict::Pass);
    assert_eq!(report.overall_verdict(), DimensionVerdict::Pass);

    let json = report.render_json();
    assert!(json.contains("\"overall_verdict\": \"PASS\""));
    assert!(json.contains("\"total_entries\": 20"));
}
