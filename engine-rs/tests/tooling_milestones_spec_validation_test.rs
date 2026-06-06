//! pat-4vy88: Validate that the Tooling Milestones Spec stays in sync with
//! the implementation and the Phase 8 audit.
//!
//! Source of truth: `prd/TOOLING_MILESTONES_SPEC.md`
//! Companion: `tooling_parity_milestone_test.rs` (runtime validation)
//!
//! This test validates the spec document itself:
//! 1. The spec exists and references the correct bead
//! 2. All 16 milestones are enumerated in the spec
//! 3. Each milestone cites test evidence
//! 4. Classifications use approved labels
//! 5. The spec disclaims blanket Godot editor parity
//! 6. The spec and Phase 8 audit agree on milestone count

use std::fs;
use std::path::PathBuf;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn read_spec() -> String {
    let path = workspace_root().join("../prd/TOOLING_MILESTONES_SPEC.md");
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("prd/TOOLING_MILESTONES_SPEC.md must exist: {e}"))
}

fn read_audit() -> String {
    let path = workspace_root().join("../prd/PHASE8_EDITOR_PARITY_AUDIT.md");
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("prd/PHASE8_EDITOR_PARITY_AUDIT.md must exist: {e}"))
}

// ===========================================================================
// 1. Spec exists and references the bead
// ===========================================================================

#[test]
fn spec_exists_and_references_bead() {
    let spec = read_spec();
    assert!(
        spec.contains("pat-4vy88"),
        "spec must reference bead pat-4vy88"
    );
    assert!(
        spec.contains("Tooling Milestones Specification"),
        "spec must have its title"
    );
}

#[test]
fn spec_references_dependency_bead() {
    let spec = read_spec();
    assert!(
        spec.contains("pat-6m9ky"),
        "spec must reference dependency bead pat-6m9ky (compatibility layer)"
    );
}

// ===========================================================================
// 2. All 16 milestones are enumerated
// ===========================================================================

#[test]
fn spec_enumerates_all_16_milestones() {
    let spec = read_spec();
    for i in 1..=16 {
        let heading = format!("### Milestone {i}:");
        assert!(
            spec.contains(&heading),
            "spec must contain heading for Milestone {i}"
        );
    }
}

#[test]
fn spec_summary_table_has_16_rows() {
    let spec = read_spec();
    let summary_start = spec
        .find("## Summary Table")
        .expect("spec must have Summary Table section");
    let summary = &spec[summary_start..];

    // Count data rows (lines starting with | and a number)
    let data_rows: Vec<&str> = summary
        .lines()
        .filter(|l| {
            l.starts_with('|')
                && !l.contains("Family")
                && !l.contains("---")
                && !l.contains("Classification")
        })
        .collect();

    assert_eq!(
        data_rows.len(),
        16,
        "summary table must have exactly 16 data rows, found {}",
        data_rows.len()
    );
}

// ===========================================================================
// 3. Each milestone cites test evidence
// ===========================================================================

#[test]
fn each_milestone_cites_exit_evidence() {
    let spec = read_spec();

    for i in 1..=16 {
        let heading = format!("### Milestone {i}:");
        let start = spec.find(&heading).unwrap_or_else(|| {
            panic!("spec must contain {heading}")
        });

        // Find the section for this milestone (up to the next ### or ## heading)
        let section_start = start;
        let remaining = &spec[section_start + heading.len()..];
        let section_end = remaining
            .find("\n### ")
            .or_else(|| remaining.find("\n## "))
            .unwrap_or(remaining.len());
        let section = &remaining[..section_end];

        assert!(
            section.contains("Exit evidence"),
            "Milestone {i} must have an 'Exit evidence' field"
        );
        assert!(
            section.contains("_test.rs") || section.contains(".rs"),
            "Milestone {i} must cite at least one .rs test/source file"
        );
    }
}

// ===========================================================================
// 4. Classifications use approved labels
// ===========================================================================

#[test]
fn milestone_classifications_use_approved_labels() {
    let spec = read_spec();
    let approved = [
        "Measured",
        "Measured (structural)",
        "Measured for tested slice",
        "Measured for local model slice",
        "Measured for bounded slice",
        "Implemented, not yet measured",
        "Deferred",
        "Missing",
    ];

    for i in 1..=16 {
        let heading = format!("### Milestone {i}:");
        let start = spec.find(&heading).unwrap();
        let remaining = &spec[start..];
        let section_end = remaining[heading.len()..]
            .find("\n### ")
            .or_else(|| remaining[heading.len()..].find("\n## "))
            .map(|e| e + heading.len())
            .unwrap_or(remaining.len());
        let section = &remaining[..section_end];

        // Find the Classification line
        let class_line = section
            .lines()
            .find(|l| l.contains("**Classification:**"))
            .unwrap_or_else(|| panic!("Milestone {i} must have a Classification field"));

        let has_approved = approved.iter().any(|a| class_line.contains(a));
        assert!(
            has_approved,
            "Milestone {i} classification must use an approved label. Found: {class_line}"
        );
    }
}

// ===========================================================================
// 5. Spec disclaims blanket Godot editor parity
// ===========================================================================

#[test]
fn spec_disclaims_blanket_parity() {
    let spec = read_spec();
    assert!(
        spec.contains("not full Godot editor parity")
            || spec.contains("not full Godot\neditor parity"),
        "spec must disclaim full Godot editor parity"
    );
}

// ===========================================================================
// 6. Spec and audit agree on milestone count
// ===========================================================================

#[test]
fn spec_and_audit_agree_on_milestone_count() {
    let spec = read_spec();
    let audit = read_audit();

    // Count milestones in spec (### Milestone N: headings)
    let spec_count = (1..=100)
        .filter(|i| spec.contains(&format!("### Milestone {i}:")))
        .count();

    // Count milestones in audit (| N | rows in the inventory table)
    let audit_start = audit
        .find("### Selected Tooling Milestone Inventory")
        .expect("audit must have milestone inventory");
    let audit_section = &audit[audit_start..];
    let audit_count = audit_section
        .lines()
        .filter(|l| {
            l.starts_with('|')
                && !l.contains("Milestone")
                && !l.contains("---")
                && !l.contains("Tooling family")
        })
        .count();

    assert_eq!(
        spec_count, audit_count,
        "spec has {spec_count} milestones but audit has {audit_count}; they must match"
    );
}

// ===========================================================================
// 7. Each milestone documents what is NOT measured
// ===========================================================================

#[test]
fn each_milestone_documents_gaps() {
    let spec = read_spec();

    for i in 1..=16 {
        let heading = format!("### Milestone {i}:");
        let start = spec.find(&heading).unwrap();
        let remaining = &spec[start..];
        let section_end = remaining[heading.len()..]
            .find("\n### ")
            .or_else(|| remaining[heading.len()..].find("\n## "))
            .map(|e| e + heading.len())
            .unwrap_or(remaining.len());
        let section = &remaining[..section_end];

        assert!(
            section.contains("What is NOT measured"),
            "Milestone {i} must document what is NOT measured"
        );
    }
}

// ===========================================================================
// 8. Spec cites the Phase 8 audit as source
// ===========================================================================

#[test]
fn spec_cites_phase8_audit() {
    let spec = read_spec();
    assert!(
        spec.contains("PHASE8_EDITOR_PARITY_AUDIT.md"),
        "spec must cite the Phase 8 audit document"
    );
}

// ===========================================================================
// 9. Spec documents the boundary rule
// ===========================================================================

#[test]
fn spec_has_boundary_rule() {
    let spec = read_spec();
    assert!(
        spec.contains("## Boundary Rule"),
        "spec must have a Boundary Rule section"
    );
}
