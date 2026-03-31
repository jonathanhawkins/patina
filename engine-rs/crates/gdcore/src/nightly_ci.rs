//! Nightly build and test infrastructure configuration.
//!
//! Defines the nightly CI pipeline as code: platform build matrix, test suite
//! scheduling, health checks, artifact collection, and structured reporting.
//! This module is the single source of truth for what the nightly build does.
//!
//! ## Usage
//!
//! ```rust,ignore
//! use gdcore::nightly_ci::{NightlyConfig, NightlyRunner, Platform, TestSuite};
//!
//! let config = NightlyConfig::default_config();
//! let mut runner = NightlyRunner::new(config);
//! runner.record_build(Platform::LinuxX86_64, BuildResult::success(45.0));
//! runner.record_test(TestSuite::Unit, TestResult::passed(120, 0, 8.5));
//! println!("{}", runner.render_report());
//! ```

use std::collections::HashMap;

// ---------------------------------------------------------------------------
// Platform
// ---------------------------------------------------------------------------

/// Target platform for a nightly build.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Platform {
    /// Linux x86_64 (primary CI target).
    LinuxX86_64,
    /// macOS x86_64 (Intel Macs).
    MacosX86_64,
    /// macOS aarch64 (Apple Silicon).
    MacosAarch64,
    /// Windows x86_64.
    WindowsX86_64,
    /// WebAssembly (wasm32-unknown-unknown).
    Wasm32,
}

impl Platform {
    /// Returns the Rust target triple.
    pub fn triple(&self) -> &'static str {
        match self {
            Platform::LinuxX86_64 => "x86_64-unknown-linux-gnu",
            Platform::MacosX86_64 => "x86_64-apple-darwin",
            Platform::MacosAarch64 => "aarch64-apple-darwin",
            Platform::WindowsX86_64 => "x86_64-pc-windows-msvc",
            Platform::Wasm32 => "wasm32-unknown-unknown",
        }
    }

    /// Returns a human-readable label.
    pub fn label(&self) -> &'static str {
        match self {
            Platform::LinuxX86_64 => "Linux x86_64",
            Platform::MacosX86_64 => "macOS x86_64",
            Platform::MacosAarch64 => "macOS aarch64",
            Platform::WindowsX86_64 => "Windows x86_64",
            Platform::Wasm32 => "WebAssembly",
        }
    }

    /// Returns all supported platforms.
    pub fn all() -> &'static [Platform] {
        &[
            Platform::LinuxX86_64,
            Platform::MacosX86_64,
            Platform::MacosAarch64,
            Platform::WindowsX86_64,
            Platform::Wasm32,
        ]
    }
}

// ---------------------------------------------------------------------------
// TestSuite
// ---------------------------------------------------------------------------

/// A named test suite in the nightly pipeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TestSuite {
    /// Unit tests (fast, in-process).
    Unit,
    /// Integration tests (cross-crate).
    Integration,
    /// Golden file comparison tests.
    Golden,
    /// Oracle parity tests (vs Godot output).
    OracleParity,
    /// Performance benchmarks.
    Benchmark,
    /// Fuzz / property-based tests.
    Fuzz,
    /// Stress / concurrency tests.
    Stress,
}

impl TestSuite {
    /// Returns a human-readable label.
    pub fn label(&self) -> &'static str {
        match self {
            TestSuite::Unit => "Unit Tests",
            TestSuite::Integration => "Integration Tests",
            TestSuite::Golden => "Golden Comparison",
            TestSuite::OracleParity => "Oracle Parity",
            TestSuite::Benchmark => "Benchmarks",
            TestSuite::Fuzz => "Fuzz Tests",
            TestSuite::Stress => "Stress Tests",
        }
    }

    /// Returns the cargo test filter pattern for this suite.
    pub fn cargo_filter(&self) -> &'static str {
        match self {
            TestSuite::Unit => "--lib",
            TestSuite::Integration => "--tests",
            TestSuite::Golden => "--test '*golden*'",
            TestSuite::OracleParity => "--test '*oracle*' --test '*parity*'",
            TestSuite::Benchmark => "--test '*bench*'",
            TestSuite::Fuzz => "--test '*fuzz*'",
            TestSuite::Stress => "--test '*stress*'",
        }
    }

    /// Returns the default timeout in seconds.
    pub fn default_timeout_secs(&self) -> u64 {
        match self {
            TestSuite::Unit => 120,
            TestSuite::Integration => 300,
            TestSuite::Golden => 180,
            TestSuite::OracleParity => 300,
            TestSuite::Benchmark => 600,
            TestSuite::Fuzz => 900,
            TestSuite::Stress => 600,
        }
    }

    /// Returns all test suites.
    pub fn all() -> &'static [TestSuite] {
        &[
            TestSuite::Unit,
            TestSuite::Integration,
            TestSuite::Golden,
            TestSuite::OracleParity,
            TestSuite::Benchmark,
            TestSuite::Fuzz,
            TestSuite::Stress,
        ]
    }

    /// Returns the suites that run in the fast nightly (skip long-running).
    pub fn fast_nightly() -> &'static [TestSuite] {
        &[TestSuite::Unit, TestSuite::Integration, TestSuite::Golden]
    }

    /// Returns the suites for the full nightly.
    pub fn full_nightly() -> &'static [TestSuite] {
        Self::all()
    }
}

// ---------------------------------------------------------------------------
// BuildResult / TestResult
// ---------------------------------------------------------------------------

/// Outcome of a platform build.
#[derive(Debug, Clone)]
pub struct BuildResult {
    /// Whether the build succeeded.
    pub success: bool,
    /// Build duration in seconds.
    pub duration_secs: f64,
    /// Error message if the build failed.
    pub error: Option<String>,
    /// Artifact size in bytes (if produced).
    pub artifact_bytes: Option<u64>,
}

impl BuildResult {
    /// Creates a successful build result.
    pub fn success(duration_secs: f64) -> Self {
        Self {
            success: true,
            duration_secs,
            error: None,
            artifact_bytes: None,
        }
    }

    /// Creates a successful build with artifact size.
    pub fn success_with_artifact(duration_secs: f64, artifact_bytes: u64) -> Self {
        Self {
            success: true,
            duration_secs,
            error: None,
            artifact_bytes: Some(artifact_bytes),
        }
    }

    /// Creates a failed build result.
    pub fn failure(duration_secs: f64, error: &str) -> Self {
        Self {
            success: false,
            duration_secs,
            error: Some(error.to_owned()),
            artifact_bytes: None,
        }
    }
}

/// Outcome of a test suite run.
#[derive(Debug, Clone)]
pub struct TestResult {
    /// Whether all tests passed.
    pub success: bool,
    /// Number of tests that passed.
    pub passed: usize,
    /// Number of tests that failed.
    pub failed: usize,
    /// Number of tests skipped/ignored.
    pub skipped: usize,
    /// Total duration in seconds.
    pub duration_secs: f64,
    /// Failure details (test name → error message).
    pub failures: Vec<(String, String)>,
}

impl TestResult {
    /// Creates a passing test result.
    pub fn passed(count: usize, skipped: usize, duration_secs: f64) -> Self {
        Self {
            success: true,
            passed: count,
            failed: 0,
            skipped,
            duration_secs,
            failures: Vec::new(),
        }
    }

    /// Creates a failing test result.
    pub fn failed(
        passed: usize,
        failed: usize,
        skipped: usize,
        duration_secs: f64,
        failures: Vec<(String, String)>,
    ) -> Self {
        Self {
            success: false,
            passed,
            failed,
            skipped,
            duration_secs,
            failures,
        }
    }

    /// Returns total tests run (passed + failed).
    pub fn total_run(&self) -> usize {
        self.passed + self.failed
    }
}

// ---------------------------------------------------------------------------
// NightlyConfig
// ---------------------------------------------------------------------------

/// Configuration for the nightly build pipeline.
#[derive(Debug, Clone)]
pub struct NightlyConfig {
    /// Platforms to build on.
    pub platforms: Vec<Platform>,
    /// Test suites to run.
    pub suites: Vec<TestSuite>,
    /// Whether to collect artifacts (binaries, reports).
    pub collect_artifacts: bool,
    /// Whether to run benchmarks and compare against baselines.
    pub run_benchmarks: bool,
    /// Maximum total pipeline duration in seconds.
    pub max_pipeline_secs: u64,
    /// Git ref to build (branch or tag).
    pub git_ref: String,
}

impl NightlyConfig {
    /// Creates the default full nightly configuration.
    pub fn default_config() -> Self {
        Self {
            platforms: Platform::all().to_vec(),
            suites: TestSuite::full_nightly().to_vec(),
            collect_artifacts: true,
            run_benchmarks: true,
            max_pipeline_secs: 3600,
            git_ref: "main".to_owned(),
        }
    }

    /// Creates a fast nightly (Linux only, fast suites).
    pub fn fast_config() -> Self {
        Self {
            platforms: vec![Platform::LinuxX86_64],
            suites: TestSuite::fast_nightly().to_vec(),
            collect_artifacts: false,
            run_benchmarks: false,
            max_pipeline_secs: 600,
            git_ref: "main".to_owned(),
        }
    }

    /// Sets the git ref.
    pub fn with_git_ref(mut self, git_ref: &str) -> Self {
        self.git_ref = git_ref.to_owned();
        self
    }

    /// Returns the total number of jobs (platforms × suites + builds).
    pub fn total_jobs(&self) -> usize {
        self.platforms.len() + self.suites.len()
    }
}

// ---------------------------------------------------------------------------
// NightlyRunner
// ---------------------------------------------------------------------------

/// Tracks the execution of a nightly build pipeline.
#[derive(Debug)]
pub struct NightlyRunner {
    config: NightlyConfig,
    build_results: HashMap<Platform, BuildResult>,
    test_results: HashMap<TestSuite, TestResult>,
    start_time: Option<f64>,
    end_time: Option<f64>,
}

impl NightlyRunner {
    /// Creates a new runner with the given configuration.
    pub fn new(config: NightlyConfig) -> Self {
        Self {
            config,
            build_results: HashMap::new(),
            test_results: HashMap::new(),
            start_time: None,
            end_time: None,
        }
    }

    /// Marks the pipeline as started at the given timestamp (epoch seconds).
    pub fn start(&mut self, timestamp: f64) {
        self.start_time = Some(timestamp);
    }

    /// Marks the pipeline as finished at the given timestamp.
    pub fn finish(&mut self, timestamp: f64) {
        self.end_time = Some(timestamp);
    }

    /// Records a build result for a platform.
    pub fn record_build(&mut self, platform: Platform, result: BuildResult) {
        self.build_results.insert(platform, result);
    }

    /// Records a test suite result.
    pub fn record_test(&mut self, suite: TestSuite, result: TestResult) {
        self.test_results.insert(suite, result);
    }

    /// Returns the configuration.
    pub fn config(&self) -> &NightlyConfig {
        &self.config
    }

    /// Returns the total pipeline duration in seconds, if both times are set.
    pub fn total_duration_secs(&self) -> Option<f64> {
        match (self.start_time, self.end_time) {
            (Some(s), Some(e)) => Some(e - s),
            _ => None,
        }
    }

    /// Returns true if all builds succeeded.
    pub fn all_builds_passed(&self) -> bool {
        self.config
            .platforms
            .iter()
            .all(|p| self.build_results.get(p).map_or(false, |r| r.success))
    }

    /// Returns true if all test suites passed.
    pub fn all_tests_passed(&self) -> bool {
        self.config
            .suites
            .iter()
            .all(|s| self.test_results.get(s).map_or(false, |r| r.success))
    }

    /// Returns true if the entire nightly is green (all builds + all tests).
    pub fn is_green(&self) -> bool {
        self.all_builds_passed() && self.all_tests_passed()
    }

    /// Returns the number of failed builds.
    pub fn failed_build_count(&self) -> usize {
        self.build_results.values().filter(|r| !r.success).count()
    }

    /// Returns the number of failed test suites.
    pub fn failed_suite_count(&self) -> usize {
        self.test_results.values().filter(|r| !r.success).count()
    }

    /// Returns the total number of individual test failures across all suites.
    pub fn total_test_failures(&self) -> usize {
        self.test_results.values().map(|r| r.failed).sum()
    }

    /// Returns the total number of individual tests passed across all suites.
    pub fn total_tests_passed(&self) -> usize {
        self.test_results.values().map(|r| r.passed).sum()
    }

    /// Returns missing builds (platforms configured but no result recorded).
    pub fn missing_builds(&self) -> Vec<Platform> {
        self.config
            .platforms
            .iter()
            .filter(|p| !self.build_results.contains_key(p))
            .copied()
            .collect()
    }

    /// Returns missing test suites.
    pub fn missing_suites(&self) -> Vec<TestSuite> {
        self.config
            .suites
            .iter()
            .filter(|s| !self.test_results.contains_key(s))
            .copied()
            .collect()
    }

    /// Renders a human-readable nightly report.
    pub fn render_report(&self) -> String {
        let mut out = String::new();

        out.push_str(&format!(
            "=== Nightly Build Report ({}) ===\n",
            self.config.git_ref
        ));

        if let Some(dur) = self.total_duration_secs() {
            out.push_str(&format!("Total duration: {:.0}s\n", dur));
        }
        out.push('\n');

        // Builds
        out.push_str("BUILDS\n");
        out.push_str(&format!(
            "{:<20} {:>10} {:>12} {:>10}\n",
            "Platform", "Status", "Duration", "Artifact"
        ));
        out.push_str(&format!(
            "{:-<20} {:-<10} {:-<12} {:-<10}\n",
            "", "", "", ""
        ));

        for platform in &self.config.platforms {
            if let Some(result) = self.build_results.get(platform) {
                let status = if result.success { "PASS" } else { "FAIL" };
                let artifact = result
                    .artifact_bytes
                    .map(|b| format_bytes(b))
                    .unwrap_or_else(|| "-".to_string());
                out.push_str(&format!(
                    "{:<20} {:>10} {:>10.1}s {:>10}\n",
                    platform.label(),
                    status,
                    result.duration_secs,
                    artifact,
                ));
            } else {
                out.push_str(&format!(
                    "{:<20} {:>10} {:>12} {:>10}\n",
                    platform.label(),
                    "MISSING",
                    "-",
                    "-",
                ));
            }
        }

        // Build errors
        let build_errors: Vec<_> = self
            .config
            .platforms
            .iter()
            .filter_map(|p| {
                self.build_results
                    .get(p)
                    .and_then(|r| r.error.as_deref().map(|e| (p.label(), e)))
            })
            .collect();
        if !build_errors.is_empty() {
            out.push_str(&format!("\nBUILD ERRORS ({})\n", build_errors.len()));
            for (label, err) in &build_errors {
                out.push_str(&format!("  [{}] {}\n", label, err));
            }
        }
        out.push('\n');

        // Tests
        out.push_str("TEST SUITES\n");
        out.push_str(&format!(
            "{:<25} {:>8} {:>8} {:>8} {:>10} {:>8}\n",
            "Suite", "Passed", "Failed", "Skip", "Duration", "Status"
        ));
        out.push_str(&format!(
            "{:-<25} {:-<8} {:-<8} {:-<8} {:-<10} {:-<8}\n",
            "", "", "", "", "", ""
        ));

        for suite in &self.config.suites {
            if let Some(result) = self.test_results.get(suite) {
                let status = if result.success { "PASS" } else { "FAIL" };
                out.push_str(&format!(
                    "{:<25} {:>8} {:>8} {:>8} {:>8.1}s {:>8}\n",
                    suite.label(),
                    result.passed,
                    result.failed,
                    result.skipped,
                    result.duration_secs,
                    status,
                ));
            } else {
                out.push_str(&format!(
                    "{:<25} {:>8} {:>8} {:>8} {:>10} {:>8}\n",
                    suite.label(),
                    "-",
                    "-",
                    "-",
                    "-",
                    "MISSING",
                ));
            }
        }
        out.push('\n');

        // Failures
        let all_failures: Vec<_> = self
            .test_results
            .iter()
            .flat_map(|(suite, result)| {
                result
                    .failures
                    .iter()
                    .map(move |(name, msg)| (suite.label(), name.as_str(), msg.as_str()))
            })
            .collect();
        if !all_failures.is_empty() {
            out.push_str(&format!("FAILURES ({})\n", all_failures.len()));
            for (suite, name, msg) in &all_failures {
                out.push_str(&format!("  [{}] {} — {}\n", suite, name, msg));
            }
            out.push('\n');
        }

        // Summary
        out.push_str(&format!(
            "Status: {}\n",
            if self.is_green() { "GREEN" } else { "RED" }
        ));
        out.push_str(&format!(
            "Builds: {}/{} passed\n",
            self.build_results.values().filter(|r| r.success).count(),
            self.config.platforms.len(),
        ));
        out.push_str(&format!(
            "Tests: {} passed, {} failed across {} suites\n",
            self.total_tests_passed(),
            self.total_test_failures(),
            self.config.suites.len(),
        ));

        out
    }

    /// Renders a JSON report.
    pub fn render_json(&self) -> String {
        let builds_json: Vec<String> = self
            .config
            .platforms
            .iter()
            .map(|p| {
                if let Some(r) = self.build_results.get(p) {
                    let err = r
                        .error
                        .as_deref()
                        .map(|e| format!("\"{}\"", escape_json(e)))
                        .unwrap_or_else(|| "null".to_string());
                    let artifact = r
                        .artifact_bytes
                        .map(|b| b.to_string())
                        .unwrap_or_else(|| "null".to_string());
                    format!(
                        "    {{\"platform\": \"{}\", \"triple\": \"{}\", \"success\": {}, \"duration_secs\": {:.1}, \"error\": {}, \"artifact_bytes\": {}}}",
                        p.label(),
                        p.triple(),
                        r.success,
                        r.duration_secs,
                        err,
                        artifact,
                    )
                } else {
                    format!(
                        "    {{\"platform\": \"{}\", \"triple\": \"{}\", \"success\": null}}",
                        p.label(),
                        p.triple(),
                    )
                }
            })
            .collect();

        let suites_json: Vec<String> = self
            .config
            .suites
            .iter()
            .map(|s| {
                if let Some(r) = self.test_results.get(s) {
                    format!(
                        "    {{\"suite\": \"{}\", \"success\": {}, \"passed\": {}, \"failed\": {}, \"skipped\": {}, \"duration_secs\": {:.1}}}",
                        s.label(),
                        r.success,
                        r.passed,
                        r.failed,
                        r.skipped,
                        r.duration_secs,
                    )
                } else {
                    format!(
                        "    {{\"suite\": \"{}\", \"success\": null}}",
                        s.label(),
                    )
                }
            })
            .collect();

        let duration = self
            .total_duration_secs()
            .map(|d| format!("{:.1}", d))
            .unwrap_or_else(|| "null".to_string());

        format!(
            concat!(
                "{{\n",
                "  \"git_ref\": \"{}\",\n",
                "  \"status\": \"{}\",\n",
                "  \"total_duration_secs\": {},\n",
                "  \"builds\": [\n{}\n  ],\n",
                "  \"test_suites\": [\n{}\n  ],\n",
                "  \"summary\": {{\n",
                "    \"builds_passed\": {},\n",
                "    \"builds_total\": {},\n",
                "    \"tests_passed\": {},\n",
                "    \"tests_failed\": {},\n",
                "    \"suites_passed\": {},\n",
                "    \"suites_total\": {}\n",
                "  }}\n",
                "}}"
            ),
            escape_json(&self.config.git_ref),
            if self.is_green() { "green" } else { "red" },
            duration,
            builds_json.join(",\n"),
            suites_json.join(",\n"),
            self.build_results.values().filter(|r| r.success).count(),
            self.config.platforms.len(),
            self.total_tests_passed(),
            self.total_test_failures(),
            self.test_results.values().filter(|r| r.success).count(),
            self.config.suites.len(),
        )
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn format_bytes(bytes: u64) -> String {
    if bytes >= 1_073_741_824 {
        format!("{:.1}GB", bytes as f64 / 1_073_741_824.0)
    } else if bytes >= 1_048_576 {
        format!("{:.1}MB", bytes as f64 / 1_048_576.0)
    } else if bytes >= 1024 {
        format!("{:.1}KB", bytes as f64 / 1024.0)
    } else {
        format!("{}B", bytes)
    }
}

fn escape_json(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn platform_all_has_five() {
        assert_eq!(Platform::all().len(), 5);
    }

    #[test]
    fn platform_triples() {
        assert_eq!(Platform::LinuxX86_64.triple(), "x86_64-unknown-linux-gnu");
        assert_eq!(Platform::MacosAarch64.triple(), "aarch64-apple-darwin");
        assert_eq!(Platform::Wasm32.triple(), "wasm32-unknown-unknown");
    }

    #[test]
    fn platform_labels() {
        assert_eq!(Platform::LinuxX86_64.label(), "Linux x86_64");
        assert_eq!(Platform::WindowsX86_64.label(), "Windows x86_64");
    }

    #[test]
    fn test_suite_all_has_seven() {
        assert_eq!(TestSuite::all().len(), 7);
    }

    #[test]
    fn test_suite_fast_nightly_subset() {
        let fast = TestSuite::fast_nightly();
        assert_eq!(fast.len(), 3);
        assert!(fast.contains(&TestSuite::Unit));
        assert!(fast.contains(&TestSuite::Integration));
        assert!(fast.contains(&TestSuite::Golden));
        assert!(!fast.contains(&TestSuite::Fuzz));
    }

    #[test]
    fn test_suite_labels() {
        assert_eq!(TestSuite::Unit.label(), "Unit Tests");
        assert_eq!(TestSuite::OracleParity.label(), "Oracle Parity");
    }

    #[test]
    fn test_suite_cargo_filters() {
        assert_eq!(TestSuite::Unit.cargo_filter(), "--lib");
        assert_eq!(TestSuite::Integration.cargo_filter(), "--tests");
    }

    #[test]
    fn test_suite_timeouts() {
        assert!(TestSuite::Unit.default_timeout_secs() < TestSuite::Fuzz.default_timeout_secs());
    }

    #[test]
    fn build_result_success() {
        let r = BuildResult::success(42.5);
        assert!(r.success);
        assert!((r.duration_secs - 42.5).abs() < f64::EPSILON);
        assert!(r.error.is_none());
    }

    #[test]
    fn build_result_with_artifact() {
        let r = BuildResult::success_with_artifact(30.0, 5_000_000);
        assert!(r.success);
        assert_eq!(r.artifact_bytes, Some(5_000_000));
    }

    #[test]
    fn build_result_failure() {
        let r = BuildResult::failure(10.0, "link error");
        assert!(!r.success);
        assert_eq!(r.error.as_deref(), Some("link error"));
    }

    #[test]
    fn test_result_passed() {
        let r = TestResult::passed(100, 5, 8.0);
        assert!(r.success);
        assert_eq!(r.passed, 100);
        assert_eq!(r.failed, 0);
        assert_eq!(r.skipped, 5);
        assert_eq!(r.total_run(), 100);
    }

    #[test]
    fn test_result_failed() {
        let r = TestResult::failed(
            90,
            3,
            2,
            12.0,
            vec![("test_foo".into(), "assertion failed".into())],
        );
        assert!(!r.success);
        assert_eq!(r.total_run(), 93);
        assert_eq!(r.failures.len(), 1);
    }

    #[test]
    fn default_config_has_all_platforms() {
        let config = NightlyConfig::default_config();
        assert_eq!(config.platforms.len(), 5);
        assert!(config.collect_artifacts);
        assert!(config.run_benchmarks);
    }

    #[test]
    fn fast_config_linux_only() {
        let config = NightlyConfig::fast_config();
        assert_eq!(config.platforms.len(), 1);
        assert_eq!(config.platforms[0], Platform::LinuxX86_64);
        assert!(!config.collect_artifacts);
    }

    #[test]
    fn config_total_jobs() {
        let config = NightlyConfig::default_config();
        assert_eq!(config.total_jobs(), 5 + 7); // 5 platforms + 7 suites
    }

    #[test]
    fn config_with_git_ref() {
        let config = NightlyConfig::default_config().with_git_ref("release/0.1");
        assert_eq!(config.git_ref, "release/0.1");
    }

    #[test]
    fn runner_empty_is_not_green() {
        let runner = NightlyRunner::new(NightlyConfig::fast_config());
        assert!(!runner.is_green());
        assert!(!runner.all_builds_passed());
        assert!(!runner.all_tests_passed());
    }

    #[test]
    fn runner_all_pass_is_green() {
        let config = NightlyConfig::fast_config();
        let mut runner = NightlyRunner::new(config);
        runner.record_build(Platform::LinuxX86_64, BuildResult::success(30.0));
        runner.record_test(TestSuite::Unit, TestResult::passed(50, 0, 5.0));
        runner.record_test(TestSuite::Integration, TestResult::passed(30, 0, 10.0));
        runner.record_test(TestSuite::Golden, TestResult::passed(20, 0, 8.0));
        assert!(runner.is_green());
    }

    #[test]
    fn runner_build_failure_is_red() {
        let config = NightlyConfig::fast_config();
        let mut runner = NightlyRunner::new(config);
        runner.record_build(
            Platform::LinuxX86_64,
            BuildResult::failure(10.0, "compile error"),
        );
        runner.record_test(TestSuite::Unit, TestResult::passed(50, 0, 5.0));
        runner.record_test(TestSuite::Integration, TestResult::passed(30, 0, 10.0));
        runner.record_test(TestSuite::Golden, TestResult::passed(20, 0, 8.0));
        assert!(!runner.is_green());
        assert_eq!(runner.failed_build_count(), 1);
    }

    #[test]
    fn runner_test_failure_is_red() {
        let config = NightlyConfig::fast_config();
        let mut runner = NightlyRunner::new(config);
        runner.record_build(Platform::LinuxX86_64, BuildResult::success(30.0));
        runner.record_test(TestSuite::Unit, TestResult::passed(50, 0, 5.0));
        runner.record_test(
            TestSuite::Integration,
            TestResult::failed(28, 2, 0, 10.0, vec![]),
        );
        runner.record_test(TestSuite::Golden, TestResult::passed(20, 0, 8.0));
        assert!(!runner.is_green());
        assert_eq!(runner.failed_suite_count(), 1);
        assert_eq!(runner.total_test_failures(), 2);
    }

    #[test]
    fn runner_missing_builds() {
        let config = NightlyConfig::default_config();
        let mut runner = NightlyRunner::new(config);
        runner.record_build(Platform::LinuxX86_64, BuildResult::success(30.0));
        let missing = runner.missing_builds();
        assert_eq!(missing.len(), 4);
    }

    #[test]
    fn runner_duration() {
        let config = NightlyConfig::fast_config();
        let mut runner = NightlyRunner::new(config);
        runner.start(1000.0);
        runner.finish(1300.0);
        assert!((runner.total_duration_secs().unwrap() - 300.0).abs() < f64::EPSILON);
    }

    #[test]
    fn runner_total_tests_passed() {
        let config = NightlyConfig::fast_config();
        let mut runner = NightlyRunner::new(config);
        runner.record_test(TestSuite::Unit, TestResult::passed(50, 0, 5.0));
        runner.record_test(TestSuite::Integration, TestResult::passed(30, 0, 10.0));
        assert_eq!(runner.total_tests_passed(), 80);
    }

    #[test]
    fn format_bytes_helper() {
        assert_eq!(format_bytes(500), "500B");
        assert_eq!(format_bytes(2048), "2.0KB");
        assert_eq!(format_bytes(5_242_880), "5.0MB");
        assert_eq!(format_bytes(2_147_483_648), "2.0GB");
    }

    #[test]
    fn report_text_contains_sections() {
        let config = NightlyConfig::fast_config();
        let mut runner = NightlyRunner::new(config);
        runner.start(0.0);
        runner.record_build(Platform::LinuxX86_64, BuildResult::success(30.0));
        runner.record_test(TestSuite::Unit, TestResult::passed(50, 0, 5.0));
        runner.record_test(TestSuite::Integration, TestResult::passed(30, 0, 10.0));
        runner.record_test(TestSuite::Golden, TestResult::passed(20, 0, 8.0));
        runner.finish(60.0);
        let report = runner.render_report();
        assert!(report.contains("Nightly Build Report"));
        assert!(report.contains("BUILDS"));
        assert!(report.contains("TEST SUITES"));
        assert!(report.contains("GREEN"));
    }

    #[test]
    fn report_text_shows_failures() {
        let config = NightlyConfig::fast_config();
        let mut runner = NightlyRunner::new(config);
        runner.record_build(Platform::LinuxX86_64, BuildResult::success(30.0));
        runner.record_test(
            TestSuite::Unit,
            TestResult::failed(
                48,
                2,
                0,
                5.0,
                vec![("test_a".into(), "assert failed".into())],
            ),
        );
        runner.record_test(TestSuite::Integration, TestResult::passed(30, 0, 10.0));
        runner.record_test(TestSuite::Golden, TestResult::passed(20, 0, 8.0));
        let report = runner.render_report();
        assert!(report.contains("FAILURES"));
        assert!(report.contains("test_a"));
        assert!(report.contains("RED"));
    }

    #[test]
    fn report_json_structure() {
        let config = NightlyConfig::fast_config();
        let mut runner = NightlyRunner::new(config);
        runner.record_build(Platform::LinuxX86_64, BuildResult::success(30.0));
        runner.record_test(TestSuite::Unit, TestResult::passed(50, 0, 5.0));
        runner.record_test(TestSuite::Integration, TestResult::passed(30, 0, 10.0));
        runner.record_test(TestSuite::Golden, TestResult::passed(20, 0, 8.0));
        let json = runner.render_json();
        assert!(json.contains("\"git_ref\": \"main\""));
        assert!(json.contains("\"status\": \"green\""));
        assert!(json.contains("\"builds\""));
        assert!(json.contains("\"test_suites\""));
        assert!(json.contains("\"summary\""));
    }
}
