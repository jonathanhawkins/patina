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

pub mod anim_interpolation;
pub mod anim_keyframes;
pub mod anim_playback;
pub mod anim_player_panel;
pub mod anim_track_types;
pub mod anim_tree_graph;
pub mod animation_editor;
pub mod asset_drag_drop;
pub mod audio_bus_layout;
pub mod auto_indent;
pub mod autoload_manager;
pub mod bezier_curve;
pub mod bottom_panel_bar;
pub mod bracket_match;
pub mod canvas_overlays;
pub mod code_folding;
pub mod command_palette;
pub mod comment_toggle;
pub mod control_binding;
pub mod create_dialog;
pub mod create_dialog_confirm;
pub mod create_node_catalog;
pub mod create_node_description;
pub mod create_node_insertion;
pub mod create_node_recent;
pub mod curve_editor;
pub mod debug_menu_toggles;
pub mod debugger_errors;
pub mod debugger_profiler;
pub mod debugger_stack;
pub mod dock;
pub mod document_history;
pub mod editor_compat;
pub mod editor_interface;
pub mod editor_menu;
pub mod editor_menu_commands;
pub mod editor_plugin;
pub mod editor_server;
pub mod editor_settings_dialog;
pub mod editor_settings_store;
pub mod editor_ui;
pub mod environment_preview;
pub mod export_dialog;
pub mod filesystem;
pub mod find_in_files;
pub mod find_replace;
pub mod fs_file_ops;
pub mod fs_import_pipeline;
pub mod fs_thumbnails;
pub mod gdscript_highlight;
pub mod group_dialog;
pub mod import;
pub mod import_settings;
pub mod export_groups;
pub mod export_layout;
pub mod export_presets;
pub mod gizmo_multi;
pub mod gizmo_snap;
pub mod goto_definition;
pub mod goto_line;
pub mod grid_display;
pub mod grid_snapping;
pub mod guides;
pub mod help_menu_commands;
pub mod indent_block;
pub mod input_map;
pub mod inspector;
pub mod line_ops;
pub mod linked_proportional;
pub mod main_screen;
pub mod menu_enablement;
pub mod menu_shortcuts;
pub mod mode_placeholder;
pub mod monitors_panel;
pub mod multi_caret;
pub mod multi_document;
pub mod multi_node_edit;
pub mod new_scene;
pub mod numeric_expression;
pub mod open_recent;
pub mod open_scene;
pub mod open_scripts_panel;
pub mod pivot_marker;
pub mod property_undo;
pub mod subresource_inline;
pub mod viewport_select;
pub mod viewport_toolbar;
pub mod output_console;
pub mod output_panel;
pub mod play_custom_scene;
pub mod plugin_manager;
pub mod profiler_panel;
pub mod project_menu_commands;
pub mod project_settings_dialog;
pub mod project_settings_store;
pub mod save_normalize;
pub mod scene_editor;
pub mod scene_menu_commands;
pub mod scene_renderer;
pub mod scene_tab_bar;
pub mod scene_tabs;
pub mod script_bookmarks;
pub mod script_breakpoints;
pub mod script_completion;
pub mod script_editor;
pub mod script_find_replace;
pub mod script_gutter;
pub mod script_main_view;
pub mod script_outline;
pub mod settings;
pub mod shader_editor;
pub mod snap_config;
pub mod signal_connect_advanced;
pub mod signal_connect_dialog;
pub mod signal_connection_edit;
pub mod signal_dialog;
pub mod signal_docs;
pub mod signal_goto_method;
pub mod signal_receiver_stub;
pub mod signals_tree;
pub mod texture_cache;
pub mod theme_editor;
pub mod tilemap_editor;
pub mod undo_redo;
pub mod variant_value;
pub mod vcs;
pub mod vcs_integration;
pub mod viewport_2d;
pub mod viewport_2d_render;
pub mod viewport_3d;
pub mod viewport_3d_gizmo;
pub mod viewport_3d_overlay;
pub mod viewport_3d_render;
pub mod viewport_3d_select;

use gdscene::node::{Node, NodeId};
use gdscene::SceneTree;
use gdvariant::Variant;
use thiserror::Error;

/// pat-ism62: Identifier for the active viewport rasterizer backend.
///
/// The default editor build links wgpu and returns `"wgpu"`. Compiling with
/// `--no-default-features --features software-render` selects the legacy
/// CPU rasterizer (and drops wgpu from the dependency graph) and returns
/// `"software"`. Tests assert this constant to prove that the default
/// editor build is GPU-backed.
pub fn viewport_backend() -> &'static str {
    if cfg!(feature = "gpu-render") {
        "wgpu"
    } else if cfg!(feature = "software-render") {
        "software"
    } else {
        // Defensive: both features off should be unreachable because the
        // crate has `default = ["gpu-render"]`. Returning "software" keeps
        // the editor functional rather than panicking in a build that
        // managed to disable every backend.
        "software"
    }
}

// Re-exports for convenience.
pub use create_dialog::{
    CatalogEntry, ClassEntry, ClassFilter, CreateDialogResult, CreateNodeDialog, HelperPreset,
    NodeCatalog2D, NodeCategory,
};
pub use dock::{
    DockPanel, NodeIndicators, NodeTypeIcon, NodeWarning, PluginDockManager, PluginDockPanel,
    PropertyDock, SceneTreeDock, SelectionState,
};
pub use editor_interface::EditorInterface;
pub use editor_settings_dialog::{EditorSettingsDialog, KeyBinding, PluginInfo, SettingsTab};
pub use export_dialog::{ExportBuildProfile, ExportDialog, ExportPlatform, ExportPreset};
pub use filesystem::EditorFileSystem;
pub use find_replace::{
    FindReplace as FindReplaceEngine, FindReplaceConfig, FindReplaceError, ReplaceResult,
    SearchMatch, SearchMode,
};
pub use import::{
    AnimationLoopMode, EditorSceneFormatImporter, EditorScenePostImport, FbxSceneImporter,
    GltfSceneImporter, ImportPipeline, ImportedAnimation, ImportedResource, ImportedScene,
    ImportedSceneNode, ObjSceneImporter, ResourceImporter, SceneFormatImporterRegistry,
    SceneImportOptions, TresImporter, TscnImporter,
};
pub use inspector::{
    coerce_variant, validate_variant, CustomPropertyEditor, EditorHint, EditorInspectorPlugin,
    InspectorPanel, InspectorPluginRegistry, InspectorSection, PropertyEditor, PropertyHint,
    ResourceSubEditor, SectionedInspector,
};
pub use profiler_panel::{
    FrameProfile, FunctionStats, ProfilerEntry, ProfilerPanel, ProfilerStats,
};
pub use project_settings_dialog::{
    ProjectSettingsDialog, SettingsCategory, SettingsEditor, SettingsProperty, SettingsValue,
};
pub use scene_editor::SceneEditor;
pub use script_editor::{FindMatch, FindOptions, FindReplace, ScriptEditor};
pub use settings::{EditorSettings, EditorTheme, ProjectSettings};
pub use shader_editor::{
    MaterialPreview, PreviewShape, PreviewUniformInfo, ShaderCompileStatus, ShaderEditor,
    ShaderHighlightKind, ShaderHighlightSpan, ShaderHighlighter, ShaderTab, UniformValue,
};
pub use theme_editor::{
    OverrideEntry, OverrideKind, PreviewControl, StyleBoxFlat, ThemeColorPalette, ThemeEditor,
    ThemeFont, ThemeItem, ThemeResource,
};
pub use vcs::{BranchInfo, ChangeArea, CommitEntry, FileChangeStatus, VcsFileStatus, VcsStatus};
pub use viewport_3d::{
    CameraMode, EnvironmentPreview3D, GizmoAxis, GizmoMode3D, Grid3D, GridLine, PickResult,
    Projection, Ray3D, Selection3D, Viewport3D, ViewportCamera3D,
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

    /// The target node has no attached script to open.
    #[error("node has no attached script")]
    NoScript,

    /// The target node is not an instanced scene with an openable source.
    #[error("node is not an instanced scene with an openable source")]
    NotSceneInstance,
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

    /// Plays the project's main scene (Godot's Run Project / F5).
    pub fn play_project(&mut self) {
        self.play(RunTarget::MainScene);
    }

    /// Plays the currently edited scene (Godot's Run Current Scene / F6).
    pub fn play_current_scene(&mut self) {
        self.play(RunTarget::CurrentScene);
    }

    /// Resolves the concrete scene path of the running instance, given the
    /// project's `main_scene` and the editor's `current_scene`. Returns the
    /// path that is (or, when paused, still) running, or `None` when stopped —
    /// so callers know exactly which scene to launch or terminate.
    pub fn running_scene(&self, main_scene: &str, current_scene: &str) -> Option<String> {
        if self.state == PlayState::Stopped {
            return None;
        }
        Some(match &self.target {
            RunTarget::MainScene => main_scene.to_string(),
            RunTarget::CurrentScene => current_scene.to_string(),
            RunTarget::CustomScene(path) => path.clone(),
        })
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
        /// When true, the node's local transform is adjusted after reparenting
        /// so its global (on-screen) transform is preserved.
        keep_transform: bool,
        /// The node's local (position, rotation, scale) captured before a
        /// keep-transform reparent, so undo can restore it.
        saved_transform: Option<(gdcore::math::Vector2, f32, gdcore::math::Vector2)>,
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
        /// The originating `.tscn` path recorded on the instanced root for the
        /// scene-tree instance indicator. `None` for in-memory sources (which
        /// fall back to a generic "instanced" marker).
        source_path: Option<String>,
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
                keep_transform,
                saved_transform,
            } => {
                use gdscene::node2d;
                *old_parent_id = tree.get_node(*node_id).and_then(|n| n.parent());
                if *keep_transform {
                    // Capture the local transform for undo, and the global
                    // transform to re-establish under the new parent.
                    *saved_transform = Some((
                        node2d::get_position(tree, *node_id),
                        node2d::get_rotation(tree, *node_id),
                        node2d::get_scale(tree, *node_id),
                    ));
                    let old_global = node2d::get_global_transform(tree, *node_id);
                    tree.reparent(*node_id, *new_parent_id)?;
                    // new_local = inverse(new_parent_global) * old_global keeps
                    // the node's global transform unchanged.
                    let parent_global = node2d::get_global_transform(tree, *new_parent_id);
                    let new_local = parent_global.affine_inverse() * old_global;
                    node2d::set_position(tree, *node_id, new_local.origin);
                    node2d::set_rotation(tree, *node_id, new_local.x.y.atan2(new_local.x.x));
                    node2d::set_scale(
                        tree,
                        *node_id,
                        gdcore::math::Vector2::new(new_local.x.length(), new_local.y.length()),
                    );
                } else {
                    tree.reparent(*node_id, *new_parent_id)?;
                }
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

                // Generate a name not already used by `siblings`.
                fn unique_sibling_name(base: &str, siblings: &[String]) -> String {
                    if !siblings.iter().any(|s| s == base) {
                        return base.to_string();
                    }
                    let stem = base.trim_end_matches(|c: char| c.is_ascii_digit());
                    let stem = if stem.is_empty() { base } else { stem };
                    let mut n = 2;
                    loop {
                        let candidate = format!("{stem}{n}");
                        if !siblings.iter().any(|s| s == &candidate) {
                            return candidate;
                        }
                        n += 1;
                    }
                }

                created_ids.clear();
                let new_top = duplicate_subtree(tree, *source_id, parent_id, created_ids)?;

                // Give the top-level duplicate a unique name among its siblings.
                let unique = {
                    let siblings: Vec<String> = tree
                        .get_node(parent_id)
                        .map(|p| {
                            p.children()
                                .iter()
                                .filter(|&&c| c != new_top)
                                .filter_map(|&c| tree.get_node(c).map(|n| n.name().to_string()))
                                .collect()
                        })
                        .unwrap_or_default();
                    let base = tree
                        .get_node(new_top)
                        .map(|n| n.name().to_string())
                        .unwrap_or_default();
                    unique_sibling_name(&base, &siblings)
                };
                if let Some(node) = tree.get_node_mut(new_top) {
                    node.set_name(unique);
                }

                // Insert the duplicate immediately after the source (next sibling).
                if let Some(src_idx) = tree
                    .get_node(parent_id)
                    .and_then(|p| p.children().iter().position(|&c| c == *source_id))
                {
                    let _ = tree.move_child(parent_id, new_top, src_idx + 1);
                }

                tracing::debug!("DuplicateNode {:?} -> {:?}", source_id, created_ids);
                Ok(())
            }
            EditorCommand::InstanceScene {
                parent_id,
                tscn_source,
                created_ids,
                root_id,
                source_path,
            } => {
                use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
                let packed = PackedScene::from_tscn(tscn_source).map_err(|e| {
                    gdcore::error::EngineError::InvalidOperation(format!(
                        "failed to parse tscn: {e}"
                    ))
                })?;
                let scene_root = add_packed_scene_to_tree(tree, *parent_id, &packed)?;
                *root_id = Some(scene_root);

                // Record the originating scene path on the instanced root so
                // the scene-tree instance indicator can show where the
                // instance came from. In-memory sources without a path fall
                // back to a generic marker.
                if let Some(node) = tree.get_node_mut(scene_root) {
                    let marker = source_path
                        .clone()
                        .unwrap_or_else(|| "instanced".to_string());
                    node.set_property("_instance_source", Variant::String(marker));
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
                let parent = tree.get_node(*parent_id).ok_or_else(|| {
                    gdcore::error::EngineError::NotFound("parent not found".into())
                })?;
                let children = parent.children();
                *old_index = children.iter().position(|&c| c == *child_id).unwrap_or(0);
                tree.move_child(*parent_id, *child_id, *new_index)?;
                tracing::debug!(
                    "MoveNode {:?} index {} -> {}",
                    child_id,
                    old_index,
                    new_index
                );
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
                    source_id,
                    signal_name,
                    target_object_id,
                    method
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
                store.disconnect(
                    signal_name,
                    gdcore::id::ObjectId::from_raw(*target_object_id),
                    method,
                );
                tracing::debug!(
                    "DisconnectSignal {:?}.{} -> {}::{}",
                    source_id,
                    signal_name,
                    target_object_id,
                    method
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
                saved_transform,
                ..
            } => {
                if let Some(old_pid) = old_parent_id {
                    tree.reparent(*node_id, *old_pid)?;
                }
                // Restore the local transform captured before a keep-transform
                // reparent so undo is an exact inverse.
                if let Some((pos, rot, scl)) = saved_transform {
                    use gdscene::node2d;
                    node2d::set_position(tree, *node_id, *pos);
                    node2d::set_rotation(tree, *node_id, *rot);
                    node2d::set_scale(tree, *node_id, *scl);
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
                store.disconnect(
                    signal_name,
                    gdcore::id::ObjectId::from_raw(*target_object_id),
                    method,
                );
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
            EditorCommand::AddNode {
                name, class_name, ..
            } => format!("Add {class_name} '{name}'"),
            EditorCommand::RemoveNode { name, .. } => format!("Remove '{name}'"),
            EditorCommand::ReparentNode { .. } => "Reparent node".into(),
            EditorCommand::RenameNode {
                old_name, new_name, ..
            } => {
                format!("Rename '{old_name}' -> '{new_name}'")
            }
            EditorCommand::DuplicateNode { .. } => "Duplicate node".into(),
            EditorCommand::InstanceScene { .. } => "Instance scene".into(),
            EditorCommand::TileMapPaint { .. } => "Paint tile".into(),
            EditorCommand::TileMapFill { .. } => "Fill tiles".into(),
            EditorCommand::TileMapResize { .. } => "Resize tilemap".into(),
            EditorCommand::MoveNode { .. } => "Move node".into(),
            EditorCommand::ConnectSignal {
                signal_name,
                method,
                ..
            } => {
                format!("Connect {signal_name} -> {method}")
            }
            EditorCommand::DisconnectSignal {
                signal_name,
                method,
                ..
            } => {
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
    /// The current selection set, in selection order. This is the single
    /// source of truth shared by the dock, viewport, and inspector; the last
    /// entry is the active/primary selection. Empty when nothing is selected.
    selection: Vec<NodeId>,
    /// When `Some`, the inspector is pinned/locked to this node: it keeps that
    /// object inspected even as the scene-tree selection changes. `None` means
    /// the inspector follows the active selection. See [`Editor::pin_inspector`].
    inspector_pin: Option<NodeId>,
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
            .field("selection", &self.selection)
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
            selection: Vec::new(),
            inspector_pin: None,
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

    /// Selects a single node, replacing any existing selection (a plain click
    /// in the dock or viewport).
    pub fn select_node(&mut self, id: NodeId) {
        self.selection = vec![id];
        tracing::debug!("Selected node {:?}", id);
    }

    /// Clears the entire selection.
    pub fn deselect(&mut self) {
        self.selection.clear();
    }

    /// Returns the active/primary selected node — the most recently added
    /// entry in the selection set — or `None` when nothing is selected. This
    /// is what the inspector header and single-node operations act on.
    pub fn selected_node(&self) -> Option<NodeId> {
        self.selection.last().copied()
    }

    /// Returns the full selection set in selection order. This is the single
    /// source of truth the dock, viewport, and inspector all read, so their
    /// highlighting stays synchronized.
    pub fn selected_nodes(&self) -> &[NodeId] {
        &self.selection
    }

    /// Returns `true` if `id` is part of the current selection.
    pub fn is_selected(&self, id: NodeId) -> bool {
        self.selection.contains(&id)
    }

    /// Replaces the selection with the given set, preserving order and dropping
    /// duplicates (keeping first occurrence).
    pub fn select_nodes(&mut self, ids: impl IntoIterator<Item = NodeId>) {
        let mut next = Vec::new();
        for id in ids {
            if !next.contains(&id) {
                next.push(id);
            }
        }
        self.selection = next;
    }

    /// Toggles `id` in the selection (ctrl/cmd-click additive semantics):
    /// removes it if already selected, otherwise appends it as the new active
    /// selection. Returns `true` if `id` ended up selected.
    pub fn toggle_select(&mut self, id: NodeId) -> bool {
        if let Some(pos) = self.selection.iter().position(|&n| n == id) {
            self.selection.remove(pos);
            false
        } else {
            self.selection.push(id);
            true
        }
    }

    /// Selects the contiguous range between the current active selection (the
    /// anchor) and `target` within `order` (shift-click range semantics). The
    /// range replaces the selection. `order` is the visible row ordering (e.g.
    /// [`SceneTree::all_nodes_in_tree_order`]). When there is no anchor, or
    /// either endpoint is absent from `order`, falls back to selecting just
    /// `target`.
    pub fn select_range(&mut self, target: NodeId, order: &[NodeId]) {
        let anchor = match self.selected_node() {
            Some(a) => a,
            None => {
                self.select_node(target);
                return;
            }
        };
        let ai = order.iter().position(|&n| n == anchor);
        let ti = order.iter().position(|&n| n == target);
        match (ai, ti) {
            (Some(a), Some(t)) => {
                let (lo, hi) = if a <= t { (a, t) } else { (t, a) };
                // Keep the range ordered so the anchor stays first and `target`
                // becomes the active (last) selection.
                let mut range: Vec<NodeId> = order[lo..=hi].to_vec();
                if a > t {
                    range.reverse();
                }
                self.selection = range;
            }
            _ => self.select_node(target),
        }
    }

    /// Returns the node the inspector is currently showing. When the inspector
    /// is pinned (see [`Editor::pin_inspector`]) this is the pinned node and
    /// stays fixed as the scene-tree selection changes; otherwise it follows
    /// the active selection ([`Editor::selected_node`]).
    pub fn inspected_node(&self) -> Option<NodeId> {
        match self.inspector_pin {
            Some(id) => Some(id),
            None => self.selected_node(),
        }
    }

    /// Pins/locks the inspector to the currently inspected object so it keeps
    /// showing that object even as the scene-tree selection moves elsewhere.
    /// No-op (leaves the inspector unpinned) when nothing is currently
    /// inspected. Re-pinning while already pinned re-captures the current
    /// selection as the pin target.
    pub fn pin_inspector(&mut self) {
        // Pin the object the inspector is showing right now. When already
        // pinned this is the pinned node, so the pin stays put; when following
        // selection it captures the active node.
        self.inspector_pin = self.inspected_node();
    }

    /// Unpins the inspector so it resumes following the scene-tree selection.
    pub fn unpin_inspector(&mut self) {
        self.inspector_pin = None;
    }

    /// Whether the inspector is currently pinned/locked to a specific object.
    pub fn is_inspector_pinned(&self) -> bool {
        self.inspector_pin.is_some()
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
        let selected = self.selected_node();
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
                    if let EditorCommand::SetProperty {
                        old_value: prev_old,
                        ..
                    } = prev
                    {
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
    fn inspector_pin_lock_holds_selection() {
        // The pin/lock toggle keeps the current object inspected even as the
        // scene-tree selection changes; unpinning resumes following selection.
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let a = tree.add_child(root, Node::new("A", "Node2D")).unwrap();
        let b = tree.add_child(root, Node::new("B", "Node2D")).unwrap();
        let mut editor = Editor::new(tree);

        // Unpinned, the inspector follows the active selection.
        editor.select_node(a);
        assert!(!editor.is_inspector_pinned());
        assert_eq!(editor.inspected_node(), Some(a));

        // Pinning locks the inspector to A.
        editor.pin_inspector();
        assert!(editor.is_inspector_pinned());

        // Selecting B in the scene tree moves the selection but the pinned
        // inspector keeps showing A.
        editor.select_node(b);
        assert_eq!(editor.selected_node(), Some(b), "tree selection follows to B");
        assert_eq!(
            editor.inspected_node(),
            Some(a),
            "pinned inspector keeps showing A"
        );

        // Unpinning resumes following the selection, so the inspector now
        // shows B.
        editor.unpin_inspector();
        assert!(!editor.is_inspector_pinned());
        assert_eq!(
            editor.inspected_node(),
            Some(b),
            "unpinning resumes following selection"
        );
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
                keep_transform: false,
                saved_transform: None,
            })
            .unwrap();

        assert_eq!(editor.tree().get_node(c_id).unwrap().parent(), Some(b_id));

        // Undo.
        editor.undo().unwrap();
        assert_eq!(editor.tree().get_node(c_id).unwrap().parent(), Some(a_id));
    }

    /// Unit-level coverage for the "Reparent" scene-tree operation
    /// (bead scene-tree-ops-reparent-node / pat-94ac2): reparenting must
    /// (1) move the node and its whole subtree under the target parent,
    /// (2) reject reparenting a node into its own descendant, and
    /// (3) preserve the node's global transform when requested.
    ///
    /// Named distinctly from the crate's HTTP-level `scene_tree_reparent_node`
    /// acceptance test (in `editor_server`) so a `-p gdeditor
    /// scene_tree_reparent_node` filter resolves to exactly one test.
    #[test]
    fn reparent_command_subtree_cycle_and_keep_transform() {
        use gdcore::math::Vector2;
        use gdscene::node2d;

        let mut editor = make_editor();
        let root = editor.tree().root_id();
        let main_id = editor.tree().get_node(root).unwrap().children()[0];

        // Build: Main -> A(100,50) -> C ;  Main -> B(10,20)
        let a = editor
            .tree_mut()
            .add_child(main_id, Node::new("A", "Node2D"))
            .unwrap();
        let c = editor
            .tree_mut()
            .add_child(a, Node::new("C", "Node2D"))
            .unwrap();
        let b = editor
            .tree_mut()
            .add_child(main_id, Node::new("B", "Node2D"))
            .unwrap();
        node2d::set_position(editor.tree_mut(), a, Vector2::new(100.0, 50.0));
        node2d::set_position(editor.tree_mut(), b, Vector2::new(10.0, 20.0));

        // (1) Subtree preservation: reparent A under B; C stays A's child.
        editor
            .execute(EditorCommand::ReparentNode {
                node_id: a,
                new_parent_id: b,
                old_parent_id: None,
                keep_transform: false,
                saved_transform: None,
            })
            .unwrap();
        assert_eq!(
            editor.tree().get_node(a).unwrap().parent(),
            Some(b),
            "A is reparented under B"
        );
        assert!(
            editor.tree().get_node(b).unwrap().children().contains(&a),
            "A is a child of B"
        );
        assert_eq!(
            editor.tree().get_node(c).unwrap().parent(),
            Some(a),
            "C subtree is preserved under A"
        );

        // (2) Reject reparenting a node into its own descendant.
        let rejected = editor.execute(EditorCommand::ReparentNode {
            node_id: a,
            new_parent_id: c,
            old_parent_id: None,
            keep_transform: false,
            saved_transform: None,
        });
        assert!(
            rejected.is_err(),
            "reparenting A into its descendant C must be rejected"
        );
        assert_eq!(
            editor.tree().get_node(a).unwrap().parent(),
            Some(b),
            "A is unchanged after the rejected reparent"
        );

        // (3) Preserve global transform when requested. Move A back under Main
        // with a known position, then reparent under B(10,20) keeping its
        // global transform.
        editor
            .execute(EditorCommand::ReparentNode {
                node_id: a,
                new_parent_id: main_id,
                old_parent_id: None,
                keep_transform: false,
                saved_transform: None,
            })
            .unwrap();
        node2d::set_position(editor.tree_mut(), a, Vector2::new(100.0, 50.0));
        let global_before = node2d::get_global_transform(editor.tree(), a).origin;

        editor
            .execute(EditorCommand::ReparentNode {
                node_id: a,
                new_parent_id: b,
                old_parent_id: None,
                keep_transform: true,
                saved_transform: None,
            })
            .unwrap();

        let global_after = node2d::get_global_transform(editor.tree(), a).origin;
        assert!(
            (global_after.x - global_before.x).abs() < 1e-3
                && (global_after.y - global_before.y).abs() < 1e-3,
            "keep_transform preserves A's global position: before {global_before:?} after {global_after:?}"
        );
        // A's local position is now offset by B's position (100-10, 50-20).
        let local_a = node2d::get_position(editor.tree(), a);
        assert!(
            (local_a.x - 90.0).abs() < 1e-3 && (local_a.y - 30.0).abs() < 1e-3,
            "A local position is adjusted relative to B: {local_a:?}"
        );

        // Undo of the keep-transform reparent restores both parent and local
        // position.
        editor.undo().unwrap();
        assert_eq!(
            editor.tree().get_node(a).unwrap().parent(),
            Some(main_id),
            "undo restores A under Main"
        );
        let restored = node2d::get_position(editor.tree(), a);
        assert!(
            (restored.x - 100.0).abs() < 1e-3 && (restored.y - 50.0).abs() < 1e-3,
            "undo restores A's local position: {restored:?}"
        );
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
                source_path: None,
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
            source_path: None,
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
                source_path: None,
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
                source_path: None,
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
            source_path: None,
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
            source_path: None,
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
        editor
            .execute(EditorCommand::SetProperty {
                node_id: main_id,
                property: "x".into(),
                new_value: Variant::Int(1),
                old_value: Variant::Nil,
            })
            .unwrap();

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
        editor
            .execute(EditorCommand::SetProperty {
                node_id: main_id,
                property: "x".into(),
                new_value: Variant::Int(1),
                old_value: Variant::Nil,
            })
            .unwrap();
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

        editor
            .execute(EditorCommand::SetProperty {
                node_id: main_id,
                property: "hp".into(),
                new_value: Variant::Int(10),
                old_value: Variant::Nil,
            })
            .unwrap();
        editor
            .execute(EditorCommand::RenameNode {
                node_id: main_id,
                new_name: "Player".into(),
                old_name: String::new(),
            })
            .unwrap();

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
            editor
                .execute(EditorCommand::SetProperty {
                    node_id: main_id,
                    property: format!("p{i}"),
                    new_value: Variant::Int(i),
                    old_value: Variant::Nil,
                })
                .unwrap();
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
        editor
            .merge_or_execute(EditorCommand::SetProperty {
                node_id: main_id,
                property: "speed".into(),
                new_value: Variant::Int(10),
                old_value: Variant::Nil,
            })
            .unwrap();

        // Second edit on same property — should merge
        editor
            .merge_or_execute(EditorCommand::SetProperty {
                node_id: main_id,
                property: "speed".into(),
                new_value: Variant::Int(20),
                old_value: Variant::Nil,
            })
            .unwrap();

        // Only one undo entry
        assert_eq!(editor.undo_depth(), 1);

        // Current value is 20
        assert_eq!(
            editor
                .tree()
                .get_node(main_id)
                .unwrap()
                .get_property("speed"),
            Variant::Int(20)
        );

        // Undo should go back to original (Nil), not to 10
        editor.undo().unwrap();
        assert_eq!(
            editor
                .tree()
                .get_node(main_id)
                .unwrap()
                .get_property("speed"),
            Variant::Nil
        );
    }

    #[test]
    fn merge_or_execute_different_property_no_merge() {
        let mut editor = make_editor();
        let root = editor.tree().root_id();
        let main_id = editor.tree().get_node(root).unwrap().children()[0];

        editor
            .merge_or_execute(EditorCommand::SetProperty {
                node_id: main_id,
                property: "a".into(),
                new_value: Variant::Int(1),
                old_value: Variant::Nil,
            })
            .unwrap();

        editor
            .merge_or_execute(EditorCommand::SetProperty {
                node_id: main_id,
                property: "b".into(),
                new_value: Variant::Int(2),
                old_value: Variant::Nil,
            })
            .unwrap();

        // Different properties — no merge
        assert_eq!(editor.undo_depth(), 2);
    }

    #[test]
    fn move_node_undo_redo() {
        let mut editor = make_editor();
        let root = editor.tree().root_id();

        // Add two more children
        editor
            .execute(EditorCommand::AddNode {
                parent_id: root,
                name: "Child1".into(),
                class_name: "Node".into(),
                created_id: None,
            })
            .unwrap();
        editor
            .execute(EditorCommand::AddNode {
                parent_id: root,
                name: "Child2".into(),
                class_name: "Node".into(),
                created_id: None,
            })
            .unwrap();

        let children: Vec<_> = editor.tree().get_node(root).unwrap().children().to_vec();
        let last_child = *children.last().unwrap();

        // Move last child to index 0
        editor
            .execute(EditorCommand::MoveNode {
                parent_id: root,
                child_id: last_child,
                new_index: 0,
                old_index: 0,
            })
            .unwrap();

        assert_eq!(
            editor.tree().get_node(root).unwrap().children()[0],
            last_child
        );

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

        editor
            .execute(EditorCommand::AddToGroup {
                node_id: main_id,
                group: "enemies".into(),
            })
            .unwrap();

        assert!(editor
            .tree()
            .get_node(main_id)
            .unwrap()
            .groups()
            .contains("enemies"));

        editor.undo().unwrap();
        assert!(!editor
            .tree()
            .get_node(main_id)
            .unwrap()
            .groups()
            .contains("enemies"));

        editor.redo().unwrap();
        assert!(editor
            .tree()
            .get_node(main_id)
            .unwrap()
            .groups()
            .contains("enemies"));
    }

    #[test]
    fn remove_from_group_undo_redo() {
        let mut editor = make_editor();
        let root = editor.tree().root_id();
        let main_id = editor.tree().get_node(root).unwrap().children()[0];

        // First add to a group directly
        editor.tree_mut().add_to_group(main_id, "players").unwrap();
        assert!(editor
            .tree()
            .get_node(main_id)
            .unwrap()
            .groups()
            .contains("players"));

        // Then remove via command
        editor
            .execute(EditorCommand::RemoveFromGroup {
                node_id: main_id,
                group: "players".into(),
            })
            .unwrap();
        assert!(!editor
            .tree()
            .get_node(main_id)
            .unwrap()
            .groups()
            .contains("players"));

        // Undo brings it back
        editor.undo().unwrap();
        assert!(editor
            .tree()
            .get_node(main_id)
            .unwrap()
            .groups()
            .contains("players"));
    }

    #[test]
    fn connect_signal_undo_redo() {
        let mut editor = make_editor();
        let root = editor.tree().root_id();
        let main_id = editor.tree().get_node(root).unwrap().children()[0];
        let target_oid = gdcore::id::ObjectId::next().raw();

        editor
            .execute(EditorCommand::ConnectSignal {
                source_id: main_id,
                signal_name: "pressed".into(),
                target_object_id: target_oid,
                method: "on_pressed".into(),
            })
            .unwrap();

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
        editor
            .execute(EditorCommand::DisconnectSignal {
                source_id: main_id,
                signal_name: "clicked".into(),
                target_object_id: target_oid.raw(),
                method: "on_click".into(),
            })
            .unwrap();

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
        }
        .label()
        .contains("pos"));

        assert!(EditorCommand::AddNode {
            parent_id: nid,
            name: "Foo".into(),
            class_name: "Sprite2D".into(),
            created_id: None,
        }
        .label()
        .contains("Sprite2D"));

        assert!(EditorCommand::MoveNode {
            parent_id: nid,
            child_id: nid,
            new_index: 0,
            old_index: 0,
        }
        .label()
        .contains("Move"));

        assert!(EditorCommand::ConnectSignal {
            source_id: nid,
            signal_name: "clicked".into(),
            target_object_id: 0,
            method: "handler".into(),
        }
        .label()
        .contains("clicked"));
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
        assert_eq!(
            EditorMode::from_str_key("assetlib"),
            Some(EditorMode::AssetLib)
        );
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
        assert_eq!(
            rc.target,
            RunTarget::CustomScene("res://levels/boss.tscn".into())
        );
    }

    /// Acceptance (pat-twbea): the top-bar run controls play the project (main
    /// scene), play the current scene, pause (toggle), and stop (terminate the
    /// running instance).
    #[test]
    fn top_bar_run_controls() {
        let main = "res://Main.tscn";
        let current = "res://levels/Level2.tscn";
        let mut rc = RunControls::new();

        // Nothing runs until a run control is pressed.
        assert!(!rc.is_running());
        assert_eq!(rc.running_scene(main, current), None);

        // Play-project (F5) launches the project's main scene.
        rc.play_project();
        assert!(rc.is_playing());
        assert!(!rc.last_ran_current);
        assert_eq!(
            rc.running_scene(main, current).as_deref(),
            Some(main),
            "play-project launches the main scene"
        );

        // Stop terminates the running instance before switching targets.
        rc.stop();
        assert!(!rc.is_running());
        assert_eq!(rc.running_scene(main, current), None);

        // Play-scene (F6) launches the currently edited scene.
        rc.play_current_scene();
        assert!(rc.is_playing());
        assert!(rc.last_ran_current);
        assert_eq!(
            rc.running_scene(main, current).as_deref(),
            Some(current),
            "play-scene launches the current scene"
        );

        // Pause toggles the paused state — the instance stays alive.
        rc.toggle_pause();
        assert!(rc.is_paused());
        assert!(rc.is_running());
        assert_eq!(
            rc.running_scene(main, current).as_deref(),
            Some(current),
            "a paused instance is still the current scene"
        );
        // Toggling again resumes play.
        rc.toggle_pause();
        assert!(rc.is_playing());
        assert!(!rc.is_paused());

        // Stop terminates the running instance.
        rc.stop();
        assert!(!rc.is_running());
        assert_eq!(rc.state, PlayState::Stopped);
        assert_eq!(
            rc.running_scene(main, current),
            None,
            "stop terminates the running instance"
        );
    }
}
