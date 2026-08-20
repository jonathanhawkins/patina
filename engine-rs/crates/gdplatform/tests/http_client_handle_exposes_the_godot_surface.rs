//! pat-s3rrv.78 — the script-visible `HTTPClient` surface.
//!
//! Godot ground truth: `core/io/http_client.h` / `http_client_tcp.cpp`, and the
//! ClassDB registration this backs (`class_db.rs`, the HTTP block) which
//! advertises connect_to_host, request, poll, get_status, has_response,
//! get_response_code, get_response_headers, get_response_body_length,
//! read_response_body_chunk and close.
//!
//! 🔑 All ten were registered with **no dispatch at all**, so `has_method`
//! answered `true` and every call returned `Nil`. These tests exist because that
//! is invisible from the Rust side — `gdplatform::http_client` was correct the
//! whole time and simply unreachable.
//!
//! ⚠️ The surface is poll-SHAPED but the transport is blocking. Nothing here
//! asserts non-blocking behaviour, because there is none to assert; what is
//! pinned is that every value reported is one Godot really produces and is
//! correct where it is reported.

use std::io::{Read, Write};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream};
use std::time::Duration;

use gdplatform::http_client_handle::{
    is_http_client_handle, new_http_client_handle, release_http_client_handle, with_http_client,
    ScriptHttpClient, DEFAULT_READ_CHUNK_SIZE, ERR_CANT_RESOLVE, ERR_INVALID_PARAMETER,
    ERR_UNAVAILABLE, ERR_UNCONFIGURED, OK,
};
use gdplatform::http_request::HttpMethod;

const BODY: &str = "HelloWorldThisIsABody";

/// A one-shot server that answers `BODY` with a marker header.
fn serve_once() -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("addr");
    std::thread::spawn(move || {
        let Ok((mut sock, _)) = listener.accept() else {
            return;
        };
        let _ = sock.set_read_timeout(Some(Duration::from_secs(10)));
        let mut buf = Vec::new();
        let mut byte = [0u8; 1];
        while sock.read(&mut byte).unwrap_or(0) == 1 {
            buf.push(byte[0]);
            if buf.ends_with(b"\r\n\r\n") {
                break;
            }
        }
        let response = format!(
            "HTTP/1.1 200 OK\r\n\
             X-Patina-Test: marker\r\n\
             Content-Type: text/plain\r\n\
             Content-Length: {}\r\n\
             Connection: close\r\n\r\n{}",
            BODY.len(),
            BODY
        );
        let _ = sock.write_all(response.as_bytes());
        let _ = sock.flush();
        let _ = sock.shutdown(Shutdown::Both);
    });
    addr
}

/// A client that has completed one exchange against a live server.
fn connected_with_response() -> ScriptHttpClient {
    let addr = serve_once();
    let mut client = ScriptHttpClient::new();
    assert_eq!(
        client.connect_to_host(&addr.ip().to_string(), i32::from(addr.port()), false),
        OK,
        "the fixture server must be reachable"
    );
    assert_eq!(
        client.request(HttpMethod::Get, "/thing", &[], &[]),
        OK,
        "the fixture request must succeed"
    );
    client
}

// ------------------------------------------------------------- fresh client

#[test]
fn a_fresh_client_is_disconnected_with_no_response() {
    let client = ScriptHttpClient::new();
    assert_eq!(client.get_status(), 0, "STATUS_DISCONNECTED");
    assert!(!client.has_response());
    assert_eq!(client.get_response_body_length(), 0);
    assert_eq!(
        client.get_read_chunk_size(),
        DEFAULT_READ_CHUNK_SIZE as i64,
        "Godot's default read_chunk_size is 65536"
    );
}

// -------------------------------------------------------------- guard rails

/// ⚠️ TLS is refused as UNAVAILABLE, not as a connection error.
///
/// Nothing went wrong on the wire — the feature is not built. Reporting
/// ERR_CONNECTION_ERROR would send a script debugging its network instead of
/// reading the release notes.
#[test]
fn requesting_tls_is_unavailable_rather_than_a_connection_error() {
    let mut client = ScriptHttpClient::new();
    assert_eq!(
        client.connect_to_host("example.invalid", 443, true),
        ERR_UNAVAILABLE
    );
    assert_eq!(
        client.get_status(),
        0,
        "a refused connect must leave the client DISCONNECTED"
    );
}

#[test]
fn an_empty_host_is_an_invalid_parameter() {
    let mut client = ScriptHttpClient::new();
    assert_eq!(client.connect_to_host("", 80, false), ERR_INVALID_PARAMETER);
}

/// A port that cannot be a TCP port is reported, not truncated into one.
#[test]
fn an_out_of_range_port_is_an_invalid_parameter() {
    let mut client = ScriptHttpClient::new();
    assert_eq!(
        client.connect_to_host("example.invalid", 70_000, false),
        ERR_INVALID_PARAMETER
    );
}

/// ⚠️ Godot's own guard: `request` requires `STATUS_CONNECTED`.
#[test]
fn a_request_before_connecting_is_unconfigured() {
    let mut client = ScriptHttpClient::new();
    assert_eq!(
        client.request(HttpMethod::Get, "/x", &[], &[]),
        ERR_UNCONFIGURED,
        "requesting on a disconnected client must not look like a network failure"
    );
}

#[test]
fn an_unresolvable_host_reports_cant_resolve() {
    let mut client = ScriptHttpClient::new();
    assert_eq!(
        client.connect_to_host("origin.invalid", 80, false),
        ERR_CANT_RESOLVE
    );
}

/// `poll()` reports success rather than an error.
///
/// 🔑 A Godot-shaped `while c.poll() == OK:` loop must not see a false failure
/// just because this transport has nothing to advance.
#[test]
fn poll_is_a_no_op_that_reports_success() {
    let mut client = ScriptHttpClient::new();
    assert_eq!(client.poll(), OK);
    assert_eq!(client.poll(), OK, "and stays OK when called repeatedly");
}

// ---------------------------------------------------------- a real exchange

#[test]
fn a_full_exchange_reports_code_headers_and_body() {
    let client = connected_with_response();

    assert!(client.has_response());
    assert_eq!(client.get_response_code(), 200);
    assert_eq!(client.get_response_body_length(), BODY.len() as i64);

    let headers = client.get_response_headers();
    assert!(
        headers
            .iter()
            .any(|h| h.starts_with("x-patina-test:") || h.starts_with("X-Patina-Test:")),
        "response headers must be delivered as lines, got {headers:?}"
    );

    let dict = client.get_response_headers_as_dictionary();
    assert!(
        dict.keys()
            .any(|k| k.eq_ignore_ascii_case("content-length")),
        "…and as a dictionary, got {dict:?}"
    );
}

#[test]
fn the_body_is_delivered_in_read_chunk_size_slices() {
    let mut client = connected_with_response();
    client.set_read_chunk_size(4);

    let mut collected = Vec::new();
    loop {
        let chunk = client.read_response_body_chunk();
        if chunk.is_empty() {
            break;
        }
        assert!(chunk.len() <= 4, "a chunk must not exceed read_chunk_size");
        collected.extend_from_slice(&chunk);
    }
    assert_eq!(
        String::from_utf8_lossy(&collected),
        BODY,
        "the chunks must reassemble into the whole body, in order"
    );
}

/// A drained body keeps returning empty — that is how a read loop ends.
#[test]
fn a_drained_body_stays_empty() {
    let mut client = connected_with_response();
    let first = client.read_response_body_chunk();
    assert_eq!(
        String::from_utf8_lossy(&first),
        BODY,
        "default chunk takes all"
    );

    assert!(client.read_response_body_chunk().is_empty());
    assert!(
        client.read_response_body_chunk().is_empty(),
        "and again — a loop must terminate, not resume"
    );
}

/// 🔑 `get_response_body_length` is the FULL length and must not shrink.
///
/// ⚠️ Godot reports `Content-Length`, which is a constant for the response. A
/// script sizing a progress bar divides by this every iteration; if it shrank as
/// chunks were consumed, the bar would read 100% from the first chunk onward.
/// Returning "bytes remaining" is the natural-looking implementation and is
/// exactly wrong.
#[test]
fn the_body_length_does_not_shrink_as_chunks_are_read() {
    let mut client = connected_with_response();
    let total = client.get_response_body_length();
    assert_eq!(total, BODY.len() as i64);

    client.set_read_chunk_size(4);
    let _ = client.read_response_body_chunk();
    assert_eq!(
        client.get_response_body_length(),
        total,
        "the denominator must stay constant while the body is consumed"
    );
    let _ = client.read_response_body_chunk();
    assert_eq!(client.get_response_body_length(), total);
}

/// 🔑 A zero chunk size is ignored, not honoured.
///
/// ⚠️ Honouring it makes every read return empty, which a read loop cannot
/// distinguish from "done" — so every body would silently truncate to nothing.
#[test]
fn a_zero_or_negative_read_chunk_size_is_ignored() {
    let mut client = connected_with_response();
    let before = client.get_read_chunk_size();

    client.set_read_chunk_size(0);
    assert_eq!(client.get_read_chunk_size(), before, "0 must not be stored");
    client.set_read_chunk_size(-8);
    assert_eq!(client.get_read_chunk_size(), before, "negatives either");

    // …and the body still reads out in full.
    assert_eq!(
        String::from_utf8_lossy(&client.read_response_body_chunk()),
        BODY
    );
}

#[test]
fn close_clears_the_response() {
    let mut client = connected_with_response();
    assert!(client.has_response());

    client.close();
    assert!(!client.has_response(), "a closed client has no response");
    assert_eq!(client.get_response_body_length(), 0);
    assert_eq!(client.get_status(), 0, "STATUS_DISCONNECTED");
    assert!(client.read_response_body_chunk().is_empty());
}

/// `blocking_mode_enabled` round-trips, which is all Godot's property contract
/// requires of it here.
#[test]
fn blocking_mode_round_trips() {
    let mut client = ScriptHttpClient::new();
    assert!(!client.is_blocking_mode_enabled());
    client.set_blocking_mode(true);
    assert!(client.is_blocking_mode_enabled());
    client.set_blocking_mode(false);
    assert!(!client.is_blocking_mode_enabled());
}

// ------------------------------------------------------------ handle table

#[test]
fn a_handle_round_trips_and_releases() {
    let id = new_http_client_handle();
    assert!(is_http_client_handle(id));

    let status = with_http_client(id, |c| c.get_status());
    assert_eq!(status, Some(0), "a live handle reaches its client");

    release_http_client_handle(id);
    assert!(
        with_http_client(id, |c| c.get_status()).is_none(),
        "a released handle must not resolve"
    );
    release_http_client_handle(id);
    assert!(
        with_http_client(id, |c| c.get_status()).is_none(),
        "and release is idempotent"
    );
}

#[test]
fn two_handles_are_independent() {
    let a = new_http_client_handle();
    let b = new_http_client_handle();
    assert_ne!(a.raw(), b.raw(), "each new() gets its own id");

    with_http_client(a, |c| c.set_read_chunk_size(7));
    assert_eq!(with_http_client(a, |c| c.get_read_chunk_size()), Some(7));
    assert_eq!(
        with_http_client(b, |c| c.get_read_chunk_size()),
        Some(DEFAULT_READ_CHUNK_SIZE as i64),
        "one client's settings must not leak into another's"
    );

    release_http_client_handle(a);
    release_http_client_handle(b);
}

/// 🔑 The tag byte is a workspace-wide namespace.
///
/// ⚠️ If another module's tag collided, `is_http_client_handle` would claim its
/// ids and the symptom would be a `MethodNotFound` naming the wrong class.
#[test]
fn a_foreign_tag_is_not_an_http_client_handle() {
    // 0x57 is WebSocket's tag; 0x5C PacketPeerStream's.
    for foreign in [0x5700_0000_0000_0001u64, 0x5C00_0000_0000_0001] {
        assert!(
            !is_http_client_handle(gdcore::id::ObjectId::from_raw(foreign)),
            "0x{foreign:016X} belongs to another module"
        );
    }
    assert!(is_http_client_handle(new_http_client_handle()));
}
