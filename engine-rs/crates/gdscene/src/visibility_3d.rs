//! pat-0q2kx.198 — `VisibleOnScreenNotifier3D` / `VisibleOnScreenEnabler3D`
//! frustum culling.
//!
//! Every ingredient existed and none of them was connected to anything:
//! `camera3d::Frustum::intersects_aabb` and `camera3d::get_camera_frustum` were
//! written and tested, the notifier was a bare ClassDB entry with no `aabb` and
//! no signals, and the enabler was not registered at all. Nothing in
//! `main_loop` referenced either class, so a notifier in a played scene never
//! fired and an enabler never paused anything.
//!
//! Godot: `upstream/godot/scene/3d/visible_on_screen_notifier_3d.cpp` — the
//! notifier's `aabb` is tested against the active camera's frustum, emitting
//! `screen_entered` / `screen_exited` on the transition, and the Enabler
//! subclass drives its target's process mode.
//!
//! The 3D counterpart of [`crate::behavioral_2d::process_visibility_notifiers`].

use gdcore::math::Vector3;
use gdcore::math3d::Aabb;
use gdvariant::Variant;

use crate::node::{NodeId, ProcessMode};
use crate::scene_tree::SceneTree;
use crate::{camera3d, node3d};

/// Published each frame so a script can read what Godot exposes as
/// `VisibleOnScreenNotifier3D.is_on_screen()`.
pub const ON_SCREEN_PROP: &str = "is_on_screen";

/// Godot's default notifier box, mirrored by the ClassDB registration.
const DEFAULT_AABB: Aabb = Aabb {
    position: Vector3 {
        x: -1.0,
        y: -1.0,
        z: -1.0,
    },
    size: Vector3 {
        x: 2.0,
        y: 2.0,
        z: 2.0,
    },
};

fn is_notifier(class_name: &str) -> bool {
    matches!(
        class_name,
        "VisibleOnScreenNotifier3D" | "VisibleOnScreenEnabler3D"
    )
}

fn bool_prop(tree: &SceneTree, id: NodeId, key: &str, default: bool) -> bool {
    match tree.get_node(id).map(|n| n.get_property(key)) {
        Some(Variant::Bool(b)) => b,
        _ => default,
    }
}

/// The `current` [`Camera3D`](crate::camera3d), if the scene has one.
pub fn current_camera_3d(tree: &SceneTree) -> Option<NodeId> {
    tree.all_nodes_in_tree_order().into_iter().find(|id| {
        tree.get_node(*id).is_some_and(|n| {
            n.class_name() == "Camera3D" && matches!(n.get_property("current"), Variant::Bool(true))
        })
    })
}

/// The notifier's `aabb` in world space.
///
/// Built from all eight transformed corners rather than by transforming
/// `position` and `size` directly: under rotation the box's axes no longer
/// line up with the world's, and only the corner hull bounds the real extent.
pub fn notifier_world_aabb(tree: &SceneTree, id: NodeId) -> Aabb {
    let local = match tree.get_node(id).map(|n| n.get_property("aabb")) {
        Some(Variant::Aabb(a)) => a,
        _ => DEFAULT_AABB,
    };
    let xform = node3d::get_global_transform(tree, id);

    let first = xform.xform(local.get_endpoint(0));
    let mut min = first;
    let mut max = first;
    for i in 1..8u8 {
        let p = xform.xform(local.get_endpoint(i));
        min = Vector3::new(min.x.min(p.x), min.y.min(p.y), min.z.min(p.z));
        max = Vector3::new(max.x.max(p.x), max.y.max(p.y), max.z.max(p.z));
    }
    Aabb::new(min, max - min)
}

/// Tests every 3D notifier against the current camera's frustum, publishes
/// [`ON_SCREEN_PROP`], and emits `screen_entered` / `screen_exited` on the
/// frames the state flips.
///
/// `VisibleOnScreenEnabler3D` additionally drives its target's process mode —
/// the reason the subclass exists, so off-screen actors stop costing anything.
pub fn process_visibility_notifiers_3d(tree: &mut SceneTree) {
    // With no current camera nothing is being rendered, so nothing is on
    // screen. Godot's notifier likewise never reports entry without a camera —
    // treating "no camera" as "everything visible" would fire `screen_entered`
    // for the whole scene on the first frame of a cameraless level.
    let frustum = current_camera_3d(tree).map(|cam| {
        let viewport = tree.root_window().visible_size();
        camera3d::get_camera_frustum(
            tree,
            cam,
            viewport.x.max(1.0) as u32,
            viewport.y.max(1.0) as u32,
        )
    });

    struct Update {
        id: NodeId,
        on_screen: bool,
        entered: bool,
        exited: bool,
        enabler_target: Option<(NodeId, ProcessMode)>,
    }
    let mut updates: Vec<Update> = Vec::new();

    for id in tree.all_nodes_in_tree_order() {
        let Some(node) = tree.get_node(id) else {
            continue;
        };
        let class_name = node.class_name().to_string();
        if !is_notifier(&class_name) {
            continue;
        }

        let on_screen = frustum
            .as_ref()
            .is_some_and(|f| f.intersects_aabb(notifier_world_aabb(tree, id)));
        // A notifier starts off-screen, so one spawned inside the frustum
        // reports entering it on its first processed frame.
        let was_on_screen = bool_prop(tree, id, ON_SCREEN_PROP, false);

        let enabler_target = (class_name == "VisibleOnScreenEnabler3D")
            .then(|| {
                let path = match tree
                    .get_node(id)
                    .map(|n| n.get_property("enable_node_path"))
                {
                    Some(Variant::NodePath(p)) if !p.is_empty() => p.to_string(),
                    Some(Variant::String(s)) if !s.is_empty() => s,
                    // Godot's default target is the parent.
                    _ => "..".to_string(),
                };
                let target = if path.starts_with('/') {
                    tree.get_node_by_path(&path)
                } else {
                    tree.get_node_relative(id, &path)
                }?;
                let mode = if on_screen {
                    // ENABLE_MODE_INHERIT / ALWAYS / WHEN_PAUSED.
                    match tree.get_node(id).map(|n| n.get_property("enable_mode")) {
                        Some(Variant::Int(1)) => ProcessMode::Always,
                        Some(Variant::Int(2)) => ProcessMode::WhenPaused,
                        _ => ProcessMode::Inherit,
                    }
                } else {
                    ProcessMode::Disabled
                };
                Some((target, mode))
            })
            .flatten();

        updates.push(Update {
            id,
            on_screen,
            entered: on_screen && !was_on_screen,
            exited: !on_screen && was_on_screen,
            enabler_target,
        });
    }

    for update in updates {
        if let Some(node) = tree.get_node_mut(update.id) {
            node.set_property(ON_SCREEN_PROP, Variant::Bool(update.on_screen));
        }
        if let Some((target, mode)) = update.enabler_target {
            if let Some(node) = tree.get_node_mut(target) {
                node.set_process_mode(mode);
            }
        }
        // Emitted after the property is published so a handler that reads
        // `is_on_screen` sees the state it just moved into.
        if update.entered {
            tree.emit_signal(update.id, "screen_entered", &[]);
        }
        if update.exited {
            tree.emit_signal(update.id, "screen_exited", &[]);
        }
    }
}
