//! Collect 2D lights and occluders from scene nodes and drive the renderer's
//! light pass (pat-u3l8c.28).
//!
//! `gdrender2d::light2d` already computes per-pixel light contribution with
//! per-light `light_mask` filtering and occluder shadows, but it was fed no node
//! data (the post-pass ran only in the unused SoftwareRenderer path). This
//! module walks a [`SceneTree`], builds [`Light2D`]/[`LightOccluder2D`] values
//! from `PointLight2D`/`DirectionalLight2D`/`LightOccluder2D` nodes, and applies
//! them to a framebuffer honoring per-item `light_mask`.

use gdcore::math::{Color, Vector2};
use gdrender2d::light2d::{apply_lights_to_framebuffer, CanvasAmbient, Light2D, LightOccluder2D};
use gdrender2d::renderer::FrameBuffer;
use gdvariant::Variant;

use crate::node::Node;
use crate::scene_tree::SceneTree;

fn prop_f32(node: &Node, key: &str, default: f32) -> f32 {
    match node.get_property(key) {
        Variant::Float(f) => f as f32,
        Variant::Int(i) => i as f32,
        _ => default,
    }
}

fn prop_u32(node: &Node, key: &str, default: u32) -> u32 {
    match node.get_property(key) {
        Variant::Int(i) => i.max(0) as u32,
        _ => default,
    }
}

fn prop_bool(node: &Node, key: &str, default: bool) -> bool {
    match node.get_property(key) {
        Variant::Bool(b) => b,
        _ => default,
    }
}

fn prop_color(node: &Node, key: &str, default: Color) -> Color {
    match node.get_property(key) {
        Variant::Color(c) => c,
        _ => default,
    }
}

fn prop_vec2(node: &Node, key: &str, default: Vector2) -> Vector2 {
    match node.get_property(key) {
        Variant::Vector2(v) => v,
        _ => default,
    }
}

/// Builds [`Light2D`] values from `PointLight2D`/`DirectionalLight2D` nodes.
pub fn collect_lights_2d(tree: &SceneTree) -> Vec<Light2D> {
    let mut lights = Vec::new();
    for id in tree.all_nodes_in_tree_order() {
        let Some(node) = tree.get_node(id) else {
            continue;
        };
        match node.class_name() {
            "PointLight2D" => {
                let pos = crate::node2d::get_position(tree, id);
                let mut light = Light2D::point(
                    pos,
                    prop_color(node, "color", Color::WHITE),
                    prop_f32(node, "energy", 1.0),
                    prop_f32(node, "range", 0.0),
                );
                light.light_mask = prop_u32(node, "light_mask", 1);
                light.enabled = prop_bool(node, "enabled", true);
                light.shadow_enabled = prop_bool(node, "shadow_enabled", true);
                lights.push(light);
            }
            "DirectionalLight2D" => {
                let mut light = Light2D::directional(
                    prop_vec2(node, "direction", Vector2::new(0.0, -1.0)),
                    prop_color(node, "color", Color::WHITE),
                    prop_f32(node, "energy", 1.0),
                );
                light.light_mask = prop_u32(node, "light_mask", 1);
                light.enabled = prop_bool(node, "enabled", true);
                lights.push(light);
            }
            _ => {}
        }
    }
    lights
}

/// Builds [`LightOccluder2D`] values from `LightOccluder2D` nodes (their
/// `polygon` property supplies the occluder points).
pub fn collect_occluders_2d(tree: &SceneTree) -> Vec<LightOccluder2D> {
    let mut occluders = Vec::new();
    for id in tree.all_nodes_in_tree_order() {
        let Some(node) = tree.get_node(id) else {
            continue;
        };
        if node.class_name() != "LightOccluder2D" {
            continue;
        }
        let points = match node.get_property("polygon") {
            Variant::PackedVector2Array(v) => v,
            _ => continue,
        };
        if points.len() < 2 {
            continue;
        }
        let mut occ = LightOccluder2D::new(points, prop_bool(node, "closed", true));
        occ.light_mask = prop_u32(node, "light_mask", 1);
        occluders.push(occ);
    }
    occluders
}

/// Collects the scene's 2D lights + occluders and applies them to `fb` for an
/// item with `item_mask`.
pub fn apply_scene_lights(
    tree: &SceneTree,
    fb: &mut FrameBuffer,
    ambient: CanvasAmbient,
    item_mask: u32,
) {
    let lights = collect_lights_2d(tree);
    let occluders = collect_occluders_2d(tree);
    apply_lights_to_framebuffer(fb, &lights, &occluders, ambient, item_mask);
}

#[cfg(test)]
mod tests {
    use super::*;
    use gdrender2d::light2d::light_factor_at;

    fn point_light(tree: &mut SceneTree, pos: Vector2, energy: f32, range: f32, mask: i64) {
        let root = tree.root_id();
        let mut n = Node::new("Light", "PointLight2D");
        n.set_property("position", Variant::Vector2(pos));
        n.set_property("color", Variant::Color(Color::new(1.0, 1.0, 1.0, 1.0)));
        n.set_property("energy", Variant::Float(energy as f64));
        n.set_property("range", Variant::Float(range as f64));
        n.set_property("light_mask", Variant::Int(mask));
        tree.add_child(root, n).unwrap();
    }

    #[test]
    fn collects_lights_and_occluders_from_nodes() {
        let mut tree = SceneTree::new();
        point_light(&mut tree, Vector2::new(50.0, 10.0), 2.0, 200.0, 1);
        let root = tree.root_id();
        let mut occ = Node::new("Wall", "LightOccluder2D");
        occ.set_property(
            "polygon",
            Variant::PackedVector2Array(vec![Vector2::new(40.0, 50.0), Vector2::new(60.0, 50.0)]),
        );
        tree.add_child(root, occ).unwrap();

        assert_eq!(collect_lights_2d(&tree).len(), 1);
        assert_eq!(collect_occluders_2d(&tree).len(), 1);
    }

    #[test]
    fn occluder_shadows_and_mask_are_honored() {
        let mut tree = SceneTree::new();
        // Light above; a horizontal occluder wall at y=50 across x=[40,60].
        point_light(&mut tree, Vector2::new(50.0, 10.0), 2.0, 500.0, 1);
        let root = tree.root_id();
        let mut occ = Node::new("Wall", "LightOccluder2D");
        occ.set_property(
            "polygon",
            Variant::PackedVector2Array(vec![Vector2::new(40.0, 50.0), Vector2::new(60.0, 50.0)]),
        );
        occ.set_property("light_mask", Variant::Int(1));
        tree.add_child(root, occ).unwrap();

        let lights = collect_lights_2d(&tree);
        let occluders = collect_occluders_2d(&tree);
        let ambient = CanvasAmbient::default();

        // A point below the wall is in shadow; the same point with no occluder
        // is brighter -> the occluder specifically casts the shadow.
        let shadowed = light_factor_at(Vector2::new(50.0, 90.0), 1, &lights, &occluders, ambient);
        let no_occluder = light_factor_at(Vector2::new(50.0, 90.0), 1, &lights, &[], ambient);
        assert!(
            no_occluder.r > shadowed.r,
            "occluder should darken the shadowed point ({} vs {})",
            no_occluder.r,
            shadowed.r,
        );

        // An item whose mask does not match the light gets ambient only.
        let unmatched = light_factor_at(Vector2::new(50.0, 20.0), 2, &lights, &occluders, ambient);
        assert!(
            (unmatched.r - ambient.color.r).abs() < 1e-4,
            "non-matching light_mask should yield ambient only, got {}",
            unmatched.r,
        );
    }

    #[test]
    fn apply_scene_lights_brightens_pixel_under_light() {
        let mut tree = SceneTree::new();
        point_light(&mut tree, Vector2::new(4.0, 4.0), 3.0, 100.0, 1);
        let mut fb = FrameBuffer::new(16, 16, Color::new(0.2, 0.2, 0.2, 1.0));
        apply_scene_lights(&tree, &mut fb, CanvasAmbient::default(), 1);
        let near = fb.get_pixel(4, 4);
        assert!(
            near.r > 0.2,
            "light should brighten the pixel under it, got {}",
            near.r
        );
    }

    #[test]
    fn no_light_nodes_collects_nothing() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        tree.add_child(root, Node::new("Plain", "Node2D")).unwrap();
        assert!(collect_lights_2d(&tree).is_empty());
        assert!(collect_occluders_2d(&tree).is_empty());
    }
}
