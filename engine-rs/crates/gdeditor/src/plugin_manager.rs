//! **Editor plugin management** (pat-tofrg).
//!
//! Manages editor plugins that can be enabled or disabled from Project
//! Settings. Each installed plugin declares the editor contributions it
//! registers (docks, menu items, toolbars…). Enabling a plugin loads it and
//! registers its contributions; disabling unloads it and removes them. The
//! enabled state persists to project settings.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// An installed editor plugin.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Plugin {
    /// The contributions this plugin registers when enabled.
    pub contributions: Vec<String>,
    /// Whether it's currently enabled (loaded).
    pub enabled: bool,
}

/// Manages installed plugins and their enabled state.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PluginManager {
    plugins: BTreeMap<String, Plugin>,
}

impl PluginManager {
    /// Creates an empty plugin manager.
    pub fn new() -> Self {
        Self::default()
    }

    /// Installs a plugin (making it available, disabled) with the contributions
    /// it would register. Returns false if it's already installed.
    pub fn install(&mut self, name: impl Into<String>, contributions: &[&str]) -> bool {
        let name = name.into();
        if self.plugins.contains_key(&name) {
            return false;
        }
        self.plugins.insert(
            name,
            Plugin {
                contributions: contributions.iter().map(|s| s.to_string()).collect(),
                enabled: false,
            },
        );
        true
    }

    /// Whether a plugin named `name` is installed.
    pub fn is_installed(&self, name: &str) -> bool {
        self.plugins.contains_key(name)
    }

    /// Whether a plugin is enabled (loaded).
    pub fn is_enabled(&self, name: &str) -> bool {
        self.plugins.get(name).is_some_and(|p| p.enabled)
    }

    /// The contributions a plugin declares.
    pub fn contributions_of(&self, name: &str) -> Option<&[String]> {
        self.plugins.get(name).map(|p| p.contributions.as_slice())
    }

    /// Enables (loads) a plugin, registering its contributions. Returns false if
    /// it's not installed or already enabled.
    pub fn enable(&mut self, name: &str) -> bool {
        match self.plugins.get_mut(name) {
            Some(p) if !p.enabled => {
                p.enabled = true;
                true
            }
            _ => false,
        }
    }

    /// Disables (unloads) a plugin, removing its contributions. Returns false if
    /// it's not installed or already disabled.
    pub fn disable(&mut self, name: &str) -> bool {
        match self.plugins.get_mut(name) {
            Some(p) if p.enabled => {
                p.enabled = false;
                true
            }
            _ => false,
        }
    }

    /// The names of enabled plugins, sorted.
    pub fn enabled_plugins(&self) -> Vec<&str> {
        self.plugins
            .iter()
            .filter(|(_, p)| p.enabled)
            .map(|(n, _)| n.as_str())
            .collect()
    }

    /// All currently-registered editor contributions, from enabled plugins (in
    /// plugin-name order).
    pub fn active_contributions(&self) -> Vec<String> {
        self.plugins
            .values()
            .filter(|p| p.enabled)
            .flat_map(|p| p.contributions.iter().cloned())
            .collect()
    }

    /// Serializes the plugin state to project-settings text.
    pub fn to_cfg(&self) -> String {
        serde_json::to_string_pretty(self).expect("PluginManager serializes")
    }

    /// Loads the plugin state from project-settings text.
    pub fn from_cfg(cfg: &str) -> serde_json::Result<Self> {
        serde_json::from_str(cfg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Acceptance (pat-tofrg): enabling a plugin loads it and registers its
    /// editor contributions; disabling unloads it.
    #[test]
    fn systems_plugin_enable_disable() {
        let mut mgr = PluginManager::new();
        mgr.install("GridMap", &["GridMap dock", "GridMap menu"]);
        mgr.install("CSG", &["CSG toolbar"]);

        // Installed but not yet enabled — no contributions registered.
        assert!(mgr.is_installed("GridMap"));
        assert!(!mgr.is_enabled("GridMap"));
        assert!(mgr.active_contributions().is_empty());
        // Re-installing is rejected.
        assert!(!mgr.install("GridMap", &["x"]));

        // Enabling loads the plugin and registers its contributions.
        assert!(mgr.enable("GridMap"));
        assert!(mgr.is_enabled("GridMap"));
        assert_eq!(
            mgr.active_contributions(),
            vec!["GridMap dock", "GridMap menu"]
        );
        // Enabling again is a no-op.
        assert!(!mgr.enable("GridMap"));

        // Enable a second plugin — contributions accumulate (plugin-name order).
        assert!(mgr.enable("CSG"));
        assert_eq!(
            mgr.active_contributions(),
            vec!["CSG toolbar", "GridMap dock", "GridMap menu"]
        );
        assert_eq!(mgr.enabled_plugins(), vec!["CSG", "GridMap"]);

        // Disabling unloads the plugin, removing its contributions.
        assert!(mgr.disable("GridMap"));
        assert!(!mgr.is_enabled("GridMap"));
        assert_eq!(mgr.active_contributions(), vec!["CSG toolbar"]);
        // Disabling again is a no-op.
        assert!(!mgr.disable("GridMap"));

        // Unknown plugins can't be toggled.
        assert!(!mgr.enable("Ghost"));
        assert!(!mgr.disable("Ghost"));

        // Enabled state persists to and from project settings.
        let cfg = mgr.to_cfg();
        let loaded = PluginManager::from_cfg(&cfg).expect("loads");
        assert!(loaded.is_enabled("CSG"));
        assert!(!loaded.is_enabled("GridMap"));
        assert_eq!(loaded.active_contributions(), vec!["CSG toolbar"]);
    }
}
