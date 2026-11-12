//! pat-nzgdu.89 — Control theme-color resolution walks Godot's lookup order:
//! a local `add_theme_color_override` wins, else the assigned Theme (checking a
//! `theme_type_variation` type before the control's own type), else the default
//! Theme. `Theme`/`ThemeDB` are constructible via ClassDB.

use gdcore::math::Color;
use gdscene::control;
use gdscene::node::Node;
use gdscene::scene_tree::SceneTree;
use gdscene::theme::Theme;

#[test]
fn control_theme_override_resolution() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let button = tree
        .add_child(root, Node::new("MyButton", "Button"))
        .unwrap();

    let default = Theme::default_theme();
    let red = Color::new(1.0, 0.0, 0.0, 1.0);
    let green = Color::new(0.0, 1.0, 0.0, 1.0);
    let blue = Color::new(0.0, 0.0, 1.0, 1.0);

    // (1) A local add_theme_color_override wins over everything.
    control::add_theme_color_override(&mut tree, button, "font_color", red);
    assert!(control::has_theme_color_override(
        &tree,
        button,
        "font_color"
    ));
    assert_eq!(
        control::resolve_theme_color(&tree, button, "Button", "", None, &default, "font_color"),
        Some(red),
        "a local override is resolved first",
    );

    // (2) Removing the override falls back to the assigned Theme's Button color.
    control::remove_theme_color_override(&mut tree, button, "font_color");
    assert!(!control::has_theme_color_override(
        &tree,
        button,
        "font_color"
    ));
    let mut assigned = Theme::new("assigned");
    assigned.set_color("Button", "font_color", green);
    assert_eq!(
        control::resolve_theme_color(
            &tree,
            button,
            "Button",
            "",
            Some(&assigned),
            &default,
            "font_color",
        ),
        Some(green),
        "resolution falls back to the assigned Theme once the override is removed",
    );

    // (3) With no assigned Theme, resolution falls through to the default theme,
    //     which exposes Button/font_color out of the box.
    let fallback =
        control::resolve_theme_color(&tree, button, "Button", "", None, &default, "font_color");
    assert_eq!(
        fallback,
        default.get_color("Button", "font_color"),
        "final fallback is the default theme",
    );
    assert!(
        fallback.is_some(),
        "the default theme exposes Button/font_color",
    );

    // (4) A theme_type_variation is checked before the control's own type.
    let mut varied = Theme::new("varied");
    varied.set_color("FlatButton", "font_color", blue); // the variation type
    varied.set_color("Button", "font_color", green); // the base type
    assert_eq!(
        control::resolve_theme_color(
            &tree,
            button,
            "Button",
            "FlatButton",
            Some(&varied),
            &default,
            "font_color",
        ),
        Some(blue),
        "theme_type_variation resolves before the base control type",
    );

    // (5) Theme + ThemeDB are constructible via ClassDB.
    gdobject::class_db::register_2d_classes();
    assert!(
        gdobject::class_db::class_exists("Theme"),
        "Theme in ClassDB"
    );
    assert!(
        gdobject::class_db::class_exists("ThemeDB"),
        "ThemeDB in ClassDB"
    );
}
