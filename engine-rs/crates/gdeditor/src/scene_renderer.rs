//! Scene renderer for the editor viewport.
//!
//! Renders a visual representation of the scene tree into a [`FrameBuffer`],
//! including a background grid, origin crosshair, rulers, node representations
//! based on class type, node labels, and selection highlighting.
//!
//! Designed to visually approximate Godot 4's 2D viewport appearance.

use gdcore::math::{Color, Rect2, Vector2};
use gdrender2d::draw;
use gdrender2d::renderer::FrameBuffer;
use gdscene::node::NodeId;
use gdscene::SceneTree;
use gdscene::{canvas_order, node2d};
use gdvariant::Variant;

use crate::texture_cache::TextureCache;

// ---------------------------------------------------------------------------
// Colors
// ---------------------------------------------------------------------------

/// Dark background color for the editor viewport.
const BG_COLOR: Color = Color::new(0.08, 0.08, 0.1, 1.0);

/// Subtle grid line color (every 50px).
const GRID_COLOR_MINOR: Color = Color::new(0.1, 0.1, 0.12, 1.0);

/// Brighter grid line color (every 200px).
const GRID_COLOR_MAJOR: Color = Color::new(0.16, 0.16, 0.2, 1.0);

/// Minor grid spacing in pixels.
const GRID_MINOR: u32 = 50;

/// Major grid spacing in pixels.
const GRID_MAJOR: u32 = 200;

// Node representation colors.
const COLOR_NODE2D: Color = Color::new(1.0, 0.75, 0.0, 1.0); // amber
const COLOR_SPRITE2D: Color = Color::new(0.3, 0.5, 1.0, 1.0); // blue
const COLOR_CAMERA2D: Color = Color::new(0.2, 0.9, 0.3, 1.0); // green
const COLOR_COLLISION: Color = Color::new(0.0, 0.85, 0.3, 0.7); // green outline
const COLOR_AREA2D: Color = Color::new(0.3, 0.5, 1.0, 0.15); // blue tint
const COLOR_NAVMESH_FACE: Color = Color::new(0.5, 1.0, 1.0, 0.4); // Godot geometry_face_color
const COLOR_NAVMESH_EDGE: Color = Color::new(0.5, 1.0, 1.0, 1.0); // Godot geometry_edge_color
const COLOR_CHARBODY: Color = Color::new(0.3, 0.5, 1.0, 1.0); // blue outline
const COLOR_RIGIDBODY: Color = Color::new(1.0, 0.85, 0.0, 1.0); // yellow outline
const COLOR_STATICBODY: Color = Color::new(0.5, 0.5, 0.5, 1.0); // gray outline
const COLOR_CONTROL: Color = Color::new(0.7, 0.3, 0.9, 1.0); // purple
const COLOR_DEFAULT: Color = Color::new(0.8, 0.8, 0.8, 1.0); // white-ish
const COLOR_SELECTED: Color = Color::new(1.0, 0.85, 0.0, 1.0); // bright amber
const COLOR_NODE_DOT: Color = Color::new(0.5, 0.5, 0.5, 1.0); // gray
const COLOR_PARTICLES: Color = Color::new(0.9, 0.6, 1.0, 1.0); // violet emitter glyph
const COLOR_LABEL_TEXT: Color = Color::new(0.85, 0.85, 0.85, 0.9); // label text

/// Gizmo colors for the transform arrows.
const GIZMO_X_COLOR: Color = Color::new(1.0, 0.2, 0.2, 1.0); // red
const GIZMO_Y_COLOR: Color = Color::new(0.2, 0.85, 0.2, 1.0); // green
const GIZMO_CENTER_COLOR: Color = Color::new(1.0, 1.0, 0.3, 1.0); // yellow

/// Origin crosshair colors (semi-transparent).
const ORIGIN_X_COLOR: Color = Color::new(1.0, 0.2, 0.2, 0.3); // red, semi-transparent
const ORIGIN_Y_COLOR: Color = Color::new(0.2, 0.85, 0.2, 0.3); // green, semi-transparent

/// Ruler colors.
const RULER_BG: Color = Color::new(0.06, 0.06, 0.08, 1.0);
const RULER_TICK: Color = Color::new(0.3, 0.3, 0.35, 1.0);
const RULER_TEXT: Color = Color::new(0.4, 0.4, 0.45, 1.0);

/// Width of the ruler bars in pixels.
const RULER_SIZE: u32 = 24;

/// Renders the scene tree into a framebuffer for the editor viewport.
///
/// Draws a grid background, visual representations of each node based on
/// its class name, and highlights the selected node if any.
/// Supports zoom and pan: `zoom` multiplies world coordinates, `pan` shifts them.
pub fn render_scene(
    tree: &SceneTree,
    selected: Option<NodeId>,
    width: u32,
    height: u32,
) -> FrameBuffer {
    render_scene_with_zoom_pan(tree, selected, width, height, 1.0, (0.0, 0.0))
}

/// Renders the scene tree with explicit zoom and pan parameters.
///
/// `zoom` scales world coordinates (1.0 = 100%). `pan` is an additional
/// pixel offset applied after zoom.
pub fn render_scene_with_zoom_pan(
    tree: &SceneTree,
    selected: Option<NodeId>,
    width: u32,
    height: u32,
    zoom: f64,
    pan: (f64, f64),
) -> FrameBuffer {
    render_scene_inner(tree, selected, width, height, zoom, pan, None, None)
}

/// Whether `tree` should be drawn through the 3D renderer (rather than the 2D
/// canvas rasterizer), mirroring Godot's main-plugin selection
/// (`EditorNode::set_edited_scene` → `_find_node_types`): the scene root's own
/// type wins — a `Node3D` root is 3D, a `CanvasItem` root is 2D — and a bare
/// `Node` root (a level container) falls back to whichever spatial/canvas node
/// type dominates the descendants. Ties and empty scenes default to 2D
/// (`false`). This is the same rule the editor uses to pick the 2D vs 3D main
/// screen for an opened scene, applied to the *running* scene so the Game view
/// renders it the way the game itself would.
pub fn scene_is_3d(tree: &SceneTree) -> bool {
    let root = tree
        .current_scene()
        .or_else(|| {
            tree.get_node(tree.root_id())
                .and_then(|n| n.children().first().copied())
        })
        .unwrap_or_else(|| tree.root_id());
    if let Some(node) = tree.get_node(root) {
        if node.is_class("Node3D") {
            return true;
        }
        if node.is_class("CanvasItem") {
            return false;
        }
    }
    let (mut count_3d, mut count_2d) = (0u32, 0u32);
    for nid in tree.all_nodes_in_tree_order() {
        if let Some(node) = tree.get_node(nid) {
            if node.is_class("Node3D") {
                count_3d += 1;
            } else if node.is_class("CanvasItem") {
                count_2d += 1;
            }
        }
    }
    count_3d > count_2d
}

/// Renders one frame of the *running* scene for the editor's Game view, picking
/// the renderer the way Godot's game viewport would: a 3D scene is drawn through
/// the spatial renderer (`render_editor_3d_frame`, using the editor's 3D
/// viewport camera so the view matches the spatial editor you launched from),
/// and a 2D scene through the canvas rasterizer.
///
/// This fixes a blank Game view for 3D scenes: the run loop previously always
/// used the 2D rasterizer, which has nothing to draw for a `Node3D` scene (no
/// `CanvasItem`s), so pressing Play on a 3D scene showed an empty grid even
/// though the scene was running. `viewport_3d` is borrowed read-only and cloned
/// internally to apply the panel size, so callers can pass `&state`'s viewport
/// alongside an immutable borrow of the running tree.
pub fn render_runtime_frame(
    tree: &SceneTree,
    viewport_3d: &crate::viewport_3d::Viewport3D,
    width: u32,
    height: u32,
    zoom: f64,
    pan: (f64, f64),
) -> FrameBuffer {
    // The editor's zoom/pan only frame the *editor* preview, so they stay out of
    // the running frame — but the scene's own Camera2D does apply, and used to be
    // discarded here alongside them, which made a scrolling game impossible to
    // see in the Game view (pat-0q2kx.40).
    let _ = (zoom, pan);
    if scene_is_3d(tree) {
        let mut vp = viewport_3d.clone();
        vp.width = width.max(1);
        vp.height = height.max(1);
        crate::viewport_3d_render::render_editor_3d_frame(&vp, tree)
    } else {
        // 2D running scene: draw the real CanvasItem pixels the game would show
        // (pat-u3l8c.23) — no editor grid/rulers/gizmos. This branch is
        // texture-less; the texture-cached Play path lives in `examples/editor.rs`,
        // so sprites here fall back to placeholder squares.
        // Inlines `render_runtime_canvas` so the Camera2D view can be folded in
        // between building the list and rasterizing it — the same order the Play
        // loop in `examples/editor.rs` uses. `apply_camera2d_view` skips
        // CanvasLayer items, so HUD pixels stay put while the world scrolls.
        let mut items = crate::viewport_2d_render::build_runtime_render_list(tree);
        let viewport_size = Vector2::new(width.max(1) as f32, height.max(1) as f32);
        if let Some(view) = crate::viewport_2d_render::camera2d_view_transform(
            tree,
            viewport_size,
            None,
            1.0 / 60.0,
        ) {
            crate::viewport_2d_render::apply_camera2d_view(&mut items, view);
        }
        // The root window's content scale is the OUTERMOST transform, applied
        // after the camera and to CanvasLayer items too (pat-nzgdu.87). The
        // window here is the Game view's rect, not the project's configured
        // one, so the project's base resolution is stretched to whatever size
        // the editor gave us — which is what makes the Game view show the same
        // framing the exported game would.
        let mut window = *tree.root_window();
        window.size = viewport_size;
        crate::viewport_2d_render::apply_stretch_transform(&mut items, window.stretch_transform());
        crate::viewport_2d_render::rasterize_render_list(
            &items,
            width,
            height,
            crate::viewport_2d_render::RUNTIME_CLEAR_COLOR,
            None,
        )
    }
}

/// Renders with texture support for Sprite2D nodes.
pub fn render_scene_with_textures(
    tree: &SceneTree,
    selected: Option<NodeId>,
    width: u32,
    height: u32,
    zoom: f64,
    pan: (f64, f64),
    texture_cache: &mut TextureCache,
) -> FrameBuffer {
    render_scene_inner(
        tree,
        selected,
        width,
        height,
        zoom,
        pan,
        Some(texture_cache),
        None,
    )
}

/// Renders the scene honouring an [`OverlaySettings`] set — Godot's View-menu
/// overlay toggles (pat-s3rrv.82).
///
/// ⚠️ Deliberately a SEPARATE entry point rather than a parameter on the
/// existing ones. `OverlaySettings::default()` enables only the origin cross and
/// the lock/group icons, so routing the current callers through it would turn
/// collision shapes and navmesh regions OFF for every existing caller and test —
/// a silent change to what the editor draws, made while adding a toggle nobody
/// had asked to flip yet. `render_scene`/`render_scene_with_zoom_pan`/
/// `render_scene_with_textures` therefore keep drawing everything, and only a
/// caller that actually has an overlay set gets the gating.
pub fn render_scene_with_overlays(
    tree: &SceneTree,
    selected: Option<NodeId>,
    width: u32,
    height: u32,
    zoom: f64,
    pan: (f64, f64),
    overlays: &crate::viewport_2d::OverlaySettings,
) -> FrameBuffer {
    render_scene_inner(
        tree,
        selected,
        width,
        height,
        zoom,
        pan,
        None,
        Some(overlays),
    )
}

fn render_scene_inner(
    tree: &SceneTree,
    selected: Option<NodeId>,
    width: u32,
    height: u32,
    zoom: f64,
    pan: (f64, f64),
    mut texture_cache: Option<&mut TextureCache>,
    overlays: Option<&crate::viewport_2d::OverlaySettings>,
) -> FrameBuffer {
    // `None` means "no overlay set supplied" — draw every gizmo, which is what
    // every pre-existing caller expects. It is NOT the same as an empty set.
    let shows = |o: crate::viewport_2d::CanvasOverlay| -> bool {
        overlays.map_or(true, |set| set.is_enabled(o))
    };
    let mut fb = FrameBuffer::new(width, height, BG_COLOR);
    let z = zoom as f32;

    // Compute camera offset to center the scene, then apply zoom/pan.
    let bounds = compute_scene_bounds(tree);
    let center_x = bounds.position.x + bounds.size.x / 2.0;
    let center_y = bounds.position.y + bounds.size.y / 2.0;
    let offset_x = width as f32 / 2.0 - center_x * z + pan.0 as f32;
    let offset_y = height as f32 / 2.0 - center_y * z + pan.1 as f32;

    // Draw grid (zoom-aware).
    draw_grid_zoomed(&mut fb, offset_x, offset_y, z);

    // Draw origin crosshair (X=red horizontal, Y=green vertical).
    draw_origin_crosshair(&mut fb, offset_x, offset_y);

    // Walk all nodes in draw order and draw them (pat-0q2kx.102).
    //
    // Godot's 2D editor draws each CanvasItem at its composed GLOBAL transform,
    // skips hidden subtrees, and honours draw order
    // (editor/plugins/canvas_item_editor_plugin.cpp). This walked raw tree order
    // using each node's *local* `position`, so any scene with parent transforms
    // looked wrong in edit mode while Play rendered it correctly.
    //
    // ⚠️ The sort must be STABLE: within one z_index, Godot keeps tree order,
    // and `sort_by_key` preserves it. An unstable sort would reshuffle same-z
    // siblings and make the edit view flicker between renders.
    let mut node_ids = tree.all_nodes_in_tree_order();
    node_ids.sort_by_key(|&id| canvas_order::effective_z_index(tree, id));
    for &node_id in &node_ids {
        let node = match tree.get_node(node_id) {
            Some(n) => n,
            None => continue,
        };

        // A hidden node hides its whole subtree, so this tests visibility up the
        // parent chain rather than the node's own `visible` flag.
        if !node2d::is_visible_in_tree(tree, node_id) {
            continue;
        }

        let world_pos = node2d::get_global_position(tree, node_id);
        let pos = Vector2::new(world_pos.x * z + offset_x, world_pos.y * z + offset_y);
        let class = node.class_name();
        let is_selected = selected == Some(node_id);

        // Draw node representation based on class.
        match class {
            "Node2D" => draw_node2d_diamond(&mut fb, pos, COLOR_NODE2D),
            "Sprite2D" => {
                draw_sprite2d_with_texture(&mut fb, node, pos, z, &mut texture_cache);
            }
            "Camera2D" => draw_camera2d_icon(&mut fb, pos, COLOR_CAMERA2D, z),
            "GPUParticles2D" | "CPUParticles2D" => {
                // A stepped emitter shows its live cloud; an idle one (no
                // published particles) keeps the emitter glyph so the node is
                // still visible and pickable (pat-0q2kx.51).
                if !draw_live_particles(&mut fb, node, pos, z) {
                    draw_node2d_diamond(&mut fb, pos, COLOR_PARTICLES);
                }
            }
            "CollisionShape2D" => {
                // ⚠️ Gated INSIDE the arm, not with a match guard: a failing
                // guard falls through to the `_` catch-all below, which draws a
                // default dot — so an overlay switched off would swap the gizmo
                // for a different mark rather than hiding it.
                if shows(crate::viewport_2d::CanvasOverlay::CollisionShapes) {
                    draw_collision_shape_from_node(&mut fb, node, pos, z, is_selected);
                }
            }
            // A baked NavigationRegion2D shows its navmesh, the same way a
            // CollisionShape2D shows its shape (pat-s3rrv.82). Unbaked regions
            // fall through to nothing drawn, which is what "not baked yet"
            // should look like.
            "NavigationRegion2D" => {
                if shows(crate::viewport_2d::CanvasOverlay::NavigationRegions) {
                    draw_navigation_region(&mut fb, node, pos, z);
                }
            }
            "CharacterBody2D" => draw_physics_body(&mut fb, pos, z, COLOR_CHARBODY, node),
            "RigidBody2D" => draw_physics_body(&mut fb, pos, z, COLOR_RIGIDBODY, node),
            "StaticBody2D" => draw_static_body(&mut fb, pos, z),
            "Area2D" => draw_area2d(&mut fb, pos, z),
            "Label" => {
                let size = extract_size(node);
                let scaled_size = Vector2::new(size.x * z, size.y * z);
                draw_control_rect(&mut fb, pos, scaled_size, COLOR_CONTROL);
                draw_label_icon(&mut fb, pos);
            }
            "Button" => {
                let size = extract_size(node);
                let scaled_size = Vector2::new(size.x * z, size.y * z);
                draw_button_rect(&mut fb, pos, scaled_size, COLOR_CONTROL);
            }
            "Control" => {
                let size = extract_size(node);
                let scaled_size = Vector2::new(size.x * z, size.y * z);
                draw_control_rect(&mut fb, pos, scaled_size, COLOR_CONTROL);
            }
            // A ColorRect is a solid block of its own `color`, not a marker
            // (pat-0q2kx.102). Without this arm it fell to the `_` case below
            // and drew a 2px COLOR_DEFAULT dot: `size` and `color` were both
            // ignored, so two stacked rects were byte-identical however they
            // were ordered — which made the z-order half of this bead
            // unobservable in edit mode even though the z sort above is
            // correct.
            //
            // The Play view already draws it properly
            // (`viewport_2d_render.rs` -> `Render2DKind::ColorRect`), so this
            // was the two-views-disagree bug this bead exists to close. Same
            // defaults as that path: `color` absent -> white, matching Godot's
            // `ColorRect.color` default.
            "ColorRect" => {
                // Read `size` directly rather than through `extract_size`: that
                // helper substitutes a 40x20 MARKER size when the property is
                // absent, which is right for the Control/Label/Button glyphs
                // but wrong here — an unsized ColorRect draws nothing in Godot,
                // and inventing a block would put pixels on screen that Play
                // does not draw.
                let size = match node.get_property("size") {
                    Variant::Vector2(v) => v,
                    _ => Vector2::ZERO,
                };
                let color = match node.get_property("color") {
                    Variant::Color(c) => c,
                    _ => Color::WHITE,
                };
                draw::fill_rect(
                    &mut fb,
                    Rect2::new(pos, Vector2::new(size.x * z, size.y * z)),
                    color,
                );
            }
            "Node" => {
                // Skip root node, draw small circle for others.
                if node.parent().is_some() {
                    draw::fill_circle(&mut fb, pos, 3.0, COLOR_NODE_DOT);
                }
            }
            _ => {
                draw::fill_circle(&mut fb, pos, 2.0, COLOR_DEFAULT);
            }
        }

        // Draw selection highlight.
        if is_selected {
            draw_selection_highlight(&mut fb, pos, class, z);
            // Draw transform gizmo at node position.
            draw_move_gizmo(&mut fb, pos, z);
            // Draw node label for selected node.
            draw_node_label(&mut fb, pos, node.name(), COLOR_SELECTED);
        } else if class != "Node" || node.parent().is_some() {
            // Draw node label for non-root nodes.
            draw_node_label(&mut fb, pos, node.name(), COLOR_LABEL_TEXT);
        }
    }

    // Draw rulers on top of everything (they are UI chrome).
    draw_rulers(&mut fb, offset_x, offset_y, z);

    fb
}

/// Computes the axis-aligned bounding box of all nodes with a position property.
pub fn compute_scene_bounds(tree: &SceneTree) -> Rect2 {
    let node_ids = tree.all_nodes_in_tree_order();
    let mut min_x = f32::MAX;
    let mut min_y = f32::MAX;
    let mut max_x = f32::MIN;
    let mut max_y = f32::MIN;
    let mut found = false;

    for &node_id in &node_ids {
        let node = match tree.get_node(node_id) {
            Some(n) => n,
            None => continue,
        };

        // Global, to match what the render pass actually draws (pat-0q2kx.102).
        // Bounding local positions while drawing global ones would centre the
        // editor camera on a point no pixel occupies, so a scene built out of
        // parent transforms would open off-screen.
        //
        // The "has no position property" skip still consults the node's OWN
        // property: a node that never declares one is a non-visual node that
        // should not stretch the bounds, even though its parent chain would
        // give it a non-zero global position.
        if !node.has_property("position") && extract_position(node) == Vector2::ZERO {
            continue;
        }
        let pos = node2d::get_global_position(tree, node_id);

        found = true;
        min_x = min_x.min(pos.x);
        min_y = min_y.min(pos.y);
        max_x = max_x.max(pos.x);
        max_y = max_y.max(pos.y);
    }

    if !found {
        return Rect2::new(Vector2::ZERO, Vector2::ZERO);
    }

    Rect2::new(
        Vector2::new(min_x, min_y),
        Vector2::new(max_x - min_x, max_y - min_y),
    )
}

/// Extracts the position from a node's properties, defaulting to (0, 0).
pub fn extract_position(node: &gdscene::node::Node) -> Vector2 {
    match node.get_property("position") {
        Variant::Vector2(v) => v,
        _ => Vector2::ZERO,
    }
}

/// Extracts the size from a node's properties, defaulting to (40, 20).
pub fn extract_size(node: &gdscene::node::Node) -> Vector2 {
    match node.get_property("size") {
        Variant::Vector2(v) => v,
        _ => Vector2::new(40.0, 20.0),
    }
}

/// Draws the background grid with zoom support.
fn draw_grid_zoomed(fb: &mut FrameBuffer, offset_x: f32, offset_y: f32, zoom: f32) {
    let w = fb.width;
    let h = fb.height;

    let minor = (GRID_MINOR as f32 * zoom).max(4.0);
    let _major = (GRID_MAJOR as f32 * zoom).max(16.0);

    // Vertical lines.
    let start_world_x = -offset_x / zoom;
    let start_x = (start_world_x / (GRID_MINOR as f32)).floor() as i32;
    let end_x = ((w as f32 - offset_x) / zoom / (GRID_MINOR as f32)).ceil() as i32;
    for ix in start_x..=end_x {
        let wx = ix as f32 * GRID_MINOR as f32;
        let sx = wx * zoom + offset_x;
        if sx < 0.0 || sx >= w as f32 {
            continue;
        }
        let is_major = ix as i64 % (GRID_MAJOR as i64 / GRID_MINOR as i64) == 0;
        let color = if is_major {
            GRID_COLOR_MAJOR
        } else {
            // Skip minor lines when too zoomed out.
            if minor < 8.0 {
                continue;
            }
            GRID_COLOR_MINOR
        };
        draw::draw_line(
            fb,
            Vector2::new(sx, 0.0),
            Vector2::new(sx, h as f32 - 1.0),
            color,
            1.0,
        );
    }

    // Horizontal lines.
    let start_world_y = -offset_y / zoom;
    let start_y = (start_world_y / (GRID_MINOR as f32)).floor() as i32;
    let end_y = ((h as f32 - offset_y) / zoom / (GRID_MINOR as f32)).ceil() as i32;
    for iy in start_y..=end_y {
        let wy = iy as f32 * GRID_MINOR as f32;
        let sy = wy * zoom + offset_y;
        if sy < 0.0 || sy >= h as f32 {
            continue;
        }
        let is_major = iy as i64 % (GRID_MAJOR as i64 / GRID_MINOR as i64) == 0;
        let color = if is_major {
            GRID_COLOR_MAJOR
        } else {
            if minor < 8.0 {
                continue;
            }
            GRID_COLOR_MINOR
        };
        draw::draw_line(
            fb,
            Vector2::new(0.0, sy),
            Vector2::new(w as f32 - 1.0, sy),
            color,
            1.0,
        );
    }
}

/// Draws the background grid (no zoom, kept for backwards compatibility).
#[allow(dead_code)]
fn draw_grid(fb: &mut FrameBuffer, offset_x: f32, offset_y: f32) {
    let w = fb.width;
    let h = fb.height;

    // Minor grid lines (every 50px).
    let start_x = ((-offset_x / GRID_MINOR as f32).floor() as i32) * GRID_MINOR as i32;
    let start_y = ((-offset_y / GRID_MINOR as f32).floor() as i32) * GRID_MINOR as i32;

    let mut x = start_x;
    while (x as f32 + offset_x) < w as f32 {
        let sx = x as f32 + offset_x;
        if sx >= 0.0 {
            let color = if (x as u32).is_multiple_of(GRID_MAJOR) {
                GRID_COLOR_MAJOR
            } else {
                GRID_COLOR_MINOR
            };
            draw::draw_line(
                fb,
                Vector2::new(sx, 0.0),
                Vector2::new(sx, h as f32 - 1.0),
                color,
                1.0,
            );
        }
        x += GRID_MINOR as i32;
    }

    let mut y = start_y;
    while (y as f32 + offset_y) < h as f32 {
        let sy = y as f32 + offset_y;
        if sy >= 0.0 {
            let color = if (y as u32).is_multiple_of(GRID_MAJOR) {
                GRID_COLOR_MAJOR
            } else {
                GRID_COLOR_MINOR
            };
            draw::draw_line(
                fb,
                Vector2::new(0.0, sy),
                Vector2::new(w as f32 - 1.0, sy),
                color,
                1.0,
            );
        }
        y += GRID_MINOR as i32;
    }
}

/// Draws a filled diamond shape for Node2D nodes (8px size).
fn draw_node2d_diamond(fb: &mut FrameBuffer, pos: Vector2, color: Color) {
    let s = 8.0;
    // Fill the diamond by drawing horizontal lines for each row.
    let y_min = (pos.y - s) as i32;
    let y_max = (pos.y + s) as i32;
    for y in y_min..=y_max {
        if y < 0 || y >= fb.height as i32 {
            continue;
        }
        let dy = (y as f32 + 0.5 - pos.y).abs();
        let half_w = s - dy;
        if half_w <= 0.0 {
            continue;
        }
        let x_start = ((pos.x - half_w) as i32).max(0) as u32;
        let x_end = ((pos.x + half_w) as i32).max(0).min(fb.width as i32) as u32;
        for x in x_start..x_end {
            fb.set_pixel(x, y as u32, color);
        }
    }
    // Draw outline on top for crispness.
    let top = Vector2::new(pos.x, pos.y - s);
    let right = Vector2::new(pos.x + s, pos.y);
    let bottom = Vector2::new(pos.x, pos.y + s);
    let left = Vector2::new(pos.x - s, pos.y);
    let outline = Color::new(color.r * 0.7, color.g * 0.7, color.b * 0.7, 1.0);
    draw::draw_line(fb, top, right, outline, 1.0);
    draw::draw_line(fb, right, bottom, outline, 1.0);
    draw::draw_line(fb, bottom, left, outline, 1.0);
    draw::draw_line(fb, left, top, outline, 1.0);
}

/// Draws a Sprite2D icon: blue rectangle with diagonal cross (image placeholder)
/// and thin white border. Size ~32x32 scaled by zoom.
fn draw_sprite2d_icon(fb: &mut FrameBuffer, pos: Vector2, color: Color, zoom: f32) {
    let half = (16.0 * zoom).max(8.0);
    let rect = Rect2::new(
        Vector2::new(pos.x - half, pos.y - half),
        Vector2::new(half * 2.0, half * 2.0),
    );
    // Fill with the sprite color.
    draw::fill_rect(fb, rect, color);
    // White border.
    let border = Color::new(1.0, 1.0, 1.0, 0.6);
    let tl = Vector2::new(pos.x - half, pos.y - half);
    let tr = Vector2::new(pos.x + half, pos.y - half);
    let br = Vector2::new(pos.x + half, pos.y + half);
    let bl = Vector2::new(pos.x - half, pos.y + half);
    draw::draw_line(fb, tl, tr, border, 1.0);
    draw::draw_line(fb, tr, br, border, 1.0);
    draw::draw_line(fb, br, bl, border, 1.0);
    draw::draw_line(fb, bl, tl, border, 1.0);
    // Diagonal cross inside to indicate image placeholder.
    let inset = half * 0.3;
    let icon_color = Color::new(1.0, 1.0, 1.0, 0.5);
    draw::draw_line(
        fb,
        Vector2::new(pos.x - half + inset, pos.y - half + inset),
        Vector2::new(pos.x + half - inset, pos.y + half - inset),
        icon_color,
        1.0,
    );
    draw::draw_line(
        fb,
        Vector2::new(pos.x + half - inset, pos.y - half + inset),
        Vector2::new(pos.x - half + inset, pos.y + half - inset),
        icon_color,
        1.0,
    );
}

/// Draws Sprite2D with texture from cache or blue placeholder.
/// Edge length (px, at zoom 1) of the quad drawn for one live particle in the
/// edit viewport. Matches the runtime quad so the preview and the Play frame
/// agree on particle size.
const PARTICLE_QUAD_SIZE: f32 = 4.0;

/// Draws a particle emitter's live particles into the edit viewport, returning
/// whether any were drawn (pat-0q2kx.51).
///
/// `MainLoop::process_particles` publishes the live set onto the node every frame
/// as `_particle_positions` (emitter-local), `_particle_colors`, and
/// `_particle_scales`; the edit viewport reads the same arrays the Play canvas
/// does, so a scene being stepped shows moving particles in both. Positions are
/// emitter-local, so each quad lands at the emitter's screen position plus its
/// zoom-scaled local offset. Returns `false` when the node has no published
/// particles (the scene is not running), leaving the caller to draw the emitter
/// glyph instead.
fn draw_live_particles(
    fb: &mut FrameBuffer,
    node: &gdscene::node::Node,
    pos: Vector2,
    zoom: f32,
) -> bool {
    let positions = match node.get_property("_particle_positions") {
        Variant::PackedVector2Array(v) => v,
        _ => return false,
    };
    if positions.is_empty() {
        return false;
    }
    let colors = match node.get_property("_particle_colors") {
        Variant::PackedColorArray(v) => v,
        _ => Vec::new(),
    };
    let scales = match node.get_property("_particle_scales") {
        Variant::PackedFloat32Array(v) => v,
        _ => Vec::new(),
    };
    for (i, local) in positions.iter().enumerate() {
        let scale = match scales.get(i).copied() {
            Some(s) if s > 0.0 => s,
            _ => 1.0,
        };
        let color = colors.get(i).copied().unwrap_or(Color::WHITE);
        let side = (PARTICLE_QUAD_SIZE * scale * zoom).max(1.0);
        let center = Vector2::new(pos.x + local.x * zoom, pos.y + local.y * zoom);
        draw::fill_rect_blended(
            fb,
            Rect2::new(
                Vector2::new(center.x - side / 2.0, center.y - side / 2.0),
                Vector2::new(side, side),
            ),
            color,
        );
    }
    true
}

fn draw_sprite2d_with_texture(
    fb: &mut FrameBuffer,
    node: &gdscene::node::Node,
    pos: Vector2,
    zoom: f32,
    texture_cache: &mut Option<&mut TextureCache>,
) {
    let texture_path = match node.get_property("texture") {
        Variant::String(s) => Some(s.clone()),
        _ => None,
    };
    let loaded = if let (Some(ref path), Some(ref mut cache)) = (&texture_path, texture_cache) {
        if !path.is_empty() {
            cache.get(path).cloned()
        } else {
            None
        }
    } else {
        None
    };
    if let Some(tex) = loaded {
        let offset = match node.get_property("offset") {
            Variant::Vector2(v) => v,
            _ => Vector2::ZERO,
        };
        let flip_h = matches!(node.get_property("flip_h"), Variant::Bool(true));
        let flip_v = matches!(node.get_property("flip_v"), Variant::Bool(true));
        let modulate = match node.get_property("modulate") {
            Variant::Color(c) => c,
            _ => Color::WHITE,
        };
        // Which pixels of the texture this sprite samples — an explicit region
        // or cell `frame` of the hframes × vframes sheet (pat-0q2kx.107). The
        // edit view drew the whole texture regardless, so a sheet-based sprite
        // looked nothing like it does at runtime.
        let sheet = crate::viewport_2d_render::sprite_sheet_for_node(node);
        let (tw, th) = (tex.width as f32, tex.height as f32);
        let mut src = gdscene::sprite_frames::sprite_source_rect(
            Vector2::new(tw, th),
            sheet.region_enabled,
            sheet.region_rect,
            sheet.hframes,
            sheet.vframes,
            sheet.frame,
        );

        let mut draw_tex = tex;
        if flip_h {
            draw_tex = draw_tex.flip_horizontal();
            // Flipping maps x → width - x, so the cell's span moves with it.
            // Mirroring the source rect keeps the same pixels selected instead
            // of sampling the mirrored position of a different cell.
            src.position.x = tw - (src.position.x + src.size.x);
        }
        if flip_v {
            draw_tex = draw_tex.flip_vertical();
            src.position.y = th - (src.position.y + src.size.y);
        }

        // The drawn size is the SOURCE size, not the whole texture.
        let sw = src.size.x * zoom;
        let sh = src.size.y * zoom;
        // `centered` (Godot's default) centres the sub-rect on the node origin.
        let (dx, dy) = if sheet.centered {
            (
                pos.x + offset.x * zoom - sw / 2.0,
                pos.y + offset.y * zoom - sh / 2.0,
            )
        } else {
            (pos.x + offset.x * zoom, pos.y + offset.y * zoom)
        };
        draw::draw_texture_region_blended(
            fb,
            &draw_tex,
            Rect2::new(Vector2::new(dx, dy), Vector2::new(sw, sh)),
            src,
            modulate,
        );
    } else {
        draw_sprite2d_icon(fb, pos, COLOR_SPRITE2D, zoom);
    }
}

/// Draws a Camera2D icon: viewport outline with a small camera icon.
fn draw_camera2d_icon(fb: &mut FrameBuffer, pos: Vector2, color: Color, zoom: f32) {
    // Viewport bounds that the camera would see (project resolution / 2, scaled).
    let vp_hw = (160.0 * zoom).max(40.0);
    let vp_hh = (90.0 * zoom).max(24.0);

    // Draw viewport outline with dashed effect (alternating segments).
    let dash_len = 6.0;
    let outline_color = Color::new(color.r, color.g, color.b, 0.4);
    draw_dashed_rect(fb, pos, vp_hw, vp_hh, outline_color, dash_len);

    // Draw small camera icon at center (a small filled rect + triangle "lens").
    let cam_w = 8.0;
    let cam_h = 6.0;
    let cam_rect = Rect2::new(
        Vector2::new(pos.x - cam_w / 2.0, pos.y - cam_h / 2.0),
        Vector2::new(cam_w, cam_h),
    );
    draw::fill_rect(fb, cam_rect, color);
    // Small triangle lens to the right.
    draw::draw_line(
        fb,
        Vector2::new(pos.x + cam_w / 2.0, pos.y - 3.0),
        Vector2::new(pos.x + cam_w / 2.0 + 5.0, pos.y),
        color,
        1.0,
    );
    draw::draw_line(
        fb,
        Vector2::new(pos.x + cam_w / 2.0 + 5.0, pos.y),
        Vector2::new(pos.x + cam_w / 2.0, pos.y + 3.0),
        color,
        1.0,
    );
}

/// Draws a dashed rectangle outline.
fn draw_dashed_rect(
    fb: &mut FrameBuffer,
    center: Vector2,
    hw: f32,
    hh: f32,
    color: Color,
    dash: f32,
) {
    let tl = Vector2::new(center.x - hw, center.y - hh);
    let tr = Vector2::new(center.x + hw, center.y - hh);
    let br = Vector2::new(center.x + hw, center.y + hh);
    let bl = Vector2::new(center.x - hw, center.y + hh);
    draw_dashed_line(fb, tl, tr, color, dash);
    draw_dashed_line(fb, tr, br, color, dash);
    draw_dashed_line(fb, br, bl, color, dash);
    draw_dashed_line(fb, bl, tl, color, dash);
}

/// Draws a dashed line by splitting it into segments.
fn draw_dashed_line(fb: &mut FrameBuffer, from: Vector2, to: Vector2, color: Color, dash: f32) {
    let dx = to.x - from.x;
    let dy = to.y - from.y;
    let len = (dx * dx + dy * dy).sqrt();
    if len < 1.0 {
        return;
    }
    let nx = dx / len;
    let ny = dy / len;
    let mut t = 0.0;
    let mut draw_on = true;
    while t < len {
        let seg_end = (t + dash).min(len);
        if draw_on {
            let a = Vector2::new(from.x + nx * t, from.y + ny * t);
            let b = Vector2::new(from.x + nx * seg_end, from.y + ny * seg_end);
            draw::draw_line(fb, a, b, color, 1.0);
        }
        t = seg_end;
        draw_on = !draw_on;
    }
}

/// Draws a CollisionShape2D: green outline circle (default shape).
/// Kept for backwards compatibility.
#[allow(dead_code)]
fn draw_collision_shape(fb: &mut FrameBuffer, pos: Vector2, zoom: f32) {
    draw_circle_outline(fb, pos, (16.0 * zoom).max(6.0), COLOR_COLLISION);
    draw_center_diamond(fb, pos, 3.0, COLOR_COLLISION);
}

/// Draws a CollisionShape2D reading shape properties from the node.
///
/// Supports `shape_type` property values: "circle", "rectangle", "capsule".
/// Falls back to a default circle when no shape_type is set.
/// When `selected`, also draws resize handles.
/// Draws a baked `NavigationRegion2D`'s navmesh as closed polygon outlines.
///
/// ⚠️ `pos` is the region's GLOBAL position already mapped into screen space by
/// the caller (`world_pos * zoom + offset`), and the polygons come back in
/// node-LOCAL space, so each point is `pos + v * zoom`. Using the pre-offset
/// points from `navigation_overlay_2d` here would apply the position twice.
///
/// Faces are filled and edges stroked, matching Godot, whose defaults are
/// `geometry_face_color = Color(0.5, 1, 1, 0.4)` and
/// `geometry_edge_color = Color(0.5, 1, 1, 1)`
/// (`servers/navigation_2d/navigation_server_2d.cpp:232`).
///
/// ⚠️ The fill must be BLENDED. `polygon_raster::fill_polygon` writes through
/// `set_pixel`, so a 0.4-alpha face there lands fully opaque and erases the
/// scene under the overlay instead of tinting it.
///
/// Edges close the loop — a polygon of N points draws N segments, not N - 1 —
/// so a region never renders as an open fan.
fn draw_navigation_region(
    fb: &mut FrameBuffer,
    node: &gdscene::node::Node,
    pos: Vector2,
    zoom: f32,
) {
    for points in crate::navigation_overlay::navigation_polygons_local(node) {
        let screen: Vec<Vector2> = points
            .iter()
            .map(|v| Vector2::new(pos.x + v.x * zoom, pos.y + v.y * zoom))
            .collect();
        gdrender2d::polygon_raster::fill_polygon_blended(fb, &screen, COLOR_NAVMESH_FACE);
        for i in 0..screen.len() {
            draw::draw_line_blended(
                fb,
                screen[i],
                screen[(i + 1) % screen.len()],
                COLOR_NAVMESH_EDGE,
                1.0,
            );
        }
    }
}

fn draw_collision_shape_from_node(
    fb: &mut FrameBuffer,
    node: &gdscene::node::Node,
    pos: Vector2,
    zoom: f32,
    selected: bool,
) {
    let color = COLOR_COLLISION;
    let shape_type = match node.get_property("shape_type") {
        Variant::String(s) => s,
        _ => String::new(),
    };

    match shape_type.as_str() {
        "rectangle" => {
            let extents = match node.get_property("shape_extents") {
                Variant::Vector2(v) => v,
                _ => Vector2::new(16.0, 16.0),
            };
            let hw = (extents.x * zoom).max(4.0);
            let hh = (extents.y * zoom).max(4.0);
            let rect = Rect2::new(
                Vector2::new(pos.x - hw, pos.y - hh),
                Vector2::new(hw * 2.0, hh * 2.0),
            );
            draw::draw_rect_outline_blended(fb, rect, color, 1.0);
            draw_center_diamond(fb, pos, 3.0, color);

            // Draw resize handles when selected.
            if selected {
                let handle_size = 3.0;
                let corners = [
                    Vector2::new(pos.x - hw, pos.y - hh),
                    Vector2::new(pos.x + hw, pos.y - hh),
                    Vector2::new(pos.x + hw, pos.y + hh),
                    Vector2::new(pos.x - hw, pos.y + hh),
                ];
                for c in &corners {
                    draw::fill_rect(
                        fb,
                        Rect2::new(
                            Vector2::new(c.x - handle_size, c.y - handle_size),
                            Vector2::new(handle_size * 2.0, handle_size * 2.0),
                        ),
                        color,
                    );
                }
                // Edge midpoint handles.
                let edges = [
                    Vector2::new(pos.x, pos.y - hh),
                    Vector2::new(pos.x + hw, pos.y),
                    Vector2::new(pos.x, pos.y + hh),
                    Vector2::new(pos.x - hw, pos.y),
                ];
                for e in &edges {
                    draw::fill_rect(
                        fb,
                        Rect2::new(Vector2::new(e.x - 2.0, e.y - 2.0), Vector2::new(4.0, 4.0)),
                        color,
                    );
                }
            }
        }
        "capsule" => {
            let radius = match node.get_property("shape_radius") {
                Variant::Float(r) => r as f32,
                _ => 10.0,
            };
            let height = match node.get_property("shape_height") {
                Variant::Float(h) => h as f32,
                _ => 40.0,
            };
            let r = (radius * zoom).max(4.0);
            let half_h = ((height / 2.0 - radius).max(0.0)) * zoom;

            // Top semicircle.
            let segments = 12;
            for i in 0..segments {
                let a0 = std::f32::consts::PI + (i as f32 / segments as f32) * std::f32::consts::PI;
                let a1 = std::f32::consts::PI
                    + ((i + 1) as f32 / segments as f32) * std::f32::consts::PI;
                let p0 = Vector2::new(pos.x + r * a0.cos(), pos.y - half_h + r * a0.sin());
                let p1 = Vector2::new(pos.x + r * a1.cos(), pos.y - half_h + r * a1.sin());
                draw::draw_line(fb, p0, p1, color, 1.0);
            }
            // Bottom semicircle.
            for i in 0..segments {
                let a0 = (i as f32 / segments as f32) * std::f32::consts::PI;
                let a1 = ((i + 1) as f32 / segments as f32) * std::f32::consts::PI;
                let p0 = Vector2::new(pos.x + r * a0.cos(), pos.y + half_h + r * a0.sin());
                let p1 = Vector2::new(pos.x + r * a1.cos(), pos.y + half_h + r * a1.sin());
                draw::draw_line(fb, p0, p1, color, 1.0);
            }
            // Connecting lines.
            draw::draw_line(
                fb,
                Vector2::new(pos.x - r, pos.y - half_h),
                Vector2::new(pos.x - r, pos.y + half_h),
                color,
                1.0,
            );
            draw::draw_line(
                fb,
                Vector2::new(pos.x + r, pos.y - half_h),
                Vector2::new(pos.x + r, pos.y + half_h),
                color,
                1.0,
            );
            draw_center_diamond(fb, pos, 3.0, color);
        }
        "segment" => {
            let a = match node.get_property("shape_point_a") {
                Variant::Vector2(v) => v,
                _ => Vector2::new(-20.0, 0.0),
            };
            let b = match node.get_property("shape_point_b") {
                Variant::Vector2(v) => v,
                _ => Vector2::new(20.0, 0.0),
            };
            let pa = Vector2::new(pos.x + a.x * zoom, pos.y + a.y * zoom);
            let pb = Vector2::new(pos.x + b.x * zoom, pos.y + b.y * zoom);
            draw::draw_line(fb, pa, pb, color, 1.0);
            // Endpoint dots.
            draw::fill_circle(fb, pa, 3.0, color);
            draw::fill_circle(fb, pb, 3.0, color);
        }
        _ => {
            // Default: circle shape (also handles "circle" explicitly).
            let radius = match node.get_property("shape_radius") {
                Variant::Float(r) => r as f32,
                _ => 16.0,
            };
            let r = (radius * zoom).max(6.0);
            draw_circle_outline(fb, pos, r, color);
            draw_center_diamond(fb, pos, 3.0, color);

            // Draw radius handle when selected.
            if selected {
                let handle_pos = Vector2::new(pos.x + r, pos.y);
                draw::fill_rect(
                    fb,
                    Rect2::new(
                        Vector2::new(handle_pos.x - 3.0, handle_pos.y - 3.0),
                        Vector2::new(6.0, 6.0),
                    ),
                    color,
                );
            }
        }
    }
}

/// Draws a circle outline using line segments.
fn draw_circle_outline(fb: &mut FrameBuffer, center: Vector2, radius: f32, color: Color) {
    let segments = 24;
    for i in 0..segments {
        let a0 = (i as f32 / segments as f32) * std::f32::consts::TAU;
        let a1 = ((i + 1) as f32 / segments as f32) * std::f32::consts::TAU;
        let p0 = Vector2::new(center.x + radius * a0.cos(), center.y + radius * a0.sin());
        let p1 = Vector2::new(center.x + radius * a1.cos(), center.y + radius * a1.sin());
        draw::draw_line(fb, p0, p1, color, 1.0);
    }
}

/// Draws a small diamond at center.
fn draw_center_diamond(fb: &mut FrameBuffer, pos: Vector2, s: f32, color: Color) {
    draw::draw_line(
        fb,
        Vector2::new(pos.x, pos.y - s),
        Vector2::new(pos.x + s, pos.y),
        color,
        1.0,
    );
    draw::draw_line(
        fb,
        Vector2::new(pos.x + s, pos.y),
        Vector2::new(pos.x, pos.y + s),
        color,
        1.0,
    );
    draw::draw_line(
        fb,
        Vector2::new(pos.x, pos.y + s),
        Vector2::new(pos.x - s, pos.y),
        color,
        1.0,
    );
    draw::draw_line(
        fb,
        Vector2::new(pos.x - s, pos.y),
        Vector2::new(pos.x, pos.y - s),
        color,
        1.0,
    );
}

// ---------------------------------------------------------------------------
// Physics body rendering
// ---------------------------------------------------------------------------

/// Draws a CharacterBody2D or RigidBody2D: colored outline + optional arrow.
fn draw_physics_body(
    fb: &mut FrameBuffer,
    pos: Vector2,
    zoom: f32,
    color: Color,
    node: &gdscene::node::Node,
) {
    let half = (14.0 * zoom).max(8.0);
    let rect = Rect2::new(
        Vector2::new(pos.x - half, pos.y - half),
        Vector2::new(half * 2.0, half * 2.0),
    );
    // Outline.
    draw::draw_rect_outline_blended(fb, rect, color, 1.0);
    // Diamond center.
    draw_node2d_diamond(fb, pos, color);

    // Draw velocity arrow for CharacterBody2D or gravity arrow for RigidBody2D.
    let arrow_vec = match node.get_property("velocity") {
        Variant::Vector2(v) if v.length_squared() > 0.01 => Some(v),
        _ => None,
    };
    if let Some(vel) = arrow_vec {
        let len = vel.length().min(60.0) * zoom;
        let dir = Vector2::new(vel.x / vel.length(), vel.y / vel.length());
        let end = Vector2::new(pos.x + dir.x * len, pos.y + dir.y * len);
        let arrow_color = Color::new(color.r, color.g, color.b, 0.7);
        draw::draw_line(fb, pos, end, arrow_color, 1.0);
        // Arrowhead.
        let head = 6.0;
        let perp = Vector2::new(-dir.y, dir.x);
        draw::draw_line(
            fb,
            end,
            Vector2::new(
                end.x - dir.x * head + perp.x * head * 0.4,
                end.y - dir.y * head + perp.y * head * 0.4,
            ),
            arrow_color,
            1.0,
        );
        draw::draw_line(
            fb,
            end,
            Vector2::new(
                end.x - dir.x * head - perp.x * head * 0.4,
                end.y - dir.y * head - perp.y * head * 0.4,
            ),
            arrow_color,
            1.0,
        );
    }

    // Gravity indicator for RigidBody2D (always draw a small down arrow).
    if node.class_name() == "RigidBody2D" && arrow_vec.is_none() {
        let grav_len = 12.0 * zoom;
        let grav_end = Vector2::new(pos.x, pos.y + half + grav_len);
        let grav_color = Color::new(color.r, color.g, color.b, 0.5);
        draw::draw_line(
            fb,
            Vector2::new(pos.x, pos.y + half),
            grav_end,
            grav_color,
            1.0,
        );
        draw::draw_line(
            fb,
            grav_end,
            Vector2::new(grav_end.x - 4.0, grav_end.y - 5.0),
            grav_color,
            1.0,
        );
        draw::draw_line(
            fb,
            grav_end,
            Vector2::new(grav_end.x + 4.0, grav_end.y - 5.0),
            grav_color,
            1.0,
        );
    }
}

/// Draws a StaticBody2D: solid gray outline indicating immovable.
fn draw_static_body(fb: &mut FrameBuffer, pos: Vector2, zoom: f32) {
    let half = (14.0 * zoom).max(8.0);
    let rect = Rect2::new(
        Vector2::new(pos.x - half, pos.y - half),
        Vector2::new(half * 2.0, half * 2.0),
    );
    // Solid gray outline (2 lines thick for emphasis).
    draw::draw_rect_outline_blended(fb, rect, COLOR_STATICBODY, 1.0);
    let inner = Rect2::new(
        Vector2::new(pos.x - half + 1.0, pos.y - half + 1.0),
        Vector2::new(half * 2.0 - 2.0, half * 2.0 - 2.0),
    );
    draw::draw_rect_outline_blended(fb, inner, COLOR_STATICBODY, 1.0);
    // Small anchor symbol at center (cross).
    let s = 4.0;
    draw::draw_line(
        fb,
        Vector2::new(pos.x - s, pos.y),
        Vector2::new(pos.x + s, pos.y),
        COLOR_STATICBODY,
        1.0,
    );
    draw::draw_line(
        fb,
        Vector2::new(pos.x, pos.y - s),
        Vector2::new(pos.x, pos.y + s),
        COLOR_STATICBODY,
        1.0,
    );
}

/// Draws an Area2D: blue tinted region.
fn draw_area2d(fb: &mut FrameBuffer, pos: Vector2, zoom: f32) {
    let half = (20.0 * zoom).max(8.0);
    let rect = Rect2::new(
        Vector2::new(pos.x - half, pos.y - half),
        Vector2::new(half * 2.0, half * 2.0),
    );
    draw::fill_rect_blended(fb, rect, COLOR_AREA2D);
    // Blue outline.
    let outline = Color::new(0.3, 0.5, 1.0, 0.5);
    draw::draw_rect_outline_blended(fb, rect, outline, 1.0);
    // Small diamond center.
    draw_node2d_diamond(fb, pos, Color::new(0.3, 0.5, 1.0, 1.0));
}

/// Draws a Label icon: "Aa" text indicator.
fn draw_label_icon(fb: &mut FrameBuffer, pos: Vector2) {
    let color = Color::new(1.0, 1.0, 1.0, 0.8);
    // Draw "Aa" using bitmap_char at the node position.
    draw_bitmap_char(fb, pos.x - 6.0, pos.y - 4.0, 'A', color);
    draw_bitmap_char(fb, pos.x + 1.0, pos.y - 2.0, 'a', color);
}

/// Draws a Button-like rectangle with a slight rounded indication.
fn draw_button_rect(fb: &mut FrameBuffer, pos: Vector2, size: Vector2, color: Color) {
    let rect = Rect2::new(pos, size);
    // Slightly lighter fill for button appearance.
    let fill = Color::new(color.r * 0.5, color.g * 0.5, color.b * 0.5, 0.6);
    draw::fill_rect_blended(fb, rect, fill);
    // Border.
    draw::draw_rect_outline_blended(fb, rect, color, 1.0);
    // Small "Btn" hint.
    let cx = pos.x + size.x / 2.0;
    let cy = pos.y + size.y / 2.0;
    let hint_color = Color::new(1.0, 1.0, 1.0, 0.6);
    draw_bitmap_char(fb, cx - 4.0, cy - 3.0, 'B', hint_color);
}

/// Draws a filled rectangle for Control-derived nodes.
fn draw_control_rect(fb: &mut FrameBuffer, pos: Vector2, size: Vector2, color: Color) {
    let rect = Rect2::new(pos, size);
    draw::fill_rect(fb, rect, color);
}

/// Draws a selection highlight around a node (zoom-aware).
fn draw_selection_highlight(fb: &mut FrameBuffer, pos: Vector2, class: &str, zoom: f32) {
    let (hw, hh) = match class {
        "Sprite2D" => ((18.0 * zoom).max(10.0), (18.0 * zoom).max(10.0)),
        "Node2D" => (11.0, 11.0),
        "Camera2D" => (12.0, 12.0),
        "CollisionShape2D" => ((18.0 * zoom).max(8.0), (18.0 * zoom).max(8.0)),
        "CharacterBody2D" | "RigidBody2D" | "StaticBody2D" => {
            ((18.0 * zoom).max(10.0), (18.0 * zoom).max(10.0))
        }
        "Area2D" => ((22.0 * zoom).max(10.0), (22.0 * zoom).max(10.0)),
        _ => (8.0, 8.0),
    };
    let tl = Vector2::new(pos.x - hw, pos.y - hh);
    let tr = Vector2::new(pos.x + hw, pos.y - hh);
    let br = Vector2::new(pos.x + hw, pos.y + hh);
    let bl = Vector2::new(pos.x - hw, pos.y + hh);
    draw::draw_line(fb, tl, tr, COLOR_SELECTED, 1.0);
    draw::draw_line(fb, tr, br, COLOR_SELECTED, 1.0);
    draw::draw_line(fb, br, bl, COLOR_SELECTED, 1.0);
    draw::draw_line(fb, bl, tl, COLOR_SELECTED, 1.0);
    // Draw small corner squares for resize handles.
    let cs = 2.0;
    for corner in &[tl, tr, br, bl] {
        draw::fill_rect(
            fb,
            Rect2::new(
                Vector2::new(corner.x - cs, corner.y - cs),
                Vector2::new(cs * 2.0, cs * 2.0),
            ),
            COLOR_SELECTED,
        );
    }
}

/// Draws a move gizmo (X/Y arrows + center square) at the given screen position.
///
/// Arrow length is ~40px in screen space, independent of zoom.
fn draw_move_gizmo(fb: &mut FrameBuffer, pos: Vector2, _zoom: f32) {
    let arrow_len = 40.0;
    let head_len = 8.0;
    let head_half = 4.0;
    let center_half = 3.0;

    // X arrow (red) — rightward.
    let x_end = Vector2::new(pos.x + arrow_len, pos.y);
    draw::draw_line(fb, pos, x_end, GIZMO_X_COLOR, 1.5);
    // Arrowhead.
    draw::draw_line(
        fb,
        x_end,
        Vector2::new(x_end.x - head_len, x_end.y - head_half),
        GIZMO_X_COLOR,
        1.5,
    );
    draw::draw_line(
        fb,
        x_end,
        Vector2::new(x_end.x - head_len, x_end.y + head_half),
        GIZMO_X_COLOR,
        1.5,
    );

    // Y arrow (green) — downward.
    let y_end = Vector2::new(pos.x, pos.y + arrow_len);
    draw::draw_line(fb, pos, y_end, GIZMO_Y_COLOR, 1.5);
    // Arrowhead.
    draw::draw_line(
        fb,
        y_end,
        Vector2::new(y_end.x - head_half, y_end.y - head_len),
        GIZMO_Y_COLOR,
        1.5,
    );
    draw::draw_line(
        fb,
        y_end,
        Vector2::new(y_end.x + head_half, y_end.y - head_len),
        GIZMO_Y_COLOR,
        1.5,
    );

    // Center square.
    let sq = Rect2::new(
        Vector2::new(pos.x - center_half, pos.y - center_half),
        Vector2::new(center_half * 2.0, center_half * 2.0),
    );
    draw::fill_rect(fb, sq, GIZMO_CENTER_COLOR);
}

// ---------------------------------------------------------------------------
// Origin crosshair
// ---------------------------------------------------------------------------

/// Draws the origin crosshair at (0,0) in world space.
///
/// - Horizontal red line at Y=0 spanning the full viewport width (X axis).
/// - Vertical green line at X=0 spanning the full viewport height (Y axis).
///
/// Both are semi-transparent so they don't overwhelm the scene.
fn draw_origin_crosshair(fb: &mut FrameBuffer, offset_x: f32, offset_y: f32) {
    let w = fb.width as f32;
    let h = fb.height as f32;

    // Y=0 in screen space is at offset_y.
    let sy = offset_y;
    if sy >= 0.0 && sy < h {
        // Horizontal red line (X axis).
        for px in 0..fb.width {
            fb.blend_pixel(px, sy as u32, ORIGIN_X_COLOR);
        }
    }

    // X=0 in screen space is at offset_x.
    let sx = offset_x;
    if sx >= 0.0 && sx < w {
        // Vertical green line (Y axis).
        for py in 0..fb.height {
            fb.blend_pixel(sx as u32, py, ORIGIN_Y_COLOR);
        }
    }
}

// ---------------------------------------------------------------------------
// Bitmap font (tiny 5x7 pixel characters)
// ---------------------------------------------------------------------------

/// Draws a single character from a built-in 5x7 bitmap font.
///
/// Each character is encoded as a `[u8; 7]` where each byte represents one row
/// and the low 5 bits are the pixel columns (MSB=left).
fn draw_bitmap_char(fb: &mut FrameBuffer, x: f32, y: f32, ch: char, color: Color) {
    let bitmap = char_bitmap(ch);
    let ix = x as i32;
    let iy = y as i32;
    for (row, &bits) in bitmap.iter().enumerate() {
        for col in 0..5u32 {
            if bits & (1 << (4 - col)) != 0 {
                let px = ix + col as i32;
                let py = iy + row as i32;
                if px >= 0 && py >= 0 && (px as u32) < fb.width && (py as u32) < fb.height {
                    fb.blend_pixel(px as u32, py as u32, color);
                }
            }
        }
    }
}

/// Returns the width of the given string in bitmap font pixels.
fn bitmap_string_width(s: &str) -> f32 {
    // Each char is 5px wide + 1px gap, minus trailing gap.
    let n = s.len();
    if n == 0 {
        0.0
    } else {
        (n * 6 - 1) as f32
    }
}

/// Draws a string using the bitmap font.
fn draw_bitmap_string(fb: &mut FrameBuffer, x: f32, y: f32, s: &str, color: Color) {
    let mut cx = x;
    for ch in s.chars() {
        draw_bitmap_char(fb, cx, y, ch, color);
        cx += 6.0; // 5px char + 1px gap
    }
}

/// Returns a 5x7 bitmap for a character, with full printable-ASCII coverage.
///
/// Delegates to the shared [`gdrender2d::font`] bitmap font so viewport labels
/// and ruler ticks use the same complete glyph set as the rest of the engine.
/// This module previously kept its own partial table (only `0-9`, `A-F`, `a`,
/// `-`, space), so every other letter fell back to a filled box — node names
/// like "Player" rendered as tofu (pat-rvebj). The encoding is identical: each
/// `u8` holds one row, low 5 bits, MSB = leftmost column.
fn char_bitmap(ch: char) -> [u8; 7] {
    gdrender2d::font::char_bitmap(ch)
}

// ---------------------------------------------------------------------------
// Node labels
// ---------------------------------------------------------------------------

/// Draws a node name label below the node position.
fn draw_node_label(fb: &mut FrameBuffer, pos: Vector2, name: &str, color: Color) {
    if name.is_empty() {
        return;
    }
    // Truncate to 12 chars to avoid very long labels.
    let display: &str = if name.len() > 12 { &name[..12] } else { name };
    let text_w = bitmap_string_width(display);
    let lx = pos.x - text_w / 2.0;
    let ly = pos.y + 14.0; // Below the node icon.

    // Draw a small dark background behind the text for readability.
    let bg = Color::new(0.0, 0.0, 0.0, 0.5);
    let pad = 2.0;
    draw::fill_rect_blended(
        fb,
        Rect2::new(
            Vector2::new(lx - pad, ly - pad),
            Vector2::new(text_w + pad * 2.0, 7.0 + pad * 2.0),
        ),
        bg,
    );

    draw_bitmap_string(fb, lx, ly, display, color);
}

// ---------------------------------------------------------------------------
// Rulers
// ---------------------------------------------------------------------------

/// Draws pixel rulers along the top and left edges of the viewport.
///
/// Tick marks appear every 100px in world space (zoom-aware). Numbers are
/// rendered using the bitmap font at major intervals.
fn draw_rulers(fb: &mut FrameBuffer, offset_x: f32, offset_y: f32, zoom: f32) {
    let w = fb.width;
    let h = fb.height;

    // Draw ruler backgrounds.
    draw::fill_rect(
        fb,
        Rect2::new(
            Vector2::new(RULER_SIZE as f32, 0.0),
            Vector2::new(w as f32 - RULER_SIZE as f32, RULER_SIZE as f32),
        ),
        RULER_BG,
    );
    draw::fill_rect(
        fb,
        Rect2::new(
            Vector2::new(0.0, RULER_SIZE as f32),
            Vector2::new(RULER_SIZE as f32, h as f32 - RULER_SIZE as f32),
        ),
        RULER_BG,
    );
    // Corner square.
    draw::fill_rect(
        fb,
        Rect2::new(
            Vector2::ZERO,
            Vector2::new(RULER_SIZE as f32, RULER_SIZE as f32),
        ),
        RULER_BG,
    );

    // Compute tick spacing: aim for ~100px world-space intervals, snapped to
    // nice numbers.
    let base_interval = 100.0;
    let interval = snap_ruler_interval(base_interval, zoom);
    let screen_interval = interval * zoom;

    // --- Top ruler (horizontal, X coordinates) ---
    {
        let start_world_x = -(offset_x - RULER_SIZE as f32) / zoom;
        let start_tick = (start_world_x / interval).floor() as i64;
        let end_tick = ((w as f32 - offset_x) / zoom / interval).ceil() as i64;

        for i in start_tick..=end_tick {
            let world_x = i as f32 * interval;
            let sx = world_x * zoom + offset_x;
            if sx < RULER_SIZE as f32 || sx >= w as f32 {
                continue;
            }
            let is_major = screen_interval >= 40.0 || i % 2 == 0;
            let tick_h = if is_major {
                RULER_SIZE as f32 * 0.6
            } else {
                RULER_SIZE as f32 * 0.3
            };
            let ty = RULER_SIZE as f32 - tick_h;
            draw::draw_line(
                fb,
                Vector2::new(sx, ty),
                Vector2::new(sx, RULER_SIZE as f32 - 1.0),
                RULER_TICK,
                1.0,
            );
            // Draw number for major ticks.
            if is_major {
                let num = format_ruler_number(world_x);
                let tw = bitmap_string_width(&num);
                if sx + 2.0 + tw < w as f32 {
                    draw_bitmap_string(fb, sx + 2.0, 2.0, &num, RULER_TEXT);
                }
            }
        }
    }

    // --- Left ruler (vertical, Y coordinates) ---
    {
        let start_world_y = -(offset_y - RULER_SIZE as f32) / zoom;
        let start_tick = (start_world_y / interval).floor() as i64;
        let end_tick = ((h as f32 - offset_y) / zoom / interval).ceil() as i64;

        for i in start_tick..=end_tick {
            let world_y = i as f32 * interval;
            let sy = world_y * zoom + offset_y;
            if sy < RULER_SIZE as f32 || sy >= h as f32 {
                continue;
            }
            let is_major = screen_interval >= 40.0 || i % 2 == 0;
            let tick_w = if is_major {
                RULER_SIZE as f32 * 0.6
            } else {
                RULER_SIZE as f32 * 0.3
            };
            let tx = RULER_SIZE as f32 - tick_w;
            draw::draw_line(
                fb,
                Vector2::new(tx, sy),
                Vector2::new(RULER_SIZE as f32 - 1.0, sy),
                RULER_TICK,
                1.0,
            );
            // Draw number for major ticks (vertically placed).
            if is_major {
                let num = format_ruler_number(world_y);
                if sy + 2.0 + 7.0 < h as f32 {
                    // Draw each digit vertically stacked.
                    let mut cy = sy + 2.0;
                    for ch in num.chars() {
                        if cy + 7.0 >= h as f32 {
                            break;
                        }
                        draw_bitmap_char(fb, 2.0, cy, ch, RULER_TEXT);
                        cy += 8.0;
                    }
                }
            }
        }
    }

    // Border lines for rulers.
    draw::draw_line(
        fb,
        Vector2::new(RULER_SIZE as f32, 0.0),
        Vector2::new(RULER_SIZE as f32, h as f32 - 1.0),
        RULER_TICK,
        1.0,
    );
    draw::draw_line(
        fb,
        Vector2::new(0.0, RULER_SIZE as f32),
        Vector2::new(w as f32 - 1.0, RULER_SIZE as f32),
        RULER_TICK,
        1.0,
    );
}

/// Snaps the ruler interval to a nice round number based on zoom level.
fn snap_ruler_interval(base: f32, zoom: f32) -> f32 {
    let screen_size = base * zoom;
    if screen_size < 20.0 {
        base * 4.0
    } else if screen_size < 40.0 {
        base * 2.0
    } else if screen_size > 200.0 {
        base / 2.0
    } else {
        base
    }
}

/// Formats a world-coordinate number for ruler display.
fn format_ruler_number(val: f32) -> String {
    let v = val.round() as i64;
    if v == 0 {
        "0".to_string()
    } else {
        format!("{}", v)
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Computes the camera offset used by [`render_scene`] to center the scene.
pub fn camera_offset(tree: &SceneTree, viewport_width: u32, viewport_height: u32) -> Vector2 {
    let bounds = compute_scene_bounds(tree);
    let center_x = bounds.position.x + bounds.size.x / 2.0;
    let center_y = bounds.position.y + bounds.size.y / 2.0;
    Vector2::new(
        viewport_width as f32 / 2.0 - center_x,
        viewport_height as f32 / 2.0 - center_y,
    )
}

/// Converts viewport pixel coordinates to scene coordinates using the same
/// camera offset logic as [`render_scene`].
pub fn pixel_to_scene(
    tree: &SceneTree,
    viewport_width: u32,
    viewport_height: u32,
    pixel_x: f32,
    pixel_y: f32,
) -> Vector2 {
    let offset = camera_offset(tree, viewport_width, viewport_height);
    Vector2::new(pixel_x - offset.x, pixel_y - offset.y)
}

/// Converts pixel coordinates to scene coordinates using a pre-computed offset.
pub fn pixel_to_scene_with_offset(offset: Vector2, pixel_x: f32, pixel_y: f32) -> Vector2 {
    Vector2::new(pixel_x - offset.x, pixel_y - offset.y)
}

/// Computes the camera offset with zoom and pan.
pub fn camera_offset_with_zoom_pan(
    tree: &SceneTree,
    viewport_width: u32,
    viewport_height: u32,
    zoom: f64,
    pan: (f64, f64),
) -> Vector2 {
    let bounds = compute_scene_bounds(tree);
    let z = zoom as f32;
    let center_x = bounds.position.x + bounds.size.x / 2.0;
    let center_y = bounds.position.y + bounds.size.y / 2.0;
    Vector2::new(
        viewport_width as f32 / 2.0 - center_x * z + pan.0 as f32,
        viewport_height as f32 / 2.0 - center_y * z + pan.1 as f32,
    )
}

/// Converts viewport pixel coordinates to scene (world) coordinates
/// accounting for zoom and pan.
pub fn pixel_to_scene_with_zoom_pan(
    tree: &SceneTree,
    viewport_width: u32,
    viewport_height: u32,
    zoom: f64,
    pan: (f64, f64),
    pixel_x: f32,
    pixel_y: f32,
) -> Vector2 {
    let offset = camera_offset_with_zoom_pan(tree, viewport_width, viewport_height, zoom, pan);
    let z = zoom as f32;
    Vector2::new((pixel_x - offset.x) / z, (pixel_y - offset.y) / z)
}

/// Hit-tests with zoom and pan support.
pub fn hit_test_with_zoom_pan(
    tree: &SceneTree,
    viewport_width: u32,
    viewport_height: u32,
    zoom: f64,
    pan: (f64, f64),
    click_x: f32,
    click_y: f32,
) -> Option<NodeId> {
    let scene_pos = pixel_to_scene_with_zoom_pan(
        tree,
        viewport_width,
        viewport_height,
        zoom,
        pan,
        click_x,
        click_y,
    );
    let node_ids = tree.all_nodes_in_tree_order();

    // Scale hit radii inversely with zoom so they feel consistent.
    let z = zoom as f32;

    let mut best: Option<(NodeId, f32, i64)> = None;

    for &node_id in &node_ids {
        let node = match tree.get_node(node_id) {
            Some(n) => n,
            None => continue,
        };

        if node.parent().is_none() {
            continue;
        }

        // Hit-testing must use the SAME transform the render pass draws with,
        // or clicking a child of a moved/rotated parent would select nothing
        // where its pixels actually are (pat-0q2kx.102). A hidden node is not
        // pickable, matching Godot's editor.
        if !node2d::is_visible_in_tree(tree, node_id) {
            continue;
        }

        let class = node.class_name();
        let node_pos = node2d::get_global_position(tree, node_id);

        if matches!(class, "Control" | "Label" | "Button") {
            let size = extract_size(node);
            let rect = Rect2::new(node_pos, size);
            if rect.contains_point(scene_pos) {
                let center = Vector2::new(node_pos.x + size.x / 2.0, node_pos.y + size.y / 2.0);
                let dist = (scene_pos - center).length();
                let z_idx = extract_z_index(node);
                if let Some((_, best_dist, best_z)) = best {
                    if z_idx > best_z || (z_idx == best_z && dist < best_dist) {
                        best = Some((node_id, dist, z_idx));
                    }
                } else {
                    best = Some((node_id, dist, z_idx));
                }
            }
            continue;
        }

        let base_radius = match class {
            "Sprite2D" => 20.0,
            _ => 15.0,
        };
        // Scale hit radius — at higher zoom, world-space radius shrinks
        // so that the hit area stays ~constant in screen pixels.
        let hit_radius = base_radius / z;

        let dist = (scene_pos - node_pos).length();
        if dist <= hit_radius {
            let z_idx = extract_z_index(node);
            if let Some((_, best_dist, best_z)) = best {
                if z_idx > best_z || (z_idx == best_z && dist < best_dist) {
                    best = Some((node_id, dist, z_idx));
                }
            } else {
                best = Some((node_id, dist, z_idx));
            }
        }
    }

    best.map(|(id, _, _)| id)
}

/// Hit-tests the scene tree at the given viewport pixel coordinates.
///
/// Returns the [`NodeId`] of the closest node under the click point,
/// or `None` if no node is within hit radius. Skips the root node.
/// Prefers nodes with higher `z_index` when overlapping.
pub fn hit_test(
    tree: &SceneTree,
    viewport_width: u32,
    viewport_height: u32,
    click_x: f32,
    click_y: f32,
) -> Option<NodeId> {
    let scene_pos = pixel_to_scene(tree, viewport_width, viewport_height, click_x, click_y);
    let node_ids = tree.all_nodes_in_tree_order();

    let mut best: Option<(NodeId, f32, i64)> = None; // (id, distance, z_index)

    for &node_id in &node_ids {
        let node = match tree.get_node(node_id) {
            Some(n) => n,
            None => continue,
        };

        // Skip root node (no parent).
        if node.parent().is_none() {
            continue;
        }

        // Hit-testing must use the SAME transform the render pass draws with,
        // or clicking a child of a moved/rotated parent would select nothing
        // where its pixels actually are (pat-0q2kx.102). A hidden node is not
        // pickable, matching Godot's editor.
        if !node2d::is_visible_in_tree(tree, node_id) {
            continue;
        }

        let class = node.class_name();
        let node_pos = node2d::get_global_position(tree, node_id);

        // For Control nodes, use bounding rect hit test.
        if matches!(class, "Control" | "Label" | "Button") {
            let size = extract_size(node);
            let rect = Rect2::new(node_pos, size);
            if rect.contains_point(scene_pos) {
                let center = Vector2::new(node_pos.x + size.x / 2.0, node_pos.y + size.y / 2.0);
                let dist = (scene_pos - center).length();
                let z = extract_z_index(node);
                if let Some((_, best_dist, best_z)) = best {
                    if z > best_z || (z == best_z && dist < best_dist) {
                        best = Some((node_id, dist, z));
                    }
                } else {
                    best = Some((node_id, dist, z));
                }
            }
            continue;
        }

        // For other nodes, use radius-based hit test.
        let hit_radius = match class {
            "Sprite2D" => 20.0,
            _ => 15.0,
        };

        let dist = (scene_pos - node_pos).length();
        if dist <= hit_radius {
            let z = extract_z_index(node);
            if let Some((_, best_dist, best_z)) = best {
                if z > best_z || (z == best_z && dist < best_dist) {
                    best = Some((node_id, dist, z));
                }
            } else {
                best = Some((node_id, dist, z));
            }
        }
    }

    best.map(|(id, _, _)| id)
}

/// Extracts z_index from a node's properties, defaulting to 0.
fn extract_z_index(node: &gdscene::node::Node) -> i64 {
    match node.get_property("z_index") {
        Variant::Int(z) => z,
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gdscene::node::Node;

    fn make_tree_with_node2d() -> (SceneTree, NodeId) {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let mut node = Node::new("Player", "Node2D");
        node.set_property("position", Variant::Vector2(Vector2::new(100.0, 100.0)));
        let id = tree.add_child(root, node).unwrap();
        (tree, id)
    }

    #[test]
    fn render_empty_scene() {
        let tree = SceneTree::new();
        let fb = render_scene(&tree, None, 100, 100);
        assert_eq!(fb.width, 100);
        assert_eq!(fb.height, 100);
        // Most pixels should be background (grid draws on top of some).
        let bg_count = fb.pixels.iter().filter(|&&p| p == BG_COLOR).count();
        assert!(bg_count > 5000, "most pixels should be background");
    }

    #[test]
    fn render_scene_dimensions() {
        let tree = SceneTree::new();
        let fb = render_scene(&tree, None, 200, 150);
        assert_eq!(fb.width, 200);
        assert_eq!(fb.height, 150);
    }

    #[test]
    fn render_scene_with_node2d() {
        let (tree, _) = make_tree_with_node2d();
        let fb = render_scene(&tree, None, 200, 200);
        // The node should have drawn something — not all BG color.
        let has_non_bg = fb.pixels.iter().any(|&p| p != BG_COLOR);
        assert!(has_non_bg, "scene with Node2D should render something");
    }

    #[test]
    fn render_scene_with_sprite2d() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let mut node = Node::new("Sprite", "Sprite2D");
        node.set_property("position", Variant::Vector2(Vector2::new(50.0, 50.0)));
        tree.add_child(root, node).unwrap();

        let fb = render_scene(&tree, None, 200, 200);
        // Should have blue-ish pixels from the Sprite2D rect.
        let has_blue = fb.pixels.iter().any(|&p| p.b > 0.5 && p.r < 0.5);
        assert!(has_blue, "Sprite2D should render blue rect");
    }

    #[test]
    fn render_scene_with_camera2d() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let mut node = Node::new("Cam", "Camera2D");
        node.set_property("position", Variant::Vector2(Vector2::new(50.0, 50.0)));
        tree.add_child(root, node).unwrap();

        let fb = render_scene(&tree, None, 200, 200);
        // Should have green pixels from the Camera2D outline.
        let has_green = fb.pixels.iter().any(|&p| p.g > 0.5 && p.r < 0.5);
        assert!(has_green, "Camera2D should render green outline");
    }

    #[test]
    fn selected_node_highlighting() {
        let (tree, node_id) = make_tree_with_node2d();
        let fb_no_sel = render_scene(&tree, None, 200, 200);
        let fb_sel = render_scene(&tree, Some(node_id), 200, 200);
        // Selected should have more amber/yellow pixels.
        let count_amber = |fb: &FrameBuffer| {
            fb.pixels
                .iter()
                .filter(|p| p.r > 0.9 && p.g > 0.7 && p.b < 0.2)
                .count()
        };
        assert!(
            count_amber(&fb_sel) > count_amber(&fb_no_sel),
            "selected node should have more amber highlight pixels"
        );
    }

    #[test]
    fn grid_draws_lines() {
        let tree = SceneTree::new();
        let fb = render_scene(&tree, None, 200, 200);
        // Grid lines should produce pixels that differ from BG.
        let non_bg_count = fb.pixels.iter().filter(|&&p| p != BG_COLOR).count();
        assert!(non_bg_count > 0, "grid should draw some lines");
    }

    #[test]
    fn compute_bounds_empty_scene() {
        let tree = SceneTree::new();
        let bounds = compute_scene_bounds(&tree);
        assert_eq!(bounds.size, Vector2::ZERO);
    }

    #[test]
    fn compute_bounds_single_node() {
        let (tree, _) = make_tree_with_node2d();
        let bounds = compute_scene_bounds(&tree);
        // Single node at (100, 100) → bounds is a zero-size rect at (100, 100).
        assert_eq!(bounds.position, Vector2::new(100.0, 100.0));
    }

    #[test]
    fn compute_bounds_multiple_nodes() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let mut n1 = Node::new("A", "Node2D");
        n1.set_property("position", Variant::Vector2(Vector2::new(10.0, 20.0)));
        tree.add_child(root, n1).unwrap();

        let mut n2 = Node::new("B", "Node2D");
        n2.set_property("position", Variant::Vector2(Vector2::new(100.0, 200.0)));
        tree.add_child(root, n2).unwrap();

        let bounds = compute_scene_bounds(&tree);
        assert_eq!(bounds.position, Vector2::new(10.0, 20.0));
        assert_eq!(bounds.size, Vector2::new(90.0, 180.0));
    }

    #[test]
    fn render_control_node() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let mut node = Node::new("Btn", "Button");
        node.set_property("position", Variant::Vector2(Vector2::new(50.0, 50.0)));
        node.set_property("size", Variant::Vector2(Vector2::new(80.0, 30.0)));
        tree.add_child(root, node).unwrap();

        let fb = render_scene(&tree, None, 200, 200);
        // Should have purple pixels from the control rect.
        let has_purple = fb.pixels.iter().any(|&p| p.r > 0.5 && p.b > 0.5);
        assert!(has_purple, "Button should render purple rect");
    }

    #[test]
    fn render_default_class_node() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let mut node = Node::new("Custom", "CustomClass");
        node.set_property("position", Variant::Vector2(Vector2::new(50.0, 50.0)));
        tree.add_child(root, node).unwrap();

        let fb = render_scene(&tree, None, 200, 200);
        let has_non_bg = fb.pixels.iter().any(|&p| p != BG_COLOR);
        assert!(has_non_bg, "custom class node should render a dot");
    }

    #[test]
    fn render_multiple_nodes() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let mut n1 = Node::new("A", "Node2D");
        n1.set_property("position", Variant::Vector2(Vector2::new(30.0, 30.0)));
        tree.add_child(root, n1).unwrap();

        let mut n2 = Node::new("B", "Sprite2D");
        n2.set_property("position", Variant::Vector2(Vector2::new(80.0, 80.0)));
        tree.add_child(root, n2).unwrap();

        let fb = render_scene(&tree, None, 200, 200);
        // Should have both amber (Node2D diamond) and blue (Sprite2D rect).
        let has_amber = fb
            .pixels
            .iter()
            .any(|&p| p.r > 0.9 && p.g > 0.6 && p.b < 0.2);
        let has_blue = fb.pixels.iter().any(|&p| p.b > 0.5 && p.r < 0.5);
        assert!(has_amber, "should render Node2D diamond");
        assert!(has_blue, "should render Sprite2D rect");
    }

    #[test]
    fn selected_dot_above_node() {
        let (tree, node_id) = make_tree_with_node2d();
        let fb = render_scene(&tree, Some(node_id), 200, 200);
        // The selected dot should be bright amber.
        let amber_count = fb
            .pixels
            .iter()
            .filter(|p| p.r > 0.9 && p.g > 0.7 && p.b < 0.2)
            .count();
        assert!(amber_count > 0, "selected node should have amber dot");
    }

    #[test]
    fn node_without_position_at_origin() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let node = Node::new("Plain", "Node");
        tree.add_child(root, node).unwrap();

        // Should not panic.
        let fb = render_scene(&tree, None, 100, 100);
        assert_eq!(fb.width, 100);
    }

    #[test]
    fn render_is_deterministic() {
        let (tree, node_id) = make_tree_with_node2d();
        let fb1 = render_scene(&tree, Some(node_id), 200, 200);
        let fb2 = render_scene(&tree, Some(node_id), 200, 200);
        assert_eq!(fb1.pixels, fb2.pixels);
    }

    // -- hit_test tests ---------------------------------------------------

    #[test]
    fn hit_test_finds_node() {
        let (tree, node_id) = make_tree_with_node2d();
        // Node is at (100, 100). Viewport is 200x200.
        // Scene bounds center = (100, 100), offset = (0, 0).
        // So pixel (100, 100) maps to scene (100, 100).
        let result = hit_test(&tree, 200, 200, 100.0, 100.0);
        assert_eq!(result, Some(node_id));
    }

    #[test]
    fn hit_test_misses_empty_area() {
        let (tree, _) = make_tree_with_node2d();
        // Click far from the node at (100, 100).
        let result = hit_test(&tree, 200, 200, 0.0, 0.0);
        assert_eq!(result, None);
    }

    #[test]
    fn hit_test_returns_closest() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let mut n1 = Node::new("A", "Node2D");
        n1.set_property("position", Variant::Vector2(Vector2::new(50.0, 50.0)));
        let id_a = tree.add_child(root, n1).unwrap();

        let mut n2 = Node::new("B", "Node2D");
        n2.set_property("position", Variant::Vector2(Vector2::new(60.0, 50.0)));
        let id_b = tree.add_child(root, n2).unwrap();

        // Bounds center = (55, 50), viewport 200x200, offset = (45, 50).
        // Click at pixel (105, 100) = scene (60, 50) = exactly on B.
        let result = hit_test(&tree, 200, 200, 105.0, 100.0);
        assert_eq!(result, Some(id_b));

        // Click at pixel (95, 100) = scene (50, 50) = exactly on A.
        let result = hit_test(&tree, 200, 200, 95.0, 100.0);
        assert_eq!(result, Some(id_a));
    }

    #[test]
    fn hit_test_skips_root() {
        let tree = SceneTree::new();
        // Click at center — root should not be returned.
        let result = hit_test(&tree, 200, 200, 100.0, 100.0);
        assert_eq!(result, None);
    }

    #[test]
    fn hit_test_sprite2d_larger_radius() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let mut sprite = Node::new("Sprite", "Sprite2D");
        sprite.set_property("position", Variant::Vector2(Vector2::new(100.0, 100.0)));
        let sprite_id = tree.add_child(root, sprite).unwrap();

        // Click 18px away — outside Node2D radius (15) but inside Sprite2D radius (20).
        // Scene pos = (100, 100), offset = (0, 0) for single node at center.
        let result = hit_test(&tree, 200, 200, 118.0, 100.0);
        assert_eq!(result, Some(sprite_id));
    }

    #[test]
    fn hit_test_control_bounding_rect() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let mut ctrl = Node::new("Btn", "Button");
        ctrl.set_property("position", Variant::Vector2(Vector2::new(80.0, 90.0)));
        ctrl.set_property("size", Variant::Vector2(Vector2::new(40.0, 20.0)));
        let ctrl_id = tree.add_child(root, ctrl).unwrap();

        // Single node at (80, 90). Bounds center = (80, 90).
        // Offset = (100 - 80, 100 - 90) = (20, 10).
        // Click at pixel (110, 105) = scene (90, 95) — inside rect (80,90)→(120,110).
        let result = hit_test(&tree, 200, 200, 110.0, 105.0);
        assert_eq!(result, Some(ctrl_id));

        // Click at pixel (50, 50) = scene (30, 40) — outside rect.
        let result = hit_test(&tree, 200, 200, 50.0, 50.0);
        assert_eq!(result, None);
    }

    #[test]
    fn hit_test_prefers_higher_z_index() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let mut n1 = Node::new("Back", "Node2D");
        n1.set_property("position", Variant::Vector2(Vector2::new(100.0, 100.0)));
        n1.set_property("z_index", Variant::Int(0));
        let _id_back = tree.add_child(root, n1).unwrap();

        let mut n2 = Node::new("Front", "Node2D");
        n2.set_property("position", Variant::Vector2(Vector2::new(100.0, 100.0)));
        n2.set_property("z_index", Variant::Int(1));
        let id_front = tree.add_child(root, n2).unwrap();

        // Both at same position — should pick the one with higher z_index.
        // Bounds center = (100, 100), offset = (0, 0).
        let result = hit_test(&tree, 200, 200, 100.0, 100.0);
        assert_eq!(result, Some(id_front));
    }

    #[test]
    fn hit_test_within_radius_boundary() {
        let (tree, node_id) = make_tree_with_node2d();
        // Node at (100, 100), Node2D hit radius = 15.
        // Click exactly at radius boundary (115, 100).
        let result = hit_test(&tree, 200, 200, 115.0, 100.0);
        assert_eq!(result, Some(node_id));

        // Click just outside radius (116, 100) — distance = 16 > 15.
        let result = hit_test(&tree, 200, 200, 116.0, 100.0);
        assert_eq!(result, None);
    }

    #[test]
    fn pixel_to_scene_round_trip() {
        let (tree, _) = make_tree_with_node2d();
        // Node at (100, 100), viewport 200x200.
        // Offset should be (0, 0), so pixel (100, 100) = scene (100, 100).
        let scene = pixel_to_scene(&tree, 200, 200, 100.0, 100.0);
        assert!((scene.x - 100.0).abs() < 0.01);
        assert!((scene.y - 100.0).abs() < 0.01);
    }

    // -- zoom/pan tests ---------------------------------------------------

    #[test]
    fn render_with_zoom_does_not_panic() {
        let (tree, node_id) = make_tree_with_node2d();
        let fb = render_scene_with_zoom_pan(&tree, Some(node_id), 200, 200, 2.0, (0.0, 0.0));
        assert_eq!(fb.width, 200);
        assert_eq!(fb.height, 200);
    }

    #[test]
    fn render_with_zoom_and_pan() {
        let (tree, _) = make_tree_with_node2d();
        let fb = render_scene_with_zoom_pan(&tree, None, 200, 200, 0.5, (50.0, -30.0));
        assert_eq!(fb.width, 200);
    }

    #[test]
    fn pixel_to_scene_with_zoom_pan_identity() {
        let (tree, _) = make_tree_with_node2d();
        // zoom=1, pan=(0,0) should match regular pixel_to_scene.
        let a = pixel_to_scene(&tree, 200, 200, 100.0, 100.0);
        let b = pixel_to_scene_with_zoom_pan(&tree, 200, 200, 1.0, (0.0, 0.0), 100.0, 100.0);
        assert!((a.x - b.x).abs() < 0.01);
        assert!((a.y - b.y).abs() < 0.01);
    }

    #[test]
    fn pixel_to_scene_zoom_scales_correctly() {
        let (tree, _) = make_tree_with_node2d();
        // At zoom=2, moving 10 pixels in screen space = 5 pixels in world space.
        let a = pixel_to_scene_with_zoom_pan(&tree, 200, 200, 2.0, (0.0, 0.0), 100.0, 100.0);
        let b = pixel_to_scene_with_zoom_pan(&tree, 200, 200, 2.0, (0.0, 0.0), 110.0, 100.0);
        assert!(
            (b.x - a.x - 5.0).abs() < 0.01,
            "10px at zoom 2x = 5 world units"
        );
    }

    #[test]
    fn pixel_to_scene_pan_offsets() {
        let (tree, _) = make_tree_with_node2d();
        // Panning right by 20px should shift the scene left by 20 world units (at zoom=1).
        let a = pixel_to_scene_with_zoom_pan(&tree, 200, 200, 1.0, (0.0, 0.0), 100.0, 100.0);
        let b = pixel_to_scene_with_zoom_pan(&tree, 200, 200, 1.0, (20.0, 0.0), 100.0, 100.0);
        assert!(
            (a.x - b.x - 20.0).abs() < 0.01,
            "pan right = scene shifts left"
        );
    }

    #[test]
    fn hit_test_with_zoom_pan_finds_node() {
        let (tree, node_id) = make_tree_with_node2d();
        // At zoom=1, pan=(0,0), pixel (100,100) maps to scene (100,100) which is on the node.
        let result = hit_test_with_zoom_pan(&tree, 200, 200, 1.0, (0.0, 0.0), 100.0, 100.0);
        assert_eq!(result, Some(node_id));
    }

    #[test]
    fn hit_test_with_zoom_pan_zoom_in() {
        let (tree, node_id) = make_tree_with_node2d();
        // At zoom=2, the node at world (100,100) appears at pixel
        // (100*2 + offset_x, 100*2 + offset_y) where offset = (100 - 100*2, 100 - 100*2) = (-100, -100).
        // So screen pos = (200 - 100, 200 - 100) = (100, 100). Same pixel.
        let result = hit_test_with_zoom_pan(&tree, 200, 200, 2.0, (0.0, 0.0), 100.0, 100.0);
        assert_eq!(result, Some(node_id));
    }

    #[test]
    fn hit_test_with_zoom_pan_miss_after_pan() {
        let (tree, _) = make_tree_with_node2d();
        // Pan far away — node should no longer be at pixel (100,100).
        let result = hit_test_with_zoom_pan(&tree, 200, 200, 1.0, (500.0, 500.0), 100.0, 100.0);
        assert_eq!(result, None);
    }

    #[test]
    fn render_selected_has_gizmo() {
        let (tree, node_id) = make_tree_with_node2d();
        let fb_sel = render_scene_with_zoom_pan(&tree, Some(node_id), 200, 200, 1.0, (0.0, 0.0));
        let fb_no = render_scene_with_zoom_pan(&tree, None, 200, 200, 1.0, (0.0, 0.0));
        // Selected frame should have red gizmo pixels (X arrow).
        let count_red = |fb: &FrameBuffer| {
            fb.pixels
                .iter()
                .filter(|p| p.r > 0.9 && p.g < 0.3 && p.b < 0.3)
                .count()
        };
        assert!(
            count_red(&fb_sel) > count_red(&fb_no),
            "selected node should have red gizmo arrow pixels"
        );
    }

    #[test]
    fn render_selected_has_green_gizmo() {
        let (tree, node_id) = make_tree_with_node2d();
        let fb_sel = render_scene_with_zoom_pan(&tree, Some(node_id), 200, 200, 1.0, (0.0, 0.0));
        let fb_no = render_scene_with_zoom_pan(&tree, None, 200, 200, 1.0, (0.0, 0.0));
        let count_green = |fb: &FrameBuffer| {
            fb.pixels
                .iter()
                .filter(|p| p.g > 0.7 && p.r < 0.3 && p.b < 0.3)
                .count()
        };
        assert!(
            count_green(&fb_sel) > count_green(&fb_no),
            "selected node should have green gizmo arrow pixels"
        );
    }

    // -- origin crosshair tests ------------------------------------------

    #[test]
    fn origin_crosshair_draws_red_horizontal_line() {
        let tree = SceneTree::new();
        let fb = render_scene_with_zoom_pan(&tree, None, 200, 200, 1.0, (0.0, 0.0));
        // The origin crosshair at Y=0 should add some reddish blended pixels.
        // offset_y for an empty scene is 100 (center of 200px viewport).
        let row = 100;
        let red_count = (0..200)
            .filter(|&x| {
                let p = fb.get_pixel(x, row);
                p.r > 0.15 && p.g < p.r && p.b < p.r
            })
            .count();
        assert!(
            red_count > 50,
            "origin crosshair should draw red horizontal line at Y=0, got {red_count} red pixels"
        );
    }

    #[test]
    fn origin_crosshair_draws_green_vertical_line() {
        let tree = SceneTree::new();
        let fb = render_scene_with_zoom_pan(&tree, None, 200, 200, 1.0, (0.0, 0.0));
        // offset_x for an empty scene is 100.
        let col = 100;
        let green_count = (0..200)
            .filter(|&y| {
                let p = fb.get_pixel(col, y);
                p.g > 0.15 && p.r < p.g
            })
            .count();
        assert!(
            green_count > 50,
            "origin crosshair should draw green vertical line at X=0, got {green_count} green pixels"
        );
    }

    #[test]
    fn origin_crosshair_semi_transparent() {
        let tree = SceneTree::new();
        let fb = render_scene_with_zoom_pan(&tree, None, 200, 200, 1.0, (0.0, 0.0));
        // The crosshair should be blended, not full red/green.
        // Check that the red line pixel is not pure red (alpha blending with BG).
        let p = fb.get_pixel(50, 100); // On the red horizontal line
        assert!(
            p.r < 0.5,
            "origin crosshair should be semi-transparent (blended), r={:.2}",
            p.r
        );
        assert!(
            p.r > 0.05,
            "origin crosshair should be visible, r={:.2}",
            p.r
        );
    }

    // -- ruler tests ------------------------------------------------------

    #[test]
    fn rulers_draw_dark_background() {
        let tree = SceneTree::new();
        let fb = render_scene_with_zoom_pan(&tree, None, 200, 200, 1.0, (0.0, 0.0));
        // The top-left corner should be ruler background (darker than BG).
        let ruler_pixel = fb.get_pixel(5, 5);
        assert!(
            ruler_pixel.r <= RULER_BG.r + 0.01 && ruler_pixel.g <= RULER_BG.g + 0.01,
            "corner should have ruler background color"
        );
    }

    #[test]
    fn rulers_have_tick_marks() {
        let tree = SceneTree::new();
        let fb = render_scene_with_zoom_pan(&tree, None, 400, 400, 1.0, (0.0, 0.0));
        // The ruler region should contain tick marks that differ from the ruler background.
        let non_bg_in_ruler = (RULER_SIZE..400)
            .filter(|&x| {
                let p = fb.get_pixel(x, RULER_SIZE - 2);
                (p.r - RULER_BG.r).abs() > 0.05
                    || (p.g - RULER_BG.g).abs() > 0.05
                    || (p.b - RULER_BG.b).abs() > 0.05
            })
            .count();
        assert!(
            non_bg_in_ruler > 2,
            "rulers should have tick marks, found {non_bg_in_ruler} non-bg pixels"
        );
    }

    #[test]
    fn rulers_border_lines_present() {
        let tree = SceneTree::new();
        let fb = render_scene_with_zoom_pan(&tree, None, 200, 200, 1.0, (0.0, 0.0));
        // Vertical ruler border at x=RULER_SIZE.
        let p = fb.get_pixel(RULER_SIZE, 50);
        assert!(
            (p.r - RULER_TICK.r).abs() < 0.05,
            "ruler border line should be visible at x=RULER_SIZE"
        );
    }

    // -- node visual tests ------------------------------------------------

    #[test]
    fn node2d_diamond_is_filled() {
        let (tree, _) = make_tree_with_node2d();
        let fb = render_scene_with_zoom_pan(&tree, None, 200, 200, 1.0, (0.0, 0.0));
        // Count amber pixels -- a filled diamond should have significantly more
        // than the old outline-only diamond.
        let amber_count = fb
            .pixels
            .iter()
            .filter(|p| p.r > 0.8 && p.g > 0.5 && p.b < 0.2)
            .count();
        // A filled 8px diamond should produce at least ~50 pixels.
        assert!(
            amber_count > 40,
            "filled Node2D diamond should have many amber pixels, got {amber_count}"
        );
    }

    #[test]
    fn sprite2d_has_white_border_and_cross() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let mut node = Node::new("Sprite", "Sprite2D");
        node.set_property("position", Variant::Vector2(Vector2::new(100.0, 100.0)));
        tree.add_child(root, node).unwrap();

        let fb = render_scene_with_zoom_pan(&tree, None, 200, 200, 1.0, (0.0, 0.0));
        // Should have white-ish pixels from the border/cross.
        let has_white = fb
            .pixels
            .iter()
            .any(|&p| p.r > 0.5 && p.g > 0.5 && p.b > 0.5);
        assert!(has_white, "Sprite2D should have white border/cross pixels");
    }

    #[test]
    fn camera2d_has_viewport_outline() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let mut node = Node::new("Cam", "Camera2D");
        node.set_property("position", Variant::Vector2(Vector2::new(50.0, 50.0)));
        tree.add_child(root, node).unwrap();

        let fb = render_scene_with_zoom_pan(&tree, None, 400, 400, 1.0, (0.0, 0.0));
        // Should have green pixels from the camera viewport outline.
        let green_count = fb
            .pixels
            .iter()
            .filter(|p| p.g > 0.5 && p.r < 0.5 && p.b < 0.5)
            .count();
        assert!(
            green_count > 20,
            "Camera2D should render green viewport outline, got {green_count}"
        );
    }

    #[test]
    fn collision_shape2d_renders_green_circle() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let mut node = Node::new("Col", "CollisionShape2D");
        node.set_property("position", Variant::Vector2(Vector2::new(100.0, 100.0)));
        tree.add_child(root, node).unwrap();

        let fb = render_scene_with_zoom_pan(&tree, None, 200, 200, 1.0, (0.0, 0.0));
        let green_count = fb.pixels.iter().filter(|p| p.g > 0.6 && p.r < 0.3).count();
        assert!(
            green_count > 10,
            "CollisionShape2D should render green circle outline, got {green_count}"
        );
    }

    #[test]
    fn area2d_renders_blue_tint() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let mut node = Node::new("Area", "Area2D");
        node.set_property("position", Variant::Vector2(Vector2::new(100.0, 100.0)));
        tree.add_child(root, node).unwrap();

        let fb = render_scene_with_zoom_pan(&tree, None, 200, 200, 1.0, (0.0, 0.0));
        // Should have blue-ish blended pixels from the Area2D region.
        let blue_count = fb.pixels.iter().filter(|p| p.b > 0.12 && p.b > p.r).count();
        assert!(
            blue_count > 20,
            "Area2D should render blue tinted region, got {blue_count}"
        );
    }

    #[test]
    fn label_node_shows_aa_icon() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let mut node = Node::new("MyLabel", "Label");
        node.set_property("position", Variant::Vector2(Vector2::new(50.0, 50.0)));
        node.set_property("size", Variant::Vector2(Vector2::new(80.0, 30.0)));
        tree.add_child(root, node).unwrap();

        let fb = render_scene_with_zoom_pan(&tree, None, 200, 200, 1.0, (0.0, 0.0));
        // Should have white-ish pixels from the "Aa" icon drawn on the label.
        let white_ish_count = fb
            .pixels
            .iter()
            .filter(|p| p.r > 0.7 && p.g > 0.7 && p.b > 0.7)
            .count();
        assert!(
            white_ish_count > 5,
            "Label should have 'Aa' icon pixels, got {white_ish_count}"
        );
    }

    #[test]
    fn button_renders_with_border() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let mut node = Node::new("Btn", "Button");
        node.set_property("position", Variant::Vector2(Vector2::new(50.0, 50.0)));
        node.set_property("size", Variant::Vector2(Vector2::new(80.0, 30.0)));
        tree.add_child(root, node).unwrap();

        let fb = render_scene_with_zoom_pan(&tree, None, 200, 200, 1.0, (0.0, 0.0));
        // Button should have purple-ish border pixels.
        let purple_count = fb
            .pixels
            .iter()
            .filter(|p| p.r > 0.5 && p.b > 0.5 && p.g < 0.5)
            .count();
        assert!(
            purple_count > 10,
            "Button should render purple border, got {purple_count}"
        );
    }

    // -- node label tests -------------------------------------------------

    #[test]
    fn selected_node_shows_label() {
        let (tree, node_id) = make_tree_with_node2d();
        let fb_sel = render_scene_with_zoom_pan(&tree, Some(node_id), 200, 200, 1.0, (0.0, 0.0));
        let fb_no = render_scene_with_zoom_pan(&tree, None, 200, 200, 1.0, (0.0, 0.0));
        // The selected version should have label text pixels (amber colored).
        let count_label = |fb: &FrameBuffer| {
            // Labels are drawn below the node, look for colored pixels in that area.
            fb.pixels
                .iter()
                .filter(|p| p.r > 0.8 && p.g > 0.6 && p.b < 0.3)
                .count()
        };
        assert!(
            count_label(&fb_sel) > count_label(&fb_no),
            "selected node should show name label"
        );
    }

    #[test]
    fn non_selected_nodes_show_labels() {
        let (tree, _) = make_tree_with_node2d();
        let fb = render_scene_with_zoom_pan(&tree, None, 200, 200, 1.0, (0.0, 0.0));
        // Non-root nodes get labels too (in gray/white).
        // The label background is a dark blended rect; text is light colored.
        let has_label_text = fb
            .pixels
            .iter()
            .any(|p| p.r > 0.7 && p.g > 0.7 && p.b > 0.7 && p.a > 0.8);
        assert!(has_label_text, "non-selected nodes should show name labels");
    }

    // -- bitmap font tests ------------------------------------------------

    #[test]
    fn bitmap_string_width_correct() {
        assert_eq!(bitmap_string_width(""), 0.0);
        assert_eq!(bitmap_string_width("A"), 5.0);
        assert_eq!(bitmap_string_width("AB"), 11.0);
        assert_eq!(bitmap_string_width("ABC"), 17.0);
    }

    #[test]
    fn bitmap_char_draws_pixels() {
        let mut fb = FrameBuffer::new(20, 20, Color::BLACK);
        draw_bitmap_char(&mut fb, 5.0, 5.0, 'A', Color::WHITE);
        // The 'A' character should draw some white pixels.
        let white_count = fb.pixels.iter().filter(|&&p| p == Color::WHITE).count();
        assert!(
            white_count > 10,
            "bitmap 'A' should draw at least 10 white pixels, got {white_count}"
        );
    }

    #[test]
    fn bitmap_char_clips_to_bounds() {
        let mut fb = FrameBuffer::new(10, 10, Color::BLACK);
        // Draw character partially off-screen; should not panic.
        draw_bitmap_char(&mut fb, -3.0, -3.0, '0', Color::WHITE);
        draw_bitmap_char(&mut fb, 8.0, 8.0, '0', Color::WHITE);
        // Just verify no panic occurred.
    }

    #[test]
    fn bitmap_string_draws_multiple_chars() {
        let mut fb = FrameBuffer::new(50, 20, Color::BLACK);
        draw_bitmap_string(&mut fb, 2.0, 5.0, "123", Color::WHITE);
        let white_count = fb.pixels.iter().filter(|&&p| p == Color::WHITE).count();
        assert!(
            white_count > 20,
            "bitmap string '123' should draw many white pixels, got {white_count}"
        );
    }

    /// Regression for pat-rvebj: viewport node-name labels rendered as tofu
    /// boxes because this module kept its own partial glyph table (only `0-9`,
    /// `A-F`, `a`, `-`, space), so every other letter fell through to a filled
    /// placeholder box. Now that `char_bitmap` delegates to the shared
    /// `gdrender2d::font`, every printable character in real node names and
    /// ruler ticks must resolve to a distinct, non-placeholder glyph.
    #[test]
    fn editor_viewport_label_glyph_coverage() {
        // The old fallback drawn for unknown letters: a filled rectangle.
        const FALLBACK_BOX: [u8; 7] = [
            0b11111, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b11111,
        ];

        // Every char that appears in the demo scene's node labels and ruler
        // ticks must map to a real glyph — not the tofu box, not blank.
        for ch in "PlayerEnemyGround0123456789-".chars() {
            let bm = char_bitmap(ch);
            assert_ne!(
                bm, FALLBACK_BOX,
                "char '{ch}' renders as a placeholder box (tofu)"
            );
            assert_ne!(bm, [0u8; 7], "char '{ch}' renders as blank");
        }

        // "Player" must not be six identical cells: each letter is its own
        // glyph. Before the fix, P/l/y/e/r all collapsed to the box and only
        // 'a' differed (2 distinct), which is exactly the reported tofu.
        let player_cells: std::collections::HashSet<[u8; 7]> =
            "Player".chars().map(char_bitmap).collect();
        assert!(
            player_cells.len() >= 5,
            "'Player' label has only {} distinct glyph cells (tofu collapse)",
            player_cells.len()
        );
    }

    // -- blend_pixel tests ------------------------------------------------

    #[test]
    fn blend_pixel_alpha_compositing() {
        let mut fb = FrameBuffer::new(5, 5, Color::rgb(0.0, 0.0, 1.0));
        // Blend semi-transparent red on top of blue.
        fb.blend_pixel(2, 2, Color::new(1.0, 0.0, 0.0, 0.5));
        let p = fb.get_pixel(2, 2);
        // Expected: r = 1.0*0.5 + 0.0*0.5 = 0.5, g = 0, b = 0.0*0.5 + 1.0*0.5 = 0.5
        assert!((p.r - 0.5).abs() < 0.01, "r should be ~0.5, got {:.3}", p.r);
        assert!((p.b - 0.5).abs() < 0.01, "b should be ~0.5, got {:.3}", p.b);
    }

    #[test]
    fn blend_pixel_fully_opaque_replaces() {
        let mut fb = FrameBuffer::new(5, 5, Color::rgb(0.0, 0.0, 1.0));
        fb.blend_pixel(1, 1, Color::rgb(1.0, 0.0, 0.0));
        let p = fb.get_pixel(1, 1);
        assert!((p.r - 1.0).abs() < 0.01);
        assert!(p.b < 0.01);
    }

    #[test]
    fn blend_pixel_fully_transparent_no_change() {
        let mut fb = FrameBuffer::new(5, 5, Color::rgb(0.0, 0.0, 1.0));
        fb.blend_pixel(1, 1, Color::new(1.0, 0.0, 0.0, 0.0));
        let p = fb.get_pixel(1, 1);
        assert!((p.b - 1.0).abs() < 0.01);
        assert!(p.r < 0.01);
    }

    // -- dashed line tests ------------------------------------------------

    #[test]
    fn dashed_line_draws_segments() {
        let mut fb = FrameBuffer::new(100, 10, Color::BLACK);
        draw_dashed_line(
            &mut fb,
            Vector2::new(0.0, 5.0),
            Vector2::new(99.0, 5.0),
            Color::WHITE,
            10.0,
        );
        // Should have some white pixels (drawn segments) and some black (gaps).
        let white_count = fb.pixels.iter().filter(|&&p| p == Color::WHITE).count();
        assert!(
            white_count > 20 && white_count < 80,
            "dashed line should have gaps, white={white_count}"
        );
    }

    // -- format_ruler_number tests ----------------------------------------

    #[test]
    fn format_ruler_number_values() {
        assert_eq!(format_ruler_number(0.0), "0");
        assert_eq!(format_ruler_number(100.0), "100");
        assert_eq!(format_ruler_number(-200.0), "-200");
        assert_eq!(format_ruler_number(99.7), "100");
    }

    // -- snap_ruler_interval tests ----------------------------------------

    #[test]
    fn snap_ruler_interval_at_normal_zoom() {
        let interval = snap_ruler_interval(100.0, 1.0);
        assert!((interval - 100.0).abs() < 0.01);
    }

    #[test]
    fn snap_ruler_interval_zoomed_out() {
        // At zoom 0.1, screen_size = 10 < 20, so interval should be 4x.
        let interval = snap_ruler_interval(100.0, 0.1);
        assert!((interval - 400.0).abs() < 0.01);
    }

    #[test]
    fn snap_ruler_interval_zoomed_in() {
        // At zoom 3.0, screen_size = 300 > 200, so interval should be halved.
        let interval = snap_ruler_interval(100.0, 3.0);
        assert!((interval - 50.0).abs() < 0.01);
    }

    // -- selection highlight with new node types --------------------------

    #[test]
    fn selection_highlight_collision_shape() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let mut node = Node::new("Col", "CollisionShape2D");
        node.set_property("position", Variant::Vector2(Vector2::new(100.0, 100.0)));
        let col_id = tree.add_child(root, node).unwrap();

        let fb = render_scene_with_zoom_pan(&tree, Some(col_id), 200, 200, 1.0, (0.0, 0.0));
        // Should have amber selection highlight pixels.
        let amber_count = fb
            .pixels
            .iter()
            .filter(|p| p.r > 0.9 && p.g > 0.7 && p.b < 0.2)
            .count();
        assert!(
            amber_count > 5,
            "selected CollisionShape2D should have amber highlight"
        );
    }

    #[test]
    fn selection_highlight_area2d() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let mut node = Node::new("Area", "Area2D");
        node.set_property("position", Variant::Vector2(Vector2::new(100.0, 100.0)));
        let area_id = tree.add_child(root, node).unwrap();

        let fb = render_scene_with_zoom_pan(&tree, Some(area_id), 200, 200, 1.0, (0.0, 0.0));
        let amber_count = fb
            .pixels
            .iter()
            .filter(|p| p.r > 0.9 && p.g > 0.7 && p.b < 0.2)
            .count();
        assert!(
            amber_count > 5,
            "selected Area2D should have amber highlight"
        );
    }

    // -- render_scene backward compatibility test -------------------------

    #[test]
    fn render_scene_backward_compat() {
        // render_scene() should still work and produce output.
        let (tree, node_id) = make_tree_with_node2d();
        let fb = render_scene(&tree, Some(node_id), 200, 200);
        assert_eq!(fb.width, 200);
        assert_eq!(fb.height, 200);
        let non_bg = fb.pixels.iter().filter(|&&p| p != BG_COLOR).count();
        assert!(
            non_bg > 100,
            "render_scene should still produce visible output"
        );
    }

    // -----------------------------------------------------------------------
    // Collision shape rendering tests
    // -----------------------------------------------------------------------

    #[test]
    fn render_collision_shape_circle() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let mut node = Node::new("Shape", "CollisionShape2D");
        node.set_property("position", Variant::Vector2(Vector2::new(100.0, 100.0)));
        node.set_property("shape_type", Variant::String("circle".to_string()));
        node.set_property("shape_radius", Variant::Float(30.0));
        tree.add_child(root, node).unwrap();

        let fb = render_scene(&tree, None, 300, 300);
        // Should have green collision outline pixels.
        let has_green = fb
            .pixels
            .iter()
            .any(|&p| p.g > 0.5 && p.r < 0.2 && p.b < 0.5);
        assert!(
            has_green,
            "circle CollisionShape2D should render green outline"
        );
    }

    #[test]
    fn render_collision_shape_rectangle() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let mut node = Node::new("Shape", "CollisionShape2D");
        node.set_property("position", Variant::Vector2(Vector2::new(100.0, 100.0)));
        node.set_property("shape_type", Variant::String("rectangle".to_string()));
        node.set_property("shape_extents", Variant::Vector2(Vector2::new(40.0, 30.0)));
        tree.add_child(root, node).unwrap();

        let fb = render_scene(&tree, None, 300, 300);
        // Should have green collision outline pixels.
        let green_count = fb
            .pixels
            .iter()
            .filter(|&&p| p.g > 0.5 && p.r < 0.2)
            .count();
        assert!(
            green_count > 10,
            "rectangle CollisionShape2D should render green outline, got {green_count}"
        );
    }

    #[test]
    fn render_collision_shape_capsule() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let mut node = Node::new("Shape", "CollisionShape2D");
        node.set_property("position", Variant::Vector2(Vector2::new(100.0, 100.0)));
        node.set_property("shape_type", Variant::String("capsule".to_string()));
        node.set_property("shape_radius", Variant::Float(10.0));
        node.set_property("shape_height", Variant::Float(40.0));
        tree.add_child(root, node).unwrap();

        let fb = render_scene(&tree, None, 300, 300);
        let green_count = fb
            .pixels
            .iter()
            .filter(|&&p| p.g > 0.5 && p.r < 0.2)
            .count();
        assert!(
            green_count > 10,
            "capsule CollisionShape2D should render green shape, got {green_count}"
        );
    }

    #[test]
    fn render_collision_shape_default_when_no_type() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let mut node = Node::new("Shape", "CollisionShape2D");
        node.set_property("position", Variant::Vector2(Vector2::new(100.0, 100.0)));
        // No shape_type set — should fall back to default circle.
        tree.add_child(root, node).unwrap();

        let fb = render_scene(&tree, None, 300, 300);
        let has_green = fb
            .pixels
            .iter()
            .any(|&p| p.g > 0.5 && p.r < 0.2 && p.b < 0.5);
        assert!(
            has_green,
            "CollisionShape2D without shape_type should render default circle"
        );
    }

    #[test]
    fn render_collision_shape_selected_has_handles() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let mut node = Node::new("Shape", "CollisionShape2D");
        node.set_property("position", Variant::Vector2(Vector2::new(100.0, 100.0)));
        node.set_property("shape_type", Variant::String("circle".to_string()));
        node.set_property("shape_radius", Variant::Float(30.0));
        let shape_id = tree.add_child(root, node).unwrap();

        let fb_no_sel = render_scene(&tree, None, 300, 300);
        let fb_sel = render_scene(&tree, Some(shape_id), 300, 300);

        // Selected version should have more colored pixels (handles + selection highlight).
        let count_sel = fb_sel.pixels.iter().filter(|&&p| p != BG_COLOR).count();
        let count_no_sel = fb_no_sel.pixels.iter().filter(|&&p| p != BG_COLOR).count();
        assert!(
            count_sel > count_no_sel,
            "selected CollisionShape2D should have more rendered pixels (handles)"
        );
    }

    // -----------------------------------------------------------------------
    // Physics body rendering tests
    // -----------------------------------------------------------------------

    #[test]
    fn render_character_body_2d() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let mut node = Node::new("Player", "CharacterBody2D");
        node.set_property("position", Variant::Vector2(Vector2::new(100.0, 100.0)));
        tree.add_child(root, node).unwrap();

        let fb = render_scene(&tree, None, 300, 300);
        // CharacterBody2D should render blue outline.
        let has_blue = fb.pixels.iter().any(|&p| p.b > 0.5 && p.r < 0.5);
        assert!(has_blue, "CharacterBody2D should render blue outline");
    }

    #[test]
    fn render_rigid_body_2d() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let mut node = Node::new("Ball", "RigidBody2D");
        node.set_property("position", Variant::Vector2(Vector2::new(100.0, 100.0)));
        tree.add_child(root, node).unwrap();

        let fb = render_scene(&tree, None, 300, 300);
        // RigidBody2D should render yellow outline + gravity arrow.
        let has_yellow = fb
            .pixels
            .iter()
            .any(|&p| p.r > 0.8 && p.g > 0.7 && p.b < 0.3);
        assert!(has_yellow, "RigidBody2D should render yellow outline");
    }

    #[test]
    fn render_static_body_2d() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let mut node = Node::new("Ground", "StaticBody2D");
        node.set_property("position", Variant::Vector2(Vector2::new(100.0, 100.0)));
        tree.add_child(root, node).unwrap();

        let fb = render_scene(&tree, None, 300, 300);
        // StaticBody2D should render gray outline.
        let has_gray = fb
            .pixels
            .iter()
            .any(|&p| p.r > 0.4 && p.r < 0.6 && p.g > 0.4 && p.g < 0.6 && p.b > 0.4 && p.b < 0.6);
        assert!(has_gray, "StaticBody2D should render gray outline");
    }

    #[test]
    fn render_rigid_body_with_velocity_arrow() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let mut node = Node::new("Ball", "RigidBody2D");
        node.set_property("position", Variant::Vector2(Vector2::new(100.0, 100.0)));
        node.set_property("velocity", Variant::Vector2(Vector2::new(50.0, 0.0)));
        tree.add_child(root, node).unwrap();

        let fb = render_scene(&tree, None, 300, 300);
        let yellow_count = fb
            .pixels
            .iter()
            .filter(|&&p| p.r > 0.7 && p.g > 0.6 && p.b < 0.3)
            .count();
        // With velocity arrow there should be significantly more yellow pixels.
        assert!(
            yellow_count > 20,
            "RigidBody2D with velocity should render arrow, got {yellow_count} yellow pixels"
        );
    }

    #[test]
    fn debug_hierarchy_hang_repro() {
        // Reproduce hierarchy.tscn: Node2D parent with Sprite2D child, wide spread positions
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let mut player = Node::new("Player", "Node2D");
        player.set_property("position", Variant::Vector2(Vector2::new(100.0, 200.0)));
        let player_id = tree.add_child(root, player).unwrap();

        // Try without Sprite first
        let mut enemy = Node::new("Enemy", "Node2D");
        enemy.set_property("position", Variant::Vector2(Vector2::new(400.0, 300.0)));
        tree.add_child(root, enemy).unwrap();

        // This should work fine
        let fb = render_scene(&tree, None, 256, 256);
        assert_eq!(fb.width, 256);
        eprintln!("Part 1 OK - no sprite");

        // Now add Sprite child
        let sprite = Node::new("Sprite", "Sprite2D");
        tree.add_child(player_id, sprite).unwrap();

        eprintln!("Part 2 - with sprite, starting render...");
        let fb = render_scene(&tree, None, 256, 256);
        assert_eq!(fb.width, 256);
        eprintln!("Part 2 OK");
    }

    /// A `NavigationRegion2D` at the scene origin, optionally carrying a baked
    /// quad navmesh.
    fn nav_region_tree(baked: bool) -> SceneTree {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let mut region = Node::new("Region", "NavigationRegion2D");
        region.set_property("position", Variant::Vector2(Vector2::new(0.0, 0.0)));
        if baked {
            region.set_property(
                "__navmesh_vertices__",
                Variant::PackedVector2Array(vec![
                    Vector2::new(-40.0, -30.0),
                    Vector2::new(40.0, -30.0),
                    Vector2::new(40.0, 30.0),
                    Vector2::new(-40.0, 30.0),
                ]),
            );
            region.set_property(
                "__navmesh_polygons__",
                Variant::Array(vec![Variant::PackedInt32Array(vec![0, 1, 2, 3])].into()),
            );
        }
        tree.add_child(root, region).unwrap();
        tree
    }

    /// Pixels of the navmesh EDGE colour, Godot's opaque `Color(0.5, 1, 1)`.
    ///
    /// ⚠️ Keyed on the edge, not the face: the face is blended at alpha 0.4 and
    /// lands near (0.25, 0.45, 0.46), which no tight predicate can share with
    /// the opaque edge. The fill is asserted separately, by interior coverage.
    /// Verified against every `Color` constant in this file — nothing else has
    /// both green and blue above 0.9.
    fn navmesh_edge_pixels(fb: &FrameBuffer) -> usize {
        fb.pixels
            .iter()
            .filter(|p| p.g > 0.9 && p.b > 0.9 && p.r > 0.3 && p.r < 0.7)
            .count()
    }

    /// 🔑 A BAKED `NavigationRegion2D` DRAWS ITS NAVMESH IN THE EDITOR VIEWPORT.
    ///
    /// ⚠️ The editor viewport is IMMEDIATE-MODE — `render_scene_inner` draws
    /// straight into the framebuffer, it does not build a `Render2DItem` list.
    /// An overlay written as render items (which is the right shape for the
    /// runtime canvas) never appears here, so this test asserts PIXELS.
    ///
    /// The unbaked half is the control: it is the same scene minus the baked
    /// properties, so a pass cannot come from some other cyan chrome.
    #[test]
    fn a_baked_navigation_region_draws_its_navmesh_in_the_editor_viewport() {
        let baked = render_scene(&nav_region_tree(true), None, 320, 240);
        let unbaked = render_scene(&nav_region_tree(false), None, 320, 240);

        let drawn = navmesh_edge_pixels(&baked);
        let control = navmesh_edge_pixels(&unbaked);
        assert!(
            drawn > control,
            "a baked region must add navmesh pixels: baked={drawn}, unbaked={control}"
        );
        assert!(
            drawn >= 100,
            "an 80x60 quad outline should cover a few hundred pixels, got {drawn} \
             — a handful means only a corner or a single edge landed"
        );

        // ⚠️ The FACE fill, asserted through the interior rather than the edge
        // predicate. A centre pixel that is still the background means only the
        // outline drew — which is what shipped before the blended fill existed.
        let centre = (baked.height / 2 * baked.width + baked.width / 2) as usize;
        assert_ne!(
            baked.pixels[centre], BG_COLOR,
            "the navmesh interior must be tinted by the filled face"
        );
        // ...and TINTED, not painted over: an opaque fill would erase whatever
        // the scene draws underneath, which is the bug `fill_polygon_blended`
        // exists to avoid.
        assert_ne!(
            baked.pixels[centre], COLOR_NAVMESH_FACE,
            "the face must be blended over the background, not written opaque"
        );
    }

    /// 🔑 AN UNBAKED REGION DRAWS NO NAVMESH.
    ///
    /// "Not baked yet" must look like nothing, not like an empty box at the
    /// origin — an author needs to be able to tell the two apart, which is the
    /// same reason the overlay draws a DISABLED region greyed rather than
    /// skipping it.
    #[test]
    fn an_unbaked_navigation_region_draws_no_navmesh() {
        let fb = render_scene(&nav_region_tree(false), None, 320, 240);
        assert_eq!(
            navmesh_edge_pixels(&fb),
            0,
            "nothing in the default chrome is navmesh-cyan, so an unbaked \
             region must contribute no such pixels"
        );
    }

    /// 🔑 THE NAVMESH IS PLACED ONCE, NOT TWICE.
    ///
    /// ⚠️ `navigation_polygons_local` returns NODE-LOCAL points and the caller
    /// has already mapped the region's GLOBAL position into screen space. A draw
    /// routine that used the pre-offset points from `navigation_overlay_2d`
    /// would apply the position twice — invisible at the origin, which is why
    /// this region sits at (120, 90).
    ///
    /// Asserted as a bounding box rather than exact pixels: the mesh must land
    /// around the region's own screen position, and double-offsetting puts it
    /// twice as far from centre.
    #[test]
    fn the_navmesh_follows_the_regions_position_exactly_once() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let mut region = Node::new("Region", "NavigationRegion2D");
        region.set_property("position", Variant::Vector2(Vector2::new(120.0, 90.0)));
        region.set_property(
            "__navmesh_vertices__",
            Variant::PackedVector2Array(vec![
                Vector2::new(-10.0, -10.0),
                Vector2::new(10.0, -10.0),
                Vector2::new(10.0, 10.0),
                Vector2::new(-10.0, 10.0),
            ]),
        );
        region.set_property(
            "__navmesh_polygons__",
            Variant::Array(vec![Variant::PackedInt32Array(vec![0, 1, 2, 3])].into()),
        );
        tree.add_child(root, region).unwrap();

        let fb = render_scene(&tree, None, 320, 240);
        let xs: Vec<usize> = (0..fb.pixels.len())
            .filter(|&i| {
                let p = fb.pixels[i];
                p.g > 0.9 && p.b > 0.9 && p.r > 0.3 && p.r < 0.7
            })
            .map(|i| i % fb.width as usize)
            .collect();
        assert!(!xs.is_empty(), "the navmesh must be drawn at all");

        // The scene is centred on its own bounds, so a single region lands near
        // the middle of the frame whatever its authored position. Double
        // offsetting breaks that — the mesh walks off toward one edge.
        let (min_x, max_x) = (
            *xs.iter().min().unwrap() as f32,
            *xs.iter().max().unwrap() as f32,
        );
        let mid = (min_x + max_x) / 2.0;
        let centre = fb.width as f32 / 2.0;
        assert!(
            (mid - centre).abs() < 40.0,
            "the navmesh should sit near the centred region (x~{centre}), got \
             x~{mid} spanning {min_x}..{max_x} — a doubled offset walks it away"
        );
    }
}
