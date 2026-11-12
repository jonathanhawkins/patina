//! pat-j2ed: External resource reference resolution across files.

use gdresource::TresLoader;

#[test]
fn ext_resource_parsed_into_map() {
    let source = r#"[gd_resource type="PackedScene" format=3]

[ext_resource type="Texture2D" path="res://icon.png" id="1"]

[resource]
texture = ExtResource("1")
"#;
    let loader = TresLoader::new();
    let res = loader.parse_str(source, "res://main.tres").unwrap();
    assert!(!res.ext_resources.is_empty(), "must have ext_resources");
    let entry = &res.ext_resources["1"];
    assert_eq!(entry.path, "res://icon.png");
    assert_eq!(entry.resource_type, "Texture2D");
}

#[test]
fn multiple_ext_resources() {
    let source = r#"[gd_resource type="Resource" format=3]

[ext_resource type="Texture2D" path="res://a.png" id="1"]
[ext_resource type="Script" path="res://b.gd" id="2"]

[resource]
tex = ExtResource("1")
script = ExtResource("2")
"#;
    let loader = TresLoader::new();
    let res = loader.parse_str(source, "res://test.tres").unwrap();
    assert_eq!(res.ext_resources.len(), 2);
    assert_eq!(res.ext_resources["1"].path, "res://a.png");
    assert_eq!(res.ext_resources["2"].path, "res://b.gd");
}

#[test]
fn ext_resource_with_uid() {
    let source = r#"[gd_resource type="Resource" format=3]

[ext_resource type="Texture2D" uid="uid://abc123" path="res://icon.png" id="1_tex"]

[resource]
texture = ExtResource("1_tex")
"#;
    let loader = TresLoader::new();
    let res = loader.parse_str(source, "res://test.tres").unwrap();
    assert!(res.ext_resources.contains_key("1_tex"));
    assert_eq!(res.ext_resources["1_tex"].path, "res://icon.png");
}

#[test]
fn no_ext_resources_is_empty_map() {
    let source = r#"[gd_resource type="Resource" format=3]

[resource]
name = "plain"
"#;
    let loader = TresLoader::new();
    let res = loader.parse_str(source, "res://plain.tres").unwrap();
    assert!(res.ext_resources.is_empty());
}
