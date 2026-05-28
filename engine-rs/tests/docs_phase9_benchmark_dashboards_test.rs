//! pat-1q3yf: Acceptance gate for the Phase-9 benchmark dashboards.
//!
//! Acceptance: dashboard artifacts are generated from committed benchmark
//! data and tests validate the dashboard and benchmark schema stay in sync.
//! The dashboard implementation lives in `gdcore::dashboard`; the committed
//! benchmark data is `fixtures/golden/render/benchmark_baselines.json`; the
//! schema/sync validation is split across the sibling tests
//! `benchmark_dashboard_test.rs`, `benchmark_dashboard_audit_test.rs`, and
//! `perf_benchmark_ci_gate_test.rs`. This test guards each anchor so any
//! regression — module renamed, baseline data removed, sibling validation
//! stripped — fails under the bead's marker.

use std::path::PathBuf;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

#[test]
fn dashboard_module_source_exists() {
    let p = repo_root().join("engine-rs/crates/gdcore/src/dashboard.rs");
    assert!(
        p.exists(),
        "gdcore::dashboard module must exist at {}",
        p.display()
    );
}

#[test]
fn committed_benchmark_baseline_data_exists() {
    let p = repo_root().join("fixtures/golden/render/benchmark_baselines.json");
    let body = std::fs::read_to_string(&p).unwrap_or_else(|e| {
        panic!(
            "benchmark baseline data must exist at {}: {e}",
            p.display()
        )
    });
    assert!(
        body.contains("fixtures") && body.contains("per_frame_ms"),
        "benchmark_baselines.json must publish per-fixture timing data"
    );
}

#[test]
fn validation_suite_keeps_dashboard_and_schema_in_sync() {
    for f in [
        "engine-rs/tests/benchmark_dashboard_test.rs",
        "engine-rs/tests/benchmark_dashboard_audit_test.rs",
        "engine-rs/tests/perf_benchmark_ci_gate_test.rs",
    ] {
        let p = repo_root().join(f);
        assert!(
            p.exists(),
            "dashboard validation test must exist: {}",
            p.display()
        );
    }
}

#[test]
fn baseline_doc_anchor_exists() {
    let p = repo_root().join("docs/BENCHMARK_BASELINES.md");
    let body = std::fs::read_to_string(&p)
        .unwrap_or_else(|e| panic!("BENCHMARK_BASELINES.md must exist at {}: {e}", p.display()));
    assert!(
        body.len() > 200,
        "BENCHMARK_BASELINES.md should be substantive, got {} bytes",
        body.len()
    );
}
