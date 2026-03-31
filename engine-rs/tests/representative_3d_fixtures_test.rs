//! pat-i2xe, pat-sp67: Representative 3D fixtures for the first runtime slice.
//!
//! Validates the 3D fixture corpus — scene loading, golden JSON structure,
//! physics golden traces, and scene tree correctness for each representative
//! 3D fixture:
//!   - indoor_3d.tscn   — multi-mesh, multi-light indoor environment
//!   - physics_3d_playground.tscn — rigid bodies, static bodies, ramp
//!   - multi_light_3d.tscn — key/fill/rim/accent lighting setup
//!
//! Also validates physics golden traces:
//!   - rigid_sphere_bounce_3d_20frames.json
//!   - multi_body_3d_20frames.json

use std::sync::Mutex;

use gdcore::compare3d::{compare_physics_traces, PhysicsTraceEntry3D};
use gdcore::math::Vector3;
use gdobject::class_db;
use gdscene::scene_tree::SceneTree;
use gdvariant::Variant;

static TEST_LOCK: Mutex<()> = Mutex::new(());

fn setup() -> std::sync::MutexGuard<'static, ()> {
    let guard = TEST_LOCK.lock().expect("test lock poisoned");
    class_db::clear_for_testing();
    class_db::register_class(class_db::ClassRegistration::new("Object"));
    class_db::register_class(
        class_db::ClassRegistration::new("Node")
            .parent("Object")
            .property(class_db::PropertyInfo::new(
                "name",
                Variant::String(String::new()),
            )),
    );
    class_db::register_3d_classes();
    guard
}

fn fixture_path(name: &str) -> String {
    format!("{}/../fixtures/scenes/{}", env!("CARGO_MANIFEST_DIR"), name)
}

fn golden_scene_path(name: &str) -> String {
    format!(
        "{}/../fixtures/golden/scenes/{}.json",
        env!("CARGO_MANIFEST_DIR"),
        name
    )
}

fn golden_physics_path(name: &str) -> String {
    format!(
        "{}/../fixtures/golden/physics/{}.json",
        env!("CARGO_MANIFEST_DIR"),
        name
    )
}

fn load_golden_scene(name: &str) -> serde_json::Value {
    let path = golden_scene_path(name);
    let data = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read golden scene {}: {}", path, e));
    serde_json::from_str(&data)
        .unwrap_or_else(|e| panic!("failed to parse golden JSON {}: {}", path, e))
}

fn load_golden_3d_trace(name: &str) -> Vec<PhysicsTraceEntry3D> {
    let path = golden_physics_path(name);
    let data = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read golden trace {}: {}", path, e));
    let entries: Vec<serde_json::Value> = serde_json::from_str(&data)
        .unwrap_or_else(|e| panic!("failed to parse golden JSON {}: {}", path, e));

    entries
        .iter()
        .map(|e| {
            PhysicsTraceEntry3D::new(
                e["name"].as_str().unwrap(),
                e["frame"].as_u64().unwrap(),
                Vector3::new(
                    e["px"].as_f64().unwrap() as f32,
                    e["py"].as_f64().unwrap() as f32,
                    e["pz"].as_f64().unwrap() as f32,
                ),
                Vector3::new(
                    e["vx"].as_f64().unwrap() as f32,
                    e["vy"].as_f64().unwrap() as f32,
                    e["vz"].as_f64().unwrap() as f32,
                ),
                0.0,
            )
        })
        .collect()
}

fn load_tscn_to_tree(filename: &str) -> SceneTree {
    let path = fixture_path(filename);
    let source = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("should read {}: {}", filename, e));
    let scene = gdscene::packed_scene::PackedScene::from_tscn(&source)
        .unwrap_or_else(|e| panic!("parse {}: {:?}", filename, e));
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    gdscene::packed_scene::add_packed_scene_to_tree(&mut tree, root, &scene)
        .unwrap_or_else(|e| panic!("add {} to tree: {:?}", filename, e));
    tree
}

// ===========================================================================
// 1. Fixture files exist and are parseable
// ===========================================================================

#[test]
fn indoor_3d_tscn_exists() {
    let path = fixture_path("indoor_3d.tscn");
    assert!(
        std::path::Path::new(&path).exists(),
        "indoor_3d.tscn fixture missing"
    );
}

#[test]
fn physics_3d_playground_tscn_exists() {
    let path = fixture_path("physics_3d_playground.tscn");
    assert!(
        std::path::Path::new(&path).exists(),
        "physics_3d_playground.tscn fixture missing"
    );
}

#[test]
fn multi_light_3d_tscn_exists() {
    let path = fixture_path("multi_light_3d.tscn");
    assert!(
        std::path::Path::new(&path).exists(),
        "multi_light_3d.tscn fixture missing"
    );
}

// ===========================================================================
// 2. Golden JSON files exist and are valid
// ===========================================================================

#[test]
fn golden_indoor_3d_valid_json() {
    let golden = load_golden_scene("indoor_3d");
    assert!(golden["fixture_id"].as_str().is_some());
    assert_eq!(golden["fixture_id"].as_str().unwrap(), "scene_indoor_3d");
    let nodes = golden["data"]["nodes"].as_array().unwrap();
    assert!(!nodes.is_empty());
}

#[test]
fn golden_physics_3d_playground_valid_json() {
    let golden = load_golden_scene("physics_3d_playground");
    assert_eq!(
        golden["fixture_id"].as_str().unwrap(),
        "scene_physics_3d_playground"
    );
    let nodes = golden["data"]["nodes"].as_array().unwrap();
    assert!(nodes.len() > 5);
}

#[test]
fn golden_multi_light_3d_valid_json() {
    let golden = load_golden_scene("multi_light_3d");
    assert_eq!(
        golden["fixture_id"].as_str().unwrap(),
        "scene_multi_light_3d"
    );
}

// ===========================================================================
// 3. Scene loading — indoor_3d.tscn
// ===========================================================================

#[test]
fn indoor_3d_loads_all_nodes() {
    let _g = setup();
    let tree = load_tscn_to_tree("indoor_3d.tscn");

    assert!(tree.get_node_by_path("/root/Room").is_some());
    assert!(tree.get_node_by_path("/root/Room/Camera").is_some());
    assert!(tree.get_node_by_path("/root/Room/Table").is_some());
    assert!(tree.get_node_by_path("/root/Room/Chair").is_some());
    assert!(tree.get_node_by_path("/root/Room/Lamp").is_some());
    assert!(tree.get_node_by_path("/root/Room/CeilingLight").is_some());
    assert!(tree.get_node_by_path("/root/Room/Floor").is_some());
    assert!(tree
        .get_node_by_path("/root/Room/Floor/FloorShape")
        .is_some());
    assert!(tree.get_node_by_path("/root/Room/Wall_Back").is_some());
    assert!(tree
        .get_node_by_path("/root/Room/Wall_Back/WallShape")
        .is_some());
}

#[test]
fn indoor_3d_correct_node_classes() {
    let _g = setup();
    let tree = load_tscn_to_tree("indoor_3d.tscn");

    let room_id = tree.get_node_by_path("/root/Room").unwrap();
    assert_eq!(tree.get_node(room_id).unwrap().class_name(), "Node3D");

    let cam_id = tree.get_node_by_path("/root/Room/Camera").unwrap();
    assert_eq!(tree.get_node(cam_id).unwrap().class_name(), "Camera3D");

    let table_id = tree.get_node_by_path("/root/Room/Table").unwrap();
    assert_eq!(
        tree.get_node(table_id).unwrap().class_name(),
        "MeshInstance3D"
    );

    let lamp_id = tree.get_node_by_path("/root/Room/Lamp").unwrap();
    assert_eq!(tree.get_node(lamp_id).unwrap().class_name(), "OmniLight3D");

    let floor_id = tree.get_node_by_path("/root/Room/Floor").unwrap();
    assert_eq!(
        tree.get_node(floor_id).unwrap().class_name(),
        "StaticBody3D"
    );
}

#[test]
fn indoor_3d_node_count() {
    let _g = setup();
    let tree = load_tscn_to_tree("indoor_3d.tscn");
    // root + Room + Camera + Table + Chair + Lamp + CeilingLight + Floor + FloorShape + Wall_Back + WallShape = 11
    assert_eq!(tree.node_count(), 11);
}

#[test]
fn indoor_3d_camera_properties() {
    let _g = setup();
    let tree = load_tscn_to_tree("indoor_3d.tscn");
    let cam_id = tree.get_node_by_path("/root/Room/Camera").unwrap();
    let cam = tree.get_node(cam_id).unwrap();

    assert_eq!(cam.get_property("fov"), Variant::Float(70.0));
    assert_eq!(cam.get_property("far"), Variant::Float(100.0));
}

#[test]
fn indoor_3d_two_lights() {
    let _g = setup();
    let tree = load_tscn_to_tree("indoor_3d.tscn");

    let lamp_id = tree.get_node_by_path("/root/Room/Lamp").unwrap();
    let lamp = tree.get_node(lamp_id).unwrap();
    assert_eq!(lamp.get_property("light_energy"), Variant::Float(1.5));

    let ceil_id = tree.get_node_by_path("/root/Room/CeilingLight").unwrap();
    let ceil = tree.get_node(ceil_id).unwrap();
    assert_eq!(ceil.get_property("light_energy"), Variant::Float(0.8));
}

// ===========================================================================
// 4. Scene loading — physics_3d_playground.tscn
// ===========================================================================

#[test]
fn physics_3d_playground_loads_all_nodes() {
    let _g = setup();
    let tree = load_tscn_to_tree("physics_3d_playground.tscn");

    assert!(tree.get_node_by_path("/root/World").is_some());
    assert!(tree.get_node_by_path("/root/World/Camera").is_some());
    assert!(tree.get_node_by_path("/root/World/Sun").is_some());
    assert!(tree.get_node_by_path("/root/World/Ball").is_some());
    assert!(tree.get_node_by_path("/root/World/Cube").is_some());
    assert!(tree.get_node_by_path("/root/World/HeavyBlock").is_some());
    assert!(tree.get_node_by_path("/root/World/Platform").is_some());
    assert!(tree.get_node_by_path("/root/World/Ramp").is_some());
}

#[test]
fn physics_3d_playground_rigid_body_properties() {
    let _g = setup();
    let tree = load_tscn_to_tree("physics_3d_playground.tscn");

    let ball_id = tree.get_node_by_path("/root/World/Ball").unwrap();
    let ball = tree.get_node(ball_id).unwrap();
    assert_eq!(ball.class_name(), "RigidBody3D");
    assert_eq!(ball.get_property("mass"), Variant::Float(2.0));

    let cube_id = tree.get_node_by_path("/root/World/Cube").unwrap();
    let cube = tree.get_node(cube_id).unwrap();
    assert_eq!(cube.class_name(), "RigidBody3D");
    assert_eq!(cube.get_property("mass"), Variant::Float(1.0));

    let heavy_id = tree.get_node_by_path("/root/World/HeavyBlock").unwrap();
    let heavy = tree.get_node(heavy_id).unwrap();
    assert_eq!(heavy.class_name(), "RigidBody3D");
    assert_eq!(heavy.get_property("mass"), Variant::Float(10.0));
}

#[test]
fn physics_3d_playground_static_bodies() {
    let _g = setup();
    let tree = load_tscn_to_tree("physics_3d_playground.tscn");

    let plat_id = tree.get_node_by_path("/root/World/Platform").unwrap();
    assert_eq!(tree.get_node(plat_id).unwrap().class_name(), "StaticBody3D");

    let ramp_id = tree.get_node_by_path("/root/World/Ramp").unwrap();
    assert_eq!(tree.get_node(ramp_id).unwrap().class_name(), "StaticBody3D");
}

#[test]
fn physics_3d_playground_node_count() {
    let _g = setup();
    let tree = load_tscn_to_tree("physics_3d_playground.tscn");
    // root + World + Camera + Sun + Ball + BallShape + BallMesh
    // + Cube + CubeShape + CubeMesh + HeavyBlock + HeavyShape
    // + Platform + PlatformShape + PlatformMesh + Ramp + RampShape = 17
    assert_eq!(tree.node_count(), 17);
}

#[test]
fn physics_3d_playground_camera_far_plane() {
    let _g = setup();
    let tree = load_tscn_to_tree("physics_3d_playground.tscn");
    let cam_id = tree.get_node_by_path("/root/World/Camera").unwrap();
    let cam = tree.get_node(cam_id).unwrap();
    assert_eq!(cam.get_property("fov"), Variant::Float(60.0));
    assert_eq!(cam.get_property("far"), Variant::Float(500.0));
}

#[test]
fn physics_3d_playground_child_hierarchy() {
    let _g = setup();
    let tree = load_tscn_to_tree("physics_3d_playground.tscn");

    // Ball has two children: BallShape and BallMesh
    assert!(tree
        .get_node_by_path("/root/World/Ball/BallShape")
        .is_some());
    assert!(tree.get_node_by_path("/root/World/Ball/BallMesh").is_some());

    // Platform has two children: PlatformShape and PlatformMesh
    assert!(tree
        .get_node_by_path("/root/World/Platform/PlatformShape")
        .is_some());
    assert!(tree
        .get_node_by_path("/root/World/Platform/PlatformMesh")
        .is_some());
}

// ===========================================================================
// 5. Scene loading — multi_light_3d.tscn
// ===========================================================================

#[test]
fn multi_light_3d_loads_all_nodes() {
    let _g = setup();
    let tree = load_tscn_to_tree("multi_light_3d.tscn");

    assert!(tree.get_node_by_path("/root/Stage").is_some());
    assert!(tree.get_node_by_path("/root/Stage/Camera").is_some());
    assert!(tree.get_node_by_path("/root/Stage/Sphere").is_some());
    assert!(tree.get_node_by_path("/root/Stage/Pedestal").is_some());
    assert!(tree.get_node_by_path("/root/Stage/KeyLight").is_some());
    assert!(tree.get_node_by_path("/root/Stage/FillLight").is_some());
    assert!(tree.get_node_by_path("/root/Stage/RimLight").is_some());
    assert!(tree.get_node_by_path("/root/Stage/AccentSpot").is_some());
    assert!(tree.get_node_by_path("/root/Stage/Floor").is_some());
}

#[test]
fn multi_light_3d_four_lights() {
    let _g = setup();
    let tree = load_tscn_to_tree("multi_light_3d.tscn");

    let key_id = tree.get_node_by_path("/root/Stage/KeyLight").unwrap();
    assert_eq!(
        tree.get_node(key_id).unwrap().class_name(),
        "DirectionalLight3D"
    );
    assert_eq!(
        tree.get_node(key_id).unwrap().get_property("light_energy"),
        Variant::Float(1.0)
    );

    let fill_id = tree.get_node_by_path("/root/Stage/FillLight").unwrap();
    assert_eq!(tree.get_node(fill_id).unwrap().class_name(), "OmniLight3D");
    assert_eq!(
        tree.get_node(fill_id).unwrap().get_property("light_energy"),
        Variant::Float(0.6)
    );

    let rim_id = tree.get_node_by_path("/root/Stage/RimLight").unwrap();
    assert_eq!(
        tree.get_node(rim_id).unwrap().get_property("light_energy"),
        Variant::Float(0.8)
    );

    let accent_id = tree.get_node_by_path("/root/Stage/AccentSpot").unwrap();
    assert_eq!(
        tree.get_node(accent_id)
            .unwrap()
            .get_property("light_energy"),
        Variant::Float(0.4)
    );
}

#[test]
fn multi_light_3d_meshes() {
    let _g = setup();
    let tree = load_tscn_to_tree("multi_light_3d.tscn");

    let sphere_id = tree.get_node_by_path("/root/Stage/Sphere").unwrap();
    assert_eq!(
        tree.get_node(sphere_id).unwrap().class_name(),
        "MeshInstance3D"
    );

    let ped_id = tree.get_node_by_path("/root/Stage/Pedestal").unwrap();
    assert_eq!(
        tree.get_node(ped_id).unwrap().class_name(),
        "MeshInstance3D"
    );
}

#[test]
fn multi_light_3d_node_count() {
    let _g = setup();
    let tree = load_tscn_to_tree("multi_light_3d.tscn");
    // root + Stage + Camera + Sphere + Pedestal + KeyLight + FillLight + RimLight + AccentSpot + Floor + FloorShape = 11
    assert_eq!(tree.node_count(), 11);
}

#[test]
fn multi_light_3d_camera_properties() {
    let _g = setup();
    let tree = load_tscn_to_tree("multi_light_3d.tscn");
    let cam_id = tree.get_node_by_path("/root/Stage/Camera").unwrap();
    let cam = tree.get_node(cam_id).unwrap();
    assert_eq!(cam.get_property("fov"), Variant::Float(65.0));
    assert_eq!(cam.get_property("far"), Variant::Float(200.0));
}

// ===========================================================================
// 6. Golden scene JSON — structure validation
// ===========================================================================

#[test]
fn golden_indoor_3d_node_count_matches() {
    let golden = load_golden_scene("indoor_3d");
    let nodes = golden["data"]["nodes"].as_array().unwrap();
    // Room + Camera + Table + Chair + Lamp + CeilingLight + Floor + FloorShape + Wall_Back + WallShape = 10
    assert_eq!(nodes.len(), 10);
}

#[test]
fn golden_physics_3d_playground_node_count_matches() {
    let golden = load_golden_scene("physics_3d_playground");
    let nodes = golden["data"]["nodes"].as_array().unwrap();
    // World + Camera + Sun + Ball + BallShape + BallMesh + Cube + CubeShape + CubeMesh
    // + HeavyBlock + HeavyShape + Platform + PlatformShape + PlatformMesh + Ramp + RampShape = 16
    assert_eq!(nodes.len(), 16);
}

#[test]
fn golden_multi_light_3d_node_count_matches() {
    let golden = load_golden_scene("multi_light_3d");
    let nodes = golden["data"]["nodes"].as_array().unwrap();
    // Stage + Camera + Sphere + Pedestal + KeyLight + FillLight + RimLight + AccentSpot + Floor + FloorShape = 10
    assert_eq!(nodes.len(), 10);
}

#[test]
fn golden_scenes_have_required_fields() {
    for name in &["indoor_3d", "physics_3d_playground", "multi_light_3d"] {
        let golden = load_golden_scene(name);
        assert!(
            golden["fixture_id"].as_str().is_some(),
            "{} missing fixture_id",
            name
        );
        assert!(
            golden["capture_type"].as_str().is_some(),
            "{} missing capture_type",
            name
        );
        assert!(
            golden["data"]["nodes"].as_array().is_some(),
            "{} missing data.nodes array",
            name
        );
    }
}

#[test]
fn golden_scene_nodes_have_required_fields() {
    for name in &["indoor_3d", "physics_3d_playground", "multi_light_3d"] {
        let golden = load_golden_scene(name);
        for node in golden["data"]["nodes"].as_array().unwrap() {
            assert!(
                node["name"].as_str().is_some(),
                "{}: node missing name",
                name
            );
            assert!(
                node["class"].as_str().is_some(),
                "{}: node {} missing class",
                name,
                node["name"]
            );
            assert!(
                node["path"].as_str().is_some(),
                "{}: node {} missing path",
                name,
                node["name"]
            );
        }
    }
}

// ===========================================================================
// 7. Physics golden traces
// ===========================================================================

#[test]
fn golden_rigid_sphere_bounce_3d_exists() {
    let trace = load_golden_3d_trace("rigid_sphere_bounce_3d_20frames");
    assert_eq!(trace.len(), 20);
    assert_eq!(trace[0].name, "Ball");
    assert!((trace[0].position.y - 8.0).abs() < 0.001);
}

#[test]
fn golden_multi_body_3d_exists() {
    let trace = load_golden_3d_trace("multi_body_3d_20frames");
    assert_eq!(trace.len(), 30); // 3 bodies × 10 frames
}

#[test]
fn golden_rigid_sphere_ball_falls_under_gravity() {
    let trace = load_golden_3d_trace("rigid_sphere_bounce_3d_20frames");
    // Ball starts at y=8 and falls
    assert!(trace[0].position.y > trace[19].position.y);
    // Velocity increases in magnitude (downward)
    assert!(trace[19].velocity.y < trace[0].velocity.y);
}

#[test]
fn golden_multi_body_all_bodies_present() {
    let trace = load_golden_3d_trace("multi_body_3d_20frames");
    let names: std::collections::HashSet<&str> = trace.iter().map(|e| e.name.as_str()).collect();
    assert!(names.contains("Ball"));
    assert!(names.contains("Cube"));
    assert!(names.contains("HeavyBlock"));
}

#[test]
fn golden_multi_body_all_fall_under_gravity() {
    let trace = load_golden_3d_trace("multi_body_3d_20frames");

    let ball_start = trace
        .iter()
        .find(|e| e.name == "Ball" && e.frame == 0)
        .unwrap();
    let ball_end = trace.iter().filter(|e| e.name == "Ball").last().unwrap();
    assert!(ball_start.position.y > ball_end.position.y);

    let cube_start = trace
        .iter()
        .find(|e| e.name == "Cube" && e.frame == 0)
        .unwrap();
    let cube_end = trace.iter().filter(|e| e.name == "Cube").last().unwrap();
    assert!(cube_start.position.y > cube_end.position.y);

    let heavy_start = trace
        .iter()
        .find(|e| e.name == "HeavyBlock" && e.frame == 0)
        .unwrap();
    let heavy_end = trace
        .iter()
        .filter(|e| e.name == "HeavyBlock")
        .last()
        .unwrap();
    assert!(heavy_start.position.y > heavy_end.position.y);
}

#[test]
fn golden_rigid_sphere_self_comparison_exact() {
    let trace = load_golden_3d_trace("rigid_sphere_bounce_3d_20frames");
    let result = compare_physics_traces(&trace, &trace, 0.0, 0.0);
    assert!(result.is_exact_match());
    assert_eq!(result.total_entries, 20);
}

#[test]
fn golden_multi_body_self_comparison_exact() {
    let trace = load_golden_3d_trace("multi_body_3d_20frames");
    let result = compare_physics_traces(&trace, &trace, 0.0, 0.0);
    assert!(result.is_exact_match());
    assert_eq!(result.total_entries, 30);
}

// ===========================================================================
// 8. Cross-fixture consistency — all fixtures parse without error
// ===========================================================================

#[test]
fn all_3d_fixtures_parse_successfully() {
    let _g = setup();
    for fixture in &[
        "minimal_3d.tscn",
        "indoor_3d.tscn",
        "physics_3d_playground.tscn",
        "multi_light_3d.tscn",
        "hierarchy_3d.tscn",
        "outdoor_3d.tscn",
        "vehicle_3d.tscn",
        "spotlight_gallery_3d.tscn",
        "animated_scene_3d.tscn",
        "physics_playground_extended.tscn",
        "foggy_terrain_3d.tscn",
        "csg_composition.tscn",
    ] {
        let path = fixture_path(fixture);
        let source = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("should read {}: {}", fixture, e));
        let scene = gdscene::packed_scene::PackedScene::from_tscn(&source)
            .unwrap_or_else(|e| panic!("parse {}: {:?}", fixture, e));
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        gdscene::packed_scene::add_packed_scene_to_tree(&mut tree, root, &scene)
            .unwrap_or_else(|e| panic!("add {} to tree: {:?}", fixture, e));
        assert!(
            tree.node_count() > 1,
            "{} should have more than root node",
            fixture
        );
    }
}

#[test]
fn corpus_has_at_least_10_scenes() {
    let fixtures = [
        "minimal_3d.tscn",
        "indoor_3d.tscn",
        "physics_3d_playground.tscn",
        "multi_light_3d.tscn",
        "hierarchy_3d.tscn",
        "outdoor_3d.tscn",
        "vehicle_3d.tscn",
        "spotlight_gallery_3d.tscn",
        "animated_scene_3d.tscn",
        "physics_playground_extended.tscn",
        "foggy_terrain_3d.tscn",
        "csg_composition.tscn",
    ];
    assert!(
        fixtures.len() >= 12,
        "corpus must have at least 12 representative 3D scenes, got {}",
        fixtures.len()
    );
    for f in &fixtures {
        let path = fixture_path(f);
        assert!(
            std::path::Path::new(&path).exists(),
            "fixture {} missing",
            f
        );
    }
}

#[test]
fn all_3d_golden_scenes_valid_json() {
    for name in &[
        "minimal_3d",
        "indoor_3d",
        "physics_3d_playground",
        "multi_light_3d",
        "hierarchy_3d",
        "outdoor_3d",
        "vehicle_3d",
        "spotlight_gallery_3d",
        "animated_scene_3d",
        "physics_playground_extended",
        "foggy_terrain_3d",
        "csg_composition",
    ] {
        let golden = load_golden_scene(name);
        // Some golden files use "data.nodes", others use flat "nodes"
        let nodes = golden["data"]["nodes"]
            .as_array()
            .or_else(|| golden["nodes"].as_array())
            .unwrap_or_else(|| panic!("{} golden has no nodes array", name));
        assert!(!nodes.is_empty(), "{} golden has no nodes", name);
    }
}

#[test]
fn all_3d_physics_golden_traces_valid() {
    for name in &[
        "minimal_3d_10frames",
        "rigid_sphere_bounce_3d_20frames",
        "multi_body_3d_20frames",
    ] {
        let trace = load_golden_3d_trace(name);
        assert!(!trace.is_empty(), "{} golden trace is empty", name);
        // All entries should have valid frame numbers
        for entry in &trace {
            assert!(!entry.name.is_empty(), "{}: entry has empty name", name);
        }
    }
}

// ===========================================================================
// 9. Fixture diversity — scenes cover different node class combinations
// ===========================================================================

#[test]
fn fixture_corpus_covers_all_3d_node_classes() {
    let _g = setup();

    let mut seen_classes = std::collections::HashSet::new();

    for fixture in &[
        "minimal_3d.tscn",
        "indoor_3d.tscn",
        "physics_3d_playground.tscn",
        "multi_light_3d.tscn",
        "hierarchy_3d.tscn",
        "outdoor_3d.tscn",
        "vehicle_3d.tscn",
        "spotlight_gallery_3d.tscn",
        "animated_scene_3d.tscn",
        "physics_playground_extended.tscn",
        "foggy_terrain_3d.tscn",
        "csg_composition.tscn",
    ] {
        let tree = load_tscn_to_tree(fixture);
        for node_id in tree.all_nodes_in_tree_order() {
            if let Some(node) = tree.get_node(node_id) {
                seen_classes.insert(node.class_name().to_string());
            }
        }
    }

    // The corpus must collectively cover these 3D classes from the Phase 6 audit
    for required in &[
        "Node3D",
        "Camera3D",
        "MeshInstance3D",
        "DirectionalLight3D",
        "OmniLight3D",
        "StaticBody3D",
        "CollisionShape3D",
        "RigidBody3D",
        "SpotLight3D",
        "Skeleton3D",
        "AnimationPlayer",
        "FogVolume",
        "WorldEnvironment",
        "CSGCombiner3D",
        "CSGBox3D",
        "ReflectionProbe",
    ] {
        assert!(
            seen_classes.contains(*required),
            "missing {} in corpus (found: {:?})",
            required,
            seen_classes
        );
    }
}

// ===========================================================================
// 10. New fixture scenes — outdoor_3d
// ===========================================================================

#[test]
fn outdoor_3d_tscn_exists() {
    assert!(std::path::Path::new(&fixture_path("outdoor_3d.tscn")).exists());
}

#[test]
fn outdoor_3d_loads_terrain_and_trees() {
    let _g = setup();
    let tree = load_tscn_to_tree("outdoor_3d.tscn");
    assert!(tree.get_node_by_path("/root/Outdoor").is_some());
    assert!(tree.get_node_by_path("/root/Outdoor/Terrain").is_some());
    assert!(tree.get_node_by_path("/root/Outdoor/Tree1").is_some());
    assert!(tree.get_node_by_path("/root/Outdoor/Tree2").is_some());
    assert!(tree.get_node_by_path("/root/Outdoor/Rock").is_some());
    assert!(tree
        .get_node_by_path("/root/Outdoor/AmbientLight")
        .is_some());
}

#[test]
fn outdoor_3d_rock_is_rigid_body() {
    let _g = setup();
    let tree = load_tscn_to_tree("outdoor_3d.tscn");
    let rock_id = tree.get_node_by_path("/root/Outdoor/Rock").unwrap();
    assert_eq!(tree.get_node(rock_id).unwrap().class_name(), "RigidBody3D");
    assert_eq!(
        tree.get_node(rock_id).unwrap().get_property("mass"),
        Variant::Float(50.0)
    );
}

#[test]
fn golden_outdoor_3d_valid() {
    let golden = load_golden_scene("outdoor_3d");
    assert_eq!(golden["fixture_id"].as_str().unwrap(), "scene_outdoor_3d");
    assert_eq!(golden["data"]["nodes"].as_array().unwrap().len(), 12);
}

// ===========================================================================
// 11. New fixture scenes — vehicle_3d
// ===========================================================================

#[test]
fn vehicle_3d_tscn_exists() {
    assert!(std::path::Path::new(&fixture_path("vehicle_3d.tscn")).exists());
}

#[test]
fn vehicle_3d_loads_chassis_and_wheels() {
    let _g = setup();
    let tree = load_tscn_to_tree("vehicle_3d.tscn");
    assert!(tree.get_node_by_path("/root/Track/Chassis").is_some());
    assert!(tree
        .get_node_by_path("/root/Track/Chassis/WheelFL")
        .is_some());
    assert!(tree
        .get_node_by_path("/root/Track/Chassis/WheelFR")
        .is_some());
    assert!(tree
        .get_node_by_path("/root/Track/Chassis/WheelRL")
        .is_some());
    assert!(tree
        .get_node_by_path("/root/Track/Chassis/WheelRR")
        .is_some());
}

#[test]
fn vehicle_3d_chassis_mass() {
    let _g = setup();
    let tree = load_tscn_to_tree("vehicle_3d.tscn");
    let chassis_id = tree.get_node_by_path("/root/Track/Chassis").unwrap();
    assert_eq!(
        tree.get_node(chassis_id).unwrap().get_property("mass"),
        Variant::Float(800.0)
    );
}

#[test]
fn golden_vehicle_3d_valid() {
    let golden = load_golden_scene("vehicle_3d");
    assert_eq!(golden["fixture_id"].as_str().unwrap(), "scene_vehicle_3d");
    let nodes = golden["data"]["nodes"].as_array().unwrap();
    assert_eq!(nodes.len(), 17);
}

// ===========================================================================
// 12. New fixture scenes — spotlight_gallery_3d
// ===========================================================================

#[test]
fn spotlight_gallery_3d_tscn_exists() {
    assert!(std::path::Path::new(&fixture_path("spotlight_gallery_3d.tscn")).exists());
}

#[test]
fn spotlight_gallery_3d_three_spots() {
    let _g = setup();
    let tree = load_tscn_to_tree("spotlight_gallery_3d.tscn");
    for name in &["SpotRed", "SpotGreen", "SpotBlue"] {
        let path = format!("/root/Gallery/{}", name);
        let id = tree
            .get_node_by_path(&path)
            .unwrap_or_else(|| panic!("{} not found", path));
        assert_eq!(
            tree.get_node(id).unwrap().class_name(),
            "SpotLight3D",
            "{} should be SpotLight3D",
            name
        );
    }
}

#[test]
fn spotlight_gallery_3d_spot_energies() {
    let _g = setup();
    let tree = load_tscn_to_tree("spotlight_gallery_3d.tscn");

    let red_id = tree.get_node_by_path("/root/Gallery/SpotRed").unwrap();
    assert_eq!(
        tree.get_node(red_id).unwrap().get_property("light_energy"),
        Variant::Float(2.0)
    );

    let green_id = tree.get_node_by_path("/root/Gallery/SpotGreen").unwrap();
    assert_eq!(
        tree.get_node(green_id)
            .unwrap()
            .get_property("light_energy"),
        Variant::Float(1.5)
    );
}

#[test]
fn golden_spotlight_gallery_3d_valid() {
    let golden = load_golden_scene("spotlight_gallery_3d");
    assert_eq!(
        golden["fixture_id"].as_str().unwrap(),
        "scene_spotlight_gallery_3d"
    );
    assert_eq!(golden["data"]["nodes"].as_array().unwrap().len(), 11);
}

// ===========================================================================
// 13. New fixture scenes — animated_scene_3d
// ===========================================================================

#[test]
fn animated_scene_3d_tscn_exists() {
    assert!(std::path::Path::new(&fixture_path("animated_scene_3d.tscn")).exists());
}

#[test]
fn animated_scene_3d_has_skeleton_and_anim() {
    let _g = setup();
    let tree = load_tscn_to_tree("animated_scene_3d.tscn");
    assert!(tree
        .get_node_by_path("/root/Scene/Character/Skeleton")
        .is_some());
    assert!(tree
        .get_node_by_path("/root/Scene/Character/AnimPlayer")
        .is_some());

    let skel_id = tree
        .get_node_by_path("/root/Scene/Character/Skeleton")
        .unwrap();
    assert_eq!(tree.get_node(skel_id).unwrap().class_name(), "Skeleton3D");

    let anim_id = tree
        .get_node_by_path("/root/Scene/Character/AnimPlayer")
        .unwrap();
    assert_eq!(
        tree.get_node(anim_id).unwrap().class_name(),
        "AnimationPlayer"
    );
}

#[test]
fn golden_animated_scene_3d_valid() {
    let golden = load_golden_scene("animated_scene_3d");
    assert_eq!(
        golden["fixture_id"].as_str().unwrap(),
        "scene_animated_scene_3d"
    );
    assert_eq!(golden["data"]["nodes"].as_array().unwrap().len(), 11);
}

// ===========================================================================
// 14. New fixture scenes — foggy_terrain_3d
// ===========================================================================

#[test]
fn foggy_terrain_3d_tscn_exists() {
    assert!(std::path::Path::new(&fixture_path("foggy_terrain_3d.tscn")).exists());
}

#[test]
fn foggy_terrain_3d_loads_all_nodes() {
    let _g = setup();
    let tree = load_tscn_to_tree("foggy_terrain_3d.tscn");

    assert!(tree.get_node_by_path("/root/World").is_some());
    assert!(tree.get_node_by_path("/root/World/Camera").is_some());
    assert!(tree.get_node_by_path("/root/World/Terrain").is_some());
    assert!(tree.get_node_by_path("/root/World/TerrainBody").is_some());
    assert!(tree
        .get_node_by_path("/root/World/TerrainBody/TerrainCollision")
        .is_some());
    assert!(tree.get_node_by_path("/root/World/Sun").is_some());
    assert!(tree.get_node_by_path("/root/World/FillLight").is_some());
    assert!(tree.get_node_by_path("/root/World/FogVolume").is_some());
    assert!(tree
        .get_node_by_path("/root/World/WorldEnvironment")
        .is_some());
    assert!(tree.get_node_by_path("/root/World/Tree1").is_some());
    assert!(tree.get_node_by_path("/root/World/Tree2").is_some());
    assert!(tree.get_node_by_path("/root/World/Rock1").is_some());
    assert!(tree
        .get_node_by_path("/root/World/Rock1/RockCollision")
        .is_some());
}

#[test]
fn foggy_terrain_3d_node_count() {
    let _g = setup();
    let tree = load_tscn_to_tree("foggy_terrain_3d.tscn");
    // root + World + Camera + Terrain + TerrainBody + TerrainCollision + Sun + FillLight
    // + FogVolume + WorldEnvironment + Tree1 + Tree2 + Rock1 + RockCollision = 14
    assert_eq!(tree.node_count(), 14);
}

#[test]
fn foggy_terrain_3d_node_classes() {
    let _g = setup();
    let tree = load_tscn_to_tree("foggy_terrain_3d.tscn");

    let fog_id = tree.get_node_by_path("/root/World/FogVolume").unwrap();
    assert_eq!(tree.get_node(fog_id).unwrap().class_name(), "FogVolume");

    let env_id = tree
        .get_node_by_path("/root/World/WorldEnvironment")
        .unwrap();
    assert_eq!(
        tree.get_node(env_id).unwrap().class_name(),
        "WorldEnvironment"
    );

    let rock_id = tree.get_node_by_path("/root/World/Rock1").unwrap();
    assert_eq!(tree.get_node(rock_id).unwrap().class_name(), "RigidBody3D");
}

#[test]
fn foggy_terrain_3d_camera_properties() {
    let _g = setup();
    let tree = load_tscn_to_tree("foggy_terrain_3d.tscn");
    let cam_id = tree.get_node_by_path("/root/World/Camera").unwrap();
    let cam = tree.get_node(cam_id).unwrap();
    assert_eq!(cam.get_property("fov"), Variant::Float(70.0));
    assert_eq!(cam.get_property("far"), Variant::Float(500.0));
}

#[test]
fn foggy_terrain_3d_light_properties() {
    let _g = setup();
    let tree = load_tscn_to_tree("foggy_terrain_3d.tscn");

    let sun_id = tree.get_node_by_path("/root/World/Sun").unwrap();
    let sun = tree.get_node(sun_id).unwrap();
    assert_eq!(sun.get_property("light_energy"), Variant::Float(0.8));
    assert_eq!(sun.get_property("shadow_enabled"), Variant::Bool(true));

    let fill_id = tree.get_node_by_path("/root/World/FillLight").unwrap();
    let fill = tree.get_node(fill_id).unwrap();
    assert_eq!(fill.get_property("light_energy"), Variant::Float(0.4));
    assert_eq!(fill.get_property("omni_range"), Variant::Float(20.0));
}

#[test]
fn foggy_terrain_3d_rock_mass() {
    let _g = setup();
    let tree = load_tscn_to_tree("foggy_terrain_3d.tscn");
    let rock_id = tree.get_node_by_path("/root/World/Rock1").unwrap();
    assert_eq!(
        tree.get_node(rock_id).unwrap().get_property("mass"),
        Variant::Float(50.0)
    );
}

#[test]
fn golden_foggy_terrain_3d_valid() {
    let golden = load_golden_scene("foggy_terrain_3d");
    assert_eq!(
        golden["fixture_id"].as_str().unwrap(),
        "scene_foggy_terrain_3d"
    );
    let nodes = golden["data"]["nodes"].as_array().unwrap();
    assert_eq!(nodes.len(), 13);
}

#[test]
fn golden_foggy_terrain_3d_has_fog_and_environment() {
    let golden = load_golden_scene("foggy_terrain_3d");
    let nodes = golden["data"]["nodes"].as_array().unwrap();
    let classes: Vec<&str> = nodes.iter().map(|n| n["class"].as_str().unwrap()).collect();
    assert!(
        classes.contains(&"FogVolume"),
        "missing FogVolume in golden"
    );
    assert!(
        classes.contains(&"WorldEnvironment"),
        "missing WorldEnvironment in golden"
    );
}

// ===========================================================================
// 15. CSG composition fixture — CSG families + ReflectionProbe
// ===========================================================================

#[test]
fn csg_composition_tscn_exists() {
    assert!(std::path::Path::new(&fixture_path("csg_composition.tscn")).exists());
}

#[test]
fn csg_composition_loads_all_nodes() {
    let _g = setup();
    let tree = load_tscn_to_tree("csg_composition.tscn");

    assert!(tree.get_node_by_path("/root/CSGWorld").is_some());
    assert!(tree.get_node_by_path("/root/CSGWorld/Combiner").is_some());
    assert!(tree
        .get_node_by_path("/root/CSGWorld/Combiner/Box")
        .is_some());
    assert!(tree
        .get_node_by_path("/root/CSGWorld/Combiner/Sphere")
        .is_some());
    assert!(tree
        .get_node_by_path("/root/CSGWorld/Combiner/Cylinder")
        .is_some());
    assert!(tree.get_node_by_path("/root/CSGWorld/Standalone").is_some());
    assert!(tree.get_node_by_path("/root/CSGWorld/Probe").is_some());
}

#[test]
fn csg_composition_node_classes() {
    let _g = setup();
    let tree = load_tscn_to_tree("csg_composition.tscn");

    let combiner_id = tree.get_node_by_path("/root/CSGWorld/Combiner").unwrap();
    assert_eq!(
        tree.get_node(combiner_id).unwrap().class_name(),
        "CSGCombiner3D"
    );

    let box_id = tree
        .get_node_by_path("/root/CSGWorld/Combiner/Box")
        .unwrap();
    assert_eq!(tree.get_node(box_id).unwrap().class_name(), "CSGBox3D");

    let sphere_id = tree
        .get_node_by_path("/root/CSGWorld/Combiner/Sphere")
        .unwrap();
    assert_eq!(
        tree.get_node(sphere_id).unwrap().class_name(),
        "CSGSphere3D"
    );

    let probe_id = tree.get_node_by_path("/root/CSGWorld/Probe").unwrap();
    assert_eq!(
        tree.get_node(probe_id).unwrap().class_name(),
        "ReflectionProbe"
    );
}

#[test]
fn golden_csg_composition_valid() {
    let golden = load_golden_scene("csg_composition");
    // csg_composition golden uses flat "nodes" key (not nested under "data")
    let nodes = golden["nodes"]
        .as_array()
        .or_else(|| golden["data"]["nodes"].as_array())
        .expect("csg_composition golden must have nodes array");
    assert!(!nodes.is_empty(), "csg_composition golden has no nodes");
}

/// Recursively collect all "class" values from a nested node tree.
fn collect_classes(node: &serde_json::Value, out: &mut Vec<String>) {
    if let Some(class) = node["class"].as_str() {
        out.push(class.to_string());
    }
    if let Some(children) = node["children"].as_array() {
        for child in children {
            collect_classes(child, out);
        }
    }
}

#[test]
fn golden_csg_composition_has_csg_and_probe() {
    let golden = load_golden_scene("csg_composition");
    let nodes = golden["nodes"]
        .as_array()
        .or_else(|| golden["data"]["nodes"].as_array())
        .expect("csg_composition golden must have nodes array");

    let mut classes = Vec::new();
    for node in nodes {
        collect_classes(node, &mut classes);
    }
    assert!(
        classes.iter().any(|c| c == "CSGCombiner3D"),
        "missing CSGCombiner3D in golden (found: {:?})",
        classes
    );
    assert!(
        classes.iter().any(|c| c == "CSGBox3D"),
        "missing CSGBox3D in golden (found: {:?})",
        classes
    );
    assert!(
        classes.iter().any(|c| c == "ReflectionProbe"),
        "missing ReflectionProbe in golden (found: {:?})",
        classes
    );
}

// ===========================================================================
// 16. Corpus ↔ Phase 6 audit mapping validation
// ===========================================================================

/// Validates that the corpus definition in prd/PHASE6_3D_PARITY_AUDIT.md
/// stays in sync with the actual fixture corpus and audited class families.
#[test]
fn fixture_corpus_maps_to_audited_families() {
    let audit_path = format!(
        "{}/../prd/PHASE6_3D_PARITY_AUDIT.md",
        env!("CARGO_MANIFEST_DIR")
    );
    let audit =
        std::fs::read_to_string(&audit_path).expect("should read PHASE6_3D_PARITY_AUDIT.md");

    // The audit must contain the corpus definition section
    assert!(
        audit.contains("## 3D Fixture Corpus Definition"),
        "PHASE6_3D_PARITY_AUDIT.md must contain '## 3D Fixture Corpus Definition'"
    );

    // The audit must reference each corpus fixture
    for fixture in &[
        "minimal_3d.tscn",
        "indoor_3d.tscn",
        "physics_3d_playground.tscn",
        "multi_light_3d.tscn",
        "hierarchy_3d.tscn",
        "outdoor_3d.tscn",
        "vehicle_3d.tscn",
        "spotlight_gallery_3d.tscn",
        "animated_scene_3d.tscn",
        "physics_playground_extended.tscn",
        "foggy_terrain_3d.tscn",
        "csg_composition.tscn",
    ] {
        assert!(
            audit.contains(fixture),
            "PHASE6_3D_PARITY_AUDIT.md corpus section must reference {}",
            fixture
        );
    }

    // The audit must reference the coverage map for each measured family
    for family in &[
        "Node3D",
        "Camera3D",
        "MeshInstance3D",
        "DirectionalLight3D",
        "OmniLight3D",
        "SpotLight3D",
        "RigidBody3D",
        "StaticBody3D",
        "CollisionShape3D",
        "Skeleton3D",
        "FogVolume",
        "WorldEnvironment",
        "ReflectionProbe",
    ] {
        assert!(
            audit.contains(family),
            "PHASE6_3D_PARITY_AUDIT.md must reference audited family {}",
            family
        );
    }
}

/// Validates that every corpus fixture has a matching golden JSON file.
#[test]
fn all_corpus_fixtures_have_golden_json() {
    for name in &[
        "minimal_3d",
        "indoor_3d",
        "physics_3d_playground",
        "multi_light_3d",
        "hierarchy_3d",
        "outdoor_3d",
        "vehicle_3d",
        "spotlight_gallery_3d",
        "animated_scene_3d",
        "physics_playground_extended",
        "foggy_terrain_3d",
        "csg_composition",
    ] {
        let golden_path = golden_scene_path(name);
        assert!(
            std::path::Path::new(&golden_path).exists(),
            "corpus fixture {} must have golden JSON at {}",
            name,
            golden_path
        );
        let golden = load_golden_scene(name);
        // Some golden files use "data.nodes", others use flat "nodes"
        let has_nodes =
            golden["data"]["nodes"].as_array().is_some() || golden["nodes"].as_array().is_some();
        assert!(
            has_nodes,
            "{} golden must have nodes array (checked data.nodes and nodes)",
            name
        );
    }
}
