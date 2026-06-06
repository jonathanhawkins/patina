//! Auto-indentation for the script editor.
//!
//! When the user presses Enter, the new line should start at a sensible indent:
//! it inherits the previous line's leading whitespace, and if the previous line
//! opens a block (a GDScript statement ending in `:`) it adds one further indent
//! level. The indent unit honors the configured tab/space style.

/// The configured indentation unit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndentStyle {
    /// Indent with this many spaces per level.
    Spaces(usize),
    /// Indent with a tab per level.
    Tabs,
}

impl IndentStyle {
    /// The whitespace string for one indent level.
    pub fn unit(self) -> String {
        match self {
            IndentStyle::Spaces(n) => " ".repeat(n),
            IndentStyle::Tabs => "\t".to_string(),
        }
    }
}

/// The leading whitespace (spaces/tabs) of `line`.
fn leading_whitespace(line: &str) -> &str {
    let end = line
        .find(|c: char| c != ' ' && c != '\t')
        .unwrap_or(line.len());
    &line[..end]
}

/// The code portion of `line` with a trailing `#` comment removed. (Naive: the
/// first `#` ends the code — good enough for indent decisions.)
fn code_part(line: &str) -> &str {
    match line.find('#') {
        Some(i) => &line[..i],
        None => line,
    }
}

/// Whether `line` opens a new block (its code ends with `:`).
pub fn opens_block(line: &str) -> bool {
    code_part(line).trim_end().ends_with(':')
}

/// Computes auto-indentation for newly inserted lines.
#[derive(Debug, Clone, Copy)]
pub struct AutoIndenter {
    style: IndentStyle,
}

impl AutoIndenter {
    /// Creates an indenter for the given style.
    pub fn new(style: IndentStyle) -> Self {
        Self { style }
    }

    /// The configured indent style.
    pub fn style(&self) -> IndentStyle {
        self.style
    }

    /// The indentation string for a line inserted directly after `prev_line`.
    /// Inherits the previous line's indent, plus one level if it opens a block.
    pub fn indent_for_new_line(&self, prev_line: &str) -> String {
        let base = leading_whitespace(prev_line);
        if opens_block(prev_line) {
            format!("{}{}", base, self.style.unit())
        } else {
            base.to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Acceptance (pat-xyger): new lines inherit the previous line's indentation
    /// and add one level after a block opener, in the configured indent style.
    #[test]
    fn script_core_auto_indent() {
        // 4-space indent style.
        let spaces = AutoIndenter::new(IndentStyle::Spaces(4));

        // A block opener at column 0 → one level (4 spaces).
        assert_eq!(spaces.indent_for_new_line("func _ready():"), "    ");
        // A plain statement inherits its own indentation.
        assert_eq!(spaces.indent_for_new_line("    var x = 1"), "    ");
        // A nested block opener → previous indent + one level.
        assert_eq!(spaces.indent_for_new_line("    if x > 0:"), "        ");
        // A statement inside the nested block keeps that indentation.
        assert_eq!(spaces.indent_for_new_line("        return x"), "        ");
        // An empty line → no indent.
        assert_eq!(spaces.indent_for_new_line(""), "");
        // A trailing comment after the colon still counts as a block opener.
        assert_eq!(spaces.indent_for_new_line("    if y: # guard"), "        ");
        // A colon inside a comment does NOT open a block.
        assert_eq!(spaces.indent_for_new_line("    var z = 1 # ratio a:b"), "    ");

        // Tab indent style.
        let tabs = AutoIndenter::new(IndentStyle::Tabs);
        assert_eq!(tabs.indent_for_new_line("\tif ready:"), "\t\t");
        assert_eq!(tabs.indent_for_new_line("\tpass"), "\t");
        assert_eq!(tabs.indent_for_new_line("for i in items:"), "\t");
    }

    /// `opens_block` recognizes colon-terminated code, ignoring comments and
    /// trailing whitespace.
    #[test]
    fn opens_block_detection() {
        assert!(opens_block("while true:"));
        assert!(opens_block("else:   "));
        assert!(opens_block("match v: # dispatch"));
        assert!(!opens_block("var a = 1"));
        assert!(!opens_block("# just a comment:"));
        assert!(!opens_block(""));
    }
}
