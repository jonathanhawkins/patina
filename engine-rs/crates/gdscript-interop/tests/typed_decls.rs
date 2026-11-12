//! pat-s3rrv.8 (partial) — typed container declarations must parse and run.
//!
//! Godot ground truth: `GDScriptParser::parse_type`
//! (upstream/godot/modules/gdscript/gdscript_parser.cpp) accepts a qualified
//! name followed by optional bracketed type parameters, which covers
//! `Array[int]`, Godot 4.4's `Dictionary[K, V]`, nested `Array[Array[T]]`, and
//! qualified names like `MyClass.State`.
//!
//! Patina's `parse_type_hint` special-cased exactly one shape, `Array[<ident>]`
//! (parser.rs:628-638). Everything else broke *silently at the statement level*,
//! which is the damaging part: `var d: Dictionary[String, int] = {}` left the
//! `[` unconsumed, so the `Assign` check failed and the declaration parsed into
//! garbage rather than raising an error. A script using a typed Dictionary did
//! not fail to compile — it ran with a variable that was never declared.
//!
//! These tests drive the real `run_class` -> `instantiate_class` ->
//! `call_instance_method` dispatch, so they prove the declarations survive into
//! a running instance, not merely that the parser accepted them.
//!
//! **Scope note:** this file covers only the *declaration syntax* third of
//! pat-s3rrv.8. Reference (shared-storage) semantics for Array/Dictionary and
//! runtime typed-container enforcement both require reshaping
//! `Variant::Array`/`Variant::Dictionary`, which is a 645-site sweep across
//! seven crates rooted at `gdvariant` — see the bead's `remaining_scope`.

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

/// A typed `Dictionary[K, V]` declaration reaches the instance with its
/// initializer intact. Before the fix the `[` derailed the statement, so
/// `counts` was never declared and `counts.size()` could not resolve.
#[test]
fn typed_dictionary_declaration_runs() {
    let src = "extends Node\n\
               var counts: Dictionary[String, int] = {\"a\": 1, \"b\": 2}\n\
               func run() -> int:\n\
               \x20   return counts.size()\n";
    assert_eq!(call_method(src, "run"), Ok(Variant::Int(2)));
}

/// A nested `Array[Array[T]]` declaration runs. The old parser hit `eat_ident`
/// on the inner `Array` and errored out.
#[test]
fn nested_typed_array_declaration_runs() {
    let src = "extends Node\n\
               var grid: Array[Array[int]] = [[1, 2], [3, 4], [5, 6]]\n\
               func run() -> int:\n\
               \x20   return grid.size()\n";
    assert_eq!(call_method(src, "run"), Ok(Variant::Int(3)));
}

/// Typed hints on parameters and return types parse and the function still
/// dispatches and returns.
#[test]
fn typed_param_and_return_hints_run() {
    let src = "extends Node\n\
               func total(d: Dictionary[String, int], rows: Array[Array[int]]) -> int:\n\
               \x20   return d.size() + rows.size()\n\
               func run() -> int:\n\
               \x20   return total({\"a\": 1}, [[1], [2]])\n";
    assert_eq!(call_method(src, "run"), Ok(Variant::Int(3)));
}

/// A `Dictionary[K, V]` whose value type is itself a typed container — the
/// recursion has to hold through both the comma and the nesting.
#[test]
fn typed_dictionary_of_typed_arrays_runs() {
    let src = "extends Node\n\
               var buckets: Dictionary[String, Array[int]] = {\"a\": [1, 2]}\n\
               func run() -> int:\n\
               \x20   return buckets.size()\n";
    assert_eq!(call_method(src, "run"), Ok(Variant::Int(1)));
}

/// The declaration is not merely tolerated — it is still a *typed* declaration
/// as far as the script is concerned, so the initializer's own contents survive
/// a round trip through the instance.
#[test]
fn typed_declaration_preserves_its_initializer_values() {
    let src = "extends Node\n\
               var counts: Dictionary[String, int] = {\"a\": 7}\n\
               var grid: Array[Array[int]] = [[9, 8]]\n\
               func run() -> int:\n\
               \x20   return counts[\"a\"] + grid[0][1]\n";
    assert_eq!(call_method(src, "run"), Ok(Variant::Int(15)));
}
