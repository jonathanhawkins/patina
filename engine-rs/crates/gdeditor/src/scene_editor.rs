//! High-level scene editor combining scene tree and editor commands.
//!
//! [`SceneEditor`] wraps a [`SceneTree`] and an [`Editor`], providing
//! convenient methods for scene manipulation with full undo support.

use gdscene::node::NodeId;
use gdscene::SceneTree;

use crate::{Editor, EditorCommand, EditorError, EditorResult};

/// A high-level scene editor.
///
/// Combines the scene tree with the editor command system, providing
/// a user-friendly API for opening, saving, and manipulating scenes
/// with full undo/redo support.
pub struct SceneEditor {
    /// The underlying editor (owns the scene tree).
    editor: Editor,
    /// Path to the currently open scene file, if any.
    open_scene_path: Option<String>,
    /// Whether the scene has unsaved changes.
    dirty: bool,
}

impl std::fmt::Debug for SceneEditor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SceneEditor")
            .field("open_scene_path", &self.open_scene_path)
            .field("dirty", &self.dirty)
            .field("selected", &self.editor.selected_node())
            .field("undo_depth", &self.editor.undo_depth())
            .finish()
    }
}

impl SceneEditor {
    /// Creates a new scene editor with a fresh scene tree.
    pub fn new() -> Self {
        Self {
            editor: Editor::new(SceneTree::new()),
            open_scene_path: None,
            dirty: false,
        }
    }

    /// Creates a scene editor wrapping an existing scene tree.
    pub fn with_tree(tree: SceneTree) -> Self {
        Self {
            editor: Editor::new(tree),
            open_scene_path: None,
            dirty: false,
        }
    }

    /// Returns a reference to the underlying editor.
    pub fn editor(&self) -> &Editor {
        &self.editor
    }

    /// Returns a mutable reference to the underlying editor.
    pub fn editor_mut(&mut self) -> &mut Editor {
        &mut self.editor
    }

    /// Returns a reference to the scene tree.
    pub fn tree(&self) -> &SceneTree {
        self.editor.tree()
    }

    /// Opens a scene by setting the path. Replaces the current tree.
    ///
    /// In a full implementation this would parse a `.tscn` file;
    /// here it resets state and records the path.
    pub fn open_scene(&mut self, path: &str) {
        self.editor = Editor::new(SceneTree::new());
        self.open_scene_path = Some(path.to_string());
        self.dirty = false;
        tracing::debug!("Opened scene: {}", path);
    }

    /// Opens a scene from an existing tree with a path.
    pub fn open_scene_with_tree(&mut self, path: &str, tree: SceneTree) {
        self.editor = Editor::new(tree);
        self.open_scene_path = Some(path.to_string());
        self.dirty = false;
    }

    /// Saves the scene (marks as clean). Returns the path.
    ///
    /// In a full implementation this would serialize to `.tscn`.
    pub fn save_scene(&mut self, path: &str) -> EditorResult<String> {
        self.open_scene_path = Some(path.to_string());
        self.dirty = false;
        tracing::debug!("Saved scene: {}", path);
        Ok(path.to_string())
    }

    /// Returns the path of the currently open scene.
    pub fn open_scene_path(&self) -> Option<&str> {
        self.open_scene_path.as_deref()
    }

    /// Returns `true` if the scene has unsaved modifications.
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// Returns the currently selected node ID.
    pub fn get_selected_node(&self) -> Option<NodeId> {
        self.editor.selected_node()
    }

    /// Selects a node by ID.
    pub fn select_node(&mut self, id: NodeId) {
        self.editor.select_node(id);
    }

    /// Clears the selection.
    pub fn deselect(&mut self) {
        self.editor.deselect();
    }

    /// Adds a child node under the currently selected node via an undoable command.
    ///
    /// If no node is selected, returns [`EditorError::NoSelection`].
    pub fn add_node_to_selected(&mut self, name: &str, class_name: &str) -> EditorResult<NodeId> {
        let parent_id = self
            .editor
            .selected_node()
            .ok_or(EditorError::NoSelection)?;

        let cmd = EditorCommand::AddNode {
            parent_id,
            name: name.to_string(),
            class_name: class_name.to_string(),
            created_id: None,
        };
        self.editor.execute(cmd)?;
        self.dirty = true;

        // The created ID was filled in by execute.
        if let Some(EditorCommand::AddNode {
            created_id: Some(id),
            ..
        }) = self.editor.undo_stack_last()
        {
            return Ok(*id);
        }

        Err(EditorError::NoSelection)
    }

    /// Deletes the currently selected node via an undoable command.
    ///
    /// Clears the selection after deletion.
    pub fn delete_selected(&mut self) -> EditorResult<()> {
        let node_id = self
            .editor
            .selected_node()
            .ok_or(EditorError::NoSelection)?;
        let node = self.editor.tree().get_node(node_id).ok_or_else(|| {
            EditorError::Engine(gdcore::error::EngineError::NotFound(
                "node not found".into(),
            ))
        })?;
        let name = node.name().to_string();
        let class_name = node.class_name().to_string();

        let cmd = EditorCommand::RemoveNode {
            node_id,
            parent_id: None,
            name,
            class_name,
        };
        self.editor.execute(cmd)?;
        self.editor.deselect();
        self.dirty = true;
        Ok(())
    }

    /// Reparents the currently selected node to a new parent via an undoable command.
    pub fn reparent_selected(&mut self, new_parent_id: NodeId) -> EditorResult<()> {
        let node_id = self
            .editor
            .selected_node()
            .ok_or(EditorError::NoSelection)?;

        let cmd = EditorCommand::ReparentNode {
            node_id,
            new_parent_id,
            old_parent_id: None,
        };
        self.editor.execute(cmd)?;
        self.dirty = true;
        Ok(())
    }

    /// Undoes the last operation.
    pub fn undo(&mut self) -> EditorResult<()> {
        self.editor.undo()?;
        self.dirty = true;
        Ok(())
    }

    /// Redoes the last undone operation.
    pub fn redo(&mut self) -> EditorResult<()> {
        self.editor.redo()?;
        self.dirty = true;
        Ok(())
    }
}

impl Default for SceneEditor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gdscene::node::Node;

    fn make_scene_editor() -> SceneEditor {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let main = Node::new("Main", "Node");
        tree.add_child(root, main).unwrap();
        SceneEditor::with_tree(tree)
    }

    #[test]
    fn new_scene_editor() {
        let se = SceneEditor::new();
        assert!(se.open_scene_path().is_none());
        assert!(!se.is_dirty());
        assert!(se.get_selected_node().is_none());
    }

    #[test]
    fn open_and_save_scene() {
        let mut se = make_scene_editor();
        se.open_scene("res://main.tscn");
        assert_eq!(se.open_scene_path(), Some("res://main.tscn"));
        assert!(!se.is_dirty());

        se.save_scene("res://main.tscn").unwrap();
        assert!(!se.is_dirty());
    }

    #[test]
    fn select_and_deselect() {
        let mut se = make_scene_editor();
        let root = se.tree().root_id();
        se.select_node(root);
        assert_eq!(se.get_selected_node(), Some(root));
        se.deselect();
        assert!(se.get_selected_node().is_none());
    }

    #[test]
    fn add_node_to_selected() {
        let mut se = make_scene_editor();
        let root = se.tree().root_id();
        let main_id = se.tree().get_node(root).unwrap().children()[0];
        se.select_node(main_id);

        let child_id = se.add_node_to_selected("Player", "Node2D").unwrap();
        assert!(se.tree().get_node(child_id).is_some());
        assert_eq!(se.tree().get_node(child_id).unwrap().name(), "Player");
        assert!(se.is_dirty());
    }

    #[test]
    fn add_node_without_selection_fails() {
        let mut se = make_scene_editor();
        let result = se.add_node_to_selected("Node", "Node");
        assert!(result.is_err());
    }

    #[test]
    fn delete_selected() {
        let mut se = make_scene_editor();
        let root = se.tree().root_id();
        let main_id = se.tree().get_node(root).unwrap().children()[0];
        let initial_count = se.tree().node_count();

        se.select_node(main_id);
        se.delete_selected().unwrap();
        assert_eq!(se.tree().node_count(), initial_count - 1);
        assert!(se.get_selected_node().is_none());
        assert!(se.is_dirty());
    }

    #[test]
    fn delete_without_selection_fails() {
        let mut se = make_scene_editor();
        assert!(se.delete_selected().is_err());
    }

    #[test]
    fn reparent_selected() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let a = Node::new("A", "Node");
        let a_id = tree.add_child(root, a).unwrap();
        let b = Node::new("B", "Node");
        let b_id = tree.add_child(root, b).unwrap();
        let c = Node::new("C", "Node");
        let c_id = tree.add_child(a_id, c).unwrap();

        let mut se = SceneEditor::with_tree(tree);
        se.select_node(c_id);
        se.reparent_selected(b_id).unwrap();

        assert_eq!(se.tree().get_node(c_id).unwrap().parent(), Some(b_id));
        assert!(se.is_dirty());
    }

    #[test]
    fn undo_redo_add_node() {
        let mut se = make_scene_editor();
        let root = se.tree().root_id();
        let main_id = se.tree().get_node(root).unwrap().children()[0];
        let initial_count = se.tree().node_count();

        se.select_node(main_id);
        se.add_node_to_selected("Child", "Sprite2D").unwrap();
        assert_eq!(se.tree().node_count(), initial_count + 1);

        se.undo().unwrap();
        assert_eq!(se.tree().node_count(), initial_count);

        se.redo().unwrap();
        assert_eq!(se.tree().node_count(), initial_count + 1);
    }

    #[test]
    fn save_clears_dirty_flag() {
        let mut se = make_scene_editor();
        let root = se.tree().root_id();
        se.select_node(root);
        se.add_node_to_selected("X", "Node").unwrap();
        assert!(se.is_dirty());

        se.save_scene("res://test.tscn").unwrap();
        assert!(!se.is_dirty());
    }
}
