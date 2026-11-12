//! pat-0q2kx.192 — the parallax types must be reachable from another crate.
//!
//! `gdserver2d::parallax` holds a working implementation — `ParallaxLayer`'s
//! `compute_offset`/`compute_offset_scaled` and `ParallaxBackground`'s
//! `base_offset`/`layer_offset` — with 11 unit tests. What it does **not** have
//! is a caller: nothing outside that module computes a parallax offset, which is
//! why the bead's status is UNWIRED even though the maths is present and correct.
//!
//! `ParallaxBackground` was also **absent from the crate's re-exports** while
//! `ParallaxLayer` was present, so a consumer could name a layer but not the
//! background that drives it — an obstacle for the render-path wiring, which has
//! to live in `gdscene`/`gdeditor`.
//!
//! This is an **integration** test on purpose: it can only compile if both types
//! are genuinely public, so it locks the export that the in-module unit tests
//! (which see private items) cannot.
//!
//! ⚠️ It asserts the maths and reachability, **not** that any scene renders with
//! parallax. Nothing folds these offsets into a draw transform yet.

use gdcore::math::Vector2;
use gdserver2d::{ParallaxBackground, ParallaxLayer};

/// Both types are nameable from outside the crate. The `use` above is half the
/// assertion; constructing them is the other half.
#[test]
fn parallax_types_are_reachable_from_another_crate() {
    let layer = ParallaxLayer::new(Vector2::new(0.5, 1.0));
    assert_eq!(layer.motion_scale, Vector2::new(0.5, 1.0));

    let bg = ParallaxBackground::new();
    assert_eq!(
        bg.scroll_offset,
        Vector2::ZERO,
        "a fresh background starts unscrolled"
    );
}

/// The core parallax relationship: a layer at half scroll scale moves half as
/// far as the camera. This is exactly the acceptance criterion's arithmetic
/// (camera +100 → sprite shifts 50), verified at the maths layer that the render
/// wiring will call.
#[test]
fn half_scroll_scale_moves_half_the_camera_distance() {
    let layer = ParallaxLayer::new(Vector2::new(0.5, 1.0));

    let at_origin = layer.compute_offset(Vector2::ZERO);
    let at_hundred = layer.compute_offset(Vector2::new(100.0, 0.0));

    let dx = at_hundred.x - at_origin.x;
    assert_eq!(
        dx.abs(),
        50.0,
        "a 0.5 scroll scale must yield half the camera's 100px delta, got {dx}"
    );

    // The y axis is at scale 1.0, so it tracks the camera one-for-one.
    let vertical = layer.compute_offset(Vector2::new(0.0, 100.0));
    assert_eq!(
        (vertical.y - at_origin.y).abs(),
        100.0,
        "a 1.0 scroll scale tracks the camera exactly"
    );
}

/// A background composes with its layers, which is the path the render wiring
/// will take: scroll the background, ask it for a layer's offset.
#[test]
fn background_drives_its_layers() {
    let mut bg = ParallaxBackground::new();
    let layer = ParallaxLayer::new(Vector2::new(0.5, 0.5));

    let before = bg.layer_offset(&layer, 1.0);
    bg.scroll_offset = Vector2::new(100.0, 100.0);
    let after = bg.layer_offset(&layer, 1.0);

    assert_ne!(
        before, after,
        "scrolling the background must change what its layers report"
    );
    let moved = (after.x - before.x).abs();
    assert_eq!(
        moved, 50.0,
        "a 0.5-scale layer moves half the background's 100px scroll, got {moved}"
    );
}

/// A layer at scale 1.0 is not parallaxed at all — it pins to the camera. Worth
/// pinning because it is the degenerate case a wiring bug most easily produces.
#[test]
fn unit_scroll_scale_is_not_parallax() {
    let layer = ParallaxLayer::new(Vector2::new(1.0, 1.0));
    let a = layer.compute_offset(Vector2::ZERO);
    let b = layer.compute_offset(Vector2::new(250.0, -75.0));
    assert_eq!((b.x - a.x).abs(), 250.0);
    assert_eq!((b.y - a.y).abs(), 75.0);
}
