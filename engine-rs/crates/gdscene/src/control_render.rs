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
    // pat-s3rrv.233: without membership here the draw walk never reaches the
    // `"GraphEdit"` arm, so the grid, the nodes and every connection stay
    // invisible — the same failure MenuBar (.72), LinkButton (.166) and the
    // pickers (.172) each hit.
    //
    // ⚠️ `GraphNode` is deliberately absent: its panel is painted by the
    // GraphEdit arm at `position_offset`, and adding it here would paint a second
    // panel at the anchor-derived rect instead.
    "GraphEdit",
    // pat-s3rrv.72: without membership here the draw walk skips the node
    // entirely and the `"MenuBar"` arm below is unreachable — the strip would
    // be laid out, clickable and invisible.
    "MenuBar",
    // pat-s3rrv.166: LinkButton was in CONTROL_HIT_CLASSES and BUTTON_CLASSES
    // but not here — clickable, focusable and invisible, the same failure the
    // note above describes.
    "LinkButton",
    // pat-s3rrv.172: same story again — both picker classes are registered and
    // load from a `.tscn`, and neither was ever painted.
    "ColorPicker",
    "ColorPickerButton",
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
    // pat-s3rrv.69 — the Range family. `CONTROL_HIT_CLASSES` already accepted
    // these, so an authored slider was clickable and focusable while drawing
    // nothing; without them here `is_control` rejects the node whenever the
    // ClassDB Control chain is unavailable and the new arms never run.
    "HScrollBar",
    "HSlider",
    "VScrollBar",
    "VSlider",
    "SpinBox",
    "TextureProgressBar",
    "CheckBox",
    "CheckButton",
    "OptionButton",
    // pat-0q2kx.85 — the item views draw rows/tabs in `draw_control`; without
    // them here `is_control` rejects the node whenever the ClassDB Control chain
    // is unavailable and the arm never runs.
    "ItemList",
    "TabBar",
    "TabContainer",
    // pat-s3rrv.75. `Tree` was already in `CONTROL_HIT_CLASSES` (control.rs:941)
    // and missing here, so it absorbed clicks while never drawing.
    "Tree",
    // pat-s3rrv.165 — same pairing: FoldableContainer is in
    // `CONTROL_HIT_CLASSES` so its title bar can take a click, and needs to be
    // here so the title bar it is taking clicks on is actually drawn.
    "FoldableContainer",
    // pat-s3rrv.170 — the third instance of that pairing. TextureButton was in
    // `BUTTON_CLASSES` and `CONTROL_HIT_CLASSES` (control.rs:829, :869), so it
    // took clicks and fired `pressed` while drawing nothing at all.
    "TextureButton",
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
    // pat-s3rrv.167. The `"NinePatchRect"` draw arm existed and was never
    // reached: membership here is what makes the walk descend to a Control, and
    // without it the class emitted NOTHING — every nine-patch test failed with
    // an empty command list while the arm below looked perfectly correct.
    //
    // ⚠️ Same bug as the `GraphEdit` note at the top of this list. That is twice
    // now: adding a draw arm is only half of wiring a Control, and the missing
    // half fails silently rather than at compile time.
    "NinePatchRect",
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
    control_draw_items_themed(
        tree,
        root,
        theme,
        &std::collections::HashMap::new(),
        viewport_size,
    )
}

/// [`control_draw_items`] with per-node theme resources (pat-s3rrv.38).
///
/// `node_themes` maps a node to the `Theme` its `theme` property resolved to.
/// Each entry joins the chain for that node and its descendants, so an authored
/// theme on a Panel reaches a Button several levels below it — the behaviour
/// `control::resolve_theme_item` implemented and nothing ever called.
///
/// The plain [`control_draw_items`] forwards an empty map, so every existing
/// caller keeps the previous single-theme behaviour exactly.
pub fn control_draw_items_themed(
    tree: &mut SceneTree,
    root: NodeId,
    theme: &Theme,
    node_themes: &std::collections::HashMap<NodeId, Theme>,
    viewport_size: Vector2,
) -> Vec<ControlDrawItem> {
    let mut viewport = Viewport::new(
        viewport_size.x.max(1.0) as u32,
        viewport_size.y.max(1.0) as u32,
        Color::new(0.0, 0.0, 0.0, 0.0),
    );
    let mut fonts: std::collections::HashMap<u64, String> = std::collections::HashMap::new();
    render_control_tree_with_fonts(
        tree,
        root,
        theme,
        node_themes,
        &mut viewport,
        viewport_size,
        &mut fonts,
    );

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
    // pat-s3rrv.72: the open popup paints LAST so it composites above every
    // control, which is what a Popup<-Window does in Godot. Appended after the
    // flattened stream rather than walked with the tree — it is not part of the
    // scene's layout and must not be re-ordered by z against it.
    if let Some((origin, width, menu, hovered)) = tree.open_popup_draw() {
        let row_h = crate::popup::PopupMenu::ROW_HEIGHT;
        let z = i32::MAX - 1;
        out.push(ControlDrawItem::Rect {
            rect: Rect2::new(origin, Vector2::new(width, menu.height())),
            color: Color::new(0.16, 0.16, 0.18, 1.0),
            filled: true,
            z,
        });
        for idx in 0..menu.item_count() {
            let Some(item) = menu.get_item(idx) else {
                continue;
            };
            let top = origin.y + idx as f32 * row_h;
            if item.separator {
                // Godot draws a separator as a rule, not a row of text.
                out.push(ControlDrawItem::Line {
                    from: Vector2::new(origin.x + 4.0, top + row_h * 0.5),
                    to: Vector2::new(origin.x + width - 4.0, top + row_h * 0.5),
                    color: Color::new(0.45, 0.45, 0.48, 1.0),
                    width: 1.0,
                    z: z + 1,
                });
                continue;
            }
            if hovered == Some(idx) && !item.disabled {
                out.push(ControlDrawItem::Rect {
                    rect: Rect2::new(Vector2::new(origin.x, top), Vector2::new(width, row_h)),
                    color: Color::new(0.26, 0.36, 0.56, 1.0),
                    filled: true,
                    z: z + 1,
                });
            }
            out.push(ControlDrawItem::Text {
                text: item.text.clone(),
                // Left padding leaves room for the check mark drawn below.
                position: Vector2::new(origin.x + 20.0, top + 4.0),
                color: if item.disabled {
                    Color::new(0.5, 0.5, 0.5, 1.0)
                } else {
                    Color::new(0.9, 0.9, 0.9, 1.0)
                },
                font_size: 14,
                font_path: None,
                z: z + 1,
            });
            if item.checkable && item.checked {
                out.push(ControlDrawItem::Line {
                    from: Vector2::new(origin.x + 6.0, top + row_h * 0.5),
                    to: Vector2::new(origin.x + 14.0, top + row_h - 6.0),
                    color: Color::new(0.9, 0.9, 0.9, 1.0),
                    width: 2.0,
                    z: z + 1,
                });
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
    // No per-node themes on this entry point — it keeps the single-theme
    // behaviour (pat-s3rrv.38).
    render_control_tree_with_fonts(
        tree,
        root,
        theme,
        &std::collections::HashMap::new(),
        viewport,
        viewport_size,
        &mut fonts,
    );
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
    node_themes: &std::collections::HashMap<NodeId, Theme>,
    viewport: &mut Viewport,
    viewport_size: Vector2,
    fonts: &mut std::collections::HashMap<u64, String>,
) {
    let mut order: u64 = 0;
    render_node(
        tree,
        root,
        theme,
        node_themes,
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

// ---------------------------------------------------------------------------
// GraphEdit / GraphNode drawing (pat-s3rrv.233)
// ---------------------------------------------------------------------------
//
// The connection model already existed in `graph_edit.rs` and was reachable from
// GDScript for `connect_node` / `disconnect_node` / `get_connection_list` — but
// nothing drew any of it, so a graph could be built and never seen.
//
// ⚠️ The WHOLE graph is drawn from the `GraphEdit` arm, including its child
// GraphNodes. Godot positions a GraphNode by `position_offset` rather than by
// anchors, so the layout walk's rect is not where the node belongs; drawing the
// children here means the panels, the port dots and the connection lines all
// derive from ONE rect function and cannot disagree. That is also why `GraphNode`
// is deliberately NOT added to `CONTROL_CLASSES` — a second, anchor-positioned
// panel would be painted underneath every node.

/// Height of a GraphNode's titlebar strip.
const GRAPH_TITLEBAR_H: f32 = 20.0;
/// Vertical distance between successive port dots.
const GRAPH_PORT_PITCH: f32 = 18.0;
/// Radius of a port dot.
const GRAPH_PORT_RADIUS: f32 = 4.0;
/// Spacing of the background grid.
const GRAPH_GRID_STEP: f32 = 32.0;
/// Default size for a GraphNode that declares none.
const GRAPH_NODE_DEFAULT_SIZE: (f32, f32) = (120.0, 60.0);

/// Reads a `Vector2` property, mirroring `float_prop` / `color_prop`.
fn vec2_prop(tree: &SceneTree, node_id: NodeId, key: &str) -> Option<Vector2> {
    match tree.get_node(node_id)?.get_property(key) {
        Variant::Vector2(v) => Some(v),
        _ => None,
    }
}

/// The rect a GraphNode occupies inside its GraphEdit.
///
/// Godot's `GraphElement.position_offset` is the graph-space position; the node's
/// `size` is its own. Component arithmetic rather than `Vector2 + Vector2` so this
/// does not depend on which operator impls exist.
pub(crate) fn graph_node_rect(tree: &SceneTree, edit_rect: Rect2, node_id: NodeId) -> Rect2 {
    let off = vec2_prop(tree, node_id, "position_offset").unwrap_or(Vector2::new(0.0, 0.0));
    let size = vec2_prop(tree, node_id, "size")
        .or_else(|| vec2_prop(tree, node_id, "custom_minimum_size"))
        .unwrap_or(Vector2::new(
            GRAPH_NODE_DEFAULT_SIZE.0,
            GRAPH_NODE_DEFAULT_SIZE.1,
        ));
    Rect2::new(
        Vector2::new(edit_rect.position.x + off.x, edit_rect.position.y + off.y),
        size,
    )
}

/// Where a GraphNode's port dot sits, in the same space as its rect.
///
/// ⚠️ SHARED BY THE DOTS AND THE LINES ON PURPOSE. If the connection line ever
/// derived its endpoint separately from the dot, a line could point at empty
/// space and the frame would still look entirely plausible — the two are one
/// quantity and must come from one function. The acceptance test asserts the line
/// endpoint equals the dot centre for exactly this reason.
pub(crate) fn graph_port_position(node_rect: Rect2, port: u32, is_output: bool) -> Vector2 {
    let y = node_rect.position.y + GRAPH_TITLEBAR_H + GRAPH_PORT_PITCH * (port as f32 + 0.5);
    let x = if is_output {
        node_rect.position.x + node_rect.size.x
    } else {
        node_rect.position.x
    };
    Vector2::new(x, y)
}

/// Samples Godot's connection curve between two ports.
///
/// Godot draws a cubic bezier whose control points are pushed horizontally out of
/// each port, so the line leaves an output rightwards and enters an input
/// leftwards regardless of their relative positions. Emitted as a polyline
/// because that is the primitive this renderer has.
fn graph_connection_points(from: Vector2, to: Vector2) -> Vec<Vector2> {
    // The horizontal pull scales with the gap, matching Godot's look when nodes
    // are close together as well as far apart.
    let pull = ((to.x - from.x).abs() * 0.5).max(24.0);
    let c1 = Vector2::new(from.x + pull, from.y);
    let c2 = Vector2::new(to.x - pull, to.y);
    const STEPS: usize = 12;
    let mut pts = Vec::with_capacity(STEPS + 1);
    for i in 0..=STEPS {
        let t = i as f32 / STEPS as f32;
        let u = 1.0 - t;
        // Cubic bezier, written out rather than looped: four terms is clearer
        // than a de Casteljau loop and cannot get the weights wrong.
        let w0 = u * u * u;
        let w1 = 3.0 * u * u * t;
        let w2 = 3.0 * u * t * t;
        let w3 = t * t * t;
        pts.push(Vector2::new(
            w0 * from.x + w1 * c1.x + w2 * c2.x + w3 * to.x,
            w0 * from.y + w1 * c1.y + w2 * c2.y + w3 * to.y,
        ));
    }
    pts
}

/// Draws the grid, every child GraphNode, and every connection between them.
fn draw_graph_edit(tree: &SceneTree, node_id: NodeId, rect: Rect2, out: &mut Vec<DrawCommand>) {
    let bg =
        color_prop(tree, node_id, "background_color").unwrap_or(Color::new(0.13, 0.13, 0.16, 1.0));
    out.push(DrawCommand::DrawRect {
        rect,
        color: bg,
        filled: true,
    });

    // Grid background. Godot offers lines or dots; lines is the default pattern.
    if bool_prop(tree, node_id, "show_grid").unwrap_or(true) {
        let grid = Color::new(1.0, 1.0, 1.0, 0.06);
        let mut x = rect.position.x;
        while x <= rect.position.x + rect.size.x {
            out.push(DrawCommand::DrawLine {
                from: Vector2::new(x, rect.position.y),
                to: Vector2::new(x, rect.position.y + rect.size.y),
                color: grid,
                width: 1.0,
            });
            x += GRAPH_GRID_STEP;
        }
        let mut y = rect.position.y;
        while y <= rect.position.y + rect.size.y {
            out.push(DrawCommand::DrawLine {
                from: Vector2::new(rect.position.x, y),
                to: Vector2::new(rect.position.x + rect.size.x, y),
                color: grid,
                width: 1.0,
            });
            y += GRAPH_GRID_STEP;
        }
    }

    // The model is keyed by node NAME, which is also how `GraphConnection`
    // addresses its endpoints, so the child lookup is by name throughout.
    let model = tree.get_graph_edit(node_id);

    for child in children_of(tree, node_id) {
        let Some(node) = tree.get_node(child) else {
            continue;
        };
        if node.class_name() != "GraphNode" {
            continue;
        }
        let name = node.name().to_string();
        let node_rect = graph_node_rect(tree, rect, child);

        // Panel, then titlebar on top of it.
        out.push(DrawCommand::DrawRect {
            rect: node_rect,
            color: Color::new(0.22, 0.23, 0.28, 1.0),
            filled: true,
        });
        out.push(DrawCommand::DrawRect {
            rect: Rect2::new(
                node_rect.position,
                Vector2::new(node_rect.size.x, GRAPH_TITLEBAR_H),
            ),
            color: Color::new(0.30, 0.31, 0.38, 1.0),
            filled: true,
        });

        // The title comes from the node property, falling back to the model and
        // then the node name — Godot shows the node name when no title is set.
        let title = string_prop(tree, child, "title")
            .or_else(|| {
                model
                    .and_then(|m| m.get_node(&name))
                    .map(|n| n.title.clone())
            })
            .unwrap_or_else(|| name.clone());
        out.push(DrawCommand::DrawString {
            text: title,
            position: Vector2::new(node_rect.position.x + 4.0, node_rect.position.y + 4.0),
            color: Color::WHITE,
            font_size: 12,
        });

        // Port dots, from the slot model this node declared.
        if let Some(gn) = model.and_then(|m| m.get_node(&name)) {
            for port in 0..gn.get_input_port_count() {
                out.push(DrawCommand::DrawCircle {
                    center: graph_port_position(node_rect, port as u32, false),
                    radius: GRAPH_PORT_RADIUS,
                    color: Color::new(0.4, 0.8, 1.0, 1.0),
                });
            }
            for port in 0..gn.get_output_port_count() {
                out.push(DrawCommand::DrawCircle {
                    center: graph_port_position(node_rect, port as u32, true),
                    radius: GRAPH_PORT_RADIUS,
                    color: Color::new(1.0, 0.8, 0.4, 1.0),
                });
            }
        }
    }

    // Connections last so they paint over the panels, as Godot's do.
    let Some(model) = model else {
        return;
    };
    for conn in model.get_connection_list() {
        let (Some(from_id), Some(to_id)) = (
            graph_child_by_name(tree, node_id, &conn.from_node),
            graph_child_by_name(tree, node_id, &conn.to_node),
        ) else {
            continue;
        };
        let from = graph_port_position(graph_node_rect(tree, rect, from_id), conn.from_port, true);
        let to = graph_port_position(graph_node_rect(tree, rect, to_id), conn.to_port, false);
        out.push(DrawCommand::DrawPolyline {
            points: graph_connection_points(from, to),
            color: Color::new(0.8, 0.8, 0.9, 1.0),
            width: 2.0,
            closed: false,
        });
    }
}

/// The port under `point`, as `(node name, port index, is_output)`.
///
/// ⚠️ Lives beside the drawing on purpose and reuses `graph_port_position`, so a
/// port can never be hit somewhere other than where its dot was painted. Splitting
/// hit-testing into `scene_tree.rs` would let the two drift apart silently — the
/// dot would render in one place and accept clicks in another, which looks like
/// "the drag sometimes doesn't take" rather than like a geometry bug.
pub(crate) fn graph_port_at(
    tree: &SceneTree,
    edit: NodeId,
    edit_rect: Rect2,
    point: Vector2,
) -> Option<(String, u32, bool)> {
    // A little larger than the dot: Godot's ports are forgiving to click, and an
    // exact-radius test makes a 4px target practically unhittable.
    let slop = GRAPH_PORT_RADIUS + 4.0;
    let model = tree.get_graph_edit(edit)?;
    for child in children_of(tree, edit) {
        let Some(node) = tree.get_node(child) else {
            continue;
        };
        if node.class_name() != "GraphNode" {
            continue;
        }
        let name = node.name().to_string();
        let Some(gn) = model.get_node(&name) else {
            continue;
        };
        let rect = graph_node_rect(tree, edit_rect, child);
        for (count, is_output) in [
            (gn.get_input_port_count(), false),
            (gn.get_output_port_count(), true),
        ] {
            for port in 0..count {
                let c = graph_port_position(rect, port as u32, is_output);
                let dx = c.x - point.x;
                let dy = c.y - point.y;
                if dx * dx + dy * dy <= slop * slop {
                    return Some((name, port as u32, is_output));
                }
            }
        }
    }
    None
}

/// The GraphNode whose titlebar is under `point`, if any.
pub(crate) fn graph_titlebar_at(
    tree: &SceneTree,
    edit: NodeId,
    edit_rect: Rect2,
    point: Vector2,
) -> Option<NodeId> {
    for child in children_of(tree, edit) {
        let Some(node) = tree.get_node(child) else {
            continue;
        };
        if node.class_name() != "GraphNode" {
            continue;
        }
        let r = graph_node_rect(tree, edit_rect, child);
        if point.x >= r.position.x
            && point.x <= r.position.x + r.size.x
            && point.y >= r.position.y
            && point.y <= r.position.y + GRAPH_TITLEBAR_H
        {
            return Some(child);
        }
    }
    None
}

/// Finds a GraphEdit's direct child by node name.
pub(crate) fn graph_child_by_name(tree: &SceneTree, edit: NodeId, name: &str) -> Option<NodeId> {
    children_of(tree, edit)
        .into_iter()
        .find(|c| tree.get_node(*c).map(|n| n.name() == name).unwrap_or(false))
}

fn render_node(
    tree: &mut SceneTree,
    node_id: NodeId,
    theme: &Theme,
    node_themes: &std::collections::HashMap<NodeId, Theme>,
    viewport: &mut Viewport,
    parent_global: Vector2,
    parent_size: Vector2,
    order: &mut u64,
    clip: Option<Rect2>,
    parent_is_container: bool,
    fonts: &mut std::collections::HashMap<u64, String>,
) {
    // pat-s3rrv.38: a theme assigned to THIS node joins the chain for it and
    // everything below it.
    //
    // The walk is already top-down, so accumulating here gives Godot's
    // precedence for free — a nearer theme merges over a farther one, and the
    // node's own theme lands last and wins. That is why this is done during the
    // descent rather than by walking back up per lookup: it is O(1) per node
    // instead of O(depth), and every existing `theme.get_*` in this file keeps
    // working untouched because it still receives one flat `&Theme`.
    let merged;
    let theme = match node_themes.get(&node_id) {
        Some(own) => {
            let mut t = theme.clone();
            t.merge_with(own);
            merged = t;
            &merged
        }
        None => theme,
    };

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
        // `fit_content` feeds the solver, so it has to be written BEFORE the
        // solve reads `min_size` (pat-s3rrv.74). No-op for every class but
        // RichTextLabel, and for a RichTextLabel that has not opted in.
        if class_of(tree, node_id) == "RichTextLabel" {
            let fs = theme
                .get_font_size("RichTextLabel", "font_size")
                .unwrap_or(RICHTEXT_DEFAULT_FONT_SIZE);
            apply_richtext_fit_content(tree, node_id, fs);
        }
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
        // A scripted StyleBox has to run before the control is drawn, and
        // running a script needs `&mut` (pat-s3rrv.160). This is the one place
        // that has both: the walk holds the tree mutably and `draw_control`
        // only reborrows it. Doing it here also means the box sees the rect the
        // control actually resolved to, which is the argument Godot passes.
        //
        // Cheap for the common case: the scan finds no `__resscript__`
        // companion on an ordinary control and returns without running anything.
        tree.flush_scripted_resources(&[(node_id, rect)]);
        // Same reason, same place: a script-defined RichTextEffect has to run
        // before the label is drawn and running it needs `&mut` (pat-fk17e.2).
        // Returns immediately unless this node is a RichTextLabel carrying
        // `custom_effects`.
        flush_custom_char_fx(tree, node_id);
        draw_control(tree, node_id, theme, rect, &mut item.commands);
        if !item.commands.is_empty() {
            viewport.add_canvas_item(item);
        }

        // Children anchor against this control's rect in its global space.
        let class = class_of(tree, node_id);
        // A PanelContainer insets its children by the `panel` stylebox's content
        // margins (pat-0q2kx.148). This happens here, not in `arrange_container`,
        // because resolving that stylebox needs the theme and the layout pipeline
        // carries none — see `control::arrange_panel_container`.
        //
        // ⚠️ This is also what makes `StyleBoxEmpty`'s margins observable at all:
        // Empty draws nothing, so padding is its *entire* purpose, and before
        // this the margins were parsed, stored, and then never read by anything.
        let is_container = if class == "PanelContainer" {
            let margins = theme
                .get_stylebox("PanelContainer", "panel")
                .or_else(|| theme.get_stylebox("Panel", "panel"))
                .map(|sb| sb.content_margins())
                .unwrap_or([0.0; 4]);
            control::arrange_panel_container(tree, node_id, size, margins);
            true
        } else {
            control::is_container_class_name(&class)
        };
        for child in children_of(tree, node_id) {
            render_node(
                tree,
                child,
                theme,
                node_themes,
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
                node_themes,
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

/// Formats a SpinBox's value the way Godot's embedded LineEdit shows it.
///
/// Godot prints as many decimals as `step` implies, so an integer step reads
/// `50` rather than `50.000000`. A whole-number step with a fractional value
/// still shows the fraction — the step controls the *display precision*, not a
/// second round of snapping, which `set_range_value` has already applied.
fn format_spinbox_value(value: f64, step: f64) -> String {
    if step >= 1.0 && (value.fract()).abs() < 1e-9 {
        return format!("{}", value as i64);
    }
    // Decimals implied by the step: 0.01 -> 2, 0.1 -> 1.
    let decimals = if step > 0.0 && step < 1.0 {
        (-step.log10()).ceil().clamp(0.0, 6.0) as usize
    } else {
        2
    };
    format!("{value:.decimals$}")
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
/// The theme a Control actually draws with, flattened from Godot's resolution
/// chain (pat-s3rrv.38).
///
/// Godot resolves each theme item through: the node's own theme resource, then
/// each ancestor's nearest-first, then the project/ThemeDB default. Patina had
/// `control::resolve_theme_item` implementing exactly that — with
/// `theme_type_variation` support — and it had ZERO production callers, so a
/// `theme` assigned in a `.tscn`, a theme on a parent, and `gui/theme/custom`
/// all changed nothing on screen.
///
/// This flattens the chain into one `Theme` rather than resolving item by item,
/// so the ~19 existing lookups and the 15 `stylebox_bg` call sites keep working
/// unchanged. `Theme::merge_with` overwrites on collision, so merging from the
/// DEFAULT outward — farthest ancestor first, the node's own theme last — gives
/// exactly Godot's precedence.
///
/// Node-local `theme_override_*` properties are NOT folded in here: they are
/// per-property and are already read straight off the node, which correctly
/// leaves them beating everything this returns.
pub fn effective_theme(
    tree: &SceneTree,
    node_id: NodeId,
    node_themes: &std::collections::HashMap<NodeId, Theme>,
    default: &Theme,
) -> Theme {
    // Walk up collecting the chain, nearest first...
    let mut chain: Vec<&Theme> = Vec::new();
    let mut current = Some(node_id);
    while let Some(id) = current {
        if let Some(t) = node_themes.get(&id) {
            chain.push(t);
        }
        current = tree.get_node(id).and_then(|n| n.parent());
    }

    // ...then merge farthest-first so nearer themes overwrite farther ones and
    // the node's own theme wins outright.
    let mut out = default.clone();
    for theme in chain.iter().rev() {
        out.merge_with(theme);
    }
    out
}

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

/// The built-in TTF face, parsed once (pat-s3rrv.37).
///
/// `TtfFont::builtin()` parses an embedded face, so it is memoized rather than
/// rebuilt per measurement — layout measures every Control's text on every
/// frame.
fn builtin_face() -> &'static gdrender2d::TtfFont {
    static FACE: std::sync::OnceLock<gdrender2d::TtfFont> = std::sync::OnceLock::new();
    FACE.get_or_init(gdrender2d::TtfFont::builtin)
}

/// Width of `text` at an exact `font_size` in PIXELS, using the real font's
/// advances and kerning (pat-s3rrv.37).
///
/// This is what Godot measures with — `Font::get_string_size` over the
/// TextServer's shaped advances — and it is what the rasterizer actually draws,
/// since the software renderer renders proportional TTF glyphs.
///
/// The [`text_width`] below is the placeholder it replaces: it charges every
/// glyph a flat 5px and takes an integer SCALE rather than a pixel size, so
/// `font_size` 16 and 20 both quantise to scale 2 and measure identically, and a
/// measured width never matches the drawn width for any string.
pub(crate) fn measure_text_px(text: &str, font_size: f32) -> f32 {
    if text.is_empty() {
        return 0.0;
    }
    builtin_face().measure_line(text, font_size.max(1.0))
}

/// Line height at an exact `font_size` in pixels (pat-s3rrv.37).
///
/// Godot's line height comes from the font's ascent + descent, not from a
/// nominal glyph box. Until the ascent/descent accessors are threaded through,
/// this uses the font size itself, which is already exact where `GLYPH_H`-based
/// arithmetic was quantised.
pub(crate) fn line_height_px(font_size: f32) -> f32 {
    font_size.max(1.0)
}

/// Approximate pixel width of `text` at the given scale, using the nominal
/// 5×7 monospace metrics.
///
/// ⚠️ PLACEHOLDER — superseded by [`measure_text_px`] (pat-s3rrv.37). Still used
/// by callers that have not been converted to carry a pixel font size.
pub(crate) fn text_width(text: &str, scale: u32) -> f32 {
    let n = text.chars().count() as f32;
    if n <= 0.0 {
        return 0.0;
    }
    (n * GLYPH_W + (n - 1.0) * GLYPH_GAP) * scale as f32
}

/// A paragraph's horizontal alignment — Godot's `[center]` / `[right]` /
/// `[left]` / `[fill]`, each of which calls `push_paragraph`
/// (rich_text_label.cpp:5675-5688).
///
/// ⚠️ Deliberately NOT [`crate::control::TextAlign`], which is a different
/// thing: that one backs a Control's `h_align`/`v_align` properties and has no
/// `Fill`. Adding `Fill` to it would ripple through every exhaustive match on it
/// (gdeditor's viewport renderer included) for the sake of a value only rich
/// text can produce. Two small types beat one shared type that means two things
/// — but they must not share a NAME, which is why this one is `ParagraphAlign`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ParagraphAlign {
    #[default]
    Left,
    Center,
    Right,
    /// ⚠️ Godot justifies the line by stretching inter-word spacing. This
    /// renderer has no word model — it lays out runs, not words — so `[fill]`
    /// currently renders left-aligned rather than pretending to justify.
    /// Recorded as a divergence instead of silently aliasing it to Left.
    Fill,
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
    /// Which line this run sits on. Bumped by a literal `\n` and by each
    /// paragraph tag.
    line: usize,
    /// The alignment of the line this run belongs to (constant within a line).
    align: ParagraphAlign,
    /// The `[bgcolor=…]` fill behind this run, if any.
    bgcolor: Option<Color>,
    /// Whether `[u]` covers this run.
    underline: bool,
    /// Whether `[s]` covers this run.
    strike: bool,
    /// Whether `[b]` covers this run (pat-s3rrv.74).
    ///
    /// ⚠️ Carried as an ATTRIBUTE, not a glyph change: the built-in 5x7 bitmap
    /// font has no weight variant, so the draw path emboldens by over-striking.
    /// The attribute is what a consumer with a real font needs.
    bold: bool,
    /// Whether `[i]` covers this run.
    italic: bool,
    /// How many `[indent]` levels enclose this run's LINE (pat-s3rrv.74).
    ///
    /// ⚠️ Line-level, not run-level, and deliberately so: an indent moves a
    /// line's left edge and narrows the width it wraps against, and a single
    /// line cannot have two left edges. A run therefore reports the depth in
    /// effect for its line rather than at its own character offset.
    indent: usize,
    /// The `[url=…]` payload covering this run, if any — Godot's "meta".
    ///
    /// Carried per-run rather than as a separate region list so the text and its
    /// clickable extent can never disagree: the rect a click is tested against
    /// is measured from this very span (pat-s3rrv.74).
    meta: Option<String>,
}

/// Parses a BBCode subset into styled spans. Recognized tags:
/// `[color=#rrggbb]`/`[color=name]` … `[/color]` set the run color;
/// `[url=target]` … `[/url]` attaches a meta payload (what `meta_clicked`
/// carries); `[u]` and `[s]` mark the run underlined / struck through.
///
/// `[b]` and `[i]` set run attributes. ⚠️ Only `[b]` is RENDERED: the built-in
/// 5x7 bitmap font has no weight or slant variant, so there is no bold face to
/// select — but weight can be faked by over-striking the run one pixel across,
/// which thickens every stem by one pixel. Slant cannot be faked the same way,
/// because shearing a bitmap glyph means resampling it, so `[i]` is parsed and
/// carried but not yet drawn differently. Underline and strikethrough are
/// *rules* rather than glyph shapes, so they are honoured exactly (pat-s3rrv.74).
///
/// Unknown `[...]` tags are dropped. `default` colors untagged text.
fn parse_bbcode(src: &str, default: Color) -> Vec<TextSpan> {
    parse_bbcode_with_effects(src, default, &[])
}

/// [`parse_bbcode`] plus the label's script-defined effects (pat-fk17e.2).
///
/// `custom` is `(effect object id, its `bbcode` tag)`. A tag that is not a
/// built-in is looked up here, so `[mytag]` becomes a
/// [`BuiltinEffect::Custom`] span instead of being stripped.
///
/// Kept as a separate entry point rather than widening [`parse_bbcode`]: that
/// name has three production callers and several in tests, and only the layout
/// path has a node to read `custom_effects` from. The others genuinely have no
/// effects to pass and should not have to say so.
fn parse_bbcode_with_effects(src: &str, default: Color, custom: &[(u64, String)]) -> Vec<TextSpan> {
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
                 meta: Option<String>,
                 bgcolor: Option<Color>,
                 underline: bool,
                 strike: bool,
                 bold: bool,
                 italic: bool,
                 line: usize,
                 align: ParagraphAlign,
                 spans: &mut Vec<TextSpan>| {
        if !cur.is_empty() {
            spans.push(TextSpan {
                text: std::mem::take(cur),
                color,
                effect: effect.map(|(e, _)| e),
                char_offset: start,
                // `end` is unknown until the tag closes; backfilled below.
                effect_range: (effect.map(|(_, s)| s).unwrap_or(0), 0),
                meta,
                bgcolor,
                underline,
                strike,
                bold,
                italic,
                // Backfilled by `apply_line_indents` once the whole string is
                // parsed — `flush` already takes twelve positional arguments and
                // has fourteen call sites, so threading a thirteenth through all
                // of them to carry a value that is constant per line would be a
                // lot of edits for no extra fidelity.
                indent: 0,
                line,
                align,
            });
        }
    };
    // `[url=a][url=b]…[/url]…[/url]` nests like the colour stack does.
    let mut meta_stack: Vec<String> = Vec::new();
    // `[indent]` nesting depth, plus a log of every change as `(line, depth)`.
    // The log exists because the depth is resolved per LINE after parsing; see
    // `apply_line_indents`.
    let mut indent_depth: usize = 0;
    let mut indent_log: Vec<(usize, usize)> = vec![(0, 0)];
    // Depth counters rather than booleans, so `[u]a[u]b[/u]c[/u]` keeps `c`
    // underlined instead of the inner closer clearing the outer tag.
    // `[bgcolor=…]` nests like the colour stack; `[fgcolor=…]` pushes onto the
    // colour stack itself, since its effect IS the run colour.
    let mut bg_stack: Vec<Color> = Vec::new();
    let mut u_depth: usize = 0;
    let mut s_depth: usize = 0;
    let mut b_depth: usize = 0;
    let mut i_depth: usize = 0;
    // Line/paragraph state. `[center]` and friends push an alignment AND start a
    // new line, matching `push_paragraph`.
    let mut line: usize = 0;
    let mut align_stack: Vec<ParagraphAlign> = vec![ParagraphAlign::Left];
    // Whether the current line has emitted anything yet. A paragraph tag at the
    // very start must not leave a blank line above the text.
    let mut line_started = false;

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
                flush(
                    &mut cur,
                    cur_color,
                    cur_effect,
                    span_start,
                    meta_stack.last().cloned(),
                    bg_stack.last().copied(),
                    u_depth > 0,
                    s_depth > 0,
                    b_depth > 0,
                    i_depth > 0,
                    line,
                    *align_stack.last().unwrap(),
                    &mut spans,
                );
                span_start = char_index;
                color_stack.push(parse_bbcode_color(spec).unwrap_or(cur_color));
            } else if let Some(spec) = lower.strip_prefix("bgcolor=") {
                // `[bgcolor=…]` fills BEHIND the run (rich_text_label.cpp:6495).
                // Unlike `[color]` it does not touch the glyph colour, so it
                // needs its own stack rather than riding on the colour one.
                flush(
                    &mut cur,
                    cur_color,
                    cur_effect,
                    span_start,
                    meta_stack.last().cloned(),
                    bg_stack.last().copied(),
                    u_depth > 0,
                    s_depth > 0,
                    b_depth > 0,
                    i_depth > 0,
                    line,
                    *align_stack.last().unwrap(),
                    &mut spans,
                );
                span_start = char_index;
                bg_stack.push(parse_bbcode_color(spec).unwrap_or(cur_color));
            } else if lower == "/bgcolor" {
                flush(
                    &mut cur,
                    cur_color,
                    cur_effect,
                    span_start,
                    meta_stack.last().cloned(),
                    bg_stack.last().copied(),
                    u_depth > 0,
                    s_depth > 0,
                    b_depth > 0,
                    i_depth > 0,
                    line,
                    *align_stack.last().unwrap(),
                    &mut spans,
                );
                span_start = char_index;
                bg_stack.pop();
            } else if let Some(spec) = lower.strip_prefix("fgcolor=") {
                // `[fgcolor=…]` sets the glyph colour, which is exactly what the
                // colour stack already means — so it pushes there rather than
                // duplicating the mechanism. Godot keeps them as separate items
                // (ItemFGColor vs ItemColor) but the rendered effect is the same.
                flush(
                    &mut cur,
                    cur_color,
                    cur_effect,
                    span_start,
                    meta_stack.last().cloned(),
                    bg_stack.last().copied(),
                    u_depth > 0,
                    s_depth > 0,
                    b_depth > 0,
                    i_depth > 0,
                    line,
                    *align_stack.last().unwrap(),
                    &mut spans,
                );
                span_start = char_index;
                color_stack.push(parse_bbcode_color(spec).unwrap_or(cur_color));
            } else if lower == "/fgcolor" {
                flush(
                    &mut cur,
                    cur_color,
                    cur_effect,
                    span_start,
                    meta_stack.last().cloned(),
                    bg_stack.last().copied(),
                    u_depth > 0,
                    s_depth > 0,
                    b_depth > 0,
                    i_depth > 0,
                    line,
                    *align_stack.last().unwrap(),
                    &mut spans,
                );
                span_start = char_index;
                if color_stack.len() > 1 {
                    color_stack.pop();
                }
            } else if let Some(target) = lower.strip_prefix("url=") {
                // Godot's `[url=target]` pushes a meta onto the run; clicking it
                // emits `meta_clicked(target)` (rich_text_label.cpp `push_meta`).
                // The payload keeps the ORIGINAL case — `lower` is only for
                // matching the tag name, and a url is data, not a keyword.
                let start = tag.len() - target.len();
                flush(
                    &mut cur,
                    cur_color,
                    cur_effect,
                    span_start,
                    meta_stack.last().cloned(),
                    bg_stack.last().copied(),
                    u_depth > 0,
                    s_depth > 0,
                    b_depth > 0,
                    i_depth > 0,
                    line,
                    *align_stack.last().unwrap(),
                    &mut spans,
                );
                span_start = char_index;
                meta_stack.push(tag[start..].to_string());
            } else if lower == "/url" {
                flush(
                    &mut cur,
                    cur_color,
                    cur_effect,
                    span_start,
                    meta_stack.last().cloned(),
                    bg_stack.last().copied(),
                    u_depth > 0,
                    s_depth > 0,
                    b_depth > 0,
                    i_depth > 0,
                    line,
                    *align_stack.last().unwrap(),
                    &mut spans,
                );
                span_start = char_index;
                meta_stack.pop();
            } else if lower == "/color" {
                flush(
                    &mut cur,
                    cur_color,
                    cur_effect,
                    span_start,
                    meta_stack.last().cloned(),
                    bg_stack.last().copied(),
                    u_depth > 0,
                    s_depth > 0,
                    b_depth > 0,
                    i_depth > 0,
                    line,
                    *align_stack.last().unwrap(),
                    &mut spans,
                );
                span_start = char_index;
                if color_stack.len() > 1 {
                    color_stack.pop();
                }
            } else if matches!(
                lower.as_str(),
                "center"
                    | "right"
                    | "left"
                    | "fill"
                    | "/center"
                    | "/right"
                    | "/left"
                    | "/fill"
                    | "p"
                    | "/p"
            ) {
                // Paragraph alignment. Each of these calls `push_paragraph` in
                // Godot (rich_text_label.cpp:5675-5688), and a paragraph is a
                // LINE — `a[center]mid[/center]b` is three paragraphs, not one
                // line with a centred middle. Treating them as inline attributes
                // would centre the whole line the moment one word was tagged.
                flush(
                    &mut cur,
                    cur_color,
                    cur_effect,
                    span_start,
                    meta_stack.last().cloned(),
                    bg_stack.last().copied(),
                    u_depth > 0,
                    s_depth > 0,
                    b_depth > 0,
                    i_depth > 0,
                    line,
                    *align_stack.last().unwrap(),
                    &mut spans,
                );
                span_start = char_index;
                // A paragraph tag at the very start must not leave a blank line
                // above the text, so the break only counts once the current line
                // has something on it.
                if line_started {
                    line += 1;
                    line_started = false;
                }
                match lower.as_str() {
                    "center" => align_stack.push(ParagraphAlign::Center),
                    "right" => align_stack.push(ParagraphAlign::Right),
                    "fill" => align_stack.push(ParagraphAlign::Fill),
                    "left" | "p" => align_stack.push(ParagraphAlign::Left),
                    // A closer restores the enclosing paragraph's alignment.
                    _ => {
                        if align_stack.len() > 1 {
                            align_stack.pop();
                        }
                    }
                }
            } else if lower == "lb" || lower == "rb" {
                // Godot's bracket escapes (rich_text_label.cpp:5621-5626):
                // `[lb]` is a literal `[`, `[rb]` a literal `]`.
                //
                // ⚠️ These emit a CHARACTER, so unlike every other tag they must
                // NOT flush the run. Flushing would split a run mid-word and, on
                // an effected run, restart the glyph phase at the escape.
                cur.push(if lower == "lb" { '[' } else { ']' });
                char_index += 1;
            } else if matches!(
                lower.as_str(),
                "u" | "s" | "/u" | "/s" | "b" | "i" | "/b" | "/i"
            ) {
                // `[u]`/`[s]` are RULES, not glyph shapes, so they are drawable
                // exactly. `[b]`/`[i]` used to be STRIPPED here, on the grounds
                // that the built-in 5x7 bitmap font has no weight or slant
                // variant (pat-s3rrv.74).
                //
                // ⚠️ That conflated two things. The font cannot *shape* a bold
                // glyph, but the run still needs to CARRY the attribute: it is
                // what the draw path emboldens from, and what any consumer with
                // a real font would use. Dropping the tag at parse time threw
                // the information away at the earliest possible point.
                //
                // ⚠️ Matched as whole tags: `[s]` must not be confused with
                // `[shake]`, and `[u]` must not swallow `[url=…]`, which is
                // handled above. A `strip_prefix` here would do both.
                flush(
                    &mut cur,
                    cur_color,
                    cur_effect,
                    span_start,
                    meta_stack.last().cloned(),
                    bg_stack.last().copied(),
                    u_depth > 0,
                    s_depth > 0,
                    b_depth > 0,
                    i_depth > 0,
                    line,
                    *align_stack.last().unwrap(),
                    &mut spans,
                );
                span_start = char_index;
                match lower.as_str() {
                    "u" => u_depth += 1,
                    "s" => s_depth += 1,
                    "b" => b_depth += 1,
                    "i" => i_depth += 1,
                    // Saturating: a stray `[/u]` with nothing open is stripped,
                    // not an underflow panic.
                    "/u" => u_depth = u_depth.saturating_sub(1),
                    "/b" => b_depth = b_depth.saturating_sub(1),
                    "/i" => i_depth = i_depth.saturating_sub(1),
                    _ => s_depth = s_depth.saturating_sub(1),
                }
            } else if lower == "indent" || lower == "/indent" {
                // Unlike `[u]`/`[b]` this does NOT flush the run. Those are run
                // attributes and need a boundary at the tag; an indent is a
                // property of the line, so splitting the run here would gain
                // nothing and would restart an effect's glyph phase mid-word.
                if lower == "indent" {
                    indent_depth += 1;
                } else {
                    // Saturating, so a stray `[/indent]` is stripped rather than
                    // underflowing.
                    indent_depth = indent_depth.saturating_sub(1);
                }
                indent_log.push((line, indent_depth));
            } else if let Some(name) = lower.strip_prefix('/') {
                // A closer: pop only if it matches the open effect.
                if effect_stack.last().map(|(n, _, _)| n.as_str()) == Some(name) {
                    flush(
                        &mut cur,
                        cur_color,
                        cur_effect,
                        span_start,
                        meta_stack.last().cloned(),
                        bg_stack.last().copied(),
                        u_depth > 0,
                        s_depth > 0,
                        b_depth > 0,
                        i_depth > 0,
                        line,
                        *align_stack.last().unwrap(),
                        &mut spans,
                    );
                    span_start = char_index;
                    effect_stack.pop();
                }
                // Otherwise it is `[/b]` and friends: strip, no state change.
            } else {
                // An opener: `wave`, or `wave amp=20 freq=3`.
                let (name, params) = parse_tag_params(&lower);
                if let Some(effect) = BuiltinEffect::parse(name, &params) {
                    flush(
                        &mut cur,
                        cur_color,
                        cur_effect,
                        span_start,
                        meta_stack.last().cloned(),
                        bg_stack.last().copied(),
                        u_depth > 0,
                        s_depth > 0,
                        b_depth > 0,
                        i_depth > 0,
                        line,
                        *align_stack.last().unwrap(),
                        &mut spans,
                    );
                    span_start = char_index;
                    effect_stack.push((name.to_string(), effect, char_index));
                } else if let Some(id) = crate::char_fx::match_custom_effect(name, custom) {
                    // A script-defined effect from `custom_effects`
                    // (pat-fk17e.2). Identical bookkeeping to a built-in —
                    // the span machinery does not care which kind it is, only
                    // the draw path does — so this opens a span and pushes the
                    // same stack entry, carrying the effect's object id.
                    flush(
                        &mut cur,
                        cur_color,
                        cur_effect,
                        span_start,
                        meta_stack.last().cloned(),
                        bg_stack.last().copied(),
                        u_depth > 0,
                        s_depth > 0,
                        b_depth > 0,
                        i_depth > 0,
                        line,
                        *align_stack.last().unwrap(),
                        &mut spans,
                    );
                    span_start = char_index;
                    effect_stack.push((
                        name.to_string(),
                        BuiltinEffect::Custom { effect: id },
                        char_index,
                    ));
                }
                // b/i/u/s and unknown tags: strip, no change. A script-defined
                // tag whose effect is NOT installed in `custom_effects` lands
                // here too and degrades to plain text, which is Godot's
                // behaviour for an unregistered tag.
            }
        } else if c == '\n' {
            // A literal newline breaks the line. Before this the arm advanced x
            // and never y, so a multi-line label drew every line on top of the
            // first — `\n` reached the bitmap font as an unrenderable glyph.
            //
            // ⚠️ It is NOT pushed into the run: it is layout, not a character,
            // and `get_parsed_text` keeps it only because Godot's newline items
            // report as "\n" there.
            //
            // Re-read here rather than reusing the tag branch's bindings: those
            // are declared inside `if c == '['`, which is a SIBLING scope.
            let cur_color = *color_stack.last().unwrap();
            let cur_effect = effect_stack.last().map(|(_, e, start)| (*e, *start));
            flush(
                &mut cur,
                cur_color,
                cur_effect,
                span_start,
                meta_stack.last().cloned(),
                bg_stack.last().copied(),
                u_depth > 0,
                s_depth > 0,
                b_depth > 0,
                i_depth > 0,
                line,
                *align_stack.last().unwrap(),
                &mut spans,
            );
            line += 1;
            line_started = false;
            char_index += 1;
            span_start = char_index;
        } else {
            cur.push(c);
            char_index += 1;
            line_started = true;
        }
    }
    let cur_color = *color_stack.last().unwrap();
    let cur_effect = effect_stack.last().map(|(_, e, start)| (*e, *start));
    flush(
        &mut cur,
        cur_color,
        cur_effect,
        span_start,
        meta_stack.last().cloned(),
        bg_stack.last().copied(),
        u_depth > 0,
        s_depth > 0,
        b_depth > 0,
        i_depth > 0,
        line,
        *align_stack.last().unwrap(),
        &mut spans,
    );
    apply_line_indents(&mut spans, &indent_log);
    backfill_effect_ends(&mut spans);
    spans
}

/// Resolves each run's `[indent]` depth from the parse-time change log.
///
/// `log` holds `(line, depth)` for every `[indent]`/`[/indent]`, in order. A
/// line's depth is the depth carried in from earlier lines, raised to the
/// deepest level any tag reached *on* that line.
///
/// ⚠️ **Taking the last change at or before the line does not work**, which is
/// worth stating because it is the obvious reading of a change log. The ordinary
/// spelling `[indent]text[/indent]` opens and closes on the SAME line, so the
/// closer is the last entry for that line and the line would resolve to zero —
/// the tag would silently do nothing in exactly the case it is written for.
///
/// ⚠️ A depth change within a line therefore applies to the whole line,
/// including text before the tag. That is a limitation of the model rather than
/// an oversight: the depth becomes the line's left inset, and a line has one
/// left edge. Godot's `[indent]` is a paragraph-level construct, so the common
/// spelling is exact.
fn apply_line_indents(spans: &mut [TextSpan], log: &[(usize, usize)]) {
    for span in spans.iter_mut() {
        // Depth carried in from lines strictly before this one.
        let mut carried = 0;
        // Deepest level any tag on THIS line reached.
        let mut on_line: Option<usize> = None;
        for (line, depth) in log {
            if *line < span.line {
                carried = *depth;
            } else if *line == span.line {
                on_line = Some(on_line.unwrap_or(0).max(*depth));
            } else {
                break;
            }
        }
        span.indent = on_line.map_or(carried, |d| d.max(carried));
    }
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
    font_size: i64,
) -> Vec<DrawCommand> {
    // pat-s3rrv.37: takes the Godot PIXEL font size and derives the placeholder
    // 5x7 scale internally, rather than receiving the already-quantised scale.
    //
    // Behaviour-preserving on its own — `scale` below is exactly what the caller
    // used to pass. The point is that the true font size is now in scope here,
    // which is the prerequisite for this function emitting a pixel `font_size`
    // on its ControlDrawItem::Text (the family-(c) change). Without it, the emit
    // site at the end of this function had no access to the real size at all.
    let scale = text_scale(font_size);
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

    let space = space_advance(scale);
    let max_width = rect.size.x;
    let line_height = (GLYPH_H + GLYPH_GAP) * scale as f32;

    let mut out = Vec::new();
    // The row this paragraph starts on, so paragraphs stack instead of
    // overwriting each other at y = 0.
    let mut row_base = 0usize;

    // 🔑 Godot shapes MANDATORY line breaks FIRST and wraps each paragraph
    // independently (`Label::_shape`, `scene/gui/label.cpp`). This used to be
    // `text.split_whitespace()` over the whole string, which treats '\n' as an
    // ordinary space — so a Label with an explicit newline drew as ONE line, and
    // with autowrap off every word landed on a single line regardless of how
    // many the author typed (pat-s3rrv.129).
    for paragraph in text.split('\n') {
        let words: Vec<&str> = paragraph.split_whitespace().collect();
        if words.is_empty() {
            // ⚠️ A blank line still OCCUPIES a row: "A\n\nB" puts B two rows
            // down, not one. Skipping it would silently close the gap the
            // author typed, which is the same class of bug as dropping the
            // newline entirely.
            row_base += 1;
            continue;
        }
        let widths: Vec<f32> = words.iter().map(|w| text_width(w, scale)).collect();

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

        let line_count = lines.len();
        for (row_in_paragraph, line) in lines.iter().enumerate() {
            let row = row_base + row_in_paragraph;
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
                    line_width =
                        widths_slice[..keep].iter().sum::<f32>() + space * keep as f32 + ell_w;
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
                font_size: font_size.max(1) as u32,
            });
        }
        // ⚠️ Advance by the paragraph's LINE COUNT, not by one: a wrapped
        // paragraph occupies several rows and the next paragraph must clear all
        // of them.
        row_base += line_count;
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

/// Default `RichTextLabel` font size — what the draw arm falls back to when the
/// theme has no entry.
pub const RICHTEXT_DEFAULT_FONT_SIZE: i64 = 16;

/// The BBCode source of a `RichTextLabel`: `bbcode_text` when set, else `text`.
///
/// Shared so the hit-test and the draw pass can never read different strings.
/// Runs this label's script-defined effects and caches the per-glyph result
/// (pat-fk17e.2).
///
/// The pre-pass half of the custom-effect feature. Running GDScript needs
/// `&mut SceneTree` and the draw path holds the tree immutably by design, so
/// the result is computed here and read back from the node during the draw —
/// the same shape as [`SceneTree::flush_scripted_resources`] and
/// [`SceneTree::flush_pending_redraws`]. See [`crate::char_fx`] for why a
/// `draw_control` signature change is not the answer.
///
/// Returns immediately for a label with no `custom_effects`, which is every
/// label in almost every scene.
fn flush_custom_char_fx(tree: &mut SceneTree, node_id: NodeId) {
    let effects = richtext_custom_effects(tree, node_id);
    if effects.is_empty() {
        return;
    }
    let raw = richtext_source(tree, node_id);
    if raw.is_empty() {
        return;
    }
    // Same inputs the draw path will use, so the cache lines up glyph for
    // glyph. `fx_elapsed_time` is the clock the built-ins read too.
    let elapsed = float_prop(tree, node_id, "fx_elapsed_time").unwrap_or(0.0) as f32;
    let spans = parse_bbcode_with_effects(&raw, Color::WHITE, &effects);
    let total: usize = spans.iter().map(|s| s.text.chars().count()).sum();
    if total == 0 {
        return;
    }

    // ⚠️ One phantom node for the whole pass, created here and removed before
    // returning. This is what `char_fx` means by carrying a `CharFXTransform`
    // as a node: the script mutates `char_fx.offset`, that write lands on this
    // node through the ordinary property path, and `read_back` collects it.
    // Creating one per glyph — or leaving this one behind — would add a node to
    // the tree on every frame of every label that has an effect.
    let root = tree.root_id();
    let Ok(charfx) = tree.add_child(
        root,
        crate::node::Node::new("CharFXTransform", "CharFXTransform"),
    ) else {
        return;
    };
    let charfx_arg = Variant::ObjectId(charfx.object_id());

    // Dense and indexed by absolute glyph index, matching `CharFXCache::get`.
    // Every glyph gets its span's own color, so even a read that should not
    // happen cannot repaint a glyph.
    let mut results: Vec<CharFXTransform> = (0..total)
        .map(|i| CharFXTransform::new(i, (0, total), elapsed, Color::WHITE))
        .collect();

    for span in &spans {
        for (i, _) in span.text.chars().enumerate() {
            let abs = span.char_offset + i;
            if abs >= results.len() {
                continue;
            }
            let mut fx = CharFXTransform::new(abs, span.effect_range, elapsed, span.color);
            if let Some(BuiltinEffect::Custom { effect }) = span.effect {
                let effect_node = NodeId::from_object_id(gdcore::id::ObjectId::from_raw(effect));
                for (key, value) in fx.to_properties() {
                    if let Some(node) = tree.get_node_mut(charfx) {
                        node.set_property(&key, value);
                    }
                }
                // Takes the script out of the arena for the call, which is what
                // lets the handler mutate the tree (including `charfx`) while we
                // hold `&mut tree`; see `SceneTree::call_script_method`.
                let returned = tree.call_script_method(
                    effect_node,
                    "_process_custom_fx",
                    &[charfx_arg.clone()],
                );
                fx.read_back(|key| {
                    tree.get_node(charfx)
                        .map(|node| node.get_property(key))
                        .unwrap_or(Variant::Nil)
                });
                // ⚠️ Godot hides the glyph when the handler RETURNS false, which
                // is separate from it clearing `char_fx.visible`. Honouring only
                // the property would implement half of this and look correct,
                // because a handler that does both is the common case.
                if matches!(returned, Some(Variant::Bool(false))) {
                    fx.visible = false;
                }
            }
            results[abs] = fx;
        }
    }

    let _ = tree.remove_node(charfx);
    for (key, value) in crate::char_fx::cache_to_properties(&results) {
        if let Some(node) = tree.get_node_mut(node_id) {
            node.set_property(&key, value);
        }
    }
}

/// The label's installed script-defined effects, as `(object id, bbcode tag)`
/// in `custom_effects` order (pat-fk17e.2).
///
/// An entry that is not an object, or whose effect declares no `bbcode`, is
/// skipped rather than defaulted: an effect with no tag can never be named by
/// any BBCode, so giving it `""` would let `[]` reach it.
fn richtext_custom_effects(tree: &SceneTree, node_id: NodeId) -> Vec<(u64, String)> {
    let Some(node) = tree.get_node(node_id) else {
        return Vec::new();
    };
    let Variant::Array(items) = node.get_property("custom_effects") else {
        return Vec::new();
    };
    items
        .to_vec()
        .into_iter()
        .filter_map(|entry| {
            let Variant::ObjectId(oid) = entry else {
                return None;
            };
            let effect_node = NodeId::from_object_id(oid);
            // ⚠️ `bbcode` is declared by the effect's SCRIPT (`var bbcode =
            // "mytag"`), and `Node::get_property` reads only the node's own
            // property bag — it does not consult the attached script. Reading
            // it as a node property finds nothing, the effect is skipped, and
            // the whole feature silently no-ops. The script is asked first for
            // that reason; the node is still a fallback, because a `.tres`
            // -loaded effect can carry `bbcode` as a stored property instead.
            let bbcode = tree
                .get_script(effect_node)
                .and_then(|script| script.get_property("bbcode"))
                .and_then(|value| match value {
                    Variant::String(tag) => Some(tag),
                    _ => None,
                })
                .or_else(|| string_prop(tree, effect_node, "bbcode"))
                .filter(|tag| !tag.is_empty())?;
            Some((oid.raw(), bbcode))
        })
        .collect()
}

fn richtext_source(tree: &SceneTree, node_id: NodeId) -> String {
    string_prop(tree, node_id, "bbcode_text")
        .filter(|s| !s.is_empty())
        .or_else(|| string_prop(tree, node_id, "text"))
        .unwrap_or_default()
}

/// The natural size of a `RichTextLabel`'s content: the widest line by the
/// number of lines (pat-s3rrv.74).
///
/// Measured from the same `parse_bbcode` + `text_width` + `GLYPH_GAP` the layout
/// advances by, so "how big is this" and "where does it go" cannot disagree.
/// Alignment is irrelevant here — it moves runs within a width, it does not
/// change the width — so this needs no rect and can be asked before one exists.
pub fn richtext_content_size(tree: &SceneTree, node_id: NodeId, font_size: i64) -> Vector2 {
    let raw = richtext_source(tree, node_id);
    if raw.is_empty() {
        return Vector2::ZERO;
    }
    let scale = text_scale(font_size);
    let gap = GLYPH_GAP * scale as f32;
    let spans = parse_bbcode(&raw, Color::WHITE);
    let last_line = spans.iter().map(|s| s.line).max().unwrap_or(0);
    let mut widest = 0.0f32;
    for line in 0..=last_line {
        let on_line: Vec<&TextSpan> = spans
            .iter()
            .filter(|s| s.line == line && !s.text.is_empty())
            .collect();
        if on_line.is_empty() {
            continue;
        }
        let w: f32 = on_line
            .iter()
            .map(|s| text_width(&s.text, scale))
            .sum::<f32>()
            + gap * (on_line.len() as f32 - 1.0);
        widest = widest.max(w);
    }
    let line_h = (GLYPH_H + GLYPH_GAP) * scale as f32;
    Vector2::new(widest, (last_line + 1) as f32 * line_h)
}

/// Applies `fit_content`: when set, the label's `min_size` follows its content
/// so the layout solver grows it (pat-s3rrv.74).
///
/// ⚠️ `fit_content` was registered and **unread** — the inspector showed it, the
/// value round-tripped through `.tscn`, and nothing consulted it. Godot: "the
/// label's minimum size will be automatically updated to fit its content"
/// (RichTextLabel.xml:740). Default is `false`, so a label that does not opt in
/// is untouched.
///
/// Runs before `resolve_control_layout`, which reads `min_size` as `inner_min`
/// (control.rs:1342) — after it, the size would be a frame stale.
pub fn apply_richtext_fit_content(tree: &mut SceneTree, node_id: NodeId, font_size: i64) {
    let fits = tree
        .get_node(node_id)
        .map(|n| matches!(n.get_property("fit_content"), Variant::Bool(true)))
        .unwrap_or(false);
    if !fits {
        return;
    }
    let size = richtext_content_size(tree, node_id, font_size);
    control::set_min_size(tree, node_id, size);
}

/// The label's text with every tag removed — Godot's `get_parsed_text`
/// (rich_text_label.cpp:7183, which concatenates the text items).
///
/// Derived from `parse_bbcode` rather than a second stripping pass, so what a
/// script reads back is exactly what the draw arm lays out.
pub fn richtext_parsed_text(tree: &SceneTree, node_id: NodeId) -> String {
    parse_bbcode(&richtext_source(tree, node_id), Color::WHITE)
        .into_iter()
        .map(|s| s.text)
        .collect()
}

/// One laid-out run: where it goes, and what it is.
pub(crate) struct PlacedSpan {
    pub rect: Rect2,
    span: TextSpan,
}

/// The full layout of a `RichTextLabel`: parse, apply the visible-character
/// budget, break into lines, and place every run (pat-s3rrv.74).
///
/// ⚠️ This is the SINGLE layout pipeline. The draw arm and the `[url]` hit
/// regions both consume it, so a link is clickable exactly where it is painted.
/// They previously each advanced `x` themselves, which agreed only by accident
/// and would have diverged the moment lines existed — a link on line 2 would
/// have been clickable on line 1.
/// How far right a `[b]` run is over-struck to fake weight (pat-s3rrv.74).
///
/// One pixel: enough to thicken a 5x7 stem, small enough that the doubled run
/// still reads as the same glyphs rather than a shadow.
const BOLD_OVERSTRIKE: f32 = 1.0;

fn richtext_placements(
    tree: &SceneTree,
    node_id: NodeId,
    font_size: i64,
    rect: Rect2,
    default_color: Color,
) -> Vec<PlacedSpan> {
    let raw = richtext_source(tree, node_id);
    if raw.is_empty() {
        return Vec::new();
    }
    let scale = text_scale(font_size);
    // The layout path is the only one with a node to read `custom_effects`
    // from, so it is the only one that can resolve a script-defined tag
    // (pat-fk17e.2). An empty list makes this identical to `parse_bbcode`.
    let mut spans =
        parse_bbcode_with_effects(&raw, default_color, &richtext_custom_effects(tree, node_id));

    // The visible-character budget is applied BEFORE placement so the widths a
    // line is centred by are the widths actually drawn — trimming afterwards
    // would centre against text that is not there.
    let total: usize = spans.iter().map(|s| s.text.chars().count()).sum();
    let limit = match int_prop(tree, node_id, "visible_characters") {
        Some(n) if n >= 0 => Some(n as usize),
        _ => float_prop(tree, node_id, "visible_ratio")
            .filter(|r| *r >= 0.0 && *r < 1.0)
            .map(|r| (r * total as f64).round() as usize),
    };
    if let Some(limit) = limit {
        let mut emitted = 0usize;
        let mut kept: Vec<TextSpan> = Vec::new();
        for mut span in spans {
            if emitted >= limit {
                break;
            }
            let room = limit - emitted;
            if span.text.chars().count() > room {
                // Truncated, not dropped: Godot reveals glyph by glyph, and
                // dropping the run would reveal a word at a time.
                span.text = span.text.chars().take(room).collect();
            }
            emitted += span.text.chars().count();
            kept.push(span);
        }
        spans = kept;
    }

    let gap = GLYPH_GAP * scale as f32;
    let line_h = (GLYPH_H + GLYPH_GAP) * scale as f32;
    // A space costs its own glyph plus the inter-glyph gap on EACH side, which
    // is what makes a word-split line measure exactly the same as the
    // unsplit run it came from: "go to" == "go" + space + "to" == 58.
    let space_w = text_width(" ", scale) + 2.0 * gap;
    // One `[indent]` level. Godot indents by a tab stop; four spaces at the
    // current scale is this renderer's equivalent, and it scales with the font
    // rather than being a fixed pixel count.
    let indent_w = text_width("    ", scale);
    // Godot: AUTOWRAP_OFF 0, ARBITRARY 1, WORD 2, WORD_SMART 3. The ClassDB
    // default for RichTextLabel is 3, and class defaults are not written onto
    // nodes, so an authored label with no property still wraps.
    //
    // ⚠️ `text_layout` implements WORD breaking only, so ARBITRARY (break
    // mid-word) is treated as WORD — the same narrowing the Label path already
    // documents rather than silently approximating.
    let autowrap = int_prop(tree, node_id, "autowrap_mode").unwrap_or(3);

    let mut out = Vec::new();
    // ⚠️ Visual lines are counted across the WHOLE label, not per paragraph. A
    // logical line that wraps into two pushes everything after it down — using
    // the logical index plus the wrap index would put paragraph 2 on top of
    // paragraph 1's second visual line.
    let mut visual_line = 0usize;
    let last_line = spans.iter().map(|s| s.line).max().unwrap_or(0);
    for line in 0..=last_line {
        let on_line: Vec<&TextSpan> = spans
            .iter()
            .filter(|s| s.line == line && !s.text.is_empty())
            .collect();
        if on_line.is_empty() {
            continue;
        }

        // Split the line into words. A word accumulates pieces ACROSS run
        // boundaries — `[color=red]he[/color]llo` is one word, and breaking it
        // at the colour change would hyphenate a word nothing asked to break.
        // Each piece remembers its run and its offset within it, so effects keep
        // phasing off the right glyph index after a wrap.
        let mut words: Vec<Vec<(usize, String, usize)>> = Vec::new();
        let mut cur: Vec<(usize, String, usize)> = Vec::new();
        for (si, span) in on_line.iter().enumerate() {
            let mut piece = String::new();
            let mut piece_start = 0usize;
            for (ci, ch) in span.text.chars().enumerate() {
                if ch == ' ' {
                    if !piece.is_empty() {
                        cur.push((si, std::mem::take(&mut piece), piece_start));
                    }
                    if !cur.is_empty() {
                        words.push(std::mem::take(&mut cur));
                    }
                } else {
                    if piece.is_empty() {
                        piece_start = ci;
                    }
                    piece.push(ch);
                }
            }
            if !piece.is_empty() {
                cur.push((si, piece, piece_start));
            }
        }
        if !cur.is_empty() {
            words.push(cur);
        }
        if words.is_empty() {
            continue;
        }

        // A word's width includes the gaps between its own pieces, so it equals
        // the width of the same characters in one run.
        let widths: Vec<f32> = words
            .iter()
            .map(|w| {
                w.iter().map(|(_, t, _)| text_width(t, scale)).sum::<f32>()
                    + gap * (w.len() as f32 - 1.0)
            })
            .collect();

        // The line's alignment, needed by both the fast path and the wrap path.
        let align = on_line[0].align;
        // `[indent]` moves the line's left edge in and takes that width away
        // from what it can wrap against — both halves matter: insetting without
        // narrowing would push indented text off the right edge instead of
        // wrapping it sooner. Constant per line, like `align`.
        let inset = on_line[0].indent as f32 * indent_w;
        let avail = (rect.size.x - inset).max(1.0);
        // ⚠️ A line that FITS is placed run-by-run, exactly as before wrapping
        // existed. The word path drops spaces (they become advance, not draw
        // items), which is right at a wrap point and wrong everywhere else: it
        // turned "[b]bold[/b] [i]it[/i]" into "boldit". Only an overflowing line
        // pays that cost, and there the dropped space IS the line break.
        let run_width: f32 = on_line
            .iter()
            .map(|s| text_width(&s.text, scale))
            .sum::<f32>()
            + gap * (on_line.len() as f32 - 1.0);
        if autowrap == 0 || run_width <= avail {
            let x0 = rect.position.x
                + inset
                + match align {
                    ParagraphAlign::Left | ParagraphAlign::Fill => 0.0,
                    ParagraphAlign::Center => ((avail - run_width) * 0.5).max(0.0),
                    ParagraphAlign::Right => (avail - run_width).max(0.0),
                };
            let y = rect.position.y + visual_line as f32 * line_h;
            visual_line += 1;
            let mut x = x0;
            for span in &on_line {
                let w = text_width(&span.text, scale);
                out.push(PlacedSpan {
                    rect: Rect2::new(Vector2::new(x, y), Vector2::new(w, GLYPH_H * scale as f32)),
                    span: (*span).clone(),
                });
                x += w + gap;
            }
            continue;
        }

        let ranges = if autowrap == 0 {
            // OFF: one visual line, however wide — the previous behaviour.
            vec![gdrender2d::text_layout::LineRange {
                start: 0,
                end: words.len(),
                width: widths.iter().sum::<f32>() + space_w * (words.len() as f32 - 1.0),
            }]
        } else {
            gdrender2d::text_layout::break_lines_word(&widths, space_w, avail)
        };

        for range in ranges.iter() {
            let mut x = rect.position.x
                + inset
                + match align {
                    // `Fill` renders left-aligned: justification needs a word
                    // model with elastic spacing, which this does not have.
                    ParagraphAlign::Left | ParagraphAlign::Fill => 0.0,
                    ParagraphAlign::Center => ((avail - range.width) * 0.5).max(0.0),
                    ParagraphAlign::Right => (avail - range.width).max(0.0),
                };
            let y = rect.position.y + visual_line as f32 * line_h;
            visual_line += 1;
            for wi in range.start..range.end {
                if wi > range.start {
                    x += space_w;
                }
                for (pi, (si, text, off)) in words[wi].iter().enumerate() {
                    if pi > 0 {
                        x += gap;
                    }
                    let w = text_width(text, scale);
                    let mut span = on_line[*si].clone();
                    // The fragment keeps its own glyph index so `[wave]` and
                    // friends phase from the tag, not from the wrap point.
                    span.char_offset = on_line[*si].char_offset + off;
                    span.text = text.clone();
                    out.push(PlacedSpan {
                        rect: Rect2::new(
                            Vector2::new(x, y),
                            Vector2::new(w, GLYPH_H * scale as f32),
                        ),
                        span,
                    });
                    x += w;
                }
            }
        }
    }
    out
}

/// The clickable `[url=…]` regions of a `RichTextLabel`, in global space
/// (pat-s3rrv.74).
///
/// ⚠️ Reads [`richtext_placements`], the same layout the draw arm paints from,
/// so a link is clickable exactly where it appears — on the right LINE as well
/// as the right column. Re-deriving the geometry here is how a link ends up
/// clickable a word to the left, or on the wrong row of a multi-line label.
///
/// Runs hidden by `visible_characters` are absent from the layout and therefore
/// not clickable, which is the behaviour you want: text that has not been
/// revealed yet is not a link.
///
/// ⚠️ `font_size` is a parameter rather than read from a theme because the input
/// stage has no theme to consult — `SceneTree` does not carry one, the draw call
/// receives it. Callers outside the draw pass pass
/// [`RICHTEXT_DEFAULT_FONT_SIZE`], which agrees with the draw arm for any label
/// that does not override the theme font size.
pub fn richtext_meta_regions(
    tree: &SceneTree,
    node_id: NodeId,
    font_size: i64,
    rect: Rect2,
) -> Vec<(Rect2, String)> {
    richtext_placements(tree, node_id, font_size, rect, Color::WHITE)
        .into_iter()
        .filter_map(|p| p.span.meta.clone().map(|m| (p.rect, m)))
        .collect()
}

fn draw_control(
    tree: &SceneTree,
    node_id: NodeId,
    theme: &Theme,
    rect: Rect2,
    out: &mut Vec<DrawCommand>,
) {
    match class_of(tree, node_id).as_str() {
        // pat-s3rrv.233: the GraphEdit arm draws the grid, every child GraphNode
        // and every connection — see `draw_graph_edit` for why the children are
        // drawn from here rather than by an arm of their own.
        "GraphEdit" => draw_graph_edit(tree, node_id, rect, out),
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
            // A SCRIPTED StyleBox paints the whole background itself
            // (pat-s3rrv.160): Godot calls its `_draw` instead of drawing the
            // default box, so a script emitting a circle gets a circle and no
            // rectangle underneath it. The commands were produced by
            // `SceneTree::flush_scripted_resources` — running a script needs
            // `&mut`, and this path is immutable by design.
            let scripted = tree.scripted_resource_commands(node_id);
            if !scripted.is_empty() {
                out.extend(scripted.iter().cloned());
            }
            // Prefer a typed StyleBox (pat-u3l8c.70); fall back to the legacy
            // Variant::Color background.
            else if let Some(sb) = theme.get_stylebox("Panel", "panel") {
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
                    font_size: font_size.max(1) as u32,
                });
            }
        }
        // pat-s3rrv.166: LinkButton is Godot's hyperlink-styled button. It was
        // in `CONTROL_HIT_CLASSES` and `BUTTON_CLASSES` but not in
        // `CONTROL_CLASSES`, so it laid out, took clicks and took focus while
        // painting nothing — an invisible rectangle you could tab to and press.
        "LinkButton" => {
            let text = string_prop(tree, node_id, "text").unwrap_or_default();
            if !text.is_empty() {
                let font_color = color_prop(tree, node_id, "theme_override_colors/font_color")
                    .or_else(|| theme.get_color("LinkButton", "font_color"))
                    // Godot's default LinkButton font colour is the same
                    // near-white as Button; the underline, not a colour, is what
                    // marks it as a link.
                    .unwrap_or(Color::new(0.875, 0.875, 0.875, 1.0));
                let font_size = theme.get_font_size("LinkButton", "font_size").unwrap_or(16);
                let scale = text_scale(font_size);
                let tw = text_width(&text, scale);
                let glyph_h = GLYPH_H * scale as f32;
                // ⚠️ Drawn at the rect's top-left, NOT centred like `"Button"`.
                // A LinkButton has no stylebox and its minimum size is the text
                // itself (`link_button.cpp::get_minimum_size`), so there is no
                // box to centre within — centring would shift the text off its
                // own underline whenever the node was stretched by a container.
                let position = rect.position;
                out.push(DrawCommand::DrawString {
                    text,
                    position,
                    color: font_color,
                    font_size: font_size.max(1) as u32,
                });

                // `UnderlineMode`: ALWAYS = 0, ON_HOVER = 1, NEVER = 2
                // (`upstream/godot/scene/gui/link_button.h:40`). ClassDB
                // registers the default as `Int(0)`, i.e. ALWAYS, so a
                // LinkButton that never sets the property is underlined.
                const UNDERLINE_ALWAYS: i64 = 0;
                const UNDERLINE_ON_HOVER: i64 = 1;
                let mode = int_prop(tree, node_id, "underline").unwrap_or(UNDERLINE_ALWAYS);
                // 🔑 `control_draw_items` takes no hover argument and MainLoop's
                // `hovered_pickables` tracks pickables rather than Controls, so
                // `SceneTree::hovered_control()` is the only thing that makes
                // ON_HOVER expressible in the draw pass at all.
                let hovered = tree.hovered_control() == Some(node_id);
                if mode == UNDERLINE_ALWAYS || (mode == UNDERLINE_ON_HOVER && hovered) {
                    // Same geometry the RichTextLabel underline uses, so the two
                    // rules sit at the same offset for identical text.
                    let thickness = scale as f32;
                    let uy = position.y + glyph_h + thickness;
                    out.push(DrawCommand::DrawLine {
                        from: Vector2::new(position.x, uy),
                        // Spans the TEXT width, not the control rect: a
                        // container-stretched LinkButton must not trail a rule
                        // past the end of its own label.
                        to: Vector2::new(position.x + tw, uy),
                        color: font_color,
                        width: thickness,
                    });
                }
            }
        }
        // pat-s3rrv.172: ColorPicker. Registered and loadable from a `.tscn` all
        // along, and `color_picker.rs` held a faithful `Color::set_hsv` port plus
        // the hit mapping — with **zero non-test callers**. Nothing drew it, so a
        // picker in a played scene was an invisible, inert rectangle.
        "ColorPicker" => {
            let color = color_prop(tree, node_id, "color").unwrap_or(Color::WHITE);
            let edit_alpha = bool_prop(tree, node_id, "edit_alpha").unwrap_or(true);
            let model = crate::color_picker::ColorPickerModel::from_color(color);
            let layout = crate::color_picker::picker_layout(rect, edit_alpha);

            // ⚠️ The SV area is a GRADIENT in Godot; Patina's draw model has no
            // gradient primitive, so it is approximated by a grid of cells. The
            // cell colours come from `color_from_hsv` — the same function the
            // hit mapping inverts — so what is painted and what a click resolves
            // to cannot drift apart.
            const SV_CELLS: usize = 16;
            if layout.sv.size.x > 0.0 && layout.sv.size.y > 0.0 {
                let cw = layout.sv.size.x / SV_CELLS as f32;
                let ch = layout.sv.size.y / SV_CELLS as f32;
                for iy in 0..SV_CELLS {
                    for ix in 0..SV_CELLS {
                        // Sample at the cell CENTRE: sampling the corner makes
                        // the top row pure white and the left column pure grey,
                        // a visible seam against the cursor's own position.
                        let s = (ix as f32 + 0.5) / SV_CELLS as f32;
                        let v = 1.0 - (iy as f32 + 0.5) / SV_CELLS as f32;
                        out.push(DrawCommand::DrawRect {
                            rect: Rect2::new(
                                Vector2::new(
                                    layout.sv.position.x + ix as f32 * cw,
                                    layout.sv.position.y + iy as f32 * ch,
                                ),
                                Vector2::new(cw, ch),
                            ),
                            color: crate::color_picker::color_from_hsv(model.hue, s, v, 1.0),
                            filled: true,
                        });
                    }
                }
            }

            // The hue bar: full saturation and value down the spectrum.
            const HUE_CELLS: usize = 24;
            if layout.hue.size.x > 0.0 && layout.hue.size.y > 0.0 {
                let hh = layout.hue.size.y / HUE_CELLS as f32;
                for i in 0..HUE_CELLS {
                    let h = (i as f32 + 0.5) / HUE_CELLS as f32;
                    out.push(DrawCommand::DrawRect {
                        rect: Rect2::new(
                            Vector2::new(
                                layout.hue.position.x,
                                layout.hue.position.y + i as f32 * hh,
                            ),
                            Vector2::new(layout.hue.size.x, hh),
                        ),
                        color: crate::color_picker::color_from_hsv(h, 1.0, 1.0, 1.0),
                        filled: true,
                    });
                }
            }

            // The alpha bar, when `edit_alpha`. Drawn as the current colour at
            // rising opacity rather than a checkerboard — Patina has no alpha
            // compositing in this pass, so a checkerboard would be decoration
            // that says nothing about the value.
            if let Some(ab) = layout.alpha {
                const A_CELLS: usize = 16;
                if ab.size.x > 0.0 && ab.size.y > 0.0 {
                    let aw = ab.size.x / A_CELLS as f32;
                    for i in 0..A_CELLS {
                        let a = (i as f32 + 0.5) / A_CELLS as f32;
                        out.push(DrawCommand::DrawRect {
                            rect: Rect2::new(
                                Vector2::new(ab.position.x + i as f32 * aw, ab.position.y),
                                Vector2::new(aw, ab.size.y),
                            ),
                            color: crate::color_picker::color_from_hsv(
                                model.hue,
                                model.saturation,
                                model.value,
                                a,
                            ),
                            filled: true,
                        });
                    }
                }
            }

            // 🔑 The cursor marks the SELECTED colour inside the SV square, and
            // it is what makes the picker readable: without it every colour of a
            // given hue paints the same square and the control shows no state at
            // all. Outlined, not filled, so it stays visible against both a light
            // and a dark selection.
            if layout.sv.size.x > 0.0 && layout.sv.size.y > 0.0 {
                const CURSOR: f32 = 6.0;
                let cx = layout.sv.position.x + model.saturation * layout.sv.size.x;
                // `value` runs bottom→top, so the y is inverted here exactly as
                // `pick_sv_rect` inverts it on the way in.
                let cy = layout.sv.position.y + (1.0 - model.value) * layout.sv.size.y;
                out.push(DrawCommand::DrawRect {
                    rect: Rect2::new(
                        Vector2::new(cx - CURSOR * 0.5, cy - CURSOR * 0.5),
                        Vector2::new(CURSOR, CURSOR),
                    ),
                    color: Color::new(1.0, 1.0, 1.0, 1.0),
                    filled: false,
                });
            }
        }
        // pat-s3rrv.172: ColorPickerButton shows its colour as its face.
        "ColorPickerButton" => {
            let color = color_prop(tree, node_id, "color").unwrap_or(Color::WHITE);
            // The swatch is inset so the button still reads as a button — Godot
            // draws the stylebox and then the colour inside it.
            const INSET: f32 = 2.0;
            let inner = Rect2::new(
                Vector2::new(rect.position.x + INSET, rect.position.y + INSET),
                Vector2::new(
                    (rect.size.x - INSET * 2.0).max(0.0),
                    (rect.size.y - INSET * 2.0).max(0.0),
                ),
            );
            if let Some(sb) = theme.get_stylebox("Button", "normal") {
                out.extend(sb.draw(rect));
            } else {
                out.push(DrawCommand::DrawRect {
                    rect,
                    color: stylebox_bg(theme, "Button", "normal", Color::new(0.2, 0.2, 0.2, 1.0)),
                    filled: true,
                });
            }
            out.push(DrawCommand::DrawRect {
                rect: inner,
                color,
                filled: true,
            });
        }
        "Label" => {
            let text = string_prop(tree, node_id, "text").unwrap_or_default();
            if !text.is_empty() {
                // Per-node `theme_override_colors/font_color` wins over the
                // Theme during draw (pat-z35r2.17).
                // A `label_settings` resource REPLACES the theme lookup for
                // every field it owns (pat-s3rrv.127).
                //
                // 🔑 Godot's Label reads `has_settings ? settings->X : theme->X`
                // per field with **no per-field fallback**
                // (`scene/gui/label.cpp:761-767`), so a LabelSettings that sets
                // only an outline still forces the font size and colour to the
                // resource's own values. Treating each field as "override if
                // present, else theme" is the natural reading and it is wrong —
                // it would leave the theme's size in place for exactly that
                // resource. Only `font` itself carries an extra validity check
                // (`:112`), which is why the marker, not the field, decides.
                let has_settings = matches!(
                    tree.get_node(node_id)
                        .map(|n| n.get_property("__label_settings__")),
                    Some(Variant::Bool(true))
                );
                let font_color = if has_settings {
                    color_prop(tree, node_id, "__ls__font_color").unwrap_or(Color::WHITE)
                } else {
                    color_prop(tree, node_id, "theme_override_colors/font_color")
                        .or_else(|| theme.get_color("Label", "font_color"))
                        .unwrap_or(Color::WHITE)
                };
                let font_size = if has_settings {
                    // 16 is `LabelSettings.font_size`'s own Godot default, so an
                    // authored resource that omits it still overrides the theme
                    // with 16 rather than inheriting.
                    int_prop(tree, node_id, "__ls__font_size").unwrap_or(16)
                } else {
                    theme.get_font_size("Label", "font_size").unwrap_or(16)
                };
                // `scale` is no longer computed here — `layout_label_lines` now
                // takes the pixel font size and derives it internally
                // (pat-s3rrv.37).
                // Lay the text out instead of drawing one flush-left line
                // (pat-0q2kx.149). `gdrender2d::text_layout` implemented word
                // breaking, alignment and ellipsis trimming with unit tests but
                // had ZERO consumers anywhere in the workspace, so an authored
                // `autowrap_mode`/`horizontal_alignment`/`text_overrun_behavior`
                // changed nothing on screen.
                for cmd in layout_label_lines(tree, node_id, &text, rect, font_color, font_size) {
                    out.push(cmd);
                }
            }
        }
        "TextureButton" => {
            // Godot picks ONE texture by draw mode and falls back to
            // `texture_normal` whenever the state's own texture is unset
            // (`texture_button.cpp:140-166`) — an unset `texture_pressed` keeps
            // showing the normal art rather than blanking the button mid-click.
            let tex = |key: &str| string_prop(tree, node_id, key).filter(|p| !p.is_empty());
            let disabled = bool_prop(tree, node_id, "disabled").unwrap_or(false);
            let pressed = bool_prop(tree, node_id, "button_pressed").unwrap_or(false);

            // ⚠️ Only three of Godot's five draw modes are reachable here.
            // `texture_hover` and `texture_focused` need hover/focus state, and
            // nothing records hover on a node — `scene_tree` tracks it only for
            // the duration of an event. Selecting them would mean inventing
            // state, so they are left for whoever adds hover tracking.
            let chosen = if disabled {
                tex("texture_disabled").or_else(|| tex("texture_normal"))
            } else if pressed {
                tex("texture_pressed").or_else(|| tex("texture_normal"))
            } else {
                tex("texture_normal")
            }
            // Godot's last resort: with nothing else valid, the focused texture
            // is drawn (`texture_button.cpp:172-176`).
            .or_else(|| tex("texture_focused"));

            if let Some(texture_path) = chosen {
                // ⚠️ Drawn into the control rect, i.e. `STRETCH_SCALE`, whatever
                // `stretch_mode` says — including its default of `STRETCH_KEEP`,
                // which should draw at the texture's NATURAL size.
                //
                // Every other mode needs the texture's dimensions, and this pass
                // cannot know them: `draw_control` emits a path, and the pixels
                // are resolved much later by `gdrender2d::Renderer::find_texture`.
                // There is no size source in `gdscene`, and threading a resource
                // cache in would change the signature of every arm in this file.
                // `TextureRect` above ignores `stretch_mode` for exactly the same
                // reason, so this is consistent rather than newly wrong — but it
                // IS wrong, and the fix belongs with a texture-metadata source
                // for the whole control pass, not in this arm (pat-s3rrv.170).
                out.push(DrawCommand::DrawTextureRect {
                    texture_path,
                    rect,
                    modulate: Color::WHITE,
                });
            }
        }
        // A playing VideoStreamPlayer paints its current frame over the control
        // rect (pat-s3rrv.171). It reaches here because ClassDB parents it to
        // Control, but there was no arm, so it fell through to `_ => {}` and a
        // video drew nothing at all.
        //
        // The command carries the stream path rather than the decoded frame:
        // `DrawCommand` addresses textures by path, and the frame itself is
        // reachable through `SceneTree::get_video_player` for anything that
        // needs the pixels.
        "VideoStreamPlayer" => {
            if let Some(player) = tree.get_video_player(node_id) {
                if !player.stream_path.is_empty() {
                    // `expand` picks the destination size (pat-s3rrv.174):
                    // Godot fills the control when true and keeps the SOURCE
                    // size when false.
                    //
                    // ⚠️ It defaults to **false**, so an absent property must
                    // NOT be read as expand — that is the assumption which had
                    // the registration defaulting to true and silently
                    // stretching every video.
                    let expand = matches!(
                        tree.get_node(node_id).map(|n| n.get_property("expand")),
                        Some(Variant::Bool(true))
                    );
                    let dest = if expand {
                        rect
                    } else {
                        // The current frame's own dimensions, anchored at the
                        // control's origin. Falling back to the control size
                        // when there is no frame yet keeps a just-bound player
                        // from drawing a zero-sized rect.
                        let (w, h) = player
                            .get_video_texture()
                            .map(|img| (img.get_width() as f32, img.get_height() as f32))
                            .unwrap_or((rect.size.x, rect.size.y));
                        Rect2::new(rect.position, Vector2::new(w, h))
                    };
                    out.push(DrawCommand::DrawTextureRect {
                        texture_path: player.stream_path.clone(),
                        rect: dest,
                        modulate: Color::WHITE,
                    });
                }
            }
        }
        "TextureRect" => {
            // A SCRIPTED Texture2D paints itself through `_draw_rect`
            // (pat-s3rrv.160), at the size its own `_get_width`/`_get_height`
            // report rather than the control's rect — a 64x32 procedural
            // texture in a larger rect still draws 64x32.
            let scripted = tree.scripted_resource_commands(node_id);
            if !scripted.is_empty() {
                out.extend(scripted.iter().cloned());
            } else if let Some(path) = string_prop(tree, node_id, "texture") {
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
                        // `region_rect` confines the patches to a sub-rect of
                        // the source texture (pat-s3rrv.167). An absent or
                        // zero-size region means the whole texture, which is
                        // Godot's default and what every existing scene relies
                        // on — so this must not invent a region when the
                        // property is missing.
                        region: match tree
                            .get_node(node_id)
                            .map(|n| n.get_property("region_rect"))
                        {
                            Some(Variant::Rect2(r)) => r,
                            _ => Rect2::new(Vector2::ZERO, Vector2::ZERO),
                        },
                        axis_h: axis("axis_stretch_horizontal"),
                        axis_v: axis("axis_stretch_vertical"),
                        modulate: Color::WHITE,
                    });
                }
            }
        }
        "RichTextLabel" => {
            let font_color = theme
                .get_color("RichTextLabel", "default_color")
                .unwrap_or(Color::WHITE);
            let font_size = theme
                .get_font_size("RichTextLabel", "font_size")
                .unwrap_or(RICHTEXT_DEFAULT_FONT_SIZE);
            let scale = text_scale(font_size);
            // Effects are pure functions of elapsed time, which the label
            // carries as a property so the renderer stays a function of the
            // tree. A frame driver advances it; a test sets it directly.
            let elapsed = float_prop(tree, node_id, "fx_elapsed_time").unwrap_or(0.0) as f32;

            // ONE layout, shared with the `[url]` hit regions — see
            // `richtext_placements`. Parsing, the visible-character budget, line
            // breaking and alignment all live there, so the thing drawn and the
            // thing clickable cannot disagree.
            for placed in richtext_placements(tree, node_id, font_size, rect, font_color) {
                let span = &placed.span;
                let (x, y) = (placed.rect.position.x, placed.rect.position.y);
                // `[bgcolor=…]` paints BEHIND the run, so it is emitted before
                // the glyphs and sized from the run's own placed rect — the same
                // rect the text and the `[u]`/`[s]` rules use, so the fill can
                // never sit a few pixels off the words it belongs to.
                if let Some(bg) = span.bgcolor {
                    out.push(DrawCommand::DrawRect {
                        rect: placed.rect,
                        color: bg,
                        filled: true,
                    });
                }
                match span.effect {
                    // Plain text: one draw call for the whole run.
                    None => {
                        out.push(DrawCommand::DrawString {
                            text: span.text.clone(),
                            position: Vector2::new(x, y),
                            color: span.color,
                            font_size: font_size.max(1) as u32,
                        });
                        // `[b]` (pat-s3rrv.74). The built-in 5x7 bitmap font has
                        // no weight variant, so there is no bold FACE to select.
                        // Over-striking the run one pixel to the right is the
                        // standard bitmap-font answer and thickens every stem by
                        // exactly one pixel.
                        //
                        // ⚠️ Emitted at the SAME y and the same advance, so the
                        // run's width and the `[u]`/`[s]` rules — which are laid
                        // out from `placed.rect` — stay correct. A bold run must
                        // not re-flow the line under it.
                        if span.bold {
                            out.push(DrawCommand::DrawString {
                                text: span.text.clone(),
                                position: Vector2::new(x + BOLD_OVERSTRIKE, y),
                                color: span.color,
                                font_size: font_size.max(1) as u32,
                            });
                        }
                    }
                    // Effected text: each glyph moves independently, so it needs
                    // its own draw call with its own offset/color.
                    Some(effect) => {
                        // The whole tag's range, not this run's — a nested tag
                        // splits the run but must not restart the phase.
                        let range = span.effect_range;
                        // A script-defined effect's result was produced by the
                        // pre-pass (`flush_custom_char_fx`) and cached on the
                        // node, because running GDScript needs `&mut` and this
                        // path holds the tree immutably (pat-fk17e.2). Empty
                        // for every label without custom effects, which is the
                        // common case and costs three property reads.
                        let custom_cache = match effect {
                            BuiltinEffect::Custom { .. } => {
                                Some(crate::char_fx::CharFXCache::from_properties(|key| {
                                    tree.get_node(node_id)
                                        .map(|n| n.get_property(key))
                                        .unwrap_or(Variant::Nil)
                                }))
                            }
                            _ => None,
                        };
                        let mut gx = x;
                        for (i, ch) in span.text.chars().enumerate() {
                            let mut fx = CharFXTransform::new(
                                span.char_offset + i,
                                range,
                                elapsed,
                                span.color,
                            );
                            // ⚠️ `apply` is inert for `Custom`, so the cache is
                            // the ONLY source for it. A glyph the pre-pass did
                            // not reach (missing, short or stale cache) keeps
                            // the transform it was constructed with and draws
                            // normally — visible and unmoved — rather than
                            // vanishing.
                            match &custom_cache {
                                Some(cache) => {
                                    if let Some((offset, color, visible)) =
                                        cache.get(span.char_offset + i)
                                    {
                                        fx.offset = offset;
                                        fx.color = color;
                                        fx.visible = visible;
                                    }
                                }
                                None => effect.apply(&mut fx),
                            }
                            let gw = text_width(&ch.to_string(), scale);
                            if fx.visible {
                                out.push(DrawCommand::DrawString {
                                    text: ch.to_string(),
                                    position: Vector2::new(gx, y) + fx.offset,
                                    color: fx.color,
                                    font_size: font_size.max(1) as u32,
                                });
                            }
                            // Advance by the un-offset width: the effect
                            // displaces the glyph, it must not re-flow the line
                            // under it.
                            gx += gw + GLYPH_GAP * scale as f32;
                        }
                    }
                }
                // `[u]` / `[s]`. The 5x7 bitmap font has no weight or slant
                // variant, so `[b]` is faked by over-striking (above) and `[i]`
                // is carried but not drawn — but underline and strikethrough are
                // RULES rather than glyph shapes, so they are drawable exactly
                // rather than approximated.
                //
                // Spanning the run's full placed width, so an effected run
                // (whose glyphs are displaced individually) still gets one
                // continuous rule: the effect moves the glyphs, not the line
                // under them.
                if span.underline || span.strike {
                    let glyph_h = GLYPH_H * scale as f32;
                    let thickness = scale as f32;
                    let w = placed.rect.size.x;
                    if span.underline {
                        let uy = y + glyph_h + thickness;
                        out.push(DrawCommand::DrawLine {
                            from: Vector2::new(x, uy),
                            to: Vector2::new(x + w, uy),
                            color: span.color,
                            width: thickness,
                        });
                    }
                    if span.strike {
                        let sy = y + glyph_h * 0.5;
                        out.push(DrawCommand::DrawLine {
                            from: Vector2::new(x, sy),
                            to: Vector2::new(x + w, sy),
                            color: span.color,
                            width: thickness,
                        });
                    }
                }
            }
        }
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
                            font_size: font_size.max(1) as u32,
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
        // pat-s3rrv.70: scrollbars. Registered in ClassDB and present in the
        // hit-test and focus lists all along, so a `.tscn` HScrollBar was
        // clickable and tab-focusable — and drew nothing at all. The theme
        // already shipped the `scroll` stylebox for both classes
        // (`theme.rs:585-588`) with no consumer.
        //
        // ⚠️ A scrollbar's grabber is sized by `page`, unlike a slider's fixed
        // one: its length is what tells the user how much of the content is
        // visible. `scrollbar_grabber_rect` owns that geometry so the drawn
        // handle and any future drag agree by construction — the same
        // discipline the slider arm below relies on.
        scroll_class @ ("HScrollBar" | "VScrollBar") => {
            let axis = match crate::range_controls::slider_axis(scroll_class) {
                Some(a) => a,
                None => return,
            };
            let track = stylebox_bg(
                theme,
                scroll_class,
                "scroll",
                Color::new(0.08, 0.08, 0.08, 1.0),
            );
            out.push(DrawCommand::DrawRect {
                rect,
                color: track,
                filled: true,
            });

            let ratio = control::get_range_ratio(tree, node_id);
            let page = num_prop(tree, node_id, "page").unwrap_or(0.0);
            let min = num_prop(tree, node_id, "min_value").unwrap_or(0.0);
            // Godot's Range default max is 100; reading 0 here would make the
            // range degenerate and collapse every grabber to its minimum.
            let max = num_prop(tree, node_id, "max_value").unwrap_or(100.0);
            let local = crate::range_controls::scrollbar_grabber_rect(
                rect.size, axis, ratio, page, min, max,
            );
            let grabber_rect = Rect2::new(
                Vector2::new(
                    rect.position.x + local.position.x,
                    rect.position.y + local.position.y,
                ),
                local.size,
            );
            let grabber = stylebox_bg(
                theme,
                scroll_class,
                "grabber",
                Color::new(0.7, 0.7, 0.7, 1.0),
            );
            out.push(DrawCommand::DrawRect {
                rect: grabber_rect,
                color: grabber,
                filled: true,
            });
        }
        // pat-s3rrv.69: sliders. Geometry comes from `range_controls` — the
        // same functions the drag path uses — so the grabber cannot be drawn
        // anywhere other than where clicking it selects.
        // ⚠️ `slider_class @ (...)` binds the matched string. The scrutinee
        // here is `class_of(tree, node_id).as_str()`, an unnamed temporary — there
        // is no `class_name` in scope, and both the theme lookups and the axis
        // choice need to know WHICH of the two matched.
        // pat-s3rrv.72: the MenuBar title strip. `MenuBarModel` owns the x
        // layout so the titles DRAWN sit exactly where `menu_bar_title_under`
        // maps clicks — one rule, as `PopupMenu::ROW_HEIGHT` is for rows.
        "MenuBar" => {
            let (titles, open) = tree.menu_bar_strip(node_id);
            let model = crate::menu::MenuBarModel::new(titles);
            let h = crate::menu::MenuBarModel::TITLE_HEIGHT;
            out.push(DrawCommand::DrawRect {
                rect: Rect2::new(rect.position, Vector2::new(rect.size.x, h)),
                color: Color::new(0.18, 0.18, 0.20, 1.0),
                filled: true,
            });
            for (i, title) in model.titles().iter().enumerate() {
                let x = rect.position.x + model.title_x(i);
                let w = crate::menu::MenuBarModel::title_width(title);
                // The open title stays highlighted while its menu is down, which
                // is what tells the user which strip entry the panel belongs to.
                if open == Some(i) {
                    out.push(DrawCommand::DrawRect {
                        rect: Rect2::new(Vector2::new(x, rect.position.y), Vector2::new(w, h)),
                        color: Color::new(0.26, 0.36, 0.56, 1.0),
                        filled: true,
                    });
                }
                out.push(DrawCommand::DrawString {
                    text: title.clone(),
                    position: Vector2::new(
                        x + crate::menu::MenuBarModel::TITLE_PADDING,
                        rect.position.y + h - 7.0,
                    ),
                    color: Color::new(0.9, 0.9, 0.9, 1.0),
                    font_size: 14,
                });
            }
        }
        slider_class @ ("HSlider" | "VSlider") => {
            let axis = match crate::range_controls::slider_axis(slider_class) {
                Some(a) => a,
                None => return,
            };
            // The track, from the theme's own `slider` stylebox.
            let track = stylebox_bg(
                theme,
                slider_class,
                "slider",
                Color::new(0.1, 0.1, 0.1, 1.0),
            );
            out.push(DrawCommand::DrawRect {
                rect,
                color: track,
                filled: true,
            });

            let ratio = control::get_range_ratio(tree, node_id);
            let local = crate::range_controls::grabber_rect(rect.size, axis, ratio);
            // `grabber_rect` works in control-local space; shift it onto the
            // control's global rect for the draw list.
            let grabber_rect = Rect2::new(
                Vector2::new(
                    rect.position.x + local.position.x,
                    rect.position.y + local.position.y,
                ),
                local.size,
            );
            let grabber = stylebox_bg(
                theme,
                slider_class,
                "grabber",
                Color::new(0.8, 0.8, 0.8, 1.0),
            );
            out.push(DrawCommand::DrawRect {
                rect: grabber_rect,
                color: grabber,
                filled: true,
            });

            // Godot draws `tick_count` marks across the track; 0 (the default)
            // means none, so the loop is skipped entirely rather than drawing a
            // single degenerate tick.
            let ticks = num_prop(tree, node_id, "tick_count").unwrap_or(0.0) as i64;
            if ticks > 1 {
                let tick_color = theme
                    .get_color(slider_class, "font_color")
                    .unwrap_or(Color::new(0.6, 0.6, 0.6, 1.0));
                let span = crate::range_controls::track_length(rect.size, axis);
                for i in 0..ticks {
                    let t = i as f32 / (ticks - 1) as f32;
                    let (from, to) = match axis {
                        crate::range_controls::SliderAxis::Horizontal => {
                            let x = rect.position.x
                                + crate::range_controls::GRABBER_EXTENT * 0.5
                                + span * t;
                            (
                                Vector2::new(x, rect.position.y),
                                Vector2::new(x, rect.position.y + rect.size.y),
                            )
                        }
                        crate::range_controls::SliderAxis::Vertical => {
                            let y = rect.position.y
                                + crate::range_controls::GRABBER_EXTENT * 0.5
                                + span * t;
                            (
                                Vector2::new(rect.position.x, y),
                                Vector2::new(rect.position.x + rect.size.x, y),
                            )
                        }
                    };
                    out.push(DrawCommand::DrawLine {
                        from,
                        to,
                        color: tick_color,
                        width: 1.0,
                    });
                }
            }
        }
        "SpinBox" => {
            // The field, then the value text, then the two arrows. Godot builds
            // this from an embedded LineEdit plus arrow buttons; Patina draws it
            // directly, which is enough for the value to be legible and the
            // arrows to be hit-testable.
            let bg = stylebox_bg(
                theme,
                "SpinBox",
                "normal",
                Color::new(0.15, 0.15, 0.15, 1.0),
            );
            out.push(DrawCommand::DrawRect {
                rect,
                color: bg,
                filled: true,
            });
            let fg = theme
                .get_color("SpinBox", "font_color")
                .unwrap_or(Color::new(0.875, 0.875, 0.875, 1.0));

            // `prefix`/`suffix` bracket the number, which is how a SpinBox shows
            // units ("50 %"). Godot puts a space between suffix and value.
            let value = control::get_range_value(tree, node_id);
            let prefix = string_prop(tree, node_id, "prefix").unwrap_or_default();
            let suffix = string_prop(tree, node_id, "suffix").unwrap_or_default();
            let text = format!(
                "{}{}{}",
                if prefix.is_empty() {
                    String::new()
                } else {
                    format!("{prefix} ")
                },
                format_spinbox_value(value, control::get_range_step(tree, node_id)),
                if suffix.is_empty() {
                    String::new()
                } else {
                    format!(" {suffix}")
                },
            );
            out.push(DrawCommand::DrawString {
                text,
                position: Vector2::new(
                    rect.position.x + 4.0,
                    rect.position.y + (rect.size.y - GLYPH_H) * 0.5,
                ),
                color: fg,
                font_size: 1,
            });

            // Up/down arrows on the right edge, stacked.
            let arrow_w = 12.0f32.min(rect.size.x * 0.25);
            let half_h = rect.size.y * 0.5;
            // ⚠️ `into_iter`, not `iter`: iterating an array of `&str` by
            // reference yields `&&str`, which does not coerce to the `&str`
            // `stylebox_bg` takes.
            for (i, item) in ["up", "down"].into_iter().enumerate() {
                let arrow = stylebox_bg(theme, "SpinBox", item, Color::new(0.3, 0.3, 0.3, 1.0));
                out.push(DrawCommand::DrawRect {
                    rect: Rect2::new(
                        Vector2::new(
                            rect.position.x + rect.size.x - arrow_w,
                            rect.position.y + half_h * i as f32,
                        ),
                        Vector2::new(arrow_w, half_h),
                    ),
                    color: arrow,
                    filled: true,
                });
            }
        }
        "TextureProgressBar" => {
            let ratio = control::get_range_ratio(tree, node_id);
            let fill_mode = num_prop(tree, node_id, "fill_mode").unwrap_or(0.0) as i64;
            let tint = |key: &str| {
                match tree.get_node(node_id).map(|n| n.get_property(key)) {
                    Some(Variant::Color(c)) => c,
                    // The tints MULTIPLY their texture, so white is "unchanged".
                    // Falling back to transparent or black would hide the bar.
                    _ => Color::new(1.0, 1.0, 1.0, 1.0),
                }
            };

            // `texture_under` is the trough and always covers the whole rect.
            if let Some(path) = string_prop(tree, node_id, "texture_under") {
                out.push(DrawCommand::DrawTextureRect {
                    texture_path: path,
                    rect,
                    modulate: tint("tint_under"),
                });
            }

            if crate::range_controls::is_radial_fill(fill_mode) {
                let offset = match tree
                    .get_node(node_id)
                    .map(|n| n.get_property("radial_center_offset"))
                {
                    Some(Variant::Vector2(v)) => v,
                    _ => Vector2::new(0.0, 0.0),
                };
                let points = crate::range_controls::radial_fill_polygon(
                    rect,
                    offset,
                    num_prop(tree, node_id, "radial_initial_angle").unwrap_or(0.0) as f32,
                    num_prop(tree, node_id, "radial_fill_degrees").unwrap_or(360.0) as f32,
                    ratio,
                    fill_mode == crate::range_controls::FILL_COUNTER_CLOCKWISE,
                );
                if points.len() >= 3 {
                    let color = tint("tint_progress");
                    out.push(DrawCommand::DrawPolygon {
                        points,
                        colors: vec![color],
                    });
                }
            } else {
                let fill = crate::range_controls::linear_fill_rect(rect, fill_mode, ratio);
                if fill.size.x > 0.0 && fill.size.y > 0.0 {
                    match string_prop(tree, node_id, "texture_progress") {
                        Some(path) => out.push(DrawCommand::DrawTextureRect {
                            texture_path: path,
                            rect: fill,
                            modulate: tint("tint_progress"),
                        }),
                        // No texture authored: still emit the filled region, so
                        // an untextured bar is visible and testable rather than
                        // silently drawing nothing.
                        None => out.push(DrawCommand::DrawRect {
                            rect: fill,
                            color: tint("tint_progress"),
                            filled: true,
                        }),
                    }
                }
            }

            // `texture_over` is drawn last: it is the frame that sits on top of
            // the fill.
            if let Some(path) = string_prop(tree, node_id, "texture_over") {
                out.push(DrawCommand::DrawTextureRect {
                    texture_path: path,
                    rect,
                    modulate: tint("tint_over"),
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
                    font_size: font_size.max(1) as u32,
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
                    font_size: font_size.max(1) as u32,
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
            let font_size = theme.get_font_size("ItemList", "font_size").unwrap_or(16);
            let scale = text_scale(font_size);
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
                        font_size: font_size.max(1) as u32,
                    });
                }
            }
        }
        // One row per visible item, indented by depth, with a fold arrow on any
        // row that has children (pat-s3rrv.75).
        //
        // ⚠️ Every rect here comes from `item_views` — the SAME row list and
        // column spans `item_view_click` resolves a click through. Laying rows
        // out independently is how a click comes to select a different item than
        // the one under the cursor, and with folding in play the two derivations
        // diverge the moment a subtree collapses.
        "Tree" => {
            let bg = stylebox_bg(theme, "Tree", "panel", Color::new(0.12, 0.12, 0.12, 1.0));
            out.push(DrawCommand::DrawRect {
                rect,
                color: bg,
                filled: true,
            });
            let selected_bg =
                stylebox_bg(theme, "Tree", "selected", Color::new(0.26, 0.42, 0.66, 1.0));
            let font_color = color_prop(tree, node_id, "theme_override_colors/font_color")
                .or_else(|| theme.get_color("Tree", "font_color"))
                .unwrap_or(Color::new(0.875, 0.875, 0.875, 1.0));
            let font_size = theme.get_font_size("Tree", "font_size").unwrap_or(16);
            let scale = text_scale(font_size);
            let th = GLYPH_H * scale as f32;

            let model = crate::item_views::load_tree(tree, node_id);
            let spans = crate::item_views::tree_column_spans(&model, rect);
            let selected = model.get_selected();
            for row in crate::item_views::tree_visible_rows(tree, node_id) {
                // Rows past the bottom edge are scrolled out of view.
                if row.rect.position.y >= rect.position.y + rect.size.y {
                    break;
                }
                if selected == Some(row.item) {
                    out.push(DrawCommand::DrawRect {
                        rect: row.rect,
                        color: selected_bg,
                        filled: true,
                    });
                }
                // The fold arrow, drawn only where one is clickable — the rect
                // is the very one the click path tests against.
                if let Some(arrow) = row.arrow_rect() {
                    let inset = arrow.size.x * 0.25;
                    out.push(DrawCommand::DrawRect {
                        rect: Rect2::new(
                            Vector2::new(arrow.position.x + inset, arrow.position.y + inset),
                            Vector2::new(arrow.size.x - inset * 2.0, arrow.size.y - inset * 2.0),
                        ),
                        color: font_color,
                        // Godot draws a filled triangle for an expanded item and
                        // an outline for a collapsed one; with no triangle
                        // primitive the fill/outline distinction carries the
                        // state so the two are still tellable apart.
                        filled: !model.is_collapsed(row.item),
                    });
                }
                for column in 0..model.column_count() {
                    // A CHECK cell's box, at the cell's own content origin.
                    if model.get_cell_mode(row.item, column)
                        == crate::item_controls::TreeCellMode::Check
                    {
                        let box_rect = row.check_rect(column, &spans);
                        out.push(DrawCommand::DrawRect {
                            rect: box_rect,
                            color: font_color,
                            filled: model.is_checked(row.item, column),
                        });
                    }
                    let text = model.get_text(row.item, column);
                    if text.is_empty() {
                        continue;
                    }
                    // Column 0 clears the indent and the arrow gutter; the rest
                    // start at their own span. A CHECK cell shifts past its box.
                    let mut x = row.cell_content_x(column, &spans);
                    if model.get_cell_mode(row.item, column)
                        == crate::item_controls::TreeCellMode::Check
                    {
                        x += row.check_rect(column, &spans).size.x + GLYPH_GAP * 2.0;
                    }
                    out.push(DrawCommand::DrawString {
                        text: text.to_string(),
                        position: Vector2::new(
                            x + GLYPH_GAP * 2.0 * scale as f32,
                            row.rect.position.y + (row.rect.size.y - th) * 0.5,
                        ),
                        color: font_color,
                        font_size: font_size.max(1) as u32,
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
            let font_size = crate::control::get_font_size(tree, node_id);
            let scale = text_scale(font_size);
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
                        font_size: font_size.max(1) as u32,
                    });
                }
            }
        }
        "FoldableContainer" => {
            // A title bar that is always drawn, plus a content panel that is
            // drawn only while the container is open (pat-s3rrv.165).
            //
            // ⚠️ The bands are ordered by `title_position`, not fixed to the
            // top: `POSITION_BOTTOM` puts the title flush with the bottom edge
            // and the content above it (`foldable_container.cpp:252`).
            let title_h = crate::control::get_foldable_title_height(tree, node_id).min(rect.size.y);
            let folded = crate::control::is_foldable_folded(tree, node_id);
            let at_bottom = crate::control::foldable_title_at_bottom(tree, node_id);

            if !folded && rect.size.y > title_h {
                let content_y = if at_bottom {
                    rect.position.y
                } else {
                    rect.position.y + title_h
                };
                out.push(DrawCommand::DrawRect {
                    rect: Rect2::new(
                        Vector2::new(rect.position.x, content_y),
                        Vector2::new(rect.size.x, rect.size.y - title_h),
                    ),
                    color: stylebox_bg(
                        theme,
                        "FoldableContainer",
                        "panel_style",
                        Color::new(0.15, 0.15, 0.15, 1.0),
                    ),
                    filled: true,
                });
            }

            if title_h <= 0.0 {
                return;
            }
            let title_y = if at_bottom {
                rect.position.y + rect.size.y - title_h
            } else {
                rect.position.y
            };
            out.push(DrawCommand::DrawRect {
                rect: Rect2::new(
                    Vector2::new(rect.position.x, title_y),
                    Vector2::new(rect.size.x, title_h),
                ),
                color: stylebox_bg(
                    theme,
                    "FoldableContainer",
                    "title_style",
                    Color::new(0.22, 0.22, 0.22, 1.0),
                ),
                filled: true,
            });

            let title = crate::control::get_foldable_title(tree, node_id);
            if !title.is_empty() {
                // ⚠️ `get_font_size`/`get_color` on `Theme`, NOT the
                // `*_or` pair — those live on `ThemeDB` (theme.rs:658-693) and
                // the names are close enough to read as interchangeable.
                let font_size = theme
                    .get_font_size("FoldableContainer", "title_font_size")
                    .unwrap_or(16);
                let text_w = measure_text_px(&title, font_size as f32);
                let pad = crate::control::FOLDABLE_TITLE_H_PADDING;
                // Godot's `HorizontalAlignment`: LEFT 0, CENTER 1, RIGHT 2,
                // FILL 3. FILL has nothing to stretch for a single run, so it
                // lands with LEFT, as it does upstream.
                let x = match crate::control::foldable_title_alignment(tree, node_id) {
                    1 => rect.position.x + (rect.size.x - text_w) * 0.5,
                    2 => rect.position.x + rect.size.x - text_w - pad,
                    _ => rect.position.x + pad,
                };
                out.push(DrawCommand::DrawString {
                    text: title,
                    // Centred in the band: the title is one line, so the glyph
                    // box is the font size and the leftover splits evenly.
                    position: Vector2::new(x, title_y + (title_h - font_size as f32) * 0.5),
                    color: theme
                        .get_color("FoldableContainer", "title_font_color")
                        .unwrap_or(Color::new(0.9, 0.9, 0.9, 1.0)),
                    font_size: font_size.max(1) as u32,
                });
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
            let font_size = crate::control::get_font_size(tree, node_id);
            let scale = text_scale(font_size);
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
                        font_size: font_size.max(1) as u32,
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

    /// pat-s3rrv.165 — the title bar is drawn, with its text, and an open
    /// container also gets a content panel.
    ///
    /// ⚠️ The folded case is the one worth asserting: a folded container still
    /// draws its title (that is the only thing left to click) but must NOT draw
    /// the content panel behind children that are collapsed to nothing.
    #[test]
    fn foldable_container_draws_its_title_bar() {
        let render = |folded: bool| {
            let mut tree = SceneTree::new();
            let root = tree.root_id();
            let id = tree
                .add_child(root, Node::new("F", "FoldableContainer"))
                .unwrap();
            control::apply_anchor_preset(&mut tree, id, AnchorPreset::FullRect);
            if let Some(n) = tree.get_node_mut(id) {
                n.set_property("title", Variant::String("Details".into()));
                n.set_property("folded", Variant::Bool(folded));
            }
            let mut vp = Viewport::new(200, 200, Color::BLACK);
            let theme = Theme::default_theme();
            render_control_tree(&mut tree, id, &theme, &mut vp, Vector2::new(200.0, 200.0));
            let cmds = vp.get_sorted_items()[0].commands.clone();
            let has_title = cmds
                .iter()
                .any(|c| matches!(c, DrawCommand::DrawString { text, .. } if text == "Details"));
            let filled = cmds
                .iter()
                .filter(|c| matches!(c, DrawCommand::DrawRect { filled: true, .. }))
                .count();
            (has_title, filled)
        };

        let (open_title, open_rects) = render(false);
        assert!(open_title, "an open container draws its title text");
        assert!(
            open_rects >= 2,
            "and both bands — content panel + title bar — got {open_rects}"
        );

        let (folded_title, folded_rects) = render(true);
        assert!(
            folded_title,
            "a FOLDED container still draws its title: it is the only thing left \
             to click, so losing it makes the container unrecoverable"
        );
        assert!(
            folded_rects < open_rects,
            "but it must draw fewer bands than an open one — the content panel \
             belongs to content that is collapsed to nothing (open {open_rects}, \
             folded {folded_rects})"
        );
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
