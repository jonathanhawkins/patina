//! pat-7jjlb: smart/relative snapping to sibling edges & centers with snap-line
//! feedback.
//!
//! Drives the public `compute_smart_snap` API: dragging a node near a sibling's
//! edge/center aligns it to that sibling and emits a `SnapGuide` at the matching
//! coordinate. A drag that lands outside the threshold neither moves nor emits a
//! guide.

use gdcore::math::Vector2;
use gdeditor::editor_server::{compute_smart_snap, SnapGuide};
use gdscene::node::Node;
use gdscene::SceneTree;
use gdvariant::Variant;

/// Adds a child of `parent` at `pos` with size `size`, returning its id.
fn add_box(
    tree: &mut SceneTree,
    parent: gdscene::node::NodeId,
    name: &str,
    pos: Vector2,
    size: Vector2,
) -> gdscene::node::NodeId {
    let id = tree.add_child(parent, Node::new(name, "Node2D")).unwrap();
    let node = tree.get_node_mut(id).unwrap();
    node.set_property("position", Variant::Vector2(pos));
    node.set_property("size", Variant::Vector2(size));
    id
}

#[test]
fn viewport_smart_snap_aligns_to_siblings() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    // A stationary sibling and the node we'll drag, both children of root.
    let sibling = add_box(
        &mut tree,
        root,
        "Static",
        Vector2::new(200.0, 150.0),
        Vector2::new(40.0, 20.0),
    );
    let dragged = add_box(
        &mut tree,
        root,
        "Dragged",
        Vector2::new(0.0, 0.0),
        Vector2::new(40.0, 20.0),
    );

    let threshold = 5.0;

    // Dragging to just shy of the sibling's center snaps onto it and shows
    // guide lines at the matched x and y.
    let candidate = Vector2::new(198.0, 153.0);
    let (snapped, guides) = compute_smart_snap(&tree, dragged, candidate, threshold);

    assert_eq!(
        snapped,
        Vector2::new(200.0, 150.0),
        "the node snaps to align with the sibling"
    );
    assert!(
        guides.iter().any(|g: &SnapGuide| g.axis == "x"
            && (g.position - 200.0).abs() < 1e-5
            && g.target_node_id == sibling),
        "a vertical snap guide is shown at the matched x, pointing at the sibling"
    );
    assert!(
        guides.iter().any(|g: &SnapGuide| g.axis == "y"
            && (g.position - 150.0).abs() < 1e-5
            && g.target_node_id == sibling),
        "a horizontal snap guide is shown at the matched y, pointing at the sibling"
    );

    // Snapping also aligns edges, not just centers: place the candidate so the
    // dragged node's left edge lines up with the sibling's left edge.
    // sibling left edge = 200 - 40/2 = 180; dragged center for that = 180 + 40/2 = 200.
    // (Centers coincide here, so use a wider sibling to separate edge from center.)
    let wide = add_box(
        &mut tree,
        root,
        "Wide",
        Vector2::new(400.0, 300.0),
        Vector2::new(100.0, 100.0),
    );
    // Wide's left edge = 400 - 50 = 350; dragged (size 40) aligns its left edge
    // there when its center x = 350 + 20 = 370.
    let edge_candidate = Vector2::new(368.0, 300.0);
    let (edge_snapped, edge_guides) =
        compute_smart_snap(&tree, dragged, edge_candidate, threshold);
    assert!(
        (edge_snapped.x - 370.0).abs() < 1e-5,
        "the node's left edge snaps to the sibling's left edge (x={})",
        edge_snapped.x
    );
    assert!(
        edge_guides.iter().any(|g| g.axis == "x" && g.target_node_id == wide),
        "an edge-alignment guide is shown for the wide sibling"
    );

    // A drag outside the threshold neither moves the node nor shows a guide.
    let far = Vector2::new(50.0, 50.0);
    let (far_snapped, far_guides) = compute_smart_snap(&tree, dragged, far, threshold);
    assert_eq!(far_snapped, far, "far drags are left exactly where they are");
    assert!(far_guides.is_empty(), "no guide lines when nothing is within range");
}
