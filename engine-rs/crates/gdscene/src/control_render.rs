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
const GLYPH_W: f32 = 5.0;
const GLYPH_H: f32 = 7.0;
const GLYPH_GAP: f32 = 1.0;

/// Control classes handled by the render pass. Used as a fallback membership
/// test in case the ClassDB inheritance chain is unavailable for `is_class`.
const CONTROL_CLASSES: &[&str] = &[
    "Control",
    "Panel",
    "PanelContainer",
    "Button",
    "Label",
    "ColorRect",
    "TextureRect",
    "RichTextLabel",
    // pat-nzgdu.198 — TextEdit/CodeEdit draw their text (and syntax-highlighted
    // keyword spans) in draw_control; without them here is_control() rejects the
    // node when the ClassDB Control chain is unavailable, and the draw arm never
    // runs.
    "TextEdit",
    "CodeEdit",
    "ProgressBar",
    "CheckBox",
    "CheckButton",
    "OptionButton",
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
    render_control_tree(tree, root, theme, &mut viewport, viewport_size);

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
    let mut order: u64 = 0;
    render_node(
        tree,
        root,
        theme,
        viewport,
        Vector2::ZERO,
        viewport_size,
        &mut order,
    );
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
) {
    if is_control(tree, node_id) {
        // Resolve this control's rect from anchors/offsets against the parent
        // rect. resolve_control_layout stores a parent-relative position.
        control::resolve_control_layout(tree, node_id, parent_size);
        let local_pos = control::get_position(tree, node_id);
        let size = control::get_size(tree, node_id);
        let global_pos = parent_global + local_pos;
        let rect = Rect2::new(global_pos, size);

        let seq = *order;
        *order += 1;
        let mut item = CanvasItem::new(CanvasItemId(seq + 1));
        // Pre-order sequence as z so children (added later) paint on top.
        item.z_index = seq as i32;
        draw_control(tree, node_id, theme, rect, &mut item.commands);
        if !item.commands.is_empty() {
            viewport.add_canvas_item(item);
        }

        // Children anchor against this control's rect in its global space.
        for child in children_of(tree, node_id) {
            render_node(tree, child, theme, viewport, global_pos, size, order);
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
            );
        }
    }
}

/// Reads a `Color`-typed node property, if present.
fn color_prop(tree: &SceneTree, node_id: NodeId, key: &str) -> Option<Color> {
    match tree.get_node(node_id)?.get_property(key) {
        Variant::Color(c) => Some(c),
        _ => None,
    }
}

/// Reads a `String`-typed node property, if present.
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
    match theme.get_item(ThemePropertyType::Stylebox, control_type, property) {
        Some(Variant::Color(c)) => *c,
        _ => fallback,
    }
}

/// Maps a theme font size (in points) to the built-in 5×7 font's integer pixel
/// scale so glyph height ≈ the requested size.
fn text_scale(font_size: i64) -> u32 {
    (font_size.max(1) as u32 / GLYPH_H as u32).max(1)
}

/// Approximate pixel width of `text` at the given scale, using the nominal
/// 5×7 monospace metrics.
fn text_width(text: &str, scale: u32) -> f32 {
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
                out.push(DrawCommand::DrawString {
                    text,
                    position: rect.position,
                    color: font_color,
                    font_size: scale,
                });
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
            // Selected item text (falls back to `text`).
            let text = string_prop(tree, node_id, "text")
                .filter(|s| !s.is_empty())
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
            let tab_bar_h = crate::control::TAB_BAR_HEIGHT.min(rect.size.y);
            let header_bg = stylebox_bg(
                theme,
                "TabContainer",
                "tab_selected",
                Color::new(0.22, 0.22, 0.22, 1.0),
            );
            out.push(DrawCommand::DrawRect {
                rect: Rect2::new(rect.position, Vector2::new(rect.size.x, tab_bar_h)),
                color: header_bg,
                filled: true,
            });
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
