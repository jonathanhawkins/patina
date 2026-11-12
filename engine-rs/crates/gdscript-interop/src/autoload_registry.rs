//! Autoload singleton global-name table (pat-nzgdu.70).
//!
//! Godot registers every `[autoload]` entry declared in `project.godot` as a
//! **global name** (`main/main.cpp` `add_autoload`): once `Global="*res://global.gd"`
//! is declared, any script can write `Global.score` and the identifier resolves
//! to the autoload node living at `/root/Global` — no `get_node` required.
//!
//! Two halves make that work, and only the first lives here:
//!
//! 1. **Which names are autoloads** — process-global, because autoloads are
//!    declared once per engine run and every script in the run shares them.
//!    That is this table.
//! 2. **Which node a name points at** — resolved per-interpreter against its own
//!    `SceneAccess` (`/root/<Name>`), so two SceneTrees in one process (the
//!    editor's and the runner's) each resolve to their *own* node. This table
//!    deliberately stores no node ids for that reason.
//!
//! The global-table shape mirrors `gdobject::class_db`, which registers class
//! metadata the same way and for the same reason.
//!
//! Registration happens during autoload instantiation
//! (`gdscene::autoload::instantiate_autoloads`), so both the runner boot path
//! and the editor's Play path bind the names by construction.

use std::collections::BTreeSet;
use std::sync::{Mutex, OnceLock};

/// The process-global set of registered autoload names.
fn registry() -> &'static Mutex<BTreeSet<String>> {
    static REGISTRY: OnceLock<Mutex<BTreeSet<String>>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(BTreeSet::new()))
}

/// Registers `name` as an autoload global.
///
/// Idempotent — re-registering an existing name is a no-op, so a re-boot in the
/// same process (the editor stopping and re-starting Play) does not duplicate.
pub fn register_autoload(name: impl Into<String>) {
    let mut reg = registry().lock().expect("autoload registry lock poisoned");
    reg.insert(name.into());
}

/// Returns `true` if `name` was registered as an autoload global.
pub fn is_autoload(name: &str) -> bool {
    let reg = registry().lock().expect("autoload registry lock poisoned");
    reg.contains(name)
}

/// Returns every registered autoload name, sorted.
pub fn autoload_names() -> Vec<String> {
    let reg = registry().lock().expect("autoload registry lock poisoned");
    reg.iter().cloned().collect()
}

/// Clears the table.
///
/// Used when a boot re-runs in a process that already booted (and by tests).
/// Under `cargo nextest` each test is its own process, so this is belt-and-braces
/// there; it matters for `cargo test`, which shares one process across threads.
pub fn clear_autoloads() {
    let mut reg = registry().lock().expect("autoload registry lock poisoned");
    reg.clear();
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Serializes the tests in this module: they share one process-global table.
    static TEST_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn registers_and_resolves_names() {
        let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        clear_autoloads();

        assert!(
            !is_autoload("Global"),
            "unregistered name is not an autoload"
        );

        register_autoload("Global");
        assert!(is_autoload("Global"));
        // Only the declared name binds — a near-miss must stay unresolved so it
        // still raises UndefinedVariable rather than silently finding a node.
        assert!(!is_autoload("global"));
        assert!(!is_autoload("Globals"));

        clear_autoloads();
    }

    #[test]
    fn registration_is_idempotent_and_listable() {
        let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        clear_autoloads();

        register_autoload("Music");
        register_autoload("Global");
        register_autoload("Global");

        assert_eq!(
            autoload_names(),
            vec!["Global".to_string(), "Music".to_string()]
        );

        clear_autoloads();
        assert!(autoload_names().is_empty(), "clear empties the table");
    }
}
