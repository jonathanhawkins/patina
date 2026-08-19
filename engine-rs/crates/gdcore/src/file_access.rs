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
// `Read`/`Seek`/`Write` are no longer imported directly: every call now goes
// through `FileBackend`, whose supertrait bounds bring those methods into scope
// (pat-s3rrv.34). `SeekFrom` is a type, so it still needs its import.
use std::io::SeekFrom;
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

// ---------------------------------------------------------------------------
// Mounted resource packs (pat-0q2kx.134)
// ---------------------------------------------------------------------------

/// A read-only archive mounted over `res://`, as Godot's `PackedData` mounts a
/// `.pck` (`core/io/file_access_pack.cpp`).
///
/// This is a trait rather than a concrete PCK type on purpose: the `.pck` reader
/// lives in `gdresource`, which depends on `gdcore`, so the dependency can only
/// point one way. `gdresource` implements this and registers its archives here,
/// which is what lets `FileAccess` serve `res://` reads out of a pack without
/// `gdcore` knowing the container format.
pub trait MountedPack: Send + Sync {
    /// Whether this pack supplies `res_path` (a full `res://…` path).
    fn has_file(&self, res_path: &str) -> bool;

    /// The bytes of `res_path`, or `None` when this pack does not supply it.
    fn read_file(&self, res_path: &str) -> Option<Vec<u8>>;

    /// Paths this pack *removes* — Godot's patch packs carry removal entries so
    /// an update can delete a file that a lower-priority pack still contains
    /// (`PackedData::add_path` with a zero-length removal marker). A removed
    /// path hides lower mounts **and** the loose file on disk.
    fn removed_files(&self) -> Vec<String> {
        Vec::new()
    }

    /// Entries this pack supplies directly inside `res_dir` (pat-s3rrv.34).
    ///
    /// Returns bare names, not full paths, matching what `DirAccess.get_files`
    /// and `get_directories` hand a script. Immediate children only — a pack
    /// stores a flat path list, so without this every nested file would surface
    /// as a sibling of the directory being listed.
    ///
    /// Defaulted to empty so a pack format that cannot enumerate stays
    /// trait-compatible; `DirAccess` then simply lists nothing extra rather
    /// than failing.
    fn list_dir(&self, _res_dir: &str) -> PackDirListing {
        PackDirListing::default()
    }
}

/// Immediate children of a directory inside a mounted pack (pat-s3rrv.34).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct PackDirListing {
    /// File names directly in the directory.
    pub files: Vec<String>,
    /// Subdirectory names directly in the directory.
    pub dirs: Vec<String>,
}

struct PackMount {
    label: String,
    priority: i32,
    pack: std::sync::Arc<dyn MountedPack>,
}

static PACK_MOUNTS: RwLock<Vec<PackMount>> = RwLock::new(Vec::new());

fn with_mounts<T>(f: impl FnOnce(&Vec<PackMount>) -> T) -> T {
    let guard = PACK_MOUNTS
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    f(&guard)
}

/// Mounts `pack` over `res://` under `label`, replacing any mount with the same
/// label. Higher `priority` is consulted first.
///
/// Mirrors `ProjectSettings.load_resource_pack`: a mounted pack shadows the
/// loose files on disk, so an exported game reads its own content even when a
/// stale project directory is still present.
pub fn mount_pack(label: impl Into<String>, pack: std::sync::Arc<dyn MountedPack>, priority: i32) {
    let label = label.into();
    let mut guard = PACK_MOUNTS
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    guard.retain(|m| m.label != label);
    guard.push(PackMount {
        label,
        priority,
        pack,
    });
    // Descending priority, so the search order is the mount order.
    guard.sort_by(|a, b| b.priority.cmp(&a.priority));
}

/// Unmounts the pack registered under `label`. Returns whether one was removed.
pub fn unmount_pack(label: &str) -> bool {
    let mut guard = PACK_MOUNTS
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let before = guard.len();
    guard.retain(|m| m.label != label);
    guard.len() < before
}

/// Removes every mounted pack. Used by tests and by a runner switching projects.
pub fn clear_mounted_packs() {
    PACK_MOUNTS
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clear()
}

/// Labels of the mounted packs, highest priority first.
pub fn mounted_pack_labels() -> Vec<String> {
    with_mounts(|m| m.iter().map(|p| p.label.clone()).collect())
}

/// Whether any mount marks `path` as removed (a patch pack's removal entry).
fn pack_removes(path: &str) -> bool {
    with_mounts(|mounts| {
        mounts
            .iter()
            .any(|m| m.pack.removed_files().iter().any(|r| r == path))
    })
}

/// Whether a mounted pack supplies `path`.
///
/// A path a higher-priority mount *removes* reports `false` even when a lower
/// mount still carries it, which is the point of a removal entry.
pub fn pack_has_file(path: &str) -> bool {
    if !path.starts_with("res://") {
        return false;
    }
    if pack_removes(path) {
        return false;
    }
    with_mounts(|mounts| mounts.iter().any(|m| m.pack.has_file(path)))
}

/// The canonical spelling of a `res://` directory used as a pack lookup key
/// (pat-s3rrv.34).
///
/// Strips a trailing slash so `res://scenes` and `res://scenes/` are one
/// directory, but leaves the root as `res://` — trimming that would produce
/// `res:`, and every prefix match against it would then fail.
///
/// Public because [`MountedPack::list_dir`] implementors must key on exactly
/// the same spelling this passes them.
pub fn normalize_pack_dir(dir: &str) -> String {
    if dir == "res://" || dir == "res:/" {
        return "res://".to_string();
    }
    dir.trim_end_matches('/').to_string()
}

/// Joins a relative path onto a normalized `res://` directory (pat-s3rrv.34).
///
/// Resolves `.` and `..` textually, because a `res://` path inside a pack has
/// no filesystem to canonicalize against. Ascending past the root clamps there,
/// matching how `res://` behaves as its own root rather than wrapping into the
/// host filesystem.
fn join_res_dir(base: &str, relative: &str) -> String {
    let mut segments: Vec<&str> = base
        .trim_start_matches("res://")
        .split('/')
        .filter(|s| !s.is_empty())
        .collect();
    for segment in relative.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                segments.pop();
            }
            other => segments.push(other),
        }
    }
    if segments.is_empty() {
        "res://".to_string()
    } else {
        format!("res://{}", segments.join("/"))
    }
}

/// Immediate children of `dir` across every mounted pack (pat-s3rrv.34).
///
/// Names are merged and de-duplicated, so a file present in two mounts is
/// listed once. Entries a pack *removes* are dropped, matching
/// [`pack_has_file`] — a patch pack that deletes a file must not leave it
/// visible in a directory listing.
///
/// Returns empty for a non-`res://` path: only `res://` can come from a pack.
pub fn pack_list_dir(dir: &str) -> PackDirListing {
    if !dir.starts_with("res://") {
        return PackDirListing::default();
    }
    let normalized = normalize_pack_dir(dir);
    // The root keeps its own trailing slash — `res://a` joins as `res://a/x`,
    // but the root joins as `res://x`, not `res:///x`.
    let prefix = if normalized == "res://" {
        normalized.clone()
    } else {
        format!("{normalized}/")
    };
    let mut files: Vec<String> = Vec::new();
    let mut dirs: Vec<String> = Vec::new();
    // Removals are gathered in their own lock scope FIRST. Calling
    // `pack_removes` from inside the `with_mounts` closure below would take a
    // second read lock on `PACK_MOUNTS` while the first is still held, which
    // deadlocks if a writer is queued between them. `pack_has_file` sequences
    // the same two reads for the same reason.
    let removed: Vec<String> =
        with_mounts(|mounts| mounts.iter().flat_map(|m| m.pack.removed_files()).collect());
    with_mounts(|mounts| {
        for mount in mounts.iter() {
            let listing = mount.pack.list_dir(&normalized);
            for name in listing.files {
                let full = format!("{prefix}{name}");
                if !removed.contains(&full) && !files.contains(&name) {
                    files.push(name);
                }
            }
            for name in listing.dirs {
                if !dirs.contains(&name) {
                    dirs.push(name);
                }
            }
        }
    });
    files.sort();
    dirs.sort();
    PackDirListing { files, dirs }
}

/// Whether any mounted pack supplies entries inside `dir` (pat-s3rrv.34).
///
/// `DirAccess::open` uses this to accept a `res://` directory that exists only
/// inside a pack, where there is no real directory on disk to stat.
pub fn pack_has_dir(dir: &str) -> bool {
    let listing = pack_list_dir(dir);
    !listing.files.is_empty() || !listing.dirs.is_empty()
}

/// Reads `path` from the highest-priority mounted pack that supplies it.
pub fn read_from_packs(path: &str) -> Option<Vec<u8>> {
    if !path.starts_with("res://") || pack_removes(path) {
        return None;
    }
    with_mounts(|mounts| mounts.iter().find_map(|m| m.pack.read_file(path)))
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

/// The IO surface a [`FileAccess`] handle reads and writes through
/// (pat-s3rrv.34).
///
/// Exists so one handle type can be backed either by an OS file or by bytes
/// from a mounted resource pack. Every `FileAccess` read/write funnels through
/// `with_file_mut`, so widening that one bound from `&mut File` to this trait
/// is what makes packed reads work everywhere at once, rather than each method
/// growing its own pack branch.
pub trait FileBackend: std::io::Read + std::io::Write + std::io::Seek {}
impl<T: std::io::Read + std::io::Write + std::io::Seek> FileBackend for T {}

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
    /// In-memory contents when this handle was served from a mounted resource
    /// pack (pat-s3rrv.34).
    ///
    /// `FileAccess::open` used to go straight to the OS, so in an exported game
    /// — where `res://` lives only inside a `.pck` — every script-side
    /// `FileAccess.open("res://…")` failed even though `read_file_bytes` had
    /// been pack-first all along. Only the static helper consulted the packs;
    /// the handle API did not.
    packed: Option<std::io::Cursor<Vec<u8>>>,
    last_error: Option<String>,
    /// When true, multi-byte typed reads/writes use big-endian order
    /// (Godot `FileAccess.set_big_endian`); default little-endian.
    big_endian: bool,
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

        // pat-s3rrv.34: a pack is a read-only archive, so opening a packed path
        // for writing fails the way Godot's `FileAccessPack` does
        // (ERR_FILE_CANT_WRITE) instead of quietly creating a loose file beside
        // the pack — that file would shadow nothing, since the pack still wins
        // every read, so the write would appear to succeed and then vanish.
        if mode.is_write() && pack_has_file(path) {
            return Err(EngineError::InvalidOperation(format!(
                "{path} is served by a mounted pack and cannot be written"
            )));
        }

        // Mounted packs answer reads first, matching the precedence
        // `read_file_bytes` already used.
        if mode.is_read() && !mode.is_write() {
            if let Some(bytes) = read_from_packs(path) {
                return Ok(Self {
                    path: resolved,
                    mode,
                    file: None,
                    packed: Some(std::io::Cursor::new(bytes)),
                    last_error: None,
                    big_endian: false,
                });
            }
        }

        let file = opts.open(&resolved).map_err(EngineError::from)?;
        Ok(Self {
            path: resolved,
            mode,
            file: Some(file),
            packed: None,
            last_error: None,
            big_endian: false,
        })
    }

    /// Returns `true` if a file exists at `path` (after `res://` / `user://` resolution).
    ///
    /// Mounted resource packs are consulted first, so an exported game's
    /// `FileAccess.file_exists("res://icon.png")` is true when the file lives in
    /// the `.pck` and nothing is on disk (pat-0q2kx.134). A path a patch pack
    /// removes reports `false` even if it is still present on disk — that is what
    /// makes a removal entry meaningful.
    pub fn file_exists(path: &str) -> bool {
        if path.starts_with("res://") {
            if pack_removes(path) {
                return false;
            }
            if pack_has_file(path) {
                return true;
            }
        }
        match resolve_path(path) {
            Ok(p) => p.is_file(),
            Err(_) => false,
        }
    }

    /// Reads a whole `res://` / `user://` / plain file as bytes, serving mounted
    /// packs before disk (pat-0q2kx.134).
    ///
    /// This is the read path a resource loader should use so packed and loose
    /// projects behave identically. `FileAccess::open` still returns a real
    /// `File` handle and therefore cannot serve a pack — seeking inside a packed
    /// entry is a separate concern from reading it whole, which is what every
    /// current loader does.
    pub fn read_file_bytes(path: &str) -> EngineResult<Vec<u8>> {
        if let Some(bytes) = read_from_packs(path) {
            return Ok(bytes);
        }
        if path.starts_with("res://") && pack_removes(path) {
            return Err(EngineError::InvalidOperation(format!(
                "{path} is removed by a mounted patch pack"
            )));
        }
        let resolved = resolve_path(path)?;
        fs::read(&resolved).map_err(EngineError::from)
    }

    /// [`read_file_bytes`](Self::read_file_bytes) decoded as UTF-8 (lossy, as
    /// Godot's `FileAccess.get_as_text` is).
    pub fn read_file_string(path: &str) -> EngineResult<String> {
        Ok(String::from_utf8_lossy(&Self::read_file_bytes(path)?).into_owned())
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
        F: FnOnce(&mut dyn FileBackend) -> std::io::Result<T>,
    {
        // pat-s3rrv.34: a pack-served handle reads from memory. Checked first
        // because such a handle has no `file` at all, and the closed-file error
        // below would otherwise report it as closed.
        if let Some(cursor) = self.packed.as_mut() {
            return match f(cursor) {
                Ok(v) => {
                    self.last_error = None;
                    Ok(v)
                }
                Err(e) => {
                    let msg = e.to_string();
                    self.last_error = Some(msg.clone());
                    Err(EngineError::Io(e))
                }
            };
        }
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

    // -- Typed binary get/store (Godot `FileAccess` numeric family) ----------
    //
    // Multi-byte values honor the current endianness (`set_big_endian`); reads
    // past EOF yield zero-padded values, matching Godot's forgiving behavior.

    /// Sets whether multi-byte typed reads/writes use big-endian order
    /// (`FileAccess.set_big_endian`). Off by default (little-endian).
    pub fn set_big_endian(&mut self, big_endian: bool) {
        self.big_endian = big_endian;
    }

    /// Returns the current endianness flag (`FileAccess.is_big_endian`).
    pub fn is_big_endian(&self) -> bool {
        self.big_endian
    }

    /// Writes a single byte (`FileAccess.store_8`).
    pub fn store_8(&mut self, value: u8) -> EngineResult<()> {
        self.store_buffer(&[value])
    }

    /// Writes a `u16` in the current endianness (`FileAccess.store_16`).
    pub fn store_16(&mut self, value: u16) -> EngineResult<()> {
        let bytes = if self.big_endian {
            value.to_be_bytes()
        } else {
            value.to_le_bytes()
        };
        self.store_buffer(&bytes)
    }

    /// Writes a `u32` in the current endianness (`FileAccess.store_32`).
    pub fn store_32(&mut self, value: u32) -> EngineResult<()> {
        let bytes = if self.big_endian {
            value.to_be_bytes()
        } else {
            value.to_le_bytes()
        };
        self.store_buffer(&bytes)
    }

    /// Writes a `u64` in the current endianness (`FileAccess.store_64`).
    pub fn store_64(&mut self, value: u64) -> EngineResult<()> {
        let bytes = if self.big_endian {
            value.to_be_bytes()
        } else {
            value.to_le_bytes()
        };
        self.store_buffer(&bytes)
    }

    /// Writes an IEEE-754 single-precision float (`FileAccess.store_float`).
    pub fn store_float(&mut self, value: f32) -> EngineResult<()> {
        self.store_32(value.to_bits())
    }

    /// Writes an IEEE-754 double-precision float (`FileAccess.store_double`).
    pub fn store_double(&mut self, value: f64) -> EngineResult<()> {
        self.store_64(value.to_bits())
    }

    /// Reads a single byte, or `0` at EOF (`FileAccess.get_8`).
    pub fn get_8(&mut self) -> EngineResult<u8> {
        Ok(self.get_buffer(1)?.first().copied().unwrap_or(0))
    }

    /// Reads a `u16` in the current endianness (`FileAccess.get_16`).
    pub fn get_16(&mut self) -> EngineResult<u16> {
        let read = self.get_buffer(2)?;
        let mut arr = [0u8; 2];
        let n = read.len().min(2);
        arr[..n].copy_from_slice(&read[..n]);
        Ok(if self.big_endian {
            u16::from_be_bytes(arr)
        } else {
            u16::from_le_bytes(arr)
        })
    }

    /// Reads a `u32` in the current endianness (`FileAccess.get_32`).
    pub fn get_32(&mut self) -> EngineResult<u32> {
        let read = self.get_buffer(4)?;
        let mut arr = [0u8; 4];
        let n = read.len().min(4);
        arr[..n].copy_from_slice(&read[..n]);
        Ok(if self.big_endian {
            u32::from_be_bytes(arr)
        } else {
            u32::from_le_bytes(arr)
        })
    }

    /// Reads a `u64` in the current endianness (`FileAccess.get_64`).
    pub fn get_64(&mut self) -> EngineResult<u64> {
        let read = self.get_buffer(8)?;
        let mut arr = [0u8; 8];
        let n = read.len().min(8);
        arr[..n].copy_from_slice(&read[..n]);
        Ok(if self.big_endian {
            u64::from_be_bytes(arr)
        } else {
            u64::from_le_bytes(arr)
        })
    }

    /// Reads an IEEE-754 single-precision float (`FileAccess.get_float`).
    pub fn get_float(&mut self) -> EngineResult<f32> {
        Ok(f32::from_bits(self.get_32()?))
    }

    /// Reads an IEEE-754 double-precision float (`FileAccess.get_double`).
    pub fn get_double(&mut self) -> EngineResult<f64> {
        Ok(f64::from_bits(self.get_64()?))
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
        // The in-memory pack backend must be dropped too (pat-s3rrv.34).
        // Clearing only `file` would leave a packed handle readable after
        // `close()`, so "closed" would mean nothing for an exported game.
        self.packed = None;
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
        file_handles()
            .get_or_insert_with(HashMap::new)
            .insert(raw, fa);
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
    /// The `res://` spelling this handle was opened with, normalized
    /// (pat-s3rrv.34).
    ///
    /// Kept alongside the resolved filesystem path because a mounted pack is
    /// keyed by `res://` path and `path` has already lost that spelling. `None`
    /// for `user://` and plain filesystem directories, which no pack can supply.
    res_path: Option<String>,
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
    ///
    /// A `res://` directory that exists only inside a mounted `.pck` opens too
    /// (pat-s3rrv.34): an exported game has no project tree on disk, so
    /// requiring a real directory here made every packed listing fail.
    pub fn open(path: &str) -> EngineResult<Self> {
        let resolved = resolve_path(path)?;
        let res_path = path.starts_with("res://").then(|| normalize_pack_dir(path));
        let in_pack = res_path.as_deref().map(pack_has_dir).unwrap_or(false);
        if !resolved.is_dir() && !in_pack {
            return Err(EngineError::NotFound(format!(
                "not a directory: {}",
                resolved.display()
            )));
        }
        Ok(Self {
            path: resolved,
            res_path,
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
        // A pack-only directory has nothing on disk to read. That is not an
        // error when a mount supplies entries for it (pat-s3rrv.34) — but a
        // genuinely missing directory must still fail, so the read error is
        // only swallowed when the pack has something to offer.
        match fs::read_dir(&self.path) {
            Ok(read) => {
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
                    if !self.include_hidden && name.starts_with('.') && name != "." && name != ".."
                    {
                        continue;
                    }
                    entries.push(DirEntry { name, is_dir });
                }
            }
            Err(e) => {
                if !self.res_path.as_deref().map(pack_has_dir).unwrap_or(false) {
                    self.last_error = Some(e.to_string());
                    return Err(EngineError::from(e));
                }
            }
        }

        // Entries the mount supplies, minus the ones already found on disk.
        if let Some(res_path) = self.res_path.as_deref() {
            let listing = pack_list_dir(res_path);
            // Snapshot the disk names first. A closure that inspected `entries`
            // directly would hold a borrow of it across the pushes below (E0502).
            let on_disk: Vec<(String, bool)> =
                entries.iter().map(|e| (e.name.clone(), e.is_dir)).collect();
            let known =
                |name: &str, is_dir: bool| on_disk.iter().any(|(n, d)| n == name && *d == is_dir);
            for name in listing.dirs {
                if (self.include_hidden || !name.starts_with('.')) && !known(&name, true) {
                    entries.push(DirEntry { name, is_dir: true });
                }
            }
            for name in listing.files {
                if (self.include_hidden || !name.starts_with('.')) && !known(&name, false) {
                    entries.push(DirEntry {
                        name,
                        is_dir: false,
                    });
                }
            }
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

    /// Splits `dir`'s entries into `(files, directories)`, sorted, honoring the
    /// hidden / navigational settings and excluding `.import` sidecars from the
    /// file list (Godot hides import metadata from `get_files`).
    /// Merges the entries a mounted pack supplies for `res_path` into `files` /
    /// `dirs` (pat-s3rrv.34).
    ///
    /// Additive rather than exclusive: Godot's `PackedData` shadows loose files
    /// but does not hide the directory they sit in, so a project running with a
    /// patch pack mounted over a real tree must list the union of both. Names
    /// already present are skipped, so a file supplied by both appears once.
    fn merge_pack_entries(
        res_path: Option<&str>,
        include_hidden: bool,
        files: &mut Vec<String>,
        dirs: &mut Vec<String>,
    ) {
        let Some(res_path) = res_path else {
            return;
        };
        let listing = pack_list_dir(res_path);
        for name in listing.files {
            if !include_hidden && name.starts_with('.') {
                continue;
            }
            // `.import` sidecars stay hidden here exactly as they are on disk.
            if !name.ends_with(".import") && !files.contains(&name) {
                files.push(name);
            }
        }
        for name in listing.dirs {
            if !include_hidden && name.starts_with('.') {
                continue;
            }
            if !dirs.contains(&name) {
                dirs.push(name);
            }
        }
    }

    fn split_entries(
        dir: &Path,
        res_path: Option<&str>,
        include_hidden: bool,
        include_navigational: bool,
    ) -> (Vec<String>, Vec<String>) {
        let mut files = Vec::new();
        let mut dirs = Vec::new();
        if let Ok(read) = fs::read_dir(dir) {
            for entry in read.flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                if !include_navigational && (name == "." || name == "..") {
                    continue;
                }
                if !include_hidden && name.starts_with('.') && name != "." && name != ".." {
                    continue;
                }
                if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                    dirs.push(name);
                } else if !name.ends_with(".import") {
                    files.push(name);
                }
            }
        }
        Self::merge_pack_entries(res_path, include_hidden, &mut files, &mut dirs);
        files.sort();
        dirs.sort();
        (files, dirs)
    }

    /// The non-directory file names in this directory, excluding `.import`
    /// sidecars (Godot `DirAccess.get_files`).
    pub fn get_files(&self) -> Vec<String> {
        Self::split_entries(
            &self.path,
            self.res_path.as_deref(),
            self.include_hidden,
            self.include_navigational,
        )
        .0
    }

    /// The subdirectory names in this directory (Godot `DirAccess.get_directories`).
    pub fn get_directories(&self) -> Vec<String> {
        Self::split_entries(
            &self.path,
            self.res_path.as_deref(),
            self.include_hidden,
            self.include_navigational,
        )
        .1
    }

    /// Static: file names at `path` (`res://` / `user://` resolved), excluding
    /// `.import` sidecars (Godot `DirAccess.get_files_at`).
    pub fn get_files_at(path: &str) -> EngineResult<Vec<String>> {
        let res_path = path.starts_with("res://").then(|| normalize_pack_dir(path));
        Ok(Self::split_entries(&resolve_path(path)?, res_path.as_deref(), false, false).0)
    }

    /// Static: subdirectory names at `path` (Godot `DirAccess.get_directories_at`).
    pub fn get_directories_at(path: &str) -> EngineResult<Vec<String>> {
        let res_path = path.starts_with("res://").then(|| normalize_pack_dir(path));
        Ok(Self::split_entries(&resolve_path(path)?, res_path.as_deref(), false, false).1)
    }

    /// The absolute path of this directory as a string (Godot `get_current_dir`).
    pub fn get_current_dir(&self) -> String {
        self.path.to_string_lossy().into_owned()
    }

    /// Changes the current directory to `path` (relative to this handle, or an
    /// absolute / `res://` path). Errors if the target is not a directory.
    pub fn change_dir(&mut self, path: &str) -> EngineResult<()> {
        let target = self.resolve_child(path)?;
        // The `res://` spelling is tracked alongside the filesystem path so a
        // pack listing keeps working after a change_dir (pat-s3rrv.34).
        let res_target = if path.starts_with("res://") {
            Some(normalize_pack_dir(path))
        } else {
            self.res_path
                .as_deref()
                .filter(|_| !Path::new(path).is_absolute())
                .map(|base| join_res_dir(base, path))
        };
        let in_pack = res_target.as_deref().map(pack_has_dir).unwrap_or(false);
        if !target.is_dir() && !in_pack {
            return Err(EngineError::NotFound(format!(
                "not a directory: {}",
                target.display()
            )));
        }
        self.path = target;
        self.res_path = res_target;
        Ok(())
    }

    /// Copies the file at `from` to `to` (both resolved relative to this handle,
    /// or absolute / `res://`) (Godot `DirAccess.copy`).
    pub fn copy(&mut self, from: &str, to: &str) -> EngineResult<()> {
        let src = self.resolve_child(from)?;
        let dst = self.resolve_child(to)?;
        fs::copy(&src, &dst).map(|_| ()).map_err(|e| {
            self.last_error = Some(e.to_string());
            EngineError::from(e)
        })
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
        // A directory that exists only inside a mounted pack counts, mirroring
        // `FileAccess::file_exists` (pat-s3rrv.34). Checked before resolving so
        // it still answers `true` for a pack mounted without a project root.
        if path.starts_with("res://") && pack_has_dir(&normalize_pack_dir(path)) {
            return true;
        }
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
        dir_handles()
            .get_or_insert_with(HashMap::new)
            .insert(raw, da);
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
    fn dir_access_get_files_and_directories() {
        with_temp_roots(|project, _| {
            let assets = project.join("assets");
            fs::create_dir_all(assets.join("sub")).unwrap();
            fs::write(assets.join("a.txt"), b"x").unwrap();
            fs::write(assets.join("tex.png"), b"x").unwrap();
            fs::write(assets.join("tex.png.import"), b"x").unwrap();
            fs::write(assets.join(".hidden"), b"x").unwrap();

            // get_files_at excludes the sibling directory, the .import sidecar,
            // and hidden files.
            let files = DirAccess::get_files_at("res://assets").unwrap();
            assert!(files.contains(&"a.txt".to_string()));
            assert!(files.contains(&"tex.png".to_string()));
            assert!(!files.iter().any(|f| f.ends_with(".import")));
            assert!(!files.iter().any(|f| f.starts_with('.')));
            assert!(!files.contains(&"sub".to_string()));

            // get_directories lists only subdirectories.
            let dir = DirAccess::open("res://assets").unwrap();
            assert_eq!(dir.get_directories(), vec!["sub".to_string()]);
            assert!(dir.get_files().contains(&"a.txt".to_string()));
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

    // pat-nzgdu.80: typed binary get_*/store_* round-trip + endianness control.
    #[test]
    fn file_access_typed_binary_roundtrip() {
        with_temp_roots(|_, _user| {
            // Little-endian (default): write a typed record, reopen, read it back.
            {
                let mut f = FileAccess::open("user://typed.dat", FileMode::Write).unwrap();
                f.store_8(0xAB).unwrap();
                f.store_16(0x1234).unwrap();
                f.store_32(0xDEAD_BEEF).unwrap();
                f.store_64(0x0123_4567_89AB_CDEF).unwrap();
                f.store_float(1.5f32).unwrap();
                f.store_double(-2.25f64).unwrap();
            }
            let mut f = FileAccess::open("user://typed.dat", FileMode::Read).unwrap();
            assert_eq!(f.get_8().unwrap(), 0xAB);
            assert_eq!(f.get_16().unwrap(), 0x1234);
            assert_eq!(f.get_32().unwrap(), 0xDEAD_BEEF);
            assert_eq!(f.get_64().unwrap(), 0x0123_4567_89AB_CDEF);
            assert_eq!(f.get_float().unwrap(), 1.5f32);
            assert_eq!(f.get_double().unwrap(), -2.25f64);

            // set_big_endian(true) flips the on-disk byte order.
            {
                let mut f = FileAccess::open("user://be.dat", FileMode::Write).unwrap();
                f.set_big_endian(true);
                assert!(f.is_big_endian());
                f.store_32(0x1122_3344).unwrap();
            }
            // Raw bytes are in big-endian order...
            let mut raw = FileAccess::open("user://be.dat", FileMode::Read).unwrap();
            assert_eq!(raw.get_buffer(4).unwrap(), vec![0x11, 0x22, 0x33, 0x44]);
            // ...and a big-endian get reads the original value.
            let mut be = FileAccess::open("user://be.dat", FileMode::Read).unwrap();
            be.set_big_endian(true);
            assert_eq!(be.get_32().unwrap(), 0x1122_3344);
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
