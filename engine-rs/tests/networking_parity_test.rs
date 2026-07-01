//! Networking parity tests for the HTTP and WebSocket node families.
//!
//! Covers two source-backed Godot gaps (pat-7xflw, pat-p2nes):
//!
//! * `HTTPRequest` / `HTTPClient` — async HTTP over the gdplatform transport,
//!   firing `request_completed` with the right result/response_code/headers/body
//!   and mapping connection/timeout failures to Godot's result codes.
//! * `WebSocketPeer` — RFC 6455 client handshake, text/binary echo round-trips,
//!   a clean close with code/reason, and CONNECTING→OPEN→CLOSING→CLOSED state
//!   transitions.
//!
//! Both families are also checked for ClassDB registration so GDScript naming
//! `HTTPRequest`/`HTTPClient`/`WebSocketPeer` resolves with Godot-matching
//! inheritance. Every server is an in-process `std::net` listener bound to
//! `127.0.0.1:0`, so the suite needs no external services and no new deps.

use std::io::{Read, Write};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use gdobject::class_db;
use gdplatform::http_request::{HTTPRequest, HttpMethod, HttpRequestConfig, HttpResultCode};
use gdplatform::websocket_peer::{
    decode_frame, encode_frame, parse_handshake_key, websocket_handshake_response, Frame, Opcode,
    WebSocketPeer, WebSocketState, WriteMode,
};

// Serializes the global ClassDB across tests when run in a shared process
// (e.g. `cargo test`); harmless under nextest's process-per-test model.
static CLASSDB_LOCK: Mutex<()> = Mutex::new(());

// ===========================================================================
// Shared test helpers
// ===========================================================================

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// Returns a loopback port with nothing listening (for connection-refused).
fn free_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    port
}

/// Reads a full HTTP request (head + body honoring Content-Length).
fn read_http_request(stream: &mut TcpStream) -> Option<(String, Vec<u8>)> {
    let mut buf = Vec::new();
    let mut tmp = [0u8; 1024];
    let header_end = loop {
        if let Some(p) = find(&buf, b"\r\n\r\n") {
            break p + 4;
        }
        match stream.read(&mut tmp) {
            Ok(0) => return None,
            Ok(n) => buf.extend_from_slice(&tmp[..n]),
            Err(_) => return None,
        }
    };
    let head = String::from_utf8_lossy(&buf[..header_end]).to_string();
    let content_length = head
        .split("\r\n")
        .find_map(|line| {
            let (k, v) = line.split_once(':')?;
            if k.trim().eq_ignore_ascii_case("content-length") {
                v.trim().parse::<usize>().ok()
            } else {
                None
            }
        })
        .unwrap_or(0);
    let mut body = buf[header_end..].to_vec();
    while body.len() < content_length {
        match stream.read(&mut tmp) {
            Ok(0) => break,
            Ok(n) => body.extend_from_slice(&tmp[..n]),
            Err(_) => break,
        }
    }
    Some((head, body))
}

/// Builds a raw HTTP/1.1 response with `Content-Length` + `Connection: close`.
fn build_response(status: &str, extra_headers: &[(&str, &str)], body: &[u8]) -> Vec<u8> {
    let mut head = format!("HTTP/1.1 {status}\r\n");
    for (k, v) in extra_headers {
        head.push_str(&format!("{k}: {v}\r\n"));
    }
    head.push_str(&format!("Content-Length: {}\r\n", body.len()));
    head.push_str("Connection: close\r\n\r\n");
    let mut out = head.into_bytes();
    out.extend_from_slice(body);
    out
}

/// Spawns an HTTP server that replies via `handler(head, body)` per connection.
fn spawn_http_server<F>(handler: F) -> SocketAddr
where
    F: Fn(&str, &[u8]) -> Vec<u8> + Send + 'static,
{
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    thread::spawn(move || {
        for conn in listener.incoming() {
            let mut stream = match conn {
                Ok(s) => s,
                Err(_) => break,
            };
            if let Some((head, body)) = read_http_request(&mut stream) {
                let response = handler(&head, &body);
                let _ = stream.write_all(&response);
                let _ = stream.flush();
                let _ = stream.shutdown(Shutdown::Both);
            }
        }
    });
    addr
}

/// Drives an [`HTTPRequest`] to completion and returns whether it finished.
fn drive_http(req: &HTTPRequest, timeout: Duration) -> bool {
    req.wait_for_completion(timeout)
}

/// Polls a [`WebSocketPeer`] until `cond` holds or the timeout elapses.
fn pump_until<F>(peer: &mut WebSocketPeer, timeout: Duration, cond: F) -> bool
where
    F: Fn(&WebSocketPeer) -> bool,
{
    let deadline = Instant::now() + timeout;
    loop {
        peer.poll();
        if cond(peer) {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        thread::sleep(Duration::from_millis(5));
    }
}

/// A minimal RFC 6455 echo server using the library's exported codec.
fn spawn_ws_echo_server() -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    thread::spawn(move || {
        for conn in listener.incoming() {
            match conn {
                Ok(stream) => {
                    thread::spawn(move || ws_echo_connection(stream));
                }
                Err(_) => break,
            }
        }
    });
    addr
}

fn ws_echo_connection(mut stream: TcpStream) {
    // 1. Read and answer the opening handshake.
    let mut buf = Vec::new();
    let mut tmp = [0u8; 1024];
    let header_end = loop {
        if let Some(p) = find(&buf, b"\r\n\r\n") {
            break p + 4;
        }
        match stream.read(&mut tmp) {
            Ok(0) => return,
            Ok(n) => buf.extend_from_slice(&tmp[..n]),
            Err(_) => return,
        }
    };
    let head = String::from_utf8_lossy(&buf[..header_end]).to_string();
    let key = match parse_handshake_key(&head) {
        Some(k) => k,
        None => return,
    };
    if stream
        .write_all(websocket_handshake_response(&key).as_bytes())
        .is_err()
    {
        return;
    }
    let _ = stream.flush();

    // 2. Echo data frames (preserving opcode); answer pings; mirror close.
    let mut frame_buf = buf[header_end..].to_vec();
    loop {
        loop {
            match decode_frame(&frame_buf) {
                Ok(Some((frame, consumed))) => {
                    frame_buf.drain(..consumed);
                    match frame.opcode {
                        Opcode::Text | Opcode::Binary => {
                            let echo = encode_frame(&Frame::new(frame.opcode, frame.payload), None);
                            if stream.write_all(&echo).is_err() {
                                return;
                            }
                        }
                        Opcode::Ping => {
                            let pong = encode_frame(&Frame::new(Opcode::Pong, frame.payload), None);
                            let _ = stream.write_all(&pong);
                        }
                        Opcode::Close => {
                            let close = encode_frame(&Frame::new(Opcode::Close, frame.payload), None);
                            let _ = stream.write_all(&close);
                            let _ = stream.flush();
                            let _ = stream.shutdown(Shutdown::Both);
                            return;
                        }
                        Opcode::Pong | Opcode::Continuation => {}
                    }
                }
                Ok(None) => break,
                Err(_) => return,
            }
        }
        match stream.read(&mut tmp) {
            Ok(0) => return,
            Ok(n) => frame_buf.extend_from_slice(&tmp[..n]),
            Err(_) => return,
        }
    }
}

// ===========================================================================
// HTTPRequest / HTTPClient — bead pat-7xflw (filter: http_request)
// ===========================================================================

#[test]
fn http_request_get_against_local_server() {
    let addr = spawn_http_server(|_head, _body| {
        build_response(
            "200 OK",
            &[("Content-Type", "text/plain"), ("X-Test-Header", "patina")],
            b"Hello, Patina!",
        )
    });

    let req = HTTPRequest::new();
    let seen: Arc<Mutex<Option<gdplatform::http_request::RequestCompleted>>> =
        Arc::new(Mutex::new(None));
    let sink = Arc::clone(&seen);
    req.connect_request_completed(move |rc| *sink.lock().unwrap() = Some(rc.clone()));

    req.request(HttpRequestConfig {
        url: format!("http://{addr}/index"),
        ..Default::default()
    });
    assert!(
        drive_http(&req, Duration::from_secs(5)),
        "request should complete"
    );

    let rc = seen.lock().unwrap().clone().expect("request_completed fired");
    assert_eq!(rc.result, HttpResultCode::Success.as_godot_result_code());
    assert_eq!(rc.result, 0);
    assert_eq!(rc.response_code, 200);
    assert_eq!(rc.body, b"Hello, Patina!");
    assert!(
        rc.headers.iter().any(|h| h == "X-Test-Header: patina"),
        "headers should carry the server header, got {:?}",
        rc.headers
    );
    assert!(rc.headers.iter().any(|h| h == "Content-Type: text/plain"));
}

#[test]
fn http_request_post_echoes_method_and_body() {
    let addr = spawn_http_server(|head, body| {
        let method = head.split_whitespace().next().unwrap_or("").to_string();
        let echo = format!("{method}:{}", String::from_utf8_lossy(body));
        build_response("200 OK", &[], echo.as_bytes())
    });

    let req = HTTPRequest::new();
    req.request(HttpRequestConfig {
        url: format!("http://{addr}/submit"),
        method: HttpMethod::Post,
        body: b"payload-123".to_vec(),
        ..Default::default()
    });
    assert!(drive_http(&req, Duration::from_secs(5)));

    let resp = req.take_response().expect("response present");
    assert_eq!(resp.status_code, 200);
    assert_eq!(resp.body_as_string().unwrap(), "POST:payload-123");
    assert!(resp.is_success());
}

#[test]
fn http_request_not_found_status_is_surfaced() {
    let addr = spawn_http_server(|_h, _b| build_response("404 Not Found", &[], b"missing"));

    let req = HTTPRequest::new();
    req.request(HttpRequestConfig {
        url: format!("http://{addr}/nope"),
        ..Default::default()
    });
    assert!(drive_http(&req, Duration::from_secs(5)));

    let resp = req.take_response().unwrap();
    // A 404 is still a *successful* transfer in Godot terms (RESULT_SUCCESS);
    // only the HTTP status reflects "not found".
    assert_eq!(resp.result, HttpResultCode::Success);
    assert_eq!(resp.status_code, 404);
    assert!(!resp.is_success());
}

#[test]
fn http_request_connection_refused_is_cant_connect() {
    let port = free_port();

    let req = HTTPRequest::new();
    let seen: Arc<Mutex<Option<gdplatform::http_request::RequestCompleted>>> =
        Arc::new(Mutex::new(None));
    let sink = Arc::clone(&seen);
    req.connect_request_completed(move |rc| *sink.lock().unwrap() = Some(rc.clone()));

    req.request(HttpRequestConfig {
        url: format!("http://127.0.0.1:{port}/"),
        timeout_seconds: 2.0,
        ..Default::default()
    });
    assert!(drive_http(&req, Duration::from_secs(5)));

    let rc = seen.lock().unwrap().clone().expect("request_completed fired");
    assert_eq!(rc.result, HttpResultCode::CantConnect.as_godot_result_code());
    assert_eq!(rc.result, 2); // Godot RESULT_CANT_CONNECT
    assert_eq!(rc.response_code, 0);
}

#[test]
fn http_request_timeout_when_server_silent() {
    // Server accepts the connection but never sends a response.
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    thread::spawn(move || {
        if let Ok((stream, _)) = listener.accept() {
            thread::sleep(Duration::from_secs(10));
            drop(stream);
        }
    });

    let req = HTTPRequest::new();
    req.request(HttpRequestConfig {
        url: format!("http://{addr}/slow"),
        timeout_seconds: 0.4,
        ..Default::default()
    });
    assert!(drive_http(&req, Duration::from_secs(5)));

    let resp = req.take_response().unwrap();
    assert_eq!(resp.result, HttpResultCode::Timeout);
    assert_eq!(
        resp.result.as_godot_result_code(),
        13,
        "Godot RESULT_TIMEOUT"
    );
}

#[test]
fn http_request_https_without_tls_reports_handshake_error() {
    // No TLS backend is wired into gdplatform, so https must fail honestly
    // rather than silently downgrading to plaintext.
    let req = HTTPRequest::new();
    req.request(HttpRequestConfig {
        url: "https://127.0.0.1:1/".into(),
        ..Default::default()
    });
    assert!(drive_http(&req, Duration::from_secs(5)));

    let resp = req.take_response().unwrap();
    assert_eq!(resp.result, HttpResultCode::SslHandshakeError);
    assert_eq!(resp.result.as_godot_result_code(), 5);
}

#[test]
fn http_request_registered_in_classdb() {
    let _guard = CLASSDB_LOCK.lock().unwrap();
    class_db::clear_for_testing();
    class_db::register_networking_classes();

    assert!(class_db::class_exists("HTTPRequest"));
    assert!(class_db::class_exists("HTTPClient"));

    // HTTPRequest : Node : Object
    assert!(class_db::is_parent_class("HTTPRequest", "Node"));
    assert_eq!(
        class_db::inheritance_chain("HTTPRequest"),
        vec!["HTTPRequest", "Node", "Object"]
    );
    // HTTPClient : RefCounted : Object
    assert_eq!(
        class_db::inheritance_chain("HTTPClient"),
        vec!["HTTPClient", "RefCounted", "Object"]
    );

    assert!(class_db::class_has_method("HTTPRequest", "request"));
    assert!(class_db::class_has_method("HTTPRequest", "cancel_request"));
    assert!(class_db::class_has_method("HTTPClient", "connect_to_host"));
    assert!(class_db::class_has_property("HTTPRequest", "timeout"));
}

// ===========================================================================
// WebSocketPeer — bead pat-p2nes (filter: websocket_peer)
// ===========================================================================

#[test]
fn websocket_peer_handshake_reaches_open() {
    let addr = spawn_ws_echo_server();
    let mut peer = WebSocketPeer::new();
    assert_eq!(peer.get_ready_state(), WebSocketState::Closed);

    peer.connect_to_url(&format!("ws://{addr}/chat")).unwrap();
    assert_eq!(peer.get_ready_state(), WebSocketState::Connecting);

    assert!(
        pump_until(&mut peer, Duration::from_secs(5), |p| {
            p.get_ready_state() == WebSocketState::Open
        }),
        "handshake should complete to OPEN"
    );
    assert_eq!(peer.get_requested_url(), format!("ws://{addr}/chat"));
}

#[test]
fn websocket_peer_text_echo_round_trip() {
    let addr = spawn_ws_echo_server();
    let mut peer = WebSocketPeer::new();
    peer.connect_to_url(&format!("ws://{addr}/")).unwrap();
    assert!(pump_until(&mut peer, Duration::from_secs(5), |p| p
        .get_ready_state()
        == WebSocketState::Open));

    peer.send_text("hello world").unwrap();
    assert!(
        pump_until(&mut peer, Duration::from_secs(5), |p| p
            .get_available_packet_count()
            > 0),
        "expected an echoed packet"
    );

    let packet = peer.get_packet();
    assert_eq!(packet, b"hello world");
    assert!(peer.was_string_packet(), "text frame must report as string");
}

#[test]
fn websocket_peer_binary_echo_round_trip() {
    let addr = spawn_ws_echo_server();
    let mut peer = WebSocketPeer::new();
    peer.connect_to_url(&format!("ws://{addr}/")).unwrap();
    assert!(pump_until(&mut peer, Duration::from_secs(5), |p| p
        .get_ready_state()
        == WebSocketState::Open));

    let payload = vec![0x00u8, 0x01, 0x02, 0xFF, 0xFE];
    peer.send(&payload, WriteMode::Binary).unwrap();
    assert!(pump_until(&mut peer, Duration::from_secs(5), |p| p
        .get_available_packet_count()
        > 0));

    let packet = peer.get_packet();
    assert_eq!(packet, payload);
    assert!(
        !peer.was_string_packet(),
        "binary frame must not report as string"
    );
}

#[test]
fn websocket_peer_clean_close_with_code_and_reason() {
    let addr = spawn_ws_echo_server();
    let mut peer = WebSocketPeer::new();
    peer.connect_to_url(&format!("ws://{addr}/")).unwrap();
    assert!(pump_until(&mut peer, Duration::from_secs(5), |p| p
        .get_ready_state()
        == WebSocketState::Open));

    peer.close(1000, "bye");
    assert_eq!(peer.get_ready_state(), WebSocketState::Closing);

    assert!(
        pump_until(&mut peer, Duration::from_secs(5), |p| p.get_ready_state()
            == WebSocketState::Closed),
        "close handshake should reach CLOSED"
    );
    assert_eq!(peer.get_close_code(), 1000);
    assert_eq!(peer.get_close_reason(), "bye");
}

#[test]
fn websocket_peer_poll_state_transitions() {
    let addr = spawn_ws_echo_server();
    let mut peer = WebSocketPeer::new();

    // CLOSED before connecting.
    assert_eq!(peer.get_ready_state(), WebSocketState::Closed);
    assert_eq!(peer.get_close_code(), -1);

    // CONNECTING right after connect_to_url.
    peer.connect_to_url(&format!("ws://{addr}/")).unwrap();
    assert_eq!(peer.get_ready_state(), WebSocketState::Connecting);

    // OPEN after the handshake completes via poll().
    assert!(pump_until(&mut peer, Duration::from_secs(5), |p| p
        .get_ready_state()
        == WebSocketState::Open));

    // Multiple round-trips keep it OPEN and ordered.
    peer.send_text("one").unwrap();
    peer.send_text("two").unwrap();
    assert!(pump_until(&mut peer, Duration::from_secs(5), |p| p
        .get_available_packet_count()
        >= 2));
    assert_eq!(peer.get_packet(), b"one");
    assert_eq!(peer.get_packet(), b"two");
    assert_eq!(peer.get_ready_state(), WebSocketState::Open);
}

#[test]
fn websocket_peer_rejects_wss_without_tls() {
    let mut peer = WebSocketPeer::new();
    let err = peer.connect_to_url("wss://example.com/").unwrap_err();
    assert!(
        matches!(err, gdplatform::websocket_peer::WebSocketError::TlsUnsupported),
        "wss:// must be rejected when no TLS backend is present"
    );
}

#[test]
fn websocket_peer_registered_in_classdb() {
    let _guard = CLASSDB_LOCK.lock().unwrap();
    class_db::clear_for_testing();
    class_db::register_networking_classes();

    assert!(class_db::class_exists("WebSocketPeer"));
    // WebSocketPeer : PacketPeer : RefCounted : Object
    assert!(class_db::is_parent_class("WebSocketPeer", "PacketPeer"));
    assert!(class_db::is_parent_class("WebSocketPeer", "RefCounted"));
    assert_eq!(
        class_db::inheritance_chain("WebSocketPeer"),
        vec!["WebSocketPeer", "PacketPeer", "RefCounted", "Object"]
    );
    assert!(class_db::class_has_method("WebSocketPeer", "connect_to_url"));
    assert!(class_db::class_has_method("WebSocketPeer", "send_text"));
    assert!(class_db::class_has_method("WebSocketPeer", "get_packet"));
}
