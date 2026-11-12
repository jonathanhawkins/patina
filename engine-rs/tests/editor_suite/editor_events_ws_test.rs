//! pat-zzgh5: `/api/events` WebSocket broadcasting scene-tree mutations.
//!
//! Acceptance: two clients connected to `/api/events` both receive the same
//! ordered stream of mutation events when a third client mutates the scene
//! tree.

use gdeditor::editor_server::{EditorServerHandle, EditorState};
use gdscene::node::Node;
use gdscene::SceneTree;

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;
use std::time::Duration;

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn fixture_state() -> EditorState {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    tree.add_child(root, Node::new("Main", "Node2D")).unwrap();
    EditorState::new(tree)
}

fn connect_with_retry(port: u16) -> TcpStream {
    for _ in 0..40 {
        if let Ok(s) = TcpStream::connect(format!("127.0.0.1:{port}")) {
            return s;
        }
        thread::sleep(Duration::from_millis(25));
    }
    panic!("could not connect to editor server on port {port}");
}

/// Opens a WebSocket client to `/api/events`, consumes the `101 Switching
/// Protocols` response headers, and leaves the stream parked ready to read
/// frames.
fn open_ws_client(port: u16) -> TcpStream {
    let mut stream = connect_with_retry(port);
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let req = "GET /api/events HTTP/1.1\r\n\
               Host: localhost\r\n\
               Upgrade: websocket\r\n\
               Connection: Upgrade\r\n\
               Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\
               Sec-WebSocket-Version: 13\r\n\
               \r\n";
    stream.write_all(req.as_bytes()).unwrap();

    // Drain bytes until we see the end-of-headers sentinel.
    let mut hdr = Vec::with_capacity(512);
    let mut byte = [0u8; 1];
    loop {
        let n = stream.read(&mut byte).expect("ws handshake read failed");
        assert_eq!(n, 1, "stream closed before WebSocket handshake completed");
        hdr.push(byte[0]);
        if hdr.len() >= 4 && &hdr[hdr.len() - 4..] == b"\r\n\r\n" {
            break;
        }
        if hdr.len() > 4096 {
            panic!(
                "handshake response did not terminate within 4 KiB: {}",
                String::from_utf8_lossy(&hdr)
            );
        }
    }
    let hdr_str = String::from_utf8_lossy(&hdr).to_string();
    assert!(
        hdr_str.starts_with("HTTP/1.1 101"),
        "expected 101 Switching Protocols, got:\n{hdr_str}"
    );
    let hdr_lower = hdr_str.to_lowercase();
    assert!(
        hdr_lower.contains("upgrade: websocket"),
        "missing Upgrade: websocket header in:\n{hdr_str}"
    );
    assert!(
        hdr_lower.contains("sec-websocket-accept:"),
        "missing Sec-WebSocket-Accept in:\n{hdr_str}"
    );
    stream
}

/// Reads exactly one unfragmented unmasked text frame from a server→client
/// WebSocket stream and returns the UTF-8 payload.
fn read_text_frame(stream: &mut TcpStream) -> String {
    let mut header = [0u8; 2];
    stream
        .read_exact(&mut header)
        .expect("failed to read frame header");
    assert_eq!(
        header[0], 0x81,
        "first byte must be FIN=1, opcode=text, got 0x{:02x}",
        header[0]
    );
    let mask_and_len = header[1];
    assert_eq!(
        mask_and_len & 0x80,
        0,
        "server→client frames must be unmasked, got mask bit set"
    );
    let len_marker = mask_and_len & 0x7F;
    let payload_len = match len_marker {
        0..=125 => len_marker as usize,
        126 => {
            let mut ext = [0u8; 2];
            stream.read_exact(&mut ext).unwrap();
            u16::from_be_bytes(ext) as usize
        }
        _ => {
            let mut ext = [0u8; 8];
            stream.read_exact(&mut ext).unwrap();
            u64::from_be_bytes(ext) as usize
        }
    };
    let mut payload = vec![0u8; payload_len];
    stream
        .read_exact(&mut payload)
        .expect("failed to read frame payload");
    String::from_utf8(payload).expect("frame payload must be valid UTF-8")
}

fn http_post(port: u16, path: &str, body: &str) -> u16 {
    let mut stream = connect_with_retry(port);
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let req = format!(
        "POST {path} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {len}\r\nConnection: close\r\n\r\n{body}",
        len = body.len(),
    );
    stream.write_all(req.as_bytes()).unwrap();
    let mut resp = String::new();
    stream.read_to_string(&mut resp).ok();
    resp.lines()
        .next()
        .and_then(|l| l.split_whitespace().nth(1).and_then(|s| s.parse().ok()))
        .unwrap_or(0)
}

fn child_node_id(port: u16) -> u64 {
    let mut stream = connect_with_retry(port);
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let req = "GET /api/scene HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n";
    stream.write_all(req.as_bytes()).unwrap();
    let mut resp = String::new();
    stream.read_to_string(&mut resp).ok();
    let body = resp.split("\r\n\r\n").nth(1).expect("scene response body");
    let v: serde_json::Value = serde_json::from_str(body)
        .unwrap_or_else(|e| panic!("/api/scene response not JSON: {body}\n{e}"));
    v["nodes"]["children"][0]["id"]
        .as_u64()
        .expect("first child must expose a u64 id")
}

#[test]
fn editor_events_ws_test() {
    let port = free_port();
    let handle = EditorServerHandle::start(port, fixture_state());
    thread::sleep(Duration::from_millis(120));

    let node_id = child_node_id(port);

    // Two WS subscribers. After both handshakes complete, the server-side
    // connection threads will each have pushed a `Sender<String>` into
    // `EditorState.event_subscribers`. Allow a brief moment for that
    // registration to land (it happens after the 101 response is written,
    // so a tiny sleep avoids a publish-vs-register race).
    let mut a = open_ws_client(port);
    let mut b = open_ws_client(port);
    thread::sleep(Duration::from_millis(150));

    // Third client mutates the scene tree via `POST /api/property/set`.
    let body = format!(
        r#"{{"node_id":{node_id},"property":"test_visible","value":{{"type":"Bool","value":true}}}}"#
    );
    assert_eq!(
        http_post(port, "/api/property/set", &body),
        200,
        "mutation must succeed"
    );

    // Both subscribers must see the same event for this mutation, in order.
    let evt_a = read_text_frame(&mut a);
    let evt_b = read_text_frame(&mut b);
    assert!(
        evt_a.contains("property_changed"),
        "event A should be a property_changed event, got: {evt_a}"
    );
    assert_eq!(
        evt_a, evt_b,
        "both subscribers must observe identical events for the same mutation"
    );
    assert!(
        evt_a.contains(&format!(r#""node_id":{node_id}"#)),
        "event must reference the mutated node ({node_id}): {evt_a}"
    );

    // Second mutation. The ordered stream property means each subscriber's
    // next read is the next event, and they still match.
    let body2 = format!(
        r#"{{"node_id":{node_id},"property":"test_visible","value":{{"type":"Bool","value":false}}}}"#
    );
    assert_eq!(http_post(port, "/api/property/set", &body2), 200);
    let next_a = read_text_frame(&mut a);
    let next_b = read_text_frame(&mut b);
    assert_eq!(
        next_a, next_b,
        "second event must also match across subscribers"
    );

    handle.stop();
}
