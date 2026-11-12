//! pat-0q2kx.101 acceptance: effective z / modulate / Y-sort decide the actual
//! **pixels** of the runtime canvas.
//!
//! The wiring this bead describes was delivered by pat-0q2kx.39:
//! `collect_canvas_items` walks with `canvas_order::draw_ordered_children`,
//! and sets `effective_z_index` / `effective_modulate`. What that bead's tests
//! assert for relative-z and Y-sort, though, is the **order of the render
//! list** — and list order is one inference away from what a player sees. A
//! later item can still end up invisible: clipped, blended away, or drawn at a
//! transform that misses the overlap. Godot's guarantee is about the overlap
//! pixel, so that is what these assert.
//!
//! Each case overlaps two rects of different colours and reads the contested
//! pixel out of a rasterized frame, through the same
//! `build_runtime_render_list` + `rasterize_render_list` pair the Play tab
//! drives every frame.

use gdcore::math::{Color, Vector2};
use gdeditor::viewport_2d_render::{build_runtime_render_list, rasterize_render_list};
use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdvariant::Variant;

const W: u32 = 64;
const H: u32 = 64;
const CLEAR: Color = Color {
    r: 0.0,
    g: 0.0,
    b: 0.0,
    a: 1.0,
};

const RED: Color = Color {
    r: 1.0,
    g: 0.0,
    b: 0.0,
    a: 1.0,
};
const GREEN: Color = Color {
    r: 0.0,
    g: 1.0,
    b: 0.0,
    a: 1.0,
};
const WHITE: Color = Color {
    r: 1.0,
    g: 1.0,
    b: 1.0,
    a: 1.0,
};

fn color_rect(
    tree: &mut SceneTree,
    parent: NodeId,
    name: &str,
    pos: Vector2,
    size: Vector2,
    color: Color,
) -> NodeId {
    let mut node = Node::new(name, "ColorRect");
    node.set_property("position", Variant::Vector2(pos));
    node.set_property("size", Variant::Vector2(size));
    node.set_property("color", Variant::Color(color));
    tree.add_child(parent, node).unwrap()
}

fn set(tree: &mut SceneTree, id: NodeId, key: &str, value: Variant) {
    tree.get_node_mut(id).unwrap().set_property(key, value);
}

/// The render list filters on `is_class("CanvasItem")`, which reads the ClassDB —
/// without registration the list comes back empty and every assertion here would
/// pass vacuously.
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
    let items = build_runtime_render_list(tree);
    assert!(
        !items.is_empty(),
        "the render list is empty — every pixel assertion below would be vacuous"
    );
    rasterize_render_list(&items, W, H, CLEAR, None)
}

fn pixel(fb: &gdrender2d::renderer::FrameBuffer, x: u32, y: u32) -> Color {
    fb.pixels[(y * fb.width + x) as usize]
}

/// Colour comparison with a tolerance, since rasterization blends.
fn approx(a: Color, b: Color) -> bool {
    (a.r - b.r).abs() < 0.02 && (a.g - b.g).abs() < 0.02 && (a.b - b.b).abs() < 0.02
}

// ---------------------------------------------------------------------------
// The bead's acceptance test
// ---------------------------------------------------------------------------

/// **The acceptance test.** All three canvas-order rules decided at the pixel.
#[test]
fn canvas_order_effective_z_modulate_ysort_runtime() {
    // -- (a) accumulated z: child(z=1) of parent(z=2) beats an absolute z=2 ---
    //
    // The child's own z is 1, but `z_as_relative` defaults true, so it
    // accumulates to 3 and must overdraw the sibling sitting at a flat 2. Both
    // rects cover the same pixel, so whichever draws last owns it.
    {
        let (mut tree, root) = new_tree();
        let parent = color_rect(
            &mut tree,
            root,
            "Parent",
            Vector2::ZERO,
            // A zero-alpha parent so only the child's colour lands on the pixel.
            Vector2::new(1.0, 1.0),
            Color::new(0.0, 0.0, 0.0, 0.0),
        );
        set(&mut tree, parent, "z_index", Variant::Int(2));
        let child = color_rect(
            &mut tree,
            parent,
            "AccumulatedChild",
            Vector2::new(10.0, 10.0),
            Vector2::new(20.0, 20.0),
            RED,
        );
        set(&mut tree, child, "z_index", Variant::Int(1));

        // Sibling at an absolute z of 2, added AFTER so tree order favours it —
        // only the z accumulation can put the child on top.
        let sibling = color_rect(
            &mut tree,
            root,
            "FlatSibling",
            Vector2::new(10.0, 10.0),
            Vector2::new(20.0, 20.0),
            GREEN,
        );
        set(&mut tree, sibling, "z_index", Variant::Int(2));

        let fb = render(&tree);
        let contested = pixel(&fb, 20, 20);
        assert!(
            approx(contested, RED),
            "child z=1 under parent z=2 accumulates to 3 and must own the \
             overlap pixel against a flat z=2 sibling; got {contested:?}"
        );
    }

    // -- (b) inherited modulate tints the child ------------------------------
    {
        let (mut tree, root) = new_tree();
        let parent = color_rect(
            &mut tree,
            root,
            "Parent",
            Vector2::ZERO,
            Vector2::new(1.0, 1.0),
            Color::new(0.0, 0.0, 0.0, 0.0),
        );
        set(&mut tree, parent, "modulate", Variant::Color(RED));
        color_rect(
            &mut tree,
            parent,
            "WhiteChild",
            Vector2::new(10.0, 10.0),
            Vector2::new(20.0, 20.0),
            WHITE,
        );

        let fb = render(&tree);
        let tinted = pixel(&fb, 20, 20);
        assert!(
            approx(tinted, RED),
            "a parent modulate of pure red must tint its white child red — the \
             child's own colour is unchanged, so anything else means modulate \
             did not propagate; got {tinted:?}"
        );
    }

    // -- (c) Y-sort orders siblings by global Y, not tree order --------------
    //
    // The HIGHER-Y rect is added FIRST, so insertion order and Y order
    // disagree: without Y-sort the later (lower-Y) node would win the pixel.
    {
        let (mut tree, root) = new_tree();
        let sorter = color_rect(
            &mut tree,
            root,
            "Sorter",
            Vector2::ZERO,
            Vector2::new(1.0, 1.0),
            Color::new(0.0, 0.0, 0.0, 0.0),
        );
        set(&mut tree, sorter, "y_sort_enabled", Variant::Bool(true));

        // Higher Y ⇒ nearer the viewer ⇒ draws last. Added first.
        color_rect(
            &mut tree,
            sorter,
            "LowOnScreen",
            Vector2::new(10.0, 24.0),
            Vector2::new(20.0, 12.0),
            RED,
        );
        // Lower Y ⇒ further away ⇒ draws first. Added last.
        color_rect(
            &mut tree,
            sorter,
            "HighOnScreen",
            Vector2::new(10.0, 12.0),
            Vector2::new(20.0, 20.0),
            GREEN,
        );

        let fb = render(&tree);
        // y = 28 lies inside both rects: red spans [24, 36), green spans [12, 32).
        let contested = pixel(&fb, 20, 28);
        assert!(
            approx(contested, RED),
            "the higher-global-Y sibling must draw last and own the overlap \
             pixel even though it was added to the tree first; got {contested:?}"
        );
    }
}

// ---------------------------------------------------------------------------
// Guard
// ---------------------------------------------------------------------------

/// Without Y-sort the same scene resolves the other way. This is the control
/// for case (c): it proves the ordering there came from `y_sort_enabled` and
/// not from the geometry happening to favour one rect.
#[test]
fn without_y_sort_tree_order_decides_the_pixel() {
    let (mut tree, root) = new_tree();
    let plain = color_rect(
        &mut tree,
        root,
        "Plain",
        Vector2::ZERO,
        Vector2::new(1.0, 1.0),
        Color::new(0.0, 0.0, 0.0, 0.0),
    );
    // Same two rects, same order, but no y_sort_enabled.
    color_rect(
        &mut tree,
        plain,
        "LowOnScreen",
        Vector2::new(10.0, 24.0),
        Vector2::new(20.0, 12.0),
        RED,
    );
    color_rect(
        &mut tree,
        plain,
        "HighOnScreen",
        Vector2::new(10.0, 12.0),
        Vector2::new(20.0, 20.0),
        GREEN,
    );

    let fb = render(&tree);
    let contested = pixel(&fb, 20, 28);
    assert!(
        approx(contested, GREEN),
        "with Y-sort off, the later sibling in tree order owns the pixel — if \
         this is red too, case (c) proves nothing; got {contested:?}"
    );
}
