//! **Autoload / singleton management** (pat-8ptny).
//!
//! Manages the project's autoloads (global singletons): add, rename, reorder,
//! enable/disable, and remove them. The ordered list is the load order, enabled
//! autoloads are registered as global singletons, and the whole list persists to
//! the `[autoload]` section of project settings (modeled as a serialized
//! round-trip).

use serde::{Deserialize, Serialize};

/// A single autoload entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Autoload {
    /// The global singleton name.
    pub name: String,
    /// The script/scene path it loads.
    pub path: String,
    /// Whether it's enabled (registered as a global).
    pub enabled: bool,
}

/// Manages the ordered list of autoloads.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AutoloadManager {
    autoloads: Vec<Autoload>,
}

impl AutoloadManager {
    /// Creates an empty manager.
    pub fn new() -> Self {
        Self::default()
    }

    /// The autoload names in load order.
    pub fn names(&self) -> Vec<&str> {
        self.autoloads.iter().map(|a| a.name.as_str()).collect()
    }

    /// The number of autoloads.
    pub fn len(&self) -> usize {
        self.autoloads.len()
    }

    /// Whether there are no autoloads.
    pub fn is_empty(&self) -> bool {
        self.autoloads.is_empty()
    }

    /// Whether an autoload named `name` exists.
    pub fn contains(&self, name: &str) -> bool {
        self.autoloads.iter().any(|a| a.name == name)
    }

    /// The autoload named `name`, if present.
    pub fn get(&self, name: &str) -> Option<&Autoload> {
        self.autoloads.iter().find(|a| a.name == name)
    }

    fn index_of(&self, name: &str) -> Option<usize> {
        self.autoloads.iter().position(|a| a.name == name)
    }

    /// Adds an enabled autoload, registering a global singleton. Returns false
    /// if the name is already taken.
    pub fn add(&mut self, name: impl Into<String>, path: impl Into<String>) -> bool {
        let name = name.into();
        if self.contains(&name) {
            return false;
        }
        self.autoloads.push(Autoload {
            name,
            path: path.into(),
            enabled: true,
        });
        true
    }

    /// Removes the autoload `name`, clearing its global. Returns whether it
    /// existed.
    pub fn remove(&mut self, name: &str) -> bool {
        match self.index_of(name) {
            Some(i) => {
                self.autoloads.remove(i);
                true
            }
            None => false,
        }
    }

    /// Renames `old` to `new`, preserving its position. Returns false if `old`
    /// is missing or `new` is taken.
    pub fn rename(&mut self, old: &str, new: &str) -> bool {
        if !self.contains(old) || self.contains(new) {
            return false;
        }
        if let Some(a) = self.autoloads.iter_mut().find(|a| a.name == old) {
            a.name = new.to_string();
        }
        true
    }

    /// Enables or disables `name`. Returns whether it exists.
    pub fn set_enabled(&mut self, name: &str, enabled: bool) -> bool {
        match self.autoloads.iter_mut().find(|a| a.name == name) {
            Some(a) => {
                a.enabled = enabled;
                true
            }
            None => false,
        }
    }

    /// Moves `name` one position earlier in the load order. Returns whether it
    /// moved.
    pub fn move_up(&mut self, name: &str) -> bool {
        match self.index_of(name) {
            Some(i) if i > 0 => {
                self.autoloads.swap(i, i - 1);
                true
            }
            _ => false,
        }
    }

    /// Moves `name` one position later in the load order. Returns whether it
    /// moved.
    pub fn move_down(&mut self, name: &str) -> bool {
        match self.index_of(name) {
            Some(i) if i + 1 < self.autoloads.len() => {
                self.autoloads.swap(i, i + 1);
                true
            }
            _ => false,
        }
    }

    /// Whether `name` is currently registered as a global (exists and enabled).
    pub fn is_global(&self, name: &str) -> bool {
        self.get(name).is_some_and(|a| a.enabled)
    }

    /// The registered global singletons, in load order (enabled autoloads).
    pub fn globals(&self) -> Vec<&str> {
        self.autoloads
            .iter()
            .filter(|a| a.enabled)
            .map(|a| a.name.as_str())
            .collect()
    }

    /// Serializes this manager's state to JSON.
    ///
    /// **This is not project.godot text** despite the historical name — it is a
    /// JSON snapshot, useful for round-tripping editor state. Writing it into
    /// `project.godot` would produce a JSON blob where an INI section belongs.
    /// Use [`to_project_godot_section`](Self::to_project_godot_section) to
    /// persist to the real file. (pat-nzgdu.97)
    pub fn to_cfg(&self) -> String {
        serde_json::to_string_pretty(self).expect("AutoloadManager serializes")
    }

    /// Loads manager state from the JSON produced by [`to_cfg`](Self::to_cfg).
    ///
    /// See `to_cfg` — this parses JSON, not project.godot text. Use
    /// [`from_project_godot_section`](Self::from_project_godot_section) for the
    /// real file.
    pub fn from_cfg(cfg: &str) -> serde_json::Result<Self> {
        serde_json::from_str(cfg)
    }

    /// Renders the real `project.godot` `[autoload]` section.
    ///
    /// Godot writes one `Name="path"` line per autoload, and marks an **enabled**
    /// entry with a `*` prefix on the path (`Global="*res://global.gd"`); the
    /// editor's "Enable" checkbox is exactly that prefix. Lines are emitted in
    /// **declaration order**, not sorted: the order is the load order, so an
    /// autoload may reference one declared before it, and reordering is a
    /// user-visible operation ([`move_up`](Self::move_up)/[`move_down`](Self::move_down)).
    ///
    /// Round-trips with [`from_project_godot_section`](Self::from_project_godot_section).
    pub fn to_project_godot_section(&self) -> String {
        let mut out = String::from("[autoload]\n\n");
        for a in &self.autoloads {
            let marker = if a.enabled { "*" } else { "" };
            out.push_str(&format!("{}=\"{}{}\"\n", a.name, marker, a.path));
        }
        out
    }

    /// Splices this manager's `[autoload]` section into a whole `project.godot`,
    /// leaving every other section untouched (pat-0q2kx.73).
    ///
    /// This is the piece that lets the editor persist an edit: the round-trip
    /// helpers above only produce/consume the section itself, and rewriting the
    /// file by hand at each call site is how a settings writer starts dropping
    /// other people's keys.
    ///
    /// An existing `[autoload]` section is replaced in place, preserving its
    /// position in the file; otherwise the section is appended. An empty
    /// manager removes the section entirely rather than leaving a stub, which is
    /// what Godot's own writer does.
    pub fn splice_into_project_godot(&self, project_godot: &str) -> String {
        let mut out = String::new();
        let mut in_autoload = false;
        let mut wrote_section = false;
        let section = self.to_project_godot_section();

        for line in project_godot.lines() {
            let trimmed = line.trim();
            let is_header = trimmed.starts_with('[') && trimmed.ends_with(']');
            if is_header {
                if trimmed == "[autoload]" {
                    // Drop the old section body; emit the new one in its place.
                    in_autoload = true;
                    if !self.is_empty() {
                        out.push_str(&section);
                        out.push('\n');
                    }
                    wrote_section = true;
                    continue;
                }
                in_autoload = false;
            }
            if in_autoload {
                continue;
            }
            out.push_str(line);
            out.push('\n');
        }

        if !wrote_section && !self.is_empty() {
            if !out.is_empty() && !out.ends_with("\n\n") {
                out.push('\n');
            }
            out.push_str(&section);
        }
        out
    }

    /// Parses the `[autoload]` section out of `project.godot` text.
    ///
    /// Reads only that section (other sections are skipped), preserves the file's
    /// declaration order, and treats a `*` path prefix as "enabled" — matching
    /// `gdresource::ProjectLoader::get_autoloads`' `is_singleton` semantics.
    /// Text with no `[autoload]` section yields an empty manager.
    pub fn from_project_godot_section(text: &str) -> Self {
        let mut mgr = Self::new();
        let mut in_section = false;
        for line in text.lines() {
            let line = line.trim();
            if line.starts_with('[') && line.ends_with(']') {
                in_section = line == "[autoload]";
                continue;
            }
            if !in_section || line.is_empty() || line.starts_with(';') || line.starts_with('#') {
                continue;
            }
            let Some((name, raw)) = line.split_once('=') else {
                continue;
            };
            let value = raw.trim().trim_matches('"');
            let (enabled, path) = match value.strip_prefix('*') {
                Some(rest) => (true, rest.to_string()),
                None => (false, value.to_string()),
            };
            mgr.autoloads.push(Autoload {
                name: name.trim().to_string(),
                path,
                enabled,
            });
        }
        mgr
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Acceptance (pat-8ptny): adding an autoload registers a global singleton,
    /// reordering changes load order, and removal clears it.
    #[test]
    fn systems_autoloads_register_globals() {
        let mut mgr = AutoloadManager::new();

        // Adding registers global singletons in load order.
        assert!(mgr.add("GameState", "res://game_state.gd"));
        assert!(mgr.add("Audio", "res://audio.gd"));
        assert!(!mgr.add("GameState", "res://dup.gd")); // duplicate name
        assert_eq!(mgr.names(), vec!["GameState", "Audio"]);
        assert!(mgr.is_global("GameState"));
        assert_eq!(mgr.globals(), vec!["GameState", "Audio"]);

        // Disabling removes it from the globals but keeps the entry.
        assert!(mgr.set_enabled("Audio", false));
        assert_eq!(mgr.globals(), vec!["GameState"]);
        assert!(mgr.contains("Audio"));
        assert!(!mgr.is_global("Audio"));

        // Reordering changes the load order.
        assert!(mgr.move_down("GameState"));
        assert_eq!(mgr.names(), vec!["Audio", "GameState"]);
        assert!(mgr.move_up("GameState"));
        assert_eq!(mgr.names(), vec!["GameState", "Audio"]);
        // Can't move past the ends.
        assert!(!mgr.move_up("GameState"));
        assert!(!mgr.move_down("Audio"));

        // Rename preserves position and path.
        assert!(mgr.rename("Audio", "Sound"));
        assert!(mgr.contains("Sound"));
        assert!(!mgr.contains("Audio"));
        assert_eq!(mgr.get("Sound").unwrap().path, "res://audio.gd");
        assert!(!mgr.rename("Sound", "GameState")); // target taken

        // Removal clears the autoload and its global.
        assert!(mgr.remove("GameState"));
        assert!(!mgr.contains("GameState"));
        assert!(!mgr.is_global("GameState"));
        assert_eq!(mgr.names(), vec!["Sound"]);

        // The JSON state snapshot persists and reloads identically.
        let cfg = mgr.to_cfg();
        let loaded = AutoloadManager::from_cfg(&cfg).expect("loads");
        assert_eq!(loaded.names(), vec!["Sound"]);
        assert_eq!(loaded.get("Sound").unwrap().path, "res://audio.gd");
        assert!(!loaded.get("Sound").unwrap().enabled);
    }

    /// pat-nzgdu.97: the manager renders the **real** `project.godot`
    /// `[autoload]` section — `Name="*path"` lines, `*` marking enabled.
    ///
    /// `to_cfg` emits JSON, so it cannot be what gets written to the file; this
    /// is the persistence the editor's Autoload tab needs.
    #[test]
    fn renders_real_project_godot_autoload_section() {
        let mut mgr = AutoloadManager::new();
        mgr.add("Global", "res://global.gd");
        mgr.add("Music", "res://music.gd");
        mgr.set_enabled("Music", false);

        let section = mgr.to_project_godot_section();

        assert!(section.starts_with("[autoload]"), "got: {section}");
        // Enabled -> `*` prefix (Godot's "Enable" checkbox is exactly this).
        assert!(
            section.contains("Global=\"*res://global.gd\""),
            "enabled autoload needs the * singleton marker; got: {section}"
        );
        // Disabled -> same line without the marker; the entry is NOT dropped.
        assert!(
            section.contains("Music=\"res://music.gd\""),
            "disabled autoload keeps its line, minus the *; got: {section}"
        );
        assert!(!section.contains("*res://music.gd"));
        // It must be INI, not the JSON `to_cfg` emits.
        assert!(!section.contains('{'), "must not be JSON: {section}");
    }

    /// The section round-trips, preserving **declaration order** (= load order,
    /// so a later autoload may reference an earlier one). Alphabetical sorting
    /// would silently reorder boot here.
    #[test]
    fn project_godot_section_round_trips_in_declaration_order() {
        let mut mgr = AutoloadManager::new();
        // Deliberately not alphabetical: Zebra first, Alpha last.
        mgr.add("Zebra", "res://zebra.gd");
        mgr.add("Alpha", "res://alpha.gd");
        mgr.set_enabled("Alpha", false);

        let loaded = AutoloadManager::from_project_godot_section(&mgr.to_project_godot_section());

        assert_eq!(
            loaded.names(),
            vec!["Zebra", "Alpha"],
            "declaration order is load order and must survive the round trip"
        );
        assert!(loaded.get("Zebra").unwrap().enabled);
        assert!(!loaded.get("Alpha").unwrap().enabled);
        assert_eq!(loaded.get("Alpha").unwrap().path, "res://alpha.gd");
        assert_eq!(loaded.globals(), vec!["Zebra"]);
    }

    /// Parsing a whole `project.godot` reads only `[autoload]`, ignoring the
    /// other sections' keys.
    #[test]
    fn parses_autoload_section_out_of_a_full_project_file() {
        let text = "\
[application]

config/name=\"MyGame\"
run/main_scene=\"res://main.tscn\"

[autoload]

Global=\"*res://global.gd\"

[input]

ui_accept={\"deadzone\": 0.5}
";
        let mgr = AutoloadManager::from_project_godot_section(text);

        assert_eq!(mgr.names(), vec!["Global"], "only [autoload] keys are read");
        assert!(mgr.get("Global").unwrap().enabled);
        assert_eq!(mgr.get("Global").unwrap().path, "res://global.gd");
        // Keys from other sections must not leak in as autoloads.
        assert!(!mgr.contains("config/name"));
        assert!(!mgr.contains("ui_accept"));
    }

    /// A project with no `[autoload]` section yields an empty manager, not an error.
    #[test]
    fn project_file_without_autoload_section_is_empty() {
        let mgr =
            AutoloadManager::from_project_godot_section("[application]\n\nconfig/name=\"X\"\n");
        assert!(mgr.is_empty());
        assert_eq!(mgr.to_project_godot_section(), "[autoload]\n\n");
    }

    // -- whole-file splice (pat-0q2kx.73) -----------------------------------

    /// Replacing the section must leave every other section byte-identical —
    /// a settings writer that drops other people's keys is worse than no writer.
    #[test]
    fn splice_replaces_only_the_autoload_section() {
        let original = "[application]\n\nconfig/name=\"Demo\"\nrun/main_scene=\"res://m.tscn\"\n\n\
             [autoload]\n\nOld=\"*res://old.gd\"\n\n\
             [input]\n\nui_accept={\"deadzone\":0.5}\n";
        let mut mgr = AutoloadManager::new();
        mgr.add("Game", "res://game.gd");

        let out = mgr.splice_into_project_godot(original);

        assert!(
            out.contains(r#"Game="*res://game.gd""#),
            "new entry written:\n{out}"
        );
        assert!(!out.contains("Old="), "the old entry is gone:\n{out}");
        assert!(
            out.contains(r#"config/name="Demo""#),
            "application kept:\n{out}"
        );
        assert!(out.contains("run/main_scene="), "main scene kept:\n{out}");
        assert!(out.contains("ui_accept="), "the input section kept:\n{out}");
        // The section keeps its position rather than migrating to the end.
        let autoload_at = out.find("[autoload]").expect("section present");
        let input_at = out.find("[input]").expect("input present");
        assert!(autoload_at < input_at, "section order preserved:\n{out}");
    }

    /// A project that never had the section gets one appended.
    #[test]
    fn splice_appends_when_no_section_exists() {
        let mut mgr = AutoloadManager::new();
        mgr.add("Game", "res://game.gd");
        let out = mgr.splice_into_project_godot("[application]\n\nconfig/name=\"Demo\"\n");
        assert!(out.contains("[autoload]"), "section added:\n{out}");
        assert!(out.contains(r#"Game="*res://game.gd""#));
        assert!(
            out.contains(r#"config/name="Demo""#),
            "existing keys survive:\n{out}"
        );
    }

    /// Removing the last autoload removes the section rather than leaving a stub.
    #[test]
    fn splice_drops_the_section_when_empty() {
        let original =
            "[application]\n\nconfig/name=\"Demo\"\n\n[autoload]\n\nGame=\"*res://game.gd\"\n";
        let out = AutoloadManager::new().splice_into_project_godot(original);
        assert!(!out.contains("[autoload]"), "empty section removed:\n{out}");
        assert!(
            out.contains(r#"config/name="Demo""#),
            "other sections survive:\n{out}"
        );
    }

    /// A disabled autoload round-trips without the `*` marker.
    #[test]
    fn splice_round_trips_the_enabled_marker() {
        let mut mgr = AutoloadManager::new();
        mgr.add("On", "res://on.gd");
        mgr.add("Off", "res://off.gd");
        mgr.set_enabled("Off", false);

        let out = mgr.splice_into_project_godot("[application]\n\nconfig/name=\"D\"\n");
        assert!(
            out.contains(r#"On="*res://on.gd""#),
            "enabled keeps the star:\n{out}"
        );
        assert!(
            out.contains(r#"Off="res://off.gd""#),
            "disabled has no star:\n{out}"
        );

        let back = AutoloadManager::from_project_godot_section(&out);
        assert_eq!(back.names(), vec!["On", "Off"], "order survives the trip");
        assert!(back.get("On").unwrap().enabled);
        assert!(!back.get("Off").unwrap().enabled);
    }
}
