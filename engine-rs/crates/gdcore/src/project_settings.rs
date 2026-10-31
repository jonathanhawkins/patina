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

use crate::math::Color;

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

    /// Returns the value as a [`Color`], if it is one. The stored `f64`
    /// channels are narrowed to `Color`'s `f32`.
    pub fn as_color(&self) -> Option<Color> {
        match self {
            SettingsValue::Color(r, g, b, a) => {
                Some(Color::new(*r as f32, *g as f32, *b as f32, *a as f32))
            }
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
    /// Godot's registered defaults, consulted by [`get`](Self::get) whenever a
    /// project omits a key (pat-0q2kx.126).
    ///
    /// Deliberately a *separate* map from `properties` rather than pre-filled
    /// into it. Godot's `project.godot` only ever stores keys that differ from
    /// their default, so folding the table into `properties` would make
    /// [`to_godot_format`](Self::to_godot_format) write out ~60 keys the
    /// project never set, and would change `property_count()` from "what this
    /// project declares" to "everything the engine knows about".
    defaults: BTreeMap<String, SettingsValue>,
    /// The `config_version` from the project file header.
    config_version: u32,
    /// Path to the loaded project file, if any.
    project_path: Option<String>,
}

impl Default for ProjectSettings {
    fn default() -> Self {
        let mut ps = Self {
            properties: BTreeMap::new(),
            defaults: godot_default_table(),
            config_version: 5,
            project_path: None,
        };
        // Godot defaults
        ps.set(
            "application/config/name",
            SettingsValue::String("New Project".into()),
        );
        ps.set(
            "application/run/main_scene",
            SettingsValue::String(String::new()),
        );
        ps.set(
            "physics/common/physics_ticks_per_second",
            SettingsValue::Int(60),
        );
        ps.set("physics/2d/default_gravity", SettingsValue::Float(980.0));
        ps.set("physics/3d/default_gravity", SettingsValue::Float(9.8));
        ps.set(
            "display/window/size/viewport_width",
            SettingsValue::Int(1152),
        );
        ps.set(
            "display/window/size/viewport_height",
            SettingsValue::Int(648),
        );
        ps.set(
            "rendering/renderer/rendering_method",
            SettingsValue::String("forward_plus".into()),
        );
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

    /// Gets a setting by path, falling back to Godot's registered default.
    ///
    /// Mirrors `GLOBAL_GET`, which always answers with the registered default
    /// when the project has not overridden a key. Returns `None` only for keys
    /// that are neither set nor documented.
    pub fn get(&self, path: &str) -> Option<&SettingsValue> {
        self.properties
            .get(path)
            .or_else(|| self.defaults.get(path))
    }

    /// Gets a setting, falling back to Godot's registered default and then to
    /// the caller's `default`.
    pub fn get_or<'a>(&'a self, path: &str, default: &'a SettingsValue) -> &'a SettingsValue {
        self.get(path).unwrap_or(default)
    }

    /// Returns the engine's registered default for `path`, ignoring anything
    /// the project set — the analogue of Godot's `property_get_revert`.
    pub fn get_default(&self, path: &str) -> Option<&SettingsValue> {
        self.defaults.get(path)
    }

    /// Sets a setting by path, creating or overwriting it.
    pub fn set(&mut self, path: &str, value: SettingsValue) {
        self.properties.insert(path.to_string(), value);
    }

    /// Returns `true` if the setting exists — either set by the project or
    /// registered as an engine default, matching `ProjectSettings.has_setting`.
    pub fn has(&self, path: &str) -> bool {
        self.properties.contains_key(path) || self.defaults.contains_key(path)
    }

    /// Removes a setting. Returns the old value if it existed.
    pub fn remove(&mut self, path: &str) -> Option<SettingsValue> {
        self.properties.remove(path)
    }

    /// Returns the total number of settings.
    pub fn property_count(&self) -> usize {
        self.properties.len()
    }

    /// Resolves a setting honoring **feature-tag overrides** (pat-nzgdu.128): a
    /// `name.<feature>` entry overrides the base `name` when `<feature>` is one
    /// of the active `features`. The first active feature (in the order given)
    /// that has an override wins; otherwise the base setting is returned (or
    /// `None` if unset). Mirrors Godot's `ProjectSettings.get_setting_with_override`
    /// fed an explicit feature set (e.g. `["web"]`, `["mobile"]`).
    pub fn get_setting_with_override_and_custom_features(
        &self,
        name: &str,
        features: &[&str],
    ) -> Option<&SettingsValue> {
        for feature in features {
            let key = format!("{name}.{feature}");
            if let Some(v) = self.properties.get(&key) {
                return Some(v);
            }
        }
        self.properties.get(name)
    }

    // -- Layer names (pat-nzgdu.130) ----------------------------------------

    /// Returns the user-assigned name for a collision/render/navigation layer,
    /// if the project set one. Godot stores these as
    /// `layer_names/<category>/layer_<n>` string settings, where `category` is
    /// one of `2d_physics`, `2d_render`, `3d_physics`, `3d_render`,
    /// `2d_navigation`, `3d_navigation`, or `avoidance`, and layers are
    /// 1-indexed (`layer_1`..`layer_32`). Returns `None` when unnamed or empty.
    pub fn layer_name(&self, category: &str, layer: u32) -> Option<&str> {
        self.get(&format!("layer_names/{category}/layer_{layer}"))
            .and_then(|v| v.as_string())
            .filter(|s| !s.is_empty())
    }

    /// Returns the label a layer grid should display for `layer`: the
    /// user-assigned [`layer_name`](Self::layer_name), or Godot's default
    /// `"Layer <n>"` when the layer is unnamed.
    pub fn layer_label(&self, category: &str, layer: u32) -> String {
        self.layer_name(category, layer)
            .map(str::to_string)
            .unwrap_or_else(|| format!("Layer {layer}"))
    }

    /// Assigns a name to a collision/render/navigation layer, writing the
    /// `layer_names/<category>/layer_<n>` setting. An empty `name` clears it.
    pub fn set_layer_name(&mut self, category: &str, layer: u32, name: impl Into<String>) {
        let name = name.into();
        let key = format!("layer_names/{category}/layer_{layer}");
        if name.is_empty() {
            self.remove(&key);
        } else {
            self.set(&key, SettingsValue::String(name));
        }
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

    /// The root viewport width (`display/window/size/viewport_width`),
    /// defaulting to Godot's `1152`.
    pub fn viewport_width(&self) -> i64 {
        self.get("display/window/size/viewport_width")
            .and_then(|v| v.as_int())
            .unwrap_or(1152)
    }

    /// The root viewport height (`display/window/size/viewport_height`),
    /// defaulting to Godot's `648`.
    pub fn viewport_height(&self) -> i64 {
        self.get("display/window/size/viewport_height")
            .and_then(|v| v.as_int())
            .unwrap_or(648)
    }

    /// The root window's content-scale ("stretch") configuration, assembled
    /// from `display/window/stretch/*` and the base viewport size
    /// (pat-nzgdu.87).
    ///
    /// Godot seeds the root window's `content_scale_size` from
    /// `viewport_width`/`viewport_height`, so a project that sets a stretch
    /// mode but no explicit base resolution still stretches against `1152×648`.
    pub fn content_scale_config(&self) -> crate::content_scale::ContentScaleConfig {
        use crate::content_scale::{
            ContentScaleAspect, ContentScaleConfig, ContentScaleMode, ContentScaleStretch,
        };
        ContentScaleConfig {
            mode: ContentScaleMode::from_setting(
                self.get("display/window/stretch/mode")
                    .and_then(|v| v.as_string())
                    .unwrap_or("disabled"),
            ),
            aspect: ContentScaleAspect::from_setting(
                self.get("display/window/stretch/aspect")
                    .and_then(|v| v.as_string())
                    .unwrap_or("keep"),
            ),
            // ⚠️ The key is `scale_mode`, NOT `stretch_mode`
            // (`project_settings.cpp:1779`). Reading the wrong key silently
            // leaves every project on fractional scaling.
            stretch: ContentScaleStretch::from_setting(
                self.get("display/window/stretch/scale_mode")
                    .and_then(|v| v.as_string())
                    .unwrap_or("fractional"),
            ),
            // ⚠️ A scale of 0 would collapse the canvas; Godot's minimum is
            // 0.5, so anything at or below zero falls back to 1.0.
            factor: {
                let f = self
                    .get("display/window/stretch/scale")
                    .and_then(|v| v.as_float())
                    .unwrap_or(1.0) as f32;
                if f > 0.0 {
                    f
                } else {
                    1.0
                }
            },
            base_size: crate::math::Vector2::new(
                self.viewport_width() as f32,
                self.viewport_height() as f32,
            ),
        }
    }

    /// The frame-rate cap (`application/run/max_fps`); `0` means uncapped,
    /// matching Godot's default.
    pub fn max_fps(&self) -> i64 {
        self.get("application/run/max_fps")
            .and_then(|v| v.as_int())
            .unwrap_or(0)
    }

    /// The default background clear color
    /// (`rendering/environment/defaults/default_clear_color`), defaulting to
    /// Godot's grey `Color(0.3, 0.3, 0.3, 1.0)` when unset.
    pub fn default_clear_color(&self) -> Color {
        self.get("rendering/environment/defaults/default_clear_color")
            .and_then(|v| v.as_color())
            .unwrap_or_else(|| Color::new(0.3, 0.3, 0.3, 1.0))
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
    /// Every setting this project declares, as `(path, value)` in path order
    /// (pat-0q2kx.135).
    ///
    /// [`get_section`](Self::get_section) can only answer for one prefix, so
    /// there was no way to hand the whole table to another layer — which is what
    /// seeding the interpreter's `ProjectSettings` singleton at boot needs.
    /// Yields only what the project actually declares, NOT the Godot default
    /// table, so a consumer can still tell "set by the project" from "engine
    /// default".
    pub fn iter(&self) -> impl Iterator<Item = (&str, &SettingsValue)> {
        self.properties.iter().map(|(k, v)| (k.as_str(), v))
    }

    /// Every Godot-registered DEFAULT, as `(path, value)` in path order —
    /// ignoring anything this project declared.
    ///
    /// The companion to [`iter`](Self::iter), which yields only declared keys.
    /// Seeding another layer (the interpreter's `ProjectSettings` singleton)
    /// needs both halves kept APART: overlay defaults first and declared keys
    /// second and you get Godot's `get_setting` semantics, while the split still
    /// tells "set by the project" from "engine default" — which is what
    /// `ProjectSettings.save()` needs so it writes back only overrides, and what
    /// `property_can_revert` would need (pat-mgixk.42).
    ///
    /// ⚠️ Deliberately NOT a merged `iter_with_defaults()`. A single merged
    /// iterator loses that distinction, and the consumer that most needs it is
    /// `save()`: fed a merged table it would write all ~230 engine defaults into
    /// the user's `project.godot`, which Godot never does.
    pub fn iter_defaults(&self) -> impl Iterator<Item = (&str, &SettingsValue)> {
        self.defaults.iter().map(|(k, v)| (k.as_str(), v))
    }

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
        // Seeded here too, not just in `default()`: a real `project.godot`
        // declares only the handful of keys it overrides, so without the table
        // `get()` answered `None` for every documented setting the file omitted
        // (pat-0q2kx.126). `properties` still holds only what the file declares,
        // so the project's own values always win and a save round-trip is
        // unchanged.
        let mut ps = Self {
            properties: BTreeMap::new(),
            defaults: godot_default_table(),
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
// Godot's registered default table (pat-0q2kx.126)
// ---------------------------------------------------------------------------

/// Every `ProjectSettings` member that `doc/classes/ProjectSettings.xml`
/// documents with a `default=` under the sections a running game actually
/// queries: `application/`, `display/`, `physics/`, `gui/`, `input_devices/`,
/// `audio/`, `rendering/textures/`, `rendering/2d/`,
/// `rendering/environment/defaults/` and `internationalization/`.
///
/// 🔑 **Transcribed from the XML, never derived.** Godot registers these with
/// `GLOBAL_DEF` in `core/config/project_settings.cpp` and friends, so
/// `get_setting("physics/2d/default_gravity")` answers `980.0` in a project
/// whose `project.godot` never mentions it — which is exactly the
/// `CharacterBody2D` idiom that used to come back `Nil` (pat-mgixk.42). Values
/// read out of the doc XML's `default=` attribute by a script, so a typo here is
/// a transcription error rather than a judgement call.
///
/// ⚠️ TWO documented keys in these sections are deliberately absent, because
/// [`SettingsValue`] has no variant for them and inventing one would be worse
/// than a recorded gap:
///
/// | key | type | default |
/// |---|---|---|
/// | `application/config/name_localized` | `Dictionary` | `{}` |
/// | `display/window/size/initial_position` | `Vector2i` | `Vector2i(0, 0)` |
///
/// Adding those means adding the variant and teaching
/// `settings_value_to_variant` about it.
///
/// This is the broad base. [`godot_default_table`] layers the hand-curated
/// entries over it — those carry source citations for the specific settings the
/// runtime consults, and cover keys outside these sections. The two agree
/// wherever they overlap; that was checked key-by-key against the XML when this
/// table was introduced.
fn documented_project_settings_defaults() -> BTreeMap<String, SettingsValue> {
    let mut t = BTreeMap::new();
    for (key, value) in [
        (
            "application/boot_splash/bg_color",
            SettingsValue::Color(0.14, 0.14, 0.14, 1.0),
        ),
        (
            "application/boot_splash/image",
            SettingsValue::String(String::new()),
        ),
        (
            "application/boot_splash/minimum_display_time",
            SettingsValue::Int(0),
        ),
        (
            "application/boot_splash/show_image",
            SettingsValue::Bool(true),
        ),
        (
            "application/boot_splash/stretch_mode",
            SettingsValue::Int(1),
        ),
        (
            "application/boot_splash/use_filter",
            SettingsValue::Bool(true),
        ),
        (
            "application/config/auto_accept_quit",
            SettingsValue::Bool(true),
        ),
        (
            "application/config/custom_user_dir_name",
            SettingsValue::String(String::new()),
        ),
        (
            "application/config/description",
            SettingsValue::String(String::new()),
        ),
        (
            "application/config/disable_project_settings_override",
            SettingsValue::Bool(false),
        ),
        (
            "application/config/icon",
            SettingsValue::String(String::new()),
        ),
        (
            "application/config/macos_native_icon",
            SettingsValue::String(String::new()),
        ),
        (
            "application/config/name",
            SettingsValue::String(String::new()),
        ),
        (
            "application/config/project_settings_override",
            SettingsValue::String(String::new()),
        ),
        (
            "application/config/quit_on_go_back",
            SettingsValue::Bool(true),
        ),
        (
            "application/config/use_custom_user_dir",
            SettingsValue::Bool(false),
        ),
        (
            "application/config/use_hidden_project_data_directory",
            SettingsValue::Bool(true),
        ),
        (
            "application/config/version",
            SettingsValue::String(String::new()),
        ),
        (
            "application/config/windows_native_icon",
            SettingsValue::String(String::new()),
        ),
        ("application/run/delta_smoothing", SettingsValue::Bool(true)),
        ("application/run/disable_stderr", SettingsValue::Bool(false)),
        ("application/run/disable_stdout", SettingsValue::Bool(false)),
        (
            "application/run/enable_alt_space_menu",
            SettingsValue::Bool(false),
        ),
        (
            "application/run/flush_stdout_on_print",
            SettingsValue::Bool(false),
        ),
        (
            "application/run/flush_stdout_on_print.debug",
            SettingsValue::Bool(true),
        ),
        ("application/run/frame_delay_msec", SettingsValue::Int(0)),
        (
            "application/run/load_shell_environment",
            SettingsValue::Bool(false),
        ),
        (
            "application/run/low_processor_mode",
            SettingsValue::Bool(false),
        ),
        (
            "application/run/low_processor_mode_sleep_usec",
            SettingsValue::Int(6900),
        ),
        (
            "application/run/main_loop_type",
            SettingsValue::String("SceneTree".to_string()),
        ),
        (
            "application/run/main_scene",
            SettingsValue::String(String::new()),
        ),
        ("application/run/max_fps", SettingsValue::Int(0)),
        ("application/run/print_header", SettingsValue::Bool(true)),
        (
            "audio/buses/channel_disable_threshold_db",
            SettingsValue::Float(-60.0),
        ),
        (
            "audio/buses/channel_disable_time",
            SettingsValue::Float(2.0),
        ),
        (
            "audio/buses/default_bus_layout",
            SettingsValue::String("res://default_bus_layout.tres".to_string()),
        ),
        ("audio/driver/enable_input", SettingsValue::Bool(false)),
        ("audio/driver/mix_rate", SettingsValue::Int(44100)),
        ("audio/driver/mix_rate.web", SettingsValue::Int(0)),
        ("audio/driver/output_latency", SettingsValue::Int(15)),
        ("audio/driver/output_latency.web", SettingsValue::Int(50)),
        (
            "audio/general/2d_panning_strength",
            SettingsValue::Float(0.5),
        ),
        (
            "audio/general/3d_panning_strength",
            SettingsValue::Float(0.5),
        ),
        ("audio/general/default_playback_type", SettingsValue::Int(0)),
        (
            "audio/general/default_playback_type.web",
            SettingsValue::Int(1),
        ),
        (
            "audio/general/ios/mix_with_others",
            SettingsValue::Bool(false),
        ),
        ("audio/general/ios/session_category", SettingsValue::Int(0)),
        ("audio/general/text_to_speech", SettingsValue::Bool(false)),
        (
            "audio/video/video_delay_compensation_ms",
            SettingsValue::Int(0),
        ),
        (
            "display/mouse_cursor/custom_image",
            SettingsValue::String(String::new()),
        ),
        (
            "display/mouse_cursor/custom_image_hotspot",
            SettingsValue::Vector2(0.0, 0.0),
        ),
        (
            "display/mouse_cursor/tooltip_position_offset",
            SettingsValue::Vector2(10.0, 10.0),
        ),
        ("display/window/dpi/allow_hidpi", SettingsValue::Bool(true)),
        (
            "display/window/energy_saving/keep_screen_on",
            SettingsValue::Bool(true),
        ),
        (
            "display/window/frame_pacing/android/enable_frame_pacing",
            SettingsValue::Bool(true),
        ),
        (
            "display/window/frame_pacing/android/swappy_mode",
            SettingsValue::Int(2),
        ),
        ("display/window/handheld/orientation", SettingsValue::Int(0)),
        (
            "display/window/ios/allow_high_refresh_rate",
            SettingsValue::Bool(true),
        ),
        (
            "display/window/ios/hide_home_indicator",
            SettingsValue::Bool(true),
        ),
        (
            "display/window/ios/hide_status_bar",
            SettingsValue::Bool(true),
        ),
        (
            "display/window/ios/suppress_ui_gesture",
            SettingsValue::Bool(true),
        ),
        (
            "display/window/per_pixel_transparency/allowed",
            SettingsValue::Bool(false),
        ),
        (
            "display/window/size/always_on_top",
            SettingsValue::Bool(false),
        ),
        ("display/window/size/borderless", SettingsValue::Bool(false)),
        (
            "display/window/size/extend_to_title",
            SettingsValue::Bool(false),
        ),
        (
            "display/window/size/initial_position_type",
            SettingsValue::Int(1),
        ),
        ("display/window/size/initial_screen", SettingsValue::Int(0)),
        (
            "display/window/size/maximize_disabled",
            SettingsValue::Bool(false),
        ),
        (
            "display/window/size/minimize_disabled",
            SettingsValue::Bool(false),
        ),
        ("display/window/size/mode", SettingsValue::Int(0)),
        ("display/window/size/no_focus", SettingsValue::Bool(false)),
        ("display/window/size/resizable", SettingsValue::Bool(true)),
        (
            "display/window/size/sharp_corners",
            SettingsValue::Bool(false),
        ),
        (
            "display/window/size/transparent",
            SettingsValue::Bool(false),
        ),
        (
            "display/window/size/viewport_height",
            SettingsValue::Int(648),
        ),
        (
            "display/window/size/viewport_width",
            SettingsValue::Int(1152),
        ),
        (
            "display/window/size/window_height_override",
            SettingsValue::Int(0),
        ),
        (
            "display/window/size/window_width_override",
            SettingsValue::Int(0),
        ),
        (
            "display/window/stretch/aspect",
            SettingsValue::String("keep".to_string()),
        ),
        (
            "display/window/stretch/mode",
            SettingsValue::String("disabled".to_string()),
        ),
        ("display/window/stretch/scale", SettingsValue::Float(1.0)),
        (
            "display/window/stretch/scale_mode",
            SettingsValue::String("fractional".to_string()),
        ),
        (
            "display/window/subwindows/embed_subwindows",
            SettingsValue::Bool(true),
        ),
        ("display/window/vsync/vsync_mode", SettingsValue::Int(1)),
        ("gui/common/default_scroll_deadzone", SettingsValue::Int(0)),
        ("gui/common/drag_threshold", SettingsValue::Int(10)),
        (
            "gui/common/show_focus_state_on_pointer_event",
            SettingsValue::Int(1),
        ),
        (
            "gui/common/snap_controls_to_pixels",
            SettingsValue::Bool(true),
        ),
        ("gui/common/swap_cancel_ok", SettingsValue::Int(0)),
        (
            "gui/common/text_edit_undo_stack_max_size",
            SettingsValue::Int(1024),
        ),
        (
            "gui/fonts/dynamic_fonts/use_oversampling",
            SettingsValue::Bool(true),
        ),
        ("gui/theme/custom", SettingsValue::String(String::new())),
        (
            "gui/theme/custom_font",
            SettingsValue::String(String::new()),
        ),
        ("gui/theme/default_font_antialiasing", SettingsValue::Int(1)),
        (
            "gui/theme/default_font_generate_mipmaps",
            SettingsValue::Bool(false),
        ),
        ("gui/theme/default_font_hinting", SettingsValue::Int(1)),
        (
            "gui/theme/default_font_multichannel_signed_distance_field",
            SettingsValue::Bool(false),
        ),
        (
            "gui/theme/default_font_subpixel_positioning",
            SettingsValue::Int(1),
        ),
        ("gui/theme/default_theme_scale", SettingsValue::Float(1.0)),
        ("gui/theme/lcd_subpixel_layout", SettingsValue::Int(1)),
        (
            "gui/timers/button_shortcut_feedback_highlight_time",
            SettingsValue::Float(0.2),
        ),
        (
            "gui/timers/incremental_search_max_interval_msec",
            SettingsValue::Int(2000),
        ),
        (
            "gui/timers/text_edit_idle_detect_sec",
            SettingsValue::Float(3.0),
        ),
        ("gui/timers/tooltip_delay_sec", SettingsValue::Float(0.5)),
        (
            "gui/timers/tooltip_delay_sec.editor_hint",
            SettingsValue::Float(0.5),
        ),
        (
            "input_devices/buffering/agile_event_flushing",
            SettingsValue::Bool(false),
        ),
        (
            "input_devices/compatibility/legacy_just_pressed_behavior",
            SettingsValue::Bool(false),
        ),
        (
            "input_devices/pointing/android/disable_scroll_deadzone",
            SettingsValue::Bool(false),
        ),
        (
            "input_devices/pointing/android/enable_long_press_as_right_click",
            SettingsValue::Bool(false),
        ),
        (
            "input_devices/pointing/android/enable_pan_and_scale_gestures",
            SettingsValue::Bool(false),
        ),
        (
            "input_devices/pointing/android/override_volume_buttons",
            SettingsValue::Bool(false),
        ),
        (
            "input_devices/pointing/android/rotary_input_scroll_axis",
            SettingsValue::Int(1),
        ),
        (
            "input_devices/pointing/emulate_mouse_from_touch",
            SettingsValue::Bool(true),
        ),
        (
            "input_devices/pointing/emulate_touch_from_mouse",
            SettingsValue::Bool(false),
        ),
        (
            "input_devices/sensors/enable_accelerometer",
            SettingsValue::Bool(false),
        ),
        (
            "input_devices/sensors/enable_gravity",
            SettingsValue::Bool(false),
        ),
        (
            "input_devices/sensors/enable_gyroscope",
            SettingsValue::Bool(false),
        ),
        (
            "input_devices/sensors/enable_magnetometer",
            SettingsValue::Bool(false),
        ),
        (
            "internationalization/locale/fallback",
            SettingsValue::String("en".to_string()),
        ),
        (
            "internationalization/locale/include_text_server_data",
            SettingsValue::Bool(false),
        ),
        (
            "internationalization/locale/line_breaking_strictness",
            SettingsValue::Int(0),
        ),
        (
            "internationalization/locale/test",
            SettingsValue::String(String::new()),
        ),
        (
            "internationalization/pseudolocalization/double_vowels",
            SettingsValue::Bool(false),
        ),
        (
            "internationalization/pseudolocalization/expansion_ratio",
            SettingsValue::Float(0.0),
        ),
        (
            "internationalization/pseudolocalization/fake_bidi",
            SettingsValue::Bool(false),
        ),
        (
            "internationalization/pseudolocalization/override",
            SettingsValue::Bool(false),
        ),
        (
            "internationalization/pseudolocalization/prefix",
            SettingsValue::String("[".to_string()),
        ),
        (
            "internationalization/pseudolocalization/replace_with_accents",
            SettingsValue::Bool(true),
        ),
        (
            "internationalization/pseudolocalization/skip_placeholders",
            SettingsValue::Bool(true),
        ),
        (
            "internationalization/pseudolocalization/suffix",
            SettingsValue::String("]".to_string()),
        ),
        (
            "internationalization/pseudolocalization/use_pseudolocalization",
            SettingsValue::Bool(false),
        ),
        (
            "internationalization/rendering/force_right_to_left_layout_direction",
            SettingsValue::Bool(false),
        ),
        (
            "internationalization/rendering/root_node_auto_translate",
            SettingsValue::Bool(true),
        ),
        (
            "internationalization/rendering/root_node_layout_direction",
            SettingsValue::Int(0),
        ),
        (
            "internationalization/rendering/text_driver",
            SettingsValue::String(String::new()),
        ),
        ("physics/2d/default_angular_damp", SettingsValue::Float(1.0)),
        ("physics/2d/default_gravity", SettingsValue::Float(980.0)),
        (
            "physics/2d/default_gravity_vector",
            SettingsValue::Vector2(0.0, 1.0),
        ),
        ("physics/2d/default_linear_damp", SettingsValue::Float(0.1)),
        (
            "physics/2d/physics_engine",
            SettingsValue::String("DEFAULT".to_string()),
        ),
        (
            "physics/2d/run_on_separate_thread",
            SettingsValue::Bool(false),
        ),
        (
            "physics/2d/sleep_threshold_angular",
            SettingsValue::Float(0.13962634),
        ),
        (
            "physics/2d/sleep_threshold_linear",
            SettingsValue::Float(2.0),
        ),
        (
            "physics/2d/solver/contact_max_allowed_penetration",
            SettingsValue::Float(0.3),
        ),
        (
            "physics/2d/solver/contact_max_separation",
            SettingsValue::Float(1.5),
        ),
        (
            "physics/2d/solver/contact_recycle_radius",
            SettingsValue::Float(1.0),
        ),
        (
            "physics/2d/solver/default_constraint_bias",
            SettingsValue::Float(0.2),
        ),
        (
            "physics/2d/solver/default_contact_bias",
            SettingsValue::Float(0.8),
        ),
        (
            "physics/2d/solver/solver_iterations",
            SettingsValue::Int(16),
        ),
        ("physics/2d/time_before_sleep", SettingsValue::Float(0.5)),
        ("physics/3d/default_angular_damp", SettingsValue::Float(0.1)),
        ("physics/3d/default_gravity", SettingsValue::Float(9.8)),
        (
            "physics/3d/default_gravity_vector",
            SettingsValue::Vector3(0.0, -1.0, 0.0),
        ),
        ("physics/3d/default_linear_damp", SettingsValue::Float(0.1)),
        (
            "physics/3d/physics_engine",
            SettingsValue::String("DEFAULT".to_string()),
        ),
        (
            "physics/3d/physics_interpolation/scene_traversal",
            SettingsValue::String("DEFAULT".to_string()),
        ),
        (
            "physics/3d/run_on_separate_thread",
            SettingsValue::Bool(false),
        ),
        (
            "physics/3d/sleep_threshold_angular",
            SettingsValue::Float(0.13962634),
        ),
        (
            "physics/3d/sleep_threshold_linear",
            SettingsValue::Float(0.1),
        ),
        (
            "physics/3d/solver/contact_max_allowed_penetration",
            SettingsValue::Float(0.01),
        ),
        (
            "physics/3d/solver/contact_max_separation",
            SettingsValue::Float(0.05),
        ),
        (
            "physics/3d/solver/contact_recycle_radius",
            SettingsValue::Float(0.01),
        ),
        (
            "physics/3d/solver/default_contact_bias",
            SettingsValue::Float(0.8),
        ),
        (
            "physics/3d/solver/solver_iterations",
            SettingsValue::Int(16),
        ),
        ("physics/3d/time_before_sleep", SettingsValue::Float(0.5)),
        (
            "physics/common/enable_object_picking",
            SettingsValue::Bool(true),
        ),
        (
            "physics/common/max_physics_steps_per_frame",
            SettingsValue::Int(8),
        ),
        (
            "physics/common/physics_interpolation",
            SettingsValue::Bool(false),
        ),
        (
            "physics/common/physics_jitter_fix",
            SettingsValue::Float(0.5),
        ),
        (
            "physics/common/physics_ticks_per_second",
            SettingsValue::Int(60),
        ),
        (
            "physics/jolt_physics_3d/collisions/active_edge_threshold",
            SettingsValue::Float(0.87266463),
        ),
        (
            "physics/jolt_physics_3d/collisions/collision_margin_fraction",
            SettingsValue::Float(0.08),
        ),
        (
            "physics/jolt_physics_3d/joints/world_node",
            SettingsValue::Int(0),
        ),
        (
            "physics/jolt_physics_3d/limits/max_angular_velocity",
            SettingsValue::Float(47.12389),
        ),
        (
            "physics/jolt_physics_3d/limits/max_bodies",
            SettingsValue::Int(10240),
        ),
        (
            "physics/jolt_physics_3d/limits/max_body_pairs",
            SettingsValue::Int(65536),
        ),
        (
            "physics/jolt_physics_3d/limits/max_contact_constraints",
            SettingsValue::Int(20480),
        ),
        (
            "physics/jolt_physics_3d/limits/max_linear_velocity",
            SettingsValue::Float(500.0),
        ),
        (
            "physics/jolt_physics_3d/limits/temporary_memory_buffer_size",
            SettingsValue::Int(32),
        ),
        (
            "physics/jolt_physics_3d/limits/world_boundary_shape_size",
            SettingsValue::Float(2000.0),
        ),
        (
            "physics/jolt_physics_3d/motion_queries/recovery_amount",
            SettingsValue::Float(0.4),
        ),
        (
            "physics/jolt_physics_3d/motion_queries/recovery_iterations",
            SettingsValue::Int(4),
        ),
        (
            "physics/jolt_physics_3d/motion_queries/use_enhanced_internal_edge_removal",
            SettingsValue::Bool(true),
        ),
        (
            "physics/jolt_physics_3d/queries/enable_ray_cast_face_index",
            SettingsValue::Bool(false),
        ),
        (
            "physics/jolt_physics_3d/queries/use_enhanced_internal_edge_removal",
            SettingsValue::Bool(false),
        ),
        (
            "physics/jolt_physics_3d/simulation/allow_sleep",
            SettingsValue::Bool(true),
        ),
        (
            "physics/jolt_physics_3d/simulation/baumgarte_stabilization_factor",
            SettingsValue::Float(0.2),
        ),
        (
            "physics/jolt_physics_3d/simulation/body_pair_contact_cache_angle_threshold",
            SettingsValue::Float(0.034906585),
        ),
        (
            "physics/jolt_physics_3d/simulation/body_pair_contact_cache_distance_threshold",
            SettingsValue::Float(0.001),
        ),
        (
            "physics/jolt_physics_3d/simulation/body_pair_contact_cache_enabled",
            SettingsValue::Bool(true),
        ),
        (
            "physics/jolt_physics_3d/simulation/bounce_velocity_threshold",
            SettingsValue::Float(1.0),
        ),
        (
            "physics/jolt_physics_3d/simulation/continuous_cd_max_penetration",
            SettingsValue::Float(0.25),
        ),
        (
            "physics/jolt_physics_3d/simulation/continuous_cd_movement_threshold",
            SettingsValue::Float(0.75),
        ),
        (
            "physics/jolt_physics_3d/simulation/generate_all_kinematic_contacts",
            SettingsValue::Bool(false),
        ),
        (
            "physics/jolt_physics_3d/simulation/penetration_slop",
            SettingsValue::Float(0.02),
        ),
        (
            "physics/jolt_physics_3d/simulation/position_steps",
            SettingsValue::Int(2),
        ),
        (
            "physics/jolt_physics_3d/simulation/sleep_time_threshold",
            SettingsValue::Float(0.5),
        ),
        (
            "physics/jolt_physics_3d/simulation/sleep_velocity_threshold",
            SettingsValue::Float(0.03),
        ),
        (
            "physics/jolt_physics_3d/simulation/soft_body_point_radius",
            SettingsValue::Float(0.01),
        ),
        (
            "physics/jolt_physics_3d/simulation/speculative_contact_distance",
            SettingsValue::Float(0.02),
        ),
        (
            "physics/jolt_physics_3d/simulation/use_enhanced_internal_edge_removal",
            SettingsValue::Bool(true),
        ),
        (
            "physics/jolt_physics_3d/simulation/velocity_steps",
            SettingsValue::Int(10),
        ),
        (
            "rendering/2d/batching/item_buffer_size",
            SettingsValue::Int(16384),
        ),
        (
            "rendering/2d/batching/uniform_set_cache_size",
            SettingsValue::Int(4096),
        ),
        ("rendering/2d/sdf/oversize", SettingsValue::Int(1)),
        ("rendering/2d/sdf/scale", SettingsValue::Int(1)),
        ("rendering/2d/shadow_atlas/size", SettingsValue::Int(2048)),
        (
            "rendering/2d/snap/snap_2d_transforms_to_pixel",
            SettingsValue::Bool(false),
        ),
        (
            "rendering/2d/snap/snap_2d_vertices_to_pixel",
            SettingsValue::Bool(false),
        ),
        (
            "rendering/environment/defaults/default_clear_color",
            SettingsValue::Color(0.3, 0.3, 0.3, 1.0),
        ),
        (
            "rendering/environment/defaults/default_environment",
            SettingsValue::String(String::new()),
        ),
        (
            "rendering/textures/basis_universal/rdo_dict_size",
            SettingsValue::Int(1024),
        ),
        (
            "rendering/textures/basis_universal/zstd_supercompression",
            SettingsValue::Bool(true),
        ),
        (
            "rendering/textures/basis_universal/zstd_supercompression_level",
            SettingsValue::Int(6),
        ),
        (
            "rendering/textures/canvas_textures/default_texture_filter",
            SettingsValue::Int(1),
        ),
        (
            "rendering/textures/canvas_textures/default_texture_repeat",
            SettingsValue::Int(0),
        ),
        ("rendering/textures/decals/filter", SettingsValue::Int(3)),
        (
            "rendering/textures/default_filters/anisotropic_filtering_level",
            SettingsValue::Int(2),
        ),
        (
            "rendering/textures/default_filters/texture_mipmap_bias",
            SettingsValue::Float(0.0),
        ),
        (
            "rendering/textures/default_filters/use_nearest_mipmap_filter",
            SettingsValue::Bool(false),
        ),
        (
            "rendering/textures/light_projectors/filter",
            SettingsValue::Int(3),
        ),
        (
            "rendering/textures/lossless_compression/force_png",
            SettingsValue::Bool(false),
        ),
        (
            "rendering/textures/vram_compression/cache_gpu_compressor",
            SettingsValue::Bool(true),
        ),
        (
            "rendering/textures/vram_compression/compress_with_gpu",
            SettingsValue::Bool(true),
        ),
        (
            "rendering/textures/vram_compression/import_etc2_astc",
            SettingsValue::Bool(false),
        ),
        (
            "rendering/textures/vram_compression/import_s3tc_bptc",
            SettingsValue::Bool(false),
        ),
        (
            "rendering/textures/webp_compression/compression_method",
            SettingsValue::Int(2),
        ),
        (
            "rendering/textures/webp_compression/lossless_compression_factor",
            SettingsValue::Float(25.0),
        ),
    ] {
        t.insert(key.to_string(), value);
    }
    t
}

/// The runtime-relevant slice of Godot's `GLOBAL_DEF` table.
///
/// Godot registers ~300 core defaults across `project_settings.cpp`,
/// `main.cpp`, the physics and audio servers and `scene_tree.cpp`, and
/// `GLOBAL_GET` answers with the registered value whenever a project has not
/// overridden the key. Patina previously had no such table at all: a loaded
/// `project.godot` carried only its own literal keys, so `get()` returned
/// `None` for every documented setting the file omitted and each caller
/// invented its own `unwrap_or` literal — which is how the same setting ended
/// up with different fallbacks in different places.
///
/// This is the single source those callers now read through. It covers the
/// settings the runtime actually consults rather than all 932 documented
/// members; adding a key here is the way to extend it.
fn godot_default_table() -> BTreeMap<String, SettingsValue> {
    // Start from every documented default in the sections a game queries, then
    // let the curated entries below win where they overlap (pat-mgixk.42).
    let mut t = documented_project_settings_defaults();
    let mut put = |k: &str, v: SettingsValue| {
        t.insert(k.to_string(), v);
    };

    // -- application (project_settings.cpp) ---------------------------------
    put(
        "application/config/name",
        SettingsValue::String(String::new()),
    );
    put(
        "application/config/description",
        SettingsValue::String(String::new()),
    );
    put(
        "application/run/main_scene",
        SettingsValue::String(String::new()),
    );
    put("application/run/disable_stdout", SettingsValue::Bool(false));
    put("application/run/disable_stderr", SettingsValue::Bool(false));
    put(
        "application/run/flush_stdout_on_print",
        SettingsValue::Bool(false),
    );
    // main.cpp:2173-2176 — engine loop caps.
    put("application/run/max_fps", SettingsValue::Int(0));
    put(
        "application/run/low_processor_mode",
        SettingsValue::Bool(false),
    );
    put(
        "application/run/low_processor_mode_sleep_usec",
        SettingsValue::Int(6900),
    );
    put(
        "application/boot_splash/show_image",
        SettingsValue::Bool(true),
    );
    put(
        "application/boot_splash/bg_color",
        SettingsValue::Color(0.14, 0.14, 0.14, 1.0),
    );

    // -- physics/common (main.cpp:2173-2176) --------------------------------
    put(
        "physics/common/physics_ticks_per_second",
        SettingsValue::Int(60),
    );
    put(
        "physics/common/max_physics_steps_per_frame",
        SettingsValue::Int(8),
    );
    put(
        "physics/common/physics_jitter_fix",
        SettingsValue::Float(0.5),
    );

    // -- physics/2d (physics_server_2d.cpp:905-919) -------------------------
    put("physics/2d/default_gravity", SettingsValue::Float(980.0));
    put(
        "physics/2d/default_gravity_vector",
        SettingsValue::Vector2(0.0, 1.0),
    );
    put("physics/2d/default_linear_damp", SettingsValue::Float(0.1));
    put("physics/2d/default_angular_damp", SettingsValue::Float(1.0));
    put(
        "physics/2d/sleep_threshold_linear",
        SettingsValue::Float(2.0),
    );
    put(
        "physics/2d/sleep_threshold_angular",
        SettingsValue::Float(0.139_626_34),
    );
    put("physics/2d/time_before_sleep", SettingsValue::Float(0.5));
    put(
        "physics/2d/solver/solver_iterations",
        SettingsValue::Int(16),
    );

    // -- physics/3d (physics_server_3d.cpp:1141-1154) -----------------------
    put("physics/3d/default_gravity", SettingsValue::Float(9.8));
    put(
        "physics/3d/default_gravity_vector",
        SettingsValue::Vector3(0.0, -1.0, 0.0),
    );
    put("physics/3d/default_linear_damp", SettingsValue::Float(0.1));
    put("physics/3d/default_angular_damp", SettingsValue::Float(0.1));
    put(
        "physics/3d/solver/solver_iterations",
        SettingsValue::Int(16),
    );

    // -- display/window (project_settings.cpp) ------------------------------
    put(
        "display/window/size/viewport_width",
        SettingsValue::Int(1152),
    );
    put(
        "display/window/size/viewport_height",
        SettingsValue::Int(648),
    );
    put("display/window/size/mode", SettingsValue::Int(0));
    put("display/window/size/resizable", SettingsValue::Bool(true));
    put("display/window/size/borderless", SettingsValue::Bool(false));
    put(
        "display/window/size/always_on_top",
        SettingsValue::Bool(false),
    );
    put(
        "display/window/stretch/mode",
        SettingsValue::String("disabled".into()),
    );
    put(
        "display/window/stretch/aspect",
        SettingsValue::String("keep".into()),
    );
    put("display/window/stretch/scale", SettingsValue::Float(1.0));
    put("display/window/vsync/vsync_mode", SettingsValue::Int(1));
    put("display/window/handheld/orientation", SettingsValue::Int(0));

    // -- rendering (main.cpp:3504, project_settings.cpp) --------------------
    put(
        "rendering/renderer/rendering_method",
        SettingsValue::String("forward_plus".into()),
    );
    put(
        "rendering/environment/defaults/default_clear_color",
        SettingsValue::Color(0.3, 0.3, 0.3, 1.0),
    );
    put(
        "rendering/textures/canvas_textures/default_texture_filter",
        SettingsValue::Int(1),
    );
    put(
        "rendering/textures/canvas_textures/default_texture_repeat",
        SettingsValue::Int(0),
    );
    put(
        "rendering/anti_aliasing/quality/msaa_2d",
        SettingsValue::Int(0),
    );
    put(
        "rendering/anti_aliasing/quality/msaa_3d",
        SettingsValue::Int(0),
    );
    put(
        "rendering/occlusion_culling/use_occlusion_culling",
        SettingsValue::Bool(false),
    );

    // -- audio (audio_server.cpp:217-218) -----------------------------------
    put("audio/driver/mix_rate", SettingsValue::Int(44100));
    put("audio/driver/output_latency", SettingsValue::Int(15));
    put(
        "audio/buses/default_bus_layout",
        SettingsValue::String("res://default_bus_layout.tres".into()),
    );

    // -- gui (scene_tree.cpp:2064-2184) -------------------------------------
    put("gui/timers/tooltip_delay_sec", SettingsValue::Float(0.5));
    put(
        "gui/timers/incremental_search_max_interval_msec",
        SettingsValue::Int(2000),
    );
    put(
        "gui/timers/text_edit_idle_detect_sec",
        SettingsValue::Float(3.0),
    );
    put("gui/common/drag_threshold", SettingsValue::Int(10));
    put(
        "gui/common/snap_controls_to_pixels",
        SettingsValue::Bool(true),
    );
    put("gui/theme/default_font_antialiasing", SettingsValue::Int(1));
    put(
        "gui/theme/default_font_subpixel_positioning",
        SettingsValue::Int(1),
    );

    // -- input / navigation / debug -----------------------------------------
    put(
        "input_devices/pointing/emulate_touch_from_mouse",
        SettingsValue::Bool(false),
    );
    put(
        "input_devices/pointing/emulate_mouse_from_touch",
        SettingsValue::Bool(true),
    );
    put("navigation/2d/default_cell_size", SettingsValue::Float(1.0));
    put(
        "navigation/3d/default_cell_size",
        SettingsValue::Float(0.25),
    );
    put(
        "debug/settings/stdout/print_fps",
        SettingsValue::Bool(false),
    );
    put(
        "debug/settings/stdout/verbose_stdout",
        SettingsValue::Bool(false),
    );

    t
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

    /// pat-nzgdu.128: a `name.<feature>` entry overrides the base `name` only
    /// when that feature tag is active (e.g. `web`), matching Godot's
    /// feature-tag override resolution.
    #[test]
    fn project_settings_feature_override() {
        let mut ps = ProjectSettings::new();
        ps.set(
            "display/window/size/viewport_width",
            SettingsValue::Int(1152),
        );
        ps.set(
            "display/window/size/viewport_width.web",
            SettingsValue::Int(800),
        );

        // The `.web` override wins when the `web` feature is active.
        assert_eq!(
            ps.get_setting_with_override_and_custom_features(
                "display/window/size/viewport_width",
                &["web"],
            ),
            Some(&SettingsValue::Int(800)),
        );
        // With no active features, the base value is returned.
        assert_eq!(
            ps.get_setting_with_override_and_custom_features(
                "display/window/size/viewport_width",
                &[],
            ),
            Some(&SettingsValue::Int(1152)),
        );
        // A feature with no matching override falls back to the base value.
        assert_eq!(
            ps.get_setting_with_override_and_custom_features(
                "display/window/size/viewport_width",
                &["mobile"],
            ),
            Some(&SettingsValue::Int(1152)),
        );
        // An unknown setting resolves to None even with features active.
        assert_eq!(
            ps.get_setting_with_override_and_custom_features("nonexistent/setting", &["web"]),
            None,
        );
    }

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
        assert!(physics
            .iter()
            .any(|(k, _)| *k == "physics/common/physics_ticks_per_second"));
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
            ps.get("physics/common/physics_ticks_per_second")
                .unwrap()
                .as_int(),
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
        assert_eq!(
            ps.get("editor/plugin/enabled").unwrap().as_bool(),
            Some(true)
        );
        assert_eq!(
            ps.get("editor/other/disabled").unwrap().as_bool(),
            Some(false)
        );
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
        ps.set(
            "application/config/name",
            SettingsValue::String("Round Trip".into()),
        );
        let output = ps.to_godot_format();
        let ps2 = ProjectSettings::from_godot_format(&output).unwrap();
        assert_eq!(
            ps2.get("application/config/name").unwrap().as_string(),
            Some("Round Trip")
        );
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
        assert_eq!(
            format!("{}", SettingsValue::String("hello".into())),
            "\"hello\""
        );
        assert_eq!(
            format!("{}", SettingsValue::Vector2(1.0, 2.0)),
            "Vector2(1, 2)"
        );
    }

    #[test]
    fn overwrite_setting() {
        let mut ps = ProjectSettings::new();
        ps.set(
            "application/config/name",
            SettingsValue::String("First".into()),
        );
        ps.set(
            "application/config/name",
            SettingsValue::String("Second".into()),
        );
        assert_eq!(ps.project_name(), "Second");
    }

    #[test]
    fn int_promoted_to_float() {
        let v = SettingsValue::Int(60);
        assert_eq!(v.as_float(), Some(60.0));
    }

    #[test]
    fn project_layer_names_get_and_label() {
        let mut ps = ProjectSettings::new();
        // Unset -> no name, default "Layer N" label.
        assert_eq!(ps.layer_name("2d_physics", 1), None);
        assert_eq!(ps.layer_label("2d_physics", 1), "Layer 1");
        // Generic store retains an arbitrary key, as project.godot loading would.
        ps.set(
            "layer_names/2d_physics/layer_1",
            SettingsValue::String("Player".into()),
        );
        assert_eq!(ps.layer_name("2d_physics", 1), Some("Player"));
        assert_eq!(ps.layer_label("2d_physics", 1), "Player");
        // The key is queryable via the generic getter (retention requirement).
        assert_eq!(
            ps.get("layer_names/2d_physics/layer_1")
                .and_then(|v| v.as_string()),
            Some("Player")
        );
        // set_layer_name round-trips; an empty name clears it back to default.
        ps.set_layer_name("3d_render", 5, "Terrain");
        assert_eq!(ps.layer_label("3d_render", 5), "Terrain");
        ps.set_layer_name("3d_render", 5, "");
        assert_eq!(ps.layer_name("3d_render", 5), None);
        assert_eq!(ps.layer_label("3d_render", 5), "Layer 5");
    }

    #[test]
    fn project_display_render_settings() {
        // Defaults match Godot.
        let ps = ProjectSettings::new();
        assert_eq!(ps.viewport_width(), 1152);
        assert_eq!(ps.viewport_height(), 648);
        assert_eq!(ps.max_fps(), 0);
        assert_eq!(ps.default_clear_color(), Color::new(0.3, 0.3, 0.3, 1.0));

        // project.godot-style overrides read back through the typed getters.
        let mut ps = ProjectSettings::new();
        ps.set(
            "display/window/size/viewport_width",
            SettingsValue::Int(800),
        );
        ps.set(
            "display/window/size/viewport_height",
            SettingsValue::Int(600),
        );
        ps.set("application/run/max_fps", SettingsValue::Int(120));
        ps.set(
            "rendering/environment/defaults/default_clear_color",
            SettingsValue::Color(0.25, 0.5, 0.75, 1.0),
        );
        assert_eq!(ps.viewport_width(), 800);
        assert_eq!(ps.viewport_height(), 600);
        assert_eq!(ps.max_fps(), 120);
        assert_eq!(ps.default_clear_color(), Color::new(0.25, 0.5, 0.75, 1.0));
    }

    #[test]
    fn settings_value_as_color() {
        assert_eq!(
            SettingsValue::Color(0.5, 0.25, 0.75, 1.0).as_color(),
            Some(Color::new(0.5, 0.25, 0.75, 1.0))
        );
        assert_eq!(SettingsValue::Int(3).as_color(), None);
    }
}
