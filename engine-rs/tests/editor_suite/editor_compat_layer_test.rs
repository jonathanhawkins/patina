//! pat-1q5sp: Integration tests for the minimal editor-facing compatibility layer.
//!
//! Source of truth: `engine-rs/crates/gdeditor/src/editor_compat.rs`
//! Classification: Measured — compatibility surface validated via integration tests
//!
//! Verifies that the `editor_compat` module exposes a correct Godot-compatible
//! API surface: type aliases, property info, selection adapter, interface compat,
//! and undo/redo compat all behave as the Godot API specifies.

use gdeditor::editor_compat::{
    EditorInterfaceCompat, EditorPropertyInfo, EditorSelection, PropertyUsageFlags,
    UndoRedoCompat,
};
use gdeditor::{Editor, EditorCommand};
use gdscene::node::Node;
use gdscene::SceneTree;
use gdvariant::variant::VariantType;
use gdvariant::Variant;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn make_editor_with_nodes() -> (Editor, gdscene::node::NodeId, gdscene::node::NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let player = tree
        .add_child(root, Node::new("Player", "CharacterBody2D"))
        .unwrap();
    let enemy = tree
        .add_child(root, Node::new("Enemy", "CharacterBody2D"))
        .unwrap();
    (Editor::new(tree), player, enemy)
}

// ---------------------------------------------------------------------------
// PropertyUsageFlags
// ---------------------------------------------------------------------------

#[test]
fn property_usage_editor_visible() {
    assert!(PropertyUsageFlags::Editor.is_editor_visible());
    assert!(PropertyUsageFlags::Default.is_editor_visible());
    assert!(!PropertyUsageFlags::NoEditor.is_editor_visible());
    assert!(!PropertyUsageFlags::Storage.is_editor_visible());
    assert!(!PropertyUsageFlags::ReadOnly.is_editor_visible());
}

#[test]
fn property_usage_storage() {
    assert!(PropertyUsageFlags::Storage.is_stored());
    assert!(PropertyUsageFlags::Default.is_stored());
    assert!(!PropertyUsageFlags::Editor.is_stored());
    assert!(!PropertyUsageFlags::NoEditor.is_stored());
    assert!(!PropertyUsageFlags::ReadOnly.is_stored());
}

// ---------------------------------------------------------------------------
// EditorPropertyInfo
// ---------------------------------------------------------------------------

#[test]
fn editor_property_info_default_usage() {
    let info = EditorPropertyInfo::new("speed", VariantType::Float);
    assert_eq!(info.name, "speed");
    assert!(matches!(info.usage, PropertyUsageFlags::Default));
    assert!(info.usage.is_editor_visible());
    assert!(info.usage.is_stored());
}

#[test]
fn editor_property_info_read_only_not_writable() {
    let info =
        EditorPropertyInfo::new("internal_id", VariantType::Int).with_usage(PropertyUsageFlags::ReadOnly);
    assert!(!info.usage.is_editor_visible());
    let editor = info.to_custom_editor();
    assert!(editor.read_only, "ReadOnly usage must produce a read-only editor");
}

#[test]
fn editor_property_info_no_editor_hidden() {
    let info = EditorPropertyInfo::new("hidden_prop", VariantType::Bool)
        .with_usage(PropertyUsageFlags::NoEditor);
    assert!(!info.usage.is_editor_visible());
    let editor = info.to_custom_editor();
    assert!(editor.read_only, "NoEditor usage must produce a read-only editor");
}

// ---------------------------------------------------------------------------
// EditorSelection — Godot-compatible selection adapter
// ---------------------------------------------------------------------------

#[test]
fn editor_selection_initially_empty() {
    let (mut editor, _, _) = make_editor_with_nodes();
    let sel = EditorSelection::new(&mut editor);
    assert_eq!(sel.get_selected_node_count(), 0);
    assert!(sel.get_selected_nodes().is_empty());
}

#[test]
fn editor_selection_add_and_query() {
    let (mut editor, player, _) = make_editor_with_nodes();
    let mut sel = EditorSelection::new(&mut editor);
    sel.add_node(player);
    assert_eq!(sel.get_selected_node_count(), 1);
    assert!(sel.is_selected(player));
    assert_eq!(sel.get_selected_nodes(), vec![player]);
}

#[test]
fn editor_selection_replace_is_single_select() {
    let (mut editor, player, enemy) = make_editor_with_nodes();
    let mut sel = EditorSelection::new(&mut editor);
    sel.add_node(player);
    sel.add_node(enemy); // replaces player (Patina is single-select)
    assert_eq!(sel.get_selected_node_count(), 1);
    assert!(sel.is_selected(enemy));
    assert!(!sel.is_selected(player));
}

#[test]
fn editor_selection_remove_clears() {
    let (mut editor, player, _) = make_editor_with_nodes();
    let mut sel = EditorSelection::new(&mut editor);
    sel.add_node(player);
    sel.remove_node(player);
    assert_eq!(sel.get_selected_node_count(), 0);
}

#[test]
fn editor_selection_remove_unrelated_no_op() {
    let (mut editor, player, enemy) = make_editor_with_nodes();
    let mut sel = EditorSelection::new(&mut editor);
    sel.add_node(player);
    sel.remove_node(enemy); // enemy is not selected — no-op
    assert!(sel.is_selected(player));
}

#[test]
fn editor_selection_clear_resets() {
    let (mut editor, player, _) = make_editor_with_nodes();
    let mut sel = EditorSelection::new(&mut editor);
    sel.add_node(player);
    sel.clear();
    assert_eq!(sel.get_selected_node_count(), 0);
}

// ---------------------------------------------------------------------------
// EditorInterfaceCompat — Godot-named accessors
// ---------------------------------------------------------------------------

#[test]
fn editor_interface_compat_scene_root() {
    let (editor, _, _) = make_editor_with_nodes();
    let root = editor.tree().root_id();
    let compat = EditorInterfaceCompat::new(&editor);
    assert_eq!(compat.get_edited_scene_root(), root);
}

#[test]
fn editor_interface_compat_no_selection_initially() {
    let (editor, _, _) = make_editor_with_nodes();
    let compat = EditorInterfaceCompat::new(&editor);
    assert_eq!(compat.get_selection(), None);
}

#[test]
fn editor_interface_compat_selection_after_select() {
    let (mut editor, player, _) = make_editor_with_nodes();
    editor.select_node(player);
    let compat = EditorInterfaceCompat::new(&editor);
    assert_eq!(compat.get_selection(), Some(player));
}

#[test]
fn editor_interface_compat_main_screen_name() {
    let (editor, _, _) = make_editor_with_nodes();
    let compat = EditorInterfaceCompat::new(&editor);
    assert_eq!(compat.get_editor_main_screen_name(), "3D");
}

#[test]
fn editor_interface_compat_undo_redo_depth_starts_zero() {
    let (editor, _, _) = make_editor_with_nodes();
    let compat = EditorInterfaceCompat::new(&editor);
    assert_eq!(compat.get_undo_redo_depth(), (0, 0));
}

// ---------------------------------------------------------------------------
// UndoRedoCompat — Godot-named undo/redo operations
// ---------------------------------------------------------------------------

#[test]
fn undo_redo_compat_starts_clean() {
    let (mut editor, _, _) = make_editor_with_nodes();
    let ur = UndoRedoCompat::new(&mut editor);
    assert!(!ur.has_undo());
    assert!(!ur.has_redo());
    assert_eq!(ur.get_version(), 0);
}

#[test]
fn undo_redo_compat_commit_increments_version() {
    let (mut editor, _, _) = make_editor_with_nodes();
    let root = editor.tree().root_id();
    let mut ur = UndoRedoCompat::new(&mut editor);

    let cmd = EditorCommand::AddNode {
        parent_id: root,
        name: "NewNode".into(),
        class_name: "Node2D".into(),
        created_id: None,
    };
    ur.commit_action(cmd).unwrap();
    assert!(ur.has_undo());
    assert!(!ur.has_redo());
    assert_eq!(ur.get_version(), 1);
}

#[test]
fn undo_redo_compat_undo_redo_cycle() {
    let (mut editor, _, _) = make_editor_with_nodes();
    let root = editor.tree().root_id();
    let mut ur = UndoRedoCompat::new(&mut editor);

    let cmd = EditorCommand::AddNode {
        parent_id: root,
        name: "Temp".into(),
        class_name: "Node2D".into(),
        created_id: None,
    };
    ur.commit_action(cmd).unwrap();

    ur.undo().unwrap();
    assert!(!ur.has_undo());
    assert!(ur.has_redo());
    assert_eq!(ur.get_version(), 0);

    ur.redo().unwrap();
    assert!(ur.has_undo());
    assert!(!ur.has_redo());
    assert_eq!(ur.get_version(), 1);
}

#[test]
fn undo_redo_compat_multiple_commits() {
    let (mut editor, _, _) = make_editor_with_nodes();
    let root = editor.tree().root_id();
    let mut ur = UndoRedoCompat::new(&mut editor);

    for i in 0..3 {
        let cmd = EditorCommand::AddNode {
            parent_id: root,
            name: format!("Node{i}"),
            class_name: "Node2D".into(),
            created_id: None,
        };
        ur.commit_action(cmd).unwrap();
    }
    assert_eq!(ur.get_version(), 3);

    ur.undo().unwrap();
    assert_eq!(ur.get_version(), 2);
    ur.undo().unwrap();
    assert_eq!(ur.get_version(), 1);
}
