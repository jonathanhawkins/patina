//! PCK (Packed) file format packer and unpacker.
//!
//! Implements Godot's `.pck` archive format used for distributing game
//! resources as a single file. The format stores files with their
//! `res://` paths, sizes, offsets, and optional MD5 checksums.
//!
//! # Format Overview
//!
//! ```text
//! Header:
//!   magic:       4 bytes  "GDPC"
//!   version:     4 bytes  LE u32  (2 for Godot 4.x)
//!   ver_major:   4 bytes  LE u32
//!   ver_minor:   4 bytes  LE u32
//!   ver_patch:   4 bytes  LE u32
//!   flags:       4 bytes  LE u32
//!   reserved:    64 bytes (16 × u32, all zero)
//!   file_count:  4 bytes  LE u32
//!
//! For each file:
//!   path_len:    4 bytes  LE u32  (byte length of path, padded to 4-byte boundary)
//!   path:        path_len bytes   (NUL-padded to alignment)
//!   offset:      8 bytes  LE u64
//!   size:        8 bytes  LE u64
//!   md5:         16 bytes         (MD5 digest, or zeroes)
//!
//! File data follows the directory block, each aligned to 64 bytes.
//! ```

use std::collections::BTreeMap;
use std::io::{self, Cursor, Read, Seek, Write};
use std::sync::Arc;

use gdcore::error::{EngineError, EngineResult};

use crate::loader::{ResourceLoader, TresLoader};
use crate::resource::Resource;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// The magic header bytes for a PCK file.
pub const PCK_MAGIC: &[u8; 4] = b"GDPC";

/// PCK format version for Godot 4.x.
pub const PCK_FORMAT_VERSION: u32 = 2;

/// Default alignment for file data within the archive.
pub const PCK_DATA_ALIGNMENT: u64 = 64;

/// Size of the reserved header area (16 × u32).
const RESERVED_FIELD_COUNT: usize = 16;

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
}

impl PckArchive {
    /// Parses a PCK archive from a byte slice, reading the header and
    /// directory but not the file data.
    pub fn from_bytes(data: &[u8]) -> io::Result<Self> {
        let mut cursor = Cursor::new(data);
        Self::read_from(&mut cursor)
    }

    /// Reads and parses a PCK archive from any seekable reader.
    pub fn read_from<R: Read + Seek>(reader: &mut R) -> io::Result<Self> {
        // Magic
        let mut magic = [0u8; 4];
        reader.read_exact(&mut magic)?;
        if &magic != PCK_MAGIC {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("invalid PCK magic: expected GDPC, got {:?}", magic),
            ));
        }

        // Header fields
        let format_version = read_u32(reader)?;
        let ver_major = read_u32(reader)?;
        let ver_minor = read_u32(reader)?;
        let ver_patch = read_u32(reader)?;
        let flags = read_u32(reader)?;

        // Reserved (16 × u32)
        for _ in 0..RESERVED_FIELD_COUNT {
            let _ = read_u32(reader)?;
        }

        let file_count = read_u32(reader)?;

        let header = PckHeader {
            format_version,
            ver_major,
            ver_minor,
            ver_patch,
            flags,
            file_count,
        };

        // Directory entries
        let mut entries = BTreeMap::new();
        for _ in 0..file_count {
            let path_len = read_u32(reader)? as usize;
            let mut path_buf = vec![0u8; path_len];
            reader.read_exact(&mut path_buf)?;
            // Trim NUL padding
            let path = String::from_utf8_lossy(&path_buf)
                .trim_end_matches('\0')
                .to_string();

            let offset = read_u64(reader)?;
            let size = read_u64(reader)?;

            let mut md5 = [0u8; 16];
            reader.read_exact(&mut md5)?;

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

        Ok(Self { header, entries })
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

    /// Writes the packed archive to any writer.
    pub fn write_to<W: Write + Seek>(&self, writer: &mut W) -> io::Result<()> {
        // -- Header --
        writer.write_all(PCK_MAGIC)?;
        write_u32(writer, PCK_FORMAT_VERSION)?;
        write_u32(writer, self.ver_major)?;
        write_u32(writer, self.ver_minor)?;
        write_u32(writer, self.ver_patch)?;
        write_u32(writer, 0)?; // flags

        // Reserved (16 × u32)
        for _ in 0..RESERVED_FIELD_COUNT {
            write_u32(writer, 0)?;
        }

        write_u32(writer, self.files.len() as u32)?;

        // -- Compute directory size to know where data starts --
        let mut dir_size: u64 = 0;
        for (path, _) in &self.files {
            let padded_len = pad_to_4(path.len());
            // path_len(4) + path(padded) + offset(8) + size(8) + md5(16)
            dir_size += 4 + padded_len as u64 + 8 + 8 + 16;
        }

        // Header size: magic(4) + 5×u32(20) + reserved(64) + file_count(4) = 92
        let header_size: u64 = 4 + 20 + (RESERVED_FIELD_COUNT as u64 * 4) + 4;
        let data_start = align_up(header_size + dir_size, self.alignment);

        // -- Write directory entries --
        let mut current_offset = data_start;
        let mut offsets = Vec::with_capacity(self.files.len());

        for (path, data) in &self.files {
            let path_bytes = path.as_bytes();
            let padded_len = pad_to_4(path_bytes.len());

            write_u32(writer, padded_len as u32)?;
            writer.write_all(path_bytes)?;
            // NUL-pad to alignment
            let padding = padded_len - path_bytes.len();
            if padding > 0 {
                writer.write_all(&vec![0u8; padding])?;
            }

            write_u64(writer, current_offset)?;
            write_u64(writer, data.len() as u64)?;

            // MD5 — write zeros (optional, not computed here)
            writer.write_all(&[0u8; 16])?;

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
            let text = std::str::from_utf8(bytes).map_err(|e| {
                EngineError::Parse(format!("invalid UTF-8 in {path}: {e}"))
            })?;
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
        Self {
            mounts: Vec::new(),
        }
    }

    /// Mounts a PCK archive with the given label and priority.
    ///
    /// Higher priority mounts are searched first. Returns an error if the
    /// PCK data is invalid. If a mount with the same label already exists,
    /// it is replaced.
    pub fn mount(&mut self, label: impl Into<String>, data: Vec<u8>, priority: i32) -> io::Result<()> {
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
        self.mounts
            .iter()
            .find_map(|m| m.loader.extract_raw(path))
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

        assert_eq!(archive.extract_file(&data, "res://a.txt").unwrap(), b"alpha");
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
        unified.register_uid_str("uid://unified_ref", "res://unified.tres");

        let by_path = unified.load("res://unified.tres").unwrap();
        let by_uid = unified.load("uid://unified_ref").unwrap();
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
        unified.register_uid_str("uid://mounted_ref", "res://mounted.tres");

        let by_path = unified.load("res://mounted.tres").unwrap();
        let by_uid = unified.load("uid://mounted_ref").unwrap();
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
