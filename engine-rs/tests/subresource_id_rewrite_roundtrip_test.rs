//! pat-qq7: Match SubResource ID rewrite semantics during save round-trips.
//!
//! Godot rewrites SubResource IDs to sequential integers ("1", "2", "3", …)
//! every time a .tres file is saved.  This test verifies that Patina's
//! save path matches this behavior, including:
//!
//! - IDs are renumbered to sequential integers
//! - Property references (SubResource("old_id")) are updated
//! - Sub-resource properties that reference other sub-resources are updated
//! - Load → save → load round-trip produces identical data
//! - Empty/single/many sub-resource cases
//! - Original IDs with various formats (strings, numbers, mixed)

use std::sync::Arc;

use gdcore::math::Color;
use gdresource::loader::TresLoader;
use gdresource::resource::Resource;
use gdresource::saver::TresSaver;
use gdvariant::Variant;

// ===========================================================================
// Helpers
// ===========================================================================

fn make_resource_with_subs(sub_ids: &[&str]) -> Resource {
    let mut r = Resource::new("Theme");
    for (i, id) in sub_ids.iter().enumerate() {
        let mut sub = Resource::new("StyleBoxFlat");
        sub.set_property(
            "bg_color",
            Variant::Color(Color::new(i as f32 * 0.1, 0.0, 0.0, 1.0)),
        );
        r.subresources
            .insert((*id).to_string(), Arc::new(sub));
    }
    r
}

// ===========================================================================
// 1. Basic renumbering
// ===========================================================================

#[test]
fn renumber_assigns_sequential_ids() {
    let mut r = make_resource_with_subs(&["zebra_1", "alpha_2", "mid_3"]);
    let id_map = r.renumber_subresources();

    // Sorted order: alpha_2, mid_3, zebra_1 → "1", "2", "3"
    assert_eq!(id_map["alpha_2"], "1");
    assert_eq!(id_map["mid_3"], "2");
    assert_eq!(id_map["zebra_1"], "3");

    // Sub-resources should now have sequential keys
    assert!(r.subresources.contains_key("1"));
    assert!(r.subresources.contains_key("2"));
    assert!(r.subresources.contains_key("3"));
    assert!(!r.subresources.contains_key("zebra_1"));
}

#[test]
fn renumber_empty_is_noop() {
    let mut r = Resource::new("Resource");
    let id_map = r.renumber_subresources();
    assert!(id_map.is_empty());
}

#[test]
fn renumber_single_sub_becomes_one() {
    let mut r = make_resource_with_subs(&["my_sub"]);
    let id_map = r.renumber_subresources();
    assert_eq!(id_map["my_sub"], "1");
    assert!(r.subresources.contains_key("1"));
}

// ===========================================================================
// 2. Property reference rewriting
// ===========================================================================

#[test]
fn renumber_updates_property_refs() {
    let mut r = make_resource_with_subs(&["style_a", "style_b"]);
    r.set_property(
        "panel",
        Variant::String("SubResource:style_b".into()),
    );
    r.set_property(
        "button",
        Variant::String("SubResource:style_a".into()),
    );

    r.renumber_subresources();

    // style_a → "1", style_b → "2" (alphabetical)
    assert_eq!(
        r.get_property("panel"),
        Some(&Variant::String("SubResource:2".into()))
    );
    assert_eq!(
        r.get_property("button"),
        Some(&Variant::String("SubResource:1".into()))
    );
}

#[test]
fn renumber_updates_sub_resource_cross_refs() {
    let mut r = Resource::new("Theme");

    let mut sub_a = Resource::new("StyleBoxFlat");
    sub_a.set_property("font", Variant::String("SubResource:font_res".into()));
    r.subresources
        .insert("style_main".to_string(), Arc::new(sub_a));

    let sub_b = Resource::new("Font");
    r.subresources
        .insert("font_res".to_string(), Arc::new(sub_b));

    let id_map = r.renumber_subresources();
    // font_res → "1", style_main → "2" (alphabetical)
    assert_eq!(id_map["font_res"], "1");
    assert_eq!(id_map["style_main"], "2");

    // The style_main sub-resource (now "2") should have its font ref updated
    let sub_2 = &r.subresources["2"];
    assert_eq!(
        sub_2.get_property("font"),
        Some(&Variant::String("SubResource:1".into()))
    );
}

#[test]
fn renumber_leaves_non_subresource_strings_alone() {
    let mut r = make_resource_with_subs(&["s1"]);
    r.set_property("name", Variant::String("hello".into()));
    r.set_property("ext", Variant::String("ExtResource:tex_1".into()));

    r.renumber_subresources();

    assert_eq!(
        r.get_property("name"),
        Some(&Variant::String("hello".into()))
    );
    assert_eq!(
        r.get_property("ext"),
        Some(&Variant::String("ExtResource:tex_1".into()))
    );
}

// ===========================================================================
// 3. Already-sequential IDs
// ===========================================================================

#[test]
fn renumber_already_sequential_is_stable() {
    let mut r = make_resource_with_subs(&["1", "2", "3"]);
    r.set_property("ref", Variant::String("SubResource:2".into()));

    let id_map = r.renumber_subresources();

    // Already sequential, so map should be identity
    assert_eq!(id_map["1"], "1");
    assert_eq!(id_map["2"], "2");
    assert_eq!(id_map["3"], "3");

    assert_eq!(
        r.get_property("ref"),
        Some(&Variant::String("SubResource:2".into()))
    );
}

// ===========================================================================
// 4. Save produces sequential IDs
// ===========================================================================

#[test]
fn save_renumbers_subresource_ids() {
    let mut r = make_resource_with_subs(&["zebra", "alpha"]);
    r.set_property("style", Variant::String("SubResource:zebra".into()));

    let saver = TresSaver::new();
    let output = saver.save_to_string(&r).unwrap();

    // alpha → "1", zebra → "2"
    assert!(
        output.contains("[sub_resource type=\"StyleBoxFlat\" id=\"1\"]"),
        "first sub should have id=\"1\", got:\n{output}"
    );
    assert!(
        output.contains("[sub_resource type=\"StyleBoxFlat\" id=\"2\"]"),
        "second sub should have id=\"2\", got:\n{output}"
    );
    assert!(
        output.contains("style = SubResource(\"2\")"),
        "property ref should be updated to new id, got:\n{output}"
    );
}

#[test]
fn save_raw_preserves_original_ids() {
    let mut r = make_resource_with_subs(&["zebra", "alpha"]);
    r.set_property("style", Variant::String("SubResource:zebra".into()));

    let saver = TresSaver::new();
    let output = saver.save_to_string_raw(&r).unwrap();

    assert!(
        output.contains("id=\"alpha\""),
        "raw save should preserve original IDs"
    );
    assert!(
        output.contains("id=\"zebra\""),
        "raw save should preserve original IDs"
    );
    assert!(
        output.contains("style = SubResource(\"zebra\")"),
        "raw save should preserve original refs"
    );
}

// ===========================================================================
// 5. Load → save → load round-trip
// ===========================================================================

#[test]
fn roundtrip_load_save_load_preserves_data() {
    let tres_source = r#"[gd_resource type="Theme" format=3]

[sub_resource type="StyleBoxFlat" id="StyleBoxFlat_abc"]
bg_color = Color(0.2, 0.3, 0.4, 1)
corner_radius = 4

[sub_resource type="StyleBoxFlat" id="StyleBoxFlat_xyz"]
bg_color = Color(0.8, 0.1, 0.0, 1)

[resource]
panel_style = SubResource("StyleBoxFlat_abc")
button_style = SubResource("StyleBoxFlat_xyz")
"#;

    let loader = TresLoader::new();
    let saver = TresSaver::new();

    // Load
    let res1 = loader.parse_str(tres_source, "res://test.tres").unwrap();
    assert_eq!(res1.subresources.len(), 2);

    // Save (renumbers IDs)
    let saved = saver.save_to_string(&res1).unwrap();

    // IDs should be sequential now
    assert!(saved.contains("id=\"1\""), "got:\n{saved}");
    assert!(saved.contains("id=\"2\""), "got:\n{saved}");

    // Load again
    let res2 = loader.parse_str(&saved, "res://test.tres").unwrap();

    // Same number of sub-resources
    assert_eq!(res2.subresources.len(), 2);

    // Properties should resolve correctly
    let panel = res2.resolve_subresource("panel_style");
    assert!(panel.is_some(), "panel_style should resolve");
    assert_eq!(
        panel.unwrap().get_property("corner_radius"),
        Some(&Variant::Int(4)),
        "panel sub-resource should have corner_radius=4"
    );

    let button = res2.resolve_subresource("button_style");
    assert!(button.is_some(), "button_style should resolve");
}

#[test]
fn roundtrip_is_idempotent() {
    let tres_source = r#"[gd_resource type="Resource" format=3]

[sub_resource type="StyleBoxFlat" id="z_last"]
bg_color = Color(1, 0, 0, 1)

[sub_resource type="StyleBoxFlat" id="a_first"]
bg_color = Color(0, 1, 0, 1)

[resource]
style_a = SubResource("a_first")
style_z = SubResource("z_last")
"#;

    let loader = TresLoader::new();
    let saver = TresSaver::new();

    let res = loader.parse_str(tres_source, "res://test.tres").unwrap();
    let save1 = saver.save_to_string(&res).unwrap();
    let res2 = loader.parse_str(&save1, "res://test.tres").unwrap();
    let save2 = saver.save_to_string(&res2).unwrap();

    // Second save should be identical to first (idempotent)
    assert_eq!(save1, save2, "save should be idempotent after renumbering");
}

// ===========================================================================
// 6. Edge cases
// ===========================================================================

#[test]
fn renumber_numeric_string_ids() {
    // IDs that are already numbers but not sequential
    let mut r = make_resource_with_subs(&["10", "5", "20"]);
    let id_map = r.renumber_subresources();

    // Sorted: "10", "20", "5" (string sort) → "1", "2", "3"
    assert_eq!(id_map["10"], "1");
    assert_eq!(id_map["20"], "2");
    assert_eq!(id_map["5"], "3");
}

#[test]
fn renumber_preserves_sub_resource_properties() {
    let mut r = Resource::new("Theme");
    let mut sub = Resource::new("StyleBoxFlat");
    sub.set_property("corner_radius", Variant::Int(8));
    sub.set_property("bg_color", Variant::Color(Color::new(1.0, 0.0, 0.0, 1.0)));
    r.subresources
        .insert("original_id".to_string(), Arc::new(sub));

    r.renumber_subresources();

    let renamed = &r.subresources["1"];
    assert_eq!(
        renamed.get_property("corner_radius"),
        Some(&Variant::Int(8))
    );
    assert_eq!(
        renamed.get_property("bg_color"),
        Some(&Variant::Color(Color::new(1.0, 0.0, 0.0, 1.0)))
    );
}

#[test]
fn renumber_with_array_refs() {
    let mut r = Resource::new("Resource");
    let sub = Resource::new("SubThing");
    r.subresources
        .insert("thing_a".to_string(), Arc::new(sub));

    r.set_property(
        "items",
        Variant::Array(vec![
            Variant::String("SubResource:thing_a".into()),
            Variant::String("normal_string".into()),
        ]),
    );

    r.renumber_subresources();

    if let Some(Variant::Array(arr)) = r.get_property("items") {
        assert_eq!(arr[0], Variant::String("SubResource:1".into()));
        assert_eq!(arr[1], Variant::String("normal_string".into()));
    } else {
        panic!("items should be an array");
    }
}

#[test]
fn renumber_unreferenced_id_in_property_unchanged() {
    let mut r = make_resource_with_subs(&["only_sub"]);
    r.set_property(
        "ref",
        Variant::String("SubResource:nonexistent".into()),
    );

    r.renumber_subresources();

    // Ref to nonexistent ID should be left as-is
    assert_eq!(
        r.get_property("ref"),
        Some(&Variant::String("SubResource:nonexistent".into()))
    );
}
