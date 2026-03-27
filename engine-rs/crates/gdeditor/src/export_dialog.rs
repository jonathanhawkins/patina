//! Export dialog with platform preset management.
//!
//! Provides a headless model for a Godot-style export dialog where users
//! manage export presets for different platforms (Windows, Linux, macOS, Web)
//! and configure per-platform settings.

use std::collections::HashMap;

use gdplatform::export::{BuildProfile, ExportConfig, ExportTemplate, PackageResult};

/// Supported export platforms.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ExportPlatform {
    /// Windows desktop (exe).
    Windows,
    /// Linux desktop (x86_64).
    Linux,
    /// macOS desktop (app bundle).
    MacOS,
    /// Web / HTML5 (WASM).
    Web,
    /// Android (APK).
    Android,
    /// iOS (Xcode project).
    IOS,
}

impl ExportPlatform {
    /// Returns all supported platforms.
    pub fn all() -> &'static [ExportPlatform] {
        &[
            Self::Windows,
            Self::Linux,
            Self::MacOS,
            Self::Web,
            Self::Android,
            Self::IOS,
        ]
    }

    /// Display name.
    pub fn label(&self) -> &'static str {
        match self {
            Self::Windows => "Windows Desktop",
            Self::Linux => "Linux/X11",
            Self::MacOS => "macOS",
            Self::Web => "Web (HTML5)",
            Self::Android => "Android",
            Self::IOS => "iOS",
        }
    }

    /// Short id for serialization.
    pub fn id(&self) -> &'static str {
        match self {
            Self::Windows => "windows",
            Self::Linux => "linux",
            Self::MacOS => "macos",
            Self::Web => "web",
            Self::Android => "android",
            Self::IOS => "ios",
        }
    }

    /// Parse from id string.
    pub fn from_id(s: &str) -> Option<Self> {
        match s {
            "windows" => Some(Self::Windows),
            "linux" => Some(Self::Linux),
            "macos" => Some(Self::MacOS),
            "web" => Some(Self::Web),
            "android" => Some(Self::Android),
            "ios" => Some(Self::IOS),
            _ => None,
        }
    }

    /// Default file extension for this platform's export.
    pub fn default_extension(&self) -> &'static str {
        match self {
            Self::Windows => ".exe",
            Self::Linux => "",
            Self::MacOS => ".app",
            Self::Web => ".html",
            Self::Android => ".apk",
            Self::IOS => ".xcodeproj",
        }
    }
}

/// Build profile for an export preset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportBuildProfile {
    /// Debug build with symbols.
    Debug,
    /// Optimized release build.
    Release,
}

impl ExportBuildProfile {
    /// Parse from string.
    pub fn from_str_name(s: &str) -> Option<Self> {
        match s {
            "debug" => Some(Self::Debug),
            "release" => Some(Self::Release),
            _ => None,
        }
    }

    /// String id.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Debug => "debug",
            Self::Release => "release",
        }
    }
}

/// An export preset configuration.
#[derive(Debug, Clone, PartialEq)]
pub struct ExportPreset {
    /// Unique preset name (user-defined).
    pub name: String,
    /// Target platform.
    pub platform: ExportPlatform,
    /// Build profile.
    pub build_profile: ExportBuildProfile,
    /// Application name override (empty = use project name).
    pub app_name: String,
    /// Icon path override.
    pub icon_path: String,
    /// Export output path.
    pub export_path: String,
    /// Resource filter patterns (include).
    pub include_filters: Vec<String>,
    /// Resource filter patterns (exclude).
    pub exclude_filters: Vec<String>,
    /// Platform-specific custom properties.
    pub custom_properties: HashMap<String, String>,
}

impl ExportPreset {
    /// Creates a new preset with defaults for the given platform.
    pub fn new(name: impl Into<String>, platform: ExportPlatform) -> Self {
        let name = name.into();
        let export_path = format!("export/{}/{name}{}", platform.id(), platform.default_extension());
        Self {
            name,
            platform,
            build_profile: ExportBuildProfile::Release,
            app_name: String::new(),
            icon_path: String::new(),
            export_path,
            include_filters: vec!["*".to_string()],
            exclude_filters: Vec::new(),
            custom_properties: Self::default_custom_properties(platform),
        }
    }

    /// Whether this preset targets a desktop platform (Windows, Linux, macOS).
    pub fn is_desktop(&self) -> bool {
        matches!(
            self.platform,
            ExportPlatform::Windows | ExportPlatform::Linux | ExportPlatform::MacOS
        )
    }

    /// Converts this preset to a `gdplatform::export::ExportConfig`.
    pub fn to_export_config(&self, project_name: &str) -> ExportConfig {
        let app_name = if self.app_name.is_empty() {
            project_name.to_string()
        } else {
            self.app_name.clone()
        };
        let mut cfg = ExportConfig::new(self.platform.id(), app_name)
            .with_build_profile(match self.build_profile {
                ExportBuildProfile::Debug => BuildProfile::Debug,
                ExportBuildProfile::Release => BuildProfile::Release,
            });
        if !self.icon_path.is_empty() {
            cfg = cfg.with_icon(&self.icon_path);
        }
        cfg = cfg.with_resources(self.include_filters.iter().cloned());
        cfg
    }

    fn default_custom_properties(platform: ExportPlatform) -> HashMap<String, String> {
        let mut props = HashMap::new();
        match platform {
            ExportPlatform::Windows => {
                props.insert("console_enabled".into(), "false".into());
                props.insert("company_name".into(), String::new());
                props.insert("product_version".into(), "1.0.0".into());
            }
            ExportPlatform::Linux => {
                props.insert("binary_format".into(), "x86_64".into());
            }
            ExportPlatform::MacOS => {
                props.insert("bundle_identifier".into(), "com.example.game".into());
                props.insert("signature".into(), String::new());
                props.insert("entitlements".into(), String::new());
            }
            ExportPlatform::Web => {
                props.insert("html_shell".into(), String::new());
                props.insert("canvas_resize_policy".into(), "adaptive".into());
                props.insert("threads_enabled".into(), "false".into());
            }
            ExportPlatform::Android => {
                props.insert("package_name".into(), "com.example.game".into());
                props.insert("min_sdk".into(), "21".into());
                props.insert("target_sdk".into(), "33".into());
            }
            ExportPlatform::IOS => {
                props.insert("bundle_identifier".into(), "com.example.game".into());
                props.insert("team_id".into(), String::new());
            }
        }
        props
    }
}

/// Validation error when attempting to export.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExportValidationError {
    /// No preset name.
    EmptyName,
    /// No export path specified.
    EmptyExportPath,
    /// Duplicate preset name.
    DuplicateName(String),
}

/// Error returned by a one-click export attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExportError {
    /// The requested preset index is out of range.
    InvalidIndex(usize),
    /// The preset failed validation.
    Validation(Vec<ExportValidationError>),
    /// The preset targets a non-desktop platform.
    NotDesktopPlatform(String),
}

/// The export dialog state.
#[derive(Debug)]
pub struct ExportDialog {
    visible: bool,
    presets: Vec<ExportPreset>,
    selected_index: Option<usize>,
}

impl ExportDialog {
    /// Creates a new empty export dialog.
    pub fn new() -> Self {
        Self {
            visible: false,
            presets: Vec::new(),
            selected_index: None,
        }
    }

    /// Opens the dialog.
    pub fn open(&mut self) {
        self.visible = true;
        if self.selected_index.is_none() && !self.presets.is_empty() {
            self.selected_index = Some(0);
        }
    }

    /// Closes the dialog.
    pub fn close(&mut self) {
        self.visible = false;
    }

    /// Whether the dialog is visible.
    pub fn is_visible(&self) -> bool {
        self.visible
    }

    /// Returns all presets.
    pub fn presets(&self) -> &[ExportPreset] {
        &self.presets
    }

    /// Returns the currently selected preset index.
    pub fn selected_index(&self) -> Option<usize> {
        self.selected_index
    }

    /// Returns the currently selected preset.
    pub fn selected_preset(&self) -> Option<&ExportPreset> {
        self.selected_index.and_then(|i| self.presets.get(i))
    }

    /// Returns a mutable reference to the selected preset.
    pub fn selected_preset_mut(&mut self) -> Option<&mut ExportPreset> {
        self.selected_index.and_then(|i| self.presets.get_mut(i))
    }

    /// Selects a preset by index.
    pub fn select(&mut self, index: usize) -> bool {
        if index < self.presets.len() {
            self.selected_index = Some(index);
            true
        } else {
            false
        }
    }

    /// Adds a new preset and selects it.
    pub fn add_preset(&mut self, preset: ExportPreset) -> Result<usize, ExportValidationError> {
        if preset.name.is_empty() {
            return Err(ExportValidationError::EmptyName);
        }
        if self.presets.iter().any(|p| p.name == preset.name) {
            return Err(ExportValidationError::DuplicateName(preset.name));
        }
        self.presets.push(preset);
        let idx = self.presets.len() - 1;
        self.selected_index = Some(idx);
        Ok(idx)
    }

    /// Removes the preset at the given index.
    pub fn remove_preset(&mut self, index: usize) -> Option<ExportPreset> {
        if index >= self.presets.len() {
            return None;
        }
        let removed = self.presets.remove(index);
        // Adjust selection
        if self.presets.is_empty() {
            self.selected_index = None;
        } else if let Some(sel) = self.selected_index {
            if sel >= self.presets.len() {
                self.selected_index = Some(self.presets.len() - 1);
            }
        }
        Some(removed)
    }

    /// Moves a preset up in the list.
    pub fn move_preset_up(&mut self, index: usize) -> bool {
        if index == 0 || index >= self.presets.len() {
            return false;
        }
        self.presets.swap(index, index - 1);
        if self.selected_index == Some(index) {
            self.selected_index = Some(index - 1);
        }
        true
    }

    /// Moves a preset down in the list.
    pub fn move_preset_down(&mut self, index: usize) -> bool {
        if index + 1 >= self.presets.len() {
            return false;
        }
        self.presets.swap(index, index + 1);
        if self.selected_index == Some(index) {
            self.selected_index = Some(index + 1);
        }
        true
    }

    /// Validates a preset before export.
    pub fn validate_preset(&self, index: usize) -> Vec<ExportValidationError> {
        let mut errors = Vec::new();
        if let Some(preset) = self.presets.get(index) {
            if preset.name.is_empty() {
                errors.push(ExportValidationError::EmptyName);
            }
            if preset.export_path.is_empty() {
                errors.push(ExportValidationError::EmptyExportPath);
            }
        }
        errors
    }

    /// Returns the number of presets.
    pub fn preset_count(&self) -> usize {
        self.presets.len()
    }

    /// One-click export for a preset at the given index.
    ///
    /// Validates the preset, checks it targets a desktop platform, converts it
    /// to an `ExportConfig`, generates a manifest via `ExportTemplate`, and
    /// returns a `PackageResult` with the output path and manifest size.
    pub fn export_one_click(
        &self,
        index: usize,
        project_name: &str,
    ) -> Result<PackageResult, ExportError> {
        let preset = self
            .presets
            .get(index)
            .ok_or(ExportError::InvalidIndex(index))?;

        let errors = self.validate_preset(index);
        if !errors.is_empty() {
            return Err(ExportError::Validation(errors));
        }

        if !preset.is_desktop() {
            return Err(ExportError::NotDesktopPlatform(
                preset.platform.label().to_string(),
            ));
        }

        let config = preset.to_export_config(project_name);
        let template = ExportTemplate::from_config(config);
        let manifest = template.generate_manifest();
        let manifest_size = manifest.len() as u64;

        let mut result = PackageResult::ok(&preset.export_path, manifest_size);
        result.messages.push(format!(
            "Export manifest generated for {} ({})",
            preset.name,
            preset.platform.label()
        ));
        Ok(result)
    }

    /// One-click export for the currently selected preset.
    pub fn export_selected(&self, project_name: &str) -> Result<PackageResult, ExportError> {
        let index = self
            .selected_index
            .ok_or(ExportError::InvalidIndex(usize::MAX))?;
        self.export_one_click(index, project_name)
    }

    /// Loads presets from serialized data.
    pub fn load_presets(&mut self, presets: Vec<ExportPreset>) {
        self.presets = presets;
        if !self.presets.is_empty() && self.selected_index.is_none() {
            self.selected_index = Some(0);
        }
    }
}

impl Default for ExportDialog {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn platform_roundtrip() {
        for p in ExportPlatform::all() {
            let id = p.id();
            let parsed = ExportPlatform::from_id(id).unwrap();
            assert_eq!(*p, parsed);
        }
    }

    #[test]
    fn new_preset_has_defaults() {
        let preset = ExportPreset::new("Windows", ExportPlatform::Windows);
        assert_eq!(preset.name, "Windows");
        assert_eq!(preset.platform, ExportPlatform::Windows);
        assert_eq!(preset.build_profile, ExportBuildProfile::Release);
        assert!(preset.export_path.contains(".exe"));
        assert!(!preset.custom_properties.is_empty());
    }

    #[test]
    fn add_and_select_preset() {
        let mut dialog = ExportDialog::new();
        let idx = dialog
            .add_preset(ExportPreset::new("Linux", ExportPlatform::Linux))
            .unwrap();
        assert_eq!(idx, 0);
        assert_eq!(dialog.selected_index(), Some(0));
        assert_eq!(dialog.selected_preset().unwrap().name, "Linux");
    }

    #[test]
    fn duplicate_name_rejected() {
        let mut dialog = ExportDialog::new();
        dialog
            .add_preset(ExportPreset::new("Web", ExportPlatform::Web))
            .unwrap();
        let err = dialog
            .add_preset(ExportPreset::new("Web", ExportPlatform::Web))
            .unwrap_err();
        assert_eq!(err, ExportValidationError::DuplicateName("Web".into()));
    }

    #[test]
    fn empty_name_rejected() {
        let mut dialog = ExportDialog::new();
        let err = dialog
            .add_preset(ExportPreset::new("", ExportPlatform::Linux))
            .unwrap_err();
        assert_eq!(err, ExportValidationError::EmptyName);
    }

    #[test]
    fn remove_preset_adjusts_selection() {
        let mut dialog = ExportDialog::new();
        dialog
            .add_preset(ExportPreset::new("A", ExportPlatform::Windows))
            .unwrap();
        dialog
            .add_preset(ExportPreset::new("B", ExportPlatform::Linux))
            .unwrap();
        dialog.select(1);
        dialog.remove_preset(1);
        assert_eq!(dialog.selected_index(), Some(0));
        assert_eq!(dialog.preset_count(), 1);
    }

    #[test]
    fn remove_all_clears_selection() {
        let mut dialog = ExportDialog::new();
        dialog
            .add_preset(ExportPreset::new("A", ExportPlatform::Web))
            .unwrap();
        dialog.remove_preset(0);
        assert_eq!(dialog.selected_index(), None);
        assert_eq!(dialog.preset_count(), 0);
    }

    #[test]
    fn move_preset_up_down() {
        let mut dialog = ExportDialog::new();
        dialog
            .add_preset(ExportPreset::new("A", ExportPlatform::Windows))
            .unwrap();
        dialog
            .add_preset(ExportPreset::new("B", ExportPlatform::Linux))
            .unwrap();
        dialog.select(1);
        assert!(dialog.move_preset_up(1));
        assert_eq!(dialog.presets()[0].name, "B");
        assert_eq!(dialog.selected_index(), Some(0));

        assert!(dialog.move_preset_down(0));
        assert_eq!(dialog.presets()[0].name, "A");
        assert_eq!(dialog.selected_index(), Some(1));
    }

    #[test]
    fn move_bounds() {
        let mut dialog = ExportDialog::new();
        dialog
            .add_preset(ExportPreset::new("A", ExportPlatform::Web))
            .unwrap();
        assert!(!dialog.move_preset_up(0));
        assert!(!dialog.move_preset_down(0));
    }

    #[test]
    fn validate_preset_empty_path() {
        let mut dialog = ExportDialog::new();
        let mut preset = ExportPreset::new("Test", ExportPlatform::Web);
        preset.export_path.clear();
        dialog.add_preset(preset).unwrap();
        let errors = dialog.validate_preset(0);
        assert!(errors.contains(&ExportValidationError::EmptyExportPath));
    }

    #[test]
    fn platform_default_custom_properties() {
        let mac_preset = ExportPreset::new("Mac", ExportPlatform::MacOS);
        assert!(mac_preset.custom_properties.contains_key("bundle_identifier"));

        let android_preset = ExportPreset::new("Droid", ExportPlatform::Android);
        assert!(android_preset.custom_properties.contains_key("min_sdk"));

        let web_preset = ExportPreset::new("Web", ExportPlatform::Web);
        assert!(web_preset.custom_properties.contains_key("canvas_resize_policy"));
    }

    #[test]
    fn build_profile_roundtrip() {
        for name in &["debug", "release"] {
            let p = ExportBuildProfile::from_str_name(name).unwrap();
            assert_eq!(p.as_str(), *name);
        }
    }

    #[test]
    fn open_selects_first_if_none() {
        let mut dialog = ExportDialog::new();
        dialog
            .add_preset(ExportPreset::new("A", ExportPlatform::Windows))
            .unwrap();
        dialog.selected_index = None;
        dialog.open();
        assert_eq!(dialog.selected_index(), Some(0));
    }

    #[test]
    fn preset_is_desktop() {
        assert!(ExportPreset::new("W", ExportPlatform::Windows).is_desktop());
        assert!(ExportPreset::new("L", ExportPlatform::Linux).is_desktop());
        assert!(ExportPreset::new("M", ExportPlatform::MacOS).is_desktop());
        assert!(!ExportPreset::new("W", ExportPlatform::Web).is_desktop());
        assert!(!ExportPreset::new("A", ExportPlatform::Android).is_desktop());
        assert!(!ExportPreset::new("I", ExportPlatform::IOS).is_desktop());
    }

    #[test]
    fn to_export_config_uses_project_name_when_app_name_empty() {
        let preset = ExportPreset::new("Linux", ExportPlatform::Linux);
        let cfg = preset.to_export_config("MyProject");
        assert_eq!(cfg.app_name, "MyProject");
        assert_eq!(cfg.target_platform, "linux");
        assert_eq!(cfg.build_profile, BuildProfile::Release);
    }

    #[test]
    fn to_export_config_uses_app_name_when_set() {
        let mut preset = ExportPreset::new("Win", ExportPlatform::Windows);
        preset.app_name = "CustomApp".into();
        let cfg = preset.to_export_config("Fallback");
        assert_eq!(cfg.app_name, "CustomApp");
    }

    #[test]
    fn to_export_config_debug_profile() {
        let mut preset = ExportPreset::new("Mac", ExportPlatform::MacOS);
        preset.build_profile = ExportBuildProfile::Debug;
        let cfg = preset.to_export_config("Game");
        assert_eq!(cfg.build_profile, BuildProfile::Debug);
    }

    #[test]
    fn to_export_config_includes_icon() {
        let mut preset = ExportPreset::new("Win", ExportPlatform::Windows);
        preset.icon_path = "icon.ico".into();
        let cfg = preset.to_export_config("Game");
        assert_eq!(cfg.icon_path, "icon.ico");
    }

    #[test]
    fn export_one_click_success_windows() {
        let mut dialog = ExportDialog::new();
        dialog
            .add_preset(ExportPreset::new("Win", ExportPlatform::Windows))
            .unwrap();
        let result = dialog.export_one_click(0, "TestGame").unwrap();
        assert!(result.success);
        assert!(result.output_path.contains("windows"));
        assert!(result.output_path.contains(".exe"));
        assert!(result.size_bytes > 0);
        assert!(!result.messages.is_empty());
    }

    #[test]
    fn export_one_click_success_linux() {
        let mut dialog = ExportDialog::new();
        dialog
            .add_preset(ExportPreset::new("Linux", ExportPlatform::Linux))
            .unwrap();
        let result = dialog.export_one_click(0, "TestGame").unwrap();
        assert!(result.success);
        assert!(result.output_path.contains("linux"));
    }

    #[test]
    fn export_one_click_success_macos() {
        let mut dialog = ExportDialog::new();
        dialog
            .add_preset(ExportPreset::new("Mac", ExportPlatform::MacOS))
            .unwrap();
        let result = dialog.export_one_click(0, "TestGame").unwrap();
        assert!(result.success);
        assert!(result.output_path.contains(".app"));
    }

    #[test]
    fn export_one_click_rejects_web() {
        let mut dialog = ExportDialog::new();
        dialog
            .add_preset(ExportPreset::new("Web", ExportPlatform::Web))
            .unwrap();
        let err = dialog.export_one_click(0, "Game").unwrap_err();
        assert!(matches!(err, ExportError::NotDesktopPlatform(_)));
    }

    #[test]
    fn export_one_click_rejects_android() {
        let mut dialog = ExportDialog::new();
        dialog
            .add_preset(ExportPreset::new("Droid", ExportPlatform::Android))
            .unwrap();
        let err = dialog.export_one_click(0, "Game").unwrap_err();
        assert!(matches!(err, ExportError::NotDesktopPlatform(_)));
    }

    #[test]
    fn export_one_click_invalid_index() {
        let dialog = ExportDialog::new();
        let err = dialog.export_one_click(0, "Game").unwrap_err();
        assert!(matches!(err, ExportError::InvalidIndex(0)));
    }

    #[test]
    fn export_one_click_validation_failure() {
        let mut dialog = ExportDialog::new();
        let mut preset = ExportPreset::new("Bad", ExportPlatform::Windows);
        preset.export_path.clear();
        dialog.add_preset(preset).unwrap();
        let err = dialog.export_one_click(0, "Game").unwrap_err();
        assert!(matches!(err, ExportError::Validation(_)));
    }

    #[test]
    fn export_selected_uses_current_selection() {
        let mut dialog = ExportDialog::new();
        dialog
            .add_preset(ExportPreset::new("Win", ExportPlatform::Windows))
            .unwrap();
        dialog
            .add_preset(ExportPreset::new("Linux", ExportPlatform::Linux))
            .unwrap();
        dialog.select(1);
        let result = dialog.export_selected("Game").unwrap();
        assert!(result.output_path.contains("linux"));
    }

    #[test]
    fn export_selected_no_selection() {
        let dialog = ExportDialog::new();
        let err = dialog.export_selected("Game").unwrap_err();
        assert!(matches!(err, ExportError::InvalidIndex(_)));
    }

    #[test]
    fn load_presets_replaces_existing() {
        let mut dialog = ExportDialog::new();
        dialog
            .add_preset(ExportPreset::new("Old", ExportPlatform::Web))
            .unwrap();
        dialog.load_presets(vec![
            ExportPreset::new("New1", ExportPlatform::Windows),
            ExportPreset::new("New2", ExportPlatform::Linux),
        ]);
        assert_eq!(dialog.preset_count(), 2);
        assert_eq!(dialog.presets()[0].name, "New1");
    }
}
