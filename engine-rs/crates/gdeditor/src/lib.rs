//! # gdeditor
//!
//! Editor-facing layers for the Patina Engine runtime.
//!
//! This crate provides editor infrastructure: property inspection,
//! dock panels, an undo/redo command system, and an extensibility
//! plugin trait.
//!
//! - [`inspector`] — Property inspection and change callbacks.
//! - [`dock`] — Scene tree and property dock panels.
//! - [`Editor`] — Central editor state: selection, undo/redo, open scene.
//! - [`EditorCommand`] — Undoable operations on the scene tree.
//! - [`EditorPlugin`] — Trait for extending the editor with custom behaviour.

#![warn(clippy::all)]

pub mod dock;
pub mod filesystem;
pub mod import;
pub mod inspector;
pub mod scene_editor;
pub mod settings;

use gdscene::node::{Node, NodeId};
use gdscene::SceneTree;
use gdvariant::Variant;
use thiserror::Error;

// Re-exports for convenience.
pub use dock::{DockPanel, PropertyDock, SceneTreeDock};
pub use filesystem::EditorFileSystem;
pub use import::{ImportPipeline, ImportedResource, ResourceImporter, TresImporter, TscnImporter};
pub use inspector::InspectorPanel;
pub use scene_editor::SceneEditor;
pub use settings::{EditorSettings, EditorTheme, ProjectSettings};

/// Errors specific to editor operations.
#[derive(Debug, Error)]
pub enum EditorError {
    /// An engine-level error occurred.
    #[error(transparent)]
    Engine(#[from] gdcore::error::EngineError),

    /// No node is currently selected.
    #[error("no node selected")]
    NoSelection,

    /// The undo stack is empty.
    #[error("nothing to undo")]
    NothingToUndo,

    /// The redo stack is empty.
    #[error("nothing to redo")]
    NothingToRedo,
}

/// Convenience alias for editor results.
pub type EditorResult<T> = Result<T, EditorError>;

/// An undoable editor command.
///
/// Each variant stores enough data to both execute and reverse the
/// operation, enabling full undo/redo support.
#[derive(Debug, Clone)]
pub enum EditorCommand {
    /// Set a property on a node.
    SetProperty {
        /// Target node.
        node_id: NodeId,
        /// Property name.
        property: String,
        /// The value to set.
        new_value: Variant,
        /// The value before the change (populated on execute).
        old_value: Variant,
    },
    /// Add a child node.
    AddNode {
        /// The parent to add to.
        parent_id: NodeId,
        /// The name of the new node.
        name: String,
        /// The class of the new node.
        class_name: String,
        /// The ID assigned after insertion (populated on execute).
        created_id: Option<NodeId>,
    },
    /// Remove a node (and its subtree).
    RemoveNode {
        /// The node to remove.
        node_id: NodeId,
        /// The parent it was attached to (populated on execute).
        parent_id: Option<NodeId>,
        /// The node's name (saved for undo).
        name: String,
        /// The node's class name (saved for undo).
        class_name: String,
    },
    /// Reparent a node to a new parent.
    ReparentNode {
        /// The node to move.
        node_id: NodeId,
        /// The new parent.
        new_parent_id: NodeId,
        /// The old parent (populated on execute).
        old_parent_id: Option<NodeId>,
    },
}

impl EditorCommand {
    /// Executes this command on the given scene tree.
    pub fn execute(&mut self, tree: &mut SceneTree) -> EditorResult<()> {
        match self {
            EditorCommand::SetProperty {
                node_id,
                property,
                new_value,
                old_value,
            } => {
                let node = tree
                    .get_node_mut(*node_id)
                    .ok_or_else(|| gdcore::error::EngineError::NotFound("node not found".into()))?;
                *old_value = node.set_property(property, new_value.clone());
                tracing::debug!(
                    "SetProperty {:?}.{} = {} (was {})",
                    node_id,
                    property,
                    new_value,
                    old_value
                );
                Ok(())
            }
            EditorCommand::AddNode {
                parent_id,
                name,
                class_name,
                created_id,
            } => {
                let node = Node::new(name.as_str(), class_name.as_str());
                let id = tree.add_child(*parent_id, node)?;
                *created_id = Some(id);
                tracing::debug!("AddNode {:?} under {:?}", id, parent_id);
                Ok(())
            }
            EditorCommand::RemoveNode {
                node_id, parent_id, ..
            } => {
                // Save parent for undo.
                *parent_id = tree.get_node(*node_id).and_then(|n| n.parent());
                tree.remove_node(*node_id)?;
                tracing::debug!("RemoveNode {:?}", node_id);
                Ok(())
            }
            EditorCommand::ReparentNode {
                node_id,
                new_parent_id,
                old_parent_id,
            } => {
                *old_parent_id = tree.get_node(*node_id).and_then(|n| n.parent());
                tree.reparent(*node_id, *new_parent_id)?;
                tracing::debug!("ReparentNode {:?} -> {:?}", node_id, new_parent_id);
                Ok(())
            }
        }
    }

    /// Reverses this command on the given scene tree.
    pub fn undo(&self, tree: &mut SceneTree) -> EditorResult<()> {
        match self {
            EditorCommand::SetProperty {
                node_id,
                property,
                old_value,
                ..
            } => {
                let node = tree
                    .get_node_mut(*node_id)
                    .ok_or_else(|| gdcore::error::EngineError::NotFound("node not found".into()))?;
                node.set_property(property, old_value.clone());
                Ok(())
            }
            EditorCommand::AddNode { created_id, .. } => {
                if let Some(id) = created_id {
                    tree.remove_node(*id)?;
                }
                Ok(())
            }
            EditorCommand::RemoveNode {
                parent_id,
                name,
                class_name,
                ..
            } => {
                // Re-add the node under its old parent. Note: the original
                // NodeId cannot be reused because the arena assigns fresh IDs.
                if let Some(pid) = parent_id {
                    let node = Node::new(name.as_str(), class_name.as_str());
                    tree.add_child(*pid, node)?;
                }
                Ok(())
            }
            EditorCommand::ReparentNode {
                node_id,
                old_parent_id,
                ..
            } => {
                if let Some(old_pid) = old_parent_id {
                    tree.reparent(*node_id, *old_pid)?;
                }
                Ok(())
            }
        }
    }
}

/// Central editor state.
///
/// Manages the currently selected node, the open scene, the undo/redo
/// stacks, and registered plugins.
pub struct Editor {
    /// The scene tree being edited.
    tree: SceneTree,
    /// The currently selected node, if any.
    selected_node: Option<NodeId>,
    /// Undo stack (most recent command on top).
    undo_stack: Vec<EditorCommand>,
    /// Redo stack (cleared on new command).
    redo_stack: Vec<EditorCommand>,
    /// Registered editor plugins.
    plugins: Vec<Box<dyn EditorPlugin>>,
}

impl std::fmt::Debug for Editor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Editor")
            .field("selected_node", &self.selected_node)
            .field("undo_depth", &self.undo_stack.len())
            .field("redo_depth", &self.redo_stack.len())
            .field("plugin_count", &self.plugins.len())
            .finish()
    }
}

impl Editor {
    /// Creates a new editor wrapping the given scene tree.
    pub fn new(tree: SceneTree) -> Self {
        Self {
            tree,
            selected_node: None,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            plugins: Vec::new(),
        }
    }

    /// Returns a reference to the scene tree.
    pub fn tree(&self) -> &SceneTree {
        &self.tree
    }

    /// Returns a mutable reference to the scene tree.
    pub fn tree_mut(&mut self) -> &mut SceneTree {
        &mut self.tree
    }

    /// Selects a node by ID.
    pub fn select_node(&mut self, id: NodeId) {
        self.selected_node = Some(id);
        tracing::debug!("Selected node {:?}", id);
    }

    /// Clears the current selection.
    pub fn deselect(&mut self) {
        self.selected_node = None;
    }

    /// Returns the currently selected node ID.
    pub fn selected_node(&self) -> Option<NodeId> {
        self.selected_node
    }

    /// Executes an editor command and pushes it onto the undo stack.
    ///
    /// Clears the redo stack.
    pub fn execute(&mut self, mut command: EditorCommand) -> EditorResult<()> {
        command.execute(&mut self.tree)?;
        self.undo_stack.push(command);
        self.redo_stack.clear();
        Ok(())
    }

    /// Undoes the most recent command.
    pub fn undo(&mut self) -> EditorResult<()> {
        let command = self.undo_stack.pop().ok_or(EditorError::NothingToUndo)?;
        command.undo(&mut self.tree)?;
        self.redo_stack.push(command);
        Ok(())
    }

    /// Redoes the most recently undone command.
    pub fn redo(&mut self) -> EditorResult<()> {
        let mut command = self.redo_stack.pop().ok_or(EditorError::NothingToRedo)?;
        command.execute(&mut self.tree)?;
        self.undo_stack.push(command);
        Ok(())
    }

    /// Returns the number of undoable commands.
    pub fn undo_depth(&self) -> usize {
        self.undo_stack.len()
    }

    /// Returns the number of redoable commands.
    pub fn redo_depth(&self) -> usize {
        self.redo_stack.len()
    }

    /// Registers an editor plugin.
    pub fn add_plugin(&mut self, plugin: Box<dyn EditorPlugin>) {
        tracing::debug!("Registered editor plugin: {}", plugin.name());
        self.plugins.push(plugin);
    }

    /// Returns the last command on the undo stack (for inspecting results).
    pub fn undo_stack_last(&self) -> Option<&EditorCommand> {
        self.undo_stack.last()
    }

    /// Returns the names of all registered plugins.
    pub fn plugin_names(&self) -> Vec<&str> {
        self.plugins.iter().map(|p| p.name()).collect()
    }

    /// Notifies all plugins that a node was selected.
    pub fn notify_selection_changed(&mut self) {
        // We need to call plugin methods but can't borrow self mutably
        // while iterating plugins. Collect node id first.
        let selected = self.selected_node;
        for plugin in &mut self.plugins {
            plugin.on_selection_changed(selected);
        }
    }
}

/// Trait for extending the editor with custom behaviour.
///
/// Mirrors Godot's `EditorPlugin` class. Implementations can react to
/// editor events like selection changes.
pub trait EditorPlugin {
    /// Returns the plugin's display name.
    fn name(&self) -> &str;

    /// Called when the selected node changes.
    fn on_selection_changed(&mut self, _selected: Option<NodeId>) {}

    /// Called when a command is executed.
    fn on_command_executed(&mut self, _command: &EditorCommand) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use gdscene::node::Node;
    use std::cell::Cell;
    use std::rc::Rc;

    fn make_editor() -> Editor {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let main = Node::new("Main", "Node");
        tree.add_child(root, main).unwrap();
        Editor::new(tree)
    }

    #[test]
    fn select_and_deselect() {
        let mut editor = make_editor();
        let root = editor.tree().root_id();
        editor.select_node(root);
        assert_eq!(editor.selected_node(), Some(root));
        editor.deselect();
        assert_eq!(editor.selected_node(), None);
    }

    #[test]
    fn set_property_undo_redo() {
        let mut editor = make_editor();
        let root = editor.tree().root_id();
        let main_id = editor.tree().get_node(root).unwrap().children()[0];

        // Set property via command.
        editor
            .execute(EditorCommand::SetProperty {
                node_id: main_id,
                property: "hp".to_string(),
                new_value: Variant::Int(100),
                old_value: Variant::Nil,
            })
            .unwrap();

        assert_eq!(
            editor.tree().get_node(main_id).unwrap().get_property("hp"),
            Variant::Int(100)
        );
        assert_eq!(editor.undo_depth(), 1);

        // Undo.
        editor.undo().unwrap();
        assert_eq!(
            editor.tree().get_node(main_id).unwrap().get_property("hp"),
            Variant::Nil
        );
        assert_eq!(editor.redo_depth(), 1);

        // Redo.
        editor.redo().unwrap();
        assert_eq!(
            editor.tree().get_node(main_id).unwrap().get_property("hp"),
            Variant::Int(100)
        );
    }

    #[test]
    fn add_node_undo() {
        let mut editor = make_editor();
        let root = editor.tree().root_id();
        let initial_count = editor.tree().node_count();

        editor
            .execute(EditorCommand::AddNode {
                parent_id: root,
                name: "NewNode".to_string(),
                class_name: "Sprite2D".to_string(),
                created_id: None,
            })
            .unwrap();

        assert_eq!(editor.tree().node_count(), initial_count + 1);

        editor.undo().unwrap();
        assert_eq!(editor.tree().node_count(), initial_count);
    }

    #[test]
    fn remove_node_undo() {
        let mut editor = make_editor();
        let root = editor.tree().root_id();
        let main_id = editor.tree().get_node(root).unwrap().children()[0];
        let initial_count = editor.tree().node_count();

        editor
            .execute(EditorCommand::RemoveNode {
                node_id: main_id,
                parent_id: None,
                name: "Main".to_string(),
                class_name: "Node".to_string(),
            })
            .unwrap();

        assert_eq!(editor.tree().node_count(), initial_count - 1);

        // Undo re-adds a node with the same name/class.
        editor.undo().unwrap();
        assert_eq!(editor.tree().node_count(), initial_count);
    }

    #[test]
    fn reparent_node_undo() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let a = Node::new("A", "Node");
        let a_id = tree.add_child(root, a).unwrap();
        let b = Node::new("B", "Node");
        let b_id = tree.add_child(root, b).unwrap();
        let c = Node::new("C", "Node");
        let c_id = tree.add_child(a_id, c).unwrap();

        let mut editor = Editor::new(tree);

        // Reparent C from A to B.
        editor
            .execute(EditorCommand::ReparentNode {
                node_id: c_id,
                new_parent_id: b_id,
                old_parent_id: None,
            })
            .unwrap();

        assert_eq!(editor.tree().get_node(c_id).unwrap().parent(), Some(b_id));

        // Undo.
        editor.undo().unwrap();
        assert_eq!(editor.tree().get_node(c_id).unwrap().parent(), Some(a_id));
    }

    #[test]
    fn undo_empty_stack_errors() {
        let mut editor = make_editor();
        assert!(editor.undo().is_err());
    }

    #[test]
    fn redo_empty_stack_errors() {
        let mut editor = make_editor();
        assert!(editor.redo().is_err());
    }

    #[test]
    fn new_command_clears_redo() {
        let mut editor = make_editor();
        let root = editor.tree().root_id();
        let main_id = editor.tree().get_node(root).unwrap().children()[0];

        editor
            .execute(EditorCommand::SetProperty {
                node_id: main_id,
                property: "x".to_string(),
                new_value: Variant::Int(1),
                old_value: Variant::Nil,
            })
            .unwrap();

        editor.undo().unwrap();
        assert_eq!(editor.redo_depth(), 1);

        // New command should clear redo stack.
        editor
            .execute(EditorCommand::SetProperty {
                node_id: main_id,
                property: "y".to_string(),
                new_value: Variant::Int(2),
                old_value: Variant::Nil,
            })
            .unwrap();
        assert_eq!(editor.redo_depth(), 0);
    }

    struct TestPlugin {
        selected: Rc<Cell<bool>>,
    }

    impl EditorPlugin for TestPlugin {
        fn name(&self) -> &str {
            "TestPlugin"
        }

        fn on_selection_changed(&mut self, selected: Option<NodeId>) {
            self.selected.set(selected.is_some());
        }
    }

    #[test]
    fn editor_plugin_registration() {
        let mut editor = make_editor();
        let flag = Rc::new(Cell::new(false));
        let plugin = TestPlugin {
            selected: flag.clone(),
        };
        editor.add_plugin(Box::new(plugin));
        assert_eq!(editor.plugin_names(), vec!["TestPlugin"]);

        let root = editor.tree().root_id();
        editor.select_node(root);
        editor.notify_selection_changed();
        assert!(flag.get());
    }

    #[test]
    fn multiple_undo_redo_cycle() {
        let mut editor = make_editor();
        let root = editor.tree().root_id();
        let main_id = editor.tree().get_node(root).unwrap().children()[0];

        // Execute two commands.
        editor
            .execute(EditorCommand::SetProperty {
                node_id: main_id,
                property: "a".to_string(),
                new_value: Variant::Int(1),
                old_value: Variant::Nil,
            })
            .unwrap();
        editor
            .execute(EditorCommand::SetProperty {
                node_id: main_id,
                property: "b".to_string(),
                new_value: Variant::Int(2),
                old_value: Variant::Nil,
            })
            .unwrap();

        assert_eq!(editor.undo_depth(), 2);

        // Undo both.
        editor.undo().unwrap();
        editor.undo().unwrap();
        assert_eq!(editor.undo_depth(), 0);
        assert_eq!(editor.redo_depth(), 2);

        // Properties should be reverted.
        assert_eq!(
            editor.tree().get_node(main_id).unwrap().get_property("a"),
            Variant::Nil
        );
        assert_eq!(
            editor.tree().get_node(main_id).unwrap().get_property("b"),
            Variant::Nil
        );

        // Redo both.
        editor.redo().unwrap();
        editor.redo().unwrap();
        assert_eq!(
            editor.tree().get_node(main_id).unwrap().get_property("a"),
            Variant::Int(1)
        );
        assert_eq!(
            editor.tree().get_node(main_id).unwrap().get_property("b"),
            Variant::Int(2)
        );
    }
}
