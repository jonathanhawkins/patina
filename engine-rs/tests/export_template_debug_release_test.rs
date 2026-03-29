//! pat-uueo0: Export template generation for debug and release.
//!
//! Integration tests covering:
//! 1. BuildProfile — default, variants, equality
//! 2. ExportConfig — builder pattern, defaults, resource accumulation
//! 3. ExportTemplate — from_config, generate_debug_and_release, output_filename, manifest
//! 4. PackageResult — success and error construction
//! 5. Cross-platform export workflows — all supported platforms
//! 6. Edge cases — empty resources, many resources, unknown platforms

use gdplatform::export::{BuildProfile, ExportConfig, ExportTemplate, PackageResult};

// ===========================================================================
// 1. BuildProfile
// ===========================================================================

#[test]
fn build_profile_default_is_release() {
    assert_eq!(BuildProfile::default(), BuildProfile::Release);
}

#[test]
fn build_profile_all_variants_distinct() {
    let profiles = [
        BuildProfile::Debug,
        BuildProfile::Release,
        BuildProfile::ReleaseDebug,
    ];
    for i in 0..profiles.len() {
        for j in (i + 1)..profiles.len() {
            assert_ne!(profiles[i], profiles[j]);
        }
    }
}

#[test]
fn build_profile_copy_and_clone() {
    let p = BuildProfile::Debug;
    let p2 = p; // Copy
    let p3 = p.clone();
    assert_eq!(p, p2);
    assert_eq!(p, p3);
}

// ===========================================================================
// 2. ExportConfig
// ===========================================================================

#[test]
fn export_config_new_defaults() {
    let cfg = ExportConfig::new("linux", "MyGame");
    assert_eq!(cfg.target_platform, "linux");
    assert_eq!(cfg.app_name, "MyGame");
    assert_eq!(cfg.build_profile, BuildProfile::Release);
    assert!(cfg.icon_path.is_empty());
    assert!(cfg.resources.is_empty());
}

#[test]
fn export_config_builder_chain() {
    let cfg = ExportConfig::new("windows", "Shooter")
        .with_build_profile(BuildProfile::Debug)
        .with_icon("icon.ico")
        .with_resource("scenes/")
        .with_resource("textures/")
        .with_resource("audio/");
    assert_eq!(cfg.build_profile, BuildProfile::Debug);
    assert_eq!(cfg.icon_path, "icon.ico");
    assert_eq!(cfg.resources, vec!["scenes/", "textures/", "audio/"]);
}

#[test]
fn export_config_with_resources_batch() {
    let cfg = ExportConfig::new("macos", "App").with_resources(["a/", "b/", "c/", "d/"]);
    assert_eq!(cfg.resources.len(), 4);
    assert_eq!(cfg.resources[0], "a/");
    assert_eq!(cfg.resources[3], "d/");
}

#[test]
fn export_config_mixed_resource_methods() {
    let cfg = ExportConfig::new("linux", "Game")
        .with_resource("single/")
        .with_resources(["batch1/", "batch2/"])
        .with_resource("another/");
    assert_eq!(
        cfg.resources,
        vec!["single/", "batch1/", "batch2/", "another/"]
    );
}

#[test]
fn export_config_clone() {
    let cfg = ExportConfig::new("web", "WebApp")
        .with_icon("favicon.png")
        .with_resource("res://");
    let cloned = cfg.clone();
    assert_eq!(cfg, cloned);
}

// ===========================================================================
// 3. ExportTemplate — from_config
// ===========================================================================

#[test]
fn template_from_config_preserves_config() {
    let cfg = ExportConfig::new("linux", "TestApp")
        .with_build_profile(BuildProfile::Debug)
        .with_icon("icon.png");
    let template = ExportTemplate::from_config(cfg.clone());
    assert_eq!(template.config, cfg);
}

#[test]
fn template_is_debug_for_debug_profile() {
    let t = ExportTemplate::from_config(
        ExportConfig::new("linux", "A").with_build_profile(BuildProfile::Debug),
    );
    assert!(t.is_debug());
    assert!(!t.is_release());
}

#[test]
fn template_is_release_for_release_profile() {
    let t = ExportTemplate::from_config(ExportConfig::new("linux", "A"));
    assert!(t.is_release());
    assert!(!t.is_debug());
}

#[test]
fn template_is_release_for_release_debug_profile() {
    let t = ExportTemplate::from_config(
        ExportConfig::new("linux", "A").with_build_profile(BuildProfile::ReleaseDebug),
    );
    assert!(t.is_release());
    assert!(!t.is_debug());
}

// ===========================================================================
// 3b. generate_debug_and_release
// ===========================================================================

#[test]
fn generate_debug_and_release_pair() {
    let base = ExportConfig::new("linux", "MyGame")
        .with_icon("icon.png")
        .with_resource("res://scenes");
    let (debug, release) = ExportTemplate::generate_debug_and_release(base);

    assert_eq!(debug.config.build_profile, BuildProfile::Debug);
    assert_eq!(release.config.build_profile, BuildProfile::Release);
    assert!(debug.is_debug());
    assert!(release.is_release());
}

#[test]
fn generate_pair_preserves_base_fields() {
    let base = ExportConfig::new("windows", "CoolGame")
        .with_icon("game.ico")
        .with_resources(["levels/", "sprites/"]);
    let (debug, release) = ExportTemplate::generate_debug_and_release(base);

    for t in [&debug, &release] {
        assert_eq!(t.config.app_name, "CoolGame");
        assert_eq!(t.config.target_platform, "windows");
        assert_eq!(t.config.icon_path, "game.ico");
        assert_eq!(t.config.resources, vec!["levels/", "sprites/"]);
    }
}

#[test]
fn generate_pair_overrides_existing_profile() {
    let base = ExportConfig::new("macos", "App").with_build_profile(BuildProfile::ReleaseDebug);
    let (debug, release) = ExportTemplate::generate_debug_and_release(base);
    assert_eq!(debug.config.build_profile, BuildProfile::Debug);
    assert_eq!(release.config.build_profile, BuildProfile::Release);
}

#[test]
fn generate_pair_manifests_differ() {
    let base = ExportConfig::new("linux", "TestApp");
    let (debug, release) = ExportTemplate::generate_debug_and_release(base);
    let dm = debug.generate_manifest();
    let rm = release.generate_manifest();
    assert_ne!(dm, rm);
    assert!(dm.contains("Debug"));
    assert!(rm.contains("Release"));
}

// ===========================================================================
// 3c. output_filename
// ===========================================================================

#[test]
fn output_filename_linux_debug() {
    let t = ExportTemplate::from_config(
        ExportConfig::new("linux", "MyGame").with_build_profile(BuildProfile::Debug),
    );
    assert_eq!(t.output_filename(), "MyGame.linux.debug.x86_64");
}

#[test]
fn output_filename_linux_release() {
    let t = ExportTemplate::from_config(ExportConfig::new("linux", "MyGame"));
    assert_eq!(t.output_filename(), "MyGame.linux.release.x86_64");
}

#[test]
fn output_filename_windows_release() {
    let t = ExportTemplate::from_config(ExportConfig::new("windows", "CoolGame"));
    assert_eq!(t.output_filename(), "CoolGame.windows.release.exe");
}

#[test]
fn output_filename_windows_debug() {
    let t = ExportTemplate::from_config(
        ExportConfig::new("windows", "CoolGame").with_build_profile(BuildProfile::Debug),
    );
    assert_eq!(t.output_filename(), "CoolGame.windows.debug.exe");
}

#[test]
fn output_filename_macos_release_debug() {
    let t = ExportTemplate::from_config(
        ExportConfig::new("macos", "MacApp").with_build_profile(BuildProfile::ReleaseDebug),
    );
    assert_eq!(t.output_filename(), "MacApp.macos.release_debug.app");
}

#[test]
fn output_filename_web_release() {
    let t = ExportTemplate::from_config(ExportConfig::new("web", "WebGame"));
    assert_eq!(t.output_filename(), "WebGame.web.release.wasm");
}

#[test]
fn output_filename_unknown_platform_no_extension() {
    let t = ExportTemplate::from_config(ExportConfig::new("android", "MobileGame"));
    assert_eq!(t.output_filename(), "MobileGame.android.release");
}

#[test]
fn output_filename_debug_release_pair() {
    let base = ExportConfig::new("windows", "Shooter");
    let (debug, release) = ExportTemplate::generate_debug_and_release(base);
    assert_eq!(debug.output_filename(), "Shooter.windows.debug.exe");
    assert_eq!(release.output_filename(), "Shooter.windows.release.exe");
}

// ===========================================================================
// 3d. Manifest generation
// ===========================================================================

#[test]
fn manifest_contains_app_name_and_platform() {
    let cfg = ExportConfig::new("linux", "TestApp")
        .with_icon("icon.png")
        .with_resource("res://scenes")
        .with_resource("res://textures");
    let manifest = ExportTemplate::from_config(cfg).generate_manifest();
    assert!(manifest.contains("TestApp"));
    assert!(manifest.contains("linux"));
    assert!(manifest.contains("Release"));
    assert!(manifest.contains("icon.png"));
    assert!(manifest.contains("res://scenes"));
    assert!(manifest.contains("res://textures"));
}

#[test]
fn manifest_omits_icon_when_empty() {
    let cfg = ExportConfig::new("web", "WebGame");
    let manifest = ExportTemplate::from_config(cfg).generate_manifest();
    assert!(!manifest.contains("icon:"));
}

#[test]
fn manifest_empty_resources_section() {
    let cfg = ExportConfig::new("linux", "Bare");
    let manifest = ExportTemplate::from_config(cfg).generate_manifest();
    assert!(manifest.contains("resources:"));
    // No resource entries after "resources:"
    let after_resources = manifest.split("resources:").nth(1).unwrap();
    assert!(after_resources.trim().is_empty());
}

// ===========================================================================
// 4. PackageResult
// ===========================================================================

#[test]
fn package_result_ok_fields() {
    let result = PackageResult::ok("build/game.zip", 1024 * 1024);
    assert!(result.success);
    assert_eq!(result.output_path, "build/game.zip");
    assert_eq!(result.size_bytes, 1024 * 1024);
    assert!(result.messages.is_empty());
}

#[test]
fn package_result_err_fields() {
    let result = PackageResult::err("missing resource: textures/hero.png");
    assert!(!result.success);
    assert!(result.output_path.is_empty());
    assert_eq!(result.size_bytes, 0);
    assert_eq!(result.messages.len(), 1);
    assert!(result.messages[0].contains("missing resource"));
}

#[test]
fn package_result_ok_zero_size() {
    let result = PackageResult::ok("empty.pck", 0);
    assert!(result.success);
    assert_eq!(result.size_bytes, 0);
}

#[test]
fn package_result_clone_equality() {
    let r1 = PackageResult::ok("out.pck", 500);
    let r2 = r1.clone();
    assert_eq!(r1, r2);
}

// ===========================================================================
// 5. Cross-platform export workflows
// ===========================================================================

#[test]
fn multi_platform_export_workflow() {
    let platforms = ["linux", "windows", "macos", "web"];
    let expected_exts = [".x86_64", ".exe", ".app", ".wasm"];

    for (platform, ext) in platforms.iter().zip(expected_exts.iter()) {
        let base = ExportConfig::new(*platform, "CrossPlatGame").with_resource("res://");
        let (debug, release) = ExportTemplate::generate_debug_and_release(base);

        assert!(debug.output_filename().ends_with(ext));
        assert!(release.output_filename().ends_with(ext));
        assert!(debug.output_filename().contains("debug"));
        assert!(release.output_filename().contains("release"));
        assert_eq!(debug.config.target_platform, *platform);
        assert_eq!(release.config.target_platform, *platform);
    }
}

#[test]
fn full_export_workflow_linux() {
    // Configure
    let cfg = ExportConfig::new("linux", "PatinaGame")
        .with_icon("res://icon.png")
        .with_resources(["res://scenes", "res://scripts", "res://assets"]);

    // Generate debug and release templates
    let (debug, release) = ExportTemplate::generate_debug_and_release(cfg);

    // Verify filenames
    assert_eq!(debug.output_filename(), "PatinaGame.linux.debug.x86_64");
    assert_eq!(release.output_filename(), "PatinaGame.linux.release.x86_64");

    // Verify manifests
    let debug_manifest = debug.generate_manifest();
    assert!(debug_manifest.contains("Debug"));
    assert!(debug_manifest.contains("res://scenes"));
    assert!(debug_manifest.contains("res://scripts"));
    assert!(debug_manifest.contains("res://assets"));

    let release_manifest = release.generate_manifest();
    assert!(release_manifest.contains("Release"));

    // Simulate packaging results
    let debug_result = PackageResult::ok("build/PatinaGame.linux.debug.x86_64", 50_000_000);
    let release_result = PackageResult::ok("build/PatinaGame.linux.release.x86_64", 30_000_000);
    assert!(debug_result.success);
    assert!(release_result.success);
    assert!(debug_result.size_bytes > release_result.size_bytes);
}

#[test]
fn full_export_workflow_windows() {
    let cfg = ExportConfig::new("windows", "PatinaGame")
        .with_icon("res://icon.ico")
        .with_resource("res://");

    let (debug, release) = ExportTemplate::generate_debug_and_release(cfg);

    assert_eq!(debug.output_filename(), "PatinaGame.windows.debug.exe");
    assert_eq!(release.output_filename(), "PatinaGame.windows.release.exe");

    assert!(debug.is_debug());
    assert!(release.is_release());
}

// ===========================================================================
// 6. Edge cases
// ===========================================================================

#[test]
fn export_config_no_resources() {
    let cfg = ExportConfig::new("linux", "Minimal");
    let template = ExportTemplate::from_config(cfg);
    assert_eq!(template.config.resources.len(), 0);
    let manifest = template.generate_manifest();
    assert!(manifest.contains("resources:"));
}

#[test]
fn export_config_many_resources() {
    let resources: Vec<String> = (0..100).map(|i| format!("res://dir_{i}")).collect();
    let cfg = ExportConfig::new("linux", "Big").with_resources(resources.clone());
    assert_eq!(cfg.resources.len(), 100);

    let template = ExportTemplate::from_config(cfg);
    let manifest = template.generate_manifest();
    for r in &resources {
        assert!(manifest.contains(r.as_str()));
    }
}

#[test]
fn export_config_empty_strings() {
    let cfg = ExportConfig::new("", "");
    assert_eq!(cfg.target_platform, "");
    assert_eq!(cfg.app_name, "");
    let template = ExportTemplate::from_config(cfg);
    // output_filename still produces something
    assert_eq!(template.output_filename(), "..release");
}

#[test]
fn package_result_err_empty_message() {
    let result = PackageResult::err("");
    assert!(!result.success);
    assert_eq!(result.messages, vec![""]);
}
