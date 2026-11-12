//! Integration tests for the theme editor with live preview.
//!
//! Validates the ThemeEditor, ThemeResource, and live preview generation
//! for bead pat-f0qa2: "Theme editor with live preview of control styling".

use gdcore::math::Color;
use gdeditor::theme_editor::{StyleBoxFlat, ThemeEditor, ThemeFont, ThemeItem, ThemeResource};

// ---------------------------------------------------------------------------
// ThemeResource basics
// ---------------------------------------------------------------------------

#[test]
fn theme_resource_empty_defaults() {
    let theme = ThemeResource::new();
    assert_eq!(theme.override_count(), 0);
    assert!(theme.control_types().is_empty());
    assert!(theme.default_font.is_none());
    assert!(theme.default_font_size.is_none());
}

#[test]
fn theme_resource_set_get_color() {
    let mut theme = ThemeResource::new();
    let red = Color::new(1.0, 0.0, 0.0, 1.0);
    theme.set_item("Button", "font_color", ThemeItem::color(red));
    assert!(theme.has_item("Button", "font_color"));
    assert_eq!(
        theme.resolve_color("Button", "font_color", Color::WHITE),
        red
    );
}

#[test]
fn theme_resource_set_get_constant() {
    let mut theme = ThemeResource::new();
    theme.set_item("Label", "shadow_offset_x", ThemeItem::Constant(3));
    assert_eq!(theme.resolve_constant("Label", "shadow_offset_x", 0), 3);
}

#[test]
fn theme_resource_set_get_stylebox() {
    let mut theme = ThemeResource::new();
    let sb = StyleBoxFlat {
        bg_color: Color::new(0.1, 0.2, 0.3, 1.0),
        corner_radius: [8, 8, 8, 8],
        ..Default::default()
    };
    theme.set_item("Button", "normal", ThemeItem::StyleBox(sb.clone()));
    let resolved = theme.resolve_stylebox("Button", "normal").unwrap();
    assert_eq!(resolved.bg_color, sb.bg_color);
    assert_eq!(resolved.corner_radius, [8, 8, 8, 8]);
}

#[test]
fn theme_resource_set_get_font() {
    let mut theme = ThemeResource::new();
    let font = ThemeFont {
        family: "Roboto".into(),
        size: 18,
        bold: true,
        italic: false,
    };
    theme.set_item("Button", "font", ThemeItem::Font(font.clone()));
    let resolved = theme.resolve_font("Button", "font").unwrap();
    assert_eq!(resolved.family, "Roboto");
    assert_eq!(resolved.size, 18);
    assert!(resolved.bold);
}

#[test]
fn theme_resource_font_fallback_to_default() {
    let mut theme = ThemeResource::new();
    theme.default_font = Some(ThemeFont {
        family: "DefaultFont".into(),
        ..Default::default()
    });
    let resolved = theme.resolve_font("Button", "font").unwrap();
    assert_eq!(resolved.family, "DefaultFont");
}

#[test]
fn theme_resource_font_size_fallback() {
    let mut theme = ThemeResource::new();
    theme.default_font_size = Some(20);
    assert_eq!(theme.resolve_font_size("Button", "font_size"), Some(20));
}

#[test]
fn theme_resource_remove_item() {
    let mut theme = ThemeResource::new();
    let red = Color::new(1.0, 0.0, 0.0, 1.0);
    theme.set_item("Button", "font_color", ThemeItem::color(red));
    assert!(theme.has_item("Button", "font_color"));
    assert!(theme.remove_item("Button", "font_color").is_some());
    assert!(!theme.has_item("Button", "font_color"));
    assert!(theme.control_types().is_empty());
}

#[test]
fn theme_resource_remove_nonexistent() {
    let mut theme = ThemeResource::new();
    assert!(theme.remove_item("Nope", "nope").is_none());
}

#[test]
fn theme_resource_control_types_sorted() {
    let mut theme = ThemeResource::new();
    theme.set_item("Panel", "panel", ThemeItem::StyleBox(Default::default()));
    theme.set_item("Button", "normal", ThemeItem::StyleBox(Default::default()));
    theme.set_item("Label", "font_color", ThemeItem::color(Color::WHITE));
    assert_eq!(theme.control_types(), vec!["Button", "Label", "Panel"]);
}

#[test]
fn theme_resource_item_names_sorted() {
    let mut theme = ThemeResource::new();
    theme.set_item("Button", "hover", ThemeItem::StyleBox(Default::default()));
    theme.set_item("Button", "font_color", ThemeItem::color(Color::WHITE));
    theme.set_item("Button", "normal", ThemeItem::StyleBox(Default::default()));
    assert_eq!(
        theme.item_names("Button"),
        vec!["font_color", "hover", "normal"]
    );
}

#[test]
fn theme_resource_resolve_color_fallback() {
    let theme = ThemeResource::new();
    let green = Color::new(0.0, 1.0, 0.0, 1.0);
    assert_eq!(theme.resolve_color("Button", "font_color", green), green);
}

#[test]
fn theme_resource_resolve_constant_fallback() {
    let theme = ThemeResource::new();
    assert_eq!(theme.resolve_constant("Label", "offset", 42), 42);
}

// ---------------------------------------------------------------------------
// ThemeEditor state management
// ---------------------------------------------------------------------------

#[test]
fn theme_editor_defaults() {
    let editor = ThemeEditor::new();
    assert_eq!(editor.revision(), 0);
    assert!(editor.selected_control().is_none());
    assert!(editor.is_preview_visible());
    assert_eq!(editor.theme().override_count(), 0);
}

#[test]
fn theme_editor_select_deselect_control() {
    let mut editor = ThemeEditor::new();
    editor.select_control("Label");
    assert_eq!(editor.selected_control(), Some("Label"));
    editor.deselect_control();
    assert!(editor.selected_control().is_none());
}

#[test]
fn theme_editor_set_item_bumps_revision() {
    let mut editor = ThemeEditor::new();
    editor.set_item("Button", "font_color", ThemeItem::color(Color::WHITE));
    assert_eq!(editor.revision(), 1);
    editor.set_item("Label", "font_color", ThemeItem::color(Color::BLACK));
    assert_eq!(editor.revision(), 2);
}

#[test]
fn theme_editor_remove_bumps_revision_only_if_existed() {
    let mut editor = ThemeEditor::new();
    editor.set_item("Button", "font_color", ThemeItem::color(Color::WHITE));
    let r = editor.revision();
    editor.remove_item("Button", "font_color");
    assert_eq!(editor.revision(), r + 1);
    // Removing again should not bump.
    editor.remove_item("Button", "font_color");
    assert_eq!(editor.revision(), r + 1);
}

#[test]
fn theme_editor_toggle_preview() {
    let mut editor = ThemeEditor::new();
    assert!(editor.is_preview_visible());
    editor.toggle_preview();
    assert!(!editor.is_preview_visible());
    editor.toggle_preview();
    assert!(editor.is_preview_visible());
}

#[test]
fn theme_editor_with_existing_theme() {
    let mut theme = ThemeResource::new();
    theme.set_item("Panel", "panel", ThemeItem::StyleBox(Default::default()));
    let editor = ThemeEditor::with_theme(theme);
    assert!(editor.theme().has_item("Panel", "panel"));
    assert_eq!(editor.revision(), 0);
}

// ---------------------------------------------------------------------------
// Live preview generation
// ---------------------------------------------------------------------------

#[test]
fn preview_generates_all_builtin_controls() {
    let editor = ThemeEditor::new();
    let preview = editor.generate_preview();
    assert!(!preview.is_empty());
    let types: Vec<&str> = preview.iter().map(|p| p.control_type.as_str()).collect();
    assert!(types.contains(&"Button"));
    assert!(types.contains(&"Label"));
    assert!(types.contains(&"LineEdit"));
    assert!(types.contains(&"Panel"));
    assert!(types.contains(&"CheckBox"));
    assert!(types.contains(&"ProgressBar"));
}

#[test]
fn preview_reflects_color_overrides() {
    let mut editor = ThemeEditor::new();
    let red = Color::new(1.0, 0.0, 0.0, 1.0);
    editor.set_item("Button", "font_color", ThemeItem::color(red));
    let preview = editor.generate_preview();
    let button = preview.iter().find(|p| p.control_type == "Button").unwrap();
    assert_eq!(*button.colors.get("font_color").unwrap(), red);
}

#[test]
fn preview_uses_defaults_when_no_overrides() {
    let editor = ThemeEditor::new();
    let preview = editor.generate_preview();
    let button = preview.iter().find(|p| p.control_type == "Button").unwrap();
    // Default color is WHITE when no override set.
    assert_eq!(*button.colors.get("font_color").unwrap(), Color::WHITE);
}

#[test]
fn preview_reflects_stylebox_overrides() {
    let mut editor = ThemeEditor::new();
    let sb = StyleBoxFlat {
        bg_color: Color::new(0.5, 0.5, 0.0, 1.0),
        corner_radius: [10, 10, 10, 10],
        ..Default::default()
    };
    editor.set_item("Button", "normal", ThemeItem::StyleBox(sb.clone()));
    let preview = editor.generate_preview();
    let button = preview.iter().find(|p| p.control_type == "Button").unwrap();
    let preview_sb = button.styleboxes.get("normal").unwrap();
    assert_eq!(preview_sb.bg_color, sb.bg_color);
    assert_eq!(preview_sb.corner_radius, [10, 10, 10, 10]);
}

#[test]
fn preview_reflects_font_override() {
    let mut editor = ThemeEditor::new();
    let font = ThemeFont {
        family: "MonoFont".into(),
        size: 12,
        bold: false,
        italic: true,
    };
    editor.set_item("Button", "font", ThemeItem::Font(font));
    let preview = editor.generate_preview();
    let button = preview.iter().find(|p| p.control_type == "Button").unwrap();
    let pf = button.font.as_ref().unwrap();
    assert_eq!(pf.family, "MonoFont");
    assert!(pf.italic);
}

#[test]
fn preview_reflects_font_size_override() {
    let mut editor = ThemeEditor::new();
    editor.set_item("Button", "font_size", ThemeItem::FontSize(24));
    let preview = editor.generate_preview();
    let button = preview.iter().find(|p| p.control_type == "Button").unwrap();
    assert_eq!(button.font_size, Some(24));
}

// ---------------------------------------------------------------------------
// Control type discovery
// ---------------------------------------------------------------------------

#[test]
fn all_control_types_includes_builtins() {
    let editor = ThemeEditor::new();
    let types = editor.all_control_types();
    assert!(types.contains(&"Button".to_string()));
    assert!(types.contains(&"Label".to_string()));
    assert!(types.contains(&"Panel".to_string()));
}

#[test]
fn all_control_types_includes_custom() {
    let mut editor = ThemeEditor::new();
    editor.set_item("MyWidget", "bg", ThemeItem::color(Color::BLACK));
    let types = editor.all_control_types();
    assert!(types.contains(&"MyWidget".to_string()));
    assert!(types.contains(&"Button".to_string()));
}

#[test]
fn standard_items_for_known_control() {
    let items = ThemeEditor::standard_items_for("Button");
    assert!(!items.is_empty());
    assert!(items
        .iter()
        .any(|(n, k)| *n == "font_color" && *k == "Color"));
    assert!(items
        .iter()
        .any(|(n, k)| *n == "normal" && *k == "StyleBox"));
}

#[test]
fn standard_items_for_unknown_control() {
    assert!(ThemeEditor::standard_items_for("UnknownWidget").is_empty());
}

// ---------------------------------------------------------------------------
// Serialization roundtrip
// ---------------------------------------------------------------------------

#[test]
fn theme_resource_json_roundtrip() {
    let mut theme = ThemeResource::new();
    theme.default_font = Some(ThemeFont::default());
    theme.default_font_size = Some(16);
    theme.set_item(
        "Button",
        "font_color",
        ThemeItem::color(Color::new(1.0, 0.0, 0.0, 1.0)),
    );
    theme.set_item("Button", "normal", ThemeItem::StyleBox(Default::default()));
    theme.set_item("Label", "shadow_offset_x", ThemeItem::Constant(2));
    theme.set_item("ProgressBar", "font_size", ThemeItem::FontSize(14));

    let json = serde_json::to_string_pretty(&theme).expect("serialize");
    let deser: ThemeResource = serde_json::from_str(&json).expect("deserialize");

    assert_eq!(deser.override_count(), theme.override_count());
    assert!(deser.has_item("Button", "font_color"));
    assert!(deser.has_item("Button", "normal"));
    assert!(deser.has_item("Label", "shadow_offset_x"));
    assert!(deser.has_item("ProgressBar", "font_size"));
    assert_eq!(deser.default_font_size, Some(16));
    assert_eq!(
        deser.resolve_color("Button", "font_color", Color::WHITE),
        Color::new(1.0, 0.0, 0.0, 1.0),
    );
}

// ---------------------------------------------------------------------------
// Edge cases
// ---------------------------------------------------------------------------

#[test]
fn override_replaces_existing() {
    let mut theme = ThemeResource::new();
    theme.set_item("Button", "font_color", ThemeItem::color(Color::WHITE));
    theme.set_item("Button", "font_color", ThemeItem::color(Color::BLACK));
    assert_eq!(theme.override_count(), 1);
    assert_eq!(
        theme.resolve_color("Button", "font_color", Color::WHITE),
        Color::BLACK,
    );
}

#[test]
fn multiple_control_types_independent() {
    let mut theme = ThemeResource::new();
    theme.set_item("Button", "font_color", ThemeItem::color(Color::WHITE));
    theme.set_item("Label", "font_color", ThemeItem::color(Color::BLACK));
    assert_eq!(theme.override_count(), 2);
    assert_eq!(
        theme.resolve_color("Button", "font_color", Color::TRANSPARENT),
        Color::WHITE,
    );
    assert_eq!(
        theme.resolve_color("Label", "font_color", Color::TRANSPARENT),
        Color::BLACK,
    );
}

#[test]
fn stylebox_flat_default_has_sane_values() {
    let sb = StyleBoxFlat::default();
    assert!(sb.anti_aliased);
    assert_eq!(sb.border_width, [1, 1, 1, 1]);
    assert_eq!(sb.corner_radius, [3, 3, 3, 3]);
    assert!(sb.content_margin.iter().all(|m| *m > 0.0));
}

#[test]
fn theme_font_default() {
    let font = ThemeFont::default();
    assert_eq!(font.family, "Noto Sans");
    assert_eq!(font.size, 14);
    assert!(!font.bold);
    assert!(!font.italic);
}
