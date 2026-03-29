//! Editor command palette with fuzzy search.
//!
//! Provides a searchable list of editor commands that can be invoked
//! by name, similar to VS Code's Ctrl+Shift+P or Godot's Ctrl+Shift+P.
//! Supports fuzzy matching, categorized commands, recently-used tracking,
//! and keyboard shortcut display.

use std::collections::HashMap;

/// Category for grouping commands in the palette.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CommandCategory {
    Scene,
    Editor,
    View,
    Debug,
    Help,
    Project,
    Script,
    Node,
    Plugin,
}

impl CommandCategory {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Scene => "Scene",
            Self::Editor => "Editor",
            Self::View => "View",
            Self::Debug => "Debug",
            Self::Help => "Help",
            Self::Project => "Project",
            Self::Script => "Script",
            Self::Node => "Node",
            Self::Plugin => "Plugin",
        }
    }

    pub fn all() -> &'static [CommandCategory] {
        &[
            Self::Scene,
            Self::Editor,
            Self::View,
            Self::Debug,
            Self::Help,
            Self::Project,
            Self::Script,
            Self::Node,
            Self::Plugin,
        ]
    }
}

/// A command that can be invoked via the palette.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Command {
    /// Unique identifier (e.g. "scene.save", "editor.settings").
    pub id: String,
    /// Human-readable label shown in the palette.
    pub label: String,
    /// Category for grouping.
    pub category: CommandCategory,
    /// Optional keyboard shortcut display string.
    pub shortcut: Option<String>,
    /// Optional description / tooltip.
    pub description: String,
}

impl Command {
    pub fn new(id: &str, label: &str, category: CommandCategory) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            category,
            shortcut: None,
            description: String::new(),
        }
    }

    pub fn with_shortcut(mut self, shortcut: &str) -> Self {
        self.shortcut = Some(shortcut.into());
        self
    }

    pub fn with_description(mut self, desc: &str) -> Self {
        self.description = desc.into();
        self
    }
}

/// A search result entry with match score.
#[derive(Debug, Clone)]
pub struct PaletteResult {
    /// Index into the command registry.
    pub command_index: usize,
    /// Fuzzy match score (higher = better match).
    pub score: i32,
    /// Character positions in the label that matched the query.
    pub matched_indices: Vec<usize>,
}

/// Computes a fuzzy match score for `query` against `text`.
/// Returns `None` if the query doesn't match.
/// Returns `Some((score, matched_indices))` on match.
///
/// Scoring:
/// - Consecutive matches: bonus
/// - Match at start of word: bonus
/// - Match at start of string: bonus
/// - Shorter text preferred (less noise)
pub fn fuzzy_match(query: &str, text: &str) -> Option<(i32, Vec<usize>)> {
    if query.is_empty() {
        return Some((0, Vec::new()));
    }

    let query_lower: Vec<char> = query.to_lowercase().chars().collect();
    let text_lower: Vec<char> = text.to_lowercase().chars().collect();
    let text_chars: Vec<char> = text.chars().collect();

    // Check if all query chars exist in text in order
    let mut qi = 0;
    let mut indices = Vec::new();
    for (ti, &tc) in text_lower.iter().enumerate() {
        if qi < query_lower.len() && tc == query_lower[qi] {
            indices.push(ti);
            qi += 1;
        }
    }
    if qi != query_lower.len() {
        return None;
    }

    // Score the match
    let mut score: i32 = 0;

    // Base score for matching
    score += (query_lower.len() as i32) * 10;

    // Bonus for consecutive matches
    for w in indices.windows(2) {
        if w[1] == w[0] + 1 {
            score += 5;
        }
    }

    // Bonus for match at start of string
    if !indices.is_empty() && indices[0] == 0 {
        score += 15;
    }

    // Bonus for match at start of words (after space, _, .)
    for &idx in &indices {
        if idx > 0 {
            let prev = text_chars[idx - 1];
            if prev == ' ' || prev == '_' || prev == '.' || prev == ':' {
                score += 8;
            }
            // CamelCase boundary
            if text_chars[idx].is_uppercase() && text_chars[idx - 1].is_lowercase() {
                score += 6;
            }
        }
    }

    // Exact prefix match bonus
    if text_lower.len() >= query_lower.len()
        && text_lower[..query_lower.len()] == query_lower[..]
    {
        score += 20;
    }

    // Prefer shorter text (less noise)
    score -= (text_lower.len() as i32) / 4;

    Some((score, indices))
}

/// The command palette state.
#[derive(Debug)]
pub struct CommandPalette {
    commands: Vec<Command>,
    /// Command ID -> index for fast lookup.
    id_index: HashMap<String, usize>,
    visible: bool,
    query: String,
    results: Vec<PaletteResult>,
    selected_index: usize,
    /// Recently used command IDs (most recent first).
    recent: Vec<String>,
    /// Category filter (None = all).
    category_filter: Option<CommandCategory>,
    /// Max recent items to track.
    max_recent: usize,
}

impl CommandPalette {
    /// Creates an empty command palette.
    pub fn new() -> Self {
        Self {
            commands: Vec::new(),
            id_index: HashMap::new(),
            visible: false,
            query: String::new(),
            results: Vec::new(),
            selected_index: 0,
            recent: Vec::new(),
            category_filter: None,
            max_recent: 20,
        }
    }

    /// Creates a palette pre-loaded with default editor commands.
    pub fn with_defaults() -> Self {
        let mut palette = Self::new();
        palette.register_defaults();
        palette
    }

    /// Registers a command. Replaces any existing command with the same ID.
    pub fn register(&mut self, cmd: Command) {
        if let Some(&idx) = self.id_index.get(&cmd.id) {
            self.commands[idx] = cmd;
        } else {
            let idx = self.commands.len();
            self.id_index.insert(cmd.id.clone(), idx);
            self.commands.push(cmd);
        }
    }

    /// Unregisters a command by ID.
    pub fn unregister(&mut self, id: &str) -> bool {
        if let Some(&idx) = self.id_index.get(id) {
            self.id_index.remove(id);
            // Mark as removed by clearing the ID (avoid shifting indices)
            self.commands[idx].id.clear();
            self.recent.retain(|r| r != id);
            true
        } else {
            false
        }
    }

    /// Returns the total number of registered commands.
    pub fn command_count(&self) -> usize {
        self.id_index.len()
    }

    /// Looks up a command by ID.
    pub fn find_command(&self, id: &str) -> Option<&Command> {
        self.id_index.get(id).map(|&idx| &self.commands[idx])
    }

    /// Opens the palette, clearing the query.
    pub fn open(&mut self) {
        self.visible = true;
        self.query.clear();
        self.selected_index = 0;
        self.category_filter = None;
        self.update_results();
    }

    /// Opens the palette with a category filter.
    pub fn open_with_category(&mut self, category: CommandCategory) {
        self.visible = true;
        self.query.clear();
        self.selected_index = 0;
        self.category_filter = Some(category);
        self.update_results();
    }

    /// Closes the palette.
    pub fn close(&mut self) {
        self.visible = false;
        self.query.clear();
        self.results.clear();
        self.category_filter = None;
    }

    /// Whether the palette is visible.
    pub fn is_visible(&self) -> bool {
        self.visible
    }

    /// Sets the search query and updates results.
    pub fn set_query(&mut self, query: &str) {
        self.query = query.to_string();
        self.selected_index = 0;
        self.update_results();
    }

    /// Returns the current query.
    pub fn query(&self) -> &str {
        &self.query
    }

    /// Returns the current results.
    pub fn results(&self) -> &[PaletteResult] {
        &self.results
    }

    /// Returns the command for a result.
    pub fn command_for_result(&self, result: &PaletteResult) -> &Command {
        &self.commands[result.command_index]
    }

    /// Returns the selected result index.
    pub fn selected_index(&self) -> usize {
        self.selected_index
    }

    /// Moves selection up.
    pub fn select_prev(&mut self) {
        if self.selected_index > 0 {
            self.selected_index -= 1;
        }
    }

    /// Moves selection down.
    pub fn select_next(&mut self) {
        if !self.results.is_empty() && self.selected_index < self.results.len() - 1 {
            self.selected_index += 1;
        }
    }

    /// Executes the currently selected command. Returns the command ID if available.
    pub fn execute_selected(&mut self) -> Option<String> {
        if self.results.is_empty() {
            return None;
        }
        let idx = self.selected_index.min(self.results.len() - 1);
        let cmd_id = self.commands[self.results[idx].command_index].id.clone();
        self.record_recent(&cmd_id);
        self.close();
        Some(cmd_id)
    }

    /// Executes a command by ID. Returns true if found.
    pub fn execute_by_id(&mut self, id: &str) -> bool {
        if self.id_index.contains_key(id) {
            self.record_recent(id);
            self.close();
            true
        } else {
            false
        }
    }

    /// Returns recently used command IDs.
    pub fn recent(&self) -> &[String] {
        &self.recent
    }

    /// Returns commands in a category.
    pub fn commands_in_category(&self, category: CommandCategory) -> Vec<&Command> {
        self.commands
            .iter()
            .filter(|c| !c.id.is_empty() && c.category == category)
            .collect()
    }

    /// Returns the active category filter.
    pub fn category_filter(&self) -> Option<CommandCategory> {
        self.category_filter
    }

    fn record_recent(&mut self, id: &str) {
        self.recent.retain(|r| r != id);
        self.recent.insert(0, id.to_string());
        self.recent.truncate(self.max_recent);
    }

    fn update_results(&mut self) {
        self.results.clear();

        for (idx, cmd) in self.commands.iter().enumerate() {
            if cmd.id.is_empty() {
                continue;
            }
            if let Some(cat) = self.category_filter {
                if cmd.category != cat {
                    continue;
                }
            }

            if self.query.is_empty() {
                // No query: show all, boost recent
                let recent_boost = self
                    .recent
                    .iter()
                    .position(|r| r == &cmd.id)
                    .map(|pos| 100 - pos as i32)
                    .unwrap_or(0);
                self.results.push(PaletteResult {
                    command_index: idx,
                    score: recent_boost,
                    matched_indices: Vec::new(),
                });
            } else {
                // Fuzzy match against label
                let label_match = fuzzy_match(&self.query, &cmd.label);
                let id_match = fuzzy_match(&self.query, &cmd.id);

                // Take the better score
                let best = match (label_match, id_match) {
                    (Some((ls, li)), Some((is, _))) => {
                        if ls >= is { Some((ls, li)) } else { Some((is, Vec::new())) }
                    }
                    (Some(m), None) | (None, Some(m)) => Some(m),
                    (None, None) => None,
                };

                if let Some((score, indices)) = best {
                    // Boost recently used
                    let recent_boost = self
                        .recent
                        .iter()
                        .position(|r| r == &cmd.id)
                        .map(|pos| 50 - pos as i32)
                        .unwrap_or(0);
                    self.results.push(PaletteResult {
                        command_index: idx,
                        score: score + recent_boost,
                        matched_indices: indices,
                    });
                }
            }
        }

        // Sort by score descending, then label alphabetically
        self.results.sort_by(|a, b| {
            b.score.cmp(&a.score).then_with(|| {
                self.commands[a.command_index]
                    .label
                    .cmp(&self.commands[b.command_index].label)
            })
        });
    }

    fn register_defaults(&mut self) {
        use CommandCategory::*;

        let cmds = vec![
            // Scene
            Command::new("scene.new", "New Scene", Scene).with_shortcut("Ctrl+N"),
            Command::new("scene.open", "Open Scene", Scene).with_shortcut("Ctrl+O"),
            Command::new("scene.save", "Save Scene", Scene).with_shortcut("Ctrl+S"),
            Command::new("scene.save_as", "Save Scene As", Scene).with_shortcut("Ctrl+Shift+S"),
            Command::new("scene.save_all", "Save All Scenes", Scene),
            Command::new("scene.close", "Close Scene", Scene).with_shortcut("Ctrl+W"),
            Command::new("scene.run", "Run Scene", Scene).with_shortcut("F5"),
            Command::new("scene.run_current", "Run Current Scene", Scene).with_shortcut("F6"),
            Command::new("scene.stop", "Stop Running Scene", Scene).with_shortcut("F8"),
            // Editor
            Command::new("editor.settings", "Editor Settings", Editor),
            Command::new("editor.toggle_fullscreen", "Toggle Fullscreen", Editor)
                .with_shortcut("F11"),
            Command::new("editor.toggle_distraction_free", "Toggle Distraction Free Mode", Editor)
                .with_shortcut("Ctrl+Shift+F11"),
            Command::new("editor.command_palette", "Command Palette", Editor)
                .with_shortcut("Ctrl+Shift+P"),
            // View
            Command::new("view.zoom_in", "Zoom In", View).with_shortcut("Ctrl++"),
            Command::new("view.zoom_out", "Zoom Out", View).with_shortcut("Ctrl+-"),
            Command::new("view.zoom_reset", "Reset Zoom", View).with_shortcut("Ctrl+0"),
            Command::new("view.toggle_output", "Toggle Output Panel", View),
            Command::new("view.toggle_filesystem", "Toggle FileSystem Dock", View),
            Command::new("view.toggle_inspector", "Toggle Inspector", View),
            Command::new("view.toggle_scene_tree", "Toggle Scene Tree", View),
            // Node
            Command::new("node.add", "Add Node", Node).with_shortcut("Ctrl+A"),
            Command::new("node.delete", "Delete Node", Node).with_shortcut("Delete"),
            Command::new("node.duplicate", "Duplicate Node", Node).with_shortcut("Ctrl+D"),
            Command::new("node.rename", "Rename Node", Node).with_shortcut("F2"),
            Command::new("node.copy", "Copy Node", Node).with_shortcut("Ctrl+C"),
            Command::new("node.paste", "Paste Node", Node).with_shortcut("Ctrl+V"),
            Command::new("node.cut", "Cut Node", Node).with_shortcut("Ctrl+X"),
            // Script
            Command::new("script.new", "New Script", Script),
            Command::new("script.open", "Open Script", Script),
            Command::new("script.find", "Find in Script", Script).with_shortcut("Ctrl+F"),
            Command::new("script.replace", "Find and Replace", Script).with_shortcut("Ctrl+H"),
            Command::new("script.goto_line", "Go to Line", Script).with_shortcut("Ctrl+G"),
            // Debug
            Command::new("debug.run_file", "Run File", Debug),
            Command::new("debug.toggle_breakpoint", "Toggle Breakpoint", Debug)
                .with_shortcut("F9"),
            Command::new("debug.step_over", "Step Over", Debug).with_shortcut("F10"),
            Command::new("debug.step_into", "Step Into", Debug).with_shortcut("F11"),
            // Project
            Command::new("project.settings", "Project Settings", Project),
            Command::new("project.export", "Export Project", Project),
            Command::new("project.reload", "Reload Current Project", Project),
            // Help
            Command::new("help.docs", "Online Documentation", Help).with_shortcut("F1"),
            Command::new("help.about", "About Patina Engine", Help),
        ];

        for cmd in cmds {
            self.register(cmd);
        }
    }
}

impl Default for CommandPalette {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- fuzzy_match tests ----

    #[test]
    fn fuzzy_match_exact() {
        let (score, indices) = fuzzy_match("save", "Save Scene").unwrap();
        assert!(score > 0);
        assert_eq!(indices, vec![0, 1, 2, 3]);
    }

    #[test]
    fn fuzzy_match_abbreviation() {
        let result = fuzzy_match("ss", "Save Scene");
        assert!(result.is_some());
        let (_, indices) = result.unwrap();
        assert_eq!(indices.len(), 2);
    }

    #[test]
    fn fuzzy_match_no_match() {
        assert!(fuzzy_match("xyz", "Save Scene").is_none());
    }

    #[test]
    fn fuzzy_match_empty_query() {
        let (score, indices) = fuzzy_match("", "anything").unwrap();
        assert_eq!(score, 0);
        assert!(indices.is_empty());
    }

    #[test]
    fn fuzzy_match_case_insensitive() {
        let result = fuzzy_match("SAVE", "save scene");
        assert!(result.is_some());
    }

    #[test]
    fn fuzzy_match_word_boundary_bonus() {
        // "ss" matching "Save Scene" should score higher than "Sassertion"
        // because the second 's' hits a word boundary in "Save Scene"
        let (score_boundary, _) = fuzzy_match("ss", "Save Scene").unwrap();
        let (score_no_boundary, _) = fuzzy_match("ss", "Sassertion").unwrap();
        assert!(score_boundary > score_no_boundary);
    }

    #[test]
    fn fuzzy_match_prefix_bonus() {
        // "sav" matching "Save" should score higher than "Unsaved"
        let (score_prefix, _) = fuzzy_match("sav", "Save").unwrap();
        let (score_mid, _) = fuzzy_match("sav", "Unsaved").unwrap();
        assert!(score_prefix > score_mid);
    }

    #[test]
    fn fuzzy_match_consecutive_bonus() {
        // "save" in "Save Scene" (4 consecutive) vs "save" in "S_a_v_e" (0 consecutive)
        let (score_consec, _) = fuzzy_match("save", "Save Scene").unwrap();
        let (score_spread, _) = fuzzy_match("save", "S_a_v_e_x").unwrap();
        assert!(score_consec > score_spread);
    }

    // ---- CommandPalette basic tests ----

    #[test]
    fn new_palette_is_empty() {
        let p = CommandPalette::new();
        assert_eq!(p.command_count(), 0);
        assert!(!p.is_visible());
    }

    #[test]
    fn with_defaults_has_commands() {
        let p = CommandPalette::with_defaults();
        assert!(p.command_count() >= 30);
    }

    #[test]
    fn register_and_find() {
        let mut p = CommandPalette::new();
        p.register(Command::new("test.cmd", "Test Command", CommandCategory::Editor));
        assert_eq!(p.command_count(), 1);
        let cmd = p.find_command("test.cmd").unwrap();
        assert_eq!(cmd.label, "Test Command");
    }

    #[test]
    fn register_replaces_existing() {
        let mut p = CommandPalette::new();
        p.register(Command::new("test.cmd", "Old Label", CommandCategory::Editor));
        p.register(Command::new("test.cmd", "New Label", CommandCategory::Editor));
        assert_eq!(p.command_count(), 1);
        assert_eq!(p.find_command("test.cmd").unwrap().label, "New Label");
    }

    #[test]
    fn unregister() {
        let mut p = CommandPalette::new();
        p.register(Command::new("test.cmd", "Test", CommandCategory::Editor));
        assert!(p.unregister("test.cmd"));
        assert_eq!(p.command_count(), 0);
        assert!(p.find_command("test.cmd").is_none());
        assert!(!p.unregister("test.cmd"));
    }

    // ---- Open/close/query ----

    #[test]
    fn open_close() {
        let mut p = CommandPalette::with_defaults();
        p.open();
        assert!(p.is_visible());
        assert!(p.query().is_empty());
        p.close();
        assert!(!p.is_visible());
    }

    #[test]
    fn open_shows_all_results() {
        let mut p = CommandPalette::with_defaults();
        let total = p.command_count();
        p.open();
        assert_eq!(p.results().len(), total);
    }

    #[test]
    fn query_filters_results() {
        let mut p = CommandPalette::with_defaults();
        p.open();
        p.set_query("save");
        assert!(p.results().len() > 0);
        assert!(p.results().len() < p.command_count());
        for r in p.results() {
            let cmd = p.command_for_result(r);
            let label_lower = cmd.label.to_lowercase();
            let id_lower = cmd.id.to_lowercase();
            assert!(
                label_lower.contains("sav") || id_lower.contains("sav"),
                "Expected match for 'save' in '{}' / '{}'",
                cmd.label,
                cmd.id
            );
        }
    }

    #[test]
    fn query_no_results() {
        let mut p = CommandPalette::with_defaults();
        p.open();
        p.set_query("xyznonexistent");
        assert!(p.results().is_empty());
    }

    #[test]
    fn results_sorted_by_score_desc() {
        let mut p = CommandPalette::with_defaults();
        p.open();
        p.set_query("scene");
        let results = p.results();
        for w in results.windows(2) {
            assert!(w[0].score >= w[1].score);
        }
    }

    // ---- Selection navigation ----

    #[test]
    fn select_next_prev() {
        let mut p = CommandPalette::with_defaults();
        p.open();
        assert_eq!(p.selected_index(), 0);
        p.select_next();
        assert_eq!(p.selected_index(), 1);
        p.select_next();
        assert_eq!(p.selected_index(), 2);
        p.select_prev();
        assert_eq!(p.selected_index(), 1);
    }

    #[test]
    fn select_prev_at_zero_stays() {
        let mut p = CommandPalette::with_defaults();
        p.open();
        p.select_prev();
        assert_eq!(p.selected_index(), 0);
    }

    #[test]
    fn select_next_at_end_stays() {
        let mut p = CommandPalette::with_defaults();
        p.open();
        let count = p.results().len();
        for _ in 0..count + 5 {
            p.select_next();
        }
        assert_eq!(p.selected_index(), count - 1);
    }

    // ---- Execute ----

    #[test]
    fn execute_selected() {
        let mut p = CommandPalette::with_defaults();
        p.open();
        p.set_query("save scene");
        let cmd_id = p.execute_selected();
        assert!(cmd_id.is_some());
        assert!(!p.is_visible()); // closes after execute
    }

    #[test]
    fn execute_selected_empty_results() {
        let mut p = CommandPalette::with_defaults();
        p.open();
        p.set_query("xyznonexistent");
        assert!(p.execute_selected().is_none());
    }

    #[test]
    fn execute_by_id() {
        let mut p = CommandPalette::with_defaults();
        p.open();
        assert!(p.execute_by_id("scene.save"));
        assert!(!p.is_visible());
        assert!(!p.execute_by_id("nonexistent.cmd"));
    }

    // ---- Recent tracking ----

    #[test]
    fn recent_tracking() {
        let mut p = CommandPalette::with_defaults();
        p.open();
        p.execute_by_id("scene.save");

        p.open();
        p.execute_by_id("scene.open");

        assert_eq!(p.recent()[0], "scene.open");
        assert_eq!(p.recent()[1], "scene.save");
    }

    #[test]
    fn recent_deduplication() {
        let mut p = CommandPalette::with_defaults();
        p.open();
        p.execute_by_id("scene.save");
        p.open();
        p.execute_by_id("scene.open");
        p.open();
        p.execute_by_id("scene.save"); // already in recent

        assert_eq!(p.recent().len(), 2);
        assert_eq!(p.recent()[0], "scene.save");
    }

    #[test]
    fn recent_boosts_results() {
        let mut p = CommandPalette::with_defaults();
        // Execute "help.about" to make it recent
        p.open();
        p.execute_by_id("help.about");

        // Open with empty query — "help.about" should be boosted near top
        p.open();
        let top_5: Vec<&str> = p
            .results()
            .iter()
            .take(5)
            .map(|r| p.command_for_result(r).id.as_str())
            .collect();
        assert!(top_5.contains(&"help.about"));
    }

    #[test]
    fn recent_capped() {
        let mut p = CommandPalette::with_defaults();
        // Execute more than max_recent commands
        for i in 0..25 {
            p.register(Command::new(
                &format!("test.cmd{i}"),
                &format!("Test {i}"),
                CommandCategory::Editor,
            ));
        }
        for i in 0..25 {
            p.open();
            p.execute_by_id(&format!("test.cmd{i}"));
        }
        assert!(p.recent().len() <= 20);
    }

    // ---- Category filter ----

    #[test]
    fn category_filter() {
        let mut p = CommandPalette::with_defaults();
        p.open_with_category(CommandCategory::Scene);
        assert_eq!(p.category_filter(), Some(CommandCategory::Scene));
        for r in p.results() {
            assert_eq!(p.command_for_result(r).category, CommandCategory::Scene);
        }
        assert!(!p.results().is_empty());
    }

    #[test]
    fn category_filter_with_query() {
        let mut p = CommandPalette::with_defaults();
        p.open_with_category(CommandCategory::Scene);
        p.set_query("save");
        for r in p.results() {
            let cmd = p.command_for_result(r);
            assert_eq!(cmd.category, CommandCategory::Scene);
        }
    }

    #[test]
    fn commands_in_category() {
        let p = CommandPalette::with_defaults();
        let scene_cmds = p.commands_in_category(CommandCategory::Scene);
        assert!(scene_cmds.len() >= 5);
        for cmd in &scene_cmds {
            assert_eq!(cmd.category, CommandCategory::Scene);
        }
    }

    #[test]
    fn close_clears_category_filter() {
        let mut p = CommandPalette::with_defaults();
        p.open_with_category(CommandCategory::Scene);
        p.close();
        assert_eq!(p.category_filter(), None);
    }

    // ---- Command builder ----

    #[test]
    fn command_with_shortcut_and_description() {
        let cmd = Command::new("test", "Test", CommandCategory::Editor)
            .with_shortcut("Ctrl+T")
            .with_description("A test command");
        assert_eq!(cmd.shortcut, Some("Ctrl+T".to_string()));
        assert_eq!(cmd.description, "A test command");
    }

    #[test]
    fn command_category_labels() {
        for cat in CommandCategory::all() {
            assert!(!cat.label().is_empty());
        }
    }

    // ---- Unregister cleans up recent ----

    #[test]
    fn unregister_cleans_recent() {
        let mut p = CommandPalette::new();
        p.register(Command::new("a", "A", CommandCategory::Editor));
        p.register(Command::new("b", "B", CommandCategory::Editor));
        p.open();
        p.execute_by_id("a");
        p.open();
        p.execute_by_id("b");
        assert_eq!(p.recent().len(), 2);
        p.unregister("a");
        assert_eq!(p.recent().len(), 1);
        assert_eq!(p.recent()[0], "b");
    }

    // ---- Query reset on open ----

    #[test]
    fn open_resets_query() {
        let mut p = CommandPalette::with_defaults();
        p.open();
        p.set_query("something");
        p.close();
        p.open();
        assert!(p.query().is_empty());
        assert_eq!(p.selected_index(), 0);
    }
}
