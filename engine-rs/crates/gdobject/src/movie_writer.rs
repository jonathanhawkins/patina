//! MovieWriter registry (pat-nzgdu.180).
//!
//! Godot's `MovieWriter.add_writer()` is a process-global registry of offline
//! video writers, consulted at movie-mode start (`--write-movie <path>`).
//! Scripts subclass the abstract `MovieWriter` class and register an instance so
//! it is considered when capture begins.
//!
//! Actual frame/audio encoding is deliberately **deferred** here — on the web
//! target the headless CLI and real filesystem output are unavailable, so full
//! frame capture is out of practical scope. What this module provides is the
//! class-surface registry: writers can be registered and retrieved without
//! error, matching Godot's `add_writer` behavior, so script/tool references
//! resolve. The `MovieWriter` class itself is registered in [`crate::class_db`].

use std::sync::{Mutex, OnceLock};

/// A writer registered via [`add_writer`], identified by the class name of the
/// (script or built-in) `MovieWriter` subclass. The per-frame encode callbacks
/// are deferred, so only the identity is retained here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegisteredWriter {
    /// Class name of the registered `MovieWriter` subclass.
    pub name: String,
}

fn registry() -> &'static Mutex<Vec<RegisteredWriter>> {
    static REGISTRY: OnceLock<Mutex<Vec<RegisteredWriter>>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(Vec::new()))
}

/// Godot `MovieWriter.add_writer(writer)`: registers a writer (by subclass name)
/// into the process-global registry so it is considered at movie-mode start.
pub fn add_writer(name: impl Into<String>) {
    registry().lock().unwrap().push(RegisteredWriter { name: name.into() });
}

/// Returns all registered writers, in registration order.
pub fn registered_writers() -> Vec<RegisteredWriter> {
    registry().lock().unwrap().clone()
}

/// The number of writers registered via [`add_writer`].
pub fn writer_count() -> usize {
    registry().lock().unwrap().len()
}

/// Removes all registered writers. Intended for test isolation.
pub fn clear_writers() {
    registry().lock().unwrap().clear();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::class_db;

    #[test]
    fn movie_writer_class_registered() {
        // MovieWriter is a registered, abstract ClassDB class inheriting Object,
        // so script subclasses resolve without error.
        class_db::register_editor_classes();
        assert!(
            class_db::class_exists("MovieWriter"),
            "MovieWriter is registered in ClassDB",
        );
        assert_eq!(
            class_db::get_class_info("MovieWriter").map(|i| i.parent_class),
            Some("Object".to_string()),
            "MovieWriter inherits Object",
        );

        // A (script-subclass) writer can be passed to add_writer() and is then
        // retrievable from the writer registry, in registration order.
        clear_writers();
        add_writer("MJPEGWriter");
        add_writer("PNGWriter");
        assert_eq!(writer_count(), 2, "both writers are registered");
        let names: Vec<String> =
            registered_writers().into_iter().map(|w| w.name).collect();
        assert_eq!(
            names,
            vec!["MJPEGWriter".to_string(), "PNGWriter".to_string()],
            "writers are retrievable in registration order",
        );
    }
}
