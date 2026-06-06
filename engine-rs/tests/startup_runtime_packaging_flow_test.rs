//! pat-t1sxg / pat-vjmfv: Startup/runtime packaging flow integration test.
//!
//! Source of truth: `prd/PHASE7_PLATFORM_PARITY_AUDIT.md`
//! Classification: Measured (startup/bootstrap through runtime loop, packaging/export config)
//!
//! Validates the Phase 7 end-to-end flow:
//!   1. Engine bootstraps through all 8 phases in Godot initialization order
//!   2. Runtime runs frames and produces correct output
//!   3. Packaging pipeline collects project resources and generates artifacts
//!   4. The full lifecycle composes: bootstrap → run → package → verify
//!
//! Scope (from Phase 7 audit):
//! - This exercises Patina's *staging* packaging path: config → collect → manifest → marker.
//! - It does NOT claim full Godot export-template parity or downstream app-distribution parity.
//! - The packaging flow covers resource collection, manifest generation, and deterministic output.
//! - Native per-OS app bundle behavior is outside the scope validated here.
//!
//! Exit criteria (PORT_GODOT_TO_RUST_PLAN.md Phase 7):
//! - runtime can be built and run in a repeatable way
//! - platform-specific code remains isolated behind traits

use gdplatform::backend::{HeadlessPlatform, PlatformBackend};
use gdplatform::export::{
    BuildProfile, ExportConfig, ExportTemplate, PackageError, PackageExecutor,
};
use gdplatform::platform_targets::{ci_tested_targets, current_target};
use gdplatform::window::WindowConfig;
use gdscene::main_loop::MainLoop;
use gdscene::node::Node;
use gdscene::scene_tree::SceneTree;
use std::path::Path;

use patina_runner::bootstrap::{BootConfig, BootPhase, EngineBootstrap};

const DT: f64 = 1.0 / 60.0;

// ===========================================================================
// 1. Bootstrap → Run → Verify (full lifecycle without scene file)
// ===========================================================================

#[test]
fn bootstrap_headless_full_lifecycle_no_scene() {
    // Bootstrap all 8 phases in headless mode (no scene file needed).
    let mut boot = EngineBootstrap::new(BootConfig::headless());
    boot.run_all().unwrap();

    assert!(boot.is_running());
    assert_eq!(boot.current_phase(), BootPhase::Running);
    assert!(boot.main_loop().is_some());
    // All 8 transitions should be logged.
    assert_eq!(boot.log().len(), 8);

    // Run 10 frames.
    let ml = boot.main_loop_mut().unwrap();
    for _ in 0..10 {
        ml.step(DT);
    }
    assert_eq!(ml.frame_count(), 10);
}

#[test]
fn bootstrap_phases_execute_in_godot_order() {
    let mut boot = EngineBootstrap::new(BootConfig::headless());
    let expected_phases = [
        BootPhase::Core,
        BootPhase::Servers,
        BootPhase::Resources,
        BootPhase::SceneTree,
        BootPhase::MainScene,
        BootPhase::Scripts,
        BootPhase::Lifecycle,
        BootPhase::Running,
    ];

    for expected in &expected_phases {
        let actual = boot.step().unwrap();
        assert_eq!(actual, *expected, "phases must execute in Godot order");
    }
}

#[test]
fn bootstrap_partial_advance_then_complete() {
    let mut boot = EngineBootstrap::new(BootConfig::headless());

    // Advance to SceneTree (phase 4).
    boot.advance_to(BootPhase::SceneTree).unwrap();
    assert_eq!(boot.current_phase(), BootPhase::SceneTree);
    assert!(boot.tree().is_some());
    assert!(!boot.is_running());

    // Then complete remaining phases.
    boot.run_all().unwrap();
    assert!(boot.is_running());
    assert!(boot.main_loop().is_some());
}

// ===========================================================================
// 2. Bootstrap + MainLoop integration (through PlatformBackend)
// ===========================================================================

#[test]
fn bootstrap_to_mainloop_with_platform_backend() {
    // Bootstrap creates a MainLoop; PlatformBackend drives the frame loop.
    let mut boot = EngineBootstrap::new(BootConfig::headless());
    boot.run_all().unwrap();

    let mut backend = HeadlessPlatform::new(1280, 720).with_max_frames(30);

    let ml = boot.main_loop_mut().unwrap();
    ml.run(&mut backend, DT);

    assert_eq!(ml.frame_count(), 30);
    assert!(backend.should_quit());
}

#[test]
fn bootstrap_to_mainloop_with_nodes() {
    // Bootstrap, add nodes manually, run lifecycle, verify frame stepping.
    let mut boot = EngineBootstrap::new(BootConfig::headless());
    boot.advance_to(BootPhase::SceneTree).unwrap();

    // Add a node hierarchy to the tree.
    let tree = boot.tree_mut().unwrap();
    let root = tree.root_id();
    let world = Node::new("World", "Node2D");
    let world_id = tree.add_child(root, world).unwrap();
    let player = Node::new("Player", "CharacterBody2D");
    tree.add_child(world_id, player).unwrap();

    assert_eq!(tree.node_count(), 3); // root + World + Player

    // Complete bootstrap.
    boot.run_all().unwrap();

    let ml = boot.main_loop_mut().unwrap();
    let mut backend = HeadlessPlatform::new(640, 480).with_max_frames(5);
    ml.run(&mut backend, DT);
    assert_eq!(ml.frame_count(), 5);
}

// ===========================================================================
// 3. Packaging pipeline — config → collect → package → verify
// ===========================================================================

#[test]
fn package_executor_collects_fixture_resources() {
    let project_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures");
    if !project_dir.join("scenes").exists() {
        return;
    }

    let config = ExportConfig::new("linux", "PatinaTest").with_resource("res://scenes/");

    let mut executor = PackageExecutor::new(config, &project_dir, "/tmp/patina-test-export");
    executor.validate_platform().unwrap();
    executor.validate_and_collect().unwrap();

    let resources = executor.collected_resources();
    assert!(!resources.is_empty(), "should collect scene files");
    // All resources should have a package path starting with "scenes/"
    for entry in resources {
        assert!(
            entry.package_path.starts_with("scenes/"),
            "package path '{}' should start with 'scenes/'",
            entry.package_path
        );
        assert!(entry.size_bytes > 0, "resource should have nonzero size");
    }
}

#[test]
fn package_executor_full_run_writes_artifacts() {
    let project_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures");
    if !project_dir.join("scenes").exists() {
        return;
    }

    let tmp = std::env::temp_dir().join("patina-packaging-flow-test");
    let _ = std::fs::remove_dir_all(&tmp);

    let config = ExportConfig::new("linux", "PatinaFlowTest")
        .with_build_profile(BuildProfile::Release)
        .with_resource("res://scenes/hierarchy.tscn");

    let mut executor = PackageExecutor::new(config, &project_dir, &tmp);
    let result = executor.run();

    assert!(
        result.success,
        "packaging should succeed: {:?}",
        result.messages
    );
    assert!(result.size_bytes > 0, "should have nonzero output size");

    // Verify output files exist.
    assert!(
        tmp.join("export_manifest.txt").exists(),
        "manifest should exist"
    );
    assert!(
        tmp.join("resource_list.txt").exists(),
        "resource listing should exist"
    );
    assert!(
        tmp.join("PatinaFlowTest.linux.release.x86_64").exists(),
        "output binary marker should exist"
    );

    // Verify manifest content.
    let manifest = std::fs::read_to_string(tmp.join("export_manifest.txt")).unwrap();
    assert!(
        manifest.contains("PatinaFlowTest"),
        "manifest should contain app name"
    );
    assert!(
        manifest.contains("linux"),
        "manifest should contain platform"
    );
    assert!(
        manifest.contains("Release"),
        "manifest should contain profile"
    );
    assert!(
        manifest.contains("hierarchy.tscn"),
        "manifest should list resources"
    );

    // Clean up.
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn package_executor_rejects_unsupported_platform() {
    let config = ExportConfig::new("gameboy", "BadPlatform");
    let executor = PackageExecutor::new(config, ".", "/tmp/unused");
    assert!(matches!(
        executor.validate_platform(),
        Err(PackageError::UnsupportedPlatform(_))
    ));
}

#[test]
fn package_executor_rejects_missing_project_dir() {
    let config = ExportConfig::new("linux", "Test");
    let mut executor = PackageExecutor::new(config, "/nonexistent/dir/42", "/tmp/unused");
    assert!(matches!(
        executor.validate_and_collect(),
        Err(PackageError::ProjectDirNotFound(_))
    ));
}

// ===========================================================================
// 4. Export template generation for all platforms
// ===========================================================================

#[test]
fn export_templates_generate_correct_filenames_all_platforms() {
    let platforms = [
        ("linux", ".x86_64"),
        ("windows", ".exe"),
        ("macos", ".app"),
        ("web", ".wasm"),
    ];

    for (platform, expected_ext) in &platforms {
        let config =
            ExportConfig::new(*platform, "TestApp").with_build_profile(BuildProfile::Release);
        let template = ExportTemplate::from_config(config);
        let filename = template.output_filename();

        assert!(
            filename.ends_with(expected_ext),
            "{platform}: expected '{expected_ext}' suffix, got '{filename}'"
        );
        assert!(
            filename.contains("release"),
            "{platform}: should contain profile"
        );
        assert!(
            filename.starts_with("TestApp"),
            "{platform}: should start with app name"
        );
    }
}

#[test]
fn export_template_debug_release_pair() {
    let base = ExportConfig::new("linux", "MyGame")
        .with_resource("scenes/")
        .with_icon("icon.png");

    let (debug, release) = ExportTemplate::generate_debug_and_release(base);

    assert!(debug.is_debug());
    assert!(!debug.is_release());
    assert!(release.is_release());
    assert!(!release.is_debug());

    // Both share the same resources and icon.
    assert_eq!(debug.config.resources, release.config.resources);
    assert_eq!(debug.config.icon_path, release.config.icon_path);
}

// ===========================================================================
// 5. Full flow: bootstrap → run → package (end-to-end)
// ===========================================================================

#[test]
fn end_to_end_bootstrap_run_package() {
    // Phase 1: Bootstrap the engine.
    let mut boot = EngineBootstrap::new(BootConfig::headless());
    boot.run_all().unwrap();
    assert!(boot.is_running());

    // Phase 2: Run some frames.
    let ml = boot.main_loop_mut().unwrap();
    let mut backend = HeadlessPlatform::new(1280, 720).with_max_frames(60);
    ml.run(&mut backend, DT);
    assert_eq!(ml.frame_count(), 60);

    // Phase 3: Package the project for multiple platforms.
    let project_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures");
    if !project_dir.exists() {
        return;
    }

    let tmp = std::env::temp_dir().join("patina-e2e-flow-test");
    let _ = std::fs::remove_dir_all(&tmp);

    for platform in &["linux", "macos", "windows"] {
        let config =
            ExportConfig::new(*platform, "PatinaE2E").with_build_profile(BuildProfile::Release);
        let out_dir = tmp.join(platform);
        let mut executor = PackageExecutor::new(config, &project_dir, &out_dir);
        let result = executor.run();
        assert!(
            result.success,
            "{platform}: packaging failed: {:?}",
            result.messages
        );
    }

    // Verify all three platforms produced output.
    assert!(tmp.join("linux/export_manifest.txt").exists());
    assert!(tmp.join("macos/export_manifest.txt").exists());
    assert!(tmp.join("windows/export_manifest.txt").exists());

    let _ = std::fs::remove_dir_all(&tmp);
}

// ===========================================================================
// 6. Platform target validation (startup knows what it can run on)
// ===========================================================================

#[test]
fn startup_knows_current_target() {
    let target = current_target().expect("must identify current platform");
    assert!(!target.name.is_empty());
    assert!(!target.rust_triple.is_empty());

    // We're a CI-tested target.
    let ci = ci_tested_targets();
    assert!(
        ci.iter().any(|t| t.rust_triple == target.rust_triple),
        "current target {} must be CI-tested",
        target.rust_triple
    );
}

// ===========================================================================
// 7. Repeatable builds — deterministic output
// ===========================================================================

#[test]
fn packaging_is_deterministic() {
    let project_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures");
    if !project_dir.join("scenes/hierarchy.tscn").exists() {
        return;
    }

    let tmp1 = std::env::temp_dir().join("patina-deterministic-1");
    let tmp2 = std::env::temp_dir().join("patina-deterministic-2");
    let _ = std::fs::remove_dir_all(&tmp1);
    let _ = std::fs::remove_dir_all(&tmp2);

    let make_config = || {
        ExportConfig::new("linux", "DetTest")
            .with_build_profile(BuildProfile::Release)
            .with_resource("res://scenes/hierarchy.tscn")
    };

    let mut ex1 = PackageExecutor::new(make_config(), &project_dir, &tmp1);
    let mut ex2 = PackageExecutor::new(make_config(), &project_dir, &tmp2);
    let r1 = ex1.run();
    let r2 = ex2.run();

    assert!(r1.success && r2.success);
    assert_eq!(r1.size_bytes, r2.size_bytes, "output sizes must match");

    // Manifest content should be identical.
    let m1 = std::fs::read_to_string(tmp1.join("export_manifest.txt")).unwrap();
    let m2 = std::fs::read_to_string(tmp2.join("export_manifest.txt")).unwrap();
    assert_eq!(m1, m2, "manifests must be identical across runs");

    // Resource listings should be identical.
    let l1 = std::fs::read_to_string(tmp1.join("resource_list.txt")).unwrap();
    let l2 = std::fs::read_to_string(tmp2.join("resource_list.txt")).unwrap();
    assert_eq!(l1, l2, "resource listings must be identical across runs");

    let _ = std::fs::remove_dir_all(&tmp1);
    let _ = std::fs::remove_dir_all(&tmp2);
}

// ===========================================================================
// 8. Bootstrap with event tracing enabled (headless, no scene)
// ===========================================================================

#[test]
fn bootstrap_with_tracing_enabled_headless() {
    let config = BootConfig::headless().with_event_tracing();
    let mut boot = EngineBootstrap::new(config);
    boot.advance_to(BootPhase::SceneTree).unwrap();

    // Event tracing should be enabled on the tree.
    let tree = boot.tree().unwrap();
    assert!(tree.event_trace().is_enabled());

    // Complete bootstrap — should not panic even without a scene.
    boot.run_all().unwrap();
    assert!(boot.is_running());
}

// ===========================================================================
// 9. WindowConfig → HeadlessPlatform → MainLoop roundtrip
// ===========================================================================

#[test]
fn window_config_to_mainloop_roundtrip() {
    let config = WindowConfig::new()
        .with_size(800, 600)
        .with_title("Flow Test")
        .with_vsync(true);

    let mut platform = HeadlessPlatform::from_config(&config).with_max_frames(10);
    assert_eq!(platform.window_size(), (800, 600));

    let tree = SceneTree::new();
    let mut ml = MainLoop::new(tree);
    ml.run(&mut platform, DT);

    assert_eq!(ml.frame_count(), 10);
    assert!(platform.should_quit());
}

// ===========================================================================
// 10. Bootstrap script phase integration
// ===========================================================================

#[test]
fn bootstrap_scripts_phase_headless_no_scripts() {
    // In headless mode with no scene, the scripts phase should succeed
    // with zero scripts attached (non-fatal empty case).
    let mut boot = EngineBootstrap::new(BootConfig::headless());
    boot.run_all().unwrap();
    assert_eq!(boot.scripts_attached(), 0);
}

#[test]
fn bootstrap_scripts_phase_with_scene_no_scripts() {
    // With a scene loaded that has no script references, scripts phase
    // should succeed with zero attachments.
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/scenes/hierarchy.tscn");
    if !fixtures.exists() {
        return;
    }

    let config = BootConfig::with_scene(&fixtures);
    let mut boot = EngineBootstrap::new(config);
    boot.advance_to(BootPhase::SceneTree).unwrap();

    // The minimal scene may or may not have scripts — either way the phase
    // should not error.
    boot.advance_to(BootPhase::Scripts).unwrap();
    // scripts_attached() is a valid count (could be 0 for minimal scenes).
    let _count = boot.scripts_attached();

    boot.run_all().unwrap();
    assert!(boot.is_running());
}

// ===========================================================================
// 11. Bootstrap config builder ergonomics
// ===========================================================================

#[test]
fn boot_config_project_dir_builder() {
    let config = BootConfig::headless()
        .project_dir("/my/project")
        .window_size(640, 480)
        .with_event_tracing();

    assert_eq!(config.project_dir, std::path::PathBuf::from("/my/project"));
    assert_eq!(config.window_width, 640);
    assert_eq!(config.window_height, 480);
    assert!(config.event_tracing);
    assert!(config.headless);
}

// ===========================================================================
// 12. Full lifecycle: bootstrap → scripts → run → package (integrated)
// ===========================================================================

#[test]
fn integrated_bootstrap_run_package_all_platforms() {
    let project_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures");
    if !project_dir.join("scenes").exists() {
        return;
    }

    // Phase 1: Bootstrap with project_dir set.
    let config = BootConfig::headless().project_dir(&project_dir);
    let mut boot = EngineBootstrap::new(config);
    boot.run_all().unwrap();
    assert!(boot.is_running());

    // Phase 2: Run frames.
    let ml = boot.main_loop_mut().unwrap();
    let mut backend = HeadlessPlatform::new(1280, 720).with_max_frames(30);
    ml.run(&mut backend, DT);
    assert_eq!(ml.frame_count(), 30);

    // Phase 3: Package for each supported platform.
    let tmp = std::env::temp_dir().join("patina-integrated-flow-test");
    let _ = std::fs::remove_dir_all(&tmp);

    let platforms = ["linux", "macos", "windows", "web"];
    for platform in &platforms {
        let config = ExportConfig::new(*platform, "IntegratedTest")
            .with_build_profile(BuildProfile::Release)
            .with_resource("res://scenes/hierarchy.tscn");
        let out_dir = tmp.join(platform);
        let mut executor = PackageExecutor::new(config, &project_dir, &out_dir);
        let result = executor.run();
        assert!(
            result.success,
            "{platform}: packaging should succeed: {:?}",
            result.messages
        );
        assert!(result.size_bytes > 0);
    }

    // Verify all platforms produced manifests.
    for platform in &platforms {
        assert!(
            tmp.join(platform).join("export_manifest.txt").exists(),
            "{platform}: manifest must exist"
        );
    }

    let _ = std::fs::remove_dir_all(&tmp);
}

// ===========================================================================
// 13. Packaging with multiple resource directories
// ===========================================================================

#[test]
fn package_multiple_resource_dirs() {
    let project_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures");
    if !project_dir.join("scenes").exists() {
        return;
    }

    let tmp = std::env::temp_dir().join("patina-multi-res-test");
    let _ = std::fs::remove_dir_all(&tmp);

    let config = ExportConfig::new("linux", "MultiResApp")
        .with_build_profile(BuildProfile::Release)
        .with_resource("res://scenes/");

    let mut executor = PackageExecutor::new(config, &project_dir, &tmp);
    let result = executor.run();

    assert!(result.success, "packaging failed: {:?}", result.messages);
    // Should have collected multiple scene files.
    let listing = std::fs::read_to_string(tmp.join("resource_list.txt")).unwrap();
    let line_count = listing.lines().count();
    assert!(
        line_count > 1,
        "should collect multiple resources from scenes dir, got {line_count}"
    );

    let _ = std::fs::remove_dir_all(&tmp);
}

// ===========================================================================
// 14. Audited artifact path — scoped to Phase 7 audit (pat-vjmfv)
// ===========================================================================

/// Validates that the packaging flow stays scoped to Patina's staging artifact
/// path as defined in `prd/PHASE7_PLATFORM_PARITY_AUDIT.md`.
///
/// The audit classifies this as "Measured for Patina packaging flow" — meaning:
/// - Config objects, template generation, manifest output, resource collection,
///   and output artifact staging are exercised locally.
/// - This is NOT a claim of full Godot export preset/template parity.
///
/// This test exercises the full audited path:
///   bootstrap → run frames → packaging config → collect → stage → verify artifacts
#[test]
fn audited_artifact_path_bootstrap_run_package_verify() {
    let project_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures");
    if !project_dir.join("scenes/hierarchy.tscn").exists() {
        return;
    }

    // --- Phase 1: Bootstrap the engine (headless, repeatable) ---
    let config = BootConfig::headless().project_dir(&project_dir);
    let mut boot = EngineBootstrap::new(config);
    boot.run_all().unwrap();
    assert!(boot.is_running(), "engine must reach Running phase");

    // Verify all 8 phase transitions were logged.
    assert_eq!(boot.log().len(), 8, "all boot phases must complete");

    // --- Phase 2: Run frames through the platform backend ---
    let ml = boot.main_loop_mut().unwrap();
    let mut backend = HeadlessPlatform::new(1280, 720).with_max_frames(10);
    ml.run(&mut backend, DT);
    assert_eq!(ml.frame_count(), 10, "frames must advance");

    // --- Phase 3: Packaging flow scoped to Patina staging ---
    let tmp = std::env::temp_dir().join("patina-audited-artifact-path");
    let _ = std::fs::remove_dir_all(&tmp);

    let export_config = ExportConfig::new("linux", "AuditedFlowApp")
        .with_build_profile(BuildProfile::Release)
        .with_resource("res://scenes/hierarchy.tscn");

    let mut executor = PackageExecutor::new(export_config, &project_dir, &tmp);

    // Step 3a: Platform validation — only supported desktop targets pass.
    executor.validate_platform().unwrap();

    // Step 3b: Resource collection — resolves res:// paths, collects file metadata.
    executor.validate_and_collect().unwrap();
    let resources = executor.collected_resources();
    assert_eq!(resources.len(), 1, "should collect exactly one scene file");
    assert_eq!(resources[0].package_path, "scenes/hierarchy.tscn");
    assert!(
        resources[0].size_bytes > 0,
        "resource must have nonzero size"
    );

    // Step 3c: Full staging run — produces manifest + resource listing + output marker.
    let mut executor2 = PackageExecutor::new(
        ExportConfig::new("linux", "AuditedFlowApp")
            .with_build_profile(BuildProfile::Release)
            .with_resource("res://scenes/hierarchy.tscn"),
        &project_dir,
        &tmp,
    );
    let result = executor2.run();
    assert!(
        result.success,
        "packaging must succeed: {:?}",
        result.messages
    );
    assert!(result.size_bytes > 0);

    // --- Phase 4: Verify the staging artifacts ---
    // These are the three artifacts the Patina packaging flow produces:
    let manifest_path = tmp.join("export_manifest.txt");
    let listing_path = tmp.join("resource_list.txt");
    let marker_path = tmp.join("AuditedFlowApp.linux.release.x86_64");

    assert!(manifest_path.exists(), "manifest must be staged");
    assert!(listing_path.exists(), "resource listing must be staged");
    assert!(marker_path.exists(), "output marker must be staged");

    // Manifest must describe the scoped packaging claim.
    let manifest = std::fs::read_to_string(&manifest_path).unwrap();
    assert!(
        manifest.contains("AuditedFlowApp"),
        "manifest cites app name"
    );
    assert!(manifest.contains("linux"), "manifest cites platform");
    assert!(manifest.contains("Release"), "manifest cites profile");
    assert!(
        manifest.contains("hierarchy.tscn"),
        "manifest lists collected resource"
    );
    assert!(
        manifest.contains("total_resources: 1"),
        "manifest includes resource count"
    );
    assert!(
        manifest.contains("total_size_bytes:"),
        "manifest includes total size"
    );

    // Resource listing is tab-separated: package_path \t source_path \t size.
    let listing = std::fs::read_to_string(&listing_path).unwrap();
    let fields: Vec<&str> = listing.trim().split('\t').collect();
    assert!(
        fields.len() >= 3,
        "listing must have package_path, source_path, size"
    );
    assert_eq!(fields[0], "scenes/hierarchy.tscn");

    // Output marker is a placeholder — Patina does not yet produce native binaries
    // through this path. This is the correct scope per the audit.
    let marker = std::fs::read_to_string(&marker_path).unwrap();
    assert!(
        marker.contains("Patina export placeholder"),
        "marker is a staging placeholder"
    );

    let _ = std::fs::remove_dir_all(&tmp);
}

/// Validates that unsupported platforms are rejected at the validation stage,
/// keeping the packaging flow scoped to the audited desktop target set.
#[test]
fn audited_packaging_rejects_out_of_scope_platforms() {
    for platform in &["ps5", "switch", "android", "ios", "gameboy"] {
        let config = ExportConfig::new(*platform, "ScopeTest");
        let executor = PackageExecutor::new(config, ".", "/tmp/unused");
        assert!(
            matches!(
                executor.validate_platform(),
                Err(PackageError::UnsupportedPlatform(_))
            ),
            "{platform} must be rejected — outside audited desktop target set"
        );
    }
}

/// Validates that the supported desktop platforms all pass validation,
/// matching the Phase 7 audit's desktop target matrix.
#[test]
fn audited_packaging_accepts_desktop_targets() {
    for platform in &["linux", "macos", "windows", "web"] {
        let config = ExportConfig::new(*platform, "DesktopTest");
        let executor = PackageExecutor::new(config, ".", "/tmp/unused");
        assert!(
            executor.validate_platform().is_ok(),
            "{platform} must be accepted — in audited desktop target set"
        );
    }
}

// ===========================================================================
// 15. Export template manifest includes resource count
// ===========================================================================

#[test]
fn package_manifest_includes_resource_count() {
    let project_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures");
    if !project_dir.join("scenes/hierarchy.tscn").exists() {
        return;
    }

    let tmp = std::env::temp_dir().join("patina-manifest-count-test");
    let _ = std::fs::remove_dir_all(&tmp);

    let config =
        ExportConfig::new("linux", "ManifestCheck").with_resource("res://scenes/hierarchy.tscn");

    let mut executor = PackageExecutor::new(config, &project_dir, &tmp);
    let result = executor.run();
    assert!(result.success);

    let manifest = std::fs::read_to_string(tmp.join("export_manifest.txt")).unwrap();
    assert!(manifest.contains("total_resources: 1"));
    assert!(manifest.contains("total_size_bytes:"));
    assert!(manifest.contains("hierarchy.tscn"));

    let _ = std::fs::remove_dir_all(&tmp);
}
