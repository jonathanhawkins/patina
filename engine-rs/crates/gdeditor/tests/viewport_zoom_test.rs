//! pat-8vphe: viewport wheel zoom + zoom controls + reset-to-100%.
//!
//! Exercises the public `ViewportCamera2D` zoom API: wheel zoom scales toward
//! the cursor within min/max bounds, the zoom buttons step in/out, and reset
//! restores 100% zoom.

use gdcore::math::Vector2;
use gdeditor::viewport_2d::ViewportCamera2D;

#[test]
fn viewport_zoom() {
    let mut cam = ViewportCamera2D::new();
    let viewport = Vector2::new(800.0, 600.0);

    assert_eq!(cam.zoom_level(), 1.0);
    assert_eq!(cam.zoom_percent(), 100.0);

    // Wheel zoom in (positive delta) toward an off-center cursor increases the
    // zoom and anchors at the cursor (the pan offset shifts).
    let cursor = Vector2::new(600.0, 200.0);
    cam.zoom_at(1.0, cursor, viewport);
    assert!(cam.zoom_level() > 1.0, "wheel zoom in increases zoom");
    assert_ne!(
        cam.offset,
        Vector2::ZERO,
        "zoom is anchored at the cursor, shifting the offset"
    );

    // Wheel zoom out (negative delta) decreases the zoom.
    let zoomed_in = cam.zoom_level();
    cam.zoom_at(-1.0, cursor, viewport);
    assert!(cam.zoom_level() < zoomed_in, "wheel zoom out decreases zoom");

    // Zoom stays within the valid [min, max] range no matter how far it scrolls.
    for _ in 0..400 {
        cam.zoom_at(1.0, cursor, viewport);
    }
    assert!(cam.zoom_level() <= cam.zoom_max, "zoom clamps to the maximum");
    for _ in 0..800 {
        cam.zoom_at(-1.0, cursor, viewport);
    }
    assert!(cam.zoom_level() >= cam.zoom_min, "zoom clamps to the minimum");

    // The zoom buttons step in and out by one increment.
    cam.reset();
    cam.zoom_in(viewport);
    assert!(cam.zoom_level() > 1.0, "the zoom-in button steps in");
    cam.zoom_out(viewport);
    assert!(
        (cam.zoom_level() - 1.0).abs() < 1e-6,
        "the zoom-out button reverses a centered zoom-in"
    );

    // Reset restores 100% zoom and recenters the view.
    cam.zoom_in(viewport);
    cam.zoom_in(viewport);
    assert_ne!(cam.zoom_level(), 1.0);
    cam.reset();
    assert_eq!(cam.zoom_level(), 1.0, "reset restores 100% zoom");
    assert_eq!(cam.zoom_percent(), 100.0);
    assert_eq!(cam.offset, Vector2::ZERO, "reset recenters the view");
}
