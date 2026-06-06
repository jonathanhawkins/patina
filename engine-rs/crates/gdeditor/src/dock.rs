//! Editor dock panels and layout management.
//!
//! Provides the [`DockPanel`] trait and concrete implementations for
//! the scene tree dock and property dock, mirroring Godot's editor
//! layout panels.
//!
//! The [`PluginDockManager`] manages dock panels registered by editor
//! plugins, allowing dynamic dock creation and removal at runtime.

use std::collections::HashMap;

use gdscene::node::NodeId;
use gdscene::SceneTree;

use crate::editor_plugin::DockSlot;
use crate::inspector::InspectorPanel;

/// A named dock panel in the editor UI.
///
/// Each dock has a title and can refresh its contents from the scene tree.
pub trait DockPanel {
    /// Returns the display title of this dock.
    fn title(&self) -> &str;

    /// Refreshes the dock's internal state from the current scene tree.
    fn refresh(&mut self, tree: &SceneTree);
}

/// Icon identifier for a node type in the scene tree.
///
/// Maps Godot class names to icon identifiers used by the editor theme.
/// In Godot 4.x, each node class has an associated icon shown in the
/// scene tree dock.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeTypeIcon {
    /// The icon identifier (e.g. "Node2D", "Sprite2D", "MeshInstance3D").
    /// Matches the class name for built-in types.
    pub icon_name: String,
    /// Color tint category for the icon.
    pub color_category: IconColorCategory,
}

/// Color categories for scene tree node icons, matching Godot 4.x conventions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum IconColorCategory {
    /// Generic node (white/gray).
    #[default]
    Default,
    /// 2D nodes (blue).
    Node2D,
    /// 3D nodes (red/orange).
    Node3D,
    /// Control/UI nodes (green).
    Control,
    /// Resource nodes (purple).
    Resource,
    /// Signal-related indicators (yellow).
    Signal,
}

/// Indicators displayed alongside a node in the scene tree.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NodeIndicators {
    /// Whether this node has an attached script.
    pub has_script: bool,
    /// Path to the attached script, if any.
    pub script_path: Option<String>,
    /// Whether this node has signal connections.
    pub has_signals: bool,
    /// Number of connected signals.
    pub signal_count: usize,
    /// Whether this node belongs to any groups.
    pub has_groups: bool,
    /// Warnings for this node (e.g. missing required children, invalid config).
    pub warnings: Vec<NodeWarning>,
    /// Whether the node is visible (not hidden).
    pub visible: bool,
    /// Whether this node has unique name access (%Name).
    pub is_unique_name: bool,
    /// Whether this node's unique name collides with another unique name in
    /// the same owner scope (ambiguous `%Name` resolution).
    pub unique_name_collision: bool,
    /// Whether the node is locked (cannot be selected in viewport).
    pub locked: bool,
    /// Whether this node is the root of an instanced scene.
    pub is_instance: bool,
    /// Source scene path for instanced nodes.
    pub instance_source: Option<String>,
}

impl NodeIndicators {
    /// Whether a configuration-warning triangle should be shown on this row —
    /// true when the node has any configuration warnings.
    pub fn has_warning_triangle(&self) -> bool {
        !self.warnings.is_empty()
    }

    /// The severity of the configuration-warning triangle on this row: the
    /// most severe warning attached to the node (`Info < Warning < Error`), or
    /// `None` when the node has no warnings (no triangle). Mirrors Godot's
    /// scene-tree warning icon, which reflects the worst warning.
    pub fn warning_triangle(&self) -> Option<WarningSeverity> {
        self.warnings.iter().map(|w| w.severity).max()
    }

    /// The tooltip text for this row's warning triangle: every warning message
    /// joined by newlines (empty when there are no warnings), matching Godot's
    /// triangle tooltip that lists all configuration warnings.
    pub fn warning_tooltip(&self) -> String {
        self.warnings
            .iter()
            .map(|w| w.message.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Whether this row shows the "has attached script" badge.
    pub fn has_script_badge(&self) -> bool {
        self.has_script
    }

    /// The attached script's path, shown in the script badge's tooltip
    /// (`None` when the node has no script and thus no badge).
    pub fn script_badge_path(&self) -> Option<&str> {
        self.script_path.as_deref()
    }

    /// Whether this row shows the "scene instance" badge — true when the node
    /// is the root of an instanced scene.
    pub fn has_instance_badge(&self) -> bool {
        self.is_instance
    }

    /// The source scene path shown in the instance badge's tooltip (`None` when
    /// the node is not an instanced-scene root and thus has no badge).
    pub fn instance_badge_source(&self) -> Option<&str> {
        self.instance_source.as_deref()
    }

    /// Whether this row shows the signal-connection indicator (the node has at
    /// least one connected signal).
    pub fn has_signal_indicator(&self) -> bool {
        self.has_signals
    }

    /// The number of connected signals shown beside the signal indicator.
    pub fn signal_indicator_count(&self) -> usize {
        self.signal_count
    }

    /// Whether this row shows the unique-name (`%`) access indicator — true when
    /// the node is accessible as `%Name` in its owner scope.
    pub fn has_unique_name_indicator(&self) -> bool {
        self.is_unique_name
    }
}

/// A warning attached to a scene tree node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeWarning {
    /// Warning severity.
    pub severity: WarningSeverity,
    /// Human-readable warning message.
    pub message: String,
}

/// Severity level for node warnings.
///
/// Ordered by escalation (`Info < Warning < Error`), so the most severe warning
/// on a node — the one the row's warning triangle reflects — is the `max`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum WarningSeverity {
    /// Informational (blue info icon).
    Info,
    /// Something might be wrong (yellow warning icon).
    Warning,
    /// Definitely wrong (red error icon).
    Error,
}

/// An entry in the scene tree dock, representing a node in the hierarchy.
#[derive(Debug, Clone)]
pub struct SceneTreeEntry {
    /// The node's ID.
    pub id: NodeId,
    /// The node's display name.
    pub name: String,
    /// The node's class name.
    pub class_name: String,
    /// The absolute path of this node.
    pub path: String,
    /// Indentation depth (0 for root).
    pub depth: usize,
    /// Type icon for this node.
    pub icon: NodeTypeIcon,
    /// Status indicators (script, signals, warnings, etc.).
    pub indicators: NodeIndicators,
}

/// Resolve the icon for a given class name, following Godot 4.x conventions.
pub fn resolve_node_icon(class_name: &str) -> NodeTypeIcon {
    let color_category = classify_node_color(class_name);
    NodeTypeIcon {
        icon_name: class_name.to_string(),
        color_category,
    }
}

/// Classify a node class name into an icon color category.
fn classify_node_color(class_name: &str) -> IconColorCategory {
    // 3D nodes
    if class_name.ends_with("3D")
        || class_name.starts_with("Mesh")
        || class_name.starts_with("Light")
        || class_name.starts_with("Camera3D")
        || class_name.starts_with("Skeleton")
        || class_name == "WorldEnvironment"
        || class_name.starts_with("RigidBody")
        || class_name.starts_with("CharacterBody3D")
        || class_name.starts_with("StaticBody3D")
        || class_name.starts_with("CollisionShape3D")
    {
        return IconColorCategory::Node3D;
    }

    // 2D nodes
    if class_name.ends_with("2D")
        || class_name.starts_with("Sprite")
        || class_name.starts_with("TileMap")
        || class_name.starts_with("Camera2D")
        || class_name.starts_with("CharacterBody2D")
        || class_name.starts_with("StaticBody2D")
        || class_name.starts_with("RigidBody2D")
        || class_name.starts_with("CollisionShape2D")
    {
        return IconColorCategory::Node2D;
    }

    // Control/UI nodes
    if class_name.starts_with("Control")
        || class_name.starts_with("Button")
        || class_name.starts_with("Label")
        || class_name.starts_with("Panel")
        || class_name.starts_with("Container")
        || class_name.starts_with("TextEdit")
        || class_name.starts_with("LineEdit")
        || class_name.starts_with("Tree")
        || class_name.starts_with("ItemList")
        || class_name.starts_with("ScrollContainer")
        || class_name.starts_with("HBox")
        || class_name.starts_with("VBox")
        || class_name.starts_with("MarginContainer")
        || class_name.starts_with("ColorRect")
        || class_name.starts_with("TextureRect")
        || class_name == "RichTextLabel"
        || class_name == "ProgressBar"
        || class_name == "SpinBox"
        || class_name == "CheckBox"
        || class_name == "CheckButton"
        || class_name == "OptionButton"
    {
        return IconColorCategory::Control;
    }

    IconColorCategory::Default
}

/// Generate configuration warnings for a node based on its type and children.
pub fn compute_node_warnings(tree: &SceneTree, node_id: NodeId) -> Vec<NodeWarning> {
    let mut warnings = Vec::new();
    let node = match tree.get_node(node_id) {
        Some(n) => n,
        None => return warnings,
    };

    let class = node.class_name();
    let children: Vec<NodeId> = node.children().to_vec();

    // CollisionShape2D/3D without a parent body
    if class == "CollisionShape2D" || class == "CollisionShape3D" {
        if let Some(parent_id) = node.parent() {
            if let Some(parent) = tree.get_node(parent_id) {
                let pc = parent.class_name();
                let is_body = pc.contains("Body") || pc.contains("Area");
                if !is_body {
                    warnings.push(NodeWarning {
                        severity: WarningSeverity::Warning,
                        message: format!("{class} must be a child of a physics body or area node."),
                    });
                }
            }
        }
    }

    // RigidBody/CharacterBody/StaticBody without CollisionShape child
    if class.contains("Body2D") || class.contains("Body3D") || class.starts_with("Area") {
        let has_shape = children.iter().any(|&cid| {
            tree.get_node(cid)
                .map(|n| n.class_name().starts_with("CollisionShape"))
                .unwrap_or(false)
        });
        if !has_shape {
            warnings.push(NodeWarning {
                severity: WarningSeverity::Warning,
                message: format!("{class} has no CollisionShape child — collisions will not work."),
            });
        }
    }

    // Sprite2D/Sprite3D without texture (check property)
    if class == "Sprite2D" || class == "Sprite3D" {
        let has_texture = !matches!(node.get_property("texture"), gdvariant::Variant::Nil);
        if !has_texture {
            warnings.push(NodeWarning {
                severity: WarningSeverity::Info,
                message: "Sprite has no texture assigned.".into(),
            });
        }
    }

    // Unique-name-in-owner (`%`) collision: another node in the same owner
    // scope is also flagged "Access as Unique Name" with the same name, so
    // `%Name` resolution is ambiguous. Surface it as a warning on the row.
    if tree.unique_name_collision(node_id) {
        warnings.push(NodeWarning {
            severity: WarningSeverity::Warning,
            message: format!(
                "Another node in this scene already uses the unique name \"%{}\".",
                node.name()
            ),
        });
    }

    warnings
}

/// Whether a node is currently visible (its `visible` property is not
/// explicitly `false`) — the source of truth for the row's visibility toggle.
fn node_visible(tree: &SceneTree, id: NodeId) -> bool {
    tree.get_node(id)
        .map(|n| !matches!(n.get_property("visible"), gdvariant::Variant::Bool(false)))
        .unwrap_or(true)
}

/// Whether a node is edit-locked (the Godot-canonical `_edit_lock_` meta or the
/// legacy `_locked` flag) — the source of truth for the row's lock toggle.
fn node_locked(tree: &SceneTree, id: NodeId) -> bool {
    tree.get_node(id)
        .map(|n| {
            matches!(n.get_property("_edit_lock_"), gdvariant::Variant::Bool(true))
                || matches!(n.get_property("_locked"), gdvariant::Variant::Bool(true))
        })
        .unwrap_or(false)
}

/// Toggles the node's visibility (the `visible` property) and returns the new
/// visibility. Mirrors clicking the eye toggle on a scene-tree row — the change
/// is reflected in [`NodeIndicators::visible`] after the dock refreshes.
pub fn toggle_node_visibility(tree: &mut SceneTree, id: NodeId) -> bool {
    let now = !node_visible(tree, id);
    if let Some(node) = tree.get_node_mut(id) {
        node.set_property("visible", gdvariant::Variant::Bool(now));
    }
    now
}

/// Toggles the node's edit-lock (the `_edit_lock_` meta) and returns the new
/// lock state. Mirrors clicking the lock toggle on a scene-tree row — reflected
/// in [`NodeIndicators::locked`] after the dock refreshes.
pub fn toggle_node_lock(tree: &mut SceneTree, id: NodeId) -> bool {
    let now = !node_locked(tree, id);
    if let Some(node) = tree.get_node_mut(id) {
        node.set_property("_edit_lock_", gdvariant::Variant::Bool(now));
    }
    now
}

/// Toggles the node's membership in `group` and returns whether the node is now
/// in the group. Mirrors toggling a group on a scene-tree row — reflected in
/// [`NodeIndicators::has_groups`] after the dock refreshes.
pub fn toggle_node_group(tree: &mut SceneTree, id: NodeId, group: &str) -> bool {
    match tree.get_node_mut(id) {
        Some(node) if node.is_in_group(group) => {
            node.remove_from_group(group);
            false
        }
        Some(node) => {
            node.add_to_group(group);
            true
        }
        None => false,
    }
}

/// A dock panel showing the scene tree node hierarchy.
///
/// Displays nodes as a flat list with indentation to convey depth,
/// similar to Godot's Scene dock.
#[derive(Debug)]
pub struct SceneTreeDock {
    /// Flattened tree entries, in depth-first order.
    entries: Vec<SceneTreeEntry>,
    /// Script paths associated with nodes (NodeId → script path).
    script_paths: HashMap<NodeId, String>,
    /// Signal connection counts per node (from scene connection data).
    signal_counts: HashMap<NodeId, usize>,
    /// Currently selected node IDs (supports multi-select).
    selected_nodes: Vec<NodeId>,
}

impl SceneTreeDock {
    /// Creates an empty scene tree dock.
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            script_paths: HashMap::new(),
            signal_counts: HashMap::new(),
            selected_nodes: Vec::new(),
        }
    }

    /// Returns the current list of tree entries.
    pub fn entries(&self) -> &[SceneTreeEntry] {
        &self.entries
    }

    /// Finds an entry by node ID.
    pub fn find_entry(&self, id: NodeId) -> Option<&SceneTreeEntry> {
        self.entries.iter().find(|e| e.id == id)
    }

    /// Associate a script path with a node.
    pub fn set_node_script(&mut self, id: NodeId, script_path: impl Into<String>) {
        self.script_paths.insert(id, script_path.into());
    }

    /// Remove a script association from a node.
    pub fn remove_node_script(&mut self, id: NodeId) {
        self.script_paths.remove(&id);
    }

    /// Get the script path for a node, if any.
    pub fn node_script(&self, id: NodeId) -> Option<&str> {
        self.script_paths.get(&id).map(|s| s.as_str())
    }

    /// Set the signal connection count for a node.
    pub fn set_node_signal_count(&mut self, id: NodeId, count: usize) {
        if count > 0 {
            self.signal_counts.insert(id, count);
        } else {
            self.signal_counts.remove(&id);
        }
    }

    /// Get the signal connection count for a node.
    pub fn node_signal_count(&self, id: NodeId) -> usize {
        self.signal_counts.get(&id).copied().unwrap_or(0)
    }

    /// Returns entries that have warnings.
    pub fn entries_with_warnings(&self) -> Vec<&SceneTreeEntry> {
        self.entries
            .iter()
            .filter(|e| !e.indicators.warnings.is_empty())
            .collect()
    }

    /// Returns entries that have scripts attached.
    pub fn entries_with_scripts(&self) -> Vec<&SceneTreeEntry> {
        self.entries
            .iter()
            .filter(|e| e.indicators.has_script)
            .collect()
    }

    /// Returns entries that are instanced scene roots.
    pub fn entries_with_instances(&self) -> Vec<&SceneTreeEntry> {
        self.entries
            .iter()
            .filter(|e| e.indicators.is_instance)
            .collect()
    }

    /// Returns entries that are locked.
    pub fn entries_locked(&self) -> Vec<&SceneTreeEntry> {
        self.entries
            .iter()
            .filter(|e| e.indicators.locked)
            .collect()
    }

    // -- Selection management --

    /// Select a single node, replacing any existing selection.
    pub fn select_node(&mut self, id: NodeId) {
        self.selected_nodes.clear();
        self.selected_nodes.push(id);
    }

    /// Add a node to the current selection (multi-select).
    pub fn add_to_selection(&mut self, id: NodeId) {
        if !self.selected_nodes.contains(&id) {
            self.selected_nodes.push(id);
        }
    }

    /// Remove a node from the current selection.
    pub fn remove_from_selection(&mut self, id: NodeId) {
        self.selected_nodes.retain(|&n| n != id);
    }

    /// Clear the selection entirely.
    pub fn clear_selection(&mut self) {
        self.selected_nodes.clear();
    }

    /// Returns the currently selected node IDs.
    pub fn selected_nodes(&self) -> &[NodeId] {
        &self.selected_nodes
    }

    /// Returns true if a specific node is selected.
    pub fn is_selected(&self, id: NodeId) -> bool {
        self.selected_nodes.contains(&id)
    }

    /// Returns the number of selected nodes.
    pub fn selection_count(&self) -> usize {
        self.selected_nodes.len()
    }

    /// Collects entries recursively from the scene tree.
    fn collect_entries(
        tree: &SceneTree,
        id: NodeId,
        depth: usize,
        script_paths: &HashMap<NodeId, String>,
        signal_counts: &HashMap<NodeId, usize>,
        out: &mut Vec<SceneTreeEntry>,
    ) {
        let node = match tree.get_node(id) {
            Some(n) => n,
            None => return,
        };
        let path = tree.node_path(id).unwrap_or_default();
        let class_name = node.class_name().to_string();
        let children: Vec<NodeId> = node.children().to_vec();

        let icon = resolve_node_icon(&class_name);
        let warnings = compute_node_warnings(tree, id);

        let script_path = script_paths.get(&id).cloned();
        let has_script = script_path.is_some();

        let groups = node.groups();
        let has_groups = !groups.is_empty();

        let node_name = node.name().to_string();
        // Read the canonical `unique_name` flag (the same source of truth the
        // scene tree, packed-scene serialization, and `%Name` resolution use)
        // rather than the loose `unique_name_in_owner` property, so the `%`
        // badge stays consistent with `set_unique_name_in_owner`.
        let is_unique_name = tree.is_unique_name_in_owner(id);
        let unique_name_collision = tree.unique_name_collision(id);

        // Edit-lock badge: honor the Godot-canonical `_edit_lock_` meta and the
        // legacy `_locked` flag.
        let locked = matches!(node.get_property("_edit_lock_"), gdvariant::Variant::Bool(true))
            || matches!(node.get_property("_locked"), gdvariant::Variant::Bool(true));

        let instance_source = match node.get_property("_instance_source") {
            gdvariant::Variant::String(s) if !s.is_empty() => Some(s.clone()),
            _ => None,
        };
        let is_instance = instance_source.is_some();

        let signal_count = signal_counts.get(&id).copied().unwrap_or(0);

        let indicators = NodeIndicators {
            has_script,
            script_path,
            has_signals: signal_count > 0,
            signal_count,
            has_groups,
            warnings,
            visible: !matches!(
                node.get_property("visible"),
                gdvariant::Variant::Bool(false)
            ),
            is_unique_name,
            unique_name_collision,
            locked,
            is_instance,
            instance_source,
        };

        out.push(SceneTreeEntry {
            id,
            name: node_name,
            class_name,
            path,
            depth,
            icon,
            indicators,
        });
        for child_id in children {
            Self::collect_entries(tree, child_id, depth + 1, script_paths, signal_counts, out);
        }
    }
}

impl Default for SceneTreeDock {
    fn default() -> Self {
        Self::new()
    }
}

/// Represents the current selection state for the scene tree dock,
/// including the primary selection and any multi-selected nodes.
#[derive(Debug, Clone, Default)]
pub struct SelectionState {
    /// The primary (most recently selected) node.
    pub primary: Option<NodeId>,
    /// All selected nodes (including primary).
    pub nodes: Vec<NodeId>,
}

impl SelectionState {
    /// Returns true if any node is selected.
    pub fn has_selection(&self) -> bool {
        !self.nodes.is_empty()
    }

    /// Returns true if multiple nodes are selected.
    pub fn is_multi_select(&self) -> bool {
        self.nodes.len() > 1
    }
}

impl SceneTreeDock {
    /// Returns the current selection state as a snapshot.
    pub fn selection_state(&self) -> SelectionState {
        SelectionState {
            primary: self.selected_nodes.last().copied(),
            nodes: self.selected_nodes.clone(),
        }
    }
}

impl DockPanel for SceneTreeDock {
    fn title(&self) -> &str {
        "Scene"
    }

    fn refresh(&mut self, tree: &SceneTree) {
        self.entries.clear();
        Self::collect_entries(
            tree,
            tree.root_id(),
            0,
            &self.script_paths,
            &self.signal_counts,
            &mut self.entries,
        );
        tracing::debug!("SceneTreeDock refreshed: {} entries", self.entries.len());
    }
}

/// A dock panel wrapping the property inspector.
///
/// Delegates to an [`InspectorPanel`] and displays the properties of
/// the currently inspected node.
#[derive(Debug)]
pub struct PropertyDock {
    /// The underlying inspector.
    inspector: InspectorPanel,
}

impl PropertyDock {
    /// Creates a new property dock.
    pub fn new() -> Self {
        Self {
            inspector: InspectorPanel::new(),
        }
    }

    /// Returns a reference to the underlying inspector.
    pub fn inspector(&self) -> &InspectorPanel {
        &self.inspector
    }

    /// Returns a mutable reference to the underlying inspector.
    pub fn inspector_mut(&mut self) -> &mut InspectorPanel {
        &mut self.inspector
    }
}

impl Default for PropertyDock {
    fn default() -> Self {
        Self::new()
    }
}

impl DockPanel for PropertyDock {
    fn title(&self) -> &str {
        "Inspector"
    }

    fn refresh(&mut self, _tree: &SceneTree) {
        // The inspector reads properties on demand — nothing to cache.
    }
}

// ---------------------------------------------------------------------------
// Plugin dock registration
// ---------------------------------------------------------------------------

/// A dock panel registered by an editor plugin.
///
/// Holds metadata about the panel (title, owning plugin, slot) without
/// prescribing how the UI renders it. The editor UI layer queries the
/// [`PluginDockManager`] to discover which plugin docks exist and where
/// they should be placed.
#[derive(Debug, Clone)]
pub struct PluginDockPanel {
    /// The plugin that registered this dock.
    pub plugin_id: String,
    /// The dock slot where this panel should be placed.
    pub slot: DockSlot,
    /// The display title of this dock panel.
    pub title: String,
    /// Whether this dock panel is currently visible.
    pub visible: bool,
}

/// Manages dock panels registered by editor plugins.
///
/// Provides add/remove/query operations for plugin-registered docks.
/// The editor UI layer uses this to build the dock layout.
#[derive(Debug, Default)]
pub struct PluginDockManager {
    panels: Vec<PluginDockPanel>,
}

impl PluginDockManager {
    /// Creates a new empty dock manager.
    pub fn new() -> Self {
        Self { panels: Vec::new() }
    }

    /// Registers a new dock panel from a plugin.
    ///
    /// If a panel with the same plugin_id and title already exists, it is
    /// not duplicated.
    pub fn add_dock(&mut self, plugin_id: &str, slot: DockSlot, title: &str) {
        let already_exists = self
            .panels
            .iter()
            .any(|p| p.plugin_id == plugin_id && p.title == title);
        if !already_exists {
            self.panels.push(PluginDockPanel {
                plugin_id: plugin_id.to_string(),
                slot,
                title: title.to_string(),
                visible: true,
            });
        }
    }

    /// Removes all dock panels registered by the given plugin.
    pub fn remove_plugin_docks(&mut self, plugin_id: &str) {
        self.panels.retain(|p| p.plugin_id != plugin_id);
    }

    /// Removes a specific dock panel by plugin ID and title.
    pub fn remove_dock(&mut self, plugin_id: &str, title: &str) {
        self.panels
            .retain(|p| !(p.plugin_id == plugin_id && p.title == title));
    }

    /// Returns all registered dock panels.
    pub fn all_panels(&self) -> &[PluginDockPanel] {
        &self.panels
    }

    /// Returns dock panels in a specific slot.
    pub fn panels_in_slot(&self, slot: DockSlot) -> Vec<&PluginDockPanel> {
        self.panels.iter().filter(|p| p.slot == slot).collect()
    }

    /// Returns visible dock panels in a specific slot.
    pub fn visible_panels_in_slot(&self, slot: DockSlot) -> Vec<&PluginDockPanel> {
        self.panels
            .iter()
            .filter(|p| p.slot == slot && p.visible)
            .collect()
    }

    /// Sets the visibility of a dock panel.
    pub fn set_visible(&mut self, plugin_id: &str, title: &str, visible: bool) {
        if let Some(panel) = self
            .panels
            .iter_mut()
            .find(|p| p.plugin_id == plugin_id && p.title == title)
        {
            panel.visible = visible;
        }
    }

    /// Returns the total number of registered dock panels.
    pub fn panel_count(&self) -> usize {
        self.panels.len()
    }

    /// Returns panels registered by a specific plugin.
    pub fn panels_for_plugin(&self, plugin_id: &str) -> Vec<&PluginDockPanel> {
        self.panels
            .iter()
            .filter(|p| p.plugin_id == plugin_id)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gdscene::node::Node;

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

    #[test]
    fn scene_tree_dock_refresh() {
        let tree = make_tree();
        let mut dock = SceneTreeDock::new();
        dock.refresh(&tree);

        assert_eq!(dock.entries().len(), 4); // root, Main, Player, Enemy
        assert_eq!(dock.entries()[0].name, "root");
        assert_eq!(dock.entries()[0].depth, 0);
        assert_eq!(dock.entries()[1].name, "Main");
        assert_eq!(dock.entries()[1].depth, 1);
        assert_eq!(dock.entries()[2].name, "Player");
        assert_eq!(dock.entries()[2].depth, 2);
    }

    #[test]
    fn scene_tree_dock_unique_name_badge_and_collision() {
        // The `%` badge and collision warning in the dock must reflect the
        // canonical `set_unique_name_in_owner` flag, not a stale property.
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let a = tree.add_child(root, Node::new("ContainerA", "Node")).unwrap();
        let b = tree.add_child(root, Node::new("ContainerB", "Node")).unwrap();
        let foo1 = tree.add_child(a, Node::new("Foo", "Node")).unwrap();
        let foo2 = tree.add_child(b, Node::new("Foo", "Node")).unwrap();
        for n in [a, b, foo1, foo2] {
            tree.get_node_mut(n).unwrap().set_owner(Some(root));
        }

        let mut dock = SceneTreeDock::new();
        dock.refresh(&tree);

        // No badge before the flag is set.
        let ind = &dock.find_entry(foo1).unwrap().indicators;
        assert!(!ind.is_unique_name);
        assert!(!ind.unique_name_collision);

        // Enabling on a single node shows the badge with no collision.
        tree.set_unique_name_in_owner(foo1, true);
        dock.refresh(&tree);
        let ind = &dock.find_entry(foo1).unwrap().indicators;
        assert!(ind.is_unique_name);
        assert!(!ind.unique_name_collision);

        // A duplicate unique name in the same owner flags both rows and adds a
        // collision warning.
        tree.set_unique_name_in_owner(foo2, true);
        dock.refresh(&tree);
        for n in [foo1, foo2] {
            let ind = &dock.find_entry(n).unwrap().indicators;
            assert!(ind.is_unique_name);
            assert!(ind.unique_name_collision);
            assert!(ind
                .warnings
                .iter()
                .any(|w| w.severity == WarningSeverity::Warning
                    && w.message.contains("unique name")));
        }

        // Clearing one flag resolves the collision on both.
        tree.set_unique_name_in_owner(foo2, false);
        dock.refresh(&tree);
        assert!(!dock.find_entry(foo2).unwrap().indicators.is_unique_name);
        assert!(!dock
            .find_entry(foo1)
            .unwrap()
            .indicators
            .unique_name_collision);
    }

    #[test]
    fn scene_tree_dock_paths() {
        let tree = make_tree();
        let mut dock = SceneTreeDock::new();
        dock.refresh(&tree);

        assert_eq!(dock.entries()[0].path, "/root");
        assert_eq!(dock.entries()[2].path, "/root/Main/Player");
    }

    #[test]
    fn scene_tree_dock_title() {
        let dock = SceneTreeDock::new();
        assert_eq!(dock.title(), "Scene");
    }

    #[test]
    fn property_dock_title() {
        let dock = PropertyDock::new();
        assert_eq!(dock.title(), "Inspector");
    }

    #[test]
    fn find_entry_by_id() {
        let tree = make_tree();
        let mut dock = SceneTreeDock::new();
        dock.refresh(&tree);

        let root_id = tree.root_id();
        let entry = dock.find_entry(root_id).unwrap();
        assert_eq!(entry.name, "root");
    }

    #[test]
    fn find_nonexistent_entry() {
        let dock = SceneTreeDock::new();
        assert!(dock.find_entry(NodeId::next()).is_none());
    }

    // -- PluginDockManager tests --

    #[test]
    fn plugin_dock_manager_add_and_query() {
        let mut mgr = PluginDockManager::new();
        assert_eq!(mgr.panel_count(), 0);

        mgr.add_dock("my_plugin", DockSlot::Bottom, "My Panel");
        assert_eq!(mgr.panel_count(), 1);
        assert_eq!(mgr.all_panels()[0].title, "My Panel");
        assert_eq!(mgr.all_panels()[0].slot, DockSlot::Bottom);
        assert!(mgr.all_panels()[0].visible);
    }

    #[test]
    fn plugin_dock_manager_no_duplicates() {
        let mut mgr = PluginDockManager::new();
        mgr.add_dock("my_plugin", DockSlot::Bottom, "My Panel");
        mgr.add_dock("my_plugin", DockSlot::Bottom, "My Panel");
        assert_eq!(mgr.panel_count(), 1);
    }

    #[test]
    fn plugin_dock_manager_different_titles_allowed() {
        let mut mgr = PluginDockManager::new();
        mgr.add_dock("my_plugin", DockSlot::Bottom, "Panel A");
        mgr.add_dock("my_plugin", DockSlot::LeftLower, "Panel B");
        assert_eq!(mgr.panel_count(), 2);
    }

    #[test]
    fn plugin_dock_manager_panels_in_slot() {
        let mut mgr = PluginDockManager::new();
        mgr.add_dock("p1", DockSlot::Bottom, "Output");
        mgr.add_dock("p2", DockSlot::Bottom, "Profiler");
        mgr.add_dock("p3", DockSlot::LeftLower, "Files");

        let bottom = mgr.panels_in_slot(DockSlot::Bottom);
        assert_eq!(bottom.len(), 2);
        let left = mgr.panels_in_slot(DockSlot::LeftLower);
        assert_eq!(left.len(), 1);
        let right = mgr.panels_in_slot(DockSlot::RightUpper);
        assert_eq!(right.len(), 0);
    }

    #[test]
    fn plugin_dock_manager_remove_plugin_docks() {
        let mut mgr = PluginDockManager::new();
        mgr.add_dock("p1", DockSlot::Bottom, "Panel A");
        mgr.add_dock("p1", DockSlot::LeftLower, "Panel B");
        mgr.add_dock("p2", DockSlot::Bottom, "Panel C");
        assert_eq!(mgr.panel_count(), 3);

        mgr.remove_plugin_docks("p1");
        assert_eq!(mgr.panel_count(), 1);
        assert_eq!(mgr.all_panels()[0].plugin_id, "p2");
    }

    #[test]
    fn plugin_dock_manager_remove_specific_dock() {
        let mut mgr = PluginDockManager::new();
        mgr.add_dock("p1", DockSlot::Bottom, "A");
        mgr.add_dock("p1", DockSlot::Bottom, "B");
        assert_eq!(mgr.panel_count(), 2);

        mgr.remove_dock("p1", "A");
        assert_eq!(mgr.panel_count(), 1);
        assert_eq!(mgr.all_panels()[0].title, "B");
    }

    #[test]
    fn plugin_dock_manager_visibility() {
        let mut mgr = PluginDockManager::new();
        mgr.add_dock("p1", DockSlot::Bottom, "Panel");

        assert_eq!(mgr.visible_panels_in_slot(DockSlot::Bottom).len(), 1);

        mgr.set_visible("p1", "Panel", false);
        assert_eq!(mgr.visible_panels_in_slot(DockSlot::Bottom).len(), 0);
        // Still in panels_in_slot (includes hidden)
        assert_eq!(mgr.panels_in_slot(DockSlot::Bottom).len(), 1);

        mgr.set_visible("p1", "Panel", true);
        assert_eq!(mgr.visible_panels_in_slot(DockSlot::Bottom).len(), 1);
    }

    #[test]
    fn plugin_dock_manager_panels_for_plugin() {
        let mut mgr = PluginDockManager::new();
        mgr.add_dock("p1", DockSlot::Bottom, "A");
        mgr.add_dock("p1", DockSlot::LeftLower, "B");
        mgr.add_dock("p2", DockSlot::Bottom, "C");

        let p1_panels = mgr.panels_for_plugin("p1");
        assert_eq!(p1_panels.len(), 2);
        let p2_panels = mgr.panels_for_plugin("p2");
        assert_eq!(p2_panels.len(), 1);
        let p3_panels = mgr.panels_for_plugin("nonexistent");
        assert_eq!(p3_panels.len(), 0);
    }

    // -- Node type icon tests --

    #[test]
    fn resolve_icon_node2d() {
        let icon = resolve_node_icon("Node2D");
        assert_eq!(icon.icon_name, "Node2D");
        assert_eq!(icon.color_category, IconColorCategory::Node2D);
    }

    #[test]
    fn resolve_icon_node3d() {
        let icon = resolve_node_icon("MeshInstance3D");
        assert_eq!(icon.color_category, IconColorCategory::Node3D);
    }

    #[test]
    fn resolve_icon_control() {
        let icon = resolve_node_icon("Button");
        assert_eq!(icon.color_category, IconColorCategory::Control);

        let label = resolve_node_icon("Label");
        assert_eq!(label.color_category, IconColorCategory::Control);

        let panel = resolve_node_icon("PanelContainer");
        assert_eq!(panel.color_category, IconColorCategory::Control);
    }

    #[test]
    fn resolve_icon_generic() {
        let icon = resolve_node_icon("Node");
        assert_eq!(icon.color_category, IconColorCategory::Default);
    }

    #[test]
    fn classify_3d_types() {
        assert_eq!(classify_node_color("Camera3D"), IconColorCategory::Node3D);
        assert_eq!(classify_node_color("Skeleton3D"), IconColorCategory::Node3D);
        assert_eq!(classify_node_color("Light3D"), IconColorCategory::Node3D);
        assert_eq!(
            classify_node_color("StaticBody3D"),
            IconColorCategory::Node3D
        );
    }

    #[test]
    fn classify_2d_types() {
        assert_eq!(classify_node_color("Sprite2D"), IconColorCategory::Node2D);
        assert_eq!(classify_node_color("TileMap"), IconColorCategory::Node2D);
        assert_eq!(classify_node_color("Camera2D"), IconColorCategory::Node2D);
    }

    #[test]
    fn classify_control_types() {
        assert_eq!(classify_node_color("CheckBox"), IconColorCategory::Control);
        assert_eq!(classify_node_color("TextEdit"), IconColorCategory::Control);
        assert_eq!(
            classify_node_color("RichTextLabel"),
            IconColorCategory::Control
        );
        assert_eq!(
            classify_node_color("ProgressBar"),
            IconColorCategory::Control
        );
    }

    // -- SceneTreeEntry indicator tests --

    #[test]
    fn entries_have_icons_after_refresh() {
        let tree = make_tree();
        let mut dock = SceneTreeDock::new();
        dock.refresh(&tree);

        // root is "Node" → Default
        assert_eq!(
            dock.entries()[0].icon.color_category,
            IconColorCategory::Default
        );
        // Main is "Node" → Default
        assert_eq!(
            dock.entries()[1].icon.color_category,
            IconColorCategory::Default
        );
        // Player is "Node2D" → Node2D
        assert_eq!(
            dock.entries()[2].icon.color_category,
            IconColorCategory::Node2D
        );
        // Enemy is "Sprite2D" → Node2D
        assert_eq!(
            dock.entries()[3].icon.color_category,
            IconColorCategory::Node2D
        );
    }

    #[test]
    fn entries_have_default_indicators() {
        let tree = make_tree();
        let mut dock = SceneTreeDock::new();
        dock.refresh(&tree);

        let entry = &dock.entries()[0];
        assert!(!entry.indicators.has_script);
        assert!(entry.indicators.script_path.is_none());
        assert!(!entry.indicators.has_signals);
        assert_eq!(entry.indicators.signal_count, 0);
        assert!(entry.indicators.warnings.is_empty());
        assert!(entry.indicators.visible);
    }

    #[test]
    fn script_indicator_shows_in_entry() {
        let tree = make_tree();
        let mut dock = SceneTreeDock::new();

        // Get the Main node id (second entry after root).
        dock.refresh(&tree);
        let main_id = dock.entries()[1].id;

        dock.set_node_script(main_id, "res://scripts/main.gd");
        dock.refresh(&tree);

        let main_entry = dock.find_entry(main_id).unwrap();
        assert!(main_entry.indicators.has_script);
        assert_eq!(
            main_entry.indicators.script_path.as_deref(),
            Some("res://scripts/main.gd")
        );
    }

    #[test]
    fn signal_indicator_shows_in_entry() {
        let tree = make_tree();
        let mut dock = SceneTreeDock::new();
        dock.refresh(&tree);
        let main_id = dock.entries()[1].id;

        dock.set_node_signal_count(main_id, 3);
        dock.refresh(&tree);

        let main_entry = dock.find_entry(main_id).unwrap();
        assert!(main_entry.indicators.has_signals);
        assert_eq!(main_entry.indicators.signal_count, 3);
    }

    #[test]
    fn group_indicator_shows_in_entry() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let mut player = Node::new("Player", "Node2D");
        player.add_to_group("enemies");
        player.add_to_group("killable");
        let player_id = tree.add_child(root, player).unwrap();

        let mut dock = SceneTreeDock::new();
        dock.refresh(&tree);

        let player_entry = dock.find_entry(player_id).unwrap();
        assert!(player_entry.indicators.has_groups);
    }

    #[test]
    fn entries_with_scripts_filter() {
        let tree = make_tree();
        let mut dock = SceneTreeDock::new();
        dock.refresh(&tree);
        let main_id = dock.entries()[1].id;
        let player_id = dock.entries()[2].id;

        dock.set_node_script(main_id, "res://main.gd");
        dock.set_node_script(player_id, "res://player.gd");
        dock.refresh(&tree);

        let scripted = dock.entries_with_scripts();
        assert_eq!(scripted.len(), 2);
    }

    #[test]
    fn remove_script_clears_indicator() {
        let tree = make_tree();
        let mut dock = SceneTreeDock::new();
        dock.refresh(&tree);
        let main_id = dock.entries()[1].id;

        dock.set_node_script(main_id, "res://main.gd");
        dock.refresh(&tree);
        assert!(dock.find_entry(main_id).unwrap().indicators.has_script);

        dock.remove_node_script(main_id);
        dock.refresh(&tree);
        assert!(!dock.find_entry(main_id).unwrap().indicators.has_script);
    }

    // -- Warning tests --

    #[test]
    fn collision_shape_without_body_parent_warns() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let shape = Node::new("Shape", "CollisionShape2D");
        let shape_id = tree.add_child(root, shape).unwrap();

        let warnings = compute_node_warnings(&tree, shape_id);
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].severity, WarningSeverity::Warning);
        assert!(warnings[0].message.contains("physics body"));
    }

    #[test]
    fn collision_shape_with_body_parent_no_warning() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let body = Node::new("Body", "RigidBody2D");
        let body_id = tree.add_child(root, body).unwrap();
        let shape = Node::new("Shape", "CollisionShape2D");
        let shape_id = tree.add_child(body_id, shape).unwrap();

        let warnings = compute_node_warnings(&tree, shape_id);
        assert!(warnings.is_empty());
    }

    #[test]
    fn body_without_collision_shape_warns() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let body = Node::new("Body", "CharacterBody2D");
        let body_id = tree.add_child(root, body).unwrap();

        let warnings = compute_node_warnings(&tree, body_id);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].message.contains("CollisionShape"));
    }

    #[test]
    fn body_with_collision_shape_no_warning() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let body = Node::new("Body", "CharacterBody2D");
        let body_id = tree.add_child(root, body).unwrap();
        let shape = Node::new("Shape", "CollisionShape2D");
        tree.add_child(body_id, shape).unwrap();

        let warnings = compute_node_warnings(&tree, body_id);
        assert!(warnings.is_empty());
    }

    #[test]
    fn sprite_without_texture_info_warning() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let sprite = Node::new("Sprite", "Sprite2D");
        let sprite_id = tree.add_child(root, sprite).unwrap();

        let warnings = compute_node_warnings(&tree, sprite_id);
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].severity, WarningSeverity::Info);
        assert!(warnings[0].message.contains("texture"));
    }

    #[test]
    fn entries_with_warnings_filter() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        // CollisionShape2D under root (not a body) → warning.
        let shape = Node::new("Shape", "CollisionShape2D");
        tree.add_child(root, shape).unwrap();
        // Normal node → no warning.
        let normal = Node::new("Normal", "Node");
        tree.add_child(root, normal).unwrap();

        let mut dock = SceneTreeDock::new();
        dock.refresh(&tree);

        let warned = dock.entries_with_warnings();
        assert_eq!(warned.len(), 1);
        assert_eq!(warned[0].name, "Shape");
    }

    #[test]
    fn node_warning_severity_variants() {
        let info = NodeWarning {
            severity: WarningSeverity::Info,
            message: "info".into(),
        };
        let warn = NodeWarning {
            severity: WarningSeverity::Warning,
            message: "warn".into(),
        };
        let err = NodeWarning {
            severity: WarningSeverity::Error,
            message: "err".into(),
        };
        assert_ne!(info.severity, warn.severity);
        assert_ne!(warn.severity, err.severity);
    }

    #[test]
    fn icon_color_category_default() {
        assert_eq!(IconColorCategory::default(), IconColorCategory::Default);
    }

    #[test]
    fn node_indicators_default() {
        let ind = NodeIndicators::default();
        assert!(!ind.has_script);
        assert!(ind.script_path.is_none());
        assert!(!ind.has_signals);
        assert_eq!(ind.signal_count, 0);
        assert!(!ind.has_groups);
        assert!(ind.warnings.is_empty());
        assert!(!ind.visible); // Default for bool is false
        assert!(!ind.is_unique_name);
        assert!(!ind.locked);
        assert!(!ind.is_instance);
        assert!(ind.instance_source.is_none());
    }

    // -- Lock indicator tests --

    #[test]
    fn locked_node_shows_lock_indicator() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let mut player = Node::new("Player", "Node2D");
        player.set_property("_locked", gdvariant::Variant::Bool(true));
        let player_id = tree.add_child(root, player).unwrap();

        let mut dock = SceneTreeDock::new();
        dock.refresh(&tree);

        let entry = dock.find_entry(player_id).unwrap();
        assert!(entry.indicators.locked);
    }

    #[test]
    fn unlocked_node_no_lock_indicator() {
        let tree = make_tree();
        let mut dock = SceneTreeDock::new();
        dock.refresh(&tree);

        for entry in dock.entries() {
            assert!(!entry.indicators.locked);
        }
    }

    #[test]
    fn entries_locked_filter() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let mut locked = Node::new("Locked", "Node2D");
        locked.set_property("_locked", gdvariant::Variant::Bool(true));
        tree.add_child(root, locked).unwrap();
        let unlocked = Node::new("Unlocked", "Node2D");
        tree.add_child(root, unlocked).unwrap();

        let mut dock = SceneTreeDock::new();
        dock.refresh(&tree);

        let locked_entries = dock.entries_locked();
        assert_eq!(locked_entries.len(), 1);
        assert_eq!(locked_entries[0].name, "Locked");
    }

    // -- Instance indicator tests --

    #[test]
    fn instanced_node_shows_instance_indicator() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let mut instanced = Node::new("Level", "Node2D");
        instanced.set_property(
            "_instance_source",
            gdvariant::Variant::String("res://scenes/level.tscn".to_string()),
        );
        let id = tree.add_child(root, instanced).unwrap();

        let mut dock = SceneTreeDock::new();
        dock.refresh(&tree);

        let entry = dock.find_entry(id).unwrap();
        assert!(entry.indicators.is_instance);
        assert_eq!(
            entry.indicators.instance_source.as_deref(),
            Some("res://scenes/level.tscn")
        );
    }

    #[test]
    fn non_instanced_node_no_instance_indicator() {
        let tree = make_tree();
        let mut dock = SceneTreeDock::new();
        dock.refresh(&tree);

        for entry in dock.entries() {
            assert!(!entry.indicators.is_instance);
            assert!(entry.indicators.instance_source.is_none());
        }
    }

    #[test]
    fn entries_with_instances_filter() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let mut inst = Node::new("Scene", "Node2D");
        inst.set_property(
            "_instance_source",
            gdvariant::Variant::String("instanced".to_string()),
        );
        tree.add_child(root, inst).unwrap();
        let plain = Node::new("Plain", "Node2D");
        tree.add_child(root, plain).unwrap();

        let mut dock = SceneTreeDock::new();
        dock.refresh(&tree);

        let instances = dock.entries_with_instances();
        assert_eq!(instances.len(), 1);
        assert_eq!(instances[0].name, "Scene");
    }

    // -- Selection state tests --

    #[test]
    fn single_select() {
        let tree = make_tree();
        let mut dock = SceneTreeDock::new();
        dock.refresh(&tree);

        let main_id = dock.entries()[1].id;
        dock.select_node(main_id);

        assert_eq!(dock.selection_count(), 1);
        assert!(dock.is_selected(main_id));
        assert_eq!(dock.selected_nodes(), &[main_id]);
    }

    #[test]
    fn multi_select() {
        let tree = make_tree();
        let mut dock = SceneTreeDock::new();
        dock.refresh(&tree);

        let main_id = dock.entries()[1].id;
        let player_id = dock.entries()[2].id;

        dock.select_node(main_id);
        dock.add_to_selection(player_id);

        assert_eq!(dock.selection_count(), 2);
        assert!(dock.is_selected(main_id));
        assert!(dock.is_selected(player_id));
    }

    #[test]
    fn add_to_selection_no_duplicates() {
        let tree = make_tree();
        let mut dock = SceneTreeDock::new();
        dock.refresh(&tree);

        let main_id = dock.entries()[1].id;
        dock.select_node(main_id);
        dock.add_to_selection(main_id);

        assert_eq!(dock.selection_count(), 1);
    }

    #[test]
    fn remove_from_selection() {
        let tree = make_tree();
        let mut dock = SceneTreeDock::new();
        dock.refresh(&tree);

        let main_id = dock.entries()[1].id;
        let player_id = dock.entries()[2].id;

        dock.select_node(main_id);
        dock.add_to_selection(player_id);
        dock.remove_from_selection(main_id);

        assert_eq!(dock.selection_count(), 1);
        assert!(!dock.is_selected(main_id));
        assert!(dock.is_selected(player_id));
    }

    #[test]
    fn clear_selection() {
        let tree = make_tree();
        let mut dock = SceneTreeDock::new();
        dock.refresh(&tree);

        let main_id = dock.entries()[1].id;
        dock.select_node(main_id);
        dock.clear_selection();

        assert_eq!(dock.selection_count(), 0);
        assert!(!dock.is_selected(main_id));
    }

    #[test]
    fn selection_state_snapshot() {
        let tree = make_tree();
        let mut dock = SceneTreeDock::new();
        dock.refresh(&tree);

        // Empty state
        let state = dock.selection_state();
        assert!(!state.has_selection());
        assert!(!state.is_multi_select());
        assert!(state.primary.is_none());

        // Single select
        let main_id = dock.entries()[1].id;
        dock.select_node(main_id);
        let state = dock.selection_state();
        assert!(state.has_selection());
        assert!(!state.is_multi_select());
        assert_eq!(state.primary, Some(main_id));

        // Multi select
        let player_id = dock.entries()[2].id;
        dock.add_to_selection(player_id);
        let state = dock.selection_state();
        assert!(state.has_selection());
        assert!(state.is_multi_select());
        assert_eq!(state.primary, Some(player_id));
        assert_eq!(state.nodes.len(), 2);
    }

    #[test]
    fn select_replaces_previous() {
        let tree = make_tree();
        let mut dock = SceneTreeDock::new();
        dock.refresh(&tree);

        let main_id = dock.entries()[1].id;
        let player_id = dock.entries()[2].id;

        dock.select_node(main_id);
        dock.select_node(player_id);

        assert_eq!(dock.selection_count(), 1);
        assert!(!dock.is_selected(main_id));
        assert!(dock.is_selected(player_id));
    }

    /// Acceptance (pat-9wup3.1): every scene-tree row carries a per-class type
    /// icon — the icon name matches the node's class and the color category
    /// follows Godot's 2D/3D/Control/Default convention. Custom/script classes
    /// still get a per-class icon, with the category inferred from the class
    /// naming convention (a `*2D` class is a 2D node).
    #[test]
    fn editor_tree_class_type_icons() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let n2d = tree.add_child(root, Node::new("Body", "Node2D")).unwrap();
        let sprite = tree.add_child(n2d, Node::new("Sprite", "Sprite2D")).unwrap();
        let cam3d = tree.add_child(root, Node::new("Cam", "Camera3D")).unwrap();
        let mesh = tree
            .add_child(root, Node::new("Mesh", "MeshInstance3D"))
            .unwrap();
        let button = tree.add_child(root, Node::new("Btn", "Button")).unwrap();
        let label = tree.add_child(root, Node::new("Txt", "Label")).unwrap();
        let plain = tree.add_child(root, Node::new("Plain", "Node")).unwrap();
        // A custom/script class with no built-in icon, named by convention.
        let custom = tree
            .add_child(root, Node::new("Hero", "HeroBody2D"))
            .unwrap();

        let mut dock = SceneTreeDock::new();
        dock.refresh(&tree);

        let expect = |dock: &SceneTreeDock, id: NodeId, class: &str, cat: IconColorCategory| {
            let entry = dock.find_entry(id).expect("scene tree row present");
            assert_eq!(entry.class_name, class);
            assert_eq!(
                entry.icon.icon_name, class,
                "the row's icon name matches its node class"
            );
            assert_eq!(
                entry.icon.color_category, cat,
                "icon color category for {class}"
            );
        };

        expect(&dock, n2d, "Node2D", IconColorCategory::Node2D);
        expect(&dock, sprite, "Sprite2D", IconColorCategory::Node2D);
        expect(&dock, cam3d, "Camera3D", IconColorCategory::Node3D);
        expect(&dock, mesh, "MeshInstance3D", IconColorCategory::Node3D);
        expect(&dock, button, "Button", IconColorCategory::Control);
        expect(&dock, label, "Label", IconColorCategory::Control);
        expect(&dock, plain, "Node", IconColorCategory::Default);
        // Custom *2D class: per-class icon name, 2D category from the convention.
        expect(&dock, custom, "HeroBody2D", IconColorCategory::Node2D);

        // No row is left iconless: every entry's icon name tracks its class.
        assert!(dock.entries().len() >= 8, "all nodes (incl. root) get a row");
        for e in dock.entries() {
            assert_eq!(
                e.icon.icon_name, e.class_name,
                "row {} carries a per-class icon",
                e.name
            );
        }
    }

    /// Acceptance (pat-9wup3.2): rows with configuration warnings show a warning
    /// triangle whose severity is the worst warning on the node, with a tooltip
    /// listing the messages; clean rows show no triangle.
    #[test]
    fn editor_tree_config_warning_triangle() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        // CollisionShape2D under a non-body parent → Warning.
        let shape = tree
            .add_child(root, Node::new("Shape", "CollisionShape2D"))
            .unwrap();
        // Sprite2D with no texture → Info.
        let sprite = tree.add_child(root, Node::new("Spr", "Sprite2D")).unwrap();
        // Plain node → no warnings.
        let plain = tree.add_child(root, Node::new("Plain", "Node")).unwrap();

        let mut dock = SceneTreeDock::new();
        dock.refresh(&tree);

        // The CollisionShape row shows a Warning-severity triangle + tooltip.
        let shape_row = dock.find_entry(shape).expect("shape row");
        assert!(shape_row.indicators.has_warning_triangle());
        assert_eq!(
            shape_row.indicators.warning_triangle(),
            Some(WarningSeverity::Warning)
        );
        assert!(
            shape_row
                .indicators
                .warning_tooltip()
                .contains("physics body"),
            "tooltip carries the warning message"
        );

        // The textureless Sprite shows an Info-severity triangle.
        let sprite_row = dock.find_entry(sprite).expect("sprite row");
        assert!(sprite_row.indicators.has_warning_triangle());
        assert_eq!(
            sprite_row.indicators.warning_triangle(),
            Some(WarningSeverity::Info)
        );

        // A clean node shows no triangle and an empty tooltip.
        let plain_row = dock.find_entry(plain).expect("plain row");
        assert!(!plain_row.indicators.has_warning_triangle());
        assert_eq!(plain_row.indicators.warning_triangle(), None);
        assert!(plain_row.indicators.warning_tooltip().is_empty());

        // The "rows with warnings" filter surfaces exactly the warned rows.
        let warned: Vec<&str> = dock
            .entries_with_warnings()
            .iter()
            .map(|e| e.name.as_str())
            .collect();
        assert!(warned.contains(&"Shape"));
        assert!(warned.contains(&"Spr"));
        assert!(!warned.contains(&"Plain"));
    }

    /// Acceptance (pat-9wup3.3): a node with an attached script shows a script
    /// badge (with its path), the root of an instanced scene shows a
    /// scene-instance badge (with its source), and a plain node shows neither.
    #[test]
    fn editor_tree_script_and_instance_badges() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let scripted = tree.add_child(root, Node::new("Player", "Node2D")).unwrap();
        // An instanced-scene root: tagged via the `_instance_source` property.
        let mut instanced = Node::new("Enemy", "Node2D");
        instanced.set_property(
            "_instance_source",
            gdvariant::Variant::String("res://enemy.tscn".to_string()),
        );
        let instanced = tree.add_child(root, instanced).unwrap();
        let plain = tree.add_child(root, Node::new("Plain", "Node")).unwrap();

        let mut dock = SceneTreeDock::new();
        dock.set_node_script(scripted, "res://player.gd");
        dock.refresh(&tree);

        // Scripted node: script badge with its path, no instance badge.
        let s = &dock.find_entry(scripted).expect("scripted row").indicators;
        assert!(s.has_script_badge());
        assert_eq!(s.script_badge_path(), Some("res://player.gd"));
        assert!(!s.has_instance_badge());
        assert_eq!(s.instance_badge_source(), None);

        // Instanced node: instance badge with its source, no script badge.
        let i = &dock.find_entry(instanced).expect("instanced row").indicators;
        assert!(i.has_instance_badge());
        assert_eq!(i.instance_badge_source(), Some("res://enemy.tscn"));
        assert!(!i.has_script_badge());
        assert_eq!(i.script_badge_path(), None);

        // Plain node: neither badge.
        let p = &dock.find_entry(plain).expect("plain row").indicators;
        assert!(!p.has_script_badge());
        assert!(!p.has_instance_badge());
        assert_eq!(p.script_badge_path(), None);
        assert_eq!(p.instance_badge_source(), None);
    }

    /// Acceptance (pat-9wup3.4): a node with connected signals shows the signal
    /// indicator (with its count), a node accessible as `%Name` shows the
    /// unique-name indicator, and a plain node shows neither.
    #[test]
    fn editor_tree_signal_and_unique_name_indicators() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let emitter = tree.add_child(root, Node::new("Emitter", "Node")).unwrap();
        let unique = tree.add_child(root, Node::new("Hero", "Node")).unwrap();
        let plain = tree.add_child(root, Node::new("Plain", "Node")).unwrap();
        // Owners are required for `%`-name resolution.
        for n in [emitter, unique, plain] {
            tree.get_node_mut(n).unwrap().set_owner(Some(root));
        }
        // `unique` is accessible as `%Hero`.
        tree.set_unique_name_in_owner(unique, true);

        let mut dock = SceneTreeDock::new();
        dock.set_node_signal_count(emitter, 2);
        dock.refresh(&tree);

        // Emitter: signal indicator with count, no unique-name indicator.
        let e = &dock.find_entry(emitter).expect("emitter row").indicators;
        assert!(e.has_signal_indicator());
        assert_eq!(e.signal_indicator_count(), 2);
        assert!(!e.has_unique_name_indicator());

        // Unique node: `%` indicator (no collision), no signal indicator.
        let u = &dock.find_entry(unique).expect("unique row").indicators;
        assert!(u.has_unique_name_indicator());
        assert!(!u.unique_name_collision);
        assert!(!u.has_signal_indicator());
        assert_eq!(u.signal_indicator_count(), 0);

        // Plain node: neither indicator.
        let p = &dock.find_entry(plain).expect("plain row").indicators;
        assert!(!p.has_signal_indicator());
        assert!(!p.has_unique_name_indicator());
        assert_eq!(p.signal_indicator_count(), 0);
    }

    /// Acceptance (pat-9wup3.5): the visibility, lock, and group toggles both
    /// render (the row reflects the node's state) and act (toggling mutates the
    /// node and the row updates on refresh).
    #[test]
    fn editor_tree_visibility_lock_group_toggles() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let node = tree.add_child(root, Node::new("Sprite", "Node2D")).unwrap();

        let mut dock = SceneTreeDock::new();
        dock.refresh(&tree);

        // Render: defaults — visible, unlocked, no groups.
        {
            let ind = &dock.find_entry(node).expect("row").indicators;
            assert!(ind.visible);
            assert!(!ind.locked);
            assert!(!ind.has_groups);
        }

        // Act: hide → the eye toggle returns the new (hidden) state and the row
        // re-renders as not visible; toggling again restores visibility.
        assert!(!toggle_node_visibility(&mut tree, node));
        dock.refresh(&tree);
        assert!(!dock.find_entry(node).unwrap().indicators.visible);
        assert!(toggle_node_visibility(&mut tree, node));
        dock.refresh(&tree);
        assert!(dock.find_entry(node).unwrap().indicators.visible);

        // Act: lock → the row re-renders as locked; toggling again unlocks.
        assert!(toggle_node_lock(&mut tree, node));
        dock.refresh(&tree);
        assert!(dock.find_entry(node).unwrap().indicators.locked);
        assert!(!toggle_node_lock(&mut tree, node));
        dock.refresh(&tree);
        assert!(!dock.find_entry(node).unwrap().indicators.locked);

        // Act: add to a group → the row re-renders with groups; removing clears.
        assert!(toggle_node_group(&mut tree, node, "enemies"));
        dock.refresh(&tree);
        assert!(dock.find_entry(node).unwrap().indicators.has_groups);
        assert!(!toggle_node_group(&mut tree, node, "enemies"));
        dock.refresh(&tree);
        assert!(!dock.find_entry(node).unwrap().indicators.has_groups);
    }
}
