//! pat-s3rrv.80 — `PacketPeerStream` is reachable from GDScript.
//!
//! ⚠️ THE IMPLEMENTATION WAS NEVER THE GAP. `gdplatform/src/packet_peer_stream.rs`
//! is 162 lines that frame packets with Godot's 4-byte little-endian length
//! prefix, with its own passing unit tests. What was missing was every route TO
//! it: the class had no ClassDB registration (`grep -c
//! 'ClassRegistration::new("PacketPeerStream")'` = 0) and no handle path in the
//! interpreter (`grep -c PacketPeerStream interpreter.rs` = 0), so
//! `PacketPeerStream.new()` raised and no script could touch a working class.
//!
//! That is the worst shape a gap takes — the code exists, the tests pass, and
//! the feature is nonetheless absent from the engine's point of view.

use gdscript_interop::interpreter::Interpreter;
use gdvariant::Variant;

fn run_val(src: &str) -> Variant {
    let mut interp = Interpreter::new();
    interp
        .run(src)
        .unwrap_or_else(|e| panic!("script failed: {e:?}\n--- source ---\n{src}"))
        .return_value
        .expect("script returned a value")
}

/// 🔑 The class can be CONSTRUCTED at all — this is what used to raise.
#[test]
fn packet_peer_stream_can_be_constructed_from_script() {
    // Returning the count rather than the object: a handle's own `to_string`
    // is not the thing under test, and an empty peer must answer 0 rather than
    // raise.
    assert_eq!(
        run_val("var p = PacketPeerStream.new()\nreturn p.get_available_packet_count()\n"),
        Variant::Int(0),
        "PacketPeerStream.new() must construct and answer 0 packets. A raise \
         here is the old behaviour: no ClassDB entry and no handle path"
    );
}

/// A packet put in is a packet read back, with its bytes intact.
///
/// ⚠️ This works on ONE object because the implementation keeps a single
/// `buffer` that `put_packet` appends to and `get_packet` consumes from — it is
/// deliberately not Godot's split input/output stream pair. Asserting the
/// round-trip on one peer pins the behaviour that actually exists rather than
/// implying a two-stream model the code does not have.
#[test]
fn a_packet_survives_the_round_trip_through_script() {
    assert_eq!(
        run_val(
            "var p = PacketPeerStream.new()\n\
             p.put_packet(PackedByteArray([1, 2, 3]))\n\
             return p.get_available_packet_count()\n"
        ),
        Variant::Int(1),
        "one framed packet must be available after put_packet"
    );

    assert_eq!(
        run_val(
            "var p = PacketPeerStream.new()\n\
             p.put_packet(PackedByteArray([1, 2, 3]))\n\
             return p.get_packet()\n"
        ),
        Variant::PackedByteArray(vec![1, 2, 3]),
        "the bytes must survive the 4-byte length framing intact"
    );
}

/// ⚠️ An empty peer returns an EMPTY array, not an error and not Nil.
///
/// Godot's `PacketPeer.get_packet` answers with an empty `PackedByteArray` when
/// nothing is queued; a script draining with
/// `while get_available_packet_count() > 0` must never have to tell Nil from
/// empty, and a raise here would break the idiom outright.
#[test]
fn draining_an_empty_peer_yields_an_empty_array() {
    assert_eq!(
        run_val("var p = PacketPeerStream.new()\nreturn p.get_packet()\n"),
        Variant::PackedByteArray(Vec::new()),
        "an empty peer must answer with an empty PackedByteArray"
    );
}

/// Two peers are independent — the handle table keys on the id, so one script
/// holding several must not have them share a buffer.
#[test]
fn two_peers_do_not_share_a_buffer() {
    assert_eq!(
        run_val(
            "var a = PacketPeerStream.new()\n\
             var b = PacketPeerStream.new()\n\
             a.put_packet(PackedByteArray([9]))\n\
             return b.get_available_packet_count()\n"
        ),
        Variant::Int(0),
        "a packet put on peer A must not appear on peer B — if it does, the \
         handle table is keyed wrongly and every peer aliases one buffer"
    );
}
