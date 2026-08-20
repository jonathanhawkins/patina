//! pat-s3rrv.253 (script half) — all 12 `ResourceUID` methods from GDScript.
//!
//! Godot ground truth: `core/io/resource_uid.cpp`, `doc/classes/ResourceUID.xml`.
//!
//! # What was wrong
//!
//! `call_resourceuid_static` bound three of the twelve methods, so the other
//! nine died on `UndefinedFunction` even though the engine-side registry could
//! serve them.
//!
//! # 🔑 `has_id` was WRONG, not merely missing
//!
//! ⚠️ It answered `id_to_text(id) != "uid://<invalid>"` — a check that the id is
//! *well-formed*, which is true of **every** non-negative integer. A script
//! could never learn that a uid was unknown, and because the bug always answered
//! `true`, the happy-path assertion "I added it, is it there?" passed against
//! the broken code. Only asking about an id that was never added catches it;
//! that is `b_*`.
//!
//! # ⚠️ The registry is a process singleton, so tests must not collide
//!
//! Godot's `ResourceUID` is one map for the whole process, and Patina now
//! matches that. Each test below therefore owns a distinct id range and never
//! assumes an empty registry. `a_*` exploits the singleton deliberately: it
//! writes from one `Interpreter` and reads from a second one.

use gdscript_interop::interpreter::Interpreter;
use gdvariant::Variant;

/// Runs a script and returns its value, naming the source on failure.
fn run_val(src: &str) -> Variant {
    // ClassDB is consulted before the singleton tables; an unseeded ClassDB is
    // what makes a singleton lookup fail in a way that reads as a missing
    // registration (pat-s3rrv.220).
    gdobject::class_db::register_all_classes();
    let mut interp = Interpreter::new();
    interp
        .run(src)
        .unwrap_or_else(|e| panic!("script failed: {e:?}\n--- source ---\n{src}"))
        .return_value
        .expect("script returned a value")
}

/// Runs a script purely for effect.
fn run_effect(src: &str) {
    gdobject::class_db::register_all_classes();
    let mut interp = Interpreter::new();
    interp
        .run(src)
        .unwrap_or_else(|e| panic!("script failed: {e:?}\n--- source ---\n{src}"));
}

/// 🔑 **THE ACCEPTANCE CLAUSE.** add_id → get_id_path / has_id → remove_id,
/// with the write and the read in **separate interpreters**.
///
/// ⚠️ Two `Interpreter`s are used on purpose. `ResourceUID` is a process
/// singleton in Godot; if Patina backed it with per-interpreter state, every
/// single-script test would still pass and only a cross-interpreter read would
/// expose it.
#[test]
fn a_add_id_is_visible_from_a_second_interpreter() {
    run_effect("ResourceUID.add_id(720100001, \"res://uid_a.tres\")\n");

    assert_eq!(
        run_val("return ResourceUID.get_id_path(720100001)\n"),
        Variant::String("res://uid_a.tres".to_string()),
        "the mapping must outlive the interpreter that wrote it"
    );
    assert_eq!(
        run_val("return ResourceUID.has_id(720100001)\n"),
        Variant::Bool(true)
    );

    run_effect("ResourceUID.remove_id(720100001)\n");
    assert_eq!(
        run_val("return ResourceUID.has_id(720100001)\n"),
        Variant::Bool(false),
        "remove_id must clear the mapping"
    );
    assert_eq!(
        run_val("return ResourceUID.get_id_path(720100001)\n"),
        Variant::String(String::new()),
        "an unregistered id resolves to the empty string, as Godot's \
         get_id_path fails to String()"
    );
}

/// 🔑 The regression assertion for the old `has_id`.
///
/// ⚠️ This id is well-formed and never registered. The previous implementation
/// returned `true` for it — and for every other non-negative integer.
#[test]
fn b_has_id_is_false_for_a_well_formed_unregistered_id() {
    // Its text form is valid, which is exactly why the old check passed.
    assert_ne!(
        run_val("return ResourceUID.id_to_text(720100002)\n"),
        Variant::String("uid://<invalid>".to_string())
    );

    assert_eq!(
        run_val("return ResourceUID.has_id(720100002)\n"),
        Variant::Bool(false),
        "has_id must consult the registry, not just check well-formedness"
    );
}

/// `create_id` mints a distinct, valid id each call.
#[test]
fn c_create_id_is_distinct_each_call() {
    let distinct = run_val(
        "var a = ResourceUID.create_id()\n\
         var b = ResourceUID.create_id()\n\
         var c = ResourceUID.create_id()\n\
         return a != b and b != c and a != c\n",
    );
    assert_eq!(distinct, Variant::Bool(true), "create_id repeated an id");

    // ⚠️ Godot masks the high bit so a UID can never be negative and so can
    // never collide with the -1 invalid sentinel.
    assert_eq!(
        run_val("return ResourceUID.create_id() >= 0\n"),
        Variant::Bool(true),
        "the high bit must be masked"
    );
}

/// `create_id_for_path` is deterministic for a given path.
#[test]
fn d_create_id_for_path_is_stable() {
    let same = run_val(
        "var a = ResourceUID.create_id_for_path(\"res://scenes/level.tscn\")\n\
         var b = ResourceUID.create_id_for_path(\"res://scenes/level.tscn\")\n\
         return a == b\n",
    );
    assert_eq!(
        same,
        Variant::Bool(true),
        "same path must yield the same id"
    );

    let differs = run_val(
        "var a = ResourceUID.create_id_for_path(\"res://scenes/level.tscn\")\n\
         var b = ResourceUID.create_id_for_path(\"res://scenes/other.tscn\")\n\
         return a != b\n",
    );
    assert_eq!(differs, Variant::Bool(true));
}

/// `path_to_uid` / `uid_to_path` round-trip — and miss differently.
///
/// ⚠️ The miss behaviours are NOT symmetric, and that asymmetry is Godot's:
/// `path_to_uid` echoes its input (`if (id == INVALID_ID) return p_path;`)
/// while `uid_to_path` yields empty. Asserting only the round trip would let a
/// "both return empty" implementation pass.
#[test]
fn e_path_and_uid_round_trip_but_miss_differently() {
    run_effect("ResourceUID.add_id(720100003, \"res://round.tres\")\n");

    assert_eq!(
        run_val(
            "var u = ResourceUID.path_to_uid(\"res://round.tres\")\n\
             return ResourceUID.uid_to_path(u)\n"
        ),
        Variant::String("res://round.tres".to_string()),
        "inverse for a registered pair"
    );
    assert_eq!(
        run_val("return ResourceUID.path_to_uid(\"res://round.tres\")\n"),
        run_val("return ResourceUID.id_to_text(720100003)\n"),
        "path_to_uid must agree with id_to_text for the same mapping"
    );

    assert_eq!(
        run_val("return ResourceUID.path_to_uid(\"res://unregistered.tres\")\n"),
        Variant::String("res://unregistered.tres".to_string()),
        "path_to_uid echoes an unregistered path rather than failing"
    );
    assert_eq!(
        run_val("return ResourceUID.uid_to_path(\"uid://abcdefgh\")\n"),
        Variant::String(String::new()),
        "…while uid_to_path yields empty for an unknown uid"
    );

    run_effect("ResourceUID.remove_id(720100003)\n");
}

/// `ensure_path` resolves a uid and passes anything else through untouched.
#[test]
fn f_ensure_path_only_touches_uid_strings() {
    run_effect("ResourceUID.add_id(720100004, \"res://ensure.tres\")\n");

    assert_eq!(
        run_val(
            "var u = ResourceUID.id_to_text(720100004)\n\
             return ResourceUID.ensure_path(u)\n"
        ),
        Variant::String("res://ensure.tres".to_string())
    );

    // ⚠️ A plain path is returned verbatim — not looked up, not emptied.
    assert_eq!(
        run_val("return ResourceUID.ensure_path(\"res://plain.tres\")\n"),
        Variant::String("res://plain.tres".to_string())
    );

    run_effect("ResourceUID.remove_id(720100004)\n");
}

/// `set_id` repoints an existing mapping.
///
/// ⚠️ `add_id` and `set_id` are exact inverses in Godot — `add_id` fails when
/// the id EXISTS, `set_id` when it does NOT. Both are `void`, so a script sees
/// `null` either way and cannot observe the failure; what it CAN observe is
/// that the rejected call left the mapping alone.
#[test]
fn g_add_does_not_overwrite_but_set_repoints() {
    run_effect("ResourceUID.add_id(720100005, \"res://first.tres\")\n");

    // A second add_id on the same id is rejected — the path must not change.
    run_effect("ResourceUID.add_id(720100005, \"res://second.tres\")\n");
    assert_eq!(
        run_val("return ResourceUID.get_id_path(720100005)\n"),
        Variant::String("res://first.tres".to_string()),
        "add_id is not an upsert; the duplicate must not overwrite"
    );

    // set_id, the inverse, does repoint it.
    run_effect("ResourceUID.set_id(720100005, \"res://moved.tres\")\n");
    assert_eq!(
        run_val("return ResourceUID.get_id_path(720100005)\n"),
        Variant::String("res://moved.tres".to_string()),
        "set_id repoints an existing mapping"
    );

    // set_id on an id that was never added must not create it.
    run_effect("ResourceUID.set_id(720100006, \"res://nope.tres\")\n");
    assert_eq!(
        run_val("return ResourceUID.has_id(720100006)\n"),
        Variant::Bool(false),
        "a rejected set_id must not have created the mapping"
    );

    run_effect("ResourceUID.remove_id(720100005)\n");
}

/// 🔑 All twelve methods dispatch — none falls through to `UndefinedFunction`.
///
/// ⚠️ `run_val` panics on a script error, so simply *reaching* a value for each
/// call is the assertion. Nine of these twelve used to fail here.
#[test]
fn h_all_twelve_methods_dispatch() {
    run_effect("ResourceUID.add_id(720100007, \"res://all.tres\")\n");

    let calls = [
        "ResourceUID.id_to_text(720100007)",
        "ResourceUID.text_to_id(\"uid://abc\")",
        "ResourceUID.has_id(720100007)",
        "ResourceUID.create_id()",
        "ResourceUID.create_id_for_path(\"res://all.tres\")",
        "ResourceUID.get_id_path(720100007)",
        "ResourceUID.uid_to_path(\"uid://abc\")",
        "ResourceUID.path_to_uid(\"res://all.tres\")",
        "ResourceUID.ensure_path(\"res://all.tres\")",
    ];
    for call in calls {
        let v = run_val(&format!("return {call}\n"));
        assert_ne!(v, Variant::Nil, "{call} returned null — is it dispatched?");
    }

    // The three `void` methods answer null by design, so they are exercised for
    // effect rather than for a value; an undispatched name would still panic.
    run_effect("ResourceUID.set_id(720100007, \"res://all2.tres\")\n");
    run_effect("ResourceUID.remove_id(720100007)\n");
    run_effect("ResourceUID.add_id(720100008, \"res://all3.tres\")\n");
    run_effect("ResourceUID.remove_id(720100008)\n");
}
