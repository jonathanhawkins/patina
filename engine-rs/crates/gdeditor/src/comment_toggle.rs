//! **Toggle-comment** for the script editor.
//!
//! The comment-toggle action operates on the block of currently selected lines.
//! If every non-blank selected line is already commented, the action removes the
//! line-comment token from each (uncomment). Otherwise it prefixes every
//! non-blank line with the token (comment). In both directions the leading
//! indentation is preserved, so commenting an indented line keeps its indent and
//! inserts the token immediately after it.
//!
//! GDScript's line-comment token is `#`. When commenting, the token is inserted
//! as `# ` (with a trailing space) after the indentation; when uncommenting, a
//! leading `#` and one optional following space are removed. This makes the
//! operation round-trip: commenting then toggling again restores the original.

/// The GDScript line-comment token.
pub const LINE_COMMENT: &str = "#";

/// The byte index just past `line`'s leading whitespace (spaces/tabs).
fn indent_end(line: &str) -> usize {
    line.find(|c: char| c != ' ' && c != '\t')
        .unwrap_or(line.len())
}

/// Whether `line` is blank (empty or only whitespace).
fn is_blank(line: &str) -> bool {
    line.trim().is_empty()
}

/// Whether `line`'s first non-whitespace character is the comment token.
pub fn is_commented(line: &str) -> bool {
    line[indent_end(line)..].starts_with(LINE_COMMENT)
}

/// Comments one line: inserts `# ` after its indentation. Blank lines are left
/// untouched.
fn comment_line(line: &str) -> String {
    if is_blank(line) {
        return line.to_string();
    }
    let idx = indent_end(line);
    format!("{}{} {}", &line[..idx], LINE_COMMENT, &line[idx..])
}

/// Uncomments one line: removes a leading `#` and one optional following space,
/// preserving indentation. Lines that aren't commented are left untouched.
fn uncomment_line(line: &str) -> String {
    let idx = indent_end(line);
    let rest = &line[idx..];
    match rest.strip_prefix(LINE_COMMENT) {
        Some(after) => {
            let after = after.strip_prefix(' ').unwrap_or(after);
            format!("{}{}", &line[..idx], after)
        }
        None => line.to_string(),
    }
}

/// Toggles comments across `lines` (a selection block) and returns the new
/// lines. If every non-blank line is already commented, all are uncommented;
/// otherwise every non-blank line is commented. Blank lines are preserved.
pub fn toggle_comment<S: AsRef<str>>(lines: &[S]) -> Vec<String> {
    let mut non_blank = lines.iter().map(|l| l.as_ref()).filter(|l| !is_blank(l));
    let mut any = false;
    let all_commented = non_blank.all(|l| {
        any = true;
        is_commented(l)
    });
    // `all()` on an empty iterator is true; treat an all-blank selection as
    // "not all commented" so it stays a no-op rather than trying to uncomment.
    let uncomment = any && all_commented;

    lines
        .iter()
        .map(|l| {
            let line = l.as_ref();
            if uncomment {
                uncomment_line(line)
            } else {
                comment_line(line)
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Acceptance (pat-l3tdi): toggling comments prefixes uncommented selected
    /// lines with the line-comment token and removes it from already-commented
    /// lines, preserving indentation.
    #[test]
    fn script_core_comment_toggle() {
        // A selection of uncommented (and indented) lines.
        let original = vec![
            "func _ready():".to_string(),
            "    var hp = 10".to_string(),
            "    if hp > 0:".to_string(),
            "        print(hp)".to_string(),
        ];

        // First toggle comments every line, keeping indentation.
        let commented = toggle_comment(&original);
        assert_eq!(
            commented,
            vec![
                "# func _ready():",
                "    # var hp = 10",
                "    # if hp > 0:",
                "        # print(hp)",
            ]
        );
        // Every line now reads as commented.
        assert!(commented.iter().all(|l| is_commented(l)));

        // Toggling the now-fully-commented selection uncomments it, restoring
        // the original lines exactly (round-trip).
        let uncommented = toggle_comment(&commented);
        assert_eq!(uncommented, original);

        // A mixed selection (some commented, some not) is treated as "comment
        // all" — the uncommented lines gain a token; already-commented lines
        // are commented again, so a single reverse toggle still round-trips.
        let mixed = vec!["# already".to_string(), "code".to_string()];
        let mixed_commented = toggle_comment(&mixed);
        assert_eq!(mixed_commented, vec!["# # already", "# code"]);
        assert_eq!(toggle_comment(&mixed_commented), mixed);

        // Uncommenting tolerates a token with no following space.
        let tight = vec!["    #tight".to_string()];
        assert_eq!(toggle_comment(&tight), vec!["    tight"]);
    }

    /// Blank lines are preserved by both directions and don't force a toggle.
    #[test]
    fn blank_lines_preserved() {
        // A selection that is only blank lines is a no-op.
        let blanks = vec!["".to_string(), "   ".to_string()];
        assert_eq!(toggle_comment(&blanks), blanks);

        // Commenting a selection with interspersed blanks leaves the blanks be.
        let with_blank = vec!["a = 1".to_string(), "".to_string(), "b = 2".to_string()];
        let commented = toggle_comment(&with_blank);
        assert_eq!(commented, vec!["# a = 1", "", "# b = 2"]);

        // Since the only non-blank lines are now commented, toggling uncomments.
        assert_eq!(toggle_comment(&commented), with_blank);
    }
}
