//! pat-vajb: Resource cache parity — concrete SubResource resolution.
//!
//! Broadens resource parity beyond string references by testing that
//! `SubResource("id")` property values can be resolved to their concrete
//! `Arc<Resource>` objects through `Resource::resolve_subresource`, and that
//! resolved sub-resources exhibit correct non-sharing semantics across
//! independent cache entries.
//!
//! Validates:
//! 1. `resolve_subresource` maps a property key to the concrete `Arc<Resource>`
//! 2. Resolved sub-resources have the expected class_name and typed properties
//! 3. Two properties referencing different sub-resources resolve to distinct objects
//! 4. Resolution through cache deduplication returns pointer-equal sub-resources
//! 5. Resolution from different cache entries yields independent sub-resources
//! 6. After cache invalidation, re-resolved sub-resources are fresh allocations
//! 7. Missing or non-SubResource properties return `None`

use std::sync::Arc;

use gdcore::error::EngineResult;
use gdresource::loader::TresLoader;
use gdresource::{Resource, ResourceCache, ResourceLoader};
use gdvariant::Variant;

// ---------------------------------------------------------------------------
// Inline .tres fixtures
// ---------------------------------------------------------------------------

/// A resource with one SubResource reference in a property.
const TRES_SINGLE_REF: &str = r#"[gd_resource type="Resource" format=3]

[sub_resource type="RectangleShape2D" id="shape_1"]
size = Vector2(64, 32)

[resource]
name = "ShapeHolder"
shape = SubResource("shape_1")
"#;

/// A resource with two SubResource references in separate properties.
const TRES_DUAL_REF: &str = r#"[gd_resource type="Resource" format=3]

[sub_resource type="RectangleShape2D" id="rect_1"]
size = Vector2(100, 50)

[sub_resource type="CircleShape2D" id="circle_1"]
radius = 24.0

[resource]
name = "DualHolder"
rect_shape = SubResource("rect_1")
circle_shape = SubResource("circle_1")
"#;

/// A different resource with a SubResource that shares the same id string
/// ("shape_1") but has different class and properties.
const TRES_SAME_ID_DIFFERENT_CLASS: &str = r#"[gd_resource type="Resource" format=3]

[sub_resource type="CircleShape2D" id="shape_1"]
radius = 99.0

[resource]
name = "CircleHolder"
shape = SubResource("shape_1")
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
// 1. resolve_subresource returns concrete Resource with correct class_name
// ===========================================================================

#[test]
fn resolve_subresource_returns_concrete_class() {
    let loader = TresLoader::new();
    let res = loader
        .parse_str(TRES_SINGLE_REF, "res://single.tres")
        .unwrap();

    let resolved = res
        .resolve_subresource("shape")
        .expect("resolve_subresource must find 'shape' property");

    assert_eq!(resolved.class_name, "RectangleShape2D");
}

// ===========================================================================
// 2. Resolved sub-resource has typed properties intact
// ===========================================================================

#[test]
fn resolved_subresource_has_typed_properties() {
    let loader = TresLoader::new();
    let res = loader
        .parse_str(TRES_SINGLE_REF, "res://single.tres")
        .unwrap();

    let resolved = res.resolve_subresource("shape").unwrap();

    match resolved.get_property("size") {
        Some(Variant::Vector2(v)) => {
            assert!((v.x - 64.0).abs() < f32::EPSILON, "size.x = {}", v.x);
            assert!((v.y - 32.0).abs() < f32::EPSILON, "size.y = {}", v.y);
        }
        other => panic!("expected Vector2 for resolved size, got {:?}", other),
    }
}

// ===========================================================================
// 3. Two properties referencing different sub-resources resolve to distinct
//    concrete objects with independent class_names and properties
// ===========================================================================

#[test]
fn dual_refs_resolve_to_distinct_concrete_objects() {
    let loader = TresLoader::new();
    let res = loader.parse_str(TRES_DUAL_REF, "res://dual.tres").unwrap();

    let rect = res
        .resolve_subresource("rect_shape")
        .expect("rect_shape must resolve");
    let circle = res
        .resolve_subresource("circle_shape")
        .expect("circle_shape must resolve");

    // Different class names.
    assert_eq!(rect.class_name, "RectangleShape2D");
    assert_eq!(circle.class_name, "CircleShape2D");

    // Different Arcs.
    assert!(
        !Arc::ptr_eq(rect, circle),
        "rect and circle must be distinct allocations"
    );

    // Each has its own properties.
    match rect.get_property("size") {
        Some(Variant::Vector2(v)) => assert!((v.x - 100.0).abs() < f32::EPSILON),
        other => panic!("expected Vector2 for rect size, got {:?}", other),
    }
    assert_eq!(circle.get_property("radius"), Some(&Variant::Float(24.0)));

    // Cross-check: rect has no radius, circle has no size.
    assert_eq!(rect.get_property("radius"), None);
    assert_eq!(circle.get_property("size"), None);
}

// ===========================================================================
// 4. resolve_subresource through cache deduplication returns pointer-equal
//    sub-resources on repeated loads
// ===========================================================================

#[test]
fn cached_resolve_returns_pointer_equal_subresources() {
    let loader = InlineTresLoader::new(vec![("res://single.tres", TRES_SINGLE_REF)]);
    let mut cache = ResourceCache::new(loader);

    let first = cache.load("res://single.tres").unwrap();
    let second = cache.load("res://single.tres").unwrap();

    let resolved1 = first.resolve_subresource("shape").unwrap();
    let resolved2 = second.resolve_subresource("shape").unwrap();

    assert!(
        Arc::ptr_eq(resolved1, resolved2),
        "resolved sub-resources from cached loads must be pointer-equal"
    );
    assert_eq!(resolved1.class_name, "RectangleShape2D");
}

// ===========================================================================
// 5. Same SubResource id in different .tres files resolves to independent
//    concrete objects (non-sharing across cache entries)
// ===========================================================================

#[test]
fn same_id_different_files_resolve_independently() {
    let loader = InlineTresLoader::new(vec![
        ("res://rect.tres", TRES_SINGLE_REF),
        ("res://circle.tres", TRES_SAME_ID_DIFFERENT_CLASS),
    ]);
    let mut cache = ResourceCache::new(loader);

    let rect_res = cache.load("res://rect.tres").unwrap();
    let circle_res = cache.load("res://circle.tres").unwrap();

    let rect_sub = rect_res
        .resolve_subresource("shape")
        .expect("rect resource must resolve shape");
    let circle_sub = circle_res
        .resolve_subresource("shape")
        .expect("circle resource must resolve shape");

    // Both use id "shape_1" but resolve to different concrete types.
    assert_eq!(rect_sub.class_name, "RectangleShape2D");
    assert_eq!(circle_sub.class_name, "CircleShape2D");

    // Not pointer-equal — independent allocations.
    assert!(
        !Arc::ptr_eq(rect_sub, circle_sub),
        "same id in different resources must not share sub-resource objects"
    );

    // Verify independent properties.
    match rect_sub.get_property("size") {
        Some(Variant::Vector2(v)) => assert!((v.x - 64.0).abs() < f32::EPSILON),
        other => panic!("expected Vector2, got {:?}", other),
    }
    assert_eq!(
        circle_sub.get_property("radius"),
        Some(&Variant::Float(99.0))
    );
}

// ===========================================================================
// 6. After cache invalidation, re-resolved sub-resource is a fresh allocation
// ===========================================================================

#[test]
fn invalidated_resolve_produces_fresh_subresource() {
    let loader = InlineTresLoader::new(vec![("res://single.tres", TRES_SINGLE_REF)]);
    let mut cache = ResourceCache::new(loader);

    let first = cache.load("res://single.tres").unwrap();
    let old_sub = Arc::clone(first.resolve_subresource("shape").unwrap());

    cache.invalidate("res://single.tres");
    let reloaded = cache.load("res://single.tres").unwrap();
    let new_sub = reloaded.resolve_subresource("shape").unwrap();

    // Fresh allocation.
    assert!(
        !Arc::ptr_eq(&old_sub, new_sub),
        "resolved sub-resource must be fresh after cache invalidation"
    );

    // Data is equivalent.
    assert_eq!(old_sub.class_name, new_sub.class_name);
    assert_eq!(old_sub.get_property("size"), new_sub.get_property("size"));
}

// ===========================================================================
// 7. resolve_subresource returns None for missing / non-SubResource properties
// ===========================================================================

#[test]
fn resolve_returns_none_for_missing_property() {
    let loader = TresLoader::new();
    let res = loader
        .parse_str(TRES_SINGLE_REF, "res://single.tres")
        .unwrap();

    assert!(
        res.resolve_subresource("nonexistent_key").is_none(),
        "missing property must return None"
    );
}

#[test]
fn resolve_returns_none_for_non_subresource_string() {
    let loader = TresLoader::new();
    let res = loader
        .parse_str(TRES_SINGLE_REF, "res://single.tres")
        .unwrap();

    // "name" is a plain string, not a SubResource reference.
    assert!(
        res.resolve_subresource("name").is_none(),
        "plain string property must not resolve as SubResource"
    );
}

#[test]
fn resolve_returns_none_for_dangling_subresource_ref() {
    // Manually build a resource with a SubResource: ref that has no matching entry.
    let mut res = Resource::new("TestRes");
    res.set_property(
        "broken_ref",
        Variant::String("SubResource:does_not_exist".into()),
    );

    assert!(
        res.resolve_subresource("broken_ref").is_none(),
        "dangling SubResource ref must return None, not panic"
    );
}

// ===========================================================================
// 8. Cache clear followed by re-resolve produces fresh concrete sub-resources
// ===========================================================================

#[test]
fn cache_clear_then_resolve_produces_fresh_objects() {
    let loader = InlineTresLoader::new(vec![("res://dual.tres", TRES_DUAL_REF)]);
    let mut cache = ResourceCache::new(loader);

    let first = cache.load("res://dual.tres").unwrap();
    let old_rect = Arc::clone(first.resolve_subresource("rect_shape").unwrap());
    let old_circle = Arc::clone(first.resolve_subresource("circle_shape").unwrap());

    cache.clear();
    let reloaded = cache.load("res://dual.tres").unwrap();

    let new_rect = reloaded.resolve_subresource("rect_shape").unwrap();
    let new_circle = reloaded.resolve_subresource("circle_shape").unwrap();

    // Fresh allocations.
    assert!(!Arc::ptr_eq(&old_rect, new_rect));
    assert!(!Arc::ptr_eq(&old_circle, new_circle));

    // Data equivalence.
    assert_eq!(old_rect.class_name, new_rect.class_name);
    assert_eq!(old_circle.class_name, new_circle.class_name);
    assert_eq!(
        old_circle.get_property("radius"),
        new_circle.get_property("radius")
    );
}

// ===========================================================================
// 9. Fixture round-trip: resolve sub-resources from theme.tres
// ===========================================================================

#[test]
fn fixture_theme_resolve_subresources() {
    let fixture_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../fixtures/resources/theme.tres"
    );
    let source = match std::fs::read_to_string(fixture_path) {
        Ok(s) => s,
        Err(_) => return, // skip if fixture not accessible
    };

    let loader = TresLoader::new();
    let res = loader.parse_str(&source, "res://theme.tres").unwrap();

    // Theme fixture has "panel_style" and "button_style" sub-resources.
    // Verify they can be looked up directly from the subresources map and
    // have distinct concrete properties.
    let panel = res
        .subresources
        .get("panel_style")
        .expect("panel_style sub-resource must exist");
    let button = res
        .subresources
        .get("button_style")
        .expect("button_style sub-resource must exist");

    assert_eq!(panel.class_name, "StyleBoxFlat");
    assert_eq!(button.class_name, "StyleBoxFlat");
    assert!(!Arc::ptr_eq(panel, button));

    // Verify distinct border_width values.
    assert_ne!(
        panel.get_property("border_width"),
        button.get_property("border_width"),
        "panel and button must have different border_width values"
    );
}

// ===========================================================================
// 10. Two properties referencing the SAME sub-resource ID resolve to the
//     same pointer-equal Arc
// ===========================================================================

/// A resource where two properties both reference the same sub-resource ID.
const TRES_SHARED_REF: &str = r#"[gd_resource type="Resource" format=3]

[sub_resource type="RectangleShape2D" id="shared_shape"]
size = Vector2(10, 20)

[resource]
name = "SharedRefHolder"
primary_shape = SubResource("shared_shape")
secondary_shape = SubResource("shared_shape")
"#;

#[test]
fn two_properties_same_id_resolve_to_pointer_equal_arc() {
    let loader = TresLoader::new();
    let res = loader
        .parse_str(TRES_SHARED_REF, "res://shared.tres")
        .unwrap();

    let primary = res
        .resolve_subresource("primary_shape")
        .expect("primary_shape must resolve");
    let secondary = res
        .resolve_subresource("secondary_shape")
        .expect("secondary_shape must resolve");

    // Both reference "shared_shape" — they must be the same Arc.
    assert!(
        Arc::ptr_eq(primary, secondary),
        "two properties referencing the same sub-resource ID must resolve to the same Arc"
    );
    assert_eq!(primary.class_name, "RectangleShape2D");
}

// ===========================================================================
// 11. Resolved sub-resource itself supports resolve_subresource (nested)
// ===========================================================================

#[test]
fn resolved_subresource_can_have_own_subresources() {
    // Build a resource manually where a sub-resource has its own sub-resources map.
    let mut inner_sub = Resource::new("InnerSub");
    inner_sub.set_property("value", Variant::Int(42));

    let mut outer_sub = Resource::new("OuterSub");
    outer_sub.set_property("inner_ref", Variant::String("SubResource:inner_1".into()));
    outer_sub
        .subresources
        .insert("inner_1".to_string(), Arc::new(inner_sub));

    let mut root = Resource::new("Root");
    root.set_property("outer_ref", Variant::String("SubResource:outer_1".into()));
    root.subresources
        .insert("outer_1".to_string(), Arc::new(outer_sub));

    // First level: resolve outer_ref → OuterSub.
    let outer = root
        .resolve_subresource("outer_ref")
        .expect("outer_ref must resolve");
    assert_eq!(outer.class_name, "OuterSub");

    // Second level: resolve inner_ref on the sub-resource → InnerSub.
    let inner = outer
        .resolve_subresource("inner_ref")
        .expect("inner_ref on sub-resource must resolve");
    assert_eq!(inner.class_name, "InnerSub");
    assert_eq!(inner.get_property("value"), Some(&Variant::Int(42)));
}

// ===========================================================================
// 12. Arc ref count semantics through cache + resolve_subresource
// ===========================================================================

#[test]
fn resolve_does_not_increment_strong_count() {
    let loader = InlineTresLoader::new(vec![("res://single.tres", TRES_SINGLE_REF)]);
    let mut cache = ResourceCache::new(loader);

    let res = cache.load("res://single.tres").unwrap();
    let sub_arc = &res.subresources["shape_1"];
    // sub_arc is held by the Resource only (one copy in cache, one in `res`,
    // but both point to the same Resource which owns the sub-resource map).
    let count_before = Arc::strong_count(sub_arc);

    // resolve_subresource returns a reference, not a clone — no count change.
    let _resolved = res.resolve_subresource("shape").unwrap();
    assert_eq!(
        Arc::strong_count(sub_arc),
        count_before,
        "resolve_subresource returns &Arc, must not increment strong count"
    );
}

// ===========================================================================
// 13. Non-SubResource Variant types all return None from resolve_subresource
// ===========================================================================

#[test]
fn resolve_returns_none_for_non_string_variant_types() {
    let mut res = Resource::new("VariantTest");
    res.set_property("int_prop", Variant::Int(99));
    res.set_property("bool_prop", Variant::Bool(true));
    res.set_property("float_prop", Variant::Float(3.14));
    res.set_property("nil_prop", Variant::Nil);

    assert!(
        res.resolve_subresource("int_prop").is_none(),
        "Int must not resolve"
    );
    assert!(
        res.resolve_subresource("bool_prop").is_none(),
        "Bool must not resolve"
    );
    assert!(
        res.resolve_subresource("float_prop").is_none(),
        "Float must not resolve"
    );
    assert!(
        res.resolve_subresource("nil_prop").is_none(),
        "Nil must not resolve"
    );
}

// ===========================================================================
// 14. ExtResource string prefix does NOT resolve as SubResource
// ===========================================================================

#[test]
fn ext_resource_ref_does_not_resolve_as_subresource() {
    let mut res = Resource::new("ExtRefTest");
    // ExtResource references use "ExtResource:" prefix, not "SubResource:".
    res.set_property("texture", Variant::String("ExtResource:1".into()));
    // Also add a sub-resource to ensure the lookup isn't confused.
    let mut sub = Resource::new("SomeSubRes");
    sub.set_property("x", Variant::Int(1));
    res.subresources.insert("1".to_string(), Arc::new(sub));

    assert!(
        res.resolve_subresource("texture").is_none(),
        "ExtResource: prefix must not resolve through resolve_subresource"
    );
}

// ===========================================================================
// 15. Shared-ref sub-resource through cache: both properties stay pointer-equal
//     after cache round-trip
// ===========================================================================

#[test]
fn shared_ref_subresource_stays_equal_through_cache() {
    let loader = InlineTresLoader::new(vec![("res://shared.tres", TRES_SHARED_REF)]);
    let mut cache = ResourceCache::new(loader);

    let first = cache.load("res://shared.tres").unwrap();
    let second = cache.load("res://shared.tres").unwrap();

    // Cache deduplication: same top-level Arc.
    assert!(Arc::ptr_eq(&first, &second));

    // Both loads resolve the shared sub-resource to the same Arc.
    let p1 = first.resolve_subresource("primary_shape").unwrap();
    let s2 = second.resolve_subresource("secondary_shape").unwrap();
    assert!(
        Arc::ptr_eq(p1, s2),
        "shared sub-resource must be pointer-equal across cached loads"
    );
}

// ===========================================================================
// 16. After invalidation, shared-ref sub-resource in reloaded resource is
//     fresh but internally consistent (both refs still point to same Arc)
// ===========================================================================

#[test]
fn invalidated_shared_ref_produces_fresh_but_internally_consistent_arcs() {
    let loader = InlineTresLoader::new(vec![("res://shared.tres", TRES_SHARED_REF)]);
    let mut cache = ResourceCache::new(loader);

    let original = cache.load("res://shared.tres").unwrap();
    let old_primary = Arc::clone(original.resolve_subresource("primary_shape").unwrap());

    cache.invalidate("res://shared.tres");
    let reloaded = cache.load("res://shared.tres").unwrap();

    // Fresh top-level allocation.
    assert!(!Arc::ptr_eq(&original, &reloaded));

    // Fresh sub-resource allocation.
    let new_primary = reloaded.resolve_subresource("primary_shape").unwrap();
    assert!(
        !Arc::ptr_eq(&old_primary, new_primary),
        "reloaded sub-resource must be a fresh Arc"
    );

    // But within the reloaded resource, both refs still share the same Arc.
    let new_secondary = reloaded.resolve_subresource("secondary_shape").unwrap();
    assert!(
        Arc::ptr_eq(new_primary, new_secondary),
        "within the reloaded resource, shared refs must still alias"
    );
}
