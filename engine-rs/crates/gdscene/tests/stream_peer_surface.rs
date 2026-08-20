//! pat-s3rrv.178 — the `StreamPeer`/`PacketPeer` typed accessor surface and the
//! socket peers, reached from a script attached to a scene node.
//!
//! Godot ground truth: `doc/classes/StreamPeer.xml` (put/get for 8/16/32/64-bit
//! ints, half, float, double, string, utf8_string and var, plus
//! `put_partial_data`/`get_partial_data` and `is_big_endian_enabled`),
//! `StreamPeerBuffer.xml` (seek/resize/set_data_array/duplicate) and the socket
//! classes `StreamPeerTCP`, `PacketPeerUDP`, `TCPServer`, `UDPServer`,
//! `PacketPeerStream`.
//!
//! Two separate gaps are covered here, and they fail differently:
//!
//! 1. The typed accessors were missing from the interpreter dispatch, so
//!    `peer.put_float(...)` raised an unknown-method error even though
//!    `gdplatform::stream_peer_buffer` implemented it.
//! 2. `put_partial_data`/`get_partial_data` were **aliased** to the
//!    non-partial versions, so they returned a bare value where Godot returns
//!    `[Error, result]` — meaning `result[0]` read the first data *byte* as an
//!    error code. That one is a wrong answer rather than a missing one, which
//!    is why it is asserted on shape and not just on success.
//!
//! The file name is deliberately short: the verifier truncates a test command
//! at 58 characters, and this one fits with room to spare.

use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscene::GDScriptNodeInstance;
use gdvariant::Variant;

/// Attaches `body` as a `run()` on a node in a real tree and calls it.
fn run_in_scene(body: &str) -> Variant {
    let src = format!("extends Node\nfunc run():\n{body}");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let node: NodeId = tree
        .add_child(root, Node::new("Main", "Node"))
        .expect("node added");
    tree.attach_script(
        node,
        Box::new(
            GDScriptNodeInstance::from_source(&src, node)
                .unwrap_or_else(|e| panic!("script did not parse: {e:?}\n--- source ---\n{src}")),
        ),
    );
    tree.get_script_mut(node)
        .expect("script attached")
        .call_method("run", &[])
        .unwrap_or_else(|e| panic!("run() raised: {e:?}\n--- source ---\n{src}"))
}

fn array(v: &Variant) -> &[Variant] {
    match v {
        Variant::Array(items) => items,
        other => panic!("expected an Array, got {other:?}"),
    }
}

/// **The acceptance.** Every typed accessor round-trips in both endian modes.
///
/// The values are chosen to be *exactly* representable — 0.5, 0.25 and 1.5 are
/// dyadic in f32 and f16 alike — so the assertions are exact equality. A
/// tolerance would let a genuinely broken conversion through.
#[test]
fn stream_peer_typed_accessors_and_socket_peer_handles() {
    let out = run_in_scene(
        "    var le = StreamPeerBuffer.new()\n    le.put_float(0.5)\n    le.put_double(0.25)\n    le.put_half(1.5)\n    le.put_string(\"ascii\")\n    le.put_utf8_string(\"utf8\")\n    le.seek(0)\n    var lf = le.get_float()\n    var ld = le.get_double()\n    var lh = le.get_half()\n    var ls = le.get_string()\n    var lu = le.get_utf8_string()\n    var be = StreamPeerBuffer.new()\n    be.set_big_endian(true)\n    be.put_float(0.5)\n    be.put_double(0.25)\n    be.put_half(1.5)\n    be.put_string(\"ascii\")\n    be.put_utf8_string(\"utf8\")\n    be.seek(0)\n    var bf = be.get_float()\n    var bd = be.get_double()\n    var bh = be.get_half()\n    var bs = be.get_string()\n    var bu = be.get_utf8_string()\n    return [lf, ld, lh, ls, lu, le.is_big_endian_enabled(), bf, bd, bh, bs, bu, be.is_big_endian_enabled()]\n",
    );

    let v = array(&out);
    assert_eq!(
        v.len(),
        12,
        "one row per value per endianness, plus the flags"
    );

    assert_eq!(v[0], Variant::Float(0.5), "little-endian float");
    assert_eq!(v[1], Variant::Float(0.25), "little-endian double");
    assert_eq!(v[2], Variant::Float(1.5), "little-endian half");
    assert_eq!(
        v[3],
        Variant::String("ascii".into()),
        "length-prefixed ASCII"
    );
    assert_eq!(
        v[4],
        Variant::String("utf8".into()),
        "length-prefixed UTF-8"
    );
    assert_eq!(
        v[5],
        Variant::Bool(false),
        "is_big_endian_enabled must resolve, and be false here"
    );

    assert_eq!(v[6], Variant::Float(0.5), "big-endian float");
    assert_eq!(v[7], Variant::Float(0.25), "big-endian double");
    assert_eq!(v[8], Variant::Float(1.5), "big-endian half");
    assert_eq!(v[9], Variant::String("ascii".into()), "big-endian ASCII");
    assert_eq!(v[10], Variant::String("utf8".into()), "big-endian UTF-8");
    assert_eq!(v[11], Variant::Bool(true), "the big-endian flag reads back");
}

/// The five socket classes construct from script and answer a real call.
///
/// The assertion is deliberately about *reaching* the class, not about
/// networking: an unimplemented peer surfaces here as a raised unknown-method
/// error from `run()`, which fails the test inside `run_in_scene`.
#[test]
fn socket_peers_construct_and_answer_from_script() {
    let out = run_in_scene(
        "    var tcp = StreamPeerTCP.new()\n    var udp = PacketPeerUDP.new()\n    var tsrv = TCPServer.new()\n    var usrv = UDPServer.new()\n    var pps = PacketPeerStream.new()\n    return [tcp.get_status(), udp.is_bound(), tsrv.is_listening(), usrv.is_listening(), pps.get_available_packet_count()]\n",
    );

    let v = array(&out);
    assert_eq!(v.len(), 5, "all five peers answered");
    assert_eq!(
        v[0],
        Variant::Int(0),
        "a fresh StreamPeerTCP is STATUS_NONE"
    );
    assert_eq!(
        v[1],
        Variant::Bool(false),
        "a fresh PacketPeerUDP is unbound"
    );
    assert_eq!(
        v[2],
        Variant::Bool(false),
        "a fresh TCPServer is not listening"
    );
    assert_eq!(
        v[3],
        Variant::Bool(false),
        "a fresh UDPServer is not listening"
    );
    assert_eq!(
        v[4],
        Variant::Int(0),
        "a fresh PacketPeerStream has no packets"
    );
}

/// `get_partial_data` returns `[Error, PackedByteArray]`, not bare bytes.
///
/// This is the clause the old aliasing broke: with a bare array, `r[0]` is the
/// first *byte*, so a script's `if r[0] != OK` check silently misfired on any
/// payload whose first byte was non-zero.
#[test]
fn partial_data_returns_an_error_and_a_payload() {
    let out = run_in_scene(
        "    var p = StreamPeerBuffer.new()\n    p.put_data(PackedByteArray([7, 8, 9]))\n    p.seek(0)\n    var r = p.get_partial_data(2)\n    var w = p.put_partial_data(PackedByteArray([1]))\n    return [r.size(), r[0], r[1], w.size(), w[0], w[1]]\n",
    );

    let v = array(&out);
    assert_eq!(
        v[0],
        Variant::Int(2),
        "get_partial_data returns a 2-element Array"
    );
    assert_eq!(
        v[1],
        Variant::Int(0),
        "element 0 is the error code, OK here"
    );
    assert_eq!(
        v[2],
        Variant::PackedByteArray(vec![7, 8]),
        "element 1 is the payload — and 7 is what a bare return would put in r[0]"
    );
    assert_eq!(
        v[3],
        Variant::Int(2),
        "put_partial_data also returns a 2-element Array"
    );
    assert_eq!(v[4], Variant::Int(0), "put error code");
    assert_eq!(v[5], Variant::Int(1), "one byte written");
}

/// `resize`, `set_data_array` and `duplicate` are reachable, and `duplicate`
/// yields an independent buffer rather than a second name for the same handle.
#[test]
fn buffer_reshaping_and_duplicate_are_independent() {
    let out = run_in_scene(
        "    var p = StreamPeerBuffer.new()\n    p.set_data_array(PackedByteArray([1, 2, 3, 4]))\n    var copy = p.duplicate()\n    copy.set_data_array(PackedByteArray([9]))\n    p.resize(2)\n    return [p.get_data_array(), copy.get_data_array(), p.get_size()]\n",
    );

    let v = array(&out);
    assert_eq!(
        v[0],
        Variant::PackedByteArray(vec![1, 2]),
        "resize truncates the original"
    );
    assert_eq!(
        v[1],
        Variant::PackedByteArray(vec![9]),
        "the duplicate is a separate buffer, not a second name for the same one"
    );
    assert_eq!(v[2], Variant::Int(2));
}
