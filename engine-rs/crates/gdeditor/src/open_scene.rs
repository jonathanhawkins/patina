//! The editor's `Scene > Open Scene` action.
//!
//! Opening a scene parses a `.tscn` file and adds it to the
//! [`MultiDocModel`](crate::multi_document::MultiDocModel) as a new, focused
//! `Scene` document — mirroring Godot's behaviour where opening a scene loads
//! it into its own tab. Re-opening an already-open scene just focuses the
//! existing tab instead of loading a duplicate.
//!
//! This lives in its own module (rather than in `multi_document.rs`) because it
//! bridges the scene loader (`gdscene::packed_scene`) and the document model;
//! it only uses `MultiDocModel`'s public API.

use gdcore::error::EngineResult;
use gdscene::packed_scene::PackedScene;

use crate::multi_document::{DocumentKind, MultiDocModel, OpenDocument};

impl MultiDocModel {
    /// Loads a scene from `.tscn` source text and opens it as a new `Scene`
    /// document at `path`, focusing it and returning its document index.
    ///
    /// The document's title is the loaded scene's root node name, which proves
    /// the `.tscn` was actually parsed rather than merely referenced by path.
    /// If a document for `path` is already open, the existing tab is focused
    /// instead of loading a duplicate.
    ///
    /// Returns an error if the `.tscn` fails to parse or contains no scene root
    /// (an empty scene cannot be opened).
    pub fn open_scene_from_tscn(&mut self, path: &str, tscn_source: &str) -> EngineResult<usize> {
        let scene = PackedScene::from_tscn(tscn_source)?;
        // Instantiating validates the scene has a root and yields its name for
        // the tab title. Errors (no nodes / bad root) propagate before any
        // document is opened, so a failed load never adds a tab.
        let nodes = scene.instance()?;
        let title = nodes
            .first()
            .map(|n| n.name().to_string())
            .unwrap_or_else(|| path.to_string());

        Ok(self.open(OpenDocument::new(
            title,
            Some(path.to_string()),
            DocumentKind::Scene,
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MAIN_TSCN: &str = "[gd_scene format=3]\n\n[node name=\"Main\" type=\"Node2D\"]\n";
    const LEVEL_TSCN: &str = "[gd_scene format=3]\n\n[node name=\"Level\" type=\"Node2D\"]\n";

    /// Acceptance (pat-w1ub4.3): opening a scene parses the `.tscn` and loads it
    /// into a new, focused `Scene` document; re-opening the same path reuses the
    /// tab; and a scene with no root fails to open without adding a document.
    #[test]
    fn editor_open_scene_loads_tscn_into_new_document() {
        let mut docs = MultiDocModel::new();
        assert!(docs.is_empty());

        // Opening a scene parses the .tscn and adds a new, active Scene document.
        let idx = docs
            .open_scene_from_tscn("res://main.tscn", MAIN_TSCN)
            .expect("a valid .tscn loads into a document");
        assert_eq!(docs.len(), 1);
        assert_eq!(docs.active_index(), Some(idx));
        let doc = docs.active().expect("active document");
        assert_eq!(doc.kind, DocumentKind::Scene);
        assert_eq!(doc.path.as_deref(), Some("res://main.tscn"));
        assert_eq!(doc.title, "Main", "title is the loaded scene's root node name");

        // Re-opening the same scene focuses the existing tab — no duplicate.
        let again = docs
            .open_scene_from_tscn("res://main.tscn", MAIN_TSCN)
            .expect("re-open succeeds");
        assert_eq!(again, idx, "re-open returns the existing document index");
        assert_eq!(docs.len(), 1, "no duplicate document for the same scene");
        assert_eq!(docs.active_index(), Some(idx));

        // A different scene opens as its own document and takes focus.
        let idx2 = docs
            .open_scene_from_tscn("res://level.tscn", LEVEL_TSCN)
            .expect("a second scene loads");
        assert_eq!(docs.len(), 2);
        assert_eq!(docs.active_index(), Some(idx2));
        assert_ne!(idx2, idx);
        assert_eq!(docs.active().unwrap().title, "Level");

        // A node-less scene fails to load and adds no document.
        let before = docs.len();
        assert!(
            docs.open_scene_from_tscn("res://empty.tscn", "[gd_scene format=3]\n")
                .is_err(),
            "a scene with no root node cannot be opened"
        );
        assert_eq!(docs.len(), before, "a failed load does not add a document");
    }
}
