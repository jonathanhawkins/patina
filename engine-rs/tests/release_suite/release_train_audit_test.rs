//! pat-d59t7: Keep release-train workflow aligned with committed automation and gates.
//!
//! Source of truth: `prd/PHASE9_HARDENING_AUDIT.md`
//!
//! This test validates:
//! 1. The Phase 9 audit doc cites the release-train workflow as partly measured
//! 2. `docs/RELEASE_PROCESS.md` distinguishes committed automation from manual steps
//! 3. The release-train workflow test file exists as evidence
//! 4. The audit doc cites the correct evidence artifacts

use std::path::PathBuf;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

fn read_audit() -> String {
    let path = repo_root().join("prd/PHASE9_HARDENING_AUDIT.md");
    std::fs::read_to_string(&path).expect("prd/PHASE9_HARDENING_AUDIT.md must exist")
}

fn read_release_doc() -> String {
    let path = repo_root().join("docs/RELEASE_PROCESS.md");
    std::fs::read_to_string(&path).expect("docs/RELEASE_PROCESS.md must exist")
}

// ── Phase 9 audit cites release train ──────────────────────────────

#[test]
fn audit_references_release_train_bead() {
    let audit = read_audit();
    assert!(
        audit.contains("pat-d59t7"),
        "Phase 9 audit must reference the release-train bead"
    );
}

#[test]
fn audit_cites_release_train_evidence() {
    let audit = read_audit();
    let expected = ["release_train_workflow_test.rs", "docs/RELEASE_PROCESS.md"];
    for evidence in &expected {
        assert!(
            audit.contains(evidence),
            "Phase 9 audit must cite evidence '{evidence}'"
        );
    }
}

#[test]
fn audit_classifies_release_train_status() {
    let audit = read_audit();
    assert!(
        audit.contains("Implemented, partly measured"),
        "audit must classify release train as implemented, partly measured"
    );
}

#[test]
fn audit_identifies_docs_overclaim_gap() {
    let audit = read_audit();
    assert!(
        audit.contains("docs-overclaim"),
        "audit must identify the docs-overclaim gap for release train"
    );
}

// ── Release doc distinguishes committed from manual ────────────────

#[test]
fn release_doc_has_release_checklist() {
    let doc = read_release_doc();
    assert!(
        doc.contains("Release Checklist"),
        "release doc must have a release checklist section"
    );
}

#[test]
fn release_doc_distinguishes_automated_from_manual() {
    let doc = read_release_doc();
    // The doc must acknowledge that release.yml may not be present
    assert!(
        doc.contains("not present") || doc.contains("not yet enabled") || doc.contains("manually"),
        "release doc must distinguish committed automation from manual steps"
    );
}

#[test]
fn release_doc_documents_ci_pipeline() {
    let doc = read_release_doc();
    assert!(
        doc.contains("ci.yml"),
        "release doc must reference the CI pipeline"
    );
}

#[test]
fn release_doc_documents_versioning() {
    let doc = read_release_doc();
    assert!(
        doc.contains("Semantic Versioning") || doc.contains("semver"),
        "release doc must document versioning strategy"
    );
}

#[test]
fn release_doc_documents_pre_release_validation() {
    let doc = read_release_doc();
    assert!(
        doc.contains("cargo test") && doc.contains("cargo clippy"),
        "release doc must document pre-release validation commands"
    );
}

#[test]
fn release_doc_documents_oracle_parity_gate() {
    let doc = read_release_doc();
    assert!(
        doc.contains("oracle parity") || doc.contains("Oracle Parity"),
        "release doc must reference oracle parity as a release gate"
    );
}

// ── Evidence files exist ───────────────────────────────────────────

#[test]
fn release_train_workflow_test_exists() {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/release_train_workflow_test.rs");
    assert!(path.exists(), "release_train_workflow_test.rs must exist");
}

#[test]
fn release_process_doc_exists() {
    assert!(repo_root().join("docs/RELEASE_PROCESS.md").exists());
}

#[test]
fn audit_doc_exists() {
    assert!(repo_root().join("prd/PHASE9_HARDENING_AUDIT.md").exists());
}

#[test]
fn ci_workflow_exists() {
    assert!(repo_root().join(".github/workflows/ci.yml").exists());
}
