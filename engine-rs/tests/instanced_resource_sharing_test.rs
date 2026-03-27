//! pat-1zej, pat-f4ta, pat-wn6t, pat-rvb1, pat-bs8j: Instanced-scene resource-sharing regression after 4.6.1 repin.
//!
//! Validates that SubResource and ExtResource references on instanced nodes
//! are independent copies — mutating a resource-bearing property on one
//! instance must never bleed into another.

use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;
use gdvariant::Variant;

// ---------------------------------------------------------------------------
// Scene with SubResource-bearing properties
// ---------------------------------------------------------------------------

const SCENE_WITH_SUB_RESOURCES: &str = r#"[gd_scene format=3]

[sub_resource type="RectangleShape2D" id="shape_1"]
size = Vector2(64, 64)

[sub_resource type="CircleShape2D" id="shape_2"]
radius = 32.0

[node name="Actor" type="CharacterBody2D"]
position = Vector2(0, 0)

[node name="Collider" type="CollisionShape2D" parent="."]
shape = SubResource("shape_1")

[node name="HitBox" type="Area2D" parent="."]

[node name="HitShape" type="CollisionShape2D" parent="HitBox"]
shape = SubResource("shape_2")
"#;

const SCENE_WITH_EXT_RESOURCES: &str = r#"[gd_scene format=3]

[ext_resource type="Texture2D" uid="uid://shared_tex" path="res://icon.png" id="t1"]
[ext_resource type="Script" uid="uid://actor_script" path="res://actor.gd" id="s1"]

[node name="Mob" type="Node2D"]
script = ExtResource("s1")

[node name="Sprite" type="Sprite2D" parent="."]
texture = ExtResource("t1")
"#;

const SCENE_WITH_ARRAY_PROPERTY: &str = r#"[gd_scene format=3]

[ext_resource type="Texture2D" path="res://a.png" id="t1"]
[ext_resource type="Texture2D" path="res://b.png" id="t2"]

[node name="AnimRoot" type="AnimatedSprite2D"]
frames = [ExtResource("t1"), ExtResource("t2")]
tags = ["enemy", "flying"]
"#;

// ===========================================================================
// 1. SubResource reference strings are independent across instances
// ===========================================================================

#[test]
fn subresource_refs_independent_across_instances() {
    let scene = PackedScene::from_tscn(SCENE_WITH_SUB_RESOURCES).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let inst1 = add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();
    let inst2 = add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();

    let collider1 = tree.get_node_relative(inst1, "Collider").unwrap();
    let collider2 = tree.get_node_relative(inst2, "Collider").unwrap();

    // Both start with the same SubResource reference.
    assert_eq!(
        tree.get_node(collider1).unwrap().get_property("shape"),
        Variant::String("SubResource:shape_1".into()),
    );
    assert_eq!(
        tree.get_node(collider2).unwrap().get_property("shape"),
        Variant::String("SubResource:shape_1".into()),
    );

    // Overwrite instance-1's shape ref — simulates local_to_scene duplication.
    tree.get_node_mut(collider1)
        .unwrap()
        .set_property("shape", Variant::String("SubResource:shape_custom".into()));

    // Instance 2 must be unaffected.
    assert_eq!(
        tree.get_node(collider2).unwrap().get_property("shape"),
        Variant::String("SubResource:shape_1".into()),
        "mutating SubResource ref on instance 1 must not bleed into instance 2"
    );

    // Instance 1 has the new value.
    assert_eq!(
        tree.get_node(collider1).unwrap().get_property("shape"),
        Variant::String("SubResource:shape_custom".into()),
    );
}

// ===========================================================================
// 2. Nested SubResource refs are also independent
// ===========================================================================

#[test]
fn nested_subresource_refs_independent() {
    let scene = PackedScene::from_tscn(SCENE_WITH_SUB_RESOURCES).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let inst1 = add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();
    let inst2 = add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();

    let hit1 = tree.get_node_relative(inst1, "HitBox/HitShape").unwrap();
    let hit2 = tree.get_node_relative(inst2, "HitBox/HitShape").unwrap();

    // Both reference shape_2.
    assert_eq!(
        tree.get_node(hit1).unwrap().get_property("shape"),
        Variant::String("SubResource:shape_2".into()),
    );

    // Overwrite hit1's shape.
    tree.get_node_mut(hit1)
        .unwrap()
        .set_property("shape", Variant::Nil);

    // hit2 unaffected.
    assert_eq!(
        tree.get_node(hit2).unwrap().get_property("shape"),
        Variant::String("SubResource:shape_2".into()),
        "nested SubResource ref must be independent per instance"
    );
}

// ===========================================================================
// 3. ExtResource reference strings are independent across instances
// ===========================================================================

#[test]
fn ext_resource_refs_independent_across_instances() {
    let scene = PackedScene::from_tscn(SCENE_WITH_EXT_RESOURCES).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let inst1 = add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();
    let inst2 = add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();

    let sprite1 = tree.get_node_relative(inst1, "Sprite").unwrap();
    let sprite2 = tree.get_node_relative(inst2, "Sprite").unwrap();

    // Both reference the same ExtResource string.
    let expected = Variant::String("ExtResource(\"t1\")".into());
    assert_eq!(tree.get_node(sprite1).unwrap().get_property("texture"), expected);
    assert_eq!(tree.get_node(sprite2).unwrap().get_property("texture"), expected);

    // Swap texture on instance 1.
    tree.get_node_mut(sprite1)
        .unwrap()
        .set_property("texture", Variant::String("ExtResource(\"t_replaced\")".into()));

    // Instance 2 unchanged.
    assert_eq!(
        tree.get_node(sprite2).unwrap().get_property("texture"),
        expected,
        "mutating ExtResource ref on instance 1 must not bleed into instance 2"
    );
}

// ===========================================================================
// 4. Array-valued properties are deep-independent across instances
// ===========================================================================

#[test]
fn array_properties_independent_across_instances() {
    let scene = PackedScene::from_tscn(SCENE_WITH_ARRAY_PROPERTY).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let inst1 = add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();
    let inst2 = add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();

    // Both should have the tags array.
    let tags1 = tree.get_node(inst1).unwrap().get_property("tags");
    let tags2 = tree.get_node(inst2).unwrap().get_property("tags");
    match (&tags1, &tags2) {
        (Variant::Array(a), Variant::Array(b)) => {
            assert_eq!(a.len(), 2);
            assert_eq!(b.len(), 2);
        }
        _ => panic!("expected Array variants, got {:?} / {:?}", tags1, tags2),
    }

    // Overwrite tags on instance 1 with a different array.
    tree.get_node_mut(inst1)
        .unwrap()
        .set_property("tags", Variant::Array(vec![Variant::String("boss".into())]));

    // Instance 2 must still have the original.
    let tags2_after = tree.get_node(inst2).unwrap().get_property("tags");
    match &tags2_after {
        Variant::Array(a) => {
            assert_eq!(a.len(), 2, "instance 2 array length must be unchanged");
            assert_eq!(a[0], Variant::String("enemy".into()));
        }
        _ => panic!("expected Array variant after mutation, got {:?}", tags2_after),
    }
}

// ===========================================================================
// 5. Script path resolution independent across instances
// ===========================================================================

#[test]
fn script_path_independent_across_instances() {
    let scene = PackedScene::from_tscn(SCENE_WITH_EXT_RESOURCES).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let inst1 = add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();
    let inst2 = add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();

    // Both resolve _script_path from ExtResource.
    let expected_path = Variant::String("res://actor.gd".into());
    assert_eq!(
        tree.get_node(inst1).unwrap().get_property("_script_path"),
        expected_path,
    );
    assert_eq!(
        tree.get_node(inst2).unwrap().get_property("_script_path"),
        expected_path,
    );

    // Overwrite script path on instance 1.
    tree.get_node_mut(inst1)
        .unwrap()
        .set_property("_script_path", Variant::String("res://override.gd".into()));

    // Instance 2 unaffected.
    assert_eq!(
        tree.get_node(inst2).unwrap().get_property("_script_path"),
        expected_path,
        "overriding _script_path on one instance must not affect the other"
    );
}

// ===========================================================================
// 6. 4.6.1 compat: three instances — mutation on middle instance is isolated
// Godot 4.6.1 fixed resource sharing when duplicating instanced scenes.
// Verify that creating 3+ instances and mutating the middle one leaves
// both the first and the third unaffected.
// ===========================================================================

#[test]
fn three_instances_middle_mutation_isolated() {
    let scene = PackedScene::from_tscn(SCENE_WITH_SUB_RESOURCES).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let inst1 = add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();
    let inst2 = add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();
    let inst3 = add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();

    let collider1 = tree.get_node_relative(inst1, "Collider").unwrap();
    let collider2 = tree.get_node_relative(inst2, "Collider").unwrap();
    let collider3 = tree.get_node_relative(inst3, "Collider").unwrap();

    let original = Variant::String("SubResource:shape_1".into());

    // All three start with the same shape reference.
    assert_eq!(tree.get_node(collider1).unwrap().get_property("shape"), original);
    assert_eq!(tree.get_node(collider2).unwrap().get_property("shape"), original);
    assert_eq!(tree.get_node(collider3).unwrap().get_property("shape"), original);

    // Mutate only the middle instance.
    tree.get_node_mut(collider2)
        .unwrap()
        .set_property("shape", Variant::String("SubResource:shape_replaced".into()));

    // First and third must be unaffected (4.6.1 resource sharing fix).
    assert_eq!(
        tree.get_node(collider1).unwrap().get_property("shape"),
        original,
        "instance 1 must be unaffected by mutation on instance 2 (4.6.1 compat)"
    );
    assert_eq!(
        tree.get_node(collider3).unwrap().get_property("shape"),
        original,
        "instance 3 must be unaffected by mutation on instance 2 (4.6.1 compat)"
    );
    assert_eq!(
        tree.get_node(collider2).unwrap().get_property("shape"),
        Variant::String("SubResource:shape_replaced".into()),
    );
}

// ===========================================================================
// 7. 4.6.1 compat: property overrides on instanced scenes are independent
// When two instances override the same property to different values,
// each must retain its own override.
// ===========================================================================

#[test]
fn property_overrides_on_instances_are_independent() {
    let scene = PackedScene::from_tscn(SCENE_WITH_SUB_RESOURCES).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let inst1 = add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();
    let inst2 = add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();

    // Override position on both instances to different values.
    tree.get_node_mut(inst1)
        .unwrap()
        .set_property("position", Variant::String("Vector2(100, 200)".into()));
    tree.get_node_mut(inst2)
        .unwrap()
        .set_property("position", Variant::String("Vector2(300, 400)".into()));

    // Each instance must retain its own override.
    assert_eq!(
        tree.get_node(inst1).unwrap().get_property("position"),
        Variant::String("Vector2(100, 200)".into()),
    );
    assert_eq!(
        tree.get_node(inst2).unwrap().get_property("position"),
        Variant::String("Vector2(300, 400)".into()),
        "property overrides must be independent per instance (4.6.1 compat)"
    );
}

// ===========================================================================
// 8. 4.6.1 compat: nested sub-resources across instances are fully isolated
// Verify that deeply nested SubResource references (grandchild nodes)
// remain independent when the scene has multiple SubResource definitions.
// ===========================================================================

#[test]
fn deep_subresource_isolation_across_instances() {
    let scene = PackedScene::from_tscn(SCENE_WITH_SUB_RESOURCES).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let inst1 = add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();
    let inst2 = add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();

    // Get the deep nested HitBox/HitShape nodes.
    let hit1 = tree.get_node_relative(inst1, "HitBox/HitShape").unwrap();
    let hit2 = tree.get_node_relative(inst2, "HitBox/HitShape").unwrap();

    // Also get the direct child Collider nodes.
    let collider1 = tree.get_node_relative(inst1, "Collider").unwrap();
    let collider2 = tree.get_node_relative(inst2, "Collider").unwrap();

    // Mutate BOTH the collider shape and the hitshape on instance 1.
    tree.get_node_mut(collider1)
        .unwrap()
        .set_property("shape", Variant::Nil);
    tree.get_node_mut(hit1)
        .unwrap()
        .set_property("shape", Variant::Nil);

    // Instance 2's collider and hitshape must be completely unaffected.
    assert_eq!(
        tree.get_node(collider2).unwrap().get_property("shape"),
        Variant::String("SubResource:shape_1".into()),
        "collider SubResource must be isolated (4.6.1 compat)"
    );
    assert_eq!(
        tree.get_node(hit2).unwrap().get_property("shape"),
        Variant::String("SubResource:shape_2".into()),
        "hitshape SubResource must be isolated (4.6.1 compat)"
    );
}

// ===========================================================================
// 9. 4.6.1 compat: sub-scene instancing via instance_with_subscenes
// Two instances of a parent scene that embeds a sub-scene must have
// independent resource references on the sub-scene's nodes.
// ===========================================================================

const CHILD_SCENE: &str = r#"[gd_scene format=3]

[sub_resource type="RectangleShape2D" id="child_shape"]
size = Vector2(32, 32)

[node name="Enemy" type="CharacterBody2D"]
health = 100

[node name="Collider" type="CollisionShape2D" parent="."]
shape = SubResource("child_shape")
"#;

const PARENT_SCENE_WITH_INSTANCE: &str = r#"[gd_scene format=3]

[ext_resource type="PackedScene" path="res://enemy.tscn" id="enemy_scene"]

[node name="Level" type="Node2D"]

[node name="Enemy1" parent="." instance=ExtResource("enemy_scene")]
health = 200

[node name="Obstacle" type="StaticBody2D" parent="."]
"#;

#[test]
fn subscene_instancing_resource_refs_independent() {
    use gdscene::packed_scene::add_packed_scene_to_tree_with_subscenes;

    let parent_scene = PackedScene::from_tscn(PARENT_SCENE_WITH_INSTANCE).unwrap();

    let resolve = |path: &str| -> Option<PackedScene> {
        if path == "res://enemy.tscn" {
            Some(PackedScene::from_tscn(CHILD_SCENE).unwrap())
        } else {
            None
        }
    };

    let mut tree = SceneTree::new();
    let root = tree.root_id();

    // Instance the parent scene twice — each embeds the child sub-scene.
    let inst1 = add_packed_scene_to_tree_with_subscenes(&mut tree, root, &parent_scene, &resolve)
        .unwrap();
    let inst2 = add_packed_scene_to_tree_with_subscenes(&mut tree, root, &parent_scene, &resolve)
        .unwrap();

    // Locate the sub-scene collider in each instance.
    let collider1 = tree.get_node_relative(inst1, "Enemy1/Collider").unwrap();
    let collider2 = tree.get_node_relative(inst2, "Enemy1/Collider").unwrap();

    let original = Variant::String("SubResource:child_shape".into());
    assert_eq!(tree.get_node(collider1).unwrap().get_property("shape"), original);
    assert_eq!(tree.get_node(collider2).unwrap().get_property("shape"), original);

    // Mutate sub-scene resource on instance 1.
    tree.get_node_mut(collider1)
        .unwrap()
        .set_property("shape", Variant::String("SubResource:custom_shape".into()));

    // Instance 2's sub-scene resource must be unaffected (4.6.1 sharing fix).
    assert_eq!(
        tree.get_node(collider2).unwrap().get_property("shape"),
        original,
        "sub-scene resource refs must be independent across parent instances (4.6.1 compat)"
    );

    // Verify property override from parent scene was applied independently.
    let enemy1_inst1 = tree.get_node_relative(inst1, "Enemy1").unwrap();
    let enemy1_inst2 = tree.get_node_relative(inst2, "Enemy1").unwrap();
    assert_eq!(
        tree.get_node(enemy1_inst1).unwrap().get_property("health"),
        Variant::Int(200),
        "parent override on sub-scene root should be applied"
    );
    assert_eq!(
        tree.get_node(enemy1_inst2).unwrap().get_property("health"),
        Variant::Int(200),
        "parent override should be applied independently to second instance"
    );

    // Mutate health on instance 1 — instance 2 must be unaffected.
    tree.get_node_mut(enemy1_inst1)
        .unwrap()
        .set_property("health", Variant::Int(50));
    assert_eq!(
        tree.get_node(enemy1_inst2).unwrap().get_property("health"),
        Variant::Int(200),
        "mutating overridden property on one instance must not bleed into another (4.6.1 compat)"
    );
}

// ===========================================================================
// 10. 4.6.1 compat: group membership independent across instances
// Nodes added with pre-existing groups during instancing must each appear
// independently in the tree's group index.
// ===========================================================================

const SCENE_WITH_GROUPS: &str = r#"[gd_scene format=3]

[node name="Player" type="CharacterBody2D" groups=["players", "persistent"]]

[node name="Weapon" type="Node2D" parent="." groups=["weapons"]]
"#;

#[test]
fn group_membership_independent_across_instances() {
    let scene = PackedScene::from_tscn(SCENE_WITH_GROUPS).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let inst1 = add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();
    let inst2 = add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();

    // Both instances should appear in the "players" group.
    let players = tree.get_nodes_in_group("players");
    assert_eq!(players.len(), 2, "both instances should be in 'players' group");

    // Both weapon children should appear in "weapons".
    let weapons = tree.get_nodes_in_group("weapons");
    assert_eq!(weapons.len(), 2, "both weapon instances should be in 'weapons' group");

    // Removing inst1 from the group should leave inst2 in it.
    tree.remove_from_group(inst1, "players").unwrap();
    let players_after = tree.get_nodes_in_group("players");
    assert_eq!(players_after.len(), 1, "only instance 2 should remain in 'players' after removal");
    assert!(
        players_after.contains(&inst2),
        "instance 2 must still be in 'players' group"
    );
}

// ===========================================================================
// 11. 4.6.1 compat: many instances stress — resource isolation at scale
// Verify that even with 20 instances, mutating one doesn't affect the rest.
// ===========================================================================

#[test]
fn many_instances_resource_isolation_stress() {
    let scene = PackedScene::from_tscn(SCENE_WITH_SUB_RESOURCES).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let instance_count = 20;
    let mut instances = Vec::new();
    for _ in 0..instance_count {
        let id = add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();
        instances.push(id);
    }

    // Collect collider IDs.
    let colliders: Vec<_> = instances
        .iter()
        .map(|inst| tree.get_node_relative(*inst, "Collider").unwrap())
        .collect();

    let original = Variant::String("SubResource:shape_1".into());

    // Mutate every other instance's collider shape.
    for (i, &cid) in colliders.iter().enumerate() {
        if i % 2 == 0 {
            tree.get_node_mut(cid)
                .unwrap()
                .set_property("shape", Variant::String(format!("SubResource:custom_{i}").into()));
        }
    }

    // Verify odd-indexed instances are untouched.
    for (i, &cid) in colliders.iter().enumerate() {
        if i % 2 == 1 {
            assert_eq!(
                tree.get_node(cid).unwrap().get_property("shape"),
                original,
                "instance {i} must retain original shape after even instances mutated"
            );
        }
    }

    // Verify even-indexed instances have their custom values.
    for (i, &cid) in colliders.iter().enumerate() {
        if i % 2 == 0 {
            assert_eq!(
                tree.get_node(cid).unwrap().get_property("shape"),
                Variant::String(format!("SubResource:custom_{i}").into()),
                "instance {i} must have its custom shape"
            );
        }
    }
}

// ===========================================================================
// 12. 4.6.1 repin: template immutability — mutating an instance must never
// affect subsequently created instances from the same PackedScene.
// ===========================================================================

#[test]
fn template_immutable_after_instance_mutation() {
    let scene = PackedScene::from_tscn(SCENE_WITH_SUB_RESOURCES).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    // Create first instance and heavily mutate it.
    let inst1 = add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();
    let collider1 = tree.get_node_relative(inst1, "Collider").unwrap();
    tree.get_node_mut(collider1)
        .unwrap()
        .set_property("shape", Variant::String("SubResource:CORRUPTED".into()));
    tree.get_node_mut(inst1)
        .unwrap()
        .set_property("position", Variant::String("Vector2(999, 999)".into()));

    // Create second instance AFTER the first was mutated.
    let inst2 = add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();
    let collider2 = tree.get_node_relative(inst2, "Collider").unwrap();

    // Second instance must have pristine template values, not the mutations.
    assert_eq!(
        tree.get_node(collider2).unwrap().get_property("shape"),
        Variant::String("SubResource:shape_1".into()),
        "instance created after mutation of prior instance must use pristine template (4.6.1 repin)"
    );
    // Position is set to (0,0) in the template, not the mutated (999,999).
    let pos2 = tree.get_node(inst2).unwrap().get_property("position");
    assert_ne!(
        pos2,
        Variant::String("Vector2(999, 999)".into()),
        "template position must not inherit from mutated prior instance"
    );
}

// ===========================================================================
// 13. 4.6.1 repin: Variant type preservation across instanced scenes.
// All property types (int, float, bool, Vector2, String, Array) must
// survive instancing with correct type tags, not just correct values.
// ===========================================================================

const SCENE_WITH_VARIED_TYPES: &str = r#"[gd_scene format=3]

[node name="Entity" type="Node2D"]
position = Vector2(10, 20)
health = 100
speed = 3.5
is_alive = true
name_tag = "hero"
tags = ["player", "team_a"]
"#;

#[test]
fn variant_types_preserved_across_instances() {
    let scene = PackedScene::from_tscn(SCENE_WITH_VARIED_TYPES).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let inst1 = add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();
    let inst2 = add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();

    // Verify each instance has the correct Variant types, not just values.
    for (inst, label) in [(inst1, "inst1"), (inst2, "inst2")] {
        let node = tree.get_node(inst).unwrap();

        match node.get_property("health") {
            Variant::Int(v) => assert_eq!(v, 100, "[{label}] health value"),
            other => panic!("[{label}] health must be Int, got {:?}", other),
        }

        match node.get_property("speed") {
            Variant::Float(v) => assert!((v - 3.5).abs() < 0.01, "[{label}] speed value"),
            other => panic!("[{label}] speed must be Float, got {:?}", other),
        }

        match node.get_property("is_alive") {
            Variant::Bool(v) => assert!(v, "[{label}] is_alive value"),
            other => panic!("[{label}] is_alive must be Bool, got {:?}", other),
        }

        match node.get_property("name_tag") {
            Variant::String(ref s) => assert_eq!(s, "hero", "[{label}] name_tag value"),
            other => panic!("[{label}] name_tag must be String, got {:?}", other),
        }

        match node.get_property("tags") {
            Variant::Array(ref a) => {
                assert_eq!(a.len(), 2, "[{label}] tags array length");
                assert_eq!(a[0], Variant::String("player".into()));
                assert_eq!(a[1], Variant::String("team_a".into()));
            }
            other => panic!("[{label}] tags must be Array, got {:?}", other),
        }
    }
}

// ===========================================================================
// 14. 4.6.1 repin: mutating varied types on one instance doesn't affect another.
// ===========================================================================

#[test]
fn varied_type_mutation_isolated_across_instances() {
    let scene = PackedScene::from_tscn(SCENE_WITH_VARIED_TYPES).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let inst1 = add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();
    let inst2 = add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();

    // Mutate every property on instance 1.
    tree.get_node_mut(inst1).unwrap().set_property("health", Variant::Int(0));
    tree.get_node_mut(inst1).unwrap().set_property("speed", Variant::Float(99.9));
    tree.get_node_mut(inst1).unwrap().set_property("is_alive", Variant::Bool(false));
    tree.get_node_mut(inst1).unwrap().set_property("name_tag", Variant::String("dead".into()));
    tree.get_node_mut(inst1).unwrap().set_property("tags", Variant::Array(vec![]));

    // Instance 2 must retain all original values.
    let node2 = tree.get_node(inst2).unwrap();
    assert_eq!(node2.get_property("health"), Variant::Int(100), "health isolated");
    assert!(matches!(node2.get_property("speed"), Variant::Float(v) if (v - 3.5).abs() < 0.01), "speed isolated");
    assert_eq!(node2.get_property("is_alive"), Variant::Bool(true), "is_alive isolated");
    assert_eq!(node2.get_property("name_tag"), Variant::String("hero".into()), "name_tag isolated");
    match node2.get_property("tags") {
        Variant::Array(ref a) => assert_eq!(a.len(), 2, "tags array isolated"),
        other => panic!("tags must be Array, got {:?}", other),
    }
}

// ===========================================================================
// 15. 4.6.1 repin: re-instancing after removal — fresh instance from same
// template must be pristine even after prior instances were added and removed.
// ===========================================================================

#[test]
fn reinstancing_after_removal_uses_clean_template() {
    let scene = PackedScene::from_tscn(SCENE_WITH_SUB_RESOURCES).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    // Create and mutate first instance.
    let inst1 = add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();
    let collider1 = tree.get_node_relative(inst1, "Collider").unwrap();
    tree.get_node_mut(collider1)
        .unwrap()
        .set_property("shape", Variant::String("SubResource:MUTATED".into()));

    // Remove the first instance from the tree.
    tree.remove_node(inst1).unwrap();

    // Create a new instance — must be pristine.
    let inst2 = add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();
    let collider2 = tree.get_node_relative(inst2, "Collider").unwrap();

    assert_eq!(
        tree.get_node(collider2).unwrap().get_property("shape"),
        Variant::String("SubResource:shape_1".into()),
        "instance after removal+re-creation must use clean template (4.6.1 repin)"
    );
}

// ===========================================================================
// 16. 4.6.1 repin: ownership boundary — sub-scene nodes have correct owner
// after instancing. Each instance's sub-scene root should be owned by the
// parent scene root, not shared across instances.
// ===========================================================================

#[test]
fn ownership_boundaries_correct_per_instance() {
    use gdscene::packed_scene::add_packed_scene_to_tree_with_subscenes;

    let parent_scene = PackedScene::from_tscn(PARENT_SCENE_WITH_INSTANCE).unwrap();

    let resolve = |path: &str| -> Option<PackedScene> {
        if path == "res://enemy.tscn" {
            Some(PackedScene::from_tscn(CHILD_SCENE).unwrap())
        } else {
            None
        }
    };

    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let inst1 = add_packed_scene_to_tree_with_subscenes(&mut tree, root, &parent_scene, &resolve)
        .unwrap();
    let inst2 = add_packed_scene_to_tree_with_subscenes(&mut tree, root, &parent_scene, &resolve)
        .unwrap();

    // Sub-scene roots should be owned by their respective parent instance roots.
    let enemy1 = tree.get_node_relative(inst1, "Enemy1").unwrap();
    let enemy2 = tree.get_node_relative(inst2, "Enemy1").unwrap();

    let owner1 = tree.get_node(enemy1).unwrap().owner();
    let owner2 = tree.get_node(enemy2).unwrap().owner();

    // Each sub-scene root's owner should be its parent scene root, not the other instance.
    assert_eq!(owner1, Some(inst1), "enemy in inst1 must be owned by inst1");
    assert_eq!(owner2, Some(inst2), "enemy in inst2 must be owned by inst2");
    assert_ne!(owner1, owner2, "owners must differ across instances");
}

// ===========================================================================
// 17. 4.6.1 repin: concurrent property writes to different instances
// Simulates rapid alternating writes to two instances — ensures no
// cross-contamination even under interleaved mutation patterns.
// ===========================================================================

#[test]
fn interleaved_writes_across_instances_isolated() {
    let scene = PackedScene::from_tscn(SCENE_WITH_SUB_RESOURCES).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let inst1 = add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();
    let inst2 = add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();

    let col1 = tree.get_node_relative(inst1, "Collider").unwrap();
    let col2 = tree.get_node_relative(inst2, "Collider").unwrap();

    // Interleave writes: inst1, inst2, inst1, inst2 — different values each time.
    for i in 0..10 {
        tree.get_node_mut(col1)
            .unwrap()
            .set_property("shape", Variant::String(format!("shape_a_{i}").into()));
        tree.get_node_mut(col2)
            .unwrap()
            .set_property("shape", Variant::String(format!("shape_b_{i}").into()));
    }

    // After all interleaved writes, each must have its own final value.
    assert_eq!(
        tree.get_node(col1).unwrap().get_property("shape"),
        Variant::String("shape_a_9".into()),
        "inst1 must have its own final value after interleaved writes"
    );
    assert_eq!(
        tree.get_node(col2).unwrap().get_property("shape"),
        Variant::String("shape_b_9".into()),
        "inst2 must have its own final value after interleaved writes"
    );
}

// ===========================================================================
// 18. 4.6.1 repin: sub-scene with multiple ext-resources — each instance
// must independently track all ext-resource references.
// ===========================================================================

const MULTI_EXT_CHILD: &str = r#"[gd_scene format=3]

[ext_resource type="Texture2D" path="res://sprite.png" id="tex"]
[ext_resource type="Script" path="res://enemy_ai.gd" id="ai"]

[sub_resource type="CircleShape2D" id="hitbox"]
radius = 16.0

[node name="Enemy" type="CharacterBody2D"]
script = ExtResource("ai")

[node name="Sprite" type="Sprite2D" parent="."]
texture = ExtResource("tex")

[node name="Hitbox" type="CollisionShape2D" parent="."]
shape = SubResource("hitbox")
"#;

const MULTI_EXT_PARENT: &str = r#"[gd_scene format=3]

[ext_resource type="PackedScene" path="res://multi_enemy.tscn" id="e1"]

[node name="Wave" type="Node2D"]

[node name="EnemyA" parent="." instance=ExtResource("e1")]

[node name="EnemyB" parent="." instance=ExtResource("e1")]
"#;

#[test]
fn multi_ext_resource_subscene_instances_independent() {
    use gdscene::packed_scene::add_packed_scene_to_tree_with_subscenes;

    let parent_scene = PackedScene::from_tscn(MULTI_EXT_PARENT).unwrap();

    let resolve = |path: &str| -> Option<PackedScene> {
        if path == "res://multi_enemy.tscn" {
            Some(PackedScene::from_tscn(MULTI_EXT_CHILD).unwrap())
        } else {
            None
        }
    };

    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let wave = add_packed_scene_to_tree_with_subscenes(&mut tree, root, &parent_scene, &resolve)
        .unwrap();

    let sprite_a = tree.get_node_relative(wave, "EnemyA/Sprite").unwrap();
    let sprite_b = tree.get_node_relative(wave, "EnemyB/Sprite").unwrap();
    let hitbox_a = tree.get_node_relative(wave, "EnemyA/Hitbox").unwrap();
    let hitbox_b = tree.get_node_relative(wave, "EnemyB/Hitbox").unwrap();

    // Both sprites start with the same texture ref.
    let tex_ref = Variant::String("ExtResource(\"tex\")".into());
    assert_eq!(tree.get_node(sprite_a).unwrap().get_property("texture"), tex_ref);
    assert_eq!(tree.get_node(sprite_b).unwrap().get_property("texture"), tex_ref);

    // Mutate EnemyA's sprite texture and hitbox shape.
    tree.get_node_mut(sprite_a)
        .unwrap()
        .set_property("texture", Variant::String("custom_tex".into()));
    tree.get_node_mut(hitbox_a)
        .unwrap()
        .set_property("shape", Variant::Nil);

    // EnemyB must be entirely unaffected.
    assert_eq!(
        tree.get_node(sprite_b).unwrap().get_property("texture"),
        tex_ref,
        "EnemyB sprite must be unaffected by EnemyA mutation (4.6.1 repin)"
    );
    assert_eq!(
        tree.get_node(hitbox_b).unwrap().get_property("shape"),
        Variant::String("SubResource:hitbox".into()),
        "EnemyB hitbox must be unaffected by EnemyA mutation (4.6.1 repin)"
    );
}

// ===========================================================================
// pat-rvb1: Additional post-repin validation
// ===========================================================================

// ---------------------------------------------------------------------------
// 19. pat-rvb1: Transform3D properties are independent across instances
// After repin, scenes may store `transform = Transform3D(...)` which must
// be deep-copied per instance.
// ---------------------------------------------------------------------------

const SCENE_WITH_TRANSFORM3D: &str = r#"[gd_scene format=3]

[node name="Object3D" type="Node3D"]
transform = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 10, 20, 30)

[node name="Child" type="MeshInstance3D" parent="."]
transform = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 1, 2, 3)
"#;

#[test]
fn rvb1_transform3d_properties_independent_across_instances() {
    use gdcore::math::Vector3;

    let scene = PackedScene::from_tscn(SCENE_WITH_TRANSFORM3D).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let inst1 = add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();
    let inst2 = add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();

    // Both should start with the same transform.
    let t1 = tree.get_node(inst1).unwrap().get_property("transform");
    let t2 = tree.get_node(inst2).unwrap().get_property("transform");
    match (&t1, &t2) {
        (Variant::Transform3D(a), Variant::Transform3D(b)) => {
            assert_eq!(a.origin, b.origin, "both instances should start with same origin");
            assert_eq!(a.origin, Vector3::new(10.0, 20.0, 30.0));
        }
        _ => panic!("expected Transform3D variants, got {:?} / {:?}", t1, t2),
    }

    // Mutate instance 1's transform.
    tree.get_node_mut(inst1)
        .unwrap()
        .set_property("transform", Variant::Transform3D(gdcore::math3d::Transform3D {
            basis: gdcore::math3d::Basis::IDENTITY,
            origin: Vector3::new(99.0, 99.0, 99.0),
        }));

    // Instance 2 must be unaffected.
    match tree.get_node(inst2).unwrap().get_property("transform") {
        Variant::Transform3D(t) => {
            assert_eq!(
                t.origin,
                Vector3::new(10.0, 20.0, 30.0),
                "instance 2 transform must be unaffected by instance 1 mutation (4.6.1 repin)"
            );
        }
        other => panic!("expected Transform3D, got {:?}", other),
    }
}

// ---------------------------------------------------------------------------
// 20. pat-rvb1: Child transform properties isolated across instances
// ---------------------------------------------------------------------------

#[test]
fn rvb1_child_transform3d_isolated_across_instances() {
    use gdcore::math::Vector3;

    let scene = PackedScene::from_tscn(SCENE_WITH_TRANSFORM3D).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let inst1 = add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();
    let inst2 = add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();

    let child1 = tree.get_node_relative(inst1, "Child").unwrap();
    let child2 = tree.get_node_relative(inst2, "Child").unwrap();

    // Verify children start with the same transform.
    match tree.get_node(child1).unwrap().get_property("transform") {
        Variant::Transform3D(t) => {
            assert_eq!(t.origin, Vector3::new(1.0, 2.0, 3.0));
        }
        other => panic!("expected Transform3D on child, got {:?}", other),
    }

    // Mutate child1's transform.
    tree.get_node_mut(child1)
        .unwrap()
        .set_property("transform", Variant::Transform3D(gdcore::math3d::Transform3D {
            basis: gdcore::math3d::Basis::IDENTITY,
            origin: Vector3::ZERO,
        }));

    // Child2 must retain its original transform.
    match tree.get_node(child2).unwrap().get_property("transform") {
        Variant::Transform3D(t) => {
            assert_eq!(
                t.origin,
                Vector3::new(1.0, 2.0, 3.0),
                "child2 transform must be unaffected by child1 mutation"
            );
        }
        other => panic!("expected Transform3D on child2, got {:?}", other),
    }
}

// ---------------------------------------------------------------------------
// 21. pat-rvb1: Nested subscene with Transform3D — resource sharing isolated
// ---------------------------------------------------------------------------

const CHILD_3D_SCENE: &str = r#"[gd_scene format=3]

[node name="Turret" type="Node3D"]
transform = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 5, 0)
damage = 25

[node name="Barrel" type="MeshInstance3D" parent="."]
transform = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 2)
"#;

const PARENT_3D_SCENE: &str = r#"[gd_scene format=3]

[ext_resource type="PackedScene" path="res://turret.tscn" id="t"]

[node name="Base" type="Node3D"]

[node name="TurretLeft" parent="." instance=ExtResource("t")]
damage = 50

[node name="TurretRight" parent="." instance=ExtResource("t")]
damage = 75
"#;

#[test]
fn rvb1_nested_subscene_3d_resource_sharing_isolated() {
    use gdscene::packed_scene::add_packed_scene_to_tree_with_subscenes;
    use gdcore::math::Vector3;

    let parent_scene = PackedScene::from_tscn(PARENT_3D_SCENE).unwrap();

    let resolve = |path: &str| -> Option<PackedScene> {
        if path == "res://turret.tscn" {
            Some(PackedScene::from_tscn(CHILD_3D_SCENE).unwrap())
        } else {
            None
        }
    };

    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let base = add_packed_scene_to_tree_with_subscenes(&mut tree, root, &parent_scene, &resolve)
        .unwrap();

    let turret_left = tree.get_node_relative(base, "TurretLeft").unwrap();
    let turret_right = tree.get_node_relative(base, "TurretRight").unwrap();
    let barrel_left = tree.get_node_relative(turret_left, "Barrel").unwrap();
    let barrel_right = tree.get_node_relative(turret_right, "Barrel").unwrap();

    // Property overrides are applied independently.
    assert_eq!(
        tree.get_node(turret_left).unwrap().get_property("damage"),
        Variant::Int(50),
        "TurretLeft damage override"
    );
    assert_eq!(
        tree.get_node(turret_right).unwrap().get_property("damage"),
        Variant::Int(75),
        "TurretRight damage override"
    );

    // Barrels should both have the same initial transform from the sub-scene.
    match tree.get_node(barrel_left).unwrap().get_property("transform") {
        Variant::Transform3D(t) => {
            assert_eq!(t.origin, Vector3::new(0.0, 0.0, 2.0), "barrel_left origin");
        }
        other => panic!("expected Transform3D on barrel_left, got {:?}", other),
    }

    // Mutate barrel_left transform and turret_left damage.
    tree.get_node_mut(barrel_left)
        .unwrap()
        .set_property("transform", Variant::Transform3D(gdcore::math3d::Transform3D {
            basis: gdcore::math3d::Basis::IDENTITY,
            origin: Vector3::new(0.0, 0.0, 10.0),
        }));
    tree.get_node_mut(turret_left)
        .unwrap()
        .set_property("damage", Variant::Int(0));

    // TurretRight and its barrel must be completely unaffected.
    assert_eq!(
        tree.get_node(turret_right).unwrap().get_property("damage"),
        Variant::Int(75),
        "TurretRight damage must be unaffected (4.6.1 repin)"
    );
    match tree.get_node(barrel_right).unwrap().get_property("transform") {
        Variant::Transform3D(t) => {
            assert_eq!(
                t.origin,
                Vector3::new(0.0, 0.0, 2.0),
                "barrel_right transform must be unaffected (4.6.1 repin)"
            );
        }
        other => panic!("expected Transform3D on barrel_right, got {:?}", other),
    }

    // Ownership boundaries: turrets owned by base, barrels owned by turrets.
    assert_eq!(tree.get_node(turret_left).unwrap().owner(), Some(base));
    assert_eq!(tree.get_node(turret_right).unwrap().owner(), Some(base));
    assert_eq!(tree.get_node(barrel_left).unwrap().owner(), Some(turret_left));
    assert_eq!(tree.get_node(barrel_right).unwrap().owner(), Some(turret_right));
}

// ===========================================================================
// 22. pat-bs8j: Signal connections on instanced scenes are independent (4.6.1)
// ===========================================================================

const SCENE_WITH_SIGNAL: &str = r#"[gd_scene format=3]

[node name="Button" type="Control"]
pressed_count = 0

[node name="Label" type="Control" parent="."]
text = "hello"
"#;

/// Connecting a signal on one instance must not affect the signal store
/// on another instance of the same packed scene.
#[test]
fn signal_connections_independent_across_instances() {
    use gdscene::SignalConnection as Connection;

    let scene = PackedScene::from_tscn(SCENE_WITH_SIGNAL).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let inst1 = add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();
    let inst2 = add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();

    // Connect a signal on instance 1.
    let label1 = tree.get_node_relative(inst1, "Label").unwrap();
    tree.connect_signal(
        inst1,
        "pressed",
        Connection::with_callback(label1.object_id(), "on_pressed", |_| Variant::Nil),
    );

    // Instance 1 should have the signal connection.
    let store1 = tree.signal_store(inst1);
    assert!(
        store1.is_some_and(|s| s.get_signal("pressed").is_some_and(|sig| sig.connection_count() == 1)),
        "instance 1 should have 1 connection on 'pressed'"
    );

    // Instance 2 must have NO connections on 'pressed'.
    let store2 = tree.signal_store(inst2);
    let conn_count = store2
        .and_then(|s| s.get_signal("pressed"))
        .map(|sig| sig.connection_count())
        .unwrap_or(0);
    assert_eq!(
        conn_count, 0,
        "instance 2 must have no signal connections (4.6.1 repin)"
    );
}

// ===========================================================================
// 23. pat-bs8j: Unique names independent across instances (4.6.1)
// ===========================================================================

const SCENE_WITH_UNIQUE_NAMES: &str = r#"[gd_scene format=3]

[node name="Root" type="Node2D"]

[node name="%Marker" type="Node2D" parent="."]
position = Vector2(100, 200)
"#;

/// %UniqueName node property mutations on one instance must not affect another.
#[test]
fn unique_name_nodes_independent_across_instances() {
    let scene = PackedScene::from_tscn(SCENE_WITH_UNIQUE_NAMES).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let inst1 = add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();
    let inst2 = add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();

    let marker1 = tree.get_node_relative(inst1, "Marker").unwrap();
    let marker2 = tree.get_node_relative(inst2, "Marker").unwrap();

    // Both should start with position (100, 200).
    match tree.get_node(marker1).unwrap().get_property("position") {
        Variant::Vector2(v) => assert_eq!(v, gdcore::math::Vector2::new(100.0, 200.0)),
        other => panic!("expected Vector2, got {:?}", other),
    }

    // Mutate marker1.
    tree.get_node_mut(marker1)
        .unwrap()
        .set_property("position", Variant::Vector2(gdcore::math::Vector2::new(0.0, 0.0)));

    // Marker2 must be unaffected.
    match tree.get_node(marker2).unwrap().get_property("position") {
        Variant::Vector2(v) => {
            assert_eq!(
                v,
                gdcore::math::Vector2::new(100.0, 200.0),
                "unique-name node on instance 2 must be unaffected (4.6.1 repin)"
            );
        }
        other => panic!("expected Vector2, got {:?}", other),
    }
}

// ===========================================================================
// 24. pat-bs8j: Ownership survives add_child after instancing (4.6.1)
// ===========================================================================

/// After instancing a packed scene, dynamically adding a child to one instance
/// must set the owner correctly and not affect the other instance's children.
#[test]
fn dynamic_child_ownership_after_instancing() {
    use gdscene::node::Node;

    let scene = PackedScene::from_tscn(SCENE_WITH_SIGNAL).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let inst1 = add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();
    let inst2 = add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();

    let inst1_child_count = tree.get_node(inst1).unwrap().children().len();
    let inst2_child_count = tree.get_node(inst2).unwrap().children().len();
    assert_eq!(inst1_child_count, inst2_child_count, "same initial children");

    // Dynamically add a child to instance 1.
    let extra = Node::new("Extra", "Node2D");
    let extra_id = tree.add_child(inst1, extra).unwrap();

    // Instance 1 has one more child.
    assert_eq!(
        tree.get_node(inst1).unwrap().children().len(),
        inst1_child_count + 1,
        "instance 1 should have one extra child"
    );

    // Instance 2 must be unaffected.
    assert_eq!(
        tree.get_node(inst2).unwrap().children().len(),
        inst2_child_count,
        "instance 2 must not gain extra children (4.6.1 repin)"
    );

    // The dynamic child's owner should be inst1 (or the root, depending on convention).
    // At minimum, it should NOT be inst2.
    let extra_owner = tree.get_node(extra_id).unwrap().owner();
    assert_ne!(
        extra_owner,
        Some(inst2),
        "dynamically added child must not be owned by the other instance"
    );
}

// ===========================================================================
// 25. pat-bs8j: Re-instancing produces clean state (4.6.1)
// ===========================================================================

/// Instancing a scene, mutating properties, removing the instance, then
/// re-instancing must produce a clean copy with original property values.
#[test]
fn reinstancing_produces_clean_state_after_mutation() {
    let scene = PackedScene::from_tscn(SCENE_WITH_SUB_RESOURCES).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    // First instance — mutate a property.
    let inst1 = add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();
    tree.get_node_mut(inst1)
        .unwrap()
        .set_property("position", Variant::Vector2(gdcore::math::Vector2::new(999.0, 999.0)));

    // Remove instance 1.
    tree.remove_node(inst1);

    // Re-instance from the same PackedScene.
    let inst_new = add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();

    // New instance must have the original position, not the mutated one.
    match tree.get_node(inst_new).unwrap().get_property("position") {
        Variant::Vector2(v) => {
            assert_eq!(
                v,
                gdcore::math::Vector2::new(0.0, 0.0),
                "re-instanced node must have clean original position (4.6.1 repin)"
            );
        }
        other => panic!("expected Vector2, got {:?}", other),
    }
}
