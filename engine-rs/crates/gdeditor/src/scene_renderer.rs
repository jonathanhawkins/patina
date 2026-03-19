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
const COLOR_CHARBODY: Color = Color::new(0.3, 0.5, 1.0, 1.0); // blue outline
const COLOR_RIGIDBODY: Color = Color::new(1.0, 0.85, 0.0, 1.0); // yellow outline
const COLOR_STATICBODY: Color = Color::new(0.5, 0.5, 0.5, 1.0); // gray outline
const COLOR_CONTROL: Color = Color::new(0.7, 0.3, 0.9, 1.0); // purple
const COLOR_DEFAULT: Color = Color::new(0.8, 0.8, 0.8, 1.0); // white-ish
const COLOR_SELECTED: Color = Color::new(1.0, 0.85, 0.0, 1.0); // bright amber
const COLOR_NODE_DOT: Color = Color::new(0.5, 0.5, 0.5, 1.0); // gray
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
    render_scene_inner(tree, selected, width, height, zoom, pan, None)
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
) -> FrameBuffer {
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

    // Walk all nodes in tree order and draw them.
    let node_ids = tree.all_nodes_in_tree_order();
    for &node_id in &node_ids {
        let node = match tree.get_node(node_id) {
            Some(n) => n,
            None => continue,
        };

        let world_pos = extract_position(node);
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
            "CollisionShape2D" => {
                draw_collision_shape_from_node(&mut fb, node, pos, z, is_selected)
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

        let pos = extract_position(node);
        if pos.x == 0.0 && pos.y == 0.0 && !node.has_property("position") {
            continue;
        }

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
fn extract_size(node: &gdscene::node::Node) -> Vector2 {
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
        let x_end = ((pos.x + half_w) as i32).min(fb.width as i32) as u32;
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
        let mut draw_tex = tex;
        if flip_h {
            draw_tex = draw_tex.flip_horizontal();
        }
        if flip_v {
            draw_tex = draw_tex.flip_vertical();
        }
        let sw = draw_tex.width as f32 * zoom;
        let sh = draw_tex.height as f32 * zoom;
        let dx = pos.x + offset.x * zoom - sw / 2.0;
        let dy = pos.y + offset.y * zoom - sh / 2.0;
        draw::draw_texture_rect_blended(
            fb,
            &draw_tex,
            Rect2::new(Vector2::new(dx, dy), Vector2::new(sw, sh)),
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

/// Returns a 5x7 bitmap for common characters. Each `u8` has its low 5 bits
/// representing pixel columns for that row.
fn char_bitmap(ch: char) -> [u8; 7] {
    match ch {
        '0' => [
            0b01110, 0b10001, 0b10011, 0b10101, 0b11001, 0b10001, 0b01110,
        ],
        '1' => [
            0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110,
        ],
        '2' => [
            0b01110, 0b10001, 0b00001, 0b00110, 0b01000, 0b10000, 0b11111,
        ],
        '3' => [
            0b01110, 0b10001, 0b00001, 0b00110, 0b00001, 0b10001, 0b01110,
        ],
        '4' => [
            0b00010, 0b00110, 0b01010, 0b10010, 0b11111, 0b00010, 0b00010,
        ],
        '5' => [
            0b11111, 0b10000, 0b11110, 0b00001, 0b00001, 0b10001, 0b01110,
        ],
        '6' => [
            0b01110, 0b10000, 0b11110, 0b10001, 0b10001, 0b10001, 0b01110,
        ],
        '7' => [
            0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b01000, 0b01000,
        ],
        '8' => [
            0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110,
        ],
        '9' => [
            0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b00001, 0b01110,
        ],
        'A' => [
            0b01110, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001,
        ],
        'a' => [
            0b00000, 0b00000, 0b01110, 0b00001, 0b01111, 0b10001, 0b01111,
        ],
        'B' => [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10001, 0b10001, 0b11110,
        ],
        'C' => [
            0b01110, 0b10001, 0b10000, 0b10000, 0b10000, 0b10001, 0b01110,
        ],
        'D' => [
            0b11110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b11110,
        ],
        'E' => [
            0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b11111,
        ],
        'F' => [
            0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b10000,
        ],
        '-' => [
            0b00000, 0b00000, 0b00000, 0b11111, 0b00000, 0b00000, 0b00000,
        ],
        ' ' => [
            0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b00000,
        ],
        _ => {
            // For unrecognized chars, draw a small filled block.
            if ch.is_ascii_uppercase() || ch.is_ascii_lowercase() || ch.is_ascii_digit() {
                // Generic letter: filled square with gap.
                [
                    0b11111, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b11111,
                ]
            } else {
                [
                    0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b00000,
                ]
            }
        }
    }
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

        let class = node.class_name();
        let node_pos = extract_position(node);

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

        let class = node.class_name();
        let node_pos = extract_position(node);

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
}
