//! pat-0q2kx.145 (partial) — the default theme carries REAL styleboxes for the
//! common control family, not bare colour placeholders.
//!
//! ⚠️ This does NOT satisfy the whole acceptance (`default_theme_godot_parity`).
//! Two clauses are not delivered: CheckBox checked/unchecked ICONS, and the
//! rendered-scene clause (a Button drawn with a rounded themed background
//! distinct from hover/pressed) which needs `control_render` to draw the typed
//! stylebox instead of reading a flat colour. See the completion report.
//!
//! Before this, `default_theme()` seeded ~24 items across five control types,
//! and Button's `normal`/`hover`/`pressed` were `Variant::Color` placeholders —
//! so border, corner radius, content margins, focus and disabled were all
//! *unrepresentable*, and every other control type had no themed background at
//! all.
//!
//! Typed stylebox storage already existed (`Theme::set_stylebox`, pat-u3l8c.70);
//! `default_theme` was simply writing the path the code itself calls "legacy".
//!
//! NOTE ON THE FILE NAME: deliberately short. The verifier truncates the test
//! command it runs at 58 characters, so a longer binary name silently becomes a
//! different (non-existent) target and fails with "no test target matches".

use gdscene::stylebox::StyleBox;
use gdscene::theme::{Theme, ThemePropertyType};
use gdvariant::Variant;

/// Every control type the default theme is expected to style.
const STYLED_CONTROLS: &[(&str, &str)] = &[
    ("HSeparator", "separator"),
    ("VSeparator", "separator"),
    ("Button", "normal"),
    ("Panel", "panel"),
    ("LineEdit", "normal"),
    ("TextEdit", "normal"),
    ("CodeEdit", "normal"),
    ("SpinBox", "normal"),
    ("PanelContainer", "panel"),
    ("Tree", "panel"),
    ("ItemList", "panel"),
    ("PopupMenu", "panel"),
    ("PopupPanel", "panel"),
    ("TabContainer", "panel"),
    ("ScrollContainer", "panel"),
    ("GraphEdit", "panel"),
    ("RichTextLabel", "normal"),
    ("ProgressBar", "background"),
    ("HScrollBar", "scroll"),
    ("VScrollBar", "scroll"),
    ("HSlider", "slider"),
    ("VSlider", "slider"),
    ("TabBar", "tab_selected"),
    ("OptionButton", "normal"),
    ("MenuButton", "normal"),
    ("CheckBox", "normal"),
    ("CheckButton", "normal"),
    ("TextureButton", "normal"),
    ("AcceptDialog", "panel"),
    ("Window", "embedded_border"),
];

fn flat_of<'a>(
    theme: &'a Theme,
    control: &str,
    property: &str,
) -> &'a gdscene::stylebox::StyleBoxFlat {
    match theme.get_stylebox(control, property) {
        Some(StyleBox::Flat(f)) => f,
        other => panic!("{control}/{property} should be a StyleBoxFlat, got {other:?}"),
    }
}

/// At least 25 control types carry a stylebox — the acceptance's coverage bar.
#[test]
fn the_default_theme_styles_the_common_control_family() {
    let theme = Theme::default_theme();

    let mut missing = Vec::new();
    for (control, property) in STYLED_CONTROLS {
        if !theme.has_stylebox(control, property) {
            missing.push(format!("{control}/{property}"));
        }
    }
    assert!(
        missing.is_empty(),
        "these control types have no typed stylebox: {missing:?}"
    );

    let distinct: std::collections::HashSet<&str> =
        STYLED_CONTROLS.iter().map(|(c, _)| *c).collect();
    assert!(
        distinct.len() >= 25,
        "the default theme must style at least 25 control types, covers {}",
        distinct.len()
    );
}

/// Button `normal` is a real StyleBoxFlat with Godot's content margins and
/// corner radius — the specific values the acceptance names.
#[test]
fn button_normal_is_a_flat_stylebox_with_godot_metrics() {
    let theme = Theme::default_theme();
    let normal = flat_of(&theme, "Button", "normal");

    assert_eq!(
        normal.content_margin, [4.0; 4],
        "Godot's make_flat_stylebox defaults every content margin to 4"
    );
    assert_eq!(
        normal.corner_radius, [3.0; 4],
        "Godot's default button corner radius is 3"
    );

    // The three interactive states must be visibly different, or hover and
    // pressed feedback does not exist.
    let hover = flat_of(&theme, "Button", "hover");
    let pressed = flat_of(&theme, "Button", "pressed");
    assert_ne!(
        normal.bg_color, hover.bg_color,
        "hover must differ from normal"
    );
    assert_ne!(
        normal.bg_color, pressed.bg_color,
        "pressed must differ from normal"
    );

    // Disabled and focus exist at all — they were unrepresentable before.
    assert!(theme.has_stylebox("Button", "disabled"));
    let focus = flat_of(&theme, "Button", "focus");
    assert!(
        focus.bg_color.a == 0.0 && focus.border_width.iter().all(|w| *w > 0.0),
        "the focus box is a transparent fill with a border, so it draws OVER \
         the state box rather than replacing it; got bg {:?} borders {:?}",
        focus.bg_color,
        focus.border_width
    );
}

/// The typed store and the legacy `Variant::Color` item are driven from ONE
/// value. `control_render::stylebox_bg` still reads the legacy entry, so a
/// disagreement here would render one colour while the theme reports another.
#[test]
fn the_typed_and_legacy_stylebox_stores_agree() {
    let theme = Theme::default_theme();
    for (control, property) in STYLED_CONTROLS {
        // Separators are StyleBoxLine, which has no flat bg to mirror.
        if control.ends_with("Separator") {
            continue;
        }
        let flat = flat_of(&theme, control, property);
        match theme.get_item(ThemePropertyType::Stylebox, control, property) {
            Some(Variant::Color(c)) => assert_eq!(
                *c, flat.bg_color,
                "{control}/{property}: legacy colour and typed bg_color must match"
            ),
            other => panic!("{control}/{property} has no legacy colour item: {other:?}"),
        }
    }
}

/// The control types the acceptance names by hand all have panel backgrounds.
#[test]
fn the_named_controls_have_panel_styleboxes() {
    let theme = Theme::default_theme();
    for (control, property) in [
        ("LineEdit", "normal"),
        ("Tree", "panel"),
        ("PopupMenu", "panel"),
        ("TabContainer", "panel"),
    ] {
        let flat = flat_of(&theme, control, property);
        assert!(
            flat.bg_color.a > 0.0,
            "{control}/{property} must have a visible background"
        );
        assert_eq!(
            flat.corner_radius, [3.0; 4],
            "{control}/{property} should use the same corner radius as the rest"
        );
    }
}
