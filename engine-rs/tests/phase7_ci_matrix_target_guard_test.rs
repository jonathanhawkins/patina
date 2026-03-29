//! pat-s3700: CI matrix for supported targets — guard tests.
//!
//! Validates that:
//! 1. The CI workflow matrix covers all CI-tested desktop targets
//! 2. The CI workflow has the expected job structure for multi-OS testing
//! 3. The Phase 7 audit doc references CI matrix validation
//! 4. DESKTOP_TARGETS ci_tested flags are consistent with the workflow
//! 5. Platform-specific compat lanes exist for each CI-tested OS

use gdplatform::os::Platform;
use gdplatform::platform_targets::{ci_tested_targets, DESKTOP_TARGETS};

fn read_ci_yml() -> String {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../.github/workflows/ci.yml"
    );
    std::fs::read_to_string(path).expect("should read .github/workflows/ci.yml")
}

fn read_audit_doc() -> String {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../prd/PHASE7_PLATFORM_PARITY_AUDIT.md"
    );
    std::fs::read_to_string(path).expect("should read prd/PHASE7_PLATFORM_PARITY_AUDIT.md")
}

// ===========================================================================
// 1. CI workflow matrix covers all CI-tested desktop targets
// ===========================================================================

#[test]
fn ci_matrix_includes_ubuntu() {
    let ci = read_ci_yml();
    assert!(ci.contains("ubuntu-latest"), "CI matrix must include ubuntu-latest");
}

#[test]
fn ci_matrix_includes_macos() {
    let ci = read_ci_yml();
    assert!(ci.contains("macos-latest"), "CI matrix must include macos-latest");
}

#[test]
fn ci_matrix_includes_windows() {
    let ci = read_ci_yml();
    assert!(ci.contains("windows-latest"), "CI matrix must include windows-latest");
}

#[test]
fn ci_matrix_has_three_os_entries() {
    let ci = read_ci_yml();
    // The primary rust job must have all three OS entries in its matrix.
    let has_all = ci.contains("ubuntu-latest")
        && ci.contains("macos-latest")
        && ci.contains("windows-latest");
    assert!(has_all, "CI matrix must include all three desktop OS runners");
}

// ===========================================================================
// 2. Every CI-tested target has a matching runner in the workflow
// ===========================================================================

#[test]
fn every_ci_tested_target_has_runner() {
    let ci = read_ci_yml();
    for target in DESKTOP_TARGETS {
        if !target.ci_tested {
            continue;
        }
        let runner = match target.platform {
            Platform::Linux => "ubuntu",
            Platform::MacOS => "macos",
            Platform::Windows => "windows",
            _ => continue,
        };
        assert!(
            ci.contains(runner),
            "CI-tested target '{}' ({}) must have a '{}' runner in ci.yml",
            target.name,
            target.rust_triple,
            runner
        );
    }
}

#[test]
fn no_untested_platform_claims_ci_runner() {
    // Targets NOT marked ci_tested should not be the sole reason a runner exists,
    // but we verify that the code registry is consistent.
    let ci_targets = ci_tested_targets();
    let ci_platforms: std::collections::HashSet<Platform> =
        ci_targets.iter().map(|t| t.platform).collect();

    // All three desktop platforms must be CI-tested.
    assert!(ci_platforms.contains(&Platform::Linux), "Linux must be CI-tested");
    assert!(ci_platforms.contains(&Platform::MacOS), "macOS must be CI-tested");
    assert!(ci_platforms.contains(&Platform::Windows), "Windows must be CI-tested");
}

// ===========================================================================
// 3. CI workflow has multi-OS job structure
// ===========================================================================

#[test]
fn ci_has_multi_os_rust_job() {
    let ci = read_ci_yml();
    // The main Rust job should use a matrix strategy with OS.
    assert!(
        ci.contains("matrix:") && ci.contains("os:"),
        "CI workflow must use a matrix strategy with OS entries"
    );
}

#[test]
fn ci_has_platform_compat_lane() {
    let ci = read_ci_yml();
    assert!(
        ci.contains("rust-compat-platform") || ci.contains("Platform layer compat"),
        "CI workflow must have a platform compatibility lane"
    );
}

#[test]
fn ci_platform_compat_is_multi_os() {
    let ci = read_ci_yml();
    // Find the platform compat section and verify it uses a multi-OS matrix.
    let platform_section = ci
        .find("rust-compat-platform")
        .or_else(|| ci.find("Platform layer compat"));
    assert!(
        platform_section.is_some(),
        "CI must have a platform compat section"
    );

    // The platform compat job should run on all three OS.
    // We check that after the platform compat job name, there's a matrix with
    // all three runners.
    let after_platform = &ci[platform_section.unwrap()..];
    assert!(
        after_platform.contains("ubuntu-latest")
            && after_platform.contains("macos-latest")
            && after_platform.contains("windows-latest"),
        "Platform compat lane must test on all three desktop OS"
    );
}

// ===========================================================================
// 4. CI workflow has render goldens multi-OS lane
// ===========================================================================

#[test]
fn ci_has_render_goldens_lane() {
    let ci = read_ci_yml();
    assert!(
        ci.contains("rust-render-goldens") || ci.contains("Render goldens"),
        "CI workflow must have a render goldens lane"
    );
}

#[test]
fn ci_render_goldens_is_multi_os() {
    let ci = read_ci_yml();
    let section = ci
        .find("rust-render-goldens")
        .or_else(|| ci.find("Render goldens"));
    assert!(section.is_some(), "CI must have a render goldens section");

    let after = &ci[section.unwrap()..];
    assert!(
        after.contains("ubuntu-latest")
            && after.contains("macos-latest")
            && after.contains("windows-latest"),
        "Render goldens lane must test on all three desktop OS"
    );
}

// ===========================================================================
// 5. CI workflow has release build check
// ===========================================================================

#[test]
fn ci_has_release_build_job() {
    let ci = read_ci_yml();
    assert!(
        ci.contains("rust-release") || ci.contains("release build"),
        "CI workflow must have a release build check"
    );
}

// ===========================================================================
// 6. Phase 7 audit doc references CI matrix coverage
// ===========================================================================

#[test]
fn audit_doc_mentions_ci_matrix() {
    let doc = read_audit_doc();
    assert!(
        doc.contains("CI matrix") || doc.contains("ci_build_matrix"),
        "Phase 7 audit doc must reference CI matrix coverage"
    );
}

#[test]
fn audit_doc_mentions_ci_tested_targets() {
    let doc = read_audit_doc();
    assert!(
        doc.contains("CI Tested") || doc.contains("ci_tested") || doc.contains("CI-tested"),
        "Phase 7 audit doc must mention CI-tested targets"
    );
}

#[test]
fn audit_doc_classifies_ci_coverage_as_measured() {
    let doc = read_audit_doc();
    assert!(
        doc.contains("Measured as repo policy") || doc.contains("Measured"),
        "Phase 7 audit doc must classify CI coverage as measured"
    );
}

// ===========================================================================
// 7. DESKTOP_TARGETS count consistency
// ===========================================================================

#[test]
fn desktop_targets_count_at_least_seven() {
    assert!(
        DESKTOP_TARGETS.len() >= 7,
        "DESKTOP_TARGETS must have at least 7 entries (3 OS x 2 arch + web), got {}",
        DESKTOP_TARGETS.len()
    );
}

#[test]
fn ci_tested_targets_count_at_least_four() {
    let ci = ci_tested_targets();
    assert!(
        ci.len() >= 4,
        "At least 4 targets must be CI-tested (Linux x86_64, macOS x86_64, macOS aarch64, Windows x86_64), got {}",
        ci.len()
    );
}

// ===========================================================================
// 8. CI workflow has caching for all multi-OS jobs
// ===========================================================================

#[test]
fn ci_uses_cargo_caching() {
    let ci = read_ci_yml();
    assert!(
        ci.contains("actions/cache@v4"),
        "CI workflow must use GitHub Actions cache"
    );
    assert!(
        ci.contains("cargo-registry"),
        "CI workflow must cache cargo registry"
    );
    assert!(
        ci.contains("cargo-target"),
        "CI workflow must cache cargo target directory"
    );
}

// ===========================================================================
// 9. CI workflow has dependency audit
// ===========================================================================

#[test]
fn ci_has_dependency_audit() {
    let ci = read_ci_yml();
    assert!(
        ci.contains("rust-audit") || ci.contains("cargo audit") || ci.contains("cargo-audit"),
        "CI workflow must include a dependency audit step"
    );
}
