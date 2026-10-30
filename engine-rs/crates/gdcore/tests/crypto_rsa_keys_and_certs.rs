//! pat-s3rrv.195: the public half of `gdcore::crypto_rsa` — RSA keys, PKCS#1
//! v1.5 signatures, PKCS#1 v1.5 encryption and self-signed X.509.
//!
//! Godot ground truth: `core/crypto/crypto.cpp` plus
//! `doc/classes/{Crypto,CryptoKey,X509Certificate}.xml`, and mbedTLS for the
//! PEM labels (`mbedtls_pk_write_key_pem` emits `RSA PRIVATE KEY`,
//! `mbedtls_pk_write_pubkey_pem` emits `PUBLIC KEY`).
//!
//! ⚠️ 1024-BIT KEYS THROUGHOUT. Key generation is a prime search and the cost
//! is superlinear in the modulus width; nothing asserted here depends on the
//! size, and the 2048-bit case the bead names is covered once, from a script,
//! by `gdscene`'s `crypto_rsa_x509_and_csprng_reachable_from_script`.
//!
//! These live in `tests/` rather than beside the code because a `#[cfg(test)]`
//! module inside the lib can only be run with a `--lib` filter, which is not a
//! gate the verifier lane can point at a named file.

use gdcore::crypto::sha256;
use gdcore::crypto_rsa::{CryptoKey, X509Certificate};

const BITS: usize = 1024;

/// Sign, verify, and REFUSE: the tampered-digest clause is what separates a
/// working verify from one that ignores its input.
#[test]
fn a_signature_verifies_and_a_tampered_digest_does_not() {
    let key = CryptoKey::generate(BITS).unwrap();
    assert!(!key.is_public_only());
    assert_eq!(key.size_bytes(), BITS / 8);

    let digest = sha256(b"patina parity");
    let sig = key.sign_sha256(&digest).unwrap();
    assert_eq!(
        sig.len(),
        key.size_bytes(),
        "a PKCS#1 v1.5 signature is exactly the modulus width"
    );
    assert!(key.verify_sha256(&digest, &sig));

    let mut tampered = digest;
    tampered[0] ^= 0xFF;
    assert!(
        !key.verify_sha256(&tampered, &sig),
        "a different digest must not verify"
    );

    let mut bad_sig = sig.clone();
    bad_sig[0] ^= 0xFF;
    assert!(
        !key.verify_sha256(&digest, &bad_sig),
        "a tampered signature must not verify"
    );

    // A digest of the wrong length cannot be a SHA-256 digest, and signing it
    // under the SHA-256 DigestInfo prefix would produce a signature that no
    // other implementation accepts. Refuse instead.
    assert!(key.sign_sha256(b"too short").is_err());
    assert!(!key.verify_sha256(b"too short", &sig));
}

#[test]
fn a_private_key_round_trips_through_its_own_pem() {
    let key = CryptoKey::generate(BITS).unwrap();
    let digest = sha256(b"round trip");
    let sig = key.sign_sha256(&digest).unwrap();

    let pem = key.save_to_string(false).unwrap();
    assert!(
        pem.contains("-----BEGIN RSA PRIVATE KEY-----"),
        "the PKCS#1 label is what mbedTLS writes, so a Godot project's key \
         loads here and ours loads there: {pem}"
    );

    let reloaded = CryptoKey::load_from_string(&pem, false).unwrap();
    assert!(!reloaded.is_public_only());
    assert!(reloaded.verify_sha256(&digest, &sig));
    // The reloaded key must still be able to SIGN, not merely verify: a loader
    // that kept only the modulus would pass the line above.
    assert!(reloaded.verify_sha256(&digest, &reloaded.sign_sha256(&digest).unwrap()));
}

#[test]
fn a_public_only_export_cannot_sign_and_does_not_carry_the_private_key() {
    let key = CryptoKey::generate(BITS).unwrap();
    let digest = sha256(b"public only");
    let sig = key.sign_sha256(&digest).unwrap();

    let pub_pem = key.save_to_string(true).unwrap();
    assert!(pub_pem.contains("-----BEGIN PUBLIC KEY-----"), "{pub_pem}");
    assert!(
        !pub_pem.contains("PRIVATE"),
        "a public_only export must not leak the private half: {pub_pem}"
    );

    let public = CryptoKey::load_from_string(&pub_pem, true).unwrap();
    assert!(public.is_public_only());
    assert!(
        public.verify_sha256(&digest, &sig),
        "verification needs only the public half"
    );
    assert!(
        public.sign_sha256(&digest).is_err(),
        "signing with a public-only key must ERROR rather than return empty \
         bytes a caller could mistake for a signature"
    );
    assert!(public.decrypt(&key.encrypt(b"x").unwrap()).is_err());
}

/// `public_only = true` on a PEM that DOES carry the private key must discard
/// it, not keep it — otherwise a server loading its own key pair defensively
/// ends up able to sign with it anyway.
#[test]
fn loading_a_private_pem_public_only_discards_the_private_half() {
    let key = CryptoKey::generate(BITS).unwrap();
    let private_pem = key.save_to_string(false).unwrap();

    let narrowed = CryptoKey::load_from_string(&private_pem, true).unwrap();
    assert!(narrowed.is_public_only());
    assert!(narrowed.sign_sha256(&sha256(b"nope")).is_err());
    assert!(
        narrowed
            .save_to_string(false)
            .unwrap()
            .contains("-----BEGIN PUBLIC KEY-----"),
        "re-exporting a narrowed key must not resurrect the private half"
    );
}

#[test]
fn encrypt_decrypt_round_trips_and_a_long_message_is_refused() {
    let key = CryptoKey::generate(BITS).unwrap();
    let ct = key.encrypt(b"sixteen byte msg").unwrap();
    assert_eq!(ct.len(), key.size_bytes());
    assert_eq!(key.decrypt(&ct).unwrap(), b"sixteen byte msg");

    // PKCS#1 v1.5 caps the plaintext at the modulus width minus 11 bytes.
    // Godot surfaces that as a failure; silently truncating would lose data.
    let too_long = vec![0x41u8; key.size_bytes()];
    assert!(key.encrypt(&too_long).is_err());
}

#[test]
fn a_self_signed_certificate_round_trips_and_is_well_formed_der() {
    let key = CryptoKey::generate(BITS).unwrap();
    let cert = X509Certificate::self_signed(
        &key,
        "CN=myserver,O=myorganisation,C=IT",
        "20140101000000",
        "20340101000000",
    )
    .unwrap();

    let pem = cert.save_to_string();
    assert!(pem.contains("-----BEGIN CERTIFICATE-----"), "{pem}");
    let reloaded = X509Certificate::load_from_string(&pem).unwrap();
    assert_eq!(
        reloaded.der(),
        cert.der(),
        "PEM must round-trip the DER byte for byte"
    );

    // The outer SEQUENCE's declared length must account for every byte: the
    // cheapest check that the length encoder and the body actually agree, and
    // the one thing a round trip through our own reader cannot catch.
    let der = cert.der();
    assert_eq!(der[0], 0x30, "a certificate is a DER SEQUENCE");
    let (header, len) = if der[1] & 0x80 == 0 {
        (2usize, der[1] as usize)
    } else {
        let n = (der[1] & 0x7F) as usize;
        let mut len = 0usize;
        for b in &der[2..2 + n] {
            len = (len << 8) | *b as usize;
        }
        (2 + n, len)
    };
    assert_eq!(
        header + len,
        der.len(),
        "the certificate's declared length must match its actual size"
    );

    // 1.2.840.113549.1.1.11 — sha256WithRSAEncryption. It appears twice: once
    // in the TBS and once in the outer signatureAlgorithm, which RFC 5280
    // requires to match.
    let oid: &[u8] = &[0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x01, 0x0B];
    let occurrences = der.windows(oid.len()).filter(|w| *w == oid).count();
    assert_eq!(
        occurrences, 2,
        "the signature algorithm OID must appear in both the TBS and the outer \
         SEQUENCE, and be the same one"
    );

    // Issuer == subject is what "self-signed" means, so each attribute value
    // appears twice.
    for value in ["myserver", "myorganisation"] {
        let needle = value.as_bytes();
        let hits = der.windows(needle.len()).filter(|w| *w == needle).count();
        assert_eq!(
            hits, 2,
            "{value} must appear as both issuer and subject in a self-signed \
             certificate"
        );
    }
}

#[test]
fn certificate_issuance_rejects_what_it_cannot_encode() {
    let key = CryptoKey::generate(BITS).unwrap();

    // A public-only key cannot sign, so it cannot issue.
    let public = CryptoKey::load_from_string(&key.save_to_string(true).unwrap(), true).unwrap();
    assert!(
        X509Certificate::self_signed(&public, "CN=x", "20140101000000", "20340101000000").is_err()
    );

    // A malformed validity timestamp is an error, not a certificate that
    // claims a nonsense date.
    assert!(
        X509Certificate::self_signed(&key, "CN=x", "2014", "20340101000000").is_err(),
        "a 4-character timestamp is not YYYYMMDDHHMMSS"
    );
    // An unknown issuer attribute must fail rather than be dropped: silently
    // omitting `O=` would produce a certificate for a different subject.
    assert!(
        X509Certificate::self_signed(&key, "ZZ=unknown", "20140101000000", "20340101000000")
            .is_err()
    );

    // Not a certificate at all.
    assert!(X509Certificate::load_from_string(
        "-----BEGIN PUBLIC KEY-----\nAA==\n-----END PUBLIC KEY-----\n"
    )
    .is_err());
}

/// The key size guard: Godot takes 1024/2048/4096, and a nonsense size must
/// come back as an error rather than sending the keygen off to hunt for primes
/// that do not exist.
#[test]
fn an_out_of_range_key_size_is_refused() {
    assert!(CryptoKey::generate(0).is_err());
    assert!(CryptoKey::generate(64).is_err());
    assert!(CryptoKey::generate(100_000).is_err());
}

/// Garbage PEM must report which reading failed, not fall through to "not a
/// public key either".
#[test]
fn a_mangled_pem_is_an_error_not_an_empty_key() {
    assert!(CryptoKey::load_from_string("", false).is_err());
    assert!(CryptoKey::load_from_string("not a pem at all", false).is_err());
    assert!(CryptoKey::load_from_string(
        "-----BEGIN RSA PRIVATE KEY-----\nQUJD\n-----END RSA PRIVATE KEY-----\n",
        false
    )
    .is_err());
}
