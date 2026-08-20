//! pat-s3rrv.127 — a Label's `label_settings` resource reaches the draw stream.
//!
//! `LabelSettings` had a full data model in `gdrender2d` with its own tests, and
//! **nothing consumed it**: `control_render`'s Label arm read only the theme and
//! `theme_override_colors/font_color`, so an authored LabelSettings changed
//! nothing on screen. Its own module doc claimed the wiring was "elsewhere".
//!
//! 🔑 The precedence is the subtle part. Godot's Label reads
//! `has_settings ? settings->X : theme->X` **per field with no per-field
//! fallback** (`scene/gui/label.cpp:761-767`), so a LabelSettings that sets only
//! an outline still forces the font size and colour to the resource's own
//! values. "Override if the field is present, else theme" is the natural
//! reading and it is wrong.

use gdcore::math::{Color, Vector2};
use gdscene::control_render::{control_draw_items, ControlDrawItem};
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::theme::Theme;
use gdscene::SceneTree;

/// A Label whose settings name both a size and a colour.
const WITH_SETTINGS: &str = r#"[gd_scene load_steps=2 format=3]

[sub_resource type="LabelSettings" id="LabelSettings_1"]
font_size = 32
font_color = Color(0, 0, 1, 1)

[node name="Root" type="Control"]

[node name="Label" type="Label" parent="."]
text = "Patina"
label_settings = SubResource("LabelSettings_1")
"#;

/// 🔑 The discriminating scene: settings that mention **only** an outline. Godot
/// still takes the font size from the resource — its own default of 16 — rather
/// than from the theme.
const SETTINGS_WITHOUT_SIZE: &str = r#"[gd_scene load_steps=2 format=3]

[sub_resource type="LabelSettings" id="LabelSettings_1"]
outline_size = 3

[node name="Root" type="Control"]

[node name="Label" type="Label" parent="."]
text = "Patina"
label_settings = SubResource("LabelSettings_1")
"#;

/// No settings at all — the pre-existing theme path, which must keep working.
const NO_SETTINGS: &str = r#"[gd_scene format=3]

[node name="Root" type="Control"]

[node name="Label" type="Label" parent="."]
text = "Patina"
"#;

/// ⚠️ The theme deliberately says **24**, not the 16 `Theme::default()` uses.
/// With a 16px theme, "the settings' default won" and "the theme won" would be
/// the same number and the middle test below could not fail.
fn theme_24() -> Theme {
    let mut theme = Theme::default();
    theme.set_font_size("Label", "font_size", 24);
    theme
}

fn label_text_item(tscn: &str) -> (u32, Color) {
    let scene = PackedScene::from_tscn(tscn).expect("scene parses");
    let mut tree = SceneTree::new();
    let root_id = tree.root_id();
    let root = add_packed_scene_to_tree(&mut tree, root_id, &scene).expect("scene instances");

    let items = control_draw_items(&mut tree, root, &theme_24(), Vector2::new(300.0, 80.0));
    items
        .iter()
        .find_map(|i| match i {
            ControlDrawItem::Text {
                text,
                font_size,
                color,
                ..
            } if text == "Patina" => Some((*font_size, *color)),
            _ => None,
        })
        .unwrap_or_else(|| panic!("the Label must emit a Text draw item: {items:?}"))
}

/// **The core of the bead.** The resource's size and colour reach the draw item
/// and beat the theme.
#[test]
fn label_settings_override_the_theme() {
    let (size, color) = label_text_item(WITH_SETTINGS);
    assert_eq!(
        size, 32,
        "the settings' font_size must win over the theme's 24"
    );
    assert_eq!(
        (color.r, color.g, color.b),
        (0.0, 0.0, 1.0),
        "the settings' font_color must win, got {color:?}"
    );
}

/// 🔑 The discriminating case. A LabelSettings that never mentions `font_size`
/// still overrides the theme — with the resource's own default of 16.
///
/// ⚠️ A per-field "use it if present, else fall back to the theme" implementation
/// reports **24** here and passes every other test in this file.
#[test]
fn settings_without_a_size_still_override_the_theme() {
    let (size, _) = label_text_item(SETTINGS_WITHOUT_SIZE);
    assert_eq!(
        size, 16,
        "settings present means the resource's default 16 wins, not the theme's 24"
    );
}

/// The control: with no settings the theme still decides, so this change cannot
/// have altered every Label in the project.
#[test]
fn without_settings_the_theme_still_wins() {
    let (size, _) = label_text_item(NO_SETTINGS);
    assert_eq!(size, 24, "no settings means the theme's 24 applies");
}
