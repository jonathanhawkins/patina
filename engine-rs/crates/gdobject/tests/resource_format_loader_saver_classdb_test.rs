//! pat-nzgdu.222 — `ResourceFormatLoader` / `ResourceFormatSaver` are registered
//! as the script-subclassable RefCounted base classes a plugin extends to add a
//! custom resource format.
//!
//! ⚠️ **SCOPE.** This covers the ClassDB *registration* half. The runtime
//! *dispatch* the bead's acceptance also describes — `ResourceLoader`.
//! `add_resource_format_loader` plus a `load()` that invokes a script subclass's
//! `_load` — is a separate, larger `gdscript-interop` feature: there is no
//! `ResourceLoader` singleton or format-loader registry in the interpreter yet.
//! That is why this file is not the acceptance's
//! `custom_resource_format_loader_dispatch_test`; see the bead comment for the
//! recommended split.

use gdobject::class_db::{can_instantiate, is_abstract, register_io_classes};
use gdobject::{class_exists, get_method_list, is_parent_class};

fn arity(class: &str, method: &str) -> usize {
    get_method_list(class)
        .iter()
        .rev()
        .find(|m| m.name == method)
        .unwrap_or_else(|| panic!("{class} has no method `{method}`"))
        .argument_count
}

/// Methods `class` declares over `base` (chain-walking lists subtracted).
fn own_methods(class: &str, base: &str) -> Vec<String> {
    let base_methods: Vec<String> = get_method_list(base).into_iter().map(|m| m.name).collect();
    let mut own: Vec<String> = get_method_list(class)
        .into_iter()
        .map(|m| m.name)
        .filter(|m| !base_methods.contains(m))
        .collect();
    own.sort();
    own
}

#[test]
fn resource_format_loader_saver_classdb_test() {
    register_io_classes();

    for class in ["ResourceFormatLoader", "ResourceFormatSaver"] {
        assert!(class_exists(class), "{class} must be registered");
        assert!(
            is_parent_class(class, "RefCounted"),
            "{class} <- RefCounted"
        );
        assert!(is_parent_class(class, "Object"));
        // GDREGISTER_CLASS (core/register_core_types.cpp:149-150) — concrete and
        // directly subclassable, NOT abstract. A plugin does `extends
        // ResourceFormatLoader` and instantiates it.
        assert!(!is_abstract(class), "{class} is instantiable in Godot");
        assert!(can_instantiate(class));
    }

    // ResourceFormatLoader binds exactly its 11 virtuals over RefCounted
    // (RefCounted itself adds no methods of its own — everything else is Object's,
    // subtracted out by own_methods).
    assert_eq!(
        own_methods("ResourceFormatLoader", "RefCounted"),
        vec![
            "_exists",
            "_get_classes_used",
            "_get_dependencies",
            "_get_recognized_extensions",
            "_get_resource_script_class",
            "_get_resource_type",
            "_get_resource_uid",
            "_handles_type",
            "_load",
            "_recognize_path",
            "_rename_dependencies",
        ],
        "Godot binds these 11 virtuals on ResourceFormatLoader",
    );
    // The load hook a custom loader overrides: (path, original_path,
    // use_sub_threads, cache_mode).
    assert_eq!(arity("ResourceFormatLoader", "_load"), 4);
    assert_eq!(
        arity("ResourceFormatLoader", "_get_recognized_extensions"),
        0
    );

    // ResourceFormatSaver binds exactly its 5 virtuals.
    assert_eq!(
        own_methods("ResourceFormatSaver", "RefCounted"),
        vec![
            "_get_recognized_extensions",
            "_recognize",
            "_recognize_path",
            "_save",
            "_set_uid",
        ],
        "Godot binds these 5 virtuals on ResourceFormatSaver",
    );
    // (resource, path, flags)
    assert_eq!(arity("ResourceFormatSaver", "_save"), 3);
    // ⚠️ On the SAVER, _get_recognized_extensions takes 1 arg (the resource),
    // whereas on the LOADER it takes 0 — the two are NOT symmetric.
    assert_eq!(
        arity("ResourceFormatSaver", "_get_recognized_extensions"),
        1
    );
}

/// Registration is idempotent — a second call must not mint new ClassIds.
#[test]
fn registration_is_idempotent() {
    let ids = || -> Vec<_> {
        ["ResourceFormatLoader", "ResourceFormatSaver"]
            .iter()
            .map(|c| gdobject::get_class_info(c).expect("registered").class_id)
            .collect()
    };
    register_io_classes();
    let first = ids();
    register_io_classes();
    assert_eq!(first, ids(), "re-registering must not mint new ClassIds");
}
