//! pat-pumyh: Cross-platform export validation workflow.
//!
//! Guarantees that the dedicated `.github/workflows/export-validation.yml`
//! workflow exists and exercises all three desktop targets (Linux, macOS,
//! Windows), and that the `gdplatform::ci_artifact` export validation
//! helpers produce the expected structural coverage.
//!
//! Covers:
//! 1. The export-validation workflow YAML exists.
//! 2. The workflow matrix covers ubuntu-latest, macos-latest, windows-latest.
//! 3. The workflow invokes the export validation tests.
//! 4. `validate_default_cross_platform_plan` returns an OK report with three
//!    validated artifacts.
//! 5. Per-target artifacts from the default plan all validate individually.
//! 6. Missing-target plans produce the expected errors.
//! 7. `validate_artifact_structure` rejects mismatched artifacts.

use gdplatform::ci_artifact::{
    expected_artifact_extension, validate_artifact_structure,
    validate_default_cross_platform_plan, validate_plan_cross_platform, CiArtifact, CiArtifactPlan,
};
use gdplatform::export::BuildProfile;

// ── Workflow YAML assertions ─────────────────────────────────────────

fn read_export_validation_yaml() -> String {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../.github/workflows/export-validation.yml"
    );
    std::fs::read_to_string(path).expect("export-validation.yml must exist")
}

#[test]
fn export_validation_workflow_exists() {
    let yaml = read_export_validation_yaml();
    assert!(!yaml.is_empty(), "workflow file must not be empty");
}

#[test]
fn export_validation_workflow_covers_all_three_os() {
    let yaml = read_export_validation_yaml();
    assert!(
        yaml.contains("ubuntu-latest"),
        "workflow must run on Linux"
    );
    assert!(yaml.contains("macos-latest"), "workflow must run on macOS");
    assert!(
        yaml.contains("windows-latest"),
        "workflow must run on Windows"
    );
}

#[test]
fn export_validation_workflow_includes_all_three_triples() {
    let yaml = read_export_validation_yaml();
    assert!(yaml.contains("x86_64-unknown-linux-gnu"));
    assert!(yaml.contains("x86_64-apple-darwin"));
    assert!(yaml.contains("x86_64-pc-windows-msvc"));
}

#[test]
fn export_validation_workflow_runs_validation_tests() {
    let yaml = read_export_validation_yaml();
    assert!(
        yaml.contains("export_validation_workflow_test"),
        "workflow must run the integration test"
    );
    assert!(
        yaml.contains("ci_artifact"),
        "workflow must run ci_artifact unit tests"
    );
}

#[test]
fn export_validation_workflow_has_export_validation_job() {
    let yaml = read_export_validation_yaml();
    assert!(
        yaml.contains("export-validation"),
        "workflow must define an export-validation job"
    );
}

// ── Validation helper assertions ─────────────────────────────────────

#[test]
fn default_cross_platform_plan_validates_ok() {
    let report = validate_default_cross_platform_plan("patina");
    assert!(
        report.is_ok(),
        "default cross-platform plan must validate: {:?}",
        report.errors
    );
    assert_eq!(
        report.validated_count, 3,
        "default plan should validate exactly three artifacts"
    );
}

#[test]
fn cross_platform_plan_requires_all_three_os() {
    // Linux + macOS only — must flag missing Windows.
    let plan = CiArtifactPlan::new("patina")
        .with_target("x86_64-unknown-linux-gnu")
        .with_target("x86_64-apple-darwin");
    let report = validate_plan_cross_platform(&plan);
    assert!(!report.is_ok(), "two-of-three plan must fail validation");
    assert!(
        report.errors.iter().any(|e| e.contains("Windows")),
        "report should flag missing Windows artifact: {:?}",
        report.errors
    );
}

#[test]
fn generated_artifacts_validate_per_target() {
    let plan = CiArtifactPlan::new("patina")
        .with_target("x86_64-unknown-linux-gnu")
        .with_target("x86_64-apple-darwin")
        .with_target("x86_64-pc-windows-msvc");
    let artifacts = plan.generate_artifacts();
    assert_eq!(artifacts.len(), 3);
    for artifact in &artifacts {
        validate_artifact_structure(artifact).unwrap_or_else(|e| {
            panic!(
                "generated artifact '{}' should validate: {}",
                artifact.name, e
            )
        });
        let ext = expected_artifact_extension(&artifact.platform);
        assert!(
            !ext.is_empty(),
            "desktop platform '{}' must have an expected extension",
            artifact.platform
        );
        assert!(
            artifact.filename.ends_with(ext),
            "artifact '{}' filename '{}' should end with '{}'",
            artifact.name,
            artifact.filename,
            ext
        );
    }
}

#[test]
fn validate_artifact_structure_rejects_mismatched_artifact() {
    let bad = CiArtifact {
        name: "bogus".into(),
        filename: "bogus.macos.release.app".into(),
        rust_triple: "x86_64-unknown-linux-gnu".into(),
        profile: BuildProfile::Release,
        platform: "macos".into(),
        arch: "x86_64".into(),
        upload: true,
        sha256: None,
        size_bytes: None,
    };
    assert!(
        validate_artifact_structure(&bad).is_err(),
        "mismatched artifact must fail validation"
    );
}

#[test]
fn extension_matches_expected_per_platform() {
    assert_eq!(expected_artifact_extension("linux"), ".x86_64");
    assert_eq!(expected_artifact_extension("macos"), ".app");
    assert_eq!(expected_artifact_extension("windows"), ".exe");
}
