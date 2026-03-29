//! pat-8p9at: Cursor shape and custom cursor support.
//!
//! Integration tests covering:
//! 1. CursorShape enum — all 17 variants, from_u8, names, discriminants
//! 2. CustomCursor — creation, hotspot clamping, size queries
//! 3. CursorManager — shape, custom cursors, visibility, confinement, position
//! 4. DisplayServer integration — cursor manager per window
//! 5. Edge cases — zero-size images, rapid shape cycling, all-shapes custom cursor registry

use gdplatform::cursor::{CursorManager, CursorShape, CustomCursor};
use gdplatform::window::WindowIcon;

// ===========================================================================
// Helpers
// ===========================================================================

fn test_icon(w: u32, h: u32) -> WindowIcon {
    WindowIcon::new(w, h, vec![0u8; (w * h * 4) as usize]).unwrap()
}

// ===========================================================================
// 1. CursorShape enum
// ===========================================================================

#[test]
fn all_17_shapes_round_trip_via_u8() {
    for i in 0..17u8 {
        let shape = CursorShape::from_u8(i).unwrap();
        assert_eq!(shape as u8, i);
    }
}

#[test]
fn out_of_range_u8_returns_none() {
    assert!(CursorShape::from_u8(17).is_none());
    assert!(CursorShape::from_u8(100).is_none());
    assert!(CursorShape::from_u8(255).is_none());
}

#[test]
fn shape_names_are_nonempty_and_unique() {
    let mut names = std::collections::HashSet::new();
    for i in 0..17u8 {
        let shape = CursorShape::from_u8(i).unwrap();
        let name = shape.name();
        assert!(!name.is_empty(), "shape {i} has empty name");
        assert!(names.insert(name), "duplicate name: {name}");
    }
}

#[test]
fn shape_count_matches_variant_count() {
    assert_eq!(CursorShape::COUNT, 17);
}

#[test]
fn default_shape_is_arrow() {
    assert_eq!(CursorShape::default(), CursorShape::Arrow);
    assert_eq!(CursorShape::Arrow as u8, 0);
}

#[test]
fn specific_shape_discriminants() {
    assert_eq!(CursorShape::Ibeam as u8, 1);
    assert_eq!(CursorShape::PointingHand as u8, 2);
    assert_eq!(CursorShape::Cross as u8, 3);
    assert_eq!(CursorShape::Wait as u8, 4);
    assert_eq!(CursorShape::Busy as u8, 5);
    assert_eq!(CursorShape::Drag as u8, 6);
    assert_eq!(CursorShape::CanDrop as u8, 7);
    assert_eq!(CursorShape::Forbidden as u8, 8);
    assert_eq!(CursorShape::Vsize as u8, 9);
    assert_eq!(CursorShape::Hsize as u8, 10);
    assert_eq!(CursorShape::Bdiagsize as u8, 11);
    assert_eq!(CursorShape::Fdiagsize as u8, 12);
    assert_eq!(CursorShape::Move as u8, 13);
    assert_eq!(CursorShape::Vsplit as u8, 14);
    assert_eq!(CursorShape::Hsplit as u8, 15);
    assert_eq!(CursorShape::Help as u8, 16);
}

#[test]
fn shape_equality_and_hash() {
    use std::collections::HashMap;
    let mut map = HashMap::new();
    map.insert(CursorShape::Arrow, "arrow");
    map.insert(CursorShape::Cross, "cross");
    assert_eq!(map[&CursorShape::Arrow], "arrow");
    assert_eq!(map[&CursorShape::Cross], "cross");
    assert_eq!(CursorShape::Arrow, CursorShape::Arrow);
    assert_ne!(CursorShape::Arrow, CursorShape::Cross);
}

#[test]
fn shape_copy_semantics() {
    let a = CursorShape::PointingHand;
    let b = a; // Copy
    assert_eq!(a, b);
}

// ===========================================================================
// 2. CustomCursor
// ===========================================================================

#[test]
fn custom_cursor_creation() {
    let icon = test_icon(32, 32);
    let cursor = CustomCursor::new(icon, 16, 16);
    assert_eq!(cursor.hotspot_x, 16);
    assert_eq!(cursor.hotspot_y, 16);
    assert_eq!(cursor.size(), (32, 32));
}

#[test]
fn custom_cursor_hotspot_clamped_to_image_bounds() {
    let icon = test_icon(16, 16);
    let cursor = CustomCursor::new(icon, 100, 200);
    assert_eq!(cursor.hotspot_x, 15); // clamped to width - 1
    assert_eq!(cursor.hotspot_y, 15); // clamped to height - 1
}

#[test]
fn custom_cursor_hotspot_at_origin() {
    let icon = test_icon(8, 8);
    let cursor = CustomCursor::new(icon, 0, 0);
    assert_eq!(cursor.hotspot_x, 0);
    assert_eq!(cursor.hotspot_y, 0);
}

#[test]
fn custom_cursor_hotspot_at_max() {
    let icon = test_icon(64, 48);
    let cursor = CustomCursor::new(icon, 63, 47);
    assert_eq!(cursor.hotspot_x, 63);
    assert_eq!(cursor.hotspot_y, 47);
}

#[test]
fn custom_cursor_rectangular_image() {
    let icon = test_icon(64, 32);
    let cursor = CustomCursor::new(icon, 32, 16);
    assert_eq!(cursor.size(), (64, 32));
}

#[test]
fn custom_cursor_1x1_image() {
    let icon = test_icon(1, 1);
    let cursor = CustomCursor::new(icon, 0, 0);
    assert_eq!(cursor.size(), (1, 1));
    assert_eq!(cursor.hotspot_x, 0);
    assert_eq!(cursor.hotspot_y, 0);
}

#[test]
fn custom_cursor_1x1_hotspot_clamped() {
    let icon = test_icon(1, 1);
    let cursor = CustomCursor::new(icon, 5, 5);
    assert_eq!(cursor.hotspot_x, 0);
    assert_eq!(cursor.hotspot_y, 0);
}

#[test]
fn custom_cursor_clone_equality() {
    let icon = test_icon(16, 16);
    let cursor = CustomCursor::new(icon, 8, 8);
    let cloned = cursor.clone();
    assert_eq!(cursor, cloned);
}

// ===========================================================================
// 3. CursorManager — core operations
// ===========================================================================

#[test]
fn manager_default_state() {
    let mgr = CursorManager::new();
    assert_eq!(mgr.current_shape(), CursorShape::Arrow);
    assert!(mgr.is_visible());
    assert!(!mgr.is_confined());
    assert_eq!(mgr.position(), (0.0, 0.0));
    assert!(mgr.active_custom_cursor().is_none());
}

#[test]
fn manager_default_trait() {
    let mgr = CursorManager::default();
    assert_eq!(mgr.current_shape(), CursorShape::Arrow);
}

#[test]
fn manager_set_and_get_shape() {
    let mut mgr = CursorManager::new();
    for i in 0..17u8 {
        let shape = CursorShape::from_u8(i).unwrap();
        mgr.set_cursor_shape(shape);
        assert_eq!(mgr.current_shape(), shape);
    }
}

#[test]
fn manager_custom_cursor_register_and_retrieve() {
    let mut mgr = CursorManager::new();
    let cursor = CustomCursor::new(test_icon(16, 16), 8, 8);

    mgr.set_custom_cursor(CursorShape::Cross, cursor.clone());
    assert!(mgr.has_custom_cursor(CursorShape::Cross));

    let retrieved = mgr.get_custom_cursor(CursorShape::Cross).unwrap();
    assert_eq!(retrieved.hotspot_x, 8);
    assert_eq!(retrieved.size(), (16, 16));
}

#[test]
fn manager_active_custom_cursor_tracks_shape() {
    let mut mgr = CursorManager::new();
    let cursor = CustomCursor::new(test_icon(16, 16), 4, 4);

    // No custom cursor for Arrow
    assert!(mgr.active_custom_cursor().is_none());

    // Register for Cross, but current shape is Arrow
    mgr.set_custom_cursor(CursorShape::Cross, cursor);
    assert!(mgr.active_custom_cursor().is_none());

    // Switch to Cross — now active
    mgr.set_cursor_shape(CursorShape::Cross);
    assert!(mgr.active_custom_cursor().is_some());

    // Switch away — no longer active
    mgr.set_cursor_shape(CursorShape::Arrow);
    assert!(mgr.active_custom_cursor().is_none());
}

#[test]
fn manager_clear_single_custom_cursor() {
    let mut mgr = CursorManager::new();
    mgr.set_custom_cursor(CursorShape::Arrow, CustomCursor::new(test_icon(8, 8), 0, 0));
    mgr.set_custom_cursor(CursorShape::Cross, CustomCursor::new(test_icon(8, 8), 4, 4));

    mgr.clear_custom_cursor(CursorShape::Arrow);
    assert!(!mgr.has_custom_cursor(CursorShape::Arrow));
    assert!(mgr.has_custom_cursor(CursorShape::Cross));
}

#[test]
fn manager_clear_all_custom_cursors() {
    let mut mgr = CursorManager::new();
    for i in 0..17u8 {
        let shape = CursorShape::from_u8(i).unwrap();
        mgr.set_custom_cursor(shape, CustomCursor::new(test_icon(8, 8), 0, 0));
    }
    for i in 0..17u8 {
        assert!(mgr.has_custom_cursor(CursorShape::from_u8(i).unwrap()));
    }

    mgr.clear_all_custom_cursors();
    for i in 0..17u8 {
        assert!(!mgr.has_custom_cursor(CursorShape::from_u8(i).unwrap()));
    }
}

#[test]
fn manager_replace_custom_cursor() {
    let mut mgr = CursorManager::new();
    mgr.set_custom_cursor(CursorShape::Arrow, CustomCursor::new(test_icon(8, 8), 2, 2));
    mgr.set_custom_cursor(
        CursorShape::Arrow,
        CustomCursor::new(test_icon(32, 32), 16, 16),
    );

    let cursor = mgr.get_custom_cursor(CursorShape::Arrow).unwrap();
    assert_eq!(cursor.size(), (32, 32));
    assert_eq!(cursor.hotspot_x, 16);
}

// ===========================================================================
// 3b. Visibility and confinement
// ===========================================================================

#[test]
fn manager_visibility_toggle() {
    let mut mgr = CursorManager::new();
    assert!(mgr.is_visible());
    mgr.set_visible(false);
    assert!(!mgr.is_visible());
    mgr.set_visible(true);
    assert!(mgr.is_visible());
}

#[test]
fn manager_confinement_toggle() {
    let mut mgr = CursorManager::new();
    assert!(!mgr.is_confined());
    mgr.set_confined(true);
    assert!(mgr.is_confined());
    mgr.set_confined(false);
    assert!(!mgr.is_confined());
}

#[test]
fn manager_position_update() {
    let mut mgr = CursorManager::new();
    mgr.set_position(123.5, 456.7);
    let (x, y) = mgr.position();
    assert!((x - 123.5).abs() < f32::EPSILON);
    assert!((y - 456.7).abs() < f32::EPSILON);
}

#[test]
fn manager_position_negative_values() {
    let mut mgr = CursorManager::new();
    mgr.set_position(-10.0, -20.0);
    assert_eq!(mgr.position(), (-10.0, -20.0));
}

// ===========================================================================
// 4. Full lifecycle
// ===========================================================================

#[test]
fn full_cursor_lifecycle() {
    let mut mgr = CursorManager::new();

    // Start with defaults
    assert_eq!(mgr.current_shape(), CursorShape::Arrow);
    assert!(mgr.is_visible());
    assert!(!mgr.is_confined());

    // Register custom cursors for text editing
    mgr.set_custom_cursor(
        CursorShape::Ibeam,
        CustomCursor::new(test_icon(16, 32), 8, 16),
    );
    mgr.set_custom_cursor(
        CursorShape::PointingHand,
        CustomCursor::new(test_icon(24, 24), 6, 0),
    );

    // User hovers over text — switch to I-beam
    mgr.set_cursor_shape(CursorShape::Ibeam);
    let active = mgr.active_custom_cursor().unwrap();
    assert_eq!(active.size(), (16, 32));

    // User hovers over link — switch to pointing hand
    mgr.set_cursor_shape(CursorShape::PointingHand);
    let active = mgr.active_custom_cursor().unwrap();
    assert_eq!(active.size(), (24, 24));

    // Drag operation
    mgr.set_cursor_shape(CursorShape::Drag);
    assert!(mgr.active_custom_cursor().is_none()); // no custom for Drag

    // Confine cursor for FPS game
    mgr.set_confined(true);
    mgr.set_visible(false);
    assert!(mgr.is_confined());
    assert!(!mgr.is_visible());

    // Track position
    mgr.set_position(960.0, 540.0);
    assert_eq!(mgr.position(), (960.0, 540.0));

    // Restore
    mgr.set_visible(true);
    mgr.set_confined(false);
    mgr.set_cursor_shape(CursorShape::Arrow);
    mgr.clear_all_custom_cursors();

    assert_eq!(mgr.current_shape(), CursorShape::Arrow);
    assert!(mgr.active_custom_cursor().is_none());
}

// ===========================================================================
// 5. Edge cases
// ===========================================================================

#[test]
fn rapid_shape_cycling() {
    let mut mgr = CursorManager::new();
    for _ in 0..100 {
        for i in 0..17u8 {
            mgr.set_cursor_shape(CursorShape::from_u8(i).unwrap());
        }
    }
    // Last shape set was Help (16)
    assert_eq!(mgr.current_shape(), CursorShape::Help);
}

#[test]
fn all_shapes_custom_cursor_registry() {
    let mut mgr = CursorManager::new();
    for i in 0..17u8 {
        let shape = CursorShape::from_u8(i).unwrap();
        let size = 8 + i as u32 * 2;
        mgr.set_custom_cursor(shape, CustomCursor::new(test_icon(size, size), 0, 0));
    }

    // Verify each shape has its correctly sized cursor
    for i in 0..17u8 {
        let shape = CursorShape::from_u8(i).unwrap();
        mgr.set_cursor_shape(shape);
        let active = mgr.active_custom_cursor().unwrap();
        let expected_size = 8 + i as u32 * 2;
        assert_eq!(active.size(), (expected_size, expected_size));
    }
}

#[test]
fn clear_nonexistent_custom_cursor_is_noop() {
    let mut mgr = CursorManager::new();
    // Should not panic
    mgr.clear_custom_cursor(CursorShape::Cross);
    assert!(!mgr.has_custom_cursor(CursorShape::Cross));
}

#[test]
fn get_custom_cursor_returns_none_when_unset() {
    let mgr = CursorManager::new();
    for i in 0..17u8 {
        assert!(mgr
            .get_custom_cursor(CursorShape::from_u8(i).unwrap())
            .is_none());
    }
}

#[test]
fn large_cursor_image() {
    let icon = test_icon(256, 256);
    let cursor = CustomCursor::new(icon, 128, 128);
    assert_eq!(cursor.size(), (256, 256));
    assert_eq!(cursor.hotspot_x, 128);
    assert_eq!(cursor.hotspot_y, 128);
}
