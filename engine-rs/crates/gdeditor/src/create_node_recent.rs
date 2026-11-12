//! The Create Node dialog's **Recent** section.
//!
//! Each time the user creates a node, its type is recorded here so the dialog
//! can offer a "Recent" shortcut list. Entries are ordered most-recent-first,
//! re-creating an existing type moves it back to the front (rather than
//! duplicating it), and the list is capped to a fixed history limit so it never
//! grows without bound.
//!
//! This is a focused recent-*types* tracker, distinct from the recent-*files*
//! list in `recent_items`.

/// The default number of recent types to remember.
pub const DEFAULT_RECENT_LIMIT: usize = 8;

/// A capped, most-recent-first list of recently created node types.
#[derive(Debug, Clone)]
pub struct RecentTypes {
    items: Vec<String>,
    limit: usize,
}

impl Default for RecentTypes {
    fn default() -> Self {
        Self::new(DEFAULT_RECENT_LIMIT)
    }
}

impl RecentTypes {
    /// Creates an empty recent list capped at `limit` entries. A `limit` of 0 is
    /// treated as 1 so at least the latest type is remembered.
    pub fn new(limit: usize) -> Self {
        Self {
            items: Vec::new(),
            limit: limit.max(1),
        }
    }

    /// Records that a node of `class_name` was created: moves it to the front of
    /// the recent list (de-duplicating), and drops the oldest entry if the list
    /// exceeds its limit.
    pub fn record(&mut self, class_name: impl Into<String>) {
        let class_name = class_name.into();
        // Remove any existing occurrence so re-creating moves it to the front.
        self.items.retain(|c| c != &class_name);
        self.items.insert(0, class_name);
        if self.items.len() > self.limit {
            self.items.truncate(self.limit);
        }
    }

    /// The recent types, most-recent-first.
    pub fn items(&self) -> &[String] {
        &self.items
    }

    /// The most-recently created type, if any.
    pub fn most_recent(&self) -> Option<&str> {
        self.items.first().map(String::as_str)
    }

    /// Whether `class_name` is in the recent list.
    pub fn contains(&self, class_name: &str) -> bool {
        self.items.iter().any(|c| c == class_name)
    }

    /// The number of recent entries.
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Whether the recent list is empty.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// The history cap.
    pub fn limit(&self) -> usize {
        self.limit
    }

    /// Clears the recent list.
    pub fn clear(&mut self) {
        self.items.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Acceptance (pat-7fkj1): creating a node records its type in the Recent
    /// section, ordered most-recent-first and capped to the history limit.
    #[test]
    fn create_node_recent() {
        let mut recent = RecentTypes::new(3);
        assert!(recent.is_empty());

        // Creating nodes records their types, most-recent-first.
        recent.record("Node2D");
        recent.record("Sprite2D");
        recent.record("Area2D");
        assert_eq!(recent.items(), &["Area2D", "Sprite2D", "Node2D"]);
        assert_eq!(recent.most_recent(), Some("Area2D"));

        // Re-creating an existing type moves it to the front without duplicating.
        recent.record("Node2D");
        assert_eq!(recent.items(), &["Node2D", "Area2D", "Sprite2D"]);
        assert_eq!(recent.len(), 3, "no duplicate entry added");

        // Exceeding the limit drops the oldest entry.
        recent.record("Camera2D");
        assert_eq!(recent.items(), &["Camera2D", "Node2D", "Area2D"]);
        assert_eq!(recent.len(), 3, "capped at the history limit");
        assert!(!recent.contains("Sprite2D"), "oldest entry evicted");

        recent.record("CharacterBody2D");
        assert_eq!(recent.items(), &["CharacterBody2D", "Camera2D", "Node2D"]);

        // Clearing empties the recent list.
        recent.clear();
        assert!(recent.is_empty());
        assert!(recent.most_recent().is_none());
    }

    /// A zero limit is clamped so at least the latest type is kept.
    #[test]
    fn zero_limit_keeps_latest() {
        let mut recent = RecentTypes::new(0);
        assert_eq!(recent.limit(), 1);
        recent.record("Node2D");
        recent.record("Sprite2D");
        assert_eq!(recent.items(), &["Sprite2D"]);
    }
}
