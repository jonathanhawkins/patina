//! pat-hby6: First 3D node subset runtime slice.
//!
//! Validates that the core 3D node classes work end-to-end in the runtime:
//! - ClassDB registration via `register_3d_classes()`
//! - Scene tree integration (add, query, property access)
//! - Node3D property helpers (position, rotation, scale, transforms)
//! - Camera3D, MeshInstance3D, Light3D property helpers
//! - 3D fixture loading via PackedScene
//! - Inheritance chain correctness for all 3D classes
//! - Global transform accumulation through parent chain

use std::sync::Mutex;

use gdcore::math::{Color, Vector3};
use gdobject::class_db;
use gdobject::object::GodotObject;
use gdscene::node::Node;
use gdscene::node3d;
use gdscene::scene_tree::SceneTree;
use gdvariant::Variant;

static TEST_LOCK: Mutex<()> = Mutex::new(());

fn setup() -> std::sync::MutexGuard<'static, ()> {
    let guard = TEST_LOCK.lock().expect("test lock poisoned");
    class_db::clear_for_testing();
    // Register base classes, then 3D classes.
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

// ===========================================================================
// 1. ClassDB: register_3d_classes registers all expected classes
// ===========================================================================

#[test]
fn register_3d_classes_registers_node3d() {
    let _g = setup();
    assert!(class_db::class_exists("Node3D"));
    assert!(class_db::is_parent_class("Node3D", "Node"));
}

#[test]
fn register_3d_classes_registers_camera3d() {
    let _g = setup();
    assert!(class_db::class_exists("Camera3D"));
    assert!(class_db::is_parent_class("Camera3D", "Node3D"));
    assert!(class_db::is_parent_class("Camera3D", "Node"));
}

#[test]
fn register_3d_classes_registers_mesh_instance3d() {
    let _g = setup();
    assert!(class_db::class_exists("MeshInstance3D"));
    assert!(class_db::is_parent_class("MeshInstance3D", "Node3D"));
}

#[test]
fn register_3d_classes_registers_directional_light3d() {
    let _g = setup();
    assert!(class_db::class_exists("DirectionalLight3D"));
    assert!(class_db::is_parent_class("DirectionalLight3D", "Node3D"));
}

#[test]
fn register_3d_classes_registers_omni_light3d() {
    let _g = setup();
    assert!(class_db::class_exists("OmniLight3D"));
    assert!(class_db::is_parent_class("OmniLight3D", "Node3D"));
}

#[test]
fn register_3d_classes_registers_static_body3d() {
    let _g = setup();
    assert!(class_db::class_exists("StaticBody3D"));
    assert!(class_db::is_parent_class("StaticBody3D", "Node3D"));
}

#[test]
fn register_3d_classes_registers_rigid_body3d() {
    let _g = setup();
    assert!(class_db::class_exists("RigidBody3D"));
    assert!(class_db::is_parent_class("RigidBody3D", "Node3D"));
}

#[test]
fn register_3d_classes_registers_collision_shape3d() {
    let _g = setup();
    assert!(class_db::class_exists("CollisionShape3D"));
    assert!(class_db::is_parent_class("CollisionShape3D", "Node3D"));
}

#[test]
fn register_3d_classes_registers_character_body3d() {
    let _g = setup();
    assert!(class_db::class_exists("CharacterBody3D"));
    assert!(class_db::is_parent_class("CharacterBody3D", "Node3D"));
    assert!(class_db::class_has_method(
        "CharacterBody3D",
        "move_and_slide"
    ));
    assert!(class_db::class_has_method("CharacterBody3D", "is_on_floor"));
}

#[test]
fn register_3d_classes_registers_area3d() {
    let _g = setup();
    assert!(class_db::class_exists("Area3D"));
    assert!(class_db::is_parent_class("Area3D", "Node3D"));
}

#[test]
fn register_3d_classes_total_count() {
    let _g = setup();
    // Object + Node + 10 3D classes = 12 total.
    assert_eq!(class_db::class_count(), 12);
}

// ===========================================================================
// 2. Inheritance chains
// ===========================================================================

#[test]
fn node3d_inheritance_chain() {
    let _g = setup();
    let chain = class_db::inheritance_chain("Node3D");
    assert_eq!(chain, vec!["Node3D", "Node", "Object"]);
}

#[test]
fn camera3d_inheritance_chain() {
    let _g = setup();
    let chain = class_db::inheritance_chain("Camera3D");
    assert_eq!(chain, vec!["Camera3D", "Node3D", "Node", "Object"]);
}

#[test]
fn rigid_body3d_inheritance_chain() {
    let _g = setup();
    let chain = class_db::inheritance_chain("RigidBody3D");
    assert_eq!(chain, vec!["RigidBody3D", "Node3D", "Node", "Object"]);
}

// ===========================================================================
// 3. Property inheritance (3D classes inherit Node3D properties)
// ===========================================================================

#[test]
fn camera3d_inherits_node3d_properties() {
    let _g = setup();
    assert!(class_db::class_has_property("Camera3D", "position"));
    assert!(class_db::class_has_property("Camera3D", "rotation"));
    assert!(class_db::class_has_property("Camera3D", "scale"));
    assert!(class_db::class_has_property("Camera3D", "visible"));
    // Own properties
    assert!(class_db::class_has_property("Camera3D", "fov"));
    assert!(class_db::class_has_property("Camera3D", "near"));
    assert!(class_db::class_has_property("Camera3D", "far"));
}

#[test]
fn rigid_body3d_inherits_node3d_properties() {
    let _g = setup();
    assert!(class_db::class_has_property("RigidBody3D", "position"));
    assert!(class_db::class_has_property("RigidBody3D", "mass"));
    assert!(class_db::class_has_property(
        "RigidBody3D",
        "linear_velocity"
    ));
}

#[test]
fn mesh_instance3d_inherits_node3d_properties() {
    let _g = setup();
    assert!(class_db::class_has_property("MeshInstance3D", "position"));
    assert!(class_db::class_has_property("MeshInstance3D", "mesh"));
    assert!(class_db::class_has_property(
        "MeshInstance3D",
        "cast_shadow"
    ));
}

// ===========================================================================
// 4. ClassDB instantiation with defaults
// ===========================================================================

#[test]
fn instantiate_node3d_defaults() {
    let _g = setup();
    let obj = class_db::instantiate("Node3D").expect("instantiate Node3D");
    assert_eq!(obj.get_class(), "Node3D");
    assert_eq!(
        obj.get_property("position"),
        Variant::Vector3(Vector3::ZERO)
    );
    assert_eq!(obj.get_property("scale"), Variant::Vector3(Vector3::ONE));
    assert_eq!(obj.get_property("visible"), Variant::Bool(true));
}

#[test]
fn instantiate_camera3d_defaults() {
    let _g = setup();
    let obj = class_db::instantiate("Camera3D").expect("instantiate Camera3D");
    assert_eq!(obj.get_class(), "Camera3D");
    assert_eq!(obj.get_property("fov"), Variant::Float(75.0));
    assert_eq!(obj.get_property("near"), Variant::Float(0.05));
    assert_eq!(obj.get_property("far"), Variant::Float(4000.0));
    assert_eq!(obj.get_property("current"), Variant::Bool(false));
}

#[test]
fn instantiate_rigid_body3d_defaults() {
    let _g = setup();
    let obj = class_db::instantiate("RigidBody3D").expect("instantiate RigidBody3D");
    assert_eq!(obj.get_class(), "RigidBody3D");
    assert_eq!(obj.get_property("mass"), Variant::Float(1.0));
    assert_eq!(obj.get_property("gravity_scale"), Variant::Float(1.0));
    assert_eq!(
        obj.get_property("linear_velocity"),
        Variant::Vector3(Vector3::ZERO)
    );
}

#[test]
fn instantiate_character_body3d_defaults() {
    let _g = setup();
    let obj = class_db::instantiate("CharacterBody3D").expect("instantiate CharacterBody3D");
    assert_eq!(obj.get_class(), "CharacterBody3D");
    assert_eq!(
        obj.get_property("velocity"),
        Variant::Vector3(Vector3::ZERO)
    );
    assert_eq!(
        obj.get_property("up_direction"),
        Variant::Vector3(Vector3::new(0.0, 1.0, 0.0))
    );
}

// ===========================================================================
// 5. Scene tree integration: 3D nodes work in the tree
// ===========================================================================

#[test]
fn add_node3d_to_scene_tree() {
    let _g = setup();
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let node = Node::new("Player", "Node3D");
    let id = tree.add_child(root, node).unwrap();

    assert_eq!(tree.get_node(id).unwrap().name(), "Player");
    assert_eq!(tree.get_node(id).unwrap().class_name(), "Node3D");
}

#[test]
fn node3d_position_in_scene_tree() {
    let _g = setup();
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let node = Node::new("Cube", "Node3D");
    let id = tree.add_child(root, node).unwrap();

    node3d::set_position(&mut tree, id, Vector3::new(1.0, 2.0, 3.0));
    assert_eq!(node3d::get_position(&tree, id), Vector3::new(1.0, 2.0, 3.0));
}

#[test]
fn node3d_rotation_in_scene_tree() {
    let _g = setup();
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let node = Node::new("Rotated", "Node3D");
    let id = tree.add_child(root, node).unwrap();

    let euler = Vector3::new(0.5, 1.0, 0.2);
    node3d::set_rotation(&mut tree, id, euler);
    assert_eq!(node3d::get_rotation(&tree, id), euler);
}

#[test]
fn node3d_scale_in_scene_tree() {
    let _g = setup();
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let node = Node::new("Scaled", "Node3D");
    let id = tree.add_child(root, node).unwrap();

    node3d::set_scale(&mut tree, id, Vector3::new(2.0, 3.0, 4.0));
    assert_eq!(node3d::get_scale(&tree, id), Vector3::new(2.0, 3.0, 4.0));
}

#[test]
fn node3d_visibility_in_scene_tree() {
    let _g = setup();
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let node = Node::new("Toggle", "Node3D");
    let id = tree.add_child(root, node).unwrap();

    assert!(node3d::is_visible(&tree, id)); // default true
    node3d::set_visible(&mut tree, id, false);
    assert!(!node3d::is_visible(&tree, id));
}

// ===========================================================================
// 6. Camera3D property helpers in scene tree
// ===========================================================================

#[test]
fn camera3d_fov_in_scene_tree() {
    let _g = setup();
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let cam = Node::new("MainCamera", "Camera3D");
    let id = tree.add_child(root, cam).unwrap();

    node3d::set_fov(&mut tree, id, 90.0);
    assert!((node3d::get_fov(&tree, id) - 90.0).abs() < 1e-6);
}

#[test]
fn camera3d_clip_planes_in_scene_tree() {
    let _g = setup();
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let cam = Node::new("Camera", "Camera3D");
    let id = tree.add_child(root, cam).unwrap();

    node3d::set_near(&mut tree, id, 0.1);
    node3d::set_far(&mut tree, id, 1000.0);
    assert!((node3d::get_near(&tree, id) - 0.1).abs() < 1e-6);
    assert!((node3d::get_far(&tree, id) - 1000.0).abs() < 1e-6);
}

// ===========================================================================
// 7. MeshInstance3D property helpers in scene tree
// ===========================================================================

#[test]
fn mesh_instance3d_mesh_path_in_scene_tree() {
    let _g = setup();
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let mesh = Node::new("Cube", "MeshInstance3D");
    let id = tree.add_child(root, mesh).unwrap();

    node3d::set_mesh_path(&mut tree, id, "res://models/cube.tres");
    assert_eq!(
        node3d::get_mesh_path(&tree, id),
        Some("res://models/cube.tres".to_string())
    );
}

// ===========================================================================
// 8. Light3D property helpers in scene tree
// ===========================================================================

#[test]
fn light3d_energy_in_scene_tree() {
    let _g = setup();
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let light = Node::new("Sun", "DirectionalLight3D");
    let id = tree.add_child(root, light).unwrap();

    node3d::set_light_energy(&mut tree, id, 2.5);
    assert!((node3d::get_light_energy(&tree, id) - 2.5).abs() < 1e-6);
}

#[test]
fn light3d_color_in_scene_tree() {
    let _g = setup();
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let light = Node::new("Lamp", "OmniLight3D");
    let id = tree.add_child(root, light).unwrap();

    let warm = Color::new(1.0, 0.9, 0.7, 1.0);
    node3d::set_light_color(&mut tree, id, warm);
    let got = node3d::get_light_color(&tree, id);
    assert!((got.r - 1.0).abs() < 1e-6);
    assert!((got.g - 0.9).abs() < 1e-6);
    assert!((got.b - 0.7).abs() < 1e-6);
}

// ===========================================================================
// 9. Global transform accumulation through parent chain
// ===========================================================================

#[test]
fn global_transform_parent_child_accumulation() {
    let _g = setup();
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let parent = Node::new("Parent", "Node3D");
    let parent_id = tree.add_child(root, parent).unwrap();
    node3d::set_position(&mut tree, parent_id, Vector3::new(10.0, 0.0, 0.0));

    let child = Node::new("Child", "Node3D");
    let child_id = tree.add_child(parent_id, child).unwrap();
    node3d::set_position(&mut tree, child_id, Vector3::new(0.0, 5.0, 0.0));

    let global_t = node3d::get_global_transform(&tree, child_id);
    // Child is at (0,5,0) under parent at (10,0,0) → global (10,5,0).
    assert!((global_t.origin.x - 10.0).abs() < 1e-4);
    assert!((global_t.origin.y - 5.0).abs() < 1e-4);
    assert!((global_t.origin.z - 0.0).abs() < 1e-4);
}

#[test]
fn global_transform_three_level_chain() {
    let _g = setup();
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let a = Node::new("A", "Node3D");
    let a_id = tree.add_child(root, a).unwrap();
    node3d::set_position(&mut tree, a_id, Vector3::new(1.0, 0.0, 0.0));

    let b = Node::new("B", "Node3D");
    let b_id = tree.add_child(a_id, b).unwrap();
    node3d::set_position(&mut tree, b_id, Vector3::new(0.0, 2.0, 0.0));

    let c = Node::new("C", "Node3D");
    let c_id = tree.add_child(b_id, c).unwrap();
    node3d::set_position(&mut tree, c_id, Vector3::new(0.0, 0.0, 3.0));

    let global_t = node3d::get_global_transform(&tree, c_id);
    assert!((global_t.origin.x - 1.0).abs() < 1e-4);
    assert!((global_t.origin.y - 2.0).abs() < 1e-4);
    assert!((global_t.origin.z - 3.0).abs() < 1e-4);
}

// ===========================================================================
// 10. Local transform composition
// ===========================================================================

#[test]
fn local_transform_from_position_rotation_scale() {
    let _g = setup();
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let node = Node::new("Cube", "Node3D");
    let id = tree.add_child(root, node).unwrap();

    node3d::set_position(&mut tree, id, Vector3::new(5.0, 0.0, 0.0));
    node3d::set_scale(&mut tree, id, Vector3::new(2.0, 2.0, 2.0));

    let local_t = node3d::get_local_transform(&tree, id);
    assert!((local_t.origin.x - 5.0).abs() < 1e-4);
    // Scale 2x should appear in the basis diagonal.
    assert!((local_t.basis.x.x - 2.0).abs() < 1e-4);
    assert!((local_t.basis.y.y - 2.0).abs() < 1e-4);
    assert!((local_t.basis.z.z - 2.0).abs() < 1e-4);
}

// ===========================================================================
// 11. 3D hierarchy in scene tree (mixed classes)
// ===========================================================================

#[test]
fn mixed_3d_hierarchy() {
    let _g = setup();
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let world = Node::new("World", "Node3D");
    let world_id = tree.add_child(root, world).unwrap();

    let cam = Node::new("Camera", "Camera3D");
    let cam_id = tree.add_child(world_id, cam).unwrap();

    let cube = Node::new("Cube", "MeshInstance3D");
    let cube_id = tree.add_child(world_id, cube).unwrap();

    let sun = Node::new("Sun", "DirectionalLight3D");
    let sun_id = tree.add_child(world_id, sun).unwrap();

    let floor = Node::new("Floor", "StaticBody3D");
    let floor_id = tree.add_child(world_id, floor).unwrap();

    let col = Node::new("CollisionShape", "CollisionShape3D");
    let col_id = tree.add_child(floor_id, col).unwrap();

    // Verify hierarchy.
    assert_eq!(tree.get_node(cam_id).unwrap().class_name(), "Camera3D");
    assert_eq!(
        tree.get_node(cube_id).unwrap().class_name(),
        "MeshInstance3D"
    );
    assert_eq!(
        tree.get_node(sun_id).unwrap().class_name(),
        "DirectionalLight3D"
    );
    assert_eq!(
        tree.get_node(floor_id).unwrap().class_name(),
        "StaticBody3D"
    );
    assert_eq!(
        tree.get_node(col_id).unwrap().class_name(),
        "CollisionShape3D"
    );

    // Verify path lookup.
    let found_cam = tree.get_node_by_path("/root/World/Camera");
    assert_eq!(found_cam, Some(cam_id));

    let found_col = tree.get_node_by_path("/root/World/Floor/CollisionShape");
    assert_eq!(found_col, Some(col_id));
}

#[test]
fn physics_body_nodes_in_tree() {
    let _g = setup();
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let rb = Node::new("Ball", "RigidBody3D");
    let rb_id = tree.add_child(root, rb).unwrap();

    let cb = Node::new("Player", "CharacterBody3D");
    let cb_id = tree.add_child(root, cb).unwrap();

    let area = Node::new("Trigger", "Area3D");
    let area_id = tree.add_child(root, area).unwrap();

    // Set physics-specific properties.
    tree.get_node_mut(rb_id)
        .unwrap()
        .set_property("mass", Variant::Float(5.0));
    assert_eq!(
        tree.get_node(rb_id).unwrap().get_property("mass"),
        Variant::Float(5.0)
    );

    tree.get_node_mut(cb_id)
        .unwrap()
        .set_property("velocity", Variant::Vector3(Vector3::new(0.0, -9.8, 0.0)));
    assert_eq!(
        tree.get_node(cb_id).unwrap().get_property("velocity"),
        Variant::Vector3(Vector3::new(0.0, -9.8, 0.0))
    );

    tree.get_node_mut(area_id)
        .unwrap()
        .set_property("monitoring", Variant::Bool(false));
    assert_eq!(
        tree.get_node(area_id).unwrap().get_property("monitoring"),
        Variant::Bool(false)
    );
}

// ===========================================================================
// 12. 3D fixture loading via PackedScene
// ===========================================================================

#[test]
fn load_minimal_3d_fixture() {
    let _g = setup();
    let fixture_path = format!(
        "{}/../fixtures/scenes/minimal_3d.tscn",
        env!("CARGO_MANIFEST_DIR")
    );
    let source =
        std::fs::read_to_string(&fixture_path).expect("should read minimal_3d.tscn fixture");
    let scene =
        gdscene::packed_scene::PackedScene::from_tscn(&source).expect("parse minimal_3d.tscn");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    gdscene::packed_scene::add_packed_scene_to_tree(&mut tree, root, &scene)
        .expect("add 3D scene to tree");

    // Verify all nodes were loaded.
    assert!(tree.get_node_by_path("/root/World").is_some());
    assert!(tree.get_node_by_path("/root/World/Camera").is_some());
    assert!(tree.get_node_by_path("/root/World/Cube").is_some());
    assert!(tree.get_node_by_path("/root/World/Sun").is_some());
    assert!(tree.get_node_by_path("/root/World/Floor").is_some());
    assert!(tree
        .get_node_by_path("/root/World/Floor/CollisionShape")
        .is_some());
}

#[test]
fn loaded_3d_fixture_has_correct_types() {
    let _g = setup();
    let fixture_path = format!(
        "{}/../fixtures/scenes/minimal_3d.tscn",
        env!("CARGO_MANIFEST_DIR")
    );
    let source =
        std::fs::read_to_string(&fixture_path).expect("should read minimal_3d.tscn fixture");
    let scene =
        gdscene::packed_scene::PackedScene::from_tscn(&source).expect("parse minimal_3d.tscn");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    gdscene::packed_scene::add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();

    let world_id = tree.get_node_by_path("/root/World").unwrap();
    let cam_id = tree.get_node_by_path("/root/World/Camera").unwrap();
    let cube_id = tree.get_node_by_path("/root/World/Cube").unwrap();
    let sun_id = tree.get_node_by_path("/root/World/Sun").unwrap();
    let floor_id = tree.get_node_by_path("/root/World/Floor").unwrap();

    assert_eq!(tree.get_node(world_id).unwrap().class_name(), "Node3D");
    assert_eq!(tree.get_node(cam_id).unwrap().class_name(), "Camera3D");
    assert_eq!(
        tree.get_node(cube_id).unwrap().class_name(),
        "MeshInstance3D"
    );
    assert_eq!(
        tree.get_node(sun_id).unwrap().class_name(),
        "DirectionalLight3D"
    );
    assert_eq!(
        tree.get_node(floor_id).unwrap().class_name(),
        "StaticBody3D"
    );
}

#[test]
fn loaded_3d_fixture_has_correct_node_count() {
    let _g = setup();
    let fixture_path = format!(
        "{}/../fixtures/scenes/minimal_3d.tscn",
        env!("CARGO_MANIFEST_DIR")
    );
    let source =
        std::fs::read_to_string(&fixture_path).expect("should read minimal_3d.tscn fixture");
    let scene =
        gdscene::packed_scene::PackedScene::from_tscn(&source).expect("parse minimal_3d.tscn");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    gdscene::packed_scene::add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();

    // root + World + Camera + Cube + Sun + Floor + CollisionShape = 7
    assert_eq!(tree.node_count(), 7);
}
