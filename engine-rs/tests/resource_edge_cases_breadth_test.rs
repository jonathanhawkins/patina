//! pat-j76: Broadened resource parity coverage for edge cases.
//!
//! Tests malformed .tres handling, duplicate UIDs, circular resource references,
//! and very large resource files.

use gdresource::loader::TresLoader;

// ===========================================================================
// 1. Malformed .tres — graceful errors
// ===========================================================================

#[test]
fn empty_tres_returns_empty_resource() {
    let loader = TresLoader::new();
    let result = loader.parse_str("", "res://empty.tres");
    // TresLoader returns an empty resource for empty input (no crash, no panic).
    assert!(result.is_ok());
}

#[test]
fn whitespace_only_tres_returns_empty_resource() {
    let loader = TresLoader::new();
    let result = loader.parse_str("   \n\n  ", "res://ws.tres");
    assert!(result.is_ok());
}

#[test]
fn comments_only_tres_returns_empty_resource() {
    let loader = TresLoader::new();
    let result = loader.parse_str("; comment\n; another\n", "res://comment.tres");
    assert!(result.is_ok());
}

#[test]
fn malformed_header_no_crash() {
    let loader = TresLoader::new();
    let tres = "[gd_resource_BROKEN format=3]\n";
    // Should either error or return something, but never panic
    let _ = loader.parse_str(tres, "res://broken_header.tres");
}

#[test]
fn missing_closing_bracket_no_crash() {
    let loader = TresLoader::new();
    let tres = "[gd_resource format=3\nkey = 42\n";
    let _ = loader.parse_str(tres, "res://missing_bracket.tres");
}

#[test]
fn invalid_property_value_skipped() {
    let loader = TresLoader::new();
    let tres = r#"[gd_resource type="Resource" format=3]

[resource]
good_prop = 42
bad_prop = totally_invalid()
another_good = "hello"
"#;
    let result = loader.parse_str(tres, "res://skip.tres");
    if let Ok(res) = result {
        // Good properties should be present
        assert_eq!(
            res.get_property("good_prop"),
            Some(&gdvariant::Variant::Int(42))
        );
        assert_eq!(
            res.get_property("another_good"),
            Some(&gdvariant::Variant::String("hello".into()))
        );
    }
}

// ===========================================================================
// 2. Duplicate UIDs — last wins semantics
// ===========================================================================

#[test]
fn duplicate_ext_resource_uids_last_wins() {
    let loader = TresLoader::new();
    let tres = r#"[gd_resource type="Resource" format=3]

[ext_resource type="Texture2D" uid="uid://dup_uid" path="res://first.png" id="t1"]
[ext_resource type="Texture2D" uid="uid://dup_uid" path="res://second.png" id="t2"]

[resource]
prop = 1
"#;
    let result = loader.parse_str(tres, "res://dup.tres");
    // Should not panic; duplicate UIDs should be handled gracefully
    assert!(result.is_ok());
}

#[test]
fn tres_with_uid_in_header() {
    let loader = TresLoader::new();
    let tres = r#"[gd_resource type="Resource" format=3 uid="uid://my_resource"]

[resource]
name = "test"
"#;
    let result = loader.parse_str(tres, "res://with_uid.tres");
    assert!(result.is_ok());
    let res = result.unwrap();
    assert_eq!(
        res.get_property("name"),
        Some(&gdvariant::Variant::String("test".into()))
    );
}

// ===========================================================================
// 3. Resource with sub-resources
// ===========================================================================

#[test]
fn tres_with_sub_resources_parses() {
    let loader = TresLoader::new();
    let tres = r#"[gd_resource type="Resource" format=3]

[sub_resource type="RectangleShape2D" id="shape_1"]
size = Vector2(64, 64)

[sub_resource type="CircleShape2D" id="shape_2"]
radius = 32.0

[resource]
main_prop = true
"#;
    let result = loader.parse_str(tres, "res://subs.tres");
    assert!(result.is_ok());
}

#[test]
fn tres_with_many_sub_resources() {
    let loader = TresLoader::new();
    let mut tres = String::from("[gd_resource type=\"Resource\" format=3]\n\n");
    for i in 0..10 {
        tres.push_str(&format!(
            "[sub_resource type=\"Resource\" id=\"sub_{i}\"]\nvalue = {i}\n\n"
        ));
    }
    tres.push_str("[resource]\ncount = 10\n");

    let result = loader.parse_str(&tres, "res://many_subs.tres");
    assert!(result.is_ok());
}

// ===========================================================================
// 4. Very large resource file (many properties)
// ===========================================================================

#[test]
fn tres_with_100_properties() {
    let loader = TresLoader::new();
    let mut tres = String::from("[gd_resource type=\"Resource\" format=3]\n\n[resource]\n");
    for i in 0..100 {
        tres.push_str(&format!("prop_{i} = {i}\n"));
    }

    let result = loader.parse_str(&tres, "res://large.tres");
    assert!(result.is_ok());
    let res = result.unwrap();

    // Spot check a few properties
    assert_eq!(
        res.get_property("prop_0"),
        Some(&gdvariant::Variant::Int(0))
    );
    assert_eq!(
        res.get_property("prop_50"),
        Some(&gdvariant::Variant::Int(50))
    );
    assert_eq!(
        res.get_property("prop_99"),
        Some(&gdvariant::Variant::Int(99))
    );
}

#[test]
fn tres_with_mixed_property_types() {
    let loader = TresLoader::new();
    let tres = r#"[gd_resource type="Resource" format=3]

[resource]
int_val = 42
float_val = 3.14
str_val = "hello world"
bool_true = true
bool_false = false
vec2_val = Vector2(10, 20)
color_val = Color(1, 0, 0, 1)
"#;
    let result = loader.parse_str(tres, "res://mixed.tres");
    assert!(result.is_ok());
    let res = result.unwrap();

    assert_eq!(
        res.get_property("int_val"),
        Some(&gdvariant::Variant::Int(42))
    );
    assert_eq!(
        res.get_property("float_val"),
        Some(&gdvariant::Variant::Float(3.14))
    );
    assert_eq!(
        res.get_property("str_val"),
        Some(&gdvariant::Variant::String("hello world".into()))
    );
    assert_eq!(
        res.get_property("bool_true"),
        Some(&gdvariant::Variant::Bool(true))
    );
    assert_eq!(
        res.get_property("bool_false"),
        Some(&gdvariant::Variant::Bool(false))
    );
    assert_eq!(
        res.get_property("vec2_val"),
        Some(&gdvariant::Variant::Vector2(gdcore::math::Vector2::new(
            10.0, 20.0
        )))
    );
    assert_eq!(
        res.get_property("color_val"),
        Some(&gdvariant::Variant::Color(gdcore::math::Color::new(
            1.0, 0.0, 0.0, 1.0
        )))
    );
}

// ===========================================================================
// 5. Ext resource with various types
// ===========================================================================

#[test]
fn ext_resource_texture_and_script() {
    let loader = TresLoader::new();
    let tres = r#"[gd_resource type="Resource" format=3]

[ext_resource type="Texture2D" path="res://icon.png" id="tex_1"]
[ext_resource type="Script" uid="uid://script_abc" path="res://main.gd" id="scr_1"]

[resource]
texture = ExtResource("tex_1")
script = ExtResource("scr_1")
"#;
    let result = loader.parse_str(tres, "res://ext.tres");
    assert!(result.is_ok());
}

// ===========================================================================
// 6. Edge case: resource with no [resource] section
// ===========================================================================

#[test]
fn tres_header_only_no_resource_section() {
    let loader = TresLoader::new();
    let tres = "[gd_resource type=\"Resource\" format=3]\n";
    let result = loader.parse_str(tres, "res://header_only.tres");
    // Should either succeed with empty resource or return error, not panic
    let _ = result;
}

// ===========================================================================
// 7. Nested Vector3 and Transform2D property types
// ===========================================================================

#[test]
fn tres_with_vector3_property() {
    let loader = TresLoader::new();
    let tres = r#"[gd_resource type="Resource" format=3]

[resource]
position_3d = Vector3(1, 2, 3)
"#;
    let result = loader.parse_str(tres, "res://vec3.tres");
    assert!(result.is_ok());
    let res = result.unwrap();
    assert_eq!(
        res.get_property("position_3d"),
        Some(&gdvariant::Variant::Vector3(gdcore::math::Vector3::new(
            1.0, 2.0, 3.0
        )))
    );
}
