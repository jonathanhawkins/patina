//! Minimal editor-facing compatibility layer.
//!
//! Provides Godot-compatible API names and type aliases that map to Patina's
//! internal editor types. This layer enables GDExtension plugins and editor
//! scripts to use familiar Godot API names while targeting the Patina engine.
//!
//! ## Design
//!
//! - **Type aliases**: `EditorInspector` → [`InspectorPanel`], etc.
//! - **Trait adapters**: Godot trait names wrapping Patina traits.
//! - **Function wrappers**: Godot-named free functions delegating to Patina.
//! - **Enum mappings**: Godot enum names/variants → Patina equivalents.
//!
//! This module does NOT duplicate logic — it is purely a naming/adapter layer.

use crate::inspector::{
    self, CustomPropertyEditor, DragAdjust, EditorInspectorPlugin, InspectorPanel,
    InspectorPluginRegistry, LinkedValues, PropertyCategory, PropertyDefaults, PropertyEditor,
    PropertyHint,
};
use crate::{Editor, EditorCommand, EditorError};
use gdscene::node::NodeId;
use gdscene::SceneTree;
use gdvariant::variant::VariantType;
use gdvariant::Variant;

// ===========================================================================
// Type aliases — Godot names → Patina types
// ===========================================================================

/// Godot's `EditorInspector` → Patina's [`InspectorPanel`].
pub type EditorInspector = InspectorPanel;

/// Godot's `EditorProperty` → Patina's [`CustomPropertyEditor`].
pub type EditorProperty = CustomPropertyEditor;

/// Godot's `EditorInspectorPluginRegistry` → same type, aliased for discoverability.
pub type EditorInspectorPluginManager = InspectorPluginRegistry;

/// Godot's `EditorUndoRedoManager` → Patina's [`Editor`] which holds undo/redo.
pub type EditorUndoRedoManager = Editor;

// ===========================================================================
// Godot-compatible enum wrappers
// ===========================================================================

/// Maps Godot's `PropertyUsageFlags` to inspector filter categories.
///
/// In Godot, properties carry usage flags that determine where they appear
/// (editor, storage, none, etc.). This simplified version covers the
/// editor-visible cases.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PropertyUsageFlags {
    /// Property is visible in the editor inspector.
    Editor,
    /// Property is stored in scene/resource files.
    Storage,
    /// Property appears in both editor and storage.
    Default,
    /// Property is hidden from the editor inspector.
    NoEditor,
    /// Property is read-only in the inspector.
    ReadOnly,
}

impl PropertyUsageFlags {
    /// Returns whether this property should appear in the inspector.
    pub fn is_editor_visible(&self) -> bool {
        matches!(self, Self::Editor | Self::Default)
    }

    /// Returns whether this property should be persisted.
    pub fn is_stored(&self) -> bool {
        matches!(self, Self::Storage | Self::Default)
    }
}

/// Godot-compatible property info structure.
///
/// Combines property metadata in the format Godot plugins expect:
/// name, type, hint, hint string, and usage flags.
#[derive(Debug, Clone)]
pub struct EditorPropertyInfo {
    /// The property name.
    pub name: String,
    /// The variant type of this property.
    pub variant_type: VariantType,
    /// How the property should be edited.
    pub hint: PropertyHint,
    /// Human-readable hint string (e.g., range description).
    pub hint_string: String,
    /// Usage flags controlling visibility and storage.
    pub usage: PropertyUsageFlags,
}

impl EditorPropertyInfo {
    /// Creates a new property info with default flags.
    pub fn new(name: impl Into<String>, variant_type: VariantType) -> Self {
        Self {
            name: name.into(),
            variant_type,
            hint: PropertyHint::None,
            hint_string: String::new(),
            usage: PropertyUsageFlags::Default,
        }
    }

    /// Sets the property hint.
    pub fn with_hint(mut self, hint: PropertyHint, hint_string: impl Into<String>) -> Self {
        self.hint = hint;
        self.hint_string = hint_string.into();
        self
    }

    /// Sets usage flags.
    pub fn with_usage(mut self, usage: PropertyUsageFlags) -> Self {
        self.usage = usage;
        self
    }

    /// Converts to a Patina [`CustomPropertyEditor`].
    pub fn to_custom_editor(&self) -> CustomPropertyEditor {
        let mut editor = CustomPropertyEditor::new(&self.name).with_hint(self.hint.clone());
        if !self.hint_string.is_empty() {
            editor = editor.with_tooltip(&self.hint_string);
        }
        if matches!(
            self.usage,
            PropertyUsageFlags::ReadOnly | PropertyUsageFlags::NoEditor
        ) {
            editor = editor.read_only();
        }
        editor
    }
}

// ===========================================================================
// Godot-compatible free functions
// ===========================================================================

/// Validates a variant value against an expected type, with Godot-compatible
/// coercion rules.
///
/// Returns the coerced value on success, or an error message.
pub fn validate_property_value(
    value: &Variant,
    expected_type: VariantType,
) -> Result<Variant, String> {
    if value.variant_type() == expected_type {
        return Ok(value.clone());
    }
    inspector::coerce_variant(value, expected_type).ok_or_else(|| {
        format!(
            "Cannot convert {:?} to {:?}",
            value.variant_type(),
            expected_type
        )
    })
}

/// Returns the Godot-compatible category name for a property.
pub fn get_property_category(property_name: &str) -> &'static str {
    match PropertyCategory::categorize(property_name) {
        PropertyCategory::Transform => "Transform",
        PropertyCategory::Rendering => "Rendering",
        PropertyCategory::Physics => "Physics",
        PropertyCategory::Script => "Script Variables",
        PropertyCategory::Misc => "Misc",
    }
}

// ===========================================================================
// EditorSelection — Godot-compatible selection wrapper
// ===========================================================================

/// Godot-compatible editor selection API.
///
/// Wraps Patina's single-selection model with Godot-named methods.
/// Patina currently supports single node selection; this adapter presents
/// the Godot API surface while mapping to the underlying model.
pub struct EditorSelection<'a> {
    editor: &'a mut Editor,
}

impl<'a> EditorSelection<'a> {
    /// Creates a new selection wrapper around an editor instance.
    pub fn new(editor: &'a mut Editor) -> Self {
        Self { editor }
    }

    /// Returns the currently selected node, if any, as a single-element vec.
    pub fn get_selected_nodes(&self) -> Vec<NodeId> {
        self.editor.selected_node().into_iter().collect()
    }

    /// Returns the number of selected nodes (0 or 1).
    pub fn get_selected_node_count(&self) -> usize {
        if self.editor.selected_node().is_some() {
            1
        } else {
            0
        }
    }

    /// Returns whether a specific node is selected.
    pub fn is_selected(&self, node_id: NodeId) -> bool {
        self.editor.selected_node() == Some(node_id)
    }

    /// Clears the selection.
    pub fn clear(&mut self) {
        self.editor.deselect();
    }

    /// Selects a node (replaces current selection since Patina is single-select).
    pub fn add_node(&mut self, node_id: NodeId) {
        self.editor.select_node(node_id);
    }

    /// Removes a node from selection. If it's the currently selected node,
    /// clears the selection.
    pub fn remove_node(&mut self, node_id: NodeId) {
        if self.editor.selected_node() == Some(node_id) {
            self.editor.deselect();
        }
    }
}

// ===========================================================================
// EditorInterfaceCompat — Godot-style access to editor subsystems
// ===========================================================================

/// Provides Godot-compatible named accessors for editor subsystems.
///
/// In Godot, `EditorInterface` is the singleton that plugins use to access
/// the inspector, file system, script editor, etc. This struct wraps
/// Patina's equivalent functionality with Godot-compatible method names.
pub struct EditorInterfaceCompat<'a> {
    editor: &'a Editor,
}

impl<'a> EditorInterfaceCompat<'a> {
    /// Creates a new compatibility interface.
    pub fn new(editor: &'a Editor) -> Self {
        Self { editor }
    }

    /// Returns the scene tree (Godot: `get_edited_scene_root()`'s tree).
    pub fn get_tree(&self) -> &SceneTree {
        self.editor.tree()
    }

    /// Returns the current scene's root node ID (Godot: `get_edited_scene_root()`).
    pub fn get_edited_scene_root(&self) -> NodeId {
        self.editor.tree().root_id()
    }

    /// Returns the currently selected node (Godot: `get_selection()`).
    pub fn get_selection(&self) -> Option<NodeId> {
        self.editor.selected_node()
    }

    /// Returns the current editor main screen name.
    pub fn get_editor_main_screen_name(&self) -> &str {
        "3D"
    }

    /// Returns the undo/redo depth.
    pub fn get_undo_redo_depth(&self) -> (usize, usize) {
        (self.editor.undo_depth(), self.editor.redo_depth())
    }
}

// ===========================================================================
// UndoRedo compat — Godot-named undo/redo operations
// ===========================================================================

/// Godot-compatible undo/redo helper.
///
/// Wraps Patina's `Editor::execute` / `Editor::undo` / `Editor::redo` with
/// the Godot UndoRedo API naming convention.
pub struct UndoRedoCompat<'a> {
    editor: &'a mut Editor,
}

impl<'a> UndoRedoCompat<'a> {
    /// Creates a new undo/redo wrapper.
    pub fn new(editor: &'a mut Editor) -> Self {
        Self { editor }
    }

    /// Commits an action (Godot: `commit_action`).
    pub fn commit_action(&mut self, command: EditorCommand) -> Result<(), EditorError> {
        self.editor.execute(command)
    }

    /// Undoes the last action (Godot: `undo`).
    pub fn undo(&mut self) -> Result<(), EditorError> {
        self.editor.undo()
    }

    /// Redoes the last undone action (Godot: `redo`).
    pub fn redo(&mut self) -> Result<(), EditorError> {
        self.editor.redo()
    }

    /// Returns whether undo is available (Godot: `has_undo`).
    pub fn has_undo(&self) -> bool {
        self.editor.undo_depth() > 0
    }

    /// Returns whether redo is available (Godot: `has_redo`).
    pub fn has_redo(&self) -> bool {
        self.editor.redo_depth() > 0
    }

    /// Returns the current undo stack depth (Godot: `get_version`).
    pub fn get_version(&self) -> usize {
        self.editor.undo_depth()
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use gdscene::node::Node;

    #[test]
    fn property_usage_flags_visibility() {
        assert!(PropertyUsageFlags::Editor.is_editor_visible());
        assert!(PropertyUsageFlags::Default.is_editor_visible());
        assert!(!PropertyUsageFlags::NoEditor.is_editor_visible());
        assert!(!PropertyUsageFlags::Storage.is_editor_visible());
        assert!(!PropertyUsageFlags::ReadOnly.is_editor_visible());
    }

    #[test]
    fn property_usage_flags_storage() {
        assert!(PropertyUsageFlags::Storage.is_stored());
        assert!(PropertyUsageFlags::Default.is_stored());
        assert!(!PropertyUsageFlags::Editor.is_stored());
        assert!(!PropertyUsageFlags::NoEditor.is_stored());
    }

    #[test]
    fn editor_property_info_creation() {
        let info = EditorPropertyInfo::new("speed", VariantType::Float);
        assert_eq!(info.name, "speed");
        assert_eq!(info.variant_type, VariantType::Float);
        assert!(matches!(info.usage, PropertyUsageFlags::Default));
    }

    #[test]
    fn editor_property_info_with_hint() {
        let info = EditorPropertyInfo::new("speed", VariantType::Float).with_hint(
            PropertyHint::Range {
                min: 0,
                max: 100,
                step: 1,
            },
            "0,100,1",
        );
        assert!(matches!(info.hint, PropertyHint::Range { .. }));
        assert_eq!(info.hint_string, "0,100,1");
    }

    #[test]
    fn editor_property_info_to_custom_editor() {
        let info = EditorPropertyInfo::new("health", VariantType::Int)
            .with_hint(
                PropertyHint::Range {
                    min: 0,
                    max: 100,
                    step: 1,
                },
                "Health points",
            )
            .with_usage(PropertyUsageFlags::Default);
        let editor = info.to_custom_editor();
        assert_eq!(editor.property_name, "health");
        assert!(!editor.read_only);
    }

    #[test]
    fn editor_property_info_read_only() {
        let info = EditorPropertyInfo::new("id", VariantType::Int)
            .with_usage(PropertyUsageFlags::ReadOnly);
        let editor = info.to_custom_editor();
        assert!(editor.read_only);
    }

    #[test]
    fn validate_property_value_same_type() {
        let result = validate_property_value(&Variant::Int(42), VariantType::Int);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), Variant::Int(42));
    }

    #[test]
    fn validate_property_value_coercion() {
        let result = validate_property_value(&Variant::Int(42), VariantType::Float);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), Variant::Float(42.0));
    }

    #[test]
    fn validate_property_value_incompatible() {
        let result = validate_property_value(&Variant::String("hello".into()), VariantType::Int);
        assert!(result.is_err());
    }

    #[test]
    fn get_property_category_maps_correctly() {
        assert_eq!(get_property_category("position"), "Transform");
        assert_eq!(get_property_category("visible"), "Rendering");
        assert_eq!(get_property_category("velocity"), "Physics");
        assert_eq!(get_property_category("script_health"), "Script Variables");
        assert_eq!(get_property_category("custom_data"), "Misc");
    }

    #[test]
    fn type_aliases_are_correct() {
        let _inspector: EditorInspector = InspectorPanel::new();
        let _registry: EditorInspectorPluginManager = InspectorPluginRegistry::new();
    }

    #[test]
    fn editor_selection_single_select() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let child = tree.add_child(root, Node::new("Child", "Node2D")).unwrap();
        let mut editor = Editor::new(tree);

        let mut sel = EditorSelection::new(&mut editor);
        assert_eq!(sel.get_selected_node_count(), 0);
        assert!(sel.get_selected_nodes().is_empty());

        sel.add_node(child);
        assert_eq!(sel.get_selected_node_count(), 1);
        assert!(sel.is_selected(child));
        assert_eq!(sel.get_selected_nodes(), vec![child]);

        sel.remove_node(child);
        assert_eq!(sel.get_selected_node_count(), 0);
    }

    #[test]
    fn editor_selection_clear() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let child = tree.add_child(root, Node::new("A", "Node2D")).unwrap();
        let mut editor = Editor::new(tree);
        editor.select_node(child);

        let mut sel = EditorSelection::new(&mut editor);
        assert_eq!(sel.get_selected_node_count(), 1);
        sel.clear();
        assert_eq!(sel.get_selected_node_count(), 0);
    }

    #[test]
    fn editor_interface_compat_accessors() {
        let tree = SceneTree::new();
        let root = tree.root_id();
        let editor = Editor::new(tree);
        let compat = EditorInterfaceCompat::new(&editor);

        assert_eq!(compat.get_edited_scene_root(), root);
        assert_eq!(compat.get_selection(), None);
        assert_eq!(compat.get_editor_main_screen_name(), "3D");
        assert_eq!(compat.get_undo_redo_depth(), (0, 0));
    }

    #[test]
    fn undo_redo_compat() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        tree.add_child(root, Node::new("Player", "Node2D")).unwrap();
        let mut editor = Editor::new(tree);

        let mut ur = UndoRedoCompat::new(&mut editor);
        assert!(!ur.has_undo());
        assert!(!ur.has_redo());
        assert_eq!(ur.get_version(), 0);

        // Execute a command
        let cmd = EditorCommand::AddNode {
            parent_id: root,
            name: "Enemy".into(),
            class_name: "Node2D".into(),
            created_id: None,
        };
        ur.commit_action(cmd).unwrap();
        assert!(ur.has_undo());
        assert_eq!(ur.get_version(), 1);

        // Undo
        ur.undo().unwrap();
        assert!(!ur.has_undo());
        assert!(ur.has_redo());

        // Redo
        ur.redo().unwrap();
        assert!(ur.has_undo());
        assert!(!ur.has_redo());
    }
}
