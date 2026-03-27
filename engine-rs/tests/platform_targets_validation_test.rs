//! pat-oa8z: Validate desktop platform target definitions and coverage.
//!
//! These integration tests verify that:
//! 1. The platform target registry is complete and consistent
//! 2. The current build matches a known target
//! 3. CI coverage spans the required platforms
//! 4. Platform capabilities are correctly reported
//! 5. Export configs can be created for all desktop targets
//! 6. Architecture detection is correct for the current host

use gdplatform::os::{current_platform, OsInfo, Platform};
use gdplatform::platform_targets::{
    ci_tested_targets, current_target, supports_capability, targets_for_platform,
    validate_current_target, Architecture, PlatformCapability, DESKTOP_TARGETS,
};
use gdplatform::export::ExportConfig;

// ===========================================================================
// 1. Target registry completeness
// ===========================================================================

#[test]
fn registry_covers_three_desktop_oses() {
    let platforms: Vec<Platform> = DESKTOP_TARGETS.iter().map(|t| t.platform).collect();
    assert!(platforms.contains(&Platform::Linux), "must include Linux");
    assert!(platforms.contains(&Platform::MacOS), "must include macOS");
    assert!(platforms.contains(&Platform::Windows), "must include Windows");
}

#[test]
fn registry_covers_both_major_architectures() {
    let arches: Vec<Architecture> = DESKTOP_TARGETS.iter().map(|t| t.arch).collect();
    assert!(arches.contains(&Architecture::X86_64), "must include x86_64");
    assert!(arches.contains(&Architecture::Aarch64), "must include aarch64");
}

#[test]
fn registry_has_at_least_five_targets() {
    assert!(
        DESKTOP_TARGETS.len() >= 5,
        "must define at least 5 targets (got {})",
        DESKTOP_TARGETS.len()
    );
}

#[test]
fn all_triples_contain_architecture() {
    for target in DESKTOP_TARGETS {
        let arch_str = target.arch.triple_component();
        assert!(
            target.rust_triple.starts_with(arch_str)
                || target.rust_triple.contains(arch_str),
            "triple '{}' must contain architecture component '{}'",
            target.rust_triple,
            arch_str
        );
    }
}

#[test]
fn all_triples_are_plausible_rust_triples() {
    for target in DESKTOP_TARGETS {
        let parts: Vec<&str> = target.rust_triple.split('-').collect();
        assert!(
            parts.len() >= 3,
            "rust_triple '{}' must have at least 3 components (arch-vendor-os)",
            target.rust_triple
        );
    }
}

// ===========================================================================
// 2. Current build validation
// ===========================================================================

#[test]
fn current_build_matches_known_target() {
    let target = current_target();
    assert!(
        target.is_some(),
        "current build ({:?}/{:?}) must match a known desktop target",
        current_platform(),
        Architecture::current()
    );
}

#[test]
fn current_target_platform_matches_os() {
    let target = current_target().unwrap();
    let os = current_platform();
    assert_eq!(
        target.platform, os,
        "current_target().platform must match current_platform()"
    );
}

#[test]
fn current_target_arch_matches_build() {
    let target = current_target().unwrap();
    let arch = Architecture::current();
    assert_eq!(target.arch, arch);
}

#[test]
fn validate_current_target_returns_ok() {
    let result = validate_current_target();
    assert!(result.is_ok(), "validate failed: {:?}", result.err());
}

#[test]
fn os_info_name_matches_target_platform() {
    let info = OsInfo::detect();
    let target = current_target().unwrap();
    match target.platform {
        Platform::Linux => assert_eq!(info.name, "Linux"),
        Platform::MacOS => assert_eq!(info.name, "macOS"),
        Platform::Windows => assert_eq!(info.name, "Windows"),
        _ => {}
    }
}

// ===========================================================================
// 3. CI coverage validation
// ===========================================================================

#[test]
fn ci_tested_includes_at_least_three_targets() {
    let ci = ci_tested_targets();
    assert!(
        ci.len() >= 3,
        "at least 3 targets must be CI-tested (got {})",
        ci.len()
    );
}

#[test]
fn ci_tested_covers_all_three_desktop_oses() {
    let ci = ci_tested_targets();
    let ci_platforms: Vec<Platform> = ci.iter().map(|t| t.platform).collect();
    assert!(ci_platforms.contains(&Platform::Linux), "CI must test Linux");
    assert!(ci_platforms.contains(&Platform::MacOS), "CI must test macOS");
    assert!(ci_platforms.contains(&Platform::Windows), "CI must test Windows");
}

#[test]
fn ci_tested_includes_x86_64() {
    let ci = ci_tested_targets();
    assert!(
        ci.iter().any(|t| t.arch == Architecture::X86_64),
        "CI must test at least one x86_64 target"
    );
}

// ===========================================================================
// 4. Platform capability validation
// ===========================================================================

#[test]
fn desktop_supports_filesystem() {
    assert!(supports_capability(PlatformCapability::FileSystem));
}

#[test]
fn desktop_supports_networking() {
    assert!(supports_capability(PlatformCapability::Networking));
}

#[test]
fn desktop_supports_threading() {
    // All native desktop targets support threading.
    let target = current_target().unwrap();
    if target.platform != Platform::Web {
        assert!(supports_capability(PlatformCapability::Threading));
    }
}

#[test]
fn desktop_supports_windowing() {
    let target = current_target().unwrap();
    if target.platform != Platform::Web {
        assert!(supports_capability(PlatformCapability::Windowing));
    }
}

#[test]
fn desktop_supports_gpu_rendering() {
    let target = current_target().unwrap();
    if target.platform != Platform::Web {
        assert!(supports_capability(PlatformCapability::GpuRendering));
    }
}

#[test]
fn desktop_supports_gamepad() {
    let target = current_target().unwrap();
    if target.platform != Platform::Web {
        assert!(supports_capability(PlatformCapability::GamepadInput));
    }
}

#[test]
fn desktop_supports_audio() {
    let target = current_target().unwrap();
    if target.platform != Platform::Web {
        assert!(supports_capability(PlatformCapability::Audio));
    }
}

// ===========================================================================
// 5. Export config integration with targets
// ===========================================================================

#[test]
fn export_config_can_be_created_for_all_targets() {
    for target in DESKTOP_TARGETS {
        let config = ExportConfig::new(target.rust_triple, "TestApp");
        assert_eq!(config.target_platform, target.rust_triple);
        assert_eq!(config.app_name, "TestApp");
    }
}

#[test]
fn export_config_current_target() {
    let target = current_target().unwrap();
    let config = ExportConfig::new(target.rust_triple, "PatinaGame")
        .with_resource("res://")
        .with_icon("icon.png");
    assert_eq!(config.target_platform, target.rust_triple);
    assert_eq!(config.icon_path, "icon.png");
    assert_eq!(config.resources.len(), 1);
}

// ===========================================================================
// 6. Architecture detection
// ===========================================================================

#[test]
fn architecture_current_matches_cfg() {
    let arch = Architecture::current();
    if cfg!(target_arch = "x86_64") {
        assert_eq!(arch, Architecture::X86_64);
    } else if cfg!(target_arch = "aarch64") {
        assert_eq!(arch, Architecture::Aarch64);
    } else if cfg!(target_arch = "x86") {
        assert_eq!(arch, Architecture::X86);
    } else if cfg!(target_arch = "wasm32") {
        assert_eq!(arch, Architecture::Wasm32);
    }
}

#[test]
fn architecture_triple_roundtrip() {
    for target in DESKTOP_TARGETS {
        let component = target.arch.triple_component();
        assert!(
            target.rust_triple.contains(component),
            "triple '{}' must contain arch component '{}'",
            target.rust_triple,
            component
        );
    }
}

// ===========================================================================
// 7. Per-platform target queries
// ===========================================================================

#[test]
fn linux_has_two_arch_targets() {
    let linux = targets_for_platform(Platform::Linux);
    assert_eq!(linux.len(), 2, "Linux should have x86_64 and aarch64");
    let arches: Vec<Architecture> = linux.iter().map(|t| t.arch).collect();
    assert!(arches.contains(&Architecture::X86_64));
    assert!(arches.contains(&Architecture::Aarch64));
}

#[test]
fn macos_has_two_arch_targets() {
    let macos = targets_for_platform(Platform::MacOS);
    assert_eq!(macos.len(), 2, "macOS should have x86_64 and aarch64");
}

#[test]
fn windows_has_at_least_one_target() {
    let windows = targets_for_platform(Platform::Windows);
    assert!(!windows.is_empty());
}

#[test]
fn web_targets_are_wasm() {
    let web = targets_for_platform(Platform::Web);
    for t in &web {
        assert_eq!(t.arch, Architecture::Wasm32);
    }
}

#[test]
fn unknown_platform_has_no_targets() {
    let unknown = targets_for_platform(Platform::Unknown);
    assert!(unknown.is_empty(), "Unknown platform should have no targets");
}

// ===========================================================================
// 8. Target property consistency
// ===========================================================================

#[test]
fn all_desktop_targets_gpu_except_web() {
    for target in DESKTOP_TARGETS {
        if target.platform != Platform::Web {
            assert!(
                target.gpu_supported,
                "desktop target {} must support GPU",
                target.name
            );
        }
    }
}

#[test]
fn all_desktop_targets_windowing_except_web() {
    for target in DESKTOP_TARGETS {
        if target.platform != Platform::Web {
            assert!(
                target.windowing_supported,
                "desktop target {} must support windowing",
                target.name
            );
        }
    }
}

#[test]
fn min_rust_version_is_at_least_1_75() {
    for target in DESKTOP_TARGETS {
        let parts: Vec<u32> = target
            .min_rust_version
            .split('.')
            .map(|s| s.parse().unwrap())
            .collect();
        let (major, minor) = (parts[0], parts[1]);
        assert!(
            major >= 1 && minor >= 75,
            "target {} min_rust_version {} is below 1.75.0",
            target.name,
            target.min_rust_version
        );
    }
}

// ===========================================================================
// 9. CI workflow matrix matches platform targets (pat-ev5tn)
// ===========================================================================

/// Validates that the CI workflow YAML matrix includes runners for all
/// platforms marked `ci_tested: true` in DESKTOP_TARGETS.
#[test]
fn ci_workflow_matrix_covers_all_ci_tested_platforms() {
    let ci_yml_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../.github/workflows/ci.yml"
    );
    let ci_yml = std::fs::read_to_string(ci_yml_path)
        .expect("should read .github/workflows/ci.yml");

    // Map platform → expected GitHub Actions runner substring.
    let platform_to_runner: Vec<(Platform, &str)> = vec![
        (Platform::Linux, "ubuntu"),
        (Platform::MacOS, "macos"),
        (Platform::Windows, "windows"),
    ];

    let ci_tested = ci_tested_targets();
    let ci_platforms: std::collections::HashSet<Platform> =
        ci_tested.iter().map(|t| t.platform).collect();

    for (platform, runner_substr) in &platform_to_runner {
        if ci_platforms.contains(platform) {
            assert!(
                ci_yml.contains(runner_substr),
                "CI workflow must include a '{}' runner for {:?} (marked ci_tested in DESKTOP_TARGETS)",
                runner_substr,
                platform
            );
        }
    }
}

/// Validates that the CI matrix uses at least 3 OS runners (Linux, macOS, Windows).
#[test]
fn ci_workflow_has_three_os_matrix() {
    let ci_yml_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../.github/workflows/ci.yml"
    );
    let ci_yml = std::fs::read_to_string(ci_yml_path)
        .expect("should read .github/workflows/ci.yml");

    assert!(ci_yml.contains("ubuntu-latest"), "CI must test on Ubuntu");
    assert!(ci_yml.contains("macos-latest"), "CI must test on macOS");
    assert!(ci_yml.contains("windows-latest"), "CI must test on Windows");
}

/// Validates there are no platforms marked ci_tested that lack a runner.
#[test]
fn no_ci_tested_platform_without_runner() {
    let ci_yml_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../.github/workflows/ci.yml"
    );
    let ci_yml = std::fs::read_to_string(ci_yml_path)
        .expect("should read .github/workflows/ci.yml");

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
            ci_yml.contains(runner),
            "Target '{}' is ci_tested=true but CI workflow has no '{}' runner",
            target.name,
            runner
        );
    }
}
