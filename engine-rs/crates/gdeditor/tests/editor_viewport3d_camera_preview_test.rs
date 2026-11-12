//! pat-nzgdu.173 — Camera3D preview: the editor renders a PNG from a selected
//! `Camera3D`'s viewpoint (distinct from the editor's orbit camera), shown as a
//! picture-in-picture inset, and the HTML references the endpoint that serves it.

use gdcore::math::Vector3;
use gdcore::math3d::{Basis, Transform3D};
use gdeditor::editor_ui::EDITOR_HTML;
use gdeditor::viewport_3d::ViewportCamera3D;
use gdeditor::viewport_3d_render::render_camera3d_preview;
use gdscene::node::Node;
use gdscene::scene_tree::SceneTree;
use gdvariant::Variant;

#[test]
fn editor_viewport3d_camera_preview_test() {
    // (1) The editor references the camera-preview endpoint (the inset fetches it).
    assert!(
        EDITOR_HTML.contains("/api/viewport/3d/camera_preview"),
        "EDITOR_HTML must reference the camera-preview endpoint",
    );

    // (2) A scene with a Camera3D at a known transform renders a preview frame
    // from that camera. The camera sits at (0, 0, 8) looking down -Z at a box.
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    tree.add_child(root, Node::new("Box", "MeshInstance3D"))
        .unwrap();
    let cam_id = tree.add_child(root, Node::new("Cam", "Camera3D")).unwrap();
    let cam_tf = Transform3D {
        basis: Basis::IDENTITY,
        origin: Vector3::new(0.0, 0.0, 8.0),
    };
    tree.get_node_mut(cam_id)
        .unwrap()
        .set_property("transform", Variant::Transform3D(cam_tf));

    let fb = render_camera3d_preview(&tree, cam_id, 96, 72);
    assert_eq!(fb.width, 96, "preview is the requested width");
    assert_eq!(fb.height, 72, "preview is the requested height");
    assert!(!fb.pixels.is_empty(), "a rendered frame, not empty");
}

/// The transform override pins the preview camera to an explicit world transform
/// — the mechanism `render_camera3d_preview` uses to render "from that camera,
/// not the editor's orbit camera".
#[test]
fn camera_override_transform_overrides_the_orbit_camera() {
    let mut cam = ViewportCamera3D::default();
    let editor_tf = cam.transform();

    let target = Transform3D {
        basis: Basis::IDENTITY,
        origin: Vector3::new(3.0, 4.0, 5.0),
    };
    cam.override_transform = Some(target);

    assert_eq!(
        cam.transform(),
        target,
        "the override wins over the orbit derivation",
    );
    assert_ne!(
        cam.transform(),
        editor_tf,
        "the preview camera is distinct from the editor orbit camera",
    );
    // view_transform is the inverse, exactly as the software renderer expects.
    assert_eq!(cam.view_transform(), target.inverse());
}
