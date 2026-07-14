//! pat-08em8: Acceptance gate for the minimal editor-facing compatibility layer.
//!
//! Acceptance: the deliverable is broken into measurable evidence with tests,
//! docs, or oracle-backed artifacts. The minimal editor-facing compatibility
//! layer ships as: (1) the `gdeditor::editor_compat` module that implements
//! the Godot-compatible API surface, (2) the Phase-8 audit doc that scopes
//! and classifies what is "measured" vs "deferred", and (3) the validation
//! test suite that checks both stay aligned. This test guards each of those
//! anchors so any regression — module renamed, audit deleted, validation
//! suite stripped — fails under the bead's marker.

use std::path::PathBuf;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

#[test]
fn editor_compat_module_source_exists() {
    let p = repo_root().join("engine-rs/crates/gdeditor/src/editor_compat.rs");
    assert!(
        p.exists(),
        "gdeditor::editor_compat module must exist at {} as the implementation anchor",
        p.display()
    );
}

#[test]
fn phase8_audit_doc_exists_and_scopes_compat_layer() {
    let p = repo_root().join("prd/PHASE8_EDITOR_PARITY_AUDIT.md");
    let body = std::fs::read_to_string(&p)
        .unwrap_or_else(|e| panic!("Phase-8 audit must exist at {}: {e}", p.display()));
    // Audit names the module by Rust path `gdeditor::editor_interface` and
    // the API surface `EditorInterface`. It also calls out the
    // "editor-facing compatibility layer" prose anchor.
    for needle in ["EditorInterface", "editor_interface", "compatibility layer"] {
        assert!(
            body.contains(needle),
            "Phase-8 audit must scope compat-layer surface '{needle}'"
        );
    }
}

#[test]
fn validation_suite_files_are_present() {
    // The minimum measured-evidence anchors. We don't care about pass/fail here
    // (the verifier lane runs them); only that they have not been deleted.
    let test_files = [
        "engine-rs/tests/phase8_editor_compat_layer_validation_test.rs",
        "engine-rs/tests/editor_compat_layer_test.rs",
        "engine-rs/tests/editor_interface_compat_test.rs",
        "engine-rs/tests/editor_compat_layer_surface_test.rs",
        "engine-rs/tests/phase8_editor_compatibility_layer_scope_test.rs",
    ];
    for f in &test_files {
        let p = repo_root().join(f);
        assert!(
            p.exists(),
            "compat-layer evidence test must exist: {}",
            p.display()
        );
    }
}

#[test]
fn compat_matrix_row_lists_editor_compat_layer() {
    let p = repo_root().join("COMPAT_MATRIX.md");
    let body = std::fs::read_to_string(&p)
        .unwrap_or_else(|e| panic!("COMPAT_MATRIX.md must exist at {}: {e}", p.display()));
    assert!(
        body.contains("Editor-Facing Compatibility Layer"),
        "COMPAT_MATRIX.md must surface the editor-facing compatibility layer row"
    );
    assert!(
        body.contains("gdeditor") && body.contains("Measured"),
        "COMPAT_MATRIX.md row must classify the gdeditor compat layer as Measured"
    );
}
