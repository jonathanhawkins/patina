//! Script-editor **go-to-line** navigation (pat-9mpy2).
//!
//! The go-to-line action moves the caret to a requested 1-based line number,
//! clamping out-of-range input to the valid `[1, line_count]` range, and scrolls
//! the viewport so the target line is centered (as far as the document bounds
//! allow).

/// Tracks the caret line and viewport scroll for go-to-line navigation.
#[derive(Debug, Clone)]
pub struct GotoLine {
    line_count: usize,
    viewport_lines: usize,
    caret_line: usize,
    top_line: usize,
}

impl GotoLine {
    /// Creates a navigator for a document of `line_count` lines (at least one)
    /// shown through a viewport `viewport_lines` tall. The caret starts on line 1.
    pub fn new(line_count: usize, viewport_lines: usize) -> Self {
        let line_count = line_count.max(1);
        let viewport_lines = viewport_lines.max(1);
        let mut g = Self {
            line_count,
            viewport_lines,
            caret_line: 1,
            top_line: 1,
        };
        g.center();
        g
    }

    /// The total number of lines.
    pub fn line_count(&self) -> usize {
        self.line_count
    }

    /// The current caret line (1-based).
    pub fn caret_line(&self) -> usize {
        self.caret_line
    }

    /// The first visible line (1-based scroll position).
    pub fn top_line(&self) -> usize {
        self.top_line
    }

    /// The last visible line (1-based).
    pub fn bottom_line(&self) -> usize {
        (self.top_line + self.viewport_lines - 1).min(self.line_count)
    }

    /// Whether `line` is currently within the visible viewport.
    pub fn is_visible(&self, line: usize) -> bool {
        line >= self.top_line && line <= self.bottom_line()
    }

    /// The largest valid value for `top_line` (the scroll position that shows
    /// the last line at the bottom of the viewport).
    fn max_top(&self) -> usize {
        self.line_count.saturating_sub(self.viewport_lines) + 1
    }

    /// Scrolls so the caret line is centered, clamped to the document bounds.
    fn center(&mut self) {
        let half = self.viewport_lines / 2;
        let desired_top = self.caret_line.saturating_sub(half).max(1);
        self.top_line = desired_top.clamp(1, self.max_top());
    }

    /// Moves the caret to `requested` (1-based), clamping out-of-range values to
    /// `[1, line_count]`, and centers the line in view. Returns the resulting
    /// caret line.
    pub fn goto(&mut self, requested: i64) -> usize {
        let clamped = requested.clamp(1, self.line_count as i64) as usize;
        self.caret_line = clamped;
        self.center();
        clamped
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Acceptance (pat-9mpy2): go-to-line moves the caret to the requested line,
    /// clamps out-of-range input, and centers the line in view.
    #[test]
    fn script_nav_goto_line() {
        // 100-line document, 20-line viewport.
        let mut g = GotoLine::new(100, 20);

        // Jump to the middle: caret lands exactly, and the line is centered.
        assert_eq!(g.goto(50), 50);
        assert_eq!(g.caret_line(), 50);
        // Centered: 10 lines above the caret are visible (top = 50 - 20/2).
        assert_eq!(g.top_line(), 40);
        assert!(g.is_visible(50));
        // Roughly centered within the viewport.
        assert_eq!(g.caret_line() - g.top_line(), 10);

        // Jumping near the top clamps the scroll to line 1 (can't center past it).
        assert_eq!(g.goto(3), 3);
        assert_eq!(g.top_line(), 1);
        assert!(g.is_visible(3));

        // Jumping near the bottom clamps the scroll so the last line stays in view.
        assert_eq!(g.goto(98), 98);
        assert_eq!(g.top_line(), 81); // max_top = 100 - 20 + 1
        assert_eq!(g.bottom_line(), 100);
        assert!(g.is_visible(98));

        // Out-of-range high clamps to the last line.
        assert_eq!(g.goto(1000), 100);
        assert_eq!(g.caret_line(), 100);
        assert!(g.is_visible(100));

        // Out-of-range low (zero / negative) clamps to line 1.
        assert_eq!(g.goto(0), 1);
        assert_eq!(g.caret_line(), 1);
        assert_eq!(g.goto(-25), 1);
        assert_eq!(g.caret_line(), 1);
    }

    /// A document shorter than the viewport never scrolls.
    #[test]
    fn short_document_never_scrolls() {
        let mut g = GotoLine::new(5, 20);
        assert_eq!(g.goto(4), 4);
        assert_eq!(g.top_line(), 1);
        assert_eq!(g.bottom_line(), 5);
        assert!(g.is_visible(4));
        // Out-of-range still clamps to the 5 available lines.
        assert_eq!(g.goto(99), 5);
        assert_eq!(g.top_line(), 1);
    }
}
