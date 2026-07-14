//! Core Variant enum and value container types.
//!
//! Godot's Variant is a tagged union that can hold any engine value.
//! This implementation covers the subset needed for Phase 3 (headless
//! runtime): scalars, strings, math types, collections, and object refs.

use gdcore::id::ObjectId;
use gdcore::math::{Color, Rect2, Transform2D, Vector2, Vector3};
use gdcore::math3d::{Aabb, Basis, Plane, Quaternion, Transform3D};
use gdcore::node_path::NodePath;
use gdcore::string_name::StringName;
use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

/// A callable reference — either a named method or a closure/lambda.
///
/// In Godot, `Callable` wraps either an object+method pair or a lambda.
/// We model both, plus a "native" closure variant for Rust callbacks.
#[derive(Clone)]
pub enum CallableRef {
    /// A reference to a method on an object: `Callable(obj, "method")`.
    Method {
        /// Target object id (0 for self).
        target_id: u64,
        /// Method name.
        method: String,
    },
    /// A lambda / anonymous function with captured body.
    Lambda {
        /// Parameter names.
        params: Vec<String>,
        /// Serialised body (stored as opaque bytes; the interpreter casts back).
        body: Arc<dyn std::any::Any + Send + Sync>,
    },
    /// A callable with extra arguments bound (appended after call-site args).
    Bound {
        /// The inner callable to delegate to.
        inner: Box<CallableRef>,
        /// Arguments to append after call-site arguments.
        bound_args: Vec<Variant>,
    },
    /// A callable that drops trailing arguments from the call-site.
    Unbound {
        /// The inner callable to delegate to.
        inner: Box<CallableRef>,
        /// Number of trailing call-site arguments to drop.
        unbind_count: usize,
    },
}

impl CallableRef {
    /// Resolve the final argument list for this callable given call-site args.
    ///
    /// - `Bound`: appends `bound_args` after `call_args`
    /// - `Unbound`: drops `unbind_count` trailing args from `call_args`
    /// - `Method` / `Lambda`: passes args through unchanged
    pub fn resolve_args(&self, call_args: &[Variant]) -> Vec<Variant> {
        match self {
            CallableRef::Bound { inner, bound_args } => {
                let mut resolved = inner.resolve_args(call_args);
                resolved.extend(bound_args.iter().cloned());
                resolved
            }
            CallableRef::Unbound {
                inner,
                unbind_count,
            } => {
                let keep = call_args.len().saturating_sub(*unbind_count);
                inner.resolve_args(&call_args[..keep])
            }
            _ => call_args.to_vec(),
        }
    }

    /// Unwrap all `Bound`/`Unbound` layers to find the innermost callable.
    pub fn inner_callable(&self) -> &CallableRef {
        match self {
            CallableRef::Bound { inner, .. } | CallableRef::Unbound { inner, .. } => {
                inner.inner_callable()
            }
            _ => self,
        }
    }
}

impl fmt::Debug for CallableRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CallableRef::Method { target_id, method } => {
                write!(f, "Callable(Object#{target_id}, \"{method}\")")
            }
            CallableRef::Lambda { params, .. } => {
                write!(f, "Callable(<lambda({})>)", params.join(", "))
            }
            CallableRef::Bound { inner, bound_args } => {
                write!(f, "Callable({inner:?}).bind({bound_args:?})")
            }
            CallableRef::Unbound {
                inner,
                unbind_count,
            } => {
                write!(f, "Callable({inner:?}).unbind({unbind_count})")
            }
        }
    }
}

impl PartialEq for CallableRef {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (
                CallableRef::Method {
                    target_id: a,
                    method: am,
                },
                CallableRef::Method {
                    target_id: b,
                    method: bm,
                },
            ) => a == b && am == bm,
            (
                CallableRef::Bound {
                    inner: a,
                    bound_args: aa,
                },
                CallableRef::Bound {
                    inner: b,
                    bound_args: ba,
                },
            ) => a == b && aa == ba,
            (
                CallableRef::Unbound {
                    inner: a,
                    unbind_count: ac,
                },
                CallableRef::Unbound {
                    inner: b,
                    unbind_count: bc,
                },
            ) => a == b && ac == bc,
            _ => false, // lambdas are never equal; mixed types are never equal
        }
    }
}

/// Metadata about a loaded resource stored in a Variant.
#[derive(Debug, Clone, PartialEq)]
pub struct ResourceRef {
    /// The `res://` path of the resource.
    pub path: String,
    /// The Godot class name (e.g. `"Texture2D"`, `"PackedScene"`).
    pub class_name: String,
    /// Properties loaded from the resource file.
    pub properties: HashMap<String, Variant>,
}

/// The set of type tags a Variant can carry.
///
/// Mirrors `Variant::Type` from upstream Godot. We start with the
/// types needed for scene/resource work and expand as needed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VariantType {
    Nil,
    Bool,
    Int,
    Float,
    String,
    StringName,
    NodePath,
    Vector2,
    Vector3,
    Rect2,
    Transform2D,
    Color,
    Basis,
    Transform3D,
    Quaternion,
    Aabb,
    Plane,
    ObjectId,
    Array,
    Dictionary,
    Callable,
    Resource,
    PackedByteArray,
    PackedInt32Array,
    PackedInt64Array,
    PackedFloat32Array,
    PackedFloat64Array,
    PackedStringArray,
    PackedVector2Array,
    PackedVector3Array,
    PackedColorArray,
}

/// A dynamically-typed engine value, analogous to Godot's `Variant`.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum Variant {
    /// The null / default value.
    #[default]
    Nil,
    /// Boolean.
    Bool(bool),
    /// 64-bit signed integer (Godot uses i64 internally).
    Int(i64),
    /// 64-bit float (Godot uses f64 for Variant floats).
    Float(f64),
    /// A UTF-8 string.
    String(String),
    /// An interned string name.
    StringName(StringName),
    /// A scene-tree node path.
    NodePath(NodePath),
    /// A 2D vector.
    Vector2(Vector2),
    /// A 3D vector.
    Vector3(Vector3),
    /// An axis-aligned 2D rectangle.
    Rect2(Rect2),
    /// A 2D affine transform.
    Transform2D(Transform2D),
    /// An RGBA color.
    Color(Color),
    /// A 3×3 rotation/scale matrix.
    Basis(Basis),
    /// A 3D affine transform.
    Transform3D(Transform3D),
    /// A quaternion rotation.
    Quaternion(Quaternion),
    /// An axis-aligned bounding box.
    Aabb(Aabb),
    /// An infinite plane.
    Plane(Plane),
    /// A reference to an engine object by ID.
    ObjectId(ObjectId),
    /// A heterogeneous ordered list.
    Array(Vec<Variant>),
    /// A string-keyed map of variants.
    Dictionary(HashMap<String, Variant>),
    /// A callable reference (method or lambda).
    Callable(Box<CallableRef>),
    /// A loaded resource.
    Resource(Box<ResourceRef>),
    /// A tightly-packed array of bytes (`PackedByteArray`).
    PackedByteArray(Vec<u8>),
    /// A tightly-packed array of 32-bit ints (`PackedInt32Array`).
    PackedInt32Array(Vec<i32>),
    /// A tightly-packed array of 64-bit ints (`PackedInt64Array`).
    PackedInt64Array(Vec<i64>),
    /// A tightly-packed array of 32-bit floats (`PackedFloat32Array`).
    PackedFloat32Array(Vec<f32>),
    /// A tightly-packed array of 64-bit floats (`PackedFloat64Array`).
    PackedFloat64Array(Vec<f64>),
    /// A tightly-packed array of strings (`PackedStringArray`).
    PackedStringArray(Vec<String>),
    /// A tightly-packed array of 2D vectors (`PackedVector2Array`).
    PackedVector2Array(Vec<Vector2>),
    /// A tightly-packed array of 3D vectors (`PackedVector3Array`).
    PackedVector3Array(Vec<Vector3>),
    /// A tightly-packed array of colors (`PackedColorArray`).
    PackedColorArray(Vec<Color>),
}

impl Variant {
    /// Returns the type tag for this value.
    pub fn variant_type(&self) -> VariantType {
        match self {
            Self::Nil => VariantType::Nil,
            Self::Bool(_) => VariantType::Bool,
            Self::Int(_) => VariantType::Int,
            Self::Float(_) => VariantType::Float,
            Self::String(_) => VariantType::String,
            Self::StringName(_) => VariantType::StringName,
            Self::NodePath(_) => VariantType::NodePath,
            Self::Vector2(_) => VariantType::Vector2,
            Self::Vector3(_) => VariantType::Vector3,
            Self::Rect2(_) => VariantType::Rect2,
            Self::Transform2D(_) => VariantType::Transform2D,
            Self::Color(_) => VariantType::Color,
            Self::Basis(_) => VariantType::Basis,
            Self::Transform3D(_) => VariantType::Transform3D,
            Self::Quaternion(_) => VariantType::Quaternion,
            Self::Aabb(_) => VariantType::Aabb,
            Self::Plane(_) => VariantType::Plane,
            Self::ObjectId(_) => VariantType::ObjectId,
            Self::Array(_) => VariantType::Array,
            Self::Dictionary(_) => VariantType::Dictionary,
            Self::Callable(_) => VariantType::Callable,
            Self::Resource(_) => VariantType::Resource,
            Self::PackedByteArray(_) => VariantType::PackedByteArray,
            Self::PackedInt32Array(_) => VariantType::PackedInt32Array,
            Self::PackedInt64Array(_) => VariantType::PackedInt64Array,
            Self::PackedFloat32Array(_) => VariantType::PackedFloat32Array,
            Self::PackedFloat64Array(_) => VariantType::PackedFloat64Array,
            Self::PackedStringArray(_) => VariantType::PackedStringArray,
            Self::PackedVector2Array(_) => VariantType::PackedVector2Array,
            Self::PackedVector3Array(_) => VariantType::PackedVector3Array,
            Self::PackedColorArray(_) => VariantType::PackedColorArray,
        }
    }

    /// Returns `true` if this variant is `Nil`.
    pub fn is_nil(&self) -> bool {
        matches!(self, Self::Nil)
    }

    /// Returns the Godot-style "truthiness" of this value.
    ///
    /// Godot rules: Nil/false/0/0.0/empty-string/empty-array/empty-dict → false.
    pub fn is_truthy(&self) -> bool {
        match self {
            Self::Nil => false,
            Self::Bool(b) => *b,
            Self::Int(i) => *i != 0,
            Self::Float(f) => *f != 0.0,
            Self::String(s) => !s.is_empty(),
            Self::StringName(sn) => !sn.as_str().is_empty(),
            Self::NodePath(np) => !np.is_empty(),
            Self::Array(a) => !a.is_empty(),
            Self::Dictionary(d) => !d.is_empty(),
            Self::Callable(_) => true,
            Self::Resource(_) => true,
            // Packed arrays follow Array truthiness: empty is falsy.
            Self::PackedByteArray(v) => !v.is_empty(),
            Self::PackedInt32Array(v) => !v.is_empty(),
            Self::PackedInt64Array(v) => !v.is_empty(),
            Self::PackedFloat32Array(v) => !v.is_empty(),
            Self::PackedFloat64Array(v) => !v.is_empty(),
            Self::PackedStringArray(v) => !v.is_empty(),
            Self::PackedVector2Array(v) => !v.is_empty(),
            Self::PackedVector3Array(v) => !v.is_empty(),
            Self::PackedColorArray(v) => !v.is_empty(),
            _ => true,
        }
    }

    /// Coerce to `i64` following Godot's rules.
    ///
    /// - `Int(i)` → `i`
    /// - `Float(f)` → `f as i64` (truncate)
    /// - `Bool(b)` → `0` or `1`
    /// - `String(s)` → parse or `0`
    /// - everything else → `0`
    pub fn to_int(&self) -> i64 {
        match self {
            Self::Int(i) => *i,
            Self::Float(f) => {
                if f.is_nan() {
                    0
                } else {
                    *f as i64
                }
            }
            Self::Bool(b) => {
                if *b {
                    1
                } else {
                    0
                }
            }
            Self::String(s) => s.trim().parse::<i64>().unwrap_or(0),
            _ => 0,
        }
    }

    /// Coerce to `f64` following Godot's rules.
    ///
    /// - `Float(f)` → `f`
    /// - `Int(i)` → `i as f64`
    /// - `Bool(b)` → `0.0` or `1.0`
    /// - `String(s)` → parse or `0.0`
    /// - everything else → `0.0`
    pub fn to_float(&self) -> f64 {
        match self {
            Self::Float(f) => *f,
            Self::Int(i) => *i as f64,
            Self::Bool(b) => {
                if *b {
                    1.0
                } else {
                    0.0
                }
            }
            Self::String(s) => s.trim().parse::<f64>().unwrap_or(0.0),
            _ => 0.0,
        }
    }

    /// Coerce to `bool` following Godot's truthiness rules.
    pub fn to_bool(&self) -> bool {
        self.is_truthy()
    }

    /// Coerce to `String` (lossy — always succeeds, never panics).
    pub fn to_string_lossy(&self) -> String {
        self.to_string()
    }

    // -----------------------------------------------------------------------
    // Packed array helpers
    //
    // A unified element-oriented API over the nine `Packed*Array` variants.
    // The GDScript method surface (`size`, `is_empty`, `append`, `resize`,
    // `slice`, `to_byte_array`, indexing) is built on top of these so the
    // interpreter bindings stay thin and every rule is unit-tested here.
    // -----------------------------------------------------------------------

    /// Returns `true` if this is any `Packed*Array` variant.
    pub fn is_packed_array(&self) -> bool {
        matches!(
            self,
            Self::PackedByteArray(_)
                | Self::PackedInt32Array(_)
                | Self::PackedInt64Array(_)
                | Self::PackedFloat32Array(_)
                | Self::PackedFloat64Array(_)
                | Self::PackedStringArray(_)
                | Self::PackedVector2Array(_)
                | Self::PackedVector3Array(_)
                | Self::PackedColorArray(_)
        )
    }

    /// Element count, if this is a packed array (`None` otherwise).
    pub fn packed_len(&self) -> Option<usize> {
        Some(match self {
            Self::PackedByteArray(v) => v.len(),
            Self::PackedInt32Array(v) => v.len(),
            Self::PackedInt64Array(v) => v.len(),
            Self::PackedFloat32Array(v) => v.len(),
            Self::PackedFloat64Array(v) => v.len(),
            Self::PackedStringArray(v) => v.len(),
            Self::PackedVector2Array(v) => v.len(),
            Self::PackedVector3Array(v) => v.len(),
            Self::PackedColorArray(v) => v.len(),
            _ => return None,
        })
    }

    /// `true` if this is an empty packed array. `None` if not packed.
    pub fn packed_is_empty(&self) -> Option<bool> {
        self.packed_len().map(|n| n == 0)
    }

    /// Element at `index`, boxed as a `Variant`. `None` if not a packed array
    /// or `index` is out of range. Byte/int elements surface as `Int`, float
    /// elements as `Float`.
    pub fn packed_get(&self, index: usize) -> Option<Variant> {
        Some(match self {
            Self::PackedByteArray(v) => Variant::Int(i64::from(*v.get(index)?)),
            Self::PackedInt32Array(v) => Variant::Int(i64::from(*v.get(index)?)),
            Self::PackedInt64Array(v) => Variant::Int(*v.get(index)?),
            Self::PackedFloat32Array(v) => Variant::Float(f64::from(*v.get(index)?)),
            Self::PackedFloat64Array(v) => Variant::Float(*v.get(index)?),
            Self::PackedStringArray(v) => Variant::String(v.get(index)?.clone()),
            Self::PackedVector2Array(v) => Variant::Vector2(*v.get(index)?),
            Self::PackedVector3Array(v) => Variant::Vector3(*v.get(index)?),
            Self::PackedColorArray(v) => Variant::Color(*v.get(index)?),
            _ => return None,
        })
    }

    /// Overwrites the element at `index`, coercing `value` to the element type.
    /// Returns `false` if this is not a packed array, `index` is out of range,
    /// or `value` is the wrong type for a vector/color array.
    pub fn packed_set(&mut self, index: usize, value: &Variant) -> bool {
        match self {
            Self::PackedByteArray(v) => set_slot(v, index, value.to_int() as u8),
            Self::PackedInt32Array(v) => set_slot(v, index, value.to_int() as i32),
            Self::PackedInt64Array(v) => set_slot(v, index, value.to_int()),
            Self::PackedFloat32Array(v) => set_slot(v, index, value.to_float() as f32),
            Self::PackedFloat64Array(v) => set_slot(v, index, value.to_float()),
            Self::PackedStringArray(v) => set_slot(v, index, value.to_string_lossy()),
            Self::PackedVector2Array(v) => match value {
                Variant::Vector2(x) => set_slot(v, index, *x),
                _ => false,
            },
            Self::PackedVector3Array(v) => match value {
                Variant::Vector3(x) => set_slot(v, index, *x),
                _ => false,
            },
            Self::PackedColorArray(v) => match value {
                Variant::Color(x) => set_slot(v, index, *x),
                _ => false,
            },
            _ => false,
        }
    }

    /// Appends `value` (coerced to the element type). Returns `false` if this
    /// is not a packed array or `value` is the wrong type for a
    /// vector/color array. Mirrors `Packed*Array.append` / `push_back`.
    pub fn packed_push(&mut self, value: &Variant) -> bool {
        match self {
            Self::PackedByteArray(v) => v.push(value.to_int() as u8),
            Self::PackedInt32Array(v) => v.push(value.to_int() as i32),
            Self::PackedInt64Array(v) => v.push(value.to_int()),
            Self::PackedFloat32Array(v) => v.push(value.to_float() as f32),
            Self::PackedFloat64Array(v) => v.push(value.to_float()),
            Self::PackedStringArray(v) => v.push(value.to_string_lossy()),
            Self::PackedVector2Array(v) => match value {
                Variant::Vector2(x) => v.push(*x),
                _ => return false,
            },
            Self::PackedVector3Array(v) => match value {
                Variant::Vector3(x) => v.push(*x),
                _ => return false,
            },
            Self::PackedColorArray(v) => match value {
                Variant::Color(x) => v.push(*x),
                _ => return false,
            },
            _ => return false,
        }
        true
    }

    /// Resizes the packed array to `new_len`, truncating or filling with the
    /// element type's zero value (opaque black for colors, matching Godot).
    /// Returns `false` if this is not a packed array.
    pub fn packed_resize(&mut self, new_len: usize) -> bool {
        match self {
            Self::PackedByteArray(v) => v.resize(new_len, 0),
            Self::PackedInt32Array(v) => v.resize(new_len, 0),
            Self::PackedInt64Array(v) => v.resize(new_len, 0),
            Self::PackedFloat32Array(v) => v.resize(new_len, 0.0),
            Self::PackedFloat64Array(v) => v.resize(new_len, 0.0),
            Self::PackedStringArray(v) => v.resize(new_len, String::new()),
            Self::PackedVector2Array(v) => v.resize(new_len, Vector2::ZERO),
            Self::PackedVector3Array(v) => v.resize(new_len, Vector3::ZERO),
            Self::PackedColorArray(v) => v.resize(new_len, Color::new(0.0, 0.0, 0.0, 1.0)),
            _ => return false,
        }
        true
    }

    /// Godot `Packed*Array.slice(begin, end)` — negative indices count from the
    /// end, bounds are clamped, and the result is the *same* packed type.
    /// `None` if this is not a packed array.
    pub fn packed_slice(&self, begin: i64, end: i64) -> Option<Variant> {
        let len = self.packed_len()?;
        let (b, e) = slice_bounds(begin, end, len);
        Some(match self {
            Self::PackedByteArray(v) => Self::PackedByteArray(v[b..e].to_vec()),
            Self::PackedInt32Array(v) => Self::PackedInt32Array(v[b..e].to_vec()),
            Self::PackedInt64Array(v) => Self::PackedInt64Array(v[b..e].to_vec()),
            Self::PackedFloat32Array(v) => Self::PackedFloat32Array(v[b..e].to_vec()),
            Self::PackedFloat64Array(v) => Self::PackedFloat64Array(v[b..e].to_vec()),
            Self::PackedStringArray(v) => Self::PackedStringArray(v[b..e].to_vec()),
            Self::PackedVector2Array(v) => Self::PackedVector2Array(v[b..e].to_vec()),
            Self::PackedVector3Array(v) => Self::PackedVector3Array(v[b..e].to_vec()),
            Self::PackedColorArray(v) => Self::PackedColorArray(v[b..e].to_vec()),
            _ => return None,
        })
    }

    /// Converts a packed array to a heterogeneous [`Variant::Array`] of boxed
    /// elements. `None` if this is not a packed array. Inverse-friendly with
    /// the parser and the GDScript constructors.
    pub fn packed_to_array(&self) -> Option<Vec<Variant>> {
        let len = self.packed_len()?;
        let mut out = Vec::with_capacity(len);
        for i in 0..len {
            out.push(self.packed_get(i)?);
        }
        Some(out)
    }

    /// Godot `Packed*Array.to_byte_array()` — the raw little-endian byte image
    /// of the elements, as a [`Variant::PackedByteArray`]. `None` if this is
    /// not a byte/numeric/vector/color packed array (`PackedStringArray` has no
    /// byte image and returns `None`).
    pub fn packed_to_byte_array(&self) -> Option<Variant> {
        let bytes = match self {
            Self::PackedByteArray(v) => v.clone(),
            Self::PackedInt32Array(v) => v.iter().flat_map(|x| x.to_le_bytes()).collect(),
            Self::PackedInt64Array(v) => v.iter().flat_map(|x| x.to_le_bytes()).collect(),
            Self::PackedFloat32Array(v) => v.iter().flat_map(|x| x.to_le_bytes()).collect(),
            Self::PackedFloat64Array(v) => v.iter().flat_map(|x| x.to_le_bytes()).collect(),
            Self::PackedVector2Array(v) => v
                .iter()
                .flat_map(|p| [p.x.to_le_bytes(), p.y.to_le_bytes()])
                .flatten()
                .collect(),
            Self::PackedVector3Array(v) => v
                .iter()
                .flat_map(|p| [p.x.to_le_bytes(), p.y.to_le_bytes(), p.z.to_le_bytes()])
                .flatten()
                .collect(),
            Self::PackedColorArray(v) => v
                .iter()
                .flat_map(|c| {
                    [
                        c.r.to_le_bytes(),
                        c.g.to_le_bytes(),
                        c.b.to_le_bytes(),
                        c.a.to_le_bytes(),
                    ]
                })
                .flatten()
                .collect(),
            _ => return None,
        };
        Some(Variant::PackedByteArray(bytes))
    }
}

/// Overwrites `v[index]`, returning `false` if `index` is out of range.
fn set_slot<T>(v: &mut [T], index: usize, value: T) -> bool {
    match v.get_mut(index) {
        Some(slot) => {
            *slot = value;
            true
        }
        None => false,
    }
}

/// Normalizes Godot `slice(begin, end)` indices into a `[start, end)` byte
/// range clamped to `len`. Negative indices count from the end.
fn slice_bounds(begin: i64, end: i64, len: usize) -> (usize, usize) {
    let len_i = len as i64;
    let norm = |i: i64| -> i64 {
        let i = if i < 0 { i + len_i } else { i };
        i.clamp(0, len_i)
    };
    let b = norm(begin);
    let e = norm(end).max(b);
    (b as usize, e as usize)
}

// ---------------------------------------------------------------------------
// Arithmetic operators
// ---------------------------------------------------------------------------

impl std::ops::Add for Variant {
    type Output = Variant;

    fn add(self, rhs: Variant) -> Variant {
        match (&self, &rhs) {
            (Variant::Int(a), Variant::Int(b)) => Variant::Int(a.wrapping_add(*b)),
            (Variant::Float(a), Variant::Float(b)) => Variant::Float(a + b),
            (Variant::Int(a), Variant::Float(b)) => Variant::Float(*a as f64 + b),
            (Variant::Float(a), Variant::Int(b)) => Variant::Float(a + *b as f64),
            (Variant::String(a), Variant::String(b)) => Variant::String(format!("{a}{b}")),
            _ => Variant::Nil,
        }
    }
}

impl std::ops::Sub for Variant {
    type Output = Variant;

    fn sub(self, rhs: Variant) -> Variant {
        match (&self, &rhs) {
            (Variant::Int(a), Variant::Int(b)) => Variant::Int(a.wrapping_sub(*b)),
            (Variant::Float(a), Variant::Float(b)) => Variant::Float(a - b),
            (Variant::Int(a), Variant::Float(b)) => Variant::Float(*a as f64 - b),
            (Variant::Float(a), Variant::Int(b)) => Variant::Float(a - *b as f64),
            _ => Variant::Nil,
        }
    }
}

impl std::ops::Mul for Variant {
    type Output = Variant;

    fn mul(self, rhs: Variant) -> Variant {
        match (&self, &rhs) {
            (Variant::Int(a), Variant::Int(b)) => Variant::Int(a.wrapping_mul(*b)),
            (Variant::Float(a), Variant::Float(b)) => Variant::Float(a * b),
            (Variant::Int(a), Variant::Float(b)) => Variant::Float(*a as f64 * b),
            (Variant::Float(a), Variant::Int(b)) => Variant::Float(a * *b as f64),
            _ => Variant::Nil,
        }
    }
}

impl std::ops::Div for Variant {
    type Output = Variant;

    fn div(self, rhs: Variant) -> Variant {
        match (&self, &rhs) {
            (Variant::Int(_), Variant::Int(0)) => Variant::Nil,
            (Variant::Int(a), Variant::Int(b)) => Variant::Int(a.wrapping_div(*b)),
            (Variant::Float(_), Variant::Float(b)) if *b == 0.0 => Variant::Nil,
            (Variant::Float(a), Variant::Float(b)) => Variant::Float(a / b),
            (Variant::Int(_), Variant::Float(b)) if *b == 0.0 => Variant::Nil,
            (Variant::Int(a), Variant::Float(b)) => Variant::Float(*a as f64 / b),
            (Variant::Float(_), Variant::Int(0)) => Variant::Nil,
            (Variant::Float(a), Variant::Int(b)) => Variant::Float(a / *b as f64),
            _ => Variant::Nil,
        }
    }
}

impl std::ops::Rem for Variant {
    type Output = Variant;

    fn rem(self, rhs: Variant) -> Variant {
        match (&self, &rhs) {
            (Variant::Int(_), Variant::Int(0)) => Variant::Nil,
            (Variant::Int(a), Variant::Int(b)) => Variant::Int(a.wrapping_rem(*b)),
            (Variant::Float(_), Variant::Float(b)) if *b == 0.0 => Variant::Nil,
            (Variant::Float(a), Variant::Float(b)) => Variant::Float(a % b),
            _ => Variant::Nil,
        }
    }
}

impl std::ops::Neg for Variant {
    type Output = Variant;

    fn neg(self) -> Variant {
        match self {
            Variant::Int(i) => Variant::Int(i.wrapping_neg()),
            Variant::Float(f) => Variant::Float(-f),
            _ => Variant::Nil,
        }
    }
}

// ---------------------------------------------------------------------------
// PartialOrd — cross-type numeric comparison
// ---------------------------------------------------------------------------

impl PartialOrd for Variant {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        match (self, other) {
            (Variant::Int(a), Variant::Int(b)) => a.partial_cmp(b),
            (Variant::Float(a), Variant::Float(b)) => a.partial_cmp(b),
            (Variant::Int(a), Variant::Float(b)) => (*a as f64).partial_cmp(b),
            (Variant::Float(a), Variant::Int(b)) => a.partial_cmp(&(*b as f64)),
            (Variant::Bool(a), Variant::Bool(b)) => a.partial_cmp(b),
            (Variant::String(a), Variant::String(b)) => a.partial_cmp(b),
            _ => None,
        }
    }
}

impl fmt::Display for Variant {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Nil => write!(f, "<null>"),
            Self::Bool(b) => write!(f, "{b}"),
            Self::Int(i) => write!(f, "{i}"),
            Self::Float(v) => write!(f, "{v}"),
            Self::String(s) => write!(f, "{s}"),
            Self::StringName(sn) => write!(f, "&{sn}"),
            Self::NodePath(np) => write!(f, "NodePath(\"{np}\")"),
            Self::Vector2(v) => write!(f, "({}, {})", v.x, v.y),
            Self::Vector3(v) => write!(f, "({}, {}, {})", v.x, v.y, v.z),
            Self::Rect2(r) => write!(
                f,
                "[({}, {}), ({}, {})]",
                r.position.x, r.position.y, r.size.x, r.size.y
            ),
            Self::Transform2D(_) => write!(f, "<Transform2D>"),
            Self::Color(c) => write!(f, "Color({}, {}, {}, {})", c.r, c.g, c.b, c.a),
            Self::Basis(_) => write!(f, "<Basis>"),
            Self::Transform3D(_) => write!(f, "<Transform3D>"),
            Self::Quaternion(q) => write!(f, "Quaternion({}, {}, {}, {})", q.x, q.y, q.z, q.w),
            Self::Aabb(a) => write!(
                f,
                "AABB(({}, {}, {}), ({}, {}, {}))",
                a.position.x, a.position.y, a.position.z, a.size.x, a.size.y, a.size.z
            ),
            Self::Plane(p) => write!(
                f,
                "Plane(({}, {}, {}), {})",
                p.normal.x, p.normal.y, p.normal.z, p.d
            ),
            Self::ObjectId(id) => write!(f, "<Object#{id}>"),
            Self::Array(a) => write!(f, "[Array; len={}]", a.len()),
            Self::Dictionary(d) => write!(f, "{{Dict; len={}}}", d.len()),
            Self::Callable(c) => write!(f, "{c:?}"),
            Self::Resource(r) => write!(f, "<Resource:{} @ {}>", r.class_name, r.path),
            Self::PackedByteArray(v) => write!(f, "[PackedByteArray; len={}]", v.len()),
            Self::PackedInt32Array(v) => write!(f, "[PackedInt32Array; len={}]", v.len()),
            Self::PackedInt64Array(v) => write!(f, "[PackedInt64Array; len={}]", v.len()),
            Self::PackedFloat32Array(v) => write!(f, "[PackedFloat32Array; len={}]", v.len()),
            Self::PackedFloat64Array(v) => write!(f, "[PackedFloat64Array; len={}]", v.len()),
            Self::PackedStringArray(v) => write!(f, "[PackedStringArray; len={}]", v.len()),
            Self::PackedVector2Array(v) => write!(f, "[PackedVector2Array; len={}]", v.len()),
            Self::PackedVector3Array(v) => write!(f, "[PackedVector3Array; len={}]", v.len()),
            Self::PackedColorArray(v) => write!(f, "[PackedColorArray; len={}]", v.len()),
        }
    }
}

impl fmt::Display for VariantType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Self::Nil => "Nil",
            Self::Bool => "bool",
            Self::Int => "int",
            Self::Float => "float",
            Self::String => "String",
            Self::StringName => "StringName",
            Self::NodePath => "NodePath",
            Self::Vector2 => "Vector2",
            Self::Vector3 => "Vector3",
            Self::Rect2 => "Rect2",
            Self::Transform2D => "Transform2D",
            Self::Color => "Color",
            Self::Basis => "Basis",
            Self::Transform3D => "Transform3D",
            Self::Quaternion => "Quaternion",
            Self::Aabb => "AABB",
            Self::Plane => "Plane",
            Self::ObjectId => "ObjectId",
            Self::Array => "Array",
            Self::Dictionary => "Dictionary",
            Self::Callable => "Callable",
            Self::Resource => "Resource",
            Self::PackedByteArray => "PackedByteArray",
            Self::PackedInt32Array => "PackedInt32Array",
            Self::PackedInt64Array => "PackedInt64Array",
            Self::PackedFloat32Array => "PackedFloat32Array",
            Self::PackedFloat64Array => "PackedFloat64Array",
            Self::PackedStringArray => "PackedStringArray",
            Self::PackedVector2Array => "PackedVector2Array",
            Self::PackedVector3Array => "PackedVector3Array",
            Self::PackedColorArray => "PackedColorArray",
        };
        write!(f, "{name}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn variant_type_tag() {
        assert_eq!(Variant::Nil.variant_type(), VariantType::Nil);
        assert_eq!(Variant::Int(42).variant_type(), VariantType::Int);
        assert_eq!(
            Variant::String("hi".into()).variant_type(),
            VariantType::String
        );
    }

    #[test]
    fn truthy_falsy() {
        assert!(!Variant::Nil.is_truthy());
        assert!(!Variant::Bool(false).is_truthy());
        assert!(Variant::Bool(true).is_truthy());
        assert!(!Variant::Int(0).is_truthy());
        assert!(Variant::Int(1).is_truthy());
        assert!(!Variant::Float(0.0).is_truthy());
        assert!(Variant::Float(0.1).is_truthy());
        assert!(!Variant::String(String::new()).is_truthy());
        assert!(Variant::String("x".into()).is_truthy());
        assert!(!Variant::Array(vec![]).is_truthy());
        assert!(Variant::Array(vec![Variant::Nil]).is_truthy());
        // Math types are always truthy (even zero vectors).
        assert!(Variant::Vector2(Vector2::ZERO).is_truthy());
    }

    #[test]
    fn default_is_nil() {
        assert!(Variant::default().is_nil());
    }

    #[test]
    fn variant_string_name_type_tag() {
        let sn = StringName::new("test");
        assert_eq!(
            Variant::StringName(sn).variant_type(),
            VariantType::StringName
        );
    }

    #[test]
    fn variant_node_path_type_tag() {
        let np = NodePath::new("/root/Player");
        assert_eq!(Variant::NodePath(np).variant_type(), VariantType::NodePath);
    }

    #[test]
    fn string_name_truthy() {
        assert!(Variant::StringName(StringName::new("x")).is_truthy());
        assert!(!Variant::StringName(StringName::new("")).is_truthy());
    }

    #[test]
    fn node_path_truthy() {
        assert!(Variant::NodePath(NodePath::new("/root")).is_truthy());
        assert!(!Variant::NodePath(NodePath::new("")).is_truthy());
    }

    // -- Display for every variant type -------------------------------------

    #[test]
    fn display_nil() {
        assert_eq!(format!("{}", Variant::Nil), "<null>");
    }

    #[test]
    fn display_bool() {
        assert_eq!(format!("{}", Variant::Bool(true)), "true");
        assert_eq!(format!("{}", Variant::Bool(false)), "false");
    }

    #[test]
    fn display_int() {
        assert_eq!(format!("{}", Variant::Int(42)), "42");
        assert_eq!(format!("{}", Variant::Int(-7)), "-7");
    }

    #[test]
    fn display_float() {
        assert_eq!(format!("{}", Variant::Float(3.14)), "3.14");
    }

    #[test]
    fn display_string() {
        assert_eq!(format!("{}", Variant::String("hello".into())), "hello");
    }

    #[test]
    fn display_string_name() {
        let sn = StringName::new("test_sn");
        assert_eq!(format!("{}", Variant::StringName(sn)), "&test_sn");
    }

    #[test]
    fn display_node_path() {
        let np = NodePath::new("/root/Player");
        assert_eq!(
            format!("{}", Variant::NodePath(np)),
            "NodePath(\"/root/Player\")"
        );
    }

    #[test]
    fn display_vector2() {
        let v = Vector2::new(1.0, 2.0);
        assert_eq!(format!("{}", Variant::Vector2(v)), "(1, 2)");
    }

    #[test]
    fn display_vector3() {
        let v = gdcore::math::Vector3::new(1.0, 2.0, 3.0);
        assert_eq!(format!("{}", Variant::Vector3(v)), "(1, 2, 3)");
    }

    #[test]
    fn display_rect2() {
        let r = gdcore::math::Rect2::new(Vector2::new(1.0, 2.0), Vector2::new(3.0, 4.0));
        assert_eq!(format!("{}", Variant::Rect2(r)), "[(1, 2), (3, 4)]");
    }

    #[test]
    fn display_transform2d() {
        let t = gdcore::math::Transform2D::IDENTITY;
        assert_eq!(format!("{}", Variant::Transform2D(t)), "<Transform2D>");
    }

    #[test]
    fn display_color() {
        let c = gdcore::math::Color::new(1.0, 0.5, 0.0, 1.0);
        assert_eq!(format!("{}", Variant::Color(c)), "Color(1, 0.5, 0, 1)");
    }

    #[test]
    fn display_object_id() {
        let id = gdcore::id::ObjectId::from_raw(99);
        assert_eq!(format!("{}", Variant::ObjectId(id)), "<Object#99>");
    }

    #[test]
    fn display_array() {
        let a = Variant::Array(vec![Variant::Int(1), Variant::Int(2)]);
        assert_eq!(format!("{a}"), "[Array; len=2]");
    }

    #[test]
    fn display_dictionary() {
        let mut d = std::collections::HashMap::new();
        d.insert("key".to_string(), Variant::Int(1));
        let v = Variant::Dictionary(d);
        assert_eq!(format!("{v}"), "{Dict; len=1}");
    }

    // -- Clone correctness --------------------------------------------------

    #[test]
    fn clone_nested_array() {
        let inner = Variant::Array(vec![Variant::Int(1), Variant::Int(2)]);
        let outer = Variant::Array(vec![inner.clone(), Variant::String("x".into())]);
        let cloned = outer.clone();
        assert_eq!(outer, cloned);
    }

    #[test]
    fn clone_nested_dictionary() {
        let mut inner = std::collections::HashMap::new();
        inner.insert("a".into(), Variant::Int(1));
        let outer = Variant::Dictionary(inner);
        let cloned = outer.clone();
        assert_eq!(outer, cloned);
    }

    // -- Truthy for remaining types -----------------------------------------

    #[test]
    fn dictionary_truthy() {
        assert!(!Variant::Dictionary(std::collections::HashMap::new()).is_truthy());
        let mut d = std::collections::HashMap::new();
        d.insert("k".into(), Variant::Nil);
        assert!(Variant::Dictionary(d).is_truthy());
    }

    #[test]
    fn object_id_is_always_truthy() {
        assert!(Variant::ObjectId(gdcore::id::ObjectId::from_raw(0)).is_truthy());
    }

    #[test]
    fn color_is_always_truthy() {
        assert!(Variant::Color(gdcore::math::Color::TRANSPARENT).is_truthy());
    }

    // -- VariantType Display ------------------------------------------------

    #[test]
    fn variant_type_display_all() {
        assert_eq!(format!("{}", VariantType::Nil), "Nil");
        assert_eq!(format!("{}", VariantType::Bool), "bool");
        assert_eq!(format!("{}", VariantType::Int), "int");
        assert_eq!(format!("{}", VariantType::Float), "float");
        assert_eq!(format!("{}", VariantType::String), "String");
        assert_eq!(format!("{}", VariantType::StringName), "StringName");
        assert_eq!(format!("{}", VariantType::NodePath), "NodePath");
        assert_eq!(format!("{}", VariantType::Vector2), "Vector2");
        assert_eq!(format!("{}", VariantType::Vector3), "Vector3");
        assert_eq!(format!("{}", VariantType::Rect2), "Rect2");
        assert_eq!(format!("{}", VariantType::Transform2D), "Transform2D");
        assert_eq!(format!("{}", VariantType::Color), "Color");
        assert_eq!(format!("{}", VariantType::ObjectId), "ObjectId");
        assert_eq!(format!("{}", VariantType::Array), "Array");
        assert_eq!(format!("{}", VariantType::Dictionary), "Dictionary");
    }

    // -- Packed array variants ----------------------------------------------

    #[test]
    fn packed_type_tags() {
        assert_eq!(
            Variant::PackedByteArray(vec![1, 2, 3]).variant_type(),
            VariantType::PackedByteArray
        );
        assert_eq!(
            Variant::PackedInt32Array(vec![1]).variant_type(),
            VariantType::PackedInt32Array
        );
        assert_eq!(
            Variant::PackedInt64Array(vec![1]).variant_type(),
            VariantType::PackedInt64Array
        );
        assert_eq!(
            Variant::PackedFloat32Array(vec![1.0]).variant_type(),
            VariantType::PackedFloat32Array
        );
        assert_eq!(
            Variant::PackedFloat64Array(vec![1.0]).variant_type(),
            VariantType::PackedFloat64Array
        );
        assert_eq!(
            Variant::PackedStringArray(vec!["x".into()]).variant_type(),
            VariantType::PackedStringArray
        );
        assert_eq!(
            Variant::PackedVector2Array(vec![Vector2::ZERO]).variant_type(),
            VariantType::PackedVector2Array
        );
        assert_eq!(
            Variant::PackedVector3Array(vec![Vector3::ZERO]).variant_type(),
            VariantType::PackedVector3Array
        );
        assert_eq!(
            Variant::PackedColorArray(vec![Color::TRANSPARENT]).variant_type(),
            VariantType::PackedColorArray
        );
    }

    #[test]
    fn packed_type_display_names() {
        assert_eq!(format!("{}", VariantType::PackedByteArray), "PackedByteArray");
        assert_eq!(
            format!("{}", VariantType::PackedInt32Array),
            "PackedInt32Array"
        );
        assert_eq!(
            format!("{}", VariantType::PackedVector2Array),
            "PackedVector2Array"
        );
        assert_eq!(
            format!("{}", VariantType::PackedColorArray),
            "PackedColorArray"
        );
    }

    #[test]
    fn packed_truthiness_follows_array() {
        assert!(!Variant::PackedByteArray(vec![]).is_truthy());
        assert!(Variant::PackedByteArray(vec![0]).is_truthy());
        assert!(!Variant::PackedStringArray(vec![]).is_truthy());
        assert!(Variant::PackedVector2Array(vec![Vector2::ZERO]).is_truthy());
    }

    #[test]
    fn packed_len_and_is_empty() {
        let v = Variant::PackedInt32Array(vec![1, 2, 3]);
        assert_eq!(v.packed_len(), Some(3));
        assert_eq!(v.packed_is_empty(), Some(false));
        assert_eq!(Variant::PackedInt32Array(vec![]).packed_is_empty(), Some(true));
        // Non-packed values return None.
        assert_eq!(Variant::Int(5).packed_len(), None);
        assert!(!Variant::Int(5).is_packed_array());
        assert!(Variant::PackedByteArray(vec![]).is_packed_array());
    }

    #[test]
    fn packed_get_indexing() {
        assert_eq!(
            Variant::PackedByteArray(vec![10, 20]).packed_get(1),
            Some(Variant::Int(20))
        );
        assert_eq!(
            Variant::PackedFloat32Array(vec![1.5]).packed_get(0),
            Some(Variant::Float(1.5))
        );
        assert_eq!(
            Variant::PackedStringArray(vec!["a".into(), "b".into()]).packed_get(0),
            Some(Variant::String("a".into()))
        );
        assert_eq!(
            Variant::PackedVector2Array(vec![Vector2::new(3.0, 4.0)]).packed_get(0),
            Some(Variant::Vector2(Vector2::new(3.0, 4.0)))
        );
        // Out of range → None.
        assert_eq!(Variant::PackedByteArray(vec![1]).packed_get(5), None);
    }

    #[test]
    fn packed_push_and_set() {
        let mut v = Variant::PackedByteArray(vec![]);
        assert!(v.packed_push(&Variant::Int(65)));
        assert!(v.packed_push(&Variant::Int(300))); // wraps to low byte (44)
        assert_eq!(v, Variant::PackedByteArray(vec![65, 44]));
        assert!(v.packed_set(0, &Variant::Int(66)));
        assert_eq!(v, Variant::PackedByteArray(vec![66, 44]));
        assert!(!v.packed_set(9, &Variant::Int(1))); // out of range

        // Vector arrays reject non-vector pushes.
        let mut vv = Variant::PackedVector2Array(vec![]);
        assert!(vv.packed_push(&Variant::Vector2(Vector2::new(1.0, 2.0))));
        assert!(!vv.packed_push(&Variant::Int(3)));
        assert_eq!(vv.packed_len(), Some(1));
    }

    #[test]
    fn packed_resize_grows_and_truncates() {
        let mut v = Variant::PackedInt32Array(vec![1, 2, 3]);
        assert!(v.packed_resize(5));
        assert_eq!(v, Variant::PackedInt32Array(vec![1, 2, 3, 0, 0]));
        assert!(v.packed_resize(2));
        assert_eq!(v, Variant::PackedInt32Array(vec![1, 2]));
        // Color fill value is opaque black.
        let mut c = Variant::PackedColorArray(vec![]);
        assert!(c.packed_resize(1));
        assert_eq!(
            c,
            Variant::PackedColorArray(vec![Color::new(0.0, 0.0, 0.0, 1.0)])
        );
    }

    #[test]
    fn packed_slice_semantics() {
        let v = Variant::PackedInt64Array(vec![0, 1, 2, 3, 4]);
        assert_eq!(v.packed_slice(1, 3), Some(Variant::PackedInt64Array(vec![1, 2])));
        // Negative indices count from the end.
        assert_eq!(
            v.packed_slice(-2, 5),
            Some(Variant::PackedInt64Array(vec![3, 4]))
        );
        // Clamped / inverted ranges yield empty, keeping the packed type.
        assert_eq!(v.packed_slice(3, 1), Some(Variant::PackedInt64Array(vec![])));
    }

    #[test]
    fn packed_to_array_roundtrips_elements() {
        let v = Variant::PackedInt32Array(vec![7, 8, 9]);
        assert_eq!(
            v.packed_to_array(),
            Some(vec![Variant::Int(7), Variant::Int(8), Variant::Int(9)])
        );
        assert_eq!(Variant::Int(1).packed_to_array(), None);
    }

    #[test]
    fn packed_to_byte_array_le_layout() {
        // PackedInt32Array([1]) → 4 LE bytes.
        assert_eq!(
            Variant::PackedInt32Array(vec![1]).packed_to_byte_array(),
            Some(Variant::PackedByteArray(vec![1, 0, 0, 0]))
        );
        // Byte array is its own byte image.
        assert_eq!(
            Variant::PackedByteArray(vec![9, 8]).packed_to_byte_array(),
            Some(Variant::PackedByteArray(vec![9, 8]))
        );
        // Float32 0.5 = 0x3F000000 LE.
        assert_eq!(
            Variant::PackedFloat32Array(vec![0.5]).packed_to_byte_array(),
            Some(Variant::PackedByteArray(vec![0x00, 0x00, 0x00, 0x3F]))
        );
        // Vector2(1,2) → two f32 = 8 bytes.
        assert_eq!(
            Variant::PackedVector2Array(vec![Vector2::new(1.0, 2.0)])
                .packed_to_byte_array(),
            Some(Variant::PackedByteArray(vec![
                0x00, 0x00, 0x80, 0x3F, 0x00, 0x00, 0x00, 0x40
            ]))
        );
        // String arrays have no byte image.
        assert_eq!(
            Variant::PackedStringArray(vec!["x".into()]).packed_to_byte_array(),
            None
        );
    }
}
