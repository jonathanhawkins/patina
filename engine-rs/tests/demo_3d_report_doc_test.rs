//! pat-57aw6: Phase 6 3D report aligned with the parity audit.
//!
//! Guards the report deliverable itself so the phase-6 milestone remains tied
//! to real fixtures, measurable subsystems, concrete rerunnable evidence, and
//! the audit-aligned three-tier classification from prd/PHASE6_3D_PARITY_AUDIT.md.

use std::fs;
use std::path::PathBuf;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf()
}

fn read_report() -> String {
    let path = repo_root().join("docs/3D_DEMO_PARITY_REPORT.md");
    fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()))
}

#[test]
fn report_names_current_bead() {
    let report = read_report();
    assert!(
        report.contains("pat-57aw6"),
        "report must identify the active bead"
    );
}

#[test]
fn report_lists_all_phase6_3d_fixtures() {
    let report = read_report();
    for fixture in [
        "minimal_3d.tscn",
        "hierarchy_3d.tscn",
        "indoor_3d.tscn",
        "multi_light_3d.tscn",
        "physics_3d_playground.tscn",
        "animated_scene_3d.tscn",
        "csg_composition.tscn",
        "foggy_terrain_3d.tscn",
        "outdoor_3d.tscn",
        "spotlight_gallery_3d.tscn",
        "vehicle_3d.tscn",
        "physics_playground_extended.tscn",
    ] {
        assert!(
            report.contains(fixture),
            "report must include fixture {fixture}"
        );
    }
}

#[test]
fn report_references_corpus_definition_in_audit() {
    let report = read_report();
    assert!(
        report.contains("prd/PHASE6_3D_PARITY_AUDIT.md"),
        "report must reference the audit doc for corpus definition"
    );
    assert!(
        report.contains("3D Fixture Corpus Definition"),
        "report must reference the corpus definition section in the audit"
    );
}

#[test]
fn report_covers_transform_render_and_physics() {
    let report = read_report();
    for subsystem in ["Transform", "Render", "Physics"] {
        assert!(
            report.contains(subsystem),
            "report must mention {subsystem} coverage"
        );
    }
}

#[test]
fn report_references_rerunnable_test_commands() {
    let report = read_report();
    assert!(
        report.contains(
            "cargo test -p patina-engine --test demo_3d_parity_report_test minimal_3d_scene_has_expected_structure -- --exact"
        ),
        "report must reference the structure parity command"
    );
    assert!(
        report.contains("cargo test -p patina-engine --test demo_3d_report_doc_test"),
        "report must reference the report guard test command"
    );
    assert!(
        report.contains(
            "cargo test -p patina-engine --test demo_3d_parity_report_test physics_3d_freefall_matches_golden_trace -- --exact"
        ),
        "report must reference the physics parity command"
    );
}

#[test]
fn report_cites_phase6_audit_and_has_three_tier_classification() {
    let report = read_report();
    assert!(
        report.contains("prd/PHASE6_3D_PARITY_AUDIT.md"),
        "report must cite the Phase 6 parity audit document"
    );
    assert!(
        report.contains("Audit-Aligned 3D Family Classification"),
        "report must have the audit-aligned classification section"
    );
    // All three tiers must be present
    assert!(
        report.contains("### Measured"),
        "report must have Measured tier"
    );
    assert!(
        report.contains("### Implemented, not yet measured"),
        "report must have Implemented-not-yet-measured tier"
    );
    assert!(
        report.contains("### Deferred or explicitly limited"),
        "report must have Deferred tier"
    );
    // Key measured families from the audit
    for family in &["Node3D", "Camera3D", "RigidBody3D", "StaticBody3D", "OmniLight3D", "SpotLight3D"] {
        assert!(
            report.contains(family),
            "report classification must include measured family {family}"
        );
    }
    // Key deferred families from the audit
    for family in &["VehicleBody3D", "SoftBody3D", "SpringArm3D"] {
        assert!(
            report.contains(family),
            "report classification must include deferred family {family}"
        );
    }
}

#[test]
fn report_describes_measurable_parity_hooks() {
    let report = read_report();
    assert!(
        report.contains("ParityReport3D"),
        "report must anchor render evidence in ParityReport3D"
    );
    assert!(
        report.contains("golden"),
        "report must mention golden comparison evidence"
    );
    assert!(
        report.contains("deterministic"),
        "report must mention deterministic evidence"
    );
}
