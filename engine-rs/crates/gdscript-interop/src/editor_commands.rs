//! pat-0q2kx.241 — commands a `@tool` script registers into the editor palette.
//!
//! Godot's `EditorCommandPalette.add_command(command_name, key_name,
//! binded_callable, shortcut_text)` lets an `EditorPlugin` put its own entries
//! in the editor's command palette. Patina's served editor already has a
//! palette and a `GET /api/commands` endpoint, but that endpoint enumerated a
//! **`const EDITOR_COMMANDS` array** — so a command added by a script had no
//! path to it whatsoever, no matter how the class was registered.
//!
//! This store is that path: the interpreter's `EditorCommandPalette` surface
//! writes here, and `api_get_commands` merges it with the built-ins.
//!
//! ## Why process-global rather than thread_local
//!
//! The two ends run on **different threads**. A plugin is enabled inside one
//! HTTP connection handler (`POST /api/plugin/enable` → `start_editor_plugin`,
//! which runs the script's `_enter_tree`), while `GET /api/commands` is served
//! on whichever connection thread arrives next. A `thread_local` store — the
//! idiom the interpreter's *handle* tables use — would put the command on the
//! enabling thread and leave the endpoint reading an empty table, which is
//! precisely the silent no-op this bead exists to close.
//!
//! ## Why `gdscript-interop` and not `gdeditor`
//!
//! The writer is the interpreter (here) and the reader is the editor server.
//! `gdeditor` depends on `gdscript-interop`, so this is visible to both; the
//! reverse placement would not be, since `gdscript-interop` cannot see
//! `gdeditor`. Same constraint that put `shader_globals` in `gdvariant`.

use std::sync::{Mutex, OnceLock};

/// One palette entry registered by a script.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptCommand {
    /// Godot's `key_name` — the unique id, conventionally `"category/action"`.
    pub key_name: String,
    /// Godot's `command_name` — the human-readable label shown in the palette.
    pub command_name: String,
    /// Godot's `shortcut_text`. Godot's own default is the string `"None"`,
    /// which is stored as `None` here so the endpoint can omit it rather than
    /// advertising a shortcut literally named "None".
    pub shortcut: Option<String>,
    /// The method name of the bound Callable, when one was supplied.
    ///
    /// Kept so the entry knows what it would invoke. Routing an
    /// `/api/command/execute` back into the owning script instance is **not**
    /// implemented — see the note in the bead report; the palette lists and
    /// identifies the command, but firing it does not yet re-enter the plugin.
    pub callback: Option<String>,
}

impl ScriptCommand {
    /// The palette category, taken from the `key_name` prefix the way Godot's
    /// palette groups entries (`"project/run"` → `"project"`).
    ///
    /// Falls back to `"plugin"` for a bare key with no `/`, rather than
    /// inventing an empty category that would render as an unlabelled group.
    pub fn category(&self) -> String {
        match self.key_name.split_once('/') {
            Some((prefix, _)) if !prefix.is_empty() => prefix.to_string(),
            _ => "plugin".to_string(),
        }
    }
}

fn store() -> &'static Mutex<Vec<ScriptCommand>> {
    static STORE: OnceLock<Mutex<Vec<ScriptCommand>>> = OnceLock::new();
    STORE.get_or_init(|| Mutex::new(Vec::new()))
}

/// `EditorCommandPalette.add_command`.
///
/// Re-adding the same `key_name` **replaces** the existing entry in place
/// rather than appending a duplicate: a plugin that is disabled and re-enabled
/// runs `_enter_tree` again, and Godot's palette shows one entry per key, not
/// one per enable.
pub fn add_command(
    command_name: &str,
    key_name: &str,
    callback: Option<String>,
    shortcut: Option<String>,
) {
    // Godot spells "no shortcut" as the literal string "None".
    let shortcut = shortcut.filter(|s| !s.is_empty() && s != "None");
    let entry = ScriptCommand {
        key_name: key_name.to_string(),
        command_name: command_name.to_string(),
        shortcut,
        callback,
    };
    let mut guard = store().lock().expect("editor commands poisoned");
    match guard.iter_mut().find(|c| c.key_name == entry.key_name) {
        Some(existing) => *existing = entry,
        None => guard.push(entry),
    }
}

/// `EditorCommandPalette.remove_command`. Returns whether anything was removed.
pub fn remove_command(key_name: &str) -> bool {
    let mut guard = store().lock().expect("editor commands poisoned");
    let before = guard.len();
    guard.retain(|c| c.key_name != key_name);
    guard.len() != before
}

/// Every script-registered command, in registration order.
///
/// Insertion order is preserved deliberately: the palette shows plugin commands
/// in the order their plugins registered them, and sorting here would reorder
/// entries whenever an unrelated plugin was enabled.
pub fn all() -> Vec<ScriptCommand> {
    store()
        .lock()
        .expect("editor commands poisoned")
        .iter()
        .cloned()
        .collect()
}

/// Look up one command by its `key_name`.
pub fn find(key_name: &str) -> Option<ScriptCommand> {
    store()
        .lock()
        .expect("editor commands poisoned")
        .iter()
        .find(|c| c.key_name == key_name)
        .cloned()
}

/// Whether a command with this `key_name` is registered.
pub fn contains(key_name: &str) -> bool {
    store()
        .lock()
        .expect("editor commands poisoned")
        .iter()
        .any(|c| c.key_name == key_name)
}

/// How many script commands are registered.
pub fn count() -> usize {
    store().lock().expect("editor commands poisoned").len()
}

/// Drops every entry — used when closing a project, and by tests.
pub fn clear() {
    store().lock().expect("editor commands poisoned").clear();
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Resets the process-global store.
    ///
    /// Safe because this repo runs tests under nextest, which gives each test
    /// its own process, so one test's `clear()` cannot race another's data.
    fn fresh() {
        clear();
    }

    #[test]
    fn add_then_list_roundtrips() {
        fresh();
        add_command("Say Hello", "plugin/hello", Some("_on_hello".into()), None);
        let all = all();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].command_name, "Say Hello");
        assert_eq!(all[0].key_name, "plugin/hello");
        assert_eq!(all[0].callback.as_deref(), Some("_on_hello"));
        assert!(contains("plugin/hello"));
    }

    #[test]
    fn re_adding_a_key_replaces_rather_than_duplicating() {
        fresh();
        add_command("First", "plugin/x", None, None);
        add_command("Second", "plugin/x", None, None);
        assert_eq!(count(), 1, "a re-enabled plugin must not double its entry");
        assert_eq!(find("plugin/x").unwrap().command_name, "Second");
    }

    #[test]
    fn godots_none_shortcut_is_stored_as_absent() {
        fresh();
        add_command("A", "plugin/a", None, Some("None".into()));
        add_command("B", "plugin/b", None, Some("Ctrl+Shift+P".into()));
        assert_eq!(find("plugin/a").unwrap().shortcut, None);
        assert_eq!(
            find("plugin/b").unwrap().shortcut.as_deref(),
            Some("Ctrl+Shift+P"),
        );
    }

    #[test]
    fn category_comes_from_the_key_prefix() {
        fresh();
        add_command("A", "project/run", None, None);
        add_command("B", "bare", None, None);
        assert_eq!(find("project/run").unwrap().category(), "project");
        assert_eq!(
            find("bare").unwrap().category(),
            "plugin",
            "a key with no '/' must not produce an empty category",
        );
    }

    #[test]
    fn remove_reports_whether_it_removed_anything() {
        fresh();
        add_command("A", "plugin/a", None, None);
        assert!(remove_command("plugin/a"));
        assert!(!remove_command("plugin/a"), "a second remove is a no-op");
        assert_eq!(count(), 0);
    }

    #[test]
    fn registration_order_is_preserved() {
        fresh();
        for k in ["plugin/c", "plugin/a", "plugin/b"] {
            add_command("x", k, None, None);
        }
        let keys: Vec<String> = all().into_iter().map(|c| c.key_name).collect();
        assert_eq!(keys, vec!["plugin/c", "plugin/a", "plugin/b"]);
    }
}
