//! **Autoload / singleton management** (pat-8ptny).
//!
//! Manages the project's autoloads (global singletons): add, rename, reorder,
//! enable/disable, and remove them. The ordered list is the load order, enabled
//! autoloads are registered as global singletons, and the whole list persists to
//! the `[autoload]` section of project settings (modeled as a serialized
//! round-trip).

use serde::{Deserialize, Serialize};

/// A single autoload entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Autoload {
    /// The global singleton name.
    pub name: String,
    /// The script/scene path it loads.
    pub path: String,
    /// Whether it's enabled (registered as a global).
    pub enabled: bool,
}

/// Manages the ordered list of autoloads.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AutoloadManager {
    autoloads: Vec<Autoload>,
}

impl AutoloadManager {
    /// Creates an empty manager.
    pub fn new() -> Self {
        Self::default()
    }

    /// The autoload names in load order.
    pub fn names(&self) -> Vec<&str> {
        self.autoloads.iter().map(|a| a.name.as_str()).collect()
    }

    /// The number of autoloads.
    pub fn len(&self) -> usize {
        self.autoloads.len()
    }

    /// Whether there are no autoloads.
    pub fn is_empty(&self) -> bool {
        self.autoloads.is_empty()
    }

    /// Whether an autoload named `name` exists.
    pub fn contains(&self, name: &str) -> bool {
        self.autoloads.iter().any(|a| a.name == name)
    }

    /// The autoload named `name`, if present.
    pub fn get(&self, name: &str) -> Option<&Autoload> {
        self.autoloads.iter().find(|a| a.name == name)
    }

    fn index_of(&self, name: &str) -> Option<usize> {
        self.autoloads.iter().position(|a| a.name == name)
    }

    /// Adds an enabled autoload, registering a global singleton. Returns false
    /// if the name is already taken.
    pub fn add(&mut self, name: impl Into<String>, path: impl Into<String>) -> bool {
        let name = name.into();
        if self.contains(&name) {
            return false;
        }
        self.autoloads.push(Autoload {
            name,
            path: path.into(),
            enabled: true,
        });
        true
    }

    /// Removes the autoload `name`, clearing its global. Returns whether it
    /// existed.
    pub fn remove(&mut self, name: &str) -> bool {
        match self.index_of(name) {
            Some(i) => {
                self.autoloads.remove(i);
                true
            }
            None => false,
        }
    }

    /// Renames `old` to `new`, preserving its position. Returns false if `old`
    /// is missing or `new` is taken.
    pub fn rename(&mut self, old: &str, new: &str) -> bool {
        if !self.contains(old) || self.contains(new) {
            return false;
        }
        if let Some(a) = self.autoloads.iter_mut().find(|a| a.name == old) {
            a.name = new.to_string();
        }
        true
    }

    /// Enables or disables `name`. Returns whether it exists.
    pub fn set_enabled(&mut self, name: &str, enabled: bool) -> bool {
        match self.autoloads.iter_mut().find(|a| a.name == name) {
            Some(a) => {
                a.enabled = enabled;
                true
            }
            None => false,
        }
    }

    /// Moves `name` one position earlier in the load order. Returns whether it
    /// moved.
    pub fn move_up(&mut self, name: &str) -> bool {
        match self.index_of(name) {
            Some(i) if i > 0 => {
                self.autoloads.swap(i, i - 1);
                true
            }
            _ => false,
        }
    }

    /// Moves `name` one position later in the load order. Returns whether it
    /// moved.
    pub fn move_down(&mut self, name: &str) -> bool {
        match self.index_of(name) {
            Some(i) if i + 1 < self.autoloads.len() => {
                self.autoloads.swap(i, i + 1);
                true
            }
            _ => false,
        }
    }

    /// Whether `name` is currently registered as a global (exists and enabled).
    pub fn is_global(&self, name: &str) -> bool {
        self.get(name).is_some_and(|a| a.enabled)
    }

    /// The registered global singletons, in load order (enabled autoloads).
    pub fn globals(&self) -> Vec<&str> {
        self.autoloads
            .iter()
            .filter(|a| a.enabled)
            .map(|a| a.name.as_str())
            .collect()
    }

    /// Serializes the `[autoload]` section to project-settings text.
    pub fn to_cfg(&self) -> String {
        serde_json::to_string_pretty(self).expect("AutoloadManager serializes")
    }

    /// Loads the `[autoload]` section from project-settings text.
    pub fn from_cfg(cfg: &str) -> serde_json::Result<Self> {
        serde_json::from_str(cfg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Acceptance (pat-8ptny): adding an autoload registers a global singleton,
    /// reordering changes load order, and removal clears it.
    #[test]
    fn systems_autoloads_register_globals() {
        let mut mgr = AutoloadManager::new();

        // Adding registers global singletons in load order.
        assert!(mgr.add("GameState", "res://game_state.gd"));
        assert!(mgr.add("Audio", "res://audio.gd"));
        assert!(!mgr.add("GameState", "res://dup.gd")); // duplicate name
        assert_eq!(mgr.names(), vec!["GameState", "Audio"]);
        assert!(mgr.is_global("GameState"));
        assert_eq!(mgr.globals(), vec!["GameState", "Audio"]);

        // Disabling removes it from the globals but keeps the entry.
        assert!(mgr.set_enabled("Audio", false));
        assert_eq!(mgr.globals(), vec!["GameState"]);
        assert!(mgr.contains("Audio"));
        assert!(!mgr.is_global("Audio"));

        // Reordering changes the load order.
        assert!(mgr.move_down("GameState"));
        assert_eq!(mgr.names(), vec!["Audio", "GameState"]);
        assert!(mgr.move_up("GameState"));
        assert_eq!(mgr.names(), vec!["GameState", "Audio"]);
        // Can't move past the ends.
        assert!(!mgr.move_up("GameState"));
        assert!(!mgr.move_down("Audio"));

        // Rename preserves position and path.
        assert!(mgr.rename("Audio", "Sound"));
        assert!(mgr.contains("Sound"));
        assert!(!mgr.contains("Audio"));
        assert_eq!(mgr.get("Sound").unwrap().path, "res://audio.gd");
        assert!(!mgr.rename("Sound", "GameState")); // target taken

        // Removal clears the autoload and its global.
        assert!(mgr.remove("GameState"));
        assert!(!mgr.contains("GameState"));
        assert!(!mgr.is_global("GameState"));
        assert_eq!(mgr.names(), vec!["Sound"]);

        // The [autoload] section persists and reloads identically.
        let cfg = mgr.to_cfg();
        let loaded = AutoloadManager::from_cfg(&cfg).expect("loads");
        assert_eq!(loaded.names(), vec!["Sound"]);
        assert_eq!(loaded.get("Sound").unwrap().path, "res://audio.gd");
        assert!(!loaded.get("Sound").unwrap().enabled);
    }
}
