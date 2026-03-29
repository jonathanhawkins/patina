//! Editor settings dialog with categorized tabs for General, Theme,
//! Keybindings, and Plugins.
//!
//! Provides a headless model for an editor settings dialog where
//! configuration is organized into tabs. Includes theme customization
//! (accent colors, font size, custom theme colors), keybinding management
//! (reset to defaults, conflict detection, categories), and plugin
//! configuration (version, author, dependencies, settings).

use std::collections::HashMap;

/// Tabs in the editor settings dialog.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SettingsTab {
    /// Grid, snap, rulers, font size.
    General,
    /// Color theme, accent color, custom background.
    Theme,
    /// Keyboard shortcut bindings.
    Keybindings,
    /// Editor plugin management.
    Plugins,
}

impl SettingsTab {
    /// Returns all tabs in display order.
    pub fn all() -> &'static [SettingsTab] {
        &[Self::General, Self::Theme, Self::Keybindings, Self::Plugins]
    }

    /// Display label.
    pub fn label(&self) -> &'static str {
        match self {
            Self::General => "General",
            Self::Theme => "Theme",
            Self::Keybindings => "Keybindings",
            Self::Plugins => "Plugins",
        }
    }

    /// CSS-safe id string.
    pub fn id(&self) -> &'static str {
        match self {
            Self::General => "general",
            Self::Theme => "theme",
            Self::Keybindings => "keybindings",
            Self::Plugins => "plugins",
        }
    }

    /// Parse from id string.
    pub fn from_id(s: &str) -> Option<Self> {
        match s {
            "general" => Some(Self::General),
            "theme" => Some(Self::Theme),
            "keybindings" => Some(Self::Keybindings),
            "plugins" => Some(Self::Plugins),
            _ => None,
        }
    }
}

/// RGBA color represented as `(r, g, b, a)` with values 0–255.
pub type Color = (u8, u8, u8, u8);

/// Built-in accent color presets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccentColor {
    Blue,
    Green,
    Orange,
    Purple,
    Red,
    Custom(u8, u8, u8),
}

impl AccentColor {
    /// Returns the RGBA color value.
    pub fn to_rgba(self) -> Color {
        match self {
            Self::Blue => (68, 138, 255, 255),
            Self::Green => (76, 175, 80, 255),
            Self::Orange => (255, 152, 0, 255),
            Self::Purple => (156, 39, 176, 255),
            Self::Red => (244, 67, 54, 255),
            Self::Custom(r, g, b) => (r, g, b, 255),
        }
    }
}

/// Theme configuration for the editor.
#[derive(Debug, Clone)]
pub struct ThemeConfig {
    /// Base theme: "dark" or "light".
    pub base_theme: String,
    /// Accent color.
    pub accent_color: AccentColor,
    /// Code editor font size in points.
    pub font_size: u8,
    /// UI scale factor (100 = normal).
    pub ui_scale: u16,
    /// Custom color overrides keyed by role (e.g. "background", "text", "selection").
    pub custom_colors: HashMap<String, Color>,
}

impl Default for ThemeConfig {
    fn default() -> Self {
        Self {
            base_theme: "dark".into(),
            accent_color: AccentColor::Blue,
            font_size: 14,
            ui_scale: 100,
            custom_colors: HashMap::new(),
        }
    }
}

impl ThemeConfig {
    /// Valid font size range.
    pub const MIN_FONT_SIZE: u8 = 8;
    pub const MAX_FONT_SIZE: u8 = 48;
    /// Valid UI scale range.
    pub const MIN_UI_SCALE: u16 = 50;
    pub const MAX_UI_SCALE: u16 = 300;

    /// Sets font size, clamping to valid range.
    pub fn set_font_size(&mut self, size: u8) {
        self.font_size = size.clamp(Self::MIN_FONT_SIZE, Self::MAX_FONT_SIZE);
    }

    /// Sets UI scale, clamping to valid range.
    pub fn set_ui_scale(&mut self, scale: u16) {
        self.ui_scale = scale.clamp(Self::MIN_UI_SCALE, Self::MAX_UI_SCALE);
    }

    /// Sets a custom color override.
    pub fn set_custom_color(&mut self, role: &str, color: Color) {
        self.custom_colors.insert(role.to_string(), color);
    }

    /// Removes a custom color override.
    pub fn remove_custom_color(&mut self, role: &str) -> bool {
        self.custom_colors.remove(role).is_some()
    }

    /// Returns the color for a role, falling back to the default for that role.
    pub fn color_for_role(&self, role: &str) -> Color {
        if let Some(&c) = self.custom_colors.get(role) {
            return c;
        }
        match (self.base_theme.as_str(), role) {
            ("dark", "background") => (37, 37, 37, 255),
            ("dark", "text") => (220, 220, 220, 255),
            ("dark", "selection") => (68, 68, 68, 255),
            ("light", "background") => (245, 245, 245, 255),
            ("light", "text") => (33, 33, 33, 255),
            ("light", "selection") => (187, 222, 251, 255),
            _ => (128, 128, 128, 255),
        }
    }

    /// Resets all custom colors.
    pub fn reset_custom_colors(&mut self) {
        self.custom_colors.clear();
    }
}

/// Keybinding category for grouping shortcuts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KeybindingCategory {
    Editing,
    Navigation,
    Scene,
    Tools,
    Playback,
    Other,
}

impl KeybindingCategory {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Editing => "Editing",
            Self::Navigation => "Navigation",
            Self::Scene => "Scene",
            Self::Tools => "Tools",
            Self::Playback => "Playback",
            Self::Other => "Other",
        }
    }

    pub fn all() -> &'static [KeybindingCategory] {
        &[
            Self::Editing,
            Self::Navigation,
            Self::Scene,
            Self::Tools,
            Self::Playback,
            Self::Other,
        ]
    }
}

/// A keyboard shortcut binding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyBinding {
    /// The action name (e.g. "delete", "duplicate", "undo").
    pub action: String,
    /// Human-readable description.
    pub description: String,
    /// The key combination string (e.g. "Ctrl+Z", "F2", "Delete").
    pub keys: String,
    /// Category for grouping.
    pub category: KeybindingCategory,
}

/// A plugin entry for the settings dialog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginInfo {
    /// Plugin name.
    pub name: String,
    /// Whether the plugin is enabled.
    pub enabled: bool,
    /// Optional description.
    pub description: String,
    /// Version string (e.g. "1.0.0").
    pub version: String,
    /// Author name.
    pub author: String,
    /// Plugin dependencies (names of other plugins).
    pub dependencies: Vec<String>,
}

impl PluginInfo {
    /// Creates a basic plugin entry.
    pub fn new(name: &str, description: &str) -> Self {
        Self {
            name: name.into(),
            enabled: false,
            description: description.into(),
            version: "1.0.0".into(),
            author: String::new(),
            dependencies: Vec::new(),
        }
    }
}

/// A keybinding conflict: two actions share the same key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyConflict {
    pub action_a: String,
    pub action_b: String,
    pub keys: String,
}

/// The editor settings dialog state.
#[derive(Debug)]
pub struct EditorSettingsDialog {
    visible: bool,
    active_tab: SettingsTab,
    search_text: String,
    keybindings: Vec<KeyBinding>,
    plugins: Vec<PluginInfo>,
    /// Pending keybinding changes: action -> new keys.
    pending_keybindings: HashMap<String, String>,
    /// Pending plugin toggles: name -> new enabled state.
    pending_plugins: HashMap<String, bool>,
    /// Theme configuration.
    theme_config: ThemeConfig,
    /// Pending theme config (applied on confirm).
    pending_theme: Option<ThemeConfig>,
    /// Filter category for keybinding tab (None = show all).
    keybinding_filter_category: Option<KeybindingCategory>,
}

impl EditorSettingsDialog {
    /// Creates a new dialog with default keybindings.
    pub fn new() -> Self {
        Self {
            visible: false,
            active_tab: SettingsTab::General,
            search_text: String::new(),
            keybindings: Self::default_keybindings(),
            plugins: Vec::new(),
            pending_keybindings: HashMap::new(),
            pending_plugins: HashMap::new(),
            theme_config: ThemeConfig::default(),
            pending_theme: None,
            keybinding_filter_category: None,
        }
    }

    /// Opens the dialog.
    pub fn open(&mut self) {
        self.visible = true;
        self.pending_keybindings.clear();
        self.pending_plugins.clear();
        self.pending_theme = None;
        self.search_text.clear();
        self.keybinding_filter_category = None;
    }

    /// Closes the dialog without applying changes.
    pub fn close(&mut self) {
        self.visible = false;
        self.pending_keybindings.clear();
        self.pending_plugins.clear();
        self.pending_theme = None;
    }

    /// Whether the dialog is visible.
    pub fn is_visible(&self) -> bool {
        self.visible
    }

    /// Sets the active tab.
    pub fn set_tab(&mut self, tab: SettingsTab) {
        self.active_tab = tab;
    }

    /// Returns the active tab.
    pub fn active_tab(&self) -> SettingsTab {
        self.active_tab
    }

    /// Sets the search text for keybinding/plugin filtering.
    pub fn set_search(&mut self, text: &str) {
        self.search_text = text.to_lowercase();
    }

    /// Returns keybindings filtered by current search text.
    pub fn filtered_keybindings(&self) -> Vec<&KeyBinding> {
        if self.search_text.is_empty() {
            return self.keybindings.iter().collect();
        }
        self.keybindings
            .iter()
            .filter(|kb| {
                kb.action.to_lowercase().contains(&self.search_text)
                    || kb.description.to_lowercase().contains(&self.search_text)
                    || kb.keys.to_lowercase().contains(&self.search_text)
            })
            .collect()
    }

    /// Returns plugins filtered by current search text.
    pub fn filtered_plugins(&self) -> Vec<&PluginInfo> {
        if self.search_text.is_empty() {
            return self.plugins.iter().collect();
        }
        self.plugins
            .iter()
            .filter(|p| {
                p.name.to_lowercase().contains(&self.search_text)
                    || p.description.to_lowercase().contains(&self.search_text)
            })
            .collect()
    }

    /// Records a pending keybinding change.
    pub fn set_keybinding(&mut self, action: &str, keys: &str) {
        self.pending_keybindings
            .insert(action.to_string(), keys.to_string());
    }

    /// Records a pending plugin toggle.
    pub fn set_plugin_enabled(&mut self, name: &str, enabled: bool) {
        self.pending_plugins.insert(name.to_string(), enabled);
    }

    /// Returns the effective keys for an action (pending or current).
    pub fn effective_keys(&self, action: &str) -> Option<&str> {
        if let Some(keys) = self.pending_keybindings.get(action) {
            return Some(keys.as_str());
        }
        self.keybindings
            .iter()
            .find(|kb| kb.action == action)
            .map(|kb| kb.keys.as_str())
    }

    /// Returns whether there are unsaved changes.
    pub fn has_pending_changes(&self) -> bool {
        !self.pending_keybindings.is_empty()
            || !self.pending_plugins.is_empty()
            || self.pending_theme.is_some()
    }

    /// Applies pending changes and returns them.
    /// Returns (keybinding_changes, plugin_changes, theme_changed).
    pub fn confirm(&mut self) -> (Vec<(String, String)>, Vec<(String, bool)>, bool) {
        let kb_changes: Vec<(String, String)> = self.pending_keybindings.drain().collect();
        for (action, keys) in &kb_changes {
            if let Some(kb) = self.keybindings.iter_mut().find(|kb| kb.action == *action) {
                kb.keys = keys.clone();
            }
        }
        let plugin_changes: Vec<(String, bool)> = self.pending_plugins.drain().collect();
        for (name, enabled) in &plugin_changes {
            if let Some(p) = self.plugins.iter_mut().find(|p| p.name == *name) {
                p.enabled = *enabled;
            }
        }
        let theme_changed = self.pending_theme.is_some();
        if let Some(theme) = self.pending_theme.take() {
            self.theme_config = theme;
        }
        self.visible = false;
        (kb_changes, plugin_changes, theme_changed)
    }

    /// Loads plugin list from server data.
    pub fn load_plugins(&mut self, plugins: Vec<PluginInfo>) {
        self.plugins = plugins;
    }

    /// Loads keybinding list from server data.
    pub fn load_keybindings(&mut self, keybindings: Vec<KeyBinding>) {
        self.keybindings = keybindings;
    }

    /// Returns the number of keybindings.
    pub fn keybinding_count(&self) -> usize {
        self.keybindings.len()
    }

    /// Returns the number of plugins.
    pub fn plugin_count(&self) -> usize {
        self.plugins.len()
    }

    // ---- Theme configuration ----

    /// Returns a reference to the current theme config.
    pub fn theme_config(&self) -> &ThemeConfig {
        &self.theme_config
    }

    /// Returns the pending theme config, or the current one if no pending changes.
    pub fn effective_theme(&self) -> &ThemeConfig {
        self.pending_theme.as_ref().unwrap_or(&self.theme_config)
    }

    /// Sets the base theme ("dark" or "light").
    pub fn set_base_theme(&mut self, theme: &str) {
        let t = self
            .pending_theme
            .get_or_insert_with(|| self.theme_config.clone());
        t.base_theme = theme.to_string();
    }

    /// Sets the accent color.
    pub fn set_accent_color(&mut self, color: AccentColor) {
        let t = self
            .pending_theme
            .get_or_insert_with(|| self.theme_config.clone());
        t.accent_color = color;
    }

    /// Sets the font size (clamped).
    pub fn set_font_size(&mut self, size: u8) {
        let t = self
            .pending_theme
            .get_or_insert_with(|| self.theme_config.clone());
        t.set_font_size(size);
    }

    /// Sets the UI scale (clamped).
    pub fn set_ui_scale(&mut self, scale: u16) {
        let t = self
            .pending_theme
            .get_or_insert_with(|| self.theme_config.clone());
        t.set_ui_scale(scale);
    }

    /// Sets a custom theme color override.
    pub fn set_theme_custom_color(&mut self, role: &str, color: Color) {
        let t = self
            .pending_theme
            .get_or_insert_with(|| self.theme_config.clone());
        t.set_custom_color(role, color);
    }

    /// Removes a custom theme color override.
    pub fn remove_theme_custom_color(&mut self, role: &str) {
        let t = self
            .pending_theme
            .get_or_insert_with(|| self.theme_config.clone());
        t.remove_custom_color(role);
    }

    /// Resets theme to defaults.
    pub fn reset_theme(&mut self) {
        self.pending_theme = Some(ThemeConfig::default());
    }

    // ---- Keybinding management ----

    /// Sets the keybinding category filter.
    pub fn set_keybinding_filter(&mut self, category: Option<KeybindingCategory>) {
        self.keybinding_filter_category = category;
    }

    /// Returns keybindings filtered by search AND category filter.
    pub fn filtered_keybindings_by_category(&self) -> Vec<&KeyBinding> {
        self.keybindings
            .iter()
            .filter(|kb| {
                if let Some(cat) = self.keybinding_filter_category {
                    if kb.category != cat {
                        return false;
                    }
                }
                if self.search_text.is_empty() {
                    return true;
                }
                kb.action.to_lowercase().contains(&self.search_text)
                    || kb.description.to_lowercase().contains(&self.search_text)
                    || kb.keys.to_lowercase().contains(&self.search_text)
            })
            .collect()
    }

    /// Detects keybinding conflicts (same key bound to multiple actions).
    /// Considers pending changes.
    pub fn detect_conflicts(&self) -> Vec<KeyConflict> {
        let mut key_map: HashMap<String, Vec<String>> = HashMap::new();
        for kb in &self.keybindings {
            let keys = self
                .pending_keybindings
                .get(&kb.action)
                .map(|s| s.as_str())
                .unwrap_or(&kb.keys);
            if !keys.is_empty() {
                key_map
                    .entry(keys.to_lowercase())
                    .or_default()
                    .push(kb.action.clone());
            }
        }
        let mut conflicts = Vec::new();
        for (keys, actions) in &key_map {
            if actions.len() > 1 {
                for i in 0..actions.len() {
                    for j in (i + 1)..actions.len() {
                        conflicts.push(KeyConflict {
                            action_a: actions[i].clone(),
                            action_b: actions[j].clone(),
                            keys: keys.clone(),
                        });
                    }
                }
            }
        }
        conflicts.sort_by(|a, b| a.keys.cmp(&b.keys).then(a.action_a.cmp(&b.action_a)));
        conflicts
    }

    /// Resets a single keybinding to its default.
    pub fn reset_keybinding(&mut self, action: &str) {
        let defaults = Self::default_keybindings();
        if let Some(default_kb) = defaults.iter().find(|kb| kb.action == action) {
            self.pending_keybindings
                .insert(action.to_string(), default_kb.keys.clone());
        }
    }

    /// Resets all keybindings to defaults.
    pub fn reset_all_keybindings(&mut self) {
        let defaults = Self::default_keybindings();
        for kb in &defaults {
            self.pending_keybindings
                .insert(kb.action.clone(), kb.keys.clone());
        }
    }

    // ---- Plugin management ----

    /// Returns plugins that depend on the given plugin name.
    pub fn dependents_of(&self, plugin_name: &str) -> Vec<&PluginInfo> {
        self.plugins
            .iter()
            .filter(|p| p.dependencies.iter().any(|d| d == plugin_name))
            .collect()
    }

    /// Returns unmet dependencies for a plugin.
    pub fn unmet_dependencies(&self, plugin_name: &str) -> Vec<String> {
        let Some(plugin) = self.plugins.iter().find(|p| p.name == plugin_name) else {
            return Vec::new();
        };
        plugin
            .dependencies
            .iter()
            .filter(|dep| !self.plugins.iter().any(|p| p.name == **dep && p.enabled))
            .cloned()
            .collect()
    }

    /// Checks if disabling a plugin would break enabled dependents.
    pub fn would_break_dependents(&self, plugin_name: &str) -> Vec<String> {
        self.plugins
            .iter()
            .filter(|p| p.enabled && p.dependencies.iter().any(|d| d == plugin_name))
            .map(|p| p.name.clone())
            .collect()
    }

    /// Returns enabled plugin count.
    pub fn enabled_plugin_count(&self) -> usize {
        self.plugins.iter().filter(|p| p.enabled).count()
    }

    /// Finds a plugin by name.
    pub fn find_plugin(&self, name: &str) -> Option<&PluginInfo> {
        self.plugins.iter().find(|p| p.name == name)
    }

    fn kb(action: &str, desc: &str, keys: &str, cat: KeybindingCategory) -> KeyBinding {
        KeyBinding {
            action: action.into(),
            description: desc.into(),
            keys: keys.into(),
            category: cat,
        }
    }

    fn default_keybindings() -> Vec<KeyBinding> {
        use KeybindingCategory::*;
        vec![
            Self::kb("delete", "Delete selected node", "Delete", Editing),
            Self::kb("rename", "Rename selected node", "F2", Editing),
            Self::kb("duplicate", "Duplicate selected node", "Ctrl+D", Editing),
            Self::kb("copy", "Copy selected node", "Ctrl+C", Editing),
            Self::kb("paste", "Paste node", "Ctrl+V", Editing),
            Self::kb("cut", "Cut selected node", "Ctrl+X", Editing),
            Self::kb("undo", "Undo last action", "Ctrl+Z", Editing),
            Self::kb("redo", "Redo last action", "Ctrl+Y", Editing),
            Self::kb("save", "Save scene", "Ctrl+S", Scene),
            Self::kb("zoom_in", "Zoom in", "Ctrl++", Navigation),
            Self::kb("zoom_out", "Zoom out", "Ctrl+-", Navigation),
            Self::kb("zoom_reset", "Reset zoom", "Ctrl+0", Navigation),
            Self::kb("tool_select", "Select tool", "Q", Tools),
            Self::kb("tool_move", "Move tool", "W", Tools),
            Self::kb("tool_rotate", "Rotate tool", "E", Tools),
            Self::kb("play", "Play scene", "F5", Playback),
            Self::kb("play_current", "Play current scene", "F6", Playback),
            Self::kb("pause", "Pause playback", "F7", Playback),
            Self::kb("stop", "Stop playback", "F8", Playback),
            Self::kb("help", "Show help", "F1", Other),
        ]
    }
}

impl Default for EditorSettingsDialog {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_plugin(name: &str, enabled: bool) -> PluginInfo {
        let mut p = PluginInfo::new(name, &format!("{name} plugin"));
        p.enabled = enabled;
        p
    }

    fn test_plugin_with_deps(name: &str, enabled: bool, deps: &[&str]) -> PluginInfo {
        let mut p = test_plugin(name, enabled);
        p.dependencies = deps.iter().map(|s| s.to_string()).collect();
        p
    }

    // ---- Tab tests ----

    #[test]
    fn tab_roundtrip() {
        for tab in SettingsTab::all() {
            let id = tab.id();
            let parsed = SettingsTab::from_id(id).unwrap();
            assert_eq!(*tab, parsed);
        }
    }

    #[test]
    fn tab_from_id_invalid() {
        assert!(SettingsTab::from_id("nonexistent").is_none());
    }

    // ---- Basic dialog tests ----

    #[test]
    fn new_dialog_has_default_keybindings() {
        let dialog = EditorSettingsDialog::new();
        assert!(dialog.keybinding_count() >= 15);
        assert!(dialog.effective_keys("undo").is_some());
        assert_eq!(dialog.effective_keys("undo"), Some("Ctrl+Z"));
    }

    #[test]
    fn open_close_clears_pending() {
        let mut dialog = EditorSettingsDialog::new();
        dialog.open();
        dialog.set_keybinding("undo", "Ctrl+Shift+Z");
        dialog.set_base_theme("light");
        assert!(dialog.has_pending_changes());
        dialog.close();
        assert!(!dialog.has_pending_changes());
    }

    #[test]
    fn search_filters_keybindings() {
        let mut dialog = EditorSettingsDialog::new();
        dialog.set_search("zoom");
        let filtered = dialog.filtered_keybindings();
        assert!(filtered.len() >= 2);
        for kb in &filtered {
            assert!(kb.action.contains("zoom") || kb.description.to_lowercase().contains("zoom"),);
        }
    }

    #[test]
    fn confirm_applies_keybinding_changes() {
        let mut dialog = EditorSettingsDialog::new();
        dialog.open();
        dialog.set_keybinding("undo", "Ctrl+Shift+Z");
        let (kb_changes, _, _) = dialog.confirm();
        assert_eq!(kb_changes.len(), 1);
        assert_eq!(dialog.effective_keys("undo"), Some("Ctrl+Shift+Z"));
    }

    #[test]
    fn plugin_toggle_and_confirm() {
        let mut dialog = EditorSettingsDialog::new();
        let mut p = PluginInfo::new("TestPlugin", "A test plugin");
        p.enabled = true;
        dialog.load_plugins(vec![p]);
        dialog.open();
        dialog.set_plugin_enabled("TestPlugin", false);
        assert!(dialog.has_pending_changes());
        let (_, plugin_changes, _) = dialog.confirm();
        assert_eq!(plugin_changes.len(), 1);
        assert_eq!(plugin_changes[0], ("TestPlugin".to_string(), false));
    }

    #[test]
    fn search_filters_plugins() {
        let mut dialog = EditorSettingsDialog::new();
        dialog.load_plugins(vec![
            test_plugin("Tilemap", true),
            test_plugin("Shader", false),
        ]);
        dialog.set_search("tile");
        let filtered = dialog.filtered_plugins();
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].name, "Tilemap");
    }

    #[test]
    fn effective_keys_prefers_pending() {
        let mut dialog = EditorSettingsDialog::new();
        dialog.open();
        dialog.set_keybinding("save", "Ctrl+Shift+S");
        assert_eq!(dialog.effective_keys("save"), Some("Ctrl+Shift+S"));
    }

    // ---- Theme tests ----

    #[test]
    fn theme_defaults() {
        let dialog = EditorSettingsDialog::new();
        let t = dialog.theme_config();
        assert_eq!(t.base_theme, "dark");
        assert_eq!(t.accent_color, AccentColor::Blue);
        assert_eq!(t.font_size, 14);
        assert_eq!(t.ui_scale, 100);
        assert!(t.custom_colors.is_empty());
    }

    #[test]
    fn theme_accent_colors() {
        assert_eq!(AccentColor::Blue.to_rgba(), (68, 138, 255, 255));
        assert_eq!(AccentColor::Green.to_rgba(), (76, 175, 80, 255));
        assert_eq!(AccentColor::Custom(10, 20, 30).to_rgba(), (10, 20, 30, 255));
    }

    #[test]
    fn set_base_theme_pending() {
        let mut dialog = EditorSettingsDialog::new();
        dialog.open();
        dialog.set_base_theme("light");
        assert!(dialog.has_pending_changes());
        assert_eq!(dialog.effective_theme().base_theme, "light");
        // Original unchanged
        assert_eq!(dialog.theme_config().base_theme, "dark");
    }

    #[test]
    fn confirm_applies_theme() {
        let mut dialog = EditorSettingsDialog::new();
        dialog.open();
        dialog.set_base_theme("light");
        dialog.set_accent_color(AccentColor::Red);
        dialog.set_font_size(20);
        let (_, _, theme_changed) = dialog.confirm();
        assert!(theme_changed);
        assert_eq!(dialog.theme_config().base_theme, "light");
        assert_eq!(dialog.theme_config().accent_color, AccentColor::Red);
        assert_eq!(dialog.theme_config().font_size, 20);
    }

    #[test]
    fn close_discards_theme_changes() {
        let mut dialog = EditorSettingsDialog::new();
        dialog.open();
        dialog.set_base_theme("light");
        dialog.close();
        assert_eq!(dialog.theme_config().base_theme, "dark");
    }

    #[test]
    fn font_size_clamped() {
        let mut dialog = EditorSettingsDialog::new();
        dialog.open();
        dialog.set_font_size(2); // below min
        assert_eq!(
            dialog.effective_theme().font_size,
            ThemeConfig::MIN_FONT_SIZE
        );
        dialog.set_font_size(200); // above max
        assert_eq!(
            dialog.effective_theme().font_size,
            ThemeConfig::MAX_FONT_SIZE
        );
    }

    #[test]
    fn ui_scale_clamped() {
        let mut dialog = EditorSettingsDialog::new();
        dialog.open();
        dialog.set_ui_scale(10);
        assert_eq!(dialog.effective_theme().ui_scale, ThemeConfig::MIN_UI_SCALE);
        dialog.set_ui_scale(500);
        assert_eq!(dialog.effective_theme().ui_scale, ThemeConfig::MAX_UI_SCALE);
    }

    #[test]
    fn custom_theme_colors() {
        let mut dialog = EditorSettingsDialog::new();
        dialog.open();
        dialog.set_theme_custom_color("background", (10, 20, 30, 255));
        let t = dialog.effective_theme();
        assert_eq!(t.color_for_role("background"), (10, 20, 30, 255));
        // Fallback for unset role
        assert_ne!(t.color_for_role("text"), (10, 20, 30, 255));
    }

    #[test]
    fn theme_color_for_role_dark_vs_light() {
        let mut t = ThemeConfig::default();
        let dark_bg = t.color_for_role("background");
        t.base_theme = "light".into();
        let light_bg = t.color_for_role("background");
        assert_ne!(dark_bg, light_bg);
    }

    #[test]
    fn reset_theme_to_defaults() {
        let mut dialog = EditorSettingsDialog::new();
        dialog.open();
        dialog.set_base_theme("light");
        dialog.set_font_size(30);
        dialog.reset_theme();
        let t = dialog.effective_theme();
        assert_eq!(t.base_theme, "dark");
        assert_eq!(t.font_size, 14);
    }

    #[test]
    fn remove_custom_color() {
        let mut t = ThemeConfig::default();
        t.set_custom_color("text", (1, 2, 3, 255));
        assert!(t.remove_custom_color("text"));
        assert!(!t.remove_custom_color("text"));
    }

    #[test]
    fn reset_all_custom_colors() {
        let mut t = ThemeConfig::default();
        t.set_custom_color("text", (1, 2, 3, 255));
        t.set_custom_color("background", (4, 5, 6, 255));
        t.reset_custom_colors();
        assert!(t.custom_colors.is_empty());
    }

    // ---- Keybinding category tests ----

    #[test]
    fn default_keybindings_have_categories() {
        let dialog = EditorSettingsDialog::new();
        let editing = dialog
            .filtered_keybindings()
            .iter()
            .filter(|kb| kb.category == KeybindingCategory::Editing)
            .count();
        assert!(editing >= 6); // delete, rename, duplicate, copy, paste, cut, undo, redo
    }

    #[test]
    fn filter_by_category() {
        let mut dialog = EditorSettingsDialog::new();
        dialog.set_keybinding_filter(Some(KeybindingCategory::Playback));
        let filtered = dialog.filtered_keybindings_by_category();
        assert!(filtered.len() >= 3); // play, play_current, pause, stop
        for kb in &filtered {
            assert_eq!(kb.category, KeybindingCategory::Playback);
        }
    }

    #[test]
    fn filter_by_category_and_search() {
        let mut dialog = EditorSettingsDialog::new();
        dialog.set_keybinding_filter(Some(KeybindingCategory::Editing));
        dialog.set_search("copy");
        let filtered = dialog.filtered_keybindings_by_category();
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].action, "copy");
    }

    #[test]
    fn keybinding_category_labels() {
        for cat in KeybindingCategory::all() {
            assert!(!cat.label().is_empty());
        }
    }

    // ---- Conflict detection ----

    #[test]
    fn no_conflicts_by_default() {
        let dialog = EditorSettingsDialog::new();
        assert!(dialog.detect_conflicts().is_empty());
    }

    #[test]
    fn detect_conflict_with_pending_change() {
        let mut dialog = EditorSettingsDialog::new();
        dialog.open();
        // Set "copy" to the same key as "undo" (Ctrl+Z)
        dialog.set_keybinding("copy", "Ctrl+Z");
        let conflicts = dialog.detect_conflicts();
        assert!(!conflicts.is_empty());
        let has_conflict = conflicts.iter().any(|c| {
            (c.action_a == "copy" || c.action_b == "copy")
                && (c.action_a == "undo" || c.action_b == "undo")
        });
        assert!(has_conflict);
    }

    // ---- Reset keybindings ----

    #[test]
    fn reset_single_keybinding() {
        let mut dialog = EditorSettingsDialog::new();
        dialog.open();
        dialog.set_keybinding("undo", "Alt+Z");
        assert_eq!(dialog.effective_keys("undo"), Some("Alt+Z"));
        dialog.reset_keybinding("undo");
        assert_eq!(dialog.effective_keys("undo"), Some("Ctrl+Z"));
    }

    #[test]
    fn reset_all_keybindings() {
        let mut dialog = EditorSettingsDialog::new();
        dialog.open();
        dialog.set_keybinding("undo", "Alt+Z");
        dialog.set_keybinding("save", "Alt+S");
        dialog.reset_all_keybindings();
        assert_eq!(dialog.effective_keys("undo"), Some("Ctrl+Z"));
        assert_eq!(dialog.effective_keys("save"), Some("Ctrl+S"));
    }

    // ---- Plugin management ----

    #[test]
    fn plugin_info_new() {
        let p = PluginInfo::new("MyPlugin", "Does things");
        assert_eq!(p.name, "MyPlugin");
        assert!(!p.enabled);
        assert_eq!(p.version, "1.0.0");
        assert!(p.author.is_empty());
        assert!(p.dependencies.is_empty());
    }

    #[test]
    fn find_plugin() {
        let mut dialog = EditorSettingsDialog::new();
        dialog.load_plugins(vec![test_plugin("Alpha", true), test_plugin("Beta", false)]);
        assert!(dialog.find_plugin("Alpha").is_some());
        assert!(dialog.find_plugin("Gamma").is_none());
    }

    #[test]
    fn enabled_plugin_count() {
        let mut dialog = EditorSettingsDialog::new();
        dialog.load_plugins(vec![
            test_plugin("A", true),
            test_plugin("B", false),
            test_plugin("C", true),
        ]);
        assert_eq!(dialog.enabled_plugin_count(), 2);
    }

    #[test]
    fn plugin_dependencies() {
        let mut dialog = EditorSettingsDialog::new();
        dialog.load_plugins(vec![
            test_plugin("Core", true),
            test_plugin_with_deps("Extension", true, &["Core"]),
            test_plugin_with_deps("Advanced", false, &["Core", "Extension"]),
        ]);

        // Dependents of Core
        let deps = dialog.dependents_of("Core");
        assert_eq!(deps.len(), 2);

        // Unmet dependencies of Advanced (Extension is enabled, Core is enabled)
        // Actually Advanced is disabled so unmet is checked against enabled plugins
        let unmet = dialog.unmet_dependencies("Advanced");
        assert!(unmet.is_empty()); // Core and Extension both exist and Core is enabled, Extension is enabled

        // Would break dependents if we disable Core
        let broken = dialog.would_break_dependents("Core");
        assert!(broken.contains(&"Extension".to_string()));
    }

    #[test]
    fn unmet_dependencies_when_dep_disabled() {
        let mut dialog = EditorSettingsDialog::new();
        dialog.load_plugins(vec![
            test_plugin("Core", false), // disabled!
            test_plugin_with_deps("Extension", true, &["Core"]),
        ]);
        let unmet = dialog.unmet_dependencies("Extension");
        assert_eq!(unmet, vec!["Core".to_string()]);
    }

    #[test]
    fn unmet_dependencies_nonexistent_plugin() {
        let dialog = EditorSettingsDialog::new();
        let unmet = dialog.unmet_dependencies("NonExistent");
        assert!(unmet.is_empty());
    }

    #[test]
    fn confirm_no_theme_change_returns_false() {
        let mut dialog = EditorSettingsDialog::new();
        dialog.open();
        dialog.set_keybinding("undo", "Alt+Z");
        let (_, _, theme_changed) = dialog.confirm();
        assert!(!theme_changed);
    }

    #[test]
    fn load_keybindings_replaces_defaults() {
        let mut dialog = EditorSettingsDialog::new();
        dialog.load_keybindings(vec![EditorSettingsDialog::kb(
            "custom",
            "Custom action",
            "F12",
            KeybindingCategory::Other,
        )]);
        assert_eq!(dialog.keybinding_count(), 1);
        assert_eq!(dialog.effective_keys("custom"), Some("F12"));
        assert_eq!(dialog.effective_keys("undo"), None);
    }

    #[test]
    fn dialog_default_tab_is_general() {
        let dialog = EditorSettingsDialog::new();
        assert_eq!(dialog.active_tab(), SettingsTab::General);
    }

    #[test]
    fn set_tab_and_get() {
        let mut dialog = EditorSettingsDialog::new();
        dialog.set_tab(SettingsTab::Theme);
        assert_eq!(dialog.active_tab(), SettingsTab::Theme);
    }
}
