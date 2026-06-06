//! WeakRef — a non-owning reference to a Godot Object.
//!
//! In Godot, `WeakRef` wraps an object ID and checks validity on access.
//! It does **not** prevent the referent from being freed; it only lets
//! callers detect that the object is gone without crashing.
//!
//! This implementation mirrors that contract: store the `ObjectId`, and
//! let the caller resolve it through whatever registry owns the object
//! (e.g. `SceneTree::get_node`).

use std::cell::RefCell;
use std::collections::HashSet;

use gdcore::id::ObjectId;
use gdvariant::Variant;

// ---------------------------------------------------------------------------
// Global alive-objects registry (thread-local)
// ---------------------------------------------------------------------------

thread_local! {
    static ALIVE_OBJECTS: RefCell<HashSet<ObjectId>> = RefCell::new(HashSet::new());
}

/// Registers an object as alive. Call when a node/object is created or
/// added to the scene tree.
pub fn register_object(id: ObjectId) {
    ALIVE_OBJECTS.with(|set| {
        set.borrow_mut().insert(id);
    });
}

/// Unregisters an object (marks it as dead). Call when a node/object is
/// freed or removed from the scene tree.
pub fn unregister_object(id: ObjectId) {
    ALIVE_OBJECTS.with(|set| {
        set.borrow_mut().remove(&id);
    });
}

/// Returns `true` if the object with the given ID is currently alive.
pub fn is_object_alive(id: ObjectId) -> bool {
    ALIVE_OBJECTS.with(|set| set.borrow().contains(&id))
}

/// Clears the entire alive-objects registry. Intended for test isolation.
pub fn clear_alive_registry() {
    ALIVE_OBJECTS.with(|set| set.borrow_mut().clear());
}

/// A weak (non-owning) reference to a Godot Object.
///
/// `WeakRef` stores only the [`ObjectId`] of the referent. It cannot
/// prevent the object from being freed. When the object is freed,
/// `get_ref()` automatically returns `None` by checking the global
/// alive-objects registry.
///
/// # Godot parity
///
/// | Godot method          | Patina equivalent                        |
/// |-----------------------|------------------------------------------|
/// | `WeakRef.get_ref()`   | `weak.get_ref()` → `Option<ObjectId>`    |
/// | `weakref(obj)`        | `WeakRef::new(obj.get_instance_id())`    |
/// | `is_instance_valid()` | `is_object_alive(id)` or `weak.get_ref().is_some()` |
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WeakRef {
    id: ObjectId,
    /// Set to `true` when the caller explicitly invalidates this ref
    /// (e.g. after observing that the object was freed).
    invalidated: bool,
}

impl WeakRef {
    /// Creates a new weak reference to the object with the given ID.
    pub fn new(id: ObjectId) -> Self {
        Self {
            id,
            invalidated: false,
        }
    }

    /// Returns the referenced [`ObjectId`], or `None` if this ref has
    /// been explicitly invalidated or the object has been freed.
    ///
    /// This checks the global alive-objects registry, so it automatically
    /// returns `None` after the referent is freed — matching Godot's
    /// `WeakRef.get_ref()` behavior.
    pub fn get_ref(&self) -> Option<ObjectId> {
        if self.invalidated {
            None
        } else if is_object_alive(self.id) {
            Some(self.id)
        } else {
            None
        }
    }

    /// Returns the stored object ID regardless of invalidation state.
    /// Useful for logging and diagnostics.
    pub fn object_id(&self) -> ObjectId {
        self.id
    }

    /// Marks this weak reference as invalidated. Subsequent calls to
    /// [`get_ref`](Self::get_ref) will return `None`.
    pub fn invalidate(&mut self) {
        self.invalidated = true;
    }

    /// Returns `true` if this ref has been explicitly invalidated.
    pub fn is_invalidated(&self) -> bool {
        self.invalidated
    }

    /// Converts the weak reference to a [`Variant`] for script interop.
    pub fn to_variant(&self) -> Variant {
        if self.get_ref().is_some() {
            Variant::Int(self.id.raw() as i64)
        } else {
            Variant::Nil
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_weak_ref_returns_id() {
        let id = ObjectId::next();
        register_object(id);
        let wr = WeakRef::new(id);
        assert_eq!(wr.get_ref(), Some(id));
        assert!(!wr.is_invalidated());
        unregister_object(id);
    }

    #[test]
    fn invalidated_ref_returns_none() {
        let id = ObjectId::next();
        register_object(id);
        let mut wr = WeakRef::new(id);
        wr.invalidate();
        assert_eq!(wr.get_ref(), None);
        assert!(wr.is_invalidated());
        unregister_object(id);
    }

    #[test]
    fn object_id_available_even_after_invalidation() {
        let id = ObjectId::next();
        let mut wr = WeakRef::new(id);
        wr.invalidate();
        assert_eq!(wr.object_id(), id);
    }

    #[test]
    fn to_variant_returns_nil_when_invalidated() {
        let id = ObjectId::next();
        register_object(id);
        let mut wr = WeakRef::new(id);
        assert!(matches!(wr.to_variant(), Variant::Int(_)));
        wr.invalidate();
        assert_eq!(wr.to_variant(), Variant::Nil);
        unregister_object(id);
    }

    #[test]
    fn clone_preserves_state() {
        let id = ObjectId::next();
        register_object(id);
        let mut wr = WeakRef::new(id);
        wr.invalidate();
        let cloned = wr;
        assert!(cloned.is_invalidated());
        assert_eq!(cloned.get_ref(), None);
        unregister_object(id);
    }

    #[test]
    fn equality() {
        let id = ObjectId::next();
        let a = WeakRef::new(id);
        let b = WeakRef::new(id);
        assert_eq!(a, b);
    }

    #[test]
    fn unregistered_object_returns_none() {
        let id = ObjectId::next();
        // Don't register — simulates freed object.
        let wr = WeakRef::new(id);
        assert_eq!(wr.get_ref(), None);
    }

    #[test]
    fn auto_invalidation_on_unregister() {
        let id = ObjectId::next();
        register_object(id);
        let wr = WeakRef::new(id);
        assert_eq!(wr.get_ref(), Some(id));
        unregister_object(id);
        assert_eq!(wr.get_ref(), None);
    }
}
