//! RSA keys, PKCS#1 v1.5 signatures and self-signed X.509 certificates
//! (pat-s3rrv.195 — "stage 2" of the crypto port).
//!
//! Godot ground truth: `core/crypto/crypto.cpp` and the mbedTLS backend behind
//! it (`modules/mbedtls/crypto_mbedtls.cpp`), plus `doc/classes/Crypto.xml`,
//! `doc/classes/CryptoKey.xml` and `doc/classes/X509Certificate.xml`.
//!
//! 🔑 WHY THIS MODULE EXISTS AT ALL. `Crypto.generate_rsa`,
//! `generate_self_signed_certificate`, `sign`, `verify`, `encrypt` and
//! `decrypt` used to return `null` / an empty `PackedByteArray` / `false` from
//! the script binding, and `CryptoKey` / `X509Certificate` were registered in
//! ClassDB but had no constructor — so `ClassDB.class_exists("CryptoKey")`
//! reported `true` while `CryptoKey.new()` reached nothing. A ported project
//! that signed anything got silence, not an error.
//!
//! The stubs were deliberately *loud* (an empty signature, `verify` ⇒ `false`)
//! rather than plausible, which is why replacing them cannot have made any
//! previously-passing signature check start failing: there were none.
//!
//! ⚠️ SHA-256 ONLY for signatures. `Crypto.sign`/`verify` take a
//! `HashingContext.HashType`, and PKCS#1 v1.5 needs the digest's ASN.1
//! `DigestInfo` prefix — which the `rsa` crate derives from the digest type's
//! OID. Only SHA-256 is wired here; MD5 and SHA-1 report failure instead of
//! signing under the wrong prefix, which would produce a signature that no
//! other implementation (Godot included) would verify.

use crate::crypto::sha256;
use rsa::pkcs1::{DecodeRsaPrivateKey, EncodeRsaPrivateKey};
use rsa::pkcs8::spki::{DecodePublicKey, EncodePublicKey};
use rsa::pkcs8::DecodePrivateKey;
use rsa::traits::PublicKeyParts;
use rsa::{Pkcs1v15Encrypt, Pkcs1v15Sign, RsaPrivateKey, RsaPublicKey};

/// The OS CSPRNG, as an `rsa`-compatible RNG.
///
/// Draws from [`crate::crypto::generate_random_bytes`] — i.e. `getrandom`, the
/// same source Godot reaches through mbedTLS's CTR_DRBG — rather than from
/// [`crate::rng`], whose xorshift is seeded from the wall clock and would make
/// every generated key guessable from the process start time.
struct OsCsprng;

impl rsa::rand_core::RngCore for OsCsprng {
    fn next_u32(&mut self) -> u32 {
        let b = crate::crypto::generate_random_bytes(4);
        u32::from_le_bytes([b[0], b[1], b[2], b[3]])
    }

    fn next_u64(&mut self) -> u64 {
        let b = crate::crypto::generate_random_bytes(8);
        u64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]])
    }

    fn fill_bytes(&mut self, dest: &mut [u8]) {
        let b = crate::crypto::generate_random_bytes(dest.len());
        dest.copy_from_slice(&b);
    }

    fn try_fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), rsa::rand_core::Error> {
        self.fill_bytes(dest);
        Ok(())
    }
}

// SAFETY-OF-CONTRACT: this is the marker trait that lets `rsa` accept the RNG
// for key generation and PKCS#1 v1.5 encryption padding. It is sound here
// precisely because `fill_bytes` goes to the OS entropy source.
impl rsa::rand_core::CryptoRng for OsCsprng {}

/// Godot's `CryptoKey`: an RSA key pair, or a public key on its own.
///
/// `public` is always present; `private` is `None` for a key loaded
/// `public_only`. That shape is what makes [`Self::is_public_only`] a field
/// read rather than a guess, and it means [`Self::verify`] works on either kind
/// while [`Self::sign`] can refuse the public-only case with a real error.
#[derive(Clone)]
pub struct CryptoKey {
    private: Option<RsaPrivateKey>,
    public: RsaPublicKey,
}

impl CryptoKey {
    /// Generates a fresh RSA key of `bits` bits (Godot `Crypto.generate_rsa`).
    ///
    /// Godot's own call takes 1024/2048/4096; anything under 512 is rejected
    /// here rather than handed to the keygen, which would otherwise spend a
    /// long time failing to find primes for a nonsense size.
    pub fn generate(bits: usize) -> Result<Self, String> {
        if !(512..=8192).contains(&bits) {
            return Err(format!("RSA key size {bits} out of range (512..=8192)"));
        }
        let mut rng = OsCsprng;
        let private = RsaPrivateKey::new(&mut rng, bits)
            .map_err(|e| format!("RSA key generation failed: {e}"))?;
        let public = private.to_public_key();
        Ok(Self {
            private: Some(private),
            public,
        })
    }

    /// `true` when no private half is held (Godot `CryptoKey.is_public_only`).
    pub fn is_public_only(&self) -> bool {
        self.private.is_none()
    }

    /// Size of the modulus in bytes — the exact length of a signature or of a
    /// PKCS#1 v1.5 ciphertext block.
    pub fn size_bytes(&self) -> usize {
        self.public.size()
    }

    /// PEM text for this key (Godot `CryptoKey.save_to_string`).
    ///
    /// Labels match what mbedTLS writes, so a key saved here loads in Godot and
    /// vice versa: `RSA PRIVATE KEY` (PKCS#1) for the private half,
    /// `PUBLIC KEY` (SPKI) for the public one.
    ///
    /// The armour is written by [`pem_wrap`] rather than by the `der` crate's
    /// PEM helpers, so this path needs only the `alloc` half of `pkcs1`/`spki`
    /// and cannot be broken by a feature-flag change upstream.
    pub fn save_to_string(&self, public_only: bool) -> Result<String, String> {
        if public_only || self.private.is_none() {
            return Ok(pem_wrap("PUBLIC KEY", &self.spki_der()?));
        }
        let private = self.private.as_ref().expect("private half checked above");
        let der = private
            .to_pkcs1_der()
            .map_err(|e| format!("encoding PKCS#1 private key failed: {e}"))?;
        Ok(pem_wrap("RSA PRIVATE KEY", der.as_bytes()))
    }

    /// Parses PEM text (Godot `CryptoKey.load_from_string`).
    ///
    /// Dispatch is on the PEM LABEL, not on trial decoding, so a mangled
    /// private key reports a private-key error instead of falling through to
    /// "not a public key either".
    ///
    /// ⚠️ `public_only` DISCARDS the private half rather than rejecting the
    /// text, matching Godot: a server that loads its own key pair with
    /// `public_only = true` must not then be able to sign with it.
    pub fn load_from_string(pem: &str, public_only: bool) -> Result<Self, String> {
        let key = if pem.contains("-----BEGIN RSA PRIVATE KEY-----") {
            let der = pem_unwrap("RSA PRIVATE KEY", pem)?;
            let private = RsaPrivateKey::from_pkcs1_der(&der)
                .map_err(|e| format!("bad PKCS#1 private key: {e}"))?;
            let public = private.to_public_key();
            Self {
                private: Some(private),
                public,
            }
        } else if pem.contains("-----BEGIN PRIVATE KEY-----") {
            let der = pem_unwrap("PRIVATE KEY", pem)?;
            let private = RsaPrivateKey::from_pkcs8_der(&der)
                .map_err(|e| format!("bad PKCS#8 private key: {e}"))?;
            let public = private.to_public_key();
            Self {
                private: Some(private),
                public,
            }
        } else if pem.contains("-----BEGIN PUBLIC KEY-----") {
            let der = pem_unwrap("PUBLIC KEY", pem)?;
            Self {
                private: None,
                public: RsaPublicKey::from_public_key_der(&der)
                    .map_err(|e| format!("bad SubjectPublicKeyInfo: {e}"))?,
            }
        } else {
            return Err("no RSA key PEM block found".into());
        };
        if public_only {
            return Ok(Self {
                private: None,
                public: key.public,
            });
        }
        Ok(key)
    }

    /// PKCS#1 v1.5 signature over an already-computed SHA-256 digest
    /// (Godot `Crypto.sign`).
    ///
    /// `digest` is the hash, NOT the message: Godot's signature takes the
    /// output of `HashingContext`/`String.sha256_buffer`, and prefixing the
    /// DigestInfo is this function's job.
    pub fn sign_sha256(&self, digest: &[u8]) -> Result<Vec<u8>, String> {
        if digest.len() != 32 {
            return Err(format!(
                "Crypto.sign with HASH_SHA256 needs a 32-byte digest, got {}",
                digest.len()
            ));
        }
        let private = self
            .private
            .as_ref()
            .ok_or_else(|| "cannot sign with a public-only key".to_string())?;
        private
            .sign(Pkcs1v15Sign::new::<rsa::sha2::Sha256>(), digest)
            .map_err(|e| format!("RSA signing failed: {e}"))
    }

    /// Verifies a PKCS#1 v1.5 signature over a SHA-256 digest
    /// (Godot `Crypto.verify`).
    ///
    /// Returns `false` for every failure — a wrong digest, a tampered
    /// signature, a mismatched key — because that is the one answer a caller
    /// must not be able to confuse with success.
    pub fn verify_sha256(&self, digest: &[u8], signature: &[u8]) -> bool {
        if digest.len() != 32 {
            return false;
        }
        self.public
            .verify(Pkcs1v15Sign::new::<rsa::sha2::Sha256>(), digest, signature)
            .is_ok()
    }

    /// PKCS#1 v1.5 encryption with the public half (Godot `Crypto.encrypt`).
    ///
    /// Godot caps the plaintext at `key_size - 11` bytes and so does the `rsa`
    /// crate; the error is surfaced rather than truncated.
    pub fn encrypt(&self, plaintext: &[u8]) -> Result<Vec<u8>, String> {
        let mut rng = OsCsprng;
        self.public
            .encrypt(&mut rng, Pkcs1v15Encrypt, plaintext)
            .map_err(|e| format!("RSA encryption failed: {e}"))
    }

    /// PKCS#1 v1.5 decryption with the private half (Godot `Crypto.decrypt`).
    pub fn decrypt(&self, ciphertext: &[u8]) -> Result<Vec<u8>, String> {
        let private = self
            .private
            .as_ref()
            .ok_or_else(|| "cannot decrypt with a public-only key".to_string())?;
        private
            .decrypt(Pkcs1v15Encrypt, ciphertext)
            .map_err(|e| format!("RSA decryption failed: {e}"))
    }

    /// The `SubjectPublicKeyInfo` DER for this key, ready to drop into a
    /// certificate's `subjectPublicKeyInfo` field.
    fn spki_der(&self) -> Result<Vec<u8>, String> {
        let doc = self
            .public
            .to_public_key_der()
            .map_err(|e| format!("encoding SubjectPublicKeyInfo failed: {e}"))?;
        Ok(doc.as_bytes().to_vec())
    }
}

/// Godot's `X509Certificate`.
///
/// The certificate is held as its DER encoding, which is what both PEM
/// round-tripping and any future TLS handover need. Nothing here parses a
/// certificate's contents: a cert that came in is re-emitted byte for byte,
/// so `load_from_string(save_to_string())` cannot drift.
#[derive(Clone)]
pub struct X509Certificate {
    der: Vec<u8>,
}

impl X509Certificate {
    /// Builds a self-signed v3 certificate over `key`
    /// (Godot `Crypto.generate_self_signed_certificate`).
    ///
    /// `issuer_name` is Godot's comma-separated RDN string, e.g.
    /// `"CN=myserver,O=myorganisation,C=IT"`. `not_before` / `not_after` are
    /// Godot's `"YYYYMMDDHHMMSS"`.
    ///
    /// Self-signed means issuer == subject and the signature is made with the
    /// certificate's own key, so `key` must carry its private half.
    pub fn self_signed(
        key: &CryptoKey,
        issuer_name: &str,
        not_before: &str,
        not_after: &str,
    ) -> Result<Self, String> {
        if key.is_public_only() {
            return Err("generate_self_signed_certificate needs a private key".into());
        }
        let name = encode_rdn_sequence(issuer_name)?;
        let validity = der_sequence(&[encode_time(not_before)?, encode_time(not_after)?]);
        let spki = key.spki_der()?;

        // X.509 v3 is `version = 2` in the explicit [0] wrapper.
        let version = der_explicit(0, &der_integer(&[2]));
        // 8 random bytes, read as a positive integer. A fixed serial would make
        // every certificate this engine emits collide in any store keyed by
        // (issuer, serial).
        let serial = der_integer(&crate::crypto::generate_random_bytes(8));
        let sig_alg = der_sequence(&[OID_SHA256_WITH_RSA.to_vec(), der_null()]);

        let tbs = der_sequence(&[
            version,
            serial,
            sig_alg.clone(),
            name.clone(),
            validity,
            name,
            spki,
        ]);

        let signature = key.sign_sha256(&sha256(&tbs))?;
        let der = der_sequence(&[tbs, sig_alg, der_bit_string(&signature)]);
        Ok(Self { der })
    }

    /// The certificate's DER bytes.
    pub fn der(&self) -> &[u8] {
        &self.der
    }

    /// PEM text (Godot `X509Certificate.save_to_string`).
    pub fn save_to_string(&self) -> String {
        pem_wrap("CERTIFICATE", &self.der)
    }

    /// Parses PEM text (Godot `X509Certificate.load_from_string`).
    pub fn load_from_string(pem: &str) -> Result<Self, String> {
        let der = pem_unwrap("CERTIFICATE", pem)?;
        if der.first() != Some(&0x30) {
            return Err("certificate body is not a DER SEQUENCE".into());
        }
        Ok(Self { der })
    }

    /// DER straight from a loader that already has the bytes.
    pub fn from_der(der: Vec<u8>) -> Result<Self, String> {
        if der.first() != Some(&0x30) {
            return Err("certificate body is not a DER SEQUENCE".into());
        }
        Ok(Self { der })
    }
}

// ---------------------------------------------------------------------------
// DER encoding
// ---------------------------------------------------------------------------
//
// Hand-written rather than pulled from `x509-cert`'s builder: everything a
// self-signed certificate needs is a handful of tag/length/value writes, and
// the OID byte strings below are transcribed from the registry rather than
// derived, because an arc encoder is one off-by-one away from a certificate
// that parses as a different algorithm.

/// `rsaEncryption`, 1.2.840.113549.1.1.1.
#[allow(dead_code)]
const OID_RSA_ENCRYPTION: &[u8] = &[
    0x06, 0x09, 0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x01, 0x01,
];
/// `sha256WithRSAEncryption`, 1.2.840.113549.1.1.11.
const OID_SHA256_WITH_RSA: &[u8] = &[
    0x06, 0x09, 0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x01, 0x0B,
];

/// The attribute OIDs Godot's issuer strings use, by their short name.
///
/// `emailAddress` lives under the PKCS#9 arc, not under 2.5.4 like the rest —
/// which is exactly the kind of thing that goes wrong when OIDs are derived
/// from a pattern instead of transcribed.
const ATTRIBUTE_OIDS: &[(&str, &[u8])] = &[
    ("CN", &[0x06, 0x03, 0x55, 0x04, 0x03]),
    ("SN", &[0x06, 0x03, 0x55, 0x04, 0x04]),
    ("C", &[0x06, 0x03, 0x55, 0x04, 0x06]),
    ("L", &[0x06, 0x03, 0x55, 0x04, 0x07]),
    ("ST", &[0x06, 0x03, 0x55, 0x04, 0x08]),
    ("O", &[0x06, 0x03, 0x55, 0x04, 0x0A]),
    ("OU", &[0x06, 0x03, 0x55, 0x04, 0x0B]),
    (
        "emailAddress",
        &[
            0x06, 0x09, 0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x09, 0x01,
        ],
    ),
];

/// DER length bytes for `len` (short form under 128, else long form).
fn der_length(len: usize) -> Vec<u8> {
    if len < 0x80 {
        return vec![len as u8];
    }
    let be = len.to_be_bytes();
    let first = be.iter().position(|b| *b != 0).unwrap_or(be.len() - 1);
    let body = &be[first..];
    let mut out = Vec::with_capacity(body.len() + 1);
    out.push(0x80 | body.len() as u8);
    out.extend_from_slice(body);
    out
}

/// One tag-length-value triple.
fn der_tlv(tag: u8, body: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(body.len() + 4);
    out.push(tag);
    out.extend_from_slice(&der_length(body.len()));
    out.extend_from_slice(body);
    out
}

fn der_sequence(parts: &[Vec<u8>]) -> Vec<u8> {
    let mut body = Vec::new();
    for p in parts {
        body.extend_from_slice(p);
    }
    der_tlv(0x30, &body)
}

fn der_set(body: &[u8]) -> Vec<u8> {
    der_tlv(0x31, body)
}

fn der_null() -> Vec<u8> {
    vec![0x05, 0x00]
}

/// A non-negative INTEGER from big-endian `bytes`.
///
/// ⚠️ Two corrections are mandatory and both are silent if skipped: leading
/// zero bytes are not allowed, and a leading byte with its top bit set would
/// read as a NEGATIVE number, so it needs a `0x00` in front. A serial number
/// that goes negative is how a certificate gets rejected by a strict parser
/// one time in two.
fn der_integer(bytes: &[u8]) -> Vec<u8> {
    let first = bytes.iter().position(|b| *b != 0);
    let trimmed = match first {
        Some(i) => &bytes[i..],
        None => &[0u8][..],
    };
    let mut body = Vec::with_capacity(trimmed.len() + 1);
    if trimmed[0] & 0x80 != 0 {
        body.push(0x00);
    }
    body.extend_from_slice(trimmed);
    der_tlv(0x02, &body)
}

/// A BIT STRING with no unused trailing bits.
fn der_bit_string(bytes: &[u8]) -> Vec<u8> {
    let mut body = Vec::with_capacity(bytes.len() + 1);
    body.push(0x00);
    body.extend_from_slice(bytes);
    der_tlv(0x03, &body)
}

/// A context-specific constructed `[n] EXPLICIT` wrapper.
fn der_explicit(n: u8, body: &[u8]) -> Vec<u8> {
    der_tlv(0xA0 | n, body)
}

/// Godot's `"YYYYMMDDHHMMSS"` as the right ASN.1 time type.
///
/// 🔑 UTCTime CARRIES A TWO-DIGIT YEAR and is only defined for 1950..=2049;
/// RFC 5280 requires GeneralizedTime from 2050 on. Godot's own default
/// `not_after` is `"20340101000000"`, which is inside the UTCTime window — so a
/// UTCTime-only encoder looks correct until somebody passes a later date and
/// gets a certificate that claims the year 50.
fn encode_time(stamp: &str) -> Result<Vec<u8>, String> {
    if stamp.len() != 14 || !stamp.bytes().all(|b| b.is_ascii_digit()) {
        return Err(format!(
            "certificate validity must be 14 digits YYYYMMDDHHMMSS, got {stamp:?}"
        ));
    }
    let year: u32 = stamp[0..4]
        .parse()
        .map_err(|_| format!("bad year in {stamp:?}"))?;
    if (1950..=2049).contains(&year) {
        let mut s = String::with_capacity(13);
        s.push_str(&stamp[2..]);
        s.push('Z');
        Ok(der_tlv(0x17, s.as_bytes()))
    } else {
        let mut s = String::with_capacity(15);
        s.push_str(stamp);
        s.push('Z');
        Ok(der_tlv(0x18, s.as_bytes()))
    }
}

/// Godot's comma-separated `"CN=a,O=b,C=IT"` as an X.501 `Name`.
///
/// Each attribute becomes its own RelativeDistinguishedName, in the order
/// written — the order is part of the encoded name, so it must not be sorted.
fn encode_rdn_sequence(issuer: &str) -> Result<Vec<u8>, String> {
    let mut rdns: Vec<Vec<u8>> = Vec::new();
    for field in issuer.split(',') {
        let field = field.trim();
        if field.is_empty() {
            continue;
        }
        let (key, value) = field
            .split_once('=')
            .ok_or_else(|| format!("issuer field {field:?} is not KEY=VALUE"))?;
        let key = key.trim();
        let value = value.trim();
        let oid = ATTRIBUTE_OIDS
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case(key))
            .map(|(_, oid)| *oid)
            .ok_or_else(|| format!("unsupported issuer attribute {key:?}"))?;
        // `C` is a PrintableString by definition (two letters); the rest are
        // written as UTF8String, which is what every current CA emits.
        let encoded_value = if key.eq_ignore_ascii_case("C") {
            der_tlv(0x13, value.as_bytes())
        } else {
            der_tlv(0x0C, value.as_bytes())
        };
        let attr = der_sequence(&[oid.to_vec(), encoded_value]);
        rdns.push(der_set(&attr));
    }
    if rdns.is_empty() {
        return Err("issuer name has no attributes".into());
    }
    Ok(der_sequence(&rdns))
}

// ---------------------------------------------------------------------------
// PEM
// ---------------------------------------------------------------------------

const BASE64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn base64_encode(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = *chunk.get(1).unwrap_or(&0) as u32;
        let b2 = *chunk.get(2).unwrap_or(&0) as u32;
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(BASE64[(n >> 18) as usize & 0x3F] as char);
        out.push(BASE64[(n >> 12) as usize & 0x3F] as char);
        out.push(if chunk.len() > 1 {
            BASE64[(n >> 6) as usize & 0x3F] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            BASE64[n as usize & 0x3F] as char
        } else {
            '='
        });
    }
    out
}

fn base64_decode(text: &str) -> Result<Vec<u8>, String> {
    let mut acc: u32 = 0;
    let mut bits = 0u32;
    let mut out = Vec::new();
    for ch in text.chars() {
        if ch.is_whitespace() || ch == '=' {
            continue;
        }
        let v = BASE64
            .iter()
            .position(|c| *c as char == ch)
            .ok_or_else(|| format!("invalid base64 character {ch:?}"))?;
        acc = (acc << 6) | v as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    Ok(out)
}

/// Wraps `der` in a PEM armour with 64-character lines, as mbedTLS writes it.
fn pem_wrap(label: &str, der: &[u8]) -> String {
    let b64 = base64_encode(der);
    let mut out = String::with_capacity(b64.len() + b64.len() / 64 + 2 * label.len() + 32);
    out.push_str("-----BEGIN ");
    out.push_str(label);
    out.push_str("-----\n");
    for line in b64.as_bytes().chunks(64) {
        out.push_str(std::str::from_utf8(line).expect("base64 is ASCII"));
        out.push('\n');
    }
    out.push_str("-----END ");
    out.push_str(label);
    out.push_str("-----\n");
    out
}

/// Pulls the body out of a PEM armour, checking the label.
fn pem_unwrap(label: &str, text: &str) -> Result<Vec<u8>, String> {
    let begin = format!("-----BEGIN {label}-----");
    let end = format!("-----END {label}-----");
    let start = text
        .find(&begin)
        .ok_or_else(|| format!("missing `{begin}`"))?
        + begin.len();
    let stop = text[start..]
        .find(&end)
        .ok_or_else(|| format!("missing `{end}`"))?
        + start;
    base64_decode(&text[start..stop])
}

#[cfg(test)]
mod tests {
    use super::*;

    // ⚠️ KEY TESTS LIVE ELSEWHERE, on purpose. Anything that needs a real
    // RSA key — sign/verify, encrypt/decrypt, PEM round trips, certificate
    // issuance — is in `gdcore/tests/crypto_rsa_keys_and_certs.rs`, because a
    // test that only exists inside the lib can only be run with `--lib`, and a
    // `--lib` filter is not a gate the verifier lane can point at a file. What
    // stays here is what an integration test CANNOT reach: the private DER,
    // base64 and PEM helpers.

    #[test]
    fn base64_round_trips_every_tail_length() {
        for len in 0..16usize {
            let data: Vec<u8> = (0..len).map(|i| (i * 37 + 11) as u8).collect();
            let text = base64_encode(&data);
            assert_eq!(
                base64_decode(&text).unwrap(),
                data,
                "length {len} must survive base64"
            );
        }
    }

    #[test]
    fn der_integer_keeps_a_high_byte_positive() {
        // 0x80 would read as a negative number without the 0x00 prefix.
        assert_eq!(der_integer(&[0x80]), vec![0x02, 0x02, 0x00, 0x80]);
        // Leading zeros are not allowed in DER.
        assert_eq!(der_integer(&[0x00, 0x00, 0x2A]), vec![0x02, 0x01, 0x2A]);
        // All-zero still has to encode as one zero byte.
        assert_eq!(der_integer(&[0x00]), vec![0x02, 0x01, 0x00]);
    }

    #[test]
    fn der_length_switches_to_the_long_form_at_128() {
        assert_eq!(der_length(0x7F), vec![0x7F]);
        assert_eq!(der_length(0x80), vec![0x81, 0x80]);
        assert_eq!(der_length(0x1234), vec![0x82, 0x12, 0x34]);
    }

    #[test]
    fn utctime_below_2050_generalized_above() {
        // Godot's default not_after is 2034 — inside the UTCTime window.
        assert_eq!(encode_time("20340101000000").unwrap()[0], 0x17);
        assert_eq!(encode_time("20500101000000").unwrap()[0], 0x18);
        assert!(encode_time("2034").is_err());
    }

    #[test]
    fn an_unknown_issuer_attribute_is_an_error_not_a_dropped_field() {
        assert!(encode_rdn_sequence("ZZ=nope").is_err());
        assert!(encode_rdn_sequence("CN").is_err());
        assert!(encode_rdn_sequence("").is_err());
        assert!(encode_rdn_sequence("CN=a,O=b,C=IT").is_ok());
    }
}
