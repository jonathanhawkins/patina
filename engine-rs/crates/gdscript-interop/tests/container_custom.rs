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
//! **Scope note:** this covers the subset of pat-s3rrv.8's method-table gap that
//! does not depend on reshaping `Variant::Array`/`Variant::Dictionary`. The
//! typed-container queries (`is_typed`, `get_typed_builtin`, …), the read-only
//! flags (`make_read_only`, `is_read_only`), `assign`, and `duplicate_deep` all
//! need per-value state or `Resource.DeepDuplicateMode`, which arrive with the
//! shared-storage refactor — see the bead's `remaining_scope`.

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
        Ok(Variant::Array(vec![
            Variant::String("a".into()),
            Variant::String("b".into()),
            Variant::String("c".into()),
        ]))
    );
}

#[test]
fn dictionary_sort_keeps_each_key_with_its_own_value() {
    // Sorting must move the pairs, not just the key column.
    assert_eq!(
        run("var d = {\"c\": 2, \"a\": 0, \"b\": 1}\nd.sort()\nreturn d.values()"),
        Ok(Variant::Array(vec![
            Variant::Int(0),
            Variant::Int(1),
            Variant::Int(2),
        ]))
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
