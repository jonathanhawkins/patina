//! pat-0q2kx.222 — `ShaderIncludeDB` registration.
//!
//! Godot ground truth: `servers/register_server_types.cpp:223` and
//! `doc/classes/ShaderIncludeDB.xml`.
//!
//! The merged finding: `ShaderInclude` (the *resource*) was registered, but
//! `ShaderIncludeDB` — the registry a script queries for engine-provided
//! `.gdshaderinc` files — had zero occurrences anywhere in the workspace.

use gdobject::class_db::register_3d_classes;
use gdobject::class_db::{class_exists, get_class_info, is_abstract, is_parent_class};

#[test]
fn shader_include_db_is_registered_with_its_three_methods() {
    register_3d_classes();

    assert!(class_exists("ShaderIncludeDB"));
    assert!(
        is_parent_class("ShaderIncludeDB", "Object"),
        "ShaderIncludeDB.xml declares inherits=\"Object\""
    );

    // ⚠️ NOT abstract. `GDREGISTER_CLASS(ShaderIncludeDB)` at
    // register_server_types.cpp:223. "It is a singleton, therefore abstract" is
    // the plausible wrong inference — I made it myself before checking the
    // source, and Godot marks plenty of singletons concrete.
    assert!(
        !is_abstract("ShaderIncludeDB"),
        "ShaderIncludeDB is GDREGISTER_CLASS, not GDREGISTER_ABSTRACT_CLASS"
    );

    let info = get_class_info("ShaderIncludeDB").expect("registered");
    for (name, arity) in [
        ("list_built_in_include_files", 0usize),
        ("has_built_in_include_file", 1),
        ("get_built_in_include_file", 1),
    ] {
        let m = info
            .methods
            .iter()
            .find(|m| m.name == name)
            .unwrap_or_else(|| panic!("ShaderIncludeDB must declare `{name}`"));
        assert_eq!(m.argument_count, arity, "`{name}` arity from the doc XML");
    }
}

/// The resource and the registry are distinct classes; registering one must not
/// be mistaken for the other.
#[test]
fn shader_include_resource_is_separate_from_the_db() {
    register_3d_classes();

    assert!(
        class_exists("ShaderInclude"),
        "the resource was already present"
    );
    assert!(
        is_parent_class("ShaderInclude", "Resource"),
        "ShaderInclude is a Resource"
    );
    assert!(
        !is_parent_class("ShaderIncludeDB", "Resource"),
        "the DB is an Object singleton, NOT a Resource — conflating them is how \
         the missing one went unnoticed"
    );
}
