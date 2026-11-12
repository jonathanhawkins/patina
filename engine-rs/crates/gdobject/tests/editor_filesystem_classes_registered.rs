//! pat-0q2kx.167 — the editor filesystem service classes are ClassDB-registered
//! with the parents, methods and signals their doc XML declares.
//!
//! `EditorFileSystem` and `FileSystemDock` are implemented natively in
//! `gdeditor/src/filesystem.rs`, but ClassDB knew nothing about them, so a
//! `@tool` script had no class to reach them through —
//! `EditorInterface.get_resource_filesystem()` had nothing to hand back, and
//! `EditorFileSystemDirectory` (the type a script WALKS to enumerate the
//! project) did not exist at all.
//!
//! Signatures come from `upstream/godot/doc/classes/*.xml`, not from Patina's
//! implementation — the two differ (Patina's `scan()` returns a count, Godot's
//! returns void), and ClassDB describes the surface a ported script expects.
//!
//! Parents are asserted rather than just "the class exists", because a wrong
//! parent is the silent failure: `Node::is_class` resolves through ClassDB, so
//! a mis-parented class drops out of the walks that consult it. Both parents
//! here were wrong in the bead description — it calls `EditorFileSystem` an
//! Object (4.6.1 says Node) — which is exactly why they are pinned.

use gdobject::class_db::class_has_method;
use gdobject::{class_exists, class_has_signal, is_parent_class, register_editor_classes};

/// Each registered class with the parent its doc XML declares.
const ADDED: &[(&str, &str)] = &[
    ("EditorFileSystem", "Node"),
    ("EditorFileSystemDirectory", "Object"),
    ("EditorPaths", "Object"),
    ("EditorResourcePreview", "Node"),
];

#[test]
fn editor_filesystem_classes_registered_with_documented_parents() {
    register_editor_classes();

    for (class, parent) in ADDED {
        assert!(class_exists(class), "{class} must be ClassDB-registered");
        assert!(
            is_parent_class(class, parent),
            "{class} must inherit {parent} (doc/classes/{class}.xml `inherits`)"
        );
    }
}

/// The methods a `@tool` script actually calls. `get_filesystem` is the entry
/// point named by this bead's acceptance criteria, and the
/// `EditorFileSystemDirectory` walk methods are what make its result usable.
#[test]
fn editor_filesystem_classes_expose_their_documented_methods() {
    register_editor_classes();

    for m in [
        "get_filesystem",
        "get_filesystem_path",
        "scan",
        "reimport_files",
        "get_file_type",
        "is_scanning",
    ] {
        assert!(
            class_has_method("EditorFileSystem", m),
            "EditorFileSystem must expose {m}()"
        );
    }

    // The tree-walk surface: without these, `get_filesystem()` returns
    // something a script cannot traverse.
    for m in [
        "get_subdir",
        "get_subdir_count",
        "get_file",
        "get_file_count",
        "get_file_path",
        "get_name",
        "get_path",
    ] {
        assert!(
            class_has_method("EditorFileSystemDirectory", m),
            "EditorFileSystemDirectory must expose {m}()"
        );
    }

    for m in [
        "get_cache_dir",
        "get_config_dir",
        "get_data_dir",
        "get_project_settings_dir",
    ] {
        assert!(
            class_has_method("EditorPaths", m),
            "EditorPaths must expose {m}()"
        );
    }

    assert!(
        class_has_method("EditorResourcePreview", "queue_resource_preview"),
        "EditorResourcePreview must expose queue_resource_preview()"
    );
}

/// Signals the editor and `@tool` scripts connect to.
#[test]
fn editor_filesystem_classes_expose_their_documented_signals() {
    register_editor_classes();

    for s in [
        "filesystem_changed",
        "resources_reimported",
        "sources_changed",
        "script_classes_updated",
    ] {
        assert!(
            class_has_signal("EditorFileSystem", s),
            "EditorFileSystem must declare the {s} signal"
        );
    }
    assert!(
        class_has_signal("EditorResourcePreview", "preview_invalidated"),
        "EditorResourcePreview must declare preview_invalidated"
    );
}

/// Registration is idempotent — `register_editor_classes` is called from
/// several entry points, and each of these guards on `class_exists`, so a
/// second call must not panic or duplicate.
#[test]
fn registering_twice_is_idempotent() {
    register_editor_classes();
    register_editor_classes();
    for (class, parent) in ADDED {
        assert!(class_exists(class));
        assert!(is_parent_class(class, parent));
    }
}
