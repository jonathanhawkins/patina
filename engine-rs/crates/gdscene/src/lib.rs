//! # gdscene
//!
//! Node, SceneTree, packed scenes, instancing, and lifecycle management
//! for the Patina Engine runtime.
//!
//! This crate provides the core scene system:
//!
//! - [`node`] — The [`Node`] type and lightweight [`NodeId`] handle.
//! - [`scene_tree`] — The [`SceneTree`] arena that owns all nodes and
//!   provides hierarchy, path, and group operations.
//! - [`lifecycle`] — [`LifecycleManager`] for dispatching enter-tree,
//!   ready, and exit-tree notifications in the correct Godot-compatible order.
//! - [`main_loop`] — [`MainLoop`] for driving deterministic frame execution
//!   with fixed-timestep physics.
//! - [`packed_scene`] — [`PackedScene`] for parsing `.tscn` files and
//!   instantiating node subtrees.

#![warn(clippy::all)]

pub mod animation;
pub mod animation_tree;
pub mod animation_tree_tscn;
pub mod animation_tscn;
pub mod astar;
pub mod audio_server_bridge;
pub mod autoload;
pub mod behavioral_2d;
pub mod bezier_bake;
pub mod camera3d;
// ⚠️ `canvas_layer.rs` existed since the HUD work but was **never declared**, so
// the compiler had never seen it: not "unused helpers" but an orphaned file, its
// 14 getters/setters and its whole test module dead text (pat-0q2kx.212).
// Declaring it is what makes `CanvasLayer` placement reachable from the renderer.
pub mod canvas_layer;
pub mod canvas_order;
pub mod char_fx;
pub mod character3d;
pub mod collision;
pub mod color_picker;
pub mod control;
pub mod control_render;
pub mod frame_driver;
pub mod graph_edit;
pub mod grid_map;
pub mod ik_solver;
pub mod ik_solver_2d;
pub mod item_controls;
pub mod item_views;
pub mod lifecycle;
pub mod light2d_scene;
pub mod line_edit;
pub mod main_loop;
pub mod menu;
pub mod multiplayer;
pub mod nav_polygon;
pub mod nav_simplify;
pub mod navigation;
pub mod navigation_agents;
pub mod node;
pub mod node2d;
pub mod node3d;
pub mod packed_scene;
pub mod particle;
pub mod particle3d;
pub mod path3d;
pub mod physics_interpolation;
pub mod physics_interpolation_3d;
pub mod physics_server;
pub mod physics_server_3d;
pub mod polygon_path_finder;
pub mod popup;
pub mod range_controls;
pub mod raycast2d;
pub mod raycast3d;
pub mod render_server_3d;
pub mod resource_preloader;
pub mod root_motion_view;
pub mod scene_saver;
pub mod scene_tree;
pub mod scripting;
pub mod shapecast3d;
pub mod skeleton3d;
pub mod springarm3d;
pub mod sprite_frames;
pub mod stylebox;
pub mod sub_viewport;
pub mod syntax_highlighter;
pub mod theme;
pub mod tilemap;
pub mod trace;
mod trace_oracle_tests;
pub mod transform_inspector;
pub mod tween;
pub mod video_stream;
pub mod viewport;
pub mod visibility_3d;
// Turning a parsed `.tres` VisualShader into a compilable graph. Lives here
// because gdresource (parsing) and gdserver2d (the graph type) are siblings —
// only a crate downstream of both can see the two halves (pat-0q2kx.226).
pub mod visual_shader_load;

// Re-export the most-used types at the crate root.
pub use autoload::{boot_autoloads_from_dir, instantiate_autoloads, AutoloadResult};
pub use grid_map::{GridCell, GridMap, MeshLibrary, MeshLibraryItem};
pub use lifecycle::LifecycleManager;
pub use main_loop::{FrameOutput, FrameRecord, FrameTrace, MainLoop, NodeSnapshot};
pub use node::{Node, NodeId, ProcessMode};
pub use packed_scene::{
    add_packed_scene_to_tree, add_packed_scene_to_tree_with_subscenes, wire_connections,
    ExtResourceEntry, PackedScene, SceneConnection,
};
pub use path3d::{Curve2D, Curve3D, Path3D, PathFollow3D, PathFollowRotation};
pub use scene_saver::TscnSaver;
pub use scene_tree::{SceneTree, SceneTreeTimer};
pub use scripting::{GDScriptNodeInstance, InputSnapshot};
pub use tilemap::{default_color_tileset, ColorTile, ColorTileSet, TileGrid, TileGridStore};
pub use trace::{EventTrace, TraceEvent, TraceEventType};

pub use theme::{Theme, ThemeDB, ThemePropertyType};

pub use sub_viewport::{
    apply_subviewport_defaults, get_clear_mode, get_size, get_transparent_bg, get_update_mode,
    render_subviewport_to_texture, set_clear_mode, set_size, set_transparent_bg, set_update_mode,
    ClearMode, ColorBuffer, SubViewport, UpdateMode, ViewportTexture,
};

// Re-export signal types for integration tests.
pub use gdobject::signal::Connection as SignalConnection;
