//! pat-1fo1e / pat-s3700: CI build matrix for supported targets.
//!
//! Source of truth: `prd/PHASE7_PLATFORM_PARITY_AUDIT.md`
//!
//! Validates:
//! 1. CI workflow YAML exists and contains a multi-platform matrix
//! 2. All three desktop OS targets (Linux, macOS, Windows) are in the matrix
//! 3. Platform target definitions cover all three OSes
//! 4. CI-tested targets include at least one per OS
//! 5. Platform detection works on current build host
//! 6. Architecture detection is consistent with target triple
//! 7. Capability checks pass on current desktop platform
//! 8. Render golden tests also use the 3-platform matrix
//! 9. Caching is configured per-platform
//! 10. The workflow has the expected job structure
//! 11. The Phase 7 audit doc exists and is cited as the source of truth
//! 12. CI-tested targets match the documented supported target matrix
//! 13. CI workflow jobs cover the expected domain lanes (headless, 2D, 3D,
//!     platform, fuzz, meta, oracle)

use gdplatform::os::Platform;
use gdplatform::platform_targets::{
    ci_tested_targets, current_target, targets_for_platform, validate_current_target, Architecture,
    PlatformCapability, DESKTOP_TARGETS,
};

// ── CI YAML validation ───────────────────────────────────────────────

fn read_ci_yaml() -> String {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../.github/workflows/ci.yml");
    std::fs::read_to_string(path).expect("ci.yml must exist")
}

#[test]
fn ci_yaml_exists() {
    let yaml = read_ci_yaml();
    assert!(!yaml.is_empty());
}

#[test]
fn ci_yaml_has_matrix_with_all_three_os() {
    let yaml = read_ci_yaml();
    assert!(yaml.contains("ubuntu-latest"), "CI must include Linux");
    assert!(yaml.contains("macos-latest"), "CI must include macOS");
    assert!(yaml.contains("windows-latest"), "CI must include Windows");
}

#[test]
fn ci_yaml_has_rust_build_job() {
    let yaml = read_ci_yaml();
    assert!(
        yaml.contains("cargo build --workspace"),
        "CI must build workspace"
    );
    assert!(
        yaml.contains("cargo test --workspace"),
        "CI must test workspace"
    );
    assert!(
        yaml.contains("cargo clippy --workspace"),
        "CI must run clippy"
    );
}

#[test]
fn ci_yaml_has_render_golden_job() {
    let yaml = read_ci_yaml();
    assert!(
        yaml.contains("Render goldens"),
        "CI must have render golden job"
    );
}

#[test]
fn ci_yaml_has_release_build_check() {
    let yaml = read_ci_yaml();
    assert!(
        yaml.contains("cargo build --workspace --release"),
        "CI must check release build"
    );
}

#[test]
fn ci_yaml_has_fmt_check() {
    let yaml = read_ci_yaml();
    assert!(yaml.contains("cargo fmt"), "CI must check formatting");
}

#[test]
fn ci_yaml_has_cargo_caching() {
    let yaml = read_ci_yaml();
    assert!(
        yaml.contains("actions/cache@v4"),
        "CI must cache cargo artifacts"
    );
    assert!(
        yaml.contains("cargo-registry"),
        "CI must cache cargo registry"
    );
    assert!(
        yaml.contains("cargo-target"),
        "CI must cache target directory"
    );
}

#[test]
fn ci_yaml_has_concurrency_cancellation() {
    let yaml = read_ci_yaml();
    assert!(
        yaml.contains("cancel-in-progress: true"),
        "CI must cancel superseded runs"
    );
}

#[test]
fn ci_yaml_has_fail_fast_disabled() {
    let yaml = read_ci_yaml();
    assert!(
        yaml.contains("fail-fast: false"),
        "Matrix should not fail-fast so all platforms report"
    );
}

#[test]
fn ci_yaml_has_dependency_audit() {
    let yaml = read_ci_yaml();
    assert!(
        yaml.contains("cargo audit") || yaml.contains("cargo-audit"),
        "CI should audit deps"
    );
}

// ── Platform targets ─────────────────────────────────────────────────

#[test]
fn desktop_targets_cover_three_oses() {
    let linux = targets_for_platform(Platform::Linux);
    let macos = targets_for_platform(Platform::MacOS);
    let windows = targets_for_platform(Platform::Windows);

    assert!(!linux.is_empty(), "must define Linux targets");
    assert!(!macos.is_empty(), "must define macOS targets");
    assert!(!windows.is_empty(), "must define Windows targets");
}

#[test]
fn linux_has_x86_64_target() {
    let linux = targets_for_platform(Platform::Linux);
    assert!(
        linux.iter().any(|t| t.arch == Architecture::X86_64),
        "Linux must have x86_64 target"
    );
}

#[test]
fn macos_has_both_architectures() {
    let macos = targets_for_platform(Platform::MacOS);
    assert!(
        macos.iter().any(|t| t.arch == Architecture::X86_64),
        "macOS must have x86_64 target"
    );
    assert!(
        macos.iter().any(|t| t.arch == Architecture::Aarch64),
        "macOS must have aarch64 (Apple Silicon) target"
    );
}

#[test]
fn windows_has_x86_64_target() {
    let windows = targets_for_platform(Platform::Windows);
    assert!(
        windows.iter().any(|t| t.arch == Architecture::X86_64),
        "Windows must have x86_64 target"
    );
}

// ── CI-tested targets ────────────────────────────────────────────────

#[test]
fn ci_tested_covers_all_three_oses() {
    let ci = ci_tested_targets();
    assert!(
        ci.iter().any(|t| t.platform == Platform::Linux),
        "CI must test at least one Linux target"
    );
    assert!(
        ci.iter().any(|t| t.platform == Platform::MacOS),
        "CI must test at least one macOS target"
    );
    assert!(
        ci.iter().any(|t| t.platform == Platform::Windows),
        "CI must test at least one Windows target"
    );
}

#[test]
fn ci_tested_targets_all_have_gpu_support() {
    let ci = ci_tested_targets();
    for target in &ci {
        assert!(
            target.gpu_supported,
            "CI-tested target {} should support GPU",
            target.name
        );
    }
}

#[test]
fn ci_tested_targets_all_have_windowing() {
    let ci = ci_tested_targets();
    for target in &ci {
        assert!(
            target.windowing_supported,
            "CI-tested target {} should support windowing",
            target.name
        );
    }
}

// ── Target metadata ──────────────────────────────────────────────────

#[test]
fn all_targets_have_unique_triples() {
    let mut seen = std::collections::HashSet::new();
    for target in DESKTOP_TARGETS {
        assert!(
            seen.insert(target.rust_triple),
            "duplicate triple: {}",
            target.rust_triple
        );
    }
}

#[test]
fn all_targets_have_valid_rust_version() {
    for target in DESKTOP_TARGETS {
        let parts: Vec<&str> = target.min_rust_version.split('.').collect();
        assert_eq!(parts.len(), 3, "bad semver: {}", target.min_rust_version);
        for part in &parts {
            assert!(part.parse::<u32>().is_ok(), "non-numeric: {}", part);
        }
    }
}

#[test]
fn linux_triple_contains_linux() {
    for target in targets_for_platform(Platform::Linux) {
        assert!(
            target.rust_triple.contains("linux"),
            "{} triple should contain 'linux'",
            target.name
        );
    }
}

#[test]
fn macos_triple_contains_darwin() {
    for target in targets_for_platform(Platform::MacOS) {
        assert!(
            target.rust_triple.contains("darwin") || target.rust_triple.contains("apple"),
            "{} triple should contain 'darwin' or 'apple'",
            target.name
        );
    }
}

#[test]
fn windows_triple_contains_windows() {
    for target in targets_for_platform(Platform::Windows) {
        assert!(
            target.rust_triple.contains("windows"),
            "{} triple should contain 'windows'",
            target.name
        );
    }
}

// ── Current platform detection ───────────────────────────────────────

#[test]
fn current_target_is_detected() {
    assert!(
        current_target().is_some(),
        "must detect current build target"
    );
}

#[test]
fn current_target_validation_passes() {
    assert!(validate_current_target().is_ok());
}

#[test]
fn current_platform_is_desktop() {
    let target = current_target().unwrap();
    assert!(
        matches!(
            target.platform,
            Platform::Linux | Platform::MacOS | Platform::Windows
        ),
        "test host must be a desktop platform"
    );
}

#[test]
fn current_architecture_matches_target() {
    let target = current_target().unwrap();
    let arch = Architecture::current();
    assert_eq!(target.arch, arch);
}

// ── Capabilities on current platform ─────────────────────────────────

#[test]
fn current_platform_supports_filesystem() {
    assert!(gdplatform::platform_targets::supports_capability(
        PlatformCapability::FileSystem
    ));
}

#[test]
fn current_platform_supports_networking() {
    assert!(gdplatform::platform_targets::supports_capability(
        PlatformCapability::Networking
    ));
}

#[test]
fn current_platform_supports_threading() {
    assert!(gdplatform::platform_targets::supports_capability(
        PlatformCapability::Threading
    ));
}

#[test]
fn current_platform_supports_windowing() {
    assert!(gdplatform::platform_targets::supports_capability(
        PlatformCapability::Windowing
    ));
}

// ── Web target constraints ───────────────────────────────────────────

#[test]
fn web_target_has_limited_capabilities() {
    let web = targets_for_platform(Platform::Web);
    for target in &web {
        assert!(!target.gpu_supported, "Web should not claim GPU");
        assert!(
            !target.windowing_supported,
            "Web should not claim windowing"
        );
        assert!(!target.ci_tested, "Web is not yet CI-tested");
    }
}

// ── Phase 7 audit source of truth (pat-s3700) ──────────────────────

fn audit_path() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../prd/PHASE7_PLATFORM_PARITY_AUDIT.md")
}

fn read_audit() -> String {
    std::fs::read_to_string(audit_path()).expect("prd/PHASE7_PLATFORM_PARITY_AUDIT.md must exist")
}

#[test]
fn phase7_audit_doc_exists_and_is_cited() {
    let audit = read_audit();
    assert!(
        audit.contains("Phase 7 Platform Parity Audit"),
        "audit doc must have its title"
    );
    assert!(
        audit.contains("pat-s3700"),
        "audit doc must reference the CI matrix bead"
    );
}

#[test]
fn phase7_audit_documents_supported_targets() {
    let audit = read_audit();
    // The audit must mention each supported platform that appears in the
    // DESKTOP_TARGETS registry so a reader can trace coverage.
    for target in DESKTOP_TARGETS {
        assert!(
            audit.contains(target.platform_name()),
            "audit must mention platform '{}'",
            target.platform_name()
        );
    }
}

/// The CI-tested target set must match what the DESKTOP_TARGETS registry
/// declares.  This guards against silent target drift.
#[test]
fn ci_tested_targets_match_documented_matrix() {
    let ci = ci_tested_targets();

    // Expected CI-tested triples from the DESKTOP_TARGETS registry.
    let expected_ci_triples: Vec<&str> = DESKTOP_TARGETS
        .iter()
        .filter(|t| t.ci_tested)
        .map(|t| t.rust_triple)
        .collect();

    // Every registry-declared CI target must appear in the ci_tested set.
    for triple in &expected_ci_triples {
        assert!(
            ci.iter().any(|t| t.rust_triple == *triple),
            "ci_tested_targets() must include registry target '{triple}'"
        );
    }

    // And the sets must be the same size (no extras).
    assert_eq!(
        ci.len(),
        expected_ci_triples.len(),
        "ci_tested_targets() count ({}) must match registry count ({})",
        ci.len(),
        expected_ci_triples.len()
    );
}

/// The CI YAML must contain jobs for each documented domain lane so that
/// every Phase 7 validation area is actually exercised in the pipeline.
#[test]
fn ci_yaml_covers_documented_domain_lanes() {
    let yaml = read_ci_yaml();

    let expected_lanes = [
        ("headless runtime", "Headless runtime compat"),
        ("2D slice", "2D slice compat"),
        ("3D slice", "3D slice compat"),
        ("platform layer", "Platform layer compat"),
        ("fuzz / property", "Fuzz / property tests"),
        ("CI meta", "CI meta / infrastructure"),
        ("oracle parity", "Oracle parity check"),
    ];

    for (label, job_name) in &expected_lanes {
        assert!(
            yaml.contains(job_name),
            "CI must have a '{label}' lane (expected job name: '{job_name}')"
        );
    }
}

/// The CI YAML multi-platform matrix must include at least three OS
/// runners matching the three desktop platforms in DESKTOP_TARGETS.
#[test]
fn ci_yaml_matrix_matches_desktop_platforms() {
    let yaml = read_ci_yaml();

    // Map platform → expected CI runner.
    let platform_runners = [
        (Platform::Linux, "ubuntu-latest"),
        (Platform::MacOS, "macos-latest"),
        (Platform::Windows, "windows-latest"),
    ];

    for (platform, runner) in &platform_runners {
        let has_target = targets_for_platform(*platform).iter().any(|t| t.ci_tested);
        if has_target {
            assert!(
                yaml.contains(runner),
                "CI matrix must include runner '{runner}' for {platform:?}"
            );
        }
    }
}

/// The platform-layer compat job must use the same multi-OS matrix as
/// the main build job, ensuring platform tests run on all three OSes.
#[test]
fn ci_platform_compat_uses_multi_os_matrix() {
    let yaml = read_ci_yaml();

    // Find the platform layer compat section — it must reference the matrix.
    let platform_section = yaml
        .find("Platform layer compat")
        .expect("CI must have Platform layer compat job");
    let section_slice = &yaml[platform_section..];

    // It must contain a matrix with all three runners.
    assert!(
        section_slice.contains("ubuntu-latest"),
        "platform compat must test on Linux"
    );
    assert!(
        section_slice.contains("macos-latest"),
        "platform compat must test on macOS"
    );
    assert!(
        section_slice.contains("windows-latest"),
        "platform compat must test on Windows"
    );
}
