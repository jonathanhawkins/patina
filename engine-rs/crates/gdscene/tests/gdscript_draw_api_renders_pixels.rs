//! pat-0q2kx.129 — a script's `_draw()` reaches actual pixels, and
//! `queue_redraw()` reflects state changes on the next frame.
//!
//! Godot scripts override `CanvasItem._draw()` and call the `draw_*` family;
//! `queue_redraw()` marks the item so `NOTIFICATION_DRAW` re-runs `_draw` next
//! frame (`scene/main/canvas_item.h:211,272`).
//!
//! `queue_redraw`/`_draw` and the rect/line/circle builtins landed with
//! pat-0q2kx.82, but the stroke and fill family did not: `DrawCommand` had no
//! polyline/polygon/arc variants, and `gdrender2d::polygon_raster` — which
//! already implemented the scanline fill and the capped/jointed stroke, with
//! unit tests — had **no caller anywhere in the workspace**. A script calling
//! `draw_polyline` reached nothing.
//!
//! This drives the whole chain end to end: script `_draw` -> `DrawCommand` ->
//! `build_canvas_viewport` -> `SoftwareRenderer` -> framebuffer pixels. Nothing
//! is injected; the commands come only from the script.

use gdcore::math::{Color, Vector2};
use gdrender2d::renderer::{FrameBuffer, SoftwareRenderer};
use gdrender2d::test_adapter::capture_frame;
use gdscene::node::{Node, NodeId};
use gdscene::scene_bridge::build_canvas_viewport;
use gdscene::scene_tree::SceneTree;
use gdscene::{node2d, GDScriptNodeInstance, MainLoop};
use gdvariant::Variant;

const W: u32 = 200;
const H: u32 = 200;
/// The drawing node sits here, so local (0,0) maps to this pixel.
const ORIGIN: Vector2 = Vector2::new(100.0, 100.0);
const BG: Color = Color::new(0.0, 0.0, 0.0, 1.0);

/// The scene the bead specifies: a radius that grows every `_process`, redrawn
/// each frame as a red circle plus a green polyline.
///
/// `_process` runs before the draw phase, so after the first `step(dt)` the
/// radius already includes one increment.
const SCRIPT: &str = "\
extends Node2D
var r = 10.0
func _process(d):
    self.r = self.r + 60.0 * d
    queue_redraw()
func _draw():
    draw_circle(Vector2.ZERO, self.r, Color.RED)
    draw_polyline([Vector2(0, 0), Vector2(40, 0), Vector2(40, 40)], Color.GREEN, 3.0)
";

fn build() -> (MainLoop, NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let node = tree
        .add_child(root, Node::new("Painter", "Node2D"))
        .unwrap();
    node2d::set_position(&mut tree, node, ORIGIN);
    let script = GDScriptNodeInstance::from_source(SCRIPT, node).unwrap();
    tree.attach_script(node, Box::new(script));
    (MainLoop::new(tree), node)
}

/// Rasterizes the tree exactly as the Play path does: the canvas bridge builds
/// the viewport from the nodes' own command lists, and the software renderer
/// turns it into pixels.
fn render(ml: &MainLoop) -> FrameBuffer {
    let no_textures = |_p: &str| -> Option<Vector2> { None };
    let viewport = build_canvas_viewport(ml.tree(), W, H, BG, &no_textures);
    capture_frame(&mut SoftwareRenderer::new(), &viewport)
}

/// Pixel at `ORIGIN + offset`.
fn px(fb: &FrameBuffer, dx: f32, dy: f32) -> Color {
    fb.get_pixel((ORIGIN.x + dx) as u32, (ORIGIN.y + dy) as u32)
}

fn is_reddish(c: Color) -> bool {
    c.r > 0.5 && c.g < 0.4 && c.b < 0.4
}

fn is_greenish(c: Color) -> bool {
    c.g > 0.5 && c.r < 0.4 && c.b < 0.4
}

fn is_background(c: Color) -> bool {
    c.r < 0.1 && c.g < 0.1 && c.b < 0.1
}

/// The script's current radius, read back off its own instance.
fn radius(ml: &MainLoop, node: NodeId) -> f32 {
    match ml
        .tree()
        .get_script(node)
        .and_then(|s| {
            s.list_properties()
                .into_iter()
                .find(|p| p.name == "r")
                .map(|p| p.default_value)
        })
        .expect("script exposes `r`")
    {
        Variant::Float(f) => f as f32,
        Variant::Int(i) => i as f32,
        other => panic!("`r` should be numeric, got {other:?}"),
    }
}

/// **The acceptance test.** Red circle and green polyline pixels appear at the
/// node's global transform, and the circle grows on the next frame.
#[test]
fn gdscript_draw_api_renders_pixels() {
    let (mut ml, node) = build();

    // dt = 0.25 -> the radius steps 10 -> 25 -> 40, so "grew" is unambiguous at
    // the sample points rather than a sub-pixel difference.
    ml.step(0.25);
    assert!(
        (radius(&ml, node) - 25.0).abs() < 1e-3,
        "_process must have run before the draw phase; r = {}",
        radius(&ml, node),
    );

    let fb = render(&ml);

    // The circle is filled at its centre and 20px up — clear of the polyline,
    // which runs right and then down from the origin.
    assert!(
        is_reddish(px(&fb, 0.0, -20.0)),
        "a point inside the r=25 circle must be red, got {:?}",
        px(&fb, 0.0, -20.0),
    );

    // Beyond the current radius is still background — this is the pixel that
    // must change on the next frame.
    assert!(
        is_background(px(&fb, 0.0, -35.0)),
        "35px out is beyond the r=25 circle and must be background, got {:?}",
        px(&fb, 0.0, -35.0),
    );

    // The polyline: 35px along +X is on the first segment and outside the
    // circle. This is the assertion the old code could not satisfy at all —
    // `draw_polyline` reached no rasterizer.
    assert!(
        is_greenish(px(&fb, 35.0, 0.0)),
        "the polyline's first segment must be green at +35x, got {:?}",
        px(&fb, 35.0, 0.0),
    );
    // ...and its second segment runs down from (40, 0) to (40, 40).
    assert!(
        is_greenish(px(&fb, 40.0, 30.0)),
        "the polyline's second segment must be green at (+40, +30), got {:?}",
        px(&fb, 40.0, 30.0),
    );

    // Somewhere neither shape covers stays background, so "everything is
    // painted" cannot pass this test.
    assert!(
        is_background(px(&fb, -60.0, 60.0)),
        "an untouched corner must stay background, got {:?}",
        px(&fb, -60.0, 60.0),
    );

    // -- one more frame: queue_redraw re-ran _draw with the larger radius ----
    ml.step(0.25);
    assert!(
        (radius(&ml, node) - 40.0).abs() < 1e-3,
        "r should have grown again, got {}",
        radius(&ml, node),
    );

    let fb2 = render(&ml);
    assert!(
        is_reddish(px(&fb2, 0.0, -35.0)),
        "the pixel that was background at r=25 must be red at r=40 — this is \
         what proves queue_redraw re-ran _draw rather than the first frame's \
         commands being reused; got {:?}",
        px(&fb2, 0.0, -35.0),
    );
}

/// `draw_polygon` fills, rather than merely outlining — the other half of
/// `polygon_raster` that had no caller.
#[test]
fn draw_polygon_fills_its_interior() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let node = tree
        .add_child(root, Node::new("Painter", "Node2D"))
        .unwrap();
    node2d::set_position(&mut tree, node, ORIGIN);
    let src = "\
extends Node2D
func _ready():
    queue_redraw()
func _draw():
    draw_colored_polygon([Vector2(-30, -30), Vector2(30, -30), Vector2(30, 30), Vector2(-30, 30)], Color.RED)
";
    let script = GDScriptNodeInstance::from_source(src, node).unwrap();
    tree.attach_script(node, Box::new(script));
    let mut ml = MainLoop::new(tree);
    ml.step(1.0 / 60.0);

    let fb = render(&ml);
    // Interior, not just the edge.
    assert!(
        is_reddish(px(&fb, 0.0, 0.0)),
        "the polygon's centre must be filled, got {:?}",
        px(&fb, 0.0, 0.0),
    );
    assert!(
        is_reddish(px(&fb, 20.0, -20.0)),
        "a point well inside the polygon must be filled, got {:?}",
        px(&fb, 20.0, -20.0),
    );
    assert!(
        is_background(px(&fb, 45.0, 0.0)),
        "outside the polygon must stay background, got {:?}",
        px(&fb, 45.0, 0.0),
    );
}
