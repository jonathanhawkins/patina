//! Building the 2D viewport's render list from CanvasItem (Node2D) content.
//!
//! Godot's 2D editor draws the actual visuals of each `CanvasItem` — a
//! `Sprite2D` shows its texture at the node's transform, not an abstract
//! position marker. This module turns the scene's visible 2D nodes into
//! concrete draw items the renderer can mount: a **sprite** (texture + offset +
//! flip) for `Sprite2D` nodes that carry a texture, or a **marker** fallback for
//! every other 2D node (cameras, empties, and shape/text nodes handled by later
//! sub-tasks) so they stay visible and pickable in the editor.
//!
//! It lives in its own module (over the public `viewport_2d` API) to compose
//! with the viewport without growing the already-large `viewport_2d.rs`.

use gdcore::math::{Color, Rect2, Transform2D, Vector2};
use gdrender2d::draw;
use gdrender2d::font::{draw_string, BitmapFont};
use gdrender2d::renderer::FrameBuffer;
use gdscene::control::TextAlign;
use gdscene::control_render::ControlDrawItem;
use gdscene::{control, node2d, SceneTree};
use gdvariant::Variant;

use crate::texture_cache::TextureCache;
use crate::viewport_2d::Viewport2D;

/// The kind of content drawn for a 2D canvas node.
#[derive(Debug, Clone, PartialEq)]
pub enum Render2DKind {
    /// A `Sprite2D` texture to render, with its draw offset and flip flags.
    Sprite {
        /// The texture resource path.
        texture: String,
        /// The sprite's local draw offset (Godot's `offset` property).
        offset: Vector2,
        /// Whether the texture is mirrored horizontally.
        flip_h: bool,
        /// Whether the texture is mirrored vertically.
        flip_v: bool,
    },
    /// A `Polygon2D`'s filled polygon: its vertices (local space) and fill color.
    Polygon {
        /// The polygon vertices, in node-local space.
        points: Vec<Vector2>,
        /// The fill color.
        color: Color,
    },
    /// A `Line2D`'s polyline: its points (local space), stroke width, and color.
    Line {
        /// The polyline points, in node-local space.
        points: Vec<Vector2>,
        /// The stroke width in pixels.
        width: f32,
        /// The stroke color.
        color: Color,
    },
    /// A `ColorRect`'s solid rectangle: its size and fill color.
    ColorRect {
        /// The rectangle size (Control's `size`).
        size: Vector2,
        /// The fill color.
        color: Color,
    },
    /// A `Label`'s text: its string, font size, and horizontal alignment.
    Text {
        /// The label's displayed text.
        text: String,
        /// The font size (theme override, defaulting to Godot's 16).
        font_size: i64,
        /// The horizontal text alignment.
        h_align: TextAlign,
    },
    /// A position marker drawn for a 2D node with no own visual yet (cameras,
    /// empty `Node2D`s, and text nodes that later sub-tasks render) so it stays
    /// visible and pickable in the editor.
    Marker {
        /// The node's class name (drives the marker glyph).
        class: String,
    },
}

/// One drawable item in the 2D viewport's render list.
#[derive(Debug, Clone, PartialEq)]
pub struct Render2DItem {
    /// The source node's raw id.
    pub node_id: u64,
    /// The node's global (canvas-space) transform — the composed parent chain,
    /// so a child is drawn relative to its parent's transform.
    pub global_transform: Transform2D,
    /// The node's `modulate` tint (defaulting to white), applied to its visual.
    pub modulate: Color,
    /// The node's `z_index`; items are returned sorted by it ascending so lower
    /// z draws first (behind) and higher z draws last (in front).
    pub z_index: i64,
    /// The `layer` of the node's nearest `CanvasLayer` ancestor (Godot's
    /// `CanvasLayer.layer`), or `0` for the default canvas. Items sort by
    /// `(canvas_layer, z_index)`, so a node on a higher layer always draws over
    /// one on a lower layer regardless of `z_index`.
    pub canvas_layer: i64,
    /// What to draw for this node.
    pub kind: Render2DKind,
}

impl Viewport2D {
    /// Builds the 2D viewport's render list from `tree`: each **visible**
    /// `CanvasItem` node (the Node2D and Control families) becomes a
    /// [`Render2DItem`] — a sprite for texture-bearing `Sprite2D` nodes, a
    /// polygon/line/color-rect for the corresponding shape nodes, or a marker
    /// otherwise. Nodes hidden by their own or an ancestor's `visible` flag are
    /// skipped, and items appear in scene-tree order (the back-to-front draw
    /// order).
    pub fn build_render_list(&self, tree: &SceneTree) -> Vec<Render2DItem> {
        collect_canvas_items(tree)
    }
}

/// Walks `tree` in scene order and returns a [`Render2DItem`] for every
/// **visible** `CanvasItem` (the Node2D and Control families), ordered
/// back-to-front by `(canvas_layer, z_index)` with scene-tree order as the
/// stable tiebreaker. Hidden and non-canvas (spatial) nodes are skipped.
///
/// Shared by the editor viewport's render list ([`Viewport2D::build_render_list`])
/// and the runtime canvas rasterizer ([`build_runtime_render_list`]) so both draw
/// the same nodes in the same order.
pub fn collect_canvas_items(tree: &SceneTree) -> Vec<Render2DItem> {
    let mut items: Vec<Render2DItem> = tree
        .all_nodes_in_tree_order()
        .into_iter()
        .filter_map(|id| {
            let node = tree.get_node(id)?;
            // Only canvas items (Node2D and Control families) participate in
            // the 2D render list; spatial (Node3D) nodes never do.
            if !node.is_class("CanvasItem") {
                return None;
            }
            // Hidden nodes (own flag or inherited) draw nothing.
            if !node2d::is_visible_in_tree(tree, id) {
                return None;
            }
            Some(Render2DItem {
                node_id: id.raw(),
                global_transform: node2d::get_global_transform(tree, id),
                modulate: read_color(tree, id, "modulate"),
                z_index: node2d::get_z_index(tree, id),
                canvas_layer: canvas_layer_for(tree, id),
                kind: render_kind_for(tree, id, node.class_name()),
            })
        })
        .collect();
    // Godot draws canvas layers in ascending `layer` order, and within a layer
    // by ascending `z_index`; a stable sort keeps scene-tree order as the
    // tiebreaker within the same layer+z.
    items.sort_by_key(|i| (i.canvas_layer, i.z_index));
    items
}

/// Returns the `layer` index of `id`'s nearest `CanvasLayer` ancestor, or `0`
/// when the node lives in the default canvas (no `CanvasLayer` above it).
///
/// Godot draws each `CanvasLayer` above the default canvas (whose implicit layer
/// is `0`), and a `CanvasLayer` node's `layer` property defaults to `1`, so a UI
/// layer naturally renders over the world. Read straight off the node's `layer`
/// property (the `gdscene` canvas-layer helper module is not yet wired in).
fn canvas_layer_for(tree: &SceneTree, id: gdscene::NodeId) -> i64 {
    let mut current = tree.get_node(id).and_then(|n| n.parent());
    while let Some(pid) = current {
        let Some(node) = tree.get_node(pid) else {
            break;
        };
        if node.is_class("CanvasLayer") {
            return match node.get_property("layer") {
                Variant::Int(l) => l,
                // Godot's CanvasLayer.layer defaults to 1.
                _ => 1,
            };
        }
        current = node.parent();
    }
    0
}

/// Chooses the draw kind for a visible canvas node: a sprite for a textured
/// `Sprite2D`, a polygon/line/color-rect for the shape nodes, otherwise a
/// class-tagged marker.
fn render_kind_for(tree: &SceneTree, id: gdscene::NodeId, class: &str) -> Render2DKind {
    match class {
        "Sprite2D" => {
            if let Some(texture) = node2d::get_texture_path(tree, id) {
                return Render2DKind::Sprite {
                    texture,
                    offset: read_vector2(tree, id, "offset"),
                    flip_h: read_bool(tree, id, "flip_h"),
                    flip_v: read_bool(tree, id, "flip_v"),
                };
            }
        }
        "Polygon2D" => {
            return Render2DKind::Polygon {
                points: read_points(tree, id, "polygon"),
                color: read_color(tree, id, "color"),
            };
        }
        "Line2D" => {
            return Render2DKind::Line {
                points: read_points(tree, id, "points"),
                width: read_f32(tree, id, "width", 0.0),
                color: read_color(tree, id, "default_color"),
            };
        }
        "ColorRect" => {
            return Render2DKind::ColorRect {
                size: control::get_size(tree, id),
                color: read_color(tree, id, "color"),
            };
        }
        "Label" => {
            return Render2DKind::Text {
                text: control::get_label_text(tree, id),
                font_size: control::get_font_size(tree, id),
                h_align: control::get_h_align(tree, id),
            };
        }
        _ => {}
    }
    Render2DKind::Marker {
        class: class.to_owned(),
    }
}

/// Reads a `Vector2` property, defaulting to [`Vector2::ZERO`] when absent.
fn read_vector2(tree: &SceneTree, id: gdscene::NodeId, key: &str) -> Vector2 {
    match tree.get_node(id).map(|n| n.get_property(key)) {
        Some(Variant::Vector2(v)) => v,
        _ => Vector2::ZERO,
    }
}

/// Reads a `bool` property, defaulting to `false` when absent.
fn read_bool(tree: &SceneTree, id: gdscene::NodeId, key: &str) -> bool {
    matches!(
        tree.get_node(id).map(|n| n.get_property(key)),
        Some(Variant::Bool(true))
    )
}

/// Reads a `Color` property, defaulting to white (Godot's default tint) when
/// absent.
fn read_color(tree: &SceneTree, id: gdscene::NodeId, key: &str) -> Color {
    match tree.get_node(id).map(|n| n.get_property(key)) {
        Some(Variant::Color(c)) => c,
        _ => Color::WHITE,
    }
}

/// Reads a packed `Vector2` array property (a polygon/polyline point list),
/// dropping any non-`Vector2` entries and defaulting to empty when absent.
fn read_points(tree: &SceneTree, id: gdscene::NodeId, key: &str) -> Vec<Vector2> {
    match tree.get_node(id).map(|n| n.get_property(key)) {
        Some(Variant::Array(items)) => items
            .into_iter()
            .filter_map(|v| match v {
                Variant::Vector2(p) => Some(p),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// Reads a numeric (`Float`/`Int`) property as `f32`, falling back to `default`
/// when absent or non-numeric.
fn read_f32(tree: &SceneTree, id: gdscene::NodeId, key: &str, default: f32) -> f32 {
    match tree.get_node(id).map(|n| n.get_property(key)) {
        Some(Variant::Float(f)) => f as f32,
        Some(Variant::Int(i)) => i as f32,
        _ => default,
    }
}

// ===========================================================================
// Runtime canvas rasterizer (pat-u3l8c.23)
//
// The editor *viewport* draws an abstract preview of the scene — a grid,
// rulers, gizmos, and class-glyph markers (see `scene_renderer`). A *running*
// 2D game draws none of that: it rasterizes the actual `CanvasItem` pixels the
// game would show. These functions turn the render list above into real pixels
// for the editor's **Play** tab and the runtime framebuffer.
// ===========================================================================

/// The background color a running 2D scene clears to before its canvas is
/// drawn — Godot's `rendering/environment/defaults/default_clear_color` default
/// (a mid gray). The runtime frame has no editor chrome (grid/rulers/gizmos),
/// only the game's own pixels over this clear.
pub const RUNTIME_CLEAR_COLOR: Color = Color::new(0.3, 0.3, 0.3, 1.0);

/// Builds the runtime 2D render list for `tree`: the same visible-`CanvasItem`
/// walk the editor viewport uses ([`collect_canvas_items`]), ordered by
/// `(canvas_layer, z_index)`.
///
/// Returned owned (borrowing only `tree`) so a caller can drop its `tree` borrow
/// before rasterizing with a mutable [`TextureCache`] — the editor's Play loop
/// borrows both the running tree and the texture cache from the same state.
pub fn build_runtime_render_list(tree: &SceneTree) -> Vec<Render2DItem> {
    collect_canvas_items(tree)
}

/// Lays out and draws the running scene's `Control` subtree into render items the
/// 2D rasterizer already understands (pat-0q2kx.19).
///
/// Before this, the Play view drew only Sprite2D/Polygon2D/Line2D/ColorRect/Label
/// and every other control fell through to [`Render2DKind::Marker`] — so a themed
/// UI rendered as a scatter of markers at the origin, with containers never laid
/// out. Two things happen here, in Godot's order:
///
/// 1. every container is sorted (`Container::_sort_children`), parents before
///    children, so child rects are real rather than all sitting at the origin;
/// 2. the control tree is drawn by [`gdscene::control_render`] — the same themed
///    pass the unit tests exercise — and its primitives are mapped onto
///    [`Render2DKind`], so no per-control drawing rule is duplicated here.
///
/// `z_index` carries the walk's paint order (parents first) and is offset by
/// [`CONTROL_Z_BASE`] so UI composites above plain 2D nodes, which is what a
/// Godot scene with a UI layer looks like.
pub fn build_control_render_items(
    tree: &mut SceneTree,
    root: gdscene::NodeId,
    size: Vector2,
) -> Vec<Render2DItem> {
    // (1) Lay out containers first. `sort_container_tree` recurses, but it must be
    // entered at every container that is *not* nested inside another container —
    // entering at the subtree root would skip a container whose parent is a plain
    // Control (e.g. Panel > VBoxContainer).
    let mut subtree = Vec::new();
    tree.collect_subtree_top_down(root, &mut subtree);
    for id in subtree {
        let is_container = tree
            .get_node(id)
            .map(|n| control::is_container_class_name(n.class_name()))
            .unwrap_or(false);
        if is_container {
            control::sort_container_tree(tree, id, size);
        }
    }

    // (2) Draw, then translate each primitive into a render item. Positions are
    // already global viewport space, so the item transform is a pure translation.
    let theme = gdscene::theme::Theme::default_theme();
    let items = gdscene::control_render::control_draw_items(tree, root, &theme, size);
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        let (transform, z, kind) = match item {
            ControlDrawItem::Rect {
                rect,
                color,
                filled,
                z,
            } => {
                if filled {
                    (
                        Transform2D::translated(rect.position),
                        z,
                        Render2DKind::ColorRect {
                            size: rect.size,
                            color,
                        },
                    )
                } else {
                    // No stroked-rect kind exists; a closed polyline is the
                    // faithful equivalent and the rasterizer already draws it.
                    let (p, s) = (rect.position, rect.size);
                    (
                        Transform2D::IDENTITY,
                        z,
                        Render2DKind::Line {
                            points: vec![
                                p,
                                Vector2::new(p.x + s.x, p.y),
                                Vector2::new(p.x + s.x, p.y + s.y),
                                Vector2::new(p.x, p.y + s.y),
                                p,
                            ],
                            width: 1.0,
                            color,
                        },
                    )
                }
            }
            ControlDrawItem::Circle {
                center,
                radius,
                color,
                z,
            } => {
                // Approximated as a filled polygon — a radio indicator drawn as a
                // square would read as a checkbox.
                const SEGMENTS: usize = 16;
                let points = (0..SEGMENTS)
                    .map(|i| {
                        let a = i as f32 / SEGMENTS as f32 * std::f32::consts::TAU;
                        Vector2::new(center.x + radius * a.cos(), center.y + radius * a.sin())
                    })
                    .collect();
                (
                    Transform2D::IDENTITY,
                    z,
                    Render2DKind::Polygon { points, color },
                )
            }
            ControlDrawItem::Line {
                from,
                to,
                color,
                width,
                z,
            } => (
                Transform2D::IDENTITY,
                z,
                Render2DKind::Line {
                    points: vec![from, to],
                    width,
                    color,
                },
            ),
            ControlDrawItem::Text {
                text,
                position,
                font_size,
                z,
                ..
            } => (
                Transform2D::translated(position),
                z,
                Render2DKind::Text {
                    text,
                    font_size: font_size as i64,
                    h_align: TextAlign::Left,
                },
            ),
            ControlDrawItem::Texture {
                texture_path,
                rect,
                z,
                ..
            } => (
                Transform2D::translated(rect.position),
                z,
                Render2DKind::Sprite {
                    texture: texture_path,
                    offset: Vector2::ZERO,
                    flip_h: false,
                    flip_v: false,
                },
            ),
        };
        out.push(Render2DItem {
            // Control primitives are not one-per-node, so there is no single node
            // id to attribute them to; 0 marks "not pickable as a node".
            node_id: 0,
            global_transform: transform,
            // Each primitive already carries its own themed colour, so the
            // item-level tint stays neutral.
            modulate: Color::new(1.0, 1.0, 1.0, 1.0),
            z_index: CONTROL_Z_BASE + z as i64,
            canvas_layer: 0,
            kind,
        });
    }
    out
}

/// Paint-order floor for control primitives, so a themed UI composites above the
/// plain 2D nodes of the same canvas layer (Godot scenes put UI on top).
pub const CONTROL_Z_BASE: i64 = 1 << 20;

/// Rasterizes an ordered runtime render list into a fresh [`FrameBuffer`],
/// drawing the real CanvasItem pixels a running 2D game would show: textured
/// sprites, filled color rects, label text, filled polygons, and polylines,
/// composited back-to-front in the list's order (`items` must already be sorted,
/// as [`build_runtime_render_list`] returns them).
///
/// This is what the editor's **Play** tab and the running-game framebuffer
/// display (pat-u3l8c.23), replacing the class-glyph icon/grid preview with the
/// pixels the game itself draws. When `texture_cache` is `Some`, `Sprite2D`
/// textures are loaded and blitted at each node's global transform (honoring
/// `offset`, `flip_h`/`flip_v`, and `modulate`); without a cache — or when a
/// texture fails to load — a sprite falls back to a modulate-tinted placeholder
/// square so it stays visible. `Marker` items (cameras, empty `Node2D`s) draw
/// nothing: they are editor-only chrome, invisible at runtime.
pub fn rasterize_render_list(
    items: &[Render2DItem],
    width: u32,
    height: u32,
    clear: Color,
    mut texture_cache: Option<&mut TextureCache>,
) -> FrameBuffer {
    let mut fb = FrameBuffer::new(width.max(1), height.max(1), clear);
    let font = BitmapFont::builtin();
    for item in items {
        let xform = item.global_transform;
        match &item.kind {
            Render2DKind::Sprite {
                texture,
                offset,
                flip_h,
                flip_v,
            } => {
                let mut drawn = false;
                if let Some(cache) = texture_cache.as_deref_mut() {
                    if let Some(tex) = cache.get(texture) {
                        let (tw, th) = (tex.width as f32, tex.height as f32);
                        if tw > 0.0 && th > 0.0 {
                            // Sprite2D is centered by default: the texture's
                            // top-left sits at `offset - size/2` in node-local
                            // space. Flip mirrors the basis about that center.
                            let rect = Rect2::new(
                                Vector2::new(offset.x - tw / 2.0, offset.y - th / 2.0),
                                Vector2::new(tw, th),
                            );
                            let flip = Transform2D::scaled(Vector2::new(
                                if *flip_h { -1.0 } else { 1.0 },
                                if *flip_v { -1.0 } else { 1.0 },
                            ));
                            draw::draw_texture_rect_transformed(
                                &mut fb,
                                tex,
                                xform * flip,
                                rect,
                                None,
                                item.modulate,
                            );
                            drawn = true;
                        }
                    }
                }
                if !drawn {
                    // No texture available: a visible tinted placeholder square
                    // so the sprite's presence is still rendered.
                    let rect = Rect2::new(
                        Vector2::new(offset.x - 16.0, offset.y - 16.0),
                        Vector2::new(32.0, 32.0),
                    );
                    draw::fill_rect_transformed(&mut fb, xform, rect, item.modulate);
                }
            }
            Render2DKind::ColorRect { size, color } => {
                let rect = Rect2::new(Vector2::ZERO, *size);
                draw::fill_rect_transformed(&mut fb, xform, rect, modulate(*color, item.modulate));
            }
            Render2DKind::Text {
                text, font_size, ..
            } => {
                // Map Godot's font size onto the built-in bitmap font's integer
                // pixel scale (native cell height), at least 1.
                let scale = ((*font_size as f32 / font.char_height as f32).round() as u32).max(1);
                draw_string(&mut fb, &font, xform.origin, text, item.modulate, scale);
            }
            Render2DKind::Polygon { points, color } => {
                fill_polygon(&mut fb, xform, points, modulate(*color, item.modulate));
            }
            Render2DKind::Line {
                points,
                width: stroke,
                color,
            } => {
                let col = modulate(*color, item.modulate);
                for seg in points.windows(2) {
                    draw::draw_line_blended(
                        &mut fb,
                        xform.xform(seg[0]),
                        xform.xform(seg[1]),
                        col,
                        *stroke,
                    );
                }
            }
            Render2DKind::Marker { .. } => {}
        }
    }
    fb
}

/// Convenience: [`build_runtime_render_list`] + [`rasterize_render_list`] in one
/// call, for callers whose `tree` and `texture_cache` don't come from the same
/// borrow (tests, and the texture-less `render_runtime_frame` 2D branch).
pub fn render_runtime_canvas(
    tree: &SceneTree,
    width: u32,
    height: u32,
    clear: Color,
    texture_cache: Option<&mut TextureCache>,
) -> FrameBuffer {
    let items = build_runtime_render_list(tree);
    rasterize_render_list(&items, width, height, clear, texture_cache)
}

/// Component-wise multiply of a base color by a `modulate` tint.
fn modulate(base: Color, tint: Color) -> Color {
    Color::new(
        base.r * tint.r,
        base.g * tint.g,
        base.b * tint.b,
        base.a * tint.a,
    )
}

/// Scanline-fills a simple polygon (its `points` transformed by `xform` into
/// screen space) with `color`, using the even-odd rule. Handles convex and
/// simple concave polygons; degenerate (<3 point) or transparent fills are no-ops.
fn fill_polygon(fb: &mut FrameBuffer, xform: Transform2D, points: &[Vector2], color: Color) {
    if points.len() < 3 || color.a <= 0.001 {
        return;
    }
    let pts: Vec<Vector2> = points.iter().map(|p| xform.xform(*p)).collect();
    let (mut min_y, mut max_y) = (f32::MAX, f32::MIN);
    for p in &pts {
        min_y = min_y.min(p.y);
        max_y = max_y.max(p.y);
    }
    let y_start = (min_y.floor() as i32).max(0);
    let y_end = (max_y.ceil() as i32).min(fb.height as i32 - 1);
    let n = pts.len();
    for y in y_start..=y_end {
        let yc = y as f32 + 0.5;
        // Collect the x crossings of the scanline with each polygon edge.
        let mut xs: Vec<f32> = Vec::new();
        for i in 0..n {
            let a = pts[i];
            let b = pts[(i + 1) % n];
            let (lo, hi) = if a.y < b.y { (a, b) } else { (b, a) };
            if yc >= lo.y && yc < hi.y {
                let t = (yc - lo.y) / (hi.y - lo.y);
                xs.push(lo.x + t * (hi.x - lo.x));
            }
        }
        xs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        // Fill between successive crossing pairs (even-odd).
        let mut k = 0;
        while k + 1 < xs.len() {
            let xa = (xs[k].ceil() as i32).max(0);
            let xb = (xs[k + 1].floor() as i32).min(fb.width as i32 - 1);
            for x in xa..=xb {
                fb.blend_pixel(x as u32, y as u32, color);
            }
            k += 2;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::viewport_2d::Viewport2D;
    use gdscene::node::Node;
    use gdscene::node3d;

    fn find(items: &[Render2DItem], node_id: u64) -> Option<&Render2DItem> {
        items.iter().find(|i| i.node_id == node_id)
    }

    /// Acceptance (pat-1p53i.1): a `Sprite2D` with a texture renders that
    /// texture at the node's transform (not an abstract marker), honoring offset
    /// and flip; hidden and non-2D nodes are excluded; texture-less and other 2D
    /// nodes fall back to a marker.
    #[test]
    fn editor2d_renders_sprite2d_texture() {
        gdobject::class_db::register_2d_classes();
        gdobject::class_db::register_3d_classes();

        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let sprite = tree.add_child(root, Node::new("Hero", "Sprite2D")).unwrap();
        let bare = tree.add_child(root, Node::new("Empty", "Node2D")).unwrap();
        let untextured = tree
            .add_child(root, Node::new("Blank", "Sprite2D"))
            .unwrap();
        let hidden = tree
            .add_child(root, Node::new("Ghost", "Sprite2D"))
            .unwrap();
        // A non-2D (spatial) node must never appear in the 2D render list.
        let node3d_id = tree.add_child(root, Node::new("Spatial", "Node3D")).unwrap();

        // The hero sprite: textured, positioned, with an offset and a flip.
        node2d::set_position(&mut tree, sprite, Vector2::new(120.0, 80.0));
        node2d::set_texture_path(&mut tree, sprite, "res://hero.png");
        node2d::set_offset(&mut tree, sprite, Vector2::new(-16.0, -32.0));
        node2d::set_flip_h(&mut tree, sprite, true);

        node2d::set_texture_path(&mut tree, hidden, "res://ghost.png");
        node2d::set_visible(&mut tree, hidden, false);
        node3d::set_position(&mut tree, node3d_id, gdcore::math::Vector3::ZERO);

        let vp = Viewport2D::new(800, 600);
        let items = vp.build_render_list(&tree);
        let ids: Vec<u64> = items.iter().map(|i| i.node_id).collect();

        // Hidden and non-2D nodes are excluded.
        assert!(!ids.contains(&hidden.raw()), "hidden sprite is not drawn");
        assert!(!ids.contains(&node3d_id.raw()), "3D node is not drawn");

        // The textured sprite renders its texture at its global transform,
        // carrying the offset and flip.
        let hero = find(&items, sprite.raw()).expect("sprite item present");
        assert_eq!(hero.global_transform.origin, Vector2::new(120.0, 80.0));
        match &hero.kind {
            Render2DKind::Sprite {
                texture,
                offset,
                flip_h,
                flip_v,
            } => {
                assert_eq!(texture, "res://hero.png");
                assert_eq!(*offset, Vector2::new(-16.0, -32.0));
                assert!(*flip_h, "flip_h honored");
                assert!(!*flip_v, "flip_v defaults off");
            }
            other => panic!("expected a Sprite, got {other:?}"),
        }

        // A texture-less Sprite2D and a plain Node2D both fall back to markers.
        assert_eq!(
            find(&items, untextured.raw()).unwrap().kind,
            Render2DKind::Marker {
                class: "Sprite2D".to_string()
            }
        );
        assert_eq!(
            find(&items, bare.raw()).unwrap().kind,
            Render2DKind::Marker {
                class: "Node2D".to_string()
            }
        );
    }

    /// Sets a raw property on a node (test helper for shape data that has no
    /// dedicated typed setter).
    fn set_prop(tree: &mut SceneTree, id: gdscene::NodeId, key: &str, value: Variant) {
        if let Some(n) = tree.get_node_mut(id) {
            n.set_property(key, value);
        }
    }

    fn points_variant(points: &[Vector2]) -> Variant {
        Variant::Array(points.iter().map(|p| Variant::Vector2(*p)).collect())
    }

    /// Acceptance (pat-1p53i.2): `Polygon2D`, `Line2D`, and `ColorRect` render
    /// their real shape/vector visuals — polygon vertices + fill, polyline
    /// points + width + color, and a sized colored rectangle at its transform —
    /// rather than abstract markers.
    #[test]
    fn editor2d_renders_shape_nodes() {
        gdobject::class_db::register_2d_classes();

        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let poly = tree.add_child(root, Node::new("Poly", "Polygon2D")).unwrap();
        let line = tree.add_child(root, Node::new("Stroke", "Line2D")).unwrap();
        let rect = tree.add_child(root, Node::new("Bg", "ColorRect")).unwrap();

        // Polygon2D: a triangle with a red fill.
        let tri = [
            Vector2::new(0.0, 0.0),
            Vector2::new(10.0, 0.0),
            Vector2::new(0.0, 10.0),
        ];
        set_prop(&mut tree, poly, "polygon", points_variant(&tri));
        set_prop(&mut tree, poly, "color", Variant::Color(Color::new(1.0, 0.0, 0.0, 1.0)));

        // Line2D: a two-point green stroke of width 4.
        let pts = [Vector2::new(0.0, 0.0), Vector2::new(20.0, 5.0)];
        set_prop(&mut tree, line, "points", points_variant(&pts));
        set_prop(&mut tree, line, "width", Variant::Float(4.0));
        set_prop(
            &mut tree,
            line,
            "default_color",
            Variant::Color(Color::new(0.0, 1.0, 0.0, 1.0)),
        );

        // ColorRect: a 100x40 blue rectangle positioned at (50, 60).
        node2d::set_position(&mut tree, rect, Vector2::new(50.0, 60.0));
        control::set_size(&mut tree, rect, Vector2::new(100.0, 40.0));
        set_prop(&mut tree, rect, "color", Variant::Color(Color::new(0.0, 0.0, 1.0, 1.0)));

        let vp = Viewport2D::new(800, 600);
        let items = vp.build_render_list(&tree);

        // Polygon2D draws its vertices and fill color.
        match &find(&items, poly.raw()).expect("polygon item present").kind {
            Render2DKind::Polygon { points, color } => {
                assert_eq!(points.len(), 3);
                assert_eq!(points[1], Vector2::new(10.0, 0.0));
                assert_eq!(*color, Color::new(1.0, 0.0, 0.0, 1.0));
            }
            other => panic!("expected a Polygon, got {other:?}"),
        }

        // Line2D draws its points, width, and color.
        match &find(&items, line.raw()).expect("line item present").kind {
            Render2DKind::Line {
                points,
                width,
                color,
            } => {
                assert_eq!(points.len(), 2);
                assert_eq!(points[1], Vector2::new(20.0, 5.0));
                assert_eq!(*width, 4.0);
                assert_eq!(*color, Color::new(0.0, 1.0, 0.0, 1.0));
            }
            other => panic!("expected a Line, got {other:?}"),
        }

        // ColorRect draws a sized colored rect at its global transform.
        let rect_item = find(&items, rect.raw()).expect("color-rect item present");
        assert_eq!(rect_item.global_transform.origin, Vector2::new(50.0, 60.0));
        match &rect_item.kind {
            Render2DKind::ColorRect { size, color } => {
                assert_eq!(*size, Vector2::new(100.0, 40.0));
                assert_eq!(*color, Color::new(0.0, 0.0, 1.0, 1.0));
            }
            other => panic!("expected a ColorRect, got {other:?}"),
        }
    }

    /// Acceptance (pat-1p53i.3): the render list honors CanvasItem state —
    /// hidden nodes are skipped, each item carries its `modulate` tint, items are
    /// ordered by `z_index` (lower draws first), and a child's transform composes
    /// its parent's (CanvasItem transform inheritance).
    #[test]
    fn editor2d_honors_visibility_modulate_zindex() {
        gdobject::class_db::register_2d_classes();

        let mut tree = SceneTree::new();
        let root = tree.root_id();

        // Three sprites added back, front, mid — but z_index decides draw order.
        let back = tree.add_child(root, Node::new("Back", "Sprite2D")).unwrap();
        let front = tree.add_child(root, Node::new("Front", "Sprite2D")).unwrap();
        let mid = tree.add_child(root, Node::new("Mid", "Sprite2D")).unwrap();
        node2d::set_z_index(&mut tree, back, 0);
        node2d::set_z_index(&mut tree, front, 10);
        node2d::set_z_index(&mut tree, mid, 5);

        // Mid carries a half-transparent red modulate.
        let tint = Color::new(1.0, 0.0, 0.0, 0.5);
        node2d::set_modulate(&mut tree, mid, tint);

        // A hidden node must be skipped entirely.
        let ghost = tree.add_child(root, Node::new("Ghost", "Sprite2D")).unwrap();
        node2d::set_visible(&mut tree, ghost, false);

        // Child under a translated parent inherits the parent's transform.
        let parent = tree.add_child(root, Node::new("Parent", "Node2D")).unwrap();
        node2d::set_position(&mut tree, parent, Vector2::new(100.0, 0.0));
        let child = tree.add_child(parent, Node::new("Child", "Sprite2D")).unwrap();
        node2d::set_position(&mut tree, child, Vector2::new(10.0, 20.0));

        let vp = Viewport2D::new(800, 600);
        let items = vp.build_render_list(&tree);
        let ids: Vec<u64> = items.iter().map(|i| i.node_id).collect();

        // Hidden node excluded.
        assert!(!ids.contains(&ghost.raw()), "hidden node is skipped");

        // The three z-sprites are ordered back (0) -> mid (5) -> front (10).
        let z_order: Vec<u64> = ids
            .iter()
            .copied()
            .filter(|id| [back.raw(), mid.raw(), front.raw()].contains(id))
            .collect();
        assert_eq!(z_order, vec![back.raw(), mid.raw(), front.raw()]);

        // z_index is recorded on each item.
        assert_eq!(find(&items, front.raw()).unwrap().z_index, 10);
        assert_eq!(find(&items, back.raw()).unwrap().z_index, 0);

        // Modulate is recorded; an untinted node defaults to white.
        assert_eq!(find(&items, mid.raw()).unwrap().modulate, tint);
        assert_eq!(find(&items, back.raw()).unwrap().modulate, Color::WHITE);

        // The child's global transform composes the parent's: (100,0)+(10,20).
        assert_eq!(
            find(&items, child.raw()).unwrap().global_transform.origin,
            Vector2::new(110.0, 20.0)
        );
    }

    /// Acceptance (pat-1p53i.4): a `Label` renders its text at the node's
    /// transform, carrying the displayed string, font size, and horizontal
    /// alignment — not an abstract marker.
    #[test]
    fn editor2d_renders_label_text() {
        gdobject::class_db::register_2d_classes();

        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let label = tree.add_child(root, Node::new("Title", "Label")).unwrap();
        control::set_label_text(&mut tree, label, "Hello, Patina");
        control::set_font_size(&mut tree, label, 24);
        control::set_h_align(&mut tree, label, TextAlign::Center);
        node2d::set_position(&mut tree, label, Vector2::new(40.0, 50.0));

        let vp = Viewport2D::new(800, 600);
        let items = vp.build_render_list(&tree);

        let item = find(&items, label.raw()).expect("label item present");
        assert_eq!(item.global_transform.origin, Vector2::new(40.0, 50.0));
        match &item.kind {
            Render2DKind::Text {
                text,
                font_size,
                h_align,
            } => {
                assert_eq!(text, "Hello, Patina");
                assert_eq!(*font_size, 24);
                assert_eq!(*h_align, TextAlign::Center);
            }
            other => panic!("expected Text, got {other:?}"),
        }
    }
}
