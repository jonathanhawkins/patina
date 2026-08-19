//! pat-s3rrv.69 — the `Range` control family: HSlider/VSlider, SpinBox,
//! TextureProgressBar.
//!
//! # What was wrong
//!
//! None of these classes were registered in ClassDB, and `draw_control` — which
//! dispatches on the exact class name — had no arm for any of them. The
//! infrastructure around them was already in place, which made the gap
//! particularly deceptive: the default theme ships their styleboxes
//! (`theme.rs:587-589`), `CONTROL_HIT_CLASSES` accepts them, and
//! `FOCUSABLE_BY_DEFAULT` focuses them. So an authored slider was **clickable,
//! focusable, invisible and inert** — it looked wired from every direction
//! except the one that matters.
//!
//! `Range`'s arithmetic was already correct too: `control::set_range_value`
//! snaps to `step` and clamps to `[min, max - page]`, and its own doc comment
//! names "Slider drag" as the intended caller. It had none.

use gdcore::math::{Color, Vector2};
use gdscene::control::{self, AnchorPreset};
use gdscene::control_render::render_control_tree;
use gdscene::node::{Node, NodeId};
use gdscene::range_controls::{self, SliderAxis, GRABBER_EXTENT};
use gdscene::scene_tree::SceneTree;
use gdscene::theme::Theme;
use gdserver2d::canvas::DrawCommand;
use gdserver2d::viewport::Viewport;
use gdvariant::Variant;

/// Builds a lone control of `class` sized `w`x`h` carrying `props`.
fn control_node(class: &str, w: f32, h: f32, props: &[(&str, Variant)]) -> (SceneTree, NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let mut n = Node::new("C", class);
    for (k, v) in props {
        n.set_property(k, v.clone());
    }
    let id = tree.add_child(root, n).unwrap();
    control::apply_anchor_preset(&mut tree, id, AnchorPreset::TopLeft);
    control::set_offset_right(&mut tree, id, w);
    control::set_offset_bottom(&mut tree, id, h);
    (tree, id)
}

fn draw(class: &str, w: f32, h: f32, props: &[(&str, Variant)]) -> Vec<DrawCommand> {
    let (mut tree, id) = control_node(class, w, h, props);
    let mut vp = Viewport::new(256, 128, Color::BLACK);
    let theme = Theme::default_theme();
    render_control_tree(&mut tree, id, &theme, &mut vp, Vector2::new(256.0, 128.0));
    vp.get_sorted_items()
        .first()
        .map(|it| it.commands.clone())
        .unwrap_or_default()
}

fn filled_rects(cmds: &[DrawCommand]) -> Vec<(Color, Vector2, Vector2)> {
    cmds.iter()
        .filter_map(|c| match c {
            DrawCommand::DrawRect {
                rect,
                color,
                filled: true,
            } => Some((*color, rect.position, rect.size)),
            _ => None,
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Registration
// ---------------------------------------------------------------------------

/// All five resolve through ClassDB to `Range` -> `Control`.
#[test]
fn range_family_resolves_through_classdb() {
    gdobject::class_db::register_2d_classes();

    for class in ["HSlider", "VSlider", "SpinBox", "TextureProgressBar"] {
        assert!(
            gdobject::class_db::is_parent_class(class, "Range"),
            "`{class}` must derive from Range"
        );
        assert!(
            gdobject::class_db::is_parent_class(class, "Control"),
            "`{class}` must reach Control through Range"
        );
    }
    // Slider is abstract in Godot: only H/VSlider are instantiable.
    assert!(
        gdobject::class_db::is_abstract("Slider"),
        "Slider is abstract — instantiating it directly is not a thing Godot allows"
    );
    assert!(
        gdobject::class_db::instantiate("HSlider").is_some(),
        "HSlider is concrete and must instantiate"
    );

    // ⚠️ `step` is the default that bites. `Range.step` is 0.01, but Slider,
    // SpinBox and TextureProgressBar each override it to 1.0. Inheriting 0.01
    // makes a slider snap to hundredths, so a drag lands on 74.99999999 where
    // Godot gives exactly 75 — indistinguishable in a screenshot, and wrong for
    // every equality test a game writes.
    for class in ["Slider", "SpinBox", "TextureProgressBar"] {
        let info = gdobject::class_db::get_class_info(class).expect("registered");
        let step = info
            .properties
            .iter()
            .find(|p| p.name == "step")
            .unwrap_or_else(|| panic!("{class} must override `step`"));
        assert_eq!(
            step.default_value,
            Variant::Float(1.0),
            "`{class}.step` must override Range's 0.01 with 1.0"
        );
    }
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

/// Each control emits draw commands, compared against the theme's own stylebox
/// colours rather than a magic threshold.
#[test]
fn each_range_control_renders_against_the_theme() {
    let theme = Theme::default_theme();

    // -- HSlider: track + grabber ------------------------------------------
    let cmds = draw("HSlider", 100.0, 16.0, &[("value", Variant::Float(50.0))]);
    let rects = filled_rects(&cmds);
    assert!(
        rects.len() >= 2,
        "an HSlider draws a track and a grabber, got {} rects: {cmds:?}",
        rects.len()
    );
    let track_color = theme
        .get_stylebox("HSlider", "slider")
        .and_then(|sb| sb.bg_color())
        .or_else(|| {
            match theme.get_item(
                gdscene::theme::ThemePropertyType::Stylebox,
                "HSlider",
                "slider",
            ) {
                Some(Variant::Color(c)) => Some(*c),
                _ => None,
            }
        })
        .expect("the default theme ships an HSlider `slider` stylebox");
    // ⚠️ Matched by CONTENT, not by index. The render pass may emit a panel or
    // focus rect around the arm's own commands, so pinning `rects[0]` would
    // make this fail for reasons that have nothing to do with sliders.
    assert!(
        rects
            .iter()
            .any(|(color, _, size)| *color == track_color && (size.x - 100.0).abs() < 0.001),
        "the full-width track must use the theme's own `slider` stylebox colour, \
         not a hard-coded one; got {rects:?}"
    );
    // The grabber sits at the middle of the track for value 50 of 0..100.
    let expected =
        range_controls::grabber_rect(Vector2::new(100.0, 16.0), SliderAxis::Horizontal, 0.5);
    assert!(
        rects
            .iter()
            .any(|(_, pos, size)| (pos.x - expected.position.x).abs() < 0.001
                && (size.x - GRABBER_EXTENT).abs() < 0.001),
        "the grabber must be drawn where `grabber_rect` puts it ({expected:?}) — the \
         same geometry the drag path reads, so the two cannot disagree; got {rects:?}"
    );

    // -- SpinBox: field + arrows -------------------------------------------
    let cmds = draw("SpinBox", 80.0, 20.0, &[("value", Variant::Float(7.0))]);
    assert!(
        filled_rects(&cmds).len() >= 3,
        "a SpinBox draws its field plus up/down arrows, got {cmds:?}"
    );
    assert!(
        cmds.iter().any(|c| matches!(
            c,
            DrawCommand::DrawString { text, .. } if text.contains('7')
        )),
        "a SpinBox must show its value, got {cmds:?}"
    );

    // -- TextureProgressBar: linear fill -----------------------------------
    let cmds = draw(
        "TextureProgressBar",
        100.0,
        20.0,
        &[("value", Variant::Float(25.0))],
    );
    let rects = filled_rects(&cmds);
    assert!(
        rects
            .iter()
            .any(|(_, _, size)| (size.x - 25.0).abs() < 0.001),
        "a bar at 25% of a 100px rect fills 25px, got {rects:?}"
    );
}

/// ⚠️ A `VSlider`'s minimum is at the **BOTTOM** (`slider.cpp:74` inverts the
/// ratio). Reusing the horizontal mapping gives a slider that moves perfectly
/// and runs backwards — which any test that merely checks "the value changed"
/// would pass.
#[test]
fn a_vertical_slider_has_its_minimum_at_the_bottom() {
    let size = Vector2::new(16.0, 100.0);
    let half = GRABBER_EXTENT / 2.0;

    // Near the TOP of the control = maximum.
    let top =
        range_controls::ratio_at_local_point(size, SliderAxis::Vertical, Vector2::new(8.0, half));
    assert!(
        top > 0.99,
        "the top of a VSlider is its MAXIMUM, got ratio {top}"
    );
    // Near the BOTTOM = minimum.
    let bottom = range_controls::ratio_at_local_point(
        size,
        SliderAxis::Vertical,
        Vector2::new(8.0, size.y - half),
    );
    assert!(
        bottom < 0.01,
        "the bottom of a VSlider is its MINIMUM, got ratio {bottom}"
    );
    // The horizontal form is NOT inverted, which is what makes the above a real
    // distinction rather than an arbitrary convention.
    let right = range_controls::ratio_at_local_point(
        Vector2::new(100.0, 16.0),
        SliderAxis::Horizontal,
        Vector2::new(100.0 - half, 8.0),
    );
    assert!(right > 0.99, "the right of an HSlider is its MAXIMUM");
}

// ---------------------------------------------------------------------------
// Drag
// ---------------------------------------------------------------------------

/// **The acceptance case.** Dragging the grabber to 75% of the track sets
/// `value` to 75.0 and reports exactly one change.
#[test]
fn range_control_family_renders_and_drags() {
    let (mut tree, id) = control_node("HSlider", 100.0, 16.0, &[]);

    // ⚠️ The point is derived from Godot's own formula, NOT from the helper
    // under test — otherwise the assertion would be circular. With width 100
    // and a 10px grabber the track is 90px, so ratio 0.75 is at
    // `0.75 * 90 + 5 = 72.5` (`slider.cpp:75`: `(pos.x - grab_w/2) / max`).
    let target = Vector2::new(0.75 * (100.0 - GRABBER_EXTENT) + GRABBER_EXTENT / 2.0, 8.0);
    assert_eq!(target.x, 72.5, "the fixture's own arithmetic");

    let changed = range_controls::drag_slider_to_local(&mut tree, id, target);
    assert_eq!(
        changed,
        Some(75.0),
        "dragging to 75% of the track must set value to exactly 75.0. A value \
         near but not equal to 75 means `step` is inheriting Range's 0.01 \
         instead of Slider's 1.0."
    );
    assert_eq!(control::get_range_value(&tree, id), 75.0);

    // ⚠️ Exactly ONE change. Godot emits `value_changed` only on a real change,
    // so a second drag to the same place must report nothing — otherwise a
    // drag would fire once per mouse-move sample and flood any connected
    // callback.
    let again = range_controls::drag_slider_to_local(&mut tree, id, target);
    assert_eq!(
        again, None,
        "re-dragging to the same position must NOT report a change; emitting \
         there would fire value_changed once per motion sample"
    );
}

/// `editable = false` makes the slider inert, matching `Slider::gui_input`.
#[test]
fn a_non_editable_slider_ignores_drags() {
    let (mut tree, id) = control_node(
        "HSlider",
        100.0,
        16.0,
        &[("editable", Variant::Bool(false))],
    );
    assert_eq!(
        range_controls::drag_slider_to_local(&mut tree, id, Vector2::new(72.5, 8.0)),
        None,
        "a non-editable slider must not move"
    );
    assert_eq!(control::get_range_value(&tree, id), 0.0);

    // The control: the same drag on a default slider DOES move it, so the
    // assertion above is about `editable` and not about the drag being broken.
    let (mut tree, id) = control_node("HSlider", 100.0, 16.0, &[]);
    assert!(range_controls::drag_slider_to_local(&mut tree, id, Vector2::new(72.5, 8.0)).is_some());
}

/// A radial `TextureProgressBar` at 25% emits a quarter-arc polygon.
#[test]
fn a_radial_bar_emits_a_quarter_arc_at_one_quarter() {
    let cmds = draw(
        "TextureProgressBar",
        64.0,
        64.0,
        &[
            ("fill_mode", Variant::Int(range_controls::FILL_CLOCKWISE)),
            ("value", Variant::Float(25.0)),
        ],
    );
    let poly = cmds
        .iter()
        .find_map(|c| match c {
            DrawCommand::DrawPolygon { points, .. } => Some(points.clone()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("a radial fill must emit a polygon, got {cmds:?}"));

    assert!(poly.len() >= 3, "an arc needs at least three points");
    // Centre first, then the rim: a triangle fan.
    let center = poly[0];
    assert!((center.x - 32.0).abs() < 0.001 && (center.y - 32.0).abs() < 0.001);

    // 25% of 360 degrees is a quarter turn. Clockwise from twelve o'clock, the
    // first rim point is straight up and the last is straight right — which is
    // what distinguishes clockwise-from-noon from the mathematical convention
    // (counter-clockwise from three o'clock).
    let first = poly[1];
    let last = *poly.last().unwrap();
    assert!(
        first.y < center.y && (first.x - center.x).abs() < 0.001,
        "the arc starts at twelve o'clock, got {first:?}"
    );
    assert!(
        last.x > center.x && (last.y - center.y).abs() < 0.001,
        "a quarter turn clockwise ends at three o'clock, got {last:?}"
    );
}
