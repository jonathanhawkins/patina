//! pat-6m9ky: Scope guard for the minimal Phase 8 editor-facing compatibility layer.
//!
//! Source of truth: `prd/PHASE8_EDITOR_PARITY_AUDIT.md`
//! Classification: Measured for the minimal editor-facing slice
//!
//! Verifies that the Phase 8 audit explicitly distinguishes the minimal
//! browser/editor-shell + `EditorInterface` slice from broader Godot editor
//! parity.

use std::fs;
use std::path::PathBuf;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn read_phase8_audit() -> String {
    fs::read_to_string(workspace_root().join("../prd/PHASE8_EDITOR_PARITY_AUDIT.md"))
        .expect("prd/PHASE8_EDITOR_PARITY_AUDIT.md must exist")
}

#[test]
fn phase8_audit_scopes_minimal_editor_compatibility_layer() {
    let audit = read_phase8_audit();
    let lower = audit.to_lowercase();

    assert!(
        lower.contains("minimal browser-served editor shell"),
        "audit must name the minimal browser/editor-shell slice"
    );
    assert!(
        audit.contains("minimal `EditorInterface` compatibility layer"),
        "audit must name the minimal EditorInterface slice"
    );
    assert!(
        lower.contains("measured minimal browser/editor-shell"),
        "audit must describe the slice as measured"
    );
    assert!(
        audit.contains("broader Godot editor parity"),
        "audit must distinguish the slice from broader parity"
    );
    assert!(
        audit.contains("pat-6m9ky"),
        "audit must reference the bead that owns the compatibility slice"
    );
}
