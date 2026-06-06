//! **Block indent / unindent** for the script editor.
//!
//! With one or more lines selected, pressing Tab indents every selected line by
//! one level and Shift+Tab unindents each by one level. Indentation honors the
//! configured indent style (a number of spaces, or a tab). Unindent removes at
//! most one level of leading whitespace and is a no-op for a line already at
//! column zero, so repeatedly unindenting never eats into the line's text.
//!
//! The indent unit is shared with the auto-indenter so both features agree on
//! what "one level" means.

use crate::auto_indent::IndentStyle;

/// Indents and unindents blocks of lines using a fixed indent style.
#[derive(Debug, Clone, Copy)]
pub struct BlockIndenter {
    style: IndentStyle,
}

impl BlockIndenter {
    /// Creates a block indenter for the given style.
    pub fn new(style: IndentStyle) -> Self {
        Self { style }
    }

    /// The configured indent style.
    pub fn style(&self) -> IndentStyle {
        self.style
    }

    /// Indents one line by one level. Empty lines are left untouched (so a Tab
    /// across a selection doesn't sprinkle trailing whitespace on blank lines).
    fn indent_line(&self, line: &str) -> String {
        if line.is_empty() {
            line.to_string()
        } else {
            format!("{}{}", self.style.unit(), line)
        }
    }

    /// Unindents one line by at most one level, preserving the rest of its
    /// leading whitespace. A line with no leading whitespace is unchanged.
    fn unindent_line(&self, line: &str) -> String {
        match self.style {
            IndentStyle::Tabs => line.strip_prefix('\t').unwrap_or(line).to_string(),
            IndentStyle::Spaces(width) => {
                let mut end = 0;
                for (i, c) in line.char_indices() {
                    if c == ' ' && i < width {
                        end = i + 1;
                    } else {
                        break;
                    }
                }
                line[end..].to_string()
            }
        }
    }

    /// Indents every line in the selection by one level.
    pub fn indent<S: AsRef<str>>(&self, lines: &[S]) -> Vec<String> {
        lines.iter().map(|l| self.indent_line(l.as_ref())).collect()
    }

    /// Unindents every line in the selection by one level.
    pub fn unindent<S: AsRef<str>>(&self, lines: &[S]) -> Vec<String> {
        lines
            .iter()
            .map(|l| self.unindent_line(l.as_ref()))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Acceptance (pat-6ochk): indent adds one indent level to each selected
    /// line and unindent removes one (no-op at column zero), using the
    /// configured indent width.
    #[test]
    fn script_core_indent_unindent() {
        // 4-space indent style.
        let spaces = BlockIndenter::new(IndentStyle::Spaces(4));

        let original = vec![
            "func _ready():".to_string(),
            "    var hp = 10".to_string(),
            "".to_string(),
            "    return hp".to_string(),
        ];

        // Indent adds 4 spaces to each non-empty line; the blank line is left be.
        let indented = spaces.indent(&original);
        assert_eq!(
            indented,
            vec!["    func _ready():", "        var hp = 10", "", "        return hp"]
        );

        // Unindent removes exactly one level, restoring the original (round-trip).
        let unindented = spaces.unindent(&indented);
        assert_eq!(unindented, original);

        // Unindent at column zero is a no-op; it never removes non-whitespace.
        assert_eq!(
            spaces.unindent(&["func _ready():".to_string()]),
            vec!["func _ready():"]
        );

        // A line indented by fewer than a full level loses only what's there.
        assert_eq!(spaces.unindent(&["  x = 1".to_string()]), vec!["x = 1"]);
        // Deeper-than-one-level indentation only drops a single level.
        assert_eq!(
            spaces.unindent(&["        deep".to_string()]),
            vec!["    deep"]
        );

        // Tab indent style: one tab per level.
        let tabs = BlockIndenter::new(IndentStyle::Tabs);
        let block = vec!["a = 1".to_string(), "\tb = 2".to_string()];
        let tindent = tabs.indent(&block);
        assert_eq!(tindent, vec!["\ta = 1", "\t\tb = 2"]);
        assert_eq!(tabs.unindent(&tindent), block);
        // No-op at column zero for tabs as well.
        assert_eq!(tabs.unindent(&["pass".to_string()]), vec!["pass"]);
    }
}
