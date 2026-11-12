//! pat-s3rrv.8 (partial) — the predicate-driven Array searches and the two
//! missing Dictionary methods.
//!
//! Godot ground truth: `doc/classes/Array.xml` and `Dictionary.xml`, with the
//! edge-case behaviour read from `core/variant/array.cpp` /
//! `core/variant/dictionary.cpp`:
//!
//! | method | signature |
//! |---|---|
//! | `Array.find_custom`    | `(method: Callable, from: int = 0) -> int` |
//! | `Array.rfind_custom`   | `(method: Callable, from: int = -1) -> int` |
//! | `Array.bsearch_custom` | `(value: Variant, func: Callable, before: bool = true) -> int` |
//! | `Dictionary.sort`      | `() -> void` (in place, ascending by key) |
//! | `Dictionary.recursive_equal` | `(dictionary: Dictionary, recursion_count: int) -> bool` |
//!
//! None of the five existed in Patina's method tables (`grep '"find_custom"'`
//! etc. over `interpreter.rs` returned 0 before this change), so a script
//! calling any of them died with `UndefinedFunction`.
//!
//! The asymmetry between `find_custom` and `rfind_custom` is deliberate and is
//! Godot's, not a slip: `find_custom` **bails** on a negative `from`, while
//! `rfind_custom` treats it as an offset from the end. Both are pinned below.
//!
//! **Scope note:** this covers the methods that are pure functions of the
//! elements. The ones that need per-container state (`is_typed`,
//! `get_typed_builtin`, `make_read_only`, `is_read_only`, `duplicate_deep`, …)
//! arrived with the pat-s3rrv.8 shared-storage flip and are pinned in
//! `container_typed_and_read_only.rs`.
//!
//! ⚠️ **Correction.** An earlier revision of this note listed `assign` in that
//! blocked set. That was wrong, and wrong in the direction that costs coverage:
//! `assign` needs no per-value state at all. For an untyped container it is a
//! plain contents copy, expressible through the same `array_mutate` /
//! `write_place` path `push_back` and `erase` already use — the type-conversion
//! clause in `Array.xml` only bites once containers can be *typed*, which they
//! cannot be here. It is implemented and pinned below.

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
// Array.find_custom / rfind_custom
// ---------------------------------------------------------------------------

#[test]
fn find_custom_returns_first_match() {
    assert_eq!(
        run("var a = [1, 5, 8, 3]\nreturn a.find_custom(func(n): return n > 4)"),
        Ok(Variant::Int(1))
    );
}

#[test]
fn find_custom_honours_the_start_index() {
    // Starting past the first match finds the second one.
    assert_eq!(
        run("var a = [1, 5, 8, 3]\nreturn a.find_custom(func(n): return n > 4, 2)"),
        Ok(Variant::Int(2))
    );
}

#[test]
fn find_custom_returns_minus_one_when_nothing_matches() {
    assert_eq!(
        run("var a = [1, 2, 3]\nreturn a.find_custom(func(n): return n > 99)"),
        Ok(Variant::Int(-1))
    );
    assert_eq!(
        run("var a = []\nreturn a.find_custom(func(n): return true)"),
        Ok(Variant::Int(-1))
    );
}

#[test]
fn find_custom_bails_on_a_negative_start() {
    // Godot's `find_custom` has no from-the-end addressing: a negative start
    // returns -1 outright (array.cpp `Array::find_custom`). Asserting this
    // matters because the obvious "clamp to 0" reading would find index 1.
    assert_eq!(
        run("var a = [1, 5, 8, 3]\nreturn a.find_custom(func(n): return n > 4, -1)"),
        Ok(Variant::Int(-1))
    );
}

#[test]
fn rfind_custom_returns_last_match() {
    assert_eq!(
        run("var a = [1, 5, 8, 3]\nreturn a.rfind_custom(func(n): return n > 4)"),
        Ok(Variant::Int(2))
    );
}

#[test]
fn rfind_custom_treats_a_negative_start_as_an_offset_from_the_end() {
    // -2 starts at index 2 and walks backwards, so index 2 (value 8) matches.
    assert_eq!(
        run("var a = [1, 5, 8, 3]\nreturn a.rfind_custom(func(n): return n > 4, -2)"),
        Ok(Variant::Int(2))
    );
    // -3 starts at index 1 (value 5).
    assert_eq!(
        run("var a = [1, 5, 8, 3]\nreturn a.rfind_custom(func(n): return n > 4, -3)"),
        Ok(Variant::Int(1))
    );
}

#[test]
fn rfind_custom_clamps_an_out_of_range_start_to_the_last_element() {
    assert_eq!(
        run("var a = [1, 5, 8, 3]\nreturn a.rfind_custom(func(n): return n > 4, 99)"),
        Ok(Variant::Int(2))
    );
    assert_eq!(
        run("var a = []\nreturn a.rfind_custom(func(n): return true)"),
        Ok(Variant::Int(-1))
    );
}

// ---------------------------------------------------------------------------
// Array.bsearch_custom
// ---------------------------------------------------------------------------

#[test]
fn bsearch_custom_finds_the_insertion_point() {
    assert_eq!(
        run("var a = [1, 3, 5, 7]\nreturn a.bsearch_custom(4, func(x, y): return x < y)"),
        Ok(Variant::Int(2))
    );
    assert_eq!(
        run("var a = [1, 3, 5, 7]\nreturn a.bsearch_custom(0, func(x, y): return x < y)"),
        Ok(Variant::Int(0))
    );
    assert_eq!(
        run("var a = [1, 3, 5, 7]\nreturn a.bsearch_custom(9, func(x, y): return x < y)"),
        Ok(Variant::Int(4))
    );
}

#[test]
fn bsearch_custom_before_flag_picks_the_side_of_a_run_of_equals() {
    // `before = true` (the default) lands before the run of 3s, `false` after —
    // this is the whole point of the flag (SortArray::bisect).
    assert_eq!(
        run("var a = [1, 3, 3, 3, 7]\nreturn a.bsearch_custom(3, func(x, y): return x < y)"),
        Ok(Variant::Int(1))
    );
    assert_eq!(
        run("var a = [1, 3, 3, 3, 7]\nreturn a.bsearch_custom(3, func(x, y): return x < y, false)"),
        Ok(Variant::Int(4))
    );
}

// ---------------------------------------------------------------------------
// Dictionary.sort / recursive_equal
// ---------------------------------------------------------------------------

#[test]
fn dictionary_sort_orders_entries_by_key_in_place() {
    assert_eq!(
        run("var d = {\"c\": 2, \"a\": 0, \"b\": 1}\nd.sort()\nreturn d.keys()"),
        Ok(Variant::Array(
            vec![
                Variant::String("a".into()),
                Variant::String("b".into()),
                Variant::String("c".into()),
            ]
            .into()
        ))
    );
}

#[test]
fn dictionary_sort_keeps_each_key_with_its_own_value() {
    // Sorting must move the pairs, not just the key column.
    assert_eq!(
        run("var d = {\"c\": 2, \"a\": 0, \"b\": 1}\nd.sort()\nreturn d.values()"),
        Ok(Variant::Array(
            vec![Variant::Int(0), Variant::Int(1), Variant::Int(2),].into()
        ))
    );
}

#[test]
fn recursive_equal_compares_nested_containers() {
    assert_eq!(
        run("var a = {\"x\": [1, 2], \"y\": {\"z\": 3}}\n\
             var b = {\"x\": [1, 2], \"y\": {\"z\": 3}}\n\
             return a.recursive_equal(b, 0)"),
        Ok(Variant::Bool(true))
    );
    // A difference buried in a nested container is still a difference.
    assert_eq!(
        run("var a = {\"x\": [1, 2], \"y\": {\"z\": 3}}\n\
             var b = {\"x\": [1, 2], \"y\": {\"z\": 4}}\n\
             return a.recursive_equal(b, 0)"),
        Ok(Variant::Bool(false))
    );
}

#[test]
fn recursive_equal_is_false_for_different_sizes_and_keys() {
    assert_eq!(
        run("var a = {\"x\": 1}\nvar b = {\"x\": 1, \"y\": 2}\nreturn a.recursive_equal(b, 0)"),
        Ok(Variant::Bool(false))
    );
    assert_eq!(
        run("var a = {\"x\": 1}\nvar b = {\"y\": 1}\nreturn a.recursive_equal(b, 0)"),
        Ok(Variant::Bool(false))
    );
}

#[test]
fn recursive_equal_gives_up_true_past_the_recursion_budget() {
    // Godot returns `true` once the budget is spent rather than reporting
    // inequality, so a cyclic structure terminates (dictionary.cpp:277).
    assert_eq!(
        run("var a = {\"x\": 1}\nvar b = {\"y\": 2}\nreturn a.recursive_equal(b, 2000)"),
        Ok(Variant::Bool(true))
    );
}

// ---------------------------------------------------------------------------
// Array.assign / Dictionary.assign
// ---------------------------------------------------------------------------
//
// `assign(other)` copies another container's CONTENTS into this one, resizing
// to match (`Array.xml`, `Dictionary.xml`). Before this change both raised
// `UndefinedFunction`, so every assertion here fails against the old tree
// rather than passing vacuously.

#[test]
fn assign_resizes_the_receiver_to_match_the_source() {
    // 3 elements in, 2 out. This is the assertion that separates `assign` from
    // an element-wise overwrite, which would leave the stale third element and
    // report a size of 3.
    assert_eq!(
        run("var a = [1, 2, 3]\nvar b = [9, 9]\na.assign(b)\nreturn a.size()"),
        Ok(Variant::Int(2))
    );
}

#[test]
fn assign_replaces_every_element() {
    assert_eq!(
        run("var a = [1, 2, 3]\nvar b = [9, 8]\na.assign(b)\nreturn a"),
        Ok(Variant::Array(
            vec![Variant::Int(9), Variant::Int(8)].into()
        ))
    );
}

#[test]
fn assign_grows_a_shorter_receiver() {
    // The opposite direction from the shrink above: a receiver *smaller* than
    // the source must take all of it, not just as many as it had room for.
    assert_eq!(
        run("var a = [1]\nvar b = [7, 8, 9]\na.assign(b)\nreturn a"),
        Ok(Variant::Array(
            vec![Variant::Int(7), Variant::Int(8), Variant::Int(9)].into()
        ))
    );
}

#[test]
fn assign_leaves_the_source_alone() {
    // ⚠️ Honest label: this CANNOT fail while Array is a value type — nothing
    // can alias anything. It is here as the pin for pat-s3rrv.8's shared-storage
    // flip, after which `a.assign(b)` must still copy rather than make the two
    // share storage. That difference is the entire reason Godot has the method,
    // so the flip must not be allowed to land without this staying green.
    assert_eq!(
        run("var a = [1]\nvar b = [2, 3]\na.assign(b)\na.push_back(4)\nreturn b"),
        Ok(Variant::Array(
            vec![Variant::Int(2), Variant::Int(3)].into()
        ))
    );
}

#[test]
fn assign_rejects_a_non_array() {
    let err = run("var a = [1]\na.assign(5)\nreturn a").unwrap_err();
    assert!(
        err.contains("takes an Array"),
        "a non-Array argument must be refused by type, got {err}"
    );
}

#[test]
fn dictionary_assign_drops_keys_the_source_does_not_have() {
    // 🔑 The discriminating case. A `merge`-shaped implementation keeps "x" and
    // passes every other assertion in this block; only this one catches it.
    assert_eq!(
        run("var a = {\"x\": 1, \"y\": 2}\nvar b = {\"z\": 3}\na.assign(b)\nreturn a.has(\"x\")"),
        Ok(Variant::Bool(false))
    );
    assert_eq!(
        run("var a = {\"x\": 1, \"y\": 2}\nvar b = {\"z\": 3}\na.assign(b)\nreturn a.size()"),
        Ok(Variant::Int(1))
    );
}

#[test]
fn dictionary_assign_brings_the_source_entries_across() {
    assert_eq!(
        run("var a = {\"x\": 1}\nvar b = {\"z\": 3}\na.assign(b)\nreturn a[\"z\"]"),
        Ok(Variant::Int(3))
    );
}

#[test]
fn dictionary_assign_rejects_a_non_dictionary() {
    let err = run("var a = {\"x\": 1}\na.assign([1])\nreturn a").unwrap_err();
    assert!(
        err.contains("takes a Dictionary"),
        "a non-Dictionary argument must be refused by type, got {err}"
    );
}
