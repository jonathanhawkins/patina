//! pat-6pyk: Validate that contributor onboarding docs cover required topics.
//!
//! These tests verify that:
//! 1. The onboarding doc exists and covers runtime workflow
//! 2. The onboarding doc covers oracle workflow
//! 3. CI pipeline documentation is present
//! 4. Repin workflow documentation is present
//! 5. Key sections have sufficient depth
//! 6. Cross-references to related docs are present

use std::fs;
use std::path::Path;

const ONBOARDING_PATH: &str =
    concat!(env!("CARGO_MANIFEST_DIR"), "/../docs/contributor-onboarding.md");

fn read_onboarding() -> String {
    fs::read_to_string(ONBOARDING_PATH)
        .expect("contributor-onboarding.md must exist at docs/contributor-onboarding.md")
}

// ===========================================================================
// 1. Doc exists and has basic structure
// ===========================================================================

#[test]
fn onboarding_doc_exists() {
    assert!(
        Path::new(ONBOARDING_PATH).exists(),
        "docs/contributor-onboarding.md must exist"
    );
}

#[test]
fn onboarding_doc_has_title() {
    let doc = read_onboarding();
    assert!(
        doc.contains("# Contributor Onboarding"),
        "doc must have a title"
    );
}

#[test]
fn onboarding_doc_has_prerequisites() {
    let doc = read_onboarding();
    assert!(
        doc.contains("## Prerequisites"),
        "doc must list prerequisites"
    );
    assert!(doc.contains("Rust"), "prerequisites must mention Rust");
    assert!(doc.contains("Godot"), "prerequisites must mention Godot");
}

// ===========================================================================
// 2. Runtime workflow coverage
// ===========================================================================

#[test]
fn onboarding_covers_runtime_workflow() {
    let doc = read_onboarding();
    assert!(
        doc.contains("## Runtime Workflow"),
        "doc must have Runtime Workflow section"
    );
}

#[test]
fn runtime_workflow_covers_building() {
    let doc = read_onboarding();
    assert!(
        doc.contains("cargo build"),
        "runtime section must explain how to build"
    );
}

#[test]
fn runtime_workflow_covers_test_tiers() {
    let doc = read_onboarding();
    assert!(
        doc.contains("Tier 1") && doc.contains("Tier 2"),
        "runtime section must describe test tiers"
    );
}

#[test]
fn runtime_workflow_covers_test_naming() {
    let doc = read_onboarding();
    assert!(
        doc.contains("_parity_test") && doc.contains("_golden_test"),
        "runtime section must explain test naming conventions"
    );
}

#[test]
fn runtime_workflow_covers_crate_structure() {
    let doc = read_onboarding();
    let key_crates = ["gdcore", "gdscene", "gdphysics2d", "gdresource", "gdobject"];
    for krate in &key_crates {
        assert!(
            doc.contains(krate),
            "runtime section must mention crate '{krate}'"
        );
    }
}

#[test]
fn runtime_workflow_covers_key_concepts() {
    let doc = read_onboarding();
    let concepts = ["Scene Tree", "PackedScene", "MainLoop", "PhysicsServer"];
    for concept in &concepts {
        assert!(
            doc.contains(concept),
            "runtime section must explain '{concept}'"
        );
    }
}

// ===========================================================================
// 3. Oracle workflow coverage
// ===========================================================================

#[test]
fn onboarding_covers_oracle_workflow() {
    let doc = read_onboarding();
    assert!(
        doc.contains("## Oracle Workflow"),
        "doc must have Oracle Workflow section"
    );
}

#[test]
fn oracle_workflow_covers_version_pinning() {
    let doc = read_onboarding();
    assert!(
        doc.contains("Version Pinning") || doc.contains("version pin"),
        "oracle section must explain version pinning"
    );
    assert!(
        doc.contains("UPSTREAM_VERSION"),
        "oracle section must reference UPSTREAM_VERSION"
    );
}

#[test]
fn oracle_workflow_covers_refresh() {
    let doc = read_onboarding();
    assert!(
        doc.contains("refresh_api.sh"),
        "oracle section must document the refresh command"
    );
}

#[test]
fn oracle_workflow_covers_gdextension_probes() {
    let doc = read_onboarding();
    assert!(
        doc.contains("GDExtension") && doc.contains("probe"),
        "oracle section must document GDExtension probes"
    );
}

#[test]
fn oracle_workflow_covers_gdscript_probes() {
    let doc = read_onboarding();
    assert!(
        doc.contains("GDScript") && doc.contains("probe"),
        "oracle section must document GDScript probes"
    );
}

#[test]
fn oracle_workflow_covers_parity_tests() {
    let doc = read_onboarding();
    assert!(
        doc.contains("Parity Test") || doc.contains("parity test") || doc.contains("Parity test"),
        "oracle section must explain how to write parity tests"
    );
}

// ===========================================================================
// 4. CI pipeline documentation
// ===========================================================================

#[test]
fn onboarding_covers_ci_pipeline() {
    let doc = read_onboarding();
    assert!(
        doc.contains("## CI Pipeline"),
        "doc must have CI Pipeline section"
    );
}

#[test]
fn ci_section_covers_main_gates() {
    let doc = read_onboarding();
    let gates = ["rust-fmt", "rust-render-goldens", "rust-oracle-parity"];
    for gate in &gates {
        assert!(
            doc.contains(gate),
            "CI section must document gate '{gate}'"
        );
    }
}

#[test]
fn ci_section_covers_compat_slices() {
    let doc = read_onboarding();
    let slices = [
        "rust-compat-headless",
        "rust-compat-2d",
        "rust-compat-3d",
        "rust-compat-platform",
    ];
    for slice in &slices {
        assert!(
            doc.contains(slice),
            "CI section must document compat slice '{slice}'"
        );
    }
}

#[test]
fn ci_section_covers_repin_validation() {
    let doc = read_onboarding();
    assert!(
        doc.contains("Repin Validation") || doc.contains("repin-validation"),
        "CI section must document the repin validation pipeline"
    );
}

#[test]
fn ci_section_covers_reading_failures() {
    let doc = read_onboarding();
    assert!(
        doc.contains("CI Failure") || doc.contains("Reading CI") || doc.contains("failing gate"),
        "CI section must explain how to read CI failures"
    );
}

// ===========================================================================
// 5. Repin workflow documentation
// ===========================================================================

#[test]
fn onboarding_covers_repin_workflow() {
    let doc = read_onboarding();
    assert!(
        doc.contains("Repin Workflow") || doc.contains("## Repin"),
        "doc must have a dedicated repin workflow section"
    );
}

#[test]
fn repin_section_covers_submodule_update() {
    let doc = read_onboarding();
    assert!(
        doc.contains("upstream/godot") && doc.contains("submodule"),
        "repin section must explain submodule update"
    );
}

#[test]
fn repin_section_covers_artifact_regeneration() {
    let doc = read_onboarding();
    assert!(
        doc.contains("Regenerate") || doc.contains("regenerat"),
        "repin section must cover artifact regeneration"
    );
}

#[test]
fn repin_section_covers_manual_dispatch() {
    let doc = read_onboarding();
    assert!(
        doc.contains("Manual Dispatch") || doc.contains("workflow_dispatch"),
        "repin section must document manual dispatch"
    );
}

// ===========================================================================
// 6. Cross-references
// ===========================================================================

#[test]
fn onboarding_references_agents_md() {
    let doc = read_onboarding();
    assert!(
        doc.contains("AGENTS.md"),
        "doc must reference AGENTS.md"
    );
}

#[test]
fn onboarding_references_ci_workflow() {
    let doc = read_onboarding();
    assert!(
        doc.contains("ci.yml"),
        "doc must reference ci.yml"
    );
}

#[test]
fn onboarding_references_repin_workflow_file() {
    let doc = read_onboarding();
    assert!(
        doc.contains("repin-validation.yml"),
        "doc must reference repin-validation.yml"
    );
}

#[test]
fn onboarding_references_port_plan() {
    let doc = read_onboarding();
    assert!(
        doc.contains("PORT_GODOT_TO_RUST_PLAN"),
        "doc must reference the port plan"
    );
}

// ===========================================================================
// 7. Doc is substantial enough
// ===========================================================================

#[test]
fn onboarding_doc_has_sufficient_length() {
    let doc = read_onboarding();
    let line_count = doc.lines().count();
    assert!(
        line_count >= 150,
        "onboarding doc must be at least 150 lines (got {line_count})"
    );
}

#[test]
fn onboarding_doc_has_at_least_6_h2_sections() {
    let doc = read_onboarding();
    let h2_count = doc.lines().filter(|l| l.starts_with("## ")).count();
    assert!(
        h2_count >= 6,
        "onboarding doc must have at least 6 top-level sections (got {h2_count})"
    );
}
