//! Interned string type for fast equality comparison.
//!
//! `StringName` mirrors Godot's `StringName`: a lightweight handle to an
//! interned string. Two `StringName` values that were created from the
//! same string will compare equal in O(1) time (pointer comparison)
//! because they reference the same entry in a global intern table.
//!
//! Interned strings are leaked intentionally — they live for the
//! duration of the process. This is the same strategy Godot uses
//! internally: string names are never freed.

use std::collections::HashSet;
use std::fmt;
use std::sync::{OnceLock, RwLock};

/// Global intern table. Strings are leaked to obtain `&'static str`,
/// which makes `StringName` both `Copy` and `Send + Sync`.
fn intern_table() -> &'static RwLock<HashSet<&'static str>> {
    static TABLE: OnceLock<RwLock<HashSet<&'static str>>> = OnceLock::new();
    TABLE.get_or_init(|| RwLock::new(HashSet::new()))
}

/// Interns a string, returning a `&'static str` that is pointer-stable.
fn intern(s: &str) -> &'static str {
    // Fast path: check if already interned (read lock only).
    {
        let table = intern_table().read().expect("intern table poisoned");
        if let Some(&existing) = table.get(s) {
            return existing;
        }
    }

    // Slow path: acquire write lock and insert.
    let mut table = intern_table().write().expect("intern table poisoned");
    // Double-check after acquiring write lock.
    if let Some(&existing) = table.get(s) {
        return existing;
    }

    // Leak the string so we get a 'static reference.
    let leaked: &'static str = Box::leak(s.to_owned().into_boxed_str());
    table.insert(leaked);
    leaked
}

/// An interned string with O(1) equality comparison.
///
/// Equivalent to Godot's `StringName`. Two `StringName` values compare
/// equal if and only if they were constructed from identical string
/// content, but the comparison itself is a pointer comparison rather
/// than a byte-by-byte comparison.
#[derive(Clone, Copy)]
pub struct StringName {
    /// Pointer-stable interned string.
    inner: &'static str,
}

impl StringName {
    /// Creates or retrieves an interned `StringName`.
    pub fn new(s: &str) -> Self {
        Self { inner: intern(s) }
    }

    /// Returns the underlying string slice.
    pub fn as_str(&self) -> &str {
        self.inner
    }
}

impl PartialEq for StringName {
    fn eq(&self, other: &Self) -> bool {
        // Pointer comparison — the core benefit of interning.
        std::ptr::eq(self.inner as *const str, other.inner as *const str)
    }
}

impl Eq for StringName {}

impl PartialOrd for StringName {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for StringName {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.inner.cmp(other.inner)
    }
}

impl std::hash::Hash for StringName {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        // Hash by pointer value for consistency with Eq.
        (self.inner as *const str).hash(state);
    }
}

impl fmt::Debug for StringName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "StringName({:?})", self.inner)
    }
}

impl fmt::Display for StringName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.inner)
    }
}

impl From<&str> for StringName {
    fn from(s: &str) -> Self {
        Self::new(s)
    }
}

impl From<String> for StringName {
    fn from(s: String) -> Self {
        Self::new(&s)
    }
}

impl From<&String> for StringName {
    fn from(s: &String) -> Self {
        Self::new(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intern_same_string_is_equal() {
        let a = StringName::new("Player");
        let b = StringName::new("Player");
        assert_eq!(a, b);
    }

    #[test]
    fn different_strings_are_not_equal() {
        let a = StringName::new("Player");
        let b = StringName::new("Enemy");
        assert_ne!(a, b);
    }

    #[test]
    fn display() {
        let sn = StringName::new("position");
        assert_eq!(format!("{sn}"), "position");
    }

    #[test]
    fn debug() {
        let sn = StringName::new("velocity");
        assert_eq!(format!("{sn:?}"), "StringName(\"velocity\")");
    }

    #[test]
    fn from_str() {
        let sn: StringName = "Hello".into();
        assert_eq!(sn.as_str(), "Hello");
    }

    #[test]
    fn from_string() {
        let owned = String::from("World");
        let sn: StringName = owned.into();
        assert_eq!(sn.as_str(), "World");
    }

    #[test]
    fn pointer_equality_holds() {
        let a = StringName::new("test_pointer");
        let b = StringName::new("test_pointer");
        // They must share the same pointer.
        assert!(std::ptr::eq(a.as_str() as *const str, b.as_str() as *const str));
    }

    #[test]
    fn hash_consistent_with_eq() {
        use std::collections::HashSet;
        let mut set = HashSet::new();
        set.insert(StringName::new("key"));
        assert!(set.contains(&StringName::new("key")));
        assert!(!set.contains(&StringName::new("other")));
    }

    #[test]
    fn ordering() {
        let a = StringName::new("alpha");
        let b = StringName::new("beta");
        assert!(a < b);
    }

    #[test]
    fn copy_semantics() {
        let a = StringName::new("copy_test");
        let b = a; // Copy
        assert_eq!(a, b);
    }
}
