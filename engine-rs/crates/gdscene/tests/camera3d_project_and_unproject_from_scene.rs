//! pat-nzgdu.55 — Camera3D projection/frustum module is compiled and wired, so
//! a scene-loaded Camera3D can project screen points into world rays and
//! unproject world points back to the screen.
//!
//! Godot ground truth: `Camera3D.project_ray_origin` / `project_ray_normal`
//! return the world-space ray through a screen pixel, and
//! `unproject_position` maps a world point back to that pixel. The two are
//! inverse operations: unprojecting a point that lies on the ray for screen
//! point `S` returns `S` again. An orthographic camera's `size` sets the world
//! extent that fills the viewport, so it controls the on-screen scale.
//!
//! Before pat-nzgdu.55 the full `camera3d` implementation (frustum, projection
//! modes, keep_aspect, doppler) existed in `camera3d.rs` but was never declared
//! as a module, so none of it compiled or ran. This test exercises the now-live
//! module end to end against a `SceneTree`.

use gdcore::math::{Vector2, Vector3};
use gdscene::camera3d::ProjectionType;
use gdscene::node::Node;
use gdscene::scene_tree::SceneTree;
use gdscene::{camera3d, node3d};

/// Build a fresh tree with a single Camera3D child and return its id. The
/// camera sits at `(0, 0, 5)` with the default (identity) basis, so it looks
/// down -Z in Godot's convention.
fn scene_with_camera() -> (SceneTree, gdscene::NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let cam = tree
        .add_child(root, Node::new("Camera3D", "Camera3D"))
        .unwrap();
    node3d::set_position(&mut tree, cam, Vector3::new(0.0, 0.0, 5.0));
    node3d::set_fov(&mut tree, cam, 70.0);
    node3d::set_near(&mut tree, cam, 0.05);
    node3d::set_far(&mut tree, cam, 100.0);
    (tree, cam)
}

#[test]
fn camera3d_project_and_unproject_from_scene() {
    let (mut tree, cam) = scene_with_camera();
    camera3d::set_projection_type(&mut tree, cam, ProjectionType::Perspective);

    let (w, h) = (800u32, 600u32);

    // A known world point in front of the perspective camera.
    let p = Vector3::new(1.0, 0.5, 0.0);
    let screen: Vector2 = camera3d::unproject_position(&tree, cam, p, w, h);

    // The screen point must land inside the viewport (the point is on-screen).
    assert!(
        screen.x > 0.0 && screen.x < w as f32 && screen.y > 0.0 && screen.y < h as f32,
        "unprojected point should be on-screen, got {screen:?}"
    );

    // The ray for that screen point originates at the camera (perspective) and
    // points from the camera toward `p`.
    let origin = camera3d::project_ray_origin(&tree, cam, screen, w, h);
    let dir = camera3d::project_ray_normal(&tree, cam, screen, w, h);
    assert!(
        (origin - Vector3::new(0.0, 0.0, 5.0)).length() < 1e-3,
        "perspective ray origin is the camera position, got {origin:?}"
    );
    let expected_dir = (p - origin).normalized();
    assert!(
        (dir - expected_dir).length() < 1e-3,
        "ray must point from the camera toward p: dir={dir:?} expected={expected_dir:?}"
    );

    // Round-trip: a point on the ray at p's distance unprojects back to `screen`.
    let dist = (p - origin).length();
    let on_ray = origin + dir * dist;
    let screen_again = camera3d::unproject_position(&tree, cam, on_ray, w, h);
    assert!(
        (screen_again - screen).length() < 0.5,
        "project/unproject must round-trip the screen point: {screen_again:?} vs {screen:?}"
    );

    // --- Orthographic: `size` controls the on-screen scale. ---
    camera3d::set_projection_type(&mut tree, cam, ProjectionType::Orthogonal);

    // Screen-space distance covered by a fixed 1-unit world offset, at two sizes.
    let vertical_span = |tree: &SceneTree| -> f32 {
        let a = camera3d::unproject_position(tree, cam, Vector3::new(0.0, 0.0, 0.0), w, h);
        let b = camera3d::unproject_position(tree, cam, Vector3::new(0.0, 1.0, 0.0), w, h);
        (b - a).length()
    };

    camera3d::set_size(&mut tree, cam, 10.0);
    let span_small_size = vertical_span(&tree);
    camera3d::set_size(&mut tree, cam, 20.0);
    let span_large_size = vertical_span(&tree);

    assert!(
        span_small_size > 1.0,
        "an orthographic camera must give a non-degenerate on-screen scale, got {span_small_size}"
    );
    // Doubling `size` halves the on-screen size of the same world offset.
    assert!(
        (span_small_size - 2.0 * span_large_size).abs() < 0.5,
        "orthographic size must scale inversely: size=10 -> {span_small_size}px, size=20 -> {span_large_size}px"
    );

    // Orthographic rays are parallel: every screen point shares the camera's
    // forward (-Z under the identity basis).
    let ortho_dir = camera3d::project_ray_normal(&tree, cam, Vector2::new(10.0, 10.0), w, h);
    assert!(
        (ortho_dir - Vector3::new(0.0, 0.0, -1.0)).length() < 1e-3,
        "orthographic ray direction is the camera forward -Z, got {ortho_dir:?}"
    );
}
