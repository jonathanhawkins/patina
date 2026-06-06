//! pat-4sps4: Acceptance gate for the improved compatibility matrix.
//!
//! Acceptance: the deliverable is broken into measurable evidence with tests,
//! docs, or oracle-backed artifacts. `COMPAT_MATRIX.md` is the human-readable
//! deliverable; the rows in its compatibility table cite test files and
//! golden-file counts as the measurable evidence. This test guards that the
//! matrix keeps that structure (status definitions, test/golden columns, and
//! the cross-link to oracle pin) so a regression to a prose-only matrix fails
//! loudly.

use std::path::PathBuf;

fn matrix_body() -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("COMPAT_MATRIX.md");
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("COMPAT_MATRIX.md must exist at {}: {e}", path.display()))
}

#[test]
fn matrix_defines_measured_claimed_deferred() {
    let body = matrix_body();
    for label in ["Measured", "Claimed", "Deferred"] {
        assert!(
            body.contains(&format!("**{label}**")),
            "matrix must define status label '{label}' in its Status Definitions table"
        );
    }
}

#[test]
fn matrix_table_has_evidence_columns() {
    let body = matrix_body();
    for header in [
        "Subsystem",
        "Crate",
        "Status",
        "Tests",
        "Goldens",
        "Parity",
        "Test files",
    ] {
        assert!(
            body.contains(header),
            "matrix compatibility table must surface column '{header}'"
        );
    }
}

#[test]
fn matrix_lists_core_subsystems_with_evidence() {
    let body = matrix_body();
    // Each named subsystem must appear as a row; we don't constrain the
    // status field — the matrix is allowed to evolve — only the presence.
    for subsys in [
        "Core Runtime",
        "Variant System",
        "Object Model",
        "Resources",
        "Scene System",
        "GDScript Interop",
        "Oracle Parity",
        "2D Rendering",
        "3D Rendering",
        "2D Physics",
        "3D Physics",
    ] {
        assert!(
            body.contains(subsys),
            "matrix must list subsystem '{subsys}' as a row with evidence"
        );
    }
}

#[test]
fn matrix_cites_oracle_pin() {
    let body = matrix_body();
    assert!(
        body.contains("Godot 4.6"),
        "matrix must cite the upstream oracle pin (Godot 4.6.x) so 'Measured' rows have a fixed reference"
    );
}

#[test]
fn matrix_quantifies_test_and_golden_totals() {
    let body = matrix_body();
    assert!(
        body.contains("Total test count"),
        "matrix must publish a 'Total test count' aggregate as measurable evidence"
    );
    assert!(
        body.contains("Total golden files"),
        "matrix must publish a 'Total golden files' aggregate as measurable evidence"
    );
    assert!(
        body.contains("Oracle output files"),
        "matrix must publish an 'Oracle output files' count tied to the upstream oracle"
    );
}
