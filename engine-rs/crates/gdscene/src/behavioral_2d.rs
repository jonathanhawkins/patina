//! pat-0q2kx.190 — runtime behavior for three 2D "behavioral" nodes.
//!
//! `RemoteTransform2D`, `VisibleOnScreenNotifier2D` (with its
//! `VisibleOnScreenEnabler2D` subclass) and `TouchScreenButton` were all
//! registered in ClassDB with the correct properties and signals, and all three
//! were **inert**: nothing in `main_loop` or `scene_tree` ever mentioned them,
//! so a scene containing one behaved exactly as if the node were absent.
//!
//! Each is a per-frame (or per-event) phase mirroring `process_path_follows`:
//! gather immutably over the tree, then apply. Godot references:
//! `upstream/godot/scene/2d/remote_transform_2d.cpp`,
//! `visible_on_screen_notifier_2d.cpp`, `touch_screen_button.cpp`.

use gdcore::math::{Rect2, Vector2};
use gdplatform::input::InputEvent;
use gdvariant::Variant;

use crate::node::{NodeId, ProcessMode};
use crate::node2d;
use crate::scene_tree::SceneTree;

/// Published each frame by [`process_visibility_notifiers`] so a script can
/// read what Godot exposes as `VisibleOnScreenNotifier2D.is_on_screen()`.
pub const ON_SCREEN_PROP: &str = "is_on_screen";

/// Published by [`route_touch_screen_buttons`] — Godot's
/// `TouchScreenButton.is_pressed()`.
pub const BUTTON_PRESSED_PROP: &str = "is_pressed";

// ===========================================================================
// Shared helpers
// ===========================================================================

/// Reads a `NodePath`-typed property as a string, tolerating a plain `String`
/// (which is how paths arrive from a parsed `.tscn` before typing).
fn node_path_prop(tree: &SceneTree, id: NodeId, key: &str) -> Option<String> {
    let node = tree.get_node(id)?;
    let path = match node.get_property(key) {
        Variant::NodePath(p) => p.to_string(),
        Variant::String(s) => s,
        _ => return None,
    };
    (!path.is_empty()).then_some(path)
}

/// Resolves a node path the way `Node.get_node` does: absolute paths from the
/// root, everything else relative to `from` (which also covers `..`).
fn resolve_path(tree: &SceneTree, from: NodeId, path: &str) -> Option<NodeId> {
    if path.starts_with('/') {
        tree.get_node_by_path(path)
    } else {
        tree.get_node_relative(from, path)
    }
}

fn bool_prop(tree: &SceneTree, id: NodeId, key: &str, default: bool) -> bool {
    match tree.get_node(id).map(|n| n.get_property(key)) {
        Some(Variant::Bool(b)) => b,
        _ => default,
    }
}

fn vec2_prop(tree: &SceneTree, id: NodeId, key: &str) -> Option<Vector2> {
    match tree.get_node(id)?.get_property(key) {
        Variant::Vector2(v) => Some(v),
        _ => None,
    }
}

fn float_prop(tree: &SceneTree, id: NodeId, key: &str) -> Option<f32> {
    match tree.get_node(id)?.get_property(key) {
        Variant::Float(f) => Some(f as f32),
        Variant::Int(i) => Some(i as f32),
        _ => None,
    }
}

/// The product of the local scales along the parent chain.
///
/// 🔑 Delegates to [`node2d::get_global_scale`] rather than repeating the walk
/// (pat-s3rrv.184). This was a byte-for-byte copy of it, and the copy had
/// already drifted: `node2d`'s version stops at a `top_level` node and this one
/// walked past it, so a detached node reported one scale here and another
/// there. One rule, one place — the whole reason that class of bug is possible
/// is having two.
///
/// ⚠️ The doc this replaced claimed the chain product is "exact for the
/// translate·rotate·scale transforms Node2D builds". That is no longer true:
/// Node2D now composes **skew** as well (`node_2d.cpp::_update_transform`), so
/// this is an approximation that ignores shear — as is `get_global_rotation`.
/// Godot derives both by decomposing the global transform. Deriving them
/// properly is a real change with its own blast radius and is NOT part of this
/// bead; recording it here so the next reader does not trust the old claim.
fn global_scale(tree: &SceneTree, id: NodeId) -> Vector2 {
    node2d::get_global_scale(tree, id)
}

// ===========================================================================
// RemoteTransform2D
// ===========================================================================

/// One resolved push, gathered before any mutation so the tree walk stays
/// immutable (a remote target may itself be another RemoteTransform2D's source).
struct RemotePush {
    target: NodeId,
    position: Option<Vector2>,
    rotation: Option<f32>,
    scale: Option<Vector2>,
    global: bool,
}

/// Copies each `RemoteTransform2D`'s transform onto the node at its
/// `remote_path`, honoring the three `update_*` toggles and
/// `use_global_coordinates`.
///
/// Godot pushes on every transform change; a per-frame phase is the equivalent
/// for a frame-stepped tree.
pub fn process_remote_transforms(tree: &mut SceneTree) {
    let mut pushes: Vec<RemotePush> = Vec::new();

    for id in tree.all_nodes_in_tree_order() {
        let Some(node) = tree.get_node(id) else {
            continue;
        };
        if node.class_name() != "RemoteTransform2D" {
            continue;
        }
        let Some(path) = node_path_prop(tree, id, "remote_path") else {
            continue;
        };
        let Some(target) = resolve_path(tree, id, &path) else {
            continue;
        };
        // Pushing onto yourself would feed the transform back into itself every
        // frame; Godot guards the same case.
        if target == id {
            continue;
        }

        // All four flags default to true in Godot.
        let global = bool_prop(tree, id, "use_global_coordinates", true);
        let position = bool_prop(tree, id, "update_position", true).then(|| {
            if global {
                node2d::get_global_position(tree, id)
            } else {
                node2d::get_position(tree, id)
            }
        });
        let rotation = bool_prop(tree, id, "update_rotation", true).then(|| {
            if global {
                node2d::get_global_rotation(tree, id)
            } else {
                node2d::get_rotation(tree, id)
            }
        });
        let scale = bool_prop(tree, id, "update_scale", true).then(|| {
            if global {
                global_scale(tree, id)
            } else {
                node2d::get_scale(tree, id)
            }
        });

        pushes.push(RemotePush {
            target,
            position,
            rotation,
            scale,
            global,
        });
    }

    for push in pushes {
        if let Some(pos) = push.position {
            if push.global {
                node2d::set_global_position(tree, push.target, pos);
            } else {
                node2d::set_position(tree, push.target, pos);
            }
        }
        if let Some(rot) = push.rotation {
            if push.global {
                // Set the local rotation that lands the target at `rot`
                // globally — 2D global rotation is the sum along the chain, so
                // subtracting the parent's is exact.
                let parent_rot = tree
                    .get_node(push.target)
                    .and_then(|n| n.parent())
                    .map(|p| node2d::get_global_rotation(tree, p))
                    .unwrap_or(0.0);
                node2d::set_rotation(tree, push.target, rot - parent_rot);
            } else {
                node2d::set_rotation(tree, push.target, rot);
            }
        }
        if let Some(scale) = push.scale {
            if push.global {
                let parent_scale = tree
                    .get_node(push.target)
                    .and_then(|n| n.parent())
                    .map(|p| global_scale(tree, p))
                    .unwrap_or(Vector2::new(1.0, 1.0));
                // A zero parent scale collapses the space entirely; there is no
                // local scale that recovers `scale`, so leave the target alone
                // rather than dividing by zero.
                if parent_scale.x.abs() > f32::EPSILON && parent_scale.y.abs() > f32::EPSILON {
                    node2d::set_scale(
                        tree,
                        push.target,
                        Vector2::new(scale.x / parent_scale.x, scale.y / parent_scale.y),
                    );
                }
            } else {
                node2d::set_scale(tree, push.target, scale);
            }
        }
    }
}

// ===========================================================================
// VisibleOnScreenNotifier2D / VisibleOnScreenEnabler2D
// ===========================================================================

fn is_notifier(class_name: &str) -> bool {
    matches!(
        class_name,
        "VisibleOnScreenNotifier2D" | "VisibleOnScreenEnabler2D"
    )
}

/// The world-space rect the viewport currently shows.
///
/// With a `current` Camera2D this is the camera's screen-center box scaled by
/// zoom (Godot 4: larger zoom magnifies, so the visible area *shrinks*).
/// Without one the viewport sits at the origin, which is what an uncameraed
/// scene renders.
pub fn visible_world_rect(tree: &SceneTree) -> Rect2 {
    let viewport = tree.root_window().visible_size();

    // Same lookup the renderer uses, so the rect culled against is the rect
    // drawn — a Godot 4 scene spells the claim `enabled`, which the old
    // `current`-only test here never matched (pat-s3rrv.84).
    let camera = current_camera2d(tree);

    match camera {
        Some(cam) => {
            let config = node2d::camera2d_config_from_node(tree, cam);
            let zoom = config.zoom;
            let size = Vector2::new(
                if zoom.x.abs() > f32::EPSILON {
                    viewport.x / zoom.x
                } else {
                    viewport.x
                },
                if zoom.y.abs() > f32::EPSILON {
                    viewport.y / zoom.y
                } else {
                    viewport.y
                },
            );
            // Prefer the centre `process_camera2d_scroll` resolved this frame,
            // so a smoothed or drag-held camera culls against the view it
            // actually renders rather than the target it is still easing toward
            // (pat-s3rrv.84). Falls back to the snapped target when no frame has
            // been stepped yet, which is what an un-run tree reports.
            let center = match tree
                .get_node(cam)
                .map(|n| n.get_property(CAMERA2D_SCROLL_CENTER))
            {
                Some(Variant::Vector2(v)) => v,
                _ => node2d::get_screen_center_position(tree, cam, viewport),
            };
            Rect2::new(center - size * 0.5, size)
        }
        None => Rect2::new(Vector2::ZERO, viewport),
    }
}

/// The notifier's `rect` in world space — the axis-aligned bound of its four
/// corners under the global transform, so a rotated or scaled notifier still
/// tests the area it actually covers.
fn notifier_world_rect(tree: &SceneTree, id: NodeId) -> Rect2 {
    let local = match tree.get_node(id).map(|n| n.get_property("rect")) {
        Some(Variant::Rect2(r)) => r,
        // Godot's default rect, mirrored by the ClassDB registration.
        _ => Rect2::new(Vector2::new(-10.0, -10.0), Vector2::new(20.0, 20.0)),
    };
    let xform = node2d::get_global_transform(tree, id);

    let corners = [
        xform.xform(local.position),
        xform.xform(local.position + Vector2::new(local.size.x, 0.0)),
        xform.xform(local.position + Vector2::new(0.0, local.size.y)),
        xform.xform(local.position + local.size),
    ];
    let mut min = corners[0];
    let mut max = corners[0];
    for c in &corners[1..] {
        min = Vector2::new(min.x.min(c.x), min.y.min(c.y));
        max = Vector2::new(max.x.max(c.x), max.y.max(c.y));
    }
    Rect2::new(min, max - min)
}

/// Tests every notifier against the visible rect, publishes
/// [`ON_SCREEN_PROP`], and emits `screen_entered` / `screen_exited` on the
/// frames the state flips.
///
/// `VisibleOnScreenEnabler2D` additionally drives its target's process mode,
/// which is the whole point of the subclass: off-screen enemies stop thinking.
pub fn process_visibility_notifiers(tree: &mut SceneTree) {
    let visible = visible_world_rect(tree);

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

        let on_screen = visible.intersects(notifier_world_rect(tree, id));
        // A notifier starts off-screen, so one that spawns inside the view
        // emits `screen_entered` on its first processed frame — matching Godot,
        // where the visibility notifier reports entry when it becomes visible.
        let was_on_screen = bool_prop(tree, id, ON_SCREEN_PROP, false);

        let enabler_target = (class_name == "VisibleOnScreenEnabler2D")
            .then(|| {
                let path = node_path_prop(tree, id, "enable_node_path")
                    .unwrap_or_else(|| "..".to_string());
                let target = resolve_path(tree, id, &path)?;
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
        // `is_on_screen` sees the new state, not the one it just left.
        if update.entered {
            tree.emit_signal(update.id, "screen_entered", &[]);
        }
        if update.exited {
            tree.emit_signal(update.id, "screen_exited", &[]);
        }
    }
}

// ===========================================================================
// TouchScreenButton
// ===========================================================================

/// An action a touch button wants pressed or released.
///
/// Returned rather than applied so the caller can hold the `InputState` borrow
/// separately from the tree borrow.
#[derive(Debug, Clone, PartialEq)]
pub struct ActionEdit {
    pub action: String,
    pub pressed: bool,
}

/// Whether `local` lands inside the button's hit area.
///
/// Godot tests the `shape` when one is assigned and the texture's rect
/// otherwise. Shapes arrive flattened as `__shape__*` companions (the
/// sub-resource convention), and texture extents as `texture_size`.
fn touch_button_hit(tree: &SceneTree, id: NodeId, local: Vector2) -> bool {
    let centered = bool_prop(tree, id, "shape_centered", true);

    if let Some(size) = vec2_prop(tree, id, "__shape__size") {
        let origin = if centered { size * -0.5 } else { Vector2::ZERO };
        return local.x >= origin.x
            && local.y >= origin.y
            && local.x <= origin.x + size.x
            && local.y <= origin.y + size.y;
    }
    if let Some(radius) = float_prop(tree, id, "__shape__radius") {
        let center = if centered {
            Vector2::ZERO
        } else {
            Vector2::new(radius, radius)
        };
        let d = local - center;
        return d.x * d.x + d.y * d.y <= radius * radius;
    }
    if let Some(size) = vec2_prop(tree, id, "texture_size") {
        // A TouchScreenButton draws `texture_normal` with its top-left at the
        // node origin, so the hit rect is not centered here.
        return local.x >= 0.0 && local.y >= 0.0 && local.x <= size.x && local.y <= size.y;
    }
    // No shape and no texture is no hit area at all — Godot's button would draw
    // nothing and swallow nothing.
    false
}

/// Publishes every `TouchScreenButton`'s [`BUTTON_PRESSED_PROP`] once per
/// frame, so a button reads `false` before it is ever touched instead of `Nil`.
///
/// [`route_touch_screen_buttons`] only runs when an input event arrives, so
/// without this a scene that has simply never been touched exposes no
/// `is_pressed` at all and a script reading it gets Nil rather than a bool —
/// Godot's `TouchScreenButton::is_pressed()` returns `false` from the start.
///
/// Re-publishing the value it just read makes this idempotent: it materializes
/// the default without ever clobbering a press routed earlier in the same
/// frame, whichever order the two phases run in.
pub fn process_touch_screen_buttons(tree: &mut SceneTree) {
    let mut buttons: Vec<NodeId> = Vec::new();
    for id in tree.all_nodes_in_tree_order() {
        if let Some(node) = tree.get_node(id) {
            if node.class_name() == "TouchScreenButton" {
                buttons.push(id);
            }
        }
    }
    for id in buttons {
        let state = bool_prop(tree, id, BUTTON_PRESSED_PROP, false);
        if let Some(node) = tree.get_node_mut(id) {
            node.set_property(BUTTON_PRESSED_PROP, Variant::Bool(state));
        }
    }
}

/// Routes a pointer/touch event to `TouchScreenButton` nodes: sets
/// [`BUTTON_PRESSED_PROP`], emits `pressed` / `released`, and reports the
/// actions to inject.
///
/// Returns the action edits for the caller to apply to the engine's
/// `InputState`, which is what makes `Input.is_action_pressed(...)` see a
/// touch button the same as a key binding.
pub fn route_touch_screen_buttons(tree: &mut SceneTree, event: &InputEvent) -> Vec<ActionEdit> {
    // `pressed = None` means "the pointer moved while already down" — only
    // interesting to a passby_press button.
    //
    // Plain `MouseMotion` is deliberately NOT handled: it fires with no button
    // held, so treating it as a drag would make merely hovering the cursor
    // press every passby button it crossed. Godot acts on
    // `InputEventScreenDrag`, which only exists while a finger is down.
    let (position, pressed): (Vector2, Option<bool>) = match event {
        InputEvent::ScreenTouch {
            position, pressed, ..
        } => (*position, Some(*pressed)),
        // Only the left button acts as a touch; a right-click opening a context
        // menu must not also fire the control underneath it.
        InputEvent::MouseButton {
            button: gdplatform::input::MouseButton::Left,
            position,
            pressed,
        } => (*position, Some(*pressed)),
        InputEvent::ScreenDrag { position, .. } => (*position, None),
        _ => return Vec::new(),
    };

    struct Change {
        id: NodeId,
        now_pressed: bool,
        action: String,
    }
    let mut changes: Vec<Change> = Vec::new();
    // Every button this event reaches republishes its state, whether or not it
    // moved. Publishing only on a flip left `is_pressed` absent — reading Nil,
    // not `false` — on any button that had never been pressed.
    let mut publish: Vec<(NodeId, bool)> = Vec::new();

    for id in tree.all_nodes_in_tree_order() {
        let Some(node) = tree.get_node(id) else {
            continue;
        };
        if node.class_name() != "TouchScreenButton" {
            continue;
        }

        let was_pressed = bool_prop(tree, id, BUTTON_PRESSED_PROP, false);
        let passby = bool_prop(tree, id, "passby_press", false);

        // A move only presses a button that opted into passby_press; without it
        // a drag across the screen must not trip every button it crosses — it
        // holds its current state rather than dropping out of the pass, so the
        // property is still published below.
        let now_pressed = if pressed.is_none() && !passby {
            was_pressed
        } else {
            let local = node2d::get_global_transform(tree, id)
                .affine_inverse()
                .xform(position);
            let hit = touch_button_hit(tree, id, local);

            match pressed {
                // A release anywhere releases a held button — lifting your
                // finger off the edge must not leave it stuck down.
                Some(false) => false,
                Some(true) => hit,
                None => hit,
            }
        };

        publish.push((id, now_pressed));
        if now_pressed == was_pressed {
            continue;
        }

        let action = match node.get_property("action") {
            Variant::String(s) => s,
            _ => String::new(),
        };
        changes.push(Change {
            id,
            now_pressed,
            action,
        });
    }

    // Published before any signal fires so a handler reading `is_pressed` sees
    // the state it is being notified about, not the one it just left.
    for (id, state) in publish {
        if let Some(node) = tree.get_node_mut(id) {
            node.set_property(BUTTON_PRESSED_PROP, Variant::Bool(state));
        }
    }

    let mut edits = Vec::new();
    for change in changes {
        tree.emit_signal(
            change.id,
            if change.now_pressed {
                "pressed"
            } else {
                "released"
            },
            &[],
        );
        if !change.action.is_empty() {
            edits.push(ActionEdit {
                action: change.action,
                pressed: change.now_pressed,
            });
        }
    }
    edits
}

/// Shadow property holding the Camera2D's resolved view centre, republished
/// every frame by [`process_camera2d_scroll`].
///
/// Godot keeps `camera_pos`/`smoothed_camera_pos` as members on the Camera2D
/// and integrates them in `_update_scroll`; Patina has no per-node runtime
/// struct, so the running state lives on the node as an internal property —
/// the same idiom `main_loop` uses for `_pathfollow_published_progress`.
pub const CAMERA2D_SCROLL_CENTER: &str = "_camera2d_scroll_center";

/// Shadow property holding the Camera2D's resolved view rotation. Stays at
/// `0.0` while `ignore_rotation` is set, which is Godot's default.
pub const CAMERA2D_SCROLL_ROTATION: &str = "_camera2d_scroll_rotation";

/// The Camera2D driving the view, or `None` when no camera claims it.
///
/// Godot 4 spells the claim `enabled`; Godot 3 spelled it `current` and scenes
/// in the wild still carry it, so a missing `enabled` falls back to it. When
/// several cameras claim at once the LAST in top-down order wins, which is the
/// tie-break the renderer has always used — this is the one definition both it
/// and the scroll pass resolve through, so they cannot disagree about which
/// camera is current (pat-s3rrv.84).
pub fn current_camera2d(tree: &SceneTree) -> Option<NodeId> {
    let mut subtree = Vec::new();
    tree.collect_subtree_top_down(tree.root_id(), &mut subtree);
    let mut found = None;
    for id in subtree {
        let Some(node) = tree.get_node(id) else {
            continue;
        };
        if node.class_name() != "Camera2D" {
            continue;
        }
        // Godot 4's `Camera2D.enabled` defaults to TRUE (doc/classes/Camera2D.xml)
        // and a `.tscn` only writes it when false. Reading an ABSENT key as
        // "not enabled" — as this did, falling through to Godot 3's `current` —
        // ignored every camera on a node the loader had not ClassDB-seeded:
        // Pong's Camera2D (offset 320,200) never applied, so the expanded
        // viewport showed the playfield flush at the top instead of centred.
        // A Godot 3 `current = true` still claims it.
        let claims = match node.get_property("enabled") {
            Variant::Bool(b) => b,
            _ => !matches!(node.get_property("current"), Variant::Bool(false)),
        };
        if claims {
            found = Some(id);
        }
    }
    found
}

/// Advances the current Camera2D's scroll state by one frame — Patina's
/// `Camera2D::_update_scroll`.
///
/// Drag margins are a dead zone measured from **last** frame's centre and
/// position smoothing eases from it, so both are identities unless the centre
/// survives between frames. Publishing it back onto the node is what makes
/// `position_smoothing_enabled`, `position_smoothing_speed` and the four
/// `drag_*_margin`s do anything at runtime: until now every caller passed
/// `previous_center = None`, which fed `current == target` into both and
/// collapsed them to a hard snap (pat-s3rrv.84).
///
/// The first frame seeds from the target rather than easing toward it, matching
/// Godot's `first` branch (camera_2d.cpp:208) — a camera that is `current` from
/// the start opens on its subject instead of sliding in from the origin.
pub fn process_camera2d_scroll(tree: &mut SceneTree, dt: f32) {
    let Some(cam) = current_camera2d(tree) else {
        return;
    };
    let viewport = tree.root_window().visible_size();
    let config = node2d::camera2d_config_from_node(tree, cam);

    let target =
        node2d::get_global_transform(tree, cam).origin + node2d::get_camera_offset(tree, cam);
    let previous = match tree
        .get_node(cam)
        .map(|n| n.get_property(CAMERA2D_SCROLL_CENTER))
    {
        Some(Variant::Vector2(v)) => Some(v),
        _ => None,
    };
    // No published centre yet => this camera's first frame: snap.
    let current = previous.unwrap_or(target);
    let center = node2d::compute_camera2d_position(&config, current, target, viewport, dt);

    let target_rotation = node2d::get_global_rotation(tree, cam);
    let previous_rotation = match tree
        .get_node(cam)
        .map(|n| n.get_property(CAMERA2D_SCROLL_ROTATION))
    {
        Some(Variant::Float(f)) => f as f32,
        _ => target_rotation,
    };
    let rotation = node2d::smooth_camera_rotation(&config, previous_rotation, target_rotation, dt);

    if let Some(node) = tree.get_node_mut(cam) {
        node.set_property(CAMERA2D_SCROLL_CENTER, Variant::Vector2(center));
        node.set_property(CAMERA2D_SCROLL_ROTATION, Variant::Float(rotation as f64));
    }
}
