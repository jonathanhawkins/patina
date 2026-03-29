//! Property inspector and editing interface.
//!
//! The [`InspectorPanel`] provides a view into a node's properties,
//! organized by category. It supports listing, getting, and setting
//! properties, and notifying listeners when a property changes.

#![allow(clippy::type_complexity)]

use std::collections::{HashMap, HashSet};

use gdscene::node::NodeId;
use gdscene::SceneTree;
use gdvariant::variant::VariantType;
use gdvariant::Variant;

/// A category grouping for inspector properties.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum PropertyCategory {
    /// Transform-related properties (position, rotation, scale).
    Transform,
    /// Visual/rendering properties (texture, color, visible).
    Rendering,
    /// Physics-related properties (velocity, mass).
    Physics,
    /// Script/user-defined properties.
    Script,
    /// Anything that doesn't match a known category.
    Misc,
}

impl PropertyCategory {
    /// Categorizes a property name into a [`PropertyCategory`].
    pub fn categorize(name: &str) -> Self {
        match name {
            "position" | "rotation" | "scale" | "transform" | "global_position"
            | "global_rotation" | "global_scale" | "global_transform" | "skew" => Self::Transform,
            "visible" | "modulate" | "self_modulate" | "texture" | "color" | "z_index"
            | "z_as_relative" | "material" | "light_mask" => Self::Rendering,
            "velocity" | "mass" | "gravity_scale" | "linear_velocity" | "angular_velocity"
            | "friction" | "bounce" => Self::Physics,
            _ if name.starts_with("script_") || name.starts_with("metadata/") => Self::Script,
            _ => Self::Misc,
        }
    }
}

/// A snapshot of a single property's state.
#[derive(Debug, Clone)]
pub struct PropertyEntry {
    /// The property name.
    pub name: String,
    /// The current value.
    pub value: Variant,
    /// The category this property belongs to.
    pub category: PropertyCategory,
}

/// Callback type for property change notifications.
type PropertyChangedCallback = Box<dyn Fn(&str, &Variant, &Variant)>;

/// Metadata for an exported script variable's grouping.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportGroup {
    /// The group name (from `@export_group("name")`).
    pub group: String,
    /// Optional subgroup name (from `@export_subgroup("name")`).
    pub subgroup: Option<String>,
}

/// The inspector panel for viewing and editing node properties.
///
/// Mirrors Godot's inspector dock — it reflects the properties of a
/// selected node and supports change callbacks.
pub struct InspectorPanel {
    /// The node currently being inspected, if any.
    inspected_node: Option<NodeId>,
    /// Callbacks invoked when a property changes.
    /// Key is an optional property name filter; `None` means all properties.
    on_changed: Vec<(Option<String>, PropertyChangedCallback)>,
    /// Properties the user has marked as favorites.
    favorite_properties: HashSet<String>,
    /// Export group assignments for script properties.
    export_groups: HashMap<String, ExportGroup>,
}

impl std::fmt::Debug for InspectorPanel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InspectorPanel")
            .field("inspected_node", &self.inspected_node)
            .field("callback_count", &self.on_changed.len())
            .field("favorite_count", &self.favorite_properties.len())
            .finish()
    }
}

impl InspectorPanel {
    /// Creates a new empty inspector panel.
    pub fn new() -> Self {
        Self {
            inspected_node: None,
            on_changed: Vec::new(),
            favorite_properties: HashSet::new(),
            export_groups: HashMap::new(),
        }
    }

    /// Sets the node to inspect.
    pub fn inspect(&mut self, node_id: NodeId) {
        self.inspected_node = Some(node_id);
        tracing::debug!("Inspector now inspecting node {:?}", node_id);
    }

    /// Clears the inspected node.
    pub fn clear(&mut self) {
        self.inspected_node = None;
    }

    /// Returns the currently inspected node, if any.
    pub fn inspected_node(&self) -> Option<NodeId> {
        self.inspected_node
    }

    /// Lists all properties of the inspected node, grouped by category.
    pub fn list_properties(&self, tree: &SceneTree) -> Vec<PropertyEntry> {
        let node_id = match self.inspected_node {
            Some(id) => id,
            None => return Vec::new(),
        };
        let node = match tree.get_node(node_id) {
            Some(n) => n,
            None => return Vec::new(),
        };

        let mut entries: Vec<PropertyEntry> = node
            .properties()
            .map(|(name, value)| PropertyEntry {
                name: name.clone(),
                value: value.clone(),
                category: PropertyCategory::categorize(name),
            })
            .collect();
        entries.sort_by(|a, b| a.name.cmp(&b.name));
        entries
    }

    /// Lists properties grouped by category.
    pub fn list_properties_by_category(
        &self,
        tree: &SceneTree,
    ) -> HashMap<PropertyCategory, Vec<PropertyEntry>> {
        let entries = self.list_properties(tree);
        let mut grouped: HashMap<PropertyCategory, Vec<PropertyEntry>> = HashMap::new();
        for entry in entries {
            grouped
                .entry(entry.category.clone())
                .or_default()
                .push(entry);
        }
        grouped
    }

    /// Gets a single property value from the inspected node.
    pub fn get_property(&self, tree: &SceneTree, name: &str) -> Variant {
        let node_id = match self.inspected_node {
            Some(id) => id,
            None => return Variant::Nil,
        };
        match tree.get_node(node_id) {
            Some(node) => node.get_property(name),
            None => Variant::Nil,
        }
    }

    /// Sets a property on the inspected node and fires change callbacks.
    ///
    /// Returns the old value.
    pub fn set_property(&self, tree: &mut SceneTree, name: &str, value: Variant) -> Variant {
        let node_id = match self.inspected_node {
            Some(id) => id,
            None => return Variant::Nil,
        };
        let old_value = match tree.get_node_mut(node_id) {
            Some(node) => node.set_property(name, value.clone()),
            None => return Variant::Nil,
        };

        // Fire callbacks.
        for (filter, callback) in &self.on_changed {
            match filter {
                Some(f) if f != name => continue,
                _ => callback(name, &old_value, &value),
            }
        }

        old_value
    }

    /// Registers a callback that fires when any property changes.
    pub fn on_property_changed(&mut self, callback: impl Fn(&str, &Variant, &Variant) + 'static) {
        self.on_changed.push((None, Box::new(callback)));
    }

    /// Registers a callback that fires only when a specific property changes.
    pub fn on_specific_property_changed(
        &mut self,
        property: impl Into<String>,
        callback: impl Fn(&str, &Variant, &Variant) + 'static,
    ) {
        self.on_changed
            .push((Some(property.into()), Box::new(callback)));
    }

    // -- Favorites API --

    /// Adds a property to favorites.
    pub fn add_favorite(&mut self, property: impl Into<String>) {
        self.favorite_properties.insert(property.into());
    }

    /// Removes a property from favorites.
    pub fn remove_favorite(&mut self, property: &str) {
        self.favorite_properties.remove(property);
    }

    /// Toggles a property's favorite status. Returns `true` if now a favorite.
    pub fn toggle_favorite(&mut self, property: &str) -> bool {
        if self.favorite_properties.contains(property) {
            self.favorite_properties.remove(property);
            false
        } else {
            self.favorite_properties.insert(property.to_string());
            true
        }
    }

    /// Returns whether a property is a favorite.
    pub fn is_favorite(&self, property: &str) -> bool {
        self.favorite_properties.contains(property)
    }

    /// Returns all favorite property names, sorted.
    pub fn favorites(&self) -> Vec<String> {
        let mut favs: Vec<String> = self.favorite_properties.iter().cloned().collect();
        favs.sort();
        favs
    }

    /// Returns favorite properties with their values from the inspected node.
    pub fn favorite_entries(&self, tree: &SceneTree) -> Vec<PropertyEntry> {
        if self.favorite_properties.is_empty() {
            return Vec::new();
        }
        self.list_properties(tree)
            .into_iter()
            .filter(|e| self.favorite_properties.contains(&e.name))
            .collect()
    }

    // -- Export group API --

    /// Sets the export group for a property.
    pub fn set_export_group(
        &mut self,
        property: impl Into<String>,
        group: impl Into<String>,
        subgroup: Option<String>,
    ) {
        self.export_groups.insert(
            property.into(),
            ExportGroup {
                group: group.into(),
                subgroup,
            },
        );
    }

    /// Returns the export group for a property, if any.
    pub fn export_group_for(&self, property: &str) -> Option<&ExportGroup> {
        self.export_groups.get(property)
    }

    /// Returns all properties organized by export group.
    pub fn properties_by_export_group(
        &self,
        tree: &SceneTree,
    ) -> HashMap<String, Vec<PropertyEntry>> {
        let entries = self.list_properties(tree);
        let mut grouped: HashMap<String, Vec<PropertyEntry>> = HashMap::new();
        for entry in entries {
            if let Some(eg) = self.export_groups.get(&entry.name) {
                grouped.entry(eg.group.clone()).or_default().push(entry);
            }
        }
        grouped
    }

    /// Clears all export group assignments.
    pub fn clear_export_groups(&mut self) {
        self.export_groups.clear();
    }
}

impl Default for InspectorPanel {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// EditorInspectorPlugin — custom property editors from plugins
// ---------------------------------------------------------------------------

/// How a property should be rendered in the inspector.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PropertyHint {
    /// Use the default editor for this property type.
    None,
    /// A numeric range: `min..=max` with optional step.
    Range { min: i64, max: i64, step: i64 },
    /// A drop-down of string options.
    Enum(Vec<String>),
    /// A file path selector (with optional extension filter like `"*.png"`).
    File(String),
    /// A multi-line text editor.
    MultilineText,
    /// A color picker without alpha channel.
    ColorNoAlpha,
    /// An exponential range slider.
    ExpRange { min: i64, max: i64, step: i64 },
    /// Bitfield flags editor with named bits.
    Flags(Vec<String>),
    /// Physics/render layer bitfield (up to 32 layers).
    Layers { layer_type: LayerType },
    /// A directory picker.
    Dir,
    /// A global file picker (not project-relative).
    GlobalFile(String),
    /// Placeholder text for empty inputs.
    PlaceholderText(String),
    /// Easing curve editor.
    ExpEasing,
    /// A node path with optional valid types filter.
    NodePathValidTypes(Vec<String>),
    /// Expected resource type for resource properties (e.g. `"Texture2D"`).
    ResourceType(String),
}

/// The type of layer bitfield.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LayerType {
    /// 2D physics layers.
    Physics2D,
    /// 2D render layers.
    Render2D,
    /// 3D physics layers.
    Physics3D,
    /// 3D render layers.
    Render3D,
}

/// A custom property editor descriptor returned by an inspector plugin.
///
/// Tells the inspector how to render and edit a specific property.
#[derive(Debug, Clone)]
pub struct CustomPropertyEditor {
    /// The property name this editor applies to.
    pub property_name: String,
    /// Optional display label (if different from the property name).
    pub label: Option<String>,
    /// A hint about how to render the editor widget.
    pub hint: PropertyHint,
    /// Whether this property should be read-only in the inspector.
    pub read_only: bool,
    /// Optional tooltip text.
    pub tooltip: Option<String>,
}

impl CustomPropertyEditor {
    /// Creates a new custom property editor with default settings.
    pub fn new(property_name: impl Into<String>) -> Self {
        Self {
            property_name: property_name.into(),
            label: None,
            hint: PropertyHint::None,
            read_only: false,
            tooltip: None,
        }
    }

    /// Sets the display label.
    pub fn with_label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Sets the property hint.
    pub fn with_hint(mut self, hint: PropertyHint) -> Self {
        self.hint = hint;
        self
    }

    /// Marks the property as read-only.
    pub fn read_only(mut self) -> Self {
        self.read_only = true;
        self
    }

    /// Sets a tooltip.
    pub fn with_tooltip(mut self, tooltip: impl Into<String>) -> Self {
        self.tooltip = Some(tooltip.into());
        self
    }
}

/// A trait for plugins that customize how properties are displayed in the
/// inspector.
///
/// Mirrors Godot's `EditorInspectorPlugin`. Plugins implementing this trait
/// can:
/// - Override how specific properties are rendered.
/// - Add extra sections before/after properties.
/// - Hide properties they want to manage differently.
///
/// Register inspector plugins with
/// [`InspectorPluginRegistry::register`].
pub trait EditorInspectorPlugin: Send {
    /// Returns a unique identifier for this inspector plugin.
    fn plugin_id(&self) -> &str;

    /// Returns true if this plugin can handle nodes of the given class.
    fn can_handle(&self, class_name: &str) -> bool;

    /// Returns custom property editors for the given node class and properties.
    ///
    /// Called when the inspector is about to display a node. The plugin can
    /// return editors for properties it wants to customize. Properties not
    /// returned here use the default editor.
    fn parse_property(
        &self,
        class_name: &str,
        property_name: &str,
        property_type: &Variant,
    ) -> Option<CustomPropertyEditor>;

    /// Returns property names that this plugin wants to hide from the
    /// default inspector.
    fn hidden_properties(&self, _class_name: &str) -> Vec<String> {
        Vec::new()
    }

    /// Returns extra sections to add at the top of the inspector for this
    /// class.
    fn add_custom_sections(&self, _class_name: &str) -> Vec<InspectorSection> {
        Vec::new()
    }
}

/// A section in the inspector, grouping related properties.
///
/// Used both by [`SectionedInspector`] (with category-based grouping) and
/// by [`EditorInspectorPlugin`] (with custom sections).
#[derive(Debug, Clone)]
pub struct InspectorSection {
    /// The section name / title.
    pub name: String,
    /// The property category this section represents.
    pub category: PropertyCategory,
    /// Whether this section is expanded (visible).
    expanded: bool,
    /// Property entries in this section.
    entries: Vec<PropertyEntry>,
    /// Custom property editors (for plugin sections).
    pub properties: Vec<CustomPropertyEditor>,
}

impl InspectorSection {
    /// Creates a new inspector section with a category.
    pub fn new(name: impl Into<String>, category: PropertyCategory) -> Self {
        Self {
            name: name.into(),
            category,
            expanded: true,
            entries: Vec::new(),
            properties: Vec::new(),
        }
    }

    /// Creates a new section for plugin use (backward-compatible).
    pub fn new_custom(title: impl Into<String>) -> Self {
        Self {
            name: title.into(),
            category: PropertyCategory::Misc,
            expanded: true,
            entries: Vec::new(),
            properties: Vec::new(),
        }
    }

    /// Adds a custom property editor to this section (plugin builder pattern).
    pub fn with_property(mut self, editor: CustomPropertyEditor) -> Self {
        self.properties.push(editor);
        self
    }

    /// Whether this section is expanded.
    pub fn is_expanded(&self) -> bool {
        self.expanded
    }

    /// Sets the expanded state.
    pub fn set_expanded(&mut self, expanded: bool) {
        self.expanded = expanded;
    }

    /// Toggles the expanded state.
    pub fn toggle(&mut self) {
        self.expanded = !self.expanded;
    }

    /// Whether this section has no entries.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Number of property entries in this section.
    pub fn property_count(&self) -> usize {
        self.entries.len()
    }

    /// Returns the entries in this section.
    pub fn entries(&self) -> &[PropertyEntry] {
        &self.entries
    }
}

/// Registry for [`EditorInspectorPlugin`] instances.
///
/// Manages inspector plugins and provides query methods used by the
/// inspector panel to resolve custom editors.
pub struct InspectorPluginRegistry {
    plugins: Vec<Box<dyn EditorInspectorPlugin>>,
}

impl std::fmt::Debug for InspectorPluginRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InspectorPluginRegistry")
            .field("plugin_count", &self.plugins.len())
            .finish()
    }
}

impl InspectorPluginRegistry {
    /// Creates a new empty registry.
    pub fn new() -> Self {
        Self {
            plugins: Vec::new(),
        }
    }

    /// Registers an inspector plugin.
    pub fn register(&mut self, plugin: Box<dyn EditorInspectorPlugin>) {
        self.plugins.push(plugin);
    }

    /// Unregisters an inspector plugin by its ID.
    pub fn unregister(&mut self, plugin_id: &str) {
        self.plugins.retain(|p| p.plugin_id() != plugin_id);
    }

    /// Returns the number of registered plugins.
    pub fn plugin_count(&self) -> usize {
        self.plugins.len()
    }

    /// Returns the IDs of all registered plugins.
    pub fn plugin_ids(&self) -> Vec<&str> {
        self.plugins.iter().map(|p| p.plugin_id()).collect()
    }

    /// Finds a custom property editor for the given class and property.
    ///
    /// Queries all registered plugins in order. The first plugin that
    /// returns `Some` wins.
    pub fn find_property_editor(
        &self,
        class_name: &str,
        property_name: &str,
        property_value: &Variant,
    ) -> Option<CustomPropertyEditor> {
        for plugin in &self.plugins {
            if plugin.can_handle(class_name) {
                if let Some(editor) =
                    plugin.parse_property(class_name, property_name, property_value)
                {
                    return Some(editor);
                }
            }
        }
        None
    }

    /// Returns properties that should be hidden for the given class.
    ///
    /// Aggregates hidden properties from all plugins that handle the class.
    pub fn hidden_properties(&self, class_name: &str) -> Vec<String> {
        let mut hidden = Vec::new();
        for plugin in &self.plugins {
            if plugin.can_handle(class_name) {
                hidden.extend(plugin.hidden_properties(class_name));
            }
        }
        hidden
    }

    /// Returns custom sections for the given class.
    ///
    /// Aggregates sections from all plugins that handle the class.
    pub fn custom_sections(&self, class_name: &str) -> Vec<InspectorSection> {
        let mut sections = Vec::new();
        for plugin in &self.plugins {
            if plugin.can_handle(class_name) {
                sections.extend(plugin.add_custom_sections(class_name));
            }
        }
        sections
    }
}

impl Default for InspectorPluginRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Variant type coercion — Godot-compatible conversion rules
// ---------------------------------------------------------------------------

/// Coerces a variant to the given target type, following Godot's implicit
/// conversion rules.
///
/// Returns `Some(converted)` if the coercion is supported, `None` otherwise.
/// Same-type coercion always succeeds (returns a clone).
///
/// Supported coercions:
/// - Bool <-> Int <-> Float (numeric promotion/truncation)
/// - Any type -> String (via Display)
/// - String <-> StringName <-> NodePath
/// - Same type -> identity clone
pub fn coerce_variant(value: &Variant, target: VariantType) -> Option<Variant> {
    use gdcore::node_path::NodePath;
    use gdcore::string_name::StringName;

    // Same type is always identity.
    if value.variant_type() == target {
        return Some(value.clone());
    }

    match (value, target) {
        // -- Bool -> numeric --
        (Variant::Bool(b), VariantType::Int) => Some(Variant::Int(if *b { 1 } else { 0 })),
        (Variant::Bool(b), VariantType::Float) => Some(Variant::Float(if *b { 1.0 } else { 0.0 })),

        // -- Int -> other numeric / bool --
        (Variant::Int(i), VariantType::Float) => Some(Variant::Float(*i as f64)),
        (Variant::Int(i), VariantType::Bool) => Some(Variant::Bool(*i != 0)),

        // -- Float -> other numeric / bool --
        (Variant::Float(f), VariantType::Int) => Some(Variant::Int(*f as i64)),
        (Variant::Float(f), VariantType::Bool) => Some(Variant::Bool(*f != 0.0)),

        // -- String <-> StringName (specific cases before catch-all) --
        (Variant::String(s), VariantType::StringName) => {
            Some(Variant::StringName(StringName::new(s)))
        }
        (Variant::StringName(sn), VariantType::String) => {
            Some(Variant::String(sn.as_str().to_owned()))
        }

        // -- String <-> NodePath --
        (Variant::String(s), VariantType::NodePath) => Some(Variant::NodePath(NodePath::new(s))),
        (Variant::NodePath(np), VariantType::String) => Some(Variant::String(np.to_string())),

        // -- StringName <-> NodePath --
        (Variant::StringName(sn), VariantType::NodePath) => {
            Some(Variant::NodePath(NodePath::new(sn.as_str())))
        }
        (Variant::NodePath(np), VariantType::StringName) => {
            Some(Variant::StringName(StringName::new(&np.to_string())))
        }

        // -- Any -> String (Godot str() semantics, catch-all after specific cases) --
        (_, VariantType::String) => Some(Variant::String(value.to_string())),

        // All other coercions are unsupported.
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// validate_variant -- type checking for property assignment
// ---------------------------------------------------------------------------

/// Validates that a variant's type matches the expected type.
///
/// Returns `Ok(())` if the types match, or an error message if not.
pub fn validate_variant(value: &Variant, expected: VariantType) -> Result<(), String> {
    if value.variant_type() == expected {
        Ok(())
    } else {
        Err(format!(
            "expected {:?}, got {:?}",
            expected,
            value.variant_type()
        ))
    }
}

// ---------------------------------------------------------------------------
// EditorHint -- refinement hints for PropertyEditor widgets
// ---------------------------------------------------------------------------

/// A hint that refines how a [`PropertyEditor`] behaves.
#[derive(Debug, Clone, PartialEq)]
pub enum EditorHint {
    /// No hint -- use defaults.
    None,
    /// Numeric range constraint.
    Range { min: f64, max: f64, step: f64 },
    /// Exponential range (logarithmic slider).
    ExpRange { min: f64, max: f64, step: f64 },
    /// Fixed list of options (becomes an enum dropdown).
    Enum(Vec<String>),
    /// Promote to multiline text editor.
    MultilineText,
    /// File picker with extension filters.
    File(Vec<String>),
    /// Resource type constraint (e.g. `"Texture2D"`).
    ResourceType(String),
    /// Bitfield flags with named bits.
    Flags(Vec<String>),
    /// Directory picker.
    Dir,
    /// Placeholder text for empty inputs.
    PlaceholderText(String),
    /// Easing curve editor.
    ExpEasing,
    /// Physics/render layer bitfield.
    Layers(LayerType),
}

// ---------------------------------------------------------------------------
// PropertyEditor -- typed editor widgets for Variant types
// ---------------------------------------------------------------------------

/// The type of editor widget to use for a property, based on its variant type.
#[derive(Debug, Clone, PartialEq)]
pub enum PropertyEditor {
    /// No editor (read-only nil/unknown).
    None,
    /// Boolean checkbox.
    CheckBox,
    /// Integer spin box.
    SpinBoxInt {
        min: Option<i64>,
        max: Option<i64>,
        step: i64,
    },
    /// Float spin box.
    SpinBoxFloat {
        min: Option<f64>,
        max: Option<f64>,
        step: f64,
    },
    /// Single-line text input.
    LineEdit,
    /// Multi-line text editor.
    TextEdit,
    /// Vector2 editor (two floats).
    Vector2,
    /// Vector3 editor (three floats).
    Vector3,
    /// Rect2 editor.
    Rect2,
    /// Transform2D editor.
    Transform2D,
    /// Color picker.
    ColorPicker,
    /// Basis editor.
    Basis,
    /// Transform3D editor.
    Transform3D,
    /// Quaternion editor.
    Quaternion,
    /// AABB editor.
    Aabb,
    /// Plane editor.
    Plane,
    /// Object ID editor (read-only).
    ObjectId,
    /// Array editor.
    Array,
    /// Dictionary editor.
    Dictionary,
    /// Callable reference (read-only).
    Callable,
    /// Resource picker.
    ResourcePicker,
    /// Node path editor.
    NodePath,
    /// File picker with filters.
    FilePicker { filters: Vec<String> },
    /// Enum dropdown.
    EnumSelect { options: Vec<String> },
    /// Bitfield flags editor with named bits.
    FlagsEditor { flags: Vec<String> },
    /// Physics/render layer bitfield editor.
    LayerEditor { layer_type: LayerType },
    /// Easing curve editor.
    EasingEditor,
    /// Directory picker.
    DirPicker,
}

impl PropertyEditor {
    /// Returns the appropriate editor for a given variant type.
    pub fn for_variant_type(vtype: VariantType) -> Self {
        match vtype {
            VariantType::Nil => Self::None,
            VariantType::Bool => Self::CheckBox,
            VariantType::Int => Self::SpinBoxInt {
                min: Option::None,
                max: Option::None,
                step: 1,
            },
            VariantType::Float => Self::SpinBoxFloat {
                min: Option::None,
                max: Option::None,
                step: 0.001,
            },
            VariantType::String => Self::LineEdit,
            VariantType::StringName => Self::LineEdit,
            VariantType::NodePath => Self::NodePath,
            VariantType::Vector2 => Self::Vector2,
            VariantType::Vector3 => Self::Vector3,
            VariantType::Rect2 => Self::Rect2,
            VariantType::Transform2D => Self::Transform2D,
            VariantType::Color => Self::ColorPicker,
            VariantType::Basis => Self::Basis,
            VariantType::Transform3D => Self::Transform3D,
            VariantType::Quaternion => Self::Quaternion,
            VariantType::Aabb => Self::Aabb,
            VariantType::Plane => Self::Plane,
            VariantType::ObjectId => Self::ObjectId,
            VariantType::Array => Self::Array,
            VariantType::Dictionary => Self::Dictionary,
            VariantType::Callable => Self::Callable,
            VariantType::Resource => Self::ResourcePicker,
        }
    }

    /// Returns a human-readable display name for this editor widget.
    pub fn display_name(&self) -> &str {
        match self {
            Self::None => "None",
            Self::CheckBox => "CheckBox",
            Self::SpinBoxInt { .. } => "SpinBox (Int)",
            Self::SpinBoxFloat { .. } => "SpinBox (Float)",
            Self::LineEdit => "LineEdit",
            Self::TextEdit => "TextEdit",
            Self::Vector2 => "Vector2",
            Self::Vector3 => "Vector3",
            Self::Rect2 => "Rect2",
            Self::Transform2D => "Transform2D",
            Self::ColorPicker => "ColorPicker",
            Self::Basis => "Basis",
            Self::Transform3D => "Transform3D",
            Self::Quaternion => "Quaternion",
            Self::Aabb => "AABB",
            Self::Plane => "Plane",
            Self::ObjectId => "ObjectId",
            Self::Array => "Array",
            Self::Dictionary => "Dictionary",
            Self::Callable => "Callable",
            Self::ResourcePicker => "Resource",
            Self::NodePath => "NodePath",
            Self::FilePicker { .. } => "FilePicker",
            Self::EnumSelect { .. } => "Enum",
            Self::FlagsEditor { .. } => "Flags",
            Self::LayerEditor { .. } => "Layers",
            Self::EasingEditor => "Easing",
            Self::DirPicker => "DirPicker",
        }
    }

    /// Whether this editor is read-only.
    pub fn is_read_only(&self) -> bool {
        matches!(self, Self::None | Self::Callable)
    }

    /// Applies a hint to refine this editor.
    pub fn with_hint(&self, hint: &EditorHint) -> Self {
        match hint {
            EditorHint::None => self.clone(),
            EditorHint::Range { min, max, step } => match self {
                Self::SpinBoxInt { .. } => Self::SpinBoxInt {
                    min: Some(*min as i64),
                    max: Some(*max as i64),
                    step: (*step as i64).max(1),
                },
                Self::SpinBoxFloat { .. } => Self::SpinBoxFloat {
                    min: Some(*min),
                    max: Some(*max),
                    step: *step,
                },
                other => other.clone(),
            },
            EditorHint::Enum(options) => Self::EnumSelect {
                options: options.clone(),
            },
            EditorHint::MultilineText => match self {
                Self::LineEdit => Self::TextEdit,
                other => other.clone(),
            },
            EditorHint::File(filters) => match self {
                Self::LineEdit => Self::FilePicker {
                    filters: filters.clone(),
                },
                other => other.clone(),
            },
            EditorHint::ResourceType(_) => self.clone(),
            EditorHint::ExpRange { min, max, step } => match self {
                Self::SpinBoxInt { .. } => Self::SpinBoxInt {
                    min: Some(*min as i64),
                    max: Some(*max as i64),
                    step: (*step as i64).max(1),
                },
                Self::SpinBoxFloat { .. } => Self::SpinBoxFloat {
                    min: Some(*min),
                    max: Some(*max),
                    step: *step,
                },
                other => other.clone(),
            },
            EditorHint::Flags(flags) => Self::FlagsEditor {
                flags: flags.clone(),
            },
            EditorHint::Dir => match self {
                Self::LineEdit => Self::DirPicker,
                other => other.clone(),
            },
            EditorHint::PlaceholderText(_) => self.clone(),
            EditorHint::ExpEasing => Self::EasingEditor,
            EditorHint::Layers(layer_type) => Self::LayerEditor {
                layer_type: layer_type.clone(),
            },
        }
    }
}

// ---------------------------------------------------------------------------
// PropertyEntryWithEditor -- a property entry paired with its editor type
// ---------------------------------------------------------------------------

/// A property entry paired with its appropriate editor widget.
#[derive(Debug, Clone)]
pub struct PropertyEntryWithEditor {
    /// The property name.
    pub name: String,
    /// The current value.
    pub value: Variant,
    /// The editor widget type.
    pub editor: PropertyEditor,
}

// ---------------------------------------------------------------------------
// ResourceSubEditor -- inline resource editing with clone-on-write
// ---------------------------------------------------------------------------

/// A change record for resource property modifications.
#[derive(Debug, Clone)]
pub struct PropertyChange {
    /// The property that was changed.
    pub property: String,
    /// The value before the change.
    pub old_value: Variant,
    /// The value after the change.
    pub new_value: Variant,
}

/// Inline editor for resources, supporting clone-on-write semantics,
/// change logging, and undo.
pub struct ResourceSubEditor {
    resource: std::sync::Arc<gdresource::Resource>,
    expanded: bool,
    change_log: Vec<PropertyChange>,
}

impl ResourceSubEditor {
    /// Creates a new sub-editor wrapping the given resource.
    pub fn new(resource: std::sync::Arc<gdresource::Resource>) -> Self {
        Self {
            resource,
            expanded: false,
            change_log: Vec::new(),
        }
    }

    /// Returns the class name of the resource.
    pub fn class_name(&self) -> &str {
        &self.resource.class_name
    }

    /// Returns the resource path.
    pub fn resource_path(&self) -> &str {
        &self.resource.path
    }

    /// Whether the sub-editor is expanded.
    pub fn is_expanded(&self) -> bool {
        self.expanded
    }

    /// Toggles the expanded state.
    pub fn toggle(&mut self) {
        self.expanded = !self.expanded;
    }

    /// Sets the expanded state.
    pub fn set_expanded(&mut self, expanded: bool) {
        self.expanded = expanded;
    }

    /// Number of properties in the resource.
    pub fn property_count(&self) -> usize {
        self.resource.property_count()
    }

    /// Lists properties sorted alphabetically with their editor types.
    pub fn list_properties(&self) -> Vec<PropertyEntryWithEditor> {
        let mut entries: Vec<PropertyEntryWithEditor> = self
            .resource
            .properties()
            .map(|(name, value)| PropertyEntryWithEditor {
                name: name.clone(),
                value: value.clone(),
                editor: PropertyEditor::for_variant_type(value.variant_type()),
            })
            .collect();
        entries.sort_by(|a, b| a.name.cmp(&b.name));
        entries
    }

    /// Gets a property value by name.
    pub fn get_property(&self, name: &str) -> Option<&Variant> {
        self.resource.get_property(name)
    }

    /// Sets a property, recording the change and returning the new resource.
    ///
    /// Uses clone-on-write: the internal resource is cloned, modified, and
    /// stored. The original Arc is not mutated.
    pub fn set_property(
        &mut self,
        name: &str,
        value: Variant,
    ) -> std::sync::Arc<gdresource::Resource> {
        let old_value = self
            .resource
            .get_property(name)
            .cloned()
            .unwrap_or(Variant::Nil);
        self.change_log.push(PropertyChange {
            property: name.to_string(),
            old_value,
            new_value: value.clone(),
        });
        let mut new_res = (*self.resource).clone();
        new_res.set_property(name, value);
        let arc = std::sync::Arc::new(new_res);
        self.resource = arc.clone();
        arc
    }

    /// Returns the number of changes recorded.
    pub fn change_count(&self) -> usize {
        self.change_log.len()
    }

    /// Returns the change log.
    pub fn change_log(&self) -> &[PropertyChange] {
        &self.change_log
    }

    /// Undoes the last change, restoring the property to its previous value.
    ///
    /// Returns the name of the undone property, or `None` if no changes.
    pub fn undo_last(&mut self) -> Option<String> {
        let change = self.change_log.pop()?;
        let mut new_res = (*self.resource).clone();
        new_res.set_property(&change.property, change.old_value);
        self.resource = std::sync::Arc::new(new_res);
        Some(change.property)
    }

    /// Replaces the resource entirely, clearing the change log.
    pub fn replace_resource(&mut self, resource: std::sync::Arc<gdresource::Resource>) {
        self.resource = resource;
        self.change_log.clear();
    }

    /// Returns the IDs of sub-resources.
    pub fn subresource_ids(&self) -> Vec<String> {
        self.resource.subresources.keys().cloned().collect()
    }

    /// Opens a sub-resource by its ID, returning a new sub-editor for it.
    pub fn open_subresource(&self, id: &str) -> Option<ResourceSubEditor> {
        self.resource
            .subresources
            .get(id)
            .map(|sub| ResourceSubEditor::new(sub.clone()))
    }
}

// ---------------------------------------------------------------------------
// SectionedInspector -- groups properties by category with expand/collapse
// ---------------------------------------------------------------------------

/// Groups property entries into collapsible sections by category.
pub struct SectionedInspector {
    sections: Vec<InspectorSection>,
}

impl SectionedInspector {
    /// Creates a sectioned inspector from a list of property entries.
    ///
    /// Sections are created in the order categories first appear.
    pub fn from_entries(entries: Vec<PropertyEntry>) -> Self {
        let mut section_order: Vec<PropertyCategory> = Vec::new();
        let mut section_map: HashMap<PropertyCategory, Vec<PropertyEntry>> = HashMap::new();

        for entry in entries {
            if !section_map.contains_key(&entry.category) {
                section_order.push(entry.category.clone());
            }
            section_map
                .entry(entry.category.clone())
                .or_default()
                .push(entry);
        }

        let sections = section_order
            .into_iter()
            .map(|cat| {
                let name = match &cat {
                    PropertyCategory::Transform => "Transform",
                    PropertyCategory::Rendering => "Rendering",
                    PropertyCategory::Physics => "Physics",
                    PropertyCategory::Script => "Script",
                    PropertyCategory::Misc => "Misc",
                };
                let entries = section_map.remove(&cat).unwrap_or_default();
                let mut section = InspectorSection::new(name, cat);
                section.entries = entries;
                section
            })
            .collect();

        Self { sections }
    }

    /// Returns the number of sections.
    pub fn section_count(&self) -> usize {
        self.sections.len()
    }

    /// Returns the total number of properties across all sections.
    pub fn total_property_count(&self) -> usize {
        self.sections.iter().map(|s| s.property_count()).sum()
    }

    /// Returns the number of properties in expanded sections.
    pub fn visible_property_count(&self) -> usize {
        self.sections
            .iter()
            .filter(|s| s.is_expanded())
            .map(|s| s.property_count())
            .sum()
    }

    /// Collapses all sections.
    pub fn collapse_all(&mut self) {
        for section in &mut self.sections {
            section.set_expanded(false);
        }
    }

    /// Expands all sections.
    pub fn expand_all(&mut self) {
        for section in &mut self.sections {
            section.set_expanded(true);
        }
    }

    /// Returns a reference to the sections.
    pub fn sections(&self) -> &[InspectorSection] {
        &self.sections
    }

    /// Finds a section by category (immutable).
    pub fn section_by_category(&self, category: &PropertyCategory) -> Option<&InspectorSection> {
        self.sections.iter().find(|s| s.category == *category)
    }

    /// Finds a section by category (mutable).
    pub fn section_by_category_mut(
        &mut self,
        category: &PropertyCategory,
    ) -> Option<&mut InspectorSection> {
        self.sections.iter_mut().find(|s| s.category == *category)
    }
}

// ---------------------------------------------------------------------------
// InspectorPanel extensions -- typed editors, validation, sectioned view
// ---------------------------------------------------------------------------

impl InspectorPanel {
    /// Returns the appropriate [`PropertyEditor`] for a named property.
    pub fn editor_for_property(&self, tree: &SceneTree, name: &str) -> PropertyEditor {
        let value = self.get_property(tree, name);
        PropertyEditor::for_variant_type(value.variant_type())
    }

    /// Returns `(name, editor)` pairs for all properties on the inspected node.
    pub fn all_editors(&self, tree: &SceneTree) -> Vec<(String, PropertyEditor)> {
        self.list_properties(tree)
            .into_iter()
            .map(|entry| {
                let editor = PropertyEditor::for_variant_type(entry.value.variant_type());
                (entry.name, editor)
            })
            .collect()
    }

    /// Sets a property with type validation and optional coercion.
    ///
    /// If the value matches the existing type, it is set directly. If not,
    /// coercion is attempted. Nil-typed properties accept any value.
    pub fn set_property_validated(
        &self,
        tree: &mut SceneTree,
        name: &str,
        value: Variant,
    ) -> Result<Variant, String> {
        let current = self.get_property(tree, name);
        let current_type = current.variant_type();

        // Nil accepts anything.
        if current_type == VariantType::Nil {
            return Ok(self.set_property(tree, name, value));
        }

        // Same type -- direct set.
        if value.variant_type() == current_type {
            return Ok(self.set_property(tree, name, value));
        }

        // Try coercion.
        if let Some(coerced) = coerce_variant(&value, current_type) {
            return Ok(self.set_property(tree, name, coerced));
        }

        Err(format!(
            "expected {:?}, got {:?}",
            current_type,
            value.variant_type()
        ))
    }

    /// Creates a [`SectionedInspector`] view of the inspected node's properties.
    pub fn sectioned_view(&self, tree: &SceneTree) -> SectionedInspector {
        SectionedInspector::from_entries(self.list_properties(tree))
    }

    /// Reverts a property to its default value.
    ///
    /// Returns the old value, or `Variant::Nil` if no node is inspected or the
    /// property has no registered default.
    pub fn revert_to_default(
        &self,
        tree: &mut SceneTree,
        name: &str,
        defaults: &PropertyDefaults,
    ) -> Variant {
        if let Some(default) = defaults.get(name) {
            self.set_property(tree, name, default.clone())
        } else {
            Variant::Nil
        }
    }

    /// Returns whether a property has been modified from its default value.
    pub fn is_property_modified(
        &self,
        tree: &SceneTree,
        name: &str,
        defaults: &PropertyDefaults,
    ) -> bool {
        let current = self.get_property(tree, name);
        match defaults.get(name) {
            Some(default) => current != *default,
            None => false,
        }
    }

    /// Returns the node path for the currently inspected node in the scene tree.
    ///
    /// This produces a path like `/root/Player` that can be used as a property
    /// path prefix.
    pub fn copy_node_path(&self, tree: &SceneTree) -> Option<String> {
        let node_id = self.inspected_node?;
        tree.node_path(node_id)
    }

    /// Returns a property path string like `/root/Player:position` suitable for
    /// clipboard copy.
    pub fn copy_property_path(&self, tree: &SceneTree, property: &str) -> Option<String> {
        let node_path = self.copy_node_path(tree)?;
        Some(format!("{}:{}", node_path, property))
    }
}

// ---------------------------------------------------------------------------
// PropertyDefaults -- stores default values for revert-to-default
// ---------------------------------------------------------------------------

/// Stores per-class default property values used by revert-to-default.
///
/// When a node is created, its class defaults are registered here so the
/// inspector can detect modifications and offer a "revert" action.
#[derive(Debug, Clone, Default)]
pub struct PropertyDefaults {
    /// Default values keyed by property name.
    defaults: HashMap<String, Variant>,
}

impl PropertyDefaults {
    /// Creates a new empty default set.
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a default value for a property.
    pub fn set(&mut self, name: impl Into<String>, value: Variant) {
        self.defaults.insert(name.into(), value);
    }

    /// Returns the default value for a property, if registered.
    pub fn get(&self, name: &str) -> Option<&Variant> {
        self.defaults.get(name)
    }

    /// Returns all registered default names and values.
    pub fn iter(&self) -> impl Iterator<Item = (&String, &Variant)> {
        self.defaults.iter()
    }

    /// Number of registered defaults.
    pub fn len(&self) -> usize {
        self.defaults.len()
    }

    /// Whether there are no registered defaults.
    pub fn is_empty(&self) -> bool {
        self.defaults.is_empty()
    }

    /// Creates defaults for common Node2D properties.
    pub fn node2d_defaults() -> Self {
        let mut d = Self::new();
        d.set("position", Variant::Vector2(gdcore::math::Vector2::ZERO));
        d.set("rotation", Variant::Float(0.0));
        d.set("scale", Variant::Vector2(gdcore::math::Vector2::ONE));
        d.set("visible", Variant::Bool(true));
        d.set("z_index", Variant::Int(0));
        d.set("modulate", Variant::Color(gdcore::math::Color::WHITE));
        d
    }

    /// Creates defaults for common Node3D properties.
    pub fn node3d_defaults() -> Self {
        let mut d = Self::new();
        d.set("position", Variant::Vector3(gdcore::math::Vector3::ZERO));
        d.set("rotation", Variant::Vector3(gdcore::math::Vector3::ZERO));
        d.set("scale", Variant::Vector3(gdcore::math::Vector3::ONE));
        d.set("visible", Variant::Bool(true));
        d
    }
}

// ---------------------------------------------------------------------------
// DragAdjust -- numeric drag-to-adjust for spin boxes
// ---------------------------------------------------------------------------

/// Tracks state for drag-to-adjust on numeric inspector properties.
///
/// When the user clicks and drags on a numeric property label, the value
/// changes proportionally to the drag distance. This mirrors Godot's
/// inspector drag behavior.
#[derive(Debug, Clone)]
pub struct DragAdjust {
    /// The property being adjusted.
    pub property: String,
    /// The value when the drag started.
    pub start_value: f64,
    /// Accumulated pixel drag delta.
    pub pixel_delta: f64,
    /// How many units per pixel of drag.
    pub sensitivity: f64,
    /// Whether the drag is currently active.
    pub active: bool,
    /// Optional minimum clamp.
    pub min: Option<f64>,
    /// Optional maximum clamp.
    pub max: Option<f64>,
}

impl DragAdjust {
    /// Begins a drag-to-adjust operation on a float property.
    pub fn begin_float(property: impl Into<String>, current: f64) -> Self {
        Self {
            property: property.into(),
            start_value: current,
            pixel_delta: 0.0,
            sensitivity: 0.01,
            active: true,
            min: None,
            max: None,
        }
    }

    /// Begins a drag-to-adjust operation on an integer property.
    pub fn begin_int(property: impl Into<String>, current: i64) -> Self {
        Self {
            property: property.into(),
            start_value: current as f64,
            pixel_delta: 0.0,
            sensitivity: 0.1,
            active: true,
            min: None,
            max: None,
        }
    }

    /// Sets the sensitivity (units per pixel).
    pub fn with_sensitivity(mut self, sensitivity: f64) -> Self {
        self.sensitivity = sensitivity;
        self
    }

    /// Sets the min/max clamp values.
    pub fn with_range(mut self, min: f64, max: f64) -> Self {
        self.min = Some(min);
        self.max = Some(max);
        self
    }

    /// Updates the drag with a new pixel delta (accumulated).
    pub fn update(&mut self, total_pixel_delta: f64) {
        self.pixel_delta = total_pixel_delta;
    }

    /// Returns the current adjusted value (float).
    pub fn current_value(&self) -> f64 {
        let raw = self.start_value + self.pixel_delta * self.sensitivity;
        match (self.min, self.max) {
            (Some(lo), Some(hi)) => raw.clamp(lo, hi),
            (Some(lo), None) => raw.max(lo),
            (None, Some(hi)) => raw.min(hi),
            (None, None) => raw,
        }
    }

    /// Returns the current adjusted value as an integer.
    pub fn current_value_int(&self) -> i64 {
        self.current_value().round() as i64
    }

    /// Returns the current value as a Variant (Float).
    pub fn to_variant_float(&self) -> Variant {
        Variant::Float(self.current_value())
    }

    /// Returns the current value as a Variant (Int).
    pub fn to_variant_int(&self) -> Variant {
        Variant::Int(self.current_value_int())
    }

    /// Ends the drag and returns the final value.
    pub fn end(&mut self) -> f64 {
        self.active = false;
        self.current_value()
    }
}

// ---------------------------------------------------------------------------
// LinkedValues -- uniform editing for vector components
// ---------------------------------------------------------------------------

/// Tracks whether vector components should be edited in lock-step.
///
/// When linked, changing one component of a Vector2/3 (or scale) will
/// set all components to the same value. This mirrors the "chain link"
/// icon in Godot's inspector for scale properties.
#[derive(Debug, Clone)]
pub struct LinkedValues {
    /// Properties that have linked editing enabled.
    linked: HashSet<String>,
}

impl LinkedValues {
    /// Creates a new instance with no linked properties.
    pub fn new() -> Self {
        Self {
            linked: HashSet::new(),
        }
    }

    /// Enables linked editing for a property.
    pub fn link(&mut self, property: impl Into<String>) {
        self.linked.insert(property.into());
    }

    /// Disables linked editing for a property.
    pub fn unlink(&mut self, property: &str) {
        self.linked.remove(property);
    }

    /// Toggles linked editing for a property. Returns `true` if now linked.
    pub fn toggle(&mut self, property: &str) -> bool {
        if self.linked.contains(property) {
            self.linked.remove(property);
            false
        } else {
            self.linked.insert(property.to_string());
            true
        }
    }

    /// Returns whether a property has linked editing enabled.
    pub fn is_linked(&self, property: &str) -> bool {
        self.linked.contains(property)
    }

    /// Applies linked-value logic to a Vector2 when one component changes.
    ///
    /// If linked, returns a Vector2 with all components set to the changed value.
    /// If not linked, returns the value unchanged.
    pub fn apply_vec2(
        &self,
        property: &str,
        value: gdcore::math::Vector2,
        changed_component: usize,
    ) -> gdcore::math::Vector2 {
        if !self.is_linked(property) {
            return value;
        }
        let uniform = match changed_component {
            0 => value.x,
            _ => value.y,
        };
        gdcore::math::Vector2::new(uniform, uniform)
    }

    /// Applies linked-value logic to a Vector3 when one component changes.
    ///
    /// If linked, returns a Vector3 with all components set to the changed value.
    /// If not linked, returns the value unchanged.
    pub fn apply_vec3(
        &self,
        property: &str,
        value: gdcore::math::Vector3,
        changed_component: usize,
    ) -> gdcore::math::Vector3 {
        if !self.is_linked(property) {
            return value;
        }
        let uniform = match changed_component {
            0 => value.x,
            1 => value.y,
            _ => value.z,
        };
        gdcore::math::Vector3::new(uniform, uniform, uniform)
    }
}

impl Default for LinkedValues {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Inspector history (back/forward navigation)
// ---------------------------------------------------------------------------

/// An entry in the inspector navigation history.
#[derive(Debug, Clone, PartialEq)]
pub struct InspectorHistoryEntry {
    /// The node being inspected.
    pub node_id: NodeId,
    /// Optional sub-resource path for drill-down (e.g. "material", "material/albedo_texture").
    pub subresource_path: Option<String>,
}

impl InspectorHistoryEntry {
    /// Creates a history entry for a node.
    pub fn node(node_id: NodeId) -> Self {
        Self {
            node_id,
            subresource_path: None,
        }
    }

    /// Creates a history entry for a sub-resource of a node.
    pub fn subresource(node_id: NodeId, path: impl Into<String>) -> Self {
        Self {
            node_id,
            subresource_path: Some(path.into()),
        }
    }
}

/// Browser-like back/forward navigation history for the inspector.
///
/// Mirrors Godot's inspector history where clicking on sub-resources or
/// switching inspected nodes pushes to the history stack, and back/forward
/// buttons navigate through it.
#[derive(Debug, Clone)]
pub struct InspectorHistory {
    /// The navigation stack.
    entries: Vec<InspectorHistoryEntry>,
    /// Current position in the stack (index into `entries`).
    cursor: usize,
    /// Maximum history depth.
    max_depth: usize,
}

impl Default for InspectorHistory {
    fn default() -> Self {
        Self::new()
    }
}

impl InspectorHistory {
    /// Creates a new empty history.
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            cursor: 0,
            max_depth: 64,
        }
    }

    /// Pushes a new entry, truncating any forward history.
    pub fn push(&mut self, entry: InspectorHistoryEntry) {
        // If we're not at the end, truncate forward history.
        if self.cursor < self.entries.len() {
            self.entries.truncate(self.cursor);
        }
        self.entries.push(entry);
        self.cursor = self.entries.len();

        // Trim oldest if we exceed max depth.
        if self.entries.len() > self.max_depth {
            let excess = self.entries.len() - self.max_depth;
            self.entries.drain(0..excess);
            self.cursor = self.entries.len();
        }
    }

    /// Navigates back. Returns the previous entry, or None if at the start.
    pub fn back(&mut self) -> Option<&InspectorHistoryEntry> {
        if self.cursor > 1 {
            self.cursor -= 1;
            Some(&self.entries[self.cursor - 1])
        } else {
            None
        }
    }

    /// Navigates forward. Returns the next entry, or None if at the end.
    pub fn forward(&mut self) -> Option<&InspectorHistoryEntry> {
        if self.cursor < self.entries.len() {
            self.cursor += 1;
            Some(&self.entries[self.cursor - 1])
        } else {
            None
        }
    }

    /// Returns the current entry, or None if history is empty.
    pub fn current(&self) -> Option<&InspectorHistoryEntry> {
        if self.cursor > 0 && self.cursor <= self.entries.len() {
            Some(&self.entries[self.cursor - 1])
        } else {
            None
        }
    }

    /// Whether back navigation is possible.
    pub fn can_go_back(&self) -> bool {
        self.cursor > 1
    }

    /// Whether forward navigation is possible.
    pub fn can_go_forward(&self) -> bool {
        self.cursor < self.entries.len()
    }

    /// Total number of entries in the history.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the history is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Clears all history.
    pub fn clear(&mut self) {
        self.entries.clear();
        self.cursor = 0;
    }
}

// ---------------------------------------------------------------------------
// Inspector toolbar / header
// ---------------------------------------------------------------------------

/// Breadcrumb segment for sub-resource navigation.
#[derive(Debug, Clone, PartialEq)]
pub struct BreadcrumbSegment {
    /// Display label (e.g. "Node2D", "SpatialMaterial", "albedo_texture").
    pub label: String,
    /// The history entry this breadcrumb points to.
    pub entry: InspectorHistoryEntry,
}

/// The inspector toolbar state, representing the header bar above the
/// property list in Godot's inspector.
///
/// Contains:
/// - Object type icon/label
/// - Back/forward navigation buttons
/// - Breadcrumb trail for sub-resource drill-down
/// - Object menu (copy, paste, make unique, etc.)
#[derive(Debug, Clone)]
pub struct InspectorToolbar {
    /// The class name of the currently inspected object (e.g. "Node2D").
    pub object_class: String,
    /// The node name (e.g. "Player").
    pub object_name: String,
    /// Breadcrumb path for sub-resource navigation.
    pub breadcrumbs: Vec<BreadcrumbSegment>,
    /// Navigation history.
    pub history: InspectorHistory,
}

impl Default for InspectorToolbar {
    fn default() -> Self {
        Self::new()
    }
}

impl InspectorToolbar {
    /// Creates a new empty toolbar.
    pub fn new() -> Self {
        Self {
            object_class: String::new(),
            object_name: String::new(),
            breadcrumbs: Vec::new(),
            history: InspectorHistory::new(),
        }
    }

    /// Inspects a node, pushing it to history and updating the header.
    pub fn inspect_node(&mut self, node_id: NodeId, name: &str, class_name: &str) {
        self.object_class = class_name.to_string();
        self.object_name = name.to_string();
        self.breadcrumbs.clear();
        self.breadcrumbs.push(BreadcrumbSegment {
            label: format!("{} ({})", name, class_name),
            entry: InspectorHistoryEntry::node(node_id),
        });
        self.history.push(InspectorHistoryEntry::node(node_id));
    }

    /// Drills into a sub-resource, appending a breadcrumb segment.
    pub fn drill_into_subresource(
        &mut self,
        node_id: NodeId,
        property_path: &str,
        display_label: &str,
    ) {
        let entry = InspectorHistoryEntry::subresource(node_id, property_path);
        self.breadcrumbs.push(BreadcrumbSegment {
            label: display_label.to_string(),
            entry: entry.clone(),
        });
        self.history.push(entry);
    }

    /// Navigates back to the previous breadcrumb level.
    pub fn navigate_back(&mut self) -> Option<InspectorHistoryEntry> {
        self.history.back().cloned().map(|entry| {
            // Trim breadcrumbs to match the back destination.
            if let Some(pos) = self.breadcrumbs.iter().position(|b| b.entry == entry) {
                self.breadcrumbs.truncate(pos + 1);
            }
            entry
        })
    }

    /// Navigates forward in history.
    pub fn navigate_forward(&mut self) -> Option<InspectorHistoryEntry> {
        self.history.forward().cloned()
    }

    /// Navigates to a specific breadcrumb by index.
    pub fn navigate_to_breadcrumb(&mut self, index: usize) -> Option<InspectorHistoryEntry> {
        if index < self.breadcrumbs.len() {
            let entry = self.breadcrumbs[index].entry.clone();
            self.breadcrumbs.truncate(index + 1);
            self.history.push(entry.clone());
            Some(entry)
        } else {
            None
        }
    }

    /// Returns the current breadcrumb depth.
    pub fn breadcrumb_depth(&self) -> usize {
        self.breadcrumbs.len()
    }

    /// Whether the toolbar can navigate back.
    pub fn can_go_back(&self) -> bool {
        self.history.can_go_back()
    }

    /// Whether the toolbar can navigate forward.
    pub fn can_go_forward(&self) -> bool {
        self.history.can_go_forward()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gdscene::node::Node;
    use std::cell::Cell;
    use std::rc::Rc;

    fn make_tree_with_node() -> (SceneTree, NodeId) {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let mut node = Node::new("Player", "Node2D");
        node.set_property("position", Variant::Int(0));
        node.set_property("velocity", Variant::Float(1.5));
        node.set_property("visible", Variant::Bool(true));
        let id = tree.add_child(root, node).unwrap();
        (tree, id)
    }

    #[test]
    fn inspect_and_list_properties() {
        let (tree, node_id) = make_tree_with_node();
        let mut panel = InspectorPanel::new();
        panel.inspect(node_id);

        let props = panel.list_properties(&tree);
        assert_eq!(props.len(), 3);
        // Sorted by name
        assert_eq!(props[0].name, "position");
        assert_eq!(props[1].name, "velocity");
        assert_eq!(props[2].name, "visible");
    }

    #[test]
    fn list_empty_when_no_node_inspected() {
        let tree = SceneTree::new();
        let panel = InspectorPanel::new();
        assert!(panel.list_properties(&tree).is_empty());
    }

    #[test]
    fn get_and_set_property() {
        let (mut tree, node_id) = make_tree_with_node();
        let mut panel = InspectorPanel::new();
        panel.inspect(node_id);

        assert_eq!(panel.get_property(&tree, "position"), Variant::Int(0));
        let old = panel.set_property(&mut tree, "position", Variant::Int(42));
        assert_eq!(old, Variant::Int(0));
        assert_eq!(panel.get_property(&tree, "position"), Variant::Int(42));
    }

    #[test]
    fn property_categories() {
        assert_eq!(
            PropertyCategory::categorize("position"),
            PropertyCategory::Transform
        );
        assert_eq!(
            PropertyCategory::categorize("visible"),
            PropertyCategory::Rendering
        );
        assert_eq!(
            PropertyCategory::categorize("velocity"),
            PropertyCategory::Physics
        );
        assert_eq!(
            PropertyCategory::categorize("script_var"),
            PropertyCategory::Script
        );
        assert_eq!(
            PropertyCategory::categorize("custom"),
            PropertyCategory::Misc
        );
    }

    #[test]
    fn property_changed_callback_fires() {
        let (mut tree, node_id) = make_tree_with_node();
        let mut panel = InspectorPanel::new();
        panel.inspect(node_id);

        let fired = Rc::new(Cell::new(false));
        let fired_clone = fired.clone();
        panel.on_property_changed(move |_name, _old, _new| {
            fired_clone.set(true);
        });

        panel.set_property(&mut tree, "position", Variant::Int(99));
        assert!(fired.get());
    }

    #[test]
    fn specific_property_callback_only_fires_for_match() {
        let (mut tree, node_id) = make_tree_with_node();
        let mut panel = InspectorPanel::new();
        panel.inspect(node_id);

        let count = Rc::new(Cell::new(0u32));
        let count_clone = count.clone();
        panel.on_specific_property_changed("position", move |_name, _old, _new| {
            count_clone.set(count_clone.get() + 1);
        });

        panel.set_property(&mut tree, "velocity", Variant::Float(5.0));
        assert_eq!(count.get(), 0); // should not fire for velocity

        panel.set_property(&mut tree, "position", Variant::Int(10));
        assert_eq!(count.get(), 1); // should fire for position
    }

    #[test]
    fn list_by_category() {
        let (tree, node_id) = make_tree_with_node();
        let mut panel = InspectorPanel::new();
        panel.inspect(node_id);

        let grouped = panel.list_properties_by_category(&tree);
        assert!(grouped.contains_key(&PropertyCategory::Transform));
        assert!(grouped.contains_key(&PropertyCategory::Physics));
        assert!(grouped.contains_key(&PropertyCategory::Rendering));
    }

    #[test]
    fn clear_inspected_node() {
        let mut panel = InspectorPanel::new();
        let id = NodeId::next();
        panel.inspect(id);
        assert_eq!(panel.inspected_node(), Some(id));
        panel.clear();
        assert_eq!(panel.inspected_node(), None);
    }

    // -- EditorInspectorPlugin tests --

    /// A test inspector plugin that customizes Node2D properties.
    struct TestInspectorPlugin;

    impl EditorInspectorPlugin for TestInspectorPlugin {
        fn plugin_id(&self) -> &str {
            "test_inspector"
        }

        fn can_handle(&self, class_name: &str) -> bool {
            class_name == "Node2D"
        }

        fn parse_property(
            &self,
            _class_name: &str,
            property_name: &str,
            _property_type: &Variant,
        ) -> Option<CustomPropertyEditor> {
            match property_name {
                "position" => Some(
                    CustomPropertyEditor::new("position")
                        .with_label("Position (px)")
                        .with_hint(PropertyHint::Range {
                            min: -10000,
                            max: 10000,
                            step: 1,
                        }),
                ),
                _ => None,
            }
        }

        fn hidden_properties(&self, _class_name: &str) -> Vec<String> {
            vec!["internal_debug".to_string()]
        }

        fn add_custom_sections(&self, _class_name: &str) -> Vec<InspectorSection> {
            vec![InspectorSection::new_custom("Custom Info").with_property(
                CustomPropertyEditor::new("info_label")
                    .with_label("Node Info")
                    .read_only()
                    .with_tooltip("Auto-generated node info"),
            )]
        }
    }

    #[test]
    fn inspector_plugin_registry_register_and_query() {
        let mut registry = InspectorPluginRegistry::new();
        assert_eq!(registry.plugin_count(), 0);

        registry.register(Box::new(TestInspectorPlugin));
        assert_eq!(registry.plugin_count(), 1);
        assert_eq!(registry.plugin_ids(), vec!["test_inspector"]);
    }

    #[test]
    fn inspector_plugin_registry_find_editor() {
        let mut registry = InspectorPluginRegistry::new();
        registry.register(Box::new(TestInspectorPlugin));

        // Should find a custom editor for "position" on Node2D
        let editor = registry.find_property_editor("Node2D", "position", &Variant::Int(0));
        assert!(editor.is_some());
        let editor = editor.unwrap();
        assert_eq!(editor.label, Some("Position (px)".to_string()));
        assert!(matches!(editor.hint, PropertyHint::Range { .. }));

        // Should not find an editor for "velocity" on Node2D
        let editor = registry.find_property_editor("Node2D", "velocity", &Variant::Float(0.0));
        assert!(editor.is_none());

        // Should not find an editor for Node3D (plugin doesn't handle it)
        let editor = registry.find_property_editor("Node3D", "position", &Variant::Int(0));
        assert!(editor.is_none());
    }

    #[test]
    fn inspector_plugin_registry_hidden_properties() {
        let mut registry = InspectorPluginRegistry::new();
        registry.register(Box::new(TestInspectorPlugin));

        let hidden = registry.hidden_properties("Node2D");
        assert_eq!(hidden, vec!["internal_debug"]);

        // Node3D is not handled
        let hidden = registry.hidden_properties("Node3D");
        assert!(hidden.is_empty());
    }

    #[test]
    fn inspector_plugin_registry_custom_sections() {
        let mut registry = InspectorPluginRegistry::new();
        registry.register(Box::new(TestInspectorPlugin));

        let sections = registry.custom_sections("Node2D");
        assert_eq!(sections.len(), 1);
        assert_eq!(sections[0].name, "Custom Info");
        assert_eq!(sections[0].properties.len(), 1);
        assert!(sections[0].properties[0].read_only);
    }

    #[test]
    fn inspector_plugin_registry_unregister() {
        let mut registry = InspectorPluginRegistry::new();
        registry.register(Box::new(TestInspectorPlugin));
        assert_eq!(registry.plugin_count(), 1);

        registry.unregister("test_inspector");
        assert_eq!(registry.plugin_count(), 0);

        // No editors should be found after unregistration
        let editor = registry.find_property_editor("Node2D", "position", &Variant::Int(0));
        assert!(editor.is_none());
    }

    #[test]
    fn custom_property_editor_builder() {
        let editor = CustomPropertyEditor::new("color")
            .with_label("Tint Color")
            .with_hint(PropertyHint::ColorNoAlpha)
            .read_only()
            .with_tooltip("The node's tint color");

        assert_eq!(editor.property_name, "color");
        assert_eq!(editor.label, Some("Tint Color".to_string()));
        assert_eq!(editor.hint, PropertyHint::ColorNoAlpha);
        assert!(editor.read_only);
        assert_eq!(editor.tooltip, Some("The node's tint color".to_string()));
    }

    #[test]
    fn property_hint_enum() {
        let hint = PropertyHint::Enum(vec!["Option A".into(), "Option B".into()]);
        assert!(matches!(hint, PropertyHint::Enum(opts) if opts.len() == 2));
    }

    #[test]
    fn inspector_section_builder() {
        let section = InspectorSection::new_custom("Physics Overrides")
            .with_property(CustomPropertyEditor::new("custom_gravity"))
            .with_property(CustomPropertyEditor::new("custom_damping"));

        assert_eq!(section.name, "Physics Overrides");
        assert_eq!(section.properties.len(), 2);
        assert_eq!(section.properties[0].property_name, "custom_gravity");
    }

    // -- Favorites tests --

    #[test]
    fn favorites_add_remove_toggle() {
        let mut panel = InspectorPanel::new();
        assert!(!panel.is_favorite("position"));

        panel.add_favorite("position");
        assert!(panel.is_favorite("position"));

        panel.add_favorite("velocity");
        assert_eq!(panel.favorites(), vec!["position", "velocity"]);

        panel.remove_favorite("position");
        assert!(!panel.is_favorite("position"));
        assert_eq!(panel.favorites(), vec!["velocity"]);
    }

    #[test]
    fn favorites_toggle_returns_state() {
        let mut panel = InspectorPanel::new();
        assert!(panel.toggle_favorite("position")); // now favorite
        assert!(panel.is_favorite("position"));
        assert!(!panel.toggle_favorite("position")); // no longer favorite
        assert!(!panel.is_favorite("position"));
    }

    #[test]
    fn favorite_entries_returns_matching_properties() {
        let (tree, node_id) = make_tree_with_node();
        let mut panel = InspectorPanel::new();
        panel.inspect(node_id);
        panel.add_favorite("position");
        panel.add_favorite("visible");

        let favs = panel.favorite_entries(&tree);
        assert_eq!(favs.len(), 2);
        let names: Vec<&str> = favs.iter().map(|e| e.name.as_str()).collect();
        assert!(names.contains(&"position"));
        assert!(names.contains(&"visible"));
    }

    #[test]
    fn favorite_entries_empty_when_no_favorites() {
        let (tree, node_id) = make_tree_with_node();
        let mut panel = InspectorPanel::new();
        panel.inspect(node_id);
        assert!(panel.favorite_entries(&tree).is_empty());
    }

    // -- Export group tests --

    #[test]
    fn export_group_assignment() {
        let mut panel = InspectorPanel::new();
        panel.set_export_group("speed", "Movement", None);
        panel.set_export_group("jump_height", "Movement", Some("Jumping".to_string()));

        let eg = panel.export_group_for("speed").unwrap();
        assert_eq!(eg.group, "Movement");
        assert_eq!(eg.subgroup, None);

        let eg = panel.export_group_for("jump_height").unwrap();
        assert_eq!(eg.group, "Movement");
        assert_eq!(eg.subgroup, Some("Jumping".to_string()));

        assert!(panel.export_group_for("position").is_none());
    }

    #[test]
    fn properties_by_export_group() {
        let (tree, node_id) = make_tree_with_node();
        let mut panel = InspectorPanel::new();
        panel.inspect(node_id);
        panel.set_export_group("position", "Transform", None);
        panel.set_export_group("velocity", "Physics", None);

        let grouped = panel.properties_by_export_group(&tree);
        assert_eq!(grouped.len(), 2);
        assert!(grouped.contains_key("Transform"));
        assert!(grouped.contains_key("Physics"));
        assert_eq!(grouped["Transform"].len(), 1);
        assert_eq!(grouped["Transform"][0].name, "position");
    }

    #[test]
    fn clear_export_groups() {
        let mut panel = InspectorPanel::new();
        panel.set_export_group("speed", "Movement", None);
        panel.clear_export_groups();
        assert!(panel.export_group_for("speed").is_none());
    }

    // -- New PropertyHint variant tests --

    #[test]
    fn property_hint_flags() {
        let hint = PropertyHint::Flags(vec!["Collision".into(), "Trigger".into()]);
        assert!(matches!(hint, PropertyHint::Flags(f) if f.len() == 2));
    }

    #[test]
    fn property_hint_layers() {
        let hint = PropertyHint::Layers {
            layer_type: LayerType::Physics2D,
        };
        assert!(matches!(
            hint,
            PropertyHint::Layers {
                layer_type: LayerType::Physics2D
            }
        ));
    }

    #[test]
    fn property_hint_exp_range() {
        let hint = PropertyHint::ExpRange {
            min: 0,
            max: 1000,
            step: 1,
        };
        assert!(matches!(
            hint,
            PropertyHint::ExpRange {
                min: 0,
                max: 1000,
                step: 1
            }
        ));
    }

    #[test]
    fn property_hint_dir_and_global_file() {
        let _dir = PropertyHint::Dir;
        let _gf = PropertyHint::GlobalFile("*.png".into());
        let _ph = PropertyHint::PlaceholderText("Enter name...".into());
        let _ee = PropertyHint::ExpEasing;
        let _np = PropertyHint::NodePathValidTypes(vec!["Node2D".into()]);
        let _rt = PropertyHint::ResourceType("Texture2D".into());
    }

    // -- New EditorHint / PropertyEditor with_hint tests --

    #[test]
    fn editor_hint_flags_produces_flags_editor() {
        let base = PropertyEditor::SpinBoxInt {
            min: None,
            max: None,
            step: 1,
        };
        let result = base.with_hint(&EditorHint::Flags(vec!["A".into(), "B".into()]));
        assert!(matches!(result, PropertyEditor::FlagsEditor { flags } if flags.len() == 2));
    }

    #[test]
    fn editor_hint_dir_produces_dir_picker() {
        let base = PropertyEditor::LineEdit;
        let result = base.with_hint(&EditorHint::Dir);
        assert!(matches!(result, PropertyEditor::DirPicker));
    }

    #[test]
    fn editor_hint_easing_produces_easing_editor() {
        let base = PropertyEditor::SpinBoxFloat {
            min: None,
            max: None,
            step: 0.001,
        };
        let result = base.with_hint(&EditorHint::ExpEasing);
        assert!(matches!(result, PropertyEditor::EasingEditor));
    }

    #[test]
    fn editor_hint_layers_produces_layer_editor() {
        let base = PropertyEditor::SpinBoxInt {
            min: None,
            max: None,
            step: 1,
        };
        let result = base.with_hint(&EditorHint::Layers(LayerType::Render3D));
        assert!(matches!(
            result,
            PropertyEditor::LayerEditor {
                layer_type: LayerType::Render3D
            }
        ));
    }

    #[test]
    fn editor_hint_exp_range_refines_spinbox() {
        let base = PropertyEditor::SpinBoxFloat {
            min: None,
            max: None,
            step: 0.001,
        };
        let result = base.with_hint(&EditorHint::ExpRange {
            min: 1.0,
            max: 100.0,
            step: 0.1,
        });
        assert!(
            matches!(result, PropertyEditor::SpinBoxFloat { min: Some(m), max: Some(mx), .. } if m == 1.0 && mx == 100.0)
        );
    }

    #[test]
    fn editor_hint_placeholder_preserves_editor() {
        let base = PropertyEditor::LineEdit;
        let result = base.with_hint(&EditorHint::PlaceholderText("hint".into()));
        assert!(matches!(result, PropertyEditor::LineEdit));
    }

    #[test]
    fn new_editor_display_names() {
        assert_eq!(
            PropertyEditor::FlagsEditor { flags: vec![] }.display_name(),
            "Flags"
        );
        assert_eq!(
            PropertyEditor::LayerEditor {
                layer_type: LayerType::Physics2D
            }
            .display_name(),
            "Layers"
        );
        assert_eq!(PropertyEditor::EasingEditor.display_name(), "Easing");
        assert_eq!(PropertyEditor::DirPicker.display_name(), "DirPicker");
    }

    // -- Revert-to-default tests --

    #[test]
    fn property_defaults_node2d() {
        let defaults = PropertyDefaults::node2d_defaults();
        assert!(!defaults.is_empty());
        assert!(defaults.get("position").is_some());
        assert!(defaults.get("visible").is_some());
        assert!(defaults.get("nonexistent").is_none());
    }

    #[test]
    fn is_property_modified_detects_change() {
        let (mut tree, node_id) = make_tree_with_node();
        let mut panel = InspectorPanel::new();
        panel.inspect(node_id);

        let mut defaults = PropertyDefaults::new();
        defaults.set("position", Variant::Int(0));

        assert!(!panel.is_property_modified(&tree, "position", &defaults));
        panel.set_property(&mut tree, "position", Variant::Int(42));
        assert!(panel.is_property_modified(&tree, "position", &defaults));
    }

    #[test]
    fn revert_to_default_restores_value() {
        let (mut tree, node_id) = make_tree_with_node();
        let mut panel = InspectorPanel::new();
        panel.inspect(node_id);

        let mut defaults = PropertyDefaults::new();
        defaults.set("position", Variant::Int(0));

        panel.set_property(&mut tree, "position", Variant::Int(99));
        assert_eq!(panel.get_property(&tree, "position"), Variant::Int(99));

        let old = panel.revert_to_default(&mut tree, "position", &defaults);
        assert_eq!(old, Variant::Int(99));
        assert_eq!(panel.get_property(&tree, "position"), Variant::Int(0));
    }

    #[test]
    fn revert_to_default_no_registered_default() {
        let (mut tree, node_id) = make_tree_with_node();
        let mut panel = InspectorPanel::new();
        panel.inspect(node_id);
        let defaults = PropertyDefaults::new();

        let result = panel.revert_to_default(&mut tree, "position", &defaults);
        assert_eq!(result, Variant::Nil);
    }

    // -- Copy property path tests --

    #[test]
    fn copy_property_path_format() {
        let (tree, node_id) = make_tree_with_node();
        let mut panel = InspectorPanel::new();
        panel.inspect(node_id);

        let path = panel.copy_property_path(&tree, "position");
        assert!(path.is_some());
        let path = path.unwrap();
        assert!(path.ends_with(":position"), "got: {}", path);
        assert!(path.contains("Player"), "got: {}", path);
    }

    #[test]
    fn copy_node_path() {
        let (tree, node_id) = make_tree_with_node();
        let mut panel = InspectorPanel::new();
        panel.inspect(node_id);

        let path = panel.copy_node_path(&tree);
        assert!(path.is_some());
        assert!(path.unwrap().contains("Player"));
    }

    #[test]
    fn copy_property_path_no_node() {
        let tree = SceneTree::new();
        let panel = InspectorPanel::new();
        assert!(panel.copy_property_path(&tree, "position").is_none());
    }

    // -- Drag-to-adjust tests --

    #[test]
    fn drag_adjust_float_basic() {
        let mut drag = DragAdjust::begin_float("rotation", 0.0);
        assert!(drag.active);
        assert_eq!(drag.current_value(), 0.0);

        drag.update(100.0); // 100px drag
        assert!((drag.current_value() - 1.0).abs() < 1e-10); // default sensitivity 0.01
    }

    #[test]
    fn drag_adjust_int_rounds() {
        let mut drag = DragAdjust::begin_int("z_index", 5);
        drag.update(30.0); // 30px * 0.1 = 3.0
        assert_eq!(drag.current_value_int(), 8); // 5 + 3
    }

    #[test]
    fn drag_adjust_with_range_clamps() {
        let mut drag = DragAdjust::begin_float("opacity", 0.5).with_range(0.0, 1.0);
        drag.update(1000.0); // would be 10.5 without clamp
        assert_eq!(drag.current_value(), 1.0);

        drag.update(-1000.0); // would be -9.5 without clamp
        assert_eq!(drag.current_value(), 0.0);
    }

    #[test]
    fn drag_adjust_custom_sensitivity() {
        let mut drag = DragAdjust::begin_float("speed", 10.0).with_sensitivity(0.1);
        drag.update(50.0);
        assert!((drag.current_value() - 15.0).abs() < 1e-10);
    }

    #[test]
    fn drag_adjust_end_deactivates() {
        let mut drag = DragAdjust::begin_float("x", 5.0);
        drag.update(50.0);
        let final_val = drag.end();
        assert!(!drag.active);
        assert!((final_val - 5.5).abs() < 1e-10);
    }

    #[test]
    fn drag_adjust_to_variant() {
        let mut drag = DragAdjust::begin_float("opacity", 0.5);
        drag.update(50.0); // 0.5 + 50*0.01 = 1.0
        assert_eq!(drag.to_variant_float(), Variant::Float(1.0));

        let mut drag_int = DragAdjust::begin_int("count", 10);
        drag_int.update(20.0); // 10 + 20*0.1 = 12
        assert_eq!(drag_int.to_variant_int(), Variant::Int(12));
    }

    // -- Linked values tests --

    #[test]
    fn linked_values_toggle() {
        let mut linked = LinkedValues::new();
        assert!(!linked.is_linked("scale"));

        assert!(linked.toggle("scale")); // now linked
        assert!(linked.is_linked("scale"));

        assert!(!linked.toggle("scale")); // now unlinked
        assert!(!linked.is_linked("scale"));
    }

    #[test]
    fn linked_values_vec2_uniform() {
        let mut linked = LinkedValues::new();
        linked.link("scale");

        let v = gdcore::math::Vector2::new(2.0, 1.0);
        let result = linked.apply_vec2("scale", v, 0); // changed x
        assert_eq!(result.x, 2.0);
        assert_eq!(result.y, 2.0);
    }

    #[test]
    fn linked_values_vec3_uniform() {
        let mut linked = LinkedValues::new();
        linked.link("scale");

        let v = gdcore::math::Vector3::new(1.0, 3.0, 1.0);
        let result = linked.apply_vec3("scale", v, 1); // changed y
        assert_eq!(result.x, 3.0);
        assert_eq!(result.y, 3.0);
        assert_eq!(result.z, 3.0);
    }

    #[test]
    fn linked_values_unlinked_passthrough() {
        let linked = LinkedValues::new();
        let v = gdcore::math::Vector3::new(1.0, 2.0, 3.0);
        let result = linked.apply_vec3("scale", v, 0);
        assert_eq!(result.x, 1.0);
        assert_eq!(result.y, 2.0);
        assert_eq!(result.z, 3.0);
    }

    #[test]
    fn linked_values_link_unlink() {
        let mut linked = LinkedValues::new();
        linked.link("scale");
        linked.link("size");
        assert!(linked.is_linked("scale"));
        assert!(linked.is_linked("size"));

        linked.unlink("scale");
        assert!(!linked.is_linked("scale"));
        assert!(linked.is_linked("size"));
    }

    // -- InspectorHistoryEntry ----------------------------------------------

    #[test]
    fn history_entry_node() {
        let id = NodeId::next();
        let entry = InspectorHistoryEntry::node(id);
        assert_eq!(entry.node_id, id);
        assert!(entry.subresource_path.is_none());
    }

    #[test]
    fn history_entry_subresource() {
        let id = NodeId::next();
        let entry = InspectorHistoryEntry::subresource(id, "material");
        assert_eq!(entry.node_id, id);
        assert_eq!(entry.subresource_path.as_deref(), Some("material"));
    }

    // -- InspectorHistory ---------------------------------------------------

    #[test]
    fn history_empty_state() {
        let h = InspectorHistory::new();
        assert!(h.is_empty());
        assert_eq!(h.len(), 0);
        assert!(!h.can_go_back());
        assert!(!h.can_go_forward());
        assert!(h.current().is_none());
    }

    #[test]
    fn history_push_and_current() {
        let mut h = InspectorHistory::new();
        let id1 = NodeId::next();
        h.push(InspectorHistoryEntry::node(id1));
        assert_eq!(h.len(), 1);
        assert_eq!(h.current().unwrap().node_id, id1);
    }

    #[test]
    fn history_back_and_forward() {
        let mut h = InspectorHistory::new();
        let id1 = NodeId::next();
        let id2 = NodeId::next();
        let id3 = NodeId::next();
        h.push(InspectorHistoryEntry::node(id1));
        h.push(InspectorHistoryEntry::node(id2));
        h.push(InspectorHistoryEntry::node(id3));

        assert!(h.can_go_back());
        assert!(!h.can_go_forward());

        let prev = h.back().unwrap();
        assert_eq!(prev.node_id, id2);
        assert!(h.can_go_forward());

        let prev2 = h.back().unwrap();
        assert_eq!(prev2.node_id, id1);

        assert!(!h.can_go_back());

        let next = h.forward().unwrap();
        assert_eq!(next.node_id, id2);
    }

    #[test]
    fn history_push_truncates_forward() {
        let mut h = InspectorHistory::new();
        let id1 = NodeId::next();
        let id2 = NodeId::next();
        let id3 = NodeId::next();
        let id4 = NodeId::next();
        h.push(InspectorHistoryEntry::node(id1));
        h.push(InspectorHistoryEntry::node(id2));
        h.push(InspectorHistoryEntry::node(id3));

        h.back(); // at 2
        h.back(); // at 1

        // Push new entry from position 1 — should discard 2 and 3
        h.push(InspectorHistoryEntry::node(id4));
        assert_eq!(h.len(), 2); // 1, 4
        assert!(!h.can_go_forward());
        assert_eq!(h.current().unwrap().node_id, id4);
    }

    #[test]
    fn history_clear() {
        let mut h = InspectorHistory::new();
        h.push(InspectorHistoryEntry::node(NodeId::next()));
        h.push(InspectorHistoryEntry::node(NodeId::next()));
        h.clear();
        assert!(h.is_empty());
        assert!(h.current().is_none());
    }

    #[test]
    fn history_back_at_start_returns_none() {
        let mut h = InspectorHistory::new();
        h.push(InspectorHistoryEntry::node(NodeId::next()));
        assert!(h.back().is_none());
    }

    #[test]
    fn history_forward_at_end_returns_none() {
        let mut h = InspectorHistory::new();
        h.push(InspectorHistoryEntry::node(NodeId::next()));
        assert!(h.forward().is_none());
    }

    // -- InspectorToolbar ---------------------------------------------------

    #[test]
    fn toolbar_inspect_node() {
        let mut tb = InspectorToolbar::new();
        let id = NodeId::next();
        tb.inspect_node(id, "Player", "CharacterBody2D");
        assert_eq!(tb.object_class, "CharacterBody2D");
        assert_eq!(tb.object_name, "Player");
        assert_eq!(tb.breadcrumb_depth(), 1);
        assert!(tb.breadcrumbs[0].label.contains("Player"));
    }

    #[test]
    fn toolbar_drill_into_subresource() {
        let mut tb = InspectorToolbar::new();
        let id = NodeId::next();
        tb.inspect_node(id, "Sprite", "Sprite2D");
        tb.drill_into_subresource(id, "material", "SpatialMaterial");
        assert_eq!(tb.breadcrumb_depth(), 2);
        assert_eq!(tb.breadcrumbs[1].label, "SpatialMaterial");
    }

    #[test]
    fn toolbar_navigate_back() {
        let mut tb = InspectorToolbar::new();
        let id = NodeId::next();
        tb.inspect_node(id, "Sprite", "Sprite2D");
        tb.drill_into_subresource(id, "material", "Material");
        assert!(tb.can_go_back());

        let entry = tb.navigate_back().unwrap();
        assert_eq!(entry.node_id, id);
        assert!(entry.subresource_path.is_none());
        assert_eq!(tb.breadcrumb_depth(), 1);
    }

    #[test]
    fn toolbar_navigate_to_breadcrumb() {
        let mut tb = InspectorToolbar::new();
        let id = NodeId::next();
        tb.inspect_node(id, "Mesh", "MeshInstance3D");
        tb.drill_into_subresource(id, "material", "Material");
        tb.drill_into_subresource(id, "material/albedo_texture", "Texture");

        assert_eq!(tb.breadcrumb_depth(), 3);

        // Navigate back to the root node breadcrumb.
        let entry = tb.navigate_to_breadcrumb(0).unwrap();
        assert_eq!(entry.node_id, id);
        assert!(entry.subresource_path.is_none());
        assert_eq!(tb.breadcrumb_depth(), 1);
    }

    #[test]
    fn toolbar_breadcrumb_out_of_bounds() {
        let mut tb = InspectorToolbar::new();
        tb.inspect_node(NodeId::next(), "Root", "Node");
        assert!(tb.navigate_to_breadcrumb(5).is_none());
    }

    #[test]
    fn toolbar_default_state() {
        let tb = InspectorToolbar::new();
        assert!(tb.object_class.is_empty());
        assert!(tb.object_name.is_empty());
        assert_eq!(tb.breadcrumb_depth(), 0);
        assert!(!tb.can_go_back());
        assert!(!tb.can_go_forward());
    }
}
