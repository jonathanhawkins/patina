//! The open-scripts panel for the script editor.
//!
//! Tracks the list of currently open script buffers and which one is the active
//! editor. Opening a script that is already open focuses it rather than adding a
//! duplicate; selecting an entry makes it the active buffer; closing an entry
//! removes it and keeps the active selection sensible (it moves to the entry that
//! slid into the closed slot, clamped to the end, and becomes `None` when the
//! panel empties).

/// Tracks open scripts (by path) and the active selection.
#[derive(Debug, Clone, Default)]
pub struct OpenScriptsPanel {
    open: Vec<String>,
    active: Option<usize>,
}

impl OpenScriptsPanel {
    /// Creates an empty panel.
    pub fn new() -> Self {
        Self::default()
    }

    /// The paths of all open scripts, in open order.
    pub fn scripts(&self) -> &[String] {
        &self.open
    }

    /// Number of open scripts.
    pub fn len(&self) -> usize {
        self.open.len()
    }

    /// Whether the panel has no open scripts.
    pub fn is_empty(&self) -> bool {
        self.open.is_empty()
    }

    /// Whether `path` is currently open.
    pub fn is_open(&self, path: &str) -> bool {
        self.open.iter().any(|p| p == path)
    }

    /// The active script's index, if any.
    pub fn active_index(&self) -> Option<usize> {
        self.active
    }

    /// The active script's path, if any.
    pub fn active_path(&self) -> Option<&str> {
        self.active.map(|i| self.open[i].as_str())
    }

    /// Opens `path`, making it active. If it is already open, focuses the
    /// existing entry instead of duplicating it. Returns the entry's index.
    pub fn open(&mut self, path: impl Into<String>) -> usize {
        let path = path.into();
        let idx = match self.open.iter().position(|p| *p == path) {
            Some(i) => i,
            None => {
                self.open.push(path);
                self.open.len() - 1
            }
        };
        self.active = Some(idx);
        idx
    }

    /// Selects the entry at `index` as the active buffer. Returns `false` if the
    /// index is out of range.
    pub fn select(&mut self, index: usize) -> bool {
        if index < self.open.len() {
            self.active = Some(index);
            true
        } else {
            false
        }
    }

    /// Closes the entry at `index`, removing it from the panel and adjusting the
    /// active selection. Returns `false` if the index is out of range.
    pub fn close(&mut self, index: usize) -> bool {
        if index >= self.open.len() {
            return false;
        }
        self.open.remove(index);
        self.active = match self.active {
            _ if self.open.is_empty() => None,
            Some(a) if a == index => Some(index.min(self.open.len() - 1)),
            Some(a) if a > index => Some(a - 1),
            other => other,
        };
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Acceptance (pat-thr6k): the panel lists open scripts, selecting one makes
    /// it the active editor buffer, and closing an entry removes it.
    #[test]
    fn script_nav_open_scripts_panel() {
        let mut p = OpenScriptsPanel::new();
        assert!(p.is_empty());
        assert_eq!(p.active_index(), None);

        // Opening scripts lists them; the most recently opened is active.
        p.open("res://player.gd");
        p.open("res://enemy.gd");
        p.open("res://ui.gd");
        assert_eq!(
            p.scripts(),
            &[
                "res://player.gd".to_string(),
                "res://enemy.gd".to_string(),
                "res://ui.gd".to_string()
            ]
        );
        assert_eq!(p.active_path(), Some("res://ui.gd"));

        // Opening an already-open script focuses it instead of duplicating.
        let idx = p.open("res://player.gd");
        assert_eq!(idx, 0);
        assert_eq!(p.len(), 3);
        assert_eq!(p.active_path(), Some("res://player.gd"));

        // Selecting an entry makes it the active editor buffer.
        assert!(p.select(1));
        assert_eq!(p.active_path(), Some("res://enemy.gd"));
        assert!(!p.select(9)); // out of range, no change
        assert_eq!(p.active_path(), Some("res://enemy.gd"));

        // Closing the active entry removes it; active moves to the entry that
        // slid into that slot (ui.gd).
        assert!(p.close(1));
        assert_eq!(
            p.scripts(),
            &["res://player.gd".to_string(), "res://ui.gd".to_string()]
        );
        assert_eq!(p.active_path(), Some("res://ui.gd"));

        // Closing an entry before the active one shifts the active index down.
        assert!(p.close(0)); // remove player.gd; active ui shifts 1 -> 0
        assert_eq!(p.scripts(), &["res://ui.gd".to_string()]);
        assert_eq!(p.active_path(), Some("res://ui.gd"));

        // Closing the last entry empties the panel and clears the active index.
        assert!(p.close(0));
        assert!(p.is_empty());
        assert_eq!(p.active_index(), None);
        assert_eq!(p.active_path(), None);

        // Out-of-range close is a no-op.
        assert!(!p.close(0));
    }

    /// `is_open` reflects membership and `open` is idempotent on paths.
    #[test]
    fn open_scripts_membership_and_idempotent_open() {
        let mut p = OpenScriptsPanel::new();
        assert!(!p.is_open("res://a.gd"));
        p.open("res://a.gd");
        assert!(p.is_open("res://a.gd"));
        // Re-opening does not grow the list.
        p.open("res://a.gd");
        assert_eq!(p.len(), 1);
    }
}
