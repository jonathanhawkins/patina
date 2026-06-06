//! pat-r61i: Crash triage process for runtime regressions.
//!
//! Validates the triage process end-to-end:
//! - Realistic crash scenarios across all subsystems
//! - Severity auto-classification
//! - Regression detection against known issue signatures
//! - Triage queue prioritization and filtering
//! - Report generation with actionable output
//! - Escalation rules for P0 and regressions

use gdcore::crash_triage::*;

// ===========================================================================
// 1. Realistic crash scenarios
// ===========================================================================

#[test]
fn scene_tree_panic_is_p0_and_escalates() {
    let report = CrashReport::new(
        "SceneTree::add_child panics on orphan node",
        Severity::P0Critical,
        Subsystem::SceneTree,
        "called `Option::unwrap()` on a `None` value at scene_tree.rs:192",
        "lifecycle_trace_oracle_parity_test::enter_tree_ordering",
    );

    assert_eq!(report.severity, Severity::P0Critical);
    assert!(report.should_escalate());
    assert!(report.is_ci_blocker());
}

#[test]
fn physics_regression_escalates() {
    let report = CrashReport::new(
        "Physics determinism broken after repin",
        Severity::P1High,
        Subsystem::Physics,
        "golden trace mismatch: frame 15 position drift > 1e-3",
        "physics_trace_golden_parity_test::gravity_fall",
    )
    .classify(CrashClassification::Regression);

    assert!(report.should_escalate());
    assert!(report.is_ci_blocker());
}

#[test]
fn render_glitch_with_workaround_is_p2() {
    let report = CrashReport::new(
        "Z-ordering wrong for overlapping sprites",
        Severity::P2Medium,
        Subsystem::Render,
        "pixel mismatch at (128,64): expected rgba(255,0,0,255) got rgba(0,255,0,255)",
        "render_golden_test::z_ordering",
    );

    assert!(!report.should_escalate());
    assert!(!report.is_ci_blocker());
}

#[test]
fn edge_case_cosmetic_is_p3() {
    let report = CrashReport::new(
        "Cursor flicker on window resize",
        Severity::P3Low,
        Subsystem::Platform,
        "intermittent visual glitch during resize event",
        "manual testing",
    );

    assert!(!report.should_escalate());
    assert!(!report.is_ci_blocker());
}

// ===========================================================================
// 2. Auto-severity classification
// ===========================================================================

#[test]
fn auto_severity_classdb_panic_is_p0() {
    let sev = auto_severity(Subsystem::ClassDB, true, false, false);
    assert_eq!(sev, Severity::P0Critical);
}

#[test]
fn auto_severity_resource_panic_is_p0() {
    let sev = auto_severity(Subsystem::Resources, true, false, false);
    assert_eq!(sev, Severity::P0Critical);
}

#[test]
fn auto_severity_render_panic_no_workaround_is_p1() {
    let sev = auto_severity(Subsystem::Render, true, false, false);
    assert_eq!(sev, Severity::P1High);
}

#[test]
fn auto_severity_non_panic_ci_blocker_is_p1() {
    let sev = auto_severity(Subsystem::Physics, false, true, false);
    assert_eq!(sev, Severity::P1High);
}

#[test]
fn auto_severity_non_panic_workaround_is_p2() {
    let sev = auto_severity(Subsystem::Audio, false, false, true);
    assert_eq!(sev, Severity::P2Medium);
}

#[test]
fn auto_severity_minor_non_blocking_is_p3() {
    let sev = auto_severity(Subsystem::Editor, false, false, false);
    assert_eq!(sev, Severity::P3Low);
}

// ===========================================================================
// 3. Crash classification against known signatures
// ===========================================================================

#[test]
fn classify_against_known_fixed_issues() {
    let known_fixed = vec![
        "duplicate method registration",
        "node lifecycle double-free",
    ];
    let known_open = vec!["audio crackling on fast seek"];

    // Regression: matches a fixed issue
    let c1 = classify_crash(
        "duplicate method registration in ClassDB",
        &known_fixed,
        &known_open,
    );
    assert_eq!(c1, CrashClassification::Regression);

    // Known issue: matches open issue
    let c2 = classify_crash(
        "audio crackling on fast seek detected",
        &known_fixed,
        &known_open,
    );
    assert_eq!(c2, CrashClassification::KnownIssue);

    // New: matches nothing
    let c3 = classify_crash("completely new crash", &known_fixed, &known_open);
    assert_eq!(c3, CrashClassification::New);
}

// ===========================================================================
// 4. Full triage queue workflow
// ===========================================================================

#[test]
fn triage_queue_end_to_end() {
    let mut queue = TriageQueue::new();

    // P0: SceneTree panic
    queue.add(CrashReport::new(
        "SceneTree panic on node removal",
        Severity::P0Critical,
        Subsystem::SceneTree,
        "index out of bounds: len is 3 but index is 5",
        "scene_instancing_edge_cases_test",
    ));

    // P1: Physics regression
    queue.add(
        CrashReport::new(
            "Physics trace drift after repin",
            Severity::P1High,
            Subsystem::Physics,
            "golden mismatch frame 22",
            "physics_playground_golden_trace_test",
        )
        .classify(CrashClassification::Regression)
        .with_bead("pat-xyz1"),
    );

    // P2: Render known issue
    queue.add(
        CrashReport::new(
            "Anti-aliasing artifacts on diagonal lines",
            Severity::P2Medium,
            Subsystem::Render,
            "pixel diff > tolerance at edges",
            "render_golden_test",
        )
        .classify(CrashClassification::KnownIssue)
        .with_occurrences(5),
    );

    // P3: Minor platform quirk
    queue.add(CrashReport::new(
        "Window title not updating on scene change",
        Severity::P3Low,
        Subsystem::Platform,
        "title stays as previous scene name",
        "manual testing",
    ));

    // Verify queue state
    assert_eq!(queue.len(), 4);

    // Severity sorting
    let sorted = queue.by_severity();
    assert_eq!(sorted[0].severity, Severity::P0Critical);
    assert_eq!(sorted[1].severity, Severity::P1High);
    assert_eq!(sorted[2].severity, Severity::P2Medium);
    assert_eq!(sorted[3].severity, Severity::P3Low);

    // CI blockers (P0 + P1)
    assert_eq!(queue.ci_blockers().len(), 2);

    // Regressions
    assert_eq!(queue.regressions().len(), 1);
    assert_eq!(
        queue.regressions()[0].summary,
        "Physics trace drift after repin"
    );

    // Escalations (P0 + regressions)
    assert_eq!(queue.escalations().len(), 2);

    // Subsystem filtering
    assert_eq!(queue.by_subsystem(Subsystem::SceneTree).len(), 1);
    assert_eq!(queue.by_subsystem(Subsystem::Physics).len(), 1);
    assert_eq!(queue.by_subsystem(Subsystem::Audio).len(), 0);

    // Report generation
    let report = queue.render_report();
    assert!(report.contains("P0-Critical: 1"));
    assert!(report.contains("P1-High: 1"));
    assert!(report.contains("Total: 4"));
    assert!(report.contains("CI BLOCKERS"));
    assert!(report.contains("REGRESSIONS"));
    assert!(report.contains("CRITICAL"));

    eprintln!("\n{}", report);
}

// ===========================================================================
// 5. Report format validation
// ===========================================================================

#[test]
fn report_shows_green_for_empty_queue() {
    let q = TriageQueue::new();
    let report = q.render_report();
    assert!(report.contains("GREEN"));
    assert!(report.contains("no crashes reported"));
}

#[test]
fn report_shows_yellow_for_p2_only() {
    let mut q = TriageQueue::new();
    q.add(CrashReport::new(
        "minor issue",
        Severity::P2Medium,
        Subsystem::Render,
        "err",
        "test",
    ));
    let report = q.render_report();
    assert!(report.contains("YELLOW"));
}

#[test]
fn report_shows_critical_for_p0() {
    let mut q = TriageQueue::new();
    q.add(CrashReport::new(
        "fatal",
        Severity::P0Critical,
        Subsystem::SceneTree,
        "panic",
        "test",
    ));
    let report = q.render_report();
    assert!(report.contains("CRITICAL"));
}

#[test]
fn report_shows_attention_for_regression_without_p0() {
    let mut q = TriageQueue::new();
    q.add(
        CrashReport::new(
            "regressed",
            Severity::P2Medium,
            Subsystem::Physics,
            "drift",
            "test",
        )
        .classify(CrashClassification::Regression),
    );
    let report = q.render_report();
    assert!(report.contains("ATTENTION"));
}

// ===========================================================================
// 6. Occurrence tracking
// ===========================================================================

#[test]
fn occurrence_count_default_is_one() {
    let r = CrashReport::new("test", Severity::P3Low, Subsystem::Other, "e", "t");
    assert_eq!(r.occurrence_count, 1);
}

#[test]
fn occurrence_count_can_be_set() {
    let r =
        CrashReport::new("test", Severity::P3Low, Subsystem::Other, "e", "t").with_occurrences(42);
    assert_eq!(r.occurrence_count, 42);
}

// ===========================================================================
// 7. Subsystem coverage (all subsystems accessible)
// ===========================================================================

#[test]
fn all_subsystems_have_labels() {
    let subsystems = [
        Subsystem::SceneTree,
        Subsystem::Physics,
        Subsystem::Render,
        Subsystem::Resources,
        Subsystem::ClassDB,
        Subsystem::Scripting,
        Subsystem::Platform,
        Subsystem::Audio,
        Subsystem::Editor,
        Subsystem::Other,
    ];
    for s in &subsystems {
        assert!(!s.label().is_empty(), "{:?} should have a label", s);
    }
    assert_eq!(subsystems.len(), 10);
}
