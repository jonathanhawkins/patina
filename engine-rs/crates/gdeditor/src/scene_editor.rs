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

// ---------------------------------------------------------------------------
// Scene Tab Bar — multi-scene tab management
// ---------------------------------------------------------------------------

/// A single tab in the scene tab bar.
#[derive(Debug, Clone)]
pub struct SceneTab {
    /// Scene file path (e.g. "res://levels/main.tscn").
    pub path: String,
    /// Display title (filename without path).
    pub title: String,
    /// Whether this tab has unsaved changes.
    pub dirty: bool,
    /// Whether this tab's close button is hovered (UI state).
    pub close_hovered: bool,
}

impl SceneTab {
    /// Create a new scene tab from a file path.
    pub fn new(path: impl Into<String>) -> Self {
        let path = path.into();
        let title = path
            .rsplit('/')
            .next()
            .unwrap_or(&path)
            .to_string();
        Self {
            path,
            title,
            dirty: false,
            close_hovered: false,
        }
    }

    /// Create a tab for a new unsaved scene.
    pub fn new_unsaved(title: impl Into<String>) -> Self {
        let title = title.into();
        Self {
            path: String::new(),
            title,
            dirty: true,
            close_hovered: false,
        }
    }

    /// Returns the display title, with a "*" prefix if dirty.
    pub fn display_title(&self) -> String {
        if self.dirty {
            format!("(*) {}", self.title)
        } else {
            self.title.clone()
        }
    }

    /// Whether this tab has been saved to a file.
    pub fn has_path(&self) -> bool {
        !self.path.is_empty()
    }
}

/// Manages a bar of scene tabs, supporting open, close, reorder,
/// and "new tab" operations per Godot 4.x editor conventions.
#[derive(Debug)]
pub struct SceneTabBar {
    /// Open tabs in display order.
    tabs: Vec<SceneTab>,
    /// Index of the currently active tab, or None if no tabs.
    active_index: Option<usize>,
    /// Counter for generating unique names for new unsaved scenes.
    new_scene_counter: usize,
}

impl SceneTabBar {
    /// Create an empty tab bar.
    pub fn new() -> Self {
        Self {
            tabs: Vec::new(),
            active_index: None,
            new_scene_counter: 0,
        }
    }

    /// Returns all open tabs.
    pub fn tabs(&self) -> &[SceneTab] {
        &self.tabs
    }

    /// Returns the number of open tabs.
    pub fn tab_count(&self) -> usize {
        self.tabs.len()
    }

    /// Returns the index of the active tab.
    pub fn active_index(&self) -> Option<usize> {
        self.active_index
    }

    /// Returns the active tab, if any.
    pub fn active_tab(&self) -> Option<&SceneTab> {
        self.active_index.and_then(|i| self.tabs.get(i))
    }

    /// Returns the active tab mutably, if any.
    pub fn active_tab_mut(&mut self) -> Option<&mut SceneTab> {
        self.active_index.and_then(|i| self.tabs.get_mut(i))
    }

    /// Open a scene in a new tab. If already open, switch to it.
    /// Returns the tab index.
    pub fn open_scene(&mut self, path: &str) -> usize {
        // Check if already open.
        if let Some(idx) = self.tabs.iter().position(|t| t.path == path) {
            self.active_index = Some(idx);
            return idx;
        }

        let tab = SceneTab::new(path);
        self.tabs.push(tab);
        let idx = self.tabs.len() - 1;
        self.active_index = Some(idx);
        idx
    }

    /// Create a new unsaved scene tab (the "+" button action).
    /// Returns the tab index.
    pub fn new_scene(&mut self) -> usize {
        self.new_scene_counter += 1;
        let title = if self.new_scene_counter == 1 {
            "[unsaved]".to_string()
        } else {
            format!("[unsaved {}]", self.new_scene_counter)
        };
        let tab = SceneTab::new_unsaved(title);
        self.tabs.push(tab);
        let idx = self.tabs.len() - 1;
        self.active_index = Some(idx);
        idx
    }

    /// Close a tab by index. Returns the closed tab, or None if invalid.
    ///
    /// After closing, the active tab adjusts:
    /// - If the closed tab was active, switch to the nearest neighbor.
    /// - If the closed tab was before the active tab, shift active left.
    pub fn close_tab(&mut self, index: usize) -> Option<SceneTab> {
        if index >= self.tabs.len() {
            return None;
        }

        let tab = self.tabs.remove(index);

        if self.tabs.is_empty() {
            self.active_index = None;
        } else if let Some(active) = self.active_index {
            if active == index {
                // Closed the active tab — switch to nearest.
                self.active_index = Some(index.min(self.tabs.len() - 1));
            } else if active > index {
                // Shift left since a tab before the active was removed.
                self.active_index = Some(active - 1);
            }
        }

        Some(tab)
    }

    /// Close the currently active tab. Returns the closed tab.
    pub fn close_active(&mut self) -> Option<SceneTab> {
        self.active_index.and_then(|i| self.close_tab(i))
    }

    /// Switch to a tab by index. Returns false if index is invalid.
    pub fn set_active(&mut self, index: usize) -> bool {
        if index < self.tabs.len() {
            self.active_index = Some(index);
            true
        } else {
            false
        }
    }

    /// Mark the active tab as dirty (unsaved changes).
    pub fn mark_active_dirty(&mut self) {
        if let Some(tab) = self.active_tab_mut() {
            tab.dirty = true;
        }
    }

    /// Mark the active tab as clean (saved).
    pub fn mark_active_clean(&mut self) {
        if let Some(tab) = self.active_tab_mut() {
            tab.dirty = false;
        }
    }

    /// Update the path for the active tab (after "Save As").
    pub fn set_active_path(&mut self, path: &str) {
        if let Some(tab) = self.active_tab_mut() {
            tab.path = path.to_string();
            tab.title = path
                .rsplit('/')
                .next()
                .unwrap_or(path)
                .to_string();
        }
    }

    /// Returns indices of tabs with unsaved changes.
    pub fn dirty_tab_indices(&self) -> Vec<usize> {
        self.tabs
            .iter()
            .enumerate()
            .filter(|(_, t)| t.dirty)
            .map(|(i, _)| i)
            .collect()
    }

    /// Returns true if any tab has unsaved changes.
    pub fn has_dirty_tabs(&self) -> bool {
        self.tabs.iter().any(|t| t.dirty)
    }

    /// Move a tab from one position to another (drag-and-drop reorder).
    pub fn move_tab(&mut self, from: usize, to: usize) -> bool {
        if from >= self.tabs.len() || to >= self.tabs.len() || from == to {
            return false;
        }

        let tab = self.tabs.remove(from);
        self.tabs.insert(to, tab);

        // Adjust active index to follow the moved tab if needed.
        if let Some(active) = self.active_index {
            if active == from {
                self.active_index = Some(to);
            } else if from < active && active <= to {
                self.active_index = Some(active - 1);
            } else if to <= active && active < from {
                self.active_index = Some(active + 1);
            }
        }

        true
    }

    /// Find a tab by path. Returns the index if found.
    pub fn find_tab(&self, path: &str) -> Option<usize> {
        self.tabs.iter().position(|t| t.path == path)
    }
}

impl Default for SceneTabBar {
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

    // -- SceneTabBar tests --

    #[test]
    fn tab_bar_empty() {
        let bar = SceneTabBar::new();
        assert_eq!(bar.tab_count(), 0);
        assert!(bar.active_index().is_none());
        assert!(bar.active_tab().is_none());
    }

    #[test]
    fn open_scene_tab() {
        let mut bar = SceneTabBar::new();
        let idx = bar.open_scene("res://main.tscn");
        assert_eq!(idx, 0);
        assert_eq!(bar.tab_count(), 1);
        assert_eq!(bar.active_index(), Some(0));
        assert_eq!(bar.active_tab().unwrap().title, "main.tscn");
        assert_eq!(bar.active_tab().unwrap().path, "res://main.tscn");
        assert!(!bar.active_tab().unwrap().dirty);
    }

    #[test]
    fn open_same_scene_switches_tab() {
        let mut bar = SceneTabBar::new();
        bar.open_scene("res://a.tscn");
        bar.open_scene("res://b.tscn");
        assert_eq!(bar.tab_count(), 2);
        assert_eq!(bar.active_index(), Some(1));

        // Opening a again should switch, not create duplicate.
        let idx = bar.open_scene("res://a.tscn");
        assert_eq!(idx, 0);
        assert_eq!(bar.tab_count(), 2);
        assert_eq!(bar.active_index(), Some(0));
    }

    #[test]
    fn new_scene_tab() {
        let mut bar = SceneTabBar::new();
        let idx = bar.new_scene();
        assert_eq!(idx, 0);
        assert_eq!(bar.active_tab().unwrap().title, "[unsaved]");
        assert!(bar.active_tab().unwrap().dirty);
        assert!(!bar.active_tab().unwrap().has_path());

        let idx2 = bar.new_scene();
        assert_eq!(idx2, 1);
        assert_eq!(bar.active_tab().unwrap().title, "[unsaved 2]");
    }

    #[test]
    fn close_tab_middle() {
        let mut bar = SceneTabBar::new();
        bar.open_scene("res://a.tscn");
        bar.open_scene("res://b.tscn");
        bar.open_scene("res://c.tscn");
        bar.set_active(1); // b is active

        let closed = bar.close_tab(1).unwrap();
        assert_eq!(closed.path, "res://b.tscn");
        assert_eq!(bar.tab_count(), 2);
        // Active should now be index 1 (c.tscn, since b was removed).
        assert_eq!(bar.active_index(), Some(1));
        assert_eq!(bar.active_tab().unwrap().path, "res://c.tscn");
    }

    #[test]
    fn close_tab_last() {
        let mut bar = SceneTabBar::new();
        bar.open_scene("res://a.tscn");
        bar.open_scene("res://b.tscn");
        // Active is b (index 1).
        assert_eq!(bar.active_index(), Some(1));

        let closed = bar.close_tab(1).unwrap();
        assert_eq!(closed.path, "res://b.tscn");
        // Should fall back to index 0 (a.tscn).
        assert_eq!(bar.active_index(), Some(0));
        assert_eq!(bar.active_tab().unwrap().path, "res://a.tscn");
    }

    #[test]
    fn close_tab_before_active() {
        let mut bar = SceneTabBar::new();
        bar.open_scene("res://a.tscn");
        bar.open_scene("res://b.tscn");
        bar.open_scene("res://c.tscn");
        // Active is c (index 2).

        bar.close_tab(0); // close a
        // Active should shift from 2 to 1.
        assert_eq!(bar.active_index(), Some(1));
        assert_eq!(bar.active_tab().unwrap().path, "res://c.tscn");
    }

    #[test]
    fn close_all_tabs() {
        let mut bar = SceneTabBar::new();
        bar.open_scene("res://a.tscn");
        bar.close_tab(0);
        assert_eq!(bar.tab_count(), 0);
        assert!(bar.active_index().is_none());
    }

    #[test]
    fn close_active_tab() {
        let mut bar = SceneTabBar::new();
        bar.open_scene("res://a.tscn");
        bar.open_scene("res://b.tscn");
        let closed = bar.close_active().unwrap();
        assert_eq!(closed.path, "res://b.tscn");
        assert_eq!(bar.tab_count(), 1);
    }

    #[test]
    fn close_invalid_index() {
        let mut bar = SceneTabBar::new();
        assert!(bar.close_tab(0).is_none());
        assert!(bar.close_tab(99).is_none());
    }

    #[test]
    fn set_active_tab() {
        let mut bar = SceneTabBar::new();
        bar.open_scene("res://a.tscn");
        bar.open_scene("res://b.tscn");
        bar.open_scene("res://c.tscn");

        assert!(bar.set_active(0));
        assert_eq!(bar.active_tab().unwrap().path, "res://a.tscn");

        assert!(bar.set_active(2));
        assert_eq!(bar.active_tab().unwrap().path, "res://c.tscn");

        assert!(!bar.set_active(99));
    }

    #[test]
    fn dirty_tracking() {
        let mut bar = SceneTabBar::new();
        bar.open_scene("res://a.tscn");
        bar.open_scene("res://b.tscn");

        assert!(!bar.has_dirty_tabs());
        assert!(bar.dirty_tab_indices().is_empty());

        bar.set_active(0);
        bar.mark_active_dirty();
        assert!(bar.has_dirty_tabs());
        assert_eq!(bar.dirty_tab_indices(), vec![0]);

        bar.mark_active_clean();
        assert!(!bar.has_dirty_tabs());
    }

    #[test]
    fn display_title_dirty() {
        let mut tab = SceneTab::new("res://level.tscn");
        assert_eq!(tab.display_title(), "level.tscn");
        tab.dirty = true;
        assert_eq!(tab.display_title(), "(*) level.tscn");
    }

    #[test]
    fn set_active_path_after_save_as() {
        let mut bar = SceneTabBar::new();
        bar.new_scene();
        assert!(!bar.active_tab().unwrap().has_path());

        bar.set_active_path("res://new_scene.tscn");
        assert_eq!(bar.active_tab().unwrap().path, "res://new_scene.tscn");
        assert_eq!(bar.active_tab().unwrap().title, "new_scene.tscn");
    }

    #[test]
    fn move_tab_reorder() {
        let mut bar = SceneTabBar::new();
        bar.open_scene("res://a.tscn");
        bar.open_scene("res://b.tscn");
        bar.open_scene("res://c.tscn");
        bar.set_active(0); // a is active

        assert!(bar.move_tab(0, 2)); // move a to end
        assert_eq!(bar.tabs()[0].path, "res://b.tscn");
        assert_eq!(bar.tabs()[1].path, "res://c.tscn");
        assert_eq!(bar.tabs()[2].path, "res://a.tscn");
        assert_eq!(bar.active_index(), Some(2)); // a followed
    }

    #[test]
    fn move_tab_invalid() {
        let mut bar = SceneTabBar::new();
        bar.open_scene("res://a.tscn");
        assert!(!bar.move_tab(0, 1)); // out of bounds
        assert!(!bar.move_tab(0, 0)); // same position
    }

    #[test]
    fn find_tab_by_path() {
        let mut bar = SceneTabBar::new();
        bar.open_scene("res://a.tscn");
        bar.open_scene("res://b.tscn");
        assert_eq!(bar.find_tab("res://a.tscn"), Some(0));
        assert_eq!(bar.find_tab("res://b.tscn"), Some(1));
        assert_eq!(bar.find_tab("res://c.tscn"), None);
    }

    #[test]
    fn scene_tab_new_unsaved() {
        let tab = SceneTab::new_unsaved("Test Scene");
        assert_eq!(tab.title, "Test Scene");
        assert!(tab.dirty);
        assert!(!tab.has_path());
        assert!(tab.path.is_empty());
    }
}
