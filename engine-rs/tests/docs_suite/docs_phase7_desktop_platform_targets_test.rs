//! pat-3abjw: Acceptance gate for the supported desktop platform target
//! matrix and per-target validation coverage.
//!
//! Acceptance: supported desktop targets are explicitly documented,
//! validation coverage is listed per target, and a test or doc-validation
//! check guards the target matrix. The deliverable lives in
//! `gdplatform::platform_targets::DESKTOP_TARGETS`, the documented matrix
//! in `docs/migration-guide.md` (per-target table) plus
//! `prd/PHASE7_PLATFORM_PARITY_AUDIT.md`, with a sibling validation test.

use std::path::PathBuf;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

#[test]
fn platform_targets_module_publishes_canonical_list() {
    let p = repo_root().join("engine-rs/crates/gdplatform/src/platform_targets.rs");
    let body = std::fs::read_to_string(&p)
        .unwrap_or_else(|e| panic!("platform_targets.rs must exist at {}: {e}", p.display()));
    assert!(
        body.contains("DESKTOP_TARGETS"),
        "platform_targets.rs must publish the canonical DESKTOP_TARGETS list"
    );
    assert!(
        body.contains("ci_tested_targets"),
        "platform_targets.rs must publish the per-target CI-tested coverage helper"
    );
}

#[test]
fn migration_guide_lists_target_matrix() {
    let p = repo_root().join("docs/migration-guide.md");
    let body = std::fs::read_to_string(&p)
        .unwrap_or_else(|e| panic!("migration-guide.md must exist at {}: {e}", p.display()));
    for triple in [
        "x86_64-unknown-linux-gnu",
        "x86_64-apple-darwin",
        "aarch64-apple-darwin",
        "x86_64-pc-windows-msvc",
    ] {
        assert!(
            body.contains(triple),
            "migration guide must list desktop target triple '{triple}'"
        );
    }
    assert!(
        body.contains("CI Tested"),
        "migration guide target table must list per-target validation (CI Tested column)"
    );
}

#[test]
fn validation_test_anchors_present() {
    for f in [
        "engine-rs/tests/platform_targets_validation_test.rs",
        "engine-rs/tests/phase7_desktop_targets_doc_validation_test.rs",
    ] {
        let p = repo_root().join(f);
        assert!(
            p.exists(),
            "platform-target validation test must exist: {}",
            p.display()
        );
    }
}

#[test]
fn audit_doc_documents_phase7_target_scope() {
    let p = repo_root().join("prd/PHASE7_PLATFORM_PARITY_AUDIT.md");
    let body = std::fs::read_to_string(&p)
        .unwrap_or_else(|e| panic!("Phase-7 audit must exist at {}: {e}", p.display()));
    assert!(
        body.to_lowercase().contains("desktop"),
        "Phase-7 audit must scope desktop-target coverage"
    );
}
