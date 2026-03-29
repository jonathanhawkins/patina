//! pat-53ato: Viewport 2D parity tests.
//!
//! Exercises the 2D viewport controller covering:
//! - Tool modes (Select, Move, Rotate, Scale)
//! - Camera pan/zoom with anchor point
//! - Selection with overlap, multi-select, toggle
//! - Locked-node behavior (cannot select locked nodes)
//! - Drag operations for move/rotate/scale
//! - Snap settings for grid, rotation, and scale
//! - Screen-to-world and world-to-screen coordinate conversion

use gdeditor::viewport_2d::*;
use gdcore::math::Vector2;

const VIEWPORT_W: u32 = 800;
const VIEWPORT_H: u32 = 600;

fn make_viewport() -> Viewport2D {
    Viewport2D::new(VIEWPORT_W, VIEWPORT_H)
}

// ===========================================================================
// Tool modes
// ===========================================================================

#[test]
fn tool_mode_default_is_select() {
    let vp = make_viewport();
    assert_eq!(vp.tool_mode, ToolMode2D::Select);
}

#[test]
fn tool_mode_cycles_through_all_modes() {
    let mut vp = make_viewport();
    for mode in [ToolMode2D::Select, ToolMode2D::Move, ToolMode2D::Rotate, ToolMode2D::Scale] {
        vp.set_tool_mode(mode);
        assert_eq!(vp.tool_mode, mode);
    }
}

// ===========================================================================
// Camera: pan
// ===========================================================================

#[test]
fn camera_pan_moves_world_offset() {
    let mut cam = ViewportCamera2D::new();
    cam.begin_pan();
    cam.pan(200.0, 100.0);
    cam.end_pan();
    // Screen-right drag => world offset shifts left (negative x).
    assert!(cam.offset.x < -0.1, "expected offset.x < 0, got {}", cam.offset.x);
    assert!(cam.offset.y < -0.1, "expected offset.y < 0, got {}", cam.offset.y);
}

#[test]
fn camera_pan_only_active_during_gesture() {
    let mut cam = ViewportCamera2D::new();
    cam.pan(999.0, 999.0); // no begin_pan
    assert!((cam.offset.x).abs() < 0.001, "should not move without begin_pan");
}

#[test]
fn camera_pan_at_higher_zoom_moves_less() {
    let mut cam_1x = ViewportCamera2D::new();
    cam_1x.begin_pan();
    cam_1x.pan(100.0, 0.0);
    cam_1x.end_pan();

    let mut cam_2x = ViewportCamera2D::new();
    cam_2x.zoom = 2.0;
    cam_2x.begin_pan();
    cam_2x.pan(100.0, 0.0);
    cam_2x.end_pan();

    // At 2x zoom the same screen delta should produce half the world offset.
    assert!(
        cam_2x.offset.x.abs() < cam_1x.offset.x.abs(),
        "2x zoom pan should move less: 1x={}, 2x={}",
        cam_1x.offset.x,
        cam_2x.offset.x
    );
}

// ===========================================================================
// Camera: zoom
// ===========================================================================

#[test]
fn camera_zoom_in_increases_level() {
    let mut cam = ViewportCamera2D::new();
    let vp = Vector2::new(VIEWPORT_W as f32, VIEWPORT_H as f32);
    cam.zoom_in(vp);
    assert!(cam.zoom > 1.0, "zoom should increase, got {}", cam.zoom);
}

#[test]
fn camera_zoom_out_decreases_level() {
    let mut cam = ViewportCamera2D::new();
    let vp = Vector2::new(VIEWPORT_W as f32, VIEWPORT_H as f32);
    cam.zoom_out(vp);
    assert!(cam.zoom < 1.0, "zoom should decrease, got {}", cam.zoom);
}

#[test]
fn camera_zoom_stays_within_bounds() {
    let mut cam = ViewportCamera2D::new();
    let vp = Vector2::new(VIEWPORT_W as f32, VIEWPORT_H as f32);
    for _ in 0..200 {
        cam.zoom_out(vp);
    }
    assert!(cam.zoom >= cam.zoom_min);
    for _ in 0..400 {
        cam.zoom_in(vp);
    }
    assert!(cam.zoom <= cam.zoom_max);
}

#[test]
fn camera_zoom_at_cursor_anchors_point() {
    let mut cam = ViewportCamera2D::new();
    let vp = Vector2::new(VIEWPORT_W as f32, VIEWPORT_H as f32);
    let cursor = Vector2::new(200.0, 150.0); // upper-left quadrant

    let world_before = cam.screen_to_world(cursor, vp);
    cam.zoom_at(1.0, cursor, vp);
    let world_after = cam.screen_to_world(cursor, vp);

    // The world point under the cursor should stay approximately fixed.
    assert!(
        (world_before.x - world_after.x).abs() < 1.0,
        "anchor x drift: before={}, after={}",
        world_before.x,
        world_after.x
    );
    assert!(
        (world_before.y - world_after.y).abs() < 1.0,
        "anchor y drift: before={}, after={}",
        world_before.y,
        world_after.y
    );
}

// ===========================================================================
// Camera: framing
// ===========================================================================

#[test]
fn camera_frame_rect_centers_on_rect() {
    let mut cam = ViewportCamera2D::new();
    let vp = Vector2::new(VIEWPORT_W as f32, VIEWPORT_H as f32);
    cam.frame_rect(
        Vector2::new(500.0, 500.0),
        Vector2::new(200.0, 200.0),
        vp,
    );
    assert!((cam.offset.x - 500.0).abs() < 0.01);
    assert!((cam.offset.y - 500.0).abs() < 0.01);
    assert!(cam.zoom > 0.0, "zoom should be positive");
}

#[test]
fn camera_reset_returns_to_origin() {
    let mut cam = ViewportCamera2D::new();
    cam.offset = Vector2::new(999.0, 999.0);
    cam.zoom = 5.0;
    cam.reset();
    assert!((cam.offset.x).abs() < 0.001);
    assert!((cam.offset.y).abs() < 0.001);
    assert!((cam.zoom - 1.0).abs() < 0.001);
}

// ===========================================================================
// Coordinate conversion
// ===========================================================================

#[test]
fn screen_to_world_roundtrip_at_various_zooms() {
    for zoom in [0.5, 1.0, 2.0, 4.0] {
        let mut cam = ViewportCamera2D::new();
        cam.zoom = zoom;
        cam.offset = Vector2::new(100.0, 200.0);
        let vp = Vector2::new(VIEWPORT_W as f32, VIEWPORT_H as f32);

        let screen = Vector2::new(123.0, 456.0);
        let world = cam.screen_to_world(screen, vp);
        let back = cam.world_to_screen(world, vp);
        assert!(
            (back.x - screen.x).abs() < 0.01 && (back.y - screen.y).abs() < 0.01,
            "roundtrip failed at zoom={}: screen={:?} -> world={:?} -> back={:?}",
            zoom, screen, world, back
        );
    }
}

// ===========================================================================
// Selection: basic
// ===========================================================================

#[test]
fn selection_single_replaces() {
    let mut sel = Selection2D::new();
    sel.select(1);
    sel.select(2);
    assert_eq!(sel.count(), 1);
    assert_eq!(sel.primary(), Some(2));
    assert!(!sel.is_selected(1));
}

#[test]
fn selection_add_multi() {
    let mut sel = Selection2D::new();
    sel.select(1);
    sel.add_to_selection(2);
    sel.add_to_selection(3);
    assert_eq!(sel.count(), 3);
    assert_eq!(sel.primary(), Some(3));
}

#[test]
fn selection_toggle_on_off() {
    let mut sel = Selection2D::new();
    sel.select(1);
    sel.toggle_selection(1);
    assert_eq!(sel.count(), 0);
    sel.toggle_selection(1);
    assert_eq!(sel.count(), 1);
}

#[test]
fn selection_clear() {
    let mut sel = Selection2D::new();
    sel.select(1);
    sel.add_to_selection(2);
    sel.clear();
    assert_eq!(sel.count(), 0);
    assert!(sel.primary().is_none());
}

// ===========================================================================
// Selection: overlap (rectangle) selection
// ===========================================================================

#[test]
fn overlap_selection_picks_intersecting_nodes() {
    let mut sel = Selection2D::new();
    let candidates = vec![
        (1, SelectionRect::new(Vector2::new(0.0, 0.0), Vector2::new(10.0, 10.0))),
        (2, SelectionRect::new(Vector2::new(20.0, 20.0), Vector2::new(30.0, 30.0))),
        (3, SelectionRect::new(Vector2::new(5.0, 5.0), Vector2::new(15.0, 15.0))),
    ];
    // Drag rect covers [0,0]-[12,12], should pick 1 and 3 but not 2.
    let rect = SelectionRect::new(Vector2::new(0.0, 0.0), Vector2::new(12.0, 12.0));
    sel.select_overlap(&rect, &candidates);
    assert!(sel.is_selected(1));
    assert!(!sel.is_selected(2));
    assert!(sel.is_selected(3));
    assert_eq!(sel.count(), 2);
}

#[test]
fn overlap_selection_empty_rect_selects_nothing() {
    let mut sel = Selection2D::new();
    let candidates = vec![
        (1, SelectionRect::new(Vector2::new(0.0, 0.0), Vector2::new(10.0, 10.0))),
    ];
    let rect = SelectionRect::new(Vector2::new(100.0, 100.0), Vector2::new(100.1, 100.1));
    sel.select_overlap(&rect, &candidates);
    assert_eq!(sel.count(), 0);
}

// ===========================================================================
// Selection: locked-node behavior
// ===========================================================================

#[test]
fn locked_node_cannot_be_click_selected() {
    let mut sel = Selection2D::new();
    sel.lock(42);
    assert!(!sel.select(42));
    assert_eq!(sel.count(), 0);
}

#[test]
fn locked_node_excluded_from_overlap_selection() {
    let mut sel = Selection2D::new();
    sel.lock(2);
    let candidates = vec![
        (1, SelectionRect::new(Vector2::new(0.0, 0.0), Vector2::new(5.0, 5.0))),
        (2, SelectionRect::new(Vector2::new(0.0, 0.0), Vector2::new(5.0, 5.0))),
    ];
    let rect = SelectionRect::new(Vector2::new(0.0, 0.0), Vector2::new(10.0, 10.0));
    sel.select_overlap(&rect, &candidates);
    assert!(sel.is_selected(1));
    assert!(!sel.is_selected(2));
}

#[test]
fn locking_removes_node_from_current_selection() {
    let mut sel = Selection2D::new();
    sel.select(1);
    sel.add_to_selection(2);
    assert_eq!(sel.count(), 2);
    sel.lock(1);
    assert!(!sel.is_selected(1));
    assert_eq!(sel.count(), 1);
}

#[test]
fn unlock_allows_selection_again() {
    let mut sel = Selection2D::new();
    sel.lock(1);
    assert!(!sel.select(1));
    sel.unlock(1);
    assert!(sel.select(1));
    assert_eq!(sel.count(), 1);
}

#[test]
fn locked_node_add_to_selection_returns_false() {
    let mut sel = Selection2D::new();
    sel.lock(5);
    assert!(!sel.add_to_selection(5));
}

// ===========================================================================
// Drag operations (move, rotate, scale)
// ===========================================================================

#[test]
fn drag_move_accumulates_delta() {
    let mut sel = Selection2D::new();
    sel.select(1);
    sel.begin_drag(ToolMode2D::Move, Vector2::new(10.0, 20.0));
    assert!(sel.is_dragging());
    sel.update_drag(Vector2::new(30.0, 50.0));
    let state = sel.drag_state().unwrap();
    assert_eq!(state.mode, ToolMode2D::Move);
    assert!((state.delta.x - 20.0).abs() < 0.01);
    assert!((state.delta.y - 30.0).abs() < 0.01);
    let delta = sel.end_drag().unwrap();
    assert!((delta.x - 20.0).abs() < 0.01);
    assert!(!sel.is_dragging());
}

#[test]
fn drag_rotate_reports_correct_mode() {
    let mut sel = Selection2D::new();
    sel.begin_drag(ToolMode2D::Rotate, Vector2::new(0.0, 0.0));
    assert_eq!(sel.drag_state().unwrap().mode, ToolMode2D::Rotate);
    sel.end_drag();
}

#[test]
fn drag_scale_reports_correct_mode() {
    let mut sel = Selection2D::new();
    sel.begin_drag(ToolMode2D::Scale, Vector2::new(0.0, 0.0));
    assert_eq!(sel.drag_state().unwrap().mode, ToolMode2D::Scale);
    sel.end_drag();
}

#[test]
fn end_drag_without_begin_returns_none() {
    let mut sel = Selection2D::new();
    assert!(sel.end_drag().is_none());
}

// ===========================================================================
// Snap settings
// ===========================================================================

#[test]
fn snap_disabled_is_passthrough() {
    let snap = SnapSettings2D::default();
    assert!(!snap.enabled);
    let pos = Vector2::new(3.14, 2.71);
    assert_eq!(snap.snap_position(pos), pos);
}

#[test]
fn snap_position_rounds_to_grid() {
    let snap = SnapSettings2D {
        enabled: true,
        grid_step: Vector2::new(16.0, 16.0),
        ..Default::default()
    };
    let snapped = snap.snap_position(Vector2::new(25.0, 7.0));
    assert!((snapped.x - 32.0).abs() < 0.01); // 25 rounds to 32 (1.5625 * 16)
    assert!((snapped.y - 0.0).abs() < 0.01);  // 7 rounds to 0
}

#[test]
fn snap_rotation_rounds_to_step() {
    let snap = SnapSettings2D {
        enabled: true,
        rotation_step: std::f32::consts::FRAC_PI_4, // 45 degrees
        ..Default::default()
    };
    // 60 degrees (1.047 rad) rounds to 45 degrees (0.785 rad)
    let snapped = snap.snap_rotation(1.047);
    assert!((snapped - std::f32::consts::FRAC_PI_4).abs() < 0.01);
}

#[test]
fn snap_scale_rounds_to_step() {
    let snap = SnapSettings2D {
        enabled: true,
        scale_step: 0.25,
        ..Default::default()
    };
    let snapped = snap.snap_scale(1.37);
    assert!((snapped - 1.25).abs() < 0.01);
}

// ===========================================================================
// Viewport2D integration
// ===========================================================================

#[test]
fn viewport_scroll_zooms_in() {
    let mut vp = make_viewport();
    let center = Vector2::new(400.0, 300.0);
    vp.on_scroll(1.0, center);
    assert!(vp.camera.zoom > 1.0);
}

#[test]
fn viewport_pan_via_mouse_drag() {
    let mut vp = make_viewport();
    vp.camera.begin_pan();
    vp.on_mouse_drag(50.0, 30.0);
    vp.camera.end_pan();
    assert!(vp.camera.offset.x < 0.0);
}

#[test]
fn viewport_frame_selection() {
    let mut vp = make_viewport();
    vp.frame_selection(
        Vector2::new(200.0, 200.0),
        Vector2::new(100.0, 100.0),
    );
    assert!((vp.camera.offset.x - 200.0).abs() < 0.01);
}

#[test]
fn viewport_grid_visibility_toggles() {
    let mut vp = make_viewport();
    assert!(vp.grid_visible);
    vp.grid_visible = false;
    assert!(!vp.grid_visible);
}

#[test]
fn viewport_aspect_ratio() {
    let vp = Viewport2D::new(1920, 1080);
    let ar = vp.aspect_ratio();
    assert!((ar - 16.0 / 9.0).abs() < 0.01);
}
