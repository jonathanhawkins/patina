//! pat-58pb7: Validate `docs/CONTRIBUTOR_ONBOARDING.md` keeps the required
//! section headers in place.
//!
//! Acceptance: onboarding docs cover setup, targeted test commands, and
//! oracle workflows. This test guards against accidental deletion of those
//! sections.

use std::path::PathBuf;

fn doc_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("docs")
        .join("CONTRIBUTOR_ONBOARDING.md")
}

fn doc_body() -> String {
    let p = doc_path();
    std::fs::read_to_string(&p).unwrap_or_else(|e| {
        panic!("failed to read {}: {e}", p.display());
    })
}

const REQUIRED_SECTIONS: &[&str] = &[
    "# Setup",
    "# Running Tests",
    "# Oracle Workflows",
    "# Build/Verify Loop",
];

#[test]
fn doc_file_exists_and_is_nonempty() {
    let body = doc_body();
    assert!(
        body.len() > 200,
        "CONTRIBUTOR_ONBOARDING.md should be substantive, got {} bytes",
        body.len()
    );
}

#[test]
fn doc_has_required_section_headers() {
    let body = doc_body();
    for section in REQUIRED_SECTIONS {
        // Match either H1 (# Setup) or H2 (## Setup) form so we tolerate
        // structural reorganization without requiring a churn-only update.
        let h1 = format!("\n{section}\n");
        let h2 = format!("\n#{section}\n");
        let prefixed_h1 = body.starts_with(&format!("{section}\n")) || body.contains(&h1);
        let prefixed_h2 = body.contains(&h2);
        assert!(
            prefixed_h1 || prefixed_h2,
            "CONTRIBUTOR_ONBOARDING.md is missing required section header: {section}"
        );
    }
}

#[test]
fn doc_documents_targeted_test_pattern() {
    let body = doc_body();
    assert!(
        body.contains("rust_task.sh") && body.contains("nextest"),
        "doc must document the rust_task.sh nextest pattern"
    );
    assert!(
        body.contains("--test"),
        "doc must show the `--test <name>` targeted-test flag"
    );
}

#[test]
fn doc_mentions_oracle_workflow_artifacts() {
    let body = doc_body();
    let mentions_oracle = body.contains("oracle") || body.contains("Oracle");
    let mentions_godot = body.contains("Godot") || body.contains("godot");
    let mentions_golden = body.contains("golden") || body.contains("Golden");
    assert!(
        mentions_oracle && mentions_godot && mentions_golden,
        "Oracle workflow section should reference oracle, Godot, and golden artifacts"
    );
}
