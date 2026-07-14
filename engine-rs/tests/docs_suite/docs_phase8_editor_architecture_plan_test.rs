//! pat-12a4e: Acceptance gate for the editor architecture plan.
//!
//! Acceptance: the plan names concrete subsystems, boundaries, and deferred
//! scope, and a validation test or doc check cites the document as the
//! source of truth. The plan lives at `docs/EDITOR_ARCHITECTURE.md`. The
//! sibling `editor_architecture_plan_test.rs` (claimed by pat-z20hm) drives
//! module-inventory and REST-endpoint conformance against the plan; this
//! test guards the plan's *content shape* so a regression to a stub doc
//! fails under the bead's marker.

use std::path::PathBuf;

fn plan_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("docs")
        .join("EDITOR_ARCHITECTURE.md")
}

fn plan_body() -> String {
    let p = plan_path();
    std::fs::read_to_string(&p).unwrap_or_else(|e| {
        panic!("EDITOR_ARCHITECTURE.md must exist at {}: {e}", p.display())
    })
}

#[test]
fn plan_file_exists_and_is_substantive() {
    let body = plan_body();
    assert!(
        body.len() > 500,
        "EDITOR_ARCHITECTURE.md should be substantive, got {} bytes",
        body.len()
    );
}

#[test]
fn plan_names_concrete_subsystems() {
    let body = plan_body();
    let lower = body.to_lowercase();
    for needle in [
        "module inventory",
        "editor_server",
        "editor_interface",
        "editor_compat",
    ] {
        assert!(
            lower.contains(needle),
            "plan must name concrete subsystem '{needle}'"
        );
    }
}

#[test]
fn plan_calls_out_boundaries() {
    let body = plan_body();
    let lower = body.to_lowercase();
    assert!(
        lower.contains("dependency") || lower.contains("boundary") || lower.contains("layer"),
        "plan must describe dependency boundaries / layering"
    );
}

#[test]
fn plan_calls_out_deferred_scope() {
    let body = plan_body();
    let lower = body.to_lowercase();
    assert!(
        lower.contains("deferred")
            || lower.contains("not yet")
            || lower.contains("out of scope")
            || lower.contains("future"),
        "plan must call out deferred / not-yet-implemented scope"
    );
}

#[test]
fn validation_test_anchor_cites_plan_as_source_of_truth() {
    // The sibling pat-z20hm test reads EDITOR_ARCHITECTURE.md by name; assert
    // it survives so the doc-validation chain stays intact.
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("editor_architecture_plan_test.rs");
    let body = std::fs::read_to_string(&p)
        .unwrap_or_else(|e| panic!("editor_architecture_plan_test.rs must exist at {}: {e}", p.display()));
    assert!(
        body.contains("EDITOR_ARCHITECTURE.md"),
        "validation test must cite EDITOR_ARCHITECTURE.md as the source of truth"
    );
}
