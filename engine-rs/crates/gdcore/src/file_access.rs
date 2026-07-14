//! Godot-compatible `FileAccess` and `DirAccess` for reading and writing files.
//!
//! Mirrors the core API surface of upstream Godot's `core/io/file_access.*` and
//! `core/io/dir_access.*` so GDScript and runtime code can open, read, write,
//! and list files through the same contracts.
//!
//! Path resolution supports plain filesystem paths plus Godot-style `res://`
//! and `user://` URIs when a project root / user data root is configured via
//! [`set_path_roots`].

use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, RwLock};

use crate::error::{EngineError, EngineResult};
use crate::id::ObjectId;

// ---------------------------------------------------------------------------
// Path roots (res:// and user://)
// ---------------------------------------------------------------------------

struct PathRoots {
    project: Option<PathBuf>,
    user: Option<PathBuf>,
}

static PATH_ROOTS: RwLock<PathRoots> = RwLock::new(PathRoots {
    project: None,
    user: None,
});

/// Configures the filesystem roots used to resolve `res://` and `user://` paths.
///
/// Passing `None` for either root clears that mapping. Callers typically set
/// the project root when loading a `project.godot` and the user root to the
/// platform-specific application data directory.
pub fn set_path_roots(project: Option<PathBuf>, user: Option<PathBuf>) {
    let mut roots = PATH_ROOTS
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    roots.project = project;
    roots.user = user;
}

/// Returns the currently configured project (`res://`) root, if any.
pub fn project_root() -> Option<PathBuf> {
    PATH_ROOTS
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .project
        .clone()
}

/// Returns the currently configured user (`user://`) data root, if any.
pub fn user_root() -> Option<PathBuf> {
    PATH_ROOTS
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .user
        .clone()
}

/// Resolves a Godot-style path (`res://`, `user://`, or plain) to a filesystem path.
///
/// # Errors
///
/// Returns [`EngineError::InvalidOperation`] when a `res://` or `user://` path is
/// used without a configured root.
pub fn resolve_path(path: &str) -> EngineResult<PathBuf> {
    if let Some(rest) = path.strip_prefix("res://") {
        let root = project_root().ok_or_else(|| {
            EngineError::InvalidOperation(
                "res:// path used but no project root configured (call set_path_roots)".into(),
            )
        })?;
        Ok(root.join(rest))
    } else if let Some(rest) = path.strip_prefix("user://") {
        let root = user_root().ok_or_else(|| {
            EngineError::InvalidOperation(
                "user:// path used but no user root configured (call set_path_roots)".into(),
            )
        })?;
        Ok(root.join(rest))
    } else {
        Ok(PathBuf::from(path))
    }
}

// ---------------------------------------------------------------------------
// FileAccess::ModeFlags (Godot-compatible bit flags)
// ---------------------------------------------------------------------------

/// File open mode flags matching Godot's `FileAccess.ModeFlags`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum FileMode {
    /// Open for reading only (`FileAccess.READ` = 1).
    Read = 1,
    /// Open for writing, truncating existing content (`FileAccess.WRITE` = 2).
    Write = 2,
    /// Open for reading and writing (`FileAccess.READ_WRITE` = 3).
    ReadWrite = 3,
    /// Open for writing and reading without truncating (`FileAccess.WRITE_READ` = 7).
    WriteRead = 7,
}

impl FileMode {
    /// Parses a Godot integer mode flag into a [`FileMode`].
    pub fn from_godot(flags: u32) -> Option<Self> {
        match flags {
            1 => Some(Self::Read),
            2 => Some(Self::Write),
            3 => Some(Self::ReadWrite),
            7 => Some(Self::WriteRead),
            _ => None,
        }
    }

    fn is_read(self) -> bool {
        matches!(self, Self::Read | Self::ReadWrite | Self::WriteRead)
    }

    fn is_write(self) -> bool {
        matches!(self, Self::Write | Self::ReadWrite | Self::WriteRead)
    }

    fn truncate(self) -> bool {
        matches!(self, Self::Write)
    }
}

// ---------------------------------------------------------------------------
// FileAccess
// ---------------------------------------------------------------------------

/// A Godot-compatible file handle for reading and writing binary or text data.
///
/// Mirrors the core of Godot's `FileAccess` class: open with a mode, read/write
/// buffers and strings, seek, and close. Errors are reported via
/// [`get_error`](Self::get_error) after operations, matching Godot's pattern of
/// not throwing on every call.
#[derive(Debug)]
pub struct FileAccess {
    path: PathBuf,
    mode: FileMode,
    file: Option<File>,
    last_error: Option<String>,
}

impl FileAccess {
    /// Opens `path` with the given [`FileMode`].
    ///
    /// `path` may be a plain filesystem path or a `res://` / `user://` URI.
    ///
    /// # Errors
    ///
    /// Returns an error when the path cannot be resolved or the OS open fails.
    pub fn open(path: &str, mode: FileMode) -> EngineResult<Self> {
        let resolved = resolve_path(path)?;
        let mut opts = OpenOptions::new();
        opts.read(mode.is_read()).write(mode.is_write());
        if mode.truncate() {
            opts.create(true).truncate(true);
        } else if mode.is_write() {
            opts.create(true);
        }

        let file = opts.open(&resolved).map_err(EngineError::from)?;
        Ok(Self {
            path: resolved,
            mode,
            file: Some(file),
            last_error: None,
        })
    }

    /// Returns `true` if a file exists at `path` (after `res://` / `user://` resolution).
    pub fn file_exists(path: &str) -> bool {
        match resolve_path(path) {
            Ok(p) => p.is_file(),
            Err(_) => false,
        }
    }

    /// Absolute filesystem path this handle is bound to.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Mode the file was opened with.
    pub fn mode(&self) -> FileMode {
        self.mode
    }

    /// Last error message, if any operation failed after open.
    pub fn get_error(&self) -> Option<&str> {
        self.last_error.as_deref()
    }

    fn with_file_mut<F, T>(&mut self, f: F) -> EngineResult<T>
    where
        F: FnOnce(&mut File) -> std::io::Result<T>,
    {
        let Some(file) = self.file.as_mut() else {
            let err = "file is closed".to_string();
            self.last_error = Some(err.clone());
            return Err(EngineError::InvalidOperation(err));
        };
        match f(file) {
            Ok(v) => {
                self.last_error = None;
                Ok(v)
            }
            Err(e) => {
                let msg = e.to_string();
                self.last_error = Some(msg.clone());
                Err(EngineError::Io(e))
            }
        }
    }

    /// Reads the entire remaining file contents as UTF-8 text.
    ///
    /// Invalid UTF-8 is lossily converted (Godot returns a string as well).
    pub fn get_as_text(&mut self) -> EngineResult<String> {
        let bytes = self.get_buffer(usize::MAX)?;
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }

    /// Reads up to `length` bytes from the current position.
    ///
    /// Passing `usize::MAX` reads until EOF.
    pub fn get_buffer(&mut self, length: usize) -> EngineResult<Vec<u8>> {
        self.with_file_mut(|file| {
            if length == usize::MAX {
                let mut buf = Vec::new();
                file.read_to_end(&mut buf)?;
                Ok(buf)
            } else {
                let mut buf = vec![0u8; length];
                let n = file.read(&mut buf)?;
                buf.truncate(n);
                Ok(buf)
            }
        })
    }

    /// Writes `text` as UTF-8 bytes at the current position.
    pub fn store_string(&mut self, text: &str) -> EngineResult<()> {
        self.store_buffer(text.as_bytes())
    }

    /// Writes raw `bytes` at the current position.
    pub fn store_buffer(&mut self, bytes: &[u8]) -> EngineResult<()> {
        self.with_file_mut(|file| {
            file.write_all(bytes)?;
            Ok(())
        })
    }

    /// Seeks to an absolute byte offset from the start of the file.
    pub fn seek(&mut self, position: u64) -> EngineResult<()> {
        self.with_file_mut(|file| {
            file.seek(SeekFrom::Start(position))?;
            Ok(())
        })
    }

    /// Returns the current byte offset from the start of the file.
    pub fn get_position(&mut self) -> EngineResult<u64> {
        self.with_file_mut(|file| file.stream_position())
    }

    /// Returns the total file length in bytes.
    pub fn get_length(&mut self) -> EngineResult<u64> {
        self.with_file_mut(|file| {
            let pos = file.stream_position()?;
            let end = file.seek(SeekFrom::End(0))?;
            file.seek(SeekFrom::Start(pos))?;
            Ok(end)
        })
    }

    /// Returns `true` when the read cursor is at or past EOF.
    pub fn eof_reached(&mut self) -> bool {
        match (self.get_position(), self.get_length()) {
            (Ok(pos), Ok(len)) => pos >= len,
            _ => true,
        }
    }

    /// Closes the file handle. Subsequent IO returns an error.
    pub fn close(&mut self) {
        self.file = None;
        self.last_error = None;
    }
}

impl Drop for FileAccess {
    fn drop(&mut self) {
        self.close();
    }
}

// ---------------------------------------------------------------------------
// Script-facing FileAccess handle registry
// ---------------------------------------------------------------------------

/// High bit tag so FileAccess handles never collide with ordinary node ObjectIds.
const FILE_HANDLE_TAG: u64 = 0xFA00_0000_0000_0000;

static NEXT_FILE_HANDLE: AtomicU64 = AtomicU64::new(1);
static FILE_HANDLES: Mutex<Option<HashMap<u64, FileAccess>>> = Mutex::new(None);

fn file_handles() -> std::sync::MutexGuard<'static, Option<HashMap<u64, FileAccess>>> {
    FILE_HANDLES
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Returns `true` when `id` was issued by [`FileAccess::open_handle`].
pub fn is_file_access_handle(id: ObjectId) -> bool {
    id.raw() & 0xFF00_0000_0000_0000 == FILE_HANDLE_TAG
}

impl FileAccess {
    /// Opens a file and stores it in the process-wide handle table.
    ///
    /// Returns an [`ObjectId`] suitable for GDScript `FileAccess.open` so
    /// subsequent instance methods can be dispatched via [`with_handle_mut`].
    pub fn open_handle(path: &str, mode: FileMode) -> EngineResult<ObjectId> {
        let fa = Self::open(path, mode)?;
        let seq = NEXT_FILE_HANDLE.fetch_add(1, Ordering::Relaxed);
        let raw = FILE_HANDLE_TAG | seq;
        file_handles().get_or_insert_with(HashMap::new).insert(raw, fa);
        Ok(ObjectId::from_raw(raw))
    }

    /// Runs `f` against the live [`FileAccess`] for `id`, if still open.
    pub fn with_handle_mut<R>(
        id: ObjectId,
        f: impl FnOnce(&mut FileAccess) -> EngineResult<R>,
    ) -> EngineResult<R> {
        let mut guard = file_handles();
        let map = guard.get_or_insert_with(HashMap::new);
        let fa = map.get_mut(&id.raw()).ok_or_else(|| {
            EngineError::InvalidOperation("FileAccess handle is closed or invalid".into())
        })?;
        f(fa)
    }

    /// Closes and drops a handle created by [`open_handle`](Self::open_handle).
    pub fn close_handle(id: ObjectId) {
        if let Some(map) = file_handles().as_mut() {
            if let Some(mut fa) = map.remove(&id.raw()) {
                fa.close();
            }
        }
    }
}

// ---------------------------------------------------------------------------
// DirAccess
// ---------------------------------------------------------------------------

/// A Godot-compatible directory handle for listing and mutating directories.
///
/// Mirrors a useful subset of Godot's `DirAccess`: open a directory, iterate
/// entries, create/remove/rename paths.
#[derive(Debug)]
pub struct DirAccess {
    path: PathBuf,
    entries: Vec<DirEntry>,
    index: usize,
    include_navigational: bool,
    include_hidden: bool,
    current_is_dir: bool,
    last_error: Option<String>,
}

/// One directory listing entry.
#[derive(Debug, Clone)]
struct DirEntry {
    name: String,
    is_dir: bool,
}

impl DirAccess {
    /// Opens a directory at `path` (`res://` / `user://` supported).
    pub fn open(path: &str) -> EngineResult<Self> {
        let resolved = resolve_path(path)?;
        if !resolved.is_dir() {
            return Err(EngineError::NotFound(format!(
                "not a directory: {}",
                resolved.display()
            )));
        }
        Ok(Self {
            path: resolved,
            entries: Vec::new(),
            index: 0,
            include_navigational: false,
            include_hidden: false,
            current_is_dir: false,
            last_error: None,
        })
    }

    /// Absolute filesystem path of this directory.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Whether `"."` / `".."` are included in listings (Godot default: false).
    pub fn set_include_navigational(&mut self, include: bool) {
        self.include_navigational = include;
    }

    /// Whether hidden entries (names starting with `'.'`) are listed.
    pub fn set_include_hidden(&mut self, include: bool) {
        self.include_hidden = include;
    }

    /// Starts a new directory listing. Call [`get_next`](Self::get_next) to iterate.
    pub fn list_dir_begin(&mut self) -> EngineResult<()> {
        let mut entries = Vec::new();
        let read = fs::read_dir(&self.path).map_err(|e| {
            self.last_error = Some(e.to_string());
            EngineError::from(e)
        })?;
        for entry in read {
            let entry = entry.map_err(|e| {
                self.last_error = Some(e.to_string());
                EngineError::from(e)
            })?;
            let name = entry.file_name().to_string_lossy().into_owned();
            let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
            if !self.include_navigational && (name == "." || name == "..") {
                continue;
            }
            if !self.include_hidden && name.starts_with('.') && name != "." && name != ".." {
                continue;
            }
            entries.push(DirEntry { name, is_dir });
        }
        entries.sort_by(|a, b| a.name.cmp(&b.name));
        self.entries = entries;
        self.index = 0;
        self.current_is_dir = false;
        self.last_error = None;
        Ok(())
    }

    /// Returns the next entry name, or an empty string when the listing is exhausted.
    pub fn get_next(&mut self) -> String {
        if self.index >= self.entries.len() {
            self.current_is_dir = false;
            return String::new();
        }
        let entry = &self.entries[self.index];
        self.current_is_dir = entry.is_dir;
        self.index += 1;
        entry.name.clone()
    }

    /// Whether the entry returned by the last [`get_next`](Self::get_next) is a directory.
    pub fn current_is_dir(&self) -> bool {
        self.current_is_dir
    }

    /// Ends the current listing and frees the entry cache.
    pub fn list_dir_end(&mut self) {
        self.entries.clear();
        self.index = 0;
        self.current_is_dir = false;
    }

    /// Creates a directory relative to this handle (or absolute / `res://` path).
    pub fn make_dir(&mut self, path: &str) -> EngineResult<()> {
        let target = self.resolve_child(path)?;
        fs::create_dir(&target).map_err(|e| {
            self.last_error = Some(e.to_string());
            EngineError::from(e)
        })?;
        self.last_error = None;
        Ok(())
    }

    /// Creates a directory and any missing parents.
    pub fn make_dir_recursive(&mut self, path: &str) -> EngineResult<()> {
        let target = self.resolve_child(path)?;
        fs::create_dir_all(&target).map_err(|e| {
            self.last_error = Some(e.to_string());
            EngineError::from(e)
        })?;
        self.last_error = None;
        Ok(())
    }

    /// Removes a file or empty directory at `path`.
    pub fn remove(&mut self, path: &str) -> EngineResult<()> {
        let target = self.resolve_child(path)?;
        let result = if target.is_dir() {
            fs::remove_dir(&target)
        } else {
            fs::remove_file(&target)
        };
        result.map_err(|e| {
            self.last_error = Some(e.to_string());
            EngineError::from(e)
        })?;
        self.last_error = None;
        Ok(())
    }

    /// Renames / moves `from` to `to`.
    pub fn rename(&mut self, from: &str, to: &str) -> EngineResult<()> {
        let src = self.resolve_child(from)?;
        let dst = self.resolve_child(to)?;
        fs::rename(&src, &dst).map_err(|e| {
            self.last_error = Some(e.to_string());
            EngineError::from(e)
        })?;
        self.last_error = None;
        Ok(())
    }

    /// Returns `true` if `path` exists as a directory.
    pub fn dir_exists(path: &str) -> bool {
        match resolve_path(path) {
            Ok(p) => p.is_dir(),
            Err(_) => false,
        }
    }

    /// Last error message from a failed mutation, if any.
    pub fn get_error(&self) -> Option<&str> {
        self.last_error.as_deref()
    }

    fn resolve_child(&self, path: &str) -> EngineResult<PathBuf> {
        if path.starts_with("res://")
            || path.starts_with("user://")
            || Path::new(path).is_absolute()
        {
            resolve_path(path)
        } else {
            Ok(self.path.join(path))
        }
    }
}

// ---------------------------------------------------------------------------
// Script-facing DirAccess handle registry
// ---------------------------------------------------------------------------

/// High bit tag so DirAccess handles never collide with ordinary node ObjectIds.
const DIR_HANDLE_TAG: u64 = 0xDA00_0000_0000_0000;

static NEXT_DIR_HANDLE: AtomicU64 = AtomicU64::new(1);
static DIR_HANDLES: Mutex<Option<HashMap<u64, DirAccess>>> = Mutex::new(None);

fn dir_handles() -> std::sync::MutexGuard<'static, Option<HashMap<u64, DirAccess>>> {
    DIR_HANDLES
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Returns `true` when `id` was issued by [`DirAccess::open_handle`].
pub fn is_dir_access_handle(id: ObjectId) -> bool {
    id.raw() & 0xFF00_0000_0000_0000 == DIR_HANDLE_TAG
}

impl DirAccess {
    /// Opens a directory and stores it in the process-wide handle table.
    pub fn open_handle(path: &str) -> EngineResult<ObjectId> {
        let da = Self::open(path)?;
        let seq = NEXT_DIR_HANDLE.fetch_add(1, Ordering::Relaxed);
        let raw = DIR_HANDLE_TAG | seq;
        dir_handles().get_or_insert_with(HashMap::new).insert(raw, da);
        Ok(ObjectId::from_raw(raw))
    }

    /// Runs `f` against the live [`DirAccess`] for `id`.
    pub fn with_handle_mut<R>(
        id: ObjectId,
        f: impl FnOnce(&mut DirAccess) -> EngineResult<R>,
    ) -> EngineResult<R> {
        let mut guard = dir_handles();
        let map = guard.get_or_insert_with(HashMap::new);
        let da = map.get_mut(&id.raw()).ok_or_else(|| {
            EngineError::InvalidOperation("DirAccess handle is closed or invalid".into())
        })?;
        f(da)
    }

    /// Closes and drops a handle created by [`open_handle`](Self::open_handle).
    pub fn close_handle(id: ObjectId) {
        if let Some(map) = dir_handles().as_mut() {
            map.remove(&id.raw());
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Serializes tests that mutate the global path roots.
    static ROOTS_LOCK: Mutex<()> = Mutex::new(());

    fn with_temp_roots<F: FnOnce(&Path, &Path)>(f: F) {
        let _guard = ROOTS_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let project = tempfile::tempdir().unwrap();
        let user = tempfile::tempdir().unwrap();
        set_path_roots(
            Some(project.path().to_path_buf()),
            Some(user.path().to_path_buf()),
        );
        f(project.path(), user.path());
        set_path_roots(None, None);
    }

    #[test]
    fn file_write_read_roundtrip_text() {
        with_temp_roots(|project, _| {
            let path = project.join("hello.txt");
            let path_str = path.to_string_lossy();
            {
                let mut f = FileAccess::open(&path_str, FileMode::Write).unwrap();
                f.store_string("hello patina").unwrap();
                f.close();
            }
            let mut f = FileAccess::open(&path_str, FileMode::Read).unwrap();
            assert_eq!(f.get_as_text().unwrap(), "hello patina");
            assert!(f.eof_reached());
        });
    }

    #[test]
    fn file_exists_true_false() {
        with_temp_roots(|project, _| {
            let path = project.join("exists.bin");
            assert!(!FileAccess::file_exists(&path.to_string_lossy()));
            fs::write(&path, b"x").unwrap();
            assert!(FileAccess::file_exists(&path.to_string_lossy()));
        });
    }

    #[test]
    fn res_uri_resolves_under_project_root() {
        with_temp_roots(|project, _| {
            fs::write(project.join("note.txt"), b"from res").unwrap();
            let mut f = FileAccess::open("res://note.txt", FileMode::Read).unwrap();
            assert_eq!(f.get_as_text().unwrap(), "from res");
            assert!(FileAccess::file_exists("res://note.txt"));
            assert!(!FileAccess::file_exists("res://missing.txt"));
        });
    }

    #[test]
    fn user_uri_write_read() {
        with_temp_roots(|_, user| {
            {
                let mut f = FileAccess::open("user://save.dat", FileMode::Write).unwrap();
                f.store_buffer(&[1, 2, 3, 4]).unwrap();
            }
            assert!(user.join("save.dat").is_file());
            let mut f = FileAccess::open("user://save.dat", FileMode::Read).unwrap();
            assert_eq!(f.get_buffer(4).unwrap(), vec![1, 2, 3, 4]);
        });
    }

    #[test]
    fn dir_access_lists_created_files() {
        with_temp_roots(|project, _| {
            fs::write(project.join("a.txt"), b"a").unwrap();
            fs::write(project.join("b.txt"), b"b").unwrap();
            fs::create_dir(project.join("sub")).unwrap();

            let mut dir = DirAccess::open(&project.to_string_lossy()).unwrap();
            dir.list_dir_begin().unwrap();
            let mut names = Vec::new();
            loop {
                let n = dir.get_next();
                if n.is_empty() {
                    break;
                }
                names.push((n, dir.current_is_dir()));
            }
            dir.list_dir_end();
            assert!(names.iter().any(|(n, d)| n == "a.txt" && !*d));
            assert!(names.iter().any(|(n, d)| n == "b.txt" && !*d));
            assert!(names.iter().any(|(n, d)| n == "sub" && *d));
        });
    }

    #[test]
    fn dir_make_and_remove() {
        with_temp_roots(|project, _| {
            let mut dir = DirAccess::open(&project.to_string_lossy()).unwrap();
            dir.make_dir("nested").unwrap();
            assert!(project.join("nested").is_dir());
            dir.remove("nested").unwrap();
            assert!(!project.join("nested").exists());
        });
    }

    #[test]
    fn file_mode_from_godot_flags() {
        assert_eq!(FileMode::from_godot(1), Some(FileMode::Read));
        assert_eq!(FileMode::from_godot(2), Some(FileMode::Write));
        assert_eq!(FileMode::from_godot(3), Some(FileMode::ReadWrite));
        assert_eq!(FileMode::from_godot(7), Some(FileMode::WriteRead));
        assert_eq!(FileMode::from_godot(99), None);
    }

    #[test]
    fn res_without_root_errors() {
        let _guard = ROOTS_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        set_path_roots(None, None);
        let err = FileAccess::open("res://x.txt", FileMode::Read).unwrap_err();
        assert!(matches!(err, EngineError::InvalidOperation(_)));
    }
}
