//! pat-cde: Object/property reflection coverage.
//!
//! Tests that Node property iteration, get_property/set_property, has_property
//! work correctly for all common node types. Verifies get/set round-trips,
//! property iteration, and expected default properties from .tscn loading.

use gdcore::math::Vector2;
use gdscene::node::Node;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::SceneTree;
use gdvariant::Variant;

/// All common node class names we want to verify.
const COMMON_CLASSES: &[&str] = &[
    "Node",
    "Node2D",
    "Sprite2D",
    "AnimatedSprite2D",
    "Camera2D",
    "Control",
    "Label",
    "Area2D",
    "CollisionShape2D",
    "RigidBody2D",
    "StaticBody2D",
    "CharacterBody2D",
    "Timer",
    "AudioStreamPlayer",
];

// ═══════════════════════════════════════════════════════════════════════
// Node-level property get/set/has round-trips
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn cde_node_set_get_roundtrip_all_variant_types() {
    let mut node = Node::new("TestNode", "Node2D");

    let cases: Vec<(&str, Variant)> = vec![
        ("bool_prop", Variant::Bool(true)),
        ("int_prop", Variant::Int(42)),
        ("float_prop", Variant::Float(3.14)),
        ("string_prop", Variant::String("hello".into())),
        ("vec2_prop", Variant::Vector2(Vector2::new(1.0, 2.0))),
        ("nil_prop", Variant::Nil),
        (
            "array_prop",
            Variant::Array(vec![Variant::Int(1), Variant::Bool(false)]),
        ),
    ];

    for (key, value) in &cases {
        node.set_property(key, value.clone());
    }

    for (key, value) in &cases {
        assert!(
            node.has_property(key),
            "has_property should be true for '{key}'"
        );
        assert_eq!(
            node.get_property(key),
            *value,
            "get_property round-trip failed for '{key}'"
        );
    }
}

#[test]
fn cde_node_get_missing_property_returns_nil() {
    let node = Node::new("TestNode", "Node2D");
    assert_eq!(node.get_property("nonexistent"), Variant::Nil);
    assert!(!node.has_property("nonexistent"));
}

#[test]
fn cde_node_set_property_returns_previous_value() {
    let mut node = Node::new("TestNode", "Node2D");

    // First set returns Nil (no previous value)
    let prev = node.set_property("x", Variant::Int(10));
    assert_eq!(prev, Variant::Nil);

    // Second set returns previous value
    let prev = node.set_property("x", Variant::Int(20));
    assert_eq!(prev, Variant::Int(10));

    // Verify new value
    assert_eq!(node.get_property("x"), Variant::Int(20));
}

#[test]
fn cde_node_property_iteration() {
    let mut node = Node::new("TestNode", "Node2D");
    node.set_property("a", Variant::Int(1));
    node.set_property("b", Variant::Int(2));
    node.set_property("c", Variant::Int(3));

    let props: std::collections::HashMap<String, Variant> = node
        .properties()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();

    assert_eq!(props.len(), 3);
    assert_eq!(props["a"], Variant::Int(1));
    assert_eq!(props["b"], Variant::Int(2));
    assert_eq!(props["c"], Variant::Int(3));
}

#[test]
fn cde_node_overwrite_property_type() {
    let mut node = Node::new("TestNode", "Node2D");
    node.set_property("x", Variant::Int(10));
    assert_eq!(node.get_property("x"), Variant::Int(10));

    // Overwrite with a different type
    node.set_property("x", Variant::String("now a string".into()));
    assert_eq!(
        node.get_property("x"),
        Variant::String("now a string".into())
    );
}

// ═══════════════════════════════════════════════════════════════════════
// SceneTree: create_node for all common classes
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn cde_create_node_all_common_classes() {
    let mut tree = SceneTree::new();

    for class_name in COMMON_CLASSES {
        let nid = tree.create_node(class_name, &format!("test_{class_name}"));
        let node = tree.get_node(nid).expect("node should exist in arena");
        assert_eq!(node.class_name(), *class_name);
        assert_eq!(node.name(), format!("test_{class_name}"));
    }
}

#[test]
fn cde_create_node_set_get_via_tree() {
    let mut tree = SceneTree::new();
    let nid = tree.create_node("Sprite2D", "MySprite");

    // Set properties via mutable access
    {
        let node = tree.get_node_mut(nid).unwrap();
        node.set_property("position", Variant::Vector2(Vector2::new(100.0, 200.0)));
        node.set_property("flip_h", Variant::Bool(true));
        node.set_property("frame", Variant::Int(3));
    }

    // Read back via immutable access
    let node = tree.get_node(nid).unwrap();
    assert_eq!(
        node.get_property("position"),
        Variant::Vector2(Vector2::new(100.0, 200.0))
    );
    assert_eq!(node.get_property("flip_h"), Variant::Bool(true));
    assert_eq!(node.get_property("frame"), Variant::Int(3));
    assert!(node.has_property("position"));
    assert!(node.has_property("flip_h"));
    assert!(!node.has_property("nonexistent"));
}

#[test]
fn cde_add_child_with_node_object() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let mut child = Node::new("ChildNode", "Node2D");
    child.set_property("position", Variant::Vector2(Vector2::new(10.0, 20.0)));
    let child_id = tree
        .add_child(root, child)
        .expect("add_child should succeed");

    let node = tree.get_node(child_id).unwrap();
    assert_eq!(node.name(), "ChildNode");
    assert_eq!(node.class_name(), "Node2D");
    assert_eq!(
        node.get_property("position"),
        Variant::Vector2(Vector2::new(10.0, 20.0))
    );
}

// ═══════════════════════════════════════════════════════════════════════
// ObjectBase: property reflection at the gdobject level
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn cde_object_base_get_set_has_roundtrip() {
    let mut obj = gdobject::object::ObjectBase::new("TestClass");

    assert!(!obj.has_property("foo"));
    assert_eq!(obj.get_property("foo"), Variant::Nil);

    obj.set_property("foo", Variant::Int(99));
    assert!(obj.has_property("foo"));
    assert_eq!(obj.get_property("foo"), Variant::Int(99));
}

#[test]
fn cde_object_base_property_names() {
    let mut obj = gdobject::object::ObjectBase::new("TestClass");
    obj.set_property("alpha", Variant::Float(1.0));
    obj.set_property("beta", Variant::Bool(false));

    let mut names = obj.property_names();
    names.sort();
    assert_eq!(names, vec!["alpha", "beta"]);
}

// ═══════════════════════════════════════════════════════════════════════
// ClassDB: registration and property info
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn cde_classdb_register_and_query() {
    use gdobject::class_db::{ClassRegistration, PropertyInfo};

    // Register a test class with unique name to avoid global state conflicts
    let class_name = format!("CdeTestClass_{}", std::process::id());
    gdobject::class_db::register_class(
        ClassRegistration::new(&class_name)
            .parent("Node2D")
            .property(PropertyInfo::new("speed", Variant::Float(100.0)))
            .property(PropertyInfo::new("health", Variant::Int(100))),
    );

    assert!(gdobject::class_db::class_exists(&class_name));

    let info = gdobject::class_db::get_class_info(&class_name).unwrap();
    assert_eq!(info.class_name, class_name);
    assert_eq!(info.parent_class, "Node2D");
    assert_eq!(info.properties.len(), 2);
    assert_eq!(info.properties[0].name, "speed");
    assert_eq!(info.properties[0].default_value, Variant::Float(100.0));
    assert_eq!(info.properties[1].name, "health");
    assert_eq!(info.properties[1].default_value, Variant::Int(100));
}

#[test]
fn cde_classdb_inheritance_chain() {
    use gdobject::class_db::{ClassRegistration, PropertyInfo};

    let base = format!("CdeBase_{}", std::process::id());
    let child = format!("CdeChild_{}", std::process::id());

    gdobject::class_db::register_class(
        ClassRegistration::new(&base).property(PropertyInfo::new("base_prop", Variant::Int(1))),
    );
    gdobject::class_db::register_class(
        ClassRegistration::new(&child)
            .parent(&base)
            .property(PropertyInfo::new("child_prop", Variant::Int(2))),
    );

    let chain = gdobject::class_db::inheritance_chain(&child);
    assert_eq!(chain, vec![child.clone(), base.clone()]);
    assert!(gdobject::class_db::is_parent_class(&child, &base));
    assert!(!gdobject::class_db::is_parent_class(&base, &child));
}

// ═══════════════════════════════════════════════════════════════════════
// .tscn scene loading: verify default properties are applied
// ═══════════════════════════════════════════════════════════════════════

const SPACE_SHOOTER_TSCN: &str = include_str!("../fixtures/scenes/space_shooter.tscn");

#[test]
fn cde_tscn_loaded_nodes_have_properties() {
    let packed = PackedScene::from_tscn(SPACE_SHOOTER_TSCN).expect("parse scene");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &packed).expect("instance scene");

    let all = tree.all_nodes_in_tree_order();
    assert!(all.len() > 1, "scene should have nodes");

    // At least one node should have properties set from the .tscn
    let mut any_has_props = false;
    for &nid in &all {
        let node = tree.get_node(nid).unwrap();
        if node.properties().count() > 0 {
            any_has_props = true;
            // Verify all properties round-trip correctly
            for (key, value) in node.properties() {
                assert_eq!(
                    node.get_property(key),
                    *value,
                    "Property iteration vs get_property mismatch for '{}' on node '{}'",
                    key,
                    node.name()
                );
                assert!(
                    node.has_property(key),
                    "has_property false for iterated property '{}' on node '{}'",
                    key,
                    node.name()
                );
            }
        }
    }
    assert!(
        any_has_props,
        "at least one loaded node should have properties"
    );
}

#[test]
fn cde_tscn_node_classes_are_set() {
    let packed = PackedScene::from_tscn(SPACE_SHOOTER_TSCN).expect("parse scene");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &packed).expect("instance scene");

    let all = tree.all_nodes_in_tree_order();

    for &nid in &all {
        let node = tree.get_node(nid).unwrap();
        assert!(
            !node.class_name().is_empty(),
            "Node '{}' has empty class_name",
            node.name()
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Property set/get on physics body nodes (standalone Node objects)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn cde_rigidbody2d_property_roundtrip() {
    let mut node = Node::new("Ball", "RigidBody2D");
    node.set_property("mass", Variant::Float(2.5));
    node.set_property("gravity_scale", Variant::Float(0.5));
    node.set_property(
        "linear_velocity",
        Variant::Vector2(Vector2::new(10.0, -5.0)),
    );
    node.set_property("angular_velocity", Variant::Float(1.57));
    node.set_property("lock_rotation", Variant::Bool(true));

    assert_eq!(node.get_property("mass"), Variant::Float(2.5));
    assert_eq!(node.get_property("gravity_scale"), Variant::Float(0.5));
    assert_eq!(
        node.get_property("linear_velocity"),
        Variant::Vector2(Vector2::new(10.0, -5.0))
    );
    assert_eq!(node.get_property("angular_velocity"), Variant::Float(1.57));
    assert_eq!(node.get_property("lock_rotation"), Variant::Bool(true));
    assert_eq!(node.properties().count(), 5);
}

#[test]
fn cde_characterbody2d_property_roundtrip() {
    let mut node = Node::new("Player", "CharacterBody2D");
    node.set_property("velocity", Variant::Vector2(Vector2::new(100.0, 0.0)));
    node.set_property("floor_max_angle", Variant::Float(0.785));
    node.set_property("position", Variant::Vector2(Vector2::new(50.0, 100.0)));

    assert_eq!(
        node.get_property("velocity"),
        Variant::Vector2(Vector2::new(100.0, 0.0))
    );
    assert_eq!(node.get_property("floor_max_angle"), Variant::Float(0.785));
    assert_eq!(
        node.get_property("position"),
        Variant::Vector2(Vector2::new(50.0, 100.0))
    );
}

#[test]
fn cde_staticbody2d_property_roundtrip() {
    let mut node = Node::new("Wall", "StaticBody2D");
    node.set_property("collision_layer", Variant::Int(3));
    node.set_property("collision_mask", Variant::Int(5));
    node.set_property("constant_linear_velocity", Variant::Vector2(Vector2::ZERO));

    assert_eq!(node.get_property("collision_layer"), Variant::Int(3));
    assert_eq!(node.get_property("collision_mask"), Variant::Int(5));
    assert_eq!(
        node.get_property("constant_linear_velocity"),
        Variant::Vector2(Vector2::ZERO)
    );
}

#[test]
fn cde_area2d_property_roundtrip() {
    let mut node = Node::new("Hitbox", "Area2D");
    node.set_property("monitoring", Variant::Bool(true));
    node.set_property("monitorable", Variant::Bool(false));
    node.set_property("collision_layer", Variant::Int(2));

    assert_eq!(node.get_property("monitoring"), Variant::Bool(true));
    assert_eq!(node.get_property("monitorable"), Variant::Bool(false));
    assert_eq!(node.get_property("collision_layer"), Variant::Int(2));
}

#[test]
fn cde_sprite2d_property_roundtrip() {
    let mut node = Node::new("Icon", "Sprite2D");
    node.set_property("position", Variant::Vector2(Vector2::new(64.0, 64.0)));
    node.set_property("flip_h", Variant::Bool(true));
    node.set_property("flip_v", Variant::Bool(false));
    node.set_property("frame", Variant::Int(2));
    node.set_property("hframes", Variant::Int(4));
    node.set_property("vframes", Variant::Int(2));
    node.set_property("centered", Variant::Bool(false));

    assert_eq!(
        node.get_property("position"),
        Variant::Vector2(Vector2::new(64.0, 64.0))
    );
    assert_eq!(node.get_property("flip_h"), Variant::Bool(true));
    assert_eq!(node.get_property("flip_v"), Variant::Bool(false));
    assert_eq!(node.get_property("frame"), Variant::Int(2));
    assert_eq!(node.get_property("hframes"), Variant::Int(4));
    assert_eq!(node.get_property("vframes"), Variant::Int(2));
    assert_eq!(node.get_property("centered"), Variant::Bool(false));
    assert_eq!(node.properties().count(), 7);
}

#[test]
fn cde_camera2d_property_roundtrip() {
    let mut node = Node::new("MainCam", "Camera2D");
    node.set_property("zoom", Variant::Vector2(Vector2::new(2.0, 2.0)));
    node.set_property("offset", Variant::Vector2(Vector2::new(0.0, -50.0)));

    assert_eq!(
        node.get_property("zoom"),
        Variant::Vector2(Vector2::new(2.0, 2.0))
    );
    assert_eq!(
        node.get_property("offset"),
        Variant::Vector2(Vector2::new(0.0, -50.0))
    );
}

#[test]
fn cde_control_property_roundtrip() {
    let mut node = Node::new("Panel", "Control");
    node.set_property("anchor_left", Variant::Float(0.0));
    node.set_property("anchor_right", Variant::Float(1.0));
    node.set_property("anchor_top", Variant::Float(0.0));
    node.set_property("anchor_bottom", Variant::Float(1.0));
    node.set_property("offset_left", Variant::Float(10.0));
    node.set_property("offset_right", Variant::Float(-10.0));

    assert_eq!(node.get_property("anchor_left"), Variant::Float(0.0));
    assert_eq!(node.get_property("anchor_right"), Variant::Float(1.0));
    assert_eq!(node.get_property("offset_left"), Variant::Float(10.0));
    assert_eq!(node.get_property("offset_right"), Variant::Float(-10.0));
    assert_eq!(node.properties().count(), 6);
}

// ═══════════════════════════════════════════════════════════════════════
// Property iteration consistency: iterate == get for all entries
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn cde_property_iteration_matches_get_for_all_classes() {
    let test_data: Vec<(&str, Vec<(&str, Variant)>)> = vec![
        (
            "Node2D",
            vec![
                ("position", Variant::Vector2(Vector2::new(10.0, 20.0))),
                ("rotation", Variant::Float(1.57)),
                ("scale", Variant::Vector2(Vector2::new(2.0, 2.0))),
                ("visible", Variant::Bool(true)),
                ("z_index", Variant::Int(5)),
            ],
        ),
        (
            "RigidBody2D",
            vec![
                ("mass", Variant::Float(5.0)),
                ("linear_velocity", Variant::Vector2(Vector2::new(1.0, 2.0))),
            ],
        ),
        (
            "Control",
            vec![
                ("anchor_left", Variant::Float(0.0)),
                ("anchor_right", Variant::Float(1.0)),
                ("offset_left", Variant::Float(10.0)),
            ],
        ),
    ];

    for (class_name, props) in &test_data {
        let mut node = Node::new(&format!("test_{class_name}"), *class_name);
        for (key, value) in props {
            node.set_property(key, value.clone());
        }

        assert_eq!(
            node.properties().count(),
            props.len(),
            "property count mismatch for {class_name}"
        );

        for (key, value) in node.properties() {
            assert_eq!(
                node.get_property(key),
                *value,
                "iterate/get mismatch for '{key}' on {class_name}"
            );
            assert!(node.has_property(key));
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Edge cases
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn cde_set_nil_property_still_exists() {
    let mut node = Node::new("TestNode", "Node");
    node.set_property("maybe", Variant::Nil);
    // In Godot, setting Nil is a valid property value — it should still "exist"
    assert!(node.has_property("maybe"));
    assert_eq!(node.get_property("maybe"), Variant::Nil);
    assert_eq!(node.properties().count(), 1);
}

#[test]
fn cde_empty_string_key_property() {
    let mut node = Node::new("TestNode", "Node");
    node.set_property("", Variant::Int(1));
    assert!(node.has_property(""));
    assert_eq!(node.get_property(""), Variant::Int(1));
}

#[test]
fn cde_many_properties_stress() {
    let mut node = Node::new("StressNode", "Node2D");
    for i in 0..100 {
        node.set_property(&format!("prop_{i}"), Variant::Int(i));
    }

    assert_eq!(node.properties().count(), 100);

    for i in 0..100 {
        let key = format!("prop_{i}");
        assert!(node.has_property(&key));
        assert_eq!(node.get_property(&key), Variant::Int(i));
    }
}
