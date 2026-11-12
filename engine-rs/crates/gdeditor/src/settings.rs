//! Editor and project settings.
//!
//! Provides [`EditorSettings`] for user preferences and
//! [`ProjectSettings`] for project-wide configuration, both
//! serializable as JSON.

use std::collections::HashMap;
use std::path::Path;

use gdcore::error::{EngineError, EngineResult};
use serde::{Deserialize, Serialize};

/// The editor color theme.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum EditorTheme {
    /// Dark theme (default).
    #[default]
    Dark,
    /// Light theme.
    Light,
}

/// User-level editor preferences.
///
/// Persisted as JSON so settings survive between sessions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EditorSettings {
    /// Recently opened files.
    pub recent_files: Vec<String>,
    /// Editor window size as `(width, height)`.
    pub window_size: (u32, u32),
    /// The color theme.
    pub theme: EditorTheme,
    /// Whether auto-save is enabled.
    pub auto_save: bool,
}

impl Default for EditorSettings {
    fn default() -> Self {
        Self {
            recent_files: Vec::new(),
            window_size: (1280, 720),
            theme: EditorTheme::Dark,
            auto_save: true,
        }
    }
}

impl EditorSettings {
    /// Creates settings with default values.
    pub fn new() -> Self {
        Self::default()
    }

    /// Loads settings from a JSON file.
    pub fn load(path: &Path) -> EngineResult<Self> {
        let data = std::fs::read_to_string(path).map_err(EngineError::Io)?;
        serde_json::from_str(&data).map_err(|e| EngineError::Parse(format!("editor settings: {e}")))
    }

    /// Saves settings to a JSON file.
    pub fn save(&self, path: &Path) -> EngineResult<()> {
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| EngineError::Parse(format!("serialize: {e}")))?;
        std::fs::write(path, json).map_err(EngineError::Io)
    }

    /// Adds a file to the recent files list (most recent first).
    ///
    /// Removes duplicates and caps the list at 20 entries.
    pub fn add_recent_file(&mut self, path: &str) {
        self.recent_files.retain(|p| p != path);
        self.recent_files.insert(0, path.to_string());
        self.recent_files.truncate(20);
    }
}

/// Configuration for launching an external code editor.
///
/// Mirrors Godot's external editor settings: an executable path and an
/// argument template with `{file}`, `{line}`, `{col}` placeholders.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ExternalEditorConfig {
    /// Path to the editor executable (e.g. `"code"`, `"/usr/bin/vim"`).
    pub exec_path: String,
    /// Argument template tokens.  Each may contain `{file}`, `{line}`, `{col}`.
    #[serde(default)]
    pub exec_args: Vec<String>,
}

impl ExternalEditorConfig {
    /// Returns `true` when a non-empty executable path has been set.
    pub fn is_configured(&self) -> bool {
        !self.exec_path.is_empty()
    }

    /// Expands placeholder tokens and returns the final argument list.
    pub fn build_args(&self, file: &str, line: usize, col: usize) -> Vec<String> {
        self.exec_args
            .iter()
            .map(|a| {
                a.replace("{file}", file)
                    .replace("{line}", &line.to_string())
                    .replace("{col}", &col.to_string())
            })
            .collect()
    }
}

/// Project-wide settings, analogous to Godot's `project.godot`.
///
/// Settings are organized by category: Application, Display, Physics,
/// Audio, and Rendering.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectSettings {
    // ---- Application ----
    /// The project's display name.
    pub project_name: String,
    /// Path to the main scene (e.g. `"res://scenes/main.tscn"`).
    pub main_scene_path: String,
    /// Project description.
    #[serde(default)]
    pub description: String,
    /// Path to the project icon.
    #[serde(default)]
    pub icon_path: String,

    // ---- Display ----
    /// Display resolution width.
    #[serde(default = "default_resolution_w")]
    pub resolution_w: u32,
    /// Display resolution height.
    #[serde(default = "default_resolution_h")]
    pub resolution_h: u32,
    /// Stretch mode: "disabled", "canvas_items", or "viewport".
    #[serde(default = "default_stretch_mode")]
    pub stretch_mode: String,
    /// Stretch aspect: "ignore", "keep", "keep_width", "keep_height", "expand".
    #[serde(default = "default_stretch_aspect")]
    pub stretch_aspect: String,
    /// Whether to start in fullscreen.
    #[serde(default)]
    pub fullscreen: bool,
    /// Whether V-Sync is enabled.
    #[serde(default = "default_true")]
    pub vsync: bool,

    // ---- Physics ----
    /// Physics ticks per second.
    pub physics_fps: u32,
    /// Default gravity in pixels/sec² (2D) or m/sec² (3D).
    pub default_gravity: f64,
    /// Default linear damping.
    #[serde(default = "default_linear_damp")]
    pub default_linear_damp: f64,
    /// Default angular damping.
    #[serde(default = "default_angular_damp")]
    pub default_angular_damp: f64,

    // ---- Audio ----
    /// Default audio bus layout resource path.
    #[serde(default = "default_bus_layout")]
    pub default_bus_layout: String,
    /// Master volume in dB.
    #[serde(default)]
    pub master_volume_db: f64,
    /// Whether audio input capture is enabled.
    #[serde(default)]
    pub enable_audio_input: bool,

    // ---- Rendering ----
    /// Renderer backend: "forward_plus", "mobile", or "compatibility".
    #[serde(default = "default_renderer")]
    pub renderer: String,
    /// Anti-aliasing mode: "disabled", "fxaa", "msaa_2x", "msaa_4x", "msaa_8x".
    #[serde(default = "default_aa")]
    pub anti_aliasing: String,
    /// Default environment resource path.
    #[serde(default)]
    pub environment_default: String,

    // ---- Input ----
    /// Input action map: action name -> list of input events.
    pub input_map: HashMap<String, Vec<String>>,
}

fn default_resolution_w() -> u32 {
    1152
}
fn default_resolution_h() -> u32 {
    648
}
fn default_stretch_mode() -> String {
    "disabled".to_string()
}
fn default_stretch_aspect() -> String {
    "keep".to_string()
}
fn default_true() -> bool {
    true
}
fn default_linear_damp() -> f64 {
    0.1
}
fn default_angular_damp() -> f64 {
    1.0
}
fn default_bus_layout() -> String {
    "res://default_bus_layout.tres".to_string()
}
fn default_renderer() -> String {
    "forward_plus".to_string()
}
fn default_aa() -> String {
    "disabled".to_string()
}

impl Default for ProjectSettings {
    fn default() -> Self {
        Self {
            project_name: "New Project".to_string(),
            main_scene_path: String::new(),
            description: String::new(),
            icon_path: String::new(),
            resolution_w: default_resolution_w(),
            resolution_h: default_resolution_h(),
            stretch_mode: default_stretch_mode(),
            stretch_aspect: default_stretch_aspect(),
            fullscreen: false,
            vsync: true,
            physics_fps: 60,
            default_gravity: 980.0,
            default_linear_damp: default_linear_damp(),
            default_angular_damp: default_angular_damp(),
            default_bus_layout: default_bus_layout(),
            master_volume_db: 0.0,
            enable_audio_input: false,
            renderer: default_renderer(),
            anti_aliasing: default_aa(),
            environment_default: String::new(),
            input_map: HashMap::new(),
        }
    }
}

impl ProjectSettings {
    /// Creates project settings with default values.
    pub fn new() -> Self {
        Self::default()
    }

    /// Loads project settings from a JSON file.
    pub fn load(path: &Path) -> EngineResult<Self> {
        let data = std::fs::read_to_string(path).map_err(EngineError::Io)?;
        serde_json::from_str(&data)
            .map_err(|e| EngineError::Parse(format!("project settings: {e}")))
    }

    /// Saves project settings to a JSON file.
    pub fn save(&self, path: &Path) -> EngineResult<()> {
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| EngineError::Parse(format!("serialize: {e}")))?;
        std::fs::write(path, json).map_err(EngineError::Io)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn editor_settings_defaults() {
        let settings = EditorSettings::new();
        assert_eq!(settings.window_size, (1280, 720));
        assert_eq!(settings.theme, EditorTheme::Dark);
        assert!(settings.auto_save);
        assert!(settings.recent_files.is_empty());
    }

    #[test]
    fn editor_settings_save_load_roundtrip() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("editor.json");

        let mut settings = EditorSettings::new();
        settings.theme = EditorTheme::Light;
        settings.window_size = (1920, 1080);
        settings.add_recent_file("res://main.tscn");
        settings.save(&path).unwrap();

        let loaded = EditorSettings::load(&path).unwrap();
        assert_eq!(loaded.theme, EditorTheme::Light);
        assert_eq!(loaded.window_size, (1920, 1080));
        assert_eq!(loaded.recent_files, vec!["res://main.tscn"]);
    }

    #[test]
    fn recent_files_deduplication() {
        let mut settings = EditorSettings::new();
        settings.add_recent_file("a.tscn");
        settings.add_recent_file("b.tscn");
        settings.add_recent_file("a.tscn"); // duplicate, should move to front
        assert_eq!(settings.recent_files, vec!["a.tscn", "b.tscn"]);
    }

    #[test]
    fn recent_files_cap_at_20() {
        let mut settings = EditorSettings::new();
        for i in 0..25 {
            settings.add_recent_file(&format!("file_{i}.tscn"));
        }
        assert_eq!(settings.recent_files.len(), 20);
    }

    #[test]
    fn project_settings_defaults() {
        let settings = ProjectSettings::new();
        assert_eq!(settings.project_name, "New Project");
        assert_eq!(settings.physics_fps, 60);
        assert_eq!(settings.default_gravity, 980.0);
        assert!(settings.main_scene_path.is_empty());
    }

    #[test]
    fn project_settings_save_load_roundtrip() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("project.json");

        let mut settings = ProjectSettings::new();
        settings.project_name = "My Game".to_string();
        settings.main_scene_path = "res://main.tscn".to_string();
        settings.physics_fps = 120;
        settings
            .input_map
            .insert("jump".to_string(), vec!["Key_Space".to_string()]);
        settings.save(&path).unwrap();

        let loaded = ProjectSettings::load(&path).unwrap();
        assert_eq!(loaded.project_name, "My Game");
        assert_eq!(loaded.main_scene_path, "res://main.tscn");
        assert_eq!(loaded.physics_fps, 120);
        assert!(loaded.input_map.contains_key("jump"));
    }

    #[test]
    fn project_settings_categorized_defaults() {
        let settings = ProjectSettings::new();
        // Display
        assert_eq!(settings.resolution_w, 1152);
        assert_eq!(settings.resolution_h, 648);
        assert_eq!(settings.stretch_mode, "disabled");
        assert_eq!(settings.stretch_aspect, "keep");
        assert!(!settings.fullscreen);
        assert!(settings.vsync);
        // Physics
        assert_eq!(settings.default_linear_damp, 0.1);
        assert_eq!(settings.default_angular_damp, 1.0);
        // Audio
        assert_eq!(settings.default_bus_layout, "res://default_bus_layout.tres");
        assert_eq!(settings.master_volume_db, 0.0);
        assert!(!settings.enable_audio_input);
        // Rendering
        assert_eq!(settings.renderer, "forward_plus");
        assert_eq!(settings.anti_aliasing, "disabled");
        assert!(settings.environment_default.is_empty());
    }

    #[test]
    fn project_settings_categorized_roundtrip() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("project_cat.json");

        let mut settings = ProjectSettings::new();
        settings.description = "A test game".to_string();
        settings.resolution_w = 1920;
        settings.resolution_h = 1080;
        settings.renderer = "mobile".to_string();
        settings.anti_aliasing = "fxaa".to_string();
        settings.master_volume_db = -6.0;
        settings.save(&path).unwrap();

        let loaded = ProjectSettings::load(&path).unwrap();
        assert_eq!(loaded.description, "A test game");
        assert_eq!(loaded.resolution_w, 1920);
        assert_eq!(loaded.resolution_h, 1080);
        assert_eq!(loaded.renderer, "mobile");
        assert_eq!(loaded.anti_aliasing, "fxaa");
        assert_eq!(loaded.master_volume_db, -6.0);
    }

    #[test]
    fn project_settings_backward_compat_load() {
        // Old-format JSON (only original fields) should still deserialize
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("old_format.json");
        std::fs::write(
            &path,
            r#"{
                "project_name": "OldGame",
                "main_scene_path": "res://main.tscn",
                "physics_fps": 60,
                "default_gravity": 980.0,
                "input_map": {}
            }"#,
        )
        .unwrap();

        let loaded = ProjectSettings::load(&path).unwrap();
        assert_eq!(loaded.project_name, "OldGame");
        // New fields should have defaults
        assert_eq!(loaded.resolution_w, 1152);
        assert_eq!(loaded.renderer, "forward_plus");
        assert!(loaded.vsync);
    }

    #[test]
    fn load_invalid_json_fails() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("bad.json");
        std::fs::write(&path, "not json").unwrap();

        assert!(EditorSettings::load(&path).is_err());
        assert!(ProjectSettings::load(&path).is_err());
    }
}
