//! Editor settings dialog with categorized tabs for General, Theme,
//! Keybindings, and Plugins.
//!
//! Provides a headless model for an editor settings dialog where
//! configuration is organized into tabs.

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

/// A keyboard shortcut binding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyBinding {
    /// The action name (e.g. "delete", "duplicate", "undo").
    pub action: String,
    /// Human-readable description.
    pub description: String,
    /// The key combination string (e.g. "Ctrl+Z", "F2", "Delete").
    pub keys: String,
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
        }
    }

    /// Opens the dialog.
    pub fn open(&mut self) {
        self.visible = true;
        self.pending_keybindings.clear();
        self.pending_plugins.clear();
        self.search_text.clear();
    }

    /// Closes the dialog.
    pub fn close(&mut self) {
        self.visible = false;
        self.pending_keybindings.clear();
        self.pending_plugins.clear();
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
        !self.pending_keybindings.is_empty() || !self.pending_plugins.is_empty()
    }

    /// Applies pending changes and returns them.
    pub fn confirm(&mut self) -> (Vec<(String, String)>, Vec<(String, bool)>) {
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
        self.visible = false;
        (kb_changes, plugin_changes)
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

    fn default_keybindings() -> Vec<KeyBinding> {
        vec![
            KeyBinding {
                action: "delete".into(),
                description: "Delete selected node".into(),
                keys: "Delete".into(),
            },
            KeyBinding {
                action: "rename".into(),
                description: "Rename selected node".into(),
                keys: "F2".into(),
            },
            KeyBinding {
                action: "duplicate".into(),
                description: "Duplicate selected node".into(),
                keys: "Ctrl+D".into(),
            },
            KeyBinding {
                action: "copy".into(),
                description: "Copy selected node".into(),
                keys: "Ctrl+C".into(),
            },
            KeyBinding {
                action: "paste".into(),
                description: "Paste node".into(),
                keys: "Ctrl+V".into(),
            },
            KeyBinding {
                action: "cut".into(),
                description: "Cut selected node".into(),
                keys: "Ctrl+X".into(),
            },
            KeyBinding {
                action: "undo".into(),
                description: "Undo last action".into(),
                keys: "Ctrl+Z".into(),
            },
            KeyBinding {
                action: "redo".into(),
                description: "Redo last action".into(),
                keys: "Ctrl+Y".into(),
            },
            KeyBinding {
                action: "save".into(),
                description: "Save scene".into(),
                keys: "Ctrl+S".into(),
            },
            KeyBinding {
                action: "zoom_in".into(),
                description: "Zoom in".into(),
                keys: "Ctrl++".into(),
            },
            KeyBinding {
                action: "zoom_out".into(),
                description: "Zoom out".into(),
                keys: "Ctrl+-".into(),
            },
            KeyBinding {
                action: "zoom_reset".into(),
                description: "Reset zoom".into(),
                keys: "Ctrl+0".into(),
            },
            KeyBinding {
                action: "tool_select".into(),
                description: "Select tool".into(),
                keys: "Q".into(),
            },
            KeyBinding {
                action: "tool_move".into(),
                description: "Move tool".into(),
                keys: "W".into(),
            },
            KeyBinding {
                action: "tool_rotate".into(),
                description: "Rotate tool".into(),
                keys: "E".into(),
            },
            KeyBinding {
                action: "play".into(),
                description: "Play scene".into(),
                keys: "F5".into(),
            },
            KeyBinding {
                action: "play_current".into(),
                description: "Play current scene".into(),
                keys: "F6".into(),
            },
            KeyBinding {
                action: "pause".into(),
                description: "Pause playback".into(),
                keys: "F7".into(),
            },
            KeyBinding {
                action: "stop".into(),
                description: "Stop playback".into(),
                keys: "F8".into(),
            },
            KeyBinding {
                action: "help".into(),
                description: "Show help".into(),
                keys: "F1".into(),
            },
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

    #[test]
    fn tab_roundtrip() {
        for tab in SettingsTab::all() {
            let id = tab.id();
            let parsed = SettingsTab::from_id(id).unwrap();
            assert_eq!(*tab, parsed);
        }
    }

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
            assert!(
                kb.action.contains("zoom") || kb.description.to_lowercase().contains("zoom"),
            );
        }
    }

    #[test]
    fn confirm_applies_keybinding_changes() {
        let mut dialog = EditorSettingsDialog::new();
        dialog.open();
        dialog.set_keybinding("undo", "Ctrl+Shift+Z");
        let (kb_changes, _) = dialog.confirm();
        assert_eq!(kb_changes.len(), 1);
        assert_eq!(dialog.effective_keys("undo"), Some("Ctrl+Shift+Z"));
    }

    #[test]
    fn plugin_toggle_and_confirm() {
        let mut dialog = EditorSettingsDialog::new();
        dialog.load_plugins(vec![
            PluginInfo {
                name: "TestPlugin".into(),
                enabled: true,
                description: "A test plugin".into(),
            },
        ]);
        dialog.open();
        dialog.set_plugin_enabled("TestPlugin", false);
        assert!(dialog.has_pending_changes());
        let (_, plugin_changes) = dialog.confirm();
        assert_eq!(plugin_changes.len(), 1);
        assert_eq!(plugin_changes[0], ("TestPlugin".to_string(), false));
    }

    #[test]
    fn search_filters_plugins() {
        let mut dialog = EditorSettingsDialog::new();
        dialog.load_plugins(vec![
            PluginInfo { name: "Tilemap".into(), enabled: true, description: "Tile editor".into() },
            PluginInfo { name: "Shader".into(), enabled: false, description: "Shader preview".into() },
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
}
