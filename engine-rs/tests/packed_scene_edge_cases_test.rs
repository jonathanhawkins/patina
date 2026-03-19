//! PackedScene edge case tests (pat-rsq).
//!
//! Tests ext_resource resolution, sub_resource handling, missing resources,
//! duplicate IDs, empty files, and header-only scenes.

use gdscene::packed_scene::PackedScene;

// ===========================================================================
// 1. ext_resource with uid:// reference resolves correctly
// ===========================================================================

#[test]
fn ext_resource_with_uid_parses() {
    let tscn = r#"[gd_scene format=3 uid="uid://scene_with_uid_ext"]

[ext_resource type="Script" uid="uid://player_script" path="res://scripts/player.gd" id="1_scr"]

[node name="Root" type="Node2D"]

[node name="Player" type="Node2D" parent="."]
script = ExtResource("1_scr")
"#;
    let scene = PackedScene::from_tscn(tscn).unwrap();
    assert_eq!(scene.node_count(), 2);

    let nodes = scene.instance().unwrap();
    // Player node should have script_path resolved from ext_resource.
    let player = &nodes[1];
    assert_eq!(player.name(), "Player");
    assert_eq!(
        player.get_property("_script_path"),
        gdvariant::Variant::String("res://scripts/player.gd".into())
    );
}

#[test]
fn ext_resource_uid_preserved_in_parse() {
    let tscn = r#"[gd_scene format=3 uid="uid://ext_uid_test"]

[ext_resource type="Texture2D" uid="uid://icon_texture" path="res://icon.png" id="tex_1"]

[node name="Root" type="Sprite2D"]
texture = ExtResource("tex_1")
"#;
    let scene = PackedScene::from_tscn(tscn).unwrap();
    assert_eq!(scene.uid.as_deref(), Some("uid://ext_uid_test"));

    let nodes = scene.instance().unwrap();
    // Texture property stored as raw ExtResource reference string.
    let root = &nodes[0];
    assert_eq!(
        root.get_property("texture"),
        gdvariant::Variant::String("ExtResource(\"tex_1\")".into())
    );
}

#[test]
fn multiple_ext_resources_with_uids() {
    let tscn = r#"[gd_scene format=3]

[ext_resource type="Script" uid="uid://script_a" path="res://a.gd" id="s1"]
[ext_resource type="Script" uid="uid://script_b" path="res://b.gd" id="s2"]
[ext_resource type="Texture2D" uid="uid://tex_c" path="res://c.png" id="t1"]

[node name="Root" type="Node2D"]
script = ExtResource("s1")

[node name="Child" type="Node2D" parent="."]
script = ExtResource("s2")
"#;
    let scene = PackedScene::from_tscn(tscn).unwrap();
    assert_eq!(scene.node_count(), 2);

    let nodes = scene.instance().unwrap();
    assert_eq!(
        nodes[0].get_property("_script_path"),
        gdvariant::Variant::String("res://a.gd".into())
    );
    assert_eq!(
        nodes[1].get_property("_script_path"),
        gdvariant::Variant::String("res://b.gd".into())
    );
}

// ===========================================================================
// 2. sub_resource sections — currently skipped by simplified parser
// ===========================================================================

#[test]
fn sub_resource_sections_do_not_crash() {
    let tscn = r#"[gd_scene format=3]

[sub_resource type="RectangleShape2D" id="shape_1"]
size = Vector2(64, 64)

[sub_resource type="CircleShape2D" id="shape_2"]
radius = 32.0

[node name="Root" type="Node2D"]

[node name="Body" type="StaticBody2D" parent="."]
"#;
    // Sub-resources are ignored by the simplified parser but should not cause errors.
    let scene = PackedScene::from_tscn(tscn).unwrap();
    assert_eq!(scene.node_count(), 2);
}

#[test]
fn sub_resource_ref_in_property_stored_as_string() {
    let tscn = r#"[gd_scene format=3]

[sub_resource type="RectangleShape2D" id="shape_1"]
size = Vector2(64, 64)

[node name="Root" type="CollisionShape2D"]
shape = SubResource("shape_1")
"#;
    let scene = PackedScene::from_tscn(tscn).unwrap();
    let nodes = scene.instance().unwrap();

    // SubResource references are stored as Variant::String via parse_variant_value.
    let root = &nodes[0];
    assert_eq!(
        root.get_property("shape"),
        gdvariant::Variant::String("SubResource:shape_1".into())
    );
}

// ===========================================================================
// 3. Nested resource references
// ===========================================================================

#[test]
fn nested_ext_resource_in_array_property() {
    let tscn = r#"[gd_scene format=3]

[ext_resource type="Texture2D" path="res://a.png" id="t1"]
[ext_resource type="Texture2D" path="res://b.png" id="t2"]

[node name="Root" type="AnimatedSprite2D"]
frames = [ExtResource("t1"), ExtResource("t2")]
"#;
    let scene = PackedScene::from_tscn(tscn).unwrap();
    let nodes = scene.instance().unwrap();

    // The array property should be parsed as a Variant::Array.
    match nodes[0].get_property("frames") {
        gdvariant::Variant::Array(items) => {
            assert_eq!(items.len(), 2);
        }
        other => panic!("expected Array, got {:?}", other),
    }
}

// ===========================================================================
// 4. Missing ext_resource file → graceful handling
// ===========================================================================

#[test]
fn script_ext_resource_id_not_found_graceful() {
    let tscn = r#"[gd_scene format=3]

[node name="Root" type="Node2D"]
script = ExtResource("nonexistent_id")
"#;
    // The ext_resource entry doesn't exist, so script_path won't be set.
    // But parsing should NOT panic.
    let scene = PackedScene::from_tscn(tscn).unwrap();
    let nodes = scene.instance().unwrap();

    // No _script_path property since the ext_resource wasn't found.
    assert_eq!(
        nodes[0].get_property("_script_path"),
        gdvariant::Variant::Nil
    );
    // But the raw ExtResource reference is stored.
    assert_eq!(
        nodes[0].get_property("script"),
        gdvariant::Variant::String("ExtResource(\"nonexistent_id\")".into())
    );
}

#[test]
fn ext_resource_without_path_attribute() {
    let tscn = r#"[gd_scene format=3]

[ext_resource type="Script" id="no_path"]

[node name="Root" type="Node2D"]
script = ExtResource("no_path")
"#;
    // ext_resource has no path= attribute. The parser requires both id and path
    // to register an ext_resource, so "no_path" won't be in the map.
    let scene = PackedScene::from_tscn(tscn).unwrap();
    let nodes = scene.instance().unwrap();

    // No _script_path since the ext_resource entry wasn't registered.
    assert_eq!(
        nodes[0].get_property("_script_path"),
        gdvariant::Variant::Nil
    );
}

// ===========================================================================
// 5. Duplicate resource IDs
// ===========================================================================

#[test]
fn duplicate_ext_resource_ids_last_wins() {
    let tscn = r#"[gd_scene format=3]

[ext_resource type="Script" path="res://first.gd" id="dup"]
[ext_resource type="Script" path="res://second.gd" id="dup"]

[node name="Root" type="Node2D"]
script = ExtResource("dup")
"#;
    let scene = PackedScene::from_tscn(tscn).unwrap();
    let nodes = scene.instance().unwrap();

    // HashMap insert semantics: last write wins.
    assert_eq!(
        nodes[0].get_property("_script_path"),
        gdvariant::Variant::String("res://second.gd".into())
    );
}

#[test]
fn duplicate_node_names_under_different_parents() {
    let tscn = r#"[gd_scene format=3]

[node name="Root" type="Node2D"]

[node name="Child" type="Node2D" parent="."]

[node name="Child" type="Sprite2D" parent="Child"]
"#;
    // Two nodes named "Child" — one under root, one under the first Child.
    let scene = PackedScene::from_tscn(tscn).unwrap();
    assert_eq!(scene.node_count(), 3);

    let nodes = scene.instance().unwrap();
    assert_eq!(nodes[0].name(), "Root");
    assert_eq!(nodes[1].name(), "Child");
    assert_eq!(nodes[1].class_name(), "Node2D");
    assert_eq!(nodes[2].name(), "Child");
    assert_eq!(nodes[2].class_name(), "Sprite2D");
    // Nested Child's parent is the first Child.
    assert_eq!(nodes[2].parent(), Some(nodes[1].id()));
}

// ===========================================================================
// 6. Empty .tscn file → clean error
// ===========================================================================

#[test]
fn empty_file_returns_error() {
    let result = PackedScene::from_tscn("");
    assert!(result.is_err());
}

#[test]
fn whitespace_only_returns_error() {
    let result = PackedScene::from_tscn("   \n\n  \n  ");
    assert!(result.is_err());
}

#[test]
fn comments_only_returns_error() {
    let tscn = r#"; This is a comment
; Another comment
"#;
    let result = PackedScene::from_tscn(tscn);
    assert!(result.is_err());
}

#[test]
fn header_only_no_nodes_returns_error() {
    let tscn = r#"[gd_scene format=3 uid="uid://header_only"]
"#;
    let result = PackedScene::from_tscn(tscn);
    assert!(
        result.is_err(),
        "scene with header but no nodes should error"
    );
}

// ===========================================================================
// 7. .tscn with only header and root node → valid scene
// ===========================================================================

#[test]
fn header_and_single_root_node_valid() {
    let tscn = r#"[gd_scene format=3 uid="uid://minimal"]

[node name="Root" type="Node"]
"#;
    let scene = PackedScene::from_tscn(tscn).unwrap();
    assert_eq!(scene.node_count(), 1);
    assert_eq!(scene.uid.as_deref(), Some("uid://minimal"));

    let nodes = scene.instance().unwrap();
    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes[0].name(), "Root");
    assert_eq!(nodes[0].class_name(), "Node");
    assert!(nodes[0].children().is_empty());
}

#[test]
fn header_no_uid_valid() {
    let tscn = r#"[gd_scene format=3]

[node name="Root" type="Node2D"]
"#;
    let scene = PackedScene::from_tscn(tscn).unwrap();
    assert_eq!(scene.uid, None);
    assert_eq!(scene.node_count(), 1);
}

#[test]
fn root_with_properties_no_children() {
    let tscn = r#"[gd_scene format=3]

[node name="Canvas" type="Node2D"]
position = Vector2(50, 100)
visible = true
"#;
    let scene = PackedScene::from_tscn(tscn).unwrap();
    let nodes = scene.instance().unwrap();
    assert_eq!(nodes.len(), 1);
    assert_eq!(
        nodes[0].get_property("position"),
        gdvariant::Variant::Vector2(gdcore::math::Vector2::new(50.0, 100.0))
    );
    assert_eq!(
        nodes[0].get_property("visible"),
        gdvariant::Variant::Bool(true)
    );
}

// ===========================================================================
// 8. Real fixture .tscn files — parse without errors
// ===========================================================================

fn fixtures_dir() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures/scenes")
}

macro_rules! fixture_parse_test {
    ($test_name:ident, $file:expr) => {
        #[test]
        fn $test_name() {
            let path = fixtures_dir().join($file);
            let content = std::fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));
            let scene = PackedScene::from_tscn(&content)
                .unwrap_or_else(|e| panic!("failed to parse {}: {e}", $file));
            assert!(
                scene.node_count() >= 1,
                "{} should have at least 1 node",
                $file
            );
            // Instance should also succeed.
            let nodes = scene
                .instance()
                .unwrap_or_else(|e| panic!("failed to instance {}: {e}", $file));
            assert_eq!(nodes.len(), scene.node_count());
        }
    };
}

fixture_parse_test!(fixture_hierarchy, "hierarchy.tscn");
fixture_parse_test!(fixture_test_scripts, "test_scripts.tscn");
fixture_parse_test!(fixture_space_shooter, "space_shooter.tscn");

// ===========================================================================
// 9. Malformed inputs — no panics
// ===========================================================================

#[test]
fn malformed_node_no_name_uses_default() {
    let tscn = r#"[gd_scene format=3]

[node type="Node2D"]
"#;
    let scene = PackedScene::from_tscn(tscn).unwrap();
    let nodes = scene.instance().unwrap();
    // Name defaults to empty string.
    assert_eq!(nodes[0].name(), "");
}

#[test]
fn malformed_node_no_type_defaults_to_node() {
    let tscn = r#"[gd_scene format=3]

[node name="Root"]
"#;
    let scene = PackedScene::from_tscn(tscn).unwrap();
    let nodes = scene.instance().unwrap();
    assert_eq!(nodes[0].class_name(), "Node");
}

#[test]
fn unparseable_property_value_skipped() {
    let tscn = r#"[gd_scene format=3]

[node name="Root" type="Node2D"]
good_prop = 42
bad_prop = totally_invalid_value
another_good = true
"#;
    let scene = PackedScene::from_tscn(tscn).unwrap();
    let nodes = scene.instance().unwrap();
    assert_eq!(
        nodes[0].get_property("good_prop"),
        gdvariant::Variant::Int(42)
    );
    assert_eq!(
        nodes[0].get_property("another_good"),
        gdvariant::Variant::Bool(true)
    );
    // bad_prop should be skipped, not present.
    assert_eq!(nodes[0].get_property("bad_prop"), gdvariant::Variant::Nil);
}

#[test]
fn connection_missing_required_attrs_skipped() {
    let tscn = r#"[gd_scene format=3]

[node name="Root" type="Node"]

[connection signal="pressed" from="Button"]
"#;
    // Missing "to" and "method" — connection should be skipped, not error.
    let scene = PackedScene::from_tscn(tscn).unwrap();
    assert_eq!(scene.connection_count(), 0);
}
