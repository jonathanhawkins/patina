//! Script-reachable `RefCounted` instances, addressed by [`ObjectId`]
//! (pat-0q2kx.163).
//!
//! [`crate::ref_counted::RefCountedBase`] already implements Godot's counting
//! semantics correctly, but nothing owned an instance of it under an id, so a
//! script had no way to name one. GDScript addresses engine objects as
//! `Variant::ObjectId` handles, so this module is the missing registry: mint a
//! tagged id, keep its `RefCountedBase` here, and answer `reference` /
//! `unreference` / `get_reference_count` against it.
//!
//! # Liveness is the same registry `weakref` reads
//!
//! A handle is registered with [`crate::weak_ref::register_object`] on creation
//! and unregistered the moment its count reaches zero, so an existing
//! [`crate::weak_ref::WeakRef`] over it starts returning `None` with no extra
//! plumbing — which is exactly what `is_instance_valid(w.get_ref())` asks.
//!
//! # What this does NOT do
//!
//! It does not free anything on scope exit. Godot drops a reference when a
//! binding goes out of scope; here only an explicit `unreference()` decrements.
//! Automatic release needs ownership tracking inside the interpreter's
//! `Environment`, which is a separate and much larger change — see the
//! pat-0q2kx.163 bead notes. Until then the explicit API is faithful and the
//! implicit half is absent, rather than the whole feature being unreachable.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use gdcore::id::ObjectId;

use crate::ref_counted::RefCountedBase;
use crate::weak_ref;

/// Top byte marking an [`ObjectId`] as a script-created `RefCounted`.
///
/// Handle tags are a process-wide registry of one-byte prefixes — see the
/// `*_HANDLE_TAG` constants across `gdcore`, `gdresource`, `gdplatform` and the
/// interpreter. `0xEC` was unused; picking a taken byte would make two unrelated
/// handle families answer each other's `is_*_handle` checks and dispatch into
/// the wrong `call_*_instance`.
pub const REFCOUNTED_HANDLE_TAG: u64 = 0xEC00_0000_0000_0000;

/// Mask selecting the tag byte out of a raw id.
const TAG_MASK: u64 = 0xFF00_0000_0000_0000;

fn registry() -> &'static Mutex<HashMap<u64, RefCountedBase>> {
    static REGISTRY: OnceLock<Mutex<HashMap<u64, RefCountedBase>>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

fn next_seq() -> u64 {
    static NEXT: OnceLock<Mutex<u64>> = OnceLock::new();
    let cell = NEXT.get_or_init(|| Mutex::new(1));
    let mut n = cell.lock().unwrap();
    let v = *n;
    *n += 1;
    v
}

/// Whether `id` names a script-created `RefCounted`.
pub fn is_refcounted_handle(id: ObjectId) -> bool {
    id.raw() & TAG_MASK == REFCOUNTED_HANDLE_TAG
}

/// Creates a `RefCounted` with a reference count of 1 — Godot's `RefCounted.new()`.
///
/// The count starts at 1 rather than 0 because in Godot the act of creating the
/// object *is* the first reference; a fresh instance whose count read 0 would be
/// one `unreference()` away from a debug assertion on an unheld object.
pub fn new_refcounted_handle() -> ObjectId {
    let id = ObjectId::from_raw(REFCOUNTED_HANDLE_TAG | next_seq());
    registry()
        .lock()
        .unwrap()
        .insert(id.raw(), RefCountedBase::new());
    weak_ref::register_object(id);
    id
}

/// Godot `RefCounted.get_reference_count()`. `None` when `id` is not a live
/// refcounted handle — including one already released, which is the state a
/// stale script variable observes.
pub fn get_reference_count(id: ObjectId) -> Option<u32> {
    registry()
        .lock()
        .unwrap()
        .get(&id.raw())
        .map(|rc| rc.get_reference_count())
}

/// Godot `RefCounted.reference()` — take a reference. Returns `false` when the
/// handle is unknown, matching the "nothing to reference" case rather than
/// silently pretending success.
pub fn reference(id: ObjectId) -> bool {
    match registry().lock().unwrap().get(&id.raw()) {
        Some(rc) => {
            rc.reference();
            true
        }
        None => false,
    }
}

/// Godot `RefCounted.unreference()` — drop a reference. Returns `true` when the
/// count reached zero and the object was therefore released.
///
/// Releasing removes it from the alive registry, so any [`WeakRef`] over it
/// reports gone from the next `get_ref()` onward.
///
/// [`WeakRef`]: crate::weak_ref::WeakRef
pub fn unreference(id: ObjectId) -> bool {
    let mut reg = registry().lock().unwrap();
    let released = match reg.get(&id.raw()) {
        Some(rc) => rc.unreference(),
        None => return false,
    };
    if released {
        reg.remove(&id.raw());
        // Drop the lock before touching the other global: the two registries are
        // independent and nesting their locks in different orders elsewhere
        // would be a deadlock waiting to happen.
        drop(reg);
        weak_ref::unregister_object(id);
    }
    released
}

/// Godot `RefCounted.init_ref()` — take the initial reference on an object that
/// does not yet have one. Returns whether this call is now holding a reference.
///
/// Godot's `init_ref` is idempotent in the sense that it never takes a *second*
/// initial reference: a live object already has its count, so this reports
/// `true` without incrementing. That is why it is not a synonym for
/// [`reference`].
pub fn init_ref(id: ObjectId) -> bool {
    registry().lock().unwrap().contains_key(&id.raw())
}

/// Drops every registered handle. Test isolation only.
pub fn clear_registry_for_testing() {
    let ids: Vec<u64> = registry().lock().unwrap().keys().copied().collect();
    registry().lock().unwrap().clear();
    for raw in ids {
        weak_ref::unregister_object(ObjectId::from_raw(raw));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_refcounted_starts_at_one_and_is_alive() {
        let id = new_refcounted_handle();
        assert_eq!(
            get_reference_count(id),
            Some(1),
            "Godot's RefCounted.new() yields a count of 1, not 0"
        );
        assert!(
            weak_ref::is_object_alive(id),
            "a fresh RefCounted must be alive so weakref/is_instance_valid see it"
        );
        assert!(is_refcounted_handle(id));
    }

    #[test]
    fn handles_are_distinct() {
        let a = new_refcounted_handle();
        let b = new_refcounted_handle();
        assert_ne!(a, b, "each .new() is a separate object");
        // Releasing one must not disturb the other.
        assert!(unreference(a));
        assert_eq!(get_reference_count(b), Some(1));
        assert!(weak_ref::is_object_alive(b));
    }

    #[test]
    fn a_second_reference_keeps_it_alive_after_one_release() {
        let id = new_refcounted_handle();
        assert!(reference(id));
        assert_eq!(get_reference_count(id), Some(2));

        assert!(
            !unreference(id),
            "dropping one of two references must not release the object"
        );
        assert_eq!(get_reference_count(id), Some(1));
        assert!(
            weak_ref::is_object_alive(id),
            "still held, so still valid — this is the acceptance's \
             'assigning to a second var then releasing one keeps it alive'"
        );

        assert!(unreference(id), "the last release reports the free");
        assert!(!weak_ref::is_object_alive(id));
        assert_eq!(
            get_reference_count(id),
            None,
            "a released handle is gone from the registry, not a zero count"
        );
    }

    #[test]
    fn a_weakref_over_a_released_object_reports_gone() {
        let id = new_refcounted_handle();
        let weak = weak_ref::WeakRef::new(id);
        assert_eq!(weak.get_ref(), Some(id));

        unreference(id);
        assert_eq!(
            weak.get_ref(),
            None,
            "release unregisters from the SAME registry WeakRef consults, so no \
             extra bookkeeping is needed to make is_instance_valid answer false"
        );
    }

    #[test]
    fn unknown_handles_are_reported_rather_than_faked() {
        // An id that was never minted here — e.g. a node, or an already-released
        // RefCounted a stale script variable still names.
        let bogus = ObjectId::from_raw(REFCOUNTED_HANDLE_TAG | 0x00FF_FFFF);
        assert_eq!(get_reference_count(bogus), None);
        assert!(!reference(bogus));
        assert!(!unreference(bogus));
        assert!(!init_ref(bogus));
    }

    #[test]
    fn init_ref_does_not_take_a_second_initial_reference() {
        let id = new_refcounted_handle();
        assert!(init_ref(id));
        assert_eq!(
            get_reference_count(id),
            Some(1),
            "init_ref reports that a reference is held; it is not a synonym for \
             reference() and must not increment"
        );
    }

    #[test]
    fn the_tag_byte_is_not_shared_with_another_handle_family() {
        let id = new_refcounted_handle();
        assert_eq!(id.raw() & TAG_MASK, REFCOUNTED_HANDLE_TAG);
        // A neighbouring tag must not answer to this family's check: dispatch
        // chains test these in sequence, so an overlap sends one family's
        // methods into another's implementation.
        for other in [0xE900_0000_0000_0000u64, 0xED00_0000_0000_0000u64] {
            assert!(
                !is_refcounted_handle(ObjectId::from_raw(other | 1)),
                "tag {other:#x} must not be mistaken for a RefCounted handle"
            );
        }
    }
}
