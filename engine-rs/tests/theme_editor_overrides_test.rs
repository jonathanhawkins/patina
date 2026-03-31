//! Integration tests for theme editor color, font, and StyleBox overrides.
//!
//! Validates typed override methods, batch operations, palette application,
//! presets, and override enumeration for bead pat-r7cqx.

use gdcore::math::Color;
use gdeditor::theme_editor::{
    OverrideKind, StyleBoxFlat, ThemeColorPalette, ThemeEditor, ThemeFont, ThemeItem,
};

// ---------------------------------------------------------------------------
// Typed color override methods
// ---------------------------------------------------------------------------

#[test]
fn set_and_get_color_override() {
    let mut editor = ThemeEditor::new();
    let red = Color::new(1.0, 0.0, 0.0, 1.0);
    editor.set_color("Button", "font_color", red);
    assert_eq!(editor.get_color("Button", "font_color"), Some(red));
}

#[test]
fn get_color_returns_none_when_missing() {
    let editor = ThemeEditor::new();
    assert_eq!(editor.get_color("Button", "font_color"), None);
}

#[test]
fn get_color_returns_none_for_wrong_type() {
    let mut editor = ThemeEditor::new();
    editor.set_constant("Button", "font_color", 42);
    assert_eq!(editor.get_color("Button", "font_color"), None);
}

#[test]
fn set_color_bumps_revision() {
    let mut editor = ThemeEditor::new();
    let rev = editor.revision();
    editor.set_color("Label", "font_color", Color::WHITE);
    assert_eq!(editor.revision(), rev + 1);
}

// ---------------------------------------------------------------------------
// Typed font override methods
// ---------------------------------------------------------------------------

#[test]
fn set_and_get_font_override() {
    let mut editor = ThemeEditor::new();
    let font = ThemeFont {
        family: "Roboto Mono".into(),
        size: 16,
        bold: true,
        italic: false,
    };
    editor.set_font("Button", "font", font.clone());
    let got = editor.get_font("Button", "font").unwrap();
    assert_eq!(got.family, "Roboto Mono");
    assert_eq!(got.size, 16);
    assert!(got.bold);
}

#[test]
fn get_font_returns_none_when_missing() {
    let editor = ThemeEditor::new();
    assert!(editor.get_font("Button", "font").is_none());
}

// ---------------------------------------------------------------------------
// Typed StyleBox override methods
// ---------------------------------------------------------------------------

#[test]
fn set_and_get_stylebox_override() {
    let mut editor = ThemeEditor::new();
    let sb = StyleBoxFlat {
        bg_color: Color::new(0.5, 0.5, 0.5, 1.0),
        corner_radius: [8, 8, 8, 8],
        ..Default::default()
    };
    editor.set_stylebox("Panel", "panel", sb.clone());
    let got = editor.get_stylebox("Panel", "panel").unwrap();
    assert_eq!(got.bg_color, sb.bg_color);
    assert_eq!(got.corner_radius, [8, 8, 8, 8]);
}

#[test]
fn get_stylebox_returns_none_when_missing() {
    let editor = ThemeEditor::new();
    assert!(editor.get_stylebox("Panel", "panel").is_none());
}

// ---------------------------------------------------------------------------
// Typed constant override methods
// ---------------------------------------------------------------------------

#[test]
fn set_and_get_constant_override() {
    let mut editor = ThemeEditor::new();
    editor.set_constant("Label", "shadow_offset_x", 3);
    assert_eq!(editor.get_constant("Label", "shadow_offset_x"), Some(3));
}

#[test]
fn get_constant_returns_none_when_missing() {
    let editor = ThemeEditor::new();
    assert_eq!(editor.get_constant("Label", "shadow_offset_x"), None);
}

// ---------------------------------------------------------------------------
// Typed font size override methods
// ---------------------------------------------------------------------------

#[test]
fn set_and_get_font_size_override() {
    let mut editor = ThemeEditor::new();
    editor.set_font_size("Button", "font_size", 20);
    assert_eq!(editor.get_font_size("Button", "font_size"), Some(20));
}

#[test]
fn get_font_size_returns_none_when_missing() {
    let editor = ThemeEditor::new();
    assert_eq!(editor.get_font_size("Button", "font_size"), None);
}

// ---------------------------------------------------------------------------
// Default font and font size
// ---------------------------------------------------------------------------

#[test]
fn set_default_font() {
    let mut editor = ThemeEditor::new();
    let rev = editor.revision();
    editor.set_default_font(ThemeFont {
        family: "Inter".into(),
        size: 15,
        bold: false,
        italic: false,
    });
    assert_eq!(editor.revision(), rev + 1);
    assert_eq!(
        editor.theme().default_font.as_ref().unwrap().family,
        "Inter"
    );
}

#[test]
fn set_default_font_size() {
    let mut editor = ThemeEditor::new();
    editor.set_default_font_size(18);
    assert_eq!(editor.theme().default_font_size, Some(18));
}

// ---------------------------------------------------------------------------
// Palette application
// ---------------------------------------------------------------------------

#[test]
fn apply_palette_sets_font_colors_across_controls() {
    let mut editor = ThemeEditor::new();
    let palette = ThemeColorPalette {
        font_color: Color::new(0.9, 0.9, 0.9, 1.0),
        bg_color: Color::new(0.2, 0.2, 0.2, 1.0),
        accent_color: Color::new(0.3, 0.6, 0.9, 1.0),
        disabled_color: Color::new(0.4, 0.4, 0.4, 1.0),
        border_color: Color::new(0.5, 0.5, 0.5, 1.0),
    };
    editor.apply_palette(&palette);

    // Check font colors set on multiple controls.
    for ct in &["Button", "Label", "LineEdit", "CheckBox", "ProgressBar"] {
        assert_eq!(
            editor.get_color(ct, "font_color"),
            Some(palette.font_color),
            "font_color not set on {}",
            ct
        );
    }

    // Check accent colors.
    assert_eq!(
        editor.get_color("Button", "font_color_hover"),
        Some(palette.accent_color),
    );
    assert_eq!(
        editor.get_color("LineEdit", "caret_color"),
        Some(palette.accent_color),
    );
}

#[test]
fn apply_palette_sets_styleboxes() {
    let mut editor = ThemeEditor::new();
    let palette = ThemeColorPalette {
        font_color: Color::WHITE,
        bg_color: Color::new(0.15, 0.15, 0.15, 1.0),
        accent_color: Color::new(0.4, 0.6, 0.8, 1.0),
        disabled_color: Color::new(0.3, 0.3, 0.3, 1.0),
        border_color: Color::new(0.4, 0.4, 0.4, 1.0),
    };
    editor.apply_palette(&palette);

    // Panel background.
    let panel_sb = editor.get_stylebox("Panel", "panel").unwrap();
    assert_eq!(panel_sb.bg_color, palette.bg_color);

    // Button hover.
    let btn_hover = editor.get_stylebox("Button", "hover").unwrap();
    assert_eq!(btn_hover.bg_color, palette.accent_color);

    // ProgressBar fill.
    let pb_fill = editor.get_stylebox("ProgressBar", "fill").unwrap();
    assert_eq!(pb_fill.bg_color, palette.accent_color);
}

#[test]
fn apply_palette_bumps_revision_multiple_times() {
    let mut editor = ThemeEditor::new();
    let palette = ThemeColorPalette {
        font_color: Color::WHITE,
        bg_color: Color::BLACK,
        accent_color: Color::new(0.5, 0.5, 1.0, 1.0),
        disabled_color: Color::new(0.3, 0.3, 0.3, 1.0),
        border_color: Color::new(0.4, 0.4, 0.4, 1.0),
    };
    editor.apply_palette(&palette);
    // Should have bumped revision many times (one per set_item call).
    assert!(editor.revision() > 10);
}

// ---------------------------------------------------------------------------
// Copy overrides
// ---------------------------------------------------------------------------

#[test]
fn copy_overrides_between_controls() {
    let mut editor = ThemeEditor::new();
    editor.set_color("Button", "font_color", Color::WHITE);
    editor.set_stylebox("Button", "normal", StyleBoxFlat::default());
    let count = editor.copy_overrides("Button", "CustomButton");
    assert_eq!(count, 2);
    assert_eq!(
        editor.get_color("CustomButton", "font_color"),
        Some(Color::WHITE)
    );
    assert!(editor.get_stylebox("CustomButton", "normal").is_some());
}

#[test]
fn copy_overrides_from_nonexistent_returns_zero() {
    let mut editor = ThemeEditor::new();
    assert_eq!(editor.copy_overrides("NonExistent", "Target"), 0);
}

#[test]
fn copy_overrides_bumps_revision() {
    let mut editor = ThemeEditor::new();
    editor.set_color("Button", "font_color", Color::WHITE);
    let rev = editor.revision();
    editor.copy_overrides("Button", "MyButton");
    assert_eq!(editor.revision(), rev + 1);
}

// ---------------------------------------------------------------------------
// Clear operations
// ---------------------------------------------------------------------------

#[test]
fn clear_control_overrides() {
    let mut editor = ThemeEditor::new();
    editor.set_color("Button", "font_color", Color::WHITE);
    editor.set_stylebox("Button", "normal", StyleBoxFlat::default());
    let removed = editor.clear_control_overrides("Button");
    assert_eq!(removed, 2);
    assert!(editor.get_color("Button", "font_color").is_none());
}

#[test]
fn clear_control_overrides_nonexistent_returns_zero() {
    let mut editor = ThemeEditor::new();
    assert_eq!(editor.clear_control_overrides("NonExistent"), 0);
}

#[test]
fn clear_all_resets_theme() {
    let mut editor = ThemeEditor::new();
    editor.set_color("Button", "font_color", Color::WHITE);
    editor.set_color("Label", "font_color", Color::BLACK);
    editor.set_default_font_size(18);
    let rev = editor.revision();
    editor.clear_all();
    assert_eq!(editor.revision(), rev + 1);
    assert_eq!(editor.total_override_count(), 0);
    assert!(editor.theme().default_font_size.is_none());
}

// ---------------------------------------------------------------------------
// Override listing
// ---------------------------------------------------------------------------

#[test]
fn list_overrides_sorted_by_name() {
    let mut editor = ThemeEditor::new();
    editor.set_stylebox("Button", "normal", StyleBoxFlat::default());
    editor.set_color("Button", "font_color", Color::WHITE);
    editor.set_constant("Button", "margin", 4);

    let overrides = editor.list_overrides("Button");
    assert_eq!(overrides.len(), 3);
    assert_eq!(overrides[0].name, "font_color");
    assert_eq!(overrides[0].kind, OverrideKind::Color);
    assert_eq!(overrides[1].name, "margin");
    assert_eq!(overrides[1].kind, OverrideKind::Constant);
    assert_eq!(overrides[2].name, "normal");
    assert_eq!(overrides[2].kind, OverrideKind::StyleBox);
}

#[test]
fn list_overrides_empty_for_unknown_control() {
    let editor = ThemeEditor::new();
    assert!(editor.list_overrides("NonExistent").is_empty());
}

#[test]
fn total_override_count() {
    let mut editor = ThemeEditor::new();
    assert_eq!(editor.total_override_count(), 0);
    editor.set_color("Button", "font_color", Color::WHITE);
    editor.set_color("Label", "font_color", Color::BLACK);
    assert_eq!(editor.total_override_count(), 2);
}

// ---------------------------------------------------------------------------
// Presets
// ---------------------------------------------------------------------------

#[test]
fn available_presets() {
    let presets = ThemeEditor::available_presets();
    assert!(presets.contains(&"Dark"));
    assert!(presets.contains(&"Light"));
}

#[test]
fn load_dark_preset() {
    let mut editor = ThemeEditor::new();
    editor.load_preset_dark();
    // Should have overrides set.
    assert!(editor.total_override_count() > 0);
    // Should have default font.
    assert!(editor.theme().default_font.is_some());
    assert_eq!(editor.theme().default_font_size, Some(14));
    // Button font color should be set.
    assert!(editor.get_color("Button", "font_color").is_some());
    // Panel should have a StyleBox.
    assert!(editor.get_stylebox("Panel", "panel").is_some());
}

#[test]
fn load_light_preset() {
    let mut editor = ThemeEditor::new();
    editor.load_preset_light();
    assert!(editor.total_override_count() > 0);
    // Light theme font color should be dark.
    let fc = editor.get_color("Button", "font_color").unwrap();
    assert!(
        fc.r < 0.5,
        "Light theme font should be dark, got r={}",
        fc.r
    );
}

#[test]
fn load_preset_by_name() {
    let mut editor = ThemeEditor::new();
    assert!(editor.load_preset("Dark"));
    assert!(editor.total_override_count() > 0);
}

#[test]
fn load_unknown_preset_returns_false() {
    let mut editor = ThemeEditor::new();
    assert!(!editor.load_preset("NonExistent"));
}

#[test]
fn preset_clears_previous_overrides() {
    let mut editor = ThemeEditor::new();
    editor.set_color("MyCustom", "special", Color::new(1.0, 0.0, 1.0, 1.0));
    editor.load_preset_dark();
    // Custom override should be gone since preset calls clear_all first.
    assert!(editor.get_color("MyCustom", "special").is_none());
}

// ---------------------------------------------------------------------------
// Preview reflects override changes
// ---------------------------------------------------------------------------

#[test]
fn preview_updates_after_palette_application() {
    let mut editor = ThemeEditor::new();
    let palette = ThemeColorPalette {
        font_color: Color::new(0.8, 0.8, 0.8, 1.0),
        bg_color: Color::new(0.1, 0.1, 0.1, 1.0),
        accent_color: Color::new(0.2, 0.4, 0.8, 1.0),
        disabled_color: Color::new(0.3, 0.3, 0.3, 1.0),
        border_color: Color::new(0.4, 0.4, 0.4, 1.0),
    };
    editor.apply_palette(&palette);
    let preview = editor.generate_preview();
    let button = preview.iter().find(|p| p.control_type == "Button").unwrap();
    assert_eq!(
        *button.colors.get("font_color").unwrap(),
        palette.font_color
    );
    assert_eq!(
        button.styleboxes.get("hover").unwrap().bg_color,
        palette.accent_color,
    );
}

#[test]
fn preview_updates_after_preset_load() {
    let mut editor = ThemeEditor::new();
    editor.load_preset_dark();
    let preview = editor.generate_preview();
    let label = preview.iter().find(|p| p.control_type == "Label").unwrap();
    // Should have font color from the dark preset.
    let fc = label.colors.get("font_color").unwrap();
    assert!(fc.r > 0.5, "Dark preset label should have light font color");
}

// ---------------------------------------------------------------------------
// Edge cases
// ---------------------------------------------------------------------------

#[test]
fn override_color_then_replace_with_stylebox() {
    let mut editor = ThemeEditor::new();
    editor.set_color("Button", "normal", Color::WHITE);
    // Replace color with a StyleBox.
    editor.set_stylebox("Button", "normal", StyleBoxFlat::default());
    assert!(editor.get_color("Button", "normal").is_none());
    assert!(editor.get_stylebox("Button", "normal").is_some());
    assert_eq!(editor.total_override_count(), 1);
}

#[test]
fn multiple_set_operations_single_item() {
    let mut editor = ThemeEditor::new();
    editor.set_color("Button", "font_color", Color::WHITE);
    editor.set_color("Button", "font_color", Color::BLACK);
    editor.set_color("Button", "font_color", Color::new(0.5, 0.5, 0.5, 1.0));
    assert_eq!(editor.total_override_count(), 1);
    let c = editor.get_color("Button", "font_color").unwrap();
    assert!((c.r - 0.5).abs() < 0.001);
}
