//! pat-41b4: Repeated-load cache deduplication regression tests.
//!
//! Validates that repeated resource loads preserve expected sharing and
//! invalidation behavior across realistic multi-resource workflows:
//!
//! - Load order independence (A-then-B == B-then-A per path)
//! - Interleaved load/invalidate/reload cycles
//! - Arc reference counting consistency across invalidation cycles
//! - Error-does-not-cache: failed loads don't pollute the cache
//! - UnifiedLoader repeated loads through UID and path
//! - Property preservation through cache round-trips
//! - Drop-all-external-refs: cache still holds Arc, re-fetch works
//! - Bulk invalidation and selective reload
//! - Multiple invalidation cycles produce unique Arcs each time

use std::cell::Cell;
use std::sync::Arc;

use gdcore::error::{EngineError, EngineResult};
use gdresource::loader::TresLoader;
use gdresource::{Resource, ResourceCache, ResourceLoader, UnifiedLoader};
use gdvariant::Variant;

// ---------------------------------------------------------------------------
// Test loaders
// ---------------------------------------------------------------------------

/// Loader that counts calls and returns a resource tagged with path + call index.
struct CountingLoader {
    call_count: Cell<u32>,
}

impl CountingLoader {
    fn new() -> Self {
        Self {
            call_count: Cell::new(0),
        }
    }
}

impl ResourceLoader for CountingLoader {
    fn load(&self, path: &str) -> EngineResult<Arc<Resource>> {
        let n = self.call_count.get() + 1;
        self.call_count.set(n);
        let mut r = Resource::new("Counted");
        r.path = path.to_string();
        r.set_property("load_index", Variant::Int(n as i64));
        Ok(Arc::new(r))
    }
}

/// Loader that fails for specific paths.
struct SelectiveFailLoader {
    fail_paths: Vec<&'static str>,
    call_count: Cell<u32>,
}

impl SelectiveFailLoader {
    fn new(fail_paths: Vec<&'static str>) -> Self {
        Self {
            fail_paths,
            call_count: Cell::new(0),
        }
    }
}

impl ResourceLoader for SelectiveFailLoader {
    fn load(&self, path: &str) -> EngineResult<Arc<Resource>> {
        self.call_count.set(self.call_count.get() + 1);
        if self.fail_paths.contains(&path) {
            return Err(EngineError::NotFound(path.to_string()));
        }
        let mut r = Resource::new("Selective");
        r.path = path.to_string();
        Ok(Arc::new(r))
    }
}

/// Inline .tres content loader for sub-resource tests.
struct InlineTresLoader {
    entries: Vec<(&'static str, &'static str)>,
    call_count: Cell<u32>,
}

impl InlineTresLoader {
    fn new(entries: Vec<(&'static str, &'static str)>) -> Self {
        Self {
            entries,
            call_count: Cell::new(0),
        }
    }
}

impl ResourceLoader for InlineTresLoader {
    fn load(&self, path: &str) -> EngineResult<Arc<Resource>> {
        self.call_count.set(self.call_count.get() + 1);
        for (p, content) in &self.entries {
            if *p == path {
                return TresLoader::new().parse_str(content, path);
            }
        }
        Err(EngineError::NotFound(path.to_string()))
    }
}

// ===========================================================================
// 1. Load order independence
// ===========================================================================

#[test]
fn load_order_does_not_affect_dedup() {
    // Load A then B, then re-fetch both — same Arcs regardless of insertion order.
    let mut cache = ResourceCache::new(CountingLoader::new());

    let a1 = cache.load("res://a.tres").unwrap();
    let b1 = cache.load("res://b.tres").unwrap();
    let b2 = cache.load("res://b.tres").unwrap();
    let a2 = cache.load("res://a.tres").unwrap();

    assert!(
        Arc::ptr_eq(&a1, &a2),
        "A must be same regardless of fetch order"
    );
    assert!(
        Arc::ptr_eq(&b1, &b2),
        "B must be same regardless of fetch order"
    );
    assert!(!Arc::ptr_eq(&a1, &b1));
    assert_eq!(cache.len(), 2);
}

#[test]
fn reverse_load_order_same_result() {
    let mut cache1 = ResourceCache::new(CountingLoader::new());
    let mut cache2 = ResourceCache::new(CountingLoader::new());

    // Cache 1: load A first
    let a1 = cache1.load("res://x.tres").unwrap();
    let _b1 = cache1.load("res://y.tres").unwrap();
    let a1_again = cache1.load("res://x.tres").unwrap();

    // Cache 2: load B first
    let _b2 = cache2.load("res://y.tres").unwrap();
    let a2 = cache2.load("res://x.tres").unwrap();
    let a2_again = cache2.load("res://x.tres").unwrap();

    assert!(Arc::ptr_eq(&a1, &a1_again));
    assert!(Arc::ptr_eq(&a2, &a2_again));
    assert_eq!(cache1.len(), 2);
    assert_eq!(cache2.len(), 2);
}

// ===========================================================================
// 2. Interleaved load/invalidate/reload cycles
// ===========================================================================

#[test]
fn interleaved_invalidate_reload_preserves_unrelated() {
    let mut cache = ResourceCache::new(CountingLoader::new());

    let a1 = cache.load("res://a.tres").unwrap();
    let b1 = cache.load("res://b.tres").unwrap();
    let c1 = cache.load("res://c.tres").unwrap();

    // Invalidate B, reload B — A and C unchanged.
    cache.invalidate("res://b.tres");
    let b2 = cache.load("res://b.tres").unwrap();
    let a1_check = cache.load("res://a.tres").unwrap();
    let c1_check = cache.load("res://c.tres").unwrap();

    assert!(!Arc::ptr_eq(&b1, &b2), "B must be fresh after invalidation");
    assert!(Arc::ptr_eq(&a1, &a1_check), "A must be unaffected");
    assert!(Arc::ptr_eq(&c1, &c1_check), "C must be unaffected");
    assert_eq!(cache.len(), 3);
}

#[test]
fn alternating_invalidate_reload_two_resources() {
    let mut cache = ResourceCache::new(CountingLoader::new());

    let a1 = cache.load("res://a.tres").unwrap();
    let b1 = cache.load("res://b.tres").unwrap();

    // Round 1: invalidate A
    cache.invalidate("res://a.tres");
    let a2 = cache.load("res://a.tres").unwrap();
    assert!(!Arc::ptr_eq(&a1, &a2));
    assert!(Arc::ptr_eq(&b1, &cache.load("res://b.tres").unwrap()));

    // Round 2: invalidate B
    cache.invalidate("res://b.tres");
    let b2 = cache.load("res://b.tres").unwrap();
    assert!(!Arc::ptr_eq(&b1, &b2));
    assert!(Arc::ptr_eq(&a2, &cache.load("res://a.tres").unwrap()));

    assert_eq!(cache.len(), 2);
}

// ===========================================================================
// 3. Arc reference counting across invalidation cycles
// ===========================================================================

#[test]
fn strong_count_tracks_through_multiple_fetches() {
    let mut cache = ResourceCache::new(CountingLoader::new());

    let r1 = cache.load("res://item.tres").unwrap();
    assert_eq!(Arc::strong_count(&r1), 2); // cache + r1

    let r2 = cache.load("res://item.tres").unwrap();
    assert_eq!(Arc::strong_count(&r1), 3); // cache + r1 + r2

    let r3 = cache.load("res://item.tres").unwrap();
    assert_eq!(Arc::strong_count(&r1), 4); // cache + r1 + r2 + r3

    drop(r3);
    assert_eq!(Arc::strong_count(&r1), 3);

    drop(r2);
    assert_eq!(Arc::strong_count(&r1), 2);
}

#[test]
fn strong_count_after_invalidate_old_survives() {
    let mut cache = ResourceCache::new(CountingLoader::new());

    let old = cache.load("res://thing.tres").unwrap();
    let _old_clone = cache.load("res://thing.tres").unwrap();
    assert_eq!(Arc::strong_count(&old), 3); // cache + old + _old_clone

    cache.invalidate("res://thing.tres");
    assert_eq!(Arc::strong_count(&old), 2); // old + _old_clone (cache dropped)

    let fresh = cache.load("res://thing.tres").unwrap();
    assert_eq!(Arc::strong_count(&fresh), 2); // cache + fresh
    assert_eq!(Arc::strong_count(&old), 2); // unchanged
    assert!(!Arc::ptr_eq(&old, &fresh));
}

// ===========================================================================
// 4. Error-does-not-cache: failed loads don't pollute the cache
// ===========================================================================

#[test]
fn failed_load_does_not_cache() {
    let mut cache = ResourceCache::new(SelectiveFailLoader::new(vec!["res://bad.tres"]));

    let result = cache.load("res://bad.tres");
    assert!(result.is_err());
    assert!(
        !cache.contains("res://bad.tres"),
        "error should not be cached"
    );
    assert!(cache.is_empty());
}

#[test]
fn successful_load_after_failure_of_different_path() {
    let mut cache = ResourceCache::new(SelectiveFailLoader::new(vec!["res://broken.tres"]));

    let fail = cache.load("res://broken.tres");
    assert!(fail.is_err());

    let ok = cache.load("res://good.tres");
    assert!(ok.is_ok());
    assert_eq!(cache.len(), 1);
    assert!(cache.contains("res://good.tres"));
    assert!(!cache.contains("res://broken.tres"));
}

#[test]
fn error_does_not_prevent_retry() {
    // If the loader fails, we should be able to retry.
    // Use a loader that fails on first call then succeeds.
    struct RetryLoader {
        call_count: Cell<u32>,
    }
    impl ResourceLoader for RetryLoader {
        fn load(&self, path: &str) -> EngineResult<Arc<Resource>> {
            let n = self.call_count.get() + 1;
            self.call_count.set(n);
            if n == 1 {
                return Err(EngineError::NotFound("transient".to_string()));
            }
            let mut r = Resource::new("Retried");
            r.path = path.to_string();
            Ok(Arc::new(r))
        }
    }

    let mut cache = ResourceCache::new(RetryLoader {
        call_count: Cell::new(0),
    });
    assert!(cache.load("res://flaky.tres").is_err());
    assert!(!cache.contains("res://flaky.tres"));

    // Second attempt should succeed and cache.
    let res = cache.load("res://flaky.tres").unwrap();
    assert!(cache.contains("res://flaky.tres"));
    assert_eq!(res.path, "res://flaky.tres");
}

// ===========================================================================
// 5. UnifiedLoader repeated loads through UID and path
// ===========================================================================

#[test]
fn unified_repeated_uid_load_deduplicates() {
    let mut ul = UnifiedLoader::new(CountingLoader::new());
    ul.register_uid_str("uid://sword", "res://sword.tres");

    let a = ul.load("uid://sword").unwrap();
    let b = ul.load("uid://sword").unwrap();
    let c = ul.load("res://sword.tres").unwrap();

    assert!(Arc::ptr_eq(&a, &b), "repeated UID loads must dedup");
    assert!(Arc::ptr_eq(&a, &c), "UID and path must resolve to same Arc");
    assert_eq!(ul.cache_len(), 1);
}

#[test]
fn unified_invalidate_then_reload_by_uid() {
    let mut ul = UnifiedLoader::new(CountingLoader::new());
    ul.register_uid_str("uid://item", "res://item.tres");

    let first = ul.load("uid://item").unwrap();
    ul.invalidate("res://item.tres");
    let second = ul.load("uid://item").unwrap();

    assert!(!Arc::ptr_eq(&first, &second), "invalidated UID must reload");
    assert!(ul.is_cached("res://item.tres"));
}

#[test]
fn unified_path_then_uid_dedup() {
    // Load by path first, then by UID — same Arc.
    let mut ul = UnifiedLoader::new(CountingLoader::new());
    ul.register_uid_str("uid://shared_ref", "res://shared.tres");

    let by_path = ul.load("res://shared.tres").unwrap();
    let by_uid = ul.load("uid://shared_ref").unwrap();

    assert!(Arc::ptr_eq(&by_path, &by_uid), "path then UID must dedup");
    assert_eq!(ul.cache_len(), 1);
}

#[test]
fn unified_uid_reregistration_with_invalidation() {
    let mut ul = UnifiedLoader::new(CountingLoader::new());
    ul.register_uid_str("uid://weapon", "res://old_weapon.tres");

    let old = ul.load("uid://weapon").unwrap();
    assert_eq!(old.path, "res://old_weapon.tres");

    // "Rename" the resource by re-registering UID + invalidating old path.
    ul.invalidate("res://old_weapon.tres");
    ul.register_uid_str("uid://weapon", "res://new_weapon.tres");

    let fresh = ul.load("uid://weapon").unwrap();
    assert_eq!(fresh.path, "res://new_weapon.tres");
    assert!(!Arc::ptr_eq(&old, &fresh));
}

// ===========================================================================
// 6. Property preservation through cache round-trips
// ===========================================================================

#[test]
fn cached_load_preserves_properties() {
    let mut cache = ResourceCache::new(CountingLoader::new());

    let first = cache.load("res://prop.tres").unwrap();
    let second = cache.load("res://prop.tres").unwrap();

    // Same Arc means properties are identical.
    assert_eq!(
        first.get_property("load_index"),
        second.get_property("load_index")
    );
}

#[test]
fn reloaded_resource_has_fresh_properties() {
    let mut cache = ResourceCache::new(CountingLoader::new());

    let first = cache.load("res://versioned.tres").unwrap();
    let idx1 = match first.get_property("load_index") {
        Some(Variant::Int(n)) => *n,
        other => panic!("expected Int, got {other:?}"),
    };

    cache.invalidate("res://versioned.tres");
    let second = cache.load("res://versioned.tres").unwrap();
    let idx2 = match second.get_property("load_index") {
        Some(Variant::Int(n)) => *n,
        other => panic!("expected Int, got {other:?}"),
    };

    assert_ne!(
        idx1, idx2,
        "reload should produce resource with new load_index"
    );
    assert_eq!(idx2, idx1 + 1);
}

// ===========================================================================
// 7. Drop-all-external-refs: cache still holds Arc
// ===========================================================================

#[test]
fn cache_holds_arc_after_external_drop() {
    let mut cache = ResourceCache::new(CountingLoader::new());

    {
        let _r = cache.load("res://ephemeral.tres").unwrap();
        // _r dropped at end of block
    }

    // Cache still has it.
    assert!(cache.contains("res://ephemeral.tres"));

    // Re-fetch returns the cached Arc (loader not called again).
    let refetched = cache.load("res://ephemeral.tres").unwrap();
    assert_eq!(Arc::strong_count(&refetched), 2); // cache + refetched
    assert_eq!(cache.len(), 1);
}

#[test]
fn cache_holds_through_multiple_drop_cycles() {
    let mut cache = ResourceCache::new(CountingLoader::new());

    for _ in 0..5 {
        let _r = cache.load("res://cycled.tres").unwrap();
        // dropped each iteration
    }

    assert_eq!(cache.len(), 1);
    assert!(cache.contains("res://cycled.tres"));
}

// ===========================================================================
// 8. Bulk invalidation and selective reload
// ===========================================================================

#[test]
fn bulk_invalidate_then_selective_reload() {
    let mut cache = ResourceCache::new(CountingLoader::new());

    let paths: Vec<&str> = (0..5)
        .map(|i| match i {
            0 => "res://r0.tres",
            1 => "res://r1.tres",
            2 => "res://r2.tres",
            3 => "res://r3.tres",
            _ => "res://r4.tres",
        })
        .collect();

    let originals: Vec<_> = paths.iter().map(|p| cache.load(p).unwrap()).collect();
    assert_eq!(cache.len(), 5);

    // Invalidate all.
    for p in &paths {
        cache.invalidate(p);
    }
    assert!(cache.is_empty());

    // Reload only 2 of 5.
    let r0_new = cache.load("res://r0.tres").unwrap();
    let r3_new = cache.load("res://r3.tres").unwrap();

    assert!(!Arc::ptr_eq(&originals[0], &r0_new));
    assert!(!Arc::ptr_eq(&originals[3], &r3_new));
    assert_eq!(cache.len(), 2);
    assert_eq!(cache.len(), 2, "only the 2 reloaded paths remain cached");
}

#[test]
fn clear_then_selective_reload_no_cross_contamination() {
    let mut cache = ResourceCache::new(CountingLoader::new());

    let a_old = cache.load("res://a.tres").unwrap();
    let b_old = cache.load("res://b.tres").unwrap();
    cache.clear();

    // Reload only A.
    let a_new = cache.load("res://a.tres").unwrap();
    assert!(!Arc::ptr_eq(&a_old, &a_new));
    assert_eq!(cache.len(), 1);
    assert!(
        !cache.contains("res://b.tres"),
        "B should not magically reappear"
    );

    // Old B is still alive but not in cache.
    assert_eq!(Arc::strong_count(&b_old), 1);
}

// ===========================================================================
// 9. Multiple invalidation cycles produce unique Arcs each time
// ===========================================================================

#[test]
fn repeated_invalidate_reload_produces_unique_arcs() {
    let mut cache = ResourceCache::new(CountingLoader::new());
    let mut arcs = Vec::new();

    for _ in 0..5 {
        let r = cache.load("res://cycling.tres").unwrap();
        arcs.push(r);
        cache.invalidate("res://cycling.tres");
    }

    // Each Arc should be unique (no two pointer-equal).
    for i in 0..arcs.len() {
        for j in (i + 1)..arcs.len() {
            assert!(
                !Arc::ptr_eq(&arcs[i], &arcs[j]),
                "cycle {i} and {j} must produce different Arcs"
            );
        }
    }

    // Cache is empty since each Arc was invalidated after load.
    assert!(cache.is_empty());
    // All old Arcs still alive (strong_count == 1 each).
    for arc in &arcs {
        assert_eq!(Arc::strong_count(arc), 1);
    }
}

#[test]
fn repeated_invalidate_reload_load_index_monotonic() {
    let mut cache = ResourceCache::new(CountingLoader::new());

    let mut indices = Vec::new();
    for _ in 0..4 {
        let r = cache.load("res://versioned.tres").unwrap();
        let idx = match r.get_property("load_index") {
            Some(Variant::Int(n)) => *n,
            other => panic!("expected Int, got {other:?}"),
        };
        indices.push(idx);
        cache.invalidate("res://versioned.tres");
    }

    // Each reload should have a strictly increasing load_index.
    for i in 1..indices.len() {
        assert!(
            indices[i] > indices[i - 1],
            "load_index must be monotonically increasing: {:?}",
            indices
        );
    }
}

// ===========================================================================
// 10. Sub-resource sharing through repeated cache loads
// ===========================================================================

const TRES_WITH_SUB: &str = r#"[gd_resource type="Resource" format=3]

[sub_resource type="RectangleShape2D" id="shape_1"]
size = Vector2(64, 32)

[resource]
name = "ShapeHolder"
shape_ref = SubResource("shape_1")
"#;

#[test]
fn repeated_load_shares_subresources() {
    let loader = InlineTresLoader::new(vec![("res://shape.tres", TRES_WITH_SUB)]);
    let mut cache = ResourceCache::new(loader);

    let r1 = cache.load("res://shape.tres").unwrap();
    let r2 = cache.load("res://shape.tres").unwrap();
    let r3 = cache.load("res://shape.tres").unwrap();

    // All are the same Arc.
    assert!(Arc::ptr_eq(&r1, &r2));
    assert!(Arc::ptr_eq(&r2, &r3));

    // Sub-resources are also shared (same top-level Arc means same internal data).
    assert!(Arc::ptr_eq(
        &r1.subresources["shape_1"],
        &r3.subresources["shape_1"]
    ));

    assert_eq!(cache.len(), 1);
}

#[test]
fn invalidated_load_produces_fresh_subresources() {
    let loader = InlineTresLoader::new(vec![("res://shape.tres", TRES_WITH_SUB)]);
    let mut cache = ResourceCache::new(loader);

    let old = cache.load("res://shape.tres").unwrap();
    let old_sub = Arc::clone(&old.subresources["shape_1"]);

    cache.invalidate("res://shape.tres");
    let fresh = cache.load("res://shape.tres").unwrap();

    assert!(!Arc::ptr_eq(&old, &fresh));
    assert!(
        !Arc::ptr_eq(&old_sub, &fresh.subresources["shape_1"]),
        "sub-resource must be fresh after invalidation"
    );

    // Data is semantically equal.
    assert_eq!(
        old_sub.get_property("size"),
        fresh.subresources["shape_1"].get_property("size")
    );
}

// ===========================================================================
// 11. Cache len/contains consistency
// ===========================================================================

#[test]
fn cache_len_consistent_through_operations() {
    let mut cache = ResourceCache::new(CountingLoader::new());

    assert_eq!(cache.len(), 0);
    assert!(cache.is_empty());

    cache.load("res://a.tres").unwrap();
    assert_eq!(cache.len(), 1);

    cache.load("res://a.tres").unwrap(); // repeated — no change
    assert_eq!(cache.len(), 1);

    cache.load("res://b.tres").unwrap();
    assert_eq!(cache.len(), 2);

    cache.invalidate("res://a.tres");
    assert_eq!(cache.len(), 1);
    assert!(!cache.contains("res://a.tres"));
    assert!(cache.contains("res://b.tres"));

    cache.load("res://a.tres").unwrap(); // re-add
    assert_eq!(cache.len(), 2);

    cache.clear();
    assert_eq!(cache.len(), 0);
    assert!(cache.is_empty());
}
