//! Script editor **function/member outline** (pat-ybeo1).
//!
//! The outline lists a script's top-level symbols — functions, variables,
//! constants, and signals — in document order. Selecting an entry moves the
//! caret to the line where that symbol is defined, letting the user jump around
//! the current script.
//!
//! This is a lightweight line scanner (not a full GDScript parser): it inspects
//! the leading keyword of each line, skipping a single leading annotation such
//! as `@export`.

/// The kind of symbol an outline entry represents.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolKind {
    /// A `func` definition.
    Function,
    /// A `var` member.
    Variable,
    /// A `const` member.
    Constant,
    /// A `signal` declaration.
    Signal,
}

/// One entry in the script outline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutlineEntry {
    /// The symbol's name.
    pub name: String,
    /// What kind of symbol it is.
    pub kind: SymbolKind,
    /// The 1-based line of its definition.
    pub line: usize,
}

/// If `line` begins with `kw` followed by whitespace, returns the remainder
/// (trimmed of leading whitespace).
fn after_keyword<'a>(line: &'a str, kw: &str) -> Option<&'a str> {
    let rest = line.strip_prefix(kw)?;
    match rest.chars().next() {
        Some(c) if c.is_whitespace() => Some(rest.trim_start()),
        _ => None,
    }
}

/// The leading identifier of `s` (letters, digits, underscores), if any.
fn leading_ident(s: &str) -> Option<String> {
    let id: String = s
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect();
    (!id.is_empty()).then_some(id)
}

/// Classifies a single (already indent-trimmed) line into an outline symbol.
fn classify(code: &str) -> Option<(SymbolKind, String)> {
    // Skip a single leading annotation, e.g. `@export var hp`.
    let code = if let Some(rest) = code.strip_prefix('@') {
        match rest.split_once(char::is_whitespace) {
            Some((_, after)) => after.trim_start(),
            None => return None,
        }
    } else {
        code
    };

    for (kw, kind) in [
        ("func", SymbolKind::Function),
        ("var", SymbolKind::Variable),
        ("const", SymbolKind::Constant),
        ("signal", SymbolKind::Signal),
    ] {
        if let Some(rest) = after_keyword(code, kw) {
            if let Some(name) = leading_ident(rest) {
                return Some((kind, name));
            }
        }
    }
    None
}

/// Scans `source` and returns its outline entries in document order.
pub fn outline(source: &str) -> Vec<OutlineEntry> {
    source
        .lines()
        .enumerate()
        .filter_map(|(i, line)| {
            classify(line.trim_start()).map(|(kind, name)| OutlineEntry {
                name,
                kind,
                line: i + 1,
            })
        })
        .collect()
}

/// The outline of a script plus the caret position, supporting jump-to-symbol.
#[derive(Debug, Clone)]
pub struct ScriptOutline {
    entries: Vec<OutlineEntry>,
    caret_line: usize,
}

impl ScriptOutline {
    /// Builds the outline for `source`. The caret starts on line 1.
    pub fn new(source: &str) -> Self {
        Self {
            entries: outline(source),
            caret_line: 1,
        }
    }

    /// The outline entries, in document order.
    pub fn entries(&self) -> &[OutlineEntry] {
        &self.entries
    }

    /// The number of entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the outline is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The current caret line (1-based).
    pub fn caret_line(&self) -> usize {
        self.caret_line
    }

    /// Selects the entry at `index`, moving the caret to its definition line.
    /// Returns the line jumped to, or `None` if the index is out of range.
    pub fn select(&mut self, index: usize) -> Option<usize> {
        let line = self.entries.get(index)?.line;
        self.caret_line = line;
        Some(line)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCRIPT: &str = "extends Node\n\nsignal hit(damage)\n\nconst MAX = 100\nvar hp = 10\n@export var speed = 5\n\nfunc _ready():\n    hp = MAX\n\nfunc take_damage(amount):\n    hp -= amount\n";

    /// Acceptance (pat-ybeo1): the outline lists the script's functions/members
    /// in document order and selecting an entry moves the caret to that symbol's
    /// definition.
    #[test]
    fn script_nav_function_list() {
        let mut o = ScriptOutline::new(SCRIPT);

        // Symbols are listed in document order with their kinds and lines.
        let got: Vec<(&str, SymbolKind, usize)> = o
            .entries()
            .iter()
            .map(|e| (e.name.as_str(), e.kind, e.line))
            .collect();
        assert_eq!(
            got,
            vec![
                ("hit", SymbolKind::Signal, 3),
                ("MAX", SymbolKind::Constant, 5),
                ("hp", SymbolKind::Variable, 6),
                ("speed", SymbolKind::Variable, 7), // annotation skipped
                ("_ready", SymbolKind::Function, 9),
                ("take_damage", SymbolKind::Function, 12),
            ]
        );

        // Selecting the `func _ready` entry jumps the caret to its line.
        assert_eq!(o.select(4), Some(9));
        assert_eq!(o.caret_line(), 9);

        // Selecting the first signal jumps to its line.
        assert_eq!(o.select(0), Some(3));
        assert_eq!(o.caret_line(), 3);

        // The last function.
        assert_eq!(o.select(5), Some(12));
        assert_eq!(o.caret_line(), 12);

        // Out-of-range selection leaves the caret unchanged.
        assert_eq!(o.select(99), None);
        assert_eq!(o.caret_line(), 12);
    }

    /// Lines that merely use a symbol (not define it) aren't outline entries.
    #[test]
    fn only_definitions_are_listed() {
        let src = "var total = 0\ntotal = total + 1\nfunc add(n):\n    total += n\n";
        let entries = outline(src);
        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, vec!["total", "add"]);
        // The `total = total + 1` and `total += n` lines are excluded.
        assert_eq!(entries.len(), 2);
    }
}
