//! pat-0q2kx.219 — physics interpolation for the render tick.
//!
//! With `physics/common/physics_interpolation` on, Godot renders a node at a
//! transform blended between the **previous** and **current** physics ticks
//! using the accumulator fraction, so a 20 Hz simulation still looks smooth at
//! 60 fps. Patina wrote the raw last-tick transform straight out of
//! `sync_from_physics`, so a slow physics rate visibly stepped.
//!
//! Godot: `scene/main/scene_tree.cpp` `set_physics_interpolation_enabled`,
//! `scene/main/node.cpp` `set_physics_interpolation_mode` /
//! `_propagate_physics_interpolated` / `reset_physics_interpolation`.
//!
//! ## Where the blended value goes
//!
//! Into [`INTERP_POSITION_PROP`] / [`INTERP_ROTATION_PROP`], **not** into
//! `position` / `rotation`. That is deliberate and matches Godot's split: the
//! node's own transform stays the raw physics result so a script reading
//! `global_position` in `_physics_process` sees the simulation, while the
//! render side reads the interpolated copy. Blending into `position` would make
//! scripts observe a value that never actually occurred in the simulation.

use std::collections::HashMap;

use gdcore::math::Vector2;
use gdvariant::Variant;

use crate::node::NodeId;
use crate::node2d;
use crate::scene_tree::SceneTree;

/// Render-facing interpolated position, published each frame.
pub const INTERP_POSITION_PROP: &str = "__interp_position";
/// Render-facing interpolated rotation (radians).
pub const INTERP_ROTATION_PROP: &str = "__interp_rotation";
/// Godot's `Node.physics_interpolation_mode`.
pub const INTERP_MODE_PROP: &str = "physics_interpolation_mode";

/// Godot's `Node.PhysicsInterpolationMode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PhysicsInterpolationMode {
    /// Follow the parent, ultimately the tree-wide toggle.
    #[default]
    Inherit = 0,
    On = 1,
    Off = 2,
}

impl PhysicsInterpolationMode {
    pub fn from_i64(value: i64) -> Self {
        match value {
            1 => Self::On,
            2 => Self::Off,
            _ => Self::Inherit,
        }
    }
}

/// The classes whose transforms the physics step drives, and therefore the ones
/// worth interpolating. Mirrors `physics_server::body_type_for_class`.
fn is_physics_driven(class_name: &str) -> bool {
    matches!(
        class_name,
        "RigidBody2D" | "StaticBody2D" | "CharacterBody2D"
    )
}

/// Resolves a node's effective interpolation, walking `INHERIT` up the parent
/// chain the way [`SceneTree::effective_process_mode`] resolves process modes.
///
/// `tree_enabled` is the root default, so a scene with the project setting off
/// can still opt a single node in with `ON`.
pub fn effective_interpolation(tree: &SceneTree, node_id: NodeId, tree_enabled: bool) -> bool {
    let mut current = Some(node_id);
    while let Some(id) = current {
        let Some(node) = tree.get_node(id) else {
            break;
        };
        if let Variant::Int(v) = node.get_property(INTERP_MODE_PROP) {
            match PhysicsInterpolationMode::from_i64(v) {
                PhysicsInterpolationMode::On => return true,
                PhysicsInterpolationMode::Off => return false,
                PhysicsInterpolationMode::Inherit => {}
            }
        }
        current = node.parent();
    }
    tree_enabled
}

/// One physics tick's transform for a node.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Snapshot {
    position: Vector2,
    rotation: f32,
}

/// Double-buffered physics transforms plus the published blend.
#[derive(Debug, Default)]
pub struct PhysicsInterpolationState {
    previous: HashMap<NodeId, Snapshot>,
    current: HashMap<NodeId, Snapshot>,
    /// Nodes whose next capture must NOT blend from the tick before it.
    ///
    /// Clearing `previous` alone is not enough: the very next `capture` rolls
    /// the old `current` straight back into `previous`, so the reset would be
    /// undone one tick later and the node would still streak across the
    /// teleport. Membership here makes that capture set `previous == current`,
    /// which pins the blend to the new location for the whole interval.
    pending_reset: std::collections::HashSet<NodeId>,
}

impl PhysicsInterpolationState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Records this tick's transforms, rolling the old `current` into
    /// `previous`. Call once per physics tick, **after** `sync_from_physics`.
    pub fn capture(&mut self, tree: &SceneTree) {
        for id in tree.all_nodes_in_tree_order() {
            let Some(node) = tree.get_node(id) else {
                continue;
            };
            if !is_physics_driven(node.class_name()) {
                continue;
            }
            let snapshot = Snapshot {
                position: node2d::get_position(tree, id),
                rotation: node2d::get_rotation(tree, id),
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
    /// snapshot yet (freshly spawned, or just reset) publishes its current
    /// transform verbatim rather than blending from nothing.
    pub fn publish(&self, tree: &mut SceneTree, fraction: f32, tree_enabled: bool) {
        let fraction = fraction.clamp(0.0, 1.0);

        let mut writes: Vec<(NodeId, Vector2, f32)> = Vec::new();
        for (&id, current) in &self.current {
            if !effective_interpolation(tree, id, tree_enabled) {
                continue;
            }
            let blended = match self.previous.get(&id) {
                Some(previous) => Snapshot {
                    position: previous.position + (current.position - previous.position) * fraction,
                    rotation: previous.rotation
                        + shortest_arc(previous.rotation, current.rotation) * fraction,
                },
                None => *current,
            };
            writes.push((id, blended.position, blended.rotation));
        }

        for (id, position, rotation) in writes {
            if let Some(node) = tree.get_node_mut(id) {
                node.set_property(INTERP_POSITION_PROP, Variant::Vector2(position));
                node.set_property(INTERP_ROTATION_PROP, Variant::Float(rotation as f64));
            }
        }
    }

    /// Godot's `Node.reset_physics_interpolation()`: drops the previous
    /// snapshot so the next frame renders at the current transform instead of
    /// blending across a teleport, which would streak the node through
    /// positions it never occupied.
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
    pub fn tick_positions(&self, node_id: NodeId) -> (Option<Vector2>, Option<Vector2>) {
        (
            self.previous.get(&node_id).map(|s| s.position),
            self.current.get(&node_id).map(|s| s.position),
        )
    }
}

/// The signed shortest angular delta from `from` to `to`.
///
/// A plain `to - from` lerp would spin the long way around whenever a body
/// crosses ±π — a full rotation of visual garbage on one frame.
fn shortest_arc(from: f32, to: f32) -> f32 {
    use std::f32::consts::PI;
    let mut delta = (to - from) % (2.0 * PI);
    if delta > PI {
        delta -= 2.0 * PI;
    } else if delta < -PI {
        delta += 2.0 * PI;
    }
    delta
}

/// Reads the interpolated position a node last published, if any.
pub fn interpolated_position(tree: &SceneTree, node_id: NodeId) -> Option<Vector2> {
    match tree.get_node(node_id)?.get_property(INTERP_POSITION_PROP) {
        Variant::Vector2(v) => Some(v),
        _ => None,
    }
}

/// Reads the interpolated rotation a node last published, if any.
pub fn interpolated_rotation(tree: &SceneTree, node_id: NodeId) -> Option<f32> {
    match tree.get_node(node_id)?.get_property(INTERP_ROTATION_PROP) {
        Variant::Float(f) => Some(f as f32),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::PI;

    #[test]
    fn shortest_arc_takes_the_short_way_around_pi() {
        // 170° -> -170° is +20°, not -340°.
        let from = 170.0_f32.to_radians();
        let to = (-170.0_f32).to_radians();
        let delta = shortest_arc(from, to);
        assert!(
            (delta - 20.0_f32.to_radians()).abs() < 1e-4,
            "expected +20°, got {}°",
            delta.to_degrees(),
        );
        assert!(delta.abs() <= PI, "a shortest arc never exceeds pi");
    }

    #[test]
    fn shortest_arc_is_plain_subtraction_away_from_the_wrap() {
        assert!((shortest_arc(0.1, 0.4) - 0.3).abs() < 1e-6);
        assert!((shortest_arc(0.4, 0.1) + 0.3).abs() < 1e-6);
    }

    #[test]
    fn mode_parses_godots_ints() {
        assert_eq!(
            PhysicsInterpolationMode::from_i64(0),
            PhysicsInterpolationMode::Inherit
        );
        assert_eq!(
            PhysicsInterpolationMode::from_i64(1),
            PhysicsInterpolationMode::On
        );
        assert_eq!(
            PhysicsInterpolationMode::from_i64(2),
            PhysicsInterpolationMode::Off
        );
        // Anything unrecognised inherits rather than silently disabling.
        assert_eq!(
            PhysicsInterpolationMode::from_i64(99),
            PhysicsInterpolationMode::Inherit
        );
    }
}
