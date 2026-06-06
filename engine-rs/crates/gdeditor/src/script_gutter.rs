//! Script editor breakpoint and bookmark gutter.
//!
//! Mirrors Godot's script editor left-margin gutter: clickable breakpoint
//! indicators (red circles) and bookmark markers (blue flags) per line.
//! The gutter tracks line-level metadata for each open script.

use std::collections::{BTreeSet, HashMap};

/// The type of marker on a gutter line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GutterMarker {
    /// A debugger breakpoint (red circle in Godot).
    Breakpoint,
    /// A user bookmark (blue flag in Godot).
    Bookmark,
}

/// Per-script gutter state: which lines have breakpoints and/or bookmarks.
#[derive(Debug, Clone)]
pub struct ScriptGutter {
    /// Lines with breakpoints (1-indexed).
    breakpoints: BTreeSet<usize>,
    /// Lines with bookmarks (1-indexed).
    bookmarks: BTreeSet<usize>,
    /// Total line count of the script (for validation).
    line_count: usize,
}

impl ScriptGutter {
    /// Creates a new gutter for a script with the given line count.
    pub fn new(line_count: usize) -> Self {
        Self {
            breakpoints: BTreeSet::new(),
            bookmarks: BTreeSet::new(),
            line_count,
        }
    }

    /// Returns the total line count.
    pub fn line_count(&self) -> usize {
        self.line_count
    }

    /// Updates the line count (e.g., after editing the script).
    ///
    /// Removes any markers beyond the new line count.
    pub fn set_line_count(&mut self, count: usize) {
        self.line_count = count;
        self.breakpoints.retain(|&line| line <= count);
        self.bookmarks.retain(|&line| line <= count);
    }

    // -- breakpoints ---------------------------------------------------------

    /// Toggles a breakpoint on the given line. Returns the new state.
    pub fn toggle_breakpoint(&mut self, line: usize) -> bool {
        if line == 0 || line > self.line_count {
            return false;
        }
        if self.breakpoints.contains(&line) {
            self.breakpoints.remove(&line);
            false
        } else {
            self.breakpoints.insert(line);
            true
        }
    }

    /// Sets a breakpoint on the given line. Returns `false` if out of range.
    pub fn set_breakpoint(&mut self, line: usize) -> bool {
        if line == 0 || line > self.line_count {
            return false;
        }
        self.breakpoints.insert(line);
        true
    }

    /// Clears a breakpoint on the given line. Returns `true` if it was set.
    pub fn clear_breakpoint(&mut self, line: usize) -> bool {
        self.breakpoints.remove(&line)
    }

    /// Returns whether the given line has a breakpoint.
    pub fn has_breakpoint(&self, line: usize) -> bool {
        self.breakpoints.contains(&line)
    }

    /// Returns all breakpoint lines, sorted.
    pub fn breakpoint_lines(&self) -> Vec<usize> {
        self.breakpoints.iter().copied().collect()
    }

    /// Returns the number of breakpoints.
    pub fn breakpoint_count(&self) -> usize {
        self.breakpoints.len()
    }

    /// Clears all breakpoints.
    pub fn clear_all_breakpoints(&mut self) {
        self.breakpoints.clear();
    }

    /// Returns the next breakpoint line after `current`, wrapping around.
    pub fn next_breakpoint(&self, current: usize) -> Option<usize> {
        // Find next after current.
        if let Some(&line) = self.breakpoints.range((current + 1)..).next() {
            return Some(line);
        }
        // Wrap around to the first breakpoint.
        self.breakpoints.iter().next().copied()
    }

    /// Returns the previous breakpoint line before `current`, wrapping around.
    pub fn prev_breakpoint(&self, current: usize) -> Option<usize> {
        if current > 1 {
            if let Some(&line) = self.breakpoints.range(..current).next_back() {
                return Some(line);
            }
        }
        // Wrap around to the last breakpoint.
        self.breakpoints.iter().next_back().copied()
    }

    // -- bookmarks -----------------------------------------------------------

    /// Toggles a bookmark on the given line. Returns the new state.
    pub fn toggle_bookmark(&mut self, line: usize) -> bool {
        if line == 0 || line > self.line_count {
            return false;
        }
        if self.bookmarks.contains(&line) {
            self.bookmarks.remove(&line);
            false
        } else {
            self.bookmarks.insert(line);
            true
        }
    }

    /// Sets a bookmark on the given line. Returns `false` if out of range.
    pub fn set_bookmark(&mut self, line: usize) -> bool {
        if line == 0 || line > self.line_count {
            return false;
        }
        self.bookmarks.insert(line);
        true
    }

    /// Clears a bookmark on the given line. Returns `true` if it was set.
    pub fn clear_bookmark(&mut self, line: usize) -> bool {
        self.bookmarks.remove(&line)
    }

    /// Returns whether the given line has a bookmark.
    pub fn has_bookmark(&self, line: usize) -> bool {
        self.bookmarks.contains(&line)
    }

    /// Returns all bookmark lines, sorted.
    pub fn bookmark_lines(&self) -> Vec<usize> {
        self.bookmarks.iter().copied().collect()
    }

    /// Returns the number of bookmarks.
    pub fn bookmark_count(&self) -> usize {
        self.bookmarks.len()
    }

    /// Clears all bookmarks.
    pub fn clear_all_bookmarks(&mut self) {
        self.bookmarks.clear();
    }

    /// Returns the next bookmark line after `current`, wrapping around.
    pub fn next_bookmark(&self, current: usize) -> Option<usize> {
        if let Some(&line) = self.bookmarks.range((current + 1)..).next() {
            return Some(line);
        }
        self.bookmarks.iter().next().copied()
    }

    /// Returns the previous bookmark line before `current`, wrapping around.
    pub fn prev_bookmark(&self, current: usize) -> Option<usize> {
        if current > 1 {
            if let Some(&line) = self.bookmarks.range(..current).next_back() {
                return Some(line);
            }
        }
        self.bookmarks.iter().next_back().copied()
    }

    // -- combined queries ----------------------------------------------------

    /// Returns all markers on a given line.
    pub fn markers_on_line(&self, line: usize) -> Vec<GutterMarker> {
        let mut markers = Vec::new();
        if self.breakpoints.contains(&line) {
            markers.push(GutterMarker::Breakpoint);
        }
        if self.bookmarks.contains(&line) {
            markers.push(GutterMarker::Bookmark);
        }
        markers
    }

    /// Returns whether the given line has any gutter marker.
    pub fn has_any_marker(&self, line: usize) -> bool {
        self.breakpoints.contains(&line) || self.bookmarks.contains(&line)
    }

    /// Shifts markers when lines are inserted or deleted.
    ///
    /// `at_line`: the line where insertion/deletion starts (1-indexed).
    /// `delta`: positive for inserted lines, negative for deleted lines.
    pub fn shift_lines(&mut self, at_line: usize, delta: isize) {
        if delta == 0 {
            return;
        }
        let shift = |set: &BTreeSet<usize>| -> BTreeSet<usize> {
            set.iter()
                .filter_map(|&line| {
                    if line < at_line {
                        Some(line)
                    } else if delta > 0 {
                        let new_line = line + delta as usize;
                        Some(new_line)
                    } else {
                        let abs_delta = (-delta) as usize;
                        if line < at_line + abs_delta {
                            None // line was deleted
                        } else {
                            Some(line - abs_delta)
                        }
                    }
                })
                .collect()
        };

        self.breakpoints = shift(&self.breakpoints);
        self.bookmarks = shift(&self.bookmarks);

        if delta > 0 {
            self.line_count += delta as usize;
        } else {
            let abs = (-delta) as usize;
            self.line_count = self.line_count.saturating_sub(abs);
        }
    }
}

impl Default for ScriptGutter {
    fn default() -> Self {
        Self::new(0)
    }
}

/// Manages gutters for multiple open scripts, keyed by file path.
#[derive(Debug, Default)]
pub struct GutterManager {
    gutters: HashMap<String, ScriptGutter>,
}

impl GutterManager {
    /// Creates an empty gutter manager.
    pub fn new() -> Self {
        Self {
            gutters: HashMap::new(),
        }
    }

    /// Opens (or re-opens) a script with the given line count.
    pub fn open_script(&mut self, path: impl Into<String>, line_count: usize) {
        self.gutters
            .insert(path.into(), ScriptGutter::new(line_count));
    }

    /// Closes a script, removing its gutter state.
    pub fn close_script(&mut self, path: &str) -> bool {
        self.gutters.remove(path).is_some()
    }

    /// Returns the gutter for a script, if open.
    pub fn get(&self, path: &str) -> Option<&ScriptGutter> {
        self.gutters.get(path)
    }

    /// Returns a mutable gutter for a script, if open.
    pub fn get_mut(&mut self, path: &str) -> Option<&mut ScriptGutter> {
        self.gutters.get_mut(path)
    }

    /// Returns all open script paths.
    pub fn open_scripts(&self) -> Vec<&str> {
        self.gutters.keys().map(|s| s.as_str()).collect()
    }

    /// Returns the number of open scripts.
    pub fn script_count(&self) -> usize {
        self.gutters.len()
    }

    /// Returns all breakpoints across all open scripts.
    pub fn all_breakpoints(&self) -> Vec<(&str, usize)> {
        let mut result = Vec::new();
        for (path, gutter) in &self.gutters {
            for &line in &gutter.breakpoints {
                result.push((path.as_str(), line));
            }
        }
        result.sort();
        result
    }

    /// Clears all breakpoints across all open scripts.
    pub fn clear_all_breakpoints(&mut self) {
        for gutter in self.gutters.values_mut() {
            gutter.clear_all_breakpoints();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_gutter_defaults() {
        let g = ScriptGutter::new(100);
        assert_eq!(g.line_count(), 100);
        assert_eq!(g.breakpoint_count(), 0);
        assert_eq!(g.bookmark_count(), 0);
    }

    #[test]
    fn toggle_breakpoint() {
        let mut g = ScriptGutter::new(50);
        assert!(g.toggle_breakpoint(10)); // set
        assert!(g.has_breakpoint(10));
        assert!(!g.toggle_breakpoint(10)); // unset
        assert!(!g.has_breakpoint(10));
    }

    #[test]
    fn toggle_breakpoint_out_of_range() {
        let mut g = ScriptGutter::new(10);
        assert!(!g.toggle_breakpoint(0)); // line 0 invalid
        assert!(!g.toggle_breakpoint(11)); // beyond line count
        assert_eq!(g.breakpoint_count(), 0);
    }

    #[test]
    fn set_and_clear_breakpoint() {
        let mut g = ScriptGutter::new(20);
        assert!(g.set_breakpoint(5));
        assert!(g.set_breakpoint(10));
        assert_eq!(g.breakpoint_count(), 2);
        assert_eq!(g.breakpoint_lines(), vec![5, 10]);

        assert!(g.clear_breakpoint(5));
        assert!(!g.clear_breakpoint(5)); // already cleared
        assert_eq!(g.breakpoint_count(), 1);
    }

    #[test]
    fn set_breakpoint_out_of_range() {
        let mut g = ScriptGutter::new(10);
        assert!(!g.set_breakpoint(0));
        assert!(!g.set_breakpoint(11));
    }

    #[test]
    fn clear_all_breakpoints() {
        let mut g = ScriptGutter::new(50);
        g.set_breakpoint(1);
        g.set_breakpoint(25);
        g.set_breakpoint(50);
        g.clear_all_breakpoints();
        assert_eq!(g.breakpoint_count(), 0);
    }

    #[test]
    fn toggle_bookmark() {
        let mut g = ScriptGutter::new(50);
        assert!(g.toggle_bookmark(7)); // set
        assert!(g.has_bookmark(7));
        assert!(!g.toggle_bookmark(7)); // unset
        assert!(!g.has_bookmark(7));
    }

    #[test]
    fn toggle_bookmark_out_of_range() {
        let mut g = ScriptGutter::new(10);
        assert!(!g.toggle_bookmark(0));
        assert!(!g.toggle_bookmark(11));
    }

    #[test]
    fn set_and_clear_bookmark() {
        let mut g = ScriptGutter::new(30);
        assert!(g.set_bookmark(3));
        assert!(g.set_bookmark(15));
        assert_eq!(g.bookmark_count(), 2);
        assert_eq!(g.bookmark_lines(), vec![3, 15]);

        assert!(g.clear_bookmark(3));
        assert!(!g.clear_bookmark(3));
        assert_eq!(g.bookmark_count(), 1);
    }

    #[test]
    fn clear_all_bookmarks() {
        let mut g = ScriptGutter::new(20);
        g.set_bookmark(1);
        g.set_bookmark(10);
        g.clear_all_bookmarks();
        assert_eq!(g.bookmark_count(), 0);
    }

    #[test]
    fn markers_on_line() {
        let mut g = ScriptGutter::new(20);
        g.set_breakpoint(5);
        g.set_bookmark(5);
        g.set_breakpoint(10);

        let m5 = g.markers_on_line(5);
        assert_eq!(m5.len(), 2);
        assert!(m5.contains(&GutterMarker::Breakpoint));
        assert!(m5.contains(&GutterMarker::Bookmark));

        let m10 = g.markers_on_line(10);
        assert_eq!(m10, vec![GutterMarker::Breakpoint]);

        assert!(g.markers_on_line(1).is_empty());
    }

    #[test]
    fn has_any_marker() {
        let mut g = ScriptGutter::new(10);
        assert!(!g.has_any_marker(5));
        g.set_bookmark(5);
        assert!(g.has_any_marker(5));
    }

    #[test]
    fn next_breakpoint_wraps() {
        let mut g = ScriptGutter::new(50);
        g.set_breakpoint(10);
        g.set_breakpoint(30);

        assert_eq!(g.next_breakpoint(5), Some(10));
        assert_eq!(g.next_breakpoint(10), Some(30));
        assert_eq!(g.next_breakpoint(30), Some(10)); // wraps
        assert_eq!(g.next_breakpoint(40), Some(10)); // wraps
    }

    #[test]
    fn next_breakpoint_empty() {
        let g = ScriptGutter::new(50);
        assert_eq!(g.next_breakpoint(1), None);
    }

    #[test]
    fn prev_breakpoint_wraps() {
        let mut g = ScriptGutter::new(50);
        g.set_breakpoint(10);
        g.set_breakpoint(30);

        assert_eq!(g.prev_breakpoint(30), Some(10));
        assert_eq!(g.prev_breakpoint(15), Some(10));
        assert_eq!(g.prev_breakpoint(10), Some(30)); // wraps
        assert_eq!(g.prev_breakpoint(1), Some(30)); // wraps
    }

    #[test]
    fn next_bookmark_wraps() {
        let mut g = ScriptGutter::new(40);
        g.set_bookmark(5);
        g.set_bookmark(20);

        assert_eq!(g.next_bookmark(1), Some(5));
        assert_eq!(g.next_bookmark(5), Some(20));
        assert_eq!(g.next_bookmark(20), Some(5)); // wraps
    }

    #[test]
    fn prev_bookmark_wraps() {
        let mut g = ScriptGutter::new(40);
        g.set_bookmark(5);
        g.set_bookmark(20);

        assert_eq!(g.prev_bookmark(20), Some(5));
        assert_eq!(g.prev_bookmark(5), Some(20)); // wraps
    }

    #[test]
    fn set_line_count_trims_markers() {
        let mut g = ScriptGutter::new(50);
        g.set_breakpoint(10);
        g.set_breakpoint(40);
        g.set_bookmark(30);
        g.set_bookmark(50);

        g.set_line_count(35);
        assert_eq!(g.line_count(), 35);
        assert_eq!(g.breakpoint_lines(), vec![10]); // 40 trimmed
        assert_eq!(g.bookmark_lines(), vec![30]); // 50 trimmed
    }

    #[test]
    fn shift_lines_insert() {
        let mut g = ScriptGutter::new(20);
        g.set_breakpoint(5);
        g.set_breakpoint(10);
        g.set_bookmark(8);

        // Insert 3 lines at line 7.
        g.shift_lines(7, 3);

        assert_eq!(g.line_count(), 23);
        assert_eq!(g.breakpoint_lines(), vec![5, 13]); // 10 -> 13
        assert_eq!(g.bookmark_lines(), vec![11]); // 8 -> 11
    }

    #[test]
    fn shift_lines_delete() {
        let mut g = ScriptGutter::new(30);
        g.set_breakpoint(5);
        g.set_breakpoint(10);
        g.set_breakpoint(20);
        g.set_bookmark(10);

        // Delete 5 lines starting at line 8.
        g.shift_lines(8, -5);

        assert_eq!(g.line_count(), 25);
        assert_eq!(g.breakpoint_lines(), vec![5, 15]); // 10 deleted, 20 -> 15
        assert_eq!(g.bookmark_lines(), Vec::<usize>::new()); // 10 was deleted
    }

    #[test]
    fn shift_lines_zero_delta_noop() {
        let mut g = ScriptGutter::new(10);
        g.set_breakpoint(5);
        g.shift_lines(3, 0);
        assert_eq!(g.breakpoint_lines(), vec![5]);
        assert_eq!(g.line_count(), 10);
    }

    #[test]
    fn default_gutter() {
        let g = ScriptGutter::default();
        assert_eq!(g.line_count(), 0);
        assert_eq!(g.breakpoint_count(), 0);
        assert_eq!(g.bookmark_count(), 0);
    }

    // -- GutterManager -------------------------------------------------------

    #[test]
    fn manager_open_close() {
        let mut mgr = GutterManager::new();
        mgr.open_script("res://main.gd", 100);
        mgr.open_script("res://player.gd", 50);
        assert_eq!(mgr.script_count(), 2);

        assert!(mgr.close_script("res://main.gd"));
        assert!(!mgr.close_script("res://main.gd")); // already closed
        assert_eq!(mgr.script_count(), 1);
    }

    #[test]
    fn manager_get_gutter() {
        let mut mgr = GutterManager::new();
        mgr.open_script("res://test.gd", 30);

        let g = mgr.get("res://test.gd").unwrap();
        assert_eq!(g.line_count(), 30);

        assert!(mgr.get("res://nonexistent.gd").is_none());
    }

    #[test]
    fn manager_toggle_breakpoint() {
        let mut mgr = GutterManager::new();
        mgr.open_script("res://test.gd", 20);

        let g = mgr.get_mut("res://test.gd").unwrap();
        g.toggle_breakpoint(5);
        g.toggle_breakpoint(15);

        assert_eq!(mgr.get("res://test.gd").unwrap().breakpoint_count(), 2);
    }

    #[test]
    fn manager_all_breakpoints() {
        let mut mgr = GutterManager::new();
        mgr.open_script("res://a.gd", 20);
        mgr.open_script("res://b.gd", 30);

        mgr.get_mut("res://a.gd").unwrap().set_breakpoint(5);
        mgr.get_mut("res://a.gd").unwrap().set_breakpoint(10);
        mgr.get_mut("res://b.gd").unwrap().set_breakpoint(3);

        let all = mgr.all_breakpoints();
        assert_eq!(all.len(), 3);
        // Sorted by path then line.
        assert!(all.contains(&("res://a.gd", 5)));
        assert!(all.contains(&("res://a.gd", 10)));
        assert!(all.contains(&("res://b.gd", 3)));
    }

    #[test]
    fn manager_clear_all_breakpoints() {
        let mut mgr = GutterManager::new();
        mgr.open_script("res://a.gd", 20);
        mgr.open_script("res://b.gd", 30);
        mgr.get_mut("res://a.gd").unwrap().set_breakpoint(5);
        mgr.get_mut("res://b.gd").unwrap().set_breakpoint(3);

        mgr.clear_all_breakpoints();
        assert!(mgr.all_breakpoints().is_empty());
    }

    #[test]
    fn manager_open_scripts() {
        let mut mgr = GutterManager::new();
        mgr.open_script("res://x.gd", 10);
        mgr.open_script("res://y.gd", 20);
        let mut scripts = mgr.open_scripts();
        scripts.sort();
        assert_eq!(scripts, vec!["res://x.gd", "res://y.gd"]);
    }

    #[test]
    fn manager_reopen_resets_gutter() {
        let mut mgr = GutterManager::new();
        mgr.open_script("res://test.gd", 50);
        mgr.get_mut("res://test.gd").unwrap().set_breakpoint(10);
        assert_eq!(mgr.get("res://test.gd").unwrap().breakpoint_count(), 1);

        // Re-open with different line count resets gutter.
        mgr.open_script("res://test.gd", 100);
        assert_eq!(mgr.get("res://test.gd").unwrap().line_count(), 100);
        assert_eq!(mgr.get("res://test.gd").unwrap().breakpoint_count(), 0);
    }
}
