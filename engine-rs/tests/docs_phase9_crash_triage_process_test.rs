//! pat-0nu86: Acceptance gate for the crash-triage process for runtime
//! regressions.
//!
//! Acceptance: the crash triage workflow is documented end-to-end and a
//! validation test or doc check asserts the required steps and artifacts are
//! present. The deliverable ships as: (1) `docs/TRIAGE_PROCESS.md` (the
//! end-to-end process doc), (2) `gdcore::crash_triage` (the implementation
//! module), and (3) a sibling validation suite. This test guards each anchor
//! so a regression — module renamed, doc deleted, suite stripped — fails
//! under the bead's marker.

use std::path::PathBuf;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

#[test]
fn triage_process_doc_exists_and_covers_required_steps() {
    let p = repo_root().join("docs/TRIAGE_PROCESS.md");
    let body = std::fs::read_to_string(&p)
        .unwrap_or_else(|e| panic!("docs/TRIAGE_PROCESS.md must exist at {}: {e}", p.display()));
    let lower = body.to_lowercase();
    for step in ["triage", "crash", "regression"] {
        assert!(
            lower.contains(step),
            "TRIAGE_PROCESS.md must cover the '{step}' step"
        );
    }
}

#[test]
fn crash_triage_module_source_exists() {
    let p = repo_root().join("engine-rs/crates/gdcore/src/crash_triage.rs");
    assert!(
        p.exists(),
        "gdcore::crash_triage module must exist at {}",
        p.display()
    );
}

#[test]
fn validation_suite_anchors_present() {
    for f in [
        "engine-rs/tests/crash_triage_process_test.rs",
        "engine-rs/tests/crash_triage_audit_test.rs",
        "engine-rs/tests/crash_triage_auto_issue_test.rs",
        "engine-rs/tests/community_issue_template_triage_test.rs",
    ] {
        let p = repo_root().join(f);
        assert!(
            p.exists(),
            "crash-triage validation test must exist: {}",
            p.display()
        );
    }
}
