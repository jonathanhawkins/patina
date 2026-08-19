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
use gdrender2d::light2d::{
    apply_lights_to_framebuffer, CanvasAmbient, Light2D, Light2DBlendMode, LightOccluder2D,
    ShadowFilter,
};
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
/// Reads the `Light2D` base-class properties both light nodes share
/// (pat-0q2kx.130). Kept in one place so `PointLight2D` and
/// `DirectionalLight2D` cannot drift apart.
///
/// Every default is Godot's: `blend_mode` MIX, cull masks layer 1, an opaque
/// black `shadow_color` (a fully blocking shadow), no shadow filtering.
fn apply_common_light_props(node: &crate::node::Node, light: &mut Light2D) {
    light.editor_only = prop_bool(node, "editor_only", false);
    light.blend_mode = match node.get_property("blend_mode") {
        Variant::Int(1) => Light2DBlendMode::Sub,
        Variant::Int(2) => Light2DBlendMode::Mix,
        // Godot's enum is ADD=0, SUB=1, MIX=2, but the *property* default is
        // ADD's neighbour MIX; an absent value keeps the constructor's MIX.
        Variant::Int(0) => Light2DBlendMode::Add,
        Variant::String(ref s) => match s.as_str() {
            "add" | "ADD" => Light2DBlendMode::Add,
            "sub" | "SUB" => Light2DBlendMode::Sub,
            _ => Light2DBlendMode::Mix,
        },
        _ => light.blend_mode,
    };
    light.range_item_cull_mask = prop_u32(node, "range_item_cull_mask", 1);
    light.shadow_item_cull_mask = prop_u32(node, "shadow_item_cull_mask", 1);
    light.shadow_color = prop_color(node, "shadow_color", Color::new(0.0, 0.0, 0.0, 1.0));
    light.shadow_filter = match node.get_property("shadow_filter") {
        Variant::Int(i) => ShadowFilter::from_godot_int(i),
        _ => ShadowFilter::None,
    };
    light.shadow_filter_smooth = prop_f32(node, "shadow_filter_smooth", 0.0);
}

pub fn collect_lights_2d(tree: &SceneTree) -> Vec<Light2D> {
    let mut lights = Vec::new();
    for id in tree.all_nodes_in_tree_order() {
        let Some(node) = tree.get_node(id) else {
            continue;
        };
        match node.class_name() {
            "PointLight2D" => {
                // GLOBAL, not local (pat-s3rrv.21): a torch parented to a moving
                // player must light where the player is, not the origin.
                let pos = crate::node2d::get_global_transform(tree, id).origin;
                let mut light = Light2D::point(
                    pos,
                    prop_color(node, "color", Color::WHITE),
                    prop_f32(node, "energy", 1.0),
                    prop_f32(node, "range", 0.0),
                );
                light.light_mask = prop_u32(node, "light_mask", 1);
                light.enabled = prop_bool(node, "enabled", true);
                light.shadow_enabled = prop_bool(node, "shadow_enabled", true);
                apply_common_light_props(node, &mut light);
                // PointLight2D-only footprint controls (pat-0q2kx.130).
                light.offset = prop_vec2(node, "offset", Vector2::new(0.0, 0.0));
                light.texture_scale = prop_f32(node, "texture_scale", 1.0);
                light.texture_path = match node.get_property("texture") {
                    Variant::String(path) if !path.is_empty() => Some(path),
                    _ => None,
                };
                lights.push(light);
            }
            "DirectionalLight2D" => {
                // Godot's DirectionalLight2D has NO `direction` property — the
                // light vector comes from the node's global ROTATION
                // (scene/2d/light_2d.cpp). Reading a `direction` that authored
                // scenes never carry meant every directional light silently used
                // the (0,-1) fallback regardless of how it was rotated
                // (pat-s3rrv.21).
                //
                // At rotation 0 the light points down -Y, matching the previous
                // default, and rotating the node sweeps it from there.
                let rot = crate::node2d::get_global_rotation(tree, id);
                let dir = Vector2::new(rot.sin(), -rot.cos());
                let mut light = Light2D::directional(
                    dir,
                    prop_color(node, "color", Color::WHITE),
                    prop_f32(node, "energy", 1.0),
                );
                light.light_mask = prop_u32(node, "light_mask", 1);
                light.enabled = prop_bool(node, "enabled", true);
                apply_common_light_props(node, &mut light);
                lights.push(light);
            }
            _ => {}
        }
    }
    lights
}

/// Builds [`LightOccluder2D`] values from `LightOccluder2D` nodes.
///
/// Godot stores the geometry in an `OccluderPolygon2D` **sub-resource**
/// (`occluder = SubResource("…")`), not on the node
/// (upstream/godot/scene/2d/light_occluder_2d.cpp), so the loader flattens it to
/// `__occluder__polygon` / `__occluder__closed` / `__occluder__cull_mode` and
/// this reads those first. Before pat-s3rrv.21 only a bare `polygon` on the node
/// was consulted, which is `Nil` for every authored scene — `collect_occluders_2d`
/// returned empty and 2D shadows never appeared from a real `.tscn`. The bare
/// property is still honored so hand-built trees keep working.
///
/// Points are returned in WORLD space: an occluder parented under a moving
/// `Node2D` must cast its shadow where the parent put it, not at the origin.
pub fn collect_occluders_2d(tree: &SceneTree) -> Vec<LightOccluder2D> {
    let mut occluders = Vec::new();
    for id in tree.all_nodes_in_tree_order() {
        let Some(node) = tree.get_node(id) else {
            continue;
        };
        if node.class_name() != "LightOccluder2D" {
            continue;
        }
        let local_points = match node.get_property("__occluder__polygon") {
            Variant::PackedVector2Array(v) => v,
            // Fallback: geometry set directly on the node (hand-built trees).
            _ => match node.get_property("polygon") {
                Variant::PackedVector2Array(v) => v,
                _ => continue,
            },
        };
        if local_points.len() < 2 {
            continue;
        }
        let xform = crate::node2d::get_global_transform(tree, id);
        let points: Vec<Vector2> = local_points.into_iter().map(|p| xform.xform(p)).collect();

        // `closed` lives on the sub-resource; fall back to the node, then to
        // Godot's default of true.
        let closed = match node.get_property("__occluder__closed") {
            Variant::Bool(b) => b,
            _ => prop_bool(node, "closed", true),
        };
        let mut occ = LightOccluder2D::new(points, closed);
        // Godot's property is `occluder_light_mask`; `light_mask` is the
        // CanvasItem one and is not what the shadow pass filters on.
        occ.light_mask = match node.get_property("occluder_light_mask") {
            Variant::Int(i) => i as u32,
            _ => prop_u32(node, "light_mask", 1),
        };
        occluders.push(occ);
    }

    // Tile occluders (pat-s3rrv.65).
    //
    // ⚠️ Until now `collect_occluders_2d` only ever looked at nodes whose class
    // is literally `LightOccluder2D`, so a tile could not cast a shadow at all —
    // which is why the occlusion half of the TileSet editor had to start here
    // rather than with a route: authoring it first would have written data that
    // round-trips through the file and lights nothing.
    for id in tree.all_nodes_in_tree_order() {
        let Some(node) = tree.get_node(id) else {
            continue;
        };
        if !matches!(node.class_name(), "TileMapLayer" | "TileMap") {
            continue;
        }
        let sources = match node.get_property(crate::tilemap::TILE_OCCLUDER_SOURCES) {
            Variant::PackedInt32Array(v) => v,
            _ => continue,
        };
        if sources.is_empty() {
            continue;
        }
        let cells = match node.get_property("tile_map_data") {
            Variant::PackedByteArray(bytes) => crate::tilemap::decode_tile_map_data(&bytes),
            _ => continue,
        };
        let tile_size = match node.get_property(crate::tilemap::TILE_SET_TILE_SIZE) {
            Variant::Vector2i(v) => v,
            _ => gdcore::math::Vector2i::new(16, 16),
        };
        let local_polygon = match node.get_property(crate::tilemap::TILE_OCCLUDER_POLYGON) {
            Variant::PackedVector2Array(v) => v,
            _ => Vec::new(),
        };
        // Layer-local -> world, the same transform the LightOccluder2D branch
        // applies. Using the node's position alone would drop rotation and
        // scale, so a rotated tilemap would cast unrotated shadows.
        let xform = crate::node2d::get_global_transform(tree, id);
        let light_mask = match node.get_property("occluder_light_mask") {
            Variant::Int(i) => i as u32,
            _ => 1,
        };
        for poly in
            crate::tilemap::tile_occluder_polygons(&cells, tile_size, &sources, &local_polygon)
        {
            let points: Vec<Vector2> = poly.into_iter().map(|p| xform.xform(p)).collect();
            // Tiles are solid, so their occluders are closed: an open polyline
            // would leave the far edge unlit-through, which reads as a shadow
            // with a missing side.
            let mut occ = LightOccluder2D::new(points, true);
            occ.light_mask = light_mask;
            occluders.push(occ);
        }
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

/// Returns the tint color of the first `CanvasModulate` node in the tree, if any.
/// Godot multiplies the entire canvas by this color as a final pass
/// (scene/2d/canvas_modulate.cpp) (pat-nzgdu.117).
pub fn collect_canvas_modulate(tree: &SceneTree) -> Option<Color> {
    for id in tree.all_nodes_in_tree_order() {
        let Some(node) = tree.get_node(id) else {
            continue;
        };
        if node.class_name() == "CanvasModulate" {
            return Some(prop_color(node, "color", Color::WHITE));
        }
    }
    None
}

/// Multiplies every pixel of `fb` by `color` — the whole-canvas tint a
/// `CanvasModulate` applies after lighting (pat-nzgdu.117).
pub fn apply_canvas_modulate(fb: &mut FrameBuffer, color: Color) {
    for p in fb.pixels.iter_mut() {
        *p = Color::new(
            (p.r * color.r).min(1.0),
            (p.g * color.g).min(1.0),
            (p.b * color.b).min(1.0),
            p.a,
        );
    }
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
