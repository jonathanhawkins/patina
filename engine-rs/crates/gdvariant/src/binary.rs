//! Binary Variant codec — a faithful port of Godot's `encode_variant` /
//! `decode_variant` (`core/io/marshalls.cpp`).
//!
//! This is the shared wire format used by binary `.res`/`.scn` resources, the
//! high-level multiplayer RPC/`var_to_bytes` APIs, and `PackedByteArray`
//! (de)serialization. Values are little-endian and every encoded variant is a
//! multiple of 4 bytes long.
//!
//! # Layout
//!
//! Each variant begins with a `u32` header whose low byte
//! ([`HEADER_TYPE_MASK`]) is the [`Variant::Type`](crate::VariantType) ordinal.
//! Bit 16 ([`HEADER_DATA_FLAG_64`]) marks 64-bit `INT`/`FLOAT` payloads (and,
//! for a `REAL_T_IS_DOUBLE` Godot build, double-precision math types). For
//! `ARRAY`/`DICTIONARY`, bits 16–19 hold typed-container kinds — unsupported
//! here and rejected on decode.
//!
//! # Scope
//!
//! Encodes every variant this crate's [`Variant`] enum can represent —
//! including the nine `Packed*Array` types (ordinals 29–37). Types the enum
//! lacks (`Vector2i`, `Projection`, `RID`, objects, `PackedVector4Array`, …)
//! are reported as [`VariantDecodeError::UnsupportedType`] on decode and — for
//! the three the enum *does* carry but the wire format can't round-trip
//! (`ObjectId`, `Callable`, `Resource`) — as [`VariantEncodeError::Unsupported`]
//! on encode.
//!
//! Math types are encoded as 32-bit floats (matching a default `real_t = float`
//! Godot build, which is also how this engine stores them). The decoder still
//! honors [`HEADER_DATA_FLAG_64`] so buffers produced by a double-precision
//! Godot build load correctly (downcast to `f32`).

use std::collections::HashMap;

use thiserror::Error;

use gdcore::math::{Color, Rect2, Transform2D, Vector2, Vector3};
use gdcore::math3d::{Aabb, Basis, Plane, Quaternion, Transform3D};
use gdcore::node_path::NodePath;
use gdcore::string_name::StringName;

use crate::variant::{Variant, VariantType};

// ---------------------------------------------------------------------------
// Header / type constants (marshalls.cpp, variant.h Variant::Type)
// ---------------------------------------------------------------------------

/// Mask selecting the variant type ordinal from the header word.
pub const HEADER_TYPE_MASK: u32 = 0xFF;
/// Header bit marking a 64-bit `INT`/`FLOAT` (and double-precision math types).
pub const HEADER_DATA_FLAG_64: u32 = 1 << 16;

/// One past the last valid `Variant::Type` ordinal (`Variant::VARIANT_MAX`).
const VARIANT_MAX: u32 = 39;
/// Maximum nesting depth, matching Godot's `Variant::MAX_RECURSION_DEPTH`.
const MAX_RECURSION_DEPTH: u32 = 1024;

// Typed-container kind bits (per-type reinterpretation of header bits 16+).
const TYPED_ARRAY_MASK: u32 = 0b11 << 16;
const TYPED_DICT_KEY_MASK: u32 = 0b11 << 16;
const TYPED_DICT_VALUE_MASK: u32 = 0b11 << 18;
/// High bit of a container count marking a "shared" array/dictionary.
const CONTAINER_SHARED_FLAG: u32 = 0x8000_0000;
/// Mask for the actual element count of a container.
const CONTAINER_COUNT_MASK: u32 = 0x7FFF_FFFF;

// Variant::Type ordinals we encode/decode.
const T_NIL: u32 = 0;
const T_BOOL: u32 = 1;
const T_INT: u32 = 2;
const T_FLOAT: u32 = 3;
const T_STRING: u32 = 4;
const T_VECTOR2: u32 = 5;
const T_RECT2: u32 = 7;
const T_VECTOR3: u32 = 9;
const T_TRANSFORM2D: u32 = 11;
const T_PLANE: u32 = 14;
const T_QUATERNION: u32 = 15;
const T_AABB: u32 = 16;
const T_BASIS: u32 = 17;
const T_TRANSFORM3D: u32 = 18;
const T_COLOR: u32 = 20;
const T_STRING_NAME: u32 = 21;
const T_NODE_PATH: u32 = 22;
const T_DICTIONARY: u32 = 27;
const T_ARRAY: u32 = 28;
const T_PACKED_BYTE_ARRAY: u32 = 29;
const T_PACKED_INT32_ARRAY: u32 = 30;
const T_PACKED_INT64_ARRAY: u32 = 31;
const T_PACKED_FLOAT32_ARRAY: u32 = 32;
const T_PACKED_FLOAT64_ARRAY: u32 = 33;
const T_PACKED_STRING_ARRAY: u32 = 34;
const T_PACKED_VECTOR2_ARRAY: u32 = 35;
const T_PACKED_VECTOR3_ARRAY: u32 = 36;
const T_PACKED_COLOR_ARRAY: u32 = 37;

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Error returned when a [`Variant`] cannot be binary-encoded.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum VariantEncodeError {
    /// The variant type has no binary wire representation in this port.
    #[error("cannot binary-encode unsupported variant type: {0}")]
    Unsupported(VariantType),
    /// Nesting exceeded [`MAX_RECURSION_DEPTH`].
    #[error("variant nesting too deep to encode")]
    RecursionLimit,
}

/// Error returned when a byte buffer cannot be decoded into a [`Variant`].
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum VariantDecodeError {
    /// The buffer ended before a field could be fully read.
    #[error("unexpected end of buffer: needed {needed} more byte(s), had {available}")]
    UnexpectedEof {
        /// Bytes the reader tried to consume.
        needed: usize,
        /// Bytes actually remaining.
        available: usize,
    },
    /// The header's type ordinal is `>= VARIANT_MAX`.
    #[error("invalid variant type id: {0}")]
    InvalidTypeId(u32),
    /// The type ordinal is valid in Godot but not supported by this port.
    #[error("unsupported variant type id: {0}")]
    UnsupportedType(u32),
    /// An encoded string was not valid UTF-8.
    #[error("invalid UTF-8 in encoded string")]
    InvalidUtf8,
    /// A typed array/dictionary (container-type header) was encountered.
    #[error("typed arrays/dictionaries are not supported")]
    TypedContainer,
    /// A shared array/dictionary (count high bit set) was encountered.
    #[error("shared array/dictionary flag is not supported")]
    SharedContainer,
    /// Nesting exceeded [`MAX_RECURSION_DEPTH`].
    #[error("variant nesting too deep to decode")]
    RecursionLimit,
    /// Structurally invalid data with a static reason.
    #[error("invalid variant data: {0}")]
    InvalidData(&'static str),
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Encodes a [`Variant`] into Godot's binary wire format.
///
/// The returned buffer length is always a multiple of 4.
pub fn encode_variant(value: &Variant) -> Result<Vec<u8>, VariantEncodeError> {
    let mut out = Vec::new();
    encode_into(value, &mut out, 0)?;
    Ok(out)
}

/// Decodes a single [`Variant`] from the front of `buf`.
///
/// Returns the decoded value and the number of bytes consumed. Trailing bytes
/// after the variant are tolerated (mirroring Godot's `r_len` semantics); the
/// caller may compare the consumed length against `buf.len()` if it wants to
/// reject trailing data.
pub fn decode_variant(buf: &[u8]) -> Result<(Variant, usize), VariantDecodeError> {
    let mut r = Reader::new(buf);
    let value = decode_from(&mut r, 0)?;
    Ok((value, r.pos))
}

// ---------------------------------------------------------------------------
// Encoding
// ---------------------------------------------------------------------------

fn put_u32(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_le_bytes());
}

fn put_f32(out: &mut Vec<u8>, v: f32) {
    out.extend_from_slice(&v.to_le_bytes());
}

/// Encodes a string as `u32 length + UTF-8 bytes + NUL padding to 4 bytes`.
///
/// Because every field is 4-aligned, padding the UTF-8 payload to a 4-byte
/// boundary reproduces Godot's `_encode_string` exactly.
fn encode_string(s: &str, out: &mut Vec<u8>) {
    let bytes = s.as_bytes();
    put_u32(out, bytes.len() as u32);
    out.extend_from_slice(bytes);
    let pad = (4 - bytes.len() % 4) % 4;
    out.resize(out.len() + pad, 0);
}

fn encode_node_path(np: &NodePath, out: &mut Vec<u8>) {
    let name_count = np.get_name_count();
    let subname_count = np.get_subname_count();
    // "New format" marker in the name-count field.
    put_u32(out, (name_count as u32) | 0x8000_0000);
    put_u32(out, subname_count as u32);
    let flags = if np.is_absolute() { 1 } else { 0 };
    put_u32(out, flags);
    for i in 0..name_count {
        encode_string(np.get_name(i).unwrap_or(""), out);
    }
    for i in 0..subname_count {
        encode_string(np.get_subname(i).unwrap_or(""), out);
    }
}

fn encode_into(
    value: &Variant,
    out: &mut Vec<u8>,
    depth: u32,
) -> Result<(), VariantEncodeError> {
    if depth > MAX_RECURSION_DEPTH {
        return Err(VariantEncodeError::RecursionLimit);
    }

    match value {
        Variant::Nil => put_u32(out, T_NIL),
        Variant::Bool(b) => {
            put_u32(out, T_BOOL);
            put_u32(out, u32::from(*b));
        }
        Variant::Int(i) => {
            let val = *i;
            if val > i32::MAX as i64 || val < i32::MIN as i64 {
                put_u32(out, T_INT | HEADER_DATA_FLAG_64);
                out.extend_from_slice(&(val as u64).to_le_bytes());
            } else {
                put_u32(out, T_INT);
                // Low 32 bits; decoder sign-extends via i32.
                out.extend_from_slice(&(val as u32).to_le_bytes());
            }
        }
        Variant::Float(d) => {
            // Encode as f32 only when the value survives the narrowing exactly
            // (Godot: `double(float(d)) == d`), else keep full f64.
            let f = *d as f32;
            if f as f64 == *d {
                put_u32(out, T_FLOAT);
                out.extend_from_slice(&f.to_le_bytes());
            } else {
                put_u32(out, T_FLOAT | HEADER_DATA_FLAG_64);
                out.extend_from_slice(&d.to_le_bytes());
            }
        }
        Variant::String(s) => {
            put_u32(out, T_STRING);
            encode_string(s, out);
        }
        Variant::StringName(sn) => {
            put_u32(out, T_STRING_NAME);
            encode_string(sn.as_str(), out);
        }
        Variant::NodePath(np) => {
            put_u32(out, T_NODE_PATH);
            encode_node_path(np, out);
        }
        Variant::Vector2(v) => {
            put_u32(out, T_VECTOR2);
            put_f32(out, v.x);
            put_f32(out, v.y);
        }
        Variant::Vector3(v) => {
            put_u32(out, T_VECTOR3);
            put_f32(out, v.x);
            put_f32(out, v.y);
            put_f32(out, v.z);
        }
        Variant::Rect2(r) => {
            put_u32(out, T_RECT2);
            put_f32(out, r.position.x);
            put_f32(out, r.position.y);
            put_f32(out, r.size.x);
            put_f32(out, r.size.y);
        }
        Variant::Transform2D(t) => {
            // Column-major on the wire: columns[0], columns[1], columns[2].
            put_u32(out, T_TRANSFORM2D);
            put_f32(out, t.x.x);
            put_f32(out, t.x.y);
            put_f32(out, t.y.x);
            put_f32(out, t.y.y);
            put_f32(out, t.origin.x);
            put_f32(out, t.origin.y);
        }
        Variant::Plane(p) => {
            put_u32(out, T_PLANE);
            put_f32(out, p.normal.x);
            put_f32(out, p.normal.y);
            put_f32(out, p.normal.z);
            put_f32(out, p.d);
        }
        Variant::Quaternion(q) => {
            put_u32(out, T_QUATERNION);
            put_f32(out, q.x);
            put_f32(out, q.y);
            put_f32(out, q.z);
            put_f32(out, q.w);
        }
        Variant::Aabb(a) => {
            put_u32(out, T_AABB);
            put_f32(out, a.position.x);
            put_f32(out, a.position.y);
            put_f32(out, a.position.z);
            put_f32(out, a.size.x);
            put_f32(out, a.size.y);
            put_f32(out, a.size.z);
        }
        Variant::Basis(b) => {
            put_u32(out, T_BASIS);
            put_basis_rows(b, out);
        }
        Variant::Transform3D(t) => {
            put_u32(out, T_TRANSFORM3D);
            put_basis_rows(&t.basis, out);
            put_f32(out, t.origin.x);
            put_f32(out, t.origin.y);
            put_f32(out, t.origin.z);
        }
        Variant::Color(c) => {
            put_u32(out, T_COLOR);
            put_f32(out, c.r);
            put_f32(out, c.g);
            put_f32(out, c.b);
            put_f32(out, c.a);
        }
        Variant::Array(arr) => {
            put_u32(out, T_ARRAY);
            put_u32(out, arr.len() as u32);
            for elem in arr {
                encode_into(elem, out, depth + 1)?;
            }
        }
        Variant::Dictionary(dict) => {
            put_u32(out, T_DICTIONARY);
            put_u32(out, dict.len() as u32);
            for (key, val) in dict {
                // This crate's Dictionary is String-keyed; keys encode as String.
                encode_into(&Variant::String(key.clone()), out, depth + 1)?;
                encode_into(val, out, depth + 1)?;
            }
        }
        Variant::PackedByteArray(v) => {
            put_u32(out, T_PACKED_BYTE_ARRAY);
            put_u32(out, v.len() as u32);
            out.extend_from_slice(v);
            // Pad the payload to a 4-byte boundary (Godot marshalls.cpp).
            let pad = (4 - v.len() % 4) % 4;
            out.resize(out.len() + pad, 0);
        }
        Variant::PackedInt32Array(v) => {
            put_u32(out, T_PACKED_INT32_ARRAY);
            put_u32(out, v.len() as u32);
            for x in v {
                out.extend_from_slice(&x.to_le_bytes());
            }
        }
        Variant::PackedInt64Array(v) => {
            put_u32(out, T_PACKED_INT64_ARRAY);
            put_u32(out, v.len() as u32);
            for x in v {
                out.extend_from_slice(&x.to_le_bytes());
            }
        }
        Variant::PackedFloat32Array(v) => {
            put_u32(out, T_PACKED_FLOAT32_ARRAY);
            put_u32(out, v.len() as u32);
            for x in v {
                out.extend_from_slice(&x.to_le_bytes());
            }
        }
        Variant::PackedFloat64Array(v) => {
            put_u32(out, T_PACKED_FLOAT64_ARRAY);
            put_u32(out, v.len() as u32);
            for x in v {
                out.extend_from_slice(&x.to_le_bytes());
            }
        }
        Variant::PackedStringArray(v) => {
            put_u32(out, T_PACKED_STRING_ARRAY);
            put_u32(out, v.len() as u32);
            for s in v {
                encode_string(s, out);
            }
        }
        Variant::PackedVector2Array(v) => {
            put_u32(out, T_PACKED_VECTOR2_ARRAY);
            put_u32(out, v.len() as u32);
            for p in v {
                put_f32(out, p.x);
                put_f32(out, p.y);
            }
        }
        Variant::PackedVector3Array(v) => {
            put_u32(out, T_PACKED_VECTOR3_ARRAY);
            put_u32(out, v.len() as u32);
            for p in v {
                put_f32(out, p.x);
                put_f32(out, p.y);
                put_f32(out, p.z);
            }
        }
        Variant::PackedColorArray(v) => {
            put_u32(out, T_PACKED_COLOR_ARRAY);
            put_u32(out, v.len() as u32);
            for c in v {
                put_f32(out, c.r);
                put_f32(out, c.g);
                put_f32(out, c.b);
                put_f32(out, c.a);
            }
        }
        Variant::ObjectId(_) => return Err(VariantEncodeError::Unsupported(VariantType::ObjectId)),
        Variant::Callable(_) => return Err(VariantEncodeError::Unsupported(VariantType::Callable)),
        Variant::Resource(_) => return Err(VariantEncodeError::Unsupported(VariantType::Resource)),
    }

    Ok(())
}

/// Writes a [`Basis`] in Godot's row-major order.
///
/// This engine stores a `Basis` as column vectors (`x`, `y`, `z`), whereas the
/// wire format is `rows[i][j]` in row-major order. `rows[i][j]` is column `j`'s
/// component `i`, so the transpose is written out.
fn put_basis_rows(b: &Basis, out: &mut Vec<u8>) {
    // row 0: (col0.x, col1.x, col2.x)
    put_f32(out, b.x.x);
    put_f32(out, b.y.x);
    put_f32(out, b.z.x);
    // row 1: (col0.y, col1.y, col2.y)
    put_f32(out, b.x.y);
    put_f32(out, b.y.y);
    put_f32(out, b.z.y);
    // row 2: (col0.z, col1.z, col2.z)
    put_f32(out, b.x.z);
    put_f32(out, b.y.z);
    put_f32(out, b.z.z);
}

// ---------------------------------------------------------------------------
// Decoding
// ---------------------------------------------------------------------------

/// A bounds-checked little-endian cursor over the input buffer.
struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn new(buf: &'a [u8]) -> Self {
        Self { buf, pos: 0 }
    }

    fn remaining(&self) -> usize {
        self.buf.len() - self.pos
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8], VariantDecodeError> {
        if self.remaining() < n {
            return Err(VariantDecodeError::UnexpectedEof {
                needed: n,
                available: self.remaining(),
            });
        }
        let s = &self.buf[self.pos..self.pos + n];
        self.pos += n;
        Ok(s)
    }

    fn u32(&mut self) -> Result<u32, VariantDecodeError> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }

    fn u64(&mut self) -> Result<u64, VariantDecodeError> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }

    fn f32(&mut self) -> Result<f32, VariantDecodeError> {
        Ok(f32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }

    fn f64(&mut self) -> Result<f64, VariantDecodeError> {
        Ok(f64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }

    /// Reads one math scalar, honoring the 64-bit flag but always returning `f32`
    /// (this engine's `real_t`).
    fn real(&mut self, flag64: bool) -> Result<f32, VariantDecodeError> {
        if flag64 {
            Ok(self.f64()? as f32)
        } else {
            self.f32()
        }
    }
}

/// Decodes a string: `u32 length + UTF-8 bytes + padding to 4 bytes`.
fn decode_string(r: &mut Reader) -> Result<String, VariantDecodeError> {
    let strlen = r.u32()? as usize;
    let pad = (4 - strlen % 4) % 4;
    let bytes = r.take(strlen)?;
    let s = std::str::from_utf8(bytes)
        .map_err(|_| VariantDecodeError::InvalidUtf8)?
        .to_string();
    r.take(pad)?; // consume padding
    Ok(s)
}

fn decode_node_path(r: &mut Reader) -> Result<Variant, VariantDecodeError> {
    let marker = r.u32()?;
    if marker & 0x8000_0000 == 0 {
        // Old format (a single string) is rejected by Godot as well.
        return Err(VariantDecodeError::InvalidData(
            "legacy string-form NodePath not supported",
        ));
    }
    let name_count = (marker & 0x7FFF_FFFF) as usize;
    let mut subname_count = r.u32()? as usize;
    let np_flags = r.u32()?;
    if np_flags & 2 != 0 {
        // Obsolete "property separate from subpath" format: one extra subname.
        subname_count += 1;
    }

    let total = name_count + subname_count;
    let mut names = Vec::new();
    let mut subnames = Vec::new();
    for i in 0..total {
        let s = decode_string(r)?;
        if i < name_count {
            names.push(s);
        } else {
            subnames.push(s);
        }
    }

    Ok(Variant::NodePath(build_node_path(
        &names,
        &subnames,
        np_flags & 1 != 0,
    )))
}

/// Rebuilds a [`NodePath`] from its decoded parts using the public string API.
fn build_node_path(names: &[String], subnames: &[String], absolute: bool) -> NodePath {
    let mut s = String::new();
    if absolute {
        s.push('/');
    }
    s.push_str(&names.join("/"));
    for sub in subnames {
        s.push(':');
        s.push_str(sub);
    }
    NodePath::new(&s)
}

/// Reads a container count, rejecting the shared flag and returning the count.
fn decode_container_count(r: &mut Reader) -> Result<u32, VariantDecodeError> {
    let raw = r.u32()?;
    if raw & CONTAINER_SHARED_FLAG != 0 {
        return Err(VariantDecodeError::SharedContainer);
    }
    Ok(raw & CONTAINER_COUNT_MASK)
}

fn decode_from(r: &mut Reader, depth: u32) -> Result<Variant, VariantDecodeError> {
    if depth > MAX_RECURSION_DEPTH {
        return Err(VariantDecodeError::RecursionLimit);
    }

    let header = r.u32()?;
    let type_id = header & HEADER_TYPE_MASK;
    if type_id >= VARIANT_MAX {
        return Err(VariantDecodeError::InvalidTypeId(type_id));
    }
    let flag64 = header & HEADER_DATA_FLAG_64 != 0;

    match type_id {
        T_NIL => Ok(Variant::Nil),
        T_BOOL => Ok(Variant::Bool(r.u32()? != 0)),
        T_INT => {
            if flag64 {
                Ok(Variant::Int(r.u64()? as i64))
            } else {
                Ok(Variant::Int(r.u32()? as i32 as i64))
            }
        }
        T_FLOAT => {
            if flag64 {
                Ok(Variant::Float(r.f64()?))
            } else {
                Ok(Variant::Float(r.f32()? as f64))
            }
        }
        T_STRING => Ok(Variant::String(decode_string(r)?)),
        T_STRING_NAME => Ok(Variant::StringName(StringName::new(&decode_string(r)?))),
        T_NODE_PATH => decode_node_path(r),
        T_VECTOR2 => {
            let x = r.real(flag64)?;
            let y = r.real(flag64)?;
            Ok(Variant::Vector2(Vector2::new(x, y)))
        }
        T_VECTOR3 => {
            let x = r.real(flag64)?;
            let y = r.real(flag64)?;
            let z = r.real(flag64)?;
            Ok(Variant::Vector3(Vector3::new(x, y, z)))
        }
        T_RECT2 => {
            let px = r.real(flag64)?;
            let py = r.real(flag64)?;
            let sx = r.real(flag64)?;
            let sy = r.real(flag64)?;
            Ok(Variant::Rect2(Rect2::new(
                Vector2::new(px, py),
                Vector2::new(sx, sy),
            )))
        }
        T_TRANSFORM2D => {
            let c: [f32; 6] = read_reals(r, flag64)?;
            Ok(Variant::Transform2D(Transform2D {
                x: Vector2::new(c[0], c[1]),
                y: Vector2::new(c[2], c[3]),
                origin: Vector2::new(c[4], c[5]),
            }))
        }
        T_PLANE => {
            let nx = r.real(flag64)?;
            let ny = r.real(flag64)?;
            let nz = r.real(flag64)?;
            let d = r.real(flag64)?;
            Ok(Variant::Plane(Plane {
                normal: Vector3::new(nx, ny, nz),
                d,
            }))
        }
        T_QUATERNION => {
            let x = r.real(flag64)?;
            let y = r.real(flag64)?;
            let z = r.real(flag64)?;
            let w = r.real(flag64)?;
            Ok(Variant::Quaternion(Quaternion { x, y, z, w }))
        }
        T_AABB => {
            let px = r.real(flag64)?;
            let py = r.real(flag64)?;
            let pz = r.real(flag64)?;
            let sx = r.real(flag64)?;
            let sy = r.real(flag64)?;
            let sz = r.real(flag64)?;
            Ok(Variant::Aabb(Aabb {
                position: Vector3::new(px, py, pz),
                size: Vector3::new(sx, sy, sz),
            }))
        }
        T_BASIS => {
            let rows: [f32; 9] = read_reals(r, flag64)?;
            Ok(Variant::Basis(basis_from_rows(&rows)))
        }
        T_TRANSFORM3D => {
            let rows: [f32; 9] = read_reals(r, flag64)?;
            let ox = r.real(flag64)?;
            let oy = r.real(flag64)?;
            let oz = r.real(flag64)?;
            Ok(Variant::Transform3D(Transform3D {
                basis: basis_from_rows(&rows),
                origin: Vector3::new(ox, oy, oz),
            }))
        }
        T_COLOR => {
            // Colors are always single-precision.
            let cr = r.f32()?;
            let cg = r.f32()?;
            let cb = r.f32()?;
            let ca = r.f32()?;
            Ok(Variant::Color(Color::new(cr, cg, cb, ca)))
        }
        T_ARRAY => {
            if header & TYPED_ARRAY_MASK != 0 {
                return Err(VariantDecodeError::TypedContainer);
            }
            let count = decode_container_count(r)?;
            let mut arr = Vec::new();
            for _ in 0..count {
                arr.push(decode_from(r, depth + 1)?);
            }
            Ok(Variant::Array(arr))
        }
        T_DICTIONARY => {
            if header & (TYPED_DICT_KEY_MASK | TYPED_DICT_VALUE_MASK) != 0 {
                return Err(VariantDecodeError::TypedContainer);
            }
            let count = decode_container_count(r)?;
            let mut map = HashMap::new();
            for _ in 0..count {
                let key = decode_from(r, depth + 1)?;
                let value = decode_from(r, depth + 1)?;
                // This crate's Dictionary is String-keyed. Non-String keys from
                // external data are coerced to their string form (lossy, but our
                // own encoder only ever emits String keys, so round-trips exactly).
                let key_str = match key {
                    Variant::String(s) => s,
                    other => other.to_string_lossy(),
                };
                map.insert(key_str, value);
            }
            Ok(Variant::Dictionary(map))
        }
        T_PACKED_BYTE_ARRAY => {
            let count = decode_container_count(r)? as usize;
            let bytes = r.take(count)?.to_vec();
            // Payload is padded to a 4-byte boundary; consume the padding.
            let pad = (4 - count % 4) % 4;
            r.take(pad)?;
            Ok(Variant::PackedByteArray(bytes))
        }
        T_PACKED_INT32_ARRAY => {
            let count = decode_container_count(r)? as usize;
            let mut v = Vec::new();
            for _ in 0..count {
                v.push(r.u32()? as i32);
            }
            Ok(Variant::PackedInt32Array(v))
        }
        T_PACKED_INT64_ARRAY => {
            let count = decode_container_count(r)? as usize;
            let mut v = Vec::new();
            for _ in 0..count {
                v.push(r.u64()? as i64);
            }
            Ok(Variant::PackedInt64Array(v))
        }
        T_PACKED_FLOAT32_ARRAY => {
            let count = decode_container_count(r)? as usize;
            let mut v = Vec::new();
            for _ in 0..count {
                v.push(r.f32()?);
            }
            Ok(Variant::PackedFloat32Array(v))
        }
        T_PACKED_FLOAT64_ARRAY => {
            let count = decode_container_count(r)? as usize;
            let mut v = Vec::new();
            for _ in 0..count {
                v.push(r.f64()?);
            }
            Ok(Variant::PackedFloat64Array(v))
        }
        T_PACKED_STRING_ARRAY => {
            let count = decode_container_count(r)? as usize;
            let mut v = Vec::new();
            for _ in 0..count {
                v.push(decode_string(r)?);
            }
            Ok(Variant::PackedStringArray(v))
        }
        T_PACKED_VECTOR2_ARRAY => {
            let count = decode_container_count(r)? as usize;
            let mut v = Vec::new();
            for _ in 0..count {
                let x = r.f32()?;
                let y = r.f32()?;
                v.push(Vector2::new(x, y));
            }
            Ok(Variant::PackedVector2Array(v))
        }
        T_PACKED_VECTOR3_ARRAY => {
            let count = decode_container_count(r)? as usize;
            let mut v = Vec::new();
            for _ in 0..count {
                let x = r.f32()?;
                let y = r.f32()?;
                let z = r.f32()?;
                v.push(Vector3::new(x, y, z));
            }
            Ok(Variant::PackedVector3Array(v))
        }
        T_PACKED_COLOR_ARRAY => {
            let count = decode_container_count(r)? as usize;
            let mut v = Vec::new();
            for _ in 0..count {
                let cr = r.f32()?;
                let cg = r.f32()?;
                let cb = r.f32()?;
                let ca = r.f32()?;
                v.push(Color::new(cr, cg, cb, ca));
            }
            Ok(Variant::PackedColorArray(v))
        }
        // Recognized Godot types this port does not (yet) support: VECTOR2I(6),
        // RECT2I(8), VECTOR3I(10), VECTOR4(12), VECTOR4I(13), PROJECTION(19),
        // RID(23), OBJECT(24), CALLABLE(25), SIGNAL(26), and PACKED_VECTOR4_ARRAY(38).
        other => Err(VariantDecodeError::UnsupportedType(other)),
    }
}

/// Reads `N` math scalars into a fixed array.
fn read_reals<const N: usize>(
    r: &mut Reader,
    flag64: bool,
) -> Result<[f32; N], VariantDecodeError> {
    let mut out = [0.0f32; N];
    for slot in out.iter_mut() {
        *slot = r.real(flag64)?;
    }
    Ok(out)
}

/// Rebuilds a column-stored [`Basis`] from 9 row-major wire values.
///
/// Inverse of [`put_basis_rows`]: `rows[i][j]` is column `j` component `i`, so
/// column 0 = (rows[0][0], rows[1][0], rows[2][0]) = (r[0], r[3], r[6]).
fn basis_from_rows(r: &[f32; 9]) -> Basis {
    Basis {
        x: Vector3::new(r[0], r[3], r[6]),
        y: Vector3::new(r[1], r[4], r[7]),
        z: Vector3::new(r[2], r[5], r[8]),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use gdcore::id::ObjectId;

    /// Encode → decode → assert equal, all bytes consumed, and length a mult of 4.
    fn roundtrip(v: Variant) {
        let bytes = encode_variant(&v).expect("encode");
        assert_eq!(bytes.len() % 4, 0, "encoded length must be 4-aligned: {v:?}");
        let (decoded, used) = decode_variant(&bytes).expect("decode");
        assert_eq!(decoded, v, "round-trip mismatch");
        assert_eq!(used, bytes.len(), "consumed length mismatch for {v:?}");
    }

    // -- (a) Round-trip every supported type, including nesting ---------------

    #[test]
    fn roundtrip_scalars() {
        roundtrip(Variant::Nil);
        roundtrip(Variant::Bool(true));
        roundtrip(Variant::Bool(false));
        roundtrip(Variant::Int(0));
        roundtrip(Variant::Int(42));
        roundtrip(Variant::Int(-7));
        roundtrip(Variant::Int(i32::MAX as i64));
        roundtrip(Variant::Int(i32::MIN as i64));
        roundtrip(Variant::Int(i32::MAX as i64 + 1)); // forces 64-bit
        roundtrip(Variant::Int(1i64 << 40));
        roundtrip(Variant::Int(i64::MAX));
        roundtrip(Variant::Int(i64::MIN));
        roundtrip(Variant::Float(0.0));
        roundtrip(Variant::Float(0.5)); // f32-exact
        roundtrip(Variant::Float(0.1)); // needs f64
        roundtrip(Variant::Float(-3.14159265358979));
        roundtrip(Variant::Float(1e300)); // needs f64
    }

    #[test]
    fn roundtrip_strings_and_paths() {
        roundtrip(Variant::String(String::new()));
        roundtrip(Variant::String("abc".into())); // needs padding
        roundtrip(Variant::String("four".into())); // exactly aligned
        roundtrip(Variant::String("héllo→".into())); // multibyte UTF-8
        roundtrip(Variant::StringName(StringName::new("some_name")));
        roundtrip(Variant::StringName(StringName::new("")));
        roundtrip(Variant::NodePath(NodePath::new("/root/Player/Sprite")));
        roundtrip(Variant::NodePath(NodePath::new("Player:position:x")));
        roundtrip(Variant::NodePath(NodePath::new("../Sibling")));
        roundtrip(Variant::NodePath(NodePath::new("")));
    }

    #[test]
    fn roundtrip_math_types() {
        roundtrip(Variant::Vector2(Vector2::new(1.0, -2.5)));
        roundtrip(Variant::Vector3(Vector3::new(1.0, 2.0, 3.0)));
        roundtrip(Variant::Rect2(Rect2::new(
            Vector2::new(1.0, 2.0),
            Vector2::new(3.0, 4.0),
        )));
        roundtrip(Variant::Transform2D(Transform2D {
            x: Vector2::new(1.0, 2.0),
            y: Vector2::new(3.0, 4.0),
            origin: Vector2::new(5.0, 6.0),
        }));
        roundtrip(Variant::Plane(Plane {
            normal: Vector3::new(0.0, 1.0, 0.0),
            d: 5.5,
        }));
        roundtrip(Variant::Quaternion(Quaternion {
            x: 0.1,
            y: 0.2,
            z: 0.3,
            w: 0.4,
        }));
        roundtrip(Variant::Aabb(Aabb {
            position: Vector3::new(1.0, 2.0, 3.0),
            size: Vector3::new(4.0, 5.0, 6.0),
        }));
        roundtrip(Variant::Color(Color::new(0.1, 0.2, 0.3, 1.0)));
    }

    #[test]
    fn roundtrip_basis_and_transform3d_asymmetric() {
        // Non-symmetric so a wrong transpose would be caught.
        let basis = Basis {
            x: Vector3::new(1.0, 2.0, 3.0),
            y: Vector3::new(4.0, 5.0, 6.0),
            z: Vector3::new(7.0, 8.0, 9.0),
        };
        roundtrip(Variant::Basis(basis));
        roundtrip(Variant::Transform3D(Transform3D {
            basis,
            origin: Vector3::new(10.0, 11.0, 12.0),
        }));
    }

    #[test]
    fn roundtrip_containers_and_nesting() {
        roundtrip(Variant::Array(vec![]));
        roundtrip(Variant::Array(vec![
            Variant::Int(1),
            Variant::String("x".into()),
            Variant::Vector2(Vector2::new(1.0, 2.0)),
        ]));

        let mut d = HashMap::new();
        d.insert("pos".to_string(), Variant::Vector2(Vector2::new(1.0, 2.0)));
        d.insert("count".to_string(), Variant::Int(5));
        roundtrip(Variant::Dictionary(d));

        // Array of dicts of vectors (deep nesting).
        let mut d1 = HashMap::new();
        d1.insert("a".to_string(), Variant::Vector3(Vector3::new(1.0, 2.0, 3.0)));
        let mut d2 = HashMap::new();
        d2.insert("b".to_string(), Variant::Color(Color::new(1.0, 0.0, 0.0, 1.0)));
        d2.insert("nested".to_string(), Variant::Array(vec![Variant::Int(9)]));
        roundtrip(Variant::Array(vec![
            Variant::Dictionary(d1),
            Variant::Dictionary(d2),
        ]));
    }

    // -- Basis wire order is row-major (Godot fidelity, not just round-trip) --

    #[test]
    fn basis_wire_is_row_major() {
        let basis = Basis {
            x: Vector3::new(1.0, 2.0, 3.0), // column 0
            y: Vector3::new(4.0, 5.0, 6.0), // column 1
            z: Vector3::new(7.0, 8.0, 9.0), // column 2
        };
        let bytes = encode_variant(&Variant::Basis(basis)).unwrap();
        // Skip the 4-byte header; read 9 f32 in wire order.
        let mut vals = [0f32; 9];
        for (i, slot) in vals.iter_mut().enumerate() {
            let off = 4 + i * 4;
            *slot = f32::from_le_bytes(bytes[off..off + 4].try_into().unwrap());
        }
        // Godot rows[i][j]: row0=(c0.x,c1.x,c2.x), row1=(c0.y,c1.y,c2.y), ...
        assert_eq!(vals, [1.0, 4.0, 7.0, 2.0, 5.0, 8.0, 3.0, 6.0, 9.0]);
    }

    // -- (b) Hand-built byte fixtures (layout commented) ---------------------

    /// Assert `v` encodes to exactly `expected` and decodes back to `v`.
    fn assert_fixture(v: Variant, expected: &[u8]) {
        assert_eq!(encode_variant(&v).unwrap().as_slice(), expected, "encode {v:?}");
        let (decoded, used) = decode_variant(expected).unwrap();
        assert_eq!(decoded, v, "decode {v:?}");
        assert_eq!(used, expected.len());
    }

    #[test]
    fn fixture_nil() {
        // header = NIL(0)
        assert_fixture(Variant::Nil, &[0, 0, 0, 0]);
    }

    #[test]
    fn fixture_bool_true() {
        // header = BOOL(1), then u32(1)
        assert_fixture(Variant::Bool(true), &[1, 0, 0, 0, 1, 0, 0, 0]);
    }

    #[test]
    fn fixture_int_32bit() {
        // header = INT(2), then i32(42) little-endian
        assert_fixture(Variant::Int(42), &[2, 0, 0, 0, 42, 0, 0, 0]);
    }

    #[test]
    fn fixture_int_64bit() {
        // header = INT(2) | FLAG_64(0x10000) = 0x10002, then u64(2^40) LE.
        // 2^40 = 0x0000_0100_0000_0000 -> LE bytes 00 00 00 00 00 01 00 00
        assert_fixture(
            Variant::Int(1i64 << 40),
            &[0x02, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00],
        );
    }

    #[test]
    fn fixture_float_f32() {
        // header = FLOAT(3), then f32(0.5) = 0x3F000000 LE
        assert_fixture(Variant::Float(0.5), &[3, 0, 0, 0, 0x00, 0x00, 0x00, 0x3F]);
    }

    #[test]
    fn fixture_float_f64() {
        // header = FLOAT(3) | FLAG_64 = 0x10003, then f64(0.1) = 0x3FB999999999999A LE
        assert_fixture(
            Variant::Float(0.1),
            &[
                0x03, 0x00, 0x01, 0x00, 0x9A, 0x99, 0x99, 0x99, 0x99, 0x99, 0xB9, 0x3F,
            ],
        );
    }

    #[test]
    fn fixture_string_padded() {
        // header = STRING(4), u32 len(3), 'a','b','c', 1 pad byte
        assert_fixture(
            Variant::String("abc".into()),
            &[4, 0, 0, 0, 3, 0, 0, 0, b'a', b'b', b'c', 0],
        );
    }

    #[test]
    fn fixture_vector2() {
        // header = VECTOR2(5), f32(1.0)=0x3F800000, f32(2.0)=0x40000000 (LE)
        assert_fixture(
            Variant::Vector2(Vector2::new(1.0, 2.0)),
            &[5, 0, 0, 0, 0x00, 0x00, 0x80, 0x3F, 0x00, 0x00, 0x00, 0x40],
        );
    }

    #[test]
    fn fixture_array_of_ints() {
        // header = ARRAY(28), u32 count(2), then INT(1), INT(2)
        assert_fixture(
            Variant::Array(vec![Variant::Int(1), Variant::Int(2)]),
            &[
                28, 0, 0, 0, // ARRAY
                2, 0, 0, 0, // count = 2
                2, 0, 0, 0, 1, 0, 0, 0, // Int(1)
                2, 0, 0, 0, 2, 0, 0, 0, // Int(2)
            ],
        );
    }

    // -- (c) Truncated input never panics and always errors ------------------

    #[test]
    fn truncated_input_errors_at_every_length() {
        // A deeply-structured value: array of a dict, a string, and a transform.
        let mut d = HashMap::new();
        d.insert("k".to_string(), Variant::Vector3(Vector3::new(1.0, 2.0, 3.0)));
        d.insert("s".to_string(), Variant::String("hello".into()));
        let v = Variant::Array(vec![
            Variant::Dictionary(d),
            Variant::NodePath(NodePath::new("/a/b:c")),
            Variant::Transform3D(Transform3D::IDENTITY),
        ]);
        let bytes = encode_variant(&v).unwrap();

        // Full buffer decodes and consumes everything.
        let (decoded, used) = decode_variant(&bytes).unwrap();
        assert_eq!(decoded, v);
        assert_eq!(used, bytes.len());

        // Every strict prefix must error rather than panic.
        for len in 0..bytes.len() {
            assert!(
                decode_variant(&bytes[..len]).is_err(),
                "prefix of length {len} should not decode"
            );
        }
    }

    #[test]
    fn trailing_bytes_are_tolerated() {
        // r_len semantics: decode reports what it consumed, ignoring extra bytes.
        let mut bytes = encode_variant(&Variant::Int(7)).unwrap();
        let consumed = bytes.len();
        bytes.extend_from_slice(&[0xDE, 0xAD, 0xBE, 0xEF]);
        let (decoded, used) = decode_variant(&bytes).unwrap();
        assert_eq!(decoded, Variant::Int(7));
        assert_eq!(used, consumed);
    }

    // -- (d) Unknown / unsupported types -------------------------------------

    #[test]
    fn decode_invalid_type_id_errors() {
        // 100 >= VARIANT_MAX(39).
        let err = decode_variant(&[100, 0, 0, 0]).unwrap_err();
        assert_eq!(err, VariantDecodeError::InvalidTypeId(100));
    }

    #[test]
    fn decode_unsupported_type_id_errors() {
        // OBJECT(24) is a valid Godot type but unsupported here.
        let err = decode_variant(&[24, 0, 0, 0]).unwrap_err();
        assert_eq!(err, VariantDecodeError::UnsupportedType(24));
        // PACKED_VECTOR4_ARRAY(38) is recognized by Godot but not carried here.
        assert_eq!(
            decode_variant(&[38, 0, 0, 0, 0, 0, 0, 0]).unwrap_err(),
            VariantDecodeError::UnsupportedType(38)
        );
    }

    #[test]
    fn packed_byte_array_now_decodes() {
        // PACKED_BYTE_ARRAY(29), count 0 → empty (previously UnsupportedType).
        let (v, used) = decode_variant(&[29, 0, 0, 0, 0, 0, 0, 0]).unwrap();
        assert_eq!(v, Variant::PackedByteArray(vec![]));
        assert_eq!(used, 8);
    }

    // -- Packed array round-trips (Godot wire ordinals 29-37) ----------------

    #[test]
    fn roundtrip_packed_arrays() {
        roundtrip(Variant::PackedByteArray(vec![]));
        roundtrip(Variant::PackedByteArray(vec![1, 2, 3])); // needs 1 pad byte
        roundtrip(Variant::PackedByteArray(vec![1, 2, 3, 4])); // exactly aligned
        roundtrip(Variant::PackedInt32Array(vec![0, -1, i32::MAX, i32::MIN]));
        roundtrip(Variant::PackedInt64Array(vec![0, i64::MAX, i64::MIN]));
        roundtrip(Variant::PackedFloat32Array(vec![0.0, 0.5, -3.25]));
        roundtrip(Variant::PackedFloat64Array(vec![0.1, 1e300, -2.5]));
        roundtrip(Variant::PackedStringArray(vec![
            "".into(),
            "abc".into(),
            "héllo".into(),
        ]));
        roundtrip(Variant::PackedVector2Array(vec![
            Vector2::new(1.0, 2.0),
            Vector2::new(-3.0, 4.5),
        ]));
        roundtrip(Variant::PackedVector3Array(vec![Vector3::new(1.0, 2.0, 3.0)]));
        roundtrip(Variant::PackedColorArray(vec![Color::new(0.1, 0.2, 0.3, 1.0)]));
    }

    #[test]
    fn packed_byte_array_wire_layout() {
        // header PACKED_BYTE_ARRAY(29), count 3, bytes 0A 0B 0C, 1 pad byte.
        assert_fixture(
            Variant::PackedByteArray(vec![10, 11, 12]),
            &[29, 0, 0, 0, 3, 0, 0, 0, 10, 11, 12, 0],
        );
    }

    #[test]
    fn packed_int32_array_wire_layout() {
        // header PACKED_INT32_ARRAY(30), count 2, then i32(1), i32(2).
        assert_fixture(
            Variant::PackedInt32Array(vec![1, 2]),
            &[30, 0, 0, 0, 2, 0, 0, 0, 1, 0, 0, 0, 2, 0, 0, 0],
        );
    }

    #[test]
    fn packed_arrays_are_4_aligned() {
        // Every packed encoding must be a multiple of 4 bytes (odd byte count too).
        for v in [
            Variant::PackedByteArray(vec![1]),
            Variant::PackedByteArray(vec![1, 2]),
            Variant::PackedStringArray(vec!["ab".into()]),
            Variant::PackedVector3Array(vec![Vector3::new(1.0, 2.0, 3.0)]),
        ] {
            let bytes = encode_variant(&v).unwrap();
            assert_eq!(bytes.len() % 4, 0, "not 4-aligned: {v:?}");
        }
    }

    #[test]
    fn truncated_packed_arrays_error_not_panic() {
        let v = Variant::PackedInt32Array(vec![1, 2, 3]);
        let bytes = encode_variant(&v).unwrap();
        for len in 0..bytes.len() {
            assert!(
                decode_variant(&bytes[..len]).is_err(),
                "prefix len {len} must not decode"
            );
        }
    }

    #[test]
    fn decode_empty_buffer_errors() {
        assert!(matches!(
            decode_variant(&[]).unwrap_err(),
            VariantDecodeError::UnexpectedEof { .. }
        ));
        assert!(matches!(
            decode_variant(&[0, 0]).unwrap_err(),
            VariantDecodeError::UnexpectedEof { .. }
        ));
    }

    #[test]
    fn encode_unsupported_variant_types_error() {
        assert_eq!(
            encode_variant(&Variant::ObjectId(ObjectId::from_raw(1))),
            Err(VariantEncodeError::Unsupported(VariantType::ObjectId))
        );
        // The error propagates out of nested containers.
        let arr = Variant::Array(vec![
            Variant::Int(1),
            Variant::ObjectId(ObjectId::from_raw(2)),
        ]);
        assert_eq!(
            encode_variant(&arr),
            Err(VariantEncodeError::Unsupported(VariantType::ObjectId))
        );
    }

    #[test]
    fn decode_typed_container_rejected() {
        // ARRAY(28) header with a typed-array kind in bits 16-17.
        let header = (T_ARRAY | (0b01 << 16)).to_le_bytes();
        let mut buf = header.to_vec();
        buf.extend_from_slice(&0u32.to_le_bytes()); // count
        assert_eq!(
            decode_variant(&buf).unwrap_err(),
            VariantDecodeError::TypedContainer
        );
    }

    #[test]
    fn decode_shared_container_rejected() {
        // ARRAY(28) header, count with the shared high bit set.
        let mut buf = T_ARRAY.to_le_bytes().to_vec();
        buf.extend_from_slice(&0x8000_0000u32.to_le_bytes());
        assert_eq!(
            decode_variant(&buf).unwrap_err(),
            VariantDecodeError::SharedContainer
        );
    }

    #[test]
    fn decode_invalid_utf8_string_errors() {
        // STRING(4), len 4, then invalid UTF-8 bytes.
        let mut buf = T_STRING.to_le_bytes().to_vec();
        buf.extend_from_slice(&4u32.to_le_bytes());
        buf.extend_from_slice(&[0xFF, 0xFE, 0xFD, 0xFC]);
        assert_eq!(
            decode_variant(&buf).unwrap_err(),
            VariantDecodeError::InvalidUtf8
        );
    }

    #[test]
    fn decode_double_precision_math_downcasts() {
        // A VECTOR2 encoded by a REAL_T_IS_DOUBLE build: header has FLAG_64 and
        // two f64 components. We must read it and downcast to f32.
        let mut buf = (T_VECTOR2 | HEADER_DATA_FLAG_64).to_le_bytes().to_vec();
        buf.extend_from_slice(&1.5f64.to_le_bytes());
        buf.extend_from_slice(&(-2.5f64).to_le_bytes());
        let (v, used) = decode_variant(&buf).unwrap();
        assert_eq!(v, Variant::Vector2(Vector2::new(1.5, -2.5)));
        assert_eq!(used, buf.len());
    }
}
