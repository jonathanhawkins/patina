//! pat-nzgdu.99 — the engine's reference model: a `RefCounted` starts with one
//! reference; taking another reference increments the count; releasing them
//! decrements it, and the final release reports the object should be freed; a
//! `WeakRef` taken to the object is invalidated once it is freed (mirroring
//! Godot's `RefCounted` + `weakref()` semantics).

use gdcore::id::ObjectId;
use gdobject::weak_ref::{self, WeakRef};
use gdobject::RefCountedBase;

#[test]
fn refcounted_reference_lifecycle_test() {
    // Isolate the alive registry from any other test in the binary. The registry
    // is PROCESS-global as of pat-0q2kx.163 (it was thread-local), so this clear
    // is no longer scoped to the calling thread — it is safe here only because
    // this binary holds exactly one test. Adding a second test to this file that
    // registers objects would make the two race.
    weak_ref::clear_alive_registry();

    // A fresh RefCounted starts with exactly one reference (`RefCounted.new()`).
    let rc = RefCountedBase::new();
    assert_eq!(rc.get_reference_count(), 1, "RefCounted.new() → count 1");

    // Model the live object and take a weak reference to it.
    let id = ObjectId::from_raw(42);
    weak_ref::register_object(id);
    let weak = WeakRef::new(id);
    assert_eq!(
        weak.get_ref(),
        Some(id),
        "the weakref resolves while the object is alive",
    );

    // `var r2 = r` — a second strong reference increments the count.
    rc.reference();
    assert_eq!(
        rc.get_reference_count(),
        2,
        "count 2 after a second reference"
    );

    // Release the references. Only the final `unreference` (count → 0) reports
    // that the object should now be freed.
    assert!(!rc.unreference(), "count 2→1 is not the last reference");
    let should_free = rc.unreference(); // count 1 → 0
    assert!(
        should_free,
        "the final unreference reports the object should be freed",
    );
    assert_eq!(rc.get_reference_count(), 0);

    // Freeing the object (as auto-free does on the last release) invalidates any
    // weakref taken to it — `WeakRef.get_ref()` now returns None.
    weak_ref::unregister_object(id);
    assert_eq!(
        weak.get_ref(),
        None,
        "the weakref is invalidated once all references are dropped and the object is freed",
    );
    assert!(
        !weak_ref::is_object_alive(id),
        "the freed object is no longer alive",
    );
}
