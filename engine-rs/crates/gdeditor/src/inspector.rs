//! Property inspector and editing interface.
//!
//! The [`InspectorPanel`] provides a view into a node's properties,
//! organized by category. It supports listing, getting, and setting
//! properties, and notifying listeners when a property changes.

#![allow(clippy::type_complexity)]

use std::collections::HashMap;

use gdscene::node::NodeId;
use gdscene::SceneTree;
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
            | "global_rotation" | "global_scale" | "global_transform" | "skew" => {
                Self::Transform
            }
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
            grouped.entry(entry.category.clone()).or_default().push(entry);
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
        assert_eq!(PropertyCategory::categorize("position"), PropertyCategory::Transform);
        assert_eq!(PropertyCategory::categorize("visible"), PropertyCategory::Rendering);
        assert_eq!(PropertyCategory::categorize("velocity"), PropertyCategory::Physics);
        assert_eq!(PropertyCategory::categorize("script_var"), PropertyCategory::Script);
        assert_eq!(PropertyCategory::categorize("custom"), PropertyCategory::Misc);
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
}
