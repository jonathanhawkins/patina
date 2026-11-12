//! pat-s3rrv.61 — the `%` operator matches Godot's `String::sprintf`.
//!
//! Patina handled exactly four specifiers — `%%`, `%s`, `%d`, `%f` (hard-coded
//! to six decimals) — plus `%v`. Everything else fell through to a branch that
//! pushed a bare `%` and left the letter as literal text, so `"%x" % 255`
//! produced `"%x"`-shaped output rather than `ff`. Width, precision, flags and
//! `*` did not exist, so `"%.2f"`, `"%04d"`, `"%c"` and `"%-10s"` were all
//! wrong.
//!
//! The quieter half: when arguments ran out the old code emitted **nothing**
//! and carried on. Godot RETURNS a diagnostic as the formatted string
//! (`ustring.cpp` :5210, :5280, :5325, :5395, :5413), so a script can see the
//! mistake. Silently dropping the specifier meant a malformed format string
//! produced plausible-looking output forever.
//!
//! Ground truth: `upstream/godot/core/string/ustring.cpp::String::sprintf`.
//! These tests drive real GDScript through the interpreter, so they prove the
//! operator as a script sees it.

use gdscript_interop::interpreter::Interpreter;
use gdvariant::Variant;

/// Evaluates `expr` as the return value of a real script.
fn eval(expr: &str) -> String {
    let src = format!("extends Node\nfunc run():\n\x20   return {expr}\n");
    let mut interp = Interpreter::new();
    let class_def = interp.run_class(&src).expect("script parses");
    let mut inst = interp.instantiate_class(&class_def).expect("instantiates");
    match interp
        .call_instance_method(&mut inst, "run", &[])
        .expect("run() executes")
    {
        Variant::String(s) => s,
        other => panic!("expected a String, got {other:?}"),
    }
}

/// **The acceptance test** — the bead's exact format string.
///
/// The expected output is what CPython's `%` produces for the same inputs,
/// which implements the same C `printf` rules Godot follows. Deriving it from a
/// second implementation rather than from my own reading is the point: a
/// hand-written expectation would encode whatever I believed the code did.
#[test]
fn string_percent_format_parity() {
    assert_eq!(
        eval(
            "\"%s scored %04d (%.2f%%) hex=%x chr=%c |%-6s|\" \
             % [\"Ada\", 7, 12.3456, 255, 65, \"ab\"]"
        ),
        "Ada scored 0007 (12.35%) hex=ff chr=A |ab    |",
    );
}

/// Running out of arguments RETURNS Godot's diagnostic (pat-s3rrv.61).
///
/// This is the assertion the old behaviour could never pass: it emitted nothing
/// for the missing argument and returned `"1 "`, which reads like success.
#[test]
fn too_few_arguments_returns_godots_diagnostic() {
    assert_eq!(
        eval("\"%d %d\" % [1]"),
        "not enough arguments for format string",
    );
    // A specifier with no argument at all, not merely a short list.
    assert_eq!(
        eval("\"%s\" % []"),
        "not enough arguments for format string",
    );
}

/// Integer specifiers, bases and flags.
#[test]
fn integer_specifiers_bases_and_flags() {
    assert_eq!(eval("\"%d\" % 42"), "42");
    assert_eq!(eval("\"%i\" % -42"), "-42");
    assert_eq!(eval("\"%o\" % 8"), "10");
    assert_eq!(eval("\"%x\" % 255"), "ff");
    assert_eq!(eval("\"%X\" % 255"), "FF");
    // Width, zero-pad, and left-justify.
    assert_eq!(eval("\"%5d\" % 42"), "   42");
    assert_eq!(eval("\"%05d\" % 42"), "00042");
    assert_eq!(eval("\"%-5d|\" % 42"), "42   |");
    // ⚠️ The sign goes BEFORE the zero padding. `00+42` would be the naive
    // result of padding the rendered string without splitting off the sign.
    assert_eq!(eval("\"%+06d\" % 42"), "+00042");
    assert_eq!(eval("\"%06d\" % -42"), "-00042");
    // A width smaller than the value never truncates.
    assert_eq!(eval("\"%2d\" % 123456"), "123456");
}

/// Float precision, including the default of six.
#[test]
fn float_precision_defaults_to_six() {
    assert_eq!(eval("\"%f\" % 1.5"), "1.500000");
    assert_eq!(eval("\"%.0f\" % 1.5"), "2");
    assert_eq!(eval("\"%.3f\" % 3.14159"), "3.142");
    assert_eq!(eval("\"%08.2f\" % 3.14159"), "00003.14");
    assert_eq!(eval("\"%+.1f\" % 2.0"), "+2.0");
}

/// `*` takes the width — and the precision — from the argument list
/// (pat-s3rrv.61).
///
/// The starred value is consumed BEFORE the value being formatted, so getting
/// the order wrong silently formats the width as the value.
#[test]
fn star_takes_width_and_precision_from_arguments() {
    assert_eq!(eval("\"%*d\" % [5, 42]"), "   42");
    assert_eq!(eval("\"%.*f\" % [2, 3.14159]"), "3.14");
    assert_eq!(eval("\"%*.*f\" % [8, 2, 3.14159]"), "    3.14");
}

/// `%c` accepts a code point or a one-character string.
#[test]
fn c_specifier_accepts_a_code_point_or_single_char() {
    assert_eq!(eval("\"%c\" % 65"), "A");
    assert_eq!(eval("\"%c\" % \"z\""), "z");
    // A multi-character string is Godot's error, not a silent truncation to
    // its first character.
    assert_eq!(
        eval("\"%c\" % \"abc\""),
        "%c requires number or single-character string",
    );
}

/// An unknown specifier is reported rather than passed through as text.
#[test]
fn an_unsupported_specifier_is_reported() {
    assert_eq!(eval("\"%q\" % 1"), "unsupported format character");
    // `%%` still consumes no argument and needs none.
    assert_eq!(eval("\"100%%\" % []"), "100%");
}

/// A non-numeric argument to a numeric specifier is an error (pat-s3rrv.61).
#[test]
fn a_numeric_specifier_rejects_a_non_number() {
    assert_eq!(eval("\"%d\" % \"abc\""), "a number is required");
}

/// Strings pad with SPACES even when `0` is given — zero-padding text would
/// change what it says (pat-s3rrv.61).
#[test]
fn string_fields_pad_with_spaces_not_zeros() {
    assert_eq!(eval("\"%6s|\" % \"ab\""), "    ab|");
    assert_eq!(eval("\"%-6s|\" % \"ab\""), "ab    |");
    assert_eq!(eval("\"%06s|\" % \"ab\""), "    ab|");
}
