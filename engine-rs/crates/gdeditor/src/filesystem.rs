//! Virtual editor filesystem.
//!
//! Provides [`EditorFileSystem`] for scanning project directories,
//! resolving `res://` paths, and tracking resource UIDs.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use gdcore::error::{EngineError, EngineResult};
use gdcore::id::ResourceUid;

/// A virtual project filesystem used by the editor.
///
/// Wraps a project root directory and provides utilities for scanning,
/// querying, and resolving resource paths. Also maintains a UID map
/// that associates `res://` paths with stable [`ResourceUid`] values.
#[derive(Debug)]
pub struct EditorFileSystem {
    /// Absolute path to the project root.
    project_root: PathBuf,
    /// Cached list of known files (relative to project root).
    files: Vec<PathBuf>,
    /// Bidirectional UID mapping: res:// path <-> ResourceUid.
    uid_map: HashMap<String, ResourceUid>,
    /// Reverse map: uid -> res:// path.
    uid_reverse: HashMap<ResourceUid, String>,
    /// UID counter for assigning new UIDs.
    next_uid: i64,
}

impl EditorFileSystem {
    /// Creates a new filesystem rooted at the given project directory.
    pub fn new(project_root: impl Into<PathBuf>) -> Self {
        Self {
            project_root: project_root.into(),
            files: Vec::new(),
            uid_map: HashMap::new(),
            uid_reverse: HashMap::new(),
            next_uid: 1,
        }
    }

    /// Returns the project root path.
    pub fn project_root(&self) -> &Path {
        &self.project_root
    }

    /// Scans the project root recursively and caches all file paths.
    pub fn scan(&mut self) -> EngineResult<usize> {
        self.files.clear();
        self.scan_dir(&self.project_root.clone())?;
        self.files.sort();
        let count = self.files.len();
        tracing::debug!("EditorFileSystem scanned {} files", count);
        Ok(count)
    }

    fn scan_dir(&mut self, dir: &Path) -> EngineResult<()> {
        let entries = std::fs::read_dir(dir).map_err(EngineError::Io)?;
        for entry in entries {
            let entry = entry.map_err(EngineError::Io)?;
            let path = entry.path();
            if path.is_dir() {
                // Skip hidden directories.
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    if name.starts_with('.') {
                        continue;
                    }
                }
                self.scan_dir(&path)?;
            } else if path.is_file() {
                if let Ok(rel) = path.strip_prefix(&self.project_root) {
                    self.files.push(rel.to_path_buf());
                }
            }
        }
        Ok(())
    }

    /// Returns all cached file paths (relative to project root).
    pub fn files(&self) -> &[PathBuf] {
        &self.files
    }

    /// Returns files matching the given extension (e.g. `"tscn"`).
    pub fn files_by_extension(&self, ext: &str) -> Vec<&PathBuf> {
        self.files
            .iter()
            .filter(|p| {
                p.extension()
                    .and_then(|e| e.to_str())
                    .map(|e| e.eq_ignore_ascii_case(ext))
                    .unwrap_or(false)
            })
            .collect()
    }

    /// Resolves a `res://` path to an absolute filesystem path.
    ///
    /// Returns `None` if the path doesn't start with `res://`.
    pub fn resolve_res_path(&self, res_path: &str) -> Option<PathBuf> {
        let relative = res_path.strip_prefix("res://")?;
        Some(self.project_root.join(relative))
    }

    /// Converts an absolute path back to a `res://` path.
    ///
    /// Returns `None` if the path is not within the project root.
    pub fn to_res_path(&self, absolute: &Path) -> Option<String> {
        let rel = absolute.strip_prefix(&self.project_root).ok()?;
        Some(format!("res://{}", rel.display()))
    }

    /// Returns `true` if a file exists at the given `res://` path.
    pub fn file_exists(&self, res_path: &str) -> bool {
        self.resolve_res_path(res_path)
            .map(|p| p.exists())
            .unwrap_or(false)
    }

    /// Returns the modification time of a file at the given `res://` path.
    pub fn get_modified_time(&self, res_path: &str) -> EngineResult<SystemTime> {
        let abs = self
            .resolve_res_path(res_path)
            .ok_or_else(|| EngineError::NotFound(format!("invalid res:// path: {res_path}")))?;
        let meta = std::fs::metadata(&abs).map_err(EngineError::Io)?;
        meta.modified().map_err(EngineError::Io)
    }

    /// Assigns a UID to a `res://` path, returning the UID.
    ///
    /// If the path already has a UID, returns the existing one.
    pub fn assign_uid(&mut self, res_path: &str) -> ResourceUid {
        if let Some(&uid) = self.uid_map.get(res_path) {
            return uid;
        }
        let uid = ResourceUid::new(self.next_uid);
        self.next_uid += 1;
        self.uid_map.insert(res_path.to_string(), uid);
        self.uid_reverse.insert(uid, res_path.to_string());
        uid
    }

    /// Looks up the UID for a `res://` path.
    pub fn get_uid(&self, res_path: &str) -> Option<ResourceUid> {
        self.uid_map.get(res_path).copied()
    }

    /// Looks up the `res://` path for a UID.
    pub fn get_path_for_uid(&self, uid: ResourceUid) -> Option<&str> {
        self.uid_reverse.get(&uid).map(|s| s.as_str())
    }

    /// Returns the total number of UID mappings.
    pub fn uid_count(&self) -> usize {
        self.uid_map.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn make_project() -> (TempDir, EditorFileSystem) {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("project.godot"), "").unwrap();
        std::fs::create_dir_all(dir.path().join("scenes")).unwrap();
        std::fs::write(dir.path().join("scenes/main.tscn"), "[gd_scene]").unwrap();
        std::fs::write(dir.path().join("scenes/player.tscn"), "[gd_scene]").unwrap();
        std::fs::write(dir.path().join("icon.png"), "PNG").unwrap();
        let fs = EditorFileSystem::new(dir.path());
        (dir, fs)
    }

    #[test]
    fn scan_finds_files() {
        let (_dir, mut fs) = make_project();
        let count = fs.scan().unwrap();
        assert!(count >= 4); // project.godot, 2 tscn, icon.png
    }

    #[test]
    fn files_by_extension() {
        let (_dir, mut fs) = make_project();
        fs.scan().unwrap();
        let tscn_files = fs.files_by_extension("tscn");
        assert_eq!(tscn_files.len(), 2);
    }

    #[test]
    fn resolve_res_path() {
        let (dir, fs) = make_project();
        let resolved = fs.resolve_res_path("res://scenes/main.tscn").unwrap();
        assert_eq!(resolved, dir.path().join("scenes/main.tscn"));
    }

    #[test]
    fn resolve_invalid_path_returns_none() {
        let (_dir, fs) = make_project();
        assert!(fs.resolve_res_path("invalid://path").is_none());
    }

    #[test]
    fn to_res_path() {
        let (dir, fs) = make_project();
        let abs = dir.path().join("scenes/main.tscn");
        let res = fs.to_res_path(&abs).unwrap();
        assert_eq!(res, "res://scenes/main.tscn");
    }

    #[test]
    fn file_exists() {
        let (_dir, fs) = make_project();
        assert!(fs.file_exists("res://icon.png"));
        assert!(!fs.file_exists("res://nonexistent.txt"));
    }

    #[test]
    fn get_modified_time() {
        let (_dir, fs) = make_project();
        let time = fs.get_modified_time("res://icon.png");
        assert!(time.is_ok());
    }

    #[test]
    fn uid_assign_and_lookup() {
        let (_dir, mut fs) = make_project();
        let uid = fs.assign_uid("res://scenes/main.tscn");
        assert!(uid.is_valid());

        // Same path returns same UID.
        let uid2 = fs.assign_uid("res://scenes/main.tscn");
        assert_eq!(uid, uid2);

        assert_eq!(fs.get_uid("res://scenes/main.tscn"), Some(uid));
        assert_eq!(fs.get_path_for_uid(uid), Some("res://scenes/main.tscn"));
    }

    #[test]
    fn uid_count() {
        let (_dir, mut fs) = make_project();
        assert_eq!(fs.uid_count(), 0);
        fs.assign_uid("res://a.tscn");
        fs.assign_uid("res://b.tres");
        assert_eq!(fs.uid_count(), 2);
    }

    #[test]
    fn unknown_uid_returns_none() {
        let (_dir, fs) = make_project();
        assert!(fs.get_uid("res://nope").is_none());
        assert!(fs.get_path_for_uid(ResourceUid::new(999)).is_none());
    }
}
