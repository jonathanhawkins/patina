//! pat-4cv6: ext-resource UID preservation and subresource edge cases in
//! PackedScene loading.
//!
//! Covers gaps not handled by prior edge-case test files (pat-rsq, pat-c7il,
//! pat-3clf):
//!
//! - ext_resource uid attribute preserved in ExtResourceEntry
//! - resolve_ext_resource_uid() API
//! - ext_resource with uid but varying path/id combinations
//! - SubResource("") empty-string id edge case
//! - SubResource with whitespace in id
//! - ExtResource + SubResource interleaving with uid attributes
//! - instance_with_subscenes where inner scene has uid-bearing ext_resources
//! - ext_resource uid=None when uid attribute is absent
//! - Connection with flags attribute from uid-bearing scenes
//!
//! Acceptance: focused tests verify UID preservation and remaining edge cases.

use gdscene::packed_scene::PackedScene;
use gdvariant::Variant;

// ===========================================================================
// 1. ext_resource uid is preserved in ExtResourceEntry
// ===========================================================================

#[test]
fn ext_resource_uid_stored_in_entry() {
    let tscn = r#"[gd_scene format=3]

[ext_resource type="Script" uid="uid://abc123" path="res://player.gd" id="s1"]

[node name="Root" type="Node2D"]
"#;
    let scene = PackedScene::from_tscn(tscn).unwrap();
    let ext = scene.ext_resources();

    assert_eq!(ext.len(), 1);
    let entry = &ext["s1"];
    assert_eq!(entry.res_type, "Script");
    assert_eq!(entry.path, "res://player.gd");
    assert_eq!(entry.uid.as_deref(), Some("uid://abc123"));
}

#[test]
fn ext_resource_without_uid_has_none() {
    let tscn = r#"[gd_scene format=3]

[ext_resource type="Texture2D" path="res://icon.png" id="t1"]

[node name="Root" type="Sprite2D"]
"#;
    let scene = PackedScene::from_tscn(tscn).unwrap();
    let entry = &scene.ext_resources()["t1"];
    assert_eq!(entry.uid, None);
}

#[test]
fn multiple_ext_resources_mixed_uid_presence() {
    let tscn = r#"[gd_scene format=3]

[ext_resource type="Script" uid="uid://scr_uid" path="res://a.gd" id="s1"]
[ext_resource type="Texture2D" path="res://b.png" id="t1"]
[ext_resource type="PackedScene" uid="uid://scene_uid" path="res://c.tscn" id="ps1"]

[node name="Root" type="Node2D"]
"#;
    let scene = PackedScene::from_tscn(tscn).unwrap();
    let ext = scene.ext_resources();

    assert_eq!(ext["s1"].uid.as_deref(), Some("uid://scr_uid"));
    assert_eq!(ext["t1"].uid, None);
    assert_eq!(ext["ps1"].uid.as_deref(), Some("uid://scene_uid"));
}

// ===========================================================================
// 2. resolve_ext_resource_uid() API
// ===========================================================================

#[test]
fn resolve_ext_resource_uid_returns_uid() {
    let tscn = r#"[gd_scene format=3]

[ext_resource type="Script" uid="uid://player_scr" path="res://player.gd" id="s1"]

[node name="Root" type="Node2D"]
"#;
    let scene = PackedScene::from_tscn(tscn).unwrap();
    assert_eq!(
        scene.resolve_ext_resource_uid(r#"ExtResource("s1")"#),
        Some("uid://player_scr")
    );
}

#[test]
fn resolve_ext_resource_uid_returns_none_when_no_uid() {
    let tscn = r#"[gd_scene format=3]

[ext_resource type="Script" path="res://player.gd" id="s1"]

[node name="Root" type="Node2D"]
"#;
    let scene = PackedScene::from_tscn(tscn).unwrap();
    assert_eq!(
        scene.resolve_ext_resource_uid(r#"ExtResource("s1")"#),
        None
    );
}

#[test]
fn resolve_ext_resource_uid_missing_id_returns_none() {
    let tscn = r#"[gd_scene format=3]

[ext_resource type="Script" uid="uid://x" path="res://a.gd" id="s1"]

[node name="Root" type="Node2D"]
"#;
    let scene = PackedScene::from_tscn(tscn).unwrap();
    assert_eq!(
        scene.resolve_ext_resource_uid(r#"ExtResource("nonexistent")"#),
        None
    );
}

#[test]
fn resolve_ext_resource_uid_malformed_ref_returns_none() {
    let tscn = r#"[gd_scene format=3]

[ext_resource type="Script" uid="uid://x" path="res://a.gd" id="s1"]

[node name="Root" type="Node2D"]
"#;
    let scene = PackedScene::from_tscn(tscn).unwrap();
    assert_eq!(scene.resolve_ext_resource_uid("s1"), None);
    assert_eq!(scene.resolve_ext_resource_uid(""), None);
}

// ===========================================================================
// 3. SubResource edge cases in variant parsing
// ===========================================================================

#[test]
fn sub_resource_empty_string_id_stored() {
    // SubResource("") — parser converts to "SubResource:" with empty suffix.
    let tscn = r#"[gd_scene format=3]

[node name="Root" type="CollisionShape2D"]
shape = SubResource("")
"#;
    let scene = PackedScene::from_tscn(tscn).unwrap();
    let nodes = scene.instance().unwrap();

    // Empty SubResource ID is still stored as the prefixed string.
    assert_eq!(
        nodes[0].get_property("shape"),
        Variant::String("SubResource:".into())
    );
}

#[test]
fn sub_resource_with_underscore_and_digits_in_id() {
    let tscn = r#"[gd_scene format=3]

[sub_resource type="RectangleShape2D" id="RectangleShape2D_abc123"]
size = Vector2(32, 32)

[node name="Root" type="CollisionShape2D"]
shape = SubResource("RectangleShape2D_abc123")
"#;
    let scene = PackedScene::from_tscn(tscn).unwrap();
    let nodes = scene.instance().unwrap();

    assert_eq!(
        nodes[0].get_property("shape"),
        Variant::String("SubResource:RectangleShape2D_abc123".into())
    );
}

// ===========================================================================
// 4. ext_resource uid with sub-scene instancing
// ===========================================================================

#[test]
fn instance_with_subscenes_inner_ext_resources_have_uid() {
    let outer_tscn = r#"[gd_scene format=3 uid="uid://outer_scene"]

[ext_resource type="PackedScene" uid="uid://inner_ref" path="res://inner.tscn" id="inner"]

[node name="Root" type="Node2D"]

[node name="Child" type="Node2D" parent="." instance=ExtResource("inner")]
"#;
    let inner_tscn = r#"[gd_scene format=3 uid="uid://inner_scene"]

[ext_resource type="Script" uid="uid://inner_script" path="res://logic.gd" id="s1"]

[node name="InnerRoot" type="Node2D"]
script = ExtResource("s1")
"#;

    let outer = PackedScene::from_tscn(outer_tscn).unwrap();

    // Verify outer scene's ext_resource uid is preserved.
    assert_eq!(
        outer.ext_resources()["inner"].uid.as_deref(),
        Some("uid://inner_ref")
    );

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
    assert_eq!(nodes[1].name(), "Child");
    // Inner script resolved from inner ext_resources.
    assert_eq!(
        nodes[1].get_property("_script_path"),
        Variant::String("res://logic.gd".into())
    );
}

#[test]
fn inner_scene_ext_resource_uid_accessible() {
    // Verify the inner PackedScene preserves its own ext_resource UIDs.
    let inner_tscn = r#"[gd_scene format=3]

[ext_resource type="Script" uid="uid://my_script_uid" path="res://script.gd" id="s1"]
[ext_resource type="Texture2D" path="res://tex.png" id="t1"]

[node name="Root" type="Node2D"]
script = ExtResource("s1")
"#;
    let inner = PackedScene::from_tscn(inner_tscn).unwrap();
    assert_eq!(
        inner.resolve_ext_resource_uid(r#"ExtResource("s1")"#),
        Some("uid://my_script_uid")
    );
    assert_eq!(
        inner.resolve_ext_resource_uid(r#"ExtResource("t1")"#),
        None
    );
}

// ===========================================================================
// 5. Scene with both scene-level and ext_resource-level UIDs
// ===========================================================================

#[test]
fn scene_uid_and_ext_resource_uid_independent() {
    let tscn = r#"[gd_scene format=3 uid="uid://scene_level"]

[ext_resource type="Script" uid="uid://ext_level" path="res://s.gd" id="s1"]

[node name="Root" type="Node2D"]
script = ExtResource("s1")
"#;
    let scene = PackedScene::from_tscn(tscn).unwrap();

    // Scene-level uid.
    assert_eq!(scene.uid.as_deref(), Some("uid://scene_level"));
    // ext_resource-level uid.
    assert_eq!(
        scene.ext_resources()["s1"].uid.as_deref(),
        Some("uid://ext_level")
    );
    // They are independent.
    assert_ne!(scene.uid.as_deref(), scene.ext_resources()["s1"].uid.as_deref());
}

// ===========================================================================
// 6. Duplicate ext_resource IDs with different UIDs — last wins
// ===========================================================================

#[test]
fn duplicate_ext_resource_ids_uid_last_wins() {
    let tscn = r#"[gd_scene format=3]

[ext_resource type="Script" uid="uid://first" path="res://first.gd" id="dup"]
[ext_resource type="Script" uid="uid://second" path="res://second.gd" id="dup"]

[node name="Root" type="Node2D"]
script = ExtResource("dup")
"#;
    let scene = PackedScene::from_tscn(tscn).unwrap();
    let entry = &scene.ext_resources()["dup"];

    assert_eq!(entry.path, "res://second.gd");
    assert_eq!(entry.uid.as_deref(), Some("uid://second"));
}

// ===========================================================================
// 7. ext_resource interleaved with sub_resource — all UIDs preserved
// ===========================================================================

#[test]
fn ext_resources_interleaved_with_sub_resources_preserve_uid() {
    let tscn = r#"[gd_scene format=3]

[ext_resource type="Script" uid="uid://scr1" path="res://main.gd" id="s1"]

[sub_resource type="RectangleShape2D" id="rect1"]
size = Vector2(10, 10)

[ext_resource type="Texture2D" uid="uid://tex1" path="res://bg.png" id="t1"]

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

    assert_eq!(ext.len(), 2);
    assert_eq!(ext["s1"].uid.as_deref(), Some("uid://scr1"));
    assert_eq!(ext["t1"].uid.as_deref(), Some("uid://tex1"));

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
// 8. Connection with flags from uid-bearing scene
// ===========================================================================

#[test]
fn connection_flags_in_uid_scene() {
    let tscn = r#"[gd_scene format=3 uid="uid://flagged_scene"]

[ext_resource type="Script" uid="uid://btn_scr" path="res://button.gd" id="s1"]

[node name="Root" type="Control"]
script = ExtResource("s1")

[node name="Btn" type="Button" parent="."]

[connection signal="pressed" from="Btn" to="." method="_on_pressed" flags=3]
"#;
    let scene = PackedScene::from_tscn(tscn).unwrap();

    assert_eq!(scene.uid.as_deref(), Some("uid://flagged_scene"));
    assert_eq!(scene.connection_count(), 1);

    let conn = &scene.connections()[0];
    assert_eq!(conn.signal_name, "pressed");
    assert_eq!(conn.flags, 3);

    // ext_resource uid accessible.
    assert_eq!(
        scene.resolve_ext_resource_uid(r#"ExtResource("s1")"#),
        Some("uid://btn_scr")
    );
}

// ===========================================================================
// 9. Stress: many ext_resources with UIDs
// ===========================================================================

#[test]
fn stress_many_ext_resources_with_uids() {
    let mut tscn = String::from("[gd_scene format=3]\n\n");
    for i in 0..30 {
        tscn.push_str(&format!(
            "[ext_resource type=\"Script\" uid=\"uid://uid_{i}\" path=\"res://s{i}.gd\" id=\"s{i}\"]\n"
        ));
    }
    tscn.push_str("\n[node name=\"Root\" type=\"Node2D\"]\n");

    let scene = PackedScene::from_tscn(&tscn).unwrap();
    assert_eq!(scene.ext_resources().len(), 30);

    for i in 0..30 {
        let ext_ref = format!(r#"ExtResource("s{i}")"#);
        assert_eq!(
            scene.resolve_ext_resource_uid(&ext_ref),
            Some(format!("uid://uid_{i}").as_str()),
            "uid for s{i} should be uid://uid_{i}"
        );
        assert_eq!(
            scene.resolve_ext_resource_path(&ext_ref),
            Some(format!("res://s{i}.gd").as_str()),
            "path for s{i} should be res://s{i}.gd"
        );
    }
}

// ===========================================================================
// 10. SubResource and ExtResource on same node — both resolved correctly
//     with uid on the ext_resource
// ===========================================================================

#[test]
fn mixed_sub_ext_with_uid_on_same_node() {
    let tscn = r#"[gd_scene format=3]

[ext_resource type="Script" uid="uid://ctrl_scr" path="res://ctrl.gd" id="s1"]

[sub_resource type="StyleBoxFlat" id="style1"]
bg_color = Color(1, 0, 0, 1)

[node name="Root" type="Panel"]
script = ExtResource("s1")
normal_style = SubResource("style1")
custom_val = 42
"#;
    let scene = PackedScene::from_tscn(tscn).unwrap();
    let nodes = scene.instance().unwrap();

    assert_eq!(
        nodes[0].get_property("_script_path"),
        Variant::String("res://ctrl.gd".into())
    );
    assert_eq!(
        nodes[0].get_property("normal_style"),
        Variant::String("SubResource:style1".into())
    );
    assert_eq!(nodes[0].get_property("custom_val"), Variant::Int(42));

    // UID is on the ext_resource entry.
    assert_eq!(
        scene.resolve_ext_resource_uid(r#"ExtResource("s1")"#),
        Some("uid://ctrl_scr")
    );
}

// ===========================================================================
// 11. ext_resource uid with special characters
// ===========================================================================

#[test]
fn ext_resource_uid_with_long_hash() {
    let tscn = r#"[gd_scene format=3]

[ext_resource type="Script" uid="uid://cw8f7xqkj2h5r" path="res://player.gd" id="1_abc"]

[node name="Root" type="Node2D"]
script = ExtResource("1_abc")
"#;
    let scene = PackedScene::from_tscn(tscn).unwrap();
    assert_eq!(
        scene.ext_resources()["1_abc"].uid.as_deref(),
        Some("uid://cw8f7xqkj2h5r")
    );
    assert_eq!(
        scene.resolve_ext_resource_uid(r#"ExtResource("1_abc")"#),
        Some("uid://cw8f7xqkj2h5r")
    );
}

// ===========================================================================
// 12. Instance fallthrough preserves parent uid context
// ===========================================================================

#[test]
fn instance_fallthrough_with_uid_ext_resource() {
    let tscn = r#"[gd_scene format=3]

[ext_resource type="PackedScene" uid="uid://missing_scene" path="res://missing.tscn" id="ms"]

[node name="Root" type="Node2D"]

[node name="Child" type="Node2D" parent="." instance=ExtResource("ms")]
"#;
    let scene = PackedScene::from_tscn(tscn).unwrap();

    // UID is accessible even if the scene can't be resolved for instancing.
    assert_eq!(
        scene.resolve_ext_resource_uid(r#"ExtResource("ms")"#),
        Some("uid://missing_scene")
    );

    // instance_with_subscenes falls through gracefully.
    let nodes = scene
        .instance_with_subscenes(&|_path: &str| -> Option<PackedScene> { None })
        .unwrap();

    assert_eq!(nodes.len(), 2);
    assert_eq!(nodes[1].name(), "Child");
    assert_eq!(
        nodes[1].get_property("_instance"),
        Variant::String(r#"ExtResource("ms")"#.into())
    );
}
