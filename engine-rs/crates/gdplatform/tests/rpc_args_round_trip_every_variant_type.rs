//! pat-s3rrv.179 acceptance: the binary Variant codec covers the types RPC
//! needs, and an argument it cannot encode fails the call instead of shipping a
//! packet whose `argc` lies.
//!
//! Ground truth: `upstream/godot/core/io/marshalls.cpp`
//! (`encode_variant`/`decode_variant`) and `scene_rpc_interface.cpp`, where a
//! malformed packet fails the call rather than dispatching with wrong values.
//!
//! # What was actually missing
//!
//! Six of the eight types the bead lists were **already implemented** —
//! `Rect2i` (8), `Vector4` (12), `Vector4i` (13), `Projection` (19), `RID` (23)
//! and `PackedVector4Array` (38) all had constants, encode arms and decode arms
//! before this change. Only `Callable` (25) and `Signal` (26) were absent, and
//! they are asserted here alongside the six so a regression in any of them
//! surfaces in one place.
//!
//! The serious half was `encode_rpc_tail`: it wrote `argc` and then appended
//! each argument only `if let Ok(bytes) = encode_variant(a)`. An unencodable
//! argument therefore vanished from the payload **while still being counted**,
//! so the receiver decoded the arguments after it from the wrong offsets.

use gdcore::id::Rid;
use gdcore::math::{Rect2i, Vector2i};
use gdcore::math::{Vector4, Vector4i};
use gdcore::math3d::Projection;
use gdplatform::network::{decode_rpc_packet, encode_rpc_packet, NetworkError};
use gdvariant::binary::{decode_variant, encode_variant};
use gdvariant::Variant;

/// Every value round-trips through the codec unchanged.
fn assert_round_trip(v: Variant, label: &str) {
    let bytes = encode_variant(&v).unwrap_or_else(|e| panic!("{label} must encode: {e}"));
    // ⚠️ `decode_variant` returns `(Variant, bytes_read)`, not a bare Variant.
    let (back, read) =
        decode_variant(&bytes).unwrap_or_else(|e| panic!("{label} must decode: {e}"));
    assert_eq!(back, v, "{label} must survive the round trip unchanged");
    assert_eq!(
        read,
        bytes.len(),
        "{label} must consume exactly what it wrote — a short read means the \
         next value in a stream would start at the wrong offset"
    );
}

/// 🔑 THE ACCEPTANCE: the eight types the RPC wire needs.
#[test]
fn rpc_args_round_trip_every_variant_type() {
    // The six that already worked — asserted so this file is the one place a
    // regression in any of them shows up.
    assert_round_trip(
        Variant::Rect2i(Rect2i::new(Vector2i::new(1, 2), Vector2i::new(3, 4))),
        "Rect2i",
    );
    assert_round_trip(
        Variant::Vector4(Vector4::new(1.0, 2.0, 3.0, 4.0)),
        "Vector4",
    );
    assert_round_trip(Variant::Vector4i(Vector4i::new(5, 6, 7, 8)), "Vector4i");
    assert_round_trip(Variant::Projection(Projection::IDENTITY), "Projection");
    assert_round_trip(Variant::Rid(Rid::from_raw(0xDEAD_BEEF_CAFE)), "RID");
    assert_round_trip(
        Variant::PackedVector4Array(vec![
            Vector4::new(1.0, 2.0, 3.0, 4.0),
            Vector4::new(-1.0, -2.0, -3.0, -4.0),
        ]),
        "PackedVector4Array",
    );

    // The two that were genuinely missing.
    assert_round_trip(
        Variant::Signal {
            object_id: 0x1234_5678_9ABC,
            name: "health_changed".to_string(),
        },
        "Signal",
    );
}

/// ⚠️ A `Callable` deliberately does NOT round-trip to itself.
///
/// Godot encodes it as its header and nothing else (`marshalls.cpp:1817-1818`)
/// and decodes it back as a bare `Callable()` (`:788-790`). A callable is a
/// binding to a live object; there is nothing meaningful to put on a wire.
///
/// This is asserted explicitly so nobody "fixes" the asymmetry later: making a
/// Callable carry its target would diverge from Godot, and making it an encode
/// ERROR would reintroduce the bug this bead is about — an arg list containing
/// one would stop encoding, and `argc` would disagree with the payload.
#[test]
fn a_callable_encodes_as_an_empty_callable() {
    let original = Variant::Callable(Box::new(gdvariant::variant::CallableRef::Method {
        target_id: 42,
        method: "on_hit".to_string(),
    }));

    let bytes = encode_variant(&original).expect("a Callable must encode, not error");
    let (back, _read) = decode_variant(&bytes).expect("and must decode");

    match back {
        Variant::Callable(c) => match *c {
            gdvariant::variant::CallableRef::Method { target_id, method } => {
                assert_eq!(target_id, 0, "the target is not transmitted");
                assert!(method.is_empty(), "nor is the method name");
            }
            other => panic!("expected an empty method binding, got {other:?}"),
        },
        other => panic!("a Callable must decode back as a Callable, got {other:?}"),
    }
}

/// The types travel through the real RPC packet, not just the bare codec.
#[test]
fn the_wire_packet_carries_the_new_types() {
    let args = vec![
        Variant::Vector4(Vector4::new(1.0, 2.0, 3.0, 4.0)),
        Variant::Rect2i(Rect2i::new(Vector2i::new(0, 0), Vector2i::new(16, 9))),
        Variant::PackedVector4Array(vec![Vector4::new(9.0, 8.0, 7.0, 6.0)]),
    ];

    let packet = encode_rpc_packet(0xABCD, "take", &args).expect("packet encodes");
    let (path, method, decoded) = decode_rpc_packet(&packet).expect("packet decodes");

    assert_eq!(path, 0xABCD);
    assert_eq!(method, "take");
    assert_eq!(decoded, args, "every argument arrives as the same Variant");
}

/// 🔑 An unencodable argument fails the call instead of corrupting the packet.
///
/// ⚠️ This is the bug, not a nicety. The old encoder wrote `argc = 2` and then
/// appended only the arguments that happened to encode — so a receiver read
/// argument 1 from the bytes of argument 0's successor and dispatched the method
/// with wrong values. Failing loudly is what Godot does.
#[test]
fn an_unencodable_argument_fails_the_call() {
    // `Resource` is the remaining type with no wire representation.
    let bad = Variant::Resource(Box::new(gdvariant::variant::ResourceRef {
        path: "res://thing.tres".to_string(),
        class_name: "Resource".to_string(),
        properties: Default::default(),
    }));
    let args = vec![Variant::Int(7), bad, Variant::Int(9)];

    let err = encode_rpc_packet(1, "take", &args)
        .expect_err("an unencodable argument must fail the call, not be skipped");

    match err {
        NetworkError::UnencodableArgument { index, .. } => {
            assert_eq!(index, 1, "the error names which argument was refused");
        }
        other => panic!("expected UnencodableArgument, got {other:?}"),
    }
}

/// The control: the same call without the bad argument still encodes, so the
/// test above is failing for the argument and not for the shape of the call.
#[test]
fn the_same_call_without_the_bad_argument_still_encodes() {
    let args = vec![Variant::Int(7), Variant::Int(9)];
    let packet = encode_rpc_packet(1, "take", &args).expect("encodes");
    let (_, _, decoded) = decode_rpc_packet(&packet).expect("decodes");
    assert_eq!(decoded, args);
}
