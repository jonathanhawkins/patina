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
        | Token::MinusAssign => HighlightKind::Operator,

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
    /// Returns a slice of all open tabs.
    pub fn tabs(&self) -> &[ScriptTab] {
        &self.tabs
    }

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

// ── Code Completion ─────────────────────────────────────────────────

/// The kind of a completion suggestion.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CompletionKind {
    /// A function/method.
    Function,
    /// A variable.
    Variable,
    /// A constant.
    Constant,
    /// A signal.
    Signal,
    /// A class name.
    Class,
    /// A property.
    Property,
    /// A keyword.
    Keyword,
    /// An enum value.
    EnumValue,
}

/// A single completion suggestion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletionItem {
    /// The label shown in the completion popup.
    pub label: String,
    /// The text to insert when accepted.
    pub insert_text: String,
    /// The kind of completion.
    pub kind: CompletionKind,
    /// Optional detail shown alongside the label (e.g., return type).
    pub detail: Option<String>,
}

/// Provides GDScript code completion.
#[derive(Debug)]
pub struct CompletionProvider {
    /// Built-in keywords for GDScript.
    keywords: Vec<String>,
    /// Built-in class names.
    builtin_classes: Vec<String>,
    /// Built-in functions.
    builtin_functions: Vec<String>,
}

impl CompletionProvider {
    pub fn new() -> Self {
        Self {
            keywords: vec![
                "var", "func", "class", "extends", "signal", "enum", "const",
                "static", "if", "elif", "else", "for", "while", "match",
                "return", "pass", "break", "continue", "in", "is", "as",
                "self", "super", "await", "yield", "true", "false", "null",
                "export", "onready", "tool", "preload", "load",
            ]
            .into_iter()
            .map(String::from)
            .collect(),
            builtin_classes: vec![
                "Node", "Node2D", "Node3D", "Sprite2D", "Sprite3D",
                "Camera2D", "Camera3D", "Control", "Label", "Button",
                "TextureRect", "Area2D", "Area3D", "CollisionShape2D",
                "CollisionShape3D", "CharacterBody2D", "CharacterBody3D",
                "RigidBody2D", "RigidBody3D", "StaticBody2D", "StaticBody3D",
                "Timer", "AnimationPlayer", "AudioStreamPlayer",
                "RayCast2D", "RayCast3D", "TileMap", "CanvasLayer",
                "Resource", "PackedScene", "Vector2", "Vector3",
                "Color", "Rect2", "Transform2D", "Transform3D",
            ]
            .into_iter()
            .map(String::from)
            .collect(),
            builtin_functions: vec![
                "print", "push_error", "push_warning", "str", "int", "float",
                "len", "abs", "min", "max", "clamp", "lerp", "range",
                "randi", "randf", "randomize", "sqrt", "pow", "sin", "cos",
                "deg_to_rad", "rad_to_deg", "is_instance_of",
                "get_node", "get_parent", "get_children", "add_child",
                "remove_child", "queue_free", "connect", "disconnect",
                "emit_signal", "has_signal",
            ]
            .into_iter()
            .map(String::from)
            .collect(),
        }
    }

    /// Returns completion suggestions for the given prefix.
    pub fn complete(&self, prefix: &str, source: &str) -> Vec<CompletionItem> {
        if prefix.is_empty() {
            return Vec::new();
        }
        let prefix_lower = prefix.to_lowercase();
        let mut items = Vec::new();

        // Keywords.
        for kw in &self.keywords {
            if kw.to_lowercase().starts_with(&prefix_lower) {
                items.push(CompletionItem {
                    label: kw.clone(),
                    insert_text: kw.clone(),
                    kind: CompletionKind::Keyword,
                    detail: None,
                });
            }
        }

        // Built-in classes.
        for cls in &self.builtin_classes {
            if cls.to_lowercase().starts_with(&prefix_lower) {
                items.push(CompletionItem {
                    label: cls.clone(),
                    insert_text: cls.clone(),
                    kind: CompletionKind::Class,
                    detail: Some("class".into()),
                });
            }
        }

        // Built-in functions.
        for func in &self.builtin_functions {
            if func.to_lowercase().starts_with(&prefix_lower) {
                items.push(CompletionItem {
                    label: format!("{func}()"),
                    insert_text: format!("{func}("),
                    kind: CompletionKind::Function,
                    detail: Some("function".into()),
                });
            }
        }

        // Identifiers from the current source.
        self.collect_source_identifiers(source, prefix, &mut items);

        // Deduplicate by label.
        items.sort_by(|a, b| a.label.cmp(&b.label));
        items.dedup_by(|a, b| a.label == b.label);
        items
    }

    fn collect_source_identifiers(
        &self,
        source: &str,
        prefix: &str,
        items: &mut Vec<CompletionItem>,
    ) {
        let prefix_lower = prefix.to_lowercase();
        let mut seen = std::collections::HashSet::new();
        for line in source.lines() {
            for word in line.split(|c: char| !c.is_alphanumeric() && c != '_') {
                if word.len() > 1
                    && word.to_lowercase().starts_with(&prefix_lower)
                    && word != prefix
                    && seen.insert(word.to_string())
                {
                    // Detect kind from context.
                    let kind = if line.trim_start().starts_with("func ") && line.contains(word) && line.contains('(') {
                        CompletionKind::Function
                    } else if line.trim_start().starts_with("var ") {
                        CompletionKind::Variable
                    } else if line.trim_start().starts_with("const ") {
                        CompletionKind::Constant
                    } else if line.trim_start().starts_with("signal ") {
                        CompletionKind::Signal
                    } else {
                        CompletionKind::Variable
                    };
                    items.push(CompletionItem {
                        label: word.to_string(),
                        insert_text: word.to_string(),
                        kind,
                        detail: None,
                    });
                }
            }
        }
    }
}

impl Default for CompletionProvider {
    fn default() -> Self {
        Self::new()
    }
}

// ── Code Folding ────────────────────────────────────────────────────

/// A foldable region in the source code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FoldRegion {
    /// 1-based start line (the line with the fold indicator).
    pub start_line: usize,
    /// 1-based end line (inclusive).
    pub end_line: usize,
    /// The kind of foldable region.
    pub kind: FoldKind,
}

/// The kind of foldable region.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FoldKind {
    /// A function body.
    Function,
    /// A class body.
    Class,
    /// An if/elif/else block.
    Conditional,
    /// A for/while loop.
    Loop,
    /// A match block.
    Match,
    /// A region comment (`#region` / `#endregion`).
    Region,
}

/// Manages code folding state.
#[derive(Debug, Clone)]
pub struct CodeFolding {
    /// Set of folded start lines (1-based).
    folded: BTreeSet<usize>,
}

impl CodeFolding {
    pub fn new() -> Self {
        Self {
            folded: BTreeSet::new(),
        }
    }

    /// Toggles the fold state of a region. Returns the new folded state.
    pub fn toggle(&mut self, start_line: usize) -> bool {
        if self.folded.contains(&start_line) {
            self.folded.remove(&start_line);
            false
        } else {
            self.folded.insert(start_line);
            true
        }
    }

    /// Folds a region.
    pub fn fold(&mut self, start_line: usize) {
        self.folded.insert(start_line);
    }

    /// Unfolds a region.
    pub fn unfold(&mut self, start_line: usize) {
        self.folded.remove(&start_line);
    }

    /// Returns whether a region is folded.
    pub fn is_folded(&self, start_line: usize) -> bool {
        self.folded.contains(&start_line)
    }

    /// Returns all folded line numbers.
    pub fn folded_lines(&self) -> Vec<usize> {
        self.folded.iter().copied().collect()
    }

    /// Folds all regions.
    pub fn fold_all(&mut self, regions: &[FoldRegion]) {
        for r in regions {
            self.folded.insert(r.start_line);
        }
    }

    /// Unfolds all regions.
    pub fn unfold_all(&mut self) {
        self.folded.clear();
    }

    /// Returns the number of currently folded regions.
    pub fn folded_count(&self) -> usize {
        self.folded.len()
    }
}

impl Default for CodeFolding {
    fn default() -> Self {
        Self::new()
    }
}

/// Detects foldable regions in GDScript source.
pub fn detect_fold_regions(source: &str) -> Vec<FoldRegion> {
    let lines: Vec<&str> = source.lines().collect();
    let mut regions = Vec::new();
    let mut region_starts: Vec<usize> = Vec::new(); // stack for #region

    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        let line_num = i + 1; // 1-based

        // #region / #endregion
        if trimmed.starts_with("#region") {
            region_starts.push(line_num);
            continue;
        }
        if trimmed.starts_with("#endregion") {
            if let Some(start) = region_starts.pop() {
                regions.push(FoldRegion {
                    start_line: start,
                    end_line: line_num,
                    kind: FoldKind::Region,
                });
            }
            continue;
        }

        // Indentation-based blocks: detect lines ending with ':'
        if trimmed.ends_with(':') && !trimmed.starts_with('#') {
            let kind = if trimmed.starts_with("func ") || trimmed.starts_with("func(") {
                FoldKind::Function
            } else if trimmed.starts_with("class ") {
                FoldKind::Class
            } else if trimmed.starts_with("if ")
                || trimmed.starts_with("elif ")
                || trimmed == "else:"
            {
                FoldKind::Conditional
            } else if trimmed.starts_with("for ") || trimmed.starts_with("while ") {
                FoldKind::Loop
            } else if trimmed.starts_with("match ") {
                FoldKind::Match
            } else {
                continue;
            };

            // Find the block end by looking for the next line with equal or less indentation.
            let base_indent = line.len() - line.trim_start().len();
            let mut end = line_num;
            for j in (i + 1)..lines.len() {
                let next = lines[j];
                if next.trim().is_empty() {
                    continue;
                }
                let next_indent = next.len() - next.trim_start().len();
                if next_indent <= base_indent {
                    break;
                }
                end = j + 1; // 1-based
            }

            if end > line_num {
                regions.push(FoldRegion {
                    start_line: line_num,
                    end_line: end,
                    kind,
                });
            }
        }
    }

    regions
}

// ── Caret Tools ─────────────────────────────────────────────────────

/// A caret (cursor) position.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Caret {
    /// 1-based line number.
    pub line: usize,
    /// 1-based column number.
    pub col: usize,
}

impl Caret {
    pub fn new(line: usize, col: usize) -> Self {
        Self { line, col }
    }
}

/// A text selection (from caret to anchor).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Selection {
    /// The caret (cursor) position.
    pub caret: Caret,
    /// The anchor (start of selection). If equal to caret, no selection.
    pub anchor: Caret,
}

impl Selection {
    pub fn cursor_only(line: usize, col: usize) -> Self {
        let c = Caret::new(line, col);
        Self {
            caret: c,
            anchor: c,
        }
    }

    pub fn range(anchor_line: usize, anchor_col: usize, caret_line: usize, caret_col: usize) -> Self {
        Self {
            caret: Caret::new(caret_line, caret_col),
            anchor: Caret::new(anchor_line, anchor_col),
        }
    }

    /// Returns true if this is just a cursor (no selection range).
    pub fn is_empty(&self) -> bool {
        self.caret == self.anchor
    }

    /// Returns the start and end positions in document order.
    pub fn ordered(&self) -> (Caret, Caret) {
        if self.anchor <= self.caret {
            (self.anchor, self.caret)
        } else {
            (self.caret, self.anchor)
        }
    }
}

/// Multi-caret editing state.
#[derive(Debug, Clone)]
pub struct MultiCaret {
    /// All active selections (each has a caret and optional anchor).
    selections: Vec<Selection>,
}

impl MultiCaret {
    pub fn new() -> Self {
        Self {
            selections: vec![Selection::cursor_only(1, 1)],
        }
    }

    /// Returns the primary (first) selection.
    pub fn primary(&self) -> &Selection {
        &self.selections[0]
    }

    /// Returns all selections.
    pub fn selections(&self) -> &[Selection] {
        &self.selections
    }

    /// Returns the number of carets.
    pub fn caret_count(&self) -> usize {
        self.selections.len()
    }

    /// Sets a single cursor, clearing any multi-cursor state.
    pub fn set_cursor(&mut self, line: usize, col: usize) {
        self.selections = vec![Selection::cursor_only(line, col)];
    }

    /// Adds an additional cursor at the given position.
    pub fn add_cursor(&mut self, line: usize, col: usize) {
        let sel = Selection::cursor_only(line, col);
        if !self.selections.contains(&sel) {
            self.selections.push(sel);
            self.selections.sort_by_key(|s| s.caret);
        }
    }

    /// Removes a cursor at the given position. If it's the last one, does nothing.
    pub fn remove_cursor(&mut self, line: usize, col: usize) -> bool {
        if self.selections.len() <= 1 {
            return false;
        }
        let target = Caret::new(line, col);
        if let Some(pos) = self.selections.iter().position(|s| s.caret == target) {
            self.selections.remove(pos);
            true
        } else {
            false
        }
    }

    /// Sets a selection range for the primary cursor.
    pub fn set_selection(
        &mut self,
        anchor_line: usize,
        anchor_col: usize,
        caret_line: usize,
        caret_col: usize,
    ) {
        self.selections = vec![Selection::range(anchor_line, anchor_col, caret_line, caret_col)];
    }

    /// Selects the entire document (line 1 to last line).
    pub fn select_all(&mut self, line_count: usize) {
        self.selections = vec![Selection::range(1, 1, line_count, usize::MAX)];
    }

    /// Returns true if any selection has a range (not just cursor).
    pub fn has_selection(&self) -> bool {
        self.selections.iter().any(|s| !s.is_empty())
    }

    /// Clears all selections, keeping carets at their current positions.
    pub fn clear_selections(&mut self) {
        for sel in &mut self.selections {
            sel.anchor = sel.caret;
        }
    }

    /// Moves the primary cursor to a specific line (go-to-line).
    pub fn go_to_line(&mut self, line: usize) {
        self.selections = vec![Selection::cursor_only(line, 1)];
    }
}

impl Default for MultiCaret {
    fn default() -> Self {
        Self::new()
    }
}

// ── Minimap ─────────────────────────────────────────────────────────

/// Configuration and state for the editor minimap.
#[derive(Debug, Clone)]
pub struct Minimap {
    /// Whether the minimap is visible.
    pub visible: bool,
    /// Width of the minimap in pixels.
    pub width: u32,
    /// The first visible line in the main editor (1-based).
    pub viewport_start: usize,
    /// The last visible line in the main editor (1-based).
    pub viewport_end: usize,
    /// Total line count of the document.
    pub total_lines: usize,
}

impl Minimap {
    pub fn new() -> Self {
        Self {
            visible: true,
            width: 80,
            viewport_start: 1,
            viewport_end: 40,
            total_lines: 0,
        }
    }

    /// Updates the minimap state from editor viewport.
    pub fn update(&mut self, viewport_start: usize, viewport_end: usize, total_lines: usize) {
        self.viewport_start = viewport_start;
        self.viewport_end = viewport_end;
        self.total_lines = total_lines;
    }

    /// Toggles minimap visibility.
    pub fn toggle(&mut self) -> bool {
        self.visible = !self.visible;
        self.visible
    }

    /// Returns the fraction of the document visible in the viewport (0.0–1.0).
    pub fn viewport_fraction(&self) -> f64 {
        if self.total_lines == 0 {
            return 1.0;
        }
        let visible = (self.viewport_end.saturating_sub(self.viewport_start) + 1) as f64;
        (visible / self.total_lines as f64).min(1.0)
    }

    /// Returns the scroll position as a fraction (0.0–1.0).
    pub fn scroll_fraction(&self) -> f64 {
        if self.total_lines <= 1 {
            return 0.0;
        }
        (self.viewport_start.saturating_sub(1) as f64) / (self.total_lines.saturating_sub(1) as f64)
    }

    /// Converts a click Y position (0.0–1.0 fraction) to a source line number.
    pub fn click_to_line(&self, fraction: f64) -> usize {
        let line = (fraction * self.total_lines as f64).round() as usize;
        line.max(1).min(self.total_lines.max(1))
    }
}

impl Default for Minimap {
    fn default() -> Self {
        Self::new()
    }
}

// ── Diagnostics ─────────────────────────────────────────────────────

/// The severity of a diagnostic message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum DiagnosticSeverity {
    /// An error that prevents the script from running.
    Error,
    /// A warning about potential issues.
    Warning,
    /// An informational hint.
    Hint,
}

/// A diagnostic message (error, warning, or hint) in the script.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    /// The severity level.
    pub severity: DiagnosticSeverity,
    /// 1-based line number.
    pub line: usize,
    /// 1-based column number (0 = whole line).
    pub col: usize,
    /// The diagnostic message.
    pub message: String,
    /// Optional error code (e.g., "E001").
    pub code: Option<String>,
}

/// Manages diagnostics for a script.
#[derive(Debug, Clone, Default)]
pub struct DiagnosticList {
    diagnostics: Vec<Diagnostic>,
}

impl DiagnosticList {
    pub fn new() -> Self {
        Self::default()
    }

    /// Replaces all diagnostics.
    pub fn set(&mut self, diagnostics: Vec<Diagnostic>) {
        self.diagnostics = diagnostics;
        self.diagnostics
            .sort_by(|a, b| a.line.cmp(&b.line).then(a.col.cmp(&b.col)));
    }

    /// Clears all diagnostics.
    pub fn clear(&mut self) {
        self.diagnostics.clear();
    }

    /// Returns all diagnostics.
    pub fn all(&self) -> &[Diagnostic] {
        &self.diagnostics
    }

    /// Returns diagnostics at a specific line.
    pub fn at_line(&self, line: usize) -> Vec<&Diagnostic> {
        self.diagnostics.iter().filter(|d| d.line == line).collect()
    }

    /// Returns only errors.
    pub fn errors(&self) -> Vec<&Diagnostic> {
        self.diagnostics
            .iter()
            .filter(|d| d.severity == DiagnosticSeverity::Error)
            .collect()
    }

    /// Returns only warnings.
    pub fn warnings(&self) -> Vec<&Diagnostic> {
        self.diagnostics
            .iter()
            .filter(|d| d.severity == DiagnosticSeverity::Warning)
            .collect()
    }

    /// Returns the total count.
    pub fn count(&self) -> usize {
        self.diagnostics.len()
    }

    /// Returns the count of errors.
    pub fn error_count(&self) -> usize {
        self.diagnostics
            .iter()
            .filter(|d| d.severity == DiagnosticSeverity::Error)
            .count()
    }

    /// Returns the count of warnings.
    pub fn warning_count(&self) -> usize {
        self.diagnostics
            .iter()
            .filter(|d| d.severity == DiagnosticSeverity::Warning)
            .count()
    }

    /// Returns true if there are any errors.
    pub fn has_errors(&self) -> bool {
        self.diagnostics
            .iter()
            .any(|d| d.severity == DiagnosticSeverity::Error)
    }

    /// Returns the next diagnostic after the given line.
    pub fn next_diagnostic(&self, line: usize) -> Option<&Diagnostic> {
        self.diagnostics.iter().find(|d| d.line > line)
    }

    /// Returns the previous diagnostic before the given line.
    pub fn prev_diagnostic(&self, line: usize) -> Option<&Diagnostic> {
        self.diagnostics.iter().rev().find(|d| d.line < line)
    }
}

// ── Method Outline ──────────────────────────────────────────────────

/// The kind of an outline entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutlineKind {
    /// A function/method definition.
    Function,
    /// A class definition.
    Class,
    /// A signal declaration.
    Signal,
    /// An enum declaration.
    Enum,
    /// A constant.
    Constant,
    /// An exported variable.
    Export,
}

/// A single entry in the script outline (method list / symbol browser).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutlineEntry {
    /// The symbol name.
    pub name: String,
    /// The kind of symbol.
    pub kind: OutlineKind,
    /// 1-based line number where the symbol is defined.
    pub line: usize,
    /// Indentation depth (0 = top-level, 1 = nested in class, etc.).
    pub depth: usize,
}

/// Extracts a method/symbol outline from GDScript source.
pub fn extract_outline(source: &str) -> Vec<OutlineEntry> {
    let mut entries = Vec::new();
    for (i, line) in source.lines().enumerate() {
        let trimmed = line.trim();
        let indent = line.len() - line.trim_start().len();
        let depth = indent / 4; // GDScript uses 4-space (or tab) indentation

        if let Some(rest) = trimmed.strip_prefix("func ") {
            let name = rest.split(|c: char| c == '(' || c == ':' || c.is_whitespace())
                .next()
                .unwrap_or("")
                .to_string();
            if !name.is_empty() {
                entries.push(OutlineEntry { name, kind: OutlineKind::Function, line: i + 1, depth });
            }
        } else if let Some(rest) = trimmed.strip_prefix("class ") {
            let name = rest.split(|c: char| c == ':' || c == '(' || c.is_whitespace())
                .next()
                .unwrap_or("")
                .to_string();
            if !name.is_empty() {
                entries.push(OutlineEntry { name, kind: OutlineKind::Class, line: i + 1, depth });
            }
        } else if let Some(rest) = trimmed.strip_prefix("signal ") {
            let name = rest.split(|c: char| c == '(' || c.is_whitespace())
                .next()
                .unwrap_or("")
                .to_string();
            if !name.is_empty() {
                entries.push(OutlineEntry { name, kind: OutlineKind::Signal, line: i + 1, depth });
            }
        } else if let Some(rest) = trimmed.strip_prefix("enum ") {
            let name = rest.split(|c: char| c == '{' || c == ':' || c.is_whitespace())
                .next()
                .unwrap_or("")
                .to_string();
            if !name.is_empty() {
                entries.push(OutlineEntry { name, kind: OutlineKind::Enum, line: i + 1, depth });
            }
        } else if let Some(rest) = trimmed.strip_prefix("const ") {
            let name = rest.split(|c: char| c == '=' || c == ':' || c.is_whitespace())
                .next()
                .unwrap_or("")
                .to_string();
            if !name.is_empty() {
                entries.push(OutlineEntry { name, kind: OutlineKind::Constant, line: i + 1, depth });
            }
        } else if trimmed.starts_with("@export") {
            // Look for the variable name after @export ... var
            if let Some(var_pos) = trimmed.find("var ") {
                let after_var = &trimmed[var_pos + 4..];
                let name = after_var.split(|c: char| c == '=' || c == ':' || c.is_whitespace())
                    .next()
                    .unwrap_or("")
                    .to_string();
                if !name.is_empty() {
                    entries.push(OutlineEntry { name, kind: OutlineKind::Export, line: i + 1, depth });
                }
            }
        }
    }
    entries
}

// ── Script List (panel) ─────────────────────────────────────────────

/// An entry in the script list panel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptListEntry {
    /// The resource path (e.g., "res://player.gd").
    pub path: String,
    /// The display name (filename without directory).
    pub display_name: String,
    /// Whether the script has unsaved changes.
    pub modified: bool,
    /// The tab index in the ScriptEditor.
    pub tab_index: usize,
}

/// Builds a script list from the current ScriptEditor state.
pub fn build_script_list(editor: &ScriptEditor) -> Vec<ScriptListEntry> {
    let mut entries = Vec::new();
    for (i, tab) in editor.tabs().iter().enumerate() {
        let display_name = tab.path.rsplit('/').next().unwrap_or(&tab.path).to_string();
        entries.push(ScriptListEntry {
            path: tab.path.clone(),
            display_name,
            modified: tab.modified,
            tab_index: i,
        });
    }
    entries
}

// ── Status Bar ──────────────────────────────────────────────────────

/// Information displayed in the script editor status bar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptStatusBar {
    /// Current cursor line (1-based).
    pub line: usize,
    /// Current cursor column (1-based).
    pub col: usize,
    /// Total line count of the active script.
    pub total_lines: usize,
    /// Number of selections/carets.
    pub selection_count: usize,
    /// Indentation mode display string (e.g., "Spaces: 4" or "Tabs").
    pub indent_mode: String,
    /// The script language (always "GDScript" for now).
    pub language: String,
    /// Number of errors in the active script.
    pub error_count: usize,
    /// Number of warnings in the active script.
    pub warning_count: usize,
}

impl ScriptStatusBar {
    /// Builds a status bar from the current editor state.
    pub fn from_editor(
        tab: &ScriptTab,
        carets: &MultiCaret,
        diagnostics: &DiagnosticList,
    ) -> Self {
        Self {
            line: tab.cursor_line,
            col: tab.cursor_col,
            total_lines: tab.line_count(),
            selection_count: carets.caret_count(),
            indent_mode: "Spaces: 4".into(),
            language: "GDScript".into(),
            error_count: diagnostics.error_count(),
            warning_count: diagnostics.warning_count(),
        }
    }

    /// Returns a formatted status string like "Ln 5, Col 12 | 100 lines | GDScript".
    pub fn display(&self) -> String {
        let mut s = format!("Ln {}, Col {} | {} lines | {}", self.line, self.col, self.total_lines, self.language);
        if self.selection_count > 1 {
            s.push_str(&format!(" | {} carets", self.selection_count));
        }
        if self.error_count > 0 || self.warning_count > 0 {
            s.push_str(&format!(" | {} errors, {} warnings", self.error_count, self.warning_count));
        }
        s
    }
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

    // ── CompletionProvider tests ────────────────────────────────────

    #[test]
    fn completion_empty_prefix_returns_nothing() {
        let cp = CompletionProvider::new();
        assert!(cp.complete("", "var x = 1").is_empty());
    }

    #[test]
    fn completion_keyword_prefix() {
        let cp = CompletionProvider::new();
        let items = cp.complete("va", "");
        assert!(items.iter().any(|i| i.label == "var" && i.kind == CompletionKind::Keyword));
    }

    #[test]
    fn completion_class_prefix() {
        let cp = CompletionProvider::new();
        let items = cp.complete("Nod", "");
        assert!(items.iter().any(|i| i.label == "Node" && i.kind == CompletionKind::Class));
        assert!(items.iter().any(|i| i.label == "Node2D"));
        assert!(items.iter().any(|i| i.label == "Node3D"));
    }

    #[test]
    fn completion_function_prefix() {
        let cp = CompletionProvider::new();
        let items = cp.complete("pri", "");
        let print_item = items.iter().find(|i| i.label == "print()");
        assert!(print_item.is_some());
        assert_eq!(print_item.unwrap().insert_text, "print(");
        assert_eq!(print_item.unwrap().kind, CompletionKind::Function);
    }

    #[test]
    fn completion_source_identifiers() {
        let cp = CompletionProvider::new();
        let source = "var my_speed = 10\nfunc my_func():\n    pass";
        let items = cp.complete("my", source);
        assert!(items.iter().any(|i| i.label == "my_speed"));
        assert!(items.iter().any(|i| i.label == "my_func"));
    }

    #[test]
    fn completion_case_insensitive() {
        let cp = CompletionProvider::new();
        let items = cp.complete("node", "");
        assert!(items.iter().any(|i| i.label == "Node"));
    }

    #[test]
    fn completion_deduplicates() {
        let cp = CompletionProvider::new();
        let source = "var foo = 1\nvar bar = foo";
        let items = cp.complete("fo", source);
        let foo_count = items.iter().filter(|i| i.label == "foo").count();
        assert_eq!(foo_count, 1);
    }

    #[test]
    fn completion_source_detects_const_kind() {
        let cp = CompletionProvider::new();
        let source = "const MAX_HP = 100";
        let items = cp.complete("MAX", source);
        let max_item = items.iter().find(|i| i.label == "MAX_HP");
        assert!(max_item.is_some());
        assert_eq!(max_item.unwrap().kind, CompletionKind::Constant);
    }

    #[test]
    fn completion_source_detects_signal_kind() {
        let cp = CompletionProvider::new();
        let source = "signal health_changed";
        let items = cp.complete("health", source);
        let sig = items.iter().find(|i| i.label == "health_changed");
        assert!(sig.is_some());
        assert_eq!(sig.unwrap().kind, CompletionKind::Signal);
    }

    // ── CodeFolding tests ──────────────────────────────────────────

    #[test]
    fn folding_toggle() {
        let mut cf = CodeFolding::new();
        assert!(!cf.is_folded(5));
        assert!(cf.toggle(5));  // now folded
        assert!(cf.is_folded(5));
        assert!(!cf.toggle(5)); // now unfolded
        assert!(!cf.is_folded(5));
    }

    #[test]
    fn folding_fold_unfold() {
        let mut cf = CodeFolding::new();
        cf.fold(10);
        cf.fold(20);
        assert_eq!(cf.folded_count(), 2);
        assert_eq!(cf.folded_lines(), vec![10, 20]);
        cf.unfold(10);
        assert_eq!(cf.folded_count(), 1);
        assert!(!cf.is_folded(10));
    }

    #[test]
    fn folding_fold_all_unfold_all() {
        let mut cf = CodeFolding::new();
        let regions = vec![
            FoldRegion { start_line: 1, end_line: 5, kind: FoldKind::Function },
            FoldRegion { start_line: 7, end_line: 12, kind: FoldKind::Class },
        ];
        cf.fold_all(&regions);
        assert_eq!(cf.folded_count(), 2);
        cf.unfold_all();
        assert_eq!(cf.folded_count(), 0);
    }

    #[test]
    fn detect_fold_regions_function() {
        let source = "func _ready():\n    print(\"hi\")\n    pass\n\nvar x = 1";
        let regions = detect_fold_regions(source);
        assert_eq!(regions.len(), 1);
        assert_eq!(regions[0].kind, FoldKind::Function);
        assert_eq!(regions[0].start_line, 1);
        assert_eq!(regions[0].end_line, 3);
    }

    #[test]
    fn detect_fold_regions_class() {
        let source = "class MyClass:\n    var x = 1\n    func f():\n        pass\n";
        let regions = detect_fold_regions(source);
        assert!(regions.iter().any(|r| r.kind == FoldKind::Class));
    }

    #[test]
    fn detect_fold_regions_conditional() {
        let source = "if x > 0:\n    print(x)\nelse:\n    print(0)\n";
        let regions = detect_fold_regions(source);
        assert!(regions.iter().any(|r| r.kind == FoldKind::Conditional && r.start_line == 1));
        assert!(regions.iter().any(|r| r.kind == FoldKind::Conditional && r.start_line == 3));
    }

    #[test]
    fn detect_fold_regions_loop() {
        let source = "for i in range(10):\n    print(i)\n";
        let regions = detect_fold_regions(source);
        assert_eq!(regions.len(), 1);
        assert_eq!(regions[0].kind, FoldKind::Loop);
    }

    #[test]
    fn detect_fold_regions_match() {
        let source = "match state:\n    \"idle\":\n        pass\n    \"run\":\n        pass\n";
        let regions = detect_fold_regions(source);
        assert!(regions.iter().any(|r| r.kind == FoldKind::Match));
    }

    #[test]
    fn detect_fold_regions_region_comments() {
        let source = "#region Exports\nvar a = 1\nvar b = 2\n#endregion\n";
        let regions = detect_fold_regions(source);
        assert_eq!(regions.len(), 1);
        assert_eq!(regions[0].kind, FoldKind::Region);
        assert_eq!(regions[0].start_line, 1);
        assert_eq!(regions[0].end_line, 4);
    }

    #[test]
    fn detect_fold_regions_empty_source() {
        assert!(detect_fold_regions("").is_empty());
    }

    // ── MultiCaret tests ───────────────────────────────────────────

    #[test]
    fn multi_caret_new_has_single_cursor() {
        let mc = MultiCaret::new();
        assert_eq!(mc.caret_count(), 1);
        assert_eq!(mc.primary().caret, Caret::new(1, 1));
        assert!(!mc.has_selection());
    }

    #[test]
    fn multi_caret_add_cursor() {
        let mut mc = MultiCaret::new();
        mc.add_cursor(3, 5);
        assert_eq!(mc.caret_count(), 2);
        // Should be sorted by position.
        assert_eq!(mc.selections()[0].caret, Caret::new(1, 1));
        assert_eq!(mc.selections()[1].caret, Caret::new(3, 5));
    }

    #[test]
    fn multi_caret_add_duplicate_ignored() {
        let mut mc = MultiCaret::new();
        mc.add_cursor(1, 1); // same as initial
        assert_eq!(mc.caret_count(), 1);
    }

    #[test]
    fn multi_caret_remove_cursor() {
        let mut mc = MultiCaret::new();
        mc.add_cursor(3, 5);
        assert!(mc.remove_cursor(1, 1));
        assert_eq!(mc.caret_count(), 1);
        assert_eq!(mc.primary().caret, Caret::new(3, 5));
    }

    #[test]
    fn multi_caret_remove_last_cursor_fails() {
        let mut mc = MultiCaret::new();
        assert!(!mc.remove_cursor(1, 1));
        assert_eq!(mc.caret_count(), 1);
    }

    #[test]
    fn multi_caret_set_cursor_clears_multi() {
        let mut mc = MultiCaret::new();
        mc.add_cursor(3, 5);
        mc.add_cursor(7, 1);
        mc.set_cursor(10, 2);
        assert_eq!(mc.caret_count(), 1);
        assert_eq!(mc.primary().caret, Caret::new(10, 2));
    }

    #[test]
    fn multi_caret_set_selection() {
        let mut mc = MultiCaret::new();
        mc.set_selection(1, 1, 3, 10);
        assert!(mc.has_selection());
        let (start, end) = mc.primary().ordered();
        assert_eq!(start, Caret::new(1, 1));
        assert_eq!(end, Caret::new(3, 10));
    }

    #[test]
    fn multi_caret_select_all() {
        let mut mc = MultiCaret::new();
        mc.select_all(50);
        assert!(mc.has_selection());
        let (start, end) = mc.primary().ordered();
        assert_eq!(start, Caret::new(1, 1));
        assert_eq!(end.line, 50);
    }

    #[test]
    fn multi_caret_clear_selections() {
        let mut mc = MultiCaret::new();
        mc.set_selection(1, 1, 5, 10);
        assert!(mc.has_selection());
        mc.clear_selections();
        assert!(!mc.has_selection());
    }

    #[test]
    fn multi_caret_go_to_line() {
        let mut mc = MultiCaret::new();
        mc.add_cursor(5, 3);
        mc.go_to_line(42);
        assert_eq!(mc.caret_count(), 1);
        assert_eq!(mc.primary().caret, Caret::new(42, 1));
    }

    #[test]
    fn selection_cursor_only_is_empty() {
        let sel = Selection::cursor_only(5, 10);
        assert!(sel.is_empty());
        assert_eq!(sel.ordered(), (Caret::new(5, 10), Caret::new(5, 10)));
    }

    #[test]
    fn selection_range_ordered_forward() {
        let sel = Selection::range(1, 1, 5, 10);
        assert!(!sel.is_empty());
        let (start, end) = sel.ordered();
        assert_eq!(start, Caret::new(1, 1));
        assert_eq!(end, Caret::new(5, 10));
    }

    #[test]
    fn selection_range_ordered_backward() {
        let sel = Selection::range(5, 10, 1, 1);
        let (start, end) = sel.ordered();
        assert_eq!(start, Caret::new(1, 1));
        assert_eq!(end, Caret::new(5, 10));
    }

    // ── Minimap tests ──────────────────────────────────────────────

    #[test]
    fn minimap_defaults() {
        let m = Minimap::new();
        assert!(m.visible);
        assert_eq!(m.width, 80);
    }

    #[test]
    fn minimap_toggle() {
        let mut m = Minimap::new();
        assert!(!m.toggle()); // now hidden
        assert!(!m.visible);
        assert!(m.toggle()); // now visible
        assert!(m.visible);
    }

    #[test]
    fn minimap_viewport_fraction() {
        let mut m = Minimap::new();
        m.update(1, 40, 200);
        let frac = m.viewport_fraction();
        assert!((frac - 0.2).abs() < 0.01);
    }

    #[test]
    fn minimap_viewport_fraction_empty() {
        let m = Minimap::new();
        assert_eq!(m.viewport_fraction(), 1.0);
    }

    #[test]
    fn minimap_scroll_fraction() {
        let mut m = Minimap::new();
        m.update(101, 140, 200);
        let frac = m.scroll_fraction();
        assert!((frac - 0.5025).abs() < 0.01);
    }

    #[test]
    fn minimap_scroll_fraction_top() {
        let mut m = Minimap::new();
        m.update(1, 40, 200);
        assert_eq!(m.scroll_fraction(), 0.0);
    }

    #[test]
    fn minimap_click_to_line() {
        let mut m = Minimap::new();
        m.update(1, 40, 100);
        assert_eq!(m.click_to_line(0.0), 1);
        assert_eq!(m.click_to_line(0.5), 50);
        assert_eq!(m.click_to_line(1.0), 100);
    }

    // ── DiagnosticList tests ───────────────────────────────────────

    #[test]
    fn diagnostic_list_empty() {
        let dl = DiagnosticList::new();
        assert_eq!(dl.count(), 0);
        assert_eq!(dl.error_count(), 0);
        assert!(!dl.has_errors());
    }

    #[test]
    fn diagnostic_list_set_and_query() {
        let mut dl = DiagnosticList::new();
        dl.set(vec![
            Diagnostic {
                severity: DiagnosticSeverity::Error,
                line: 10,
                col: 5,
                message: "undefined var".into(),
                code: Some("E001".into()),
            },
            Diagnostic {
                severity: DiagnosticSeverity::Warning,
                line: 5,
                col: 1,
                message: "unused var".into(),
                code: None,
            },
            Diagnostic {
                severity: DiagnosticSeverity::Hint,
                line: 15,
                col: 0,
                message: "consider renaming".into(),
                code: None,
            },
        ]);
        assert_eq!(dl.count(), 3);
        assert_eq!(dl.error_count(), 1);
        assert_eq!(dl.warning_count(), 1);
        assert!(dl.has_errors());
        // Sorted by line.
        assert_eq!(dl.all()[0].line, 5);
        assert_eq!(dl.all()[1].line, 10);
        assert_eq!(dl.all()[2].line, 15);
    }

    #[test]
    fn diagnostic_list_at_line() {
        let mut dl = DiagnosticList::new();
        dl.set(vec![
            Diagnostic { severity: DiagnosticSeverity::Error, line: 5, col: 1, message: "a".into(), code: None },
            Diagnostic { severity: DiagnosticSeverity::Warning, line: 5, col: 10, message: "b".into(), code: None },
            Diagnostic { severity: DiagnosticSeverity::Error, line: 10, col: 1, message: "c".into(), code: None },
        ]);
        assert_eq!(dl.at_line(5).len(), 2);
        assert_eq!(dl.at_line(10).len(), 1);
        assert_eq!(dl.at_line(1).len(), 0);
    }

    #[test]
    fn diagnostic_list_errors_and_warnings() {
        let mut dl = DiagnosticList::new();
        dl.set(vec![
            Diagnostic { severity: DiagnosticSeverity::Error, line: 1, col: 1, message: "err".into(), code: None },
            Diagnostic { severity: DiagnosticSeverity::Warning, line: 2, col: 1, message: "warn".into(), code: None },
        ]);
        assert_eq!(dl.errors().len(), 1);
        assert_eq!(dl.warnings().len(), 1);
    }

    #[test]
    fn diagnostic_list_navigation() {
        let mut dl = DiagnosticList::new();
        dl.set(vec![
            Diagnostic { severity: DiagnosticSeverity::Error, line: 5, col: 1, message: "a".into(), code: None },
            Diagnostic { severity: DiagnosticSeverity::Warning, line: 15, col: 1, message: "b".into(), code: None },
            Diagnostic { severity: DiagnosticSeverity::Error, line: 25, col: 1, message: "c".into(), code: None },
        ]);
        // Next from line 1 → line 5.
        assert_eq!(dl.next_diagnostic(1).unwrap().line, 5);
        // Next from line 10 → line 15.
        assert_eq!(dl.next_diagnostic(10).unwrap().line, 15);
        // Prev from line 20 → line 15.
        assert_eq!(dl.prev_diagnostic(20).unwrap().line, 15);
        // Prev from line 3 → None.
        assert!(dl.prev_diagnostic(3).is_none());
        // Next from line 30 → None.
        assert!(dl.next_diagnostic(30).is_none());
    }

    #[test]
    fn diagnostic_list_clear() {
        let mut dl = DiagnosticList::new();
        dl.set(vec![
            Diagnostic { severity: DiagnosticSeverity::Error, line: 1, col: 1, message: "err".into(), code: None },
        ]);
        assert_eq!(dl.count(), 1);
        dl.clear();
        assert_eq!(dl.count(), 0);
        assert!(!dl.has_errors());
    }

    // ── Outline tests ─────────────────────────────────────────────

    #[test]
    fn outline_extracts_functions() {
        let source = "func _ready():\n    pass\n\nfunc _process(delta):\n    pass";
        let outline = extract_outline(source);
        assert_eq!(outline.len(), 2);
        assert_eq!(outline[0].name, "_ready");
        assert_eq!(outline[0].kind, OutlineKind::Function);
        assert_eq!(outline[0].line, 1);
        assert_eq!(outline[1].name, "_process");
        assert_eq!(outline[1].line, 4);
    }

    #[test]
    fn outline_extracts_class() {
        let source = "class MyClass:\n    var x = 1";
        let outline = extract_outline(source);
        assert!(outline.iter().any(|e| e.name == "MyClass" && e.kind == OutlineKind::Class));
    }

    #[test]
    fn outline_extracts_signals() {
        let source = "signal health_changed\nsignal mana_changed(amount)";
        let outline = extract_outline(source);
        assert_eq!(outline.len(), 2);
        assert_eq!(outline[0].name, "health_changed");
        assert_eq!(outline[0].kind, OutlineKind::Signal);
        assert_eq!(outline[1].name, "mana_changed");
    }

    #[test]
    fn outline_extracts_enums() {
        let source = "enum State { IDLE, RUN, JUMP }";
        let outline = extract_outline(source);
        assert_eq!(outline.len(), 1);
        assert_eq!(outline[0].name, "State");
        assert_eq!(outline[0].kind, OutlineKind::Enum);
    }

    #[test]
    fn outline_extracts_constants() {
        let source = "const MAX_SPEED = 200\nconst GRAVITY: float = 9.8";
        let outline = extract_outline(source);
        assert_eq!(outline.len(), 2);
        assert_eq!(outline[0].name, "MAX_SPEED");
        assert_eq!(outline[0].kind, OutlineKind::Constant);
        assert_eq!(outline[1].name, "GRAVITY");
    }

    #[test]
    fn outline_extracts_exports() {
        let source = "@export var health: int = 100\n@export_range(0, 100) var speed: float = 50.0";
        let outline = extract_outline(source);
        assert_eq!(outline.len(), 2);
        assert_eq!(outline[0].name, "health");
        assert_eq!(outline[0].kind, OutlineKind::Export);
        assert_eq!(outline[1].name, "speed");
    }

    #[test]
    fn outline_nested_depth() {
        let source = "class Outer:\n    func inner_method():\n        pass";
        let outline = extract_outline(source);
        let cls = outline.iter().find(|e| e.name == "Outer").unwrap();
        assert_eq!(cls.depth, 0);
        let method = outline.iter().find(|e| e.name == "inner_method").unwrap();
        assert_eq!(method.depth, 1);
    }

    #[test]
    fn outline_empty_source() {
        assert!(extract_outline("").is_empty());
    }

    #[test]
    fn outline_sample_script() {
        let outline = extract_outline(SAMPLE_GD);
        // SAMPLE_GD has: var speed, @export var health, func _ready, func _process
        assert!(outline.iter().any(|e| e.name == "_ready" && e.kind == OutlineKind::Function));
        assert!(outline.iter().any(|e| e.name == "_process" && e.kind == OutlineKind::Function));
        assert!(outline.iter().any(|e| e.name == "health" && e.kind == OutlineKind::Export));
    }

    // ── Script List tests ─────────────────────────────────────────

    #[test]
    fn script_list_empty_editor() {
        let editor = ScriptEditor::new();
        let list = build_script_list(&editor);
        assert!(list.is_empty());
    }

    #[test]
    fn script_list_shows_open_tabs() {
        let mut editor = ScriptEditor::new();
        editor.open("res://player.gd", "var x = 1");
        editor.open("res://enemy.gd", "var y = 2");
        let list = build_script_list(&editor);
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].path, "res://player.gd");
        assert_eq!(list[0].display_name, "player.gd");
        assert_eq!(list[0].tab_index, 0);
        assert_eq!(list[1].display_name, "enemy.gd");
    }

    #[test]
    fn script_list_tracks_modified() {
        let mut editor = ScriptEditor::new();
        editor.open("res://test.gd", "original");
        let list = build_script_list(&editor);
        assert!(!list[0].modified);
        editor.active_mut().unwrap().set_source("changed");
        let list = build_script_list(&editor);
        assert!(list[0].modified);
    }

    // ── Status Bar tests ──────────────────────────────────────────

    #[test]
    fn status_bar_basic() {
        let mut tab = ScriptTab::new("test.gd", "line1\nline2\nline3");
        tab.set_cursor(2, 5);
        let carets = MultiCaret::new();
        let diag = DiagnosticList::new();
        let bar = ScriptStatusBar::from_editor(&tab, &carets, &diag);
        assert_eq!(bar.line, 2);
        assert_eq!(bar.col, 5);
        assert_eq!(bar.total_lines, 3);
        assert_eq!(bar.selection_count, 1);
        assert_eq!(bar.language, "GDScript");
        assert_eq!(bar.error_count, 0);
    }

    #[test]
    fn status_bar_display_simple() {
        let tab = ScriptTab::new("test.gd", "line1\nline2");
        let carets = MultiCaret::new();
        let diag = DiagnosticList::new();
        let bar = ScriptStatusBar::from_editor(&tab, &carets, &diag);
        let display = bar.display();
        assert!(display.contains("Ln 1"));
        assert!(display.contains("Col 1"));
        assert!(display.contains("2 lines"));
        assert!(display.contains("GDScript"));
        assert!(!display.contains("carets"));
        assert!(!display.contains("errors"));
    }

    #[test]
    fn status_bar_display_multi_caret() {
        let tab = ScriptTab::new("test.gd", "abc");
        let mut carets = MultiCaret::new();
        carets.add_cursor(1, 3);
        let diag = DiagnosticList::new();
        let bar = ScriptStatusBar::from_editor(&tab, &carets, &diag);
        assert_eq!(bar.selection_count, 2);
        assert!(bar.display().contains("2 carets"));
    }

    #[test]
    fn status_bar_display_with_diagnostics() {
        let tab = ScriptTab::new("test.gd", "x");
        let carets = MultiCaret::new();
        let mut diag = DiagnosticList::new();
        diag.set(vec![
            Diagnostic { severity: DiagnosticSeverity::Error, line: 1, col: 1, message: "err".into(), code: None },
            Diagnostic { severity: DiagnosticSeverity::Warning, line: 1, col: 2, message: "warn".into(), code: None },
        ]);
        let bar = ScriptStatusBar::from_editor(&tab, &carets, &diag);
        assert_eq!(bar.error_count, 1);
        assert_eq!(bar.warning_count, 1);
        assert!(bar.display().contains("1 errors"));
        assert!(bar.display().contains("1 warnings"));
    }

    #[test]
    fn editor_tabs_accessor() {
        let mut e = ScriptEditor::new();
        e.open("a.gd", "src_a");
        e.open("b.gd", "src_b");
        assert_eq!(e.tabs().len(), 2);
        assert_eq!(e.tabs()[0].path, "a.gd");
    }
}
