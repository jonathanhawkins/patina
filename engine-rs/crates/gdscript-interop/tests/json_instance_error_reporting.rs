//! pat-0q2kx.216 — the instantiable `JSON` resource and `from_native` /
//! `to_native`.
//!
//! Godot ground truth: `core/io/json.cpp`. `JSON` is both a static utility and a
//! resource: `JSON.new().parse(text)` returns an **Error code** and the instance
//! then answers `get_error_line` / `get_error_message` / `get_data` /
//! `get_parsed_text`.
//!
//! This is the half `json_static.rs` explicitly left undelivered — that file
//! covers `parse_string`'s null-on-error and `stringify`'s indent/sort_keys, and
//! says so in its own scope note. Nothing here re-tests those.
//!
//! ⚠️ Integration test on purpose: `-p gdscript-interop --lib` cannot pass (a
//! pre-existing `fuzz_deeply_nested_parens` stack overflow cancels the run), so
//! a unit test beside the code would be unverifiable.

use gdscript_interop::interpreter::Interpreter;
use gdvariant::Variant;

fn run_val(src: &str) -> Variant {
    let mut interp = Interpreter::new();
    interp
        .run(&format!("{src}\n"))
        .unwrap_or_else(|e| panic!("script failed: {e:?}"))
        .return_value
        .expect("script returned a value")
}

fn array(v: Variant) -> Vec<Variant> {
    match v {
        Variant::Array(a) => a,
        other => panic!("expected an Array, got {other:?}"),
    }
}

/// **The acceptance's first clause.** A failed instance parse reports a nonzero
/// Error code, a line number, and a message — and does **not** raise.
///
/// Raising is the tempting shortcut and it is wrong: the whole point of the
/// instance API is that a script checks the code and then reads the diagnostics.
/// A script that died on `parse()` could never reach `get_error_line()`.
#[test]
fn a_failed_instance_parse_reports_code_line_and_message() {
    let v = array(run_val(
        "var j = JSON.new()\nvar code = j.parse('{\"a\": [1,')\nreturn [code, j.get_error_line(), j.get_error_message()]",
    ));

    match v[0] {
        Variant::Int(code) => assert_ne!(code, 0, "a malformed parse must report a nonzero Error"),
        ref other => panic!("parse() must return an Error code, got {other:?}"),
    }
    match v[1] {
        // Godot and serde_json both count lines from 1. Zero is what "no error"
        // looks like, so reporting 0 for a first-line failure would be
        // indistinguishable from success.
        Variant::Int(line) => assert!(line > 0, "error line must be > 0, got {line}"),
        ref other => panic!("get_error_line() must be an int, got {other:?}"),
    }
    match &v[2] {
        Variant::String(m) => assert!(!m.is_empty(), "error message must not be empty"),
        other => panic!("get_error_message() must be a String, got {other:?}"),
    }
}

/// **The acceptance's second clause.** A successful parse exposes the structure
/// through `get_data()` and the source through `get_parsed_text()`.
#[test]
fn a_successful_instance_parse_exposes_data_and_source_text() {
    let v = array(run_val(
        "var j = JSON.new()\nvar code = j.parse('{\"a\": 1}')\nreturn [code, j.get_data().a, j.get_parsed_text(), j.get_error_line()]",
    ));

    assert_eq!(v[0], Variant::Int(0), "a valid parse must report OK (0)");
    assert_eq!(v[1], Variant::Int(1), "get_data() must carry the structure");
    assert_eq!(
        v[2],
        Variant::String("{\"a\": 1}".to_string()),
        "get_parsed_text() must return the source that was handed to parse()"
    );
    assert_eq!(
        v[3],
        Variant::Int(0),
        "a successful parse must clear the previous error line, not leave it set"
    );
}

/// A second parse must replace the first result, not accumulate onto it.
///
/// The instance keeps its last parse, so a stale error surviving a later success
/// (or stale data surviving a later failure) is the realistic bug here.
#[test]
fn a_later_parse_replaces_the_previous_result() {
    let v = array(run_val(
        "var j = JSON.new()\nj.parse('{\"a\": 1}')\nvar code = j.parse('{oops')\nreturn [code, j.get_data(), j.get_error_line()]",
    ));

    assert_ne!(v[0], Variant::Int(0), "the second parse failed");
    assert_eq!(
        v[1],
        Variant::Nil,
        "a failed parse must clear the previous parse's data, not keep serving it"
    );
    match v[2] {
        Variant::Int(line) => assert!(line > 0, "the failure's line must be recorded"),
        ref other => panic!("expected an int line, got {other:?}"),
    }
}

/// **The acceptance's fifth clause.** `to_native(from_native(v))` round-trips a
/// Variant JSON cannot express natively.
#[test]
fn from_native_to_native_round_trips_a_vector2() {
    let v = run_val("return JSON.to_native(JSON.from_native(Vector2(1, 2)))");
    match v {
        Variant::Vector2(p) => {
            assert!(
                (p.x - 1.0).abs() < 1e-6 && (p.y - 2.0).abs() < 1e-6,
                "Vector2 must survive the round trip, got {p:?}"
            );
        }
        other => panic!("expected a Vector2 back, got {other:?}"),
    }
}

/// `from_native` must actually *tag* the type, not pass the Variant through.
///
/// Without the tag the round trip above could still "pass" by never converting
/// anything — this pins that the intermediate really is JSON-native data
/// carrying a recoverable type marker.
#[test]
fn from_native_tags_the_type_for_json_native_data() {
    // Indexed rather than `.type` — a bare `type` risks reading as a keyword,
    // and this test is about the marker, not about member-access syntax.
    let v = run_val("return JSON.from_native(Vector2(1, 2))[\"type\"]");
    assert_eq!(v, Variant::String("Vector2".to_string()));
}

/// Plain JSON data must survive `to_native` untouched — an object without a
/// recognised `type`/`args` pair is ordinary data, not a failed marker.
#[test]
fn to_native_leaves_untagged_data_alone() {
    let v = run_val("return JSON.to_native(JSON.parse_string('{\"a\": 1}')).a");
    assert_eq!(v, Variant::Int(1));
}

/// The bead's named acceptance, running its clauses together.
#[test]
fn json_instance_error_reporting() {
    a_failed_instance_parse_reports_code_line_and_message();
    a_successful_instance_parse_exposes_data_and_source_text();
    from_native_to_native_round_trips_a_vector2();
}
