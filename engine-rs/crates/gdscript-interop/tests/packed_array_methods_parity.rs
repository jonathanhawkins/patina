//! pat-0q2kx.68 — the packed arrays carry Godot's shared method surface, and
//! `PackedByteArray` carries its codecs.
//!
//! `call_packed_method` dispatched about sixteen methods and errored on
//! everything else, so a script could not sort a `PackedInt32Array`, search one,
//! or move a float through a byte buffer — the save-game and networking
//! workhorses. The element-type rules live in `gdvariant`; these tests drive
//! them through a running script so a pass proves the dispatch is wired, not
//! just that the helpers exist.

use gdscript_interop::interpreter::Interpreter;
use gdvariant::Variant;

/// Runs `body` as the statements of a `check()` method and returns its value.
/// One statement per line keeps GDScript's indentation out of the Rust literal.
fn run(body: &[&str]) -> Variant {
    let mut src = String::from("extends Node\nfunc check():\n");
    for line in body {
        src.push_str("    ");
        src.push_str(line);
        src.push('\n');
    }
    let mut interp = Interpreter::new();
    let class_def = interp.run_class(&src).expect("script parses");
    let mut inst = interp
        .instantiate_class(&class_def)
        .expect("class instantiates");
    interp
        .call_instance_method(&mut inst, "check", &[])
        .unwrap_or_else(|e| panic!("check() failed: {e:?}"))
}

fn items(v: Variant) -> Vec<Variant> {
    match v {
        Variant::Array(items) => items,
        other => panic!("expected Array, got {other:?}"),
    }
}

/// The bead's acceptance, in one script: sort, bsearch, an encode/decode round
/// trip through the byte buffer, a var round trip, and a lossless typed
/// reinterpretation.
#[test]
fn packed_array_methods_parity() {
    let got = items(run(&[
        "var ints = PackedInt32Array()",
        "ints.append(30)",
        "ints.append(10)",
        "ints.append(20)",
        "ints.sort()",
        "var bytes = PackedByteArray()",
        "bytes.encode_double(0, 3.5)",
        "var floats = PackedFloat32Array()",
        "floats.append(1.5)",
        "floats.append(-2.25)",
        "var round_tripped = floats.to_byte_array().to_float32_array()",
        "var vars = PackedByteArray()",
        "vars.encode_var(0, {\"hp\": 7})",
        "return [ints.get(0), ints.get(1), ints.get(2), ints.bsearch(20), \
         bytes.decode_double(0), round_tripped.get(0), round_tripped.get(1), \
         vars.decode_var(0)]",
    ]));

    assert_eq!(got.len(), 8, "all eight expressions must evaluate");

    // sort() orders the array in place...
    assert_eq!(
        (got[0].clone(), got[1].clone(), got[2].clone()),
        (Variant::Int(10), Variant::Int(20), Variant::Int(30)),
        "PackedInt32Array.sort() must order the elements"
    );
    // ...and bsearch finds where a value belongs in the sorted result.
    assert_eq!(
        got[3],
        Variant::Int(1),
        "bsearch(20) must report index 1 in [10, 20, 30]"
    );

    // encode_double / decode_double round-trip exactly.
    assert_eq!(
        got[4],
        Variant::Float(3.5),
        "encode_double then decode_double must return the same value"
    );

    // to_byte_array().to_float32_array() is lossless.
    assert_eq!(got[5], Variant::Float(1.5));
    assert_eq!(got[6], Variant::Float(-2.25));

    // encode_var / decode_var carry a whole Dictionary through the buffer.
    match &got[7] {
        Variant::Dictionary(d) => assert_eq!(
            d.get("hp"),
            Some(&Variant::Int(7)),
            "decode_var must return the encoded dictionary"
        ),
        other => panic!("decode_var must return a Dictionary, got {other:?}"),
    }
}

/// The searching family reports Godot's values, including `-1` for a miss.
#[test]
fn find_rfind_count_and_has() {
    let got = items(run(&[
        "var a = PackedInt32Array()",
        "a.append(5)",
        "a.append(7)",
        "a.append(5)",
        "return [a.find(5), a.find(5, 1), a.rfind(5), a.find(99), a.count(5), \
         a.has(7), a.has(99)]",
    ]));

    assert_eq!(got[0], Variant::Int(0), "find returns the first index");
    assert_eq!(got[1], Variant::Int(2), "find honors the from offset");
    assert_eq!(got[2], Variant::Int(2), "rfind returns the last index");
    assert_eq!(got[3], Variant::Int(-1), "a miss is -1, not an error");
    assert_eq!(got[4], Variant::Int(2), "count counts every match");
    assert_eq!(got[5], Variant::Bool(true));
    assert_eq!(got[6], Variant::Bool(false));
}

/// The editing family mutates the array in place through the place-expression
/// write-back, so the script sees its own changes.
#[test]
fn insert_remove_reverse_fill_and_append_array() {
    let got = items(run(&[
        "var a = PackedInt32Array()",
        "a.append(1)",
        "a.append(3)",
        "a.insert(1, 2)",
        "var after_insert = a.size()",
        "var mid = a.get(1)",
        "a.remove_at(0)",
        "var after_remove = a.get(0)",
        "a.reverse()",
        "var reversed_head = a.get(0)",
        "var b = PackedInt32Array()",
        "b.append(9)",
        "a.append_array(b)",
        "var tail = a.get(a.size() - 1)",
        "var f = PackedInt32Array()",
        "f.append(0)",
        "f.append(0)",
        "f.fill(4)",
        "return [after_insert, mid, after_remove, reversed_head, tail, f.get(0), f.get(1)]",
    ]));

    assert_eq!(got[0], Variant::Int(3), "insert grows the array");
    assert_eq!(
        got[1],
        Variant::Int(2),
        "insert places the value at the index"
    );
    assert_eq!(
        got[2],
        Variant::Int(2),
        "remove_at drops the leading element"
    );
    assert_eq!(got[3], Variant::Int(3), "reverse flips [2, 3] to [3, 2]");
    assert_eq!(
        got[4],
        Variant::Int(9),
        "append_array appends the other array"
    );
    assert_eq!(got[5], Variant::Int(4));
    assert_eq!(got[6], Variant::Int(4), "fill overwrites every element");
}

/// The integer codecs round-trip at their declared widths, and a signed decode
/// sees the sign the encode wrote.
#[test]
fn byte_array_integer_codecs_round_trip() {
    let got = items(run(&[
        "var b = PackedByteArray()",
        "b.encode_u8(0, 200)",
        "b.encode_s16(1, -300)",
        "b.encode_u32(3, 70000)",
        "b.encode_s64(7, -5)",
        "b.encode_float(15, 0.5)",
        "return [b.decode_u8(0), b.decode_s16(1), b.decode_u32(3), b.decode_s64(7), \
         b.decode_float(15), b.size()]",
    ]));

    assert_eq!(got[0], Variant::Int(200), "u8");
    assert_eq!(got[1], Variant::Int(-300), "s16 keeps the sign");
    assert_eq!(got[2], Variant::Int(70000), "u32");
    assert_eq!(got[3], Variant::Int(-5), "s64 keeps the sign");
    assert_eq!(got[4], Variant::Float(0.5), "f32 is exact for 0.5");
    assert_eq!(
        got[5],
        Variant::Int(19),
        "encoding past the end grows the buffer to fit"
    );
}

/// Sorting is not integer-only: strings and floats order too.
#[test]
fn sort_orders_strings_and_floats() {
    let got = items(run(&[
        "var s = PackedStringArray()",
        "s.append(\"pear\")",
        "s.append(\"apple\")",
        "s.sort()",
        "var f = PackedFloat64Array()",
        "f.append(2.5)",
        "f.append(-1.0)",
        "f.sort()",
        "return [s.get(0), s.get(1), f.get(0), f.get(1)]",
    ]));

    assert_eq!(got[0], Variant::String("apple".into()));
    assert_eq!(got[1], Variant::String("pear".into()));
    assert_eq!(got[2], Variant::Float(-1.0));
    assert_eq!(got[3], Variant::Float(2.5));
}
