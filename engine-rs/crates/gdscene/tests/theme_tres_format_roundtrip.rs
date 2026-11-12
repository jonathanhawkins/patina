//! pat-0q2kx.150 — a Godot-authored Theme `.tres` loads and changes pixels.
//!
//! Godot's `Theme` (`scene/resources/theme.cpp`) keys its items in the
//! `[resource]` section as `Type/category/name = value`, with styleboxes as
//! `SubResource(...)`, type variations as `Variation/base_type = &"Base"`, and
//! theme-wide `default_font_size` / `default_base_scale`.
//!
//! Patina's ingestion (`load_styleboxes_from_tscn`) keyed styleboxes by
//! **sub-resource id** under a caller-supplied type — so a real theme file
//! produced entries named `StyleBoxFlat_abc`, which no Control ever asks for,
//! and applied nothing. There was also no icon or font item map, no type
//! variations, and no defaults.

use gdcore::math::{Color, Vector2};
use gdscene::control;
use gdscene::control::AnchorPreset;
use gdscene::control_render::control_draw_items;
use gdscene::node::Node;
use gdscene::scene_tree::SceneTree;
use gdscene::theme::{Theme, ThemePropertyType};

/// A theme in Godot's real on-disk format: a stylebox sub-resource referenced
/// by an item key, a colour, a type variation, and a theme-wide default.
const THEME_TRES: &str = r#"[gd_resource type="Theme" load_steps=2 format=3]

[sub_resource type="StyleBoxFlat" id="StyleBoxFlat_btn"]
bg_color = Color(0.8, 0.2, 0.1, 1)

[resource]
default_font_size = 20
default_base_scale = 1.5
Button/styles/normal = SubResource("StyleBoxFlat_btn")
Button/colors/font_color = Color(0.1, 0.9, 0.2, 1)
Button/constants/h_separation = 7
Button/font_sizes/font_size = 14
FlatButton/base_type = &"Button"
"#;

fn approx(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-3
}

/// **The acceptance test.** The real format parses, variations resolve to their
/// base, and the theme's stylebox colour reaches the rendered Button.
#[test]
fn theme_tres_format_roundtrip() {
    let theme = Theme::from_tres(THEME_TRES);

    // -- the stylebox resolved through its SubResource reference -----------
    let sb = theme
        .get_stylebox("Button", "normal")
        .expect("`Button/styles/normal` must resolve its SubResource stylebox");
    let bg = sb.bg_color().expect("a flat box has a bg colour");
    assert!(
        approx(bg.r, 0.8) && approx(bg.g, 0.2) && approx(bg.b, 0.1),
        "the stylebox must keep its authored bg_color, got {bg:?}",
    );

    // -- items in every category -------------------------------------------
    let fc = theme
        .get_color("Button", "font_color")
        .expect("`Button/colors/font_color` must load");
    assert!(
        approx(fc.g, 0.9),
        "font_color should be greenish, got {fc:?}"
    );
    assert_eq!(theme.get_constant("Button", "h_separation"), Some(7));
    assert_eq!(theme.get_font_size("Button", "font_size"), Some(14));

    // -- theme-wide defaults ------------------------------------------------
    assert_eq!(theme.default_font_size, Some(20));
    assert_eq!(theme.default_base_scale, Some(1.5));

    // -- type variations resolve to their base ------------------------------
    assert_eq!(theme.get_type_variation_base("FlatButton"), Some("Button"));
    assert!(
        theme.get_stylebox("FlatButton", "normal").is_some(),
        "a variation must inherit its base type's stylebox",
    );
    assert_eq!(
        theme.get_constant("FlatButton", "h_separation"),
        Some(7),
        "a variation must inherit its base type's constants",
    );
    // A type that is *not* a variation inherits nothing.
    assert_eq!(theme.get_constant("Label", "h_separation"), None);

    // -- and it reaches pixels ----------------------------------------------
    //
    // This is the half that a parse-only fix would miss: the render path reads
    // the generic Variant item, which a loaded `.tres` does not populate, so it
    // now falls through to the typed stylebox.
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let btn = tree.add_child(root, Node::new("Btn", "Button")).unwrap();
    // A Control's rect comes from its anchor preset plus offsets, not from a
    // `size` property — set only the latter and the button lays out zero-sized
    // and emits no draw items at all.
    control::apply_anchor_preset(&mut tree, btn, AnchorPreset::TopLeft);
    control::set_offset_right(&mut tree, btn, 40.0);
    control::set_offset_bottom(&mut tree, btn, 20.0);

    let items = control_draw_items(&mut tree, btn, &theme, Vector2::new(64.0, 64.0));
    assert!(
        items.iter().any(|i| matches!(
            i,
            gdscene::control_render::ControlDrawItem::Rect { color, .. }
                if approx(color.r, 0.8) && approx(color.g, 0.2) && approx(color.b, 0.1)
        )),
        "the themed Button must be drawn with the theme's bg colour; got {items:?}",
    );
}

/// The item-list and whole-theme APIs Godot exposes for introspection.
#[test]
fn theme_introspection_and_merge() {
    let mut theme = Theme::from_tres(THEME_TRES);

    let types = theme.get_type_list();
    assert!(
        types.contains(&"Button".to_string()),
        "every type carrying an item must be listed, got {types:?}",
    );
    assert_eq!(
        theme.get_item_list("Button", ThemePropertyType::Color),
        vec!["font_color".to_string()],
    );
    assert_eq!(
        theme.get_type_variation_list(),
        vec!["FlatButton".to_string()]
    );

    // `merge_with` adopts the other theme's items but not a default it has
    // already set for itself.
    let mut other = Theme::new("Other");
    other.set_color("Label", "font_color", Color::new(0.0, 0.0, 1.0, 1.0));
    other.default_font_size = Some(99);
    theme.merge_with(&other);
    assert!(theme.get_color("Label", "font_color").is_some());
    assert_eq!(
        theme.default_font_size,
        Some(20),
        "merge must not overwrite a default the receiver already set",
    );

    // `clear` empties everything, variations included.
    theme.clear();
    assert!(theme.get_type_list().is_empty());
    assert_eq!(theme.get_type_variation_base("FlatButton"), None);
    assert_eq!(theme.default_font_size, None);
}

/// A variation cycle must not hang the lookup.
#[test]
fn variation_cycle_terminates() {
    let mut theme = Theme::new("Cyclic");
    theme.set_type_variation("A", "B");
    theme.set_type_variation("B", "A");
    theme.set_color("B", "font_color", Color::new(1.0, 0.0, 0.0, 1.0));

    assert!(
        theme.get_color("A", "font_color").is_some(),
        "A resolves through B",
    );
    // The real assertion is that this returns at all rather than looping.
    assert_eq!(theme.get_color("A", "missing"), None);
}
