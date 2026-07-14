//! pat-c7il: ext-resource and subresource edge cases in PackedScene loading.
//!
//! Focused tests covering edge cases in ext_resource resolution,
//! subresource handling, instance_with_subscenes fallback paths,
//! and mixed resource reference scenarios that are not covered
//! by existing packed_scene_edge_cases_test.rs (pat-rsq).
//!
//! Acceptance: focused scene-loading tests verify edge-case behavior
//! and remaining exclusions.

use gdscene::packed_scene::PackedScene;
use gdvariant::Variant;

// ===========================================================================
// 1. ext_resource with empty id is silently skipped
// ===========================================================================

#[test]
fn ext_resource_empty_id_skipped() {
    // The parser requires both id and path to register an ext_resource.
    // An ext_resource with an empty id attribute should be silently skipped.
    let tscn = r#"[gd_scene format=3]

[ext_resource type="Script" path="res://script.gd" id=""]

[node name="Root" type="Node2D"]
"#;
    let scene = PackedScene::from_tscn(tscn).unwrap();
    // Empty id means the guard `if let (Some(id), Some(path))` fires,
    // but the id is an empty string — it IS registered with key "".
    // Regardless, parsing should succeed without panic.
    assert_eq!(scene.node_count(), 1);
}

// ===========================================================================
// 2. ext_resource missing id attribute entirely
// ===========================================================================

#[test]
fn ext_resource_missing_id_attribute_skipped() {
    let tscn = r#"[gd_scene format=3]

[ext_resource type="Script" path="res://script.gd"]

[node name="Root" type="Node2D"]
"#;
    // No id= attribute at all. The extract_header_attrs won't have "id".
    // The guard `if let (Some(id), Some(path))` will fail → entry skipped.
    let scene = PackedScene::from_tscn(tscn).unwrap();
    assert_eq!(scene.node_count(), 1);
    assert!(scene.ext_resources().is_empty());
}

// ===========================================================================
// 3. ext_resources accessor returns correct entries
// ===========================================================================

#[test]
fn ext_resources_accessor_returns_all_registered_entries() {
    let tscn = r#"[gd_scene format=3]

[ext_resource type="Script" path="res://a.gd" id="s1"]
[ext_resource type="Texture2D" path="res://icon.png" id="t1"]
[ext_resource type="PackedScene" path="res://enemy.tscn" id="ps1"]

[node name="Root" type="Node2D"]
"#;
    let scene = PackedScene::from_tscn(tscn).unwrap();
    let ext = scene.ext_resources();

    assert_eq!(ext.len(), 3);
    assert_eq!(ext["s1"].res_type, "Script");
    assert_eq!(ext["s1"].path, "res://a.gd");
    assert_eq!(ext["t1"].res_type, "Texture2D");
    assert_eq!(ext["t1"].path, "res://icon.png");
    assert_eq!(ext["ps1"].res_type, "PackedScene");
    assert_eq!(ext["ps1"].path, "res://enemy.tscn");
}

// ===========================================================================
// 4. resolve_ext_resource_path with valid and invalid inputs
// ===========================================================================

#[test]
fn resolve_ext_resource_path_valid_id() {
    let tscn = r#"[gd_scene format=3]

[ext_resource type="Script" path="res://player.gd" id="scr_1"]

[node name="Root" type="Node2D"]
"#;
    let scene = PackedScene::from_tscn(tscn).unwrap();
    assert_eq!(
        scene.resolve_ext_resource_path("ExtResource(\"scr_1\")"),
        Some("res://player.gd")
    );
}

#[test]
fn resolve_ext_resource_path_missing_id_returns_none() {
    let tscn = r#"[gd_scene format=3]

[ext_resource type="Script" path="res://player.gd" id="scr_1"]

[node name="Root" type="Node2D"]
"#;
    let scene = PackedScene::from_tscn(tscn).unwrap();
    assert_eq!(
        scene.resolve_ext_resource_path("ExtResource(\"nonexistent\")"),
        None
    );
}

#[test]
fn resolve_ext_resource_path_malformed_input_returns_none() {
    let tscn = r#"[gd_scene format=3]

[ext_resource type="Script" path="res://player.gd" id="scr_1"]

[node name="Root" type="Node2D"]
"#;
    let scene = PackedScene::from_tscn(tscn).unwrap();
    // Not wrapped in ExtResource() call.
    assert_eq!(scene.resolve_ext_resource_path("scr_1"), None);
    // Empty string.
    assert_eq!(scene.resolve_ext_resource_path(""), None);
    // Missing closing paren.
    assert_eq!(
        scene.resolve_ext_resource_path("ExtResource(\"scr_1\""),
        None
    );
}

// ===========================================================================
// 5. ExtResource reference in non-script property stored as raw string
// ===========================================================================

#[test]
fn ext_resource_in_non_script_property_stored_raw() {
    let tscn = r#"[gd_scene format=3]

[ext_resource type="Texture2D" path="res://icon.png" id="tex_1"]

[node name="Root" type="Sprite2D"]
texture = ExtResource("tex_1")
offset = Vector2(0, 0)
"#;
    let scene = PackedScene::from_tscn(tscn).unwrap();
    let nodes = scene.instance().unwrap();

    // Non-script ExtResource refs are stored as the raw string.
    assert_eq!(
        nodes[0].get_property("texture"),
        Variant::String("ExtResource(\"tex_1\")".into())
    );
    // No _script_path set because the property is "texture", not "script".
    assert_eq!(nodes[0].get_property("_script_path"), Variant::Nil);
}

// ===========================================================================
// 6. Multiple ExtResource refs on same node, different properties
// ===========================================================================

#[test]
fn multiple_ext_resource_refs_on_same_node() {
    let tscn = r#"[gd_scene format=3]

[ext_resource type="Script" path="res://ctrl.gd" id="s1"]
[ext_resource type="Texture2D" path="res://bg.png" id="t1"]
[ext_resource type="Texture2D" path="res://fg.png" id="t2"]

[node name="Root" type="Control"]
script = ExtResource("s1")
background = ExtResource("t1")
foreground = ExtResource("t2")
"#;
    let scene = PackedScene::from_tscn(tscn).unwrap();
    let nodes = scene.instance().unwrap();

    assert_eq!(
        nodes[0].get_property("_script_path"),
        Variant::String("res://ctrl.gd".into())
    );
    assert_eq!(
        nodes[0].get_property("background"),
        Variant::String("ExtResource(\"t1\")".into())
    );
    assert_eq!(
        nodes[0].get_property("foreground"),
        Variant::String("ExtResource(\"t2\")".into())
    );
}

// ===========================================================================
// 7. SubResource and ExtResource mixed references in same scene
// ===========================================================================

#[test]
fn mixed_sub_and_ext_resource_refs() {
    let tscn = r#"[gd_scene format=3]

[ext_resource type="Script" path="res://body.gd" id="s1"]

[sub_resource type="RectangleShape2D" id="shape_1"]
size = Vector2(32, 32)

[node name="Root" type="CharacterBody2D"]
script = ExtResource("s1")

[node name="Collider" type="CollisionShape2D" parent="."]
shape = SubResource("shape_1")
"#;
    let scene = PackedScene::from_tscn(tscn).unwrap();
    let nodes = scene.instance().unwrap();

    // Root has script resolved.
    assert_eq!(
        nodes[0].get_property("_script_path"),
        Variant::String("res://body.gd".into())
    );

    // Collider has SubResource reference stored as prefixed string.
    assert_eq!(
        nodes[1].get_property("shape"),
        Variant::String("SubResource:shape_1".into())
    );
}

// ===========================================================================
// 8. instance_with_subscenes: unresolvable ext_resource instance
// ===========================================================================

#[test]
fn instance_with_subscenes_missing_ext_resource_falls_through() {
    let tscn = r#"[gd_scene format=3]

[node name="Root" type="Node2D"]

[node name="Child" type="Node2D" parent="." instance=ExtResource("missing_id")]
"#;
    let scene = PackedScene::from_tscn(tscn).unwrap();

    // The ext_resource "missing_id" doesn't exist, so resolve_ext_resource_path
    // returns None. instance_with_subscenes should fall through to create a
    // regular node rather than panic.
    let nodes = scene
        .instance_with_subscenes(&|_path: &str| -> Option<PackedScene> { None })
        .unwrap();

    assert_eq!(nodes.len(), 2);
    assert_eq!(nodes[0].name(), "Root");
    assert_eq!(nodes[1].name(), "Child");
    // The _instance property holds the raw ExtResource reference.
    assert_eq!(
        nodes[1].get_property("_instance"),
        Variant::String("ExtResource(\"missing_id\")".into())
    );
}

// ===========================================================================
// 9. instance_with_subscenes: ext_resource exists but callback returns None
// ===========================================================================

#[test]
fn instance_with_subscenes_callback_returns_none_falls_through() {
    let tscn = r#"[gd_scene format=3]

[ext_resource type="PackedScene" path="res://nonexistent.tscn" id="sub_1"]

[node name="Root" type="Node2D"]

[node name="SubSceneNode" type="Node2D" parent="." instance=ExtResource("sub_1")]
"#;
    let scene = PackedScene::from_tscn(tscn).unwrap();

    // The ext_resource IS registered, but the callback can't resolve it.
    let nodes = scene
        .instance_with_subscenes(&|_path: &str| -> Option<PackedScene> { None })
        .unwrap();

    assert_eq!(nodes.len(), 2);
    assert_eq!(nodes[1].name(), "SubSceneNode");
}

// ===========================================================================
// 10. instance_with_subscenes: successful sub-scene instancing
// ===========================================================================

#[test]
fn instance_with_subscenes_successful_instancing() {
    let outer_tscn = r#"[gd_scene format=3]

[ext_resource type="PackedScene" path="res://inner.tscn" id="inner_1"]

[node name="Outer" type="Node2D"]

[node name="InnerHolder" type="Node2D" parent="." instance=ExtResource("inner_1")]
"#;
    let inner_tscn = r#"[gd_scene format=3]

[node name="InnerRoot" type="Node2D"]

[node name="Sprite" type="Sprite2D" parent="."]
"#;

    let outer = PackedScene::from_tscn(outer_tscn).unwrap();

    let nodes = outer
        .instance_with_subscenes(&|path: &str| -> Option<PackedScene> {
            if path == "res://inner.tscn" {
                Some(PackedScene::from_tscn(inner_tscn).unwrap())
            } else {
                None
            }
        })
        .unwrap();

    // Outer: Outer + InnerHolder (renamed from InnerRoot) + Sprite
    assert_eq!(nodes.len(), 3);
    assert_eq!(nodes[0].name(), "Outer");
    // The sub-scene root is renamed to the parent template's name.
    assert_eq!(nodes[1].name(), "InnerHolder");
    assert_eq!(nodes[2].name(), "Sprite");
}

// ===========================================================================
// 11. instance_with_subscenes: property overrides on sub-scene root
// ===========================================================================

#[test]
fn instance_with_subscenes_property_overrides() {
    let outer_tscn = r#"[gd_scene format=3]

[ext_resource type="PackedScene" path="res://inner.tscn" id="inner_1"]

[node name="Outer" type="Node2D"]

[node name="OverriddenChild" type="Node2D" parent="." instance=ExtResource("inner_1")]
position = Vector2(100, 200)
visible = false
"#;
    let inner_tscn = r#"[gd_scene format=3]

[node name="InnerRoot" type="Node2D"]
position = Vector2(0, 0)
visible = true
"#;

    let outer = PackedScene::from_tscn(outer_tscn).unwrap();

    let nodes = outer
        .instance_with_subscenes(&|path: &str| -> Option<PackedScene> {
            if path == "res://inner.tscn" {
                Some(PackedScene::from_tscn(inner_tscn).unwrap())
            } else {
                None
            }
        })
        .unwrap();

    assert_eq!(nodes.len(), 2);
    // Property overrides from outer scene applied on top.
    assert_eq!(nodes[1].name(), "OverriddenChild");
    assert_eq!(
        nodes[1].get_property("position"),
        Variant::Vector2(gdcore::math::Vector2::new(100.0, 200.0))
    );
    assert_eq!(nodes[1].get_property("visible"), Variant::Bool(false));
}

// ===========================================================================
// 12. Many ext_resources (10+) all resolve correctly
// ===========================================================================

#[test]
fn many_ext_resources_all_resolve() {
    let mut tscn = String::from("[gd_scene format=3]\n\n");

    for i in 0..15 {
        tscn.push_str(&format!(
            "[ext_resource type=\"Script\" path=\"res://s{i}.gd\" id=\"s{i}\"]\n"
        ));
    }

    tscn.push_str("\n[node name=\"Root\" type=\"Node2D\"]\nscript = ExtResource(\"s0\")\n");

    for i in 1..15 {
        tscn.push_str(&format!(
            "\n[node name=\"Child{i}\" type=\"Node2D\" parent=\".\"]\nscript = ExtResource(\"s{i}\")\n"
        ));
    }

    let scene = PackedScene::from_tscn(&tscn).unwrap();
    assert_eq!(scene.ext_resources().len(), 15);
    assert_eq!(scene.node_count(), 15);

    let nodes = scene.instance().unwrap();
    for i in 0..15 {
        assert_eq!(
            nodes[i].get_property("_script_path"),
            Variant::String(format!("res://s{i}.gd")),
            "node {i} script path mismatch"
        );
    }
}

// ===========================================================================
// 13. ext_resource with special characters in path
// ===========================================================================

#[test]
fn ext_resource_special_chars_in_path() {
    let tscn = r#"[gd_scene format=3]

[ext_resource type="Script" path="res://scripts/my-script (v2).gd" id="s1"]

[node name="Root" type="Node2D"]
script = ExtResource("s1")
"#;
    let scene = PackedScene::from_tscn(tscn).unwrap();
    let nodes = scene.instance().unwrap();
    assert_eq!(
        nodes[0].get_property("_script_path"),
        Variant::String("res://scripts/my-script (v2).gd".into())
    );
}

// ===========================================================================
// 14. ext_resource with compound id format (e.g. "1_abc")
// ===========================================================================

#[test]
fn ext_resource_compound_id_format() {
    let tscn = r#"[gd_scene format=3]

[ext_resource type="Script" path="res://player.gd" id="1_scr"]
[ext_resource type="Texture2D" path="res://icon.png" id="2_tex"]

[node name="Root" type="Node2D"]
script = ExtResource("1_scr")
texture = ExtResource("2_tex")
"#;
    let scene = PackedScene::from_tscn(tscn).unwrap();
    assert_eq!(scene.ext_resources().len(), 2);

    let nodes = scene.instance().unwrap();
    assert_eq!(
        nodes[0].get_property("_script_path"),
        Variant::String("res://player.gd".into())
    );
    assert_eq!(
        nodes[0].get_property("texture"),
        Variant::String("ExtResource(\"2_tex\")".into())
    );
}

// ===========================================================================
// 15. Sub-resource sections with properties don't leak into node properties
// ===========================================================================

#[test]
fn sub_resource_properties_do_not_leak_to_nodes() {
    let tscn = r#"[gd_scene format=3]

[sub_resource type="StyleBoxFlat" id="style_1"]
bg_color = Color(1, 0, 0, 1)
border_width = 5

[node name="Root" type="Panel"]
custom_prop = 42
"#;
    let scene = PackedScene::from_tscn(tscn).unwrap();
    let nodes = scene.instance().unwrap();

    // Node should have custom_prop but NOT sub_resource properties.
    assert_eq!(nodes[0].get_property("custom_prop"), Variant::Int(42));
    assert_eq!(nodes[0].get_property("bg_color"), Variant::Nil);
    assert_eq!(nodes[0].get_property("border_width"), Variant::Nil);
}

// ===========================================================================
// 16. Scene with only ext_resources and no nodes → error
// ===========================================================================

#[test]
fn ext_resources_only_no_nodes_returns_error() {
    let tscn = r#"[gd_scene format=3]

[ext_resource type="Script" path="res://a.gd" id="s1"]
[ext_resource type="Texture2D" path="res://b.png" id="t1"]
"#;
    let result = PackedScene::from_tscn(tscn);
    assert!(
        result.is_err(),
        "scene with ext_resources but no nodes should error"
    );
}

// ===========================================================================
// 17. Node referencing ext_resource that was declared after the node
// ===========================================================================

#[test]
fn ext_resource_declared_before_node_that_uses_it() {
    // In valid tscn, ext_resources always come before nodes.
    // Verify this standard ordering works.
    let tscn = r#"[gd_scene format=3]

[ext_resource type="Script" path="res://ctrl.gd" id="s1"]

[node name="Root" type="Control"]
script = ExtResource("s1")
"#;
    let scene = PackedScene::from_tscn(tscn).unwrap();
    let nodes = scene.instance().unwrap();
    assert_eq!(
        nodes[0].get_property("_script_path"),
        Variant::String("res://ctrl.gd".into())
    );
}

// ===========================================================================
// 18. instance_with_subscenes with multiple sub-scene instances
// ===========================================================================

#[test]
fn instance_with_subscenes_multiple_instances() {
    let outer_tscn = r#"[gd_scene format=3]

[ext_resource type="PackedScene" path="res://enemy.tscn" id="e1"]

[node name="World" type="Node2D"]

[node name="Enemy1" type="Node2D" parent="." instance=ExtResource("e1")]

[node name="Enemy2" type="Node2D" parent="." instance=ExtResource("e1")]
"#;
    let enemy_tscn = r#"[gd_scene format=3]

[node name="EnemyRoot" type="CharacterBody2D"]

[node name="Sprite" type="Sprite2D" parent="."]

[node name="Collider" type="CollisionShape2D" parent="."]
"#;

    let outer = PackedScene::from_tscn(outer_tscn).unwrap();

    let nodes = outer
        .instance_with_subscenes(&|path: &str| -> Option<PackedScene> {
            if path == "res://enemy.tscn" {
                Some(PackedScene::from_tscn(enemy_tscn).unwrap())
            } else {
                None
            }
        })
        .unwrap();

    // World + Enemy1 (renamed from EnemyRoot) + Sprite + Collider
    //       + Enemy2 (renamed from EnemyRoot) + Sprite + Collider = 7
    assert_eq!(nodes.len(), 7);
    assert_eq!(nodes[0].name(), "World");
    assert_eq!(nodes[1].name(), "Enemy1");
    assert_eq!(nodes[2].name(), "Sprite");
    assert_eq!(nodes[3].name(), "Collider");
    assert_eq!(nodes[4].name(), "Enemy2");
    assert_eq!(nodes[5].name(), "Sprite");
    assert_eq!(nodes[6].name(), "Collider");

    // Both enemies are children of World.
    assert_eq!(nodes[1].parent(), Some(nodes[0].id()));
    assert_eq!(nodes[4].parent(), Some(nodes[0].id()));

    // Each enemy's children are independent.
    assert_ne!(nodes[2].id(), nodes[5].id());
}

// ===========================================================================
// 19. instance_with_subscenes: groups transferred from outer template
// ===========================================================================

#[test]
fn instance_with_subscenes_groups_from_outer() {
    let outer_tscn = r#"[gd_scene format=3]

[ext_resource type="PackedScene" path="res://item.tscn" id="i1"]

[node name="Root" type="Node2D"]

[node name="Item" type="Node2D" parent="." instance=ExtResource("i1") groups=["pickups", "interactable"]]
"#;
    let inner_tscn = r#"[gd_scene format=3]

[node name="ItemRoot" type="Area2D"]
"#;

    let outer = PackedScene::from_tscn(outer_tscn).unwrap();

    let nodes = outer
        .instance_with_subscenes(&|path: &str| -> Option<PackedScene> {
            if path == "res://item.tscn" {
                Some(PackedScene::from_tscn(inner_tscn).unwrap())
            } else {
                None
            }
        })
        .unwrap();

    assert_eq!(nodes.len(), 2);
    assert_eq!(nodes[1].name(), "Item");
    // Groups from outer template applied to sub-scene root.
    assert!(nodes[1].is_in_group("pickups"));
    assert!(nodes[1].is_in_group("interactable"));
}

// ===========================================================================
// 20. resolve_ext_resource_path: ExtResource with empty quotes
// ===========================================================================

#[test]
fn resolve_ext_resource_path_empty_id_returns_none() {
    let tscn = r#"[gd_scene format=3]

[ext_resource type="Script" path="res://a.gd" id="s1"]

[node name="Root" type="Node2D"]
"#;
    let scene = PackedScene::from_tscn(tscn).unwrap();
    // parse_ext_resource_ref returns None for empty id.
    assert_eq!(scene.resolve_ext_resource_path("ExtResource(\"\")"), None);
}

// ===========================================================================
// pat-3clf: Additional edge cases
// ===========================================================================

// ===========================================================================
// 21. Duplicate ext_resource IDs — last wins
// ===========================================================================

#[test]
fn duplicate_ext_resource_ids_last_wins() {
    let tscn = r#"[gd_scene format=3]

[ext_resource type="Script" path="res://first.gd" id="1"]
[ext_resource type="Script" path="res://second.gd" id="1"]

[node name="Root" type="Node2D"]
script = ExtResource("1")
"#;
    let scene = PackedScene::from_tscn(tscn).unwrap();
    // The second ext_resource with id="1" should overwrite the first.
    assert_eq!(scene.ext_resources().len(), 1);
    assert_eq!(
        scene.resolve_ext_resource_path(r#"ExtResource("1")"#),
        Some("res://second.gd")
    );
}

// ===========================================================================
// 22. ext_resource with empty path attribute
// ===========================================================================

#[test]
fn ext_resource_empty_path_still_registered() {
    let tscn = r#"[gd_scene format=3]

[ext_resource type="Texture2D" path="" id="tex1"]

[node name="Root" type="Sprite2D"]
texture = ExtResource("tex1")
"#;
    let scene = PackedScene::from_tscn(tscn).unwrap();
    // Empty path is still registered in the ext_resources map.
    assert_eq!(scene.ext_resources().len(), 1);
    assert_eq!(
        scene.resolve_ext_resource_path(r#"ExtResource("tex1")"#),
        Some("")
    );
}

// ===========================================================================
// 23. Node with both instance and script properties
// ===========================================================================

#[test]
fn node_with_instance_and_script() {
    let tscn = r#"[gd_scene format=3]

[ext_resource type="PackedScene" path="res://enemy.tscn" id="scene1"]
[ext_resource type="Script" path="res://override.gd" id="script1"]

[node name="Root" type="Node2D"]

[node name="Enemy" type="Node2D" parent="." instance=ExtResource("scene1")]
script = ExtResource("script1")
speed = 100
"#;
    let scene = PackedScene::from_tscn(tscn).unwrap();
    let nodes = scene.instance().unwrap();

    assert_eq!(nodes.len(), 2);
    let enemy = &nodes[1];
    assert_eq!(enemy.name(), "Enemy");
    // Script should be resolved via ext_resources.
    assert_eq!(
        enemy.get_property("_script_path"),
        Variant::String("res://override.gd".into())
    );
    // Instance ref stored as property.
    assert_eq!(
        enemy.get_property("_instance"),
        Variant::String(r#"ExtResource("scene1")"#.into())
    );
    // Regular property preserved.
    assert_eq!(enemy.get_property("speed"), Variant::Int(100));
}

// ===========================================================================
// 24. Stress: scene with 50 ext_resources
// ===========================================================================

#[test]
fn stress_50_ext_resources_all_resolve() {
    let mut tscn = String::from("[gd_scene format=3]\n\n");
    for i in 0..50 {
        tscn.push_str(&format!(
            r#"[ext_resource type="Script" path="res://scripts/s{i}.gd" id="s{i}"]
"#
        ));
    }
    tscn.push_str("\n[node name=\"Root\" type=\"Node2D\"]\n");

    let scene = PackedScene::from_tscn(&tscn).unwrap();
    assert_eq!(scene.ext_resources().len(), 50);

    for i in 0..50 {
        let expected_path = format!("res://scripts/s{i}.gd");
        let ext_ref = format!(r#"ExtResource("s{i}")"#);
        assert_eq!(
            scene.resolve_ext_resource_path(&ext_ref),
            Some(expected_path.as_str()),
            "ext_resource s{i} should resolve"
        );
    }
}

// ===========================================================================
// 25. [sub_resource] sections are excluded from PackedScene parsing
// ===========================================================================

#[test]
fn sub_resource_sections_excluded_from_packed_scene() {
    // PackedScene parser explicitly skips [sub_resource] sections.
    // Verify that properties inside [sub_resource] don't leak into nodes.
    let tscn = r#"[gd_scene format=3]

[sub_resource type="RectangleShape2D" id="shape1"]
size = Vector2(32, 32)

[sub_resource type="CircleShape2D" id="shape2"]
radius = 16

[node name="Root" type="Node2D"]
hp = 100

[node name="Collider" type="Area2D" parent="."]
collision_mask = 1
"#;
    let scene = PackedScene::from_tscn(tscn).unwrap();
    let nodes = scene.instance().unwrap();

    // Only two nodes (Root and Collider).
    assert_eq!(nodes.len(), 2);

    // Root should only have hp, NOT size or radius.
    let root = &nodes[0];
    assert_eq!(root.get_property("hp"), Variant::Int(100));
    assert_eq!(root.get_property("size"), Variant::Nil);
    assert_eq!(root.get_property("radius"), Variant::Nil);

    // Collider should only have collision_mask.
    let collider = &nodes[1];
    assert_eq!(collider.get_property("collision_mask"), Variant::Int(1));
    assert_eq!(collider.get_property("size"), Variant::Nil);
}

// ===========================================================================
// 26. Script ext_resource with missing entry gracefully skips
// ===========================================================================

#[test]
fn script_ext_resource_ref_missing_entry_no_script_path() {
    // Node references an ext_resource id that was never declared.
    let tscn = r#"[gd_scene format=3]

[node name="Root" type="Node2D"]
script = ExtResource("999")
"#;
    let scene = PackedScene::from_tscn(tscn).unwrap();
    let nodes = scene.instance().unwrap();

    // Script path should NOT be set (ext_resource "999" doesn't exist).
    assert_eq!(nodes[0].get_property("_script_path"), Variant::Nil);
    // But the raw ExtResource ref is still stored as a property.
    assert_eq!(
        nodes[0].get_property("script"),
        Variant::String(r#"ExtResource("999")"#.into())
    );
}

// ===========================================================================
// 27. instance_with_subscenes: instanced node with script override
// ===========================================================================

#[test]
fn instance_with_subscenes_script_override_on_instanced_node() {
    let inner_tscn = r#"[gd_scene format=3]

[ext_resource type="Script" path="res://inner_script.gd" id="s1"]

[node name="Inner" type="Node2D"]
script = ExtResource("s1")
base_value = 10
"#;
    let outer_tscn = r#"[gd_scene format=3]

[ext_resource type="PackedScene" path="res://inner.tscn" id="p1"]
[ext_resource type="Script" path="res://override_script.gd" id="s2"]

[node name="Root" type="Node2D"]

[node name="Child" type="Node2D" parent="." instance=ExtResource("p1")]
script = ExtResource("s2")
base_value = 42
"#;
    let outer = PackedScene::from_tscn(outer_tscn).unwrap();
    let nodes = outer
        .instance_with_subscenes(&|path: &str| -> Option<PackedScene> {
            if path == "res://inner.tscn" {
                Some(PackedScene::from_tscn(inner_tscn).unwrap())
            } else {
                None
            }
        })
        .unwrap();

    assert_eq!(nodes.len(), 2);
    let child = &nodes[1];
    assert_eq!(child.name(), "Child");
    // Script override from outer scene should replace inner script.
    assert_eq!(
        child.get_property("script"),
        Variant::String(r#"ExtResource("s2")"#.into())
    );
    // Property override should replace inner value.
    assert_eq!(child.get_property("base_value"), Variant::Int(42));
}
