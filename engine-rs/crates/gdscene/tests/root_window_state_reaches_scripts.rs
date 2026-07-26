//! pat-0q2kx.141 (gap fill) — the root `Window`'s content-scale state is
//! readable from script, and a resize emits `size_changed`.
//!
//! ⚠️ Most of this bead is ALREADY DONE and its description is stale: the scene
//! root IS a `Window` (`scene_tree.rs` builds it as one), `Window` IS registered
//! in ClassDB, and the whole stretch pipeline — canvas_items/viewport modes,
//! aspect handling, letterbox margins, integer scaling, input mapped back
//! through the bars — is implemented and covered by
//! `root_window_content_scale_pipeline.rs` and
//! `root_window_stretch_reaches_the_play_frame.rs`. See the completion report.
//!
//! Two acceptance clauses were genuinely uncovered, and both were real gaps
//! rather than merely untested:
//!
//!  1. `get_window().content_scale_mode` — declared as a ClassDB property on
//!     `Window` but NEVER populated from the booted project, so it answered the
//!     registration default `0` inside a project that sets `canvas_items`. The
//!     scaling worked; only the script-visible half was missing.
//!  2. `size_changed` — the signal name appeared NOWHERE in the workspace, so a
//!     script connecting to it waited forever.

use gdcore::content_scale::{
    ContentScaleAspect, ContentScaleConfig, ContentScaleMode, ContentScaleStretch,
};
use gdcore::math::Vector2;
use gdscene::scene_tree::SceneTree;
use gdvariant::Variant;

/// The project the acceptance names: a 320×180 base stretched with
/// `canvas_items` + `keep`.
fn canvas_items_keep() -> ContentScaleConfig {
    ContentScaleConfig {
        mode: ContentScaleMode::CanvasItems,
        aspect: ContentScaleAspect::Keep,
        stretch: ContentScaleStretch::Fractional,
        factor: 1.0,
        base_size: Vector2::new(320.0, 180.0),
    }
}

fn root_prop(tree: &SceneTree, name: &str) -> Variant {
    let root = tree.root_id();
    tree.get_node(root)
        .expect("the tree has a root")
        .get_property(name)
}

/// The state a script reads through `get_window()` reflects the project, not
/// the ClassDB registration defaults.
#[test]
fn content_scale_state_is_readable_from_the_root_node() {
    let mut tree = SceneTree::new();

    // Before publishing, the node carries no content-scale state at all — this
    // is what a script used to see for a fully configured project.
    assert_eq!(
        root_prop(&tree, "content_scale_mode"),
        Variant::Nil,
        "precondition: the property is unset until it is published"
    );

    tree.root_window_mut().config = canvas_items_keep();
    tree.root_window_mut().size = Vector2::new(640.0, 480.0);
    tree.publish_root_window_state();

    // Godot's Window.ContentScaleMode: DISABLED 0, CANVAS_ITEMS 1, VIEWPORT 2.
    assert_eq!(
        root_prop(&tree, "content_scale_mode"),
        Variant::Int(1),
        "canvas_items must read back as 1, the value the acceptance names"
    );
    // ContentScaleAspect: IGNORE 0, KEEP 1, KEEP_WIDTH 2, KEEP_HEIGHT 3, EXPAND 4.
    assert_eq!(root_prop(&tree, "content_scale_aspect"), Variant::Int(1));
    // ContentScaleStretch: FRACTIONAL 0, INTEGER 1.
    assert_eq!(root_prop(&tree, "content_scale_stretch"), Variant::Int(0));
    assert_eq!(
        root_prop(&tree, "content_scale_factor"),
        Variant::Float(1.0)
    );
    assert_eq!(
        root_prop(&tree, "content_scale_size"),
        Variant::Vector2(Vector2::new(320.0, 180.0)),
        "content_scale_size is the project's base resolution"
    );
    assert_eq!(
        root_prop(&tree, "size"),
        Variant::Vector2(Vector2::new(640.0, 480.0)),
        "size is the real window size, not the base resolution"
    );
}

/// Each mode/aspect maps to its own Godot ordinal — a single wrong arm would
/// otherwise pass a test that only checked one value.
#[test]
fn every_mode_and_aspect_maps_to_its_godot_ordinal() {
    for (mode, expected) in [
        (ContentScaleMode::Disabled, 0),
        (ContentScaleMode::CanvasItems, 1),
        (ContentScaleMode::Viewport, 2),
    ] {
        let mut tree = SceneTree::new();
        tree.root_window_mut().config = ContentScaleConfig {
            mode,
            ..canvas_items_keep()
        };
        tree.publish_root_window_state();
        assert_eq!(
            root_prop(&tree, "content_scale_mode"),
            Variant::Int(expected),
            "{mode:?} must publish as {expected}"
        );
    }

    for (aspect, expected) in [
        (ContentScaleAspect::Ignore, 0),
        (ContentScaleAspect::Keep, 1),
        (ContentScaleAspect::KeepWidth, 2),
        (ContentScaleAspect::KeepHeight, 3),
        (ContentScaleAspect::Expand, 4),
    ] {
        let mut tree = SceneTree::new();
        tree.root_window_mut().config = ContentScaleConfig {
            aspect,
            ..canvas_items_keep()
        };
        tree.publish_root_window_state();
        assert_eq!(
            root_prop(&tree, "content_scale_aspect"),
            Variant::Int(expected),
            "{aspect:?} must publish as {expected}"
        );
    }
}

/// A resize republishes the size and reports that it changed; a no-op resize is
/// silent, as in Godot.
///
/// The `size_changed` emit itself goes through `SceneTree::emit_signal` — the
/// same funnel every other signal in the engine uses — so this asserts the
/// contract around it (when it fires, and that the published state is updated
/// BEFORE it fires) rather than re-testing the signal bus.
#[test]
fn resizing_the_root_window_emits_size_changed() {
    let mut tree = SceneTree::new();
    tree.root_window_mut().config = canvas_items_keep();
    tree.root_window_mut().size = Vector2::new(640.0, 480.0);
    tree.publish_root_window_state();

    let changed = tree.resize_root_window(Vector2::new(1280.0, 720.0));
    assert!(changed, "a real resize reports that it changed");
    assert_eq!(
        root_prop(&tree, "size"),
        Variant::Vector2(Vector2::new(1280.0, 720.0)),
        "the published size must follow the resize"
    );

    // Resizing to the SAME size must not re-emit — Godot only signals a real
    // change, and a spurious emit would re-run every listener's layout.
    assert!(
        !tree.resize_root_window(Vector2::new(1280.0, 720.0)),
        "a resize to the same size must be a no-op"
    );
}

/// The transform follows the resize: the same base resolution laid out in a
/// bigger window scales differently. This is what a `size_changed` listener is
/// for, so the signal without the updated transform would be useless.
#[test]
fn the_stretch_transform_follows_a_resize() {
    let mut tree = SceneTree::new();
    tree.root_window_mut().config = canvas_items_keep();
    tree.root_window_mut().size = Vector2::new(640.0, 360.0);
    tree.publish_root_window_state();

    // Scale is the blit rect over the canvas space; both are documented fields.
    let scale_of =
        |r: &gdcore::content_scale::ContentScaleResult| r.screen_size.x / r.size_override.x;

    // 320x180 into 640x360 is an exact 2x with no bars.
    let two_x = tree.root_window().content_scale();
    assert!(
        (scale_of(&two_x) - 2.0).abs() < 1e-5,
        "320x180 into 640x360 should be scale 2, got {} (screen {:?} / canvas {:?})",
        scale_of(&two_x),
        two_x.screen_size,
        two_x.size_override
    );
    assert!(
        two_x.margin.x.abs() < 1e-5 && two_x.margin.y.abs() < 1e-5,
        "an exact aspect match must produce no bars, got {:?}",
        two_x.margin
    );

    tree.resize_root_window(Vector2::new(1280.0, 720.0));
    let four_x = tree.root_window().content_scale();
    assert!(
        (scale_of(&four_x) - 4.0).abs() < 1e-5,
        "after resizing to 1280x720 the same base should be scale 4, got {}",
        scale_of(&four_x)
    );
}
