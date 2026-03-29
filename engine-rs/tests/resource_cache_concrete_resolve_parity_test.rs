//! pat-8fvf + pat-fn8k: Resource cache parity — concrete SubResource resolution with
//! coexisting ExtResource entries and multi-operation cache semantics.
//!
//! Broadens resource cache parity coverage beyond the basic resolve/non-sharing
//! tests in pat-vajb by exercising:
//!
//! 1. SubResource resolution coexisting with ExtResource entries (no cross-talk)
//! 2. Fixture round-trip for `with_ext_refs.tres` (mixed ext + sub resources)
//! 3. Cache counts only top-level resources, not sub-resources
//! 4. Selective invalidation preserves sub-resources of unaffected cache entries
//! 5. Load/invalidate/reload cycle stability for sub-resource resolution
//! 6. Multiple resources sharing the same sub-resource class but different ids
//! 7. resolve_subresource is consistent before and after caching a second resource
//! 8. Sub-resource map isolation: adding to one cache entry doesn't leak to another

use std::sync::Arc;

use gdcore::error::EngineResult;
use gdresource::loader::TresLoader;
use gdresource::{Resource, ResourceCache, ResourceLoader};
use gdvariant::Variant;

// ---------------------------------------------------------------------------
// Inline .tres fixtures
// ---------------------------------------------------------------------------

/// Resource with both ext_resource and sub_resource sections.
const TRES_MIXED: &str = r#"[gd_resource type="PackedScene" format=3 uid="uid://mixed_test"]

[ext_resource type="Texture2D" uid="uid://icon" path="res://icon.png" id="1"]
[ext_resource type="Script" uid="uid://script" path="res://player.gd" id="2"]

[sub_resource type="StyleBoxFlat" id="inline_style"]
bg_color = Color(1.0, 0.0, 0.0, 1.0)
border_width = 3

[sub_resource type="RectangleShape2D" id="collision_shape"]
size = Vector2(32, 16)

[resource]
name = "MixedScene"
style = SubResource("inline_style")
shape = SubResource("collision_shape")
"#;

/// Simple resource with one sub-resource for cache interaction tests.
const TRES_ALPHA: &str = r#"[gd_resource type="Resource" format=3]

[sub_resource type="RectangleShape2D" id="alpha_shape"]
size = Vector2(10, 20)

[resource]
name = "Alpha"
shape = SubResource("alpha_shape")
"#;

/// Another simple resource for cache interaction tests.
const TRES_BETA: &str = r#"[gd_resource type="Resource" format=3]

[sub_resource type="CircleShape2D" id="beta_shape"]
radius = 50.0

[resource]
name = "Beta"
shape = SubResource("beta_shape")
"#;

/// Two sub-resources of the same class but different IDs and properties.
const TRES_SAME_CLASS_MULTI: &str = r#"[gd_resource type="Resource" format=3]

[sub_resource type="RectangleShape2D" id="rect_a"]
size = Vector2(100, 50)

[sub_resource type="RectangleShape2D" id="rect_b"]
size = Vector2(200, 100)

[resource]
name = "MultiRect"
first = SubResource("rect_a")
second = SubResource("rect_b")
"#;

/// A loader that serves inline .tres content by path.
struct InlineTresLoader {
    entries: Vec<(&'static str, &'static str)>,
}

impl InlineTresLoader {
    fn new(entries: Vec<(&'static str, &'static str)>) -> Self {
        Self { entries }
    }
}

impl ResourceLoader for InlineTresLoader {
    fn load(&self, path: &str) -> EngineResult<Arc<Resource>> {
        for (p, content) in &self.entries {
            if *p == path {
                let tres = TresLoader::new();
                return tres.parse_str(content, path);
            }
        }
        Err(gdcore::error::EngineError::Parse(format!(
            "InlineTresLoader: unknown path: {path}"
        )))
    }
}

// ===========================================================================
// 1. SubResource resolution coexists with ExtResource entries
// ===========================================================================

#[test]
fn subresource_resolve_with_ext_resources_present() {
    let loader = TresLoader::new();
    let res = loader.parse_str(TRES_MIXED, "res://mixed.tres").unwrap();

    // ExtResource entries are parsed.
    assert!(
        !res.ext_resources.is_empty(),
        "ext_resources must be populated"
    );

    // SubResource resolution still works.
    let style = res
        .resolve_subresource("style")
        .expect("style must resolve to inline_style");
    assert_eq!(style.class_name, "StyleBoxFlat");

    let shape = res
        .resolve_subresource("shape")
        .expect("shape must resolve to collision_shape");
    assert_eq!(shape.class_name, "RectangleShape2D");

    // ExtResource entries do not appear in the subresources map.
    assert!(
        res.subresources.get("1").is_none(),
        "ext_resource id '1' must not appear in subresources"
    );
    assert!(
        res.subresources.get("2").is_none(),
        "ext_resource id '2' must not appear in subresources"
    );
}

// ===========================================================================
// 2. SubResource and ExtResource maps are independent
// ===========================================================================

#[test]
fn ext_resource_and_subresource_maps_are_disjoint() {
    let loader = TresLoader::new();
    let res = loader.parse_str(TRES_MIXED, "res://mixed.tres").unwrap();

    // Verify ext_resources and subresources have no overlapping keys.
    for key in res.ext_resources.keys() {
        assert!(
            !res.subresources.contains_key(key),
            "ext_resource key '{}' must not appear in subresources map",
            key
        );
    }
    for key in res.subresources.keys() {
        assert!(
            !res.ext_resources.contains_key(key),
            "sub_resource key '{}' must not appear in ext_resources map",
            key
        );
    }
}

// ===========================================================================
// 3. Fixture round-trip: with_ext_refs.tres (mixed ext + sub resources)
// ===========================================================================

#[test]
fn fixture_with_ext_refs_has_both_resource_types() {
    let fixture_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../fixtures/resources/with_ext_refs.tres"
    );
    let source = match std::fs::read_to_string(fixture_path) {
        Ok(s) => s,
        Err(_) => return, // skip if fixture not accessible
    };

    let loader = TresLoader::new();
    let res = loader
        .parse_str(&source, "res://with_ext_refs.tres")
        .unwrap();

    // Must have ext_resources.
    assert!(
        !res.ext_resources.is_empty(),
        "fixture must have ext_resources"
    );

    // Must have the inline_style sub-resource.
    let style = res
        .subresources
        .get("inline_style")
        .expect("inline_style sub-resource must exist");
    assert_eq!(style.class_name, "StyleBoxFlat");

    // Verify typed property on the sub-resource.
    assert_eq!(style.get_property("border_width"), Some(&Variant::Int(3)));

    // Verify bg_color is a Color variant.
    match style.get_property("bg_color") {
        Some(Variant::Color(c)) => {
            assert!((c.r - 1.0).abs() < 0.01, "bg_color.r should be 1.0");
        }
        other => panic!("expected Color for bg_color, got {:?}", other),
    }
}

// ===========================================================================
// 4. Cache counts only top-level resources, not sub-resources
// ===========================================================================

#[test]
fn cache_len_counts_top_level_only() {
    let loader = InlineTresLoader::new(vec![
        ("res://mixed.tres", TRES_MIXED),
        ("res://alpha.tres", TRES_ALPHA),
    ]);
    let mut cache = ResourceCache::new(loader);

    cache.load("res://mixed.tres").unwrap();
    // TRES_MIXED has 2 sub-resources, but cache.len() should still be 1.
    assert_eq!(cache.len(), 1, "cache must count only top-level resources");

    cache.load("res://alpha.tres").unwrap();
    assert_eq!(cache.len(), 2, "two top-level resources loaded");
}

// ===========================================================================
// 5. Selective invalidation preserves sub-resources of other cache entries
// ===========================================================================

#[test]
fn invalidate_one_preserves_other_subresources() {
    let loader = InlineTresLoader::new(vec![
        ("res://alpha.tres", TRES_ALPHA),
        ("res://beta.tres", TRES_BETA),
    ]);
    let mut cache = ResourceCache::new(loader);

    let alpha = cache.load("res://alpha.tres").unwrap();
    let beta = cache.load("res://beta.tres").unwrap();

    let alpha_sub = Arc::clone(alpha.resolve_subresource("shape").unwrap());
    let beta_sub = Arc::clone(beta.resolve_subresource("shape").unwrap());

    // Invalidate only alpha.
    cache.invalidate("res://alpha.tres");

    // Beta's sub-resource is untouched.
    let beta_again = cache.load("res://beta.tres").unwrap();
    assert!(Arc::ptr_eq(&beta, &beta_again), "beta must still be cached");
    let beta_sub_again = beta_again.resolve_subresource("shape").unwrap();
    assert!(
        Arc::ptr_eq(&beta_sub, beta_sub_again),
        "beta's sub-resource must be preserved after alpha invalidation"
    );

    // Alpha reloads as fresh.
    let alpha_new = cache.load("res://alpha.tres").unwrap();
    assert!(!Arc::ptr_eq(&alpha, &alpha_new));
    let alpha_sub_new = alpha_new.resolve_subresource("shape").unwrap();
    assert!(
        !Arc::ptr_eq(&alpha_sub, alpha_sub_new),
        "alpha's sub-resource must be fresh after invalidation"
    );
}

// ===========================================================================
// 6. Load/invalidate/reload cycle stability
// ===========================================================================

#[test]
fn load_invalidate_reload_cycle_stable() {
    let loader = InlineTresLoader::new(vec![("res://alpha.tres", TRES_ALPHA)]);
    let mut cache = ResourceCache::new(loader);

    let mut prev_sub: Option<Arc<Resource>> = None;

    for i in 0..5 {
        let res = cache.load("res://alpha.tres").unwrap();
        let sub = res.resolve_subresource("shape").unwrap();

        // Class and properties must always be correct.
        assert_eq!(sub.class_name, "RectangleShape2D", "cycle {i}: class_name");
        match sub.get_property("size") {
            Some(Variant::Vector2(v)) => {
                assert!((v.x - 10.0).abs() < f32::EPSILON, "cycle {i}: size.x");
                assert!((v.y - 20.0).abs() < f32::EPSILON, "cycle {i}: size.y");
            }
            other => panic!("cycle {i}: expected Vector2, got {:?}", other),
        }

        if let Some(ref prev) = prev_sub {
            // After invalidation, sub-resource must be a fresh allocation.
            assert!(
                !Arc::ptr_eq(prev, sub),
                "cycle {i}: sub-resource must be fresh after invalidation"
            );
        }

        prev_sub = Some(Arc::clone(sub));
        cache.invalidate("res://alpha.tres");
    }
}

// ===========================================================================
// 7. Same class, different IDs — distinct resolution
// ===========================================================================

#[test]
fn same_class_different_ids_resolve_distinctly() {
    let loader = TresLoader::new();
    let res = loader
        .parse_str(TRES_SAME_CLASS_MULTI, "res://multi.tres")
        .unwrap();

    let first = res
        .resolve_subresource("first")
        .expect("first must resolve");
    let second = res
        .resolve_subresource("second")
        .expect("second must resolve");

    // Same class but different allocations.
    assert_eq!(first.class_name, "RectangleShape2D");
    assert_eq!(second.class_name, "RectangleShape2D");
    assert!(
        !Arc::ptr_eq(first, second),
        "same class, different IDs must not alias"
    );

    // Different property values.
    match (first.get_property("size"), second.get_property("size")) {
        (Some(Variant::Vector2(a)), Some(Variant::Vector2(b))) => {
            assert!((a.x - 100.0).abs() < f32::EPSILON, "first.size.x");
            assert!((b.x - 200.0).abs() < f32::EPSILON, "second.size.x");
        }
        (a, b) => panic!("expected Vector2 for both, got {:?} / {:?}", a, b),
    }
}

// ===========================================================================
// 8. resolve_subresource is consistent before and after caching a second
//    resource
// ===========================================================================

#[test]
fn resolve_consistent_across_cache_growth() {
    let loader = InlineTresLoader::new(vec![
        ("res://alpha.tres", TRES_ALPHA),
        ("res://beta.tres", TRES_BETA),
    ]);
    let mut cache = ResourceCache::new(loader);

    // Load alpha, resolve its sub-resource.
    let alpha = cache.load("res://alpha.tres").unwrap();
    let alpha_sub_before = Arc::clone(alpha.resolve_subresource("shape").unwrap());

    // Load beta — cache grows but alpha is untouched.
    let _beta = cache.load("res://beta.tres").unwrap();

    // Re-load alpha from cache and resolve again.
    let alpha_again = cache.load("res://alpha.tres").unwrap();
    let alpha_sub_after = alpha_again.resolve_subresource("shape").unwrap();

    assert!(
        Arc::ptr_eq(&alpha_sub_before, alpha_sub_after),
        "alpha's sub-resource must be pointer-equal before and after loading beta"
    );
}

// ===========================================================================
// 9. Mixed resource through cache: sub-resource resolve + ext_resource
//    independence
// ===========================================================================

#[test]
fn mixed_resource_cached_resolve_and_ext_independence() {
    let loader = InlineTresLoader::new(vec![("res://mixed.tres", TRES_MIXED)]);
    let mut cache = ResourceCache::new(loader);

    let first = cache.load("res://mixed.tres").unwrap();
    let second = cache.load("res://mixed.tres").unwrap();

    // Cache dedup: same Arc.
    assert!(Arc::ptr_eq(&first, &second));

    // Sub-resource resolution through cached resource.
    let style1 = first.resolve_subresource("style").unwrap();
    let style2 = second.resolve_subresource("style").unwrap();
    assert!(Arc::ptr_eq(style1, style2));

    // ExtResource entries are also identical (same Arc, same data).
    assert_eq!(first.ext_resources.len(), second.ext_resources.len());
    assert_eq!(first.ext_resources["1"].path, "res://icon.png");
}

// ===========================================================================
// 10. Sub-resource map isolation after clone: mutating clone doesn't leak
// ===========================================================================

#[test]
fn subresource_map_isolated_after_clone() {
    let loader = TresLoader::new();
    let res = loader.parse_str(TRES_ALPHA, "res://alpha.tres").unwrap();

    // Clone the resource and add a new sub-resource to the clone.
    let mut cloned = (*res).clone();
    let extra = Resource::new("ExtraSub");
    cloned
        .subresources
        .insert("extra_id".to_string(), Arc::new(extra));

    // Original must not see the new sub-resource.
    assert!(
        res.subresources.get("extra_id").is_none(),
        "original must not gain sub-resources added to clone"
    );
    assert_eq!(res.subresources.len(), 1);
    assert_eq!(cloned.subresources.len(), 2);
}

// ===========================================================================
// 11. Cache invalidation of mixed resource: ext_resources are re-parsed too
// ===========================================================================

#[test]
fn invalidated_mixed_resource_has_fresh_ext_resources() {
    let loader = InlineTresLoader::new(vec![("res://mixed.tres", TRES_MIXED)]);
    let mut cache = ResourceCache::new(loader);

    let original = cache.load("res://mixed.tres").unwrap();
    let orig_ext_count = original.ext_resources.len();

    cache.invalidate("res://mixed.tres");
    let reloaded = cache.load("res://mixed.tres").unwrap();

    // Fresh allocation.
    assert!(!Arc::ptr_eq(&original, &reloaded));

    // Ext resources re-parsed with same content.
    assert_eq!(reloaded.ext_resources.len(), orig_ext_count);
    assert_eq!(reloaded.ext_resources["1"].path, "res://icon.png");

    // Sub-resources also fresh.
    let old_style = &original.subresources["inline_style"];
    let new_style = &reloaded.subresources["inline_style"];
    assert!(
        !Arc::ptr_eq(old_style, new_style),
        "sub-resource must be fresh after invalidation of mixed resource"
    );
    assert_eq!(old_style.class_name, new_style.class_name);
}

// ===========================================================================
// pat-fn8k tests: Additional concrete SubResource resolution parity coverage
// ===========================================================================

// ===========================================================================
// 12. Overlapping sub-resource IDs across different cache entries resolve
//     to the correct concrete type (no cross-entry contamination)
// ===========================================================================

/// Two resources that both use id "shape_1" but with different concrete types.
const TRES_RECT_SHAPE: &str = r#"[gd_resource type="Resource" format=3]

[sub_resource type="RectangleShape2D" id="shape_1"]
size = Vector2(80, 40)

[resource]
name = "RectHolder"
shape = SubResource("shape_1")
"#;

const TRES_CIRCLE_SHAPE: &str = r#"[gd_resource type="Resource" format=3]

[sub_resource type="CircleShape2D" id="shape_1"]
radius = 42.0

[resource]
name = "CircleHolder"
shape = SubResource("shape_1")
"#;

#[test]
fn overlapping_subresource_ids_resolve_correct_type_across_cache() {
    let loader = InlineTresLoader::new(vec![
        ("res://rect.tres", TRES_RECT_SHAPE),
        ("res://circle.tres", TRES_CIRCLE_SHAPE),
    ]);
    let mut cache = ResourceCache::new(loader);

    let rect_res = cache.load("res://rect.tres").unwrap();
    let circle_res = cache.load("res://circle.tres").unwrap();

    // Both use id "shape_1" internally, but resolve to their own concrete type.
    let rect_sub = rect_res.resolve_subresource("shape").unwrap();
    let circle_sub = circle_res.resolve_subresource("shape").unwrap();

    assert_eq!(rect_sub.class_name, "RectangleShape2D");
    assert_eq!(circle_sub.class_name, "CircleShape2D");
    assert!(!Arc::ptr_eq(rect_sub, circle_sub));

    // Properties are from the correct sub-resource, not cross-contaminated.
    match rect_sub.get_property("size") {
        Some(Variant::Vector2(v)) => {
            assert!((v.x - 80.0).abs() < f32::EPSILON, "rect size.x");
            assert!((v.y - 40.0).abs() < f32::EPSILON, "rect size.y");
        }
        other => panic!("expected Vector2, got {:?}", other),
    }
    assert_eq!(
        circle_sub.get_property("radius"),
        Some(&Variant::Float(42.0))
    );

    // Cross-check: rect has no radius, circle has no size.
    assert!(rect_sub.get_property("radius").is_none());
    assert!(circle_sub.get_property("size").is_none());
}

// ===========================================================================
// 13. cache.contains / is_empty semantics with sub-resource-bearing resources
// ===========================================================================

#[test]
fn cache_contains_tracks_top_level_paths_not_subresource_ids() {
    let loader = InlineTresLoader::new(vec![("res://alpha.tres", TRES_ALPHA)]);
    let mut cache = ResourceCache::new(loader);

    assert!(cache.is_empty());
    assert!(!cache.contains("res://alpha.tres"));

    cache.load("res://alpha.tres").unwrap();

    assert!(!cache.is_empty());
    assert!(cache.contains("res://alpha.tres"));

    // Sub-resource IDs are NOT cache keys.
    assert!(
        !cache.contains("alpha_shape"),
        "sub-resource id must not appear as a cache key"
    );
    assert!(
        !cache.contains("SubResource:alpha_shape"),
        "SubResource: prefixed id must not be a cache key"
    );
}

// ===========================================================================
// 14. resolve_subresource on resource with zero sub-resources returns None
// ===========================================================================

const TRES_NO_SUBS: &str = r#"[gd_resource type="Resource" format=3]

[resource]
name = "Bare"
value = 123
"#;

#[test]
fn resolve_on_resource_with_no_subresources_returns_none() {
    let loader = TresLoader::new();
    let res = loader.parse_str(TRES_NO_SUBS, "res://bare.tres").unwrap();

    assert!(res.subresources.is_empty(), "must have no sub-resources");
    assert!(
        res.resolve_subresource("value").is_none(),
        "non-SubResource property on sub-resource-free resource must return None"
    );
    assert!(
        res.resolve_subresource("name").is_none(),
        "plain string property must not resolve"
    );
    assert!(
        res.resolve_subresource("missing").is_none(),
        "missing property must return None"
    );
}

// ===========================================================================
// 15. Thread-safe read access to resolved sub-resources
// ===========================================================================

#[test]
fn resolved_subresources_are_send_and_sync() {
    let loader = TresLoader::new();
    let res = loader.parse_str(TRES_MIXED, "res://mixed.tres").unwrap();

    let style = Arc::clone(res.resolve_subresource("style").unwrap());

    // Spawn a thread to read the resolved sub-resource — proves Send + Sync.
    let handle = std::thread::spawn(move || {
        assert_eq!(style.class_name, "StyleBoxFlat");
        match style.get_property("bg_color") {
            Some(Variant::Color(c)) => {
                assert!((c.r - 1.0).abs() < 0.01);
            }
            other => panic!("expected Color, got {:?}", other),
        }
        assert_eq!(style.get_property("border_width"), Some(&Variant::Int(3)));
    });

    handle.join().expect("thread must not panic");
}

// ===========================================================================
// 16. Invalidate + reload: sub-resource of reloaded resource is semantically
//     equal but structurally independent from the old one
// ===========================================================================

#[test]
fn invalidate_reload_subresource_semantic_equality_structural_independence() {
    let loader = InlineTresLoader::new(vec![("res://multi.tres", TRES_SAME_CLASS_MULTI)]);
    let mut cache = ResourceCache::new(loader);

    let original = cache.load("res://multi.tres").unwrap();
    let old_first = Arc::clone(original.resolve_subresource("first").unwrap());
    let old_second = Arc::clone(original.resolve_subresource("second").unwrap());

    cache.invalidate("res://multi.tres");
    let reloaded = cache.load("res://multi.tres").unwrap();

    let new_first = reloaded.resolve_subresource("first").unwrap();
    let new_second = reloaded.resolve_subresource("second").unwrap();

    // Structurally independent (different Arcs).
    assert!(!Arc::ptr_eq(&old_first, new_first));
    assert!(!Arc::ptr_eq(&old_second, new_second));

    // Semantically equal (same class + properties).
    assert_eq!(old_first.class_name, new_first.class_name);
    assert_eq!(old_second.class_name, new_second.class_name);
    assert_eq!(
        old_first.get_property("size"),
        new_first.get_property("size")
    );
    assert_eq!(
        old_second.get_property("size"),
        new_second.get_property("size")
    );

    // The two sub-resources within the reloaded resource are still distinct.
    assert!(!Arc::ptr_eq(new_first, new_second));
}

// ===========================================================================
// 17. Multiple invalidation cycles: sub-resource Arcs are unique per reload
// ===========================================================================

#[test]
fn multiple_invalidation_cycles_produce_unique_subresource_arcs() {
    let loader = InlineTresLoader::new(vec![("res://alpha.tres", TRES_ALPHA)]);
    let mut cache = ResourceCache::new(loader);

    let mut seen_arcs: Vec<Arc<Resource>> = Vec::new();

    for _ in 0..4 {
        let res = cache.load("res://alpha.tres").unwrap();
        let sub = Arc::clone(res.resolve_subresource("shape").unwrap());

        // Each reload after invalidation must produce an Arc distinct from all previous ones.
        for prev in &seen_arcs {
            assert!(
                !Arc::ptr_eq(prev, &sub),
                "sub-resource Arc must be unique per reload cycle"
            );
        }

        seen_arcs.push(sub);
        cache.invalidate("res://alpha.tres");
    }

    assert_eq!(seen_arcs.len(), 4);
}

// ===========================================================================
// 18. Clearing cache with multiple sub-resource-bearing entries: all
//     sub-resources from all entries become independent
// ===========================================================================

#[test]
fn clear_releases_all_subresource_bearing_entries() {
    let loader = InlineTresLoader::new(vec![
        ("res://alpha.tres", TRES_ALPHA),
        ("res://beta.tres", TRES_BETA),
        ("res://multi.tres", TRES_SAME_CLASS_MULTI),
    ]);
    let mut cache = ResourceCache::new(loader);

    let alpha = cache.load("res://alpha.tres").unwrap();
    let beta = cache.load("res://beta.tres").unwrap();
    let multi = cache.load("res://multi.tres").unwrap();
    assert_eq!(cache.len(), 3);

    let alpha_sub = Arc::clone(alpha.resolve_subresource("shape").unwrap());
    let beta_sub = Arc::clone(beta.resolve_subresource("shape").unwrap());
    let multi_first = Arc::clone(multi.resolve_subresource("first").unwrap());

    cache.clear();
    assert!(cache.is_empty());

    // All top-level resources now have strong_count == 1 (only local ref).
    assert_eq!(Arc::strong_count(&alpha), 1);
    assert_eq!(Arc::strong_count(&beta), 1);
    assert_eq!(Arc::strong_count(&multi), 1);

    // Reload and verify all sub-resources are fresh.
    let alpha2 = cache.load("res://alpha.tres").unwrap();
    let alpha_sub2 = alpha2.resolve_subresource("shape").unwrap();
    assert!(!Arc::ptr_eq(&alpha_sub, alpha_sub2));

    let beta2 = cache.load("res://beta.tres").unwrap();
    let beta_sub2 = beta2.resolve_subresource("shape").unwrap();
    assert!(!Arc::ptr_eq(&beta_sub, beta_sub2));

    let multi2 = cache.load("res://multi.tres").unwrap();
    let multi_first2 = multi2.resolve_subresource("first").unwrap();
    assert!(!Arc::ptr_eq(&multi_first, multi_first2));
}
