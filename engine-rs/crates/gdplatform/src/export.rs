//! Export and packaging configuration.
//!
//! Defines structures for configuring how a Patina project is exported
//! to a target platform, mirroring Godot's export template system.

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
}
