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
                            let close =
                                encode_frame(&Frame::new(Opcode::Close, frame.payload), None);
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

    let rc = seen
        .lock()
        .unwrap()
        .clone()
        .expect("request_completed fired");
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

    let rc = seen
        .lock()
        .unwrap()
        .clone()
        .expect("request_completed fired");
    assert_eq!(
        rc.result,
        HttpResultCode::CantConnect.as_godot_result_code()
    );
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
        matches!(
            err,
            gdplatform::websocket_peer::WebSocketError::TlsUnsupported
        ),
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
    assert!(class_db::class_has_method(
        "WebSocketPeer",
        "connect_to_url"
    ));
    assert!(class_db::class_has_method("WebSocketPeer", "send_text"));
    assert!(class_db::class_has_method("WebSocketPeer", "get_packet"));
}

// ===========================================================================
// pat-fg8dk: TcpMultiplayerPeer — real loopback multiplayer beyond MockNetwork
// ===========================================================================

use gdplatform::network::{
    decode_spawn_packet, decode_sync_packet, ConnectionStatus, MultiplayerAPI, MultiplayerSpawner,
    MultiplayerSynchronizer, NetworkPeer, Packet, PeerId, TransferMode,
};
use gdplatform::TcpMultiplayerPeer;

fn multiplayer_pump_handshake(server: &mut TcpMultiplayerPeer, client: &mut TcpMultiplayerPeer) {
    for _ in 0..100 {
        let _ = server.poll();
        let _ = client.poll();
        if client.get_connection_status() == ConnectionStatus::Connected {
            return;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    panic!(
        "TCP multiplayer handshake timed out (client status={:?})",
        client.get_connection_status()
    );
}

#[test]
fn multiplayer_tcp_server_client_packet_round_trip() {
    let mut server = TcpMultiplayerPeer::new();
    let addr = server
        .create_server("127.0.0.1:0")
        .expect("bind loopback server");
    assert!(server.is_server());
    assert_eq!(server.get_connection_status(), ConnectionStatus::Connected);

    let mut client = TcpMultiplayerPeer::new();
    client.create_client(addr).expect("connect client");
    assert_eq!(client.get_connection_status(), ConnectionStatus::Connecting);

    multiplayer_pump_handshake(&mut server, &mut client);
    assert!(!client.is_server());
    assert_ne!(client.get_unique_id(), PeerId::SERVER);

    let hello = b"pat-fg8dk-hello".to_vec();
    client
        .send_packet(
            PeerId::SERVER,
            Packet {
                sender: client.get_unique_id(),
                channel: 3,
                data: hello.clone(),
                transfer_mode: TransferMode::Reliable,
            },
        )
        .expect("client send");

    let mut received = None;
    for _ in 0..80 {
        let _ = client.poll();
        for p in server.poll() {
            if p.data == hello {
                received = Some(p);
                break;
            }
        }
        if received.is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    let p = received.expect("server must receive client packet over TCP");
    assert_eq!(p.sender, client.get_unique_id());
    assert_eq!(p.channel, 3);

    let ack = b"pat-fg8dk-ack".to_vec();
    server
        .send_packet(
            client.get_unique_id(),
            Packet {
                sender: PeerId::SERVER,
                channel: 4,
                data: ack.clone(),
                transfer_mode: TransferMode::Reliable,
            },
        )
        .expect("server send");

    let mut ack_rx = None;
    for _ in 0..80 {
        let _ = server.poll();
        for p in client.poll() {
            if p.data == ack {
                ack_rx = Some(p);
                break;
            }
        }
        if ack_rx.is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    let p = ack_rx.expect("client must receive server packet over TCP");
    assert_eq!(p.sender, PeerId::SERVER);
    assert_eq!(p.channel, 4);

    client.close();
    server.close();
}

#[test]
fn multiplayer_api_accepts_tcp_peer() {
    let mut server = TcpMultiplayerPeer::new();
    let addr = server.create_server("127.0.0.1:0").unwrap();
    let mut client = TcpMultiplayerPeer::new();
    client.create_client(addr).unwrap();
    multiplayer_pump_handshake(&mut server, &mut client);

    let mut api = MultiplayerAPI::new();
    api.set_multiplayer_peer(Box::new(server));
    assert!(api.is_server());

    let mut api_c = MultiplayerAPI::new();
    api_c.set_multiplayer_peer(Box::new(client));
    assert!(!api_c.is_server());
}

#[test]
fn multiplayer_tcp_registered_in_classdb() {
    let _guard = CLASSDB_LOCK.lock().unwrap();
    class_db::clear_for_testing();
    class_db::register_networking_classes();

    assert!(class_db::class_exists("TcpMultiplayerPeer"));
    assert!(class_db::class_exists("MultiplayerPeer"));
    assert!(class_db::class_exists("MultiplayerAPI"));
    assert!(class_db::is_parent_class(
        "TcpMultiplayerPeer",
        "MultiplayerPeer"
    ));
    assert!(class_db::is_parent_class(
        "TcpMultiplayerPeer",
        "PacketPeer"
    ));
    assert!(class_db::class_has_method(
        "TcpMultiplayerPeer",
        "create_server"
    ));
    assert!(class_db::class_has_method(
        "TcpMultiplayerPeer",
        "create_client"
    ));
}

// ===========================================================================
// pat-z26jd: ENetMultiplayerPeer — ClassDB alias over TcpMultiplayerPeer
// ===========================================================================

use gdplatform::ENetMultiplayerPeer;

fn enet_pump_handshake(server: &mut ENetMultiplayerPeer, client: &mut ENetMultiplayerPeer) {
    for _ in 0..100 {
        let _ = server.poll();
        let _ = client.poll();
        if client.get_connection_status() == ConnectionStatus::Connected {
            return;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    panic!(
        "ENet multiplayer handshake timed out (client status={:?})",
        client.get_connection_status()
    );
}

#[test]
fn multiplayer_enet_server_client_reliable_packet() {
    let mut server = ENetMultiplayerPeer::new();
    let addr = server
        .create_server("127.0.0.1:0")
        .expect("bind ENet (TCP-backed) server");
    assert!(server.is_server());
    assert_eq!(server.get_connection_status(), ConnectionStatus::Connected);

    let mut client = ENetMultiplayerPeer::new();
    client.create_client(addr).expect("connect ENet client");
    assert_eq!(client.get_connection_status(), ConnectionStatus::Connecting);

    enet_pump_handshake(&mut server, &mut client);
    assert!(!client.is_server());
    assert_ne!(client.get_unique_id(), PeerId::SERVER);

    let hello = b"pat-z26jd-enet-hello".to_vec();
    client
        .send_packet(
            PeerId::SERVER,
            Packet {
                sender: client.get_unique_id(),
                channel: 5,
                data: hello.clone(),
                transfer_mode: TransferMode::Reliable,
            },
        )
        .expect("client send reliable");

    let mut received = None;
    for _ in 0..80 {
        let _ = client.poll();
        for p in server.poll() {
            if p.data == hello {
                received = Some(p);
                break;
            }
        }
        if received.is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    let p = received.expect("server must receive reliable packet via ENetMultiplayerPeer");
    assert_eq!(p.sender, client.get_unique_id());
    assert_eq!(p.channel, 5);
    assert_eq!(p.transfer_mode, TransferMode::Reliable);

    let ack = b"pat-z26jd-enet-ack".to_vec();
    server
        .send_packet(
            client.get_unique_id(),
            Packet {
                sender: PeerId::SERVER,
                channel: 2,
                data: ack.clone(),
                transfer_mode: TransferMode::Ordered,
            },
        )
        .expect("server send");

    let mut ack_rx = None;
    for _ in 0..80 {
        let _ = server.poll();
        for p in client.poll() {
            if p.data == ack {
                ack_rx = Some(p);
                break;
            }
        }
        if ack_rx.is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    let p = ack_rx.expect("client must receive server packet via ENetMultiplayerPeer");
    assert_eq!(p.sender, PeerId::SERVER);
    assert_eq!(p.channel, 2);
    assert_eq!(p.transfer_mode, TransferMode::Ordered);

    client.close();
    server.close();
}

#[test]
fn multiplayer_enet_preserves_channel_metadata() {
    let mut server = ENetMultiplayerPeer::new();
    let addr = server.create_server("127.0.0.1:0").unwrap();
    let mut client = ENetMultiplayerPeer::new();
    client.create_client(addr).unwrap();
    enet_pump_handshake(&mut server, &mut client);

    // Unreliable mode is metadata-only over TCP; channel must still round-trip.
    let payload = b"pat-z26jd-ch-meta".to_vec();
    client
        .send_packet(
            PeerId::SERVER,
            Packet {
                sender: client.get_unique_id(),
                channel: 9,
                data: payload.clone(),
                transfer_mode: TransferMode::Unreliable,
            },
        )
        .unwrap();

    let mut got = None;
    for _ in 0..80 {
        let _ = client.poll();
        for p in server.poll() {
            if p.data == payload {
                got = Some(p);
                break;
            }
        }
        if got.is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    let p = got.expect("channel metadata preserved in ENet alias packet headers");
    assert_eq!(p.channel, 9);
    assert_eq!(p.transfer_mode, TransferMode::Unreliable);
}

#[test]
fn multiplayer_api_accepts_enet_peer() {
    let mut server = ENetMultiplayerPeer::new();
    let addr = server.create_server("127.0.0.1:0").unwrap();
    let mut client = ENetMultiplayerPeer::new();
    client.create_client(addr).unwrap();
    enet_pump_handshake(&mut server, &mut client);

    let mut api = MultiplayerAPI::new();
    api.set_multiplayer_peer(Box::new(server));
    assert!(api.is_server());

    let mut api_c = MultiplayerAPI::new();
    api_c.set_multiplayer_peer(Box::new(client));
    assert!(!api_c.is_server());
}

#[test]
fn multiplayer_enet_registered_in_classdb() {
    let _guard = CLASSDB_LOCK.lock().unwrap();
    class_db::clear_for_testing();
    class_db::register_networking_classes();

    assert!(class_db::class_exists("ENetMultiplayerPeer"));
    assert!(class_db::class_exists("MultiplayerPeer"));
    assert!(class_db::is_parent_class(
        "ENetMultiplayerPeer",
        "MultiplayerPeer"
    ));
    assert!(class_db::is_parent_class(
        "ENetMultiplayerPeer",
        "PacketPeer"
    ));
    assert!(class_db::class_has_method(
        "ENetMultiplayerPeer",
        "create_server"
    ));
    assert!(class_db::class_has_method(
        "ENetMultiplayerPeer",
        "create_client"
    ));
    assert!(class_db::class_has_method("ENetMultiplayerPeer", "poll"));
    assert!(class_db::class_has_method("ENetMultiplayerPeer", "close"));
}

// ===========================================================================
// MultiplayerSpawner / MultiplayerSynchronizer — bead pat-ikq04
// Filters: multiplayer_spawner, multiplayer_synchronizer
// ===========================================================================

#[test]
fn multiplayer_spawner_host_spawn_packet_observed_by_client() {
    // Host spawns a scene path over TcpMultiplayerPeer; client decodes the
    // spawn packet (bounded multiplayer replication slice).
    let mut server = TcpMultiplayerPeer::new();
    let addr = server
        .create_server("127.0.0.1:0")
        .expect("bind loopback server");
    let mut client = TcpMultiplayerPeer::new();
    client.create_client(addr).expect("connect client");
    multiplayer_pump_handshake(&mut server, &mut client);

    let client_id = client.get_unique_id();
    let mut host_spawner = MultiplayerSpawner::new(0);
    host_spawner.add_spawnable_scene("res://player.tscn");
    host_spawner.set_spawn_path(".");
    let req = host_spawner
        .spawn_and_notify(&mut server, client_id, "team=blue")
        .expect("host spawn_and_notify");
    assert_eq!(req.scene_path, "res://player.tscn");
    assert_eq!(host_spawner.spawn_count(), 1);

    let mut observed = None;
    for _ in 0..80 {
        let _ = server.poll();
        for p in client.poll() {
            if let Some(spawn) = decode_spawn_packet(&p.data) {
                observed = Some((p, spawn));
                break;
            }
        }
        if observed.is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    let (packet, spawn) = observed.expect("client must observe host spawn packet");
    assert_eq!(spawn.scene_path, "res://player.tscn");
    assert_eq!(spawn.data, "team=blue");
    assert_eq!(spawn.net_id, req.net_id);
    assert_eq!(packet.sender, PeerId::SERVER);

    let mut client_spawner = MultiplayerSpawner::new(0);
    let applied = client_spawner
        .observe_spawn_packet(&packet)
        .expect("client spawner applies spawn packet");
    assert_eq!(applied.scene_path, "res://player.tscn");
    assert_eq!(client_spawner.spawn_count(), 1);

    client.close();
    server.close();
}

#[test]
fn multiplayer_synchronizer_property_sync_once() {
    // Authority writes position, polls outbound once; remote applies the sync.
    let mut server = TcpMultiplayerPeer::new();
    let addr = server
        .create_server("127.0.0.1:0")
        .expect("bind loopback server");
    let mut client = TcpMultiplayerPeer::new();
    client.create_client(addr).expect("connect client");
    multiplayer_pump_handshake(&mut server, &mut client);

    let client_id = client.get_unique_id();
    let mut auth = MultiplayerSynchronizer::new(16);
    auth.set_root_path("..");
    auth.set_net_id(9);
    auth.add_property("position");
    auth.set_property_value("position", gdvariant::Variant::from("12.5,-3.0"));
    assert!(
        auth.poll_outbound(&mut server, client_id, 1000, true)
            .expect("poll_outbound"),
        "authority should emit a sync packet"
    );

    let mut applied = None;
    for _ in 0..80 {
        let _ = server.poll();
        for p in client.poll() {
            if decode_sync_packet(&p.data).is_some() {
                applied = Some(p);
                break;
            }
        }
        if applied.is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    let packet = applied.expect("client must receive property sync packet");
    assert_eq!(packet.sender, PeerId::SERVER);

    let mut remote = MultiplayerSynchronizer::new(16);
    remote.add_property("position");
    let sync = remote
        .apply_sync_packet(&packet)
        .expect("apply_sync_packet");
    assert_eq!(sync.net_id, 9);
    assert_eq!(
        remote.get_property_value("position"),
        Some(&gdvariant::Variant::from("12.5,-3.0"))
    );
    assert_eq!(remote.get_net_id(), 9);

    client.close();
    server.close();
}

#[test]
fn multiplayer_spawner_synchronizer_registered_in_classdb() {
    let _guard = CLASSDB_LOCK.lock().unwrap();
    class_db::clear_for_testing();
    class_db::register_networking_classes();

    assert!(class_db::class_exists("MultiplayerSpawner"));
    assert!(class_db::class_exists("MultiplayerSynchronizer"));
    assert!(class_db::is_parent_class("MultiplayerSpawner", "Node"));
    assert!(class_db::is_parent_class("MultiplayerSynchronizer", "Node"));
    assert_eq!(
        class_db::inheritance_chain("MultiplayerSpawner"),
        vec!["MultiplayerSpawner", "Node", "Object"]
    );
    assert_eq!(
        class_db::inheritance_chain("MultiplayerSynchronizer"),
        vec!["MultiplayerSynchronizer", "Node", "Object"]
    );

    assert!(class_db::class_has_method("MultiplayerSpawner", "spawn"));
    assert!(class_db::class_has_method(
        "MultiplayerSpawner",
        "add_spawnable_scene"
    ));
    assert!(class_db::class_has_method(
        "MultiplayerSpawner",
        "get_spawnable_scene_count"
    ));
    assert!(class_db::class_has_property(
        "MultiplayerSpawner",
        "spawn_path"
    ));
    assert!(class_db::class_has_property(
        "MultiplayerSpawner",
        "spawn_limit"
    ));

    assert!(class_db::class_has_method(
        "MultiplayerSynchronizer",
        "set_root_path"
    ));
    assert!(class_db::class_has_method(
        "MultiplayerSynchronizer",
        "get_replication_interval"
    ));
    assert!(class_db::class_has_property(
        "MultiplayerSynchronizer",
        "root_path"
    ));
    assert!(class_db::class_has_property(
        "MultiplayerSynchronizer",
        "replication_interval"
    ));
}

// ===========================================================================
// PacketPeerUDP / StreamPeerTCP — bead pat-72jn7
// ===========================================================================

#[test]
fn packet_peer_udp_loopback_echo() {
    use gdplatform::PacketPeerUDP;

    let mut server = PacketPeerUDP::new();
    let local = server.bind("127.0.0.1:0").unwrap();

    let mut client = PacketPeerUDP::new();
    client.bind("127.0.0.1:0").unwrap();
    client.set_dest_address(local).unwrap();
    client.put_packet(b"udp-ping").unwrap();

    let start = Instant::now();
    while server.get_available_packet_count() == 0 && start.elapsed() < Duration::from_secs(2) {
        server.poll();
        client.poll();
        thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(server.get_packet(), b"udp-ping");
}

#[test]
fn stream_peer_tcp_loopback_byte_stream() {
    use gdplatform::{StreamPeerTCP, StreamPeerTcpStatus};
    use std::net::TcpListener;

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();

    let mut client = StreamPeerTCP::new();
    client.connect_to_addr(addr).unwrap();
    assert_eq!(client.get_status(), StreamPeerTcpStatus::Connected);

    let start = Instant::now();
    let mut server = loop {
        match StreamPeerTCP::accept_from_listener(&listener) {
            Ok(s) => break s,
            Err(_) if start.elapsed() < Duration::from_secs(2) => {
                thread::sleep(Duration::from_millis(1));
            }
            Err(e) => panic!("accept: {e}"),
        }
    };

    client.put_data(b"stream-data").unwrap();
    let start = Instant::now();
    while server.get_available_bytes() < 11 && start.elapsed() < Duration::from_secs(2) {
        server.poll();
        thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(server.get_data(64), b"stream-data");
}

#[test]
fn packet_stream_peers_registered_in_classdb() {
    let _guard = CLASSDB_LOCK.lock().unwrap();
    class_db::clear_for_testing();
    class_db::register_networking_classes();

    assert!(class_db::class_exists("PacketPeerUDP"));
    assert!(class_db::class_exists("StreamPeerTCP"));
    assert!(class_db::class_exists("StreamPeer"));
    assert!(class_db::is_parent_class("PacketPeerUDP", "PacketPeer"));
    assert!(class_db::is_parent_class("StreamPeerTCP", "StreamPeer"));
    assert!(class_db::class_has_method("PacketPeerUDP", "bind"));
    assert!(class_db::class_has_method("PacketPeerUDP", "put_packet"));
    assert!(class_db::class_has_method(
        "StreamPeerTCP",
        "connect_to_host"
    ));
    assert!(class_db::class_has_method("StreamPeerTCP", "get_status"));
}
