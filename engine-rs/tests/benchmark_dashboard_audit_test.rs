//! pat-5jwj9: Keep benchmark dashboards aligned with committed baselines and gates.
//!
//! Source of truth: `prd/PHASE9_HARDENING_AUDIT.md`
//!
//! This test validates:
//! 1. The Phase 9 audit doc exists and cites benchmark dashboard artifacts
//! 2. The committed baselines doc exists and has the expected structure
//! 3. Dashboard schema can represent committed baseline entries (schema sync)
//! 4. The CI gate can load and evaluate baselines from the committed format
//! 5. The benchmark evidence files cited in the audit actually exist

use std::path::PathBuf;

use gdcore::dashboard::{
    BenchmarkBaseline, BenchmarkEntry, BenchmarkGate, Dashboard, ParityMetric, RuntimeDashboard,
};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

fn read_audit() -> String {
    let path = repo_root().join("prd/PHASE9_HARDENING_AUDIT.md");
    std::fs::read_to_string(&path).expect("prd/PHASE9_HARDENING_AUDIT.md must exist")
}

fn read_baselines_doc() -> String {
    let path = repo_root().join("docs/BENCHMARK_BASELINES.md");
    std::fs::read_to_string(&path).expect("docs/BENCHMARK_BASELINES.md must exist")
}

// ── Phase 9 audit cites benchmark artifacts ─────────────────────────

#[test]
fn audit_doc_references_benchmark_bead() {
    let audit = read_audit();
    assert!(
        audit.contains("pat-5jwj9"),
        "Phase 9 audit must reference the benchmark dashboard bead"
    );
}

#[test]
fn audit_doc_cites_benchmark_evidence() {
    let audit = read_audit();
    let expected_evidence = [
        "benchmark_dashboard_test.rs",
        "perf_benchmark_ci_gate_test.rs",
        "docs/BENCHMARK_BASELINES.md",
    ];
    for evidence in &expected_evidence {
        assert!(
            audit.contains(evidence),
            "Phase 9 audit must cite evidence '{evidence}'"
        );
    }
}

#[test]
fn audit_classifies_benchmarks_as_measured() {
    let audit = read_audit();
    assert!(
        audit.contains("Measured for local dashboard/tooling slice"),
        "audit must classify benchmark dashboards as measured"
    );
}

// ── Committed baselines doc structure ───────────────────────────────

#[test]
fn baselines_doc_exists_and_has_godot_pin() {
    let doc = read_baselines_doc();
    assert!(
        doc.contains("Godot 4.6.1-stable"),
        "baselines must reference the current Godot pin"
    );
    assert!(
        doc.contains("14d19694e0c88a3f9e82d899a0400f27a24c176e"),
        "baselines must include the pinned commit hash"
    );
}

#[test]
fn baselines_doc_has_expected_benchmarks() {
    let doc = read_baselines_doc();
    let expected_benchmarks = [
        "scene_load",
        "resource_load",
        "physics_step_2d",
        "physics_step_3d",
        "variant_conversion",
        "render_frame_2d",
    ];
    for bench in &expected_benchmarks {
        assert!(
            doc.contains(bench),
            "baselines doc must include benchmark '{bench}'"
        );
    }
}

#[test]
fn baselines_doc_has_regeneration_instructions() {
    let doc = read_baselines_doc();
    assert!(
        doc.contains("Regeneration Instructions"),
        "baselines doc must include regeneration instructions"
    );
}

// ── Dashboard schema can represent committed baselines (schema sync) ─

/// Parse the baseline table from the doc and verify each row can be
/// represented as a BenchmarkEntry in the dashboard schema.
#[test]
fn dashboard_schema_represents_committed_baselines() {
    let doc = read_baselines_doc();

    // Extract baseline rows from the table under "Baseline: Godot 4.6.1-stable"
    // Format: | `name` | total_us | avg_us | notes |
    let baselines = parse_baseline_rows(&doc);
    assert!(
        !baselines.is_empty(),
        "must parse at least one baseline from the doc"
    );

    // Each parsed baseline must be representable as a BenchmarkEntry
    let mut dash = Dashboard::new("Schema Sync Validation");
    for (name, avg_us) in &baselines {
        let avg_ms = *avg_us / 1000.0;
        let entry = BenchmarkEntry::new(name, avg_ms, avg_ms, 2.0);
        assert!(
            !entry.is_regression(),
            "baseline '{name}' should not be a regression against itself"
        );
        dash.add_benchmark(entry);
    }
    assert!(
        dash.is_green(),
        "all baselines loaded into dashboard should be green"
    );
}

/// Parse baselines and load them into a BenchmarkGate to verify the gate
/// schema is compatible with the committed baseline format.
#[test]
fn ci_gate_schema_matches_committed_baselines() {
    let doc = read_baselines_doc();
    let baselines = parse_baseline_rows(&doc);
    assert!(!baselines.is_empty());

    let mut gate = BenchmarkGate::new(2.0);
    for (name, avg_us) in &baselines {
        let avg_ms = *avg_us / 1000.0;
        gate.add_baseline(BenchmarkBaseline::new(name, avg_ms, 2.0));
    }

    assert_eq!(
        gate.baseline_count(),
        baselines.len(),
        "gate baseline count must match parsed baselines"
    );

    // Evaluate with the same values — should all pass
    let measurements: Vec<(&str, f64)> = baselines
        .iter()
        .map(|(name, avg_us)| (name.as_str(), *avg_us / 1000.0))
        .collect();
    let results = gate.evaluate(&measurements);
    assert!(
        BenchmarkGate::gate_passed(&results),
        "evaluating baselines against themselves must pass"
    );
}

/// The RuntimeDashboard type must be able to aggregate parity and
/// benchmark data together — this is the schema the dashboard artifact uses.
#[test]
fn runtime_dashboard_aggregates_baselines_and_parity() {
    let doc = read_baselines_doc();
    let baselines = parse_baseline_rows(&doc);

    let mut dash = RuntimeDashboard::new("Baseline Sync Dashboard");

    // Add baselines as benchmarks
    for (name, avg_us) in &baselines {
        let avg_ms = *avg_us / 1000.0;
        dash.add_benchmark(BenchmarkEntry::new(name, avg_ms, avg_ms, 2.0));
    }

    // Add a representative parity metric
    dash.add_parity(ParityMetric::new("Benchmark coverage", baselines.len(), 6));

    assert!(dash.is_healthy());
    assert_eq!(dash.regression_count(), 0);

    let json = dash.to_json();
    assert!(json.contains("\"healthy\":true"));
}

// ── Evidence file existence ─────────────────────────────────────────

#[test]
fn benchmark_evidence_files_exist() {
    let tests_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests");
    let expected_files = [
        "benchmark_dashboard_test.rs",
        "perf_benchmark_ci_gate_test.rs",
        "bench_runtime_baselines.rs",
        "bench_render_baselines.rs",
    ];
    for file in &expected_files {
        let path = tests_dir.join(file);
        assert!(
            path.exists(),
            "evidence file must exist: {}",
            path.display()
        );
    }
}

#[test]
fn dashboard_module_exists() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("crates/gdcore/src/dashboard.rs");
    assert!(path.exists(), "gdcore::dashboard module must exist");
}

#[test]
fn bench_regression_module_exists() {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("crates/gdcore/src/bench_regression.rs");
    assert!(path.exists(), "gdcore::bench_regression module must exist");
}

// ── Helpers ─────────────────────────────────────────────────────────

/// Parse baseline rows from the BENCHMARK_BASELINES.md table.
/// Returns (name, avg_us) pairs.
fn parse_baseline_rows(doc: &str) -> Vec<(String, f64)> {
    let mut results = Vec::new();

    for line in doc.lines() {
        let line = line.trim();
        // Match rows like: | `scene_load` | 1,976 | 19 | ... |
        if !line.starts_with("| `") {
            continue;
        }
        let cols: Vec<&str> = line.split('|').map(str::trim).collect();
        if cols.len() < 4 {
            continue;
        }
        // cols[0] is empty (before first |), cols[1] is name, cols[2] is total, cols[3] is avg
        let name = cols[1].trim_matches('`').to_string();
        if name.is_empty() || name.contains("Benchmark") {
            continue; // Skip header
        }
        let avg_str: String = cols[3]
            .chars()
            .filter(|c| c.is_ascii_digit() || *c == '.')
            .collect();
        if let Ok(avg) = avg_str.parse::<f64>() {
            results.push((name, avg));
        }
    }
    results
}
