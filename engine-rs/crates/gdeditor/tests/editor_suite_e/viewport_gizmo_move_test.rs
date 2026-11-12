//! pat-hg5eb: viewport move-gizmo axis-constrained vs free translation.
//!
//! Drives the public `GizmoDragState` API: dragging an axis handle constrains
//! the translation to that axis only, the center (XY) handle translates freely,
//! and the resulting `GizmoTransform::Translate` commits to the selection's
//! position.

use gdcore::math::Vector2;
use gdeditor::viewport_2d::{Gizmo2D, GizmoAxis, GizmoDragState, GizmoHit, GizmoTransform};

/// Builds a move-gizmo drag from `start` to `current` on the given axis.
fn move_drag(axis: GizmoAxis, start: Vector2, current: Vector2) -> GizmoDragState {
    GizmoDragState {
        hit: GizmoHit::Move(axis),
        gizmo: Gizmo2D::new(Vector2::new(100.0, 100.0)),
        start,
        current,
    }
}

#[test]
fn viewport_gizmo_move() {
    let start = Vector2::new(0.0, 0.0);
    // A diagonal drag — exercises that the off-axis component is dropped.
    let current = Vector2::new(40.0, 25.0);

    // Dragging the X-axis handle translates along X only.
    let x = move_drag(GizmoAxis::X, start, current);
    assert_eq!(
        x.current_transform(),
        GizmoTransform::Translate(Vector2::new(40.0, 0.0)),
        "X-axis handle translates along X only"
    );

    // Dragging the Y-axis handle translates along Y only.
    let y = move_drag(GizmoAxis::Y, start, current);
    assert_eq!(
        y.current_transform(),
        GizmoTransform::Translate(Vector2::new(0.0, 25.0)),
        "Y-axis handle translates along Y only"
    );

    // The center (XY) handle translates freely along both axes.
    let free = move_drag(GizmoAxis::XY, start, current);
    let transform = free.current_transform();
    assert_eq!(
        transform,
        GizmoTransform::Translate(Vector2::new(40.0, 25.0)),
        "center handle translates freely"
    );

    // Committing the drag applies the translation to the selection's position.
    let original = Vector2::new(10.0, 20.0);
    let GizmoTransform::Translate(delta) = transform else {
        panic!("expected a translation from a move-gizmo drag");
    };
    assert_eq!(
        original + delta,
        Vector2::new(50.0, 45.0),
        "committing the free drag moves the selection to its new position"
    );

    // The X-only commit moves horizontally and leaves Y untouched.
    let GizmoTransform::Translate(xdelta) = x.current_transform() else {
        unreachable!()
    };
    assert_eq!(
        original + xdelta,
        Vector2::new(50.0, 20.0),
        "an X-axis commit moves only horizontally"
    );
}
