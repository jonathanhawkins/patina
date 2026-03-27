//! pat-8onny: Scene dock context menu (add node, duplicate, delete, rename).
//!
//! Integration tests covering:
//! 1. SceneTreeDock — refresh, entry listing, selection
//! 2. Add node — via SceneEditor.add_node_to_selected, dock refresh
//! 3. Delete node — via SceneEditor.delete_selected, selection cleared
//! 4. Rename node — via EditorCommand::RenameNode
//! 5. Duplicate node — via EditorCommand::DuplicateNode
//! 6. Undo/redo — all operations reversible
//! 7. Context menu integration — selection drives which actions are available
//! 8. Drag-drop in dock — reorder and reparent via DragDropAction

use gdeditor::dock::{ContextMenuAction, DockPanel, DragDropAction, SceneTreeDock};
use gdeditor::scene_editor::SceneEditor;
use gdeditor::{Editor, EditorCommand};
use gdscene::node::{Node, NodeId};
use gdscene::SceneTree;

// ===========================================================================
// Helpers
// ===========================================================================

/// Helper to get root's children from a SceneTree.
fn root_children(tree: &SceneTree) -> Vec<NodeId> {
    tree.get_node(tree.root_id()).unwrap().children().to_vec()
}

fn setup_tree_with_children() -> SceneTree {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    tree.add_child(root, Node::new("Player", "CharacterBody2D")).unwrap();
    tree.add_child(root, Node::new("Enemy", "Node2D")).unwrap();
    tree.add_child(root, Node::new("UI", "Control")).unwrap();
    tree
}

fn setup_editor_with_children() -> SceneEditor {
    let tree = setup_tree_with_children();
    SceneEditor::with_tree(tree)
}

// ===========================================================================
// 1. SceneTreeDock — refresh, entries, selection
// ===========================================================================

#[test]
fn dock_refresh_populates_entries() {
    let tree = setup_tree_with_children();
    let mut dock = SceneTreeDock::new();
    dock.refresh(&tree);

    assert_eq!(dock.entries().len(), 4); // root + 3 children
    assert_eq!(dock.entries()[0].name, "root");
    assert_eq!(dock.entries()[0].depth, 0);
    assert_eq!(dock.entries()[1].name, "Player");
    assert_eq!(dock.entries()[1].depth, 1);
}

#[test]
fn dock_find_entry_by_id() {
    let tree = setup_tree_with_children();
    let root = tree.root_id();
    let mut dock = SceneTreeDock::new();
    dock.refresh(&tree);

    let entry = dock.find_entry(root).unwrap();
    assert_eq!(entry.name, "root");
}

#[test]
fn dock_select_and_deselect() {
    let tree = setup_tree_with_children();
    let root = tree.root_id();
    let mut dock = SceneTreeDock::new();
    dock.refresh(&tree);

    assert!(dock.selected().is_none());
    assert!(dock.select(root));
    assert_eq!(dock.selected(), Some(root));
    dock.deselect();
    assert!(dock.selected().is_none());
}

#[test]
fn dock_select_nonexistent_returns_false() {
    let mut dock = SceneTreeDock::new();
    // Use a fresh NodeId that won't be in the dock
    assert!(!dock.select(NodeId::next()));
}

#[test]
fn dock_title_is_scene() {
    let dock = SceneTreeDock::new();
    assert_eq!(dock.title(), "Scene");
}

// ===========================================================================
// 2. Add node
// ===========================================================================

#[test]
fn add_node_to_selected_parent() {
    let mut se = setup_editor_with_children();
    let root = se.tree().root_id();
    se.select_node(root);

    let child_id = se.add_node_to_selected("Sprite", "Sprite2D").unwrap();
    assert!(se.tree().get_node(child_id).is_some());
    assert_eq!(se.tree().get_node(child_id).unwrap().name(), "Sprite");
    assert_eq!(se.tree().get_node(child_id).unwrap().class_name(), "Sprite2D");
    assert!(se.is_dirty());
}

#[test]
fn add_node_without_selection_fails() {
    let mut se = setup_editor_with_children();
    let result = se.add_node_to_selected("Node", "Node");
    assert!(result.is_err());
}

#[test]
fn add_node_updates_dock_entries() {
    let mut se = setup_editor_with_children();
    let root = se.tree().root_id();
    se.select_node(root);
    se.add_node_to_selected("NewChild", "Node2D").unwrap();

    let mut dock = SceneTreeDock::new();
    dock.refresh(se.tree());
    assert_eq!(dock.entries().len(), 5); // root + 3 original + 1 new
    assert!(dock.entries().iter().any(|e| e.name == "NewChild"));
}

#[test]
fn add_node_nested() {
    let mut se = setup_editor_with_children();
    let children = root_children(se.tree());
    let player_id = children[0];
    se.select_node(player_id);

    let sprite_id = se.add_node_to_selected("PlayerSprite", "Sprite2D").unwrap();
    let sprite_node = se.tree().get_node(sprite_id).unwrap();
    assert_eq!(sprite_node.parent(), Some(player_id));
}

// ===========================================================================
// 3. Delete node
// ===========================================================================

#[test]
fn delete_selected_node() {
    let mut se = setup_editor_with_children();
    let children = root_children(se.tree());
    let enemy_id = children[1];

    se.select_node(enemy_id);
    se.delete_selected().unwrap();

    assert!(se.tree().get_node(enemy_id).is_none());
    assert!(se.get_selected_node().is_none()); // selection cleared
    assert!(se.is_dirty());
}

#[test]
fn delete_without_selection_fails() {
    let mut se = setup_editor_with_children();
    let result = se.delete_selected();
    assert!(result.is_err());
}

#[test]
fn delete_updates_dock_entries() {
    let mut se = setup_editor_with_children();
    let children = root_children(se.tree());
    se.select_node(children[0]);
    se.delete_selected().unwrap();

    let mut dock = SceneTreeDock::new();
    dock.refresh(se.tree());
    assert_eq!(dock.entries().len(), 3); // root + 2 remaining children
    assert!(!dock.entries().iter().any(|e| e.name == "Player"));
}

// ===========================================================================
// 4. Rename node
// ===========================================================================

#[test]
fn rename_node_via_command() {
    let tree = setup_tree_with_children();
    let children = root_children(&tree);
    let player_id = children[0];

    let mut editor = Editor::new(tree);
    let cmd = EditorCommand::RenameNode {
        node_id: player_id,
        new_name: "Hero".to_string(),
        old_name: String::new(),
    };
    editor.execute(cmd).unwrap();

    assert_eq!(editor.tree().get_node(player_id).unwrap().name(), "Hero");
}

#[test]
fn rename_updates_dock_entry() {
    let tree = setup_tree_with_children();
    let children = root_children(&tree);
    let enemy_id = children[1];

    let mut editor = Editor::new(tree);
    editor.execute(EditorCommand::RenameNode {
        node_id: enemy_id,
        new_name: "Boss".to_string(),
        old_name: String::new(),
    }).unwrap();

    let mut dock = SceneTreeDock::new();
    dock.refresh(editor.tree());
    assert!(dock.entries().iter().any(|e| e.name == "Boss"));
    assert!(!dock.entries().iter().any(|e| e.name == "Enemy"));
}

// ===========================================================================
// 5. Duplicate node
// ===========================================================================

#[test]
fn duplicate_node_via_command() {
    let tree = setup_tree_with_children();
    let children = root_children(&tree);
    let player_id = children[0];

    let mut editor = Editor::new(tree);
    let cmd = EditorCommand::DuplicateNode {
        source_id: player_id,
        created_ids: Vec::new(),
    };
    editor.execute(cmd).unwrap();

    // Should now have 4 children under root (original 3 + 1 duplicate)
    assert_eq!(root_children(editor.tree()).len(), 4);
}

#[test]
fn duplicate_updates_dock_entries() {
    let tree = setup_tree_with_children();
    let children = root_children(&tree);
    let ui_id = children[2];

    let mut editor = Editor::new(tree);
    editor.execute(EditorCommand::DuplicateNode {
        source_id: ui_id,
        created_ids: Vec::new(),
    }).unwrap();

    let mut dock = SceneTreeDock::new();
    dock.refresh(editor.tree());
    assert_eq!(dock.entries().len(), 5); // root + 3 + 1 duplicate
}

// ===========================================================================
// 6. Undo/redo
// ===========================================================================

#[test]
fn undo_add_node() {
    let mut se = setup_editor_with_children();
    let root = se.tree().root_id();
    se.select_node(root);
    let child_id = se.add_node_to_selected("Temp", "Node").unwrap();

    assert!(se.tree().get_node(child_id).is_some());
    se.undo().unwrap();
    assert!(se.tree().get_node(child_id).is_none());
}

#[test]
fn undo_delete_node() {
    let mut se = setup_editor_with_children();
    let children = root_children(se.tree());
    let player_id = children[0];

    se.select_node(player_id);
    se.delete_selected().unwrap();
    assert!(se.tree().get_node(player_id).is_none());
    assert_eq!(root_children(se.tree()).len(), 2);

    se.undo().unwrap();
    // Undo re-creates the node with a fresh NodeId, so check by name + count
    assert_eq!(root_children(se.tree()).len(), 3);
    let has_player = root_children(se.tree()).iter().any(|&id| {
        se.tree().get_node(id).map(|n| n.name() == "Player").unwrap_or(false)
    });
    assert!(has_player);
}

#[test]
fn undo_rename_node() {
    let tree = setup_tree_with_children();
    let children = root_children(&tree);
    let player_id = children[0];

    let mut editor = Editor::new(tree);
    editor.execute(EditorCommand::RenameNode {
        node_id: player_id,
        new_name: "Hero".to_string(),
        old_name: String::new(),
    }).unwrap();
    assert_eq!(editor.tree().get_node(player_id).unwrap().name(), "Hero");

    editor.undo().unwrap();
    assert_eq!(editor.tree().get_node(player_id).unwrap().name(), "Player");
}

#[test]
fn undo_duplicate_node() {
    let tree = setup_tree_with_children();
    let children = root_children(&tree);
    let player_id = children[0];

    let mut editor = Editor::new(tree);
    editor.execute(EditorCommand::DuplicateNode {
        source_id: player_id,
        created_ids: Vec::new(),
    }).unwrap();
    assert_eq!(root_children(editor.tree()).len(), 4);

    editor.undo().unwrap();
    assert_eq!(root_children(editor.tree()).len(), 3);
}

#[test]
fn redo_after_undo() {
    let mut se = setup_editor_with_children();
    let root = se.tree().root_id();
    se.select_node(root);
    se.add_node_to_selected("RedoTest", "Node").unwrap();
    assert_eq!(root_children(se.tree()).len(), 4);

    se.undo().unwrap();
    assert_eq!(root_children(se.tree()).len(), 3);

    se.redo().unwrap();
    // Redo re-creates with a fresh NodeId, so check by name + count
    assert_eq!(root_children(se.tree()).len(), 4);
    let has_redo = root_children(se.tree()).iter().any(|&id| {
        se.tree().get_node(id).map(|n| n.name() == "RedoTest").unwrap_or(false)
    });
    assert!(has_redo);
}

// ===========================================================================
// 7. Context menu integration — selection drives actions
// ===========================================================================

#[test]
fn context_menu_actions_require_selection() {
    let mut se = setup_editor_with_children();

    // No selection — add and delete should fail
    assert!(se.add_node_to_selected("X", "Node").is_err());
    assert!(se.delete_selected().is_err());
}

#[test]
fn context_menu_add_then_delete_cycle() {
    let mut se = setup_editor_with_children();
    let root = se.tree().root_id();

    // Add
    se.select_node(root);
    let new_id = se.add_node_to_selected("Temp", "Node2D").unwrap();
    assert_eq!(root_children(se.tree()).len(), 4);

    // Select the new node and delete it
    se.select_node(new_id);
    se.delete_selected().unwrap();
    assert_eq!(root_children(se.tree()).len(), 3);
}

#[test]
fn context_menu_rename_preserves_hierarchy() {
    let tree = setup_tree_with_children();
    let root = tree.root_id();
    let children = root_children(&tree);
    let player_id = children[0];

    let mut editor = Editor::new(tree);
    editor.execute(EditorCommand::RenameNode {
        node_id: player_id,
        new_name: "MainCharacter".to_string(),
        old_name: String::new(),
    }).unwrap();

    // Hierarchy unchanged
    assert_eq!(root_children(editor.tree()).len(), 3);
    assert_eq!(editor.tree().get_node(player_id).unwrap().parent(), Some(root));
}

#[test]
fn context_menu_duplicate_preserves_class() {
    let tree = setup_tree_with_children();
    let children = root_children(&tree);
    let player_id = children[0];

    let mut editor = Editor::new(tree);
    editor.execute(EditorCommand::DuplicateNode {
        source_id: player_id,
        created_ids: Vec::new(),
    }).unwrap();

    let new_children = root_children(editor.tree());
    let dup_id = new_children[new_children.len() - 1];
    let dup_node = editor.tree().get_node(dup_id).unwrap();
    assert_eq!(dup_node.class_name(), "CharacterBody2D");
}

#[test]
fn available_actions_for_root_excludes_duplicate_and_delete() {
    let tree = setup_tree_with_children();
    let root = tree.root_id();
    let dock = SceneTreeDock::new();

    let actions = dock.available_actions(&tree, root);
    assert!(actions.contains(&"Add Child Node"));
    assert!(actions.contains(&"Rename"));
    assert!(!actions.contains(&"Duplicate"));
    assert!(!actions.contains(&"Delete"));
}

#[test]
fn available_actions_for_child_includes_all() {
    let tree = setup_tree_with_children();
    let children = root_children(&tree);
    let dock = SceneTreeDock::new();

    let actions = dock.available_actions(&tree, children[0]);
    assert!(actions.contains(&"Add Child Node"));
    assert!(actions.contains(&"Rename"));
    assert!(actions.contains(&"Duplicate"));
    assert!(actions.contains(&"Delete"));
}

#[test]
fn apply_context_action_add_node() {
    let mut tree = setup_tree_with_children();
    let root = tree.root_id();
    let mut dock = SceneTreeDock::new();
    dock.refresh(&tree);

    let result = dock.apply_context_action(
        &mut tree,
        root,
        ContextMenuAction::AddNode { class_name: "Sprite2D".to_string() },
    ).unwrap();
    assert!(result.affected_node.is_some());
    assert_eq!(root_children(&tree).len(), 4);
}

#[test]
fn apply_context_action_delete() {
    let mut tree = setup_tree_with_children();
    let children = root_children(&tree);
    let enemy_id = children[1];
    let mut dock = SceneTreeDock::new();
    dock.refresh(&tree);

    let result = dock.apply_context_action(
        &mut tree,
        enemy_id,
        ContextMenuAction::Delete,
    ).unwrap();
    assert!(result.affected_node.is_none());
    assert_eq!(root_children(&tree).len(), 2);
}

#[test]
fn apply_context_action_rename() {
    let mut tree = setup_tree_with_children();
    let children = root_children(&tree);
    let player_id = children[0];
    let mut dock = SceneTreeDock::new();
    dock.refresh(&tree);

    let result = dock.apply_context_action(
        &mut tree,
        player_id,
        ContextMenuAction::Rename { new_name: "Hero".to_string() },
    ).unwrap();
    assert_eq!(result.affected_node, Some(player_id));
    assert_eq!(tree.get_node(player_id).unwrap().name(), "Hero");
}

#[test]
fn apply_context_action_duplicate() {
    let mut tree = setup_tree_with_children();
    let children = root_children(&tree);
    let player_id = children[0];
    let mut dock = SceneTreeDock::new();
    dock.refresh(&tree);

    let result = dock.apply_context_action(
        &mut tree,
        player_id,
        ContextMenuAction::Duplicate,
    ).unwrap();
    assert!(result.affected_node.is_some());
    assert_eq!(root_children(&tree).len(), 4);
}

// ===========================================================================
// 8. Drag-drop in dock
// ===========================================================================

#[test]
fn dock_drag_and_drop_into() {
    let mut tree = setup_tree_with_children();
    let children = root_children(&tree);
    let player_id = children[0];
    let enemy_id = children[1];

    let mut dock = SceneTreeDock::new();
    dock.refresh(&tree);

    // Drag Enemy into Player
    assert!(dock.begin_drag(enemy_id));
    assert!(dock.can_drop(&tree, enemy_id, player_id, DragDropAction::Into));
    dock.apply_drag_drop(&mut tree, enemy_id, player_id, DragDropAction::Into).unwrap();

    // Enemy is now child of Player
    assert_eq!(tree.get_node(enemy_id).unwrap().parent(), Some(player_id));
    // Root has 2 children now
    assert_eq!(root_children(&tree).len(), 2);
    // Drag cleared
    assert!(dock.drag_source().is_none());
}

#[test]
fn dock_cannot_drag_root() {
    let tree = setup_tree_with_children();
    let root = tree.root_id();
    let children = root_children(&tree);
    let player_id = children[0];

    let dock = SceneTreeDock::new();
    assert!(!dock.can_drop(&tree, root, player_id, DragDropAction::Into));
}

#[test]
fn dock_cannot_drag_onto_self() {
    let tree = setup_tree_with_children();
    let children = root_children(&tree);
    let player_id = children[0];

    let dock = SceneTreeDock::new();
    assert!(!dock.can_drop(&tree, player_id, player_id, DragDropAction::Into));
}

#[test]
fn dock_cancel_drag() {
    let tree = setup_tree_with_children();
    let children = root_children(&tree);

    let mut dock = SceneTreeDock::new();
    dock.refresh(&tree);
    dock.begin_drag(children[0]);
    assert!(dock.drag_source().is_some());

    dock.cancel_drag();
    assert!(dock.drag_source().is_none());
}

// ===========================================================================
// 9. Full lifecycle
// ===========================================================================

#[test]
fn full_scene_dock_context_menu_lifecycle() {
    let mut se = setup_editor_with_children();
    let root = se.tree().root_id();
    let mut dock = SceneTreeDock::new();
    dock.refresh(se.tree());
    assert_eq!(dock.entries().len(), 4);

    // 1. Select root, add a node
    se.select_node(root);
    let sprite_id = se.add_node_to_selected("Sprite", "Sprite2D").unwrap();
    dock.refresh(se.tree());
    assert_eq!(dock.entries().len(), 5);

    // 2. Rename the new node
    se.editor_mut().execute(EditorCommand::RenameNode {
        node_id: sprite_id,
        new_name: "HeroSprite".to_string(),
        old_name: String::new(),
    }).unwrap();
    dock.refresh(se.tree());
    assert!(dock.entries().iter().any(|e| e.name == "HeroSprite"));

    // 3. Duplicate it
    se.editor_mut().execute(EditorCommand::DuplicateNode {
        source_id: sprite_id,
        created_ids: Vec::new(),
    }).unwrap();
    dock.refresh(se.tree());
    assert_eq!(dock.entries().len(), 6);

    // 4. Delete the original
    se.select_node(sprite_id);
    se.delete_selected().unwrap();
    dock.refresh(se.tree());
    assert_eq!(dock.entries().len(), 5);

    // 5. Undo delete
    se.undo().unwrap();
    dock.refresh(se.tree());
    assert_eq!(dock.entries().len(), 6);

    // 6. Verify dirty state
    assert!(se.is_dirty());
}
