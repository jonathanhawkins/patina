//! Unique identifier types for engine objects.
//!
//! Godot uses integer IDs extensively for objects, resources, and internal
//! bookkeeping. This module provides strongly-typed wrappers so different
//! ID domains cannot be accidentally mixed.

use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};

/// A unique object instance ID, analogous to Godot's `ObjectID`.
///
/// Every object in the runtime receives a unique ID at creation time.
/// IDs are never reused within a single engine session.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct ObjectId(u64);

impl ObjectId {
    /// Generates the next unique object ID.
    pub fn next() -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(1);
        Self(COUNTER.fetch_add(1, Ordering::Relaxed))
    }

    /// Returns the raw numeric value.
    pub fn raw(self) -> u64 {
        self.0
    }

    /// Wraps a raw numeric value. Intended for deserialization and tests.
    pub fn from_raw(v: u64) -> Self {
        Self(v)
    }
}

impl fmt::Debug for ObjectId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ObjectId({})", self.0)
    }
}

impl fmt::Display for ObjectId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// A resource unique ID, analogous to Godot's `ResourceUID`.
///
/// Resources may be identified by path *or* by UID. UIDs survive
/// renames and are the preferred stable identifier.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct ResourceUid(i64);

impl ResourceUid {
    /// Sentinel value meaning "no UID assigned".
    pub const INVALID: Self = Self(-1);

    /// Creates a UID from a raw value.
    pub fn new(v: i64) -> Self {
        Self(v)
    }

    /// Returns the raw numeric value.
    pub fn raw(self) -> i64 {
        self.0
    }

    /// Returns `true` if this UID is the invalid/unassigned sentinel.
    pub fn is_valid(self) -> bool {
        self.0 != Self::INVALID.0
    }
}

impl fmt::Debug for ResourceUid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ResourceUid({})", self.0)
    }
}

impl fmt::Display for ResourceUid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// An opaque identifier for a class registered in the ClassDB.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct ClassId(u32);

impl ClassId {
    /// Creates a class ID from a raw value.
    pub fn new(v: u32) -> Self {
        Self(v)
    }

    /// Returns the raw numeric value.
    pub fn raw(self) -> u32 {
        self.0
    }
}

impl fmt::Debug for ClassId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ClassId({})", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn object_ids_are_unique() {
        let a = ObjectId::next();
        let b = ObjectId::next();
        assert_ne!(a, b);
    }

    #[test]
    fn object_id_roundtrip() {
        let id = ObjectId::from_raw(42);
        assert_eq!(id.raw(), 42);
    }

    #[test]
    fn resource_uid_invalid_sentinel() {
        assert!(!ResourceUid::INVALID.is_valid());
        assert!(ResourceUid::new(0).is_valid());
        assert!(ResourceUid::new(100).is_valid());
    }

    #[test]
    fn object_id_display() {
        let id = ObjectId::from_raw(42);
        assert_eq!(format!("{id}"), "42");
    }

    #[test]
    fn object_id_debug() {
        let id = ObjectId::from_raw(42);
        assert_eq!(format!("{id:?}"), "ObjectId(42)");
    }

    #[test]
    fn object_id_hash_consistent() {
        use std::collections::HashSet;
        let id = ObjectId::from_raw(99);
        let mut set = HashSet::new();
        set.insert(id);
        assert!(set.contains(&ObjectId::from_raw(99)));
        assert!(!set.contains(&ObjectId::from_raw(100)));
    }

    #[test]
    fn object_id_next_always_increases() {
        let a = ObjectId::next();
        let b = ObjectId::next();
        let c = ObjectId::next();
        assert!(a.raw() < b.raw());
        assert!(b.raw() < c.raw());
    }

    #[test]
    fn class_id_display() {
        let id = ClassId::new(7);
        assert_eq!(format!("{id:?}"), "ClassId(7)");
    }

    #[test]
    fn class_id_roundtrip() {
        let id = ClassId::new(123);
        assert_eq!(id.raw(), 123);
    }

    #[test]
    fn class_id_equality() {
        assert_eq!(ClassId::new(5), ClassId::new(5));
        assert_ne!(ClassId::new(5), ClassId::new(6));
    }

    #[test]
    fn resource_uid_display() {
        let uid = ResourceUid::new(42);
        assert_eq!(format!("{uid}"), "42");
        assert_eq!(format!("{uid:?}"), "ResourceUid(42)");
    }

    #[test]
    fn resource_uid_negative_value_is_valid() {
        // -1 is INVALID, but -2 should be valid
        assert!(ResourceUid::new(-2).is_valid());
    }

    #[test]
    fn resource_uid_hash_consistent() {
        use std::collections::HashSet;
        let uid = ResourceUid::new(42);
        let mut set = HashSet::new();
        set.insert(uid);
        assert!(set.contains(&ResourceUid::new(42)));
    }

    #[test]
    fn resource_uid_roundtrip() {
        let uid = ResourceUid::new(i64::MAX);
        assert_eq!(uid.raw(), i64::MAX);
    }
}
