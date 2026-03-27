//! Integration tests for the Patina vs Godot performance comparison report.
//!
//! Validates the full comparison pipeline: measurements → benchmarks → report
//! → text/JSON output → verdicts for bead pat-po3r7.

use gdcore::perf_comparison::{
    ComparisonReport, Measurement, OverallVerdict, SubsystemBenchmark, Verdict,
};

// ---------------------------------------------------------------------------
// End-to-end: build report and verify output
// ---------------------------------------------------------------------------

#[test]
fn full_report_pipeline() {
    let mut report = ComparisonReport::new("4.6.1", "0.1.0");

    report.add(SubsystemBenchmark::new(
        "scene_tree",
        "Instantiate 1000 nodes",
        Measurement::new(12.5, "ms"),
        Measurement::new(14.2, "ms"),
    ));
    report.add(SubsystemBenchmark::new(
        "scene_tree",
        "Free 1000 nodes",
        Measurement::new(8.3, "ms"),
        Measurement::new(7.1, "ms"),
    ));
    report.add(SubsystemBenchmark::new(
        "physics",
        "30-frame sim",
        Measurement::new(4.2, "ms"),
        Measurement::new(4.5, "ms"),
    ));
    report.add(SubsystemBenchmark::new_higher_better(
        "render",
        "2D sprites FPS",
        Measurement::new(120.0, "fps"),
        Measurement::new(115.0, "fps"),
    ));
    report.add(SubsystemBenchmark::new(
        "resource",
        "Load .tscn (50 nodes)",
        Measurement::new(3.1, "ms"),
        Measurement::new(2.8, "ms"),
    ));

    // Text report
    let text = report.render_text();
    assert!(text.contains("Godot 4.6.1 vs Patina 0.1.0"));
    assert!(text.contains("scene_tree"));
    assert!(text.contains("physics"));
    assert!(text.contains("render"));
    assert!(text.contains("resource"));
    assert!(text.contains("Summary"));
    assert!(text.contains("Total benchmarks: 5"));

    // JSON report
    let json = report.render_json();
    assert!(json.contains("\"godot_version\": \"4.6.1\""));
    assert!(json.contains("\"patina_version\": \"0.1.0\""));
    assert!(json.contains("\"benchmark_count\": 5"));
}

// ---------------------------------------------------------------------------
// Measurement construction
// ---------------------------------------------------------------------------

#[test]
fn measurement_stores_value_and_unit() {
    let m = Measurement::new(42.5, "MB");
    assert!((m.value - 42.5).abs() < f64::EPSILON);
    assert_eq!(m.unit, "MB");
}

#[test]
fn measurement_zero_value() {
    let m = Measurement::new(0.0, "ms");
    assert!((m.value).abs() < f64::EPSILON);
}

// ---------------------------------------------------------------------------
// Lower-is-better benchmarks
// ---------------------------------------------------------------------------

#[test]
fn lower_is_better_patina_much_faster() {
    let b = SubsystemBenchmark::new(
        "scene",
        "Test",
        Measurement::new(100.0, "ms"),
        Measurement::new(50.0, "ms"),
    );
    assert!((b.ratio() - 0.5).abs() < 0.01);
    assert!(b.delta_pct() < -5.0);
    assert_eq!(b.verdict(10.0), Verdict::PatinaFaster);
}

#[test]
fn lower_is_better_slightly_slower_within_tolerance() {
    let b = SubsystemBenchmark::new(
        "scene",
        "Test",
        Measurement::new(100.0, "ms"),
        Measurement::new(108.0, "ms"),
    );
    // 8% slower, within 10% tolerance
    assert_eq!(b.verdict(10.0), Verdict::Comparable);
}

#[test]
fn lower_is_better_significantly_slower() {
    let b = SubsystemBenchmark::new(
        "scene",
        "Test",
        Measurement::new(100.0, "ms"),
        Measurement::new(150.0, "ms"),
    );
    // 50% slower
    assert_eq!(b.verdict(10.0), Verdict::GodotFaster);
}

// ---------------------------------------------------------------------------
// Higher-is-better benchmarks
// ---------------------------------------------------------------------------

#[test]
fn higher_is_better_patina_better() {
    let b = SubsystemBenchmark::new_higher_better(
        "render",
        "FPS",
        Measurement::new(60.0, "fps"),
        Measurement::new(90.0, "fps"),
    );
    assert!(b.delta_pct() < 0.0);
    assert_eq!(b.verdict(10.0), Verdict::PatinaFaster);
}

#[test]
fn higher_is_better_comparable() {
    let b = SubsystemBenchmark::new_higher_better(
        "render",
        "FPS",
        Measurement::new(60.0, "fps"),
        Measurement::new(58.0, "fps"),
    );
    // ~3% worse, within 10% tolerance
    assert_eq!(b.verdict(10.0), Verdict::Comparable);
}

#[test]
fn higher_is_better_godot_better() {
    let b = SubsystemBenchmark::new_higher_better(
        "render",
        "FPS",
        Measurement::new(60.0, "fps"),
        Measurement::new(30.0, "fps"),
    );
    // 50% fewer fps
    assert_eq!(b.verdict(10.0), Verdict::GodotFaster);
}

// ---------------------------------------------------------------------------
// Edge cases
// ---------------------------------------------------------------------------

#[test]
fn zero_godot_value_does_not_panic() {
    let b = SubsystemBenchmark::new(
        "misc",
        "Zero",
        Measurement::new(0.0, "ms"),
        Measurement::new(5.0, "ms"),
    );
    assert!((b.ratio() - 1.0).abs() < f64::EPSILON);
    assert!((b.delta_pct()).abs() < f64::EPSILON);
}

#[test]
fn identical_values_are_comparable() {
    let b = SubsystemBenchmark::new(
        "scene",
        "Same",
        Measurement::new(10.0, "ms"),
        Measurement::new(10.0, "ms"),
    );
    assert!((b.ratio() - 1.0).abs() < f64::EPSILON);
    assert_eq!(b.verdict(10.0), Verdict::Comparable);
}

// ---------------------------------------------------------------------------
// Overall verdicts
// ---------------------------------------------------------------------------

#[test]
fn empty_report_no_data() {
    let report = ComparisonReport::new("4.6.1", "0.1.0");
    assert_eq!(report.overall_verdict(), OverallVerdict::NoData);
}

#[test]
fn all_faster_patina_wins() {
    let mut report = ComparisonReport::new("4.6.1", "0.1.0");
    for _ in 0..5 {
        report.add(SubsystemBenchmark::new(
            "scene",
            "Fast",
            Measurement::new(20.0, "ms"),
            Measurement::new(10.0, "ms"),
        ));
    }
    assert_eq!(report.overall_verdict(), OverallVerdict::PatinaWins);
}

#[test]
fn all_comparable_verdict() {
    let mut report = ComparisonReport::new("4.6.1", "0.1.0");
    for _ in 0..5 {
        report.add(SubsystemBenchmark::new(
            "scene",
            "Same",
            Measurement::new(10.0, "ms"),
            Measurement::new(10.5, "ms"),
        ));
    }
    assert_eq!(report.overall_verdict(), OverallVerdict::Comparable);
}

#[test]
fn one_slower_out_of_eight_mostly_comparable() {
    let mut report = ComparisonReport::new("4.6.1", "0.1.0");
    for _ in 0..7 {
        report.add(SubsystemBenchmark::new(
            "scene",
            "OK",
            Measurement::new(10.0, "ms"),
            Measurement::new(10.5, "ms"),
        ));
    }
    report.add(SubsystemBenchmark::new(
        "render",
        "Slow",
        Measurement::new(10.0, "ms"),
        Measurement::new(25.0, "ms"),
    ));
    // 1/8 = 12.5% which is <= 25% threshold
    assert_eq!(report.overall_verdict(), OverallVerdict::MostlyComparable);
}

#[test]
fn many_slower_needs_work() {
    let mut report = ComparisonReport::new("4.6.1", "0.1.0");
    for _ in 0..4 {
        report.add(SubsystemBenchmark::new(
            "render",
            "Slow",
            Measurement::new(10.0, "ms"),
            Measurement::new(25.0, "ms"),
        ));
    }
    assert_eq!(report.overall_verdict(), OverallVerdict::NeedsWork);
}

// ---------------------------------------------------------------------------
// Tolerance adjustment
// ---------------------------------------------------------------------------

#[test]
fn tight_tolerance_flags_small_differences() {
    let mut report = ComparisonReport::new("4.6.1", "0.1.0").with_tolerance(2.0);
    // 5% slower, within default 10% but outside 2%
    report.add(SubsystemBenchmark::new(
        "scene",
        "Tight",
        Measurement::new(100.0, "ms"),
        Measurement::new(105.0, "ms"),
    ));
    let (_, _, slower) = report.verdict_counts();
    assert_eq!(slower, 1);
}

#[test]
fn loose_tolerance_passes_large_differences() {
    let mut report = ComparisonReport::new("4.6.1", "0.1.0").with_tolerance(100.0);
    // 80% slower but within 100% tolerance
    report.add(SubsystemBenchmark::new(
        "scene",
        "Loose",
        Measurement::new(10.0, "ms"),
        Measurement::new(18.0, "ms"),
    ));
    let (_, comparable, _) = report.verdict_counts();
    assert_eq!(comparable, 1);
}

// ---------------------------------------------------------------------------
// Subsystem grouping
// ---------------------------------------------------------------------------

#[test]
fn subsystems_returns_sorted_unique() {
    let mut report = ComparisonReport::new("4.6.1", "0.1.0");
    report.add(SubsystemBenchmark::new("render", "A", Measurement::new(1.0, "ms"), Measurement::new(1.0, "ms")));
    report.add(SubsystemBenchmark::new("audio", "B", Measurement::new(1.0, "ms"), Measurement::new(1.0, "ms")));
    report.add(SubsystemBenchmark::new("render", "C", Measurement::new(1.0, "ms"), Measurement::new(1.0, "ms")));
    report.add(SubsystemBenchmark::new("physics", "D", Measurement::new(1.0, "ms"), Measurement::new(1.0, "ms")));
    assert_eq!(report.subsystems(), vec!["audio", "physics", "render"]);
}

#[test]
fn by_subsystem_groups_correctly() {
    let mut report = ComparisonReport::new("4.6.1", "0.1.0");
    report.add(SubsystemBenchmark::new("scene", "A", Measurement::new(1.0, "ms"), Measurement::new(1.0, "ms")));
    report.add(SubsystemBenchmark::new("scene", "B", Measurement::new(1.0, "ms"), Measurement::new(1.0, "ms")));
    report.add(SubsystemBenchmark::new("render", "C", Measurement::new(1.0, "ms"), Measurement::new(1.0, "ms")));
    let groups = report.by_subsystem();
    assert_eq!(groups["scene"].len(), 2);
    assert_eq!(groups["render"].len(), 1);
}

// ---------------------------------------------------------------------------
// Text report content
// ---------------------------------------------------------------------------

#[test]
fn text_report_has_header_and_tolerance() {
    let report = ComparisonReport::new("4.6.1", "0.1.0").with_tolerance(15.0);
    let text = report.render_text();
    assert!(text.contains("Godot 4.6.1 vs Patina 0.1.0"));
    assert!(text.contains("Tolerance: 15%"));
}

#[test]
fn text_report_shows_verdicts() {
    let mut report = ComparisonReport::new("4.6.1", "0.1.0");
    report.add(SubsystemBenchmark::new(
        "scene",
        "Fast",
        Measurement::new(20.0, "ms"),
        Measurement::new(10.0, "ms"),
    ));
    report.add(SubsystemBenchmark::new(
        "render",
        "Slow",
        Measurement::new(10.0, "ms"),
        Measurement::new(25.0, "ms"),
    ));
    let text = report.render_text();
    assert!(text.contains("PATINA FASTER"));
    assert!(text.contains("GODOT FASTER"));
}

#[test]
fn text_report_summary_counts() {
    let mut report = ComparisonReport::new("4.6.1", "0.1.0");
    report.add(SubsystemBenchmark::new("a", "1", Measurement::new(20.0, "ms"), Measurement::new(10.0, "ms")));
    report.add(SubsystemBenchmark::new("b", "2", Measurement::new(10.0, "ms"), Measurement::new(10.0, "ms")));
    report.add(SubsystemBenchmark::new("c", "3", Measurement::new(10.0, "ms"), Measurement::new(25.0, "ms")));
    let text = report.render_text();
    assert!(text.contains("Patina faster:  1"));
    assert!(text.contains("Comparable:     1"));
    assert!(text.contains("Godot faster:   1"));
}

// ---------------------------------------------------------------------------
// JSON report content
// ---------------------------------------------------------------------------

#[test]
fn json_report_has_all_fields() {
    let mut report = ComparisonReport::new("4.6.1", "0.1.0");
    report.add(SubsystemBenchmark::new(
        "scene",
        "Test bench",
        Measurement::new(10.0, "ms"),
        Measurement::new(12.0, "ms"),
    ));
    let json = report.render_json();
    assert!(json.contains("\"godot_version\""));
    assert!(json.contains("\"patina_version\""));
    assert!(json.contains("\"tolerance_pct\""));
    assert!(json.contains("\"benchmark_count\""));
    assert!(json.contains("\"summary\""));
    assert!(json.contains("\"patina_faster\""));
    assert!(json.contains("\"comparable\""));
    assert!(json.contains("\"godot_faster\""));
    assert!(json.contains("\"overall\""));
    assert!(json.contains("\"benchmarks\""));
    assert!(json.contains("\"subsystem\""));
    assert!(json.contains("\"delta_pct\""));
    assert!(json.contains("\"verdict\""));
}

#[test]
fn json_report_escapes_special_chars() {
    let mut report = ComparisonReport::new("4.6.1", "0.1.0");
    report.add(SubsystemBenchmark::new(
        "scene",
        "Test \"special\" chars",
        Measurement::new(10.0, "ms"),
        Measurement::new(10.0, "ms"),
    ));
    let json = report.render_json();
    assert!(json.contains("\\\"special\\\""));
}

// ---------------------------------------------------------------------------
// Realistic CI scenario
// ---------------------------------------------------------------------------

#[test]
fn ci_pipeline_realistic_scenario() {
    let mut report = ComparisonReport::new("4.6.1", "0.1.0").with_tolerance(15.0);

    // Scene tree
    report.add(SubsystemBenchmark::new(
        "scene_tree",
        "Instantiate 500 nodes",
        Measurement::new(6.2, "ms"),
        Measurement::new(7.0, "ms"),
    ));
    report.add(SubsystemBenchmark::new(
        "scene_tree",
        "Reparent 100 nodes",
        Measurement::new(2.1, "ms"),
        Measurement::new(1.8, "ms"),
    ));
    report.add(SubsystemBenchmark::new(
        "scene_tree",
        "Free 500 nodes",
        Measurement::new(4.5, "ms"),
        Measurement::new(3.9, "ms"),
    ));

    // Physics
    report.add(SubsystemBenchmark::new(
        "physics2d",
        "10-body sim 60 frames",
        Measurement::new(3.8, "ms"),
        Measurement::new(4.1, "ms"),
    ));

    // Resource I/O
    report.add(SubsystemBenchmark::new(
        "resource_io",
        "Parse complex .tscn",
        Measurement::new(5.0, "ms"),
        Measurement::new(3.2, "ms"),
    ));
    report.add(SubsystemBenchmark::new(
        "resource_io",
        ".tres roundtrip",
        Measurement::new(1.5, "ms"),
        Measurement::new(1.4, "ms"),
    ));

    // GDScript
    report.add(SubsystemBenchmark::new(
        "gdscript",
        "Parse 1000-line script",
        Measurement::new(8.0, "ms"),
        Measurement::new(9.5, "ms"),
    ));

    // Memory
    report.add(SubsystemBenchmark::new(
        "memory",
        "Peak RSS empty project",
        Measurement::new(45.0, "MB"),
        Measurement::new(28.0, "MB"),
    ));

    // Startup
    report.add(SubsystemBenchmark::new(
        "startup",
        "Cold start to first frame",
        Measurement::new(320.0, "ms"),
        Measurement::new(180.0, "ms"),
    ));

    assert_eq!(report.benchmark_count(), 9);

    let (faster, comparable, slower) = report.verdict_counts();
    // Most benchmarks should be comparable or better
    assert!(slower <= 2, "Too many regressions: {}", slower);
    assert!(faster >= 3, "Expected at least 3 Patina-faster: {}", faster);

    // Verify text output is well-formed
    let text = report.render_text();
    assert!(text.contains("Total benchmarks: 9"));
    assert!(text.contains("Tolerance: 15%"));
    assert!(!text.contains("No benchmarks recorded"));

    // Verify JSON output parses key fields
    let json = report.render_json();
    assert!(json.contains("\"benchmark_count\": 9"));
    assert!(json.contains("\"overall\""));

    // Overall should not be NEEDS_WORK for this data set
    let overall = report.overall_verdict();
    assert_ne!(overall, OverallVerdict::NeedsWork);
    assert_ne!(overall, OverallVerdict::NoData);
}
