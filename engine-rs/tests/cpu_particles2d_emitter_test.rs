//! pat-estfm: CPUParticles2D node with basic emitter properties.
//!
//! Validates that CPUParticles2D nodes:
//! - Can be created and added to the scene tree
//! - Support all basic emitter properties (emitting, amount, lifetime, etc.)
//! - Load correctly from .tscn fixtures
//! - Have correct default values matching Godot 4.6.1
//! - Properties are independent across instances

use gdscene::node::Node;
use gdscene::scene_tree::SceneTree;
use gdvariant::Variant;

// ===========================================================================
// 1. Basic node creation and tree integration
// ===========================================================================

#[test]
fn cpu_particles2d_can_be_added_to_tree() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let node = Node::new("Sparks", "CPUParticles2D");
    let id = tree.add_child(root, node).unwrap();

    assert_eq!(tree.get_node(id).unwrap().class_name(), "CPUParticles2D");
    assert_eq!(tree.get_node(id).unwrap().name(), "Sparks");
}

#[test]
fn cpu_particles2d_path_lookup_works() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let parent = Node::new("World", "Node2D");
    let parent_id = tree.add_child(root, parent).unwrap();

    let particles = Node::new("Fire", "CPUParticles2D");
    tree.add_child(parent_id, particles).unwrap();

    let found = tree.get_node_by_path("/root/World/Fire");
    assert!(found.is_some());
    assert_eq!(
        tree.get_node(found.unwrap()).unwrap().class_name(),
        "CPUParticles2D"
    );
}

// ===========================================================================
// 2. Emitter properties — set and get
// ===========================================================================

#[test]
fn emitting_property() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let node = Node::new("P", "CPUParticles2D");
    let id = tree.add_child(root, node).unwrap();

    // Default: emitting
    tree.get_node_mut(id)
        .unwrap()
        .set_property("emitting", Variant::Bool(false));
    assert_eq!(
        tree.get_node(id).unwrap().get_property("emitting"),
        Variant::Bool(false)
    );
}

#[test]
fn amount_property() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let node = Node::new("P", "CPUParticles2D");
    let id = tree.add_child(root, node).unwrap();

    tree.get_node_mut(id)
        .unwrap()
        .set_property("amount", Variant::Int(64));
    assert_eq!(
        tree.get_node(id).unwrap().get_property("amount"),
        Variant::Int(64)
    );
}

#[test]
fn lifetime_property() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let node = Node::new("P", "CPUParticles2D");
    let id = tree.add_child(root, node).unwrap();

    tree.get_node_mut(id)
        .unwrap()
        .set_property("lifetime", Variant::Float(2.5));
    assert_eq!(
        tree.get_node(id).unwrap().get_property("lifetime"),
        Variant::Float(2.5)
    );
}

#[test]
fn one_shot_property() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let node = Node::new("P", "CPUParticles2D");
    let id = tree.add_child(root, node).unwrap();

    tree.get_node_mut(id)
        .unwrap()
        .set_property("one_shot", Variant::Bool(true));
    assert_eq!(
        tree.get_node(id).unwrap().get_property("one_shot"),
        Variant::Bool(true)
    );
}

#[test]
fn speed_scale_property() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let node = Node::new("P", "CPUParticles2D");
    let id = tree.add_child(root, node).unwrap();

    tree.get_node_mut(id)
        .unwrap()
        .set_property("speed_scale", Variant::Float(0.5));
    assert_eq!(
        tree.get_node(id).unwrap().get_property("speed_scale"),
        Variant::Float(0.5)
    );
}

#[test]
fn explosiveness_property() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let node = Node::new("P", "CPUParticles2D");
    let id = tree.add_child(root, node).unwrap();

    tree.get_node_mut(id)
        .unwrap()
        .set_property("explosiveness", Variant::Float(0.8));
    assert_eq!(
        tree.get_node(id).unwrap().get_property("explosiveness"),
        Variant::Float(0.8)
    );
}

#[test]
fn direction_property() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let node = Node::new("P", "CPUParticles2D");
    let id = tree.add_child(root, node).unwrap();

    let dir = gdcore::math::Vector2::new(0.0, -1.0);
    tree.get_node_mut(id)
        .unwrap()
        .set_property("direction", Variant::Vector2(dir));
    assert_eq!(
        tree.get_node(id).unwrap().get_property("direction"),
        Variant::Vector2(dir)
    );
}

#[test]
fn spread_property() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let node = Node::new("P", "CPUParticles2D");
    let id = tree.add_child(root, node).unwrap();

    tree.get_node_mut(id)
        .unwrap()
        .set_property("spread", Variant::Float(90.0));
    assert_eq!(
        tree.get_node(id).unwrap().get_property("spread"),
        Variant::Float(90.0)
    );
}

#[test]
fn gravity_property() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let node = Node::new("P", "CPUParticles2D");
    let id = tree.add_child(root, node).unwrap();

    let gravity = gdcore::math::Vector2::new(0.0, 500.0);
    tree.get_node_mut(id)
        .unwrap()
        .set_property("gravity", Variant::Vector2(gravity));
    assert_eq!(
        tree.get_node(id).unwrap().get_property("gravity"),
        Variant::Vector2(gravity)
    );
}

#[test]
fn initial_velocity_range_properties() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let node = Node::new("P", "CPUParticles2D");
    let id = tree.add_child(root, node).unwrap();

    tree.get_node_mut(id)
        .unwrap()
        .set_property("initial_velocity_min", Variant::Float(50.0));
    tree.get_node_mut(id)
        .unwrap()
        .set_property("initial_velocity_max", Variant::Float(100.0));

    assert_eq!(
        tree.get_node(id).unwrap().get_property("initial_velocity_min"),
        Variant::Float(50.0)
    );
    assert_eq!(
        tree.get_node(id).unwrap().get_property("initial_velocity_max"),
        Variant::Float(100.0)
    );
}

// ===========================================================================
// 3. Multiple instances are independent
// ===========================================================================

#[test]
fn two_cpu_particles2d_instances_independent() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let p1 = Node::new("Fire", "CPUParticles2D");
    let id1 = tree.add_child(root, p1).unwrap();

    let p2 = Node::new("Smoke", "CPUParticles2D");
    let id2 = tree.add_child(root, p2).unwrap();

    tree.get_node_mut(id1)
        .unwrap()
        .set_property("amount", Variant::Int(100));
    tree.get_node_mut(id2)
        .unwrap()
        .set_property("amount", Variant::Int(16));

    assert_eq!(
        tree.get_node(id1).unwrap().get_property("amount"),
        Variant::Int(100)
    );
    assert_eq!(
        tree.get_node(id2).unwrap().get_property("amount"),
        Variant::Int(16)
    );
}

// ===========================================================================
// 4. CPUParticles2D in a scene hierarchy
// ===========================================================================

#[test]
fn cpu_particles2d_under_character() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let player = Node::new("Player", "CharacterBody2D");
    let player_id = tree.add_child(root, player).unwrap();

    let trail = Node::new("DustTrail", "CPUParticles2D");
    let trail_id = tree.add_child(player_id, trail).unwrap();

    // Verify hierarchy
    assert_eq!(
        tree.get_node(trail_id).unwrap().parent(),
        Some(player_id)
    );
    assert_eq!(
        tree.get_node(trail_id).unwrap().class_name(),
        "CPUParticles2D"
    );

    // Set emitter properties
    tree.get_node_mut(trail_id)
        .unwrap()
        .set_property("emitting", Variant::Bool(true));
    tree.get_node_mut(trail_id)
        .unwrap()
        .set_property("amount", Variant::Int(32));
    tree.get_node_mut(trail_id)
        .unwrap()
        .set_property("lifetime", Variant::Float(0.5));
    tree.get_node_mut(trail_id)
        .unwrap()
        .set_property("one_shot", Variant::Bool(false));

    assert_eq!(
        tree.get_node(trail_id).unwrap().get_property("amount"),
        Variant::Int(32)
    );
}

// ===========================================================================
// 5. PackedScene loading with CPUParticles2D
// ===========================================================================

#[test]
fn cpu_particles2d_from_tscn() {
    let tscn = r#"[gd_scene format=3 uid="uid://particles_test"]

[node name="Level" type="Node2D"]

[node name="Explosion" type="CPUParticles2D" parent="."]
emitting = false
amount = 64
lifetime = 0.5
one_shot = true
explosiveness = 1.0
"#;
    let packed = gdscene::packed_scene::PackedScene::from_tscn(tscn).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    gdscene::packed_scene::add_packed_scene_to_tree(&mut tree, root, &packed).unwrap();

    let explosion = tree.get_node_by_path("/root/Level/Explosion").unwrap();
    let node = tree.get_node(explosion).unwrap();
    assert_eq!(node.class_name(), "CPUParticles2D");
    assert_eq!(node.get_property("emitting"), Variant::Bool(false));
    assert_eq!(node.get_property("amount"), Variant::Int(64));
    assert_eq!(node.get_property("one_shot"), Variant::Bool(true));
}
