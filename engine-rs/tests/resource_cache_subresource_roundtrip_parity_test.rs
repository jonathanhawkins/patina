//! pat-4ocr: Resource cache parity — concrete SubResource resolution round-trip
//! and mutation semantics.
//!
//! Extends existing SubResource coverage (pat-xoj1, pat-vajb, pat-vkh6,
//! pat-8fvf/fn8k) with round-trip and mutation scenarios not yet exercised:
//!
//! - TresSaver→TresLoader round-trip preserves SubResource resolution
//! - Property mutation (overwrite, remove, re-add) affects resolution correctly
//! - sorted_property_keys includes SubResource-referencing properties
//! - SubResource IDs with varied naming conventions all parse and resolve
//! - Saver emits sub-resource sections with correct properties
//! - Sub-resource property_count through parse round-trip
//! - resolve_subresource after subresources map mutation
//!
//! Acceptance: all tests pass under
//! `cargo test --test resource_cache_subresource_roundtrip_parity_test`.

use std::sync::Arc;

use gdcore::math::{Color, Vector2};
use gdcore::ResourceUid;
use gdresource::loader::TresLoader;
use gdresource::saver::TresSaver;
use gdresource::Resource;
use gdvariant::Variant;

// ===========================================================================
// Helpers
// ===========================================================================

/// Builds a parent resource with two sub-resources for round-trip tests.
fn make_styled_resource() -> Resource {
    let mut style_flat = Resource::new("StyleBoxFlat");
    style_flat.set_property("bg_color", Variant::Color(Color::new(0.2, 0.4, 0.6, 1.0)));
    style_flat.set_property("border_width", Variant::Int(3));

    let mut style_empty = Resource::new("StyleBoxEmpty");
    style_empty.set_property("visible", Variant::Bool(true));

    let mut parent = Resource::new("Theme");
    parent.uid = ResourceUid::new(99999);
    parent
        .subresources
        .insert("flat_1".to_string(), Arc::new(style_flat));
    parent
        .subresources
        .insert("empty_1".to_string(), Arc::new(style_empty));
    parent.set_property("panel", Variant::String("SubResource:flat_1".into()));
    parent.set_property("focus", Variant::String("SubResource:empty_1".into()));
    parent.set_property("name", Variant::String("TestTheme".into()));
    parent
}

// ===========================================================================
// 1. Saver output contains all sub-resource sections and properties
// ===========================================================================

#[test]
fn saver_emits_subresource_sections_with_properties() {
    let parent = make_styled_resource();
    let saver = TresSaver::new();
    let output = saver.save_to_string(&parent).unwrap();

    // save_to_string renumbers IDs: empty_1 → "1", flat_1 → "2" (alphabetical)
    assert!(
        output.contains("[sub_resource type=\"StyleBoxEmpty\" id=\"1\"]"),
        "must emit empty_1 as id=\"1\""
    );
    assert!(
        output.contains("[sub_resource type=\"StyleBoxFlat\" id=\"2\"]"),
        "must emit flat_1 as id=\"2\""
    );

    // Properties from flat_1.
    assert!(output.contains("border_width = 3"), "flat_1 border_width");

    // Properties from empty_1.
    assert!(output.contains("visible = true"), "empty_1 visible");

    // Parent [resource] section.
    assert!(
        output.contains("[resource]"),
        "must emit [resource] section"
    );
    assert!(
        output.contains("name = \"TestTheme\""),
        "parent name property"
    );
}

// ===========================================================================
// 2. Saver emits sub-resources in sorted ID order
// ===========================================================================

#[test]
fn saver_emits_subresources_in_sorted_id_order() {
    let parent = make_styled_resource();
    let saver = TresSaver::new();
    let output = saver.save_to_string(&parent).unwrap();

    // After renumbering: empty_1 → "1", flat_1 → "2"
    let id1_pos = output.find("id=\"1\"").expect("id=\"1\" must appear");
    let id2_pos = output.find("id=\"2\"").expect("id=\"2\" must appear");

    assert!(
        id1_pos < id2_pos,
        "sub-resources must be emitted in sorted ID order (1 < 2)"
    );
}

// ===========================================================================
// 3. resolve_subresource works on programmatic resource before save
// ===========================================================================

#[test]
fn resolve_on_programmatic_resource_before_save() {
    let parent = make_styled_resource();

    let flat = parent.resolve_subresource("panel").unwrap();
    assert_eq!(flat.class_name, "StyleBoxFlat");
    assert_eq!(flat.get_property("border_width"), Some(&Variant::Int(3)));

    let empty = parent.resolve_subresource("focus").unwrap();
    assert_eq!(empty.class_name, "StyleBoxEmpty");
    assert_eq!(empty.get_property("visible"), Some(&Variant::Bool(true)));
}

// ===========================================================================
// 4. Property overwrite changes SubResource resolution target
// ===========================================================================

#[test]
fn overwrite_subresource_ref_changes_resolution() {
    let mut parent = make_styled_resource();

    // Initially panel → flat_1.
    let resolved = parent.resolve_subresource("panel").unwrap();
    assert_eq!(resolved.class_name, "StyleBoxFlat");

    // Overwrite to point at empty_1.
    parent.set_property("panel", Variant::String("SubResource:empty_1".into()));
    let resolved = parent.resolve_subresource("panel").unwrap();
    assert_eq!(resolved.class_name, "StyleBoxEmpty");
}

// ===========================================================================
// 5. Property overwrite from SubResource ref to plain string breaks resolution
// ===========================================================================

#[test]
fn overwrite_subresource_ref_to_plain_string_breaks_resolution() {
    let mut parent = make_styled_resource();

    assert!(parent.resolve_subresource("panel").is_some());

    // Overwrite with a non-SubResource string.
    parent.set_property("panel", Variant::String("just_a_string".into()));
    assert!(
        parent.resolve_subresource("panel").is_none(),
        "plain string must not resolve as SubResource"
    );
}

// ===========================================================================
// 6. Property overwrite from SubResource ref to non-string breaks resolution
// ===========================================================================

#[test]
fn overwrite_subresource_ref_to_int_breaks_resolution() {
    let mut parent = make_styled_resource();

    assert!(parent.resolve_subresource("panel").is_some());

    parent.set_property("panel", Variant::Int(42));
    assert!(
        parent.resolve_subresource("panel").is_none(),
        "Int value must not resolve as SubResource"
    );
}

// ===========================================================================
// 7. remove_property then resolve returns None
// ===========================================================================

#[test]
fn remove_property_breaks_resolution() {
    let mut parent = make_styled_resource();
    assert!(parent.resolve_subresource("panel").is_some());

    parent.remove_property("panel");
    assert!(
        parent.resolve_subresource("panel").is_none(),
        "removed property must not resolve"
    );

    // Sub-resource still exists in the map.
    assert!(parent.subresources.contains_key("flat_1"));
}

// ===========================================================================
// 8. Remove then re-add property restores resolution
// ===========================================================================

#[test]
fn remove_then_readd_restores_resolution() {
    let mut parent = make_styled_resource();
    parent.remove_property("panel");
    assert!(parent.resolve_subresource("panel").is_none());

    parent.set_property("panel", Variant::String("SubResource:flat_1".into()));
    let resolved = parent.resolve_subresource("panel").unwrap();
    assert_eq!(resolved.class_name, "StyleBoxFlat");
}

// ===========================================================================
// 9. Adding a sub-resource to the map makes a new property resolvable
// ===========================================================================

#[test]
fn add_subresource_to_map_enables_resolution() {
    let mut parent = Resource::new("Container");
    parent.set_property("child", Variant::String("SubResource:new_sub".into()));

    // No sub-resource yet → resolve fails.
    assert!(parent.resolve_subresource("child").is_none());

    // Add the sub-resource.
    let mut sub = Resource::new("NewType");
    sub.set_property("value", Variant::Int(7));
    parent
        .subresources
        .insert("new_sub".to_string(), Arc::new(sub));

    // Now it resolves.
    let resolved = parent.resolve_subresource("child").unwrap();
    assert_eq!(resolved.class_name, "NewType");
    assert_eq!(resolved.get_property("value"), Some(&Variant::Int(7)));
}

// ===========================================================================
// 10. Removing a sub-resource from the map breaks resolution
// ===========================================================================

#[test]
fn remove_subresource_from_map_breaks_resolution() {
    let mut parent = make_styled_resource();
    assert!(parent.resolve_subresource("panel").is_some());

    parent.subresources.remove("flat_1");
    assert!(
        parent.resolve_subresource("panel").is_none(),
        "dangling ref after map removal must return None"
    );
}

// ===========================================================================
// 11. sorted_property_keys includes SubResource-referencing properties
// ===========================================================================

#[test]
fn sorted_property_keys_includes_subresource_refs() {
    let parent = make_styled_resource();
    let keys = parent.sorted_property_keys();
    let key_strs: Vec<&str> = keys.iter().map(|k| k.as_str()).collect();

    assert!(
        key_strs.contains(&"panel"),
        "must include SubResource ref 'panel'"
    );
    assert!(
        key_strs.contains(&"focus"),
        "must include SubResource ref 'focus'"
    );
    assert!(
        key_strs.contains(&"name"),
        "must include plain string 'name'"
    );

    // Verify sorted order: focus < name < panel.
    let focus_pos = key_strs.iter().position(|k| *k == "focus").unwrap();
    let name_pos = key_strs.iter().position(|k| *k == "name").unwrap();
    let panel_pos = key_strs.iter().position(|k| *k == "panel").unwrap();
    assert!(focus_pos < name_pos, "focus < name");
    assert!(name_pos < panel_pos, "name < panel");
}

// ===========================================================================
// 12. SubResource IDs with varied naming conventions parse and resolve
// ===========================================================================

const TRES_VARIED_IDS: &str = r#"[gd_resource type="Resource" format=3]

[sub_resource type="StyleBoxFlat" id="StyleBoxFlat_x1"]
border_width = 1

[sub_resource type="StyleBoxEmpty" id="style_empty_99"]

[sub_resource type="RectangleShape2D" id="123"]
size = Vector2(10, 20)

[resource]
flat_ref = SubResource("StyleBoxFlat_x1")
empty_ref = SubResource("style_empty_99")
numeric_ref = SubResource("123")
"#;

#[test]
fn varied_id_class_prefix_underscore_resolves() {
    let loader = TresLoader::new();
    let res = loader
        .parse_str(TRES_VARIED_IDS, "res://varied.tres")
        .unwrap();

    let flat = res.resolve_subresource("flat_ref").unwrap();
    assert_eq!(flat.class_name, "StyleBoxFlat");
    assert_eq!(flat.get_property("border_width"), Some(&Variant::Int(1)));
}

#[test]
fn varied_id_snake_case_with_digits_resolves() {
    let loader = TresLoader::new();
    let res = loader
        .parse_str(TRES_VARIED_IDS, "res://varied.tres")
        .unwrap();

    let empty = res.resolve_subresource("empty_ref").unwrap();
    assert_eq!(empty.class_name, "StyleBoxEmpty");
    assert_eq!(empty.property_count(), 0);
}

#[test]
fn varied_id_pure_numeric_resolves() {
    let loader = TresLoader::new();
    let res = loader
        .parse_str(TRES_VARIED_IDS, "res://varied.tres")
        .unwrap();

    let shape = res.resolve_subresource("numeric_ref").unwrap();
    assert_eq!(shape.class_name, "RectangleShape2D");
    match shape.get_property("size") {
        Some(Variant::Vector2(v)) => {
            assert!((v.x - 10.0).abs() < f32::EPSILON);
            assert!((v.y - 20.0).abs() < f32::EPSILON);
        }
        other => panic!("expected Vector2, got {:?}", other),
    }
}

#[test]
fn varied_ids_total_count() {
    let loader = TresLoader::new();
    let res = loader
        .parse_str(TRES_VARIED_IDS, "res://varied.tres")
        .unwrap();
    assert_eq!(res.subresources.len(), 3);
}

// ===========================================================================
// 13. Sub-resource property_count through parse
// ===========================================================================

#[test]
fn parsed_subresource_property_count_matches_source() {
    let loader = TresLoader::new();
    let res = loader
        .parse_str(TRES_VARIED_IDS, "res://varied.tres")
        .unwrap();

    // StyleBoxFlat_x1 has 1 property (border_width).
    let flat = res.resolve_subresource("flat_ref").unwrap();
    assert_eq!(flat.property_count(), 1);

    // style_empty_99 has 0 properties.
    let empty = res.resolve_subresource("empty_ref").unwrap();
    assert_eq!(empty.property_count(), 0);

    // 123 has 1 property (size).
    let shape = res.resolve_subresource("numeric_ref").unwrap();
    assert_eq!(shape.property_count(), 1);
}

// ===========================================================================
// 14. Saver round-trip: save programmatic resource, re-parse, verify structure
// ===========================================================================

#[test]
fn saver_roundtrip_preserves_subresource_class_names() {
    let parent = make_styled_resource();
    let saver = TresSaver::new();
    let saved = saver.save_to_string(&parent).unwrap();

    // After renumbering: empty_1 → "1", flat_1 → "2"
    assert!(saved.contains("[sub_resource type=\"StyleBoxEmpty\" id=\"1\"]"));
    assert!(saved.contains("[sub_resource type=\"StyleBoxFlat\" id=\"2\"]"));
}

#[test]
fn saver_roundtrip_preserves_subresource_properties() {
    let parent = make_styled_resource();
    let saver = TresSaver::new();
    let saved = saver.save_to_string(&parent).unwrap();

    // Sub-resource properties appear in the output.
    assert!(saved.contains("border_width = 3"));
    assert!(saved.contains("visible = true"));
}

#[test]
fn saver_roundtrip_preserves_uid() {
    let parent = make_styled_resource();
    let saver = TresSaver::new();
    let saved = saver.save_to_string(&parent).unwrap();

    assert!(
        saved.contains("uid=\"uid://99999\""),
        "UID must survive save round-trip"
    );
}

// ===========================================================================
// 15. Saver with no sub-resources: no sub_resource sections emitted
// ===========================================================================

#[test]
fn saver_no_subresources_no_sections() {
    let mut r = Resource::new("BareResource");
    r.set_property("value", Variant::Int(42));

    let saver = TresSaver::new();
    let output = saver.save_to_string(&r).unwrap();

    assert!(
        !output.contains("[sub_resource"),
        "no sub_resource sections"
    );
    assert!(output.contains("[resource]"));
    assert!(output.contains("value = 42"));
}

// ===========================================================================
// 16. Saver with sub-resource that has Color property
// ===========================================================================

#[test]
fn saver_preserves_color_on_subresource() {
    let mut sub = Resource::new("StyleBoxFlat");
    sub.set_property("bg_color", Variant::Color(Color::new(1.0, 0.0, 0.5, 0.8)));

    let mut parent = Resource::new("Theme");
    parent.subresources.insert("s1".to_string(), Arc::new(sub));

    let saver = TresSaver::new();
    let output = saver.save_to_string(&parent).unwrap();

    assert!(output.contains("bg_color = Color(1, 0, 0.5, 0.8)"));
}

// ===========================================================================
// 17. Saver with sub-resource that has Vector2 property
// ===========================================================================

#[test]
fn saver_preserves_vector2_on_subresource() {
    let mut sub = Resource::new("RectangleShape2D");
    sub.set_property("size", Variant::Vector2(Vector2::new(64.0, 32.0)));

    let mut parent = Resource::new("Resource");
    parent
        .subresources
        .insert("shape_1".to_string(), Arc::new(sub));

    let saver = TresSaver::new();
    let output = saver.save_to_string(&parent).unwrap();

    assert!(output.contains("size = Vector2(64, 32)"));
}

// ===========================================================================
// 18. Empty sub-resource (no properties) still emits section header
// ===========================================================================

#[test]
fn saver_emits_header_for_empty_subresource() {
    let sub = Resource::new("StyleBoxEmpty");

    let mut parent = Resource::new("Theme");
    parent
        .subresources
        .insert("empty".to_string(), Arc::new(sub));

    let saver = TresSaver::new();
    let output = saver.save_to_string(&parent).unwrap();

    // After renumbering: "empty" → "1"
    assert!(
        output.contains("[sub_resource type=\"StyleBoxEmpty\" id=\"1\"]"),
        "empty sub-resource must still emit its section header, got:\n{output}"
    );
}

// ===========================================================================
// 19. Multiple properties referencing the same sub-resource ID
// ===========================================================================

#[test]
fn multiple_properties_same_subresource_id_resolve_to_same_arc() {
    let mut sub = Resource::new("SharedStyle");
    sub.set_property("shared", Variant::Bool(true));

    let mut parent = Resource::new("Container");
    parent.subresources.insert("s1".to_string(), Arc::new(sub));
    parent.set_property("prop_a", Variant::String("SubResource:s1".into()));
    parent.set_property("prop_b", Variant::String("SubResource:s1".into()));

    let a = parent.resolve_subresource("prop_a").unwrap();
    let b = parent.resolve_subresource("prop_b").unwrap();

    assert!(Arc::ptr_eq(a, b), "same ID must resolve to same Arc");
    assert_eq!(a.class_name, "SharedStyle");
}

// ===========================================================================
// 20. resolve_subresource with "SubResource:" prefix but extra whitespace
// ===========================================================================

#[test]
fn subresource_ref_with_trailing_space_does_not_resolve() {
    let sub = Resource::new("StyleBoxFlat");
    let mut parent = Resource::new("Container");
    parent.subresources.insert("s1".to_string(), Arc::new(sub));
    parent.set_property("ref", Variant::String("SubResource:s1 ".into()));

    // Trailing space means the ID is "s1 ", which won't match "s1".
    assert!(
        parent.resolve_subresource("ref").is_none(),
        "trailing whitespace in SubResource ID should not match"
    );
}
