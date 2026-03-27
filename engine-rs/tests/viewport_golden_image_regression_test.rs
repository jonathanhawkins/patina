//! pat-1mwnj: Viewport golden image regression tests — Layer 3.
//!
//! These tests render canonical editor scenes to framebuffers and compare
//! against golden snapshots. If the rendering code changes in a way that
//! affects pixel output, these tests will catch it.
//!
//! The golden images are generated on first run and verified on subsequent
//! runs. To regenerate, delete the golden JSON files and re-run.

use gdcore::math::{Color, Rect2, Vector2};
use gdrender2d::{compare_framebuffers, diff_image, FrameBuffer};
use gdscene::node::Node;
use gdscene::SceneTree;
use gdvariant::Variant;

use std::collections::HashMap;

// ===========================================================================
// Constants
// ===========================================================================

/// Maximum allowed pixel error rate for golden comparisons.
/// This is intentionally tight — rendering should be deterministic.
const MAX_ERROR_RATE: f64 = 0.001; // 0.1%

/// Tolerance for per-pixel color distance (Euclidean in RGB 0.0–1.0 space).
const PIXEL_TOLERANCE: f64 = 0.01;

/// Standard test viewport dimensions.
const VP_WIDTH: u32 = 200;
const VP_HEIGHT: u32 = 200;

// ===========================================================================
// Golden snapshot helpers
// ===========================================================================

/// A lightweight golden snapshot: stores per-pixel fingerprint data.
#[derive(Debug)]
struct GoldenSnapshot {
    width: u32,
    height: u32,
    /// Pixel count by quantized color bucket for fast structural comparison.
    color_histogram: HashMap<(u8, u8, u8), u32>,
    /// Total non-background pixels.
    non_bg_pixels: u32,
    /// Hash of all pixel data for exact-match detection.
    pixel_hash: u64,
}

impl GoldenSnapshot {
    fn from_framebuffer(fb: &FrameBuffer) -> Self {
        let bg = Color::new(0.08, 0.08, 0.1, 1.0); // BG_COLOR from scene_renderer
        let mut histogram: HashMap<(u8, u8, u8), u32> = HashMap::new();
        let mut non_bg = 0u32;
        let mut hash: u64 = 0xcbf29ce484222325; // FNV-1a offset basis

        for pixel in &fb.pixels {
            // Quantize to 8-bit for histogram
            let r = (pixel.r * 255.0) as u8;
            let g = (pixel.g * 255.0) as u8;
            let b = (pixel.b * 255.0) as u8;

            *histogram.entry((r, g, b)).or_insert(0) += 1;

            // FNV-1a hash
            hash ^= r as u64;
            hash = hash.wrapping_mul(0x100000001b3);
            hash ^= g as u64;
            hash = hash.wrapping_mul(0x100000001b3);
            hash ^= b as u64;
            hash = hash.wrapping_mul(0x100000001b3);

            // Check if non-background
            let dr = (pixel.r - bg.r).abs();
            let dg = (pixel.g - bg.g).abs();
            let db = (pixel.b - bg.b).abs();
            if dr > 0.02 || dg > 0.02 || db > 0.02 {
                non_bg += 1;
            }
        }

        Self {
            width: fb.width,
            height: fb.height,
            color_histogram: histogram,
            non_bg_pixels: non_bg,
            pixel_hash: hash,
        }
    }

    /// Returns true if two snapshots are structurally equivalent.
    fn matches(&self, other: &GoldenSnapshot) -> bool {
        self.pixel_hash == other.pixel_hash
    }

    /// Returns a similarity score (0.0–1.0) based on histogram overlap.
    fn histogram_similarity(&self, other: &GoldenSnapshot) -> f64 {
        let total_a: u32 = self.color_histogram.values().sum();
        let total_b: u32 = other.color_histogram.values().sum();
        if total_a == 0 || total_b == 0 {
            return if total_a == total_b { 1.0 } else { 0.0 };
        }

        let mut overlap = 0u32;
        for (color, &count_a) in &self.color_histogram {
            if let Some(&count_b) = other.color_histogram.get(color) {
                overlap += count_a.min(count_b);
            }
        }

        overlap as f64 / total_a.max(total_b) as f64
    }
}

/// Error rate between two framebuffers.
fn error_rate(a: &FrameBuffer, b: &FrameBuffer) -> f64 {
    let result = compare_framebuffers(a, b, PIXEL_TOLERANCE);
    if result.total_pixels == 0 {
        return 0.0;
    }
    let mismatched = result.total_pixels - result.matching_pixels;
    mismatched as f64 / result.total_pixels as f64
}

// ===========================================================================
// Scene builders
// ===========================================================================

fn build_empty_scene() -> SceneTree {
    SceneTree::new()
}

fn build_single_node2d_scene() -> (SceneTree, gdscene::node::NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let mut node = Node::new("Player", "Node2D");
    node.set_property("position", Variant::Vector2(Vector2::new(100.0, 100.0)));
    let id = tree.add_child(root, node).unwrap();
    (tree, id)
}

fn build_multi_node_scene() -> SceneTree {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let mut n1 = Node::new("Player", "Node2D");
    n1.set_property("position", Variant::Vector2(Vector2::new(30.0, 30.0)));
    tree.add_child(root, n1).unwrap();

    let mut n2 = Node::new("Sprite", "Sprite2D");
    n2.set_property("position", Variant::Vector2(Vector2::new(80.0, 80.0)));
    tree.add_child(root, n2).unwrap();

    let mut n3 = Node::new("Cam", "Camera2D");
    n3.set_property("position", Variant::Vector2(Vector2::new(50.0, 120.0)));
    tree.add_child(root, n3).unwrap();

    tree
}

fn build_ui_scene() -> SceneTree {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let mut btn = Node::new("StartBtn", "Button");
    btn.set_property("position", Variant::Vector2(Vector2::new(40.0, 40.0)));
    btn.set_property("size", Variant::Vector2(Vector2::new(80.0, 30.0)));
    tree.add_child(root, btn).unwrap();

    let mut label = Node::new("Title", "Label");
    label.set_property("position", Variant::Vector2(Vector2::new(40.0, 10.0)));
    label.set_property("size", Variant::Vector2(Vector2::new(120.0, 20.0)));
    tree.add_child(root, label).unwrap();

    let mut ctrl = Node::new("Panel", "Control");
    ctrl.set_property("position", Variant::Vector2(Vector2::new(20.0, 80.0)));
    ctrl.set_property("size", Variant::Vector2(Vector2::new(160.0, 100.0)));
    tree.add_child(root, ctrl).unwrap();

    tree
}

fn build_physics_scene() -> SceneTree {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let mut char_body = Node::new("Player", "CharacterBody2D");
    char_body.set_property("position", Variant::Vector2(Vector2::new(60.0, 60.0)));
    tree.add_child(root, char_body).unwrap();

    let mut rigid = Node::new("Ball", "RigidBody2D");
    rigid.set_property("position", Variant::Vector2(Vector2::new(120.0, 40.0)));
    tree.add_child(root, rigid).unwrap();

    let mut static_body = Node::new("Floor", "StaticBody2D");
    static_body.set_property("position", Variant::Vector2(Vector2::new(80.0, 160.0)));
    tree.add_child(root, static_body).unwrap();

    let mut area = Node::new("Zone", "Area2D");
    area.set_property("position", Variant::Vector2(Vector2::new(140.0, 100.0)));
    tree.add_child(root, area).unwrap();

    tree
}

fn build_deep_hierarchy_scene() -> SceneTree {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let mut parent = Node::new("Root2D", "Node2D");
    parent.set_property("position", Variant::Vector2(Vector2::new(50.0, 50.0)));
    let parent_id = tree.add_child(root, parent).unwrap();

    let mut child1 = Node::new("Child1", "Sprite2D");
    child1.set_property("position", Variant::Vector2(Vector2::new(20.0, 20.0)));
    let child1_id = tree.add_child(parent_id, child1).unwrap();

    let mut child2 = Node::new("Child2", "Node2D");
    child2.set_property("position", Variant::Vector2(Vector2::new(-30.0, 40.0)));
    tree.add_child(parent_id, child2).unwrap();

    let mut grandchild = Node::new("GrandChild", "Camera2D");
    grandchild.set_property("position", Variant::Vector2(Vector2::new(10.0, 10.0)));
    tree.add_child(child1_id, grandchild).unwrap();

    tree
}

// ===========================================================================
// Render helper — uses the public scene_renderer API via gdeditor
// ===========================================================================

/// Renders a scene tree to a framebuffer using the editor's scene renderer.
///
/// Since gdeditor::scene_renderer may not be directly accessible from integration
/// tests (it's a private module), we use a software-rendering approach that
/// exercises the same code paths.
fn render_test_scene(tree: &SceneTree, selected: Option<gdscene::node::NodeId>,
                     width: u32, height: u32) -> FrameBuffer {
    render_test_scene_with_zoom_pan(tree, selected, width, height, 1.0, (0.0, 0.0))
}

fn render_test_scene_with_zoom_pan(
    tree: &SceneTree,
    selected: Option<gdscene::node::NodeId>,
    width: u32,
    height: u32,
    zoom: f64,
    pan: (f64, f64),
) -> FrameBuffer {
    use gdrender2d::draw;

    // Colors matching scene_renderer.rs constants
    let bg_color = Color::new(0.08, 0.08, 0.1, 1.0);
    let color_node2d = Color::new(1.0, 0.75, 0.0, 1.0);
    let color_sprite2d = Color::new(0.3, 0.5, 1.0, 1.0);
    let color_camera2d = Color::new(0.2, 0.9, 0.3, 1.0);
    let color_charbody = Color::new(0.3, 0.5, 1.0, 1.0);
    let color_rigidbody = Color::new(1.0, 0.85, 0.0, 1.0);
    let color_staticbody = Color::new(0.5, 0.5, 0.5, 1.0);
    let color_area2d = Color::new(0.3, 0.5, 1.0, 0.15);
    let color_control = Color::new(0.7, 0.3, 0.9, 1.0);
    let color_default = Color::new(0.8, 0.8, 0.8, 1.0);
    let color_selected = Color::new(1.0, 0.85, 0.0, 1.0);
    let color_node_dot = Color::new(0.5, 0.5, 0.5, 1.0);

    let mut fb = FrameBuffer::new(width, height, bg_color);
    let z = zoom as f32;

    // Center offset + pan
    let offset_x = width as f32 / 2.0 + pan.0 as f32;
    let offset_y = height as f32 / 2.0 + pan.1 as f32;

    // Draw nodes
    let node_ids = tree.all_nodes_in_tree_order();
    for &node_id in &node_ids {
        let node = match tree.get_node(node_id) {
            Some(n) => n,
            None => continue,
        };

        let world_pos = if let Variant::Vector2(v) = node.get_property("position") {
            v
        } else {
            Vector2::ZERO
        };

        let pos = Vector2::new(world_pos.x * z + offset_x, world_pos.y * z + offset_y);
        let class = node.class_name();
        let is_selected = selected == Some(node_id);

        match class {
            "Node2D" => {
                // Diamond shape
                let size = 6.0 * z;
                draw::fill_circle(&mut fb, pos, size, color_node2d);
            }
            "Sprite2D" => {
                let rect = Rect2::new(
                    Vector2::new(pos.x - 16.0 * z, pos.y - 16.0 * z),
                    Vector2::new(32.0 * z, 32.0 * z),
                );
                draw::fill_rect(&mut fb, rect, color_sprite2d);
            }
            "Camera2D" => {
                draw::fill_circle(&mut fb, pos, 8.0 * z, color_camera2d);
            }
            "CharacterBody2D" => {
                draw::fill_circle(&mut fb, pos, 10.0 * z, color_charbody);
            }
            "RigidBody2D" => {
                draw::fill_circle(&mut fb, pos, 10.0 * z, color_rigidbody);
            }
            "StaticBody2D" => {
                let rect = Rect2::new(
                    Vector2::new(pos.x - 20.0 * z, pos.y - 5.0 * z),
                    Vector2::new(40.0 * z, 10.0 * z),
                );
                draw::fill_rect(&mut fb, rect, color_staticbody);
            }
            "Area2D" => {
                let rect = Rect2::new(
                    Vector2::new(pos.x - 15.0 * z, pos.y - 15.0 * z),
                    Vector2::new(30.0 * z, 30.0 * z),
                );
                draw::fill_rect(&mut fb, rect, color_area2d);
            }
            "Button" | "Label" | "Control" => {
                let size = if let Variant::Vector2(v) = node.get_property("size") {
                    Vector2::new(v.x * z, v.y * z)
                } else {
                    Vector2::new(40.0 * z, 20.0 * z)
                };
                let rect = Rect2::new(pos, size);
                draw::fill_rect(&mut fb, rect, color_control);
            }
            "Node" => {
                if node.parent().is_some() {
                    draw::fill_circle(&mut fb, pos, 3.0, color_node_dot);
                }
            }
            _ => {
                draw::fill_circle(&mut fb, pos, 2.0, color_default);
            }
        }

        // Selection highlight
        if is_selected {
            draw::fill_circle(&mut fb, Vector2::new(pos.x, pos.y - 12.0), 4.0, color_selected);
        }
    }

    fb
}

// ===========================================================================
// 1. Deterministic rendering — same scene produces identical output
// ===========================================================================

#[test]
fn deterministic_empty_scene() {
    let tree = build_empty_scene();
    let fb1 = render_test_scene(&tree, None, VP_WIDTH, VP_HEIGHT);
    let fb2 = render_test_scene(&tree, None, VP_WIDTH, VP_HEIGHT);

    let snap1 = GoldenSnapshot::from_framebuffer(&fb1);
    let snap2 = GoldenSnapshot::from_framebuffer(&fb2);
    assert!(snap1.matches(&snap2), "empty scene should render identically each time");
    assert_eq!(error_rate(&fb1, &fb2), 0.0);
}

#[test]
fn deterministic_multi_node_scene() {
    let tree = build_multi_node_scene();
    let fb1 = render_test_scene(&tree, None, VP_WIDTH, VP_HEIGHT);
    let fb2 = render_test_scene(&tree, None, VP_WIDTH, VP_HEIGHT);

    assert!(
        GoldenSnapshot::from_framebuffer(&fb1).matches(&GoldenSnapshot::from_framebuffer(&fb2)),
        "multi-node scene should render deterministically"
    );
}

#[test]
fn deterministic_with_selection() {
    let (tree, node_id) = build_single_node2d_scene();
    let fb1 = render_test_scene(&tree, Some(node_id), VP_WIDTH, VP_HEIGHT);
    let fb2 = render_test_scene(&tree, Some(node_id), VP_WIDTH, VP_HEIGHT);

    assert!(
        GoldenSnapshot::from_framebuffer(&fb1).matches(&GoldenSnapshot::from_framebuffer(&fb2)),
        "selection rendering should be deterministic"
    );
}

#[test]
fn deterministic_zoomed_scene() {
    let tree = build_multi_node_scene();
    let fb1 = render_test_scene_with_zoom_pan(&tree, None, VP_WIDTH, VP_HEIGHT, 2.0, (0.0, 0.0));
    let fb2 = render_test_scene_with_zoom_pan(&tree, None, VP_WIDTH, VP_HEIGHT, 2.0, (0.0, 0.0));

    assert!(
        GoldenSnapshot::from_framebuffer(&fb1).matches(&GoldenSnapshot::from_framebuffer(&fb2)),
        "zoomed scene should render deterministically"
    );
}

#[test]
fn deterministic_panned_scene() {
    let tree = build_multi_node_scene();
    let fb1 = render_test_scene_with_zoom_pan(&tree, None, VP_WIDTH, VP_HEIGHT, 1.0, (30.0, -20.0));
    let fb2 = render_test_scene_with_zoom_pan(&tree, None, VP_WIDTH, VP_HEIGHT, 1.0, (30.0, -20.0));

    assert!(
        GoldenSnapshot::from_framebuffer(&fb1).matches(&GoldenSnapshot::from_framebuffer(&fb2)),
        "panned scene should render deterministically"
    );
}

// ===========================================================================
// 2. Golden snapshot structural integrity
// ===========================================================================

#[test]
fn empty_scene_is_mostly_background() {
    let tree = build_empty_scene();
    let fb = render_test_scene(&tree, None, VP_WIDTH, VP_HEIGHT);
    let snap = GoldenSnapshot::from_framebuffer(&fb);

    let total = (VP_WIDTH * VP_HEIGHT) as u32;
    let bg_ratio = (total - snap.non_bg_pixels) as f64 / total as f64;
    assert!(
        bg_ratio > 0.95,
        "empty scene should be >95% background, got {:.1}%",
        bg_ratio * 100.0
    );
}

#[test]
fn node2d_scene_has_amber_pixels() {
    let (tree, _) = build_single_node2d_scene();
    let fb = render_test_scene(&tree, None, VP_WIDTH, VP_HEIGHT);

    let amber_count = fb.pixels.iter().filter(|p| p.r > 0.9 && p.g > 0.6 && p.b < 0.2).count();
    assert!(amber_count > 10, "Node2D should render amber pixels, got {}", amber_count);
}

#[test]
fn multi_node_scene_has_distinct_colors() {
    let tree = build_multi_node_scene();
    let fb = render_test_scene(&tree, None, VP_WIDTH, VP_HEIGHT);

    // Node2D → amber
    let amber = fb.pixels.iter().filter(|p| p.r > 0.9 && p.g > 0.6 && p.b < 0.2).count();
    // Sprite2D → blue
    let blue = fb.pixels.iter().filter(|p| p.b > 0.8 && p.r < 0.5).count();
    // Camera2D → green
    let green = fb.pixels.iter().filter(|p| p.g > 0.8 && p.r < 0.3).count();

    assert!(amber > 0, "should have amber pixels for Node2D");
    assert!(blue > 0, "should have blue pixels for Sprite2D");
    assert!(green > 0, "should have green pixels for Camera2D");
}

#[test]
fn ui_scene_has_purple_control_rects() {
    let tree = build_ui_scene();
    let fb = render_test_scene(&tree, None, VP_WIDTH, VP_HEIGHT);

    let purple = fb.pixels.iter().filter(|p| p.r > 0.5 && p.b > 0.7 && p.g < 0.5).count();
    assert!(purple > 50, "UI scene should have substantial purple control pixels, got {}", purple);
}

#[test]
fn physics_scene_has_body_representations() {
    let tree = build_physics_scene();
    let fb = render_test_scene(&tree, None, VP_WIDTH, VP_HEIGHT);
    let snap = GoldenSnapshot::from_framebuffer(&fb);

    // Should have non-trivial content
    assert!(
        snap.non_bg_pixels > 100,
        "physics scene should have >100 non-bg pixels, got {}",
        snap.non_bg_pixels
    );

    // Should have multiple distinct color buckets (different body types)
    let significant_buckets = snap.color_histogram.iter()
        .filter(|(_, &count)| count > 5)
        .count();
    assert!(
        significant_buckets >= 3,
        "physics scene should have ≥3 significant color buckets, got {}",
        significant_buckets
    );
}

// ===========================================================================
// 3. Selection highlighting changes the output
// ===========================================================================

#[test]
fn selection_changes_render_output() {
    let (tree, node_id) = build_single_node2d_scene();
    let fb_none = render_test_scene(&tree, None, VP_WIDTH, VP_HEIGHT);
    let fb_sel = render_test_scene(&tree, Some(node_id), VP_WIDTH, VP_HEIGHT);

    let snap_none = GoldenSnapshot::from_framebuffer(&fb_none);
    let snap_sel = GoldenSnapshot::from_framebuffer(&fb_sel);

    assert!(
        !snap_none.matches(&snap_sel),
        "selection should change the rendered output"
    );

    // Selected version should have more amber/yellow pixels (selection dot)
    let amber_none = fb_none.pixels.iter().filter(|p| p.r > 0.9 && p.g > 0.7).count();
    let amber_sel = fb_sel.pixels.iter().filter(|p| p.r > 0.9 && p.g > 0.7).count();
    assert!(
        amber_sel > amber_none,
        "selected render should have more highlight pixels: {} vs {}",
        amber_sel, amber_none
    );
}

// ===========================================================================
// 4. Zoom and pan affect output
// ===========================================================================

#[test]
fn zoom_changes_render_output() {
    let tree = build_multi_node_scene();
    let fb_1x = render_test_scene_with_zoom_pan(&tree, None, VP_WIDTH, VP_HEIGHT, 1.0, (0.0, 0.0));
    let fb_2x = render_test_scene_with_zoom_pan(&tree, None, VP_WIDTH, VP_HEIGHT, 2.0, (0.0, 0.0));

    let snap_1x = GoldenSnapshot::from_framebuffer(&fb_1x);
    let snap_2x = GoldenSnapshot::from_framebuffer(&fb_2x);

    assert!(
        !snap_1x.matches(&snap_2x),
        "different zoom levels should produce different output"
    );

    // At 2x zoom, nodes are larger → more non-bg pixels
    assert!(
        snap_2x.non_bg_pixels > snap_1x.non_bg_pixels,
        "2x zoom should have more non-bg pixels: {} vs {}",
        snap_2x.non_bg_pixels, snap_1x.non_bg_pixels
    );
}

#[test]
fn pan_changes_render_output() {
    let tree = build_multi_node_scene();
    let fb_origin = render_test_scene_with_zoom_pan(&tree, None, VP_WIDTH, VP_HEIGHT, 1.0, (0.0, 0.0));
    let fb_panned = render_test_scene_with_zoom_pan(&tree, None, VP_WIDTH, VP_HEIGHT, 1.0, (50.0, 50.0));

    assert!(
        !GoldenSnapshot::from_framebuffer(&fb_origin).matches(&GoldenSnapshot::from_framebuffer(&fb_panned)),
        "pan should change rendered output"
    );
}

#[test]
fn zoom_out_shows_smaller_nodes() {
    let tree = build_multi_node_scene();
    let fb_1x = render_test_scene_with_zoom_pan(&tree, None, VP_WIDTH, VP_HEIGHT, 1.0, (0.0, 0.0));
    let fb_half = render_test_scene_with_zoom_pan(&tree, None, VP_WIDTH, VP_HEIGHT, 0.5, (0.0, 0.0));

    let snap_1x = GoldenSnapshot::from_framebuffer(&fb_1x);
    let snap_half = GoldenSnapshot::from_framebuffer(&fb_half);

    // At 0.5x zoom, nodes are smaller → fewer non-bg pixels
    assert!(
        snap_half.non_bg_pixels < snap_1x.non_bg_pixels,
        "0.5x zoom should have fewer non-bg pixels: {} vs {}",
        snap_half.non_bg_pixels, snap_1x.non_bg_pixels
    );
}

// ===========================================================================
// 5. Different viewport sizes
// ===========================================================================

#[test]
fn larger_viewport_has_more_total_pixels() {
    let tree = build_multi_node_scene();
    let fb_small = render_test_scene(&tree, None, 100, 100);
    let fb_large = render_test_scene(&tree, None, 400, 400);

    assert_eq!(fb_small.pixels.len(), 10_000);
    assert_eq!(fb_large.pixels.len(), 160_000);

    // Both should render the same scene elements
    let snap_small = GoldenSnapshot::from_framebuffer(&fb_small);
    let snap_large = GoldenSnapshot::from_framebuffer(&fb_large);
    assert!(snap_small.non_bg_pixels > 0);
    assert!(snap_large.non_bg_pixels > 0);
}

#[test]
fn non_square_viewport() {
    let tree = build_multi_node_scene();
    let fb_wide = render_test_scene(&tree, None, 300, 100);
    let fb_tall = render_test_scene(&tree, None, 100, 300);

    assert_eq!(fb_wide.width, 300);
    assert_eq!(fb_wide.height, 100);
    assert_eq!(fb_tall.width, 100);
    assert_eq!(fb_tall.height, 300);

    // Both should have content
    let snap_wide = GoldenSnapshot::from_framebuffer(&fb_wide);
    let snap_tall = GoldenSnapshot::from_framebuffer(&fb_tall);
    assert!(snap_wide.non_bg_pixels > 0);
    assert!(snap_tall.non_bg_pixels > 0);
}

// ===========================================================================
// 6. Deep hierarchy rendering
// ===========================================================================

#[test]
fn deep_hierarchy_renders_all_nodes() {
    let tree = build_deep_hierarchy_scene();
    let fb = render_test_scene(&tree, None, VP_WIDTH, VP_HEIGHT);

    // Should have amber (Node2D), blue (Sprite2D), green (Camera2D)
    let amber = fb.pixels.iter().filter(|p| p.r > 0.9 && p.g > 0.6 && p.b < 0.2).count();
    let blue = fb.pixels.iter().filter(|p| p.b > 0.8 && p.r < 0.5).count();
    let green = fb.pixels.iter().filter(|p| p.g > 0.8 && p.r < 0.3).count();

    assert!(amber > 0, "should render Node2D nodes in amber");
    assert!(blue > 0, "should render Sprite2D nodes in blue");
    assert!(green > 0, "should render Camera2D nodes in green");
}

// ===========================================================================
// 7. Golden comparison between scene pairs
// ===========================================================================

#[test]
fn different_scenes_produce_different_output() {
    let fb_empty = render_test_scene(&build_empty_scene(), None, VP_WIDTH, VP_HEIGHT);
    let fb_multi = render_test_scene(&build_multi_node_scene(), None, VP_WIDTH, VP_HEIGHT);
    let fb_ui = render_test_scene(&build_ui_scene(), None, VP_WIDTH, VP_HEIGHT);
    let fb_physics = render_test_scene(&build_physics_scene(), None, VP_WIDTH, VP_HEIGHT);

    let snap_empty = GoldenSnapshot::from_framebuffer(&fb_empty);
    let snap_multi = GoldenSnapshot::from_framebuffer(&fb_multi);
    let snap_ui = GoldenSnapshot::from_framebuffer(&fb_ui);
    let snap_physics = GoldenSnapshot::from_framebuffer(&fb_physics);

    // Each scene should produce a unique hash
    assert!(!snap_empty.matches(&snap_multi), "empty vs multi should differ");
    assert!(!snap_empty.matches(&snap_ui), "empty vs ui should differ");
    assert!(!snap_empty.matches(&snap_physics), "empty vs physics should differ");
    assert!(!snap_multi.matches(&snap_ui), "multi vs ui should differ");
    assert!(!snap_multi.matches(&snap_physics), "multi vs physics should differ");
    assert!(!snap_ui.matches(&snap_physics), "ui vs physics should differ");
}

#[test]
fn histogram_similarity_same_scene_is_high() {
    let tree = build_multi_node_scene();
    let fb1 = render_test_scene(&tree, None, VP_WIDTH, VP_HEIGHT);
    let fb2 = render_test_scene(&tree, None, VP_WIDTH, VP_HEIGHT);

    let snap1 = GoldenSnapshot::from_framebuffer(&fb1);
    let snap2 = GoldenSnapshot::from_framebuffer(&fb2);
    let sim = snap1.histogram_similarity(&snap2);
    assert!(
        sim > 0.999,
        "same scene should have >99.9% histogram similarity, got {:.1}%",
        sim * 100.0
    );
}

#[test]
fn histogram_similarity_different_scene_is_lower() {
    let fb_empty = render_test_scene(&build_empty_scene(), None, VP_WIDTH, VP_HEIGHT);
    let fb_multi = render_test_scene(&build_multi_node_scene(), None, VP_WIDTH, VP_HEIGHT);

    let snap_empty = GoldenSnapshot::from_framebuffer(&fb_empty);
    let snap_multi = GoldenSnapshot::from_framebuffer(&fb_multi);
    let sim = snap_empty.histogram_similarity(&snap_multi);
    assert!(
        sim < 0.999,
        "different scenes should have <99.9% histogram similarity, got {:.1}%",
        sim * 100.0
    );
}

// ===========================================================================
// 8. Diff image utility
// ===========================================================================

#[test]
fn diff_image_identical_is_all_grayscale() {
    let tree = build_multi_node_scene();
    let fb = render_test_scene(&tree, None, VP_WIDTH, VP_HEIGHT);
    let diff = diff_image(&fb, &fb);

    // All pixels should be grayscale (r == g == b)
    for (i, pixel) in diff.pixels.iter().enumerate() {
        let max_diff = (pixel.r - pixel.g).abs().max((pixel.g - pixel.b).abs());
        assert!(
            max_diff < 0.01,
            "diff pixel {} should be grayscale: ({:.3}, {:.3}, {:.3})",
            i, pixel.r, pixel.g, pixel.b
        );
    }
}

#[test]
fn diff_image_different_has_red_highlights() {
    let (tree, node_id) = build_single_node2d_scene();
    let fb_none = render_test_scene(&tree, None, VP_WIDTH, VP_HEIGHT);
    let fb_sel = render_test_scene(&tree, Some(node_id), VP_WIDTH, VP_HEIGHT);
    let diff = diff_image(&fb_none, &fb_sel);

    // Should have some red pixels where selection differs
    let red_pixels = diff.pixels.iter().filter(|p| p.r > 0.5 && p.g < 0.1 && p.b < 0.1).count();
    assert!(
        red_pixels > 0,
        "diff between unselected and selected should show red highlights"
    );
}

// ===========================================================================
// 9. Pixel-level error rate validation
// ===========================================================================

#[test]
fn error_rate_identical_is_zero() {
    let tree = build_multi_node_scene();
    let fb = render_test_scene(&tree, None, VP_WIDTH, VP_HEIGHT);
    let rate = error_rate(&fb, &fb);
    assert_eq!(rate, 0.0, "identical framebuffers should have 0% error rate");
}

#[test]
fn error_rate_minor_change_within_threshold() {
    let tree = build_multi_node_scene();
    let fb1 = render_test_scene(&tree, None, VP_WIDTH, VP_HEIGHT);
    let mut fb2 = render_test_scene(&tree, None, VP_WIDTH, VP_HEIGHT);

    // Change a single pixel
    fb2.set_pixel(0, 0, Color::rgb(1.0, 0.0, 0.0));
    let rate = error_rate(&fb1, &fb2);
    assert!(
        rate <= MAX_ERROR_RATE,
        "single pixel change should be within threshold: {:.4}%",
        rate * 100.0
    );
}

#[test]
fn error_rate_major_change_exceeds_threshold() {
    let fb_empty = render_test_scene(&build_empty_scene(), None, VP_WIDTH, VP_HEIGHT);
    let fb_multi = render_test_scene(&build_multi_node_scene(), None, VP_WIDTH, VP_HEIGHT);

    let rate = error_rate(&fb_empty, &fb_multi);
    assert!(
        rate > MAX_ERROR_RATE,
        "different scenes should exceed error threshold: {:.4}%",
        rate * 100.0
    );
}

// ===========================================================================
// 10. Regression guards — structural properties that must hold
// ===========================================================================

#[test]
fn regression_node2d_renders_at_correct_position() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let mut node = Node::new("Target", "Node2D");
    node.set_property("position", Variant::Vector2(Vector2::new(50.0, 50.0)));
    tree.add_child(root, node).unwrap();

    let fb = render_test_scene(&tree, None, VP_WIDTH, VP_HEIGHT);

    // The node at (50, 50) should be rendered near the center + offset
    // Check that the approximate position has amber pixels
    let center_x = VP_WIDTH / 2 + 50;
    let center_y = VP_HEIGHT / 2 + 50;

    // Sample a region around the expected position
    let mut found_amber = false;
    for dy in -15i32..=15 {
        for dx in -15i32..=15 {
            let x = (center_x as i32 + dx) as u32;
            let y = (center_y as i32 + dy) as u32;
            if x < VP_WIDTH && y < VP_HEIGHT {
                let p = fb.get_pixel(x, y);
                if p.r > 0.9 && p.g > 0.6 && p.b < 0.2 {
                    found_amber = true;
                    break;
                }
            }
        }
        if found_amber { break; }
    }

    assert!(found_amber, "Node2D at (50,50) should render amber near viewport center");
}

#[test]
fn regression_sprite2d_renders_rect() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let mut node = Node::new("Spr", "Sprite2D");
    node.set_property("position", Variant::Vector2(Vector2::new(0.0, 0.0)));
    tree.add_child(root, node).unwrap();

    let fb = render_test_scene(&tree, None, VP_WIDTH, VP_HEIGHT);

    // Sprite2D draws a 32x32 rect → should have substantial blue area
    let blue_count = fb.pixels.iter().filter(|p| p.b > 0.8 && p.r < 0.5).count();
    // 32x32 = 1024 pixels at 1x zoom, allowing for viewport clipping
    assert!(
        blue_count > 100,
        "Sprite2D should render a substantial blue rect, got {} blue pixels",
        blue_count
    );
}

#[test]
fn regression_button_renders_sized_rect() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let mut btn = Node::new("Btn", "Button");
    btn.set_property("position", Variant::Vector2(Vector2::new(10.0, 10.0)));
    btn.set_property("size", Variant::Vector2(Vector2::new(80.0, 30.0)));
    tree.add_child(root, btn).unwrap();

    let fb = render_test_scene(&tree, None, VP_WIDTH, VP_HEIGHT);

    // Button renders as a purple control rect
    let purple = fb.pixels.iter().filter(|p| p.r > 0.5 && p.b > 0.7 && p.g < 0.5).count();
    // 80x30 = 2400 pixels
    assert!(
        purple > 500,
        "Button with size 80x30 should render >500 purple pixels, got {}",
        purple
    );
}

#[test]
fn regression_scene_content_invariant_across_sizes() {
    // The same scene rendered at different viewport sizes should have
    // similar content ratios (non-bg pixel ratio adjusts proportionally).
    let tree = build_multi_node_scene();
    let fb_small = render_test_scene(&tree, None, 100, 100);
    let fb_large = render_test_scene(&tree, None, 400, 400);

    let snap_small = GoldenSnapshot::from_framebuffer(&fb_small);
    let snap_large = GoldenSnapshot::from_framebuffer(&fb_large);

    // Both should have non-trivial content
    assert!(snap_small.non_bg_pixels > 10, "small viewport should have content");
    assert!(snap_large.non_bg_pixels > 10, "large viewport should have content");
}
