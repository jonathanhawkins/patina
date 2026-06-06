//! Scene-menu command dispatch and the scene-document lifecycle.
//!
//! Maps the top-bar **Scene** menu items (New Scene, New Inherited Scene, Open
//! Scene, Save, Save As, Save All, Close Scene, Revert Scene, Quit) to the
//! document commands they run against the set of open scene documents, and owns
//! that document set. Save and Save As persist the active scene to disk.
//!
//! This is the action→command layer for the Scene menu; it mirrors
//! [`crate::editor_menu_commands`] for the Editor menu.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// An item in the Scene menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SceneMenuAction {
    /// Create a fresh, empty scene.
    NewScene,
    /// Create a scene that inherits from a base scene.
    NewInheritedScene,
    /// Open a scene from disk.
    OpenScene,
    /// Save the active scene to its existing path.
    Save,
    /// Save the active scene to a new path.
    SaveAs,
    /// Save every open scene that has a path.
    SaveAll,
    /// Close the active scene.
    CloseScene,
    /// Reload the active scene from disk, discarding edits.
    RevertScene,
    /// Quit the editor.
    Quit,
}

/// The document command produced by dispatching a Scene-menu action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SceneCommand {
    /// A new empty scene was created and made active.
    Created,
    /// A new scene inheriting from the given base was created and made active.
    CreatedInherited(PathBuf),
    /// A scene was opened from the given path and made active.
    Opened(PathBuf),
    /// The active scene was saved to the given path.
    Saved(PathBuf),
    /// The active scene was saved to a new given path.
    SavedAs(PathBuf),
    /// `n` open scenes were saved to disk.
    SavedAll(usize),
    /// The active scene was closed.
    Closed,
    /// The active scene was reverted to its on-disk contents.
    Reverted(PathBuf),
    /// The editor was asked to quit.
    Quit,
    /// The action needed an active scene but none was open.
    NoActiveScene,
    /// The action needed a path argument but none was given.
    NeedsPath,
}

/// A single open scene document.
#[derive(Debug, Clone, Default)]
pub struct SceneDocument {
    /// The on-disk path, once the scene has been saved or opened.
    path: Option<PathBuf>,
    /// The scene's serialized contents.
    content: String,
    /// The base scene this document inherits from, if any.
    inherited_from: Option<PathBuf>,
    /// Whether the document has unsaved edits.
    dirty: bool,
}

impl SceneDocument {
    /// The document's on-disk path, if it has one.
    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// The document's current contents.
    pub fn content(&self) -> &str {
        &self.content
    }

    /// The base scene this document inherits from, if any.
    pub fn inherited_from(&self) -> Option<&Path> {
        self.inherited_from.as_deref()
    }

    /// Whether the document has unsaved edits.
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// Replaces the document's contents, marking it dirty.
    pub fn set_content(&mut self, content: &str) {
        self.content = content.to_string();
        self.dirty = true;
    }
}

/// The set of open scene documents and the Scene-menu dispatcher over them.
#[derive(Debug, Clone, Default)]
pub struct SceneDocuments {
    docs: Vec<SceneDocument>,
    active: Option<usize>,
}

impl SceneDocuments {
    /// Creates an empty document set (no scenes open).
    pub fn new() -> Self {
        Self::default()
    }

    /// The Scene-menu items, in display order.
    pub fn items() -> [SceneMenuAction; 9] {
        [
            SceneMenuAction::NewScene,
            SceneMenuAction::NewInheritedScene,
            SceneMenuAction::OpenScene,
            SceneMenuAction::Save,
            SceneMenuAction::SaveAs,
            SceneMenuAction::SaveAll,
            SceneMenuAction::CloseScene,
            SceneMenuAction::RevertScene,
            SceneMenuAction::Quit,
        ]
    }

    /// Number of open scene documents.
    pub fn len(&self) -> usize {
        self.docs.len()
    }

    /// Whether no scenes are open.
    pub fn is_empty(&self) -> bool {
        self.docs.is_empty()
    }

    /// The index of the active scene document, if any.
    pub fn active_index(&self) -> Option<usize> {
        self.active
    }

    /// The active scene document, if any.
    pub fn active(&self) -> Option<&SceneDocument> {
        self.active.and_then(|i| self.docs.get(i))
    }

    /// The active scene document mutably, if any.
    pub fn active_mut(&mut self) -> Option<&mut SceneDocument> {
        match self.active {
            Some(i) => self.docs.get_mut(i),
            None => None,
        }
    }

    /// Dispatches a Scene-menu action to its document command, applying the
    /// matching lifecycle change. Actions that touch disk (Open, New Inherited,
    /// Save As, and Save when the scene is unsaved) consult `path`; Save reuses
    /// the active scene's existing path when `path` is `None`.
    pub fn dispatch(
        &mut self,
        action: SceneMenuAction,
        path: Option<&Path>,
    ) -> io::Result<SceneCommand> {
        match action {
            SceneMenuAction::NewScene => {
                self.push_active(SceneDocument::default());
                Ok(SceneCommand::Created)
            }
            SceneMenuAction::NewInheritedScene => match path {
                Some(base) => {
                    let doc = SceneDocument {
                        inherited_from: Some(base.to_path_buf()),
                        dirty: true,
                        ..Default::default()
                    };
                    self.push_active(doc);
                    Ok(SceneCommand::CreatedInherited(base.to_path_buf()))
                }
                None => Ok(SceneCommand::NeedsPath),
            },
            SceneMenuAction::OpenScene => match path {
                Some(p) => {
                    let content = fs::read_to_string(p)?;
                    let doc = SceneDocument {
                        path: Some(p.to_path_buf()),
                        content,
                        ..Default::default()
                    };
                    self.push_active(doc);
                    Ok(SceneCommand::Opened(p.to_path_buf()))
                }
                None => Ok(SceneCommand::NeedsPath),
            },
            SceneMenuAction::Save => self.save_active(path),
            SceneMenuAction::SaveAs => match path {
                Some(p) => {
                    let Some(doc) = self.active_mut() else {
                        return Ok(SceneCommand::NoActiveScene);
                    };
                    doc.path = Some(p.to_path_buf());
                    Self::persist(doc)?;
                    Ok(SceneCommand::SavedAs(p.to_path_buf()))
                }
                None => Ok(SceneCommand::NeedsPath),
            },
            SceneMenuAction::SaveAll => {
                let mut saved = 0;
                for doc in self.docs.iter_mut() {
                    if doc.path.is_some() {
                        Self::persist(doc)?;
                        saved += 1;
                    }
                }
                Ok(SceneCommand::SavedAll(saved))
            }
            SceneMenuAction::CloseScene => match self.active {
                Some(i) => {
                    self.docs.remove(i);
                    self.active = if self.docs.is_empty() {
                        None
                    } else {
                        Some(i.min(self.docs.len() - 1))
                    };
                    Ok(SceneCommand::Closed)
                }
                None => Ok(SceneCommand::NoActiveScene),
            },
            SceneMenuAction::RevertScene => {
                let Some(doc) = self.active_mut() else {
                    return Ok(SceneCommand::NoActiveScene);
                };
                match doc.path.clone() {
                    Some(p) => {
                        doc.content = fs::read_to_string(&p)?;
                        doc.dirty = false;
                        Ok(SceneCommand::Reverted(p))
                    }
                    None => Ok(SceneCommand::NeedsPath),
                }
            }
            SceneMenuAction::Quit => Ok(SceneCommand::Quit),
        }
    }

    fn push_active(&mut self, doc: SceneDocument) {
        self.docs.push(doc);
        self.active = Some(self.docs.len() - 1);
    }

    fn save_active(&mut self, fallback_path: Option<&Path>) -> io::Result<SceneCommand> {
        let Some(doc) = self.active_mut() else {
            return Ok(SceneCommand::NoActiveScene);
        };
        if doc.path.is_none() {
            match fallback_path {
                Some(p) => doc.path = Some(p.to_path_buf()),
                None => return Ok(SceneCommand::NeedsPath),
            }
        }
        Self::persist(doc)?;
        // `persist` only runs when a path is present.
        let path = doc.path.clone().expect("path set above");
        Ok(SceneCommand::Saved(path))
    }

    fn persist(doc: &mut SceneDocument) -> io::Result<()> {
        if let Some(path) = doc.path.clone() {
            fs::write(&path, &doc.content)?;
            doc.dirty = false;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A throwaway directory for this test's on-disk scenes. Named uniquely so
    /// it cannot collide with other tests running in parallel.
    fn temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join("patina_pat_v7pk1_scene_menu");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    /// Acceptance (pat-v7pk1): each Scene menu action dispatches its document
    /// command, and Save / Save As persist the active scene to disk.
    #[test]
    fn menus_scene_actions_dispatch() {
        let dir = temp_dir();
        let mut docs = SceneDocuments::new();

        // The menu exposes all nine scene items, in order.
        assert_eq!(
            SceneDocuments::items(),
            [
                SceneMenuAction::NewScene,
                SceneMenuAction::NewInheritedScene,
                SceneMenuAction::OpenScene,
                SceneMenuAction::Save,
                SceneMenuAction::SaveAs,
                SceneMenuAction::SaveAll,
                SceneMenuAction::CloseScene,
                SceneMenuAction::RevertScene,
                SceneMenuAction::Quit,
            ]
        );

        // New Scene creates a fresh active document.
        assert_eq!(
            docs.dispatch(SceneMenuAction::NewScene, None).unwrap(),
            SceneCommand::Created
        );
        assert_eq!(docs.len(), 1);
        assert_eq!(docs.active_index(), Some(0));
        docs.active_mut().unwrap().set_content("[gd_scene]\nname=\"Main\"\n");
        assert!(docs.active().unwrap().is_dirty());

        // Save As persists the active scene to disk at the chosen path.
        let main_path = dir.join("main.tscn");
        let cmd = docs
            .dispatch(SceneMenuAction::SaveAs, Some(&main_path))
            .unwrap();
        assert_eq!(cmd, SceneCommand::SavedAs(main_path.clone()));
        assert!(main_path.exists(), "Save As wrote the scene to disk");
        assert_eq!(
            fs::read_to_string(&main_path).unwrap(),
            "[gd_scene]\nname=\"Main\"\n"
        );
        assert!(!docs.active().unwrap().is_dirty(), "saving clears dirty");

        // Editing then Save persists to the existing path (no new path needed).
        docs.active_mut().unwrap().set_content("[gd_scene]\nname=\"Main2\"\n");
        let cmd = docs.dispatch(SceneMenuAction::Save, None).unwrap();
        assert_eq!(cmd, SceneCommand::Saved(main_path.clone()));
        assert_eq!(
            fs::read_to_string(&main_path).unwrap(),
            "[gd_scene]\nname=\"Main2\"\n",
            "Save overwrote the file on disk"
        );

        // New Inherited Scene records its base and becomes active.
        let cmd = docs
            .dispatch(SceneMenuAction::NewInheritedScene, Some(&main_path))
            .unwrap();
        assert_eq!(cmd, SceneCommand::CreatedInherited(main_path.clone()));
        assert_eq!(docs.len(), 2);
        assert_eq!(
            docs.active().unwrap().inherited_from(),
            Some(main_path.as_path())
        );

        // Open Scene reads a file from disk into a new active document.
        let other_path = dir.join("other.tscn");
        fs::write(&other_path, "[gd_scene]\nname=\"Other\"\n").unwrap();
        let cmd = docs
            .dispatch(SceneMenuAction::OpenScene, Some(&other_path))
            .unwrap();
        assert_eq!(cmd, SceneCommand::Opened(other_path.clone()));
        assert_eq!(docs.len(), 3);
        assert_eq!(docs.active().unwrap().content(), "[gd_scene]\nname=\"Other\"\n");

        // Revert Scene reloads the active scene from disk, dropping edits.
        docs.active_mut().unwrap().set_content("dirty edits");
        assert!(docs.active().unwrap().is_dirty());
        let cmd = docs.dispatch(SceneMenuAction::RevertScene, None).unwrap();
        assert_eq!(cmd, SceneCommand::Reverted(other_path.clone()));
        assert_eq!(docs.active().unwrap().content(), "[gd_scene]\nname=\"Other\"\n");
        assert!(!docs.active().unwrap().is_dirty(), "revert clears dirty");

        // Save All writes every document that has a path (main + other; the
        // inherited scene has no path yet and is skipped).
        let cmd = docs.dispatch(SceneMenuAction::SaveAll, None).unwrap();
        assert_eq!(cmd, SceneCommand::SavedAll(2));

        // Close Scene removes the active document.
        let before = docs.len();
        assert_eq!(
            docs.dispatch(SceneMenuAction::CloseScene, None).unwrap(),
            SceneCommand::Closed
        );
        assert_eq!(docs.len(), before - 1);

        // Quit reports the quit command.
        assert_eq!(
            docs.dispatch(SceneMenuAction::Quit, None).unwrap(),
            SceneCommand::Quit
        );

        let _ = fs::remove_dir_all(&dir);
    }

    /// Save with no active scene reports `NoActiveScene`; Save As / Open with no
    /// path report `NeedsPath`.
    #[test]
    fn scene_actions_guard_missing_preconditions() {
        let mut docs = SceneDocuments::new();
        assert_eq!(
            docs.dispatch(SceneMenuAction::Save, None).unwrap(),
            SceneCommand::NoActiveScene
        );
        assert_eq!(
            docs.dispatch(SceneMenuAction::CloseScene, None).unwrap(),
            SceneCommand::NoActiveScene
        );
        docs.dispatch(SceneMenuAction::NewScene, None).unwrap();
        assert_eq!(
            docs.dispatch(SceneMenuAction::SaveAs, None).unwrap(),
            SceneCommand::NeedsPath
        );
        assert_eq!(
            docs.dispatch(SceneMenuAction::Save, None).unwrap(),
            SceneCommand::NeedsPath,
            "an unsaved scene with no path can't Save"
        );
        assert_eq!(
            docs.dispatch(SceneMenuAction::OpenScene, None).unwrap(),
            SceneCommand::NeedsPath
        );
    }
}
