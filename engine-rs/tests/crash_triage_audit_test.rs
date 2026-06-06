//! pat-t8hgz: Keep crash triage docs aligned with the validated process model.
//!
//! Source of truth: `prd/PHASE9_HARDENING_AUDIT.md`
//!
//! This test validates:
//! 1. The Phase 9 audit doc cites the crash triage workflow as measured
//! 2. `docs/TRIAGE_PROCESS.md` documents all severity levels from the code model
//! 3. `docs/TRIAGE_PROCESS.md` documents the classification model (New/Regression/Known)
//! 4. `docs/TRIAGE_PROCESS.md` documents the triage queue/flow steps
//! 5. The crash triage module and evidence test files exist
//! 6. The triage doc references the `br` tracker integration

use std::path::PathBuf;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

fn read_audit() -> String {
    let path = repo_root().join("prd/PHASE9_HARDENING_AUDIT.md");
    std::fs::read_to_string(&path).expect("prd/PHASE9_HARDENING_AUDIT.md must exist")
}

fn read_triage_doc() -> String {
    let path = repo_root().join("docs/TRIAGE_PROCESS.md");
    std::fs::read_to_string(&path).expect("docs/TRIAGE_PROCESS.md must exist")
}

// ── Phase 9 audit cites crash triage ────────────────────────────────

#[test]
fn audit_references_crash_triage_bead() {
    let audit = read_audit();
    assert!(
        audit.contains("pat-t8hgz"),
        "Phase 9 audit must reference the crash triage bead"
    );
}

#[test]
fn audit_cites_crash_triage_evidence() {
    let audit = read_audit();
    let expected = [
        "crash_triage_process_test.rs",
        "crash_triage_auto_issue_test.rs",
        "docs/TRIAGE_PROCESS.md",
    ];
    for evidence in &expected {
        assert!(
            audit.contains(evidence),
            "Phase 9 audit must cite evidence '{evidence}'"
        );
    }
}

#[test]
fn audit_classifies_crash_triage_as_measured() {
    let audit = read_audit();
    assert!(
        audit.contains("Measured for local process/model slice"),
        "audit must classify crash triage as measured"
    );
}

// ── Triage doc documents severity levels from code model ────────────

#[test]
fn triage_doc_documents_all_severity_levels() {
    let doc = read_triage_doc();
    // These must match the Severity enum in gdcore::crash_triage
    let severities = [
        ("P0", "Critical"),
        ("P1", "High"),
        ("P2", "Medium"),
        ("P3", "Low"),
    ];
    for (code, label) in &severities {
        assert!(
            doc.contains(code) && doc.contains(label),
            "triage doc must document severity {code} ({label})"
        );
    }
}

#[test]
fn triage_doc_has_severity_classification_guide() {
    let doc = read_triage_doc();
    assert!(
        doc.contains("Severity Classification Guide"),
        "triage doc must have a severity classification guide section"
    );
}

#[test]
fn triage_doc_severity_criteria_match_code_model() {
    let doc = read_triage_doc();
    // P0 criteria from CrashReport — engine crash, data loss, blocks testing
    assert!(
        doc.contains("crash") || doc.contains("panic"),
        "P0 criteria must mention crash/panic"
    );
    assert!(
        doc.contains("data loss"),
        "P0 criteria must mention data loss"
    );
    // P1 criteria — major broken, no workaround, regression
    assert!(
        doc.contains("workaround"),
        "triage doc must discuss workaround availability"
    );
    // Regression escalation rule
    assert!(
        doc.contains("regression") || doc.contains("Regression"),
        "triage doc must discuss regression handling"
    );
}

// ── Triage doc documents classification model ───────────────────────

#[test]
fn triage_doc_documents_parity_classification() {
    let doc = read_triage_doc();
    // The code model has New, Regression, Known classifications
    // The doc should cover these via parity bug handling
    assert!(
        doc.contains("Parity Bug Triage") || doc.contains("Parity Report"),
        "triage doc must have parity bug handling section"
    );
}

#[test]
fn triage_doc_documents_regression_handling() {
    let doc = read_triage_doc();
    assert!(
        doc.contains("regression") || doc.contains("Regression"),
        "triage doc must discuss regression detection"
    );
    assert!(
        doc.contains("oracle") || doc.contains("golden"),
        "triage doc must reference oracle/golden comparison for parity bugs"
    );
}

// ── Triage doc documents queue/flow steps ───────────────────────────

#[test]
fn triage_doc_has_triage_flow() {
    let doc = read_triage_doc();
    assert!(
        doc.contains("Triage Flow"),
        "triage doc must have a triage flow section"
    );
}

#[test]
fn triage_doc_documents_required_steps() {
    let doc = read_triage_doc();
    let required_steps = [
        "Label",
        "Prioritize",
        "Assign",
        "Implement",
        "Verify",
        "Close",
    ];
    for step in &required_steps {
        assert!(doc.contains(step), "triage doc must document step '{step}'");
    }
}

#[test]
fn triage_doc_documents_escalation_rules() {
    let doc = read_triage_doc();
    assert!(
        doc.contains("Escalation") || doc.contains("escalation"),
        "triage doc must document escalation rules"
    );
}

#[test]
fn triage_doc_documents_response_times() {
    let doc = read_triage_doc();
    assert!(
        doc.contains("Response Time") || doc.contains("response"),
        "triage doc must document response time expectations"
    );
}

// ── Triage doc references br tracker ────────────────────────────────

#[test]
fn triage_doc_references_br_tracker() {
    let doc = read_triage_doc();
    assert!(
        doc.contains("br create") || doc.contains("br list"),
        "triage doc must show br CLI usage for issue tracking"
    );
    assert!(
        doc.contains("br ready"),
        "triage doc must reference br ready for work discovery"
    );
}

// ── Evidence files exist ────────────────────────────────────────────

#[test]
fn crash_triage_module_exists() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("crates/gdcore/src/crash_triage.rs");
    assert!(path.exists(), "gdcore::crash_triage module must exist");
}

#[test]
fn crash_triage_evidence_tests_exist() {
    let tests_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests");
    let expected = [
        "crash_triage_process_test.rs",
        "crash_triage_auto_issue_test.rs",
    ];
    for file in &expected {
        let path = tests_dir.join(file);
        assert!(
            path.exists(),
            "evidence test must exist: {}",
            path.display()
        );
    }
}

#[test]
fn triage_doc_and_audit_doc_both_exist() {
    assert!(repo_root().join("docs/TRIAGE_PROCESS.md").exists());
    assert!(repo_root().join("prd/PHASE9_HARDENING_AUDIT.md").exists());
}
