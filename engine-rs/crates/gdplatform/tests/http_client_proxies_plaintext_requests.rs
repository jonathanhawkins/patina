//! pat-s3rrv.78 — `set_http_proxy` / `set_https_proxy` on the native transport.
//!
//! Godot ground truth: `core/io/http_client_tcp.cpp` —
//! `set_http_proxy` (:770-778), `set_https_proxy` (:780-788), the proxy
//! selection in `connect_to_host` (:76-86), the absolute request-target
//! (:162-164), and the `Host:` header (:186-190).
//!
//! 🔑 Every proxied test names the origin `origin.invalid` — an RFC 2606
//! reserved label that cannot resolve. If the proxy were bypassed the call
//! fails with `CantResolve`, so none of these can pass by accidentally
//! connecting straight to the origin.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream};
use std::sync::mpsc::Receiver;
use std::time::Duration;

use gdplatform::http_client::HttpClient;
use gdplatform::http_request::{HttpMethod, HttpRequestConfig, HttpResultCode};

// --------------------------------------------------------------- fake proxy

/// A stand-in proxy (or plain origin server): answers `responses` in order and
/// reports each request head it saw.
struct FakeServer {
    addr: SocketAddr,
    heads: Receiver<String>,
}

impl FakeServer {
    /// The head of the next request this server received.
    fn next_head(&self) -> String {
        self.heads
            .recv_timeout(Duration::from_secs(10))
            .expect("the server received a request")
    }

    /// The first line of the next request — the request-line under test.
    fn next_request_line(&self) -> String {
        let head = self.next_head();
        head.lines()
            .next()
            .expect("a request line")
            .trim_end()
            .to_string()
    }

    fn host(&self) -> String {
        self.addr.ip().to_string()
    }

    fn port_i32(&self) -> i32 {
        i32::from(self.addr.port())
    }
}

fn spawn_server(responses: Vec<String>) -> FakeServer {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind the fake server");
    let addr = listener.local_addr().expect("the fake server's address");
    let (tx, heads) = std::sync::mpsc::channel();

    // Detached: the test's assertions are driven by the channel, and the
    // listener dies with the process.
    std::thread::spawn(move || {
        for response in responses {
            let Ok((mut sock, _)) = listener.accept() else {
                return;
            };
            let head = read_head(&mut sock);
            if tx.send(head).is_err() {
                return;
            }
            let _ = sock.write_all(response.as_bytes());
            let _ = sock.flush();
            // Explicit shutdown rather than a bare drop, matching the proven
            // helper in `tests/misc_suite_d/networking_parity_test.rs:112`:
            // it is what closes the body for a client reading to EOF.
            let _ = sock.shutdown(Shutdown::Both);
        }
    });

    FakeServer { addr, heads }
}

/// Reads exactly up to the blank line, byte by byte.
///
/// ⚠️ Deliberately does not over-read: consuming into the request body would
/// make the recorded head depend on how the client happened to packetise it.
fn read_head(sock: &mut TcpStream) -> String {
    let _ = sock.set_read_timeout(Some(Duration::from_secs(10)));
    let mut buf = Vec::new();
    let mut byte = [0u8; 1];
    while sock.read(&mut byte).unwrap_or(0) == 1 {
        buf.push(byte[0]);
        if buf.ends_with(b"\r\n\r\n") {
            break;
        }
    }
    String::from_utf8_lossy(&buf).into_owned()
}

fn ok_response(body: &str) -> String {
    format!(
        "HTTP/1.1 200 OK\r\n\
         Content-Type: text/plain\r\n\
         Content-Length: {}\r\n\
         Connection: close\r\n\r\n{}",
        body.len(),
        body
    )
}

fn redirect_response(location: &str) -> String {
    format!(
        "HTTP/1.1 302 Found\r\n\
         Location: {location}\r\n\
         Content-Length: 0\r\n\
         Connection: close\r\n\r\n"
    )
}

/// Drives one complete exchange on an already-connected client.
fn exchange(client: &mut HttpClient, path: &str) {
    client
        .request(HttpMethod::Get, path, &HashMap::new(), &[])
        .expect("the request is written");
    client.read_response().expect("the response is read");
}

// ------------------------------------------------------- setter semantics

#[test]
fn a_fresh_client_has_no_proxy() {
    let client = HttpClient::new();
    assert_eq!(client.http_proxy(), None);
    assert_eq!(client.https_proxy(), None);
}

#[test]
fn an_empty_host_or_a_minus_one_port_clears_the_proxy() {
    let mut client = HttpClient::new();

    client.set_http_proxy("proxy.example", 3128);
    assert_eq!(client.http_proxy(), Some(("proxy.example", 3128)));

    client.set_http_proxy("", 3128);
    assert_eq!(client.http_proxy(), None, "an empty host clears it");

    client.set_http_proxy("proxy.example", 3128);
    client.set_http_proxy("proxy.example", -1);
    assert_eq!(client.http_proxy(), None, "a port of -1 clears it");
}

/// 🔑 `0` is not a clear sentinel — only `-1` is.
///
/// ⚠️ Upstream's condition is `p_port == -1` (`http_client_tcp.cpp:771`).
/// Writing it as `port <= 0` looks like the same rule and silently discards a
/// caller that asked for port 0, which is a legal (if unusual) value. This test
/// is the whole reason the comparison is spelled exactly.
#[test]
fn a_zero_port_is_stored_because_only_minus_one_clears() {
    let mut client = HttpClient::new();
    client.set_http_proxy("proxy.example", 0);
    assert_eq!(client.http_proxy(), Some(("proxy.example", 0)));

    client.set_https_proxy("proxy.example", 0);
    assert_eq!(client.https_proxy(), Some(("proxy.example", 0)));
}

/// The two settings are separate fields upstream and must stay separate here.
#[test]
fn the_http_and_https_proxies_are_stored_independently() {
    let mut client = HttpClient::new();

    client.set_https_proxy("tls.example", 8443);
    assert_eq!(client.https_proxy(), Some(("tls.example", 8443)));
    assert_eq!(
        client.http_proxy(),
        None,
        "setting the TLS proxy must not set the plaintext one"
    );

    client.set_http_proxy("plain.example", 3128);
    assert_eq!(client.http_proxy(), Some(("plain.example", 3128)));
    assert_eq!(
        client.https_proxy(),
        Some(("tls.example", 8443)),
        "and setting the plaintext one must not disturb the TLS one"
    );

    client.set_http_proxy("", -1);
    assert_eq!(
        client.https_proxy(),
        Some(("tls.example", 8443)),
        "clearing one must not clear the other"
    );
}

// --------------------------------------------------- the wire, through a proxy

/// A proxied request is addressed to the ORIGIN in absolute form, and the
/// `Host:` header still names the origin rather than the proxy.
#[test]
fn a_proxied_request_addresses_the_origin_in_absolute_form() {
    let proxy = spawn_server(vec![ok_response("hi")]);

    let mut client = HttpClient::new();
    client.set_http_proxy(&proxy.host(), proxy.port_i32());
    client
        .connect_to_host("origin.invalid", 8080)
        .expect("dials the proxy, not the unresolvable origin");
    exchange(&mut client, "/thing");

    let head = proxy.next_head();
    assert_eq!(
        head.lines().next().expect("a request line").trim_end(),
        "GET http://origin.invalid:8080/thing HTTP/1.1"
    );
    assert!(
        head.contains("Host: origin.invalid:8080\r\n"),
        "the Host header names the ORIGIN, not the proxy; got:\n{head}"
    );
    assert_eq!(client.response_code(), 200);
    assert_eq!(client.response_body(), b"hi".as_slice());
}

/// 🔑 The coincidence point: port 80.
///
/// ⚠️ The absolute URI keeps `:80`; the `Host:` header drops it. Two rules that
/// look like they should agree and do not (`http_client_tcp.cpp:163` vs
/// `:186-190`). Every other port makes the two spellings identical, so this is
/// the only origin port at which getting it wrong is visible.
#[test]
fn the_absolute_uri_keeps_the_default_port_that_the_host_header_drops() {
    let proxy = spawn_server(vec![ok_response("ok")]);

    let mut client = HttpClient::new();
    client.set_http_proxy(&proxy.host(), proxy.port_i32());
    client
        .connect_to_host("origin.invalid", 80)
        .expect("dials the proxy");
    exchange(&mut client, "/");

    let head = proxy.next_head();
    assert_eq!(
        head.lines().next().expect("a request line").trim_end(),
        "GET http://origin.invalid:80/ HTTP/1.1",
        "the absolute request-target always carries the port"
    );
    assert!(
        head.contains("Host: origin.invalid\r\n"),
        "the Host header omits the standard port; got:\n{head}"
    );
    assert!(
        !head.contains("Host: origin.invalid:80"),
        "…and must not carry it; got:\n{head}"
    );
}

/// The positive control: with no proxy the request-target is the bare path.
///
/// ⚠️ Without this, every assertion above would still pass if `via_http_proxy`
/// were stuck on and absolute form were emitted unconditionally.
#[test]
fn an_unproxied_request_uses_origin_form() {
    let server = spawn_server(vec![ok_response("direct")]);

    let mut client = HttpClient::new();
    client
        .connect_to_host(&server.host(), server.addr.port())
        .expect("connects directly");
    exchange(&mut client, "/thing");

    assert_eq!(server.next_request_line(), "GET /thing HTTP/1.1");
    assert_eq!(client.response_body(), b"direct".as_slice());
}

/// The same invariant repaired in the other direction: clearing the proxy must
/// also clear the absolute-form target on the next connection.
#[test]
fn clearing_the_proxy_returns_the_next_connection_to_origin_form() {
    let server = spawn_server(vec![ok_response("a"), ok_response("b")]);

    let mut client = HttpClient::new();
    client.set_http_proxy(&server.host(), server.port_i32());
    client
        .connect_to_host("origin.invalid", 8080)
        .expect("dials the proxy");
    exchange(&mut client, "/x");
    client.close();

    client.set_http_proxy("", -1);
    client
        .connect_to_host(&server.host(), server.addr.port())
        .expect("connects directly once the proxy is cleared");
    exchange(&mut client, "/x");

    assert_eq!(
        server.next_request_line(),
        "GET http://origin.invalid:8080/x HTTP/1.1"
    );
    assert_eq!(
        server.next_request_line(),
        "GET /x HTTP/1.1",
        "clearing the proxy must clear the absolute-form target with it"
    );
}

/// A proxy port outside the TCP range is reported, not clamped into one.
#[test]
fn a_proxy_port_outside_the_tcp_range_is_reported_not_clamped() {
    let mut client = HttpClient::new();
    client.set_http_proxy("127.0.0.1", 70_000);

    assert_eq!(
        client.connect_to_host("origin.invalid", 8080),
        Err(HttpResultCode::CantConnect),
        "an undialable proxy port must fail rather than silently become another port"
    );
}

// ------------------------------------------------------------- via perform()

#[test]
fn perform_routes_through_the_configured_proxy() {
    let proxy = spawn_server(vec![ok_response("through")]);

    let config = HttpRequestConfig {
        url: "http://origin.invalid:8080/data".into(),
        http_proxy: Some((proxy.host(), proxy.port_i32())),
        ..Default::default()
    };
    let response = HttpClient::perform(&config);

    assert_eq!(response.result, HttpResultCode::Success);
    assert_eq!(response.status_code, 200);
    assert_eq!(response.body.as_slice(), b"through".as_slice());
    assert_eq!(
        proxy.next_request_line(),
        "GET http://origin.invalid:8080/data HTTP/1.1"
    );
}

/// 🔑 A redirect is followed through the proxy too.
///
/// ⚠️ `perform` builds a fresh client for each hop, so the proxy has to be
/// applied inside the redirect loop. Applied once outside it, the first request
/// would be proxied and every redirect would go direct — invisible against any
/// URL that does not redirect, which is most of them.
#[test]
fn a_redirect_is_followed_through_the_proxy_too() {
    let proxy = spawn_server(vec![
        redirect_response("http://origin.invalid:8080/second"),
        ok_response("arrived"),
    ]);

    let config = HttpRequestConfig {
        url: "http://origin.invalid:8080/first".into(),
        http_proxy: Some((proxy.host(), proxy.port_i32())),
        ..Default::default()
    };
    let response = HttpClient::perform(&config);

    assert_eq!(response.result, HttpResultCode::Success);
    assert_eq!(response.body.as_slice(), b"arrived".as_slice());
    assert_eq!(
        proxy.next_request_line(),
        "GET http://origin.invalid:8080/first HTTP/1.1"
    );
    assert_eq!(
        proxy.next_request_line(),
        "GET http://origin.invalid:8080/second HTTP/1.1",
        "the redirect hop must reach the proxy as well"
    );
}

/// A broken proxy is an error, never a quiet fallback to a direct connection.
#[test]
fn an_unresolvable_proxy_fails_rather_than_falling_back_to_direct() {
    let config = HttpRequestConfig {
        url: "http://origin.invalid:8080/data".into(),
        http_proxy: Some(("proxy.invalid".to_string(), 3128)),
        ..Default::default()
    };

    assert_eq!(
        HttpClient::perform(&config).result,
        HttpResultCode::CantResolve,
        "a proxy that cannot be reached must not silently become a direct request"
    );
}

/// The TLS proxy round-trips through the config but changes nothing yet: an
/// `https://` URL still stops at the missing TLS backend.
///
/// ⚠️ Recorded as the current truth, not as desired behaviour. When TLS lands
/// this test is the one that has to change.
#[test]
fn an_https_proxy_is_carried_but_inert_while_tls_is_unimplemented() {
    let config = HttpRequestConfig {
        url: "https://origin.invalid/data".into(),
        https_proxy: Some(("tls-proxy.example".to_string(), 8443)),
        ..Default::default()
    };

    assert_eq!(
        HttpClient::perform(&config).result,
        HttpResultCode::SslHandshakeError,
        "https still stops at the missing TLS backend, proxy or not"
    );
}
