//! Validates `prd/3D_DEMO_PARITY_REPORT.md` (the markdown companion to the
//! JSON aggregate at `fixtures/patina_outputs/real_3d_demo_parity_report.json`)
//! is checked in, has the required sections, and contains at least one
//! PASS/FAIL row. Acceptance gate for bead pat-01qzk.

use std::path::PathBuf;

fn report_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("prd")
        .join("3D_DEMO_PARITY_REPORT.md")
}

fn read_report() -> String {
    let path = report_path();
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()))
}

#[test]
fn report_file_exists() {
    let path = report_path();
    assert!(
        path.exists(),
        "prd/3D_DEMO_PARITY_REPORT.md must exist at {}",
        path.display()
    );
}

#[test]
fn report_has_demo_section() {
    let body = read_report();
    assert!(
        body.contains("## Demo"),
        "report must contain a `## Demo` section"
    );
}

#[test]
fn report_has_oracle_section() {
    let body = read_report();
    assert!(
        body.contains("## Oracle"),
        "report must contain a `## Oracle` section"
    );
}

#[test]
fn report_has_results_section() {
    let body = read_report();
    assert!(
        body.contains("## Results"),
        "report must contain a `## Results` section"
    );
}

#[test]
fn report_has_pass_or_fail_row() {
    let body = read_report();
    let has_pass = body.contains("PASS");
    let has_fail = body.contains("FAIL");
    assert!(
        has_pass || has_fail,
        "report must contain at least one PASS/FAIL row"
    );
}

#[test]
fn report_references_a_concrete_fixture() {
    let body = read_report();
    assert!(
        body.contains("fixtures/scenes/") || body.contains("fixtures/golden/"),
        "report must reference a concrete fixture path under fixtures/"
    );
}

#[test]
fn report_cites_aggregate_oracle_artifact() {
    let body = read_report();
    assert!(
        body.contains("real_3d_demo_parity_report.json"),
        "report must cite the aggregate oracle artifact \
         fixtures/patina_outputs/real_3d_demo_parity_report.json"
    );
}
