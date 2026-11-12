//! pat-nzgdu — `Object`'s four dispatched script virtuals must stay declared.
//!
//! `gdscene/src/scripting.rs` has a dedicated "Object virtual chain" section
//! that fully implements all four, calling each through `call_script_method`:
//! `_to_string` (`:2520`), `_get` (`:2532`), `_set` (`:2551`) and
//! `_get_property_list` (`:2560`). ClassDB declared **none** of them.
//!
//! 🔑 These are the widest-reaching instance of the inverse gap this audit
//! found, because `Object` is the ROOT: `class_has_method(<any class>, "_get")`
//! answered **false** for a virtual the engine runs on every object. A script
//! could implement `_get`, have it work, and still be told by `has_method` that
//! it does not exist — and `has_method` is precisely what a defensive script
//! asks, which is why nothing surfaced the gap.
//!
//! Found by `docs/port-audit/verify_invoked_virtuals_are_declared.py`.

use gdobject::class_db::{class_has_method, get_class_info, register_all_classes};

/// The four, with the arity `upstream/godot/doc/classes/Object.xml` documents.
const CHAIN: &[(&str, usize, &str)] = &[
    ("_get", 1, "gdscene/src/scripting.rs:2532"),
    ("_set", 2, "gdscene/src/scripting.rs:2551"),
    ("_get_property_list", 0, "gdscene/src/scripting.rs:2560"),
    ("_to_string", 0, "gdscene/src/scripting.rs:2520"),
];

/// `Object`'s OWN registered methods.
///
/// ⚠️ `get_class_info(..).methods`, not `get_method_list`, which walks the
/// inheritance chain — for the root that happens to be the same thing today,
/// but using the chain-walking helper here would make the test pass if the
/// declarations migrated to a subclass, which is the mistake worth excluding.
macro_rules! own_methods {
    () => {{
        register_all_classes();
        get_class_info("Object")
            .expect("Object is registered")
            .methods
    }};
}

/// The assertion that would have caught the gap.
#[test]
fn the_object_virtual_chain_is_declared() {
    let methods = own_methods!();
    for (name, _, site) in CHAIN {
        assert!(
            methods.iter().any(|m| &m.name == name),
            "🔑 `scripting.rs` dispatches `Object.{name}()` at {site}, but \
             ClassDB declares no such method — so `has_method` answers false \
             for a virtual the engine runs on every object. Declared: {:?}",
            methods.iter().map(|m| &m.name).collect::<Vec<_>>()
        );
    }
}

/// Arity must match the dispatch site.
///
/// ⚠️ `_set` takes **two** (property, value) and `_get` **one**. A wrong count
/// still satisfies `has_method`, so the engine would call the override with a
/// mismatched signature and the failure would surface inside the script.
#[test]
fn the_chain_keeps_its_documented_arities() {
    let methods = own_methods!();
    for (name, argc, site) in CHAIN {
        let m = methods
            .iter()
            .find(|m| &m.name == name)
            .unwrap_or_else(|| panic!("Object.{name}() must be declared"));
        assert_eq!(
            m.argument_count, *argc,
            "`Object.{name}()` is dispatched with {argc} argument(s) at {site}"
        );
    }
}

/// 🔑 Virtual but **not** const — transcribed, not derived.
///
/// `Object.xml` gives all four `qualifiers="virtual"`, unlike `Texture2D`'s
/// `virtual const` trio. Marking `_set` const would contradict its purpose: it
/// exists to consume a write and reports whether it did
/// (`scripting.rs` `object_set_virtual` keys on an explicit `true`).
#[test]
fn the_chain_is_virtual_and_not_const() {
    let methods = own_methods!();
    for (name, _, _) in CHAIN {
        let m = methods.iter().find(|m| &m.name == name).unwrap();
        assert!(
            m.is_virtual,
            "`Object.{name}()` must be marked virtual, or it is a declared \
             method rather than a script override point"
        );
        assert!(
            !m.is_const,
            "`Object.{name}()` is plain `virtual` upstream, NOT `virtual \
             const`; a const `_set` contradicts a method whose job is to \
             consume a write"
        );
    }
}

/// The point of declaring on the ROOT: every class inherits the override point.
///
/// This is the question the interpreter actually asks (`node_has_method`
/// resolves through `class_has_method`), and the one that answered `false`.
#[test]
fn every_class_inherits_the_chain() {
    register_all_classes();
    for class in ["Object", "Node", "Node2D", "Resource", "RefCounted"] {
        for (name, _, _) in CHAIN {
            assert!(
                class_has_method(class, name),
                "`{class}` must inherit `{name}` from Object — a script on a \
                 {class} overrides it, so `has_method` has to agree"
            );
        }
    }
}

/// The underscore virtuals must not have displaced the engine-side accessors.
///
/// ⚠️ `Object` already declared non-underscore `set`/`get` with arities 2 and 1
/// — the same arities as `_set`/`_get`. All four must coexist as distinct
/// entries: ClassDB resolves a duplicate name to the last one silently, so a
/// mis-typed addition would have shadowed an accessor rather than failing.
#[test]
fn the_engine_side_accessors_still_exist_alongside() {
    let methods = own_methods!();
    for (name, argc) in [("set", 2usize), ("get", 1), ("_set", 2), ("_get", 1)] {
        let hits: Vec<_> = methods.iter().filter(|m| m.name == name).collect();
        assert_eq!(
            hits.len(),
            1,
            "`Object.{name}()` must be declared exactly once; {} entries means \
             a duplicate, and ClassDB silently resolves duplicates to the last",
            hits.len()
        );
        assert_eq!(
            hits[0].argument_count, argc,
            "`Object.{name}()` takes {argc} argument(s)"
        );
    }
}
