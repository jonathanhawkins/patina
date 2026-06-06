//! Integration tests for Performance benchmark CI gate with regression detection.
//!
//! Covers BenchmarkBaseline, BenchmarkGate evaluation, GateVerdict,
//! strict vs lenient mode, summary output, and Dashboard integration.

use gdcore::dashboard::{
    BenchmarkBaseline, BenchmarkEntry, BenchmarkGate, Dashboard, GateVerdict, ParityMetric,
};

// ── BenchmarkBaseline ────────────────────────────────────────────────────────

#[test]
fn baseline_construction() {
    let b = BenchmarkBaseline::new("render_grid", 10.0, 2.0);
    assert_eq!(b.name, "render_grid");
    assert!((b.ms - 10.0).abs() < f64::EPSILON);
    assert!((b.threshold - 2.0).abs() < f64::EPSILON);
}

#[test]
fn baseline_equality() {
    let a = BenchmarkBaseline::new("test", 5.0, 1.5);
    let b = BenchmarkBaseline::new("test", 5.0, 1.5);
    assert_eq!(a, b);
}

// ── BenchmarkGate Construction ───────────────────────────────────────────────

#[test]
fn gate_empty_has_no_baselines() {
    let gate = BenchmarkGate::new(2.0);
    assert_eq!(gate.baseline_count(), 0);
}

#[test]
fn gate_add_baseline() {
    let mut gate = BenchmarkGate::new(2.0);
    gate.add_baseline(BenchmarkBaseline::new("a", 10.0, 2.0));
    gate.add_baseline(BenchmarkBaseline::new("b", 5.0, 1.5));
    assert_eq!(gate.baseline_count(), 2);
}

#[test]
fn gate_load_baselines() {
    let mut gate = BenchmarkGate::new(2.0);
    gate.load_baselines(vec![
        BenchmarkBaseline::new("a", 10.0, 2.0),
        BenchmarkBaseline::new("b", 5.0, 1.5),
        BenchmarkBaseline::new("c", 20.0, 3.0),
    ]);
    assert_eq!(gate.baseline_count(), 3);
}

#[test]
fn gate_get_baseline() {
    let mut gate = BenchmarkGate::new(2.0);
    gate.add_baseline(BenchmarkBaseline::new("render", 10.0, 2.0));
    let b = gate.get_baseline("render").unwrap();
    assert_eq!(b.name, "render");
    assert!(gate.get_baseline("nonexistent").is_none());
}

// ── Single Evaluation ────────────────────────────────────────────────────────

#[test]
fn evaluate_one_pass_within_threshold() {
    let mut gate = BenchmarkGate::new(2.0);
    gate.add_baseline(BenchmarkBaseline::new("render", 10.0, 2.0));

    let result = gate.evaluate_one("render", 15.0); // 1.5x, under 2.0
    assert_eq!(result.verdict, GateVerdict::Pass);
    assert!((result.ratio - 1.5).abs() < 0.001);
    assert!((result.baseline_ms - 10.0).abs() < f64::EPSILON);
}

#[test]
fn evaluate_one_fail_exceeds_threshold() {
    let mut gate = BenchmarkGate::new(2.0);
    gate.add_baseline(BenchmarkBaseline::new("render", 10.0, 2.0));

    let result = gate.evaluate_one("render", 25.0); // 2.5x, above 2.0
    assert_eq!(result.verdict, GateVerdict::Fail);
    assert!((result.ratio - 2.5).abs() < 0.001);
}

#[test]
fn evaluate_one_exact_threshold_passes() {
    let mut gate = BenchmarkGate::new(2.0);
    gate.add_baseline(BenchmarkBaseline::new("render", 10.0, 2.0));

    let result = gate.evaluate_one("render", 20.0); // exactly 2.0x
    assert_eq!(result.verdict, GateVerdict::Pass);
}

#[test]
fn evaluate_one_improvement() {
    let mut gate = BenchmarkGate::new(2.0);
    gate.add_baseline(BenchmarkBaseline::new("render", 10.0, 2.0));

    let result = gate.evaluate_one("render", 5.0); // 0.5x — faster
    assert_eq!(result.verdict, GateVerdict::Pass);
    assert!(result.ratio < 1.0);
}

#[test]
fn evaluate_one_missing_baseline_skip() {
    let gate = BenchmarkGate::new(2.0);
    let result = gate.evaluate_one("unknown", 10.0);
    assert_eq!(result.verdict, GateVerdict::Skip);
    assert!((result.baseline_ms - 0.0).abs() < f64::EPSILON);
}

#[test]
fn evaluate_one_missing_baseline_strict_fail() {
    let gate = BenchmarkGate::new(2.0).strict();
    let result = gate.evaluate_one("unknown", 10.0);
    assert_eq!(result.verdict, GateVerdict::Fail);
}

#[test]
fn evaluate_one_zero_baseline() {
    let mut gate = BenchmarkGate::new(2.0);
    gate.add_baseline(BenchmarkBaseline::new("edge", 0.0, 2.0));

    let result = gate.evaluate_one("edge", 5.0);
    assert_eq!(result.verdict, GateVerdict::Pass);
    assert!((result.ratio - 1.0).abs() < 0.001);
}

// ── Batch Evaluation ─────────────────────────────────────────────────────────

#[test]
fn evaluate_batch_all_pass() {
    let mut gate = BenchmarkGate::new(2.0);
    gate.load_baselines(vec![
        BenchmarkBaseline::new("a", 10.0, 2.0),
        BenchmarkBaseline::new("b", 5.0, 2.0),
    ]);

    let results = gate.evaluate(&[("a", 12.0), ("b", 4.0)]);
    assert_eq!(results.len(), 2);
    assert!(BenchmarkGate::gate_passed(&results));
    assert!(BenchmarkGate::failures(&results).is_empty());
}

#[test]
fn evaluate_batch_one_failure() {
    let mut gate = BenchmarkGate::new(2.0);
    gate.load_baselines(vec![
        BenchmarkBaseline::new("a", 10.0, 2.0),
        BenchmarkBaseline::new("b", 5.0, 2.0),
    ]);

    let results = gate.evaluate(&[("a", 12.0), ("b", 15.0)]); // b is 3.0x
    assert!(!BenchmarkGate::gate_passed(&results));
    let failures = BenchmarkGate::failures(&results);
    assert_eq!(failures.len(), 1);
    assert_eq!(failures[0].name, "b");
}

#[test]
fn evaluate_batch_mixed_with_skip() {
    let mut gate = BenchmarkGate::new(2.0);
    gate.add_baseline(BenchmarkBaseline::new("known", 10.0, 2.0));

    let results = gate.evaluate(&[("known", 12.0), ("new_bench", 5.0)]);
    assert_eq!(results.len(), 2);
    assert_eq!(results[0].verdict, GateVerdict::Pass);
    assert_eq!(results[1].verdict, GateVerdict::Skip);
    assert!(BenchmarkGate::gate_passed(&results)); // skips don't fail
}

#[test]
fn evaluate_batch_strict_missing_fails() {
    let gate = BenchmarkGate::new(2.0).strict();
    let results = gate.evaluate(&[("unknown", 5.0)]);
    assert!(!BenchmarkGate::gate_passed(&results));
}

#[test]
fn evaluate_empty_passes() {
    let gate = BenchmarkGate::new(2.0);
    let results = gate.evaluate(&[]);
    assert!(BenchmarkGate::gate_passed(&results));
}

// ── Per-Benchmark Thresholds ─────────────────────────────────────────────────

#[test]
fn different_thresholds_per_benchmark() {
    let mut gate = BenchmarkGate::new(2.0);
    gate.add_baseline(BenchmarkBaseline::new("tight", 10.0, 1.2)); // strict
    gate.add_baseline(BenchmarkBaseline::new("loose", 10.0, 5.0)); // lenient

    let r_tight = gate.evaluate_one("tight", 15.0); // 1.5x > 1.2 → FAIL
    let r_loose = gate.evaluate_one("loose", 15.0); // 1.5x < 5.0 → PASS

    assert_eq!(r_tight.verdict, GateVerdict::Fail);
    assert_eq!(r_loose.verdict, GateVerdict::Pass);
}

// ── Summary Output ───────────────────────────────────────────────────────────

#[test]
fn summary_shows_pass_count() {
    let mut gate = BenchmarkGate::new(2.0);
    gate.add_baseline(BenchmarkBaseline::new("a", 10.0, 2.0));
    let results = gate.evaluate(&[("a", 12.0)]);
    let summary = BenchmarkGate::summary(&results);
    assert!(summary.contains("1 passed"));
    assert!(summary.contains("0 failed"));
    assert!(summary.contains("GATE: PASSED"));
}

#[test]
fn summary_shows_failure() {
    let mut gate = BenchmarkGate::new(2.0);
    gate.add_baseline(BenchmarkBaseline::new("slow", 10.0, 1.5));
    let results = gate.evaluate(&[("slow", 20.0)]);
    let summary = BenchmarkGate::summary(&results);
    assert!(summary.contains("FAIL"));
    assert!(summary.contains("GATE: FAILED"));
}

#[test]
fn summary_shows_skip() {
    let gate = BenchmarkGate::new(2.0);
    let results = gate.evaluate(&[("new", 5.0)]);
    let summary = BenchmarkGate::summary(&results);
    assert!(summary.contains("SKIP"));
    assert!(summary.contains("no baseline"));
}

#[test]
fn summary_shows_ratio_and_threshold() {
    let mut gate = BenchmarkGate::new(2.0);
    gate.add_baseline(BenchmarkBaseline::new("render", 10.0, 2.0));
    let results = gate.evaluate(&[("render", 15.0)]);
    let summary = BenchmarkGate::summary(&results);
    assert!(summary.contains("1.5x"));
    assert!(summary.contains("2.0x"));
}

// ── Dashboard Integration ────────────────────────────────────────────────────

#[test]
fn dashboard_regression_matches_gate() {
    let mut dashboard = Dashboard::new("CI Run");
    dashboard.add_benchmark(BenchmarkEntry::new("a", 12.0, 10.0, 2.0)); // OK
    dashboard.add_benchmark(BenchmarkEntry::new("b", 25.0, 10.0, 2.0)); // regression

    let mut gate = BenchmarkGate::new(2.0);
    gate.load_baselines(vec![
        BenchmarkBaseline::new("a", 10.0, 2.0),
        BenchmarkBaseline::new("b", 10.0, 2.0),
    ]);
    let results = gate.evaluate(&[("a", 12.0), ("b", 25.0)]);

    // Dashboard and gate should agree on regression count
    assert_eq!(dashboard.regression_count(), 1);
    assert_eq!(BenchmarkGate::failures(&results).len(), 1);
    assert!(!dashboard.is_green());
    assert!(!BenchmarkGate::gate_passed(&results));
}

#[test]
fn dashboard_green_matches_gate_pass() {
    let mut dashboard = Dashboard::new("CI Run");
    dashboard.add_benchmark(BenchmarkEntry::new("a", 8.0, 10.0, 2.0));
    dashboard.add_parity(ParityMetric::new("methods", 100, 100));

    let mut gate = BenchmarkGate::new(2.0);
    gate.add_baseline(BenchmarkBaseline::new("a", 10.0, 2.0));
    let results = gate.evaluate(&[("a", 8.0)]);

    assert!(dashboard.is_green());
    assert!(BenchmarkGate::gate_passed(&results));
}

// ── Edge Cases ───────────────────────────────────────────────────────────────

#[test]
fn gate_very_small_measurements() {
    let mut gate = BenchmarkGate::new(2.0);
    gate.add_baseline(BenchmarkBaseline::new("tiny", 0.001, 3.0));
    let result = gate.evaluate_one("tiny", 0.002);
    assert_eq!(result.verdict, GateVerdict::Pass); // 2.0x < 3.0
}

#[test]
fn gate_large_measurements() {
    let mut gate = BenchmarkGate::new(2.0);
    gate.add_baseline(BenchmarkBaseline::new("big", 5000.0, 1.5));
    let result = gate.evaluate_one("big", 4000.0);
    assert_eq!(result.verdict, GateVerdict::Pass);
    assert!(result.ratio < 1.0);
}

#[test]
fn gate_result_fields_populated() {
    let mut gate = BenchmarkGate::new(2.0);
    gate.add_baseline(BenchmarkBaseline::new("test", 10.0, 2.0));
    let r = gate.evaluate_one("test", 15.0);
    assert_eq!(r.name, "test");
    assert!((r.current_ms - 15.0).abs() < f64::EPSILON);
    assert!((r.baseline_ms - 10.0).abs() < f64::EPSILON);
    assert!((r.threshold - 2.0).abs() < f64::EPSILON);
    assert!((r.ratio - 1.5).abs() < 0.001);
}
