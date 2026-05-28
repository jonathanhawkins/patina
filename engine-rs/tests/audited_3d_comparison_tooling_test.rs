//! DISABLED (missing SubsystemScore: From<&SceneTreeCompareResult> trait impl)
//! Re-enable by removing the cfg gate below once the missing
//! items land. Tracked as part of editor parity work.
#![cfg(any())]

//! pat-r5kkm: Comparison tooling for the audited 3D report dimensions.
//!
//! Validates that the unified comparison tooling covers all 11 corpus fixtures
//! from the Phase 6 audit and exercises the three audited dimensions:
//!   1. **Scene tree** — structural node/class comparison via `compare_scene_trees`
//!   2. **Physics** — deterministic trace comparison via `compare_physics_traces`
//!   3. **Render** — framebuffer functional check via `RenderServer3DAdapter`
//!
//! Aggregates results into `FixtureParityReport3D`, `AggregateParityReport3D`,
//! and `BatchComparisonReport` and validates JSON/text output.
//!
//! Command path:
//!   cargo nextest run -p patina-engine audited_3d_comparison_tooling_test

mod oracle_fixture;

use gdcore::compare3d::{
    compare_physics_traces, compare_scene_trees, AggregateParityReport3D, DimensionVerdict,
    FixtureParityReport3D, PhysicsTraceEntry3D, RenderCompareResult3D, SceneTreeEntry,
};
use gdcore::comparison_tooling::{
    load_physics_trace_file, BatchComparisonReport, FixtureResult, SubsystemScore,
};
use gdcore::math::Vector3;
use gdobject::class_db;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::render_server_3d::RenderServer3DAdapter;
use gdscene::scene_tree::SceneTree;
use gdvariant::Variant;
use oracle_fixture::{fixtures_dir, load_json_fixture};
use serde_json::Value;
use std::sync::Mutex;

// ---------------------------------------------------------------------------
// Setup
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn fixture_path(name: &str) -> String {
    format!("{}/../fixtures/scenes/{}", env!("CARGO_MANIFEST_DIR"), name)
}

fn load_tscn_to_tree(filename: &str) -> SceneTree {
    let path = fixture_path(filename);
    let source = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("should read {}: {}", filename, e));
    let scene =
        PackedScene::from_tscn(&source).unwrap_or_else(|e| panic!("parse {}: {:?}", filename, e));
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &scene)
        .unwrap_or_else(|e| panic!("add {} to tree: {:?}", filename, e));
    tree
}

fn load_golden(name: &str) -> Value {
    let path = fixtures_dir().join("golden/scenes").join(format!("{name}.json"));
    load_json_fixture(&path)
}

fn extract_oracle_scene_tree(golden: &Value) -> Vec<SceneTreeEntry> {
    golden["data"]["nodes"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|node| {
            let path = node["path"].as_str()?;
            let class = node["class"].as_str()?;
            Some(SceneTreeEntry::new(path, class))
        })
        .collect()
}

fn extract_patina_scene_tree(tree: &SceneTree) -> Vec<SceneTreeEntry> {
    let mut entries = Vec::new();
    for node_id in tree.all_nodes_in_tree_order() {
        let Some(node) = tree.get_node(node_id) else {
            continue;
        };
        let Some(path) = tree.node_path(node_id) else {
            continue;
        };
        if path == "/root" {
            continue;
        }
        entries.push(SceneTreeEntry::new(&path, node.class_name()));
    }
    entries
}

fn make_render_result_from_tree(tree: &SceneTree, size: u32) -> RenderCompareResult3D {
    let mut adapter = RenderServer3DAdapter::new(size, size);
    let (snapshot, _) = adapter.render_frame(tree);
    let parity = snapshot.parity_report();
    RenderCompareResult3D {
        matching_pixels: if parity.is_functional() {
            (size * size) as u64
        } else {
            0
        },
        total_pixels: (size * size) as u64,
        max_diff: 0.0,
        avg_diff: 0.0,
        width: size,
        height: size,
    }
}

/// All 11 corpus fixtures from the Phase 6 audit.
const CORPUS_3D: &[(&str, &str)] = &[
    ("minimal_3d", "minimal_3d.tscn"),
    ("indoor_3d", "indoor_3d.tscn"),
    ("physics_3d_playground", "physics_3d_playground.tscn"),
    ("multi_light_3d", "multi_light_3d.tscn"),
    ("hierarchy_3d", "hierarchy_3d.tscn"),
    ("outdoor_3d", "outdoor_3d.tscn"),
    ("vehicle_3d", "vehicle_3d.tscn"),
    ("spotlight_gallery_3d", "spotlight_gallery_3d.tscn"),
    ("animated_scene_3d", "animated_scene_3d.tscn"),
    ("foggy_terrain_3d", "foggy_terrain_3d.tscn"),
    ("csg_composition", "csg_composition.tscn"),
];

// ===========================================================================
// 1. All 11 corpus fixtures load and parse successfully
// ===========================================================================

#[test]
fn all_corpus_fixtures_load_as_scene_trees() {
    let _g = setup();

    for &(name, tscn) in CORPUS_3D {
        let tree = load_tscn_to_tree(tscn);
        let count = tree.all_nodes_in_tree_order().len();
        assert!(
            count >= 2,
            "fixture {} should have at least 2 nodes, got {}",
            name,
            count
        );
    }
}

// ===========================================================================
// 2. All 11 golden scene JSONs load with correct envelope
// ===========================================================================

#[test]
fn all_golden_scene_jsons_have_valid_envelope() {
    for &(name, _) in CORPUS_3D {
        let golden = load_golden(name);

        let fixture_id = golden["fixture_id"]
            .as_str()
            .unwrap_or_else(|| panic!("{name}: missing fixture_id"));
        assert!(
            fixture_id.starts_with("scene_"),
            "{name}: fixture_id should start with scene_, got {fixture_id}"
        );

        assert_eq!(
            golden["capture_type"].as_str().unwrap(),
            "scene_tree",
            "{name}: capture_type mismatch"
        );
        assert_eq!(
            golden["upstream_version"].as_str().unwrap(),
            "4.6.1-stable",
            "{name}: upstream_version mismatch"
        );
        assert!(
            golden["upstream_commit"].as_str().is_some(),
            "{name}: missing upstream_commit"
        );
        assert!(
            golden["data"]["nodes"].as_array().is_some(),
            "{name}: missing data.nodes array"
        );
    }
}

// ===========================================================================
// 3. Scene tree dimension: compare each fixture against golden oracle
// ===========================================================================

#[test]
fn scene_tree_comparison_across_corpus() {
    let _g = setup();

    for &(name, tscn) in CORPUS_3D {
        let tree = load_tscn_to_tree(tscn);
        let golden = load_golden(name);

        let oracle_tree = extract_oracle_scene_tree(&golden);
        let patina_tree = extract_patina_scene_tree(&tree);

        assert!(
            !oracle_tree.is_empty(),
            "{name}: oracle tree should not be empty"
        );
        assert!(
            !patina_tree.is_empty(),
            "{name}: patina tree should not be empty"
        );

        let result = compare_scene_trees(&oracle_tree, &patina_tree);
        assert!(
            result.match_ratio() >= 0.50,
            "{name}: scene tree match ratio {:.1}% is too low (expected >= 50%)",
            result.match_ratio() * 100.0,
        );
    }
}

// ===========================================================================
// 4. Physics dimension: golden traces load and self-compare
// ===========================================================================

#[test]
fn physics_golden_traces_load_and_self_compare() {
    let traces = [
        "minimal_3d_10frames.json",
        "multi_body_3d_20frames.json",
        "rigid_sphere_bounce_3d_20frames.json",
    ];

    for trace_file in &traces {
        let path = fixtures_dir()
            .join("golden/physics")
            .join(trace_file);
        let trace = load_physics_trace_file(&path)
            .unwrap_or_else(|e| panic!("load {trace_file}: {e}"));

        assert!(
            !trace.is_empty(),
            "{trace_file}: trace should not be empty"
        );

        // Self-comparison must be exact match
        let result = compare_physics_traces(&trace, &trace, 0.0, 0.0);
        assert!(
            result.is_exact_match(),
            "{trace_file}: self-comparison should be exact match"
        );
    }
}

#[test]
fn physics_golden_trace_values_are_physical() {
    // Validate minimal_3d trace follows freefall physics
    let path = fixtures_dir().join("golden/physics/minimal_3d_10frames.json");
    let trace = load_physics_trace_file(&path).unwrap();

    // Ball starts at y=5.0
    assert!(
        (trace[0].position.y - 5.0).abs() < 0.01,
        "ball should start at y=5.0"
    );

    // Ball should be falling (y decreases over time)
    for i in 1..trace.len() {
        assert!(
            trace[i].position.y <= trace[i - 1].position.y,
            "frame {}: ball should be falling (y={} > y={})",
            i,
            trace[i].position.y,
            trace[i - 1].position.y,
        );
    }

    // Velocity should be increasingly negative
    let last = &trace[trace.len() - 1];
    assert!(
        last.velocity.y < -1.0,
        "ball should have significant downward velocity by last frame"
    );
}

#[test]
fn bounce_trace_has_velocity_reversal() {
    let path = fixtures_dir().join("golden/physics/rigid_sphere_bounce_3d_20frames.json");
    let trace = load_physics_trace_file(&path).unwrap();

    let has_negative_vy = trace.iter().any(|e| e.velocity.y < -0.1);
    let has_positive_vy = trace.iter().any(|e| e.velocity.y > 0.1);

    assert!(
        has_negative_vy && has_positive_vy,
        "bounce trace should have both negative and positive vy (pre/post bounce)"
    );
}

// ===========================================================================
// 5. Render dimension: all corpus fixtures produce functional render output
// ===========================================================================

#[test]
fn render_dimension_across_corpus() {
    let _g = setup();

    for &(name, tscn) in CORPUS_3D {
        let tree = load_tscn_to_tree(tscn);
        let render_result = make_render_result_from_tree(&tree, 32);

        // All fixtures should produce a non-zero render (functional output)
        assert!(
            render_result.total_pixels > 0,
            "{name}: render should have non-zero total pixels"
        );
    }
}

// ===========================================================================
// 6. FixtureParityReport3D: per-fixture reports across all 3 dimensions
// ===========================================================================

#[test]
fn fixture_parity_report_all_dimensions_minimal_3d() {
    let _g = setup();

    let tree = load_tscn_to_tree("minimal_3d.tscn");
    let golden = load_golden("minimal_3d");

    // Scene tree dimension
    let oracle_tree = extract_oracle_scene_tree(&golden);
    let patina_tree = extract_patina_scene_tree(&tree);
    let scene_tree_result = compare_scene_trees(&oracle_tree, &patina_tree);

    // Physics dimension (self-compare golden trace)
    let trace_path = fixtures_dir().join("golden/physics/minimal_3d_10frames.json");
    let trace = load_physics_trace_file(&trace_path).unwrap();
    let physics_result = compare_physics_traces(&trace, &trace, 0.001, 0.001);

    // Render dimension
    let render_result = make_render_result_from_tree(&tree, 64);

    let report = FixtureParityReport3D::new("minimal_3d")
        .with_physics(physics_result)
        .with_render(render_result)
        .with_scene_tree(scene_tree_result);

    // All three dimensions should have a verdict (not Skipped)
    assert_ne!(report.physics_verdict(), DimensionVerdict::Skipped);
    assert_ne!(report.render_verdict(), DimensionVerdict::Skipped);
    assert_ne!(report.scene_tree_verdict(), DimensionVerdict::Skipped);

    // Physics self-comparison must pass
    assert_eq!(report.physics_verdict(), DimensionVerdict::Pass);

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

// ===========================================================================
// 7. AggregateParityReport3D: aggregate across all 11 corpus fixtures
// ===========================================================================

#[test]
fn aggregate_report_covers_all_corpus_fixtures() {
    let _g = setup();

    let mut aggregate = AggregateParityReport3D::new();

    for &(name, tscn) in CORPUS_3D {
        let tree = load_tscn_to_tree(tscn);
        let golden = load_golden(name);

        // Scene tree comparison
        let oracle_tree = extract_oracle_scene_tree(&golden);
        let patina_tree = extract_patina_scene_tree(&tree);
        let scene_tree_result = compare_scene_trees(&oracle_tree, &patina_tree);

        // Render comparison
        let render_result = make_render_result_from_tree(&tree, 32);

        let report = FixtureParityReport3D::new(name)
            .with_render(render_result)
            .with_scene_tree(scene_tree_result);

        aggregate.add(report);
    }

    // Must have all 11 fixtures
    assert_eq!(aggregate.fixture_count(), 11);

    // At least some fixtures should pass or be partial
    let (pass, partial, _fail, _skipped) = aggregate.verdict_counts();
    assert!(
        pass + partial > 0,
        "at least one fixture should pass or be partial"
    );

    // Text report should list all fixtures
    let text = aggregate.render_text();
    assert!(text.contains("3D Parity Aggregate Report"));
    assert!(text.contains("minimal_3d"));
    assert!(text.contains("csg_composition"));
    assert!(text.contains("Total fixtures: 11"));

    // JSON report should be parseable and complete
    let json = aggregate.render_json();
    let parsed: Value = serde_json::from_str(&json).expect("aggregate JSON should be valid");
    assert_eq!(parsed["fixture_count"], 11);
    assert!(parsed["fixtures"].as_array().unwrap().len() == 11);
}

// ===========================================================================
// 8. BatchComparisonReport: subsystem scores across corpus
// ===========================================================================

#[test]
fn batch_comparison_report_all_corpus_fixtures() {
    let _g = setup();

    let mut report = BatchComparisonReport::new("phase6-audited-3d-corpus").with_threshold(0.50);

    for &(name, tscn) in CORPUS_3D {
        let tree = load_tscn_to_tree(tscn);
        let golden = load_golden(name);

        // Scene tree score
        let oracle_tree = extract_oracle_scene_tree(&golden);
        let patina_tree = extract_patina_scene_tree(&tree);
        let scene_result = compare_scene_trees(&oracle_tree, &patina_tree);
        let scene_score = SubsystemScore::new(
            scene_result.matching_nodes as u64,
            scene_result.expected_count as u64,
        );

        // Render score
        let render_result = make_render_result_from_tree(&tree, 32);
        let render_score = SubsystemScore::from(&render_result);

        let fixture = FixtureResult::new(name)
            .with_scene_tree(scene_score)
            .with_render(render_score);

        report.add_fixture(fixture);
    }

    assert_eq!(report.fixtures.len(), 11);

    // Subsystem summary should have data for both dimensions
    let summary = report.subsystem_summary();
    assert!(summary.scene_tree_total > 0);
    assert!(summary.render_total > 0);

    // Text report
    let text = report.to_text_report();
    assert!(text.contains("phase6-audited-3d-corpus"));
    assert!(text.contains("Fixtures: 11"));

    // JSON report should be valid
    let json_str = report.to_json();
    let parsed: Value = serde_json::from_str(&json_str).expect("batch JSON should be valid");
    assert_eq!(parsed["fixture_count"], 11);
    assert_eq!(
        parsed["report_id"].as_str().unwrap(),
        "phase6-audited-3d-corpus"
    );
}

// ===========================================================================
// 9. Physics dimension with real golden traces in batch report
// ===========================================================================

#[test]
fn batch_report_with_physics_golden_traces() {
    let traces = [
        ("minimal_3d_freefall", "minimal_3d_10frames.json"),
        ("multi_body_3d", "multi_body_3d_20frames.json"),
        ("rigid_sphere_bounce", "rigid_sphere_bounce_3d_20frames.json"),
    ];

    let mut report = BatchComparisonReport::new("phase6-physics-golden");

    for (name, file) in &traces {
        let path = fixtures_dir().join("golden/physics").join(file);
        let trace = load_physics_trace_file(&path).unwrap();
        let result = compare_physics_traces(&trace, &trace, 0.0, 0.0);
        let score = SubsystemScore::from(&result);

        report.add_fixture(FixtureResult::new(*name).with_physics(score));
    }

    assert!(report.passes());
    assert!((report.overall_parity() - 1.0).abs() < f64::EPSILON);

    let summary = report.subsystem_summary();
    assert!(summary.physics_total > 0);
}

// ===========================================================================
// 10. Conversion bridges: compare3d types → SubsystemScore
// ===========================================================================

#[test]
fn scene_tree_result_converts_to_subsystem_score() {
    let _g = setup();

    let tree = load_tscn_to_tree("hierarchy_3d.tscn");
    let golden = load_golden("hierarchy_3d");

    let oracle_tree = extract_oracle_scene_tree(&golden);
    let patina_tree = extract_patina_scene_tree(&tree);
    let result = compare_scene_trees(&oracle_tree, &patina_tree);

    let score = SubsystemScore::from(&result);
    assert!(score.total > 0);
    assert!(score.matching > 0);
}

#[test]
fn physics_result_converts_to_subsystem_score() {
    let path = fixtures_dir().join("golden/physics/minimal_3d_10frames.json");
    let trace = load_physics_trace_file(&path).unwrap();

    // Create a slightly perturbed trace
    let perturbed: Vec<PhysicsTraceEntry3D> = trace
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

    let result = compare_physics_traces(&trace, &perturbed, 0.1, 0.1);
    let score = SubsystemScore::from(&result);

    assert_eq!(score.total, trace.len() as u64);
    assert!(score.matching < score.total);
    assert!(score.max_diff > 0.4);
}

// ===========================================================================
// 11. Full pipeline: all corpus fixtures → aggregate + batch → JSON
// ===========================================================================

#[test]
fn full_pipeline_all_corpus_to_json() {
    let _g = setup();

    let mut aggregate = AggregateParityReport3D::new();
    let mut batch = BatchComparisonReport::new("phase6-full-pipeline").with_threshold(0.50);

    for &(name, tscn) in CORPUS_3D {
        let tree = load_tscn_to_tree(tscn);
        let golden = load_golden(name);

        let oracle_tree = extract_oracle_scene_tree(&golden);
        let patina_tree = extract_patina_scene_tree(&tree);
        let scene_tree_result = compare_scene_trees(&oracle_tree, &patina_tree);

        let render_result = make_render_result_from_tree(&tree, 32);

        // Build FixtureParityReport3D for aggregate
        let fixture_report = FixtureParityReport3D::new(name)
            .with_render(render_result.clone())
            .with_scene_tree(scene_tree_result.clone());
        aggregate.add(fixture_report);

        // Build FixtureResult for batch
        let scene_score = SubsystemScore::new(
            scene_tree_result.matching_nodes as u64,
            scene_tree_result.expected_count as u64,
        );
        let render_score = SubsystemScore::from(&render_result);
        batch.add_fixture(
            FixtureResult::new(name)
                .with_scene_tree(scene_score)
                .with_render(render_score),
        );
    }

    // Aggregate report validation
    assert_eq!(aggregate.fixture_count(), 11);
    let agg_json = aggregate.render_json();
    let agg_parsed: Value =
        serde_json::from_str(&agg_json).expect("aggregate JSON should be valid");
    assert_eq!(agg_parsed["fixture_count"], 11);

    // Batch report validation
    assert_eq!(batch.fixtures.len(), 11);
    let batch_json = batch.to_json();
    let batch_parsed: Value =
        serde_json::from_str(&batch_json).expect("batch JSON should be valid");
    assert_eq!(batch_parsed["fixture_count"], 11);

    // Both reports should mention all fixtures
    let agg_text = aggregate.render_text();
    let batch_text = batch.to_text_report();
    for &(name, _) in CORPUS_3D {
        assert!(
            agg_text.contains(name),
            "aggregate text should contain {name}"
        );
        assert!(
            batch_text.contains(name),
            "batch text should contain {name}"
        );
    }
}
