//! Godot `project.godot` file parser and project configuration loader.
//!
//! Provides [`GodotProjectFile`] for parsing the INI-like `project.godot`
//! format, [`ProjectConfig`] for extracted engine settings, and
//! [`ProjectLoader`] for loading a full project from disk.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use gdcore::error::{EngineError, EngineResult};
use gdcore::math::Vector2;

use crate::importers::resolve_res_path;

// ---------------------------------------------------------------------------
// GodotProjectFile — raw INI-like parser
// ---------------------------------------------------------------------------

/// A parsed Godot `project.godot` file as raw sections and key-value pairs.
///
/// The file uses an INI-like format with `[section]` headers and `key=value`
/// lines. Values may be quoted strings, Godot variant expressions, or bare
/// tokens.
#[derive(Debug, Clone, Default)]
pub struct GodotProjectFile {
    /// Sections mapped to their key-value pairs.
    pub sections: HashMap<String, HashMap<String, String>>,
}

impl GodotProjectFile {
    /// Parses a `project.godot` file from its text contents.
    pub fn parse(content: &str) -> EngineResult<GodotProjectFile> {
        let mut sections: HashMap<String, HashMap<String, String>> = HashMap::new();
        let mut current_section = String::new();
        // For multi-line values (e.g. input actions with {...}).
        let mut pending_key: Option<String> = None;
        let mut pending_value = String::new();
        let mut brace_depth: i32 = 0;

        for line in content.lines() {
            let trimmed = line.trim();

            // If we're accumulating a multi-line value, append until braces balance.
            if pending_key.is_some() {
                pending_value.push('\n');
                pending_value.push_str(trimmed);
                for ch in trimmed.chars() {
                    match ch {
                        '{' | '[' => brace_depth += 1,
                        '}' | ']' => brace_depth -= 1,
                        _ => {}
                    }
                }
                if brace_depth <= 0 {
                    let key = pending_key.take().unwrap();
                    sections
                        .entry(current_section.clone())
                        .or_default()
                        .insert(key, pending_value.clone());
                    pending_value.clear();
                    brace_depth = 0;
                }
                continue;
            }

            // Skip empty lines and comments.
            if trimmed.is_empty() || trimmed.starts_with(';') || trimmed.starts_with('#') {
                continue;
            }

            // Section header: [name]
            if trimmed.starts_with('[') && trimmed.ends_with(']') {
                current_section = trimmed[1..trimmed.len() - 1].to_string();
                sections.entry(current_section.clone()).or_default();
                continue;
            }

            // Key=value (first `=` splits key from value).
            if let Some((key, value)) = trimmed.split_once('=') {
                let key = key.trim().to_string();
                let value = value.trim().to_string();

                // Check if the value starts an unbalanced brace/bracket block.
                let mut depth: i32 = 0;
                for ch in value.chars() {
                    match ch {
                        '{' | '[' => depth += 1,
                        '}' | ']' => depth -= 1,
                        _ => {}
                    }
                }

                if depth > 0 {
                    // Multi-line value — accumulate.
                    pending_key = Some(key);
                    pending_value = value;
                    brace_depth = depth;
                } else {
                    sections
                        .entry(current_section.clone())
                        .or_default()
                        .insert(key, value);
                }
            }
        }

        // Flush any remaining pending value.
        if let Some(key) = pending_key {
            sections
                .entry(current_section.clone())
                .or_default()
                .insert(key, pending_value);
        }

        Ok(GodotProjectFile { sections })
    }

    /// Gets a value from a specific section.
    pub fn get(&self, section: &str, key: &str) -> Option<&str> {
        self.sections
            .get(section)
            .and_then(|s| s.get(key))
            .map(|s| s.as_str())
    }
}

// ---------------------------------------------------------------------------
// ProjectConfig
// ---------------------------------------------------------------------------

/// Extracted project configuration from a `project.godot` file.
#[derive(Debug, Clone)]
pub struct ProjectConfig {
    /// The config file version (typically 5 for Godot 4.x).
    pub config_version: u32,
    /// The project's display name.
    pub project_name: String,
    /// The main scene path (res:// format).
    pub main_scene: String,
    /// The project icon path.
    pub icon_path: Option<String>,
    /// Viewport width in pixels.
    pub viewport_width: u32,
    /// Viewport height in pixels.
    pub viewport_height: u32,
    /// Physics tick rate.
    pub physics_ticks_per_second: u32,
    /// Default gravity magnitude.
    pub default_gravity: f32,
    /// Default gravity direction vector.
    pub default_gravity_vector: Vector2,
}

impl Default for ProjectConfig {
    fn default() -> Self {
        Self {
            config_version: 5,
            project_name: String::new(),
            main_scene: String::new(),
            icon_path: None,
            viewport_width: 1152,
            viewport_height: 648,
            physics_ticks_per_second: 60,
            default_gravity: 980.0,
            default_gravity_vector: Vector2::new(0.0, 1.0),
        }
    }
}

impl ProjectConfig {
    /// Extracts a [`ProjectConfig`] from a parsed [`GodotProjectFile`].
    pub fn from_project_file(pf: &GodotProjectFile) -> EngineResult<ProjectConfig> {
        let mut config = ProjectConfig::default();

        // [application] section
        if let Some(name) = pf.get("application", "config/name") {
            config.project_name = unquote(name);
        }
        if let Some(scene) = pf.get("application", "run/main_scene") {
            config.main_scene = unquote(scene);
        }
        if let Some(icon) = pf.get("application", "config/icon") {
            config.icon_path = Some(unquote(icon));
        }

        // Top-level config_version (often in the unnamed/root section or "" section)
        if let Some(ver) = pf.get("", "config_version") {
            config.config_version = ver.parse::<u32>().unwrap_or(config.config_version);
        }

        // [display] section
        if let Some(w) = pf.get("display", "window/size/viewport_width") {
            config.viewport_width = w.parse::<u32>().unwrap_or(config.viewport_width);
        }
        if let Some(h) = pf.get("display", "window/size/viewport_height") {
            config.viewport_height = h.parse::<u32>().unwrap_or(config.viewport_height);
        }

        // [physics] section
        if let Some(tps) = pf.get("physics", "common/physics_ticks_per_second") {
            config.physics_ticks_per_second = tps
                .parse::<u32>()
                .unwrap_or(config.physics_ticks_per_second);
        }
        if let Some(g) = pf.get("physics", "2d/default_gravity") {
            config.default_gravity = g.parse::<f32>().unwrap_or(config.default_gravity);
        }
        if let Some(gv) = pf.get("physics", "2d/default_gravity_vector") {
            config.default_gravity_vector =
                parse_vector2(gv).unwrap_or(config.default_gravity_vector);
        }

        Ok(config)
    }
}

// ---------------------------------------------------------------------------
// AutoloadEntry
// ---------------------------------------------------------------------------

/// A project autoload entry.
///
/// In `project.godot`, autoloads are in the `[autoload]` section:
/// ```text
/// [autoload]
/// Global="*res://scripts/global.gd"
/// Config="res://scenes/config.tscn"
/// ```
/// A `*` prefix marks a singleton.
#[derive(Debug, Clone, PartialEq)]
pub struct AutoloadEntry {
    /// The autoload name (e.g. `"Global"`).
    pub name: String,
    /// The resource path (res:// format).
    pub path: String,
    /// Whether the autoload is a singleton (prefixed with `*`).
    pub is_singleton: bool,
}

// ---------------------------------------------------------------------------
// InputMapEntry
// ---------------------------------------------------------------------------

/// A simplified input map entry.
///
/// Extracts action names and key codes from the `[input]` section.
#[derive(Debug, Clone, PartialEq)]
pub struct InputMapEntry {
    /// The action name (e.g. `"ui_accept"`).
    pub action_name: String,
    /// Raw event descriptions extracted from the value.
    pub events: Vec<String>,
}

// ---------------------------------------------------------------------------
// ProjectLoader
// ---------------------------------------------------------------------------

/// Loads and interprets a Godot project from a directory on disk.
///
/// Reads `project.godot` from the project root, parses it, and provides
/// access to configuration, autoloads, and input map entries.
#[derive(Debug)]
pub struct ProjectLoader {
    project_root: PathBuf,
    project_file: GodotProjectFile,
    config: ProjectConfig,
}

impl ProjectLoader {
    /// Loads a project from the given root directory.
    ///
    /// Reads and parses `project.godot` at `project_root/project.godot`.
    pub fn load(project_root: &Path) -> EngineResult<ProjectLoader> {
        let godot_file = project_root.join("project.godot");
        let content = std::fs::read_to_string(&godot_file).map_err(EngineError::Io)?;
        let project_file = GodotProjectFile::parse(&content)?;
        let config = ProjectConfig::from_project_file(&project_file)?;

        Ok(ProjectLoader {
            project_root: project_root.to_path_buf(),
            project_file,
            config,
        })
    }

    /// Returns the extracted project configuration.
    pub fn config(&self) -> &ProjectConfig {
        &self.config
    }

    /// Returns the raw parsed project file.
    pub fn project_file(&self) -> &GodotProjectFile {
        &self.project_file
    }

    /// Returns the project root directory.
    pub fn project_root(&self) -> &Path {
        &self.project_root
    }

    /// Resolves a `res://` path relative to this project's root.
    pub fn resolve_path(&self, res_path: &str) -> EngineResult<PathBuf> {
        resolve_res_path(&self.project_root, res_path)
    }

    /// Returns the list of autoload entries from the `[autoload]` section.
    pub fn get_autoloads(&self) -> Vec<AutoloadEntry> {
        let Some(section) = self.project_file.sections.get("autoload") else {
            return Vec::new();
        };

        let mut entries: Vec<AutoloadEntry> = section
            .iter()
            .map(|(name, raw_value)| {
                let value = unquote(raw_value);
                let (is_singleton, path) = if let Some(rest) = value.strip_prefix('*') {
                    (true, rest.to_string())
                } else {
                    (false, value)
                };
                AutoloadEntry {
                    name: name.clone(),
                    path,
                    is_singleton,
                }
            })
            .collect();

        // Sort by name for deterministic order.
        entries.sort_by(|a, b| a.name.cmp(&b.name));
        entries
    }

    /// Returns the list of input map entries from the `[input]` section.
    ///
    /// This is a simplified parser that extracts action names and attempts
    /// to pull out key code references from the raw value strings.
    pub fn get_input_map(&self) -> Vec<InputMapEntry> {
        let Some(section) = self.project_file.sections.get("input") else {
            return Vec::new();
        };

        let mut entries: Vec<InputMapEntry> = section
            .iter()
            .map(|(name, raw_value)| {
                let events = parse_input_events(raw_value);
                InputMapEntry {
                    action_name: name.clone(),
                    events,
                }
            })
            .collect();

        entries.sort_by(|a, b| a.action_name.cmp(&b.action_name));
        entries
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Strips surrounding double quotes from a value string.
fn unquote(s: &str) -> String {
    let s = s.trim();
    if s.len() >= 2 && s.starts_with('"') && s.ends_with('"') {
        s[1..s.len() - 1].to_string()
    } else {
        s.to_string()
    }
}

/// Parses a `Vector2(x, y)` string into a [`Vector2`].
fn parse_vector2(s: &str) -> Option<Vector2> {
    let s = s.trim();
    let inner = s.strip_prefix("Vector2(")?.strip_suffix(')')?;
    let (x_str, y_str) = inner.split_once(',')?;
    let x = x_str.trim().parse::<f32>().ok()?;
    let y = y_str.trim().parse::<f32>().ok()?;
    Some(Vector2::new(x, y))
}

/// Extracts event descriptions from a Godot input action value.
///
/// The format is typically:
/// ```text
/// {"dead_zone": 0.5, "events": [Object(InputEventKey, "resource_local_to_scene": false, ..., "keycode": 4194305, ...)]}
/// ```
///
/// We extract `keycode` values and `InputEvent*` type names as simplified strings.
fn parse_input_events(raw: &str) -> Vec<String> {
    let mut events = Vec::new();

    // Find all Object(...) entries.
    let mut search = raw;
    while let Some(obj_start) = search.find("Object(") {
        let rest = &search[obj_start + 7..];

        // Extract the event type name (first argument before comma).
        let type_end = rest.find(',').unwrap_or(rest.len());
        let event_type = rest[..type_end].trim().trim_matches('"');

        // Try to find a keycode in this Object(...).
        let obj_end = find_balanced_paren(rest).unwrap_or(rest.len());
        let obj_body = &rest[..obj_end];

        let mut desc = event_type.to_string();
        if let Some(kc) = extract_field(obj_body, "keycode") {
            desc = format!("{event_type}:keycode={kc}");
        } else if let Some(bi) = extract_field(obj_body, "button_index") {
            desc = format!("{event_type}:button_index={bi}");
        }

        events.push(desc);
        search = &rest[obj_end..];
    }

    events
}

/// Finds the position of the closing `)` that matches the first `(` in the string.
fn find_balanced_paren(s: &str) -> Option<usize> {
    let mut depth = 0i32;
    let mut in_string = false;
    let mut escape = false;

    for (i, c) in s.char_indices() {
        if escape {
            escape = false;
            continue;
        }
        if c == '\\' && in_string {
            escape = true;
            continue;
        }
        if c == '"' {
            in_string = !in_string;
            continue;
        }
        if in_string {
            continue;
        }
        match c {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth < 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
}

/// Extracts a numeric field value from a comma-separated key:value string.
fn extract_field<'a>(s: &'a str, field: &str) -> Option<&'a str> {
    let pattern = format!("\"{field}\":");
    let start = s.find(&pattern)?;
    let rest = &s[start + pattern.len()..];
    let rest = rest.trim_start();
    // Read until comma, paren, or end.
    let end = rest
        .find([',', ')', '}'])
        .unwrap_or(rest.len());
    let val = rest[..end].trim();
    if val.is_empty() {
        None
    } else {
        Some(val)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    /// Realistic project.godot content for testing.
    const FIXTURE_CONTENT: &str = r#"; Engine configuration file.
; It's best edited using the editor UI and not directly,
; since the parameters that go here are not all obvious.
;
; Format:
;   [section] ; section goes between []
;   param=value ; assign values to parameters

config_version=5

[application]

config/name="Test Platformer"
run/main_scene="res://scenes/main.tscn"
config/features=PackedStringArray("4.2", "Forward Plus")
config/icon="res://icon.svg"

[autoload]

Global="*res://scripts/global.gd"
Config="res://scenes/config.tscn"
AudioManager="*res://scripts/audio_manager.gd"

[display]

window/size/viewport_width=1920
window/size/viewport_height=1080

[input]

move_left={
"dead_zone": 0.5,
"events": [Object(InputEventKey,"resource_local_to_scene":false,"resource_name":"","device":-1,"window_id":0,"alt_pressed":false,"shift_pressed":false,"ctrl_pressed":false,"meta_pressed":false,"pressed":false,"keycode":65,"physical_keycode":0,"key_label":0,"unicode":97,"location":0,"echo":false,"script":null)
]
}
move_right={
"dead_zone": 0.5,
"events": [Object(InputEventKey,"resource_local_to_scene":false,"resource_name":"","device":-1,"window_id":0,"alt_pressed":false,"shift_pressed":false,"ctrl_pressed":false,"meta_pressed":false,"pressed":false,"keycode":68,"physical_keycode":0,"key_label":0,"unicode":100,"location":0,"echo":false,"script":null)
]
}
jump={
"dead_zone": 0.5,
"events": [Object(InputEventKey,"resource_local_to_scene":false,"resource_name":"","device":-1,"window_id":0,"alt_pressed":false,"shift_pressed":false,"ctrl_pressed":false,"meta_pressed":false,"pressed":false,"keycode":4194320,"physical_keycode":0,"key_label":0,"unicode":0,"location":0,"echo":false,"script":null)
]
}

[physics]

common/physics_ticks_per_second=120
2d/default_gravity=1200.0
2d/default_gravity_vector=Vector2(0, 1)

[rendering]

renderer/rendering_method="forward_plus"
"#;

    fn create_test_project(dir: &Path) {
        fs::write(dir.join("project.godot"), FIXTURE_CONTENT).unwrap();
    }

    // -- GodotProjectFile parsing ---

    #[test]
    fn parse_project_file_sections() {
        let pf = GodotProjectFile::parse(FIXTURE_CONTENT).unwrap();
        assert!(pf.sections.contains_key("application"));
        assert!(pf.sections.contains_key("autoload"));
        assert!(pf.sections.contains_key("display"));
        assert!(pf.sections.contains_key("input"));
        assert!(pf.sections.contains_key("physics"));
        assert!(pf.sections.contains_key("rendering"));
    }

    #[test]
    fn parse_project_file_root_section() {
        let pf = GodotProjectFile::parse(FIXTURE_CONTENT).unwrap();
        assert_eq!(pf.get("", "config_version"), Some("5"));
    }

    #[test]
    fn parse_project_file_quoted_values() {
        let pf = GodotProjectFile::parse(FIXTURE_CONTENT).unwrap();
        assert_eq!(
            pf.get("application", "config/name"),
            Some("\"Test Platformer\"")
        );
    }

    #[test]
    fn parse_project_file_get_missing_section() {
        let pf = GodotProjectFile::parse(FIXTURE_CONTENT).unwrap();
        assert_eq!(pf.get("nonexistent", "key"), None);
    }

    #[test]
    fn parse_project_file_get_missing_key() {
        let pf = GodotProjectFile::parse(FIXTURE_CONTENT).unwrap();
        assert_eq!(pf.get("application", "nonexistent"), None);
    }

    #[test]
    fn parse_empty_content() {
        let pf = GodotProjectFile::parse("").unwrap();
        assert!(pf.sections.is_empty());
    }

    #[test]
    fn parse_comments_only() {
        let pf = GodotProjectFile::parse("; comment\n# another\n").unwrap();
        assert!(pf.sections.is_empty());
    }

    // -- ProjectConfig extraction ---

    #[test]
    fn config_project_name() {
        let pf = GodotProjectFile::parse(FIXTURE_CONTENT).unwrap();
        let config = ProjectConfig::from_project_file(&pf).unwrap();
        assert_eq!(config.project_name, "Test Platformer");
    }

    #[test]
    fn config_main_scene() {
        let pf = GodotProjectFile::parse(FIXTURE_CONTENT).unwrap();
        let config = ProjectConfig::from_project_file(&pf).unwrap();
        assert_eq!(config.main_scene, "res://scenes/main.tscn");
    }

    #[test]
    fn config_icon_path() {
        let pf = GodotProjectFile::parse(FIXTURE_CONTENT).unwrap();
        let config = ProjectConfig::from_project_file(&pf).unwrap();
        assert_eq!(config.icon_path, Some("res://icon.svg".to_string()));
    }

    #[test]
    fn config_version() {
        let pf = GodotProjectFile::parse(FIXTURE_CONTENT).unwrap();
        let config = ProjectConfig::from_project_file(&pf).unwrap();
        assert_eq!(config.config_version, 5);
    }

    #[test]
    fn config_viewport_dimensions() {
        let pf = GodotProjectFile::parse(FIXTURE_CONTENT).unwrap();
        let config = ProjectConfig::from_project_file(&pf).unwrap();
        assert_eq!(config.viewport_width, 1920);
        assert_eq!(config.viewport_height, 1080);
    }

    #[test]
    fn config_physics_ticks() {
        let pf = GodotProjectFile::parse(FIXTURE_CONTENT).unwrap();
        let config = ProjectConfig::from_project_file(&pf).unwrap();
        assert_eq!(config.physics_ticks_per_second, 120);
    }

    #[test]
    fn config_gravity() {
        let pf = GodotProjectFile::parse(FIXTURE_CONTENT).unwrap();
        let config = ProjectConfig::from_project_file(&pf).unwrap();
        assert!((config.default_gravity - 1200.0).abs() < f32::EPSILON);
    }

    #[test]
    fn config_gravity_vector() {
        let pf = GodotProjectFile::parse(FIXTURE_CONTENT).unwrap();
        let config = ProjectConfig::from_project_file(&pf).unwrap();
        assert!((config.default_gravity_vector.x).abs() < f32::EPSILON);
        assert!((config.default_gravity_vector.y - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn config_defaults_when_missing() {
        let pf = GodotProjectFile::parse("[application]\nconfig/name=\"Minimal\"").unwrap();
        let config = ProjectConfig::from_project_file(&pf).unwrap();
        assert_eq!(config.viewport_width, 1152);
        assert_eq!(config.viewport_height, 648);
        assert_eq!(config.physics_ticks_per_second, 60);
        assert!((config.default_gravity - 980.0).abs() < f32::EPSILON);
    }

    // -- AutoloadEntry ---

    #[test]
    fn autoloads_parsed() {
        let pf = GodotProjectFile::parse(FIXTURE_CONTENT).unwrap();
        let config = ProjectConfig::from_project_file(&pf).unwrap();
        let loader = ProjectLoader {
            project_root: PathBuf::from("/fake"),
            project_file: pf,
            config,
        };
        let autoloads = loader.get_autoloads();
        assert_eq!(autoloads.len(), 3);
    }

    #[test]
    fn autoload_singleton_detection() {
        let pf = GodotProjectFile::parse(FIXTURE_CONTENT).unwrap();
        let config = ProjectConfig::from_project_file(&pf).unwrap();
        let loader = ProjectLoader {
            project_root: PathBuf::from("/fake"),
            project_file: pf,
            config,
        };
        let autoloads = loader.get_autoloads();
        let global = autoloads.iter().find(|a| a.name == "Global").unwrap();
        assert!(global.is_singleton);
        assert_eq!(global.path, "res://scripts/global.gd");

        let cfg = autoloads.iter().find(|a| a.name == "Config").unwrap();
        assert!(!cfg.is_singleton);
        assert_eq!(cfg.path, "res://scenes/config.tscn");
    }

    #[test]
    fn autoload_empty_section() {
        let pf = GodotProjectFile::parse("[application]\nconfig/name=\"NoAutoloads\"").unwrap();
        let config = ProjectConfig::from_project_file(&pf).unwrap();
        let loader = ProjectLoader {
            project_root: PathBuf::from("/fake"),
            project_file: pf,
            config,
        };
        assert!(loader.get_autoloads().is_empty());
    }

    // -- InputMapEntry ---

    #[test]
    fn input_map_parsed() {
        let pf = GodotProjectFile::parse(FIXTURE_CONTENT).unwrap();
        let config = ProjectConfig::from_project_file(&pf).unwrap();
        let loader = ProjectLoader {
            project_root: PathBuf::from("/fake"),
            project_file: pf,
            config,
        };
        let input_map = loader.get_input_map();
        assert_eq!(input_map.len(), 3);
    }

    #[test]
    fn input_map_action_names() {
        let pf = GodotProjectFile::parse(FIXTURE_CONTENT).unwrap();
        let config = ProjectConfig::from_project_file(&pf).unwrap();
        let loader = ProjectLoader {
            project_root: PathBuf::from("/fake"),
            project_file: pf,
            config,
        };
        let input_map = loader.get_input_map();
        let names: Vec<&str> = input_map.iter().map(|e| e.action_name.as_str()).collect();
        assert!(names.contains(&"move_left"));
        assert!(names.contains(&"move_right"));
        assert!(names.contains(&"jump"));
    }

    #[test]
    fn input_map_keycodes_extracted() {
        let pf = GodotProjectFile::parse(FIXTURE_CONTENT).unwrap();
        let config = ProjectConfig::from_project_file(&pf).unwrap();
        let loader = ProjectLoader {
            project_root: PathBuf::from("/fake"),
            project_file: pf,
            config,
        };
        let input_map = loader.get_input_map();
        let move_left = input_map
            .iter()
            .find(|e| e.action_name == "move_left")
            .unwrap();
        assert!(!move_left.events.is_empty());
        assert!(move_left.events[0].contains("InputEventKey"));
        assert!(move_left.events[0].contains("keycode=65"));
    }

    #[test]
    fn input_map_empty_section() {
        let pf = GodotProjectFile::parse("[application]\nconfig/name=\"NoInput\"").unwrap();
        let config = ProjectConfig::from_project_file(&pf).unwrap();
        let loader = ProjectLoader {
            project_root: PathBuf::from("/fake"),
            project_file: pf,
            config,
        };
        assert!(loader.get_input_map().is_empty());
    }

    // -- ProjectLoader from disk ---

    #[test]
    fn loader_reads_project_from_disk() {
        let dir = TempDir::new().unwrap();
        create_test_project(dir.path());

        let loader = ProjectLoader::load(dir.path()).unwrap();
        assert_eq!(loader.config().project_name, "Test Platformer");
        assert_eq!(loader.config().main_scene, "res://scenes/main.tscn");
    }

    #[test]
    fn loader_project_root() {
        let dir = TempDir::new().unwrap();
        create_test_project(dir.path());

        let loader = ProjectLoader::load(dir.path()).unwrap();
        assert_eq!(loader.project_root(), dir.path());
    }

    #[test]
    fn loader_resolve_path() {
        let dir = TempDir::new().unwrap();
        create_test_project(dir.path());

        let loader = ProjectLoader::load(dir.path()).unwrap();
        let resolved = loader.resolve_path("res://scenes/main.tscn").unwrap();
        assert_eq!(resolved, dir.path().join("scenes/main.tscn"));
    }

    #[test]
    fn loader_missing_project_file_fails() {
        let dir = TempDir::new().unwrap();
        assert!(ProjectLoader::load(dir.path()).is_err());
    }

    #[test]
    fn loader_autoloads_from_disk() {
        let dir = TempDir::new().unwrap();
        create_test_project(dir.path());

        let loader = ProjectLoader::load(dir.path()).unwrap();
        let autoloads = loader.get_autoloads();
        assert_eq!(autoloads.len(), 3);

        let audio = autoloads.iter().find(|a| a.name == "AudioManager").unwrap();
        assert!(audio.is_singleton);
        assert_eq!(audio.path, "res://scripts/audio_manager.gd");
    }

    #[test]
    fn loader_input_map_from_disk() {
        let dir = TempDir::new().unwrap();
        create_test_project(dir.path());

        let loader = ProjectLoader::load(dir.path()).unwrap();
        let input_map = loader.get_input_map();
        assert_eq!(input_map.len(), 3);
    }

    // -- Helper unit tests ---

    #[test]
    fn unquote_strips_quotes() {
        assert_eq!(unquote("\"hello\""), "hello");
        assert_eq!(unquote("bare"), "bare");
        assert_eq!(unquote("\"\""), "");
    }

    #[test]
    fn parse_vector2_helper() {
        let v = parse_vector2("Vector2(0, 1)").unwrap();
        assert!((v.x).abs() < f32::EPSILON);
        assert!((v.y - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn parse_vector2_invalid() {
        assert!(parse_vector2("not a vector").is_none());
        assert!(parse_vector2("Vector2(bad)").is_none());
    }
}
