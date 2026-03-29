//! pat-vkh6: Resource cache parity — concrete SubResource resolution and
//! non-sharing semantics.
//!
//! Tests cover:
//! - `resolve_subresource()` happy path and edge cases
//! - Multiple properties pointing to the same sub-resource ID → same Arc
//! - Nested sub-resources (sub-resource with its own sub-resources)
//! - Typed variant properties on resolved sub-resources
//! - Non-sharing across separate Resource instances (clone isolation)
//! - Cache-level: sub-resources travel with cached parent
//! - Arc reference-count semantics through resolution chains
//!
//! Acceptance: bounded tests for resolution and non-sharing semantics.

use std::sync::Arc;

use gdcore::error::EngineResult;
use gdcore::math::Vector2;
use gdresource::{Resource, ResourceCache, ResourceLoader};
use gdvariant::Variant;

// ===========================================================================
// Helpers
// ===========================================================================

/// A loader that returns a Resource pre-populated with sub-resources.
struct SubResourceLoader;

impl SubResourceLoader {
    /// Builds a parent resource with two sub-resources:
    /// - "style1" (StyleBoxFlat) with color and corner_radius properties
    /// - "style2" (StyleBoxEmpty) with no extra properties
    fn make_parent() -> Resource {
        let mut style1 = Resource::new("StyleBoxFlat");
        style1.path = String::new();
        style1.set_property("bg_color", Variant::String("Color(1, 0, 0, 1)".into()));
        style1.set_property("corner_radius", Variant::Int(8));
        style1.set_property("size", Variant::Vector2(Vector2::new(100.0, 50.0)));

        let style2 = Resource::new("StyleBoxEmpty");

        let mut parent = Resource::new("Theme");
        parent.path = "res://theme.tres".into();

        // Wire sub-resources into the parent
        parent
            .subresources
            .insert("style1".into(), Arc::new(style1));
        parent
            .subresources
            .insert("style2".into(), Arc::new(style2));

        // Properties that reference sub-resources
        parent.set_property("normal_style", Variant::String("SubResource:style1".into()));
        parent.set_property("hover_style", Variant::String("SubResource:style1".into()));
        parent.set_property("focus_style", Variant::String("SubResource:style2".into()));
        // Non-SubResource properties
        parent.set_property("name", Variant::String("MyTheme".into()));
        parent.set_property("version", Variant::Int(2));

        parent
    }
}

impl ResourceLoader for SubResourceLoader {
    fn load(&self, path: &str) -> EngineResult<Arc<Resource>> {
        let mut r = SubResourceLoader::make_parent();
        r.path = path.to_string();
        Ok(Arc::new(r))
    }
}

/// A loader that produces resources with nested sub-resources (sub-resource
/// containing its own sub-resources).
struct NestedSubResourceLoader;

impl ResourceLoader for NestedSubResourceLoader {
    fn load(&self, path: &str) -> EngineResult<Arc<Resource>> {
        // Inner sub-resource
        let mut gradient = Resource::new("Gradient");
        gradient.set_property("colors", Variant::String("PackedColorArray(...)".into()));

        // Outer sub-resource that contains the inner one
        let mut texture = Resource::new("GradientTexture2D");
        texture
            .subresources
            .insert("grad1".into(), Arc::new(gradient));
        texture.set_property("gradient", Variant::String("SubResource:grad1".into()));

        // Top-level resource
        let mut parent = Resource::new("Material");
        parent.path = path.to_string();
        parent.subresources.insert("tex1".into(), Arc::new(texture));
        parent.set_property("albedo_texture", Variant::String("SubResource:tex1".into()));

        Ok(Arc::new(parent))
    }
}

// ===========================================================================
// resolve_subresource — happy path
// ===========================================================================

#[test]
fn resolve_subresource_returns_correct_resource() {
    let parent = SubResourceLoader::make_parent();
    let resolved = parent.resolve_subresource("normal_style").unwrap();
    assert_eq!(resolved.class_name, "StyleBoxFlat");
}

#[test]
fn resolve_subresource_different_ids_return_different_arcs() {
    let parent = SubResourceLoader::make_parent();
    let style1 = parent.resolve_subresource("normal_style").unwrap();
    let style2 = parent.resolve_subresource("focus_style").unwrap();
    assert!(!Arc::ptr_eq(style1, style2));
    assert_eq!(style1.class_name, "StyleBoxFlat");
    assert_eq!(style2.class_name, "StyleBoxEmpty");
}

#[test]
fn resolve_subresource_same_id_returns_same_arc() {
    let parent = SubResourceLoader::make_parent();
    // normal_style and hover_style both reference "style1"
    let a = parent.resolve_subresource("normal_style").unwrap();
    let b = parent.resolve_subresource("hover_style").unwrap();
    assert!(
        Arc::ptr_eq(a, b),
        "Two properties referencing the same SubResource ID must resolve to the same Arc"
    );
}

// ===========================================================================
// resolve_subresource — edge cases
// ===========================================================================

#[test]
fn resolve_subresource_missing_property_returns_none() {
    let parent = SubResourceLoader::make_parent();
    assert!(parent.resolve_subresource("nonexistent_key").is_none());
}

#[test]
fn resolve_subresource_non_string_property_returns_none() {
    let parent = SubResourceLoader::make_parent();
    // "version" is Variant::Int(2), not a string
    assert!(parent.resolve_subresource("version").is_none());
}

#[test]
fn resolve_subresource_non_subresource_string_returns_none() {
    let parent = SubResourceLoader::make_parent();
    // "name" is a plain string, not a "SubResource:" reference
    assert!(parent.resolve_subresource("name").is_none());
}

#[test]
fn resolve_subresource_dangling_id_returns_none() {
    let mut r = Resource::new("Test");
    r.set_property(
        "missing_ref",
        Variant::String("SubResource:nonexistent_id".into()),
    );
    assert!(r.resolve_subresource("missing_ref").is_none());
}

#[test]
fn resolve_subresource_empty_id_returns_none() {
    let mut r = Resource::new("Test");
    r.set_property("empty_ref", Variant::String("SubResource:".into()));
    // Empty ID won't match any key in the subresources map
    assert!(r.resolve_subresource("empty_ref").is_none());
}

// ===========================================================================
// Typed variant properties on resolved sub-resources
// ===========================================================================

#[test]
fn resolved_subresource_has_typed_properties() {
    let parent = SubResourceLoader::make_parent();
    let style = parent.resolve_subresource("normal_style").unwrap();

    assert_eq!(style.get_property("corner_radius"), Some(&Variant::Int(8)));
    assert_eq!(
        style.get_property("size"),
        Some(&Variant::Vector2(Vector2::new(100.0, 50.0)))
    );
    assert_eq!(
        style.get_property("bg_color"),
        Some(&Variant::String("Color(1, 0, 0, 1)".into()))
    );
}

#[test]
fn resolved_subresource_property_count() {
    let parent = SubResourceLoader::make_parent();
    let style = parent.resolve_subresource("normal_style").unwrap();
    assert_eq!(style.property_count(), 3); // bg_color, corner_radius, size
}

// ===========================================================================
// Nested sub-resources
// ===========================================================================

#[test]
fn nested_subresource_resolution() {
    let mut loader = ResourceCache::new(NestedSubResourceLoader);
    let parent = loader.load("res://material.tres").unwrap();

    // Level 1: parent → tex1
    let texture = parent.resolve_subresource("albedo_texture").unwrap();
    assert_eq!(texture.class_name, "GradientTexture2D");

    // Level 2: tex1 → grad1
    let gradient = texture.resolve_subresource("gradient").unwrap();
    assert_eq!(gradient.class_name, "Gradient");
    assert!(gradient.get_property("colors").is_some());
}

#[test]
fn nested_subresource_class_names_preserved() {
    let mut cache = ResourceCache::new(NestedSubResourceLoader);
    let parent = cache.load("res://mat.tres").unwrap();

    assert_eq!(parent.class_name, "Material");
    assert_eq!(parent.subresources.len(), 1);

    let tex = parent.subresources.get("tex1").unwrap();
    assert_eq!(tex.class_name, "GradientTexture2D");
    assert_eq!(tex.subresources.len(), 1);

    let grad = tex.subresources.get("grad1").unwrap();
    assert_eq!(grad.class_name, "Gradient");
    assert!(grad.subresources.is_empty());
}

// ===========================================================================
// Non-sharing / clone isolation
// ===========================================================================

#[test]
fn cloned_resource_subresources_share_arcs() {
    let parent = SubResourceLoader::make_parent();
    let clone = parent.clone();

    // Clone shares the same Arc sub-resources (Rust's Arc::clone semantics)
    let orig_style = parent.subresources.get("style1").unwrap();
    let clone_style = clone.subresources.get("style1").unwrap();
    assert!(
        Arc::ptr_eq(orig_style, clone_style),
        "Clone of Resource shares Arc sub-resources (shallow clone)"
    );
}

#[test]
fn separate_loads_produce_independent_subresources() {
    // Two separate loads (not cached) must produce independent sub-resource Arcs.
    let loader = SubResourceLoader;
    let a = loader.load("res://theme_a.tres").unwrap();
    let b = loader.load("res://theme_b.tres").unwrap();

    let style_a = a.subresources.get("style1").unwrap();
    let style_b = b.subresources.get("style1").unwrap();

    assert!(
        !Arc::ptr_eq(style_a, style_b),
        "Separate loads must produce independent sub-resource allocations"
    );
}

// ===========================================================================
// Cache-level sub-resource behavior
// ===========================================================================

#[test]
fn cached_resource_preserves_subresources() {
    let mut cache = ResourceCache::new(SubResourceLoader);
    let first = cache.load("res://theme.tres").unwrap();
    let second = cache.load("res://theme.tres").unwrap();

    // Same parent Arc (cache hit)
    assert!(Arc::ptr_eq(&first, &second));

    // Sub-resources accessible through cache hit
    let s1 = first.resolve_subresource("normal_style").unwrap();
    let s2 = second.resolve_subresource("normal_style").unwrap();
    assert!(Arc::ptr_eq(s1, s2));
}

#[test]
fn cache_invalidate_reload_produces_new_subresources() {
    let mut cache = ResourceCache::new(SubResourceLoader);

    let old = cache.load("res://theme.tres").unwrap();
    let old_style = old.subresources.get("style1").unwrap().clone();

    cache.invalidate("res://theme.tres");

    let new = cache.load("res://theme.tres").unwrap();
    let new_style = new.subresources.get("style1").unwrap();

    assert!(
        !Arc::ptr_eq(&old, &new),
        "Reload after invalidation produces new parent"
    );
    assert!(
        !Arc::ptr_eq(&old_style, new_style),
        "Reload after invalidation produces new sub-resources"
    );
}

#[test]
fn cache_different_paths_independent_subresources() {
    let mut cache = ResourceCache::new(SubResourceLoader);
    let a = cache.load("res://theme_a.tres").unwrap();
    let b = cache.load("res://theme_b.tres").unwrap();

    assert!(!Arc::ptr_eq(&a, &b));

    let style_a = a.subresources.get("style1").unwrap();
    let style_b = b.subresources.get("style1").unwrap();
    assert!(
        !Arc::ptr_eq(style_a, style_b),
        "Sub-resources from different cache entries must be independent"
    );
}

// ===========================================================================
// Arc reference-count semantics
// ===========================================================================

#[test]
fn subresource_arc_strong_count_from_parent() {
    let parent = SubResourceLoader::make_parent();
    let style1 = parent.subresources.get("style1").unwrap();
    // Only held by the parent's HashMap
    assert_eq!(Arc::strong_count(style1), 1);
}

#[test]
fn subresource_arc_strong_count_with_resolution() {
    let parent = SubResourceLoader::make_parent();
    let resolved = parent.resolve_subresource("normal_style").unwrap();
    // resolve_subresource returns a reference, not a clone — still count=1
    assert_eq!(Arc::strong_count(resolved), 1);

    // Cloning the Arc bumps the count
    let cloned = Arc::clone(resolved);
    assert_eq!(Arc::strong_count(&cloned), 2);
}

#[test]
fn cached_parent_subresource_strong_count() {
    let mut cache = ResourceCache::new(SubResourceLoader);
    let parent = cache.load("res://theme.tres").unwrap();

    // parent Arc: one in cache, one local
    assert_eq!(Arc::strong_count(&parent), 2);

    // Sub-resource Arc: held only by parent's subresources HashMap
    let style = parent.subresources.get("style1").unwrap();
    assert_eq!(Arc::strong_count(style), 1);
}

// ===========================================================================
// Subresources map direct access
// ===========================================================================

#[test]
fn subresources_map_keys_match_ids() {
    let parent = SubResourceLoader::make_parent();
    let mut keys: Vec<_> = parent.subresources.keys().cloned().collect();
    keys.sort();
    assert_eq!(keys, vec!["style1".to_string(), "style2".to_string()]);
}

#[test]
fn empty_subresources_map_on_fresh_resource() {
    let r = Resource::new("Bare");
    assert!(r.subresources.is_empty());
    assert!(r.resolve_subresource("anything").is_none());
}
