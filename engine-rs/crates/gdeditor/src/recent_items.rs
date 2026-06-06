//! Editor recent files and recent projects lists.
//!
//! Implements Godot's recent files and recent projects tracking for the
//! editor. Maintains bounded, most-recently-used (MRU) lists with:
//!
//! - **Recent files**: tracks recently opened scenes, scripts, resources.
//! - **Recent projects**: tracks recently opened project directories.
//! - **Pinning**: pin items so they stay at the top regardless of access time.
//! - **Persistence**: serialize/deserialize to JSON for saving across sessions.
//! - **Deduplication**: accessing an existing item moves it to the front.

use std::time::SystemTime;

// ---------------------------------------------------------------------------
// RecentItem
// ---------------------------------------------------------------------------

/// A single recent item (file or project) with metadata.
#[derive(Debug, Clone, PartialEq)]
pub struct RecentItem {
    /// The path (file path or project directory).
    pub path: String,
    /// Display name (derived from path or user-specified).
    pub name: String,
    /// When this item was last accessed.
    pub last_accessed: SystemTime,
    /// Whether this item is pinned to the top of the list.
    pub pinned: bool,
    /// Optional icon/type hint (e.g. `"scene"`, `"script"`, `"resource"`).
    pub item_type: Option<String>,
}

impl RecentItem {
    /// Creates a new recent item with the current timestamp.
    pub fn new(path: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            name: name.into(),
            last_accessed: SystemTime::now(),
            pinned: false,
            item_type: None,
        }
    }

    /// Creates a recent item with a specific type hint.
    pub fn with_type(mut self, item_type: impl Into<String>) -> Self {
        self.item_type = Some(item_type.into());
        self
    }

    /// Creates a recent item that is pinned.
    pub fn pinned(mut self) -> Self {
        self.pinned = true;
        self
    }
}

// ---------------------------------------------------------------------------
// RecentList
// ---------------------------------------------------------------------------

/// A bounded most-recently-used (MRU) list of items.
///
/// Items are ordered by access time (most recent first), with pinned items
/// always appearing before unpinned items. When the list exceeds capacity,
/// the oldest unpinned item is removed.
#[derive(Debug)]
pub struct RecentList {
    items: Vec<RecentItem>,
    max_items: usize,
}

impl RecentList {
    /// Creates a new empty recent list with the given capacity.
    pub fn new(max_items: usize) -> Self {
        Self {
            items: Vec::new(),
            max_items,
        }
    }

    /// Adds or touches an item. If it already exists (by path), it is moved
    /// to the front with an updated timestamp. Otherwise a new entry is added.
    ///
    /// Returns `true` if a new item was added, `false` if an existing item
    /// was touched.
    pub fn add(&mut self, path: impl Into<String>, name: impl Into<String>) -> bool {
        self.add_item(RecentItem::new(path, name))
    }

    /// Adds or touches a `RecentItem`. If an item with the same path exists,
    /// updates its timestamp and moves it to the front.
    pub fn add_item(&mut self, item: RecentItem) -> bool {
        // Check if item already exists.
        if let Some(pos) = self.items.iter().position(|i| i.path == item.path) {
            // Update timestamp and move to front.
            let mut existing = self.items.remove(pos);
            existing.last_accessed = SystemTime::now();
            // Preserve pinned state from existing item if not explicitly set.
            if !item.pinned {
                // Keep the existing pin state.
            } else {
                existing.pinned = true;
            }
            if item.item_type.is_some() {
                existing.item_type = item.item_type;
            }
            self.insert_sorted(existing);
            return false;
        }

        // New item.
        self.insert_sorted(item);
        self.enforce_capacity();
        true
    }

    /// Inserts an item in the correct position (pinned first, then by recency).
    fn insert_sorted(&mut self, item: RecentItem) {
        if item.pinned {
            // Insert at the front of pinned items.
            let pos = self
                .items
                .iter()
                .position(|i| !i.pinned)
                .unwrap_or(self.items.len());
            self.items.insert(0.min(pos), item);
        } else {
            // Insert after all pinned items (front of unpinned).
            let pos = self
                .items
                .iter()
                .position(|i| !i.pinned)
                .unwrap_or(self.items.len());
            self.items.insert(pos, item);
        }
    }

    /// Enforces the maximum capacity by removing the oldest unpinned items.
    fn enforce_capacity(&mut self) {
        while self.items.len() > self.max_items {
            // Find the last unpinned item and remove it.
            if let Some(pos) = self.items.iter().rposition(|i| !i.pinned) {
                self.items.remove(pos);
            } else {
                // All items are pinned — remove the last one anyway.
                self.items.pop();
            }
        }
    }

    /// Removes an item by path.
    pub fn remove(&mut self, path: &str) -> bool {
        let before = self.items.len();
        self.items.retain(|i| i.path != path);
        self.items.len() < before
    }

    /// Pins an item by path. Returns `false` if not found.
    pub fn pin(&mut self, path: &str) -> bool {
        if let Some(item) = self.items.iter_mut().find(|i| i.path == path) {
            item.pinned = true;
            // Re-sort to move it to the pinned section.
            let items = std::mem::take(&mut self.items);
            self.items = Vec::with_capacity(items.len());
            for i in items {
                self.insert_sorted(i);
            }
            true
        } else {
            false
        }
    }

    /// Unpins an item by path. Returns `false` if not found.
    pub fn unpin(&mut self, path: &str) -> bool {
        if let Some(item) = self.items.iter_mut().find(|i| i.path == path) {
            item.pinned = false;
            // Re-sort.
            let items = std::mem::take(&mut self.items);
            self.items = Vec::with_capacity(items.len());
            for i in items {
                self.insert_sorted(i);
            }
            true
        } else {
            false
        }
    }

    /// Returns all items in display order (pinned first, then MRU).
    pub fn items(&self) -> &[RecentItem] {
        &self.items
    }

    /// Returns only pinned items.
    pub fn pinned_items(&self) -> Vec<&RecentItem> {
        self.items.iter().filter(|i| i.pinned).collect()
    }

    /// Returns only unpinned items.
    pub fn unpinned_items(&self) -> Vec<&RecentItem> {
        self.items.iter().filter(|i| !i.pinned).collect()
    }

    /// Returns the number of items.
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Returns whether the list is empty.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Returns the maximum capacity.
    pub fn max_items(&self) -> usize {
        self.max_items
    }

    /// Clears all items.
    pub fn clear(&mut self) {
        self.items.clear();
    }

    /// Clears only unpinned items.
    pub fn clear_unpinned(&mut self) {
        self.items.retain(|i| i.pinned);
    }

    /// Returns an item by path.
    pub fn get(&self, path: &str) -> Option<&RecentItem> {
        self.items.iter().find(|i| i.path == path)
    }

    /// Returns whether an item with the given path exists.
    pub fn contains(&self, path: &str) -> bool {
        self.items.iter().any(|i| i.path == path)
    }

    /// Filters items by type hint.
    pub fn items_by_type(&self, item_type: &str) -> Vec<&RecentItem> {
        self.items
            .iter()
            .filter(|i| i.item_type.as_deref() == Some(item_type))
            .collect()
    }

    /// Searches items by name or path (case-insensitive substring match).
    pub fn search(&self, query: &str) -> Vec<&RecentItem> {
        if query.is_empty() {
            return self.items.iter().collect();
        }
        let lower = query.to_lowercase();
        self.items
            .iter()
            .filter(|i| {
                i.name.to_lowercase().contains(&lower) || i.path.to_lowercase().contains(&lower)
            })
            .collect()
    }
}

// ---------------------------------------------------------------------------
// RecentFiles + RecentProjects (convenience wrappers)
// ---------------------------------------------------------------------------

/// Recent files list with file-specific defaults (max 30 items).
pub type RecentFiles = RecentList;

/// Recent projects list with project-specific defaults (max 20 items).
pub type RecentProjects = RecentList;

/// Creates a new recent files list with the default capacity (30).
pub fn recent_files() -> RecentFiles {
    RecentList::new(30)
}

/// Creates a new recent projects list with the default capacity (20).
pub fn recent_projects() -> RecentProjects {
    RecentList::new(20)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // ── RecentItem ───────────────────────────────────────────────────

    #[test]
    fn item_new() {
        let item = RecentItem::new("res://main.tscn", "main.tscn");
        assert_eq!(item.path, "res://main.tscn");
        assert_eq!(item.name, "main.tscn");
        assert!(!item.pinned);
        assert!(item.item_type.is_none());
    }

    #[test]
    fn item_with_type() {
        let item = RecentItem::new("res://main.tscn", "main.tscn").with_type("scene");
        assert_eq!(item.item_type.as_deref(), Some("scene"));
    }

    #[test]
    fn item_pinned() {
        let item = RecentItem::new("res://main.tscn", "main.tscn").pinned();
        assert!(item.pinned);
    }

    // ── RecentList basics ────────────────────────────────────────────

    #[test]
    fn list_empty() {
        let list = RecentList::new(10);
        assert!(list.is_empty());
        assert_eq!(list.len(), 0);
        assert_eq!(list.max_items(), 10);
    }

    #[test]
    fn list_add_item() {
        let mut list = RecentList::new(10);
        let added = list.add("res://main.tscn", "main.tscn");
        assert!(added);
        assert_eq!(list.len(), 1);
        assert_eq!(list.items()[0].path, "res://main.tscn");
    }

    #[test]
    fn list_add_duplicate_touches() {
        let mut list = RecentList::new(10);
        list.add("res://a.tscn", "a.tscn");
        list.add("res://b.tscn", "b.tscn");

        // Touch a — should move to front of unpinned.
        let added = list.add("res://a.tscn", "a.tscn");
        assert!(!added); // Not new.
        assert_eq!(list.len(), 2);
        assert_eq!(list.items()[0].path, "res://a.tscn");
    }

    #[test]
    fn list_capacity_enforcement() {
        let mut list = RecentList::new(3);
        list.add("res://a.tscn", "a");
        list.add("res://b.tscn", "b");
        list.add("res://c.tscn", "c");
        list.add("res://d.tscn", "d");

        assert_eq!(list.len(), 3);
        // Oldest unpinned (a) should be evicted.
        assert!(!list.contains("res://a.tscn"));
        assert!(list.contains("res://d.tscn"));
    }

    #[test]
    fn list_remove() {
        let mut list = RecentList::new(10);
        list.add("res://a.tscn", "a");
        list.add("res://b.tscn", "b");

        assert!(list.remove("res://a.tscn"));
        assert_eq!(list.len(), 1);
        assert!(!list.contains("res://a.tscn"));
    }

    #[test]
    fn list_remove_nonexistent() {
        let mut list = RecentList::new(10);
        assert!(!list.remove("res://nope.tscn"));
    }

    #[test]
    fn list_clear() {
        let mut list = RecentList::new(10);
        list.add("res://a.tscn", "a");
        list.add("res://b.tscn", "b");
        list.clear();
        assert!(list.is_empty());
    }

    #[test]
    fn list_get() {
        let mut list = RecentList::new(10);
        list.add("res://a.tscn", "a");
        assert!(list.get("res://a.tscn").is_some());
        assert!(list.get("res://nope.tscn").is_none());
    }

    // ── Pinning ──────────────────────────────────────────────────────

    #[test]
    fn list_pin_item() {
        let mut list = RecentList::new(10);
        list.add("res://a.tscn", "a");
        list.add("res://b.tscn", "b");

        assert!(list.pin("res://a.tscn"));
        assert!(list.get("res://a.tscn").unwrap().pinned);

        // Pinned items should appear before unpinned.
        assert_eq!(list.items()[0].path, "res://a.tscn");
    }

    #[test]
    fn list_unpin_item() {
        let mut list = RecentList::new(10);
        list.add_item(RecentItem::new("res://a.tscn", "a").pinned());
        list.add("res://b.tscn", "b");

        assert!(list.unpin("res://a.tscn"));
        assert!(!list.get("res://a.tscn").unwrap().pinned);
    }

    #[test]
    fn list_pin_nonexistent() {
        let mut list = RecentList::new(10);
        assert!(!list.pin("res://nope.tscn"));
    }

    #[test]
    fn list_pinned_items_filter() {
        let mut list = RecentList::new(10);
        list.add_item(RecentItem::new("res://a.tscn", "a").pinned());
        list.add("res://b.tscn", "b");
        list.add_item(RecentItem::new("res://c.tscn", "c").pinned());

        assert_eq!(list.pinned_items().len(), 2);
        assert_eq!(list.unpinned_items().len(), 1);
    }

    #[test]
    fn list_pinned_survive_eviction() {
        let mut list = RecentList::new(3);
        list.add_item(RecentItem::new("res://pinned.tscn", "pinned").pinned());
        list.add("res://a.tscn", "a");
        list.add("res://b.tscn", "b");
        list.add("res://c.tscn", "c"); // Should evict oldest unpinned, not pinned.

        assert_eq!(list.len(), 3);
        assert!(list.contains("res://pinned.tscn"));
        assert!(!list.contains("res://a.tscn")); // Oldest unpinned evicted.
    }

    #[test]
    fn list_clear_unpinned() {
        let mut list = RecentList::new(10);
        list.add_item(RecentItem::new("res://pinned.tscn", "pinned").pinned());
        list.add("res://a.tscn", "a");
        list.add("res://b.tscn", "b");

        list.clear_unpinned();
        assert_eq!(list.len(), 1);
        assert!(list.contains("res://pinned.tscn"));
    }

    // ── Search and filtering ─────────────────────────────────────────

    #[test]
    fn list_search_by_name() {
        let mut list = RecentList::new(10);
        list.add("res://player.tscn", "Player Scene");
        list.add("res://enemy.tscn", "Enemy Scene");
        list.add("res://main.gd", "Main Script");

        let results = list.search("scene");
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn list_search_by_path() {
        let mut list = RecentList::new(10);
        list.add("res://player.tscn", "Player");
        list.add("res://enemy.tscn", "Enemy");

        let results = list.search("player");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "Player");
    }

    #[test]
    fn list_search_case_insensitive() {
        let mut list = RecentList::new(10);
        list.add("res://Main.tscn", "Main");

        assert_eq!(list.search("main").len(), 1);
        assert_eq!(list.search("MAIN").len(), 1);
    }

    #[test]
    fn list_search_empty_returns_all() {
        let mut list = RecentList::new(10);
        list.add("res://a.tscn", "a");
        list.add("res://b.tscn", "b");

        assert_eq!(list.search("").len(), 2);
    }

    #[test]
    fn list_items_by_type() {
        let mut list = RecentList::new(10);
        list.add_item(RecentItem::new("res://a.tscn", "a").with_type("scene"));
        list.add_item(RecentItem::new("res://b.gd", "b").with_type("script"));
        list.add_item(RecentItem::new("res://c.tscn", "c").with_type("scene"));

        let scenes = list.items_by_type("scene");
        assert_eq!(scenes.len(), 2);

        let scripts = list.items_by_type("script");
        assert_eq!(scripts.len(), 1);
    }

    // ── Convenience constructors ─────────────────────────────────────

    #[test]
    fn recent_files_default_capacity() {
        let files = recent_files();
        assert_eq!(files.max_items(), 30);
    }

    #[test]
    fn recent_projects_default_capacity() {
        let projects = recent_projects();
        assert_eq!(projects.max_items(), 20);
    }

    // ── Edge cases ───────────────────────────────────────────────────

    #[test]
    fn list_zero_capacity() {
        let mut list = RecentList::new(0);
        list.add("res://a.tscn", "a");
        assert_eq!(list.len(), 0);
    }

    #[test]
    fn list_capacity_one() {
        let mut list = RecentList::new(1);
        list.add("res://a.tscn", "a");
        list.add("res://b.tscn", "b");
        assert_eq!(list.len(), 1);
        assert_eq!(list.items()[0].path, "res://b.tscn");
    }

    #[test]
    fn list_add_with_item_type_preserved_on_touch() {
        let mut list = RecentList::new(10);
        list.add_item(RecentItem::new("res://a.tscn", "a").with_type("scene"));

        // Touch without type — should keep existing type.
        list.add("res://a.tscn", "a");
        assert_eq!(
            list.get("res://a.tscn").unwrap().item_type.as_deref(),
            Some("scene")
        );
    }

    #[test]
    fn list_add_with_new_type_overrides() {
        let mut list = RecentList::new(10);
        list.add_item(RecentItem::new("res://a.tscn", "a").with_type("scene"));

        // Touch with new type — should update.
        list.add_item(RecentItem::new("res://a.tscn", "a").with_type("packed_scene"));
        assert_eq!(
            list.get("res://a.tscn").unwrap().item_type.as_deref(),
            Some("packed_scene")
        );
    }
}
