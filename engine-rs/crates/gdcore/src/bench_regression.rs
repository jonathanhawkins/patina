//! Benchmark regression detection with threshold alerts.
//!
//! Tracks benchmark results over time and detects regressions when
//! a metric exceeds a configurable threshold relative to a baseline.
//!
//! Designed for CI integration:
//! - JSON-serializable results for pipeline consumption
//! - Configurable thresholds per benchmark
//! - Summary verdicts (pass/warn/fail)
//! - Baseline management (rolling average or explicit)

use std::collections::HashMap;

// ---------------------------------------------------------------------------
// BenchmarkResult
// ---------------------------------------------------------------------------

/// A single benchmark measurement.
#[derive(Debug, Clone, PartialEq)]
pub struct BenchmarkResult {
    /// Benchmark name (e.g., "scene_tree/node_instantiation").
    pub name: String,
    /// Measured value.
    pub value: f64,
    /// Unit (e.g., "ms", "MB", "ops/s").
    pub unit: String,
    /// Whether higher values are better (e.g., throughput).
    pub higher_is_better: bool,
    /// Optional Git commit hash this was measured at.
    pub commit: Option<String>,
    /// Optional timestamp (Unix seconds).
    pub timestamp: Option<u64>,
}

impl BenchmarkResult {
    /// Creates a result where lower is better (latency, memory).
    pub fn lower_is_better(name: impl Into<String>, value: f64, unit: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            value,
            unit: unit.into(),
            higher_is_better: false,
            commit: None,
            timestamp: None,
        }
    }

    /// Creates a result where higher is better (throughput, fps).
    pub fn higher_is_better(name: impl Into<String>, value: f64, unit: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            value,
            unit: unit.into(),
            higher_is_better: true,
            commit: None,
            timestamp: None,
        }
    }

    /// Attaches a commit hash.
    pub fn with_commit(mut self, commit: impl Into<String>) -> Self {
        self.commit = Some(commit.into());
        self
    }

    /// Attaches a timestamp.
    pub fn with_timestamp(mut self, ts: u64) -> Self {
        self.timestamp = Some(ts);
        self
    }
}

// ---------------------------------------------------------------------------
// Threshold
// ---------------------------------------------------------------------------

/// Threshold configuration for regression detection.
#[derive(Debug, Clone)]
pub struct Threshold {
    /// Percentage degradation that triggers a warning.
    pub warn_pct: f64,
    /// Percentage degradation that triggers a failure.
    pub fail_pct: f64,
}

impl Default for Threshold {
    fn default() -> Self {
        Self {
            warn_pct: 5.0,
            fail_pct: 10.0,
        }
    }
}

impl Threshold {
    /// Creates a threshold with custom warn/fail percentages.
    pub fn new(warn_pct: f64, fail_pct: f64) -> Self {
        Self { warn_pct, fail_pct }
    }

    /// A strict threshold (2% warn, 5% fail).
    pub fn strict() -> Self {
        Self::new(2.0, 5.0)
    }

    /// A relaxed threshold (10% warn, 20% fail).
    pub fn relaxed() -> Self {
        Self::new(10.0, 20.0)
    }
}

// ---------------------------------------------------------------------------
// RegressionVerdict
// ---------------------------------------------------------------------------

/// The verdict for a single benchmark comparison.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegressionVerdict {
    /// Within acceptable range — no regression.
    Pass,
    /// Degradation exceeds warn threshold but not fail threshold.
    Warn,
    /// Degradation exceeds fail threshold — regression detected.
    Fail,
    /// Performance improved.
    Improved,
}

impl RegressionVerdict {
    /// Returns a CI-friendly exit code (0=pass, 1=warn, 2=fail).
    pub fn exit_code(self) -> i32 {
        match self {
            Self::Pass | Self::Improved => 0,
            Self::Warn => 1,
            Self::Fail => 2,
        }
    }

    /// Returns a display label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Pass => "PASS",
            Self::Warn => "WARN",
            Self::Fail => "FAIL",
            Self::Improved => "IMPROVED",
        }
    }
}

impl std::fmt::Display for RegressionVerdict {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}

// ---------------------------------------------------------------------------
// ComparisonResult
// ---------------------------------------------------------------------------

/// Result of comparing a benchmark against its baseline.
#[derive(Debug, Clone)]
pub struct ComparisonResult {
    /// Benchmark name.
    pub name: String,
    /// Baseline value.
    pub baseline: f64,
    /// Current value.
    pub current: f64,
    /// Change percentage (positive = degradation for lower-is-better).
    pub change_pct: f64,
    /// Unit.
    pub unit: String,
    /// Verdict.
    pub verdict: RegressionVerdict,
}

// ---------------------------------------------------------------------------
// Baseline
// ---------------------------------------------------------------------------

/// A set of baseline benchmark values to compare against.
#[derive(Debug, Clone, Default)]
pub struct Baseline {
    /// Benchmark name → baseline value.
    values: HashMap<String, f64>,
    /// Per-benchmark thresholds (overrides the default).
    thresholds: HashMap<String, Threshold>,
}

impl Baseline {
    /// Creates an empty baseline.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets a baseline value for a benchmark.
    pub fn set(&mut self, name: impl Into<String>, value: f64) {
        self.values.insert(name.into(), value);
    }

    /// Gets the baseline value for a benchmark.
    pub fn get(&self, name: &str) -> Option<f64> {
        self.values.get(name).copied()
    }

    /// Sets a per-benchmark threshold override.
    pub fn set_threshold(&mut self, name: impl Into<String>, threshold: Threshold) {
        self.thresholds.insert(name.into(), threshold);
    }

    /// Gets the threshold for a benchmark (per-benchmark or default).
    pub fn threshold(&self, name: &str) -> Threshold {
        self.thresholds
            .get(name)
            .cloned()
            .unwrap_or_default()
    }

    /// Returns the number of baseline entries.
    pub fn len(&self) -> usize {
        self.values.len()
    }

    /// Returns whether the baseline is empty.
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    /// Builds a baseline from the rolling average of historical results.
    pub fn from_history(history: &[Vec<BenchmarkResult>], window: usize) -> Self {
        let mut baseline = Self::new();
        let mut accum: HashMap<String, Vec<f64>> = HashMap::new();

        let start = history.len().saturating_sub(window);
        for run in &history[start..] {
            for result in run {
                accum
                    .entry(result.name.clone())
                    .or_default()
                    .push(result.value);
            }
        }

        for (name, values) in accum {
            if !values.is_empty() {
                let avg = values.iter().sum::<f64>() / values.len() as f64;
                baseline.set(name, avg);
            }
        }

        baseline
    }
}

// ---------------------------------------------------------------------------
// RegressionDetector
// ---------------------------------------------------------------------------

/// Detects benchmark regressions by comparing current results against a baseline.
#[derive(Debug)]
pub struct RegressionDetector {
    /// The baseline to compare against.
    baseline: Baseline,
    /// Default threshold for benchmarks without per-benchmark overrides.
    default_threshold: Threshold,
}

impl RegressionDetector {
    /// Creates a detector with the given baseline and default threshold.
    pub fn new(baseline: Baseline, default_threshold: Threshold) -> Self {
        Self {
            baseline,
            default_threshold,
        }
    }

    /// Creates a detector with default thresholds (5% warn, 10% fail).
    pub fn with_baseline(baseline: Baseline) -> Self {
        Self::new(baseline, Threshold::default())
    }

    /// Compares a single benchmark result against its baseline.
    pub fn check(&self, result: &BenchmarkResult) -> Option<ComparisonResult> {
        let baseline_val = self.baseline.get(&result.name)?;
        let threshold = self
            .baseline
            .thresholds
            .get(&result.name)
            .cloned()
            .unwrap_or_else(|| self.default_threshold.clone());

        let change_pct = if result.higher_is_better {
            // Higher is better: degradation is negative change
            if baseline_val > 0.0 {
                ((baseline_val - result.value) / baseline_val) * 100.0
            } else {
                0.0
            }
        } else {
            // Lower is better: degradation is positive change
            if baseline_val > 0.0 {
                ((result.value - baseline_val) / baseline_val) * 100.0
            } else {
                0.0
            }
        };

        let verdict = if change_pct >= threshold.fail_pct {
            RegressionVerdict::Fail
        } else if change_pct >= threshold.warn_pct {
            RegressionVerdict::Warn
        } else if change_pct < -threshold.warn_pct {
            RegressionVerdict::Improved
        } else {
            RegressionVerdict::Pass
        };

        Some(ComparisonResult {
            name: result.name.clone(),
            baseline: baseline_val,
            current: result.value,
            change_pct,
            unit: result.unit.clone(),
            verdict,
        })
    }

    /// Checks all results and returns a full regression report.
    pub fn check_all(&self, results: &[BenchmarkResult]) -> RegressionReport {
        let comparisons: Vec<ComparisonResult> = results
            .iter()
            .filter_map(|r| self.check(r))
            .collect();

        let overall = if comparisons.iter().any(|c| c.verdict == RegressionVerdict::Fail) {
            RegressionVerdict::Fail
        } else if comparisons.iter().any(|c| c.verdict == RegressionVerdict::Warn) {
            RegressionVerdict::Warn
        } else {
            RegressionVerdict::Pass
        };

        let missing: Vec<String> = results
            .iter()
            .filter(|r| self.baseline.get(&r.name).is_none())
            .map(|r| r.name.clone())
            .collect();

        RegressionReport {
            comparisons,
            overall,
            missing_baselines: missing,
        }
    }
}

// ---------------------------------------------------------------------------
// RegressionReport
// ---------------------------------------------------------------------------

/// A complete regression detection report.
#[derive(Debug, Clone)]
pub struct RegressionReport {
    /// Per-benchmark comparison results.
    pub comparisons: Vec<ComparisonResult>,
    /// Overall verdict (worst among all benchmarks).
    pub overall: RegressionVerdict,
    /// Benchmarks that had no baseline (skipped).
    pub missing_baselines: Vec<String>,
}

impl RegressionReport {
    /// Returns the number of passing benchmarks.
    pub fn pass_count(&self) -> usize {
        self.comparisons
            .iter()
            .filter(|c| c.verdict == RegressionVerdict::Pass || c.verdict == RegressionVerdict::Improved)
            .count()
    }

    /// Returns the number of warnings.
    pub fn warn_count(&self) -> usize {
        self.comparisons
            .iter()
            .filter(|c| c.verdict == RegressionVerdict::Warn)
            .count()
    }

    /// Returns the number of failures.
    pub fn fail_count(&self) -> usize {
        self.comparisons
            .iter()
            .filter(|c| c.verdict == RegressionVerdict::Fail)
            .count()
    }

    /// Returns the number of improvements.
    pub fn improved_count(&self) -> usize {
        self.comparisons
            .iter()
            .filter(|c| c.verdict == RegressionVerdict::Improved)
            .count()
    }

    /// Returns a CI-friendly exit code (0=pass, 1=warn, 2=fail).
    pub fn exit_code(&self) -> i32 {
        self.overall.exit_code()
    }

    /// Renders a human-readable text summary.
    pub fn render_text(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!(
            "Regression Report: {} benchmarks | {} pass | {} warn | {} fail | {} improved\n",
            self.comparisons.len(),
            self.pass_count(),
            self.warn_count(),
            self.fail_count(),
            self.improved_count(),
        ));
        out.push_str(&format!("Overall: {}\n\n", self.overall));

        for c in &self.comparisons {
            out.push_str(&format!(
                "  [{}] {} : {:.2}{} -> {:.2}{} ({:+.1}%)\n",
                c.verdict.label(),
                c.name,
                c.baseline,
                c.unit,
                c.current,
                c.unit,
                c.change_pct,
            ));
        }

        if !self.missing_baselines.is_empty() {
            out.push_str(&format!(
                "\nMissing baselines ({}): {}\n",
                self.missing_baselines.len(),
                self.missing_baselines.join(", ")
            ));
        }

        out
    }

    /// Serializes the report to a JSON-like summary string.
    pub fn to_json(&self) -> String {
        let mut out = String::from("{\n");
        out.push_str(&format!("  \"overall\": \"{}\",\n", self.overall.label()));
        out.push_str(&format!("  \"total\": {},\n", self.comparisons.len()));
        out.push_str(&format!("  \"pass\": {},\n", self.pass_count()));
        out.push_str(&format!("  \"warn\": {},\n", self.warn_count()));
        out.push_str(&format!("  \"fail\": {},\n", self.fail_count()));
        out.push_str("  \"comparisons\": [\n");
        for (i, c) in self.comparisons.iter().enumerate() {
            out.push_str(&format!(
                "    {{\"name\": \"{}\", \"baseline\": {:.2}, \"current\": {:.2}, \"change_pct\": {:.1}, \"verdict\": \"{}\"}}",
                c.name, c.baseline, c.current, c.change_pct, c.verdict.label()
            ));
            if i + 1 < self.comparisons.len() {
                out.push(',');
            }
            out.push('\n');
        }
        out.push_str("  ]\n}");
        out
    }

    /// Returns only the failed comparisons.
    pub fn failures(&self) -> Vec<&ComparisonResult> {
        self.comparisons
            .iter()
            .filter(|c| c.verdict == RegressionVerdict::Fail)
            .collect()
    }

    /// Returns only the warnings.
    pub fn warnings(&self) -> Vec<&ComparisonResult> {
        self.comparisons
            .iter()
            .filter(|c| c.verdict == RegressionVerdict::Warn)
            .collect()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_baseline() -> Baseline {
        let mut b = Baseline::new();
        b.set("latency", 10.0);
        b.set("throughput", 1000.0);
        b.set("memory", 50.0);
        b
    }

    // ── BenchmarkResult ─────────────────────────────────────────────

    #[test]
    fn result_lower_is_better() {
        let r = BenchmarkResult::lower_is_better("latency", 12.0, "ms");
        assert_eq!(r.name, "latency");
        assert!(!r.higher_is_better);
    }

    #[test]
    fn result_higher_is_better() {
        let r = BenchmarkResult::higher_is_better("throughput", 1000.0, "ops/s");
        assert!(r.higher_is_better);
    }

    #[test]
    fn result_with_metadata() {
        let r = BenchmarkResult::lower_is_better("test", 1.0, "ms")
            .with_commit("abc123")
            .with_timestamp(1234567890);
        assert_eq!(r.commit, Some("abc123".to_string()));
        assert_eq!(r.timestamp, Some(1234567890));
    }

    // ── Threshold ───────────────────────────────────────────────────

    #[test]
    fn threshold_default() {
        let t = Threshold::default();
        assert!((t.warn_pct - 5.0).abs() < 0.01);
        assert!((t.fail_pct - 10.0).abs() < 0.01);
    }

    #[test]
    fn threshold_strict() {
        let t = Threshold::strict();
        assert!((t.warn_pct - 2.0).abs() < 0.01);
        assert!((t.fail_pct - 5.0).abs() < 0.01);
    }

    #[test]
    fn threshold_relaxed() {
        let t = Threshold::relaxed();
        assert!((t.warn_pct - 10.0).abs() < 0.01);
        assert!((t.fail_pct - 20.0).abs() < 0.01);
    }

    // ── RegressionVerdict ───────────────────────────────────────────

    #[test]
    fn verdict_exit_codes() {
        assert_eq!(RegressionVerdict::Pass.exit_code(), 0);
        assert_eq!(RegressionVerdict::Improved.exit_code(), 0);
        assert_eq!(RegressionVerdict::Warn.exit_code(), 1);
        assert_eq!(RegressionVerdict::Fail.exit_code(), 2);
    }

    #[test]
    fn verdict_labels() {
        assert_eq!(RegressionVerdict::Pass.label(), "PASS");
        assert_eq!(RegressionVerdict::Fail.label(), "FAIL");
        assert_eq!(RegressionVerdict::Warn.label(), "WARN");
        assert_eq!(RegressionVerdict::Improved.label(), "IMPROVED");
    }

    // ── Baseline ────────────────────────────────────────────────────

    #[test]
    fn baseline_set_and_get() {
        let b = make_baseline();
        assert_eq!(b.get("latency"), Some(10.0));
        assert_eq!(b.get("unknown"), None);
        assert_eq!(b.len(), 3);
    }

    #[test]
    fn baseline_from_history() {
        let history = vec![
            vec![
                BenchmarkResult::lower_is_better("latency", 10.0, "ms"),
                BenchmarkResult::lower_is_better("memory", 50.0, "MB"),
            ],
            vec![
                BenchmarkResult::lower_is_better("latency", 12.0, "ms"),
                BenchmarkResult::lower_is_better("memory", 48.0, "MB"),
            ],
        ];

        let b = Baseline::from_history(&history, 2);
        assert!((b.get("latency").unwrap() - 11.0).abs() < 0.01);
        assert!((b.get("memory").unwrap() - 49.0).abs() < 0.01);
    }

    #[test]
    fn baseline_from_history_window() {
        let history = vec![
            vec![BenchmarkResult::lower_is_better("x", 100.0, "ms")],
            vec![BenchmarkResult::lower_is_better("x", 10.0, "ms")],
            vec![BenchmarkResult::lower_is_better("x", 12.0, "ms")],
        ];

        // Window of 2 → only last 2 runs.
        let b = Baseline::from_history(&history, 2);
        assert!((b.get("x").unwrap() - 11.0).abs() < 0.01);
    }

    #[test]
    fn baseline_per_benchmark_threshold() {
        let mut b = Baseline::new();
        b.set("strict_bench", 10.0);
        b.set_threshold("strict_bench", Threshold::strict());

        let t = b.threshold("strict_bench");
        assert!((t.warn_pct - 2.0).abs() < 0.01);

        let t_default = b.threshold("unknown");
        assert!((t_default.warn_pct - 5.0).abs() < 0.01);
    }

    // ── RegressionDetector ──────────────────────────────────────────

    #[test]
    fn detect_pass() {
        let detector = RegressionDetector::with_baseline(make_baseline());
        let result = BenchmarkResult::lower_is_better("latency", 10.3, "ms");

        let cmp = detector.check(&result).unwrap();
        assert_eq!(cmp.verdict, RegressionVerdict::Pass);
        assert!(cmp.change_pct < 5.0);
    }

    #[test]
    fn detect_warn() {
        let detector = RegressionDetector::with_baseline(make_baseline());
        // 10.0 → 10.7 = 7% increase (latency, lower is better)
        let result = BenchmarkResult::lower_is_better("latency", 10.7, "ms");

        let cmp = detector.check(&result).unwrap();
        assert_eq!(cmp.verdict, RegressionVerdict::Warn);
    }

    #[test]
    fn detect_fail() {
        let detector = RegressionDetector::with_baseline(make_baseline());
        // 10.0 → 12.0 = 20% increase
        let result = BenchmarkResult::lower_is_better("latency", 12.0, "ms");

        let cmp = detector.check(&result).unwrap();
        assert_eq!(cmp.verdict, RegressionVerdict::Fail);
    }

    #[test]
    fn detect_improved() {
        let detector = RegressionDetector::with_baseline(make_baseline());
        // 10.0 → 8.0 = -20% (improvement for lower-is-better)
        let result = BenchmarkResult::lower_is_better("latency", 8.0, "ms");

        let cmp = detector.check(&result).unwrap();
        assert_eq!(cmp.verdict, RegressionVerdict::Improved);
    }

    #[test]
    fn detect_higher_is_better_regression() {
        let detector = RegressionDetector::with_baseline(make_baseline());
        // Throughput: baseline=1000, current=850 → 15% drop
        let result = BenchmarkResult::higher_is_better("throughput", 850.0, "ops/s");

        let cmp = detector.check(&result).unwrap();
        assert_eq!(cmp.verdict, RegressionVerdict::Fail);
    }

    #[test]
    fn detect_higher_is_better_improvement() {
        let detector = RegressionDetector::with_baseline(make_baseline());
        // Throughput: baseline=1000, current=1200 → 20% improvement
        let result = BenchmarkResult::higher_is_better("throughput", 1200.0, "ops/s");

        let cmp = detector.check(&result).unwrap();
        assert_eq!(cmp.verdict, RegressionVerdict::Improved);
    }

    #[test]
    fn missing_baseline_returns_none() {
        let detector = RegressionDetector::with_baseline(make_baseline());
        let result = BenchmarkResult::lower_is_better("new_bench", 5.0, "ms");

        assert!(detector.check(&result).is_none());
    }

    // ── RegressionReport ────────────────────────────────────────────

    #[test]
    fn report_all_pass() {
        let detector = RegressionDetector::with_baseline(make_baseline());
        let results = vec![
            BenchmarkResult::lower_is_better("latency", 10.1, "ms"),
            BenchmarkResult::lower_is_better("memory", 50.5, "MB"),
        ];

        let report = detector.check_all(&results);
        assert_eq!(report.overall, RegressionVerdict::Pass);
        assert_eq!(report.pass_count(), 2);
        assert_eq!(report.fail_count(), 0);
        assert_eq!(report.exit_code(), 0);
    }

    #[test]
    fn report_with_failure() {
        let detector = RegressionDetector::with_baseline(make_baseline());
        let results = vec![
            BenchmarkResult::lower_is_better("latency", 10.1, "ms"),   // pass
            BenchmarkResult::lower_is_better("memory", 60.0, "MB"),    // fail (20% increase)
        ];

        let report = detector.check_all(&results);
        assert_eq!(report.overall, RegressionVerdict::Fail);
        assert_eq!(report.fail_count(), 1);
        assert_eq!(report.exit_code(), 2);
    }

    #[test]
    fn report_with_warning() {
        let detector = RegressionDetector::with_baseline(make_baseline());
        let results = vec![
            BenchmarkResult::lower_is_better("latency", 10.7, "ms"), // 7% → warn
        ];

        let report = detector.check_all(&results);
        assert_eq!(report.overall, RegressionVerdict::Warn);
        assert_eq!(report.warn_count(), 1);
        assert_eq!(report.exit_code(), 1);
    }

    #[test]
    fn report_missing_baselines() {
        let detector = RegressionDetector::with_baseline(make_baseline());
        let results = vec![
            BenchmarkResult::lower_is_better("latency", 10.1, "ms"),
            BenchmarkResult::lower_is_better("new_bench", 5.0, "ms"),
        ];

        let report = detector.check_all(&results);
        assert_eq!(report.missing_baselines, vec!["new_bench"]);
        assert_eq!(report.comparisons.len(), 1); // Only latency compared
    }

    #[test]
    fn report_render_text() {
        let detector = RegressionDetector::with_baseline(make_baseline());
        let results = vec![
            BenchmarkResult::lower_is_better("latency", 12.0, "ms"),
            BenchmarkResult::lower_is_better("memory", 50.1, "MB"),
        ];

        let report = detector.check_all(&results);
        let text = report.render_text();
        assert!(text.contains("Regression Report"));
        assert!(text.contains("FAIL"));
        assert!(text.contains("latency"));
    }

    #[test]
    fn report_to_json() {
        let detector = RegressionDetector::with_baseline(make_baseline());
        let results = vec![
            BenchmarkResult::lower_is_better("latency", 10.1, "ms"),
        ];

        let report = detector.check_all(&results);
        let json = report.to_json();
        assert!(json.contains("\"overall\""));
        assert!(json.contains("\"comparisons\""));
    }

    #[test]
    fn report_failures_and_warnings() {
        let detector = RegressionDetector::with_baseline(make_baseline());
        let results = vec![
            BenchmarkResult::lower_is_better("latency", 12.0, "ms"),  // fail
            BenchmarkResult::lower_is_better("memory", 53.5, "MB"),   // warn (7%)
        ];

        let report = detector.check_all(&results);
        assert_eq!(report.failures().len(), 1);
        assert_eq!(report.warnings().len(), 1);
        assert_eq!(report.failures()[0].name, "latency");
        assert_eq!(report.warnings()[0].name, "memory");
    }

    #[test]
    fn report_improved_count() {
        let detector = RegressionDetector::with_baseline(make_baseline());
        let results = vec![
            BenchmarkResult::lower_is_better("latency", 8.0, "ms"),  // improved
            BenchmarkResult::lower_is_better("memory", 50.0, "MB"),  // pass
        ];

        let report = detector.check_all(&results);
        assert_eq!(report.improved_count(), 1);
        assert_eq!(report.pass_count(), 2); // improved counts as pass
    }

    #[test]
    fn custom_threshold_per_benchmark() {
        let mut baseline = make_baseline();
        baseline.set_threshold("latency", Threshold::strict()); // 2% warn, 5% fail

        let detector = RegressionDetector::with_baseline(baseline);
        // 10.0 → 10.3 = 3% increase → WARN with strict, PASS with default
        let result = BenchmarkResult::lower_is_better("latency", 10.3, "ms");

        let cmp = detector.check(&result).unwrap();
        assert_eq!(cmp.verdict, RegressionVerdict::Warn);
    }

    #[test]
    fn zero_baseline_doesnt_panic() {
        let mut baseline = Baseline::new();
        baseline.set("zero_bench", 0.0);

        let detector = RegressionDetector::with_baseline(baseline);
        let result = BenchmarkResult::lower_is_better("zero_bench", 5.0, "ms");

        let cmp = detector.check(&result).unwrap();
        // 0% change since baseline is 0
        assert_eq!(cmp.verdict, RegressionVerdict::Pass);
    }

    #[test]
    fn baseline_empty() {
        let b = Baseline::new();
        assert!(b.is_empty());
        assert_eq!(b.len(), 0);
    }
}
