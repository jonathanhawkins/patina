//! Applies the editor's snap settings to a live gizmo drag.
//!
//! While a transform gizmo is being dragged it produces a [`GizmoTransform`]
//! (a translation delta, a rotation in radians, or per-axis scale factors).
//! With snapping enabled these are quantized as the drag happens: move drags
//! land on the grid step, rotate drags on the angle step, and scale drags on
//! the scale step — driven by the user's [`SnapConfig`].
//!
//! A step of `0` in the config disables that axis of snapping, so the transform
//! passes through unchanged.

use crate::snap_config::SnapConfig;
use crate::viewport_2d::GizmoTransform;
use gdcore::math::Vector2;

/// Snaps a translation drag: the node's resulting position (`start + delta`)
/// snaps to the grid, and the adjusted delta to reach it is returned.
pub fn snap_translation(delta: Vector2, start: Vector2, snap: &SnapConfig) -> Vector2 {
    let snapped_pos = snap.snap_position(start + delta);
    snapped_pos - start
}

/// Snaps a live gizmo transform to the active snap settings, given the node's
/// pre-drag `start` position (used to snap a move to absolute grid lines).
pub fn snap_gizmo_transform(
    transform: GizmoTransform,
    start: Vector2,
    snap: &SnapConfig,
) -> GizmoTransform {
    match transform {
        GizmoTransform::Translate(delta) => {
            GizmoTransform::Translate(snap_translation(delta, start, snap))
        }
        GizmoTransform::Rotate(radians) => {
            // The gizmo works in radians; the snap step is configured in degrees.
            let snapped_deg = snap.snap_rotation_deg(radians.to_degrees());
            GizmoTransform::Rotate(snapped_deg.to_radians())
        }
        GizmoTransform::Scale(sx, sy) => {
            GizmoTransform::Scale(snap.snap_scale(sx), snap.snap_scale(sy))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> SnapConfig {
        SnapConfig {
            grid_step: 10.0,
            grid_offset: [0.0, 0.0],
            rotation_step_deg: 45.0,
            scale_step: 0.25,
        }
    }

    /// Acceptance (pat-5m8ej): with snapping enabled, move drags snap to the
    /// grid step, rotate drags snap to the angle step, and scale drags snap to
    /// the scale step during the gizmo operation.
    #[test]
    fn viewport_gizmo_snap_during_drag() {
        let snap = config();
        let start = Vector2::new(0.0, 0.0);

        // Move drag: the resulting position snaps to the 10px grid.
        let moved = snap_gizmo_transform(
            GizmoTransform::Translate(Vector2::new(23.0, 7.0)),
            start,
            &snap,
        );
        match moved {
            GizmoTransform::Translate(d) => {
                assert!(
                    (d.x - 20.0).abs() < 1e-4 && (d.y - 10.0).abs() < 1e-4,
                    "move snaps to the grid step, got {d:?}"
                );
            }
            other => panic!("expected a translate, got {other:?}"),
        }

        // Rotate drag: snaps to the 45° angle step.
        let rotated = snap_gizmo_transform(
            GizmoTransform::Rotate(50.0_f32.to_radians()),
            start,
            &snap,
        );
        match rotated {
            GizmoTransform::Rotate(rad) => {
                assert!(
                    (rad.to_degrees() - 45.0).abs() < 1e-3,
                    "rotation snaps to the 45° step, got {}°",
                    rad.to_degrees()
                );
            }
            other => panic!("expected a rotate, got {other:?}"),
        }

        // Scale drag: each axis snaps to the 0.25 scale step.
        let scaled = snap_gizmo_transform(GizmoTransform::Scale(0.3, 0.62), start, &snap);
        match scaled {
            GizmoTransform::Scale(sx, sy) => {
                assert!(
                    (sx - 0.25).abs() < 1e-4 && (sy - 0.5).abs() < 1e-4,
                    "scale snaps to the scale step, got ({sx}, {sy})"
                );
            }
            other => panic!("expected a scale, got {other:?}"),
        }
    }

    /// With snapping disabled (zero steps), the transform is untouched.
    #[test]
    fn snapping_disabled_passes_through() {
        let snap = SnapConfig {
            grid_step: 0.0,
            grid_offset: [0.0, 0.0],
            rotation_step_deg: 0.0,
            scale_step: 0.0,
        };
        let start = Vector2::new(5.0, 5.0);

        let t = GizmoTransform::Translate(Vector2::new(23.0, 7.0));
        assert_eq!(snap_gizmo_transform(t, start, &snap), t, "no grid snap when disabled");

        let r = GizmoTransform::Rotate(50.0_f32.to_radians());
        assert_eq!(snap_gizmo_transform(r, start, &snap), r, "no rotation snap when disabled");

        let s = GizmoTransform::Scale(0.3, 0.62);
        assert_eq!(snap_gizmo_transform(s, start, &snap), s, "no scale snap when disabled");
    }
}
