//! Integration tests for the VisualScript compatibility stub.
//!
//! VisualScript was deprecated and removed in Godot 4. These tests verify that
//! the Patina engine can handle `.vs` file references and VisualScript resources
//! without crashing — the stub provides graceful degradation.

use std::path::Path;

use gdresource::importers::ResourceFormatLoader;
use gdscript_interop::bindings::{ScriptError, ScriptInstance};
use gdscript_interop::VisualScriptStub;

// ---------------------------------------------------------------------------
// ScriptInstance trait — graceful no-op behavior
// ---------------------------------------------------------------------------

#[test]
fn visual_script_stub_implements_script_instance() {
    let mut stub = VisualScriptStub::new("res://scripts/enemy_ai.vs");

    // Name identifies as VisualScript
    assert_eq!(stub.get_script_name(), "VisualScript");

    // Method calls fail gracefully with MethodNotFound
    let err = stub.call_method("_ready", &[]).unwrap_err();
    assert!(matches!(err, ScriptError::MethodNotFound(_)));

    let err = stub.call_method("_process", &[gdvariant::Variant::Float(0.016)]).unwrap_err();
    assert!(matches!(err, ScriptError::MethodNotFound(_)));

    // Properties are inert
    assert_eq!(stub.get_property("speed"), None);
    assert!(!stub.set_property("speed", gdvariant::Variant::Float(100.0)));

    // Introspection returns empty
    assert!(stub.list_methods().is_empty());
    assert!(stub.list_properties().is_empty());
    assert!(!stub.has_method("_ready"));
    assert!(!stub.has_method("custom_func"));
}

#[test]
fn visual_script_stub_preserves_path() {
    let stub = VisualScriptStub::new("res://ai/patrol.vs");
    assert_eq!(stub.script_path(), "res://ai/patrol.vs");
}

#[test]
fn visual_script_stub_scene_access_is_noop() {
    // set_scene_access and resolve_onready should not panic
    let mut stub = VisualScriptStub::new("res://test.vs");
    stub.resolve_onready();
    // set_scene_access requires a trait object — verify it compiles and doesn't panic
    // by calling clear_scene_access (inherited default, also a no-op)
    stub.clear_scene_access();
}

// ---------------------------------------------------------------------------
// ResourceFormatLoader — .vs extension registered
// ---------------------------------------------------------------------------

#[test]
fn resource_loader_can_load_vs_extension() {
    let rfl = ResourceFormatLoader::with_defaults();
    assert!(rfl.can_load(".vs"), ".vs must be a registered extension");
    assert!(rfl.can_load("vs"), "should accept without leading dot too");
}

#[test]
fn resource_loader_loads_vs_file_as_visual_script_resource() {
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("old_script.vs");
    // Write minimal content — the loader just checks existence
    std::fs::write(&path, b"[visual_script_stub]").unwrap();

    let rfl = ResourceFormatLoader::with_defaults();
    let res = rfl.load_resource(&path).unwrap();

    assert_eq!(res.class_name, "VisualScript");
    assert_eq!(
        res.get_property("_deprecated"),
        Some(&gdvariant::Variant::Bool(true))
    );
}

#[test]
fn resource_loader_vs_missing_file_returns_error() {
    let rfl = ResourceFormatLoader::with_defaults();
    let result = rfl.load_resource(Path::new("/nonexistent/path/script.vs"));
    assert!(result.is_err(), "loading a missing .vs file should error");
}

// ---------------------------------------------------------------------------
// ClassDB — VisualScript registered as a class
// ---------------------------------------------------------------------------

#[test]
fn classdb_recognizes_visual_script_class() {
    // register_3d_classes registers VisualScript; ensure it's callable
    gdobject::class_db::register_3d_classes();
    assert!(
        gdobject::class_db::class_exists("VisualScript"),
        "VisualScript must be registered in ClassDB"
    );
}

#[test]
fn classdb_visual_script_inherits_from_resource() {
    gdobject::class_db::register_3d_classes();
    // Need Resource registered for inheritance to work
    if !gdobject::class_db::class_exists("Resource") {
        gdobject::class_db::register_class(
            gdobject::class_db::ClassRegistration::new("Resource").parent("Object"),
        );
    }
    assert!(
        gdobject::class_db::is_parent_class("VisualScript", "Resource"),
        "VisualScript should inherit from Resource"
    );
}

// ---------------------------------------------------------------------------
// Multiple stubs don't interfere with each other
// ---------------------------------------------------------------------------

#[test]
fn multiple_stubs_are_independent() {
    let mut stub_a = VisualScriptStub::new("res://a.vs");
    let mut stub_b = VisualScriptStub::new("res://b.vs");

    assert_eq!(stub_a.script_path(), "res://a.vs");
    assert_eq!(stub_b.script_path(), "res://b.vs");

    // Both return independent errors
    let err_a = stub_a.call_method("foo", &[]).unwrap_err();
    let err_b = stub_b.call_method("bar", &[]).unwrap_err();
    match (err_a, err_b) {
        (ScriptError::MethodNotFound(a), ScriptError::MethodNotFound(b)) => {
            assert!(a.contains("foo"));
            assert!(b.contains("bar"));
        }
        _ => panic!("expected MethodNotFound for both"),
    }
}
