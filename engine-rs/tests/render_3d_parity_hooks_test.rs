//! pat-sh5: Build the initial 3D render path with measurable parity hooks.
//!
//! Integration tests validating the complete 3D render pipeline has measurable
//! parity hooks in place: snapshot JSON, parity reports, frame comparison,
//! determinism, and golden-style metrics suitable for oracle comparison.
//!
//! These tests prove the render path produces structured, deterministic output
//! that can be compared against Godot 4.6.1 oracle data.

use gdcore::math::Color;
use gdcore::math::Vector3;
use gdcore::math3d::{Basis, Transform3D};
use gdrender3d::compare::{compare_framebuffers_3d, diff_image_3d};
use gdrender3d::renderer::FrameBuffer3D;
use gdrender3d::test_adapter::{capture_frame_3d, count_visible_pixels};
use gdrender3d::SoftwareRenderer3D;
use gdscene::node::Node;
use gdscene::render_server_3d::RenderServer3DAdapter;
use gdscene::scene_tree::SceneTree;
use gdscene::MainLoop;
use gdserver3d::material::Material3D;
use gdserver3d::mesh::Mesh3D;
use gdserver3d::server::RenderingServer3D;
use gdserver3d::viewport::Viewport3D;
use gdvariant::Variant;

// ===========================================================================
// Helper: build representative 3D scenes matching oracle fixture structure
// ===========================================================================

/// Builds a minimal_3d-style scene: World > Camera3D + MeshInstance3D + DirectionalLight3D.
fn build_minimal_3d_scene() -> SceneTree {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let world = Node::new("World", "Node3D");
    let world_id = tree.add_child(root, world).unwrap();

    let mut camera = Node::new("Camera3D", "Camera3D");
    camera.set_property("fov", Variant::Float(75.0));
    camera.set_property("near", Variant::Float(0.05));
    camera.set_property("far", Variant::Float(4000.0));
    camera.set_property("current", Variant::Bool(true));
    let cam_id = tree.add_child(world_id, camera).unwrap();
    gdscene::node3d::set_position(&mut tree, cam_id, Vector3::new(0.0, 2.0, 5.0));

    let cube = Node::new("MeshInstance3D", "MeshInstance3D");
    let cube_id = tree.add_child(world_id, cube).unwrap();
    gdscene::node3d::set_position(&mut tree, cube_id, Vector3::new(0.0, 0.0, 0.0));

    let mut sun = Node::new("DirectionalLight3D", "DirectionalLight3D");
    sun.set_property("light_energy", Variant::Float(1.0));
    sun.set_property("shadow_enabled", Variant::Bool(true));
    tree.add_child(world_id, sun).unwrap();

    tree
}

/// Builds a multi-light scene: Camera3D + 2 MeshInstance3D + DirectionalLight3D + OmniLight3D.
fn build_multi_light_scene() -> SceneTree {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let mut camera = Node::new("Camera3D", "Camera3D");
    camera.set_property("fov", Variant::Float(75.0));
    camera.set_property("current", Variant::Bool(true));
    let cam_id = tree.add_child(root, camera).unwrap();
    gdscene::node3d::set_position(&mut tree, cam_id, Vector3::new(0.0, 3.0, 8.0));

    let cube1 = Node::new("Cube1", "MeshInstance3D");
    let id1 = tree.add_child(root, cube1).unwrap();
    gdscene::node3d::set_position(&mut tree, id1, Vector3::new(-2.0, 0.0, 0.0));

    let cube2 = Node::new("Cube2", "MeshInstance3D");
    let id2 = tree.add_child(root, cube2).unwrap();
    gdscene::node3d::set_position(&mut tree, id2, Vector3::new(2.0, 0.0, 0.0));

    let mut sun = Node::new("Sun", "DirectionalLight3D");
    sun.set_property("light_energy", Variant::Float(1.0));
    tree.add_child(root, sun).unwrap();

    let lamp = Node::new("Lamp", "OmniLight3D");
    tree.add_child(root, lamp).unwrap();

    tree
}

// ===========================================================================
// 1. Parity snapshot JSON has all required fields for oracle comparison
// ===========================================================================

#[test]
fn snapshot_json_has_all_oracle_fields() {
    let tree = build_minimal_3d_scene();
    let mut adapter = RenderServer3DAdapter::new(64, 64);
    let (snapshot, _) = adapter.render_frame(&tree);

    let json = snapshot.to_json();

    // Every field needed for oracle comparison must be present.
    let required_fields = [
        "frame_number",
        "width",
        "height",
        "visible_mesh_count",
        "light_count",
        "nonblack_pixel_count",
        "total_pixel_count",
        "depth_written_count",
        "coverage",
        "camera_fov",
    ];
    for field in &required_fields {
        assert!(
            json.contains(&format!("\"{}\":", field)),
            "snapshot JSON missing required field '{}': {}",
            field,
            json
        );
    }
}

#[test]
fn parity_report_json_has_all_oracle_fields() {
    let tree = build_minimal_3d_scene();
    let mut adapter = RenderServer3DAdapter::new(64, 64);
    let (snapshot, _) = adapter.render_frame(&tree);

    let report = snapshot.parity_report();
    let json = report.to_json();

    let required_fields = [
        "frame_number",
        "mesh_count",
        "light_count",
        "coverage",
        "depth_coverage",
        "has_camera",
        "viewport_pixels",
        "is_functional",
    ];
    for field in &required_fields {
        assert!(
            json.contains(&format!("\"{}\":", field)),
            "parity report JSON missing required field '{}': {}",
            field,
            json
        );
    }
}

// ===========================================================================
// 2. Determinism: same scene → identical output (required for golden comparison)
// ===========================================================================

#[test]
fn render_path_deterministic_pixel_level() {
    let tree = build_minimal_3d_scene();
    let mut a = RenderServer3DAdapter::new(64, 64);
    let mut b = RenderServer3DAdapter::new(64, 64);

    a.render_frame(&tree);
    b.render_frame(&tree);

    let fa = a.last_frame().unwrap();
    let fb = b.last_frame().unwrap();

    let diff = RenderServer3DAdapter::compare_frames(fa, fb, 0.0, 0.0);
    assert!(
        diff.is_exact_color_match(),
        "two independent renders of the same scene must be pixel-identical (match ratio: {})",
        diff.color_match_ratio()
    );
}

#[test]
fn render_path_deterministic_multi_frame() {
    let tree = build_multi_light_scene();
    let mut adapter = RenderServer3DAdapter::new(64, 64);

    // Render 5 frames of the same scene — all must be identical.
    let mut snapshots = Vec::new();
    for _ in 0..5 {
        let (snap, _) = adapter.render_frame(&tree);
        snapshots.push(snap);
    }

    let first_coverage = snapshots[0].nonblack_pixel_count;
    for (i, s) in snapshots.iter().enumerate().skip(1) {
        assert_eq!(
            s.nonblack_pixel_count, first_coverage,
            "frame {} coverage differs from frame 0: {} vs {}",
            i, s.nonblack_pixel_count, first_coverage
        );
    }
}

// ===========================================================================
// 3. Parity report correctly classifies scene state
// ===========================================================================

#[test]
fn parity_report_functional_scene_classified_correctly() {
    let tree = build_minimal_3d_scene();
    let mut adapter = RenderServer3DAdapter::new(64, 64);
    let (snapshot, _) = adapter.render_frame(&tree);

    let report = snapshot.parity_report();
    assert!(report.is_functional());
    assert!(report.has_camera);
    assert_eq!(report.mesh_count, 1);
    assert_eq!(report.light_count, 1);
    assert!(report.coverage > 0.0);
    assert!(report.coverage < 1.0);
}

#[test]
fn parity_report_empty_scene_not_functional() {
    let tree = SceneTree::new();
    let mut adapter = RenderServer3DAdapter::new(32, 32);
    let (snapshot, _) = adapter.render_frame(&tree);

    let report = snapshot.parity_report();
    assert!(
        !report.is_functional(),
        "empty scene should not be functional"
    );
    assert_eq!(report.mesh_count, 0);
    assert_eq!(report.light_count, 0);
    assert_eq!(report.coverage, 0.0);
}

#[test]
fn parity_report_multi_light_scene() {
    let tree = build_multi_light_scene();
    let mut adapter = RenderServer3DAdapter::new(64, 64);
    let (snapshot, _) = adapter.render_frame(&tree);

    let report = snapshot.parity_report();
    assert!(report.is_functional());
    assert_eq!(report.mesh_count, 2);
    assert_eq!(report.light_count, 2);
}

// ===========================================================================
// 4. Frame comparison hooks detect scene changes
// ===========================================================================

#[test]
fn frame_comparison_detects_mesh_movement() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let cam = Node::new("Camera", "Camera3D");
    let cam_id = tree.add_child(root, cam).unwrap();
    gdscene::node3d::set_position(&mut tree, cam_id, Vector3::new(0.0, 0.0, 10.0));
    gdscene::node3d::set_camera_current(&mut tree, cam_id, true);

    let mesh = Node::new("Cube", "MeshInstance3D");
    let mesh_id = tree.add_child(root, mesh).unwrap();
    gdscene::node3d::set_position(&mut tree, mesh_id, Vector3::new(0.0, 0.0, 0.0));

    let mut adapter = RenderServer3DAdapter::new(64, 64);
    adapter.render_frame(&tree);
    let before = adapter.last_frame().unwrap().clone();

    // Move the mesh.
    gdscene::node3d::set_position(&mut tree, mesh_id, Vector3::new(3.0, 3.0, 0.0));
    adapter.render_frame(&tree);
    let after = adapter.last_frame().unwrap().clone();

    let diff = RenderServer3DAdapter::compare_frames(&before, &after, 0.0, 0.0);
    assert!(
        !diff.is_exact_color_match(),
        "moving a mesh must change the rendered output"
    );
    assert!(
        diff.color_match_ratio() < 1.0,
        "color match ratio should drop when mesh moves"
    );
}

#[test]
fn frame_comparison_detects_color_change() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let cam = Node::new("Camera", "Camera3D");
    let cam_id = tree.add_child(root, cam).unwrap();
    gdscene::node3d::set_position(&mut tree, cam_id, Vector3::new(0.0, 0.0, 10.0));
    gdscene::node3d::set_camera_current(&mut tree, cam_id, true);

    let mut mesh = Node::new("Cube", "MeshInstance3D");
    mesh.set_property("albedo", Variant::Color(Color::rgb(1.0, 0.0, 0.0)));
    let mesh_id = tree.add_child(root, mesh).unwrap();
    gdscene::node3d::set_position(&mut tree, mesh_id, Vector3::new(0.0, 0.0, 0.0));

    let mut adapter = RenderServer3DAdapter::new(64, 64);
    adapter.render_frame(&tree);
    let red_frame = adapter.last_frame().unwrap().clone();

    // Change to green.
    if let Some(node) = tree.get_node_mut(mesh_id) {
        node.set_property("albedo", Variant::Color(Color::rgb(0.0, 1.0, 0.0)));
    }
    adapter.render_frame(&tree);
    let green_frame = adapter.last_frame().unwrap().clone();

    let diff = RenderServer3DAdapter::compare_frames(&red_frame, &green_frame, 0.0, 0.0);
    assert!(
        !diff.is_exact_color_match(),
        "changing material color must change rendered output"
    );
}

// ===========================================================================
// 5. MainLoop 3D render integration
// ===========================================================================
// NOTE: MainLoop::enable_render_3d() and render_3d_frame() are not yet
// implemented. Tests will be added once the MainLoop 3D render path is built.
// For now, 3D rendering is tested via RenderServer3DAdapter directly.

// ===========================================================================
// 6. Low-level renderer parity hooks
// ===========================================================================

#[test]
fn software_renderer_wireframe_coverage_measurable() {
    let mut renderer = SoftwareRenderer3D::new();
    let id = renderer.create_instance();
    renderer.set_mesh(id, Mesh3D::cube(1.0));
    renderer.set_material(id, Material3D::default());
    renderer.set_transform(
        id,
        Transform3D {
            basis: Basis::IDENTITY,
            origin: Vector3::new(0.0, 0.0, -5.0),
        },
    );

    let vp = Viewport3D::new(64, 64);
    let fb = capture_frame_3d(&mut renderer, &vp);

    let visible = count_visible_pixels(&fb);
    let total = (fb.width * fb.height) as usize;
    let coverage = visible as f64 / total as f64;

    assert!(visible > 0, "wireframe must produce visible pixels");
    assert!(
        coverage < 0.5,
        "wireframe should not fill more than half the viewport"
    );
    assert!(
        coverage > 0.001,
        "wireframe coverage should be measurable (> 0.1%)"
    );
}

#[test]
fn framebuffer_comparison_exact_match_self() {
    let mut renderer = SoftwareRenderer3D::new();
    let id = renderer.create_instance();
    renderer.set_mesh(id, Mesh3D::cube(1.0));
    renderer.set_material(id, Material3D::default());
    renderer.set_transform(
        id,
        Transform3D {
            basis: Basis::IDENTITY,
            origin: Vector3::new(0.0, 0.0, -5.0),
        },
    );

    let vp = Viewport3D::new(32, 32);
    let fb = capture_frame_3d(&mut renderer, &vp);

    let diff = compare_framebuffers_3d(&fb, &fb, 0.0, 0.0);
    assert!(diff.is_exact_color_match());
    assert!(diff.is_exact_depth_match());
    assert_eq!(diff.color_match_ratio(), 1.0);
    assert_eq!(diff.depth_match_ratio(), 1.0);
}

#[test]
fn diff_image_highlights_differences_in_red() {
    let a = FrameBuffer3D::new(4, 4, Color::BLACK);
    let mut b = FrameBuffer3D::new(4, 4, Color::BLACK);
    b.set_pixel(2, 2, Color::WHITE);

    let diff = diff_image_3d(&a, &b);

    // Matching pixels → grayscale, differing pixel → red.
    let matching = diff.get_pixel(0, 0);
    let different = diff.get_pixel(2, 2);

    // Matching black pixel should be grayscale (luma of black = 0).
    assert!(matching.r < 0.01 && matching.g < 0.01 && matching.b < 0.01);

    // Different pixel should have red component.
    assert!(different.r > 0.0, "diff pixel should be red-tinted");
}

// ===========================================================================
// 7. Mesh primitive variety (parity with Godot mesh types)
// ===========================================================================

#[test]
fn sphere_and_cube_produce_different_wireframes() {
    let vp = Viewport3D::new(64, 64);
    let transform = Transform3D {
        basis: Basis::IDENTITY,
        origin: Vector3::new(0.0, 0.0, -5.0),
    };

    let mut r_cube = SoftwareRenderer3D::new();
    let id = r_cube.create_instance();
    r_cube.set_mesh(id, Mesh3D::cube(1.0));
    r_cube.set_material(id, Material3D::default());
    r_cube.set_transform(id, transform);
    let fb_cube = capture_frame_3d(&mut r_cube, &vp);

    let mut r_sphere = SoftwareRenderer3D::new();
    let id = r_sphere.create_instance();
    r_sphere.set_mesh(id, Mesh3D::sphere(1.0, 8));
    r_sphere.set_material(id, Material3D::default());
    r_sphere.set_transform(id, transform);
    let fb_sphere = capture_frame_3d(&mut r_sphere, &vp);

    let diff = compare_framebuffers_3d(&fb_cube, &fb_sphere, 0.0, 1.0);
    assert!(
        diff.color_match_ratio() < 0.99,
        "sphere and cube must produce visually different wireframes"
    );
}

#[test]
fn plane_mesh_renders_visible_wireframe() {
    let mut renderer = SoftwareRenderer3D::new();
    let id = renderer.create_instance();
    renderer.set_mesh(id, Mesh3D::plane(2.0));
    renderer.set_material(id, Material3D::default());
    renderer.set_transform(
        id,
        Transform3D {
            basis: Basis::IDENTITY,
            origin: Vector3::new(0.0, 0.0, -5.0),
        },
    );

    let vp = Viewport3D::new(64, 64);
    let fb = capture_frame_3d(&mut renderer, &vp);
    assert!(
        count_visible_pixels(&fb) > 0,
        "plane should produce wireframe pixels"
    );
}

// ===========================================================================
// 8. Coverage metrics are meaningful and bounded
// ===========================================================================

#[test]
fn coverage_bounded_zero_to_one() {
    let tree = build_minimal_3d_scene();
    let mut adapter = RenderServer3DAdapter::new(64, 64);
    let (snapshot, _) = adapter.render_frame(&tree);

    let c = snapshot.coverage();
    assert!(
        c >= 0.0 && c <= 1.0,
        "coverage must be in [0, 1], got {}",
        c
    );
}

#[test]
fn coverage_increases_with_closer_objects() {
    let mut tree1 = SceneTree::new();
    let root1 = tree1.root_id();
    let cam1 = Node::new("Camera", "Camera3D");
    let cam1_id = tree1.add_child(root1, cam1).unwrap();
    gdscene::node3d::set_position(&mut tree1, cam1_id, Vector3::new(0.0, 0.0, 10.0));
    gdscene::node3d::set_camera_current(&mut tree1, cam1_id, true);
    let cube1 = Node::new("Cube", "MeshInstance3D");
    let c1_id = tree1.add_child(root1, cube1).unwrap();
    gdscene::node3d::set_position(&mut tree1, c1_id, Vector3::new(0.0, 0.0, 0.0));

    let mut tree2 = SceneTree::new();
    let root2 = tree2.root_id();
    let cam2 = Node::new("Camera", "Camera3D");
    let cam2_id = tree2.add_child(root2, cam2).unwrap();
    gdscene::node3d::set_position(&mut tree2, cam2_id, Vector3::new(0.0, 0.0, 3.0));
    gdscene::node3d::set_camera_current(&mut tree2, cam2_id, true);
    let cube2 = Node::new("Cube", "MeshInstance3D");
    let c2_id = tree2.add_child(root2, cube2).unwrap();
    gdscene::node3d::set_position(&mut tree2, c2_id, Vector3::new(0.0, 0.0, 0.0));

    let mut a1 = RenderServer3DAdapter::new(64, 64);
    let (snap_far, _) = a1.render_frame(&tree1);

    let mut a2 = RenderServer3DAdapter::new(64, 64);
    let (snap_near, _) = a2.render_frame(&tree2);

    assert!(
        snap_near.coverage() >= snap_far.coverage(),
        "closer object should have >= coverage: near={}, far={}",
        snap_near.coverage(),
        snap_far.coverage()
    );
}

// ===========================================================================
// 9. Scene tree node removal reflected in render output
// ===========================================================================

#[test]
fn removing_mesh_node_clears_from_render() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let cam = Node::new("Camera", "Camera3D");
    let cam_id = tree.add_child(root, cam).unwrap();
    gdscene::node3d::set_position(&mut tree, cam_id, Vector3::new(0.0, 0.0, 10.0));
    gdscene::node3d::set_camera_current(&mut tree, cam_id, true);

    let mesh = Node::new("Cube", "MeshInstance3D");
    let mesh_id = tree.add_child(root, mesh).unwrap();
    gdscene::node3d::set_position(&mut tree, mesh_id, Vector3::new(0.0, 0.0, 0.0));

    let mut adapter = RenderServer3DAdapter::new(32, 32);
    let (s1, _) = adapter.render_frame(&tree);
    assert_eq!(s1.visible_mesh_count, 1);
    assert!(s1.nonblack_pixel_count > 0);

    tree.remove_node(mesh_id).unwrap();
    let (s2, _) = adapter.render_frame(&tree);
    assert_eq!(s2.visible_mesh_count, 0);
    assert_eq!(s2.nonblack_pixel_count, 0);
}

// ===========================================================================
// 10. Visibility toggle reflected in parity metrics
// ===========================================================================

#[test]
fn visibility_toggle_reflected_in_metrics() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let cam = Node::new("Camera", "Camera3D");
    let cam_id = tree.add_child(root, cam).unwrap();
    gdscene::node3d::set_position(&mut tree, cam_id, Vector3::new(0.0, 0.0, 10.0));
    gdscene::node3d::set_camera_current(&mut tree, cam_id, true);

    let mesh = Node::new("Cube", "MeshInstance3D");
    let mesh_id = tree.add_child(root, mesh).unwrap();
    gdscene::node3d::set_position(&mut tree, mesh_id, Vector3::new(0.0, 0.0, 0.0));

    let mut adapter = RenderServer3DAdapter::new(32, 32);

    // Visible.
    let (s_vis, _) = adapter.render_frame(&tree);
    assert_eq!(s_vis.visible_mesh_count, 1);
    assert!(s_vis.nonblack_pixel_count > 0);

    // Hidden.
    gdscene::node3d::set_visible(&mut tree, mesh_id, false);
    let (s_hid, _) = adapter.render_frame(&tree);
    assert_eq!(s_hid.visible_mesh_count, 0);
    assert_eq!(s_hid.nonblack_pixel_count, 0);
}
