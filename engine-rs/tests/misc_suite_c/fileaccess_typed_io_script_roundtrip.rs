//! pat-mgixk.54 acceptance: the typed FileAccess surface is reachable from GDScript.
//!
//! `gdcore::FileAccess` has implemented `store_8/16/32/64/float/double`,
//! `get_8..get_double`, `seek`, `get_length` and `eof_reached` all along — but
//! `call_file_access_instance` had arms only for `get_as_text`, `get_buffer`,
//! `store_string`, `store_buffer`, `close` and `get_error`. Every other method
//! raised `UndefinedFunction` while the engine function sat a few lines away, so
//! save-game and binary-format code could not be written at all.
//!
//! Two other Godot behaviours this pins, both of which a save system depends on:
//!
//! * `FileAccess.open` returns **null** on failure and records the reason for
//!   `get_open_error()` (`core/io/file_access.cpp`). Patina raised instead, so
//!   `var f = FileAccess.open(p, READ); if f == null:` — the idiom every save
//!   system is built around — killed the script before it could test anything.
//! * `get_buffer` returns a `PackedByteArray`, not an `Array` of `Int`.
//!
//! ⚠️ Written through `user://` on purpose: that is the path a real game saves to,
//! and it exercises the user-root resolution rather than the project root.

use std::path::PathBuf;

use gdcore::set_path_roots;
use gdscript_interop::interpreter::Interpreter;
use gdvariant::Variant;

fn with_temp_roots<F: FnOnce()>(f: F) {
    let project = tempfile::tempdir().expect("project tempdir");
    let user = tempfile::tempdir().expect("user tempdir");
    set_path_roots(
        Some(PathBuf::from(project.path())),
        Some(PathBuf::from(user.path())),
    );
    f();
    set_path_roots(None, None);
}

fn run_val(src: &str) -> Variant {
    let mut interp = Interpreter::new();
    let result = interp.run(src).expect("script should not error");
    result.return_value.expect("expected a return value")
}

/// **The acceptance test (pat-mgixk.54 part 1).**
///
/// One script writes every typed width and reads them all back, so each assertion
/// is a true round trip through the file rather than a check that a getter returns
/// *something*.
#[test]
fn fileaccess_typed_io_script_roundtrip() {
    with_temp_roots(|| {
        let val = run_val(
            r#"
var w = FileAccess.open("user://typed.bin", FileAccess.WRITE)
w.store_8(200)
w.store_16(40000)
w.store_32(3000000000)
w.store_64(42)
w.store_float(0.5)
w.store_double(1.25)
w.store_line("hello")
w.store_buffer(PackedByteArray([1, 2, 3]))
var wrote = w.get_position()
w.close()

var r = FileAccess.open("user://typed.bin", FileAccess.READ)
var out = []
out.append(r.get_8())
out.append(r.get_16())
out.append(r.get_32())
out.append(r.get_64())
out.append(r.get_float())
out.append(r.get_double())
out.append(r.get_line())
out.append(r.get_buffer(3))
out.append(r.get_length())
out.append(wrote)
out.append(r.eof_reached())
r.close()
return out
"#,
        );

        let items = match &val {
            Variant::Array(a) => a.to_vec(),
            other => panic!("the script returns an Array; got {other:?}"),
        };
        assert_eq!(items.len(), 11, "one entry per read, plus the two sizes");

        // -- the integer widths come back exactly, including the unsigned edges --
        assert_eq!(items[0], Variant::Int(200), "store_8/get_8");
        assert_eq!(items[1], Variant::Int(40000), "store_16/get_16");
        // Above i32::MAX on purpose: a u32 round-tripped through a signed
        // intermediate would come back negative.
        assert_eq!(items[2], Variant::Int(3000000000), "store_32/get_32");
        assert_eq!(items[3], Variant::Int(42), "store_64/get_64");

        // -- floats: both widths, and 0.5/1.25 are exact in binary32 and binary64
        //    so this cannot fail on rounding --------------------------------
        assert_eq!(items[4], Variant::Float(0.5), "store_float/get_float");
        assert_eq!(items[5], Variant::Float(1.25), "store_double/get_double");

        // -- store_line writes exactly "\n" and get_line strips it ------------
        assert_eq!(
            items[6],
            Variant::String("hello".into()),
            "store_line/get_line must round-trip without the newline"
        );

        // -- get_buffer is a PackedByteArray, not an Array of Int -------------
        //
        // Asserted on the TYPE, not just the contents: an `Array` of `Int` would
        // compare unequal here, which is the point — a script doing
        // `var b: PackedByteArray = f.get_buffer(n)` needs the real type.
        assert_eq!(
            items[7],
            Variant::PackedByteArray(vec![1, 2, 3]),
            "get_buffer must return a PackedByteArray; an Array of Int is the bug"
        );

        // -- positions and lengths agree -------------------------------------
        //
        // 1+2+4+8+4+8 typed bytes = 27, + "hello\n" = 33, + 3 buffer bytes = 36.
        // Asserted as an identity between the writer's final position and the
        // reader's length rather than against a hand-counted literal alone, so a
        // change to any single width cannot quietly pass.
        assert_eq!(items[8], Variant::Int(36), "get_length after the writes");
        assert_eq!(
            items[9], items[8],
            "the writer's final get_position must equal the file's length"
        );

        // Everything was consumed, so the cursor is at the end.
        assert_eq!(
            items[10],
            Variant::Bool(true),
            "eof_reached after reading every byte"
        );
    });
}

/// `FileAccess.open` must hand back `null` for a missing file, not kill the script.
///
/// Separate test because the failure mode is "the script never reaches line 2":
/// folding it into the round trip above would have made a raise look like a
/// round-trip failure.
#[test]
fn opening_a_missing_file_returns_null_and_records_the_error() {
    with_temp_roots(|| {
        let val = run_val(
            r#"
var f = FileAccess.open("user://definitely_absent.bin", FileAccess.READ)
var was_null = f == null
var err = FileAccess.get_open_error()
return [was_null, err != 0]
"#,
        );
        let items = match &val {
            Variant::Array(a) => a.to_vec(),
            other => panic!("expected an Array; got {other:?}"),
        };
        assert_eq!(
            items[0],
            Variant::Bool(true),
            "FileAccess.open must RETURN null for a missing file — raising means a \
             script can never test the result, which is how every save system is \
             written"
        );
        assert_eq!(
            items[1],
            Variant::Bool(true),
            "get_open_error() must report the failure after a null open; 0 is OK and \
             would mean the error was never recorded"
        );
    });
}
