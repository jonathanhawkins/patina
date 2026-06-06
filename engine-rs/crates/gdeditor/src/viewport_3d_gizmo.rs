//! One-shot driver for the 3D transform gizmos (move / rotate / scale).
//!
//! The full drag lifecycle (`begin_gizmo_drag` → `update_gizmo_drag` →
//! `end_gizmo_drag`) lives in [`viewport_3d`](crate::viewport_3d). This module
//! adds a convenience that runs a complete drag in a single call — handy for
//! scripted/headless use and for exercising the gizmos end-to-end — over that
//! public API, in its own file to avoid growing the large `viewport_3d.rs`.

use gdcore::math::Vector3;

use crate::viewport_3d::{GizmoAxis, GizmoSnap3D, GizmoTransform3D, Viewport3D};

impl Viewport3D {
    /// Runs a complete gizmo drag for the active gizmo mode in one call:
    /// presses on `axis` at the gizmo's screen start `(start_px, start_py)`,
    /// drags to `(end_px, end_py)`, and returns the resulting transform —
    /// translation for Move, angle for Rotate, factor for Scale — with `snap`
    /// applied.
    ///
    /// Returns `None` if the gizmo can't be dragged (Select mode or
    /// `GizmoAxis::None`). The active mode is set via
    /// `selection.set_gizmo_mode` beforehand.
    #[allow(clippy::too_many_arguments)]
    pub fn run_gizmo_drag(
        &mut self,
        axis: GizmoAxis,
        gizmo_center: Vector3,
        start_px: f32,
        start_py: f32,
        end_px: f32,
        end_py: f32,
        snap: &GizmoSnap3D,
    ) -> Option<GizmoTransform3D> {
        if !self.begin_gizmo_drag(start_px, start_py, gizmo_center, axis) {
            return None;
        }
        self.update_gizmo_drag(end_px, end_py, snap);
        self.end_gizmo_drag()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::viewport_3d::{GizmoMode3D, Viewport3D};

    /// Acceptance (pat-kka71.3): the move, rotate, and scale gizmos each produce
    /// the right transform across a full drag, and Select/no-axis produce none.
    #[test]
    fn editor3d_transform_gizmos() {
        let mut vp = Viewport3D::new(800, 600);
        let center = Vector3::ZERO;
        let snap = GizmoSnap3D::default();
        let (cx, cy) = vp.world_to_screen(center);

        // MOVE along X: a horizontal drag translates only along the X axis.
        vp.selection.set_gizmo_mode(GizmoMode3D::Move);
        let mv = vp
            .run_gizmo_drag(GizmoAxis::X, center, cx, cy, cx + 200.0, cy, &snap)
            .expect("move drag yields a transform");
        match mv {
            GizmoTransform3D::Move(d) => {
                assert_eq!(d.y, 0.0, "X move stays on the X axis");
                assert_eq!(d.z, 0.0, "X move stays on the X axis");
                assert!(d.x != 0.0, "the drag produced an X translation");
            }
            other => panic!("expected Move, got {other:?}"),
        }
        assert!(!vp.is_gizmo_dragging(), "drag ended");

        // ROTATE about Y: an arc around the center yields a non-zero angle.
        vp.selection.set_gizmo_mode(GizmoMode3D::Rotate);
        let rot = vp
            .run_gizmo_drag(GizmoAxis::Y, center, cx + 50.0, cy, cx, cy + 50.0, &snap)
            .expect("rotate drag yields a transform");
        match rot {
            GizmoTransform3D::Rotate { axis, angle } => {
                assert_eq!(axis, GizmoAxis::Y);
                assert!(angle.abs() > 0.0, "rotation has a non-zero angle");
            }
            other => panic!("expected Rotate, got {other:?}"),
        }

        // SCALE along X: dragging outward from the center scales up (>1).
        vp.selection.set_gizmo_mode(GizmoMode3D::Scale);
        let sc = vp
            .run_gizmo_drag(GizmoAxis::X, center, cx + 20.0, cy, cx + 100.0, cy, &snap)
            .expect("scale drag yields a transform");
        match sc {
            GizmoTransform3D::Scale { axis, factor } => {
                assert_eq!(axis, GizmoAxis::X);
                assert!(factor > 1.0, "dragging outward scales up");
            }
            other => panic!("expected Scale, got {other:?}"),
        }

        // Select mode produces no transform.
        vp.selection.set_gizmo_mode(GizmoMode3D::Select);
        assert!(vp
            .run_gizmo_drag(GizmoAxis::X, center, cx, cy, cx + 50.0, cy, &snap)
            .is_none());

        // Even in a transform mode, a no-axis drag produces nothing.
        vp.selection.set_gizmo_mode(GizmoMode3D::Move);
        assert!(vp
            .run_gizmo_drag(GizmoAxis::None, center, cx, cy, cx + 50.0, cy, &snap)
            .is_none());
    }
}
