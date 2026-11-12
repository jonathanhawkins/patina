//! CanvasItem draw-order + inheritance rules (pat-u3l8c.29).
//!
//! Godot's 2D draw order depends on accumulated relative z-index, per-parent
//! Y-sort, and modulate that multiplies down the tree (with `self_modulate`
//! applied to the item only, not inherited). None of this was applied at
//! runtime (Y-sort was an editor overlay, `z_as_relative` stored but unused, no
//! modulate multiply). This module computes those values from scene nodes.

use gdcore::math::Color;
use gdvariant::Variant;

use crate::node::Node;
use crate::node::NodeId;
use crate::scene_tree::SceneTree;

fn prop_color(node: &Node, key: &str, default: Color) -> Color {
    match node.get_property(key) {
        Variant::Color(c) => c,
        _ => default,
    }
}

fn prop_i32(node: &Node, key: &str, default: i32) -> i32 {
    match node.get_property(key) {
        Variant::Int(i) => i as i32,
        _ => default,
    }
}

fn prop_bool(node: &Node, key: &str, default: bool) -> bool {
    match node.get_property(key) {
        Variant::Bool(b) => b,
        _ => default,
    }
}

fn color_mul(a: Color, b: Color) -> Color {
    Color::new(a.r * b.r, a.g * b.g, a.b * b.b, a.a * b.a)
}

/// Effective z-index of a node: when `z_as_relative` (Godot default), the node's
/// `z_index` adds to its parent's effective z; otherwise it is absolute.
pub fn effective_z_index(tree: &SceneTree, node_id: NodeId) -> i32 {
    let Some(node) = tree.get_node(node_id) else {
        return 0;
    };
    let z = prop_i32(node, "z_index", 0);
    if prop_bool(node, "z_as_relative", true) {
        if let Some(parent) = node.parent() {
            return effective_z_index(tree, parent) + z;
        }
    }
    z
}

/// Modulate a node contributes to its subtree: the product of `modulate` from
/// the root down to and including this node. This is what children inherit
/// (note: `self_modulate` is intentionally excluded).
pub fn inherited_modulate(tree: &SceneTree, node_id: NodeId) -> Color {
    let Some(node) = tree.get_node(node_id) else {
        return Color::WHITE;
    };
    let m = prop_color(node, "modulate", Color::WHITE);
    let parent = match node.parent() {
        Some(p) => inherited_modulate(tree, p),
        None => Color::WHITE,
    };
    color_mul(parent, m)
}

/// Final draw color multiplier for a node: its inherited modulate (which already
/// includes its own `modulate`) times its own `self_modulate`.
pub fn effective_modulate(tree: &SceneTree, node_id: NodeId) -> Color {
    let Some(node) = tree.get_node(node_id) else {
        return Color::WHITE;
    };
    let self_mod = prop_color(node, "self_modulate", Color::WHITE);
    color_mul(inherited_modulate(tree, node_id), self_mod)
}

/// Global Y of a node (sum of local Y up the parent chain), used for Y-sorting.
pub fn global_y(tree: &SceneTree, node_id: NodeId) -> f32 {
    // The composed transform, not a sum of local Ys: Godot Y-sorts on the item's
    // canvas-space origin, so a rotated or scaled ancestor changes a child's
    // screen Y. Summing `position.y` up the chain ignores both and mis-orders
    // exactly the scenes where sorting is visible (pat-0q2kx.39).
    crate::node2d::get_global_position(tree, node_id).y
}

/// Returns `parent_id`'s children in draw order. When the parent has
/// `y_sort_enabled`, siblings are sorted by ascending global Y (higher on the
/// screen draws first / behind); otherwise scene-tree (insertion) order is kept.
pub fn draw_ordered_children(tree: &SceneTree, parent_id: NodeId) -> Vec<NodeId> {
    // The parent's own child list, in order: the same nodes, in the same order,
    // that filtering a whole-tree walk by `parent()` yields — without walking
    // the whole tree once PER PARENT (pat-414k3). The HUD pass calls this for
    // every canvas node of a 3D game whose tree holds thousands of nodes, under
    // the editor-state lock, every Play frame.
    let mut children: Vec<NodeId> = tree
        .get_node(parent_id)
        .map(|p| {
            p.children()
                .iter()
                .copied()
                .filter(|&id| tree.get_node(id).and_then(|n| n.parent()) == Some(parent_id))
                .collect()
        })
        .unwrap_or_default();
    let y_sort = tree
        .get_node(parent_id)
        .map(|n| prop_bool(n, "y_sort_enabled", false))
        .unwrap_or(false);
    if y_sort {
        children.sort_by(|&a, &b| {
            global_y(tree, a)
                .partial_cmp(&global_y(tree, b))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
    }
    children
}

/// Whether `node_id` has `y_sort_enabled` set.
pub fn y_sort_enabled(tree: &SceneTree, node_id: NodeId) -> bool {
    tree.get_node(node_id)
        .map(|n| prop_bool(n, "y_sort_enabled", false))
        .unwrap_or(false)
}

/// The Y-sort GROUP of a `y_sort_enabled` parent: every descendant reachable
/// through non-y-sorted children, flattened into one list and sorted by global Y
/// (pat-s3rrv.22).
///
/// Godot does not sort a y-sorted node's direct children in isolation — it
/// collects the whole reachable subtree into a single list and sorts that
/// (`renderer_canvas_cull.cpp:110-181`), so a grandchild can interleave between
/// its own parent and that parent's siblings. Sorting only direct children makes
/// a child's entire subtree move as one block, which is visibly wrong the moment
/// a character's sprite should pass in front of a prop nested under something
/// else.
///
/// ⚠️ The walk STOPS at a child that is itself `y_sort_enabled`: that node is a
/// member of this group (it is drawn, and sorts by its own Y), but its subtree
/// belongs to *its* group, not this one. Descending into it would merge two
/// independent sorts into one and destroy the nesting Godot preserves.
pub fn y_sort_group(tree: &SceneTree, parent_id: NodeId) -> Vec<NodeId> {
    let mut group: Vec<NodeId> = Vec::new();
    collect_y_sort_group(tree, parent_id, &mut group);
    // Stable, so members sharing a Y keep scene-tree order — the same tiebreak
    // `draw_ordered_children` relies on.
    group.sort_by(|&a, &b| {
        global_y(tree, a)
            .partial_cmp(&global_y(tree, b))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    group
}

fn collect_y_sort_group(tree: &SceneTree, parent_id: NodeId, out: &mut Vec<NodeId>) {
    for child in direct_children(tree, parent_id) {
        out.push(child);
        if !y_sort_enabled(tree, child) {
            collect_y_sort_group(tree, child, out);
        }
    }
}

/// Scene-tree-order children of `parent_id`.
fn direct_children(tree: &SceneTree, parent_id: NodeId) -> Vec<NodeId> {
    tree.all_nodes_in_tree_order()
        .into_iter()
        .filter(|&id| tree.get_node(id).and_then(|n| n.parent()) == Some(parent_id))
        .collect()
}

/// Godot `clip_children` mode for a node (0 = disabled, 1 = clip only,
/// 2 = clip and draw). Consumers mask child draws to the parent when != 0.
pub fn clip_children_mode(tree: &SceneTree, node_id: NodeId) -> i32 {
    tree.get_node(node_id)
        .map(|n| prop_i32(n, "clip_children", 0))
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use gdcore::math::Vector2;
    use gdvariant::Variant;

    fn approx(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-4
    }

    #[test]
    fn relative_z_accumulates_and_absolute_does_not() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let mut a = Node::new("A", "Node2D");
        a.set_property("z_index", Variant::Int(5));
        let a_id = tree.add_child(root, a).unwrap();
        let mut b = Node::new("B", "Node2D");
        b.set_property("z_index", Variant::Int(3)); // relative default
        let b_id = tree.add_child(a_id, b).unwrap();
        let mut c = Node::new("C", "Node2D");
        c.set_property("z_index", Variant::Int(100));
        c.set_property("z_as_relative", Variant::Bool(false)); // absolute
        let c_id = tree.add_child(b_id, c).unwrap();

        assert_eq!(effective_z_index(&tree, a_id), 5);
        assert_eq!(effective_z_index(&tree, b_id), 8); // 5 + 3
        assert_eq!(effective_z_index(&tree, c_id), 100); // absolute
    }

    #[test]
    fn modulate_multiplies_down_but_self_modulate_stays_local() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let mut parent = Node::new("P", "Node2D");
        parent.set_property("modulate", Variant::Color(Color::new(0.5, 0.5, 0.5, 1.0)));
        parent.set_property(
            "self_modulate",
            Variant::Color(Color::new(0.5, 0.5, 0.5, 1.0)),
        );
        let p_id = tree.add_child(root, parent).unwrap();
        let mut child = Node::new("C", "Node2D");
        child.set_property("modulate", Variant::Color(Color::new(0.5, 0.5, 0.5, 1.0)));
        let c_id = tree.add_child(p_id, child).unwrap();

        // Child inherits parent.modulate * child.modulate = 0.25 (self_modulate excluded).
        let child_inh = inherited_modulate(&tree, c_id);
        assert!(
            approx(child_inh.r, 0.25),
            "child inherited modulate {}",
            child_inh.r
        );
        // Parent's own draw color includes its self_modulate: 0.5 * 0.5 = 0.25.
        let parent_eff = effective_modulate(&tree, p_id);
        assert!(
            approx(parent_eff.r, 0.25),
            "parent effective {}",
            parent_eff.r
        );
    }

    #[test]
    fn y_sort_orders_siblings_by_global_y() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let mut parent = Node::new("Sorter", "Node2D");
        parent.set_property("y_sort_enabled", Variant::Bool(true));
        let p_id = tree.add_child(root, parent).unwrap();

        // Add in a deliberately non-Y order.
        let mut lower = Node::new("Lower", "Node2D");
        lower.set_property("position", Variant::Vector2(Vector2::new(0.0, 100.0)));
        let lower_id = tree.add_child(p_id, lower).unwrap();
        let mut upper = Node::new("Upper", "Node2D");
        upper.set_property("position", Variant::Vector2(Vector2::new(0.0, 10.0)));
        let upper_id = tree.add_child(p_id, upper).unwrap();

        let order = draw_ordered_children(&tree, p_id);
        // Smaller global Y (upper) draws first.
        assert_eq!(order, vec![upper_id, lower_id]);
    }

    #[test]
    fn without_y_sort_tree_order_is_kept() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let p_id = tree.add_child(root, Node::new("P", "Node2D")).unwrap();
        let mut first = Node::new("First", "Node2D");
        first.set_property("position", Variant::Vector2(Vector2::new(0.0, 100.0)));
        let first_id = tree.add_child(p_id, first).unwrap();
        let mut second = Node::new("Second", "Node2D");
        second.set_property("position", Variant::Vector2(Vector2::new(0.0, 10.0)));
        let second_id = tree.add_child(p_id, second).unwrap();

        // No y_sort -> insertion order preserved despite Y values.
        assert_eq!(
            draw_ordered_children(&tree, p_id),
            vec![first_id, second_id]
        );
    }

    #[test]
    fn clip_children_mode_is_read() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let mut n = Node::new("Clipper", "Control");
        n.set_property("clip_children", Variant::Int(2));
        let id = tree.add_child(root, n).unwrap();
        assert_eq!(clip_children_mode(&tree, id), 2);
        assert_eq!(clip_children_mode(&tree, root), 0);
    }
}
