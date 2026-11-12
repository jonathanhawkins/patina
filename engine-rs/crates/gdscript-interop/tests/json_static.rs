//! pat-0q2kx.216 — `JSON.parse_string` null-on-error, and `JSON.stringify`'s
//! `indent` / `sort_keys` arguments.
//!
//! Godot ground truth: `core/io/json.cpp` and `doc/classes/JSON.xml`, whose
//! signature is
//! `stringify(data, indent: String = "", sort_keys: bool = true, full_precision: bool = false)`
//! and `parse_string(json_string) -> Variant` (null on bad input).
//!
//! Before this bead `stringify` called `serde_json::to_string(&value)` and
//! dropped every argument after the first, and `parse_string` raised a runtime
//! error where Godot returns null.
//!
//! ⚠️ SCOPE: the bead also asks for an instantiable `JSON` handle
//! (`JSON.new().parse(...)` with `get_error_line` / `get_error_message` /
//! `get_data` / `get_parsed_text`) and `from_native` / `to_native`. Neither is
//! delivered; the acceptance test `json_instance_error_reporting` does not
//! exist. See the completion report for why.
//!
//! These are integration tests on purpose: `-p gdscript-interop --lib` cannot
//! pass (a pre-existing `fuzz_deeply_nested_parens` stack overflow cancels the
//! run), so unit tests here would be unverifiable.

use gdscript_interop::interpreter::Interpreter;
use gdvariant::Variant;

/// Runs `src` and returns its `return` value.
///
/// The trailing newline is added here rather than written into every literal:
/// every script in the sibling interop tests terminates its last statement, and
/// none omit it, so this keeps the proven shape without cluttering the cases.
fn run_val(src: &str) -> Variant {
    let mut interp = Interpreter::new();
    interp
        .run(&format!("{src}\n"))
        .unwrap_or_else(|e| panic!("script failed: {e:?}"))
        .return_value
        .expect("script returned a value")
}

fn run_str(src: &str) -> String {
    match run_val(src) {
        Variant::String(s) => s,
        other => panic!("expected a String, got {other:?}"),
    }
}

/// `parse_string` returns null on malformed input rather than raising — the
/// difference between a script that can check its input and one that dies.
#[test]
fn parse_string_returns_null_on_bad_input() {
    assert_eq!(
        run_val("return JSON.parse_string('{\"a\": [1,')"),
        Variant::Nil,
        "Godot's parse_string pushes an error and returns null (json.cpp); \
         raising instead makes the failure unrecoverable from script"
    );
    assert_eq!(
        run_val("return JSON.parse_string('not json at all')"),
        Variant::Nil
    );

    // Valid input still parses — the null path must not have swallowed success.
    assert_eq!(
        run_val("return JSON.parse_string('{\"a\": 1}').a"),
        Variant::Int(1),
        "a well-formed document must still round-trip"
    );
    // `null` as *valid* JSON also yields Nil, so the two are indistinguishable
    // by value. Recorded rather than asserted away: it is why Godot pairs the
    // static with the instance API, which reports an error code separately.
    assert_eq!(run_val("return JSON.parse_string('null')"), Variant::Nil);
}

/// An omitted `indent` keeps the compact form, so existing callers are
/// unaffected by the new arguments.
#[test]
fn stringify_without_indent_stays_compact() {
    assert_eq!(run_str("return JSON.stringify({\"a\": 1})"), "{\"a\":1}");
    assert_eq!(run_str("return JSON.stringify([1, 2])"), "[1,2]");
    assert_eq!(run_str("return JSON.stringify({})"), "{}");
    assert_eq!(run_str("return JSON.stringify([])"), "[]");
}

/// `indent` produces Godot's pretty form. This argument was previously parsed
/// and then ignored.
#[test]
fn stringify_honours_the_indent_argument() {
    assert_eq!(
        run_str("return JSON.stringify({\"a\": 1, \"b\": 2}, '  ')"),
        "{\n  \"a\": 1,\n  \"b\": 2\n}",
        "two-space indent, newline-separated, `: ` after each key"
    );

    // Nesting must increase the pad, which a single-level implementation gets
    // wrong while still looking indented at the top level.
    assert_eq!(
        run_str("return JSON.stringify({\"o\": {\"k\": 1}}, '  ')"),
        "{\n  \"o\": {\n    \"k\": 1\n  }\n}",
        "nested containers indent cumulatively"
    );
    assert_eq!(
        run_str("return JSON.stringify([1, [2]], '  ')"),
        "[\n  1,\n  [\n    2\n  ]\n]"
    );

    // Empty containers stay on one line even when pretty-printing, matching
    // the compact case rather than emitting a lone newline.
    assert_eq!(
        run_str("return JSON.stringify({\"a\": {}}, '  ')"),
        "{\n  \"a\": {}\n}"
    );

    // A tab indent works too — `indent` is an arbitrary string, not a width.
    assert_eq!(
        run_str("return JSON.stringify({\"a\": 1}, '\\t')"),
        "{\n\t\"a\": 1\n}"
    );
}

/// `sort_keys` defaults to **true**, which is the surprising half of the
/// signature — an omitted third argument must still sort.
#[test]
fn stringify_sorts_keys_by_default_and_can_be_turned_off() {
    // Written b, c, a; default output must be a, b, c.
    let src = "return JSON.stringify({\"b\": 1, \"c\": 2, \"a\": 3}%ARGS%)";
    assert_eq!(
        run_str(&src.replace("%ARGS%", "")),
        "{\"a\":3,\"b\":1,\"c\":2}",
        "Godot's sort_keys defaults to TRUE (JSON.xml), so omitting it sorts"
    );
    assert_eq!(
        run_str(&src.replace("%ARGS%", ", '', true")),
        "{\"a\":3,\"b\":1,\"c\":2}",
        "explicit true matches the default"
    );

    // false must preserve insertion order. This is the case that proves the
    // implementation does not just lean on serde_json, whose Map is a BTreeMap
    // in this workspace and is therefore always sorted.
    assert_eq!(
        run_str(&src.replace("%ARGS%", ", '', false")),
        "{\"b\":1,\"c\":2,\"a\":3}",
        "sort_keys = false must emit Dictionary insertion order"
    );

    // Sorting applies at every depth, not only the top level.
    assert_eq!(
        run_str("return JSON.stringify({\"z\": {\"y\": 1, \"x\": 2}}, '', true)"),
        "{\"z\":{\"x\":2,\"y\":1}}"
    );
}

/// The two arguments compose, and `full_precision` is accepted rather than
/// rejected — a correct Godot 4-argument call must not fail.
#[test]
fn indent_and_sort_keys_compose_and_full_precision_is_accepted() {
    assert_eq!(
        run_str("return JSON.stringify({\"b\": 1, \"a\": 2}, '  ', true)"),
        "{\n  \"a\": 2,\n  \"b\": 1\n}"
    );
    assert_eq!(
        run_str("return JSON.stringify({\"b\": 1, \"a\": 2}, '  ', false)"),
        "{\n  \"b\": 1,\n  \"a\": 2\n}"
    );
    // Four arguments, as Godot allows.
    assert_eq!(
        run_str("return JSON.stringify({\"a\": 1}, '', true, false)"),
        "{\"a\":1}"
    );
}

/// Strings and scalars still serialize through serde_json, so escaping is
/// unchanged by the new container framing.
#[test]
fn scalar_escaping_is_unchanged() {
    assert_eq!(
        run_str("return JSON.stringify({\"k\": \"a\\\"b\"})"),
        "{\"k\":\"a\\\"b\"}",
        "quotes inside a value stay escaped"
    );
    assert_eq!(
        run_str("return JSON.stringify({\"a\\\"b\": 1})"),
        "{\"a\\\"b\":1}",
        "and so do quotes inside a KEY, which the walker writes itself"
    );
    assert_eq!(run_str("return JSON.stringify(true)"), "true");
    assert_eq!(run_str("return JSON.stringify(null)"), "null");
}

/// Round-trip: stringify then parse_string returns the original structure.
#[test]
fn stringify_output_parses_back() {
    assert_eq!(
        run_val("return JSON.parse_string(JSON.stringify({\"a\": 1, \"b\": 2}, '  ')).b"),
        Variant::Int(2),
        "pretty output must still be valid JSON"
    );
    assert_eq!(
        run_val("return JSON.parse_string(JSON.stringify([1, [2]], '\\t'))[1][0]"),
        Variant::Int(2)
    );
}
