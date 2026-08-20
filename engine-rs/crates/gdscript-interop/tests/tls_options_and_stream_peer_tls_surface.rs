//! pat-s3rrv.237 — `TLSOptions` and `StreamPeerTLS` are reachable from a script.
//!
//! # Two classes, opposite web stories
//!
//! The bead's own scope note is the design, and it is worth restating because
//! the two halves must NOT be implemented the same way:
//!
//! * **`TLSOptions` is pure configuration.** It is what
//!   `HTTPClient.connect_to_host`, `HTTPRequest.set_tls_options` and
//!   `WebSocketPeer.connect_to_url` accept, and a browser performs the handshake
//!   itself through fetch/WebSocket. So it must genuinely work and carry real
//!   values — stubbing it would break TLS config on the one platform where TLS
//!   actually functions.
//! * **`StreamPeerTLS` wraps a raw TCP socket**, which a browser does not have.
//!   Godot's own web build stubs every NetSocket operation to `ERR_UNAVAILABLE`
//!   (`upstream/godot/platform/web/net_socket_web.h`). Parity is therefore a
//!   class that EXISTS and returns that error — not an absent class raising
//!   "unknown class", and emphatically not a fake success.
//!
//! ⚠️ Godot's `TLSOptions` has **no constructor**: the three statics are the
//! only way to build one, and `TLSOptions.new()` is an error. Which static was
//! used stays observable through `is_server()` / `is_unsafe_client()`, so those
//! are asserted rather than assumed from the fields.
//!
//! Constants are transcribed, not derived: `ERR_UNAVAILABLE = 2` from
//! `global_constants.rs`, and `STATUS_DISCONNECTED = 0` / `STATUS_ERROR = 3`
//! from `StreamPeerTLS.xml`.

use gdscript_interop::interpreter::Interpreter;
use gdvariant::Variant;

/// Runs `method` on a fresh instance of `src` through the real VM dispatch.
fn call_method(src: &str, method: &str) -> Variant {
    let mut interp = Interpreter::new();
    let class_def = interp.run_class(src).expect("script parses");
    let mut inst = interp
        .instantiate_class(&class_def)
        .expect("class instantiates");
    interp
        .call_instance_method(&mut inst, method, &[])
        .unwrap_or_else(|e| panic!("call {method}() failed: {e:?}"))
}

const SRC: &str = "\
extends Node

func client_is_tls_options():
    var o = TLSOptions.client()
    return o.is_class(\"TLSOptions\")

func client_is_refcounted():
    var o = TLSOptions.client()
    return o.is_class(\"RefCounted\")

func client_is_not_a_stream():
    var o = TLSOptions.client()
    return o.is_class(\"StreamPeer\")

func server_is_tls_options():
    var o = TLSOptions.server(\"key\", \"cert\")
    return o.is_class(\"TLSOptions\")

func server_reports_server():
    var o = TLSOptions.server(\"key\", \"cert\")
    return o.is_server()

func client_reports_server():
    var o = TLSOptions.client()
    return o.is_server()

func unsafe_client_reports_unsafe():
    var o = TLSOptions.client_unsafe()
    return o.is_unsafe_client()

func plain_client_reports_unsafe():
    var o = TLSOptions.client()
    return o.is_unsafe_client()

func common_name_round_trips():
    var o = TLSOptions.client(null, \"example.org\")
    return o.get_common_name_override()

func server_key_round_trips():
    var o = TLSOptions.server(\"KEYDATA\", \"CERTDATA\")
    return o.get_private_key()

func server_cert_round_trips():
    var o = TLSOptions.server(\"KEYDATA\", \"CERTDATA\")
    return o.get_own_certificate()

func http_client_accepts_options():
    var c = HTTPClient.new()
    var o = TLSOptions.client()
    return c.connect_to_host(\"example.org\", 443, o)

func http_client_without_options():
    var c = HTTPClient.new()
    return c.connect_to_host(\"example.org\", 80)

func stream_peer_tls_connect():
    var s = StreamPeerTLS.new()
    return s.connect_to_stream(null, \"example.org\", null)

func stream_peer_tls_status_before():
    var s = StreamPeerTLS.new()
    return s.get_status()

func stream_peer_tls_status_after():
    var s = StreamPeerTLS.new()
    s.connect_to_stream(null, \"example.org\", null)
    return s.get_status()

func stream_peer_tls_is_class():
    var s = StreamPeerTLS.new()
    return s.is_class(\"StreamPeer\")

func stream_peer_tls_accept():
    var s = StreamPeerTLS.new()
    return s.accept_stream(null, null)
";

// ===========================================================================
// The acceptance test
// ===========================================================================

/// **The acceptance test.** A script builds both flavours of `TLSOptions`, they
/// report `is_class("TLSOptions")`, an `HTTPClient` accepts one, and
/// `StreamPeerTLS.new().connect_to_stream(...)` answers `ERR_UNAVAILABLE` with
/// `get_status()` reporting the error — rather than raising unknown-class.
#[test]
fn tls_options_and_stream_peer_tls_surface() {
    // -- TLSOptions is a real object -----------------------------------------
    assert_eq!(
        call_method(SRC, "client_is_tls_options"),
        Variant::Bool(true),
        "TLSOptions.client() must produce an object reporting its own class"
    );
    assert_eq!(
        call_method(SRC, "server_is_tls_options"),
        Variant::Bool(true),
        "TLSOptions.server(key, cert) must too"
    );
    // The chain, not just the leaf — Godot's `is_class` answers for every
    // ancestor, and a leaf-only comparison is the easy way to get this wrong.
    assert_eq!(
        call_method(SRC, "client_is_refcounted"),
        Variant::Bool(true),
        "TLSOptions extends RefCounted, so is_class must say so"
    );
    // The control for the two above: `is_class` must not say yes to anything.
    assert_eq!(
        call_method(SRC, "client_is_not_a_stream"),
        Variant::Bool(false),
        "TLSOptions is not a StreamPeer — without this, an is_class that \
         returned true unconditionally would pass every assertion here"
    );

    // -- which static built it stays observable ------------------------------
    assert_eq!(
        call_method(SRC, "server_reports_server"),
        Variant::Bool(true),
        "server() must be distinguishable from client()"
    );
    assert_eq!(
        call_method(SRC, "client_reports_server"),
        Variant::Bool(false),
        "…and client() must not claim to be a server"
    );
    assert_eq!(
        call_method(SRC, "unsafe_client_reports_unsafe"),
        Variant::Bool(true),
        "client_unsafe() must report itself unsafe — this is a security-visible \
         flag, so a script has to be able to tell"
    );
    assert_eq!(
        call_method(SRC, "plain_client_reports_unsafe"),
        Variant::Bool(false),
        "…and a plain client() must never read as unsafe"
    );

    // -- the configuration actually survives ---------------------------------
    // Pure config that discards its arguments would satisfy every `is_class`
    // assertion above while being useless.
    assert_eq!(
        call_method(SRC, "common_name_round_trips"),
        Variant::String("example.org".into()),
        "the common-name override must survive into the object"
    );
    assert_eq!(
        call_method(SRC, "server_key_round_trips"),
        Variant::String("KEYDATA".into()),
        "the private key must survive"
    );
    // Distinct values for key and cert, so a constructor that stored one into
    // both slots fails here rather than passing.
    assert_eq!(
        call_method(SRC, "server_cert_round_trips"),
        Variant::String("CERTDATA".into()),
        "the certificate must survive, and must not be the key"
    );

    // -- it is accepted where Godot accepts one ------------------------------
    // Accepted as an ARGUMENT — the call resolves and returns a code rather
    // than raising on an unknown type. The code itself is `ERR_UNAVAILABLE`,
    // because passing TLSOptions means "use TLS" and there is no native TLS
    // backend; `http_client_handle.rs` refuses honestly rather than opening a
    // plaintext socket to an https port, which would look like it worked and
    // then serve garbage. That refusal is pre-existing behaviour, not this
    // bead's.
    assert_eq!(
        call_method(SRC, "http_client_accepts_options"),
        Variant::Int(2),
        "HTTPClient.connect_to_host must take a TLSOptions third argument and \
         report the honest ERR_UNAVAILABLE"
    );
    // The pair is what proves the argument is READ rather than ignored: the
    // same call without it connects. A TLSOptions silently dropped on the floor
    // would return OK here too.
    assert_eq!(
        call_method(SRC, "http_client_without_options"),
        Variant::Int(0),
        "…while the same call with no TLSOptions connects, so the argument is \
         doing something rather than being discarded"
    );

    // -- StreamPeerTLS: present, and honestly unavailable ---------------------
    assert_eq!(
        call_method(SRC, "stream_peer_tls_connect"),
        Variant::Int(2),
        "connect_to_stream must return ERR_UNAVAILABLE (2), which is what \
         Godot's own web build returns"
    );
    assert_eq!(
        call_method(SRC, "stream_peer_tls_accept"),
        Variant::Int(2),
        "accept_stream is unavailable for the same reason"
    );
    // Before/after, so the status is a real transition rather than a constant.
    assert_eq!(
        call_method(SRC, "stream_peer_tls_status_before"),
        Variant::Int(0),
        "a fresh peer is STATUS_DISCONNECTED — an implementation hardcoded to \
         STATUS_ERROR would pass the next assertion and lose the distinction \
         between never-used and failed"
    );
    assert_eq!(
        call_method(SRC, "stream_peer_tls_status_after"),
        Variant::Int(3),
        "after a failed connect the peer reports STATUS_ERROR"
    );
    assert_eq!(
        call_method(SRC, "stream_peer_tls_is_class"),
        Variant::Bool(true),
        "StreamPeerTLS extends StreamPeer"
    );
}

/// ClassDB must agree with the dispatch, or `ClassDB.class_exists` and the
/// inspector deny classes a script can plainly construct and call.
///
/// ⚠️ `TLSOptions` is registered **abstract on purpose**: Godot's class is not
/// instantiable and the three statics are the only way to build one. A script
/// writing `TLSOptions.new()` must be refused, not handed an empty object that
/// no static ever configured.
#[test]
fn tls_options_and_stream_peer_tls_surface_is_registered() {
    use gdobject::class_db::{class_exists, get_class_info, is_parent_class, register_all_classes};

    register_all_classes();

    for (class, parent) in [
        ("StreamPeerTLS", "StreamPeer"),
        ("TLSOptions", "RefCounted"),
    ] {
        assert!(class_exists(class), "{class} must be registered");
        let info = get_class_info(class).expect("registered class has info");
        assert_eq!(
            info.parent_class, parent,
            "{class} must inherit {parent}, per its doc XML"
        );
        // The chain must WALK, not merely store a name — the parent has to be a
        // class ClassDB knows. Both parents are long-registered, but asserting
        // it is what would catch a typo in the parent string.
        assert!(
            is_parent_class(class, parent),
            "{class}'s inheritance chain must actually resolve {parent}"
        );
    }

    let opts = get_class_info("TLSOptions").expect("registered");
    for name in [
        "client",
        "client_unsafe",
        "server",
        "is_server",
        "is_unsafe_client",
        "get_common_name_override",
        "get_own_certificate",
        "get_private_key",
        "get_trusted_ca_chain",
    ] {
        assert!(
            opts.methods.iter().any(|m| m.name == name),
            "TLSOptions must declare {name}"
        );
    }
    // The three constructors are static in Godot; a script calls them on the
    // class, never on an instance.
    for name in ["client", "client_unsafe", "server"] {
        let m = opts
            .methods
            .iter()
            .find(|m| m.name == name)
            .expect("declared above");
        assert!(m.is_static, "TLSOptions.{name} is a static constructor");
    }

    let tls = get_class_info("StreamPeerTLS").expect("registered");
    for name in [
        "accept_stream",
        "connect_to_stream",
        "disconnect_from_stream",
        "get_status",
        "get_stream",
        "poll",
    ] {
        assert!(
            tls.methods.iter().any(|m| m.name == name),
            "StreamPeerTLS must declare {name}"
        );
    }
    // The control: neither class reports a method it never declared.
    assert!(
        !opts.methods.iter().any(|m| m.name == "_no_such_method")
            && !tls.methods.iter().any(|m| m.name == "_no_such_method"),
        "a method lookup that says yes to anything would satisfy every \
         assertion above"
    );
}
