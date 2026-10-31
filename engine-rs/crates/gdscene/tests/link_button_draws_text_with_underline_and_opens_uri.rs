//! pat-s3rrv.166 acceptance: `LinkButton` paints its text with an underline and
//! asks the host to open its `uri` when pressed.
//!
//! LinkButton was registered in ClassDB (`class_db.rs:19260`, with `text`,
//! `underline` and `uri`) and present in three class-name fallback lists —
//! `BUTTON_CLASSES`, `CONTROL_HIT_CLASSES` (`control.rs:910`) and
//! `FOCUSABLE_BY_DEFAULT`. It was **not** in `CONTROL_CLASSES`
//! (`control_render.rs:42`), the list that gates the draw walk. The result was
//! the worst combination: an invisible rectangle that took clicks and tab
//! focus. Neither `uri` nor `underline` had any behaviour at all.
//!
//! Ported from `upstream/godot/scene/gui/link_button.cpp`.
//!
//! # Why the geometry below is exact rather than approximate
//!
//! ⚠️ REVISED for pat-mgixk.66. This note used to derive every number from the
//! 5x7 bitmap constants — `text_width = (4*5 + 3*1) * 2` = **46**, `glyph_h =
//! 7 * 2` = **14**, underline `y = 14 + 2` = **16**. Default text is now drawn
//! and measured with the embedded TTF face, so those literals described a rule
//! that no longer matches the glyphs underneath it.
//!
//! The width is therefore taken from `TtfFont::builtin().measure_line("Docs",
//! 16.0)` — the public spelling of the computation `control_render` itself uses,
//! so the expectation tracks the face instead of restating a constant. The
//! vertical offset is `line_height_px(16) + thickness(2)` = **18**.
//!
//! ⚠️ The Line's ENDPOINTS are still asserted, not merely its presence. Clauses
//! 3 and 4 only require a Line to appear and disappear, which a stub emitting a
//! zero-length or full-rect line would satisfy; `to.x == docs_w()` is what proves
//! the rule spans the TEXT (not the 120-wide control rect), and `y == 18` that it
//! sits under the baseline rather than through the glyphs.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use gdcore::id::ObjectId;
use gdobject::signal::Connection;
use gdplatform::input::InputEvent;

use gdcore::math::Vector2;
use gdscene::control::{
    click_at_point, resolve_control_layout, set_offset_bottom, set_offset_left, set_offset_right,
    set_offset_top,
};
use gdscene::control_render::{control_draw_items, ControlDrawItem};
use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscene::Theme;
use gdvariant::Variant;

/// `UnderlineMode` (`upstream/godot/scene/gui/link_button.h:40`).
const ALWAYS: i64 = 0;
const ON_HOVER: i64 = 1;
const NEVER: i64 = 2;

/// Text width of `"Docs"` at the default font size.
///
/// ⚠️ DERIVED from the real face, not the 5x7 bitmap constants (pat-mgixk.66).
/// This was a hard-coded `46.0` computed from `GLYPH_W/GLYPH_GAP * text_scale`,
/// which is what the underline used to span. The rasterizer now draws default
/// text with the embedded TTF, and `control_render` measures the rule with the
/// same face — so a bitmap-derived literal here would assert the rule is the
/// wrong length on purpose.
///
/// `TtfFont::builtin().measure_line` is the public spelling of the very
/// computation `control_render::measure_text_px` performs (that helper is
/// `pub(crate)` and an integration test cannot call it), so the two agree
/// bit-for-bit rather than approximately.
fn docs_w() -> f32 {
    gdrender2d::TtfFont::builtin().measure_line("Docs", 16.0)
}

/// Underline offset below a top-left-anchored label at the default font size.
///
/// `line_height_px(16) + thickness(2)`. The line height is now the authored
/// pixel size rather than `GLYPH_H * text_scale` (= 14), so this moved 16 -> 18.
const UNDERLINE_Y: f32 = 18.0;

/// A tree holding one `LinkButton` at the origin, 120x24.
///
/// Hand-built rather than loaded from a `.tscn` so the rect is exact: the
/// numbers above are only derivable if the node sits at a known origin.
fn tree_with_link(underline: Option<i64>, uri: &str) -> (SceneTree, NodeId) {
    gdobject::class_db::register_all_classes();
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let mut link = Node::new("Docs", "LinkButton");
    link.set_property("text", Variant::String("Docs".into()));
    if let Some(mode) = underline {
        link.set_property("underline", Variant::Int(mode));
    }
    if !uri.is_empty() {
        link.set_property("uri", Variant::String(uri.into()));
    }
    let id = tree.add_child(root, link).expect("link added");
    // 🔑 The rect is expressed as OFFSETS, not by writing `position`/`size`
    // directly. `size` is DERIVED: the draw walk calls
    // `control::resolve_control_layout` (`control_render.rs:558`), which
    // recomputes the rect from anchors + offsets and OVERWRITES whatever
    // `set_size` wrote. With every anchor and offset at zero it collapses to the
    // minimum size, so a fixture that sets `size` directly is 120x24 until the
    // first draw and 0x0 afterwards — the hit test then misses and hover never
    // registers, which is exactly how the ON_HOVER clause failed the first time.
    //
    // Anchors default to 0, so `x1 = offset_left`, `x2 = offset_right`: these
    // four make resolution idempotent at (0, 0) 120x24.
    set_offset_left(&mut tree, id, 0.0);
    set_offset_top(&mut tree, id, 0.0);
    set_offset_right(&mut tree, id, 120.0);
    set_offset_bottom(&mut tree, id, 24.0);
    // ⚠️ And resolve once here, because the HIT TEST does not: `control_at_point`
    // reads the `size` property as-is, so before any draw a hand-built control
    // is 0x0 and every click misses. That failure is silent — a `click_at_point`
    // that hits nothing simply does nothing, so an assertion like "no shell_open
    // was requested" passes for the wrong reason.
    resolve_control_layout(&mut tree, id, Vector2::new(320.0, 200.0));
    (tree, id)
}

fn items(tree: &mut SceneTree) -> Vec<ControlDrawItem> {
    let root = tree.root_id();
    control_draw_items(tree, root, &Theme::default(), Vector2::new(320.0, 200.0))
}

/// The `Text` item's string, if one was emitted.
fn text_of(items: &[ControlDrawItem]) -> Option<String> {
    items.iter().find_map(|i| match i {
        ControlDrawItem::Text { text, .. } => Some(text.clone()),
        _ => None,
    })
}

/// The first `Line` item's `(from, to, width)`, if one was emitted.
fn line_of(items: &[ControlDrawItem]) -> Option<(Vector2, Vector2, f32)> {
    items.iter().find_map(|i| match i {
        ControlDrawItem::Line {
            from, to, width, ..
        } => Some((*from, *to, *width)),
        _ => None,
    })
}

fn motion(x: f32, y: f32) -> InputEvent {
    InputEvent::MouseMotion {
        position: Vector2::new(x, y),
        relative: Vector2::ZERO,
    }
}

/// **The acceptance.** Text plus an underline, and the uri reaches the host.
#[test]
fn link_button_draws_text_with_underline_and_opens_uri() {
    // -- clause 2: a Text item and a Line beneath it ------------------------
    let (mut tree, _id) = tree_with_link(None, "");
    let drawn = items(&mut tree);
    assert_eq!(
        text_of(&drawn).as_deref(),
        Some("Docs"),
        "the label must be painted — before this bead the draw walk skipped \
         LinkButton entirely and emitted nothing: {drawn:?}"
    );
    let (from, to, width) = line_of(&drawn).expect("an underline Line item");
    assert_eq!(from, Vector2::new(0.0, UNDERLINE_Y));
    assert_eq!(
        to,
        Vector2::new(docs_w(), UNDERLINE_Y),
        "the rule spans the TEXT width, not the 120-wide control rect — a \
         stretched LinkButton must not trail a line past its own label"
    );
    assert_eq!(width, 2.0);

    // ⓘ An absent `underline` property means ALWAYS: ClassDB registers the
    // default as Int(0), and a LinkButton that never sets it is underlined.
    // The fixture above omitted the property entirely, so the Line proves it.

    // -- clause 4: NEVER emits no Line, ever --------------------------------
    let (mut tree, _) = tree_with_link(Some(NEVER), "");
    let drawn = items(&mut tree);
    assert_eq!(
        text_of(&drawn).as_deref(),
        Some("Docs"),
        "NEVER suppresses the underline, not the text"
    );
    assert!(
        line_of(&drawn).is_none(),
        "underline = NEVER must emit no Line: {drawn:?}"
    );

    // -- clause 3: ON_HOVER follows the pointer -----------------------------
    let (mut tree, _) = tree_with_link(Some(ON_HOVER), "");
    let cold = items(&mut tree);
    assert!(
        line_of(&cold).is_none(),
        "ON_HOVER must not underline while the pointer is elsewhere: {cold:?}"
    );

    // 🔑 Hover is driven through a real motion event, so this exercises the
    // same `SceneTree::hovered_control` the draw arm reads — poking the field
    // would prove only that the arm reads a field, not that hovering sets it.
    tree.push_input(&motion(60.0, 12.0));
    let hot = items(&mut tree);
    assert!(
        line_of(&hot).is_some(),
        "ON_HOVER must underline once the pointer is inside the rect: {hot:?}"
    );
    // The pair is what makes each half meaningful: absent-then-present rules
    // out both "never draws" and "always draws".
    assert_eq!(
        text_of(&hot).as_deref(),
        Some("Docs"),
        "hovering changes the underline, not the label"
    );

    // -- clause 5: pressing records the uri for the host --------------------
    let (mut tree, id) = tree_with_link(Some(ALWAYS), "https://example.com");
    assert!(
        tree.take_shell_open_requests().is_empty(),
        "nothing is requested before the press"
    );
    click_at_point(&mut tree, Vector2::new(20.0, 12.0));
    assert_eq!(
        tree.take_shell_open_requests(),
        vec!["https://example.com".to_string()],
        "pressing a LinkButton with a uri must ask the host to open it"
    );
    // ⚠️ Drained, so a second press is a second request rather than a repeat
    // of the first — and an un-pressed LinkButton asks for nothing.
    assert!(
        tree.take_shell_open_requests().is_empty(),
        "the request queue drains"
    );
    let _ = id;

    // -- clause 6: the `pressed` signal still fires -------------------------
    //
    // 🔑 Godot's `LinkButton::pressed()` is a BaseButton virtual that runs
    // ALONGSIDE the signal (`link_button.cpp:185`), so opening the uri and
    // emitting `pressed` are independent. The bead described the uri as opening
    // only when no handler is connected; upstream has no such condition, and
    // implementing one would make a LinkButton stop navigating the moment a
    // script connected a listener.
    let (mut tree, id) = tree_with_link(Some(ALWAYS), "https://example.com");
    let seen = Arc::new(AtomicUsize::new(0));
    let flag = seen.clone();
    tree.connect_signal(
        id,
        "pressed",
        Connection::with_callback(ObjectId::from_raw(9001), "on_pressed", move |_args| {
            flag.fetch_add(1, Ordering::SeqCst);
            Variant::Nil
        }),
    );
    click_at_point(&mut tree, Vector2::new(20.0, 12.0));
    assert_eq!(
        seen.load(Ordering::SeqCst),
        1,
        "a connected `pressed` handler must still fire"
    );
    assert_eq!(
        tree.take_shell_open_requests(),
        vec!["https://example.com".to_string()],
        "and the uri is still requested — the handler does not suppress it"
    );
}

/// An empty `uri` asks for nothing.
///
/// ⚠️ Godot returns early on an empty uri (`link_button.cpp:186`). Without this
/// a press on every ordinary LinkButton would queue an empty request, and the
/// host would be handed `""` to open.
#[test]
fn a_link_button_without_a_uri_requests_nothing() {
    let (mut tree, _) = tree_with_link(Some(ALWAYS), "");
    // The click must actually land, or "nothing was requested" is vacuous.
    assert!(
        gdscene::control::control_at_point(&tree, Vector2::new(20.0, 12.0)).is_some(),
        "the fixture must be clickable"
    );
    click_at_point(&mut tree, Vector2::new(20.0, 12.0));
    assert!(
        tree.take_shell_open_requests().is_empty(),
        "an empty uri must not be queued"
    );
}

/// A plain Button is unaffected — the uri path is LinkButton-only.
///
/// Without this the press hook could be firing for every BaseButton and the
/// assertions above would not notice.
#[test]
fn a_plain_button_never_requests_a_shell_open() {
    gdobject::class_db::register_all_classes();
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let mut btn = Node::new("Plain", "Button");
    btn.set_property("text", Variant::String("Docs".into()));
    // Even carrying a `uri`, which a Button has no business acting on.
    btn.set_property("uri", Variant::String("https://example.com".into()));
    let id = tree.add_child(root, btn).expect("button added");
    set_offset_left(&mut tree, id, 0.0);
    set_offset_top(&mut tree, id, 0.0);
    set_offset_right(&mut tree, id, 120.0);
    set_offset_bottom(&mut tree, id, 24.0);
    resolve_control_layout(&mut tree, id, Vector2::new(320.0, 200.0));

    // The control is genuinely hit-testable here, so "requests nothing" means
    // the LinkButton gate refused it — not that the click missed.
    assert!(
        gdscene::control::control_at_point(&tree, Vector2::new(20.0, 12.0)).is_some(),
        "the fixture must actually be clickable, or the assertion below is vacuous"
    );
    click_at_point(&mut tree, Vector2::new(20.0, 12.0));
    assert!(
        tree.take_shell_open_requests().is_empty(),
        "only a LinkButton opens a uri"
    );
}
