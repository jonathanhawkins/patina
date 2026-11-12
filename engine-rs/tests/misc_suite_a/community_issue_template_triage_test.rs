//! Validation tests for community issue templates and triage process (pat-5n5si).
//!
//! Ensures issue templates exist with required fields and the triage
//! process document covers all expected sections.

use std::fs;
use std::path::Path;

fn repo_root() -> &'static Path {
    let manifest = env!("CARGO_MANIFEST_DIR");
    Path::new(manifest).parent().unwrap()
}

fn read_file(rel_path: &str) -> String {
    let path = repo_root().join(rel_path);
    fs::read_to_string(&path).unwrap_or_else(|_| panic!("{rel_path} must exist at {path:?}"))
}

// ===========================================================================
// 1. Issue template files exist
// ===========================================================================

#[test]
fn bug_report_template_exists() {
    assert!(
        repo_root()
            .join(".github/ISSUE_TEMPLATE/bug_report.yml")
            .exists(),
        "bug report template must exist"
    );
}

#[test]
fn feature_request_template_exists() {
    assert!(
        repo_root()
            .join(".github/ISSUE_TEMPLATE/feature_request.yml")
            .exists(),
        "feature request template must exist"
    );
}

#[test]
fn parity_report_template_exists() {
    assert!(
        repo_root()
            .join(".github/ISSUE_TEMPLATE/parity_report.yml")
            .exists(),
        "parity report template must exist"
    );
}

#[test]
fn template_config_exists() {
    assert!(
        repo_root()
            .join(".github/ISSUE_TEMPLATE/config.yml")
            .exists(),
        "template config.yml must exist"
    );
}

// ===========================================================================
// 2. Bug report template content
// ===========================================================================

#[test]
fn bug_report_has_required_fields() {
    let tmpl = read_file(".github/ISSUE_TEMPLATE/bug_report.yml");

    let fields = [
        "Subsystem",
        "Severity",
        "Description",
        "Steps to Reproduce",
        "Expected Behavior",
        "Actual Behavior",
        "Patina Version",
        "Platform",
    ];

    for field in &fields {
        assert!(
            tmpl.contains(field),
            "bug report template must have field: {field}"
        );
    }
}

#[test]
fn bug_report_lists_engine_subsystems() {
    let tmpl = read_file(".github/ISSUE_TEMPLATE/bug_report.yml");

    let subsystems = [
        "gdcore",
        "gdvariant",
        "gdobject",
        "gdresource",
        "gdscene",
        "gdphysics2d",
        "gdphysics3d",
        "gdaudio",
        "gdplatform",
        "gdscript-interop",
        "gdeditor",
    ];

    for sub in &subsystems {
        assert!(tmpl.contains(sub), "bug report must list subsystem: {sub}");
    }
}

#[test]
fn bug_report_has_severity_levels() {
    let tmpl = read_file(".github/ISSUE_TEMPLATE/bug_report.yml");

    let severities = ["P0 Critical", "P1 High", "P2 Medium", "P3 Low"];
    for sev in &severities {
        assert!(
            tmpl.contains(sev),
            "bug report must have severity level: {sev}"
        );
    }
}

#[test]
fn bug_report_has_godot_parity_question() {
    let tmpl = read_file(".github/ISSUE_TEMPLATE/bug_report.yml");
    assert!(
        tmpl.contains("Godot Parity"),
        "bug report must ask about Godot parity"
    );
}

// ===========================================================================
// 3. Feature request template content
// ===========================================================================

#[test]
fn feature_request_has_required_fields() {
    let tmpl = read_file(".github/ISSUE_TEMPLATE/feature_request.yml");

    let fields = [
        "Subsystem",
        "Problem Statement",
        "Proposed Solution",
        "Alternatives Considered",
    ];

    for field in &fields {
        assert!(
            tmpl.contains(field),
            "feature request template must have field: {field}"
        );
    }
}

// ===========================================================================
// 4. Parity report template content
// ===========================================================================

#[test]
fn parity_report_has_required_fields() {
    let tmpl = read_file(".github/ISSUE_TEMPLATE/parity_report.yml");

    let fields = [
        "Godot 4.6 Behavior",
        "Patina Behavior",
        "Test Case",
        "Godot Version Tested",
        "Patina Version",
    ];

    for field in &fields {
        assert!(
            tmpl.contains(field),
            "parity report template must have field: {field}"
        );
    }
}

// ===========================================================================
// 5. Triage process document
// ===========================================================================

#[test]
fn triage_process_doc_exists() {
    assert!(
        repo_root().join("docs/TRIAGE_PROCESS.md").exists(),
        "docs/TRIAGE_PROCESS.md must exist"
    );
}

#[test]
fn triage_process_has_required_sections() {
    let doc = read_file("docs/TRIAGE_PROCESS.md");

    let sections = [
        "## Triage Flow",
        "## Issue Templates",
        "## Severity Classification Guide",
        "## Triage Labels Quick Reference",
        "## Parity Bug Triage",
        "## Response Time Expectations",
    ];

    for section in &sections {
        assert!(
            doc.contains(section),
            "triage process must have section: {section}"
        );
    }
}

#[test]
fn triage_process_covers_all_priorities() {
    let doc = read_file("docs/TRIAGE_PROCESS.md");

    let priorities = [
        "### P0 Critical",
        "### P1 High",
        "### P2 Medium",
        "### P3 Low",
    ];
    for p in &priorities {
        assert!(
            doc.contains(p),
            "triage process must document priority: {p}"
        );
    }
}

#[test]
fn triage_process_references_br_cli() {
    let doc = read_file("docs/TRIAGE_PROCESS.md");
    assert!(
        doc.contains("br create") && doc.contains("br list") && doc.contains("br ready"),
        "triage process must reference br CLI commands"
    );
}

#[test]
fn triage_process_documents_labels() {
    let doc = read_file("docs/TRIAGE_PROCESS.md");

    let labels = [
        "needs-triage",
        "triaged",
        "blocked",
        "wontfix",
        "duplicate",
        "bug",
        "enhancement",
        "parity",
    ];

    for label in &labels {
        assert!(
            doc.contains(label),
            "triage process must document label: {label}"
        );
    }
}

#[test]
fn triage_process_covers_parity_workflow() {
    let doc = read_file("docs/TRIAGE_PROCESS.md");
    assert!(
        doc.contains("oracle") && doc.contains("golden"),
        "parity triage must mention oracle outputs and golden data"
    );
}
