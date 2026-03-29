//! pat-sp67: Hierarchy 3D fixture — nested Node3D chain for global transform
//! accumulation.
//!
//! This fixture completes the 5-fixture set described in
//! `docs/3D_ARCHITECTURE_SPEC.md`:
//!   1. Minimal 3D (minimal_3d.tscn)
//!   2. Camera + Mesh (indoor_3d.tscn)
//!   3. Lit scene (multi_light_3d.tscn)
//!   4. Physics scene (physics_3d_playground.tscn)
//!   5. **Hierarchy scene** (hierarchy_3d.tscn) ← this file
//!
//! The hierarchy scene has a nested Node3D chain: World → Arm → Forearm → Hand
//! with cumulative local transforms that must compose to correct global positions.

use std::sync::Mutex;

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

fn find_node_by_name<'a>(
    tree: &'a SceneTree,
    name: &str,
) -> gdscene::node::NodeId {
    tree.all_nodes_in_tree_order()
        .into_iter()
        .find(|id| tree.get_node(*id).map(|n| n.name()) == Some(name))
        .unwrap_or_else(|| panic!("node '{}' must exist", name))
}

// ===========================================================================
// 1. Fixture and golden files exist
// ===========================================================================

#[test]
fn hierarchy_3d_tscn_exists() {
    let path = fixture_path("hierarchy_3d.tscn");
    assert!(
        std::path::Path::new(&path).exists(),
        "hierarchy_3d.tscn fixture missing"
    );
}

#[test]
fn hierarchy_3d_golden_exists_and_valid() {
    let path = golden_scene_path("hierarchy_3d");
    assert!(
        std::path::Path::new(&path).exists(),
        "hierarchy_3d.json golden missing"
    );
    let data = std::fs::read_to_string(&path).unwrap();
    let golden: serde_json::Value = serde_json::from_str(&data).unwrap();
    assert_eq!(
        golden["fixture_id"].as_str().unwrap(),
        "scene_hierarchy_3d"
    );
    let nodes = golden["data"]["nodes"].as_array().unwrap();
    assert_eq!(nodes.len(), 6, "golden should have 6 nodes");
}

// ===========================================================================
// 2. Scene loads and has correct node count
// ===========================================================================

#[test]
fn hierarchy_3d_loads_all_nodes() {
    let _g = setup();
    let tree = load_tscn_to_tree("hierarchy_3d.tscn");
    let all = tree.all_nodes_in_tree_order();
    // root + World + Arm + Forearm + Hand + Camera + Light = 7
    assert!(
        all.len() >= 7,
        "hierarchy_3d should have at least 7 nodes (including root), got {}",
        all.len()
    );
}

#[test]
fn hierarchy_3d_expected_node_names() {
    let _g = setup();
    let tree = load_tscn_to_tree("hierarchy_3d.tscn");
    let all = tree.all_nodes_in_tree_order();
    let names: Vec<_> = all
        .iter()
        .filter_map(|id| tree.get_node(*id))
        .map(|n| n.name().to_string())
        .collect();

    for expected in &["World", "Arm", "Forearm", "Hand", "Camera", "Light"] {
        assert!(
            names.contains(&expected.to_string()),
            "Must have {} node, got {:?}",
            expected,
            names
        );
    }
}

// ===========================================================================
// 3. Correct node classes
// ===========================================================================

#[test]
fn hierarchy_3d_correct_classes() {
    let _g = setup();
    let tree = load_tscn_to_tree("hierarchy_3d.tscn");

    let expected = [
        ("World", "Node3D"),
        ("Arm", "Node3D"),
        ("Forearm", "Node3D"),
        ("Hand", "MeshInstance3D"),
        ("Camera", "Camera3D"),
        ("Light", "DirectionalLight3D"),
    ];

    for (name, class) in &expected {
        let nid = find_node_by_name(&tree, name);
        let node = tree.get_node(nid).unwrap();
        assert_eq!(
            node.class_name(),
            *class,
            "{} should be {}, got {}",
            name,
            class,
            node.class_name()
        );
    }
}

// ===========================================================================
// 4. Nested depth — Hand is 4 levels below root
// ===========================================================================

#[test]
fn hierarchy_3d_nested_depth() {
    let _g = setup();
    let tree = load_tscn_to_tree("hierarchy_3d.tscn");
    let hand_id = find_node_by_name(&tree, "Hand");

    // Walk up the parent chain
    let mut chain = Vec::new();
    let mut current = hand_id;
    loop {
        if let Some(node) = tree.get_node(current) {
            chain.push(node.name().to_string());
            if let Some(parent) = node.parent() {
                current = parent;
            } else {
                break;
            }
        } else {
            break;
        }
    }
    chain.reverse();

    // Expected chain: root → World → Arm → Forearm → Hand
    assert!(
        chain.len() >= 5,
        "parent chain should be at least 5 deep, got {:?}",
        chain
    );
    assert_eq!(chain[chain.len() - 1], "Hand");
    assert_eq!(chain[chain.len() - 2], "Forearm");
    assert_eq!(chain[chain.len() - 3], "Arm");
    assert_eq!(chain[chain.len() - 4], "World");
}

// ===========================================================================
// 5. Global transform accumulation
// ===========================================================================

#[test]
fn hierarchy_3d_transform_accumulation() {
    let _g = setup();
    let tree = load_tscn_to_tree("hierarchy_3d.tscn");

    // Expected global positions:
    // World: local(2,0,0) → global(2,0,0)
    // Arm: local(0,3,0) under World(2,0,0) → global(2,3,0)
    // Forearm: local(0,2,0) under Arm(2,3,0) → global(2,5,0)
    // Hand: local(0,1,0) under Forearm(2,5,0) → global(2,6,0)
    let expected: &[(&str, [f32; 3])] = &[
        ("World", [2.0, 0.0, 0.0]),
        ("Arm", [2.0, 3.0, 0.0]),
        ("Forearm", [2.0, 5.0, 0.0]),
        ("Hand", [2.0, 6.0, 0.0]),
    ];

    for (name, expected_pos) in expected {
        let node_id = find_node_by_name(&tree, name);
        let global_transform = gdscene::node3d::get_global_transform(&tree, node_id);
        let global_pos = global_transform.xform(Vector3::ZERO);

        let eps = 0.001;
        assert!(
            (global_pos.x - expected_pos[0]).abs() < eps
                && (global_pos.y - expected_pos[1]).abs() < eps
                && (global_pos.z - expected_pos[2]).abs() < eps,
            "{}: expected global pos {:?}, got ({}, {}, {})",
            name,
            expected_pos,
            global_pos.x,
            global_pos.y,
            global_pos.z
        );
    }
}

// ===========================================================================
// 6. Golden cross-check — golden transform_chain matches computed globals
// ===========================================================================

#[test]
fn hierarchy_3d_golden_transform_cross_check() {
    let _g = setup();
    let tree = load_tscn_to_tree("hierarchy_3d.tscn");

    let golden_path = golden_scene_path("hierarchy_3d");
    let data = std::fs::read_to_string(&golden_path).unwrap();
    let golden: serde_json::Value = serde_json::from_str(&data).unwrap();
    let chain = &golden["transform_chain"];

    for name in &["World", "Arm", "Forearm", "Hand"] {
        let golden_arr = chain[name]
            .as_array()
            .unwrap_or_else(|| panic!("golden transform_chain missing {}", name));
        let gx = golden_arr[0].as_f64().unwrap() as f32;
        let gy = golden_arr[1].as_f64().unwrap() as f32;
        let gz = golden_arr[2].as_f64().unwrap() as f32;

        let node_id = find_node_by_name(&tree, name);
        let global_transform = gdscene::node3d::get_global_transform(&tree, node_id);
        let global_pos = global_transform.xform(Vector3::ZERO);

        let eps = 0.001;
        assert!(
            (global_pos.x - gx).abs() < eps
                && (global_pos.y - gy).abs() < eps
                && (global_pos.z - gz).abs() < eps,
            "{}: computed ({}, {}, {}) != golden ({}, {}, {})",
            name,
            global_pos.x,
            global_pos.y,
            global_pos.z,
            gx,
            gy,
            gz
        );
    }
}

// ===========================================================================
// 7. Camera and light properties
// ===========================================================================

#[test]
fn hierarchy_3d_camera_properties() {
    let _g = setup();
    let tree = load_tscn_to_tree("hierarchy_3d.tscn");
    let cam_id = find_node_by_name(&tree, "Camera");
    let cam = tree.get_node(cam_id).unwrap();

    if let Variant::Float(fov) = cam.get_property("fov") {
        assert!(
            (fov - 70.0).abs() < 0.1,
            "fov should be 70.0, got {}",
            fov
        );
    }
    if let Variant::Float(far) = cam.get_property("far") {
        assert!(
            (far - 100.0).abs() < 0.1,
            "far should be 100.0, got {}",
            far
        );
    }
    if let Variant::Float(near) = cam.get_property("near") {
        assert!(
            (near - 0.1).abs() < 0.01,
            "near should be 0.1, got {}",
            near
        );
    }
}

#[test]
fn hierarchy_3d_light_properties() {
    let _g = setup();
    let tree = load_tscn_to_tree("hierarchy_3d.tscn");
    let light_id = find_node_by_name(&tree, "Light");
    let light = tree.get_node(light_id).unwrap();

    assert_eq!(light.class_name(), "DirectionalLight3D");
    if let Variant::Float(energy) = light.get_property("light_energy") {
        assert!(
            (energy - 0.8).abs() < 0.01,
            "light_energy should be 0.8, got {}",
            energy
        );
    }
    if let Variant::Bool(shadow) = light.get_property("shadow_enabled") {
        assert!(shadow, "shadow_enabled should be true");
    }
}

// ===========================================================================
// 8. Corpus completeness — hierarchy_3d + existing = all 5 spec fixtures
// ===========================================================================

#[test]
fn all_five_spec_fixtures_exist() {
    for name in &[
        "minimal_3d.tscn",
        "indoor_3d.tscn",
        "multi_light_3d.tscn",
        "physics_3d_playground.tscn",
        "hierarchy_3d.tscn",
    ] {
        let path = fixture_path(name);
        assert!(
            std::path::Path::new(&path).exists(),
            "missing fixture: {}",
            name
        );
    }
}
