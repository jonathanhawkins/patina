//! Godot 4-compatible `EditorInterface` singleton.
//!
//! In Godot 4, `EditorInterface` is the central API that editor plugins use
//! to access editor subsystems — the inspector, file system dock, script
//! editor, editor settings, and scene operations. This module provides
//! Patina's equivalent, wiring together the existing editor modules behind
//! a single, discoverable API surface.
//!
//! Plugins call `EditorInterface::get_editor_settings()`,
//! `EditorInterface::get_selection()`, etc., matching Godot's naming so
//! that documentation and tutorials transfer directly.

use std::path::Path;

use gdscene::node::NodeId;
use gdscene::scene_saver::TscnSaver;
use gdscene::SceneTree;

use crate::filesystem::EditorFileSystem;
use crate::inspector::InspectorPanel;
use crate::settings::{EditorSettings, EditorTheme};
use crate::{Editor, EditorCommand, EditorError, EditorResult};

/// Central editor API, mirroring Godot 4's `EditorInterface` singleton.
///
/// Provides access to editor subsystems so that plugins and tools can
/// inspect and manipulate the editor state through a stable, Godot-
/// compatible surface.
pub struct EditorInterface {
    /// The core editor state (scene tree, undo/redo, plugins).
    editor: Editor,
    /// User-level editor settings.
    settings: EditorSettings,
    /// Project file system browser.
    file_system: EditorFileSystem,
    /// Inspector panel.
    inspector: InspectorPanel,
    /// The currently open scene path (if saved).
    current_scene_path: Option<String>,
    /// Whether distraction-free mode is active.
    distraction_free: bool,
    /// Whether the bottom panel is visible.
    bottom_panel_visible: bool,
}

impl EditorInterface {
    /// Creates a new `EditorInterface` wrapping the given editor and project root.
    pub fn new(editor: Editor, project_root: impl Into<std::path::PathBuf>) -> Self {
        Self {
            editor,
            settings: EditorSettings::default(),
            file_system: EditorFileSystem::new(project_root),
            inspector: InspectorPanel::new(),
            current_scene_path: None,
            distraction_free: false,
            bottom_panel_visible: true,
        }
    }

    // -----------------------------------------------------------------
    // Editor access (matches Godot EditorInterface API names)
    // -----------------------------------------------------------------

    /// Returns a reference to the core `Editor`.
    pub fn get_editor(&self) -> &Editor {
        &self.editor
    }

    /// Returns a mutable reference to the core `Editor`.
    pub fn get_editor_mut(&mut self) -> &mut Editor {
        &mut self.editor
    }

    /// Returns the current editor settings.
    pub fn get_editor_settings(&self) -> &EditorSettings {
        &self.settings
    }

    /// Returns a mutable reference to the editor settings.
    pub fn get_editor_settings_mut(&mut self) -> &mut EditorSettings {
        &mut self.settings
    }

    /// Returns the editor file system dock.
    pub fn get_file_system_dock(&self) -> &EditorFileSystem {
        &self.file_system
    }

    /// Returns a mutable reference to the file system dock.
    pub fn get_file_system_dock_mut(&mut self) -> &mut EditorFileSystem {
        &mut self.file_system
    }

    /// Returns the inspector panel.
    pub fn get_inspector(&self) -> &InspectorPanel {
        &self.inspector
    }

    /// Returns a mutable reference to the inspector panel.
    pub fn get_inspector_mut(&mut self) -> &mut InspectorPanel {
        &mut self.inspector
    }

    /// Returns the scene tree being edited.
    pub fn get_edited_scene_root(&self) -> &SceneTree {
        self.editor.tree()
    }

    /// Returns a mutable reference to the scene tree being edited.
    pub fn get_edited_scene_root_mut(&mut self) -> &mut SceneTree {
        self.editor.tree_mut()
    }

    // -----------------------------------------------------------------
    // Selection (matches Godot EditorInterface.get_selection())
    // -----------------------------------------------------------------

    /// Returns the currently selected node ID, if any.
    pub fn get_selection(&self) -> Option<NodeId> {
        self.editor.selected_node()
    }

    /// Selects a node by ID, notifying plugins.
    pub fn select_node(&mut self, id: NodeId) {
        self.editor.select_node(id);
        self.editor.notify_selection_changed();
    }

    /// Clears the current selection.
    pub fn deselect(&mut self) {
        self.editor.deselect();
        self.editor.notify_selection_changed();
    }

    // -----------------------------------------------------------------
    // Scene operations
    // -----------------------------------------------------------------

    /// Returns the file path of the currently open scene, if saved.
    pub fn get_current_path(&self) -> Option<&str> {
        self.current_scene_path.as_deref()
    }

    /// Sets the current scene path (called after save/load).
    pub fn set_current_path(&mut self, path: impl Into<String>) {
        self.current_scene_path = Some(path.into());
    }

    /// Saves the current scene to a `.tscn` file at `path`.
    pub fn save_scene(&mut self, path: &str) -> EditorResult<()> {
        let tree = self.editor.tree();
        let root = tree.root_id();
        let tscn = TscnSaver::save_tree(tree, root);
        std::fs::write(path, tscn)
            .map_err(|e| EditorError::Engine(gdcore::error::EngineError::Io(e)))?;
        self.current_scene_path = Some(path.to_string());
        self.settings.recent_files.retain(|f| f != path);
        self.settings.recent_files.insert(0, path.to_string());
        tracing::info!("Saved scene to {}", path);
        Ok(())
    }

    /// Loads a scene from a `.tscn` file at `path`, replacing the current scene.
    pub fn open_scene_from_path(&mut self, path: &str) -> EditorResult<()> {
        let source = std::fs::read_to_string(path)
            .map_err(|e| EditorError::Engine(gdcore::error::EngineError::Io(e)))?;
        use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
        let packed = PackedScene::from_tscn(&source).map_err(|e| {
            EditorError::Engine(gdcore::error::EngineError::InvalidOperation(format!(
                "failed to parse tscn: {e}"
            )))
        })?;
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        add_packed_scene_to_tree(&mut tree, root, &packed).map_err(EditorError::Engine)?;
        self.editor = Editor::new(tree);
        self.current_scene_path = Some(path.to_string());
        self.settings.recent_files.retain(|f| f != path);
        self.settings.recent_files.insert(0, path.to_string());
        tracing::info!("Opened scene from {}", path);
        Ok(())
    }

    /// Reloads the currently open scene from disk.
    pub fn reload_scene_from_disk(&mut self) -> EditorResult<()> {
        let path = self.current_scene_path.clone().ok_or(EditorError::Engine(
            gdcore::error::EngineError::InvalidOperation("no scene path to reload from".into()),
        ))?;
        self.open_scene_from_path(&path)
    }

    // -----------------------------------------------------------------
    // Command execution (delegates to Editor)
    // -----------------------------------------------------------------

    /// Executes an editor command with undo support.
    pub fn execute_command(&mut self, command: EditorCommand) -> EditorResult<()> {
        self.editor.execute(command)
    }

    /// Undoes the last command.
    pub fn undo(&mut self) -> EditorResult<()> {
        self.editor.undo()
    }

    /// Redoes the last undone command.
    pub fn redo(&mut self) -> EditorResult<()> {
        self.editor.redo()
    }

    // -----------------------------------------------------------------
    // UI state (matches Godot EditorInterface display methods)
    // -----------------------------------------------------------------

    /// Returns whether distraction-free mode is active.
    pub fn is_distraction_free_mode_enabled(&self) -> bool {
        self.distraction_free
    }

    /// Toggles distraction-free mode.
    pub fn set_distraction_free_mode(&mut self, enabled: bool) {
        self.distraction_free = enabled;
    }

    /// Returns whether the bottom panel is visible.
    pub fn is_bottom_panel_visible(&self) -> bool {
        self.bottom_panel_visible
    }

    /// Shows or hides the bottom panel.
    pub fn set_bottom_panel_visible(&mut self, visible: bool) {
        self.bottom_panel_visible = visible;
    }

    /// Returns the editor theme.
    pub fn get_editor_theme(&self) -> EditorTheme {
        self.settings.theme
    }

    // -----------------------------------------------------------------
    // File system scanning
    // -----------------------------------------------------------------

    /// Triggers a project file system scan.
    pub fn scan_file_system(&mut self) -> EditorResult<usize> {
        self.file_system.scan().map_err(EditorError::Engine)
    }

    /// Returns the project root path.
    pub fn get_project_root(&self) -> &Path {
        self.file_system.project_root()
    }

    // -----------------------------------------------------------------
    // Plugin convenience
    // -----------------------------------------------------------------

    /// Returns the names of all registered editor plugins.
    pub fn get_plugin_names(&self) -> Vec<&str> {
        self.editor.plugin_names()
    }

    /// Returns whether the editor has unsaved changes (non-empty undo stack).
    pub fn is_scene_modified(&self) -> bool {
        self.editor.undo_depth() > 0
    }
}

impl std::fmt::Debug for EditorInterface {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EditorInterface")
            .field("current_scene_path", &self.current_scene_path)
            .field("distraction_free", &self.distraction_free)
            .field("editor", &self.editor)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gdscene::node::Node;

    fn make_interface() -> EditorInterface {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let main = Node::new("Main", "Node2D");
        tree.add_child(root, main).unwrap();
        let editor = Editor::new(tree);
        EditorInterface::new(editor, "/tmp/test_project")
    }

    #[test]
    fn selection_round_trip() {
        let mut ei = make_interface();
        assert!(ei.get_selection().is_none());

        let root = ei.get_edited_scene_root().root_id();
        ei.select_node(root);
        assert_eq!(ei.get_selection(), Some(root));

        ei.deselect();
        assert!(ei.get_selection().is_none());
    }

    #[test]
    fn settings_access() {
        let ei = make_interface();
        assert_eq!(ei.get_editor_settings().window_size, (1280, 720));
        assert_eq!(ei.get_editor_theme(), EditorTheme::Dark);
    }

    #[test]
    fn distraction_free_mode() {
        let mut ei = make_interface();
        assert!(!ei.is_distraction_free_mode_enabled());
        ei.set_distraction_free_mode(true);
        assert!(ei.is_distraction_free_mode_enabled());
    }

    #[test]
    fn bottom_panel_visibility() {
        let mut ei = make_interface();
        assert!(ei.is_bottom_panel_visible());
        ei.set_bottom_panel_visible(false);
        assert!(!ei.is_bottom_panel_visible());
    }

    #[test]
    fn execute_command_and_undo() {
        let mut ei = make_interface();
        let root = ei.get_edited_scene_root().root_id();
        let children = ei
            .get_edited_scene_root()
            .get_node(root)
            .unwrap()
            .children()
            .to_vec();
        let main_id = children[0];

        let cmd = EditorCommand::SetProperty {
            node_id: main_id,
            property: "visible".to_string(),
            new_value: gdvariant::Variant::Bool(false),
            old_value: gdvariant::Variant::Nil,
        };
        ei.execute_command(cmd).unwrap();
        assert!(ei.is_scene_modified());

        ei.undo().unwrap();
        assert!(!ei.is_scene_modified());
    }

    #[test]
    fn plugin_names() {
        let ei = make_interface();
        assert!(ei.get_plugin_names().is_empty());
    }

    #[test]
    fn project_root() {
        let ei = make_interface();
        assert_eq!(ei.get_project_root(), Path::new("/tmp/test_project"));
    }

    #[test]
    fn current_path() {
        let mut ei = make_interface();
        assert!(ei.get_current_path().is_none());
        ei.set_current_path("res://main.tscn");
        assert_eq!(ei.get_current_path(), Some("res://main.tscn"));
    }

    #[test]
    fn debug_display() {
        let ei = make_interface();
        let debug = format!("{:?}", ei);
        assert!(debug.contains("EditorInterface"));
        assert!(debug.contains("current_scene_path"));
    }
}
