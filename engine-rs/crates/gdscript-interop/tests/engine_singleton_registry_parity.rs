//! pat-s3rrv.124 — `Engine.register_singleton` / `unregister_singleton` guards,
//! and `editor_only` filtering of `get_singleton_list`.
//!
//! `register_singleton` silently overwrote anything, including built-ins, and
//! `unregister_singleton` removed anything. Godot `ERR_FAIL`s on a duplicate
//! name, on a name that is not a valid ASCII identifier, and on removing an
//! entry it did not create (`core/core_bind.cpp:1988-2003`).
//!
//! ⚠️ Those are `ERR_FAIL_COND_MSG`, which **prints and returns** — it does not
//! throw. A script calling `Engine.register_singleton("OS", …)` keeps
//! running, so the contract under test is "the registration did not happen",
//! never "the call raised".
//!
//! ⓘ `ENGINE_SINGLETONS` and `ENGINE_EDITOR_HINT` are process-wide. nextest runs
//! each test in its own process, so these do not interfere; they would if this
//! file were ever run as in-process threads.

use gdscript_interop::interpreter::{set_engine_editor_hint, Interpreter};
use gdvariant::Variant;

fn eval(body: &str) -> Variant {
    let src = format!("extends Node\nfunc probe():\n{body}");
    let mut interp = Interpreter::new();
    let class_def = interp.run_class(&src).expect("script parses");
    let mut inst = interp
        .instantiate_class(&class_def)
        .expect("class instantiates");
    interp
        .call_instance_method(&mut inst, "probe", &[])
        .unwrap_or_else(|e| panic!("probe raised: {e:?}"))
}

fn list_contains(name: &str) -> bool {
    match eval(&format!(
        "\x20   return Engine.get_singleton_list().has(\"{name}\")\n"
    )) {
        Variant::Bool(b) => b,
        other => panic!("get_singleton_list().has() must be a bool, got {other:?}"),
    }
}

/// **The core guard.** A user cannot take over a built-in name.
///
/// 🔑 Discriminating: before this, the call inserted into the user map and
/// `get_singleton("OS")` handed back whatever was passed. The rejection is only
/// visible as the object *not* being stored — the call itself returns normally
/// either way, because Godot's failure path returns rather than throwing.
#[test]
fn registering_over_a_builtin_name_is_rejected() {
    let stored = eval(
        "\x20   Engine.register_singleton(\"OS\", 99)\n\
         \x20   return Engine.get_singleton(\"OS\")\n",
    );
    assert_eq!(
        stored,
        Variant::Nil,
        "register_singleton(\"OS\", …) must not shadow the built-in OS singleton"
    );
    assert!(list_contains("OS"), "OS itself stays registered");
}

/// The positive control: a fresh name round-trips through
/// register → get → unregister → get.
///
/// Without this, "rejected" above would pass against an implementation that
/// rejects *everything*.
#[test]
fn a_user_singleton_round_trips() {
    let after_register = eval(
        "\x20   Engine.register_singleton(\"MyThing\", 42)\n\
         \x20   return Engine.get_singleton(\"MyThing\")\n",
    );
    assert_eq!(
        after_register,
        Variant::Int(42),
        "a fresh name must actually register"
    );

    let after_unregister = eval(
        "\x20   Engine.register_singleton(\"MyThing\", 42)\n\
         \x20   Engine.unregister_singleton(\"MyThing\")\n\
         \x20   return Engine.get_singleton(\"MyThing\")\n",
    );
    assert_eq!(
        after_unregister,
        Variant::Nil,
        "a user-created singleton must be removable"
    );
}

/// ⚠️ A built-in is **not user-created**, so it cannot be unregistered — a
/// script must not be able to delete the engine's own singletons out from under
/// every other script in the process (`core_bind.cpp:2001`).
#[test]
fn unregistering_a_builtin_is_rejected() {
    eval("\x20   Engine.unregister_singleton(\"OS\")\n\x20   return 0\n");
    assert!(list_contains("OS"), "OS must survive an unregister attempt");
}

/// 🔑 Godot's identifier rule rejects a leading **digit**, not "anything that is
/// not a letter". `_private` is a valid singleton name and a naive
/// "must start with a letter" check would wrongly reject it — which is why both
/// cases are pinned.
#[test]
fn the_identifier_rule_rejects_a_leading_digit_but_allows_underscore() {
    let bad = eval(
        "\x20   Engine.register_singleton(\"1bad\", 1)\n\
         \x20   return Engine.get_singleton(\"1bad\")\n",
    );
    assert_eq!(
        bad,
        Variant::Nil,
        "a leading digit is not a valid identifier"
    );

    let hyphen = eval(
        "\x20   Engine.register_singleton(\"has-hyphen\", 1)\n\
         \x20   return Engine.get_singleton(\"has-hyphen\")\n",
    );
    assert_eq!(hyphen, Variant::Nil, "a hyphen is not an identifier char");

    let underscore = eval(
        "\x20   Engine.register_singleton(\"_private\", 7)\n\
         \x20   return Engine.get_singleton(\"_private\")\n",
    );
    assert_eq!(
        underscore,
        Variant::Int(7),
        "a leading underscore IS valid — rejecting it would be over-strict"
    );
}

/// `EditorInterface` is registered `editor_only`, so a running game must not see
/// it listed (`core/config/engine.cpp:388`).
///
/// ⚠️ Asserted in both directions. "Absent when not in the editor" alone would
/// pass against an implementation that dropped the name entirely.
#[test]
fn editor_only_singletons_are_hidden_outside_the_editor() {
    set_engine_editor_hint(false);
    assert!(
        !list_contains("EditorInterface"),
        "a running game must not list the editor-only singleton"
    );
    // A non-editor-only built-in is unaffected, so the filter is not just
    // emptying the list.
    assert!(list_contains("OS"));

    set_engine_editor_hint(true);
    assert!(list_contains("EditorInterface"), "the editor must list it");
    set_engine_editor_hint(false);
}
