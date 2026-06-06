//! Editor Settings store (pat-cb2es).
//!
//! Backs the Editor Settings dialog: interface and text-editor preferences plus
//! keyboard shortcuts. Changing a setting or rebinding a shortcut takes effect
//! immediately (the store's value is the effective value) and persists to the
//! editor settings store so it survives editor restarts.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// The editor's settings and shortcut bindings.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EditorSettings {
    settings: BTreeMap<String, String>,
    shortcuts: BTreeMap<String, String>,
}

impl EditorSettings {
    /// Creates an empty settings store.
    pub fn new() -> Self {
        Self::default()
    }

    /// The effective value of a setting (e.g. `"interface/theme/preset"`).
    pub fn get(&self, key: &str) -> Option<String> {
        self.settings.get(key).cloned()
    }

    /// Sets a setting; the new value applies immediately.
    pub fn set(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.settings.insert(key.into(), value.into());
    }

    /// Resets (removes) a setting. Returns whether it existed.
    pub fn reset(&mut self, key: &str) -> bool {
        self.settings.remove(key).is_some()
    }

    /// The binding for a shortcut action (e.g. `"editor/save_scene"`).
    pub fn shortcut(&self, action: &str) -> Option<&str> {
        self.shortcuts.get(action).map(String::as_str)
    }

    /// Binds (or rebinds) a shortcut; applies immediately.
    pub fn set_shortcut(&mut self, action: impl Into<String>, binding: impl Into<String>) {
        self.shortcuts.insert(action.into(), binding.into());
    }

    /// Clears a shortcut binding. Returns whether it existed.
    pub fn clear_shortcut(&mut self, action: &str) -> bool {
        self.shortcuts.remove(action).is_some()
    }

    /// All setting keys, sorted.
    pub fn keys(&self) -> Vec<&str> {
        self.settings.keys().map(String::as_str).collect()
    }

    /// Serializes to the editor settings store contents.
    pub fn to_store(&self) -> String {
        serde_json::to_string_pretty(self).expect("EditorSettings serializes")
    }

    /// Loads from the editor settings store contents (on editor restart).
    pub fn from_store(store: &str) -> serde_json::Result<Self> {
        serde_json::from_str(store)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Acceptance (pat-cb2es): changing an editor setting and a shortcut
    /// persists across editor restarts and applies immediately.
    #[test]
    fn systems_editor_settings_persist_and_apply() {
        let mut es = EditorSettings::new();

        // Changing interface and text-editor settings applies immediately.
        es.set("interface/theme/preset", "Dark");
        es.set("text_editor/appearance/font_size", "16");
        assert_eq!(es.get("interface/theme/preset").as_deref(), Some("Dark"));
        assert_eq!(es.get("text_editor/appearance/font_size").as_deref(), Some("16"));

        // Binding and rebinding a shortcut applies immediately.
        es.set_shortcut("editor/save_scene", "Ctrl+S");
        assert_eq!(es.shortcut("editor/save_scene"), Some("Ctrl+S"));
        es.set_shortcut("editor/save_scene", "Cmd+S");
        assert_eq!(es.shortcut("editor/save_scene"), Some("Cmd+S"));

        // Persisting and reloading (as on restart) keeps everything.
        let store = es.to_store();
        let reloaded = EditorSettings::from_store(&store).expect("loads");
        assert_eq!(reloaded.get("interface/theme/preset").as_deref(), Some("Dark"));
        assert_eq!(
            reloaded.get("text_editor/appearance/font_size").as_deref(),
            Some("16")
        );
        assert_eq!(reloaded.shortcut("editor/save_scene"), Some("Cmd+S"));
        assert_eq!(reloaded, es);

        // Resetting a setting and clearing a shortcut remove them.
        assert!(es.reset("interface/theme/preset"));
        assert!(es.get("interface/theme/preset").is_none());
        assert!(!es.reset("interface/theme/preset")); // already gone

        assert!(es.clear_shortcut("editor/save_scene"));
        assert!(es.shortcut("editor/save_scene").is_none());
        assert!(!es.clear_shortcut("editor/save_scene"));

        // The remaining setting is still present and listed.
        assert_eq!(es.keys(), vec!["text_editor/appearance/font_size"]);
    }
}
