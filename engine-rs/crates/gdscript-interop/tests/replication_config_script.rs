//! pat-s3rrv.177 — `SceneReplicationConfig` reachable from GDScript.
//!
//! Godot ground truth: `modules/multiplayer/doc_classes/SceneReplicationConfig.xml`
//! (inherits `Resource`, thirteen methods) and
//! `scene_replication_config.cpp`.
//!
//! 🔑 The class was **registered in ClassDB with all thirteen methods and no
//! dispatch whatsoever** — no `.new()` arm, no routing. `has_method("add_property")`
//! answered `true` while every call returned `Nil`. Same failure shape as
//! HTTPClient on pat-s3rrv.78: registered, therefore looks supported; unreachable,
//! therefore does nothing.
//!
//! ⚠️ `sync` and `watch` are two views over ONE `mode` field upstream —
//! `property_get_watch()` *is* `mode == REPLICATION_MODE_ON_CHANGE`. The tests
//! below pin that they are mutually exclusive, because a naive implementation
//! with two independent bools passes every single-flag assertion and only fails
//! when both are queried.

use gdscript_interop::interpreter::Interpreter;
use gdvariant::Variant;

/// Runs `check()` and returns its value.
///
/// ⚠️ `register_networking_classes`, not `register_3d_classes`: a bare
/// interpreter starts with an EMPTY ClassDB, and `SceneReplicationConfig` is
/// registered in the networking block. Seeding the wrong one leaves
/// `class_exists` false and fails the test for an unrelated reason.
fn call_check(src: &str) -> Variant {
    gdobject::class_db::register_networking_classes();
    let mut interp = Interpreter::new();
    let class_def = interp.run_class(src).expect("script parses");
    let mut inst = interp
        .instantiate_class(&class_def)
        .expect("class instantiates");
    interp
        .call_instance_method(&mut inst, "check", &[])
        .unwrap_or_else(|e| panic!("check() failed: {e:?}"))
}

fn int(v: Variant) -> i64 {
    match v {
        Variant::Int(i) => i,
        other => panic!("expected an Int, got {other:?}"),
    }
}

const PRELUDE: &str = "extends Node\nfunc check():\n    var c = SceneReplicationConfig.new()\n";

// ============================================================ the acceptance

/// `SceneReplicationConfig.new()` yields an object whose methods answer real
/// values rather than `Nil`.
#[test]
fn a_script_can_construct_a_config_and_call_it() {
    let v = call_check(&format!(
        "{PRELUDE}    c.add_property(\".:position\")\n    return c.has_property(\".:position\")\n"
    ));
    assert_eq!(
        v,
        Variant::Bool(true),
        "has_property must answer a real Bool; Nil means the class is registered \
         but still undispatched"
    );
}

#[test]
fn the_class_is_registered_as_a_resource() {
    let v = call_check(
        "extends Node\nfunc check():\n    return ClassDB.class_exists(\"SceneReplicationConfig\")\n",
    );
    assert_eq!(v, Variant::Bool(true));

    let v = call_check(
        "extends Node\nfunc check():\n    return ClassDB.is_parent_class(\"SceneReplicationConfig\", \"Resource\")\n",
    );
    assert_eq!(v, Variant::Bool(true), "it inherits Resource upstream");
}

/// `get_properties()` returns NodePaths, in registration order.
#[test]
fn get_properties_returns_the_paths_in_order() {
    let v = call_check(&format!(
        "{PRELUDE}    c.add_property(\".:position\")\n    c.add_property(\".:health\")\n    return c.get_properties()\n"
    ));
    match v {
        Variant::Array(items) => {
            let paths: Vec<String> = items.to_vec().iter().map(|i| i.to_string_lossy()).collect();
            assert_eq!(
                paths,
                vec![".:position".to_string(), ".:health".to_string()]
            );
            assert!(
                items
                    .to_vec()
                    .iter()
                    .all(|i| matches!(i, Variant::NodePath(_))),
                "entries must be NodePaths, as Godot returns; got {items:?}"
            );
        }
        other => panic!("expected an Array, got {other:?}"),
    }
}

// ------------------------------------------------------------- the mode views

/// 🔑 sync and watch are one field: enabling watch must turn sync off.
///
/// ⚠️ An implementation with two independent bools passes every assertion that
/// checks only one of them. Querying BOTH after setting ONE is what catches it.
#[test]
fn sync_and_watch_are_mutually_exclusive() {
    let v = call_check(&format!(
        "{PRELUDE}    c.add_property(\".:hp\")\n    c.property_set_watch(\".:hp\", true)\n    return c.property_get_sync(\".:hp\")\n"
    ));
    assert_eq!(
        v,
        Variant::Bool(false),
        "watch and sync are two views of one mode — enabling watch must clear sync"
    );

    let v = call_check(&format!(
        "{PRELUDE}    c.add_property(\".:hp\")\n    c.property_set_watch(\".:hp\", true)\n    return c.property_get_watch(\".:hp\")\n"
    ));
    assert_eq!(
        v,
        Variant::Bool(true),
        "…and watch itself must read back true"
    );
}

/// 🔑 The named enum constants resolve from a script AND agree with the wire
/// ordinals.
///
/// ⚠️ Both halves are needed, and asserting only the first would be a tautology:
/// comparing `property_get_replication_mode(p)` against
/// `SceneReplicationConfig.REPLICATION_MODE_ON_CHANGE` passes even if BOTH the
/// registration and the implementation carry the same wrong number. So the
/// constants are pinned to their literal Godot values here, and the ordinals the
/// engine actually stores are pinned separately in the test below.
#[test]
fn the_mode_constants_resolve_from_script_with_godots_values() {
    for (name, want) in [
        ("REPLICATION_MODE_NEVER", 0),
        ("REPLICATION_MODE_ALWAYS", 1),
        ("REPLICATION_MODE_ON_CHANGE", 2),
    ] {
        let v = call_check(&format!(
            "extends Node\nfunc check():\n    return SceneReplicationConfig.{name}\n"
        ));
        assert_eq!(
            int(v),
            want,
            "`SceneReplicationConfig.{name}` must resolve from a script to {want}. \
             A failure here is either the enum not being registered, or the \
             member-access fallback (pat-s3rrv.42) not reaching class constants"
        );
    }
}

/// …and the engine's stored ordinals match those constants.
///
/// ⚠️ Deliberately spelled as literals, not as the constants: see above.
/// The mode ordinals are Godot's: NEVER 0, ALWAYS 1, ON_CHANGE 2.
#[test]
fn the_replication_mode_ordinals_are_godots() {
    let v = call_check(&format!(
        "{PRELUDE}    c.add_property(\".:hp\")\n    return c.property_get_replication_mode(\".:hp\")\n"
    ));
    assert_eq!(int(v), 1, "a fresh property defaults to ALWAYS");

    let v = call_check(&format!(
        "{PRELUDE}    c.add_property(\".:hp\")\n    c.property_set_watch(\".:hp\", true)\n    return c.property_get_replication_mode(\".:hp\")\n"
    ));
    assert_eq!(int(v), 2, "watch is ON_CHANGE");

    let v = call_check(&format!(
        "{PRELUDE}    c.add_property(\".:hp\")\n    c.property_set_sync(\".:hp\", false)\n    return c.property_get_replication_mode(\".:hp\")\n"
    ));
    assert_eq!(int(v), 0, "clearing sync on an ALWAYS property gives NEVER");
}

/// ⚠️ `property_get_index` returns **-1** for an unknown path, not null.
///
/// A script writes `if c.property_get_index(p) == -1`. Returning `Nil` makes that
/// comparison silently false and the branch never runs.
#[test]
fn an_unknown_property_index_is_minus_one() {
    let v = call_check(&format!(
        "{PRELUDE}    return c.property_get_index(\".:nope\")\n"
    ));
    assert_eq!(int(v), -1);

    let v = call_check(&format!(
        "{PRELUDE}    c.add_property(\".:a\")\n    c.add_property(\".:b\")\n    return c.property_get_index(\".:b\")\n"
    ));
    assert_eq!(int(v), 1, "and a known path reports its registration index");
}

/// `spawn` is independent of the mode, as upstream's `_update()` has it.
#[test]
fn spawn_is_independent_of_the_mode() {
    let v = call_check(&format!(
        "{PRELUDE}    c.add_property(\".:team\")\n    c.property_set_sync(\".:team\", false)\n    return c.property_get_spawn(\".:team\")\n"
    ));
    assert_eq!(
        v,
        Variant::Bool(true),
        "clearing sync must not clear spawn — upstream pushes spawn_props outside \
         the mode switch entirely"
    );
}

/// An unknown method still raises rather than answering `Nil`.
#[test]
fn an_unknown_method_raises() {
    gdobject::class_db::register_networking_classes();
    let mut interp = Interpreter::new();
    let class_def = interp
        .run_class(&format!("{PRELUDE}    return c.no_such_method()\n"))
        .expect("script parses");
    let mut inst = interp
        .instantiate_class(&class_def)
        .expect("class instantiates");
    assert!(
        interp
            .call_instance_method(&mut inst, "check", &[])
            .is_err(),
        "an unrecognised method must raise, not answer Nil — that is the bug \
         this increment removes"
    );
}
