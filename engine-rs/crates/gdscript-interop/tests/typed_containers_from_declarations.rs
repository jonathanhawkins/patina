//! pat-s3rrv.265 — typed containers *created* from declarations.
//!
//! pat-s3rrv.8 gave `Array`/`Dictionary` shared storage that can carry an
//! element type and a read-only flag, and pinned the typed queries in their
//! untyped answers because nothing could set either
//! (`container_typed_and_read_only.rs`). This file is the other half: a
//! declaration now produces the state those queries report.
//!
//! Four things are asserted, matching the bead's acceptance:
//!
//! 1. `var a: Array[int] = []` makes a typed array, and `a.append("x")` is a
//!    runtime error;
//! 2. `a.is_typed()` is true and `a.get_typed_builtin() == TYPE_INT`;
//! 3. `const C = [1]` is read-only, so `C.append(2)` is an error — at class
//!    level *and* inside a function, which are two different code paths;
//! 4. a `.tscn` `Array[int]([1, 2])` property value loads typed.
//!
//! ⚠️ Scope of clause 4. The real `.tscn` reader is
//! `gdscene::packed_scene`, which calls `gdresource::loader::parse_variant_value`
//! for every node property (`packed_scene.rs:494` and `:524`, checked). This
//! crate cannot depend on `gdscene` — `gdscene -> gdscript-interop` is the
//! direction, so the reverse is a cycle — so the assertion here goes through
//! that same shared function rather than through a whole scene file. What it
//! therefore does *not* prove is the `[node]`-section plumbing around the
//! value; it does prove the value itself loads typed.
//!
//! ⚠️ The two negative guards at the bottom are the ones to keep. Making
//! `Array[int]` reject a `String` meant routing every array mutation through
//! the element check, and the cheap way to get clause 1 passing is a change
//! that also rejects writes it should allow. An untyped array must still take
//! anything, and a typed array must still take values that fit.

use gdresource::loader::parse_variant_value;
use gdscript_interop::interpreter::Interpreter;
use gdvariant::{Variant, VariantType};

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
// Clause 1 + 2 — a declared Array[int] is typed, and enforces its type
// ---------------------------------------------------------------------------

#[test]
fn a_declared_typed_array_reports_its_element_type() {
    assert_eq!(
        run("var a: Array[int] = []\nreturn a.is_typed()"),
        Ok(Variant::Bool(true)),
        "the declaration's element type must reach the container"
    );
    // Compared against the engine's own TYPE_INT rather than the literal 2, so
    // the test cannot drift from the constant the engine publishes.
    assert_eq!(
        run("var a: Array[int] = []\nreturn a.get_typed_builtin() == TYPE_INT"),
        Ok(Variant::Bool(true)),
        "get_typed_builtin must report TYPE_INT"
    );
    assert_eq!(
        run("var a: Array[int] = []\nreturn a.get_typed_builtin()"),
        Ok(Variant::Int(2)),
        "TYPE_INT is 2; asserted directly too, so a broken TYPE_INT cannot make \
         the comparison above pass trivially"
    );
}

#[test]
fn appending_the_wrong_type_to_a_declared_typed_array_is_an_error() {
    let got = run("var a: Array[int] = []\na.append(\"x\")\nreturn a");
    assert!(
        got.is_err(),
        "a String appended to Array[int] must be refused, not silently stored: {got:?}"
    );
}

#[test]
fn a_declared_typed_array_keeps_its_initial_elements() {
    // ⚠️ The interesting case: `set_typed` refuses a *non-empty* array, so a
    // non-empty initialiser can only work by constructing a typed array and
    // assigning into it, which is what Godot's OPCODE_ASSIGN_TYPED_ARRAY does.
    assert_eq!(
        run("var a: Array[int] = [1, 2]\nreturn a.size()"),
        Ok(Variant::Int(2))
    );
    assert_eq!(
        run("var a: Array[int] = [1, 2]\nreturn a.is_typed()"),
        Ok(Variant::Bool(true))
    );
}

#[test]
fn an_initialiser_that_does_not_fit_the_declared_type_is_an_error() {
    let got = run("var a: Array[int] = [\"x\"]\nreturn a");
    assert!(
        got.is_err(),
        "the declaration itself must refuse an element that does not fit: {got:?}"
    );
}

#[test]
fn a_bare_array_hint_leaves_the_array_untyped() {
    // `var a: Array` constrains the variable, not the elements. Typing here
    // would both be wrong and rebind a fresh handle, breaking pat-s3rrv.8's
    // reference semantics.
    assert_eq!(
        run("var a: Array = [1]\nreturn a.is_typed()"),
        Ok(Variant::Bool(false))
    );
    assert_eq!(
        run("var a: Array = [1]\nvar b = a\nb.append(2)\nreturn a.size()"),
        Ok(Variant::Int(2)),
        "a bare Array hint must not detach the alias"
    );
}

#[test]
fn a_declared_typed_dictionary_reports_both_types() {
    assert_eq!(
        run("var d: Dictionary[String, int] = {}\nreturn d.is_typed()"),
        Ok(Variant::Bool(true))
    );
    let got = run("var d: Dictionary[String, int] = {}\nd[\"k\"] = \"x\"\nreturn d");
    assert!(
        got.is_err(),
        "a String value in Dictionary[String, int] must be refused: {got:?}"
    );
}

// ---------------------------------------------------------------------------
// Clause 3 — const containers are read-only
// ---------------------------------------------------------------------------

#[test]
fn a_const_array_declared_in_a_function_is_read_only() {
    assert_eq!(
        run("const C = [1]\nreturn C.is_read_only()"),
        Ok(Variant::Bool(true))
    );
    let got = run("const C = [1]\nC.append(2)\nreturn C");
    assert!(
        got.is_err(),
        "appending to a const array must be refused: {got:?}"
    );
}

#[test]
fn a_class_level_const_array_is_read_only() {
    // ⚠️ A separate path from the one above: class constants are stored as
    // unevaluated expressions and evaluated when an instance is set up, so a
    // fix applied only to `exec_stmt` would leave this one mutable — which is
    // the spelling real Godot code actually uses.
    let got = call_method(
        "extends Node\nconst DIRS = [1, 2]\nfunc go():\n    DIRS.append(3)\n    return DIRS\n",
        "go",
    );
    assert!(
        got.is_err(),
        "appending to a class-level const array must be refused: {got:?}"
    );
    assert_eq!(
        call_method(
            "extends Node\nconst DIRS = [1, 2]\nfunc go():\n    return DIRS.is_read_only()\n",
            "go",
        ),
        Ok(Variant::Bool(true))
    );
}

#[test]
fn a_const_dictionary_is_read_only_too() {
    let got = run("const C = {\"k\": 1}\nC[\"k\"] = 2\nreturn C");
    assert!(
        got.is_err(),
        "writing into a const dictionary must be refused: {got:?}"
    );
}

// ---------------------------------------------------------------------------
// Clause 4 — a .tscn typed-array property loads typed
// ---------------------------------------------------------------------------

#[test]
fn a_tscn_typed_array_property_loads_typed() {
    let value = parse_variant_value("Array[int]([1, 2])").expect("a typed array literal parses");
    let Variant::Array(array) = value else {
        panic!("a typed array literal is still an Array");
    };
    assert!(
        array.is_typed(),
        "the element type written in the scene must survive the load"
    );
    assert_eq!(array.typed_builtin(), Some(VariantType::Int));
    assert_eq!(array.len(), 2, "the elements must load too");
    // And it enforces the type from then on, which is the point of loading it
    // typed rather than merely remembering a label.
    assert!(
        array.push(Variant::String("x".into())).is_err(),
        "a loaded typed array must refuse a mismatched element"
    );
}

#[test]
fn a_tscn_typed_array_still_equals_the_plain_literal() {
    // ⚠️ Load-bearing for two existing suites: `loader.rs`'s own
    // `parse_typed_array_value` and `tscn_value_parser_fidelity` both compare a
    // typed literal against the plain one. Godot's `Array::operator==` is
    // structural — the element type is not part of equality — so adding the
    // type must not change this. Asserted here so the coupling is visible from
    // the change that introduced it.
    let typed = parse_variant_value("Array[int]([1, 2])").expect("typed parses");
    let plain = parse_variant_value("[1, 2]").expect("plain parses");
    assert_eq!(typed, plain);
}

#[test]
fn a_tscn_untyped_array_property_stays_untyped() {
    let value = parse_variant_value("[1, 2]").expect("a plain array literal parses");
    let Variant::Array(array) = value else {
        panic!("a plain array literal is an Array");
    };
    assert!(!array.is_typed(), "a plain literal must not acquire a type");
}

#[test]
fn a_tscn_typed_array_whose_elements_disagree_is_an_error() {
    // A corrupt scene is better reported than loaded as a silently untyped
    // array, which would read back as working and fail much later.
    assert!(
        parse_variant_value("Array[int]([1, \"x\"])").is_err(),
        "a declared element type that its own literal violates must not load"
    );
}

// ---------------------------------------------------------------------------
// Negative guards — the type check must not over-restrict
// ---------------------------------------------------------------------------

#[test]
fn an_untyped_array_still_accepts_anything() {
    // ⚠️ Routing array mutation through the element check is what makes clause
    // 1 fail correctly; if it also constrained untyped arrays, most GDScript in
    // the engine would start erroring. This is the guard for that.
    assert_eq!(
        run("var a = []\na.append(\"x\")\na.append(1)\na.append(null)\nreturn a.size()"),
        Ok(Variant::Int(3))
    );
}

#[test]
fn a_typed_array_still_accepts_values_that_fit() {
    assert_eq!(
        run("var a: Array[int] = []\na.append(1)\na.append(2)\nreturn a.size()"),
        Ok(Variant::Int(2))
    );
}

#[test]
fn non_append_mutations_still_work_on_a_typed_array() {
    // Every array mutation shares one write path, so a sort or a reverse has to
    // keep working on a typed array — the elements are unchanged and must all
    // still satisfy the type.
    assert_eq!(
        run("var a: Array[int] = [3, 1, 2]\na.sort()\nreturn a[0]"),
        Ok(Variant::Int(1))
    );
    assert_eq!(
        run("var a: Array[int] = [1, 2, 3]\na.reverse()\nreturn a[0]"),
        Ok(Variant::Int(3))
    );
}

#[test]
fn an_undeclared_array_is_unaffected_by_all_of_this() {
    // The common case, kept honest: no hint, no const, nothing changes.
    assert_eq!(
        run("var a = [1]\nvar b = a\nb.append(2)\nreturn a.size()"),
        Ok(Variant::Int(2)),
        "reference semantics must survive"
    );
    assert_eq!(
        run("var a = [1]\nreturn a.is_read_only()"),
        Ok(Variant::Bool(false))
    );
}
