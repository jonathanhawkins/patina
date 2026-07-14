//! pat-1zi: Sub-resource inline loading in .tres files.
//!
//! Validates that TresLoader correctly parses [sub_resource] sections,
//! stores them in the resource's subresources map, and resolves property
//! references via SubResource("id") syntax.

use gdresource::TresLoader;

#[test]
fn subresource_inline_loading_basic() {
    let source = r#"[gd_resource type="Theme" format=3]

[sub_resource type="StyleBoxFlat" id="StyleBoxFlat_abc"]
bg_color = Color(0.2, 0.3, 0.4, 1)
corner_radius_top_left = 4

[sub_resource type="StyleBoxFlat" id="StyleBoxFlat_def"]
bg_color = Color(0.5, 0.6, 0.7, 1)

[resource]
panel_style = SubResource("StyleBoxFlat_abc")
button_style = SubResource("StyleBoxFlat_def")
"#;

    let loader = TresLoader::new();
    let res = loader.parse_str(source, "res://theme.tres").unwrap();

    assert_eq!(res.subresources.len(), 2, "must have 2 sub-resources");

    // Property references must resolve to sub-resources.
    let panel = res.resolve_subresource("panel_style");
    assert!(
        panel.is_some(),
        "resolve_subresource('panel_style') must find the sub-resource"
    );
    let panel = panel.unwrap();
    assert_eq!(panel.class_name, "StyleBoxFlat");

    // Nested property must be accessible.
    let bg = panel.get_property("bg_color");
    assert!(bg.is_some(), "sub-resource must have bg_color property");
}

#[test]
fn subresource_button_style_resolves() {
    let source = r#"[gd_resource type="Theme" format=3]

[sub_resource type="StyleBoxFlat" id="StyleBoxFlat_abc"]
bg_color = Color(0.2, 0.3, 0.4, 1)

[sub_resource type="StyleBoxFlat" id="StyleBoxFlat_def"]
bg_color = Color(0.5, 0.6, 0.7, 1)

[resource]
panel_style = SubResource("StyleBoxFlat_abc")
button_style = SubResource("StyleBoxFlat_def")
"#;

    let loader = TresLoader::new();
    let res = loader.parse_str(source, "res://theme.tres").unwrap();

    let button = res.resolve_subresource("button_style");
    assert!(button.is_some(), "button_style must resolve");
    let button = button.unwrap();
    assert_eq!(button.class_name, "StyleBoxFlat");

    let bg = button.get_property("bg_color");
    assert!(bg.is_some(), "button sub-resource must have bg_color");
}

#[test]
fn subresource_property_values_preserved() {
    let source = r#"[gd_resource type="Resource" format=3]

[sub_resource type="StyleBoxFlat" id="sb1"]
corner_radius_top_left = 8
corner_radius_top_right = 12

[resource]
style = SubResource("sb1")
"#;

    let loader = TresLoader::new();
    let res = loader.parse_str(source, "res://test.tres").unwrap();

    let sub = res.resolve_subresource("style").unwrap();
    let tl = sub.get_property("corner_radius_top_left");
    assert!(tl.is_some(), "corner_radius_top_left must be set");

    let tr = sub.get_property("corner_radius_top_right");
    assert!(tr.is_some(), "corner_radius_top_right must be set");
}

#[test]
fn subresource_unresolvable_property_returns_none() {
    let source = r#"[gd_resource type="Resource" format=3]

[sub_resource type="StyleBoxFlat" id="sb1"]
bg_color = Color(1, 0, 0, 1)

[resource]
style = SubResource("sb1")
"#;

    let loader = TresLoader::new();
    let res = loader.parse_str(source, "res://test.tres").unwrap();

    // "nonexistent" is not a property that references a sub-resource
    assert!(
        res.resolve_subresource("nonexistent").is_none(),
        "non-existent property should return None"
    );
}

#[test]
fn subresource_direct_access_by_id() {
    let source = r#"[gd_resource type="Resource" format=3]

[sub_resource type="GradientTexture2D" id="grad_1"]
fill_from = Vector2(0, 0)
fill_to = Vector2(1, 1)

[resource]
texture = SubResource("grad_1")
"#;

    let loader = TresLoader::new();
    let res = loader.parse_str(source, "res://test.tres").unwrap();

    // Direct access to the subresources map
    assert!(res.subresources.contains_key("grad_1"));
    let grad = &res.subresources["grad_1"];
    assert_eq!(grad.class_name, "GradientTexture2D");
}

#[test]
fn subresource_multiple_types_mixed() {
    let source = r#"[gd_resource type="Theme" format=3]

[sub_resource type="StyleBoxFlat" id="sb_flat"]
bg_color = Color(1, 0, 0, 1)

[sub_resource type="Font" id="font_1"]
size = 16

[resource]
style = SubResource("sb_flat")
font = SubResource("font_1")
"#;

    let loader = TresLoader::new();
    let res = loader.parse_str(source, "res://theme.tres").unwrap();

    assert_eq!(res.subresources.len(), 2);

    let style = res.resolve_subresource("style").unwrap();
    assert_eq!(style.class_name, "StyleBoxFlat");

    let font = res.resolve_subresource("font").unwrap();
    assert_eq!(font.class_name, "Font");
}
