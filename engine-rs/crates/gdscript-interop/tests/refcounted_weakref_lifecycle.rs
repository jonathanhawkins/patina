//! pat-0q2kx.163 — **the acceptance test**, written but `#[ignore]`d.
//!
//! The bead's acceptance names `refcounted_weakref_lifecycle`. Until now that
//! test did not exist anywhere in the tree, which meant the bead could not be
//! verified against its own acceptance even after the code landed — every
//! recorded run had to substitute some other command. This file removes that
//! blocker: the acceptance is now written down as executable assertions rather
//! than as prose in a bead description.
//!
//! # Why it is ignored
//!
//! It asserts AUTO-FREE ON SCOPE EXIT, which is not implemented. The explicit
//! half of the API (`RefCounted.new()`, `reference` / `unreference` /
//! `get_reference_count` / `init_ref`, `weakref` over a counted handle) does
//! work and is covered by `refcounted_script_lifecycle.rs`. What is missing is
//! Godot's implicit release when a binding leaves scope.
//!
//! **Run it with `--ignored` to see the real state; do not report it as a
//! passing command.** A skipped test makes nextest exit 0, so quoting this file
//! as evidence would go green while proving nothing — the same vacuous-pass trap
//! the bead already fell into once.
//!
//! # What has to land before the `#[ignore]` comes off
//!
//! Refcount the interpreter's `Environment`. Four sites, and the invariant is
//! subtler than "hook `pop_scope`":
//!
//! 1. `define` / `define_const` — a new binding of a refcounted handle takes a
//!    reference.
//! 2. `set` — overwriting a binding releases the OLD value before taking the new
//!    one, or `var a = r; a = other` leaks `r` permanently.
//! 3. `pop_scope` — releases every refcounted handle in the scope being
//!    discarded.
//! 4. `impl Clone for Environment` — must increment everything it copies.
//!    ⚠️ This is the one that makes the naive version dangerous: `Interpreter`
//!    is `Clone`, and a cloned environment that does not increment lets the
//!    clone's `pop_scope` free objects the ORIGINAL still holds — an
//!    intermittent use-after-free in the crate everything depends on. Prefer a
//!    hand-written `Clone` that increments and NO `Drop`, so a dropped clone
//!    leaks instead of double-freeing. Leaking is recoverable; freeing live
//!    objects out from under unrelated scripts is not.
//!
//! Note also that `Environment::get` returns `v.clone()`, so a handle is copied
//! into temporaries constantly without ever becoming a binding. Reference
//! ownership therefore has to key off the binding sites above, not off Variant
//! copies — counting clones instead would inflate every count without bound.

use gdscript_interop::interpreter::Interpreter;
use gdvariant::Variant;

fn run_val(src: &str) -> Variant {
    let mut interp = Interpreter::new();
    interp
        .run(src)
        .unwrap_or_else(|e| panic!("script failed: {e:?}"))
        .return_value
        .expect("script returned a value")
}

fn arr(v: &Variant) -> Vec<Variant> {
    match v {
        Variant::Array(items) => items.to_vec(),
        other => panic!("expected an array, got {other:?}"),
    }
}

/// **The acceptance.** Every clause of the bead's acceptance criteria, in order.
///
/// Currently fails at the third block: nothing releases `r` when the inner scope
/// ends, so the stored weakref still resolves.
#[test]
#[ignore = "auto-free on scope exit is not implemented — see the module docs (pat-0q2kx.163)"]
fn refcounted_weakref_lifecycle() {
    // (1) `var r = RefCounted.new(); var c = r.get_reference_count()` gives 1.
    let v = run_val("var r = RefCounted.new()\nreturn r.get_reference_count()\n");
    assert_eq!(v, Variant::Int(1), "a fresh RefCounted starts at 1");

    // (2) Assigning to a second var then releasing one keeps it alive.
    //     The second binding must take a reference on its own — this is the
    //     clause that fails without `define` participating in the count.
    let v = run_val(
        "var r = RefCounted.new()\n\
         var second = r\n\
         var held = r.get_reference_count()\n\
         r.unreference()\n\
         return [held, is_instance_valid(second)]\n",
    );
    assert_eq!(
        arr(&v)[0],
        Variant::Int(2),
        "binding to a second variable must take a second reference"
    );
    assert_eq!(
        arr(&v)[1],
        Variant::Bool(true),
        "releasing one of two references must leave the object alive"
    );

    // (3) After all references leave scope, a stored weakref reports gone.
    //     `w` is declared in the outer scope so it outlives `r`.
    let v = run_val(
        "var w = null\n\
         if true:\n\
         \tvar r = RefCounted.new()\n\
         \tw = weakref(r)\n\
         return is_instance_valid(w.get_ref())\n",
    );
    assert_eq!(
        v,
        Variant::Bool(false),
        "when the only binding leaves scope the object must be freed, so the \
         stored weakref reports gone"
    );

    // (4) An `extends RefCounted` class instance behaves identically.
    let v = run_val(
        "class Counted extends RefCounted:\n\
         \tvar tag = 7\n\
         var w = null\n\
         if true:\n\
         \tvar c = Counted.new()\n\
         \tw = weakref(c)\n\
         return is_instance_valid(w.get_ref())\n",
    );
    assert_eq!(
        v,
        Variant::Bool(false),
        "an `extends RefCounted` instance must follow the same lifecycle as a \
         direct RefCounted.new()"
    );
}
