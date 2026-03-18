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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EditorTheme {
    /// Dark theme (default).
    Dark,
    /// Light theme.
    Light,
}

impl Default for EditorTheme {
    fn default() -> Self {
        Self::Dark
    }
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
        serde_json::from_str(&data)
            .map_err(|e| EngineError::Parse(format!("editor settings: {e}")))
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

/// Project-wide settings, analogous to Godot's `project.godot`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectSettings {
    /// The project's display name.
    pub project_name: String,
    /// Path to the main scene (e.g. `"res://scenes/main.tscn"`).
    pub main_scene_path: String,
    /// Physics ticks per second.
    pub physics_fps: u32,
    /// Default gravity in pixels/sec² (2D) or m/sec² (3D).
    pub default_gravity: f64,
    /// Input action map: action name -> list of input events.
    pub input_map: HashMap<String, Vec<String>>,
}

impl Default for ProjectSettings {
    fn default() -> Self {
        Self {
            project_name: "New Project".to_string(),
            main_scene_path: String::new(),
            physics_fps: 60,
            default_gravity: 980.0,
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
    fn load_invalid_json_fails() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("bad.json");
        std::fs::write(&path, "not json").unwrap();

        assert!(EditorSettings::load(&path).is_err());
        assert!(ProjectSettings::load(&path).is_err());
    }
}
