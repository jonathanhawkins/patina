//! RefCounted base class and reference counting.
//!
//! In Godot, `RefCounted` (formerly `Reference`) is the base class for
//! objects that are automatically freed when no more references exist.
//! Resources, scripts, and many utility types inherit from it.
//!
//! This module provides:
//! - [`RefCounted`] trait — the interface for reference-counted objects.
//! - [`RefCountedBase`] — a thread-safe reference count using `Arc<AtomicU32>`.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

/// Trait implemented by objects that use Godot-style reference counting.
///
/// In Godot, `RefCounted` objects start with a reference count of 1. Calling
/// [`reference()`](RefCounted::reference) increments the count, and
/// [`unreference()`](RefCounted::unreference) decrements it. When the count
/// reaches zero, the object should be freed.
pub trait RefCounted {
    /// Increments the reference count by one.
    fn reference(&self);

    /// Decrements the reference count by one. Returns `true` if the count
    /// reached zero (meaning the object should be freed).
    fn unreference(&self) -> bool;

    /// Returns the current reference count.
    fn get_reference_count(&self) -> u32;
}

/// Thread-safe reference count backing store.
///
/// Wraps an `Arc<AtomicU32>` so that multiple owners can share and
/// manipulate the same count. The initial count is 1, matching Godot's
/// behavior where a newly created `RefCounted` object starts with one
/// reference.
#[derive(Debug, Clone)]
pub struct RefCountedBase {
    /// The shared atomic reference count.
    count: Arc<AtomicU32>,
}

impl RefCountedBase {
    /// Creates a new `RefCountedBase` with an initial count of 1.
    pub fn new() -> Self {
        Self {
            count: Arc::new(AtomicU32::new(1)),
        }
    }

    /// Creates a `RefCountedBase` with a specific initial count.
    ///
    /// This is primarily useful for testing. In normal usage, objects
    /// start with a count of 1.
    pub fn with_count(initial: u32) -> Self {
        Self {
            count: Arc::new(AtomicU32::new(initial)),
        }
    }

    /// Increments the reference count by one.
    pub fn reference(&self) {
        self.count.fetch_add(1, Ordering::AcqRel);
    }

    /// Decrements the reference count by one. Returns `true` if the count
    /// reached zero.
    ///
    /// # Panics
    ///
    /// Debug-asserts that the count was not already zero before decrementing.
    pub fn unreference(&self) -> bool {
        let prev = self.count.fetch_sub(1, Ordering::AcqRel);
        debug_assert!(prev > 0, "unreference() called when count was already 0");
        prev == 1
    }

    /// Returns the current reference count.
    pub fn get_reference_count(&self) -> u32 {
        self.count.load(Ordering::Acquire)
    }
}

impl Default for RefCountedBase {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initial_count_is_one() {
        let rc = RefCountedBase::new();
        assert_eq!(rc.get_reference_count(), 1);
    }

    #[test]
    fn reference_increments() {
        let rc = RefCountedBase::new();
        rc.reference();
        assert_eq!(rc.get_reference_count(), 2);
        rc.reference();
        assert_eq!(rc.get_reference_count(), 3);
    }

    #[test]
    fn unreference_decrements() {
        let rc = RefCountedBase::new();
        rc.reference(); // count = 2
        assert!(!rc.unreference()); // count = 1
        assert_eq!(rc.get_reference_count(), 1);
    }

    #[test]
    fn unreference_returns_true_at_zero() {
        let rc = RefCountedBase::new(); // count = 1
        assert!(rc.unreference()); // count = 0
        assert_eq!(rc.get_reference_count(), 0);
    }

    #[test]
    fn with_count_custom_initial() {
        let rc = RefCountedBase::with_count(5);
        assert_eq!(rc.get_reference_count(), 5);
    }

    #[test]
    fn clone_shares_count() {
        let rc1 = RefCountedBase::new();
        let rc2 = rc1.clone();
        rc1.reference(); // count = 2
        assert_eq!(rc2.get_reference_count(), 2);
    }

    #[test]
    fn default_is_one() {
        let rc = RefCountedBase::default();
        assert_eq!(rc.get_reference_count(), 1);
    }

    #[test]
    fn multiple_reference_unreference_cycles() {
        let rc = RefCountedBase::new(); // 1
        rc.reference(); // 2
        rc.reference(); // 3
        rc.reference(); // 4
        assert_eq!(rc.get_reference_count(), 4);
        assert!(!rc.unreference()); // 3
        assert!(!rc.unreference()); // 2
        assert!(!rc.unreference()); // 1
        assert_eq!(rc.get_reference_count(), 1);
        assert!(rc.unreference()); // 0
    }

    #[test]
    fn thread_safety() {
        use std::thread;

        let rc = RefCountedBase::new(); // count = 1

        let handles: Vec<_> = (0..10)
            .map(|_| {
                let rc_clone = rc.clone();
                thread::spawn(move || {
                    rc_clone.reference();
                })
            })
            .collect();

        for h in handles {
            h.join().unwrap();
        }

        // 1 (initial) + 10 (from threads) = 11
        assert_eq!(rc.get_reference_count(), 11);
    }

    #[test]
    fn trait_impl_on_wrapper() {
        /// A test struct that wraps RefCountedBase and implements RefCounted.
        struct TestResource {
            refcount: RefCountedBase,
        }

        impl RefCounted for TestResource {
            fn reference(&self) {
                self.refcount.reference();
            }
            fn unreference(&self) -> bool {
                self.refcount.unreference()
            }
            fn get_reference_count(&self) -> u32 {
                self.refcount.get_reference_count()
            }
        }

        let res = TestResource {
            refcount: RefCountedBase::new(),
        };
        assert_eq!(res.get_reference_count(), 1);
        res.reference();
        assert_eq!(res.get_reference_count(), 2);
        assert!(!res.unreference());
        assert!(res.unreference());
        assert_eq!(res.get_reference_count(), 0);
    }

    #[test]
    fn concurrent_reference_and_unreference() {
        use std::thread;

        let rc = RefCountedBase::with_count(100);

        let mut handles = Vec::new();

        // 50 threads increment
        for _ in 0..50 {
            let rc_clone = rc.clone();
            handles.push(thread::spawn(move || {
                rc_clone.reference();
            }));
        }

        // 50 threads decrement
        for _ in 0..50 {
            let rc_clone = rc.clone();
            handles.push(thread::spawn(move || {
                rc_clone.unreference();
            }));
        }

        for h in handles {
            h.join().unwrap();
        }

        // 100 + 50 - 50 = 100
        assert_eq!(rc.get_reference_count(), 100);
    }

    #[test]
    fn with_count_zero() {
        let rc = RefCountedBase::with_count(0);
        assert_eq!(rc.get_reference_count(), 0);
        // Referencing from zero should work
        rc.reference();
        assert_eq!(rc.get_reference_count(), 1);
    }
}
