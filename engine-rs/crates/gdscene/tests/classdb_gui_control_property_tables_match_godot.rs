//! pat-s3rrv.110: the GUI Control classes carry their real property tables.
//!
//! Several of these were registered as bare names — `register_class(
//! ClassRegistration::new("Button").parent("BaseButton"))` — so the class
//! existed, `class_exists` said yes, and every property read fell through to a
//! type-default. A ColorRect came back transparent instead of white and a
//! GridContainer had no `columns` at all.
//!
//! 🔑 THE EXPECTED NAMES ARE MACHINE-EXTRACTED from
//! `upstream/godot/doc/classes/<Class>.xml`, not typed. 145 names across
//! 15 classes is exactly the scale at which a hand-transcribed
//! table acquires a silent typo, and a typo'd expectation makes this test
//! assert the wrong contract while staying green.
//!
//! `get_property_list` walks the whole inheritance chain and concatenates, so
//! these are SUPERSET assertions: the class's own documented members must all
//! be present. Asserting equality would fail on every inherited Control
//! property and say nothing about the registration under test.

use gdcore::math::{Color, Vector2};
use gdobject::class_db::{get_property_list, register_all_classes};
use gdrender2d::renderer::SoftwareRenderer;
use gdrender2d::test_adapter::{capture_frame, pixel_at};
use gdscene::control::{self, AnchorPreset};
use gdscene::control_render::render_control_tree;
use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscene::theme::Theme;
use gdserver2d::viewport::Viewport;
use gdvariant::Variant;

/// `(class, its own documented members)` — straight from the Godot doc XML.
const GUI_CLASS_MEMBERS: &[(&str, &[&str])] = &[
    (
        "Button",
        &[
            "alignment",
            "autowrap_mode",
            "autowrap_trim_flags",
            "clip_text",
            "expand_icon",
            "flat",
            "icon",
            "icon_alignment",
            "language",
            "text",
            "text_direction",
            "text_overrun_behavior",
            "vertical_icon_alignment",
        ],
    ),
    (
        "Label",
        &[
            "autowrap_mode",
            "autowrap_trim_flags",
            "clip_text",
            "ellipsis_char",
            "horizontal_alignment",
            "justification_flags",
            "label_settings",
            "language",
            "lines_skipped",
            "max_lines_visible",
            "mouse_filter",
            "paragraph_separator",
            "size_flags_vertical",
            "structured_text_bidi_override",
            "structured_text_bidi_override_options",
            "tab_stops",
            "text",
            "text_direction",
            "text_overrun_behavior",
            "uppercase",
            "vertical_alignment",
            "visible_characters",
            "visible_characters_behavior",
            "visible_ratio",
        ],
    ),
    ("ColorRect", &["color"]),
    (
        "TextureRect",
        &[
            "expand_mode",
            "flip_h",
            "flip_v",
            "mouse_filter",
            "stretch_mode",
            "texture",
        ],
    ),
    (
        "ProgressBar",
        &[
            "editor_preview_indeterminate",
            "fill_mode",
            "indeterminate",
            "show_percentage",
        ],
    ),
    (
        "RichTextLabel",
        &[
            "autowrap_mode",
            "autowrap_trim_flags",
            "bbcode_enabled",
            "clip_contents",
            "context_menu_enabled",
            "custom_effects",
            "deselect_on_focus_loss_enabled",
            "drag_and_drop_selection_enabled",
            "fit_content",
            "focus_mode",
            "hint_underlined",
            "horizontal_alignment",
            "justification_flags",
            "language",
            "meta_underlined",
            "progress_bar_delay",
            "scroll_active",
            "scroll_following",
            "scroll_following_visible_characters",
            "selection_enabled",
            "shortcut_keys_enabled",
            "structured_text_bidi_override",
            "structured_text_bidi_override_options",
            "tab_size",
            "tab_stops",
            "text",
            "text_direction",
            "threaded",
            "vertical_alignment",
            "visible_characters",
            "visible_characters_behavior",
            "visible_ratio",
        ],
    ),
    (
        "CodeEdit",
        &[
            "auto_brace_completion_enabled",
            "auto_brace_completion_highlight_matching",
            "auto_brace_completion_pairs",
            "code_completion_enabled",
            "code_completion_prefixes",
            "delimiter_comments",
            "delimiter_strings",
            "gutters_draw_bookmarks",
            "gutters_draw_breakpoints_gutter",
            "gutters_draw_executing_lines",
            "gutters_draw_fold_gutter",
            "gutters_draw_line_numbers",
            "gutters_line_numbers_min_digits",
            "gutters_zero_pad_line_numbers",
            "indent_automatic",
            "indent_automatic_prefixes",
            "indent_size",
            "indent_use_spaces",
            "layout_direction",
            "line_folding",
            "line_length_guidelines",
            "symbol_lookup_on_click",
            "symbol_tooltip_on_hover",
            "text_direction",
        ],
    ),
    (
        "SplitContainer",
        &[
            "collapsed",
            "drag_area_highlight_in_editor",
            "drag_area_margin_begin",
            "drag_area_margin_end",
            "drag_area_offset",
            "dragger_visibility",
            "dragging_enabled",
            "split_offset",
            "split_offsets",
            "touch_dragger_enabled",
            "vertical",
        ],
    ),
    (
        "ScrollContainer",
        &[
            "clip_contents",
            "draw_focus_border",
            "follow_focus",
            "horizontal_scroll_mode",
            "scroll_deadzone",
            "scroll_hint_mode",
            "scroll_horizontal",
            "scroll_horizontal_custom_step",
            "scroll_vertical",
            "scroll_vertical_custom_step",
            "tile_scroll_hint",
            "vertical_scroll_mode",
        ],
    ),
    ("GridContainer", &["columns"]),
    (
        "SubViewportContainer",
        &["focus_mode", "mouse_target", "stretch", "stretch_shrink"],
    ),
    ("Container", &["mouse_filter"]),
    (
        "StyleBox",
        &[
            "content_margin_bottom",
            "content_margin_left",
            "content_margin_right",
            "content_margin_top",
        ],
    ),
    (
        "Theme",
        &["default_base_scale", "default_font", "default_font_size"],
    ),
    (
        "ThemeDB",
        &[
            "fallback_base_scale",
            "fallback_font",
            "fallback_font_size",
            "fallback_icon",
            "fallback_stylebox",
        ],
    ),
];

fn prop_default(class: &str, name: &str) -> Variant {
    get_property_list(class)
        .into_iter()
        .find(|p| p.name == name)
        .unwrap_or_else(|| panic!("`{class}` registers no `{name}` property"))
        .default_value
}

/// Positions a control with an explicit local rect. Anchors/offsets, never
/// `set_size`: layout resolution recomputes size from the offsets and would
/// overwrite a directly-set size, leaving a zero-sized control that draws
/// nothing at all.
fn place(tree: &mut SceneTree, id: NodeId, l: f32, t: f32, r: f32, b: f32) {
    control::apply_anchor_preset(tree, id, AnchorPreset::TopLeft);
    control::set_offset_left(tree, id, l);
    control::set_offset_top(tree, id, t);
    control::set_offset_right(tree, id, r);
    control::set_offset_bottom(tree, id, b);
}

/// Renders a single ColorRect and samples a pixel from its middle.
fn color_rect_pixel(explicit: Option<Color>) -> Color {
    register_all_classes();

    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let ui = tree.add_child(root, Node::new("UI", "Control")).unwrap();
    control::apply_anchor_preset(&mut tree, ui, AnchorPreset::FullRect);

    let cr = tree.add_child(ui, Node::new("Rect", "ColorRect")).unwrap();
    place(&mut tree, cr, 10.0, 10.0, 90.0, 90.0);
    if let Some(c) = explicit {
        tree.get_node_mut(cr)
            .unwrap()
            .set_property("color", Variant::Color(c));
    }

    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(100, 100, Color::BLACK);
    let theme = Theme::default_theme();
    render_control_tree(&mut tree, ui, &theme, &mut vp, Vector2::new(100.0, 100.0));
    let fb = capture_frame(&mut renderer, &vp);
    pixel_at(&fb, 50, 50)
}

/// **The acceptance.** Every documented member of all fifteen classes is
/// registered.
#[test]
fn classdb_gui_control_property_tables_match_godot() {
    register_all_classes();

    let mut missing: Vec<String> = Vec::new();
    for (class, expected) in GUI_CLASS_MEMBERS {
        let got: Vec<String> = get_property_list(class)
            .into_iter()
            .map(|p| p.name)
            .collect();
        // CONTROL: an unregistered class returns an empty list, which would
        // otherwise report as "every property missing" and read like a table
        // bug rather than a missing registration.
        assert!(
            !got.is_empty(),
            "`{class}` has no properties at all — the class is not registered, \
             which is a different defect from an incomplete table"
        );
        for name in *expected {
            if !got.iter().any(|g| g == name) {
                missing.push(format!("{class}.{name}"));
            }
        }
    }

    assert!(
        missing.is_empty(),
        "{} documented member(s) are not registered: {missing:?}",
        missing.len()
    );
}

/// The defaults the acceptance names, which are the ones a scene relies on
/// when the `.tscn` overrides nothing.
#[test]
fn gui_control_defaults_match_the_documented_values() {
    register_all_classes();

    // `Color(1, 1, 1, 1)` per ColorRect.xml. A ColorRect with no override is
    // WHITE, not transparent — the difference between a visible panel and an
    // invisible one.
    match prop_default("ColorRect", "color") {
        Variant::Color(c) => assert!(
            c.r == 1.0 && c.g == 1.0 && c.b == 1.0 && c.a == 1.0,
            "ColorRect.color must default to Color(1,1,1,1), got {c:?}"
        ),
        other => panic!("ColorRect.color must be a Color, got {other:?}"),
    }

    // `default="1"` per GridContainer.xml. Zero would be a division-by-zero
    // trap in any layout pass that divides by the column count.
    assert_eq!(
        prop_default("GridContainer", "columns"),
        Variant::Int(1),
        "GridContainer.columns must default to 1"
    );

    // The four the acceptance names for Button, which must be reachable from
    // the inspector and from GDScript.
    for name in ["text", "icon", "flat", "alignment"] {
        let found = get_property_list("Button")
            .into_iter()
            .any(|p| p.name == name);
        assert!(found, "Button must expose `{name}`");
    }

    // ⚠️ `alignment` defaults to 1 (CENTER), not enum-zero.
    assert_eq!(prop_default("Button", "alignment"), Variant::Int(1));
}

/// A default ColorRect draws white, and the draw path really reads the
/// property.
///
/// 🔑 THE WHITE ASSERTION IS VACUOUS ON ITS OWN. `control_render` resolves the
/// colour as `color_prop(..).unwrap_or(Color::WHITE)`, so a ColorRect whose
/// `color` is missing entirely ALSO paints white — the renderer's fallback and
/// the registered default are the same value, a coincidence point. The red
/// case is what makes this test mean something: it proves the draw path
/// consults the property rather than always painting white, and the registered
/// default itself is pinned by `gui_control_defaults_match_the_documented_values`.
#[test]
fn a_default_color_rect_draws_white_and_an_overridden_one_does_not() {
    let tol = 0.03;

    let default_px = color_rect_pixel(None);
    assert!(
        (default_px.r - 1.0).abs() <= tol
            && (default_px.g - 1.0).abs() <= tol
            && (default_px.b - 1.0).abs() <= tol,
        "a ColorRect with no override must draw white, got {default_px:?}"
    );

    // CONTROL: an explicit colour must win. If this also came back white the
    // rect would be painting a constant and the assertion above would prove
    // nothing.
    let red_px = color_rect_pixel(Some(Color::rgb(1.0, 0.0, 0.0)));
    assert!(
        (red_px.r - 1.0).abs() <= tol && red_px.g <= tol && red_px.b <= tol,
        "an explicit ColorRect.color must reach the draw list, got {red_px:?}"
    );
}
