//! Code folding for the script editor.
//!
//! A *foldable region* is a header line (a `func`, `if`, loop, etc. — any line)
//! immediately followed by a more-deeply-indented block. The region spans from
//! the header through the last line that belongs to that deeper block (blank
//! lines inside the block are absorbed). Regions nest: an inner indented block
//! is its own foldable region within an outer one.
//!
//! Folding a region's header hides its body lines and renders the header with a
//! trailing placeholder; unfolding restores the hidden lines. Fold state is keyed
//! by header line, so content edits to lines outside a folded region leave the
//! fold intact.

use std::collections::{BTreeSet, HashMap};

/// The placeholder appended to a folded region's header line.
pub const PLACEHOLDER: &str = "⋯";

/// Tracks a script buffer and which foldable regions are currently collapsed.
#[derive(Debug, Clone)]
pub struct CodeFolding {
    lines: Vec<String>,
    /// Header line indices (0-based) whose regions are folded.
    folded: BTreeSet<usize>,
}

/// The amount of leading whitespace on `line` (spaces and tabs counted equally).
fn indent(line: &str) -> usize {
    line.chars().take_while(|c| *c == ' ' || *c == '\t').count()
}

/// Whether `line` has no non-whitespace content.
fn is_blank(line: &str) -> bool {
    line.trim().is_empty()
}

impl CodeFolding {
    /// Builds folding state from `source`, split into lines. Nothing is folded
    /// initially.
    pub fn new(source: &str) -> Self {
        Self {
            lines: source.lines().map(|l| l.to_string()).collect(),
            folded: BTreeSet::new(),
        }
    }

    /// The current lines of the buffer.
    pub fn lines(&self) -> &[String] {
        &self.lines
    }

    /// All foldable regions as `(start, end)` inclusive 0-based line ranges,
    /// where `start` is the header line. Ordered by start line.
    pub fn foldable_regions(&self) -> Vec<(usize, usize)> {
        let n = self.lines.len();
        let mut regions = Vec::new();
        for i in 0..n {
            if is_blank(&self.lines[i]) {
                continue;
            }
            let base = indent(&self.lines[i]);
            let mut last = i;
            let mut j = i + 1;
            while j < n {
                if is_blank(&self.lines[j]) {
                    j += 1;
                    continue;
                }
                if indent(&self.lines[j]) > base {
                    last = j;
                    j += 1;
                } else {
                    break;
                }
            }
            if last > i {
                regions.push((i, last));
            }
        }
        regions
    }

    /// Whether line `header` starts a foldable region (has a gutter affordance).
    pub fn is_foldable(&self, header: usize) -> bool {
        self.foldable_regions().iter().any(|(s, _)| *s == header)
    }

    /// Collapses the region whose header is `header`. No-op if the line is not a
    /// region header.
    pub fn fold(&mut self, header: usize) {
        if self.is_foldable(header) {
            self.folded.insert(header);
        }
    }

    /// Expands the region whose header is `header`.
    pub fn unfold(&mut self, header: usize) {
        self.folded.remove(&header);
    }

    /// Toggles the fold state of the region whose header is `header`, returning
    /// the new state (`true` = folded).
    pub fn toggle_fold(&mut self, header: usize) -> bool {
        if self.is_folded(header) {
            self.unfold(header);
            false
        } else {
            self.fold(header);
            self.is_folded(header)
        }
    }

    /// Whether the region whose header is `header` is currently folded.
    pub fn is_folded(&self, header: usize) -> bool {
        self.folded.contains(&header)
    }

    /// Replaces the text of line `idx`. Line indices are unchanged, so existing
    /// folds (keyed by header line) survive edits to lines outside their region.
    pub fn set_line(&mut self, idx: usize, text: impl Into<String>) {
        if idx < self.lines.len() {
            self.lines[idx] = text.into();
        }
    }

    /// The lines as displayed: every folded region collapses to its header line
    /// plus a trailing [`PLACEHOLDER`], with the body lines hidden. A folded
    /// header whose region no longer exists is shown normally.
    pub fn visible_lines(&self) -> Vec<String> {
        let region_end: HashMap<usize, usize> = self.foldable_regions().into_iter().collect();
        let mut out = Vec::new();
        let mut i = 0;
        while i < self.lines.len() {
            if self.folded.contains(&i) {
                if let Some(&end) = region_end.get(&i) {
                    out.push(format!("{} {}", self.lines[i], PLACEHOLDER));
                    i = end + 1;
                    continue;
                }
            }
            out.push(self.lines[i].clone());
            i += 1;
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Acceptance (pat-ate1n): foldable regions expose a gutter affordance,
    /// folding hides the body and shows a placeholder, unfolding restores it,
    /// and folds survive edits outside the region.
    #[test]
    fn script_core_code_folding() {
        let src = "func foo():\n    var a = 1\n    var b = 2\nfunc bar():\n    return 0\n";
        let mut cf = CodeFolding::new(src);

        // foo() spans lines 0..=2, bar() spans 3..=4.
        assert_eq!(cf.foldable_regions(), vec![(0, 2), (3, 4)]);

        // Region headers have a gutter affordance; body lines do not.
        assert!(cf.is_foldable(0));
        assert!(cf.is_foldable(3));
        assert!(!cf.is_foldable(1));

        // Fold foo(): body lines 1-2 hidden, header shows the placeholder.
        cf.fold(0);
        assert!(cf.is_folded(0));
        assert_eq!(
            cf.visible_lines(),
            vec![
                format!("func foo(): {}", PLACEHOLDER),
                "func bar():".to_string(),
                "    return 0".to_string(),
            ]
        );

        // Editing a line outside the folded region keeps the fold intact.
        cf.set_line(4, "    return 42");
        assert!(cf.is_folded(0));
        assert_eq!(
            cf.visible_lines(),
            vec![
                format!("func foo(): {}", PLACEHOLDER),
                "func bar():".to_string(),
                "    return 42".to_string(),
            ]
        );

        // Unfolding restores the hidden body.
        cf.unfold(0);
        assert!(!cf.is_folded(0));
        assert_eq!(
            cf.visible_lines(),
            vec![
                "func foo():".to_string(),
                "    var a = 1".to_string(),
                "    var b = 2".to_string(),
                "func bar():".to_string(),
                "    return 42".to_string(),
            ]
        );
    }

    /// Nested regions fold independently, and blank lines inside a block are
    /// absorbed into the region.
    #[test]
    fn code_folding_nested_and_blanks() {
        // func with a nested if, plus a blank line inside the block.
        let src = "func f():\n    if x:\n        a()\n\n        b()\n    c()\n";
        let cf = CodeFolding::new(src);
        // Outer func spans the whole body (through the last deeper line, 5);
        // the inner `if` spans its own deeper block (2..=4, absorbing the blank).
        let regions = cf.foldable_regions();
        assert!(regions.contains(&(0, 5)));
        assert!(regions.contains(&(1, 4)));

        // Folding only the inner region hides its body but keeps the rest.
        let mut cf2 = cf.clone();
        cf2.fold(1);
        assert_eq!(
            cf2.visible_lines(),
            vec![
                "func f():".to_string(),
                format!("    if x: {}", PLACEHOLDER),
                "    c()".to_string(),
            ]
        );
    }

    /// toggle_fold flips state, and folding a non-header line is a no-op.
    #[test]
    fn code_folding_toggle_and_guard() {
        let mut cf = CodeFolding::new("func f():\n    pass\n");
        assert!(cf.toggle_fold(0)); // now folded
        assert!(cf.is_folded(0));
        assert!(!cf.toggle_fold(0)); // now unfolded
        assert!(!cf.is_folded(0));

        // Line 1 ("    pass") is not a header — fold is a no-op.
        cf.fold(1);
        assert!(!cf.is_folded(1));
    }
}
