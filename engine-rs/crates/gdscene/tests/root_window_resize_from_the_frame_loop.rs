//! pat-s3rrv.120 — a viewport resize delivered through the runtime path
//! reaches the root window.
//!
//! `SceneTree::resize_root_window` already recomputed the content scale,
//! republished the mirrored root-window properties and emitted `size_changed` —
//! and had **no caller outside tests**. `MainLoop::set_viewport_size` stored a
//! separate tuple used only for 3D pick rays. So after a resize the stretch
//! transform, `get_viewport().get_visible_rect()` and Control anchoring all
//! stayed at the boot size, the signal never fired, and the two sizes could
//! drift apart from each other.
//!
//! ⚠️ **Nothing here calls `resize_root_window`.** The acceptance criterion is
//! explicit that the resize must arrive the way the runtime delivers it, and a
//! test that reached past `set_viewport_size` would pass against the old
//! disconnected code.

use gdcore::content_scale::{
    ContentScaleAspect, ContentScaleConfig, ContentScaleMode, ContentScaleStretch,
};
use gdcore::math::Vector2;
use gdscene::main_loop::MainLoop;
use gdscene::scene_tree::SceneTree;
use gdvariant::Variant;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

/// The project the acceptance names: a 640x360 base stretched with
/// `canvas_items` + `keep`.
fn canvas_items_keep() -> ContentScaleConfig {
    ContentScaleConfig {
        mode: ContentScaleMode::CanvasItems,
        aspect: ContentScaleAspect::Keep,
        stretch: ContentScaleStretch::Fractional,
        factor: 1.0,
        base_size: Vector2::new(640.0, 360.0),
    }
}

/// A booted loop sitting at its base size, as the runner leaves it.
fn booted() -> MainLoop {
    let mut tree = SceneTree::new();
    tree.root_window_mut().config = canvas_items_keep();
    tree.root_window_mut().size = Vector2::new(640.0, 360.0);
    tree.publish_root_window_state();
    MainLoop::new(tree)
}

fn root_prop(ml: &MainLoop, name: &str) -> Variant {
    let tree = ml.tree();
    let root = tree.root_id();
    tree.get_node(root)
        .expect("the tree has a root")
        .get_property(name)
}

/// **The core of the bead.** The resize arrives through the frame loop's own
/// entry point and the root window follows it.
#[test]
fn set_viewport_size_resizes_the_root_window() {
    let mut ml = booted();
    assert_eq!(
        root_prop(&ml, "size"),
        Variant::Vector2(Vector2::new(640.0, 360.0)),
        "precondition: the loop boots at its base size"
    );

    ml.set_viewport_size(1280, 720);

    assert_eq!(
        root_prop(&ml, "size"),
        Variant::Vector2(Vector2::new(1280.0, 720.0)),
        "the published root-window size must follow the viewport resize"
    );
}

/// 🔑 Both sizes come from the one call. They used to be independent — the pick
/// ray tuple moved and the root window did not — so a project could be picking
/// against 1280x720 while its Controls laid out for 640x360.
#[test]
fn the_pick_ray_size_and_the_window_size_cannot_disagree() {
    let mut ml = booted();
    ml.set_viewport_size(1280, 720);

    assert_eq!(
        ml.viewport_size(),
        (1280, 720),
        "the pick-ray tuple follows"
    );
    assert_eq!(
        root_prop(&ml, "size"),
        Variant::Vector2(Vector2::new(1280.0, 720.0)),
        "and so does the window, from the same call"
    );
}

/// `size_changed` fires on a real resize, and **exactly once**.
///
/// ⚠️ The "exactly once" half is what makes calling this every frame legal. The
/// editor's play loop now publishes the served view size on every frame, so a
/// signal that re-fired on an unchanged size would re-run every listener's
/// layout forever — Godot delivers this from `NOTIFICATION_WM_SIZE_CHANGED`, an
/// event, so a per-frame emit would be a parity bug rather than mere noise.
#[test]
fn size_changed_fires_once_per_real_resize() {
    let mut ml = booted();
    let count = Arc::new(AtomicUsize::new(0));

    let root = ml.tree().root_id();
    // ⚠️ `object_id()` lives on `NodeId`, not on `Node` — `get_node()` hands
    // back a `&Node`, which does not have it.
    let target = root.object_id();
    let seen = Arc::clone(&count);
    ml.tree_mut().connect_signal(
        root,
        "size_changed",
        gdobject::signal::Connection::with_callback(target, "_on_size_changed", move |_| {
            seen.fetch_add(1, Ordering::SeqCst);
            Variant::Nil
        }),
    );

    ml.set_viewport_size(1280, 720);
    assert_eq!(
        count.load(Ordering::SeqCst),
        1,
        "a real resize signals once"
    );

    // The same size again — what every subsequent frame delivers.
    ml.set_viewport_size(1280, 720);
    ml.set_viewport_size(1280, 720);
    assert_eq!(
        count.load(Ordering::SeqCst),
        1,
        "an unchanged size must not re-emit, or the per-frame call would fire \
         the signal on every frame forever"
    );

    // A second real resize signals again.
    ml.set_viewport_size(800, 600);
    assert_eq!(count.load(Ordering::SeqCst), 2);
}

/// The stretch scale follows the resize, which is the point of the signal: a
/// listener that reflowed on `size_changed` would otherwise read the boot-time
/// transform.
#[test]
fn the_stretch_scale_follows_the_resize() {
    let mut ml = booted();
    ml.set_viewport_size(1280, 720);

    // 640x360 base in a 1280x720 window with `keep` is an exact 2x, with no
    // letterbox bars — the acceptance's headline number.
    // Scale is the blit rect over the canvas space; both are documented fields,
    // read the same way the sibling `root_window_state_reaches_scripts` test
    // reads them.
    let result = ml.tree().root_window().content_scale();
    let scale = result.screen_size.x / result.size_override.x;
    assert!(
        (scale - 2.0).abs() < 1e-5,
        "a 640x360 base at 1280x720 is a 2x stretch, got {scale} (screen {:?} / canvas {:?})",
        result.screen_size,
        result.size_override
    );
    assert!(
        result.margin.x.abs() < 1e-5 && result.margin.y.abs() < 1e-5,
        "an exact aspect match must produce no letterbox bars, got {:?}",
        result.margin
    );
}
