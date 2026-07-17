//! `ZIPReader` / `ZIPPacker` — Godot's archive classes (`core/io/zip_reader.cpp`,
//! `core/io/zip_packer.cpp`).
//!
//! Godot exposes minizip through two script classes: [`ZipPacker`] writes an
//! archive one file at a time (`open` → `start_file` → `write_file` →
//! `close_file` → `close`), and [`ZipReader`] opens an archive and reads
//! entries back out (`get_files`, `file_exists`, `read_file`).
//!
//! This is a self-contained implementation of the subset of the ZIP format
//! those classes need: local file headers, the central directory, and the
//! end-of-central-directory record, with STORE (method 0) and DEFLATE
//! (method 8) entries. DEFLATE uses `miniz_oxide`'s raw (headerless) codec,
//! which is exactly what ZIP method 8 stores.
//!
//! Not supported (rejected with a clear error rather than silently mis-read):
//! ZIP64 (archives ≥ 4 GiB or > 65535 entries), encrypted entries, and
//! compression methods other than STORE/DEFLATE.

use std::path::PathBuf;

use gdcore::error::{EngineError, EngineResult};

/// Local file header signature (`PK\x03\x04`).
const LOCAL_FILE_HEADER_SIG: u32 = 0x0403_4b50;
/// Central directory file header signature (`PK\x01\x02`).
const CENTRAL_DIR_HEADER_SIG: u32 = 0x0201_4b50;
/// End-of-central-directory record signature (`PK\x05\x06`).
const END_OF_CENTRAL_DIR_SIG: u32 = 0x0605_4b50;

/// Fixed size of a local file header, before the name/extra fields.
const LOCAL_HEADER_LEN: usize = 30;
/// Fixed size of a central directory header, before the name/extra/comment.
const CENTRAL_HEADER_LEN: usize = 46;
/// Fixed size of the end-of-central-directory record, before the comment.
const EOCD_LEN: usize = 22;

/// Stored, uncompressed.
const METHOD_STORE: u16 = 0;
/// Deflate-compressed.
const METHOD_DEFLATE: u16 = 8;

/// Version-needed-to-extract: 2.0, the minimum that implies DEFLATE support.
const VERSION_NEEDED: u16 = 20;
/// Version-made-by: high byte 3 (Unix), low byte 30 (spec 3.0).
const VERSION_MADE_BY: u16 = 0x031E;
/// MS-DOS date for 1980-01-01, the epoch of the ZIP timestamp field.
const DOS_EPOCH_DATE: u16 = 0x0021;

/// How [`ZipPacker::open`] treats a file that already exists at the path.
///
/// Mirrors Godot's `ZIPPacker.ZipAppend`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ZipAppend {
    /// `APPEND_CREATE` — create a new archive, discarding any existing file.
    #[default]
    Create,
    /// `APPEND_CREATEAFTER` — start a new archive appended after the existing
    /// file's bytes (how self-extracting archives are built).
    CreateAfter,
    /// `APPEND_ADDINZIP` — add to the existing archive, preserving its entries.
    AddInZip,
}

/// One archive member, as described by its central directory record.
#[derive(Debug, Clone)]
struct ZipEntry {
    name: String,
    method: u16,
    crc32: u32,
    compressed_size: u32,
    uncompressed_size: u32,
    local_header_offset: u32,
}

/// The file currently open for writing in a [`ZipPacker`].
#[derive(Debug, Clone)]
struct CurrentFile {
    name: String,
    data: Vec<u8>,
}

// ---------------------------------------------------------------------------
// Byte helpers
// ---------------------------------------------------------------------------

fn read_u16(data: &[u8], off: usize) -> Option<u16> {
    let b = data.get(off..off + 2)?;
    Some(u16::from_le_bytes([b[0], b[1]]))
}

fn read_u32(data: &[u8], off: usize) -> Option<u32> {
    let b = data.get(off..off + 4)?;
    Some(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

fn u16_at(data: &[u8], off: usize) -> EngineResult<u16> {
    read_u16(data, off)
        .ok_or_else(|| EngineError::Parse(format!("zip: truncated archive at offset {off}")))
}

fn u32_at(data: &[u8], off: usize) -> EngineResult<u32> {
    read_u32(data, off)
        .ok_or_else(|| EngineError::Parse(format!("zip: truncated archive at offset {off}")))
}

/// Computes the ZIP entry checksum (CRC-32, polynomial 0xEDB88320).
fn crc32(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFF_FFFF;
    for &byte in data {
        let idx = ((crc ^ byte as u32) & 0xFF) as usize;
        crc = CRC_TABLE[idx] ^ (crc >> 8);
    }
    crc ^ 0xFFFF_FFFF
}

/// Pre-computed CRC-32 lookup table (polynomial 0xEDB88320).
const CRC_TABLE: [u32; 256] = {
    let mut table = [0u32; 256];
    let mut i = 0;
    while i < 256 {
        let mut crc = i as u32;
        let mut j = 0;
        while j < 8 {
            if crc & 1 != 0 {
                crc = 0xEDB8_8320 ^ (crc >> 1);
            } else {
                crc >>= 1;
            }
            j += 1;
        }
        table[i] = crc;
        i += 1;
    }
    table
};

// ---------------------------------------------------------------------------
// Central directory parsing
// ---------------------------------------------------------------------------

/// Locates the end-of-central-directory record.
///
/// The record sits at the end of the file but may be followed by a variable
/// archive comment, so it is found by scanning backwards for the signature and
/// confirming the declared comment length reaches exactly the end of the data.
fn find_eocd(data: &[u8]) -> Option<usize> {
    if data.len() < EOCD_LEN {
        return None;
    }
    // A comment is at most 0xFFFF bytes, so the record cannot start earlier.
    let max_back = data.len().min(EOCD_LEN + 0xFFFF);
    let earliest = data.len() - max_back;
    let mut i = data.len() - EOCD_LEN;
    loop {
        if read_u32(data, i) == Some(END_OF_CENTRAL_DIR_SIG) {
            let comment_len = read_u16(data, i + 20)? as usize;
            if i + EOCD_LEN + comment_len == data.len() {
                return Some(i);
            }
        }
        if i == earliest {
            return None;
        }
        i -= 1;
    }
}

/// Parses the central directory, returning its entries and its file offset.
fn parse_directory(data: &[u8]) -> EngineResult<(Vec<ZipEntry>, usize)> {
    let eocd = find_eocd(data).ok_or_else(|| {
        EngineError::Parse("zip: end-of-central-directory record not found".to_string())
    })?;

    let count = u16_at(data, eocd + 10)? as usize;
    let cd_offset = u32_at(data, eocd + 16)? as usize;
    if cd_offset > data.len() {
        return Err(EngineError::Parse(format!(
            "zip: central directory offset {cd_offset} is past the end of the archive"
        )));
    }

    let mut entries = Vec::with_capacity(count);
    let mut off = cd_offset;
    for index in 0..count {
        if read_u32(data, off) != Some(CENTRAL_DIR_HEADER_SIG) {
            return Err(EngineError::Parse(format!(
                "zip: bad central directory header for entry {index} at offset {off}"
            )));
        }
        let method = u16_at(data, off + 10)?;
        let entry_crc = u32_at(data, off + 16)?;
        let compressed_size = u32_at(data, off + 20)?;
        let uncompressed_size = u32_at(data, off + 24)?;
        let name_len = u16_at(data, off + 28)? as usize;
        let extra_len = u16_at(data, off + 30)? as usize;
        let comment_len = u16_at(data, off + 32)? as usize;
        let local_header_offset = u32_at(data, off + 42)?;

        let name_bytes = data
            .get(off + CENTRAL_HEADER_LEN..off + CENTRAL_HEADER_LEN + name_len)
            .ok_or_else(|| {
                EngineError::Parse(format!("zip: truncated entry name for entry {index}"))
            })?;

        entries.push(ZipEntry {
            name: String::from_utf8_lossy(name_bytes).into_owned(),
            method,
            crc32: entry_crc,
            compressed_size,
            uncompressed_size,
            local_header_offset,
        });

        off += CENTRAL_HEADER_LEN + name_len + extra_len + comment_len;
    }

    Ok((entries, cd_offset))
}

fn name_matches(entry: &str, query: &str, case_sensitive: bool) -> bool {
    if case_sensitive {
        entry == query
    } else {
        entry.eq_ignore_ascii_case(query)
    }
}

// ---------------------------------------------------------------------------
// ZIPReader
// ---------------------------------------------------------------------------

/// Reads a ZIP archive, matching Godot's `ZIPReader` API.
///
/// The whole archive is held in memory, which is what the browser/wasm target
/// needs anyway (a fetched `.zip` arrives as a byte buffer).
#[derive(Debug, Clone, Default)]
pub struct ZipReader {
    data: Vec<u8>,
    entries: Vec<ZipEntry>,
    is_open: bool,
}

impl ZipReader {
    /// Creates a new, closed reader.
    pub fn new() -> Self {
        Self::default()
    }

    /// Opens the archive at `path`.
    pub fn open(&mut self, path: &str) -> EngineResult<()> {
        let data = std::fs::read(path)?;
        self.open_from_bytes(data)
    }

    /// Opens an archive already held in memory.
    pub fn open_from_bytes(&mut self, data: Vec<u8>) -> EngineResult<()> {
        let (entries, _) = parse_directory(&data)?;
        self.data = data;
        self.entries = entries;
        self.is_open = true;
        Ok(())
    }

    /// Closes the archive, releasing its bytes.
    pub fn close(&mut self) -> EngineResult<()> {
        if !self.is_open {
            return Err(EngineError::InvalidOperation(
                "ZIPReader: archive is not open".to_string(),
            ));
        }
        self.data = Vec::new();
        self.entries = Vec::new();
        self.is_open = false;
        Ok(())
    }

    /// Returns `true` while an archive is open.
    pub fn is_open(&self) -> bool {
        self.is_open
    }

    /// Lists every entry path in the archive, in central-directory order.
    pub fn get_files(&self) -> Vec<String> {
        self.entries.iter().map(|e| e.name.clone()).collect()
    }

    /// Returns `true` if `path` names an entry in the archive.
    pub fn file_exists(&self, path: &str, case_sensitive: bool) -> bool {
        self.find(path, case_sensitive).is_some()
    }

    /// Reads and decompresses the entry at `path`.
    ///
    /// The entry's CRC-32 is verified against the central directory, so a
    /// corrupt archive is reported rather than returning wrong bytes.
    pub fn read_file(&self, path: &str, case_sensitive: bool) -> EngineResult<Vec<u8>> {
        if !self.is_open {
            return Err(EngineError::InvalidOperation(
                "ZIPReader: archive is not open".to_string(),
            ));
        }
        let entry = self
            .find(path, case_sensitive)
            .ok_or_else(|| EngineError::NotFound(format!("zip: no entry named '{path}'")))?;

        let lho = entry.local_header_offset as usize;
        if read_u32(&self.data, lho) != Some(LOCAL_FILE_HEADER_SIG) {
            return Err(EngineError::Parse(format!(
                "zip: bad local file header for '{}' at offset {lho}",
                entry.name
            )));
        }
        // The local header's name/extra fields may differ in length from the
        // central directory's, so the data offset must be derived from it.
        let name_len = u16_at(&self.data, lho + 26)? as usize;
        let extra_len = u16_at(&self.data, lho + 28)? as usize;
        let start = lho + LOCAL_HEADER_LEN + name_len + extra_len;
        let end = start + entry.compressed_size as usize;
        let raw = self.data.get(start..end).ok_or_else(|| {
            EngineError::Parse(format!("zip: truncated entry data for '{}'", entry.name))
        })?;

        let out = match entry.method {
            METHOD_STORE => raw.to_vec(),
            METHOD_DEFLATE => miniz_oxide::inflate::decompress_to_vec_with_limit(
                raw,
                entry.uncompressed_size as usize,
            )
            .map_err(|e| {
                EngineError::Parse(format!("zip: inflate failed for '{}': {e}", entry.name))
            })?,
            other => {
                return Err(EngineError::Unsupported(format!(
                    "zip: '{}' uses compression method {other}; only store and deflate \
                     are supported",
                    entry.name
                )))
            }
        };

        if crc32(&out) != entry.crc32 {
            return Err(EngineError::Parse(format!(
                "zip: CRC-32 mismatch for '{}' (archive is corrupt)",
                entry.name
            )));
        }
        Ok(out)
    }

    fn find(&self, path: &str, case_sensitive: bool) -> Option<&ZipEntry> {
        self.entries
            .iter()
            .find(|e| name_matches(&e.name, path, case_sensitive))
    }
}

// ---------------------------------------------------------------------------
// ZIPPacker
// ---------------------------------------------------------------------------

/// Writes a ZIP archive, matching Godot's `ZIPPacker` API.
///
/// The archive is assembled in memory and flushed to disk by [`close`].
///
/// [`close`]: ZipPacker::close
#[derive(Debug, Clone, Default)]
pub struct ZipPacker {
    path: Option<PathBuf>,
    /// Local file headers + entry data written so far.
    buffer: Vec<u8>,
    entries: Vec<ZipEntry>,
    current: Option<CurrentFile>,
    /// Godot's `compression_level`: -1 selects the default, 0 stores.
    compression_level: i32,
}

impl ZipPacker {
    /// Creates a new, closed packer at Godot's default compression level.
    pub fn new() -> Self {
        Self {
            compression_level: -1,
            ..Self::default()
        }
    }

    /// Opens `path` for writing, honoring `append`.
    pub fn open(&mut self, path: &str, append: ZipAppend) -> EngineResult<()> {
        let p = PathBuf::from(path);
        self.current = None;
        match append {
            ZipAppend::Create => {
                self.buffer.clear();
                self.entries.clear();
            }
            ZipAppend::CreateAfter => {
                // Keep the existing bytes; the new archive starts after them.
                self.buffer = std::fs::read(&p).unwrap_or_default();
                self.entries.clear();
            }
            ZipAppend::AddInZip => {
                let existing = std::fs::read(&p).unwrap_or_default();
                if existing.is_empty() {
                    self.buffer.clear();
                    self.entries.clear();
                } else {
                    let (entries, cd_offset) = parse_directory(&existing)?;
                    // Drop the old directory; new entries are appended in its
                    // place and it is rewritten by close().
                    let mut buffer = existing;
                    buffer.truncate(cd_offset);
                    self.buffer = buffer;
                    self.entries = entries;
                }
            }
        }
        self.path = Some(p);
        Ok(())
    }

    /// Returns `true` while an archive is open.
    pub fn is_open(&self) -> bool {
        self.path.is_some()
    }

    /// Returns the current compression level.
    pub fn compression_level(&self) -> i32 {
        self.compression_level
    }

    /// Sets the compression level; -1 selects the default, 0 stores.
    pub fn set_compression_level(&mut self, level: i32) {
        self.compression_level = level;
    }

    /// Begins a new entry named `path` inside the archive.
    pub fn start_file(&mut self, path: &str) -> EngineResult<()> {
        if self.path.is_none() {
            return Err(EngineError::InvalidOperation(
                "ZIPPacker: archive is not open; call open() first".to_string(),
            ));
        }
        if let Some(current) = &self.current {
            return Err(EngineError::InvalidOperation(format!(
                "ZIPPacker: '{}' is still open; call close_file() first",
                current.name
            )));
        }
        self.current = Some(CurrentFile {
            name: path.to_string(),
            data: Vec::new(),
        });
        Ok(())
    }

    /// Appends `data` to the entry opened by [`start_file`].
    ///
    /// [`start_file`]: ZipPacker::start_file
    pub fn write_file(&mut self, data: &[u8]) -> EngineResult<()> {
        let current = self.current.as_mut().ok_or_else(|| {
            EngineError::InvalidOperation(
                "ZIPPacker: no file is open; call start_file() first".to_string(),
            )
        })?;
        current.data.extend_from_slice(data);
        Ok(())
    }

    /// Finishes the current entry, compressing and writing it into the archive.
    pub fn close_file(&mut self) -> EngineResult<()> {
        let current = self.current.take().ok_or_else(|| {
            EngineError::InvalidOperation(
                "ZIPPacker: no file is open; call start_file() first".to_string(),
            )
        })?;

        let entry_crc = crc32(&current.data);
        let uncompressed_size = u32::try_from(current.data.len()).map_err(|_| {
            EngineError::Unsupported(format!(
                "zip: '{}' exceeds 4 GiB, which requires ZIP64",
                current.name
            ))
        })?;
        let (method, payload) = self.compress(&current.data);
        let compressed_size = u32::try_from(payload.len()).map_err(|_| {
            EngineError::Unsupported(format!(
                "zip: '{}' exceeds 4 GiB, which requires ZIP64",
                current.name
            ))
        })?;
        let local_header_offset = u32::try_from(self.buffer.len()).map_err(|_| {
            EngineError::Unsupported("zip: archive exceeds 4 GiB, which requires ZIP64".to_string())
        })?;

        let name_bytes = current.name.as_bytes().to_vec();
        let name_len = u16::try_from(name_bytes.len()).map_err(|_| {
            EngineError::InvalidOperation(format!(
                "zip: entry name '{}' is longer than 65535 bytes",
                current.name
            ))
        })?;

        self.buffer
            .extend_from_slice(&LOCAL_FILE_HEADER_SIG.to_le_bytes());
        self.buffer.extend_from_slice(&VERSION_NEEDED.to_le_bytes());
        self.buffer.extend_from_slice(&0u16.to_le_bytes()); // general purpose flags
        self.buffer.extend_from_slice(&method.to_le_bytes());
        self.buffer.extend_from_slice(&0u16.to_le_bytes()); // mod time
        self.buffer.extend_from_slice(&DOS_EPOCH_DATE.to_le_bytes());
        self.buffer.extend_from_slice(&entry_crc.to_le_bytes());
        self.buffer
            .extend_from_slice(&compressed_size.to_le_bytes());
        self.buffer
            .extend_from_slice(&uncompressed_size.to_le_bytes());
        self.buffer.extend_from_slice(&name_len.to_le_bytes());
        self.buffer.extend_from_slice(&0u16.to_le_bytes()); // extra len
        self.buffer.extend_from_slice(&name_bytes);
        self.buffer.extend_from_slice(&payload);

        self.entries.push(ZipEntry {
            name: current.name,
            method,
            crc32: entry_crc,
            compressed_size,
            uncompressed_size,
            local_header_offset,
        });
        Ok(())
    }

    /// Writes the central directory and flushes the archive to its path.
    pub fn close(&mut self) -> EngineResult<()> {
        if let Some(current) = &self.current {
            return Err(EngineError::InvalidOperation(format!(
                "ZIPPacker: close() called while '{}' is still open; call close_file() first",
                current.name
            )));
        }
        let path = self.path.take().ok_or_else(|| {
            EngineError::InvalidOperation("ZIPPacker: archive is not open".to_string())
        })?;
        let bytes = self.finish()?;
        std::fs::write(&path, &bytes)?;
        self.buffer.clear();
        self.entries.clear();
        Ok(())
    }

    /// Serializes the archive: written entries + central directory + EOCD.
    fn finish(&self) -> EngineResult<Vec<u8>> {
        let count = u16::try_from(self.entries.len()).map_err(|_| {
            EngineError::Unsupported("zip: more than 65535 entries requires ZIP64".to_string())
        })?;

        let mut out = self.buffer.clone();
        let cd_offset = u32::try_from(out.len()).map_err(|_| {
            EngineError::Unsupported("zip: archive exceeds 4 GiB, which requires ZIP64".to_string())
        })?;

        for entry in &self.entries {
            let name_bytes = entry.name.as_bytes();
            let name_len = u16::try_from(name_bytes.len()).map_err(|_| {
                EngineError::InvalidOperation(format!(
                    "zip: entry name '{}' is longer than 65535 bytes",
                    entry.name
                ))
            })?;
            out.extend_from_slice(&CENTRAL_DIR_HEADER_SIG.to_le_bytes());
            out.extend_from_slice(&VERSION_MADE_BY.to_le_bytes());
            out.extend_from_slice(&VERSION_NEEDED.to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes()); // general purpose flags
            out.extend_from_slice(&entry.method.to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes()); // mod time
            out.extend_from_slice(&DOS_EPOCH_DATE.to_le_bytes());
            out.extend_from_slice(&entry.crc32.to_le_bytes());
            out.extend_from_slice(&entry.compressed_size.to_le_bytes());
            out.extend_from_slice(&entry.uncompressed_size.to_le_bytes());
            out.extend_from_slice(&name_len.to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes()); // extra len
            out.extend_from_slice(&0u16.to_le_bytes()); // comment len
            out.extend_from_slice(&0u16.to_le_bytes()); // disk number start
            out.extend_from_slice(&0u16.to_le_bytes()); // internal attributes
            out.extend_from_slice(&0u32.to_le_bytes()); // external attributes
            out.extend_from_slice(&entry.local_header_offset.to_le_bytes());
            out.extend_from_slice(name_bytes);
        }

        let cd_size = u32::try_from(out.len() - cd_offset as usize).map_err(|_| {
            EngineError::Unsupported(
                "zip: central directory exceeds 4 GiB, which requires ZIP64".to_string(),
            )
        })?;

        out.extend_from_slice(&END_OF_CENTRAL_DIR_SIG.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes()); // this disk number
        out.extend_from_slice(&0u16.to_le_bytes()); // disk with central directory
        out.extend_from_slice(&count.to_le_bytes()); // entries on this disk
        out.extend_from_slice(&count.to_le_bytes()); // entries total
        out.extend_from_slice(&cd_size.to_le_bytes());
        out.extend_from_slice(&cd_offset.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes()); // comment len
        Ok(out)
    }

    /// Picks the storage method for `data`.
    ///
    /// Deflate is only used when it actually wins; empty and incompressible
    /// entries are stored, which is what real ZIP writers do.
    fn compress(&self, data: &[u8]) -> (u16, Vec<u8>) {
        if data.is_empty() || self.compression_level == 0 {
            return (METHOD_STORE, data.to_vec());
        }
        let deflated = miniz_oxide::deflate::compress_to_vec(data, self.deflate_level());
        if deflated.len() < data.len() {
            (METHOD_DEFLATE, deflated)
        } else {
            (METHOD_STORE, data.to_vec())
        }
    }

    /// Maps Godot's `compression_level` onto miniz_oxide's 0-10 scale.
    fn deflate_level(&self) -> u8 {
        if self.compression_level < 0 {
            6
        } else {
            self.compression_level.min(10) as u8
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(name: &str) -> PathBuf {
        let mut p = std::env::temp_dir();
        p.push(name);
        p
    }

    fn pack(path: &str, files: &[(&str, &[u8])]) {
        let mut packer = ZipPacker::new();
        packer.open(path, ZipAppend::Create).expect("open");
        for (name, data) in files {
            packer.start_file(name).expect("start_file");
            packer.write_file(data).expect("write_file");
            packer.close_file().expect("close_file");
        }
        packer.close().expect("close");
    }

    #[test]
    fn crc32_matches_known_vector() {
        // The canonical CRC-32 check value for "123456789".
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(crc32(b""), 0);
    }

    #[test]
    fn store_and_deflate_entries_both_round_trip() {
        let path = temp_path("pat_nzgdu224_methods.zip");
        let path_str = path.to_str().unwrap();
        // Highly compressible: takes the deflate branch.
        let compressible = vec![b'a'; 4096];
        // Tiny: deflate cannot beat it, so it is stored.
        let incompressible: &[u8] = b"xy";
        pack(
            path_str,
            &[
                ("big.txt", compressible.as_slice()),
                ("small.txt", incompressible),
            ],
        );

        let mut reader = ZipReader::new();
        reader.open(path_str).expect("open");
        assert_eq!(reader.read_file("big.txt", true).unwrap(), compressible);
        assert_eq!(reader.read_file("small.txt", true).unwrap(), incompressible);

        // Confirm the two entries really took different branches.
        let methods: Vec<u16> = reader.entries.iter().map(|e| e.method).collect();
        assert_eq!(
            methods,
            vec![METHOD_DEFLATE, METHOD_STORE],
            "the compressible entry should deflate and the tiny one should store"
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn empty_entry_round_trips() {
        let path = temp_path("pat_nzgdu224_empty.zip");
        let path_str = path.to_str().unwrap();
        pack(path_str, &[("empty.txt", b"")]);

        let mut reader = ZipReader::new();
        reader.open(path_str).expect("open");
        assert_eq!(reader.get_files(), vec!["empty.txt".to_string()]);
        assert!(reader.read_file("empty.txt", true).unwrap().is_empty());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn corrupt_entry_data_is_reported_not_returned() {
        let path = temp_path("pat_nzgdu224_corrupt.zip");
        let path_str = path.to_str().unwrap();
        pack(path_str, &[("a.txt", b"the original contents")]);

        // Flip a byte inside the stored entry data; the CRC must catch it.
        let mut bytes = std::fs::read(&path).unwrap();
        let local_data_start = LOCAL_HEADER_LEN + "a.txt".len();
        bytes[local_data_start] ^= 0xFF;

        let mut reader = ZipReader::new();
        reader
            .open_from_bytes(bytes)
            .expect("directory still parses");
        let err = reader.read_file("a.txt", true).unwrap_err();
        assert!(
            matches!(err, EngineError::Parse(ref m) if m.contains("CRC-32 mismatch")),
            "corrupt data should be a CRC error, got: {err}"
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn reader_rejects_non_zip_data() {
        let mut reader = ZipReader::new();
        let err = reader
            .open_from_bytes(b"this is definitely not a zip archive".to_vec())
            .unwrap_err();
        assert!(matches!(err, EngineError::Parse(_)));
        assert!(!reader.is_open());
    }

    #[test]
    fn operations_on_a_closed_archive_error() {
        let reader = ZipReader::new();
        assert!(reader.read_file("a.txt", true).is_err());

        let mut packer = ZipPacker::new();
        // start_file before open is an error, not a silent no-op.
        assert!(packer.start_file("a.txt").is_err());
        assert!(packer.close().is_err());
    }

    #[test]
    fn packer_rejects_overlapping_files() {
        let path = temp_path("pat_nzgdu224_overlap.zip");
        let mut packer = ZipPacker::new();
        packer
            .open(path.to_str().unwrap(), ZipAppend::Create)
            .unwrap();
        packer.start_file("a.txt").unwrap();
        assert!(
            packer.start_file("b.txt").is_err(),
            "starting a second file before close_file() must error"
        );
        // close() with a file still open must error rather than truncate it.
        assert!(packer.close().is_err());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn write_file_appends_across_calls() {
        let path = temp_path("pat_nzgdu224_chunks.zip");
        let path_str = path.to_str().unwrap();
        let mut packer = ZipPacker::new();
        packer.open(path_str, ZipAppend::Create).unwrap();
        packer.start_file("joined.txt").unwrap();
        packer.write_file(b"hello ").unwrap();
        packer.write_file(b"world").unwrap();
        packer.close_file().unwrap();
        packer.close().unwrap();

        let mut reader = ZipReader::new();
        reader.open(path_str).unwrap();
        assert_eq!(
            reader.read_file("joined.txt", true).unwrap(),
            b"hello world"
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn case_sensitivity_is_honored() {
        let path = temp_path("pat_nzgdu224_case.zip");
        let path_str = path.to_str().unwrap();
        pack(path_str, &[("Mixed.TXT", b"data")]);

        let mut reader = ZipReader::new();
        reader.open(path_str).unwrap();
        assert!(reader.file_exists("Mixed.TXT", true));
        assert!(!reader.file_exists("mixed.txt", true));
        assert!(reader.file_exists("mixed.txt", false));
        assert!(reader.read_file("mixed.txt", true).is_err());
        assert_eq!(reader.read_file("mixed.txt", false).unwrap(), b"data");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn store_level_zero_skips_deflate() {
        let path = temp_path("pat_nzgdu224_level0.zip");
        let path_str = path.to_str().unwrap();
        let compressible = vec![b'q'; 2048];

        let mut packer = ZipPacker::new();
        packer.set_compression_level(0);
        assert_eq!(packer.compression_level(), 0);
        packer.open(path_str, ZipAppend::Create).unwrap();
        packer.start_file("plain.bin").unwrap();
        packer.write_file(&compressible).unwrap();
        packer.close_file().unwrap();
        packer.close().unwrap();

        let mut reader = ZipReader::new();
        reader.open(path_str).unwrap();
        assert_eq!(reader.entries[0].method, METHOD_STORE);
        assert_eq!(reader.read_file("plain.bin", true).unwrap(), compressible);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn close_releases_the_reader() {
        let path = temp_path("pat_nzgdu224_close.zip");
        let path_str = path.to_str().unwrap();
        pack(path_str, &[("a.txt", b"data")]);

        let mut reader = ZipReader::new();
        reader.open(path_str).unwrap();
        assert!(reader.is_open());
        reader.close().unwrap();
        assert!(!reader.is_open());
        assert!(reader.get_files().is_empty());
        // Closing twice is an error, not a panic.
        assert!(reader.close().is_err());
        let _ = std::fs::remove_file(&path);
    }
}
