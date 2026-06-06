//! Script editor **line bookmarks** (pat-dk6zm).
//!
//! Bookmarks mark lines of interest in the current script: toggling a line adds
//! or removes its bookmark (shown in the gutter), and next/previous-bookmark
//! moves the caret through the bookmarked lines in order, wrapping around at the
//! ends.

use std::collections::BTreeSet;

/// Tracks bookmarked lines and the caret, for jump-to-bookmark navigation.
#[derive(Debug, Clone, Default)]
pub struct Bookmarks {
    marks: BTreeSet<usize>,
    caret_line: usize,
}

impl Bookmarks {
    /// Creates an empty bookmark set with the caret on line 1.
    pub fn new() -> Self {
        Self {
            marks: BTreeSet::new(),
            caret_line: 1,
        }
    }

    /// The current caret line (1-based).
    pub fn caret_line(&self) -> usize {
        self.caret_line
    }

    /// Moves the caret to `line` without changing bookmarks.
    pub fn set_caret(&mut self, line: usize) {
        self.caret_line = line;
    }

    /// Whether `line` is bookmarked.
    pub fn is_bookmarked(&self, line: usize) -> bool {
        self.marks.contains(&line)
    }

    /// All bookmarked lines in ascending order.
    pub fn bookmarks(&self) -> Vec<usize> {
        self.marks.iter().copied().collect()
    }

    /// The number of bookmarks.
    pub fn len(&self) -> usize {
        self.marks.len()
    }

    /// Whether there are no bookmarks.
    pub fn is_empty(&self) -> bool {
        self.marks.is_empty()
    }

    /// Toggles the bookmark on `line`, returning whether it is now bookmarked.
    pub fn toggle(&mut self, line: usize) -> bool {
        if self.marks.remove(&line) {
            false
        } else {
            self.marks.insert(line);
            true
        }
    }

    /// Removes every bookmark.
    pub fn clear(&mut self) {
        self.marks.clear();
    }

    /// Moves the caret to the next bookmark after the current line, wrapping to
    /// the first bookmark. Returns the line jumped to, or `None` if there are no
    /// bookmarks.
    pub fn next_bookmark(&mut self) -> Option<usize> {
        if self.marks.is_empty() {
            return None;
        }
        let target = self
            .marks
            .iter()
            .find(|&&l| l > self.caret_line)
            .copied()
            .unwrap_or_else(|| *self.marks.iter().next().unwrap());
        self.caret_line = target;
        Some(target)
    }

    /// Moves the caret to the previous bookmark before the current line,
    /// wrapping to the last bookmark. Returns the line jumped to, or `None` if
    /// there are no bookmarks.
    pub fn prev_bookmark(&mut self) -> Option<usize> {
        if self.marks.is_empty() {
            return None;
        }
        let target = self
            .marks
            .iter()
            .rev()
            .find(|&&l| l < self.caret_line)
            .copied()
            .unwrap_or_else(|| *self.marks.iter().next_back().unwrap());
        self.caret_line = target;
        Some(target)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Acceptance (pat-dk6zm): toggling a bookmark marks the line, and
    /// next/previous-bookmark cycles the caret through bookmarks across the
    /// script.
    #[test]
    fn script_nav_bookmarks() {
        let mut bm = Bookmarks::new();
        assert!(bm.is_empty());

        // Toggling marks a line; toggling again clears it.
        assert!(bm.toggle(5));
        assert!(bm.is_bookmarked(5));
        assert!(!bm.toggle(5));
        assert!(!bm.is_bookmarked(5));

        // Bookmark three lines (out of order); they're kept sorted.
        bm.toggle(7);
        bm.toggle(3);
        bm.toggle(12);
        assert_eq!(bm.bookmarks(), vec![3, 7, 12]);

        // From the top of the file, next cycles forward through bookmarks…
        bm.set_caret(1);
        assert_eq!(bm.next_bookmark(), Some(3));
        assert_eq!(bm.next_bookmark(), Some(7));
        assert_eq!(bm.next_bookmark(), Some(12));
        // …wrapping back to the first after the last.
        assert_eq!(bm.next_bookmark(), Some(3));
        assert_eq!(bm.caret_line(), 3);

        // Previous cycles backward, wrapping to the last before the first.
        assert_eq!(bm.prev_bookmark(), Some(12));
        assert_eq!(bm.prev_bookmark(), Some(7));
        assert_eq!(bm.prev_bookmark(), Some(3));
        assert_eq!(bm.prev_bookmark(), Some(12)); // wrapped
        assert_eq!(bm.caret_line(), 12);

        // Navigation from a non-bookmarked line lands on the nearest in each
        // direction.
        bm.set_caret(8);
        assert_eq!(bm.next_bookmark(), Some(12));
        bm.set_caret(8);
        assert_eq!(bm.prev_bookmark(), Some(7));
    }

    /// With no bookmarks, navigation is a no-op; clear removes all.
    #[test]
    fn empty_and_clear() {
        let mut bm = Bookmarks::new();
        assert_eq!(bm.next_bookmark(), None);
        assert_eq!(bm.prev_bookmark(), None);

        bm.toggle(2);
        bm.toggle(4);
        assert_eq!(bm.len(), 2);
        bm.clear();
        assert!(bm.is_empty());
        assert_eq!(bm.next_bookmark(), None);
    }
}
