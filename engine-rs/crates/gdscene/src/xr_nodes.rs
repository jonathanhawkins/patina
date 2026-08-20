//! Drives `XRNode3D` and its subclasses from `XRServer`'s trackers each frame
//! (pat-s3rrv.219).
//!
//! Godot's `XRNode3D` (upstream `scene/3d/xr/xr_nodes.cpp`) binds a tracker by
//! name on entering the tree, connects its `pose_changed`, and writes the named
//! pose's transform onto its own local transform, scaled by `XRServer`'s world
//! scale, toggling visibility when `show_when_tracked` is set.
//!
//! Patina had all five XR node classes registered in ClassDB and **no
//! behaviour at all** — `rg 'XR' crates/gdscene/src` matched three comment
//! lines. This module is the missing frame tick.
//!
//! ⚠️ Godot is signal-driven (`_pose_changed` fires when the runtime reports a
//! new pose); this is poll-driven from the frame tick. The observable result is
//! the same because a pose can only change between frames, and polling avoids a
//! second cross-crate signal path. The tracking-state *edge* is still an edge:
//! `tracking_changed` fires only on a transition, never per frame.

use crate::node::NodeId;
use crate::node3d;
use crate::scene_tree::SceneTree;
use gdcore::math3d::Transform3D;
use gdvariant::Variant;

/// Where the previous tracking state is parked between frames.
///
/// An internal `__`-prefixed key so it never shows up in an inspector dump or
/// an oracle comparison, matching how the rest of the runtime hides its
/// bookkeeping on nodes.
const TRACKING_KEY: &str = "__xr_tracking__";

/// Godot's default pose name — `XRNode3D.pose` defaults to `&"default"`.
const DEFAULT_POSE: &str = "default";

/// Reads a string property, tolerating `StringName` as well as `String`.
fn string_prop(tree: &SceneTree, id: NodeId, key: &str) -> Option<String> {
    match tree.get_node(id)?.get_property(key) {
        Variant::String(s) if !s.is_empty() => Some(s),
        Variant::StringName(s) => {
            // `StringName` has no `Display`; `as_str()` is the house idiom.
            let s = s.as_str().to_string();
            if s.is_empty() {
                None
            } else {
                Some(s)
            }
        }
        _ => None,
    }
}

fn bool_prop(tree: &SceneTree, id: NodeId, key: &str) -> bool {
    matches!(
        tree.get_node(id).map(|n| n.get_property(key)),
        Some(Variant::Bool(true))
    )
}

/// The world scale in force for `id`, from the nearest `XROrigin3D` ancestor.
///
/// Godot keeps world scale on `XRServer` and `XROrigin3D.world_scale` writes
/// it; walking to the ancestor gives the same answer for any sane scene and
/// keeps the value where the `.tscn` author put it. A non-positive scale is
/// ignored rather than collapsing the scene to a point.
fn world_scale_for(tree: &SceneTree, id: NodeId) -> f32 {
    let mut current = tree.get_node(id).and_then(|n| n.parent());
    while let Some(node_id) = current {
        let Some(node) = tree.get_node(node_id) else {
            break;
        };
        if node.is_class("XROrigin3D") {
            if let Some(scale) = match node.get_property("world_scale") {
                Variant::Float(v) => Some(v as f32),
                Variant::Int(v) => Some(v as f32),
                _ => None,
            } {
                if scale > 0.0 {
                    return scale;
                }
            }
            return 1.0;
        }
        current = node.parent();
    }
    1.0
}

/// Whether `id` currently has tracking data — a pose under its tracker name.
pub fn has_tracking_data(tree: &SceneTree, id: NodeId) -> bool {
    let Some(tracker) = string_prop(tree, id, "tracker") else {
        return false;
    };
    let pose = string_prop(tree, id, "pose").unwrap_or_else(|| DEFAULT_POSE.to_string());
    gdscript_interop::xr_tracker_pose(&tracker, &pose).is_some()
}

/// Godot `XRNode3D.get_is_active()` — the tracker exists *and* is posed.
pub fn is_active(tree: &SceneTree, id: NodeId) -> bool {
    let Some(tracker) = string_prop(tree, id, "tracker") else {
        return false;
    };
    gdscript_interop::xr_tracker_exists(&tracker) && has_tracking_data(tree, id)
}

/// Godot `XRController3D.is_button_pressed(name)`.
///
/// Reads the bound tracker's input map. A missing tracker, a missing input, or
/// a non-boolean value is `false` — Godot's answer for anything that is not a
/// pressed button.
pub fn is_button_pressed(tree: &SceneTree, id: NodeId, button: &str) -> bool {
    let Some(tracker) = string_prop(tree, id, "tracker") else {
        return false;
    };
    matches!(
        gdscript_interop::xr_tracker_input(&tracker, button),
        Some(Variant::Bool(true))
    )
}

/// Godot `XRController3D.get_input(name)`.
pub fn get_input(tree: &SceneTree, id: NodeId, name: &str) -> Variant {
    let Some(tracker) = string_prop(tree, id, "tracker") else {
        return Variant::Nil;
    };
    gdscript_interop::xr_tracker_input(&tracker, name).unwrap_or(Variant::Nil)
}

/// Advances every `XRNode3D` in the tree from its tracker's pose.
///
/// Returns the nodes whose tracking state *changed* this frame, so the caller
/// can emit `tracking_changed` — an edge, not a level. Emitting from here would
/// mean running script inside the walk while the tree is mutably borrowed.
pub fn update_xr_nodes(tree: &mut SceneTree) -> Vec<NodeId> {
    let mut changed = Vec::new();

    for id in tree.all_nodes_in_tree_order() {
        let Some(node) = tree.get_node(id) else {
            continue;
        };
        if !node.is_class("XRNode3D") {
            continue;
        }

        let tracker = string_prop(tree, id, "tracker");
        let pose_name = string_prop(tree, id, "pose").unwrap_or_else(|| DEFAULT_POSE.to_string());
        let pose = tracker
            .as_deref()
            .and_then(|t| gdscript_interop::xr_tracker_pose(t, &pose_name));
        let tracking = pose.is_some();

        if let Some(pose) = pose {
            // Godot scales the pose's *position* by world scale, not its basis:
            // a 2x world makes the controller twice as far from the origin, not
            // twice as large.
            let scale = world_scale_for(tree, id);
            let local = Transform3D {
                basis: pose.transform.basis,
                origin: pose.transform.origin * scale,
            };
            // Godot writes the pose as the node's LOCAL transform, so the
            // XROrigin3D's own placement still applies. Only a global setter is
            // public here, so compose the parent's global back in.
            // Parent id resolved to an owned value FIRST, so no immutable
            // borrow of `tree` is still live when the mutable setter runs.
            let parent = tree.get_node(id).and_then(|n| n.parent());
            let parent_global = match parent {
                Some(p) => node3d::get_global_transform(tree, p),
                None => Transform3D::IDENTITY,
            };
            node3d::set_global_transform(tree, id, parent_global * local);
        }

        // `show_when_tracked` hides the node until a pose arrives. Nodes without
        // the flag are left alone — Godot does not force them visible.
        if bool_prop(tree, id, "show_when_tracked") {
            node3d::set_visible(tree, id, tracking);
        }

        let previous = matches!(
            tree.get_node(id).map(|n| n.get_property(TRACKING_KEY)),
            Some(Variant::Bool(true))
        );
        if previous != tracking {
            if let Some(node) = tree.get_node_mut(id) {
                node.set_property(TRACKING_KEY, Variant::Bool(tracking));
            }
            changed.push(id);
        }
    }

    changed
}

/// Answers `XRNode3D`/`XRController3D` instance methods for a script.
///
/// Declines (`None`) every node that is not an `XRNode3D`, so the generic node
/// method table behind it stays reachable — the same contract the
/// NavigationAgent and ShapeCast handlers use.
///
/// ⚠️ Registering these in ClassDB is **not** enough on its own: a name that
/// `has_method` admits but nothing dispatches answers `Nil` silently, which is
/// worse than an unknown method. This is the dispatch half.
pub fn call_xr_node_method(
    tree: &SceneTree,
    id: NodeId,
    method: &str,
    args: &[Variant],
) -> Option<Variant> {
    if !tree.get_node(id)?.is_class("XRNode3D") {
        return None;
    }
    let name_arg = || match args.first() {
        Some(Variant::String(s)) => s.clone(),
        Some(Variant::StringName(s)) => s.as_str().to_string(),
        _ => String::new(),
    };

    Some(match method {
        "get_has_tracking_data" => Variant::Bool(has_tracking_data(tree, id)),
        "get_is_active" => Variant::Bool(is_active(tree, id)),
        "is_button_pressed" => Variant::Bool(is_button_pressed(tree, id, &name_arg())),
        "get_input" => get_input(tree, id, &name_arg()),
        // Godot's `get_float`/`get_vector2` are typed reads of the same input
        // map; a value of the wrong type is the type's zero, not an error.
        "get_float" => Variant::Float(match get_input(tree, id, &name_arg()) {
            Variant::Float(v) => v,
            Variant::Int(v) => v as f64,
            Variant::Bool(b) => {
                if b {
                    1.0
                } else {
                    0.0
                }
            }
            _ => 0.0,
        }),
        "get_vector2" => match get_input(tree, id, &name_arg()) {
            Variant::Vector2(v) => Variant::Vector2(v),
            _ => Variant::Vector2(gdcore::math::Vector2::new(0.0, 0.0)),
        },
        "get_tracker" => Variant::String(string_prop(tree, id, "tracker").unwrap_or_default()),
        "get_pose_name" => Variant::String(
            string_prop(tree, id, "pose").unwrap_or_else(|| DEFAULT_POSE.to_string()),
        ),
        _ => return None,
    })
}
