//! pat-nzgdu.189 — an object encoded as a bare instance id round-trips through
//! the Variant byte codec.
//!
//! Godot builds an `EncodedObjectAsID` on decode; this crate's
//! `Variant::ObjectId` carries the same single fact — the id — and gdvariant
//! cannot construct engine objects (gdobject depends on gdvariant, not the
//! reverse). The ClassDB half is gdobject's
//! `encoded_object_as_id_registration`. See the bead notes.
//!
//! Wire format verified against Godot's own `core/io/marshalls.cpp`, vendored at
//! `upstream/godot/`.

use gdcore::id::ObjectId;
use gdvariant::binary::{
    decode_variant, encode_variant, VariantDecodeError, HEADER_DATA_FLAG_OBJECT_AS_ID,
};
use gdvariant::Variant;

/// Godot's `Variant::OBJECT` ordinal.
const T_OBJECT: u32 = 24;

fn header_of(bytes: &[u8]) -> u32 {
    u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
}

/// The acceptance: encode an object as an id, decode it, get the id back.
#[test]
fn encoded_object_as_id_roundtrip() {
    let id = ObjectId::from_raw(0x1234_5678_9abc);
    let bytes = encode_variant(&Variant::ObjectId(id)).expect("an object encodes as its id");

    // 4-byte header + 8-byte id. Godot reads the id with decode_uint64 and
    // ERR_FAILs below 8 bytes (marshalls.cpp:701-702).
    assert_eq!(bytes.len(), 12, "header + u64 id, got {bytes:?}");

    let header = header_of(&bytes);
    assert_eq!(
        header & 0xFF,
        T_OBJECT,
        "the type ordinal must be OBJECT(24)"
    );
    assert!(
        header & HEADER_DATA_FLAG_OBJECT_AS_ID != 0,
        "the OBJECT_AS_ID flag must be set, header = {header:#x}"
    );

    let (decoded, used) = decode_variant(&bytes).expect("an object-as-id decodes");
    assert_eq!(used, 12, "the whole buffer is consumed");
    assert_eq!(
        decoded,
        Variant::ObjectId(id),
        "the id must survive the round trip"
    );
}

/// ⚠️ A null object is NIL on BOTH sides, and never carries an OBJECT header.
///
/// Encode decides this before it ever reaches the as-id flag — the `if (!obj)`
/// early return at `marshalls.cpp:1385-1393` writes a bare 4-byte NIL. Decode
/// agrees from the other direction: a null id yields `nullptr`, not an object
/// with id 0 (`:707-708`). So `object_id == 0` is unreachable through the wire
/// in either direction, even though `EncodedObjectAsID`'s property defaults to 0.
///
/// Note the deliberate asymmetry: `ObjectId(0)` does **not** round-trip.
#[test]
fn null_id_is_nil_on_both_sides() {
    let bytes = encode_variant(&Variant::ObjectId(ObjectId::from_raw(0))).expect("encodes");
    assert_eq!(
        bytes,
        vec![0, 0, 0, 0],
        "a null object is a bare 4-byte NIL — Godot never writes an OBJECT \
         header for it, got {bytes:?}"
    );

    let (decoded, used) = decode_variant(&bytes).expect("decodes");
    assert_eq!(used, 4, "only the NIL header is consumed");
    assert_eq!(decoded, Variant::Nil, "a null object id is NIL");
}

/// The decoder must also handle an OBJECT|AS_ID header carrying a zero id — a
/// buffer this encoder never writes, but a hostile or foreign one might.
#[test]
fn zero_id_in_an_object_header_decodes_to_nil() {
    let mut bytes = ((24u32) | (1 << 16)).to_le_bytes().to_vec();
    bytes.extend_from_slice(&0u64.to_le_bytes());
    let (decoded, used) = decode_variant(&bytes).expect("decodes");
    assert_eq!(used, 12, "the id is still consumed");
    assert_eq!(
        decoded,
        Variant::Nil,
        "a null id is NIL, not an object with id 0 (marshalls.cpp:707-708)"
    );
}

/// ⚠️ The id is a full 64-bit value. Truncating it to 32 bits would silently
/// alias distinct objects, which is exactly the bug a round-trip test exists to
/// catch.
#[test]
fn full_64_bit_ids_survive() {
    for raw in [1u64, u32::MAX as u64, u32::MAX as u64 + 1, u64::MAX] {
        let id = ObjectId::from_raw(raw);
        let bytes = encode_variant(&Variant::ObjectId(id)).expect("encodes");
        let (decoded, _) = decode_variant(&bytes).expect("decodes");
        assert_eq!(
            decoded,
            Variant::ObjectId(id),
            "id {raw:#x} must survive intact"
        );
    }
}

/// ⚠️ A *full* object — class name plus properties — is refused. Decoding one
/// would mean instantiating an arbitrary class named by untrusted bytes, which
/// is why Godot gates it behind `p_allow_objects` and errors ERR_UNAUTHORIZED
/// when it is off (`marshalls.cpp:717`). This codec has no allow-objects mode.
#[test]
fn full_object_encoding_is_refused() {
    // OBJECT(24) with NO as-id flag = the full-object form.
    let err = decode_variant(&[24, 0, 0, 0]).unwrap_err();
    assert_eq!(
        err,
        VariantDecodeError::UnsupportedType(24),
        "a full object must be refused, got {err:?}"
    );
}

/// A truncated id must be a clean error, not a panic or a half-read id.
#[test]
fn truncated_id_payload_errors() {
    let mut bytes = encode_variant(&Variant::ObjectId(ObjectId::from_raw(9))).unwrap();
    bytes.truncate(8); // header + only 4 of the 8 id bytes
    assert!(
        matches!(
            decode_variant(&bytes),
            Err(VariantDecodeError::UnexpectedEof { .. })
        ),
        "a short id payload must error cleanly"
    );
}

/// Objects-as-ids nest inside containers like any other variant — RPC payloads
/// are usually an array of arguments, not a bare object.
#[test]
fn object_ids_round_trip_inside_containers() {
    let inner = vec![
        Variant::Int(1),
        Variant::ObjectId(ObjectId::from_raw(42)),
        Variant::String("x".to_string()),
    ];
    let bytes = encode_variant(&Variant::Array(inner.clone())).expect("array encodes");
    let (decoded, _) = decode_variant(&bytes).expect("array decodes");
    assert_eq!(decoded, Variant::Array(inner), "the id survives nesting");
}
