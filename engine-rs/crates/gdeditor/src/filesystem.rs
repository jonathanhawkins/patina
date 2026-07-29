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

    /// Lists the immediate children of a `res://` directory (pat-0q2kx.167).
    ///
    /// This is what a script walks: `EditorFileSystem.get_filesystem()` hands
    /// back a directory and the script descends it with `get_subdir` /
    /// `get_file_count` / `get_file_path`
    /// (`upstream/godot/editor/file_system/editor_file_system.cpp`).
    ///
    /// ⚠️ This returned `Ok(Vec::new())` unconditionally, ignoring its own
    /// argument. Every caller therefore saw a project with no files in it and
    /// no error — the asset browser rendered empty and a directory walk
    /// terminated immediately, which looks like "the project is empty" rather
    /// than like a missing implementation.
    ///
    /// Ordering and filtering follow Godot's dock:
    ///
    /// * directories first, then files, each alphabetically — Godot groups
    ///   folders above files rather than interleaving them;
    /// * dot-entries are skipped, matching [`scan_dir`](Self::scan_dir), so
    ///   `.godot/` and `.import/` do not appear as project content. Skipping
    ///   them here and not there (or vice versa) would make `list_dir` and
    ///   [`files`](Self::files) disagree about what the project contains.
    ///
    /// A path that is missing, or is a file rather than a directory, is an
    /// error rather than an empty list: "no children" and "not a directory"
    /// are different answers, and collapsing them is what made the stub
    /// invisible.
    pub fn list_dir(&self, res_path: &str) -> EngineResult<Vec<DirChild>> {
        let dir = self.resolve_res_path(res_path).ok_or_else(|| {
            EngineError::InvalidOperation(format!(
                "list_dir expects a res:// path, got '{res_path}'"
            ))
        })?;
        if !dir.is_dir() {
            return Err(EngineError::InvalidOperation(format!(
                "'{res_path}' is not a directory"
            )));
        }

        let mut children = Vec::new();
        for entry in std::fs::read_dir(&dir).map_err(EngineError::Io)? {
            let entry = entry.map_err(EngineError::Io)?;
            let path = entry.path();
            let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                // A non-UTF-8 name has no `res://` spelling, so a script could
                // not address it anyway.
                continue;
            };
            if name.starts_with('.') {
                continue;
            }
            let is_directory = path.is_dir();
            let icon = if is_directory {
                FileIcon::Directory
            } else {
                FileIcon::from_extension(path.extension().and_then(|e| e.to_str()).unwrap_or(""))
            };
            let Some(child_res_path) = self.to_res_path(&path) else {
                continue;
            };
            children.push(DirChild {
                name: name.to_string(),
                is_directory,
                icon,
                res_path: child_res_path,
            });
        }

        // `read_dir` order is filesystem-defined and differs between machines,
        // so sort explicitly — otherwise the dock's row order (and any test
        // asserting it) is non-deterministic.
        children.sort_by(|a, b| {
            b.is_directory
                .cmp(&a.is_directory)
                .then_with(|| a.name.cmp(&b.name))
        });
        Ok(children)
    }
}

/// A direct child entry of a directory, surfaced to the asset-browser panel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirChild {
    /// File or directory name (no path component).
    pub name: String,
    /// `true` if this entry is a directory.
    pub is_directory: bool,
    /// Icon derived from the file's extension (or `Directory`).
    pub icon: FileIcon,
    /// Full `res://` path to this entry.
    pub res_path: String,
}

// ---------------------------------------------------------------------------
// FileSystem Dock — file browser with icons, filter, and favorites
// ---------------------------------------------------------------------------

/// An action offered by the filesystem dock's file/folder context menu.
///
/// Mirrors Godot's `FileSystemDock` `tree_popup`/`file_list_popup`
/// (upstream/godot/editor/docks/filesystem_dock.cpp `_file_and_folders_fill_popup`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileContextAction {
    /// Open the file in its default editor / instance the scene (folders: expand).
    Open,
    /// Open the dependency editor for the file (`FILE_MENU_DEPENDENCIES`).
    EditDependencies,
    /// Show which resources reference (own) this file (`FILE_MENU_OWNERS`).
    ViewOwners,
    /// Copy the `res://` path to the clipboard (`FILE_MENU_COPY_PATH`).
    CopyPath,
    /// Copy the absolute on-disk path (`FILE_MENU_COPY_ABSOLUTE_PATH`).
    CopyAbsolutePath,
    /// Copy the resource UID (`FILE_MENU_COPY_UID`). Files only.
    CopyUid,
    /// Rename the file/folder (`FILE_MENU_RENAME`).
    Rename,
    /// Duplicate the file/folder (`FILE_MENU_DUPLICATE`).
    Duplicate,
    /// Delete the file/folder (`FILE_MENU_REMOVE`).
    Delete,
    /// Reveal the file in the OS file manager (`FILE_MENU_SHOW_IN_EXPLORER`).
    ShowInFileManager,
    /// Create New ▸ (Folder/Scene/Script/Resource) submenu (`FILE_MENU_NEW`).
    New,
}

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
/// The dock's split display mode, mirroring Godot's
/// `FileSystemDock::DisplayMode` (upstream/godot/editor/docks/filesystem_dock.h):
/// a single combined tree, or a split with a separate file list laid out
/// vertically (file list below the tree) or horizontally (beside it).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FsDisplayMode {
    /// One combined tree; no separate file list (`DISPLAY_MODE_TREE_ONLY`).
    TreeOnly,
    /// Tree on top, file list below (`DISPLAY_MODE_VSPLIT`).
    VSplit,
    /// Tree beside file list (`DISPLAY_MODE_HSPLIT`).
    HSplit,
}

impl FsDisplayMode {
    /// Stable wire/persistence token.
    pub fn as_str(&self) -> &'static str {
        match self {
            FsDisplayMode::TreeOnly => "tree_only",
            FsDisplayMode::VSplit => "vsplit",
            FsDisplayMode::HSplit => "hsplit",
        }
    }

    /// Parse a token back to a mode, defaulting to `TreeOnly` for anything
    /// unrecognized.
    pub fn from_str(s: &str) -> Self {
        match s {
            "vsplit" => FsDisplayMode::VSplit,
            "hsplit" => FsDisplayMode::HSplit,
            _ => FsDisplayMode::TreeOnly,
        }
    }

    /// The next mode in Godot's `_change_split_mode` cycle:
    /// TreeOnly → VSplit → HSplit → TreeOnly.
    pub fn next(&self) -> Self {
        match self {
            FsDisplayMode::TreeOnly => FsDisplayMode::VSplit,
            FsDisplayMode::VSplit => FsDisplayMode::HSplit,
            FsDisplayMode::HSplit => FsDisplayMode::TreeOnly,
        }
    }

    /// Whether this mode shows the separate file list (i.e. is a split layout).
    pub fn is_split(&self) -> bool {
        !matches!(self, FsDisplayMode::TreeOnly)
    }
}

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
    /// Tree-only vs split display layout (Godot `DisplayMode`).
    display_mode: FsDisplayMode,
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
            display_mode: FsDisplayMode::TreeOnly,
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

    /// Map a `res://` path to its on-disk location under the project root.
    /// `res://` itself maps to the project root.
    fn res_to_disk(&self, res_path: &str) -> std::path::PathBuf {
        let rel = res_path.strip_prefix("res://").unwrap_or(res_path);
        let rel = rel.trim_end_matches('/');
        if rel.is_empty() {
            self.filesystem.project_root().to_path_buf()
        } else {
            self.filesystem.project_root().join(rel)
        }
    }

    /// The file Godot persists per-project favorites to:
    /// `<project>/.godot/editor/favorites`, one `res://` path per line
    /// (upstream/godot/editor/settings/editor_settings.cpp set_favorites).
    pub fn favorites_file_path(&self) -> std::path::PathBuf {
        self.filesystem
            .project_root()
            .join(".godot")
            .join("editor")
            .join("favorites")
    }

    /// Persist the current favorites list to disk so it survives editor
    /// reloads and restarts. Mirrors Godot's `EditorSettings::set_favorites`,
    /// which writes one path per line to the project settings dir.
    pub fn save_favorites(&self) -> std::io::Result<()> {
        let path = self.favorites_file_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut body = String::new();
        for fav in &self.favorites {
            body.push_str(fav);
            body.push('\n');
        }
        std::fs::write(path, body)
    }

    /// Load favorites from disk, pruning any whose target no longer exists
    /// (matching Godot's `_update_tree`, which drops dead favorite paths and
    /// rewrites the file). `res://` is always kept. Replaces the in-memory
    /// list. Missing file is a no-op (leaves favorites empty).
    pub fn load_favorites(&mut self) {
        let path = self.favorites_file_path();
        let contents = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(_) => return,
        };
        let mut loaded = Vec::new();
        let mut pruned_any = false;
        for line in contents.lines() {
            let fav = line.trim();
            if fav.is_empty() || !fav.starts_with("res://") {
                continue;
            }
            // `res://` (the project root) is always valid; other favorites are
            // kept only while their file/dir still exists on disk.
            let keep = fav == "res://" || self.res_to_disk(fav).exists();
            if keep {
                if !loaded.contains(&fav.to_string()) {
                    loaded.push(fav.to_string());
                }
            } else {
                pruned_any = true;
            }
        }
        self.favorites = loaded;
        // Rewrite the file if we dropped dead entries, as Godot does.
        if pruned_any {
            let _ = self.save_favorites();
        }
    }

    /// The current split display mode.
    pub fn display_mode(&self) -> FsDisplayMode {
        self.display_mode
    }

    /// Set the split display mode and persist it.
    pub fn set_display_mode(&mut self, mode: FsDisplayMode) {
        self.display_mode = mode;
        let _ = self.save_display_mode();
    }

    /// Advance to the next display mode in Godot's cycle
    /// (TreeOnly → VSplit → HSplit → TreeOnly) and persist. Returns the new
    /// mode. Mirrors `FileSystemDock::_change_split_mode`.
    pub fn cycle_display_mode(&mut self) -> FsDisplayMode {
        self.set_display_mode(self.display_mode.next());
        self.display_mode
    }

    /// File the dock's split display mode is persisted to, alongside favorites.
    fn display_mode_file_path(&self) -> std::path::PathBuf {
        self.filesystem
            .project_root()
            .join(".godot")
            .join("editor")
            .join("fs_display_mode")
    }

    /// Persist the display mode so it survives editor reloads/restarts.
    pub fn save_display_mode(&self) -> std::io::Result<()> {
        let path = self.display_mode_file_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, self.display_mode.as_str())
    }

    /// Load the persisted display mode; missing file leaves the default.
    pub fn load_display_mode(&mut self) {
        if let Ok(s) = std::fs::read_to_string(self.display_mode_file_path()) {
            self.display_mode = FsDisplayMode::from_str(s.trim());
        }
    }

    /// The file context-menu actions available for the currently selected
    /// entry. Files expose the full set; directories expose only the actions
    /// that apply to them (open / show in file manager). Returns an empty list
    /// when nothing is selected.
    pub fn context_menu_actions(&self) -> Vec<FileContextAction> {
        use FileContextAction::*;
        match self.selected_entry() {
            None => Vec::new(),
            // Folders: no Edit Dependencies / View Owners / Copy UID (those are
            // resource-only in Godot), but everything else applies.
            Some(entry) if entry.is_directory => vec![
                Open,
                CopyPath,
                CopyAbsolutePath,
                Rename,
                Duplicate,
                Delete,
                ShowInFileManager,
                New,
            ],
            // Files: the full Godot menu.
            Some(_) => vec![
                Open,
                EditDependencies,
                ViewOwners,
                CopyPath,
                CopyAbsolutePath,
                CopyUid,
                Rename,
                Duplicate,
                Delete,
                ShowInFileManager,
                New,
            ],
        }
    }

    /// The res:// path the context-menu actions operate on (the selected
    /// entry's path), or `None` when nothing is selected.
    pub fn context_action_target(&self) -> Option<&str> {
        self.selected_entry().map(|e| e.res_path.as_str())
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
        // Restore persisted favorites so reopening a project (or reloading the
        // editor) pins them under the Favorites header again. pat-k6600.
        self.load_favorites();
        // Restore the persisted split display mode (pat-96yv4).
        self.load_display_mode();
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
            if !filter_lower.is_empty() && !path_str.to_ascii_lowercase().contains(&filter_lower) {
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
            let depth = dir_path.matches('/').count() + if dir_path.is_empty() { 0 } else { 1 };

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
                let name = dir_path.rsplit('/').next().unwrap_or(dir_path).to_string();
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
            if !filter_lower.is_empty() && !path_str.to_ascii_lowercase().contains(&filter_lower) {
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

            let ext = file_path.extension().and_then(|e| e.to_str()).unwrap_or("");
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
        visible_items.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));

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
        self.entries.iter().filter(|e| e.icon == icon).collect()
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

    // -- list_dir (pat-0q2kx.167) -------------------------------------------
    //
    // The directory walk a `@tool` script performs through
    // `EditorFileSystem.get_filesystem()`. `list_dir` previously returned an
    // empty Vec for every input, so the walk terminated at the root and the
    // project looked empty rather than unimplemented.

    /// The root lists its immediate children — and ONLY its immediate
    /// children. A recursive result would make a script's own descent
    /// re-visit everything it already saw.
    #[test]
    fn list_dir_returns_immediate_children_only() {
        let (_dir, fs) = make_project();
        let children = fs.list_dir("res://").expect("root is a directory");

        let names: Vec<&str> = children.iter().map(|c| c.name.as_str()).collect();
        assert!(names.contains(&"scenes"), "got {names:?}");
        assert!(names.contains(&"icon.png"), "got {names:?}");
        assert!(
            !names.contains(&"main.tscn"),
            "main.tscn is nested inside scenes/ and must not appear at the root: {names:?}"
        );
    }

    /// ⚠️ The regression this exists to catch: an empty result is what the
    /// stub returned, and it is indistinguishable from an empty project
    /// unless something asserts real content came back.
    #[test]
    fn list_dir_is_not_empty_for_a_populated_project() {
        let (_dir, fs) = make_project();
        assert!(
            !fs.list_dir("res://").unwrap().is_empty(),
            "a project with files must not list as empty"
        );
    }

    /// Godot's dock groups folders above files; `read_dir` order is
    /// filesystem-defined, so this must be explicit or row order varies by
    /// machine.
    #[test]
    fn list_dir_sorts_directories_first_then_alphabetically() {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join("zeta")).unwrap();
        std::fs::create_dir_all(dir.path().join("alpha")).unwrap();
        std::fs::write(dir.path().join("b.gd"), "").unwrap();
        std::fs::write(dir.path().join("a.gd"), "").unwrap();
        let fs = EditorFileSystem::new(dir.path());

        let names: Vec<String> = fs
            .list_dir("res://")
            .unwrap()
            .into_iter()
            .map(|c| c.name)
            .collect();
        assert_eq!(names, vec!["alpha", "zeta", "a.gd", "b.gd"]);
    }

    /// Dot-entries are skipped, and skipped the SAME way `scan` skips them —
    /// otherwise `list_dir` and `files()` disagree about the project's
    /// contents.
    #[test]
    fn list_dir_skips_dot_entries_like_scan_does() {
        let (dir, mut fs) = make_project();
        std::fs::create_dir_all(dir.path().join(".godot")).unwrap();
        std::fs::write(dir.path().join(".gitignore"), "").unwrap();

        let names: Vec<String> = fs
            .list_dir("res://")
            .unwrap()
            .into_iter()
            .map(|c| c.name)
            .collect();
        assert!(!names.iter().any(|n| n.starts_with('.')), "got {names:?}");

        fs.scan().unwrap();
        assert!(
            !fs.files().iter().any(|p| p.starts_with(".godot")),
            "scan must agree with list_dir about dot-entries"
        );
    }

    /// Children carry a usable `res://` path and a type-derived icon — the two
    /// fields a script and the dock actually consume.
    #[test]
    fn list_dir_children_carry_res_paths_and_icons() {
        let (_dir, fs) = make_project();
        let children = fs.list_dir("res://").unwrap();

        let scenes = children.iter().find(|c| c.name == "scenes").unwrap();
        assert!(scenes.is_directory);
        assert_eq!(scenes.icon, FileIcon::Directory);
        assert_eq!(scenes.res_path, "res://scenes");

        let icon = children.iter().find(|c| c.name == "icon.png").unwrap();
        assert!(!icon.is_directory);
        assert_eq!(icon.icon, FileIcon::Texture);
        assert_eq!(icon.res_path, "res://icon.png");

        // And that res:// path round-trips back into a listable directory.
        let nested: Vec<String> = fs
            .list_dir(&scenes.res_path)
            .unwrap()
            .into_iter()
            .map(|c| c.name)
            .collect();
        assert_eq!(nested, vec!["main.tscn", "player.tscn"]);
    }

    /// "Not a directory" and "no children" are different answers; the stub
    /// collapsed both into an empty Vec, which is why it went unnoticed.
    #[test]
    fn list_dir_errors_on_a_file_or_a_missing_path() {
        let (_dir, fs) = make_project();
        assert!(
            fs.list_dir("res://icon.png").is_err(),
            "a file is not a directory"
        );
        assert!(
            fs.list_dir("res://nope/").is_err(),
            "missing path must error"
        );
        assert!(
            fs.list_dir("/etc").is_err(),
            "a non-res:// path has no project-relative meaning"
        );
    }

    /// An empty directory lists empty *successfully* — the one case where an
    /// empty Vec is the right answer.
    #[test]
    fn list_dir_on_an_empty_directory_succeeds_with_no_children() {
        let (dir, fs) = make_project();
        std::fs::create_dir_all(dir.path().join("empty")).unwrap();
        assert!(fs.list_dir("res://empty").unwrap().is_empty());
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

    /// Acceptance (pat-a4uyc): typing in the dock filter narrows the listing to
    /// name matches across the current scope, and clearing it restores the full
    /// view.
    #[test]
    fn fs_dock_search_filter_narrows_listing() {
        let (_dir, mut dock) = make_dock();
        dock.refresh().unwrap();
        dock.expand_dir("scenes");
        dock.expand_dir("scripts");

        let full = dock.entries().len();
        assert!(full > 0);

        // Typing a query narrows the listing to matching files.
        dock.set_filter("player");
        let narrowed = dock.entries().len();
        assert!(narrowed < full);
        // The matching file is present...
        assert!(dock
            .entries()
            .iter()
            .any(|e| !e.is_directory && e.name == "player.tscn"));
        // ...and a non-matching file is excluded.
        assert!(!dock
            .entries()
            .iter()
            .any(|e| !e.is_directory && e.name == "icon.png"));

        // A query that matches nothing leaves no file entries.
        dock.set_filter("zzzznomatch");
        assert!(dock.entries().iter().all(|e| e.is_directory));

        // Clearing the filter restores the full view.
        dock.clear_filter();
        assert_eq!(dock.filter(), "");
        assert_eq!(dock.entries().len(), full);
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

    /// Acceptance (pat-es9i0): expanding/selecting a folder in the tree lists its
    /// files and subfolders, and the view tracks the on-disk `res://` contents.
    #[test]
    fn fs_dock_browser_lists_directory_contents() {
        let (dir, mut dock) = make_dock();
        dock.refresh().unwrap();

        // The tree lists the project's top-level folders.
        let top_names: Vec<&str> = dock.entries().iter().map(|e| e.name.as_str()).collect();
        assert!(top_names.contains(&"scenes"));
        assert!(top_names.contains(&"scripts"));

        // Expanding a folder lists the files it contains.
        dock.expand_dir("scenes");
        let scene_files: Vec<&str> = dock
            .entries()
            .iter()
            .filter(|e| !e.is_directory)
            .map(|e| e.res_path.as_str())
            .collect();
        assert!(scene_files.contains(&"res://scenes/main.tscn"));
        assert!(scene_files.contains(&"res://scenes/player.tscn"));

        // The view tracks on-disk `res://` contents: a file added on disk shows
        // up in the folder after a refresh.
        std::fs::write(dir.path().join("scenes/extra.tscn"), "[gd_scene]").unwrap();
        dock.refresh().unwrap();
        dock.expand_dir("scenes");
        assert!(dock
            .entries()
            .iter()
            .any(|e| e.res_path == "res://scenes/extra.tscn"));
    }

    /// Acceptance (pat-ai5tb): favoriting a directory pins it to the Favorites
    /// section, and the file context menu's actions operate on the selected
    /// file.
    #[test]
    fn fs_dock_favorites_and_context_menu() {
        let (_dir, mut dock) = make_dock();
        dock.refresh().unwrap();
        dock.expand_dir("scenes");

        // Favoriting a directory pins it to the Favorites section.
        assert!(dock.add_favorite("res://scenes"));
        assert!(dock.favorites().contains(&"res://scenes".to_string()));
        assert!(dock.is_favorite("res://scenes"));

        // Selecting a file exposes the full context-menu action set.
        let idx = dock.find_entry("res://scenes/main.tscn").unwrap();
        assert!(dock.select(idx));
        let actions = dock.context_menu_actions();
        assert!(actions.contains(&FileContextAction::Open));
        assert!(actions.contains(&FileContextAction::EditDependencies));
        assert!(actions.contains(&FileContextAction::ViewOwners));
        // The actions operate on the selected file.
        assert_eq!(dock.context_action_target(), Some("res://scenes/main.tscn"));

        // A selected directory exposes only the directory-applicable actions.
        let dir_idx = dock
            .entries()
            .iter()
            .position(|e| e.is_directory && e.name == "scenes")
            .unwrap();
        assert!(dock.select(dir_idx));
        let dir_actions = dock.context_menu_actions();
        assert!(dir_actions.contains(&FileContextAction::Open));
        assert!(!dir_actions.contains(&FileContextAction::ViewOwners));

        // With nothing selected there are no actions and no target.
        dock.deselect();
        assert!(dock.context_menu_actions().is_empty());
        assert_eq!(dock.context_action_target(), None);
    }

    /// pat-k6600: favorites round-trip to `<project>/.godot/editor/favorites`
    /// and reload on refresh, and dead favorites are pruned on load (matching
    /// Godot's behavior of dropping favorites whose path no longer exists).
    #[test]
    fn dock_favorites_persist_and_prune() {
        let (dir, mut dock) = make_dock();
        dock.refresh().unwrap();

        // Favorite a real dir and a real file, then persist.
        assert!(dock.add_favorite("res://scenes"));
        assert!(dock.add_favorite("res://scripts/main.gd"));
        dock.save_favorites().unwrap();

        // The file lives where Godot writes it, one path per line.
        let fav_file = dir.path().join(".godot").join("editor").join("favorites");
        let written = std::fs::read_to_string(&fav_file).unwrap();
        assert!(written.lines().any(|l| l == "res://scenes"));
        assert!(written.lines().any(|l| l == "res://scripts/main.gd"));

        // A fresh dock at the same root reloads them on refresh.
        let mut reloaded = FileSystemDock::new(EditorFileSystem::new(dir.path()));
        reloaded.refresh().unwrap();
        assert!(reloaded.is_favorite("res://scenes"));
        assert!(reloaded.is_favorite("res://scripts/main.gd"));

        // Append a favorite whose target was deleted; it is pruned on load and
        // the file is rewritten without it.
        std::fs::write(
            &fav_file,
            "res://scenes\nres://scripts/main.gd\nres://gone_dir\n",
        )
        .unwrap();
        let mut pruned = FileSystemDock::new(EditorFileSystem::new(dir.path()));
        pruned.refresh().unwrap();
        assert!(pruned.is_favorite("res://scenes"));
        assert!(
            !pruned.is_favorite("res://gone_dir"),
            "dead favorite pruned"
        );
        let rewritten = std::fs::read_to_string(&fav_file).unwrap();
        assert!(!rewritten.contains("gone_dir"), "prune rewrites the file");
    }

    /// pat-96yv4: the split display mode cycles like Godot's
    /// `_change_split_mode` (TreeOnly → VSplit → HSplit → TreeOnly) and is
    /// persisted/reloaded across a fresh dock at the same root.
    #[test]
    fn dock_display_mode_cycles_and_persists() {
        let (dir, mut dock) = make_dock();
        dock.refresh().unwrap();

        // Default is tree-only.
        assert_eq!(dock.display_mode(), FsDisplayMode::TreeOnly);

        // Godot's cycle.
        assert_eq!(dock.cycle_display_mode(), FsDisplayMode::VSplit);
        assert_eq!(dock.cycle_display_mode(), FsDisplayMode::HSplit);
        assert_eq!(dock.cycle_display_mode(), FsDisplayMode::TreeOnly);

        // Land on a split mode and confirm it's on disk.
        dock.cycle_display_mode(); // -> VSplit
        let mode_file = dir
            .path()
            .join(".godot")
            .join("editor")
            .join("fs_display_mode");
        assert_eq!(
            std::fs::read_to_string(&mode_file).unwrap().trim(),
            "vsplit"
        );

        // A fresh dock at the same root reloads the persisted mode.
        let mut reloaded = FileSystemDock::new(EditorFileSystem::new(dir.path()));
        reloaded.refresh().unwrap();
        assert_eq!(reloaded.display_mode(), FsDisplayMode::VSplit);
    }

    /// pat-ypgqq: right-clicking a file exposes Godot's full menu, and a folder
    /// exposes the same minus the resource-only items (Edit Dependencies, View
    /// Owners, Copy UID). Every action targets the selected path.
    #[test]
    fn fs_dock_context_menu_full_actions() {
        use FileContextAction::*;
        let (_dir, mut dock) = make_dock();
        dock.refresh().unwrap();
        dock.expand_dir("scenes");

        // A selected file exposes the complete menu.
        let idx = dock.find_entry("res://scenes/main.tscn").unwrap();
        assert!(dock.select(idx));
        let fa = dock.context_menu_actions();
        for expected in [
            Open,
            EditDependencies,
            ViewOwners,
            CopyPath,
            CopyAbsolutePath,
            CopyUid,
            Rename,
            Duplicate,
            Delete,
            ShowInFileManager,
            New,
        ] {
            assert!(fa.contains(&expected), "file menu missing {expected:?}");
        }
        assert_eq!(dock.context_action_target(), Some("res://scenes/main.tscn"));

        // A selected folder exposes the same set minus resource-only items.
        let dir_idx = dock
            .entries()
            .iter()
            .position(|e| e.is_directory && e.name == "scenes")
            .unwrap();
        assert!(dock.select(dir_idx));
        let da = dock.context_menu_actions();
        for expected in [
            Open,
            CopyPath,
            CopyAbsolutePath,
            Rename,
            Duplicate,
            Delete,
            ShowInFileManager,
            New,
        ] {
            assert!(da.contains(&expected), "folder menu missing {expected:?}");
        }
        for absent in [EditDependencies, ViewOwners, CopyUid] {
            assert!(
                !da.contains(&absent),
                "folder menu should not have {absent:?}"
            );
        }
        // Directory entries carry a trailing slash in their res_path.
        assert_eq!(dock.context_action_target(), Some("res://scenes/"));
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

        let main = dock.entries().iter().find(|e| e.name == "main.tscn");
        assert!(main.is_some());
        assert_eq!(main.unwrap().res_path, "res://scenes/main.tscn");
    }
}
