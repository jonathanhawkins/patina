//! pat-0mhg8: Acceptance gate for the CI matrix that covers supported
//! desktop targets.
//!
//! Acceptance: the deliverable is broken into measurable evidence with
//! tests, docs, or oracle-backed artifacts. The CI matrix is published in
//! `.github/workflows/ci.yml`; the source-of-truth target list lives in
//! `gdplatform::platform_targets::DESKTOP_TARGETS`. This test asserts that
//! both anchors stay in place and the CI matrix actually names the three
//! desktop OSes.

use std::path::PathBuf;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

fn read_ci_yaml() -> String {
    let p = repo_root().join(".github/workflows/ci.yml");
    std::fs::read_to_string(&p)
        .unwrap_or_else(|e| panic!(".github/workflows/ci.yml must exist at {}: {e}", p.display()))
}

#[test]
fn ci_yaml_lists_three_desktop_os_runners() {
    let body = read_ci_yaml();
    for os in ["ubuntu-latest", "macos-latest", "windows-latest"] {
        assert!(
            body.contains(os),
            "CI workflow must include runner '{os}' for the supported-target matrix"
        );
    }
}

#[test]
fn platform_targets_module_source_exists() {
    let p = repo_root().join("engine-rs/crates/gdplatform/src/platform_targets.rs");
    let body = std::fs::read_to_string(&p)
        .unwrap_or_else(|e| panic!("platform_targets.rs must exist at {}: {e}", p.display()));
    assert!(
        body.contains("DESKTOP_TARGETS"),
        "platform_targets.rs must publish the canonical DESKTOP_TARGETS list"
    );
}

#[test]
fn phase7_audit_documents_ci_matrix() {
    let p = repo_root().join("prd/PHASE7_PLATFORM_PARITY_AUDIT.md");
    let body = std::fs::read_to_string(&p)
        .unwrap_or_else(|e| panic!("Phase-7 audit must exist at {}: {e}", p.display()));
    let lower = body.to_lowercase();
    assert!(
        lower.contains("ci matrix") || lower.contains("ci build matrix") || lower.contains("desktop_targets"),
        "Phase-7 audit must document the CI build matrix or DESKTOP_TARGETS"
    );
}

#[test]
fn sibling_ci_matrix_test_anchors_exist() {
    for f in [
        "engine-rs/tests/ci_build_matrix_platform_test.rs",
        "engine-rs/tests/phase7_ci_matrix_target_guard_test.rs",
    ] {
        let p = repo_root().join(f);
        assert!(
            p.exists(),
            "CI matrix evidence test must exist: {}",
            p.display()
        );
    }
}
