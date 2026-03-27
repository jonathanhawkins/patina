//! Script editor with GDScript syntax highlighting.
//!
//! Provides a [`ScriptEditor`] that manages open script tabs and a
//! [`SyntaxHighlighter`] that tokenizes GDScript source and assigns
//! [`HighlightKind`] categories to each token span for rendering.

use std::collections::BTreeSet;
pub use crate::script_gutter::GutterMarker;
use crate::settings::ExternalEditorConfig;
use gdscript_interop::tokenizer::{tokenize, LexError, Token, TokenSpan};

// ── Syntax highlighting ─────────────────────────────────────────────────

/// The category of highlighting to apply to a source span.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HighlightKind {
    /// A language keyword (`func`, `var`, `if`, `return`, etc.).
    Keyword,
    /// A built-in type or annotation (`@onready`, `@export`).
    Annotation,
    /// A string literal.
    StringLiteral,
    /// A numeric literal (int or float).
    NumberLiteral,
    /// A boolean literal (`true`, `false`) or `null`.
    ConstantLiteral,
    /// A comment.
    Comment,
    /// An identifier (variable, function name, etc.).
    Identifier,
    /// An operator (`+`, `-`, `==`, etc.).
    Operator,
    /// Punctuation (parens, brackets, colons, etc.).
    Punctuation,
    /// Indentation tokens (not rendered, but tracked).
    Whitespace,
    /// Plain text / unknown.
    Plain,
}

/// A highlighted span of source code.
#[derive(Debug, Clone, PartialEq)]
pub struct HighlightSpan {
    /// The highlight category.
    pub kind: HighlightKind,
    /// 1-based line number.
    pub line: usize,
    /// 1-based column number.
    pub col: usize,
    /// The token text (reconstructed from the token for display).
    pub text: String,
}

/// Classifies a [`Token`] into a [`HighlightKind`].
fn classify_token(token: &Token) -> HighlightKind {
    match token {
        // Keywords
        Token::Var
        | Token::Func
        | Token::If
        | Token::Else
        | Token::Elif
        | Token::While
        | Token::For
        | Token::In
        | Token::Return
        | Token::Class
        | Token::Extends
        | Token::Signal
        | Token::Enum
        | Token::Match
        | Token::Pass
        | Token::Break
        | Token::Continue
        | Token::Const
        | Token::Static
        | Token::Self_
        | Token::Super
        | Token::ClassName
        | Token::Await => HighlightKind::Keyword,

        // Annotations
        Token::Onready | Token::Export => HighlightKind::Annotation,

        // Literals
        Token::StringLit(_) => HighlightKind::StringLiteral,
        Token::IntLit(_) | Token::FloatLit(_) => HighlightKind::NumberLiteral,
        Token::BoolLit(_) | Token::Null => HighlightKind::ConstantLiteral,

        // Identifiers
        Token::Ident(_) => HighlightKind::Identifier,

        // Operators
        Token::Plus
        | Token::Minus
        | Token::Star
        | Token::Slash
        | Token::Percent
        | Token::EqEq
        | Token::BangEq
        | Token::Lt
        | Token::Gt
        | Token::LtEq
        | Token::GtEq
        | Token::And
        | Token::Or
        | Token::Not
        | Token::Assign
        | Token::Arrow
        | Token::PlusAssign
        | Token::MinusAssign
        | Token::ColonAssign => HighlightKind::Operator,

        // Punctuation
        Token::LParen
        | Token::RParen
        | Token::LBracket
        | Token::RBracket
        | Token::LBrace
        | Token::RBrace
        | Token::Colon
        | Token::Comma
        | Token::Dot
        | Token::Semicolon
        | Token::AtSign
        | Token::Dollar => HighlightKind::Punctuation,

        // Indentation / whitespace
        Token::Indent | Token::Dedent | Token::Newline => HighlightKind::Whitespace,

        // EOF
        Token::Eof => HighlightKind::Plain,
    }
}

/// Returns a display string for a token.
fn token_text(token: &Token) -> String {
    match token {
        Token::Var => "var".into(),
        Token::Func => "func".into(),
        Token::If => "if".into(),
        Token::Else => "else".into(),
        Token::Elif => "elif".into(),
        Token::While => "while".into(),
        Token::For => "for".into(),
        Token::In => "in".into(),
        Token::Return => "return".into(),
        Token::Class => "class".into(),
        Token::Extends => "extends".into(),
        Token::Signal => "signal".into(),
        Token::Enum => "enum".into(),
        Token::Match => "match".into(),
        Token::Pass => "pass".into(),
        Token::Break => "break".into(),
        Token::Continue => "continue".into(),
        Token::Const => "const".into(),
        Token::Static => "static".into(),
        Token::Self_ => "self".into(),
        Token::Super => "super".into(),
        Token::ClassName => "class_name".into(),
        Token::Onready => "@onready".into(),
        Token::Export => "@export".into(),
        Token::Await => "await".into(),
        Token::IntLit(n) => n.to_string(),
        Token::FloatLit(f) => format!("{f}"),
        Token::StringLit(s) => format!("\"{s}\""),
        Token::BoolLit(b) => b.to_string(),
        Token::Null => "null".into(),
        Token::Ident(name) => name.clone(),
        Token::Plus => "+".into(),
        Token::Minus => "-".into(),
        Token::Star => "*".into(),
        Token::Slash => "/".into(),
        Token::Percent => "%".into(),
        Token::EqEq => "==".into(),
        Token::BangEq => "!=".into(),
        Token::Lt => "<".into(),
        Token::Gt => ">".into(),
        Token::LtEq => "<=".into(),
        Token::GtEq => ">=".into(),
        Token::And => "and".into(),
        Token::Or => "or".into(),
        Token::Not => "not".into(),
        Token::Assign => "=".into(),
        Token::Arrow => "->".into(),
        Token::PlusAssign => "+=".into(),
        Token::MinusAssign => "-=".into(),
        Token::ColonAssign => ":=".into(),
        Token::LParen => "(".into(),
        Token::RParen => ")".into(),
        Token::LBracket => "[".into(),
        Token::RBracket => "]".into(),
        Token::LBrace => "{".into(),
        Token::RBrace => "}".into(),
        Token::Colon => ":".into(),
        Token::Comma => ",".into(),
        Token::Dot => ".".into(),
        Token::Semicolon => ";".into(),
        Token::AtSign => "@".into(),
        Token::Dollar => "$".into(),
        Token::Indent => "".into(),
        Token::Dedent => "".into(),
        Token::Newline => "\n".into(),
        Token::Eof => "".into(),
    }
}

/// GDScript syntax highlighter.
///
/// Tokenizes GDScript source and produces [`HighlightSpan`]s for rendering.
#[derive(Debug, Default)]
pub struct SyntaxHighlighter;

impl SyntaxHighlighter {
    /// Creates a new highlighter.
    pub fn new() -> Self {
        Self
    }

    /// Highlights GDScript source code, returning a list of spans.
    ///
    /// Whitespace tokens (Indent, Dedent, Newline) are excluded from the output.
    /// Returns an error if tokenization fails.
    pub fn highlight(&self, source: &str) -> Result<Vec<HighlightSpan>, LexError> {
        let tokens = tokenize(source)?;
        Ok(tokens
            .into_iter()
            .filter(|ts| {
                !matches!(
                    ts.token,
                    Token::Indent | Token::Dedent | Token::Newline | Token::Eof
                )
            })
            .map(|ts| {
                let kind = classify_token(&ts.token);
                let text = token_text(&ts.token);
                HighlightSpan {
                    kind,
                    line: ts.line,
                    col: ts.col,
                    text,
                }
            })
            .collect())
    }

    /// Highlights and returns only spans on the given line (1-based).
    pub fn highlight_line(
        &self,
        source: &str,
        line: usize,
    ) -> Result<Vec<HighlightSpan>, LexError> {
        Ok(self
            .highlight(source)?
            .into_iter()
            .filter(|s| s.line == line)
            .collect())
    }

    /// Returns the set of unique highlight kinds used in the source.
    pub fn used_kinds(&self, source: &str) -> Result<Vec<HighlightKind>, LexError> {
        let spans = self.highlight(source)?;
        let mut kinds: Vec<HighlightKind> = spans.iter().map(|s| s.kind).collect();
        kinds.sort_by_key(|k| *k as u8);
        kinds.dedup();
        Ok(kinds)
    }
}

// ── Script editor ───────────────────────────────────────────────────────
// Note: Gutter (breakpoints & bookmarks) is in script_gutter.rs

/// Per-script gutter state tracking breakpoints and bookmarks by line number.
///
/// Line numbers are 1-based, matching the editor display.
#[derive(Debug, Clone, Default)]
pub struct Gutter {
    /// Lines with breakpoints (1-based).
    breakpoints: BTreeSet<usize>,
    /// Lines with bookmarks (1-based).
    bookmarks: BTreeSet<usize>,
}

impl Gutter {
    /// Creates an empty gutter.
    pub fn new() -> Self {
        Self::default()
    }

    /// Toggles a breakpoint on the given line. Returns `true` if added, `false` if removed.
    pub fn toggle_breakpoint(&mut self, line: usize) -> bool {
        if !self.breakpoints.remove(&line) {
            self.breakpoints.insert(line);
            true
        } else {
            false
        }
    }

    /// Sets a breakpoint on the given line. Returns `false` if already set.
    pub fn set_breakpoint(&mut self, line: usize) -> bool {
        self.breakpoints.insert(line)
    }

    /// Clears a breakpoint on the given line. Returns `true` if it was set.
    pub fn clear_breakpoint(&mut self, line: usize) -> bool {
        self.breakpoints.remove(&line)
    }

    /// Returns whether the given line has a breakpoint.
    pub fn has_breakpoint(&self, line: usize) -> bool {
        self.breakpoints.contains(&line)
    }

    /// Returns all breakpoint lines in sorted order.
    pub fn breakpoints(&self) -> Vec<usize> {
        self.breakpoints.iter().copied().collect()
    }

    /// Returns the number of breakpoints.
    pub fn breakpoint_count(&self) -> usize {
        self.breakpoints.len()
    }

    /// Clears all breakpoints.
    pub fn clear_all_breakpoints(&mut self) {
        self.breakpoints.clear();
    }

    /// Toggles a bookmark on the given line. Returns `true` if added, `false` if removed.
    pub fn toggle_bookmark(&mut self, line: usize) -> bool {
        if !self.bookmarks.remove(&line) {
            self.bookmarks.insert(line);
            true
        } else {
            false
        }
    }

    /// Sets a bookmark on the given line. Returns `false` if already set.
    pub fn set_bookmark(&mut self, line: usize) -> bool {
        self.bookmarks.insert(line)
    }

    /// Clears a bookmark on the given line. Returns `true` if it was set.
    pub fn clear_bookmark(&mut self, line: usize) -> bool {
        self.bookmarks.remove(&line)
    }

    /// Returns whether the given line has a bookmark.
    pub fn has_bookmark(&self, line: usize) -> bool {
        self.bookmarks.contains(&line)
    }

    /// Returns all bookmark lines in sorted order.
    pub fn bookmarks(&self) -> Vec<usize> {
        self.bookmarks.iter().copied().collect()
    }

    /// Returns the number of bookmarks.
    pub fn bookmark_count(&self) -> usize {
        self.bookmarks.len()
    }

    /// Clears all bookmarks.
    pub fn clear_all_bookmarks(&mut self) {
        self.bookmarks.clear();
    }

    /// Returns all markers on the given line.
    pub fn markers_at(&self, line: usize) -> Vec<GutterMarker> {
        let mut markers = Vec::new();
        if self.breakpoints.contains(&line) {
            markers.push(GutterMarker::Breakpoint);
        }
        if self.bookmarks.contains(&line) {
            markers.push(GutterMarker::Bookmark);
        }
        markers
    }

    /// Returns the next bookmark line after `line`, wrapping around.
    /// Returns `None` if there are no bookmarks.
    pub fn next_bookmark(&self, line: usize) -> Option<usize> {
        // Find first bookmark after current line.
        self.bookmarks
            .range((line + 1)..)
            .next()
            .or_else(|| self.bookmarks.iter().next())
            .copied()
    }

    /// Returns the previous bookmark line before `line`, wrapping around.
    /// Returns `None` if there are no bookmarks.
    pub fn prev_bookmark(&self, line: usize) -> Option<usize> {
        self.bookmarks
            .range(..line)
            .next_back()
            .or_else(|| self.bookmarks.iter().next_back())
            .copied()
    }

    /// Clears all markers (breakpoints and bookmarks).
    pub fn clear_all(&mut self) {
        self.breakpoints.clear();
        self.bookmarks.clear();
    }
}

// ── Script editor ───────────────────────────────────────────────────────

/// A single open script tab.
#[derive(Debug, Clone)]
pub struct ScriptTab {
    /// The file path (e.g., `res://player.gd`).
    pub path: String,
    /// The current source text.
    pub source: String,
    /// Whether the source has unsaved modifications.
    pub modified: bool,
    /// The cursor line (1-based).
    pub cursor_line: usize,
    /// The cursor column (1-based).
    pub cursor_col: usize,
    /// Undo history (previous source states).
    undo_stack: Vec<String>,
    /// Redo stack.
    redo_stack: Vec<String>,
    /// Gutter state (breakpoints and bookmarks).
    pub gutter: Gutter,
}

impl ScriptTab {
    /// Creates a new tab for the given path and source.
    pub fn new(path: impl Into<String>, source: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            source: source.into(),
            modified: false,
            cursor_line: 1,
            cursor_col: 1,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            gutter: Gutter::new(),
        }
    }

    /// Replaces the source text, pushing the old version onto the undo stack.
    pub fn set_source(&mut self, new_source: impl Into<String>) {
        let old = std::mem::replace(&mut self.source, new_source.into());
        self.undo_stack.push(old);
        self.redo_stack.clear();
        self.modified = true;
    }

    /// Undoes the last edit. Returns `true` if there was something to undo.
    pub fn undo(&mut self) -> bool {
        if let Some(prev) = self.undo_stack.pop() {
            let current = std::mem::replace(&mut self.source, prev);
            self.redo_stack.push(current);
            self.modified = true;
            true
        } else {
            false
        }
    }

    /// Redoes the last undone edit. Returns `true` if there was something to redo.
    pub fn redo(&mut self) -> bool {
        if let Some(next) = self.redo_stack.pop() {
            let current = std::mem::replace(&mut self.source, next);
            self.undo_stack.push(current);
            self.modified = true;
            true
        } else {
            false
        }
    }

    /// Marks the tab as saved (clears the modified flag).
    pub fn mark_saved(&mut self) {
        self.modified = false;
    }

    /// Sets the cursor position (1-based line and column).
    pub fn set_cursor(&mut self, line: usize, col: usize) {
        self.cursor_line = line;
        self.cursor_col = col;
    }

    /// Returns the number of lines in the source.
    pub fn line_count(&self) -> usize {
        self.source.lines().count().max(1)
    }

    /// Returns the text of a specific line (1-based). `None` if out of range.
    pub fn get_line(&self, line: usize) -> Option<&str> {
        self.source.lines().nth(line.saturating_sub(1))
    }
}

/// The script editor, managing multiple open script tabs.
#[derive(Debug)]
pub struct ScriptEditor {
    /// Open tabs.
    tabs: Vec<ScriptTab>,
    /// Index of the active tab (if any).
    active_tab: Option<usize>,
    /// The syntax highlighter.
    highlighter: SyntaxHighlighter,
}

impl ScriptEditor {
    /// Creates a new empty script editor.
    pub fn new() -> Self {
        Self {
            tabs: Vec::new(),
            active_tab: None,
            highlighter: SyntaxHighlighter::new(),
        }
    }

    /// Opens a script. If already open, switches to that tab.
    /// Returns the tab index.
    pub fn open(&mut self, path: impl Into<String>, source: impl Into<String>) -> usize {
        let path = path.into();
        if let Some(idx) = self.tabs.iter().position(|t| t.path == path) {
            self.active_tab = Some(idx);
            return idx;
        }
        let tab = ScriptTab::new(path, source);
        self.tabs.push(tab);
        let idx = self.tabs.len() - 1;
        self.active_tab = Some(idx);
        idx
    }

    /// Closes a tab by index. Returns `true` if it existed.
    pub fn close(&mut self, index: usize) -> bool {
        if index >= self.tabs.len() {
            return false;
        }
        self.tabs.remove(index);
        // Adjust active tab.
        if self.tabs.is_empty() {
            self.active_tab = None;
        } else if let Some(active) = self.active_tab {
            if active >= self.tabs.len() {
                self.active_tab = Some(self.tabs.len() - 1);
            } else if active > index {
                self.active_tab = Some(active - 1);
            }
        }
        true
    }

    /// Returns the number of open tabs.
    pub fn tab_count(&self) -> usize {
        self.tabs.len()
    }

    /// Returns the active tab index.
    pub fn active_tab_index(&self) -> Option<usize> {
        self.active_tab
    }

    /// Switches to a tab by index. Returns `false` if out of range.
    pub fn set_active_tab(&mut self, index: usize) -> bool {
        if index < self.tabs.len() {
            self.active_tab = Some(index);
            true
        } else {
            false
        }
    }

    /// Returns a reference to the active tab, if any.
    pub fn active(&self) -> Option<&ScriptTab> {
        self.active_tab.and_then(|i| self.tabs.get(i))
    }

    /// Returns a mutable reference to the active tab, if any.
    pub fn active_mut(&mut self) -> Option<&mut ScriptTab> {
        self.active_tab.and_then(|i| self.tabs.get_mut(i))
    }

    /// Returns a reference to a tab by index.
    pub fn tab(&self, index: usize) -> Option<&ScriptTab> {
        self.tabs.get(index)
    }

    /// Returns a mutable reference to a tab by index.
    pub fn tab_mut(&mut self, index: usize) -> Option<&mut ScriptTab> {
        self.tabs.get_mut(index)
    }

    /// Returns all open tab paths.
    pub fn open_paths(&self) -> Vec<&str> {
        self.tabs.iter().map(|t| t.path.as_str()).collect()
    }

    /// Returns whether any tab has unsaved modifications.
    pub fn has_unsaved(&self) -> bool {
        self.tabs.iter().any(|t| t.modified)
    }

    /// Highlights the active tab's source. Returns `None` if no active tab.
    pub fn highlight_active(&self) -> Option<Result<Vec<HighlightSpan>, LexError>> {
        let tab = self.active()?;
        Some(self.highlighter.highlight(&tab.source))
    }

    /// Highlights a specific line of the active tab.
    pub fn highlight_active_line(
        &self,
        line: usize,
    ) -> Option<Result<Vec<HighlightSpan>, LexError>> {
        let tab = self.active()?;
        Some(self.highlighter.highlight_line(&tab.source, line))
    }

    /// Returns the highlighter for direct use.
    pub fn highlighter(&self) -> &SyntaxHighlighter {
        &self.highlighter
    }
}

impl Default for ScriptEditor {
    fn default() -> Self {
        Self::new()
    }
}

// ── External editor launch support ──────────────────────────────────

/// The result of attempting to launch an external editor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExternalEditorResult {
    /// The editor process was spawned successfully.
    Launched {
        /// The command that was executed.
        command: String,
        /// The expanded arguments.
        args: Vec<String>,
    },
    /// No external editor is configured.
    NotConfigured,
    /// The external editor executable was not found.
    ExecNotFound(String),
    /// The launch failed with an OS error.
    LaunchError(String),
}

/// Launches an external editor for a given script file.
///
/// This is the core launch function — it builds the command from the
/// [`ExternalEditorConfig`], expands placeholders, and spawns the process.
/// The process is detached (not waited on) so it doesn't block the editor.
pub fn launch_external_editor(
    config: &ExternalEditorConfig,
    file: &str,
    line: usize,
    col: usize,
) -> ExternalEditorResult {
    if !config.is_configured() {
        return ExternalEditorResult::NotConfigured;
    }

    let args = config.build_args(file, line, col);

    match std::process::Command::new(&config.exec_path)
        .args(&args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
    {
        Ok(_child) => {
            tracing::info!(
                "Launched external editor: {} {}",
                config.exec_path,
                args.join(" ")
            );
            ExternalEditorResult::Launched {
                command: config.exec_path.clone(),
                args,
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            ExternalEditorResult::ExecNotFound(config.exec_path.clone())
        }
        Err(e) => ExternalEditorResult::LaunchError(format!("{e}")),
    }
}

impl ScriptEditor {
    /// Opens the active tab's script in the configured external editor.
    ///
    /// Uses the tab's current cursor position for `{line}` and `{col}`.
    /// Returns [`ExternalEditorResult::NotConfigured`] if no external editor is set.
    pub fn open_in_external_editor(
        &self,
        config: &ExternalEditorConfig,
    ) -> ExternalEditorResult {
        let tab = match self.active() {
            Some(t) => t,
            None => return ExternalEditorResult::LaunchError("no active tab".into()),
        };
        launch_external_editor(config, &tab.path, tab.cursor_line, tab.cursor_col)
    }

    /// Opens a specific file path in the configured external editor at line 1, col 1.
    pub fn open_path_in_external_editor(
        &self,
        config: &ExternalEditorConfig,
        path: &str,
        line: usize,
        col: usize,
    ) -> ExternalEditorResult {
        launch_external_editor(config, path, line, col)
    }
}

// ── Find and Replace ─────────────────────────────────────────────────

/// A match result from a find operation in the script editor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FindMatch {
    /// 0-based line number.
    pub line: usize,
    /// 0-based column (byte offset within the line).
    pub col: usize,
    /// Length of the match in bytes.
    pub length: usize,
    /// The matched text.
    pub text: String,
}

/// Options controlling find/replace behaviour.
#[derive(Debug, Clone)]
pub struct FindOptions {
    /// Whether the search is case-sensitive.
    pub case_sensitive: bool,
    /// Whether the query is a regular expression.
    pub regex: bool,
    /// Whether to match whole words only.
    pub whole_word: bool,
    /// Whether to wrap around to the beginning when reaching the end.
    pub wrap_around: bool,
}

impl Default for FindOptions {
    fn default() -> Self {
        Self {
            case_sensitive: true,
            regex: false,
            whole_word: false,
            wrap_around: true,
        }
    }
}

/// Find-and-replace engine for the script editor.
#[derive(Debug)]
pub struct FindReplace {
    query: String,
    replacement: String,
    options: FindOptions,
}

impl FindReplace {
    /// Creates a new find/replace instance with the given query.
    pub fn new(query: impl Into<String>) -> Self {
        Self {
            query: query.into(),
            replacement: String::new(),
            options: FindOptions::default(),
        }
    }

    /// Sets the replacement text.
    pub fn with_replacement(mut self, replacement: impl Into<String>) -> Self {
        self.replacement = replacement.into();
        self
    }

    /// Sets the search options.
    pub fn with_options(mut self, options: FindOptions) -> Self {
        self.options = options;
        self
    }

    /// Returns the current query.
    pub fn query(&self) -> &str { &self.query }

    /// Sets a new query.
    pub fn set_query(&mut self, query: impl Into<String>) { self.query = query.into(); }

    /// Returns the current replacement text.
    pub fn replacement(&self) -> &str { &self.replacement }

    /// Sets a new replacement text.
    pub fn set_replacement(&mut self, replacement: impl Into<String>) { self.replacement = replacement.into(); }

    /// Returns the current options.
    pub fn options(&self) -> &FindOptions { &self.options }

    /// Sets new options.
    pub fn set_options(&mut self, options: FindOptions) { self.options = options; }

    /// Finds all matches in the given source text.
    pub fn find_all(&self, source: &str) -> Vec<FindMatch> {
        if self.query.is_empty() { return Vec::new(); }
        if self.options.regex { self.find_all_regex(source) } else { self.find_all_plain(source) }
    }

    /// Counts the total number of matches.
    pub fn count(&self, source: &str) -> usize { self.find_all(source).len() }

    /// Finds the next match starting from the given line and column.
    pub fn find_next(&self, source: &str, from_line: usize, from_col: usize) -> Option<FindMatch> {
        let matches = self.find_all(source);
        let next = matches.iter().find(|m| m.line > from_line || (m.line == from_line && m.col >= from_col));
        if let Some(m) = next { return Some(m.clone()); }
        if self.options.wrap_around { return matches.into_iter().next(); }
        None
    }

    /// Finds the previous match before the given line and column.
    pub fn find_prev(&self, source: &str, from_line: usize, from_col: usize) -> Option<FindMatch> {
        let matches = self.find_all(source);
        let prev = matches.iter().rev().find(|m| m.line < from_line || (m.line == from_line && m.col < from_col));
        if let Some(m) = prev { return Some(m.clone()); }
        if self.options.wrap_around { return matches.into_iter().last(); }
        None
    }

    /// Replaces the first occurrence and returns the new source.
    pub fn replace_next(&self, source: &str) -> Option<String> {
        if self.options.regex { self.replace_regex(source, false) } else { self.replace_plain(source, false) }
    }

    /// Replaces all occurrences and returns the new source.
    pub fn replace_all(&self, source: &str) -> String {
        if self.options.regex {
            self.replace_regex(source, true).unwrap_or_else(|| source.to_string())
        } else {
            self.replace_plain(source, true).unwrap_or_else(|| source.to_string())
        }
    }

    fn find_all_plain(&self, source: &str) -> Vec<FindMatch> {
        let mut results = Vec::new();
        let query = if self.options.case_sensitive { self.query.clone() } else { self.query.to_lowercase() };
        for (line_idx, line) in source.lines().enumerate() {
            let search_line = if self.options.case_sensitive { line.to_string() } else { line.to_lowercase() };
            let mut start = 0;
            while let Some(pos) = search_line[start..].find(&query) {
                let col = start + pos;
                if self.options.whole_word && !is_whole_word(line, col, query.len()) {
                    start = col + 1;
                    continue;
                }
                results.push(FindMatch {
                    line: line_idx, col, length: self.query.len(),
                    text: line[col..col + self.query.len()].to_string(),
                });
                start = col + self.query.len();
            }
        }
        results
    }

    fn find_all_regex(&self, source: &str) -> Vec<FindMatch> {
        let pattern = if self.options.case_sensitive { self.query.clone() } else { format!("(?i){}", self.query) };
        let re = match gdcore::regex::RegEx::create_from_string(&pattern) {
            Some(re) => re,
            None => return Vec::new(),
        };
        let mut results = Vec::new();
        for (line_idx, line) in source.lines().enumerate() {
            for m in re.search_all(line, 0, 0) {
                let col = m.start;
                let length = m.end - m.start;
                if self.options.whole_word && !is_whole_word(line, col, length) { continue; }
                results.push(FindMatch {
                    line: line_idx, col, length,
                    text: m.strings.first().cloned().unwrap_or_default(),
                });
            }
        }
        results
    }

    fn replace_plain(&self, source: &str, all: bool) -> Option<String> {
        if self.query.is_empty() { return None; }
        if self.options.case_sensitive {
            if source.contains(&self.query) {
                Some(if all { source.replace(&self.query, &self.replacement) } else { source.replacen(&self.query, &self.replacement, 1) })
            } else { None }
        } else {
            let pattern = format!("(?i){}", regex::escape(&self.query));
            let re = regex::Regex::new(&pattern).ok()?;
            if re.is_match(source) {
                Some(if all { re.replace_all(source, self.replacement.as_str()).to_string() } else { re.replace(source, self.replacement.as_str()).to_string() })
            } else { None }
        }
    }

    fn replace_regex(&self, source: &str, all: bool) -> Option<String> {
        let pattern = if self.options.case_sensitive { self.query.clone() } else { format!("(?i){}", self.query) };
        let re = regex::Regex::new(&pattern).ok()?;
        if re.is_match(source) {
            Some(if all { re.replace_all(source, self.replacement.as_str()).to_string() } else { re.replace(source, self.replacement.as_str()).to_string() })
        } else { None }
    }
}

impl Default for FindReplace {
    fn default() -> Self { Self::new("") }
}

fn is_whole_word(line: &str, col: usize, length: usize) -> bool {
    let before = if col == 0 { true } else { line.as_bytes().get(col - 1).map_or(true, |&b| !is_word_char(b)) };
    let after = line.as_bytes().get(col + length).map_or(true, |&b| !is_word_char(b));
    before && after
}

fn is_word_char(b: u8) -> bool { b.is_ascii_alphanumeric() || b == b'_' }

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_GD: &str = r#"extends Node2D

var speed: float = 100.0
@export var health: int = 10

func _ready():
    print("Hello")

func _process(delta):
    position.x += speed * delta
"#;

    // ── SyntaxHighlighter tests ─────────────────────────────────────

    #[test]
    fn highlight_empty_source() {
        let h = SyntaxHighlighter::new();
        let spans = h.highlight("").unwrap();
        assert!(spans.is_empty());
    }

    #[test]
    fn highlight_keywords() {
        let h = SyntaxHighlighter::new();
        let spans = h.highlight("var x = 10").unwrap();
        assert_eq!(spans[0].kind, HighlightKind::Keyword);
        assert_eq!(spans[0].text, "var");
    }

    #[test]
    fn highlight_string_literal() {
        let h = SyntaxHighlighter::new();
        let spans = h.highlight("var s = \"hello\"").unwrap();
        let string_span = spans.iter().find(|s| s.kind == HighlightKind::StringLiteral);
        assert!(string_span.is_some());
        assert!(string_span.unwrap().text.contains("hello"));
    }

    #[test]
    fn highlight_number_literal() {
        let h = SyntaxHighlighter::new();
        let spans = h.highlight("var x = 42").unwrap();
        let num_span = spans.iter().find(|s| s.kind == HighlightKind::NumberLiteral);
        assert!(num_span.is_some());
        assert_eq!(num_span.unwrap().text, "42");
    }

    #[test]
    fn highlight_float_literal() {
        let h = SyntaxHighlighter::new();
        let spans = h.highlight("var x = 3.14").unwrap();
        let num_span = spans.iter().find(|s| s.kind == HighlightKind::NumberLiteral);
        assert!(num_span.is_some());
    }

    #[test]
    fn highlight_bool_and_null() {
        let h = SyntaxHighlighter::new();
        let spans = h.highlight("var a = true\nvar b = null").unwrap();
        let constants: Vec<_> = spans
            .iter()
            .filter(|s| s.kind == HighlightKind::ConstantLiteral)
            .collect();
        assert_eq!(constants.len(), 2);
    }

    #[test]
    fn highlight_annotation() {
        let h = SyntaxHighlighter::new();
        let spans = h.highlight("@export var x = 1").unwrap();
        let anno = spans.iter().find(|s| s.kind == HighlightKind::Annotation);
        assert!(anno.is_some());
        assert_eq!(anno.unwrap().text, "@export");
    }

    #[test]
    fn highlight_comment_stripped() {
        // The GDScript tokenizer strips comments, so a comment-only source
        // produces no tokens.
        let h = SyntaxHighlighter::new();
        let spans = h.highlight("# this is a comment").unwrap();
        assert!(spans.is_empty());
    }

    #[test]
    fn highlight_operators() {
        let h = SyntaxHighlighter::new();
        let spans = h.highlight("x + y == z").unwrap();
        let ops: Vec<_> = spans
            .iter()
            .filter(|s| s.kind == HighlightKind::Operator)
            .collect();
        assert!(ops.len() >= 2); // + and ==
    }

    #[test]
    fn highlight_punctuation() {
        let h = SyntaxHighlighter::new();
        let spans = h.highlight("func f(a, b):").unwrap();
        let puncts: Vec<_> = spans
            .iter()
            .filter(|s| s.kind == HighlightKind::Punctuation)
            .collect();
        assert!(puncts.len() >= 3); // (, , , ), :
    }

    #[test]
    fn highlight_identifiers() {
        let h = SyntaxHighlighter::new();
        let spans = h.highlight("var my_var = other_var").unwrap();
        let idents: Vec<_> = spans
            .iter()
            .filter(|s| s.kind == HighlightKind::Identifier)
            .collect();
        assert_eq!(idents.len(), 2);
    }

    #[test]
    fn highlight_excludes_whitespace_tokens() {
        let h = SyntaxHighlighter::new();
        let spans = h.highlight(SAMPLE_GD).unwrap();
        assert!(spans.iter().all(|s| s.kind != HighlightKind::Whitespace));
    }

    #[test]
    fn highlight_line_filters_correctly() {
        let h = SyntaxHighlighter::new();
        let line1 = h.highlight_line(SAMPLE_GD, 1).unwrap();
        // Line 1: "extends Node2D"
        assert!(line1.iter().any(|s| s.text == "extends"));
        assert!(line1.iter().all(|s| s.line == 1));
    }

    #[test]
    fn highlight_sample_script() {
        let h = SyntaxHighlighter::new();
        let spans = h.highlight(SAMPLE_GD).unwrap();
        assert!(!spans.is_empty());

        // Should have keywords, identifiers, literals
        let kinds: Vec<_> = spans.iter().map(|s| s.kind).collect();
        assert!(kinds.contains(&HighlightKind::Keyword));
        assert!(kinds.contains(&HighlightKind::Identifier));
        assert!(kinds.contains(&HighlightKind::NumberLiteral));
        assert!(kinds.contains(&HighlightKind::StringLiteral));
    }

    #[test]
    fn used_kinds_returns_unique_set() {
        let h = SyntaxHighlighter::new();
        let kinds = h.used_kinds(SAMPLE_GD).unwrap();
        // No duplicates.
        let mut deduped = kinds.clone();
        deduped.dedup();
        assert_eq!(kinds, deduped);
    }

    // ── ScriptTab tests ─────────────────────────────────────────────

    #[test]
    fn tab_new_defaults() {
        let tab = ScriptTab::new("res://test.gd", "var x = 1");
        assert_eq!(tab.path, "res://test.gd");
        assert!(!tab.modified);
        assert_eq!(tab.cursor_line, 1);
        assert_eq!(tab.cursor_col, 1);
    }

    #[test]
    fn tab_set_source_marks_modified() {
        let mut tab = ScriptTab::new("test.gd", "old");
        tab.set_source("new");
        assert!(tab.modified);
        assert_eq!(tab.source, "new");
    }

    #[test]
    fn tab_undo_redo() {
        let mut tab = ScriptTab::new("test.gd", "v1");
        tab.set_source("v2");
        tab.set_source("v3");
        assert_eq!(tab.source, "v3");

        assert!(tab.undo());
        assert_eq!(tab.source, "v2");
        assert!(tab.undo());
        assert_eq!(tab.source, "v1");
        assert!(!tab.undo()); // nothing more

        assert!(tab.redo());
        assert_eq!(tab.source, "v2");
        assert!(tab.redo());
        assert_eq!(tab.source, "v3");
        assert!(!tab.redo()); // nothing more
    }

    #[test]
    fn tab_set_source_clears_redo() {
        let mut tab = ScriptTab::new("test.gd", "v1");
        tab.set_source("v2");
        tab.undo();
        tab.set_source("v3"); // should clear redo
        assert!(!tab.redo());
    }

    #[test]
    fn tab_mark_saved() {
        let mut tab = ScriptTab::new("test.gd", "src");
        tab.set_source("new src");
        assert!(tab.modified);
        tab.mark_saved();
        assert!(!tab.modified);
    }

    #[test]
    fn tab_cursor() {
        let mut tab = ScriptTab::new("test.gd", "");
        tab.set_cursor(5, 10);
        assert_eq!(tab.cursor_line, 5);
        assert_eq!(tab.cursor_col, 10);
    }

    #[test]
    fn tab_line_count() {
        let tab = ScriptTab::new("test.gd", "line1\nline2\nline3");
        assert_eq!(tab.line_count(), 3);
    }

    #[test]
    fn tab_get_line() {
        let tab = ScriptTab::new("test.gd", "aaa\nbbb\nccc");
        assert_eq!(tab.get_line(1), Some("aaa"));
        assert_eq!(tab.get_line(2), Some("bbb"));
        assert_eq!(tab.get_line(3), Some("ccc"));
        assert_eq!(tab.get_line(4), None);
    }

    // ── ScriptEditor tests ──────────────────────────────────────────

    #[test]
    fn editor_new_empty() {
        let e = ScriptEditor::new();
        assert_eq!(e.tab_count(), 0);
        assert!(e.active_tab_index().is_none());
        assert!(!e.has_unsaved());
    }

    #[test]
    fn editor_open_and_switch() {
        let mut e = ScriptEditor::new();
        let i0 = e.open("a.gd", "a");
        let i1 = e.open("b.gd", "b");
        assert_eq!(e.tab_count(), 2);
        assert_eq!(e.active_tab_index(), Some(i1));
        e.set_active_tab(i0);
        assert_eq!(e.active().unwrap().path, "a.gd");
    }

    #[test]
    fn editor_open_existing_switches() {
        let mut e = ScriptEditor::new();
        e.open("a.gd", "a");
        e.open("b.gd", "b");
        let idx = e.open("a.gd", "ignored");
        assert_eq!(idx, 0);
        assert_eq!(e.active_tab_index(), Some(0));
        assert_eq!(e.tab_count(), 2); // no duplicate tab
    }

    #[test]
    fn editor_close_tab() {
        let mut e = ScriptEditor::new();
        e.open("a.gd", "a");
        e.open("b.gd", "b");
        assert!(e.close(0));
        assert_eq!(e.tab_count(), 1);
        assert_eq!(e.active().unwrap().path, "b.gd");
    }

    #[test]
    fn editor_close_last_tab() {
        let mut e = ScriptEditor::new();
        e.open("a.gd", "a");
        e.close(0);
        assert_eq!(e.tab_count(), 0);
        assert!(e.active_tab_index().is_none());
    }

    #[test]
    fn editor_close_invalid_index() {
        let mut e = ScriptEditor::new();
        assert!(!e.close(0));
    }

    #[test]
    fn editor_open_paths() {
        let mut e = ScriptEditor::new();
        e.open("a.gd", "");
        e.open("b.gd", "");
        assert_eq!(e.open_paths(), vec!["a.gd", "b.gd"]);
    }

    #[test]
    fn editor_has_unsaved() {
        let mut e = ScriptEditor::new();
        e.open("a.gd", "src");
        assert!(!e.has_unsaved());
        e.active_mut().unwrap().set_source("new");
        assert!(e.has_unsaved());
    }

    #[test]
    fn editor_highlight_active() {
        let mut e = ScriptEditor::new();
        e.open("test.gd", "var x = 42");
        let spans = e.highlight_active().unwrap().unwrap();
        assert!(!spans.is_empty());
        assert!(spans.iter().any(|s| s.kind == HighlightKind::Keyword));
    }

    #[test]
    fn editor_highlight_active_line() {
        let mut e = ScriptEditor::new();
        e.open("test.gd", "var x = 1\nfunc f():\n    pass");
        let line2 = e.highlight_active_line(2).unwrap().unwrap();
        assert!(line2.iter().any(|s| s.text == "func"));
    }

    #[test]
    fn editor_highlight_no_active() {
        let e = ScriptEditor::new();
        assert!(e.highlight_active().is_none());
    }

    #[test]
    fn editor_set_active_invalid() {
        let mut e = ScriptEditor::new();
        assert!(!e.set_active_tab(0));
    }

    #[test]
    fn editor_default() {
        let e = ScriptEditor::default();
        assert_eq!(e.tab_count(), 0);
    }

    #[test]
    fn classify_all_keyword_tokens() {
        let keywords = vec![
            Token::Var,
            Token::Func,
            Token::If,
            Token::Else,
            Token::Elif,
            Token::While,
            Token::For,
            Token::In,
            Token::Return,
            Token::Class,
            Token::Extends,
            Token::Signal,
            Token::Enum,
            Token::Match,
            Token::Pass,
            Token::Break,
            Token::Continue,
            Token::Const,
            Token::Static,
            Token::Self_,
            Token::Super,
            Token::ClassName,
            Token::Await,
        ];
        for kw in keywords {
            assert_eq!(classify_token(&kw), HighlightKind::Keyword);
        }
    }
}
