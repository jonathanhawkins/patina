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
//! ⚠️ NOT delivered, and reported as such: RSA keygen / sign / verify / encrypt /
//! decrypt and self-signed X509 (stage 2), `CryptoKey`/`X509Certificate` save and
//! load, and `AESContext.get_iv_state()`. Those are registered but answer
//! null/empty/false — deliberately, so a script cannot build a "signature" that
//! verifies nothing. `Crypto.generate_random_bytes` is **not** a CSPRNG; see the
//! doc comment on `gdcore::crypto::generate_random_bytes`.

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

/// The stage-2 surface must fail visibly rather than return something that looks
/// like a key or a signature.
#[test]
fn the_rsa_half_reports_unavailable_rather_than_faking_it() {
    const RSA_GD: &str = "\
extends Node
var key = null
var signature = null
var verified = true

func _ready():
    var c = Crypto.new()
    self.key = c.generate_rsa(2048)
    self.signature = c.sign(2, 'digest', null)
    self.verified = c.verify(2, 'digest', 'sig', null)
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

    assert_eq!(
        script_var(&ml, node, "key"),
        Some(Variant::Nil),
        "generate_rsa must return null until stage 2, not a fake key"
    );
    assert_eq!(
        script_var(&ml, node, "signature"),
        Some(Variant::PackedByteArray(Vec::new())),
        "sign must return no bytes rather than a plausible-looking signature"
    );
    assert_eq!(
        script_var(&ml, node, "verified"),
        Some(Variant::Bool(false)),
        "verify must be false — a stub that returned true would make every \
         signature check pass"
    );
}
