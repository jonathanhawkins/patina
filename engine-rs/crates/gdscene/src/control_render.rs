//! Control-tree render pass (pat-u3l8c.67).
//!
//! Bridges the Control layout functions in [`crate::control`] (previously
//! exercised only from tests) to gdserver2d's draw-command `Viewport`.
//! [`render_control_tree`] walks a [`SceneTree`]'s Control subtree, resolves
//! each control's rect **live** from its anchors/offsets against the parent
//! rect, looks up themed colors and StyleBoxes, and emits [`DrawCommand`]s so a
//! 2D renderer produces themed pixels for `Button`/`Label`/`Panel`/`ColorRect`/
//! `TextureRect`.
//!
//! Positions accumulate down the tree into global (viewport) space; each
//! control becomes one [`CanvasItem`] added in tree pre-order so children paint
//! over their parents (painter's algorithm), matching Godot's draw order.

use std::collections::HashMap;

use gdcore::math::{Color, Rect2, Vector2};
use gdserver2d::canvas::{CanvasItem, CanvasItemId, DrawCommand};
use gdserver2d::viewport::Viewport;
use gdvariant::Variant;

use crate::char_fx::{BuiltinEffect, CharFXTransform};
use crate::control;
use crate::node::NodeId;
use crate::scene_tree::SceneTree;
use crate::syntax_highlighter::CodeHighlighter;
use crate::theme::{Theme, ThemePropertyType};

/// Nominal glyph metrics of the built-in 5×7 bitmap font used by the software
/// renderer's `DrawString`. Used to place/centre control text.
pub(crate) const GLYPH_W: f32 = 5.0;
pub(crate) const GLYPH_H: f32 = 7.0;
pub(crate) const GLYPH_GAP: f32 = 1.0;

/// Width of a ScrollContainer's scrollbars, in pixels (pat-0q2kx.86). Stands in
/// for Godot's `scroll` StyleBox minimum size, which the default theme sets to
/// roughly this.
pub(crate) const SCROLLBAR_THICKNESS: f32 = 8.0;

/// Control classes handled by the render pass. Used as a fallback membership
/// test in case the ClassDB inheritance chain is unavailable for `is_class`.
const CONTROL_CLASSES: &[&str] = &[
    "Control",
    "Panel",
    "PanelContainer",
    "Button",
    "Label",
    "ColorRect",
    // pat-0q2kx.245 — like the entries below, this membership fallback is what
    // keeps `is_control()` from rejecting the node when the ClassDB Control
    // chain is unavailable, which would skip the draw arm entirely.
    "ReferenceRect",
    "TextureRect",
    "RichTextLabel",
    // pat-nzgdu.198 — TextEdit/CodeEdit draw their text (and syntax-highlighted
    // keyword spans) in draw_control; without them here is_control() rejects the
    // node when the ClassDB Control chain is unavailable, and the draw arm never
    // runs.
    "TextEdit",
    "CodeEdit",
    // pat-0q2kx.87 — a LineEdit draws its text, selection and caret in
    // draw_control; without it here `is_control` rejects the node whenever the
    // ClassDB Control chain is unavailable and the arm never runs.
    "LineEdit",
    "ProgressBar",
    "CheckBox",
    "CheckButton",
    "OptionButton",
    // pat-0q2kx.85 — the item views draw rows/tabs in `draw_control`; without
    // them here `is_control` rejects the node whenever the ClassDB Control chain
    // is unavailable and the arm never runs.
    "ItemList",
    "TabBar",
    "TabContainer",
    "MarginContainer",
    "ScrollContainer",
    "GridContainer",
    "VBoxContainer",
    "HBoxContainer",
    "CenterContainer",
    // pat-nzgdu.197. `Separator` itself is abstract and never instantiated, but
    // it stays here so a subclass reaching the fallback still counts as a
    // Control when the ClassDB chain is unavailable.
    "Separator",
    "HSeparator",
    "VSeparator",
];

/// One primitive of a control's drawing, in global viewport space, expressed
/// without any `gdserver2d` type (pat-0q2kx.19).
///
/// [`render_control_tree`] emits `gdserver2d` [`DrawCommand`]s, which only hosts
/// that depend on that crate can consume — the editor's 2D rasterizer does not.
/// [`control_draw_items`] returns this instead, so a host can render real themed
/// controls by mapping seven small cases onto whatever primitives it already has,
/// rather than reimplementing per-control drawing.
///
/// `z` is the paint order assigned by the tree walk (pre-order: parents first),
/// and items are returned already sorted by it.
#[derive(Debug, Clone, PartialEq)]
pub enum ControlDrawItem {
    /// A filled or outlined rectangle — styleboxes, panels, progress fills.
    Rect {
        /// Rectangle in global viewport space.
        rect: Rect2,
        /// Fill (or stroke) color.
        color: Color,
        /// `true` fills the rect, `false` strokes its outline.
        filled: bool,
        /// Paint order.
        z: i32,
    },
    /// A circle — radio buttons, round indicators.
    Circle {
        /// Center in global viewport space.
        center: Vector2,
        /// Radius in pixels.
        radius: f32,
        /// Fill color.
        color: Color,
        /// Paint order.
        z: i32,
    },
    /// A line segment — separators, focus underlines, check marks.
    Line {
        /// Start point in global viewport space.
        from: Vector2,
        /// End point in global viewport space.
        to: Vector2,
        /// Stroke color.
        color: Color,
        /// Stroke width in pixels.
        width: f32,
        /// Paint order.
        z: i32,
    },
    /// Text — button labels, `Label` content, tab titles.
    Text {
        /// The string to draw.
        text: String,
        /// Top-left position in global viewport space.
        position: Vector2,
        /// Text color.
        color: Color,
        /// Font size.
        font_size: u32,
        /// The `theme_override_fonts/font` resource path this control asked for,
        /// or `None` to use the renderer's default face (pat-0q2kx.146).
        font_path: Option<String>,
        /// Paint order.
        z: i32,
    },
    /// A texture stretched into a rect — theme icons, `TextureRect`. Nine-patch
    /// styleboxes arrive here too, flattened to their destination rect: a host
    /// without 9-slice support still draws the texture rather than nothing.
    Texture {
        /// Source texture path.
        texture_path: String,
        /// Destination rectangle in global viewport space.
        rect: Rect2,
        /// Tint.
        modulate: Color,
        /// Paint order.
        z: i32,
    },
}

/// Lays out and draws the Control subtree at `root`, returning its drawing as
/// [`ControlDrawItem`]s in paint order (pat-0q2kx.19).
///
/// Same layout and theming as [`render_control_tree`] — it *is* that function,
/// with the resulting canvas items flattened — so hosts get real themed controls
/// without duplicating a single per-control drawing rule.
pub fn control_draw_items(
    tree: &mut SceneTree,
    root: NodeId,
    theme: &Theme,
    viewport_size: Vector2,
) -> Vec<ControlDrawItem> {
    let mut viewport = Viewport::new(
        viewport_size.x.max(1.0) as u32,
        viewport_size.y.max(1.0) as u32,
        Color::new(0.0, 0.0, 0.0, 0.0),
    );
    let mut fonts: std::collections::HashMap<u64, String> = std::collections::HashMap::new();
    render_control_tree_with_fonts(tree, root, theme, &mut viewport, viewport_size, &mut fonts);

    let mut out = Vec::new();
    // `get_sorted_items` orders by z, which the walk assigned in pre-order, so
    // the flattened stream is already in paint order.
    for item in viewport.get_sorted_items() {
        let z = item.z_index;
        for command in &item.commands {
            match command {
                DrawCommand::DrawRect {
                    rect,
                    color,
                    filled,
                } => out.push(ControlDrawItem::Rect {
                    rect: *rect,
                    color: *color,
                    filled: *filled,
                    z,
                }),
                DrawCommand::DrawCircle {
                    center,
                    radius,
                    color,
                } => out.push(ControlDrawItem::Circle {
                    center: *center,
                    radius: *radius,
                    color: *color,
                    z,
                }),
                DrawCommand::DrawLine {
                    from,
                    to,
                    color,
                    width,
                } => out.push(ControlDrawItem::Line {
                    from: *from,
                    to: *to,
                    color: *color,
                    width: *width,
                    z,
                }),
                DrawCommand::DrawString {
                    text,
                    position,
                    color,
                    font_size,
                } => out.push(ControlDrawItem::Text {
                    text: text.clone(),
                    position: *position,
                    color: *color,
                    font_size: *font_size,
                    // The walk recorded the override per canvas item, because a
                    // `DrawCommand` carries no node identity and `CanvasItem`
                    // carries no node id (pat-0q2kx.146).
                    font_path: fonts.get(&item.id.0).cloned(),
                    z,
                }),
                DrawCommand::DrawTextureRect {
                    texture_path,
                    rect,
                    modulate,
                }
                | DrawCommand::DrawTextureRegion {
                    texture_path,
                    rect,
                    modulate,
                    ..
                }
                | DrawCommand::DrawNinePatch {
                    texture_path,
                    rect,
                    modulate,
                    ..
                } => out.push(ControlDrawItem::Texture {
                    texture_path: texture_path.clone(),
                    rect: *rect,
                    modulate: *modulate,
                    z,
                }),
                // The stroke/fill family (polyline, multiline, dashed line, arc,
                // polygon — pat-0q2kx.129) has no `ControlDrawItem` counterpart:
                // this list models what a *theme* draws, and no Control theme
                // emits those. A script's `_draw` reaches the framebuffer
                // through the renderer's own command path instead, so nothing
                // is lost by skipping them here.
                _ => {}
            }
        }
    }
    out
}

/// Renders the Control subtree rooted at `root` into `viewport`.
///
/// `viewport_size` is the rect the root control anchors against (typically the
/// framebuffer size). Each control's layout is resolved live from its anchors
/// and offsets; global positions accumulate down the tree. Non-control nodes
/// are transparent to layout — their children inherit the same parent rect.
pub fn render_control_tree(
    tree: &mut SceneTree,
    root: NodeId,
    theme: &Theme,
    viewport: &mut Viewport,
    viewport_size: Vector2,
) {
    let mut fonts = std::collections::HashMap::new();
    render_control_tree_with_fonts(tree, root, theme, viewport, viewport_size, &mut fonts);
}

/// [`render_control_tree`] that also records each canvas item's
/// `theme_override_fonts/font` (pat-0q2kx.146).
///
/// The map is keyed by `CanvasItemId` because that is the only identity that
/// survives into the flattened draw stream: a `DrawCommand` carries no font
/// field (adding one touches 41 call sites) and `CanvasItem` carries no node id,
/// so the override has to be recorded during the walk — the one place where both
/// the node and the item id are in hand.
pub fn render_control_tree_with_fonts(
    tree: &mut SceneTree,
    root: NodeId,
    theme: &Theme,
    viewport: &mut Viewport,
    viewport_size: Vector2,
    fonts: &mut std::collections::HashMap<u64, String>,
) {
    let mut order: u64 = 0;
    render_node(
        tree,
        root,
        theme,
        viewport,
        Vector2::ZERO,
        viewport_size,
        &mut order,
        None,
        false,
        fonts,
    );
}

/// Whether this control cuts its own drawing and its descendants off at its
/// rect — Godot's `Control.clip_contents` (pat-0q2kx.86).
///
/// ScrollContainer clips unconditionally: `ScrollContainer::_notification`
/// forces `set_clip_contents(true)` in its constructor
/// (upstream/godot/scene/gui/scroll_container.cpp), and a `.tscn` never
/// serializes that, so keying only off the property would leave every authored
/// ScrollContainer spilling its content.
fn clips_contents(tree: &SceneTree, node_id: NodeId, class: &str) -> bool {
    class == "ScrollContainer" || bool_prop(tree, node_id, "clip_contents").unwrap_or(false)
}

/// The overlap of two rects, empty when they do not overlap. A nested clip can
/// only shrink what an ancestor already allowed.
fn rect_intersection(a: Rect2, b: Rect2) -> Rect2 {
    let x0 = a.position.x.max(b.position.x);
    let y0 = a.position.y.max(b.position.y);
    let x1 = (a.position.x + a.size.x).min(b.position.x + b.size.x);
    let y1 = (a.position.y + a.size.y).min(b.position.y + b.size.y);
    Rect2::new(
        Vector2::new(x0, y0),
        Vector2::new((x1 - x0).max(0.0), (y1 - y0).max(0.0)),
    )
}

fn is_control(tree: &SceneTree, node_id: NodeId) -> bool {
    tree.get_node(node_id)
        .map(|n| n.is_class("Control") || CONTROL_CLASSES.contains(&n.class_name()))
        .unwrap_or(false)
}

fn class_of(tree: &SceneTree, node_id: NodeId) -> String {
    tree.get_node(node_id)
        .map(|n| n.class_name().to_string())
        .unwrap_or_default()
}

fn children_of(tree: &SceneTree, node_id: NodeId) -> Vec<NodeId> {
    tree.get_node(node_id)
        .map(|n| n.children().to_vec())
        .unwrap_or_default()
}

fn render_node(
    tree: &mut SceneTree,
    node_id: NodeId,
    theme: &Theme,
    viewport: &mut Viewport,
    parent_global: Vector2,
    parent_size: Vector2,
    order: &mut u64,
    clip: Option<Rect2>,
    parent_is_container: bool,
    fonts: &mut std::collections::HashMap<u64, String>,
) {
    if is_control(tree, node_id) {
        // Resolve this control's rect from anchors/offsets against the parent
        // rect. resolve_control_layout stores a parent-relative position.
        //
        // A container-managed child is the exception: the container already
        // owns its rect (`Container::fit_child_in_rect` — Godot does not consult
        // such a child's anchors either). Re-deriving here would snap every
        // VBox child back to its own anchor rect, collapsing the stack onto one
        // row, and would undo the `-scroll` offset `arrange_scroll` just applied
        // so a ScrollContainer could never be observed to scroll (pat-0q2kx.86).
        //
        // This assumes the container has been sorted, which is the documented
        // order for the render pass: `build_control_render_items` sorts every
        // container before drawing.
        if !parent_is_container {
            control::resolve_control_layout(tree, node_id, parent_size);
        }
        let local_pos = control::get_position(tree, node_id);
        let size = control::get_size(tree, node_id);
        let global_pos = parent_global + local_pos;
        let rect = Rect2::new(global_pos, size);

        // pat-0q2kx.86: a clipping control narrows the clip for itself and for
        // everything below it. Items are emitted flat here (no parent/child
        // links, `z_index` alone orders them), so each one has to carry the
        // resolved clip rather than inherit it at raster time.
        let clip = if clips_contents(tree, node_id, &class_of(tree, node_id)) {
            Some(match clip {
                Some(outer) => rect_intersection(outer, rect),
                None => rect,
            })
        } else {
            clip
        };

        let seq = *order;
        *order += 1;
        let mut item = CanvasItem::new(CanvasItemId(seq + 1));
        // Godot resolves a Control's font as local `theme_override_fonts/font`
        // first, then the Theme — the same precedence the colour override above
        // already follows. Recorded here because this is the only point where
        // the node id and the canvas item id are both known (pat-0q2kx.146).
        if let Some(path) = font_override_path(tree, node_id) {
            fonts.insert(seq + 1, path);
        }
        // Pre-order sequence as z so children (added later) paint on top.
        item.z_index = seq as i32;
        item.clip_rect = clip;
        draw_control(tree, node_id, theme, rect, &mut item.commands);
        if !item.commands.is_empty() {
            viewport.add_canvas_item(item);
        }

        // Children anchor against this control's rect in its global space.
        let class = class_of(tree, node_id);
        let is_container = control::is_container_class_name(&class);
        for child in children_of(tree, node_id) {
            render_node(
                tree,
                child,
                theme,
                viewport,
                global_pos,
                size,
                order,
                clip,
                is_container,
                fonts,
            );
        }

        // A ScrollContainer's bars are internal children that draw last, so
        // they go in their own item after the content walk — otherwise the very
        // content they scroll paints over them (pat-0q2kx.86).
        if class == "ScrollContainer" {
            let commands = scrollbar_commands(tree, node_id, rect);
            if !commands.is_empty() {
                let seq = *order;
                *order += 1;
                let mut bars = CanvasItem::new(CanvasItemId(seq + 1));
                bars.z_index = seq as i32;
                bars.clip_rect = clip;
                bars.commands = commands;
                viewport.add_canvas_item(bars);
            }
        }
    } else {
        // Non-control node: pass the parent rect straight through to children.
        for child in children_of(tree, node_id) {
            render_node(
                tree,
                child,
                theme,
                viewport,
                parent_global,
                parent_size,
                order,
                clip,
                false,
                fonts,
            );
        }
    }
}

/// The scrollbar draw commands for a ScrollContainer, or empty when nothing
/// overflows (pat-0q2kx.86).
///
/// Godot's ScrollContainer creates internal HScrollBar / VScrollBar children in
/// its constructor and sizes their grabbers from the Range model — `page` is the
/// viewport extent, `max` the content extent
/// (upstream/godot/scene/gui/scroll_container.cpp `_update_scrollbar_position`).
/// Patina has no internal-node machinery, so the bars are built here from that
/// same math. A bar appears only on the axis that actually overflows, as in
/// Godot.
///
/// These go in their own canvas item emitted *after* the container's children
/// (see [`render_node`]) because the internal bars are siblings that come last
/// in Godot's draw order — folding them into the container's own item would
/// paint them under the very content they scroll.
fn scrollbar_commands(tree: &SceneTree, node_id: NodeId, rect: Rect2) -> Vec<DrawCommand> {
    let mut out = Vec::new();
    let content = control::scroll_content_size(tree, node_id, rect.size);
    let scroll = control::get_scroll(tree, node_id);
    let trough = Color::new(0.1, 0.1, 0.1, 1.0);
    let grabber = Color::new(0.45, 0.45, 0.45, 1.0);

    if content.y > rect.size.y {
        let bar = Rect2::new(
            Vector2::new(
                rect.position.x + rect.size.x - SCROLLBAR_THICKNESS,
                rect.position.y,
            ),
            Vector2::new(SCROLLBAR_THICKNESS, rect.size.y),
        );
        out.push(DrawCommand::DrawRect {
            rect: bar,
            color: trough,
            filled: true,
        });
        // Grabber length is the visible fraction of the content; its
        // offset is how far through the scrollable range we are.
        let frac = (rect.size.y / content.y).clamp(0.0, 1.0);
        let grab_h = (bar.size.y * frac).max(SCROLLBAR_THICKNESS);
        let travel = (bar.size.y - grab_h).max(0.0);
        let max_scroll = (content.y - rect.size.y).max(0.0);
        let t = if max_scroll > 0.0 {
            (scroll.y / max_scroll).clamp(0.0, 1.0)
        } else {
            0.0
        };
        out.push(DrawCommand::DrawRect {
            rect: Rect2::new(
                Vector2::new(bar.position.x, bar.position.y + travel * t),
                Vector2::new(bar.size.x, grab_h),
            ),
            color: grabber,
            filled: true,
        });
    }

    if content.x > rect.size.x {
        let bar = Rect2::new(
            Vector2::new(
                rect.position.x,
                rect.position.y + rect.size.y - SCROLLBAR_THICKNESS,
            ),
            Vector2::new(rect.size.x, SCROLLBAR_THICKNESS),
        );
        out.push(DrawCommand::DrawRect {
            rect: bar,
            color: trough,
            filled: true,
        });
        let frac = (rect.size.x / content.x).clamp(0.0, 1.0);
        let grab_w = (bar.size.x * frac).max(SCROLLBAR_THICKNESS);
        let travel = (bar.size.x - grab_w).max(0.0);
        let max_scroll = (content.x - rect.size.x).max(0.0);
        let t = if max_scroll > 0.0 {
            (scroll.x / max_scroll).clamp(0.0, 1.0)
        } else {
            0.0
        };
        out.push(DrawCommand::DrawRect {
            rect: Rect2::new(
                Vector2::new(bar.position.x + travel * t, bar.position.y),
                Vector2::new(grab_w, bar.size.y),
            ),
            color: grabber,
            filled: true,
        });
    }

    out
}

/// Reads a `Color`-typed node property, if present.
fn color_prop(tree: &SceneTree, node_id: NodeId, key: &str) -> Option<Color> {
    match tree.get_node(node_id)?.get_property(key) {
        Variant::Color(c) => Some(c),
        _ => None,
    }
}

/// Reads a `String`-typed node property, if present.
/// The font resource path a Control asks for via `theme_override_fonts/font`
/// (pat-0q2kx.146), or `None` to use the renderer's default face.
///
/// A `.tscn` writes the slot as `ExtResource("id")`, which the loader resolves
/// into an `__extres__<prop>` companion holding the real `res://` path, so the
/// companion is preferred and the raw property is the fallback for a
/// hand-written scene that spells the path directly.
pub fn font_override_path(tree: &SceneTree, node_id: NodeId) -> Option<String> {
    string_prop(tree, node_id, "__extres__theme_override_fonts/font")
        .or_else(|| string_prop(tree, node_id, "theme_override_fonts/font"))
        .filter(|p| !p.is_empty() && !p.starts_with("ExtResource("))
}

fn string_prop(tree: &SceneTree, node_id: NodeId, key: &str) -> Option<String> {
    match tree.get_node(node_id)?.get_property(key) {
        Variant::String(s) => Some(s),
        _ => None,
    }
}

/// Reads a number-typed node property, if present. `Int` is accepted as well as
/// `Float` because a script assigning `0` yields an int (pat-nzgdu.157).
fn float_prop(tree: &SceneTree, node_id: NodeId, key: &str) -> Option<f64> {
    match tree.get_node(node_id)?.get_property(key) {
        Variant::Float(f) => Some(f),
        Variant::Int(i) => Some(i as f64),
        _ => None,
    }
}

/// Resolves a StyleBox theme item to its background color. Default styleboxes
/// are stored as `Variant::Color` (their flat background). Falls back to
/// `fallback` when the item is missing or a different Variant type.
fn stylebox_bg(theme: &Theme, control_type: &str, property: &str, fallback: Color) -> Color {
    // The generic Variant item wins when set — that is how `default_theme`
    // stores its colours. A Godot-authored `.tres` instead yields a *typed*
    // StyleBox, so the typed map is consulted next; without this a loaded theme
    // parsed correctly and then changed no pixels (pat-0q2kx.150).
    if let Some(Variant::Color(c)) =
        theme.get_item(ThemePropertyType::Stylebox, control_type, property)
    {
        return *c;
    }
    theme
        .get_stylebox(control_type, property)
        .and_then(|sb| sb.bg_color())
        .unwrap_or(fallback)
}

/// Maps a theme font size (in points) to the built-in 5×7 font's integer pixel
/// scale so glyph height ≈ the requested size.
pub(crate) fn text_scale(font_size: i64) -> u32 {
    (font_size.max(1) as u32 / GLYPH_H as u32).max(1)
}

/// Approximate pixel width of `text` at the given scale, using the nominal
/// 5×7 monospace metrics.
pub(crate) fn text_width(text: &str, scale: u32) -> f32 {
    let n = text.chars().count() as f32;
    if n <= 0.0 {
        return 0.0;
    }
    (n * GLYPH_W + (n - 1.0) * GLYPH_GAP) * scale as f32
}

/// A run of text sharing one color, produced by [`parse_bbcode`].
#[derive(Clone, Debug, PartialEq)]
struct TextSpan {
    text: String,
    color: Color,
    /// The per-glyph effect covering this run, from `[wave]`/`[shake]`/… .
    /// `None` is the common case and keeps the whole run in one draw call.
    effect: Option<BuiltinEffect>,
    /// Absolute index of this run's first glyph within the label's text, with
    /// tags stripped. Effects phase off the index, so a run has to know where
    /// it starts (pat-nzgdu.157).
    char_offset: usize,
    /// The `[start, end)` glyph range of the **whole effect application** this
    /// run belongs to — deliberately NOT this run's own bounds. A nested tag
    /// splits an effected run into several spans (`[wave]a[color=red]b[/color]c[/wave]`
    /// is three), and they must keep phasing as one travelling wave, so
    /// `relative_index` has to keep counting across the split. Only meaningful
    /// when `effect` is `Some`; the `end` is backfilled once the tag closes.
    effect_range: (usize, usize),
}

/// Parses a BBCode subset into colored spans. Recognized tags:
/// `[color=#rrggbb]`/`[color=name]` … `[/color]` set the run color; `[b]`,
/// `[i]`, `[u]`, `[s]` (and their closers) are stripped — the built-in bitmap
/// font has no weight/style variants, so formatting only affects the stripped
/// text. Unknown `[...]` tags are dropped. `default` colors untagged text.
fn parse_bbcode(src: &str, default: Color) -> Vec<TextSpan> {
    let mut spans: Vec<TextSpan> = Vec::new();
    // Color stack so nested `[color]` tags restore the outer color on `[/color]`.
    let mut color_stack: Vec<Color> = vec![default];
    // Effect stack, mirroring the color stack: `[wave]a[shake]b[/shake]c[/wave]`
    // leaves `c` waving. Each entry carries the tag name that opened it (so
    // `[/wave]` only closes a wave) and the glyph index it opened at (so every
    // run it produces phases from the tag, even across a nested tag).
    let mut effect_stack: Vec<(String, BuiltinEffect, usize)> = Vec::new();
    // Absolute glyph index, counting only text that survives tag stripping —
    // this is what effects phase off, so it must not count tag characters.
    let mut char_index: usize = 0;
    let mut cur = String::new();
    let mut span_start: usize = 0;
    let mut chars = src.chars();

    // Closes the current run. `span_start` is where the run began; the caller
    // advances it afterwards.
    let flush = |cur: &mut String,
                 color: Color,
                 effect: Option<(BuiltinEffect, usize)>,
                 start: usize,
                 spans: &mut Vec<TextSpan>| {
        if !cur.is_empty() {
            spans.push(TextSpan {
                text: std::mem::take(cur),
                color,
                effect: effect.map(|(e, _)| e),
                char_offset: start,
                // `end` is unknown until the tag closes; backfilled below.
                effect_range: (effect.map(|(_, s)| s).unwrap_or(0), 0),
            });
        }
    };

    while let Some(c) = chars.next() {
        if c == '[' {
            // Collect up to the closing ']'.
            let mut tag = String::new();
            let mut closed = false;
            for tc in chars.by_ref() {
                if tc == ']' {
                    closed = true;
                    break;
                }
                tag.push(tc);
            }
            if !closed {
                // Unterminated '[' — treat literally.
                cur.push('[');
                cur.push_str(&tag);
                char_index += 1 + tag.chars().count();
                break;
            }
            let cur_color = *color_stack.last().unwrap();
            let cur_effect = effect_stack.last().map(|(_, e, start)| (*e, *start));
            let lower = tag.to_ascii_lowercase();
            if let Some(spec) = lower.strip_prefix("color=") {
                flush(&mut cur, cur_color, cur_effect, span_start, &mut spans);
                span_start = char_index;
                color_stack.push(parse_bbcode_color(spec).unwrap_or(cur_color));
            } else if lower == "/color" {
                flush(&mut cur, cur_color, cur_effect, span_start, &mut spans);
                span_start = char_index;
                if color_stack.len() > 1 {
                    color_stack.pop();
                }
            } else if let Some(name) = lower.strip_prefix('/') {
                // A closer: pop only if it matches the open effect.
                if effect_stack.last().map(|(n, _, _)| n.as_str()) == Some(name) {
                    flush(&mut cur, cur_color, cur_effect, span_start, &mut spans);
                    span_start = char_index;
                    effect_stack.pop();
                }
                // Otherwise it is `[/b]` and friends: strip, no state change.
            } else {
                // An opener: `wave`, or `wave amp=20 freq=3`.
                let (name, params) = parse_tag_params(&lower);
                if let Some(effect) = BuiltinEffect::parse(name, &params) {
                    flush(&mut cur, cur_color, cur_effect, span_start, &mut spans);
                    span_start = char_index;
                    effect_stack.push((name.to_string(), effect, char_index));
                }
                // b/i/u/s, unknown tags, and script-defined effect tags (which
                // cannot be dispatched yet — see `char_fx`): strip, no change.
            }
        } else {
            cur.push(c);
            char_index += 1;
        }
    }
    let cur_color = *color_stack.last().unwrap();
    let cur_effect = effect_stack.last().map(|(_, e, start)| (*e, *start));
    flush(&mut cur, cur_color, cur_effect, span_start, &mut spans);
    backfill_effect_ends(&mut spans);
    spans
}

/// Fills in each effect application's `end` once all its runs are known.
///
/// Consecutive effected spans sharing an `effect_range.0` are the same
/// application — a nested tag (`[wave]a[color=red]b[/color]c[/wave]`) split it
/// into several runs, but it is one travelling wave, so they must all report the
/// full range. Without this, every sub-run would restart at `relative_index 0`
/// and the glyphs would bob in phase instead of travelling (pat-nzgdu.157).
fn backfill_effect_ends(spans: &mut [TextSpan]) {
    let mut i = 0;
    while i < spans.len() {
        if spans[i].effect.is_none() {
            i += 1;
            continue;
        }
        let start = spans[i].effect_range.0;
        // Walk the whole application: same effect start, uninterrupted.
        let mut j = i;
        let mut end = start;
        while j < spans.len() && spans[j].effect.is_some() && spans[j].effect_range.0 == start {
            end = spans[j].char_offset + spans[j].text.chars().count();
            j += 1;
        }
        for span in &mut spans[i..j] {
            span.effect_range.1 = end;
        }
        i = j;
    }
}

/// Splits a BBCode tag body into its name and `key=value` params:
/// `"wave amp=20 freq=3"` -> `("wave", {"amp": "20", "freq": "3"})`.
fn parse_tag_params(tag: &str) -> (&str, HashMap<String, String>) {
    let mut parts = tag.split_whitespace();
    let name = parts.next().unwrap_or("");
    let mut params = HashMap::new();
    for p in parts {
        if let Some((k, v)) = p.split_once('=') {
            params.insert(k.to_string(), v.to_string());
        }
    }
    (name, params)
}

/// Parses a BBCode color spec: `#rgb`, `#rrggbb`, `#rrggbbaa`, or a small set of
/// named colors. Returns `None` for anything unrecognized.
fn parse_bbcode_color(spec: &str) -> Option<Color> {
    let s = spec.trim();
    if let Some(hex) = s.strip_prefix('#') {
        return parse_hex_color(hex);
    }
    let named = match s.to_ascii_lowercase().as_str() {
        "white" => Color::WHITE,
        "black" => Color::BLACK,
        "red" => Color::new(1.0, 0.0, 0.0, 1.0),
        "green" => Color::new(0.0, 1.0, 0.0, 1.0),
        "blue" => Color::new(0.0, 0.0, 1.0, 1.0),
        "yellow" => Color::new(1.0, 1.0, 0.0, 1.0),
        "cyan" => Color::new(0.0, 1.0, 1.0, 1.0),
        "magenta" => Color::new(1.0, 0.0, 1.0, 1.0),
        "gray" | "grey" => Color::new(0.5, 0.5, 0.5, 1.0),
        "orange" => Color::new(1.0, 0.65, 0.0, 1.0),
        _ => return None,
    };
    Some(named)
}

/// Parses `rgb`, `rrggbb`, or `rrggbbaa` hex (no leading `#`) into a `Color`.
fn parse_hex_color(hex: &str) -> Option<Color> {
    let h = hex.trim();
    let comp = |s: &str| u8::from_str_radix(s, 16).ok().map(|v| v as f32 / 255.0);
    match h.len() {
        3 => {
            let dup = |c: char| {
                let s: String = [c, c].iter().collect();
                comp(&s)
            };
            let mut it = h.chars();
            let r = dup(it.next()?)?;
            let g = dup(it.next()?)?;
            let b = dup(it.next()?)?;
            Some(Color::new(r, g, b, 1.0))
        }
        6 => Some(Color::new(
            comp(&h[0..2])?,
            comp(&h[2..4])?,
            comp(&h[4..6])?,
            1.0,
        )),
        8 => Some(Color::new(
            comp(&h[0..2])?,
            comp(&h[2..4])?,
            comp(&h[4..6])?,
            comp(&h[6..8])?,
        )),
        _ => None,
    }
}

/// Reads a numeric (`float`/`int`) node property, if present.
fn num_prop(tree: &SceneTree, node_id: NodeId, key: &str) -> Option<f64> {
    match tree.get_node(node_id)?.get_property(key) {
        Variant::Float(f) => Some(f),
        Variant::Int(i) => Some(i as f64),
        _ => None,
    }
}

/// Reads a `bool`-typed node property, if present.
/// Reads an integer-valued node property (Godot enums arrive as ints).
fn int_prop(tree: &SceneTree, node_id: NodeId, key: &str) -> Option<i64> {
    match tree.get_node(node_id)?.get_property(key) {
        Variant::Int(i) => Some(i),
        Variant::Float(f) => Some(f as i64),
        _ => None,
    }
}

/// The advance between two words, chosen so that
/// `width(a) + SPACE + width(b) == text_width("a b")` — the wrap is only
/// correct if the width model the layout uses matches the one that draws.
fn space_advance(scale: u32) -> f32 {
    (GLYPH_W + 2.0 * GLYPH_GAP) * scale as f32
}

/// Lays a `Label`'s text out as Godot does and emits one `DrawString` per line
/// (pat-0q2kx.149).
///
/// Honors `autowrap_mode` (OFF/ARBITRARY/WORD/WORD_SMART),
/// `horizontal_alignment` (LEFT/CENTER/RIGHT/FILL) and
/// `text_overrun_behavior` (NONE / TRIM_ELLIPSIS and friends), delegating the
/// actual computation to `gdrender2d::text_layout` rather than reimplementing
/// it here.
///
/// ⚠️ Godot distinguishes ARBITRARY (break mid-word) from WORD; `text_layout`
/// only implements word breaking, so ARBITRARY is treated as WORD. That is a
/// narrower wrap than Godot for a single word wider than the rect, and is
/// called out rather than silently approximated.
fn layout_label_lines(
    tree: &SceneTree,
    node_id: NodeId,
    text: &str,
    rect: Rect2,
    color: Color,
    scale: u32,
) -> Vec<DrawCommand> {
    use gdrender2d::text_layout::{align_offset, break_lines_word, ellipsis_word_count, HAlign};

    // Godot: AUTOWRAP_OFF 0, ARBITRARY 1, WORD 2, WORD_SMART 3.
    let autowrap = int_prop(tree, node_id, "autowrap_mode").unwrap_or(0);
    // Godot: LEFT 0, CENTER 1, RIGHT 2, FILL 3.
    let align = match int_prop(tree, node_id, "horizontal_alignment").unwrap_or(0) {
        1 => HAlign::Center,
        2 => HAlign::Right,
        3 => HAlign::Fill,
        _ => HAlign::Left,
    };
    // Godot: NONE 0, TRIM_CHAR 1, TRIM_WORD 2, ELLIPSIS 3, WORD_ELLIPSIS 4.
    let overrun = int_prop(tree, node_id, "text_overrun_behavior").unwrap_or(0);

    let words: Vec<&str> = text.split_whitespace().collect();
    if words.is_empty() {
        return Vec::new();
    }
    let widths: Vec<f32> = words.iter().map(|w| text_width(w, scale)).collect();
    let space = space_advance(scale);
    let max_width = rect.size.x;
    let line_height = (GLYPH_H + GLYPH_GAP) * scale as f32;

    // Wrapping is off unless the node asks for it, matching Godot's default.
    let lines = if autowrap == 0 {
        vec![gdrender2d::text_layout::LineRange {
            start: 0,
            end: words.len(),
            width: widths.iter().sum::<f32>() + space * (words.len() as f32 - 1.0),
        }]
    } else {
        break_lines_word(&widths, space, max_width)
    };

    let mut out = Vec::new();
    for (row, line) in lines.iter().enumerate() {
        let mut slice: Vec<&str> = words[line.start..line.end].to_vec();
        let mut line_width = line.width;

        // Overrun trimming only applies to a line that still overflows — i.e.
        // when wrapping is off, or a single word is wider than the rect.
        let ellipsis_requested = matches!(overrun, 3 | 4);
        if ellipsis_requested && line_width > max_width {
            let ell = "...";
            let ell_w = text_width(ell, scale);
            let widths_slice = &widths[line.start..line.end];
            if let Some(keep) = ellipsis_word_count(widths_slice, space, max_width, ell_w) {
                slice.truncate(keep);
                slice.push(ell);
                line_width = widths_slice[..keep].iter().sum::<f32>() + space * keep as f32 + ell_w;
            }
        }

        let text_line = slice.join(" ");
        if text_line.is_empty() {
            continue;
        }
        let dx = align_offset(line_width, max_width, align);
        out.push(DrawCommand::DrawString {
            text: text_line,
            position: Vector2::new(
                rect.position.x + dx,
                rect.position.y + row as f32 * line_height,
            ),
            color,
            font_size: scale,
        });
    }
    out
}

fn bool_prop(tree: &SceneTree, node_id: NodeId, key: &str) -> Option<bool> {
    match tree.get_node(node_id)?.get_property(key) {
        Variant::Bool(b) => Some(b),
        _ => None,
    }
}

/// A separator's minimum size — Godot's `Separator::get_minimum_size`
/// (`scene/gui/separator.cpp:35-43`), pat-nzgdu.197.
///
/// ⚠️ The base is **(3, 3), not zero**: a separator always reserves 3px on its
/// *long* axis so it never collapses to nothing in a container. Only the cross
/// axis takes the theme's `separation`. So an HSeparator is `(3, separation)` —
/// returning `(0, separation)` or `(separation, separation)` are both wrong, and
/// both look plausible.
///
/// The orientation comes from the CLASS, matching Godot, where H/VSeparator are
/// separate classes rather than one node with a `vertical` flag.
pub fn separator_minimum_size(theme: &Theme, class: &str) -> Vector2 {
    // NB `get_constant_or` lives on `ThemeDB`, NOT on `Theme` — only `Theme`'s
    // `get_constant` is available here, so the fallback is explicit.
    let separation = theme.get_constant(class, "separation").unwrap_or(4) as f32;
    match class {
        // VERTICAL: the line runs down, so `separation` is its WIDTH.
        "VSeparator" => Vector2::new(separation, 3.0),
        // HORIZONTAL: the line runs across, so `separation` is its HEIGHT.
        _ => Vector2::new(3.0, separation),
    }
}

fn draw_control(
    tree: &SceneTree,
    node_id: NodeId,
    theme: &Theme,
    rect: Rect2,
    out: &mut Vec<DrawCommand>,
) {
    match class_of(tree, node_id).as_str() {
        "ColorRect" => {
            let color = color_prop(tree, node_id, "color").unwrap_or(Color::WHITE);
            out.push(DrawCommand::DrawRect {
                rect,
                color,
                filled: true,
            });
        }
        // ReferenceRect draws only a border, to visualise a Control's extents
        // (pat-0q2kx.245; `scene/gui/reference_rect.cpp:37-44`).
        //
        // Godot's condition is `is_editor_hint() || !editor_only`. This is the
        // **runtime** pass, where `is_editor_hint()` is false, so it reduces to
        // `!editor_only` — and `editor_only` defaults to **true**
        // (`reference_rect.h:40`), so an unconfigured ReferenceRect correctly
        // draws nothing here. The editor's own viewport is a separate renderer;
        // when an editor-hint accessor exists on this side, this becomes the
        // full two-term condition.
        "ReferenceRect" => {
            if bool_prop(tree, node_id, "editor_only").unwrap_or(true) {
                return;
            }
            let color = color_prop(tree, node_id, "border_color")
                // Godot's default is red (`Color(1, 0, 0)`), not white.
                .unwrap_or(Color::new(1.0, 0.0, 0.0, 1.0));
            let width = float_prop(tree, node_id, "border_width").unwrap_or(1.0) as f32;
            if width <= 0.0 {
                return;
            }
            // `DrawCommand::DrawRect` strokes a single-pixel outline and carries
            // no width, so a thick border is drawn as concentric outlines inset
            // one pixel at a time. Godot strokes with a real width; this matches
            // it at width 1 and approximates it above that.
            let steps = width.round().max(1.0) as i32;
            for i in 0..steps {
                let inset = i as f32;
                let size = Vector2::new(rect.size.x - inset * 2.0, rect.size.y - inset * 2.0);
                if size.x <= 0.0 || size.y <= 0.0 {
                    break;
                }
                out.push(DrawCommand::DrawRect {
                    rect: Rect2 {
                        position: Vector2::new(rect.position.x + inset, rect.position.y + inset),
                        size,
                    },
                    color,
                    filled: false,
                });
            }
        }
        // pat-nzgdu.197. The orientation is the CLASS, not a property — so the
        // only difference between the two is which stylebox the theme hands
        // back. Godot's is a StyleBoxLine, which already knows how to centre
        // its line in the rect and honour `vertical`.
        "HSeparator" | "VSeparator" => {
            if let Some(sb) = theme.get_stylebox(&class_of(tree, node_id), "separator") {
                out.extend(sb.draw(rect));
            }
        }
        "Panel" | "PanelContainer" => {
            // Prefer a typed StyleBox (pat-u3l8c.70); fall back to the legacy
            // Variant::Color background.
            if let Some(sb) = theme.get_stylebox("Panel", "panel") {
                out.extend(sb.draw(rect));
            } else {
                let bg = stylebox_bg(theme, "Panel", "panel", Color::new(0.15, 0.15, 0.15, 1.0));
                out.push(DrawCommand::DrawRect {
                    rect,
                    color: bg,
                    filled: true,
                });
            }
        }
        "Button" => {
            if let Some(sb) = theme.get_stylebox("Button", "normal") {
                out.extend(sb.draw(rect));
            } else {
                let bg = stylebox_bg(theme, "Button", "normal", Color::new(0.2, 0.2, 0.2, 1.0));
                out.push(DrawCommand::DrawRect {
                    rect,
                    color: bg,
                    filled: true,
                });
            }
            let text = string_prop(tree, node_id, "text").unwrap_or_default();
            if !text.is_empty() {
                // Godot resolves a Control's theme color as: local
                // `theme_override_colors/<name>` first, then the Theme
                // (pat-z35r2.17). Honor the per-node override during draw.
                let font_color = color_prop(tree, node_id, "theme_override_colors/font_color")
                    .or_else(|| theme.get_color("Button", "font_color"))
                    .unwrap_or(Color::new(0.875, 0.875, 0.875, 1.0));
                let font_size = theme.get_font_size("Button", "font_size").unwrap_or(16);
                let scale = text_scale(font_size);
                let tw = text_width(&text, scale);
                let th = GLYPH_H * scale as f32;
                // Centre the label within the button rect.
                let position = Vector2::new(
                    rect.position.x + (rect.size.x - tw) * 0.5,
                    rect.position.y + (rect.size.y - th) * 0.5,
                );
                out.push(DrawCommand::DrawString {
                    text,
                    position,
                    color: font_color,
                    font_size: scale,
                });
            }
        }
        "Label" => {
            let text = string_prop(tree, node_id, "text").unwrap_or_default();
            if !text.is_empty() {
                // Per-node `theme_override_colors/font_color` wins over the
                // Theme during draw (pat-z35r2.17).
                let font_color = color_prop(tree, node_id, "theme_override_colors/font_color")
                    .or_else(|| theme.get_color("Label", "font_color"))
                    .unwrap_or(Color::WHITE);
                let font_size = theme.get_font_size("Label", "font_size").unwrap_or(16);
                let scale = text_scale(font_size);
                // Lay the text out instead of drawing one flush-left line
                // (pat-0q2kx.149). `gdrender2d::text_layout` implemented word
                // breaking, alignment and ellipsis trimming with unit tests but
                // had ZERO consumers anywhere in the workspace, so an authored
                // `autowrap_mode`/`horizontal_alignment`/`text_overrun_behavior`
                // changed nothing on screen.
                for cmd in layout_label_lines(tree, node_id, &text, rect, font_color, scale) {
                    out.push(cmd);
                }
            }
        }
        "TextureRect" => {
            if let Some(path) = string_prop(tree, node_id, "texture") {
                if !path.is_empty() {
                    out.push(DrawCommand::DrawTextureRect {
                        texture_path: path,
                        rect,
                        modulate: Color::WHITE,
                    });
                }
            }
        }
        // NinePatchRect (pat-0q2kx.179). The nine-patch rasterizer has existed in
        // `gdrender2d` with **no producer** — `draw_control` had no arm, so the
        // class fell through to `_ => {}` and drew nothing at all.
        //
        // The per-axis stretch mapping is reused from `stylebox::AxisStretchMode`
        // rather than re-derived: `NinePatchRect` and `StyleBoxTexture` share
        // Godot's `AXIS_STRETCH_MODE_*` constants (STRETCH=0, TILE=1, TILE_FIT=2),
        // and duplicating the table is how the two drift apart.
        "NinePatchRect" => {
            if let Some(path) = string_prop(tree, node_id, "texture") {
                if !path.is_empty() {
                    let margin = |key: &str| int_prop(tree, node_id, key).unwrap_or(0) as f32;
                    let axis = |key: &str| {
                        crate::stylebox::AxisStretchMode::from_i64(
                            int_prop(tree, node_id, key).unwrap_or(0),
                        )
                        .to_nine_patch_axis()
                    };
                    out.push(DrawCommand::DrawNinePatch {
                        texture_path: path,
                        rect,
                        margin_left: margin("patch_margin_left"),
                        margin_top: margin("patch_margin_top"),
                        margin_right: margin("patch_margin_right"),
                        margin_bottom: margin("patch_margin_bottom"),
                        // Godot's `draw_center` defaults to true; an absent
                        // property must not blank the middle patch.
                        draw_center: bool_prop(tree, node_id, "draw_center").unwrap_or(true),
                        axis_h: axis("axis_stretch_horizontal"),
                        axis_v: axis("axis_stretch_vertical"),
                        modulate: Color::WHITE,
                    });
                }
            }
        }
        "RichTextLabel" => {
            // Accept either `bbcode_text` (bbcode_enabled) or plain `text`.
            let raw = string_prop(tree, node_id, "bbcode_text")
                .filter(|s| !s.is_empty())
                .or_else(|| string_prop(tree, node_id, "text"))
                .unwrap_or_default();
            if !raw.is_empty() {
                let font_color = theme
                    .get_color("RichTextLabel", "default_color")
                    .unwrap_or(Color::WHITE);
                let font_size = theme
                    .get_font_size("RichTextLabel", "font_size")
                    .unwrap_or(16);
                let scale = text_scale(font_size);
                // Effects are pure functions of elapsed time, which the label
                // carries as a property so the renderer stays a function of the
                // tree. A frame driver advances it; a test sets it directly.
                let elapsed = float_prop(tree, node_id, "fx_elapsed_time").unwrap_or(0.0) as f32;
                let spans = parse_bbcode(&raw, font_color);
                // The effect's range is its own run, so glyphs phase from the
                // tag rather than from the start of the label's text.
                let mut x = rect.position.x;
                let y = rect.position.y;
                for span in spans {
                    if span.text.is_empty() {
                        continue;
                    }
                    let w = text_width(&span.text, scale);
                    match span.effect {
                        // Plain text: one draw call for the whole run.
                        None => {
                            out.push(DrawCommand::DrawString {
                                text: span.text,
                                position: Vector2::new(x, y),
                                color: span.color,
                                font_size: scale,
                            });
                            x += w + GLYPH_GAP * scale as f32;
                        }
                        // Effected text: each glyph moves independently, so it
                        // needs its own draw call with its own offset/color.
                        Some(effect) => {
                            // The whole tag's range, not this run's — a nested
                            // tag splits the run but must not restart the phase.
                            let range = span.effect_range;
                            let mut gx = x;
                            for (i, ch) in span.text.chars().enumerate() {
                                let mut fx = CharFXTransform::new(
                                    span.char_offset + i,
                                    range,
                                    elapsed,
                                    span.color,
                                );
                                effect.apply(&mut fx);
                                let gw = text_width(&ch.to_string(), scale);
                                if fx.visible {
                                    out.push(DrawCommand::DrawString {
                                        text: ch.to_string(),
                                        position: Vector2::new(gx, y) + fx.offset,
                                        color: fx.color,
                                        font_size: scale,
                                    });
                                }
                                // Advance by the un-offset width: the effect
                                // displaces the glyph, it must not re-flow the
                                // line under it.
                                gx += gw + GLYPH_GAP * scale as f32;
                            }
                            x = gx;
                        }
                    }
                }
            }
        }
        // A LineEdit draws its background, then the selection band, then the
        // text (masked when `secret`), then the caret — Godot's paint order in
        // `LineEdit::_notification(NOTIFICATION_DRAW)`. Empty text falls back to
        // `placeholder_text` in the dimmer placeholder colour. pat-0q2kx.87.
        "LineEdit" => {
            if let Some(sb) = theme.get_stylebox("LineEdit", "normal") {
                out.extend(sb.draw(rect));
            } else {
                let bg = stylebox_bg(theme, "LineEdit", "normal", Color::new(0.1, 0.1, 0.1, 1.0));
                out.push(DrawCommand::DrawRect {
                    rect,
                    color: bg,
                    filled: true,
                });
            }

            let font_size = theme.get_font_size("LineEdit", "font_size").unwrap_or(16);
            let scale = text_scale(font_size);
            let glyph_h = GLYPH_H * scale as f32;
            // Vertically centre the single line, as Godot does.
            let baseline_y = rect.position.y + (rect.size.y - glyph_h) * 0.5;

            // The displayed string is the masked one; the caret and selection
            // are measured against it so a `secret` field's caret still lands
            // between mask glyphs rather than between the real characters.
            let shown = crate::line_edit::display_text(tree, node_id);

            // x offset of a caret sitting *before* character `column`.
            let caret_x = |column: usize| -> f32 {
                if column == 0 {
                    return rect.position.x;
                }
                let prefix: String = shown.chars().take(column).collect();
                rect.position.x + text_width(&prefix, scale) + GLYPH_GAP * scale as f32
            };

            if let Some((begin, end)) = crate::line_edit::selection(tree, node_id) {
                let x0 = caret_x(begin);
                let x1 = caret_x(end);
                if x1 > x0 {
                    let selection_color = theme
                        .get_color("LineEdit", "selection_color")
                        .unwrap_or(Color::new(0.3, 0.45, 0.7, 1.0));
                    out.push(DrawCommand::DrawRect {
                        rect: Rect2::new(
                            Vector2::new(x0, baseline_y),
                            Vector2::new(x1 - x0, glyph_h),
                        ),
                        color: selection_color,
                        filled: true,
                    });
                }
            }

            if shown.is_empty() {
                let placeholder =
                    string_prop(tree, node_id, "placeholder_text").unwrap_or_default();
                if !placeholder.is_empty() {
                    let color = theme
                        .get_color("LineEdit", "font_placeholder_color")
                        .unwrap_or(Color::new(0.6, 0.6, 0.6, 1.0));
                    out.push(DrawCommand::DrawString {
                        text: placeholder,
                        position: Vector2::new(rect.position.x, baseline_y),
                        color,
                        font_size: scale,
                    });
                }
            } else {
                let font_color = color_prop(tree, node_id, "theme_override_colors/font_color")
                    .or_else(|| theme.get_color("LineEdit", "font_color"))
                    .unwrap_or(Color::WHITE);
                out.push(DrawCommand::DrawString {
                    text: shown.clone(),
                    position: Vector2::new(rect.position.x, baseline_y),
                    color: font_color,
                    font_size: scale,
                });
            }

            // Godot only paints the caret while the field holds focus, so an
            // unfocused LineEdit must not sprout one.
            if tree.focus_owner() == Some(node_id) {
                let caret_color = theme
                    .get_color("LineEdit", "caret_color")
                    .unwrap_or(Color::WHITE);
                let x = caret_x(crate::line_edit::caret_column(tree, node_id));
                out.push(DrawCommand::DrawRect {
                    rect: Rect2::new(
                        Vector2::new(x, baseline_y),
                        Vector2::new(scale.max(1) as f32, glyph_h),
                    ),
                    color: caret_color,
                    filled: true,
                });
            }
        }
        // A TextEdit/CodeEdit draws its `text` line by line. When a
        // CodeHighlighter is assigned — its `keyword_colors` carried on the node
        // as the `syntax_highlighter/keyword_colors` dictionary, so the renderer
        // stays a pure function of the tree, the way RichTextLabel carries its fx
        // time — each keyword run draws in its configured colour; otherwise the
        // whole line draws in the font colour. pat-nzgdu.198.
        "TextEdit" | "CodeEdit" => {
            let text = string_prop(tree, node_id, "text").unwrap_or_default();
            if !text.is_empty() {
                let font_color = color_prop(tree, node_id, "theme_override_colors/font_color")
                    .or_else(|| theme.get_color("TextEdit", "font_color"))
                    .unwrap_or(Color::WHITE);
                let font_size = theme.get_font_size("TextEdit", "font_size").unwrap_or(16);
                let scale = text_scale(font_size);
                let keyword_dict = tree
                    .get_node(node_id)
                    .map(|n| n.get_property("syntax_highlighter/keyword_colors"))
                    .unwrap_or(Variant::Nil);
                let highlighter = CodeHighlighter::from_keyword_dict(&keyword_dict, font_color);
                let line_h = (GLYPH_H + GLYPH_GAP) * scale as f32;
                let mut y = rect.position.y;
                for line in text.split('\n') {
                    let mut x = rect.position.x;
                    for span in highlighter.line_spans(line) {
                        let w = text_width(&span.text, scale);
                        out.push(DrawCommand::DrawString {
                            text: span.text,
                            position: Vector2::new(x, y),
                            color: span.color,
                            font_size: scale,
                        });
                        x += w + GLYPH_GAP * scale as f32;
                    }
                    y += line_h;
                }
            }
        }
        "ProgressBar" => {
            // Background trough.
            let bg = stylebox_bg(
                theme,
                "ProgressBar",
                "background",
                Color::new(0.1, 0.1, 0.1, 1.0),
            );
            out.push(DrawCommand::DrawRect {
                rect,
                color: bg,
                filled: true,
            });
            // Filled portion by (value - min) / (max - min).
            let min = num_prop(tree, node_id, "min_value").unwrap_or(0.0);
            let max = num_prop(tree, node_id, "max_value").unwrap_or(100.0);
            let value = num_prop(tree, node_id, "value").unwrap_or(0.0);
            let ratio = if max > min {
                ((value - min) / (max - min)).clamp(0.0, 1.0)
            } else {
                0.0
            };
            if ratio > 0.0 {
                let fill =
                    stylebox_bg(theme, "ProgressBar", "fill", Color::new(0.4, 0.6, 0.9, 1.0));
                let fill_rect = Rect2::new(
                    rect.position,
                    Vector2::new(rect.size.x * ratio as f32, rect.size.y),
                );
                out.push(DrawCommand::DrawRect {
                    rect: fill_rect,
                    color: fill,
                    filled: true,
                });
            }
        }
        "CheckBox" | "CheckButton" => {
            // Box glyph on the left, text to its right.
            let checked = bool_prop(tree, node_id, "button_pressed").unwrap_or(false);
            let box_side = rect.size.y.min(GLYPH_H * 3.0).max(8.0);
            let box_rect = Rect2::new(
                Vector2::new(
                    rect.position.x,
                    rect.position.y + (rect.size.y - box_side) * 0.5,
                ),
                Vector2::new(box_side, box_side),
            );
            let border = theme
                .get_color("CheckBox", "font_color")
                .unwrap_or(Color::new(0.875, 0.875, 0.875, 1.0));
            // Outline (unfilled) box.
            out.push(DrawCommand::DrawRect {
                rect: box_rect,
                color: border,
                filled: false,
            });
            if checked {
                // Inset filled square marks the checked state.
                let inset = box_side * 0.25;
                out.push(DrawCommand::DrawRect {
                    rect: Rect2::new(
                        Vector2::new(box_rect.position.x + inset, box_rect.position.y + inset),
                        Vector2::new(box_side - inset * 2.0, box_side - inset * 2.0),
                    ),
                    color: border,
                    filled: true,
                });
            }
            let text = string_prop(tree, node_id, "text").unwrap_or_default();
            if !text.is_empty() {
                let font_size = theme.get_font_size("CheckBox", "font_size").unwrap_or(16);
                let scale = text_scale(font_size);
                let th = GLYPH_H * scale as f32;
                out.push(DrawCommand::DrawString {
                    text,
                    position: Vector2::new(
                        box_rect.position.x + box_side + GLYPH_GAP * 2.0 * scale as f32,
                        rect.position.y + (rect.size.y - th) * 0.5,
                    ),
                    color: border,
                    font_size: scale,
                });
            }
        }
        "OptionButton" => {
            // Button background.
            if let Some(sb) = theme.get_stylebox("OptionButton", "normal") {
                out.extend(sb.draw(rect));
            } else {
                let bg = stylebox_bg(
                    theme,
                    "OptionButton",
                    "normal",
                    Color::new(0.2, 0.2, 0.2, 1.0),
                );
                out.push(DrawCommand::DrawRect {
                    rect,
                    color: bg,
                    filled: true,
                });
            }
            let font_color = theme
                .get_color("OptionButton", "font_color")
                .unwrap_or(Color::new(0.875, 0.875, 0.875, 1.0));
            let font_size = theme
                .get_font_size("OptionButton", "font_size")
                .unwrap_or(16);
            let scale = text_scale(font_size);
            // Godot's `OptionButton::_select` mirrors the chosen item's label
            // onto the button's own `text`, so that is the first source. A
            // `.tscn` that only carries `items` + `selected` (never having gone
            // through a select call) is resolved from the item model instead —
            // otherwise the button draws blank (pat-0q2kx.85).
            let text = string_prop(tree, node_id, "text")
                .filter(|s| !s.is_empty())
                .or_else(|| {
                    let ob = crate::item_views::load_option_button(tree, node_id);
                    let sel = ob.get_selected();
                    if sel < 0 {
                        return None;
                    }
                    ob.get_item(sel as usize).map(|it| it.text.clone())
                })
                .or_else(|| string_prop(tree, node_id, "selected_text"))
                .unwrap_or_default();
            if !text.is_empty() {
                let th = GLYPH_H * scale as f32;
                out.push(DrawCommand::DrawString {
                    text,
                    position: Vector2::new(
                        rect.position.x + GLYPH_GAP * 2.0 * scale as f32,
                        rect.position.y + (rect.size.y - th) * 0.5,
                    ),
                    color: font_color,
                    font_size: scale,
                });
            }
            // Down-arrow indicator on the right edge (small filled triangle
            // approximated by a rect for the bitmap renderer).
            let arrow_w = 6.0 * scale as f32;
            let arrow_rect = Rect2::new(
                Vector2::new(
                    rect.position.x + rect.size.x - arrow_w - GLYPH_GAP * scale as f32,
                    rect.position.y + (rect.size.y - arrow_w) * 0.5,
                ),
                Vector2::new(arrow_w, arrow_w),
            );
            out.push(DrawCommand::DrawRect {
                rect: arrow_rect,
                color: font_color,
                filled: true,
            });
        }
        // One row per item, the selected one behind a highlight — Godot draws
        // these in `ItemList::_notification(NOTIFICATION_DRAW)` from the same
        // item model the click path selects into (pat-0q2kx.85).
        "ItemList" => {
            let bg = stylebox_bg(
                theme,
                "ItemList",
                "panel",
                Color::new(0.12, 0.12, 0.12, 1.0),
            );
            out.push(DrawCommand::DrawRect {
                rect,
                color: bg,
                filled: true,
            });
            let selected_bg = stylebox_bg(
                theme,
                "ItemList",
                "selected",
                Color::new(0.26, 0.42, 0.66, 1.0),
            );
            let font_color = color_prop(tree, node_id, "theme_override_colors/font_color")
                .or_else(|| theme.get_color("ItemList", "font_color"))
                .unwrap_or(Color::new(0.875, 0.875, 0.875, 1.0));
            let font_color_off = theme
                .get_color("ItemList", "font_disabled_color")
                .unwrap_or(Color::new(0.5, 0.5, 0.5, 1.0));
            let scale = text_scale(theme.get_font_size("ItemList", "font_size").unwrap_or(16));
            let th = GLYPH_H * scale as f32;
            let list = crate::item_views::load_item_list(tree, node_id);
            for (i, row) in crate::item_views::item_row_rects(list.item_count(), rect)
                .into_iter()
                .enumerate()
            {
                // Rows past the bottom edge are scrolled out of view.
                if row.position.y >= rect.position.y + rect.size.y {
                    break;
                }
                let Some(item) = list.get_item(i) else { break };
                if item.selected {
                    out.push(DrawCommand::DrawRect {
                        rect: row,
                        color: selected_bg,
                        filled: true,
                    });
                }
                if !item.text.is_empty() {
                    out.push(DrawCommand::DrawString {
                        text: item.text.clone(),
                        position: Vector2::new(
                            row.position.x + GLYPH_GAP * 2.0 * scale as f32,
                            row.position.y + (row.size.y - th) * 0.5,
                        ),
                        color: if item.disabled {
                            font_color_off
                        } else {
                            font_color
                        },
                        font_size: scale,
                    });
                }
            }
        }
        // A standalone tab strip. Same span layout as the TabContainer header
        // (pat-0q2kx.83), so the two draw and hit-test alike.
        "TabBar" => {
            let unselected_bg = stylebox_bg(
                theme,
                "TabBar",
                "tab_unselected",
                Color::new(0.16, 0.16, 0.16, 1.0),
            );
            out.push(DrawCommand::DrawRect {
                rect,
                color: unselected_bg,
                filled: true,
            });
            let selected_bg = stylebox_bg(
                theme,
                "TabBar",
                "tab_selected",
                Color::new(0.22, 0.22, 0.22, 1.0),
            );
            let font_color = theme
                .get_color("TabBar", "font_selected_color")
                .unwrap_or(Color::new(0.95, 0.95, 0.95, 1.0));
            let font_color_off = theme
                .get_color("TabBar", "font_unselected_color")
                .unwrap_or(Color::new(0.7, 0.7, 0.7, 1.0));
            let scale = text_scale(crate::control::get_font_size(tree, node_id));
            let th = GLYPH_H * scale as f32;
            let current = float_prop(tree, node_id, "current_tab").unwrap_or(-1.0) as i64;
            let titles = crate::item_views::tab_titles(tree, node_id);
            for (i, (x, w)) in crate::item_views::tab_spans(tree, node_id)
                .into_iter()
                .enumerate()
            {
                if x >= rect.size.x {
                    break;
                }
                if i as i64 == current {
                    out.push(DrawCommand::DrawRect {
                        rect: Rect2::new(
                            Vector2::new(rect.position.x + x, rect.position.y),
                            Vector2::new(w.min(rect.size.x - x), rect.size.y),
                        ),
                        color: selected_bg,
                        filled: true,
                    });
                }
                let Some(title) = titles.get(i) else { break };
                if !title.is_empty() {
                    out.push(DrawCommand::DrawString {
                        text: title.clone(),
                        position: Vector2::new(
                            rect.position.x + x + crate::control::TAB_H_PADDING,
                            rect.position.y + (rect.size.y - th) * 0.5,
                        ),
                        color: if i as i64 == current {
                            font_color
                        } else {
                            font_color_off
                        },
                        font_size: scale,
                    });
                }
            }
        }
        "TabContainer" => {
            // Panel background behind the content area, plus a header strip.
            let panel_bg = stylebox_bg(
                theme,
                "TabContainer",
                "panel",
                Color::new(0.15, 0.15, 0.15, 1.0),
            );
            out.push(DrawCommand::DrawRect {
                rect,
                color: panel_bg,
                filled: true,
            });
            let tab_bar_h = crate::control::get_tab_bar_height(tree, node_id).min(rect.size.y);
            if tab_bar_h <= 0.0 {
                return;
            }
            let selected_bg = stylebox_bg(
                theme,
                "TabContainer",
                "tab_selected",
                Color::new(0.22, 0.22, 0.22, 1.0),
            );
            let unselected_bg = stylebox_bg(
                theme,
                "TabContainer",
                "tab_unselected",
                Color::new(0.16, 0.16, 0.16, 1.0),
            );
            out.push(DrawCommand::DrawRect {
                rect: Rect2::new(rect.position, Vector2::new(rect.size.x, tab_bar_h)),
                color: unselected_bg,
                filled: true,
            });
            // One box per tab plus its title, with the current tab highlighted —
            // Godot draws the tab StyleBoxes and titles in
            // `TabBar::_notification(NOTIFICATION_DRAW)` (pat-0q2kx.83).
            let current = crate::control::get_current_tab(tree, node_id);
            let font_color = theme
                .get_color("TabContainer", "font_selected_color")
                .unwrap_or(Color::new(0.95, 0.95, 0.95, 1.0));
            let font_color_off = theme
                .get_color("TabContainer", "font_unselected_color")
                .unwrap_or(Color::new(0.7, 0.7, 0.7, 1.0));
            let scale = text_scale(crate::control::get_font_size(tree, node_id));
            let th = GLYPH_H * scale as f32;
            for (i, (x, w)) in crate::control::tab_rects(tree, node_id)
                .into_iter()
                .enumerate()
            {
                if x >= rect.size.x {
                    break;
                }
                let selected = i == current;
                if selected {
                    out.push(DrawCommand::DrawRect {
                        rect: Rect2::new(
                            Vector2::new(rect.position.x + x, rect.position.y),
                            Vector2::new(w.min(rect.size.x - x), tab_bar_h),
                        ),
                        color: selected_bg,
                        filled: true,
                    });
                }
                let title = crate::control::get_tab_title(tree, node_id, i);
                if !title.is_empty() {
                    out.push(DrawCommand::DrawString {
                        text: title,
                        position: Vector2::new(
                            rect.position.x + x + crate::control::TAB_H_PADDING,
                            rect.position.y + (tab_bar_h - th) * 0.5,
                        ),
                        color: if selected { font_color } else { font_color_off },
                        font_size: scale,
                    });
                }
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::AnchorPreset;
    use crate::node::Node;

    fn approx(a: Color, b: Color) -> bool {
        (a.r - b.r).abs() < 0.02 && (a.g - b.g).abs() < 0.02 && (a.b - b.b).abs() < 0.02
    }

    #[test]
    fn text_scale_maps_point_size_to_font_scale() {
        assert_eq!(text_scale(16), 2); // 16 / 7 = 2
        assert_eq!(text_scale(7), 1);
        assert_eq!(text_scale(0), 1); // never zero
    }

    #[test]
    fn colorrect_emits_filled_rect_with_property_color() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let mut n = Node::new("Rect", "ColorRect");
        n.set_property("color", Variant::Color(Color::new(1.0, 0.0, 0.0, 1.0)));
        let id = tree.add_child(root, n).unwrap();
        control::apply_anchor_preset(&mut tree, id, AnchorPreset::FullRect);

        let mut vp = Viewport::new(64, 64, Color::BLACK);
        let theme = Theme::default_theme();
        render_control_tree(&mut tree, id, &theme, &mut vp, Vector2::new(64.0, 64.0));

        let items = vp.get_sorted_items();
        assert_eq!(items.len(), 1);
        match &items[0].commands[0] {
            DrawCommand::DrawRect { color, filled, .. } => {
                assert!(filled);
                assert!(approx(*color, Color::new(1.0, 0.0, 0.0, 1.0)));
            }
            other => panic!("expected DrawRect, got {other:?}"),
        }
    }

    #[test]
    fn panel_uses_theme_stylebox_bg() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let id = tree.add_child(root, Node::new("P", "Panel")).unwrap();
        control::apply_anchor_preset(&mut tree, id, AnchorPreset::FullRect);

        let mut vp = Viewport::new(32, 32, Color::BLACK);
        let theme = Theme::default_theme();
        render_control_tree(&mut tree, id, &theme, &mut vp, Vector2::new(32.0, 32.0));

        match &vp.get_sorted_items()[0].commands[0] {
            DrawCommand::DrawRect { color, .. } => {
                assert!(approx(*color, Color::new(0.15, 0.15, 0.15, 1.0)));
            }
            other => panic!("expected DrawRect, got {other:?}"),
        }
    }

    #[test]
    fn button_emits_bg_and_centered_text() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let mut n = Node::new("B", "Button");
        n.set_property("text", Variant::String("OK".to_owned()));
        let id = tree.add_child(root, n).unwrap();
        control::apply_anchor_preset(&mut tree, id, AnchorPreset::TopLeft);
        control::set_offset_right(&mut tree, id, 100.0);
        control::set_offset_bottom(&mut tree, id, 40.0);

        let mut vp = Viewport::new(128, 64, Color::BLACK);
        let theme = Theme::default_theme();
        render_control_tree(&mut tree, id, &theme, &mut vp, Vector2::new(128.0, 64.0));

        let cmds = &vp.get_sorted_items()[0].commands;
        assert_eq!(cmds.len(), 2, "button = bg rect + text");
        assert!(matches!(
            cmds[0],
            DrawCommand::DrawRect { filled: true, .. }
        ));
        match &cmds[1] {
            DrawCommand::DrawString { text, position, .. } => {
                assert_eq!(text, "OK");
                assert!(position.x > 0.0 && position.x < 100.0);
                assert!(position.y > 0.0 && position.y < 40.0);
            }
            other => panic!("expected DrawString, got {other:?}"),
        }
    }

    #[test]
    fn parse_bbcode_splits_color_spans() {
        let spans = parse_bbcode("hi [color=#ff0000]red[/color] bye", Color::WHITE);
        assert_eq!(spans.len(), 3);
        assert_eq!(spans[0].text, "hi ");
        assert!(approx(spans[0].color, Color::WHITE));
        assert_eq!(spans[1].text, "red");
        assert!(approx(spans[1].color, Color::new(1.0, 0.0, 0.0, 1.0)));
        assert_eq!(spans[2].text, " bye");
        assert!(approx(spans[2].color, Color::WHITE));
    }

    #[test]
    fn parse_bbcode_strips_formatting_tags() {
        let spans = parse_bbcode("[b]bold[/b] [i]it[/i]", Color::WHITE);
        let joined: String = spans.iter().map(|s| s.text.as_str()).collect();
        assert_eq!(joined, "bold it");
    }

    #[test]
    fn parse_bbcode_named_and_short_hex_colors() {
        assert!(approx(
            parse_bbcode_color("red").unwrap(),
            Color::new(1.0, 0.0, 0.0, 1.0)
        ));
        assert!(approx(
            parse_bbcode_color("#0f0").unwrap(),
            Color::new(0.0, 1.0, 0.0, 1.0)
        ));
        assert!(parse_bbcode_color("notacolor").is_none());
    }

    #[test]
    fn richtextlabel_emits_colored_spans() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let mut n = Node::new("R", "RichTextLabel");
        n.set_property(
            "bbcode_text",
            Variant::String("a[color=#00ff00]b[/color]".to_owned()),
        );
        let id = tree.add_child(root, n).unwrap();
        control::apply_anchor_preset(&mut tree, id, AnchorPreset::FullRect);

        let mut vp = Viewport::new(64, 32, Color::BLACK);
        let theme = Theme::default_theme();
        render_control_tree(&mut tree, id, &theme, &mut vp, Vector2::new(64.0, 32.0));

        let cmds = &vp.get_sorted_items()[0].commands;
        // Two DrawString spans: "a" (default) then "b" (green).
        let strings: Vec<_> = cmds
            .iter()
            .filter_map(|c| match c {
                DrawCommand::DrawString { text, color, .. } => Some((text.clone(), *color)),
                _ => None,
            })
            .collect();
        assert_eq!(strings.len(), 2);
        assert_eq!(strings[0].0, "a");
        assert_eq!(strings[1].0, "b");
        assert!(approx(strings[1].1, Color::new(0.0, 1.0, 0.0, 1.0)));
    }

    #[test]
    fn progressbar_fill_width_tracks_value() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let mut n = Node::new("PB", "ProgressBar");
        n.set_property("min_value", Variant::Float(0.0));
        n.set_property("max_value", Variant::Float(100.0));
        n.set_property("value", Variant::Float(25.0));
        let id = tree.add_child(root, n).unwrap();
        control::apply_anchor_preset(&mut tree, id, AnchorPreset::TopLeft);
        control::set_offset_right(&mut tree, id, 200.0);
        control::set_offset_bottom(&mut tree, id, 20.0);

        let mut vp = Viewport::new(256, 64, Color::BLACK);
        let theme = Theme::default_theme();
        render_control_tree(&mut tree, id, &theme, &mut vp, Vector2::new(256.0, 64.0));

        let cmds = &vp.get_sorted_items()[0].commands;
        // First rect = trough (full width), second = fill (25% width).
        let rects: Vec<Rect2> = cmds
            .iter()
            .filter_map(|c| match c {
                DrawCommand::DrawRect {
                    rect, filled: true, ..
                } => Some(*rect),
                _ => None,
            })
            .collect();
        assert_eq!(rects.len(), 2, "trough + fill");
        assert!((rects[0].size.x - 200.0).abs() < 0.5);
        assert!((rects[1].size.x - 50.0).abs() < 0.5, "25% of 200 = 50");
    }

    #[test]
    fn checkbox_checked_adds_inner_mark() {
        let make = |checked: bool| {
            let mut tree = SceneTree::new();
            let root = tree.root_id();
            let mut n = Node::new("C", "CheckBox");
            n.set_property("button_pressed", Variant::Bool(checked));
            n.set_property("text", Variant::String("On".to_owned()));
            let id = tree.add_child(root, n).unwrap();
            control::apply_anchor_preset(&mut tree, id, AnchorPreset::TopLeft);
            control::set_offset_right(&mut tree, id, 100.0);
            control::set_offset_bottom(&mut tree, id, 24.0);
            let mut vp = Viewport::new(128, 32, Color::BLACK);
            let theme = Theme::default_theme();
            render_control_tree(&mut tree, id, &theme, &mut vp, Vector2::new(128.0, 32.0));
            let cmds = &vp.get_sorted_items()[0].commands;
            cmds.iter()
                .filter(|c| matches!(c, DrawCommand::DrawRect { filled: true, .. }))
                .count()
        };
        // Unchecked: no filled rect (only the outline box). Checked: one filled mark.
        assert_eq!(make(false), 0);
        assert_eq!(make(true), 1);
    }

    #[test]
    fn optionbutton_draws_bg_text_and_arrow() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let mut n = Node::new("O", "OptionButton");
        n.set_property("text", Variant::String("Pick".to_owned()));
        let id = tree.add_child(root, n).unwrap();
        control::apply_anchor_preset(&mut tree, id, AnchorPreset::TopLeft);
        control::set_offset_right(&mut tree, id, 120.0);
        control::set_offset_bottom(&mut tree, id, 30.0);

        let mut vp = Viewport::new(128, 64, Color::BLACK);
        let theme = Theme::default_theme();
        render_control_tree(&mut tree, id, &theme, &mut vp, Vector2::new(128.0, 64.0));

        let cmds = &vp.get_sorted_items()[0].commands;
        let has_string = cmds
            .iter()
            .any(|c| matches!(c, DrawCommand::DrawString { text, .. } if text == "Pick"));
        let filled_rects = cmds
            .iter()
            .filter(|c| matches!(c, DrawCommand::DrawRect { filled: true, .. }))
            .count();
        assert!(has_string, "selected text drawn");
        assert!(filled_rects >= 2, "bg + arrow indicator");
    }

    #[test]
    fn tabcontainer_draws_panel_and_header() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let id = tree
            .add_child(root, Node::new("T", "TabContainer"))
            .unwrap();
        control::apply_anchor_preset(&mut tree, id, AnchorPreset::FullRect);

        let mut vp = Viewport::new(200, 200, Color::BLACK);
        let theme = Theme::default_theme();
        render_control_tree(&mut tree, id, &theme, &mut vp, Vector2::new(200.0, 200.0));

        let cmds = &vp.get_sorted_items()[0].commands;
        let filled = cmds
            .iter()
            .filter(|c| matches!(c, DrawCommand::DrawRect { filled: true, .. }))
            .count();
        assert!(filled >= 2, "panel bg + header strip");
    }

    #[test]
    fn nested_controls_accumulate_global_position() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let panel = tree.add_child(root, Node::new("Panel", "Panel")).unwrap();
        control::apply_anchor_preset(&mut tree, panel, AnchorPreset::TopLeft);
        control::set_offset_left(&mut tree, panel, 10.0);
        control::set_offset_top(&mut tree, panel, 10.0);
        control::set_offset_right(&mut tree, panel, 210.0);
        control::set_offset_bottom(&mut tree, panel, 210.0);

        let btn = tree.add_child(panel, Node::new("Btn", "Button")).unwrap();
        control::apply_anchor_preset(&mut tree, btn, AnchorPreset::TopLeft);
        control::set_offset_left(&mut tree, btn, 20.0);
        control::set_offset_top(&mut tree, btn, 20.0);
        control::set_offset_right(&mut tree, btn, 120.0);
        control::set_offset_bottom(&mut tree, btn, 60.0);

        let mut vp = Viewport::new(256, 256, Color::BLACK);
        let theme = Theme::default_theme();
        render_control_tree(
            &mut tree,
            panel,
            &theme,
            &mut vp,
            Vector2::new(256.0, 256.0),
        );

        let items = vp.get_sorted_items();
        assert_eq!(items.len(), 2, "panel + button");
        // items sorted by z_index; panel (seq 0) first, button (seq 1) second.
        match &items[1].commands[0] {
            DrawCommand::DrawRect { rect, .. } => {
                assert!((rect.position.x - 30.0).abs() < 0.01, "global x=30");
                assert!((rect.position.y - 30.0).abs() < 0.01, "global y=30");
            }
            other => panic!("expected button DrawRect, got {other:?}"),
        }
    }
}
