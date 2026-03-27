//! Project settings dialog with categorized property editing.
//!
//! Provides a headless model for a Godot-style project settings dialog
//! where settings are organized by category (Application, Display, Physics,
//! Audio, Rendering) with search/filter support.

use std::collections::HashMap;

/// Categories that organize project settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SettingsCategory {
    /// Application name, main scene, description.
    Application,
    /// Window resolution, stretch, fullscreen, vsync.
    Display,
    /// Physics FPS, gravity, damping.
    Physics,
    /// Audio bus layout, master volume.
    Audio,
    /// Renderer backend, anti-aliasing.
    Rendering,
}

impl SettingsCategory {
    /// Returns all categories in display order.
    pub fn all() -> &'static [SettingsCategory] {
        &[
            Self::Application,
            Self::Display,
            Self::Physics,
            Self::Audio,
            Self::Rendering,
        ]
    }

    /// Returns the display name.
    pub fn label(&self) -> &'static str {
        match self {
            Self::Application => "Application",
            Self::Display => "Display",
            Self::Physics => "Physics",
            Self::Audio => "Audio",
            Self::Rendering => "Rendering",
        }
    }

    /// Returns a CSS-safe id string.
    pub fn id(&self) -> &'static str {
        match self {
            Self::Application => "application",
            Self::Display => "display",
            Self::Physics => "physics",
            Self::Audio => "audio",
            Self::Rendering => "rendering",
        }
    }

    /// Parses from a string id.
    pub fn from_id(s: &str) -> Option<Self> {
        match s {
            "application" => Some(Self::Application),
            "display" => Some(Self::Display),
            "physics" => Some(Self::Physics),
            "audio" => Some(Self::Audio),
            "rendering" => Some(Self::Rendering),
            _ => None,
        }
    }
}

/// A single property definition in the settings dialog.
#[derive(Debug, Clone)]
pub struct SettingsProperty {
    /// Machine key (e.g. "application/project_name").
    pub key: String,
    /// Display label.
    pub label: String,
    /// Category this property belongs to.
    pub category: SettingsCategory,
    /// What kind of editor to use.
    pub editor: PropertyEditor,
    /// Current value.
    pub value: SettingsValue,
}

/// The type of editor control for a property.
#[derive(Debug, Clone)]
pub enum PropertyEditor {
    /// Single-line text input.
    Text,
    /// Numeric input with min/max/step.
    Number { min: f64, max: f64, step: f64 },
    /// Integer input with min/max.
    Integer { min: i64, max: i64 },
    /// Dropdown with fixed options.
    Enum(Vec<String>),
    /// Boolean toggle.
    Bool,
}

/// Typed setting value.
#[derive(Debug, Clone, PartialEq)]
pub enum SettingsValue {
    /// String value.
    Text(String),
    /// Floating-point number.
    Number(f64),
    /// Integer value.
    Integer(i64),
    /// Boolean value.
    Bool(bool),
}

impl SettingsValue {
    /// Returns the value as a string for display.
    pub fn as_display(&self) -> String {
        match self {
            Self::Text(s) => s.clone(),
            Self::Number(n) => format!("{n}"),
            Self::Integer(n) => format!("{n}"),
            Self::Bool(b) => format!("{b}"),
        }
    }
}

/// The project settings dialog state.
#[derive(Debug)]
pub struct ProjectSettingsDialog {
    visible: bool,
    active_category: SettingsCategory,
    search_text: String,
    properties: Vec<SettingsProperty>,
    /// Pending changes that haven't been saved yet.
    pending_changes: HashMap<String, SettingsValue>,
}

impl ProjectSettingsDialog {
    /// Creates a new dialog with the default property definitions.
    pub fn new() -> Self {
        Self {
            visible: false,
            active_category: SettingsCategory::Application,
            search_text: String::new(),
            properties: Self::default_properties(),
            pending_changes: HashMap::new(),
        }
    }

    /// Opens the dialog.
    pub fn open(&mut self) {
        self.visible = true;
        self.pending_changes.clear();
        self.search_text.clear();
    }

    /// Closes the dialog without saving.
    pub fn close(&mut self) {
        self.visible = false;
        self.pending_changes.clear();
    }

    /// Returns whether the dialog is visible.
    pub fn is_visible(&self) -> bool {
        self.visible
    }

    /// Sets the active category.
    pub fn set_category(&mut self, category: SettingsCategory) {
        self.active_category = category;
    }

    /// Returns the active category.
    pub fn active_category(&self) -> SettingsCategory {
        self.active_category
    }

    /// Sets the search filter text.
    pub fn set_search(&mut self, text: &str) {
        self.search_text = text.to_lowercase();
    }

    /// Returns properties visible for the current category and search filter.
    pub fn visible_properties(&self) -> Vec<&SettingsProperty> {
        self.properties
            .iter()
            .filter(|p| {
                if !self.search_text.is_empty() {
                    // When searching, show matches across all categories.
                    p.label.to_lowercase().contains(&self.search_text)
                        || p.key.to_lowercase().contains(&self.search_text)
                } else {
                    p.category == self.active_category
                }
            })
            .collect()
    }

    /// Records a pending change for a property key.
    pub fn set_value(&mut self, key: &str, value: SettingsValue) {
        self.pending_changes.insert(key.to_string(), value);
    }

    /// Returns the effective value for a property (pending change or current).
    pub fn effective_value(&self, key: &str) -> Option<&SettingsValue> {
        self.pending_changes.get(key).or_else(|| {
            self.properties
                .iter()
                .find(|p| p.key == key)
                .map(|p| &p.value)
        })
    }

    /// Returns whether there are unsaved changes.
    pub fn has_pending_changes(&self) -> bool {
        !self.pending_changes.is_empty()
    }

    /// Confirms all pending changes, applying them to the properties.
    /// Returns the list of changed key-value pairs.
    pub fn confirm(&mut self) -> Vec<(String, SettingsValue)> {
        let changes: Vec<(String, SettingsValue)> = self.pending_changes.drain().collect();
        for (key, value) in &changes {
            if let Some(prop) = self.properties.iter_mut().find(|p| p.key == *key) {
                prop.value = value.clone();
            }
        }
        self.visible = false;
        changes
    }

    /// Updates the dialog's property values from server data.
    pub fn load_values(&mut self, values: &HashMap<String, SettingsValue>) {
        for (key, value) in values {
            if let Some(prop) = self.properties.iter_mut().find(|p| p.key == *key) {
                prop.value = value.clone();
            }
        }
    }

    /// Returns the number of properties in a given category.
    pub fn category_count(&self, category: SettingsCategory) -> usize {
        self.properties.iter().filter(|p| p.category == category).count()
    }

    /// Returns all categories that have matching properties for the current search.
    pub fn matching_categories(&self) -> Vec<SettingsCategory> {
        if self.search_text.is_empty() {
            return SettingsCategory::all().to_vec();
        }
        let mut cats = Vec::new();
        for cat in SettingsCategory::all() {
            if self.properties.iter().any(|p| {
                p.category == *cat
                    && (p.label.to_lowercase().contains(&self.search_text)
                        || p.key.to_lowercase().contains(&self.search_text))
            }) {
                cats.push(*cat);
            }
        }
        cats
    }

    fn default_properties() -> Vec<SettingsProperty> {
        vec![
            // ---- Application ----
            SettingsProperty {
                key: "application/project_name".into(),
                label: "Project Name".into(),
                category: SettingsCategory::Application,
                editor: PropertyEditor::Text,
                value: SettingsValue::Text("New Project".into()),
            },
            SettingsProperty {
                key: "application/main_scene".into(),
                label: "Main Scene".into(),
                category: SettingsCategory::Application,
                editor: PropertyEditor::Text,
                value: SettingsValue::Text(String::new()),
            },
            SettingsProperty {
                key: "application/description".into(),
                label: "Description".into(),
                category: SettingsCategory::Application,
                editor: PropertyEditor::Text,
                value: SettingsValue::Text(String::new()),
            },
            SettingsProperty {
                key: "application/icon".into(),
                label: "Icon Path".into(),
                category: SettingsCategory::Application,
                editor: PropertyEditor::Text,
                value: SettingsValue::Text(String::new()),
            },
            // ---- Display ----
            SettingsProperty {
                key: "display/resolution_w".into(),
                label: "Resolution Width".into(),
                category: SettingsCategory::Display,
                editor: PropertyEditor::Integer { min: 1, max: 7680 },
                value: SettingsValue::Integer(1152),
            },
            SettingsProperty {
                key: "display/resolution_h".into(),
                label: "Resolution Height".into(),
                category: SettingsCategory::Display,
                editor: PropertyEditor::Integer { min: 1, max: 4320 },
                value: SettingsValue::Integer(648),
            },
            SettingsProperty {
                key: "display/stretch_mode".into(),
                label: "Stretch Mode".into(),
                category: SettingsCategory::Display,
                editor: PropertyEditor::Enum(vec![
                    "disabled".into(),
                    "canvas_items".into(),
                    "viewport".into(),
                ]),
                value: SettingsValue::Text("disabled".into()),
            },
            SettingsProperty {
                key: "display/stretch_aspect".into(),
                label: "Stretch Aspect".into(),
                category: SettingsCategory::Display,
                editor: PropertyEditor::Enum(vec![
                    "ignore".into(),
                    "keep".into(),
                    "keep_width".into(),
                    "keep_height".into(),
                    "expand".into(),
                ]),
                value: SettingsValue::Text("keep".into()),
            },
            SettingsProperty {
                key: "display/fullscreen".into(),
                label: "Fullscreen".into(),
                category: SettingsCategory::Display,
                editor: PropertyEditor::Bool,
                value: SettingsValue::Bool(false),
            },
            SettingsProperty {
                key: "display/vsync".into(),
                label: "V-Sync".into(),
                category: SettingsCategory::Display,
                editor: PropertyEditor::Bool,
                value: SettingsValue::Bool(true),
            },
            // ---- Physics ----
            SettingsProperty {
                key: "physics/fps".into(),
                label: "Physics FPS".into(),
                category: SettingsCategory::Physics,
                editor: PropertyEditor::Integer { min: 1, max: 240 },
                value: SettingsValue::Integer(60),
            },
            SettingsProperty {
                key: "physics/gravity".into(),
                label: "Default Gravity".into(),
                category: SettingsCategory::Physics,
                editor: PropertyEditor::Number {
                    min: 0.0,
                    max: 10000.0,
                    step: 0.1,
                },
                value: SettingsValue::Number(980.0),
            },
            SettingsProperty {
                key: "physics/default_linear_damp".into(),
                label: "Default Linear Damp".into(),
                category: SettingsCategory::Physics,
                editor: PropertyEditor::Number {
                    min: 0.0,
                    max: 100.0,
                    step: 0.01,
                },
                value: SettingsValue::Number(0.1),
            },
            SettingsProperty {
                key: "physics/default_angular_damp".into(),
                label: "Default Angular Damp".into(),
                category: SettingsCategory::Physics,
                editor: PropertyEditor::Number {
                    min: 0.0,
                    max: 100.0,
                    step: 0.01,
                },
                value: SettingsValue::Number(1.0),
            },
            // ---- Audio ----
            SettingsProperty {
                key: "audio/default_bus_layout".into(),
                label: "Default Bus Layout".into(),
                category: SettingsCategory::Audio,
                editor: PropertyEditor::Text,
                value: SettingsValue::Text("res://default_bus_layout.tres".into()),
            },
            SettingsProperty {
                key: "audio/master_volume_db".into(),
                label: "Master Volume (dB)".into(),
                category: SettingsCategory::Audio,
                editor: PropertyEditor::Number {
                    min: -80.0,
                    max: 24.0,
                    step: 0.1,
                },
                value: SettingsValue::Number(0.0),
            },
            SettingsProperty {
                key: "audio/enable_audio_input".into(),
                label: "Enable Audio Input".into(),
                category: SettingsCategory::Audio,
                editor: PropertyEditor::Bool,
                value: SettingsValue::Bool(false),
            },
            // ---- Rendering ----
            SettingsProperty {
                key: "rendering/renderer".into(),
                label: "Renderer".into(),
                category: SettingsCategory::Rendering,
                editor: PropertyEditor::Enum(vec![
                    "forward_plus".into(),
                    "mobile".into(),
                    "compatibility".into(),
                ]),
                value: SettingsValue::Text("forward_plus".into()),
            },
            SettingsProperty {
                key: "rendering/anti_aliasing".into(),
                label: "Anti-Aliasing".into(),
                category: SettingsCategory::Rendering,
                editor: PropertyEditor::Enum(vec![
                    "disabled".into(),
                    "fxaa".into(),
                    "msaa_2x".into(),
                    "msaa_4x".into(),
                    "msaa_8x".into(),
                ]),
                value: SettingsValue::Text("disabled".into()),
            },
            SettingsProperty {
                key: "rendering/environment_default".into(),
                label: "Default Environment".into(),
                category: SettingsCategory::Rendering,
                editor: PropertyEditor::Text,
                value: SettingsValue::Text(String::new()),
            },
        ]
    }
}

impl Default for ProjectSettingsDialog {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_dialog_has_all_categories() {
        let dialog = ProjectSettingsDialog::new();
        for cat in SettingsCategory::all() {
            assert!(
                dialog.category_count(*cat) > 0,
                "category {:?} should have properties",
                cat
            );
        }
    }

    #[test]
    fn category_roundtrip() {
        for cat in SettingsCategory::all() {
            let id = cat.id();
            let parsed = SettingsCategory::from_id(id).unwrap();
            assert_eq!(*cat, parsed);
        }
    }

    #[test]
    fn open_close_clears_pending() {
        let mut dialog = ProjectSettingsDialog::new();
        dialog.open();
        assert!(dialog.is_visible());
        dialog.set_value(
            "application/project_name",
            SettingsValue::Text("Test".into()),
        );
        assert!(dialog.has_pending_changes());
        dialog.close();
        assert!(!dialog.is_visible());
        assert!(!dialog.has_pending_changes());
    }

    #[test]
    fn visible_properties_filters_by_category() {
        let mut dialog = ProjectSettingsDialog::new();
        dialog.set_category(SettingsCategory::Physics);
        let props = dialog.visible_properties();
        assert!(props.len() >= 2);
        for p in &props {
            assert_eq!(p.category, SettingsCategory::Physics);
        }
    }

    #[test]
    fn search_crosses_categories() {
        let mut dialog = ProjectSettingsDialog::new();
        dialog.set_search("gravity");
        let props = dialog.visible_properties();
        assert_eq!(props.len(), 1);
        assert_eq!(props[0].key, "physics/gravity");
    }

    #[test]
    fn confirm_applies_changes() {
        let mut dialog = ProjectSettingsDialog::new();
        dialog.open();
        dialog.set_value(
            "application/project_name",
            SettingsValue::Text("My Game".into()),
        );
        dialog.set_value("physics/fps", SettingsValue::Integer(120));
        let changes = dialog.confirm();
        assert_eq!(changes.len(), 2);
        assert!(!dialog.is_visible());

        // Values should be applied
        let val = dialog.effective_value("application/project_name").unwrap();
        assert_eq!(*val, SettingsValue::Text("My Game".into()));
    }

    #[test]
    fn load_values_updates_properties() {
        let mut dialog = ProjectSettingsDialog::new();
        let mut values = HashMap::new();
        values.insert(
            "application/project_name".to_string(),
            SettingsValue::Text("Loaded".into()),
        );
        values.insert("physics/fps".to_string(), SettingsValue::Integer(120));
        dialog.load_values(&values);

        let name = dialog.effective_value("application/project_name").unwrap();
        assert_eq!(*name, SettingsValue::Text("Loaded".into()));
    }

    #[test]
    fn matching_categories_with_search() {
        let mut dialog = ProjectSettingsDialog::new();
        dialog.set_search("gravity");
        let cats = dialog.matching_categories();
        assert_eq!(cats, vec![SettingsCategory::Physics]);
    }

    #[test]
    fn effective_value_prefers_pending() {
        let mut dialog = ProjectSettingsDialog::new();
        dialog.open();
        dialog.set_value(
            "application/project_name",
            SettingsValue::Text("Pending".into()),
        );
        let val = dialog.effective_value("application/project_name").unwrap();
        assert_eq!(*val, SettingsValue::Text("Pending".into()));
    }

    #[test]
    fn settings_value_display() {
        assert_eq!(SettingsValue::Text("hello".into()).as_display(), "hello");
        assert_eq!(SettingsValue::Number(3.14).as_display(), "3.14");
        assert_eq!(SettingsValue::Integer(42).as_display(), "42");
        assert_eq!(SettingsValue::Bool(true).as_display(), "true");
    }
}
