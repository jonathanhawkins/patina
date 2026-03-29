// NOTE: Editor feature gate was LIFTED on 2026-03-19 — runtime parity exits are green.
// Editor feature work is now the primary focus. See CLAUDE.md.

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

pub mod animation_editor;
pub mod asset_drag_drop;
pub mod command_palette;
pub mod create_dialog;
pub mod curve_editor;
pub mod dock;
pub mod editor_compat;
pub mod editor_interface;
pub mod editor_menu;
pub mod editor_server;
pub mod editor_plugin;
pub mod editor_settings_dialog;
pub mod editor_ui;
pub mod environment_preview;
pub mod export_dialog;
pub mod filesystem;
pub mod find_replace;
pub mod group_dialog;
pub mod import;
pub mod import_settings;
pub mod inspector;
pub mod output_panel;
pub mod profiler_panel;
pub mod project_settings_dialog;
pub mod scene_editor;
pub mod scene_renderer;
pub mod script_completion;
pub mod script_editor;
pub mod script_gutter;
pub mod shader_editor;
pub mod signal_dialog;
pub mod settings;
pub mod texture_cache;
pub mod theme_editor;
pub mod tilemap_editor;
pub mod undo_redo;
pub mod vcs;
pub mod viewport_2d;
pub mod viewport_3d;

use gdscene::node::{Node, NodeId};
use gdscene::SceneTree;
use gdvariant::Variant;
use thiserror::Error;

// Re-exports for convenience.
pub use dock::{
    DockPanel, NodeIndicators, NodeTypeIcon, NodeWarning, PluginDockManager, PluginDockPanel,
    PropertyDock, SceneTreeDock, SelectionState,
};
pub use editor_interface::EditorInterface;
pub use export_dialog::{ExportBuildProfile, ExportDialog, ExportPlatform, ExportPreset};
pub use filesystem::EditorFileSystem;
pub use import::{
    AnimationLoopMode, EditorSceneFormatImporter, EditorScenePostImport, FbxSceneImporter,
    GltfSceneImporter, ImportPipeline, ImportedAnimation, ImportedResource, ImportedScene,
    ImportedSceneNode, ObjSceneImporter, ResourceImporter, SceneFormatImporterRegistry,
    SceneImportOptions, TresImporter, TscnImporter,
};
pub use inspector::{
    coerce_variant, validate_variant, CustomPropertyEditor, EditorHint,
    EditorInspectorPlugin, InspectorPanel, InspectorPluginRegistry,
    InspectorSection, PropertyEditor, PropertyHint, ResourceSubEditor,
    SectionedInspector,
};
pub use scene_editor::SceneEditor;
pub use settings::{EditorSettings, EditorTheme, ProjectSettings};
pub use viewport_3d::{
    CameraMode, EnvironmentPreview3D, GizmoAxis, GizmoMode3D, Grid3D, GridLine, PickResult,
    Projection, Ray3D, Selection3D, Viewport3D, ViewportCamera3D,
};
pub use editor_settings_dialog::{
    EditorSettingsDialog, KeyBinding, PluginInfo, SettingsTab,
};
pub use profiler_panel::{
    FrameProfile, FunctionStats, ProfilerEntry, ProfilerPanel, ProfilerStats,
};
pub use project_settings_dialog::{
    ProjectSettingsDialog, SettingsCategory, SettingsEditor, SettingsProperty, SettingsValue,
};
pub use shader_editor::{
    MaterialPreview, PreviewShape, PreviewUniformInfo, ShaderEditor, ShaderHighlightKind,
    ShaderHighlightSpan, ShaderHighlighter, ShaderTab, UniformValue,
};
pub use theme_editor::{
    OverrideEntry, OverrideKind, PreviewControl, StyleBoxFlat, ThemeColorPalette, ThemeEditor,
    ThemeFont, ThemeItem, ThemeResource,
};
pub use create_dialog::{
    CatalogEntry, ClassEntry, ClassFilter, CreateDialogResult, CreateNodeDialog, HelperPreset,
    NodeCatalog2D, NodeCategory,
};
pub use script_editor::{FindMatch, FindOptions, FindReplace, ScriptEditor};
pub use find_replace::{
    FindReplace as FindReplaceEngine, FindReplaceConfig, FindReplaceError,
    ReplaceResult, SearchMatch, SearchMode,
};
pub use vcs::{
    BranchInfo, ChangeArea, CommitEntry, FileChangeStatus, VcsFileStatus, VcsStatus,
};

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

// ---------------------------------------------------------------------------
// Editor mode (top bar mode switching)
// ---------------------------------------------------------------------------

/// The active editor workspace mode. Mirrors Godot's top-bar mode buttons:
/// 2D, 3D, Script, Game, AssetLib.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EditorMode {
    /// 2D canvas editor.
    Canvas2D,
    /// 3D spatial editor.
    Spatial3D,
    /// Script/code editor.
    Script,
    /// Game preview (running scene).
    Game,
    /// Asset library browser.
    AssetLib,
}

impl Default for EditorMode {
    fn default() -> Self {
        Self::Canvas2D
    }
}

impl EditorMode {
    /// Returns the display label for the toolbar button.
    pub fn label(&self) -> &'static str {
        match self {
            Self::Canvas2D => "2D",
            Self::Spatial3D => "3D",
            Self::Script => "Script",
            Self::Game => "Game",
            Self::AssetLib => "AssetLib",
        }
    }

    /// All modes in toolbar display order.
    pub fn all() -> [EditorMode; 5] {
        [
            Self::Canvas2D,
            Self::Spatial3D,
            Self::Script,
            Self::Game,
            Self::AssetLib,
        ]
    }

    /// Parse a mode from its string key (as used in the REST API).
    pub fn from_str_key(s: &str) -> Option<Self> {
        match s {
            "2d" => Some(Self::Canvas2D),
            "3d" => Some(Self::Spatial3D),
            "script" => Some(Self::Script),
            "game" => Some(Self::Game),
            "assetlib" => Some(Self::AssetLib),
            _ => None,
        }
    }

    /// Returns the string key for the REST API.
    pub fn key(&self) -> &'static str {
        match self {
            Self::Canvas2D => "2d",
            Self::Spatial3D => "3d",
            Self::Script => "script",
            Self::Game => "game",
            Self::AssetLib => "assetlib",
        }
    }
}

// ---------------------------------------------------------------------------
// Run controls (play/pause/stop)
// ---------------------------------------------------------------------------

/// The current play state of the editor's run controls.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PlayState {
    /// No scene is running.
    Stopped,
    /// A scene is running.
    Playing,
    /// A scene is running but paused.
    Paused,
}

impl Default for PlayState {
    fn default() -> Self {
        Self::Stopped
    }
}

/// Which scene to run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunTarget {
    /// Run the project's main scene.
    MainScene,
    /// Run the currently edited scene.
    CurrentScene,
    /// Run a specific scene by path.
    CustomScene(String),
}

/// Editor run controls state — play, pause, stop, and run target.
///
/// Mirrors Godot's top-bar run buttons (F5 = Run Project, F6 = Run Current Scene,
/// F7 = Pause, F8 = Stop).
#[derive(Debug, Clone)]
pub struct RunControls {
    /// Current play state.
    pub state: PlayState,
    /// What to run next.
    pub target: RunTarget,
    /// Whether "Run Current Scene" was the last run action.
    pub last_ran_current: bool,
}

impl Default for RunControls {
    fn default() -> Self {
        Self::new()
    }
}

impl RunControls {
    /// Creates new run controls in the stopped state.
    pub fn new() -> Self {
        Self {
            state: PlayState::Stopped,
            target: RunTarget::MainScene,
            last_ran_current: false,
        }
    }

    /// Starts playing the target scene.
    pub fn play(&mut self, target: RunTarget) {
        self.last_ran_current = matches!(target, RunTarget::CurrentScene);
        self.target = target;
        self.state = PlayState::Playing;
    }

    /// Pauses the running scene. No-op if not playing.
    pub fn pause(&mut self) {
        if self.state == PlayState::Playing {
            self.state = PlayState::Paused;
        }
    }

    /// Resumes a paused scene. No-op if not paused.
    pub fn resume(&mut self) {
        if self.state == PlayState::Paused {
            self.state = PlayState::Playing;
        }
    }

    /// Toggles between playing and paused.
    pub fn toggle_pause(&mut self) {
        match self.state {
            PlayState::Playing => self.state = PlayState::Paused,
            PlayState::Paused => self.state = PlayState::Playing,
            PlayState::Stopped => {}
        }
    }

    /// Stops the running scene.
    pub fn stop(&mut self) {
        self.state = PlayState::Stopped;
    }

    /// Whether a scene is currently running (playing or paused).
    pub fn is_running(&self) -> bool {
        self.state != PlayState::Stopped
    }

    /// Whether the scene is playing (not paused, not stopped).
    pub fn is_playing(&self) -> bool {
        self.state == PlayState::Playing
    }

    /// Whether the scene is paused.
    pub fn is_paused(&self) -> bool {
        self.state == PlayState::Paused
    }
}

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
    /// Rename a node.
    RenameNode {
        /// The node to rename.
        node_id: NodeId,
        /// The new name.
        new_name: String,
        /// The old name (populated on execute).
        old_name: String,
    },
    /// Duplicate a node (and its subtree) as a sibling.
    DuplicateNode {
        /// The node to duplicate.
        source_id: NodeId,
        /// The IDs of nodes created (populated on execute, for undo).
        created_ids: Vec<NodeId>,
    },
    /// Instance a packed scene (from `.tscn` source) under a parent node.
    InstanceScene {
        /// The parent node to instance under.
        parent_id: NodeId,
        /// The `.tscn` source text.
        tscn_source: String,
        /// The IDs of nodes created (populated on execute, for undo).
        created_ids: Vec<NodeId>,
        /// The root node of the instanced scene (populated on execute).
        root_id: Option<NodeId>,
    },
    TileMapPaint {
        node_id: NodeId,
        x: i32,
        y: i32,
        tile_id: i32,
        old_tile_id: i32,
    },
    TileMapFill {
        node_id: NodeId,
        x1: i32,
        y1: i32,
        x2: i32,
        y2: i32,
        tile_id: i32,
        old_tiles: Vec<(i32, i32, i32)>,
    },
    TileMapResize {
        node_id: NodeId,
        new_width: usize,
        new_height: usize,
        old_width: usize,
        old_height: usize,
        old_cells: Vec<i32>,
    },
    /// Move a child node to a new index within its parent's child list.
    MoveNode {
        /// The parent node.
        parent_id: NodeId,
        /// The child node to move.
        child_id: NodeId,
        /// The target index.
        new_index: usize,
        /// The original index (populated on execute).
        old_index: usize,
    },
    /// Connect a signal on a source node.
    ConnectSignal {
        /// Source node emitting the signal.
        source_id: NodeId,
        /// Signal name.
        signal_name: String,
        /// Target object ID (as raw u64).
        target_object_id: u64,
        /// Method name on the target.
        method: String,
    },
    /// Disconnect a signal on a source node.
    DisconnectSignal {
        /// Source node.
        source_id: NodeId,
        /// Signal name.
        signal_name: String,
        /// Target object ID (as raw u64).
        target_object_id: u64,
        /// Method name on the target.
        method: String,
    },
    /// Add a node to a named group.
    AddToGroup {
        /// The node.
        node_id: NodeId,
        /// The group name.
        group: String,
    },
    /// Remove a node from a named group.
    RemoveFromGroup {
        /// The node.
        node_id: NodeId,
        /// The group name.
        group: String,
    },
    /// A compound command that groups multiple sub-commands as a single
    /// undo/redo step.
    Group {
        /// Human-readable label for the compound operation.
        label: String,
        /// The sub-commands in execution order.
        commands: Vec<EditorCommand>,
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
            EditorCommand::RenameNode {
                node_id,
                new_name,
                old_name,
            } => {
                let node = tree
                    .get_node_mut(*node_id)
                    .ok_or_else(|| gdcore::error::EngineError::NotFound("node not found".into()))?;
                *old_name = node.name().to_string();
                node.set_name(new_name.as_str());
                tracing::debug!("RenameNode {:?} '{}' -> '{}'", node_id, old_name, new_name);
                Ok(())
            }
            EditorCommand::DuplicateNode {
                source_id,
                created_ids,
            } => {
                // Find the parent of the source node.
                let parent_id = tree
                    .get_node(*source_id)
                    .and_then(|n| n.parent())
                    .ok_or_else(|| {
                        gdcore::error::EngineError::InvalidOperation(
                            "cannot duplicate root node".into(),
                        )
                    })?;

                // Recursively duplicate the subtree.
                fn duplicate_subtree(
                    tree: &mut SceneTree,
                    src_id: NodeId,
                    dest_parent: NodeId,
                    created: &mut Vec<NodeId>,
                ) -> EditorResult<NodeId> {
                    let (name, class_name, props) = {
                        let node = tree.get_node(src_id).ok_or_else(|| {
                            gdcore::error::EngineError::NotFound("node not found".into())
                        })?;
                        let name = node.name().to_string();
                        let class = node.class_name().to_string();
                        let props: Vec<(String, Variant)> = node
                            .properties()
                            .map(|(k, v)| (k.clone(), v.clone()))
                            .collect();
                        (name, class, props)
                    };

                    // Get children before mutating tree.
                    let children: Vec<NodeId> = tree
                        .get_node(src_id)
                        .map(|n| n.children().to_vec())
                        .unwrap_or_default();

                    let mut new_node = Node::new(name, class_name);
                    for (k, v) in props {
                        new_node.set_property(&k, v);
                    }
                    let new_id = tree.add_child(dest_parent, new_node)?;
                    created.push(new_id);

                    for child_id in children {
                        duplicate_subtree(tree, child_id, new_id, created)?;
                    }

                    Ok(new_id)
                }

                created_ids.clear();
                duplicate_subtree(tree, *source_id, parent_id, created_ids)?;
                tracing::debug!("DuplicateNode {:?} -> {:?}", source_id, created_ids);
                Ok(())
            }
            EditorCommand::InstanceScene {
                parent_id,
                tscn_source,
                created_ids,
                root_id,
            } => {
                use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
                let packed = PackedScene::from_tscn(tscn_source).map_err(|e| {
                    gdcore::error::EngineError::InvalidOperation(format!(
                        "failed to parse tscn: {e}"
                    ))
                })?;
                let scene_root = add_packed_scene_to_tree(tree, *parent_id, &packed)?;
                *root_id = Some(scene_root);

                // Mark the instanced root with a source indicator for the UI.
                if let Some(node) = tree.get_node_mut(scene_root) {
                    node.set_property("_instance_source", Variant::String("instanced".to_string()));
                }

                // Collect all created node IDs for undo.
                fn collect_subtree_ids(tree: &SceneTree, nid: NodeId, out: &mut Vec<NodeId>) {
                    out.push(nid);
                    if let Some(node) = tree.get_node(nid) {
                        for &child in node.children() {
                            collect_subtree_ids(tree, child, out);
                        }
                    }
                }
                created_ids.clear();
                collect_subtree_ids(tree, scene_root, created_ids);
                tracing::debug!(
                    "InstanceScene root {:?} ({} nodes)",
                    scene_root,
                    created_ids.len()
                );
                Ok(())
            }
            EditorCommand::TileMapPaint { .. }
            | EditorCommand::TileMapFill { .. }
            | EditorCommand::TileMapResize { .. } => Ok(()),
            EditorCommand::MoveNode {
                parent_id,
                child_id,
                new_index,
                old_index,
            } => {
                // Find the current index of the child.
                let parent = tree
                    .get_node(*parent_id)
                    .ok_or_else(|| gdcore::error::EngineError::NotFound("parent not found".into()))?;
                let children = parent.children();
                *old_index = children
                    .iter()
                    .position(|&c| c == *child_id)
                    .unwrap_or(0);
                tree.move_child(*parent_id, *child_id, *new_index)?;
                tracing::debug!("MoveNode {:?} index {} -> {}", child_id, old_index, new_index);
                Ok(())
            }
            EditorCommand::ConnectSignal {
                source_id,
                signal_name,
                target_object_id,
                method,
            } => {
                let conn = gdobject::signal::Connection::new(
                    gdcore::id::ObjectId::from_raw(*target_object_id),
                    method.as_str(),
                );
                tree.connect_signal(*source_id, signal_name, conn);
                tracing::debug!(
                    "ConnectSignal {:?}.{} -> {}::{}",
                    source_id, signal_name, target_object_id, method
                );
                Ok(())
            }
            EditorCommand::DisconnectSignal {
                source_id,
                signal_name,
                target_object_id,
                method,
            } => {
                let store = tree.signal_store_mut(*source_id);
                store.disconnect(signal_name, gdcore::id::ObjectId::from_raw(*target_object_id), method);
                tracing::debug!(
                    "DisconnectSignal {:?}.{} -> {}::{}",
                    source_id, signal_name, target_object_id, method
                );
                Ok(())
            }
            EditorCommand::AddToGroup { node_id, group } => {
                tree.add_to_group(*node_id, group)?;
                tracing::debug!("AddToGroup {:?} -> '{}'", node_id, group);
                Ok(())
            }
            EditorCommand::RemoveFromGroup { node_id, group } => {
                tree.remove_from_group(*node_id, group)?;
                tracing::debug!("RemoveFromGroup {:?} <- '{}'", node_id, group);
                Ok(())
            }
            EditorCommand::Group { commands, label } => {
                tracing::debug!("Group '{}' executing {} commands", label, commands.len());
                for cmd in commands.iter_mut() {
                    cmd.execute(tree)?;
                }
                Ok(())
            }
        }
    }

    pub fn execute_tilemap(
        &mut self,
        store: &mut gdscene::tilemap::TileGridStore,
    ) -> EditorResult<()> {
        match self {
            EditorCommand::TileMapPaint {
                node_id,
                x,
                y,
                tile_id,
                old_tile_id,
            } => {
                if let Some(g) = store.get_mut(*node_id) {
                    *old_tile_id = g.get(*x, *y).unwrap_or(0);
                    g.set(*x, *y, *tile_id);
                }
                Ok(())
            }
            EditorCommand::TileMapFill {
                node_id,
                x1,
                y1,
                x2,
                y2,
                tile_id,
                old_tiles,
            } => {
                if let Some(g) = store.get_mut(*node_id) {
                    old_tiles.clear();
                    let (ax, bx) = (
                        (*x1).min(*x2).max(0),
                        (*x1).max(*x2).min(g.width as i32 - 1),
                    );
                    let (ay, by) = (
                        (*y1).min(*y2).max(0),
                        (*y1).max(*y2).min(g.height as i32 - 1),
                    );
                    for r in ay..=by {
                        for c in ax..=bx {
                            old_tiles.push((c, r, g.get(c, r).unwrap_or(0)));
                        }
                    }
                    g.fill_rect(*x1, *y1, *x2, *y2, *tile_id);
                }
                Ok(())
            }
            EditorCommand::TileMapResize {
                node_id,
                new_width,
                new_height,
                old_width,
                old_height,
                old_cells,
            } => {
                if let Some(g) = store.get_mut(*node_id) {
                    *old_width = g.width;
                    *old_height = g.height;
                    *old_cells = g.cells.clone();
                    g.resize(*new_width, *new_height);
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }

    pub fn undo_tilemap(&self, store: &mut gdscene::tilemap::TileGridStore) -> EditorResult<()> {
        match self {
            EditorCommand::TileMapPaint {
                node_id,
                x,
                y,
                old_tile_id,
                ..
            } => {
                if let Some(g) = store.get_mut(*node_id) {
                    g.set(*x, *y, *old_tile_id);
                }
                Ok(())
            }
            EditorCommand::TileMapFill {
                node_id, old_tiles, ..
            } => {
                if let Some(g) = store.get_mut(*node_id) {
                    for &(cx, cy, oid) in old_tiles {
                        g.set(cx, cy, oid);
                    }
                }
                Ok(())
            }
            EditorCommand::TileMapResize {
                node_id,
                old_width,
                old_height,
                old_cells,
                ..
            } => {
                if let Some(g) = store.get_mut(*node_id) {
                    g.width = *old_width;
                    g.height = *old_height;
                    g.cells = old_cells.clone();
                }
                Ok(())
            }
            _ => Ok(()),
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
            EditorCommand::RenameNode {
                node_id, old_name, ..
            } => {
                let node = tree
                    .get_node_mut(*node_id)
                    .ok_or_else(|| gdcore::error::EngineError::NotFound("node not found".into()))?;
                node.set_name(old_name.as_str());
                Ok(())
            }
            EditorCommand::DuplicateNode { created_ids, .. }
            | EditorCommand::InstanceScene { created_ids, .. } => {
                // Remove all created nodes in reverse order (children first).
                for &id in created_ids.iter().rev() {
                    let _ = tree.remove_node(id);
                }
                Ok(())
            }
            EditorCommand::TileMapPaint { .. }
            | EditorCommand::TileMapFill { .. }
            | EditorCommand::TileMapResize { .. } => Ok(()),
            EditorCommand::MoveNode {
                parent_id,
                child_id,
                old_index,
                ..
            } => {
                tree.move_child(*parent_id, *child_id, *old_index)?;
                Ok(())
            }
            EditorCommand::ConnectSignal {
                source_id,
                signal_name,
                target_object_id,
                method,
            } => {
                // Undo connect = disconnect.
                let store = tree.signal_store_mut(*source_id);
                store.disconnect(signal_name, gdcore::id::ObjectId::from_raw(*target_object_id), method);
                Ok(())
            }
            EditorCommand::DisconnectSignal {
                source_id,
                signal_name,
                target_object_id,
                method,
            } => {
                // Undo disconnect = reconnect.
                let conn = gdobject::signal::Connection::new(
                    gdcore::id::ObjectId::from_raw(*target_object_id),
                    method.as_str(),
                );
                tree.connect_signal(*source_id, signal_name, conn);
                Ok(())
            }
            EditorCommand::AddToGroup { node_id, group } => {
                // Undo add = remove.
                tree.remove_from_group(*node_id, group)?;
                Ok(())
            }
            EditorCommand::RemoveFromGroup { node_id, group } => {
                // Undo remove = add.
                tree.add_to_group(*node_id, group)?;
                Ok(())
            }
            EditorCommand::Group { commands, .. } => {
                // Undo in reverse order.
                for cmd in commands.iter().rev() {
                    cmd.undo(tree)?;
                }
                Ok(())
            }
        }
    }

    /// Returns a human-readable label for this command.
    pub fn label(&self) -> String {
        match self {
            EditorCommand::SetProperty { property, .. } => format!("Set {property}"),
            EditorCommand::AddNode { name, class_name, .. } => format!("Add {class_name} '{name}'"),
            EditorCommand::RemoveNode { name, .. } => format!("Remove '{name}'"),
            EditorCommand::ReparentNode { .. } => "Reparent node".into(),
            EditorCommand::RenameNode { old_name, new_name, .. } => {
                format!("Rename '{old_name}' -> '{new_name}'")
            }
            EditorCommand::DuplicateNode { .. } => "Duplicate node".into(),
            EditorCommand::InstanceScene { .. } => "Instance scene".into(),
            EditorCommand::TileMapPaint { .. } => "Paint tile".into(),
            EditorCommand::TileMapFill { .. } => "Fill tiles".into(),
            EditorCommand::TileMapResize { .. } => "Resize tilemap".into(),
            EditorCommand::MoveNode { .. } => "Move node".into(),
            EditorCommand::ConnectSignal { signal_name, method, .. } => {
                format!("Connect {signal_name} -> {method}")
            }
            EditorCommand::DisconnectSignal { signal_name, method, .. } => {
                format!("Disconnect {signal_name} -> {method}")
            }
            EditorCommand::AddToGroup { group, .. } => format!("Add to group '{group}'"),
            EditorCommand::RemoveFromGroup { group, .. } => format!("Remove from group '{group}'"),
            EditorCommand::Group { label, .. } => label.clone(),
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

    /// Enables a plugin by name, calling its `on_enable` hook.
    pub fn enable_plugin(&mut self, name: &str) -> bool {
        for p in &mut self.plugins {
            if p.name() == name {
                p.on_enable();
                return true;
            }
        }
        false
    }

    /// Disables a plugin by name, calling its `on_disable` hook.
    pub fn disable_plugin(&mut self, name: &str) -> bool {
        for p in &mut self.plugins {
            if p.name() == name {
                p.on_disable();
                return true;
            }
        }
        false
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

    // -------------------------------------------------------------------
    // Extended undo/redo features
    // -------------------------------------------------------------------

    /// Returns `true` if there is at least one undoable command.
    pub fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
    }

    /// Returns `true` if there is at least one redoable command.
    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }

    /// Clears both undo and redo history.
    pub fn clear_history(&mut self) {
        self.undo_stack.clear();
        self.redo_stack.clear();
    }

    /// Returns labels for the undo history, most recent first.
    pub fn undo_history(&self) -> Vec<String> {
        self.undo_stack.iter().rev().map(|c| c.label()).collect()
    }

    /// Returns labels for the redo history, most recent first.
    pub fn redo_history(&self) -> Vec<String> {
        self.redo_stack.iter().rev().map(|c| c.label()).collect()
    }

    /// Undoes multiple commands at once, returning the number actually undone.
    pub fn undo_many(&mut self, count: usize) -> usize {
        let mut undone = 0;
        for _ in 0..count {
            if self.undo().is_ok() {
                undone += 1;
            } else {
                break;
            }
        }
        undone
    }

    /// Redoes multiple commands at once, returning the number actually redone.
    pub fn redo_many(&mut self, count: usize) -> usize {
        let mut redone = 0;
        for _ in 0..count {
            if self.redo().is_ok() {
                redone += 1;
            } else {
                break;
            }
        }
        redone
    }

    /// Executes a command, merging it with the previous command if both are
    /// `SetProperty` on the same node and property.
    ///
    /// This avoids flooding the undo stack when the user drags a slider or
    /// types into a text field — each keystroke would otherwise create a
    /// separate undo entry.
    pub fn merge_or_execute(&mut self, mut command: EditorCommand) -> EditorResult<()> {
        if let EditorCommand::SetProperty {
            node_id,
            ref property,
            ..
        } = command
        {
            if let Some(EditorCommand::SetProperty {
                node_id: prev_node,
                property: ref prev_prop,
                ..
            }) = self.undo_stack.last()
            {
                if node_id == *prev_node && property == prev_prop {
                    // Merge: keep the old_value from the previous command,
                    // apply the new value to the tree, and push directly
                    // (bypassing EditorCommand::execute which would overwrite old_value).
                    let prev = self.undo_stack.pop().unwrap();
                    if let EditorCommand::SetProperty { old_value: prev_old, .. } = prev {
                        if let EditorCommand::SetProperty {
                            node_id,
                            ref property,
                            ref new_value,
                            ref mut old_value,
                        } = command
                        {
                            let node = self.tree.get_node_mut(node_id).ok_or_else(|| {
                                gdcore::error::EngineError::NotFound("node not found".into())
                            })?;
                            node.set_property(property, new_value.clone());
                            *old_value = prev_old;
                        }
                    }
                    self.undo_stack.push(command);
                    self.redo_stack.clear();
                    return Ok(());
                }
            }
        }
        self.execute(command)
    }

    /// Executes a group of commands as a single undo/redo step.
    pub fn execute_group(
        &mut self,
        label: impl Into<String>,
        commands: Vec<EditorCommand>,
    ) -> EditorResult<()> {
        let group = EditorCommand::Group {
            label: label.into(),
            commands,
        };
        self.execute(group)
    }
}

/// Trait for extending the editor with custom behaviour.
///
/// Mirrors Godot's `EditorPlugin` class. Implementations can react to
/// editor events like selection changes.
pub trait EditorPlugin {
    /// Returns the plugin's display name.
    fn name(&self) -> &str;

    /// Called when the plugin is enabled.
    fn on_enable(&mut self) {}

    /// Called when the plugin is disabled.
    fn on_disable(&mut self) {}

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

    // -----------------------------------------------------------------------
    // InstanceScene command tests
    // -----------------------------------------------------------------------

    const INSTANCE_TSCN: &str = r#"
[gd_scene format=3]

[node name="Enemy" type="Node2D"]

[node name="Sprite" type="Sprite2D" parent="."]
position = Vector2(10, 20)
"#;

    #[test]
    fn instance_scene_adds_nodes() {
        let mut editor = make_editor();
        let root = editor.tree().root_id();
        let main_id = editor.tree().get_node(root).unwrap().children()[0];
        let before = editor.tree().node_count();

        editor
            .execute(EditorCommand::InstanceScene {
                parent_id: main_id,
                tscn_source: INSTANCE_TSCN.to_string(),
                created_ids: Vec::new(),
                root_id: None,
            })
            .unwrap();

        // Should have added 2 nodes (Enemy + Sprite).
        assert_eq!(editor.tree().node_count(), before + 2);
    }

    #[test]
    fn instance_scene_returns_root_id() {
        let mut editor = make_editor();
        let root = editor.tree().root_id();
        let main_id = editor.tree().get_node(root).unwrap().children()[0];

        let mut cmd = EditorCommand::InstanceScene {
            parent_id: main_id,
            tscn_source: INSTANCE_TSCN.to_string(),
            created_ids: Vec::new(),
            root_id: None,
        };
        cmd.execute(editor.tree_mut()).unwrap();

        let root_id = match &cmd {
            EditorCommand::InstanceScene { root_id, .. } => root_id.unwrap(),
            _ => unreachable!(),
        };

        let node = editor.tree().get_node(root_id).unwrap();
        assert_eq!(node.name(), "Enemy");
        assert_eq!(node.class_name(), "Node2D");
        assert_eq!(
            node.get_property("_instance_source"),
            Variant::String("instanced".to_string())
        );
    }

    #[test]
    fn instance_scene_undo_removes_nodes() {
        let mut editor = make_editor();
        let root = editor.tree().root_id();
        let main_id = editor.tree().get_node(root).unwrap().children()[0];
        let before = editor.tree().node_count();

        editor
            .execute(EditorCommand::InstanceScene {
                parent_id: main_id,
                tscn_source: INSTANCE_TSCN.to_string(),
                created_ids: Vec::new(),
                root_id: None,
            })
            .unwrap();

        assert_eq!(editor.tree().node_count(), before + 2);

        editor.undo().unwrap();
        assert_eq!(editor.tree().node_count(), before);
    }

    #[test]
    fn instance_scene_redo_restores_nodes() {
        let mut editor = make_editor();
        let root = editor.tree().root_id();
        let main_id = editor.tree().get_node(root).unwrap().children()[0];
        let before = editor.tree().node_count();

        editor
            .execute(EditorCommand::InstanceScene {
                parent_id: main_id,
                tscn_source: INSTANCE_TSCN.to_string(),
                created_ids: Vec::new(),
                root_id: None,
            })
            .unwrap();

        editor.undo().unwrap();
        assert_eq!(editor.tree().node_count(), before);

        editor.redo().unwrap();
        assert_eq!(editor.tree().node_count(), before + 2);
    }

    #[test]
    fn instance_scene_invalid_tscn_fails() {
        let mut editor = make_editor();
        let root = editor.tree().root_id();
        let main_id = editor.tree().get_node(root).unwrap().children()[0];

        let result = editor.execute(EditorCommand::InstanceScene {
            parent_id: main_id,
            tscn_source: "not valid tscn".to_string(),
            created_ids: Vec::new(),
            root_id: None,
        });
        assert!(result.is_err());
    }

    #[test]
    fn instance_scene_hierarchy_correct() {
        let mut editor = make_editor();
        let root = editor.tree().root_id();
        let main_id = editor.tree().get_node(root).unwrap().children()[0];

        let mut cmd = EditorCommand::InstanceScene {
            parent_id: main_id,
            tscn_source: INSTANCE_TSCN.to_string(),
            created_ids: Vec::new(),
            root_id: None,
        };
        cmd.execute(editor.tree_mut()).unwrap();

        let enemy_id = match &cmd {
            EditorCommand::InstanceScene { root_id, .. } => root_id.unwrap(),
            _ => unreachable!(),
        };

        // Enemy should be a child of Main.
        let enemy = editor.tree().get_node(enemy_id).unwrap();
        assert_eq!(enemy.parent(), Some(main_id));

        // Sprite should be a child of Enemy.
        let sprite_id = enemy.children()[0];
        let sprite = editor.tree().get_node(sprite_id).unwrap();
        assert_eq!(sprite.name(), "Sprite");
        assert_eq!(sprite.parent(), Some(enemy_id));
    }

    #[test]
    fn tm_paint_undo() {
        use gdscene::tilemap::{TileGrid, TileGridStore};
        let n = gdscene::node::NodeId::next();
        let mut s = TileGridStore::new_with_defaults();
        s.insert(n, TileGrid::new(10, 10));
        let mut c = EditorCommand::TileMapPaint {
            node_id: n,
            x: 3,
            y: 4,
            tile_id: 1,
            old_tile_id: 0,
        };
        c.execute_tilemap(&mut s).unwrap();
        assert_eq!(s.get(n).unwrap().get(3, 4), Some(1));
        c.undo_tilemap(&mut s).unwrap();
        assert_eq!(s.get(n).unwrap().get(3, 4), Some(0));
    }
    #[test]
    fn tm_fill_undo() {
        use gdscene::tilemap::{TileGrid, TileGridStore};
        let n = gdscene::node::NodeId::next();
        let mut s = TileGridStore::new_with_defaults();
        let mut g = TileGrid::new(10, 10);
        g.set(1, 1, 5);
        s.insert(n, g);
        let mut c = EditorCommand::TileMapFill {
            node_id: n,
            x1: 0,
            y1: 0,
            x2: 2,
            y2: 2,
            tile_id: 2,
            old_tiles: Vec::new(),
        };
        c.execute_tilemap(&mut s).unwrap();
        assert_eq!(s.get(n).unwrap().get(1, 1), Some(2));
        c.undo_tilemap(&mut s).unwrap();
        assert_eq!(s.get(n).unwrap().get(1, 1), Some(5));
    }
    #[test]
    fn tm_resize_undo() {
        use gdscene::tilemap::{TileGrid, TileGridStore};
        let n = gdscene::node::NodeId::next();
        let mut s = TileGridStore::new_with_defaults();
        let mut g = TileGrid::new(5, 5);
        g.set(4, 4, 7);
        s.insert(n, g);
        let mut c = EditorCommand::TileMapResize {
            node_id: n,
            new_width: 10,
            new_height: 10,
            old_width: 0,
            old_height: 0,
            old_cells: Vec::new(),
        };
        c.execute_tilemap(&mut s).unwrap();
        assert_eq!(s.get(n).unwrap().width, 10);
        c.undo_tilemap(&mut s).unwrap();
        assert_eq!(s.get(n).unwrap().width, 5);
        assert_eq!(s.get(n).unwrap().get(4, 4), Some(7));
    }

    // ===== Batch 3: pat-0fa Plugin on_enable/on_disable =====

    struct LifecyclePlugin {
        enabled_count: Rc<Cell<u32>>,
        disabled_count: Rc<Cell<u32>>,
    }

    impl EditorPlugin for LifecyclePlugin {
        fn name(&self) -> &str {
            "LifecyclePlugin"
        }
        fn on_enable(&mut self) {
            self.enabled_count.set(self.enabled_count.get() + 1);
        }
        fn on_disable(&mut self) {
            self.disabled_count.set(self.disabled_count.get() + 1);
        }
    }

    #[test]
    fn plugin_enable_disable_hooks() {
        let mut editor = make_editor();
        let en = Rc::new(Cell::new(0u32));
        let dis = Rc::new(Cell::new(0u32));
        let plugin = LifecyclePlugin {
            enabled_count: en.clone(),
            disabled_count: dis.clone(),
        };
        editor.add_plugin(Box::new(plugin));

        assert!(editor.enable_plugin("LifecyclePlugin"));
        assert_eq!(en.get(), 1);
        assert_eq!(dis.get(), 0);

        assert!(editor.disable_plugin("LifecyclePlugin"));
        assert_eq!(dis.get(), 1);

        // Non-existent plugin returns false
        assert!(!editor.enable_plugin("NoSuchPlugin"));
        assert!(!editor.disable_plugin("NoSuchPlugin"));
    }

    // ===== Undo/redo full parity tests (pat-l8nyi) =====

    #[test]
    fn can_undo_can_redo() {
        let mut editor = make_editor();
        assert!(!editor.can_undo());
        assert!(!editor.can_redo());

        let root = editor.tree().root_id();
        let main_id = editor.tree().get_node(root).unwrap().children()[0];
        editor.execute(EditorCommand::SetProperty {
            node_id: main_id,
            property: "x".into(),
            new_value: Variant::Int(1),
            old_value: Variant::Nil,
        }).unwrap();

        assert!(editor.can_undo());
        assert!(!editor.can_redo());

        editor.undo().unwrap();
        assert!(!editor.can_undo());
        assert!(editor.can_redo());
    }

    #[test]
    fn clear_history() {
        let mut editor = make_editor();
        let root = editor.tree().root_id();
        let main_id = editor.tree().get_node(root).unwrap().children()[0];
        editor.execute(EditorCommand::SetProperty {
            node_id: main_id,
            property: "x".into(),
            new_value: Variant::Int(1),
            old_value: Variant::Nil,
        }).unwrap();
        editor.undo().unwrap();
        assert!(editor.can_redo());

        editor.clear_history();
        assert!(!editor.can_undo());
        assert!(!editor.can_redo());
    }

    #[test]
    fn undo_redo_history_labels() {
        let mut editor = make_editor();
        let root = editor.tree().root_id();
        let main_id = editor.tree().get_node(root).unwrap().children()[0];

        editor.execute(EditorCommand::SetProperty {
            node_id: main_id,
            property: "hp".into(),
            new_value: Variant::Int(10),
            old_value: Variant::Nil,
        }).unwrap();
        editor.execute(EditorCommand::RenameNode {
            node_id: main_id,
            new_name: "Player".into(),
            old_name: String::new(),
        }).unwrap();

        let history = editor.undo_history();
        assert_eq!(history.len(), 2);
        assert!(history[0].contains("Rename"));
        assert!(history[1].contains("Set hp"));

        editor.undo().unwrap();
        let redo_hist = editor.redo_history();
        assert_eq!(redo_hist.len(), 1);
        assert!(redo_hist[0].contains("Rename"));
    }

    #[test]
    fn undo_many_and_redo_many() {
        let mut editor = make_editor();
        let root = editor.tree().root_id();
        let main_id = editor.tree().get_node(root).unwrap().children()[0];

        for i in 0..5 {
            editor.execute(EditorCommand::SetProperty {
                node_id: main_id,
                property: format!("p{i}"),
                new_value: Variant::Int(i),
                old_value: Variant::Nil,
            }).unwrap();
        }

        assert_eq!(editor.undo_depth(), 5);
        let undone = editor.undo_many(3);
        assert_eq!(undone, 3);
        assert_eq!(editor.undo_depth(), 2);
        assert_eq!(editor.redo_depth(), 3);

        let redone = editor.redo_many(10); // more than available
        assert_eq!(redone, 3);
        assert_eq!(editor.undo_depth(), 5);
        assert_eq!(editor.redo_depth(), 0);
    }

    #[test]
    fn merge_or_execute_merges_same_property() {
        let mut editor = make_editor();
        let root = editor.tree().root_id();
        let main_id = editor.tree().get_node(root).unwrap().children()[0];

        // First edit
        editor.merge_or_execute(EditorCommand::SetProperty {
            node_id: main_id,
            property: "speed".into(),
            new_value: Variant::Int(10),
            old_value: Variant::Nil,
        }).unwrap();

        // Second edit on same property — should merge
        editor.merge_or_execute(EditorCommand::SetProperty {
            node_id: main_id,
            property: "speed".into(),
            new_value: Variant::Int(20),
            old_value: Variant::Nil,
        }).unwrap();

        // Only one undo entry
        assert_eq!(editor.undo_depth(), 1);

        // Current value is 20
        assert_eq!(
            editor.tree().get_node(main_id).unwrap().get_property("speed"),
            Variant::Int(20)
        );

        // Undo should go back to original (Nil), not to 10
        editor.undo().unwrap();
        assert_eq!(
            editor.tree().get_node(main_id).unwrap().get_property("speed"),
            Variant::Nil
        );
    }

    #[test]
    fn merge_or_execute_different_property_no_merge() {
        let mut editor = make_editor();
        let root = editor.tree().root_id();
        let main_id = editor.tree().get_node(root).unwrap().children()[0];

        editor.merge_or_execute(EditorCommand::SetProperty {
            node_id: main_id,
            property: "a".into(),
            new_value: Variant::Int(1),
            old_value: Variant::Nil,
        }).unwrap();

        editor.merge_or_execute(EditorCommand::SetProperty {
            node_id: main_id,
            property: "b".into(),
            new_value: Variant::Int(2),
            old_value: Variant::Nil,
        }).unwrap();

        // Different properties — no merge
        assert_eq!(editor.undo_depth(), 2);
    }

    #[test]
    fn move_node_undo_redo() {
        let mut editor = make_editor();
        let root = editor.tree().root_id();

        // Add two more children
        editor.execute(EditorCommand::AddNode {
            parent_id: root,
            name: "Child1".into(),
            class_name: "Node".into(),
            created_id: None,
        }).unwrap();
        editor.execute(EditorCommand::AddNode {
            parent_id: root,
            name: "Child2".into(),
            class_name: "Node".into(),
            created_id: None,
        }).unwrap();

        let children: Vec<_> = editor.tree().get_node(root).unwrap().children().to_vec();
        let last_child = *children.last().unwrap();

        // Move last child to index 0
        editor.execute(EditorCommand::MoveNode {
            parent_id: root,
            child_id: last_child,
            new_index: 0,
            old_index: 0,
        }).unwrap();

        assert_eq!(editor.tree().get_node(root).unwrap().children()[0], last_child);

        // Undo should restore original order
        editor.undo().unwrap();
        let restored = editor.tree().get_node(root).unwrap().children().to_vec();
        assert_eq!(restored, children);
    }

    #[test]
    fn add_to_group_undo_redo() {
        let mut editor = make_editor();
        let root = editor.tree().root_id();
        let main_id = editor.tree().get_node(root).unwrap().children()[0];

        editor.execute(EditorCommand::AddToGroup {
            node_id: main_id,
            group: "enemies".into(),
        }).unwrap();

        assert!(editor.tree().get_node(main_id).unwrap().groups().contains("enemies"));

        editor.undo().unwrap();
        assert!(!editor.tree().get_node(main_id).unwrap().groups().contains("enemies"));

        editor.redo().unwrap();
        assert!(editor.tree().get_node(main_id).unwrap().groups().contains("enemies"));
    }

    #[test]
    fn remove_from_group_undo_redo() {
        let mut editor = make_editor();
        let root = editor.tree().root_id();
        let main_id = editor.tree().get_node(root).unwrap().children()[0];

        // First add to a group directly
        editor.tree_mut().add_to_group(main_id, "players").unwrap();
        assert!(editor.tree().get_node(main_id).unwrap().groups().contains("players"));

        // Then remove via command
        editor.execute(EditorCommand::RemoveFromGroup {
            node_id: main_id,
            group: "players".into(),
        }).unwrap();
        assert!(!editor.tree().get_node(main_id).unwrap().groups().contains("players"));

        // Undo brings it back
        editor.undo().unwrap();
        assert!(editor.tree().get_node(main_id).unwrap().groups().contains("players"));
    }

    #[test]
    fn connect_signal_undo_redo() {
        let mut editor = make_editor();
        let root = editor.tree().root_id();
        let main_id = editor.tree().get_node(root).unwrap().children()[0];
        let target_oid = gdcore::id::ObjectId::next().raw();

        editor.execute(EditorCommand::ConnectSignal {
            source_id: main_id,
            signal_name: "pressed".into(),
            target_object_id: target_oid,
            method: "on_pressed".into(),
        }).unwrap();

        // Signal should be connected
        let store = editor.tree().signal_store(main_id).unwrap();
        let sig = store.get_signal("pressed").unwrap();
        assert_eq!(sig.connection_count(), 1);

        // Undo should disconnect
        editor.undo().unwrap();
        let store = editor.tree().signal_store(main_id).unwrap();
        let sig = store.get_signal("pressed").unwrap();
        assert_eq!(sig.connection_count(), 0);

        // Redo should reconnect
        editor.redo().unwrap();
        let store = editor.tree().signal_store(main_id).unwrap();
        let sig = store.get_signal("pressed").unwrap();
        assert_eq!(sig.connection_count(), 1);
    }

    #[test]
    fn disconnect_signal_undo_redo() {
        let mut editor = make_editor();
        let root = editor.tree().root_id();
        let main_id = editor.tree().get_node(root).unwrap().children()[0];
        let target_oid = gdcore::id::ObjectId::next();

        // Connect directly first
        let conn = gdobject::signal::Connection::new(target_oid, "on_click");
        editor.tree_mut().connect_signal(main_id, "clicked", conn);

        // Disconnect via command
        editor.execute(EditorCommand::DisconnectSignal {
            source_id: main_id,
            signal_name: "clicked".into(),
            target_object_id: target_oid.raw(),
            method: "on_click".into(),
        }).unwrap();

        let store = editor.tree().signal_store(main_id).unwrap();
        let sig = store.get_signal("clicked").unwrap();
        assert_eq!(sig.connection_count(), 0);

        // Undo should reconnect
        editor.undo().unwrap();
        let store = editor.tree().signal_store(main_id).unwrap();
        let sig = store.get_signal("clicked").unwrap();
        assert_eq!(sig.connection_count(), 1);
    }

    #[test]
    fn group_command_undo_redo() {
        let mut editor = make_editor();
        let root = editor.tree().root_id();
        let main_id = editor.tree().get_node(root).unwrap().children()[0];

        let commands = vec![
            EditorCommand::SetProperty {
                node_id: main_id,
                property: "hp".into(),
                new_value: Variant::Int(100),
                old_value: Variant::Nil,
            },
            EditorCommand::SetProperty {
                node_id: main_id,
                property: "mp".into(),
                new_value: Variant::Int(50),
                old_value: Variant::Nil,
            },
        ];

        editor.execute_group("Set stats", commands).unwrap();

        assert_eq!(editor.undo_depth(), 1); // single group entry
        assert_eq!(
            editor.tree().get_node(main_id).unwrap().get_property("hp"),
            Variant::Int(100)
        );
        assert_eq!(
            editor.tree().get_node(main_id).unwrap().get_property("mp"),
            Variant::Int(50)
        );

        // Undo the group — both properties revert
        editor.undo().unwrap();
        assert_eq!(
            editor.tree().get_node(main_id).unwrap().get_property("hp"),
            Variant::Nil
        );
        assert_eq!(
            editor.tree().get_node(main_id).unwrap().get_property("mp"),
            Variant::Nil
        );

        // Redo restores both
        editor.redo().unwrap();
        assert_eq!(
            editor.tree().get_node(main_id).unwrap().get_property("hp"),
            Variant::Int(100)
        );
    }

    #[test]
    fn group_command_label() {
        let cmd = EditorCommand::Group {
            label: "Transform all".into(),
            commands: vec![],
        };
        assert_eq!(cmd.label(), "Transform all");
    }

    #[test]
    fn command_labels() {
        let nid = NodeId::next();
        assert!(EditorCommand::SetProperty {
            node_id: nid,
            property: "pos".into(),
            new_value: Variant::Nil,
            old_value: Variant::Nil,
        }.label().contains("pos"));

        assert!(EditorCommand::AddNode {
            parent_id: nid,
            name: "Foo".into(),
            class_name: "Sprite2D".into(),
            created_id: None,
        }.label().contains("Sprite2D"));

        assert!(EditorCommand::MoveNode {
            parent_id: nid,
            child_id: nid,
            new_index: 0,
            old_index: 0,
        }.label().contains("Move"));

        assert!(EditorCommand::ConnectSignal {
            source_id: nid,
            signal_name: "clicked".into(),
            target_object_id: 0,
            method: "handler".into(),
        }.label().contains("clicked"));
    }

    // -- EditorMode ---------------------------------------------------------

    #[test]
    fn editor_mode_default_is_2d() {
        assert_eq!(EditorMode::default(), EditorMode::Canvas2D);
    }

    #[test]
    fn editor_mode_labels() {
        assert_eq!(EditorMode::Canvas2D.label(), "2D");
        assert_eq!(EditorMode::Spatial3D.label(), "3D");
        assert_eq!(EditorMode::Script.label(), "Script");
        assert_eq!(EditorMode::Game.label(), "Game");
        assert_eq!(EditorMode::AssetLib.label(), "AssetLib");
    }

    #[test]
    fn editor_mode_all_returns_five() {
        assert_eq!(EditorMode::all().len(), 5);
    }

    #[test]
    fn editor_mode_from_str_key() {
        assert_eq!(EditorMode::from_str_key("2d"), Some(EditorMode::Canvas2D));
        assert_eq!(EditorMode::from_str_key("3d"), Some(EditorMode::Spatial3D));
        assert_eq!(EditorMode::from_str_key("script"), Some(EditorMode::Script));
        assert_eq!(EditorMode::from_str_key("game"), Some(EditorMode::Game));
        assert_eq!(EditorMode::from_str_key("assetlib"), Some(EditorMode::AssetLib));
        assert_eq!(EditorMode::from_str_key("invalid"), None);
    }

    #[test]
    fn editor_mode_key_roundtrip() {
        for mode in EditorMode::all() {
            assert_eq!(EditorMode::from_str_key(mode.key()), Some(mode));
        }
    }

    // -- PlayState / RunControls --------------------------------------------

    #[test]
    fn play_state_default_is_stopped() {
        assert_eq!(PlayState::default(), PlayState::Stopped);
    }

    #[test]
    fn run_controls_initial_state() {
        let rc = RunControls::new();
        assert_eq!(rc.state, PlayState::Stopped);
        assert!(!rc.is_running());
        assert!(!rc.is_playing());
        assert!(!rc.is_paused());
    }

    #[test]
    fn run_controls_play() {
        let mut rc = RunControls::new();
        rc.play(RunTarget::MainScene);
        assert!(rc.is_playing());
        assert!(rc.is_running());
        assert!(!rc.is_paused());
        assert!(!rc.last_ran_current);
    }

    #[test]
    fn run_controls_play_current_scene() {
        let mut rc = RunControls::new();
        rc.play(RunTarget::CurrentScene);
        assert!(rc.is_playing());
        assert!(rc.last_ran_current);
    }

    #[test]
    fn run_controls_pause_resume() {
        let mut rc = RunControls::new();
        rc.play(RunTarget::MainScene);
        rc.pause();
        assert!(rc.is_paused());
        assert!(rc.is_running());
        assert!(!rc.is_playing());

        rc.resume();
        assert!(rc.is_playing());
        assert!(!rc.is_paused());
    }

    #[test]
    fn run_controls_toggle_pause() {
        let mut rc = RunControls::new();
        rc.play(RunTarget::MainScene);
        rc.toggle_pause();
        assert!(rc.is_paused());
        rc.toggle_pause();
        assert!(rc.is_playing());
    }

    #[test]
    fn run_controls_stop() {
        let mut rc = RunControls::new();
        rc.play(RunTarget::MainScene);
        rc.stop();
        assert!(!rc.is_running());
        assert_eq!(rc.state, PlayState::Stopped);
    }

    #[test]
    fn run_controls_pause_when_stopped_is_noop() {
        let mut rc = RunControls::new();
        rc.pause();
        assert_eq!(rc.state, PlayState::Stopped);
    }

    #[test]
    fn run_controls_resume_when_stopped_is_noop() {
        let mut rc = RunControls::new();
        rc.resume();
        assert_eq!(rc.state, PlayState::Stopped);
    }

    #[test]
    fn run_controls_custom_scene() {
        let mut rc = RunControls::new();
        rc.play(RunTarget::CustomScene("res://levels/boss.tscn".into()));
        assert!(rc.is_playing());
        assert!(!rc.last_ran_current);
        assert_eq!(rc.target, RunTarget::CustomScene("res://levels/boss.tscn".into()));
    }
}
