//! Scene renderer for the editor viewport.
//!
//! Renders a visual representation of the scene tree into a [`FrameBuffer`],
//! including a background grid, node representations based on class type,
//! and selection highlighting.

use gdcore::math::{Color, Rect2, Vector2};
use gdrender2d::draw;
use gdrender2d::renderer::FrameBuffer;
use gdscene::node::NodeId;
use gdscene::SceneTree;
use gdvariant::Variant;

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
const COLOR_CONTROL: Color = Color::new(0.7, 0.3, 0.9, 1.0); // purple
const COLOR_DEFAULT: Color = Color::new(0.8, 0.8, 0.8, 1.0); // white-ish
const COLOR_SELECTED: Color = Color::new(1.0, 0.85, 0.0, 1.0); // bright amber
const COLOR_NODE_DOT: Color = Color::new(0.5, 0.5, 0.5, 1.0); // gray

/// Renders the scene tree into a framebuffer for the editor viewport.
///
/// Draws a grid background, visual representations of each node based on
/// its class name, and highlights the selected node if any.
pub fn render_scene(
    tree: &SceneTree,
    selected: Option<NodeId>,
    width: u32,
    height: u32,
) -> FrameBuffer {
    let mut fb = FrameBuffer::new(width, height, BG_COLOR);

    // Compute camera offset to center the scene.
    let bounds = compute_scene_bounds(tree);
    let center_x = bounds.position.x + bounds.size.x / 2.0;
    let center_y = bounds.position.y + bounds.size.y / 2.0;
    let offset_x = width as f32 / 2.0 - center_x;
    let offset_y = height as f32 / 2.0 - center_y;

    // Draw grid.
    draw_grid(&mut fb, offset_x, offset_y);

    // Walk all nodes in tree order and draw them.
    let node_ids = tree.all_nodes_in_tree_order();
    for &node_id in &node_ids {
        let node = match tree.get_node(node_id) {
            Some(n) => n,
            None => continue,
        };

        let pos = extract_position(node) + Vector2::new(offset_x, offset_y);
        let class = node.class_name();
        let is_selected = selected == Some(node_id);

        // Draw node representation based on class.
        match class {
            "Node2D" => draw_node2d_diamond(&mut fb, pos, COLOR_NODE2D),
            "Sprite2D" => draw_sprite2d_rect(&mut fb, pos, COLOR_SPRITE2D),
            "Camera2D" => draw_camera2d_outline(&mut fb, pos, COLOR_CAMERA2D, width, height),
            "Control" | "Label" | "Button" => {
                let size = extract_size(node);
                draw_control_rect(&mut fb, pos, size, COLOR_CONTROL);
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
            draw_selection_highlight(&mut fb, pos, class);
            // Selected node dot (amber).
            draw::fill_circle(
                &mut fb,
                Vector2::new(pos.x, pos.y - 16.0),
                3.0,
                COLOR_SELECTED,
            );
        } else if class != "Node" || node.parent().is_some() {
            // Gray dot above non-root nodes.
            draw::fill_circle(
                &mut fb,
                Vector2::new(pos.x, pos.y - 16.0),
                2.0,
                COLOR_NODE_DOT,
            );
        }
    }

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
fn extract_position(node: &gdscene::node::Node) -> Vector2 {
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

/// Draws the background grid.
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

/// Draws a diamond shape for Node2D nodes.
fn draw_node2d_diamond(fb: &mut FrameBuffer, pos: Vector2, color: Color) {
    let s = 6.0;
    let top = Vector2::new(pos.x, pos.y - s);
    let right = Vector2::new(pos.x + s, pos.y);
    let bottom = Vector2::new(pos.x, pos.y + s);
    let left = Vector2::new(pos.x - s, pos.y);
    draw::draw_line(fb, top, right, color, 1.0);
    draw::draw_line(fb, right, bottom, color, 1.0);
    draw::draw_line(fb, bottom, left, color, 1.0);
    draw::draw_line(fb, left, top, color, 1.0);
}

/// Draws a filled rectangle for Sprite2D nodes.
fn draw_sprite2d_rect(fb: &mut FrameBuffer, pos: Vector2, color: Color) {
    let rect = Rect2::new(
        Vector2::new(pos.x - 10.0, pos.y - 10.0),
        Vector2::new(20.0, 20.0),
    );
    draw::fill_rect(fb, rect, color);
}

/// Draws an outline rectangle for Camera2D nodes.
fn draw_camera2d_outline(fb: &mut FrameBuffer, pos: Vector2, color: Color, vw: u32, vh: u32) {
    let hw = vw as f32 / 4.0;
    let hh = vh as f32 / 4.0;
    let tl = Vector2::new(pos.x - hw, pos.y - hh);
    let tr = Vector2::new(pos.x + hw, pos.y - hh);
    let br = Vector2::new(pos.x + hw, pos.y + hh);
    let bl = Vector2::new(pos.x - hw, pos.y + hh);
    draw::draw_line(fb, tl, tr, color, 1.0);
    draw::draw_line(fb, tr, br, color, 1.0);
    draw::draw_line(fb, br, bl, color, 1.0);
    draw::draw_line(fb, bl, tl, color, 1.0);
}

/// Draws a filled rectangle for Control-derived nodes.
fn draw_control_rect(fb: &mut FrameBuffer, pos: Vector2, size: Vector2, color: Color) {
    let rect = Rect2::new(pos, size);
    draw::fill_rect(fb, rect, color);
}

/// Draws a selection highlight around a node.
fn draw_selection_highlight(fb: &mut FrameBuffer, pos: Vector2, class: &str) {
    let (hw, hh) = match class {
        "Sprite2D" => (13.0, 13.0),
        "Node2D" => (9.0, 9.0),
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
}

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
}
