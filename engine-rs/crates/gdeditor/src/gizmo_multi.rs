//! Multi-node gizmo transforms about a shared pivot, recorded as one undo step.
//!
//! When several nodes are selected, a gizmo drag transforms the whole selection
//! about a single shared pivot (the selection's center / gizmo origin) rather
//! than each node about its own center — so the group moves, rotates, and scales
//! rigidly, exactly as Godot does. The entire multi-node change is pushed as a
//! single [`UndoAction`] so one undo/redo reverses the whole operation and the
//! scene is marked dirty once.
//!
//! The per-node geometry reuses [`Gizmo2D::apply_transform_about_pivot`], so a
//! node coincident with the pivot stays put under rotate/scale while the others
//! orbit / scale around it.

use crate::undo_redo::{UndoAction, UndoOp, UndoRedoManager};
use crate::viewport_2d::{Gizmo2D, GizmoTransform};
use gdcore::math::Vector2;

/// A node participating in a multi-node gizmo transform: its id and current
/// world-space position.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GizmoNode {
    /// The node's id.
    pub id: u64,
    /// The node's current world-space position.
    pub position: Vector2,
}

impl GizmoNode {
    /// Creates a node descriptor.
    pub fn new(id: u64, position: Vector2) -> Self {
        Self { id, position }
    }
}

/// Applies `transform` to every node about the `pivot` shared by the whole
/// selection, returning each node's new position. Every node is transformed
/// about the SAME pivot (not its own center), so the selection moves rigidly.
pub fn transform_nodes_about_pivot(
    nodes: &[GizmoNode],
    pivot: Vector2,
    transform: GizmoTransform,
) -> Vec<GizmoNode> {
    let gizmo = Gizmo2D::new(pivot);
    nodes
        .iter()
        .map(|n| GizmoNode {
            id: n.id,
            position: gizmo.apply_transform_about_pivot(n.position, transform),
        })
        .collect()
}

/// Applies a gizmo transform to a multi-node selection about the shared pivot
/// and records it as exactly ONE undo entry (a single grouped action) on
/// `undo`, which marks the scene dirty. Returns the new node positions.
///
/// The grouped action holds one `SetProperty` op per node (position
/// old → new), so a single undo/redo reverses the whole selection's move.
pub fn apply_multi_node_gizmo(
    nodes: &[GizmoNode],
    pivot: Vector2,
    transform: GizmoTransform,
    undo: &mut UndoRedoManager,
) -> Vec<GizmoNode> {
    let moved = transform_nodes_about_pivot(nodes, pivot, transform);
    let ops: Vec<UndoOp> = nodes
        .iter()
        .zip(moved.iter())
        .map(|(before, after)| UndoOp::SetProperty {
            node_id: before.id,
            property: "position".to_string(),
            new_value: format!("{},{}", after.position.x, after.position.y),
            old_value: format!("{},{}", before.position.x, before.position.y),
        })
        .collect();
    undo.push(UndoAction::group("Transform Selection", ops));
    moved
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Acceptance (pat-cc5c2): with multiple nodes selected, a gizmo transform
    /// applies to every selection about the shared pivot and is recorded as one
    /// reversible undo/redo step that marks the scene dirty.
    #[test]
    fn viewport_gizmo_multi_node_undo() {
        use std::f32::consts::FRAC_PI_2;

        // Two selected nodes; the shared pivot is the origin.
        let nodes = vec![
            GizmoNode::new(1, Vector2::new(10.0, 0.0)),
            GizmoNode::new(2, Vector2::new(0.0, 20.0)),
        ];
        let pivot = Vector2::new(0.0, 0.0);

        let mut undo = UndoRedoManager::new(64);
        undo.mark_saved();
        assert!(!undo.is_dirty(), "clean before the transform");

        // A quarter-turn rotation about the shared pivot orbits BOTH nodes.
        let moved =
            apply_multi_node_gizmo(&nodes, pivot, GizmoTransform::Rotate(FRAC_PI_2), &mut undo);

        // Node 1: (10,0) -> (0,10); node 2: (0,20) -> (-20,0). Both orbit the
        // shared pivot rather than their own centers.
        assert!(
            (moved[0].position.x).abs() < 1e-3 && (moved[0].position.y - 10.0).abs() < 1e-3,
            "node 1 orbits the shared pivot: {:?}",
            moved[0].position
        );
        assert!(
            (moved[1].position.x + 20.0).abs() < 1e-3 && (moved[1].position.y).abs() < 1e-3,
            "node 2 orbits the shared pivot: {:?}",
            moved[1].position
        );

        // Recorded as exactly ONE undo entry covering both nodes.
        assert_eq!(
            undo.undo_count(),
            1,
            "a single undo entry for the whole selection"
        );
        assert_eq!(
            undo.undo_stack()[0].op_count(),
            2,
            "one op per selected node inside the single entry"
        );
        assert!(undo.is_dirty(), "the transform marks the scene dirty");

        // The single entry undoes/redoes the whole selection in one step.
        assert!(undo.undo().is_some(), "one undo reverses the whole transform");
        assert_eq!(undo.undo_count(), 0);
        assert!(undo.can_redo());
        assert!(!undo.is_dirty(), "undo returns to the saved (clean) state");

        assert!(undo.redo().is_some(), "one redo reapplies the whole transform");
        assert_eq!(undo.undo_count(), 1);
        assert!(undo.is_dirty(), "redo re-dirties the scene");
    }

    /// A node coincident with the shared pivot stays put under rotate, while the
    /// others orbit — and it's still part of the single grouped undo entry.
    #[test]
    fn node_on_pivot_stays_put_in_group() {
        use std::f32::consts::FRAC_PI_2;

        let pivot = Vector2::new(5.0, 5.0);
        let nodes = vec![
            GizmoNode::new(1, pivot),                       // on the pivot
            GizmoNode::new(2, Vector2::new(15.0, 5.0)),     // 10px to the right
        ];
        let mut undo = UndoRedoManager::new(64);

        let moved =
            apply_multi_node_gizmo(&nodes, pivot, GizmoTransform::Rotate(FRAC_PI_2), &mut undo);

        // Node on the pivot is unchanged.
        assert!(
            (moved[0].position.x - pivot.x).abs() < 1e-3
                && (moved[0].position.y - pivot.y).abs() < 1e-3,
            "node on the pivot stays put: {:?}",
            moved[0].position
        );
        // The other orbits a quarter turn: (15,5) about (5,5) -> (5,15).
        assert!(
            (moved[1].position.x - 5.0).abs() < 1e-3 && (moved[1].position.y - 15.0).abs() < 1e-3,
            "the other node orbits the shared pivot: {:?}",
            moved[1].position
        );

        assert_eq!(undo.undo_count(), 1, "still a single undo entry");
        assert_eq!(undo.undo_stack()[0].op_count(), 2);
    }
}
