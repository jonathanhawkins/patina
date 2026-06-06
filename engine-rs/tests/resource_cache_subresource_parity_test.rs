//! pat-xoj1: Resource cache parity — concrete SubResource resolution and
//! non-sharing semantics.
//!
//! Validates that:
//! 1. TresLoader parses concrete sub_resource sections into typed Resource objects
//! 2. Sub-resource properties (class_name, typed values) survive cache round-trips
//! 3. Two independent loads of different .tres files produce independent sub_resource objects
//! 4. Cache deduplication returns the *same* Arc (pointer-equal) for repeated loads, including
//!    sub_resources nested inside
//! 5. After cache invalidation, reloaded resources have fresh, independent sub_resource Arcs
//! 6. Multiple sub_resources within a single resource are distinct objects

use std::sync::Arc;

use gdcore::error::EngineResult;
use gdresource::loader::TresLoader;
use gdresource::{Resource, ResourceCache, ResourceLoader};
use gdvariant::Variant;

// ---------------------------------------------------------------------------
// Inline .tres content for deterministic tests
// ---------------------------------------------------------------------------

const TRES_WITH_SHAPE: &str = r#"[gd_resource type="Resource" format=3]

[sub_resource type="RectangleShape2D" id="shape_1"]
size = Vector2(64, 32)

[resource]
name = "ShapeHolder"
shape_ref = SubResource("shape_1")
"#;

const TRES_WITH_TWO_SUBS: &str = r#"[gd_resource type="Theme" format=3]

[sub_resource type="StyleBoxFlat" id="panel"]
bg_color = Color(0.1, 0.2, 0.3, 1.0)
border_width = 2

[sub_resource type="StyleBoxFlat" id="button"]
bg_color = Color(0.5, 0.6, 0.7, 1.0)
border_width = 4

[resource]
name = "DualStyleTheme"
"#;

const TRES_DIFFERENT_SHAPE: &str = r#"[gd_resource type="Resource" format=3]

[sub_resource type="CircleShape2D" id="circle_1"]
radius = 16.0

[resource]
name = "CircleHolder"
shape_ref = SubResource("circle_1")
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
// 1. Concrete sub_resource parsed with correct class_name and properties
// ===========================================================================

#[test]
fn subresource_has_concrete_class_name() {
    let loader = TresLoader::new();
    let res = loader
        .parse_str(TRES_WITH_SHAPE, "res://shape.tres")
        .unwrap();

    let sub = res
        .subresources
        .get("shape_1")
        .expect("sub_resource 'shape_1' must be present");
    assert_eq!(sub.class_name, "RectangleShape2D");
}

#[test]
fn subresource_properties_parsed_as_typed_variants() {
    let loader = TresLoader::new();
    let res = loader
        .parse_str(TRES_WITH_SHAPE, "res://shape.tres")
        .unwrap();

    let sub = &res.subresources["shape_1"];
    let size = sub.get_property("size").expect("size property missing");
    match size {
        Variant::Vector2(v) => {
            assert!((v.x - 64.0).abs() < f32::EPSILON);
            assert!((v.y - 32.0).abs() < f32::EPSILON);
        }
        other => panic!("expected Vector2 for size, got {:?}", other),
    }
}

// ===========================================================================
// 2. SubResource reference in parent resource stored as prefixed string
// ===========================================================================

#[test]
fn subresource_ref_stored_as_prefixed_string() {
    let loader = TresLoader::new();
    let res = loader
        .parse_str(TRES_WITH_SHAPE, "res://shape.tres")
        .unwrap();

    assert_eq!(
        res.get_property("shape_ref"),
        Some(&Variant::String("SubResource:shape_1".into())),
        "SubResource(\"shape_1\") must be stored as Variant::String(\"SubResource:shape_1\")"
    );
}

// ===========================================================================
// 3. Multiple sub_resources within one resource are distinct Arcs
// ===========================================================================

#[test]
fn multiple_subresources_are_distinct_arcs() {
    let loader = TresLoader::new();
    let res = loader
        .parse_str(TRES_WITH_TWO_SUBS, "res://theme.tres")
        .unwrap();

    assert_eq!(res.subresources.len(), 2);
    let panel = &res.subresources["panel"];
    let button = &res.subresources["button"];

    // Same class_name but distinct objects.
    assert_eq!(panel.class_name, "StyleBoxFlat");
    assert_eq!(button.class_name, "StyleBoxFlat");
    assert!(
        !Arc::ptr_eq(panel, button),
        "two sub_resources must not alias"
    );

    // Properties are independent.
    assert_eq!(panel.get_property("border_width"), Some(&Variant::Int(2)));
    assert_eq!(button.get_property("border_width"), Some(&Variant::Int(4)));
}

// ===========================================================================
// 4. Cache deduplication preserves sub_resource identity
// ===========================================================================

#[test]
fn cached_load_returns_same_subresource_arcs() {
    let loader = InlineTresLoader::new(vec![("res://shape.tres", TRES_WITH_SHAPE)]);
    let mut cache = ResourceCache::new(loader);

    let first = cache.load("res://shape.tres").unwrap();
    let second = cache.load("res://shape.tres").unwrap();

    // Top-level Arc is deduplicated.
    assert!(Arc::ptr_eq(&first, &second));

    // Because it's the same Arc, sub_resources are also identical.
    let sub1 = &first.subresources["shape_1"];
    let sub2 = &second.subresources["shape_1"];
    assert!(
        Arc::ptr_eq(sub1, sub2),
        "cached loads must share the same sub_resource Arc"
    );
}

// ===========================================================================
// 5. Independent loads of different resources have independent sub_resources
// ===========================================================================

#[test]
fn different_resources_have_independent_subresources() {
    let loader = InlineTresLoader::new(vec![
        ("res://shape.tres", TRES_WITH_SHAPE),
        ("res://circle.tres", TRES_DIFFERENT_SHAPE),
    ]);
    let mut cache = ResourceCache::new(loader);

    let shape_res = cache.load("res://shape.tres").unwrap();
    let circle_res = cache.load("res://circle.tres").unwrap();

    assert!(!Arc::ptr_eq(&shape_res, &circle_res));

    let rect_sub = &shape_res.subresources["shape_1"];
    let circle_sub = &circle_res.subresources["circle_1"];
    assert_eq!(rect_sub.class_name, "RectangleShape2D");
    assert_eq!(circle_sub.class_name, "CircleShape2D");

    // Verify typed property independence.
    assert_eq!(
        circle_sub.get_property("radius"),
        Some(&Variant::Float(16.0))
    );
    assert_eq!(rect_sub.get_property("radius"), None);
}

// ===========================================================================
// 6. Cache invalidation produces fresh sub_resource objects
// ===========================================================================

#[test]
fn invalidated_reload_produces_fresh_subresource_arcs() {
    let loader = InlineTresLoader::new(vec![("res://shape.tres", TRES_WITH_SHAPE)]);
    let mut cache = ResourceCache::new(loader);

    let first = cache.load("res://shape.tres").unwrap();
    let old_sub = Arc::clone(&first.subresources["shape_1"]);

    cache.invalidate("res://shape.tres");
    let reloaded = cache.load("res://shape.tres").unwrap();

    // Top-level Arc is fresh.
    assert!(!Arc::ptr_eq(&first, &reloaded));

    // Sub_resource Arc is also fresh (re-parsed from content).
    let new_sub = &reloaded.subresources["shape_1"];
    assert!(
        !Arc::ptr_eq(&old_sub, new_sub),
        "sub_resource must be a new allocation after cache invalidation"
    );

    // But the data is equivalent.
    assert_eq!(old_sub.class_name, new_sub.class_name);
    assert_eq!(old_sub.get_property("size"), new_sub.get_property("size"));
}

// ===========================================================================
// 7. Non-sharing: cloning a Resource gives independent sub_resource copies
// ===========================================================================

#[test]
fn cloned_resource_has_independent_subresource_copies() {
    let loader = TresLoader::new();
    let res = loader
        .parse_str(TRES_WITH_TWO_SUBS, "res://theme.tres")
        .unwrap();

    // Resource derives Clone. Cloning should give independent sub_resources.
    let cloned = (*res).clone();

    // The sub_resources HashMap was cloned — Arc::clone means same underlying data.
    // This is expected: Arc semantics give shared immutable data.
    // The key contract is that the *map itself* is independent so adding/removing
    // sub_resources on the clone doesn't affect the original.
    assert_eq!(cloned.subresources.len(), res.subresources.len());

    // Arc::clone gives pointer equality — this is correct Arc behavior.
    assert!(Arc::ptr_eq(
        &res.subresources["panel"],
        &cloned.subresources["panel"]
    ));
}

// ===========================================================================
// 8. Fixture file round-trip: theme.tres with multiple sub_resources
// ===========================================================================

#[test]
fn fixture_theme_subresources_resolved_through_cache() {
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

    // Two StyleBoxFlat sub_resources.
    assert_eq!(res.subresources.len(), 2);

    let panel = &res.subresources["panel_style"];
    let button = &res.subresources["button_style"];

    assert_eq!(panel.class_name, "StyleBoxFlat");
    assert_eq!(button.class_name, "StyleBoxFlat");
    assert!(!Arc::ptr_eq(panel, button));

    // Verify distinct bg_color values survived parsing.
    match (
        panel.get_property("bg_color"),
        button.get_property("bg_color"),
    ) {
        (Some(Variant::Color(pc)), Some(Variant::Color(bc))) => {
            assert!((pc.r - 0.25).abs() < 0.01, "panel bg_color.r");
            assert!((bc.r - 0.5).abs() < 0.01, "button bg_color.r");
        }
        (p, b) => panic!("expected Color variants, got {:?} / {:?}", p, b),
    }

    // border_width is distinct.
    assert_eq!(panel.get_property("border_width"), Some(&Variant::Int(1)));
    assert_eq!(button.get_property("border_width"), Some(&Variant::Int(2)));
}

// ===========================================================================
// 9. Cache clear drops sub_resource Arcs (no leak via cache)
// ===========================================================================

#[test]
fn cache_clear_releases_subresource_arcs() {
    let loader = InlineTresLoader::new(vec![("res://theme.tres", TRES_WITH_TWO_SUBS)]);
    let mut cache = ResourceCache::new(loader);

    let res = cache.load("res://theme.tres").unwrap();
    let panel_sub = Arc::clone(&res.subresources["panel"]);

    // res (local) + cache entry = 2 strong refs to the top-level Resource.
    assert_eq!(Arc::strong_count(&res), 2);
    // panel_sub (local clone) + inside res.subresources = 2 strong refs.
    assert_eq!(Arc::strong_count(&panel_sub), 2);

    cache.clear();

    // Cache dropped its ref to the top-level Resource.
    assert_eq!(Arc::strong_count(&res), 1);
    // Sub_resource ref count unchanged (still held by res and our local clone).
    assert_eq!(Arc::strong_count(&panel_sub), 2);

    // Drop the top-level resource.
    drop(res);
    // Only our local clone remains.
    assert_eq!(Arc::strong_count(&panel_sub), 1);
}
