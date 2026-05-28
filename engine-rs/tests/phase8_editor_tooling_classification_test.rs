//! pat-de2bl: Validate that editor tooling milestones are classified against
//! concrete tested slices in COMPAT_MATRIX.md.
//!
//! Ensures:
//! 1. COMPAT_MATRIX has a Selected Editor Tooling Classification section
//! 2. Each required tooling family is present and classified
//! 3. Classifications use approved labels
//! 4. Measured rows cite concrete test files
//! 5. No row implies blanket Godot editor parity

use std::fs;

const COMPAT_MATRIX_PATH: &str =
    concat!(env!("CARGO_MANIFEST_DIR"), "/../COMPAT_MATRIX.md");

fn read_compat_matrix() -> String {
    fs::read_to_string(COMPAT_MATRIX_PATH)
        .unwrap_or_else(|e| panic!("must read COMPAT_MATRIX.md: {e}"))
}

fn tooling_section(matrix: &str) -> &str {
    let start = matrix
        .find("### Selected Editor Tooling Classification")
        .expect("COMPAT_MATRIX must have Selected Editor Tooling Classification section");
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
fn compat_matrix_has_editor_tooling_classification() {
    let matrix = read_compat_matrix();
    assert!(
        matrix.contains("### Selected Editor Tooling Classification"),
        "COMPAT_MATRIX must have a Selected Editor Tooling Classification section"
    );
}

// ===========================================================================
// 2. Required tooling families are present
// ===========================================================================

#[test]
fn all_required_tooling_families_are_classified() {
    let matrix = read_compat_matrix();
    let section = tooling_section(&matrix);

    let required_families = [
        "Script editor",
        "Inspector",
        "Animation editor",
        "Theme editor",
        "Tilemap tooling",
        "Editor systems",
    ];

    for family in &required_families {
        assert!(
            section.contains(family),
            "tooling classification must include '{family}'"
        );
    }
}

// ===========================================================================
// 3. Classifications use approved labels
// ===========================================================================

#[test]
fn tooling_rows_use_approved_classifications() {
    let matrix = read_compat_matrix();
    let section = tooling_section(&matrix);

    let approved = ["Measured", "Implemented, not yet measured", "Deferred", "Missing"];

    let rows: Vec<&str> = section
        .lines()
        .filter(|l| {
            l.starts_with('|')
                && !l.contains("Tooling Family")
                && !l.contains("Classification")
                && !l.contains("---")
        })
        .collect();

    assert!(
        rows.len() >= 6,
        "tooling classification must have at least 6 rows, found {}",
        rows.len()
    );

    for row in &rows {
        let cols: Vec<&str> = row.split('|').map(|c| c.trim()).filter(|c| !c.is_empty()).collect();
        if cols.len() >= 2 {
            let classification = cols[1];
            assert!(
                approved.iter().any(|a| classification.contains(a)),
                "tooling row '{}' uses non-approved classification '{}'. \
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
fn measured_tooling_rows_cite_test_files() {
    let matrix = read_compat_matrix();
    let section = tooling_section(&matrix);

    let measured_rows: Vec<&str> = section
        .lines()
        .filter(|l| {
            l.starts_with('|')
                && l.contains("Measured")
                && !l.contains("Tooling Family")
                && !l.contains("---")
        })
        .collect();

    assert!(!measured_rows.is_empty(), "must have Measured tooling rows");

    for row in &measured_rows {
        assert!(
            row.contains("_test.rs") || row.contains("_test`"),
            "Measured tooling row must cite a test file: {}",
            row
        );
    }
}

// ===========================================================================
// 5. Section disclaims blanket parity
// ===========================================================================

#[test]
fn tooling_section_disclaims_blanket_parity() {
    let matrix = read_compat_matrix();
    let section = tooling_section(&matrix);

    assert!(
        section.contains("do not imply blanket Godot editor parity")
            || section.contains("not blanket Godot editor parity"),
        "tooling section must disclaim blanket Godot editor parity"
    );
}

// ===========================================================================
// 6. Section cites Phase 8 audit as source of truth
// ===========================================================================

#[test]
fn tooling_section_cites_phase8_audit() {
    let matrix = read_compat_matrix();
    let section = tooling_section(&matrix);

    assert!(
        section.contains("PHASE8_EDITOR_PARITY_AUDIT.md"),
        "tooling section must cite the Phase 8 audit doc"
    );
}
