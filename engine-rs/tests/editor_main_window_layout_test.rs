//! pat-14o2t: Editor main window layout with dockable panels.
//!
//! Validates:
//! 1. DockPanel trait implementations (SceneTreeDock, PropertyDock)
//! 2. SceneTreeDock refresh, selection, find, depth-first ordering
//! 3. SceneTreeDock drag-drop validation and application
//! 4. PropertyDock inspector access
//! 5. DockSlot enum: names, left/right classification
//! 6. EditorLayout: add/remove/move docks, slot queries
//! 7. EditorLayout: visibility toggle
//! 8. EditorLayout: panel sizing (left, right, bottom)
//! 9. EditorLayout: default Godot layout with 10 standard docks
//! 10. DragDropAction: Before/After/Into semantics
//! 11. Cross-branch reparenting via drag-drop
//! 12. ClassDB registration for EditorPlugin

use gdeditor::dock::{DockSlot, EditorLayout, SceneTreeDock, PropertyDock, DragDropAction};
use gdeditor::DockPanel;
use gdscene::node::{Node, NodeId};
use gdscene::SceneTree;

// ── Helper ──────────────────────────────────────────────────────────

fn make_tree() -> SceneTree {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let main = Node::new("Main", "Node");
    let main_id = tree.add_child(root, main).unwrap();
    let player = Node::new("Player", "Node2D");
    tree.add_child(main_id, player).unwrap();
    let enemy = Node::new("Enemy", "Sprite2D");
    tree.add_child(main_id, enemy).unwrap();
    tree
}

// ── SceneTreeDock ───────────────────────────────────────────────────

#[test]
fn scene_tree_dock_refresh_populates_entries() {
    let tree = make_tree();
    let mut dock = SceneTreeDock::new();
    dock.refresh(&tree);
    assert_eq!(dock.entries().len(), 4); // root, Main, Player, Enemy
}

#[test]
fn scene_tree_dock_depth_first_order() {
    let tree = make_tree();
    let mut dock = SceneTreeDock::new();
    dock.refresh(&tree);
    assert_eq!(dock.entries()[0].name, "root");
    assert_eq!(dock.entries()[0].depth, 0);
    assert_eq!(dock.entries()[1].name, "Main");
    assert_eq!(dock.entries()[1].depth, 1);
    assert_eq!(dock.entries()[2].name, "Player");
    assert_eq!(dock.entries()[2].depth, 2);
    assert_eq!(dock.entries()[3].name, "Enemy");
    assert_eq!(dock.entries()[3].depth, 2);
}

#[test]
fn scene_tree_dock_entry_paths() {
    let tree = make_tree();
    let mut dock = SceneTreeDock::new();
    dock.refresh(&tree);
    assert_eq!(dock.entries()[0].path, "/root");
    assert_eq!(dock.entries()[2].path, "/root/Main/Player");
    assert_eq!(dock.entries()[3].path, "/root/Main/Enemy");
}

#[test]
fn scene_tree_dock_find_entry() {
    let tree = make_tree();
    let mut dock = SceneTreeDock::new();
    dock.refresh(&tree);
    let root_entry = dock.find_entry(tree.root_id()).unwrap();
    assert_eq!(root_entry.name, "root");
    assert!(dock.find_entry(NodeId::next()).is_none());
}

#[test]
fn scene_tree_dock_selection() {
    let tree = make_tree();
    let mut dock = SceneTreeDock::new();
    dock.refresh(&tree);

    assert_eq!(dock.selected(), None);
    let main_id = dock.entries()[1].id;
    assert!(dock.select(main_id));
    assert_eq!(dock.selected(), Some(main_id));
    dock.deselect();
    assert_eq!(dock.selected(), None);
}

#[test]
fn scene_tree_dock_select_nonexistent_fails() {
    let mut dock = SceneTreeDock::new();
    assert!(!dock.select(NodeId::next()));
}

#[test]
fn scene_tree_dock_title() {
    let dock = SceneTreeDock::new();
    assert_eq!(dock.title(), "Scene");
}

// ── PropertyDock ────────────────────────────────────────────────────

#[test]
fn property_dock_title() {
    let dock = PropertyDock::new();
    assert_eq!(dock.title(), "Inspector");
}

#[test]
fn property_dock_inspector_access() {
    let mut dock = PropertyDock::new();
    let _ = dock.inspector();
    let _ = dock.inspector_mut();
}

// ── DragDropAction ──────────────────────────────────────────────────

#[test]
fn drag_drop_cannot_drop_on_self() {
    let tree = make_tree();
    let mut dock = SceneTreeDock::new();
    dock.refresh(&tree);
    let player_id = dock.entries()[2].id;
    assert!(!dock.can_drop(&tree, player_id, player_id, DragDropAction::Into));
    assert!(!dock.can_drop(&tree, player_id, player_id, DragDropAction::Before));
    assert!(!dock.can_drop(&tree, player_id, player_id, DragDropAction::After));
}

#[test]
fn drag_drop_cannot_move_root() {
    let tree = make_tree();
    let mut dock = SceneTreeDock::new();
    dock.refresh(&tree);
    let main_id = dock.entries()[1].id;
    assert!(!dock.can_drop(&tree, tree.root_id(), main_id, DragDropAction::Into));
}

#[test]
fn drag_drop_cannot_before_after_root() {
    let tree = make_tree();
    let mut dock = SceneTreeDock::new();
    dock.refresh(&tree);
    let player_id = dock.entries()[2].id;
    assert!(!dock.can_drop(&tree, player_id, tree.root_id(), DragDropAction::Before));
    assert!(!dock.can_drop(&tree, player_id, tree.root_id(), DragDropAction::After));
}

#[test]
fn drag_drop_cannot_into_own_descendant() {
    let tree = make_tree();
    let mut dock = SceneTreeDock::new();
    dock.refresh(&tree);
    let main_id = dock.entries()[1].id;
    let player_id = dock.entries()[2].id;
    assert!(!dock.can_drop(&tree, main_id, player_id, DragDropAction::Into));
}

#[test]
fn drag_drop_sibling_reorder() {
    let tree = make_tree();
    let mut dock = SceneTreeDock::new();
    dock.refresh(&tree);
    let player_id = dock.entries()[2].id;
    let enemy_id = dock.entries()[3].id;
    assert!(dock.can_drop(&tree, enemy_id, player_id, DragDropAction::Before));
    assert!(dock.can_drop(&tree, player_id, enemy_id, DragDropAction::After));
}

#[test]
fn drag_drop_begin_and_cancel() {
    let tree = make_tree();
    let mut dock = SceneTreeDock::new();
    dock.refresh(&tree);
    let player_id = dock.entries()[2].id;
    assert!(dock.begin_drag(player_id));
    assert_eq!(dock.drag_source(), Some(player_id));
    dock.cancel_drag();
    assert_eq!(dock.drag_source(), None);
}

#[test]
fn drag_drop_begin_nonexistent_fails() {
    let mut dock = SceneTreeDock::new();
    assert!(!dock.begin_drag(NodeId::next()));
}

#[test]
fn apply_drag_drop_into_reparents() {
    let mut tree = make_tree();
    let mut dock = SceneTreeDock::new();
    dock.refresh(&tree);
    let player_id = dock.entries()[2].id;
    let enemy_id = dock.entries()[3].id;

    dock.apply_drag_drop(&mut tree, enemy_id, player_id, DragDropAction::Into).unwrap();

    let player = tree.get_node(player_id).unwrap();
    assert!(player.children().contains(&enemy_id));
    let entry = dock.find_entry(enemy_id).unwrap();
    assert_eq!(entry.depth, 3);
}

#[test]
fn apply_drag_drop_before_reorders() {
    let mut tree = make_tree();
    let mut dock = SceneTreeDock::new();
    dock.refresh(&tree);
    let player_id = dock.entries()[2].id;
    let enemy_id = dock.entries()[3].id;

    dock.apply_drag_drop(&mut tree, enemy_id, player_id, DragDropAction::Before).unwrap();

    let main_id = dock.entries()[1].id;
    let main = tree.get_node(main_id).unwrap();
    let children = main.children();
    let enemy_idx = children.iter().position(|&c| c == enemy_id).unwrap();
    let player_idx = children.iter().position(|&c| c == player_id).unwrap();
    assert!(enemy_idx < player_idx);
}

#[test]
fn apply_drag_drop_clears_drag_state() {
    let mut tree = make_tree();
    let mut dock = SceneTreeDock::new();
    dock.refresh(&tree);
    let player_id = dock.entries()[2].id;
    let enemy_id = dock.entries()[3].id;

    dock.begin_drag(enemy_id);
    dock.apply_drag_drop(&mut tree, enemy_id, player_id, DragDropAction::Into).unwrap();
    assert_eq!(dock.drag_source(), None);
}

#[test]
fn cross_branch_reparent() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let main_id = tree.add_child(root, Node::new("Main", "Node")).unwrap();
    let player_id = tree.add_child(main_id, Node::new("Player", "Node2D")).unwrap();
    let enemy_id = tree.add_child(main_id, Node::new("Enemy", "Node2D")).unwrap();
    let minion_id = tree.add_child(enemy_id, Node::new("Minion", "Sprite2D")).unwrap();

    let mut dock = SceneTreeDock::new();
    dock.refresh(&tree);

    dock.apply_drag_drop(&mut tree, minion_id, player_id, DragDropAction::Into).unwrap();

    assert!(tree.get_node(player_id).unwrap().children().contains(&minion_id));
    assert!(!tree.get_node(enemy_id).unwrap().children().contains(&minion_id));
}

// ── DockSlot ────────────────────────────────────────────────────────

#[test]
fn dock_slot_names() {
    assert_eq!(DockSlot::LeftUpper.name(), "Left Upper");
    assert_eq!(DockSlot::LeftLower.name(), "Left Lower");
    assert_eq!(DockSlot::RightUpper.name(), "Right Upper");
    assert_eq!(DockSlot::RightLower.name(), "Right Lower");
    assert_eq!(DockSlot::Bottom.name(), "Bottom");
}

#[test]
fn dock_slot_left_right_classification() {
    assert!(DockSlot::LeftUpper.is_left());
    assert!(DockSlot::LeftLower.is_left());
    assert!(!DockSlot::LeftUpper.is_right());
    assert!(DockSlot::RightUpper.is_right());
    assert!(DockSlot::RightLower.is_right());
    assert!(!DockSlot::Bottom.is_left());
    assert!(!DockSlot::Bottom.is_right());
}

// ── EditorLayout ────────────────────────────────────────────────────

#[test]
fn layout_starts_empty_with_default_sizes() {
    let layout = EditorLayout::new();
    assert_eq!(layout.dock_count(), 0);
    assert!(layout.dock_names().is_empty());
    assert!((layout.left_width() - EditorLayout::DEFAULT_LEFT_WIDTH).abs() < f32::EPSILON);
    assert!((layout.right_width() - EditorLayout::DEFAULT_RIGHT_WIDTH).abs() < f32::EPSILON);
    assert!((layout.bottom_height() - EditorLayout::DEFAULT_BOTTOM_HEIGHT).abs() < f32::EPSILON);
}

#[test]
fn layout_add_and_query_dock() {
    let mut layout = EditorLayout::new();
    assert!(layout.add_dock("Scene", DockSlot::LeftUpper));
    assert_eq!(layout.dock_count(), 1);
    assert_eq!(layout.dock_slot("Scene"), Some(DockSlot::LeftUpper));
}

#[test]
fn layout_duplicate_dock_rejected() {
    let mut layout = EditorLayout::new();
    assert!(layout.add_dock("Scene", DockSlot::LeftUpper));
    assert!(!layout.add_dock("Scene", DockSlot::RightUpper));
    assert_eq!(layout.dock_count(), 1);
}

#[test]
fn layout_remove_dock() {
    let mut layout = EditorLayout::new();
    layout.add_dock("Scene", DockSlot::LeftUpper);
    assert!(layout.remove_dock("Scene"));
    assert_eq!(layout.dock_count(), 0);
    assert!(!layout.remove_dock("Scene"));
}

#[test]
fn layout_move_dock_between_slots() {
    let mut layout = EditorLayout::new();
    layout.add_dock("Inspector", DockSlot::RightUpper);
    assert!(layout.move_dock("Inspector", DockSlot::Bottom));
    assert_eq!(layout.dock_slot("Inspector"), Some(DockSlot::Bottom));
}

#[test]
fn layout_move_nonexistent_dock_fails() {
    let mut layout = EditorLayout::new();
    assert!(!layout.move_dock("Missing", DockSlot::Bottom));
}

#[test]
fn layout_docks_in_slot_ordering() {
    let mut layout = EditorLayout::new();
    layout.add_dock("Scene", DockSlot::LeftUpper);
    layout.add_dock("Import", DockSlot::LeftUpper);
    layout.add_dock("Inspector", DockSlot::RightUpper);

    assert_eq!(layout.docks_in_slot(DockSlot::LeftUpper), vec!["Scene", "Import"]);
    assert_eq!(layout.docks_in_slot(DockSlot::RightUpper), vec!["Inspector"]);
    assert!(layout.docks_in_slot(DockSlot::Bottom).is_empty());
}

#[test]
fn layout_visibility_toggle() {
    let mut layout = EditorLayout::new();
    layout.add_dock("Output", DockSlot::Bottom);
    assert_eq!(layout.is_visible("Output"), Some(true));
    layout.set_visible("Output", false);
    assert_eq!(layout.is_visible("Output"), Some(false));
    layout.set_visible("Output", true);
    assert_eq!(layout.is_visible("Output"), Some(true));
}

#[test]
fn layout_visibility_nonexistent_returns_none() {
    let layout = EditorLayout::new();
    assert_eq!(layout.is_visible("Missing"), None);
}

#[test]
fn layout_panel_sizing() {
    let mut layout = EditorLayout::new();
    layout.set_left_width(400.0);
    layout.set_right_width(350.0);
    layout.set_bottom_height(150.0);
    assert!((layout.left_width() - 400.0).abs() < f32::EPSILON);
    assert!((layout.right_width() - 350.0).abs() < f32::EPSILON);
    assert!((layout.bottom_height() - 150.0).abs() < f32::EPSILON);
}

#[test]
fn layout_sizing_clamped_to_zero() {
    let mut layout = EditorLayout::new();
    layout.set_left_width(-100.0);
    layout.set_right_width(-50.0);
    layout.set_bottom_height(-25.0);
    assert!((layout.left_width()).abs() < f32::EPSILON);
    assert!((layout.right_width()).abs() < f32::EPSILON);
    assert!((layout.bottom_height()).abs() < f32::EPSILON);
}

#[test]
fn layout_default_godot_has_10_docks() {
    let layout = EditorLayout::default_godot_layout();
    assert_eq!(layout.dock_count(), 10);
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
fn layout_default_godot_bottom_has_4_docks() {
    let layout = EditorLayout::default_godot_layout();
    assert_eq!(layout.docks_in_slot(DockSlot::Bottom).len(), 4);
}

#[test]
fn layout_default_godot_left_upper_has_2_docks() {
    let layout = EditorLayout::default_godot_layout();
    assert_eq!(layout.docks_in_slot(DockSlot::LeftUpper).len(), 2);
}

#[test]
fn layout_dock_names_preserve_insertion_order() {
    let mut layout = EditorLayout::new();
    layout.add_dock("C", DockSlot::Bottom);
    layout.add_dock("A", DockSlot::LeftUpper);
    layout.add_dock("B", DockSlot::RightUpper);
    assert_eq!(layout.dock_names(), vec!["C", "A", "B"]);
}

// ── ClassDB registration ────────────────────────────────────────────

#[test]
fn classdb_editor_plugin_exists() {
    gdobject::class_db::register_3d_classes();
    assert!(gdobject::class_db::class_exists("EditorPlugin"));
}

#[test]
fn classdb_editor_plugin_has_dock_methods() {
    gdobject::class_db::register_3d_classes();
    assert!(gdobject::class_db::class_has_method("EditorPlugin", "add_control_to_dock"));
    assert!(gdobject::class_db::class_has_method("EditorPlugin", "remove_control_from_docks"));
    assert!(gdobject::class_db::class_has_method("EditorPlugin", "add_control_to_bottom_panel"));
    assert!(gdobject::class_db::class_has_method("EditorPlugin", "remove_control_from_bottom_panel"));
    assert!(gdobject::class_db::class_has_method("EditorPlugin", "get_editor_interface"));
}
