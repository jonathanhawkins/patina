//! The Debugger panel's **Errors tab**.
//!
//! While the game runs, runtime errors and warnings are collected here. Each
//! entry shows its severity, message, and source location (file + line).
//! Activating an entry — double-clicking it or pressing its jump-to-source
//! action — navigates the script editor to that line.

/// Severity of a reported runtime entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorSeverity {
    /// A runtime error.
    Error,
    /// A runtime warning.
    Warning,
}

/// Where activating an entry should navigate the editor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceLocation {
    /// Source file (e.g. `"res://player.gd"`).
    pub file: String,
    /// 1-based line number.
    pub line: u32,
}

/// A single runtime error/warning in the Errors tab.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ErrorEntry {
    /// Severity (error or warning).
    pub severity: ErrorSeverity,
    /// Human-readable message.
    pub message: String,
    /// Source location the entry points at.
    pub location: SourceLocation,
}

impl ErrorEntry {
    /// The file this entry points at.
    pub fn file(&self) -> &str {
        &self.location.file
    }

    /// The line this entry points at.
    pub fn line(&self) -> u32 {
        self.location.line
    }
}

/// The Errors tab: the collected entries plus the most recent navigation.
#[derive(Debug, Clone, Default)]
pub struct DebuggerErrorsTab {
    entries: Vec<ErrorEntry>,
    selected: Option<usize>,
    last_navigation: Option<SourceLocation>,
}

impl DebuggerErrorsTab {
    /// Creates an empty Errors tab.
    pub fn new() -> Self {
        Self::default()
    }

    /// Reports a runtime entry of the given `severity`, returning its index.
    pub fn report(
        &mut self,
        severity: ErrorSeverity,
        message: impl Into<String>,
        file: impl Into<String>,
        line: u32,
    ) -> usize {
        self.entries.push(ErrorEntry {
            severity,
            message: message.into(),
            location: SourceLocation {
                file: file.into(),
                line,
            },
        });
        self.entries.len() - 1
    }

    /// Reports a runtime error.
    pub fn report_error(
        &mut self,
        message: impl Into<String>,
        file: impl Into<String>,
        line: u32,
    ) -> usize {
        self.report(ErrorSeverity::Error, message, file, line)
    }

    /// Reports a runtime warning.
    pub fn report_warning(
        &mut self,
        message: impl Into<String>,
        file: impl Into<String>,
        line: u32,
    ) -> usize {
        self.report(ErrorSeverity::Warning, message, file, line)
    }

    /// All entries, in arrival order.
    pub fn entries(&self) -> &[ErrorEntry] {
        &self.entries
    }

    /// The number of entries.
    pub fn count(&self) -> usize {
        self.entries.len()
    }

    /// The number of entries matching `severity`.
    pub fn count_of(&self, severity: ErrorSeverity) -> usize {
        self.entries.iter().filter(|e| e.severity == severity).count()
    }

    /// Activates (jumps to source for) the entry at `index`: selects it and
    /// returns the location to navigate to, recording it as the last
    /// navigation. Returns `None` for an out-of-range index.
    pub fn activate(&mut self, index: usize) -> Option<SourceLocation> {
        let entry = self.entries.get(index)?;
        let location = entry.location.clone();
        self.selected = Some(index);
        self.last_navigation = Some(location.clone());
        Some(location)
    }

    /// The index of the selected entry, if any.
    pub fn selected_index(&self) -> Option<usize> {
        self.selected
    }

    /// The most recent navigation target produced by [`activate`](Self::activate).
    pub fn last_navigation(&self) -> Option<&SourceLocation> {
        self.last_navigation.as_ref()
    }

    /// Clears all entries, the selection, and the last navigation.
    pub fn clear(&mut self) {
        self.entries.clear();
        self.selected = None;
        self.last_navigation = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Acceptance (pat-muthx): a runtime error appears in the Errors tab with
    /// its source location, and activating it navigates to that line.
    #[test]
    fn bottom_debugger_errors_tab_lists_and_navigates() {
        let mut tab = DebuggerErrorsTab::new();
        assert_eq!(tab.count(), 0);
        assert!(tab.last_navigation().is_none());

        // A runtime error appears with its message and source location.
        tab.report_error("Invalid get index 'x' on null instance", "res://player.gd", 42);
        tab.report_warning("Unused variable 'tmp'", "res://enemy.gd", 8);

        assert_eq!(tab.count(), 2);
        assert_eq!(tab.count_of(ErrorSeverity::Error), 1);
        assert_eq!(tab.count_of(ErrorSeverity::Warning), 1);

        let first = &tab.entries()[0];
        assert_eq!(first.severity, ErrorSeverity::Error);
        assert_eq!(first.message, "Invalid get index 'x' on null instance");
        assert_eq!(first.file(), "res://player.gd");
        assert_eq!(first.line(), 42);

        // Activating the error navigates to its source line.
        let nav = tab.activate(0).expect("activating a real entry navigates");
        assert_eq!(
            nav,
            SourceLocation {
                file: "res://player.gd".to_string(),
                line: 42,
            }
        );
        assert_eq!(tab.selected_index(), Some(0));
        assert_eq!(tab.last_navigation(), Some(&nav));

        // Activating the warning navigates to its own location.
        let nav2 = tab.activate(1).expect("activating the warning navigates");
        assert_eq!(nav2.file, "res://enemy.gd");
        assert_eq!(nav2.line, 8);
        assert_eq!(tab.last_navigation(), Some(&nav2));

        // Out-of-range activation does nothing and leaves state intact.
        assert!(tab.activate(9).is_none());
        assert_eq!(tab.selected_index(), Some(1));

        // Clearing empties the tab and forgets navigation.
        tab.clear();
        assert_eq!(tab.count(), 0);
        assert!(tab.last_navigation().is_none());
        assert!(tab.selected_index().is_none());
    }
}
