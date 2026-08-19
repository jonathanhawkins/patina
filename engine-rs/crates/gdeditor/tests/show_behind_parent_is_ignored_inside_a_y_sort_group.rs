//! pat-s3rrv.22 — `show_behind_parent` must not reorder a Y-SORTED group.
//!
//! Godot has two distinct branches in `_cull_canvas_item`
//! (upstream/godot/servers/rendering/renderer_canvas_cull.cpp): the y-sort
//! branch flattens the subtree, sorts it with `ItemYSort`, and draws it in that
//! order **without ever reading `behind`**; the `behind` reads (`:479`, `:486`)
//! are in the other branch, after the y-sort branch has closed. So inside a
//! y-sorted group, position is the only thing that orders items.
//!
//! Patina partitioned the FLATTENED group by the flag, which hoisted a flagged
//! descendant ahead of items Godot draws first purely by Y.
//!
//! ⚠️ The flagged child here sits at the HIGHEST Y on purpose. Y-order puts it
//! last, the old partition put it first — so the assertion separates the two
//! implementations. A flagged child at a low Y would come first either way and
//! prove nothing.

use gdcore::math::Vector2;
use gdeditor::viewport_2d_render::build_runtime_render_list;
use gdscene::node::Node;
use gdscene::{NodeId, SceneTree};
use gdvariant::Variant;

/// Adds a drawable Sprite2D. A texture path is what admits it to the render
/// list; without one it falls through to a marker.
fn sprite(tree: &mut SceneTree, parent: NodeId, name: &str, y: f32, behind: bool) -> NodeId {
    let id = tree.add_child(parent, Node::new(name, "Sprite2D")).unwrap();
    let node = tree.get_node_mut(id).unwrap();
    node.set_property("position", Variant::Vector2(Vector2::new(0.0, y)));
    node.set_property("texture", Variant::String("res://missing.png".into()));
    if behind {
        node.set_property("show_behind_parent", Variant::Bool(true));
    }
    id
}

/// Index of a node in the draw list.
fn index_of(items: &[gdeditor::viewport_2d_render::Render2DItem], id: NodeId) -> usize {
    items
        .iter()
        .position(|i| i.node_id == id.raw())
        .unwrap_or_else(|| panic!("node {id:?} is missing from the draw list"))
}

#[test]
fn show_behind_parent_is_ignored_inside_a_y_sort_group() {
    gdobject::class_db::register_2d_classes();
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let sorter = sprite(&mut tree, root, "Sorter", 50.0, false);
    tree.get_node_mut(sorter)
        .unwrap()
        .set_property("y_sort_enabled", Variant::Bool(true));

    let low = sprite(&mut tree, sorter, "Low", 10.0, false);
    let high = sprite(&mut tree, sorter, "High", 100.0, true);

    let items = build_runtime_render_list(&tree);
    let (i_low, i_high) = (index_of(&items, low), index_of(&items, high));

    assert!(
        i_low < i_high,
        "inside a y-sorted group the order is by Y alone: Low (y=10) must draw \
         before High (y=100) even though High sets show_behind_parent. \
         Got Low at {i_low}, High at {i_high} — High first means the flag is \
         still partitioning the flattened group."
    );
}

/// Control: OUTSIDE a y-sorted group the flag keeps its meaning, so the fix is
/// a narrowing rather than a removal. Without this, deleting the partition
/// entirely would also pass the test above.
#[test]
fn show_behind_parent_still_orders_an_unsorted_parent() {
    gdobject::class_db::register_2d_classes();
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let parent = sprite(&mut tree, root, "Parent", 50.0, false);
    let child = sprite(&mut tree, parent, "Child", 100.0, true);

    let items = build_runtime_render_list(&tree);
    let (i_parent, i_child) = (index_of(&items, parent), index_of(&items, child));

    assert!(
        i_child < i_parent,
        "with no y-sort, a show_behind_parent child draws BEFORE its parent; \
         got child at {i_child}, parent at {i_parent}"
    );
}
