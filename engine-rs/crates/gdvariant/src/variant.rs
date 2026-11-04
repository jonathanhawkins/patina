//! Core Variant enum and value container types.
//!
//! Godot's Variant is a tagged union that can hold any engine value.
//! This implementation covers the subset needed for Phase 3 (headless
//! runtime): scalars, strings, math types, collections, and object refs.

use crate::shared::{SharedArray, SharedDictionary};
use gdcore::id::ObjectId;
use gdcore::id::Rid;
use gdcore::math::{
    Color, Rect2, Rect2i, Transform2D, Vector2, Vector2i, Vector3, Vector3i, Vector4, Vector4i,
};
use gdcore::math3d::{Aabb, Basis, Plane, Projection, Quaternion, Transform3D};
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
        /// Enclosing locals captured BY VALUE when the `func(...)` expression
        /// evaluated (pat-s3rrv.67).
        ///
        /// Godot's `GDScriptLambdaCallable` stores the captured Variants and
        /// rebinds them on every invocation, so a lambda held in an Array, a
        /// Tween or a signal connection still sees the values from where it was
        /// written. Patina bound nothing, and invoked the body against whatever
        /// environment happened to be current at CALL time — so the canonical
        /// `for i in n: cbs.append(func(): total += i)` read a stale `i`, or
        /// raised because the name no longer existed.
        ///
        /// `Arc` so cloning a Callable stays cheap; the map is never mutated
        /// after construction, which is what "by value" means here.
        captures: Arc<HashMap<String, Variant>>,
        /// Default-value EXPRESSIONS for the parameters, positionally aligned
        /// with `params` — `None` where a parameter has no default
        /// (pat-s3rrv.67).
        ///
        /// Stored opaquely for the same reason `body` is: gdvariant cannot see
        /// the parser's `Expr` type. The interpreter downcasts it back to
        /// `Vec<Option<Expr>>`.
        ///
        /// ⚠️ EXPRESSIONS, not evaluated values. Godot evaluates a default when
        /// the call omits the argument, not when the function is defined, so a
        /// default that reads a captured local must see it at CALL time. Storing
        /// pre-evaluated Variants here would be a different language.
        param_defaults: Arc<dyn std::any::Any + Send + Sync>,
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
            // Godot's `CallableCustomBind::call` (`core/variant/callable_bind.cpp:141`)
            // appends THIS level's binds and then calls the inner callable with
            // the result, so the inner level sees them. Resolving the inner level
            // first put nested binds in reverse (`bind(100).bind(200)` gave
            // `[.., 100, 200]` where Godot gives `[.., 200, 100]`) and let an inner
            // `unbind` skip an outer bind it should have dropped.
            CallableRef::Bound { inner, bound_args } => {
                let mut with_binds = call_args.to_vec();
                with_binds.extend(bound_args.iter().cloned());
                inner.resolve_args(&with_binds)
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
            // Godot compares lambda callables BY REFERENCE — `compare_equal` is
            // literally `return p_a == p_b;`
            // (modules/gdscript/gdscript_lambda_callable.cpp:37-40). Two clones
            // of one Callable share this `Arc`, so a lambda stored in an Array
            // or a signal connection still equals the value the script holds;
            // two separate evaluations of the same `func(): ...` expression each
            // build a fresh `Arc` and are correctly unequal, matching Godot,
            // where each evaluation produces a separate GDScriptLambdaCallable.
            //
            // ⚠️ Cast the fat pointer to a thin one before comparing.
            // `Arc::ptr_eq` on a `dyn Trait` also compares the vtable address,
            // which is not guaranteed unique per type and can give a false
            // negative for two Arcs that really are the same allocation.
            //
            // ⚠️ Compares `body` only, deliberately. `captures` and
            // `param_defaults` are per-instance data hanging off the same
            // lambda; two Callables sharing a body Arc ARE the same lambda
            // instance, and hashing the captures would make equality depend on
            // runtime values Godot never consults (pat-s3rrv.67).
            (CallableRef::Lambda { body: a, .. }, CallableRef::Lambda { body: b, .. }) => {
                std::ptr::eq(Arc::as_ptr(a) as *const (), Arc::as_ptr(b) as *const ())
            }
            _ => false, // mixed callable types are never equal
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
    /// Integer 2D vector (Godot `Vector2i`) — kept distinct from
    /// [`Vector2`](Self::Vector2) so a `.tscn` literal round-trips as an int
    /// vector rather than a float one (pat-nzgdu.78).
    Vector2i,
    Vector3,
    /// Integer 3D vector (Godot `Vector3i`).
    Vector3i,
    Rect2,
    /// Integer 2D rectangle (Godot `Rect2i`) — a TileSet atlas region or a
    /// pixel-exact UI rect, which [`Rect2`](Self::Rect2)'s floats would round
    /// (pat-s3rrv.59).
    Rect2i,
    Transform2D,
    /// 4D float vector (Godot `Vector4`): a shader `vec4`, a homogeneous
    /// coordinate, a column of a [`Projection`](Self::Projection).
    Vector4,
    /// 4D integer vector (Godot `Vector4i`).
    Vector4i,
    Color,
    Basis,
    Transform3D,
    /// A 4×4 projection matrix (Godot `Projection`): every camera and XR view
    /// matrix.
    Projection,
    Quaternion,
    Aabb,
    Plane,
    /// An opaque server handle (Godot `RID`) — what every `*Server` API
    /// returns.
    Rid,
    ObjectId,
    Array,
    Dictionary,
    Callable,
    /// A bound `object.signal_name` pair (Godot's `Signal` builtin).
    /// Placed next to `Callable` to mirror Godot's own adjacency.
    Signal,
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
    /// A tightly-packed array of 4D vectors (`PackedVector4Array`, Godot 4.3+):
    /// skin weights and raw shader data.
    PackedVector4Array,
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
    /// An integer 2D vector (Godot `Vector2i`): tile/grid coordinates, which
    /// lose their exactness if carried as floats (pat-nzgdu.78).
    Vector2i(Vector2i),
    /// A 3D vector.
    Vector3(Vector3),
    /// An integer 3D vector (Godot `Vector3i`).
    Vector3i(Vector3i),
    /// An axis-aligned 2D rectangle.
    Rect2(Rect2),
    /// An axis-aligned 2D rectangle with integer bounds (Godot `Rect2i`).
    Rect2i(Rect2i),
    /// A 2D affine transform.
    Transform2D(Transform2D),
    /// A 4D vector (Godot `Vector4`).
    Vector4(Vector4),
    /// An integer 4D vector (Godot `Vector4i`).
    Vector4i(Vector4i),
    /// An RGBA color.
    Color(Color),
    /// A 3×3 rotation/scale matrix.
    Basis(Basis),
    /// A 3D affine transform.
    Transform3D(Transform3D),
    /// A 4×4 projection matrix (Godot `Projection`), stored column-major.
    Projection(Projection),
    /// A quaternion rotation.
    Quaternion(Quaternion),
    /// An axis-aligned bounding box.
    Aabb(Aabb),
    /// An infinite plane.
    Plane(Plane),
    /// An opaque server handle (Godot `RID`).
    ///
    /// Carried by value rather than boxed: it is a single `u64` and every
    /// `*Server` call produces one, so an allocation per handle would be a
    /// real cost.
    Rid(Rid),
    /// A reference to an engine object by ID.
    ObjectId(ObjectId),
    /// A heterogeneous ordered list (Godot `Array`).
    ///
    /// A handle to shared storage, like Godot's refcounted `ArrayPrivate`:
    /// cloning the `Variant` shares the array (`var b = a; b.append(1)` is
    /// visible through `a`). Use [`SharedArray::duplicate`] for a copy
    /// (pat-s3rrv.8).
    Array(SharedArray),
    /// A Variant-keyed, insertion-ordered map (Godot `Dictionary`).
    ///
    /// Shared storage with the same contract as [`Variant::Array`].
    Dictionary(SharedDictionary),
    /// A callable reference (method or lambda).
    Callable(Box<CallableRef>),
    /// A signal bound to the object that declares it (Godot's `Signal`
    /// builtin), as produced by `object.signal_name` (pat-0q2kx.9).
    ///
    /// The object is carried as a raw `ObjectId` rather than a reference so a
    /// `Signal` stays a plain value. Binding the object is the whole point:
    /// before this, a bareword signal resolved to a name-only await token, so
    /// `await $ButtonA.pressed` was resumed by `$ButtonB.pressed` too.
    Signal {
        /// Raw `ObjectId` of the object that owns the signal.
        object_id: u64,
        /// The signal's name.
        name: String,
    },
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
    /// A tightly-packed array of 4D vectors (`PackedVector4Array`).
    PackedVector4Array(Vec<Vector4>),
}

impl Variant {
    /// Godot's `Variant::duplicate(deep)`: a NEW Array/Dictionary handle with
    /// the same contents (nested containers too when `deep`), and a plain
    /// clone for every other kind.
    ///
    /// Plain `clone()` SHARES an Array or Dictionary (pat-s3rrv.8). Use this
    /// wherever one stored value seeds many owners — class property defaults,
    /// a parsed scene instanced repeatedly — so they do not alias each other.
    pub fn duplicate(&self, deep: bool) -> Variant {
        match self {
            Variant::Array(a) => Variant::Array(a.duplicate(deep)),
            Variant::Dictionary(d) => Variant::Dictionary(d.duplicate(deep)),
            other => other.clone(),
        }
    }

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
            Self::Vector2i(_) => VariantType::Vector2i,
            Self::Vector3(_) => VariantType::Vector3,
            Self::Vector3i(_) => VariantType::Vector3i,
            Self::Rect2(_) => VariantType::Rect2,
            Self::Rect2i(_) => VariantType::Rect2i,
            Self::Transform2D(_) => VariantType::Transform2D,
            Self::Vector4(_) => VariantType::Vector4,
            Self::Vector4i(_) => VariantType::Vector4i,
            Self::Color(_) => VariantType::Color,
            Self::Basis(_) => VariantType::Basis,
            Self::Transform3D(_) => VariantType::Transform3D,
            Self::Projection(_) => VariantType::Projection,
            Self::Quaternion(_) => VariantType::Quaternion,
            Self::Aabb(_) => VariantType::Aabb,
            Self::Plane(_) => VariantType::Plane,
            Self::Rid(_) => VariantType::Rid,
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
            Self::PackedVector4Array(_) => VariantType::PackedVector4Array,
            Self::Signal { .. } => VariantType::Signal,
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
            // Godot: a Signal is falsy only when null — i.e. bound to nothing.
            // `Signal.is_null()` reports the same condition.
            Self::Signal { object_id, name } => *object_id != 0 && !name.is_empty(),
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
            Self::PackedVector4Array(v) => !v.is_empty(),
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
                | Self::PackedVector4Array(_)
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
            Self::PackedVector4Array(v) => v.len(),
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
            Self::PackedVector4Array(v) => Variant::Vector4(*v.get(index)?),
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
            Self::PackedVector4Array(v) => match value {
                Variant::Vector4(x) => set_slot(v, index, *x),
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
            Self::PackedVector4Array(v) => match value {
                Variant::Vector4(x) => v.push(*x),
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
            Self::PackedVector4Array(v) => v.resize(new_len, Vector4::ZERO),
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
            Self::PackedVector4Array(v) => Self::PackedVector4Array(v[b..e].to_vec()),
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
            Self::PackedVector4Array(v) => v
                .iter()
                .flat_map(|p| {
                    [
                        p.x.to_le_bytes(),
                        p.y.to_le_bytes(),
                        p.z.to_le_bytes(),
                        p.w.to_le_bytes(),
                    ]
                })
                .flatten()
                .collect(),
            _ => return None,
        };
        Some(Variant::PackedByteArray(bytes))
    }

    // -- the shared method surface (pat-0q2kx.68) ---------------------------
    //
    // Godot gives every packed array the same 23-method surface. These are the
    // ordering/searching/editing half of it; the interpreter dispatches to them
    // by name. Each returns `false`/`None` for a non-packed receiver so the
    // caller can keep its own "no such method" error.

    /// Sorts in place (Godot `Packed*Array.sort`).
    ///
    /// Vectors and colors sort lexicographically by component, matching Godot's
    /// `operator<` on those types. Floats order with `partial_cmp`, treating an
    /// incomparable pair (NaN) as equal rather than panicking — `sort_by` would
    /// otherwise be free to misbehave on an inconsistent comparator.
    pub fn packed_sort(&mut self) -> bool {
        fn sort_floats<T: PartialOrd + Copy>(v: &mut [T]) {
            v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        }
        match self {
            Self::PackedByteArray(v) => v.sort_unstable(),
            Self::PackedInt32Array(v) => v.sort_unstable(),
            Self::PackedInt64Array(v) => v.sort_unstable(),
            Self::PackedFloat32Array(v) => sort_floats(v),
            Self::PackedFloat64Array(v) => sort_floats(v),
            Self::PackedStringArray(v) => v.sort(),
            Self::PackedVector2Array(v) => v.sort_by(|a, b| cmp_f32_pair(a.x, a.y, b.x, b.y)),
            Self::PackedVector3Array(v) => {
                v.sort_by(|a, b| cmp_f32_pair(a.x, a.y, b.x, b.y).then(f32_cmp(a.z, b.z)))
            }
            Self::PackedColorArray(v) => v.sort_by(|a, b| {
                cmp_f32_pair(a.r, a.g, b.r, b.g)
                    .then(f32_cmp(a.b, b.b))
                    .then(f32_cmp(a.a, b.a))
            }),
            // Godot sorts a PackedVector4Array lexicographically by component,
            // the same total order the Vector2/Vector3 arms use extended by one
            // axis.
            Self::PackedVector4Array(v) => v.sort_by(|a, b| {
                cmp_f32_pair(a.x, a.y, b.x, b.y)
                    .then(f32_cmp(a.z, b.z))
                    .then(f32_cmp(a.w, b.w))
            }),
            _ => return false,
        }
        true
    }

    /// Godot `Packed*Array.bsearch` — the index where `value` belongs in a
    /// sorted array. With `before` (Godot's default) the result is the first
    /// such index, otherwise the last. Returns `len` when `value` sorts past the
    /// end, exactly as Godot does.
    ///
    /// Only meaningful on a sorted array; Godot says the same.
    pub fn packed_bsearch(&self, value: &Variant, before: bool) -> Option<usize> {
        let len = self.packed_len()?;
        // Partition on the comparison the sort above uses, so bsearch and sort
        // agree by construction rather than by two hand-written orderings.
        let mut lo = 0usize;
        let mut hi = len;
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            let elem = self.packed_get(mid)?;
            let ord = compare_packed_elements(&elem, value);
            let go_right = if before {
                ord == std::cmp::Ordering::Less
            } else {
                ord != std::cmp::Ordering::Greater
            };
            if go_right {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        Some(lo)
    }

    /// Godot `Packed*Array.find` — first index at or after `from` holding
    /// `value`, or `-1`. `None` only when this is not a packed array.
    pub fn packed_find(&self, value: &Variant, from: usize) -> Option<i64> {
        let len = self.packed_len()?;
        for i in from..len {
            if packed_elements_equal(&self.packed_get(i)?, value) {
                return Some(i as i64);
            }
        }
        Some(-1)
    }

    /// Godot `Packed*Array.rfind` — last index at or before `to` holding
    /// `value`, or `-1`.
    pub fn packed_rfind(&self, value: &Variant, to: Option<usize>) -> Option<i64> {
        let len = self.packed_len()?;
        let start = to
            .unwrap_or(len.saturating_sub(1))
            .min(len.saturating_sub(1));
        if len == 0 {
            return Some(-1);
        }
        for i in (0..=start).rev() {
            if packed_elements_equal(&self.packed_get(i)?, value) {
                return Some(i as i64);
            }
        }
        Some(-1)
    }

    /// Godot `Packed*Array.count` — how many elements equal `value`.
    pub fn packed_count(&self, value: &Variant) -> Option<usize> {
        let len = self.packed_len()?;
        let mut n = 0;
        for i in 0..len {
            if packed_elements_equal(&self.packed_get(i)?, value) {
                n += 1;
            }
        }
        Some(n)
    }

    /// Godot `Packed*Array.has` — whether any element equals `value`.
    pub fn packed_has(&self, value: &Variant) -> Option<bool> {
        Some(self.packed_find(value, 0)? >= 0)
    }

    /// Godot `Packed*Array.reverse` — reverses in place.
    pub fn packed_reverse(&mut self) -> bool {
        match self {
            Self::PackedByteArray(v) => v.reverse(),
            Self::PackedInt32Array(v) => v.reverse(),
            Self::PackedInt64Array(v) => v.reverse(),
            Self::PackedFloat32Array(v) => v.reverse(),
            Self::PackedFloat64Array(v) => v.reverse(),
            Self::PackedStringArray(v) => v.reverse(),
            Self::PackedVector2Array(v) => v.reverse(),
            Self::PackedVector3Array(v) => v.reverse(),
            Self::PackedColorArray(v) => v.reverse(),
            Self::PackedVector4Array(v) => v.reverse(),
            _ => return false,
        }
        true
    }

    /// Godot `Packed*Array.fill` — overwrites every element with `value`.
    pub fn packed_fill(&mut self, value: &Variant) -> bool {
        let Some(len) = self.packed_len() else {
            return false;
        };
        for i in 0..len {
            if !self.packed_set(i, value) {
                return false;
            }
        }
        true
    }

    /// Godot `Packed*Array.insert` — inserts before `index` (clamped to the
    /// end, as Godot does for an index past the tail).
    pub fn packed_insert(&mut self, index: usize, value: &Variant) -> bool {
        let Some(len) = self.packed_len() else {
            return false;
        };
        let at = index.min(len);
        // Push then rotate: `packed_push` already owns the per-type coercion
        // rules, so there is one place that decides what a value means for each
        // element type.
        if !self.packed_push(value) {
            return false;
        }
        match self {
            Self::PackedByteArray(v) => v[at..].rotate_right(1),
            Self::PackedInt32Array(v) => v[at..].rotate_right(1),
            Self::PackedInt64Array(v) => v[at..].rotate_right(1),
            Self::PackedFloat32Array(v) => v[at..].rotate_right(1),
            Self::PackedFloat64Array(v) => v[at..].rotate_right(1),
            Self::PackedStringArray(v) => v[at..].rotate_right(1),
            Self::PackedVector2Array(v) => v[at..].rotate_right(1),
            Self::PackedVector3Array(v) => v[at..].rotate_right(1),
            Self::PackedColorArray(v) => v[at..].rotate_right(1),
            Self::PackedVector4Array(v) => v[at..].rotate_right(1),
            _ => return false,
        }
        true
    }

    /// Godot `Packed*Array.remove_at` — removes the element at `index`.
    /// `false` when the index is out of range.
    pub fn packed_remove_at(&mut self, index: usize) -> bool {
        let Some(len) = self.packed_len() else {
            return false;
        };
        if index >= len {
            return false;
        }
        match self {
            Self::PackedByteArray(v) => drop(v.remove(index)),
            Self::PackedInt32Array(v) => drop(v.remove(index)),
            Self::PackedInt64Array(v) => drop(v.remove(index)),
            Self::PackedFloat32Array(v) => drop(v.remove(index)),
            Self::PackedFloat64Array(v) => drop(v.remove(index)),
            Self::PackedStringArray(v) => drop(v.remove(index)),
            Self::PackedVector2Array(v) => drop(v.remove(index)),
            Self::PackedVector3Array(v) => drop(v.remove(index)),
            Self::PackedColorArray(v) => drop(v.remove(index)),
            Self::PackedVector4Array(v) => drop(v.remove(index)),
            _ => return false,
        }
        true
    }

    /// Godot `Packed*Array.append_array` — appends every element of `other`,
    /// coerced to this array's element type.
    pub fn packed_append_array(&mut self, other: &Variant) -> bool {
        let Some(items) = other.packed_to_array() else {
            return false;
        };
        for item in items {
            if !self.packed_push(&item) {
                return false;
            }
        }
        true
    }

    // -- PackedByteArray codecs (pat-0q2kx.68) ------------------------------

    /// Godot `PackedByteArray.decode_*` — reads one little-endian value at
    /// `offset`. `kind` is the method's suffix (`"u8"`, `"s32"`, `"double"`, …).
    ///
    /// `None` when this is not a byte array, the kind is unknown, or the read
    /// would run past the end — Godot returns 0 for a short buffer, but the
    /// caller is better placed to decide that than this function is.
    pub fn packed_byte_decode(&self, offset: usize, kind: &str) -> Option<Variant> {
        let Self::PackedByteArray(bytes) = self else {
            return None;
        };
        let take = |n: usize| -> Option<&[u8]> { bytes.get(offset..offset + n) };
        Some(match kind {
            "u8" => Variant::Int(i64::from(*bytes.get(offset)?)),
            "s8" => Variant::Int(i64::from(*bytes.get(offset)? as i8)),
            "u16" => Variant::Int(i64::from(u16::from_le_bytes(take(2)?.try_into().ok()?))),
            "s16" => Variant::Int(i64::from(i16::from_le_bytes(take(2)?.try_into().ok()?))),
            "u32" => Variant::Int(i64::from(u32::from_le_bytes(take(4)?.try_into().ok()?))),
            "s32" => Variant::Int(i64::from(i32::from_le_bytes(take(4)?.try_into().ok()?))),
            // Godot's decode_u64 hands back a 64-bit pattern in an int; the
            // reinterpretation is Godot's, not a loss introduced here.
            "u64" => Variant::Int(u64::from_le_bytes(take(8)?.try_into().ok()?) as i64),
            "s64" => Variant::Int(i64::from_le_bytes(take(8)?.try_into().ok()?)),
            "float" => Variant::Float(f64::from(f32::from_le_bytes(take(4)?.try_into().ok()?))),
            "double" => Variant::Float(f64::from_le_bytes(take(8)?.try_into().ok()?)),
            _ => return None,
        })
    }

    /// Godot `PackedByteArray.encode_*` — writes one little-endian value at
    /// `offset`, growing the array when the write runs past the end (Godot
    /// requires the caller to `resize` first; growing is the forgiving choice
    /// and never truncates existing data).
    pub fn packed_byte_encode(&mut self, offset: usize, kind: &str, value: &Variant) -> bool {
        let encoded: Vec<u8> = match kind {
            "u8" => vec![value.to_int() as u8],
            "s8" => vec![value.to_int() as i8 as u8],
            "u16" => (value.to_int() as u16).to_le_bytes().to_vec(),
            "s16" => (value.to_int() as i16).to_le_bytes().to_vec(),
            "u32" => (value.to_int() as u32).to_le_bytes().to_vec(),
            "s32" => (value.to_int() as i32).to_le_bytes().to_vec(),
            "u64" => (value.to_int() as u64).to_le_bytes().to_vec(),
            "s64" => value.to_int().to_le_bytes().to_vec(),
            "float" => (value.to_float() as f32).to_le_bytes().to_vec(),
            "double" => value.to_float().to_le_bytes().to_vec(),
            _ => return false,
        };
        let Self::PackedByteArray(bytes) = self else {
            return false;
        };
        let end = offset + encoded.len();
        if bytes.len() < end {
            bytes.resize(end, 0);
        }
        bytes[offset..end].copy_from_slice(&encoded);
        true
    }

    /// Godot `Packed*Array.to_int32_array()` and friends — reinterprets this
    /// array's byte image as `kind`'s element type.
    ///
    /// `kind` is the converter's suffix: `"int32"`, `"int64"`, `"float32"`,
    /// `"float64"`. Trailing bytes that do not fill a whole element are
    /// dropped, as Godot does.
    pub fn packed_to_typed_array(&self, kind: &str) -> Option<Variant> {
        let Variant::PackedByteArray(bytes) = self.packed_to_byte_array()? else {
            return None;
        };
        Some(match kind {
            "int32" => Variant::PackedInt32Array(
                bytes
                    .chunks_exact(4)
                    .map(|c| i32::from_le_bytes(c.try_into().unwrap()))
                    .collect(),
            ),
            "int64" => Variant::PackedInt64Array(
                bytes
                    .chunks_exact(8)
                    .map(|c| i64::from_le_bytes(c.try_into().unwrap()))
                    .collect(),
            ),
            "float32" => Variant::PackedFloat32Array(
                bytes
                    .chunks_exact(4)
                    .map(|c| f32::from_le_bytes(c.try_into().unwrap()))
                    .collect(),
            ),
            "float64" => Variant::PackedFloat64Array(
                bytes
                    .chunks_exact(8)
                    .map(|c| f64::from_le_bytes(c.try_into().unwrap()))
                    .collect(),
            ),
            _ => return None,
        })
    }
}

/// Total order over two `f32`s, with NaN treated as equal to everything so a
/// comparator built from this stays consistent.
fn f32_cmp(a: f32, b: f32) -> std::cmp::Ordering {
    a.partial_cmp(&b).unwrap_or(std::cmp::Ordering::Equal)
}

/// Lexicographic order over a leading pair of components.
fn cmp_f32_pair(ax: f32, ay: f32, bx: f32, by: f32) -> std::cmp::Ordering {
    f32_cmp(ax, bx).then(f32_cmp(ay, by))
}

/// Orders two packed elements the way [`Variant::packed_sort`] does, so
/// `bsearch` agrees with `sort`.
fn compare_packed_elements(a: &Variant, b: &Variant) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    match (a, b) {
        (Variant::String(x), Variant::String(y)) => x.cmp(y),
        (Variant::Vector2(x), Variant::Vector2(y)) => cmp_f32_pair(x.x, x.y, y.x, y.y),
        (Variant::Vector3(x), Variant::Vector3(y)) => {
            cmp_f32_pair(x.x, x.y, y.x, y.y).then(f32_cmp(x.z, y.z))
        }
        (Variant::Color(x), Variant::Color(y)) => cmp_f32_pair(x.r, x.g, y.r, y.g)
            .then(f32_cmp(x.b, y.b))
            .then(f32_cmp(x.a, y.a)),
        // Numbers compare numerically across int/float, so a float probe against
        // an int array behaves.
        _ => a
            .to_float()
            .partial_cmp(&b.to_float())
            .unwrap_or(Ordering::Equal),
    }
}

/// Equality for `find`/`rfind`/`count`/`has`, numeric-aware for the same reason.
fn packed_elements_equal(a: &Variant, b: &Variant) -> bool {
    match (a, b) {
        (Variant::String(x), Variant::String(y)) => x == y,
        (Variant::Vector2(_), Variant::Vector2(_))
        | (Variant::Vector3(_), Variant::Vector3(_))
        | (Variant::Color(_), Variant::Color(_)) => a == b,
        _ => a.to_float() == b.to_float(),
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

/// A numeric scalar operand as `f32`, for vector-by-scalar operators.
fn scalar_operand(v: &Variant) -> Option<f32> {
    match v {
        Variant::Int(i) => Some(*i as f32),
        Variant::Float(f) => Some(*f as f32),
        _ => None,
    }
}

// The vector arms below follow Godot's operator table
// (core/variant/variant_op.cpp: OP_ADD/OP_SUBTRACT/OP_MULTIPLY/OP_DIVIDE/
// OP_NEGATE register Vector2/Vector3 with themselves and with int/float).
// They were missing, so `Variant::Vector2(..) + Variant::Vector2(..)` was Nil.

impl std::ops::Add for Variant {
    type Output = Variant;

    fn add(self, rhs: Variant) -> Variant {
        match (&self, &rhs) {
            (Variant::Int(a), Variant::Int(b)) => Variant::Int(a.wrapping_add(*b)),
            (Variant::Float(a), Variant::Float(b)) => Variant::Float(a + b),
            (Variant::Int(a), Variant::Float(b)) => Variant::Float(*a as f64 + b),
            (Variant::Float(a), Variant::Int(b)) => Variant::Float(a + *b as f64),
            (Variant::String(a), Variant::String(b)) => Variant::String(format!("{a}{b}")),
            (Variant::Vector2(a), Variant::Vector2(b)) => Variant::Vector2(*a + *b),
            (Variant::Vector3(a), Variant::Vector3(b)) => Variant::Vector3(*a + *b),
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
            (Variant::Vector2(a), Variant::Vector2(b)) => Variant::Vector2(*a - *b),
            (Variant::Vector3(a), Variant::Vector3(b)) => Variant::Vector3(*a - *b),
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
            // Component-wise, as Godot's Vector * Vector.
            (Variant::Vector2(a), Variant::Vector2(b)) => {
                Variant::Vector2(gdcore::math::Vector2::new(a.x * b.x, a.y * b.y))
            }
            (Variant::Vector3(a), Variant::Vector3(b)) => {
                Variant::Vector3(gdcore::math::Vector3::new(a.x * b.x, a.y * b.y, a.z * b.z))
            }
            (Variant::Vector2(v), k) | (k, Variant::Vector2(v)) => match scalar_operand(k) {
                Some(k) => Variant::Vector2(*v * k),
                None => Variant::Nil,
            },
            (Variant::Vector3(v), k) | (k, Variant::Vector3(v)) => match scalar_operand(k) {
                Some(k) => Variant::Vector3(*v * k),
                None => Variant::Nil,
            },
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
            // Float division, so a zero divisor gives inf as in Godot.
            (Variant::Vector2(a), Variant::Vector2(b)) => {
                Variant::Vector2(gdcore::math::Vector2::new(a.x / b.x, a.y / b.y))
            }
            (Variant::Vector3(a), Variant::Vector3(b)) => {
                Variant::Vector3(gdcore::math::Vector3::new(a.x / b.x, a.y / b.y, a.z / b.z))
            }
            (Variant::Vector2(v), k) => match scalar_operand(k) {
                Some(k) => Variant::Vector2(gdcore::math::Vector2::new(v.x / k, v.y / k)),
                None => Variant::Nil,
            },
            (Variant::Vector3(v), k) => match scalar_operand(k) {
                Some(k) => Variant::Vector3(gdcore::math::Vector3::new(v.x / k, v.y / k, v.z / k)),
                None => Variant::Nil,
            },
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
            Variant::Vector2(v) => Variant::Vector2(-v),
            Variant::Vector3(v) => Variant::Vector3(-v),
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

/// Comma-joins nested variants for container printing. Godot prints a
/// container's ELEMENTS, and a nested string keeps its quotes inside a
/// container while printing bare on its own (`print("hi")` → `hi`,
/// `print(["hi"])` → `["hi"]`).
fn join_variants(items: &[Variant]) -> String {
    items
        .iter()
        .map(display_element)
        .collect::<Vec<_>>()
        .join(", ")
}

/// An element inside a container: strings are quoted, everything else prints as
/// itself.
fn display_element(v: &Variant) -> String {
    match v {
        Variant::String(s) => format!("\"{s}\""),
        other => other.to_string(),
    }
}

/// A dictionary key, quoted the same way an element is.
fn display_key(k: &Variant) -> String {
    display_element(k)
}

fn join_display<T: fmt::Display>(items: &[T]) -> String {
    items
        .iter()
        .map(|i| i.to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

// ---------------------------------------------------------------------------
// Godot-exact float stringification (pat-mgixk.103)
// ---------------------------------------------------------------------------

/// Godot's `String::num` (`core/string/ustring.cpp`): format with
/// `%.<decimals>f`, then destroy trailing zeros, and the `.` with them if
/// nothing is left after it.
///
/// ⚠️ The trailing `.` IS removed, despite Godot's comment saying "except one
/// after the decimal point" — its loop breaks on `.` only after zeroing it. That
/// is why `num_real` has to handle the integral case separately to produce
/// `"1.0"`; `num(1.0, 14)` alone yields `"1"`.
pub fn num_with_decimals(value: f64, decimals: i32) -> String {
    if !value.is_finite() {
        // C's `snprintf` spells these lowercase, and Godot inherits that;
        // Rust's `{}` would give "NaN".
        return if value.is_nan() {
            "nan".to_string()
        } else if value > 0.0 {
            "inf".to_string()
        } else {
            "-inf".to_string()
        };
    }
    // Godot clamps to MAX_DECIMALS (32) and treats a negative count as "use
    // num_real's own default", which never reaches here.
    let decimals = decimals.clamp(0, 32) as usize;
    let mut out = format!("{value:.decimals$}");
    if out.contains('.') {
        while out.ends_with('0') {
            out.pop();
        }
        if out.ends_with('.') {
            out.pop();
        }
    }
    out
}

/// Godot's `String::num_real(double, bool)` — the formatter
/// `Variant::stringify` uses for FLOAT (`core/variant/variant.cpp`).
///
/// An integral value prints with a trailing `.0`, which is the whole reason
/// `str(1.0)` is `"1.0"` in Godot and was `"1"` here. Otherwise 14 significant
/// decimals, which is what turns `0.1 + 0.2` from
/// `"0.30000000000000004"` into `"0.3"`.
///
/// ⚠️ The magnitude correction is `if value > 10.0`, POSITIVE only — Godot does
/// not use `abs` here, so a large negative keeps all 14 decimals. Ported as-is
/// rather than "fixed": this function exists to match Godot's output, and
/// symmetrising it would diverge from the engine it is a port of.
pub fn num_real_f64(value: f64, trailing: bool) -> String {
    if value.is_finite() && value == ((value as i64) as f64) {
        let int_part = format!("{}", value as i64);
        return if trailing {
            format!("{int_part}.0")
        } else {
            int_part
        };
    }
    let mut decimals = 14;
    if value > 10.0 {
        decimals -= value.log10().floor() as i32;
    }
    num_with_decimals(value, decimals)
}

/// Godot's `String::num_real(float, bool)` overload, which every math type's
/// `operator String` uses for its components (`core/math/vector2.cpp` and
/// siblings).
///
/// 🔑 Identical to [`num_real_f64`] except the decimal budget is **6**, not 14 —
/// single-precision carries no more than that, and Godot picks the overload by
/// the component's own type. Using the f64 budget for an `f32` component would
/// print the binary representation error (a `0.1f32` widened to f64 is
/// `0.100000001490116...`), which is exactly the noise this is meant to hide.
pub fn num_real_f32(value: f32, trailing: bool) -> String {
    if value.is_finite() && value == ((value as i64) as f32) {
        let int_part = format!("{}", value as i64);
        return if trailing {
            format!("{int_part}.0")
        } else {
            int_part
        };
    }
    let mut decimals = 6;
    if value > 10.0 {
        decimals -= value.log10().floor() as i32;
    }
    num_with_decimals(value as f64, decimals)
}

/// Shorthand for a math-type component: the vector, rect, basis and
/// transform `operator String`s pass `true` for `p_trailing`
/// (core/math/vector2.cpp:210), so a whole number keeps its `.0`.
///
/// ⚠️ NOT every type: Color uses `String::num(r, 4)` ([`num4`]) and Quaternion
/// `num_real(x, false)` — see those arms.
fn c(value: f32) -> String {
    num_real_f32(value, true)
}

/// Godot's `String::num(v, 4)` (`Color::operator String`, core/math/color.cpp:483):
/// at most four decimals, trailing zeros and a bare `.` dropped — `1`, `0.5`.
fn num4(value: f32) -> String {
    let s = format!("{:.4}", value);
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" { "0".to_string() } else { s.to_string() }
}

impl fmt::Display for Variant {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Nil => write!(f, "<null>"),
            Self::Bool(b) => write!(f, "{b}"),
            Self::Int(i) => write!(f, "{i}"),
            // `Variant::stringify` routes FLOAT through `String::num_real(v, true)`.
            Self::Float(v) => write!(f, "{}", num_real_f64(*v, true)),
            Self::String(s) => write!(f, "{s}"),
            // ⚠️ BARE, not `&name`. `Variant::stringify` prints STRING_NAME and
            // NODE_PATH as their plain text; the decorated forms belong to
            // `var_to_str`, which has its own formatter (`variant_to_gd_string`)
            // and is deliberately untouched.
            Self::StringName(sn) => write!(f, "{sn}"),
            Self::NodePath(np) => write!(f, "{np}"),
            Self::Vector2(v) => write!(f, "({}, {})", c(v.x), c(v.y)),
            Self::Vector2i(v) => write!(f, "({}, {})", v.x, v.y),
            Self::Vector3(v) => write!(f, "({}, {}, {})", c(v.x), c(v.y), c(v.z)),
            Self::Vector3i(v) => write!(f, "({}, {}, {})", v.x, v.y, v.z),
            // Godot's `operator String` shapes (core/variant/variant.cpp): a
            // Rect2 is `[P: …, S: …]`, a matrix type labels its columns, and a
            // container prints its ELEMENTS. The old `[Array; len=2]` /
            // `<Transform2D>` placeholders meant every `print()` of a container
            // or transform diverged from Godot (pat-0q2kx.69).
            Self::Rect2(r) => write!(
                f,
                "[P: ({}, {}), S: ({}, {})]",
                c(r.position.x),
                c(r.position.y),
                c(r.size.x),
                c(r.size.y)
            ),
            // `Rect2i::operator String` (core/math/rect2i.cpp:36) is the same
            // `[P: …, S: …]` shape as Rect2 — the components differ, the
            // envelope does not.
            Self::Rect2i(r) => write!(
                f,
                "[P: ({}, {}), S: ({}, {})]",
                r.position.x, r.position.y, r.size.x, r.size.y
            ),
            Self::Vector4(v) => write!(f, "({}, {}, {}, {})", c(v.x), c(v.y), c(v.z), c(v.w)),
            Self::Vector4i(v) => write!(f, "({}, {}, {}, {})", v.x, v.y, v.z, v.w),
            Self::Transform2D(t) => write!(
                f,
                "[X: ({}, {}), Y: ({}, {}), O: ({}, {})]",
                c(t.x.x),
                c(t.x.y),
                c(t.y.x),
                c(t.y.y),
                c(t.origin.x),
                c(t.origin.y)
            ),
            Self::Color(col) => write!(
                f,
                "({}, {}, {}, {})",
                num4(col.r),
                num4(col.g),
                num4(col.b),
                num4(col.a)
            ),
            Self::Basis(b) => write!(
                f,
                "[X: ({}, {}, {}), Y: ({}, {}, {}), Z: ({}, {}, {})]",
                c(b.x.x),
                c(b.x.y),
                c(b.x.z),
                c(b.y.x),
                c(b.y.y),
                c(b.y.z),
                c(b.z.x),
                c(b.z.y),
                c(b.z.z)
            ),
            // Each column is a `Vector3::operator String`, so it formats like
            // one — this printed raw `{}` floats (`(1, 0, 0)`) while Basis and
            // Vector3 printed `(1.0, 0.0, 0.0)`.
            Self::Transform3D(t) => write!(
                f,
                "[X: ({}, {}, {}), Y: ({}, {}, {}), Z: ({}, {}, {}), O: ({}, {}, {})]",
                c(t.basis.x.x),
                c(t.basis.x.y),
                c(t.basis.x.z),
                c(t.basis.y.x),
                c(t.basis.y.y),
                c(t.basis.y.z),
                c(t.basis.z.x),
                c(t.basis.z.y),
                c(t.basis.z.z),
                c(t.origin.x),
                c(t.origin.y),
                c(t.origin.z)
            ),
            // `Projection::operator String` (core/math/projection.cpp:851)
            // labels its four COLUMNS X/Y/Z/W, each printed as a Vector4.
            // Storage is column-major, so `m[c]` is a whole column.
            Self::Projection(p) => write!(
                f,
                "[X: ({}, {}, {}, {}), Y: ({}, {}, {}, {}), Z: ({}, {}, {}, {}), W: ({}, {}, {}, {})]",
                c(p.m[0][0]), c(p.m[0][1]), c(p.m[0][2]), c(p.m[0][3]),
                c(p.m[1][0]), c(p.m[1][1]), c(p.m[1][2]), c(p.m[1][3]),
                c(p.m[2][0]), c(p.m[2][1]), c(p.m[2][2]), c(p.m[2][3]),
                c(p.m[3][0]), c(p.m[3][1]), c(p.m[3][2]), c(p.m[3][3]),
            ),
            // `Quaternion::operator String` passes `false` (core/math/quaternion.cpp:268).
            Self::Quaternion(q) => write!(
                f,
                "({}, {}, {}, {})",
                num_real_f32(q.x, false),
                num_real_f32(q.y, false),
                num_real_f32(q.z, false),
                num_real_f32(q.w, false)
            ),
            Self::Aabb(a) => write!(
                f,
                "[P: ({}, {}, {}), S: ({}, {}, {})]",
                c(a.position.x),
                c(a.position.y),
                c(a.position.z),
                c(a.size.x),
                c(a.size.y),
                c(a.size.z)
            ),
            Self::Plane(p) => write!(
                f,
                "[N: ({}, {}, {}), D: {}]",
                c(p.normal.x),
                c(p.normal.y),
                c(p.normal.z),
                // `num_real(d, false)` (core/math/plane.cpp:184): no `.0`.
                num_real_f32(p.d, false)
            ),
            // `Variant::stringify` (core/variant/variant.cpp:1739) prints a
            // handle as `RID(<id>)`, NOT the bare number.
            Self::Rid(r) => write!(f, "RID({})", r.id()),
            Self::ObjectId(id) => write!(f, "<Object#{id}>"),
            Self::Array(a) => write!(f, "[{}]", join_variants(&a.to_vec())),
            // Godot pads a dictionary's braces: `{ "k": v }`.
            Self::Dictionary(d) => {
                if d.is_empty() {
                    return write!(f, "{{  }}");
                }
                let body = d
                    .to_dict().iter()
                    .map(|(k, v)| format!("{}: {}", display_key(k), v))
                    .collect::<Vec<_>>()
                    .join(", ");
                write!(f, "{{ {body} }}")
            }
            Self::Callable(c) => write!(f, "{c:?}"),
            Self::Resource(r) => write!(f, "<Resource:{} @ {}>", r.class_name, r.path),
            Self::PackedByteArray(v) => write!(f, "[{}]", join_display(v)),
            Self::PackedInt32Array(v) => write!(f, "[{}]", join_display(v)),
            Self::PackedInt64Array(v) => write!(f, "[{}]", join_display(v)),
            // Packed REAL arrays stringify element-wise through num_real, like
            // any other float; the integer-packed arrays keep `{}`.
            Self::PackedFloat32Array(v) => write!(
                f,
                "[{}]",
                v.iter()
                    .map(|e| num_real_f32(*e, true))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Self::PackedFloat64Array(v) => write!(
                f,
                "[{}]",
                v.iter()
                    .map(|e| num_real_f64(*e, true))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Self::PackedStringArray(v) => write!(f, "[{}]", v.join(", ")),
            Self::PackedVector2Array(v) => write!(
                f,
                "[{}]",
                v.iter()
                    .map(|e| format!("({}, {})", c(e.x), c(e.y)))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Self::PackedVector3Array(v) => write!(
                f,
                "[{}]",
                v.iter()
                    .map(|e| format!("({}, {}, {})", c(e.x), c(e.y), c(e.z)))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Self::PackedColorArray(v) => write!(
                f,
                "[{}]",
                v.iter()
                    .map(|e| format!("({}, {}, {}, {})", c(e.r), c(e.g), c(e.b), c(e.a)))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Self::PackedVector4Array(v) => write!(
                f,
                "[{}]",
                v.iter()
                    .map(|e| format!("({}, {}, {}, {})", e.x, e.y, e.z, e.w))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Self::Signal { object_id, name } => {
                write!(f, "Signal(<Object#{object_id}>, \"{name}\")")
            }
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
            Self::Vector2i => "Vector2i",
            Self::Vector3 => "Vector3",
            Self::Vector3i => "Vector3i",
            Self::Rect2 => "Rect2",
            Self::Rect2i => "Rect2i",
            Self::Transform2D => "Transform2D",
            Self::Vector4 => "Vector4",
            Self::Vector4i => "Vector4i",
            Self::Color => "Color",
            Self::Basis => "Basis",
            Self::Transform3D => "Transform3D",
            Self::Projection => "Projection",
            Self::Quaternion => "Quaternion",
            Self::Aabb => "AABB",
            Self::Plane => "Plane",
            // Godot's own type name is the upper-case acronym (`RID`), which
            // is what `type_string()` and every error message print.
            Self::Rid => "RID",
            Self::ObjectId => "ObjectId",
            Self::Array => "Array",
            Self::Dictionary => "Dictionary",
            Self::Callable => "Callable",
            Self::Signal => "Signal",
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
            Self::PackedVector4Array => "PackedVector4Array",
        };
        write!(f, "{name}")
    }
}

#[cfg(test)]
mod tests {
    /// 🔑 A LAMBDA CALLABLE EQUALS ITSELF, INCLUDING AFTER A CLONE.
    ///
    /// Godot: *"Lambda callables are only compared by reference"*
    /// (`gdscript_lambda_callable.cpp:37-40`). `CallableRef` is
    /// `#[derive(Clone)]`, so cloning bumps the body `Arc`'s refcount rather
    /// than deep-copying — which is what lets a Callable survive being stored
    /// in an Array or a signal connection and still compare equal to the value
    /// the script is holding.
    ///
    /// ⚠️ Before this, `PartialEq` ended `_ => false` with the note "lambdas are
    /// never equal", so `f == f` was FALSE. That silently broke every
    /// remove-by-value path: `remove_visibility_filter(f)` could never match
    /// (`scene_tree.rs`), because `Vec::retain` compares by equality.
    #[test]
    fn a_lambda_equals_its_own_clone() {
        let f = CallableRef::Lambda {
            params: vec!["x".into()],
            body: std::sync::Arc::new(()),
            captures: std::sync::Arc::new(std::collections::HashMap::new()),
            param_defaults: std::sync::Arc::new(()),
        };
        let g = f.clone();
        assert_eq!(f, g, "a cloned Callable must still be the same lambda");
        assert_eq!(f, f, "and a lambda must equal itself");
    }

    /// 🔑 TWO SEPARATELY-BUILT LAMBDAS ARE NOT EQUAL.
    ///
    /// ⚠️ The other half, and the one that keeps the fix honest. Identical
    /// parameter lists and identical (empty) captures must NOT make two
    /// lambdas equal — in Godot each `func(): ...` evaluation produces its own
    /// `GDScriptLambdaCallable`. A comparison that fell back to comparing
    /// `params` would pass the test above and fail this one.
    #[test]
    fn two_distinct_lambdas_are_not_equal() {
        let mk = || CallableRef::Lambda {
            params: vec!["x".into()],
            body: std::sync::Arc::new(()),
            captures: std::sync::Arc::new(std::collections::HashMap::new()),
            param_defaults: std::sync::Arc::new(()),
        };
        assert_ne!(
            mk(),
            mk(),
            "two separate lambda instances must not compare equal, however \
             identical their parameters look"
        );
    }

    /// 🔑 A LAMBDA IS NEVER EQUAL TO A METHOD CALLABLE.
    ///
    /// The mixed-type arm still falls through to `false`; this pins that the
    /// new Lambda arm did not widen it.
    #[test]
    fn a_lambda_is_not_equal_to_a_method() {
        let lambda = CallableRef::Lambda {
            params: vec![],
            body: std::sync::Arc::new(()),
            captures: std::sync::Arc::new(std::collections::HashMap::new()),
            param_defaults: std::sync::Arc::new(()),
        };
        let method = CallableRef::Method {
            target_id: 1,
            method: "foo".into(),
        };
        assert_ne!(lambda, method);
    }

    use super::*;
    use crate::dictionary::Dictionary;

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
        assert!(!Variant::Array(Default::default()).is_truthy());
        assert!(Variant::Array(vec![Variant::Nil].into()).is_truthy());
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

    /// 🔑 The two cases the old `write!("{v}")` got wrong, which is the whole
    /// point of the num_real port.
    #[test]
    fn display_float_matches_godot_num_real() {
        // Integral floats keep a trailing `.0` — Rust's `{}` gave "1".
        assert_eq!(format!("{}", Variant::Float(1.0)), "1.0");
        assert_eq!(format!("{}", Variant::Float(-3.0)), "-3.0");
        // 14 significant decimals hide binary representation error, which is
        // why Godot prints 0.3 for this and Rust printed
        // "0.30000000000000004".
        assert_eq!(format!("{}", Variant::Float(0.1 + 0.2)), "0.3");
        // Not integral, so the decimals path runs and trailing zeros go.
        assert_eq!(format!("{}", Variant::Float(1.5)), "1.5");
        // ⚠️ Above 10 the budget shrinks by the magnitude, matching Godot's
        // `if (p_num > 10) decimals -= floor(log10(p_num))`.
        assert_eq!(format!("{}", Variant::Float(1234.5)), "1234.5");
        // Non-finite spells lowercase, as C's snprintf does and Godot inherits.
        assert_eq!(format!("{}", Variant::Float(f64::NAN)), "nan");
        assert_eq!(format!("{}", Variant::Float(f64::INFINITY)), "inf");
    }

    #[test]
    fn display_string() {
        assert_eq!(format!("{}", Variant::String("hello".into())), "hello");
    }

    #[test]
    fn display_string_name() {
        let sn = StringName::new("test_sn");
        // ⚠️ BARE now, not `&test_sn`. `Variant::stringify` prints a
        // StringName as its plain text (pat-mgixk.103); the `&`-decorated form
        // is `var_to_str`'s, which keeps its own formatter.
        assert_eq!(format!("{}", Variant::StringName(sn)), "test_sn");
    }

    #[test]
    fn display_node_path() {
        let np = NodePath::new("/root/Player");
        assert_eq!(
            format!("{}", Variant::NodePath(np)),
            // Bare text, as `Variant::stringify` prints NODE_PATH.
            "/root/Player"
        );
    }

    #[test]
    fn display_vector2() {
        let v = Vector2::new(1.0, 2.0);
        // Godot's `Vector2::operator String` runs each component through
        // `num_real(x, true)`, so whole numbers carry a trailing `.0`.
        assert_eq!(format!("{}", Variant::Vector2(v)), "(1.0, 2.0)");
    }

    #[test]
    fn display_vector3() {
        let v = gdcore::math::Vector3::new(1.0, 2.0, 3.0);
        assert_eq!(format!("{}", Variant::Vector3(v)), "(1.0, 2.0, 3.0)");
    }

    #[test]
    fn display_rect2() {
        let r = gdcore::math::Rect2::new(Vector2::new(1.0, 2.0), Vector2::new(3.0, 4.0));
        assert_eq!(
            format!("{}", Variant::Rect2(r)),
            "[P: (1.0, 2.0), S: (3.0, 4.0)]"
        );
    }

    #[test]
    fn display_transform2d() {
        let t = gdcore::math::Transform2D::IDENTITY;
        assert_eq!(
            format!("{}", Variant::Transform2D(t)),
            "[X: (1.0, 0.0), Y: (0.0, 1.0), O: (0.0, 0.0)]"
        );
    }

    #[test]
    fn display_color() {
        let c = gdcore::math::Color::new(1.0, 0.5, 0.0, 1.0);
        // `String::num(r, 4)` — no trailing `.0` (core/math/color.cpp:483).
        assert_eq!(format!("{}", Variant::Color(c)), "(1, 0.5, 0, 1)");
    }

    #[test]
    fn display_object_id() {
        let id = gdcore::id::ObjectId::from_raw(99);
        assert_eq!(format!("{}", Variant::ObjectId(id)), "<Object#99>");
    }

    #[test]
    fn display_array() {
        let a = Variant::Array(vec![Variant::Int(1), Variant::Int(2)].into());
        assert_eq!(format!("{a}"), "[1, 2]");
    }

    #[test]
    fn display_dictionary() {
        let mut d = Dictionary::new();
        d.insert("key".to_string(), Variant::Int(1));
        let v = Variant::Dictionary(d.into());
        assert_eq!(format!("{v}"), "{ \"key\": 1 }");
    }

    // -- Clone correctness --------------------------------------------------

    #[test]
    fn clone_nested_array() {
        let inner = Variant::Array(vec![Variant::Int(1), Variant::Int(2)].into());
        let outer = Variant::Array(vec![inner.clone(), Variant::String("x".into())].into());
        let cloned = outer.clone();
        assert_eq!(outer, cloned);
    }

    #[test]
    fn clone_nested_dictionary() {
        let mut inner = Dictionary::new();
        inner.insert("a".to_string(), Variant::Int(1));
        let outer = Variant::Dictionary(inner.into());
        let cloned = outer.clone();
        assert_eq!(outer, cloned);
    }

    // -- Truthy for remaining types -----------------------------------------

    #[test]
    fn dictionary_truthy() {
        assert!(!Variant::Dictionary(Default::default()).is_truthy());
        let mut d = Dictionary::new();
        d.insert("k", Variant::Nil);
        assert!(Variant::Dictionary(d.into()).is_truthy());
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
        assert_eq!(
            format!("{}", VariantType::PackedByteArray),
            "PackedByteArray"
        );
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
        assert_eq!(
            Variant::PackedInt32Array(vec![]).packed_is_empty(),
            Some(true)
        );
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
        assert_eq!(
            v.packed_slice(1, 3),
            Some(Variant::PackedInt64Array(vec![1, 2]))
        );
        // Negative indices count from the end.
        assert_eq!(
            v.packed_slice(-2, 5),
            Some(Variant::PackedInt64Array(vec![3, 4]))
        );
        // Clamped / inverted ranges yield empty, keeping the packed type.
        assert_eq!(
            v.packed_slice(3, 1),
            Some(Variant::PackedInt64Array(vec![]))
        );
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
            Variant::PackedVector2Array(vec![Vector2::new(1.0, 2.0)]).packed_to_byte_array(),
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
