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
    pub editor: SettingsEditor,
    /// Current value.
    pub value: SettingsValue,
    /// Description/tooltip text.
    pub description: String,
    /// Whether this is an advanced property (hidden by default).
    pub advanced: bool,
    /// Whether this is a user-defined custom property.
    pub custom: bool,
    /// The default value for reset.
    pub default_value: SettingsValue,
}

/// Validation error for a settings value.
#[derive(Debug, Clone, PartialEq)]
pub enum SettingsValidationError {
    /// Number out of range.
    NumberOutOfRange {
        key: String,
        value: f64,
        min: f64,
        max: f64,
    },
    /// Integer out of range.
    IntegerOutOfRange {
        key: String,
        value: i64,
        min: i64,
        max: i64,
    },
    /// Value not in the allowed enum options.
    InvalidEnumValue {
        key: String,
        value: String,
        options: Vec<String>,
    },
    /// Type mismatch (expected vs actual).
    TypeMismatch {
        key: String,
        expected: String,
        actual: String,
    },
}

/// The type of editor control for a property.
#[derive(Debug, Clone)]
pub enum SettingsEditor {
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
    /// Whether to show advanced properties.
    show_advanced: bool,
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
            show_advanced: false,
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
    ///
    /// Respects the advanced toggle: when `show_advanced` is false, advanced
    /// properties are hidden unless a search is active.
    pub fn visible_properties(&self) -> Vec<&SettingsProperty> {
        self.properties
            .iter()
            .filter(|p| {
                // Hide advanced properties unless toggled on or searching
                if p.advanced && !self.show_advanced && self.search_text.is_empty() {
                    return false;
                }
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

    /// Returns whether advanced properties are shown.
    pub fn show_advanced(&self) -> bool {
        self.show_advanced
    }

    /// Toggles the advanced property visibility.
    pub fn toggle_advanced(&mut self) {
        self.show_advanced = !self.show_advanced;
    }

    /// Sets the advanced property visibility.
    pub fn set_show_advanced(&mut self, show: bool) {
        self.show_advanced = show;
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
        self.properties
            .iter()
            .filter(|p| p.category == category)
            .count()
    }

    /// Validates a pending value against its editor constraints.
    ///
    /// Returns errors if the value doesn't match the editor's constraints.
    pub fn validate_value(&self, key: &str, value: &SettingsValue) -> Vec<SettingsValidationError> {
        let mut errors = Vec::new();
        let Some(prop) = self.properties.iter().find(|p| p.key == key) else {
            return errors;
        };
        match (&prop.editor, value) {
            (SettingsEditor::Number { min, max, .. }, SettingsValue::Number(n)) => {
                if *n < *min || *n > *max {
                    errors.push(SettingsValidationError::NumberOutOfRange {
                        key: key.into(),
                        value: *n,
                        min: *min,
                        max: *max,
                    });
                }
            }
            (SettingsEditor::Integer { min, max }, SettingsValue::Integer(n)) => {
                if *n < *min || *n > *max {
                    errors.push(SettingsValidationError::IntegerOutOfRange {
                        key: key.into(),
                        value: *n,
                        min: *min,
                        max: *max,
                    });
                }
            }
            (SettingsEditor::Enum(options), SettingsValue::Text(s)) => {
                if !options.contains(s) {
                    errors.push(SettingsValidationError::InvalidEnumValue {
                        key: key.into(),
                        value: s.clone(),
                        options: options.clone(),
                    });
                }
            }
            (SettingsEditor::Bool, SettingsValue::Bool(_)) => {}
            (SettingsEditor::Text, SettingsValue::Text(_)) => {}
            (editor, val) => {
                let expected = match editor {
                    SettingsEditor::Text => "Text",
                    SettingsEditor::Number { .. } => "Number",
                    SettingsEditor::Integer { .. } => "Integer",
                    SettingsEditor::Enum(_) => "Enum (Text)",
                    SettingsEditor::Bool => "Bool",
                };
                let actual = match val {
                    SettingsValue::Text(_) => "Text",
                    SettingsValue::Number(_) => "Number",
                    SettingsValue::Integer(_) => "Integer",
                    SettingsValue::Bool(_) => "Bool",
                };
                errors.push(SettingsValidationError::TypeMismatch {
                    key: key.into(),
                    expected: expected.into(),
                    actual: actual.into(),
                });
            }
        }
        errors
    }

    /// Validates all pending changes. Returns all validation errors.
    pub fn validate_pending(&self) -> Vec<SettingsValidationError> {
        let mut errors = Vec::new();
        for (key, value) in &self.pending_changes {
            errors.extend(self.validate_value(key, value));
        }
        errors
    }

    /// Resets a single property to its default value, recording it as a pending change.
    pub fn reset_to_default(&mut self, key: &str) -> bool {
        if let Some(prop) = self.properties.iter().find(|p| p.key == key) {
            let default = prop.default_value.clone();
            self.pending_changes.insert(key.to_string(), default);
            true
        } else {
            false
        }
    }

    /// Resets all properties to their default values.
    pub fn reset_all_to_defaults(&mut self) {
        for prop in &self.properties {
            if prop.value != prop.default_value {
                self.pending_changes
                    .insert(prop.key.clone(), prop.default_value.clone());
            }
        }
    }

    /// Returns true if the property at the given key has been modified from default.
    pub fn is_modified(&self, key: &str) -> bool {
        if let Some(prop) = self.properties.iter().find(|p| p.key == key) {
            let effective = self.pending_changes.get(key).unwrap_or(&prop.value);
            *effective != prop.default_value
        } else {
            false
        }
    }

    /// Returns the description/tooltip for a property.
    pub fn property_description(&self, key: &str) -> Option<&str> {
        self.properties
            .iter()
            .find(|p| p.key == key)
            .map(|p| p.description.as_str())
    }

    /// Adds a custom user-defined property.
    ///
    /// Returns true if added, false if a property with that key already exists.
    pub fn add_custom_property(
        &mut self,
        key: impl Into<String>,
        label: impl Into<String>,
        category: SettingsCategory,
        editor: SettingsEditor,
        value: SettingsValue,
        description: impl Into<String>,
    ) -> bool {
        let key = key.into();
        if self.properties.iter().any(|p| p.key == key) {
            return false;
        }
        self.properties.push(SettingsProperty {
            key,
            label: label.into(),
            category,
            editor,
            default_value: value.clone(),
            value,
            description: description.into(),
            advanced: false,
            custom: true,
        });
        true
    }

    /// Removes a custom property by key. Only custom properties can be removed.
    pub fn remove_custom_property(&mut self, key: &str) -> bool {
        let len_before = self.properties.len();
        self.properties.retain(|p| !(p.custom && p.key == key));
        self.pending_changes.remove(key);
        self.properties.len() < len_before
    }

    /// Returns all custom properties.
    pub fn custom_properties(&self) -> Vec<&SettingsProperty> {
        self.properties.iter().filter(|p| p.custom).collect()
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

    fn prop(
        key: &str,
        label: &str,
        category: SettingsCategory,
        editor: SettingsEditor,
        value: SettingsValue,
        description: &str,
        advanced: bool,
    ) -> SettingsProperty {
        SettingsProperty {
            key: key.into(),
            label: label.into(),
            category,
            editor,
            default_value: value.clone(),
            value,
            description: description.into(),
            advanced,
            custom: false,
        }
    }

    fn default_properties() -> Vec<SettingsProperty> {
        vec![
            // ---- Application ----
            Self::prop(
                "application/project_name",
                "Project Name",
                SettingsCategory::Application,
                SettingsEditor::Text,
                SettingsValue::Text("New Project".into()),
                "The name of the project, displayed in the title bar.",
                false,
            ),
            Self::prop(
                "application/main_scene",
                "Main Scene",
                SettingsCategory::Application,
                SettingsEditor::Text,
                SettingsValue::Text(String::new()),
                "The scene to load when the project runs.",
                false,
            ),
            Self::prop(
                "application/description",
                "Description",
                SettingsCategory::Application,
                SettingsEditor::Text,
                SettingsValue::Text(String::new()),
                "A short description of the project.",
                false,
            ),
            Self::prop(
                "application/icon",
                "Icon Path",
                SettingsCategory::Application,
                SettingsEditor::Text,
                SettingsValue::Text(String::new()),
                "Path to the project icon.",
                false,
            ),
            // ---- Display ----
            Self::prop(
                "display/resolution_w",
                "Resolution Width",
                SettingsCategory::Display,
                SettingsEditor::Integer { min: 1, max: 7680 },
                SettingsValue::Integer(1152),
                "Viewport width in pixels.",
                false,
            ),
            Self::prop(
                "display/resolution_h",
                "Resolution Height",
                SettingsCategory::Display,
                SettingsEditor::Integer { min: 1, max: 4320 },
                SettingsValue::Integer(648),
                "Viewport height in pixels.",
                false,
            ),
            Self::prop(
                "display/stretch_mode",
                "Stretch Mode",
                SettingsCategory::Display,
                SettingsEditor::Enum(vec![
                    "disabled".into(),
                    "canvas_items".into(),
                    "viewport".into(),
                ]),
                SettingsValue::Text("disabled".into()),
                "How the viewport scales when the window is resized.",
                false,
            ),
            Self::prop(
                "display/stretch_aspect",
                "Stretch Aspect",
                SettingsCategory::Display,
                SettingsEditor::Enum(vec![
                    "ignore".into(),
                    "keep".into(),
                    "keep_width".into(),
                    "keep_height".into(),
                    "expand".into(),
                ]),
                SettingsValue::Text("keep".into()),
                "Aspect ratio handling when stretching.",
                true,
            ),
            Self::prop(
                "display/fullscreen",
                "Fullscreen",
                SettingsCategory::Display,
                SettingsEditor::Bool,
                SettingsValue::Bool(false),
                "Whether the window starts in fullscreen mode.",
                false,
            ),
            Self::prop(
                "display/vsync",
                "V-Sync",
                SettingsCategory::Display,
                SettingsEditor::Bool,
                SettingsValue::Bool(true),
                "Enable vertical synchronization.",
                false,
            ),
            // ---- Physics ----
            Self::prop(
                "physics/fps",
                "Physics FPS",
                SettingsCategory::Physics,
                SettingsEditor::Integer { min: 1, max: 240 },
                SettingsValue::Integer(60),
                "Number of physics simulation steps per second.",
                false,
            ),
            Self::prop(
                "physics/gravity",
                "Default Gravity",
                SettingsCategory::Physics,
                SettingsEditor::Number {
                    min: 0.0,
                    max: 10000.0,
                    step: 0.1,
                },
                SettingsValue::Number(980.0),
                "Default gravity strength (pixels/s² for 2D, m/s² for 3D).",
                false,
            ),
            Self::prop(
                "physics/default_linear_damp",
                "Default Linear Damp",
                SettingsCategory::Physics,
                SettingsEditor::Number {
                    min: 0.0,
                    max: 100.0,
                    step: 0.01,
                },
                SettingsValue::Number(0.1),
                "Default linear damping for rigid bodies.",
                true,
            ),
            Self::prop(
                "physics/default_angular_damp",
                "Default Angular Damp",
                SettingsCategory::Physics,
                SettingsEditor::Number {
                    min: 0.0,
                    max: 100.0,
                    step: 0.01,
                },
                SettingsValue::Number(1.0),
                "Default angular damping for rigid bodies.",
                true,
            ),
            // ---- Audio ----
            Self::prop(
                "audio/default_bus_layout",
                "Default Bus Layout",
                SettingsCategory::Audio,
                SettingsEditor::Text,
                SettingsValue::Text("res://default_bus_layout.tres".into()),
                "Path to the default audio bus layout resource.",
                false,
            ),
            Self::prop(
                "audio/master_volume_db",
                "Master Volume (dB)",
                SettingsCategory::Audio,
                SettingsEditor::Number {
                    min: -80.0,
                    max: 24.0,
                    step: 0.1,
                },
                SettingsValue::Number(0.0),
                "Master output volume in decibels.",
                false,
            ),
            Self::prop(
                "audio/enable_audio_input",
                "Enable Audio Input",
                SettingsCategory::Audio,
                SettingsEditor::Bool,
                SettingsValue::Bool(false),
                "Allow microphone and audio input capture.",
                true,
            ),
            // ---- Rendering ----
            Self::prop(
                "rendering/renderer",
                "Renderer",
                SettingsCategory::Rendering,
                SettingsEditor::Enum(vec![
                    "forward_plus".into(),
                    "mobile".into(),
                    "compatibility".into(),
                ]),
                SettingsValue::Text("forward_plus".into()),
                "Rendering backend to use.",
                false,
            ),
            Self::prop(
                "rendering/anti_aliasing",
                "Anti-Aliasing",
                SettingsCategory::Rendering,
                SettingsEditor::Enum(vec![
                    "disabled".into(),
                    "fxaa".into(),
                    "msaa_2x".into(),
                    "msaa_4x".into(),
                    "msaa_8x".into(),
                ]),
                SettingsValue::Text("disabled".into()),
                "Anti-aliasing method.",
                false,
            ),
            Self::prop(
                "rendering/environment_default",
                "Default Environment",
                SettingsCategory::Rendering,
                SettingsEditor::Text,
                SettingsValue::Text(String::new()),
                "Path to the default environment resource.",
                true,
            ),
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

    // -- Advanced toggle --

    #[test]
    fn advanced_hidden_by_default() {
        let dialog = ProjectSettingsDialog::new();
        assert!(!dialog.show_advanced());
    }

    #[test]
    fn advanced_properties_hidden_when_not_toggled() {
        let mut dialog = ProjectSettingsDialog::new();
        dialog.set_category(SettingsCategory::Display);
        let props = dialog.visible_properties();
        // stretch_aspect is advanced
        assert!(!props.iter().any(|p| p.key == "display/stretch_aspect"));
    }

    #[test]
    fn advanced_properties_shown_when_toggled() {
        let mut dialog = ProjectSettingsDialog::new();
        dialog.set_category(SettingsCategory::Display);
        dialog.toggle_advanced();
        assert!(dialog.show_advanced());
        let props = dialog.visible_properties();
        assert!(props.iter().any(|p| p.key == "display/stretch_aspect"));
    }

    #[test]
    fn search_shows_advanced_regardless() {
        let mut dialog = ProjectSettingsDialog::new();
        assert!(!dialog.show_advanced());
        dialog.set_search("stretch_aspect");
        let props = dialog.visible_properties();
        assert_eq!(props.len(), 1);
        assert_eq!(props[0].key, "display/stretch_aspect");
    }

    // -- Validation --

    #[test]
    fn validate_integer_in_range() {
        let dialog = ProjectSettingsDialog::new();
        let errors = dialog.validate_value("physics/fps", &SettingsValue::Integer(60));
        assert!(errors.is_empty());
    }

    #[test]
    fn validate_integer_out_of_range() {
        let dialog = ProjectSettingsDialog::new();
        let errors = dialog.validate_value("physics/fps", &SettingsValue::Integer(999));
        assert_eq!(errors.len(), 1);
        assert!(matches!(
            errors[0],
            SettingsValidationError::IntegerOutOfRange { .. }
        ));
    }

    #[test]
    fn validate_number_in_range() {
        let dialog = ProjectSettingsDialog::new();
        let errors = dialog.validate_value("physics/gravity", &SettingsValue::Number(500.0));
        assert!(errors.is_empty());
    }

    #[test]
    fn validate_number_out_of_range() {
        let dialog = ProjectSettingsDialog::new();
        let errors = dialog.validate_value("physics/gravity", &SettingsValue::Number(-1.0));
        assert_eq!(errors.len(), 1);
        assert!(matches!(
            errors[0],
            SettingsValidationError::NumberOutOfRange { .. }
        ));
    }

    #[test]
    fn validate_enum_valid() {
        let dialog = ProjectSettingsDialog::new();
        let errors =
            dialog.validate_value("rendering/renderer", &SettingsValue::Text("mobile".into()));
        assert!(errors.is_empty());
    }

    #[test]
    fn validate_enum_invalid() {
        let dialog = ProjectSettingsDialog::new();
        let errors = dialog.validate_value(
            "rendering/renderer",
            &SettingsValue::Text("vulkan_raw".into()),
        );
        assert_eq!(errors.len(), 1);
        assert!(matches!(
            errors[0],
            SettingsValidationError::InvalidEnumValue { .. }
        ));
    }

    #[test]
    fn validate_type_mismatch() {
        let dialog = ProjectSettingsDialog::new();
        let errors = dialog.validate_value("physics/fps", &SettingsValue::Text("sixty".into()));
        assert_eq!(errors.len(), 1);
        assert!(matches!(
            errors[0],
            SettingsValidationError::TypeMismatch { .. }
        ));
    }

    #[test]
    fn validate_pending_all() {
        let mut dialog = ProjectSettingsDialog::new();
        dialog.set_value("physics/fps", SettingsValue::Integer(999));
        dialog.set_value("rendering/renderer", SettingsValue::Text("vulkan".into()));
        let errors = dialog.validate_pending();
        assert_eq!(errors.len(), 2);
    }

    // -- Reset to default --

    #[test]
    fn reset_single_property() {
        let mut dialog = ProjectSettingsDialog::new();
        dialog.open();
        dialog.set_value(
            "application/project_name",
            SettingsValue::Text("Changed".into()),
        );
        dialog.confirm();
        assert_eq!(
            *dialog.effective_value("application/project_name").unwrap(),
            SettingsValue::Text("Changed".into()),
        );

        dialog.reset_to_default("application/project_name");
        assert_eq!(
            *dialog.effective_value("application/project_name").unwrap(),
            SettingsValue::Text("New Project".into()),
        );
    }

    #[test]
    fn reset_unknown_key_returns_false() {
        let mut dialog = ProjectSettingsDialog::new();
        assert!(!dialog.reset_to_default("nonexistent/key"));
    }

    #[test]
    fn reset_all_to_defaults() {
        let mut dialog = ProjectSettingsDialog::new();
        dialog.open();
        dialog.set_value("application/project_name", SettingsValue::Text("X".into()));
        dialog.set_value("physics/fps", SettingsValue::Integer(120));
        dialog.confirm();

        dialog.reset_all_to_defaults();
        assert!(dialog.has_pending_changes());
        dialog.confirm();

        assert_eq!(
            *dialog.effective_value("application/project_name").unwrap(),
            SettingsValue::Text("New Project".into()),
        );
        assert_eq!(
            *dialog.effective_value("physics/fps").unwrap(),
            SettingsValue::Integer(60),
        );
    }

    #[test]
    fn is_modified_tracks_changes() {
        let mut dialog = ProjectSettingsDialog::new();
        assert!(!dialog.is_modified("application/project_name"));
        dialog.set_value(
            "application/project_name",
            SettingsValue::Text("Changed".into()),
        );
        assert!(dialog.is_modified("application/project_name"));
    }

    // -- Property description --

    #[test]
    fn property_description() {
        let dialog = ProjectSettingsDialog::new();
        let desc = dialog.property_description("physics/gravity").unwrap();
        assert!(!desc.is_empty());
        assert!(desc.contains("gravity"));
    }

    #[test]
    fn property_description_unknown_key() {
        let dialog = ProjectSettingsDialog::new();
        assert!(dialog.property_description("nonexistent").is_none());
    }

    // -- Custom properties --

    #[test]
    fn add_custom_property() {
        let mut dialog = ProjectSettingsDialog::new();
        let added = dialog.add_custom_property(
            "custom/my_setting",
            "My Setting",
            SettingsCategory::Application,
            SettingsEditor::Text,
            SettingsValue::Text("hello".into()),
            "A custom user setting.",
        );
        assert!(added);
        assert_eq!(dialog.custom_properties().len(), 1);
        let prop = dialog.custom_properties()[0];
        assert_eq!(prop.key, "custom/my_setting");
        assert!(prop.custom);
    }

    #[test]
    fn add_custom_property_duplicate_rejected() {
        let mut dialog = ProjectSettingsDialog::new();
        dialog.add_custom_property(
            "custom/x",
            "X",
            SettingsCategory::Application,
            SettingsEditor::Bool,
            SettingsValue::Bool(false),
            "",
        );
        let added = dialog.add_custom_property(
            "custom/x",
            "X Again",
            SettingsCategory::Application,
            SettingsEditor::Bool,
            SettingsValue::Bool(true),
            "",
        );
        assert!(!added);
    }

    #[test]
    fn add_custom_property_conflicting_builtin_rejected() {
        let mut dialog = ProjectSettingsDialog::new();
        let added = dialog.add_custom_property(
            "physics/fps",
            "Override FPS",
            SettingsCategory::Physics,
            SettingsEditor::Integer { min: 1, max: 999 },
            SettingsValue::Integer(30),
            "",
        );
        assert!(!added);
    }

    #[test]
    fn remove_custom_property() {
        let mut dialog = ProjectSettingsDialog::new();
        dialog.add_custom_property(
            "custom/temp",
            "Temp",
            SettingsCategory::Audio,
            SettingsEditor::Text,
            SettingsValue::Text("x".into()),
            "",
        );
        assert_eq!(dialog.custom_properties().len(), 1);
        let removed = dialog.remove_custom_property("custom/temp");
        assert!(removed);
        assert!(dialog.custom_properties().is_empty());
    }

    #[test]
    fn remove_builtin_property_fails() {
        let mut dialog = ProjectSettingsDialog::new();
        let count_before = dialog.category_count(SettingsCategory::Physics);
        let removed = dialog.remove_custom_property("physics/fps");
        assert!(!removed);
        assert_eq!(
            dialog.category_count(SettingsCategory::Physics),
            count_before
        );
    }

    #[test]
    fn custom_property_visible_in_category() {
        let mut dialog = ProjectSettingsDialog::new();
        dialog.add_custom_property(
            "custom/game_mode",
            "Game Mode",
            SettingsCategory::Application,
            SettingsEditor::Enum(vec!["single".into(), "multi".into()]),
            SettingsValue::Text("single".into()),
            "Select game mode.",
        );
        dialog.set_category(SettingsCategory::Application);
        let props = dialog.visible_properties();
        assert!(props.iter().any(|p| p.key == "custom/game_mode"));
    }

    #[test]
    fn custom_property_searchable() {
        let mut dialog = ProjectSettingsDialog::new();
        dialog.add_custom_property(
            "custom/difficulty",
            "Difficulty Level",
            SettingsCategory::Application,
            SettingsEditor::Integer { min: 1, max: 10 },
            SettingsValue::Integer(5),
            "",
        );
        dialog.set_search("difficulty");
        let props = dialog.visible_properties();
        assert_eq!(props.len(), 1);
        assert_eq!(props[0].key, "custom/difficulty");
    }
}
