//! pat-6covi / pat-9byz5 / pat-iarrt: Integration tests for macOS `.app`
//! bundle, Windows `.exe` + manifest, and Linux AppImage package
//! generation.
//!
//! Exercises `PackageExecutor::generate_macos_bundle`,
//! `PackageExecutor::generate_windows_exe`,
//! `PackageExecutor::generate_linux_appimage`, and the full
//! `run_with_binary` pipeline on `macos`, `windows`, and `linux`
//! targets. Verifies layout, manifest contents, resource placement,
//! icon copy, and the platform mismatch error paths.

use std::fs;
use std::path::Path;

use gdplatform::export::{
    BuildProfile, ExportConfig, PackageError, PackageExecutor,
};
use gdplatform::linux::{
    appdir_name, appimage_filename, desktop_filename, linux_icon_filename,
    sanitize_desktop_id,
};
use tempfile::TempDir;

fn seed_project(dir: &Path) {
    fs::create_dir_all(dir.join("scenes")).unwrap();
    fs::create_dir_all(dir.join("textures")).unwrap();
    fs::write(dir.join("scenes/main.tscn"), b"[node name=\"Main\"]\n").unwrap();
    fs::write(dir.join("scenes/level1.tscn"), b"[node name=\"Level1\"]\n").unwrap();
    fs::write(dir.join("textures/hero.png"), b"\x89PNG\r\n\x1a\nFAKE").unwrap();
    fs::write(dir.join("icon.icns"), b"icns\x00\x00\x00\x08FAKE").unwrap();
    fs::write(dir.join("icon.ico"), b"\x00\x00\x01\x00ICO_FAKE").unwrap();
    fs::write(dir.join("icon.png"), b"ICONDATA").unwrap();
}

fn macos_config(app: &str) -> ExportConfig {
    ExportConfig::new("macos", app)
        .with_icon("res://icon.icns")
        .with_resource("res://scenes")
        .with_resource("res://textures")
}

fn windows_config(app: &str) -> ExportConfig {
    ExportConfig::new("windows", app)
        .with_icon("res://icon.ico")
        .with_resource("res://scenes")
        .with_resource("res://textures")
}

fn linux_config(app: &str) -> ExportConfig {
    ExportConfig::new("linux", app)
        .with_icon("res://icon.png")
        .with_resource("res://scenes")
        .with_resource("res://textures")
}

/// Asserts that a file has at least the owner-execute bit set on unix.
/// On non-unix targets this is a no-op, so the suite remains portable.
#[cfg(unix)]
fn assert_mode_executable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let mode = fs::metadata(path)
        .unwrap_or_else(|e| panic!("metadata for {}: {e}", path.display()))
        .permissions()
        .mode();
    assert!(
        mode & 0o111 != 0,
        "{} should be executable (mode={:o})",
        path.display(),
        mode
    );
}

#[cfg(not(unix))]
fn assert_mode_executable(_path: &Path) {}

// ---------------------------------------------------------------------------
// Bundle layout — full run() pipeline
// ---------------------------------------------------------------------------

#[test]
fn macos_bundle_run_produces_valid_layout() {
    let tmp = TempDir::new().unwrap();
    let project = tmp.path().join("project");
    let output = tmp.path().join("export");
    seed_project(&project);

    let mut executor = PackageExecutor::new(macos_config("PatinaGame"), &project, &output);
    let result = executor.run();

    assert!(result.success, "run() failed: {:?}", result.messages);

    let bundle = output.join("PatinaGame.app");
    assert!(bundle.is_dir(), "bundle dir missing: {}", bundle.display());

    let contents = bundle.join("Contents");
    assert!(contents.join("Info.plist").is_file());
    assert!(contents.join("PkgInfo").is_file());
    assert!(contents.join("MacOS/PatinaGame").is_file());
    assert!(contents.join("Resources").is_dir());

    assert_eq!(
        fs::read_to_string(contents.join("PkgInfo")).unwrap(),
        "APPL????"
    );
}

#[test]
fn macos_bundle_info_plist_contains_required_keys() {
    let tmp = TempDir::new().unwrap();
    let project = tmp.path().join("project");
    let output = tmp.path().join("export");
    seed_project(&project);

    let mut executor = PackageExecutor::new(macos_config("Patina Demo"), &project, &output);
    assert!(executor.run().success);

    let plist = fs::read_to_string(
        output.join("Patina Demo.app/Contents/Info.plist"),
    )
    .unwrap();

    assert!(plist.contains("<key>CFBundleExecutable</key>"));
    assert!(plist.contains("<string>Patina Demo</string>"));
    assert!(plist.contains("<key>CFBundleIdentifier</key>"));
    assert!(plist.contains("<string>com.patina.patina-demo</string>"));
    assert!(plist.contains("<key>CFBundlePackageType</key>"));
    assert!(plist.contains("<string>APPL</string>"));
    assert!(plist.contains("<key>LSMinimumSystemVersion</key>"));
    assert!(plist.contains("<key>NSHighResolutionCapable</key>"));
    assert!(plist.contains("<true/>"));
    assert!(plist.contains("<key>CFBundleIconFile</key>"));
    assert!(plist.contains("<string>icon.icns</string>"));
}

#[test]
fn macos_bundle_copies_collected_resources() {
    let tmp = TempDir::new().unwrap();
    let project = tmp.path().join("project");
    let output = tmp.path().join("export");
    seed_project(&project);

    let mut executor = PackageExecutor::new(macos_config("ResGame"), &project, &output);
    assert!(executor.run().success);

    let resources = output.join("ResGame.app/Contents/Resources");
    assert!(resources.join("scenes/main.tscn").is_file());
    assert!(resources.join("scenes/level1.tscn").is_file());
    assert!(resources.join("textures/hero.png").is_file());

    let main = fs::read_to_string(resources.join("scenes/main.tscn")).unwrap();
    assert!(main.contains("Main"));
}

#[test]
fn macos_bundle_copies_icon_when_configured() {
    let tmp = TempDir::new().unwrap();
    let project = tmp.path().join("project");
    let output = tmp.path().join("export");
    seed_project(&project);

    let mut executor = PackageExecutor::new(macos_config("IconGame"), &project, &output);
    assert!(executor.run().success);

    let icon = output.join("IconGame.app/Contents/Resources/icon.icns");
    assert!(icon.is_file(), "icon missing: {}", icon.display());
    let bytes = fs::read(&icon).unwrap();
    assert!(bytes.starts_with(b"icns"));
}

#[test]
fn macos_bundle_omits_icon_key_when_unset() {
    let tmp = TempDir::new().unwrap();
    let project = tmp.path().join("project");
    let output = tmp.path().join("export");
    fs::create_dir_all(&project).unwrap();

    let cfg = ExportConfig::new("macos", "BareApp");
    let mut executor = PackageExecutor::new(cfg, &project, &output);
    assert!(executor.run().success);

    let plist = fs::read_to_string(
        output.join("BareApp.app/Contents/Info.plist"),
    )
    .unwrap();
    assert!(plist.contains("<key>CFBundleIconFile</key>"));
    assert!(plist.contains("<string></string>"));

    let icon = output.join("BareApp.app/Contents/Resources/icon.icns");
    assert!(!icon.exists());
}

#[test]
fn macos_bundle_placeholder_binary_when_no_source() {
    let tmp = TempDir::new().unwrap();
    let project = tmp.path().join("project");
    let output = tmp.path().join("export");
    seed_project(&project);

    let mut executor = PackageExecutor::new(macos_config("PlaceholderGame"), &project, &output);
    assert!(executor.run().success);

    let binary = output.join("PlaceholderGame.app/Contents/MacOS/PlaceholderGame");
    let contents = fs::read_to_string(&binary).unwrap();
    assert!(contents.contains("Patina export placeholder"));
    assert!(contents.contains("PlaceholderGame"));
}

#[test]
fn macos_bundle_embeds_real_binary_via_run_with_binary() {
    let tmp = TempDir::new().unwrap();
    let project = tmp.path().join("project");
    let output = tmp.path().join("export");
    seed_project(&project);

    let fake_binary = tmp.path().join("real_binary");
    fs::write(&fake_binary, b"ELF_FAKE_BINARY_CONTENTS").unwrap();

    let mut executor = PackageExecutor::new(macos_config("RealBinGame"), &project, &output);
    let result = executor.run_with_binary(Some(&fake_binary));
    assert!(result.success, "run_with_binary failed: {:?}", result.messages);

    let embedded = output.join("RealBinGame.app/Contents/MacOS/RealBinGame");
    assert_eq!(fs::read(&embedded).unwrap(), b"ELF_FAKE_BINARY_CONTENTS");
}

#[test]
fn macos_bundle_honors_debug_profile_in_manifest() {
    let tmp = TempDir::new().unwrap();
    let project = tmp.path().join("project");
    let output = tmp.path().join("export");
    seed_project(&project);

    let cfg = macos_config("DbgGame").with_build_profile(BuildProfile::Debug);
    let mut executor = PackageExecutor::new(cfg, &project, &output);
    assert!(executor.run().success);

    let manifest = fs::read_to_string(output.join("export_manifest.txt")).unwrap();
    assert!(manifest.contains("Debug"));
    assert!(output.join("DbgGame.app").is_dir());
}

#[test]
fn macos_bundle_result_points_at_bundle_path() {
    let tmp = TempDir::new().unwrap();
    let project = tmp.path().join("project");
    let output = tmp.path().join("export");
    seed_project(&project);

    let mut executor = PackageExecutor::new(macos_config("PathGame"), &project, &output);
    let result = executor.run();
    assert!(result.success);
    assert!(result.output_path.ends_with("PathGame.app"));
}

// ---------------------------------------------------------------------------
// Error paths
// ---------------------------------------------------------------------------

#[test]
fn macos_bundle_rejects_non_macos_platform() {
    let tmp = TempDir::new().unwrap();
    let project = tmp.path().join("project");
    let output = tmp.path().join("export");
    seed_project(&project);

    let cfg = ExportConfig::new("linux", "LinuxGame").with_resource("res://scenes");
    let mut executor = PackageExecutor::new(cfg, &project, &output);
    // Prime the collected list so the guard is the only reason this fails.
    executor.validate_and_collect().unwrap();

    let err = executor.generate_macos_bundle(None).unwrap_err();
    match err {
        PackageError::BundleFailed(msg) => {
            assert!(msg.contains("macOS bundle requires"));
            assert!(msg.contains("linux"));
        }
        other => panic!("expected BundleFailed, got {other:?}"),
    }
}

#[test]
fn macos_bundle_non_macos_run_skips_bundle_generation() {
    let tmp = TempDir::new().unwrap();
    let project = tmp.path().join("project");
    let output = tmp.path().join("export");
    seed_project(&project);

    let cfg = ExportConfig::new("linux", "LinuxGame").with_resource("res://scenes");
    let mut executor = PackageExecutor::new(cfg, &project, &output);
    assert!(executor.run().success);

    // No .app directory for non-macOS targets.
    assert!(!output.join("LinuxGame.app").exists());
    // Staging output still written.
    assert!(output.join("export_manifest.txt").is_file());
}

// ---------------------------------------------------------------------------
// pat-9byz5: Windows .exe + side-by-side manifest — full run() pipeline
// ---------------------------------------------------------------------------

#[test]
fn windows_exe_run_produces_valid_layout() {
    let tmp = TempDir::new().unwrap();
    let project = tmp.path().join("project");
    let output = tmp.path().join("export");
    seed_project(&project);

    let mut executor = PackageExecutor::new(windows_config("PatinaGame"), &project, &output);
    let result = executor.run();

    assert!(result.success, "run() failed: {:?}", result.messages);

    assert!(output.join("PatinaGame.exe").is_file());
    assert!(output.join("PatinaGame.exe.manifest").is_file());
    assert!(output.join("resources").is_dir());
    assert!(output.join("export_manifest.txt").is_file());
}

#[test]
fn windows_exe_manifest_contains_required_sections() {
    let tmp = TempDir::new().unwrap();
    let project = tmp.path().join("project");
    let output = tmp.path().join("export");
    seed_project(&project);

    let mut executor = PackageExecutor::new(windows_config("PatinaGame"), &project, &output);
    assert!(executor.run().success);

    let manifest = fs::read_to_string(output.join("PatinaGame.exe.manifest")).unwrap();

    assert!(manifest.contains("<?xml"));
    assert!(manifest.contains("urn:schemas-microsoft-com:asm.v1"));
    assert!(manifest.contains("<assemblyIdentity"));
    assert!(manifest.contains("type=\"win32\""));
    assert!(manifest.contains("Microsoft.Windows.Common-Controls"));
    assert!(manifest.contains("6595b64144ccf1df"));
    assert!(manifest.contains("asInvoker"));
    assert!(manifest.contains("PerMonitorV2"));
    assert!(manifest.contains("longPathAware"));
}

#[test]
fn windows_exe_manifest_identity_sanitized() {
    let tmp = TempDir::new().unwrap();
    let project = tmp.path().join("project");
    let output = tmp.path().join("export");
    seed_project(&project);

    let mut executor = PackageExecutor::new(windows_config("Patina Demo"), &project, &output);
    assert!(executor.run().success);

    let manifest = fs::read_to_string(output.join("Patina Demo.exe.manifest")).unwrap();
    assert!(manifest.contains("com.patina.patina-demo"));
}

#[test]
fn windows_exe_copies_collected_resources() {
    let tmp = TempDir::new().unwrap();
    let project = tmp.path().join("project");
    let output = tmp.path().join("export");
    seed_project(&project);

    let mut executor = PackageExecutor::new(windows_config("ResGame"), &project, &output);
    assert!(executor.run().success);

    let resources = output.join("resources");
    assert!(resources.join("scenes/main.tscn").is_file());
    assert!(resources.join("scenes/level1.tscn").is_file());
    assert!(resources.join("textures/hero.png").is_file());

    let main = fs::read_to_string(resources.join("scenes/main.tscn")).unwrap();
    assert!(main.contains("Main"));
}

#[test]
fn windows_exe_copies_icon_when_configured() {
    let tmp = TempDir::new().unwrap();
    let project = tmp.path().join("project");
    let output = tmp.path().join("export");
    seed_project(&project);

    let mut executor = PackageExecutor::new(windows_config("IconGame"), &project, &output);
    assert!(executor.run().success);

    let icon = output.join("IconGame.ico");
    assert!(icon.is_file(), "icon missing: {}", icon.display());
    let bytes = fs::read(&icon).unwrap();
    assert!(bytes.starts_with(b"\x00\x00\x01\x00"));
}

#[test]
fn windows_exe_placeholder_exe_when_no_source() {
    let tmp = TempDir::new().unwrap();
    let project = tmp.path().join("project");
    let output = tmp.path().join("export");
    seed_project(&project);

    let mut executor = PackageExecutor::new(windows_config("PlaceholderGame"), &project, &output);
    assert!(executor.run().success);

    let exe = output.join("PlaceholderGame.exe");
    let contents = fs::read_to_string(&exe).unwrap();
    assert!(contents.starts_with("MZ"));
    assert!(contents.contains("Patina export placeholder"));
    assert!(contents.contains("PlaceholderGame"));
}

#[test]
fn windows_exe_embeds_real_binary_via_run_with_binary() {
    let tmp = TempDir::new().unwrap();
    let project = tmp.path().join("project");
    let output = tmp.path().join("export");
    seed_project(&project);

    let fake_binary = tmp.path().join("real_binary.exe");
    fs::write(&fake_binary, b"MZ_FAKE_PE_BINARY_CONTENTS").unwrap();

    let mut executor = PackageExecutor::new(windows_config("RealBinGame"), &project, &output);
    let result = executor.run_with_binary(Some(&fake_binary));
    assert!(result.success, "run_with_binary failed: {:?}", result.messages);

    let embedded = output.join("RealBinGame.exe");
    assert_eq!(fs::read(&embedded).unwrap(), b"MZ_FAKE_PE_BINARY_CONTENTS");
}

#[test]
fn windows_exe_honors_debug_profile_in_manifest() {
    let tmp = TempDir::new().unwrap();
    let project = tmp.path().join("project");
    let output = tmp.path().join("export");
    seed_project(&project);

    let cfg = windows_config("DbgGame").with_build_profile(BuildProfile::Debug);
    let mut executor = PackageExecutor::new(cfg, &project, &output);
    assert!(executor.run().success);

    let manifest = fs::read_to_string(output.join("export_manifest.txt")).unwrap();
    assert!(manifest.contains("Debug"));
    assert!(output.join("DbgGame.exe").is_file());
}

#[test]
fn windows_exe_rejects_non_windows_platform() {
    let tmp = TempDir::new().unwrap();
    let project = tmp.path().join("project");
    let output = tmp.path().join("export");
    seed_project(&project);

    let cfg = ExportConfig::new("linux", "LinuxGame").with_resource("res://scenes");
    let mut executor = PackageExecutor::new(cfg, &project, &output);
    executor.validate_and_collect().unwrap();

    let err = executor.generate_windows_exe(None).unwrap_err();
    match err {
        PackageError::BundleFailed(msg) => {
            assert!(msg.contains("Windows exe requires"));
            assert!(msg.contains("linux"));
        }
        other => panic!("expected BundleFailed, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// pat-iarrt: Linux AppImage — full run() pipeline
// ---------------------------------------------------------------------------

#[test]
fn linux_appimage() {
    let tmp = TempDir::new().unwrap();
    let project = tmp.path().join("project");
    let output = tmp.path().join("export");
    seed_project(&project);

    let app_name = "LinuxGame";
    let mut executor = PackageExecutor::new(linux_config(app_name), &project, &output);
    let result = executor.run();

    assert!(
        result.success,
        "linux packaging failed: {:?}",
        result.messages
    );

    // Output path is the final .AppImage marker.
    let appimage_path = Path::new(&result.output_path).to_path_buf();
    let expected_appimage_name = appimage_filename(app_name);
    assert_eq!(
        appimage_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(""),
        expected_appimage_name.as_str(),
        "output should be {}.AppImage",
        app_name
    );
    assert!(
        appimage_path.exists(),
        "AppImage marker missing: {}",
        appimage_path.display()
    );
    assert!(
        appimage_path.starts_with(&output),
        "AppImage must live under output dir"
    );

    // AppDir layout sits next to the AppImage marker.
    let appdir = output.join(appdir_name(app_name));
    assert!(appdir.is_dir(), "AppDir missing: {}", appdir.display());

    // AppRun script is required and must be executable on unix.
    let apprun = appdir.join("AppRun");
    assert!(apprun.exists(), "AppRun missing: {}", apprun.display());
    let apprun_text = fs::read_to_string(&apprun).unwrap();
    assert!(
        apprun_text.starts_with("#!/bin/sh"),
        "AppRun must be a shell script"
    );
    assert!(
        apprun_text.contains(&format!("usr/bin/{}", sanitize_desktop_id(app_name))),
        "AppRun must exec the sanitized binary"
    );
    assert_mode_executable(&apprun);

    // .desktop entry exists at AppDir root and under usr/share/applications/.
    let desktop_name = desktop_filename(app_name);
    let desktop_root = appdir.join(&desktop_name);
    let desktop_share = appdir
        .join("usr")
        .join("share")
        .join("applications")
        .join(&desktop_name);
    assert!(desktop_root.exists(), "root .desktop missing");
    assert!(desktop_share.exists(), "usr/share .desktop missing");
    let desktop_text = fs::read_to_string(&desktop_root).unwrap();
    assert!(
        desktop_text.contains("[Desktop Entry]"),
        ".desktop must declare a Desktop Entry section"
    );
    assert!(
        desktop_text.contains(&format!("Name={}", app_name)),
        ".desktop must carry the app name"
    );

    // Icon copied to both the AppDir root and usr/share/icons/hicolor/....
    let icon_name = linux_icon_filename(app_name);
    let icon_root = appdir.join(&icon_name);
    let icon_share = appdir
        .join("usr")
        .join("share")
        .join("icons")
        .join("hicolor")
        .join("256x256")
        .join("apps")
        .join(&icon_name);
    assert!(
        icon_root.exists(),
        "root icon missing: {}",
        icon_root.display()
    );
    assert!(
        icon_share.exists(),
        "hicolor icon missing: {}",
        icon_share.display()
    );
    assert_eq!(fs::read(&icon_root).unwrap(), b"ICONDATA");
    assert_eq!(fs::read(&icon_share).unwrap(), b"ICONDATA");

    // Binary staged under usr/bin/<sanitized> and executable on unix.
    let binary = appdir
        .join("usr")
        .join("bin")
        .join(sanitize_desktop_id(app_name));
    assert!(binary.exists(), "binary missing: {}", binary.display());
    assert_mode_executable(&binary);

    // Collected resources live under usr/share/<app_name>/.
    let usr_share_app = appdir.join("usr").join("share").join(app_name);
    assert!(
        usr_share_app.join("scenes").join("main.tscn").exists(),
        "scenes/main.tscn not staged"
    );
    assert!(
        usr_share_app.join("scenes").join("level1.tscn").exists(),
        "scenes/level1.tscn not staged"
    );
    assert!(
        usr_share_app.join("textures").join("hero.png").exists(),
        "textures/hero.png not staged"
    );

    // Staging pipeline also writes the shared manifest + listing.
    assert!(output.join("export_manifest.txt").exists());
    assert!(output.join("resource_list.txt").exists());

    // AppImage marker is an executable shell stub pointing at AppRun.
    let marker_text = fs::read_to_string(&appimage_path).unwrap();
    assert!(marker_text.starts_with("#!/bin/sh"));
    assert!(marker_text.contains("AppRun"));
    assert_mode_executable(&appimage_path);
}

#[test]
fn linux_appimage_rejects_non_linux_platform() {
    let tmp = TempDir::new().unwrap();
    let project = tmp.path().join("project");
    let output = tmp.path().join("export");
    seed_project(&project);

    let cfg = ExportConfig::new("macos", "MacGame").with_resource("res://scenes");
    let mut executor = PackageExecutor::new(cfg, &project, &output);
    executor.validate_and_collect().unwrap();

    let err = executor.generate_linux_appimage(None).unwrap_err();
    match err {
        PackageError::BundleFailed(msg) => {
            assert!(msg.contains("Linux AppImage requires"));
            assert!(msg.contains("macos"));
        }
        other => panic!("expected BundleFailed, got {other:?}"),
    }
}
