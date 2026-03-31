//! pat-vjmfv: Doc-validation tests for Phase 7 startup and packaging flow.
//!
//! Guards that:
//! 1. `prd/PHASE7_PLATFORM_PARITY_AUDIT.md` documents the startup and packaging flow
//! 2. The audit doc cites the correct implementation and evidence files
//! 3. The doc explicitly scopes what the packaging path does NOT cover
//! 4. The startup_runtime_packaging_flow_test.rs cites the audit doc

/// Read the Phase 7 audit doc.
fn read_audit_doc() -> String {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../prd/PHASE7_PLATFORM_PARITY_AUDIT.md"
    );
    std::fs::read_to_string(path).expect("should read prd/PHASE7_PLATFORM_PARITY_AUDIT.md")
}

/// Read the startup/packaging flow test.
fn read_flow_test() -> String {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/startup_runtime_packaging_flow_test.rs"
    );
    std::fs::read_to_string(path).expect("should read startup_runtime_packaging_flow_test.rs")
}

// ===========================================================================
// 1. Audit doc contains the startup and packaging sections
// ===========================================================================

#[test]
fn audit_doc_has_startup_and_packaging_flow_section() {
    let doc = read_audit_doc();
    assert!(
        doc.contains("## Startup and Packaging Flow"),
        "audit doc must contain a '## Startup and Packaging Flow' section"
    );
}

#[test]
fn audit_doc_has_startup_lifecycle_section() {
    let doc = read_audit_doc();
    assert!(
        doc.contains("### Startup Lifecycle"),
        "audit doc must document the startup lifecycle"
    );
}

#[test]
fn audit_doc_has_packaging_artifact_path_section() {
    let doc = read_audit_doc();
    assert!(
        doc.contains("### Packaging Artifact Path"),
        "audit doc must document the packaging artifact path"
    );
}

#[test]
fn audit_doc_has_not_covered_section() {
    let doc = read_audit_doc();
    assert!(
        doc.contains("### What This Path Does NOT Cover"),
        "audit doc must explicitly list what the packaging path does not cover"
    );
}

// ===========================================================================
// 2. Audit doc cites implementation and evidence files
// ===========================================================================

#[test]
fn audit_doc_cites_bootstrap_implementation() {
    let doc = read_audit_doc();
    assert!(
        doc.contains("patina-runner/src/bootstrap.rs"),
        "audit doc must cite the bootstrap implementation"
    );
}

#[test]
fn audit_doc_cites_export_implementation() {
    let doc = read_audit_doc();
    assert!(
        doc.contains("gdplatform/src/export.rs"),
        "audit doc must cite the export implementation"
    );
}

#[test]
fn audit_doc_cites_flow_test_as_evidence() {
    let doc = read_audit_doc();
    assert!(
        doc.contains("startup_runtime_packaging_flow_test.rs"),
        "audit doc must cite the flow test as evidence"
    );
}

#[test]
fn audit_doc_cites_ci_gate_test() {
    let doc = read_audit_doc();
    assert!(
        doc.contains("startup_packaging_ci_gate_test.rs"),
        "audit doc must cite the CI gate test"
    );
}

// ===========================================================================
// 3. Audit doc describes the 8-phase bootstrap sequence
// ===========================================================================

#[test]
fn audit_doc_lists_all_boot_phases() {
    let doc = read_audit_doc();
    let required_phases = [
        "Core",
        "Servers",
        "Resources",
        "SceneTree",
        "MainScene",
        "Scripts",
        "Lifecycle",
        "Running",
    ];
    for phase in &required_phases {
        assert!(
            doc.contains(phase),
            "audit doc must list boot phase '{phase}'"
        );
    }
}

// ===========================================================================
// 4. Audit doc describes the 3 staging artifacts
// ===========================================================================

#[test]
fn audit_doc_describes_manifest_artifact() {
    let doc = read_audit_doc();
    assert!(
        doc.contains("export_manifest.txt"),
        "audit doc must describe the export manifest artifact"
    );
}

#[test]
fn audit_doc_describes_resource_listing_artifact() {
    let doc = read_audit_doc();
    assert!(
        doc.contains("resource_list.txt"),
        "audit doc must describe the resource listing artifact"
    );
}

#[test]
fn audit_doc_describes_output_marker() {
    let doc = read_audit_doc();
    // The output marker pattern: <AppName>.<platform>.<profile>.<arch>
    assert!(
        doc.contains("output marker") || doc.contains("staging placeholder"),
        "audit doc must describe the output marker artifact"
    );
}

// ===========================================================================
// 5. Audit doc explicitly disclaims what is NOT covered
// ===========================================================================

#[test]
fn audit_doc_disclaims_godot_export_parity() {
    let doc = read_audit_doc();
    assert!(
        doc.contains("Godot export preset/template parity") || doc.contains("full Godot export"),
        "audit doc must disclaim Godot export parity"
    );
}

#[test]
fn audit_doc_disclaims_native_binary_generation() {
    let doc = read_audit_doc();
    assert!(
        doc.contains("Native binary generation") || doc.contains("native binary"),
        "audit doc must disclaim native binary generation"
    );
}

#[test]
fn audit_doc_disclaims_app_bundle_creation() {
    let doc = read_audit_doc();
    assert!(
        doc.contains("App bundle creation") || doc.contains("app bundle"),
        "audit doc must disclaim app bundle creation"
    );
}

// ===========================================================================
// 6. Flow test cites the audit doc as source of truth
// ===========================================================================

#[test]
fn flow_test_cites_audit_doc() {
    let test = read_flow_test();
    assert!(
        test.contains("PHASE7_PLATFORM_PARITY_AUDIT.md"),
        "startup_runtime_packaging_flow_test.rs must cite the Phase 7 audit doc"
    );
}

#[test]
fn flow_test_documents_scope() {
    let test = read_flow_test();
    assert!(
        test.contains("staging") || test.contains("Patina packaging flow"),
        "flow test must document that scope is limited to Patina's staging path"
    );
}

// ===========================================================================
// 7. Packaging flow exercises the 4-step pipeline
// ===========================================================================

#[test]
fn audit_doc_describes_config_step() {
    let doc = read_audit_doc();
    assert!(
        doc.contains("ExportConfig"),
        "audit doc must describe the ExportConfig step"
    );
}

#[test]
fn audit_doc_describes_validate_step() {
    let doc = read_audit_doc();
    assert!(
        doc.contains("validate_platform"),
        "audit doc must describe the platform validation step"
    );
}

#[test]
fn audit_doc_describes_collect_step() {
    let doc = read_audit_doc();
    assert!(
        doc.contains("validate_and_collect"),
        "audit doc must describe the resource collection step"
    );
}

#[test]
fn audit_doc_describes_stage_step() {
    let doc = read_audit_doc();
    assert!(
        doc.contains("PackageExecutor::run()") || doc.contains("Stage"),
        "audit doc must describe the staging step"
    );
}
