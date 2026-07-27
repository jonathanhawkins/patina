//! pat-0q2kx.235 (partial) — the active-camera scan is inheritance-aware.
//!
//! Godot ground truth: `scene/3d/xr/xr_nodes.cpp` — `XRCamera3D` IS a
//! `Camera3D`, and a scene containing one renders normally with no XR session.
//!
//! `RenderServer3DAdapter::build_viewport` selected the active camera with
//! `class_name() == "Camera3D"`, exact string equality. `XRCamera3D` is
//! registered with parent `Camera3D`, so it could never be found — a `.tscn`
//! whose only camera was an XRCamera3D rendered from NO camera at all, silently
//! falling back to the default view.
//!
//! ⚠️ SCOPE: the camera lookup plus the `XRNode3D` instantiability fix. The
//! bead's remaining parts — registering XRPose/XRTracker/XRPositionalTracker,
//! filling in the XRController3D/XRAnchor3D surfaces, and the per-tick pose
//! sync — are NOT delivered. See the completion report.

use gdobject::class_db::{is_abstract, register_3d_classes};
use gdscene::node::Node;
use gdscene::render_server_3d::RenderServer3DAdapter;
use gdscene::scene_tree::SceneTree;
use gdvariant::Variant;

/// A scene whose only camera is `camera_class`, placed at z = 25 so the
/// published view can be attributed to it rather than to a default at origin.
fn render_with_camera(camera_class: Option<&str>) -> [f32; 12] {
    register_3d_classes();
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    if let Some(class) = camera_class {
        let cam = tree
            .add_child(root, Node::new("Cam", class))
            .expect("camera added");
        let node = tree.get_node_mut(cam).expect("node exists");
        node.set_property("current", Variant::Bool(true));
        node.set_property(
            "position",
            Variant::Vector3(gdcore::math::Vector3::new(0.0, 0.0, 25.0)),
        );
    }

    let mut adapter = RenderServer3DAdapter::new(64, 48);
    let (snapshot, _frame) = adapter.render_frame(&tree);
    snapshot.camera_transform
}

/// The predicate the fixed scan uses resolves the subclass through the ClassDB
/// chain.
#[test]
fn xr_camera_3d_is_a_camera_3d() {
    register_3d_classes();
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let cam = tree
        .add_child(root, Node::new("Cam", "XRCamera3D"))
        .expect("added");
    let node = tree.get_node(cam).expect("node exists");

    assert!(
        node.is_class("Camera3D"),
        "XRCamera3D is registered with parent Camera3D — exact name equality is \
         what the old scan got wrong"
    );
    assert!(node.is_class("XRCamera3D"), "and it is its own class");
    assert!(
        !node.is_class("Camera2D"),
        "the check must not be a blanket true"
    );
}

/// **The fix.** An XRCamera3D drives the published view exactly as a Camera3D
/// in the same place does, and differs from the no-camera fallback.
#[test]
fn an_xr_camera_drives_the_view_like_a_plain_camera() {
    let xr = render_with_camera(Some("XRCamera3D"));
    let plain = render_with_camera(Some("Camera3D"));
    let none = render_with_camera(None);

    assert_eq!(
        xr, plain,
        "Godot renders XRCamera3D as an ordinary camera with no XR session, so \
         the published transform must match a Camera3D at the same position"
    );

    // The comparison above is only meaningful if a camera actually changes the
    // result — otherwise both could be the default and still be equal.
    assert_ne!(
        plain, none,
        "precondition: an authored camera must change the published transform, \
         or the assertion above proves nothing"
    );
    assert_ne!(
        xr, none,
        "the XR camera must have been FOUND — before the fix this equalled the \
         no-camera fallback"
    );
}

/// `XRNode3D` is instantiable in Godot; it was wrongly marked abstract here.
#[test]
fn the_xr_node_family_is_not_abstract() {
    register_3d_classes();

    assert!(
        !is_abstract("XRNode3D"),
        "GDREGISTER_CLASS(XRNode3D) — marking it abstract makes a node Godot \
         lets you place in a scene un-instantiable"
    );
    for c in ["XRCamera3D", "XRController3D", "XRAnchor3D", "XROrigin3D"] {
        assert!(!is_abstract(c), "{c} is GDREGISTER_CLASS in Godot");
    }
}
