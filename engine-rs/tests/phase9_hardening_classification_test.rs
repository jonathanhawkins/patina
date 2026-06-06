//! pat-iov9s: Validate that Phase 9 hardening artifacts are classified in
//! COMPAT_MATRIX.md and backed by concrete test/doc evidence.
//!
//! Ensures:
//! 1. COMPAT_MATRIX has a Phase 9 Hardening Artifact Classification section
//! 2. All required deliverables are present
//! 3. Classifications use approved labels
//! 4. Measured rows cite concrete test files
//! 5. Section cites Phase 9 audit as source of truth

use std::fs;

const COMPAT_MATRIX_PATH: &str =
    concat!(env!("CARGO_MANIFEST_DIR"), "/../COMPAT_MATRIX.md");
const AUDIT_PATH: &str =
    concat!(env!("CARGO_MANIFEST_DIR"), "/../prd/PHASE9_HARDENING_AUDIT.md");

fn read_compat_matrix() -> String {
    fs::read_to_string(COMPAT_MATRIX_PATH)
        .unwrap_or_else(|e| panic!("must read COMPAT_MATRIX.md: {e}"))
}

fn read_audit() -> String {
    fs::read_to_string(AUDIT_PATH)
        .unwrap_or_else(|e| panic!("must read PHASE9_HARDENING_AUDIT.md: {e}"))
}

fn hardening_section(matrix: &str) -> &str {
    let start = matrix
        .find("## Phase 9 Hardening Artifact Classification")
        .expect("COMPAT_MATRIX must have Phase 9 Hardening Artifact Classification section");
    let section = &matrix[start..];
    let end = section[1..]
        .find("\n## ")
        .or_else(|| section[1..].find("\n---\n"))
        .map(|i| i + 1)
        .unwrap_or(section.len());
    &section[..end]
}

// ===========================================================================
// 1. Section exists
// ===========================================================================

#[test]
fn compat_matrix_has_hardening_classification() {
    let matrix = read_compat_matrix();
    assert!(
        matrix.contains("## Phase 9 Hardening Artifact Classification"),
        "COMPAT_MATRIX must have Phase 9 hardening classification section"
    );
}

// ===========================================================================
// 2. Required deliverables are present
// ===========================================================================

#[test]
fn all_required_deliverables_are_classified() {
    let matrix = read_compat_matrix();
    let section = hardening_section(&matrix);

    let required = [
        "Benchmark dashboards",
        "Fuzz/property tests",
        "Crash triage process",
        "Release train",
        "Contributor onboarding",
        "Migration guide",
    ];

    for deliverable in &required {
        assert!(
            section.contains(deliverable),
            "hardening classification must include '{deliverable}'"
        );
    }
}

// ===========================================================================
// 3. Classifications use approved labels
// ===========================================================================

#[test]
fn hardening_rows_use_approved_classifications() {
    let matrix = read_compat_matrix();
    let section = hardening_section(&matrix);

    let approved = ["Measured", "Implemented, not yet measured", "Deferred", "Missing"];

    let rows: Vec<&str> = section
        .lines()
        .filter(|l| {
            l.starts_with('|')
                && !l.contains("Deliverable")
                && !l.contains("Classification")
                && !l.contains("---")
        })
        .collect();

    assert!(
        rows.len() >= 6,
        "hardening classification must have at least 6 rows, found {}",
        rows.len()
    );

    for row in &rows {
        let cols: Vec<&str> = row.split('|').map(|c| c.trim()).filter(|c| !c.is_empty()).collect();
        if cols.len() >= 2 {
            let classification = cols[1];
            assert!(
                approved.iter().any(|a| classification.contains(a)),
                "hardening row '{}' uses non-approved classification '{}'. \
                 Approved: {:?}",
                cols[0],
                classification,
                approved
            );
        }
    }
}

// ===========================================================================
// 4. Measured rows cite test files
// ===========================================================================

#[test]
fn measured_hardening_rows_cite_test_files() {
    let matrix = read_compat_matrix();
    let section = hardening_section(&matrix);

    let measured_rows: Vec<&str> = section
        .lines()
        .filter(|l| {
            l.starts_with('|')
                && l.contains("Measured")
                && !l.contains("Deliverable")
                && !l.contains("---")
        })
        .collect();

    assert!(!measured_rows.is_empty(), "must have Measured hardening rows");

    for row in &measured_rows {
        assert!(
            row.contains("_test.rs"),
            "Measured hardening row must cite a test file: {}",
            row
        );
    }
}

// ===========================================================================
// 5. Section cites Phase 9 audit
// ===========================================================================

#[test]
fn hardening_section_cites_phase9_audit() {
    let matrix = read_compat_matrix();
    let section = hardening_section(&matrix);

    assert!(
        section.contains("PHASE9_HARDENING_AUDIT.md"),
        "hardening section must cite the Phase 9 audit doc"
    );
}

// ===========================================================================
// 6. Phase 9 audit classifies all required deliverables
// ===========================================================================

#[test]
fn audit_classifies_all_deliverables() {
    let audit = read_audit();

    let families = [
        "benchmark",
        "fuzz",
        "crash triage",
        "release train",
        "onboarding",
        "migration guide",
    ];

    for family in &families {
        assert!(
            audit.to_lowercase().contains(family),
            "Phase 9 audit must classify '{family}'"
        );
    }
}

#[test]
fn audit_uses_approved_status_labels() {
    let audit = read_audit();

    // At least one of the approved statuses must appear
    let approved = ["Measured", "Implemented, not yet measured", "Deferred", "Missing"];
    let found = approved.iter().filter(|s| audit.contains(**s)).count();
    assert!(
        found >= 2,
        "Phase 9 audit must use at least 2 different approved status labels, found {}",
        found
    );
}

// ===========================================================================
// 7. Section disclaims broader claims
// ===========================================================================

#[test]
fn hardening_section_distinguishes_measured_from_docs_only() {
    let matrix = read_compat_matrix();
    let section = hardening_section(&matrix);

    assert!(
        section.contains("docs-only") || section.contains("measured test-backed"),
        "hardening section must distinguish measured artifacts from docs-only claims"
    );
}
