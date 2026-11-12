//! Runtime parallax scrolling (pat-s3rrv.86).
//!
//! `gdserver2d::parallax` has implemented `ParallaxBackground`/`ParallaxLayer`
//! offsets — with its own test file — since it was written, and had **no
//! production caller**: `rg 'parallax::'` found only the `pub mod` line. So a
//! `.tscn` containing a ParallaxBackground loaded, kept every property, and
//! scrolled not one pixel. This module is the missing consumer; the maths is
//! called, not re-derived.

use gdvariant::Variant;

use gdcore::math::{Transform2D, Vector2};

use crate::node::NodeId;
use crate::scene_tree::SceneTree;

/// Shadow property holding a `ParallaxLayer`'s **authored** `position` —
/// Godot's `ParallaxLayer::orig_offset`, captured on ENTER_TREE and added back
/// (scaled) on every scroll (`parallax_layer.cpp`).
///
/// ⚠️ Without this the authored position is destroyed rather than merely
/// ignored: this phase overwrites `position` every frame, so frame 1 reads the
/// authored value and frame 2 reads the value frame 1 wrote. A layer nudged 100px
/// down in the editor would snap to the bare parallax offset on the first tick,
/// with nothing left to recover it from.
pub const LAYER_ORIG_OFFSET: &str = "_parallax_orig_offset";

/// Drives `ParallaxBackground` / `ParallaxLayer` scroll from the active
/// `Camera2D` (pat-s3rrv.86).
///
/// The maths has been in `gdserver2d::parallax` — with its own test file —
/// since it was written, and had **no production caller**: a `.tscn` with a
/// ParallaxBackground loaded, kept its properties, and scrolled not one pixel.
/// This is the phase that calls it.
///
/// ⚠️ **A ParallaxLayer's `position` is a SCREEN position, not a world one.**
/// `ParallaxBackground` is registered `.parent("CanvasLayer")`
/// (`gdobject/src/class_db.rs`), and the render list gives a CanvasLayer's items
/// `CanvasFollow::Fixed` — `apply_camera2d_view` skips them, so the camera
/// transform is never applied to a layer (`gdeditor/src/viewport_2d_render.rs`).
/// That is exactly Godot's arrangement, and it is what makes the whole node work:
/// a layer starts pinned to the screen, and its offset is what moves it.
///
/// So the value written here is Godot's own, with no sign adjustment: the
/// **canvas transform origin** — where a world-space node at the origin renders
/// — scaled by `motion_scale`. Hence the documented endpoints:
///
/// * `motion_scale = 1` ⇒ the layer sits exactly where a world object at the
///   origin does, so it scrolls at the full camera rate;
/// * `motion_scale = 0` ⇒ the offset stays `0` and the layer never scrolls
///   ("If an axis is set to 0, it will not scroll" — `doc/classes/ParallaxLayer.xml`);
/// * `motion_scale = 0.5` ⇒ half rate, the distant-background case.
///
/// ⚠️ An earlier pass shipped this NEGATED, on the reasoning that the offset
/// lands on a world `position` and so needed a sign flip. It does not — a
/// CanvasLayer child is screen-fixed. The bug scrolled every background the
/// wrong way, and its own acceptance test could not see it: at
/// `motion_scale = 0.5` both conventions read 100px of travel. **0.5 is a
/// coincidence point**; only the `0` and `1` endpoints separate them, which is
/// why the test now pins all three.
///
/// ⚠️ Reads `CAMERA2D_SCROLL_CENTER`, the centre `process_camera2d_scroll`
/// resolved THIS frame, not the camera's raw position — so a smoothed or
/// drag-held camera parallaxes against the view it actually renders. That is
/// why this phase runs after it.
pub fn process_parallax(tree: &mut SceneTree, delta_secs: f32) {
    // A parallax node follows the camera of the viewport it is IN (pat-8mb8y):
    // the split-screen scene has one ParallaxBackground per SubViewport, each
    // under its own player's camera. Without SubViewports this is one pass over
    // the root, exactly as before.
    let sub_viewports = crate::behavioral_2d::sub_viewports(tree);
    if sub_viewports.is_empty() {
        process_parallax_in(tree, None, false, delta_secs);
        return;
    }
    process_parallax_in(tree, None, true, delta_secs);
    for vp in sub_viewports {
        process_parallax_in(tree, Some(vp), true, delta_secs);
    }
}

/// [`process_parallax`] for the nodes of one viewport (`None` = the root).
/// `filter` restricts the pass to nodes inside `vp`; off, every node is taken.
fn process_parallax_in(tree: &mut SceneTree, vp: Option<NodeId>, filter: bool, delta_secs: f32) {
    let viewport = crate::behavioral_2d::sub_viewport_size(tree, vp);
    let Some(cam) = crate::behavioral_2d::current_camera2d_in(tree, vp) else {
        return;
    };
    let in_viewport = |tree: &SceneTree, id: NodeId| {
        !filter || crate::behavioral_2d::enclosing_sub_viewport(tree, id) == vp
    };
    let origin = canvas_origin(tree, cam, viewport);
    let zoom = camera_zoom_scale(tree, cam);
    let screen_offset = camera_screen_offset(tree, cam, viewport);

    // Collected first: the write loop needs `&mut tree` while the read of each
    // background's own properties needs `&tree`.
    let mut writes: Vec<(NodeId, Vector2, Vector2)> = Vec::new();
    for id in tree.all_nodes_in_tree_order() {
        let Some(node) = tree.get_node(id) else {
            continue;
        };
        if node.class_name() != "ParallaxBackground" || !in_viewport(tree, id) {
            continue;
        }
        let background = background_from_node(node, origin);
        let limits = limits_from_node(node, viewport);
        for child in node.children().iter().copied() {
            let Some(layer_node) = tree.get_node(child) else {
                continue;
            };
            if layer_node.class_name() != "ParallaxLayer" {
                continue;
            }
            let layer = layer_from_node(layer_node);
            // Godot: `new_ofs = base * motion_scale + motion_offset * scale +
            // orig_offset * scale`. `gdserver2d` models the first two terms —
            // it has no notion of an authored position — so the third is added
            // here rather than re-deriving the other two.
            //
            // ⚠️ The authored term takes the SAME effective scale the offset
            // did: Godot hands the layer `1.0` when `scroll_ignore_camera_zoom`
            // is set (`parallax_background.cpp`), so using the raw zoom here
            // would leave a layer's authored offset zooming while its scroll
            // did not.
            let scale = if background.scroll_ignore_camera_zoom {
                1.0
            } else {
                zoom
            };
            let orig = authored_offset(layer_node);
            let offset = background.layer_offset_limited(&layer, zoom, screen_offset, &limits);
            writes.push((
                child,
                Vector2::new(offset.x + orig.x * scale, offset.y + orig.y * scale),
                orig,
            ));
        }
    }

    process_parallax_2d(tree, cam, viewport, delta_secs, &in_viewport);

    for (layer_id, offset, orig) in writes {
        // Written as `position`, the same property the editor and the render
        // list already read, so no new transform channel is introduced.
        if let Some(node) = tree.get_node_mut(layer_id) {
            // Seeded before the first overwrite, so it captures the authored
            // value rather than one of this phase's own writes.
            node.set_property(LAYER_ORIG_OFFSET, Variant::Vector2(orig));
            node.set_property("position", Variant::Vector2(offset));
        }
    }
}

/// Drives every `Parallax2D` in the tree — Godot 4.3's replacement for the
/// `ParallaxBackground`/`ParallaxLayer` pair.
///
/// ⚠️ **A `Parallax2D` is a `Node2D`, not a `CanvasLayer`**, so unlike a
/// ParallaxLayer the camera transform DOES apply to it. It scrolls by being
/// placed in the WORLD, which is why `gdserver2d`'s formula for it is inverted
/// relative to the layer one and why `scroll_scale`'s endpoints are the other
/// way round (`1` = moves with the world, `0` = pinned to the screen).
///
/// It also takes a DIFFERENT camera quantity: Godot's `adj_screen_pos`, the
/// view's world-space top-left (`camera_screen_center - screen_size * 0.5`,
/// `camera_2d.cpp:63`) — the third `_camera_moved` argument, which
/// ParallaxBackground ignores.
///
/// The class has been registered in ClassDB with all ten of its properties and
/// no behaviour whatsoever: a `.tscn` containing one loaded, kept every
/// property, and never moved. This is the phase that drives it.
fn process_parallax_2d(
    tree: &mut SceneTree,
    cam: NodeId,
    viewport: Vector2,
    delta_secs: f32,
    in_viewport: &dyn Fn(&SceneTree, NodeId) -> bool,
) {
    let center = match tree
        .get_node(cam)
        .map(|n| n.get_property(crate::behavioral_2d::CAMERA2D_SCROLL_CENTER))
    {
        Some(Variant::Vector2(v)) => v,
        _ => crate::node2d::get_screen_center_position(tree, cam, viewport),
    };
    let adj_screen_pos = Vector2::new(center.x - viewport.x * 0.5, center.y - viewport.y * 0.5);

    let mut writes: Vec<(NodeId, Vector2, Vector2, Vector2)> = Vec::new();
    for id in tree.all_nodes_in_tree_order() {
        let Some(node) = tree.get_node(id) else {
            continue;
        };
        if node.class_name() != "Parallax2D" || !in_viewport(tree, id) {
            continue;
        }
        let mut parallax = parallax_2d_from_node(node);
        // Advanced BEFORE the camera is applied and before the position is
        // computed, matching Godot's NOTIFICATION_INTERNAL_PROCESS, which runs
        // the autoscroll step and then calls `_update_scroll`.
        parallax.advance_autoscroll(delta_secs);
        parallax.follow_camera(adj_screen_pos);
        // The node's OWN scale stretches the repeat period; this is not a zoom.
        writes.push((
            id,
            parallax.position(viewport, crate::node2d::get_scale(tree, id)),
            parallax.scroll_offset,
            parallax.screen_offset,
        ));
    }

    for (id, position, scroll_offset, screen_offset) in writes {
        if let Some(node) = tree.get_node_mut(id) {
            node.set_property("position", Variant::Vector2(position));
            // ⚠️ Both are read back on the next frame, so they must be written:
            // `scroll_offset` is the autoscroll ACCUMULATOR (dropping it would
            // leave an autoscrolling node advancing one frame's worth for ever
            // and never actually moving), and `screen_offset` is what
            // `ignore_camera_scroll` freezes. Godot sets both through their
            // property setters for the same reason.
            node.set_property("scroll_offset", Variant::Vector2(scroll_offset));
            node.set_property("screen_offset", Variant::Vector2(screen_offset));
        }
    }
}

/// Reads a `Parallax2D`'s authored properties, falling back to Godot's defaults.
///
/// ⚠️ The limit defaults are `±1e7`, NOT zero. Defaulting a missing limit to
/// zero would clamp every Parallax2D to a degenerate range and freeze it.
fn parallax_2d_from_node(node: &crate::node::Node) -> gdserver2d::parallax::Parallax2D {
    let vec2 = |name: &str, default: Vector2| match node.get_property(name) {
        Variant::Vector2(v) => v,
        _ => default,
    };
    let flag = |name: &str, default: bool| match node.get_property(name) {
        Variant::Bool(b) => b,
        _ => default,
    };
    let mut parallax = gdserver2d::parallax::Parallax2D::new();
    parallax.scroll_scale = vec2("scroll_scale", parallax.scroll_scale);
    parallax.scroll_offset = vec2("scroll_offset", parallax.scroll_offset);
    parallax.repeat_size = vec2("repeat_size", parallax.repeat_size);
    parallax.autoscroll = vec2("autoscroll", parallax.autoscroll);
    parallax.limit_begin = vec2("limit_begin", parallax.limit_begin);
    parallax.limit_end = vec2("limit_end", parallax.limit_end);
    parallax.follow_viewport = flag("follow_viewport", parallax.follow_viewport);
    parallax.ignore_camera_scroll = flag("ignore_camera_scroll", parallax.ignore_camera_scroll);
    parallax.screen_offset = vec2("screen_offset", parallax.screen_offset);
    if let Variant::Int(times) = node.get_property("repeat_times") {
        parallax.repeat_times = times as i32;
    }
    parallax
}

/// Where a world-space node at the origin renders — Godot's canvas transform
/// origin, which is the quantity `ParallaxBackground` scrolls by.
///
/// Godot hands `_camera_moved` the viewport's canvas transform and takes its
/// origin (`parallax_background.cpp:52`). The equivalent here is the origin of
/// `viewport_2d_render::camera2d_view_transform`, and this mirrors that
/// composition — `translate(half) * scale(zoom) * rotate(-θ) * translate(-centre)`
/// — because that function lives in `gdeditor`, which depends on this crate and
/// so cannot be called from it.
///
/// ⚠️ Both read the same published centre, rotation and zoom, so they agree by
/// construction on every input; what is duplicated is only the composition.
/// Hoisting it into this crate and having the renderer call it is recorded as
/// remaining scope rather than done here, because the renderer's own file is
/// heavily shared.
fn canvas_origin(tree: &SceneTree, cam: NodeId, viewport: Vector2) -> Vector2 {
    let config = crate::node2d::camera2d_config_from_node(tree, cam);
    let center = match tree
        .get_node(cam)
        .map(|n| n.get_property(crate::behavioral_2d::CAMERA2D_SCROLL_CENTER))
    {
        Some(Variant::Vector2(v)) => v,
        _ => crate::node2d::get_screen_center_position(tree, cam, viewport),
    };
    // Godot folds the camera's rotation into the canvas transform only when
    // `ignore_rotation` is false, which is NOT the default.
    let rotation = if config.ignore_rotation {
        0.0
    } else {
        match tree
            .get_node(cam)
            .map(|n| n.get_property(crate::behavioral_2d::CAMERA2D_SCROLL_ROTATION))
        {
            Some(Variant::Float(f)) => f as f32,
            _ => crate::node2d::get_global_rotation(tree, cam),
        }
    };

    let half = Vector2::new(viewport.x * 0.5, viewport.y * 0.5);
    let view = Transform2D::translated(half) * Transform2D::scaled(config.zoom);
    let view = if rotation == 0.0 {
        view
    } else {
        view * Transform2D::rotated(-rotation)
    };
    (view * Transform2D::translated(-center)).origin
}

/// The camera zoom as the single scale `ParallaxBackground` expects.
///
/// ⚠️ Godot takes `transform.get_scale().dot(Vector2(0.5, 0.5))`
/// (`parallax_background.cpp:51`) — the MEAN of the two axes, not `zoom.x`.
/// A non-uniform zoom otherwise parallaxes at the horizontal rate on both axes.
fn camera_zoom_scale(tree: &SceneTree, cam: NodeId) -> f32 {
    match tree.get_node(cam).map(|n| n.get_property("zoom")) {
        Some(Variant::Vector2(z)) => (z.x + z.y) * 0.5,
        _ => 1.0,
    }
}

/// The layer's authored `position`, from the shadow if this phase has already
/// overwritten it and from `position` itself on the first frame.
fn authored_offset(node: &crate::node::Node) -> Vector2 {
    match node.get_property(LAYER_ORIG_OFFSET) {
        Variant::Vector2(v) => v,
        _ => match node.get_property("position") {
            Variant::Vector2(v) => v,
            _ => Vector2::ZERO,
        },
    }
}

/// Godot's `screen_offset`, the term the `scroll_ignore_camera_zoom` branch adds.
///
/// `Camera2D::_notification` (`scene/2d/camera_2d.cpp:62`):
/// ```text
/// Point2 screen_offset = (anchor_mode == ANCHOR_MODE_DRAG_CENTER ? (screen_size * 0.5) : Point2());
/// ```
///
/// ⚠️ It is a HALF-VIEWPORT, not a camera position and not the view's
/// world-space top-left — that last one is `adj_screen_pos`, a THIRD argument
/// `_camera_moved` receives and `ParallaxBackground` ignores (only the
/// standalone `Parallax2D` uses it). Passing a camera-relative value here would
/// make an ignore-zoom background drift with the camera instead of holding
/// still, which is the very bug the ignore-zoom branch exists to prevent.
///
/// Godot's default anchor mode is DRAG_CENTER, so the zero case is the
/// exception rather than the rule.
fn camera_screen_offset(tree: &SceneTree, cam: NodeId, viewport: Vector2) -> Vector2 {
    let config = crate::node2d::camera2d_config_from_node(tree, cam);
    if config.anchor_mode == crate::node2d::ANCHOR_MODE_DRAG_CENTER {
        Vector2::new(viewport.x * 0.5, viewport.y * 0.5)
    } else {
        Vector2::ZERO
    }
}

/// The background's `scroll_limit_begin` / `scroll_limit_end`, measured against
/// the viewport Godot's `_update_scroll` reads from `get_viewport_size()`.
///
/// ⚠️ The viewport size is read HERE, per frame, rather than stored on the
/// background: it is not node state and would go stale on the first resize.
///
/// Both limits default to `(0, 0)`, and equal limits DISABLE the axis rather
/// than pinning it to zero — so an unconfigured background, which is almost all
/// of them, scrolls freely.
fn limits_from_node(
    node: &crate::node::Node,
    viewport: Vector2,
) -> gdserver2d::parallax::ScrollLimits {
    let vec2 = |name: &str, default: Vector2| match node.get_property(name) {
        Variant::Vector2(v) => v,
        _ => default,
    };
    gdserver2d::parallax::ScrollLimits {
        begin: vec2("scroll_limit_begin", Vector2::ZERO),
        end: vec2("scroll_limit_end", Vector2::ZERO),
        viewport_size: viewport,
    }
}

fn background_from_node(
    node: &crate::node::Node,
    canvas_origin: Vector2,
) -> gdserver2d::parallax::ParallaxBackground {
    let vec2 = |name: &str, default: Vector2| match node.get_property(name) {
        Variant::Vector2(v) => v,
        _ => default,
    };
    gdserver2d::parallax::ParallaxBackground {
        scroll_offset: canvas_origin,
        scroll_base_offset: vec2("scroll_base_offset", Vector2::ZERO),
        scroll_base_scale: vec2("scroll_base_scale", Vector2::ONE),
        scroll_ignore_camera_zoom: matches!(
            node.get_property("scroll_ignore_camera_zoom"),
            Variant::Bool(true)
        ),
    }
}

fn layer_from_node(node: &crate::node::Node) -> gdserver2d::parallax::ParallaxLayer {
    let vec2 = |name: &str, default: Vector2| match node.get_property(name) {
        Variant::Vector2(v) => v,
        _ => default,
    };
    gdserver2d::parallax::ParallaxLayer {
        // ⚠️ Godot's default is `(1, 1)`, not zero: an unconfigured layer scrolls
        // at the full camera rate, like a world object. Defaulting to zero would
        // pin every layer to the screen, which reads as "the camera stopped
        // affecting the background" rather than as a missing default.
        motion_scale: vec2("motion_scale", Vector2::ONE),
        motion_offset: vec2("motion_offset", Vector2::ZERO),
        motion_mirroring: vec2("motion_mirroring", Vector2::ZERO),
    }
}
