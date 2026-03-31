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
        let export_path = format!(
            "export/{}/{name}{}",
            platform.id(),
            platform.default_extension()
        );
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
        let mut cfg = ExportConfig::new(self.platform.id(), app_name).with_build_profile(
            match self.build_profile {
                ExportBuildProfile::Debug => BuildProfile::Debug,
                ExportBuildProfile::Release => BuildProfile::Release,
            },
        );
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
    export_history: Vec<ExportHistoryEntry>,
}

impl ExportDialog {
    const MAX_HISTORY: usize = 50;

    /// Creates a new empty export dialog.
    pub fn new() -> Self {
        Self {
            visible: false,
            presets: Vec::new(),
            selected_index: None,
            export_history: Vec::new(),
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

    /// Duplicates a preset at the given index with a new unique name.
    ///
    /// Returns the index of the new preset, or an error if the source index is invalid.
    pub fn duplicate_preset(&mut self, index: usize) -> Result<usize, ExportError> {
        let source = self
            .presets
            .get(index)
            .ok_or(ExportError::InvalidIndex(index))?
            .clone();

        // Generate a unique name by appending " (copy)", " (copy 2)", etc.
        let base_name = source.name.clone();
        let mut new_name = format!("{} (copy)", base_name);
        let mut counter = 2u32;
        while self.presets.iter().any(|p| p.name == new_name) {
            new_name = format!("{} (copy {})", base_name, counter);
            counter += 1;
        }

        let mut new_preset = source;
        new_preset.name = new_name;
        // Update export path to reflect the new name
        new_preset.export_path = format!(
            "export/{}/{}{}",
            new_preset.platform.id(),
            new_preset.name,
            new_preset.platform.default_extension(),
        );

        self.presets.push(new_preset);
        let idx = self.presets.len() - 1;
        self.selected_index = Some(idx);
        Ok(idx)
    }

    /// Renames the preset at the given index.
    ///
    /// Validates that the new name is non-empty and unique.
    pub fn rename_preset(
        &mut self,
        index: usize,
        new_name: impl Into<String>,
    ) -> Result<(), ExportValidationError> {
        let new_name = new_name.into();
        if new_name.is_empty() {
            return Err(ExportValidationError::EmptyName);
        }
        if self
            .presets
            .iter()
            .enumerate()
            .any(|(i, p)| i != index && p.name == new_name)
        {
            return Err(ExportValidationError::DuplicateName(new_name));
        }
        if let Some(preset) = self.presets.get_mut(index) {
            preset.name = new_name;
            Ok(())
        } else {
            Err(ExportValidationError::EmptyName) // index out of range
        }
    }

    /// Creates presets for all desktop platforms (Windows, Linux, macOS) at once.
    ///
    /// Returns the number of presets successfully added (skips duplicates).
    pub fn add_desktop_presets(&mut self) -> usize {
        let desktops = [
            ("Windows Desktop", ExportPlatform::Windows),
            ("Linux Desktop", ExportPlatform::Linux),
            ("macOS Desktop", ExportPlatform::MacOS),
        ];
        let mut added = 0;
        for (name, platform) in desktops {
            if self.add_preset(ExportPreset::new(name, platform)).is_ok() {
                added += 1;
            }
        }
        added
    }

    /// Returns presets filtered by platform.
    pub fn presets_for_platform(&self, platform: ExportPlatform) -> Vec<(usize, &ExportPreset)> {
        self.presets
            .iter()
            .enumerate()
            .filter(|(_, p)| p.platform == platform)
            .collect()
    }

    /// Finds a preset by name, returning its index.
    pub fn find_preset(&self, name: &str) -> Option<usize> {
        self.presets.iter().position(|p| p.name == name)
    }

    /// Serializes all presets to a portable text format.
    ///
    /// Format: one line per field, presets separated by `---`.
    pub fn serialize_presets(&self) -> String {
        let mut out = String::new();
        for (i, preset) in self.presets.iter().enumerate() {
            if i > 0 {
                out.push_str("---\n");
            }
            out.push_str(&format!("name={}\n", preset.name));
            out.push_str(&format!("platform={}\n", preset.platform.id()));
            out.push_str(&format!(
                "build_profile={}\n",
                preset.build_profile.as_str()
            ));
            out.push_str(&format!("export_path={}\n", preset.export_path));
            if !preset.app_name.is_empty() {
                out.push_str(&format!("app_name={}\n", preset.app_name));
            }
            if !preset.icon_path.is_empty() {
                out.push_str(&format!("icon_path={}\n", preset.icon_path));
            }
            for filter in &preset.include_filters {
                out.push_str(&format!("include={}\n", filter));
            }
            for filter in &preset.exclude_filters {
                out.push_str(&format!("exclude={}\n", filter));
            }
            for (k, v) in &preset.custom_properties {
                out.push_str(&format!("custom.{}={}\n", k, v));
            }
        }
        out
    }

    /// Deserializes presets from the portable text format.
    ///
    /// Returns the number of presets loaded.
    pub fn deserialize_presets(&mut self, data: &str) -> usize {
        let mut presets = Vec::new();
        let blocks: Vec<&str> = data.split("---\n").collect();

        for block in blocks {
            let block = block.trim();
            if block.is_empty() {
                continue;
            }

            let mut name = String::new();
            let mut platform_id = String::new();
            let mut build_profile = ExportBuildProfile::Release;
            let mut export_path = String::new();
            let mut app_name = String::new();
            let mut icon_path = String::new();
            let mut includes = Vec::new();
            let mut excludes = Vec::new();
            let mut custom = HashMap::new();

            for line in block.lines() {
                let line = line.trim();
                if let Some((key, value)) = line.split_once('=') {
                    match key {
                        "name" => name = value.to_string(),
                        "platform" => platform_id = value.to_string(),
                        "build_profile" => {
                            if let Some(bp) = ExportBuildProfile::from_str_name(value) {
                                build_profile = bp;
                            }
                        }
                        "export_path" => export_path = value.to_string(),
                        "app_name" => app_name = value.to_string(),
                        "icon_path" => icon_path = value.to_string(),
                        "include" => includes.push(value.to_string()),
                        "exclude" => excludes.push(value.to_string()),
                        k if k.starts_with("custom.") => {
                            let prop_name = &k["custom.".len()..];
                            custom.insert(prop_name.to_string(), value.to_string());
                        }
                        _ => {}
                    }
                }
            }

            if name.is_empty() || platform_id.is_empty() {
                continue;
            }

            let Some(platform) = ExportPlatform::from_id(&platform_id) else {
                continue;
            };

            if includes.is_empty() {
                includes.push("*".to_string());
            }

            if export_path.is_empty() {
                export_path = format!(
                    "export/{}/{}{}",
                    platform.id(),
                    name,
                    platform.default_extension()
                );
            }

            // Merge default custom properties (serialized ones override)
            let mut merged_custom = ExportPreset::default_custom_properties(platform);
            for (k, v) in custom {
                merged_custom.insert(k, v);
            }

            presets.push(ExportPreset {
                name,
                platform,
                build_profile,
                app_name,
                icon_path,
                export_path,
                include_filters: includes,
                exclude_filters: excludes,
                custom_properties: merged_custom,
            });
        }

        let count = presets.len();
        self.load_presets(presets);
        count
    }

    /// One-click export all desktop presets. Returns results for each desktop preset.
    pub fn export_all_desktop(
        &self,
        project_name: &str,
    ) -> Vec<(String, Result<PackageResult, ExportError>)> {
        self.presets
            .iter()
            .enumerate()
            .filter(|(_, p)| p.is_desktop())
            .map(|(i, p)| (p.name.clone(), self.export_one_click(i, project_name)))
            .collect()
    }

    /// Quick-export: ensures desktop presets exist, then exports all of them.
    /// Creates missing desktop presets automatically if needed.
    pub fn quick_export_desktop(
        &mut self,
        project_name: &str,
    ) -> Vec<(String, Result<PackageResult, ExportError>)> {
        self.add_desktop_presets(); // no-op if already exist
        self.export_all_desktop(project_name)
    }

    /// Returns desktop presets only.
    pub fn desktop_presets(&self) -> Vec<(usize, &ExportPreset)> {
        self.presets
            .iter()
            .enumerate()
            .filter(|(_, p)| p.is_desktop())
            .collect()
    }

    /// Records an export result in the history.
    pub fn record_export(&mut self, entry: ExportHistoryEntry) {
        self.export_history.insert(0, entry);
        self.export_history.truncate(Self::MAX_HISTORY);
    }

    /// Returns the export history.
    pub fn export_history(&self) -> &[ExportHistoryEntry] {
        &self.export_history
    }

    /// Clears the export history.
    pub fn clear_export_history(&mut self) {
        self.export_history.clear();
    }

    /// Performs a one-click export and records the result in history.
    pub fn export_one_click_with_history(
        &mut self,
        index: usize,
        project_name: &str,
    ) -> Result<PackageResult, ExportError> {
        let preset_name = self
            .presets
            .get(index)
            .map(|p| p.name.clone())
            .unwrap_or_default();
        let platform = self
            .presets
            .get(index)
            .map(|p| p.platform)
            .unwrap_or(ExportPlatform::Windows);

        // Borrow `self` immutably for the export
        let result = self.export_one_click(index, project_name);

        let status = match &result {
            Ok(_) => ExportStatus::Completed,
            Err(e) => ExportStatus::Failed(format!("{e:?}")),
        };

        self.record_export(ExportHistoryEntry {
            preset_name,
            platform,
            status,
            output_path: result
                .as_ref()
                .map(|r| r.output_path.clone())
                .unwrap_or_default(),
        });

        result
    }

    /// Quick-export all desktop presets with history tracking.
    pub fn quick_export_desktop_with_history(
        &mut self,
        project_name: &str,
    ) -> Vec<(String, Result<PackageResult, ExportError>)> {
        self.add_desktop_presets();

        let desktop_indices: Vec<(usize, String, ExportPlatform)> = self
            .presets
            .iter()
            .enumerate()
            .filter(|(_, p)| p.is_desktop())
            .map(|(i, p)| (i, p.name.clone(), p.platform))
            .collect();

        let mut results = Vec::new();
        for (idx, name, platform) in desktop_indices {
            let result = self.export_one_click(idx, project_name);

            let status = match &result {
                Ok(_) => ExportStatus::Completed,
                Err(e) => ExportStatus::Failed(format!("{e:?}")),
            };

            self.export_history.insert(
                0,
                ExportHistoryEntry {
                    preset_name: name.clone(),
                    platform,
                    status,
                    output_path: result
                        .as_ref()
                        .map(|r| r.output_path.clone())
                        .unwrap_or_default(),
                },
            );

            results.push((name, result));
        }

        self.export_history.truncate(Self::MAX_HISTORY);
        results
    }
}

/// Status of an export operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExportStatus {
    /// Export is queued / not yet started.
    Pending,
    /// Export is currently in progress.
    InProgress,
    /// Export completed successfully.
    Completed,
    /// Export failed with an error message.
    Failed(String),
}

/// A record of a past export operation.
#[derive(Debug, Clone)]
pub struct ExportHistoryEntry {
    /// Name of the preset used.
    pub preset_name: String,
    /// Target platform.
    pub platform: ExportPlatform,
    /// Result status.
    pub status: ExportStatus,
    /// Output path (empty if failed).
    pub output_path: String,
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
        assert!(mac_preset
            .custom_properties
            .contains_key("bundle_identifier"));

        let android_preset = ExportPreset::new("Droid", ExportPlatform::Android);
        assert!(android_preset.custom_properties.contains_key("min_sdk"));

        let web_preset = ExportPreset::new("Web", ExportPlatform::Web);
        assert!(web_preset
            .custom_properties
            .contains_key("canvas_resize_policy"));
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

    // -- Duplicate preset --

    #[test]
    fn duplicate_preset_creates_copy() {
        let mut dialog = ExportDialog::new();
        dialog
            .add_preset(ExportPreset::new("Win", ExportPlatform::Windows))
            .unwrap();
        let idx = dialog.duplicate_preset(0).unwrap();
        assert_eq!(idx, 1);
        assert_eq!(dialog.preset_count(), 2);
        assert_eq!(dialog.presets()[1].name, "Win (copy)");
        assert_eq!(dialog.presets()[1].platform, ExportPlatform::Windows);
        assert_eq!(dialog.selected_index(), Some(1));
    }

    #[test]
    fn duplicate_preset_increments_copy_number() {
        let mut dialog = ExportDialog::new();
        dialog
            .add_preset(ExportPreset::new("A", ExportPlatform::Linux))
            .unwrap();
        dialog.duplicate_preset(0).unwrap();
        // Now we have "A" and "A (copy)", duplicate again
        dialog.duplicate_preset(0).unwrap();
        assert_eq!(dialog.presets()[2].name, "A (copy 2)");
    }

    #[test]
    fn duplicate_preset_preserves_settings() {
        let mut dialog = ExportDialog::new();
        let mut preset = ExportPreset::new("Mac", ExportPlatform::MacOS);
        preset.build_profile = ExportBuildProfile::Debug;
        preset.app_name = "TestApp".into();
        preset.icon_path = "icon.icns".into();
        dialog.add_preset(preset).unwrap();

        dialog.duplicate_preset(0).unwrap();
        let dup = &dialog.presets()[1];
        assert_eq!(dup.build_profile, ExportBuildProfile::Debug);
        assert_eq!(dup.app_name, "TestApp");
        assert_eq!(dup.icon_path, "icon.icns");
        assert_eq!(dup.platform, ExportPlatform::MacOS);
    }

    #[test]
    fn duplicate_preset_invalid_index() {
        let mut dialog = ExportDialog::new();
        assert!(dialog.duplicate_preset(0).is_err());
    }

    // -- Rename preset --

    #[test]
    fn rename_preset_success() {
        let mut dialog = ExportDialog::new();
        dialog
            .add_preset(ExportPreset::new("Old", ExportPlatform::Windows))
            .unwrap();
        dialog.rename_preset(0, "New").unwrap();
        assert_eq!(dialog.presets()[0].name, "New");
    }

    #[test]
    fn rename_preset_empty_rejected() {
        let mut dialog = ExportDialog::new();
        dialog
            .add_preset(ExportPreset::new("Test", ExportPlatform::Linux))
            .unwrap();
        let err = dialog.rename_preset(0, "").unwrap_err();
        assert_eq!(err, ExportValidationError::EmptyName);
    }

    #[test]
    fn rename_preset_duplicate_rejected() {
        let mut dialog = ExportDialog::new();
        dialog
            .add_preset(ExportPreset::new("A", ExportPlatform::Windows))
            .unwrap();
        dialog
            .add_preset(ExportPreset::new("B", ExportPlatform::Linux))
            .unwrap();
        let err = dialog.rename_preset(1, "A").unwrap_err();
        assert_eq!(err, ExportValidationError::DuplicateName("A".into()));
    }

    #[test]
    fn rename_preset_same_name_ok() {
        let mut dialog = ExportDialog::new();
        dialog
            .add_preset(ExportPreset::new("A", ExportPlatform::Windows))
            .unwrap();
        // Renaming to the same name should succeed
        dialog.rename_preset(0, "A").unwrap();
        assert_eq!(dialog.presets()[0].name, "A");
    }

    // -- Quick-add desktop presets --

    #[test]
    fn add_desktop_presets_creates_three() {
        let mut dialog = ExportDialog::new();
        let added = dialog.add_desktop_presets();
        assert_eq!(added, 3);
        assert_eq!(dialog.preset_count(), 3);
        assert!(dialog
            .presets()
            .iter()
            .any(|p| p.platform == ExportPlatform::Windows));
        assert!(dialog
            .presets()
            .iter()
            .any(|p| p.platform == ExportPlatform::Linux));
        assert!(dialog
            .presets()
            .iter()
            .any(|p| p.platform == ExportPlatform::MacOS));
    }

    #[test]
    fn add_desktop_presets_skips_existing() {
        let mut dialog = ExportDialog::new();
        dialog
            .add_preset(ExportPreset::new(
                "Windows Desktop",
                ExportPlatform::Windows,
            ))
            .unwrap();
        let added = dialog.add_desktop_presets();
        assert_eq!(added, 2); // Linux + macOS
        assert_eq!(dialog.preset_count(), 3);
    }

    // -- Filter by platform --

    #[test]
    fn presets_for_platform() {
        let mut dialog = ExportDialog::new();
        dialog
            .add_preset(ExportPreset::new("Win1", ExportPlatform::Windows))
            .unwrap();
        dialog
            .add_preset(ExportPreset::new("Linux1", ExportPlatform::Linux))
            .unwrap();
        dialog
            .add_preset(ExportPreset::new("Win2", ExportPlatform::Windows))
            .unwrap();
        let wins = dialog.presets_for_platform(ExportPlatform::Windows);
        assert_eq!(wins.len(), 2);
        assert_eq!(wins[0].0, 0); // index
        assert_eq!(wins[1].0, 2);
    }

    // -- Find preset --

    #[test]
    fn find_preset_by_name() {
        let mut dialog = ExportDialog::new();
        dialog
            .add_preset(ExportPreset::new("A", ExportPlatform::Windows))
            .unwrap();
        dialog
            .add_preset(ExportPreset::new("B", ExportPlatform::Linux))
            .unwrap();
        assert_eq!(dialog.find_preset("B"), Some(1));
        assert_eq!(dialog.find_preset("C"), None);
    }

    // -- Serialize / Deserialize --

    #[test]
    fn serialize_presets_roundtrip() {
        let mut dialog = ExportDialog::new();
        let mut preset = ExportPreset::new("Win", ExportPlatform::Windows);
        preset.app_name = "TestApp".into();
        preset.build_profile = ExportBuildProfile::Debug;
        dialog.add_preset(preset).unwrap();
        dialog
            .add_preset(ExportPreset::new("Linux", ExportPlatform::Linux))
            .unwrap();

        let serialized = dialog.serialize_presets();
        assert!(serialized.contains("name=Win"));
        assert!(serialized.contains("platform=windows"));
        assert!(serialized.contains("build_profile=debug"));
        assert!(serialized.contains("app_name=TestApp"));
        assert!(serialized.contains("---"));
        assert!(serialized.contains("name=Linux"));

        let mut dialog2 = ExportDialog::new();
        let count = dialog2.deserialize_presets(&serialized);
        assert_eq!(count, 2);
        assert_eq!(dialog2.presets()[0].name, "Win");
        assert_eq!(dialog2.presets()[0].platform, ExportPlatform::Windows);
        assert_eq!(
            dialog2.presets()[0].build_profile,
            ExportBuildProfile::Debug
        );
        assert_eq!(dialog2.presets()[0].app_name, "TestApp");
        assert_eq!(dialog2.presets()[1].name, "Linux");
        assert_eq!(dialog2.presets()[1].platform, ExportPlatform::Linux);
    }

    #[test]
    fn serialize_includes_custom_properties() {
        let mut dialog = ExportDialog::new();
        dialog
            .add_preset(ExportPreset::new("Web", ExportPlatform::Web))
            .unwrap();
        let serialized = dialog.serialize_presets();
        assert!(serialized.contains("custom.canvas_resize_policy=adaptive"));
    }

    #[test]
    fn deserialize_empty_string() {
        let mut dialog = ExportDialog::new();
        let count = dialog.deserialize_presets("");
        assert_eq!(count, 0);
    }

    #[test]
    fn deserialize_invalid_platform_skipped() {
        let mut dialog = ExportDialog::new();
        let count = dialog.deserialize_presets("name=Bad\nplatform=dreamcast\n");
        assert_eq!(count, 0);
    }

    #[test]
    fn deserialize_missing_name_skipped() {
        let mut dialog = ExportDialog::new();
        let count = dialog.deserialize_presets("platform=windows\n");
        assert_eq!(count, 0);
    }

    // -- One-click desktop export batch --

    #[test]
    fn export_all_desktop() {
        let mut dialog = ExportDialog::new();
        dialog.add_desktop_presets();
        dialog
            .add_preset(ExportPreset::new("Web", ExportPlatform::Web))
            .unwrap();
        let results = dialog.export_all_desktop("TestGame");
        assert_eq!(results.len(), 3); // Windows, Linux, macOS only
        for (name, result) in &results {
            assert!(result.is_ok(), "Export failed for {name}");
        }
    }

    #[test]
    fn export_all_desktop_empty_presets() {
        let dialog = ExportDialog::new();
        let results = dialog.export_all_desktop("Game");
        assert!(results.is_empty());
    }

    #[test]
    fn quick_export_desktop_creates_presets() {
        let mut dialog = ExportDialog::new();
        assert_eq!(dialog.preset_count(), 0);
        let results = dialog.quick_export_desktop("TestGame");
        assert_eq!(results.len(), 3);
        assert_eq!(dialog.preset_count(), 3); // auto-created
        for (_, result) in &results {
            assert!(result.is_ok());
        }
    }

    #[test]
    fn quick_export_desktop_idempotent() {
        let mut dialog = ExportDialog::new();
        dialog.quick_export_desktop("Game");
        assert_eq!(dialog.preset_count(), 3);
        // Second call doesn't add more presets
        dialog.quick_export_desktop("Game");
        assert_eq!(dialog.preset_count(), 3);
    }

    #[test]
    fn desktop_presets_filter() {
        let mut dialog = ExportDialog::new();
        dialog.add_desktop_presets();
        dialog
            .add_preset(ExportPreset::new("Web", ExportPlatform::Web))
            .unwrap();
        dialog
            .add_preset(ExportPreset::new("Android", ExportPlatform::Android))
            .unwrap();
        let desktops = dialog.desktop_presets();
        assert_eq!(desktops.len(), 3);
        for (_, p) in &desktops {
            assert!(p.is_desktop());
        }
    }

    // -- Export history --

    #[test]
    fn export_with_history_records_success() {
        let mut dialog = ExportDialog::new();
        dialog
            .add_preset(ExportPreset::new("Win", ExportPlatform::Windows))
            .unwrap();
        let result = dialog.export_one_click_with_history(0, "Game");
        assert!(result.is_ok());
        assert_eq!(dialog.export_history().len(), 1);
        assert_eq!(dialog.export_history()[0].preset_name, "Win");
        assert_eq!(dialog.export_history()[0].status, ExportStatus::Completed);
        assert!(!dialog.export_history()[0].output_path.is_empty());
    }

    #[test]
    fn export_with_history_records_failure() {
        let mut dialog = ExportDialog::new();
        dialog
            .add_preset(ExportPreset::new("Web", ExportPlatform::Web))
            .unwrap();
        let result = dialog.export_one_click_with_history(0, "Game");
        assert!(result.is_err());
        assert_eq!(dialog.export_history().len(), 1);
        assert!(matches!(
            dialog.export_history()[0].status,
            ExportStatus::Failed(_)
        ));
    }

    #[test]
    fn export_history_most_recent_first() {
        let mut dialog = ExportDialog::new();
        dialog
            .add_preset(ExportPreset::new("Win", ExportPlatform::Windows))
            .unwrap();
        dialog
            .add_preset(ExportPreset::new("Linux", ExportPlatform::Linux))
            .unwrap();
        dialog.export_one_click_with_history(0, "Game");
        dialog.export_one_click_with_history(1, "Game");
        assert_eq!(dialog.export_history()[0].preset_name, "Linux");
        assert_eq!(dialog.export_history()[1].preset_name, "Win");
    }

    #[test]
    fn clear_export_history() {
        let mut dialog = ExportDialog::new();
        dialog
            .add_preset(ExportPreset::new("Win", ExportPlatform::Windows))
            .unwrap();
        dialog.export_one_click_with_history(0, "Game");
        assert_eq!(dialog.export_history().len(), 1);
        dialog.clear_export_history();
        assert!(dialog.export_history().is_empty());
    }

    #[test]
    fn quick_export_desktop_with_history() {
        let mut dialog = ExportDialog::new();
        let results = dialog.quick_export_desktop_with_history("TestGame");
        assert_eq!(results.len(), 3);
        assert_eq!(dialog.export_history().len(), 3);
        // Most recent is last exported (macOS)
        for entry in dialog.export_history() {
            assert_eq!(entry.status, ExportStatus::Completed);
        }
    }

    #[test]
    fn export_status_variants() {
        let pending = ExportStatus::Pending;
        let progress = ExportStatus::InProgress;
        let done = ExportStatus::Completed;
        let failed = ExportStatus::Failed("test error".into());
        assert_ne!(pending, progress);
        assert_ne!(done, failed);
    }

    #[test]
    fn serialize_includes_filters() {
        let mut dialog = ExportDialog::new();
        let mut preset = ExportPreset::new("Test", ExportPlatform::Linux);
        preset.include_filters = vec!["*.tscn".into(), "*.tres".into()];
        preset.exclude_filters = vec!["*.tmp".into()];
        dialog.add_preset(preset).unwrap();

        let serialized = dialog.serialize_presets();
        assert!(serialized.contains("include=*.tscn"));
        assert!(serialized.contains("include=*.tres"));
        assert!(serialized.contains("exclude=*.tmp"));

        let mut dialog2 = ExportDialog::new();
        dialog2.deserialize_presets(&serialized);
        assert_eq!(
            dialog2.presets()[0].include_filters,
            vec!["*.tscn", "*.tres"]
        );
        assert_eq!(dialog2.presets()[0].exclude_filters, vec!["*.tmp"]);
    }
}
