//! Project Settings store (pat-2s4n4).
//!
//! Backs the Project Settings dialog: built-in default values plus user
//! overrides. The user can change a setting (add an override), add a brand-new
//! custom setting, or revert one back to its default. Only the overrides are
//! written to `project.godot`; on reopen the engine supplies the defaults and
//! the file supplies the overrides.

use std::collections::BTreeMap;

/// The project's settings: built-in defaults and user overrides.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProjectSettings {
    defaults: BTreeMap<String, String>,
    overrides: BTreeMap<String, String>,
}

impl ProjectSettings {
    /// Creates an empty settings store.
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a built-in default value for `key`.
    pub fn set_default(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.defaults.insert(key.into(), value.into());
    }

    /// The effective value of `key` (override if present, else default).
    pub fn get(&self, key: &str) -> Option<String> {
        self.overrides
            .get(key)
            .or_else(|| self.defaults.get(key))
            .cloned()
    }

    /// Whether `key` has a user override (differs from / shadows the default).
    pub fn has_override(&self, key: &str) -> bool {
        self.overrides.contains_key(key)
    }

    /// Whether `key`'s effective value is its default (no override).
    pub fn is_default(&self, key: &str) -> bool {
        !self.overrides.contains_key(key)
    }

    /// Changes a setting, recording an override.
    pub fn set(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.overrides.insert(key.into(), value.into());
    }

    /// Adds a brand-new custom setting (recorded as an override). Equivalent to
    /// `set`, named for the dialog's "Add" action.
    pub fn add(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.set(key, value);
    }

    /// Reverts `key` to its default by removing the override. Returns whether an
    /// override was removed.
    pub fn revert(&mut self, key: &str) -> bool {
        self.overrides.remove(key).is_some()
    }

    /// The overrides (the part persisted to `project.godot`).
    pub fn overrides(&self) -> &BTreeMap<String, String> {
        &self.overrides
    }

    /// Serializes the overrides to `project.godot` contents.
    pub fn to_godot(&self) -> String {
        serde_json::to_string_pretty(&self.overrides).expect("overrides serialize")
    }

    /// Reloads a settings store: built-in `defaults` plus the overrides parsed
    /// from `project.godot` contents.
    pub fn from_godot(
        defaults: BTreeMap<String, String>,
        cfg: &str,
    ) -> serde_json::Result<Self> {
        let overrides: BTreeMap<String, String> = serde_json::from_str(cfg)?;
        Ok(Self { defaults, overrides })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn defaults() -> BTreeMap<String, String> {
        let mut d = BTreeMap::new();
        d.insert("application/config/name".to_string(), "Untitled".to_string());
        d.insert("display/window/size/width".to_string(), "1152".to_string());
        d
    }

    /// Acceptance (pat-2s4n4): changing, adding, and reverting a project setting
    /// persists to `project.godot` and reloads on reopen.
    #[test]
    fn systems_project_settings_persist() {
        let mut ps = ProjectSettings::new();
        ps.set_default("application/config/name", "Untitled");
        ps.set_default("display/window/size/width", "1152");

        // Defaults are returned until overridden.
        assert_eq!(ps.get("application/config/name").as_deref(), Some("Untitled"));
        assert!(ps.is_default("application/config/name"));
        assert!(!ps.has_override("application/config/name"));

        // Change a setting (records an override).
        ps.set("application/config/name", "My Game");
        assert_eq!(ps.get("application/config/name").as_deref(), Some("My Game"));
        assert!(ps.has_override("application/config/name"));

        // Add a brand-new custom setting.
        ps.add("my/custom/flag", "true");
        assert_eq!(ps.get("my/custom/flag").as_deref(), Some("true"));

        // Persist: only overrides are written; default-only keys are not.
        let cfg = ps.to_godot();
        assert!(cfg.contains("application/config/name"));
        assert!(cfg.contains("My Game"));
        assert!(cfg.contains("my/custom/flag"));
        assert!(!cfg.contains("display/window/size/width"));

        // Reopen: defaults from the engine, overrides from the file.
        let mut reloaded = ProjectSettings::from_godot(defaults(), &cfg).expect("loads");
        assert_eq!(reloaded.get("application/config/name").as_deref(), Some("My Game"));
        assert_eq!(reloaded.get("display/window/size/width").as_deref(), Some("1152"));
        assert_eq!(reloaded.get("my/custom/flag").as_deref(), Some("true"));

        // Reverting a built-in setting restores its default.
        assert!(reloaded.revert("application/config/name"));
        assert_eq!(reloaded.get("application/config/name").as_deref(), Some("Untitled"));
        assert!(!reloaded.has_override("application/config/name"));

        // Reverting a custom setting (no default) removes it entirely.
        assert!(reloaded.revert("my/custom/flag"));
        assert!(reloaded.get("my/custom/flag").is_none());

        // Reverting a setting with no override is a no-op.
        assert!(!reloaded.revert("display/window/size/width"));
    }
}
