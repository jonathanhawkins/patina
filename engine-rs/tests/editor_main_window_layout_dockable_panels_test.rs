//! pat-14o2t: Editor main window layout with dockable panels.
//!
//! Integration tests covering:
//! 1. DockSlot enum — names, is_left, is_right
//! 2. EditorLayout construction and defaults
//! 3. Adding docks — single, multiple, duplicate rejection
//! 4. Removing docks
//! 5. Moving docks between slots
//! 6. Dock visibility toggling
//! 7. Querying docks by slot
//! 8. Sidebar and bottom panel sizing
//! 9. Default Godot-like layout
//! 10. Full layout lifecycle
//! 11. ClassDB registration

use gdeditor::dock::{DockSlot, EditorLayout};

// ===========================================================================
// 1. DockSlot enum
// ===========================================================================

#[test]
fn dock_slot_names() {
    assert_eq!(DockSlot::LeftUpper.name(), "Left Upper");
    assert_eq!(DockSlot::LeftLower.name(), "Left Lower");
    assert_eq!(DockSlot::RightUpper.name(), "Right Upper");
    assert_eq!(DockSlot::RightLower.name(), "Right Lower");
    assert_eq!(DockSlot::Bottom.name(), "Bottom");
}

#[test]
fn dock_slot_left_side() {
    assert!(DockSlot::LeftUpper.is_left());
    assert!(DockSlot::LeftLower.is_left());
    assert!(!DockSlot::RightUpper.is_left());
    assert!(!DockSlot::RightLower.is_left());
    assert!(!DockSlot::Bottom.is_left());
}

#[test]
fn dock_slot_right_side() {
    assert!(!DockSlot::LeftUpper.is_right());
    assert!(!DockSlot::LeftLower.is_right());
    assert!(DockSlot::RightUpper.is_right());
    assert!(DockSlot::RightLower.is_right());
    assert!(!DockSlot::Bottom.is_right());
}

#[test]
fn dock_slot_bottom_is_neither_left_nor_right() {
    assert!(!DockSlot::Bottom.is_left());
    assert!(!DockSlot::Bottom.is_right());
}

// ===========================================================================
// 2. EditorLayout construction and defaults
// ===========================================================================

#[test]
fn layout_new_empty() {
    let layout = EditorLayout::new();
    assert_eq!(layout.dock_count(), 0);
    assert!(layout.dock_names().is_empty());
}

#[test]
fn layout_default_sizes() {
    let layout = EditorLayout::new();
    assert!((layout.left_width() - EditorLayout::DEFAULT_LEFT_WIDTH).abs() < f32::EPSILON);
    assert!((layout.right_width() - EditorLayout::DEFAULT_RIGHT_WIDTH).abs() < f32::EPSILON);
    assert!((layout.bottom_height() - EditorLayout::DEFAULT_BOTTOM_HEIGHT).abs() < f32::EPSILON);
}

#[test]
fn layout_default_trait() {
    let layout = EditorLayout::default();
    assert_eq!(layout.dock_count(), 0);
}

// ===========================================================================
// 3. Adding docks
// ===========================================================================

#[test]
fn add_single_dock() {
    let mut layout = EditorLayout::new();
    assert!(layout.add_dock("Scene", DockSlot::LeftUpper));
    assert_eq!(layout.dock_count(), 1);
    assert_eq!(layout.dock_slot("Scene"), Some(DockSlot::LeftUpper));
}

#[test]
fn add_multiple_docks() {
    let mut layout = EditorLayout::new();
    layout.add_dock("Scene", DockSlot::LeftUpper);
    layout.add_dock("Inspector", DockSlot::RightUpper);
    layout.add_dock("Output", DockSlot::Bottom);
    assert_eq!(layout.dock_count(), 3);
}

#[test]
fn add_duplicate_dock_rejected() {
    let mut layout = EditorLayout::new();
    assert!(layout.add_dock("Scene", DockSlot::LeftUpper));
    assert!(!layout.add_dock("Scene", DockSlot::RightUpper));
    assert_eq!(layout.dock_count(), 1);
    // Should still be in original slot
    assert_eq!(layout.dock_slot("Scene"), Some(DockSlot::LeftUpper));
}

#[test]
fn add_multiple_docks_same_slot() {
    let mut layout = EditorLayout::new();
    layout.add_dock("Scene", DockSlot::LeftUpper);
    layout.add_dock("Import", DockSlot::LeftUpper);
    assert_eq!(layout.docks_in_slot(DockSlot::LeftUpper).len(), 2);
}

// ===========================================================================
// 4. Removing docks
// ===========================================================================

#[test]
fn remove_existing_dock() {
    let mut layout = EditorLayout::new();
    layout.add_dock("Scene", DockSlot::LeftUpper);
    assert!(layout.remove_dock("Scene"));
    assert_eq!(layout.dock_count(), 0);
    assert!(layout.dock_slot("Scene").is_none());
}

#[test]
fn remove_nonexistent_dock() {
    let mut layout = EditorLayout::new();
    assert!(!layout.remove_dock("NoSuchDock"));
}

#[test]
fn remove_one_of_many() {
    let mut layout = EditorLayout::new();
    layout.add_dock("Scene", DockSlot::LeftUpper);
    layout.add_dock("Inspector", DockSlot::RightUpper);
    layout.add_dock("Output", DockSlot::Bottom);
    layout.remove_dock("Inspector");
    assert_eq!(layout.dock_count(), 2);
    assert!(layout.dock_slot("Inspector").is_none());
    assert!(layout.dock_slot("Scene").is_some());
    assert!(layout.dock_slot("Output").is_some());
}

// ===========================================================================
// 5. Moving docks
// ===========================================================================

#[test]
fn move_dock_between_slots() {
    let mut layout = EditorLayout::new();
    layout.add_dock("Inspector", DockSlot::RightUpper);
    assert!(layout.move_dock("Inspector", DockSlot::Bottom));
    assert_eq!(layout.dock_slot("Inspector"), Some(DockSlot::Bottom));
}

#[test]
fn move_nonexistent_dock_fails() {
    let mut layout = EditorLayout::new();
    assert!(!layout.move_dock("NoSuchDock", DockSlot::Bottom));
}

#[test]
fn move_dock_left_to_right() {
    let mut layout = EditorLayout::new();
    layout.add_dock("Scene", DockSlot::LeftUpper);
    layout.move_dock("Scene", DockSlot::RightLower);
    assert_eq!(layout.dock_slot("Scene"), Some(DockSlot::RightLower));
    assert!(layout.docks_in_slot(DockSlot::LeftUpper).is_empty());
    assert_eq!(layout.docks_in_slot(DockSlot::RightLower), vec!["Scene"]);
}

// ===========================================================================
// 6. Dock visibility
// ===========================================================================

#[test]
fn docks_visible_by_default() {
    let mut layout = EditorLayout::new();
    layout.add_dock("Scene", DockSlot::LeftUpper);
    assert_eq!(layout.is_visible("Scene"), Some(true));
}

#[test]
fn toggle_dock_visibility() {
    let mut layout = EditorLayout::new();
    layout.add_dock("Output", DockSlot::Bottom);
    assert!(layout.set_visible("Output", false));
    assert_eq!(layout.is_visible("Output"), Some(false));
    assert!(layout.set_visible("Output", true));
    assert_eq!(layout.is_visible("Output"), Some(true));
}

#[test]
fn visibility_nonexistent_dock() {
    let mut layout = EditorLayout::new();
    assert!(layout.is_visible("NoSuchDock").is_none());
    assert!(!layout.set_visible("NoSuchDock", false));
}

// ===========================================================================
// 7. Querying docks by slot
// ===========================================================================

#[test]
fn docks_in_slot_returns_ordered() {
    let mut layout = EditorLayout::new();
    layout.add_dock("Scene", DockSlot::LeftUpper);
    layout.add_dock("Import", DockSlot::LeftUpper);
    let docks = layout.docks_in_slot(DockSlot::LeftUpper);
    assert_eq!(docks, vec!["Scene", "Import"]);
}

#[test]
fn docks_in_empty_slot() {
    let layout = EditorLayout::new();
    assert!(layout.docks_in_slot(DockSlot::Bottom).is_empty());
}

#[test]
fn dock_names_returns_all() {
    let mut layout = EditorLayout::new();
    layout.add_dock("A", DockSlot::LeftUpper);
    layout.add_dock("B", DockSlot::RightUpper);
    layout.add_dock("C", DockSlot::Bottom);
    let names = layout.dock_names();
    assert_eq!(names.len(), 3);
    assert!(names.contains(&"A"));
    assert!(names.contains(&"B"));
    assert!(names.contains(&"C"));
}

// ===========================================================================
// 8. Sidebar and bottom panel sizing
// ===========================================================================

#[test]
fn set_left_width() {
    let mut layout = EditorLayout::new();
    layout.set_left_width(300.0);
    assert!((layout.left_width() - 300.0).abs() < f32::EPSILON);
}

#[test]
fn set_right_width() {
    let mut layout = EditorLayout::new();
    layout.set_right_width(400.0);
    assert!((layout.right_width() - 400.0).abs() < f32::EPSILON);
}

#[test]
fn set_bottom_height() {
    let mut layout = EditorLayout::new();
    layout.set_bottom_height(150.0);
    assert!((layout.bottom_height() - 150.0).abs() < f32::EPSILON);
}

#[test]
fn negative_size_clamped_to_zero() {
    let mut layout = EditorLayout::new();
    layout.set_left_width(-100.0);
    assert!(layout.left_width() >= 0.0);
    layout.set_right_width(-50.0);
    assert!(layout.right_width() >= 0.0);
    layout.set_bottom_height(-200.0);
    assert!(layout.bottom_height() >= 0.0);
}

// ===========================================================================
// 9. Default Godot-like layout
// ===========================================================================

#[test]
fn default_godot_layout_dock_count() {
    let layout = EditorLayout::default_godot_layout();
    assert_eq!(layout.dock_count(), 10);
}

#[test]
fn default_godot_layout_slot_assignments() {
    let layout = EditorLayout::default_godot_layout();
    assert_eq!(layout.dock_slot("Scene"), Some(DockSlot::LeftUpper));
    assert_eq!(layout.dock_slot("Import"), Some(DockSlot::LeftUpper));
    assert_eq!(layout.dock_slot("FileSystem"), Some(DockSlot::LeftLower));
    assert_eq!(layout.dock_slot("Inspector"), Some(DockSlot::RightUpper));
    assert_eq!(layout.dock_slot("Node"), Some(DockSlot::RightUpper));
    assert_eq!(layout.dock_slot("History"), Some(DockSlot::RightLower));
    assert_eq!(layout.dock_slot("Output"), Some(DockSlot::Bottom));
    assert_eq!(layout.dock_slot("Debugger"), Some(DockSlot::Bottom));
    assert_eq!(layout.dock_slot("Audio"), Some(DockSlot::Bottom));
    assert_eq!(layout.dock_slot("Animation"), Some(DockSlot::Bottom));
}

#[test]
fn default_godot_layout_bottom_panel_docks() {
    let layout = EditorLayout::default_godot_layout();
    let bottom = layout.docks_in_slot(DockSlot::Bottom);
    assert_eq!(bottom.len(), 4);
    assert!(bottom.contains(&"Output"));
    assert!(bottom.contains(&"Debugger"));
    assert!(bottom.contains(&"Audio"));
    assert!(bottom.contains(&"Animation"));
}

#[test]
fn default_godot_layout_left_sidebar_docks() {
    let layout = EditorLayout::default_godot_layout();
    let left_upper = layout.docks_in_slot(DockSlot::LeftUpper);
    assert_eq!(left_upper.len(), 2);
    assert!(left_upper.contains(&"Scene"));
    assert!(left_upper.contains(&"Import"));
    let left_lower = layout.docks_in_slot(DockSlot::LeftLower);
    assert_eq!(left_lower.len(), 1);
    assert!(left_lower.contains(&"FileSystem"));
}

#[test]
fn default_godot_layout_right_sidebar_docks() {
    let layout = EditorLayout::default_godot_layout();
    let right_upper = layout.docks_in_slot(DockSlot::RightUpper);
    assert_eq!(right_upper.len(), 2);
    assert!(right_upper.contains(&"Inspector"));
    assert!(right_upper.contains(&"Node"));
    let right_lower = layout.docks_in_slot(DockSlot::RightLower);
    assert_eq!(right_lower.len(), 1);
    assert!(right_lower.contains(&"History"));
}

// ===========================================================================
// 10. Full layout lifecycle
// ===========================================================================

#[test]
fn full_layout_lifecycle() {
    // Start with Godot defaults
    let mut layout = EditorLayout::default_godot_layout();
    assert_eq!(layout.dock_count(), 10);

    // Resize sidebars
    layout.set_left_width(280.0);
    layout.set_right_width(350.0);
    layout.set_bottom_height(180.0);
    assert!((layout.left_width() - 280.0).abs() < f32::EPSILON);
    assert!((layout.right_width() - 350.0).abs() < f32::EPSILON);
    assert!((layout.bottom_height() - 180.0).abs() < f32::EPSILON);

    // Move Inspector to bottom
    layout.move_dock("Inspector", DockSlot::Bottom);
    assert_eq!(layout.dock_slot("Inspector"), Some(DockSlot::Bottom));
    assert_eq!(layout.docks_in_slot(DockSlot::Bottom).len(), 5);

    // Hide Output
    layout.set_visible("Output", false);
    assert_eq!(layout.is_visible("Output"), Some(false));

    // Add a custom dock
    layout.add_dock("Profiler", DockSlot::Bottom);
    assert_eq!(layout.dock_count(), 11);

    // Remove Debugger
    layout.remove_dock("Debugger");
    assert_eq!(layout.dock_count(), 10);
    assert!(layout.dock_slot("Debugger").is_none());

    // Move Inspector back
    layout.move_dock("Inspector", DockSlot::RightUpper);
    assert_eq!(layout.dock_slot("Inspector"), Some(DockSlot::RightUpper));
}

// ===========================================================================
// 11. ClassDB registration
// ===========================================================================

#[test]
fn classdb_editor_layout_exists() {
    gdobject::class_db::register_3d_classes();
    assert!(gdobject::class_db::class_exists("EditorLayout"));
}

#[test]
fn classdb_editor_layout_has_methods() {
    gdobject::class_db::register_3d_classes();
    assert!(gdobject::class_db::class_has_method("EditorLayout", "add_dock"));
    assert!(gdobject::class_db::class_has_method("EditorLayout", "remove_dock"));
    assert!(gdobject::class_db::class_has_method("EditorLayout", "move_dock"));
}
