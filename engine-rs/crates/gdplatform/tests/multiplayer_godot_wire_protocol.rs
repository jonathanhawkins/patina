//! pat-0q2kx.95 — the multiplayer wire protocol is Godot's command-byte framing
//! carrying real binary Variants, with a negotiated path-id cache.
//!
//! The three clauses of the acceptance, in order:
//!   1. structured args (Vector2 / Array / Dictionary) round-trip as *Variants*,
//!      not as strings;
//!   2. a second call to the same node stops paying for the full path;
//!   3. a RAW packet never enters RPC dispatch.

use gdplatform::network::{
    decode_confirm_path, decode_raw_packet, decode_rpc_packet, decode_rpc_packet_cached,
    decode_simplify_path, encode_confirm_path, encode_raw_packet, encode_rpc_call, NetworkCommand,
    ObjectDecoding, RpcAddressing, RpcPathCache,
};
use gdvariant::Variant;

const NODE: u64 = 0xDEAD_BEEF;

fn structured_args() -> Vec<Variant> {
    vec![
        Variant::Vector2(gdcore::math::Vector2::new(1.5, -2.25)),
        Variant::Array(vec![Variant::Int(7), Variant::Bool(true)]),
        Variant::Dictionary(
            vec![(Variant::String("hp".into()), Variant::Int(42))]
                .into_iter()
                .collect(),
        ),
    ]
}

/// **Clause 1.** Structured args survive the wire as themselves.
///
/// The old private codec had a fallback arm that `format!`-ed anything it did
/// not understand, so a `Vector2` arrived as the *string* `"(1.5, -2.25)"` and
/// the receiver ran the method with a wrong-typed value instead of failing. The
/// explicit "no arg is a String" check is what catches a regression to that
/// behaviour — an `assert_eq` on the whole vector would too, but this says why.
#[test]
fn structured_rpc_args_round_trip_as_variants() {
    let args = structured_args();
    let mut cache = RpcPathCache::new();
    let (_simplify, call) = encode_rpc_call(&mut cache, NODE, "take_damage", &args).unwrap();

    let (path, method, decoded) = decode_rpc_packet(&call).expect("first call decodes");
    assert_eq!(path, NODE);
    assert_eq!(method, "take_damage");
    assert_eq!(
        decoded, args,
        "every arg must round-trip as the same Variant"
    );
    for arg in &decoded {
        assert!(
            !matches!(arg, Variant::String(_)),
            "no structured arg may degrade to a String: {arg:?}"
        );
    }
}

/// **Clause 2.** Once the peer confirms the id, the call stops carrying the
/// path — asserted on the wire bytes, not on a helper's return value.
///
/// ⚠️ The switch happens on **confirmation**, not on the second send. Addressing
/// by an id the receiver has not acknowledged would be dropped at the far end,
/// so a test that merely called twice without the handshake would "pass" while
/// describing a protocol that loses calls.
#[test]
fn a_confirmed_path_id_replaces_the_full_path_on_the_wire() {
    let mut sender = RpcPathCache::new();
    let mut receiver = RpcPathCache::new();

    // --- first call: announces the id, still addresses by full path ---------
    let (simplify, first) = encode_rpc_call(&mut sender, NODE, "ping", &[]).unwrap();
    let simplify = simplify.expect("the first call to a node must announce its id");
    assert_eq!(simplify[0], NetworkCommand::SimplifyPath.to_byte());
    assert_eq!(first[1], RpcAddressing::FullPath.to_byte());

    let (id, announced) = decode_simplify_path(&simplify).expect("simplify decodes");
    assert_eq!(announced, NODE);
    receiver.learn(id, announced);

    // --- receiver confirms --------------------------------------------------
    let confirm = encode_confirm_path(id);
    assert_eq!(confirm[0], NetworkCommand::ConfirmPath.to_byte());
    sender.confirm(decode_confirm_path(&confirm).expect("confirm decodes"));
    assert!(sender.is_confirmed(NODE));

    // --- second call: short form, and it still dispatches --------------------
    let (again, second) = encode_rpc_call(&mut sender, NODE, "ping", &[]).unwrap();
    assert!(again.is_none(), "the id is announced once, not every call");
    assert_eq!(second[1], RpcAddressing::CachedId.to_byte());
    assert!(
        second.len() < first.len(),
        "the id form must be shorter than the path form: {} vs {}",
        second.len(),
        first.len()
    );

    let (path, method, _) = decode_rpc_packet_cached(&second, &receiver, ObjectDecoding::Deny)
        .expect("an id-addressed call still dispatches");
    assert_eq!(path, NODE, "the id must resolve back to the same node");
    assert_eq!(method, "ping");
}

/// An id the receiver never learned must drop the call, not resolve to whatever
/// node happens to collide — and a cache-less decoder cannot resolve one at all.
#[test]
fn an_unknown_path_id_is_dropped_rather_than_guessed() {
    let mut sender = RpcPathCache::new();
    let (_, _) = encode_rpc_call(&mut sender, NODE, "ping", &[]).unwrap();
    let id = sender.id_for(NODE).expect("id assigned");
    sender.confirm(id);
    let (_, call) = encode_rpc_call(&mut sender, NODE, "ping", &[]).unwrap();
    assert_eq!(call[1], RpcAddressing::CachedId.to_byte());

    let empty = RpcPathCache::new();
    assert!(
        decode_rpc_packet_cached(&call, &empty, ObjectDecoding::Deny).is_none(),
        "an unlearned id must not resolve"
    );
    assert!(
        decode_rpc_packet(&call).is_none(),
        "the cache-less decoder cannot resolve an id and must refuse it"
    );
}

/// **Clause 3.** A RAW packet must never enter RPC dispatch, and vice versa.
#[test]
fn raw_packets_never_enter_rpc_dispatch() {
    let raw = encode_raw_packet(b"hello");
    assert_eq!(raw[0], NetworkCommand::Raw.to_byte());
    assert!(decode_rpc_packet(&raw).is_none());
    assert!(decode_rpc_packet_cached(&raw, &RpcPathCache::new(), ObjectDecoding::Deny).is_none());
    assert_eq!(decode_raw_packet(&raw), Some(b"hello".to_vec()));

    let mut cache = RpcPathCache::new();
    let (_, call) = encode_rpc_call(&mut cache, NODE, "ping", &[]).unwrap();
    assert!(
        decode_raw_packet(&call).is_none(),
        "an RPC must not be consumed as a raw payload"
    );
}

/// The whole acceptance in one place, so the bead has a single named test.
#[test]
fn multiplayer_godot_wire_protocol() {
    structured_rpc_args_round_trip_as_variants();
    a_confirmed_path_id_replaces_the_full_path_on_the_wire();
    raw_packets_never_enter_rpc_dispatch();
}
