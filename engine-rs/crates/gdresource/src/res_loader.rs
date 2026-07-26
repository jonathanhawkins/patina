//! Binary `.res` resource format parser.
//!
//! Godot's `.res` files are binary-serialized resources. This module
//! provides detection and header parsing for the binary format. Full
//! property deserialization is not yet implemented — attempting to fully
//! load a `.res` file returns an informative error.
//!
//! ## Binary format overview
//!
//! The `.res` binary format starts with a 4-byte magic number (`RSRC`),
//! followed by metadata including the format version, engine version,
//! resource type string, and then serialized properties using Godot's
//! binary variant encoding.

use gdvariant::Dictionary;
use std::sync::Arc;

use gdcore::error::{EngineError, EngineResult};
use gdcore::math::{Color, Rect2, Transform2D, Vector2, Vector3};
use gdcore::math3d::{Aabb, Basis, Plane, Quaternion, Transform3D};
use gdcore::node_path::NodePath;
use gdcore::string_name::StringName;
use gdvariant::Variant;

use crate::resource::{ExtResource, Resource};

/// Magic bytes at the start of every Godot `.res` binary resource file.
pub const RES_MAGIC: &[u8; 4] = b"RSRC";

/// Information extracted from a `.res` binary header.
#[derive(Debug, Clone, PartialEq)]
pub struct ResBinaryHeader {
    /// Whether this file uses big-endian byte order.
    pub big_endian: bool,
    /// Whether 64-bit variant encoding is used.
    pub use_64bit: bool,
    /// The engine version major number.
    pub version_major: u32,
    /// The engine version minor number.
    pub version_minor: u32,
    /// The format version.
    pub format_version: u32,
    /// The resource type string (e.g. `"Resource"`, `"PackedScene"`).
    pub resource_type: String,
}

/// Returns `true` if the given bytes start with the `.res` magic number.
pub fn is_res_binary(data: &[u8]) -> bool {
    data.len() >= 4 && &data[..4] == RES_MAGIC
}

/// Parses the binary header from a `.res` file.
///
/// This extracts metadata (endianness, version, resource type) without
/// deserializing the full resource. Returns an error if the data is too
/// short or doesn't start with the expected magic bytes.
pub fn parse_res_header(data: &[u8]) -> EngineResult<ResBinaryHeader> {
    if data.len() < 4 {
        return Err(EngineError::Parse(
            "data too short to be a .res file".into(),
        ));
    }

    if &data[..4] != RES_MAGIC {
        return Err(EngineError::Parse(format!(
            "invalid .res magic bytes: expected {:?}, got {:?}",
            RES_MAGIC,
            &data[..4]
        )));
    }

    // After magic: 4 bytes for big_endian flag, 4 bytes for use_64bit flag.
    if data.len() < 12 {
        return Err(EngineError::Parse(
            ".res header truncated: missing endian/64bit flags".into(),
        ));
    }

    let big_endian = read_u32_le(&data[4..8]) != 0;
    let use_64bit = read_u32_le(&data[8..12]) != 0;

    let read_u32 = if big_endian { read_u32_be } else { read_u32_le };

    // Version info: major (4), minor (4), format_version (4).
    if data.len() < 24 {
        return Err(EngineError::Parse(
            ".res header truncated: missing version info".into(),
        ));
    }

    let version_major = read_u32(&data[12..16]);
    let version_minor = read_u32(&data[16..20]);
    let format_version = read_u32(&data[20..24]);

    // Resource type string: 4 bytes length, then UTF-8 data (padded to 4-byte alignment).
    let resource_type = if data.len() >= 28 {
        let type_len = read_u32(&data[24..28]) as usize;
        let padded_len = (type_len + 3) & !3; // align to 4 bytes
        if data.len() >= 28 + padded_len {
            String::from_utf8_lossy(&data[28..28 + type_len])
                .trim_end_matches('\0')
                .to_string()
        } else {
            String::new()
        }
    } else {
        String::new()
    };

    Ok(ResBinaryHeader {
        big_endian,
        use_64bit,
        version_major,
        version_minor,
        format_version,
        resource_type,
    })
}

/// Loads a binary `.res`/`.scn`/`.tres` resource (Godot's
/// `ResourceFormatBinary`, `resource_format_binary.cpp`).
///
/// Parses the header, string table, external- and internal-resource tables,
/// then each internal resource's typed properties. The last internal resource
/// is the main one and is returned; the others become entries in its
/// [`subresources`](Resource) map (keyed by their internal index as a string).
/// Internal/external object references become
/// `Variant::String("SubResource:<idx>")` / `"ExtResource:<idx>"` values plus
/// entries in the main resource's `subresources`/`ext_resources` maps — the
/// same in-memory model the text loader produces.
///
/// The `PackedByteArray`/`PackedInt32Array`/… family decodes into the matching
/// [`Variant`] packed types (pat-nzgdu.82).
///
/// Returns an [`EngineError::Unsupported`] for compressed (`RSCC`), big-endian,
/// newer-than-6 format, and the variant types that still have no `Variant`
/// counterpart (RID, InputEvent, Callable/Signal, the integer vectors,
/// Vector4/Projection).
/// All reads are bounds-checked, so truncated or corrupt input yields an
/// [`EngineError::Parse`] rather than a panic.
pub fn load_res_binary(data: &[u8], path: &str) -> EngineResult<Arc<Resource>> {
    let mut cur = Cursor::new(data);

    // -- Magic --
    let magic = cur.take(4)?;
    if magic == RES_MAGIC_COMPRESSED {
        return Err(EngineError::Unsupported(format!(
            "compressed binary resource (RSCC) is not supported: {path}"
        )));
    }
    if magic != RES_MAGIC {
        return Err(EngineError::Parse(format!(
            "invalid binary resource magic: expected RSRC, got {magic:?} ({path})"
        )));
    }

    // -- Header --
    let big_endian = cur.u32()? != 0;
    if big_endian {
        return Err(EngineError::Unsupported(format!(
            "big-endian binary resource is not supported: {path}"
        )));
    }
    let _use_real64 = cur.u32()? != 0; // informational; real size is set by the flag below
    let _ver_major = cur.u32()?;
    let _ver_minor = cur.u32()?;
    let ver_format = cur.u32()?;
    if ver_format > FORMAT_VERSION_MAX {
        return Err(EngineError::Unsupported(format!(
            "binary resource format version {ver_format} is newer than supported ({FORMAT_VERSION_MAX}): {path}"
        )));
    }

    let type_name = read_unicode_string(&mut cur)?;
    let _importmd_ofs = cur.u64()?;
    let flags = cur.u32()?;
    let using_uids = flags & FORMAT_FLAG_UIDS != 0;
    let real_is_double = flags & FORMAT_FLAG_REAL_T_IS_DOUBLE != 0;

    // The uid field is always present (read whether or not the UID flag is set).
    let _uid = cur.u64()?;

    if flags & FORMAT_FLAG_HAS_SCRIPT_CLASS != 0 {
        let _script_class = read_unicode_string(&mut cur)?;
    }

    for _ in 0..RESERVED_FIELDS {
        cur.u32()?;
    }

    // -- String table --
    let str_count = cur.u32()?;
    let mut string_map = Vec::new();
    for _ in 0..str_count {
        string_map.push(read_unicode_string(&mut cur)?);
    }

    // -- External resources --
    let ext_count = cur.u32()?;
    let mut ext_resources = Vec::new();
    for i in 0..ext_count {
        let resource_type = read_unicode_string(&mut cur)?;
        let ext_path = read_unicode_string(&mut cur)?;
        let uid_str = if using_uids {
            let uid = cur.u64()? as i64;
            if uid >= 0 {
                crate::uid::id_to_text(uid)
            } else {
                String::new()
            }
        } else {
            String::new()
        };
        ext_resources.push(ExtResource {
            resource_type,
            uid: uid_str,
            path: ext_path,
            id: i.to_string(),
        });
    }

    // -- Internal resources (path + absolute byte offset) --
    let int_count = cur.u32()?;
    let mut internal_offsets = Vec::new();
    for _ in 0..int_count {
        let _int_path = read_unicode_string(&mut cur)?;
        let offset = cur.u64()?;
        internal_offsets.push(offset);
    }
    if internal_offsets.is_empty() {
        return Err(EngineError::Parse(format!(
            "binary resource has no internal resources: {path}"
        )));
    }

    // -- Parse each internal resource's properties --
    let mut reader = BinaryResourceReader {
        cur,
        real_is_double,
        ver_format,
        string_map,
        ext_count,
        internal_count: internal_offsets.len(),
    };

    let mut resources: Vec<Resource> = Vec::with_capacity(internal_offsets.len());
    for &offset in &internal_offsets {
        reader.cur.seek(offset)?;
        let res_type = read_unicode_string(&mut reader.cur)?;
        let prop_count = reader.cur.u32()?;
        let mut res = Resource::new(res_type);
        for _ in 0..prop_count {
            let name = reader.get_string()?;
            if name.is_empty() {
                return Err(EngineError::Parse(format!(
                    "binary resource has an empty property name: {path}"
                )));
            }
            let value = reader.parse_variant(0)?;
            res.set_property(name, value);
        }
        resources.push(res);
    }

    // Last internal resource is the main one; the rest are sub-resources keyed
    // by their internal index (matching OBJECT_INTERNAL_RESOURCE references).
    let mut main = resources
        .pop()
        .ok_or_else(|| EngineError::Parse(format!("binary resource has no resources: {path}")))?;
    main.path = path.to_string();
    if main.class_name.is_empty() {
        main.class_name = type_name;
    }
    for (idx, sub) in resources.into_iter().enumerate() {
        main.subresources.insert(idx.to_string(), Arc::new(sub));
    }
    for ext in ext_resources {
        main.ext_resources.insert(ext.id.clone(), ext);
    }

    Ok(Arc::new(main))
}

/// Reads a little-endian `u32` from a 4-byte slice.
fn read_u32_le(bytes: &[u8]) -> u32 {
    u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
}

/// Reads a big-endian `u32` from a 4-byte slice.
fn read_u32_be(bytes: &[u8]) -> u32 {
    u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
}

// ---------------------------------------------------------------------------
// Binary resource format constants (resource_format_binary.cpp / .h)
// ---------------------------------------------------------------------------

/// Magic for a zstd-compressed binary resource (unsupported here).
const RES_MAGIC_COMPRESSED: &[u8; 4] = b"RSCC";

/// Highest binary `FORMAT_VERSION` this loader accepts.
const FORMAT_VERSION_MAX: u32 = 6;
/// Below this format version, `NodePath` carries an extra property subname.
const FORMAT_VERSION_NO_NODEPATH_PROPERTY: u32 = 3;
/// Number of reserved `u32` header fields after the (optional) script class.
const RESERVED_FIELDS: usize = 11;

// Header flags.
const FORMAT_FLAG_UIDS: u32 = 2;
const FORMAT_FLAG_REAL_T_IS_DOUBLE: u32 = 4;
const FORMAT_FLAG_HAS_SCRIPT_CLASS: u32 = 8;

// Variant type tags (distinct from marshalls.cpp's values).
const V_NIL: u32 = 1;
const V_BOOL: u32 = 2;
const V_INT: u32 = 3;
const V_FLOAT: u32 = 4;
const V_STRING: u32 = 5;
const V_VECTOR2: u32 = 10;
const V_RECT2: u32 = 11;
const V_VECTOR3: u32 = 12;
const V_PLANE: u32 = 13;
const V_QUATERNION: u32 = 14;
const V_AABB: u32 = 15;
const V_BASIS: u32 = 16;
const V_TRANSFORM3D: u32 = 17;
const V_TRANSFORM2D: u32 = 18;
const V_COLOR: u32 = 20;
const V_NODE_PATH: u32 = 22;
const V_OBJECT: u32 = 24;
const V_DICTIONARY: u32 = 26;
const V_ARRAY: u32 = 30;
const V_PACKED_BYTE_ARRAY: u32 = 31;
const V_PACKED_INT32_ARRAY: u32 = 32;
const V_PACKED_FLOAT32_ARRAY: u32 = 33;
const V_PACKED_STRING_ARRAY: u32 = 34;
const V_PACKED_VECTOR2_ARRAY: u32 = 35;
const V_PACKED_VECTOR3_ARRAY: u32 = 36;
const V_PACKED_COLOR_ARRAY: u32 = 37;
const V_INT64: u32 = 40;
const V_DOUBLE: u32 = 41;
const V_STRING_NAME: u32 = 44;
const V_PACKED_INT64_ARRAY: u32 = 48;
const V_PACKED_FLOAT64_ARRAY: u32 = 49;

// Object reference sub-kinds (within V_OBJECT).
const OBJECT_EMPTY: u32 = 0;
const OBJECT_EXTERNAL_RESOURCE: u32 = 1;
const OBJECT_INTERNAL_RESOURCE: u32 = 2;
const OBJECT_EXTERNAL_RESOURCE_INDEX: u32 = 3;

/// Maximum array/dictionary nesting depth (guards against stack overflow on
/// crafted input).
const MAX_DEPTH: u32 = 128;

// ---------------------------------------------------------------------------
// Bounds-checked byte cursor
// ---------------------------------------------------------------------------

/// A little-endian, seekable, panic-free cursor over the resource bytes.
struct Cursor<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    fn take(&mut self, n: usize) -> EngineResult<&'a [u8]> {
        let end = self
            .pos
            .checked_add(n)
            .ok_or_else(|| EngineError::Parse("binary resource length overflow".into()))?;
        if end > self.data.len() {
            return Err(EngineError::Parse(format!(
                "binary resource truncated: need {n} byte(s) at offset {}, have {}",
                self.pos,
                self.data.len().saturating_sub(self.pos)
            )));
        }
        let s = &self.data[self.pos..end];
        self.pos = end;
        Ok(s)
    }

    fn seek(&mut self, pos: u64) -> EngineResult<()> {
        let p = usize::try_from(pos)
            .map_err(|_| EngineError::Parse("binary resource offset overflow".into()))?;
        if p > self.data.len() {
            return Err(EngineError::Parse(format!(
                "binary resource offset {p} out of range (len {})",
                self.data.len()
            )));
        }
        self.pos = p;
        Ok(())
    }

    fn u16(&mut self) -> EngineResult<u16> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }

    fn u32(&mut self) -> EngineResult<u32> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }

    fn u64(&mut self) -> EngineResult<u64> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }

    fn f32(&mut self) -> EngineResult<f32> {
        Ok(f32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }

    fn f64(&mut self) -> EngineResult<f64> {
        Ok(f64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }
}

/// Reads a length-prefixed UTF-8 string (`u32 len + len bytes`, NO padding).
///
/// Unlike marshalls.cpp, the binary resource format does not 4-byte-align
/// strings. Invalid UTF-8 is replaced lossily (as Godot's `String::utf8` is
/// lenient) rather than erroring.
fn read_unicode_string(cur: &mut Cursor) -> EngineResult<String> {
    let len = cur.u32()? as usize;
    if len == 0 {
        return Ok(String::new());
    }
    let bytes = cur.take(len)?;
    Ok(String::from_utf8_lossy(bytes).into_owned())
}

// ---------------------------------------------------------------------------
// Variant decoder for the binary resource format
// ---------------------------------------------------------------------------

/// Holds decode state shared while parsing a resource's property variants.
struct BinaryResourceReader<'a> {
    cur: Cursor<'a>,
    real_is_double: bool,
    ver_format: u32,
    /// The string table (`_get_string` indexes into this).
    string_map: Vec<String>,
    /// Number of external resources (for reference-index validation).
    ext_count: u32,
    /// Number of internal resources (for reference-index validation).
    internal_count: usize,
}

impl BinaryResourceReader<'_> {
    /// Reads a table-indexed or inline string (`_get_string`): a `u32` where the
    /// high bit means an inline string (`len = id & 0x7FFFFFFF`, no padding),
    /// otherwise an index into the string table.
    fn get_string(&mut self) -> EngineResult<String> {
        let id = self.cur.u32()?;
        if id & 0x8000_0000 != 0 {
            let len = (id & 0x7FFF_FFFF) as usize;
            if len == 0 {
                return Ok(String::new());
            }
            let bytes = self.cur.take(len)?;
            return Ok(String::from_utf8_lossy(bytes).into_owned());
        }
        self.string_map
            .get(id as usize)
            .cloned()
            .ok_or_else(|| EngineError::Parse(format!("string table index {id} out of range")))
    }

    /// Reads one `real_t` component as `f32` (the engine's storage type),
    /// honoring the file's real-is-double flag.
    fn real(&mut self) -> EngineResult<f32> {
        if self.real_is_double {
            Ok(self.cur.f64()? as f32)
        } else {
            self.cur.f32()
        }
    }

    /// Reads a `VARIANT_FLOAT` (a `real_t`) at full precision into `f64`.
    fn real_f64(&mut self) -> EngineResult<f64> {
        if self.real_is_double {
            self.cur.f64()
        } else {
            Ok(self.cur.f32()? as f64)
        }
    }

    /// Reads a packed array's `u32` element count, rejecting counts whose
    /// payload cannot fit in the remaining bytes.
    ///
    /// `min_elem_size` is the smallest number of bytes one element can occupy
    /// (for `PackedStringArray` that is the 4-byte length prefix). Checking
    /// before allocating keeps a corrupt count from reserving gigabytes — the
    /// cursor alone would catch the truncation, but only after the allocation.
    fn packed_len(&mut self, min_elem_size: usize) -> EngineResult<usize> {
        let len = self.cur.u32()? as usize;
        let needed = len
            .checked_mul(min_elem_size)
            .ok_or_else(|| EngineError::Parse("packed array length overflow".into()))?;
        let remaining = self.cur.data.len().saturating_sub(self.cur.pos);
        if needed > remaining {
            return Err(EngineError::Parse(format!(
                "packed array truncated: {len} element(s) need {needed} byte(s), have {remaining}"
            )));
        }
        Ok(len)
    }

    /// Skips the zero padding Godot writes to re-align to 4 bytes after a
    /// `PackedByteArray` payload (`_advance_padding`).
    fn advance_padding(&mut self, len: usize) -> EngineResult<()> {
        let extra = (4 - (len % 4)) % 4;
        if extra > 0 {
            self.cur.take(extra)?;
        }
        Ok(())
    }

    /// Byte width of one `real_t` component in this file.
    fn real_size(&self) -> usize {
        if self.real_is_double {
            8
        } else {
            4
        }
    }

    fn parse_variant(&mut self, depth: u32) -> EngineResult<Variant> {
        if depth > MAX_DEPTH {
            return Err(EngineError::Parse(
                "binary resource variant nesting too deep".into(),
            ));
        }

        let ty = self.cur.u32()?;
        match ty {
            V_NIL => Ok(Variant::Nil),
            V_BOOL => Ok(Variant::Bool(self.cur.u32()? != 0)),
            V_INT => Ok(Variant::Int(self.cur.u32()? as i32 as i64)),
            V_INT64 => Ok(Variant::Int(self.cur.u64()? as i64)),
            V_FLOAT => Ok(Variant::Float(self.real_f64()?)),
            V_DOUBLE => Ok(Variant::Float(self.cur.f64()?)),
            V_STRING => Ok(Variant::String(read_unicode_string(&mut self.cur)?)),
            V_STRING_NAME => Ok(Variant::StringName(StringName::new(&read_unicode_string(
                &mut self.cur,
            )?))),
            V_VECTOR2 => {
                let x = self.real()?;
                let y = self.real()?;
                Ok(Variant::Vector2(Vector2::new(x, y)))
            }
            V_VECTOR3 => {
                let x = self.real()?;
                let y = self.real()?;
                let z = self.real()?;
                Ok(Variant::Vector3(Vector3::new(x, y, z)))
            }
            V_RECT2 => {
                let px = self.real()?;
                let py = self.real()?;
                let sx = self.real()?;
                let sy = self.real()?;
                Ok(Variant::Rect2(Rect2::new(
                    Vector2::new(px, py),
                    Vector2::new(sx, sy),
                )))
            }
            V_PLANE => {
                let nx = self.real()?;
                let ny = self.real()?;
                let nz = self.real()?;
                let d = self.real()?;
                Ok(Variant::Plane(Plane {
                    normal: Vector3::new(nx, ny, nz),
                    d,
                }))
            }
            V_QUATERNION => {
                let x = self.real()?;
                let y = self.real()?;
                let z = self.real()?;
                let w = self.real()?;
                Ok(Variant::Quaternion(Quaternion { x, y, z, w }))
            }
            V_AABB => {
                let px = self.real()?;
                let py = self.real()?;
                let pz = self.real()?;
                let sx = self.real()?;
                let sy = self.real()?;
                let sz = self.real()?;
                Ok(Variant::Aabb(Aabb {
                    position: Vector3::new(px, py, pz),
                    size: Vector3::new(sx, sy, sz),
                }))
            }
            V_TRANSFORM2D => {
                // columns[0], columns[1], columns[2] (each x,y).
                let a = self.real()?;
                let b = self.real()?;
                let c = self.real()?;
                let d = self.real()?;
                let e = self.real()?;
                let f = self.real()?;
                Ok(Variant::Transform2D(Transform2D {
                    x: Vector2::new(a, b),
                    y: Vector2::new(c, d),
                    origin: Vector2::new(e, f),
                }))
            }
            V_BASIS => {
                let rows = self.read_reals::<9>()?;
                Ok(Variant::Basis(basis_from_rows(&rows)))
            }
            V_TRANSFORM3D => {
                let rows = self.read_reals::<9>()?;
                let ox = self.real()?;
                let oy = self.real()?;
                let oz = self.real()?;
                Ok(Variant::Transform3D(Transform3D {
                    basis: basis_from_rows(&rows),
                    origin: Vector3::new(ox, oy, oz),
                }))
            }
            V_COLOR => {
                // Colors are always single-precision.
                let r = self.cur.f32()?;
                let g = self.cur.f32()?;
                let b = self.cur.f32()?;
                let a = self.cur.f32()?;
                Ok(Variant::Color(Color::new(r, g, b, a)))
            }
            V_NODE_PATH => self.parse_node_path(),
            V_OBJECT => self.parse_object(),
            V_ARRAY => {
                let len = self.cur.u32()? & 0x7FFF_FFFF;
                let mut arr = Vec::new();
                for _ in 0..len {
                    arr.push(self.parse_variant(depth + 1)?);
                }
                Ok(Variant::Array(arr))
            }
            V_PACKED_BYTE_ARRAY => {
                let len = self.packed_len(1)?;
                let bytes = self.cur.take(len)?.to_vec();
                // Byte arrays (and only byte arrays) are padded back to a
                // 4-byte boundary.
                self.advance_padding(len)?;
                Ok(Variant::PackedByteArray(bytes))
            }
            V_PACKED_INT32_ARRAY => {
                let len = self.packed_len(4)?;
                let mut arr = Vec::with_capacity(len);
                for _ in 0..len {
                    arr.push(self.cur.u32()? as i32);
                }
                Ok(Variant::PackedInt32Array(arr))
            }
            V_PACKED_INT64_ARRAY => {
                let len = self.packed_len(8)?;
                let mut arr = Vec::with_capacity(len);
                for _ in 0..len {
                    arr.push(self.cur.u64()? as i64);
                }
                Ok(Variant::PackedInt64Array(arr))
            }
            V_PACKED_FLOAT32_ARRAY => {
                // Always 32-bit, regardless of the file's real_t width.
                let len = self.packed_len(4)?;
                let mut arr = Vec::with_capacity(len);
                for _ in 0..len {
                    arr.push(self.cur.f32()?);
                }
                Ok(Variant::PackedFloat32Array(arr))
            }
            V_PACKED_FLOAT64_ARRAY => {
                // Always 64-bit, regardless of the file's real_t width.
                let len = self.packed_len(8)?;
                let mut arr = Vec::with_capacity(len);
                for _ in 0..len {
                    arr.push(self.cur.f64()?);
                }
                Ok(Variant::PackedFloat64Array(arr))
            }
            V_PACKED_STRING_ARRAY => {
                // Each element is at minimum its own 4-byte length prefix.
                let len = self.packed_len(4)?;
                let mut arr = Vec::with_capacity(len);
                for _ in 0..len {
                    arr.push(read_unicode_string(&mut self.cur)?);
                }
                Ok(Variant::PackedStringArray(arr))
            }
            V_PACKED_VECTOR2_ARRAY => {
                let len = self.packed_len(self.real_size() * 2)?;
                let mut arr = Vec::with_capacity(len);
                for _ in 0..len {
                    let x = self.real()?;
                    let y = self.real()?;
                    arr.push(Vector2::new(x, y));
                }
                Ok(Variant::PackedVector2Array(arr))
            }
            V_PACKED_VECTOR3_ARRAY => {
                let len = self.packed_len(self.real_size() * 3)?;
                let mut arr = Vec::with_capacity(len);
                for _ in 0..len {
                    let x = self.real()?;
                    let y = self.real()?;
                    let z = self.real()?;
                    arr.push(Vector3::new(x, y, z));
                }
                Ok(Variant::PackedVector3Array(arr))
            }
            V_PACKED_COLOR_ARRAY => {
                // Colors are always single-precision (Color is float-based,
                // not real_t), matching the V_COLOR scalar arm above.
                let len = self.packed_len(16)?;
                let mut arr = Vec::with_capacity(len);
                for _ in 0..len {
                    let r = self.cur.f32()?;
                    let g = self.cur.f32()?;
                    let b = self.cur.f32()?;
                    let a = self.cur.f32()?;
                    arr.push(Color::new(r, g, b, a));
                }
                Ok(Variant::PackedColorArray(arr))
            }
            V_DICTIONARY => {
                let len = self.cur.u32()? & 0x7FFF_FFFF;
                let mut map = Dictionary::new();
                for _ in 0..len {
                    let key = self.parse_variant(depth + 1)?;
                    let value = self.parse_variant(depth + 1)?;
                    // Keys keep their decoded Variant type (typed dictionary keys).
                    map.insert(key, value);
                }
                Ok(Variant::Dictionary(map))
            }
            // Still unmapped: RID(23), INPUT_EVENT(25), CALLABLE(42), SIGNAL(43),
            // the *I integer vectors (45/46/47), PACKED_VECTOR4_ARRAY(38),
            // VECTOR4(50)/VECTOR4I(51)/PROJECTION(52) — none have a Variant
            // counterpart yet.
            other => Err(EngineError::Unsupported(format!(
                "binary resource variant type {other} is not supported yet"
            ))),
        }
    }

    fn parse_node_path(&mut self) -> EngineResult<Variant> {
        let name_count = self.cur.u16()? as usize;
        let raw_subname = self.cur.u16()?;
        let absolute = raw_subname & 0x8000 != 0;
        let mut subname_count = (raw_subname & 0x7FFF) as usize;
        if self.ver_format < FORMAT_VERSION_NO_NODEPATH_PROPERTY {
            subname_count += 1;
        }

        let mut names = Vec::new();
        for _ in 0..name_count {
            names.push(self.get_string()?);
        }
        let mut subnames = Vec::new();
        for _ in 0..subname_count {
            subnames.push(self.get_string()?);
        }

        // Rebuild via the public string API (names/subnames are separator-free).
        let mut s = String::new();
        if absolute {
            s.push('/');
        }
        s.push_str(&names.join("/"));
        for sub in &subnames {
            s.push(':');
            s.push_str(sub);
        }
        Ok(Variant::NodePath(NodePath::new(&s)))
    }

    fn parse_object(&mut self) -> EngineResult<Variant> {
        let objtype = self.cur.u32()?;
        match objtype {
            OBJECT_EMPTY => Ok(Variant::Nil),
            OBJECT_INTERNAL_RESOURCE => {
                let index = self.cur.u32()? as usize;
                if index >= self.internal_count {
                    return Err(EngineError::Parse(format!(
                        "internal resource reference index {index} out of range ({})",
                        self.internal_count
                    )));
                }
                Ok(Variant::String(format!("SubResource:{index}")))
            }
            OBJECT_EXTERNAL_RESOURCE_INDEX => {
                let index = self.cur.u32()?;
                if index >= self.ext_count {
                    return Err(EngineError::Parse(format!(
                        "external resource reference index {index} out of range ({})",
                        self.ext_count
                    )));
                }
                Ok(Variant::String(format!("ExtResource:{index}")))
            }
            OBJECT_EXTERNAL_RESOURCE => {
                // Deprecated inline form: type + path strings (no table index).
                let _exttype = read_unicode_string(&mut self.cur)?;
                let path = read_unicode_string(&mut self.cur)?;
                Ok(Variant::String(format!("ExtResource:{path}")))
            }
            other => Err(EngineError::Parse(format!(
                "unknown object reference kind: {other}"
            ))),
        }
    }

    /// Reads `N` `real_t` components into a fixed array.
    fn read_reals<const N: usize>(&mut self) -> EngineResult<[f32; N]> {
        let mut out = [0.0f32; N];
        for slot in out.iter_mut() {
            *slot = self.real()?;
        }
        Ok(out)
    }
}

/// Rebuilds a column-stored [`Basis`] from 9 row-major wire values
/// (`rows[i][j] = r[i*3+j]`; column 0 = (r0, r3, r6), etc.).
fn basis_from_rows(r: &[f32; 9]) -> Basis {
    Basis {
        x: Vector3::new(r[0], r[3], r[6]),
        y: Vector3::new(r[1], r[4], r[7]),
        z: Vector3::new(r[2], r[5], r[8]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a minimal .res binary header for testing.
    fn build_test_header(
        big_endian: bool,
        use_64bit: bool,
        major: u32,
        minor: u32,
        format_ver: u32,
        resource_type: &str,
    ) -> Vec<u8> {
        let mut data = Vec::new();
        data.extend_from_slice(RES_MAGIC);

        // Endian and 64bit flags are always written as LE at offset 4 and 8.
        data.extend_from_slice(&(big_endian as u32).to_le_bytes());
        data.extend_from_slice(&(use_64bit as u32).to_le_bytes());

        let write_u32 = if big_endian {
            |v: u32| v.to_be_bytes()
        } else {
            |v: u32| v.to_le_bytes()
        };

        data.extend_from_slice(&write_u32(major));
        data.extend_from_slice(&write_u32(minor));
        data.extend_from_slice(&write_u32(format_ver));

        // Resource type string with length prefix.
        let type_bytes = resource_type.as_bytes();
        data.extend_from_slice(&write_u32(type_bytes.len() as u32));
        data.extend_from_slice(type_bytes);
        // Pad to 4-byte alignment.
        let padding = (4 - (type_bytes.len() % 4)) % 4;
        data.extend(std::iter::repeat(0u8).take(padding));

        data
    }

    #[test]
    fn is_res_binary_detects_magic() {
        let data = build_test_header(false, false, 4, 3, 5, "Resource");
        assert!(is_res_binary(&data));
    }

    #[test]
    fn is_res_binary_rejects_non_res() {
        assert!(!is_res_binary(b"[gd_resource"));
        assert!(!is_res_binary(b"RSR"));
        assert!(!is_res_binary(b""));
        assert!(!is_res_binary(b"RSRD")); // close but wrong
    }

    #[test]
    fn is_res_binary_rejects_tres_text() {
        let tres = b"[gd_resource type=\"Theme\" format=3]";
        assert!(!is_res_binary(tres));
    }

    #[test]
    fn parse_header_little_endian() {
        let data = build_test_header(false, false, 4, 3, 5, "Resource");
        let header = parse_res_header(&data).unwrap();
        assert!(!header.big_endian);
        assert!(!header.use_64bit);
        assert_eq!(header.version_major, 4);
        assert_eq!(header.version_minor, 3);
        assert_eq!(header.format_version, 5);
        assert_eq!(header.resource_type, "Resource");
    }

    #[test]
    fn parse_header_big_endian() {
        let data = build_test_header(true, false, 4, 2, 3, "PackedScene");
        let header = parse_res_header(&data).unwrap();
        assert!(header.big_endian);
        assert!(!header.use_64bit);
        assert_eq!(header.version_major, 4);
        assert_eq!(header.version_minor, 2);
        assert_eq!(header.format_version, 3);
        assert_eq!(header.resource_type, "PackedScene");
    }

    #[test]
    fn parse_header_64bit_flag() {
        let data = build_test_header(false, true, 4, 0, 1, "Texture2D");
        let header = parse_res_header(&data).unwrap();
        assert!(header.use_64bit);
        assert_eq!(header.resource_type, "Texture2D");
    }

    #[test]
    fn parse_header_empty_type() {
        let data = build_test_header(false, false, 4, 0, 1, "");
        let header = parse_res_header(&data).unwrap();
        assert_eq!(header.resource_type, "");
    }

    #[test]
    fn parse_header_too_short() {
        let result = parse_res_header(b"RS");
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("too short"));
    }

    #[test]
    fn parse_header_wrong_magic() {
        let result = parse_res_header(b"NOTRSRC\x00\x00\x00\x00\x00");
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("invalid .res magic"));
    }

    #[test]
    fn parse_header_truncated_after_magic() {
        let mut data = Vec::new();
        data.extend_from_slice(RES_MAGIC);
        data.extend_from_slice(&[0u8; 4]); // only endian flag, missing 64bit
        let result = parse_res_header(&data);
        assert!(result.is_err());
    }

    #[test]
    fn load_res_binary_header_only_is_truncated() {
        // A bare header (no importmd/flags/tables) must fail cleanly, not panic.
        let data = build_test_header(false, false, 4, 3, 5, "Theme");
        let result = load_res_binary(&data, "res://theme.res");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("truncated"));
    }

    #[test]
    fn load_res_binary_invalid_data() {
        let result = load_res_binary(b"not a res file", "test.res");
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("invalid binary resource magic"));
    }

    #[test]
    fn load_res_binary_compressed_unsupported() {
        let mut data = RES_MAGIC_COMPRESSED.to_vec();
        data.extend_from_slice(&[0u8; 8]);
        let err = load_res_binary(&data, "res://c.res")
            .unwrap_err()
            .to_string();
        assert!(err.contains("compressed"), "got: {err}");
    }

    #[test]
    fn parse_header_long_resource_type() {
        let long_type = "VeryLongResourceTypeName";
        let data = build_test_header(false, false, 4, 0, 1, long_type);
        let header = parse_res_header(&data).unwrap();
        assert_eq!(header.resource_type, long_type);
    }
}

// ===========================================================================
// Full binary-resource loader tests (hand-built fixtures)
// ===========================================================================

#[cfg(test)]
mod binary_tests {
    use super::*;

    fn u32b(v: u32) -> Vec<u8> {
        v.to_le_bytes().to_vec()
    }
    fn u64b(v: u64) -> Vec<u8> {
        v.to_le_bytes().to_vec()
    }
    /// Length-prefixed string, no padding (`get_unicode_string` format).
    fn ustr(s: &str) -> Vec<u8> {
        let mut o = u32b(s.len() as u32);
        o.extend_from_slice(s.as_bytes());
        o
    }

    // Variant byte encoders (resource-binary VARIANT_* tags).
    fn v_nil() -> Vec<u8> {
        u32b(V_NIL)
    }
    fn v_bool(b: bool) -> Vec<u8> {
        let mut o = u32b(V_BOOL);
        o.extend(u32b(u32::from(b)));
        o
    }
    fn v_int(i: i32) -> Vec<u8> {
        let mut o = u32b(V_INT);
        o.extend_from_slice(&i.to_le_bytes());
        o
    }
    fn v_int64(i: i64) -> Vec<u8> {
        let mut o = u32b(V_INT64);
        o.extend_from_slice(&i.to_le_bytes());
        o
    }
    fn v_float32(f: f32) -> Vec<u8> {
        let mut o = u32b(V_FLOAT);
        o.extend_from_slice(&f.to_le_bytes());
        o
    }
    fn v_float64(f: f64) -> Vec<u8> {
        let mut o = u32b(V_FLOAT);
        o.extend_from_slice(&f.to_le_bytes());
        o
    }
    fn v_double(f: f64) -> Vec<u8> {
        let mut o = u32b(V_DOUBLE);
        o.extend_from_slice(&f.to_le_bytes());
        o
    }
    fn v_string(s: &str) -> Vec<u8> {
        let mut o = u32b(V_STRING);
        o.extend(ustr(s));
        o
    }
    fn v_vec2_f32(x: f32, y: f32) -> Vec<u8> {
        let mut o = u32b(V_VECTOR2);
        o.extend_from_slice(&x.to_le_bytes());
        o.extend_from_slice(&y.to_le_bytes());
        o
    }
    fn v_vec2_f64(x: f64, y: f64) -> Vec<u8> {
        let mut o = u32b(V_VECTOR2);
        o.extend_from_slice(&x.to_le_bytes());
        o.extend_from_slice(&y.to_le_bytes());
        o
    }
    fn v_array(elems: &[Vec<u8>]) -> Vec<u8> {
        let mut o = u32b(V_ARRAY);
        o.extend(u32b(elems.len() as u32));
        for e in elems {
            o.extend_from_slice(e);
        }
        o
    }
    /// `PackedByteArray`, including the 4-byte-alignment padding Godot writes.
    fn v_packed_byte(bytes: &[u8]) -> Vec<u8> {
        let mut o = u32b(V_PACKED_BYTE_ARRAY);
        o.extend(u32b(bytes.len() as u32));
        o.extend_from_slice(bytes);
        o.extend(vec![0u8; (4 - (bytes.len() % 4)) % 4]);
        o
    }
    fn v_packed_int32(vals: &[i32]) -> Vec<u8> {
        let mut o = u32b(V_PACKED_INT32_ARRAY);
        o.extend(u32b(vals.len() as u32));
        for &v in vals {
            o.extend_from_slice(&v.to_le_bytes());
        }
        o
    }
    fn v_packed_int64(vals: &[i64]) -> Vec<u8> {
        let mut o = u32b(V_PACKED_INT64_ARRAY);
        o.extend(u32b(vals.len() as u32));
        for &v in vals {
            o.extend_from_slice(&v.to_le_bytes());
        }
        o
    }
    fn v_packed_f32(vals: &[f32]) -> Vec<u8> {
        let mut o = u32b(V_PACKED_FLOAT32_ARRAY);
        o.extend(u32b(vals.len() as u32));
        for &v in vals {
            o.extend_from_slice(&v.to_le_bytes());
        }
        o
    }
    fn v_packed_f64(vals: &[f64]) -> Vec<u8> {
        let mut o = u32b(V_PACKED_FLOAT64_ARRAY);
        o.extend(u32b(vals.len() as u32));
        for &v in vals {
            o.extend_from_slice(&v.to_le_bytes());
        }
        o
    }
    fn v_packed_string(vals: &[&str]) -> Vec<u8> {
        let mut o = u32b(V_PACKED_STRING_ARRAY);
        o.extend(u32b(vals.len() as u32));
        for s in vals {
            o.extend(ustr(s));
        }
        o
    }
    fn v_packed_vec2_f32(vals: &[(f32, f32)]) -> Vec<u8> {
        let mut o = u32b(V_PACKED_VECTOR2_ARRAY);
        o.extend(u32b(vals.len() as u32));
        for &(x, y) in vals {
            o.extend_from_slice(&x.to_le_bytes());
            o.extend_from_slice(&y.to_le_bytes());
        }
        o
    }
    fn v_packed_vec2_f64(vals: &[(f64, f64)]) -> Vec<u8> {
        let mut o = u32b(V_PACKED_VECTOR2_ARRAY);
        o.extend(u32b(vals.len() as u32));
        for &(x, y) in vals {
            o.extend_from_slice(&x.to_le_bytes());
            o.extend_from_slice(&y.to_le_bytes());
        }
        o
    }
    fn v_packed_vec3_f32(vals: &[(f32, f32, f32)]) -> Vec<u8> {
        let mut o = u32b(V_PACKED_VECTOR3_ARRAY);
        o.extend(u32b(vals.len() as u32));
        for &(x, y, z) in vals {
            o.extend_from_slice(&x.to_le_bytes());
            o.extend_from_slice(&y.to_le_bytes());
            o.extend_from_slice(&z.to_le_bytes());
        }
        o
    }
    fn v_packed_vec3_f64(vals: &[(f64, f64, f64)]) -> Vec<u8> {
        let mut o = u32b(V_PACKED_VECTOR3_ARRAY);
        o.extend(u32b(vals.len() as u32));
        for &(x, y, z) in vals {
            o.extend_from_slice(&x.to_le_bytes());
            o.extend_from_slice(&y.to_le_bytes());
            o.extend_from_slice(&z.to_le_bytes());
        }
        o
    }
    /// `PackedColorArray` — always single-precision, even in a real64 file.
    fn v_packed_color(vals: &[(f32, f32, f32, f32)]) -> Vec<u8> {
        let mut o = u32b(V_PACKED_COLOR_ARRAY);
        o.extend(u32b(vals.len() as u32));
        for &(r, g, b, a) in vals {
            o.extend_from_slice(&r.to_le_bytes());
            o.extend_from_slice(&g.to_le_bytes());
            o.extend_from_slice(&b.to_le_bytes());
            o.extend_from_slice(&a.to_le_bytes());
        }
        o
    }
    fn v_dict(pairs: &[(Vec<u8>, Vec<u8>)]) -> Vec<u8> {
        let mut o = u32b(V_DICTIONARY);
        o.extend(u32b(pairs.len() as u32));
        for (k, v) in pairs {
            o.extend_from_slice(k);
            o.extend_from_slice(v);
        }
        o
    }
    fn v_ext_index(i: u32) -> Vec<u8> {
        let mut o = u32b(V_OBJECT);
        o.extend(u32b(OBJECT_EXTERNAL_RESOURCE_INDEX));
        o.extend(u32b(i));
        o
    }
    fn v_internal_index(i: u32) -> Vec<u8> {
        let mut o = u32b(V_OBJECT);
        o.extend(u32b(OBJECT_INTERNAL_RESOURCE));
        o.extend(u32b(i));
        o
    }

    /// Builds a complete binary resource. Property names are gathered into the
    /// string table and referenced by index (as Godot's saver does). `internals`
    /// is `[(type, [(prop_name, variant_bytes)])]`; the last is the main one.
    fn build(
        flags: u32,
        ext: &[(&str, &str)],
        internals: &[(&str, Vec<(&str, Vec<u8>)>)],
    ) -> Vec<u8> {
        // Gather property names into the string table (first-appearance order).
        let mut table: Vec<String> = Vec::new();
        for (_, props) in internals {
            for (name, _) in props {
                if !table.iter().any(|s| s.as_str() == *name) {
                    table.push((*name).to_string());
                }
            }
        }
        let index_of = |name: &str| table.iter().position(|s| s.as_str() == name).unwrap() as u32;

        let using_uids = flags & FORMAT_FLAG_UIDS != 0;

        // Header.
        let mut header = Vec::new();
        header.extend_from_slice(RES_MAGIC);
        header.extend(u32b(0)); // big_endian
        header.extend(u32b(0)); // use_real64 (informational)
        header.extend(u32b(4)); // ver_major
        header.extend(u32b(6)); // ver_minor
        header.extend(u32b(6)); // ver_format
        header.extend(ustr("Resource")); // top-level type
        header.extend(u64b(0)); // importmd_ofs
        header.extend(u32b(flags));
        header.extend(u64b(0)); // uid (always present)
        for _ in 0..RESERVED_FIELDS {
            header.extend(u32b(0));
        }
        // String table.
        header.extend(u32b(table.len() as u32));
        for s in &table {
            header.extend(ustr(s));
        }
        // External resources.
        header.extend(u32b(ext.len() as u32));
        for entry in ext {
            header.extend(ustr(entry.0));
            header.extend(ustr(entry.1));
            if using_uids {
                header.extend(u64b(0)); // uid 0 -> "uid://a"
            }
        }

        // Resource bodies (built to compute their absolute offsets).
        let mut bodies: Vec<Vec<u8>> = Vec::new();
        for entry in internals {
            let mut body = ustr(entry.0);
            body.extend(u32b(entry.1.len() as u32));
            for prop in &entry.1 {
                body.extend(u32b(index_of(prop.0))); // table-indexed name
                body.extend_from_slice(&prop.1);
            }
            bodies.push(body);
        }

        // Internal-resource table (path + absolute offset).
        let paths: Vec<String> = (0..internals.len())
            .map(|i| format!("local://{i}"))
            .collect();
        let mut int_table_size = 4usize; // count u32
        for p in &paths {
            int_table_size += 4 + p.len() + 8;
        }
        let bodies_start = header.len() + int_table_size;
        let mut offsets = Vec::new();
        let mut acc = bodies_start;
        for b in &bodies {
            offsets.push(acc as u64);
            acc += b.len();
        }

        let mut out = header;
        out.extend(u32b(internals.len() as u32));
        for (i, p) in paths.iter().enumerate() {
            out.extend(ustr(p));
            out.extend(u64b(offsets[i]));
        }
        for b in bodies {
            out.extend(b);
        }
        out
    }

    #[test]
    fn minimal_resource_int_string_vector2() {
        let data = build(
            0,
            &[],
            &[(
                "MyResource",
                vec![
                    ("count", v_int(42)),
                    ("name", v_string("hello")),
                    ("pos", v_vec2_f32(1.0, 2.0)),
                ],
            )],
        );
        let res = load_res_binary(&data, "res://x.res").unwrap();
        assert_eq!(res.class_name, "MyResource");
        assert_eq!(res.path, "res://x.res");
        assert_eq!(res.get_property("count"), Some(&Variant::Int(42)));
        assert_eq!(
            res.get_property("name"),
            Some(&Variant::String("hello".into()))
        );
        assert_eq!(
            res.get_property("pos"),
            Some(&Variant::Vector2(Vector2::new(1.0, 2.0)))
        );
    }

    #[test]
    fn scalars_and_math_roundtrip_through_loader() {
        let data = build(
            0,
            &[],
            &[(
                "R",
                vec![
                    ("nil", v_nil()),
                    ("t", v_bool(true)),
                    ("big", v_int64(1i64 << 40)),
                    ("f", v_float32(0.5)),
                    ("d", v_double(0.1)),
                ],
            )],
        );
        let res = load_res_binary(&data, "res://r.res").unwrap();
        assert_eq!(res.get_property("nil"), Some(&Variant::Nil));
        assert_eq!(res.get_property("t"), Some(&Variant::Bool(true)));
        assert_eq!(res.get_property("big"), Some(&Variant::Int(1i64 << 40)));
        assert_eq!(res.get_property("f"), Some(&Variant::Float(0.5)));
        assert_eq!(res.get_property("d"), Some(&Variant::Float(0.1)));
    }

    #[test]
    fn external_resource_reference() {
        let data = build(
            FORMAT_FLAG_UIDS,
            &[("Texture2D", "res://icon.png")],
            &[("Sprite2D", vec![("texture", v_ext_index(0))])],
        );
        let res = load_res_binary(&data, "res://s.res").unwrap();
        assert_eq!(
            res.get_property("texture"),
            Some(&Variant::String("ExtResource:0".into()))
        );
        assert_eq!(res.ext_resources.len(), 1);
        let ext = &res.ext_resources["0"];
        assert_eq!(ext.resource_type, "Texture2D");
        assert_eq!(ext.path, "res://icon.png");
        assert_eq!(ext.uid, "uid://a"); // uid 0 -> base-34 "a"
    }

    #[test]
    fn internal_subresource_reference() {
        // internal 0 = sub, internal 1 = main (references the sub by index 0).
        let data = build(
            0,
            &[],
            &[
                ("StyleBoxFlat", vec![("bg", v_int(7))]),
                ("Theme", vec![("panel", v_internal_index(0))]),
            ],
        );
        let res = load_res_binary(&data, "res://t.res").unwrap();
        assert_eq!(res.class_name, "Theme");
        assert_eq!(
            res.get_property("panel"),
            Some(&Variant::String("SubResource:0".into()))
        );
        assert_eq!(res.subresources.len(), 1);
        let sub = &res.subresources["0"];
        assert_eq!(sub.class_name, "StyleBoxFlat");
        assert_eq!(sub.get_property("bg"), Some(&Variant::Int(7)));
        // And it resolves through the shared model helper.
        let resolved = res.resolve_subresource("panel").unwrap();
        assert_eq!(resolved.class_name, "StyleBoxFlat");
    }

    #[test]
    fn nested_array_and_dictionary() {
        let data = build(
            0,
            &[],
            &[(
                "Data",
                vec![
                    ("items", v_array(&[v_int(1), v_int(2), v_int(3)])),
                    ("map", v_dict(&[(v_string("k"), v_vec2_f32(1.0, 2.0))])),
                ],
            )],
        );
        let res = load_res_binary(&data, "res://d.res").unwrap();
        assert_eq!(
            res.get_property("items"),
            Some(&Variant::Array(vec![
                Variant::Int(1),
                Variant::Int(2),
                Variant::Int(3)
            ]))
        );
        match res.get_property("map") {
            Some(Variant::Dictionary(d)) => {
                assert_eq!(d.get("k"), Some(&Variant::Vector2(Vector2::new(1.0, 2.0))));
            }
            other => panic!("expected dictionary, got {other:?}"),
        }
    }

    #[test]
    fn real64_flag_decodes_double_precision_math() {
        let data = build(
            FORMAT_FLAG_REAL_T_IS_DOUBLE,
            &[],
            &[(
                "R",
                vec![
                    ("pos", v_vec2_f64(1.5, 2.5)),
                    ("f", v_float64(0.25)), // VARIANT_FLOAT reads a double here
                ],
            )],
        );
        let res = load_res_binary(&data, "res://r64.res").unwrap();
        assert_eq!(
            res.get_property("pos"),
            Some(&Variant::Vector2(Vector2::new(1.5, 2.5)))
        );
        assert_eq!(res.get_property("f"), Some(&Variant::Float(0.25)));
    }

    #[test]
    fn big_endian_flag_unsupported() {
        let mut data = build(0, &[], &[("R", vec![("x", v_int(1))])]);
        data[4] = 1; // set big_endian flag
        let err = load_res_binary(&data, "res://be.res")
            .unwrap_err()
            .to_string();
        assert!(err.contains("big-endian"), "got: {err}");
    }

    #[test]
    fn newer_format_version_unsupported() {
        let mut data = build(0, &[], &[("R", vec![("x", v_int(1))])]);
        // ver_format is the 6th u32 (offset 20).
        data[20..24].copy_from_slice(&7u32.to_le_bytes());
        let err = load_res_binary(&data, "res://v7.res")
            .unwrap_err()
            .to_string();
        assert!(err.contains("format version"), "got: {err}");
    }

    #[test]
    fn packed_arrays_roundtrip_through_loader() {
        let data = build(
            0,
            &[],
            &[(
                "R",
                vec![
                    // len 4 => no padding; a stray pad byte would desync "i32s".
                    ("bytes", v_packed_byte(&[1, 2, 3, 4])),
                    ("i32s", v_packed_int32(&[-1, 0, 7])),
                    ("i64s", v_packed_int64(&[-1, 1i64 << 40])),
                    ("f32s", v_packed_f32(&[0.5, -1.5])),
                    ("f64s", v_packed_f64(&[0.25, 1e300])),
                    ("strs", v_packed_string(&["a", "", "hello"])),
                    ("v2s", v_packed_vec2_f32(&[(1.0, 2.0), (3.0, 4.0)])),
                    ("v3s", v_packed_vec3_f32(&[(1.0, 2.0, 3.0)])),
                    ("cols", v_packed_color(&[(1.0, 0.5, 0.25, 1.0)])),
                ],
            )],
        );
        let res = load_res_binary(&data, "res://packed.res").unwrap();
        assert_eq!(
            res.get_property("bytes"),
            Some(&Variant::PackedByteArray(vec![1, 2, 3, 4]))
        );
        assert_eq!(
            res.get_property("i32s"),
            Some(&Variant::PackedInt32Array(vec![-1, 0, 7]))
        );
        assert_eq!(
            res.get_property("i64s"),
            Some(&Variant::PackedInt64Array(vec![-1, 1i64 << 40]))
        );
        assert_eq!(
            res.get_property("f32s"),
            Some(&Variant::PackedFloat32Array(vec![0.5, -1.5]))
        );
        assert_eq!(
            res.get_property("f64s"),
            Some(&Variant::PackedFloat64Array(vec![0.25, 1e300]))
        );
        assert_eq!(
            res.get_property("strs"),
            Some(&Variant::PackedStringArray(vec![
                "a".to_string(),
                String::new(),
                "hello".to_string(),
            ]))
        );
        assert_eq!(
            res.get_property("v2s"),
            Some(&Variant::PackedVector2Array(vec![
                Vector2::new(1.0, 2.0),
                Vector2::new(3.0, 4.0),
            ]))
        );
        assert_eq!(
            res.get_property("v3s"),
            Some(&Variant::PackedVector3Array(vec![Vector3::new(
                1.0, 2.0, 3.0
            )]))
        );
        assert_eq!(
            res.get_property("cols"),
            Some(&Variant::PackedColorArray(vec![Color::new(
                1.0, 0.5, 0.25, 1.0
            )]))
        );
    }

    #[test]
    fn packed_byte_array_padding_is_consumed() {
        // 5 bytes => 3 bytes of padding. If the padding is not skipped the
        // following property decodes as garbage.
        let data = build(
            0,
            &[],
            &[(
                "R",
                vec![
                    ("bytes", v_packed_byte(&[1, 2, 3, 4, 5])),
                    ("after", v_int(42)),
                ],
            )],
        );
        let res = load_res_binary(&data, "res://pad.res").unwrap();
        assert_eq!(
            res.get_property("bytes"),
            Some(&Variant::PackedByteArray(vec![1, 2, 3, 4, 5]))
        );
        assert_eq!(res.get_property("after"), Some(&Variant::Int(42)));
    }

    #[test]
    fn empty_packed_arrays_decode_to_empty_vecs() {
        let data = build(
            0,
            &[],
            &[(
                "R",
                vec![
                    ("bytes", v_packed_byte(&[])),
                    ("strs", v_packed_string(&[])),
                    ("after", v_int(9)),
                ],
            )],
        );
        let res = load_res_binary(&data, "res://empty.res").unwrap();
        assert_eq!(
            res.get_property("bytes"),
            Some(&Variant::PackedByteArray(Vec::new()))
        );
        assert_eq!(
            res.get_property("strs"),
            Some(&Variant::PackedStringArray(Vec::new()))
        );
        assert_eq!(res.get_property("after"), Some(&Variant::Int(9)));
    }

    #[test]
    fn packed_vector_arrays_honor_real64_flag_but_colors_stay_f32() {
        let data = build(
            FORMAT_FLAG_REAL_T_IS_DOUBLE,
            &[],
            &[(
                "R",
                vec![
                    ("v2s", v_packed_vec2_f64(&[(1.5, 2.5)])),
                    ("v3s", v_packed_vec3_f64(&[(1.5, 2.5, 3.5)])),
                    // Colors are float-based, not real_t — still 4×f32 here.
                    ("cols", v_packed_color(&[(1.0, 0.5, 0.25, 1.0)])),
                    ("after", v_int(3)),
                ],
            )],
        );
        let res = load_res_binary(&data, "res://p64.res").unwrap();
        assert_eq!(
            res.get_property("v2s"),
            Some(&Variant::PackedVector2Array(vec![Vector2::new(1.5, 2.5)]))
        );
        assert_eq!(
            res.get_property("v3s"),
            Some(&Variant::PackedVector3Array(vec![Vector3::new(
                1.5, 2.5, 3.5
            )]))
        );
        assert_eq!(
            res.get_property("cols"),
            Some(&Variant::PackedColorArray(vec![Color::new(
                1.0, 0.5, 0.25, 1.0
            )]))
        );
        // Proves the color array consumed exactly 16 bytes, not 32.
        assert_eq!(res.get_property("after"), Some(&Variant::Int(3)));
    }

    #[test]
    fn packed_arrays_nest_inside_array_and_dictionary() {
        let data = build(
            0,
            &[],
            &[(
                "R",
                vec![
                    ("arr", v_array(&[v_packed_int32(&[1, 2])])),
                    ("dict", v_dict(&[(v_string("k"), v_packed_string(&["v"]))])),
                ],
            )],
        );
        let res = load_res_binary(&data, "res://nest.res").unwrap();
        assert_eq!(
            res.get_property("arr"),
            Some(&Variant::Array(vec![Variant::PackedInt32Array(vec![1, 2])]))
        );
        assert!(matches!(
            res.get_property("dict"),
            Some(Variant::Dictionary(_))
        ));
    }

    #[test]
    fn packed_array_with_absurd_length_errors_without_allocating() {
        // A corrupt count must be rejected against the bytes actually left,
        // never used to reserve ~16 GB.
        let mut packed = u32b(V_PACKED_INT32_ARRAY);
        packed.extend(u32b(u32::MAX));
        let data = build(0, &[], &[("R", vec![("evil", packed)])]);
        let err = load_res_binary(&data, "res://evil.res")
            .unwrap_err()
            .to_string();
        assert!(err.contains("truncated"), "got: {err}");
    }

    #[test]
    fn packed_byte_array_with_truncated_payload_errors() {
        let mut packed = u32b(V_PACKED_BYTE_ARRAY);
        packed.extend(u32b(1_000_000)); // claims 1 MB; the file is far smaller
        let data = build(0, &[], &[("R", vec![("evil", packed)])]);
        let err = load_res_binary(&data, "res://evil2.res")
            .unwrap_err()
            .to_string();
        assert!(err.contains("truncated"), "got: {err}");
    }

    #[test]
    fn truncated_input_never_panics() {
        let data = build(
            0,
            &[("Ext", "res://e.res")],
            &[
                ("Sub", vec![("v", v_int(9))]),
                ("Main", vec![("arr", v_array(&[v_int(1), v_int(2)]))]),
            ],
        );
        // Full parse works.
        assert!(load_res_binary(&data, "res://full.res").is_ok());
        // Every strict prefix errors cleanly (no panic).
        for len in 0..data.len() {
            assert!(
                load_res_binary(&data[..len], "res://t.res").is_err(),
                "prefix len {len} should error"
            );
        }
    }

    #[test]
    fn out_of_range_internal_reference_errors() {
        // Only one internal resource exists (index 0 = main), but a property
        // references internal index 5.
        let data = build(0, &[], &[("R", vec![("bad", v_internal_index(5))])]);
        let err = load_res_binary(&data, "res://c.res")
            .unwrap_err()
            .to_string();
        assert!(err.contains("out of range"), "got: {err}");
    }

    #[test]
    fn out_of_range_external_reference_errors() {
        // No external resources, but a property references external index 0.
        let data = build(0, &[], &[("R", vec![("bad", v_ext_index(0))])]);
        let err = load_res_binary(&data, "res://c.res")
            .unwrap_err()
            .to_string();
        assert!(err.contains("out of range"), "got: {err}");
    }
}
