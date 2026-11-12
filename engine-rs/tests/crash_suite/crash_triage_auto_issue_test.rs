//! Integration tests for crash triage auto-issue creation.
//!
//! Validates the full pipeline: crash report → issue generation → dedup tracking
//! for bead pat-nzq3r.

use gdcore::crash_triage::{
    auto_severity, classify_crash, generate_issue, AutoIssueTracker, CrashClassification,
    CrashReport, CrashTrace, Severity, StackFrame, Subsystem, TriageQueue,
};

// ---------------------------------------------------------------------------
// End-to-end: trace → report → issue
// ---------------------------------------------------------------------------

#[test]
fn trace_to_report_to_issue_pipeline() {
    let frames = vec![
        StackFrame::new(0, "core::panicking::panic"),
        StackFrame::new(1, "gdscene::node::add_child"),
    ];
    let trace = CrashTrace::new("index out of bounds: len is 0", frames);
    let report = CrashReport::from_trace(&trace, &[], &[]);
    let issue = generate_issue(&report);

    assert!(issue.title.contains("P0-Critical"));
    assert!(issue.title.contains("SceneTree"));
    assert!(issue.body.contains("index out of bounds"));
    assert!(issue.body.contains("Backtrace"));
    assert_eq!(issue.priority, 0);
    assert!(issue.labels.contains(&"crash".to_string()));
    assert!(!issue.signature.is_empty());
}

#[test]
fn regression_trace_flagged_in_issue() {
    let frames = vec![StackFrame::new(0, "gdrender2d::draw::rect")];
    let trace = CrashTrace::new("pixel mismatch in draw::rect", frames);
    let report = CrashReport::from_trace(&trace, &["pixel mismatch"], &[]);
    let issue = generate_issue(&report);

    assert!(issue.labels.contains(&"regression".to_string()));
    assert!(issue.body.contains("Regression"));
}

// ---------------------------------------------------------------------------
// Auto-severity classification
// ---------------------------------------------------------------------------

#[test]
fn scene_tree_panic_is_p0() {
    assert_eq!(
        auto_severity(Subsystem::SceneTree, true, false, false),
        Severity::P0Critical,
    );
}

#[test]
fn resource_panic_is_p0() {
    assert_eq!(
        auto_severity(Subsystem::Resources, true, false, false),
        Severity::P0Critical,
    );
}

#[test]
fn ci_blocking_is_p1() {
    assert_eq!(
        auto_severity(Subsystem::Render, false, true, false),
        Severity::P1High,
    );
}

#[test]
fn panic_without_workaround_is_p1() {
    assert_eq!(
        auto_severity(Subsystem::Audio, true, false, false),
        Severity::P1High,
    );
}

#[test]
fn workaround_available_is_p2() {
    assert_eq!(
        auto_severity(Subsystem::Editor, false, false, true),
        Severity::P2Medium,
    );
}

#[test]
fn minor_issue_is_p3() {
    assert_eq!(
        auto_severity(Subsystem::Other, false, false, false),
        Severity::P3Low,
    );
}

// ---------------------------------------------------------------------------
// Crash classification
// ---------------------------------------------------------------------------

#[test]
fn classify_matches_fixed_pattern_as_regression() {
    let c = classify_crash("assertion failed: node count == 0", &["node count"], &[]);
    assert_eq!(c, CrashClassification::Regression);
}

#[test]
fn classify_matches_open_pattern_as_known() {
    let c = classify_crash("TODO: not yet implemented", &[], &["not yet implemented"]);
    assert_eq!(c, CrashClassification::KnownIssue);
}

#[test]
fn classify_unknown_error_as_new() {
    let c = classify_crash("never seen before", &["old bug"], &["tracked bug"]);
    assert_eq!(c, CrashClassification::New);
}

#[test]
fn classify_fixed_takes_precedence_over_open() {
    let c = classify_crash(
        "shared pattern here",
        &["shared pattern"],
        &["shared pattern"],
    );
    assert_eq!(c, CrashClassification::Regression);
}

// ---------------------------------------------------------------------------
// Issue generation details
// ---------------------------------------------------------------------------

#[test]
fn issue_includes_bead_id_when_set() {
    let report = CrashReport::new(
        "linked crash",
        Severity::P2Medium,
        Subsystem::Audio,
        "buffer underrun",
        "audio_test",
    )
    .with_bead("pat-abc123");
    let issue = generate_issue(&report);
    assert!(issue.body.contains("pat-abc123"));
}

#[test]
fn issue_includes_occurrence_count() {
    let report = CrashReport::new(
        "frequent crash",
        Severity::P1High,
        Subsystem::Physics,
        "collision error",
        "physics_test",
    )
    .with_occurrences(5);
    let issue = generate_issue(&report);
    assert!(issue.body.contains("5"));
}

#[test]
fn issue_ci_blocker_acceptance_criteria() {
    let mut report = CrashReport::new(
        "ci fail",
        Severity::P0Critical,
        Subsystem::SceneTree,
        "panic in tree",
        "ci_test",
    );
    report.blocks_ci = true;
    let issue = generate_issue(&report);
    assert!(issue.body.contains("CI green after fix"));
}

#[test]
fn issue_no_ci_line_when_not_blocking() {
    let mut report = CrashReport::new(
        "minor",
        Severity::P3Low,
        Subsystem::Editor,
        "cosmetic glitch",
        "editor_test",
    );
    report.blocks_ci = false;
    let issue = generate_issue(&report);
    assert!(!issue.body.contains("CI green after fix"));
}

#[test]
fn br_command_output_is_valid() {
    let report = CrashReport::new(
        "test crash",
        Severity::P1High,
        Subsystem::Render,
        "gpu error",
        "render_test",
    );
    let issue = generate_issue(&report);
    let cmd = issue.to_br_command();
    assert!(cmd.starts_with("br create"));
    assert!(cmd.contains("--title"));
    assert!(cmd.contains("--priority 1"));
    assert!(cmd.contains("--labels"));
    assert!(cmd.contains("--description"));
}

// ---------------------------------------------------------------------------
// AutoIssueTracker: full queue processing
// ---------------------------------------------------------------------------

#[test]
fn tracker_processes_mixed_queue() {
    let mut queue = TriageQueue::new();
    queue.add(CrashReport::new(
        "crash A",
        Severity::P0Critical,
        Subsystem::SceneTree,
        "panic in add_child",
        "scene_test",
    ));
    queue.add(CrashReport::new(
        "crash B",
        Severity::P2Medium,
        Subsystem::Audio,
        "buffer underrun",
        "audio_test",
    ));
    queue.add(CrashReport::new(
        "crash A dup",
        Severity::P0Critical,
        Subsystem::SceneTree,
        "panic in add_child", // same error+trigger = same signature
        "scene_test",
    ));
    queue.add(CrashReport::new(
        "crash C",
        Severity::P1High,
        Subsystem::Render,
        "shader compile error",
        "shader_test",
    ));

    let mut tracker = AutoIssueTracker::new();
    let filed = tracker.process_queue(&queue);
    assert_eq!(filed, 3); // 3 unique, 1 duplicate
    assert_eq!(tracker.filed_count(), 3);

    // Verify priority distribution.
    assert_eq!(tracker.issues_by_priority(0).len(), 1);
    assert_eq!(tracker.issues_by_priority(1).len(), 1);
    assert_eq!(tracker.issues_by_priority(2).len(), 1);
}

#[test]
fn tracker_pre_loaded_signatures_prevent_filing() {
    let mut tracker = AutoIssueTracker::new();
    let report = CrashReport::new(
        "known crash",
        Severity::P1High,
        Subsystem::Physics,
        "collision overflow",
        "physics_test",
    );
    let sig = generate_issue(&report).signature;
    tracker.mark_filed(&sig);

    // Should not file because signature is already known.
    assert!(tracker.file_from_report(&report).is_none());
    assert_eq!(tracker.filed_count(), 0);
}

#[test]
fn tracker_summary_lists_all_issues() {
    let mut tracker = AutoIssueTracker::new();
    tracker.file_from_report(&CrashReport::new(
        "crash 1",
        Severity::P0Critical,
        Subsystem::SceneTree,
        "panic",
        "test1",
    ));
    tracker.file_from_report(&CrashReport::new(
        "crash 2",
        Severity::P2Medium,
        Subsystem::Audio,
        "underrun",
        "test2",
    ));

    let summary = tracker.render_summary();
    assert!(summary.contains("Auto-filed crash issues: 2"));
    assert!(summary.contains("[P0]"));
    assert!(summary.contains("[P2]"));
}
