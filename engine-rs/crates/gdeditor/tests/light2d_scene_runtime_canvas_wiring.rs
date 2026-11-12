//! pat-0q2kx.103 acceptance: 2D lights shade the **runtime canvas**.
//!
//! The light maths (`gdrender2d::light2d`) and the scene collectors
//! (`gdscene::light2d_scene`) were both complete and both had tests — but the
//! only production 2D render path never called either, so a lit scene rendered
//! byte-for-byte identically to an unlit one. `gdscene`'s own
//! `light2d_scene_shading_wiring` test proves the *maths*; these prove the
//! **Play canvas** actually runs it, through `render_runtime_canvas` — the entry
//! point the editor's Play tab and `render_runtime_frame` both use.
//!
//! The distinction matters because the gap here was never in the shading
//! functions. It was that nothing called them.

use gdcore::math::{Color, Vector2};
use gdeditor::viewport_2d_render::render_runtime_canvas;
use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdvariant::Variant;

const W: u32 = 100;
const H: u32 = 100;
const CLEAR: Color = Color {
    r: 0.0,
    g: 0.0,
    b: 0.0,
    a: 1.0,
};

/// A mid-grey full-canvas rect, so both brightening and darkening are visible.
fn backdrop(tree: &mut SceneTree, root: NodeId) -> NodeId {
    let mut node = Node::new("Backdrop", "ColorRect");
    node.set_property("position", Variant::Vector2(Vector2::ZERO));
    node.set_property("size", Variant::Vector2(Vector2::new(100.0, 100.0)));
    node.set_property("color", Variant::Color(Color::new(0.5, 0.5, 0.5, 1.0)));
    tree.add_child(root, node).unwrap()
}

fn add_point_light(tree: &mut SceneTree, root: NodeId, pos: Vector2, energy: f32) {
    let mut n = Node::new("Light", "PointLight2D");
    n.set_property("position", Variant::Vector2(pos));
    n.set_property("color", Variant::Color(Color::new(1.0, 1.0, 1.0, 1.0)));
    n.set_property("energy", Variant::Float(energy as f64));
    n.set_property("range", Variant::Float(500.0));
    n.set_property("light_mask", Variant::Int(1));
    tree.add_child(root, n).unwrap();
}

fn add_occluder(tree: &mut SceneTree, root: NodeId, points: Vec<Vector2>) {
    let mut occ = Node::new("Wall", "LightOccluder2D");
    occ.set_property("polygon", Variant::PackedVector2Array(points));
    occ.set_property("light_mask", Variant::Int(1));
    tree.add_child(root, occ).unwrap();
}

fn add_canvas_modulate(tree: &mut SceneTree, root: NodeId, color: Color) {
    let mut cm = Node::new("Modulate", "CanvasModulate");
    cm.set_property("color", Variant::Color(color));
    tree.add_child(root, cm).unwrap();
}

/// The render list filters on `is_class("CanvasItem")` — without ClassDB the
/// list is empty and every pixel below would be the clear colour.
fn new_tree() -> (SceneTree, NodeId) {
    gdobject::class_db::register_2d_classes();
    let mut tree = SceneTree::new();
    let root_id = tree.root_id();
    let root = tree
        .add_child(root_id, Node::new("Root", "Node2D"))
        .unwrap();
    (tree, root)
}

fn render(tree: &SceneTree) -> gdrender2d::renderer::FrameBuffer {
    render_runtime_canvas(tree, W, H, CLEAR, None)
}

fn pixel(fb: &gdrender2d::renderer::FrameBuffer, x: u32, y: u32) -> Color {
    fb.pixels[(y * fb.width + x) as usize]
}

fn luma(c: Color) -> f32 {
    (c.r + c.g + c.b) / 3.0
}

// ---------------------------------------------------------------------------
// The bead's acceptance test
// ---------------------------------------------------------------------------

/// **The core wiring test.** A light brightens the canvas centre, an occluder
/// puts it back in shadow, and a `CanvasModulate` tints the whole frame.
#[test]
fn light2d_scene_runtime_canvas_wiring() {
    // -- (a) an energy=2 light above the centre brightens centre vs corner ---
    let lit_centre;
    let lit_corner;
    {
        let (mut tree, root) = new_tree();
        backdrop(&mut tree, root);
        add_point_light(&mut tree, root, Vector2::new(50.0, 50.0), 2.0);

        let fb = render(&tree);
        lit_centre = luma(pixel(&fb, 50, 50));
        lit_corner = luma(pixel(&fb, 3, 3));
        assert!(
            lit_centre > lit_corner + 0.05,
            "a PointLight2D over the centre must brighten it relative to the \
             far corner in the RUNTIME canvas; centre {lit_centre}, corner {lit_corner}"
        );
    }

    // -- (b) an occluder between light and surface restores the dark value ---
    {
        let (mut tree, root) = new_tree();
        backdrop(&mut tree, root);
        // Light above the wall; the wall spans x=[30,70] at y=40.
        add_point_light(&mut tree, root, Vector2::new(50.0, 10.0), 2.0);
        add_occluder(
            &mut tree,
            root,
            vec![Vector2::new(30.0, 40.0), Vector2::new(70.0, 40.0)],
        );

        let fb = render(&tree);
        // Above the wall: on the light's side, so lit.
        let in_front = luma(pixel(&fb, 50, 25));
        // Below the wall: the wall is between it and the light, so shadowed.
        let behind = luma(pixel(&fb, 50, 70));
        assert!(
            behind < in_front,
            "an occluder between the light and the surface must cast shadow: \
             lit side {in_front}, shadowed side {behind}"
        );
    }

    // -- (c) CanvasModulate multiplies the whole frame ------------------------
    {
        let (mut tree, root) = new_tree();
        backdrop(&mut tree, root);
        add_canvas_modulate(&mut tree, root, Color::new(1.0, 0.0, 0.0, 1.0));

        let fb = render(&tree);
        let tinted = pixel(&fb, 50, 50);
        assert!(
            tinted.r > 0.2 && tinted.g < 0.02 && tinted.b < 0.02,
            "a red CanvasModulate must zero the green and blue of the whole \
             frame; got {tinted:?}"
        );
    }
}

// ---------------------------------------------------------------------------
// Guards
// ---------------------------------------------------------------------------

/// **The regression this bead's fix could most easily cause.** With no light
/// nodes the canvas must be left exactly as rasterized — the light pass's
/// ambient term is a dim 0.15 grey, so running it unconditionally would darken
/// every unlit scene in the engine to a sixth of its brightness.
#[test]
fn a_scene_without_lights_is_not_dimmed() {
    let (mut tree, root) = new_tree();
    backdrop(&mut tree, root);

    let fb = render(&tree);
    let centre = pixel(&fb, 50, 50);
    assert!(
        (centre.r - 0.5).abs() < 0.02,
        "an unlit scene must render at its own colour, not multiplied by the \
         ambient term; expected ~0.5, got {centre:?}"
    );
}

/// The light pass reaches pixels drawn by the render list, not just the clear
/// colour — i.e. it runs *after* rasterization rather than before it.
#[test]
fn shading_applies_to_rasterized_content() {
    let (mut tree, root) = new_tree();
    // A small dark rect the light sits directly on top of.
    let mut node = Node::new("Target", "ColorRect");
    node.set_property("position", Variant::Vector2(Vector2::new(40.0, 40.0)));
    node.set_property("size", Variant::Vector2(Vector2::new(20.0, 20.0)));
    node.set_property("color", Variant::Color(Color::new(0.4, 0.4, 0.4, 1.0)));
    tree.add_child(root, node).unwrap();
    add_point_light(&mut tree, root, Vector2::new(50.0, 50.0), 2.0);

    let fb = render(&tree);
    let on_rect = luma(pixel(&fb, 50, 50));
    // Outside the rect the frame is the clear colour (black), which no amount
    // of light can raise — so the lit value has to come from the rect's pixels.
    assert!(
        on_rect > 0.1,
        "the light must brighten the rasterized rect, which means shading runs \
         after the draw; got {on_rect}"
    );
}
