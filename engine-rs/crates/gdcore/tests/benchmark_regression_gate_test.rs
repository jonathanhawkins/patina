//! Benchmark regression detection tests.
//!
//! Exercises the `BenchmarkGate` CI gate system: loading baselines,
//! evaluating measurements, detecting regressions, and generating
//! threshold alert summaries. Also validates the real golden baselines file.

use gdcore::dashboard::{BenchmarkBaseline, BenchmarkGate, GateVerdict};
use std::path::PathBuf;

fn fixtures_dir() -> PathBuf {
    // gdcore/ -> crates/ -> engine-rs/ -> patina/
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("fixtures")
}

// ---------------------------------------------------------------------------
// BenchmarkGate unit tests
// ---------------------------------------------------------------------------

#[test]
fn gate_passes_when_within_threshold() {
    let mut gate = BenchmarkGate::new(2.0);
    gate.add_baseline(BenchmarkBaseline::new("render", 10.0, 2.0));

    let results = gate.evaluate(&[("render", 15.0)]); // 1.5x, under 2.0
    assert!(BenchmarkGate::gate_passed(&results));
    assert_eq!(results[0].verdict, GateVerdict::Pass);
    assert!((results[0].ratio - 1.5).abs() < 0.001);
}

#[test]
fn gate_fails_when_exceeds_threshold() {
    let mut gate = BenchmarkGate::new(2.0);
    gate.add_baseline(BenchmarkBaseline::new("render", 10.0, 2.0));

    let results = gate.evaluate(&[("render", 25.0)]); // 2.5x, over 2.0
    assert!(!BenchmarkGate::gate_passed(&results));
    assert_eq!(results[0].verdict, GateVerdict::Fail);
}

#[test]
fn gate_skips_unknown_benchmarks_in_lenient_mode() {
    let gate = BenchmarkGate::new(2.0);
    // No baselines loaded — unknown benchmark
    let results = gate.evaluate(&[("unknown", 50.0)]);
    assert!(BenchmarkGate::gate_passed(&results)); // Skip = not a failure
    assert_eq!(results[0].verdict, GateVerdict::Skip);
}

#[test]
fn gate_fails_unknown_benchmarks_in_strict_mode() {
    let gate = BenchmarkGate::new(2.0).strict();
    let results = gate.evaluate(&[("unknown", 50.0)]);
    assert!(!BenchmarkGate::gate_passed(&results));
    assert_eq!(results[0].verdict, GateVerdict::Fail);
}

#[test]
fn gate_handles_multiple_benchmarks() {
    let mut gate = BenchmarkGate::new(2.0);
    gate.add_baseline(BenchmarkBaseline::new("fast", 5.0, 2.0));
    gate.add_baseline(BenchmarkBaseline::new("medium", 10.0, 2.0));
    gate.add_baseline(BenchmarkBaseline::new("slow", 20.0, 2.0));

    // fast: 1.0x (pass), medium: 1.5x (pass), slow: 2.5x (fail)
    let results = gate.evaluate(&[("fast", 5.0), ("medium", 15.0), ("slow", 50.0)]);
    assert!(!BenchmarkGate::gate_passed(&results));

    let failures = BenchmarkGate::failures(&results);
    assert_eq!(failures.len(), 1);
    assert_eq!(failures[0].name, "slow");
}

#[test]
fn gate_passes_with_improvements() {
    let mut gate = BenchmarkGate::new(2.0);
    gate.add_baseline(BenchmarkBaseline::new("render", 10.0, 2.0));

    let results = gate.evaluate(&[("render", 3.0)]); // 0.3x, faster!
    assert!(BenchmarkGate::gate_passed(&results));
    assert_eq!(results[0].verdict, GateVerdict::Pass);
    assert!(results[0].ratio < 1.0);
}

#[test]
fn gate_handles_zero_baseline_gracefully() {
    let mut gate = BenchmarkGate::new(2.0);
    gate.add_baseline(BenchmarkBaseline::new("zero", 0.0, 2.0));

    let results = gate.evaluate(&[("zero", 10.0)]);
    // Zero baseline → ratio defaults to 1.0 (safe)
    assert!(BenchmarkGate::gate_passed(&results));
}

#[test]
fn gate_exact_threshold_boundary() {
    let mut gate = BenchmarkGate::new(2.0);
    gate.add_baseline(BenchmarkBaseline::new("edge", 10.0, 2.0));

    // Exactly 2.0x should not fail (> threshold, not >=)
    let results = gate.evaluate(&[("edge", 20.0)]);
    assert!(BenchmarkGate::gate_passed(&results));

    // Just over 2.0x should fail
    let results = gate.evaluate(&[("edge", 20.001)]);
    assert!(!BenchmarkGate::gate_passed(&results));
}

#[test]
fn gate_per_benchmark_threshold_overrides_default() {
    let mut gate = BenchmarkGate::new(2.0);
    // This benchmark has a tighter threshold of 1.5x
    gate.add_baseline(BenchmarkBaseline::new("tight", 10.0, 1.5));

    // 1.6x exceeds the per-benchmark 1.5x threshold
    let results = gate.evaluate(&[("tight", 16.0)]);
    assert!(!BenchmarkGate::gate_passed(&results));
    assert_eq!(results[0].verdict, GateVerdict::Fail);
}

#[test]
fn gate_load_baselines_replaces_previous() {
    let mut gate = BenchmarkGate::new(2.0);
    gate.add_baseline(BenchmarkBaseline::new("old", 5.0, 2.0));
    assert_eq!(gate.baseline_count(), 1);

    gate.load_baselines(vec![
        BenchmarkBaseline::new("new_a", 10.0, 2.0),
        BenchmarkBaseline::new("new_b", 20.0, 2.0),
    ]);
    assert_eq!(gate.baseline_count(), 2);
    assert!(gate.get_baseline("old").is_none());
    assert!(gate.get_baseline("new_a").is_some());
}

// ---------------------------------------------------------------------------
// Summary output tests
// ---------------------------------------------------------------------------

#[test]
fn gate_summary_shows_pass_fail_skip_counts() {
    let mut gate = BenchmarkGate::new(2.0);
    gate.add_baseline(BenchmarkBaseline::new("pass_bench", 10.0, 2.0));
    gate.add_baseline(BenchmarkBaseline::new("fail_bench", 10.0, 2.0));

    let results = gate.evaluate(&[
        ("pass_bench", 12.0),  // pass
        ("fail_bench", 25.0),  // fail
        ("skip_bench", 5.0),   // skip (no baseline)
    ]);

    let summary = BenchmarkGate::summary(&results);
    assert!(summary.contains("1 passed"), "summary: {}", summary);
    assert!(summary.contains("1 failed"), "summary: {}", summary);
    assert!(summary.contains("1 skipped"), "summary: {}", summary);
    assert!(summary.contains("GATE: FAILED"), "summary: {}", summary);
}

#[test]
fn gate_summary_shows_passed_when_green() {
    let mut gate = BenchmarkGate::new(2.0);
    gate.add_baseline(BenchmarkBaseline::new("ok", 10.0, 2.0));

    let results = gate.evaluate(&[("ok", 10.0)]);
    let summary = BenchmarkGate::summary(&results);
    assert!(summary.contains("GATE: PASSED"), "summary: {}", summary);
    assert!(summary.contains("[PASS]"), "summary: {}", summary);
}

#[test]
fn gate_summary_includes_ratio_and_threshold() {
    let mut gate = BenchmarkGate::new(2.0);
    gate.add_baseline(BenchmarkBaseline::new("render_grid", 11.0, 2.0));

    let results = gate.evaluate(&[("render_grid", 16.5)]);
    let summary = BenchmarkGate::summary(&results);
    assert!(summary.contains("render_grid"), "summary: {}", summary);
    assert!(summary.contains("1.5x"), "summary: {}", summary);
    assert!(summary.contains("2.0x"), "summary: {}", summary);
}

// ---------------------------------------------------------------------------
// Integration: validate real golden baselines file
// ---------------------------------------------------------------------------

#[test]
fn golden_baselines_file_exists_and_is_valid_json() {
    let baselines_path = fixtures_dir()
        .join("golden/render/benchmark_baselines.json");

    assert!(
        baselines_path.exists(),
        "Golden baselines file not found at {:?}",
        baselines_path
    );

    let content = std::fs::read_to_string(&baselines_path).unwrap();
    let json: serde_json::Value = serde_json::from_str(&content)
        .expect("Golden baselines is not valid JSON");

    // Validate structure
    assert!(json["fixtures"].is_object(), "Missing 'fixtures' object");
    assert!(json["regression_threshold"].is_number(), "Missing 'regression_threshold'");
    assert!(json["version"].is_number(), "Missing 'version'");
}

#[test]
fn golden_baselines_load_into_gate() {
    let baselines_path = fixtures_dir()
        .join("golden/render/benchmark_baselines.json");
    let content = std::fs::read_to_string(&baselines_path).unwrap();
    let json: serde_json::Value = serde_json::from_str(&content).unwrap();

    let default_threshold = json["regression_threshold"].as_f64().unwrap();
    let fixtures = json["fixtures"].as_object().unwrap();

    let mut gate = BenchmarkGate::new(default_threshold);
    let mut baselines = Vec::new();
    for (name, entry) in fixtures {
        let ms = entry["per_frame_ms"].as_f64().unwrap();
        baselines.push(BenchmarkBaseline::new(name, ms, default_threshold));
    }
    gate.load_baselines(baselines);

    assert!(
        gate.baseline_count() >= 4,
        "Expected at least 4 baselines, got {}",
        gate.baseline_count()
    );

    // Simulate "current run matches baseline" — should all pass
    let measurements: Vec<(&str, f64)> = fixtures
        .iter()
        .map(|(name, entry)| {
            let ms = entry["per_frame_ms"].as_f64().unwrap();
            (name.as_str(), ms)
        })
        .collect();

    let results = gate.evaluate(&measurements);
    assert!(
        BenchmarkGate::gate_passed(&results),
        "Gate should pass when current == baseline:\n{}",
        BenchmarkGate::summary(&results)
    );
}

#[test]
fn golden_baselines_detect_simulated_regression() {
    let baselines_path = fixtures_dir()
        .join("golden/render/benchmark_baselines.json");
    let content = std::fs::read_to_string(&baselines_path).unwrap();
    let json: serde_json::Value = serde_json::from_str(&content).unwrap();

    let default_threshold = json["regression_threshold"].as_f64().unwrap();
    let fixtures = json["fixtures"].as_object().unwrap();

    let mut gate = BenchmarkGate::new(default_threshold);
    let baselines: Vec<BenchmarkBaseline> = fixtures
        .iter()
        .map(|(name, entry)| {
            let ms = entry["per_frame_ms"].as_f64().unwrap();
            BenchmarkBaseline::new(name, ms, default_threshold)
        })
        .collect();
    gate.load_baselines(baselines);

    // Simulate 3x regression on one fixture
    let mut measurements: Vec<(&str, f64)> = fixtures
        .iter()
        .map(|(name, entry)| {
            let ms = entry["per_frame_ms"].as_f64().unwrap();
            (name.as_str(), ms)
        })
        .collect();

    // Make the first one 3x slower (above the 2.0 threshold)
    if let Some(first) = measurements.first_mut() {
        first.1 *= 3.0;
    }

    let results = gate.evaluate(&measurements);
    assert!(
        !BenchmarkGate::gate_passed(&results),
        "Gate should FAIL when a benchmark regresses 3x:\n{}",
        BenchmarkGate::summary(&results)
    );

    let failures = BenchmarkGate::failures(&results);
    assert_eq!(failures.len(), 1, "Expected exactly 1 failure");

    let summary = BenchmarkGate::summary(&results);
    assert!(summary.contains("GATE: FAILED"), "Summary should show failure");
}

#[test]
fn golden_baselines_all_have_positive_values() {
    let baselines_path = fixtures_dir()
        .join("golden/render/benchmark_baselines.json");
    let content = std::fs::read_to_string(&baselines_path).unwrap();
    let json: serde_json::Value = serde_json::from_str(&content).unwrap();
    let fixtures = json["fixtures"].as_object().unwrap();

    for (name, entry) in fixtures {
        let ms = entry["per_frame_ms"].as_f64().unwrap();
        assert!(ms > 0.0, "Baseline '{}' has non-positive ms: {}", name, ms);

        let mp = entry["mp_per_sec"].as_f64().unwrap();
        assert!(mp > 0.0, "Baseline '{}' has non-positive mp/s: {}", name, mp);
    }
}
