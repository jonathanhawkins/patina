//! 2D node property helpers for Node2D, CanvasItem, Sprite2D, and Camera2D.
//!
//! In Patina (like Godot), all nodes use the same [`Node`](crate::node::Node)
//! struct. The `class_name` and stored properties determine behavior. This
//! module provides typed helper functions that read and write well-known
//! properties on nodes, compose local transforms from position/rotation/scale,
//! and compute global transforms by walking the parent chain.

use gdcore::math::{Color, Transform2D, Vector2};
use gdvariant::Variant;

use crate::node::NodeId;
use crate::scene_tree::SceneTree;

// ===========================================================================
// Node2D properties
// ===========================================================================

/// Sets the `"position"` property on a node.
pub fn set_position(tree: &mut SceneTree, node_id: NodeId, pos: Vector2) {
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property("position", Variant::Vector2(pos));
    }
}

/// Reads the `"position"` property, defaulting to [`Vector2::ZERO`].
pub fn get_position(tree: &SceneTree, node_id: NodeId) -> Vector2 {
    tree.get_node(node_id)
        .map(|n| match n.get_property("position") {
            Variant::Vector2(v) => v,
            _ => Vector2::ZERO,
        })
        .unwrap_or(Vector2::ZERO)
}

/// Sets the `"rotation"` property (radians) on a node.
pub fn set_rotation(tree: &mut SceneTree, node_id: NodeId, radians: f32) {
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property("rotation", Variant::Float(radians as f64));
    }
}

/// Reads the `"rotation"` property in radians, defaulting to `0.0`.
pub fn get_rotation(tree: &SceneTree, node_id: NodeId) -> f32 {
    tree.get_node(node_id)
        .map(|n| match n.get_property("rotation") {
            Variant::Float(f) => f as f32,
            _ => 0.0,
        })
        .unwrap_or(0.0)
}

/// Sets the `"scale"` property on a node.
pub fn set_scale(tree: &mut SceneTree, node_id: NodeId, scale: Vector2) {
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property("scale", Variant::Vector2(scale));
    }
}

/// Reads the `"scale"` property, defaulting to [`Vector2::ONE`].
pub fn get_scale(tree: &SceneTree, node_id: NodeId) -> Vector2 {
    tree.get_node(node_id)
        .map(|n| match n.get_property("scale") {
            Variant::Vector2(v) => v,
            _ => Vector2::ONE,
        })
        .unwrap_or(Vector2::ONE)
}

/// Composes a local [`Transform2D`] from the node's position, rotation, and scale.
///
/// The composition order matches Godot: translate * rotate * scale.
pub fn get_local_transform(tree: &SceneTree, node_id: NodeId) -> Transform2D {
    let pos = get_position(tree, node_id);
    let rot = get_rotation(tree, node_id);
    let scl = get_scale(tree, node_id);

    Transform2D::translated(pos)
        * Transform2D::rotated(rot)
        * Transform2D::scaled(scl)
}

/// Computes the global transform by walking the parent chain and multiplying
/// local transforms from the root downward.
pub fn get_global_transform(tree: &SceneTree, node_id: NodeId) -> Transform2D {
    // Collect ancestor chain (node -> parent -> ... -> root).
    let mut chain = Vec::new();
    let mut current = node_id;
    loop {
        chain.push(current);
        match tree.get_node(current).and_then(|n| n.parent()) {
            Some(parent_id) => current = parent_id,
            None => break,
        }
    }
    // Multiply from root (last) down to the node (first).
    chain.reverse();
    let mut global = Transform2D::IDENTITY;
    for id in chain {
        global = global * get_local_transform(tree, id);
    }
    global
}

/// Sets the node's local position such that its global position equals `pos`.
///
/// This computes the inverse of the parent's global transform and applies it
/// to the desired global position.
pub fn set_global_position(tree: &mut SceneTree, node_id: NodeId, pos: Vector2) {
    let parent_global = tree
        .get_node(node_id)
        .and_then(|n| n.parent())
        .map(|pid| get_global_transform(tree, pid))
        .unwrap_or(Transform2D::IDENTITY);

    let local_pos = parent_global.affine_inverse().xform(pos);
    set_position(tree, node_id, local_pos);
}

// ===========================================================================
// CanvasItem properties
// ===========================================================================

/// Sets the `"visible"` property on a node.
pub fn set_visible(tree: &mut SceneTree, node_id: NodeId, visible: bool) {
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property("visible", Variant::Bool(visible));
    }
}

/// Reads the `"visible"` property, defaulting to `true`.
pub fn is_visible(tree: &SceneTree, node_id: NodeId) -> bool {
    tree.get_node(node_id)
        .map(|n| match n.get_property("visible") {
            Variant::Bool(b) => b,
            // Default is true (Godot default: nodes are visible).
            _ => true,
        })
        .unwrap_or(true)
}

/// Returns `true` only if this node and every ancestor are visible.
pub fn is_visible_in_tree(tree: &SceneTree, node_id: NodeId) -> bool {
    let mut current = node_id;
    loop {
        if !is_visible(tree, current) {
            return false;
        }
        match tree.get_node(current).and_then(|n| n.parent()) {
            Some(parent_id) => current = parent_id,
            None => return true,
        }
    }
}

/// Sets the `"z_index"` property on a node.
pub fn set_z_index(tree: &mut SceneTree, node_id: NodeId, z: i64) {
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property("z_index", Variant::Int(z));
    }
}

/// Reads the `"z_index"` property, defaulting to `0`.
pub fn get_z_index(tree: &SceneTree, node_id: NodeId) -> i64 {
    tree.get_node(node_id)
        .map(|n| match n.get_property("z_index") {
            Variant::Int(i) => i,
            _ => 0,
        })
        .unwrap_or(0)
}

/// Sets the `"modulate"` color property on a node.
pub fn set_modulate(tree: &mut SceneTree, node_id: NodeId, color: Color) {
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property("modulate", Variant::Color(color));
    }
}

// ===========================================================================
// Sprite2D properties
// ===========================================================================

/// Sets the `"texture"` property to a resource path string.
pub fn set_texture_path(tree: &mut SceneTree, node_id: NodeId, path: &str) {
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property("texture", Variant::String(path.to_owned()));
    }
}

/// Reads the `"texture"` property as a path string, if present.
pub fn get_texture_path(tree: &SceneTree, node_id: NodeId) -> Option<String> {
    tree.get_node(node_id).and_then(|n| match n.get_property("texture") {
        Variant::String(s) => Some(s),
        _ => None,
    })
}

/// Sets the `"offset"` property on a Sprite2D node.
pub fn set_offset(tree: &mut SceneTree, node_id: NodeId, offset: Vector2) {
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property("offset", Variant::Vector2(offset));
    }
}

/// Sets the `"flip_h"` property on a Sprite2D node.
pub fn set_flip_h(tree: &mut SceneTree, node_id: NodeId, flip: bool) {
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property("flip_h", Variant::Bool(flip));
    }
}

/// Sets the `"flip_v"` property on a Sprite2D node.
pub fn set_flip_v(tree: &mut SceneTree, node_id: NodeId, flip: bool) {
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property("flip_v", Variant::Bool(flip));
    }
}

// ===========================================================================
// Camera2D properties
// ===========================================================================

/// Sets the `"zoom"` property on a Camera2D node.
pub fn set_zoom(tree: &mut SceneTree, node_id: NodeId, zoom: Vector2) {
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property("zoom", Variant::Vector2(zoom));
    }
}

/// Reads the `"zoom"` property, defaulting to [`Vector2::ONE`].
pub fn get_zoom(tree: &SceneTree, node_id: NodeId) -> Vector2 {
    tree.get_node(node_id)
        .map(|n| match n.get_property("zoom") {
            Variant::Vector2(v) => v,
            _ => Vector2::ONE,
        })
        .unwrap_or(Vector2::ONE)
}

/// Sets the `"current"` property on a Camera2D node.
pub fn set_camera_current(tree: &mut SceneTree, node_id: NodeId, current: bool) {
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property("current", Variant::Bool(current));
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node::Node;
    use std::f32::consts::FRAC_PI_2;

    const EPSILON: f32 = 1e-4;

    fn approx_eq(a: f32, b: f32) -> bool {
        (a - b).abs() < EPSILON
    }

    fn approx_vec(a: Vector2, b: Vector2) -> bool {
        approx_eq(a.x, b.x) && approx_eq(a.y, b.y)
    }

    /// Helper: build a tree with root and return (tree, root_id).
    fn make_tree() -> SceneTree {
        SceneTree::new()
    }

    // -- Node2D position/rotation/scale -------------------------------------

    #[test]
    fn default_position_is_zero() {
        let tree = make_tree();
        let root = tree.root_id();
        assert_eq!(get_position(&tree, root), Vector2::ZERO);
    }

    #[test]
    fn set_get_position() {
        let mut tree = make_tree();
        let root = tree.root_id();
        let node = Node::new("Player", "Node2D");
        let id = tree.add_child(root, node).unwrap();

        set_position(&mut tree, id, Vector2::new(100.0, 200.0));
        assert_eq!(get_position(&tree, id), Vector2::new(100.0, 200.0));
    }

    #[test]
    fn set_get_rotation() {
        let mut tree = make_tree();
        let root = tree.root_id();
        let node = Node::new("Turret", "Node2D");
        let id = tree.add_child(root, node).unwrap();

        set_rotation(&mut tree, id, FRAC_PI_2);
        assert!(approx_eq(get_rotation(&tree, id), FRAC_PI_2));
    }

    #[test]
    fn default_rotation_is_zero() {
        let tree = make_tree();
        assert!(approx_eq(get_rotation(&tree, tree.root_id()), 0.0));
    }

    #[test]
    fn set_get_scale() {
        let mut tree = make_tree();
        let root = tree.root_id();
        let node = Node::new("Scaled", "Node2D");
        let id = tree.add_child(root, node).unwrap();

        set_scale(&mut tree, id, Vector2::new(2.0, 3.0));
        assert_eq!(get_scale(&tree, id), Vector2::new(2.0, 3.0));
    }

    #[test]
    fn default_scale_is_one() {
        let tree = make_tree();
        assert_eq!(get_scale(&tree, tree.root_id()), Vector2::ONE);
    }

    // -- Local transform composition ----------------------------------------

    #[test]
    fn local_transform_translate_only() {
        let mut tree = make_tree();
        let root = tree.root_id();
        let node = Node::new("N", "Node2D");
        let id = tree.add_child(root, node).unwrap();

        set_position(&mut tree, id, Vector2::new(10.0, 20.0));
        let t = get_local_transform(&tree, id);
        let p = t.xform(Vector2::ZERO);
        assert!(approx_vec(p, Vector2::new(10.0, 20.0)));
    }

    #[test]
    fn local_transform_translate_and_rotate() {
        let mut tree = make_tree();
        let root = tree.root_id();
        let node = Node::new("N", "Node2D");
        let id = tree.add_child(root, node).unwrap();

        set_position(&mut tree, id, Vector2::new(5.0, 0.0));
        set_rotation(&mut tree, id, FRAC_PI_2);

        let t = get_local_transform(&tree, id);
        // A point at (1,0) local should be rotated 90deg to (0,1), then translated to (5,1).
        let p = t.xform(Vector2::new(1.0, 0.0));
        assert!(approx_vec(p, Vector2::new(5.0, 1.0)));
    }

    // -- Global transform through hierarchy ---------------------------------

    #[test]
    fn global_transform_three_levels() {
        let mut tree = make_tree();
        let root = tree.root_id();

        let grandparent = Node::new("GP", "Node2D");
        let gp_id = tree.add_child(root, grandparent).unwrap();
        set_position(&mut tree, gp_id, Vector2::new(100.0, 0.0));

        let parent = Node::new("P", "Node2D");
        let p_id = tree.add_child(gp_id, parent).unwrap();
        set_position(&mut tree, p_id, Vector2::new(0.0, 50.0));

        let child = Node::new("C", "Node2D");
        let c_id = tree.add_child(p_id, child).unwrap();
        set_position(&mut tree, c_id, Vector2::new(10.0, 10.0));

        let global = get_global_transform(&tree, c_id);
        let world_pos = global.xform(Vector2::ZERO);
        // 100+0+10 = 110, 0+50+10 = 60
        assert!(approx_vec(world_pos, Vector2::new(110.0, 60.0)));
    }

    #[test]
    fn moving_parent_updates_child_global_position() {
        let mut tree = make_tree();
        let root = tree.root_id();

        let parent = Node::new("Parent", "Node2D");
        let p_id = tree.add_child(root, parent).unwrap();
        set_position(&mut tree, p_id, Vector2::new(10.0, 0.0));

        let child = Node::new("Child", "Node2D");
        let c_id = tree.add_child(p_id, child).unwrap();
        set_position(&mut tree, c_id, Vector2::new(5.0, 5.0));

        // Before move: child global = (15, 5).
        let g1 = get_global_transform(&tree, c_id).xform(Vector2::ZERO);
        assert!(approx_vec(g1, Vector2::new(15.0, 5.0)));

        // Move parent to (100, 0).
        set_position(&mut tree, p_id, Vector2::new(100.0, 0.0));

        // After move: child global = (105, 5).
        let g2 = get_global_transform(&tree, c_id).xform(Vector2::ZERO);
        assert!(approx_vec(g2, Vector2::new(105.0, 5.0)));
    }

    // -- Visibility ---------------------------------------------------------

    #[test]
    fn default_visible_is_true() {
        let tree = make_tree();
        assert!(is_visible(&tree, tree.root_id()));
    }

    #[test]
    fn set_visible_false() {
        let mut tree = make_tree();
        let root = tree.root_id();
        let node = Node::new("N", "CanvasItem");
        let id = tree.add_child(root, node).unwrap();

        set_visible(&mut tree, id, false);
        assert!(!is_visible(&tree, id));
    }

    #[test]
    fn is_visible_in_tree_checks_parents() {
        let mut tree = make_tree();
        let root = tree.root_id();

        let parent = Node::new("Parent", "Node2D");
        let p_id = tree.add_child(root, parent).unwrap();

        let child = Node::new("Child", "Sprite2D");
        let c_id = tree.add_child(p_id, child).unwrap();

        // Both visible by default.
        assert!(is_visible_in_tree(&tree, c_id));

        // Hide the parent.
        set_visible(&mut tree, p_id, false);
        assert!(!is_visible_in_tree(&tree, c_id));
        // The child itself is still "visible", but not "visible in tree".
        assert!(is_visible(&tree, c_id));
    }

    // -- Z-index ------------------------------------------------------------

    #[test]
    fn z_index_default_and_set() {
        let mut tree = make_tree();
        let root = tree.root_id();
        let node = Node::new("N", "Node2D");
        let id = tree.add_child(root, node).unwrap();

        assert_eq!(get_z_index(&tree, id), 0);
        set_z_index(&mut tree, id, 5);
        assert_eq!(get_z_index(&tree, id), 5);
    }

    // -- Sprite2D properties ------------------------------------------------

    #[test]
    fn sprite2d_texture_roundtrip() {
        let mut tree = make_tree();
        let root = tree.root_id();
        let node = Node::new("Spr", "Sprite2D");
        let id = tree.add_child(root, node).unwrap();

        assert_eq!(get_texture_path(&tree, id), None);
        set_texture_path(&mut tree, id, "res://icon.png");
        assert_eq!(get_texture_path(&tree, id), Some("res://icon.png".into()));
    }

    #[test]
    fn sprite2d_flip_and_offset() {
        let mut tree = make_tree();
        let root = tree.root_id();
        let node = Node::new("Spr", "Sprite2D");
        let id = tree.add_child(root, node).unwrap();

        set_offset(&mut tree, id, Vector2::new(16.0, 32.0));
        set_flip_h(&mut tree, id, true);
        set_flip_v(&mut tree, id, false);

        let n = tree.get_node(id).unwrap();
        assert_eq!(n.get_property("offset"), Variant::Vector2(Vector2::new(16.0, 32.0)));
        assert_eq!(n.get_property("flip_h"), Variant::Bool(true));
        assert_eq!(n.get_property("flip_v"), Variant::Bool(false));
    }

    // -- Camera2D properties ------------------------------------------------

    #[test]
    fn camera2d_zoom_default_and_set() {
        let mut tree = make_tree();
        let root = tree.root_id();
        let node = Node::new("Cam", "Camera2D");
        let id = tree.add_child(root, node).unwrap();

        assert_eq!(get_zoom(&tree, id), Vector2::ONE);
        set_zoom(&mut tree, id, Vector2::new(2.0, 2.0));
        assert_eq!(get_zoom(&tree, id), Vector2::new(2.0, 2.0));
    }

    #[test]
    fn camera2d_current() {
        let mut tree = make_tree();
        let root = tree.root_id();
        let node = Node::new("Cam", "Camera2D");
        let id = tree.add_child(root, node).unwrap();

        set_camera_current(&mut tree, id, true);
        let n = tree.get_node(id).unwrap();
        assert_eq!(n.get_property("current"), Variant::Bool(true));
    }

    // -- set_global_position (inverse transform) ----------------------------

    #[test]
    fn set_global_position_inverse_transform() {
        let mut tree = make_tree();
        let root = tree.root_id();

        let parent = Node::new("Parent", "Node2D");
        let p_id = tree.add_child(root, parent).unwrap();
        set_position(&mut tree, p_id, Vector2::new(100.0, 50.0));

        let child = Node::new("Child", "Node2D");
        let c_id = tree.add_child(p_id, child).unwrap();

        // Place the child at global (150, 75).
        set_global_position(&mut tree, c_id, Vector2::new(150.0, 75.0));

        // Local position should be (50, 25).
        let local = get_position(&tree, c_id);
        assert!(approx_vec(local, Vector2::new(50.0, 25.0)));

        // Verify global transform matches.
        let g = get_global_transform(&tree, c_id).xform(Vector2::ZERO);
        assert!(approx_vec(g, Vector2::new(150.0, 75.0)));
    }

    // -- Modulate -----------------------------------------------------------

    #[test]
    fn set_modulate_stores_color() {
        let mut tree = make_tree();
        let root = tree.root_id();
        let node = Node::new("N", "Node2D");
        let id = tree.add_child(root, node).unwrap();

        let red = Color::new(1.0, 0.0, 0.0, 1.0);
        set_modulate(&mut tree, id, red);
        let n = tree.get_node(id).unwrap();
        assert_eq!(n.get_property("modulate"), Variant::Color(red));
    }
}
