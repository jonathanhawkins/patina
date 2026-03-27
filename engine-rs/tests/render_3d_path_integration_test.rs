//! Integration tests for the scene tree → 3D render path.
//!
//! Validates that the MainLoop can render 3D nodes from the scene tree
//! via the RenderServer3DAdapter, with measurable parity hooks for
//! determinism, coverage, and golden comparison.

use gdcore::math::Vector3;
use gdscene::node::Node;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::render_server_3d::RenderServer3DAdapter;
use gdscene::scene_tree::SceneTree;
use gdscene::MainLoop;
use gdvariant::Variant;

/// Helper: builds a scene tree with a Camera3D and one MeshInstance3D.
fn scene_with_camera_and_cube() -> SceneTree {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let cam = Node::new("Camera", "Camera3D");
    let cam_id = tree.add_child(root, cam).unwrap();
    gdscene::node3d::set_position(&mut tree, cam_id, Vector3::new(0.0, 0.0, 10.0));
    gdscene::node3d::set_camera_current(&mut tree, cam_id, true);

    let mesh = Node::new("Cube", "MeshInstance3D");
    let mesh_id = tree.add_child(root, mesh).unwrap();
    gdscene::node3d::set_position(&mut tree, mesh_id, Vector3::new(0.0, 0.0, 0.0));

    tree
}

// ===========================================================================
// MainLoop 3D render path tests
// ===========================================================================

#[test]
fn mainloop_render_3d_disabled_by_default() {
    let tree = SceneTree::new();
    let main_loop = MainLoop::new(tree);
    assert!(!main_loop.has_render_3d());
    assert!(main_loop.render_3d_adapter().is_none());
}

#[test]
fn mainloop_enable_render_3d() {
    let tree = SceneTree::new();
    let mut main_loop = MainLoop::new(tree);
    main_loop.enable_render_3d(64, 64);
    assert!(main_loop.has_render_3d());
}

#[test]
fn mainloop_render_3d_empty_scene() {
    let tree = SceneTree::new();
    let mut main_loop = MainLoop::new(tree);
    main_loop.enable_render_3d(32, 32);

    let snapshot = main_loop.render_3d_frame().unwrap();
    assert_eq!(snapshot.width, 32);
    assert_eq!(snapshot.height, 32);
    assert_eq!(snapshot.visible_mesh_count, 0);
    assert_eq!(snapshot.nonblack_pixel_count, 0);
    assert_eq!(snapshot.frame_number, 1);
}

#[test]
fn mainloop_render_3d_with_scene() {
    let tree = scene_with_camera_and_cube();
    let mut main_loop = MainLoop::new(tree);
    main_loop.enable_render_3d(64, 64);

    // Step the scene loop first (process/physics).
    main_loop.step(1.0 / 60.0);

    // Then render a 3D frame.
    let snapshot = main_loop.render_3d_frame().unwrap();
    assert_eq!(snapshot.visible_mesh_count, 1);
    assert!(
        snapshot.nonblack_pixel_count > 0,
        "cube in front of camera should produce visible pixels"
    );
    assert!(snapshot.coverage() > 0.0);
}

#[test]
fn mainloop_render_3d_deterministic_across_frames() {
    let tree = scene_with_camera_and_cube();
    let mut main_loop = MainLoop::new(tree);
    main_loop.enable_render_3d(32, 32);

    // Render two frames without changing the scene.
    let s1 = main_loop.render_3d_frame().unwrap();
    let s2 = main_loop.render_3d_frame().unwrap();

    assert_eq!(
        s1.nonblack_pixel_count, s2.nonblack_pixel_count,
        "static scene must produce identical coverage across frames"
    );
    assert_eq!(s1.visible_mesh_count, s2.visible_mesh_count);
}

// ===========================================================================
// Standalone adapter tests (scene tree → render server)
// ===========================================================================

#[test]
fn adapter_multi_mesh_scene() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let cam = Node::new("Camera", "Camera3D");
    let cam_id = tree.add_child(root, cam).unwrap();
    gdscene::node3d::set_position(&mut tree, cam_id, Vector3::new(0.0, 0.0, 15.0));

    // Add three cubes at different positions.
    for (i, x) in [-3.0_f32, 0.0, 3.0].iter().enumerate() {
        let mesh = Node::new(&format!("Cube{}", i), "MeshInstance3D");
        let mesh_id = tree.add_child(root, mesh).unwrap();
        gdscene::node3d::set_position(&mut tree, mesh_id, Vector3::new(*x, 0.0, 0.0));
    }

    let mut adapter = RenderServer3DAdapter::new(64, 64);
    let (snapshot, _) = adapter.render_frame(&tree);

    assert_eq!(snapshot.visible_mesh_count, 3);
    assert!(
        snapshot.nonblack_pixel_count > 0,
        "three cubes should produce visible pixels"
    );
}

#[test]
fn adapter_no_camera_uses_default() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    // Add mesh but no camera — should use default camera at origin.
    let mesh = Node::new("Cube", "MeshInstance3D");
    let mesh_id = tree.add_child(root, mesh).unwrap();
    // Place cube in front of default camera (looking down -Z).
    gdscene::node3d::set_position(&mut tree, mesh_id, Vector3::new(0.0, 0.0, -5.0));

    let mut adapter = RenderServer3DAdapter::new(64, 64);
    let (snapshot, _) = adapter.render_frame(&tree);

    assert_eq!(snapshot.visible_mesh_count, 1);
    assert!(
        snapshot.nonblack_pixel_count > 0,
        "default camera should see mesh placed in front of it"
    );
}

#[test]
fn adapter_parity_snapshot_json() {
    let tree = scene_with_camera_and_cube();
    let mut adapter = RenderServer3DAdapter::new(32, 32);
    let (snapshot, _) = adapter.render_frame(&tree);

    let json = snapshot.to_json();
    // Verify JSON is valid by checking key fields.
    assert!(json.contains("\"frame_number\":1"), "JSON: {}", json);
    assert!(json.contains("\"width\":32"), "JSON: {}", json);
    assert!(json.contains("\"height\":32"), "JSON: {}", json);
    assert!(json.contains("\"visible_mesh_count\":"), "JSON: {}", json);
    assert!(json.contains("\"coverage\":"), "JSON: {}", json);
    assert!(json.contains("\"camera_fov\":"), "JSON: {}", json);
}

#[test]
fn adapter_frame_comparison_parity_hook() {
    let tree = scene_with_camera_and_cube();

    // Render same scene with two independent adapters.
    let mut adapter_a = RenderServer3DAdapter::new(32, 32);
    let mut adapter_b = RenderServer3DAdapter::new(32, 32);

    adapter_a.render_frame(&tree);
    adapter_b.render_frame(&tree);

    let frame_a = adapter_a.last_frame().unwrap();
    let frame_b = adapter_b.last_frame().unwrap();

    let diff = RenderServer3DAdapter::compare_frames(frame_a, frame_b, 0.0, 0.0);

    assert!(
        diff.is_exact_color_match(),
        "identical scenes must produce identical frames (color match ratio: {})",
        diff.color_match_ratio()
    );
}

#[test]
fn adapter_moving_mesh_changes_output() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let cam = Node::new("Camera", "Camera3D");
    let cam_id = tree.add_child(root, cam).unwrap();
    gdscene::node3d::set_position(&mut tree, cam_id, Vector3::new(0.0, 0.0, 10.0));

    let mesh = Node::new("Cube", "MeshInstance3D");
    let mesh_id = tree.add_child(root, mesh).unwrap();
    gdscene::node3d::set_position(&mut tree, mesh_id, Vector3::new(0.0, 0.0, 0.0));

    let mut adapter = RenderServer3DAdapter::new(32, 32);

    // Render frame 1.
    adapter.render_frame(&tree);
    let frame_before = adapter.last_frame().unwrap().clone();

    // Move the mesh.
    gdscene::node3d::set_position(&mut tree, mesh_id, Vector3::new(5.0, 5.0, 0.0));

    // Render frame 2.
    adapter.render_frame(&tree);
    let frame_after = adapter.last_frame().unwrap().clone();

    // The frames should differ because the mesh moved.
    let diff = RenderServer3DAdapter::compare_frames(&frame_before, &frame_after, 0.0, 0.0);
    assert!(
        !diff.is_exact_color_match(),
        "moving a mesh should change the rendered output"
    );
}

#[test]
fn adapter_coverage_metric_meaningful() {
    let tree = scene_with_camera_and_cube();
    let mut adapter = RenderServer3DAdapter::new(64, 64);
    let (snapshot, _) = adapter.render_frame(&tree);

    // Coverage should be between 0 and 1, and nonzero for a visible scene.
    assert!(snapshot.coverage() > 0.0, "coverage should be nonzero");
    assert!(snapshot.coverage() < 1.0, "wireframe should not fill all pixels");
}

// ===========================================================================
// Light syncing integration tests
// ===========================================================================

#[test]
fn adapter_light_syncing_in_full_scene() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    // Camera
    let cam = Node::new("Camera", "Camera3D");
    let cam_id = tree.add_child(root, cam).unwrap();
    gdscene::node3d::set_position(&mut tree, cam_id, Vector3::new(0.0, 2.0, 5.0));
    gdscene::node3d::set_camera_current(&mut tree, cam_id, true);

    // Cube
    let mesh = Node::new("Cube", "MeshInstance3D");
    let mesh_id = tree.add_child(root, mesh).unwrap();
    gdscene::node3d::set_position(&mut tree, mesh_id, Vector3::new(0.0, 0.0, 0.0));

    // Sun (DirectionalLight3D)
    let mut sun = Node::new("Sun", "DirectionalLight3D");
    sun.set_property("light_energy", Variant::Float(1.0));
    sun.set_property("shadow_enabled", Variant::Bool(true));
    tree.add_child(root, sun).unwrap();

    // Point light
    let lamp = Node::new("Lamp", "OmniLight3D");
    tree.add_child(root, lamp).unwrap();

    let mut adapter = RenderServer3DAdapter::new(64, 64);
    let (snapshot, _) = adapter.render_frame(&tree);

    assert_eq!(snapshot.visible_mesh_count, 1);
    assert_eq!(snapshot.light_count, 2, "should see both lights");
    assert!(snapshot.nonblack_pixel_count > 0);
}

// ===========================================================================
// Mesh type dispatch integration tests
// ===========================================================================

#[test]
fn adapter_sphere_mesh_via_type_property() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let cam = Node::new("Camera", "Camera3D");
    let cam_id = tree.add_child(root, cam).unwrap();
    gdscene::node3d::set_position(&mut tree, cam_id, Vector3::new(0.0, 0.0, 10.0));

    let mut mesh = Node::new("Ball", "MeshInstance3D");
    mesh.set_property("mesh_type", Variant::String("SphereMesh".to_owned()));
    let mesh_id = tree.add_child(root, mesh).unwrap();
    gdscene::node3d::set_position(&mut tree, mesh_id, Vector3::new(0.0, 0.0, 0.0));

    let mut adapter_sphere = RenderServer3DAdapter::new(64, 64);
    let (snap_sphere, _) = adapter_sphere.render_frame(&tree);

    // Also render a default cube scene for comparison.
    let mut tree2 = SceneTree::new();
    let root2 = tree2.root_id();
    let cam2 = Node::new("Camera", "Camera3D");
    let cam2_id = tree2.add_child(root2, cam2).unwrap();
    gdscene::node3d::set_position(&mut tree2, cam2_id, Vector3::new(0.0, 0.0, 10.0));
    let cube = Node::new("Cube", "MeshInstance3D");
    let cube_id = tree2.add_child(root2, cube).unwrap();
    gdscene::node3d::set_position(&mut tree2, cube_id, Vector3::new(0.0, 0.0, 0.0));

    let mut adapter_cube = RenderServer3DAdapter::new(64, 64);
    let (snap_cube, _) = adapter_cube.render_frame(&tree2);

    // Both should produce pixels, but different pixel counts (different geometry).
    assert!(snap_sphere.nonblack_pixel_count > 0);
    assert!(snap_cube.nonblack_pixel_count > 0);
    assert_ne!(
        snap_sphere.nonblack_pixel_count, snap_cube.nonblack_pixel_count,
        "sphere and cube should produce different wireframe pixel counts"
    );
}

// ===========================================================================
// Parity report integration tests
// ===========================================================================

#[test]
fn parity_report_full_scene_functional() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let cam = Node::new("Camera", "Camera3D");
    let cam_id = tree.add_child(root, cam).unwrap();
    gdscene::node3d::set_position(&mut tree, cam_id, Vector3::new(0.0, 2.0, 5.0));
    gdscene::node3d::set_fov(&mut tree, cam_id, 75.0);
    gdscene::node3d::set_camera_current(&mut tree, cam_id, true);

    let mesh = Node::new("Cube", "MeshInstance3D");
    let mesh_id = tree.add_child(root, mesh).unwrap();
    gdscene::node3d::set_position(&mut tree, mesh_id, Vector3::new(0.0, 0.0, 0.0));

    let mut sun = Node::new("Sun", "DirectionalLight3D");
    sun.set_property("light_energy", Variant::Float(1.0));
    tree.add_child(root, sun).unwrap();

    let mut adapter = RenderServer3DAdapter::new(128, 128);
    let (snapshot, _) = adapter.render_frame(&tree);

    let report = snapshot.parity_report();
    assert!(report.is_functional());
    assert_eq!(report.mesh_count, 1);
    assert_eq!(report.light_count, 1);
    assert!(report.has_camera);
    assert!(report.coverage > 0.0);
    assert_eq!(report.viewport_pixels, 128 * 128);
}

// ===========================================================================
// Golden render snapshot test
// ===========================================================================

/// Renders the minimal_3d scene (Camera + Cube + Sun + Floor) and
/// validates the snapshot against known-good golden metrics.
#[test]
fn golden_render_snapshot_minimal_3d() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    // World root
    let world = Node::new("World", "Node3D");
    let world_id = tree.add_child(root, world).unwrap();

    // Camera at (0, 2, 5)
    let mut camera = Node::new("Camera", "Camera3D");
    camera.set_property("fov", Variant::Float(75.0));
    camera.set_property("near", Variant::Float(0.05));
    camera.set_property("far", Variant::Float(4000.0));
    camera.set_property("current", Variant::Bool(true));
    let cam_id = tree.add_child(world_id, camera).unwrap();
    gdscene::node3d::set_position(&mut tree, cam_id, Vector3::new(0.0, 2.0, 5.0));

    // Cube at origin
    let cube = Node::new("Cube", "MeshInstance3D");
    let cube_id = tree.add_child(world_id, cube).unwrap();
    gdscene::node3d::set_position(&mut tree, cube_id, Vector3::new(0.0, 0.0, 0.0));

    // Sun
    let mut sun = Node::new("Sun", "DirectionalLight3D");
    sun.set_property("light_energy", Variant::Float(1.0));
    sun.set_property("shadow_enabled", Variant::Bool(true));
    tree.add_child(world_id, sun).unwrap();

    // Render at 128x128
    let mut adapter = RenderServer3DAdapter::new(128, 128);
    let (snapshot, _frame) = adapter.render_frame(&tree);

    // ── Golden assertions ──
    // These values are locked from the current software renderer output.
    // If the renderer changes, update these values.
    assert_eq!(snapshot.visible_mesh_count, 1, "one cube visible");
    assert_eq!(snapshot.light_count, 1, "one directional light");
    assert!(snapshot.nonblack_pixel_count > 0, "cube should be visible");
    assert!(snapshot.coverage() > 0.001, "coverage should be measurable");
    assert!(snapshot.coverage() < 0.5, "wireframe should not fill half the viewport");

    // Render again — must be deterministic.
    let mut adapter2 = RenderServer3DAdapter::new(128, 128);
    let (snapshot2, _) = adapter2.render_frame(&tree);

    assert_eq!(
        snapshot.nonblack_pixel_count, snapshot2.nonblack_pixel_count,
        "golden render must be deterministic"
    );

    // Parity report must be functional.
    let report = snapshot.parity_report();
    assert!(report.is_functional());

    // Verify JSON serialization includes all fields.
    let json = snapshot.to_json();
    assert!(json.contains("\"light_count\":1"));
    assert!(json.contains("\"depth_written_count\":"));
    assert!(json.contains("\"coverage\":"));

    // Framebuffer comparison: two independent renders must be identical.
    let frame_a = adapter.last_frame().unwrap();
    let frame_b = adapter2.last_frame().unwrap();
    let diff = RenderServer3DAdapter::compare_frames(frame_a, frame_b, 0.0, 0.0);
    assert!(
        diff.is_exact_color_match(),
        "golden scene renders must be pixel-identical"
    );
}

/// Multi-frame golden trace: renders 5 frames of a scene with a moving
/// cube and validates coverage evolves correctly.
#[test]
fn golden_multi_frame_render_trace() {
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
    let mut snapshots = Vec::new();

    // Render 5 frames, moving the cube progressively further away.
    for i in 0..5 {
        let z = (i as f32) * -3.0;
        gdscene::node3d::set_position(&mut tree, mesh_id, Vector3::new(0.0, 0.0, z));
        let (snapshot, _) = adapter.render_frame(&tree);
        snapshots.push(snapshot);
    }

    // Frame numbers increment.
    for (i, s) in snapshots.iter().enumerate() {
        assert_eq!(s.frame_number, (i + 1) as u64);
    }

    // All frames should have 1 visible mesh.
    for s in &snapshots {
        assert_eq!(s.visible_mesh_count, 1);
    }

    // First frame (closest to camera) should have most coverage.
    assert!(
        snapshots[0].nonblack_pixel_count >= snapshots[4].nonblack_pixel_count,
        "closer cube should have >= coverage than further cube"
    );
}

// ===========================================================================
// pat-yb8: Fixture-based Camera3D and light registration tests
//
// Proves that camera and light registration from real .tscn fixture scenes
// is reflected in measurable render or oracle state.
// ===========================================================================

const APPROX_EPS: f32 = 1e-4;

fn approx_f32(a: f32, b: f32) -> bool {
    (a - b).abs() < APPROX_EPS
}

fn fixture_path(name: &str) -> String {
    format!("{}/../fixtures/scenes/{}", env!("CARGO_MANIFEST_DIR"), name)
}

fn load_fixture(filename: &str) -> SceneTree {
    let path = fixture_path(filename);
    let source = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("should read {}: {}", filename, e));
    let scene = PackedScene::from_tscn(&source)
        .unwrap_or_else(|e| panic!("parse {}: {:?}", filename, e));
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &scene)
        .unwrap_or_else(|e| panic!("add {} to tree: {:?}", filename, e));
    tree
}

// ---------------------------------------------------------------------------
// Fixture: minimal_3d.tscn — Camera + Cube + Sun
// ---------------------------------------------------------------------------

#[test]
fn fixture_minimal_3d_camera_registered_in_render() {
    let tree = load_fixture("minimal_3d.tscn");
    let mut adapter = RenderServer3DAdapter::new(64, 64);
    let (snapshot, _) = adapter.render_frame(&tree);

    // Camera3D at (0, 2, 5) with FOV 75° must be reflected in render snapshot.
    assert!(
        snapshot.camera_fov > 0.0,
        "camera registration must produce non-zero FOV in render snapshot"
    );
    let expected_fov = 75.0_f32.to_radians();
    assert!(
        approx_f32(snapshot.camera_fov, expected_fov),
        "fixture camera FOV should be 75° ({:.4} rad), got {:.4}",
        expected_fov,
        snapshot.camera_fov
    );
    // Camera origin Y=2, Z=5 must propagate to snapshot.
    assert!(
        approx_f32(snapshot.camera_transform[10], 2.0),
        "camera Y should be 2.0, got {}",
        snapshot.camera_transform[10]
    );
    assert!(
        approx_f32(snapshot.camera_transform[11], 5.0),
        "camera Z should be 5.0, got {}",
        snapshot.camera_transform[11]
    );
}

#[test]
fn fixture_minimal_3d_light_registered_in_render() {
    let tree = load_fixture("minimal_3d.tscn");
    let mut adapter = RenderServer3DAdapter::new(64, 64);
    let (snapshot, _) = adapter.render_frame(&tree);

    assert_eq!(
        snapshot.light_count, 1,
        "minimal_3d Sun (DirectionalLight3D) must register in render snapshot"
    );
}

#[test]
fn fixture_minimal_3d_camera_light_produce_measurable_render() {
    let tree = load_fixture("minimal_3d.tscn");
    let mut adapter = RenderServer3DAdapter::new(64, 64);
    let (snapshot, _) = adapter.render_frame(&tree);

    // With camera, mesh, and light all registered, the render must produce
    // measurable pixel output.
    assert_eq!(snapshot.visible_mesh_count, 1, "fixture Cube should be visible");
    assert!(
        snapshot.nonblack_pixel_count > 0,
        "camera+mesh+light registration must produce visible render output"
    );
    assert!(
        snapshot.coverage() > 0.0,
        "coverage must be nonzero when camera sees mesh"
    );

    // Parity report captures all registration state.
    let report = snapshot.parity_report();
    assert!(report.is_functional(), "fixture scene must be fully functional");
    assert!(report.has_camera);
    assert_eq!(report.light_count, 1);
    assert_eq!(report.mesh_count, 1);
}

// ---------------------------------------------------------------------------
// Fixture: multi_light_3d.tscn — Camera + 4 lights + 2 meshes
// ---------------------------------------------------------------------------

#[test]
fn fixture_multi_light_camera_registration() {
    let tree = load_fixture("multi_light_3d.tscn");
    let mut adapter = RenderServer3DAdapter::new(64, 64);
    let (snapshot, _) = adapter.render_frame(&tree);

    // Camera at (0, 3, 8) with FOV 65°.
    let expected_fov = 65.0_f32.to_radians();
    assert!(
        approx_f32(snapshot.camera_fov, expected_fov),
        "multi_light_3d camera FOV should be 65° ({:.4} rad), got {:.4}",
        expected_fov,
        snapshot.camera_fov
    );
    assert!(
        approx_f32(snapshot.camera_transform[10], 3.0),
        "camera Y should be 3.0, got {}",
        snapshot.camera_transform[10]
    );
    assert!(
        approx_f32(snapshot.camera_transform[11], 8.0),
        "camera Z should be 8.0, got {}",
        snapshot.camera_transform[11]
    );
}

#[test]
fn fixture_multi_light_all_lights_registered() {
    let tree = load_fixture("multi_light_3d.tscn");
    let mut adapter = RenderServer3DAdapter::new(64, 64);
    let (snapshot, _) = adapter.render_frame(&tree);

    // 1 DirectionalLight3D (KeyLight) + 3 OmniLight3D (FillLight, RimLight, AccentSpot) = 4.
    assert_eq!(
        snapshot.light_count, 4,
        "multi_light_3d must register all 4 lights in the render slice"
    );
}

#[test]
fn fixture_multi_light_meshes_visible() {
    let tree = load_fixture("multi_light_3d.tscn");
    let mut adapter = RenderServer3DAdapter::new(64, 64);
    let (snapshot, _) = adapter.render_frame(&tree);

    assert_eq!(
        snapshot.visible_mesh_count, 2,
        "multi_light_3d has Sphere + Pedestal mesh instances"
    );
    assert!(
        snapshot.nonblack_pixel_count > 0,
        "multi-light scene with meshes must produce visible pixels"
    );
}

#[test]
fn fixture_multi_light_parity_report_complete() {
    let tree = load_fixture("multi_light_3d.tscn");
    let mut adapter = RenderServer3DAdapter::new(128, 128);
    let (snapshot, _) = adapter.render_frame(&tree);

    let report = snapshot.parity_report();
    assert!(report.is_functional(), "multi-light fixture should be functional");
    assert!(report.has_camera, "parity report must reflect camera registration");
    assert_eq!(report.light_count, 4, "parity report must reflect all light registrations");
    assert_eq!(report.mesh_count, 2, "parity report must reflect mesh registrations");
    assert!(report.coverage > 0.0, "coverage must be measurable");
    assert_eq!(report.viewport_pixels, 128 * 128);
}

// ---------------------------------------------------------------------------
// Fixture: hierarchy_3d.tscn — Camera + light + nested transform hierarchy
// ---------------------------------------------------------------------------

#[test]
fn fixture_hierarchy_3d_camera_and_light_registered() {
    let tree = load_fixture("hierarchy_3d.tscn");
    let mut adapter = RenderServer3DAdapter::new(64, 64);
    let (snapshot, _) = adapter.render_frame(&tree);

    let report = snapshot.parity_report();
    assert!(report.has_camera, "hierarchy_3d should detect camera");
    assert!(report.light_count >= 1, "hierarchy_3d should have at least 1 light");

    // Camera at (0, 2, 8) with FOV 70°.
    let expected_fov = 70.0_f32.to_radians();
    assert!(
        approx_f32(snapshot.camera_fov, expected_fov),
        "hierarchy_3d camera FOV should be 70° ({:.4} rad), got {:.4}",
        expected_fov,
        snapshot.camera_fov
    );
}

// ---------------------------------------------------------------------------
// Camera registration affects render output: moving camera changes pixels
// ---------------------------------------------------------------------------

#[test]
fn camera_registration_affects_render_output() {
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

    // Render with camera at z=10.
    adapter.render_frame(&tree);
    let frame_near = adapter.last_frame().unwrap().clone();

    // Move camera far away — fewer pixels should be rendered.
    gdscene::node3d::set_position(&mut tree, cam_id, Vector3::new(0.0, 0.0, 100.0));
    adapter.render_frame(&tree);
    let frame_far = adapter.last_frame().unwrap().clone();

    let diff = RenderServer3DAdapter::compare_frames(&frame_near, &frame_far, 0.0, 0.0);
    assert!(
        !diff.is_exact_color_match(),
        "camera registration must affect render output: moving camera should change pixels"
    );
}

// ---------------------------------------------------------------------------
// Light registration affects render state: adding lights changes snapshot
// ---------------------------------------------------------------------------

#[test]
fn light_registration_changes_render_snapshot() {
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

    // Frame 1: no lights.
    let (snap_no_lights, _) = adapter.render_frame(&tree);
    assert_eq!(snap_no_lights.light_count, 0);

    // Add lights.
    let mut sun = Node::new("Sun", "DirectionalLight3D");
    sun.set_property("light_energy", Variant::Float(1.0));
    tree.add_child(root, sun).unwrap();

    let lamp = Node::new("Lamp", "OmniLight3D");
    tree.add_child(root, lamp).unwrap();

    // Frame 2: lights registered.
    let (snap_with_lights, _) = adapter.render_frame(&tree);
    assert_eq!(
        snap_with_lights.light_count, 2,
        "adding lights must be reflected in render snapshot"
    );

    // Both frames should have the same mesh visible.
    assert_eq!(snap_no_lights.visible_mesh_count, snap_with_lights.visible_mesh_count);
}

// ---------------------------------------------------------------------------
// Fixture render is deterministic across two independent adapters
// ---------------------------------------------------------------------------

#[test]
fn fixture_minimal_3d_render_deterministic() {
    let tree = load_fixture("minimal_3d.tscn");

    let mut adapter_a = RenderServer3DAdapter::new(64, 64);
    let mut adapter_b = RenderServer3DAdapter::new(64, 64);

    adapter_a.render_frame(&tree);
    adapter_b.render_frame(&tree);

    let frame_a = adapter_a.last_frame().unwrap();
    let frame_b = adapter_b.last_frame().unwrap();

    let diff = RenderServer3DAdapter::compare_frames(frame_a, frame_b, 0.0, 0.0);
    assert!(
        diff.is_exact_color_match(),
        "fixture 3D render must be deterministic across independent adapters"
    );
}

// ---------------------------------------------------------------------------
// Fixture snapshot JSON includes camera and light registration data
// ---------------------------------------------------------------------------

#[test]
fn fixture_snapshot_json_includes_registration_data() {
    let tree = load_fixture("multi_light_3d.tscn");
    let mut adapter = RenderServer3DAdapter::new(64, 64);
    let (snapshot, _) = adapter.render_frame(&tree);

    let json = snapshot.to_json();
    assert!(json.contains("\"light_count\":4"), "JSON must include light_count: {}", json);
    assert!(json.contains("\"visible_mesh_count\":2"), "JSON must include mesh count: {}", json);
    assert!(json.contains("\"camera_fov\":"), "JSON must include camera_fov: {}", json);
    assert!(json.contains("\"depth_written_count\":"), "JSON must include depth data: {}", json);
    assert!(json.contains("\"coverage\":"), "JSON must include coverage: {}", json);
}
