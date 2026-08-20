//! Godot-identical `Variant::hash()` (pat-0q2kx.230).
//!
//! Godot ground truth, read from the vendored source rather than from memory:
//! `core/variant/variant.cpp` `Variant::recursive_hash`,
//! `core/templates/hashfuncs.{h,cpp}`, `core/math/vector2.h`,
//! `core/string/ustring.cpp` `String::hash`, `core/variant/array.cpp`
//! `Array::recursive_hash`.
//!
//! ⚠️ **The bead description of this algorithm is wrong for Godot 4.x**, and the
//! vendored source settles it. It says "float → hash_djb2_one_float" and
//! "Vector2/3/Color → hash_djb2_buffer over the float components". In 4.6 those
//! are **murmur3**: `FLOAT` uses `hash_murmur3_one_double`, and `Vector2` uses
//! its own `hash()` (murmur3 per component, then `hash_fmix32`). The djb2 float
//! helpers still exist in `hashfuncs.h` but `Variant::hash` no longer calls
//! them. Implementing the description would have produced confidently wrong
//! numbers for every float and vector.
//!
//! Only `INT` (`hash_one_uint64`) and `STRING` (djb2, matching `String::hash`)
//! match the bead's account.
//!
//! Everything here is `u32` arithmetic with explicit wrapping, because the C++
//! relies on unsigned overflow throughout.

use crate::variant::{CallableRef, Variant};

/// `HASH_MURMUR3_SEED` (`hashfuncs.h:108`).
pub const HASH_MURMUR3_SEED: u32 = 0x7F07_C65;

/// `Variant::Type::ARRAY`'s ordinal, which seeds an array's hash
/// (`array.cpp:201`).
///
/// ⚠️ Counted from `variant.h`'s enum: NIL 0, BOOL 1, INT 2, FLOAT 3, STRING 4,
/// then the 15 math types, COLOR 20, STRING_NAME 21, NODE_PATH 22, RID 23,
/// OBJECT 24, CALLABLE 25, SIGNAL 26, DICTIONARY 27, **ARRAY 28**. A wrong
/// ordinal changes every array hash and nothing else, so it is worth pinning.
const VARIANT_TYPE_ARRAY: u32 = 28;

/// `Variant::Type::DICTIONARY`'s ordinal, which seeds a dictionary's hash
/// (`dictionary.cpp:379`) exactly as ARRAY seeds an array's.
const VARIANT_TYPE_DICTIONARY: u32 = 27;

/// Thomas Wang's 64→32 mix (`hashfuncs.h:87`). Godot's `INT` hash.
pub fn hash_one_uint64(p_int: u64) -> u32 {
    let mut v = p_int;
    v = (!v).wrapping_add(v << 18);
    v ^= v >> 31;
    v = v.wrapping_mul(21);
    v ^= v >> 11;
    v = v.wrapping_add(v << 6);
    v ^= v >> 22;
    v as u32
}

/// One murmur3 round over a `u32` (`hashfuncs.h:112`).
pub fn hash_murmur3_one_32(p_in: u32, p_seed: u32) -> u32 {
    let mut k = p_in;
    k = k.wrapping_mul(0xcc9e_2d51);
    k = k.rotate_left(15);
    k = k.wrapping_mul(0x1b87_3593);

    let mut s = p_seed ^ k;
    s = s.rotate_left(13);
    s.wrapping_mul(5).wrapping_add(0xe654_6b64)
}

/// `hash_murmur3_one_float` (`hashfuncs.cpp:33`).
///
/// ⚠️ `+0.0`, `-0.0` and every NaN are normalised so they hash alike — Godot
/// does this explicitly, and without it `-0.0` and `0.0` would land in different
/// dictionary buckets despite comparing equal.
pub fn hash_murmur3_one_float(p_in: f32, p_seed: u32) -> u32 {
    let bits = if p_in == 0.0 {
        0.0_f32.to_bits()
    } else if p_in.is_nan() {
        f32::NAN.to_bits()
    } else {
        p_in.to_bits()
    };
    hash_murmur3_one_32(bits, p_seed)
}

/// `hash_murmur3_one_double` (`hashfuncs.cpp:51`) — the low half first, then the
/// high half.
pub fn hash_murmur3_one_double(p_in: f64, p_seed: u32) -> u32 {
    let bits = if p_in == 0.0 {
        0.0_f64.to_bits()
    } else if p_in.is_nan() {
        f64::NAN.to_bits()
    } else {
        p_in.to_bits()
    };
    let s = hash_murmur3_one_32((bits & 0xFFFF_FFFF) as u32, p_seed);
    hash_murmur3_one_32((bits >> 32) as u32, s)
}

/// `hash_murmur3_one_64` (`hashfuncs.h:124`) — low half then high half.
pub fn hash_murmur3_one_64(p_in: u64, p_seed: u32) -> u32 {
    let s = hash_murmur3_one_32((p_in & 0xFFFF_FFFF) as u32, p_seed);
    hash_murmur3_one_32((p_in >> 32) as u32, s)
}

/// Full MurmurHash3_x86_32 over a byte buffer (`hashfuncs.cpp:69`), used by the
/// byte and integer packed arrays.
///
/// ⚠️ Three details that are easy to lose: the body reads **little-endian**
/// `u32` blocks, the 1–3 byte tail is folded in through the same `c1`/rotl15/`c2`
/// path, and the length is XORed into the accumulator **before** `fmix32`.
pub fn hash_murmur3_buffer(data: &[u8], seed: u32) -> u32 {
    const C1: u32 = 0xcc9e_2d51;
    const C2: u32 = 0x1b87_3593;

    let nblocks = data.len() / 4;
    let mut h1 = seed;

    for i in 0..nblocks {
        let b = &data[i * 4..i * 4 + 4];
        let mut k1 = u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
        k1 = k1.wrapping_mul(C1);
        k1 = k1.rotate_left(15);
        k1 = k1.wrapping_mul(C2);

        h1 ^= k1;
        h1 = h1.rotate_left(13);
        h1 = h1.wrapping_mul(5).wrapping_add(0xe654_6b64);
    }

    let tail = &data[nblocks * 4..];
    let mut k1: u32 = 0;
    if tail.len() >= 3 {
        k1 ^= u32::from(tail[2]) << 16;
    }
    if tail.len() >= 2 {
        k1 ^= u32::from(tail[1]) << 8;
    }
    if !tail.is_empty() {
        k1 ^= u32::from(tail[0]);
        k1 = k1.wrapping_mul(C1);
        k1 = k1.rotate_left(15);
        k1 = k1.wrapping_mul(C2);
        h1 ^= k1;
    }

    h1 ^= data.len() as u32;
    hash_fmix32(h1)
}

/// Murmur3's finaliser (`hashfuncs.h:144`).
pub fn hash_fmix32(mut h: u32) -> u32 {
    h ^= h >> 16;
    h = h.wrapping_mul(0x85eb_ca6b);
    h ^= h >> 13;
    h = h.wrapping_mul(0xc2b2_ae35);
    h ^= h >> 16;
    h
}

/// Godot `String::hash()` — djb2 over **code points**, not bytes
/// (`ustring.cpp`).
///
/// ⚠️ `hash * 33 + c`, with a **plus**. The other djb2 helpers in `hashfuncs.h`
/// use `^ c`; copying one of those would agree on nothing.
pub fn string_hash(s: &str) -> u32 {
    let mut h: u32 = 5381;
    for c in s.chars() {
        h = (h << 5).wrapping_add(h).wrapping_add(c as u32);
    }
    h
}

/// Godot's `Variant::hash()`.
///
/// Returns the `u32` the C++ returns; callers that expose it to scripts widen it
/// to `i64`, as Godot's `hash()` builtin does.
pub fn godot_hash(v: &Variant) -> u32 {
    recursive_hash(v, 0)
}

/// Godot caps recursion at `MAX_RECURSION` (`array.cpp:196`) and returns 0
/// beyond it rather than overflowing the stack on a self-referencing container.
const MAX_RECURSION: u32 = 100;

fn recursive_hash(v: &Variant, depth: u32) -> u32 {
    if depth > MAX_RECURSION {
        return 0;
    }
    match v {
        Variant::Nil => 0,
        Variant::Bool(b) => u32::from(*b),
        // ⚠️ Reinterpreted as u64, not sign-extended-then-truncated: Godot casts
        // `(uint64_t)_data._int`, so -1 hashes as 0xFFFF_FFFF_FFFF_FFFF.
        Variant::Int(i) => hash_one_uint64(*i as u64),
        Variant::Float(f) => hash_murmur3_one_double(*f, HASH_MURMUR3_SEED),
        Variant::String(s) => string_hash(s),
        Variant::StringName(s) => string_hash(s.as_str()),
        Variant::Vector2(p) => {
            let h = hash_murmur3_one_float(p.x, HASH_MURMUR3_SEED);
            hash_fmix32(hash_murmur3_one_float(p.y, h))
        }
        Variant::Vector3(p) => {
            let h = hash_murmur3_one_float(p.x, HASH_MURMUR3_SEED);
            let h = hash_murmur3_one_float(p.y, h);
            hash_fmix32(hash_murmur3_one_float(p.z, h))
        }
        Variant::Color(c) => {
            let h = hash_murmur3_one_float(c.r, HASH_MURMUR3_SEED);
            let h = hash_murmur3_one_float(c.g, h);
            let h = hash_murmur3_one_float(c.b, h);
            hash_fmix32(hash_murmur3_one_float(c.a, h))
        }
        Variant::Array(items) => {
            // Seeded with the ARRAY type ordinal, so [] and {} differ.
            let mut h = hash_murmur3_one_32(VARIANT_TYPE_ARRAY, HASH_MURMUR3_SEED);
            for item in items {
                h = hash_murmur3_one_32(recursive_hash(item, depth + 1), h);
            }
            hash_fmix32(h)
        }
        Variant::Dictionary(d) => {
            // ⚠️ Seeded with the DICTIONARY ordinal, not 0 — `dictionary.cpp:379`
            // mirrors the array path. Seeding with 0 makes every dictionary hash
            // disagree with Godot while still looking self-consistent, which is
            // the failure mode this whole bead exists to remove.
            //
            // Godot folds each key's hash then its value's over the map's own
            // iteration order, so this is deliberately order-DEPENDENT: two
            // dictionaries with the same pairs inserted in different orders hash
            // differently in Godot too.
            let mut h = hash_murmur3_one_32(VARIANT_TYPE_DICTIONARY, HASH_MURMUR3_SEED);
            for (k, val) in d.iter() {
                h = hash_murmur3_one_32(recursive_hash(k, depth + 1), h);
                h = hash_murmur3_one_32(recursive_hash(val, depth + 1), h);
            }
            hash_fmix32(h)
        }
        // The remaining math types, all "murmur3 each component from the seed,
        // then fmix32" — the shape `Vector2::hash()` establishes.
        Variant::Vector2i(p) => {
            // ⚠️ `uint32_t(x)` in C++ is a REINTERPRET, so -1 feeds 0xFFFFFFFF.
            let h = hash_murmur3_one_32(p.x as u32, HASH_MURMUR3_SEED);
            hash_fmix32(hash_murmur3_one_32(p.y as u32, h))
        }
        Variant::Vector3i(p) => {
            let h = hash_murmur3_one_32(p.x as u32, HASH_MURMUR3_SEED);
            let h = hash_murmur3_one_32(p.y as u32, h);
            hash_fmix32(hash_murmur3_one_32(p.z as u32, h))
        }
        Variant::Rect2(r) => {
            let h = hash_murmur3_one_float(r.position.x, HASH_MURMUR3_SEED);
            let h = hash_murmur3_one_float(r.position.y, h);
            let h = hash_murmur3_one_float(r.size.x, h);
            hash_fmix32(hash_murmur3_one_float(r.size.y, h))
        }
        Variant::Aabb(a) => {
            let h = hash_murmur3_one_float(a.position.x, HASH_MURMUR3_SEED);
            let h = hash_murmur3_one_float(a.position.y, h);
            let h = hash_murmur3_one_float(a.position.z, h);
            let h = hash_murmur3_one_float(a.size.x, h);
            let h = hash_murmur3_one_float(a.size.y, h);
            hash_fmix32(hash_murmur3_one_float(a.size.z, h))
        }
        Variant::Plane(pl) => {
            // Normal first, then `d` — the order in `variant.cpp`'s PLANE arm.
            let h = hash_murmur3_one_float(pl.normal.x, HASH_MURMUR3_SEED);
            let h = hash_murmur3_one_float(pl.normal.y, h);
            let h = hash_murmur3_one_float(pl.normal.z, h);
            hash_fmix32(hash_murmur3_one_float(pl.d, h))
        }
        Variant::Quaternion(q) => {
            let h = hash_murmur3_one_float(q.x, HASH_MURMUR3_SEED);
            let h = hash_murmur3_one_float(q.y, h);
            let h = hash_murmur3_one_float(q.z, h);
            hash_fmix32(hash_murmur3_one_float(q.w, h))
        }
        // -- packed arrays -------------------------------------------------
        //
        // ⚠️ Godot splits these two ways and the EMPTY cases differ per kind,
        // which is the part a reimplementation gets wrong. For the byte/int
        // arrays the whole buffer goes through murmur3 at once; for the rest
        // each element is folded. And `hash_fmix32` sits *inside* the non-empty
        // branch for the folded kinds, so an empty one returns the raw seed —
        // unmixed. Each empty case below is the literal expression from
        // `variant.cpp`.
        Variant::PackedByteArray(a) => {
            if a.is_empty() {
                hash_murmur3_one_64(0, HASH_MURMUR3_SEED)
            } else {
                hash_murmur3_buffer(a, HASH_MURMUR3_SEED)
            }
        }
        Variant::PackedInt32Array(a) => {
            if a.is_empty() {
                hash_murmur3_one_64(0, HASH_MURMUR3_SEED)
            } else {
                let bytes: Vec<u8> = a.iter().flat_map(|v| v.to_le_bytes()).collect();
                hash_murmur3_buffer(&bytes, HASH_MURMUR3_SEED)
            }
        }
        Variant::PackedInt64Array(a) => {
            if a.is_empty() {
                hash_murmur3_one_64(0, HASH_MURMUR3_SEED)
            } else {
                let bytes: Vec<u8> = a.iter().flat_map(|v| v.to_le_bytes()).collect();
                hash_murmur3_buffer(&bytes, HASH_MURMUR3_SEED)
            }
        }
        Variant::PackedFloat32Array(a) => {
            if a.is_empty() {
                hash_murmur3_one_float(0.0, HASH_MURMUR3_SEED)
            } else {
                let mut h = HASH_MURMUR3_SEED;
                for f in a {
                    h = hash_murmur3_one_float(*f, h);
                }
                hash_fmix32(h)
            }
        }
        Variant::PackedFloat64Array(a) => {
            if a.is_empty() {
                hash_murmur3_one_double(0.0, HASH_MURMUR3_SEED)
            } else {
                let mut h = HASH_MURMUR3_SEED;
                for f in a {
                    h = hash_murmur3_one_double(*f, h);
                }
                hash_fmix32(h)
            }
        }
        Variant::PackedStringArray(a) => {
            let mut h = HASH_MURMUR3_SEED;
            if a.is_empty() {
                return h;
            }
            for s in a {
                h = hash_murmur3_one_32(string_hash(s), h);
            }
            hash_fmix32(h)
        }
        Variant::PackedVector2Array(a) => {
            let mut h = HASH_MURMUR3_SEED;
            if a.is_empty() {
                return h;
            }
            for p in a {
                h = hash_murmur3_one_float(p.x, h);
                h = hash_murmur3_one_float(p.y, h);
            }
            hash_fmix32(h)
        }
        Variant::PackedVector3Array(a) => {
            let mut h = HASH_MURMUR3_SEED;
            if a.is_empty() {
                return h;
            }
            for p in a {
                h = hash_murmur3_one_float(p.x, h);
                h = hash_murmur3_one_float(p.y, h);
                h = hash_murmur3_one_float(p.z, h);
            }
            hash_fmix32(h)
        }
        Variant::PackedColorArray(a) => {
            let mut h = HASH_MURMUR3_SEED;
            if a.is_empty() {
                return h;
            }
            for c in a {
                h = hash_murmur3_one_float(c.r, h);
                h = hash_murmur3_one_float(c.g, h);
                h = hash_murmur3_one_float(c.b, h);
                h = hash_murmur3_one_float(c.a, h);
            }
            hash_fmix32(h)
        }
        // -- pat-s3rrv.223: the fourteen kinds that used to fall through --
        //
        // The `_ => 0` catch-all that stood here is deliberately GONE. A new
        // Variant kind is now a non-exhaustive-match compile error instead of a
        // silent zero, which is what let these fourteen sit unhashed: every one
        // of them collapsed a Dictionary keyed by that type into a single bucket.
        Variant::NodePath(np) => np.godot_hash(),
        Variant::Rect2i(r) => {
            let h = hash_murmur3_one_32(r.position.x as u32, HASH_MURMUR3_SEED);
            let h = hash_murmur3_one_32(r.position.y as u32, h);
            let h = hash_murmur3_one_32(r.size.x as u32, h);
            hash_fmix32(hash_murmur3_one_32(r.size.y as u32, h))
        }
        Variant::Vector4(v) => {
            let h = hash_murmur3_one_float(v.x, HASH_MURMUR3_SEED);
            let h = hash_murmur3_one_float(v.y, h);
            let h = hash_murmur3_one_float(v.z, h);
            hash_fmix32(hash_murmur3_one_float(v.w, h))
        }
        Variant::Vector4i(v) => {
            let h = hash_murmur3_one_32(v.x as u32, HASH_MURMUR3_SEED);
            let h = hash_murmur3_one_32(v.y as u32, h);
            let h = hash_murmur3_one_32(v.z as u32, h);
            hash_fmix32(hash_murmur3_one_32(v.w as u32, h))
        }
        // `Transform2D::operator[]` returns COLUMNS
        // (`core/math/transform_2d.h`), and Patina stores columns too, so this
        // one maps across directly: t[0], t[1], t[2] = x, y, origin.
        Variant::Transform2D(t) => {
            let h = hash_murmur3_one_float(t.x.x, HASH_MURMUR3_SEED);
            let h = hash_murmur3_one_float(t.x.y, h);
            let h = hash_murmur3_one_float(t.y.x, h);
            let h = hash_murmur3_one_float(t.y.y, h);
            let h = hash_murmur3_one_float(t.origin.x, h);
            hash_fmix32(hash_murmur3_one_float(t.origin.y, h))
        }
        // ⚠️ **Transposed on purpose.** `Basis::operator[](int p_row)` returns a
        // ROW (`core/math/basis.h:47`), while Patina's `Basis { x, y, z }` holds
        // COLUMNS. Godot hashes `b[0].x, b[0].y, b[0].z` — row 0 — which is
        // `(x.x, y.x, z.x)` here. Feeding our columns straight through would
        // hash the transpose and disagree with Godot on every non-symmetric
        // matrix, while still passing any test built on the identity.
        Variant::Basis(b) => hash_fmix32(basis_rows_hash(b, HASH_MURMUR3_SEED)),
        Variant::Transform3D(t) => {
            let h = basis_rows_hash(&t.basis, HASH_MURMUR3_SEED);
            let h = hash_murmur3_one_float(t.origin.x, h);
            let h = hash_murmur3_one_float(t.origin.y, h);
            hash_fmix32(hash_murmur3_one_float(t.origin.z, h))
        }
        // ⓘ `Projection` is the exception: variant.cpp hashes `t.columns[i]`
        // explicitly, so there is no transpose here.
        Variant::Projection(p) => {
            let mut h = HASH_MURMUR3_SEED;
            for col in 0..4 {
                for row in 0..4 {
                    h = hash_murmur3_one_float(p.m[col][row], h);
                }
            }
            hash_fmix32(h)
        }
        Variant::Rid(r) => hash_one_uint64(r.id()),
        // Godot hashes an OBJECT by its pointer; the handle is our equivalent
        // stable identity.
        Variant::ObjectId(o) => hash_one_uint64(o.raw()),
        // ⓘ Godot has no Resource Variant kind — a resource travels as OBJECT
        // and hashes by pointer. A pointer is not reproducible here, so the
        // resource PATH is used: it is what identifies the same resource across
        // loads, which is the property a hash needs.
        Variant::Resource(r) => string_hash(&r.path),
        // `Callable::hash()` (`core/variant/callable.cpp`): the method name's
        // hash, then the object id, then fmix.
        Variant::Callable(c) => match c.as_ref() {
            CallableRef::Method { target_id, method } => {
                hash_fmix32(hash_murmur3_one_64(*target_id, string_hash(method)))
            }
            // Godot routes a lambda through `custom->hash()`, which has no
            // cross-engine definition. Hashing the parameter list keeps distinct
            // lambdas in distinct buckets without inventing a parity claim.
            CallableRef::Lambda { params, .. } => {
                let mut h = HASH_MURMUR3_SEED;
                for p in params {
                    h = hash_murmur3_one_32(string_hash(p), h);
                }
                hash_fmix32(h)
            }
            other => hash_fmix32(string_hash(&format!("{other:?}"))),
        },
        // `SIGNAL` in variant.cpp: the name's hash seeds the object id.
        Variant::Signal { object_id, name } => hash_murmur3_one_64(*object_id, string_hash(name)),
        Variant::PackedVector4Array(a) => {
            let mut h = HASH_MURMUR3_SEED;
            if a.is_empty() {
                return h;
            }
            for p in a {
                h = hash_murmur3_one_float(p.x, h);
                h = hash_murmur3_one_float(p.y, h);
                h = hash_murmur3_one_float(p.z, h);
                h = hash_murmur3_one_float(p.w, h);
            }
            hash_fmix32(h)
        }
    }
}

/// Hashes a `Basis` in Godot's ROW order, returning the running hash.
///
/// Shared by the `Basis` and `Transform3D` arms because `variant.cpp` hashes
/// the identical nine values in both. See the transpose note at the `Basis` arm.
fn basis_rows_hash(b: &gdcore::math3d::Basis, seed: u32) -> u32 {
    let mut h = seed;
    for row in [
        [b.x.x, b.y.x, b.z.x],
        [b.x.y, b.y.y, b.z.y],
        [b.x.z, b.y.z, b.z.z],
    ] {
        for v in row {
            h = hash_murmur3_one_float(v, h);
        }
    }
    h
}

#[cfg(test)]
mod tests {
    use super::*;
    use gdcore::math::{Color, Vector2, Vector3};

    // ⚠️ These constants are what Godot's own algorithms produce, computed by
    // running the C++ read out of `upstream/godot` step by step. They are the
    // point of the bead: a hash that is self-consistent but disagrees with
    // Godot is worthless for interop.
    #[test]
    fn golden_scalars_match_godot() {
        assert_eq!(godot_hash(&Variant::Nil), 0);
        assert_eq!(godot_hash(&Variant::Bool(true)), 1);
        assert_eq!(godot_hash(&Variant::Bool(false)), 0);
        assert_eq!(godot_hash(&Variant::Int(42)), 2_136_435_707);
        assert_eq!(godot_hash(&Variant::Float(1.5)), 3_990_958_085);
        assert_eq!(godot_hash(&Variant::String("hello".into())), 261_238_937);
    }

    #[test]
    fn golden_math_types_match_godot() {
        assert_eq!(
            godot_hash(&Variant::Vector2(Vector2::new(1.0, 2.0))),
            4_195_279_070
        );
    }

    /// The math types added after the first pass, same derivation.
    #[test]
    fn golden_extra_math_types_match_godot() {
        use gdcore::math::{Rect2, Vector2i};
        use gdcore::math3d::Quaternion;

        // ⚠️ -1 is REINTERPRETED as 0xFFFFFFFF, matching C++'s `uint32_t(x)`.
        assert_eq!(
            godot_hash(&Variant::Vector2i(Vector2i::new(3, -1))),
            2_297_723_828
        );
        assert_eq!(
            godot_hash(&Variant::Rect2(Rect2::new(
                Vector2::new(1.0, 2.0),
                Vector2::new(3.0, 4.0)
            ))),
            3_873_189_715
        );
        assert_eq!(
            godot_hash(&Variant::Quaternion(Quaternion::new(0.0, 0.0, 0.0, 1.0))),
            3_827_421_425
        );
    }

    /// Packed arrays, including the tail path of `hash_murmur3_buffer` (a
    /// 3-byte input is not a whole `u32` block) and the per-kind empty cases.
    #[test]
    fn golden_packed_arrays_match_godot() {
        assert_eq!(
            godot_hash(&Variant::PackedByteArray(vec![1, 2, 3])),
            3_028_899_408,
            "3 bytes exercises the 1-3 byte tail fold, not just whole blocks"
        );
        assert_eq!(
            godot_hash(&Variant::PackedInt32Array(vec![1, 2])),
            790_755_381
        );
        assert_eq!(
            godot_hash(&Variant::PackedFloat32Array(vec![1.0, 2.0])),
            4_195_279_070
        );

        // ⚠️ The empty cases genuinely differ per kind, which is the detail a
        // reimplementation flattens. Byte/int arrays go through
        // `hash_murmur3_one_64(0)`; the folded kinds return the RAW seed,
        // because Godot's `hash_fmix32` sits inside the non-empty branch.
        assert_eq!(
            godot_hash(&Variant::PackedByteArray(vec![])),
            546_098_158,
            "empty byte array is hash_murmur3_one_64(0)"
        );
        assert_eq!(
            godot_hash(&Variant::PackedStringArray(vec![])),
            HASH_MURMUR3_SEED,
            "an empty folded array returns the seed UNMIXED"
        );
        assert_ne!(
            godot_hash(&Variant::PackedByteArray(vec![])),
            godot_hash(&Variant::PackedStringArray(vec![])),
            "…so the two empty forms must not collide"
        );
    }

    /// Not a copy-paste slip: `PackedFloat32Array([1,2])` and `Vector2(1,2)`
    /// really do collide in Godot — both fold the same two floats from the same
    /// seed and finish with `hash_fmix32`. Recorded so nobody "fixes" it.
    #[test]
    fn float_array_and_vector2_collide_as_godot_does() {
        use gdcore::math::Vector2 as V2;
        assert_eq!(
            godot_hash(&Variant::PackedFloat32Array(vec![1.0, 2.0])),
            godot_hash(&Variant::Vector2(V2::new(1.0, 2.0)))
        );
    }

    #[test]
    fn golden_dictionary_matches_godot() {
        let mut d = crate::dictionary::Dictionary::new();
        d.insert(Variant::String("a".into()), Variant::Int(1));
        assert_eq!(godot_hash(&Variant::Dictionary(d)), 120_477_264);
    }

    #[test]
    fn golden_array_matches_godot() {
        let arr = Variant::Array(vec![Variant::Int(1), Variant::Int(2), Variant::Int(3)]);
        assert_eq!(godot_hash(&arr), 3_860_078_832);
    }

    /// The whole reason the bead exists: the global `hash()` must agree with
    /// `String.hash()`, which was already djb2-correct.
    #[test]
    fn string_variant_hash_equals_string_hash() {
        for s in ["", "hello", "a longer string with spaces", "ünïcødé"] {
            assert_eq!(
                godot_hash(&Variant::String(s.into())),
                string_hash(s),
                "hash(\"{s}\") must equal \"{s}\".hash()"
            );
        }
    }

    /// `+0.0` and `-0.0` compare equal, so they must hash equal or a dictionary
    /// keyed on a float silently grows two entries for one key.
    #[test]
    fn signed_zero_and_nan_normalise() {
        assert_eq!(
            godot_hash(&Variant::Float(0.0)),
            godot_hash(&Variant::Float(-0.0))
        );
        assert_eq!(
            godot_hash(&Variant::Vector2(Vector2::new(0.0, 0.0))),
            godot_hash(&Variant::Vector2(Vector2::new(-0.0, -0.0)))
        );
        assert_eq!(
            godot_hash(&Variant::Float(f64::NAN)),
            godot_hash(&Variant::Float(-f64::NAN)),
            "every NaN must hash alike — Godot normalises them explicitly"
        );
    }

    /// A negative int is reinterpreted, not sign-truncated.
    #[test]
    fn negative_int_reinterprets_as_u64() {
        assert_eq!(godot_hash(&Variant::Int(-1)), hash_one_uint64(u64::MAX));
    }

    /// Distinct shapes must not collide trivially — the array seed is what
    /// separates an empty array from other empty things.
    #[test]
    fn distinct_values_hash_distinctly() {
        let empty_arr = godot_hash(&Variant::Array(vec![]));
        assert_ne!(empty_arr, 0, "an empty array is seeded, not zero");
        assert_ne!(godot_hash(&Variant::Int(1)), godot_hash(&Variant::Int(2)));
        assert_ne!(
            godot_hash(&Variant::Vector3(Vector3::new(1.0, 2.0, 3.0))),
            godot_hash(&Variant::Vector3(Vector3::new(3.0, 2.0, 1.0))),
            "component order must matter"
        );
        assert_ne!(
            godot_hash(&Variant::Color(Color::new(1.0, 0.0, 0.0, 1.0))),
            godot_hash(&Variant::Color(Color::new(0.0, 1.0, 0.0, 1.0)))
        );
    }

    /// Nested arrays recurse rather than hashing by identity.
    #[test]
    fn nested_arrays_recurse() {
        let inner = Variant::Array(vec![Variant::Int(1)]);
        let outer = Variant::Array(vec![inner.clone()]);
        let flat = Variant::Array(vec![Variant::Int(1)]);
        assert_ne!(
            godot_hash(&outer),
            godot_hash(&flat),
            "[[1]] and [1] must differ"
        );
        assert_eq!(
            godot_hash(&outer),
            godot_hash(&Variant::Array(vec![inner])),
            "and the same shape must hash the same"
        );
    }
}
