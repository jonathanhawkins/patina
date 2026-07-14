//! pat-7ekue: viewport scale-gizmo axis vs uniform scaling.
//!
//! Drives the public `GizmoDragState` API: dragging an axis scale handle scales
//! the selection along that axis only, the uniform (XY) handle scales both axes
//! proportionally, and the resulting `GizmoTransform::Scale` commits to the
//! selection's scale.

use gdcore::math::Vector2;
use gdeditor::viewport_2d::{Gizmo2D, GizmoAxis, GizmoDragState, GizmoHit, GizmoTransform};

const PIVOT: Vector2 = Vector2 { x: 100.0, y: 100.0 };

/// Builds a scale-gizmo drag from `start` to `current` on the given axis.
fn scale_drag(axis: GizmoAxis, start: Vector2, current: Vector2) -> GizmoDragState {
    GizmoDragState {
        hit: GizmoHit::Scale(axis),
        gizmo: Gizmo2D::new(PIVOT),
        start,
        current,
    }
}

/// Unwraps a `Scale` transform's factors, panicking on any other variant.
fn scale_factors(t: GizmoTransform) -> (f32, f32) {
    match t {
        GizmoTransform::Scale(sx, sy) => (sx, sy),
        other => panic!("expected a scale transform, got {other:?}"),
    }
}

#[test]
fn viewport_gizmo_scale() {
    // X-axis handle: dragging from +100px to +200px along X about the pivot
    // doubles the X scale and leaves Y untouched.
    let x = scale_drag(
        GizmoAxis::X,
        Vector2::new(200.0, 100.0),
        Vector2::new(300.0, 100.0),
    );
    let (sx, sy) = scale_factors(x.current_transform());
    assert!((sx - 2.0).abs() < 1e-5, "X handle scales X by 2x, got {sx}");
    assert!((sy - 1.0).abs() < 1e-5, "X handle leaves Y unscaled, got {sy}");

    // Y-axis handle: scales Y only.
    let y = scale_drag(
        GizmoAxis::Y,
        Vector2::new(100.0, 200.0),
        Vector2::new(100.0, 250.0),
    );
    let (sx, sy) = scale_factors(y.current_transform());
    assert!((sx - 1.0).abs() < 1e-5, "Y handle leaves X unscaled, got {sx}");
    assert!((sy - 1.5).abs() < 1e-5, "Y handle scales Y by 1.5x, got {sy}");

    // Uniform handle: dragging diagonally doubles the distance from the pivot,
    // scaling both axes proportionally.
    let uniform = scale_drag(
        GizmoAxis::XY,
        Vector2::new(200.0, 200.0),
        Vector2::new(300.0, 300.0),
    );
    let (ux, uy) = scale_factors(uniform.current_transform());
    assert!((ux - 2.0).abs() < 1e-5, "uniform handle scales X by 2x, got {ux}");
    assert!((uy - 2.0).abs() < 1e-5, "uniform handle scales Y by 2x, got {uy}");
    assert!(
        (ux - uy).abs() < 1e-6,
        "the uniform handle scales both axes equally"
    );

    // Committing applies the scale factors to the selection's current scale.
    let original = Vector2::new(3.0, 4.0);
    let committed = Vector2::new(original.x * ux, original.y * uy);
    assert_eq!(
        committed,
        Vector2::new(6.0, 8.0),
        "committing a 2x uniform scale doubles the selection's scale"
    );
}
