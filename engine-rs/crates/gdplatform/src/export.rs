//! Export and packaging configuration.
//!
//! Defines structures for configuring how a Patina project is exported
//! to a target platform, mirroring Godot's export template system.
//!
//! The [`PackageExecutor`] drives the packaging pipeline: it validates
//! the export configuration, collects resources from the project tree,
//! writes them into a staging directory alongside a manifest, and
//! produces a [`PackageResult`] describing the output.

use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// BuildProfile
// ---------------------------------------------------------------------------

/// Build optimization profile for exports.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BuildProfile {
    /// Unoptimized with debug symbols and assertions.
    Debug,
    /// Fully optimized release build.
    #[default]
    Release,
    /// Optimized but with debug symbols retained.
    ReleaseDebug,
}

// ---------------------------------------------------------------------------
// ExportConfig
// ---------------------------------------------------------------------------

/// Configuration for exporting a project to a target platform.
#[derive(Debug, Clone, PartialEq)]
pub struct ExportConfig {
    /// Target platform identifier (e.g. "linux", "windows", "macos", "web").
    pub target_platform: String,
    /// Build optimization profile.
    pub build_profile: BuildProfile,
    /// Application display name.
    pub app_name: String,
    /// Path to the application icon (may be empty).
    pub icon_path: String,
    /// Resource files/directories to include in the export.
    pub resources: Vec<String>,
}

impl ExportConfig {
    /// Creates a new export config with required fields.
    pub fn new(target_platform: impl Into<String>, app_name: impl Into<String>) -> Self {
        Self {
            target_platform: target_platform.into(),
            build_profile: BuildProfile::default(),
            app_name: app_name.into(),
            icon_path: String::new(),
            resources: Vec::new(),
        }
    }

    /// Builder: sets the build profile.
    pub fn with_build_profile(mut self, profile: BuildProfile) -> Self {
        self.build_profile = profile;
        self
    }

    /// Builder: sets the icon path.
    pub fn with_icon(mut self, path: impl Into<String>) -> Self {
        self.icon_path = path.into();
        self
    }

    /// Builder: adds a resource path.
    pub fn with_resource(mut self, path: impl Into<String>) -> Self {
        self.resources.push(path.into());
        self
    }

    /// Builder: adds multiple resource paths.
    pub fn with_resources(mut self, paths: impl IntoIterator<Item = impl Into<String>>) -> Self {
        for p in paths {
            self.resources.push(p.into());
        }
        self
    }
}

// ---------------------------------------------------------------------------
// ExportTemplate
// ---------------------------------------------------------------------------

/// An export template describes the files that make up an export.
#[derive(Debug, Clone, PartialEq)]
pub struct ExportTemplate {
    /// The config this template was generated from.
    pub config: ExportConfig,
}

impl ExportTemplate {
    /// Creates a template from an export config.
    pub fn from_config(config: ExportConfig) -> Self {
        Self { config }
    }

    /// Returns `true` if this template uses a debug build profile.
    pub fn is_debug(&self) -> bool {
        self.config.build_profile == BuildProfile::Debug
    }

    /// Returns `true` if this template uses a release or release-debug build profile.
    pub fn is_release(&self) -> bool {
        matches!(
            self.config.build_profile,
            BuildProfile::Release | BuildProfile::ReleaseDebug
        )
    }

    /// Generates a debug/release template pair from a base config.
    pub fn generate_debug_and_release(base: ExportConfig) -> (Self, Self) {
        let mut debug_config = base.clone();
        debug_config.build_profile = BuildProfile::Debug;
        let mut release_config = base;
        release_config.build_profile = BuildProfile::Release;
        (
            Self {
                config: debug_config,
            },
            Self {
                config: release_config,
            },
        )
    }

    /// Returns the output filename including platform and build profile
    /// (e.g. "MyGame.linux.release.x86_64", "CoolGame.windows.debug.exe").
    pub fn output_filename(&self) -> String {
        let name = &self.config.app_name;
        let platform = &self.config.target_platform;
        let profile = match self.config.build_profile {
            BuildProfile::Debug => "debug",
            BuildProfile::Release => "release",
            BuildProfile::ReleaseDebug => "release_debug",
        };
        let ext = match platform.as_str() {
            "windows" => ".exe",
            "macos" => ".app",
            "linux" => ".x86_64",
            "web" => ".wasm",
            _ => "",
        };
        format!("{name}.{platform}.{profile}{ext}")
    }

    /// Generates a manifest string listing all files in the export.
    pub fn generate_manifest(&self) -> String {
        let mut lines = Vec::new();
        lines.push(format!("# Export Manifest: {}", self.config.app_name));
        lines.push(format!("platform: {}", self.config.target_platform));
        lines.push(format!("profile: {:?}", self.config.build_profile));
        if !self.config.icon_path.is_empty() {
            lines.push(format!("icon: {}", self.config.icon_path));
        }
        lines.push("resources:".to_string());
        for res in &self.config.resources {
            lines.push(format!("  - {res}"));
        }
        lines.join("\n")
    }
}

// ---------------------------------------------------------------------------
// PackageResult
// ---------------------------------------------------------------------------

/// Result of an export/packaging operation.
#[derive(Debug, Clone, PartialEq)]
pub struct PackageResult {
    /// Whether the packaging succeeded.
    pub success: bool,
    /// Output path of the packaged artifact.
    pub output_path: String,
    /// Total size in bytes (0 if not yet computed).
    pub size_bytes: u64,
    /// Any warnings or errors encountered.
    pub messages: Vec<String>,
}

impl PackageResult {
    /// Creates a successful package result.
    pub fn ok(output_path: impl Into<String>, size_bytes: u64) -> Self {
        Self {
            success: true,
            output_path: output_path.into(),
            size_bytes,
            messages: Vec::new(),
        }
    }

    /// Creates a failed package result.
    pub fn err(message: impl Into<String>) -> Self {
        Self {
            success: false,
            output_path: String::new(),
            size_bytes: 0,
            messages: vec![message.into()],
        }
    }
}

// ---------------------------------------------------------------------------
// PackageError
// ---------------------------------------------------------------------------

/// Error that can occur during the packaging pipeline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PackageError {
    /// The project directory does not exist.
    ProjectDirNotFound(String),
    /// A resource path referenced in the config was not found.
    ResourceNotFound(String),
    /// The output directory could not be created.
    OutputDirCreationFailed(String),
    /// Writing a file during packaging failed.
    WriteFailed(String),
    /// The target platform is not recognized.
    UnsupportedPlatform(String),
}

impl std::fmt::Display for PackageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ProjectDirNotFound(p) => write!(f, "project directory not found: {p}"),
            Self::ResourceNotFound(p) => write!(f, "resource not found: {p}"),
            Self::OutputDirCreationFailed(p) => write!(f, "failed to create output directory: {p}"),
            Self::WriteFailed(msg) => write!(f, "write failed: {msg}"),
            Self::UnsupportedPlatform(p) => write!(f, "unsupported platform: {p}"),
        }
    }
}

impl std::error::Error for PackageError {}

// ---------------------------------------------------------------------------
// ResourceEntry
// ---------------------------------------------------------------------------

/// A single resource collected for packaging.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceEntry {
    /// The source path on disk (absolute or relative to project dir).
    pub source_path: PathBuf,
    /// The path inside the package (relative, using forward slashes).
    pub package_path: String,
    /// Size in bytes.
    pub size_bytes: u64,
}

// ---------------------------------------------------------------------------
// PackageExecutor
// ---------------------------------------------------------------------------

/// Drives the export packaging pipeline.
///
/// The executor takes an [`ExportConfig`], a project directory, and an output
/// directory. It validates inputs, collects resources, writes the manifest and
/// resource listing into the staging area, and returns a [`PackageResult`].
///
/// # Example
///
/// ```no_run
/// use gdplatform::export::{ExportConfig, PackageExecutor};
/// use std::path::Path;
///
/// let config = ExportConfig::new("linux", "MyGame")
///     .with_resource("scenes/")
///     .with_resource("textures/");
///
/// let mut executor = PackageExecutor::new(
///     config,
///     Path::new("/project"),
///     Path::new("/project/export"),
/// );
/// let result = executor.run();
/// assert!(result.success);
/// ```
pub struct PackageExecutor {
    config: ExportConfig,
    project_dir: PathBuf,
    output_dir: PathBuf,
    collected: Vec<ResourceEntry>,
    messages: Vec<String>,
}

impl PackageExecutor {
    /// Creates a new executor for the given config, project dir, and output dir.
    pub fn new(
        config: ExportConfig,
        project_dir: impl Into<PathBuf>,
        output_dir: impl Into<PathBuf>,
    ) -> Self {
        Self {
            config,
            project_dir: project_dir.into(),
            output_dir: output_dir.into(),
            collected: Vec::new(),
            messages: Vec::new(),
        }
    }

    /// Returns the collected resource entries (populated after [`validate_and_collect`]).
    pub fn collected_resources(&self) -> &[ResourceEntry] {
        &self.collected
    }

    /// Returns any messages logged during execution.
    pub fn messages(&self) -> &[String] {
        &self.messages
    }

    /// Validates the platform target against the known desktop targets.
    pub fn validate_platform(&self) -> Result<(), PackageError> {
        let platform = self.config.target_platform.as_str();
        // Accept both platform names ("linux") and rust triples ("x86_64-unknown-linux-gnu").
        let known_platform = crate::platform_targets::DESKTOP_TARGETS
            .iter()
            .any(|t| t.platform_name() == platform || t.rust_triple == platform);
        if known_platform {
            Ok(())
        } else {
            Err(PackageError::UnsupportedPlatform(platform.to_string()))
        }
    }

    /// Validates the project directory and collects resource entries.
    ///
    /// For each resource path in the config, this resolves `res://` prefixes
    /// against the project directory, walks directories to collect individual
    /// files, and records each file as a [`ResourceEntry`].
    pub fn validate_and_collect(&mut self) -> Result<(), PackageError> {
        if !self.project_dir.exists() {
            return Err(PackageError::ProjectDirNotFound(
                self.project_dir.display().to_string(),
            ));
        }

        self.collected.clear();

        let resource_paths = self.config.resources.clone();
        for res_path in &resource_paths {
            let resolved = resolve_resource_path(&self.project_dir, res_path);

            if !resolved.exists() {
                return Err(PackageError::ResourceNotFound(res_path.clone()));
            }

            if resolved.is_dir() {
                self.collect_directory(&resolved, res_path)?;
            } else {
                let size = resolved.metadata().map(|m| m.len()).unwrap_or(0);
                let package_path = strip_res_prefix(res_path);
                self.collected.push(ResourceEntry {
                    source_path: resolved,
                    package_path,
                    size_bytes: size,
                });
            }
        }

        Ok(())
    }

    /// Generates the export manifest as a string.
    pub fn generate_manifest(&self) -> String {
        let template = ExportTemplate::from_config(self.config.clone());
        let mut manifest = template.generate_manifest();
        manifest.push_str("\nfiles:\n");
        for entry in &self.collected {
            manifest.push_str(&format!(
                "  - {} ({} bytes)\n",
                entry.package_path, entry.size_bytes,
            ));
        }
        let total: u64 = self.collected.iter().map(|e| e.size_bytes).sum();
        manifest.push_str(&format!("total_resources: {}\n", self.collected.len()));
        manifest.push_str(&format!("total_size_bytes: {total}\n"));
        manifest
    }

    /// Writes the manifest and resource listing to the output directory.
    ///
    /// This creates the output directory if needed, writes `export_manifest.txt`,
    /// and writes `resource_list.txt` with one line per collected resource.
    pub fn write_output(&mut self) -> Result<PathBuf, PackageError> {
        std::fs::create_dir_all(&self.output_dir).map_err(|e| {
            PackageError::OutputDirCreationFailed(format!("{}: {e}", self.output_dir.display()))
        })?;

        // Write manifest.
        let manifest_path = self.output_dir.join("export_manifest.txt");
        let manifest = self.generate_manifest();
        std::fs::write(&manifest_path, &manifest)
            .map_err(|e| PackageError::WriteFailed(format!("{}: {e}", manifest_path.display())))?;
        self.messages
            .push(format!("wrote manifest: {}", manifest_path.display()));

        // Write resource listing.
        let listing_path = self.output_dir.join("resource_list.txt");
        let mut listing = String::new();
        for entry in &self.collected {
            listing.push_str(&format!(
                "{}\t{}\t{}\n",
                entry.package_path,
                entry.source_path.display(),
                entry.size_bytes,
            ));
        }
        std::fs::write(&listing_path, &listing)
            .map_err(|e| PackageError::WriteFailed(format!("{}: {e}", listing_path.display())))?;
        self.messages
            .push(format!("wrote resource list: {}", listing_path.display()));

        // Write the output filename marker (what the final binary would be named).
        let template = ExportTemplate::from_config(self.config.clone());
        let output_name = template.output_filename();
        let marker_path = self.output_dir.join(&output_name);
        std::fs::write(
            &marker_path,
            format!(
                "# Patina export placeholder\n# Platform: {}\n# Profile: {:?}\n",
                self.config.target_platform, self.config.build_profile
            ),
        )
        .map_err(|e| PackageError::WriteFailed(format!("{}: {e}", marker_path.display())))?;
        self.messages
            .push(format!("wrote output marker: {}", marker_path.display()));

        Ok(marker_path)
    }

    /// Runs the full packaging pipeline: validate, collect, write, return result.
    pub fn run(&mut self) -> PackageResult {
        // Step 1: Validate platform.
        if let Err(e) = self.validate_platform() {
            return PackageResult::err(e.to_string());
        }

        // Step 2: Validate and collect resources.
        if let Err(e) = self.validate_and_collect() {
            return PackageResult::err(e.to_string());
        }

        let total_size: u64 = self.collected.iter().map(|e| e.size_bytes).sum();
        self.messages.push(format!(
            "collected {} resources ({total_size} bytes)",
            self.collected.len(),
        ));

        // Step 3: Write output.
        match self.write_output() {
            Ok(output_path) => {
                let mut result = PackageResult::ok(output_path.display().to_string(), total_size);
                result.messages = self.messages.clone();
                result
            }
            Err(e) => PackageResult::err(e.to_string()),
        }
    }

    // -- Internal helpers ----------------------------------------------------

    fn collect_directory(&mut self, dir: &Path, base_res: &str) -> Result<(), PackageError> {
        let entries = std::fs::read_dir(dir)
            .map_err(|e| PackageError::ResourceNotFound(format!("{}: {e}", dir.display())))?;

        for entry in entries {
            let entry = entry
                .map_err(|e| PackageError::ResourceNotFound(format!("read_dir entry: {e}")))?;
            let path = entry.path();
            if path.is_dir() {
                // Recurse into subdirectories.
                let sub_name = path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();
                let sub_res = format!("{base_res}{sub_name}/");
                self.collect_directory(&path, &sub_res)?;
            } else {
                let size = path.metadata().map(|m| m.len()).unwrap_or(0);
                let file_name = path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();
                let package_path = format!("{}{file_name}", strip_res_prefix(base_res));
                self.collected.push(ResourceEntry {
                    source_path: path,
                    package_path,
                    size_bytes: size,
                });
            }
        }

        Ok(())
    }
}

impl std::fmt::Debug for PackageExecutor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PackageExecutor")
            .field("target_platform", &self.config.target_platform)
            .field("app_name", &self.config.app_name)
            .field("project_dir", &self.project_dir)
            .field("output_dir", &self.output_dir)
            .field("collected_count", &self.collected.len())
            .field("messages_count", &self.messages.len())
            .finish()
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Resolves a `res://`-prefixed path against the project directory.
/// Paths without `res://` are treated as relative to the project dir.
fn resolve_resource_path(project_dir: &Path, res_path: &str) -> PathBuf {
    let stripped = res_path.strip_prefix("res://").unwrap_or(res_path);
    project_dir.join(stripped)
}

/// Strips the `res://` prefix from a path for use as a package-internal path.
fn strip_res_prefix(path: &str) -> String {
    path.strip_prefix("res://").unwrap_or(path).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_profile_default_is_release() {
        assert_eq!(BuildProfile::default(), BuildProfile::Release);
    }

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
    fn export_config_builder() {
        let cfg = ExportConfig::new("windows", "CoolGame")
            .with_build_profile(BuildProfile::Debug)
            .with_icon("icon.png")
            .with_resource("scenes/")
            .with_resource("textures/");
        assert_eq!(cfg.build_profile, BuildProfile::Debug);
        assert_eq!(cfg.icon_path, "icon.png");
        assert_eq!(cfg.resources, vec!["scenes/", "textures/"]);
    }

    #[test]
    fn export_config_with_resources_batch() {
        let cfg = ExportConfig::new("macos", "App").with_resources(["a/", "b/", "c/"]);
        assert_eq!(cfg.resources.len(), 3);
    }

    #[test]
    fn export_template_generate_manifest() {
        let cfg = ExportConfig::new("linux", "TestApp")
            .with_icon("icon.png")
            .with_resource("res://scenes")
            .with_resource("res://textures");
        let template = ExportTemplate::from_config(cfg);
        let manifest = template.generate_manifest();
        assert!(manifest.contains("TestApp"));
        assert!(manifest.contains("linux"));
        assert!(manifest.contains("Release"));
        assert!(manifest.contains("icon.png"));
        assert!(manifest.contains("res://scenes"));
        assert!(manifest.contains("res://textures"));
    }

    #[test]
    fn export_template_manifest_no_icon() {
        let cfg = ExportConfig::new("web", "WebGame");
        let template = ExportTemplate::from_config(cfg);
        let manifest = template.generate_manifest();
        assert!(!manifest.contains("icon:"));
    }

    #[test]
    fn package_result_ok() {
        let result = PackageResult::ok("build/game.zip", 1024);
        assert!(result.success);
        assert_eq!(result.output_path, "build/game.zip");
        assert_eq!(result.size_bytes, 1024);
        assert!(result.messages.is_empty());
    }

    #[test]
    fn package_result_err() {
        let result = PackageResult::err("missing resource");
        assert!(!result.success);
        assert!(result.output_path.is_empty());
        assert_eq!(result.size_bytes, 0);
        assert_eq!(result.messages, vec!["missing resource"]);
    }

    #[test]
    fn build_profile_variants_distinct() {
        assert_ne!(BuildProfile::Debug, BuildProfile::Release);
        assert_ne!(BuildProfile::Release, BuildProfile::ReleaseDebug);
        assert_ne!(BuildProfile::Debug, BuildProfile::ReleaseDebug);
    }

    // -- PackageError tests --------------------------------------------------

    #[test]
    fn package_error_display() {
        let err = PackageError::ProjectDirNotFound("/nonexistent".into());
        assert!(err.to_string().contains("/nonexistent"));

        let err = PackageError::ResourceNotFound("res://missing".into());
        assert!(err.to_string().contains("res://missing"));

        let err = PackageError::UnsupportedPlatform("gamecube".into());
        assert!(err.to_string().contains("gamecube"));
    }

    // -- resolve_resource_path tests -----------------------------------------

    #[test]
    fn resolve_resource_path_with_res_prefix() {
        let project = Path::new("/my/project");
        let result = resolve_resource_path(project, "res://scenes/main.tscn");
        assert_eq!(result, PathBuf::from("/my/project/scenes/main.tscn"));
    }

    #[test]
    fn resolve_resource_path_without_prefix() {
        let project = Path::new("/my/project");
        let result = resolve_resource_path(project, "textures/icon.png");
        assert_eq!(result, PathBuf::from("/my/project/textures/icon.png"));
    }

    #[test]
    fn strip_res_prefix_with_prefix() {
        assert_eq!(
            strip_res_prefix("res://scenes/main.tscn"),
            "scenes/main.tscn"
        );
    }

    #[test]
    fn strip_res_prefix_without_prefix() {
        assert_eq!(strip_res_prefix("textures/icon.png"), "textures/icon.png");
    }

    // -- PackageExecutor tests -----------------------------------------------

    #[test]
    fn executor_validate_platform_valid() {
        for platform in &["linux", "windows", "macos", "web"] {
            let cfg = ExportConfig::new(*platform, "Game");
            let exec = PackageExecutor::new(cfg, "/tmp", "/tmp/out");
            assert!(
                exec.validate_platform().is_ok(),
                "platform {platform} should be valid"
            );
        }
    }

    #[test]
    fn executor_validate_platform_invalid() {
        let cfg = ExportConfig::new("ps5", "Game");
        let exec = PackageExecutor::new(cfg, "/tmp", "/tmp/out");
        assert!(matches!(
            exec.validate_platform(),
            Err(PackageError::UnsupportedPlatform(_))
        ));
    }

    #[test]
    fn executor_validate_and_collect_missing_project_dir() {
        let cfg = ExportConfig::new("linux", "Game");
        let mut exec = PackageExecutor::new(cfg, "/nonexistent_dir_xyz", "/tmp/out");
        assert!(matches!(
            exec.validate_and_collect(),
            Err(PackageError::ProjectDirNotFound(_))
        ));
    }

    #[test]
    fn executor_validate_and_collect_empty_resources() {
        let tmp = std::env::temp_dir().join("patina_test_empty_resources");
        let _ = std::fs::create_dir_all(&tmp);
        let cfg = ExportConfig::new("linux", "Game");
        let mut exec = PackageExecutor::new(cfg, &tmp, tmp.join("out"));
        assert!(exec.validate_and_collect().is_ok());
        assert!(exec.collected_resources().is_empty());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn executor_collect_single_file() {
        let tmp = std::env::temp_dir().join("patina_test_single_file");
        let _ = std::fs::create_dir_all(&tmp);
        std::fs::write(tmp.join("main.tscn"), "test scene").unwrap();

        let cfg = ExportConfig::new("linux", "Game").with_resource("main.tscn");
        let mut exec = PackageExecutor::new(cfg, &tmp, tmp.join("out"));
        exec.validate_and_collect().unwrap();
        assert_eq!(exec.collected_resources().len(), 1);
        assert_eq!(exec.collected_resources()[0].package_path, "main.tscn");
        assert!(exec.collected_resources()[0].size_bytes > 0);

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn executor_collect_directory() {
        let tmp = std::env::temp_dir().join("patina_test_collect_dir");
        let scenes_dir = tmp.join("scenes");
        let _ = std::fs::create_dir_all(&scenes_dir);
        std::fs::write(scenes_dir.join("a.tscn"), "scene a").unwrap();
        std::fs::write(scenes_dir.join("b.tscn"), "scene b").unwrap();

        let cfg = ExportConfig::new("linux", "Game").with_resource("scenes/");
        let mut exec = PackageExecutor::new(cfg, &tmp, tmp.join("out"));
        exec.validate_and_collect().unwrap();
        assert_eq!(exec.collected_resources().len(), 2);

        let mut paths: Vec<&str> = exec
            .collected_resources()
            .iter()
            .map(|e| e.package_path.as_str())
            .collect();
        paths.sort();
        assert_eq!(paths, vec!["scenes/a.tscn", "scenes/b.tscn"]);

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn executor_collect_with_res_prefix() {
        let tmp = std::env::temp_dir().join("patina_test_res_prefix");
        let _ = std::fs::create_dir_all(&tmp);
        std::fs::write(tmp.join("icon.png"), "png data").unwrap();

        let cfg = ExportConfig::new("linux", "Game").with_resource("res://icon.png");
        let mut exec = PackageExecutor::new(cfg, &tmp, tmp.join("out"));
        exec.validate_and_collect().unwrap();
        assert_eq!(exec.collected_resources().len(), 1);
        assert_eq!(exec.collected_resources()[0].package_path, "icon.png");

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn executor_collect_missing_resource() {
        let tmp = std::env::temp_dir().join("patina_test_missing_res");
        let _ = std::fs::create_dir_all(&tmp);

        let cfg = ExportConfig::new("linux", "Game").with_resource("nonexistent.tscn");
        let mut exec = PackageExecutor::new(cfg, &tmp, tmp.join("out"));
        assert!(matches!(
            exec.validate_and_collect(),
            Err(PackageError::ResourceNotFound(_))
        ));

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn executor_generate_manifest() {
        let tmp = std::env::temp_dir().join("patina_test_manifest");
        let _ = std::fs::create_dir_all(&tmp);
        std::fs::write(tmp.join("main.tscn"), "test data").unwrap();

        let cfg = ExportConfig::new("linux", "TestApp").with_resource("main.tscn");
        let mut exec = PackageExecutor::new(cfg, &tmp, tmp.join("out"));
        exec.validate_and_collect().unwrap();

        let manifest = exec.generate_manifest();
        assert!(manifest.contains("TestApp"));
        assert!(manifest.contains("linux"));
        assert!(manifest.contains("main.tscn"));
        assert!(manifest.contains("total_resources: 1"));

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn executor_write_output_creates_files() {
        let tmp = std::env::temp_dir().join("patina_test_write_output");
        let _ = std::fs::remove_dir_all(&tmp);
        let _ = std::fs::create_dir_all(&tmp);
        std::fs::write(tmp.join("scene.tscn"), "scene data here").unwrap();

        let out_dir = tmp.join("export_out");
        let cfg = ExportConfig::new("linux", "MyGame").with_resource("scene.tscn");
        let mut exec = PackageExecutor::new(cfg, &tmp, &out_dir);
        exec.validate_and_collect().unwrap();
        let output_path = exec.write_output().unwrap();

        assert!(out_dir.join("export_manifest.txt").exists());
        assert!(out_dir.join("resource_list.txt").exists());
        assert!(output_path.exists());
        assert!(output_path
            .to_string_lossy()
            .contains("MyGame.linux.release"));

        let manifest = std::fs::read_to_string(out_dir.join("export_manifest.txt")).unwrap();
        assert!(manifest.contains("MyGame"));

        let listing = std::fs::read_to_string(out_dir.join("resource_list.txt")).unwrap();
        assert!(listing.contains("scene.tscn"));

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn executor_run_full_pipeline() {
        let tmp = std::env::temp_dir().join("patina_test_run_pipeline");
        let _ = std::fs::remove_dir_all(&tmp);
        let _ = std::fs::create_dir_all(&tmp);
        let textures = tmp.join("textures");
        std::fs::create_dir_all(&textures).unwrap();
        std::fs::write(tmp.join("main.tscn"), "main scene").unwrap();
        std::fs::write(textures.join("player.png"), "player sprite").unwrap();

        let cfg = ExportConfig::new("windows", "CoolGame")
            .with_build_profile(BuildProfile::Debug)
            .with_resource("main.tscn")
            .with_resource("textures/");

        let mut exec = PackageExecutor::new(cfg, &tmp, tmp.join("build"));
        let result = exec.run();

        assert!(result.success);
        assert!(result.output_path.contains("CoolGame.windows.debug"));
        assert!(result.size_bytes > 0);
        assert!(!result.messages.is_empty());

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn executor_run_bad_platform_returns_err() {
        let cfg = ExportConfig::new("dreamcast", "Game");
        let mut exec = PackageExecutor::new(cfg, "/tmp", "/tmp/out");
        let result = exec.run();
        assert!(!result.success);
        assert!(result.messages[0].contains("unsupported platform"));
    }

    #[test]
    fn executor_debug_output() {
        let cfg = ExportConfig::new("linux", "Game");
        let exec = PackageExecutor::new(cfg, "/project", "/out");
        let debug = format!("{exec:?}");
        assert!(debug.contains("PackageExecutor"));
        assert!(debug.contains("linux"));
        assert!(debug.contains("Game"));
    }

    #[test]
    fn executor_collect_nested_subdirectories() {
        let tmp = std::env::temp_dir().join("patina_test_nested_dirs");
        let _ = std::fs::remove_dir_all(&tmp);
        let sub = tmp.join("assets").join("models");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(sub.join("mesh.obj"), "mesh data").unwrap();
        std::fs::write(tmp.join("assets").join("texture.png"), "tex").unwrap();

        let cfg = ExportConfig::new("linux", "Game").with_resource("assets/");
        let mut exec = PackageExecutor::new(cfg, &tmp, tmp.join("out"));
        exec.validate_and_collect().unwrap();

        let paths: Vec<&str> = exec
            .collected_resources()
            .iter()
            .map(|e| e.package_path.as_str())
            .collect();
        // Should have both the nested and direct files.
        assert!(paths.iter().any(|p| p.contains("mesh.obj")));
        assert!(paths.iter().any(|p| p.contains("texture.png")));

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn executor_run_with_multiple_platforms() {
        let tmp = std::env::temp_dir().join("patina_test_multi_platform");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        std::fs::write(tmp.join("game.tscn"), "game").unwrap();

        for platform in &["linux", "windows", "macos", "web"] {
            let out = tmp.join(format!("out_{platform}"));
            let cfg = ExportConfig::new(*platform, "Game").with_resource("game.tscn");
            let mut exec = PackageExecutor::new(cfg, &tmp, &out);
            let result = exec.run();
            assert!(result.success, "packaging failed for {platform}");
            assert!(
                result.output_path.contains(platform),
                "output should contain platform name for {platform}"
            );
        }

        let _ = std::fs::remove_dir_all(&tmp);
    }
}
