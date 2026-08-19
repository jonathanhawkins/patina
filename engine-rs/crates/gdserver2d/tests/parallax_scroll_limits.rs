//! pat-s3rrv.86 item (3) — `scroll_limit_begin` / `scroll_limit_end` clamp the
//! scroll position, using Godot's negate-clamp-negate order.
//!
//! ⚠️ A NEW FILE ON PURPOSE, like the ignore-camera-zoom tests: appending to an
//! existing binary would adopt its tests as this work's gate, and nextest
//! cancels on the first failure.
//!
//! Godot (`scene/2d/parallax_background.cpp:78-95`, inside `_update_scroll`):
//!
//! ```text
//! scroll_ofs = -scroll_ofs;
//! if (limit_begin.x < limit_end.x) {
//!     if (scroll_ofs.x < limit_begin.x)             scroll_ofs.x = limit_begin.x;
//!     else if (scroll_ofs.x + vps.x > limit_end.x)  scroll_ofs.x = limit_end.x - vps.x;
//! }
//! ... same for y ...
//! scroll_ofs = -scroll_ofs;
//! ```
//!
//! Expected values come from re-transcribing that block into Python and running
//! it, not from printing what this implementation returns.

use gdcore::math::Vector2;
use gdserver2d::{ParallaxBackground, ParallaxLayer, ScrollLimits};

/// A deliberately ASYMMETRIC range: `begin` is negative on both axes, `end` is
/// positive, and the viewport is not square.
///
/// Symmetric or origin-centred limits are a coincidence point — they read the
/// same whether or not the negation is applied, and a viewport of zero hides
/// the `end - viewport_size` term entirely.
fn limits() -> ScrollLimits {
    ScrollLimits {
        begin: Vector2::new(-100.0, -50.0),
        end: Vector2::new(700.0, 350.0),
        viewport_size: Vector2::new(640.0, 200.0),
    }
}

/// `scroll_base_scale` stays at ONE so `base_offset()` is exactly the scroll
/// position — this file is about the clamp, not about that product.
fn background_at(scroll: Vector2) -> ParallaxBackground {
    let mut bg = ParallaxBackground::new();
    bg.scroll_offset = scroll;
    bg
}

fn approx(got: Vector2, x: f32, y: f32, what: &str) {
    assert!(
        (got.x - x).abs() < 1e-3 && (got.y - y).abs() < 1e-3,
        "{what}: got ({}, {}), expected ({x}, {y})",
        got.x,
        got.y
    );
}

/// A scroll position inside the range passes through untouched.
///
/// Without this, every assertion below would still pass if the clamp simply
/// pinned the offset to a limit unconditionally.
#[test]
fn a_scroll_position_within_the_limits_is_unchanged() {
    let bg = background_at(Vector2::new(-30.0, -20.0));
    approx(
        bg.clamped_base_offset(&limits()),
        -30.0,
        -20.0,
        "in-range scroll must not be touched",
    );
}

/// 🔑 THE NEGATION. A POSITIVE scroll offset hits the `begin` limit, because the
/// scroll offset moves opposite to the camera.
///
/// This is the assertion that separates Godot's order from the direct-looking
/// port. Clamping `base_offset` itself would leave x at 60 here (it is inside
/// `[-100, 700]` but past `end - viewport`), not 100 — a plausible number that
/// holds at the wrong end of the level.
#[test]
fn a_positive_offset_is_clamped_by_the_begin_limit() {
    let bg = background_at(Vector2::new(200.0, 100.0));
    approx(
        bg.clamped_base_offset(&limits()),
        100.0,
        50.0,
        "-200 < begin.x -100 => -100, negated back to +100",
    );
}

/// The far end, where the `end - viewport_size` term shows.
///
/// x stops at `700 - 640 = 60` and y at `350 - 200 = 150` — both differ from
/// the raw limits, so an implementation that clamped to `end` directly fails
/// here while passing the `begin` case above.
#[test]
fn the_end_limit_is_measured_at_the_far_edge_of_the_viewport() {
    let bg = background_at(Vector2::new(-500.0, -400.0));
    approx(
        bg.clamped_base_offset(&limits()),
        -60.0,
        -150.0,
        "500 + 640 > end.x 700 => 60; 400 + 200 > end.y 350 => 150",
    );
}

/// ⚠️ EQUAL LIMITS DISABLE THE AXIS — they do not pin the scroll to zero.
///
/// Godot's default is `limit_begin == limit_end == (0, 0)`, so reading the
/// defaults as an active `0..0` range would freeze every parallax background in
/// a project — a total failure that no in-range test would ever reach, since
/// with clamping "on" nothing is ever in range.
#[test]
fn default_limits_disable_clamping_entirely() {
    let bg = background_at(Vector2::new(200.0, 100.0));
    approx(
        bg.clamped_base_offset(&ScrollLimits::default()),
        200.0,
        100.0,
        "equal begin/end must disable, not pin to zero",
    );
    approx(
        bg.clamped_base_offset(&ScrollLimits::default()),
        bg.base_offset().x,
        bg.base_offset().y,
        "with limits disabled the clamp must be exactly base_offset",
    );
}

/// The axes are independent: `begin < end` is tested per-axis.
///
/// x is limited and y is not, so a scalar guard covering both axes at once
/// fails here while passing every test above.
#[test]
fn one_axis_can_be_limited_while_the_other_is_free() {
    let bg = background_at(Vector2::new(200.0, 100.0));
    let x_only = ScrollLimits {
        begin: Vector2::new(-100.0, 0.0),
        end: Vector2::new(700.0, 0.0),
        viewport_size: Vector2::new(640.0, 200.0),
    };
    approx(
        bg.clamped_base_offset(&x_only),
        100.0,
        100.0,
        "x clamps to the begin limit while y passes through unclamped",
    );
}

/// The clamp must actually reach the LAYER, not just the accessor.
///
/// A clamp that stops at `clamped_base_offset` would be invisible in every
/// rendered pixel — the failure this whole audit keeps finding.
#[test]
fn the_clamped_offset_is_what_layers_are_drawn_at() {
    let bg = background_at(Vector2::new(-500.0, -400.0));
    let layer = ParallaxLayer::new(Vector2::new(0.5, 0.5));

    approx(
        bg.layer_offset_limited(&layer, 1.0, Vector2::ZERO, &limits()),
        -30.0,
        -75.0,
        "the clamped (-60, -150) times motion_scale 0.5",
    );
    approx(
        bg.layer_offset(&layer, 1.0),
        -250.0,
        -200.0,
        "unlimited path still uses the raw (-500, -400)",
    );
}

/// ⚠️ The limits bound the SCROLL POSITION, not each layer's offset.
///
/// A layer at half speed moves half of the clamped range; it does not stop at
/// the limit itself. Pinned because clamping per-layer instead would leave
/// distant layers visibly tied to the near ones at the level edges.
#[test]
fn a_slow_layer_moves_a_fraction_of_the_clamped_range() {
    let slow = ParallaxLayer::new(Vector2::new(0.25, 0.25));
    let fast = ParallaxLayer::new(Vector2::new(1.0, 1.0));
    let bg = background_at(Vector2::new(-500.0, -400.0));

    approx(
        bg.layer_offset_limited(&slow, 1.0, Vector2::ZERO, &limits()),
        -15.0,
        -37.5,
        "quarter-speed layer gets a quarter of the clamped offset",
    );
    approx(
        bg.layer_offset_limited(&fast, 1.0, Vector2::ZERO, &limits()),
        -60.0,
        -150.0,
        "a full-speed layer sits exactly at the clamped scroll position",
    );
}

/// The pre-existing entry points are the disabled-limits case, unchanged.
///
/// This is the compatibility contract: adding the clamp must not have moved any
/// caller that does not opt in.
#[test]
fn the_unlimited_entry_points_are_the_disabled_limits_case() {
    let mut bg = background_at(Vector2::new(200.0, 100.0));
    let layer = ParallaxLayer::new(Vector2::new(0.5, 0.5));
    let screen = Vector2::new(320.0, 180.0);

    for ignore_zoom in [false, true] {
        bg.scroll_ignore_camera_zoom = ignore_zoom;
        for zoom in [0.5f32, 1.0, 2.0] {
            assert_eq!(
                bg.layer_offset_with_screen(&layer, zoom, screen),
                bg.layer_offset_limited(&layer, zoom, screen, &ScrollLimits::default()),
                "zoom {zoom}, ignore_camera_zoom {ignore_zoom}"
            );
        }
    }
}
