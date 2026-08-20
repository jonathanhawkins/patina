//! pat-s3rrv.195 — AES-CBC block semantics and the `generate_random_bytes`
//! CSPRNG, as an INTEGRATION test.
//!
//! # Why these live here and not in `crypto.rs`
//!
//! They were written inside `gdcore`'s `#[cfg(test)] mod tests`, which is
//! reachable only through `-p gdcore --lib`. That target has been red on a
//! FOREIGN failure — `math::tests::color_64bit_forms_and_hex_constructors` —
//! so these eight tests could not be named as a gate on any bead without
//! importing an unrelated crate-wide failure into the verdict. They passed;
//! they simply could not be *cited*.
//!
//! Nothing here needs crate-private access: `AesContext`, `AesMode`,
//! `generate_random_bytes` and `rng::seed` are all public, so moving them out
//! costs no coverage and makes them gateable on their own.
//!
//! ⚠️ Moved verbatim. The assertions and their comments are the original
//! author's; only the `use` paths changed (`crate::rng` -> `gdcore::rng`).

use gdcore::crypto::{generate_random_bytes, AesContext, AesMode};

// ---- pat-s3rrv.195: AESContext matches Godot's block semantics ---------

const K256: &[u8] = b"0123456789abcdef0123456789abcdef";
const IV0: &[u8] = b"iviviviviviviviv";

/// **The chaining bug, stated as a test.** A message split across two
/// `update()` calls must encrypt identically to the same message passed in
/// one call.
///
/// The old code rebuilt the cipher from the ORIGINAL iv every call, so the
/// second chunk was encrypted as though it were the first — this assertion
/// failed on the second 16 bytes while the first 16 matched, which is
/// exactly why the bug survived a round-trip test.
#[test]
fn cbc_update_chains_the_iv_across_calls() {
    let msg = b"sixteen byte msgsixteen byte two";

    let mut one_shot = AesContext::new();
    one_shot.start(AesMode::Encrypt, K256, IV0).unwrap();
    let whole = one_shot.update(msg).unwrap();

    let mut split = AesContext::new();
    split.start(AesMode::Encrypt, K256, IV0).unwrap();
    let mut chunked = split.update(&msg[..16]).unwrap();
    chunked.extend_from_slice(&split.update(&msg[16..]).unwrap());

    assert_eq!(
        whole, chunked,
        "splitting a message across update() calls must not change the ciphertext"
    );
}

/// Decrypt chains from the INPUT block, not the output. Getting this
/// backwards corrupts every block after the first while leaving the first
/// one correct — a failure that looks like a padding bug.
#[test]
fn cbc_round_trips_across_chained_updates() {
    let msg = b"sixteen byte msgsixteen byte two";

    let mut enc = AesContext::new();
    enc.start(AesMode::Encrypt, K256, IV0).unwrap();
    let ct = enc.update(msg).unwrap();

    let mut dec = AesContext::new();
    dec.start(AesMode::Decrypt, K256, IV0).unwrap();
    let mut out = dec.update(&ct[..16]).unwrap();
    out.extend_from_slice(&dec.update(&ct[16..]).unwrap());

    assert_eq!(out, msg, "chunked decrypt must reproduce the plaintext");
}

/// Godot's `update()` adds NO padding: 16 bytes in, 16 bytes out.
#[test]
fn update_adds_no_padding() {
    let mut ctx = AesContext::new();
    ctx.start(AesMode::Encrypt, K256, IV0).unwrap();
    let out = ctx.update(b"sixteen byte msg").unwrap();
    assert_eq!(
        out.len(),
        16,
        "PKCS7 would have produced 32 bytes; Godot returns the raw block"
    );
}

/// And it refuses a partial block rather than padding it silently.
#[test]
fn update_rejects_a_partial_block() {
    let mut ctx = AesContext::new();
    ctx.start(AesMode::Encrypt, K256, IV0).unwrap();
    assert!(
        ctx.update(b"not sixteen").is_err(),
        "a non-multiple-of-16 input must error, not be padded"
    );
}

/// `get_iv_state` reports the chained IV, not the one `start` was given.
#[test]
fn get_iv_state_advances_with_the_stream() {
    let mut ctx = AesContext::new();
    ctx.start(AesMode::Encrypt, K256, IV0).unwrap();
    assert_eq!(ctx.get_iv_state(), IV0, "before any block, the starting IV");

    let ct = ctx.update(b"sixteen byte msg").unwrap();
    assert_eq!(
        ctx.get_iv_state(),
        ct,
        "after one block the IV is that ciphertext block"
    );
    assert_ne!(
        ctx.get_iv_state(),
        IV0,
        "an unchanged IV would mean the stream never advanced"
    );
}

// ---- pat-s3rrv.195: generate_random_bytes is a CSPRNG ------------------

/// **The vulnerability this replaces, stated as a test.**
///
/// The old body drew from `crate::rng`, a SystemTime-seeded xorshift. Two
/// runs that reached the same seed produced the SAME "random" bytes — so a
/// key, nonce or session id was recoverable by anyone who could guess the
/// process start time closely enough.
///
/// Reseeding the xorshift identically and asking twice therefore returns
/// identical output under the old implementation and differing output
/// under a real CSPRNG. A plain "two calls differ" assertion would NOT
/// catch the regression: the xorshift advances, so consecutive draws
/// differ there too.
#[test]
fn generate_random_bytes_does_not_come_from_the_seeded_xorshift() {
    gdcore::rng::seed(0xDEAD_BEEF);
    let first = generate_random_bytes(32);
    gdcore::rng::seed(0xDEAD_BEEF);
    let second = generate_random_bytes(32);

    assert_ne!(
        first, second,
        "resetting the xorshift seed must not reproduce the byte stream — \
         if it does, key material is predictable from the process start time"
    );
}

#[test]
fn generate_random_bytes_respects_the_requested_length() {
    for len in [0usize, 1, 16, 32, 129] {
        assert_eq!(generate_random_bytes(len).len(), len);
    }
}

/// Not a randomness-quality test — just a smoke check that the source is
/// not returning a constant buffer.
#[test]
fn generate_random_bytes_is_not_all_zeroes() {
    let bytes = generate_random_bytes(64);
    assert!(
        bytes.iter().any(|&b| b != 0),
        "64 zero bytes from the CSPRNG would mean the fill never happened"
    );
}
