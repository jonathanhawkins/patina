//! The **Output console**.
//!
//! The Output console captures lines emitted while the game and editor run —
//! ordinary prints plus warnings and errors. Each line is tagged with a
//! severity so the UI can style it (e.g. warnings amber, errors red). A
//! per-severity filter hides matching lines without discarding them, and
//! **Clear** empties the log entirely.
//!
//! This is a focused, self-contained model of the console's capture/style/
//! filter/clear behavior, kept separate from the richer `output_panel` so the
//! two can evolve independently.

/// Severity of a captured output line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputSeverity {
    /// An ordinary `print()` line.
    Print,
    /// A warning line.
    Warning,
    /// An error line.
    Error,
}

impl OutputSeverity {
    /// All severities, in display order.
    pub fn all() -> [OutputSeverity; 3] {
        [
            OutputSeverity::Print,
            OutputSeverity::Warning,
            OutputSeverity::Error,
        ]
    }

    /// The style class used to render lines of this severity.
    pub fn style(self) -> &'static str {
        match self {
            OutputSeverity::Print => "output-print",
            OutputSeverity::Warning => "output-warning",
            OutputSeverity::Error => "output-error",
        }
    }
}

/// A single captured output line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputLine {
    /// Severity of the line.
    pub severity: OutputSeverity,
    /// The captured text.
    pub text: String,
}

impl OutputLine {
    /// The style class for rendering this line.
    pub fn style(&self) -> &'static str {
        self.severity.style()
    }
}

/// The Output console: captured lines plus the per-severity visibility filter.
#[derive(Debug, Clone)]
pub struct OutputConsole {
    lines: Vec<OutputLine>,
    /// Severities currently hidden by the filter.
    hidden: Vec<OutputSeverity>,
}

impl Default for OutputConsole {
    fn default() -> Self {
        Self {
            lines: Vec::new(),
            hidden: Vec::new(),
        }
    }
}

impl OutputConsole {
    /// Creates an empty console with every severity visible.
    pub fn new() -> Self {
        Self::default()
    }

    /// Captures a line of the given `severity`.
    pub fn capture(&mut self, severity: OutputSeverity, text: impl Into<String>) {
        self.lines.push(OutputLine {
            severity,
            text: text.into(),
        });
    }

    /// Captures an ordinary print line.
    pub fn print(&mut self, text: impl Into<String>) {
        self.capture(OutputSeverity::Print, text);
    }

    /// Captures a warning line.
    pub fn warning(&mut self, text: impl Into<String>) {
        self.capture(OutputSeverity::Warning, text);
    }

    /// Captures an error line.
    pub fn error(&mut self, text: impl Into<String>) {
        self.capture(OutputSeverity::Error, text);
    }

    /// Every captured line, regardless of the filter.
    pub fn all_lines(&self) -> &[OutputLine] {
        &self.lines
    }

    /// The total number of captured lines.
    pub fn len(&self) -> usize {
        self.lines.len()
    }

    /// Whether the console has no captured lines.
    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    /// Whether lines of `severity` are currently shown.
    pub fn is_severity_visible(&self, severity: OutputSeverity) -> bool {
        !self.hidden.contains(&severity)
    }

    /// Shows or hides all lines of `severity`. Hidden lines are filtered from
    /// [`visible_lines`](Self::visible_lines) but remain captured.
    pub fn set_severity_visible(&mut self, severity: OutputSeverity, visible: bool) {
        if visible {
            self.hidden.retain(|s| *s != severity);
        } else if !self.hidden.contains(&severity) {
            self.hidden.push(severity);
        }
    }

    /// The lines currently shown: those whose severity is not filtered out, in
    /// capture order.
    pub fn visible_lines(&self) -> Vec<&OutputLine> {
        self.lines
            .iter()
            .filter(|line| self.is_severity_visible(line.severity))
            .collect()
    }

    /// Clears the log, emptying every captured line. The filter is left
    /// unchanged.
    pub fn clear(&mut self) {
        self.lines.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Acceptance (pat-eixao): running prints appear in Output, error/warning
    /// lines are styled by severity, the severity filter hides matching lines,
    /// and Clear empties the log.
    #[test]
    fn bottom_output_console_captures_and_filters() {
        let mut console = OutputConsole::new();
        assert!(console.is_empty());

        // Running prints appear in the Output console.
        console.print("Game started");
        console.warning("Texture not found, using placeholder");
        console.error("Null instance in _process()");
        console.print("Tick 1");

        assert_eq!(console.len(), 4);
        // By default every severity is visible.
        assert_eq!(console.visible_lines().len(), 4);

        // Lines are styled by severity.
        let lines = console.all_lines();
        assert_eq!(lines[0].style(), "output-print");
        assert_eq!(lines[1].style(), "output-warning");
        assert_eq!(lines[2].style(), "output-error");
        assert_eq!(OutputSeverity::Error.style(), "output-error");

        // The severity filter hides matching lines (without discarding them).
        console.set_severity_visible(OutputSeverity::Warning, false);
        assert!(!console.is_severity_visible(OutputSeverity::Warning));
        let visible = console.visible_lines();
        assert_eq!(visible.len(), 3, "the one warning line is hidden");
        assert!(visible.iter().all(|l| l.severity != OutputSeverity::Warning));
        // Hidden lines are still captured.
        assert_eq!(console.len(), 4);

        // Hiding errors too leaves only the two prints.
        console.set_severity_visible(OutputSeverity::Error, false);
        let prints = console.visible_lines();
        assert_eq!(prints.len(), 2);
        assert!(prints.iter().all(|l| l.severity == OutputSeverity::Print));

        // Re-showing a severity brings its lines back.
        console.set_severity_visible(OutputSeverity::Warning, true);
        assert_eq!(console.visible_lines().len(), 3);

        // Clear empties the log entirely.
        console.clear();
        assert!(console.is_empty());
        assert_eq!(console.visible_lines().len(), 0);
    }
}
