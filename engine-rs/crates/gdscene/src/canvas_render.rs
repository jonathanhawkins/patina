//! The runtime 2D canvas builder: scene tree -> draw list -> framebuffer.
//!
//! Each visible `CanvasItem` becomes a [`Render2DItem`], which the rasterizer
//! turns into pixels. This is Godot's `NOTIFICATION_DRAW` ->
//! `RenderingServer::canvas_item_*` -> `renderer_canvas_cull` path, and in
//! Godot it is the SAME path in the editor, an exported game and a web export.
//!
//! 🔑 This module used to live in `gdeditor::viewport_2d_render`, which made
//! the editor the only thing in the engine able to draw a 2D scene: the editor
//! binary and `gdeditor::scene_renderer` were its only callers,
//! `patina-runner` emitted tree JSON and nothing else, and `patina-web`
//! depended on neither `gdeditor` nor `gdrender2d` and only cleared its canvas.
//! A 2D game run anywhere but the editor's Play tab drew no `Sprite2D`,
//! `TileMapLayer` or `Line2D` pixels at all (pat-mgixk.28).
//!
//! It lives in `gdscene` rather than `gdrender2d` because it needs both: the
//! `SceneTree` it walks and the `FrameBuffer`/`Texture2D` it draws into.
//! `gdrender2d` sits BELOW `gdscene` and cannot see a scene tree.
//!
//! ⚠️ Editor CHROME stays in `gdeditor`. `Viewport2D::build_render_list` and
//! `navigation_overlay_items` compose this module's output with the editor's
//! overlays; nothing here knows about selection handles, gizmos or navmesh
//! debug draw. `gdeditor::viewport_2d_render` re-exports everything below, so
//! every existing editor call site and test keeps working unchanged.
//!
//! Original module documentation follows.
//!
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

use crate::control::TextAlign;
use crate::control_render::ControlDrawItem;
use crate::node::NodeId;
use crate::tilemap::{self, TILE_SET_PREFIX};
use crate::{canvas_order, control, node2d, SceneTree};
use gdcore::file_access::FileAccess;
use gdcore::math::{Color, Rect2, Transform2D, Vector2, Vector2i};
use gdrender2d::draw;
use gdrender2d::renderer::FrameBuffer;
use gdscript_interop::canvas_server;
use gdserver2d::material::{
    BlendMode, CanvasItemMaterial, LightMode, TextureFilter, TextureRepeat,
};
use gdserver2d::shader::{
    parse_fragment_body, FragmentInstruction, Shader, ShaderMaterial, ShaderType,
};
use gdvariant::Variant;

use crate::texture_cache::TextureCache;

/// How a `Sprite2D` samples and places its texture (pat-0q2kx.107).
///
/// Godot's `Sprite2D` draws a *sub-rectangle* of its texture — an explicit
/// `region_rect`, or cell `frame` of an `hframes × vframes` sheet — positioned
/// by `centered`/`offset` (`scene/2d/sprite_2d.cpp` `_get_rects`). Patina had
/// that maths in `crate::sprite_frames::sprite_source_rect` with no renderer
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
pub fn sprite_sheet_for_node(node: &crate::node::Node) -> SpriteSheet {
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
    // Godot exposes the cell as either `frame` or `frame_coords` (a column/row
    // Vector2i). They are two VIEWS OF ONE VALUE — setting either updates the
    // other — so a `.tscn` serialises at most one of them.
    //
    // ⚠️ PRESENCE IS NOT THE DISCRIMINATOR, however much it looks like it.
    // `packed_scene::node_with_class_defaults` seeds every ClassDB default onto
    // the node before the scene's own values override a subset, so as soon as
    // `register_2d_classes()` has run — which the editor and the runner both do
    // at startup — `frame_coords` is ALWAYS present, as `Vector2i(0, 0)`.
    // Letting mere presence win meant that seeded default silently beat an
    // authored `frame`, pinning every sprite sheet to cell 0 in any real
    // project. It read as "sheets are ignored" rather than as a precedence bug,
    // and it hid until a test registered the ClassDB (pat-s3rrv.22).
    //
    // A NON-ZERO cell, by contrast, cannot have come from the default, so it is
    // the one signal that `frame_coords` was actually authored. When both are
    // zero the two agree anyway, so nothing is lost by preferring `frame`.
    let hframes = int("hframes", d.hframes).max(1);
    let authored_coords = match node.get_property("frame_coords") {
        Variant::Vector2i(c) => Some((c.x, c.y)),
        Variant::Vector2(c) => Some((c.x as i32, c.y as i32)),
        _ => None,
    };
    let frame = match authored_coords {
        Some((x, y)) if x != 0 || y != 0 => y * hframes + x,
        _ => int("frame", d.frame),
    };
    // An `AtlasTexture` assigned to `texture` is a sub-rect of a sheet, which is
    // the same thing `region_enabled`/`region_rect` already expresses — so it
    // drives that path rather than getting a parallel one (pat-s3rrv.117).
    // `packed_scene` redirected the texture to the sheet and parked the rect in
    // `__atlas_region`.
    //
    // ⚠️ The node's own `region_enabled` wins when the scene set it. Godot keeps
    // the two independent: a Sprite2D can region a texture that is *itself* an
    // AtlasTexture, and there the node's rect is the one in sprite coordinates.
    let atlas_region = match node.get_property("__atlas_region") {
        Variant::Rect2(r) if r.size.x > 0.0 && r.size.y > 0.0 => Some(r),
        _ => None,
    };
    let node_region_enabled = flag("region_enabled", d.region_enabled);

    SpriteSheet {
        centered: flag("centered", d.centered),
        region_enabled: node_region_enabled || atlas_region.is_some(),
        region_rect: match node.get_property("region_rect") {
            Variant::Rect2(r) if node_region_enabled => r,
            _ => atlas_region.unwrap_or(d.region_rect),
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
    /// A `Line2D` whose stroke varies along its length — a `gradient`, a
    /// `width_curve`, or a non-default cap/joint mode.
    ///
    /// Kept separate from [`Render2DKind::Line`] so the plain uniform stroke
    /// (and every `_draw`-command line, which can never carry these) keeps its
    /// existing shape and rasterizer path untouched.
    LineShaded {
        /// The polyline points, in node-local space.
        points: Vec<Vector2>,
        /// Per-vertex width: `width` scaled by the baked `width_curve`.
        widths: Vec<f32>,
        /// Per-vertex color: `default_color` modulated by the `gradient`.
        colors: Vec<Color>,
        /// Whether the stroke loops back to its first point.
        closed: bool,
        /// `begin_cap_mode`/`end_cap_mode` resolved to a round cap.
        round_cap: bool,
        /// `joint_mode` resolved to a round joint.
        round_joint: bool,
    },
    /// A `Polygon2D` filled through a texture sampled by per-vertex UVs.
    PolygonTextured {
        /// The polygon vertices, in node-local space.
        points: Vec<Vector2>,
        /// One UV per vertex.
        uvs: Vec<Vector2>,
        /// Resource path of the fill texture.
        texture: String,
        /// The node's `color`, drawn when the texture cannot be loaded so a
        /// textured polygon never renders as nothing.
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
    ///
    /// `QuadMesh` and `PlaneMesh` produce DIFFERENT sizes here — 1x1 and 2x2
    /// respectively, matching Godot's defaults (pat-s3rrv.198). They were one
    /// hardcoded 1x1 case until that bead.
    Mesh {
        /// Texture resource path, empty when untextured.
        texture: String,
        /// Local-space corners, already in draw order.
        vertices: Vec<Vector2>,
        /// One UV per vertex, same order.
        uvs: Vec<Vector2>,
    },
    /// A `BackBufferCopy`: snapshots the frame so far into the back buffer a
    /// canvas_item shader samples as `SCREEN_TEXTURE` (pat-s3rrv.183).
    ///
    /// Draws nothing itself. It is a draw-list item purely so the snapshot
    /// happens at its ORDER position — everything below it is in the copy,
    /// everything above is not, which is the whole point of the node.
    BackBufferCopy {
        /// Godot's `copy_mode`: 0 disabled, 1 rect, 2 whole viewport.
        /// ⚠️ Defaults to 1 (RECT), not 0 — `doc/classes/BackBufferCopy.xml`.
        copy_mode: i64,
        /// The region copied in `COPY_MODE_RECT`, in the node's local space.
        rect: Rect2,
    },
    /// A `MultiMeshInstance2D`: one mesh drawn once per instance transform
    /// (pat-s3rrv.183).
    ///
    /// The geometry is shared and the per-instance data travels with it, so the
    /// whole node is still a single draw-list item rather than N items — which
    /// is the point of a MultiMesh.
    MultiMesh {
        /// Texture resource path, empty when untextured.
        texture: String,
        /// Local-space corners of the instanced mesh, in draw order.
        vertices: Vec<Vector2>,
        /// One UV per vertex, same order.
        uvs: Vec<Vector2>,
        /// Per-instance transform and colour. The colour is white when the
        /// authored buffer carries no per-instance colours.
        instances: Vec<(Transform2D, Color)>,
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
    /// The cell's alternative-tile orientation, as `tilemap::TILE_FLIP_H` |
    /// `TILE_FLIP_V` | `TILE_TRANSPOSE` bits (0 = drawn as authored).
    pub flags: i64,
}

impl TileQuad {
    /// The texture-space → layer-space transform that places this cell's
    /// region in `dest` with its orientation, about the cell's centre.
    ///
    /// Godot's `TileMapLayer::draw_tile` negates the destination size per
    /// flipped axis and passes `transpose` to `draw_texture_rect_region`,
    /// which swaps the UV axes. Inverting that sampling (both steps are their
    /// own inverse) gives the drawing transform `F · T`: `T` swaps x/y when
    /// transposed, `F` mirrors the flipped axes.
    pub fn orientation(&self) -> Transform2D {
        let fx = if self.flags & crate::tilemap::TILE_FLIP_H != 0 { -1.0 } else { 1.0 };
        let fy = if self.flags & crate::tilemap::TILE_FLIP_V != 0 { -1.0 } else { 1.0 };
        let (x, y) = if self.flags & crate::tilemap::TILE_TRANSPOSE != 0 {
            (Vector2::new(0.0, fy), Vector2::new(fx, 0.0))
        } else {
            (Vector2::new(fx, 0.0), Vector2::new(0.0, fy))
        };
        let c = Vector2::new(
            self.dest.position.x + self.dest.size.x / 2.0,
            self.dest.position.y + self.dest.size.y / 2.0,
        );
        // translate(c) · M · translate(-c)
        let m = Transform2D { x, y, origin: Vector2::ZERO };
        let to_origin = Transform2D::translated(Vector2::new(-c.x, -c.y));
        Transform2D::translated(c) * m * to_origin
    }
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
/// emitter has no usable texture.
const PARTICLE_QUAD_SIZE: f32 = 4.0;

/// Safety bound on parallax repeat copies per axis (pat-s3rrv.86).
///
/// Godot bounds the equivalent tiling through culling; this renderer emits the
/// copies eagerly, so a stride far smaller than the viewport would ask for an
/// unbounded number of them.
const MAX_PARALLAX_REPEATS_PER_AXIS: i64 = 64;

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
    /// The node's `CanvasItemMaterial`, rebuilt from the `__canvas_material__*`
    /// companions `packed_scene` flattens at load (pat-s3rrv.28).
    ///
    /// `None` — the overwhelmingly common case — composites with plain `Mix`.
    /// Resolved during collection rather than at raster time because the
    /// rasterizer has no `SceneTree` to read the companions back off.
    pub canvas_material: Option<CanvasItemMaterial>,
    /// The node's canvas_item `ShaderMaterial`, rebuilt from the `shader_code` /
    /// `shader_path` + `shader_parameter/*` properties `packed_scene` leaves on
    /// the node (pat-s3rrv.28).
    ///
    /// `Some` routes the item's textured draws through the shaded blit, so an
    /// authored `.gdshader` rewrites `COLOR` in the running frame instead of
    /// being loaded and ignored.
    pub shader_material: Option<ShaderMaterial>,
    /// `CanvasItem.texture_filter`, **already resolved** up the parent chain.
    ///
    /// Godot's `FILTER_PARENT_NODE` (0) is an inheritance marker, not a
    /// sampling mode, and the chain terminates at the project default. It is
    /// walked here — where the ancestors are still reachable — so what reaches
    /// the blit is always a concrete mode.
    pub texture_filter: TextureFilter,
    /// `CanvasItem.texture_repeat`, resolved up the parent chain the same way.
    pub texture_repeat: TextureRepeat,
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

/// Walks `tree` in scene order and returns a [`Render2DItem`] for every
/// **visible** `CanvasItem` (the Node2D and Control families), ordered
/// back-to-front by `(canvas_layer, z_index)` with scene-tree order as the
/// stable tiebreaker. Hidden and non-canvas (spatial) nodes are skipped.
///
/// Shared by the editor viewport's render list
/// (`gdeditor`'s `Viewport2D::build_render_list`, which adds its overlays on
/// top) and the runtime canvas rasterizer ([`build_runtime_render_list`]), so
/// both draw the same nodes in the same order.
///
/// ⚠️ The editor reference is plain backticks, not an intra-doc link: that type
/// lives in `gdeditor`, which depends on this crate and not the reverse, so a
/// link would be unresolvable from here.
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
                // pat-s3rrv.28: the material and sampling state travel WITH
                // the item, because the rasterizer has no `SceneTree`. The
                // filter/repeat walks resolve Godot's PARENT_NODE inheritance
                // here, where the ancestors are still in hand.
                canvas_material: resolve_canvas_item_material(tree, id),
                shader_material: resolve_canvas_shader_material(tree, id),
                texture_filter: resolve_texture_filter(tree, id),
                texture_repeat: resolve_texture_repeat(tree, id),
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
                        // A `RenderingServer.canvas_item_*` item is parented to
                        // the host node's canvas item, so it composites and
                        // samples under the host's material exactly as a child
                        // canvas item does in Godot.
                        canvas_material: host.canvas_material,
                        shader_material: host.shader_material.clone(),
                        texture_filter: host.texture_filter,
                        texture_repeat: host.texture_repeat,
                        kind,
                    });
                }
            }
        }
    }
    // A node's own `_draw()` output (pat-s3rrv.22). Godot appends `_draw`
    // commands to the SAME CanvasItem command list as the item's built-in
    // drawing, which is why a `_draw` on a Sprite2D overlays the sprite rather
    // than replacing it — so these are pushed AFTER the class-driven items.
    //
    // Until now the only consumer of `custom_draw_commands` was
    // a canvas bridge module with no production caller (since removed): on the
    // shipped path `RenderingServer.canvas_item_add_rect` drew but a script's
    // `_draw` did not.
    let hosts: Vec<Render2DItem> = items.clone();
    for host in &hosts {
        // `Render2DItem::node_id` is the raw u64; the store is keyed by NodeId.
        let node = NodeId::from_object_id(gdcore::id::ObjectId::from_raw(host.node_id));
        let cmds = tree.custom_draw_commands(node);
        if cmds.is_empty() {
            continue;
        }
        for cmd in cmds {
            // A multiline/dashed command is a SET of disjoint segments, which no
            // single polyline can express without drawing the connecting runs;
            // each pair becomes its own Line item.
            for (xf, kind) in draw_command_items(host.global_transform, cmd) {
                items.push(Render2DItem {
                    node_id: host.node_id,
                    global_transform: xf,
                    modulate: host.modulate,
                    z_index: host.z_index,
                    canvas_layer: host.canvas_layer,
                    clip_rect: host.clip_rect,
                    light_mask: host.light_mask,
                    visibility_layer: host.visibility_layer,
                    canvas_follow: host.canvas_follow,
                    // `_draw` output goes on the SAME CanvasItem command list as
                    // the node's built-in drawing, so it carries the node's
                    // material and sampling modes too.
                    canvas_material: host.canvas_material,
                    shader_material: host.shader_material.clone(),
                    texture_filter: host.texture_filter,
                    texture_repeat: host.texture_repeat,
                    kind,
                });
            }
        }
    }
    // Godot draws canvas layers in ascending `layer` order, and within a layer
    // by ascending `z_index`; a stable sort keeps the draw-order walk above as
    // the tiebreaker within the same layer+z.
    items.sort_by_key(|i| (i.canvas_layer, i.z_index));
    items
}

/// Expands one `_draw` command into the render items it needs (pat-s3rrv.22).
///
/// Most commands are one item; `draw_multiline` and `draw_dashed_line` are sets
/// of disjoint segments and become one `Line` item each, because a single
/// polyline would draw the gaps too.
fn draw_command_items(
    base: Transform2D,
    cmd: &gdserver2d::canvas::DrawCommand,
) -> Vec<(Transform2D, Render2DKind)> {
    use gdserver2d::canvas::DrawCommand as Dc;
    match cmd {
        Dc::DrawMultiline {
            points,
            color,
            width,
        } => points
            .chunks_exact(2)
            .map(|seg| {
                (
                    base,
                    Render2DKind::Line {
                        points: vec![seg[0], seg[1]],
                        width: *width,
                        color: *color,
                    },
                )
            })
            .collect(),
        Dc::DrawDashedLine {
            from,
            to,
            color,
            width,
            dash,
        } => {
            let delta = Vector2::new(to.x - from.x, to.y - from.y);
            let len = (delta.x * delta.x + delta.y * delta.y).sqrt();
            let step = dash.max(0.1);
            if len <= f32::EPSILON {
                return Vec::new();
            }
            let dir = Vector2::new(delta.x / len, delta.y / len);
            let mut out = Vec::new();
            let mut t = 0.0f32;
            // Godot alternates dash / gap of the same length.
            while t < len {
                let end = (t + step).min(len);
                out.push((
                    base,
                    Render2DKind::Line {
                        points: vec![
                            Vector2::new(from.x + dir.x * t, from.y + dir.y * t),
                            Vector2::new(from.x + dir.x * end, from.y + dir.y * end),
                        ],
                        width: *width,
                        color: *color,
                    },
                ));
                t += step * 2.0;
            }
            out
        }
        other => draw_command_render(base, other).into_iter().collect(),
    }
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

/// Converts one GDScript `_draw()` command into a render item (pat-s3rrv.22).
///
/// `CanvasItem.draw_*` calls land in `SceneTree::custom_draw_commands` as
/// `gdserver2d::canvas::DrawCommand`s. Until this existed the ONLY consumer of
/// that store was a canvas bridge module with no production caller (since removed) —
/// so `RenderingServer.canvas_item_add_rect` drew on the shipped path while a
/// script's `_draw` did not, and the test that "proved" `_draw` asserted pixels
/// through the dead bridge.
///
/// Follows the same convention as [`canvas_command_render`]: geometry that
/// `Render2DKind` carries in local space stays in the kind, and geometry that
/// the kind cannot express (a rect's position, a circle's centre) rides in the
/// returned transform.
///
/// Returns `None` for a command the render list has no representation for, so
/// an unsupported draw call is skipped rather than mis-drawn.
fn draw_command_render(
    base: Transform2D,
    cmd: &gdserver2d::canvas::DrawCommand,
) -> Option<(Transform2D, Render2DKind)> {
    use gdserver2d::canvas::DrawCommand as Dc;

    /// Godot strokes an unfilled rect as its four edges.
    fn rect_outline(rect: &gdcore::math::Rect2) -> Vec<Vector2> {
        let (w, h) = (rect.size.x, rect.size.y);
        vec![
            Vector2::new(0.0, 0.0),
            Vector2::new(w, 0.0),
            Vector2::new(w, h),
            Vector2::new(0.0, h),
            Vector2::new(0.0, 0.0),
        ]
    }

    /// Samples a circular arc into a polyline, matching the circle
    /// approximation `canvas_command_render` already uses.
    fn arc_points(radius: f32, start: f32, end: f32, count: usize) -> Vec<Vector2> {
        let count = count.max(2);
        (0..=count)
            .map(|i| {
                let t = i as f32 / count as f32;
                let a = start + (end - start) * t;
                Vector2::new(radius * a.cos(), radius * a.sin())
            })
            .collect()
    }

    match cmd {
        Dc::DrawRect {
            rect,
            color,
            filled,
        } => Some(if *filled {
            (
                base * Transform2D::translated(rect.position),
                Render2DKind::ColorRect {
                    size: rect.size,
                    color: *color,
                },
            )
        } else {
            (
                base * Transform2D::translated(rect.position),
                Render2DKind::Line {
                    points: rect_outline(rect),
                    // `draw_rect`'s `width` is dropped upstream (the DrawCommand
                    // carries none), so a 1px outline is the honest default.
                    width: 1.0,
                    color: *color,
                },
            )
        }),
        Dc::DrawCircle {
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
            Some((
                base * Transform2D::translated(*center),
                Render2DKind::Polygon {
                    points,
                    color: *color,
                },
            ))
        }
        Dc::DrawLine {
            from,
            to,
            color,
            width,
        } => Some((
            base,
            Render2DKind::Line {
                points: vec![*from, *to],
                width: *width,
                color: *color,
            },
        )),
        Dc::DrawPolyline {
            points,
            color,
            width,
            closed,
        } => {
            let mut pts = points.clone();
            if *closed {
                if let Some(first) = points.first() {
                    pts.push(*first);
                }
            }
            Some((
                base,
                Render2DKind::Line {
                    points: pts,
                    width: *width,
                    color: *color,
                },
            ))
        }
        // Independent segments cannot be one polyline without drawing the
        // connecting runs, so each pair is emitted by the caller instead.
        Dc::DrawMultiline { .. } => None,
        Dc::DrawDashedLine { .. } => None,
        // A 9-slice needs per-corner/edge stretching that `Render2DKind` cannot
        // express — drawing it as a plain stretched Sprite would silently
        // distort the corners, so it is skipped until the render list grows a
        // nine-patch kind. Matched explicitly rather than with a wildcard so a
        // future `DrawCommand` variant fails this match instead of vanishing.
        Dc::DrawNinePatch { .. } => None,
        Dc::DrawArc {
            center,
            radius,
            start_angle,
            end_angle,
            point_count,
            color,
            width,
        } => Some((
            base * Transform2D::translated(*center),
            Render2DKind::Line {
                points: arc_points(*radius, *start_angle, *end_angle, *point_count as usize),
                width: *width,
                color: *color,
            },
        )),
        Dc::DrawPolygon { points, colors } => Some((
            base,
            Render2DKind::Polygon {
                points: points.clone(),
                // The rasterizer has no per-vertex interpolation, so the first
                // colour fills — the same choice `gdserver2d` documents.
                color: colors.first().copied().unwrap_or(Color::WHITE),
            },
        )),
        Dc::DrawTextureRect {
            texture_path,
            rect,
            modulate: _,
        } => Some((
            base * Transform2D::translated(rect.position),
            Render2DKind::Sprite {
                texture: texture_path.clone(),
                offset: Vector2::ZERO,
                flip_h: false,
                flip_v: false,
                sheet: SpriteSheet::default(),
            },
        )),
        Dc::DrawTextureRegion {
            texture_path,
            rect,
            source_rect: _,
            modulate: _,
        } => Some((
            base * Transform2D::translated(rect.position),
            Render2DKind::Sprite {
                texture: texture_path.clone(),
                offset: Vector2::ZERO,
                flip_h: false,
                flip_v: false,
                sheet: SpriteSheet::default(),
            },
        )),
        Dc::DrawString {
            text,
            position,
            color: _,
            font_size,
        } => Some((
            base * Transform2D::translated(*position),
            Render2DKind::Text {
                text: text.clone(),
                font_size: *font_size as i64,
                h_align: TextAlign::Left,
                font_path: None,
            },
        )),
    }
}

/// Appends `id` and its subtree to `out` in Godot's draw order.
///
/// Children come from [`canvas_order::draw_ordered_children`], so a `y_sort_enabled`
/// parent's children interleave by global Y rather than insertion order. A child
/// with `show_behind_parent` is emitted *before* its parent, which is exactly what
/// that flag means (`canvas_item.cpp`).
fn push_draw_order(tree: &SceneTree, id: NodeId, out: &mut Vec<NodeId>) {
    // ⚠️ A `y_sort_enabled` node draws its whole reachable subtree as ONE sorted
    // group (pat-s3rrv.22): Godot flattens every descendant reached through
    // non-y-sorted children and sorts them together, so a grandchild can
    // interleave between its own parent and that parent's siblings. Sorting only
    // direct children moves each child's subtree as an indivisible block.
    //
    // The members of a flattened group are emitted here and MUST NOT be recursed
    // into again — doing both is the double-listing trap, and a node drawn twice
    // at the same transform is pixel-identical, which is why
    // `draw_list_has_no_duplicates_or_drops` asserts the multiset rather than
    // the image.
    let flattened = canvas_order::y_sort_enabled(tree, id);
    let children = if flattened {
        canvas_order::y_sort_group(tree, id)
    } else {
        canvas_order::draw_ordered_children(tree, id)
    };
    // `show_behind_parent` applies ONLY outside a y-sorted group.
    //
    // Godot has two distinct branches in `_cull_canvas_item`
    // (upstream/godot/servers/rendering/renderer_canvas_cull.cpp): the y-sort
    // branch flattens the subtree, sorts it with `ItemYSort`, and draws it in
    // that order **without ever reading the flag**; the `behind` reads live in
    // the OTHER branch (`:479`, `:486`), after the y-sort branch has closed.
    // So inside a y-sorted group, position is the only thing that orders items.
    //
    // This previously partitioned the FLATTENED group, which put a grandchild
    // carrying the flag before this node — ahead of its own parent, and ahead of
    // items Godot would have drawn first purely by Y. Gathering `behind`
    // children next to their parent instead would be equally wrong: it invents
    // an adjacency Godot does not have, and the by-Y sort would scramble it.
    let (behind, front): (Vec<NodeId>, Vec<NodeId>) = if flattened {
        (Vec::new(), children)
    } else {
        children
            .into_iter()
            .partition(|&child| read_bool(tree, child, "show_behind_parent"))
    };
    for child in behind {
        push_group_member(tree, child, flattened, out);
    }
    // ⚠️ Godot's `CLIP_CHILDREN_ONLY` (1) means "this node is used as a mask and
    // is **not** drawn" (doc/classes/CanvasItem.xml:809-811);
    // `CLIP_CHILDREN_AND_DRAW` (2) is a mask that IS drawn. Both still clip, and
    // `clip_rect_for` already covers that half with its `!= 0` test — so the
    // self-draw is the only thing the two modes disagree about, and it was the
    // half nothing implemented: `clip_children_mode` had exactly one production
    // consumer and it could not tell 1 from 2.
    //
    // Only this node's own emission is skipped. Its children are still walked
    // below, because a mask that swallowed its subtree would clip everything to
    // nothing rather than mask it.
    if canvas_order::clip_children_mode(tree, id) != 1 {
        out.push(id);
    }
    for child in front {
        push_group_member(tree, child, flattened, out);
    }
}

/// Emits one member of a parent's draw list.
///
/// When the parent was NOT y-sorted the member is an ordinary child and owns its
/// own subtree walk. When it WAS, the member's descendants are already present in
/// the flattened group — except for a member that is itself `y_sort_enabled`,
/// which the group walk deliberately did not descend into because it owns a
/// nested sort of its own.
fn push_group_member(tree: &SceneTree, id: NodeId, flattened: bool, out: &mut Vec<NodeId>) {
    if !flattened || canvas_order::y_sort_enabled(tree, id) {
        push_draw_order(tree, id, out);
        return;
    }
    if canvas_order::clip_children_mode(tree, id) != 1 {
        out.push(id);
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

// ---------------------------------------------------------------------------
// CanvasItemMaterial + shader + texture filter/repeat resolution (pat-s3rrv.28)
// ---------------------------------------------------------------------------
//
// These four resolvers used to live in `crate::scene_bridge`, feeding
// `gdrender2d::SoftwareRenderer` — a complete blend/filter/shader pipeline with
// no production caller. That module is gone (pat-s3rrv.22 collapsed the two 2D
// paths onto this one), so the logic is reimplemented here, against the render
// list that actually ships. Without them an authored `CanvasItemMaterial`
// composited as plain Mix, a `.gdshader` on a Sprite2D did nothing, and
// `texture_filter = LINEAR` / `texture_repeat = ENABLED` were inert in the
// running game however the scene set them.

/// Reads an integer enum property, accepting the int or float form a `.tscn`
/// may carry.
fn enum_prop(tree: &SceneTree, node_id: NodeId, key: &str) -> Option<i64> {
    match tree.get_node(node_id)?.get_property(key) {
        Variant::Int(i) => Some(i),
        Variant::Float(f) => Some(f as i64),
        _ => None,
    }
}

/// Builds the node's [`CanvasItemMaterial`] from the `__canvas_material__*`
/// companions `packed_scene` flattens at load.
///
/// `None` when the node has no CanvasItemMaterial, which is the common case —
/// the rasterizer then composites with plain `Mix`.
fn resolve_canvas_item_material(tree: &SceneTree, node_id: NodeId) -> Option<CanvasItemMaterial> {
    let node = tree.get_node(node_id)?;
    // A material with every property defaulted flattens to an empty bag, so
    // presence is recorded explicitly rather than inferred from the companions.
    let present = matches!(
        node.get_property("__canvas_material__present"),
        Variant::Bool(true)
    );
    if !present {
        return None;
    }
    // Godot's `CanvasItemMaterial.BlendMode`: MIX/ADD/SUB/MUL/PREMULT_ALPHA.
    let blend_mode = match enum_prop(tree, node_id, "__canvas_material__blend_mode") {
        Some(1) => BlendMode::Add,
        Some(2) => BlendMode::Sub,
        Some(3) => BlendMode::Mul,
        Some(4) => BlendMode::PremultAlpha,
        _ => BlendMode::Mix,
    };
    // `LightMode`: NORMAL/UNSHADED/LIGHT_ONLY. Carried through so the lighting
    // pass can gate on it; `shade_runtime_canvas` does not consume it yet.
    let light_mode = match enum_prop(tree, node_id, "__canvas_material__light_mode") {
        Some(1) => LightMode::Unshaded,
        Some(2) => LightMode::LightOnly,
        _ => LightMode::Normal,
    };
    Some(CanvasItemMaterial {
        blend_mode,
        light_mode,
    })
}

/// Resolves `texture_filter` up the ancestor chain (`FILTER_PARENT_NODE`).
///
/// Godot's enum: 0 = PARENT_NODE, 1 = NEAREST, 2 = LINEAR, 3/4 add mipmaps and
/// 5..7 anisotropy — the software rasterizer has no mip chain, so the mipmap
/// variants collapse onto their base filter rather than being ignored.
///
/// ⚠️ The walk is the point: reading only the node's own property looks right
/// and silently drops inheritance, so a child left on the default PARENT_NODE
/// would ignore the LINEAR its parent set.
///
/// The chain terminates at the project default
/// (`rendering/textures/canvas_textures/default_texture_filter`, which Patina
/// seeds as 1 = NEAREST), so a root node left on PARENT_NODE still samples.
fn resolve_texture_filter(tree: &SceneTree, node_id: NodeId) -> TextureFilter {
    let mut current = Some(node_id);
    while let Some(id) = current {
        match enum_prop(tree, id, "texture_filter") {
            Some(1) | Some(3) => return TextureFilter::Nearest,
            Some(2) | Some(4) => return TextureFilter::Linear,
            Some(5) => return TextureFilter::Nearest,
            Some(6) | Some(7) => return TextureFilter::Linear,
            // 0 (PARENT_NODE) or absent: keep walking up.
            _ => {}
        }
        current = tree.get_node(id).and_then(|n| n.parent());
    }
    TextureFilter::Nearest
}

/// Resolves `texture_repeat` up the ancestor chain (`REPEAT_PARENT_NODE`).
///
/// Godot's enum: 0 = PARENT_NODE, 1 = DISABLED, 2 = ENABLED, 3 = MIRROR. The
/// chain terminates at the project default, which is DISABLED.
fn resolve_texture_repeat(tree: &SceneTree, node_id: NodeId) -> TextureRepeat {
    let mut current = Some(node_id);
    while let Some(id) = current {
        match enum_prop(tree, id, "texture_repeat") {
            Some(1) => return TextureRepeat::Disabled,
            Some(2) => return TextureRepeat::Enabled,
            Some(3) => return TextureRepeat::Mirror,
            _ => {}
        }
        current = tree.get_node(id).and_then(|n| n.parent());
    }
    TextureRepeat::Disabled
}

/// Resolves the node's canvas_item `ShaderMaterial` from the flat properties
/// `packed_scene` leaves behind at load.
///
/// The source is either an inline `shader_code` (an `[sub_resource
/// type="Shader"]` in the `.tscn`) or a `.gdshader` named by `shader_path`, and
/// every `shader_parameter/*` property becomes a uniform override. The
/// `ShaderMaterial` sub-resource itself is gone by this point — only the
/// `SceneTree` survives — which is why the data travels on the node.
///
/// `None` when the node has no shader, which is the overwhelmingly common case,
/// so the caller leaves the item unshaded and the plain blit runs.
fn resolve_canvas_shader_material(tree: &SceneTree, node_id: NodeId) -> Option<ShaderMaterial> {
    let node = tree.get_node(node_id)?;

    let source = match node.get_property("shader_code") {
        Variant::String(code) if !code.is_empty() => code,
        _ => match node.get_property("shader_path") {
            Variant::String(path) if !path.is_empty() => {
                // `res://` resolves relative to the project root, as elsewhere.
                let fs = path.strip_prefix("res://").unwrap_or(path.as_str());
                let res = gdresource::importers::load_gdshader(std::path::Path::new(fs)).ok()?;
                match res.get_property("code") {
                    Some(Variant::String(code)) if !code.is_empty() => code.clone(),
                    _ => return None,
                }
            }
            _ => return None,
        },
    };

    // Always CanvasItem here: this walk only ever builds 2D canvas items, and a
    // spatial shader reaching a canvas item would be a scene authoring error
    // rather than something to silently rasterize.
    let mut material = ShaderMaterial::new();
    material.shader = Some(Shader::new(ShaderType::CanvasItem, &source));
    for (key, value) in node.properties() {
        if let Some(name) = key.strip_prefix("shader_parameter/") {
            if value != &Variant::Nil {
                material.set_shader_parameter(name, value.clone());
            }
        }
    }
    Some(material)
}

/// Returns the `layer` index of `id`'s nearest `CanvasLayer` ancestor, or `0`
/// when the node lives in the default canvas (no `CanvasLayer` above it).
///
/// Godot draws each `CanvasLayer` above the default canvas (whose implicit layer
/// is `0`), and a `CanvasLayer` node's `layer` property defaults to `1`, so a UI
/// layer naturally renders over the world.
fn canvas_layer_for(tree: &SceneTree, id: crate::NodeId) -> i64 {
    match canvas_layer_ancestor(tree, id) {
        Some(layer) => crate::canvas_layer::get_layer(tree, layer),
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
fn canvas_layer_ancestor(tree: &SceneTree, id: crate::NodeId) -> Option<crate::NodeId> {
    let mut current = tree.get_node(id).and_then(|n| n.parent());
    while let Some(pid) = current {
        let node = tree.get_node(pid)?;
        if node.is_class("CanvasLayer") {
            return Some(pid);
        }
        // A SubViewport is a canvas of its own: a CanvasLayer above it belongs
        // to the outer viewport and does not place what is inside (pat-8mb8y).
        if node.class_name() == "SubViewport" {
            return None;
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
fn canvas_layer_placement(tree: &SceneTree, id: crate::NodeId) -> (Transform2D, CanvasFollow) {
    let Some(layer) = canvas_layer_ancestor(tree, id) else {
        return (Transform2D::IDENTITY, CanvasFollow::DefaultCanvas);
    };
    // The composition itself lives next to the other CanvasLayer property
    // helpers in `gdscene`, where the `transform`-wins rule is documented.
    let xform = crate::canvas_layer::layer_transform(tree, layer);
    let follow = if crate::canvas_layer::get_follow_viewport_enabled(tree, layer) {
        CanvasFollow::Follows(crate::canvas_layer::get_follow_viewport_scale(tree, layer) as f32)
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
pub fn tilemap_quads(tree: &SceneTree, id: crate::NodeId) -> Vec<TileQuad> {
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
        let flags = if tile.alternative_id != 0 {
            match node.get_property(&tilemap::tile_alternative_flags_key(
                tile.source_id,
                tile.atlas_coords,
                tile.alternative_id,
            )) {
                Variant::Int(m) => m,
                _ => 0,
            }
        } else {
            0
        };
        quads.push(TileQuad {
            flags,
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

/// Deforms a `Polygon2D`'s vertices by its skeleton, or `None` when it has none.
///
/// pat-s3rrv.185. `gdrender2d::polygon_raster::deform_polygon_vertices` has held
/// this maths since the port began with no caller outside its own unit tests;
/// this is the binding that puts it on the frame.
///
/// ⚠️ **Spaces.** The skinning transforms are GLOBAL (a bone's rest and pose
/// both are), while `polygon` is LOCAL to the Polygon2D, and the render list
/// wants local points back because the item's own transform is applied
/// separately. So the vertices go out to global, deform, and come back:
/// `local⁻¹ ∘ skin ∘ local`. Skipping either conversion looks right only when
/// the polygon sits at the origin with no rotation or scale.
fn skinned_polygon_points(
    tree: &SceneTree,
    id: crate::NodeId,
    points: &[Vector2],
) -> Option<Vec<Vector2>> {
    // ⚠️ `Variant::NodePath` wraps a `gdcore::NodePath`, NOT a String — the two
    // cannot share an or-pattern binding. A `.tscn` writes `skeleton =
    // NodePath("../Skel")`, but a hand-built node or an older scene may carry a
    // plain String, so both are accepted and normalised here.
    let skeleton_path = match tree.get_node(id).map(|n| n.get_property("skeleton")) {
        Some(Variant::NodePath(p)) if !p.is_empty() => p.get_concatenated_names(),
        Some(Variant::String(p)) if !p.is_empty() => p,
        _ => return None,
    };
    // `skeleton` is relative to the POLYGON (`polygon_2d.cpp` resolves it with
    // the polygon's own `get_node`), unlike the bone paths below.
    let skeleton = resolve_relative(tree, id, &skeleton_path)?;
    let skin = crate::skeleton2d::published_skin_transforms(tree, skeleton);
    if skin.is_empty() {
        return None;
    }

    // 🔑 Weights are placed by the bone's INDEX IN THE SKELETON, not by the
    // order the polygon happens to list them — `polygon_2d.cpp:268` looks the
    // bone up and calls `get_index_in_skeleton()`. A polygon listing its bones
    // in a different order than the skeleton declares them would otherwise skin
    // every vertex to the wrong bone.
    let order = crate::skeleton2d::gather_bones(tree, skeleton);
    let mut weights: Vec<Vec<f32>> = vec![Vec::new(); skin.len()];

    let entries = match tree.get_node(id).map(|n| n.get_property("bones")) {
        Some(Variant::Array(v)) => v,
        _ => return None,
    };
    // Godot serializes `bones` as alternating path/weight pairs
    // (`Polygon2D::_get_bones`), so an odd-length array is malformed.
    for pair in entries.to_vec().chunks(2) {
        let [path, w] = pair else { continue };
        // `_get_bones` deliberately pushes the path as a **String** ("to avoid
        // errors due to invalid node path in editor"), so String is the normal
        // case here — NodePath is accepted for hand-built nodes.
        let path = match path {
            Variant::String(p) => p.clone(),
            Variant::NodePath(p) => p.get_concatenated_names(),
            _ => continue,
        };
        let w: Vec<f32> = match w {
            Variant::PackedFloat32Array(f) => f.clone(),
            Variant::PackedFloat64Array(f) => f.iter().map(|x| *x as f32).collect(),
            Variant::Array(items) => items
                .to_vec()
                .iter()
                .filter_map(|v| match v {
                    Variant::Float(f) => Some(*f as f32),
                    Variant::Int(i) => Some(*i as f32),
                    _ => None,
                })
                .collect(),
            _ => continue,
        };
        // ⚠️ Upstream's guard, ported verbatim in spirit: "different number of
        // vertices, sorry not using". A short weight array is a mis-authored rig,
        // and skinning by it would drag the unweighted tail to the origin.
        if w.len() != points.len() {
            continue;
        }
        // Bone paths are relative to the SKELETON, not the polygon.
        let Some(bone) = resolve_relative(tree, skeleton, &path) else {
            continue;
        };
        if let Some(idx) = order.iter().position(|&b| b == bone) {
            if idx < weights.len() {
                weights[idx] = w;
            }
        }
    }
    if weights.iter().all(|w| w.is_empty()) {
        return None;
    }

    let to_global = crate::node2d::get_global_transform(tree, id);
    let to_local = to_global.affine_inverse();
    let global_pts: Vec<Vector2> = points.iter().map(|p| to_global.xform(*p)).collect();
    let deformed =
        gdrender2d::polygon_raster::deform_polygon_vertices(&global_pts, &skin, &weights);
    Some(deformed.into_iter().map(|p| to_local.xform(p)).collect())
}

/// Resolves a `/`-separated node path relative to `from`.
fn resolve_relative(tree: &SceneTree, from: crate::NodeId, path: &str) -> Option<crate::NodeId> {
    let mut current = from;
    for part in path.split('/') {
        if part.is_empty() || part == "." {
            continue;
        }
        // `Polygon2D.skeleton` is authored as `NodePath("../Skel")` — the
        // skeleton is a SIBLING, so the common case starts by going up. `..` is
        // kept as a name by `NodePath`, so without this arm the walk searched
        // for a child called ".." and gave up, and `skinned_polygon_points`
        // returned `None` for every normally-authored rig: the polygon drew at
        // its rest pose no matter how the bones moved (pat-s3rrv.185).
        if part == ".." {
            current = tree.get_node(current).and_then(|n| n.parent())?;
            continue;
        }
        let children = tree.get_node(current).map(|n| n.children().to_vec())?;
        current = children
            .into_iter()
            .find(|&c| tree.get_node(c).map(|n| n.name() == part).unwrap_or(false))?;
    }
    Some(current)
}

fn render_kind_for(tree: &SceneTree, id: crate::NodeId, class: &str) -> Render2DKind {
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
            // 🔑 QuadMesh and PlaneMesh are NOT the same size, and treating
            // them as one was wrong in this direction as well as in the 3D
            // renderer's (pat-s3rrv.198). Godot's QuadMesh overrides PlaneMesh's
            // `size` to Vector2(1, 1) (`doc/classes/QuadMesh.xml:15`) while
            // PlaneMesh itself defaults to Vector2(2, 2)
            // (`doc/classes/PlaneMesh.xml:19`). The old code gave BOTH a
            // hardcoded 1x1 under a comment naming only QuadMesh, so every
            // MeshInstance2D holding a PlaneMesh drew at half its real size.
            //
            // An authored `__mesh__size` companion wins over either default —
            // that is the same companion the 3D path reads, so a sub-resource
            // that sets `size` is honoured identically in both viewports.
            //
            // ⚠️ Godot would additionally project by the mesh's `orientation`,
            // which for a default PlaneMesh (FACE_Y) means an XZ quad whose
            // vertices all share y = 0 and which therefore collapses to a line
            // in 2D. Patina draws the size'd quad instead: the bead asks for
            // "a PlaneMesh draws at 2x2", and a deliberately degenerate node is
            // indistinguishable on screen from an unimplemented one. Flagged
            // rather than silently matched.
            let quad_size = match primitive.as_deref() {
                Some("QuadMesh") => Some(Vector2::new(1.0, 1.0)),
                Some("PlaneMesh") => Some(Vector2::new(2.0, 2.0)),
                _ => None,
            };
            if let Some(default_size) = quad_size {
                let authored = read_vector2(tree, id, "__mesh__size");
                let size = if authored == Vector2::ZERO {
                    default_size
                } else {
                    authored
                };
                let (hx, hy) = (size.x * 0.5, size.y * 0.5);
                return Render2DKind::Mesh {
                    texture: node2d::get_texture_path(tree, id).unwrap_or_default(),
                    vertices: vec![
                        Vector2::new(-hx, -hy),
                        Vector2::new(hx, -hy),
                        Vector2::new(hx, hy),
                        Vector2::new(-hx, hy),
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
        // BackBufferCopy (pat-s3rrv.183). Registered with no behaviour, so it
        // fell through to `Marker` — and with nothing copying the frame, a
        // canvas_item shader sampling SCREEN_TEXTURE had no screen to read.
        "BackBufferCopy" => {
            return Render2DKind::BackBufferCopy {
                copy_mode: read_int(tree, id, "copy_mode").unwrap_or(1),
                rect: read_rect2(tree, id, "rect").unwrap_or_else(|| {
                    Rect2::new(Vector2::new(-100.0, -100.0), Vector2::new(200.0, 200.0))
                }),
            };
        }
        // MultiMeshInstance2D (pat-s3rrv.183). Registered with no draw
        // semantics, so it fell through to `Marker` and showed a placeholder
        // glyph instead of its instances.
        //
        // `packed_scene` already flattens `multimesh = SubResource(..)` onto the
        // node as `__multimesh__*`, and that pass is class-agnostic — it copies
        // every property of any `multimesh` sub-resource, so the 2D node gets
        // the same companions the 3D one does and needs no new flattening.
        //
        // Geometry: the PRIMITIVE case only, matching the MeshInstance2D arm
        // above. The multimesh's own `mesh` is a nested SubResource reference
        // whose body is not copied onto the node, so there is nothing to read
        // real geometry from; Godot's QuadMesh default of 1x1 centred on the
        // origin is what a 2D multimesh almost always instances.
        "MultiMeshInstance2D" => {
            let instances = multimesh_2d_instances(tree, id);
            if !instances.is_empty() {
                let h = 0.5;
                return Render2DKind::MultiMesh {
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
                    instances,
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
            let rest_points = read_points(tree, id, "polygon");
            // pat-s3rrv.185: a polygon bound to a Skeleton2D draws at its bones'
            // current pose. Falls back to the rest vertices when there is no
            // skeleton, no published pose, or no usable weights.
            let points = skinned_polygon_points(tree, id, &rest_points)
                .unwrap_or_else(|| rest_points.clone());
            // Godot leaves `uv` empty until the polygon is unwrapped and then
            // textures it by its own VERTEX POSITIONS — `polygon_2d.cpp:237`
            // uses `points[i]` when the uv array is the wrong length, so an
            // un-unwrapped textured polygon is still textured, not flat
            // (pat-s3rrv.85).
            let authored_uv = read_points(tree, id, "uv");
            let uvs = if authored_uv.len() == points.len() {
                authored_uv
            } else {
                points.clone()
            };
            if points.len() >= 3 {
                if let Some(texture) = node2d::get_texture_path(tree, id) {
                    return Render2DKind::PolygonTextured {
                        points,
                        uvs,
                        texture,
                        color: read_color(tree, id, "color"),
                    };
                }
            }
            return Render2DKind::Polygon {
                points,
                color: read_color(tree, id, "color"),
            };
        }
        "Line2D" => {
            let points = read_points(tree, id, "points");
            let width = read_f32(tree, id, "width", 0.0);
            let color = read_color(tree, id, "default_color");
            // `gradient` and `width_curve` are sub-resources that `packed_scene`
            // flattened onto the node; caps and joints are plain enums. Any one
            // of them makes the stroke non-uniform, which the shaded path exists
            // to draw (pat-s3rrv.85).
            let offsets = read_f32_array(tree, id, "__gradient_offsets");
            let grad_colors = read_colors(tree, id, "__gradient_colors");
            let curve = read_f32_array(tree, id, "__width_curve_samples");
            // Godot: 0 = NONE, 1 = BOX, 2 = ROUND for both caps and joints.
            let round_cap = read_f32(tree, id, "begin_cap_mode", 0.0) as i32 == 2
                || read_f32(tree, id, "end_cap_mode", 0.0) as i32 == 2;
            let round_joint = read_f32(tree, id, "joint_mode", 0.0) as i32 == 2;
            let shaded = !curve.is_empty()
                || (!grad_colors.is_empty() && grad_colors.len() == offsets.len())
                || round_cap
                || round_joint;
            if shaded && points.len() >= 2 {
                let last = (points.len() - 1) as f32;
                let (widths, colors): (Vec<f32>, Vec<Color>) = (0..points.len())
                    .map(|i| {
                        // Godot samples both along the stroke's normalized
                        // length (`line_builder.cpp`), so a two-point line reads
                        // the curve/gradient at exactly 0.0 and 1.0.
                        let t = if last > 0.0 { i as f32 / last } else { 0.0 };
                        (
                            width * sample_baked(&curve, t),
                            sample_gradient(&offsets, &grad_colors, t).unwrap_or(color),
                        )
                    })
                    .unzip();
                return Render2DKind::LineShaded {
                    points,
                    widths,
                    colors,
                    closed: read_bool(tree, id, "closed"),
                    round_cap,
                    round_joint,
                };
            }
            return Render2DKind::Line {
                points,
                width,
                color,
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
                font_path: crate::control_render::font_override_path(tree, id),
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
fn read_particles(tree: &SceneTree, id: crate::NodeId) -> Vec<ParticleQuad> {
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
fn read_vector2(tree: &SceneTree, id: crate::NodeId, key: &str) -> Vector2 {
    match tree.get_node(id).map(|n| n.get_property(key)) {
        Some(Variant::Vector2(v)) => v,
        _ => Vector2::ZERO,
    }
}

/// Reads a `bool` property, defaulting to `false` when absent.
/// Reads an int property, or `None` when absent or another type.
fn read_int(tree: &SceneTree, id: crate::NodeId, key: &str) -> Option<i64> {
    match tree.get_node(id)?.get_property(key) {
        Variant::Int(v) => Some(v),
        _ => None,
    }
}

/// Reads a `Rect2` property, or `None` when absent or another type.
fn read_rect2(tree: &SceneTree, id: crate::NodeId, key: &str) -> Option<Rect2> {
    match tree.get_node(id)?.get_property(key) {
        Variant::Rect2(r) => Some(r),
        _ => None,
    }
}

fn read_bool(tree: &SceneTree, id: crate::NodeId, key: &str) -> bool {
    matches!(
        tree.get_node(id).map(|n| n.get_property(key)),
        Some(Variant::Bool(true))
    )
}

/// Reads a `Color` property, defaulting to white (Godot's default tint) when
/// absent.
fn read_color(tree: &SceneTree, id: crate::NodeId, key: &str) -> Color {
    match tree.get_node(id).map(|n| n.get_property(key)) {
        Some(Variant::Color(c)) => c,
        _ => Color::WHITE,
    }
}

/// Reads a packed `Vector2` array property (a polygon/polyline point list),
/// dropping any non-`Vector2` entries and defaulting to empty when absent.
fn read_points(tree: &SceneTree, id: crate::NodeId, key: &str) -> Vec<Vector2> {
    match tree.get_node(id).map(|n| n.get_property(key)) {
        // ⚠️ A `.tscn` writes `polygon = PackedVector2Array(...)` and the loader
        // parses it to the packed variant, NOT a generic Array — so only
        // hand-built nodes ever matched the arm below and every authored
        // Polygon2D/Line2D rendered empty (pat-0q2kx.77).
        Some(Variant::PackedVector2Array(items)) => items,
        Some(Variant::Array(items)) => items
            .to_vec()
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
fn read_f32(tree: &SceneTree, id: crate::NodeId, key: &str, default: f32) -> f32 {
    match tree.get_node(id).map(|n| n.get_property(key)) {
        Some(Variant::Float(f)) => f as f32,
        Some(Variant::Int(i)) => i as f32,
        _ => default,
    }
}

/// Reads a `PackedFloat32Array` property, tolerating the generic `Array` form a
/// hand-built node may carry (same split `read_points` documents).
fn read_f32_array(tree: &SceneTree, id: crate::NodeId, key: &str) -> Vec<f32> {
    match tree.get_node(id).map(|n| n.get_property(key)) {
        Some(Variant::PackedFloat32Array(v)) => v,
        Some(Variant::PackedFloat64Array(v)) => v.into_iter().map(|f| f as f32).collect(),
        Some(Variant::Array(items)) => items
            .to_vec()
            .into_iter()
            .filter_map(|v| match v {
                Variant::Float(f) => Some(f as f32),
                Variant::Int(i) => Some(i as f32),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// Reads a `PackedColorArray` property, tolerating the generic `Array` form.
fn read_colors(tree: &SceneTree, id: crate::NodeId, key: &str) -> Vec<Color> {
    match tree.get_node(id).map(|n| n.get_property(key)) {
        Some(Variant::PackedColorArray(v)) => v,
        Some(Variant::Array(items)) => items
            .to_vec()
            .into_iter()
            .filter_map(|v| match v {
                Variant::Color(c) => Some(c),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// Samples a flattened `Gradient` at `t`.
///
/// Godot's default `GRADIENT_INTERPOLATE_LINEAR` lerps between the two stops
/// bracketing the offset and clamps outside the first and last
/// (`Gradient::get_color_at_offset`, upstream/godot/scene/resources/gradient.h).
/// Returns `None` when the node carries no usable gradient, so the caller falls
/// back to `default_color` rather than to black.
fn sample_gradient(offsets: &[f32], colors: &[Color], t: f32) -> Option<Color> {
    if colors.is_empty() || offsets.len() != colors.len() {
        return None;
    }
    if colors.len() == 1 || t <= offsets[0] {
        return Some(colors[0]);
    }
    let last = offsets.len() - 1;
    if t >= offsets[last] {
        return Some(colors[last]);
    }
    let mut i = 0;
    while i + 1 < last && offsets[i + 1] <= t {
        i += 1;
    }
    let span = offsets[i + 1] - offsets[i];
    let k = if span.abs() < 1e-6 {
        0.0
    } else {
        (t - offsets[i]) / span
    };
    let (a, b) = (colors[i], colors[i + 1]);
    Some(Color::new(
        a.r + (b.r - a.r) * k,
        a.g + (b.g - a.g) * k,
        a.b + (b.b - a.b) * k,
        a.a + (b.a - a.a) * k,
    ))
}

/// Samples a baked curve table at `t`, interpolating between the two nearest
/// samples the way Godot's `Curve::sample_baked` does.
///
/// An empty table means the node has no `width_curve`, which scales the stroke
/// by 1.0 — never by 0.0, which would erase it.
fn sample_baked(samples: &[f32], t: f32) -> f32 {
    if samples.is_empty() {
        return 1.0;
    }
    if samples.len() == 1 {
        return samples[0];
    }
    let last = samples.len() - 1;
    let pos = t.clamp(0.0, 1.0) * last as f32;
    let i = (pos.floor() as usize).min(last);
    if i == last {
        return samples[last];
    }
    let k = pos - i as f32;
    samples[i] + (samples[i + 1] - samples[i]) * k
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
    let mut items = collect_canvas_items(tree);
    apply_parallax_mirroring(tree, &mut items);
    items
}

/// Godot `ParallaxLayer.motion_mirroring` — the layer's content repeats every
/// `mirroring` pixels on each non-zero axis, which is how an infinitely
/// scrolling background is built (pat-s3rrv.86).
///
/// ⚠️ The wrap arithmetic was already right: `gdserver2d`'s `mirror_axis` wraps
/// the layer's scroll offset, so a mirrored background scrolled correctly — and
/// then simply ran out, because the layer was drawn ONCE. This is the missing
/// half: the same items re-emitted at multiples of the stride.
///
/// ⚠️ A stride of `0` on an axis means "do not repeat that axis", NOT "repeat
/// every 0 pixels". Godot clamps the property to `>= 0`
/// (`parallax_layer.cpp:81`) and treats either axis being non-zero as enabling
/// repeat (`renderer_canvas_cull.cpp:533`); dividing by a zero stride is the
/// obvious way to hang the frame.
pub fn apply_parallax_mirroring(tree: &SceneTree, items: &mut Vec<Render2DItem>) {
    let viewport = tree.root_window().visible_size();
    for layer_id in tree.all_nodes_in_tree_order() {
        let Some(layer) = tree.get_node(layer_id) else {
            continue;
        };
        if layer.class_name() != "ParallaxLayer" {
            continue;
        }
        let mirroring = match layer.get_property("motion_mirroring") {
            Variant::Vector2(v) => v,
            _ => continue,
        };
        let (stride_x, stride_y) = (mirroring.x.max(0.0), mirroring.y.max(0.0));
        if stride_x <= 0.0 && stride_y <= 0.0 {
            continue;
        }

        let owned: Vec<usize> = items
            .iter()
            .enumerate()
            .filter(|(_, item)| item_belongs_to_layer(tree, item.node_id, layer_id))
            .map(|(i, _)| i)
            .collect();
        if owned.is_empty() {
            continue;
        }

        // Enough copies to cover the viewport either side of the original.
        //
        // ⚠️ Capped. Godot bounds this through culling; here a stride far
        // smaller than the viewport (a mis-authored 0.01) would otherwise ask
        // for hundreds of thousands of copies and hang the frame rather than
        // draw a wrong picture. The cap is generous enough that any stride
        // large enough to be visible as a repeat is unaffected.
        let axis_copies = |stride: f32, extent: f32| -> i32 {
            if stride <= 0.0 {
                return 0;
            }
            let needed = (extent / stride).ceil() as i64 + 1;
            needed.clamp(0, MAX_PARALLAX_REPEATS_PER_AXIS) as i32
        };
        let (nx, ny) = (
            axis_copies(stride_x, viewport.x),
            axis_copies(stride_y, viewport.y),
        );

        let mut copies: Vec<Render2DItem> = Vec::new();
        for ix in -nx..=nx {
            for iy in -ny..=ny {
                if ix == 0 && iy == 0 {
                    continue;
                }
                let dx = ix as f32 * stride_x;
                let dy = iy as f32 * stride_y;
                for &i in &owned {
                    let mut copy = items[i].clone();
                    copy.global_transform.origin = Vector2::new(
                        copy.global_transform.origin.x + dx,
                        copy.global_transform.origin.y + dy,
                    );
                    copies.push(copy);
                }
            }
        }
        items.extend(copies);
    }
}

/// Whether a render item was produced by `layer` or one of its descendants.
fn item_belongs_to_layer(tree: &SceneTree, item_node: u64, layer: NodeId) -> bool {
    let mut cur = Some(NodeId::from_object_id(gdcore::id::ObjectId::from_raw(
        item_node,
    )));
    while let Some(id) = cur {
        if id == layer {
            return true;
        }
        cur = tree.get_node(id).and_then(|n| n.parent());
    }
    false
}

/// The `res://` path of a node's `theme` resource, or `""` when it has none.
///
/// ⚠️ Reads the `__extres__theme` COMPANION key first, and that is the whole
/// trick. `packed_scene` stores a `theme = ExtResource("1_t")` property as the
/// **literal reference string** and puts the resolved path in
/// `__extres__<prop>` (`packed_scene.rs:1524-1556`); only `script` is rewritten
/// in place. Reading `theme` alone yields `ExtResource("1_t")`, which is not a
/// path, so the load would silently find nothing. Same shape as
/// `control::foldable_group_of`.
///
/// The raw property is still consulted as a fallback, for a hand-written
/// `theme = "res://ui.tres"` and for a node whose theme was set from a script.
fn theme_resource_path(node: &crate::node::Node) -> String {
    match node.get_property("__extres__theme") {
        Variant::String(s) if !s.is_empty() => return s,
        _ => {}
    }
    match node.get_property("theme") {
        // A still-unresolved `ExtResource(...)` reference is not a path; treating
        // it as one would ask the filesystem for a literal `ExtResource("1_t")`.
        Variant::String(s) if s.starts_with("res://") || s.starts_with("user://") => s,
        _ => String::new(),
    }
}

/// Every Control in `root`'s subtree that carries its own `theme`, loaded.
///
/// Godot resolves a theme item through the node's own `theme`, then each
/// ancestor's, then the project theme, then the built-in default
/// (`scene/theme/theme_owner.cpp`). The control walk already implements the
/// ancestor half of that, merging each node's own theme over the one handed down
/// from its parent (`control_render.rs:871-880`) — it just needs this map.
///
/// Themes are read through [`FileAccess`], so a mounted `.pck` serves them, and
/// cached per path: a UI where twenty Buttons sit under one themed Panel parses
/// the `.tres` once. The cache stores the FAILED lookup too, so a missing or
/// unparseable theme is not re-read once per node.
/// The base theme every `Control` inherits before its own `theme` applies: the
/// project's `gui/theme/custom`, merged over the engine default (pat-mgixk.65).
///
/// Godot loads this in `ThemeDB::initialize_theme` and it sits UNDER every node
/// theme in the resolution order, so a project theme styles a UI whose nodes
/// name no theme at all — which is how a shipped game is usually themed, and
/// the clause of this bead that [`collect_node_themes`] does not cover.
///
/// Read through [`FileAccess`] so it resolves against whatever `res://` root
/// the caller installed, and so a mounted `.pck` serves it — the same
/// mechanism `collect_node_themes` uses, rather than a second notion of where
/// the project lives. A project with no `project.godot`, no `gui/theme/custom`,
/// or an unreadable theme file keeps the plain default, so this cannot make a
/// previously-working scene render differently.
fn project_base_theme() -> crate::theme::Theme {
    let mut base = crate::theme::Theme::default_theme();
    let Ok(project) = FileAccess::read_file_string("res://project.godot") else {
        return base;
    };
    let Ok(settings) = gdcore::project_settings::ProjectSettings::from_godot_format(&project)
    else {
        return base;
    };
    let Some(path) = settings
        .get("gui/theme/custom")
        .and_then(|v| v.as_string())
        .filter(|p| !p.is_empty())
    else {
        return base;
    };
    match FileAccess::read_file_string(path) {
        // ⚠️ `Theme::merge_with` lets its ARGUMENT win on collisions, so the
        // project theme is merged INTO the default. The other way round would
        // leave the default masking every project override — and silently, the
        // pixels simply staying default.
        Ok(text) => base.merge_with(&crate::theme::Theme::from_tres(&text)),
        Err(e) => tracing::warn!(
            theme = %path,
            error = %e,
            "gui/theme/custom names a theme resource that cannot be read; \
             falling back to the default theme"
        ),
    }
    base
}

fn collect_node_themes(
    tree: &SceneTree,
    root: NodeId,
) -> std::collections::HashMap<NodeId, crate::theme::Theme> {
    let mut subtree = Vec::new();
    tree.collect_subtree_top_down(root, &mut subtree);

    let mut by_path: std::collections::HashMap<String, Option<crate::theme::Theme>> =
        std::collections::HashMap::new();
    let mut out = std::collections::HashMap::new();

    for id in subtree {
        let Some(node) = tree.get_node(id) else {
            continue;
        };
        let path = theme_resource_path(node);
        if path.is_empty() {
            continue;
        }
        let loaded = by_path.entry(path.clone()).or_insert_with(|| {
            match FileAccess::read_file_string(&path) {
                Ok(text) => Some(crate::theme::Theme::from_tres(&text)),
                Err(e) => {
                    tracing::warn!(
                        theme = %path,
                        error = %e,
                        "a node names a theme resource that cannot be read; \
                         falling back to the inherited theme"
                    );
                    None
                }
            }
        });
        if let Some(theme) = loaded {
            out.insert(id, theme.clone());
        }
    }
    out
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
/// 2. the control tree is drawn by [`crate::control_render`] — and its
///    primitives are mapped onto [`Render2DKind`], so no per-control drawing
///    rule is duplicated here.
///
/// Theme resolution (pat-mgixk.65): each Control's own `theme` resource is
/// loaded by [`collect_node_themes`] and passed to
/// [`crate::control_render::control_draw_items_themed`], so a `.tscn`'s
/// `theme = ExtResource("ui.tres")` reaches the node and every descendant.
///
/// The inheritance happens on the way DOWN, inside the control walk: each node
/// merges its own theme over the one handed to it and passes the result to its
/// children (`control_render.rs:871-880`). That is O(1) per node instead of a
/// walk back up per lookup. Note it is *not*
/// `control_render::effective_theme`, which implements the same precedence by
/// walking ancestors and still has no callers.
///
/// ⚠️ The base is still [`crate::theme::Theme::default_theme`]: the project
/// theme from ProjectSettings `gui/theme/custom` is **not** wired in yet, so a
/// project that sets only that key still renders with the built-in default.
/// Godot loads it in `ThemeDB::initialize_theme` (`scene/theme/theme_db.cpp`)
/// and merges it under the node chain. Doing that needs ProjectSettings to
/// reach this function, which it currently does not.
///
/// `z_index` carries the walk's paint order (parents first) and is offset by
/// [`CONTROL_Z_BASE`] so UI composites above plain 2D nodes, which is what a
/// Godot scene with a UI layer looks like.
pub fn build_control_render_items(
    tree: &mut SceneTree,
    root: crate::NodeId,
    size: Vector2,
) -> Vec<Render2DItem> {
    build_control_render_items_sourced(tree, root, size)
        .into_iter()
        .map(|(item, _)| item)
        .collect()
}

/// [`build_control_render_items`], each item paired with the Control node that
/// drew it, and placed where that node really sits in the canvas: its own
/// canvas layer, its effective `z_index`, and its layer's follow mode.
///
/// Feed the result to [`merge_control_items`], which puts each item at its
/// node's TREE position among the runtime items. That is what Godot's canvas
/// does — every CanvasItem, Control or Node2D, draws by layer, then z, then
/// tree order.
pub fn build_control_render_items_sourced(
    tree: &mut SceneTree,
    root: crate::NodeId,
    size: Vector2,
) -> Vec<(Render2DItem, Option<crate::NodeId>)> {
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
            // The container's OWN rect, from its anchors and offsets, not the
            // viewport: an outermost container was laid out as if it filled
            // the screen, so a centred 300px menu spread its children across
            // the whole window. A container placed only by a stored `size`
            // (no offsets) keeps that; one with neither still gets the viewport.
            let anchored = control::anchored_size(tree, id, size);
            let own = if anchored.x > 0.0 && anchored.y > 0.0 {
                anchored
            } else {
                let stored = control::get_size(tree, id);
                if stored.x > 0.0 && stored.y > 0.0 {
                    stored
                } else {
                    size
                }
            };
            control::sort_container_tree(tree, id, own);
        }
    }

    // (2) Draw, then translate each primitive into a render item. Positions are
    // already global viewport space, so the item transform is a pure translation.
    // 🔑 Each Control's own `theme` resource is resolved here and handed to the
    // THEMED pass (pat-mgixk.65). `control_draw_items` forwards an EMPTY
    // `node_themes` map (`control_render.rs:231-236`), so going through it meant
    // the walk had nothing but the default to merge and a scene's
    // `theme = ExtResource("ui.tres")` changed no pixels at all. The merge
    // machinery was already written and simply had no caller that fed it.
    let node_themes = collect_node_themes(tree, root);
    // The project theme is the BASE of the chain, under every node theme
    // (pat-mgixk.65 clause 2). `effective_theme` merges a node's own theme over
    // whatever base it is handed, so supplying it here is all the wiring the
    // project setting needs.
    let theme = project_base_theme();
    let items = crate::control_render::control_draw_items_themed_sourced(
        tree,
        root,
        &theme,
        &node_themes,
        size,
    );
    let mut out = Vec::with_capacity(items.len());
    for (item, source) in items {
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
        // 🔑 The node's REAL place in the canvas. Every primitive used to get
        // `CONTROL_Z_BASE + walk order` on layer 0 — "above every Node2D" — so a
        // full-rect ColorRect background (the first child of Dodge the Creeps'
        // Main) painted over the player and every enemy, and a HUD inside a
        // CanvasLayer followed the Camera2D. A primitive with no single node
        // (a popup overlay) keeps the old above-everything placement.
        let (z_index, canvas_layer, canvas_follow) = match source {
            Some(node) => (
                canvas_order::effective_z_index(tree, node) as i64,
                canvas_layer_for(tree, node),
                canvas_layer_placement(tree, node).1,
            ),
            None => (CONTROL_Z_BASE + z as i64, 0, CanvasFollow::DefaultCanvas),
        };
        out.push((Render2DItem {
            // Control primitives are not one-per-node, so there is no single node
            // id to attribute them to; 0 marks "not pickable as a node".
            node_id: 0,
            global_transform: transform,
            // Each primitive already carries its own themed colour, so the
            // item-level tint stays neutral.
            modulate: Color::new(1.0, 1.0, 1.0, 1.0),
            z_index,
            canvas_layer,
            canvas_follow,
            // Control primitives are drawn by their own layout pass, which has
            // already resolved them inside their container.
            clip_rect: None,
            // Control primitives inherit the default lighting layer.
            light_mask: 1,
            // ...and the default visibility layer, so a viewport using the
            // default cull mask still draws the editor's own chrome.
            visibility_layer: 1,
            // Editor/UI chrome is not scene-authored, so it carries no material
            // and samples with the project defaults. Concrete modes rather than
            // `ParentNode`: these items never go through the ancestor walk, so
            // an inheritance marker here would reach the blit unresolved.
            canvas_material: None,
            shader_material: None,
            texture_filter: TextureFilter::Nearest,
            texture_repeat: TextureRepeat::Disabled,
            kind,
        }, source));
    }
    out
}

/// Interleaves themed Control items into a runtime render list at each source
/// node's TREE position, so the raster's one stable `(canvas_layer, z_index)`
/// sort reproduces Godot's canvas order — layer, then z, then tree order — for
/// Controls and Node2Ds together.
///
/// A node BOTH passes draw is drawn once, by whichever representation is the
/// complete one — drawing both double-blended any translucent colour:
/// - a **ColorRect** keeps the runtime item, which honours an authored
///   `position`/`size`, the inherited modulate, materials and CanvasGroups; the
///   Control pass's rect for it is the duplicate;
/// - a **Label** keeps the Control item, which carries its colour and its
///   in-rect alignment; the runtime Text item has neither.
pub fn merge_control_items(
    tree: &SceneTree,
    runtime: Vec<Render2DItem>,
    controls: Vec<(Render2DItem, Option<crate::NodeId>)>,
) -> Vec<Render2DItem> {
    let order: std::collections::HashMap<u64, usize> = tree
        .all_nodes_in_tree_order()
        .into_iter()
        .enumerate()
        .map(|(i, id)| (id.raw(), i))
        .collect();
    let class_of = |raw: u64| {
        tree.get_node(crate::NodeId::from_object_id(gdcore::id::ObjectId::from_raw(raw)))
            .map(|n| n.class_name().to_string())
            .unwrap_or_default()
    };
    let drawn_by_controls: std::collections::HashSet<u64> = controls
        .iter()
        .filter_map(|(_, source)| source.map(|id| id.raw()))
        .collect();
    let runtime_color_rects: std::collections::HashSet<u64> = runtime
        .iter()
        .filter(|item| matches!(item.kind, Render2DKind::ColorRect { .. }))
        .map(|item| item.node_id)
        .collect();
    let runtime: Vec<Render2DItem> = runtime
        .into_iter()
        .filter(|item| {
            !(drawn_by_controls.contains(&item.node_id)
                && matches!(item.kind, Render2DKind::Text { .. })
                && class_of(item.node_id) == "Label")
        })
        .collect();
    let controls: Vec<(Render2DItem, Option<crate::NodeId>)> = controls
        .into_iter()
        .filter(|(_, source)| {
            !source.is_some_and(|id| {
                runtime_color_rects.contains(&id.raw()) && class_of(id.raw()) == "ColorRect"
            })
        })
        .collect();
    // (layer, z, tree index): sourceless items sort after every node.
    let key = |item: &Render2DItem, node: Option<u64>| {
        (
            item.canvas_layer,
            item.z_index,
            node.and_then(|n| order.get(&n).copied()).unwrap_or(usize::MAX),
        )
    };
    let mut controls: Vec<((i64, i64, usize), Render2DItem)> = controls
        .into_iter()
        .map(|(item, source)| (key(&item, source.map(|s| s.raw())), item))
        .collect();
    // Stable: a node's own primitives keep their draw order.
    controls.sort_by_key(|(k, _)| *k);
    let mut controls = controls.into_iter().peekable();
    let mut out = Vec::with_capacity(runtime.len() + controls.len());
    for item in runtime {
        let here = key(&item, Some(item.node_id));
        // A Control draws before a runtime item it precedes in the canvas order;
        // at an equal key (the node's own `_draw` output) the theme draw goes
        // first, as Godot draws a Control's theme before calling `_draw`.
        while controls.peek().is_some_and(|(k, _)| *k <= here) {
            out.push(controls.next().unwrap().1);
        }
        out.push(item);
    }
    out.extend(controls.map(|(_, item)| item));
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
pub fn current_camera2d(tree: &SceneTree) -> Option<crate::NodeId> {
    // One definition, shared with the per-frame scroll pass so the camera that
    // integrates is the camera that renders (pat-s3rrv.84).
    crate::behavioral_2d::current_camera2d(tree)
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
    Some(camera2d_view_transform_of(
        tree,
        camera,
        viewport_size,
        previous_center,
        dt,
    ))
}

/// [`camera2d_view_transform`] for a given camera — the one a SubViewport's
/// pass uses with its own camera and size (pat-8mb8y).
pub fn camera2d_view_transform_of(
    tree: &SceneTree,
    camera: crate::NodeId,
    viewport_size: Vector2,
    previous_center: Option<Vector2>,
    dt: f32,
) -> Transform2D {
    let config = node2d::camera2d_config_from_node(tree, camera);
    // The camera's own global position plus its `offset`, as Godot's target.
    let target = node2d::get_global_transform(tree, camera).origin;
    let center = resolve_camera2d_center(
        tree,
        camera,
        &config,
        target,
        viewport_size,
        previous_center,
        dt,
    );
    // Godot folds the camera's rotation into the canvas transform only when
    // `ignore_rotation` is false, which is NOT the default: an upright view
    // under a rotated camera is the normal case (camera_2d.cpp:259).
    let rotation = if config.ignore_rotation {
        0.0
    } else {
        published_camera2d_rotation(tree, camera)
            .unwrap_or_else(|| node2d::get_global_rotation(tree, camera))
    };

    let half = Vector2::new(viewport_size.x * 0.5, viewport_size.y * 0.5);
    // Applied right-to-left: recentre on the camera, scale by zoom, then move to
    // the middle of the viewport (`A * B` applies B first).
    // The world counter-rotates by the camera's angle, so the canvas transform
    // carries its negation.
    let view = Transform2D::translated(half) * Transform2D::scaled(config.zoom);
    let view = if rotation == 0.0 {
        view
    } else {
        view * Transform2D::rotated(-rotation)
    };
    view * Transform2D::translated(-center)
}

/// Whether `id` is a `CanvasItem` (a Node2D or a Control), with the same
/// ClassDB-free fallback the Control hit test uses.
fn is_canvas_item(tree: &SceneTree, id: crate::NodeId) -> bool {
    tree.get_node(id)
        .is_some_and(|n| n.is_class("CanvasItem") || n.is_class("Node2D"))
        || control::node_is_control(tree, id)
}

/// A `CanvasItem`'s LOCAL transform, as Godot's `get_transform()` builds it.
///
/// A Control is `T(position) * T(pivot) * R(rotation) * S(scale) * T(-pivot)`
/// (`control.cpp::_get_internal_transform`); everything else is the Node2D
/// transform.
fn canvas_item_local_transform(tree: &SceneTree, id: crate::NodeId) -> Transform2D {
    if !control::node_is_control(tree, id) {
        return node2d::get_local_transform(tree, id);
    }
    let Some(node) = tree.get_node(id) else {
        return Transform2D::IDENTITY;
    };
    let rotation = match node.get_property("rotation") {
        Variant::Float(v) => v as f32,
        Variant::Int(v) => v as f32,
        _ => 0.0,
    };
    let scale = match node.get_property("scale") {
        Variant::Vector2(v) => v,
        _ => Vector2::new(1.0, 1.0),
    };
    let pivot = match node.get_property("pivot_offset") {
        Variant::Vector2(v) => v,
        _ => Vector2::ZERO,
    };
    Transform2D::translated(control::get_position(tree, id) + pivot)
        * Transform2D::rotated(rotation)
        * Transform2D::scaled(scale)
        * Transform2D::translated(Vector2::new(-pivot.x, -pivot.y))
}

/// `CanvasItem.get_global_transform()` for any canvas item, Controls included
/// (pat-b4zhq).
///
/// Godot composes through `get_parent_item()`, which is null for a top-level
/// item and for a parent that is not a CanvasItem — so a `CanvasLayer` or a
/// plain `Node` ends the walk rather than being passed through.
pub fn canvas_item_global_transform(tree: &SceneTree, id: crate::NodeId) -> Transform2D {
    let mut global = canvas_item_local_transform(tree, id);
    let mut current = id;
    while !node2d::is_top_level(tree, current) {
        match tree.get_node(current).and_then(|n| n.parent()) {
            Some(parent) if is_canvas_item(tree, parent) => {
                global = canvas_item_local_transform(tree, parent) * global;
                current = parent;
            }
            _ => break,
        }
    }
    global
}

/// `CanvasItem.get_global_transform_with_canvas()` (pat-b4zhq): the item's
/// global transform carried through its canvas — the enclosing `CanvasLayer`'s
/// final transform, or the viewport's canvas transform (the `Camera2D` view)
/// for an item on the default canvas (`canvas_item.cpp:159`).
///
/// The layer's final transform follows `canvas_layer.cpp::get_final_transform`:
/// a `follow_viewport_enabled` layer is `canvas * scale(follow) * transform`,
/// otherwise the layer's own transform.
pub fn global_transform_with_canvas(tree: &SceneTree, id: crate::NodeId) -> Transform2D {
    let global = canvas_item_global_transform(tree, id);
    // The canvas of the viewport the item is IN: a SubViewport's own camera at
    // the SubViewport's size, or the root window's (pat-8mb8y).
    let viewport_canvas = || {
        let vp = crate::behavioral_2d::enclosing_sub_viewport(tree, id);
        let size = crate::behavioral_2d::sub_viewport_size(tree, vp);
        crate::behavioral_2d::current_camera2d_in(tree, vp)
            .map(|cam| camera2d_view_transform_of(tree, cam, size, None, 0.0))
            .unwrap_or(Transform2D::IDENTITY)
    };
    let canvas = match canvas_layer_ancestor(tree, id) {
        Some(layer) => {
            let xform = crate::canvas_layer::layer_transform(tree, layer);
            if crate::canvas_layer::get_follow_viewport_enabled(tree, layer) {
                let s = crate::canvas_layer::get_follow_viewport_scale(tree, layer) as f32;
                viewport_canvas() * Transform2D::scaled(Vector2::new(s, s)) * xform
            } else {
                xform
            }
        }
        None => viewport_canvas(),
    };
    canvas * global
}

/// The Camera2D scroll centre `behavioral_2d::process_camera2d_scroll` published
/// for this frame, if any.
fn published_camera2d_center(tree: &SceneTree, camera: crate::NodeId) -> Option<Vector2> {
    match tree
        .get_node(camera)
        .map(|n| n.get_property(crate::behavioral_2d::CAMERA2D_SCROLL_CENTER))
    {
        Some(gdvariant::Variant::Vector2(v)) => Some(v),
        _ => None,
    }
}

/// The published view rotation, companion to [`published_camera2d_center`].
fn published_camera2d_rotation(tree: &SceneTree, camera: crate::NodeId) -> Option<f32> {
    match tree
        .get_node(camera)
        .map(|n| n.get_property(crate::behavioral_2d::CAMERA2D_SCROLL_ROTATION))
    {
        Some(gdvariant::Variant::Float(f)) => Some(f as f32),
        _ => None,
    }
}

/// Decides which centre a frame renders at.
///
/// The three cases are distinct on purpose. An explicit `previous_center` means
/// the CALLER keeps the scroll state and wants this call to advance it. A
/// published centre means the frame step already advanced it, so integrating
/// again here would ease twice per frame and converge at double speed. With
/// neither, there is no state to ease from and the camera snaps — which is also
/// the correct answer whenever smoothing is off (pat-s3rrv.84).
fn resolve_camera2d_center(
    tree: &SceneTree,
    camera: crate::NodeId,
    config: &node2d::Camera2DConfig,
    target: Vector2,
    viewport_size: Vector2,
    previous_center: Option<Vector2>,
    dt: f32,
) -> Vector2 {
    // 🔑 `offset` is added AFTER the limits, not to the target: Godot clamps
    // `screen_rect` to the limits and only then does
    // `screen_rect.position += offset` (camera_2d.cpp:253). The 2D Platformer's
    // camera (offset (0, 50), limit_bottom 690) shows 50 px more below its
    // bottom limit than a clamp of `target + offset` would allow.
    // `target` is the camera's origin WITHOUT the offset; centres handed in or
    // published are screen centres WITH it.
    let offset = node2d::get_camera_offset(tree, camera);
    match (previous_center, published_camera2d_center(tree, camera)) {
        (Some(prev), _) => {
            node2d::compute_camera2d_position(config, prev - offset, target, viewport_size, dt)
                + offset
        }
        (None, Some(published)) => published,
        (None, None) => {
            node2d::compute_camera2d_position(config, target, target, viewport_size, dt) + offset
        }
    }
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
    let target = node2d::get_global_transform(tree, camera).origin;
    Some(resolve_camera2d_center(
        tree,
        camera,
        &config,
        target,
        viewport_size,
        previous_center,
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
    // The back buffer a canvas_item shader samples as `SCREEN_TEXTURE`
    // (pat-s3rrv.183). `None` until a BackBufferCopy item runs, because Godot
    // gives an uncopied SCREEN_TEXTURE undefined contents — serving the item's
    // own texture instead, which is what the shared sampler did, hides the
    // missing copy behind plausible pixels.
    let mut screen_texture: Option<gdrender2d::texture::Texture2D> = None;
    for item in items {
        let xform = item.global_transform;
        // `clip_children` (pat-0q2kx.39): the software rasterizer has no scissor to
        // set, so a clipped item draws normally and everything it wrote outside its
        // clip rect is restored afterwards. Only clipped subtrees pay the copy.
        let clip_snapshot = item.clip_rect.map(|_| fb.pixels.clone());
        // pat-s3rrv.28: the item's `CanvasItemMaterial` blend mode and its
        // resolved sampling modes apply to EVERY draw the item makes — rects,
        // lines, glyphs and textures alike — because `FrameBuffer::blend_pixel`
        // is the only path to a pixel. Threading them through each `draw::`
        // signature instead would miss whichever routine was added next.
        //
        // Restored on the way out for the same reason the clip snapshot is:
        // items are rasterized flat here, so there is no draw-order stack to
        // unwind. This mirrors `SoftwareRenderer::rasterize_item` exactly.
        let prev_blend = fb.blend_mode;
        let prev_filter = fb.texture_filter;
        let prev_repeat = fb.texture_repeat;
        fb.blend_mode = item
            .canvas_material
            .map(|m| m.blend_mode)
            .unwrap_or(BlendMode::Mix);
        fb.texture_filter = item.texture_filter;
        fb.texture_repeat = item.texture_repeat;
        // The item's canvas_item `fragment()` program, compiled ONCE per item
        // rather than per pixel. `uniforms` merges the shader's declared
        // defaults with the material's runtime parameter overrides, so editing
        // a `shader_parameter/*` changes the pixels.
        let shader_program: Option<(
            Vec<FragmentInstruction>,
            std::collections::HashMap<String, Variant>,
            // Uniform names carrying `: hint_screen_texture` (pat-s3rrv.202).
            // Collected HERE because `sh.uniforms` already holds the parsed
            // hints — re-parsing the source at draw time would be a second
            // source of truth for the same fact.
            Vec<String>,
        )> = item
            .shader_material
            .as_ref()
            .and_then(|m| m.shader.as_ref().map(|sh| (m, sh)))
            .map(|(m, sh)| {
                let instructions = parse_fragment_body(&sh.source_code);
                let mut uniforms: std::collections::HashMap<String, Variant> =
                    std::collections::HashMap::new();
                for u in &sh.uniforms {
                    uniforms.insert(u.name.clone(), u.default_value.clone());
                }
                for (k, v) in &m.parameters {
                    uniforms.insert(k.clone(), v.clone());
                }
                // ⚠️ Godot allows TWO spellings for a back-buffer read: the
                // legacy built-in `SCREEN_TEXTURE`, and a user-named
                // `uniform sampler2D screen_tex : hint_screen_texture`. The
                // sampler is handed the LITERAL name, so a hint-declared
                // uniform that is not listed here falls through to the item's
                // own texture — a sprite sampling itself, which still produces
                // plausible pixels and reads as a working screen effect.
                let screen_uniforms: Vec<String> = sh
                    .uniforms
                    .iter()
                    .filter(|u| {
                        u.hint
                            .as_deref()
                            .is_some_and(|h| h.trim() == "hint_screen_texture")
                    })
                    .map(|u| u.name.clone())
                    .collect();
                (instructions, uniforms, screen_uniforms)
            });
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
                            let src = crate::sprite_frames::sprite_source_rect(
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
                            // ⚠️ Mirror about the RECT'S OWN CENTRE, not the
                            // node origin (pat-s3rrv.22).
                            //
                            // Godot flips a Sprite2D by negating
                            // `dst_rect.size` (`sprite_2d.cpp:124-129`), and
                            // `canvas_item_add_texture_rect_region` normalises
                            // that back to a positive rect plus a
                            // `CANVAS_RECT_FLIP_H` flag
                            // (`renderer_canvas_cull.cpp:1572-1579`). So the
                            // QUAD stays exactly where it was and only the
                            // texture SAMPLING mirrors.
                            //
                            // Scaling by -1 about the origin instead moves an
                            // uncentred sprite to the far side of its node: a
                            // 4px cell placed at x∈[16,20) lands at x∈[12,16),
                            // and every pixel it was meant to cover reads the
                            // clear colour. For `centered = true` the rect is
                            // symmetric about the origin, so the two agree —
                            // which is why this survived every flip test that
                            // used the default.
                            let centre = rect.position + rect.size / 2.0;
                            let flip = Transform2D::translated(centre)
                                * Transform2D::scaled(Vector2::new(
                                    if *flip_h { -1.0 } else { 1.0 },
                                    if *flip_v { -1.0 } else { 1.0 },
                                ))
                                * Transform2D::translated(-centre);
                            // pat-s3rrv.28: an authored canvas_item shader
                            // rewrites COLOR per fragment; without one this is
                            // the plain blit, now carrying the item's resolved
                            // filter/repeat instead of hardcoded nearest-clamp.
                            match &shader_program {
                                Some((instructions, uniforms, screen_uniforms)) => {
                                    draw::draw_texture_rect_transformed_shaded_with_screen(
                                        &mut fb,
                                        tex,
                                        xform * flip,
                                        rect,
                                        Some(src),
                                        item.modulate,
                                        item.texture_filter,
                                        item.texture_repeat,
                                        instructions,
                                        uniforms,
                                        // TIME: this raster entry point is
                                        // handed a finished render list with no
                                        // clock attached, so an animated shader
                                        // renders its t=0 frame rather than a
                                        // wrong one. Feeding real time needs a
                                        // parameter on the public signature and
                                        // is left to its own bead.
                                        0.0,
                                        // `None` unless a BackBufferCopy ran
                                        // earlier in this frame's draw order.
                                        screen_texture.as_ref(),
                                        screen_uniforms,
                                    );
                                }
                                None => {
                                    draw::draw_texture_rect_transformed_filtered(
                                        &mut fb,
                                        tex,
                                        xform * flip,
                                        rect,
                                        Some(src),
                                        item.modulate,
                                        item.texture_filter,
                                        item.texture_repeat,
                                    );
                                }
                            }
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
                        // A flipped / transposed alternative draws through
                        // its orientation about the cell centre.
                        let cell_xform = if cell.flags != 0 {
                            xform * cell.orientation()
                        } else {
                            xform
                        };
                        draw::draw_texture_rect_transformed_filtered(
                            &mut fb,
                            tex,
                            cell_xform,
                            cell.dest,
                            Some(cell.src),
                            item.modulate,
                            item.texture_filter,
                            item.texture_repeat,
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
                // rasterizer at the authored pixel size (pat-0q2kx.146).
                //
                // ⚠️ The `else` below is no longer "keep the bitmap font" — as
                // of pat-mgixk.66 it is the embedded TTF face too, so BOTH
                // branches rasterize outlines and the only difference is WHICH
                // face. Default-theme text does change appearance by design:
                // that is the bug being fixed.
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
                    // 🔑 The embedded TTF face, not the 5x7 bitmap
                    // (pat-mgixk.66). Godot draws every theme string with a
                    // dynamic FontFile (`default_theme.cpp make_default_theme`)
                    // and measures with that same face; Patina measured with
                    // `builtin_face` and drew with `BitmapFont::builtin`, so
                    // every default-theme label in Play was positioned for one
                    // font and painted in another — at widths that disagreed
                    // with the layout that placed it.
                    //
                    // Drawn at the AUTHORED pixel size rather than a quantised
                    // integer scale: the bitmap path had to round `font_size`
                    // onto a cell-height multiple, which is why font_size 16 and
                    // 20 rendered identically.
                    with_builtin_face(|ttf, atlas| {
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
                    });
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
            // pat-s3rrv.85. `draw_polyline_shaded` has taken per-vertex widths
            // and colours since pat-0q2kx.191 with ZERO non-test callers: the
            // rasterizer did not need writing, only wiring. Points are
            // transformed here because the gdrender2d entry point takes
            // screen-space vertices, the same as the `Mesh` arm below.
            Render2DKind::LineShaded {
                points,
                widths,
                colors,
                closed,
                round_cap,
                round_joint,
            } => {
                if points.len() >= 2 && widths.len() == points.len() && colors.len() == points.len()
                {
                    let pts: Vec<Vector2> = points.iter().map(|p| xform.xform(*p)).collect();
                    let cols: Vec<Color> =
                        colors.iter().map(|c| modulate(*c, item.modulate)).collect();
                    gdrender2d::polygon_raster::draw_polyline_shaded(
                        &mut fb,
                        &pts,
                        widths,
                        &cols,
                        *closed,
                        if *round_cap {
                            gdrender2d::polygon_raster::LineCap::Round
                        } else {
                            gdrender2d::polygon_raster::LineCap::None
                        },
                        if *round_joint {
                            gdrender2d::polygon_raster::LineJoint::Round
                        } else {
                            gdrender2d::polygon_raster::LineJoint::Sharp
                        },
                    );
                }
            }
            Render2DKind::PolygonTextured {
                points,
                uvs,
                texture,
                color,
            } => {
                let pts: Vec<Vector2> = points.iter().map(|p| xform.xform(*p)).collect();
                let mut drawn = false;
                if let Some(cache) = texture_cache.as_deref_mut() {
                    if let Some(tex) = cache.get(texture) {
                        if tex.width > 0 && tex.height > 0 {
                            // Godot authors Polygon2D UVs in TEXTURE PIXELS and
                            // divides by the texture size before handing them to
                            // the renderer (`polygon_2d.cpp:232`). The rasterizer
                            // clamps to 0..1, so skipping this would collapse
                            // every texel coordinate onto one edge of the image.
                            let (tw, th) = (tex.width as f32, tex.height as f32);
                            let norm: Vec<Vector2> = uvs
                                .iter()
                                .map(|uv| Vector2::new(uv.x / tw, uv.y / th))
                                .collect();
                            gdrender2d::polygon_raster::fill_polygon_textured(
                                &mut fb, &pts, &norm, tex,
                            );
                            drawn = true;
                        }
                    }
                }
                if !drawn {
                    // No cache, or the texture failed to load. Godot still shows
                    // the polygon tinted by `color`, and falling through to
                    // nothing would make a textured Polygon2D VANISH where the
                    // untextured one drew fine.
                    fill_polygon(&mut fb, xform, points, modulate(*color, item.modulate));
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
                                    draw::draw_texture_rect_transformed_filtered(
                                        &mut fb,
                                        tex,
                                        xform,
                                        rect,
                                        None,
                                        modulate(quad.color, item.modulate),
                                        item.texture_filter,
                                        item.texture_repeat,
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
            Render2DKind::MultiMesh {
                texture,
                vertices,
                uvs,
                instances,
            } => {
                if vertices.len() >= 3 {
                    for (instance, color) in instances {
                        // `a * b` applies `b` first, so this puts the instance
                        // in the node's space and then into canvas space.
                        let composed = xform * *instance;
                        let pts: Vec<Vector2> =
                            vertices.iter().map(|v| composed.xform(*v)).collect();
                        let tinted = modulate(*color, item.modulate);
                        let drawn = texture_cache
                            .as_deref_mut()
                            .and_then(|cache| cache.get(texture))
                            .map(|tex| {
                                gdrender2d::polygon_raster::fill_polygon_textured(
                                    &mut fb, &pts, uvs, tex,
                                )
                            })
                            .is_some();
                        // Untextured instances still have to appear — a
                        // MultiMesh of flat-coloured quads is a normal scene,
                        // and silently drawing nothing would look identical to
                        // the Marker fallthrough this arm exists to replace.
                        if !drawn {
                            fill_polygon(&mut fb, Transform2D::IDENTITY, &pts, tinted);
                        }
                    }
                }
            }
            Render2DKind::BackBufferCopy { copy_mode, .. } => {
                // COPY_MODE_DISABLED (0) is a real authored state and must not
                // refresh the buffer.
                if *copy_mode != 0 {
                    // The whole frame is captured even in COPY_MODE_RECT.
                    // SCREEN_TEXTURE is addressed in full-screen SCREEN_UV, so a
                    // partial buffer would need the sampler to know the copied
                    // region to remap; Godot leaves reads outside the copied
                    // rect undefined, so a superset is permissive rather than
                    // wrong. Honouring `rect` exactly is a follow-up.
                    screen_texture = Some(gdrender2d::texture::Texture2D {
                        width: fb.width,
                        height: fb.height,
                        pixels: fb.pixels.clone(),
                    });
                }
            }
            Render2DKind::Marker { .. } => {}
        }
        if let (Some(rect), Some(before)) = (item.clip_rect, clip_snapshot) {
            restore_outside_clip(&mut fb, rect, &before);
        }
        fb.blend_mode = prev_blend;
        fb.texture_filter = prev_filter;
        fb.texture_repeat = prev_repeat;
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
/// (`crate::light2d_scene`) with **no production caller** — the Play canvas
/// never shaded, so a lit scene rendered exactly like an unlit one.
///
/// ⚠️ The light pass is gated on the scene actually containing a light. With no
/// lights, `light_factor_at` returns the ambient term alone — a dim 0.15 grey —
/// so running it unconditionally would darken every unlit scene to a sixth of
/// its brightness. "No lights" has to mean "no shading", not "ambient only".
pub fn shade_runtime_canvas(tree: &SceneTree, fb: &mut FrameBuffer) {
    shade_runtime_canvas_with_surfaces(tree, fb, &[]);
}

/// [`shade_runtime_canvas`] with per-pixel surface normals from
/// [`canvas_surface_buffer`] (pat-s3rrv.203).
///
/// An empty `surfaces` slice is exactly the old behaviour — every lookup misses
/// and every pixel takes the flat path — so this is a superset rather than a
/// replacement, and a caller with no texture cache loses nothing.
pub fn shade_runtime_canvas_with_surfaces(
    tree: &SceneTree,
    fb: &mut FrameBuffer,
    surfaces: &[Option<gdrender2d::light2d::SurfaceSample>],
) {
    let lights = crate::light2d_scene::collect_lights_2d(tree);
    if !lights.is_empty() {
        let occluders = crate::light2d_scene::collect_occluders_2d(tree);
        // Godot culls lights per canvas item, comparing the *item's* `light_mask`
        // against the light's `range_item_cull_mask`, so the mask varies across
        // the frame (pat-0q2kx.130). The buffer starts at layer 1 — the default
        // a fresh CanvasItem is on, and what this pass used to assume for the
        // whole frame — and each item stamps its own mask over its own pixels.
        let masks = item_light_mask_buffer(tree, fb.width, fb.height);
        gdrender2d::light2d::apply_lights_with_surfaces(
            fb,
            &masks,
            surfaces,
            &lights,
            &occluders,
            gdrender2d::light2d::CanvasAmbient::default(),
        );
    }
    // CanvasModulate tints the finished frame whether or not anything is lit.
    if let Some(color) = crate::light2d_scene::collect_canvas_modulate(tree) {
        crate::light2d_scene::apply_canvas_modulate(fb, color);
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
/// Bakes every `NoiseTexture2D` in the tree into the texture cache
/// (pat-s3rrv.208).
///
/// `gdresource::noise` has had a working FastNoiseLite core since
/// pat-nzgdu.219 with **no caller anywhere in the workspace** — the classic
/// orphaned module. This is the binding: a Sprite2D whose texture is a
/// NoiseTexture2D resolves to a `noise://<node>` key (see
/// `crate::node2d::get_texture_path`), and this fills that key in before the
/// frame is rasterized, so the ordinary sprite path draws generated pixels
/// without knowing they were generated.
///
/// Idempotent: an entry already present is left alone, so the bake happens once
/// per node rather than once per frame.
pub fn bake_noise_textures(tree: &SceneTree, texture_cache: &mut TextureCache) {
    use gdresource::noise::{fractal_type_from_godot, noise_type_from_godot, FastNoiseLite};

    for nid in tree.all_nodes_in_tree_order() {
        let Some(node) = tree.get_node(nid) else {
            continue;
        };
        if !matches!(
            node.get_property("__noise_tex__class"),
            Variant::String(ref c) if c == "NoiseTexture2D"
        ) {
            continue;
        }
        // ⚠️ A NoiseTexture2D whose `noise` is null generates NOTHING. Godot
        // returns an empty image from `_generate_texture` in that case
        // (`noise_texture_2d.cpp`), so baking a default field here would invent
        // pixels the author never asked for — and would make "the sprite shows
        // a varying field" true even when no generator was ever attached,
        // which is precisely the assertion this feature is tested by.
        if !matches!(node.get_property("__noise__class"), Variant::String(_)) {
            continue;
        }
        let key = format!("{}{}", crate::node2d::NOISE_TEXTURE_SCHEME, nid.raw());
        if texture_cache.get(&key).is_some() {
            continue;
        }

        let int_prop = |k: &str, default: i64| match node.get_property(k) {
            Variant::Int(i) => i,
            _ => default,
        };
        let float_prop = |k: &str, default: f32| match node.get_property(k) {
            Variant::Float(f) => f as f32,
            Variant::Int(i) => i as f32,
            _ => default,
        };

        // Godot's documented defaults, used when the .tscn leaves a property
        // out — which is the common case, since the editor only writes
        // non-default values.
        let (noise_type, _exact) = noise_type_from_godot(int_prop("__noise__noise_type", 1));
        let noise = FastNoiseLite::new()
            .seed(int_prop("__noise__seed", 0) as i32)
            .frequency(float_prop("__noise__frequency", 0.01))
            .noise_type(noise_type)
            .fractal(fractal_type_from_godot(int_prop(
                "__noise__fractal_type",
                1,
            )))
            .octaves(int_prop("__noise__fractal_octaves", 5) as i32)
            .lacunarity(float_prop("__noise__fractal_lacunarity", 2.0))
            .gain(float_prop("__noise__fractal_gain", 0.5))
            .weighted_strength(float_prop("__noise__fractal_weighted_strength", 0.0))
            .ping_pong_strength(float_prop("__noise__fractal_ping_pong_strength", 2.0));

        // ⚠️ Godot's default is 512x512. Honouring that literally would bake a
        // quarter of a million samples per texture during a test frame, so the
        // authored size is used as-is but clamped to something a frame can
        // afford; a scene that really wants 512 still gets 512.
        let width = int_prop("__noise_tex__width", 512).clamp(1, 2048) as u32;
        let height = int_prop("__noise_tex__height", 512).clamp(1, 2048) as u32;
        let invert = matches!(
            node.get_property("__noise_tex__invert"),
            Variant::Bool(true)
        );

        let l8 = noise.bake_image_l8(width, height);
        let pixels = l8
            .into_iter()
            .map(|v| {
                let f = if invert {
                    1.0 - v as f32 / 255.0
                } else {
                    v as f32 / 255.0
                };
                Color::new(f, f, f, 1.0)
            })
            .collect();
        texture_cache.insert(
            &key,
            gdrender2d::texture::Texture2D {
                width,
                height,
                pixels,
            },
        );
    }
}

/// Builds the per-pixel surface buffer the normal-mapped light pass consumes
/// (pat-s3rrv.203).
///
/// Only items whose `texture` was a `CanvasTexture` with a `normal_texture`
/// contribute; every other pixel stays `None` and takes the ordinary flat
/// shading path. That distinction is load-bearing rather than an optimisation —
/// see `gdrender2d::light2d::apply_lights_with_surfaces`, where substituting a
/// flat normal would black out every plain sprite under a default-height light.
///
/// Like [`item_light_mask_buffer`], the normal is stamped over the item's
/// **axis-aligned screen extent**, so a rotated sprite's normals spill into its
/// corners. Same deliberate bound on scope, for the same reason: per-pixel
/// coverage would mean threading a surface through every `draw::*` call.
///
/// ⚠️ The normal is NOT rotated into the item's frame. Godot does that
/// (`canvas.glsl:704` multiplies by the item transform), so a rotated
/// normal-mapped sprite lights differently here than it does there. Recorded
/// rather than silently approximated: it needs the same per-pixel plumbing the
/// bound above defers.
pub fn canvas_surface_buffer(
    tree: &SceneTree,
    items: &[Render2DItem],
    width: u32,
    height: u32,
    mut texture_cache: Option<&mut TextureCache>,
) -> Vec<Option<gdrender2d::light2d::SurfaceSample>> {
    let mut surfaces = vec![None; (width as usize) * (height as usize)];
    let Some(cache) = texture_cache.as_deref_mut() else {
        return surfaces;
    };

    // 🔑 The CALLER's items, not a fresh `collect_canvas_items(tree)` walk
    // (pat-mgixk.45). The samples have to land on the pixels the rasterizer
    // actually painted, and by this point the caller has already pushed those
    // items through `apply_camera2d_view` and `apply_stretch_transform`. Walking
    // the tree again rebuilt them in UNTRANSFORMED canvas space, so under the
    // Play path's content-scale stretch a sprite drawn at window x 396..524 had
    // its normal map written at x 198..262 — disjoint. `surfaces` was `None`
    // everywhere the sprite existed, the light shaded a flat surface, and a
    // normal-mapped frame came out BIT-IDENTICAL to one with the map removed.
    //
    // Reading the items the caller is about to rasterize is also the single
    // source of truth this bead exists to establish: item geometry is decided
    // once, by whoever built the list, and every later pass agrees with it.
    for item in items {
        let node_id = NodeId::from_object_id(gdcore::id::ObjectId::from_raw(item.node_id));
        let Some(node) = tree.get_node(node_id) else {
            continue;
        };
        let normal_path = match node.get_property("__canvas_texture__normal_texture") {
            Variant::String(p) if !p.is_empty() => p,
            _ => continue,
        };
        // Specular is independently optional: a CanvasTexture may carry a normal
        // map and no specular map, in which case Godot shades plain Lambert.
        let specular = match node.get_property("__canvas_texture__specular_texture") {
            Variant::String(ref p) if !p.is_empty() => {
                Some(gdrender2d::light2d::SpecularShininess {
                    color: match node.get_property("__canvas_texture__specular_color") {
                        Variant::Color(c) => c,
                        _ => Color::new(1.0, 1.0, 1.0, 1.0),
                    },
                    shininess: match node.get_property("__canvas_texture__specular_shininess") {
                        Variant::Float(f) => f as f32,
                        Variant::Int(i) => i as f32,
                        _ => 1.0,
                    },
                })
            }
            _ => None,
        };

        // Sprites fall through to the cache-aware helper: `item_screen_bounds`
        // has no texture size and so returns `None` for every one of them.
        let bounds = match item_screen_bounds(item) {
            Some(b) => b,
            None => match sprite_screen_bounds(item, cache) {
                Some(b) => b,
                None => continue,
            },
        };
        let Some(tex) = cache.get(&normal_path) else {
            continue;
        };
        if tex.width == 0 || tex.height == 0 || bounds.size.x <= 0.0 || bounds.size.y <= 0.0 {
            continue;
        }

        let x0 = bounds.position.x.floor().max(0.0) as u32;
        let y0 = bounds.position.y.floor().max(0.0) as u32;
        let x1 = ((bounds.position.x + bounds.size.x).ceil().max(0.0) as u32).min(width);
        let y1 = ((bounds.position.y + bounds.size.y).ceil().max(0.0) as u32).min(height);
        for y in y0..y1 {
            for x in x0..x1 {
                let u = (x as f32 + 0.5 - bounds.position.x) / bounds.size.x;
                let v = (y as f32 + 0.5 - bounds.position.y) / bounds.size.y;
                let tx = ((u * tex.width as f32) as u32).min(tex.width - 1);
                let ty = ((v * tex.height as f32) as u32).min(tex.height - 1);
                surfaces[(y * width + x) as usize] = Some(gdrender2d::light2d::SurfaceSample {
                    normal: gdrender2d::light2d::decode_normal_map(tex.get_pixel(tx, ty)),
                    specular,
                });
            }
        }
    }
    surfaces
}

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
        Render2DKind::Polygon { points, .. }
        | Render2DKind::Line { points, .. }
        | Render2DKind::LineShaded { points, .. }
        | Render2DKind::PolygonTextured { points, .. } => {
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

/// Screen bounds for a `Sprite2D` item, which [`item_screen_bounds`] cannot
/// give (pat-s3rrv.203).
///
/// 🔑 A sprite's extent is its TEXTURE's size, so bounding one needs the cache.
/// `item_screen_bounds` deliberately has none and returns `None` for every
/// sprite — which meant [`canvas_surface_buffer`] skipped every sprite, left the
/// surface buffer entirely `None`, and normal-mapped sprites shaded **exactly**
/// like flat ones. Bit-identical output, not merely wrong output.
///
/// ⚠️ The local rect is computed the same way the rasterizer computes it
/// (`sprite_source_rect` for the region/sheet cell, then Godot's `centered`
/// rule). Deriving it independently is how the lit extent and the drawn quad
/// drift apart.
fn sprite_screen_bounds(item: &Render2DItem, cache: &mut TextureCache) -> Option<Rect2> {
    let Render2DKind::Sprite {
        texture,
        offset,
        sheet,
        ..
    } = &item.kind
    else {
        return None;
    };
    // ⚠️ Copy the dimensions out immediately: `get` borrows the cache, and the
    // caller needs it again for the normal map on the next line.
    let (tw, th) = {
        let tex = cache.get(texture)?;
        (tex.width as f32, tex.height as f32)
    };
    if tw <= 0.0 || th <= 0.0 {
        return None;
    }
    let src = crate::sprite_frames::sprite_source_rect(
        Vector2::new(tw, th),
        sheet.region_enabled,
        sheet.region_rect,
        sheet.hframes,
        sheet.vframes,
        sheet.frame,
    );
    let (dw, dh) = (src.size.x, src.size.y);
    let local = if sheet.centered {
        Rect2::new(
            Vector2::new(offset.x - dw / 2.0, offset.y - dh / 2.0),
            Vector2::new(dw, dh),
        )
    } else {
        Rect2::new(*offset, Vector2::new(dw, dh))
    };
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

/// 🔑 THE one implementation of "turn a built draw list into a canvas frame".
///
/// Every runtime canvas path funnels through here, which is the whole point
/// (pat-mgixk.45). There used to be THREE pipelines that each remembered a
/// different subset of these steps:
///
/// | step | `render_runtime_canvas` | `render_runtime_frame` | editor.rs Play |
/// |---|---|---|---|
/// | CanvasGroup compositing | yes | **no** | **no** |
/// | NoiseTexture2D bake | yes | **no** | **no** |
/// | normal/specular surfaces | yes | **no** | **no** |
/// | canvas quality (msaa/snap) | **no** | **no** | **no** |
///
/// So `collect_canvas_groups`, `bake_noise_textures`, `canvas_surface_buffer`
/// and the quality settings had no production caller at all, and the tests that
/// drove `render_runtime_canvas` were asserting against a path the editor's Play
/// tab did not run.
///
/// ⚠️ The draw list is a PARAMETER rather than built here, and that is the
/// constraint the whole shape follows from: the themed Control pass
/// ([`build_control_render_items`]) needs `&mut SceneTree`, which
/// `render_runtime_canvas`'s callers cannot give — they hold the tree
/// immutably. So each entry builds the list it can and hands it over; only the
/// steps below are shared, and those are exactly the ones that were being
/// skipped.
///
/// Step ORDER matters and is not arbitrary:
/// 1. the noise bake must populate the cache before anything looks a texture up;
/// 2. the surface sample must happen before the cache moves into the rasterizer
///    (it is an `Option<&mut _>`, so the reborrow has to be taken here);
/// 3. shading runs last, over the composited frame.
/// `pub` because a caller in another crate can need it: `gdeditor`'s
/// `scene_renderer::render_runtime_frame` composites its canvas over a 3D frame
/// and so needs its own transparent clear, and it builds its own list. Without
/// this being reachable it would keep its own copy of the four steps — which is
/// how the drift this bead fixes happened in the first place.
pub fn render_canvas_from_list(
    tree: &SceneTree,
    items: &[Render2DItem],
    width: u32,
    height: u32,
    clear: Color,
    mut texture_cache: Option<&mut TextureCache>,
    quality: &CanvasQuality,
) -> FrameBuffer {
    // CanvasGroup subtrees composite as one layer (pat-s3rrv.183). With no
    // CanvasGroup in the scene this is byte-for-byte the plain path — the
    // groups-aware entry delegates straight back to the quality rasterizer.
    let groups = collect_canvas_groups(tree);
    if let Some(cache) = texture_cache.as_deref_mut() {
        bake_noise_textures(tree, cache);
    }
    let surfaces = canvas_surface_buffer(tree, items, width, height, texture_cache.as_deref_mut());
    let mut fb = rasterize_render_list_with_groups_and_quality(
        items,
        &groups,
        width,
        height,
        clear,
        texture_cache,
        quality,
    );
    shade_runtime_canvas_with_surfaces(tree, &mut fb, &surfaces);
    fb
}

/// Convenience: [`build_runtime_render_list`] + [`render_canvas_from_list`] in one
/// call, for callers whose `tree` and `texture_cache` don't come from the same
/// borrow (tests, and the texture-less `render_runtime_frame` 2D branch).
///
/// Also runs the light pass, so the frame this produces is the *shaded* one —
/// that is part of rendering a canvas, not an extra step each caller has to
/// remember (pat-0q2kx.103).
///
/// ⚠️ Takes `&SceneTree`, so it cannot run the themed Control pass or read the
/// project's canvas quality. [`render_play_frame`] is the full-fidelity entry;
/// this one is deliberately kept for the many callers that only have an
/// immutable tree and no `ProjectSettings`. Behaviour is unchanged from before
/// pat-mgixk.45.
pub fn render_runtime_canvas(
    tree: &SceneTree,
    width: u32,
    height: u32,
    clear: Color,
    texture_cache: Option<&mut TextureCache>,
) -> FrameBuffer {
    let items = build_runtime_render_list(tree);
    render_canvas_from_list(
        tree,
        &items,
        width,
        height,
        clear,
        texture_cache,
        &CanvasQuality::default(),
    )
}

/// The frame the editor's Play tab publishes, and the single entry point for a
/// running 2D canvas (pat-mgixk.45).
///
/// This is what `examples/editor.rs` calls once per Play tick. It is the only
/// path that runs ALL of it:
///
/// 1. [`build_runtime_render_list`] — the scene's CanvasItems, with parallax;
/// 2. [`build_control_render_items`] — the themed Control pass, which resolves
///    container layout and is why this takes `&mut SceneTree`. Control items
///    carry `CONTROL_Z_BASE`-offset z, so appending keeps the list ascending and
///    UI composites above the plain 2D nodes;
/// 3. [`camera2d_view_transform`] — the scene's own Camera2D scroll/zoom;
/// 4. [`apply_stretch_transform`] — the root Window's content scale, the
///    OUTERMOST transform, applied after the camera and to CanvasLayer items
///    too (pat-nzgdu.87);
/// 5. [`render_canvas_from_list`] with [`CanvasQuality::from_project_settings`] —
///    CanvasGroups, the noise bake, normal/specular surfaces, `msaa_2d` and
///    `snap_2d_*`, then the light pass.
///
/// Transform order is the part worth not getting wrong: camera THEN stretch.
/// The content scale maps the game's base resolution onto the window, so it has
/// to sit outside the camera — swapping them scales the scroll offset instead of
/// the composed canvas.
///
/// Clears to [`RUNTIME_CLEAR_COLOR`]; a caller compositing over a 3D frame needs
/// a transparent clear and so goes through [`render_canvas_from_list`] with its own.
/// The size of the canvas coordinate space a running game lays itself out in:
/// Godot's `Viewport.get_visible_rect().size`. Root-level Controls anchor to it
/// and a Camera2D centres on it.
///
/// 🔑 Under stretch this is the project's BASE resolution, not the window: the
/// stretch transform maps canvas → window afterwards. Laying Controls out
/// against the window and then stretching them scaled the layout twice — a
/// 480×720 `canvas_items` game's full-rect background and centred HUD spilled
/// off the right of the Game view. With stretch off it IS the window size, so
/// a non-stretched game is unchanged.
pub fn canvas_size_of(window: &crate::viewport::RootWindow) -> Vector2 {
    if window.is_stretching() {
        window.visible_size()
    } else {
        window.size
    }
}

/// The node property recording which viewport's `World2D` a SubViewport draws,
/// written by `viewport.world_2d = other.world_2d` (pat-8mb8y). Holds the raw
/// id of the viewport that OWNS the world; absent means the viewport's own.
pub const WORLD_2D_OWNER_PROP: &str = "__world_2d_owner__";

/// The viewport whose `World2D` `vp` renders (`None` = the root window's).
fn world_2d_owner(tree: &SceneTree, vp: Option<crate::NodeId>) -> Option<crate::NodeId> {
    let vp = vp?;
    match tree.get_node(vp).map(|n| n.get_property(WORLD_2D_OWNER_PROP)) {
        Some(Variant::Int(raw)) if raw > 0 => {
            let owner = crate::NodeId::from_object_id(gdcore::id::ObjectId::from_raw(raw as u64));
            if crate::behavioral_2d::is_sub_viewport(tree, owner) {
                Some(owner)
            } else {
                // The root window's world.
                None
            }
        }
        // Owner 0: the root window's world was assigned.
        Some(Variant::Int(0)) => None,
        _ => Some(vp),
    }
}

/// Composites every SubViewport shown by a `SubViewportContainer` into the
/// root frame, Godot's split-screen model (pat-8mb8y).
///
/// Godot renders each SubViewport separately — its World2D's canvas under ITS
/// current Camera2D at ITS size, plus its own CanvasLayers — and the container
/// draws the result at its rect, scaled by `stretch_shrink`
/// (`subviewport_container.cpp`). Before this the whole tree was one flat
/// canvas under one camera, so the 2D platformer's split-screen scene showed a
/// single full-width view.
///
/// Here, per displayed SubViewport `V`:
/// - the world items are every DEFAULT-canvas item of any viewport sharing
///   `V`'s World2D (`viewport_2.world_2d = viewport_1.world_2d` makes both
///   draw Viewport1's level), and the layer items are `V`'s own CanvasLayers;
/// - `V`'s camera view applies to them (as on the root canvas, so a
///   ParallaxBackground and a fixed HUD behave as they do there);
/// - they are moved to the container's rect, scaled by the shrink, clipped to
///   it, and spliced into the root list where the container draws.
///
/// Root items get the ROOT viewport's camera, which is no longer whichever
/// camera happened to be last in the tree. A SubViewport no container shows (a
/// render target only) contributes nothing to the frame.
///
/// ⚠️ Items arrive in the merged, globally sorted order; filtering preserves
/// it, so each viewport's own `(layer, z)` order holds inside its slot.
fn compose_sub_viewports(
    tree: &SceneTree,
    items: Vec<Render2DItem>,
    sub_viewports: &[crate::NodeId],
    canvas_size: Vector2,
    stretch: Transform2D,
) -> Vec<Render2DItem> {
    use crate::behavioral_2d::{current_camera2d_in, enclosing_sub_viewport, sub_viewport_size};
    let node_of =
        |raw: u64| crate::NodeId::from_object_id(gdcore::id::ObjectId::from_raw(raw));
    let item_viewport = |item: &Render2DItem| {
        let id = node_of(item.node_id);
        if tree.get_node(id).is_none() {
            return None;
        }
        if crate::behavioral_2d::is_sub_viewport(tree, id) {
            return Some(id);
        }
        enclosing_sub_viewport(tree, id)
    };
    let viewports: Vec<Option<crate::NodeId>> = items.iter().map(&item_viewport).collect();

    // Root canvas: only the root viewport's own camera.
    let mut main: Vec<Render2DItem> = Vec::new();
    for (item, vp) in items.iter().zip(&viewports) {
        if vp.is_none() {
            main.push(item.clone());
        }
    }
    if let Some(cam) = current_camera2d_in(tree, None) {
        let view = camera2d_view_transform_of(tree, cam, canvas_size, None, 1.0 / 60.0);
        apply_camera2d_view(&mut main, view);
    }

    let order: std::collections::HashMap<u64, usize> = tree
        .all_nodes_in_tree_order()
        .into_iter()
        .enumerate()
        .map(|(i, id)| (id.raw(), i))
        .collect();

    for &vp in sub_viewports {
        let Some(container) = tree.get_node(vp).and_then(|n| n.parent()) else {
            continue;
        };
        let shown = tree
            .get_node(container)
            .is_some_and(|n| n.class_name() == "SubViewportContainer")
            && node2d::is_visible_in_tree(tree, container);
        if !shown {
            continue;
        }
        let world = world_2d_owner(tree, Some(vp));
        let mut slot: Vec<Render2DItem> = items
            .iter()
            .zip(&viewports)
            .filter(|(item, item_vp)| match item.canvas_follow {
                CanvasFollow::DefaultCanvas => {
                    item_vp.is_some() && world_2d_owner(tree, **item_vp) == world
                }
                _ => **item_vp == Some(vp),
            })
            .map(|(item, _)| item.clone())
            .collect();

        let size = sub_viewport_size(tree, Some(vp));
        if let Some(cam) = current_camera2d_in(tree, Some(vp)) {
            let view = camera2d_view_transform_of(tree, cam, size, None, 1.0 / 60.0);
            apply_camera2d_view(&mut slot, view);
        }

        // Container placement: `stretch` draws the viewport scaled by
        // `stretch_shrink`; without it the texture is drawn at its own size.
        let rect = control::get_global_rect(tree, container);
        let stretch_on = matches!(
            tree.get_node(container).map(|n| n.get_property("stretch")),
            Some(Variant::Bool(true))
        );
        let shrink = match tree.get_node(container).map(|n| n.get_property("stretch_shrink")) {
            Some(Variant::Int(s)) if stretch_on && s > 1 => s as f32,
            _ => 1.0,
        };
        let place = Transform2D::translated(rect.position) * Transform2D::scaled(Vector2::new(shrink, shrink));
        // The clip is in final pixels, past the stretch transform the caller
        // applies to the positions.
        let clip = transformed_bounds(stretch, rect);
        for item in &mut slot {
            item.global_transform = place * item.global_transform;
            item.clip_rect = Some(match item.clip_rect {
                Some(own) => intersect_rects(own, clip),
                None => clip,
            });
        }

        // Splice where the container draws: after everything before the end of
        // its subtree on its canvas layer, before anything later or higher.
        let layer = canvas_layer_for(tree, container);
        let start = order.get(&container.raw()).copied().unwrap_or(usize::MAX);
        let end = start.saturating_add(subtree_len(tree, container));
        let at = main
            .iter()
            .position(|m| {
                let idx = order.get(&m.node_id).copied().unwrap_or(usize::MAX);
                m.canvas_layer > layer || (m.canvas_layer == layer && idx > end)
            })
            .unwrap_or(main.len());
        main.splice(at..at, slot);
    }
    main
}

/// Number of descendants under `id` (not counting `id`).
fn subtree_len(tree: &SceneTree, id: crate::NodeId) -> usize {
    let mut nodes = Vec::new();
    tree.collect_subtree_top_down(id, &mut nodes);
    nodes.len().saturating_sub(1)
}

/// Axis-aligned bounds of `rect` under `xform`.
fn transformed_bounds(xform: Transform2D, rect: Rect2) -> Rect2 {
    let p = rect.position;
    let s = rect.size;
    let corners = [
        xform.xform(p),
        xform.xform(Vector2::new(p.x + s.x, p.y)),
        xform.xform(Vector2::new(p.x, p.y + s.y)),
        xform.xform(Vector2::new(p.x + s.x, p.y + s.y)),
    ];
    let (mut min, mut max) = (corners[0], corners[0]);
    for c in &corners[1..] {
        min = Vector2::new(min.x.min(c.x), min.y.min(c.y));
        max = Vector2::new(max.x.max(c.x), max.y.max(c.y));
    }
    Rect2::new(min, Vector2::new(max.x - min.x, max.y - min.y))
}

/// The overlap of two rects (zero-sized when they do not meet).
fn intersect_rects(a: Rect2, b: Rect2) -> Rect2 {
    let x0 = a.position.x.max(b.position.x);
    let y0 = a.position.y.max(b.position.y);
    let x1 = (a.position.x + a.size.x).min(b.position.x + b.size.x);
    let y1 = (a.position.y + a.size.y).min(b.position.y + b.size.y);
    Rect2::new(Vector2::new(x0, y0), Vector2::new((x1 - x0).max(0.0), (y1 - y0).max(0.0)))
}

pub fn render_play_frame(
    tree: &mut SceneTree,
    width: u32,
    height: u32,
    texture_cache: &mut TextureCache,
    settings: &gdcore::project_settings::ProjectSettings,
) -> FrameBuffer {
    prepare_play_frame(tree, width, height, texture_cache, settings).rasterize(texture_cache)
}

/// One Play frame with everything it reads from the [`SceneTree`] already
/// gathered: the half of [`render_play_frame`] that needs the tree, so the
/// half that does not — the rasterizer, which is nearly all of a frame's
/// cost — can run without it.
///
/// 🔑 Why this split exists: the web editor rendered the whole frame while
/// holding the editor-state lock, and the game loop needs that same lock to
/// step. On the platformer demo a 995×843 frame took 12–17 ms to rasterize,
/// so the game thread spent a third of each 16.7 ms tick blocked
/// ("game-bound: 53 of 60 frames stepped"), and the audio feed it drives ran
/// dry. The editor now calls [`prepare_play_frame`] under the lock and
/// [`PreparedPlayFrame::rasterize`] after releasing it.
///
/// [`render_play_frame`] is exactly `prepare_play_frame(..).rasterize(..)`,
/// so both paths draw the same pixels.
#[derive(Debug, Clone)]
pub struct PreparedPlayFrame {
    items: Vec<Render2DItem>,
    groups: Vec<CanvasGroup2D>,
    surfaces: Vec<Option<gdrender2d::light2d::SurfaceSample>>,
    lights: Vec<gdrender2d::light2d::Light2D>,
    occluders: Vec<gdrender2d::light2d::LightOccluder2D>,
    light_masks: Vec<u32>,
    canvas_modulate: Option<Color>,
    window: crate::viewport::RootWindow,
    width: u32,
    height: u32,
    clear: Color,
    quality: CanvasQuality,
}

/// The tree half of [`render_play_frame`]; see [`PreparedPlayFrame`].
///
/// Takes the texture cache because the noise bake and the normal/specular
/// surface pass write into or read from it, and both read the tree.
pub fn prepare_play_frame(
    tree: &mut SceneTree,
    width: u32,
    height: u32,
    texture_cache: &mut TextureCache,
    settings: &gdcore::project_settings::ProjectSettings,
) -> PreparedPlayFrame {
    let viewport_size = Vector2::new(width.max(1) as f32, height.max(1) as f32);
    // The window here is the Play surface's rect, not the project's configured
    // one, so the project's base resolution stretches to whatever size the host
    // gave us — which is what makes the Play tab frame the scene the way the
    // exported game would.
    let mut window = *tree.root_window();
    window.size = viewport_size;
    let canvas_size = canvas_size_of(&window);

    // The Control pass FIRST: its layout writes each Control's rect back onto
    // the node, and the runtime list reads that rect — built the other way round
    // a ColorRect drew at last frame's size (zero on the first frame).
    let root = tree.root_id();
    let controls = build_control_render_items_sourced(tree, root, canvas_size);
    let runtime = build_runtime_render_list(tree);
    let mut items = merge_control_items(tree, runtime, controls);

    let sub_viewports = crate::behavioral_2d::sub_viewports(tree);
    if sub_viewports.is_empty() {
        if let Some(view) = camera2d_view_transform(tree, canvas_size, None, 1.0 / 60.0) {
            apply_camera2d_view(&mut items, view);
        }
    } else {
        items = compose_sub_viewports(
            tree,
            items,
            &sub_viewports,
            canvas_size,
            window.stretch_transform(),
        );
    }
    apply_stretch_transform(&mut items, window.stretch_transform());

    // What `render_canvas_from_list` reads from the tree, in its order.
    let groups = collect_canvas_groups(tree);
    bake_noise_textures(tree, texture_cache);
    let surfaces = canvas_surface_buffer(tree, &items, width, height, Some(texture_cache));
    let lights = crate::light2d_scene::collect_lights_2d(tree);
    let (occluders, light_masks) = if lights.is_empty() {
        (Vec::new(), Vec::new())
    } else {
        (
            crate::light2d_scene::collect_occluders_2d(tree),
            item_light_mask_buffer(tree, width, height),
        )
    };

    PreparedPlayFrame {
        items,
        groups,
        surfaces,
        lights,
        occluders,
        light_masks,
        canvas_modulate: crate::light2d_scene::collect_canvas_modulate(tree),
        window,
        width,
        height,
        // The viewport clears to the PROJECT's `rendering/environment/defaults/
        // default_clear_color` (Godot's 0.3 grey only when unset). The fixed grey
        // ignored it, so Pong's near-black (0.106, 0.106, 0.125) showed as a grey
        // band wherever an expanded viewport reached past the authored background.
        clear: settings.default_clear_color(),
        quality: CanvasQuality::from_project_settings(settings),
    }
}

impl PreparedPlayFrame {
    /// The items this frame draws, in draw order, after the camera and stretch
    /// transforms — what a parity check holds the edit view against.
    pub fn items(&self) -> &[Render2DItem] {
        &self.items
    }

    /// The tree-free half of [`render_play_frame`]: rasterize, shade, bars.
    pub fn rasterize(&self, texture_cache: &mut TextureCache) -> FrameBuffer {
        let mut fb = rasterize_render_list_with_groups_and_quality(
            &self.items,
            &self.groups,
            self.width,
            self.height,
            self.clear,
            Some(texture_cache),
            &self.quality,
        );
        // `shade_runtime_canvas_with_surfaces`, from the gathered inputs.
        if !self.lights.is_empty() {
            gdrender2d::light2d::apply_lights_with_surfaces(
                &mut fb,
                &self.light_masks,
                &self.surfaces,
                &self.lights,
                &self.occluders,
                gdrender2d::light2d::CanvasAmbient::default(),
            );
        }
        if let Some(color) = self.canvas_modulate {
            crate::light2d_scene::apply_canvas_modulate(&mut fb, color);
        }
        paint_black_bars(&mut fb, &self.window);
        fb
    }
}

/// Paints the letter/pillarbox bars a `keep`-aspect stretch leaves around the
/// viewport, as Godot does (`RenderingServer.black_bars_set_margins`, black).
///
/// Also the viewport's CLIP: anything a game draws past its base resolution —
/// a sprite half off-screen, a background wider than the view — falls in the
/// bars and must not show, since in Godot it lies outside the viewport. A
/// no-op when nothing stretches or the aspect fits exactly (zero margins).
pub fn paint_black_bars(fb: &mut FrameBuffer, window: &crate::viewport::RootWindow) {
    if !window.is_stretching() {
        return;
    }
    let scale = window.content_scale();
    let (mx, my) = (scale.margin.x.max(0.0), scale.margin.y.max(0.0));
    if mx < 0.5 && my < 0.5 {
        return;
    }
    let (w, h) = (fb.width as f32, fb.height as f32);
    let black = gdcore::math::Color::new(0.0, 0.0, 0.0, 1.0);
    let inner = Rect2::new(Vector2::new(mx, my), scale.screen_size);
    for y in 0..fb.height {
        for x in 0..fb.width {
            let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
            let inside = px >= inner.position.x
                && px < (inner.position.x + inner.size.x).min(w)
                && py >= inner.position.y
                && py < (inner.position.y + inner.size.y).min(h);
            if !inside {
                fb.pixels[(y * fb.width + x) as usize] = black;
            }
        }
    }
}

/// A `CanvasGroup` and the draw-list items that belong to its subtree
/// (pat-s3rrv.183).
#[derive(Debug, Clone, PartialEq)]
pub struct CanvasGroup2D {
    /// The CanvasGroup node's own raw id.
    pub node_id: u64,
    /// The tint applied ONCE to the flattened subtree: the group's
    /// `inherited_modulate` times its own `self_modulate`.
    pub modulate: Color,
    /// What the group's children have ALREADY baked into their own item
    /// modulate — the group's `inherited_modulate`, which is `modulate` up the
    /// chain and excludes `self_modulate`.
    ///
    /// ⚠️ These two differ exactly when the group sets `self_modulate`, which
    /// children do NOT inherit. Stripping children by the full tint would then
    /// remove a factor they never had.
    pub inherited: Color,
    /// Raw ids of every descendant that produces a draw-list item.
    pub members: Vec<u64>,
}

/// Finds every `CanvasGroup` in the tree and the descendants it flattens.
///
/// Godot renders a CanvasGroup's whole subtree into an offscreen buffer and
/// composites that buffer once, so `modulate` applies to the GROUP rather than
/// to each child. The observable difference is at overlaps: two opaque children
/// under a half-transparent group show one uniform half-alpha, where per-child
/// blending shows a darker seam (1 - 0.5² = 0.75) exactly where they cross.
pub fn collect_canvas_groups(tree: &SceneTree) -> Vec<CanvasGroup2D> {
    let mut groups = Vec::new();
    for id in tree.all_nodes_in_tree_order() {
        let is_group = tree
            .get_node(id)
            .map(|n| n.class_name() == "CanvasGroup")
            .unwrap_or(false);
        if !is_group {
            continue;
        }
        let members: Vec<u64> = tree
            .all_nodes_in_tree_order()
            .into_iter()
            .filter(|&other| other != id && is_descendant_of(tree, other, id))
            .map(|n| n.raw())
            .collect();
        if members.is_empty() {
            continue;
        }
        groups.push(CanvasGroup2D {
            node_id: id.raw(),
            modulate: canvas_order::effective_modulate(tree, id),
            inherited: canvas_order::inherited_modulate(tree, id),
            members,
        });
    }
    groups
}

/// Whether `node` sits anywhere under `ancestor`.
fn is_descendant_of(tree: &SceneTree, node: NodeId, ancestor: NodeId) -> bool {
    let mut cur = tree.get_node(node).and_then(|n| n.parent());
    while let Some(p) = cur {
        if p == ancestor {
            return true;
        }
        cur = tree.get_node(p).and_then(|n| n.parent());
    }
    false
}

/// Removes `group`'s contribution from a child's already-composed modulate.
///
/// ⚠️ `canvas_order::effective_modulate` multiplies the WHOLE ancestor chain,
/// so a CanvasGroup's modulate is already baked into every child item. Leaving
/// it there and also tinting at composite time applies it TWICE — a group at
/// alpha 0.5 would render its children at 0.25. Since the chain is a pure
/// product, dividing the group's factor back out is exact.
///
/// A zero component means the group hides the subtree entirely, so the child
/// contributes nothing and 0 is the right answer rather than a division by zero.
fn strip_group_modulate(child: Color, group: Color) -> Color {
    let undo = |c: f32, g: f32| {
        if g > 0.0 {
            (c / g).clamp(0.0, 1.0)
        } else {
            0.0
        }
    };
    Color::new(
        undo(child.r, group.r),
        undo(child.g, group.g),
        undo(child.b, group.b),
        undo(child.a, group.a),
    )
}

/// Blends `scratch` over `fb` once, tinted by `tint`.
///
/// This single pass is what makes a CanvasGroup a group: the subtree has
/// already been flattened into `scratch`, so overlapping children have resolved
/// against each other at full opacity and the tint applies to the result.
fn composite_group_buffer(fb: &mut FrameBuffer, scratch: &FrameBuffer, tint: Color) {
    let w = fb.width.min(scratch.width);
    let h = fb.height.min(scratch.height);
    for y in 0..h {
        for x in 0..w {
            let src = scratch.pixels[(y * scratch.width + x) as usize];
            if src.a <= 0.0 {
                continue;
            }
            fb.blend_pixel(
                x,
                y,
                Color::new(
                    src.r * tint.r,
                    src.g * tint.g,
                    src.b * tint.b,
                    src.a * tint.a,
                ),
            );
        }
    }
}

/// Rasterizes `items`, compositing each `CanvasGroup` subtree as one layer
/// (pat-s3rrv.183).
///
/// Additive on purpose: [`rasterize_render_list`] keeps its signature and its
/// behaviour, and this is the entry point that knows about groups. Adding a
/// group field to [`Render2DItem`] instead would have broken all fifteen of its
/// construction sites, thirteen of them in other agents' test files.
///
/// A group composites at the position of its FIRST member in the sorted list,
/// which is where the group's own z-order puts it.
pub fn rasterize_render_list_with_groups(
    items: &[Render2DItem],
    groups: &[CanvasGroup2D],
    width: u32,
    height: u32,
    clear: Color,
    texture_cache: Option<&mut TextureCache>,
) -> FrameBuffer {
    rasterize_render_list_with_groups_and_quality(
        items,
        groups,
        width,
        height,
        clear,
        texture_cache,
        &CanvasQuality::default(),
    )
}

/// [`rasterize_render_list_with_groups`] honouring the viewport's canvas-quality
/// settings — the two axes compose, so a scene can have both CanvasGroups and
/// `msaa_2d`/`snap_2d_*` (pat-mgixk.45).
///
/// Exists because the groups entry and the quality entry were parallel
/// specialisations of the same rasterizer with no way to ask for both. The Play
/// path needs both, so this is the one that takes everything and
/// [`rasterize_render_list_with_groups`] delegates here with the default
/// quality — byte-identical to its old body.
pub fn rasterize_render_list_with_groups_and_quality(
    items: &[Render2DItem],
    groups: &[CanvasGroup2D],
    width: u32,
    height: u32,
    clear: Color,
    mut texture_cache: Option<&mut TextureCache>,
    quality: &CanvasQuality,
) -> FrameBuffer {
    if groups.is_empty() {
        return rasterize_render_list_with_quality(
            items,
            width,
            height,
            clear,
            texture_cache,
            quality,
        );
    }

    // Split the draw list into ordered layers: runs of ungrouped items, and one
    // layer per group at the position of its first member.
    enum Layer {
        Plain(Vec<Render2DItem>),
        Group(usize, Vec<Render2DItem>),
    }
    let group_of = |node_id: u64| groups.iter().position(|g| g.members.contains(&node_id));

    let mut layers: Vec<Layer> = Vec::new();
    let mut emitted: Vec<usize> = Vec::new();
    for item in items {
        match group_of(item.node_id) {
            Some(gi) => {
                if !emitted.contains(&gi) {
                    emitted.push(gi);
                    layers.push(Layer::Group(gi, Vec::new()));
                }
                if let Some(Layer::Group(_, bucket)) = layers
                    .iter_mut()
                    .find(|l| matches!(l, Layer::Group(g, _) if *g == gi))
                {
                    let mut stripped = item.clone();
                    // Strip what the child actually inherited, not the full
                    // composite tint.
                    stripped.modulate = strip_group_modulate(item.modulate, groups[gi].inherited);
                    bucket.push(stripped);
                }
            }
            None => match layers.last_mut() {
                Some(Layer::Plain(bucket)) => bucket.push(item.clone()),
                _ => layers.push(Layer::Plain(vec![item.clone()])),
            },
        }
    }

    let transparent = Color::new(0.0, 0.0, 0.0, 0.0);
    let mut fb = FrameBuffer::new(width.max(1), height.max(1), clear);
    for layer in &layers {
        let (batch, tint) = match layer {
            Layer::Plain(batch) => (batch, Color::WHITE),
            Layer::Group(gi, batch) => (batch, groups[*gi].modulate),
        };
        if batch.is_empty() {
            continue;
        }
        // Every layer goes through the SAME rasterizer, so a grouped item draws
        // exactly as it would ungrouped — only the compositing differs.
        let scratch = rasterize_render_list_with_quality(
            batch,
            width.max(1),
            height.max(1),
            transparent,
            texture_cache.as_deref_mut(),
            quality,
        );
        composite_group_buffer(&mut fb, &scratch, tint);
    }
    fb
}

/// Component-wise multiply of a base color by a `modulate` tint.
/// Floats per instance in a `MULTIMESH_TRANSFORM_2D` buffer: a row-major 2x4
/// matrix (pat-s3rrv.183).
const MULTIMESH_XFORM_2D_FLOATS: usize = 8;
/// The per-instance colour block that follows the transform when `use_colors`.
const MULTIMESH_COLOR_FLOATS: usize = 4;

/// Decodes a `MultiMeshInstance2D`'s per-instance transforms and colours out of
/// the `__multimesh__*` companions `packed_scene` flattens onto the node.
///
/// 🔑 THE BUFFER IS ROW-MAJOR AND OUR `Transform2D` IS COLUMN-MAJOR.
/// `mesh_storage.cpp:1927-1934` writes `columns[0][0], columns[1][0], 0,
/// columns[2][0]` then `columns[0][1], columns[1][1], 0, columns[2][1]` — so a
/// basis column is STRIDED out of the buffer, not read contiguously. Reading
/// `f[0..2]` into `x` is the natural-looking version and it is the transpose:
/// invisible for a pure translation, wrong for any rotation or non-uniform
/// scale. The 3D decoder in `crate::node3d` documents the same trap.
fn multimesh_2d_instances(tree: &SceneTree, id: NodeId) -> Vec<(Transform2D, Color)> {
    let Some(node) = tree.get_node(id) else {
        return Vec::new();
    };
    // ⚠️ Godot's `transform_format` DEFAULTS TO 2D (`MultiMesh.xml` gives
    // `default="0"`), so a 2D buffer is the unmarked case. A 3D buffer strides
    // by 12 and would be mis-read at every instance after the first.
    if matches!(
        node.get_property("__multimesh__transform_format"),
        Variant::Int(1)
    ) {
        return Vec::new();
    }
    let buffer = match node.get_property("__multimesh__buffer") {
        Variant::PackedFloat32Array(v) if !v.is_empty() => v,
        _ => return Vec::new(),
    };
    let use_colors = matches!(
        node.get_property("__multimesh__use_colors"),
        Variant::Bool(true)
    );
    let stride = MULTIMESH_XFORM_2D_FLOATS
        + if use_colors {
            MULTIMESH_COLOR_FLOATS
        } else {
            0
        };

    // The authored instance count wins, but is clamped to what the buffer can
    // actually supply: a count larger than the data would otherwise read zeroed
    // transforms and stack degenerate quads at the origin.
    let available = buffer.len() / stride;
    let declared = match node.get_property("__multimesh__instance_count") {
        Variant::Int(v) if v >= 0 => v as usize,
        _ => match node.get_property("instance_count") {
            Variant::Int(v) if v >= 0 => v as usize,
            _ => available,
        },
    };
    let count = declared.min(available);

    (0..count)
        .filter_map(|i| {
            let base = i.checked_mul(stride)?;
            let f = buffer.get(base..base.checked_add(MULTIMESH_XFORM_2D_FLOATS)?)?;
            let transform = Transform2D {
                x: Vector2::new(f[0], f[4]),
                y: Vector2::new(f[1], f[5]),
                origin: Vector2::new(f[3], f[7]),
            };
            let color = if use_colors {
                buffer
                    .get(base + MULTIMESH_XFORM_2D_FLOATS..base + stride)
                    .map(|c| Color::new(c[0], c[1], c[2], c[3]))
                    .unwrap_or(Color::WHITE)
            } else {
                Color::WHITE
            };
            Some((transform, color))
        })
        .collect()
}

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

// ---------------------------------------------------------------------------
// Outline-font cache (pat-0q2kx.146)
// ---------------------------------------------------------------------------
//
// ⚠️ This sat AFTER the `#[cfg(test)] mod tests` block in the module's old home,
// which is why the first pass of the pat-mgixk.28 move left it behind: the split
// assumed the test module ran to the end of the file. It does not — 60 lines of
// real code followed it. `rasterize_render_list` calls this, so `gdscene` did
// not compile.
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
            // Through the configured `res://` root first: the working directory
            // is the project only when the host happened to start there.
            gdcore::file_access::resolve_path(path)
                .ok()
                .and_then(|p| std::fs::read(p).ok())
                .or_else(|| std::fs::read(disk).ok())
                .and_then(|bytes| {
                // pat-s3rrv.214: a WOFF is an SFNT with zlib-compressed tables,
                // and `TtfFont::from_bytes` only understands SFNT — so a `.woff`
                // face parsed as nothing and the Label silently drew the
                // built-in fallback.
                //
                // 🔑 This is the ONLY file-backed font load in the engine
                // (everything else builds a `TtfFont` from embedded bytes), so
                // decoding here covers every path that can name a font on disk.
                // It is also why the decoder stays in `gdresource` rather than
                // being duplicated into `gdrender2d`: those two crates cannot
                // see each other, but `gdeditor` — where this call lives — sees
                // both.
                let sfnt =
                    if gdresource::woff::is_woff1(&bytes) || gdresource::woff::is_woff2(&bytes) {
                        gdresource::woff::woff_to_sfnt(&bytes).ok()?
                    } else {
                        bytes
                    };
                gdrender2d::ttf_font::TtfFont::from_bytes(sfnt).ok()
            })
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

/// Width of `text` at `px_size` in the override face at `path`, or `None` when
/// that face cannot be loaded (the rasterizer then falls back to the built-in
/// face, and so must the measurement). Shares [`with_override_font`]'s cache,
/// so layout measures exactly the face the rasterizer draws.
pub(crate) fn measure_with_override_font(path: &str, text: &str, px_size: f32) -> Option<f32> {
    let mut width = None;
    with_override_font(path, |ttf, _atlas| width = Some(ttf.measure_line(text, px_size)));
    width
}

/// Runs `f` with the embedded default UI face and its glyph atlas
/// (pat-mgixk.66).
///
/// 🔑 The face comes from `control_render::builtin_face`, the SAME accessor
/// layout measures with. Default-theme text used to be measured with that face
/// and drawn with a 5x7 bitmap, so wrapped and centred strings were positioned
/// for one font and painted in another. Sharing one accessor makes measured and
/// drawn widths impossible to drift apart.
///
/// ⚠️ Its OWN atlas, deliberately NOT the one [`with_override_font`] uses.
/// `GlyphAtlas`'s cache key is `(glyph_id, quantized_size)` with no face
/// identity (`gdrender2d/src/ttf_font.rs:299`), so two faces sharing an atlas
/// collide whenever they want the same glyph id at the same size — and glyph id
/// 5 is a different letter in a different font. A separate atlas means wiring
/// the default face cannot corrupt override-font text.
fn with_builtin_face<
    F: FnOnce(&gdrender2d::ttf_font::TtfFont, &mut gdrender2d::ttf_font::GlyphAtlas),
>(
    f: F,
) {
    use std::cell::RefCell;
    thread_local! {
        static BUILTIN_ATLAS: RefCell<gdrender2d::ttf_font::GlyphAtlas> =
            RefCell::new(gdrender2d::ttf_font::GlyphAtlas::new(512, 512));
    }
    let face = crate::control_render::builtin_face();
    BUILTIN_ATLAS.with(|atlas| f(face, &mut atlas.borrow_mut()));
}
