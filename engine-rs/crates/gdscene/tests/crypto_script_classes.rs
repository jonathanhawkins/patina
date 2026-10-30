//! pat-0q2kx.206 acceptance: `HashingContext` / `AESContext` / `Crypto` are
//! constructible and operable from a scene-attached script.
//!
//! Godot ground truth: `doc/classes/{HashingContext,AESContext,Crypto}.xml` and
//! `core/crypto/crypto_core.cpp`. Digest vectors are the published ones for
//! `"abc"`. The HMAC case is HMAC-SHA256(key="key", msg="msg") — NOT an RFC
//! 4231 vector (those use a 20-byte 0x0b key), but a value cross-checked against
//! Python's `hmac` before it was written down.
//!
//! `gdcore::crypto` has had streaming MD5/SHA-1/SHA-256 (`MultiHashContext`),
//! HMAC (`HmacContext`) and real AES-CBC all along, and **none of it was
//! reachable from a script**: only `String.sha256_text` touched the module,
//! there were no `.new()` arms, and ClassDB denied all six classes existed. So
//! `HashingContext.new()` failed outright.
//!
//! ✅ STAGE 2 HAS LANDED (pat-s3rrv.195): RSA keygen, sign / verify / encrypt /
//! decrypt, self-signed X509, and `CryptoKey` / `X509Certificate` construction
//! with PEM save/load all work from a script, through `gdcore::crypto_rsa`.
//! `crypto_rsa_x509_and_csprng_reachable_from_script` below is the acceptance.
//!
//! The old `the_rsa_half_reports_unavailable_rather_than_faking_it` has been
//! REWRITTEN rather than deleted, as the note here asked: it now pins that a
//! MISSING key still yields empty bytes and `false` instead of a signature
//! that verifies nothing. Deleting it would have dropped the one guard that
//! says a failure must stay visibly a failure.
//!
//! ⚠️ `Crypto.sign`/`verify` are SHA-256 ONLY. MD5 and SHA-1 report failure
//! rather than signing under the wrong PKCS#1 DigestInfo prefix, which would
//! produce a signature no other implementation — Godot included — verifies.
//!
//! ✅ Two caveats this header used to carry have since been closed, and the old
//! text had gone stale enough to mis-triage the bead:
//! - `AESContext.get_iv_state()` IS wired — `gdscript-interop`'s `"get_iv_state"`
//!   arm returns `AesContext::get_iv_state()`, covered by
//!   `gdcore/tests/aes_cbc_chaining_and_iv_state.rs`.
//! - `Crypto.generate_random_bytes` IS a CSPRNG — `gdcore::crypto` draws it from
//!   `getrandom::fill` (OS entropy), not the SystemTime-seeded xorshift it used
//!   to share with `randi()`. The two script-level draws asserted below are the
//!   real Godot idiom now, not a placeholder.

use gdscene::main_loop::MainLoop;
use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscene::{GDScriptNodeInstance, LifecycleManager};
use gdscript_interop::bindings::ScriptInstance;
use gdvariant::Variant;

/// Everything the acceptance asks for, computed inside one scripted node.
///
/// Bytes are passed as plain strings: the binding takes a `String` as its UTF-8
/// bytes, so the script does not depend on `to_utf8_buffer()` also being bound.
const CRYPTO_GD: &str = "\
extends Node
var md5_abc = null
var sha1_abc = null
var sha256_abc = null
var ecb128_round_trip = null
var cbc256_round_trip = null
var random_a = null
var random_b = null
var hmac_sha256 = null
var equal_compare = false
var unequal_compare = true

func _ready():
    var h = HashingContext.new()
    h.start(0)
    h.update('a')
    h.update('bc')
    self.md5_abc = h.finish()

    h.start(1)
    h.update('abc')
    self.sha1_abc = h.finish()

    h.start(2)
    h.update('abc')
    self.sha256_abc = h.finish()

    var aes = AESContext.new()
    aes.start(0, '0123456789abcdef', '')
    var ct = aes.update('sixteen byte msg')
    aes.start(1, '0123456789abcdef', '')
    self.ecb128_round_trip = aes.update(ct)

    aes.start(2, '0123456789abcdef0123456789abcdef', 'iviviviviviviviv')
    var ct2 = aes.update('sixteen byte msg')
    aes.start(3, '0123456789abcdef0123456789abcdef', 'iviviviviviviviv')
    self.cbc256_round_trip = aes.update(ct2)

    var c = Crypto.new()
    self.random_a = c.generate_random_bytes(16)
    self.random_b = c.generate_random_bytes(16)
    self.hmac_sha256 = c.hmac_digest(2, 'key', 'msg')
    self.equal_compare = c.constant_time_compare('abc', 'abc')
    self.unequal_compare = c.constant_time_compare('abc', 'abd')
";

fn script_var(ml: &MainLoop, node: NodeId, name: &str) -> Option<Variant> {
    ml.tree()
        .get_script(node)?
        .list_properties()
        .into_iter()
        .find(|p| p.name == name)
        .map(|p| p.default_value)
}

/// Reads a script var that must be a `PackedByteArray`, failing loudly rather
/// than letting a `Nil` slide through as "no bytes".
fn bytes_var(ml: &MainLoop, node: NodeId, name: &str) -> Vec<u8> {
    match script_var(ml, node, name) {
        Some(Variant::PackedByteArray(b)) => b,
        other => panic!("`{name}` must be a PackedByteArray, got {other:?}"),
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Reads a script var that must be a `String`.
///
/// Loud on `Nil` for the same reason as [`bytes_var`]: an empty string would
/// make `contains("-----BEGIN ...")` fail with a message about PEM labels when
/// the real problem was that the script never ran.
fn string_var(ml: &MainLoop, node: NodeId, name: &str) -> String {
    match script_var(ml, node, name) {
        Some(Variant::String(s)) => s,
        other => panic!("`{name}` must be a String, got {other:?}"),
    }
}

/// Builds the scene and runs `_ready`.
///
/// ⚠️ `_ready` is dispatched by `LifecycleManager::enter_tree`, **not** by
/// `MainLoop::step` — `add_child` alone does not fire it. Without the explicit
/// call every var here stays at its declared `null` and the file fails on the
/// first assertion with `got Some(Nil)`, which reads like "the crypto binding
/// returned nothing" rather than "the script never ran".
fn run_crypto_script() -> (MainLoop, NodeId) {
    let mut tree = SceneTree::new();
    let root_id = tree.root_id();
    let node = tree.add_child(root_id, Node::new("Crypt", "Node")).unwrap();
    tree.attach_script(
        node,
        Box::new(GDScriptNodeInstance::from_source(CRYPTO_GD, node).unwrap()),
    );
    LifecycleManager::enter_tree(&mut tree, node);
    let mut ml = MainLoop::new(tree);
    ml.step(1.0 / 60.0);
    (ml, node)
}

/// **The acceptance.** Every clause, from a scene-attached script.
#[test]
fn crypto_script_classes() {
    let (ml, node) = run_crypto_script();

    // -- HashingContext, against the published vectors for "abc" -------------
    // Split across two `update` calls for MD5 specifically, so this also pins
    // that the context genuinely streams rather than hashing the last chunk.
    assert_eq!(
        hex(&bytes_var(&ml, node, "md5_abc")),
        "900150983cd24fb0d6963f7d28e17f72",
        "MD5(\"abc\"), fed as \"a\" then \"bc\""
    );
    assert_eq!(
        hex(&bytes_var(&ml, node, "sha1_abc")),
        "a9993e364706816aba3e25717850c26c9cd0d89d",
        "SHA-1(\"abc\")"
    );
    assert_eq!(
        hex(&bytes_var(&ml, node, "sha256_abc")),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
        "SHA-256(\"abc\") — the vector the bead names"
    );

    // -- AESContext round trips ----------------------------------------------
    let ecb = bytes_var(&ml, node, "ecb128_round_trip");
    assert_eq!(
        String::from_utf8_lossy(&ecb),
        "sixteen byte msg",
        "ECB-128 encrypt->decrypt must restore the plaintext"
    );
    let cbc = bytes_var(&ml, node, "cbc256_round_trip");
    assert_eq!(
        String::from_utf8_lossy(&cbc),
        "sixteen byte msg",
        "CBC-256 encrypt->decrypt must restore the plaintext"
    );

    // -- Crypto.generate_random_bytes ----------------------------------------
    let a = bytes_var(&ml, node, "random_a");
    let b = bytes_var(&ml, node, "random_b");
    assert_eq!(
        a.len(),
        16,
        "generate_random_bytes(16) must return 16 bytes"
    );
    assert_eq!(
        b.len(),
        16,
        "generate_random_bytes(16) must return 16 bytes"
    );
    assert_ne!(a, b, "two draws must differ — a constant is not random");
    assert!(
        a.iter().any(|byte| *byte != a[0]),
        "the 16 bytes must not all be identical, got {a:?}"
    );

    // -- hmac_digest -----------------------------------------------------------
    // HMAC-SHA256(key="key", msg="msg"), cross-checked against Python's `hmac`.
    assert_eq!(
        hex(&bytes_var(&ml, node, "hmac_sha256")),
        "2d93cbc1be167bcb1637a4a23cbff01a7878f0c50ee833954ea5221bb1b8c628",
        "HMAC-SHA256(\"key\", \"msg\")"
    );

    // -- constant_time_compare -------------------------------------------------
    assert_eq!(
        script_var(&ml, node, "equal_compare"),
        Some(Variant::Bool(true)),
        "equal buffers must compare equal"
    );
    assert_eq!(
        script_var(&ml, node, "unequal_compare"),
        Some(Variant::Bool(false)),
        "buffers differing in the last byte must compare unequal — an early-exit \
         comparison that only checks a prefix would pass the equal case and fail \
         here"
    );
}

/// The classes must exist in ClassDB too, not merely be constructible: a script
/// calling `ClassDB.class_exists("AESContext")` is how a plugin feature-detects.
#[test]
fn the_crypto_classes_are_registered() {
    gdobject::class_db::register_io_classes();
    use gdobject::class_db::{class_exists, class_has_method, get_enum_constants};

    for class in [
        "HashingContext",
        "AESContext",
        "HMACContext",
        "Crypto",
        "CryptoKey",
        "X509Certificate",
    ] {
        assert!(class_exists(class), "{class} must be registered");
    }

    assert!(class_has_method("HashingContext", "finish"));
    assert!(class_has_method("Crypto", "constant_time_compare"));
    // Registered even though stage 2 answers them with null/empty/false, so
    // feature detection by method name still works.
    assert!(class_has_method("Crypto", "generate_rsa"));

    // ⚠️ ECB comes FIRST in Godot's Mode enum. Assuming the intuitive CBC=0
    // would encrypt in the wrong mode without any error.
    let modes = get_enum_constants("AESContext", "Mode");
    assert!(
        modes.iter().any(|c| c == "MODE_ECB_ENCRYPT"),
        "AESContext.Mode must be a named enum, got {modes:?}"
    );
    assert_eq!(
        gdobject::class_db::class_get_integer_constant("AESContext", "MODE_CBC_ENCRYPT"),
        Some(2),
        "MODE_CBC_ENCRYPT is 2, after both ECB modes"
    );
    assert_eq!(
        gdobject::class_db::class_get_integer_constant("HashingContext", "HASH_SHA256"),
        Some(2),
        "HASH_SHA256 is 2"
    );
}

/// **pat-s3rrv.195.** `HMACContext` is constructible and its streaming digest
/// matches the one-shot `Crypto.hmac_digest`.
///
/// The class was registered in ClassDB and implemented in `gdcore::crypto`
/// (unit-tested against the RFC 2202 vectors), but nothing constructed one —
/// `HMACContext.new()` reached no handle, so the streaming API was unusable
/// from a script while `ClassDB.class_exists` cheerfully reported true.
///
/// The assertion is that the two paths AGREE, not that either matches a
/// hard-coded constant: a shared digest that both compute wrongly would be a
/// different bug, and `hmac_sha256` above already pins the value itself.
#[test]
fn hmac_context_streams_to_the_same_digest_as_hmac_digest() {
    const HMAC_GD: &str = "\
extends Node
var streamed = null
var one_shot = null
var started_late = null

func _ready():
    var h = HMACContext.new()
    h.start(2, 'key')
    h.update('ms')
    h.update('g')
    self.streamed = h.finish()

    var c = Crypto.new()
    self.one_shot = c.hmac_digest(2, 'key', 'msg')

    # `update` before `start` must report an error, not accumulate silently.
    var late = HMACContext.new()
    self.started_late = late.update('x')
";
    let mut tree = SceneTree::new();
    let root_id = tree.root_id();
    let node = tree.add_child(root_id, Node::new("Hmac", "Node")).unwrap();
    tree.attach_script(
        node,
        Box::new(GDScriptNodeInstance::from_source(HMAC_GD, node).unwrap()),
    );
    LifecycleManager::enter_tree(&mut tree, node);
    let mut ml = MainLoop::new(tree);
    ml.step(1.0 / 60.0);

    let streamed = script_var(&ml, node, "streamed");
    let one_shot = script_var(&ml, node, "one_shot");

    // Split across two `update` calls on purpose: a context that ignored all
    // but the last chunk would still produce *a* digest, just not this one.
    assert_eq!(
        streamed, one_shot,
        "HMACContext.start/update/update/finish must equal Crypto.hmac_digest \
         over the concatenated message"
    );
    assert!(
        !matches!(streamed, Some(Variant::PackedByteArray(ref b)) if b.is_empty()),
        "an empty digest would make the equality above pass vacuously, got {streamed:?}"
    );

    assert_eq!(
        script_var(&ml, node, "started_late"),
        Some(Variant::Int(3)),
        "update() before start() must return ERR_UNCONFIGURED, not OK"
    );
}

/// A call that cannot succeed must fail visibly rather than return something
/// that looks like a key or a signature.
///
/// 🔑 THIS TEST USED TO PIN THE STUBS. It asserted `generate_rsa` ⇒ `null`,
/// because stage 2 was not implemented. Landing stage 2 falsified that clause,
/// so it was swapped for the real behaviour — the key is now an object — while
/// the two clauses that are STILL true were kept verbatim: `sign` and `verify`
/// are called here with `null` where the key belongs, and a missing key must
/// not produce bytes that look like a signature, nor a `true` that would make
/// every check pass. Weakening those, or deleting the test along with the stub
/// it described, would have removed the only guard on the failure path.
#[test]
fn the_rsa_half_refuses_a_missing_key_rather_than_faking_it() {
    const RSA_GD: &str = "\
extends Node
var key = null
var signature = null
var verified = true
var wrong_hash = null

func _ready():
    var c = Crypto.new()
    # 1024, not 2048: this test asserts the TYPE of what comes back, not the
    # modulus width, and a second 2048-bit prime search doubles the file's cost.
    self.key = c.generate_rsa(1024)
    self.signature = c.sign(2, 'digest', null)
    self.verified = c.verify(2, 'digest', 'sig', null)

    # MD5 (hash type 0) has no wired DigestInfo prefix: refuse, do not sign.
    var h = HashingContext.new()
    h.start(0)
    h.update('abc')
    self.wrong_hash = c.sign(0, h.finish(), self.key)
";
    let mut tree = SceneTree::new();
    let root_id = tree.root_id();
    let node = tree.add_child(root_id, Node::new("Rsa", "Node")).unwrap();
    tree.attach_script(
        node,
        Box::new(GDScriptNodeInstance::from_source(RSA_GD, node).unwrap()),
    );
    LifecycleManager::enter_tree(&mut tree, node);
    let mut ml = MainLoop::new(tree);
    ml.step(1.0 / 60.0);

    // The one clause that CHANGED: a real key object, not `null`.
    let key = script_var(&ml, node, "key");
    assert!(
        matches!(key, Some(Variant::ObjectId(_))),
        "generate_rsa must hand back a CryptoKey now that stage 2 is \
         implemented, got {key:?}"
    );
    assert_eq!(
        script_var(&ml, node, "signature"),
        Some(Variant::PackedByteArray(Vec::new())),
        "sign() with `null` where the key belongs must return no bytes rather \
         than a plausible-looking signature"
    );
    assert_eq!(
        script_var(&ml, node, "verified"),
        Some(Variant::Bool(false)),
        "verify() with `null` where the key belongs must be false — a path that \
         returned true would make every signature check pass"
    );
    assert_eq!(
        script_var(&ml, node, "wrong_hash"),
        Some(Variant::PackedByteArray(Vec::new())),
        "signing an MD5 digest must refuse: a PKCS#1 v1.5 signature carries the \
         digest's own ASN.1 prefix, so signing one hash under another's prefix \
         produces a signature nothing else verifies"
    );
}

/// **pat-s3rrv.195 acceptance.** Every clause of the bead's "Prove it", from a
/// scene-attached script: a 2048-bit RSA key signs and verifies a SHA-256
/// digest, a tampered digest does not verify, the key and a self-signed
/// certificate round-trip through `save_to_string`/`load_from_string`,
/// `HMACContext` agrees with `Crypto.hmac_digest`, and two CSPRNG draws differ.
///
/// ⚠️ ONE SCRIPT, ONE KEYGEN. `generate_rsa(2048)` is a prime search; splitting
/// these clauses across tests would pay for it several times over, and they
/// genuinely depend on each other — the signature the reloaded PEM has to
/// verify is the one this key made.
#[test]
fn crypto_rsa_x509_and_csprng_reachable_from_script() {
    const ACCEPT_GD: &str = "\
extends Node
var digest = null
var signature = null
var verified = false
var tampered_verified = true
var key_pem = ''
var reloaded_verified = false
var reloaded_is_public_only = true
var pub_pem = ''
var pub_only_flag = false
var pub_verified = false
var decrypted = null
var cert_pem = ''
var cert_reload = -1
var cert_pem_again = ''
var hmac_streamed = null
var hmac_oneshot = null
var random_a = null
var random_b = null

func _ready():
    var c = Crypto.new()
    var k = c.generate_rsa(2048)

    var h = HashingContext.new()
    h.start(2)
    h.update('patina parity')
    self.digest = h.finish()

    self.signature = c.sign(2, self.digest, k)
    self.verified = c.verify(2, self.digest, self.signature, k)

    var h2 = HashingContext.new()
    h2.start(2)
    h2.update('patina parityX')
    self.tampered_verified = c.verify(2, h2.finish(), self.signature, k)

    self.key_pem = k.save_to_string()
    var k2 = CryptoKey.new()
    k2.load_from_string(self.key_pem)
    self.reloaded_verified = c.verify(2, self.digest, self.signature, k2)
    self.reloaded_is_public_only = k2.is_public_only()

    self.pub_pem = k.save_to_string(true)
    var kpub = CryptoKey.new()
    kpub.load_from_string(self.pub_pem, true)
    self.pub_only_flag = kpub.is_public_only()
    self.pub_verified = c.verify(2, self.digest, self.signature, kpub)

    self.decrypted = c.decrypt(k, c.encrypt(k, 'secret payload'))

    var cert = c.generate_self_signed_certificate(k, 'CN=patina,O=patina engine,C=IT', '20140101000000', '20340101000000')
    self.cert_pem = cert.save_to_string()
    var cert2 = X509Certificate.new()
    self.cert_reload = cert2.load_from_string(self.cert_pem)
    self.cert_pem_again = cert2.save_to_string()

    var hm = HMACContext.new()
    hm.start(2, 'key')
    hm.update('ms')
    hm.update('g')
    self.hmac_streamed = hm.finish()
    self.hmac_oneshot = c.hmac_digest(2, 'key', 'msg')

    self.random_a = c.generate_random_bytes(32)
    self.random_b = c.generate_random_bytes(32)
";
    let mut tree = SceneTree::new();
    let root_id = tree.root_id();
    let node = tree
        .add_child(root_id, Node::new("RsaAccept", "Node"))
        .unwrap();
    tree.attach_script(
        node,
        Box::new(GDScriptNodeInstance::from_source(ACCEPT_GD, node).unwrap()),
    );
    LifecycleManager::enter_tree(&mut tree, node);
    let mut ml = MainLoop::new(tree);
    ml.step(1.0 / 60.0);

    // -- sign / verify over a SHA-256 digest ---------------------------------
    assert_eq!(
        bytes_var(&ml, node, "digest").len(),
        32,
        "HashingContext must have produced a SHA-256 digest for the key to sign"
    );
    let signature = bytes_var(&ml, node, "signature");
    assert_eq!(
        signature.len(),
        256,
        "a PKCS#1 v1.5 signature is exactly the modulus width — 256 bytes for \
         RSA-2048. An empty array here means `sign` still refused"
    );
    assert_eq!(
        script_var(&ml, node, "verified"),
        Some(Variant::Bool(true)),
        "the signature this key just made must verify under that key"
    );
    assert_eq!(
        script_var(&ml, node, "tampered_verified"),
        Some(Variant::Bool(false)),
        "a DIFFERENT digest must not verify — without this clause a `verify` \
         that ignored its input would pass the assertion above"
    );

    // -- CryptoKey PEM round trip --------------------------------------------
    let key_pem = string_var(&ml, node, "key_pem");
    assert!(
        key_pem.contains("-----BEGIN RSA PRIVATE KEY-----"),
        "save_to_string() must emit the PKCS#1 label mbedTLS writes, got {key_pem:?}"
    );
    assert_eq!(
        script_var(&ml, node, "reloaded_verified"),
        Some(Variant::Bool(true)),
        "a key reloaded from its own PEM must verify the same signature"
    );
    assert_eq!(
        script_var(&ml, node, "reloaded_is_public_only"),
        Some(Variant::Bool(false)),
        "that PEM carried the private half, so is_public_only() must be false"
    );

    let pub_pem = string_var(&ml, node, "pub_pem");
    assert!(
        pub_pem.contains("-----BEGIN PUBLIC KEY-----"),
        "save_to_string(true) must emit a SubjectPublicKeyInfo, got {pub_pem:?}"
    );
    assert!(
        !pub_pem.contains("PRIVATE"),
        "a public_only export must not contain the private key: {pub_pem:?}"
    );
    assert_eq!(
        script_var(&ml, node, "pub_only_flag"),
        Some(Variant::Bool(true)),
        "a key loaded public_only must report is_public_only() == true"
    );
    assert_eq!(
        script_var(&ml, node, "pub_verified"),
        Some(Variant::Bool(true)),
        "verification needs only the public half"
    );

    // -- encrypt / decrypt ---------------------------------------------------
    assert_eq!(
        String::from_utf8_lossy(&bytes_var(&ml, node, "decrypted")),
        "secret payload",
        "RSA encrypt -> decrypt must restore the plaintext"
    );

    // -- self-signed X509 ----------------------------------------------------
    let cert_pem = string_var(&ml, node, "cert_pem");
    assert!(
        cert_pem.contains("-----BEGIN CERTIFICATE-----"),
        "generate_self_signed_certificate must produce a PEM certificate, got {cert_pem:?}"
    );
    assert_eq!(
        script_var(&ml, node, "cert_reload"),
        Some(Variant::Int(0)),
        "X509Certificate.load_from_string must return OK"
    );
    assert_eq!(
        string_var(&ml, node, "cert_pem_again"),
        cert_pem,
        "the certificate must survive save_to_string -> load_from_string -> \
         save_to_string unchanged"
    );

    // -- HMACContext agrees with the one-shot digest -------------------------
    let streamed = bytes_var(&ml, node, "hmac_streamed");
    assert!(!streamed.is_empty(), "an empty digest would pass vacuously");
    assert_eq!(
        streamed,
        bytes_var(&ml, node, "hmac_oneshot"),
        "HMACContext.start/update/update/finish must equal Crypto.hmac_digest"
    );

    // -- the CSPRNG ----------------------------------------------------------
    let a = bytes_var(&ml, node, "random_a");
    let b = bytes_var(&ml, node, "random_b");
    assert_eq!(
        a.len(),
        32,
        "generate_random_bytes(32) must return 32 bytes"
    );
    assert_eq!(
        b.len(),
        32,
        "generate_random_bytes(32) must return 32 bytes"
    );
    assert_ne!(a, b, "two draws must differ");
}

/// The other half of the bead's CSPRNG clause: the draws "do not repeat across
/// a process restarted with the same wall clock".
///
/// 🔑 A RESTART CANNOT BE SPAWNED FROM A TEST, but the property it is really
/// asking about can be measured directly. `gdcore::rng` is seeded from
/// `SystemTime`, so "the same wall clock" is exactly "the same seed" — and this
/// pins that re-seeding that generator to a fixed value reproduces ITS stream
/// while leaving `generate_random_bytes` unaffected. If `generate_random_bytes`
/// were still drawing from `rng::randi()`, the second half would come back
/// equal.
///
/// ⚠️ THIS TEST WRITES PROCESS-GLOBAL STATE (`rng::seed`). It is the only test
/// in this file that touches the global RNG and nothing else here reads it, but
/// a test added later that depends on `gdcore::rng` must not assume a pristine
/// seed under a runner that shares one process across test threads. The
/// per-test-process runner this repo uses is unaffected.
#[test]
fn the_csprng_does_not_follow_the_wall_clock_seeded_xorshift() {
    fn xorshift_draw(seed: u64, n: usize) -> Vec<u8> {
        gdcore::rng::seed(seed);
        (0..n).map(|_| gdcore::rng::randi() as u8).collect()
    }

    // Control: the legacy RNG IS reproducible from its seed, which is what
    // makes a wall-clock seed a vulnerability in the first place. Without this
    // clause, a `seed()` that silently did nothing would make the real
    // assertion below pass for the wrong reason.
    assert_eq!(
        xorshift_draw(0x5EED, 32),
        xorshift_draw(0x5EED, 32),
        "the legacy RNG must be reproducible from its seed, or this test is \
         measuring nothing"
    );

    gdcore::rng::seed(0x5EED);
    let first = gdcore::crypto::generate_random_bytes(32);
    gdcore::rng::seed(0x5EED);
    let second = gdcore::crypto::generate_random_bytes(32);
    assert_ne!(
        first, second,
        "generate_random_bytes must not repeat when the legacy RNG is re-seeded \
         identically — equal draws here mean it is still reading rng::randi()"
    );
}
