//! Node-type 3D gizmo handles: the draggable dots Godot puts on a selected
//! node to edit its SHAPE rather than its transform (pat-nzgdu.106).
//!
//! The transform gizmo (move/rotate/scale) lives in
//! [`viewport_3d`](crate::viewport_3d); this module is the geometry behind the
//! *other* gizmo Godot draws — the one that resizes a `BoxShape3D`, sets an
//! `OmniLight3D` range, and so on.
//!
//! This is a 1:1 port of Godot's `Gizmo3DHelper`
//! (`upstream/godot/editor/scene/3d/gizmos/gizmo_3d_helper.cpp`), so a drag
//! lands on the same size and position Godot would produce rather than on a
//! plausible-looking approximation:
//!
//! - [`box_get_handles`] ← `box_get_handles` (:53)
//! - [`box_set_handle`] ← `box_set_handle` (:79)
//!
//! Two details of that formula are worth calling out, because both are easy to
//! get wrong by guessing:
//!
//! 1. A plain drag moves **one face** and re-centres the node so the opposite
//!    face stays put — it does not grow the box symmetrically. Godot returns a
//!    new *position* alongside the new size for exactly this reason
//!    (`collision_shape_3d_gizmo_plugin.cpp:215-222` sets both).
//! 2. Holding **Alt** switches to symmetric resize about the node origin, and
//!    only then does the position stay fixed.
//!
//! Wiring this into the served viewport (handle draw, hit-test, and the
//! `/api/viewport/shape_resize` round-trip) is deliberately NOT here — see the
//! bead notes. This module is the parity-exact core those layers call.

use gdcore::math::Vector3;
use gdcore::math3d::Transform3D;

/// Smallest extent Godot lets a box handle collapse to
/// (`gizmo_3d_helper.cpp:105` — `MAX(r_box_size[axis], 0.001)`).
pub const MIN_BOX_EXTENT: f32 = 0.001;

/// Number of handles a `BoxShape3D` exposes: one per face.
pub const BOX_HANDLE_COUNT: usize = 6;

/// Reads component `axis` (0=x, 1=y, 2=z) — Godot indexes `Vector3` directly.
fn axis_get(v: Vector3, axis: usize) -> f32 {
    match axis {
        0 => v.x,
        1 => v.y,
        _ => v.z,
    }
}

/// Writes component `axis` (0=x, 1=y, 2=z).
fn axis_set(v: &mut Vector3, axis: usize, value: f32) {
    match axis {
        0 => v.x = value,
        1 => v.y = value,
        _ => v.z = value,
    }
}

/// Godot's `Math::snapped`: rounds to the nearest multiple of `step`, and is a
/// no-op when `step` is zero.
fn snapped(value: f32, step: f32) -> f32 {
    if step != 0.0 {
        (value / step + 0.5).floor() * step
    } else {
        value
    }
}

/// The axis (0=x, 1=y, 2=z) and sign a handle id drives.
///
/// Godot packs both into the id: `axis = id / 2`, `sign = id % 2 * -2 + 1`
/// (`gizmo_3d_helper.cpp:80-81`), which pairs with the push order in
/// [`box_get_handles`] — even ids are the `+axis` face, odd ids the `-axis`.
pub fn box_handle_axis_sign(handle_id: usize) -> (usize, f32) {
    (handle_id / 2, if handle_id % 2 == 0 { 1.0 } else { -1.0 })
}

/// Local-space positions of a `BoxShape3D`'s six face-centre handles, in
/// Godot's handle-id order: `0=+X, 1=-X, 2=+Y, 3=-Y, 4=+Z, 5=-Z`.
///
/// Port of `Gizmo3DHelper::box_get_handles` (:53), which pushes `+ax` then
/// `-ax` for each of the three axes at half the box size.
pub fn box_get_handles(box_size: Vector3) -> [Vector3; BOX_HANDLE_COUNT] {
    let mut handles = [Vector3::ZERO; BOX_HANDLE_COUNT];
    for axis in 0..3 {
        let mut ax = Vector3::ZERO;
        axis_set(&mut ax, axis, axis_get(box_size, axis) / 2.0);
        handles[axis * 2] = ax;
        handles[axis * 2 + 1] = Vector3::new(-ax.x, -ax.y, -ax.z);
    }
    handles
}

/// Resolves a box handle drag to a new `(size, world_position)`.
///
/// Port of `Gizmo3DHelper::box_set_handle` (:79-121). `ra` is the point on the
/// dragged handle's axis closest to the mouse ray, in the shape's LOCAL space
/// — Godot obtains it via `get_closest_points_between_segments` against an
/// axis segment, which is the caller's job (it needs the camera ray).
///
/// `symmetric` is Godot's Alt modifier. `snap_step` applies the editor's
/// translate snap when `Some`.
///
/// Returns the new size and the new world-space origin for the node. With
/// `symmetric` the origin is unchanged; otherwise the node shifts so the face
/// opposite the dragged one stays where it was.
pub fn box_set_handle(
    initial_size: Vector3,
    initial_transform: &Transform3D,
    handle_id: usize,
    ra: Vector3,
    symmetric: bool,
    snap_step: Option<f32>,
) -> (Vector3, Vector3) {
    let (axis, sign) = box_handle_axis_sign(handle_id);

    let mut neg_end = axis_get(initial_size, axis) * -0.5;
    let mut pos_end = axis_get(initial_size, axis) * 0.5;
    let ra_axis = axis_get(ra, axis);

    // Calculate new size.
    let mut box_size = initial_size;
    let new_extent = if symmetric {
        ra_axis * sign * 2.0
    } else if sign > 0.0 {
        ra_axis - neg_end
    } else {
        pos_end - ra_axis
    };
    axis_set(&mut box_size, axis, new_extent);

    // Snap to grid. Each new extent is computed into a local BEFORE the
    // `&mut box_size` borrow — reading `box_size` inside the same argument list
    // is E0503 (two-phase borrows cover `v.push(v.len())`-style method autoref,
    // not an explicit `&mut` passed to a free function).
    if let Some(step) = snap_step {
        let snapped_extent = snapped(axis_get(box_size, axis), step);
        axis_set(&mut box_size, axis, snapped_extent);
    }
    let clamped_extent = axis_get(box_size, axis).max(MIN_BOX_EXTENT);
    axis_set(&mut box_size, axis, clamped_extent);

    // Adjust position.
    let box_position = if symmetric {
        initial_transform.origin
    } else {
        if sign > 0.0 {
            pos_end = neg_end + axis_get(box_size, axis);
        } else {
            neg_end = pos_end - axis_get(box_size, axis);
        }
        let mut offset = Vector3::ZERO;
        axis_set(&mut offset, axis, (pos_end + neg_end) * 0.5);
        initial_transform.xform(offset)
    };

    (box_size, box_position)
}

/// Resolves a `BoxShape3D` face-handle drag on `node_id` into UNDOABLE editor
/// commands (pat-0q2kx.173).
///
/// This is the wiring the handle math above was missing: `box_get_handles` /
/// `box_set_handle` were a faithful Godot port with tests and **zero callers**,
/// so no drag could ever reach them.
///
/// Returns `(size_command, position_command)`. Both are plain
/// [`SetProperty`](crate::EditorCommand::SetProperty) commands, which buys three
/// things at once rather than needing a bespoke command variant:
///
/// * undo/redo for free, via the existing stack;
/// * persistence for free — a `CollisionShape3D`'s shape sub-resource is
///   flattened onto the node as `__shape__*`, and `scene_saver` already writes
///   those companions, so the new size survives a `.tscn` round trip;
/// * the renderer already reads `__shape__size`, so the wireframe follows the
///   drag with no extra plumbing.
///
/// `ra` is the point on the dragged handle's axis closest to the mouse ray, in
/// the shape's LOCAL space — the caller computes it because it needs the camera.
/// Returns `None` when the node has no box size to edit.
pub fn box_handle_drag_commands(
    tree: &gdscene::scene_tree::SceneTree,
    node_id: gdscene::node::NodeId,
    handle_id: usize,
    ra: Vector3,
    symmetric: bool,
    snap_step: Option<f32>,
) -> Option<(crate::EditorCommand, crate::EditorCommand)> {
    if handle_id >= BOX_HANDLE_COUNT {
        return None;
    }
    let node = tree.get_node(node_id)?;
    let initial_size = match node.get_property("__shape__size") {
        gdvariant::Variant::Vector3(v) => v,
        // A node with no recorded size is not a box we can edit; refusing is
        // better than inventing a default and silently resizing to it.
        _ => return None,
    };
    let initial_transform = gdscene::node3d::get_global_transform(tree, node_id);

    let (new_size, new_position) = box_set_handle(
        initial_size,
        &initial_transform,
        handle_id,
        ra,
        symmetric,
        snap_step,
    );

    Some((
        crate::EditorCommand::SetProperty {
            node_id,
            property: "__shape__size".to_string(),
            new_value: gdvariant::Variant::Vector3(new_size),
            old_value: gdvariant::Variant::Nil,
        },
        // A non-symmetric drag moves one face, so the node re-centres to keep
        // the opposite face fixed — that shift is a real position edit and has
        // to be undoable alongside the size.
        crate::EditorCommand::SetProperty {
            node_id,
            property: "position".to_string(),
            new_value: gdvariant::Variant::Vector3(new_position),
            old_value: gdvariant::Variant::Nil,
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity() -> Transform3D {
        Transform3D::IDENTITY
    }

    /// Handles sit at the six face centres in Godot's id order, at half-size.
    #[test]
    fn box_handles_are_face_centres_in_godot_id_order() {
        let handles = box_get_handles(Vector3::new(2.0, 4.0, 6.0));

        assert_eq!(handles[0], Vector3::new(1.0, 0.0, 0.0), "id 0 is +X");
        assert_eq!(handles[1], Vector3::new(-1.0, 0.0, 0.0), "id 1 is -X");
        assert_eq!(handles[2], Vector3::new(0.0, 2.0, 0.0), "id 2 is +Y");
        assert_eq!(handles[3], Vector3::new(0.0, -2.0, 0.0), "id 3 is -Y");
        assert_eq!(handles[4], Vector3::new(0.0, 0.0, 3.0), "id 4 is +Z");
        assert_eq!(handles[5], Vector3::new(0.0, 0.0, -3.0), "id 5 is -Z");
    }

    /// The id→(axis, sign) packing must agree with the handle order above, or a
    /// drag on +X would resize Y.
    #[test]
    fn handle_id_maps_to_axis_and_sign() {
        assert_eq!(box_handle_axis_sign(0), (0, 1.0));
        assert_eq!(box_handle_axis_sign(1), (0, -1.0));
        assert_eq!(box_handle_axis_sign(2), (1, 1.0));
        assert_eq!(box_handle_axis_sign(3), (1, -1.0));
        assert_eq!(box_handle_axis_sign(4), (2, 1.0));
        assert_eq!(box_handle_axis_sign(5), (2, -1.0));
    }

    /// Acceptance core: dragging the +X handle outward grows `size.x` and only
    /// `size.x`. Box size 2 → `neg_end = -1`; dragging to `ra.x = 3` gives
    /// `size.x = 3 - (-1) = 4`.
    #[test]
    fn dragging_positive_x_handle_grows_size_x() {
        let (size, _) = box_set_handle(
            Vector3::new(2.0, 2.0, 2.0),
            &identity(),
            0,
            Vector3::new(3.0, 0.0, 0.0),
            false,
            None,
        );

        assert_eq!(size.x, 4.0, "the dragged axis takes the new extent");
        assert_eq!(size.y, 2.0, "other axes are untouched");
        assert_eq!(size.z, 2.0, "other axes are untouched");
    }

    /// A plain drag keeps the OPPOSITE face fixed and re-centres the node —
    /// Godot returns a position for this reason. Growing +X from 2 to 4 leaves
    /// the -X face at -1, so the centre moves from 0 to +1.
    #[test]
    fn plain_drag_holds_opposite_face_and_recentres() {
        let (size, position) = box_set_handle(
            Vector3::new(2.0, 2.0, 2.0),
            &identity(),
            0,
            Vector3::new(3.0, 0.0, 0.0),
            false,
            None,
        );

        assert_eq!(size.x, 4.0);
        assert_eq!(
            position,
            Vector3::new(1.0, 0.0, 0.0),
            "centre shifts by half the growth so the -X face stays at -1"
        );
        // The -X face really did not move: centre - size/2 == -1.
        assert_eq!(position.x - size.x / 2.0, -1.0);
    }

    /// Alt resizes symmetrically about the origin and leaves the node put.
    /// `size.x = ra.x * sign * 2 = 3 * 1 * 2 = 6`.
    #[test]
    fn alt_drag_is_symmetric_and_keeps_position() {
        let (size, position) = box_set_handle(
            Vector3::new(2.0, 2.0, 2.0),
            &identity(),
            0,
            Vector3::new(3.0, 0.0, 0.0),
            true,
            None,
        );

        assert_eq!(size.x, 6.0, "symmetric drag doubles the half-extent");
        assert_eq!(
            position,
            Vector3::ZERO,
            "symmetric resize does not move the node"
        );
    }

    /// Dragging the -X handle: `size.x = pos_end - ra.x = 1 - (-3) = 4`, and
    /// the +X face stays at +1 so the centre moves to -1.
    #[test]
    fn dragging_negative_x_handle_mirrors_the_positive_case() {
        let (size, position) = box_set_handle(
            Vector3::new(2.0, 2.0, 2.0),
            &identity(),
            1,
            Vector3::new(-3.0, 0.0, 0.0),
            false,
            None,
        );

        assert_eq!(size.x, 4.0);
        assert_eq!(position, Vector3::new(-1.0, 0.0, 0.0));
        assert_eq!(position.x + size.x / 2.0, 1.0, "+X face stayed at +1");
    }

    /// A drag that would invert the box clamps to Godot's floor instead of
    /// going negative.
    #[test]
    fn collapsing_a_handle_clamps_to_min_extent() {
        let (size, _) = box_set_handle(
            Vector3::new(2.0, 2.0, 2.0),
            &identity(),
            0,
            Vector3::new(-50.0, 0.0, 0.0),
            false,
            None,
        );

        assert_eq!(size.x, MIN_BOX_EXTENT, "size never goes negative");
    }

    /// Snap rounds the resulting extent to the translate-snap step.
    #[test]
    fn snap_rounds_the_new_extent() {
        // ra.x = 2.6 → size.x = 2.6 - (-1) = 3.6 → snapped(3.6, 1.0) = 4.0
        let (size, _) = box_set_handle(
            Vector3::new(2.0, 2.0, 2.0),
            &identity(),
            0,
            Vector3::new(2.6, 0.0, 0.0),
            false,
            Some(1.0),
        );

        assert_eq!(size.x, 4.0, "3.6 snaps to the nearest whole step");
    }

    /// Godot's `snapped` is a no-op at step 0 rather than dividing by zero.
    #[test]
    fn snap_step_zero_is_a_no_op() {
        let (size, _) = box_set_handle(
            Vector3::new(2.0, 2.0, 2.0),
            &identity(),
            0,
            Vector3::new(3.0, 0.0, 0.0),
            false,
            Some(0.0),
        );

        assert_eq!(size.x, 4.0);
    }

    /// The returned position is in world space: a translated node's re-centre
    /// is pushed through its transform, not left in local space.
    #[test]
    fn recentred_position_is_world_space() {
        let mut xf = Transform3D::IDENTITY;
        xf.origin = Vector3::new(10.0, 0.0, 0.0);

        let (size, position) = box_set_handle(
            Vector3::new(2.0, 2.0, 2.0),
            &xf,
            0,
            Vector3::new(3.0, 0.0, 0.0),
            false,
            None,
        );

        assert_eq!(size.x, 4.0);
        assert_eq!(
            position,
            Vector3::new(11.0, 0.0, 0.0),
            "local offset +1 is xformed through the node's transform"
        );
    }

    /// Y and Z drags use their own axis — a regression guard for the id/2 packing.
    #[test]
    fn drags_on_y_and_z_axes_resize_their_own_axis() {
        let (size_y, _) = box_set_handle(
            Vector3::new(2.0, 2.0, 2.0),
            &identity(),
            2,
            Vector3::new(0.0, 3.0, 0.0),
            false,
            None,
        );
        assert_eq!((size_y.x, size_y.y, size_y.z), (2.0, 4.0, 2.0));

        let (size_z, _) = box_set_handle(
            Vector3::new(2.0, 2.0, 2.0),
            &identity(),
            4,
            Vector3::new(0.0, 0.0, 3.0),
            false,
            None,
        );
        assert_eq!((size_z.x, size_z.y, size_z.z), (2.0, 2.0, 4.0));
    }
}
