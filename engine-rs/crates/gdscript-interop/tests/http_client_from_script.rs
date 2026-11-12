//! pat-s3rrv.78 — `HTTPClient` reachable from GDScript.
//!
//! Godot ground truth: `HTTPClient` is a `RefCounted`. A script writes
//! `var c = HTTPClient.new()`, then `connect_to_host` / `request` / `poll` /
//! `get_status` / `read_response_body_chunk`.
//!
//! 🔑 The class was **registered in ClassDB with ten methods and no dispatch
//! anywhere** — no `HTTPClient.new()` arm, no handle table, no routing. So
//! `has_method` answered `true` for all ten while every call returned `Nil`, and
//! the 639-line `gdplatform::http_client` behind it had no consumer in the scene
//! tree, the interpreter, or the runner. These tests are what make it reachable.
//!
//! ⚠️ Binary named `http_client_from_script` rather than something fuller: a
//! peer note in `tests/backtrace.rs` reports that an over-long `--test` target
//! can make a verify command resolve to nothing. I could not reproduce that in
//! `verifier.rs` (its only truncation is on output tails, at :624-633) and my
//! own 70+ char gates have linked fine — but a short name costs nothing.

use gdscript_interop::interpreter::Interpreter;
use gdvariant::Variant;

/// Runs `check()` on a script and returns its value.
///
/// ⚠️ ClassDB is seeded first: a bare interpreter starts with an **empty**
/// registry, so `ClassDB.class_exists("HTTPClient")` would answer false however
/// the class is registered. Registration is `class_exists`-guarded and therefore
/// idempotent.
///
/// ⚠️ `register_networking_classes`, NOT `register_3d_classes` — HTTPClient is
/// registered at `class_db.rs:24491`, inside the networking block. The
/// neighbouring `tests/backtrace.rs` seeds with `register_3d_classes` because
/// *its* class lives there; copying that call without checking would leave this
/// registry empty of HTTPClient and make `class_exists` answer false.
fn call_check(src: &str) -> Variant {
    gdobject::class_db::register_networking_classes();
    let mut interp = Interpreter::new();
    let class_def = interp.run_class(src).expect("script parses");
    let mut inst = interp
        .instantiate_class(&class_def)
        .expect("class instantiates");
    interp
        .call_instance_method(&mut inst, "check", &[])
        .unwrap_or_else(|e| panic!("check() failed: {e:?}"))
}

fn int(v: Variant) -> i64 {
    match v {
        Variant::Int(i) => i,
        other => panic!("expected an Int, got {other:?}"),
    }
}

// ============================================================ the acceptance

/// **The acceptance.** `HTTPClient.new()` yields an object a script can call
/// methods on, and those calls return real values rather than `Nil`.
///
/// ⚠️ Asserting the TYPE matters as much as the value. Before this increment the
/// call returned `Variant::Nil`, and in GDScript `Nil == 0` is false — so a
/// script's `if c.get_status() == HTTPClient.STATUS_DISCONNECTED:` was silently
/// false forever. A value-only check would not distinguish the two.
#[test]
fn a_script_can_construct_an_http_client_and_call_it() {
    let status = call_check(
        "extends Node\nfunc check():\n    var c = HTTPClient.new()\n    return c.get_status()\n",
    );
    assert_eq!(
        status,
        Variant::Int(0),
        "a fresh client reports STATUS_DISCONNECTED as an Int, not Nil"
    );
}

#[test]
fn the_class_is_registered_and_new_returns_an_object() {
    let v = call_check(
        "extends Node\nfunc check():\n    return ClassDB.class_exists(\"HTTPClient\")\n",
    );
    assert_eq!(v, Variant::Bool(true));

    let v = call_check("extends Node\nfunc check():\n    return HTTPClient.new()\n");
    assert!(
        matches!(v, Variant::ObjectId(_)),
        "HTTPClient.new() must yield an object handle, got {v:?}"
    );
}

/// Two `new()` calls are two independent clients.
#[test]
fn each_new_call_is_its_own_client() {
    let v = call_check(
        "extends Node\nfunc check():\n    var a = HTTPClient.new()\n    var b = HTTPClient.new()\n    a.set_read_chunk_size(7)\n    return b.get_read_chunk_size()\n",
    );
    assert_eq!(
        int(v),
        65536,
        "one client's chunk size must not leak into another's"
    );
}

// -------------------------------------------------------------- guard rails

/// ⚠️ `poll()` reports OK, so a Godot-shaped `while c.poll() == OK:` loop does
/// not see a false failure on a transport that has nothing to advance.
#[test]
fn poll_reports_ok_rather_than_an_error() {
    let v = call_check(
        "extends Node\nfunc check():\n    var c = HTTPClient.new()\n    return c.poll()\n",
    );
    assert_eq!(int(v), 0, "OK");
}

/// Requesting before connecting is ERR_UNCONFIGURED (3), not a network error.
#[test]
fn a_request_before_connecting_is_unconfigured() {
    let v = call_check(
        "extends Node\nfunc check():\n    var c = HTTPClient.new()\n    return c.request(0, \"/x\", [], \"\")\n",
    );
    assert_eq!(int(v), 3, "ERR_UNCONFIGURED");
}

/// 🔑 A TLS request is refused as ERR_UNAVAILABLE (2), not ERR_CONNECTION_ERROR.
///
/// ⚠️ Nothing went wrong on the wire — there is no TLS backend. A connection
/// error would send a script debugging its network.
#[test]
fn a_tls_connect_is_unavailable_not_a_connection_error() {
    let v = call_check(
        "extends Node\nfunc check():\n    var c = HTTPClient.new()\n    return c.connect_to_host(\"example.invalid\", 443, {})\n",
    );
    assert_eq!(int(v), 2, "ERR_UNAVAILABLE");
}

/// …and a plain connect to the same unresolvable host is ERR_CANT_RESOLVE (26),
/// which is the control proving the TLS arm above is not just failing early.
#[test]
fn a_plain_connect_to_an_unresolvable_host_reports_cant_resolve() {
    let v = call_check(
        "extends Node\nfunc check():\n    var c = HTTPClient.new()\n    return c.connect_to_host(\"origin.invalid\", 80, null)\n",
    );
    assert_eq!(int(v), 26, "ERR_CANT_RESOLVE");
}

/// 🔑 The method ordinals are Godot's, and PATCH is 8 — **not** 6, which is
/// TRACE.
///
/// ⚠️ OPTIONS/TRACE/CONNECT have no transport variant and must report
/// ERR_UNAVAILABLE rather than silently downgrading to GET, which would look
/// like a successful request while doing something else entirely. Here the
/// client is unconnected, so a *recognised* method reports ERR_UNCONFIGURED (3)
/// and an unsupported one reports ERR_UNAVAILABLE (2) — the two are
/// distinguishable, which is what makes this test meaningful.
#[test]
fn an_unsupported_method_ordinal_is_refused_before_the_connection_check() {
    let trace = call_check(
        "extends Node\nfunc check():\n    var c = HTTPClient.new()\n    return c.request(6, \"/x\", [], \"\")\n",
    );
    assert_eq!(int(trace), 2, "ordinal 6 is TRACE — ERR_UNAVAILABLE");

    let patch = call_check(
        "extends Node\nfunc check():\n    var c = HTTPClient.new()\n    return c.request(8, \"/x\", [], \"\")\n",
    );
    assert_eq!(
        int(patch),
        3,
        "ordinal 8 is PATCH — a real method, so it reaches the ERR_UNCONFIGURED guard"
    );
}

// ------------------------------------------------------------- readout API

#[test]
fn a_fresh_client_reports_empty_response_state() {
    let v = call_check(
        "extends Node\nfunc check():\n    var c = HTTPClient.new()\n    return c.has_response()\n",
    );
    assert_eq!(v, Variant::Bool(false));

    let v = call_check(
        "extends Node\nfunc check():\n    var c = HTTPClient.new()\n    return c.get_response_body_length()\n",
    );
    assert_eq!(int(v), 0);

    let v = call_check(
        "extends Node\nfunc check():\n    var c = HTTPClient.new()\n    return c.read_response_body_chunk()\n",
    );
    assert!(
        matches!(&v, Variant::PackedByteArray(b) if b.is_empty()),
        "an empty chunk must be an empty PackedByteArray, not Nil — a read loop \
         tests emptiness, and Nil would not compare equal to []; got {v:?}"
    );

    let v = call_check(
        "extends Node\nfunc check():\n    var c = HTTPClient.new()\n    return c.get_response_headers()\n",
    );
    assert!(
        matches!(&v, Variant::PackedStringArray(h) if h.is_empty()),
        "headers must be a PackedStringArray, got {v:?}"
    );
}

/// `set_read_chunk_size` round-trips, and a zero is ignored rather than stored.
///
/// ⚠️ Storing zero would make every read return empty, which a read loop cannot
/// distinguish from "done" — silently truncating every body to nothing.
#[test]
fn the_read_chunk_size_round_trips_and_rejects_zero() {
    let v = call_check(
        "extends Node\nfunc check():\n    var c = HTTPClient.new()\n    c.set_read_chunk_size(4096)\n    return c.get_read_chunk_size()\n",
    );
    assert_eq!(int(v), 4096);

    let v = call_check(
        "extends Node\nfunc check():\n    var c = HTTPClient.new()\n    c.set_read_chunk_size(0)\n    return c.get_read_chunk_size()\n",
    );
    assert_eq!(int(v), 65536, "0 must be ignored, leaving Godot's default");
}

#[test]
fn close_is_callable_and_leaves_the_client_disconnected() {
    let v = call_check(
        "extends Node\nfunc check():\n    var c = HTTPClient.new()\n    c.close()\n    return c.get_status()\n",
    );
    assert_eq!(int(v), 0, "STATUS_DISCONNECTED");
}

/// An unknown method still raises, rather than silently answering `Nil`.
///
/// ⚠️ This is the guard on the dispatch's fall-through arm: returning `Nil` for
/// anything unrecognised would recreate exactly the bug this increment fixes.
#[test]
fn an_unknown_method_raises_rather_than_returning_nil() {
    gdobject::class_db::register_networking_classes();
    let mut interp = Interpreter::new();
    let class_def = interp
        .run_class(
            "extends Node\nfunc check():\n    var c = HTTPClient.new()\n    return c.no_such_method()\n",
        )
        .expect("script parses");
    let mut inst = interp
        .instantiate_class(&class_def)
        .expect("class instantiates");
    assert!(
        interp
            .call_instance_method(&mut inst, "check", &[])
            .is_err(),
        "an unrecognised HTTPClient method must raise, not answer Nil"
    );
}
