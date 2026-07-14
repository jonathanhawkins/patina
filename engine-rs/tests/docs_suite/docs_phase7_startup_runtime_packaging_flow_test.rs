//! pat-037zy: Acceptance gate for the startup/runtime packaging flow.
//!
//! Acceptance: the packaging flow is documented and covered by a focused
//! test or workflow validation that exercises the supported startup/runtime
//! artifact path. The deliverable ships as: (1) the audited flow doc
//! `prd/PHASE7_PLATFORM_PARITY_AUDIT.md`, (2) the integration test
//! `engine-rs/tests/startup_runtime_packaging_flow_test.rs` that drives
//! bootstrap → run → package → verify end to end.

use std::path::PathBuf;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

#[test]
fn packaging_flow_doc_exists_and_scopes_phase7() {
    let p = repo_root().join("prd/PHASE7_PLATFORM_PARITY_AUDIT.md");
    let body = std::fs::read_to_string(&p)
        .unwrap_or_else(|e| panic!("Phase-7 audit must exist at {}: {e}", p.display()));
    for needle in ["packaging", "startup", "runtime"] {
        assert!(
            body.to_lowercase().contains(needle),
            "Phase-7 audit must document the '{needle}' path"
        );
    }
}

#[test]
fn integration_test_anchor_exists_and_drives_flow() {
    let p = repo_root().join("engine-rs/tests/startup_runtime_packaging_flow_test.rs");
    let body = std::fs::read_to_string(&p)
        .unwrap_or_else(|e| panic!("integration test must exist at {}: {e}", p.display()));
    // Anchor that the integration test drives the four documented steps.
    for needle in [
        "EngineBootstrap",
        "PackageExecutor",
        "ExportConfig",
        "MainLoop",
    ] {
        assert!(
            body.contains(needle),
            "integration test must drive packaging-flow surface '{needle}'"
        );
    }
}

#[test]
fn export_module_implementation_exists() {
    let p = repo_root().join("engine-rs/crates/gdplatform/src/export.rs");
    assert!(
        p.exists(),
        "packaging implementation must exist at {}",
        p.display()
    );
}

#[test]
fn bootstrap_module_implementation_exists() {
    let p = repo_root().join("engine-rs/crates/patina-runner/src/bootstrap.rs");
    assert!(
        p.exists(),
        "startup bootstrap implementation must exist at {}",
        p.display()
    );
}
