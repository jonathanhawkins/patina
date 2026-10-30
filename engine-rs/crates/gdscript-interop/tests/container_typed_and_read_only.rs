//! pat-s3rrv.8 — the Array/Dictionary methods that need per-container state.
//!
//! `is_typed`, `get_typed_builtin`, `make_read_only`, `is_read_only`,
//! `duplicate_deep` and friends answer for a specific container *instance*, so
//! they could not exist while Array/Dictionary were value types. Now each is a
//! handle to shared storage carrying its own type constraint and read-only
//! flag, and that state is visible through every alias.
//!
//! Containers cannot yet be *created* typed from a declaration
//! (`var a: Array[int] = []`): the parser keeps that hint as a string. So the
//! typed queries are pinned here in their untyped answers, which are Godot's
//! answers for an untyped container.

use gdscript_interop::interpreter::Interpreter;
use gdvariant::Variant;

/// Parse `src` as a class, instantiate it, and call `method` with no args
/// through the real interpreter dispatch.
fn call_method(src: &str, method: &str) -> Result<Variant, String> {
    let mut interp = Interpreter::new();
    let class_def = interp.run_class(src).map_err(|e| format!("{e:?}"))?;
    let mut inst = interp
        .instantiate_class(&class_def)
        .map_err(|e| format!("{e:?}"))?;
    interp
        .call_instance_method(&mut inst, method, &[])
        .map_err(|e| format!("{e:?}"))
}

/// Wraps a body in the smallest class that can run it.
fn run(body: &str) -> Result<Variant, String> {
    let indented = body
        .lines()
        .map(|l| format!("    {l}"))
        .collect::<Vec<_>>()
        .join("\n");
    call_method(&format!("extends Node\nfunc go():\n{indented}\n"), "go")
}

// ---------------------------------------------------------------------------
// Read-only
// ---------------------------------------------------------------------------

#[test]
fn make_read_only_is_seen_through_every_alias() {
    assert_eq!(
        run("var a = [1]\nvar b = a\na.make_read_only()\nreturn b.is_read_only()"),
        Ok(Variant::Bool(true)),
        "the flag lives on the shared storage, not on the variable"
    );
}

#[test]
fn a_fresh_array_is_writable() {
    assert_eq!(
        run("var a = []\nreturn a.is_read_only()"),
        Ok(Variant::Bool(false))
    );
}

#[test]
fn appending_through_an_alias_of_a_read_only_array_is_an_error() {
    let got = run("var a = [1]\nvar b = a\na.make_read_only()\nb.append(2)\nreturn b");
    assert!(
        got.is_err(),
        "a write to read-only storage must be refused, not silently applied: {got:?}"
    );
}

#[test]
fn index_write_into_a_read_only_dictionary_is_an_error() {
    let got = run("var d = {\"k\": 1}\nd.make_read_only()\nd[\"k\"] = 2\nreturn d");
    assert!(
        got.is_err(),
        "read-only Dictionary accepted a write: {got:?}"
    );
}

#[test]
fn duplicate_of_a_read_only_array_is_writable() {
    // Godot's duplicate never copies the read-only flag.
    assert_eq!(
        run("var a = [1]\na.make_read_only()\nvar c = a.duplicate()\nc.append(2)\nreturn c"),
        Ok(Variant::Array(
            vec![Variant::Int(1), Variant::Int(2)].into()
        ))
    );
}

// ---------------------------------------------------------------------------
// duplicate_deep
// ---------------------------------------------------------------------------

#[test]
fn duplicate_deep_does_not_share_nested_arrays() {
    assert_eq!(
        run("var inner = [1]\nvar outer = [inner]\nvar deep = outer.duplicate_deep()\ndeep[0].append(2)\nreturn inner"),
        Ok(Variant::Array(vec![Variant::Int(1)].into())),
        "a deep copy must not reach back into the original's nested array"
    );
}

#[test]
fn dictionary_duplicate_deep_does_not_share_nested_arrays() {
    assert_eq!(
        run("var inner = [1]\nvar d = {\"k\": inner}\nvar deep = d.duplicate_deep()\ndeep[\"k\"].append(2)\nreturn inner"),
        Ok(Variant::Array(vec![Variant::Int(1)].into()))
    );
}

// ---------------------------------------------------------------------------
// Typed queries, untyped answers
// ---------------------------------------------------------------------------

#[test]
fn an_untyped_array_reports_untyped() {
    assert_eq!(run("return [].is_typed()"), Ok(Variant::Bool(false)));
    assert_eq!(
        run("return [].get_typed_builtin()"),
        Ok(Variant::Int(0)),
        "TYPE_NIL for an untyped array"
    );
    assert_eq!(run("return [].is_same_typed([1])"), Ok(Variant::Bool(true)));
}

#[test]
fn an_untyped_dictionary_reports_untyped() {
    assert_eq!(run("return {}.is_typed()"), Ok(Variant::Bool(false)));
    assert_eq!(run("return {}.is_typed_key()"), Ok(Variant::Bool(false)));
    assert_eq!(
        run("return {}.get_typed_value_builtin()"),
        Ok(Variant::Int(0))
    );
    assert_eq!(run("return {}.is_same_typed({})"), Ok(Variant::Bool(true)));
}
