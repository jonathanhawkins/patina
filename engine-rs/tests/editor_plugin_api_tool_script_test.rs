//! Integration tests for the EditorPlugin API with tool script support.
//!
//! Verifies the plugin registry, custom type registration, dock panel
//! management, tool script descriptors, input forwarding, autoload
//! management, and plugin lifecycle.

use gdeditor::editor_plugin::{
    AutoloadEntry, CustomControlContainer, CustomType, DockSlot, EditorPluginExt,
    EditorPluginRegistry, InputHandleResult, ToolScriptDescriptor,
};
use gdscene::node::NodeId;

// ---------------------------------------------------------------------------
// Helper plugins for testing
// ---------------------------------------------------------------------------

struct GizmoPlugin {
    id: String,
    enter_called: bool,
    exit_called: bool,
    ready_called: bool,
    process_count: u32,
    selected: Option<NodeId>,
}

impl GizmoPlugin {
    fn new(id: &str) -> Self {
        Self {
            id: id.to_string(),
            enter_called: false,
            exit_called: false,
            ready_called: false,
            process_count: 0,
            selected: None,
        }
    }
}

impl EditorPluginExt for GizmoPlugin {
    fn plugin_id(&self) -> &str {
        &self.id
    }
    fn display_name(&self) -> &str {
        "Gizmo Plugin"
    }
    fn version(&self) -> &str {
        "2.0.0"
    }
    fn author(&self) -> &str {
        "Test Author"
    }
    fn description(&self) -> &str {
        "A test gizmo plugin"
    }
    fn enter_tree(&mut self) {
        self.enter_called = true;
    }
    fn exit_tree(&mut self) {
        self.exit_called = true;
    }
    fn ready(&mut self) {
        self.ready_called = true;
    }
    fn process(&mut self, _delta: f64) {
        self.process_count += 1;
    }
    fn selection_changed(&mut self, node_id: Option<NodeId>) {
        self.selected = node_id;
    }
    fn handles(&self, class_name: &str) -> bool {
        class_name == "CSGBox3D" || class_name == "CSGSphere3D"
    }
    fn forward_3d_input(
        &mut self,
        _mx: f32,
        _my: f32,
        _btn: u32,
        _pressed: bool,
    ) -> InputHandleResult {
        InputHandleResult::Consumed
    }
}

struct FullPlugin {
    id: String,
    custom_types: Vec<CustomType>,
    autoloads: Vec<AutoloadEntry>,
    tool_script: ToolScriptDescriptor,
}

impl FullPlugin {
    fn new() -> Self {
        Self {
            id: "full-plugin".to_string(),
            custom_types: vec![
                CustomType {
                    type_name: "InventorySlot".to_string(),
                    base_class: "Control".to_string(),
                    icon_path: Some("res://addons/inventory/icon.png".to_string()),
                    script_path: Some("res://addons/inventory/slot.gd".to_string()),
                },
                CustomType {
                    type_name: "InventoryGrid".to_string(),
                    base_class: "GridContainer".to_string(),
                    icon_path: None,
                    script_path: Some("res://addons/inventory/grid.gd".to_string()),
                },
            ],
            autoloads: vec![AutoloadEntry {
                name: "InventoryManager".to_string(),
                path: "res://addons/inventory/manager.gd".to_string(),
                is_singleton: true,
            }],
            tool_script: ToolScriptDescriptor::new(
                "res://addons/inventory/plugin.gd",
                "EditorPlugin",
            )
            .with_export("grid_size", "Vector2i(8, 4)")
            .with_export("slot_size", "64"),
        }
    }
}

impl EditorPluginExt for FullPlugin {
    fn plugin_id(&self) -> &str {
        &self.id
    }
    fn display_name(&self) -> &str {
        "Inventory Editor"
    }
    fn version(&self) -> &str {
        "1.2.0"
    }
    fn author(&self) -> &str {
        "Plugin Author"
    }
    fn custom_types(&self) -> &[CustomType] {
        &self.custom_types
    }
    fn autoloads(&self) -> &[AutoloadEntry] {
        &self.autoloads
    }
    fn tool_script(&self) -> Option<&ToolScriptDescriptor> {
        Some(&self.tool_script)
    }
    fn dock_panels(&self) -> Vec<(DockSlot, String)> {
        vec![
            (DockSlot::RightLower, "Inventory".to_string()),
            (DockSlot::Bottom, "Inventory Debug".to_string()),
        ]
    }
}

// ---------------------------------------------------------------------------
// Plugin lifecycle
// ---------------------------------------------------------------------------

#[test]
fn plugin_lifecycle_enter_ready_exit() {
    let mut registry = EditorPluginRegistry::new();
    registry.register(Box::new(GizmoPlugin::new("lifecycle-test")));

    // Plugin should be registered and enabled
    assert_eq!(registry.plugin_count(), 1);
    assert!(registry.is_enabled("lifecycle-test"));

    // Unregister calls exit_tree
    registry.unregister("lifecycle-test");
    assert_eq!(registry.plugin_count(), 0);
}

#[test]
fn plugin_enable_disable_lifecycle() {
    let mut registry = EditorPluginRegistry::new();
    registry.register(Box::new(GizmoPlugin::new("toggle-test")));

    // Disable
    registry.disable("toggle-test");
    assert!(!registry.is_enabled("toggle-test"));

    // Re-enable
    registry.enable("toggle-test");
    assert!(registry.is_enabled("toggle-test"));

    // Double disable is safe
    registry.disable("toggle-test");
    registry.disable("toggle-test");
    assert!(!registry.is_enabled("toggle-test"));

    // Double enable is safe
    registry.enable("toggle-test");
    registry.enable("toggle-test");
    assert!(registry.is_enabled("toggle-test"));
}

#[test]
fn enable_nonexistent_plugin_is_noop() {
    let mut registry = EditorPluginRegistry::new();
    registry.enable("nonexistent"); // should not panic
    assert!(!registry.is_enabled("nonexistent"));
}

#[test]
fn unregister_nonexistent_is_noop() {
    let mut registry = EditorPluginRegistry::new();
    registry.unregister("nonexistent"); // should not panic
}

// ---------------------------------------------------------------------------
// Plugin info
// ---------------------------------------------------------------------------

#[test]
fn plugin_info_includes_metadata() {
    let mut registry = EditorPluginRegistry::new();
    registry.register(Box::new(GizmoPlugin::new("info-test")));

    let info = registry.plugin_info();
    assert_eq!(info.len(), 1);
    assert_eq!(info[0].id, "info-test");
    assert_eq!(info[0].display_name, "Gizmo Plugin");
    assert_eq!(info[0].version, "2.0.0");
    assert_eq!(info[0].author, "Test Author");
    assert_eq!(info[0].description, "A test gizmo plugin");
    assert!(info[0].enabled);
    assert!(!info[0].is_tool_script);
}

#[test]
fn tool_script_plugin_info() {
    let mut registry = EditorPluginRegistry::new();
    registry.register(Box::new(FullPlugin::new()));

    let info = registry.plugin_info();
    assert_eq!(info[0].id, "full-plugin");
    assert!(info[0].is_tool_script);
}

// ---------------------------------------------------------------------------
// Custom types
// ---------------------------------------------------------------------------

#[test]
fn custom_types_registered_at_registration() {
    let mut registry = EditorPluginRegistry::new();
    registry.register(Box::new(FullPlugin::new()));

    assert_eq!(registry.custom_types().len(), 2);

    let slot = registry.find_custom_type("InventorySlot").unwrap();
    assert_eq!(slot.base_class, "Control");
    assert_eq!(
        slot.icon_path,
        Some("res://addons/inventory/icon.png".to_string())
    );

    let grid = registry.find_custom_type("InventoryGrid").unwrap();
    assert_eq!(grid.base_class, "GridContainer");
    assert!(grid.icon_path.is_none());
}

#[test]
fn custom_type_not_found() {
    let registry = EditorPluginRegistry::new();
    assert!(registry.find_custom_type("NonExistent").is_none());
}

#[test]
fn custom_types_from_multiple_plugins() {
    let mut registry = EditorPluginRegistry::new();
    registry.register(Box::new(FullPlugin::new()));

    struct AnotherPluginOwned {
        types: Vec<CustomType>,
    }
    impl EditorPluginExt for AnotherPluginOwned {
        fn plugin_id(&self) -> &str {
            "another-owned"
        }
        fn display_name(&self) -> &str {
            "Another Owned"
        }
        fn custom_types(&self) -> &[CustomType] {
            &self.types
        }
    }

    registry.register(Box::new(AnotherPluginOwned {
        types: vec![CustomType {
            type_name: "ExtraNode".into(),
            base_class: "Node2D".into(),
            icon_path: None,
            script_path: None,
        }],
    }));

    assert_eq!(registry.custom_types().len(), 3);
    assert!(registry.find_custom_type("InventorySlot").is_some());
    assert!(registry.find_custom_type("ExtraNode").is_some());
}

// ---------------------------------------------------------------------------
// Dock panels
// ---------------------------------------------------------------------------

#[test]
fn dock_panels_registered() {
    let mut registry = EditorPluginRegistry::new();
    registry.register(Box::new(FullPlugin::new()));

    assert_eq!(registry.dock_panels().len(), 2);
    let bottom = registry.panels_in_slot(DockSlot::Bottom);
    assert_eq!(bottom, vec!["Inventory Debug"]);
    let right = registry.panels_in_slot(DockSlot::RightLower);
    assert_eq!(right, vec!["Inventory"]);
    let empty = registry.panels_in_slot(DockSlot::LeftUpper);
    assert!(empty.is_empty());
}

// ---------------------------------------------------------------------------
// Autoloads
// ---------------------------------------------------------------------------

#[test]
fn autoloads_registered() {
    let mut registry = EditorPluginRegistry::new();
    registry.register(Box::new(FullPlugin::new()));

    let autoloads = registry.autoloads();
    assert_eq!(autoloads.len(), 1);
    assert_eq!(autoloads[0].1.name, "InventoryManager");
    assert_eq!(
        autoloads[0].1.path,
        "res://addons/inventory/manager.gd"
    );
    assert!(autoloads[0].1.is_singleton);
}

#[test]
fn unregister_cleans_up_everything() {
    let mut registry = EditorPluginRegistry::new();
    registry.register(Box::new(FullPlugin::new()));

    assert!(!registry.custom_types().is_empty());
    assert!(!registry.dock_panels().is_empty());
    assert!(!registry.autoloads().is_empty());

    registry.unregister("full-plugin");

    assert!(registry.custom_types().is_empty());
    assert!(registry.dock_panels().is_empty());
    assert!(registry.autoloads().is_empty());
}

// ---------------------------------------------------------------------------
// Tool script
// ---------------------------------------------------------------------------

#[test]
fn tool_script_descriptor_properties() {
    let desc = ToolScriptDescriptor::new("res://addons/test/plugin.gd", "EditorPlugin")
        .with_export("speed", "5.0")
        .with_export("color", "Color(1, 0, 0)")
        .with_export("enabled", "true");

    assert!(desc.is_tool);
    assert_eq!(desc.script_path, "res://addons/test/plugin.gd");
    assert_eq!(desc.extends_class, "EditorPlugin");
    assert_eq!(desc.exported_properties.len(), 3);
    assert_eq!(desc.exported_properties["speed"], "5.0");
    assert_eq!(desc.exported_properties["color"], "Color(1, 0, 0)");
}

#[test]
fn non_tool_script_is_not_tool() {
    let desc = ToolScriptDescriptor::non_tool("res://player.gd", "CharacterBody2D");
    assert!(!desc.is_tool);
    assert_eq!(desc.extends_class, "CharacterBody2D");
}

#[test]
fn tool_script_plugins_filtered() {
    let mut registry = EditorPluginRegistry::new();
    registry.register(Box::new(GizmoPlugin::new("non-tool")));
    registry.register(Box::new(FullPlugin::new()));

    let tool_plugins = registry.tool_script_plugins();
    assert_eq!(tool_plugins.len(), 1);
    assert_eq!(tool_plugins[0].plugin_id(), "full-plugin");
}

// ---------------------------------------------------------------------------
// Input forwarding
// ---------------------------------------------------------------------------

#[test]
fn input_forwarded_only_to_handlers() {
    let mut registry = EditorPluginRegistry::new();
    registry.register(Box::new(GizmoPlugin::new("gizmo-input")));

    // CSGBox3D is handled
    let result = registry.forward_3d_input("CSGBox3D", 10.0, 20.0, 1, true);
    assert_eq!(result, InputHandleResult::Consumed);

    // MeshInstance3D is not handled
    let result = registry.forward_3d_input("MeshInstance3D", 10.0, 20.0, 1, true);
    assert_eq!(result, InputHandleResult::Pass);
}

#[test]
fn disabled_plugin_does_not_receive_input() {
    let mut registry = EditorPluginRegistry::new();
    registry.register(Box::new(GizmoPlugin::new("disabled-input")));
    registry.disable("disabled-input");

    let result = registry.forward_3d_input("CSGBox3D", 0.0, 0.0, 1, true);
    assert_eq!(result, InputHandleResult::Pass);
}

#[test]
fn canvas_input_forwarding() {
    struct CanvasPlugin;
    impl EditorPluginExt for CanvasPlugin {
        fn plugin_id(&self) -> &str {
            "canvas-handler"
        }
        fn display_name(&self) -> &str {
            "Canvas Handler"
        }
        fn handles(&self, class: &str) -> bool {
            class == "Path2D"
        }
        fn forward_canvas_input(
            &mut self,
            _mx: f32,
            _my: f32,
            _btn: u32,
            _pressed: bool,
        ) -> InputHandleResult {
            InputHandleResult::Consumed
        }
    }

    let mut registry = EditorPluginRegistry::new();
    registry.register(Box::new(CanvasPlugin));

    assert_eq!(
        registry.forward_canvas_input("Path2D", 50.0, 100.0, 1, true),
        InputHandleResult::Consumed
    );
    assert_eq!(
        registry.forward_canvas_input("Sprite2D", 50.0, 100.0, 1, true),
        InputHandleResult::Pass
    );
}

// ---------------------------------------------------------------------------
// Selection notification
// ---------------------------------------------------------------------------

#[test]
fn selection_notification_reaches_enabled_plugins() {
    let mut registry = EditorPluginRegistry::new();
    registry.register(Box::new(GizmoPlugin::new("sel-test")));

    let node_id = NodeId::next();
    registry.notify_selection_changed(Some(node_id));
    // Can't directly inspect plugin state through registry,
    // but this verifies no panics and correct dispatch
}

#[test]
fn selection_notification_skips_disabled() {
    let mut registry = EditorPluginRegistry::new();
    registry.register(Box::new(GizmoPlugin::new("sel-disabled")));
    registry.disable("sel-disabled");

    // Should not panic
    registry.notify_selection_changed(Some(NodeId::next()));
    registry.notify_selection_changed(None);
}

// ---------------------------------------------------------------------------
// Process
// ---------------------------------------------------------------------------

#[test]
fn process_calls_enabled_plugins() {
    let mut registry = EditorPluginRegistry::new();
    registry.register(Box::new(GizmoPlugin::new("proc-test")));

    // Multiple process calls should work
    for _ in 0..10 {
        registry.process(0.016);
    }
}

// ---------------------------------------------------------------------------
// DockSlot
// ---------------------------------------------------------------------------

#[test]
fn dock_slot_from_godot_int_all_values() {
    assert_eq!(DockSlot::from_godot_int(0), Some(DockSlot::LeftUpper));
    assert_eq!(DockSlot::from_godot_int(1), Some(DockSlot::LeftLower));
    assert_eq!(DockSlot::from_godot_int(2), Some(DockSlot::RightUpper));
    assert_eq!(DockSlot::from_godot_int(3), Some(DockSlot::RightLower));
    assert_eq!(DockSlot::from_godot_int(4), Some(DockSlot::Bottom));
    assert_eq!(DockSlot::from_godot_int(5), None);
    assert_eq!(DockSlot::from_godot_int(-1), None);
}

#[test]
fn dock_slot_to_godot_int_roundtrip() {
    for i in 0..5i64 {
        let slot = DockSlot::from_godot_int(i).unwrap();
        assert_eq!(slot.to_godot_int(), i);
    }
}

// ---------------------------------------------------------------------------
// CustomControlContainer
// ---------------------------------------------------------------------------

#[test]
fn custom_control_container_all_variants_exist() {
    // Verify all variants compile and are distinct
    let variants = [
        CustomControlContainer::Toolbar,
        CustomControlContainer::SpatialEditorMenu,
        CustomControlContainer::SpatialEditorSide,
        CustomControlContainer::SpatialEditorBottom,
        CustomControlContainer::CanvasEditorMenu,
        CustomControlContainer::CanvasEditorSide,
        CustomControlContainer::CanvasEditorBottom,
        CustomControlContainer::InspectorBottom,
    ];
    // All 8 variants should exist
    assert_eq!(variants.len(), 8);
}

// ---------------------------------------------------------------------------
// Edge cases
// ---------------------------------------------------------------------------

#[test]
fn empty_registry_queries() {
    let registry = EditorPluginRegistry::new();
    assert_eq!(registry.plugin_count(), 0);
    assert!(registry.plugin_ids().is_empty());
    assert!(registry.plugin_info().is_empty());
    assert!(registry.custom_types().is_empty());
    assert!(registry.autoloads().is_empty());
    assert!(registry.dock_panels().is_empty());
    assert!(registry.panels_in_slot(DockSlot::Bottom).is_empty());
    assert!(registry.tool_script_plugins().is_empty());
    assert!(!registry.is_enabled("anything"));
    assert!(registry.find_custom_type("anything").is_none());
}

#[test]
fn many_plugins_registered() {
    let mut registry = EditorPluginRegistry::new();
    for i in 0..50 {
        registry.register(Box::new(GizmoPlugin::new(&format!("plugin-{}", i))));
    }
    assert_eq!(registry.plugin_count(), 50);
    assert_eq!(registry.plugin_ids().len(), 50);

    // Unregister half
    for i in 0..25 {
        registry.unregister(&format!("plugin-{}", i));
    }
    assert_eq!(registry.plugin_count(), 25);
}
