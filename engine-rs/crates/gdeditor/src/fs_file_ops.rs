//! FileSystem dock **file operations** (pat-luft1).
//!
//! Models the dock's file operations over a virtual project filesystem: create
//! a folder, rename/move a resource, duplicate, and delete. The important
//! behavior is *dependency-safe remapping*: renaming or moving a resource
//! rewrites every reference to its old path in dependent scenes/resources so
//! their links keep resolving.

use std::collections::{BTreeMap, BTreeSet};

/// A virtual project filesystem of files (path → content) and folders.
#[derive(Debug, Clone, Default)]
pub struct VirtualFs {
    files: BTreeMap<String, String>,
    folders: BTreeSet<String>,
}

impl VirtualFs {
    /// Creates an empty filesystem.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds (or overwrites) a file with `content`.
    pub fn add_file(&mut self, path: impl Into<String>, content: impl Into<String>) {
        self.files.insert(path.into(), content.into());
    }

    /// Whether a file or folder exists at `path`.
    pub fn exists(&self, path: &str) -> bool {
        self.files.contains_key(path) || self.folders.contains(path)
    }

    /// Reads a file's content.
    pub fn read(&self, path: &str) -> Option<&str> {
        self.files.get(path).map(String::as_str)
    }

    /// The number of files tracked.
    pub fn file_count(&self) -> usize {
        self.files.len()
    }

    /// Creates a new folder at `path`. Returns whether it was created (false if
    /// something already exists there).
    pub fn new_folder(&mut self, path: impl Into<String>) -> bool {
        let path = path.into();
        if self.exists(&path) {
            return false;
        }
        self.folders.insert(path);
        true
    }

    /// Renames or moves the file at `src` to `dst`, remapping every reference to
    /// `src` in other files to `dst` (dependency-safe). Returns whether the move
    /// happened (false if `src` is missing or `dst` already exists).
    pub fn rename(&mut self, src: &str, dst: &str) -> bool {
        if src == dst || self.files.contains_key(dst) {
            return false;
        }
        let content = match self.files.remove(src) {
            Some(c) => c,
            None => return false,
        };
        self.files.insert(dst.to_string(), content);
        // Remap references in every dependent file (not the moved file itself).
        for (path, body) in self.files.iter_mut() {
            if path != dst {
                *body = body.replace(src, dst);
            }
        }
        true
    }

    /// Moves `src` to `dst`. Alias of [`rename`](Self::rename) — both remap
    /// dependent references.
    pub fn move_file(&mut self, src: &str, dst: &str) -> bool {
        self.rename(src, dst)
    }

    /// Duplicates the file at `src` to `dst`. Returns whether it was duplicated
    /// (false if `src` is missing or `dst` already exists).
    pub fn duplicate(&mut self, src: &str, dst: &str) -> bool {
        if self.files.contains_key(dst) {
            return false;
        }
        match self.files.get(src) {
            Some(content) => {
                let copy = content.clone();
                self.files.insert(dst.to_string(), copy);
                true
            }
            None => false,
        }
    }

    /// Deletes the file or folder at `path`. Returns whether something was
    /// removed.
    pub fn delete(&mut self, path: &str) -> bool {
        self.files.remove(path).is_some() || self.folders.remove(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Acceptance (pat-luft1): renaming/moving a resource updates references in
    /// dependent scenes/resources, and delete/duplicate/new-folder affect the
    /// filesystem correctly.
    #[test]
    fn fs_dock_file_ops_remap_dependencies() {
        let mut fs = VirtualFs::new();
        fs.add_file("res://art/old.png", "PNGDATA");
        fs.add_file(
            "res://scenes/level.tscn",
            "[ext_resource path=\"res://art/old.png\" type=\"Texture2D\" id=\"1\"]\n[node name=\"Sprite\"]",
        );
        fs.add_file("res://scenes/menu.tscn", "no refs here");

        // New folder.
        assert!(fs.new_folder("res://textures"));
        assert!(fs.exists("res://textures"));
        // Can't create a folder where something already exists.
        assert!(!fs.new_folder("res://textures"));

        // Move/rename the resource into the new folder.
        assert!(fs.rename("res://art/old.png", "res://textures/new.png"));
        assert!(!fs.exists("res://art/old.png"));
        assert!(fs.exists("res://textures/new.png"));

        // Dependent scene's reference is remapped to the new path.
        let level = fs.read("res://scenes/level.tscn").unwrap();
        assert!(level.contains("res://textures/new.png"));
        assert!(!level.contains("res://art/old.png"));

        // Unrelated file is untouched.
        assert_eq!(fs.read("res://scenes/menu.tscn"), Some("no refs here"));

        // Renaming a missing file, or onto an existing one, fails.
        assert!(!fs.rename("res://art/old.png", "res://x.png"));
        assert!(!fs.rename("res://textures/new.png", "res://scenes/level.tscn"));

        // Duplicate the resource.
        assert!(fs.duplicate("res://textures/new.png", "res://textures/new_copy.png"));
        assert_eq!(fs.read("res://textures/new_copy.png"), Some("PNGDATA"));
        // Duplicating onto an existing path fails.
        assert!(!fs.duplicate("res://textures/new.png", "res://textures/new_copy.png"));

        // Delete.
        assert!(fs.delete("res://textures/new_copy.png"));
        assert!(!fs.exists("res://textures/new_copy.png"));
        assert!(!fs.delete("res://does/not/exist"));

        // Deleting the new folder removes it.
        assert!(fs.delete("res://textures"));
        assert!(!fs.exists("res://textures"));
    }

    /// `move_file` behaves like rename, remapping every dependent reference.
    #[test]
    fn move_remaps_multiple_dependents() {
        let mut fs = VirtualFs::new();
        fs.add_file("res://a.tres", "data");
        fs.add_file("res://one.tscn", "uses res://a.tres here");
        fs.add_file("res://two.tscn", "also res://a.tres twice: res://a.tres");

        assert!(fs.move_file("res://a.tres", "res://sub/a.tres"));
        assert_eq!(fs.read("res://one.tscn"), Some("uses res://sub/a.tres here"));
        assert_eq!(
            fs.read("res://two.tscn"),
            Some("also res://sub/a.tres twice: res://sub/a.tres")
        );
    }
}
