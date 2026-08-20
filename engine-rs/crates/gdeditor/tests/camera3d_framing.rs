//! pat-s3rrv.186 — a scene `Camera3D`'s `near`/`far`, `keep_aspect` and
//! `h_offset`/`v_offset` frame the Play view the way Godot's game window does.
//!
//! Godot ground truth: `scene/3d/camera_3d.cpp` — `_get_camera_projection`
//! (:274) passes `keep_aspect == KEEP_WIDTH` as `set_perspective`'s `flip_fov`,
//! and `_get_adjusted_camera_transform` (:253-258) shifts the origin by
//! `basis.column(1) * v_offset` and `basis.column(0) * h_offset`.
//!
//! ⚠️ THREE separate things had to connect for any of this to be observable,
//! and each was independently inert:
//!
//! 1. `viewport_for_scene_camera` copied only the transform, projection type,
//!    `fov` and `size` — near/far/keep_aspect/offsets were never read.
//! 2. `editor_render_viewport` then HARDCODED `near = 0.05` and `far = 2000.0`,
//!    so even a populated camera would have been overwritten.
//! 3. `clip_to_screen` rejected only `w <= 0` (behind the camera), so the clip
//!    planes changed the depth VALUE and hid nothing at all.
//!
//! Assertions are differential — each frame is compared against the same scene
//! with no mesh — because a "the mesh is gone" claim must not be satisfiable by
//! a frame that was empty for some unrelated reason.

use gdcore::math::Vector3;
use gdcore::math3d::{Basis, Transform3D};
use gdeditor::viewport_3d_render::render_game_3d_frame;
use gdrender2d::renderer::FrameBuffer;
use gdscene::node::Node;
use gdscene::scene_tree::SceneTree;
use gdvariant::Variant;

/// Deliberately non-square: `keep_aspect` only changes framing when the
/// viewport's aspect is not 1.
const W: u32 = 128;
const H: u32 = 64;

/// Builds a Play frame from a camera at the origin looking down −Z.
///
/// `mesh_z` places a single `MeshInstance3D` that far in front of the camera;
/// `None` renders the scene with no mesh at all, which is the baseline every
/// assertion compares against.
fn frame(
    mesh_z: Option<f32>,
    near: f64,
    far: f64,
    keep_aspect: Option<i64>,
    h_offset: f64,
) -> FrameBuffer {
    // The 3D render list filters on `is_class("Node3D")`, which reads the
    // ClassDB — without this every list comes back empty and each assertion
    // below would pass vacuously.
    gdobject::class_db::register_3d_classes();

    let mut tree = SceneTree::new();
    let root = tree.root_id();

    if let Some(z) = mesh_z {
        let mesh = tree
            .add_child(root, Node::new("Box", "MeshInstance3D"))
            .unwrap();
        tree.get_node_mut(mesh).unwrap().set_property(
            "transform",
            Variant::Transform3D(Transform3D {
                basis: Basis::IDENTITY,
                // Camera looks down −Z, so a mesh in front sits at negative z.
                origin: Vector3::new(0.0, 0.0, -z),
            }),
        );
    }
    tree.add_child(root, Node::new("Sun", "DirectionalLight3D"))
        .unwrap();

    let camera = tree.add_child(root, Node::new("Cam", "Camera3D")).unwrap();
    let node = tree.get_node_mut(camera).unwrap();
    node.set_property(
        "transform",
        Variant::Transform3D(Transform3D {
            basis: Basis::IDENTITY,
            origin: Vector3::ZERO,
        }),
    );
    node.set_property("current", Variant::Bool(true));
    node.set_property("fov", Variant::Float(60.0));
    node.set_property("near", Variant::Float(near));
    node.set_property("far", Variant::Float(far));
    node.set_property("h_offset", Variant::Float(h_offset));
    if let Some(k) = keep_aspect {
        node.set_property("keep_aspect", Variant::Int(k));
    }

    render_game_3d_frame(&tree, W, H).expect("a scene with a Camera3D renders")
}

/// Pixels that differ between two frames.
fn differing(a: &FrameBuffer, b: &FrameBuffer) -> usize {
    a.pixels
        .iter()
        .zip(b.pixels.iter())
        .filter(|(p, q)| {
            (p.r - q.r).abs() > 0.01 || (p.g - q.g).abs() > 0.01 || (p.b - q.b).abs() > 0.01
        })
        .count()
}

/// **The acceptance's near/far clause.** With `near = 2` and `far = 10`, a mesh
/// at distance 5 draws while meshes at 1 and 20 are clipped away entirely.
#[test]
fn camera3d_near_and_far_clip_the_play_frame() {
    let empty = frame(None, 2.0, 10.0, None, 0.0);

    let inside = frame(Some(5.0), 2.0, 10.0, None, 0.0);
    assert!(
        differing(&inside, &empty) > 0,
        "a mesh between the near and far planes must be visible — otherwise the \
         two assertions below prove nothing"
    );

    let too_near = frame(Some(1.0), 2.0, 10.0, None, 0.0);
    assert_eq!(
        differing(&too_near, &empty),
        0,
        "a mesh closer than `near` must be clipped out of the frame"
    );

    let too_far = frame(Some(20.0), 2.0, 10.0, None, 0.0);
    assert_eq!(
        differing(&too_far, &empty),
        0,
        "a mesh beyond `far` must be clipped out of the frame"
    );
}

/// A generous `far` keeps the distant mesh, proving the clip above tracked the
/// authored value rather than some fixed limit.
///
/// This is the control for the previous test: the old code hardcoded
/// `far = 2000`, so a mesh at 20 was ALWAYS visible and a mesh at 5 always was
/// too — the near/far assertions would have been meaningless without showing
/// that the same mesh changes visibility with the property.
#[test]
fn the_same_distant_mesh_is_visible_when_far_allows_it() {
    let empty = frame(None, 0.05, 100.0, None, 0.0);
    let far_mesh = frame(Some(20.0), 0.05, 100.0, None, 0.0);
    assert!(
        differing(&far_mesh, &empty) > 0,
        "with far = 100 the mesh at distance 20 must draw; if it does not, the \
         clip test above is measuring something other than `far`"
    );
}

/// **The acceptance's keep_aspect clause.** On a non-square viewport,
/// KEEP_WIDTH frames differently from KEEP_HEIGHT.
///
/// Godot treats `fov` as vertical under KEEP_HEIGHT and horizontal under
/// KEEP_WIDTH, so the same authored angle covers a different amount of world.
#[test]
fn keep_width_frames_differently_from_keep_height() {
    // KEEP_WIDTH = 0, KEEP_HEIGHT = 1 (doc/classes/Camera3D.xml).
    let keep_height = frame(Some(5.0), 0.05, 100.0, Some(1), 0.0);
    let keep_width = frame(Some(5.0), 0.05, 100.0, Some(0), 0.0);

    assert!(
        differing(&keep_width, &keep_height) > 0,
        "on a {W}x{H} viewport the two keep_aspect modes must frame differently"
    );
}

/// **The acceptance's h_offset clause.** A horizontal offset shifts the image.
#[test]
fn h_offset_shifts_the_image() {
    let centered = frame(Some(5.0), 0.05, 100.0, None, 0.0);
    let shifted = frame(Some(5.0), 0.05, 100.0, None, 1.0);

    assert!(
        differing(&shifted, &centered) > 0,
        "h_offset must move the view; it shifts the camera along its own X axis"
    );
}
