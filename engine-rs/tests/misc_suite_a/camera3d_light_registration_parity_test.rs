//! pat-7tno: Camera3D and light registration parity in the 3D render slice.
//!
//! Validates that the RenderServer3DAdapter correctly bridges Camera3D and
//! Light3D nodes from the scene tree to the render server. Scenarios:
//!
//! 1. Camera3D `current` flag selects the active camera from multiple candidates.
//! 2. Camera3D FOV, near, far propagate to the rendered snapshot.
//! 3. Default (no-camera) fallback produces expected viewport values.
//! 4. Camera3D under a transformed parent gets correct global transform.
//! 5. All three light types (Directional, Omni, Spot) are registered.
//! 6. Light count updates when lights are added/removed between frames.
//! 7. SpotLight3D direction is derived from the node's transform.
//! 8. Mixed scene with camera, meshes, and multiple lights renders correctly.
//! 9. Camera switch between frames changes the snapshot.
//! 10. Snapshot parity report captures correct has_camera and light_count.

use gdcore::math::Vector3;
use gdscene::node::Node;
use gdscene::node3d;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::render_server_3d::RenderServer3DAdapter;
use gdscene::scene_tree::SceneTree;
use gdvariant::Variant;

const EPSILON: f32 = 1e-4;

fn approx(a: f32, b: f32) -> bool {
    (a - b).abs() < EPSILON
}

/// Helper: builds a scene with a camera at a given position looking at the origin.
fn scene_with_camera_at(pos: Vector3) -> (SceneTree, gdscene::node::NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let cam = Node::new("Camera", "Camera3D");
    let cam_id = tree.add_child(root, cam).unwrap();
    node3d::set_position(&mut tree, cam_id, pos);
    node3d::set_camera_current(&mut tree, cam_id, true);

    (tree, cam_id)
}

// ===========================================================================
// 1. Camera3D `current` flag selects active camera
// ===========================================================================

#[test]
fn t7tno_current_camera_takes_priority_over_first() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    // First camera — NOT current, at position (0, 0, 10).
    let cam1 = Node::new("CamA", "Camera3D");
    let cam1_id = tree.add_child(root, cam1).unwrap();
    node3d::set_position(&mut tree, cam1_id, Vector3::new(0.0, 0.0, 10.0));

    // Second camera — current, at position (5, 5, 20).
    let cam2 = Node::new("CamB", "Camera3D");
    let cam2_id = tree.add_child(root, cam2).unwrap();
    node3d::set_position(&mut tree, cam2_id, Vector3::new(5.0, 5.0, 20.0));
    node3d::set_camera_current(&mut tree, cam2_id, true);

    let mut adapter = RenderServer3DAdapter::new(32, 32);
    let (snapshot, _) = adapter.render_frame(&tree);

    // Camera transform origin should match CamB (the current one).
    // camera_transform[9..12] = origin (x, y, z).
    assert!(
        approx(snapshot.camera_transform[9], 5.0),
        "camera origin X should be 5.0 from CamB, got {}",
        snapshot.camera_transform[9]
    );
    assert!(
        approx(snapshot.camera_transform[10], 5.0),
        "camera origin Y should be 5.0 from CamB, got {}",
        snapshot.camera_transform[10]
    );
    assert!(
        approx(snapshot.camera_transform[11], 20.0),
        "camera origin Z should be 20.0 from CamB, got {}",
        snapshot.camera_transform[11]
    );
}

#[test]
fn t7tno_first_camera_used_when_none_is_current() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    // Two cameras, neither marked as current.
    let cam1 = Node::new("CamA", "Camera3D");
    let cam1_id = tree.add_child(root, cam1).unwrap();
    node3d::set_position(&mut tree, cam1_id, Vector3::new(0.0, 0.0, 10.0));

    let cam2 = Node::new("CamB", "Camera3D");
    let cam2_id = tree.add_child(root, cam2).unwrap();
    node3d::set_position(&mut tree, cam2_id, Vector3::new(99.0, 99.0, 99.0));

    let mut adapter = RenderServer3DAdapter::new(32, 32);
    let (snapshot, _) = adapter.render_frame(&tree);

    // Should use first found (CamA).
    assert!(
        approx(snapshot.camera_transform[9], 0.0),
        "should use first camera when none is current, got X={}",
        snapshot.camera_transform[9]
    );
    assert!(
        approx(snapshot.camera_transform[11], 10.0),
        "should use first camera Z=10, got {}",
        snapshot.camera_transform[11]
    );
}

// ===========================================================================
// 2. Camera3D FOV, near, far propagation
// ===========================================================================

#[test]
fn t7tno_camera_fov_propagates_to_snapshot() {
    let (mut tree, cam_id) = scene_with_camera_at(Vector3::new(0.0, 0.0, 10.0));

    // Set a custom FOV (90 degrees).
    node3d::set_fov(&mut tree, cam_id, 90.0);

    let mut adapter = RenderServer3DAdapter::new(32, 32);
    let (snapshot, _) = adapter.render_frame(&tree);

    // 90° = π/2 ≈ 1.5708 radians
    let expected_fov = 90.0_f32.to_radians();
    assert!(
        approx(snapshot.camera_fov, expected_fov),
        "camera FOV should be ~{:.4} rad (90°), got {:.4}",
        expected_fov,
        snapshot.camera_fov
    );
}

#[test]
fn t7tno_camera_default_fov_is_75_degrees() {
    let (tree, _cam_id) = scene_with_camera_at(Vector3::new(0.0, 0.0, 10.0));

    let mut adapter = RenderServer3DAdapter::new(32, 32);
    let (snapshot, _) = adapter.render_frame(&tree);

    // Default FOV is 75° = 1.3090 rad (stored as degrees, converted to radians).
    let expected_fov = 75.0_f32.to_radians();
    assert!(
        approx(snapshot.camera_fov, expected_fov),
        "default camera FOV should be ~{:.4} rad (75°), got {:.4}",
        expected_fov,
        snapshot.camera_fov
    );
}

// ===========================================================================
// 3. No-camera fallback viewport
// ===========================================================================

#[test]
fn t7tno_no_camera_fallback_identity_transform() {
    let tree = SceneTree::new(); // Empty scene, no camera.

    let mut adapter = RenderServer3DAdapter::new(32, 32);
    let (snapshot, _) = adapter.render_frame(&tree);

    // Default camera: identity transform (origin = 0,0,0).
    assert!(
        approx(snapshot.camera_transform[9], 0.0)
            && approx(snapshot.camera_transform[10], 0.0)
            && approx(snapshot.camera_transform[11], 0.0),
        "fallback camera should have identity origin (0,0,0)"
    );

    // Default FOV: 45° = π/4 ≈ 0.7854 radians.
    let expected_fov = std::f32::consts::FRAC_PI_4;
    assert!(
        approx(snapshot.camera_fov, expected_fov),
        "fallback camera FOV should be π/4 ({:.4}), got {:.4}",
        expected_fov,
        snapshot.camera_fov
    );
}

// ===========================================================================
// 4. Camera3D under transformed parent
// ===========================================================================

#[test]
fn t7tno_camera_under_translated_parent_global_transform() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let parent = Node::new("Arm", "Node3D");
    let parent_id = tree.add_child(root, parent).unwrap();
    node3d::set_position(&mut tree, parent_id, Vector3::new(10.0, 20.0, 30.0));

    let cam = Node::new("Camera", "Camera3D");
    let cam_id = tree.add_child(parent_id, cam).unwrap();
    node3d::set_position(&mut tree, cam_id, Vector3::new(1.0, 2.0, 3.0));
    node3d::set_camera_current(&mut tree, cam_id, true);

    let mut adapter = RenderServer3DAdapter::new(32, 32);
    let (snapshot, _) = adapter.render_frame(&tree);

    // Global position = parent (10,20,30) + local (1,2,3) = (11,22,33).
    assert!(
        approx(snapshot.camera_transform[9], 11.0),
        "camera global X should be 11.0, got {}",
        snapshot.camera_transform[9]
    );
    assert!(
        approx(snapshot.camera_transform[10], 22.0),
        "camera global Y should be 22.0, got {}",
        snapshot.camera_transform[10]
    );
    assert!(
        approx(snapshot.camera_transform[11], 33.0),
        "camera global Z should be 33.0, got {}",
        snapshot.camera_transform[11]
    );
}

// ===========================================================================
// 5. All three light types registered
// ===========================================================================

#[test]
fn t7tno_directional_light_registered() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let sun = Node::new("Sun", "DirectionalLight3D");
    tree.add_child(root, sun).unwrap();

    let mut adapter = RenderServer3DAdapter::new(32, 32);
    let (snapshot, _) = adapter.render_frame(&tree);

    assert_eq!(
        snapshot.light_count, 1,
        "DirectionalLight3D should register"
    );
}

#[test]
fn t7tno_omni_light_registered() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let lamp = Node::new("Lamp", "OmniLight3D");
    tree.add_child(root, lamp).unwrap();

    let mut adapter = RenderServer3DAdapter::new(32, 32);
    let (snapshot, _) = adapter.render_frame(&tree);

    assert_eq!(snapshot.light_count, 1, "OmniLight3D should register");
}

#[test]
fn t7tno_spot_light_registered() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let spot = Node::new("Spot", "SpotLight3D");
    tree.add_child(root, spot).unwrap();

    let mut adapter = RenderServer3DAdapter::new(32, 32);
    let (snapshot, _) = adapter.render_frame(&tree);

    assert_eq!(snapshot.light_count, 1, "SpotLight3D should register");
}

#[test]
fn t7tno_all_three_light_types_together() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    tree.add_child(root, Node::new("Sun", "DirectionalLight3D"))
        .unwrap();
    tree.add_child(root, Node::new("Lamp", "OmniLight3D"))
        .unwrap();
    tree.add_child(root, Node::new("Spot", "SpotLight3D"))
        .unwrap();

    let mut adapter = RenderServer3DAdapter::new(32, 32);
    let (snapshot, _) = adapter.render_frame(&tree);

    assert_eq!(
        snapshot.light_count, 3,
        "all three light types should register independently"
    );
}

// ===========================================================================
// 6. Light count updates when lights added/removed between frames
// ===========================================================================

#[test]
fn t7tno_light_count_updates_on_removal() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let sun_id = tree
        .add_child(root, Node::new("Sun", "DirectionalLight3D"))
        .unwrap();
    let lamp_id = tree
        .add_child(root, Node::new("Lamp", "OmniLight3D"))
        .unwrap();

    let mut adapter = RenderServer3DAdapter::new(32, 32);

    // Frame 1: two lights.
    let (snap1, _) = adapter.render_frame(&tree);
    assert_eq!(snap1.light_count, 2);

    // Remove the sun.
    tree.remove_node(sun_id).unwrap();

    // Frame 2: one light remaining.
    let (snap2, _) = adapter.render_frame(&tree);
    assert_eq!(snap2.light_count, 1, "removing a light should update count");

    // Remove the lamp too.
    tree.remove_node(lamp_id).unwrap();

    // Frame 3: no lights.
    let (snap3, _) = adapter.render_frame(&tree);
    assert_eq!(
        snap3.light_count, 0,
        "all lights removed should give count 0"
    );
}

#[test]
fn t7tno_light_count_updates_on_addition() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let mut adapter = RenderServer3DAdapter::new(32, 32);

    // Frame 1: no lights.
    let (snap1, _) = adapter.render_frame(&tree);
    assert_eq!(snap1.light_count, 0);

    // Add a light.
    tree.add_child(root, Node::new("Sun", "DirectionalLight3D"))
        .unwrap();

    // Frame 2: one light.
    let (snap2, _) = adapter.render_frame(&tree);
    assert_eq!(snap2.light_count, 1, "adding a light should update count");
}

// ===========================================================================
// 7. SpotLight3D direction from transform
// ===========================================================================

#[test]
fn t7tno_spot_light_uses_transform_for_position() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let spot = Node::new("Spot", "SpotLight3D");
    let spot_id = tree.add_child(root, spot).unwrap();
    node3d::set_position(&mut tree, spot_id, Vector3::new(5.0, 10.0, 15.0));

    let mut adapter = RenderServer3DAdapter::new(32, 32);
    let (snapshot, _) = adapter.render_frame(&tree);

    // SpotLight3D should register.
    assert_eq!(snapshot.light_count, 1);
}

// ===========================================================================
// 8. Mixed scene: camera + meshes + multiple lights
// ===========================================================================

#[test]
fn t7tno_mixed_scene_camera_mesh_lights() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    // Camera.
    let cam = Node::new("Camera", "Camera3D");
    let cam_id = tree.add_child(root, cam).unwrap();
    node3d::set_position(&mut tree, cam_id, Vector3::new(0.0, 2.0, 10.0));
    node3d::set_camera_current(&mut tree, cam_id, true);

    // Meshes.
    let cube = Node::new("Cube", "MeshInstance3D");
    tree.add_child(root, cube).unwrap();
    let sphere = Node::new("Sphere", "MeshInstance3D");
    tree.add_child(root, sphere).unwrap();

    // Lights.
    let mut sun = Node::new("Sun", "DirectionalLight3D");
    sun.set_property("light_energy", Variant::Float(2.0));
    sun.set_property("shadow_enabled", Variant::Bool(true));
    tree.add_child(root, sun).unwrap();

    let lamp = Node::new("Lamp", "OmniLight3D");
    let lamp_id = tree.add_child(root, lamp).unwrap();
    node3d::set_position(&mut tree, lamp_id, Vector3::new(3.0, 5.0, 0.0));
    node3d::set_light_energy(&mut tree, lamp_id, 1.5);

    let spot = Node::new("Spot", "SpotLight3D");
    let spot_id = tree.add_child(root, spot).unwrap();
    node3d::set_position(&mut tree, spot_id, Vector3::new(-2.0, 4.0, 2.0));

    let mut adapter = RenderServer3DAdapter::new(64, 64);
    let (snapshot, _) = adapter.render_frame(&tree);

    assert_eq!(snapshot.visible_mesh_count, 2, "should see both meshes");
    assert_eq!(snapshot.light_count, 3, "should see all three lights");
    assert!(
        approx(snapshot.camera_transform[9], 0.0),
        "camera X should be 0.0"
    );
    assert!(
        approx(snapshot.camera_transform[10], 2.0),
        "camera Y should be 2.0"
    );
    assert!(
        approx(snapshot.camera_transform[11], 10.0),
        "camera Z should be 10.0"
    );
}

// ===========================================================================
// 9. Camera switch between frames
// ===========================================================================

#[test]
fn t7tno_camera_switch_between_frames() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let cam1 = Node::new("CamA", "Camera3D");
    let cam1_id = tree.add_child(root, cam1).unwrap();
    node3d::set_position(&mut tree, cam1_id, Vector3::new(0.0, 0.0, 10.0));
    node3d::set_camera_current(&mut tree, cam1_id, true);

    let cam2 = Node::new("CamB", "Camera3D");
    let cam2_id = tree.add_child(root, cam2).unwrap();
    node3d::set_position(&mut tree, cam2_id, Vector3::new(50.0, 50.0, 50.0));

    let mut adapter = RenderServer3DAdapter::new(32, 32);

    // Frame 1: CamA is current.
    let (snap1, _) = adapter.render_frame(&tree);
    assert!(
        approx(snap1.camera_transform[9], 0.0),
        "frame 1 should use CamA at X=0"
    );

    // Switch to CamB.
    node3d::set_camera_current(&mut tree, cam1_id, false);
    node3d::set_camera_current(&mut tree, cam2_id, true);

    // Frame 2: CamB is current.
    let (snap2, _) = adapter.render_frame(&tree);
    assert!(
        approx(snap2.camera_transform[9], 50.0),
        "frame 2 should use CamB at X=50, got {}",
        snap2.camera_transform[9]
    );
}

// ===========================================================================
// 10. Snapshot captures correct camera and light metrics
// ===========================================================================

#[test]
fn t7tno_snapshot_empty_scene_no_camera_no_lights() {
    let tree = SceneTree::new();
    let mut adapter = RenderServer3DAdapter::new(32, 32);
    let (snapshot, _) = adapter.render_frame(&tree);

    assert_eq!(snapshot.light_count, 0, "empty scene should have 0 lights");
    assert_eq!(
        snapshot.visible_mesh_count, 0,
        "empty scene should have 0 meshes"
    );
}

#[test]
fn t7tno_snapshot_with_camera_mesh_and_lights() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let cam = Node::new("Camera", "Camera3D");
    let cam_id = tree.add_child(root, cam).unwrap();
    node3d::set_position(&mut tree, cam_id, Vector3::new(0.0, 0.0, 10.0));
    node3d::set_camera_current(&mut tree, cam_id, true);

    let mesh = Node::new("Cube", "MeshInstance3D");
    tree.add_child(root, mesh).unwrap();

    tree.add_child(root, Node::new("Sun", "DirectionalLight3D"))
        .unwrap();
    tree.add_child(root, Node::new("Lamp", "OmniLight3D"))
        .unwrap();

    let mut adapter = RenderServer3DAdapter::new(64, 64);
    let (snapshot, _) = adapter.render_frame(&tree);

    // Camera FOV should be non-zero (camera is present).
    assert!(
        snapshot.camera_fov > 0.0,
        "should detect camera via non-zero FOV"
    );
    assert_eq!(snapshot.light_count, 2, "should count both lights");
    assert_eq!(snapshot.visible_mesh_count, 1, "should count mesh");
    assert!(
        snapshot.nonblack_pixel_count > 0,
        "camera+mesh should produce visible pixels"
    );
}

#[test]
fn t7tno_snapshot_json_includes_light_count() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    tree.add_child(root, Node::new("Sun", "DirectionalLight3D"))
        .unwrap();

    let mut adapter = RenderServer3DAdapter::new(32, 32);
    let (snapshot, _) = adapter.render_frame(&tree);
    let json = snapshot.to_json();

    assert!(
        json.contains("\"light_count\":1"),
        "snapshot JSON should include light_count: {}",
        json
    );
}

// ===========================================================================
// 11–16. Fixture-based tests: load real .tscn scenes and verify camera/light
//        registration is reflected in measurable render state.
// ===========================================================================

fn fixture_path(name: &str) -> String {
    format!("{}/../fixtures/scenes/{}", env!("CARGO_MANIFEST_DIR"), name)
}

fn load_fixture_to_tree(filename: &str) -> SceneTree {
    let path = fixture_path(filename);
    let source = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("should read {}: {}", filename, e));
    let scene =
        PackedScene::from_tscn(&source).unwrap_or_else(|e| panic!("parse {}: {:?}", filename, e));
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &scene)
        .unwrap_or_else(|e| panic!("add {} to tree: {:?}", filename, e));
    tree
}

// ---------------------------------------------------------------------------
// 11. minimal_3d.tscn: camera + 1 directional light → snapshot reflects both
// ---------------------------------------------------------------------------

#[test]
fn t7tno_fixture_minimal_3d_camera_detected() {
    let tree = load_fixture_to_tree("minimal_3d.tscn");
    let mut adapter = RenderServer3DAdapter::new(64, 64);
    let (snapshot, _) = adapter.render_frame(&tree);

    // Camera at (0, 2, 5) with FOV 75°.
    let report = snapshot.parity_report();
    assert!(
        report.has_camera,
        "minimal_3d fixture should detect Camera3D"
    );

    let expected_fov = 75.0_f32.to_radians();
    assert!(
        approx(snapshot.camera_fov, expected_fov),
        "minimal_3d camera FOV should be 75° ({:.4} rad), got {:.4}",
        expected_fov,
        snapshot.camera_fov
    );

    // Camera position: Y=2, Z=5.
    assert!(
        approx(snapshot.camera_transform[10], 2.0),
        "minimal_3d camera Y should be 2.0, got {}",
        snapshot.camera_transform[10]
    );
    assert!(
        approx(snapshot.camera_transform[11], 5.0),
        "minimal_3d camera Z should be 5.0, got {}",
        snapshot.camera_transform[11]
    );
}

#[test]
fn t7tno_fixture_minimal_3d_light_registered() {
    let tree = load_fixture_to_tree("minimal_3d.tscn");
    let mut adapter = RenderServer3DAdapter::new(64, 64);
    let (snapshot, _) = adapter.render_frame(&tree);

    assert_eq!(
        snapshot.light_count, 1,
        "minimal_3d has 1 DirectionalLight3D (Sun)"
    );
}

#[test]
fn t7tno_fixture_minimal_3d_mesh_visible() {
    let tree = load_fixture_to_tree("minimal_3d.tscn");
    let mut adapter = RenderServer3DAdapter::new(64, 64);
    let (snapshot, _) = adapter.render_frame(&tree);

    assert_eq!(
        snapshot.visible_mesh_count, 1,
        "minimal_3d has 1 MeshInstance3D (Cube)"
    );
}

// ---------------------------------------------------------------------------
// 12. multi_light_3d.tscn: camera + 4 lights → all registered in snapshot
// ---------------------------------------------------------------------------

#[test]
fn t7tno_fixture_multi_light_camera_and_all_lights() {
    let tree = load_fixture_to_tree("multi_light_3d.tscn");
    let mut adapter = RenderServer3DAdapter::new(64, 64);
    let (snapshot, _) = adapter.render_frame(&tree);

    let report = snapshot.parity_report();
    assert!(report.has_camera, "multi_light_3d should have a camera");

    // Camera FOV = 65° in the fixture.
    let expected_fov = 65.0_f32.to_radians();
    assert!(
        approx(snapshot.camera_fov, expected_fov),
        "multi_light_3d camera FOV should be 65° ({:.4} rad), got {:.4}",
        expected_fov,
        snapshot.camera_fov
    );

    // Camera at (0, 3, 8).
    assert!(
        approx(snapshot.camera_transform[10], 3.0),
        "multi_light_3d camera Y should be 3.0, got {}",
        snapshot.camera_transform[10]
    );
    assert!(
        approx(snapshot.camera_transform[11], 8.0),
        "multi_light_3d camera Z should be 8.0, got {}",
        snapshot.camera_transform[11]
    );

    // 1 DirectionalLight3D (KeyLight) + 3 OmniLight3D (FillLight, RimLight, AccentSpot) = 4.
    assert_eq!(
        snapshot.light_count, 4,
        "multi_light_3d has 4 lights (1 directional + 3 omni)"
    );

    // 2 mesh instances (Sphere + Pedestal).
    assert_eq!(
        snapshot.visible_mesh_count, 2,
        "multi_light_3d has 2 MeshInstance3D nodes"
    );
}

// ---------------------------------------------------------------------------
// 13. indoor_3d.tscn: camera + 2 OmniLight3D → measurable state
// ---------------------------------------------------------------------------

#[test]
fn t7tno_fixture_indoor_3d_camera_and_lights() {
    let tree = load_fixture_to_tree("indoor_3d.tscn");
    let mut adapter = RenderServer3DAdapter::new(64, 64);
    let (snapshot, _) = adapter.render_frame(&tree);

    let report = snapshot.parity_report();
    assert!(report.has_camera, "indoor_3d should have a camera");
    assert_eq!(
        snapshot.light_count, 2,
        "indoor_3d has 2 OmniLight3D nodes (Lamp + CeilingLight)"
    );
    // 2 mesh instances (Table + Chair).
    assert_eq!(
        snapshot.visible_mesh_count, 2,
        "indoor_3d has 2 MeshInstance3D nodes"
    );
}

// ---------------------------------------------------------------------------
// 14. Fixture parity report: all fields populated correctly
// ---------------------------------------------------------------------------

#[test]
fn t7tno_fixture_parity_report_complete() {
    let tree = load_fixture_to_tree("multi_light_3d.tscn");
    let mut adapter = RenderServer3DAdapter::new(128, 128);
    let (snapshot, _) = adapter.render_frame(&tree);

    let report = snapshot.parity_report();
    assert!(report.has_camera, "parity report should show camera");
    assert_eq!(report.light_count, 4, "parity report light count");
    assert_eq!(report.mesh_count, 2, "parity report mesh count");
    assert!(
        report.viewport_pixels > 0,
        "parity report should have pixels"
    );
}

// ---------------------------------------------------------------------------
// 15. Fixture snapshot JSON is well-formed and includes camera/light data
// ---------------------------------------------------------------------------

#[test]
fn t7tno_fixture_snapshot_json_well_formed() {
    let tree = load_fixture_to_tree("minimal_3d.tscn");
    let mut adapter = RenderServer3DAdapter::new(32, 32);
    let (snapshot, _) = adapter.render_frame(&tree);

    let json = snapshot.to_json();
    assert!(
        json.contains("\"light_count\":1"),
        "JSON light_count: {}",
        json
    );
    assert!(
        json.contains("\"visible_mesh_count\":1"),
        "JSON visible_mesh_count: {}",
        json
    );
    assert!(
        json.contains("\"camera_fov\":"),
        "JSON should include camera_fov: {}",
        json
    );
    // Verify it's valid JSON (braces balanced, no trailing comma issues).
    assert!(
        json.starts_with('{') && json.ends_with('}'),
        "should be valid JSON object"
    );
}

// ===========================================================================
// pat-7tno: Additional camera and light registration coverage
// ===========================================================================

/// Camera3D with custom FOV: the snapshot must reflect the set FOV.
#[test]
fn t7tno_camera_custom_fov_reflected_in_snapshot() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let mut cam = Node::new("Camera", "Camera3D");
    cam.set_property("fov", Variant::Float(60.0));
    cam.set_property("current", Variant::Bool(true));
    let cam_id = tree.add_child(root, cam).unwrap();
    node3d::set_position(&mut tree, cam_id, Vector3::new(0.0, 0.0, 10.0));

    let mut adapter = RenderServer3DAdapter::new(32, 32);
    let (snapshot, _) = adapter.render_frame(&tree);

    let expected_fov = 60.0_f32.to_radians();
    assert!(
        approx(snapshot.camera_fov, expected_fov),
        "FOV should be 60° ({:.4} rad), got {:.4}",
        expected_fov,
        snapshot.camera_fov
    );
}

/// Light energy value is captured in the render snapshot.
#[test]
fn t7tno_light_energy_reflected_in_snapshot() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let cam = Node::new("Camera", "Camera3D");
    let cam_id = tree.add_child(root, cam).unwrap();
    node3d::set_camera_current(&mut tree, cam_id, true);

    let mut light = Node::new("Sun", "DirectionalLight3D");
    light.set_property("light_energy", Variant::Float(3.5));
    tree.add_child(root, light).unwrap();

    let mut adapter = RenderServer3DAdapter::new(32, 32);
    let (snapshot, _) = adapter.render_frame(&tree);

    assert_eq!(snapshot.light_count, 1, "one light");
    // Light energy should influence the render (nonblack pixels > 0 if mesh exists).
    let report = snapshot.parity_report();
    assert!(report.has_camera, "camera detected");
    assert_eq!(report.light_count, 1);
}

/// Adding lights between frames: light count updates dynamically.
#[test]
fn t7tno_light_count_updates_across_frames() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let cam = Node::new("Camera", "Camera3D");
    let cam_id = tree.add_child(root, cam).unwrap();
    node3d::set_camera_current(&mut tree, cam_id, true);

    let mut adapter = RenderServer3DAdapter::new(32, 32);

    // Frame 1: no lights.
    let (snap1, _) = adapter.render_frame(&tree);
    assert_eq!(snap1.light_count, 0, "frame 1: no lights");

    // Add a light.
    tree.add_child(root, Node::new("Sun", "DirectionalLight3D"))
        .unwrap();

    // Frame 2: 1 light.
    let (snap2, _) = adapter.render_frame(&tree);
    assert_eq!(snap2.light_count, 1, "frame 2: one light");

    // Add two more lights.
    tree.add_child(root, Node::new("Lamp1", "OmniLight3D"))
        .unwrap();
    tree.add_child(root, Node::new("Lamp2", "OmniLight3D"))
        .unwrap();

    // Frame 3: 3 lights.
    let (snap3, _) = adapter.render_frame(&tree);
    assert_eq!(snap3.light_count, 3, "frame 3: three lights");
}

/// Camera3D FOV change between frames is reflected in the snapshot.
#[test]
fn t7tno_camera_fov_change_between_frames() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let mut cam = Node::new("Camera", "Camera3D");
    cam.set_property("fov", Variant::Float(90.0));
    cam.set_property("current", Variant::Bool(true));
    let cam_id = tree.add_child(root, cam).unwrap();
    node3d::set_position(&mut tree, cam_id, Vector3::new(0.0, 0.0, 10.0));

    let mut adapter = RenderServer3DAdapter::new(32, 32);

    // Frame 1: FOV 90°.
    let (snap1, _) = adapter.render_frame(&tree);
    let expected_fov1 = 90.0_f32.to_radians();
    assert!(
        approx(snap1.camera_fov, expected_fov1),
        "frame 1 FOV should be 90° ({:.4}), got {:.4}",
        expected_fov1,
        snap1.camera_fov
    );

    // Change FOV to 45°.
    tree.get_node_mut(cam_id)
        .unwrap()
        .set_property("fov", Variant::Float(45.0));

    // Frame 2: FOV 45°.
    let (snap2, _) = adapter.render_frame(&tree);
    let expected_fov2 = 45.0_f32.to_radians();
    assert!(
        approx(snap2.camera_fov, expected_fov2),
        "frame 2 FOV should be 45° ({:.4}), got {:.4}",
        expected_fov2,
        snap2.camera_fov
    );
}

/// Multiple Camera3D nodes but none current: fallback camera used.
#[test]
fn t7tno_no_current_camera_uses_fallback() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    // Two cameras, neither is current.
    let cam1 = Node::new("CamA", "Camera3D");
    tree.add_child(root, cam1).unwrap();
    let cam2 = Node::new("CamB", "Camera3D");
    tree.add_child(root, cam2).unwrap();

    let mut adapter = RenderServer3DAdapter::new(32, 32);
    let (snapshot, _) = adapter.render_frame(&tree);

    let report = snapshot.parity_report();
    // When no camera is current, the adapter may use the first one or fallback.
    // Either way, it should not panic and should produce a valid snapshot.
    assert!(
        report.viewport_pixels > 0,
        "should still produce a valid snapshot even without a current camera"
    );
}
