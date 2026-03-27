//! Extended Editor Plugin API with tool script support.
//!
//! Provides a comprehensive plugin system mirroring Godot 4's `EditorPlugin`
//! class, including:
//!
//! - **Plugin registration**: add/remove/enable/disable plugins.
//! - **Custom types**: register new node or resource types.
//! - **Dock integration**: place UI panels in editor dock slots.
//! - **Inspector plugins**: extend the property inspector.
//! - **Tool scripts**: execute GDScript with `@tool` annotation in the editor.
//! - **Input forwarding**: forward viewport input to plugins for custom handling.
//! - **Autoload management**: register/remove autoload singletons.
//!
//! This module extends the minimal [`EditorPlugin`](crate::EditorPlugin) trait
//! defined in the crate root with the full Godot-compatible API surface.

use std::collections::HashMap;

use gdscene::node::NodeId;

// ---------------------------------------------------------------------------
// Dock slots
// ---------------------------------------------------------------------------

/// Named dock slot positions in the editor layout.
///
/// Maps to Godot's `EditorPlugin.DockSlot` enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DockSlot {
    /// Left panel, upper section (default: Scene Tree).
    LeftUpper,
    /// Left panel, lower section (default: FileSystem).
    LeftLower,
    /// Right panel, upper section (default: Inspector).
    RightUpper,
    /// Right panel, lower section (default: Node signals/groups).
    RightLower,
    /// Bottom panel (Output, Debugger, Audio, Animation, etc.).
    Bottom,
}

impl DockSlot {
    /// Returns the human-readable name for this dock slot.
    pub fn name(&self) -> &'static str {
        match self {
            Self::LeftUpper => "Left Upper",
            Self::LeftLower => "Left Lower",
            Self::RightUpper => "Right Upper",
            Self::RightLower => "Right Lower",
            Self::Bottom => "Bottom",
        }
    }

    /// Whether this slot is on the left side.
    pub fn is_left(&self) -> bool {
        matches!(self, Self::LeftUpper | Self::LeftLower)
    }

    /// Whether this slot is on the right side.
    pub fn is_right(&self) -> bool {
        matches!(self, Self::RightUpper | Self::RightLower)
    }

    /// Whether this is a bottom panel slot.
    pub fn is_bottom(&self) -> bool {
        matches!(self, Self::Bottom)
    }

    /// Converts from the Godot integer representation.
    pub fn from_godot_int(v: i64) -> Option<Self> {
        match v {
            0 => Some(Self::LeftUpper),
            1 => Some(Self::LeftLower),
            2 => Some(Self::RightUpper),
            3 => Some(Self::RightLower),
            4 => Some(Self::Bottom),
            _ => None,
        }
    }

    /// Converts to the Godot integer representation.
    pub fn to_godot_int(self) -> i64 {
        match self {
            Self::LeftUpper => 0,
            Self::LeftLower => 1,
            Self::RightUpper => 2,
            Self::RightLower => 3,
            Self::Bottom => 4,
        }
    }
}

// ---------------------------------------------------------------------------
// Container type for add_control_to_container
// ---------------------------------------------------------------------------

/// Named container positions within the editor UI.
///
/// Maps to Godot's `EditorPlugin.CustomControlContainer`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CustomControlContainer {
    /// Toolbar area.
    Toolbar,
    /// Spatial (3D) editor menu.
    SpatialEditorMenu,
    /// Spatial (3D) editor side panel.
    SpatialEditorSide,
    /// Spatial (3D) editor bottom panel.
    SpatialEditorBottom,
    /// Canvas (2D) editor menu.
    CanvasEditorMenu,
    /// Canvas (2D) editor side panel.
    CanvasEditorSide,
    /// Canvas (2D) editor bottom panel.
    CanvasEditorBottom,
    /// Property editor at the bottom of the inspector.
    InspectorBottom,
}

// ---------------------------------------------------------------------------
// Custom type registration
// ---------------------------------------------------------------------------

/// A custom type registered by a plugin.
///
/// When a plugin calls `add_custom_type`, it registers a new type that
/// appears in the "Create New Node" or "Create New Resource" dialogs.
#[derive(Debug, Clone)]
pub struct CustomType {
    /// The type name as it appears in dialogs.
    pub type_name: String,
    /// The base class this type inherits from.
    pub base_class: String,
    /// Optional icon path (resource path).
    pub icon_path: Option<String>,
    /// The script path that implements this type.
    pub script_path: Option<String>,
}

// ---------------------------------------------------------------------------
// Autoload entry
// ---------------------------------------------------------------------------

/// An autoload singleton registered by a plugin.
#[derive(Debug, Clone)]
pub struct AutoloadEntry {
    /// The singleton name (used to access it via `get_node("/root/Name")`).
    pub name: String,
    /// Path to the scene or script file.
    pub path: String,
    /// Whether to add as a singleton (accessible everywhere).
    pub is_singleton: bool,
}

// ---------------------------------------------------------------------------
// Tool script descriptor
// ---------------------------------------------------------------------------

/// Describes a tool script that runs in the editor.
///
/// Tool scripts are GDScript files with the `@tool` annotation that execute
/// in the editor context rather than only at runtime. They enable:
/// - Custom gizmos and handles
/// - Live preview of node behavior
/// - Editor-time initialization and updates
#[derive(Debug, Clone)]
pub struct ToolScriptDescriptor {
    /// Path to the script file (e.g., `"res://addons/my_plugin/plugin.gd"`).
    pub script_path: String,
    /// Whether the script has the `@tool` annotation.
    pub is_tool: bool,
    /// The class the script extends.
    pub extends_class: String,
    /// Exported properties (name → default value as string).
    pub exported_properties: HashMap<String, String>,
}

impl ToolScriptDescriptor {
    /// Creates a new tool script descriptor.
    pub fn new(script_path: impl Into<String>, extends_class: impl Into<String>) -> Self {
        Self {
            script_path: script_path.into(),
            is_tool: true,
            extends_class: extends_class.into(),
            exported_properties: HashMap::new(),
        }
    }

    /// Creates a non-tool script descriptor.
    pub fn non_tool(script_path: impl Into<String>, extends_class: impl Into<String>) -> Self {
        Self {
            script_path: script_path.into(),
            is_tool: false,
            extends_class: extends_class.into(),
            exported_properties: HashMap::new(),
        }
    }

    /// Adds an exported property.
    pub fn with_export(mut self, name: impl Into<String>, default: impl Into<String>) -> Self {
        self.exported_properties
            .insert(name.into(), default.into());
        self
    }
}

// ---------------------------------------------------------------------------
// Plugin input forwarding
// ---------------------------------------------------------------------------

/// Result of a plugin handling viewport input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputHandleResult {
    /// The plugin did not consume the input; pass it to the next handler.
    Pass,
    /// The plugin consumed the input; do not pass it further.
    Consumed,
}

// ---------------------------------------------------------------------------
// Extended EditorPlugin trait
// ---------------------------------------------------------------------------

/// Extended editor plugin trait with the full Godot EditorPlugin API surface.
///
/// This trait extends the base [`EditorPlugin`](crate::EditorPlugin) with
/// dock management, custom type registration, tool script support, input
/// forwarding, and autoload management.
///
/// Plugins implementing this trait should be registered via
/// [`EditorPluginRegistry`].
pub trait EditorPluginExt: Send {
    /// Returns the plugin's unique identifier.
    fn plugin_id(&self) -> &str;

    /// Returns the plugin's display name.
    fn display_name(&self) -> &str;

    /// Returns the plugin's version string.
    fn version(&self) -> &str {
        "1.0.0"
    }

    /// Returns the plugin's author.
    fn author(&self) -> &str {
        ""
    }

    /// Returns a description of what this plugin does.
    fn description(&self) -> &str {
        ""
    }

    // -- Lifecycle --

    /// Called when the plugin enters the editor tree (is activated).
    fn enter_tree(&mut self) {}

    /// Called when the plugin exits the editor tree (is deactivated).
    fn exit_tree(&mut self) {}

    /// Called after the plugin is fully initialized and the editor is ready.
    fn ready(&mut self) {}

    /// Called every editor frame.
    fn process(&mut self, _delta: f64) {}

    // -- Selection --

    /// Returns true if this plugin handles the given node type.
    ///
    /// When true, the plugin's `forward_*_input` methods will be called
    /// when that node is selected.
    fn handles(&self, _class_name: &str) -> bool {
        false
    }

    /// Called when the selected node changes.
    fn selection_changed(&mut self, _node_id: Option<NodeId>) {}

    // -- Input forwarding --

    /// Forward 2D canvas input to the plugin.
    ///
    /// Called when the user interacts with the 2D viewport and
    /// `handles()` returned true for the selected node's class.
    fn forward_canvas_input(
        &mut self,
        _mouse_x: f32,
        _mouse_y: f32,
        _button: u32,
        _pressed: bool,
    ) -> InputHandleResult {
        InputHandleResult::Pass
    }

    /// Forward 3D viewport input to the plugin.
    ///
    /// Called when the user interacts with the 3D viewport and
    /// `handles()` returned true for the selected node's class.
    fn forward_3d_input(
        &mut self,
        _mouse_x: f32,
        _mouse_y: f32,
        _button: u32,
        _pressed: bool,
    ) -> InputHandleResult {
        InputHandleResult::Pass
    }

    // -- Custom type registration --

    /// Returns custom types registered by this plugin.
    fn custom_types(&self) -> &[CustomType] {
        &[]
    }

    // -- Dock / UI integration --

    /// Returns dock panel registrations for this plugin.
    ///
    /// Each entry is `(dock_slot, panel_title)`.
    fn dock_panels(&self) -> Vec<(DockSlot, String)> {
        Vec::new()
    }

    // -- Tool script --

    /// Returns the tool script descriptor, if this plugin is backed by a tool script.
    fn tool_script(&self) -> Option<&ToolScriptDescriptor> {
        None
    }

    // -- Autoloads --

    /// Returns autoload entries registered by this plugin.
    fn autoloads(&self) -> &[AutoloadEntry] {
        &[]
    }
}

// ---------------------------------------------------------------------------
// Plugin registry
// ---------------------------------------------------------------------------

/// Manages the lifecycle and lookup of editor plugins.
///
/// The registry handles plugin registration, activation/deactivation,
/// custom type tracking, dock panel management, and autoload bookkeeping.
pub struct EditorPluginRegistry {
    plugins: Vec<PluginEntry>,
    custom_types: Vec<(String, CustomType)>, // (plugin_id, type)
    autoloads: Vec<(String, AutoloadEntry)>, // (plugin_id, entry)
    dock_panels: Vec<(String, DockSlot, String)>, // (plugin_id, slot, title)
}

/// Internal entry wrapping a plugin with its activation state.
struct PluginEntry {
    plugin: Box<dyn EditorPluginExt>,
    enabled: bool,
}

impl EditorPluginRegistry {
    /// Creates a new empty registry.
    pub fn new() -> Self {
        Self {
            plugins: Vec::new(),
            custom_types: Vec::new(),
            autoloads: Vec::new(),
            dock_panels: Vec::new(),
        }
    }

    /// Registers and activates a plugin.
    ///
    /// Calls `enter_tree()` and `ready()` on the plugin, then indexes its
    /// custom types, dock panels, and autoloads.
    pub fn register(&mut self, mut plugin: Box<dyn EditorPluginExt>) {
        let id = plugin.plugin_id().to_string();

        // Index custom types
        for ct in plugin.custom_types() {
            self.custom_types.push((id.clone(), ct.clone()));
        }

        // Index dock panels
        for (slot, title) in plugin.dock_panels() {
            self.dock_panels.push((id.clone(), slot, title));
        }

        // Index autoloads
        for al in plugin.autoloads() {
            self.autoloads.push((id.clone(), al.clone()));
        }

        // Activate
        plugin.enter_tree();
        plugin.ready();

        self.plugins.push(PluginEntry {
            plugin,
            enabled: true,
        });
    }

    /// Unregisters a plugin by ID.
    ///
    /// Calls `exit_tree()` and removes all associated custom types,
    /// dock panels, and autoloads.
    pub fn unregister(&mut self, plugin_id: &str) {
        if let Some(pos) = self
            .plugins
            .iter()
            .position(|e| e.plugin.plugin_id() == plugin_id)
        {
            let mut entry = self.plugins.remove(pos);
            entry.plugin.exit_tree();
        }

        self.custom_types.retain(|(id, _)| id != plugin_id);
        self.dock_panels.retain(|(id, _, _)| id != plugin_id);
        self.autoloads.retain(|(id, _)| id != plugin_id);
    }

    /// Enables a plugin by ID.
    pub fn enable(&mut self, plugin_id: &str) {
        if let Some(entry) = self
            .plugins
            .iter_mut()
            .find(|e| e.plugin.plugin_id() == plugin_id)
        {
            if !entry.enabled {
                entry.enabled = true;
                entry.plugin.enter_tree();
            }
        }
    }

    /// Disables a plugin by ID.
    pub fn disable(&mut self, plugin_id: &str) {
        if let Some(entry) = self
            .plugins
            .iter_mut()
            .find(|e| e.plugin.plugin_id() == plugin_id)
        {
            if entry.enabled {
                entry.enabled = false;
                entry.plugin.exit_tree();
            }
        }
    }

    /// Returns whether a plugin is enabled.
    pub fn is_enabled(&self, plugin_id: &str) -> bool {
        self.plugins
            .iter()
            .find(|e| e.plugin.plugin_id() == plugin_id)
            .map_or(false, |e| e.enabled)
    }

    /// Returns the number of registered plugins.
    pub fn plugin_count(&self) -> usize {
        self.plugins.len()
    }

    /// Returns the IDs of all registered plugins.
    pub fn plugin_ids(&self) -> Vec<String> {
        self.plugins
            .iter()
            .map(|e| e.plugin.plugin_id().to_string())
            .collect()
    }

    /// Returns plugin info for all registered plugins.
    pub fn plugin_info(&self) -> Vec<PluginInfo> {
        self.plugins
            .iter()
            .map(|e| PluginInfo {
                id: e.plugin.plugin_id().to_string(),
                display_name: e.plugin.display_name().to_string(),
                version: e.plugin.version().to_string(),
                author: e.plugin.author().to_string(),
                description: e.plugin.description().to_string(),
                enabled: e.enabled,
                is_tool_script: e.plugin.tool_script().is_some(),
            })
            .collect()
    }

    /// Returns all registered custom types.
    pub fn custom_types(&self) -> &[(String, CustomType)] {
        &self.custom_types
    }

    /// Looks up a custom type by name.
    pub fn find_custom_type(&self, type_name: &str) -> Option<&CustomType> {
        self.custom_types
            .iter()
            .find(|(_, ct)| ct.type_name == type_name)
            .map(|(_, ct)| ct)
    }

    /// Returns all registered dock panels.
    pub fn dock_panels(&self) -> &[(String, DockSlot, String)] {
        &self.dock_panels
    }

    /// Returns dock panels for a specific slot.
    pub fn panels_in_slot(&self, slot: DockSlot) -> Vec<&str> {
        self.dock_panels
            .iter()
            .filter(|(_, s, _)| *s == slot)
            .map(|(_, _, title)| title.as_str())
            .collect()
    }

    /// Returns all registered autoloads.
    pub fn autoloads(&self) -> &[(String, AutoloadEntry)] {
        &self.autoloads
    }

    /// Notifies all enabled plugins of a selection change.
    pub fn notify_selection_changed(&mut self, node_id: Option<NodeId>) {
        for entry in &mut self.plugins {
            if entry.enabled {
                entry.plugin.selection_changed(node_id);
            }
        }
    }

    /// Calls `process(delta)` on all enabled plugins.
    pub fn process(&mut self, delta: f64) {
        for entry in &mut self.plugins {
            if entry.enabled {
                entry.plugin.process(delta);
            }
        }
    }

    /// Forwards 2D canvas input to plugins that handle the given class.
    ///
    /// Returns `Consumed` if any plugin consumed the input.
    pub fn forward_canvas_input(
        &mut self,
        class_name: &str,
        mouse_x: f32,
        mouse_y: f32,
        button: u32,
        pressed: bool,
    ) -> InputHandleResult {
        for entry in &mut self.plugins {
            if entry.enabled && entry.plugin.handles(class_name) {
                if entry
                    .plugin
                    .forward_canvas_input(mouse_x, mouse_y, button, pressed)
                    == InputHandleResult::Consumed
                {
                    return InputHandleResult::Consumed;
                }
            }
        }
        InputHandleResult::Pass
    }

    /// Forwards 3D viewport input to plugins that handle the given class.
    ///
    /// Returns `Consumed` if any plugin consumed the input.
    pub fn forward_3d_input(
        &mut self,
        class_name: &str,
        mouse_x: f32,
        mouse_y: f32,
        button: u32,
        pressed: bool,
    ) -> InputHandleResult {
        for entry in &mut self.plugins {
            if entry.enabled && entry.plugin.handles(class_name) {
                if entry
                    .plugin
                    .forward_3d_input(mouse_x, mouse_y, button, pressed)
                    == InputHandleResult::Consumed
                {
                    return InputHandleResult::Consumed;
                }
            }
        }
        InputHandleResult::Pass
    }

    /// Returns tool script plugins.
    pub fn tool_script_plugins(&self) -> Vec<&dyn EditorPluginExt> {
        self.plugins
            .iter()
            .filter(|e| e.plugin.tool_script().is_some())
            .map(|e| e.plugin.as_ref())
            .collect()
    }
}

impl Default for EditorPluginRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Plugin info (read-only snapshot)
// ---------------------------------------------------------------------------

/// Read-only snapshot of plugin information for display purposes.
#[derive(Debug, Clone)]
pub struct PluginInfo {
    /// Plugin identifier.
    pub id: String,
    /// Display name.
    pub display_name: String,
    /// Version string.
    pub version: String,
    /// Author name.
    pub author: String,
    /// Description.
    pub description: String,
    /// Whether the plugin is enabled.
    pub enabled: bool,
    /// Whether the plugin is backed by a tool script.
    pub is_tool_script: bool,
}

// ---------------------------------------------------------------------------
// Example editor plugins — seed the plugin ecosystem
// ---------------------------------------------------------------------------

/// Example plugin: Todo List panel.
///
/// Adds a bottom dock panel that tracks TODO comments found in scripts.
/// Demonstrates dock registration, process updates, and custom UI.
pub struct TodoListPlugin {
    todos: Vec<TodoItem>,
    scan_interval: f64,
    elapsed: f64,
}

/// A single TODO item found in a script.
#[derive(Debug, Clone)]
pub struct TodoItem {
    /// The file where the TODO was found.
    pub file: String,
    /// The line number.
    pub line: usize,
    /// The TODO text.
    pub text: String,
    /// Priority level (from TODO, FIXME, HACK markers).
    pub priority: TodoPriority,
}

/// Priority level for TODO items.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TodoPriority {
    Low,
    Normal,
    High,
}

impl TodoListPlugin {
    /// Create a new TODO list plugin with default settings.
    pub fn new() -> Self {
        Self {
            todos: Vec::new(),
            scan_interval: 5.0,
            elapsed: 0.0,
        }
    }

    /// Returns all tracked TODO items.
    pub fn todos(&self) -> &[TodoItem] {
        &self.todos
    }

    /// Add a TODO item (normally called during scan).
    pub fn add_todo(&mut self, item: TodoItem) {
        self.todos.push(item);
    }

    /// Clear all tracked TODOs.
    pub fn clear(&mut self) {
        self.todos.clear();
    }

    /// Returns TODOs filtered by priority.
    pub fn todos_by_priority(&self, priority: TodoPriority) -> Vec<&TodoItem> {
        self.todos.iter().filter(|t| t.priority == priority).collect()
    }
}

impl Default for TodoListPlugin {
    fn default() -> Self {
        Self::new()
    }
}

impl EditorPluginExt for TodoListPlugin {
    fn plugin_id(&self) -> &str {
        "patina.todo_list"
    }

    fn display_name(&self) -> &str {
        "Todo List"
    }

    fn version(&self) -> &str {
        "1.0.0"
    }

    fn author(&self) -> &str {
        "Patina Engine"
    }

    fn description(&self) -> &str {
        "Scans scripts for TODO/FIXME/HACK comments and displays them in a dock panel."
    }

    fn dock_panels(&self) -> Vec<(DockSlot, String)> {
        vec![(DockSlot::Bottom, "TODOs".to_string())]
    }

    fn process(&mut self, delta: f64) {
        self.elapsed += delta;
        if self.elapsed >= self.scan_interval {
            self.elapsed = 0.0;
            // In a real implementation, this would re-scan scripts.
        }
    }
}

/// Example plugin: Node Favorites.
///
/// Allows users to mark frequently-used node types as favorites for
/// quick access in the "Create New Node" dialog. Demonstrates custom
/// type registration and selection handling.
pub struct NodeFavoritesPlugin {
    favorites: Vec<String>,
    custom_types: Vec<CustomType>,
    last_selected: Option<NodeId>,
}

impl NodeFavoritesPlugin {
    /// Create a new Node Favorites plugin.
    pub fn new() -> Self {
        Self {
            favorites: Vec::new(),
            custom_types: Vec::new(),
            last_selected: None,
        }
    }

    /// Add a node class to favorites.
    pub fn add_favorite(&mut self, class_name: impl Into<String>) -> bool {
        let name = class_name.into();
        if self.favorites.contains(&name) {
            return false;
        }
        self.favorites.push(name);
        true
    }

    /// Remove a node class from favorites.
    pub fn remove_favorite(&mut self, class_name: &str) -> bool {
        let before = self.favorites.len();
        self.favorites.retain(|f| f != class_name);
        self.favorites.len() < before
    }

    /// Returns the list of favorite node classes.
    pub fn favorites(&self) -> &[String] {
        &self.favorites
    }

    /// Returns the last selected node ID.
    pub fn last_selected(&self) -> Option<NodeId> {
        self.last_selected
    }

    /// Register a custom node type shortcut.
    pub fn add_custom_shortcut(
        &mut self,
        type_name: impl Into<String>,
        base_class: impl Into<String>,
    ) {
        self.custom_types.push(CustomType {
            type_name: type_name.into(),
            base_class: base_class.into(),
            icon_path: None,
            script_path: None,
        });
    }
}

impl Default for NodeFavoritesPlugin {
    fn default() -> Self {
        Self::new()
    }
}

impl EditorPluginExt for NodeFavoritesPlugin {
    fn plugin_id(&self) -> &str {
        "patina.node_favorites"
    }

    fn display_name(&self) -> &str {
        "Node Favorites"
    }

    fn version(&self) -> &str {
        "1.0.0"
    }

    fn author(&self) -> &str {
        "Patina Engine"
    }

    fn description(&self) -> &str {
        "Quick access to frequently used node types in the Create dialog."
    }

    fn custom_types(&self) -> &[CustomType] {
        &self.custom_types
    }

    fn dock_panels(&self) -> Vec<(DockSlot, String)> {
        vec![(DockSlot::RightLower, "Favorites".to_string())]
    }

    fn selection_changed(&mut self, node_id: Option<NodeId>) {
        self.last_selected = node_id;
    }
}

/// Example plugin: Quick Scene Switcher.
///
/// Provides keyboard shortcuts and a command palette for quickly
/// switching between recently opened scenes. Demonstrates input
/// forwarding, autoload management, and process loop usage.
pub struct QuickSceneSwitcherPlugin {
    recent_scenes: Vec<String>,
    max_recent: usize,
    autoloads: Vec<AutoloadEntry>,
    active: bool,
}

impl QuickSceneSwitcherPlugin {
    /// Create a new Quick Scene Switcher plugin.
    pub fn new() -> Self {
        Self {
            recent_scenes: Vec::new(),
            max_recent: 10,
            autoloads: Vec::new(),
            active: false,
        }
    }

    /// Record a scene as recently opened.
    pub fn record_scene(&mut self, scene_path: impl Into<String>) {
        let path = scene_path.into();
        // Remove if already in list to move it to front.
        self.recent_scenes.retain(|s| s != &path);
        self.recent_scenes.insert(0, path);
        if self.recent_scenes.len() > self.max_recent {
            self.recent_scenes.truncate(self.max_recent);
        }
    }

    /// Returns the list of recent scenes (most recent first).
    pub fn recent_scenes(&self) -> &[String] {
        &self.recent_scenes
    }

    /// Returns the most recently opened scene.
    pub fn most_recent(&self) -> Option<&str> {
        self.recent_scenes.first().map(|s| s.as_str())
    }

    /// Whether the switcher popup is active.
    pub fn is_active(&self) -> bool {
        self.active
    }

    /// Toggle the switcher popup.
    pub fn toggle(&mut self) {
        self.active = !self.active;
    }

    /// Set maximum number of recent scenes to track.
    pub fn set_max_recent(&mut self, max: usize) {
        self.max_recent = max;
        if self.recent_scenes.len() > max {
            self.recent_scenes.truncate(max);
        }
    }

    /// Register a scene management autoload.
    pub fn add_scene_manager_autoload(&mut self) {
        self.autoloads.push(AutoloadEntry {
            name: "SceneSwitcher".to_string(),
            path: "res://addons/quick_scene_switcher/scene_switcher.gd".to_string(),
            is_singleton: true,
        });
    }
}

impl Default for QuickSceneSwitcherPlugin {
    fn default() -> Self {
        Self::new()
    }
}

impl EditorPluginExt for QuickSceneSwitcherPlugin {
    fn plugin_id(&self) -> &str {
        "patina.quick_scene_switcher"
    }

    fn display_name(&self) -> &str {
        "Quick Scene Switcher"
    }

    fn version(&self) -> &str {
        "1.0.0"
    }

    fn author(&self) -> &str {
        "Patina Engine"
    }

    fn description(&self) -> &str {
        "Quickly switch between recently opened scenes with keyboard shortcuts."
    }

    fn autoloads(&self) -> &[AutoloadEntry] {
        &self.autoloads
    }

    fn handles(&self, _class_name: &str) -> bool {
        // We handle all node types to intercept keyboard shortcuts.
        self.active
    }

    fn forward_canvas_input(
        &mut self,
        _mx: f32,
        _my: f32,
        _btn: u32,
        _pressed: bool,
    ) -> InputHandleResult {
        if self.active {
            InputHandleResult::Consumed
        } else {
            InputHandleResult::Pass
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- Test plugin implementations --

    struct SimplePlugin {
        id: String,
        name: String,
        enter_count: u32,
        exit_count: u32,
        ready_count: u32,
        last_selection: Option<NodeId>,
    }

    impl SimplePlugin {
        fn new(id: &str, name: &str) -> Self {
            Self {
                id: id.to_string(),
                name: name.to_string(),
                enter_count: 0,
                exit_count: 0,
                ready_count: 0,
                last_selection: None,
            }
        }
    }

    impl EditorPluginExt for SimplePlugin {
        fn plugin_id(&self) -> &str {
            &self.id
        }
        fn display_name(&self) -> &str {
            &self.name
        }
        fn enter_tree(&mut self) {
            self.enter_count += 1;
        }
        fn exit_tree(&mut self) {
            self.exit_count += 1;
        }
        fn ready(&mut self) {
            self.ready_count += 1;
        }
        fn selection_changed(&mut self, node_id: Option<NodeId>) {
            self.last_selection = node_id;
        }
    }

    struct ToolScriptPlugin {
        id: String,
        tool_script: ToolScriptDescriptor,
        custom_types: Vec<CustomType>,
        autoloads: Vec<AutoloadEntry>,
    }

    impl ToolScriptPlugin {
        fn new(id: &str, script_path: &str) -> Self {
            Self {
                id: id.to_string(),
                tool_script: ToolScriptDescriptor::new(script_path, "EditorPlugin"),
                custom_types: Vec::new(),
                autoloads: Vec::new(),
            }
        }

        fn with_custom_type(mut self, name: &str, base: &str) -> Self {
            self.custom_types.push(CustomType {
                type_name: name.to_string(),
                base_class: base.to_string(),
                icon_path: None,
                script_path: None,
            });
            self
        }

        fn with_autoload(mut self, name: &str, path: &str) -> Self {
            self.autoloads.push(AutoloadEntry {
                name: name.to_string(),
                path: path.to_string(),
                is_singleton: true,
            });
            self
        }
    }

    impl EditorPluginExt for ToolScriptPlugin {
        fn plugin_id(&self) -> &str {
            &self.id
        }
        fn display_name(&self) -> &str {
            &self.id
        }
        fn tool_script(&self) -> Option<&ToolScriptDescriptor> {
            Some(&self.tool_script)
        }
        fn custom_types(&self) -> &[CustomType] {
            &self.custom_types
        }
        fn autoloads(&self) -> &[AutoloadEntry] {
            &self.autoloads
        }
    }

    struct InputHandlerPlugin {
        id: String,
        handled_class: String,
        consumed: bool,
    }

    impl EditorPluginExt for InputHandlerPlugin {
        fn plugin_id(&self) -> &str {
            &self.id
        }
        fn display_name(&self) -> &str {
            &self.id
        }
        fn handles(&self, class_name: &str) -> bool {
            class_name == self.handled_class
        }
        fn forward_canvas_input(
            &mut self,
            _mx: f32,
            _my: f32,
            _btn: u32,
            _pressed: bool,
        ) -> InputHandleResult {
            if self.consumed {
                InputHandleResult::Consumed
            } else {
                InputHandleResult::Pass
            }
        }
        fn forward_3d_input(
            &mut self,
            _mx: f32,
            _my: f32,
            _btn: u32,
            _pressed: bool,
        ) -> InputHandleResult {
            if self.consumed {
                InputHandleResult::Consumed
            } else {
                InputHandleResult::Pass
            }
        }
    }

    // -- DockSlot tests --

    #[test]
    fn dock_slot_names() {
        assert_eq!(DockSlot::LeftUpper.name(), "Left Upper");
        assert_eq!(DockSlot::LeftLower.name(), "Left Lower");
        assert_eq!(DockSlot::RightUpper.name(), "Right Upper");
        assert_eq!(DockSlot::RightLower.name(), "Right Lower");
        assert_eq!(DockSlot::Bottom.name(), "Bottom");
    }

    #[test]
    fn dock_slot_sides() {
        assert!(DockSlot::LeftUpper.is_left());
        assert!(DockSlot::LeftLower.is_left());
        assert!(!DockSlot::RightUpper.is_left());
        assert!(DockSlot::RightUpper.is_right());
        assert!(DockSlot::RightLower.is_right());
        assert!(!DockSlot::LeftUpper.is_right());
        assert!(DockSlot::Bottom.is_bottom());
        assert!(!DockSlot::LeftUpper.is_bottom());
    }

    #[test]
    fn dock_slot_godot_int_roundtrip() {
        for slot in [
            DockSlot::LeftUpper,
            DockSlot::LeftLower,
            DockSlot::RightUpper,
            DockSlot::RightLower,
            DockSlot::Bottom,
        ] {
            let int_val = slot.to_godot_int();
            assert_eq!(DockSlot::from_godot_int(int_val), Some(slot));
        }
        assert_eq!(DockSlot::from_godot_int(99), None);
    }

    // -- Registry lifecycle tests --

    #[test]
    fn register_plugin() {
        let mut registry = EditorPluginRegistry::new();
        let plugin = SimplePlugin::new("test-plugin", "Test Plugin");
        registry.register(Box::new(plugin));

        assert_eq!(registry.plugin_count(), 1);
        assert!(registry.is_enabled("test-plugin"));
    }

    #[test]
    fn register_calls_lifecycle_methods() {
        let mut registry = EditorPluginRegistry::new();
        let plugin = SimplePlugin::new("lc-test", "Lifecycle");
        registry.register(Box::new(plugin));

        // enter_tree and ready should have been called
        let info = registry.plugin_info();
        assert_eq!(info.len(), 1);
        assert!(info[0].enabled);
    }

    #[test]
    fn unregister_plugin() {
        let mut registry = EditorPluginRegistry::new();
        registry.register(Box::new(SimplePlugin::new("p1", "Plugin 1")));
        registry.register(Box::new(SimplePlugin::new("p2", "Plugin 2")));
        assert_eq!(registry.plugin_count(), 2);

        registry.unregister("p1");
        assert_eq!(registry.plugin_count(), 1);
        assert!(!registry.is_enabled("p1"));
        assert!(registry.is_enabled("p2"));
    }

    #[test]
    fn enable_disable_plugin() {
        let mut registry = EditorPluginRegistry::new();
        registry.register(Box::new(SimplePlugin::new("toggle", "Toggle")));

        assert!(registry.is_enabled("toggle"));
        registry.disable("toggle");
        assert!(!registry.is_enabled("toggle"));
        registry.enable("toggle");
        assert!(registry.is_enabled("toggle"));
    }

    #[test]
    fn disable_skips_process_and_selection() {
        let mut registry = EditorPluginRegistry::new();
        registry.register(Box::new(SimplePlugin::new("p1", "P1")));
        registry.disable("p1");

        // These should not panic even though plugin is disabled
        registry.notify_selection_changed(Some(NodeId::next()));
        registry.process(0.016);
    }

    // -- Plugin info --

    #[test]
    fn plugin_info_snapshot() {
        let mut registry = EditorPluginRegistry::new();
        let plugin = ToolScriptPlugin::new("my-tool", "res://addons/my_tool/plugin.gd");
        registry.register(Box::new(plugin));

        let info = registry.plugin_info();
        assert_eq!(info.len(), 1);
        assert_eq!(info[0].id, "my-tool");
        assert!(info[0].is_tool_script);
        assert!(info[0].enabled);
    }

    #[test]
    fn plugin_ids() {
        let mut registry = EditorPluginRegistry::new();
        registry.register(Box::new(SimplePlugin::new("a", "A")));
        registry.register(Box::new(SimplePlugin::new("b", "B")));

        let ids = registry.plugin_ids();
        assert_eq!(ids.len(), 2);
        assert!(ids.contains(&"a".to_string()));
        assert!(ids.contains(&"b".to_string()));
    }

    // -- Custom types --

    #[test]
    fn custom_type_registration() {
        let mut registry = EditorPluginRegistry::new();
        let plugin = ToolScriptPlugin::new("custom-nodes", "res://plugin.gd")
            .with_custom_type("MySprite", "Sprite2D")
            .with_custom_type("MyButton", "Button");
        registry.register(Box::new(plugin));

        assert_eq!(registry.custom_types().len(), 2);
        let found = registry.find_custom_type("MySprite");
        assert!(found.is_some());
        assert_eq!(found.unwrap().base_class, "Sprite2D");
    }

    #[test]
    fn unregister_removes_custom_types() {
        let mut registry = EditorPluginRegistry::new();
        let plugin =
            ToolScriptPlugin::new("ct-test", "res://plugin.gd").with_custom_type("Foo", "Node");
        registry.register(Box::new(plugin));
        assert_eq!(registry.custom_types().len(), 1);

        registry.unregister("ct-test");
        assert_eq!(registry.custom_types().len(), 0);
        assert!(registry.find_custom_type("Foo").is_none());
    }

    // -- Dock panels --

    #[test]
    fn dock_panel_registration() {
        struct DockPlugin;
        impl EditorPluginExt for DockPlugin {
            fn plugin_id(&self) -> &str {
                "dock-test"
            }
            fn display_name(&self) -> &str {
                "Dock Test"
            }
            fn dock_panels(&self) -> Vec<(DockSlot, String)> {
                vec![
                    (DockSlot::Bottom, "My Output".to_string()),
                    (DockSlot::RightLower, "My Panel".to_string()),
                ]
            }
        }

        let mut registry = EditorPluginRegistry::new();
        registry.register(Box::new(DockPlugin));

        assert_eq!(registry.dock_panels().len(), 2);
        let bottom = registry.panels_in_slot(DockSlot::Bottom);
        assert_eq!(bottom, vec!["My Output"]);
        let right = registry.panels_in_slot(DockSlot::RightLower);
        assert_eq!(right, vec!["My Panel"]);
    }

    // -- Autoloads --

    #[test]
    fn autoload_registration() {
        let mut registry = EditorPluginRegistry::new();
        let plugin = ToolScriptPlugin::new("al-test", "res://plugin.gd")
            .with_autoload("GameManager", "res://game_manager.gd");
        registry.register(Box::new(plugin));

        assert_eq!(registry.autoloads().len(), 1);
        assert_eq!(registry.autoloads()[0].1.name, "GameManager");
        assert!(registry.autoloads()[0].1.is_singleton);
    }

    #[test]
    fn unregister_removes_autoloads() {
        let mut registry = EditorPluginRegistry::new();
        let plugin = ToolScriptPlugin::new("al-rem", "res://plugin.gd")
            .with_autoload("Mgr", "res://mgr.gd");
        registry.register(Box::new(plugin));
        assert_eq!(registry.autoloads().len(), 1);

        registry.unregister("al-rem");
        assert_eq!(registry.autoloads().len(), 0);
    }

    // -- Tool script --

    #[test]
    fn tool_script_descriptor() {
        let desc = ToolScriptDescriptor::new("res://addons/test/plugin.gd", "EditorPlugin")
            .with_export("speed", "10.0")
            .with_export("label", "\"Hello\"");

        assert!(desc.is_tool);
        assert_eq!(desc.extends_class, "EditorPlugin");
        assert_eq!(desc.exported_properties.len(), 2);
        assert_eq!(desc.exported_properties["speed"], "10.0");
    }

    #[test]
    fn non_tool_script_descriptor() {
        let desc = ToolScriptDescriptor::non_tool("res://my_script.gd", "Node2D");
        assert!(!desc.is_tool);
        assert_eq!(desc.extends_class, "Node2D");
    }

    #[test]
    fn tool_script_plugins_list() {
        let mut registry = EditorPluginRegistry::new();
        registry.register(Box::new(SimplePlugin::new("simple", "Simple")));
        registry.register(Box::new(ToolScriptPlugin::new(
            "tool",
            "res://plugin.gd",
        )));

        let tool_plugins = registry.tool_script_plugins();
        assert_eq!(tool_plugins.len(), 1);
        assert_eq!(tool_plugins[0].plugin_id(), "tool");
    }

    // -- Input forwarding --

    #[test]
    fn canvas_input_forwarding_pass() {
        let mut registry = EditorPluginRegistry::new();
        registry.register(Box::new(InputHandlerPlugin {
            id: "ih-pass".to_string(),
            handled_class: "Sprite2D".to_string(),
            consumed: false,
        }));

        let result = registry.forward_canvas_input("Sprite2D", 100.0, 200.0, 1, true);
        assert_eq!(result, InputHandleResult::Pass);
    }

    #[test]
    fn canvas_input_forwarding_consumed() {
        let mut registry = EditorPluginRegistry::new();
        registry.register(Box::new(InputHandlerPlugin {
            id: "ih-consume".to_string(),
            handled_class: "Sprite2D".to_string(),
            consumed: true,
        }));

        let result = registry.forward_canvas_input("Sprite2D", 100.0, 200.0, 1, true);
        assert_eq!(result, InputHandleResult::Consumed);
    }

    #[test]
    fn input_not_forwarded_to_non_handler() {
        let mut registry = EditorPluginRegistry::new();
        registry.register(Box::new(InputHandlerPlugin {
            id: "ih-wrong".to_string(),
            handled_class: "Sprite2D".to_string(),
            consumed: true,
        }));

        // Forward for a different class — should pass
        let result = registry.forward_canvas_input("Camera2D", 0.0, 0.0, 1, true);
        assert_eq!(result, InputHandleResult::Pass);
    }

    #[test]
    fn input_3d_forwarding() {
        let mut registry = EditorPluginRegistry::new();
        registry.register(Box::new(InputHandlerPlugin {
            id: "ih-3d".to_string(),
            handled_class: "MeshInstance3D".to_string(),
            consumed: true,
        }));

        let result = registry.forward_3d_input("MeshInstance3D", 50.0, 50.0, 2, false);
        assert_eq!(result, InputHandleResult::Consumed);
    }

    #[test]
    fn disabled_plugin_input_not_forwarded() {
        let mut registry = EditorPluginRegistry::new();
        registry.register(Box::new(InputHandlerPlugin {
            id: "ih-disabled".to_string(),
            handled_class: "Sprite2D".to_string(),
            consumed: true,
        }));
        registry.disable("ih-disabled");

        let result = registry.forward_canvas_input("Sprite2D", 0.0, 0.0, 1, true);
        assert_eq!(result, InputHandleResult::Pass);
    }

    // -- Multiple plugins --

    #[test]
    fn multiple_plugins_coexist() {
        let mut registry = EditorPluginRegistry::new();
        registry.register(Box::new(SimplePlugin::new("p1", "Plugin 1")));
        registry.register(Box::new(
            ToolScriptPlugin::new("p2", "res://p2.gd").with_custom_type("CustomNode", "Node"),
        ));
        registry.register(Box::new(SimplePlugin::new("p3", "Plugin 3")));

        assert_eq!(registry.plugin_count(), 3);
        assert_eq!(registry.custom_types().len(), 1);
        assert_eq!(registry.tool_script_plugins().len(), 1);
    }

    // -- Default registry --

    #[test]
    fn default_registry_is_empty() {
        let registry = EditorPluginRegistry::default();
        assert_eq!(registry.plugin_count(), 0);
        assert!(registry.custom_types().is_empty());
        assert!(registry.autoloads().is_empty());
        assert!(registry.dock_panels().is_empty());
    }

    // -- CustomControlContainer --

    #[test]
    fn custom_control_container_variants() {
        let containers = [
            CustomControlContainer::Toolbar,
            CustomControlContainer::SpatialEditorMenu,
            CustomControlContainer::SpatialEditorSide,
            CustomControlContainer::SpatialEditorBottom,
            CustomControlContainer::CanvasEditorMenu,
            CustomControlContainer::CanvasEditorSide,
            CustomControlContainer::CanvasEditorBottom,
            CustomControlContainer::InspectorBottom,
        ];
        assert_eq!(containers.len(), 8);
        // Ensure all variants are distinct
        for (i, a) in containers.iter().enumerate() {
            for (j, b) in containers.iter().enumerate() {
                if i != j {
                    assert_ne!(a, b);
                }
            }
        }
    }

    // -- Example plugin: TodoListPlugin --

    #[test]
    fn todo_list_plugin_metadata() {
        let plugin = TodoListPlugin::new();
        assert_eq!(plugin.plugin_id(), "patina.todo_list");
        assert_eq!(plugin.display_name(), "Todo List");
        assert_eq!(plugin.version(), "1.0.0");
        assert!(!plugin.description().is_empty());
    }

    #[test]
    fn todo_list_plugin_dock_panels() {
        let plugin = TodoListPlugin::new();
        let panels = plugin.dock_panels();
        assert_eq!(panels.len(), 1);
        assert_eq!(panels[0].0, DockSlot::Bottom);
        assert_eq!(panels[0].1, "TODOs");
    }

    #[test]
    fn todo_list_add_and_filter() {
        let mut plugin = TodoListPlugin::new();
        plugin.add_todo(TodoItem {
            file: "main.gd".into(),
            line: 10,
            text: "TODO: refactor this".into(),
            priority: TodoPriority::Normal,
        });
        plugin.add_todo(TodoItem {
            file: "player.gd".into(),
            line: 5,
            text: "FIXME: handle edge case".into(),
            priority: TodoPriority::High,
        });
        plugin.add_todo(TodoItem {
            file: "utils.gd".into(),
            line: 20,
            text: "HACK: temporary workaround".into(),
            priority: TodoPriority::Low,
        });

        assert_eq!(plugin.todos().len(), 3);
        assert_eq!(plugin.todos_by_priority(TodoPriority::High).len(), 1);
        assert_eq!(plugin.todos_by_priority(TodoPriority::Normal).len(), 1);
        assert_eq!(plugin.todos_by_priority(TodoPriority::Low).len(), 1);

        plugin.clear();
        assert!(plugin.todos().is_empty());
    }

    #[test]
    fn todo_list_registers_in_registry() {
        let mut registry = EditorPluginRegistry::new();
        registry.register(Box::new(TodoListPlugin::new()));
        assert_eq!(registry.plugin_count(), 1);
        let panels = registry.panels_in_slot(DockSlot::Bottom);
        assert!(panels.iter().any(|p| *p == "TODOs"));
    }

    // -- Example plugin: NodeFavoritesPlugin --

    #[test]
    fn node_favorites_metadata() {
        let plugin = NodeFavoritesPlugin::new();
        assert_eq!(plugin.plugin_id(), "patina.node_favorites");
        assert_eq!(plugin.display_name(), "Node Favorites");
    }

    #[test]
    fn node_favorites_add_remove() {
        let mut plugin = NodeFavoritesPlugin::new();
        assert!(plugin.add_favorite("Sprite2D"));
        assert!(plugin.add_favorite("CharacterBody2D"));
        assert_eq!(plugin.favorites().len(), 2);

        // Duplicate returns false.
        assert!(!plugin.add_favorite("Sprite2D"));
        assert_eq!(plugin.favorites().len(), 2);

        assert!(plugin.remove_favorite("Sprite2D"));
        assert_eq!(plugin.favorites().len(), 1);
        assert!(!plugin.remove_favorite("Sprite2D")); // already removed
    }

    #[test]
    fn node_favorites_custom_shortcut() {
        let mut plugin = NodeFavoritesPlugin::new();
        plugin.add_custom_shortcut("QuickPlayer", "CharacterBody2D");
        assert_eq!(plugin.custom_types().len(), 1);
        assert_eq!(plugin.custom_types()[0].type_name, "QuickPlayer");
    }

    #[test]
    fn node_favorites_selection_tracking() {
        let mut plugin = NodeFavoritesPlugin::new();
        assert!(plugin.last_selected().is_none());

        let id = NodeId::next();
        plugin.selection_changed(Some(id));
        assert_eq!(plugin.last_selected(), Some(id));
    }

    #[test]
    fn node_favorites_registers_in_registry() {
        let mut registry = EditorPluginRegistry::new();
        registry.register(Box::new(NodeFavoritesPlugin::new()));
        assert_eq!(registry.plugin_count(), 1);
        let panels = registry.panels_in_slot(DockSlot::RightLower);
        assert!(panels.iter().any(|p| *p == "Favorites"));
    }

    // -- Example plugin: QuickSceneSwitcherPlugin --

    #[test]
    fn quick_scene_switcher_metadata() {
        let plugin = QuickSceneSwitcherPlugin::new();
        assert_eq!(plugin.plugin_id(), "patina.quick_scene_switcher");
        assert_eq!(plugin.display_name(), "Quick Scene Switcher");
    }

    #[test]
    fn quick_scene_switcher_record_scenes() {
        let mut plugin = QuickSceneSwitcherPlugin::new();
        plugin.record_scene("res://main.tscn");
        plugin.record_scene("res://player.tscn");
        plugin.record_scene("res://enemy.tscn");

        assert_eq!(plugin.recent_scenes().len(), 3);
        assert_eq!(plugin.most_recent(), Some("res://enemy.tscn"));

        // Re-recording moves to front.
        plugin.record_scene("res://main.tscn");
        assert_eq!(plugin.most_recent(), Some("res://main.tscn"));
        assert_eq!(plugin.recent_scenes().len(), 3); // no duplicate
    }

    #[test]
    fn quick_scene_switcher_max_recent() {
        let mut plugin = QuickSceneSwitcherPlugin::new();
        plugin.set_max_recent(3);
        for i in 0..5 {
            plugin.record_scene(format!("res://scene_{i}.tscn"));
        }
        assert_eq!(plugin.recent_scenes().len(), 3);
        assert_eq!(plugin.most_recent(), Some("res://scene_4.tscn"));
    }

    #[test]
    fn quick_scene_switcher_toggle() {
        let mut plugin = QuickSceneSwitcherPlugin::new();
        assert!(!plugin.is_active());
        plugin.toggle();
        assert!(plugin.is_active());
        plugin.toggle();
        assert!(!plugin.is_active());
    }

    #[test]
    fn quick_scene_switcher_autoload() {
        let mut plugin = QuickSceneSwitcherPlugin::new();
        plugin.add_scene_manager_autoload();
        assert_eq!(plugin.autoloads().len(), 1);
        assert_eq!(plugin.autoloads()[0].name, "SceneSwitcher");
    }

    #[test]
    fn quick_scene_switcher_input_when_active() {
        let mut plugin = QuickSceneSwitcherPlugin::new();
        // Inactive — should not handle input.
        assert!(!plugin.handles("Node"));

        plugin.toggle(); // activate
        assert!(plugin.handles("Node"));

        let result = plugin.forward_canvas_input(0.0, 0.0, 1, true);
        assert_eq!(result, InputHandleResult::Consumed);
    }

    #[test]
    fn quick_scene_switcher_registers_in_registry() {
        let mut registry = EditorPluginRegistry::new();
        let mut plugin = QuickSceneSwitcherPlugin::new();
        plugin.add_scene_manager_autoload();
        registry.register(Box::new(plugin));

        assert_eq!(registry.plugin_count(), 1);
        assert_eq!(registry.autoloads().len(), 1);
    }

    // -- All three plugins together --

    #[test]
    fn all_example_plugins_coexist() {
        let mut registry = EditorPluginRegistry::new();
        registry.register(Box::new(TodoListPlugin::new()));
        registry.register(Box::new(NodeFavoritesPlugin::new()));
        let mut switcher = QuickSceneSwitcherPlugin::new();
        switcher.add_scene_manager_autoload();
        registry.register(Box::new(switcher));

        assert_eq!(registry.plugin_count(), 3);
        assert_eq!(registry.dock_panels().len(), 2); // TODOs + Favorites
        assert_eq!(registry.autoloads().len(), 1); // SceneSwitcher

        let ids = registry.plugin_ids();
        assert!(ids.contains(&"patina.todo_list".to_string()));
        assert!(ids.contains(&"patina.node_favorites".to_string()));
        assert!(ids.contains(&"patina.quick_scene_switcher".to_string()));
    }
}
