//! pat-7txt0: Theme resource parity tests.
//!
//! Validates that the [`gdscene::theme::Theme`] resource supports
//! Godot-compatible type/name-keyed lookup for every property category
//! (StyleBox, Color, Font, FontSize, Constant) and that the matching
//! `.tres` syntax parses through [`gdresource`] without data loss.
//!
//! Acceptance: ./scripts/rust_task.sh nextest run --test theme_parity_test -- theme_resource

use gdcore::math::Color;
use gdresource::loader::TresLoader;
use gdresource::resource::Resource;
use gdresource::saver::TresSaver;
use gdscene::theme::{Theme, ThemeDB, ThemePropertyType};
use gdvariant::Variant;
use std::sync::Arc;

// ===========================================================================
// Helpers
// ===========================================================================

fn parse_tres(content: &str) -> Arc<Resource> {
    let loader = TresLoader::new();
    loader.parse_str(content, "test://inline").unwrap()
}

fn roundtrip(resource: &Resource) -> Arc<Resource> {
    let saver = TresSaver::new();
    let serialized = saver.save_to_string(resource).unwrap();
    parse_tres(&serialized)
}

// ===========================================================================
// Built-in defaults (Label / Button / Panel)
// ===========================================================================

#[test]
fn theme_resource_default_has_label_defaults() {
    let theme = Theme::default_theme();
    assert_eq!(theme.get_color("Label", "font_color"), Some(Color::WHITE));
    assert_eq!(theme.get_font_size("Label", "font_size"), Some(16));
    assert_eq!(theme.get_constant("Label", "shadow_offset_x"), Some(1));
    assert_eq!(theme.get_constant("Label", "shadow_offset_y"), Some(1));
    assert_eq!(theme.get_constant("Label", "line_spacing"), Some(3));
}

#[test]
fn theme_resource_default_has_button_defaults() {
    let theme = Theme::default_theme();
    assert!(theme.has_color("Button", "font_color"));
    assert!(theme.has_color("Button", "font_hover_color"));
    assert!(theme.has_color("Button", "font_pressed_color"));
    assert!(theme.has_color("Button", "font_disabled_color"));
    assert_eq!(theme.get_font_size("Button", "font_size"), Some(16));
    assert_eq!(theme.get_constant("Button", "h_separation"), Some(4));
    for state in ["normal", "hover", "pressed"] {
        assert!(
            theme.has_item(ThemePropertyType::Stylebox, "Button", state),
            "Button should have a '{state}' stylebox"
        );
    }
}

#[test]
fn theme_resource_default_has_panel_defaults() {
    let theme = Theme::default_theme();
    assert!(theme.has_item(ThemePropertyType::Stylebox, "Panel", "panel"));
    for margin in [
        "content_margin_left",
        "content_margin_top",
        "content_margin_right",
        "content_margin_bottom",
    ] {
        assert_eq!(theme.get_constant("Panel", margin), Some(4));
    }
}

// ===========================================================================
// Per-category roundtrip
// ===========================================================================

#[test]
fn theme_resource_color_roundtrip() {
    let mut theme = Theme::new("palette");
    let red = Color::rgb(1.0, 0.0, 0.0);
    theme.set_color("Label", "font_color", red);
    assert_eq!(theme.get_color("Label", "font_color"), Some(red));
    assert!(theme.has_color("Label", "font_color"));
    assert!(!theme.has_color("Label", "nonexistent"));
}

#[test]
fn theme_resource_font_size_roundtrip() {
    let mut theme = Theme::new("sizes");
    theme.set_font_size("Button", "font_size", 24);
    assert_eq!(theme.get_font_size("Button", "font_size"), Some(24));
    assert!(theme.has_font_size("Button", "font_size"));
    assert!(theme.get_font_size("Button", "other").is_none());
}

#[test]
fn theme_resource_constant_roundtrip() {
    let mut theme = Theme::new("constants");
    theme.set_constant("HBoxContainer", "separation", 12);
    assert_eq!(theme.get_constant("HBoxContainer", "separation"), Some(12));
    assert!(theme.has_constant("HBoxContainer", "separation"));
    assert!(theme.get_constant("HBoxContainer", "missing").is_none());
}

#[test]
fn theme_resource_font_item_roundtrip() {
    let mut theme = Theme::new("fonts");
    let font_ref = Variant::String("res://fonts/mono.ttf".into());
    theme.set_item(ThemePropertyType::Font, "Label", "font", font_ref.clone());
    assert_eq!(
        theme.get_item(ThemePropertyType::Font, "Label", "font"),
        Some(&font_ref)
    );
    let removed = theme.remove_item(ThemePropertyType::Font, "Label", "font");
    assert_eq!(removed, Some(font_ref));
    assert!(!theme.has_item(ThemePropertyType::Font, "Label", "font"));
}

#[test]
fn theme_resource_stylebox_roundtrip() {
    let mut theme = Theme::new("styleboxes");
    let stylebox = Variant::Color(Color::new(0.1, 0.2, 0.3, 1.0));
    theme.set_item(
        ThemePropertyType::Stylebox,
        "Panel",
        "panel",
        stylebox.clone(),
    );
    assert_eq!(
        theme.get_item(ThemePropertyType::Stylebox, "Panel", "panel"),
        Some(&stylebox)
    );
    assert!(theme.has_item(ThemePropertyType::Stylebox, "Panel", "panel"));
}

// ===========================================================================
// Semantics: missing keys, category independence, type separation
// ===========================================================================

#[test]
fn theme_resource_missing_keys_return_none() {
    let theme = Theme::default_theme();
    assert!(theme.get_color("Label", "nonexistent").is_none());
    assert!(theme.get_font_size("Label", "nonexistent").is_none());
    assert!(theme.get_constant("Label", "nonexistent").is_none());
    assert!(theme
        .get_item(ThemePropertyType::Font, "Label", "nonexistent")
        .is_none());
    assert!(theme
        .get_item(ThemePropertyType::Stylebox, "Label", "nonexistent")
        .is_none());
}

#[test]
fn theme_resource_control_types_are_independent() {
    let mut theme = Theme::new("independent");
    theme.set_color("Label", "font_color", Color::WHITE);
    theme.set_color("Button", "font_color", Color::rgb(0.5, 0.5, 0.5));
    assert_ne!(
        theme.get_color("Label", "font_color"),
        theme.get_color("Button", "font_color")
    );
}

#[test]
fn theme_resource_categories_do_not_shadow() {
    // Setting a color and a stylebox with identical control_type+property
    // must not collide — each property category has its own keyspace.
    let mut theme = Theme::new("categories");
    theme.set_color("Label", "paint", Color::rgb(1.0, 0.0, 0.0));
    theme.set_item(
        ThemePropertyType::Stylebox,
        "Label",
        "paint",
        Variant::Color(Color::rgb(0.0, 1.0, 0.0)),
    );
    assert_eq!(
        theme.get_color("Label", "paint"),
        Some(Color::rgb(1.0, 0.0, 0.0))
    );
    assert_eq!(
        theme.get_item(ThemePropertyType::Stylebox, "Label", "paint"),
        Some(&Variant::Color(Color::rgb(0.0, 1.0, 0.0)))
    );
}

#[test]
fn theme_resource_item_category_type_is_discriminant() {
    // An item stored under ThemePropertyType::Font must not be visible
    // through a lookup under ThemePropertyType::Stylebox.
    let mut theme = Theme::new("discriminant");
    theme.set_item(
        ThemePropertyType::Font,
        "Label",
        "shared",
        Variant::Int(1),
    );
    assert!(theme.has_item(ThemePropertyType::Font, "Label", "shared"));
    assert!(!theme.has_item(ThemePropertyType::Stylebox, "Label", "shared"));
}

// ===========================================================================
// ThemeDB fallback semantics
// ===========================================================================

#[test]
fn theme_resource_themedb_fallback_returns_theme_value_when_present() {
    let db = ThemeDB::new();
    assert_eq!(
        db.get_color_or("Label", "font_color", Color::BLACK),
        Color::WHITE
    );
    assert_eq!(db.get_font_size_or("Label", "font_size", 99), 16);
    assert_eq!(db.get_constant_or("Panel", "content_margin_left", 0), 4);
}

#[test]
fn theme_resource_themedb_fallback_returns_fallback_when_missing() {
    let db = ThemeDB::new();
    assert_eq!(
        db.get_color_or("Unknown", "missing", Color::BLACK),
        Color::BLACK
    );
    assert_eq!(db.get_font_size_or("Unknown", "missing", 7), 7);
    assert_eq!(db.get_constant_or("Unknown", "missing", 42), 42);
}

#[test]
fn theme_resource_themedb_replace_default() {
    let mut db = ThemeDB::new();
    let mut custom = Theme::new("custom");
    custom.set_color("Label", "font_color", Color::rgb(1.0, 0.5, 0.0));
    db.set_default_theme(custom);
    assert_eq!(
        db.default_theme().get_color("Label", "font_color"),
        Some(Color::rgb(1.0, 0.5, 0.0))
    );
    // Replacement drops the built-in engine defaults.
    assert!(db
        .default_theme()
        .get_font_size("Label", "font_size")
        .is_none());
}

// ===========================================================================
// .tres parsing / roundtrip
// ===========================================================================

#[test]
fn theme_resource_tres_fixture_parses() {
    let content = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../apps/godot/fixtures/test_theme.tres"),
    )
    .unwrap();
    let res = parse_tres(&content);
    assert_eq!(res.class_name, "Theme");
    assert_eq!(
        res.get_property("default_font_size"),
        Some(&Variant::Int(16))
    );
}

#[test]
fn theme_resource_tres_inline_roundtrip() {
    let res = parse_tres(
        r#"[gd_resource type="Theme" format=3]

[resource]
default_font_size = 18
Label/colors/font_color = Color(1, 1, 1, 1)
Label/constants/shadow_offset_x = 2
Button/constants/h_separation = 6
"#,
    );
    assert_eq!(res.class_name, "Theme");
    assert_eq!(
        res.get_property("default_font_size"),
        Some(&Variant::Int(18))
    );
    assert_eq!(
        res.get_property("Label/constants/shadow_offset_x"),
        Some(&Variant::Int(2))
    );
    assert_eq!(
        res.get_property("Button/constants/h_separation"),
        Some(&Variant::Int(6))
    );

    // Re-serialize and re-parse; property values must survive the round trip.
    let again = roundtrip(&res);
    assert_eq!(again.class_name, "Theme");
    assert_eq!(
        again.get_property("default_font_size"),
        Some(&Variant::Int(18))
    );
    assert_eq!(
        again.get_property("Label/constants/shadow_offset_x"),
        Some(&Variant::Int(2))
    );
    assert_eq!(
        again.get_property("Button/constants/h_separation"),
        Some(&Variant::Int(6))
    );
}
