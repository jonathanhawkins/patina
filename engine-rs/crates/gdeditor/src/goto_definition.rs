//! Goto-definition for the script editor.
//!
//! Resolves the identifier under the caret to the line where it is defined in
//! the current script. A *definition* is a top-level declaration keyword
//! (`func`, `static func`, `var`, `const`, `signal`, `class`, `enum`) followed by
//! the symbol name. The lookup is a no-op (`None`) when the caret is not on an
//! identifier or the symbol has no definition in the script.
//!
//! This operates on source text so it stays deterministic and testable; the
//! editor supplies the caret's line text and column.

/// Whether `c` can appear in an identifier.
fn is_ident_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// The declaration keywords that introduce a definition, longest first so that
/// `static func` is matched before `func`.
const DEF_KEYWORDS: &[&str] = &[
    "static func ",
    "func ",
    "var ",
    "const ",
    "signal ",
    "class ",
    "enum ",
];

/// If `line` is a definition, returns the defined symbol name.
fn def_name(line: &str) -> Option<String> {
    let trimmed = line.trim_start();
    for kw in DEF_KEYWORDS {
        if let Some(rest) = trimmed.strip_prefix(kw) {
            let name: String = rest.chars().take_while(|c| is_ident_char(*c)).collect();
            if !name.is_empty() {
                return Some(name);
            }
        }
    }
    None
}

/// The identifier covering character column `col` (0-based) in `line`, or `None`
/// if that position is not on an identifier character.
pub fn symbol_at(line: &str, col: usize) -> Option<String> {
    let chars: Vec<char> = line.chars().collect();
    if col >= chars.len() || !is_ident_char(chars[col]) {
        return None;
    }
    let mut start = col;
    while start > 0 && is_ident_char(chars[start - 1]) {
        start -= 1;
    }
    let mut end = col;
    while end < chars.len() && is_ident_char(chars[end]) {
        end += 1;
    }
    Some(chars[start..end].iter().collect())
}

/// The 1-based line where `symbol` is defined in `source`, or `None` if it has no
/// definition. Returns the first matching definition in document order.
pub fn find_definition(source: &str, symbol: &str) -> Option<usize> {
    for (i, line) in source.lines().enumerate() {
        if let Some(name) = def_name(line) {
            if name == symbol {
                return Some(i + 1);
            }
        }
    }
    None
}

/// Resolves the identifier at column `col` of `caret_line` to its definition line
/// (1-based) in `source`. Returns `None` when the caret is not on an identifier
/// or the symbol is unresolved (a no-op for the editor).
pub fn goto_definition(source: &str, caret_line: &str, col: usize) -> Option<usize> {
    let symbol = symbol_at(caret_line, col)?;
    find_definition(source, &symbol)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Acceptance (pat-yzqv7): invoking goto-definition on the symbol under the
    /// caret navigates to the definition line, and is a no-op for unresolved
    /// symbols (or when the caret is not on an identifier).
    #[test]
    fn script_nav_goto_definition() {
        let src = "\
var health = 100
func take_damage(amount):
    health -= amount
    die()
func die():
    queue_free()
";

        // Caret on the `die()` call (line 4) jumps to `func die` (line 5).
        assert_eq!(goto_definition(src, "    die()", 5), Some(5));
        // Caret on `health` usage jumps to its `var` definition (line 1).
        assert_eq!(goto_definition(src, "    health -= amount", 5), Some(1));

        // Unresolved symbol under the caret is a no-op.
        assert_eq!(goto_definition(src, "    amount += 1", 5), None);
        // Caret not on an identifier is a no-op.
        assert_eq!(goto_definition(src, "    die()", 0), None);

        // Direct symbol lookups.
        assert_eq!(find_definition(src, "take_damage"), Some(2));
        assert_eq!(find_definition(src, "health"), Some(1));
        assert_eq!(find_definition(src, "die"), Some(5));
        assert_eq!(find_definition(src, "nope"), None);

        // symbol_at extracts the identifier under the caret.
        assert_eq!(symbol_at("    die()", 4).as_deref(), Some("die")); // 'd'
        assert_eq!(symbol_at("    die()", 6).as_deref(), Some("die")); // 'e'
        assert_eq!(symbol_at("    die()", 7), None); // '('
        assert_eq!(symbol_at("    die()", 0), None); // ' '
    }

    /// `static func`, `const`, `signal`, `class`, and `enum` definitions resolve.
    #[test]
    fn goto_definition_keyword_kinds() {
        let src = "\
const MAX = 10
signal died
class Inner:
    pass
enum Mode { A, B }
static func helper():
    pass
";
        assert_eq!(find_definition(src, "MAX"), Some(1));
        assert_eq!(find_definition(src, "died"), Some(2));
        assert_eq!(find_definition(src, "Inner"), Some(3));
        assert_eq!(find_definition(src, "Mode"), Some(5));
        assert_eq!(find_definition(src, "helper"), Some(6));
    }
}
