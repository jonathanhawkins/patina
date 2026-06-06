//! The SCRIPT main-screen view: the editable, highlighted code pane.
//!
//! In Godot the SCRIPT main-screen mode replaces the central viewport with a
//! full code editor. This composes that central pane from the active main view
//! ([`MainView`]) and the currently selected scripted node: only when SCRIPT
//! mode is active *and* a node with an attached script is selected does the
//! pane show that node's source as an editable, syntax-highlighted buffer wired
//! to save. Any other combination shows the "select a node with a script"
//! prompt, mirroring the editor's empty state.

use crate::main_screen::MainView;
use crate::script_completion::{CompletionContext, CompletionEngine, CompletionItem};
use crate::script_editor::{HighlightSpan, SyntaxHighlighter};

/// A located occurrence of a find query in the pane's buffer: a 1-based line
/// number, the 0-based byte column where the match starts within that line, and
/// the match's byte length. Identifiers/queries are ASCII in GDScript, so byte
/// offsets line up with characters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FindMatch {
    /// 1-based line number the match was found on.
    pub line: usize,
    /// 0-based byte offset of the match's start within the line.
    pub column: usize,
    /// Byte length of the matched text (equal to the query length).
    pub length: usize,
}

/// The prompt shown when no scripted node is available to edit.
pub const EMPTY_PROMPT: &str = "Select a node with a script to view its content";

/// What the SCRIPT main view renders in the central editor area.
#[derive(Debug, Clone, PartialEq)]
pub enum ScriptMainView {
    /// No scripted node is selected (or the active view isn't the script
    /// editor) — the editor shows the empty prompt instead of a code pane.
    Empty {
        /// The prompt text shown in place of a code pane.
        prompt: String,
    },
    /// An editable, syntax-highlighted code pane for the selected node's script.
    Editor(ScriptPane),
}

/// An editable, syntax-highlighted code pane bound to a single script file.
#[derive(Debug, Clone, PartialEq)]
pub struct ScriptPane {
    /// The script's path (`res://...` or absolute) — what the pane is editing.
    path: String,
    /// The current buffer contents.
    source: String,
    /// Syntax-highlight spans for `source`, recomputed on every edit.
    highlight: Vec<HighlightSpan>,
    /// Whether the buffer has unsaved edits since it was opened or last saved.
    dirty: bool,
}

impl ScriptPane {
    /// Builds a pane for `path` holding `source`, highlighting it immediately.
    fn new(path: impl Into<String>, source: impl Into<String>) -> Self {
        let path = path.into();
        let source = source.into();
        let highlight = highlight_source(&source);
        Self {
            path,
            source,
            highlight,
            dirty: false,
        }
    }

    /// The script path the pane is editing (and the target it saves back to).
    pub fn path(&self) -> &str {
        &self.path
    }

    /// The current buffer contents.
    pub fn source(&self) -> &str {
        &self.source
    }

    /// The syntax-highlight spans for the current buffer. Non-empty for any
    /// non-blank GDScript source — this is what makes the pane "highlighted".
    pub fn highlight(&self) -> &[HighlightSpan] {
        &self.highlight
    }

    /// The number of lines shown in the pane's gutter: newlines + 1, so a
    /// trailing newline yields an extra (empty) trailing line, matching a code
    /// editor's gutter. An empty buffer still shows a single line.
    pub fn line_count(&self) -> usize {
        self.source.split('\n').count()
    }

    /// The 1-based line numbers rendered in the pane's gutter — one per source
    /// line, `[1, 2, …, line_count]`. Recomputed from the buffer, so it tracks
    /// edits.
    pub fn line_numbers(&self) -> Vec<usize> {
        (1..=self.line_count()).collect()
    }

    /// A code pane is always editable — the SCRIPT main view is a real editor,
    /// not a read-only preview.
    pub fn is_editable(&self) -> bool {
        true
    }

    /// Whether the buffer has unsaved edits.
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// Applies an edit: replaces the buffer, re-highlights, and marks the pane
    /// dirty (an edit that doesn't change the text leaves the dirty flag alone).
    pub fn edit(&mut self, new_source: impl Into<String>) {
        let new_source = new_source.into();
        if new_source == self.source {
            return;
        }
        self.source = new_source;
        self.highlight = highlight_source(&self.source);
        self.dirty = true;
    }

    /// Resolves a save: clears the dirty flag and returns the `(path, source)`
    /// the caller should persist (e.g. via `POST /api/script/save`). Calling
    /// this is how the pane is "wired to save".
    pub fn save(&mut self) -> (String, String) {
        self.dirty = false;
        (self.path.clone(), self.source.clone())
    }

    /// The class this script extends, parsed from its first `extends X` line.
    /// Defaults to `"Node"` when the script has no `extends` (Godot's implicit
    /// base). Used to seed code completion with the right ClassDB members.
    pub fn current_class(&self) -> String {
        for line in self.source.lines() {
            let line = line.trim_start();
            if let Some(rest) = line.strip_prefix("extends ") {
                if let Some(name) = leading_identifier(rest.trim_start()) {
                    return name;
                }
            }
        }
        "Node".to_string()
    }

    /// The local variable/constant names declared in the buffer (`var x`,
    /// `const Y`), in source order. Feeds local-variable completions so the
    /// pane suggests symbols the user just defined.
    pub fn local_variables(&self) -> Vec<String> {
        let mut locals = Vec::new();
        for line in self.source.lines() {
            let trimmed = line.trim_start();
            let rest = trimmed
                .strip_prefix("var ")
                .or_else(|| trimmed.strip_prefix("const "));
            if let Some(rest) = rest {
                if let Some(name) = leading_identifier(rest.trim_start()) {
                    if !locals.contains(&name) {
                        locals.push(name);
                    }
                }
            }
        }
        locals
    }

    /// Autocomplete suggestions for `prefix` at the current cursor context:
    /// GDScript keywords, members of the script's [`current_class`], global
    /// class names, and the buffer's [`local_variables`], ranked by relevance.
    /// An empty `prefix` still returns keyword/member suggestions.
    ///
    /// [`current_class`]: Self::current_class
    /// [`local_variables`]: Self::local_variables
    pub fn autocomplete(&self, prefix: &str) -> Vec<CompletionItem> {
        let ctx = CompletionContext::bare(self.current_class(), prefix)
            .with_locals(self.local_variables());
        CompletionEngine::new().complete(&ctx)
    }

    /// Every occurrence of `query` in the buffer, scanned line by line
    /// (case-sensitive, non-overlapping). An empty `query` matches nothing.
    /// This is the "Find" half of find/replace.
    pub fn find(&self, query: &str) -> Vec<FindMatch> {
        let mut matches = Vec::new();
        if query.is_empty() {
            return matches;
        }
        for (idx, line) in self.source.split('\n').enumerate() {
            let mut start = 0;
            while let Some(rel) = line[start..].find(query) {
                let column = start + rel;
                matches.push(FindMatch {
                    line: idx + 1,
                    column,
                    length: query.len(),
                });
                start = column + query.len();
            }
        }
        matches
    }

    /// Replaces every occurrence of `query` with `replacement` across the
    /// buffer, re-highlighting and marking the pane dirty when anything
    /// changed, and returns the number of replacements made. An empty `query`
    /// (or no matches) is a no-op returning 0. This is the "Replace All" half
    /// of find/replace.
    pub fn replace_all(&mut self, query: &str, replacement: &str) -> usize {
        if query.is_empty() {
            return 0;
        }
        let count = self.source.matches(query).count();
        if count == 0 {
            return 0;
        }
        let replaced = self.source.replace(query, replacement);
        self.edit(replaced);
        count
    }
}

/// Extracts the leading GDScript identifier (`[A-Za-z_][A-Za-z0-9_]*`) from the
/// start of `s`, e.g. `"speed := 200"` → `Some("speed")`, `"speed: int"` →
/// `Some("speed")`. Returns `None` if `s` doesn't start with an identifier.
fn leading_identifier(s: &str) -> Option<String> {
    let mut chars = s.char_indices();
    let first = chars.next()?;
    if !(first.1.is_ascii_alphabetic() || first.1 == '_') {
        return None;
    }
    let mut end = s.len();
    for (i, c) in chars {
        if !(c.is_ascii_alphanumeric() || c == '_') {
            end = i;
            break;
        }
    }
    Some(s[..end].to_string())
}

/// The file-name component of a script path: `res://scripts/player.gd` →
/// `player.gd`. Used as the display label in the open-scripts switcher.
fn script_file_name(path: &str) -> String {
    path.rsplit(|c| c == '/' || c == '\\')
        .next()
        .unwrap_or(path)
        .to_string()
}

/// One row in the editor's "open scripts" switcher list (Godot's script-editor
/// left-hand list): the open script's path, its display file name, whether it
/// has unsaved edits, and whether it's the currently active pane.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenScriptEntry {
    /// The script's resource path — the unique key identifying the open script.
    pub path: String,
    /// The display name (the path's file-name component).
    pub name: String,
    /// Whether this script has unsaved edits.
    pub dirty: bool,
    /// Whether this is the currently active (visible) script.
    pub active: bool,
}

/// The set of scripts currently open in the editor — the model behind Godot's
/// script-editor open-scripts list and tab switcher. Tracks every open
/// [`ScriptPane`] plus which one is active. Opening a script that's already
/// open switches to it instead of adding a duplicate; the most-recently-opened
/// script becomes active.
#[derive(Debug, Clone, Default)]
pub struct OpenScripts {
    panes: Vec<ScriptPane>,
    active: usize,
}

impl OpenScripts {
    /// Creates an empty open-scripts set (no scripts open).
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether no scripts are open.
    pub fn is_empty(&self) -> bool {
        self.panes.is_empty()
    }

    /// How many scripts are open.
    pub fn len(&self) -> usize {
        self.panes.len()
    }

    /// Index of the already-open pane editing `path`, if any.
    fn index_of(&self, path: &str) -> Option<usize> {
        self.panes.iter().position(|p| p.path() == path)
    }

    /// Opens `script` in the editor: if a script with the same path is already
    /// open, switches to it (keeping its existing buffer/edits); otherwise
    /// appends a fresh pane. Either way the opened script becomes active.
    /// Returns the active script's index.
    pub fn open(&mut self, script: SelectedNodeScript) -> usize {
        if let Some(idx) = self.index_of(&script.path) {
            self.active = idx;
            return idx;
        }
        self.panes.push(ScriptPane::new(script.path, script.source));
        self.active = self.panes.len() - 1;
        self.active
    }

    /// The index of the active script, or `None` when nothing is open.
    pub fn active_index(&self) -> Option<usize> {
        if self.panes.is_empty() {
            None
        } else {
            Some(self.active)
        }
    }

    /// The active script's pane, or `None` when nothing is open.
    pub fn active(&self) -> Option<&ScriptPane> {
        self.panes.get(self.active)
    }

    /// The active script's pane mutably (e.g. to edit/save it).
    pub fn active_mut(&mut self) -> Option<&mut ScriptPane> {
        self.panes.get_mut(self.active)
    }

    /// Switches the active script to the one at `index`. Returns `false` and
    /// leaves the active script unchanged if `index` is out of range.
    pub fn switch_to(&mut self, index: usize) -> bool {
        if index < self.panes.len() {
            self.active = index;
            true
        } else {
            false
        }
    }

    /// Switches to the open script editing `path`, if one is open. Returns
    /// whether a matching open script was found (active unchanged if not).
    pub fn switch_to_path(&mut self, path: &str) -> bool {
        match self.index_of(path) {
            Some(idx) => {
                self.active = idx;
                true
            }
            None => false,
        }
    }

    /// The switcher list: one [`OpenScriptEntry`] per open script, in open
    /// order, with the active one and any dirty ones flagged.
    pub fn open_list(&self) -> Vec<OpenScriptEntry> {
        self.panes
            .iter()
            .enumerate()
            .map(|(i, p)| OpenScriptEntry {
                path: p.path().to_string(),
                name: script_file_name(p.path()),
                dirty: p.is_dirty(),
                active: i == self.active,
            })
            .collect()
    }

    /// Closes the script at `index`. Returns `false` for an out-of-range index.
    /// The active selection is kept in range: closing a script before the
    /// active one shifts the active index down; closing the active (or last)
    /// script falls back to the new last script.
    pub fn close(&mut self, index: usize) -> bool {
        if index >= self.panes.len() {
            return false;
        }
        self.panes.remove(index);
        if self.panes.is_empty() {
            self.active = 0;
        } else if self.active >= self.panes.len() {
            self.active = self.panes.len() - 1;
        } else if index < self.active {
            self.active -= 1;
        }
        true
    }
}

/// A scripted node's script, resolved for opening in the code pane: where the
/// `.gd` lives and its current source text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectedNodeScript {
    /// The script's resource path (`res://...` or absolute).
    pub path: String,
    /// The script's current source text.
    pub source: String,
}

impl ScriptMainView {
    /// Composes the SCRIPT main view from the active `view` and the selected
    /// node's script, if any. `selected_script` is `Some((path, source))` when
    /// a node with an attached script is selected, else `None`.
    ///
    /// Only `(MainView::ScriptEditor, Some(..))` yields an editable code pane;
    /// every other combination (wrong mode, or no scripted node) yields the
    /// empty prompt.
    pub fn compose(view: MainView, selected_script: Option<(&str, &str)>) -> Self {
        match (view, selected_script) {
            (MainView::ScriptEditor, Some((path, source))) => {
                ScriptMainView::Editor(ScriptPane::new(path, source))
            }
            _ => ScriptMainView::Empty {
                prompt: EMPTY_PROMPT.to_string(),
            },
        }
    }

    /// Opens the selected node's script directly in the full code pane — Godot's
    /// "open in script editor" action (e.g. activating a scripted node's
    /// open-script badge / double-clicking it).
    ///
    /// Unlike [`compose`](Self::compose), this is the *explicit open action*: it
    /// always brings up an editable, syntax-highlighted [`ScriptPane`] for a
    /// scripted node (the caller switches the central view to the script editor
    /// as part of opening). A node with no script (`None`) keeps the empty
    /// prompt.
    pub fn open_selected_node(node_script: Option<SelectedNodeScript>) -> Self {
        match node_script {
            Some(script) => ScriptMainView::Editor(ScriptPane::new(script.path, script.source)),
            None => ScriptMainView::Empty {
                prompt: EMPTY_PROMPT.to_string(),
            },
        }
    }

    /// Whether the view is showing an editable code pane.
    pub fn is_editor(&self) -> bool {
        matches!(self, ScriptMainView::Editor(_))
    }

    /// The code pane, when one is shown.
    pub fn pane(&self) -> Option<&ScriptPane> {
        match self {
            ScriptMainView::Editor(p) => Some(p),
            ScriptMainView::Empty { .. } => None,
        }
    }

    /// Mutable access to the code pane, when one is shown.
    pub fn pane_mut(&mut self) -> Option<&mut ScriptPane> {
        match self {
            ScriptMainView::Editor(p) => Some(p),
            ScriptMainView::Empty { .. } => None,
        }
    }
}

/// Highlights GDScript `source`, falling back to no spans on a lex error so a
/// malformed buffer still renders (as plain, unhighlighted text) rather than
/// breaking the pane.
fn highlight_source(source: &str) -> Vec<HighlightSpan> {
    SyntaxHighlighter::new().highlight(source).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::script_editor::HighlightKind;

    /// Acceptance (pat-yxe4s): choosing a scripted node in SCRIPT mode shows an
    /// editable, syntax-highlighted code pane wired to save. Any other state
    /// (wrong mode, or no scripted node) shows the empty prompt.
    #[test]
    fn script_main_view_editable_highlighted_pane_wired_to_save() {
        let src = "extends Node2D\n\nfunc _ready():\n\tvar x = 1\n\tprint(x)\n";
        let path = "res://player.gd";

        // No scripted node selected → empty prompt, no pane.
        let empty = ScriptMainView::compose(MainView::ScriptEditor, None);
        assert!(!empty.is_editor());
        assert!(empty.pane().is_none());
        match empty {
            ScriptMainView::Empty { prompt } => assert_eq!(prompt, EMPTY_PROMPT),
            _ => panic!("expected empty prompt"),
        }

        // A scripted node but not in SCRIPT mode → still the empty prompt: the
        // code pane only takes over the central area in SCRIPT mode.
        let wrong_mode = ScriptMainView::compose(MainView::Canvas2D, Some((path, src)));
        assert!(!wrong_mode.is_editor(), "code pane only shows in SCRIPT mode");

        // SCRIPT mode + scripted node → an editable, highlighted pane.
        let mut view = ScriptMainView::compose(MainView::ScriptEditor, Some((path, src)));
        assert!(view.is_editor(), "scripted node in SCRIPT mode shows the editor");
        let pane = view.pane().expect("editor pane present");
        assert_eq!(pane.path(), path, "pane edits the node's script");
        assert_eq!(pane.source(), src, "pane shows the node's source");
        assert!(pane.is_editable(), "the code pane is editable, not a preview");
        assert!(!pane.is_dirty(), "a freshly opened script is clean");

        // The pane is syntax-highlighted: GDScript keywords are classified.
        assert!(!pane.highlight().is_empty(), "the pane is highlighted");
        assert!(
            pane.highlight()
                .iter()
                .any(|s| s.kind == HighlightKind::Keyword),
            "GDScript keywords (extends/func/var) are highlighted"
        );

        // Editing the buffer marks it dirty and re-highlights.
        let edited = "extends Node\n\nfunc _ready():\n\tpass\n";
        view.pane_mut().unwrap().edit(edited);
        let pane = view.pane().unwrap();
        assert_eq!(pane.source(), edited, "edits update the buffer");
        assert!(pane.is_dirty(), "an edit marks the pane dirty");
        assert!(
            pane.highlight().iter().any(|s| s.kind == HighlightKind::Keyword),
            "the edited buffer is re-highlighted"
        );

        // Saving returns the (path, source) to persist and clears the dirty
        // flag — this is the pane being "wired to save".
        let (save_path, save_source) = view.pane_mut().unwrap().save();
        assert_eq!(save_path, path, "save targets the node's script path");
        assert_eq!(save_source, edited, "save persists the edited buffer");
        assert!(!view.pane().unwrap().is_dirty(), "saving clears the dirty flag");

        // A no-op edit (same text) doesn't re-dirty a saved buffer.
        view.pane_mut().unwrap().edit(edited);
        assert!(
            !view.pane().unwrap().is_dirty(),
            "an edit that changes nothing leaves the pane clean"
        );
    }

    /// Acceptance (pat-yxe4s.1): opening the selected node's script brings up a
    /// full, editable, syntax-highlighted code pane bound to that script.
    /// Opening with no scripted node selected keeps the empty prompt.
    #[test]
    fn editor_script_open_selected_node_script() {
        let src = "extends Node2D\n\nfunc _ready():\n\tvar speed = 5\n\tprint(speed)\n";
        let path = "res://player.gd";

        // No scripted node selected => nothing to open => the empty prompt.
        let none = ScriptMainView::open_selected_node(None);
        assert!(!none.is_editor());
        assert!(none.pane().is_none());
        match none {
            ScriptMainView::Empty { prompt } => assert_eq!(prompt, EMPTY_PROMPT),
            _ => panic!("expected the empty prompt"),
        }

        // Opening the selected node's script => a full, editable, highlighted
        // code pane bound to that script (regardless of prior mode — this is the
        // explicit open action).
        let view = ScriptMainView::open_selected_node(Some(SelectedNodeScript {
            path: path.to_string(),
            source: src.to_string(),
        }));
        assert!(view.is_editor(), "opening a scripted node shows the code pane");
        let pane = view.pane().expect("code pane present");
        assert_eq!(pane.path(), path, "the pane opens the node's script path");
        assert_eq!(pane.source(), src, "the pane shows the node's script source");
        assert!(pane.is_editable(), "the opened script is editable");
        assert!(!pane.is_dirty(), "a freshly opened script starts clean");
        assert!(
            pane.highlight()
                .iter()
                .any(|s| s.kind == HighlightKind::Keyword),
            "the opened script is syntax-highlighted"
        );
    }

    /// Acceptance (pat-yxe4s.2): the code pane is syntax-highlighted and shows
    /// line numbers in its gutter. Highlighting classifies GDScript tokens on
    /// their correct source lines, and the gutter renders one 1-based number per
    /// line (tracking edits).
    #[test]
    fn editor_script_pane_syntax_highlight() {
        let src = "extends Node2D\n\nfunc _ready():\n\tvar x = 1\n\tprint(x)\n";
        let view = ScriptMainView::open_selected_node(Some(SelectedNodeScript {
            path: "res://demo.gd".to_string(),
            source: src.to_string(),
        }));
        let pane = view.pane().expect("editor pane present");

        // Syntax highlighting: GDScript keywords are classified...
        let spans = pane.highlight();
        assert!(!spans.is_empty(), "the pane is syntax-highlighted");
        assert!(
            spans.iter().any(|s| s.kind == HighlightKind::Keyword),
            "GDScript keywords (extends/func/var) are highlighted"
        );
        // ...on their correct 1-based source lines (multi-line highlighting):
        // a keyword on the first line, and at least one on a later line.
        assert!(
            spans.iter().any(|s| s.kind == HighlightKind::Keyword && s.line == 1),
            "a keyword is highlighted on line 1"
        );
        assert!(
            spans.iter().any(|s| s.kind == HighlightKind::Keyword && s.line > 1),
            "keywords on later lines are highlighted too"
        );
        // No span points past the gutter.
        let max_line = spans.iter().map(|s| s.line).max().unwrap_or(0);
        assert!(
            max_line <= pane.line_count(),
            "highlight spans stay within the gutter ({} <= {})",
            max_line,
            pane.line_count()
        );

        // Line numbers: the gutter shows one number per source line. `src` has
        // 5 newlines => 6 gutter lines (the trailing newline adds an empty last
        // line).
        assert_eq!(pane.line_count(), 6, "gutter line count = newlines + 1");
        assert_eq!(
            pane.line_numbers(),
            vec![1, 2, 3, 4, 5, 6],
            "the gutter renders 1..=line_count"
        );

        // Line numbers track edits: a shorter buffer shrinks the gutter and the
        // pane stays highlighted.
        let mut view = view;
        view.pane_mut()
            .unwrap()
            .edit("extends Node\nfunc _ready():\n\tpass\n");
        let pane = view.pane().unwrap();
        assert_eq!(pane.line_count(), 4, "line count follows edits (3 newlines => 4)");
        assert_eq!(pane.line_numbers(), vec![1, 2, 3, 4]);
        assert!(
            pane.highlight()
                .iter()
                .any(|s| s.kind == HighlightKind::Keyword),
            "the edited buffer is re-highlighted"
        );
    }

    /// Acceptance (pat-yxe4s.3): editing the code pane marks it dirty, and
    /// saving yields the script's `res://` path plus the edited source for the
    /// editor to persist, then clears the dirty flag. Re-editing dirties again.
    #[test]
    fn editor_script_pane_edit_and_save() {
        let path = "res://scripts/player.gd";
        let original = "extends Node2D\nfunc _ready():\n\tpass\n";
        let mut view = ScriptMainView::open_selected_node(Some(SelectedNodeScript {
            path: path.to_string(),
            source: original.to_string(),
        }));

        // A freshly opened script is clean and shows its source.
        {
            let pane = view.pane().expect("code pane present");
            assert!(!pane.is_dirty(), "a freshly opened script is clean");
            assert_eq!(pane.source(), original);
        }

        // Editing the buffer marks it dirty and updates the contents.
        let edited = "extends Node2D\n\nvar speed := 200\n\nfunc _ready():\n\tprint(speed)\n";
        view.pane_mut().unwrap().edit(edited);
        assert!(view.pane().unwrap().is_dirty(), "editing marks the buffer dirty");
        assert_eq!(view.pane().unwrap().source(), edited, "the buffer holds the edits");

        // Saving returns the (res:// path, edited source) for the editor to
        // write back, and clears the dirty flag.
        let (save_path, save_source) = view.pane_mut().unwrap().save();
        assert_eq!(save_path, path, "save targets the script's path");
        assert!(
            save_path.starts_with("res://"),
            "the save target is a res:// resource path: {save_path}"
        );
        assert_eq!(save_source, edited, "save persists the edited buffer contents");
        assert!(!view.pane().unwrap().is_dirty(), "saving clears the dirty flag");

        // Re-editing after a save dirties the buffer again; saving it once more
        // returns the new contents and re-clears the flag.
        let edited2 = "extends Node\n";
        view.pane_mut().unwrap().edit(edited2);
        assert!(view.pane().unwrap().is_dirty(), "a post-save edit dirties again");
        let (_p2, save_source2) = view.pane_mut().unwrap().save();
        assert_eq!(save_source2, edited2, "the second save persists the latest contents");
        assert!(!view.pane().unwrap().is_dirty(), "the second save clears the flag");
    }

    /// Acceptance (pat-yxe4s.4): the code pane offers autocomplete (keywords +
    /// the script's own local variables) and find/replace over the buffer, with
    /// replace re-highlighting and dirtying the pane.
    #[test]
    fn editor_script_pane_autocomplete_find_replace() {
        let source = "extends Node2D\n\nvar speed := 200\nvar spawn_point := Vector2()\n\nfunc _ready():\n\tprint(speed)\n\tspeed += 1\n";
        let mut view = ScriptMainView::open_selected_node(Some(SelectedNodeScript {
            path: "res://scripts/player.gd".to_string(),
            source: source.to_string(),
        }));

        // --- Autocomplete ---------------------------------------------------
        // The script's `extends` drives the completion class.
        assert_eq!(view.pane().unwrap().current_class(), "Node2D");
        // Declared locals are collected for completion.
        let locals = view.pane().unwrap().local_variables();
        assert!(locals.contains(&"speed".to_string()), "locals: {locals:?}");
        assert!(locals.contains(&"spawn_point".to_string()), "locals: {locals:?}");

        // Prefix "sp" surfaces both local variables.
        let sp = view.pane().unwrap().autocomplete("sp");
        assert!(
            sp.iter().any(|c| c.label == "speed"),
            "autocomplete('sp') should suggest the local 'speed': {:?}",
            sp.iter().map(|c| &c.label).collect::<Vec<_>>()
        );
        assert!(
            sp.iter().any(|c| c.label == "spawn_point"),
            "autocomplete('sp') should suggest the local 'spawn_point'"
        );
        // Prefix "fu" surfaces the `func` keyword.
        let fu = view.pane().unwrap().autocomplete("fu");
        assert!(
            fu.iter().any(|c| c.label == "func"),
            "autocomplete('fu') should suggest the 'func' keyword"
        );

        // --- Find -----------------------------------------------------------
        let hits = view.pane().unwrap().find("speed");
        // `var speed`, `print(speed)`, and `speed += 1` => 3 occurrences.
        assert_eq!(hits.len(), 3, "find('speed') hits: {hits:?}");
        assert_eq!(hits[0].line, 3, "first 'speed' is on the var line");
        assert_eq!(hits[0].column, 4, "after 'var '");
        assert_eq!(hits[0].length, 5);
        // An empty query matches nothing.
        assert!(view.pane().unwrap().find("").is_empty());
        // A query that isn't present matches nothing.
        assert!(view.pane().unwrap().find("velocity").is_empty());

        // --- Replace --------------------------------------------------------
        let replaced = view.pane_mut().unwrap().replace_all("speed", "velocity");
        assert_eq!(replaced, 3, "all three 'speed' occurrences are replaced");
        {
            let pane = view.pane().unwrap();
            assert!(
                pane.source().contains("var velocity"),
                "the buffer now holds the replacement"
            );
            assert!(!pane.source().contains("speed"), "no 'speed' remains");
            assert!(pane.is_dirty(), "replacing edits the buffer => dirty");
            assert!(
                !pane.highlight().is_empty(),
                "the replaced buffer is re-highlighted"
            );
            assert!(pane.find("speed").is_empty(), "find no longer matches 'speed'");
            assert_eq!(pane.find("velocity").len(), 3, "find now matches 'velocity'");
        }
        // Replacing a missing query is a no-op.
        let none = view.pane_mut().unwrap().replace_all("speed", "x");
        assert_eq!(none, 0, "replacing an absent query changes nothing");
    }

    /// Acceptance (pat-yxe4s.5): the editor tracks multiple open scripts, lists
    /// them with the active/dirty state, switches between them by index or
    /// path, re-opens without duplicating, and closes them keeping the active
    /// selection in range.
    #[test]
    fn editor_script_open_scripts_switcher() {
        let mut open = OpenScripts::new();
        assert!(open.is_empty());
        assert_eq!(open.len(), 0);
        assert_eq!(open.active_index(), None);
        assert!(open.active().is_none());

        // Open two scripts; the most-recently-opened becomes active.
        open.open(SelectedNodeScript {
            path: "res://scripts/player.gd".into(),
            source: "extends Node2D\n".into(),
        });
        let idx_enemy = open.open(SelectedNodeScript {
            path: "res://scripts/enemy.gd".into(),
            source: "extends Node2D\nvar hp := 5\n".into(),
        });
        assert_eq!(open.len(), 2);
        assert_eq!(open.active_index(), Some(idx_enemy));
        assert_eq!(open.active().unwrap().path(), "res://scripts/enemy.gd");

        // The switcher list shows both, by file name, with the active flag set.
        let list = open.open_list();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].name, "player.gd");
        assert_eq!(list[1].name, "enemy.gd");
        assert!(!list[0].active && list[1].active, "the second script is active");
        assert!(!list[0].dirty && !list[1].dirty, "freshly opened scripts are clean");

        // Switch back to the first script by index.
        assert!(open.switch_to(0));
        assert_eq!(open.active().unwrap().path(), "res://scripts/player.gd");
        assert!(open.open_list()[0].active);

        // Switch by path; failed switches leave the active script unchanged.
        assert!(open.switch_to_path("res://scripts/enemy.gd"));
        assert_eq!(open.active_index(), Some(1));
        assert!(!open.switch_to_path("res://scripts/missing.gd"));
        assert!(!open.switch_to(9));
        assert_eq!(open.active_index(), Some(1), "failed switches don't move active");

        // Editing the active script marks its switcher entry dirty.
        open.active_mut()
            .unwrap()
            .edit("extends Node2D\nvar hp := 10\n");
        assert!(open.open_list()[1].dirty, "the edited script shows dirty");
        assert!(!open.open_list()[0].dirty, "the untouched script stays clean");

        // Re-opening an already-open script switches to it without duplicating
        // and keeps its existing buffer (doesn't clobber with the new source).
        let reopened = open.open(SelectedNodeScript {
            path: "res://scripts/player.gd".into(),
            source: "ignored replacement source".into(),
        });
        assert_eq!(open.len(), 2, "re-opening doesn't add a duplicate tab");
        assert_eq!(reopened, 0);
        assert_eq!(open.active().unwrap().path(), "res://scripts/player.gd");
        assert_eq!(
            open.active().unwrap().source(),
            "extends Node2D\n",
            "re-open keeps the already-open buffer"
        );

        // Closing the active script shifts selection and shrinks the list.
        assert!(open.close(0));
        assert_eq!(open.len(), 1);
        assert_eq!(open.active().unwrap().path(), "res://scripts/enemy.gd");
        assert!(!open.close(5), "closing an out-of-range index is a no-op");

        // Closing the last script empties the set.
        assert!(open.close(0));
        assert!(open.is_empty());
        assert_eq!(open.active_index(), None);
    }
}
