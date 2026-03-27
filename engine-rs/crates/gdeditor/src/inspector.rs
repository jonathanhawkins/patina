//! Property inspector and editing interface.
//!
//! The [`InspectorPanel`] provides a view into a node's properties,
//! organized by category. It supports listing, getting, and setting
//! properties, and notifying listeners when a property changes.

#![allow(clippy::type_complexity)]

use std::collections::HashMap;

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
}

impl std::fmt::Debug for InspectorPanel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InspectorPanel")
            .field("inspected_node", &self.inspected_node)
            .field("callback_count", &self.on_changed.len())
            .finish()
    }
}

impl InspectorPanel {
    /// Creates a new empty inspector panel.
    pub fn new() -> Self {
        Self {
            inspected_node: None,
            on_changed: Vec::new(),
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
    Range {
        min: i64,
        max: i64,
        step: i64,
    },
    /// A drop-down of string options.
    Enum(Vec<String>),
    /// A file path selector (with optional extension filter like `"*.png"`).
    File(String),
    /// A multi-line text editor.
    MultilineText,
    /// A color picker (with optional alpha).
    ColorNoAlpha,
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

/// A custom section added to the inspector by a plugin.
#[derive(Debug, Clone)]
pub struct InspectorSection {
    /// The section title.
    pub title: String,
    /// Properties to show in this section.
    pub properties: Vec<CustomPropertyEditor>,
}

impl InspectorSection {
    /// Creates a new inspector section.
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            properties: Vec::new(),
        }
    }

    /// Adds a property editor to this section.
    pub fn with_property(mut self, editor: CustomPropertyEditor) -> Self {
        self.properties.push(editor);
        self
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
                if let Some(editor) = plugin.parse_property(class_name, property_name, property_value) {
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
/// - Bool ↔ Int ↔ Float (numeric promotion/truncation)
/// - Any type → String (via Display)
/// - String ↔ StringName ↔ NodePath
/// - Same type → identity clone
pub fn coerce_variant(value: &Variant, target: VariantType) -> Option<Variant> {
    use gdcore::node_path::NodePath;
    use gdcore::string_name::StringName;

    // Same type is always identity.
    if value.variant_type() == target {
        return Some(value.clone());
    }

    match (value, target) {
        // -- Bool → numeric --
        (Variant::Bool(b), VariantType::Int) => Some(Variant::Int(if *b { 1 } else { 0 })),
        (Variant::Bool(b), VariantType::Float) => {
            Some(Variant::Float(if *b { 1.0 } else { 0.0 }))
        }

        // -- Int → other numeric / bool --
        (Variant::Int(i), VariantType::Float) => Some(Variant::Float(*i as f64)),
        (Variant::Int(i), VariantType::Bool) => Some(Variant::Bool(*i != 0)),

        // -- Float → other numeric / bool --
        (Variant::Float(f), VariantType::Int) => Some(Variant::Int(*f as i64)),
        (Variant::Float(f), VariantType::Bool) => Some(Variant::Bool(*f != 0.0)),

        // -- String ↔ StringName (specific cases before catch-all) --
        (Variant::String(s), VariantType::StringName) => {
            Some(Variant::StringName(StringName::new(s)))
        }
        (Variant::StringName(sn), VariantType::String) => {
            Some(Variant::String(sn.as_str().to_owned()))
        }

        // -- String ↔ NodePath --
        (Variant::String(s), VariantType::NodePath) => {
            Some(Variant::NodePath(NodePath::new(s)))
        }
        (Variant::NodePath(np), VariantType::String) => {
            Some(Variant::String(np.to_string()))
        }

        // -- StringName ↔ NodePath --
        (Variant::StringName(sn), VariantType::NodePath) => {
            Some(Variant::NodePath(NodePath::new(sn.as_str())))
        }
        (Variant::NodePath(np), VariantType::StringName) => {
            Some(Variant::StringName(StringName::new(&np.to_string())))
        }

        // -- Any → String (Godot str() semantics, catch-all after specific cases) --
        (_, VariantType::String) => Some(Variant::String(value.to_string())),

        // All other coercions are unsupported.
        _ => None,
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
            vec![InspectorSection::new("Custom Info").with_property(
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
        assert_eq!(sections[0].title, "Custom Info");
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
        let section = InspectorSection::new("Physics Overrides")
            .with_property(CustomPropertyEditor::new("custom_gravity"))
            .with_property(CustomPropertyEditor::new("custom_damping"));

        assert_eq!(section.title, "Physics Overrides");
        assert_eq!(section.properties.len(), 2);
        assert_eq!(section.properties[0].property_name, "custom_gravity");
    }
}
