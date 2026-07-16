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

    Transform2D::translated(pos) * Transform2D::rotated(rot) * Transform2D::scaled(scl)
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

/// The visibility state a Scene Tree row's eye indicator should display.
///
/// Mirrors Godot's three-way visibility presentation for CanvasItem / Node3D
/// rows: a node that is itself visible may still be hidden because an ancestor
/// is hidden ("inherited" hidden), which the dock distinguishes from a node the
/// user explicitly hid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VisibilityState {
    /// The node and all of its ancestors are visible (open eye).
    Visible,
    /// The node's own `visible` flag is `false` (closed/slashed eye).
    Hidden,
    /// The node's own `visible` flag is `true`, but an ancestor is hidden so it
    /// is not drawn (greyed/inherited-hidden eye).
    InheritedHidden,
}

/// Computes the Scene Tree visibility indicator for `node_id`.
///
/// - [`VisibilityState::Hidden`] when the node's own `visible` property is off.
/// - [`VisibilityState::Visible`] when the node and every ancestor are visible.
/// - [`VisibilityState::InheritedHidden`] when the node is itself visible but an
///   ancestor is hidden.
pub fn visibility_state(tree: &SceneTree, node_id: NodeId) -> VisibilityState {
    if !is_visible(tree, node_id) {
        VisibilityState::Hidden
    } else if is_visible_in_tree(tree, node_id) {
        VisibilityState::Visible
    } else {
        VisibilityState::InheritedHidden
    }
}

/// Toggles the node's own `visible` flag (the click action behind the eye
/// indicator) and returns the new value.
///
/// Only the clicked node's `visible` property is written; visibility propagates
/// to descendants implicitly via [`is_visible_in_tree`] / [`visibility_state`],
/// matching Godot: hiding a parent leaves each child's own `visible` flag
/// untouched but renders the child inherited-hidden.
pub fn toggle_visible(tree: &mut SceneTree, node_id: NodeId) -> bool {
    let new_value = !is_visible(tree, node_id);
    set_visible(tree, node_id, new_value);
    new_value
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
    tree.get_node(node_id)
        .and_then(|n| match n.get_property("texture") {
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
// NinePatchRect properties
// ===========================================================================

/// Sets the `"texture"` property on a NinePatchRect node.
pub fn set_nine_patch_texture(tree: &mut SceneTree, node_id: NodeId, path: &str) {
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property("texture", Variant::String(path.to_owned()));
    }
}

/// Sets the nine-patch margins on a NinePatchRect node.
///
/// Stores `margin_left`, `margin_top`, `margin_right`, `margin_bottom` as
/// individual `Int` properties matching Godot's convention.
pub fn set_nine_patch_margins(
    tree: &mut SceneTree,
    node_id: NodeId,
    left: i64,
    top: i64,
    right: i64,
    bottom: i64,
) {
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property("patch_margin_left", Variant::Int(left));
        node.set_property("patch_margin_top", Variant::Int(top));
        node.set_property("patch_margin_right", Variant::Int(right));
        node.set_property("patch_margin_bottom", Variant::Int(bottom));
    }
}

/// Reads the nine-patch margins, defaulting each to `0`.
pub fn get_nine_patch_margins(tree: &SceneTree, node_id: NodeId) -> (i64, i64, i64, i64) {
    let get = |prop: &str| -> i64 {
        tree.get_node(node_id)
            .map(|n| match n.get_property(prop) {
                Variant::Int(i) => i,
                _ => 0,
            })
            .unwrap_or(0)
    };
    (
        get("patch_margin_left"),
        get("patch_margin_top"),
        get("patch_margin_right"),
        get("patch_margin_bottom"),
    )
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

/// Godot Camera2D limit sentinel (`±10_000_000`) meaning "no limit".
pub const CAMERA2D_LIMIT_DEFAULT: f32 = 10_000_000.0;

/// Runtime configuration for a Camera2D: view limits, position smoothing, and
/// drag margins. Mirrors Godot's `Camera2D` properties that shape the canvas
/// transform. Values are the resolved runtime form (limits as `f32`, drag
/// margins as `[0, 1]` fractions of the half-view).
#[derive(Debug, Clone, Copy)]
pub struct Camera2DConfig {
    /// Left/right/top/bottom world-space clamp limits for the visible rect.
    pub limit_left: f32,
    pub limit_right: f32,
    pub limit_top: f32,
    pub limit_bottom: f32,
    /// When enabled, the camera eases toward its target instead of snapping.
    pub position_smoothing_enabled: bool,
    /// Smoothing rate (higher = snappier). Combined with `dt` as `speed * dt`.
    pub position_smoothing_speed: f32,
    /// Whether horizontal/vertical drag margins are active.
    pub drag_horizontal_enabled: bool,
    pub drag_vertical_enabled: bool,
    /// Drag margins as fractions `[0, 1]` of the half-view on each side.
    pub drag_left_margin: f32,
    pub drag_right_margin: f32,
    pub drag_top_margin: f32,
    pub drag_bottom_margin: f32,
    /// Camera zoom (Godot 4: larger zoom = magnified = smaller visible area).
    pub zoom: Vector2,
}

impl Default for Camera2DConfig {
    fn default() -> Self {
        Self {
            limit_left: -CAMERA2D_LIMIT_DEFAULT,
            limit_right: CAMERA2D_LIMIT_DEFAULT,
            limit_top: -CAMERA2D_LIMIT_DEFAULT,
            limit_bottom: CAMERA2D_LIMIT_DEFAULT,
            position_smoothing_enabled: false,
            position_smoothing_speed: 5.0,
            drag_horizontal_enabled: false,
            drag_vertical_enabled: false,
            drag_left_margin: 0.2,
            drag_right_margin: 0.2,
            drag_top_margin: 0.2,
            drag_bottom_margin: 0.2,
            zoom: Vector2::ONE,
        }
    }
}

/// Half the visible world-space extent for a viewport under `zoom`. In Godot 4
/// a larger zoom magnifies (shows less), so the half-extent divides by zoom.
fn camera2d_half_view(viewport_size: Vector2, zoom: Vector2) -> Vector2 {
    let zx = if zoom.x.abs() < 1e-6 { 1.0 } else { zoom.x };
    let zy = if zoom.y.abs() < 1e-6 { 1.0 } else { zoom.y };
    Vector2::new(viewport_size.x * 0.5 / zx, viewport_size.y * 0.5 / zy)
}

/// Applies the drag "dead zone": the camera only moves enough to keep `target`
/// within the drag margins around `current`. Disabled axes track the target
/// exactly. Returns the drag-adjusted desired camera center.
pub fn apply_drag_margins(
    config: &Camera2DConfig,
    current: Vector2,
    target: Vector2,
    half_view: Vector2,
) -> Vector2 {
    let x = if !config.drag_horizontal_enabled {
        target.x
    } else {
        let left_edge = current.x - config.drag_left_margin * half_view.x;
        let right_edge = current.x + config.drag_right_margin * half_view.x;
        if target.x < left_edge {
            target.x + config.drag_left_margin * half_view.x
        } else if target.x > right_edge {
            target.x - config.drag_right_margin * half_view.x
        } else {
            current.x
        }
    };
    let y = if !config.drag_vertical_enabled {
        target.y
    } else {
        let top_edge = current.y - config.drag_top_margin * half_view.y;
        let bottom_edge = current.y + config.drag_bottom_margin * half_view.y;
        if target.y < top_edge {
            target.y + config.drag_top_margin * half_view.y
        } else if target.y > bottom_edge {
            target.y - config.drag_bottom_margin * half_view.y
        } else {
            current.y
        }
    };
    Vector2::new(x, y)
}

/// Eases `current` toward `desired`. With smoothing disabled (or non-positive
/// `dt`) the camera snaps to `desired`; otherwise it lerps by `speed * dt`.
pub fn smooth_camera_position(
    config: &Camera2DConfig,
    current: Vector2,
    desired: Vector2,
    dt: f32,
) -> Vector2 {
    if config.position_smoothing_enabled && dt > 0.0 {
        let t = (config.position_smoothing_speed * dt).clamp(0.0, 1.0);
        current.lerp(desired, t)
    } else {
        desired
    }
}

/// Clamps a camera center so its visible rectangle stays within the configured
/// limits. When the limits are tighter than the view on an axis, the camera
/// centers on that axis's limit midpoint.
pub fn clamp_camera_to_limits(
    config: &Camera2DConfig,
    center: Vector2,
    half_view: Vector2,
) -> Vector2 {
    let clamp_axis = |v: f32, lo: f32, hi: f32, hv: f32| {
        let min = lo + hv;
        let max = hi - hv;
        if min <= max {
            v.clamp(min, max)
        } else {
            (lo + hi) * 0.5
        }
    };
    Vector2::new(
        clamp_axis(center.x, config.limit_left, config.limit_right, half_view.x),
        clamp_axis(center.y, config.limit_top, config.limit_bottom, half_view.y),
    )
}

/// Computes the next Camera2D center from a target: drag dead-zone → position
/// smoothing → limit clamp. The result feeds the canvas transform
/// (`viewport.camera_position`). `current` is the camera's previous center.
pub fn compute_camera2d_position(
    config: &Camera2DConfig,
    current: Vector2,
    target: Vector2,
    viewport_size: Vector2,
    dt: f32,
) -> Vector2 {
    let half_view = camera2d_half_view(viewport_size, config.zoom);
    let dragged = apply_drag_margins(config, current, target, half_view);
    let smoothed = smooth_camera_position(config, current, dragged, dt);
    clamp_camera_to_limits(config, smoothed, half_view)
}

// -- Camera2D limit / smoothing / drag property accessors -------------------

fn get_f32_prop(tree: &SceneTree, node_id: NodeId, key: &str, default: f32) -> f32 {
    tree.get_node(node_id)
        .map(|n| match n.get_property(key) {
            Variant::Float(f) => f as f32,
            Variant::Int(i) => i as f32,
            _ => default,
        })
        .unwrap_or(default)
}

fn get_bool_prop(tree: &SceneTree, node_id: NodeId, key: &str, default: bool) -> bool {
    tree.get_node(node_id)
        .map(|n| match n.get_property(key) {
            Variant::Bool(b) => b,
            _ => default,
        })
        .unwrap_or(default)
}

/// Sets a Camera2D limit (`limit_left`/`limit_right`/`limit_top`/`limit_bottom`)
/// in pixels. Godot stores limits as integers.
pub fn set_camera_limit(tree: &mut SceneTree, node_id: NodeId, side: &str, value: i64) {
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property(&format!("limit_{side}"), Variant::Int(value));
    }
}

/// Enables/sets position smoothing for a Camera2D node.
pub fn set_position_smoothing(
    tree: &mut SceneTree,
    node_id: NodeId,
    enabled: bool,
    speed: f32,
) {
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property("position_smoothing_enabled", Variant::Bool(enabled));
        node.set_property("position_smoothing_speed", Variant::Float(speed as f64));
    }
}

/// Sets a Camera2D drag margin fraction (`side` = left/right/top/bottom).
pub fn set_drag_margin(tree: &mut SceneTree, node_id: NodeId, side: &str, fraction: f32) {
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property(&format!("drag_{side}_margin"), Variant::Float(fraction as f64));
    }
}

/// Enables horizontal/vertical drag margins for a Camera2D node.
pub fn set_drag_enabled(
    tree: &mut SceneTree,
    node_id: NodeId,
    horizontal: bool,
    vertical: bool,
) {
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property("drag_horizontal_enabled", Variant::Bool(horizontal));
        node.set_property("drag_vertical_enabled", Variant::Bool(vertical));
    }
}

/// Reads a Camera2D node's limit/smoothing/drag/zoom properties into a
/// [`Camera2DConfig`], filling Godot defaults for any that are unset.
pub fn camera2d_config_from_node(tree: &SceneTree, node_id: NodeId) -> Camera2DConfig {
    let d = Camera2DConfig::default();
    Camera2DConfig {
        limit_left: get_f32_prop(tree, node_id, "limit_left", d.limit_left),
        limit_right: get_f32_prop(tree, node_id, "limit_right", d.limit_right),
        limit_top: get_f32_prop(tree, node_id, "limit_top", d.limit_top),
        limit_bottom: get_f32_prop(tree, node_id, "limit_bottom", d.limit_bottom),
        position_smoothing_enabled: get_bool_prop(
            tree,
            node_id,
            "position_smoothing_enabled",
            d.position_smoothing_enabled,
        ),
        position_smoothing_speed: get_f32_prop(
            tree,
            node_id,
            "position_smoothing_speed",
            d.position_smoothing_speed,
        ),
        drag_horizontal_enabled: get_bool_prop(
            tree,
            node_id,
            "drag_horizontal_enabled",
            d.drag_horizontal_enabled,
        ),
        drag_vertical_enabled: get_bool_prop(
            tree,
            node_id,
            "drag_vertical_enabled",
            d.drag_vertical_enabled,
        ),
        drag_left_margin: get_f32_prop(tree, node_id, "drag_left_margin", d.drag_left_margin),
        drag_right_margin: get_f32_prop(tree, node_id, "drag_right_margin", d.drag_right_margin),
        drag_top_margin: get_f32_prop(tree, node_id, "drag_top_margin", d.drag_top_margin),
        drag_bottom_margin: get_f32_prop(
            tree,
            node_id,
            "drag_bottom_margin",
            d.drag_bottom_margin,
        ),
        zoom: get_zoom(tree, node_id),
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

    #[test]
    fn scene_tree_visibility_toggle_reflects_state() {
        // root -> A -> B (a CanvasItem-style chain; the logic is class-agnostic).
        let mut tree = make_tree();
        let root = tree.root_id();
        let a_id = tree.add_child(root, Node::new("A", "Node2D")).unwrap();
        let b_id = tree.add_child(a_id, Node::new("B", "Sprite2D")).unwrap();

        // Everything visible by default -> both rows show Visible.
        assert_eq!(visibility_state(&tree, a_id), VisibilityState::Visible);
        assert_eq!(visibility_state(&tree, b_id), VisibilityState::Visible);

        // Clicking the eye on A flips A.visible to false and returns it.
        assert!(!toggle_visible(&mut tree, a_id));
        assert!(!is_visible(&tree, a_id));

        // A's own row now reads Hidden; B is itself still visible but an
        // ancestor is hidden, so its row reads InheritedHidden (propagation).
        assert_eq!(visibility_state(&tree, a_id), VisibilityState::Hidden);
        assert_eq!(
            visibility_state(&tree, b_id),
            VisibilityState::InheritedHidden
        );
        // B's own `visible` flag was NOT modified by hiding the ancestor.
        assert!(is_visible(&tree, b_id));

        // Toggling A back makes both rows Visible again.
        assert!(toggle_visible(&mut tree, a_id));
        assert_eq!(visibility_state(&tree, a_id), VisibilityState::Visible);
        assert_eq!(visibility_state(&tree, b_id), VisibilityState::Visible);

        // Hiding B directly reads Hidden on B while A stays Visible.
        assert!(!toggle_visible(&mut tree, b_id));
        assert_eq!(visibility_state(&tree, b_id), VisibilityState::Hidden);
        assert_eq!(visibility_state(&tree, a_id), VisibilityState::Visible);
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
        assert_eq!(
            n.get_property("offset"),
            Variant::Vector2(Vector2::new(16.0, 32.0))
        );
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

    // -- Camera2D limits / smoothing / drag ---------------------------------

    #[test]
    fn camera2d_clamps_view_to_limits() {
        let mut cfg = Camera2DConfig {
            limit_left: 0.0,
            limit_right: 1000.0,
            limit_top: 0.0,
            limit_bottom: 1000.0,
            ..Default::default()
        };
        cfg.zoom = Vector2::ONE;
        let half_view = Vector2::new(400.0, 300.0); // 800x600 viewport, zoom 1
        // A center near the top-left corner is pushed in so the view fits.
        let clamped = clamp_camera_to_limits(&cfg, Vector2::new(100.0, 100.0), half_view);
        assert!(approx_vec(clamped, Vector2::new(400.0, 300.0)), "{clamped:?}");
        // A center past the bottom-right is pulled back.
        let clamped2 = clamp_camera_to_limits(&cfg, Vector2::new(5000.0, 5000.0), half_view);
        assert!(approx_vec(clamped2, Vector2::new(600.0, 700.0)), "{clamped2:?}");
    }

    #[test]
    fn camera2d_smoothing_eases_toward_target() {
        let mut cfg = Camera2DConfig {
            position_smoothing_enabled: true,
            position_smoothing_speed: 5.0,
            ..Default::default()
        };
        // speed 5 * dt 0.1 = 0.5 -> halfway.
        let eased = smooth_camera_position(&cfg, Vector2::ZERO, Vector2::new(100.0, 0.0), 0.1);
        assert!(approx_eq(eased.x, 50.0), "eased halfway: {eased:?}");
        // Disabled -> snaps to target.
        cfg.position_smoothing_enabled = false;
        let snapped = smooth_camera_position(&cfg, Vector2::ZERO, Vector2::new(100.0, 0.0), 0.1);
        assert!(approx_vec(snapped, Vector2::new(100.0, 0.0)), "{snapped:?}");
    }

    #[test]
    fn camera2d_drag_margin_is_a_dead_zone() {
        let cfg = Camera2DConfig {
            drag_horizontal_enabled: true,
            drag_left_margin: 0.2,
            drag_right_margin: 0.2,
            ..Default::default()
        };
        let half_view = Vector2::new(400.0, 300.0); // dead zone = ±80 px
        // Target within the dead zone: camera does not move.
        let inside = apply_drag_margins(&cfg, Vector2::ZERO, Vector2::new(50.0, 0.0), half_view);
        assert!(approx_eq(inside.x, 0.0), "inside dead zone: {inside:?}");
        // Target past the right edge: camera moves so target sits on the margin.
        let outside = apply_drag_margins(&cfg, Vector2::ZERO, Vector2::new(200.0, 0.0), half_view);
        assert!(approx_eq(outside.x, 120.0), "dragged to margin: {outside:?}");
        // Vertical drag disabled -> y tracks the target exactly.
        let v = apply_drag_margins(&cfg, Vector2::ZERO, Vector2::new(0.0, 999.0), half_view);
        assert!(approx_eq(v.y, 999.0), "disabled axis tracks target: {v:?}");
    }

    #[test]
    fn camera2d_compute_position_composes_drag_smoothing_limits() {
        // No drag / no smoothing / wide limits -> follows target exactly.
        let cfg = Camera2DConfig::default();
        let p = compute_camera2d_position(
            &cfg,
            Vector2::ZERO,
            Vector2::new(500.0, 500.0),
            Vector2::new(800.0, 600.0),
            0.016,
        );
        assert!(approx_vec(p, Vector2::new(500.0, 500.0)), "{p:?}");

        // Tight limits clamp the followed target.
        let cfg2 = Camera2DConfig {
            limit_left: 0.0,
            limit_right: 1000.0,
            limit_top: 0.0,
            limit_bottom: 1000.0,
            ..Default::default()
        };
        let p2 = compute_camera2d_position(
            &cfg2,
            Vector2::ZERO,
            Vector2::new(5000.0, 5000.0),
            Vector2::new(800.0, 600.0),
            0.016,
        );
        assert!(approx_vec(p2, Vector2::new(600.0, 700.0)), "{p2:?}");
    }

    #[test]
    fn camera2d_config_reads_node_properties() {
        let mut tree = make_tree();
        let root = tree.root_id();
        let node = Node::new("Cam", "Camera2D");
        let id = tree.add_child(root, node).unwrap();

        // Unset -> Godot defaults.
        let d = camera2d_config_from_node(&tree, id);
        assert_eq!(d.limit_right, CAMERA2D_LIMIT_DEFAULT);
        assert!(!d.position_smoothing_enabled);
        assert!(approx_eq(d.position_smoothing_speed, 5.0));
        assert!(approx_eq(d.drag_left_margin, 0.2));

        // Set via helpers -> config reflects them.
        set_camera_limit(&mut tree, id, "left", -100);
        set_camera_limit(&mut tree, id, "right", 100);
        set_position_smoothing(&mut tree, id, true, 8.0);
        set_drag_enabled(&mut tree, id, true, false);
        set_drag_margin(&mut tree, id, "left", 0.35);
        set_zoom(&mut tree, id, Vector2::new(2.0, 2.0));

        let c = camera2d_config_from_node(&tree, id);
        assert!(approx_eq(c.limit_left, -100.0));
        assert!(approx_eq(c.limit_right, 100.0));
        assert!(c.position_smoothing_enabled);
        assert!(approx_eq(c.position_smoothing_speed, 8.0));
        assert!(c.drag_horizontal_enabled);
        assert!(!c.drag_vertical_enabled);
        assert!(approx_eq(c.drag_left_margin, 0.35));
        assert!(approx_vec(c.zoom, Vector2::new(2.0, 2.0)));
    }

    // -- NinePatchRect properties --------------------------------------------

    #[test]
    fn nine_patch_texture_roundtrip() {
        let mut tree = make_tree();
        let root = tree.root_id();
        let node = Node::new("Patch", "NinePatchRect");
        let id = tree.add_child(root, node).unwrap();

        set_nine_patch_texture(&mut tree, id, "res://panel.png");
        assert_eq!(get_texture_path(&tree, id), Some("res://panel.png".into()));
    }

    #[test]
    fn nine_patch_margins_default_zero() {
        let mut tree = make_tree();
        let root = tree.root_id();
        let node = Node::new("Patch", "NinePatchRect");
        let id = tree.add_child(root, node).unwrap();
        assert_eq!(get_nine_patch_margins(&tree, id), (0, 0, 0, 0));
    }

    #[test]
    fn nine_patch_margins_roundtrip() {
        let mut tree = make_tree();
        let root = tree.root_id();
        let node = Node::new("Patch", "NinePatchRect");
        let id = tree.add_child(root, node).unwrap();

        set_nine_patch_margins(&mut tree, id, 4, 8, 4, 8);
        assert_eq!(get_nine_patch_margins(&tree, id), (4, 8, 4, 8));
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
