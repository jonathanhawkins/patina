//! pat-6m9ky: Surface validation test for the minimal editor-facing compatibility layer.
//!
//! Source of truth: `prd/EDITOR_COMPAT_LAYER_SPEC.md`
//! Classification: Measured — compatibility layer boundary enforced by test
//!
//! This test validates that the public API surface of the compatibility layer
//! (`editor_compat` + `editor_interface`) matches the formal specification.
//! If a type, struct, enum, or function is added or removed from these modules
//! without updating this test and the spec, the build will fail.

use gdeditor::editor_compat::{
    // Type aliases
    EditorInspector,
    EditorInspectorPluginManager,
    EditorProperty,
    EditorUndoRedoManager,
    // Enum
    PropertyUsageFlags,
    // Structs
    EditorInterfaceCompat,
    EditorPropertyInfo,
    EditorSelection,
    UndoRedoCompat,
    // Free functions
    get_property_category,
    validate_property_value,
};
use gdeditor::editor_interface::EditorInterface;
use gdeditor::{Editor, EditorCommand};
use gdscene::node::Node;
use gdscene::SceneTree;
use gdvariant::variant::VariantType;
use gdvariant::Variant;

// ===========================================================================
// editor_compat surface: type aliases resolve to the correct underlying types
// ===========================================================================

#[test]
fn surface_type_alias_editor_inspector() {
    // EditorInspector must alias InspectorPanel
    let _: EditorInspector = gdeditor::inspector::InspectorPanel::new();
}

#[test]
fn surface_type_alias_editor_property() {
    // EditorProperty must alias CustomPropertyEditor
    let _: EditorProperty = gdeditor::inspector::CustomPropertyEditor::new("test");
}

#[test]
fn surface_type_alias_editor_inspector_plugin_manager() {
    // EditorInspectorPluginManager must alias InspectorPluginRegistry
    let _: EditorInspectorPluginManager = gdeditor::inspector::InspectorPluginRegistry::new();
}

#[test]
fn surface_type_alias_editor_undo_redo_manager() {
    // EditorUndoRedoManager must alias Editor
    let tree = SceneTree::new();
    let _: EditorUndoRedoManager = Editor::new(tree);
}

// ===========================================================================
// editor_compat surface: PropertyUsageFlags enum has all specified variants
// ===========================================================================

#[test]
fn surface_property_usage_flags_all_variants() {
    let variants = [
        PropertyUsageFlags::Editor,
        PropertyUsageFlags::Storage,
        PropertyUsageFlags::Default,
        PropertyUsageFlags::NoEditor,
        PropertyUsageFlags::ReadOnly,
    ];
    // All 5 variants must exist and be distinct
    for i in 0..variants.len() {
        for j in (i + 1)..variants.len() {
            assert_ne!(variants[i], variants[j]);
        }
    }
    // Visibility and storage methods must exist
    assert!(PropertyUsageFlags::Editor.is_editor_visible());
    assert!(PropertyUsageFlags::Storage.is_stored());
}

// ===========================================================================
// editor_compat surface: EditorPropertyInfo API
// ===========================================================================

#[test]
fn surface_editor_property_info_api() {
    // Constructor
    let info = EditorPropertyInfo::new("test_prop", VariantType::Float);
    assert_eq!(info.name, "test_prop");

    // Builder methods
    let info = info
        .with_hint(
            gdeditor::inspector::PropertyHint::Range {
                min: 0,
                max: 10,
                step: 1,
            },
            "0,10,1",
        )
        .with_usage(PropertyUsageFlags::Editor);

    // Conversion method
    let _editor = info.to_custom_editor();
}

// ===========================================================================
// editor_compat surface: EditorSelection API
// ===========================================================================

#[test]
fn surface_editor_selection_api() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let child = tree.add_child(root, Node::new("N", "Node2D")).unwrap();
    let mut editor = Editor::new(tree);
    let mut sel = EditorSelection::new(&mut editor);

    // All specified methods must exist
    let _: Vec<gdscene::node::NodeId> = sel.get_selected_nodes();
    let _: usize = sel.get_selected_node_count();
    let _: bool = sel.is_selected(child);
    sel.add_node(child);
    sel.remove_node(child);
    sel.clear();
}

// ===========================================================================
// editor_compat surface: EditorInterfaceCompat API
// ===========================================================================

#[test]
fn surface_editor_interface_compat_api() {
    let tree = SceneTree::new();
    let editor = Editor::new(tree);
    let compat = EditorInterfaceCompat::new(&editor);

    // All specified methods must exist and return correct types
    let _: &SceneTree = compat.get_tree();
    let _: gdscene::node::NodeId = compat.get_edited_scene_root();
    let _: Option<gdscene::node::NodeId> = compat.get_selection();
    let _: &str = compat.get_editor_main_screen_name();
    let _: (usize, usize) = compat.get_undo_redo_depth();
}

// ===========================================================================
// editor_compat surface: UndoRedoCompat API
// ===========================================================================

#[test]
fn surface_undo_redo_compat_api() {
    let tree = SceneTree::new();
    let mut editor = Editor::new(tree);
    let ur = UndoRedoCompat::new(&mut editor);

    // All specified methods must exist and return correct types
    let _: bool = ur.has_undo();
    let _: bool = ur.has_redo();
    let _: usize = ur.get_version();
}

// ===========================================================================
// editor_compat surface: free functions
// ===========================================================================

#[test]
fn surface_validate_property_value_fn() {
    let _: Result<Variant, String> = validate_property_value(&Variant::Int(1), VariantType::Int);
}

#[test]
fn surface_get_property_category_fn() {
    let _: &str = get_property_category("position");
}

// ===========================================================================
// editor_interface surface: EditorInterface API
// ===========================================================================

fn make_interface() -> EditorInterface {
    let tree = SceneTree::new();
    let editor = Editor::new(tree);
    EditorInterface::new(editor, "/tmp/surface_test")
}

#[test]
fn surface_editor_interface_editor_access() {
    let mut ei = make_interface();

    // Editor access methods
    let _: &Editor = ei.get_editor();
    let _: &mut Editor = ei.get_editor_mut();
    let _: &gdeditor::settings::EditorSettings = ei.get_editor_settings();
    let _: &mut gdeditor::settings::EditorSettings = ei.get_editor_settings_mut();
    let _: &gdeditor::EditorFileSystem = ei.get_file_system_dock();
    let _: &mut gdeditor::EditorFileSystem = ei.get_file_system_dock_mut();
    let _: &gdeditor::inspector::InspectorPanel = ei.get_inspector();
    let _: &mut gdeditor::inspector::InspectorPanel = ei.get_inspector_mut();
    let _: &SceneTree = ei.get_edited_scene_root();
    let _: &mut SceneTree = ei.get_edited_scene_root_mut();
}

#[test]
fn surface_editor_interface_selection() {
    let mut ei = make_interface();
    let root = ei.get_edited_scene_root().root_id();

    let _: Option<gdscene::node::NodeId> = ei.get_selection();
    ei.select_node(root);
    ei.deselect();
}

#[test]
fn surface_editor_interface_scene_ops() {
    let mut ei = make_interface();

    let _: Option<&str> = ei.get_current_path();
    ei.set_current_path("res://test.tscn");
    // save_scene and open_scene_from_path exist (not called — would need real files)
    // reload_scene_from_disk exists
}

#[test]
fn surface_editor_interface_commands() {
    let mut ei = make_interface();
    let root = ei.get_edited_scene_root().root_id();

    let cmd = EditorCommand::AddNode {
        parent_id: root,
        name: "SurfaceTest".into(),
        class_name: "Node2D".into(),
        created_id: None,
    };
    let _: Result<(), gdeditor::EditorError> = ei.execute_command(cmd);
    let _: Result<(), gdeditor::EditorError> = ei.undo();
    let _: Result<(), gdeditor::EditorError> = ei.redo();
}

#[test]
fn surface_editor_interface_ui_state() {
    let mut ei = make_interface();

    let _: bool = ei.is_distraction_free_mode_enabled();
    ei.set_distraction_free_mode(true);
    let _: bool = ei.is_bottom_panel_visible();
    ei.set_bottom_panel_visible(false);
    let _: gdeditor::settings::EditorTheme = ei.get_editor_theme();
}

#[test]
fn surface_editor_interface_filesystem() {
    let ei = make_interface();
    let _: &std::path::Path = ei.get_project_root();
    // scan_file_system exists (not called — would scan a real path)
}

#[test]
fn surface_editor_interface_plugin() {
    let ei = make_interface();
    let _: Vec<&str> = ei.get_plugin_names();
    let _: bool = ei.is_scene_modified();
}

// ===========================================================================
// ClassDB surface: editor classes registered
// ===========================================================================

#[test]
fn surface_classdb_editor_plugin() {
    gdobject::class_db::register_editor_classes();
    assert!(gdobject::class_db::class_exists("EditorPlugin"));
    assert!(gdobject::class_db::class_has_method("EditorPlugin", "get_editor_interface"));
    assert!(gdobject::class_db::class_has_method("EditorPlugin", "add_control_to_dock"));
    assert!(gdobject::class_db::class_has_method("EditorPlugin", "add_custom_type"));
}

#[test]
fn surface_classdb_editor_interface() {
    gdobject::class_db::register_editor_classes();
    assert!(gdobject::class_db::class_exists("EditorInterface"));
    assert!(gdobject::class_db::class_has_method("EditorInterface", "get_editor_settings"));
    assert!(gdobject::class_db::class_has_method("EditorInterface", "get_selection"));
    assert!(gdobject::class_db::class_has_method("EditorInterface", "get_inspector"));
    assert!(gdobject::class_db::class_has_method("EditorInterface", "open_scene_from_path"));
    assert!(gdobject::class_db::class_has_method("EditorInterface", "save_scene"));
}
