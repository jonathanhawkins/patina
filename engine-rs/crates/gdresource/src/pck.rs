//! PCK (Packed) file format packer and unpacker.
//!
//! Implements Godot's `.pck` archive format used for distributing game
//! resources as a single file. The format stores files with their
//! `res://` paths, sizes, offsets, and optional MD5 checksums.
//!
//! # Format Overview
//!
//! Matches Godot 4.6's `core/io/file_access_pack.cpp`. Two on-disk versions are
//! accepted for reading — V2 and V3 — and the writer emits V2 (which Godot's
//! reader still accepts, and which keeps the directory inline for a simple
//! single-pass writer).
//!
//! ```text
//! Header (both V2 and V3):
//!   magic:       4 bytes  "GDPC" (0x43504447 LE)
//!   version:     4 bytes  LE u32  (2 or 3)
//!   ver_major:   4 bytes  LE u32
//!   ver_minor:   4 bytes  LE u32
//!   ver_patch:   4 bytes  LE u32
//!   pack_flags:  4 bytes  LE u32  (PACK_DIR_ENCRYPTED | PACK_REL_FILEBASE | PACK_SPARSE_BUNDLE)
//!   file_base:   8 bytes  LE u64  (base offset added to every entry's offset;
//!                                  relative to the header start for V3 and for
//!                                  V2 when PACK_REL_FILEBASE is set)
//!   V3 only:
//!     dir_offset: 8 bytes LE u64  (offset of the directory; reader seeks here)
//!   V2 only:
//!     reserved:  64 bytes (16 × u32, all zero) — directory follows inline
//!
//! Directory:
//!   file_count:  4 bytes  LE u32
//!   For each file:
//!     path_len:  4 bytes  LE u32  (byte length of path INCLUDING NUL padding to 4)
//!     path:      path_len bytes   (UTF-8, NUL-padded to a 4-byte boundary)
//!     offset:    8 bytes  LE u64  (relative to file_base)
//!     size:      8 bytes  LE u64
//!     md5:       16 bytes         (MD5 digest, or zeroes)
//!     flags:     4 bytes  LE u32  (PACK_FILE_ENCRYPTED | PACK_FILE_REMOVAL | PACK_FILE_DELTA)
//!
//! File data is aligned (Godot default 64) within the archive.
//!
//! Encryption (PACK_DIR_ENCRYPTED / PACK_FILE_ENCRYPTED) is detected and
//! reported as an unsupported-format error; no decryption is performed.
//!
//! Embedded packs (self-contained executables) are located by scanning for the
//! magic at the start, or via the end-of-file trailer `[data_size: u64][magic]`.
//! ```

use std::collections::BTreeMap;
use std::io::{self, Cursor, Read, Seek, SeekFrom, Write};
use std::sync::Arc;

use gdcore::error::{EngineError, EngineResult};
use gdcore::file_access::{FileAccess, MountedPack};

use crate::loader::{ResourceLoader, TresLoader};
use crate::resource::Resource;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// The magic header bytes for a PCK file (`0x43504447` little-endian).
pub const PCK_MAGIC: &[u8; 4] = b"GDPC";

/// Legacy pack format version (directory inline after a reserved area).
pub const PCK_FORMAT_VERSION_V2: u32 = 2;

/// Current pack format version (directory located via a `dir_offset` field).
pub const PCK_FORMAT_VERSION_V3: u32 = 3;

/// PCK format version emitted by [`PckPacker`].
///
/// V2 is written because Godot's reader still accepts it and it keeps the
/// directory inline, allowing a single seekable pass. Godot's own writer emits
/// V3; both are readable here.
pub const PCK_FORMAT_VERSION: u32 = PCK_FORMAT_VERSION_V2;

/// Default alignment for file data within the archive.
pub const PCK_DATA_ALIGNMENT: u64 = 64;

/// Size of the reserved header area (16 × u32).
const RESERVED_FIELD_COUNT: usize = 16;

// Pack-level flags (header `pack_flags`), from `file_access_pack.h`.
/// The directory block is encrypted (unsupported here).
const PACK_DIR_ENCRYPTED: u32 = 1 << 0;
/// `file_base` is relative to the header start rather than absolute.
const PACK_REL_FILEBASE: u32 = 1 << 1;
/// The pack is a sparse bundle (file data lives in sibling files).
#[allow(dead_code)]
const PACK_SPARSE_BUNDLE: u32 = 1 << 2;

// Per-file flags (directory entry `flags`), from `file_access_pack.h`.
/// The file data is encrypted (unsupported here).
const PACK_FILE_ENCRYPTED: u32 = 1 << 0;
/// The entry is a removal record (the path was deleted by a patch pack).
const PACK_FILE_REMOVAL: u32 = 1 << 1;
/// The entry is a delta patch over a base pack.
#[allow(dead_code)]
const PACK_FILE_DELTA: u32 = 1 << 2;

// ---------------------------------------------------------------------------
// PckEntry
// ---------------------------------------------------------------------------

/// Metadata for a single file entry within a PCK archive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PckEntry {
    /// The `res://` path of the file.
    pub path: String,
    /// Byte offset of the file data within the archive.
    pub offset: u64,
    /// Size of the file data in bytes.
    pub size: u64,
    /// MD5 checksum (16 bytes), or all zeros if not computed.
    pub md5: [u8; 16],
}

// ---------------------------------------------------------------------------
// PckHeader
// ---------------------------------------------------------------------------

/// Parsed PCK file header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PckHeader {
    /// Format version (2 for Godot 4.x).
    pub format_version: u32,
    /// Engine major version.
    pub ver_major: u32,
    /// Engine minor version.
    pub ver_minor: u32,
    /// Engine patch version.
    pub ver_patch: u32,
    /// Flags (currently unused, reserved).
    pub flags: u32,
    /// Number of files in the archive.
    pub file_count: u32,
}

impl Default for PckHeader {
    fn default() -> Self {
        Self {
            format_version: PCK_FORMAT_VERSION,
            ver_major: 4,
            ver_minor: 6,
            ver_patch: 1,
            flags: 0,
            file_count: 0,
        }
    }
}

// ---------------------------------------------------------------------------
// PckArchive — read/unpack
// ---------------------------------------------------------------------------

/// A parsed PCK archive. Stores the header and directory of entries.
#[derive(Debug, Clone)]
pub struct PckArchive {
    /// The archive header.
    pub header: PckHeader,
    /// File entries keyed by their `res://` path.
    pub entries: BTreeMap<String, PckEntry>,
    /// Paths this pack *removes* — Godot patch packs carry `PACK_FILE_REMOVAL`
    /// records so an update can delete a file a base pack still contains
    /// (`file_access_pack.h`). These were parsed and discarded before, so a
    /// removal entry had no effect at all (pat-0q2kx.134).
    pub removed: Vec<String>,
}

impl PckArchive {
    /// Parses a PCK archive from a byte slice, reading the header and
    /// directory but not the file data.
    ///
    /// The archive may be a standalone `.pck` (magic at offset 0) or embedded at
    /// the end of a self-contained executable (located via the end-of-file
    /// trailer). Entry offsets returned in [`PckEntry::offset`] are absolute
    /// within `data`, so [`extract_file`](Self::extract_file) works directly.
    pub fn from_bytes(data: &[u8]) -> io::Result<Self> {
        let pck_start = find_pck_start(data)?;
        Self::parse_at(data, pck_start)
    }

    /// Reads and parses a PCK archive from any seekable reader.
    ///
    /// The reader is drained from its current position to the end, then parsed
    /// with [`from_bytes`](Self::from_bytes) (so embedded-pack scanning applies).
    pub fn read_from<R: Read + Seek>(reader: &mut R) -> io::Result<Self> {
        let mut buf = Vec::new();
        reader.read_to_end(&mut buf)?;
        Self::from_bytes(&buf)
    }

    /// Parses the header and directory of a PCK whose `GDPC` magic begins at
    /// `pck_start` within `data`, matching `PackedSourcePCK::try_open_pack`.
    fn parse_at(data: &[u8], pck_start: u64) -> io::Result<Self> {
        let mut cursor = Cursor::new(data);
        cursor.seek(SeekFrom::Start(pck_start))?;

        // Magic.
        let mut magic = [0u8; 4];
        cursor.read_exact(&mut magic)?;
        if &magic != PCK_MAGIC {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("invalid PCK magic: expected GDPC, got {magic:?}"),
            ));
        }

        // Version fields (shared by V2 and V3).
        let format_version = read_u32(&mut cursor)?;
        let ver_major = read_u32(&mut cursor)?;
        let ver_minor = read_u32(&mut cursor)?;
        let ver_patch = read_u32(&mut cursor)?;

        if format_version != PCK_FORMAT_VERSION_V2 && format_version != PCK_FORMAT_VERSION_V3 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("unsupported PCK version: {format_version} (expected 2 or 3)"),
            ));
        }

        let pack_flags = read_u32(&mut cursor)?;
        let enc_directory = pack_flags & PACK_DIR_ENCRYPTED != 0;
        let rel_filebase = pack_flags & PACK_REL_FILEBASE != 0;

        if enc_directory {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "encrypted PCK directory is not supported",
            ));
        }

        // file_base is present in both V2 and V3. It is relative to the pack
        // start for V3, and for V2 only when PACK_REL_FILEBASE is set.
        let mut file_base = read_u64(&mut cursor)?;
        if format_version == PCK_FORMAT_VERSION_V3
            || (format_version == PCK_FORMAT_VERSION_V2 && rel_filebase)
        {
            file_base += pck_start;
        }

        if format_version == PCK_FORMAT_VERSION_V3 {
            // V3: directory offset follows, then reserved space is skipped.
            let dir_offset = read_u64(&mut cursor)? + pck_start;
            cursor.seek(SeekFrom::Start(dir_offset))?;
        } else {
            // V2: 16 × u32 reserved, directory follows inline.
            for _ in 0..RESERVED_FIELD_COUNT {
                let _ = read_u32(&mut cursor)?;
            }
        }

        let file_count = read_u32(&mut cursor)?;

        let header = PckHeader {
            format_version,
            ver_major,
            ver_minor,
            ver_patch,
            flags: pack_flags,
            file_count,
        };

        // Directory entries.
        let mut entries = BTreeMap::new();
        let mut removed: Vec<String> = Vec::new();
        for _ in 0..file_count {
            let path_len = read_u32(&mut cursor)? as usize;
            let mut path_buf = vec![0u8; path_len];
            cursor.read_exact(&mut path_buf)?;
            // Trim NUL padding.
            let path = String::from_utf8_lossy(&path_buf)
                .trim_end_matches('\0')
                .to_string();

            let rel_offset = read_u64(&mut cursor)?;
            let size = read_u64(&mut cursor)?;

            let mut md5 = [0u8; 16];
            cursor.read_exact(&mut md5)?;

            let flags = read_u32(&mut cursor)?;

            // A removal record marks a path deleted by a patch pack. It carries
            // no data, so it is kept out of `entries` — but it must be recorded,
            // or mounting the patch could not hide the base pack's copy.
            if flags & PACK_FILE_REMOVAL != 0 {
                removed.push(path);
                continue;
            }
            if flags & PACK_FILE_ENCRYPTED != 0 {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("encrypted PCK file entry is not supported: {path}"),
                ));
            }

            // Store the absolute offset within `data` so extraction is direct.
            let offset = file_base + rel_offset;

            entries.insert(
                path.clone(),
                PckEntry {
                    path,
                    offset,
                    size,
                    md5,
                },
            );
        }

        Ok(Self {
            header,
            entries,
            removed,
        })
    }

    /// Extracts the data for a specific file entry from the archive bytes.
    pub fn extract_file<'a>(&self, data: &'a [u8], path: &str) -> Option<&'a [u8]> {
        let entry = self.entries.get(path)?;
        let start = entry.offset as usize;
        let end = start + entry.size as usize;
        if end <= data.len() {
            Some(&data[start..end])
        } else {
            None
        }
    }

    /// Returns the list of file paths in the archive (sorted).
    pub fn file_paths(&self) -> Vec<&str> {
        self.entries.keys().map(|s| s.as_str()).collect()
    }

    /// Returns the number of files in the archive.
    pub fn file_count(&self) -> usize {
        self.entries.len()
    }

    /// Returns `true` if the archive contains the given path.
    pub fn has_file(&self, path: &str) -> bool {
        self.entries.contains_key(path)
    }

    /// Returns the entry for the given path, if present.
    pub fn get_entry(&self, path: &str) -> Option<&PckEntry> {
        self.entries.get(path)
    }

    /// Returns the total size of all file data in the archive.
    pub fn total_data_size(&self) -> u64 {
        self.entries.values().map(|e| e.size).sum()
    }
}

// ---------------------------------------------------------------------------
// PckPacker — write/pack
// ---------------------------------------------------------------------------

/// Builds a PCK archive from a set of files.
///
/// Files are added with their `res://` paths and byte content, then
/// packed into the PCK binary format.
#[derive(Debug)]
pub struct PckPacker {
    /// Engine version to embed in the header.
    pub ver_major: u32,
    pub ver_minor: u32,
    pub ver_patch: u32,
    /// Alignment for file data.
    pub alignment: u64,
    /// Files to pack: (path, data).
    files: Vec<(String, Vec<u8>)>,
}

impl PckPacker {
    /// Creates a new packer with default Godot 4.6.1 version.
    pub fn new() -> Self {
        Self {
            ver_major: 4,
            ver_minor: 6,
            ver_patch: 1,
            alignment: PCK_DATA_ALIGNMENT,
            files: Vec::new(),
        }
    }

    /// Sets the engine version embedded in the header.
    pub fn with_version(mut self, major: u32, minor: u32, patch: u32) -> Self {
        self.ver_major = major;
        self.ver_minor = minor;
        self.ver_patch = patch;
        self
    }

    /// Sets the data alignment (must be a power of 2).
    pub fn with_alignment(mut self, alignment: u64) -> Self {
        self.alignment = alignment;
        self
    }

    /// Adds a file to the archive.
    pub fn add_file(&mut self, path: impl Into<String>, data: Vec<u8>) {
        self.files.push((path.into(), data));
    }

    /// Returns the number of files queued for packing.
    pub fn file_count(&self) -> usize {
        self.files.len()
    }

    /// Packs all added files into a PCK byte vector.
    pub fn pack(&self) -> io::Result<Vec<u8>> {
        let mut out = Cursor::new(Vec::new());
        self.write_to(&mut out)?;
        Ok(out.into_inner())
    }

    /// Writes the packed archive to any writer, using the V2 layout.
    ///
    /// The output matches Godot's reader: a `file_base` header field and a
    /// per-entry trailing `flags` u32 (always 0 here). `pack_flags` sets
    /// `PACK_REL_FILEBASE` (as Godot's own writer does) and the directory is
    /// inline after the reserved area.
    pub fn write_to<W: Write + Seek>(&self, writer: &mut W) -> io::Result<()> {
        // -- Compute the directory size to locate the data section --
        let mut dir_size: u64 = 0;
        for (path, _) in &self.files {
            let padded_len = pad_to_4(path.len());
            // path_len(4) + path(padded) + offset(8) + size(8) + md5(16) + flags(4)
            dir_size += 4 + padded_len as u64 + 8 + 8 + 16 + 4;
        }

        // Header size (V2): magic(4) + version(4) + ver×3(12) + pack_flags(4)
        //                   + file_base(8) + reserved(64) + file_count(4) = 100.
        let header_size: u64 = 4 + 4 + 12 + 4 + 8 + (RESERVED_FIELD_COUNT as u64 * 4) + 4;
        let data_start = align_up(header_size + dir_size, self.alignment);
        // file_base is written relative to the pack start (here the buffer start).
        // Matching Godot's writer, PACK_REL_FILEBASE is set so the reader adds the
        // pack's start offset — which makes the output correct both standalone and
        // when appended to an executable at a non-zero offset.
        let file_base = data_start;

        // -- Header --
        writer.write_all(PCK_MAGIC)?;
        write_u32(writer, PCK_FORMAT_VERSION_V2)?;
        write_u32(writer, self.ver_major)?;
        write_u32(writer, self.ver_minor)?;
        write_u32(writer, self.ver_patch)?;
        write_u32(writer, PACK_REL_FILEBASE)?; // pack_flags: relative file_base.
        write_u64(writer, file_base)?; // Files base (relative to pack start).

        // Reserved (16 × u32).
        for _ in 0..RESERVED_FIELD_COUNT {
            write_u32(writer, 0)?;
        }

        write_u32(writer, self.files.len() as u32)?;

        // -- Write directory entries --
        let mut current_offset = data_start;
        let mut offsets = Vec::with_capacity(self.files.len());

        for (path, data) in &self.files {
            let path_bytes = path.as_bytes();
            let padded_len = pad_to_4(path_bytes.len());

            write_u32(writer, padded_len as u32)?;
            writer.write_all(path_bytes)?;
            // NUL-pad to alignment.
            let padding = padded_len - path_bytes.len();
            if padding > 0 {
                writer.write_all(&vec![0u8; padding])?;
            }

            // Offset is stored relative to file_base (Godot: `ofs - file_base`).
            write_u64(writer, current_offset - file_base)?;
            write_u64(writer, data.len() as u64)?;

            // MD5 — write zeros (optional, not computed here).
            writer.write_all(&[0u8; 16])?;

            // Per-file flags: not encrypted, not a removal, not a delta.
            write_u32(writer, 0)?;

            offsets.push(current_offset);
            current_offset = align_up(current_offset + data.len() as u64, self.alignment);
        }

        // -- Write file data --
        for (i, (_, data)) in self.files.iter().enumerate() {
            let target = offsets[i];
            let current = writer.stream_position()?;
            if current < target {
                writer.write_all(&vec![0u8; (target - current) as usize])?;
            }
            writer.write_all(data)?;
        }

        Ok(())
    }
}

impl Default for PckPacker {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// PckResourceLoader — ResourceLoader backed by a PCK archive
// ---------------------------------------------------------------------------

/// A [`ResourceLoader`] that serves resources from an in-memory PCK archive.
///
/// The loader holds the raw archive bytes and the parsed directory. When a
/// `res://` path is requested, it extracts the file bytes from the archive
/// and parses `.tres` files via [`TresLoader`]. Non-`.tres` files are
/// returned as opaque resources with a `"bytes"` property containing the
/// raw data length (the actual bytes can be retrieved via [`extract_raw`]).
///
/// # Usage
///
/// ```rust,ignore
/// let loader = PckResourceLoader::from_bytes(pck_bytes)?;
/// let res = loader.load("res://player.tres")?;
/// ```
#[derive(Debug, Clone)]
pub struct PckResourceLoader {
    archive: PckArchive,
    data: Vec<u8>,
}

impl PckResourceLoader {
    /// Creates a new loader from raw PCK archive bytes.
    pub fn from_bytes(data: Vec<u8>) -> io::Result<Self> {
        let archive = PckArchive::from_bytes(&data)?;
        Ok(Self { archive, data })
    }

    /// Returns the parsed archive directory.
    pub fn archive(&self) -> &PckArchive {
        &self.archive
    }

    /// Extracts raw bytes for a path from the PCK data.
    pub fn extract_raw(&self, path: &str) -> Option<&[u8]> {
        self.archive.extract_file(&self.data, path)
    }

    /// Returns all file paths in the archive.
    pub fn file_paths(&self) -> Vec<&str> {
        self.archive.file_paths()
    }

    /// Returns `true` if the archive contains the given path.
    pub fn has_file(&self, path: &str) -> bool {
        self.archive.has_file(path)
    }

    /// Returns the number of files in the archive.
    pub fn file_count(&self) -> usize {
        self.archive.file_count()
    }

    /// Returns the PCK header metadata.
    pub fn header(&self) -> &PckHeader {
        &self.archive.header
    }
}

impl ResourceLoader for PckResourceLoader {
    fn load(&self, path: &str) -> EngineResult<Arc<Resource>> {
        let bytes = self.archive.extract_file(&self.data, path).ok_or_else(|| {
            EngineError::NotFound(format!("file not found in PCK archive: {path}"))
        })?;

        // Parse .tres files as text resources.
        if path.ends_with(".tres") || path.ends_with(".tscn") {
            let text = std::str::from_utf8(bytes)
                .map_err(|e| EngineError::Parse(format!("invalid UTF-8 in {path}: {e}")))?;
            let loader = TresLoader::new();
            loader.parse_str(text, path)
        } else {
            // Non-text resources: return an opaque Resource with metadata.
            let mut res = Resource::new("PackedFile");
            res.path = path.to_string();
            res.set_property("byte_size", gdvariant::Variant::Int(bytes.len() as i64));
            Ok(Arc::new(res))
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn read_u32<R: Read>(reader: &mut R) -> io::Result<u32> {
    let mut buf = [0u8; 4];
    reader.read_exact(&mut buf)?;
    Ok(u32::from_le_bytes(buf))
}

fn read_u64<R: Read>(reader: &mut R) -> io::Result<u64> {
    let mut buf = [0u8; 8];
    reader.read_exact(&mut buf)?;
    Ok(u64::from_le_bytes(buf))
}

/// Locates the offset of the `GDPC` magic within `data`, matching Godot's
/// `PackedSourcePCK::try_open_pack` scan.
///
/// 1. Magic at offset 0 — a standalone `.pck` file.
/// 2. End-of-file trailer `[data_size: u64][magic: u32]` — a self-contained
///    executable with the pack appended; the header starts `12 + data_size`
///    bytes before the end.
///
/// The executable "embedded pck section" scan (`OS::get_embedded_pck_offset`)
/// is platform/executable-specific and not applicable to an in-memory slice, so
/// it is intentionally omitted.
fn find_pck_start(data: &[u8]) -> io::Result<u64> {
    // 1. Standalone: magic at the very start.
    if data.len() >= 4 && &data[0..4] == PCK_MAGIC {
        return Ok(0);
    }

    // 2. Appended to an executable: check the end-of-file trailer.
    let n = data.len();
    if n >= 12 && &data[n - 4..n] == PCK_MAGIC {
        let ds = u64::from_le_bytes(data[n - 12..n - 4].try_into().unwrap());
        // The pack header begins `data_size` bytes before the 12-byte trailer.
        if let Some(start) = (n as u64).checked_sub(12 + ds) {
            let s = start as usize;
            if s + 4 <= n && &data[s..s + 4] == PCK_MAGIC {
                return Ok(start);
            }
        }
    }

    Err(io::Error::new(
        io::ErrorKind::InvalidData,
        "invalid PCK magic: no GDPC header at offset 0 or in the end-of-file trailer",
    ))
}

fn write_u32<W: Write>(writer: &mut W, v: u32) -> io::Result<()> {
    writer.write_all(&v.to_le_bytes())
}

fn write_u64<W: Write>(writer: &mut W, v: u64) -> io::Result<()> {
    writer.write_all(&v.to_le_bytes())
}

/// Rounds `n` up to the next multiple of 4.
fn pad_to_4(n: usize) -> usize {
    (n + 3) & !3
}

/// Rounds `offset` up to the next multiple of `alignment`.
fn align_up(offset: u64, alignment: u64) -> u64 {
    if alignment == 0 {
        return offset;
    }
    (offset + alignment - 1) / alignment * alignment
}

// ---------------------------------------------------------------------------
// PckMountManager — runtime PCK archive mounting
// ---------------------------------------------------------------------------

/// A mount point representing a loaded PCK archive.
#[derive(Debug, Clone)]
struct MountedPck {
    /// User-assigned label for this mount (e.g. the filename).
    label: String,
    /// Priority — higher values are searched first.
    priority: i32,
    /// The resource loader for this archive.
    loader: PckResourceLoader,
}

/// Manages runtime mounting of PCK archives as resource overlays.
///
/// Mirrors Godot's `ProjectSettings.load_resource_pack()` behaviour:
/// multiple PCK files can be mounted at runtime with a priority order.
/// When loading a resource, the manager searches mounted archives from
/// highest to lowest priority and returns the first match.
///
/// # Example
///
/// ```rust,ignore
/// let mut mgr = PckMountManager::new();
/// mgr.mount("dlc1", pck_bytes, 10)?;
/// let res = mgr.load("res://dlc_level.tres")?;
/// mgr.unmount("dlc1");
/// ```
#[derive(Debug)]
pub struct PckMountManager {
    mounts: Vec<MountedPck>,
}

impl PckMountManager {
    /// Creates a new empty mount manager.
    pub fn new() -> Self {
        Self { mounts: Vec::new() }
    }

    /// Mounts a PCK archive with the given label and priority.
    ///
    /// Higher priority mounts are searched first. Returns an error if the
    /// PCK data is invalid. If a mount with the same label already exists,
    /// it is replaced.
    pub fn mount(
        &mut self,
        label: impl Into<String>,
        data: Vec<u8>,
        priority: i32,
    ) -> io::Result<()> {
        let label = label.into();
        let loader = PckResourceLoader::from_bytes(data)?;
        // Remove existing mount with the same label.
        self.mounts.retain(|m| m.label != label);
        self.mounts.push(MountedPck {
            label,
            priority,
            loader,
        });
        // Sort by descending priority for search order.
        self.mounts.sort_by(|a, b| b.priority.cmp(&a.priority));
        Ok(())
    }

    /// Unmounts a PCK archive by label. Returns `true` if it was found and removed.
    pub fn unmount(&mut self, label: &str) -> bool {
        let before = self.mounts.len();
        self.mounts.retain(|m| m.label != label);
        self.mounts.len() < before
    }

    /// Returns the number of currently mounted archives.
    pub fn mount_count(&self) -> usize {
        self.mounts.len()
    }

    /// Returns the labels of all mounted archives, in priority order (highest first).
    pub fn mounted_labels(&self) -> Vec<&str> {
        self.mounts.iter().map(|m| m.label.as_str()).collect()
    }

    /// Returns `true` if any mounted archive contains the given path.
    pub fn has_file(&self, path: &str) -> bool {
        self.mounts.iter().any(|m| m.loader.has_file(path))
    }

    /// Returns all unique file paths across all mounted archives.
    pub fn all_files(&self) -> Vec<String> {
        let mut paths = std::collections::BTreeSet::new();
        for mount in &self.mounts {
            for p in mount.loader.file_paths() {
                paths.insert(p.to_string());
            }
        }
        paths.into_iter().collect()
    }

    /// Returns which mount label owns a particular path (highest priority wins).
    pub fn which_mount(&self, path: &str) -> Option<&str> {
        self.mounts
            .iter()
            .find(|m| m.loader.has_file(path))
            .map(|m| m.label.as_str())
    }

    /// Extracts raw bytes for a path from the highest-priority mount that has it.
    pub fn extract_raw(&self, path: &str) -> Option<&[u8]> {
        self.mounts.iter().find_map(|m| m.loader.extract_raw(path))
    }
}

impl Default for PckMountManager {
    fn default() -> Self {
        Self::new()
    }
}

impl ResourceLoader for PckMountManager {
    fn load(&self, path: &str) -> EngineResult<Arc<Resource>> {
        for mount in &self.mounts {
            if mount.loader.has_file(path) {
                return mount.loader.load(path);
            }
        }
        Err(EngineError::NotFound(format!(
            "file not found in any mounted PCK: {path}"
        )))
    }
}

// ---------------------------------------------------------------------------
// Mounting a PCK over res:// (pat-0q2kx.134)
// ---------------------------------------------------------------------------

/// A [`PckResourceLoader`] adapted to `gdcore`'s [`MountedPack`] overlay so
/// `res://` reads resolve out of the archive.
///
/// `gdcore` cannot depend on this crate (the dependency runs the other way), so
/// the overlay is defined there as a trait and implemented here.
#[derive(Debug)]
pub struct PckMount {
    loader: PckResourceLoader,
}

impl PckMount {
    /// Wraps already-parsed PCK bytes.
    pub fn from_bytes(data: Vec<u8>) -> io::Result<Self> {
        Ok(Self {
            loader: PckResourceLoader::from_bytes(data)?,
        })
    }

    /// The underlying loader, for callers that want the archive directory.
    pub fn loader(&self) -> &PckResourceLoader {
        &self.loader
    }
}

impl MountedPack for PckMount {
    fn has_file(&self, res_path: &str) -> bool {
        self.loader.has_file(res_path)
    }

    fn read_file(&self, res_path: &str) -> Option<Vec<u8>> {
        self.loader.extract_raw(res_path).map(|b| b.to_vec())
    }

    fn removed_files(&self) -> Vec<String> {
        self.loader.archive().removed.clone()
    }
}

/// Mounts a `.pck` over `res://`, Godot's `ProjectSettings.load_resource_pack`.
///
/// * `label` identifies the mount for later [`unmount_resource_pack`].
/// * `replace` mirrors Godot's `replace_files`: `true` mounts above everything
///   already mounted (its files win), `false` mounts below them so existing
///   content keeps precedence.
/// * `offset` skips into the file, for a pack embedded after another payload.
///
/// The bytes are read through [`FileAccess::read_file_bytes`], so a pack can
/// itself live inside an already-mounted pack — which is how a game ships DLC
/// referenced from its own `res://`.
pub fn load_resource_pack(
    label: impl Into<String>,
    path: &str,
    replace: bool,
    offset: u64,
) -> EngineResult<()> {
    let mut bytes = FileAccess::read_file_bytes(path)?;
    if offset > 0 {
        if (offset as usize) >= bytes.len() {
            return Err(EngineError::InvalidOperation(format!(
                "resource pack offset {offset} is past the end of {path}"
            )));
        }
        bytes.drain(..offset as usize);
    }
    let mount = PckMount::from_bytes(bytes)
        .map_err(|e| EngineError::Parse(format!("{path} is not a valid .pck: {e}")))?;
    // Godot's `replace_files` decides precedence rather than whether files are
    // overwritten on disk: a replacing pack is searched before the packs already
    // mounted, a non-replacing one after them.
    let priority = if replace {
        next_mount_priority_above()
    } else {
        next_mount_priority_below()
    };
    gdcore::file_access::mount_pack(label, Arc::new(mount), priority);
    Ok(())
}

/// Removes a pack mounted by [`load_resource_pack`].
pub fn unmount_resource_pack(label: &str) -> bool {
    gdcore::file_access::unmount_pack(label)
}

/// Priority counters for mount ordering. `load_resource_pack` needs "above
/// everything" and "below everything" without the caller tracking numbers.
static MOUNT_ABOVE: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);
static MOUNT_BELOW: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);

fn next_mount_priority_above() -> i32 {
    MOUNT_ABOVE.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1
}

fn next_mount_priority_below() -> i32 {
    MOUNT_BELOW.fetch_sub(1, std::sync::atomic::Ordering::SeqCst) - 1
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pack_empty_archive() {
        let packer = PckPacker::new();
        let data = packer.pack().unwrap();
        let archive = PckArchive::from_bytes(&data).unwrap();
        assert_eq!(archive.file_count(), 0);
        assert_eq!(archive.header.format_version, PCK_FORMAT_VERSION);
        assert_eq!(archive.header.ver_major, 4);
    }

    #[test]
    fn roundtrip_single_file() {
        let mut packer = PckPacker::new();
        packer.add_file("res://hello.txt", b"Hello, world!".to_vec());
        let data = packer.pack().unwrap();

        let archive = PckArchive::from_bytes(&data).unwrap();
        assert_eq!(archive.file_count(), 1);
        assert!(archive.has_file("res://hello.txt"));

        let content = archive.extract_file(&data, "res://hello.txt").unwrap();
        assert_eq!(content, b"Hello, world!");
    }

    #[test]
    fn roundtrip_multiple_files() {
        let mut packer = PckPacker::new();
        packer.add_file("res://a.txt", b"alpha".to_vec());
        packer.add_file("res://b.txt", b"beta".to_vec());
        packer.add_file("res://sub/c.txt", b"gamma".to_vec());
        let data = packer.pack().unwrap();

        let archive = PckArchive::from_bytes(&data).unwrap();
        assert_eq!(archive.file_count(), 3);

        assert_eq!(
            archive.extract_file(&data, "res://a.txt").unwrap(),
            b"alpha"
        );
        assert_eq!(archive.extract_file(&data, "res://b.txt").unwrap(), b"beta");
        assert_eq!(
            archive.extract_file(&data, "res://sub/c.txt").unwrap(),
            b"gamma"
        );
    }

    #[test]
    fn file_paths_sorted() {
        let mut packer = PckPacker::new();
        packer.add_file("res://z.txt", vec![]);
        packer.add_file("res://a.txt", vec![]);
        packer.add_file("res://m.txt", vec![]);
        let data = packer.pack().unwrap();

        let archive = PckArchive::from_bytes(&data).unwrap();
        let paths = archive.file_paths();
        assert_eq!(paths, vec!["res://a.txt", "res://m.txt", "res://z.txt"]);
    }

    #[test]
    fn header_version_preserved() {
        let packer = PckPacker::new().with_version(5, 0, 0);
        let data = packer.pack().unwrap();
        let archive = PckArchive::from_bytes(&data).unwrap();
        assert_eq!(archive.header.ver_major, 5);
        assert_eq!(archive.header.ver_minor, 0);
        assert_eq!(archive.header.ver_patch, 0);
    }

    #[test]
    fn invalid_magic_rejected() {
        let mut data = PckPacker::new().pack().unwrap();
        data[0] = b'X'; // corrupt magic
        let result = PckArchive::from_bytes(&data);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("invalid PCK magic"));
    }

    #[test]
    fn truncated_data_rejected() {
        let result = PckArchive::from_bytes(&[0u8; 10]);
        assert!(result.is_err());
    }

    #[test]
    fn extract_nonexistent_file_returns_none() {
        let packer = PckPacker::new();
        let data = packer.pack().unwrap();
        let archive = PckArchive::from_bytes(&data).unwrap();
        assert!(archive.extract_file(&data, "res://nope.txt").is_none());
    }

    #[test]
    fn entry_offsets_are_aligned() {
        let mut packer = PckPacker::new();
        packer.add_file("res://small.txt", b"hi".to_vec());
        packer.add_file("res://other.txt", b"there".to_vec());
        let data = packer.pack().unwrap();

        let archive = PckArchive::from_bytes(&data).unwrap();
        for entry in archive.entries.values() {
            assert_eq!(
                entry.offset % PCK_DATA_ALIGNMENT,
                0,
                "entry {} offset {} not aligned to {}",
                entry.path,
                entry.offset,
                PCK_DATA_ALIGNMENT
            );
        }
    }

    #[test]
    fn total_data_size() {
        let mut packer = PckPacker::new();
        packer.add_file("res://a.bin", vec![0u8; 100]);
        packer.add_file("res://b.bin", vec![0u8; 200]);
        let data = packer.pack().unwrap();
        let archive = PckArchive::from_bytes(&data).unwrap();
        assert_eq!(archive.total_data_size(), 300);
    }

    #[test]
    fn empty_file_content() {
        let mut packer = PckPacker::new();
        packer.add_file("res://empty.txt", vec![]);
        let data = packer.pack().unwrap();
        let archive = PckArchive::from_bytes(&data).unwrap();
        let content = archive.extract_file(&data, "res://empty.txt").unwrap();
        assert!(content.is_empty());
    }

    #[test]
    fn large_file_roundtrip() {
        let big = vec![0xAB_u8; 10_000];
        let mut packer = PckPacker::new();
        packer.add_file("res://big.bin", big.clone());
        let data = packer.pack().unwrap();
        let archive = PckArchive::from_bytes(&data).unwrap();
        let content = archive.extract_file(&data, "res://big.bin").unwrap();
        assert_eq!(content.len(), 10_000);
        assert_eq!(content[0], 0xAB);
        assert_eq!(content[9_999], 0xAB);
    }

    #[test]
    fn packer_file_count() {
        let mut packer = PckPacker::new();
        assert_eq!(packer.file_count(), 0);
        packer.add_file("res://x", vec![]);
        assert_eq!(packer.file_count(), 1);
        packer.add_file("res://y", vec![]);
        assert_eq!(packer.file_count(), 2);
    }

    #[test]
    fn get_entry_returns_metadata() {
        let mut packer = PckPacker::new();
        packer.add_file("res://test.gd", b"extends Node".to_vec());
        let data = packer.pack().unwrap();
        let archive = PckArchive::from_bytes(&data).unwrap();
        let entry = archive.get_entry("res://test.gd").unwrap();
        assert_eq!(entry.path, "res://test.gd");
        assert_eq!(entry.size, 12);
    }

    #[test]
    fn md5_field_is_zero_by_default() {
        let mut packer = PckPacker::new();
        packer.add_file("res://f.txt", b"data".to_vec());
        let data = packer.pack().unwrap();
        let archive = PckArchive::from_bytes(&data).unwrap();
        let entry = archive.get_entry("res://f.txt").unwrap();
        assert_eq!(entry.md5, [0u8; 16]);
    }

    #[test]
    fn has_file_check() {
        let mut packer = PckPacker::new();
        packer.add_file("res://yes.txt", vec![]);
        let data = packer.pack().unwrap();
        let archive = PckArchive::from_bytes(&data).unwrap();
        assert!(archive.has_file("res://yes.txt"));
        assert!(!archive.has_file("res://no.txt"));
    }

    #[test]
    fn pad_to_4_helper() {
        assert_eq!(pad_to_4(0), 0);
        assert_eq!(pad_to_4(1), 4);
        assert_eq!(pad_to_4(3), 4);
        assert_eq!(pad_to_4(4), 4);
        assert_eq!(pad_to_4(5), 8);
    }

    #[test]
    fn align_up_helper() {
        assert_eq!(align_up(0, 64), 0);
        assert_eq!(align_up(1, 64), 64);
        assert_eq!(align_up(64, 64), 64);
        assert_eq!(align_up(65, 64), 128);
        assert_eq!(align_up(100, 0), 100); // zero alignment passthrough
    }

    // -----------------------------------------------------------------------
    // Godot 4.6 format-compatibility tests (V2/V3, embedding, encryption)
    // -----------------------------------------------------------------------

    #[test]
    fn writer_emits_file_base_and_entry_flags() {
        // Verify the V2 header carries a file_base and each entry a trailing
        // flags u32, by decoding the raw bytes the writer produced.
        let mut packer = PckPacker::new();
        packer.add_file("res://a.txt", b"hello".to_vec());
        let data = packer.pack().unwrap();

        // magic(4) + version(4) + 3×ver(12) + pack_flags(4) = offset 24 → file_base.
        assert_eq!(&data[0..4], PCK_MAGIC);
        assert_eq!(u32::from_le_bytes(data[4..8].try_into().unwrap()), 2); // V2
        let pack_flags = u32::from_le_bytes(data[20..24].try_into().unwrap());
        assert_eq!(pack_flags, PACK_REL_FILEBASE);
        let file_base = u64::from_le_bytes(data[24..32].try_into().unwrap());
        // Header is 100 bytes; with one 12-byte-padded entry (52 bytes) the
        // directory ends at 152, aligned up to 192 by the default 64 alignment.
        assert_eq!(file_base % PCK_DATA_ALIGNMENT, 0);
        assert!(file_base >= 152);

        // Re-reading yields the correct absolute offset and contents.
        let archive = PckArchive::from_bytes(&data).unwrap();
        let entry = archive.get_entry("res://a.txt").unwrap();
        assert_eq!(entry.offset, file_base); // first file sits at file_base + 0
        assert_eq!(
            archive.extract_file(&data, "res://a.txt").unwrap(),
            b"hello"
        );
    }

    #[test]
    fn v2_byte_exact_fixture_parses() {
        // Hand-built Godot V2 pack: single file "res://a.txt" -> "hello".
        // pack_flags = 0 → absolute file_base (reader uses it as-is at pck_start 0).
        let mut d: Vec<u8> = Vec::new();
        d.extend_from_slice(b"GDPC"); //            [0..4]   magic
        d.extend_from_slice(&2u32.to_le_bytes()); //  [4..8]   version = 2
        d.extend_from_slice(&4u32.to_le_bytes()); //  [8..12]  ver_major = 4
        d.extend_from_slice(&6u32.to_le_bytes()); //  [12..16] ver_minor = 6
        d.extend_from_slice(&1u32.to_le_bytes()); //  [16..20] ver_patch = 1
        d.extend_from_slice(&0u32.to_le_bytes()); //  [20..24] pack_flags = 0
        d.extend_from_slice(&152u64.to_le_bytes()); // [24..32] file_base = 152 (abs)
        for _ in 0..16 {
            d.extend_from_slice(&0u32.to_le_bytes()); // [32..96] reserved 16×u32
        }
        d.extend_from_slice(&1u32.to_le_bytes()); //  [96..100] file_count = 1
                                                  // ---- entry ----
        d.extend_from_slice(&12u32.to_le_bytes()); // [100..104] path_len = 12 (11+NUL)
        d.extend_from_slice(b"res://a.txt\0"); //     [104..116] path (NUL-padded)
        d.extend_from_slice(&0u64.to_le_bytes()); //  [116..124] offset = 0 (rel file_base)
        d.extend_from_slice(&5u64.to_le_bytes()); //  [124..132] size = 5
        d.extend_from_slice(&[0u8; 16]); //           [132..148] md5 = zeros
        d.extend_from_slice(&0u32.to_le_bytes()); //  [148..152] flags = 0
        d.extend_from_slice(b"hello"); //             [152..157] file data
        assert_eq!(d.len(), 157);

        let archive = PckArchive::from_bytes(&d).unwrap();
        assert_eq!(archive.header.format_version, 2);
        assert_eq!(archive.file_count(), 1);
        let entry = archive.get_entry("res://a.txt").unwrap();
        assert_eq!(entry.offset, 152); // file_base(152) + 0
        assert_eq!(entry.size, 5);
        assert_eq!(archive.extract_file(&d, "res://a.txt").unwrap(), b"hello");
    }

    #[test]
    fn v3_byte_exact_fixture_parses() {
        // Hand-built Godot V3 pack: file data precedes the directory, which is
        // located via the dir_offset field. file_base and dir_offset are both
        // relative to the pack start (pck_start = 0 here).
        let mut d: Vec<u8> = Vec::new();
        d.extend_from_slice(b"GDPC"); //             [0..4]   magic
        d.extend_from_slice(&3u32.to_le_bytes()); //   [4..8]   version = 3
        d.extend_from_slice(&4u32.to_le_bytes()); //   [8..12]  ver_major = 4
        d.extend_from_slice(&6u32.to_le_bytes()); //   [12..16] ver_minor = 6
        d.extend_from_slice(&1u32.to_le_bytes()); //   [16..20] ver_patch = 1
        d.extend_from_slice(&2u32.to_le_bytes()); //   [20..24] pack_flags = PACK_REL_FILEBASE
        d.extend_from_slice(&104u64.to_le_bytes()); // [24..32] file_base = 104
        d.extend_from_slice(&109u64.to_le_bytes()); // [32..40] dir_offset = 109
        for _ in 0..16 {
            d.extend_from_slice(&0u32.to_le_bytes()); // [40..104] reserved (skipped by seek)
        }
        d.extend_from_slice(b"hello"); //             [104..109] file data at file_base
                                       // ---- directory at dir_offset = 109 ----
        d.extend_from_slice(&1u32.to_le_bytes()); //  [109..113] file_count = 1
        d.extend_from_slice(&12u32.to_le_bytes()); // [113..117] path_len = 12
        d.extend_from_slice(b"res://a.txt\0"); //     [117..129] path
        d.extend_from_slice(&0u64.to_le_bytes()); //  [129..137] offset = 0 (rel file_base)
        d.extend_from_slice(&5u64.to_le_bytes()); //  [137..145] size = 5
        d.extend_from_slice(&[0u8; 16]); //           [145..161] md5 = zeros
        d.extend_from_slice(&0u32.to_le_bytes()); //  [161..165] flags = 0
        assert_eq!(d.len(), 165);

        let archive = PckArchive::from_bytes(&d).unwrap();
        assert_eq!(archive.header.format_version, 3);
        assert_eq!(archive.file_count(), 1);
        let entry = archive.get_entry("res://a.txt").unwrap();
        assert_eq!(entry.offset, 104); // file_base(104) + 0
        assert_eq!(archive.extract_file(&d, "res://a.txt").unwrap(), b"hello");
    }

    #[test]
    fn embedded_pck_end_trailer_scan() {
        // A pack appended to an executable is found via the trailer
        // `[data_size: u64][magic]`. Because the writer uses PACK_REL_FILEBASE,
        // the entry offsets remain correct at the non-zero embed offset.
        let mut packer = PckPacker::new();
        packer.add_file("res://embedded.txt", b"payload".to_vec());
        let pck = packer.pack().unwrap();

        let mut buf = vec![0xEEu8; 40]; // pretend executable bytes
        buf.extend_from_slice(&pck);
        buf.extend_from_slice(&(pck.len() as u64).to_le_bytes()); // data_size
        buf.extend_from_slice(PCK_MAGIC); // trailer magic

        let archive = PckArchive::from_bytes(&buf).unwrap();
        assert!(archive.has_file("res://embedded.txt"));
        assert_eq!(
            archive.extract_file(&buf, "res://embedded.txt").unwrap(),
            b"payload"
        );
    }

    #[test]
    fn unsupported_version_rejected() {
        let mut d: Vec<u8> = Vec::new();
        d.extend_from_slice(b"GDPC");
        d.extend_from_slice(&99u32.to_le_bytes()); // bogus version
        d.extend_from_slice(&4u32.to_le_bytes());
        d.extend_from_slice(&6u32.to_le_bytes());
        d.extend_from_slice(&1u32.to_le_bytes());
        d.extend_from_slice(&0u32.to_le_bytes());
        d.extend_from_slice(&0u64.to_le_bytes());
        let err = PckArchive::from_bytes(&d).unwrap_err();
        assert!(
            err.to_string().contains("unsupported PCK version"),
            "got: {err}"
        );
    }

    #[test]
    fn truncated_header_after_magic_errors() {
        // Magic present, but the header is cut off before the version fields.
        let d = b"GDPC\x02\x00\x00\x00".to_vec(); // magic + version only
        assert!(PckArchive::from_bytes(&d).is_err());
    }

    #[test]
    fn encrypted_directory_flag_is_unsupported() {
        let mut d: Vec<u8> = Vec::new();
        d.extend_from_slice(b"GDPC");
        d.extend_from_slice(&2u32.to_le_bytes()); // V2
        d.extend_from_slice(&4u32.to_le_bytes());
        d.extend_from_slice(&6u32.to_le_bytes());
        d.extend_from_slice(&1u32.to_le_bytes());
        d.extend_from_slice(&PACK_DIR_ENCRYPTED.to_le_bytes()); // pack_flags
        d.extend_from_slice(&0u64.to_le_bytes()); // file_base
        let err = PckArchive::from_bytes(&d).unwrap_err();
        assert!(
            err.to_string().contains("encrypted PCK directory"),
            "got: {err}"
        );
    }

    #[test]
    fn encrypted_file_flag_is_unsupported() {
        let mut d: Vec<u8> = Vec::new();
        d.extend_from_slice(b"GDPC");
        d.extend_from_slice(&2u32.to_le_bytes()); // V2
        d.extend_from_slice(&4u32.to_le_bytes());
        d.extend_from_slice(&6u32.to_le_bytes());
        d.extend_from_slice(&1u32.to_le_bytes());
        d.extend_from_slice(&0u32.to_le_bytes()); // pack_flags = 0
        d.extend_from_slice(&152u64.to_le_bytes()); // file_base
        for _ in 0..16 {
            d.extend_from_slice(&0u32.to_le_bytes());
        }
        d.extend_from_slice(&1u32.to_le_bytes()); // file_count = 1
        d.extend_from_slice(&12u32.to_le_bytes()); // path_len
        d.extend_from_slice(b"res://a.txt\0");
        d.extend_from_slice(&0u64.to_le_bytes()); // offset
        d.extend_from_slice(&5u64.to_le_bytes()); // size
        d.extend_from_slice(&[0u8; 16]); // md5
        d.extend_from_slice(&PACK_FILE_ENCRYPTED.to_le_bytes()); // flags = encrypted
        d.extend_from_slice(b"hello");
        let err = PckArchive::from_bytes(&d).unwrap_err();
        assert!(
            err.to_string().contains("encrypted PCK file entry"),
            "got: {err}"
        );
    }

    #[test]
    fn file_removal_entry_is_skipped() {
        // A patch-pack removal record must not appear as a readable file.
        let mut d: Vec<u8> = Vec::new();
        d.extend_from_slice(b"GDPC");
        d.extend_from_slice(&2u32.to_le_bytes());
        d.extend_from_slice(&4u32.to_le_bytes());
        d.extend_from_slice(&6u32.to_le_bytes());
        d.extend_from_slice(&1u32.to_le_bytes());
        d.extend_from_slice(&0u32.to_le_bytes());
        d.extend_from_slice(&0u64.to_le_bytes()); // file_base (unused, size 0)
        for _ in 0..16 {
            d.extend_from_slice(&0u32.to_le_bytes());
        }
        d.extend_from_slice(&1u32.to_le_bytes()); // file_count = 1
        d.extend_from_slice(&12u32.to_le_bytes());
        d.extend_from_slice(b"res://a.txt\0");
        d.extend_from_slice(&0u64.to_le_bytes());
        d.extend_from_slice(&0u64.to_le_bytes());
        d.extend_from_slice(&[0u8; 16]);
        d.extend_from_slice(&PACK_FILE_REMOVAL.to_le_bytes()); // removal flag
        let archive = PckArchive::from_bytes(&d).unwrap();
        assert_eq!(archive.file_count(), 0);
        assert!(!archive.has_file("res://a.txt"));
    }

    #[test]
    fn packer_multi_file_offsets_relative_to_file_base() {
        // Sanity: the packer's V2 output roundtrips with multiple files and the
        // stored offsets are relative to file_base (i.e. the first is 0).
        let mut packer = PckPacker::new();
        packer.add_file("res://a.txt", b"alpha".to_vec());
        packer.add_file("res://b.txt", b"beta!".to_vec());
        let data = packer.pack().unwrap();

        let file_base = u64::from_le_bytes(data[24..32].try_into().unwrap());
        let archive = PckArchive::from_bytes(&data).unwrap();
        let a = archive.get_entry("res://a.txt").unwrap();
        assert_eq!(a.offset, file_base); // first file: relative offset 0
        assert_eq!(
            archive.extract_file(&data, "res://a.txt").unwrap(),
            b"alpha"
        );
        assert_eq!(
            archive.extract_file(&data, "res://b.txt").unwrap(),
            b"beta!"
        );
    }

    // -----------------------------------------------------------------------
    // PckResourceLoader tests
    // -----------------------------------------------------------------------

    #[test]
    fn pck_loader_loads_tres_resource() {
        let tres_content = r#"[gd_resource type="Resource" format=3]

[resource]
name = "FromPCK"
value = 99
"#;
        let mut packer = PckPacker::new();
        packer.add_file("res://item.tres", tres_content.as_bytes().to_vec());
        let data = packer.pack().unwrap();

        let loader = PckResourceLoader::from_bytes(data).unwrap();
        let res = loader.load("res://item.tres").unwrap();
        assert_eq!(res.class_name, "Resource");
        assert_eq!(res.path, "res://item.tres");
    }

    #[test]
    fn pck_loader_nonexistent_file_returns_error() {
        let packer = PckPacker::new();
        let data = packer.pack().unwrap();
        let loader = PckResourceLoader::from_bytes(data).unwrap();
        let result = loader.load("res://missing.tres");
        assert!(result.is_err());
    }

    #[test]
    fn pck_loader_loads_non_tres_as_packed_file() {
        let mut packer = PckPacker::new();
        packer.add_file("res://icon.png", vec![0x89, 0x50, 0x4E, 0x47]);
        let data = packer.pack().unwrap();

        let loader = PckResourceLoader::from_bytes(data).unwrap();
        let res = loader.load("res://icon.png").unwrap();
        assert_eq!(res.class_name, "PackedFile");
        assert_eq!(
            res.get_property("byte_size"),
            Some(&gdvariant::Variant::Int(4))
        );
    }

    #[test]
    fn pck_loader_extract_raw_returns_bytes() {
        let content = b"raw binary data";
        let mut packer = PckPacker::new();
        packer.add_file("res://data.bin", content.to_vec());
        let data = packer.pack().unwrap();

        let loader = PckResourceLoader::from_bytes(data).unwrap();
        assert_eq!(loader.extract_raw("res://data.bin").unwrap(), content);
        assert!(loader.extract_raw("res://nope.bin").is_none());
    }

    #[test]
    fn pck_loader_has_file_and_count() {
        let mut packer = PckPacker::new();
        packer.add_file("res://a.txt", vec![]);
        packer.add_file("res://b.txt", vec![]);
        let data = packer.pack().unwrap();

        let loader = PckResourceLoader::from_bytes(data).unwrap();
        assert!(loader.has_file("res://a.txt"));
        assert!(!loader.has_file("res://c.txt"));
        assert_eq!(loader.file_count(), 2);
    }

    #[test]
    fn pck_loader_file_paths() {
        let mut packer = PckPacker::new();
        packer.add_file("res://z.gd", vec![]);
        packer.add_file("res://a.gd", vec![]);
        let data = packer.pack().unwrap();

        let loader = PckResourceLoader::from_bytes(data).unwrap();
        let paths = loader.file_paths();
        assert_eq!(paths, vec!["res://a.gd", "res://z.gd"]);
    }

    #[test]
    fn pck_loader_header_metadata() {
        let packer = PckPacker::new().with_version(4, 6, 1);
        let data = packer.pack().unwrap();
        let loader = PckResourceLoader::from_bytes(data).unwrap();
        assert_eq!(loader.header().ver_major, 4);
        assert_eq!(loader.header().ver_minor, 6);
        assert_eq!(loader.header().ver_patch, 1);
    }

    #[test]
    fn pck_loader_tscn_parsed_as_text() {
        let tscn_content = r#"[gd_scene format=3 uid="uid://test123"]

[node name="Root" type="Node2D"]
"#;
        let mut packer = PckPacker::new();
        packer.add_file("res://level.tscn", tscn_content.as_bytes().to_vec());
        let data = packer.pack().unwrap();

        let loader = PckResourceLoader::from_bytes(data).unwrap();
        let res = loader.load("res://level.tscn").unwrap();
        assert_eq!(res.path, "res://level.tscn");
    }

    #[test]
    fn pck_loader_multiple_tres_resources() {
        let tres_a = r#"[gd_resource type="Resource" format=3]

[resource]
name = "Alpha"
"#;
        let tres_b = r#"[gd_resource type="Resource" format=3]

[resource]
name = "Beta"
"#;
        let mut packer = PckPacker::new();
        packer.add_file("res://a.tres", tres_a.as_bytes().to_vec());
        packer.add_file("res://b.tres", tres_b.as_bytes().to_vec());
        let data = packer.pack().unwrap();

        let loader = PckResourceLoader::from_bytes(data).unwrap();
        let a = loader.load("res://a.tres").unwrap();
        let b = loader.load("res://b.tres").unwrap();
        assert_ne!(a.path, b.path);
    }

    #[test]
    fn pck_loader_with_cache_deduplicates() {
        use crate::cache::ResourceCache;

        let tres = r#"[gd_resource type="Resource" format=3]

[resource]
name = "Cached"
"#;
        let mut packer = PckPacker::new();
        packer.add_file("res://cached.tres", tres.as_bytes().to_vec());
        let data = packer.pack().unwrap();

        let loader = PckResourceLoader::from_bytes(data).unwrap();
        let mut cache = ResourceCache::new(loader);
        let first = cache.load("res://cached.tres").unwrap();
        let second = cache.load("res://cached.tres").unwrap();
        assert!(Arc::ptr_eq(&first, &second));
    }

    #[test]
    fn pck_loader_with_unified_loader() {
        use crate::unified::UnifiedLoader;

        let tres = r#"[gd_resource type="Resource" format=3]

[resource]
name = "Unified"
"#;
        let mut packer = PckPacker::new();
        packer.add_file("res://unified.tres", tres.as_bytes().to_vec());
        let data = packer.pack().unwrap();

        let loader = PckResourceLoader::from_bytes(data).unwrap();
        let mut unified = UnifiedLoader::new(loader);
        unified.register_uid_str("uid://unifiedref", "res://unified.tres");

        let by_path = unified.load("res://unified.tres").unwrap();
        let by_uid = unified.load("uid://unifiedref").unwrap();
        assert!(Arc::ptr_eq(&by_path, &by_uid));
    }

    #[test]
    fn pck_loader_invalid_utf8_tres_returns_error() {
        let mut packer = PckPacker::new();
        // Invalid UTF-8 bytes
        packer.add_file("res://bad.tres", vec![0xFF, 0xFE, 0x80, 0x81]);
        let data = packer.pack().unwrap();

        let loader = PckResourceLoader::from_bytes(data).unwrap();
        let result = loader.load("res://bad.tres");
        assert!(result.is_err());
    }

    #[test]
    fn pck_loader_invalid_archive_bytes() {
        let result = PckResourceLoader::from_bytes(vec![0, 1, 2, 3]);
        assert!(result.is_err());
    }

    // -----------------------------------------------------------------------
    // PckMountManager tests
    // -----------------------------------------------------------------------

    fn make_pck(files: &[(&str, &str)]) -> Vec<u8> {
        let mut packer = PckPacker::new();
        for &(path, content) in files {
            packer.add_file(path, content.as_bytes().to_vec());
        }
        packer.pack().unwrap()
    }

    #[test]
    fn mount_manager_empty() {
        let mgr = PckMountManager::new();
        assert_eq!(mgr.mount_count(), 0);
        assert!(mgr.mounted_labels().is_empty());
        assert!(!mgr.has_file("res://anything"));
    }

    #[test]
    fn mount_and_load_resource() {
        let tres = r#"[gd_resource type="Resource" format=3]

[resource]
name = "Mounted"
"#;
        let data = make_pck(&[("res://item.tres", tres)]);
        let mut mgr = PckMountManager::new();
        mgr.mount("base", data, 0).unwrap();

        assert_eq!(mgr.mount_count(), 1);
        assert!(mgr.has_file("res://item.tres"));

        let res = mgr.load("res://item.tres").unwrap();
        assert_eq!(res.class_name, "Resource");
    }

    #[test]
    fn unmount_removes_archive() {
        let data = make_pck(&[("res://a.txt", "hello")]);
        let mut mgr = PckMountManager::new();
        mgr.mount("pack1", data, 0).unwrap();
        assert!(mgr.has_file("res://a.txt"));

        assert!(mgr.unmount("pack1"));
        assert!(!mgr.has_file("res://a.txt"));
        assert_eq!(mgr.mount_count(), 0);
    }

    #[test]
    fn unmount_nonexistent_returns_false() {
        let mut mgr = PckMountManager::new();
        assert!(!mgr.unmount("nope"));
    }

    #[test]
    fn higher_priority_wins() {
        let tres_low = r#"[gd_resource type="Resource" format=3]

[resource]
name = "Low"
"#;
        let tres_high = r#"[gd_resource type="Resource" format=3]

[resource]
name = "High"
"#;
        let data_low = make_pck(&[("res://item.tres", tres_low)]);
        let data_high = make_pck(&[("res://item.tres", tres_high)]);

        let mut mgr = PckMountManager::new();
        mgr.mount("low", data_low, 0).unwrap();
        mgr.mount("high", data_high, 10).unwrap();

        let res = mgr.load("res://item.tres").unwrap();
        assert_eq!(
            res.get_property("name"),
            Some(&gdvariant::Variant::String("High".into()))
        );
    }

    #[test]
    fn priority_order_labels() {
        let data1 = make_pck(&[("res://a.txt", "a")]);
        let data2 = make_pck(&[("res://b.txt", "b")]);
        let data3 = make_pck(&[("res://c.txt", "c")]);

        let mut mgr = PckMountManager::new();
        mgr.mount("low", data1, 0).unwrap();
        mgr.mount("high", data2, 100).unwrap();
        mgr.mount("mid", data3, 50).unwrap();

        assert_eq!(mgr.mounted_labels(), vec!["high", "mid", "low"]);
    }

    #[test]
    fn remount_replaces_same_label() {
        let data1 = make_pck(&[("res://old.txt", "old")]);
        let data2 = make_pck(&[("res://new.txt", "new")]);

        let mut mgr = PckMountManager::new();
        mgr.mount("pack", data1, 0).unwrap();
        assert!(mgr.has_file("res://old.txt"));

        mgr.mount("pack", data2, 0).unwrap();
        assert!(!mgr.has_file("res://old.txt"));
        assert!(mgr.has_file("res://new.txt"));
        assert_eq!(mgr.mount_count(), 1);
    }

    #[test]
    fn load_not_found_in_any_mount() {
        let data = make_pck(&[("res://exists.txt", "yes")]);
        let mut mgr = PckMountManager::new();
        mgr.mount("base", data, 0).unwrap();

        let err = mgr.load("res://missing.tres").unwrap_err();
        assert!(err.to_string().contains("not found in any mounted PCK"));
    }

    #[test]
    fn which_mount_returns_highest_priority() {
        let data1 = make_pck(&[("res://shared.txt", "from-low")]);
        let data2 = make_pck(&[("res://shared.txt", "from-high")]);

        let mut mgr = PckMountManager::new();
        mgr.mount("low", data1, 0).unwrap();
        mgr.mount("high", data2, 10).unwrap();

        assert_eq!(mgr.which_mount("res://shared.txt"), Some("high"));
        assert_eq!(mgr.which_mount("res://missing"), None);
    }

    #[test]
    fn all_files_across_mounts() {
        let data1 = make_pck(&[("res://a.txt", "a"), ("res://shared.txt", "1")]);
        let data2 = make_pck(&[("res://b.txt", "b"), ("res://shared.txt", "2")]);

        let mut mgr = PckMountManager::new();
        mgr.mount("first", data1, 0).unwrap();
        mgr.mount("second", data2, 0).unwrap();

        let files = mgr.all_files();
        assert!(files.contains(&"res://a.txt".to_string()));
        assert!(files.contains(&"res://b.txt".to_string()));
        assert!(files.contains(&"res://shared.txt".to_string()));
        // Deduplicated.
        assert_eq!(files.len(), 3);
    }

    #[test]
    fn extract_raw_from_priority_mount() {
        let data1 = make_pck(&[("res://data.bin", "low-content")]);
        let data2 = make_pck(&[("res://data.bin", "high-content")]);

        let mut mgr = PckMountManager::new();
        mgr.mount("low", data1, 0).unwrap();
        mgr.mount("high", data2, 10).unwrap();

        let raw = mgr.extract_raw("res://data.bin").unwrap();
        assert_eq!(raw, b"high-content");
    }

    #[test]
    fn mount_manager_with_unified_loader() {
        use crate::UnifiedLoader;

        let tres = r#"[gd_resource type="Resource" format=3]

[resource]
name = "FromMount"
"#;
        let data = make_pck(&[("res://mounted.tres", tres)]);
        let mut mgr = PckMountManager::new();
        mgr.mount("base", data, 0).unwrap();

        // Use mount manager as the backing loader for UnifiedLoader.
        let mut unified = UnifiedLoader::new(mgr);
        unified.register_uid_str("uid://mountedref", "res://mounted.tres");

        let by_path = unified.load("res://mounted.tres").unwrap();
        let by_uid = unified.load("uid://mountedref").unwrap();
        assert!(Arc::ptr_eq(&by_path, &by_uid));
        assert_eq!(
            by_path.get_property("name"),
            Some(&gdvariant::Variant::String("FromMount".into()))
        );
    }

    #[test]
    fn mount_invalid_data_fails() {
        let mut mgr = PckMountManager::new();
        let result = mgr.mount("bad", vec![0, 1, 2, 3], 0);
        assert!(result.is_err());
        assert_eq!(mgr.mount_count(), 0);
    }
}

#[cfg(test)]
mod mount_tests {
    use super::*;

    /// Builds a pck whose directory carries a `PACK_FILE_REMOVAL` record for
    /// `removed_path`, which `PckPacker` has no API to emit. Written by hand so
    /// the parser's removal branch is exercised against real bytes rather than a
    /// synthesised `PckArchive` (pat-0q2kx.134).
    fn pack_with_removal(present: (&str, &[u8]), removed_path: &str) -> Vec<u8> {
        // The removal is packed as its own single-entry archive and layered by
        // mount precedence — simpler and far less brittle than splicing an extra
        // entry into an existing directory and fixing up every offset.
        let _ = present;
        let mut only_removal = PckPacker::new();
        only_removal.add_file(removed_path, Vec::new());
        let mut bytes = only_removal.pack().expect("pack");
        // Flip the entry's flags word to PACK_FILE_REMOVAL. The entry layout is
        // path_len(4) + padded path + offset(8) + size(8) + md5(16) + flags(4),
        // so the flags are the last 4 bytes before the (empty) data section.
        let flags_pos = find_removal_flags_offset(&bytes, removed_path);
        bytes[flags_pos..flags_pos + 4].copy_from_slice(&PACK_FILE_REMOVAL.to_le_bytes());
        bytes
    }

    /// Locates the `flags` u32 of `path`'s directory entry in a freshly packed
    /// single-file archive.
    ///
    /// The padded path length is read from the entry's own `path_len` field —
    /// `PckPacker` writes the *already padded* length there — rather than
    /// scanning for NUL padding, which would run into the following `offset`
    /// whenever that field's low byte happens to be zero.
    fn find_removal_flags_offset(bytes: &[u8], path: &str) -> usize {
        let needle = path.as_bytes();
        let at = bytes
            .windows(needle.len())
            .position(|w| w == needle)
            .expect("path present in directory");
        let len_field = at - 4;
        let padded_len = u32::from_le_bytes([
            bytes[len_field],
            bytes[len_field + 1],
            bytes[len_field + 2],
            bytes[len_field + 3],
        ]) as usize;
        assert!(
            padded_len >= needle.len(),
            "path_len field must cover the path",
        );
        // path_len(4) + padded path + offset(8) + size(8) + md5(16) → flags.
        at + padded_len + 8 + 8 + 16
    }

    fn reset() {
        gdcore::file_access::clear_mounted_packs();
    }

    #[test]
    fn mounted_pack_serves_res_reads_without_a_disk_file() {
        reset();
        let mut packer = PckPacker::new();
        packer.add_file("res://icon.png", b"PNGDATA".to_vec());
        let bytes = packer.pack().expect("pack");

        let mount = PckMount::from_bytes(bytes).expect("valid pck");
        gdcore::file_access::mount_pack("game", Arc::new(mount), 0);

        assert!(
            FileAccess::file_exists("res://icon.png"),
            "a packed file must satisfy FileAccess.file_exists with nothing on disk",
        );
        assert_eq!(
            FileAccess::read_file_bytes("res://icon.png").expect("read"),
            b"PNGDATA".to_vec(),
        );
        assert!(
            !FileAccess::file_exists("res://absent.png"),
            "a path in no pack must not report as existing",
        );
        reset();
    }

    #[test]
    fn higher_priority_mount_overrides_a_lower_one() {
        reset();
        let mut base = PckPacker::new();
        base.add_file("res://data.txt", b"base".to_vec());
        let mut patch = PckPacker::new();
        patch.add_file("res://data.txt", b"patch".to_vec());

        gdcore::file_access::mount_pack(
            "base",
            Arc::new(PckMount::from_bytes(base.pack().unwrap()).unwrap()),
            0,
        );
        gdcore::file_access::mount_pack(
            "patch",
            Arc::new(PckMount::from_bytes(patch.pack().unwrap()).unwrap()),
            10,
        );

        assert_eq!(
            FileAccess::read_file_string("res://data.txt").unwrap(),
            "patch",
            "the higher-priority mount must win",
        );
        assert!(gdcore::file_access::unmount_pack("patch"));
        assert_eq!(
            FileAccess::read_file_string("res://data.txt").unwrap(),
            "base",
            "unmounting the patch must fall back to the base pack",
        );
        reset();
    }

    #[test]
    fn a_removal_entry_hides_a_lower_mounts_file() {
        reset();
        let mut base = PckPacker::new();
        base.add_file("res://gone.txt", b"still here".to_vec());
        gdcore::file_access::mount_pack(
            "base",
            Arc::new(PckMount::from_bytes(base.pack().unwrap()).unwrap()),
            0,
        );
        assert!(FileAccess::file_exists("res://gone.txt"));

        let patch = pack_with_removal(("res://kept.txt", b"kept"), "res://gone.txt");
        let mount = PckMount::from_bytes(patch).expect("valid pck");
        assert_eq!(
            mount.removed_files(),
            vec!["res://gone.txt".to_string()],
            "the removal record must survive parsing — it used to be discarded",
        );
        gdcore::file_access::mount_pack("patch", Arc::new(mount), 10);

        assert!(
            !FileAccess::file_exists("res://gone.txt"),
            "a patch pack's removal entry must hide the base pack's copy",
        );
        assert!(
            FileAccess::read_file_bytes("res://gone.txt").is_err(),
            "reading a removed path must fail rather than serve the stale bytes",
        );
        reset();
    }

    #[test]
    fn non_res_paths_are_never_served_from_packs() {
        reset();
        let mut packer = PckPacker::new();
        packer.add_file("res://x.txt", b"x".to_vec());
        gdcore::file_access::mount_pack(
            "game",
            Arc::new(PckMount::from_bytes(packer.pack().unwrap()).unwrap()),
            0,
        );
        // `user://` is writable state, never packed; a plain path is the host FS.
        assert!(gdcore::file_access::read_from_packs("user://x.txt").is_none());
        assert!(gdcore::file_access::read_from_packs("/tmp/x.txt").is_none());
        reset();
    }
}
