//! pat-8ekh + pat-lystw: Benchmark dashboard for runtime parity and regressions.
//!
//! Validates the dashboard tooling in `gdcore::dashboard`:
//! - ParityMetric creation and percentage calculation
//! - BenchmarkEntry regression detection
//! - Dashboard aggregation and combined parity
//! - Report generation with real-world parity data
//! - Regression detection across multiple benchmarks
//! - End-to-end dashboard workflow with ClassDB + physics + render metrics
//! - FrameTimeStats with min/max/avg/p99 (pat-lystw)
//! - PhysicsStepMetrics with budget ratio (pat-lystw)
//! - RenderMetrics with draw calls and vertices (pat-lystw)
//! - RuntimeDashboard aggregating all subsystem metrics (pat-lystw)

use gdcore::dashboard::{
    BenchmarkEntry, Dashboard, FrameTimeStats, ParityMetric, PhysicsStepMetrics, RenderMetrics,
    RuntimeDashboard,
};

// ===========================================================================
// 1. Real-world parity dashboard (mirrors actual engine state)
// ===========================================================================

#[test]
fn full_engine_parity_dashboard() {
    let mut dash = Dashboard::new("Patina Engine — Runtime Parity Dashboard");

    // ClassDB parity (from classdb_measurable_parity_test.rs baseline)
    dash.add_parity(ParityMetric::new("ClassDB methods", 312, 400));
    dash.add_parity(ParityMetric::new("ClassDB properties", 52, 83));
    dash.add_parity(ParityMetric::new("ClassDB signals", 28, 28));

    // Lifecycle trace parity (from lifecycle_trace_oracle_parity_test.rs)
    dash.add_parity(ParityMetric::new("Lifecycle ENTER_TREE", 71, 71));
    dash.add_parity(ParityMetric::new("Lifecycle READY", 71, 71));
    dash.add_parity(ParityMetric::new("Lifecycle EXIT_TREE", 71, 71));

    // Physics trace parity (from physics golden tests)
    dash.add_parity(ParityMetric::new("Physics 2D traces", 170, 170));
    dash.add_parity(ParityMetric::new("Physics 3D traces", 10, 10));

    // 3D node subset (from node3d_runtime_slice_test.rs)
    dash.add_parity(ParityMetric::new("3D node classes", 10, 10));

    let combined = dash.combined_parity();
    assert!(
        combined.percentage() > 80.0,
        "combined parity should be > 80%, got {:.1}%",
        combined.percentage()
    );

    let report = dash.render_report();
    assert!(report.contains("ClassDB methods"));
    assert!(report.contains("Lifecycle ENTER_TREE"));
    assert!(report.contains("Physics 3D traces"));
    assert!(report.contains("COMBINED"));
    eprintln!("\n{}", report);
}

// ===========================================================================
// 2. Benchmark regression detection
// ===========================================================================

#[test]
fn render_benchmark_no_regression() {
    let mut dash = Dashboard::new("Render Benchmarks");

    // Simulated render benchmarks based on fixture baselines
    dash.add_benchmark(BenchmarkEntry::new("grid_100_1280x720", 11.1, 11.125, 2.0));
    dash.add_benchmark(BenchmarkEntry::new(
        "layered_5x20_1280x720",
        6.2,
        6.173,
        2.0,
    ));
    dash.add_benchmark(BenchmarkEntry::new(
        "layered_10x50_1280x720",
        14.8,
        15.0,
        2.0,
    ));

    assert!(dash.is_green());
    assert_eq!(dash.regression_count(), 0);

    let report = dash.render_report();
    assert!(report.contains("GREEN"));
    assert!(!report.contains("REGRESS"));
}

#[test]
fn detect_render_regression() {
    let mut dash = Dashboard::new("Regression Check");

    dash.add_benchmark(BenchmarkEntry::new("grid_100", 11.0, 11.0, 2.0)); // OK
    dash.add_benchmark(BenchmarkEntry::new("grid_500", 25.0, 10.0, 2.0)); // REGRESSION (2.5x)

    assert!(!dash.is_green());
    assert_eq!(dash.regression_count(), 1);

    let report = dash.render_report();
    assert!(report.contains("RED"));
    assert!(report.contains("REGRESS"));
    assert!(report.contains("grid_500"));
}

#[test]
fn detect_improvement() {
    let entry = BenchmarkEntry::new("optimized_render", 5.0, 10.0, 2.0);
    assert!(entry.is_improvement());
    assert!((entry.delta_pct() - (-50.0)).abs() < 0.001);
}

// ===========================================================================
// 3. Threshold sensitivity
// ===========================================================================

#[test]
fn strict_threshold_catches_small_regressions() {
    let b = BenchmarkEntry::new("sensitive", 12.0, 10.0, 1.1);
    assert!(b.is_regression()); // 1.2x > 1.1 threshold
}

#[test]
fn lenient_threshold_allows_moderate_slowdown() {
    let b = BenchmarkEntry::new("lenient", 18.0, 10.0, 2.0);
    assert!(!b.is_regression()); // 1.8x < 2.0 threshold
}

// ===========================================================================
// 4. Combined parity across subsystems
// ===========================================================================

#[test]
fn combined_parity_calculation() {
    let mut dash = Dashboard::new("test");
    dash.add_parity(ParityMetric::new("A", 80, 100));
    dash.add_parity(ParityMetric::new("B", 50, 50));
    dash.add_parity(ParityMetric::new("C", 20, 50));

    let combined = dash.combined_parity();
    assert_eq!(combined.matched, 150);
    assert_eq!(combined.total, 200);
    assert!((combined.percentage() - 75.0).abs() < 0.001);
}

#[test]
fn combined_parity_all_perfect() {
    let mut dash = Dashboard::new("perfect");
    dash.add_parity(ParityMetric::new("A", 100, 100));
    dash.add_parity(ParityMetric::new("B", 50, 50));
    let combined = dash.combined_parity();
    assert!(combined.is_full_parity());
}

// ===========================================================================
// 5. Dashboard report format validation
// ===========================================================================

#[test]
fn report_includes_all_parity_metrics() {
    let mut dash = Dashboard::new("Format Test");
    dash.add_parity(ParityMetric::new("ClassDB methods", 312, 400));
    dash.add_parity(ParityMetric::new("Signals", 28, 28));

    let report = dash.render_report();
    assert!(report.contains("ClassDB methods"));
    assert!(report.contains("312"));
    assert!(report.contains("400"));
    assert!(report.contains("Signals"));
    assert!(report.contains("28"));
    assert!(report.contains("100.0%"));
}

#[test]
fn report_includes_benchmark_details() {
    let mut dash = Dashboard::new("Bench Format");
    dash.add_benchmark(BenchmarkEntry::new("render_test", 11.5, 10.0, 2.0));

    let report = dash.render_report();
    assert!(report.contains("render_test"));
    assert!(report.contains("11.500ms"));
    assert!(report.contains("10.000ms"));
    assert!(report.contains("OK"));
}

#[test]
fn report_shows_no_regressions_message() {
    let mut dash = Dashboard::new("Clean");
    dash.add_benchmark(BenchmarkEntry::new("test", 10.0, 10.0, 2.0));
    let report = dash.render_report();
    assert!(report.contains("No regressions detected"));
}

#[test]
fn report_shows_regression_count() {
    let mut dash = Dashboard::new("Regressed");
    dash.add_benchmark(BenchmarkEntry::new("a", 30.0, 10.0, 2.0));
    dash.add_benchmark(BenchmarkEntry::new("b", 25.0, 10.0, 2.0));
    let report = dash.render_report();
    assert!(report.contains("REGRESSIONS DETECTED: 2"));
}

// ===========================================================================
// 6. Edge cases
// ===========================================================================

#[test]
fn empty_dashboard_renders_cleanly() {
    let dash = Dashboard::new("Empty");
    let report = dash.render_report();
    assert!(report.contains("Empty"));
    assert!(report.contains("GREEN"));
}

#[test]
fn parity_only_dashboard() {
    let mut dash = Dashboard::new("Parity Only");
    dash.add_parity(ParityMetric::new("Test", 50, 100));
    let report = dash.render_report();
    assert!(report.contains("PARITY"));
    assert!(!report.contains("BENCHMARKS"));
    assert!(report.contains("GREEN"));
}

#[test]
fn benchmark_only_dashboard() {
    let mut dash = Dashboard::new("Bench Only");
    dash.add_benchmark(BenchmarkEntry::new("test", 5.0, 5.0, 2.0));
    let report = dash.render_report();
    assert!(!report.contains("PARITY"));
    assert!(report.contains("BENCHMARKS"));
}

// ===========================================================================
// 7. Multi-subsystem end-to-end dashboard
// ===========================================================================

#[test]
fn end_to_end_dashboard_workflow() {
    let mut dash = Dashboard::new("Patina v0.1 — CI Dashboard");

    // Parity: ClassDB
    dash.add_parity(ParityMetric::new("ClassDB methods", 312, 400));
    dash.add_parity(ParityMetric::new("ClassDB properties", 52, 83));
    dash.add_parity(ParityMetric::new("ClassDB signals", 28, 28));

    // Parity: Lifecycle
    dash.add_parity(ParityMetric::new("Lifecycle traces", 213, 213));

    // Parity: Physics
    dash.add_parity(ParityMetric::new("Physics 2D golden", 170, 170));
    dash.add_parity(ParityMetric::new("Physics 3D golden", 10, 10));

    // Parity: Render
    dash.add_parity(ParityMetric::new("Render 2D golden", 5, 5));

    // Benchmarks
    dash.add_benchmark(BenchmarkEntry::new("render_grid_100", 11.1, 11.125, 2.0));
    dash.add_benchmark(BenchmarkEntry::new("render_layered_5x20", 6.2, 6.173, 2.0));
    dash.add_benchmark(BenchmarkEntry::new("physics_playground_60f", 0.5, 0.6, 2.0));
    dash.add_benchmark(BenchmarkEntry::new("scene_load_hierarchy", 0.3, 0.4, 2.0));

    // Verify combined parity
    let combined = dash.combined_parity();
    assert!(combined.percentage() > 85.0);

    // Verify no regressions
    assert!(dash.is_green());

    // Verify report
    let report = dash.render_report();
    assert!(report.contains("CI Dashboard"));
    assert!(report.contains("COMBINED"));
    assert!(report.contains("GREEN"));

    // Print the full report (visible with --nocapture)
    eprintln!("\n{}", report);
}

// ===========================================================================
// 8. Regression threshold edge cases
// ===========================================================================

#[test]
fn exactly_at_threshold_is_not_regression() {
    let b = BenchmarkEntry::new("edge", 20.0, 10.0, 2.0);
    // ratio = 2.0, threshold = 2.0, not strictly greater
    assert!(!b.is_regression());
}

#[test]
fn just_above_threshold_is_regression() {
    let b = BenchmarkEntry::new("edge", 20.01, 10.0, 2.0);
    assert!(b.is_regression());
}

#[test]
fn ratio_for_identical_values() {
    let b = BenchmarkEntry::new("stable", 10.0, 10.0, 2.0);
    assert!((b.ratio() - 1.0).abs() < 0.001);
    assert!(!b.is_regression());
    assert!(!b.is_improvement());
}

// ===========================================================================
// 9. FrameTimeStats (pat-lystw)
// ===========================================================================

#[test]
fn frame_time_stats_realistic_60fps() {
    // Simulate 120 frames at ~60 FPS (16.667ms) with some variance
    let mut samples = Vec::new();
    for i in 0..120 {
        let jitter = (i as f64 * 0.1).sin() * 2.0;
        samples.push(16.667 + jitter);
    }

    let stats = FrameTimeStats::from_samples(&samples).unwrap();
    assert!(stats.min_ms < 16.667, "min should be below target");
    assert!(stats.max_ms > 16.667, "max should be above target");
    assert!(
        (stats.avg_ms - 16.667).abs() < 1.0,
        "avg should be near target"
    );
    assert!(
        stats.avg_fps() > 55.0 && stats.avg_fps() < 65.0,
        "FPS should be near 60"
    );
    assert_eq!(stats.sample_count, 120);
}

#[test]
fn frame_time_stats_p99_with_spike() {
    // 99 frames at 16ms, 1 frame at 100ms
    let mut samples = vec![16.0; 99];
    samples.push(100.0);

    let stats = FrameTimeStats::from_samples(&samples).unwrap();
    assert!((stats.min_ms - 16.0).abs() < 0.001);
    assert!((stats.max_ms - 100.0).abs() < 0.001);
    // p99 index = ceil(0.99 * 100) - 1 = 98 → sorted[98] = 16.0 (the 99th of 99 identical values)
    // The spike at index 99 is beyond p99
    assert!((stats.p99_ms - 16.0).abs() < 0.001);
}

#[test]
fn frame_time_stats_json_roundtrip_fields() {
    let stats = FrameTimeStats::from_samples(&[10.0, 20.0, 15.0]).unwrap();
    let json = stats.to_json();
    assert!(json.contains("\"min_ms\":10.0000"));
    assert!(json.contains("\"max_ms\":20.0000"));
    assert!(json.contains("\"sample_count\":3"));
}

// ===========================================================================
// 10. PhysicsStepMetrics (pat-lystw)
// ===========================================================================

#[test]
fn physics_metrics_60tps_well_within_budget() {
    // At 60 TPS, budget = 16.667ms. Steps averaging 0.5ms = 3% budget usage
    let steps: Vec<f64> = (0..60).map(|i| 0.4 + (i as f64 * 0.01) % 0.2).collect();
    let pm = PhysicsStepMetrics::from_step_times(&steps, 25, 60);
    assert!(pm.budget_ratio() < 0.1, "should use < 10% of budget");
    assert_eq!(pm.step_count, 60);
    assert_eq!(pm.body_count, 25);
}

#[test]
fn physics_metrics_over_budget_is_detected() {
    // Step average > budget
    let pm = PhysicsStepMetrics::from_step_times(&[20.0, 18.0, 22.0], 100, 60);
    assert!(pm.budget_ratio() > 1.0, "should exceed physics budget");
}

#[test]
fn physics_metrics_json_fields() {
    let pm = PhysicsStepMetrics::from_step_times(&[1.5], 7, 120);
    let json = pm.to_json();
    assert!(json.contains("\"body_count\":7"));
    assert!(json.contains("\"target_tps\":120"));
    assert!(json.contains("\"budget_ratio\""));
}

// ===========================================================================
// 11. RenderMetrics (pat-lystw)
// ===========================================================================

#[test]
fn render_metrics_realistic_scene() {
    let times: Vec<f64> = (0..60).map(|i| 2.0 + (i as f64 * 0.05) % 1.0).collect();
    let rm = RenderMetrics::from_frame_times(&times, 3600, 540_000, 1920, 1080);
    assert!((rm.avg_draw_calls_per_frame() - 60.0).abs() < 0.001);
    assert!((rm.avg_vertices_per_frame() - 9000.0).abs() < 0.001);
    assert_eq!(rm.viewport_width, 1920);
    assert_eq!(rm.viewport_height, 1080);
}

#[test]
fn render_metrics_json_fields() {
    let rm = RenderMetrics::from_frame_times(&[3.0, 4.0], 50, 10000, 1280, 720);
    let json = rm.to_json();
    assert!(json.contains("\"viewport_width\":1280"));
    assert!(json.contains("\"viewport_height\":720"));
    assert!(json.contains("\"avg_draw_calls_per_frame\":25.00"));
}

// ===========================================================================
// 12. RuntimeDashboard (pat-lystw) — full integration
// ===========================================================================

#[test]
fn runtime_dashboard_full_engine_workflow() {
    let mut dash = RuntimeDashboard::new("Patina v0.1 — Runtime Dashboard");

    // Frame times: 120 frames at ~60 FPS
    let frame_times: Vec<f64> = (0..120)
        .map(|i| 16.0 + (i as f64 * 0.1).sin() * 1.5)
        .collect();
    dash.set_frame_times(&frame_times);

    // Physics: 60 steps at 60 TPS with 15 bodies
    let phys_times: Vec<f64> = (0..60).map(|i| 0.3 + (i as f64 * 0.01) % 0.2).collect();
    dash.set_physics_metrics(PhysicsStepMetrics::from_step_times(&phys_times, 15, 60));

    // Render: 120 frames, 100 draw calls/frame, 5000 verts/frame
    let render_times: Vec<f64> = (0..120).map(|i| 2.0 + (i as f64 * 0.05) % 0.5).collect();
    dash.set_render_metrics(RenderMetrics::from_frame_times(
        &render_times,
        12000,
        600_000,
        1920,
        1080,
    ));

    // Parity
    dash.add_parity(ParityMetric::new("ClassDB methods", 400, 500));
    dash.add_parity(ParityMetric::new("Lifecycle traces", 213, 213));
    dash.add_parity(ParityMetric::new("Physics golden", 180, 180));

    // Benchmarks
    dash.add_benchmark(BenchmarkEntry::new("render_grid", 11.0, 11.0, 2.0));
    dash.add_benchmark(BenchmarkEntry::new("physics_60f", 0.5, 0.6, 2.0));

    // Assertions
    assert!(dash.is_healthy());
    assert_eq!(dash.regression_count(), 0);

    let fs = dash.frame_stats.as_ref().unwrap();
    assert!(fs.avg_fps() > 55.0 && fs.avg_fps() < 65.0);

    let pm = dash.physics_metrics.as_ref().unwrap();
    assert!(pm.budget_ratio() < 0.1);

    let rm = dash.render_metrics.as_ref().unwrap();
    assert!((rm.avg_draw_calls_per_frame() - 100.0).abs() < 0.001);

    let combined = dash.combined_parity();
    assert!(combined.percentage() > 85.0);

    // JSON output
    let json = dash.to_json();
    assert!(json.contains("\"frame_stats\""));
    assert!(json.contains("\"physics_metrics\""));
    assert!(json.contains("\"render_metrics\""));
    assert!(json.contains("\"healthy\":true"));

    // ASCII report
    let report = dash.render_report();
    assert!(report.contains("FRAME TIME"));
    assert!(report.contains("PHYSICS"));
    assert!(report.contains("RENDER"));
    assert!(report.contains("HEALTHY"));

    eprintln!("\n{}", report);
    eprintln!("\nJSON:\n{}", json);
}

#[test]
fn runtime_dashboard_detects_physics_budget_overrun() {
    let mut dash = RuntimeDashboard::new("Overloaded");
    dash.set_physics_metrics(PhysicsStepMetrics::from_step_times(
        &[20.0, 18.0, 22.0],
        200,
        60,
    ));
    assert!(
        !dash.is_healthy(),
        "physics over budget should be unhealthy"
    );
    let report = dash.render_report();
    assert!(report.contains("UNHEALTHY"));
}

#[test]
fn runtime_dashboard_json_minimal() {
    let dash = RuntimeDashboard::new("minimal");
    let json = dash.to_json();
    assert!(json.contains("\"title\":\"minimal\""));
    assert!(json.contains("\"healthy\":true"));
    // No frame_stats/physics/render when not set
    assert!(!json.contains("\"frame_stats\""));
}
