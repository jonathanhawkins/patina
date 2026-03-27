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

// ---------------------------------------------------------------------------
// FileSystem Dock — file browser with icons, filter, and favorites
// ---------------------------------------------------------------------------

/// Icon type for a file or directory in the filesystem dock.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileIcon {
    Directory,
    Scene,
    Script,
    Resource,
    Texture,
    Audio,
    Shader,
    Font,
    Config,
    Unknown,
}

impl FileIcon {
    /// Determine the icon for a file based on its extension.
    pub fn from_extension(ext: &str) -> Self {
        match ext.to_ascii_lowercase().as_str() {
            "tscn" | "scn" => FileIcon::Scene,
            "gd" | "gdscript" => FileIcon::Script,
            "tres" | "res" => FileIcon::Resource,
            "png" | "jpg" | "jpeg" | "bmp" | "svg" | "webp" | "tga" | "exr" | "hdr" => {
                FileIcon::Texture
            }
            "wav" | "ogg" | "mp3" | "opus" | "flac" => FileIcon::Audio,
            "gdshader" | "shader" | "glsl" => FileIcon::Shader,
            "ttf" | "otf" | "woff" | "woff2" | "fnt" => FileIcon::Font,
            "cfg" | "godot" | "import" | "json" | "toml" | "yaml" | "yml" => FileIcon::Config,
            _ => FileIcon::Unknown,
        }
    }
}

/// An entry in the filesystem dock tree.
#[derive(Debug, Clone)]
pub struct FileSystemEntry {
    /// File or directory name.
    pub name: String,
    /// Path relative to project root (e.g. "scenes/main.tscn").
    pub relative_path: String,
    /// Corresponding res:// path.
    pub res_path: String,
    /// Whether this is a directory.
    pub is_directory: bool,
    /// Icon for this entry.
    pub icon: FileIcon,
    /// Nesting depth (0 for top-level items).
    pub depth: usize,
    /// Whether this entry is expanded (directories only).
    pub expanded: bool,
    /// Number of children (directories only).
    pub child_count: usize,
}

/// The filesystem dock panel, providing a file browser for the editor.
///
/// Mirrors Godot 4.x's FileSystem dock with:
/// - Directory tree view with expand/collapse
/// - File type icons
/// - Text filter/search
/// - Favorites list
#[derive(Debug)]
pub struct FileSystemDock {
    /// The underlying filesystem.
    filesystem: EditorFileSystem,
    /// Flattened tree entries.
    entries: Vec<FileSystemEntry>,
    /// Current filter text (empty = show all).
    filter: String,
    /// Favorite paths (res:// format).
    favorites: Vec<String>,
    /// Currently selected entry index.
    selected_index: Option<usize>,
    /// Set of expanded directory paths.
    expanded_dirs: std::collections::HashSet<String>,
}

impl FileSystemDock {
    /// Create a new filesystem dock wrapping an EditorFileSystem.
    pub fn new(filesystem: EditorFileSystem) -> Self {
        Self {
            filesystem,
            entries: Vec::new(),
            filter: String::new(),
            favorites: Vec::new(),
            selected_index: None,
            expanded_dirs: std::collections::HashSet::new(),
        }
    }

    /// Returns the underlying filesystem.
    pub fn filesystem(&self) -> &EditorFileSystem {
        &self.filesystem
    }

    /// Returns a mutable reference to the underlying filesystem.
    pub fn filesystem_mut(&mut self) -> &mut EditorFileSystem {
        &mut self.filesystem
    }

    /// Returns the current entries (after filtering).
    pub fn entries(&self) -> &[FileSystemEntry] {
        &self.entries
    }

    /// Returns the current filter text.
    pub fn filter(&self) -> &str {
        &self.filter
    }

    /// Set the filter text and rebuild entries.
    pub fn set_filter(&mut self, filter: impl Into<String>) {
        self.filter = filter.into();
        self.rebuild_entries();
    }

    /// Clear the filter.
    pub fn clear_filter(&mut self) {
        self.filter.clear();
        self.rebuild_entries();
    }

    /// Returns the favorites list.
    pub fn favorites(&self) -> &[String] {
        &self.favorites
    }

    /// Add a path to favorites. Returns false if already a favorite.
    pub fn add_favorite(&mut self, res_path: impl Into<String>) -> bool {
        let path = res_path.into();
        if self.favorites.contains(&path) {
            return false;
        }
        self.favorites.push(path);
        true
    }

    /// Remove a path from favorites. Returns false if not found.
    pub fn remove_favorite(&mut self, res_path: &str) -> bool {
        let before = self.favorites.len();
        self.favorites.retain(|f| f != res_path);
        self.favorites.len() < before
    }

    /// Check if a path is a favorite.
    pub fn is_favorite(&self, res_path: &str) -> bool {
        self.favorites.iter().any(|f| f == res_path)
    }

    /// Returns the selected entry index.
    pub fn selected_index(&self) -> Option<usize> {
        self.selected_index
    }

    /// Returns the selected entry.
    pub fn selected_entry(&self) -> Option<&FileSystemEntry> {
        self.selected_index.and_then(|i| self.entries.get(i))
    }

    /// Select an entry by index.
    pub fn select(&mut self, index: usize) -> bool {
        if index < self.entries.len() {
            self.selected_index = Some(index);
            true
        } else {
            false
        }
    }

    /// Clear selection.
    pub fn deselect(&mut self) {
        self.selected_index = None;
    }

    /// Toggle expansion of a directory entry by index.
    pub fn toggle_expand(&mut self, index: usize) {
        if let Some(entry) = self.entries.get(index) {
            if entry.is_directory {
                let path = entry.relative_path.clone();
                if self.expanded_dirs.contains(&path) {
                    self.expanded_dirs.remove(&path);
                } else {
                    self.expanded_dirs.insert(path);
                }
                self.rebuild_entries();
            }
        }
    }

    /// Expand a directory by relative path.
    pub fn expand_dir(&mut self, relative_path: &str) {
        if self.expanded_dirs.insert(relative_path.to_string()) {
            self.rebuild_entries();
        }
    }

    /// Collapse a directory by relative path.
    pub fn collapse_dir(&mut self, relative_path: &str) {
        if self.expanded_dirs.remove(relative_path) {
            self.rebuild_entries();
        }
    }

    /// Scan the filesystem and rebuild the tree entries.
    pub fn refresh(&mut self) -> EngineResult<usize> {
        let count = self.filesystem.scan()?;
        // Auto-expand root level.
        self.expanded_dirs.insert(String::new());
        self.rebuild_entries();
        Ok(count)
    }

    /// Rebuild entries from the cached file list, applying filter and expansion.
    fn rebuild_entries(&mut self) {
        self.entries.clear();

        let files = self.filesystem.files().to_vec();
        let filter_lower = self.filter.to_ascii_lowercase();

        // Collect unique directories and files.
        let mut dir_set: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();

        for file_path in &files {
            let path_str = file_path.display().to_string();

            // Apply filter.
            if !filter_lower.is_empty()
                && !path_str.to_ascii_lowercase().contains(&filter_lower)
            {
                continue;
            }

            // Collect parent directories.
            if let Some(parent) = file_path.parent() {
                let mut current = PathBuf::new();
                for component in parent.components() {
                    current.push(component);
                    dir_set.insert(current.display().to_string());
                }
            }
        }

        // Build a sorted flat list: directories first, then files within each dir.
        // Only show items whose parent is expanded.
        let mut visible_items: Vec<FileSystemEntry> = Vec::new();

        // Add directories.
        for dir_path in &dir_set {
            let depth = dir_path.matches('/').count()
                + if dir_path.is_empty() { 0 } else { 1 };

            // Check if parent is expanded.
            let parent = if let Some(idx) = dir_path.rfind('/') {
                &dir_path[..idx]
            } else {
                ""
            };

            if depth > 0 && !self.expanded_dirs.contains(parent) {
                continue;
            }

            // When filter is active, show all matching dirs.
            if !filter_lower.is_empty() || self.expanded_dirs.contains(parent) || depth == 0 {
                let name = dir_path
                    .rsplit('/')
                    .next()
                    .unwrap_or(dir_path)
                    .to_string();
                let child_count = files
                    .iter()
                    .filter(|f| {
                        f.parent()
                            .map(|p| p.display().to_string() == *dir_path)
                            .unwrap_or(false)
                    })
                    .count();

                visible_items.push(FileSystemEntry {
                    name,
                    relative_path: dir_path.clone(),
                    res_path: format!("res://{dir_path}/"),
                    is_directory: true,
                    icon: FileIcon::Directory,
                    depth,
                    expanded: self.expanded_dirs.contains(dir_path),
                    child_count,
                });
            }
        }

        // Add files.
        for file_path in &files {
            let path_str = file_path.display().to_string();

            // Apply filter.
            if !filter_lower.is_empty()
                && !path_str.to_ascii_lowercase().contains(&filter_lower)
            {
                continue;
            }

            let parent_str = file_path
                .parent()
                .map(|p| p.display().to_string())
                .unwrap_or_default();

            // Check if parent directory is expanded (or filter is active).
            if filter_lower.is_empty() && !self.expanded_dirs.contains(&parent_str) {
                continue;
            }

            let depth = path_str.matches('/').count()
                + if path_str.contains('/') { 0 } else { 0 }
                + if parent_str.is_empty() { 0 } else { 1 };

            let name = file_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("")
                .to_string();

            let ext = file_path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("");
            let icon = FileIcon::from_extension(ext);

            visible_items.push(FileSystemEntry {
                name,
                relative_path: path_str,
                res_path: format!("res://{}", file_path.display()),
                is_directory: false,
                icon,
                depth,
                expanded: false,
                child_count: 0,
            });
        }

        // Sort: directories before files at each level, alphabetical within.
        visible_items.sort_by(|a, b| {
            a.relative_path.cmp(&b.relative_path)
        });

        self.entries = visible_items;
    }

    /// Find an entry by res:// path.
    pub fn find_entry(&self, res_path: &str) -> Option<usize> {
        self.entries.iter().position(|e| e.res_path == res_path)
    }

    /// Navigate to a specific file, expanding parent directories as needed.
    pub fn navigate_to(&mut self, res_path: &str) {
        // Extract relative path from res://.
        let relative = res_path.strip_prefix("res://").unwrap_or(res_path);

        // Expand all parent directories.
        let path = Path::new(relative);
        let mut current = PathBuf::new();
        if let Some(parent) = path.parent() {
            for component in parent.components() {
                current.push(component);
                self.expanded_dirs.insert(current.display().to_string());
            }
        }

        self.rebuild_entries();

        // Select the target file.
        if let Some(idx) = self.find_entry(res_path) {
            self.selected_index = Some(idx);
        }
    }

    /// Returns entries matching a specific icon type.
    pub fn entries_by_type(&self, icon: FileIcon) -> Vec<&FileSystemEntry> {
        self.entries
            .iter()
            .filter(|e| e.icon == icon)
            .collect()
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

    // -- FileIcon tests --

    #[test]
    fn file_icon_from_extension() {
        assert_eq!(FileIcon::from_extension("tscn"), FileIcon::Scene);
        assert_eq!(FileIcon::from_extension("gd"), FileIcon::Script);
        assert_eq!(FileIcon::from_extension("tres"), FileIcon::Resource);
        assert_eq!(FileIcon::from_extension("png"), FileIcon::Texture);
        assert_eq!(FileIcon::from_extension("wav"), FileIcon::Audio);
        assert_eq!(FileIcon::from_extension("gdshader"), FileIcon::Shader);
        assert_eq!(FileIcon::from_extension("ttf"), FileIcon::Font);
        assert_eq!(FileIcon::from_extension("godot"), FileIcon::Config);
        assert_eq!(FileIcon::from_extension("xyz"), FileIcon::Unknown);
    }

    #[test]
    fn file_icon_case_insensitive() {
        assert_eq!(FileIcon::from_extension("TSCN"), FileIcon::Scene);
        assert_eq!(FileIcon::from_extension("PNG"), FileIcon::Texture);
        assert_eq!(FileIcon::from_extension("Gd"), FileIcon::Script);
    }

    // -- FileSystemDock tests --

    fn make_dock() -> (TempDir, FileSystemDock) {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("project.godot"), "").unwrap();
        std::fs::create_dir_all(dir.path().join("scenes")).unwrap();
        std::fs::write(dir.path().join("scenes/main.tscn"), "[gd_scene]").unwrap();
        std::fs::write(dir.path().join("scenes/player.tscn"), "[gd_scene]").unwrap();
        std::fs::create_dir_all(dir.path().join("scripts")).unwrap();
        std::fs::write(dir.path().join("scripts/main.gd"), "extends Node").unwrap();
        std::fs::write(dir.path().join("icon.png"), "PNG").unwrap();
        let fs = EditorFileSystem::new(dir.path());
        let dock = FileSystemDock::new(fs);
        (dir, dock)
    }

    #[test]
    fn dock_refresh_populates_entries() {
        let (_dir, mut dock) = make_dock();
        let count = dock.refresh().unwrap();
        assert!(count >= 4);
        assert!(!dock.entries().is_empty());
    }

    #[test]
    fn dock_entries_include_directories() {
        let (_dir, mut dock) = make_dock();
        dock.refresh().unwrap();
        let dirs: Vec<_> = dock.entries().iter().filter(|e| e.is_directory).collect();
        assert!(dirs.len() >= 2); // scenes, scripts
    }

    #[test]
    fn dock_entries_have_correct_icons() {
        let (_dir, mut dock) = make_dock();
        dock.refresh().unwrap();
        // Expand all dirs to see files.
        dock.expand_dir("scenes");
        dock.expand_dir("scripts");

        let scenes = dock.entries_by_type(FileIcon::Scene);
        assert!(scenes.len() >= 2); // main.tscn, player.tscn

        let scripts = dock.entries_by_type(FileIcon::Script);
        assert!(scripts.len() >= 1); // main.gd
    }

    #[test]
    fn dock_filter() {
        let (_dir, mut dock) = make_dock();
        dock.refresh().unwrap();
        dock.expand_dir("scenes");
        dock.expand_dir("scripts");

        let total = dock.entries().len();
        dock.set_filter("main");
        assert!(dock.entries().len() < total);
        // Should find main.tscn and main.gd.
        let names: Vec<&str> = dock
            .entries()
            .iter()
            .filter(|e| !e.is_directory)
            .map(|e| e.name.as_str())
            .collect();
        assert!(names.contains(&"main.tscn") || names.contains(&"main.gd"));

        dock.clear_filter();
        assert_eq!(dock.filter(), "");
    }

    #[test]
    fn dock_favorites() {
        let (_dir, mut dock) = make_dock();
        dock.refresh().unwrap();

        assert!(!dock.is_favorite("res://scenes/main.tscn"));
        assert!(dock.add_favorite("res://scenes/main.tscn"));
        assert!(dock.is_favorite("res://scenes/main.tscn"));
        assert_eq!(dock.favorites().len(), 1);

        // Adding again returns false.
        assert!(!dock.add_favorite("res://scenes/main.tscn"));

        assert!(dock.remove_favorite("res://scenes/main.tscn"));
        assert!(!dock.is_favorite("res://scenes/main.tscn"));
        assert!(dock.favorites().is_empty());
    }

    #[test]
    fn dock_select_and_deselect() {
        let (_dir, mut dock) = make_dock();
        dock.refresh().unwrap();

        assert!(dock.selected_index().is_none());
        assert!(dock.select(0));
        assert_eq!(dock.selected_index(), Some(0));
        assert!(dock.selected_entry().is_some());

        dock.deselect();
        assert!(dock.selected_index().is_none());

        assert!(!dock.select(9999));
    }

    #[test]
    fn dock_expand_collapse() {
        let (_dir, mut dock) = make_dock();
        dock.refresh().unwrap();

        let entries_before = dock.entries().len();
        dock.expand_dir("scenes");
        let entries_after = dock.entries().len();
        assert!(entries_after > entries_before);

        dock.collapse_dir("scenes");
        let entries_collapsed = dock.entries().len();
        assert!(entries_collapsed < entries_after);
    }

    #[test]
    fn dock_navigate_to_expands_parents() {
        let (_dir, mut dock) = make_dock();
        dock.refresh().unwrap();

        dock.navigate_to("res://scenes/main.tscn");
        // The scenes dir should now be expanded.
        let entry = dock.selected_entry();
        assert!(entry.is_some());
        if let Some(e) = entry {
            assert_eq!(e.name, "main.tscn");
        }
    }

    #[test]
    fn dock_find_entry() {
        let (_dir, mut dock) = make_dock();
        dock.refresh().unwrap();
        dock.expand_dir("scenes");

        let idx = dock.find_entry("res://scenes/main.tscn");
        assert!(idx.is_some());
    }

    #[test]
    fn dock_directory_entry_child_count() {
        let (_dir, mut dock) = make_dock();
        dock.refresh().unwrap();

        let scenes_dir = dock
            .entries()
            .iter()
            .find(|e| e.is_directory && e.name == "scenes");
        assert!(scenes_dir.is_some());
        if let Some(d) = scenes_dir {
            assert_eq!(d.child_count, 2); // main.tscn, player.tscn
        }
    }

    #[test]
    fn dock_file_entry_has_res_path() {
        let (_dir, mut dock) = make_dock();
        dock.refresh().unwrap();
        dock.expand_dir("scenes");

        let main = dock
            .entries()
            .iter()
            .find(|e| e.name == "main.tscn");
        assert!(main.is_some());
        assert_eq!(main.unwrap().res_path, "res://scenes/main.tscn");
    }
}
