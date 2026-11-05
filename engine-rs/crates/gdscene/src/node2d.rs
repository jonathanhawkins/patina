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

/// Sets `Node2D.skew` in radians (pat-s3rrv.184).
pub fn set_skew(tree: &mut SceneTree, node_id: NodeId, radians: f32) {
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property("skew", Variant::Float(radians as f64));
    }
}

/// Reads the `"skew"` property in radians, defaulting to `0.0`.
///
/// ⚠️ Accepts `Int` as well as `Float`, unlike its neighbours here. That is
/// deliberate rather than accidental divergence: a `.tscn` may write a whole
/// number without a decimal point (`skew = 1`), which parses to `Variant::Int`,
/// and a Float-only reader would silently answer 0.0 — turning a type mismatch
/// into "no skew at all", the failure mode that is hardest to notice because it
/// looks exactly like the un-skewed default.
pub fn get_skew(tree: &SceneTree, node_id: NodeId) -> f32 {
    tree.get_node(node_id)
        .map(|n| match n.get_property("skew") {
            Variant::Float(f) => f as f32,
            Variant::Int(i) => i as f32,
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
///
/// A `CanvasLayer` contributes IDENTITY (pat-0q2kx.212). It is not a
/// `CanvasItem`: in Godot its `offset`/`rotation`/`scale` describe the *layer's
/// canvas*, which children are drawn relative to, and they are applied once by
/// [`crate::canvas_layer::layer_transform`]. This walk happens to read the same
/// property NAMES off any node it passes, so a layer's `rotation`/`scale` were
/// being counted a SECOND time here — a layer with `rotation = 0.5` rotated its
/// children twice, and one that also carried an explicit `transform` mixed the
/// authored matrix with a rotation the authored matrix was supposed to replace.
///
/// The doc on `layer_transform` asserted this walk "skips straight past" a
/// CanvasLayer. It did not — nothing here ever looked at the class. This is that
/// claim made true.
pub fn get_local_transform(tree: &SceneTree, node_id: NodeId) -> Transform2D {
    if tree
        .get_node(node_id)
        .is_some_and(|n| n.is_class("CanvasLayer"))
    {
        return Transform2D::IDENTITY;
    }
    let pos = get_position(tree, node_id);
    let rot = get_rotation(tree, node_id);
    let scl = get_scale(tree, node_id);
    let skew = get_skew(tree, node_id);

    // pat-s3rrv.184: Godot builds this as `Transform2D(rotation, scale, skew,
    // position)` (`scene/2d/node_2d.cpp::_update_transform`), where skew shears
    // the Y basis relative to X. `skew` was never read here, so a skewed Node2D
    // round-tripped through the `.tscn` and the inspector and drew unskewed.
    //
    // 🔑 Behaviour-preserving at `skew == 0`, which matters because this
    // function is the whole 2D transform path — render, physics and input.
    // `translated(pos) * rotated(rot) * scaled(scl)` expands to
    // `x = (cos*sx, sin*sx)`, `y = (-sin*sy, cos*sy)`, `origin = pos`, and
    // `from_rotation_scale_skew_origin(rot, scl, 0.0, pos)` produces exactly
    // those same three, term for term. So every existing scene composes
    // identically and only a non-zero skew changes anything.
    Transform2D::from_rotation_scale_skew_origin(rot, scl, skew, pos)
}

/// Computes the global transform by walking the parent chain and multiplying
/// local transforms from the root downward.
/// Sets `CanvasItem.top_level` (pat-s3rrv.22).
///
/// Stored as the plain `top_level` node property, which is the same key the
/// `.tscn` parser writes — so a scene that declares `top_level = true` and a
/// script that sets it at runtime end up in the same place.
pub fn set_top_level(tree: &mut SceneTree, node_id: NodeId, top_level: bool) {
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property("top_level", Variant::Bool(top_level));
    }
}

/// Reads `CanvasItem.top_level`, defaulting to `false` (pat-s3rrv.22).
///
/// The 3D side keeps the equivalent in `node3d`, but the flag is a plain node
/// property with the same name on both, and a 2D module reaching into `node3d`
/// for it would read as a layering accident rather than a shared convention.
pub fn is_top_level(tree: &SceneTree, node_id: NodeId) -> bool {
    tree.get_node(node_id)
        .map(|n| matches!(n.get_property("top_level"), Variant::Bool(true)))
        .unwrap_or(false)
}

pub fn get_global_transform(tree: &SceneTree, node_id: NodeId) -> Transform2D {
    // Collect ancestor chain (node -> parent -> ... -> root).
    let mut chain = Vec::new();
    let mut current = node_id;
    loop {
        chain.push(current);
        // pat-s3rrv.22: `top_level` ends the walk. Godot implements this in
        // `CanvasItem::get_parent_item`, which returns null for a top-level
        // item, so `get_global_transform` falls back to the item's OWN
        // transform (canvas_item.cpp). The node keeps its own local transform —
        // it is the ancestors above it that stop contributing — so the break
        // comes AFTER the push.
        //
        // ⚠️ This is fixed in `get_global_transform` itself rather than in a
        // parallel `_top_level` variant, which is how the 3D side did it: a
        // second function leaves every existing caller silently on the wrong
        // one, and here the callers are the whole 2D render path
        // (`viewport_2d_render::collect_canvas_items`), physics and input.
        if is_top_level(tree, current) {
            break;
        }
        match tree.get_node(current).and_then(|n| n.parent()) {
            // A `SubViewport` is its own canvas: what sits in it is placed in
            // the sub-viewport's coordinates, never offset by the container
            // that displays it (pat-8mb8y). Godot's `get_parent_item()` is
            // null there because a Viewport is not a CanvasItem.
            Some(parent_id) if crate::behavioral_2d::is_sub_viewport(tree, parent_id) => break,
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
    // pat-s3rrv.22: a `top_level` node's global transform IS its local one, so
    // there is no parent transform to invert — using the parent's here would
    // write a local position that lands the node somewhere else entirely.
    let parent_global = if is_top_level(tree, node_id) {
        Transform2D::IDENTITY
    } else {
        tree.get_node(node_id)
            .and_then(|n| n.parent())
            .map(|pid| get_global_transform(tree, pid))
            .unwrap_or(Transform2D::IDENTITY)
    };

    let local_pos = parent_global.affine_inverse().xform(pos);
    set_position(tree, node_id, local_pos);
}

/// The node's global position — the origin of its global transform.
pub fn get_global_position(tree: &SceneTree, node_id: NodeId) -> Vector2 {
    get_global_transform(tree, node_id).xform(Vector2::ZERO)
}

/// The node's global rotation in radians.
///
/// In 2D this is the sum of the local rotations along the parent chain, which
/// avoids decomposing the global matrix — `Transform2D` exposes no rotation
/// extractor, and pulling one out of a matrix that also carries scale is where
/// sign and shear errors creep in (pat-0q2kx.31).
pub fn get_global_rotation(tree: &SceneTree, node_id: NodeId) -> f32 {
    let mut rotation = 0.0;
    let mut current = Some(node_id);
    while let Some(id) = current {
        rotation += get_rotation(tree, id);
        // pat-s3rrv.22: stop at a `top_level` node, for the same reason the
        // transform walk does. Summing through it would leave the rotation
        // disagreeing with the transform this function is supposed to describe.
        if is_top_level(tree, id) {
            break;
        }
        current = tree.get_node(id).and_then(|n| n.parent());
    }
    rotation
}

/// The node's global scale — the component-wise product along the parent chain.
pub fn get_global_scale(tree: &SceneTree, node_id: NodeId) -> Vector2 {
    let mut scale = Vector2::new(1.0, 1.0);
    let mut current = Some(node_id);
    while let Some(id) = current {
        let s = get_scale(tree, id);
        scale = Vector2::new(scale.x * s.x, scale.y * s.y);
        // pat-s3rrv.184: stop at a `top_level` node, exactly as
        // `get_global_rotation` above and `get_global_transform` already do.
        // pat-s3rrv.22 swept the rotation and position helpers and missed this
        // one, so a detached node reported its ancestors' scale while its
        // transform correctly ignored them — the two answers disagreed about
        // the same node.
        if is_top_level(tree, id) {
            break;
        }
        current = tree.get_node(id).and_then(|n| n.parent());
    }
    scale
}

/// Sets the node's local rotation such that its global rotation equals
/// `radians`, by subtracting the parent chain's accumulated rotation.
pub fn set_global_rotation(tree: &mut SceneTree, node_id: NodeId, radians: f32) {
    let parent_rotation = tree
        .get_node(node_id)
        .and_then(|n| n.parent())
        .map(|pid| get_global_rotation(tree, pid))
        .unwrap_or(0.0);
    set_rotation(tree, node_id, radians - parent_rotation);
}

/// Whether `node_id` is a `Node2D` (or a subclass).
fn is_node2d(tree: &SceneTree, node_id: NodeId) -> bool {
    tree.get_node(node_id)
        .is_some_and(|n| gdobject::class_db::is_parent_class(n.class_name(), "Node2D"))
}

/// Node2D's COMPUTED properties, for a script reading `global_position` and
/// friends. None of them is stored on the node, so the accessor's property-bag
/// read answered `Nil` and the interpreter reported `global_position` as an
/// undefined variable — inside any 2D script. `None` for a node that is not a
/// Node2D or a property this does not compute.
pub fn get_derived_property(tree: &SceneTree, node_id: NodeId, prop: &str) -> Option<Variant> {
    if !is_node2d(tree, node_id) {
        return None;
    }
    Some(match prop {
        "global_position" => Variant::Vector2(get_global_position(tree, node_id)),
        "global_rotation" => Variant::Float(get_global_rotation(tree, node_id) as f64),
        "global_rotation_degrees" => {
            Variant::Float((get_global_rotation(tree, node_id) as f64).to_degrees())
        }
        "global_scale" => Variant::Vector2(get_global_scale(tree, node_id)),
        "global_transform" => Variant::Transform2D(get_global_transform(tree, node_id)),
        _ => return None,
    })
}

/// The write half of [`get_derived_property`]: converts a global value into the
/// local one the node stores. `false` when this does not own the property.
pub fn set_derived_property(
    tree: &mut SceneTree,
    node_id: NodeId,
    prop: &str,
    value: &Variant,
) -> bool {
    if !is_node2d(tree, node_id) {
        return false;
    }
    let as_f32 = |v: &Variant| match v {
        Variant::Float(f) => Some(*f as f32),
        Variant::Int(i) => Some(*i as f32),
        _ => None,
    };
    // The parent whose global transform composes this node's — none for a
    // `top_level` node, which ignores its ancestors.
    let parent = tree
        .get_node(node_id)
        .and_then(|n| n.parent())
        .filter(|_| !is_top_level(tree, node_id));
    match (prop, value) {
        ("global_position", Variant::Vector2(v)) => set_global_position(tree, node_id, *v),
        ("global_rotation", v) if as_f32(v).is_some() => {
            set_global_rotation(tree, node_id, as_f32(v).unwrap_or(0.0))
        }
        ("global_rotation_degrees", v) if as_f32(v).is_some() => {
            set_global_rotation(tree, node_id, as_f32(v).unwrap_or(0.0).to_radians())
        }
        // Godot's `Node2D::set_global_scale`: the local scale that, composed
        // with the parent chain's, gives the requested one.
        ("global_scale", Variant::Vector2(v)) => {
            let parent_scale = parent
                .map(|p| get_global_scale(tree, p))
                .unwrap_or(Vector2::new(1.0, 1.0));
            let div = |a: f32, b: f32| if b == 0.0 { a } else { a / b };
            set_scale(
                tree,
                node_id,
                Vector2::new(div(v.x, parent_scale.x), div(v.y, parent_scale.y)),
            );
        }
        // Godot's `set_global_transform`: `parent_global⁻¹ * t`, decomposed into
        // the position/rotation/scale/skew the node stores.
        ("global_transform", Variant::Transform2D(t)) => {
            let local = match parent {
                Some(p) => get_global_transform(tree, p).affine_inverse() * *t,
                None => *t,
            };
            set_position(tree, node_id, local.origin);
            set_rotation(tree, node_id, local.get_rotation());
            set_scale(tree, node_id, local.get_scale());
            set_skew(tree, node_id, local.get_skew());
        }
        _ => return false,
    }
    true
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
/// Cache-key scheme for a generated (baked) texture that has no file behind it.
///
/// Deliberately not a `res://` path: nothing on disk backs it, and a loader that
/// tried would fail and cache the failure. The renderer's texture cache is
/// pre-populated with these entries before a frame is rasterized.
pub const NOISE_TEXTURE_SCHEME: &str = "noise://";

pub fn get_texture_path(tree: &SceneTree, node_id: NodeId) -> Option<String> {
    let node = tree.get_node(node_id)?;
    // ⚠️ A `.tscn` writes `texture = ExtResource("1_icon")` and the loader leaves
    // that ref in place, parking the real path in an `__extres__texture`
    // companion (`packed_scene.rs:809`) so the scene round-trips byte-for-byte.
    // The inspector's resource picker writes the same pair. Reading `texture`
    // alone therefore handed every consumer the literal string
    // `ExtResource("1_icon")` **as if it were a path** — the editor's
    // `TextureCache` fed it to `resolve_res_path`, got `<root>/ExtResource("…")`,
    // failed to load, and fell back to the placeholder. Every sprite that got
    // its texture from a scene file or from the picker rendered untextured
    // (pat-0q2kx.13).
    // A NoiseTexture2D has no file behind it — its pixels are GENERATED — so
    // it resolves to a synthetic cache key rather than a path (pat-s3rrv.208).
    //
    // 🔑 Returning `None` here would be the quiet failure: the sprite would
    // simply draw nothing, which is indistinguishable from a texture that has
    // not finished loading. The key is stable per node so the bake is cached
    // across frames instead of being recomputed for every draw.
    if let Variant::String(class) = node.get_property("__noise_tex__class") {
        if class == "NoiseTexture2D" {
            return Some(format!("{NOISE_TEXTURE_SCHEME}{}", node_id.raw()));
        }
    }

    // A CanvasTexture bundles three channels, and the one that gets DRAWN is
    // `diffuse_texture` (pat-s3rrv.203). It is checked before `__extres__` so a
    // Sprite2D whose `texture` is a CanvasTexture draws its diffuse map rather
    // than nothing: the `texture` property itself holds `SubResource:<id>`,
    // which names a resource and not a file.
    if let Variant::String(path) = node.get_property("__canvas_texture__diffuse_texture") {
        if !path.is_empty() {
            return Some(path);
        }
    }
    if let Variant::String(path) = node.get_property("__extres__texture") {
        if !path.is_empty() {
            return Some(path);
        }
    }
    match node.get_property("texture") {
        // A bare path (what `set_texture_path` writes, and what a hand-built
        // node carries). An unresolved ref is NOT a path — returning it would
        // reintroduce the bug above for a node whose `[ext_resource]` header is
        // missing, so it yields `None` and the sprite draws no texture rather
        // than chasing a file named `ExtResource("…")`.
        Variant::String(s) if !s.starts_with("ExtResource(") => Some(s),
        _ => None,
    }
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

/// `Camera2D.AnchorMode`: the camera's position is the view's top-left corner.
pub const ANCHOR_MODE_FIXED_TOP_LEFT: i64 = 0;
/// `Camera2D.AnchorMode`: the camera's position is the view's centre (default).
pub const ANCHOR_MODE_DRAG_CENTER: i64 = 1;

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
    /// `ANCHOR_MODE_FIXED_TOP_LEFT` (0) or `ANCHOR_MODE_DRAG_CENTER` (1).
    /// Decides whether the camera's position is the view's top-left or its
    /// centre; Godot's default is DRAG_CENTER.
    pub anchor_mode: i64,
    /// Gates the limit clamp entirely. With this false a camera is free to
    /// leave its configured limits (Godot `Camera2D.limit_enabled`).
    pub limit_enabled: bool,
    /// Eases into the limits instead of clamping hard. Godot applies the clamp
    /// BEFORE smoothing when this is set, and after it when it is not.
    pub limit_smoothed: bool,
    /// When false the camera's global rotation is folded into the view, so the
    /// whole world counter-rotates. Godot's default is `true` (upright view).
    pub ignore_rotation: bool,
    /// Eases the view's rotation toward the camera's, mirroring position
    /// smoothing. Only consulted when `ignore_rotation` is false.
    pub rotation_smoothing_enabled: bool,
    pub rotation_smoothing_speed: f32,
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
            anchor_mode: ANCHOR_MODE_DRAG_CENTER,
            limit_enabled: true,
            limit_smoothed: false,
            ignore_rotation: true,
            rotation_smoothing_enabled: false,
            rotation_smoothing_speed: 5.0,
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
    if !config.limit_enabled {
        return center;
    }
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
    // Godot resolves the scroll against the view's CENTRE. Under
    // `ANCHOR_MODE_FIXED_TOP_LEFT` the camera's position is the view's top-left
    // corner instead (`screen_offset` is zero in that branch,
    // camera_2d.cpp:162), so the centre sits half a view further along each
    // axis. Converting up front lets the rest of the pipeline stay in one space.
    let fixed_top_left = config.anchor_mode == ANCHOR_MODE_FIXED_TOP_LEFT;
    let target = if fixed_top_left {
        target + half_view
    } else {
        target
    };
    // The drag dead-zone is DRAG_CENTER-only: the FIXED_TOP_LEFT branch takes
    // the target verbatim (camera_2d.cpp:158-160).
    let dragged = if fixed_top_left {
        target
    } else {
        apply_drag_margins(config, current, target, half_view)
    };
    // `limit_smoothed` moves the clamp to BEFORE the ease, which is what lets a
    // smoothed camera glide into its limit instead of stopping dead on it.
    // Godot clamps twice under exactly these two conditions (camera_2d.cpp:165
    // and :226); `clamp_camera_to_limits` is itself a no-op when the limits are
    // disabled, so `limit_enabled` need not be repeated here.
    let limited = if config.limit_smoothed {
        clamp_camera_to_limits(config, dragged, half_view)
    } else {
        dragged
    };
    let smoothed = smooth_camera_position(config, current, limited, dt);
    if config.position_smoothing_enabled && config.limit_smoothed {
        smoothed
    } else {
        clamp_camera_to_limits(config, smoothed, half_view)
    }
}

/// The rotation Godot folds into the canvas transform (`Camera2D::get_screen_rotation`).
///
/// Zero while `ignore_rotation` is set — the default, which keeps the view
/// upright however the camera node is rotated. Otherwise the camera's global
/// rotation, eased from `current` by `rotation_smoothing_speed * dt` when
/// rotation smoothing is on (camera_2d.cpp:214-221). The ease takes the short
/// way round, so crossing +/-PI does not spin the world the long way.
pub fn smooth_camera_rotation(config: &Camera2DConfig, current: f32, target: f32, dt: f32) -> f32 {
    if config.ignore_rotation {
        return 0.0;
    }
    if config.rotation_smoothing_enabled && dt > 0.0 {
        let step = (config.rotation_smoothing_speed * dt).clamp(0.0, 1.0);
        gdcore::math::lerp_angle(current, target, step)
    } else {
        target
    }
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

fn get_i64_prop(tree: &SceneTree, node_id: NodeId, key: &str, default: i64) -> i64 {
    tree.get_node(node_id)
        .map(|n| match n.get_property(key) {
            Variant::Int(i) => i,
            Variant::Float(f) => f as i64,
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
pub fn set_position_smoothing(tree: &mut SceneTree, node_id: NodeId, enabled: bool, speed: f32) {
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property("position_smoothing_enabled", Variant::Bool(enabled));
        node.set_property("position_smoothing_speed", Variant::Float(speed as f64));
    }
}

/// Sets a Camera2D drag margin fraction (`side` = left/right/top/bottom).
pub fn set_drag_margin(tree: &mut SceneTree, node_id: NodeId, side: &str, fraction: f32) {
    if let Some(node) = tree.get_node_mut(node_id) {
        node.set_property(
            &format!("drag_{side}_margin"),
            Variant::Float(fraction as f64),
        );
    }
}

/// Enables horizontal/vertical drag margins for a Camera2D node.
pub fn set_drag_enabled(tree: &mut SceneTree, node_id: NodeId, horizontal: bool, vertical: bool) {
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
        drag_bottom_margin: get_f32_prop(tree, node_id, "drag_bottom_margin", d.drag_bottom_margin),
        zoom: get_zoom(tree, node_id),
        anchor_mode: get_i64_prop(tree, node_id, "anchor_mode", d.anchor_mode),
        limit_enabled: get_bool_prop(tree, node_id, "limit_enabled", d.limit_enabled),
        limit_smoothed: get_bool_prop(tree, node_id, "limit_smoothed", d.limit_smoothed),
        ignore_rotation: get_bool_prop(tree, node_id, "ignore_rotation", d.ignore_rotation),
        rotation_smoothing_enabled: get_bool_prop(
            tree,
            node_id,
            "rotation_smoothing_enabled",
            d.rotation_smoothing_enabled,
        ),
        rotation_smoothing_speed: get_f32_prop(
            tree,
            node_id,
            "rotation_smoothing_speed",
            d.rotation_smoothing_speed,
        ),
    }
}

/// Reads the Camera2D `"offset"` property — a world-space nudge of the view
/// center (Godot default `Vector2::ZERO`).
pub fn get_camera_offset(tree: &SceneTree, node_id: NodeId) -> Vector2 {
    tree.get_node(node_id)
        .map(|n| match n.get_property("offset") {
            Variant::Vector2(v) => v,
            _ => Vector2::ZERO,
        })
        .unwrap_or(Vector2::ZERO)
}

/// Returns the Camera2D's screen-center world position (Godot
/// `Camera2D.get_screen_center_position`): the camera's global position plus
/// its `offset`, clamped so the visible rect stays within the configured
/// limits. This is the point the viewport centers on.
///
/// Drag margins and position smoothing shape only the per-frame easing toward
/// this target (they need the running scroll state), so at rest the screen
/// center equals this clamped target — which is what a freshly-current camera
/// reports before any frames advance.
pub fn get_screen_center_position(
    tree: &SceneTree,
    node_id: NodeId,
    viewport_size: Vector2,
) -> Vector2 {
    let config = camera2d_config_from_node(tree, node_id);
    // 🔑 `offset` is added AFTER the limits, not to the target: Godot clamps
    // `screen_rect` to the limits and only then does
    // `screen_rect.position += offset` (camera_2d.cpp:253). The 2D Platformer's
    // camera (offset (0, 50), limit_bottom 690) shows 50 px more below its
    // bottom limit than a clamp of `target + offset` would allow.
    let target = get_global_transform(tree, node_id).origin;
    let half_view = camera2d_half_view(viewport_size, config.zoom);
    clamp_camera_to_limits(&config, target, half_view) + get_camera_offset(tree, node_id)
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
        assert!(
            approx_vec(clamped, Vector2::new(400.0, 300.0)),
            "{clamped:?}"
        );
        // A center past the bottom-right is pulled back.
        let clamped2 = clamp_camera_to_limits(&cfg, Vector2::new(5000.0, 5000.0), half_view);
        assert!(
            approx_vec(clamped2, Vector2::new(600.0, 700.0)),
            "{clamped2:?}"
        );
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
        assert!(
            approx_eq(outside.x, 120.0),
            "dragged to margin: {outside:?}"
        );
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

    #[test]
    fn camera2d_screen_center_position_clamps_to_limit() {
        // Viewport 200x100 -> half-view 100x50 at zoom 1. A camera far left of
        // its limit_left=0 must report a screen center clamped so the visible
        // rect's left edge sits at the limit: center.x = 0 + half_view.x = 100.
        let mut tree = make_tree();
        let root = tree.root_id();
        let cam = tree.add_child(root, Node::new("Cam", "Camera2D")).unwrap();
        set_camera_current(&mut tree, cam, true);
        set_camera_limit(&mut tree, cam, "left", 0);
        set_position(&mut tree, cam, Vector2::new(-500.0, 30.0));

        let center = get_screen_center_position(&tree, cam, Vector2::new(200.0, 100.0));
        assert!(
            approx_eq(center.x, 100.0),
            "x clamps to limit_left + half_view (0 + 100), got {}",
            center.x
        );
        // Y is within the (default) vertical limits, so it is unclamped.
        assert!(
            approx_eq(center.y, 30.0),
            "y is within limits and unclamped, got {}",
            center.y
        );

        // Within limits: the screen center follows the camera position.
        set_position(&mut tree, cam, Vector2::new(400.0, 20.0));
        let c2 = get_screen_center_position(&tree, cam, Vector2::new(200.0, 100.0));
        assert!(
            approx_vec(c2, Vector2::new(400.0, 20.0)),
            "unclamped center follows the camera, got ({}, {})",
            c2.x,
            c2.y
        );
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

    // -- CanvasLayer ancestors (pat-0q2kx.212) ------------------------------

    /// A `CanvasLayer` must contribute nothing to the 2D walk.
    ///
    /// It is not a `CanvasItem`; its `offset`/`rotation`/`scale` describe the
    /// layer's own canvas and are applied once by
    /// `canvas_layer::layer_transform`. This walk reads property NAMES off
    /// whatever node it passes, so before the fix a layer's rotation was counted
    /// a second time here and every child of a rotated layer was rotated twice.
    #[test]
    fn canvas_layer_contributes_identity_to_the_walk() {
        let mut tree = make_tree();
        let root = tree.root_id();
        let mut layer = Node::new("HUD", "CanvasLayer");
        // The same property names a Node2D would answer to. A CanvasLayer really
        // does carry `rotation`/`scale` — that is exactly why the walk picked
        // them up.
        layer.set_property("rotation", Variant::Float(FRAC_PI_2 as f64));
        layer.set_property("scale", Variant::Vector2(Vector2::new(3.0, 3.0)));
        let layer_id = tree.add_child(root, layer).unwrap();

        assert_eq!(
            get_local_transform(&tree, layer_id),
            Transform2D::IDENTITY,
            "a CanvasLayer's own rotation/scale must not enter the 2D walk"
        );
    }

    /// The consequence that actually bit: a child of a rotated layer keeps its
    /// authored local position, instead of being rotated by the layer twice.
    #[test]
    fn a_child_of_a_rotated_canvas_layer_is_not_pre_rotated() {
        let mut tree = make_tree();
        let root = tree.root_id();
        let mut layer = Node::new("HUD", "CanvasLayer");
        layer.set_property("rotation", Variant::Float(FRAC_PI_2 as f64));
        let layer_id = tree.add_child(root, layer).unwrap();

        let child = Node::new("Bar", "Node2D");
        let child_id = tree.add_child(layer_id, child).unwrap();
        set_position(&mut tree, child_id, Vector2::new(10.0, 0.0));

        let origin = get_global_transform(&tree, child_id).origin;
        assert!(
            approx_vec(origin, Vector2::new(10.0, 0.0)),
            "the layer's rotation belongs to layer_transform, not to this walk — \
             a quarter turn applied here would put the child near (0, 10); got \
             {origin:?}"
        );
    }
}

/// Answers `Node2D`'s global-transform script methods, or `None` for any other
/// node (pat-s3rrv.184).
///
/// Runs ahead of the generic node-method table in `scripting::call_node_method`
/// and declines every non-Node2D node, the contract
/// `tilemap::call_tilemap_method` follows.
///
/// 🔑 **The 2D side had no script method surface at all.** `scripting.rs` grew a
/// Node3D block (`look_at`, `rotate`, `to_global`, `to_local`) and nothing
/// equivalent for 2D, so `sprite.get_global_position()` raised
/// `UndefinedFunction` — the value was computed correctly by
/// [`get_global_position`] and simply unreachable from a game. Registration
/// alone would not fix it: a ClassDB entry makes a method *reflectable*, but a
/// call still has to land on an arm.
///
/// ⚠️ The setter ships with the getter deliberately. A getter-only surface reads
/// as complete and then breaks `node.global_position += delta`, which needs both
/// halves — and that failure surfaces at the caller, not here.
pub fn call_node2d_method(
    tree: &mut SceneTree,
    node_id: NodeId,
    method: &str,
    args: &[Variant],
) -> Option<Variant> {
    // `is_class` rather than a name equality test: every 2D node worth asking
    // (Sprite2D, CharacterBody2D, a scripted subclass) inherits Node2D, and a
    // `class_name() == "Node2D"` check would answer only for the bare base.
    if !tree.get_node(node_id)?.is_class("Node2D") {
        return None;
    }
    match method {
        "get_global_position" => Some(Variant::Vector2(get_global_position(tree, node_id))),
        "set_global_position" => {
            let target = match args.first() {
                Some(Variant::Vector2(v)) => *v,
                _ => return None,
            };
            set_global_position(tree, node_id, target);
            Some(Variant::Nil)
        }
        _ => None,
    }
}
