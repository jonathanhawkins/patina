//! Line operations for the script editor: duplicate, move up/down, and delete.
//!
//! Each operation works on a *block* of lines — a `start..=end` inclusive range
//! of 0-based line indices. A bare caret on a single line is the degenerate case
//! `start == end`; a multi-line selection spans `start < end`. Every operation
//! returns the new buffer together with the line the caret should land on, so
//! the editor can keep the cursor on the affected text:
//!
//! - **duplicate** copies the block immediately below itself; the caret moves to
//!   the first line of the inserted copy.
//! - **move up / move down** shift the block one line in that direction (no-op at
//!   the top/bottom edge); the caret follows the block.
//! - **delete** removes the block; the caret stays at the same index, clamped to
//!   the line that slid into its place. Deleting every line leaves one empty line.

/// The outcome of a line operation: the rewritten buffer and the caret's line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineOpResult {
    /// The lines after the operation.
    pub lines: Vec<String>,
    /// The 0-based line the caret should occupy afterwards.
    pub caret_line: usize,
}

/// Clamps a `start..=end` block to the valid line range, returning `(start, end)`
/// with `start <= end < lines.len()`. Returns `None` for an empty buffer.
fn clamp_block(len: usize, start: usize, end: usize) -> Option<(usize, usize)> {
    if len == 0 {
        return None;
    }
    let start = start.min(len - 1);
    let end = end.min(len - 1).max(start);
    Some((start, end))
}

/// Duplicates the `start..=end` block, inserting the copy directly below it. The
/// caret lands on the first line of the inserted copy.
pub fn duplicate_lines(lines: &[String], start: usize, end: usize) -> LineOpResult {
    let Some((start, end)) = clamp_block(lines.len(), start, end) else {
        return LineOpResult {
            lines: vec![String::new()],
            caret_line: 0,
        };
    };
    let mut out = Vec::with_capacity(lines.len() + (end - start + 1));
    out.extend_from_slice(&lines[..=end]);
    out.extend_from_slice(&lines[start..=end]);
    out.extend_from_slice(&lines[end + 1..]);
    LineOpResult {
        lines: out,
        caret_line: end + 1,
    }
}

/// Moves the `start..=end` block up one line. No-op when the block is already at
/// the top. The caret follows the block to its new starting line.
pub fn move_lines_up(lines: &[String], start: usize, end: usize) -> LineOpResult {
    let Some((start, end)) = clamp_block(lines.len(), start, end) else {
        return LineOpResult {
            lines: vec![String::new()],
            caret_line: 0,
        };
    };
    if start == 0 {
        return LineOpResult {
            lines: lines.to_vec(),
            caret_line: start,
        };
    }
    let mut out = lines.to_vec();
    let block: Vec<String> = out.drain(start..=end).collect();
    out.splice((start - 1)..(start - 1), block);
    LineOpResult {
        lines: out,
        caret_line: start - 1,
    }
}

/// Moves the `start..=end` block down one line. No-op when the block is already
/// at the bottom. The caret follows the block to its new starting line.
pub fn move_lines_down(lines: &[String], start: usize, end: usize) -> LineOpResult {
    let Some((start, end)) = clamp_block(lines.len(), start, end) else {
        return LineOpResult {
            lines: vec![String::new()],
            caret_line: 0,
        };
    };
    if end + 1 >= lines.len() {
        return LineOpResult {
            lines: lines.to_vec(),
            caret_line: start,
        };
    }
    let mut out = lines.to_vec();
    let block: Vec<String> = out.drain(start..=end).collect();
    out.splice((start + 1)..(start + 1), block);
    LineOpResult {
        lines: out,
        caret_line: start + 1,
    }
}

/// Deletes the `start..=end` block. The caret stays at `start`, clamped to the
/// new last line. Deleting every line leaves a single empty line, caret at 0.
pub fn delete_lines(lines: &[String], start: usize, end: usize) -> LineOpResult {
    let Some((start, end)) = clamp_block(lines.len(), start, end) else {
        return LineOpResult {
            lines: vec![String::new()],
            caret_line: 0,
        };
    };
    let mut out = lines.to_vec();
    out.drain(start..=end);
    if out.is_empty() {
        return LineOpResult {
            lines: vec![String::new()],
            caret_line: 0,
        };
    }
    let caret_line = start.min(out.len() - 1);
    LineOpResult {
        lines: out,
        caret_line,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vec_lines(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    fn text(lines: &[String]) -> String {
        lines.join("\n")
    }

    /// Acceptance (pat-2ulga): duplicate copies the line/selection, move
    /// up/down reorders it, delete removes it, each updating the caret line
    /// consistently.
    #[test]
    fn script_core_line_operations() {
        let lines = vec_lines(&["a", "b", "c"]);

        // Duplicate the caret line ("b"): copy inserted below; caret on the copy.
        let r = duplicate_lines(&lines, 1, 1);
        assert_eq!(text(&r.lines), "a\nb\nb\nc");
        assert_eq!(r.caret_line, 2);

        // Duplicate a multi-line selection (lines 0..=1).
        let r = duplicate_lines(&lines, 0, 1);
        assert_eq!(text(&r.lines), "a\nb\na\nb\nc");
        assert_eq!(r.caret_line, 2);

        // Move line up: "c" moves above "b".
        let r = move_lines_up(&lines, 2, 2);
        assert_eq!(text(&r.lines), "a\nc\nb");
        assert_eq!(r.caret_line, 1);

        // Move up at the top is a no-op.
        let r = move_lines_up(&lines, 0, 0);
        assert_eq!(text(&r.lines), "a\nb\nc");
        assert_eq!(r.caret_line, 0);

        // Move line down: "a" moves below "b".
        let r = move_lines_down(&lines, 0, 0);
        assert_eq!(text(&r.lines), "b\na\nc");
        assert_eq!(r.caret_line, 1);

        // Move a selection down: lines 0..=1 ("a","b") move below "c".
        let r = move_lines_down(&lines, 0, 1);
        assert_eq!(text(&r.lines), "c\na\nb");
        assert_eq!(r.caret_line, 1);

        // Move down at the bottom is a no-op.
        let r = move_lines_down(&lines, 2, 2);
        assert_eq!(text(&r.lines), "a\nb\nc");
        assert_eq!(r.caret_line, 2);

        // Delete the caret line ("b"): caret stays at index 1 (now "c").
        let r = delete_lines(&lines, 1, 1);
        assert_eq!(text(&r.lines), "a\nc");
        assert_eq!(r.caret_line, 1);

        // Delete the last line: caret clamps to the new last line.
        let r = delete_lines(&lines, 2, 2);
        assert_eq!(text(&r.lines), "a\nb");
        assert_eq!(r.caret_line, 1);

        // Deleting every line leaves a single empty line with caret at 0.
        let r = delete_lines(&lines, 0, 2);
        assert_eq!(text(&r.lines), "");
        assert_eq!(r.caret_line, 0);
    }

    /// Out-of-range indices are clamped rather than panicking.
    #[test]
    fn line_ops_clamp_out_of_range() {
        let lines = vec_lines(&["x", "y"]);

        // end past the last line clamps to the last line.
        let r = duplicate_lines(&lines, 1, 99);
        assert_eq!(text(&r.lines), "x\ny\ny");
        assert_eq!(r.caret_line, 2);

        // start past the end clamps to the last line for delete.
        let r = delete_lines(&lines, 5, 5);
        assert_eq!(text(&r.lines), "x");
        assert_eq!(r.caret_line, 0);

        // An empty buffer yields a single empty line.
        let empty: Vec<String> = Vec::new();
        let r = move_lines_down(&empty, 0, 0);
        assert_eq!(r.lines, vec![String::new()]);
        assert_eq!(r.caret_line, 0);
    }
}
