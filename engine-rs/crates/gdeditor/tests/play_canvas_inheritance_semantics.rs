//! pat-0q2kx.39 acceptance: the Play render path honours Godot's CanvasItem
//! inheritance semantics, not just each node's own properties.
//!
//! `gdscene::canvas_order` had the math (effective modulate, relative z, Y-sort,
//! clip mode) with its own unit tests, but the only runtime render path —
//! `collect_canvas_items`, which `examples/editor.rs` drives every frame for the
//! Play tab — ignored all of it: own `modulate` only, raw `z_index`, tree-order
//! walk with no Y-sort, and no clipping. The editor inspector exposed every one of
//! those properties, so they read as done while doing nothing.
//!
//! These drive the real Play entry points (`build_runtime_render_list` +
//! `rasterize_render_list`) and assert on the produced frame or the item order.

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

/// A `ColorRect` child: `position`/`size`/`color` are plain properties, which is
/// what `render_kind_for` and `get_global_transform` read.
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
/// without registration the list comes back empty and every assertion below would
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

fn pixel(fb: &gdrender2d::renderer::FrameBuffer, x: u32, y: u32) -> Color {
    fb.pixels[(y * fb.width + x) as usize]
}

fn index_of(items: &[gdeditor::viewport_2d_render::Render2DItem], id: NodeId) -> usize {
    items
        .iter()
        .position(|i| i.node_id == id.raw())
        .unwrap_or_else(|| panic!("node {id:?} is missing from the render list"))
}

/// **The acceptance test.** A parent's `modulate` tints its children, and a
/// `clip_children` parent confines an oversized child to its own rect.
#[test]
fn play_canvas_inheritance_semantics() {
    // -- (a) modulate propagates down the chain ---------------------------------
    //
    // Rendered twice: once with a half-alpha parent, once with an opaque one. The
    // child itself is full white and unchanged, so any difference is inherited.
    let tinted = {
        let (mut tree, root) = new_tree();
        let parent = color_rect(
            &mut tree,
            root,
            "Parent",
            Vector2::ZERO,
            Vector2::new(1.0, 1.0),
            Color::new(0.0, 0.0, 0.0, 0.0),
        );
        set(
            &mut tree,
            parent,
            "modulate",
            Variant::Color(Color::new(1.0, 1.0, 1.0, 0.5)),
        );
        color_rect(
            &mut tree,
            parent,
            "Child",
            Vector2::new(20.0, 20.0),
            Vector2::new(10.0, 10.0),
            Color::new(1.0, 1.0, 1.0, 1.0),
        );
        let items = build_runtime_render_list(&tree);
        rasterize_render_list(&items, W, H, CLEAR, None)
    };
    let opaque = {
        let (mut tree, root) = new_tree();
        let parent = color_rect(
            &mut tree,
            root,
            "Parent",
            Vector2::ZERO,
            Vector2::new(1.0, 1.0),
            Color::new(0.0, 0.0, 0.0, 0.0),
        );
        color_rect(
            &mut tree,
            parent,
            "Child",
            Vector2::new(20.0, 20.0),
            Vector2::new(10.0, 10.0),
            Color::new(1.0, 1.0, 1.0, 1.0),
        );
        let items = build_runtime_render_list(&tree);
        rasterize_render_list(&items, W, H, CLEAR, None)
    };

    let (tinted_px, opaque_px) = (pixel(&tinted, 25, 25), pixel(&opaque, 25, 25));
    assert!(
        opaque_px.r > 0.9,
        "control: an unmodulated white child must draw white, got {opaque_px:?}",
    );
    assert!(
        tinted_px.r < opaque_px.r - 0.1,
        "the parent's half-alpha modulate must tint the child: tinted {tinted_px:?} \
         vs unmodulated {opaque_px:?}",
    );

    // -- (d) clip_children confines an oversized child --------------------------
    let clipped = {
        let (mut tree, root) = new_tree();
        let clipper = color_rect(
            &mut tree,
            root,
            "Clipper",
            Vector2::ZERO,
            Vector2::new(20.0, 20.0),
            Color::new(0.0, 0.0, 0.0, 0.0),
        );
        // 2 = CLIP_CHILDREN_AND_DRAW.
        set(&mut tree, clipper, "clip_children", Variant::Int(2));
        color_rect(
            &mut tree,
            clipper,
            "Oversized",
            Vector2::ZERO,
            Vector2::new(60.0, 60.0),
            Color::new(1.0, 0.0, 0.0, 1.0),
        );
        let items = build_runtime_render_list(&tree);
        rasterize_render_list(&items, W, H, CLEAR, None)
    };
    let inside = pixel(&clipped, 10, 10);
    let outside = pixel(&clipped, 40, 40);
    assert!(
        inside.r > 0.9,
        "inside the clip rect the child must draw, got {inside:?}",
    );
    assert_eq!(
        outside, CLEAR,
        "outside the clip parent's 20x20 rect the child must be clipped away, got {outside:?}",
    );
}

/// `z_as_relative` (Godot's default) accumulates the parent's `z_index`, so a
/// relative child of a z=10 parent draws above an absolute z=5 sibling — the
/// opposite of what the raw property alone would give.
#[test]
fn relative_z_accumulates_above_an_absolute_sibling() {
    let (mut tree, root) = new_tree();
    let deep_parent = color_rect(
        &mut tree,
        root,
        "DeepParent",
        Vector2::ZERO,
        Vector2::new(1.0, 1.0),
        Color::new(0.0, 0.0, 0.0, 0.0),
    );
    set(&mut tree, deep_parent, "z_index", Variant::Int(10));
    // Own z_index 0, but relative to a z=10 parent → effective 10.
    let relative_child = color_rect(
        &mut tree,
        deep_parent,
        "RelativeChild",
        Vector2::ZERO,
        Vector2::new(5.0, 5.0),
        Color::new(1.0, 0.0, 0.0, 1.0),
    );
    // Absolute z=5: ignores its ancestors entirely.
    let absolute_sibling = color_rect(
        &mut tree,
        root,
        "AbsoluteSibling",
        Vector2::ZERO,
        Vector2::new(5.0, 5.0),
        Color::new(0.0, 1.0, 0.0, 1.0),
    );
    set(&mut tree, absolute_sibling, "z_index", Variant::Int(5));
    set(
        &mut tree,
        absolute_sibling,
        "z_as_relative",
        Variant::Bool(false),
    );

    let items = build_runtime_render_list(&tree);
    assert!(
        index_of(&items, relative_child) > index_of(&items, absolute_sibling),
        "the relative child of a z=10 parent must draw after (above) the z=5 sibling",
    );
}

/// A `y_sort_enabled` parent orders children by global Y, not insertion order.
/// The lower-Y child is added **last** here, so tree order and Y order disagree.
#[test]
fn y_sort_orders_by_global_y_not_tree_order() {
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

    // Added first, but further down the screen: must draw last (in front).
    let far = color_rect(
        &mut tree,
        sorter,
        "Far",
        Vector2::new(0.0, 40.0),
        Vector2::new(5.0, 5.0),
        Color::new(1.0, 0.0, 0.0, 1.0),
    );
    // Added last, but higher up: must draw first (behind).
    let near = color_rect(
        &mut tree,
        sorter,
        "Near",
        Vector2::new(0.0, 5.0),
        Vector2::new(5.0, 5.0),
        Color::new(0.0, 1.0, 0.0, 1.0),
    );

    let items = build_runtime_render_list(&tree);
    assert!(
        index_of(&items, near) < index_of(&items, far),
        "y_sort must draw the smaller-global-Y child first, regardless of the \
         order the children were added",
    );

    // Without y_sort the same tree keeps insertion order, so the assertion above
    // is really testing the sort and not the tree.
    set(&mut tree, sorter, "y_sort_enabled", Variant::Bool(false));
    let unsorted = build_runtime_render_list(&tree);
    assert!(
        index_of(&unsorted, far) < index_of(&unsorted, near),
        "with y_sort off, insertion order must be preserved",
    );
}
