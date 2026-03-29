//! pat-6m9ky: Integration tests for the `EditorInterface` compatibility layer.
//!
//! Source of truth: `prd/PHASE8_EDITOR_PARITY_AUDIT.md`
//! Classification: Measured for explicit API slice
//!
//! Verifies that the Godot 4-compatible `EditorInterface` API works correctly
//! and that the ClassDB registrations for EditorPlugin and EditorInterface
//! are present.

use gdeditor::{Editor, EditorCommand, EditorInterface};
use gdscene::node::Node;
use gdscene::SceneTree;

fn make_interface() -> EditorInterface {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let mut player = Node::new("Player", "Node2D");
    player.set_property(
        "position",
        gdvariant::Variant::Vector2(gdcore::math::Vector2::new(100.0, 200.0)),
    );
    tree.add_child(root, player).unwrap();
    let enemy = Node::new("Enemy", "Node2D");
    tree.add_child(root, enemy).unwrap();
    let editor = Editor::new(tree);
    EditorInterface::new(editor, "/tmp/compat_test_project")
}

#[test]
fn editor_interface_selection_notifies_plugins() {
    let mut ei = make_interface();
    let root = ei.get_edited_scene_root().root_id();
    let children: Vec<_> = ei
        .get_edited_scene_root()
        .get_node(root)
        .unwrap()
        .children()
        .to_vec();

    // Select player node
    ei.select_node(children[0]);
    assert_eq!(ei.get_selection(), Some(children[0]));

    // Select enemy node
    ei.select_node(children[1]);
    assert_eq!(ei.get_selection(), Some(children[1]));

    // Deselect
    ei.deselect();
    assert!(ei.get_selection().is_none());
}

#[test]
fn editor_interface_command_execution() {
    let mut ei = make_interface();
    let root = ei.get_edited_scene_root().root_id();
    let children: Vec<_> = ei
        .get_edited_scene_root()
        .get_node(root)
        .unwrap()
        .children()
        .to_vec();
    let player_id = children[0];

    // Not modified initially
    assert!(!ei.is_scene_modified());

    // Execute a property change
    let cmd = EditorCommand::SetProperty {
        node_id: player_id,
        property: "visible".to_string(),
        new_value: gdvariant::Variant::Bool(false),
        old_value: gdvariant::Variant::Nil,
    };
    ei.execute_command(cmd).unwrap();
    assert!(ei.is_scene_modified());

    // Undo returns to unmodified
    ei.undo().unwrap();
    assert!(!ei.is_scene_modified());

    // Redo re-applies
    ei.redo().unwrap();
    assert!(ei.is_scene_modified());
}

#[test]
fn editor_interface_add_node_via_command() {
    let mut ei = make_interface();
    let root = ei.get_edited_scene_root().root_id();

    let initial_count = ei
        .get_edited_scene_root()
        .get_node(root)
        .unwrap()
        .children()
        .len();

    let cmd = EditorCommand::AddNode {
        parent_id: root,
        name: "NewSprite".to_string(),
        class_name: "Sprite2D".to_string(),
        created_id: None,
    };
    ei.execute_command(cmd).unwrap();

    let new_count = ei
        .get_edited_scene_root()
        .get_node(root)
        .unwrap()
        .children()
        .len();
    assert_eq!(new_count, initial_count + 1);
}

#[test]
fn editor_interface_settings_mutation() {
    let mut ei = make_interface();

    // Modify settings
    ei.get_editor_settings_mut().auto_save = false;
    assert!(!ei.get_editor_settings().auto_save);

    ei.get_editor_settings_mut().window_size = (1920, 1080);
    assert_eq!(ei.get_editor_settings().window_size, (1920, 1080));
}

#[test]
fn editor_interface_scene_path_tracking() {
    let mut ei = make_interface();

    assert!(ei.get_current_path().is_none());
    ei.set_current_path("res://levels/main.tscn");
    assert_eq!(ei.get_current_path(), Some("res://levels/main.tscn"));
}

#[test]
fn editor_interface_distraction_free_toggle() {
    let mut ei = make_interface();

    assert!(!ei.is_distraction_free_mode_enabled());
    assert!(ei.is_bottom_panel_visible());

    ei.set_distraction_free_mode(true);
    ei.set_bottom_panel_visible(false);

    assert!(ei.is_distraction_free_mode_enabled());
    assert!(!ei.is_bottom_panel_visible());
}

#[test]
fn classdb_editor_plugin_registered() {
    gdobject::class_db::register_editor_classes();

    assert!(gdobject::class_db::class_exists("EditorPlugin"));
    assert!(gdobject::class_db::class_has_method(
        "EditorPlugin",
        "get_editor_interface"
    ));
    assert!(gdobject::class_db::class_has_method(
        "EditorPlugin",
        "add_control_to_dock"
    ));
    assert!(gdobject::class_db::class_has_method(
        "EditorPlugin",
        "add_custom_type"
    ));
}

#[test]
fn classdb_editor_interface_registered() {
    gdobject::class_db::register_editor_classes();

    assert!(gdobject::class_db::class_exists("EditorInterface"));
    assert!(gdobject::class_db::class_has_method(
        "EditorInterface",
        "get_editor_settings"
    ));
    assert!(gdobject::class_db::class_has_method(
        "EditorInterface",
        "get_selection"
    ));
    assert!(gdobject::class_db::class_has_method(
        "EditorInterface",
        "get_inspector"
    ));
    assert!(gdobject::class_db::class_has_method(
        "EditorInterface",
        "open_scene_from_path"
    ));
    assert!(gdobject::class_db::class_has_method(
        "EditorInterface",
        "save_scene"
    ));
}

#[test]
fn editor_interface_project_root() {
    let ei = make_interface();
    assert_eq!(
        ei.get_project_root(),
        std::path::Path::new("/tmp/compat_test_project")
    );
}
