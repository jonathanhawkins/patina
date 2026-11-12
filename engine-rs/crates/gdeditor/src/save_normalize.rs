//! Saving and whitespace normalization for the script editor.
//!
//! When the user saves, the buffer is persisted. If whitespace normalization is
//! enabled in the editor settings, the persisted text has trailing whitespace
//! trimmed from every line and a single final newline enforced. The pre-save
//! buffer is pushed onto an undo stack, so undo restores exactly what was on
//! screen before the save (including any whitespace the save removed).

/// Whitespace-normalization options applied on save.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SaveSettings {
    /// Trim trailing spaces/tabs from each line.
    pub trim_trailing_whitespace: bool,
    /// Ensure the file ends with exactly one newline (collapsing trailing blank
    /// lines). Empty content stays empty.
    pub ensure_final_newline: bool,
}

impl SaveSettings {
    /// Both normalizations enabled (the common editor default).
    pub fn normalized() -> Self {
        Self {
            trim_trailing_whitespace: true,
            ensure_final_newline: true,
        }
    }

    /// No normalization — content is persisted verbatim.
    pub fn verbatim() -> Self {
        Self {
            trim_trailing_whitespace: false,
            ensure_final_newline: false,
        }
    }
}

/// Returns `text` with the given normalizations applied.
///
/// - `trim_trailing_whitespace`: each line has trailing whitespace removed.
/// - `ensure_final_newline`: trailing blank lines collapse to a single `\n`;
///   non-empty content that lacks a final newline gains one. Empty (or
///   all-blank) content normalizes to the empty string.
pub fn normalize(text: &str, settings: &SaveSettings) -> String {
    let mut lines: Vec<String> = text.split('\n').map(|l| l.to_string()).collect();
    if settings.trim_trailing_whitespace {
        for line in &mut lines {
            *line = line.trim_end().to_string();
        }
    }
    let mut out = lines.join("\n");
    if settings.ensure_final_newline {
        let trimmed = out.trim_end_matches('\n');
        out = if trimmed.is_empty() {
            String::new()
        } else {
            format!("{}\n", trimmed)
        };
    }
    out
}

/// A savable text buffer with an undo stack of pre-save states.
#[derive(Debug, Clone)]
pub struct SaveBuffer {
    content: String,
    saved: Option<String>,
    undo_stack: Vec<String>,
}

impl SaveBuffer {
    /// Creates a buffer with the given initial content (unsaved).
    pub fn new(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            saved: None,
            undo_stack: Vec::new(),
        }
    }

    /// The current buffer content.
    pub fn content(&self) -> &str {
        &self.content
    }

    /// The last persisted content, if the buffer has been saved.
    pub fn saved(&self) -> Option<&str> {
        self.saved.as_deref()
    }

    /// Whether the buffer matches what was last persisted.
    pub fn is_saved(&self) -> bool {
        self.saved.as_deref() == Some(self.content.as_str())
    }

    /// Replaces the buffer content (an edit). Does not touch the undo stack used
    /// by save; callers manage edit-level undo separately.
    pub fn set_content(&mut self, content: impl Into<String>) {
        self.content = content.into();
    }

    /// Persists the buffer, applying `settings` normalization. The pre-save
    /// content is pushed onto the undo stack. Returns the persisted text.
    pub fn save(&mut self, settings: &SaveSettings) -> String {
        self.undo_stack.push(self.content.clone());
        let normalized = normalize(&self.content, settings);
        self.content = normalized.clone();
        self.saved = Some(normalized.clone());
        normalized
    }

    /// Restores the buffer to the state before the most recent save. Returns
    /// `false` if there is nothing to undo.
    pub fn undo(&mut self) -> bool {
        if let Some(prev) = self.undo_stack.pop() {
            self.content = prev;
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Acceptance (pat-mox9t): save persists the script; with normalization on it
    /// trims trailing whitespace and ensures a single final newline; undo
    /// restores the pre-save buffer.
    #[test]
    fn script_core_save_and_trim() {
        let mut buf = SaveBuffer::new("func f():  \n    return 1   \n\n\n");
        let settings = SaveSettings::normalized();

        // Save normalizes: trailing whitespace trimmed, single final newline.
        let persisted = buf.save(&settings);
        assert_eq!(persisted, "func f():\n    return 1\n");
        assert_eq!(buf.content(), "func f():\n    return 1\n");
        assert_eq!(buf.saved(), Some("func f():\n    return 1\n"));
        assert!(buf.is_saved());

        // Undo restores the exact pre-save buffer (original whitespace intact).
        assert!(buf.undo());
        assert_eq!(buf.content(), "func f():  \n    return 1   \n\n\n");
        // Nothing left to undo.
        assert!(!buf.undo());

        // With normalization disabled, save persists verbatim.
        let mut buf2 = SaveBuffer::new("a = 1   \n");
        assert_eq!(buf2.save(&SaveSettings::verbatim()), "a = 1   \n");
        assert_eq!(buf2.content(), "a = 1   \n");

        // normalize(): trims each line and collapses trailing blanks to one \n.
        assert_eq!(normalize("x  \ny\t\n\n\n", &settings), "x\ny\n");
        // ensure_final_newline adds a newline when missing.
        assert_eq!(normalize("noeol", &settings), "noeol\n");
        // Empty / all-blank content stays empty (no spurious newline).
        assert_eq!(normalize("", &settings), "");
        assert_eq!(normalize("   \n\n", &settings), "");
    }

    /// The two normalization toggles are independent.
    #[test]
    fn normalize_toggles_independent() {
        // Only trim: trailing spaces gone, but blank-line structure preserved.
        let trim_only = SaveSettings {
            trim_trailing_whitespace: true,
            ensure_final_newline: false,
        };
        assert_eq!(normalize("a  \nb \n\n", &trim_only), "a\nb\n\n");

        // Only final newline: trailing blanks collapse, inner whitespace kept.
        let eol_only = SaveSettings {
            trim_trailing_whitespace: false,
            ensure_final_newline: true,
        };
        assert_eq!(normalize("a \nb \n\n\n", &eol_only), "a \nb \n");
    }

    /// is_saved reflects edits made after a save.
    #[test]
    fn save_buffer_dirty_tracking() {
        let mut buf = SaveBuffer::new("x = 1\n");
        assert!(!buf.is_saved()); // never saved
        buf.save(&SaveSettings::normalized());
        assert!(buf.is_saved());
        buf.set_content("x = 2\n");
        assert!(!buf.is_saved()); // edited since save
    }
}
