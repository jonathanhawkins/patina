//! Integration tests for nightly build and test infrastructure.
//!
//! Validates the full nightly pipeline configuration, execution tracking,
//! and report generation for bead pat-pj409.

use gdcore::nightly_ci::{
    BuildResult, NightlyConfig, NightlyRunner, Platform, TestResult, TestSuite,
};

// ---------------------------------------------------------------------------
// End-to-end: configure → execute → report
// ---------------------------------------------------------------------------

#[test]
fn full_nightly_pipeline_green() {
    let config = NightlyConfig::default_config();
    let mut runner = NightlyRunner::new(config);
    runner.start(1000.0);

    // All platforms build successfully.
    for platform in Platform::all() {
        runner.record_build(*platform, BuildResult::success_with_artifact(45.0, 15_000_000));
    }

    // All test suites pass.
    for suite in TestSuite::full_nightly() {
        runner.record_test(*suite, TestResult::passed(50, 2, 10.0));
    }

    runner.finish(1600.0);

    assert!(runner.is_green());
    assert!(runner.all_builds_passed());
    assert!(runner.all_tests_passed());
    assert_eq!(runner.failed_build_count(), 0);
    assert_eq!(runner.failed_suite_count(), 0);
    assert!((runner.total_duration_secs().unwrap() - 600.0).abs() < f64::EPSILON);

    let text = runner.render_report();
    assert!(text.contains("GREEN"));
    assert!(text.contains("Nightly Build Report"));

    let json = runner.render_json();
    assert!(json.contains("\"status\": \"green\""));
}

#[test]
fn fast_nightly_pipeline_green() {
    let config = NightlyConfig::fast_config();
    let mut runner = NightlyRunner::new(config);

    runner.record_build(Platform::LinuxX86_64, BuildResult::success(20.0));
    runner.record_test(TestSuite::Unit, TestResult::passed(100, 0, 5.0));
    runner.record_test(TestSuite::Integration, TestResult::passed(50, 3, 15.0));
    runner.record_test(TestSuite::Golden, TestResult::passed(30, 0, 8.0));

    assert!(runner.is_green());
    assert_eq!(runner.total_tests_passed(), 180);
    assert_eq!(runner.total_test_failures(), 0);
}

#[test]
fn nightly_with_build_failure() {
    let config = NightlyConfig::default_config();
    let mut runner = NightlyRunner::new(config);

    runner.record_build(Platform::LinuxX86_64, BuildResult::success(30.0));
    runner.record_build(Platform::MacosX86_64, BuildResult::success(35.0));
    runner.record_build(Platform::MacosAarch64, BuildResult::success(32.0));
    runner.record_build(
        Platform::WindowsX86_64,
        BuildResult::failure(15.0, "MSVC link error: unresolved external"),
    );
    runner.record_build(Platform::Wasm32, BuildResult::success(40.0));

    for suite in TestSuite::full_nightly() {
        runner.record_test(*suite, TestResult::passed(50, 0, 10.0));
    }

    assert!(!runner.is_green());
    assert!(!runner.all_builds_passed());
    assert!(runner.all_tests_passed());
    assert_eq!(runner.failed_build_count(), 1);

    let text = runner.render_report();
    assert!(text.contains("RED"));
    assert!(text.contains("FAIL"));

    let json = runner.render_json();
    assert!(json.contains("\"status\": \"red\""));
    assert!(json.contains("MSVC link error"));
}

#[test]
fn nightly_with_test_failures() {
    let config = NightlyConfig::fast_config();
    let mut runner = NightlyRunner::new(config);

    runner.record_build(Platform::LinuxX86_64, BuildResult::success(30.0));
    runner.record_test(TestSuite::Unit, TestResult::passed(100, 0, 5.0));
    runner.record_test(
        TestSuite::Integration,
        TestResult::failed(
            45,
            5,
            0,
            20.0,
            vec![
                ("test_scene_load".into(), "timeout after 30s".into()),
                ("test_physics_step".into(), "assertion failed: dt > 0".into()),
            ],
        ),
    );
    runner.record_test(TestSuite::Golden, TestResult::passed(30, 0, 8.0));

    assert!(!runner.is_green());
    assert!(runner.all_builds_passed());
    assert!(!runner.all_tests_passed());
    assert_eq!(runner.failed_suite_count(), 1);
    assert_eq!(runner.total_test_failures(), 5);
    assert_eq!(runner.total_tests_passed(), 175);

    let text = runner.render_report();
    assert!(text.contains("FAILURES"));
    assert!(text.contains("test_scene_load"));
    assert!(text.contains("timeout after 30s"));
}

// ---------------------------------------------------------------------------
// Platform configuration
// ---------------------------------------------------------------------------

#[test]
fn platform_triples_are_valid() {
    for p in Platform::all() {
        let triple = p.triple();
        assert!(!triple.is_empty());
        assert!(triple.contains('-'), "Triple should contain dashes: {}", triple);
    }
}

#[test]
fn platform_labels_readable() {
    for p in Platform::all() {
        let label = p.label();
        assert!(!label.is_empty());
        assert!(label.len() > 3);
    }
}

// ---------------------------------------------------------------------------
// Test suite configuration
// ---------------------------------------------------------------------------

#[test]
fn test_suite_cargo_filters_non_empty() {
    for s in TestSuite::all() {
        assert!(!s.cargo_filter().is_empty());
    }
}

#[test]
fn test_suite_timeouts_reasonable() {
    for s in TestSuite::all() {
        let timeout = s.default_timeout_secs();
        assert!(timeout >= 60, "{:?} timeout too low: {}", s, timeout);
        assert!(timeout <= 1800, "{:?} timeout too high: {}", s, timeout);
    }
}

#[test]
fn fast_nightly_suites_are_subset() {
    let fast = TestSuite::fast_nightly();
    let all = TestSuite::all();
    for s in fast {
        assert!(all.contains(s), "{:?} not in all suites", s);
    }
    assert!(fast.len() < all.len());
}

// ---------------------------------------------------------------------------
// NightlyConfig
// ---------------------------------------------------------------------------

#[test]
fn default_config_properties() {
    let config = NightlyConfig::default_config();
    assert_eq!(config.platforms.len(), 5);
    assert_eq!(config.suites.len(), 7);
    assert!(config.collect_artifacts);
    assert!(config.run_benchmarks);
    assert_eq!(config.git_ref, "main");
    assert_eq!(config.max_pipeline_secs, 3600);
}

#[test]
fn fast_config_properties() {
    let config = NightlyConfig::fast_config();
    assert_eq!(config.platforms.len(), 1);
    assert_eq!(config.suites.len(), 3);
    assert!(!config.collect_artifacts);
    assert!(!config.run_benchmarks);
    assert_eq!(config.max_pipeline_secs, 600);
}

#[test]
fn config_custom_git_ref() {
    let config = NightlyConfig::default_config().with_git_ref("v0.2.0-rc1");
    assert_eq!(config.git_ref, "v0.2.0-rc1");
}

#[test]
fn config_total_jobs_count() {
    let config = NightlyConfig::default_config();
    assert_eq!(config.total_jobs(), 12); // 5 platforms + 7 suites
    let fast = NightlyConfig::fast_config();
    assert_eq!(fast.total_jobs(), 4); // 1 platform + 3 suites
}

// ---------------------------------------------------------------------------
// Missing results
// ---------------------------------------------------------------------------

#[test]
fn missing_builds_detected() {
    let config = NightlyConfig::default_config();
    let mut runner = NightlyRunner::new(config);
    runner.record_build(Platform::LinuxX86_64, BuildResult::success(30.0));
    runner.record_build(Platform::Wasm32, BuildResult::success(40.0));
    let missing = runner.missing_builds();
    assert_eq!(missing.len(), 3);
    assert!(missing.contains(&Platform::MacosX86_64));
    assert!(missing.contains(&Platform::MacosAarch64));
    assert!(missing.contains(&Platform::WindowsX86_64));
}

#[test]
fn missing_suites_detected() {
    let config = NightlyConfig::fast_config();
    let mut runner = NightlyRunner::new(config);
    runner.record_test(TestSuite::Unit, TestResult::passed(50, 0, 5.0));
    let missing = runner.missing_suites();
    assert_eq!(missing.len(), 2);
    assert!(missing.contains(&TestSuite::Integration));
    assert!(missing.contains(&TestSuite::Golden));
}

#[test]
fn missing_results_prevent_green() {
    let config = NightlyConfig::fast_config();
    let mut runner = NightlyRunner::new(config);
    // Only record build, no tests
    runner.record_build(Platform::LinuxX86_64, BuildResult::success(30.0));
    assert!(!runner.is_green());
}

// ---------------------------------------------------------------------------
// Report output
// ---------------------------------------------------------------------------

#[test]
fn text_report_includes_missing_platforms() {
    let config = NightlyConfig::default_config();
    let mut runner = NightlyRunner::new(config);
    runner.record_build(Platform::LinuxX86_64, BuildResult::success(30.0));
    let text = runner.render_report();
    assert!(text.contains("MISSING"));
}

#[test]
fn text_report_shows_artifact_sizes() {
    let config = NightlyConfig::fast_config();
    let mut runner = NightlyRunner::new(config);
    runner.record_build(
        Platform::LinuxX86_64,
        BuildResult::success_with_artifact(30.0, 15_728_640),
    );
    runner.record_test(TestSuite::Unit, TestResult::passed(50, 0, 5.0));
    runner.record_test(TestSuite::Integration, TestResult::passed(30, 0, 10.0));
    runner.record_test(TestSuite::Golden, TestResult::passed(20, 0, 8.0));
    let text = runner.render_report();
    assert!(text.contains("15.0MB"));
}

#[test]
fn json_report_has_all_sections() {
    let config = NightlyConfig::fast_config();
    let mut runner = NightlyRunner::new(config);
    runner.start(0.0);
    runner.record_build(Platform::LinuxX86_64, BuildResult::success(30.0));
    runner.record_test(TestSuite::Unit, TestResult::passed(50, 0, 5.0));
    runner.record_test(TestSuite::Integration, TestResult::passed(30, 0, 10.0));
    runner.record_test(TestSuite::Golden, TestResult::passed(20, 0, 8.0));
    runner.finish(60.0);

    let json = runner.render_json();
    assert!(json.contains("\"git_ref\""));
    assert!(json.contains("\"status\""));
    assert!(json.contains("\"total_duration_secs\""));
    assert!(json.contains("\"builds\""));
    assert!(json.contains("\"test_suites\""));
    assert!(json.contains("\"summary\""));
    assert!(json.contains("\"builds_passed\""));
    assert!(json.contains("\"tests_passed\""));
}

#[test]
fn json_report_null_duration_without_timestamps() {
    let config = NightlyConfig::fast_config();
    let runner = NightlyRunner::new(config);
    let json = runner.render_json();
    assert!(json.contains("\"total_duration_secs\": null"));
}

// ---------------------------------------------------------------------------
// Realistic scenario: nightly with mixed results
// ---------------------------------------------------------------------------

#[test]
fn realistic_mixed_nightly() {
    let config = NightlyConfig::default_config().with_git_ref("main");
    let mut runner = NightlyRunner::new(config);
    runner.start(0.0);

    // Builds: 4 pass, 1 fails (wasm)
    runner.record_build(Platform::LinuxX86_64, BuildResult::success_with_artifact(45.0, 20_000_000));
    runner.record_build(Platform::MacosX86_64, BuildResult::success_with_artifact(50.0, 18_000_000));
    runner.record_build(Platform::MacosAarch64, BuildResult::success_with_artifact(42.0, 17_000_000));
    runner.record_build(Platform::WindowsX86_64, BuildResult::success_with_artifact(55.0, 22_000_000));
    runner.record_build(Platform::Wasm32, BuildResult::failure(30.0, "wasm-bindgen version mismatch"));

    // Tests: most pass, one suite has failures
    runner.record_test(TestSuite::Unit, TestResult::passed(200, 5, 8.0));
    runner.record_test(TestSuite::Integration, TestResult::passed(80, 3, 25.0));
    runner.record_test(TestSuite::Golden, TestResult::passed(40, 0, 15.0));
    runner.record_test(TestSuite::OracleParity, TestResult::passed(60, 0, 20.0));
    runner.record_test(
        TestSuite::Benchmark,
        TestResult::failed(
            15,
            1,
            0,
            45.0,
            vec![("bench_render_grid".into(), "regression: 2.3x slower than baseline".into())],
        ),
    );
    runner.record_test(TestSuite::Fuzz, TestResult::passed(1000, 0, 120.0));
    runner.record_test(TestSuite::Stress, TestResult::passed(10, 0, 60.0));

    runner.finish(500.0);

    // Status
    assert!(!runner.is_green());
    assert!(!runner.all_builds_passed());
    assert!(!runner.all_tests_passed());
    assert_eq!(runner.failed_build_count(), 1);
    assert_eq!(runner.failed_suite_count(), 1);
    assert_eq!(runner.total_test_failures(), 1);
    assert_eq!(runner.total_tests_passed(), 1405);
    assert!(runner.missing_builds().is_empty());
    assert!(runner.missing_suites().is_empty());

    // Reports
    let text = runner.render_report();
    assert!(text.contains("RED"));
    assert!(text.contains("wasm-bindgen"));
    assert!(text.contains("bench_render_grid"));
    assert!(text.contains("regression"));
    assert!(text.contains("4/5 passed"));
    assert!(text.contains("1405 passed"));

    let json = runner.render_json();
    assert!(json.contains("\"status\": \"red\""));
    assert!(json.contains("\"builds_passed\": 4"));
    assert!(json.contains("\"builds_total\": 5"));
    assert!(json.contains("\"tests_failed\": 1"));
}
