//! pat-s3rrv.86 item (4) — `scroll_ignore_camera_zoom` uses Godot's real formula.
//!
//! ⚠️ A NEW FILE ON PURPOSE. Appending to `parallax_api.rs` would adopt its
//! tests as this work's gate, and nextest cancels on the first failure — the
//! mechanism that parked two beads earlier today on reds their author had not
//! written.
//!
//! Godot (`scene/2d/parallax_background.cpp:104-108`):
//!
//! ```text
//! if (ignore_camera_zoom)
//!     set_base_offset_and_scale((scroll_ofs + screen_offset * (scale - 1)) / scale, 1.0);
//! else
//!     set_base_offset_and_scale(scroll_ofs, scale);
//! ```
//!
//! The implementation kept only the `1.0` and passed the offset through
//! untouched, dropping both the division and the `screen_offset` term.
//!
//! Expected values come from re-transcribing that expression plus
//! `compute_offset_scaled` into Python and RUNNING it, not from printing what
//! this implementation returns.

use gdcore::math::Vector2;
use gdserver2d::{ParallaxBackground, ParallaxLayer};

/// scroll 200px right, a half-speed layer, and a 640x360 viewport — so Godot's
/// `screen_offset` under the default `ANCHOR_MODE_DRAG_CENTER` is (320, 180).
fn fixture() -> (ParallaxBackground, ParallaxLayer, Vector2) {
    let mut bg = ParallaxBackground::new();
    bg.scroll_offset = Vector2::new(200.0, 0.0);
    bg.scroll_ignore_camera_zoom = true;
    (
        bg,
        ParallaxLayer::new(Vector2::new(0.5, 0.5)),
        Vector2::new(320.0, 180.0),
    )
}

fn approx(got: Vector2, x: f32, y: f32, what: &str) {
    assert!(
        (got.x - x).abs() < 1e-3 && (got.y - y).abs() < 1e-3,
        "{what}: got ({}, {}), expected ({x}, {y})",
        got.x,
        got.y
    );
}

/// 🔑 AT ZOOM 1 THE OLD AND NEW FORMULAS AGREE — which is exactly why the bug
/// survived.
///
/// `(ofs + screen*(1-1)) / 1 == ofs`. Any test that never zooms passes under
/// both. This pins the agreement so the fix cannot be blamed for changing the
/// common case.
#[test]
fn zoom_one_is_unchanged_by_the_correction() {
    let (bg, layer, screen) = fixture();
    approx(
        bg.layer_offset_with_screen(&layer, 1.0, screen),
        100.0,
        0.0,
        "zoom 1 must still be base_offset * motion_scale",
    );
}

/// The zoomed cases, where the dropped terms actually show.
///
/// Old behaviour returned (100, 0) at EVERY zoom, because it passed the base
/// offset through untouched. The y component is the clearest tell: it was
/// always 0 and must now move, since `screen_offset.y` is non-zero.
#[test]
fn zooming_applies_the_division_and_the_screen_offset_term() {
    let (bg, layer, screen) = fixture();

    approx(
        bg.layer_offset_with_screen(&layer, 2.0, screen),
        130.0,
        45.0,
        "zoom 2: (200 + 320*1)/2 * 0.5 = 130, and (0 + 180*1)/2 * 0.5 = 45",
    );
    approx(
        bg.layer_offset_with_screen(&layer, 0.5, screen),
        40.0,
        -90.0,
        "zoom 0.5: (200 + 320*-0.5)/0.5 * 0.5 = 40, (0 + 180*-0.5)/0.5 * 0.5 = -90",
    );
}

/// ⚠️ `screen_offset` is ZERO under `ANCHOR_MODE_FIXED_TOP_LEFT`
/// (`scene/2d/camera_2d.cpp:62`), leaving a pure division.
///
/// Pinned separately because it is a DIFFERENT value from both the old
/// behaviour (100) and the drag-centre result (130) — so a port that ignored
/// the anchor mode and always used half the screen would pass the test above
/// and fail here.
#[test]
fn a_zero_screen_offset_leaves_a_pure_division() {
    let (bg, layer, _) = fixture();
    approx(
        bg.layer_offset_with_screen(&layer, 2.0, Vector2::ZERO),
        50.0,
        0.0,
        "zoom 2 with no screen offset: 200/2 * 0.5 = 50",
    );
}

/// The two-argument convenience method must BE the zero-screen-offset case.
///
/// It keeps every existing caller compiling, so its contract has to be explicit:
/// it is Godot's FIXED_TOP_LEFT anchor mode, not a third behaviour.
#[test]
fn layer_offset_is_the_zero_screen_offset_case() {
    let (bg, layer, _) = fixture();
    for zoom in [0.5f32, 1.0, 2.0] {
        assert_eq!(
            bg.layer_offset(&layer, zoom),
            bg.layer_offset_with_screen(&layer, zoom, Vector2::ZERO),
            "layer_offset must delegate with a zero screen offset (zoom {zoom})"
        );
    }
}

/// The control: with `scroll_ignore_camera_zoom` OFF, nothing changed.
///
/// Without this, the assertions above would still pass if the correction had
/// leaked into the normal path and altered ordinary zoomed scrolling.
#[test]
fn the_normal_path_is_untouched_by_the_correction() {
    let (mut bg, layer, screen) = fixture();
    bg.scroll_ignore_camera_zoom = false;
    for zoom in [0.5f32, 1.0, 2.0] {
        approx(
            bg.layer_offset_with_screen(&layer, zoom, screen),
            100.0,
            0.0,
            "ignore-zoom OFF must be unaffected by screen_offset or zoom here",
        );
    }
}
