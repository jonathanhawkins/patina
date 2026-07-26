//! pat-0q2kx.165 — `RegEx`/`RegExMatch` are reachable from GDScript.
//!
//! `gdcore::regex` has been a complete engine all along — compile, search,
//! search_all, sub, named groups, backed by the `regex` crate — and the editor
//! used it internally. But neither class was registered in ClassDB and neither
//! was bound in the interpreter, so a game script could not construct one:
//! `RegEx.new()` fell through the `.new()` gate and errored.
//!
//! Both are `RefCounted` in Godot, so they are handle-table objects here rather
//! than scene nodes — the same shape as ConfigFile/CameraFeed/UndoRedo.

use gdobject::class_db::class_exists;
use gdobject::register_3d_classes;
use gdscript_interop::interpreter::Interpreter;
use gdvariant::Variant;

/// Run `method` on a fresh instance of `src` through the real VM dispatch.
fn call_method(src: &str, method: &str) -> Variant {
    let mut interp = Interpreter::new();
    let class_def = interp.run_class(src).expect("script parses");
    let mut inst = interp
        .instantiate_class(&class_def)
        .expect("class instantiates");
    interp
        .call_instance_method(&mut inst, method, &[])
        .unwrap_or_else(|e| panic!("call {method}() failed: {e:?}"))
}

/// **The acceptance test.**
#[test]
fn regex_scriptable_from_gdscript() {
    // -- the script path: construct, compile, search, read the match ---------
    //
    // `[0-9]+` deliberately avoids backslashes so the assertion is about RegEx
    // wiring and not about GDScript's string-escape lexing.
    let src = "\
extends Node

func check():
    var re = RegEx.new()
    re.compile(\"[0-9]+\")
    var m = re.search(\"a12b\")
    return m.get_string()
";
    assert_eq!(
        call_method(src, "check"),
        Variant::String("12".to_string()),
        "RegEx.new().compile().search().get_string() must reach gdcore's engine",
    );

    // -- ClassDB knows both classes ------------------------------------------
    register_3d_classes();
    assert!(class_exists("RegEx"), "ClassDB must expose RegEx");
    assert!(class_exists("RegExMatch"), "ClassDB must expose RegExMatch");
}

/// The match object carries offsets and group structure, not just the text.
#[test]
fn regex_match_reports_offsets_and_groups() {
    let src = "\
extends Node

func check():
    var re = RegEx.new()
    re.compile(\"([0-9]+)-([0-9]+)\")
    var m = re.search(\"x 10-20 y\")
    return [m.get_string(), m.get_start(), m.get_group_count(), m.get_string(1), m.get_string(2)]
";
    let got = call_method(src, "check");
    let Variant::Array(items) = got else {
        panic!("check() should return an array, got {got:?}");
    };
    assert_eq!(items[0], Variant::String("10-20".to_string()), "full match");
    // "x " is two bytes, so the match starts at offset 2.
    assert_eq!(items[1], Variant::Int(2), "byte offset of the match start");
    assert_eq!(items[2], Variant::Int(2), "two capture groups");
    assert_eq!(items[3], Variant::String("10".to_string()), "group 1");
    assert_eq!(items[4], Variant::String("20".to_string()), "group 2");
}

/// A non-matching search returns null, which is what `if m:` branches on —
/// returning an empty match object instead would silently take the wrong branch.
#[test]
fn regex_search_returns_null_when_no_match() {
    let src = "\
extends Node

func check():
    var re = RegEx.new()
    re.compile(\"[0-9]+\")
    var m = re.search(\"no digits here\")
    return m == null
";
    assert_eq!(call_method(src, "check"), Variant::Bool(true));
}

/// `search_all` and `sub`, the other two methods scripts actually reach for.
#[test]
fn regex_search_all_and_sub() {
    let src = "\
extends Node

func check():
    var re = RegEx.new()
    re.compile(\"[0-9]+\")
    var hits = re.search_all(\"a1b22c333\")
    var replaced = re.sub(\"a1b22c333\", \"#\", true)
    return [hits.size(), hits[1].get_string(), replaced]
";
    let got = call_method(src, "check");
    let Variant::Array(items) = got else {
        panic!("check() should return an array, got {got:?}");
    };
    assert_eq!(items[0], Variant::Int(3), "three runs of digits");
    assert_eq!(items[1], Variant::String("22".to_string()), "second hit");
    assert_eq!(
        items[2],
        Variant::String("a#b#c#".to_string()),
        "sub(all=true) replaces every run",
    );
}

/// `compile` reports Godot's Error code, and `is_valid` follows it — a script
/// that checks `if re.compile(p) != OK` must be able to detect a bad pattern.
#[test]
fn regex_compile_reports_error_code() {
    let src = "\
extends Node

func check():
    var re = RegEx.new()
    var ok = re.compile(\"[0-9]+\")
    var valid = re.is_valid()
    var bad = re.compile(\"[unterminated\")
    return [ok, valid, bad]
";
    let got = call_method(src, "check");
    let Variant::Array(items) = got else {
        panic!("check() should return an array, got {got:?}");
    };
    assert_eq!(items[0], Variant::Int(0), "OK == 0 for a good pattern");
    assert_eq!(
        items[1],
        Variant::Bool(true),
        "is_valid after a good compile"
    );
    assert_eq!(items[2], Variant::Int(1), "FAILED == 1 for a bad pattern");
}
