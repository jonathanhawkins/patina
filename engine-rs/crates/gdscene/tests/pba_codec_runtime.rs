//! pat-0q2kx.229 — `PackedByteArray`'s binary codec reached from GDScript.
//!
//! Godot ground truth: `core/variant/variant_call.cpp`'s
//! `func_PackedByteArray_*` family over `core/io/marshalls.h`. Everything is
//! little-endian; `decode_*` past the end returns 0 and `encode_*` past the end
//! writes nothing.
//!
//! `call_packed_method` previously dispatched 15 PackedByteArray methods and
//! none of the codec ones, so `b.encode_u32(...)` did not resolve at all.
//!
//! ⚠️ The file is named `pba_codec_runtime` while the acceptance function is
//! `packed_byte_array_codec_runtime`: the coordinator's verifier truncates a
//! reported test command at 58 characters, and `--test
//! packed_byte_array_codec_runtime` overruns that. The acceptance name is a
//! function name, not a binary name.
//!
//! ⚠️ Literals are decimal, not `0x…`: the GDScript tokenizer has no radix
//! literals (no `from_str_radix` anywhere in it), so `0x01020304` would not
//! parse. `16909060` is the same value.

use gdscene::node::{Node, NodeId};
use gdscene::{GDScriptNodeInstance, SceneTree};
use gdvariant::Variant;

/// Every clause is its own function so a failure names the broken codec rather
/// than "the script errored".
const GD: &str = r#"
extends Node

func hex_after_encode_u32():
    var b = PackedByteArray()
    b.resize(4)
    b.encode_u32(0, 16909060)
    return b.hex_encode()

func decode_u32_roundtrip():
    var b = PackedByteArray()
    b.resize(4)
    b.encode_u32(0, 16909060)
    return b.decode_u32(0)

func float_roundtrip():
    var f = PackedByteArray()
    f.resize(4)
    f.encode_float(0, 1.5)
    return f.decode_float(0)

func double_roundtrip():
    var d = PackedByteArray()
    d.resize(8)
    d.encode_double(0, 0.25)
    return d.decode_double(0)

func half_roundtrip():
    var h = PackedByteArray()
    h.resize(2)
    h.encode_half(0, 0.5)
    return h.decode_half(0)

func int32_array_roundtrip():
    return PackedInt32Array([1, 2]).to_byte_array().to_int32_array()

func signed_decode():
    var s = PackedByteArray()
    s.resize(2)
    s.encode_s16(0, -2)
    return s.decode_s16(0)

func unsigned_sees_raw_bits():
    var s = PackedByteArray()
    s.resize(2)
    s.encode_s16(0, -2)
    return s.decode_u16(0)

func decode_past_end_is_zero():
    var b = PackedByteArray()
    b.resize(2)
    return b.decode_u32(0)

func encode_past_end_writes_nothing():
    var b = PackedByteArray()
    b.resize(2)
    b.encode_u32(0, 16909060)
    return b.hex_encode()

func bswap32_reverses():
    var b = PackedByteArray()
    b.resize(4)
    b.encode_u32(0, 16909060)
    b.bswap32(0)
    return b.hex_encode()

func var_roundtrip():
    var b = PackedByteArray()
    b.resize(64)
    var n = b.encode_var(0, 1234)
    if n <= 0:
        return "encode_var failed"
    return b.decode_var(0)
"#;

fn scripted() -> (SceneTree, NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let n = tree.add_child(root, Node::new("Codec", "Node")).unwrap();
    tree.attach_script(
        n,
        Box::new(GDScriptNodeInstance::from_source(GD, n).expect("script parses")),
    );
    (tree, n)
}

fn call(tree: &mut SceneTree, node: NodeId, method: &str) -> Variant {
    tree.call_script_method(node, method, &[])
        .unwrap_or_else(|| panic!("{method} must be callable and must not throw"))
}

/// **The acceptance test.**
#[test]
fn packed_byte_array_codec_runtime() {
    let (mut tree, n) = scripted();

    // The bead's clause 1: 0x01020304 stored little-endian reads back as the
    // bytes 04 03 02 01. A big-endian implementation gives "01020304" here, so
    // this pins byte order rather than just "it wrote something".
    assert_eq!(
        call(&mut tree, n, "hex_after_encode_u32"),
        Variant::String("04030201".into()),
        "encode_u32 must store little-endian"
    );

    // Clause 2.
    assert_eq!(
        call(&mut tree, n, "decode_u32_roundtrip"),
        Variant::Int(16909060),
        "decode_u32 must invert encode_u32"
    );

    // Clause 3.
    match call(&mut tree, n, "float_roundtrip") {
        Variant::Float(f) => assert!((f - 1.5).abs() < 1e-6, "encode/decode_float: got {f}"),
        other => panic!("decode_float should be a Float, got {other:?}"),
    }
    match call(&mut tree, n, "double_roundtrip") {
        Variant::Float(f) => assert!((f - 0.25).abs() < 1e-12, "encode/decode_double: got {f}"),
        other => panic!("decode_double should be a Float, got {other:?}"),
    }
    // 0.5 is exactly representable as an IEEE-754 half, so this is exact — it
    // checks the hand-written binary16 conversion both ways.
    match call(&mut tree, n, "half_roundtrip") {
        Variant::Float(f) => assert!((f - 0.5).abs() < 1e-6, "encode/decode_half: got {f}"),
        other => panic!("decode_half should be a Float, got {other:?}"),
    }

    // Clause 4 — the reinterpret conversions round-trip through bytes.
    assert_eq!(
        call(&mut tree, n, "int32_array_roundtrip"),
        Variant::PackedInt32Array(vec![1, 2]),
        "PackedInt32Array -> to_byte_array -> to_int32_array must be identity"
    );

    // Signedness: the same two bytes read as -2 signed and 65534 unsigned. This
    // is what catches a decode that forgets to sign-extend.
    assert_eq!(
        call(&mut tree, n, "signed_decode"),
        Variant::Int(-2),
        "decode_s16 must sign-extend"
    );
    assert_eq!(
        call(&mut tree, n, "unsigned_sees_raw_bits"),
        Variant::Int(65534),
        "decode_u16 must NOT sign-extend the same bytes"
    );

    // Godot's out-of-range contracts, which are easy to get wrong in both
    // directions: a short read yields 0 rather than erroring...
    assert_eq!(
        call(&mut tree, n, "decode_past_end_is_zero"),
        Variant::Int(0),
        "decode past the end must return 0 (Godot's ERR_FAIL_COND_V default)"
    );
    // ...and a short write is a silent no-op that leaves the buffer untouched.
    assert_eq!(
        call(&mut tree, n, "encode_past_end_writes_nothing"),
        Variant::String("0000".into()),
        "encode past the end must write nothing"
    );

    // bswap32 reverses the 4 bytes in place: 04030201 -> 01020304.
    assert_eq!(
        call(&mut tree, n, "bswap32_reverses"),
        Variant::String("01020304".into()),
        "bswap32 must byte-swap in place"
    );

    // encode_var/decode_var over Godot's binary Variant marshalling.
    assert_eq!(
        call(&mut tree, n, "var_roundtrip"),
        Variant::Int(1234),
        "encode_var then decode_var must round-trip the value"
    );
}
