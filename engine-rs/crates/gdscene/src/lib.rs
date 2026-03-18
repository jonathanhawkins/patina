//! # gdscene
//!
//! Node, SceneTree, packed scenes, instancing, and lifecycle management
//! for the Patina Engine runtime.
//!
//! This crate provides the core scene system:
//!
//! - [`node`] — The [`Node`](node::Node) type and lightweight
//!   [`NodeId`](node::NodeId) handle.
//! - [`scene_tree`] — The [`SceneTree`](scene_tree::SceneTree) arena that
//!   owns all nodes and provides hierarchy, path, and group operations.
//! - [`lifecycle`] — [`LifecycleManager`](lifecycle::LifecycleManager) for
//!   dispatching enter-tree, ready, and exit-tree notifications in the
//!   correct Godot-compatible order.
//! - [`main_loop`] — [`MainLoop`](main_loop::MainLoop) for driving
//!   deterministic frame execution with fixed-timestep physics.
//! - [`packed_scene`] — [`PackedScene`](packed_scene::PackedScene) for
//!   parsing `.tscn` files and instantiating node subtrees.

#![warn(clippy::all)]

pub mod lifecycle;
pub mod main_loop;
pub mod node;
pub mod node2d;
pub mod packed_scene;
pub mod scene_tree;

// Re-export the most-used types at the crate root.
pub use lifecycle::LifecycleManager;
pub use main_loop::MainLoop;
pub use node::{Node, NodeId};
pub use packed_scene::{add_packed_scene_to_tree, wire_connections, PackedScene, SceneConnection};
pub use scene_tree::SceneTree;
