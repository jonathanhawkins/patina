//! CanvasLayer node property helpers for HUD and overlay rendering.
//!
//! In Godot, a CanvasLayer creates a separate rendering layer that is
//! not affected by the main canvas transform. This is used for HUDs,
//! pause menus, and other UI overlays that should remain fixed on screen
//! regardless of camera movement.
//!
//! Follows the same pattern as [`node2d`](crate::node2d): typed helper
//! functions that read and write well-known properties on nodes stored in
//! a [`SceneTree`].

use gdcore::math::{Transform2D, Vector2};
use gdvariant::Variant;

use crate::node::NodeId;
use crate::scene_tree::SceneTree;

/// Sets the rendering layer index (default 1, higher = rendered later/on top).
pub fn set_layer(tree: &mut SceneTree, node_id: NodeId, layer: i64) {
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property("layer", Variant::Int(layer));
    }
}

/// Gets the rendering layer index, defaulting to `1`.
pub fn get_layer(tree: &SceneTree, node_id: NodeId) -> i64 {
    tree.get_node(node_id)
        .map(|n| match n.get_property("layer") {
            Variant::Int(i) => i,
            _ => 1,
        })
        .unwrap_or(1)
}

/// Sets the layer offset in pixels.
pub fn set_offset(tree: &mut SceneTree, node_id: NodeId, offset: Vector2) {
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property("offset", Variant::Vector2(offset));
    }
}

/// Gets the layer offset, defaulting to `Vector2::ZERO`.
pub fn get_offset(tree: &SceneTree, node_id: NodeId) -> Vector2 {
    tree.get_node(node_id)
        .map(|n| match n.get_property("offset") {
            Variant::Vector2(v) => v,
            _ => Vector2::ZERO,
        })
        .unwrap_or(Vector2::ZERO)
}

/// Sets the layer rotation in radians.
pub fn set_rotation(tree: &mut SceneTree, node_id: NodeId, radians: f64) {
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property("rotation", Variant::Float(radians));
    }
}

/// Gets the layer rotation in radians, defaulting to `0.0`.
pub fn get_rotation(tree: &SceneTree, node_id: NodeId) -> f64 {
    tree.get_node(node_id)
        .map(|n| match n.get_property("rotation") {
            Variant::Float(f) => f,
            _ => 0.0,
        })
        .unwrap_or(0.0)
}

/// Sets the layer scale.
pub fn set_scale(tree: &mut SceneTree, node_id: NodeId, scale: Vector2) {
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property("scale", Variant::Vector2(scale));
    }
}

/// Gets the layer scale, defaulting to `Vector2::ONE`.
pub fn get_scale(tree: &SceneTree, node_id: NodeId) -> Vector2 {
    tree.get_node(node_id)
        .map(|n| match n.get_property("scale") {
            Variant::Vector2(v) => v,
            _ => Vector2::ONE,
        })
        .unwrap_or(Vector2::ONE)
}

/// Sets whether the layer is visible.
pub fn set_visible(tree: &mut SceneTree, node_id: NodeId, visible: bool) {
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property("visible", Variant::Bool(visible));
    }
}

/// Gets whether the layer is visible, defaulting to `true`.
pub fn is_visible(tree: &SceneTree, node_id: NodeId) -> bool {
    tree.get_node(node_id)
        .map(|n| !matches!(n.get_property("visible"), Variant::Bool(false)))
        .unwrap_or(true)
}

/// Sets whether the layer follows the viewport transform.
pub fn set_follow_viewport_enabled(tree: &mut SceneTree, node_id: NodeId, enabled: bool) {
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property("follow_viewport_enabled", Variant::Bool(enabled));
    }
}

/// Gets whether follow-viewport is enabled, defaulting to `false`.
pub fn get_follow_viewport_enabled(tree: &SceneTree, node_id: NodeId) -> bool {
    tree.get_node(node_id)
        .map(|n| {
            matches!(
                n.get_property("follow_viewport_enabled"),
                Variant::Bool(true)
            )
        })
        .unwrap_or(false)
}

/// Sets the follow-viewport scale factor.
pub fn set_follow_viewport_scale(tree: &mut SceneTree, node_id: NodeId, scale: f64) {
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property("follow_viewport_scale", Variant::Float(scale));
    }
}

/// Gets the follow-viewport scale factor, defaulting to `1.0`.
pub fn get_follow_viewport_scale(tree: &SceneTree, node_id: NodeId) -> f64 {
    tree.get_node(node_id)
        .map(|n| match n.get_property("follow_viewport_scale") {
            Variant::Float(f) => f,
            _ => 1.0,
        })
        .unwrap_or(1.0)
}

/// The layer's own transform, which pre-multiplies every descendant's
/// (pat-0q2kx.212).
///
/// Mirrors Godot's `CanvasLayer::_update_xform` (`scene/main/canvas_layer.cpp`):
/// the basis comes from `rotation` and `scale`, and the origin is `offset` —
/// **not** `position`, which a CanvasLayer does not have. An explicitly assigned
/// `transform` property wins outright, because Godot's `set_transform` overwrites
/// the decomposed trio rather than composing with it.
///
/// ⚠️ This is the piece that makes a CanvasLayer's placement mean anything.
/// `node2d::get_global_transform` walks up multiplying each ancestor's *local*
/// transform, and a CanvasLayer is not a `CanvasItem` — it has no local transform
/// in that sense — so the walk skips straight past it and the layer reads as
/// identity. Composition has to restart here.
pub fn layer_transform(tree: &SceneTree, node_id: NodeId) -> Transform2D {
    if let Some(node) = tree.get_node(node_id) {
        if let Variant::Transform2D(t) = node.get_property("transform") {
            return t;
        }
    }
    Transform2D::from_rotation_scale_skew_origin(
        get_rotation(tree, node_id) as f32,
        get_scale(tree, node_id),
        0.0,
        get_offset(tree, node_id),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node::Node;

    fn make_tree_with_canvas_layer() -> (SceneTree, NodeId) {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let node = Node::new("HUD", "CanvasLayer");
        let id = tree.add_child(root, node).unwrap();
        (tree, id)
    }

    #[test]
    fn layer_default() {
        let (tree, id) = make_tree_with_canvas_layer();
        assert_eq!(get_layer(&tree, id), 1);
    }

    // -- layer_transform (pat-0q2kx.212) ------------------------------------

    #[test]
    fn layer_transform_defaults_to_identity() {
        let (tree, id) = make_tree_with_canvas_layer();
        assert_eq!(layer_transform(&tree, id), Transform2D::IDENTITY);
    }

    #[test]
    fn layer_transform_uses_offset_as_the_origin() {
        let (mut tree, id) = make_tree_with_canvas_layer();
        set_offset(&mut tree, id, Vector2::new(50.0, 20.0));
        let t = layer_transform(&tree, id);
        // ⚠️ `offset`, not `position` — a CanvasLayer has no `position`, which is
        // exactly why the Node2D transform walk misses it.
        assert_eq!(t.origin, Vector2::new(50.0, 20.0));
        assert_eq!(
            t.x,
            Vector2::new(1.0, 0.0),
            "no rotation means no basis change"
        );
    }

    #[test]
    fn layer_transform_composes_rotation_and_scale() {
        let (mut tree, id) = make_tree_with_canvas_layer();
        set_rotation(&mut tree, id, std::f64::consts::FRAC_PI_2);
        set_scale(&mut tree, id, Vector2::new(2.0, 3.0));
        let t = layer_transform(&tree, id);

        // A quarter turn sends +X to +Y, scaled by scale.x.
        assert!(
            (t.x.x).abs() < 1e-5,
            "x basis should have no x component: {:?}",
            t.x
        );
        assert!(
            (t.x.y - 2.0).abs() < 1e-5,
            "x basis magnitude is scale.x: {:?}",
            t.x
        );
        // …and +Y to -X, scaled by scale.y.
        assert!(
            (t.y.x + 3.0).abs() < 1e-5,
            "y basis is -scale.y on x: {:?}",
            t.y
        );
        assert!(
            (t.y.y).abs() < 1e-5,
            "y basis should have no y component: {:?}",
            t.y
        );
    }

    #[test]
    fn an_explicit_transform_property_wins_over_the_trio() {
        let (mut tree, id) = make_tree_with_canvas_layer();
        // Author both spellings; Godot's `set_transform` overwrites the
        // decomposed trio rather than composing with it, so the explicit one
        // must win outright — not be multiplied on top.
        set_offset(&mut tree, id, Vector2::new(50.0, 20.0));
        set_rotation(&mut tree, id, 1.0);
        let explicit = Transform2D::translated(Vector2::new(-7.0, 3.0));
        tree.get_node_mut(id)
            .unwrap()
            .set_property("transform", Variant::Transform2D(explicit));

        assert_eq!(
            layer_transform(&tree, id),
            explicit,
            "the explicit `transform` must win outright, with no trace of the \
             offset/rotation that was also set"
        );
    }

    #[test]
    fn set_get_layer() {
        let (mut tree, id) = make_tree_with_canvas_layer();
        set_layer(&mut tree, id, 5);
        assert_eq!(get_layer(&tree, id), 5);
    }

    #[test]
    fn offset_default() {
        let (tree, id) = make_tree_with_canvas_layer();
        assert_eq!(get_offset(&tree, id), Vector2::ZERO);
    }

    #[test]
    fn set_get_offset() {
        let (mut tree, id) = make_tree_with_canvas_layer();
        set_offset(&mut tree, id, Vector2::new(50.0, 100.0));
        assert_eq!(get_offset(&tree, id), Vector2::new(50.0, 100.0));
    }

    #[test]
    fn rotation_default() {
        let (tree, id) = make_tree_with_canvas_layer();
        assert!((get_rotation(&tree, id) - 0.0).abs() < 1e-6);
    }

    #[test]
    fn set_get_rotation() {
        let (mut tree, id) = make_tree_with_canvas_layer();
        set_rotation(&mut tree, id, std::f64::consts::FRAC_PI_4);
        assert!((get_rotation(&tree, id) - std::f64::consts::FRAC_PI_4).abs() < 1e-6);
    }

    #[test]
    fn scale_default() {
        let (tree, id) = make_tree_with_canvas_layer();
        assert_eq!(get_scale(&tree, id), Vector2::ONE);
    }

    #[test]
    fn set_get_scale() {
        let (mut tree, id) = make_tree_with_canvas_layer();
        set_scale(&mut tree, id, Vector2::new(2.0, 2.0));
        assert_eq!(get_scale(&tree, id), Vector2::new(2.0, 2.0));
    }

    #[test]
    fn visible_default() {
        let (tree, id) = make_tree_with_canvas_layer();
        assert!(is_visible(&tree, id));
    }

    #[test]
    fn set_get_visible() {
        let (mut tree, id) = make_tree_with_canvas_layer();
        set_visible(&mut tree, id, false);
        assert!(!is_visible(&tree, id));
    }

    #[test]
    fn follow_viewport_default() {
        let (tree, id) = make_tree_with_canvas_layer();
        assert!(!get_follow_viewport_enabled(&tree, id));
        assert!((get_follow_viewport_scale(&tree, id) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn set_get_follow_viewport() {
        let (mut tree, id) = make_tree_with_canvas_layer();
        set_follow_viewport_enabled(&mut tree, id, true);
        set_follow_viewport_scale(&mut tree, id, 0.5);
        assert!(get_follow_viewport_enabled(&tree, id));
        assert!((get_follow_viewport_scale(&tree, id) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn canvas_layer_with_children() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let cl = tree
            .add_child(root, Node::new("HUD", "CanvasLayer"))
            .unwrap();
        let label = tree.add_child(cl, Node::new("Score", "Label")).unwrap();
        let btn = tree.add_child(cl, Node::new("Pause", "Button")).unwrap();

        assert_eq!(tree.get_node(cl).unwrap().children().len(), 2);
        assert_eq!(tree.get_node(label).unwrap().parent(), Some(cl));
        assert_eq!(tree.get_node(btn).unwrap().parent(), Some(cl));
    }

    #[test]
    fn canvas_layer_from_tscn() {
        let tscn = r#"[gd_scene format=3 uid="uid://canvas_test"]

[node name="Game" type="Node2D"]

[node name="HUD" type="CanvasLayer" parent="."]
layer = 10

[node name="ScoreLabel" type="Label" parent="HUD"]

[node name="PauseMenu" type="CanvasLayer" parent="."]
layer = 20
visible = false
"#;
        let packed = crate::packed_scene::PackedScene::from_tscn(tscn).unwrap();
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        crate::packed_scene::add_packed_scene_to_tree(&mut tree, root, &packed).unwrap();

        let hud = tree.get_node_by_path("/root/Game/HUD").unwrap();
        assert_eq!(tree.get_node(hud).unwrap().class_name(), "CanvasLayer");
        assert_eq!(get_layer(&tree, hud), 10);

        let pause = tree.get_node_by_path("/root/Game/PauseMenu").unwrap();
        assert_eq!(get_layer(&tree, pause), 20);
        assert!(!is_visible(&tree, pause));

        let score = tree.get_node_by_path("/root/Game/HUD/ScoreLabel").unwrap();
        assert_eq!(tree.get_node(score).unwrap().class_name(), "Label");
    }
}
