//! pat-3clf: Advanced ext-resource and subresource edge cases in PackedScene loading.
//!
//! Covers edge cases NOT already handled by packed_scene_edge_cases_test.rs (pat-rsq)
//! or packed_scene_ext_sub_edge_cases_test.rs (pat-c7il):
//!
//! - Deeply nested sub-scene instancing (3+ levels)
//! - Sub-scene with its own ext_resources and scripts
//! - Multiple SubResource references on the same node
//! - SubResource reference to non-existent sub_resource id
//! - Empty sub_resource section (no properties)
//! - Sub-scene instancing with unique name flag
//! - Instance attribute on root node (unusual but parseable)
//! - Sub-scene with connections (connections from inner scene)
//! - ext_resource ordering: declared interleaved with sub_resources
//! - Property override on sub-scene child that has its own script

use gdscene::packed_scene::PackedScene;
use gdvariant::Variant;

// ===========================================================================
// 1. Deeply nested sub-scene instancing (3 levels)
// ===========================================================================

#[test]
fn instance_with_subscenes_three_levels_deep() {
    let level3_tscn = r#"[gd_scene format=3]

[node name="Leaf" type="Sprite2D"]
"#;
    let level2_tscn = r#"[gd_scene format=3]

[ext_resource type="PackedScene" path="res://level3.tscn" id="l3"]

[node name="Middle" type="Node2D"]

[node name="LeafHolder" type="Node2D" parent="." instance=ExtResource("l3")]
"#;
    let level1_tscn = r#"[gd_scene format=3]

[ext_resource type="PackedScene" path="res://level2.tscn" id="l2"]

[node name="Top" type="Node2D"]

[node name="MiddleHolder" type="Node2D" parent="." instance=ExtResource("l2")]
"#;

    let top = PackedScene::from_tscn(level1_tscn).unwrap();

    let nodes = top
        .instance_with_subscenes(&|path: &str| -> Option<PackedScene> {
            match path {
                "res://level2.tscn" => Some(PackedScene::from_tscn(level2_tscn).unwrap()),
                "res://level3.tscn" => Some(PackedScene::from_tscn(level3_tscn).unwrap()),
                _ => None,
            }
        })
        .unwrap();

    // Top + MiddleHolder (renamed from Middle) + LeafHolder (renamed from Leaf) = 3
    assert_eq!(nodes.len(), 3);
    assert_eq!(nodes[0].name(), "Top");
    assert_eq!(nodes[1].name(), "MiddleHolder");
    assert_eq!(nodes[2].name(), "LeafHolder");

    // Ownership follows Godot semantics: each sub-scene root is owned
    // by its parent scene root, and sub-scene children are owned by
    // their own sub-scene root.
    let root_id = nodes[0].id();
    let middle_id = nodes[1].id();
    // MiddleHolder (sub-scene root of level2) owned by Top.
    assert_eq!(nodes[1].owner(), Some(root_id));
    // LeafHolder (sub-scene root of level3, nested inside level2)
    // is owned by MiddleHolder (its parent scene root).
    assert_eq!(nodes[2].owner(), Some(middle_id));
}

// ===========================================================================
// 2. Sub-scene with its own ext_resources and scripts
// ===========================================================================

#[test]
fn sub_scene_with_own_ext_resources_and_script() {
    let outer_tscn = r#"[gd_scene format=3]

[ext_resource type="PackedScene" path="res://inner_scripted.tscn" id="inner"]

[node name="Outer" type="Node2D"]

[node name="ScriptedChild" type="Node2D" parent="." instance=ExtResource("inner")]
"#;
    let inner_tscn = r#"[gd_scene format=3]

[ext_resource type="Script" path="res://inner_logic.gd" id="inner_scr"]
[ext_resource type="Texture2D" path="res://inner_tex.png" id="inner_tex"]

[node name="InnerRoot" type="Node2D"]
script = ExtResource("inner_scr")

[node name="InnerSprite" type="Sprite2D" parent="."]
texture = ExtResource("inner_tex")
"#;

    let outer = PackedScene::from_tscn(outer_tscn).unwrap();

    let nodes = outer
        .instance_with_subscenes(&|path: &str| -> Option<PackedScene> {
            if path == "res://inner_scripted.tscn" {
                Some(PackedScene::from_tscn(inner_tscn).unwrap())
            } else {
                None
            }
        })
        .unwrap();

    assert_eq!(nodes.len(), 3);
    assert_eq!(nodes[0].name(), "Outer");
    assert_eq!(nodes[1].name(), "ScriptedChild");
    assert_eq!(nodes[2].name(), "InnerSprite");

    // Inner scene's script was resolved from inner ext_resources.
    assert_eq!(
        nodes[1].get_property("_script_path"),
        Variant::String("res://inner_logic.gd".into())
    );

    // Inner sprite's texture is stored as raw ExtResource ref (not resolved to path).
    assert_eq!(
        nodes[2].get_property("texture"),
        Variant::String("ExtResource(\"inner_tex\")".into())
    );
}

// ===========================================================================
// 3. Multiple SubResource references on the same node
// ===========================================================================

#[test]
fn multiple_sub_resource_refs_on_same_node() {
    let tscn = r#"[gd_scene format=3]

[sub_resource type="RectangleShape2D" id="rect_shape"]
size = Vector2(32, 32)

[sub_resource type="PhysicsMaterial" id="phys_mat"]
bounce = 0.5

[node name="Root" type="RigidBody2D"]
shape = SubResource("rect_shape")
physics_material = SubResource("phys_mat")
"#;
    let scene = PackedScene::from_tscn(tscn).unwrap();
    let nodes = scene.instance().unwrap();

    assert_eq!(
        nodes[0].get_property("shape"),
        Variant::String("SubResource:rect_shape".into())
    );
    assert_eq!(
        nodes[0].get_property("physics_material"),
        Variant::String("SubResource:phys_mat".into())
    );
}

// ===========================================================================
// 4. SubResource reference to non-existent sub_resource id
// ===========================================================================

#[test]
fn sub_resource_ref_to_nonexistent_id_stored_as_string() {
    let tscn = r#"[gd_scene format=3]

[node name="Root" type="CollisionShape2D"]
shape = SubResource("nonexistent_shape")
"#;
    let scene = PackedScene::from_tscn(tscn).unwrap();
    let nodes = scene.instance().unwrap();

    // SubResource references are stored as prefixed strings regardless
    // of whether the sub_resource section exists.
    assert_eq!(
        nodes[0].get_property("shape"),
        Variant::String("SubResource:nonexistent_shape".into())
    );
}

// ===========================================================================
// 5. Empty sub_resource section (no properties)
// ===========================================================================

#[test]
fn empty_sub_resource_section_no_crash() {
    let tscn = r#"[gd_scene format=3]

[sub_resource type="RectangleShape2D" id="empty_shape"]

[node name="Root" type="CollisionShape2D"]
shape = SubResource("empty_shape")
"#;
    let scene = PackedScene::from_tscn(tscn).unwrap();
    let nodes = scene.instance().unwrap();

    assert_eq!(nodes.len(), 1);
    assert_eq!(
        nodes[0].get_property("shape"),
        Variant::String("SubResource:empty_shape".into())
    );
}

// ===========================================================================
// 6. Sub-scene instancing preserves unique name flag from outer scene
// ===========================================================================

#[test]
fn instance_with_subscenes_preserves_unique_name() {
    let outer_tscn = r#"[gd_scene format=3]

[ext_resource type="PackedScene" path="res://dialog.tscn" id="dlg"]

[node name="Root" type="Control"]

[node name="%DialogBox" type="Control" parent="." instance=ExtResource("dlg")]
"#;
    let inner_tscn = r#"[gd_scene format=3]

[node name="DialogRoot" type="Panel"]

[node name="Label" type="Label" parent="."]
"#;

    let outer = PackedScene::from_tscn(outer_tscn).unwrap();

    let nodes = outer
        .instance_with_subscenes(&|path: &str| -> Option<PackedScene> {
            if path == "res://dialog.tscn" {
                Some(PackedScene::from_tscn(inner_tscn).unwrap())
            } else {
                None
            }
        })
        .unwrap();

    assert_eq!(nodes.len(), 3);
    // Sub-scene root renamed to template name (without % prefix).
    assert_eq!(nodes[1].name(), "DialogBox");
    // Unique name flag propagated from outer template.
    assert!(nodes[1].is_unique_name());
}

// ===========================================================================
// 7. Instance attribute on root node (edge case: root has instance ref)
// ===========================================================================

#[test]
fn root_node_with_instance_attribute_stored_as_property() {
    // In Godot, a root node with instance= is technically an inherited scene.
    // Our parser stores it as _instance property on the root.
    let tscn = r#"[gd_scene format=3]

[ext_resource type="PackedScene" path="res://base.tscn" id="base"]

[node name="Derived" instance=ExtResource("base")]
"#;
    let scene = PackedScene::from_tscn(tscn).unwrap();
    let nodes = scene.instance().unwrap();

    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes[0].name(), "Derived");
    assert_eq!(
        nodes[0].get_property("_instance"),
        Variant::String("ExtResource(\"base\")".into())
    );
}

// ===========================================================================
// 8. ext_resources interleaved with sub_resources
// ===========================================================================

#[test]
fn ext_and_sub_resources_interleaved() {
    let tscn = r#"[gd_scene format=3]

[ext_resource type="Script" path="res://main.gd" id="s1"]

[sub_resource type="RectangleShape2D" id="rect1"]
size = Vector2(10, 10)

[ext_resource type="Texture2D" path="res://bg.png" id="t1"]

[sub_resource type="CircleShape2D" id="circ1"]
radius = 5.0

[node name="Root" type="Node2D"]
script = ExtResource("s1")
bg = ExtResource("t1")
shape1 = SubResource("rect1")
shape2 = SubResource("circ1")
"#;
    let scene = PackedScene::from_tscn(tscn).unwrap();
    let ext = scene.ext_resources();

    // Both ext_resources registered despite interleaving.
    assert_eq!(ext.len(), 2);
    assert_eq!(ext["s1"].path, "res://main.gd");
    assert_eq!(ext["t1"].path, "res://bg.png");

    let nodes = scene.instance().unwrap();
    assert_eq!(
        nodes[0].get_property("_script_path"),
        Variant::String("res://main.gd".into())
    );
    assert_eq!(
        nodes[0].get_property("shape1"),
        Variant::String("SubResource:rect1".into())
    );
    assert_eq!(
        nodes[0].get_property("shape2"),
        Variant::String("SubResource:circ1".into())
    );
}

// ===========================================================================
// 9. Property override on sub-scene that has script — script overridden
// ===========================================================================

#[test]
fn property_override_replaces_sub_scene_script() {
    let outer_tscn = r#"[gd_scene format=3]

[ext_resource type="PackedScene" path="res://base_item.tscn" id="item"]
[ext_resource type="Script" path="res://override_item.gd" id="override_scr"]

[node name="Root" type="Node2D"]

[node name="CustomItem" type="Node2D" parent="." instance=ExtResource("item")]
script = ExtResource("override_scr")
"#;
    let inner_tscn = r#"[gd_scene format=3]

[ext_resource type="Script" path="res://base_item.gd" id="base_scr"]

[node name="ItemRoot" type="Node2D"]
script = ExtResource("base_scr")
"#;

    let outer = PackedScene::from_tscn(outer_tscn).unwrap();

    let nodes = outer
        .instance_with_subscenes(&|path: &str| -> Option<PackedScene> {
            if path == "res://base_item.tscn" {
                Some(PackedScene::from_tscn(inner_tscn).unwrap())
            } else {
                None
            }
        })
        .unwrap();

    assert_eq!(nodes.len(), 2);
    assert_eq!(nodes[1].name(), "CustomItem");

    // The outer scene's property override for "script" should be applied.
    // Since outer stores it as raw ExtResource string, it overrides the
    // inner scene's resolved _script_path with the raw reference.
    let script_prop = nodes[1].get_property("script");
    assert_eq!(
        script_prop,
        Variant::String("ExtResource(\"override_scr\")".into())
    );
}

// ===========================================================================
// 10. Sub-scene with no children — just the root node
// ===========================================================================

#[test]
fn instance_with_subscenes_single_node_sub_scene() {
    let outer_tscn = r#"[gd_scene format=3]

[ext_resource type="PackedScene" path="res://marker.tscn" id="m1"]

[node name="Root" type="Node2D"]

[node name="Marker" type="Node2D" parent="." instance=ExtResource("m1")]
"#;
    let inner_tscn = r#"[gd_scene format=3]

[node name="MarkerRoot" type="Marker2D"]
"#;

    let outer = PackedScene::from_tscn(outer_tscn).unwrap();

    let nodes = outer
        .instance_with_subscenes(&|path: &str| -> Option<PackedScene> {
            if path == "res://marker.tscn" {
                Some(PackedScene::from_tscn(inner_tscn).unwrap())
            } else {
                None
            }
        })
        .unwrap();

    assert_eq!(nodes.len(), 2);
    assert_eq!(nodes[0].name(), "Root");
    assert_eq!(nodes[1].name(), "Marker");
    // Type from inner scene is preserved.
    assert_eq!(nodes[1].class_name(), "Marker2D");
}

// ===========================================================================
// 11. Sub-scene and regular nodes as siblings
// ===========================================================================

#[test]
fn instance_with_subscenes_mixed_regular_and_instanced_children() {
    let outer_tscn = r#"[gd_scene format=3]

[ext_resource type="PackedScene" path="res://sub.tscn" id="sub1"]

[node name="Root" type="Node2D"]

[node name="RegularChild" type="Sprite2D" parent="."]
visible = true

[node name="InstancedChild" type="Node2D" parent="." instance=ExtResource("sub1")]

[node name="AnotherRegular" type="Label" parent="."]
text = "hello"
"#;
    let inner_tscn = r#"[gd_scene format=3]

[node name="SubRoot" type="Area2D"]

[node name="SubSprite" type="Sprite2D" parent="."]
"#;

    let outer = PackedScene::from_tscn(outer_tscn).unwrap();

    let nodes = outer
        .instance_with_subscenes(&|path: &str| -> Option<PackedScene> {
            if path == "res://sub.tscn" {
                Some(PackedScene::from_tscn(inner_tscn).unwrap())
            } else {
                None
            }
        })
        .unwrap();

    // Root + RegularChild + InstancedChild(renamed SubRoot) + SubSprite + AnotherRegular = 5
    assert_eq!(nodes.len(), 5);
    assert_eq!(nodes[0].name(), "Root");
    assert_eq!(nodes[1].name(), "RegularChild");
    assert_eq!(nodes[1].class_name(), "Sprite2D");
    assert_eq!(nodes[2].name(), "InstancedChild");
    assert_eq!(nodes[2].class_name(), "Area2D");
    assert_eq!(nodes[3].name(), "SubSprite");
    assert_eq!(nodes[4].name(), "AnotherRegular");
    assert_eq!(nodes[4].class_name(), "Label");
}

// ===========================================================================
// 12. Sub-scene instancing where callback panics-safely returns None
//     for one path but succeeds for another
// ===========================================================================

#[test]
fn instance_with_subscenes_partial_resolution() {
    let outer_tscn = r#"[gd_scene format=3]

[ext_resource type="PackedScene" path="res://found.tscn" id="found"]
[ext_resource type="PackedScene" path="res://missing.tscn" id="missing"]

[node name="Root" type="Node2D"]

[node name="Good" type="Node2D" parent="." instance=ExtResource("found")]

[node name="Bad" type="Node2D" parent="." instance=ExtResource("missing")]
"#;
    let found_tscn = r#"[gd_scene format=3]

[node name="FoundRoot" type="Control"]
"#;

    let outer = PackedScene::from_tscn(outer_tscn).unwrap();

    let nodes = outer
        .instance_with_subscenes(&|path: &str| -> Option<PackedScene> {
            if path == "res://found.tscn" {
                Some(PackedScene::from_tscn(found_tscn).unwrap())
            } else {
                None
            }
        })
        .unwrap();

    // Root + Good (resolved from FoundRoot) + Bad (fallthrough, regular node)
    assert_eq!(nodes.len(), 3);
    assert_eq!(nodes[1].name(), "Good");
    assert_eq!(nodes[1].class_name(), "Control");
    assert_eq!(nodes[2].name(), "Bad");
    // Bad falls through to regular node creation with original class.
    assert_eq!(nodes[2].class_name(), "Node2D");
    // Bad has _instance property set from fallthrough.
    assert_eq!(
        nodes[2].get_property("_instance"),
        Variant::String("ExtResource(\"missing\")".into())
    );
}

// ===========================================================================
// 13. Many sub_resources (10+) interspersed with nodes
// ===========================================================================

#[test]
fn many_sub_resources_all_referenced() {
    let mut tscn = String::from("[gd_scene format=3]\n\n");

    for i in 0..12 {
        tscn.push_str(&format!(
            "[sub_resource type=\"RectangleShape2D\" id=\"shape_{i}\"]\nsize = Vector2({i}, {i})\n\n"
        ));
    }

    tscn.push_str("[node name=\"Root\" type=\"Node2D\"]\n");
    for i in 0..12 {
        tscn.push_str(&format!("shape_{i} = SubResource(\"shape_{i}\")\n"));
    }

    let scene = PackedScene::from_tscn(&tscn).unwrap();
    let nodes = scene.instance().unwrap();

    for i in 0..12 {
        assert_eq!(
            nodes[0].get_property(&format!("shape_{i}")),
            Variant::String(format!("SubResource:shape_{i}")),
            "SubResource reference shape_{i} mismatch"
        );
    }
}

// ===========================================================================
// 14. ext_resource with type but no path and no id — completely skipped
// ===========================================================================

#[test]
fn ext_resource_with_only_type_skipped() {
    let tscn = r#"[gd_scene format=3]

[ext_resource type="Script"]

[node name="Root" type="Node2D"]
"#;
    let scene = PackedScene::from_tscn(tscn).unwrap();
    assert!(scene.ext_resources().is_empty());
    assert_eq!(scene.node_count(), 1);
}

// ===========================================================================
// 15. Sub-scene instancing with sibling after instanced subtree
//     referencing a path that includes the instanced node
// ===========================================================================

#[test]
fn sibling_after_instanced_subtree_with_child_under_it() {
    let outer_tscn = r#"[gd_scene format=3]

[ext_resource type="PackedScene" path="res://panel.tscn" id="panel"]

[node name="Root" type="Control"]

[node name="MyPanel" type="Control" parent="." instance=ExtResource("panel")]

[node name="Footer" type="Label" parent="."]
text = "footer"
"#;
    let inner_tscn = r#"[gd_scene format=3]

[node name="PanelRoot" type="Panel"]

[node name="Title" type="Label" parent="."]

[node name="Content" type="RichTextLabel" parent="."]
"#;

    let outer = PackedScene::from_tscn(outer_tscn).unwrap();

    let nodes = outer
        .instance_with_subscenes(&|path: &str| -> Option<PackedScene> {
            if path == "res://panel.tscn" {
                Some(PackedScene::from_tscn(inner_tscn).unwrap())
            } else {
                None
            }
        })
        .unwrap();

    // Root + MyPanel(PanelRoot) + Title + Content + Footer = 5
    assert_eq!(nodes.len(), 5);
    assert_eq!(nodes[0].name(), "Root");
    assert_eq!(nodes[1].name(), "MyPanel");
    assert_eq!(nodes[1].class_name(), "Panel");
    assert_eq!(nodes[2].name(), "Title");
    assert_eq!(nodes[3].name(), "Content");
    assert_eq!(nodes[4].name(), "Footer");
    assert_eq!(nodes[4].class_name(), "Label");

    // Footer is a child of Root, not of MyPanel.
    assert_eq!(nodes[4].parent(), Some(nodes[0].id()));
}

// ===========================================================================
// 16. Scene with sub_resource and ext_resource having same ID string
// ===========================================================================

#[test]
fn sub_and_ext_resource_same_id_string_no_collision() {
    // SubResource and ExtResource IDs live in separate namespaces.
    let tscn = r#"[gd_scene format=3]

[ext_resource type="Script" path="res://script.gd" id="shared_id"]

[sub_resource type="RectangleShape2D" id="shared_id"]
size = Vector2(10, 10)

[node name="Root" type="Node2D"]
script = ExtResource("shared_id")
shape = SubResource("shared_id")
"#;
    let scene = PackedScene::from_tscn(tscn).unwrap();
    let nodes = scene.instance().unwrap();

    // Script resolved from ext_resource.
    assert_eq!(
        nodes[0].get_property("_script_path"),
        Variant::String("res://script.gd".into())
    );
    // Shape stored as SubResource reference.
    assert_eq!(
        nodes[0].get_property("shape"),
        Variant::String("SubResource:shared_id".into())
    );
}

// ===========================================================================
// 17. Connection section after sub_resources and nodes
// ===========================================================================

#[test]
fn connections_after_sub_resources_and_nodes() {
    let tscn = r#"[gd_scene format=3]

[sub_resource type="ButtonGroup" id="btn_group"]

[node name="Root" type="Control"]

[node name="Button" type="Button" parent="."]
button_group = SubResource("btn_group")

[connection signal="pressed" from="Button" to="." method="_on_button_pressed"]
"#;
    let scene = PackedScene::from_tscn(tscn).unwrap();

    assert_eq!(scene.node_count(), 2);
    assert_eq!(scene.connection_count(), 1);

    let conn = &scene.connections()[0];
    assert_eq!(conn.signal_name, "pressed");
    assert_eq!(conn.from_path, "Button");
    assert_eq!(conn.to_path, ".");
    assert_eq!(conn.method_name, "_on_button_pressed");

    let nodes = scene.instance().unwrap();
    assert_eq!(
        nodes[1].get_property("button_group"),
        Variant::String("SubResource:btn_group".into())
    );
}

// ===========================================================================
// 18. Sub-scene instancing: inner scene has connections (preserved in parse)
// ===========================================================================

#[test]
fn sub_scene_connections_are_preserved_in_inner_parse() {
    let inner_tscn = r#"[gd_scene format=3]

[node name="InnerRoot" type="Control"]

[node name="Btn" type="Button" parent="."]

[connection signal="pressed" from="Btn" to="." method="_on_btn"]
"#;

    let inner = PackedScene::from_tscn(inner_tscn).unwrap();
    assert_eq!(inner.connection_count(), 1);
    assert_eq!(inner.connections()[0].signal_name, "pressed");
}

// ===========================================================================
// 19. ext_resource with very long path
// ===========================================================================

#[test]
fn ext_resource_long_path() {
    let long_path = format!(
        "res://deeply/nested/{}/path/to/script.gd",
        "subdir/".repeat(20)
    );
    let tscn = format!(
        r#"[gd_scene format=3]

[ext_resource type="Script" path="{long_path}" id="long_s"]

[node name="Root" type="Node2D"]
script = ExtResource("long_s")
"#
    );

    let scene = PackedScene::from_tscn(&tscn).unwrap();
    let nodes = scene.instance().unwrap();

    assert_eq!(
        nodes[0].get_property("_script_path"),
        Variant::String(long_path)
    );
}

// ===========================================================================
// 20. Instance with subscenes: two different sub-scenes under same parent
// ===========================================================================

#[test]
fn instance_with_subscenes_different_sub_scenes_same_parent() {
    let outer_tscn = r#"[gd_scene format=3]

[ext_resource type="PackedScene" path="res://player.tscn" id="p1"]
[ext_resource type="PackedScene" path="res://enemy.tscn" id="e1"]

[node name="World" type="Node2D"]

[node name="Player" type="Node2D" parent="." instance=ExtResource("p1")]

[node name="Enemy" type="Node2D" parent="." instance=ExtResource("e1")]
"#;
    let player_tscn = r#"[gd_scene format=3]

[node name="PlayerRoot" type="CharacterBody2D"]

[node name="PlayerSprite" type="Sprite2D" parent="."]
"#;
    let enemy_tscn = r#"[gd_scene format=3]

[node name="EnemyRoot" type="RigidBody2D"]

[node name="EnemyAI" type="Node" parent="."]
"#;

    let outer = PackedScene::from_tscn(outer_tscn).unwrap();

    let nodes = outer
        .instance_with_subscenes(&|path: &str| -> Option<PackedScene> {
            match path {
                "res://player.tscn" => Some(PackedScene::from_tscn(player_tscn).unwrap()),
                "res://enemy.tscn" => Some(PackedScene::from_tscn(enemy_tscn).unwrap()),
                _ => None,
            }
        })
        .unwrap();

    // World + Player(CharacterBody2D) + PlayerSprite + Enemy(RigidBody2D) + EnemyAI = 5
    assert_eq!(nodes.len(), 5);
    assert_eq!(nodes[0].name(), "World");
    assert_eq!(nodes[1].name(), "Player");
    assert_eq!(nodes[1].class_name(), "CharacterBody2D");
    assert_eq!(nodes[2].name(), "PlayerSprite");
    assert_eq!(nodes[3].name(), "Enemy");
    assert_eq!(nodes[3].class_name(), "RigidBody2D");
    assert_eq!(nodes[4].name(), "EnemyAI");

    // Both instanced roots are children of World.
    assert_eq!(nodes[1].parent(), Some(nodes[0].id()));
    assert_eq!(nodes[3].parent(), Some(nodes[0].id()));
}
