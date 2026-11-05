//! pat-u3l8c.67 — GUI Control render pass, framebuffer app test.
//!
//! Before this bead NO Control drawing from the scene tree existed: the
//! viewport was a flat Vec of manual draw commands and
//! `resolve_control_layout`/`arrange_container` were called only from unit
//! tests. This test builds a real Control tree (Panel > ColorRect, plus Button,
//! Label, TextureRect), runs the render pass that walks the tree and resolves
//! anchors/offsets live, rasterizes the resulting draw commands with the
//! software renderer, and asserts that each control produced themed pixels in
//! the framebuffer.

use gdcore::math::{Color, Vector2};
use gdplatform::input::{InputEvent, MouseButton};
use gdrender2d::renderer::{FrameBuffer, SoftwareRenderer};
use gdrender2d::test_adapter::{capture_frame, pixel_at};
use gdrender2d::texture::Texture2D;
use gdscene::control::{self, AnchorPreset};
use gdscene::control_render::render_control_tree;
use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscene::theme::Theme;
use gdserver2d::viewport::Viewport;
use gdvariant::Variant;

fn add(tree: &mut SceneTree, parent: NodeId, name: &str, class: &str) -> NodeId {
    tree.add_child(parent, Node::new(name, class)).unwrap()
}

fn set_prop(tree: &mut SceneTree, id: NodeId, key: &str, value: Variant) {
    tree.get_node_mut(id).unwrap().set_property(key, value);
}

/// Positions a control top-left with an explicit local rect.
fn place(tree: &mut SceneTree, id: NodeId, l: f32, t: f32, r: f32, b: f32) {
    control::apply_anchor_preset(tree, id, AnchorPreset::TopLeft);
    control::set_offset_left(tree, id, l);
    control::set_offset_top(tree, id, t);
    control::set_offset_right(tree, id, r);
    control::set_offset_bottom(tree, id, b);
}

fn close(a: Color, b: Color, tol: f32) -> bool {
    (a.r - b.r).abs() <= tol && (a.g - b.g).abs() <= tol && (a.b - b.b).abs() <= tol
}

/// Returns true if any pixel in the (inclusive) rect matches `color`.
fn region_has(fb: &FrameBuffer, x0: u32, y0: u32, x1: u32, y1: u32, color: Color, tol: f32) -> bool {
    for y in y0..=y1 {
        for x in x0..=x1 {
            if close(pixel_at(fb, x, y), color, tol) {
                return true;
            }
        }
    }
    false
}

#[test]
fn control_tree_renders_themed_pixels() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    // Root Control fills the 200x200 viewport.
    let ui = add(&mut tree, root, "UI", "Control");
    control::apply_anchor_preset(&mut tree, ui, AnchorPreset::FullRect);

    // Panel across the top half; ColorRect nested inside it.
    let panel = add(&mut tree, ui, "Panel", "Panel");
    place(&mut tree, panel, 0.0, 0.0, 200.0, 100.0);
    let cr = add(&mut tree, panel, "Rect", "ColorRect");
    place(&mut tree, cr, 10.0, 10.0, 60.0, 60.0); // local → global (10,10)-(60,60)
    set_prop(&mut tree, cr, "color", Variant::Color(Color::rgb(1.0, 0.0, 0.0)));

    // Button with centered text.
    let btn = add(&mut tree, ui, "Btn", "Button");
    place(&mut tree, btn, 10.0, 120.0, 110.0, 160.0);
    set_prop(&mut tree, btn, "text", Variant::String("OK".to_owned()));

    // Label with text.
    let lbl = add(&mut tree, ui, "Lbl", "Label");
    place(&mut tree, lbl, 10.0, 170.0, 110.0, 190.0);
    set_prop(&mut tree, lbl, "text", Variant::String("HI".to_owned()));

    // TextureRect referencing a registered texture.
    let tex = add(&mut tree, ui, "Tex", "TextureRect");
    place(&mut tree, tex, 120.0, 120.0, 180.0, 180.0);
    set_prop(&mut tree, tex, "texture", Variant::String("icon.png".to_owned()));

    // Render the tree into draw commands, then rasterize.
    let mut renderer = SoftwareRenderer::new();
    renderer.register_texture("icon.png", Texture2D::solid(4, 4, Color::rgb(0.0, 0.0, 1.0)));
    let mut vp = Viewport::new(200, 200, Color::BLACK);
    let theme = Theme::default_theme();
    render_control_tree(&mut tree, ui, &theme, &mut vp, Vector2::new(200.0, 200.0));
    let fb = capture_frame(&mut renderer, &vp);

    let tol = 0.03;

    // Panel background: Godot's default `style_normal_color` (0.1, 0.1, 0.1)
    // at 60% alpha (pat-9ww3e) over the black clear = 0.06 gray, on the top
    // half where the ColorRect does not cover it.
    assert!(
        close(pixel_at(&fb, 150, 50), Color::new(0.06, 0.06, 0.06, 1.0), tol),
        "Panel StyleBox bg should fill its rect, got {:?}",
        pixel_at(&fb, 150, 50)
    );

    // ColorRect draws its property color (red), on top of the panel.
    assert!(
        close(pixel_at(&fb, 30, 30), Color::rgb(1.0, 0.0, 0.0), tol),
        "ColorRect should draw its color over the panel, got {:?}",
        pixel_at(&fb, 30, 30)
    );

    // Button background: Godot's normal StyleBox is `style_normal_color`, the
    // same 60%-alpha (0.1, 0.1, 0.1) as the Panel (pat-9ww3e) -> 0.06 over the
    // black clear, away from the centred text.
    assert!(
        close(pixel_at(&fb, 15, 155), Color::new(0.06, 0.06, 0.06, 1.0), tol),
        "Button StyleBox bg should fill its rect, got {:?}",
        pixel_at(&fb, 15, 155)
    );

    // Button text: themed font_color (~0.875 gray) appears somewhere in the button.
    assert!(
        region_has(&fb, 10, 120, 109, 159, Color::new(0.875, 0.875, 0.875, 1.0), tol),
        "Button should render themed font_color text pixels"
    );

    // Label text: themed font_color (white) appears somewhere in the label rect.
    assert!(
        region_has(&fb, 10, 170, 109, 189, Color::WHITE, tol),
        "Label should render themed font_color text pixels"
    );

    // TextureRect draws the registered texture (blue), tinted by white modulate.
    assert!(
        close(pixel_at(&fb, 150, 150), Color::rgb(0.0, 0.0, 1.0), tol),
        "TextureRect should draw its texture, got {:?}",
        pixel_at(&fb, 150, 150)
    );
}

// ===========================================================================
// pat-0q2kx.86: ScrollContainer clipping, scroll offset, clamping, and wheel.
//
// Before this the draw stack had no clipping primitive at all — `DrawCommand`
// had no clip/scissor variant and `CanvasItem` no clip rect — so a
// ScrollContainer's oversized content drew straight through its container and
// over everything below it. `arrange_scroll` also applied `scroll_*` unclamped,
// so scrolling past the end pushed the content off into blank space.
// ===========================================================================

const RED: Color = Color {
    r: 1.0,
    g: 0.0,
    b: 0.0,
    a: 1.0,
};
const GREEN: Color = Color {
    r: 0.0,
    g: 1.0,
    b: 0.0,
    a: 1.0,
};
const BLUE: Color = Color {
    r: 0.0,
    g: 0.0,
    b: 1.0,
    a: 1.0,
};

/// Returns true if `color` appears anywhere in the frame.
fn frame_contains(fb: &FrameBuffer, color: Color, tol: f32) -> bool {
    (0..fb.height).any(|y| (0..fb.width).any(|x| close(pixel_at(fb, x, y), color, tol)))
}

/// `ScrollContainer(200x100) > VBoxContainer(200x300) > [red, green, blue]`,
/// each band exactly 100px tall, in a 200x200 viewport. The container fills the
/// top half, so the bottom half of the frame is where unclipped overflow would
/// show up.
fn scroll_scene() -> (SceneTree, NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let scroll = add(&mut tree, root, "Scroll", "ScrollContainer");
    place(&mut tree, scroll, 0.0, 0.0, 200.0, 100.0);

    let vbox = add(&mut tree, scroll, "Content", "VBoxContainer");
    // Gapless bands: Godot's default `separation` is 4 (pat-9ww3e), and the
    // scroll arithmetic below is for 3 x 100 = 300px of content.
    set_prop(&mut tree, vbox, "theme_override_constants/separation", Variant::Int(0));
    // A container distributes space from `effective_min`, and Patina does not
    // derive an implicit minimum from children yet, so the content extent is
    // authored — the same accommodation play_view_renders_controls_e2e makes.
    set_prop(
        &mut tree,
        vbox,
        "custom_minimum_size",
        Variant::Vector2(Vector2::new(200.0, 300.0)),
    );

    for (name, color) in [("A", RED), ("B", GREEN), ("C", BLUE)] {
        let band = add(&mut tree, vbox, name, "ColorRect");
        set_prop(&mut tree, band, "color", Variant::Color(color));
        set_prop(
            &mut tree,
            band,
            "custom_minimum_size",
            Variant::Vector2(Vector2::new(200.0, 100.0)),
        );
    }

    (tree, scroll)
}

/// Resolves the container's own rect from its anchors/offsets, then sorts its
/// content into it.
///
/// `sort_container_tree` lays out a container's *children*; the container's own
/// `position`/`size` still come from anchor resolution, which the render pass
/// does on the way down. Tests that skip rendering (the wheel ones) have to do
/// it themselves or the hit test walks a zero-sized rect.
fn layout_scroll(tree: &mut SceneTree, scroll: NodeId) {
    control::resolve_control_layout(tree, scroll, Vector2::new(200.0, 200.0));
    control::sort_container_tree(tree, scroll, Vector2::new(200.0, 100.0));
}

/// Lays the containers out and rasterizes, the two-step the Play view uses
/// (`build_control_render_items`): sort first, then draw.
fn render_scroll(tree: &mut SceneTree, scroll: NodeId) -> FrameBuffer {
    layout_scroll(tree, scroll);
    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(200, 200, Color::BLACK);
    let theme = Theme::default_theme();
    render_control_tree(tree, scroll, &theme, &mut vp, Vector2::new(200.0, 200.0));
    capture_frame(&mut renderer, &vp)
}

fn set_scroll_v(tree: &mut SceneTree, scroll: NodeId, value: f64) {
    set_prop(tree, scroll, "scroll_vertical", Variant::Float(value));
}

/// **The acceptance test.** Clip, scroll, and clamp, asserted on real pixels.
#[test]
fn scroll_container_clips_and_scrolls() {
    let (mut tree, scroll) = scroll_scene();
    let tol = 0.03;

    // (1) Unscrolled: only the first 100px band is on screen. The green and
    // blue bands live below the fold and must be clipped away entirely — not
    // merely covered, so checking the whole frame is the real assertion.
    let fb = render_scroll(&mut tree, scroll);
    assert!(
        close(pixel_at(&fb, 50, 50), RED, tol),
        "the first band must fill the container, got {:?}",
        pixel_at(&fb, 50, 50)
    );
    assert!(
        !frame_contains(&fb, GREEN, tol),
        "the below-fold band must be clipped out of the frame entirely"
    );
    assert!(
        !frame_contains(&fb, BLUE, tol),
        "the third band is further below the fold and must also be clipped"
    );
    assert!(
        close(pixel_at(&fb, 50, 150), Color::BLACK, tol),
        "content must not overflow past the container's bottom edge, got {:?}",
        pixel_at(&fb, 50, 150)
    );

    // (2) Scrolling by exactly one band brings the next one into view and takes
    // the first one out.
    set_scroll_v(&mut tree, scroll, 100.0);
    let fb = render_scroll(&mut tree, scroll);
    assert!(
        close(pixel_at(&fb, 50, 50), GREEN, tol),
        "scroll_vertical = 100 must show the second band, got {:?}",
        pixel_at(&fb, 50, 50)
    );
    assert!(
        !frame_contains(&fb, RED, tol),
        "the scrolled-past band must be clipped above the container"
    );

    // (3) Scrolling past the end clamps to content - viewport = 300 - 100.
    set_scroll_v(&mut tree, scroll, 999.0);
    let fb = render_scroll(&mut tree, scroll);
    assert_eq!(
        control::get_scroll(&tree, scroll).y,
        200.0,
        "scroll must clamp to content - viewport, not run past the content"
    );
    assert!(
        close(pixel_at(&fb, 50, 50), BLUE, tol),
        "clamped scroll parks on the last band, got {:?}",
        pixel_at(&fb, 50, 50)
    );
}

/// The vertical scrollbar renders, sized and positioned from the Range model:
/// the grabber covers the visible fraction of the content (100/300 of the
/// track) and slides from top to bottom as the scroll offset does.
///
/// It must also survive the content — the bars are drawn after the children, so
/// a band that spans the container's full width cannot bury them.
#[test]
fn scroll_container_draws_a_proportional_scrollbar() {
    let (mut tree, scroll) = scroll_scene();
    let grabber = Color::new(0.45, 0.45, 0.45, 1.0);
    let trough = Color::new(0.1, 0.1, 0.1, 1.0);
    let tol = 0.03;
    // The bar occupies the container's right edge; sample its middle column.
    let x = 195;

    let fb = render_scroll(&mut tree, scroll);
    assert!(
        close(pixel_at(&fb, x, 10), grabber, tol),
        "at the top the grabber must sit at the top of the track, got {:?}",
        pixel_at(&fb, x, 10)
    );
    assert!(
        close(pixel_at(&fb, x, 90), trough, tol),
        "the rest of the track is trough, got {:?}",
        pixel_at(&fb, x, 90)
    );

    // Fully scrolled, the grabber is at the bottom of the track.
    set_scroll_v(&mut tree, scroll, 200.0);
    let fb = render_scroll(&mut tree, scroll);
    assert!(
        close(pixel_at(&fb, x, 90), grabber, tol),
        "fully scrolled, the grabber must reach the bottom, got {:?}",
        pixel_at(&fb, x, 90)
    );
    assert!(
        close(pixel_at(&fb, x, 10), trough, tol),
        "and leave the top of the track empty, got {:?}",
        pixel_at(&fb, x, 10)
    );
}

/// Content that fits needs no bar at all, as in Godot.
#[test]
fn scroll_container_without_overflow_draws_no_scrollbar() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let scroll = add(&mut tree, root, "Scroll", "ScrollContainer");
    place(&mut tree, scroll, 0.0, 0.0, 200.0, 100.0);
    let inner = add(&mut tree, scroll, "Content", "ColorRect");
    set_prop(&mut tree, inner, "color", Variant::Color(RED));
    set_prop(
        &mut tree,
        inner,
        "custom_minimum_size",
        Variant::Vector2(Vector2::new(200.0, 60.0)),
    );

    let fb = render_scroll(&mut tree, scroll);
    assert!(
        close(pixel_at(&fb, 195, 50), RED, 0.03),
        "with nothing to scroll the right edge is content, not a bar, got {:?}",
        pixel_at(&fb, 195, 50)
    );
}

/// A control without `clip_contents` still lets its children overflow — the
/// clip must come from the flag, not from being a parent.
#[test]
fn a_non_clipping_control_does_not_clip_its_children() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let panel = add(&mut tree, root, "Panel", "Panel");
    place(&mut tree, panel, 0.0, 0.0, 100.0, 50.0);
    let cr = add(&mut tree, panel, "Over", "ColorRect");
    place(&mut tree, cr, 0.0, 0.0, 100.0, 150.0);
    set_prop(&mut tree, cr, "color", Variant::Color(GREEN));

    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(200, 200, Color::BLACK);
    let theme = Theme::default_theme();
    render_control_tree(
        &mut tree,
        panel,
        &theme,
        &mut vp,
        Vector2::new(200.0, 200.0),
    );
    let fb = capture_frame(&mut renderer, &vp);

    assert!(
        close(pixel_at(&fb, 50, 120), GREEN, 0.03),
        "a plain Panel must not clip its child, got {:?}",
        pixel_at(&fb, 50, 120)
    );
}

/// `clip_contents = true` on any Control clips it, not just ScrollContainer.
#[test]
fn clip_contents_flag_clips_any_control() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let panel = add(&mut tree, root, "Panel", "Panel");
    place(&mut tree, panel, 0.0, 0.0, 100.0, 50.0);
    set_prop(&mut tree, panel, "clip_contents", Variant::Bool(true));
    let cr = add(&mut tree, panel, "Over", "ColorRect");
    place(&mut tree, cr, 0.0, 0.0, 100.0, 150.0);
    set_prop(&mut tree, cr, "color", Variant::Color(GREEN));

    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(200, 200, Color::BLACK);
    let theme = Theme::default_theme();
    render_control_tree(
        &mut tree,
        panel,
        &theme,
        &mut vp,
        Vector2::new(200.0, 200.0),
    );
    let fb = capture_frame(&mut renderer, &vp);

    assert!(
        close(pixel_at(&fb, 50, 25), GREEN, 0.03),
        "inside the clip the child still draws, got {:?}",
        pixel_at(&fb, 50, 25)
    );
    assert!(
        close(pixel_at(&fb, 50, 120), Color::BLACK, 0.03),
        "clip_contents must cut the child off at the parent's edge, got {:?}",
        pixel_at(&fb, 50, 120)
    );
}

/// A wheel event anywhere over the container scrolls it, by Godot's
/// `page / 8` notch, and clamps at the end of the content.
#[test]
fn mouse_wheel_scrolls_the_container_under_the_cursor() {
    let (mut tree, scroll) = scroll_scene();
    // Lay out first so the hit test has real rects to walk.
    layout_scroll(&mut tree, scroll);

    let wheel = |down: bool| InputEvent::MouseButton {
        button: if down {
            MouseButton::WheelDown
        } else {
            MouseButton::WheelUp
        },
        pressed: true,
        position: Vector2::new(50.0, 50.0),
    };

    // page / PAGE_DIVISOR = 100 / 8 = 12.5 per notch.
    tree.push_input(&wheel(true));
    tree.push_input(&wheel(true));
    assert_eq!(
        control::get_scroll(&tree, scroll).y,
        25.0,
        "two wheel-down notches must scroll 2 * (viewport / 8)"
    );

    // Wheeling back up returns to the top and stops there — no negative scroll.
    for _ in 0..10 {
        tree.push_input(&wheel(false));
    }
    assert_eq!(
        control::get_scroll(&tree, scroll).y,
        0.0,
        "scrolling up must clamp at the top"
    );

    // And wheeling far past the end parks at content - viewport.
    for _ in 0..50 {
        tree.push_input(&wheel(true));
    }
    assert_eq!(
        control::get_scroll(&tree, scroll).y,
        200.0,
        "scrolling down must clamp at content - viewport"
    );
}

/// A wheel over content that fits needs no scrolling, so the GUI stage must
/// leave the event for gameplay rather than swallowing it.
#[test]
fn wheel_over_non_scrolling_content_is_not_consumed() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let scroll = add(&mut tree, root, "Scroll", "ScrollContainer");
    place(&mut tree, scroll, 0.0, 0.0, 200.0, 100.0);
    let vbox = add(&mut tree, scroll, "Content", "VBoxContainer");
    // Gapless bands: Godot's default `separation` is 4 (pat-9ww3e), and the
    // scroll arithmetic below is for 3 x 100 = 300px of content.
    set_prop(&mut tree, vbox, "theme_override_constants/separation", Variant::Int(0));
    set_prop(
        &mut tree,
        vbox,
        "custom_minimum_size",
        Variant::Vector2(Vector2::new(200.0, 50.0)),
    );
    layout_scroll(&mut tree, scroll);

    tree.push_input(&InputEvent::MouseButton {
        button: MouseButton::WheelDown,
        pressed: true,
        position: Vector2::new(50.0, 50.0),
    });

    assert_eq!(control::get_scroll(&tree, scroll).y, 0.0);
    assert!(
        !tree.is_input_handled(),
        "content that fits has nothing to scroll, so the wheel must fall \
         through to _unhandled_input"
    );
}

#[test]
fn anchors_resolved_live_fullrect_fills_viewport() {
    // A single ColorRect with a FullRect preset must fill the whole viewport,
    // proving anchors/offsets are resolved during the render pass (not baked).
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let cr = add(&mut tree, root, "Full", "ColorRect");
    control::apply_anchor_preset(&mut tree, cr, AnchorPreset::FullRect);
    set_prop(&mut tree, cr, "color", Variant::Color(Color::rgb(0.0, 1.0, 0.0)));

    let mut renderer = SoftwareRenderer::new();
    let mut vp = Viewport::new(48, 48, Color::BLACK);
    let theme = Theme::default_theme();
    render_control_tree(&mut tree, cr, &theme, &mut vp, Vector2::new(48.0, 48.0));
    let fb = capture_frame(&mut renderer, &vp);

    // Corners and center should all be green.
    for (x, y) in [(0, 0), (47, 0), (0, 47), (47, 47), (24, 24)] {
        assert!(
            close(pixel_at(&fb, x, y), Color::rgb(0.0, 1.0, 0.0), 0.03),
            "FullRect ColorRect should fill ({x},{y}), got {:?}",
            pixel_at(&fb, x, y)
        );
    }
}
