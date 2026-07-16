//! pat-nzgdu.117 — a scene with a `PointLight2D` + `LightOccluder2D` must shade
//! the rendered framebuffer (a lit region and a darker shadowed region behind
//! the occluder), and a `CanvasModulate` must tint the whole output.
//!
//! The lighting math + scene collectors already existed
//! (`collect_lights_2d`/`collect_occluders_2d`/`apply_scene_lights`); this bead
//! adds `CanvasModulate` (collect + whole-canvas multiply) and proves the
//! end-to-end wiring from scene nodes to framebuffer pixels.

use gdcore::math::{Color, Vector2};
use gdrender2d::light2d::CanvasAmbient;
use gdrender2d::renderer::FrameBuffer;
use gdscene::light2d_scene::{apply_canvas_modulate, apply_scene_lights, collect_canvas_modulate};
use gdscene::node::Node;
use gdscene::scene_tree::SceneTree;
use gdvariant::Variant;

fn add_point_light(tree: &mut SceneTree, pos: Vector2, energy: f32, range: f32) {
    let root = tree.root_id();
    let mut n = Node::new("Light", "PointLight2D");
    n.set_property("position", Variant::Vector2(pos));
    n.set_property("color", Variant::Color(Color::new(1.0, 1.0, 1.0, 1.0)));
    n.set_property("energy", Variant::Float(energy as f64));
    n.set_property("range", Variant::Float(range as f64));
    n.set_property("light_mask", Variant::Int(1));
    tree.add_child(root, n).unwrap();
}

fn add_occluder(tree: &mut SceneTree, points: Vec<Vector2>) {
    let root = tree.root_id();
    let mut occ = Node::new("Wall", "LightOccluder2D");
    occ.set_property("polygon", Variant::PackedVector2Array(points));
    occ.set_property("light_mask", Variant::Int(1));
    tree.add_child(root, occ).unwrap();
}

fn add_canvas_modulate(tree: &mut SceneTree, color: Color) {
    let root = tree.root_id();
    let mut cm = Node::new("Modulate", "CanvasModulate");
    cm.set_property("color", Variant::Color(color));
    tree.add_child(root, cm).unwrap();
}

#[test]
fn light2d_scene_shading_wiring() {
    let mut tree = SceneTree::new();
    // Light above; a horizontal occluder wall at y=50 across x=[40,60]; a red
    // CanvasModulate tinting the whole canvas.
    add_point_light(&mut tree, Vector2::new(50.0, 10.0), 2.0, 500.0);
    add_occluder(
        &mut tree,
        vec![Vector2::new(40.0, 50.0), Vector2::new(60.0, 50.0)],
    );
    add_canvas_modulate(&mut tree, Color::new(1.0, 0.0, 0.0, 1.0));

    // White base canvas so the lighting shows through.
    let mut fb = FrameBuffer::new(100, 100, Color::new(1.0, 1.0, 1.0, 1.0));
    apply_scene_lights(&tree, &mut fb, CanvasAmbient::default(), 1);

    // (a) Lit vs shadowed: (50,30) is between the light and the wall → lit;
    // (50,80) is behind the wall → shadowed. Measured before the tint.
    let lit = fb.get_pixel(50, 30);
    let shadowed = fb.get_pixel(50, 80);
    assert!(
        lit.r > shadowed.r + 0.05,
        "lit region must be brighter than the occluder's shadow (lit {} vs shadow {})",
        lit.r,
        shadowed.r
    );
    // Before the tint the lit pixel still carries green/blue (white × light).
    assert!(lit.g > 0.05 && lit.b > 0.05, "pre-tint lit pixel keeps g/b, got {lit:?}");

    // (b) CanvasModulate red is collected and multiplies the whole canvas,
    // zeroing green/blue while preserving the (lit) red channel.
    let modulate = collect_canvas_modulate(&tree).expect("CanvasModulate color collected");
    assert!(modulate.r > 0.9 && modulate.g < 0.1 && modulate.b < 0.1, "collected red modulate");
    apply_canvas_modulate(&mut fb, modulate);

    let tinted = fb.get_pixel(50, 30);
    assert!(
        tinted.g < 0.02 && tinted.b < 0.02,
        "red CanvasModulate must zero green/blue across the output, got {tinted:?}"
    );
    assert!(
        tinted.r > 0.1,
        "red channel must survive the tint in the lit region, got {tinted:?}"
    );
}
