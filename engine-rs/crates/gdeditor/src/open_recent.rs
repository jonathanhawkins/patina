//! Scene > Open Recent.
//!
//! A bounded, de-duplicated, most-recent-first list of opened scene paths.
//! Opening a scene records it at the front; selecting an existing entry opens
//! it and moves it back to the front. The whole list serializes to JSON so the
//! Open Recent menu survives across editor sessions, mirroring Godot.

use serde::{Deserialize, Serialize};

/// The Scene > Open Recent menu model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenRecent {
    /// Scene paths, most-recent-first.
    entries: Vec<String>,
    /// Maximum number of remembered entries.
    max_entries: usize,
}

impl Default for OpenRecent {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
            max_entries: 20,
        }
    }
}

impl OpenRecent {
    /// Creates an empty Open Recent list bounded to `max_entries` (min 1).
    pub fn new(max_entries: usize) -> Self {
        Self {
            entries: Vec::new(),
            max_entries: max_entries.max(1),
        }
    }

    /// Records that `path` was opened: it moves to the front (most-recent),
    /// de-duplicated, and the list is truncated to `max_entries` (oldest
    /// entries dropped).
    pub fn record_open(&mut self, path: impl Into<String>) {
        let path = path.into();
        self.entries.retain(|p| p != &path);
        self.entries.insert(0, path);
        if self.entries.len() > self.max_entries {
            self.entries.truncate(self.max_entries);
        }
    }

    /// The recent scene paths, most-recent-first — what Open Recent renders.
    pub fn entries(&self) -> &[String] {
        &self.entries
    }

    /// Number of remembered entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether Open Recent is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The maximum number of remembered entries.
    pub fn max_entries(&self) -> usize {
        self.max_entries
    }

    /// Selects the Open Recent entry at `index`, opening that scene: returns its
    /// path and moves it to the front (most-recent). `None` if out of range.
    pub fn select(&mut self, index: usize) -> Option<String> {
        let path = self.entries.get(index)?.clone();
        self.record_open(path.clone());
        Some(path)
    }

    /// Clears the Open Recent list.
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// Serializes to JSON for persistence across sessions.
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| "{}".to_string())
    }

    /// Restores from previously persisted JSON, or `None` if invalid.
    pub fn from_json(json: &str) -> Option<Self> {
        serde_json::from_str(json).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    /// Acceptance (pat-0gmlw): opening scenes populates Open Recent
    /// most-recent-first, selecting an entry opens it, and the list persists
    /// after reload.
    #[test]
    fn menus_open_recent_tracks_and_opens() {
        let mut recent = OpenRecent::new(20);
        assert!(recent.is_empty(), "empty before any scene is opened");

        // Opening scenes populates Open Recent most-recent-first.
        recent.record_open("res://Main.tscn");
        recent.record_open("res://Player.tscn");
        recent.record_open("res://Enemy.tscn");
        assert_eq!(
            recent.entries(),
            paths(&["res://Enemy.tscn", "res://Player.tscn", "res://Main.tscn"]).as_slice(),
            "most-recent-first ordering"
        );

        // Re-opening an existing scene moves it to the front (de-duplicated).
        recent.record_open("res://Main.tscn");
        assert_eq!(
            recent.entries(),
            paths(&["res://Main.tscn", "res://Enemy.tscn", "res://Player.tscn"]).as_slice(),
            "re-open moves to front without duplicating"
        );
        assert_eq!(recent.len(), 3);

        // Selecting an entry opens it and moves it to the front.
        let opened = recent.select(2).expect("entry at index 2 exists");
        assert_eq!(opened, "res://Player.tscn", "select returns the opened path");
        assert_eq!(
            recent.entries()[0],
            "res://Player.tscn",
            "selected entry becomes most-recent"
        );
        // Out-of-range selection is a no-op returning None.
        assert!(recent.select(99).is_none());

        // The list persists across a reload (save → load).
        let json = recent.to_json();
        let reloaded = OpenRecent::from_json(&json).expect("persisted list reloads");
        assert_eq!(
            reloaded.entries(),
            recent.entries(),
            "Open Recent survives a save/reload"
        );

        // The bound drops the oldest entries.
        let mut bounded = OpenRecent::new(2);
        bounded.record_open("a");
        bounded.record_open("b");
        bounded.record_open("c");
        assert_eq!(
            bounded.entries(),
            paths(&["c", "b"]).as_slice(),
            "oldest entry dropped at capacity"
        );
    }
}
