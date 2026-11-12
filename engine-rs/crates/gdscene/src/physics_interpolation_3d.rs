//! pat-0q2kx.250 — render-tick physics interpolation for 3D bodies.
//!
//! The 3D counterpart of [`crate::physics_interpolation`]. `sync_from_physics`
//! (`physics_server_3d.rs`) snapped node transforms once per tick with no
//! previous/current state, so at 10 tps against 60 fps rendering five frames in
//! six showed a *stale* transform and motion visibly stepped.
//!
//! Godot 4.4 blends between the previous and current physics ticks using the
//! accumulator fraction, gated by `physics/common/physics_interpolation`, with
//! `Node.physics_interpolation_mode` opting nodes in or out and
//! `reset_physics_interpolation()` snapping after teleports.
//!
//! ## What is shared with the 2D module and what is not
//!
//! [`PhysicsInterpolationMode`], [`INTERP_MODE_PROP`] and
//! [`effective_interpolation`] are **reused directly** — `physics_interpolation_
//! mode` is a `Node` property with no dimensionality, so a second copy here
//! would be a second place for the inherit-walk to drift.
//!
//! The state is separate because the maths genuinely differs: 2D blends a
//! scalar angle along its shortest arc, while 3D blends a [`Quaternion`] with
//! `slerp`. Storing Euler triples and lerping each axis would have been the
//! easy mirror and is wrong — Euler angles wrap independently and gimbal-lock,
//! so a body tumbling through a pole would visibly snap.

use std::collections::{HashMap, HashSet};

use gdcore::math::Vector3;
use gdcore::math3d::Quaternion;
use gdvariant::Variant;

use crate::node::NodeId;
use crate::node3d;
use crate::scene_tree::SceneTree;

// Re-exported so callers of the 3D path do not have to know the mode plumbing
// lives in the 2D module.
pub use crate::physics_interpolation::{
    effective_interpolation, PhysicsInterpolationMode, INTERP_MODE_PROP,
};

/// Where the blended 3D position is published for the renderer to read.
///
/// Published to a **property** rather than written back onto the node
/// transform, so physics keeps reading the raw last-tick transform. Writing the
/// blend onto the node itself would feed interpolated values back into the next
/// tick's `sync_to_physics` and corrupt the simulation.
pub const INTERP_POSITION_3D_PROP: &str = "__interp_position_3d";

/// Where the blended 3D rotation (Euler, radians) is published.
pub const INTERP_ROTATION_3D_PROP: &str = "__interp_rotation_3d";

/// The node classes `PhysicsServer3D` simulates.
fn is_physics_driven_3d(class_name: &str) -> bool {
    matches!(
        class_name,
        "RigidBody3D" | "StaticBody3D" | "CharacterBody3D"
    )
}

/// One physics tick's 3D transform for a node.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Snapshot3D {
    position: Vector3,
    rotation: Quaternion,
}

/// Double-buffered 3D physics transforms plus the published blend.
#[derive(Debug, Default)]
pub struct PhysicsInterpolation3DState {
    previous: HashMap<NodeId, Snapshot3D>,
    current: HashMap<NodeId, Snapshot3D>,
    /// Nodes whose next capture must NOT blend from the tick before it.
    ///
    /// Clearing `previous` alone is not enough: the very next `capture` rolls
    /// the old `current` straight back into `previous`, undoing the reset one
    /// tick later so the node still streaks across the teleport. Membership
    /// here makes that capture set `previous == current`, pinning the blend to
    /// the new location for the whole interval.
    pending_reset: HashSet<NodeId>,
}

impl PhysicsInterpolation3DState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Records this tick's transforms, rolling the old `current` into
    /// `previous`. Call once per physics tick, **after** `sync_from_physics`,
    /// so it stores the transforms the solver actually produced.
    pub fn capture(&mut self, tree: &SceneTree) {
        for id in tree.all_nodes_in_tree_order() {
            let Some(node) = tree.get_node(id) else {
                continue;
            };
            if !is_physics_driven_3d(node.class_name()) {
                continue;
            }
            let snapshot = Snapshot3D {
                position: node3d::get_position(tree, id),
                rotation: Quaternion::from_euler(node3d::get_rotation(tree, id)),
            };
            let displaced = self.current.insert(id, snapshot);
            if self.pending_reset.remove(&id) {
                // Snap: previous == current, so this interval blends to a
                // constant at the post-teleport transform.
                self.previous.insert(id, snapshot);
            } else if let Some(prev_current) = displaced {
                self.previous.insert(id, prev_current);
            }
        }
    }

    /// Publishes the blended transform for every interpolated node.
    ///
    /// `fraction` is `physics_accumulator / physics_dt` — how far the render
    /// frame sits past the last completed tick. A node with no `previous`
    /// snapshot (freshly spawned, or just reset) publishes its current
    /// transform verbatim rather than blending from nothing.
    pub fn publish(&self, tree: &mut SceneTree, fraction: f32, tree_enabled: bool) {
        let fraction = fraction.clamp(0.0, 1.0);

        let mut writes: Vec<(NodeId, Vector3, Vector3)> = Vec::new();
        for (&id, current) in &self.current {
            if !effective_interpolation(tree, id, tree_enabled) {
                continue;
            }
            let (position, rotation) = match self.previous.get(&id) {
                Some(previous) => (
                    previous.position + (current.position - previous.position) * fraction,
                    previous.rotation.slerp(current.rotation, fraction),
                ),
                None => (current.position, current.rotation),
            };
            writes.push((id, position, rotation.to_euler()));
        }

        for (id, position, euler) in writes {
            if let Some(node) = tree.get_node_mut(id) {
                node.set_property(INTERP_POSITION_3D_PROP, Variant::Vector3(position));
                node.set_property(INTERP_ROTATION_3D_PROP, Variant::Vector3(euler));
            }
        }
    }

    /// Godot's `Node.reset_physics_interpolation()`: drops the snapshots so the
    /// next frame renders at the post-teleport transform instead of streaking
    /// through positions the body never occupied.
    pub fn reset_node(&mut self, node_id: NodeId) {
        self.previous.remove(&node_id);
        // Also drop the current snapshot: it predates the teleport, so leaving
        // it would blend the very next frame from the OLD position.
        self.current.remove(&node_id);
        self.pending_reset.insert(node_id);
    }

    /// Drops every buffer — used when the tree-wide toggle flips off.
    pub fn clear(&mut self) {
        self.previous.clear();
        self.current.clear();
        self.pending_reset.clear();
    }

    /// The last two recorded positions for a node, for tests and debugging.
    pub fn tick_positions(&self, node_id: NodeId) -> (Option<Vector3>, Option<Vector3>) {
        (
            self.previous.get(&node_id).map(|s| s.position),
            self.current.get(&node_id).map(|s| s.position),
        )
    }
}

/// Reads the interpolated 3D position a node last published, if any.
pub fn interpolated_position_3d(tree: &SceneTree, node_id: NodeId) -> Option<Vector3> {
    match tree
        .get_node(node_id)?
        .get_property(INTERP_POSITION_3D_PROP)
    {
        Variant::Vector3(v) => Some(v),
        _ => None,
    }
}

/// Reads the interpolated 3D rotation (Euler, radians) a node last published.
pub fn interpolated_rotation_3d(tree: &SceneTree, node_id: NodeId) -> Option<Vector3> {
    match tree
        .get_node(node_id)?
        .get_property(INTERP_ROTATION_3D_PROP)
    {
        Variant::Vector3(v) => Some(v),
        _ => None,
    }
}
