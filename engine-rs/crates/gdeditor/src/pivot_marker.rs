//! The origin / pivot marker for the 2D viewport gizmo.
//!
//! Every selected node draws a small marker at its transform pivot. By default
//! the pivot sits on the node's origin (the point where its content is drawn).
//! In *pivot-edit* mode the user can drag that marker to relocate the pivot —
//! the point that rotation and scale operate about — **without** moving the
//! node's visual position. Relocating the pivot only changes the pivot offset;
//! the node's content stays exactly where it was rendered.
//!
//! This mirrors Godot's "Move Pivot" viewport tool, where dragging the pivot
//! handle changes the rotation/scale center while leaving the node in place.

use gdcore::math::Vector2;

/// Tracks a node's pivot marker: where the node's content is anchored (its
/// origin) and where the pivot currently sits.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PivotMarker {
    /// World-space origin of the node — where its content is rendered. Moving
    /// the pivot never changes this.
    node_origin: Vector2,
    /// World-space position of the pivot point (where the marker is drawn).
    pivot: Vector2,
    /// Whether pivot-edit mode is active. Dragging only relocates the pivot
    /// while this is on.
    edit_mode: bool,
}

impl PivotMarker {
    /// Creates a marker for a node whose origin is at `node_origin`. The pivot
    /// starts on the origin, so the marker is drawn at the node's origin.
    pub fn new(node_origin: Vector2) -> Self {
        Self {
            node_origin,
            pivot: node_origin,
            edit_mode: false,
        }
    }

    /// The node's visual origin — unaffected by pivot edits.
    pub fn node_origin(&self) -> Vector2 {
        self.node_origin
    }

    /// Where the pivot marker renders (the current pivot point).
    pub fn marker_position(&self) -> Vector2 {
        self.pivot
    }

    /// The pivot's offset from the node's origin. Zero when the pivot sits on
    /// the origin; rotation/scale operate about `node_origin + pivot_offset`.
    pub fn pivot_offset(&self) -> Vector2 {
        Vector2::new(self.pivot.x - self.node_origin.x, self.pivot.y - self.node_origin.y)
    }

    /// Enables or disables pivot-edit mode.
    pub fn set_edit_mode(&mut self, enabled: bool) {
        self.edit_mode = enabled;
    }

    /// Whether pivot-edit mode is active.
    pub fn is_edit_mode(&self) -> bool {
        self.edit_mode
    }

    /// Drags the pivot marker to a new world-space point.
    ///
    /// In pivot-edit mode this relocates the pivot to `world_point` and leaves
    /// the node's visual origin untouched, returning `true`. Outside pivot-edit
    /// mode the drag is ignored (the move gizmo, not the pivot, owns plain
    /// drags), returning `false`.
    pub fn drag_to(&mut self, world_point: Vector2) -> bool {
        if !self.edit_mode {
            return false;
        }
        self.pivot = world_point;
        true
    }

    /// Resets the pivot back onto the node's origin.
    pub fn reset(&mut self) {
        self.pivot = self.node_origin;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Acceptance (pat-f44yt): the pivot marker is drawn at the node's origin,
    /// and dragging it in pivot-edit mode relocates the transform pivot without
    /// moving the node's visual position.
    #[test]
    fn viewport_gizmo_origin_marker() {
        let origin = Vector2::new(120.0, 80.0);
        let mut marker = PivotMarker::new(origin);

        // The marker is drawn at the node's origin, with no pivot offset yet.
        assert_eq!(marker.marker_position(), origin, "marker renders at the node origin");
        assert_eq!(marker.node_origin(), origin);
        assert_eq!(marker.pivot_offset(), Vector2::ZERO, "pivot starts on the origin");

        // Outside pivot-edit mode, dragging the marker does nothing.
        assert!(!marker.is_edit_mode());
        assert!(!marker.drag_to(Vector2::new(200.0, 200.0)), "no pivot move without edit mode");
        assert_eq!(marker.marker_position(), origin, "pivot unchanged outside edit mode");

        // Enter pivot-edit mode and drag the marker to a new world point.
        marker.set_edit_mode(true);
        let new_pivot = Vector2::new(170.0, 130.0);
        assert!(marker.drag_to(new_pivot), "pivot-edit drag relocates the pivot");

        // The pivot relocated to the drag point...
        assert_eq!(marker.marker_position(), new_pivot, "marker follows the pivot drag");
        assert_eq!(
            marker.pivot_offset(),
            Vector2::new(50.0, 50.0),
            "pivot offset reflects the relocation"
        );
        // ...but the node's visual origin did NOT move.
        assert_eq!(
            marker.node_origin(),
            origin,
            "relocating the pivot does not move the node's visual position"
        );

        // Resetting snaps the pivot back to the origin.
        marker.reset();
        assert_eq!(marker.marker_position(), origin);
        assert_eq!(marker.pivot_offset(), Vector2::ZERO);
    }
}
