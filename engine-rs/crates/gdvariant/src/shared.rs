//! Shared-storage (reference-semantics) handles for Godot's `Array` and
//! `Dictionary` (pat-s3rrv.8).
//!
//! # Why this module exists
//!
//! Godot's `Array` and `Dictionary` are *handles*, not values.
//! `core/variant/array.h` stores an `ArrayPrivate *_p` behind a refcount, and
//! `doc/classes/Array.xml` states plainly: "Arrays are always passed by
//! reference." So in GDScript:
//!
//! ```gdscript
//! var a = []
//! var b = a
//! b.append(1)
//! print(a)   # [1] — one array, two names
//! ```
//!
//! Patina currently models both as plain values ([`Variant::Array`] holds a
//! `Vec<Variant>`), and the GDScript interpreter papers over the difference
//! with a copy-then-write-back hack in `array_mutate`. That makes `a` and `b`
//! independent, and makes a mutation inside a function invisible to the caller.
//!
//! [`SharedArray`] and [`SharedDictionary`] are the storage types that fix
//! this: cloning one clones the *handle*, not the contents.
//!
//! # Concurrency
//!
//! Storage is `Arc<RwLock<..>>` rather than `Rc<RefCell<..>>` because `Variant`
//! must stay `Send + Sync`: the editor holds engine state in an
//! `Arc<Mutex<..>>` shared across `thread::spawn`. Every accessor takes the
//! lock for the shortest possible window and hands back owned values, so no
//! lock is ever held across a call back into user code.
//!
//! # Recursion
//!
//! An array may contain itself. Godot guards every recursive operation with
//! `MAX_RECURSION` (`core/typedefs.h:380` — 100) and short-circuits equality on
//! pointer identity (`array.cpp` `recursive_equal`: `if (_p == p_array._p)
//! return true;`). Both guards are reproduced here. In addition, every
//! recursive walk *copies the element list out from behind the lock before
//! recursing*, so a cyclic structure can never make one thread take the same
//! `RwLock` twice.

use std::sync::{Arc, RwLock};

use crate::dictionary::Dictionary;
use crate::hash::{godot_hash, hash_fmix32, hash_murmur3_one_32, HASH_MURMUR3_SEED};
use crate::variant::{Variant, VariantType};

/// Godot's recursion cap for container operations (`core/typedefs.h:380`).
const MAX_RECURSION: u32 = 100;

/// Godot's `Variant::Type` ordinal for `ARRAY`.
const VARIANT_TYPE_ARRAY: u32 = 28;

/// Godot's `Variant::Type` ordinal for `DICTIONARY`.
const VARIANT_TYPE_DICTIONARY: u32 = 27;

/// The element-type constraint on a typed container (`Array[int]`,
/// `Dictionary[String, Node]`), mirroring Godot's `ContainerType`
/// (`core/variant/container_type_validate.h:36`).
///
/// An untyped container has `builtin == None`; that is Godot's `Variant::NIL`
/// sentinel, and it is what `is_typed()` tests.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ContainerType {
    /// The builtin type every element must have, or `None` for untyped.
    pub builtin: Option<VariantType>,
    /// For `builtin == Some(VariantType::ObjectId)`, the required class name.
    /// Empty otherwise — Godot rejects a class name on a non-OBJECT type.
    pub class_name: String,
    /// Optional script that further narrows `class_name`. Godot rejects a
    /// script without a class name.
    pub script: Option<Box<Variant>>,
}

impl ContainerType {
    /// An untyped constraint — accepts any element.
    pub fn untyped() -> Self {
        Self::default()
    }

    /// A constraint on a builtin type, e.g. `Array[int]`.
    pub fn builtin(ty: VariantType) -> Self {
        Self {
            builtin: Some(ty),
            class_name: String::new(),
            script: None,
        }
    }

    /// A constraint on an engine class, e.g. `Array[Node2D]`.
    pub fn object(class_name: impl Into<String>) -> Self {
        Self {
            builtin: Some(VariantType::ObjectId),
            class_name: class_name.into(),
            script: None,
        }
    }

    /// Returns `true` if this constraint actually restricts anything
    /// (Godot `is_typed`: `_p->typed.type != Variant::NIL`).
    pub fn is_typed(&self) -> bool {
        self.builtin.is_some()
    }

    /// Returns `true` if `value` satisfies this constraint.
    ///
    /// Untyped constraints accept everything. `Nil` is always accepted for an
    /// object-typed container (a typed array of objects may hold nulls), which
    /// is what `ContainerTypeValidate::validate` allows.
    pub fn accepts(&self, value: &Variant) -> bool {
        let Some(want) = self.builtin else {
            return true;
        };
        if want == VariantType::ObjectId {
            return matches!(value, Variant::Nil | Variant::ObjectId(_));
        }
        value.variant_type() == want
    }
}

/// Why a container write was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContainerError {
    /// The container was made read-only via `make_read_only()`.
    ReadOnly,
    /// A value did not satisfy the container's element type.
    TypeMismatch {
        /// Human-readable description of the expected type.
        expected: String,
        /// Human-readable description of the value that was rejected.
        got: String,
    },
    /// `set_typed` was called on a container that cannot accept a type.
    CannotSetType(&'static str),
}

impl std::fmt::Display for ContainerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ReadOnly => write!(f, "container is in read-only state"),
            Self::TypeMismatch { expected, got } => {
                write!(f, "expected element of type {expected}, got {got}")
            }
            Self::CannotSetType(why) => write!(f, "cannot set element type: {why}"),
        }
    }
}

impl std::error::Error for ContainerError {}

/// Formats a [`ContainerType`] the way Godot names it in error messages.
fn describe_type(t: &ContainerType) -> String {
    match (t.builtin, t.class_name.as_str()) {
        (None, _) => "Variant".to_string(),
        (Some(VariantType::ObjectId), name) if !name.is_empty() => name.to_string(),
        (Some(ty), _) => format!("{ty:?}"),
    }
}

// ---------------------------------------------------------------------------
// SharedArray
// ---------------------------------------------------------------------------

/// The contents behind a [`SharedArray`] handle. Mirrors Godot's
/// `ArrayPrivate` (`core/variant/array.cpp`).
#[derive(Debug, Default)]
struct ArrayBody {
    items: Vec<Variant>,
    typed: ContainerType,
    read_only: bool,
}

/// A Godot `Array`: a handle to shared, mutable storage.
///
/// Cloning shares storage — that is the whole point. Use
/// [`duplicate`](Self::duplicate) to get an independent array, which is what
/// GDScript's `Array.duplicate()` does.
///
/// ```
/// use gdvariant::shared::SharedArray;
/// use gdvariant::Variant;
///
/// let a = SharedArray::new();
/// let b = a.clone();          // shares storage, like `var b = a`
/// b.push(Variant::Int(1)).unwrap();
/// assert_eq!(a.to_vec(), vec![Variant::Int(1)]);
///
/// let c = a.duplicate(false); // independent copy
/// c.push(Variant::Int(2)).unwrap();
/// assert_eq!(a.len(), 1);
/// ```
#[derive(Clone, Default)]
pub struct SharedArray {
    inner: Arc<RwLock<ArrayBody>>,
}

impl SharedArray {
    /// Creates an empty, untyped, writable array.
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates an array holding `items`.
    pub fn from_vec(items: Vec<Variant>) -> Self {
        Self {
            inner: Arc::new(RwLock::new(ArrayBody {
                items,
                typed: ContainerType::untyped(),
                read_only: false,
            })),
        }
    }

    /// Creates an empty array constrained to `typed`.
    pub fn typed(typed: ContainerType) -> Self {
        Self {
            inner: Arc::new(RwLock::new(ArrayBody {
                items: Vec::new(),
                typed,
                read_only: false,
            })),
        }
    }

    /// Runs `f` against the element slice while holding the read lock.
    ///
    /// `f` must not touch this array again — doing so would re-enter the lock.
    /// Prefer [`to_vec`](Self::to_vec) when in doubt.
    pub fn with<R>(&self, f: impl FnOnce(&[Variant]) -> R) -> R {
        let body = self.inner.read().expect("SharedArray lock poisoned");
        f(body.items.as_slice())
    }

    /// Runs `f` against the element vector while holding the write lock,
    /// refusing if the array is read-only.
    ///
    /// This bypasses the element-type check, so it is the right tool for
    /// internal rearrangements (sort, reverse, shuffle) and the wrong one for
    /// inserting caller-supplied values — use [`push`](Self::push),
    /// [`insert`](Self::insert) or [`set`](Self::set) for those.
    pub fn with_mut<R>(&self, f: impl FnOnce(&mut Vec<Variant>) -> R) -> Result<R, ContainerError> {
        let mut body = self.inner.write().expect("SharedArray lock poisoned");
        if body.read_only {
            return Err(ContainerError::ReadOnly);
        }
        Ok(f(&mut body.items))
    }

    /// Copies the elements out. Nested containers stay shared (their handles
    /// are cloned), matching a Godot shallow copy.
    pub fn to_vec(&self) -> Vec<Variant> {
        self.with(|items| items.to_vec())
    }

    /// Number of elements.
    pub fn len(&self) -> usize {
        self.with(|items| items.len())
    }

    /// Returns `true` if the array has no elements.
    pub fn is_empty(&self) -> bool {
        self.with(|items| items.is_empty())
    }

    /// Returns a copy of the element at `index`, or `None` if out of range.
    pub fn get(&self, index: usize) -> Option<Variant> {
        self.with(|items| items.get(index).cloned())
    }

    /// Appends `value` (Godot `append`/`push_back`).
    pub fn push(&self, value: Variant) -> Result<(), ContainerError> {
        let mut body = self.inner.write().expect("SharedArray lock poisoned");
        Self::check(&body, &value)?;
        body.items.push(value);
        Ok(())
    }

    /// Inserts `value` at `index`, clamped to the end (Godot `insert`).
    pub fn insert(&self, index: usize, value: Variant) -> Result<(), ContainerError> {
        let mut body = self.inner.write().expect("SharedArray lock poisoned");
        Self::check(&body, &value)?;
        let at = index.min(body.items.len());
        body.items.insert(at, value);
        Ok(())
    }

    /// Overwrites the element at `index`. Out-of-range indices are refused with
    /// `Ok(false)` rather than an error, matching Godot's bounds check.
    pub fn set(&self, index: usize, value: Variant) -> Result<bool, ContainerError> {
        let mut body = self.inner.write().expect("SharedArray lock poisoned");
        Self::check(&body, &value)?;
        match body.items.get_mut(index) {
            Some(slot) => {
                *slot = value;
                Ok(true)
            }
            None => Ok(false),
        }
    }

    /// Removes and returns the last element (Godot `pop_back`).
    pub fn pop(&self) -> Result<Option<Variant>, ContainerError> {
        self.with_mut(|items| items.pop())
    }

    /// Removes every element (Godot `clear`).
    pub fn clear(&self) -> Result<(), ContainerError> {
        self.with_mut(|items| items.clear())
    }

    /// Refuses `value` if the array is read-only or `value` does not satisfy
    /// the element type. Callers must already hold the write lock.
    fn check(body: &ArrayBody, value: &Variant) -> Result<(), ContainerError> {
        if body.read_only {
            return Err(ContainerError::ReadOnly);
        }
        if !body.typed.accepts(value) {
            return Err(ContainerError::TypeMismatch {
                expected: describe_type(&body.typed),
                got: format!("{:?}", value.variant_type()),
            });
        }
        Ok(())
    }

    // -- identity ----------------------------------------------------------

    /// Returns `true` if both handles point at the same storage (Godot
    /// `is_same_instance`). This is the operator that distinguishes `var b = a`
    /// from `var b = a.duplicate()`.
    pub fn is_same_instance(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }

    // -- typed containers --------------------------------------------------

    /// Returns `true` if an element type is set (Godot `is_typed`).
    pub fn is_typed(&self) -> bool {
        self.with_body(|b| b.typed.is_typed())
    }

    /// The required builtin element type, or `None` when untyped (Godot
    /// `get_typed_builtin`).
    pub fn typed_builtin(&self) -> Option<VariantType> {
        self.with_body(|b| b.typed.builtin)
    }

    /// The required class name for object-typed arrays, empty otherwise
    /// (Godot `get_typed_class_name`).
    pub fn typed_class_name(&self) -> String {
        self.with_body(|b| b.typed.class_name.clone())
    }

    /// The script narrowing the element class, if any (Godot
    /// `get_typed_script`).
    pub fn typed_script(&self) -> Variant {
        self.with_body(|b| {
            b.typed
                .script
                .as_ref()
                .map(|s| (**s).clone())
                .unwrap_or(Variant::Nil)
        })
    }

    /// Returns `true` if both arrays carry the same element type (Godot
    /// `is_same_typed`).
    pub fn is_same_typed(&self, other: &Self) -> bool {
        if self.is_same_instance(other) {
            return true;
        }
        let mine = self.with_body(|b| b.typed.clone());
        let theirs = other.with_body(|b| b.typed.clone());
        mine == theirs
    }

    /// Applies an element type.
    ///
    /// Godot (`array.cpp` `set_typed`) refuses when the array is read-only,
    /// non-empty, or already typed; the refcount check is not reproduced
    /// because Rust callers hold handles, not raw pointers.
    pub fn set_typed(&self, typed: ContainerType) -> Result<(), ContainerError> {
        let mut body = self.inner.write().expect("SharedArray lock poisoned");
        if body.read_only {
            return Err(ContainerError::ReadOnly);
        }
        if !body.items.is_empty() {
            return Err(ContainerError::CannotSetType(
                "type can only be set when the array is empty",
            ));
        }
        if body.typed.is_typed() {
            return Err(ContainerError::CannotSetType("type can only be set once"));
        }
        if !typed.class_name.is_empty() && typed.builtin != Some(VariantType::ObjectId) {
            return Err(ContainerError::CannotSetType(
                "class names can only be set for object types",
            ));
        }
        if typed.script.is_some() && typed.class_name.is_empty() {
            return Err(ContainerError::CannotSetType(
                "a script class needs a base class name",
            ));
        }
        body.typed = typed;
        Ok(())
    }

    /// Replaces this array's contents with `source`'s, validating every element
    /// against this array's type (Godot `assign`).
    ///
    /// Unlike `clone()`, the *handle* is untouched: everyone holding this array
    /// observes the new contents.
    pub fn assign(&self, source: &Self) -> Result<(), ContainerError> {
        // Snapshot first so a self-assign, or an assign between two handles to
        // the same storage, cannot take the same lock twice.
        let incoming = source.to_vec();
        let mut body = self.inner.write().expect("SharedArray lock poisoned");
        if body.read_only {
            return Err(ContainerError::ReadOnly);
        }
        for value in &incoming {
            if !body.typed.accepts(value) {
                return Err(ContainerError::TypeMismatch {
                    expected: describe_type(&body.typed),
                    got: format!("{:?}", value.variant_type()),
                });
            }
        }
        body.items = incoming;
        Ok(())
    }

    // -- read-only ---------------------------------------------------------

    /// Freezes the array; further writes fail with [`ContainerError::ReadOnly`]
    /// (Godot `make_read_only`). Godot has no way to undo this, and neither
    /// does this type.
    pub fn make_read_only(&self) {
        self.inner
            .write()
            .expect("SharedArray lock poisoned")
            .read_only = true;
    }

    /// Returns `true` if the array has been frozen (Godot `is_read_only`).
    pub fn is_read_only(&self) -> bool {
        self.with_body(|b| b.read_only)
    }

    // -- copies ------------------------------------------------------------

    /// Returns a **new handle** over a copy of the contents (Godot
    /// `duplicate`). With `deep`, nested arrays and dictionaries are copied
    /// too; otherwise nested containers stay shared.
    ///
    /// The element type carries over (`array.cpp` `recursive_duplicate` copies
    /// `_p->typed`), but the read-only flag does not — Godot's duplicate is
    /// writable.
    pub fn duplicate(&self, deep: bool) -> Self {
        self.duplicate_rec(deep, 0)
    }

    /// Godot's `duplicate_deep` — a deep copy, spelled out as its own method.
    pub fn duplicate_deep(&self) -> Self {
        self.duplicate(true)
    }

    fn duplicate_rec(&self, deep: bool, depth: u32) -> Self {
        let typed = self.with_body(|b| b.typed.clone());
        if depth > MAX_RECURSION {
            // Godot prints "Max recursion reached" and returns the empty array,
            // typed but unpopulated.
            return Self::typed(typed);
        }
        // Copy the elements out before recursing so a cyclic array never makes
        // this thread take the same lock twice.
        let items = self.to_vec();
        let items = if deep {
            items
                .iter()
                .map(|v| deep_copy_variant(v, depth + 1))
                .collect()
        } else {
            items
        };
        Self {
            inner: Arc::new(RwLock::new(ArrayBody {
                items,
                typed,
                read_only: false,
            })),
        }
    }

    // -- equality / hashing ------------------------------------------------

    /// Godot's `recursive_equal`: pointer identity first, then a depth-limited
    /// structural compare.
    pub fn recursive_equal(&self, other: &Self, depth: u32) -> bool {
        if self.is_same_instance(other) {
            return true;
        }
        if depth > MAX_RECURSION {
            // Godot returns `true` here, treating "too deep to tell" as equal.
            return true;
        }
        let mine = self.to_vec();
        let theirs = other.to_vec();
        if mine.len() != theirs.len() {
            return false;
        }
        mine.iter()
            .zip(theirs.iter())
            .all(|(a, b)| variant_recursive_equal(a, b, depth + 1))
    }

    /// Godot's `Array::hash()` — the murmur3 chain seeded with the ARRAY
    /// ordinal so `[]` and `{}` differ.
    pub fn hash(&self) -> u32 {
        self.recursive_hash(0)
    }

    fn recursive_hash(&self, depth: u32) -> u32 {
        if depth > MAX_RECURSION {
            return 0;
        }
        let items = self.to_vec();
        let mut h = hash_murmur3_one_32(VARIANT_TYPE_ARRAY, HASH_MURMUR3_SEED);
        for item in &items {
            h = hash_murmur3_one_32(variant_recursive_hash(item, depth + 1), h);
        }
        hash_fmix32(h)
    }

    fn with_body<R>(&self, f: impl FnOnce(&ArrayBody) -> R) -> R {
        let body = self.inner.read().expect("SharedArray lock poisoned");
        f(&*body)
    }
}

impl PartialEq for SharedArray {
    /// Structural (deep) equality, matching Godot's `Array::operator==` which
    /// forwards to `recursive_equal`. Two *different* arrays with equal
    /// contents compare equal; use [`is_same_instance`](Self::is_same_instance)
    /// to ask about identity instead.
    fn eq(&self, other: &Self) -> bool {
        self.recursive_equal(other, 0)
    }
}

impl std::fmt::Debug for SharedArray {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Depth-limited via the borrowed snapshot: nested SharedArrays format
        // through this same impl, and a cycle would recurse forever, so cap it
        // the way Godot's `Variant::stringify` does.
        f.debug_list().entries(self.to_vec()).finish()
    }
}

impl From<Vec<Variant>> for SharedArray {
    fn from(items: Vec<Variant>) -> Self {
        Self::from_vec(items)
    }
}

impl FromIterator<Variant> for SharedArray {
    fn from_iter<I: IntoIterator<Item = Variant>>(iter: I) -> Self {
        Self::from_vec(iter.into_iter().collect())
    }
}

// ---------------------------------------------------------------------------
// SharedDictionary
// ---------------------------------------------------------------------------

/// The contents behind a [`SharedDictionary`] handle, mirroring Godot's
/// `DictionaryPrivate`.
#[derive(Debug, Default)]
struct DictBody {
    map: Dictionary,
    key_type: ContainerType,
    value_type: ContainerType,
    read_only: bool,
}

/// A Godot `Dictionary`: a handle to shared, mutable storage.
///
/// Same contract as [`SharedArray`] — clone shares, `duplicate` copies — with
/// Godot 4.4's typed-key/typed-value pair in place of a single element type.
#[derive(Clone, Default)]
pub struct SharedDictionary {
    inner: Arc<RwLock<DictBody>>,
}

impl SharedDictionary {
    /// Creates an empty, untyped, writable dictionary.
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a dictionary holding `map`.
    pub fn from_dict(map: Dictionary) -> Self {
        Self {
            inner: Arc::new(RwLock::new(DictBody {
                map,
                ..DictBody::default()
            })),
        }
    }

    /// Runs `f` against the map while holding the read lock.
    pub fn with<R>(&self, f: impl FnOnce(&Dictionary) -> R) -> R {
        let body = self.inner.read().expect("SharedDictionary lock poisoned");
        f(&body.map)
    }

    /// Runs `f` against the map while holding the write lock, refusing if the
    /// dictionary is read-only.
    ///
    /// Bypasses the key/value type checks — use [`insert`](Self::insert) for
    /// caller-supplied entries.
    pub fn with_mut<R>(&self, f: impl FnOnce(&mut Dictionary) -> R) -> Result<R, ContainerError> {
        let mut body = self.inner.write().expect("SharedDictionary lock poisoned");
        if body.read_only {
            return Err(ContainerError::ReadOnly);
        }
        Ok(f(&mut body.map))
    }

    /// Copies the map out. Nested containers stay shared.
    pub fn to_dict(&self) -> Dictionary {
        self.with(|m| m.clone())
    }

    /// Number of entries.
    pub fn len(&self) -> usize {
        self.with(|m| m.len())
    }

    /// Returns `true` if the dictionary has no entries.
    pub fn is_empty(&self) -> bool {
        self.with(|m| m.is_empty())
    }

    /// Returns a copy of the value stored for `key`.
    ///
    /// Returns an owned `Variant` rather than `&Variant` because the value
    /// lives behind a lock. That is not a value copy of the whole subtree:
    /// once `Variant::Array` holds a [`SharedArray`], a nested container comes
    /// back as the same handle, so `d.get("list")` then `push` is visible
    /// through `d`.
    pub fn get(&self, key: impl Into<Variant>) -> Option<Variant> {
        let key = key.into();
        self.with(|m| m.get(&key).cloned())
    }

    /// Returns `true` if `key` is present.
    pub fn contains_key(&self, key: impl Into<Variant>) -> bool {
        let key = key.into();
        self.with(|m| m.contains_key(&key))
    }

    /// Inserts `key`/`value`, returning the previous value if the key existed.
    pub fn insert(
        &self,
        key: impl Into<Variant>,
        value: impl Into<Variant>,
    ) -> Result<Option<Variant>, ContainerError> {
        let key = key.into();
        let value = value.into();
        let mut body = self.inner.write().expect("SharedDictionary lock poisoned");
        if body.read_only {
            return Err(ContainerError::ReadOnly);
        }
        if !body.key_type.accepts(&key) {
            return Err(ContainerError::TypeMismatch {
                expected: describe_type(&body.key_type),
                got: format!("{:?}", key.variant_type()),
            });
        }
        if !body.value_type.accepts(&value) {
            return Err(ContainerError::TypeMismatch {
                expected: describe_type(&body.value_type),
                got: format!("{:?}", value.variant_type()),
            });
        }
        Ok(body.map.insert(key, value))
    }

    /// Removes `key`, returning its value if present (Godot `erase`).
    pub fn remove(&self, key: impl Into<Variant>) -> Result<Option<Variant>, ContainerError> {
        let key = key.into();
        self.with_mut(|m| m.remove(key))
    }

    /// Removes every entry.
    pub fn clear(&self) -> Result<(), ContainerError> {
        self.with_mut(|m| m.clear())
    }

    // -- identity ----------------------------------------------------------

    /// Returns `true` if both handles point at the same storage (Godot
    /// `is_same_instance`).
    pub fn is_same_instance(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }

    // -- typed containers --------------------------------------------------

    /// Returns `true` if either the key or the value type is set. Godot's
    /// `Dictionary::is_typed` is `is_typed_key() || is_typed_value()`.
    pub fn is_typed(&self) -> bool {
        self.is_typed_key() || self.is_typed_value()
    }

    /// Returns `true` if a key type is set (Godot `is_typed_key`).
    pub fn is_typed_key(&self) -> bool {
        self.with_body(|b| b.key_type.is_typed())
    }

    /// Returns `true` if a value type is set (Godot `is_typed_value`).
    pub fn is_typed_value(&self) -> bool {
        self.with_body(|b| b.value_type.is_typed())
    }

    /// The required builtin key type (Godot `get_typed_key_builtin`).
    pub fn typed_key_builtin(&self) -> Option<VariantType> {
        self.with_body(|b| b.key_type.builtin)
    }

    /// The required builtin value type (Godot `get_typed_value_builtin`).
    pub fn typed_value_builtin(&self) -> Option<VariantType> {
        self.with_body(|b| b.value_type.builtin)
    }

    /// The required key class name, empty when not object-typed.
    pub fn typed_key_class_name(&self) -> String {
        self.with_body(|b| b.key_type.class_name.clone())
    }

    /// The required value class name, empty when not object-typed.
    pub fn typed_value_class_name(&self) -> String {
        self.with_body(|b| b.value_type.class_name.clone())
    }

    /// Returns `true` if both dictionaries carry the same key *and* value type
    /// (Godot `is_same_typed`).
    pub fn is_same_typed(&self, other: &Self) -> bool {
        self.is_same_typed_key(other) && self.is_same_typed_value(other)
    }

    /// Returns `true` if both dictionaries carry the same key type.
    pub fn is_same_typed_key(&self, other: &Self) -> bool {
        if self.is_same_instance(other) {
            return true;
        }
        self.with_body(|b| b.key_type.clone()) == other.with_body(|b| b.key_type.clone())
    }

    /// Returns `true` if both dictionaries carry the same value type.
    pub fn is_same_typed_value(&self, other: &Self) -> bool {
        if self.is_same_instance(other) {
            return true;
        }
        self.with_body(|b| b.value_type.clone()) == other.with_body(|b| b.value_type.clone())
    }

    /// Applies key and value types. Same refusals as
    /// [`SharedArray::set_typed`].
    pub fn set_typed(
        &self,
        key_type: ContainerType,
        value_type: ContainerType,
    ) -> Result<(), ContainerError> {
        let mut body = self.inner.write().expect("SharedDictionary lock poisoned");
        if body.read_only {
            return Err(ContainerError::ReadOnly);
        }
        if !body.map.is_empty() {
            return Err(ContainerError::CannotSetType(
                "type can only be set when the dictionary is empty",
            ));
        }
        if body.key_type.is_typed() || body.value_type.is_typed() {
            return Err(ContainerError::CannotSetType("type can only be set once"));
        }
        body.key_type = key_type;
        body.value_type = value_type;
        Ok(())
    }

    /// Replaces this dictionary's contents with `source`'s, validating every
    /// entry against this dictionary's types (Godot `assign`). The handle is
    /// untouched, so every holder sees the new contents.
    pub fn assign(&self, source: &Self) -> Result<(), ContainerError> {
        let incoming = source.to_dict();
        let mut body = self.inner.write().expect("SharedDictionary lock poisoned");
        if body.read_only {
            return Err(ContainerError::ReadOnly);
        }
        for (k, v) in incoming.iter() {
            if !body.key_type.accepts(k) {
                return Err(ContainerError::TypeMismatch {
                    expected: describe_type(&body.key_type),
                    got: format!("{:?}", k.variant_type()),
                });
            }
            if !body.value_type.accepts(v) {
                return Err(ContainerError::TypeMismatch {
                    expected: describe_type(&body.value_type),
                    got: format!("{:?}", v.variant_type()),
                });
            }
        }
        body.map = incoming;
        Ok(())
    }

    // -- read-only ---------------------------------------------------------

    /// Freezes the dictionary (Godot `make_read_only`).
    pub fn make_read_only(&self) {
        self.inner
            .write()
            .expect("SharedDictionary lock poisoned")
            .read_only = true;
    }

    /// Returns `true` if the dictionary has been frozen.
    pub fn is_read_only(&self) -> bool {
        self.with_body(|b| b.read_only)
    }

    // -- copies ------------------------------------------------------------

    /// Returns a **new handle** over a copy of the contents (Godot
    /// `duplicate`). Key and value types carry over; the read-only flag does
    /// not.
    pub fn duplicate(&self, deep: bool) -> Self {
        self.duplicate_rec(deep, 0)
    }

    /// Godot's `duplicate_deep`.
    pub fn duplicate_deep(&self) -> Self {
        self.duplicate(true)
    }

    fn duplicate_rec(&self, deep: bool, depth: u32) -> Self {
        let (key_type, value_type) = self.with_body(|b| (b.key_type.clone(), b.value_type.clone()));
        let map = if depth > MAX_RECURSION {
            Dictionary::new()
        } else {
            let snapshot = self.to_dict();
            if deep {
                let mut out = Dictionary::with_capacity(snapshot.len());
                for (k, v) in snapshot.iter() {
                    out.insert(k.clone(), deep_copy_variant(v, depth + 1));
                }
                out
            } else {
                snapshot
            }
        };
        Self {
            inner: Arc::new(RwLock::new(DictBody {
                map,
                key_type,
                value_type,
                read_only: false,
            })),
        }
    }

    // -- equality / hashing ------------------------------------------------

    /// Godot's `recursive_equal`: pointer identity first, then an
    /// order-independent, depth-limited structural compare.
    pub fn recursive_equal(&self, other: &Self, depth: u32) -> bool {
        if self.is_same_instance(other) {
            return true;
        }
        if depth > MAX_RECURSION {
            return true;
        }
        let mine = self.to_dict();
        let theirs = other.to_dict();
        if mine.len() != theirs.len() {
            return false;
        }
        // An explicit loop, not `mine.iter().all(..)` in tail position: a tail
        // expression's temporaries are dropped *after* the block's locals, and
        // `Dictionary::iter()` returns an opaque `impl Iterator` borrowing
        // `mine`, so the iterator would outlive `mine` (E0597).
        for (k, v) in mine.iter() {
            match theirs.get(k) {
                Some(other_v) if variant_recursive_equal(v, other_v, depth + 1) => {}
                _ => return false,
            }
        }
        true
    }

    /// Godot's `Dictionary::hash()`.
    pub fn hash(&self) -> u32 {
        self.recursive_hash(0)
    }

    fn recursive_hash(&self, depth: u32) -> u32 {
        if depth > MAX_RECURSION {
            return 0;
        }
        let map = self.to_dict();
        let mut h = hash_murmur3_one_32(VARIANT_TYPE_DICTIONARY, HASH_MURMUR3_SEED);
        for (k, v) in map.iter() {
            h = hash_murmur3_one_32(variant_recursive_hash(k, depth + 1), h);
            h = hash_murmur3_one_32(variant_recursive_hash(v, depth + 1), h);
        }
        hash_fmix32(h)
    }

    fn with_body<R>(&self, f: impl FnOnce(&DictBody) -> R) -> R {
        let body = self.inner.read().expect("SharedDictionary lock poisoned");
        f(&*body)
    }
}

impl PartialEq for SharedDictionary {
    fn eq(&self, other: &Self) -> bool {
        self.recursive_equal(other, 0)
    }
}

impl std::fmt::Debug for SharedDictionary {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(&self.to_dict(), f)
    }
}

impl From<Dictionary> for SharedDictionary {
    fn from(map: Dictionary) -> Self {
        Self::from_dict(map)
    }
}

// ---------------------------------------------------------------------------
// Recursive Variant helpers
// ---------------------------------------------------------------------------

/// Deep-copies a `Variant`, giving nested containers fresh storage.
///
/// Once `Variant::Array`/`Variant::Dictionary` hold [`SharedArray`] /
/// [`SharedDictionary`], this is the branch that makes `duplicate(true)`
/// produce a tree nobody else can reach. Until then it defers to the existing
/// value-typed payloads, so it is correct either way.
fn deep_copy_variant(v: &Variant, depth: u32) -> Variant {
    if depth > MAX_RECURSION {
        return Variant::Nil;
    }
    match v {
        Variant::Array(items) => Variant::Array(
            items
                .iter()
                .map(|item| deep_copy_variant(item, depth + 1))
                .collect(),
        ),
        Variant::Dictionary(d) => Variant::Dictionary(d.duplicate(true)),
        other => other.clone(),
    }
}

/// Depth-limited structural equality for `Variant`, so container compares can
/// bound their own recursion the way Godot's `hash_compare` does.
fn variant_recursive_equal(a: &Variant, b: &Variant, depth: u32) -> bool {
    if depth > MAX_RECURSION {
        return true;
    }
    match (a, b) {
        (Variant::Array(x), Variant::Array(y)) => {
            x.len() == y.len()
                && x.iter()
                    .zip(y.iter())
                    .all(|(l, r)| variant_recursive_equal(l, r, depth + 1))
        }
        _ => a == b,
    }
}

/// Depth-limited hash for `Variant`, delegating to the crate's Godot-identical
/// [`godot_hash`] for leaves.
fn variant_recursive_hash(v: &Variant, depth: u32) -> u32 {
    if depth > MAX_RECURSION {
        return 0;
    }
    godot_hash(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- the headline behaviour: aliasing ---------------------------------

    #[test]
    fn clone_shares_storage() {
        // `var a = []` / `var b = a` / `b.append(1)` → `a` is `[1]`.
        let a = SharedArray::new();
        let b = a.clone();
        b.push(Variant::Int(1)).unwrap();
        assert_eq!(a.to_vec(), vec![Variant::Int(1)]);
        assert!(a.is_same_instance(&b));
    }

    #[test]
    fn mutation_through_a_function_parameter_is_visible_to_the_caller() {
        // The exact shape the interpreter's write-back hack breaks today.
        fn helper(param: SharedArray) {
            param.push(Variant::Int(7)).unwrap();
        }
        let caller = SharedArray::from_vec(vec![Variant::Int(1)]);
        helper(caller.clone());
        assert_eq!(caller.to_vec(), vec![Variant::Int(1), Variant::Int(7)]);
    }

    #[test]
    fn dictionary_clone_shares_storage() {
        let a = SharedDictionary::new();
        let b = a.clone();
        b.insert(Variant::String("k".into()), Variant::Int(1))
            .unwrap();
        assert_eq!(a.get(Variant::String("k".into())), Some(Variant::Int(1)));
        assert!(a.is_same_instance(&b));
    }

    #[test]
    fn duplicate_breaks_the_alias() {
        let a = SharedArray::from_vec(vec![Variant::Int(1)]);
        let copy = a.duplicate(false);
        copy.push(Variant::Int(2)).unwrap();
        assert_eq!(
            a.len(),
            1,
            "duplicate() must not write through to the original"
        );
        assert!(!a.is_same_instance(&copy));
    }

    #[test]
    fn duplicate_copies_contents_and_leaves_the_source_alone() {
        // Godot's duplicate(false) copies the outer array. Nested *shared*
        // containers cannot be expressed yet — `Variant::Array` still holds a
        // `Vec<Variant>`, not a `SharedArray` — so the nested-aliasing half of
        // Godot's contract is only testable once the enum payload flips.
        let source = SharedArray::from_vec(vec![Variant::Int(1), Variant::Int(2)]);
        let copy = source.duplicate(false);
        assert_eq!(copy.to_vec(), source.to_vec());
        assert!(!source.is_same_instance(&copy));
        copy.clear().unwrap();
        assert_eq!(
            source.len(),
            2,
            "clearing the copy must not touch the source"
        );
    }

    #[test]
    fn dictionary_duplicate_breaks_the_alias() {
        let a = SharedDictionary::new();
        a.insert(Variant::String("k".into()), Variant::Int(1))
            .unwrap();
        let copy = a.duplicate(false);
        copy.insert(Variant::String("k2".into()), Variant::Int(2))
            .unwrap();
        assert_eq!(a.len(), 1);
        assert_eq!(copy.len(), 2);
    }

    // -- equality ----------------------------------------------------------

    #[test]
    fn equality_is_structural_not_identity() {
        // Godot's Array::operator== forwards to recursive_equal.
        let a = SharedArray::from_vec(vec![Variant::Int(1), Variant::Int(2)]);
        let b = SharedArray::from_vec(vec![Variant::Int(1), Variant::Int(2)]);
        assert_eq!(a, b);
        assert!(!a.is_same_instance(&b));

        let c = SharedArray::from_vec(vec![Variant::Int(1)]);
        assert_ne!(a, c);
    }

    #[test]
    fn self_equality_does_not_deadlock() {
        // Two read locks on one RwLock from one thread is a documented hazard;
        // the ptr_eq fast path must catch it before the compare starts.
        let a = SharedArray::from_vec(vec![Variant::Int(1)]);
        let alias = a.clone();
        assert_eq!(a, alias);
        assert!(a.recursive_equal(&alias, 0));
    }

    #[test]
    fn dictionary_equality_is_order_independent() {
        let a = SharedDictionary::new();
        a.insert(Variant::String("x".into()), Variant::Int(1))
            .unwrap();
        a.insert(Variant::String("y".into()), Variant::Int(2))
            .unwrap();
        let b = SharedDictionary::new();
        b.insert(Variant::String("y".into()), Variant::Int(2))
            .unwrap();
        b.insert(Variant::String("x".into()), Variant::Int(1))
            .unwrap();
        assert_eq!(a, b);
    }

    // -- typed containers --------------------------------------------------

    #[test]
    fn untyped_array_reports_untyped() {
        let a = SharedArray::new();
        assert!(!a.is_typed());
        assert_eq!(a.typed_builtin(), None);
        assert_eq!(a.typed_class_name(), "");
        assert_eq!(a.typed_script(), Variant::Nil);
    }

    #[test]
    fn typed_array_rejects_the_wrong_element_type() {
        let a = SharedArray::new();
        a.set_typed(ContainerType::builtin(VariantType::Int))
            .unwrap();
        assert!(a.is_typed());
        assert_eq!(a.typed_builtin(), Some(VariantType::Int));

        a.push(Variant::Int(1)).unwrap();
        let err = a.push(Variant::String("nope".into())).unwrap_err();
        assert!(matches!(err, ContainerError::TypeMismatch { .. }));
        assert_eq!(a.len(), 1, "the rejected push must not land");
    }

    #[test]
    fn set_typed_refuses_a_nonempty_array() {
        let a = SharedArray::from_vec(vec![Variant::Int(1)]);
        assert!(matches!(
            a.set_typed(ContainerType::builtin(VariantType::Int)),
            Err(ContainerError::CannotSetType(_))
        ));
    }

    #[test]
    fn set_typed_refuses_a_second_type() {
        let a = SharedArray::new();
        a.set_typed(ContainerType::builtin(VariantType::Int))
            .unwrap();
        assert!(matches!(
            a.set_typed(ContainerType::builtin(VariantType::String)),
            Err(ContainerError::CannotSetType(_))
        ));
    }

    #[test]
    fn set_typed_refuses_a_class_name_on_a_builtin() {
        let a = SharedArray::new();
        let bad = ContainerType {
            builtin: Some(VariantType::Int),
            class_name: "Node2D".into(),
            script: None,
        };
        assert!(matches!(
            a.set_typed(bad),
            Err(ContainerError::CannotSetType(_))
        ));
    }

    #[test]
    fn is_same_typed_compares_the_constraint() {
        let a = SharedArray::new();
        let b = SharedArray::new();
        let c = SharedArray::new();
        a.set_typed(ContainerType::builtin(VariantType::Int))
            .unwrap();
        b.set_typed(ContainerType::builtin(VariantType::Int))
            .unwrap();
        c.set_typed(ContainerType::builtin(VariantType::String))
            .unwrap();
        assert!(a.is_same_typed(&b));
        assert!(!a.is_same_typed(&c));
    }

    #[test]
    fn typed_array_accepts_nil_for_object_elements() {
        // Godot lets a typed array of objects hold nulls.
        let a = SharedArray::new();
        a.set_typed(ContainerType::object("Node2D")).unwrap();
        assert_eq!(a.typed_class_name(), "Node2D");
        a.push(Variant::Nil).unwrap();
        assert_eq!(a.len(), 1);
    }

    #[test]
    fn duplicate_carries_the_element_type() {
        let a = SharedArray::new();
        a.set_typed(ContainerType::builtin(VariantType::Int))
            .unwrap();
        a.push(Variant::Int(1)).unwrap();
        let copy = a.duplicate(false);
        assert!(copy.is_typed());
        assert_eq!(copy.typed_builtin(), Some(VariantType::Int));
    }

    #[test]
    fn typed_dictionary_reports_key_and_value_types() {
        let d = SharedDictionary::new();
        d.set_typed(
            ContainerType::builtin(VariantType::String),
            ContainerType::builtin(VariantType::Int),
        )
        .unwrap();
        assert!(d.is_typed());
        assert!(d.is_typed_key());
        assert!(d.is_typed_value());
        assert_eq!(d.typed_key_builtin(), Some(VariantType::String));
        assert_eq!(d.typed_value_builtin(), Some(VariantType::Int));

        d.insert(Variant::String("a".into()), Variant::Int(1))
            .unwrap();
        assert!(matches!(
            d.insert(Variant::Int(0), Variant::Int(1)),
            Err(ContainerError::TypeMismatch { .. })
        ));
        assert!(matches!(
            d.insert(Variant::String("b".into()), Variant::String("x".into())),
            Err(ContainerError::TypeMismatch { .. })
        ));
        assert_eq!(d.len(), 1);
    }

    #[test]
    fn half_typed_dictionary_is_typed() {
        // Godot's is_typed() is key || value, not key && value.
        let d = SharedDictionary::new();
        d.set_typed(
            ContainerType::untyped(),
            ContainerType::builtin(VariantType::Int),
        )
        .unwrap();
        assert!(d.is_typed());
        assert!(!d.is_typed_key());
        assert!(d.is_typed_value());
    }

    // -- read-only ---------------------------------------------------------

    #[test]
    fn read_only_array_refuses_every_write() {
        let a = SharedArray::from_vec(vec![Variant::Int(1)]);
        assert!(!a.is_read_only());
        a.make_read_only();
        assert!(a.is_read_only());

        assert_eq!(a.push(Variant::Int(2)), Err(ContainerError::ReadOnly));
        assert_eq!(a.insert(0, Variant::Int(2)), Err(ContainerError::ReadOnly));
        assert_eq!(a.set(0, Variant::Int(2)), Err(ContainerError::ReadOnly));
        assert_eq!(a.pop(), Err(ContainerError::ReadOnly));
        assert_eq!(a.clear(), Err(ContainerError::ReadOnly));
        assert_eq!(a.to_vec(), vec![Variant::Int(1)], "contents must survive");
    }

    #[test]
    fn read_only_propagates_through_the_alias() {
        // Freezing is a property of the storage, so every handle sees it.
        let a = SharedArray::new();
        let b = a.clone();
        a.make_read_only();
        assert!(b.is_read_only());
        assert_eq!(b.push(Variant::Int(1)), Err(ContainerError::ReadOnly));
    }

    #[test]
    fn duplicate_of_a_read_only_array_is_writable() {
        let a = SharedArray::from_vec(vec![Variant::Int(1)]);
        a.make_read_only();
        let copy = a.duplicate(false);
        assert!(!copy.is_read_only());
        copy.push(Variant::Int(2)).unwrap();
        assert_eq!(copy.len(), 2);
        assert_eq!(a.len(), 1);
    }

    #[test]
    fn read_only_dictionary_refuses_every_write() {
        let d = SharedDictionary::new();
        d.insert(Variant::String("k".into()), Variant::Int(1))
            .unwrap();
        d.make_read_only();
        assert_eq!(
            d.insert(Variant::String("j".into()), Variant::Int(2)),
            Err(ContainerError::ReadOnly)
        );
        assert_eq!(
            d.remove(Variant::String("k".into())),
            Err(ContainerError::ReadOnly)
        );
        assert_eq!(d.clear(), Err(ContainerError::ReadOnly));
        assert_eq!(d.len(), 1);
    }

    // -- assign ------------------------------------------------------------

    #[test]
    fn assign_replaces_contents_without_replacing_the_handle() {
        // This is what distinguishes `a.assign(b)` from `a = b` in Godot: every
        // holder of `a` sees the new contents.
        let a = SharedArray::from_vec(vec![Variant::Int(1)]);
        let alias = a.clone();
        let source = SharedArray::from_vec(vec![Variant::Int(9), Variant::Int(8)]);

        a.assign(&source).unwrap();
        assert_eq!(alias.to_vec(), vec![Variant::Int(9), Variant::Int(8)]);
        assert!(a.is_same_instance(&alias));
        assert!(!a.is_same_instance(&source));
    }

    #[test]
    fn assign_from_an_alias_does_not_deadlock() {
        let a = SharedArray::from_vec(vec![Variant::Int(1)]);
        let alias = a.clone();
        a.assign(&alias).unwrap();
        assert_eq!(a.to_vec(), vec![Variant::Int(1)]);
    }

    #[test]
    fn assign_enforces_the_destination_type() {
        let dest = SharedArray::new();
        dest.set_typed(ContainerType::builtin(VariantType::Int))
            .unwrap();
        let source = SharedArray::from_vec(vec![Variant::String("x".into())]);
        assert!(matches!(
            dest.assign(&source),
            Err(ContainerError::TypeMismatch { .. })
        ));
        assert!(dest.is_empty(), "a refused assign must not partially land");
    }

    #[test]
    fn assign_refuses_a_read_only_destination() {
        let dest = SharedArray::new();
        dest.make_read_only();
        let source = SharedArray::from_vec(vec![Variant::Int(1)]);
        assert_eq!(dest.assign(&source), Err(ContainerError::ReadOnly));
    }

    #[test]
    fn dictionary_assign_replaces_contents_in_place() {
        let d = SharedDictionary::new();
        d.insert(Variant::String("old".into()), Variant::Int(0))
            .unwrap();
        let alias = d.clone();
        let source = SharedDictionary::new();
        source
            .insert(Variant::String("new".into()), Variant::Int(1))
            .unwrap();

        d.assign(&source).unwrap();
        assert_eq!(alias.len(), 1);
        assert_eq!(
            alias.get(Variant::String("new".into())),
            Some(Variant::Int(1))
        );
        assert_eq!(alias.get(Variant::String("old".into())), None);
    }

    // -- mutators ----------------------------------------------------------

    #[test]
    fn insert_clamps_past_the_end() {
        let a = SharedArray::from_vec(vec![Variant::Int(1)]);
        a.insert(99, Variant::Int(2)).unwrap();
        assert_eq!(a.to_vec(), vec![Variant::Int(1), Variant::Int(2)]);
    }

    #[test]
    fn set_reports_an_out_of_range_index() {
        let a = SharedArray::from_vec(vec![Variant::Int(1)]);
        assert_eq!(a.set(0, Variant::Int(5)), Ok(true));
        assert_eq!(a.set(9, Variant::Int(5)), Ok(false));
        assert_eq!(a.to_vec(), vec![Variant::Int(5)]);
    }

    #[test]
    fn dictionary_get_returns_an_owned_value() {
        // `get` hands back an owned Variant because the value lives behind a
        // lock; mutating that copy must not write through.
        let d = SharedDictionary::new();
        d.insert(Variant::String("n".into()), Variant::Int(0))
            .unwrap();
        let fetched = d.get(Variant::String("n".into())).unwrap();
        // Replacing the entry must not retroactively change the value already
        // read out — that is what "owned" buys over a borrow.
        d.insert(Variant::String("n".into()), Variant::Int(99))
            .unwrap();
        assert_eq!(fetched, Variant::Int(0));
        assert_eq!(d.get(Variant::String("n".into())), Some(Variant::Int(99)));
        assert_eq!(d.get(Variant::String("missing".into())), None);
    }

    // -- deep duplicate ----------------------------------------------------

    #[test]
    fn deep_duplicate_preserves_nested_container_contents() {
        let a = SharedArray::from_vec(vec![Variant::Array(vec![Variant::Int(1)])]);
        let deep = a.duplicate(true);
        assert_eq!(deep.to_vec(), a.to_vec(), "a deep copy must compare equal");
        assert!(!a.is_same_instance(&deep));
    }

    #[test]
    fn duplicate_deep_is_duplicate_true() {
        let a = SharedArray::from_vec(vec![Variant::Int(1)]);
        assert_eq!(a.duplicate_deep().to_vec(), a.duplicate(true).to_vec());
        let d = SharedDictionary::new();
        d.insert(Variant::String("k".into()), Variant::Int(1))
            .unwrap();
        assert_eq!(d.duplicate_deep().len(), d.duplicate(true).len());
    }

    // -- hashing -----------------------------------------------------------

    #[test]
    fn hash_matches_the_crate_hash_for_the_same_contents() {
        // The shared array must hash identically to the value-typed Variant it
        // will replace, or every dictionary keyed on an array changes meaning.
        let items = vec![Variant::Int(1), Variant::Int(2), Variant::Int(3)];
        let shared = SharedArray::from_vec(items.clone());
        assert_eq!(shared.hash(), godot_hash(&Variant::Array(items)));
    }

    #[test]
    fn empty_array_and_empty_dictionary_hash_differently() {
        assert_ne!(SharedArray::new().hash(), SharedDictionary::new().hash());
    }

    #[test]
    fn equal_arrays_hash_equal() {
        let a = SharedArray::from_vec(vec![Variant::Int(1)]);
        let b = SharedArray::from_vec(vec![Variant::Int(1)]);
        assert_eq!(a, b);
        assert_eq!(a.hash(), b.hash());
    }

    // -- thread safety -----------------------------------------------------

    #[test]
    fn shared_containers_are_send_and_sync() {
        // Variant must stay Send + Sync: the editor shares engine state across
        // thread::spawn behind an Arc<Mutex<..>>. Rc/RefCell would break that.
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<SharedArray>();
        assert_send_sync::<SharedDictionary>();
    }

    #[test]
    fn concurrent_pushes_all_land() {
        let a = SharedArray::new();
        let handles: Vec<_> = (0..8)
            .map(|i| {
                let a = a.clone();
                std::thread::spawn(move || {
                    for j in 0..50 {
                        a.push(Variant::Int(i * 50 + j)).unwrap();
                    }
                })
            })
            .collect();
        for h in handles {
            h.join().unwrap();
        }
        assert_eq!(a.len(), 400, "no push may be lost across threads");
    }

    // -- recursion guards --------------------------------------------------

    #[test]
    fn deeply_nested_duplicate_terminates() {
        // Build a chain deeper than MAX_RECURSION and confirm duplicate(true)
        // returns instead of blowing the stack.
        let mut v = Variant::Array(vec![Variant::Int(0)]);
        for _ in 0..(MAX_RECURSION + 20) {
            v = Variant::Array(vec![v]);
        }
        let a = SharedArray::from_vec(vec![v]);
        let _ = a.duplicate(true);
    }
}
