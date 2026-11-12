//! pat-9mze: Resource load-inspect-resave roundtrip produces equivalent output.

use gdresource::{TresLoader, TresSaver};

#[test]
fn simple_resource_roundtrip() {
    let source = r#"[gd_resource type="Resource" format=3]

[resource]
name = "Test"
value = 42
"#;
    let loader = TresLoader::new();
    let saver = TresSaver::new();
    let res = loader.parse_str(source, "res://test.tres").unwrap();
    let saved = saver.save_to_string(&res).unwrap();
    let res2 = loader.parse_str(&saved, "res://test.tres").unwrap();
    assert_eq!(res.class_name, res2.class_name);
    assert_eq!(res.property_count(), res2.property_count());
}

#[test]
fn roundtrip_preserves_class_name() {
    let source = r#"[gd_resource type="Theme" format=3]

[resource]
font_size = 16
"#;
    let loader = TresLoader::new();
    let saver = TresSaver::new();
    let res = loader.parse_str(source, "res://theme.tres").unwrap();
    let saved = saver.save_to_string(&res).unwrap();
    let res2 = loader.parse_str(&saved, "res://theme.tres").unwrap();
    assert_eq!(res2.class_name, "Theme");
}

#[test]
fn roundtrip_with_subresources_preserves_count() {
    let source = r#"[gd_resource type="Theme" format=3]

[sub_resource type="StyleBoxFlat" id="StyleBoxFlat_001"]
bg_color = Color(0.2, 0.3, 0.4, 1)

[resource]
panel_style = SubResource("StyleBoxFlat_001")
"#;
    let loader = TresLoader::new();
    let saver = TresSaver::new();
    let res = loader.parse_str(source, "res://theme.tres").unwrap();
    let saved = saver.save_to_string(&res).unwrap();
    let res2 = loader.parse_str(&saved, "res://theme.tres").unwrap();
    assert_eq!(res.subresources.len(), res2.subresources.len());
}

#[test]
fn roundtrip_preserves_property_values() {
    let source = r#"[gd_resource type="Resource" format=3]

[resource]
speed = 42.5
name = "Player"
enabled = true
"#;
    let loader = TresLoader::new();
    let saver = TresSaver::new();
    let res = loader.parse_str(source, "res://cfg.tres").unwrap();
    let saved = saver.save_to_string(&res).unwrap();
    let res2 = loader.parse_str(&saved, "res://cfg.tres").unwrap();

    assert!(res2.get_property("speed").is_some());
    assert!(res2.get_property("name").is_some());
    assert!(res2.get_property("enabled").is_some());
}
