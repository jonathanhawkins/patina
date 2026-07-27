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

use gdcore::math::{Color, Rect2, Transform2D, Vector2, Vector2i};
use gdrender2d::draw;
use gdrender2d::font::{draw_string, BitmapFont};
use gdrender2d::renderer::FrameBuffer;
use gdscene::control::TextAlign;
use gdscene::control_render::ControlDrawItem;
use gdscene::node::NodeId;
use gdscene::tilemap::{self, TILE_SET_PREFIX};
use gdscene::{canvas_order, control, node2d, SceneTree};
use gdscript_interop::canvas_server;
use gdvariant::Variant;

use crate::texture_cache::TextureCache;
use crate::viewport_2d::Viewport2D;

/// How a `Sprite2D` samples and places its texture (pat-0q2kx.107).
///
/// Godot's `Sprite2D` draws a *sub-rectangle* of its texture — an explicit
/// `region_rect`, or cell `frame` of an `hframes × vframes` sheet — positioned
/// by `centered`/`offset` (`scene/2d/sprite_2d.cpp` `_get_rects`). Patina had
/// that maths in `gdscene::sprite_frames::sprite_source_rect` with no renderer
/// calling it, so every sprite drew its whole texture, always centred.
///
/// These are the *inputs*, not a resolved rect: the sheet-cell calculation
/// needs the texture's pixel size, which is only known once the texture is
/// loaded at draw time. Resolving early would mean guessing it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpriteSheet {
    /// Godot `centered` — the texture's origin sits at its centre (default) or
    /// at its top-left.
    pub centered: bool,
    /// Godot `region_enabled`: sample `region_rect` instead of the sheet grid.
    pub region_enabled: bool,
    /// The explicit sub-rectangle used when `region_enabled`.
    pub region_rect: Rect2,
    /// Sheet columns (Godot `hframes`, default 1).
    pub hframes: i32,
    /// Sheet rows (Godot `vframes`, default 1).
    pub vframes: i32,
    /// Which cell of the sheet to draw (Godot `frame`).
    pub frame: i32,
}

impl Default for SpriteSheet {
    fn default() -> Self {
        Self {
            // Godot's Sprite2D is centred by default.
            centered: true,
            region_enabled: false,
            region_rect: Rect2::new(Vector2::ZERO, Vector2::ZERO),
            hframes: 1,
            vframes: 1,
            frame: 0,
        }
    }
}

/// Reads a `Sprite2D`'s sheet/region properties off the node (pat-0q2kx.107).
///
/// Every default matches Godot's, so a `.tscn` that serializes none of them —
/// the common case — samples the whole texture centred, exactly as before.
pub fn sprite_sheet_for(tree: &SceneTree, id: NodeId) -> SpriteSheet {
    match tree.get_node(id) {
        Some(node) => sprite_sheet_for_node(node),
        None => SpriteSheet::default(),
    }
}

/// [`sprite_sheet_for`] against a node directly, for callers that hold a `&Node`
/// rather than a tree id (the editor's edit-view draw).
pub fn sprite_sheet_for_node(node: &gdscene::node::Node) -> SpriteSheet {
    let d = SpriteSheet::default();
    let int = |key: &str, fallback: i32| -> i32 {
        match node.get_property(key) {
            Variant::Int(i) => i as i32,
            Variant::Float(f) => f as i32,
            _ => fallback,
        }
    };
    let flag = |key: &str, fallback: bool| -> bool {
        match node.get_property(key) {
            Variant::Bool(b) => b,
            _ => fallback,
        }
    };
    // Godot exposes the cell as either `frame` or `frame_coords` (a
    // column/row Vector2i); `frame_coords` wins when present, since that is the
    // one the inspector writes for a sheet.
    let hframes = int("hframes", d.hframes).max(1);
    let frame = match node.get_property("frame_coords") {
        Variant::Vector2i(c) => c.y * hframes + c.x,
        Variant::Vector2(c) => c.y as i32 * hframes + c.x as i32,
        _ => int("frame", d.frame),
    };
    SpriteSheet {
        centered: flag("centered", d.centered),
        region_enabled: flag("region_enabled", d.region_enabled),
        region_rect: match node.get_property("region_rect") {
            Variant::Rect2(r) => r,
            _ => d.region_rect,
        },
        hframes,
        vframes: int("vframes", d.vframes).max(1),
        frame,
    }
}

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
        /// Which part of the texture to sample, and how to place it
        /// (pat-0q2kx.107).
        sheet: SpriteSheet,
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
        /// `theme_override_fonts/font` resource path, or `None` for the default
        /// face (pat-0q2kx.146).
        font_path: Option<String>,
    },
    /// A `GPUParticles2D`/`CPUParticles2D` node's live particles: one textured
    /// (or solid) quad each, in the emitter's local space (pat-0q2kx.51).
    Particles {
        /// The emitter's `texture`, drawn per particle when it can be loaded.
        /// `None` — or a texture that fails to load — falls back to a solid quad.
        texture: Option<String>,
        /// The live particles, in the emitter's local space and in the node's
        /// `draw_order`.
        particles: Vec<ParticleQuad>,
    },
    /// A `TileMapLayer`/`TileMap`'s painted cells: one textured quad per used
    /// cell, in the layer's local space (pat-0q2kx.108).
    TileMap {
        /// The cells to draw, in a stable (row, column) order.
        cells: Vec<TileQuad>,
    },
    /// A position marker drawn for a 2D node with no own visual yet (cameras,
    /// empty `Node2D`s, and text nodes that later sub-tasks render) so it stays
    /// visible and pickable in the editor.
    Marker {
        /// The node's class name (drives the marker glyph).
        class: String,
    },
    /// A `MeshInstance2D`'s geometry as a textured triangle fan
    /// (pat-0q2kx.248).
    ///
    /// Only PRIMITIVE meshes reach here today. `packed_scene.rs` flattens a
    /// `mesh = SubResource(..)` into a `__viewport_mesh__` marker naming the
    /// primitive type, so a QuadMesh's corners can be generated without
    /// parsing anything; real `ArrayMesh` geometry needs vertex/uv/index
    /// flattening that does not exist yet, and such a node still falls through
    /// to `Marker`.
    Mesh {
        /// Texture resource path, empty when untextured.
        texture: String,
        /// Local-space corners, already in draw order.
        vertices: Vec<Vector2>,
        /// One UV per vertex, same order.
        uvs: Vec<Vector2>,
    },
}

/// One painted tile cell, resolved from `tile_map_data` plus the layer's
/// `TileSet` atlas sources (pat-0q2kx.108).
#[derive(Debug, Clone, PartialEq)]
pub struct TileQuad {
    /// The atlas texture this cell samples.
    pub texture: String,
    /// Where the tile lands, in the layer's local space.
    pub dest: Rect2,
    /// The sub-rectangle of `texture` the cell's atlas coordinates select.
    pub src: Rect2,
}

/// One live particle to draw, as published by the runtime particle phase
/// (`MainLoop::process_particles`) onto the emitter node.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ParticleQuad {
    /// Position in the emitter's local space (the item's global transform maps
    /// it to the canvas, exactly as for any other local-space geometry).
    pub position: Vector2,
    /// The particle's current color, interpolated over its lifetime.
    pub color: Color,
    /// Size multiplier for the drawn quad.
    pub scale: f32,
}

/// Edge length (px) of the solid quad drawn for a particle at scale 1 when the
/// emitter has no usable texture. Matches the runtime bridge's
/// `gdscene::scene_bridge` quad so both surfaces draw the same size.
const PARTICLE_QUAD_SIZE: f32 = 4.0;

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
    /// Screen-space rect this item's pixels are confined to, from the nearest
    /// ancestor with `clip_children` set (Godot's canvas-group clip). `None` means
    /// unclipped (pat-0q2kx.39).
    pub clip_rect: Option<Rect2>,
    /// The node's `CanvasItem.light_mask` (Godot default layer 1) — which 2D
    /// lights may reach it. A light applies only when this intersects the light's
    /// `range_item_cull_mask`, so it has to travel with the item rather than be
    /// assumed frame-wide (pat-0q2kx.130).
    pub light_mask: u32,
    /// The node's `CanvasItem.visibility_layer` (Godot default layer 1), ANDed
    /// against the viewport's `canvas_cull_mask` to decide whether the item is
    /// drawn at all (pat-0q2kx.254).
    ///
    /// ⚠️ Not the same thing as [`light_mask`](Self::light_mask), which they are
    /// easy to confuse: `light_mask` decides which *lights reach* an item that
    /// is already being drawn, `visibility_layer` decides whether it is drawn.
    /// This was inspector-settable long before it was read anywhere, so a scene
    /// could carry a considered value that changed nothing.
    pub visibility_layer: u32,
    /// How the `Camera2D` view applies to this item (pat-0q2kx.212).
    ///
    /// Resolved during collection because it needs the `CanvasLayer` ancestor's
    /// properties, which are gone by the time the view is applied.
    pub canvas_follow: CanvasFollow,
    /// What to draw for this node.
    pub kind: Render2DKind,
}

/// Whether the `Camera2D` view moves an item, and by how much (pat-0q2kx.212).
///
/// This replaces testing `canvas_layer != 0`, which
/// [`apply_camera2d_view`] used to do and which its own doc comment flagged as
/// wrong: a `CanvasLayer` with `layer` explicitly set to `0` was indistinguishable
/// from having no `CanvasLayer` ancestor, so it followed the camera when it should
/// have stayed put. `layer` is a **draw-order** number in `-128..128`; it was never
/// a statement about viewport following.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CanvasFollow {
    /// On the default canvas: the camera view applies in full.
    DefaultCanvas,
    /// On a `CanvasLayer` with `follow_viewport_enabled` off (Godot's default) —
    /// screen-fixed. This is what keeps a HUD still while the world scrolls.
    Fixed,
    /// On a `CanvasLayer` that follows the viewport, carrying its
    /// `follow_viewport_scale`. Godot re-parents the layer's canvas to the
    /// viewport's with that scale (`canvas_set_parent`), which is how a
    /// fractional value produces parallax.
    Follows(f32),
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
    // Draw order first, then the layer/z sort — the traversal order is what
    // survives as the tiebreaker, so Y-sort and `show_behind_parent` have to be
    // decided here rather than by scene-tree order (pat-0q2kx.39).
    let mut order: Vec<NodeId> = Vec::new();
    push_draw_order(tree, tree.root_id(), &mut order);

    let mut items: Vec<Render2DItem> = order
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
            // The CanvasLayer ancestor's transform and follow mode, resolved
            // once (pat-0q2kx.212).
            let (layer_xform, canvas_follow) = canvas_layer_placement(tree, id);
            Some(Render2DItem {
                node_id: id.raw(),
                // The layer transform pre-multiplies the node's own chain, which
                // is what Godot's separate-canvas-per-layer arrangement amounts
                // to. Identity for the default canvas, so nothing else moves.
                global_transform: layer_xform * node2d::get_global_transform(tree, id),
                // Godot's composite: inherited modulate (the chain's `modulate`,
                // including this node's own) times this node's `self_modulate`.
                // Reading only the node's own `modulate` meant a half-alpha parent
                // did nothing to its children.
                modulate: canvas_order::effective_modulate(tree, id),
                // Accumulated when `z_as_relative` (Godot's default), so a child
                // of a z=10 parent sorts above an absolute z=5 sibling.
                z_index: canvas_order::effective_z_index(tree, id) as i64,
                canvas_layer: canvas_layer_for(tree, id),
                clip_rect: clip_rect_for(tree, id),
                canvas_follow,
                // Godot's `CanvasItem.light_mask` default is layer 1.
                light_mask: match node.get_property("light_mask") {
                    Variant::Int(i) => i as u32,
                    _ => 1,
                },
                // `CanvasItem.visibility_layer`, same default of layer 1
                // (pat-0q2kx.254). Read here rather than at rasterize time
                // because the node is already in hand; the culling decision
                // needs the viewport's mask, which the raster owns.
                visibility_layer: match node.get_property("visibility_layer") {
                    Variant::Int(i) => i as u32,
                    _ => 1,
                },
                kind: render_kind_for(tree, id, node.class_name()),
            })
        })
        .collect();
    // -- script-created canvas items (pat-0q2kx.213) --
    //
    // `RenderingServer.canvas_item_*` items are parented to a node's canvas item
    // (`get_canvas_item()`), so they draw under that node's global transform,
    // layer, clip and light mask. Merging them by iterating the node items above
    // is what makes that automatic — and it also gives the right visibility
    // semantics for free: a node filtered out above for being hidden contributes
    // no script items either, exactly as hiding a Godot node hides the canvas
    // items attached to it.
    //
    // Appended before the sort so a script item's `z_index` participates in the
    // same ordering as node items rather than always landing on top.
    if canvas_server::has_any_node_items() {
        // Cloned because the loop pushes into `items` while reading the host
        // list; script items never host other script items, so one pass is enough.
        let hosts: Vec<Render2DItem> = items.clone();
        for host in &hosts {
            for ci in canvas_server::items_for_node(host.node_id) {
                let base = host.global_transform * ci.transform;
                for cmd in &ci.commands {
                    let (global_transform, kind) = canvas_command_render(base, cmd);
                    items.push(Render2DItem {
                        node_id: host.node_id,
                        global_transform,
                        // The item's own modulate composes with the host node's,
                        // the way a child canvas item's does in Godot.
                        modulate: Color::new(
                            host.modulate.r * ci.modulate.r,
                            host.modulate.g * ci.modulate.g,
                            host.modulate.b * ci.modulate.b,
                            host.modulate.a * ci.modulate.a,
                        ),
                        z_index: host.z_index + ci.z_index as i64,
                        canvas_layer: host.canvas_layer,
                        clip_rect: host.clip_rect,
                        light_mask: host.light_mask,
                        // A script-drawn item is part of its host node, so it
                        // is culled with the host rather than on its own.
                        visibility_layer: host.visibility_layer,
                        // Inherited from the host, like every other canvas
                        // property here: a script item drawn onto a node inside
                        // a HUD layer must stay pinned with that node
                        // (pat-0q2kx.212). The host's `global_transform` already
                        // carries the layer transform, so `base` above does too.
                        canvas_follow: host.canvas_follow,
                        kind,
                    });
                }
            }
        }
    }
    // Godot draws canvas layers in ascending `layer` order, and within a layer
    // by ascending `z_index`; a stable sort keeps the draw-order walk above as
    // the tiebreaker within the same layer+z.
    items.sort_by_key(|i| (i.canvas_layer, i.z_index));
    items
}

/// Maps one stored canvas command onto the render list's `(transform, kind)`.
///
/// `base` is the host node's global transform composed with the canvas item's
/// own, so each command only contributes its local placement.
///
/// ⚠️ `Circle` is emitted as a filled regular polygon: the rasterizer has no
/// circle primitive, and a 32-gon at typical canvas sizes is visually a circle.
/// That is an approximation rather than parity, and it is here rather than
/// silently dropping the command.
fn canvas_command_render(
    base: Transform2D,
    cmd: &canvas_server::CanvasCommand,
) -> (Transform2D, Render2DKind) {
    use canvas_server::CanvasCommand;
    match cmd {
        // ColorRect draws from the transform origin, so the rect's own position
        // rides in the transform.
        CanvasCommand::Rect { rect, color } => (
            base * Transform2D::translated(rect.position),
            Render2DKind::ColorRect {
                size: rect.size,
                color: *color,
            },
        ),
        CanvasCommand::Circle {
            center,
            radius,
            color,
        } => {
            const SEGMENTS: usize = 32;
            let points = (0..SEGMENTS)
                .map(|i| {
                    let a = (i as f32) * std::f32::consts::TAU / (SEGMENTS as f32);
                    Vector2::new(radius * a.cos(), radius * a.sin())
                })
                .collect();
            (
                base * Transform2D::translated(*center),
                Render2DKind::Polygon {
                    points,
                    color: *color,
                },
            )
        }
        CanvasCommand::Line {
            from,
            to,
            width,
            color,
        } => (
            base,
            Render2DKind::Line {
                points: vec![*from, *to],
                width: *width,
                color: *color,
            },
        ),
        CanvasCommand::Polygon { points, color } => (
            base,
            Render2DKind::Polygon {
                points: points.clone(),
                color: *color,
            },
        ),
    }
}

/// Appends `id` and its subtree to `out` in Godot's draw order.
///
/// Children come from [`canvas_order::draw_ordered_children`], so a `y_sort_enabled`
/// parent's children interleave by global Y rather than insertion order. A child
/// with `show_behind_parent` is emitted *before* its parent, which is exactly what
/// that flag means (`canvas_item.cpp`).
fn push_draw_order(tree: &SceneTree, id: NodeId, out: &mut Vec<NodeId>) {
    let children = canvas_order::draw_ordered_children(tree, id);
    let (behind, front): (Vec<NodeId>, Vec<NodeId>) = children
        .into_iter()
        .partition(|&child| read_bool(tree, child, "show_behind_parent"));
    for child in behind {
        push_draw_order(tree, child, out);
    }
    out.push(id);
    for child in front {
        push_draw_order(tree, child, out);
    }
}

/// The screen-space rect an item's pixels are confined to: the rect of its nearest
/// ancestor with `clip_children` set (pat-0q2kx.39).
///
/// The ancestor's extent comes from its `size` property — the Control/`ColorRect`
/// family — and the rect is axis-aligned at the ancestor's global position. A
/// clip parent whose extent is implied by a texture instead (a bare `Sprite2D`)
/// contributes no clip rather than a guessed one.
fn clip_rect_for(tree: &SceneTree, id: NodeId) -> Option<Rect2> {
    let mut cursor = tree.get_node(id).and_then(|n| n.parent());
    while let Some(parent) = cursor {
        if canvas_order::clip_children_mode(tree, parent) != 0 {
            let size = match tree.get_node(parent).map(|n| n.get_property("size")) {
                Some(Variant::Vector2(s)) if s.x > 0.0 && s.y > 0.0 => s,
                _ => return None,
            };
            return Some(Rect2::new(node2d::get_global_position(tree, parent), size));
        }
        cursor = tree.get_node(parent).and_then(|n| n.parent());
    }
    None
}

/// Returns the `layer` index of `id`'s nearest `CanvasLayer` ancestor, or `0`
/// when the node lives in the default canvas (no `CanvasLayer` above it).
///
/// Godot draws each `CanvasLayer` above the default canvas (whose implicit layer
/// is `0`), and a `CanvasLayer` node's `layer` property defaults to `1`, so a UI
/// layer naturally renders over the world.
fn canvas_layer_for(tree: &SceneTree, id: gdscene::NodeId) -> i64 {
    match canvas_layer_ancestor(tree, id) {
        Some(layer) => gdscene::canvas_layer::get_layer(tree, layer),
        None => 0,
    }
}

/// The nearest `CanvasLayer` ancestor of `id`, or `None` in the default canvas.
///
/// Returning the node rather than just its `layer` number is what lets one walk
/// answer all three questions the render list needs — draw order, the layer's own
/// transform, and whether it follows the viewport (pat-0q2kx.212). The number
/// alone could not distinguish "no CanvasLayer" from "a CanvasLayer with
/// `layer = 0`".
fn canvas_layer_ancestor(tree: &SceneTree, id: gdscene::NodeId) -> Option<gdscene::NodeId> {
    let mut current = tree.get_node(id).and_then(|n| n.parent());
    while let Some(pid) = current {
        let node = tree.get_node(pid)?;
        if node.is_class("CanvasLayer") {
            return Some(pid);
        }
        current = node.parent();
    }
    None
}

/// The layer transform to pre-multiply into `id`'s descendants, and how the
/// camera view should treat them (pat-0q2kx.212).
///
/// ⚠️ The transform has to be applied here rather than fall out of the parent
/// walk: `node2d::get_global_transform` composes each ancestor's *local*
/// transform, and a `CanvasLayer` is not a `CanvasItem` — it has `offset` where a
/// Node2D has `position` — so the walk passes straight through it and the layer
/// silently reads as identity. Authoring `offset` on a CanvasLayer moved nothing
/// at all before this.
fn canvas_layer_placement(tree: &SceneTree, id: gdscene::NodeId) -> (Transform2D, CanvasFollow) {
    let Some(layer) = canvas_layer_ancestor(tree, id) else {
        return (Transform2D::IDENTITY, CanvasFollow::DefaultCanvas);
    };
    // The composition itself lives next to the other CanvasLayer property
    // helpers in `gdscene`, where the `transform`-wins rule is documented.
    let xform = gdscene::canvas_layer::layer_transform(tree, layer);
    let follow = if gdscene::canvas_layer::get_follow_viewport_enabled(tree, layer) {
        CanvasFollow::Follows(gdscene::canvas_layer::get_follow_viewport_scale(tree, layer) as f32)
    } else {
        CanvasFollow::Fixed
    };
    (xform, follow)
}

/// Chooses the draw kind for a visible canvas node: a sprite for a textured
/// `Sprite2D`, a polygon/line/color-rect for the shape nodes, otherwise a
/// class-tagged marker.
/// Resolves a `TileMapLayer`'s painted cells into drawable quads.
///
/// Reads the node's `tile_map_data` (Godot's packed cell records) and the
/// `__tile_set__*` companions `packed_scene` wrote when it resolved
/// `tile_set = SubResource(...)`, then maps every cell's `(source_id,
/// atlas_coords)` to a texture sub-rectangle.
///
/// Placement follows `TileMapLayer::_rendering_update`: the tile's region is
/// centred on the cell centre, so a tile the size of a cell lands exactly on the
/// cell's top-left and an oversized one overhangs evenly.
///
/// Returns an empty vec when the layer is disabled, carries no tile set, or has
/// no cells — the caller then falls through to a marker.
pub fn tilemap_quads(tree: &SceneTree, id: gdscene::NodeId) -> Vec<TileQuad> {
    let Some(node) = tree.get_node(id) else {
        return Vec::new();
    };
    // Godot's `TileMapLayer.enabled` defaults to true; a disabled layer draws
    // nothing at all.
    if let Variant::Bool(false) = node.get_property("enabled") {
        return Vec::new();
    }
    let Variant::PackedByteArray(data) = node.get_property("tile_map_data") else {
        return Vec::new();
    };

    let source_ids =
        tilemap::tile_set_source_ids(&node.get_property(&format!("{}source_ids", TILE_SET_PREFIX)));
    let Some(tile_set) =
        tilemap::tile_set_from_properties(|key| node.get_property(key), &source_ids)
    else {
        return Vec::new();
    };

    let cells = tilemap::decode_tile_map_data(&data);
    // Sorted so the draw order (and any golden test) is stable rather than
    // following `HashMap` iteration.
    let mut coords: Vec<&Vector2i> = cells.keys().collect();
    coords.sort_by_key(|c| (c.y, c.x));

    let mut quads = Vec::with_capacity(coords.len());
    for coord in coords {
        let tile = &cells[coord];
        let Some((texture, src)) = tile_set.tile_texture_region(tile) else {
            continue;
        };
        let centre = tilemap::map_to_local(tile_set.tile_size, *coord);
        quads.push(TileQuad {
            texture: texture.to_string(),
            dest: Rect2::new(
                Vector2::new(centre.x - src.size.x / 2.0, centre.y - src.size.y / 2.0),
                src.size,
            ),
            src,
        });
    }
    quads
}

fn render_kind_for(tree: &SceneTree, id: gdscene::NodeId, class: &str) -> Render2DKind {
    match class {
        // An AnimatedSprite2D draws exactly like a Sprite2D — the difference is
        // only *which* texture, and the playback phase has already published the
        // current frame's onto the node (pat-0q2kx.100). Without this arm the
        // node fell through to `Marker` and animated content was invisible.
        // MeshInstance2D (pat-0q2kx.248). Registered since forever with no draw
        // semantics, so it was invisible at runtime.
        //
        // Handles the PRIMITIVE case only. `packed_scene.rs` already flattens
        // `mesh = SubResource(..)` into `__viewport_mesh__ = "primitive:<Type>"`,
        // and a QuadMesh's geometry is four corners — derivable with no `.tscn`
        // parsing at all. Real ArrayMesh geometry needs vertex/uv/index
        // flattening that does not exist yet (there is no `__mesh__` companion),
        // so those still fall through to `Marker` rather than drawing wrong.
        "MeshInstance2D" => {
            let primitive = tree
                .get_node(id)
                .map(|n| n.get_property("__viewport_mesh__"))
                .and_then(|v| match v {
                    Variant::String(s) => s.strip_prefix("primitive:").map(str::to_string),
                    _ => None,
                });
            // Godot's QuadMesh defaults to 1x1 centred on the origin.
            if matches!(primitive.as_deref(), Some("QuadMesh") | Some("PlaneMesh")) {
                let h = 0.5;
                return Render2DKind::Mesh {
                    texture: node2d::get_texture_path(tree, id).unwrap_or_default(),
                    vertices: vec![
                        Vector2::new(-h, -h),
                        Vector2::new(h, -h),
                        Vector2::new(h, h),
                        Vector2::new(-h, h),
                    ],
                    uvs: vec![
                        Vector2::new(0.0, 0.0),
                        Vector2::new(1.0, 0.0),
                        Vector2::new(1.0, 1.0),
                        Vector2::new(0.0, 1.0),
                    ],
                };
            }
        }
        "Sprite2D" | "AnimatedSprite2D" => {
            if let Some(texture) = node2d::get_texture_path(tree, id) {
                return Render2DKind::Sprite {
                    texture,
                    offset: read_vector2(tree, id, "offset"),
                    flip_h: read_bool(tree, id, "flip_h"),
                    flip_v: read_bool(tree, id, "flip_v"),
                    sheet: sprite_sheet_for(tree, id),
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
        // pat-0q2kx.77: a Path2D's curve is a Curve2D sub-resource, flattened
        // onto the node as `__curve__points` the same way a collision shape is
        // flattened to `__shape__*`. Godot draws the curve as a polyline in the
        // editor; with points it renders as one, and without them it falls
        // through to the marker below rather than drawing an empty line.
        "Path2D" => {
            let points = read_points(tree, id, "__curve__points");
            if !points.is_empty() {
                return Render2DKind::Line {
                    points,
                    width: read_f32(tree, id, "__curve__width", 2.0),
                    color: Color::new(1.0, 0.8, 0.2, 1.0),
                };
            }
        }
        // pat-0q2kx.108: a TileMapLayer's cells live in its `tile_map_data`
        // PackedByteArray, and the atlas regions they index come from the
        // `tile_set` sub-resource that `packed_scene` flattened onto the node
        // under `__tile_set__*`. Without this arm the node fell through to
        // `Marker` and a painted level was invisible.
        "TileMapLayer" | "TileMap" => {
            let cells = tilemap_quads(tree, id);
            if !cells.is_empty() {
                return Render2DKind::TileMap { cells };
            }
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
                font_path: gdscene::control_render::font_override_path(tree, id),
            };
        }
        "GPUParticles2D" | "CPUParticles2D" => {
            // Only while the scene is running: the particle phase republishes
            // `_particle_*` every frame, so an idle (not-yet-stepped) emitter has
            // no arrays and keeps its editor marker.
            let particles = read_particles(tree, id);
            if !particles.is_empty() {
                return Render2DKind::Particles {
                    texture: node2d::get_texture_path(tree, id),
                    particles,
                };
            }
        }
        _ => {}
    }
    Render2DKind::Marker {
        class: class.to_owned(),
    }
}

/// Reads the live particles the runtime published onto a particle emitter node.
///
/// `MainLoop::process_particles` writes three parallel internal arrays each
/// frame — `_particle_positions` (`PackedVector2Array`, emitter-local),
/// `_particle_colors`, `_particle_scales`. Positions drive the count; a shorter
/// (or absent) color/scale array falls back to white at scale 1 rather than
/// dropping the particle, so a partial publish still renders.
fn read_particles(tree: &SceneTree, id: gdscene::NodeId) -> Vec<ParticleQuad> {
    let Some(node) = tree.get_node(id) else {
        return Vec::new();
    };
    let positions = match node.get_property("_particle_positions") {
        Variant::PackedVector2Array(v) => v,
        _ => return Vec::new(),
    };
    let colors = match node.get_property("_particle_colors") {
        Variant::PackedColorArray(v) => v,
        _ => Vec::new(),
    };
    let scales = match node.get_property("_particle_scales") {
        Variant::PackedFloat32Array(v) => v,
        _ => Vec::new(),
    };
    positions
        .into_iter()
        .enumerate()
        .map(|(i, position)| ParticleQuad {
            position,
            color: colors.get(i).copied().unwrap_or(Color::WHITE),
            scale: match scales.get(i).copied() {
                Some(s) if s > 0.0 => s,
                _ => 1.0,
            },
        })
        .collect()
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
        // ⚠️ A `.tscn` writes `polygon = PackedVector2Array(...)` and the loader
        // parses it to the packed variant, NOT a generic Array — so only
        // hand-built nodes ever matched the arm below and every authored
        // Polygon2D/Line2D rendered empty (pat-0q2kx.77).
        Some(Variant::PackedVector2Array(items)) => items,
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
    //
    // ⚠️ "not nested inside another container" is load-bearing and was not
    // actually implemented (pat-0q2kx.39). `sort_container_tree` already
    // recurses into nested containers itself (`control.rs:2032`), handing each
    // child its *parent's allotted* size. Re-entering at a nested container
    // then arranged it a second time with the whole viewport `size`, throwing
    // away the size its parent had just given it — so a container inside a
    // container ended up laid out against the viewport instead of its own rect.
    // Entering only at the outermost containers leaves the recursion to do the
    // nested ones, which is what the comment above always claimed.
    let mut subtree = Vec::new();
    tree.collect_subtree_top_down(root, &mut subtree);
    for id in subtree {
        let is_container = tree
            .get_node(id)
            .map(|n| control::is_container_class_name(n.class_name()))
            .unwrap_or(false);
        if !is_container {
            continue;
        }
        // Skip containers whose parent chain already contains a container: the
        // outermost one's `sort_container_tree` reaches them with the right size.
        let nested = {
            let mut cursor = tree.get_node(id).and_then(|n| n.parent());
            let mut found = false;
            while let Some(pid) = cursor {
                if tree
                    .get_node(pid)
                    .map(|n| control::is_container_class_name(n.class_name()))
                    .unwrap_or(false)
                {
                    found = true;
                    break;
                }
                cursor = tree.get_node(pid).and_then(|n| n.parent());
            }
            found
        };
        if !nested {
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
                font_path,
                z,
                ..
            } => (
                Transform2D::translated(position),
                z,
                Render2DKind::Text {
                    text,
                    font_size: font_size as i64,
                    h_align: TextAlign::Left,
                    font_path,
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
                    // A Control's texture is placed by its own rect, not by
                    // Sprite2D's centred/sheet rules.
                    sheet: SpriteSheet {
                        centered: false,
                        ..SpriteSheet::default()
                    },
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
            // These primitives are emitted by the Control layout pass with
            // `canvas_layer: 0` already, i.e. the default canvas — so the camera
            // view applies to them as it always has (pat-0q2kx.212).
            canvas_follow: CanvasFollow::DefaultCanvas,
            // Control primitives are drawn by their own layout pass, which has
            // already resolved them inside their container.
            clip_rect: None,
            // Control primitives inherit the default lighting layer.
            light_mask: 1,
            // ...and the default visibility layer, so a viewport using the
            // default cull mask still draws the editor's own chrome.
            visibility_layer: 1,
            kind,
        });
    }
    out
}

/// Paint-order floor for control primitives, so a themed UI composites above the
/// plain 2D nodes of the same canvas layer (Godot scenes put UI on top).
pub const CONTROL_Z_BASE: i64 = 1 << 20;

/// The `Camera2D` driving the runtime canvas, or `None` if the scene enables none
/// (pat-0q2kx.27).
///
/// Godot's Viewport tracks a *current* camera: a node claims it through `enabled`
/// (Godot 4) or the legacy `current` property, and when several claim it the last
/// one in tree order wins (upstream/godot/scene/2d/camera_2d.cpp).
pub fn current_camera2d(tree: &SceneTree) -> Option<gdscene::NodeId> {
    let mut subtree = Vec::new();
    tree.collect_subtree_top_down(tree.root_id(), &mut subtree);
    let mut found = None;
    for id in subtree {
        let Some(node) = tree.get_node(id) else {
            continue;
        };
        if node.class_name() != "Camera2D" {
            continue;
        }
        let claims = match node.get_property("enabled") {
            Variant::Bool(b) => b,
            // Godot 3 named it `current`; scenes in the wild still carry it.
            _ => matches!(node.get_property("current"), Variant::Bool(true)),
        };
        if claims {
            found = Some(id);
        }
    }
    found
}

/// The canvas transform an enabled `Camera2D` imposes on the runtime frame —
/// world space to screen pixels (pat-0q2kx.27).
///
/// Godot's `Camera2D::_update_scroll` writes this to the viewport once per frame
/// rather than per node, so a camera centred on `C` with zoom `Z` places world
/// point `P` at `(P - C) * Z + viewport/2`: content centres on the camera and zoom
/// magnifies about that centre. Limits, drag margins and position smoothing all
/// come from [`node2d::compute_camera2d_position`], which already implements them
/// and until now had no runtime caller.
///
/// `previous_center` is last frame's camera centre, which smoothing and drag
/// margins integrate from — pass `None` to snap straight to the target, which is
/// also the correct result when smoothing is disabled (Godot's default).
pub fn camera2d_view_transform(
    tree: &SceneTree,
    viewport_size: Vector2,
    previous_center: Option<Vector2>,
    dt: f32,
) -> Option<Transform2D> {
    let camera = current_camera2d(tree)?;
    let config = node2d::camera2d_config_from_node(tree, camera);
    // The camera's own global position plus its `offset`, as Godot's target.
    let target =
        node2d::get_global_transform(tree, camera).origin + node2d::get_camera_offset(tree, camera);
    let current = previous_center.unwrap_or(target);
    let center = node2d::compute_camera2d_position(&config, current, target, viewport_size, dt);

    let half = Vector2::new(viewport_size.x * 0.5, viewport_size.y * 0.5);
    // Applied right-to-left: recentre on the camera, scale by zoom, then move to
    // the middle of the viewport (`A * B` applies B first).
    Some(
        Transform2D::translated(half)
            * Transform2D::scaled(config.zoom)
            * Transform2D::translated(-center),
    )
}

/// Pre-multiplies a camera view transform onto every **world-canvas** render item.
///
/// Godot applies the canvas transform once, at the viewport, rather than per node;
/// folding it into each item's global transform is the equivalent for a flat
/// render list, and it means the existing rasterizer needs no camera awareness —
/// it already draws rects and textures *through* the item transform, so zoom
/// scales sizes as well as positions.
///
/// Items on a `CanvasLayer` are skipped: Godot's canvas transform applies to the
/// default canvas, and a `CanvasLayer` is screen-fixed unless
/// `follow_viewport_enabled` is set (default **false**,
/// `doc/classes/CanvasLayer.xml`). That is what keeps a HUD still while the world
/// scrolls and zooms under it — previously the view was folded into every item,
/// so a health bar drifted off-screen as soon as the camera moved (pat-0q2kx.40).
///
/// The former KNOWN LIMITATION here — that a `CanvasLayer` with `layer` set to
/// `0` was indistinguishable from "no CanvasLayer ancestor" and so followed the
/// camera — is **fixed** (pat-0q2kx.212). The decision now rides on
/// [`CanvasFollow`], resolved during collection from the layer node itself, so
/// `layer` is back to meaning only draw order.
///
/// A layer with `follow_viewport_enabled` now follows, scaled by
/// `follow_viewport_scale`: Godot re-parents the layer's canvas to the viewport's
/// with that factor (`canvas_set_parent`), so `0.5` moves the layer half as far
/// as the world — the parallax idiom.
pub fn apply_camera2d_view(items: &mut [Render2DItem], view: Transform2D) {
    for item in items.iter_mut() {
        match item.canvas_follow {
            CanvasFollow::DefaultCanvas => {
                item.global_transform = view * item.global_transform;
            }
            CanvasFollow::Fixed => {}
            CanvasFollow::Follows(scale) => {
                // Scale the view's TRANSLATION, which is what makes a following
                // layer move a fraction of the world (pat-0q2kx.212).
                //
                // The previous form was `view * scaled * item`, which is inert:
                // post-multiplying by a scale rescales the basis it is applied
                // to, but leaves `view`'s own origin untouched, so a layer at
                // the origin moved the FULL camera distance no matter what
                // `follow_viewport_scale` said. A -100 camera put a 0.5-scaled
                // layer at -100 instead of -50, i.e. the property did nothing.
                //
                // KNOWN DIVERGENCE from Godot, deliberate and bounded: Godot
                // applies this as a scale about the VIEWPORT CENTRE across the
                // whole composed transform — `xfpivot * xfscale * xfpivot⁻¹ * xf`
                // in `servers/rendering/renderer_viewport.cpp`
                // `_canvas_get_transform` — so it also resizes the layer's
                // contents (the "pseudo-3D effect" the class docs describe). That
                // needs the viewport size, which this function is not given.
                // Scaling the translation alone reproduces the parallax idiom the
                // property is overwhelmingly used for and leaves contents at
                // their authored size; a faithful port needs `p_vp_size` threaded
                // in and belongs in its own bead.
                let followed = Transform2D {
                    x: view.x,
                    y: view.y,
                    origin: view.origin * scale,
                };
                item.global_transform = followed * item.global_transform;
            }
        }
    }
}

/// Applies the root window's content-scale ("stretch") transform to a render
/// list (pat-nzgdu.87).
///
/// ⚠️ Unlike [`apply_camera2d_view`], this applies to **every** item, including
/// those on a `CanvasLayer`. The camera moves the world under a fixed HUD; the
/// stretch transform maps the whole finished viewport onto the window, so a HUD
/// pinned to a `CanvasLayer` has to scale and shift with everything else.
/// Skipping canvas layers here would leave the HUD at 1:1 while the game around
/// it scaled — the classic "UI in the corner" bug.
///
/// Applied AFTER the camera view, since it is the outermost transform in
/// Godot's chain (window ← viewport ← canvas layer ← camera ← item).
pub fn apply_stretch_transform(items: &mut [Render2DItem], stretch: Transform2D) {
    if stretch == Transform2D::IDENTITY {
        return;
    }
    for item in items.iter_mut() {
        item.global_transform = stretch * item.global_transform;
    }
}

/// The camera centre [`camera2d_view_transform`] resolved, for a caller that keeps
/// per-frame smoothing state. `None` when no camera is enabled.
pub fn camera2d_center(
    tree: &SceneTree,
    viewport_size: Vector2,
    previous_center: Option<Vector2>,
    dt: f32,
) -> Option<Vector2> {
    let camera = current_camera2d(tree)?;
    let config = node2d::camera2d_config_from_node(tree, camera);
    let target =
        node2d::get_global_transform(tree, camera).origin + node2d::get_camera_offset(tree, camera);
    let current = previous_center.unwrap_or(target);
    Some(node2d::compute_camera2d_position(
        &config,
        current,
        target,
        viewport_size,
        dt,
    ))
}

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
/// The `Viewport` canvas-quality settings the software raster honours
/// (pat-0q2kx.254).
///
/// Godot ground truth: `Viewport.snap_2d_transforms_to_pixel`,
/// `snap_2d_vertices_to_pixel`, `canvas_cull_mask`, `msaa_2d`, `sdf_oversize`
/// and `sdf_scale` in `extension_api.json`; the snapping itself is
/// `servers/rendering/renderer_canvas_cull.cpp`.
///
/// [`Default`] is Godot's default viewport: no snapping, every layer visible,
/// no MSAA — so [`rasterize_render_list`] keeps its existing behaviour exactly.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CanvasQuality {
    /// Round each canvas item's **transform origin** to whole pixels.
    pub snap_2d_transforms_to_pixel: bool,
    /// Round each item's **vertices** to whole pixels after transforming.
    ///
    /// Godot keeps this separate from the transform flag: snapping the origin
    /// moves a sprite as a unit, snapping vertices also squares up a rotated or
    /// scaled shape's corners. In this raster the two coincide for axis-aligned
    /// primitives, which is why the acceptance exercises the transform flag.
    pub snap_2d_vertices_to_pixel: bool,
    /// Layers this viewport draws. An item is culled when
    /// `(item.visibility_layer & canvas_cull_mask) == 0`.
    ///
    /// Godot's default is all 32 layers (`0xFFFF_FFFF`), **not** layer 1 — a
    /// viewport shows everything until you narrow it.
    pub canvas_cull_mask: u32,
    /// Antialiasing **factor**: 1 (off), 2, 4 or 8.
    ///
    /// ⚠️ This is the multiplier, **not** Godot's `Viewport.MSAA` enum, which
    /// is an index: `MSAA_DISABLED=0, MSAA_2X=1, MSAA_4X=2, MSAA_8X=3`. Storing
    /// the enum here would make `msaa_2d = 2` mean 2× to a reader and 4× to
    /// Godot — a silent quality difference, never an error. The field carries
    /// `_factor` in its name for that reason; convert with
    /// [`msaa_factor_from_godot_enum`] at the settings boundary.
    pub msaa_2d_factor: u32,
    /// `Viewport.sdf_oversize` / `sdf_scale`, parsed and carried but **not yet
    /// consumed** — the canvas SDF is deferred to the GPUParticles2D-collision
    /// follow-up, per this bead's own scope note. Stored rather than dropped so
    /// the project setting round-trips instead of being silently discarded.
    pub sdf_oversize: f32,
    /// See [`sdf_oversize`](Self::sdf_oversize) — carried, not consumed.
    pub sdf_scale: f32,
}

impl Default for CanvasQuality {
    fn default() -> Self {
        Self {
            snap_2d_transforms_to_pixel: false,
            snap_2d_vertices_to_pixel: false,
            // Godot's default shows every layer.
            canvas_cull_mask: u32::MAX,
            msaa_2d_factor: 1,
            // Godot's defaults are the 1.0 entries of the SDF_OVERSIZE_* and
            // SDF_SCALE_* enums (oversize 120% -> 1.2, scale 50% -> 0.5).
            sdf_oversize: 1.2,
            sdf_scale: 0.5,
        }
    }
}

impl CanvasQuality {
    /// Build the Play viewport's quality from `project.godot` (pat-0q2kx.254).
    ///
    /// Keys read verbatim from `scene/main/scene_tree.cpp`, which is where
    /// Godot applies them to the root viewport:
    ///
    /// | setting | line | default |
    /// |---|---|---|
    /// | `rendering/2d/snap/snap_2d_transforms_to_pixel` | 2106 | `false` |
    /// | `rendering/2d/snap/snap_2d_vertices_to_pixel` | 2109 | `false` |
    /// | `rendering/anti_aliasing/quality/msaa_2d` | 2079 | `0` (disabled) |
    /// | `rendering/2d/sdf/oversize` | 2145 | `1` (120%) |
    /// | `rendering/2d/sdf/scale` | 2147 | `1` (50%) |
    ///
    /// ⚠️ `canvas_cull_mask` is deliberately **not** read here: it is a
    /// per-`Viewport` property with no project setting, so it keeps the
    /// all-layers default. Inventing a setting name for it would create a key
    /// that a real Godot project never writes.
    pub fn from_project_settings(settings: &gdcore::project_settings::ProjectSettings) -> Self {
        let flag = |key: &str| settings.get(key).and_then(|v| v.as_bool()).unwrap_or(false);
        let index = |key: &str, fallback: i64| {
            settings
                .get(key)
                .and_then(|v| v.as_int())
                .unwrap_or(fallback)
        };
        Self {
            snap_2d_transforms_to_pixel: flag("rendering/2d/snap/snap_2d_transforms_to_pixel"),
            snap_2d_vertices_to_pixel: flag("rendering/2d/snap/snap_2d_vertices_to_pixel"),
            canvas_cull_mask: u32::MAX,
            // ⚠️ The stored value is Godot's MSAA *enum index*, so it must go
            // through the conversion — reading it as a factor would make a
            // project asking for 4× render at 2×.
            msaa_2d_factor: msaa_factor_from_godot_enum(index(
                "rendering/anti_aliasing/quality/msaa_2d",
                0,
            )),
            sdf_oversize: sdf_oversize_from_godot_enum(index("rendering/2d/sdf/oversize", 1)),
            sdf_scale: sdf_scale_from_godot_enum(index("rendering/2d/sdf/scale", 1)),
        }
    }

    /// Whether `item` survives this viewport's cull mask.
    ///
    /// ⚠️ A mask of 0 hides everything, which is a legitimate setting and must
    /// not be "helpfully" treated as unset — Godot draws nothing for a zero
    /// cull mask, and silently substituting `u32::MAX` would make a deliberately
    /// blanked viewport render.
    pub fn shows(&self, item: &Render2DItem) -> bool {
        item.visibility_layer & self.canvas_cull_mask != 0
    }

    /// The supersample factor, clamped to the 1/2/4/8 Godot offers.
    ///
    /// Anything else rounds **down** to the nearest legal factor rather than up:
    /// an unexpected value should cost quality, never a 64× allocation.
    pub fn msaa_factor(&self) -> u32 {
        match self.msaa_2d_factor {
            0 | 1 => 1,
            2 | 3 => 2,
            4..=7 => 4,
            _ => 8,
        }
    }
}

/// `Viewport.MSAA` enum index -> supersample factor (pat-0q2kx.254).
///
/// `MSAA_DISABLED=0, MSAA_2X=1, MSAA_4X=2, MSAA_8X=3` (`extension_api.json`,
/// `Viewport.MSAA`). The project setting
/// `rendering/anti_aliasing/quality/msaa_2d` stores this index, and
/// `scene_tree.cpp:2079` casts it straight to the enum — so a raw 2 there means
/// **4×**, not 2×. Out-of-range values fall back to disabled rather than being
/// clamped upward: an unrecognised setting should cost quality, never a 64×
/// allocation.
pub fn msaa_factor_from_godot_enum(index: i64) -> u32 {
    match index {
        1 => 2,
        2 => 4,
        3 => 8,
        _ => 1,
    }
}

/// `Viewport.SDFOversize` index -> multiplier: 100%/120%/150%/200%.
/// Godot's project default is index 1 (`scene_tree.cpp:2145`).
pub fn sdf_oversize_from_godot_enum(index: i64) -> f32 {
    match index {
        0 => 1.0,
        2 => 1.5,
        3 => 2.0,
        _ => 1.2,
    }
}

/// `Viewport.SDFScale` index -> multiplier: 100%/50%/25%.
/// Godot's project default is index 1 (`scene_tree.cpp:2147`).
pub fn sdf_scale_from_godot_enum(index: i64) -> f32 {
    match index {
        0 => 1.0,
        2 => 0.25,
        _ => 0.5,
    }
}

/// Apply `snap_2d_*_to_pixel` to one item's transform (pat-0q2kx.254).
///
/// Godot rounds the canvas item's transform origin so a sprite authored at a
/// fractional position lands on whole pixels — the point of the setting is that
/// pixel art stops getting a half-covered blend column along its edge.
///
/// ⚠️ Uses `floor(v + 0.5)`, not `round`. Rust's `f32::round` breaks ties **away
/// from zero**, so `-10.5` would go to `-11` while `10.5` goes to `11` — the
/// rounding direction would flip either side of the origin, and an object
/// dragged left across x=0 would jitter by a pixel. Godot's `Math::round` has
/// the same away-from-zero behaviour, but its canvas snap goes through
/// `floor(x + 0.5)` in `renderer_canvas_cull.cpp`, which is uniform.
fn snap_item(item: &Render2DItem, quality: &CanvasQuality) -> Render2DItem {
    if !quality.snap_2d_transforms_to_pixel && !quality.snap_2d_vertices_to_pixel {
        return item.clone();
    }
    let mut out = item.clone();
    let o = out.global_transform.origin;
    out.global_transform.origin = Vector2::new((o.x + 0.5).floor(), (o.y + 0.5).floor());
    out
}

/// Scale a transform into supersampled space for an MSAA pass.
///
/// Scales the basis as well as the origin: scaling only the origin would move
/// each item to the right place at `factor`× and then draw it at its original
/// size, so everything would render tiny in a corner-anchored cluster.
fn scale_transform(t: Transform2D, factor: f32) -> Transform2D {
    Transform2D {
        x: Vector2::new(t.x.x * factor, t.x.y * factor),
        y: Vector2::new(t.y.x * factor, t.y.y * factor),
        origin: Vector2::new(t.origin.x * factor, t.origin.y * factor),
    }
}

/// Box-downsample a supersampled frame back to its final size (pat-0q2kx.254).
///
/// Averages each `factor`×`factor` block. This is what turns a hard-edged
/// diagonal drawn at 4× into the intermediate-alpha pixels the acceptance looks
/// for — the aliased original has only fully-in and fully-out pixels.
///
/// Alpha is averaged with the colour rather than used to weight it: the source
/// pixels come from a framebuffer that has already blended everything down onto
/// its clear colour, so they are final composite values, not premultiplied
/// contributions waiting to be composited again.
fn box_downsample(src: &FrameBuffer, width: u32, height: u32, factor: u32) -> FrameBuffer {
    let mut out = FrameBuffer::new(width, height, Color::new(0.0, 0.0, 0.0, 0.0));
    let n = (factor * factor) as f32;
    for y in 0..height {
        for x in 0..width {
            let (mut r, mut g, mut b, mut a) = (0.0f32, 0.0f32, 0.0f32, 0.0f32);
            for sy in 0..factor {
                for sx in 0..factor {
                    let p = src.get_pixel(x * factor + sx, y * factor + sy);
                    r += p.r;
                    g += p.g;
                    b += p.b;
                    a += p.a;
                }
            }
            out.set_pixel(x, y, Color::new(r / n, g / n, b / n, a / n));
        }
    }
    out
}

/// Rasterize with Godot's default viewport quality (no snapping, no culling,
/// no MSAA). Unchanged behaviour for the 22 existing call sites.
pub fn rasterize_render_list(
    items: &[Render2DItem],
    width: u32,
    height: u32,
    clear: Color,
    texture_cache: Option<&mut TextureCache>,
) -> FrameBuffer {
    rasterize_render_list_with_quality(
        items,
        width,
        height,
        clear,
        texture_cache,
        &CanvasQuality::default(),
    )
}

/// Rasterize honouring the viewport's canvas-quality settings (pat-0q2kx.254).
///
/// MSAA is a supersample: the whole frame is rasterized at `factor`× in each
/// axis and box-downsampled. That is not what Godot's hardware `msaa_2d` does
/// internally, but it is the same *observable* result for a software raster —
/// a diagonal edge gains intermediate-alpha pixels — and it needs no
/// per-primitive coverage maths, which this rasterizer has nowhere to put.
///
/// ⚠️ Snapping is applied **before** the supersample scale, not after. Rounding
/// in supersampled space would snap to a quarter-pixel of the final image,
/// which is not snapping at all — the flag exists to put edges exactly on final
/// pixel boundaries.
pub fn rasterize_render_list_with_quality(
    items: &[Render2DItem],
    width: u32,
    height: u32,
    clear: Color,
    mut texture_cache: Option<&mut TextureCache>,
    quality: &CanvasQuality,
) -> FrameBuffer {
    let factor = quality.msaa_factor();
    if factor > 1 {
        // Snap first, at final-image scale, then scale the already-snapped
        // transforms up for the supersampled pass.
        let prepared: Vec<Render2DItem> = items
            .iter()
            .filter(|i| quality.shows(i))
            .map(|i| {
                let mut it = snap_item(i, quality);
                it.global_transform = scale_transform(it.global_transform, factor as f32);
                it
            })
            .collect();
        // The inner pass must not cull or snap again: culling is already done,
        // and re-snapping would round to supersampled pixels.
        let big = rasterize_render_list_with_quality(
            &prepared,
            width.max(1) * factor,
            height.max(1) * factor,
            clear,
            texture_cache,
            &CanvasQuality::default(),
        );
        return box_downsample(&big, width.max(1), height.max(1), factor);
    }

    let culled: Vec<Render2DItem> = items
        .iter()
        .filter(|i| quality.shows(i))
        .map(|i| snap_item(i, quality))
        .collect();
    let items = &culled[..];

    let mut fb = FrameBuffer::new(width.max(1), height.max(1), clear);
    let font = BitmapFont::builtin();
    for item in items {
        let xform = item.global_transform;
        // `clip_children` (pat-0q2kx.39): the software rasterizer has no scissor to
        // set, so a clipped item draws normally and everything it wrote outside its
        // clip rect is restored afterwards. Only clipped subtrees pay the copy.
        let clip_snapshot = item.clip_rect.map(|_| fb.pixels.clone());
        match &item.kind {
            Render2DKind::Sprite {
                texture,
                offset,
                flip_h,
                flip_v,
                sheet,
            } => {
                let mut drawn = false;
                if let Some(cache) = texture_cache.as_deref_mut() {
                    if let Some(tex) = cache.get(texture) {
                        let (tw, th) = (tex.width as f32, tex.height as f32);
                        if tw > 0.0 && th > 0.0 {
                            // Which pixels of the texture this sprite samples:
                            // an explicit region, or cell `frame` of the
                            // hframes x vframes sheet. The whole texture when
                            // neither is configured (pat-0q2kx.107).
                            let src = gdscene::sprite_frames::sprite_source_rect(
                                Vector2::new(tw, th),
                                sheet.region_enabled,
                                sheet.region_rect,
                                sheet.hframes,
                                sheet.vframes,
                                sheet.frame,
                            );
                            // The drawn size is the SOURCE size, not the whole
                            // texture: a 4-cell sheet draws one cell.
                            let (dw, dh) = (src.size.x, src.size.y);
                            // `centered` (Godot's default) puts the sub-rect's
                            // centre at the node origin; otherwise its top-left
                            // sits there. Flip mirrors the basis about that point.
                            let rect = if sheet.centered {
                                Rect2::new(
                                    Vector2::new(offset.x - dw / 2.0, offset.y - dh / 2.0),
                                    Vector2::new(dw, dh),
                                )
                            } else {
                                Rect2::new(*offset, Vector2::new(dw, dh))
                            };
                            let flip = Transform2D::scaled(Vector2::new(
                                if *flip_h { -1.0 } else { 1.0 },
                                if *flip_v { -1.0 } else { 1.0 },
                            ));
                            draw::draw_texture_rect_transformed(
                                &mut fb,
                                tex,
                                xform * flip,
                                rect,
                                Some(src),
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
            // pat-0q2kx.108: one textured quad per painted cell. A cell whose
            // atlas texture is missing from the cache is skipped rather than
            // drawn as a placeholder — a level is hundreds of cells, and
            // stand-in squares would bury the tiles that did load.
            Render2DKind::TileMap { cells } => {
                if let Some(cache) = texture_cache.as_deref_mut() {
                    for cell in cells {
                        let Some(tex) = cache.get(&cell.texture) else {
                            continue;
                        };
                        draw::draw_texture_rect_transformed(
                            &mut fb,
                            tex,
                            xform,
                            cell.dest,
                            Some(cell.src),
                            item.modulate,
                        );
                    }
                }
            }
            Render2DKind::ColorRect { size, color } => {
                let rect = Rect2::new(Vector2::ZERO, *size);
                draw::fill_rect_transformed(&mut fb, xform, rect, modulate(*color, item.modulate));
            }
            Render2DKind::Text {
                text,
                font_size,
                font_path,
                ..
            } => {
                // A `theme_override_fonts/font` draws through the real outline
                // rasterizer at the authored pixel size; everything else keeps
                // the built-in bitmap font, so no unrelated editor text changes
                // appearance (pat-0q2kx.146).
                let drawn = font_path.as_deref().is_some_and(|path| {
                    with_override_font(path, |ttf, atlas| {
                        gdrender2d::ttf_font::draw_ttf_text_with_fallbacks(
                            &mut fb,
                            atlas,
                            ttf,
                            &[],
                            text,
                            xform.origin,
                            *font_size as f32,
                            item.modulate,
                        );
                    })
                });
                if !drawn {
                    // Map Godot's font size onto the built-in bitmap font's integer
                    // pixel scale (native cell height), at least 1.
                    let scale =
                        ((*font_size as f32 / font.char_height as f32).round() as u32).max(1);
                    draw_string(&mut fb, &font, xform.origin, text, item.modulate, scale);
                }
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
            Render2DKind::Particles { texture, particles } => {
                // Godot draws one billboard per live particle each frame,
                // textured by the emitter's `texture`
                // (`scene/2d/gpu_particles_2d.cpp`). Each quad is centered on the
                // particle and sized by its `scale`; the emitter's transform is
                // the item transform, so a `local_coords` emitter carries its
                // cloud while a global-space one does not (the published
                // positions already compensate — see
                // `ParticleSimulator::draw_commands_in_local_space`).
                let mut textured = false;
                if let Some(path) = texture.as_ref() {
                    if let Some(cache) = texture_cache.as_deref_mut() {
                        if let Some(tex) = cache.get(path) {
                            let (tw, th) = (tex.width as f32, tex.height as f32);
                            if tw > 0.0 && th > 0.0 {
                                for quad in particles {
                                    let rect = centered_quad(
                                        quad.position,
                                        tw * quad.scale,
                                        th * quad.scale,
                                    );
                                    draw::draw_texture_rect_transformed(
                                        &mut fb,
                                        tex,
                                        xform,
                                        rect,
                                        None,
                                        modulate(quad.color, item.modulate),
                                    );
                                }
                                textured = true;
                            }
                        }
                    }
                }
                if !textured {
                    // No texture (or it failed to load): Godot's own untextured
                    // emitter still shows a point per particle, so fall back to a
                    // solid quad rather than drawing nothing.
                    for quad in particles {
                        let side = PARTICLE_QUAD_SIZE * quad.scale;
                        let rect = centered_quad(quad.position, side, side);
                        draw::fill_rect_transformed(
                            &mut fb,
                            xform,
                            rect,
                            modulate(quad.color, item.modulate),
                        );
                    }
                }
            }
            // pat-0q2kx.248. `fill_polygon_textured` has existed in
            // gdrender2d since forever with ZERO non-test callers — the
            // rasterizer did not need writing, only wiring. Vertices are
            // transformed here the same way the local `fill_polygon` does it,
            // because the gdrender2d entry point takes screen-space points.
            Render2DKind::Mesh {
                texture,
                vertices,
                uvs,
            } => {
                if vertices.len() >= 3 && uvs.len() == vertices.len() {
                    let pts: Vec<Vector2> = vertices.iter().map(|v| xform.xform(*v)).collect();
                    if let Some(cache) = texture_cache.as_deref_mut() {
                        if let Some(tex) = cache.get(texture) {
                            gdrender2d::polygon_raster::fill_polygon_textured(
                                &mut fb, &pts, uvs, tex,
                            );
                        }
                    }
                }
            }
            Render2DKind::Marker { .. } => {}
        }
        if let (Some(rect), Some(before)) = (item.clip_rect, clip_snapshot) {
            restore_outside_clip(&mut fb, rect, &before);
        }
    }
    fb
}

/// A `w` × `h` rect centered on `center`, in the same local space as `center` —
/// the billboard footprint of one particle.
fn centered_quad(center: Vector2, w: f32, h: f32) -> Rect2 {
    Rect2::new(
        Vector2::new(center.x - w / 2.0, center.y - h / 2.0),
        Vector2::new(w, h),
    )
}

/// Puts back every pixel outside `rect`, undoing the part of the last draw that
/// escaped its `clip_children` ancestor.
fn restore_outside_clip(fb: &mut FrameBuffer, rect: Rect2, before: &[Color]) {
    let (x0, y0) = (rect.position.x, rect.position.y);
    let (x1, y1) = (x0 + rect.size.x, y0 + rect.size.y);
    for y in 0..fb.height {
        for x in 0..fb.width {
            let (px, py) = (x as f32, y as f32);
            let inside = px >= x0 && px < x1 && py >= y0 && py < y1;
            if inside {
                continue;
            }
            let index = (y * fb.width + x) as usize;
            if let (Some(slot), Some(&prev)) = (fb.pixels.get_mut(index), before.get(index)) {
                *slot = prev;
            }
        }
    }
}

/// Applies the 2D light / occluder pass and `CanvasModulate` to an already
/// rasterized frame (pat-0q2kx.103).
///
/// Godot shades the canvas after drawing it: `PointLight2D` / `DirectionalLight2D`
/// contribute per-pixel light, `LightOccluder2D` polygons cast shadow into it,
/// and a `CanvasModulate` multiplies the whole frame. Patina had all of that
/// maths (`gdrender2d::light2d`) and the scene collectors
/// (`gdscene::light2d_scene`) with **no production caller** — the Play canvas
/// never shaded, so a lit scene rendered exactly like an unlit one.
///
/// ⚠️ The light pass is gated on the scene actually containing a light. With no
/// lights, `light_factor_at` returns the ambient term alone — a dim 0.15 grey —
/// so running it unconditionally would darken every unlit scene to a sixth of
/// its brightness. "No lights" has to mean "no shading", not "ambient only".
pub fn shade_runtime_canvas(tree: &SceneTree, fb: &mut FrameBuffer) {
    let lights = gdscene::light2d_scene::collect_lights_2d(tree);
    if !lights.is_empty() {
        let occluders = gdscene::light2d_scene::collect_occluders_2d(tree);
        // Godot culls lights per canvas item, comparing the *item's* `light_mask`
        // against the light's `range_item_cull_mask`, so the mask varies across
        // the frame (pat-0q2kx.130). The buffer starts at layer 1 — the default
        // a fresh CanvasItem is on, and what this pass used to assume for the
        // whole frame — and each item stamps its own mask over its own pixels.
        let masks = item_light_mask_buffer(tree, fb.width, fb.height);
        gdrender2d::light2d::apply_lights_with_item_masks(
            fb,
            &masks,
            &lights,
            &occluders,
            gdrender2d::light2d::CanvasAmbient::default(),
        );
    }
    // CanvasModulate tints the finished frame whether or not anything is lit.
    if let Some(color) = gdscene::light2d_scene::collect_canvas_modulate(tree) {
        gdscene::light2d_scene::apply_canvas_modulate(fb, color);
    }
}

/// Builds a per-pixel `light_mask` buffer for the frame (pat-0q2kx.130).
///
/// Each canvas item's own `light_mask` is stamped over the screen-space bounds of
/// what it draws, in draw order, so a later item wins where they overlap — the
/// same precedence the colour buffer already has. Pixels no item covers keep the
/// default layer 1.
///
/// The bounds are the item's **axis-aligned extent**, not its exact coverage: a
/// rotated sprite's mask therefore spills into its corners. That is a deliberate
/// bound on scope — it is exact for the axis-aligned rects and sprites that make
/// up almost every 2D scene, and per-pixel coverage would mean threading a mask
/// through every `draw::*` call in the rasterizer.
fn item_light_mask_buffer(tree: &SceneTree, width: u32, height: u32) -> Vec<u32> {
    let mut masks = vec![1u32; (width as usize) * (height as usize)];
    for item in collect_canvas_items(tree) {
        let mask = item.light_mask;
        let Some(bounds) = item_screen_bounds(&item) else {
            continue;
        };
        let x0 = bounds.position.x.floor().max(0.0) as u32;
        let y0 = bounds.position.y.floor().max(0.0) as u32;
        let x1 = ((bounds.position.x + bounds.size.x).ceil().max(0.0) as u32).min(width);
        let y1 = ((bounds.position.y + bounds.size.y).ceil().max(0.0) as u32).min(height);
        for y in y0..y1 {
            for x in x0..x1 {
                masks[(y * width + x) as usize] = mask;
            }
        }
    }
    masks
}

/// The screen-space axis-aligned bounds of what `item` draws, or `None` when the
/// kind has no bounded extent this pass can derive.
fn item_screen_bounds(item: &Render2DItem) -> Option<Rect2> {
    let local = match &item.kind {
        Render2DKind::ColorRect { size, .. } => Rect2::new(Vector2::ZERO, *size),
        Render2DKind::TileMap { cells } => {
            let first = cells.first()?;
            let mut r = first.dest;
            for cell in &cells[1..] {
                let (min_x, min_y) = (
                    r.position.x.min(cell.dest.position.x),
                    r.position.y.min(cell.dest.position.y),
                );
                let (max_x, max_y) = (
                    (r.position.x + r.size.x).max(cell.dest.position.x + cell.dest.size.x),
                    (r.position.y + r.size.y).max(cell.dest.position.y + cell.dest.size.y),
                );
                r = Rect2::new(
                    Vector2::new(min_x, min_y),
                    Vector2::new(max_x - min_x, max_y - min_y),
                );
            }
            r
        }
        Render2DKind::Polygon { points, .. } | Render2DKind::Line { points, .. } => {
            let first = points.first()?;
            let (mut min, mut max) = (*first, *first);
            for p in &points[1..] {
                min = Vector2::new(min.x.min(p.x), min.y.min(p.y));
                max = Vector2::new(max.x.max(p.x), max.y.max(p.y));
            }
            Rect2::new(min, Vector2::new(max.x - min.x, max.y - min.y))
        }
        // Sprites need their texture's size to bound them, which this pass does
        // not load; markers and text have no lit extent worth masking.
        _ => return None,
    };
    // Transform the four corners and take their axis-aligned extent, so a
    // rotated or scaled item still bounds correctly.
    let xf = item.global_transform;
    let corners = [
        xf.xform(local.position),
        xf.xform(Vector2::new(
            local.position.x + local.size.x,
            local.position.y,
        )),
        xf.xform(Vector2::new(
            local.position.x + local.size.x,
            local.position.y + local.size.y,
        )),
        xf.xform(Vector2::new(
            local.position.x,
            local.position.y + local.size.y,
        )),
    ];
    let mut min = corners[0];
    let mut max = corners[0];
    for c in &corners[1..] {
        min = Vector2::new(min.x.min(c.x), min.y.min(c.y));
        max = Vector2::new(max.x.max(c.x), max.y.max(c.y));
    }
    Some(Rect2::new(min, Vector2::new(max.x - min.x, max.y - min.y)))
}

/// Convenience: [`build_runtime_render_list`] + [`rasterize_render_list`] in one
/// call, for callers whose `tree` and `texture_cache` don't come from the same
/// borrow (tests, and the texture-less `render_runtime_frame` 2D branch).
///
/// Also runs [`shade_runtime_canvas`], so the Play frame this produces is the
/// *shaded* one — the light pass is part of rendering a canvas, not an extra
/// step each caller has to remember (pat-0q2kx.103).
pub fn render_runtime_canvas(
    tree: &SceneTree,
    width: u32,
    height: u32,
    clear: Color,
    texture_cache: Option<&mut TextureCache>,
) -> FrameBuffer {
    let items = build_runtime_render_list(tree);
    let mut fb = rasterize_render_list(&items, width, height, clear, texture_cache);
    shade_runtime_canvas(tree, &mut fb);
    fb
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
        let node3d_id = tree
            .add_child(root, Node::new("Spatial", "Node3D"))
            .unwrap();

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
                ..
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

        let poly = tree
            .add_child(root, Node::new("Poly", "Polygon2D"))
            .unwrap();
        let line = tree.add_child(root, Node::new("Stroke", "Line2D")).unwrap();
        let rect = tree.add_child(root, Node::new("Bg", "ColorRect")).unwrap();

        // Polygon2D: a triangle with a red fill.
        let tri = [
            Vector2::new(0.0, 0.0),
            Vector2::new(10.0, 0.0),
            Vector2::new(0.0, 10.0),
        ];
        set_prop(&mut tree, poly, "polygon", points_variant(&tri));
        set_prop(
            &mut tree,
            poly,
            "color",
            Variant::Color(Color::new(1.0, 0.0, 0.0, 1.0)),
        );

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
        set_prop(
            &mut tree,
            rect,
            "color",
            Variant::Color(Color::new(0.0, 0.0, 1.0, 1.0)),
        );

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
        let front = tree
            .add_child(root, Node::new("Front", "Sprite2D"))
            .unwrap();
        let mid = tree.add_child(root, Node::new("Mid", "Sprite2D")).unwrap();
        node2d::set_z_index(&mut tree, back, 0);
        node2d::set_z_index(&mut tree, front, 10);
        node2d::set_z_index(&mut tree, mid, 5);

        // Mid carries a half-transparent red modulate.
        let tint = Color::new(1.0, 0.0, 0.0, 0.5);
        node2d::set_modulate(&mut tree, mid, tint);

        // A hidden node must be skipped entirely.
        let ghost = tree
            .add_child(root, Node::new("Ghost", "Sprite2D"))
            .unwrap();
        node2d::set_visible(&mut tree, ghost, false);

        // Child under a translated parent inherits the parent's transform.
        let parent = tree.add_child(root, Node::new("Parent", "Node2D")).unwrap();
        node2d::set_position(&mut tree, parent, Vector2::new(100.0, 0.0));
        let child = tree
            .add_child(parent, Node::new("Child", "Sprite2D"))
            .unwrap();
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
                ..
            } => {
                assert_eq!(text, "Hello, Patina");
                assert_eq!(*font_size, 24);
                assert_eq!(*h_align, TextAlign::Center);
            }
            other => panic!("expected Text, got {other:?}"),
        }
    }
}

/// Runs `f` with the outline font at `path` and a shared glyph atlas, returning
/// whether the font was available (pat-0q2kx.146).
///
/// Cached per process and keyed by path: a Label redraws every frame, and
/// re-parsing a `.ttf` per frame would dominate the render. A path that fails to
/// load caches its failure too, so a scene pointing at a missing font degrades
/// to the built-in face once rather than retrying the read every frame.
fn with_override_font<
    F: FnOnce(&gdrender2d::ttf_font::TtfFont, &mut gdrender2d::ttf_font::GlyphAtlas),
>(
    path: &str,
    f: F,
) -> bool {
    use std::cell::RefCell;
    use std::collections::HashMap;
    thread_local! {
        static FONTS: RefCell<HashMap<String, Option<gdrender2d::ttf_font::TtfFont>>> =
            RefCell::new(HashMap::new());
        static ATLAS: RefCell<gdrender2d::ttf_font::GlyphAtlas> =
            RefCell::new(gdrender2d::ttf_font::GlyphAtlas::new(512, 512));
    }
    FONTS.with(|cache| {
        let mut cache = cache.borrow_mut();
        let entry = cache.entry(path.to_string()).or_insert_with(|| {
            // `res://` is the project-relative spelling; strip it so the path
            // resolves against the working directory like every other asset read.
            let disk = path.strip_prefix("res://").unwrap_or(path);
            std::fs::read(disk)
                .ok()
                .and_then(|bytes| gdrender2d::ttf_font::TtfFont::from_bytes(bytes).ok())
        });
        match entry {
            Some(ttf) => {
                ATLAS.with(|atlas| f(ttf, &mut atlas.borrow_mut()));
                true
            }
            None => false,
        }
    })
}
