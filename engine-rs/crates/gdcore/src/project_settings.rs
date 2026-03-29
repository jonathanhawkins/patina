//! ProjectSettings singleton with `project.godot` file loading.
//!
//! Mirrors Godot's `ProjectSettings` autoload singleton, providing:
//! - Section/key based configuration (`"section/subsection/key"`)
//! - Loading from Godot's INI-style `project.godot` format
//! - Dynamic get/set with [`Variant`]-typed values
//! - Default value fallback
//! - Property iteration and section listing
//!
//! The `project.godot` file uses a format like:
//! ```text
//! ; Engine configuration file.
//! config_version=5
//!
//! [application]
//! config/name="My Game"
//! run/main_scene="res://main.tscn"
//!
//! [physics]
//! common/physics_ticks_per_second=60
//! ```

use std::collections::BTreeMap;
use std::path::Path;

// ---------------------------------------------------------------------------
// Variant (lightweight, for settings values)
// ---------------------------------------------------------------------------

/// A lightweight variant type for project settings values.
///
/// Simpler than the full `gdvariant::Variant` — only the types that appear
/// in `project.godot` files.
#[derive(Debug, Clone, PartialEq)]
pub enum SettingsValue {
    /// A boolean value.
    Bool(bool),
    /// An integer value.
    Int(i64),
    /// A floating-point value.
    Float(f64),
    /// A string value.
    String(String),
    /// A `Vector2(x, y)` value.
    Vector2(f64, f64),
    /// A `Vector3(x, y, z)` value.
    Vector3(f64, f64, f64),
    /// A `Color(r, g, b, a)` value.
    Color(f64, f64, f64, f64),
}

impl SettingsValue {
    /// Returns the value as a bool, if it is one.
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            SettingsValue::Bool(b) => Some(*b),
            _ => None,
        }
    }

    /// Returns the value as an integer, if it is one.
    pub fn as_int(&self) -> Option<i64> {
        match self {
            SettingsValue::Int(i) => Some(*i),
            _ => None,
        }
    }

    /// Returns the value as a float. Integers are promoted.
    pub fn as_float(&self) -> Option<f64> {
        match self {
            SettingsValue::Float(f) => Some(*f),
            SettingsValue::Int(i) => Some(*i as f64),
            _ => None,
        }
    }

    /// Returns the value as a string, if it is one.
    pub fn as_string(&self) -> Option<&str> {
        match self {
            SettingsValue::String(s) => Some(s),
            _ => None,
        }
    }
}

impl std::fmt::Display for SettingsValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SettingsValue::Bool(b) => write!(f, "{b}"),
            SettingsValue::Int(i) => write!(f, "{i}"),
            SettingsValue::Float(v) => write!(f, "{v}"),
            SettingsValue::String(s) => write!(f, "\"{s}\""),
            SettingsValue::Vector2(x, y) => write!(f, "Vector2({x}, {y})"),
            SettingsValue::Vector3(x, y, z) => write!(f, "Vector3({x}, {y}, {z})"),
            SettingsValue::Color(r, g, b, a) => write!(f, "Color({r}, {g}, {b}, {a})"),
        }
    }
}

// ---------------------------------------------------------------------------
// ProjectSettings
// ---------------------------------------------------------------------------

/// Project-wide settings singleton, mirroring Godot's `ProjectSettings`.
///
/// Settings are stored as `"section/key"` paths mapping to [`SettingsValue`].
/// The top-level section (before the first `/`) corresponds to the INI
/// `[section]` header in `project.godot`.
#[derive(Debug, Clone)]
pub struct ProjectSettings {
    /// All settings keyed by their full path (e.g. `"application/config/name"`).
    properties: BTreeMap<String, SettingsValue>,
    /// The `config_version` from the project file header.
    config_version: u32,
    /// Path to the loaded project file, if any.
    project_path: Option<String>,
}

impl Default for ProjectSettings {
    fn default() -> Self {
        let mut ps = Self {
            properties: BTreeMap::new(),
            config_version: 5,
            project_path: None,
        };
        // Godot defaults
        ps.set("application/config/name", SettingsValue::String("New Project".into()));
        ps.set("application/run/main_scene", SettingsValue::String(String::new()));
        ps.set("physics/common/physics_ticks_per_second", SettingsValue::Int(60));
        ps.set("physics/2d/default_gravity", SettingsValue::Float(980.0));
        ps.set("physics/3d/default_gravity", SettingsValue::Float(9.8));
        ps.set("display/window/size/viewport_width", SettingsValue::Int(1152));
        ps.set("display/window/size/viewport_height", SettingsValue::Int(648));
        ps.set("rendering/renderer/rendering_method", SettingsValue::String("forward_plus".into()));
        ps
    }
}

impl ProjectSettings {
    /// Creates a new `ProjectSettings` with Godot-compatible defaults.
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the config version.
    pub fn config_version(&self) -> u32 {
        self.config_version
    }

    /// Returns the project file path, if loaded from disk.
    pub fn project_path(&self) -> Option<&str> {
        self.project_path.as_deref()
    }

    // -- get / set ----------------------------------------------------------

    /// Gets a setting by path. Returns `None` if not set.
    pub fn get(&self, path: &str) -> Option<&SettingsValue> {
        self.properties.get(path)
    }

    /// Gets a setting, falling back to a default if not set.
    pub fn get_or<'a>(&'a self, path: &str, default: &'a SettingsValue) -> &'a SettingsValue {
        self.properties.get(path).unwrap_or(default)
    }

    /// Sets a setting by path, creating or overwriting it.
    pub fn set(&mut self, path: &str, value: SettingsValue) {
        self.properties.insert(path.to_string(), value);
    }

    /// Returns `true` if the setting exists.
    pub fn has(&self, path: &str) -> bool {
        self.properties.contains_key(path)
    }

    /// Removes a setting. Returns the old value if it existed.
    pub fn remove(&mut self, path: &str) -> Option<SettingsValue> {
        self.properties.remove(path)
    }

    /// Returns the total number of settings.
    pub fn property_count(&self) -> usize {
        self.properties.len()
    }

    // -- Convenience accessors ----------------------------------------------

    /// Returns the project name.
    pub fn project_name(&self) -> &str {
        self.get("application/config/name")
            .and_then(|v| v.as_string())
            .unwrap_or("New Project")
    }

    /// Returns the main scene path.
    pub fn main_scene(&self) -> &str {
        self.get("application/run/main_scene")
            .and_then(|v| v.as_string())
            .unwrap_or("")
    }

    /// Returns physics ticks per second.
    pub fn physics_ticks_per_second(&self) -> i64 {
        self.get("physics/common/physics_ticks_per_second")
            .and_then(|v| v.as_int())
            .unwrap_or(60)
    }

    // -- Iteration ----------------------------------------------------------

    /// Returns all setting paths.
    pub fn property_list(&self) -> Vec<&str> {
        self.properties.keys().map(|k| k.as_str()).collect()
    }

    /// Returns all unique top-level sections.
    pub fn sections(&self) -> Vec<String> {
        let mut seen = Vec::new();
        for key in self.properties.keys() {
            if let Some(section) = key.split('/').next() {
                if !seen.contains(&section.to_string()) {
                    seen.push(section.to_string());
                }
            }
        }
        seen
    }

    /// Returns all settings under a given section prefix.
    pub fn get_section(&self, prefix: &str) -> Vec<(&str, &SettingsValue)> {
        let prefix_slash = if prefix.ends_with('/') {
            prefix.to_string()
        } else {
            format!("{prefix}/")
        };
        self.properties
            .iter()
            .filter(|(k, _)| k.starts_with(&prefix_slash))
            .map(|(k, v)| (k.as_str(), v))
            .collect()
    }

    // -- Loading from project.godot format ----------------------------------

    /// Loads settings from a Godot `project.godot` INI-style file.
    pub fn load_godot_project(path: &Path) -> Result<Self, String> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| format!("failed to read {}: {e}", path.display()))?;
        let mut ps = Self::from_godot_format(&content)?;
        ps.project_path = Some(path.to_string_lossy().into_owned());
        Ok(ps)
    }

    /// Parses settings from a Godot project file string.
    pub fn from_godot_format(content: &str) -> Result<Self, String> {
        let mut ps = Self {
            properties: BTreeMap::new(),
            config_version: 5,
            project_path: None,
        };

        let mut current_section = String::new();

        for line in content.lines() {
            let trimmed = line.trim();

            // Skip comments and blank lines.
            if trimmed.is_empty() || trimmed.starts_with(';') || trimmed.starts_with('#') {
                continue;
            }

            // Section header: [section_name]
            if trimmed.starts_with('[') && trimmed.ends_with(']') {
                current_section = trimmed[1..trimmed.len() - 1].to_string();
                continue;
            }

            // Key=value pair
            if let Some(eq_pos) = trimmed.find('=') {
                let key = trimmed[..eq_pos].trim();
                let raw_value = trimmed[eq_pos + 1..].trim();

                // Top-level keys (before any section)
                if current_section.is_empty() {
                    if key == "config_version" {
                        ps.config_version = raw_value.parse().unwrap_or(5);
                    }
                    // Other top-level keys are stored as-is.
                    continue;
                }

                let full_path = format!("{}/{}", current_section, key);
                let value = parse_godot_value(raw_value);
                ps.properties.insert(full_path, value);
            }
        }

        Ok(ps)
    }

    /// Serializes settings back to Godot's INI-style format.
    pub fn to_godot_format(&self) -> String {
        let mut output = String::new();
        output.push_str("; Engine configuration file.\n");
        output.push_str(&format!("config_version={}\n\n", self.config_version));

        let sections = self.sections();
        for section in &sections {
            output.push_str(&format!("[{section}]\n\n"));
            for (key, value) in self.get_section(section) {
                // Strip the section prefix.
                let short_key = &key[section.len() + 1..];
                output.push_str(&format!("{short_key}={value}\n"));
            }
            output.push('\n');
        }

        output
    }
}

// ---------------------------------------------------------------------------
// Value parser
// ---------------------------------------------------------------------------

/// Parses a Godot project file value string into a [`SettingsValue`].
fn parse_godot_value(raw: &str) -> SettingsValue {
    let s = raw.trim();

    // Quoted string
    if s.starts_with('"') && s.ends_with('"') && s.len() >= 2 {
        return SettingsValue::String(s[1..s.len() - 1].to_string());
    }

    // Boolean
    if s == "true" {
        return SettingsValue::Bool(true);
    }
    if s == "false" {
        return SettingsValue::Bool(false);
    }

    // Vector2(x, y)
    if s.starts_with("Vector2(") && s.ends_with(')') {
        let inner = &s[8..s.len() - 1];
        let parts: Vec<&str> = inner.split(',').collect();
        if parts.len() == 2 {
            if let (Ok(x), Ok(y)) = (parts[0].trim().parse(), parts[1].trim().parse()) {
                return SettingsValue::Vector2(x, y);
            }
        }
    }

    // Vector3(x, y, z)
    if s.starts_with("Vector3(") && s.ends_with(')') {
        let inner = &s[8..s.len() - 1];
        let parts: Vec<&str> = inner.split(',').collect();
        if parts.len() == 3 {
            if let (Ok(x), Ok(y), Ok(z)) = (
                parts[0].trim().parse(),
                parts[1].trim().parse(),
                parts[2].trim().parse(),
            ) {
                return SettingsValue::Vector3(x, y, z);
            }
        }
    }

    // Color(r, g, b, a)
    if s.starts_with("Color(") && s.ends_with(')') {
        let inner = &s[6..s.len() - 1];
        let parts: Vec<&str> = inner.split(',').collect();
        if parts.len() == 4 {
            if let (Ok(r), Ok(g), Ok(b), Ok(a)) = (
                parts[0].trim().parse(),
                parts[1].trim().parse(),
                parts[2].trim().parse(),
                parts[3].trim().parse(),
            ) {
                return SettingsValue::Color(r, g, b, a);
            }
        }
    }

    // Integer (no decimal point)
    if let Ok(i) = s.parse::<i64>() {
        return SettingsValue::Int(i);
    }

    // Float
    if let Ok(f) = s.parse::<f64>() {
        return SettingsValue::Float(f);
    }

    // Fallback: treat as string (unquoted)
    SettingsValue::String(s.to_string())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_godot() {
        let ps = ProjectSettings::new();
        assert_eq!(ps.project_name(), "New Project");
        assert_eq!(ps.main_scene(), "");
        assert_eq!(ps.physics_ticks_per_second(), 60);
        assert_eq!(ps.config_version(), 5);
    }

    #[test]
    fn get_set_has_remove() {
        let mut ps = ProjectSettings::new();
        ps.set("custom/my_setting", SettingsValue::Int(42));
        assert!(ps.has("custom/my_setting"));
        assert_eq!(ps.get("custom/my_setting").unwrap().as_int(), Some(42));

        let old = ps.remove("custom/my_setting");
        assert_eq!(old.unwrap().as_int(), Some(42));
        assert!(!ps.has("custom/my_setting"));
    }

    #[test]
    fn get_or_returns_default() {
        let ps = ProjectSettings::new();
        let default = SettingsValue::Int(999);
        let val = ps.get_or("nonexistent/key", &default);
        assert_eq!(val.as_int(), Some(999));
    }

    #[test]
    fn sections_list() {
        let ps = ProjectSettings::new();
        let sections = ps.sections();
        assert!(sections.contains(&"application".to_string()));
        assert!(sections.contains(&"physics".to_string()));
        assert!(sections.contains(&"display".to_string()));
        assert!(sections.contains(&"rendering".to_string()));
    }

    #[test]
    fn get_section_returns_matching() {
        let ps = ProjectSettings::new();
        let physics = ps.get_section("physics");
        assert!(physics.len() >= 3);
        assert!(physics.iter().any(|(k, _)| *k == "physics/common/physics_ticks_per_second"));
    }

    #[test]
    fn parse_godot_format_basic() {
        let content = r#"; Engine configuration file.
config_version=5

[application]
config/name="My Game"
run/main_scene="res://main.tscn"

[physics]
common/physics_ticks_per_second=120
2d/default_gravity=980.0
"#;
        let ps = ProjectSettings::from_godot_format(content).unwrap();
        assert_eq!(ps.config_version(), 5);
        assert_eq!(
            ps.get("application/config/name").unwrap().as_string(),
            Some("My Game")
        );
        assert_eq!(
            ps.get("application/run/main_scene").unwrap().as_string(),
            Some("res://main.tscn")
        );
        assert_eq!(
            ps.get("physics/common/physics_ticks_per_second").unwrap().as_int(),
            Some(120)
        );
        assert_eq!(
            ps.get("physics/2d/default_gravity").unwrap().as_float(),
            Some(980.0)
        );
    }

    #[test]
    fn parse_boolean_values() {
        let content = "[editor]\nplugin/enabled=true\nother/disabled=false\n";
        let ps = ProjectSettings::from_godot_format(content).unwrap();
        assert_eq!(ps.get("editor/plugin/enabled").unwrap().as_bool(), Some(true));
        assert_eq!(ps.get("editor/other/disabled").unwrap().as_bool(), Some(false));
    }

    #[test]
    fn parse_vector2_value() {
        let content = "[display]\nwindow/size=Vector2(1920, 1080)\n";
        let ps = ProjectSettings::from_godot_format(content).unwrap();
        match ps.get("display/window/size").unwrap() {
            SettingsValue::Vector2(x, y) => {
                assert!((x - 1920.0).abs() < f64::EPSILON);
                assert!((y - 1080.0).abs() < f64::EPSILON);
            }
            other => panic!("expected Vector2, got {other:?}"),
        }
    }

    #[test]
    fn parse_color_value() {
        let content = "[rendering]\nclear_color=Color(0.3, 0.3, 0.3, 1.0)\n";
        let ps = ProjectSettings::from_godot_format(content).unwrap();
        match ps.get("rendering/clear_color").unwrap() {
            SettingsValue::Color(r, g, b, a) => {
                assert!((r - 0.3).abs() < 0.001);
                assert!((a - 1.0).abs() < f64::EPSILON);
                let _ = (g, b); // checked implicitly
            }
            other => panic!("expected Color, got {other:?}"),
        }
    }

    #[test]
    fn skip_comments_and_blanks() {
        let content = "; comment\n# also comment\n\n[app]\nname=\"Test\"\n";
        let ps = ProjectSettings::from_godot_format(content).unwrap();
        assert_eq!(ps.get("app/name").unwrap().as_string(), Some("Test"));
        assert_eq!(ps.property_count(), 1);
    }

    #[test]
    fn roundtrip_to_godot_format() {
        let mut ps = ProjectSettings::new();
        ps.set("application/config/name", SettingsValue::String("Round Trip".into()));
        let output = ps.to_godot_format();
        let ps2 = ProjectSettings::from_godot_format(&output).unwrap();
        assert_eq!(ps2.get("application/config/name").unwrap().as_string(), Some("Round Trip"));
    }

    #[test]
    fn load_godot_project_file() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("project.godot");
        std::fs::write(
            &path,
            "; Engine configuration file.\nconfig_version=5\n\n[application]\nconfig/name=\"File Test\"\n",
        )
        .unwrap();

        let ps = ProjectSettings::load_godot_project(&path).unwrap();
        assert_eq!(
            ps.get("application/config/name").unwrap().as_string(),
            Some("File Test")
        );
        assert!(ps.project_path().is_some());
    }

    #[test]
    fn load_missing_file_errors() {
        let result = ProjectSettings::load_godot_project(Path::new("/nonexistent/project.godot"));
        assert!(result.is_err());
    }

    #[test]
    fn property_list_and_count() {
        let mut ps = ProjectSettings::new();
        let initial = ps.property_count();
        ps.set("custom/foo", SettingsValue::Int(1));
        ps.set("custom/bar", SettingsValue::Int(2));
        assert_eq!(ps.property_count(), initial + 2);
        assert!(ps.property_list().contains(&"custom/foo"));
    }

    #[test]
    fn settings_value_display() {
        assert_eq!(format!("{}", SettingsValue::Bool(true)), "true");
        assert_eq!(format!("{}", SettingsValue::Int(42)), "42");
        assert_eq!(format!("{}", SettingsValue::String("hello".into())), "\"hello\"");
        assert_eq!(
            format!("{}", SettingsValue::Vector2(1.0, 2.0)),
            "Vector2(1, 2)"
        );
    }

    #[test]
    fn overwrite_setting() {
        let mut ps = ProjectSettings::new();
        ps.set("application/config/name", SettingsValue::String("First".into()));
        ps.set("application/config/name", SettingsValue::String("Second".into()));
        assert_eq!(ps.project_name(), "Second");
    }

    #[test]
    fn int_promoted_to_float() {
        let v = SettingsValue::Int(60);
        assert_eq!(v.as_float(), Some(60.0));
    }
}
