//! The binary FBX 7.x container format (pat-s3rrv.136).
//!
//! This is the **container** layer only: the file header and the node-record
//! tree with scalar and string properties. It does not yet interpret the
//! `Objects`/`Connections` graph, and it does not decode the zlib-compressed
//! array properties — both are reported as explicit errors rather than skipped,
//! so a caller can never mistake a partial parse for a complete one.
//!
//! Every offset and size here is taken from ufbx, the parser Godot itself uses,
//! vendored at `upstream/godot/thirdparty/ufbx/ufbx.c`. Layout references are
//! cited per item.

use gdcore::{EngineError, EngineResult};

/// `ufbxi_binary_magic` (`ufbx.c:9393`) — 22 bytes, including the trailing
/// `\x00\x1a`.
pub const FBX_BINARY_MAGIC: &[u8] = b"Kaydara FBX Binary  \x00\x1a";

/// `UFBXI_BINARY_HEADER_SIZE`: the 22-byte magic, one endianness byte, then a
/// `u32` version (`ufbx.c:11177-11198`).
const FBX_HEADER_SIZE: usize = 27;

/// The file header of a binary FBX.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FbxHeader {
    /// `7400` for FBX 7.4, `7500` for 7.5, and so on.
    pub version: u32,
    /// Whether the file's integers are big-endian.
    pub big_endian: bool,
}

/// One property value carried by a node record.
///
/// ⓘ FBX tags each value with a single ASCII character. The scalar tags are
/// modelled here; the array tags (`f d l i b`) are not, because their payloads
/// are zlib-compressed — see [`FbxError::CompressedArray`].
#[derive(Debug, Clone, PartialEq)]
pub enum FbxProperty {
    /// `'C'` — a one-byte boolean.
    Bool(bool),
    /// `'Y'` — `i16`.
    I16(i16),
    /// `'I'` — `i32`.
    I32(i32),
    /// `'L'` — `i64`.
    I64(i64),
    /// `'F'` — `f32`.
    F32(f32),
    /// `'D'` — `f64`.
    F64(f64),
    /// `'S'` — a length-prefixed string. ⚠️ FBX strings are **not**
    /// NUL-terminated and may contain interior NULs (object names use `\x00\x01`
    /// as a separator), so this keeps the raw bytes rather than a `String`.
    Str(Vec<u8>),
    /// `'R'` — a length-prefixed opaque blob.
    Raw(Vec<u8>),
}

/// One node record: a name, its property values and its children.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct FbxNode {
    /// The record's name, e.g. `"Objects"`.
    pub name: String,
    /// Its property values, in file order.
    pub properties: Vec<FbxProperty>,
    /// Nested records.
    pub children: Vec<FbxNode>,
}

impl FbxNode {
    /// The first child named `name`, if any.
    pub fn child(&self, name: &str) -> Option<&FbxNode> {
        self.children.iter().find(|c| c.name == name)
    }
}

/// Why a binary FBX could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FbxError {
    /// The magic did not match — this is not a binary FBX at all.
    NotBinaryFbx,
    /// The file ended inside a record.
    Truncated,
    /// A property carried an array tag, whose payload is zlib-compressed.
    CompressedArray(char),
    /// A property tag this reader does not know.
    UnknownProperty(char),
}

impl From<FbxError> for EngineError {
    fn from(e: FbxError) -> Self {
        match e {
            FbxError::NotBinaryFbx => {
                EngineError::Parse("not a binary FBX file (magic mismatch)".into())
            }
            FbxError::Truncated => EngineError::Parse("truncated binary FBX record".into()),
            FbxError::CompressedArray(tag) => EngineError::Unsupported(format!(
                "fbx: array property '{tag}' is zlib-compressed and is not decoded yet"
            )),
            FbxError::UnknownProperty(tag) => {
                EngineError::Parse(format!("fbx: unknown property tag '{tag}'"))
            }
        }
    }
}

/// Whether `data` begins with the binary FBX magic.
///
/// ⚠️ This is a *container* check, not a validity check: an ASCII FBX is still a
/// real FBX and returns `false` here.
pub fn is_binary_fbx(data: &[u8]) -> bool {
    data.len() >= FBX_BINARY_MAGIC.len() && data.starts_with(FBX_BINARY_MAGIC)
}

/// Reads the file header.
///
/// 🔑 The byte immediately after the magic is the **endianness indicator**
/// (`ufbx.c:11184-11186`), not padding — a distinction the widely-circulated
/// community format notes get wrong. A reader that treats it as filler and
/// always parses little-endian silently misreads every big-endian file.
pub fn parse_header(data: &[u8]) -> Result<FbxHeader, FbxError> {
    if !is_binary_fbx(data) {
        return Err(FbxError::NotBinaryFbx);
    }
    if data.len() < FBX_HEADER_SIZE {
        return Err(FbxError::Truncated);
    }
    let big_endian = data[FBX_BINARY_MAGIC.len()] != 0;
    let v = &data[FBX_BINARY_MAGIC.len() + 1..FBX_BINARY_MAGIC.len() + 5];
    let version = if big_endian {
        u32::from_be_bytes([v[0], v[1], v[2], v[3]])
    } else {
        u32::from_le_bytes([v[0], v[1], v[2], v[3]])
    };
    Ok(FbxHeader {
        version,
        big_endian,
    })
}

/// Parses a whole binary FBX into its top-level node records.
///
/// ⚠️ Only little-endian files are read; a big-endian one is reported rather
/// than byte-swapped incorrectly.
pub fn parse(data: &[u8]) -> EngineResult<(FbxHeader, Vec<FbxNode>)> {
    let header = parse_header(data)?;
    if header.big_endian {
        return Err(EngineError::Unsupported(
            "fbx: big-endian files are not decoded yet".into(),
        ));
    }
    let mut cursor = FBX_HEADER_SIZE;
    let mut nodes = Vec::new();
    // 🔑 The record list ends with a NULL record — an all-zero header — NOT at
    // end-of-file: a valid FBX carries a footer after it. Reading to EOF instead
    // would try to parse the footer as a record.
    while let Some(node) = read_node(data, &mut cursor, header.version)? {
        nodes.push(node);
    }
    Ok((header, nodes))
}

/// The node-record header size: 13 bytes below FBX 7500, 25 at or above it,
/// because the three offset fields widen from `u32` to `u64` (`ufbx.c:8967`).
fn record_header_size(version: u32) -> usize {
    if version >= 7500 {
        25
    } else {
        13
    }
}

fn take<'a>(data: &'a [u8], cursor: &mut usize, n: usize) -> Result<&'a [u8], FbxError> {
    let end = cursor.checked_add(n).ok_or(FbxError::Truncated)?;
    let slice = data.get(*cursor..end).ok_or(FbxError::Truncated)?;
    *cursor = end;
    Ok(slice)
}

fn u32_at(b: &[u8]) -> u32 {
    u32::from_le_bytes([b[0], b[1], b[2], b[3]])
}

fn u64_at(b: &[u8]) -> u64 {
    u64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]])
}

/// Reads one node record, or `None` at the NULL record that ends a list.
fn read_node(data: &[u8], cursor: &mut usize, version: u32) -> EngineResult<Option<FbxNode>> {
    let hdr_size = record_header_size(version);
    let header = take(data, cursor, hdr_size)?;

    let (end_offset, num_properties, name_len) = if version >= 7500 {
        (
            u64_at(&header[0..8]) as usize,
            u64_at(&header[8..16]) as usize,
            header[24] as usize,
        )
    } else {
        (
            u32_at(&header[0..4]) as usize,
            u32_at(&header[4..8]) as usize,
            header[12] as usize,
        )
    };

    // The NULL record: every header field zero.
    if end_offset == 0 && num_properties == 0 && name_len == 0 {
        return Ok(None);
    }

    let name = String::from_utf8_lossy(take(data, cursor, name_len)?).into_owned();
    let mut properties = Vec::with_capacity(num_properties);
    for _ in 0..num_properties {
        properties.push(read_property(data, cursor)?);
    }

    // 🔑 Children are present exactly when the record has not already reached
    // its own `end_offset`. There is no child count in the format, so a reader
    // that assumes "properties then always a child list" consumes the NEXT
    // record's header as a NULL terminator and loses it.
    let mut children = Vec::new();
    if *cursor < end_offset {
        while let Some(child) = read_node(data, cursor, version)? {
            children.push(child);
        }
    }
    // Trust the record's own end offset over our arithmetic.
    if end_offset > 0 && end_offset <= data.len() {
        *cursor = end_offset;
    }

    Ok(Some(FbxNode {
        name,
        properties,
        children,
    }))
}

fn read_property(data: &[u8], cursor: &mut usize) -> EngineResult<FbxProperty> {
    let tag = take(data, cursor, 1)?[0] as char;
    let prop = match tag {
        'C' => FbxProperty::Bool(take(data, cursor, 1)?[0] != 0),
        'Y' => {
            let b = take(data, cursor, 2)?;
            FbxProperty::I16(i16::from_le_bytes([b[0], b[1]]))
        }
        'I' => FbxProperty::I32(u32_at(take(data, cursor, 4)?) as i32),
        'L' => FbxProperty::I64(u64_at(take(data, cursor, 8)?) as i64),
        'F' => FbxProperty::F32(f32::from_bits(u32_at(take(data, cursor, 4)?))),
        'D' => FbxProperty::F64(f64::from_bits(u64_at(take(data, cursor, 8)?))),
        'S' | 'R' => {
            let len = u32_at(take(data, cursor, 4)?) as usize;
            let bytes = take(data, cursor, len)?.to_vec();
            if tag == 'S' {
                FbxProperty::Str(bytes)
            } else {
                FbxProperty::Raw(bytes)
            }
        }
        // ⚠️ Array payloads carry their own encoding word and are usually
        // zlib-deflated. Reported rather than skipped: silently dropping them
        // would yield a node tree that looks complete and has lost its geometry.
        'f' | 'd' | 'l' | 'i' | 'b' => return Err(FbxError::CompressedArray(tag).into()),
        other => return Err(FbxError::UnknownProperty(other).into()),
    };
    Ok(prop)
}
