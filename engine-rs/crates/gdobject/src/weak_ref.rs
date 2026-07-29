//! WeakRef — a non-owning reference to a Godot Object.
//!
//! In Godot, `WeakRef` wraps an object ID and checks validity on access.
//! It does **not** prevent the referent from being freed; it only lets
//! callers detect that the object is gone without crashing.
//!
//! This implementation mirrors that contract: store the `ObjectId`, and
//! let the caller resolve it through whatever registry owns the object
//! (e.g. `SceneTree::get_node`).

use std::collections::HashSet;
use std::sync::{Mutex, OnceLock};

use gdcore::id::ObjectId;
use gdvariant::Variant;

// ---------------------------------------------------------------------------
// Global alive-objects registry (process-global)
// ---------------------------------------------------------------------------

/// Process-global, **not** thread-local (pat-0q2kx.163).
///
/// Object liveness is a property of the object, not of the thread that happens
/// to ask. This was a `thread_local!`, which meant every thread saw its own
/// empty registry: the editor server registers a node on its request thread,
/// and a `weakref` resolved from any other thread reported the object dead —
/// or, worse, an object freed on one thread still read as alive on another.
/// `is_instance_valid()` was therefore answerable only from the thread that
/// created the object.
///
/// Same `OnceLock<Mutex<_>>` shape the rest of this crate uses for
/// process-global state (`class_db::db`, `movie_writer::registry`), so the
/// locking discipline matches its neighbours.
fn alive_objects() -> &'static Mutex<HashSet<ObjectId>> {
    static ALIVE_OBJECTS: OnceLock<Mutex<HashSet<ObjectId>>> = OnceLock::new();
    ALIVE_OBJECTS.get_or_init(|| Mutex::new(HashSet::new()))
}

/// Registers an object as alive. Call when a node/object is created or
/// added to the scene tree.
pub fn register_object(id: ObjectId) {
    alive_objects().lock().unwrap().insert(id);
}

/// Unregisters an object (marks it as dead). Call when a node/object is
/// freed or removed from the scene tree.
pub fn unregister_object(id: ObjectId) {
    alive_objects().lock().unwrap().remove(&id);
}

/// Returns `true` if the object with the given ID is currently alive.
pub fn is_object_alive(id: ObjectId) -> bool {
    alive_objects().lock().unwrap().contains(&id)
}

/// Clears the entire alive-objects registry. Intended for test isolation.
///
/// ⚠️ Now that the registry is process-global, this clears it for EVERY thread.
/// A test that calls it while another test is running in the same process can
/// make that test see its own live objects as freed, so tests which both clear
/// and assert liveness must not run concurrently with each other.
pub fn clear_alive_registry() {
    alive_objects().lock().unwrap().clear();
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

    /// Liveness is a property of the OBJECT, not of the asking thread
    /// (pat-0q2kx.163).
    ///
    /// The registry used to be `thread_local!`, so each thread saw its own
    /// empty set: an object registered on one thread read as dead everywhere
    /// else, and a free on one thread was invisible to the others. That made
    /// `weakref` / `is_instance_valid` unusable from the multi-threaded editor
    /// server, where the thread that creates a node is rarely the one that
    /// later asks about it.
    ///
    /// Deliberately does NOT call `clear_alive_registry` — the registry is now
    /// shared, so clearing it here could free another test's objects out from
    /// under it. Fresh `ObjectId::next()` values keep this test independent
    /// without touching anyone else's entries.
    #[test]
    fn liveness_is_visible_across_threads() {
        let id = ObjectId::next();
        register_object(id);

        // Registered on THIS thread — another thread must agree it is alive.
        let seen_alive = std::thread::spawn(move || is_object_alive(id))
            .join()
            .expect("reader thread panicked");
        assert!(
            seen_alive,
            "an object registered on one thread must read as alive on another; \
             a thread-local registry reports false here"
        );

        // And a free performed on ANOTHER thread must be visible back here.
        std::thread::spawn(move || unregister_object(id))
            .join()
            .expect("writer thread panicked");
        assert!(
            !is_object_alive(id),
            "a free on another thread must be observable from this one"
        );
        assert_eq!(
            WeakRef::new(id).get_ref(),
            None,
            "get_ref() must follow the shared registry, not a per-thread view"
        );
    }
}
