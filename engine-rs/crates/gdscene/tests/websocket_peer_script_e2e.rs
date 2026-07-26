//! pat-0q2kx.96 acceptance: `WebSocketPeer` is constructible and drivable from
//! GDScript inside a running SceneTree.
//!
//! Godot ground truth: `modules/websocket/websocket_peer.cpp` — scripts do
//! `WebSocketPeer.new()`, set `supported_protocols`, `connect_to_url`, then
//! `poll()` each frame until `get_ready_state()` is `STATE_OPEN`, exchanging
//! text and binary packets and reading back the negotiated subprotocol.
//!
//! Patina had a complete RFC 6455 client in `gdplatform` and **no way to reach
//! it**: `grep -ic websocket interpreter.rs` was 0, so no script could construct
//! the class. This drives the real class from a real script in a stepped
//! `MainLoop` against a real socket — not a unit test of the module.
//!
//! Note the call order in the script below: `was_string_packet()` reports the
//! flag `get_packet()` sets as it pops, so it must be read *after* the packet,
//! not before — Godot documents it as "the last received packet". Asking first
//! returns the previous message's flag (or `false` before any read).
//!
//! The echo server here is built from the crate's own exported frame codec,
//! which `websocket_peer.rs` documents as "public so a test/echo server can
//! reuse it". A raw server is used rather than `WebSocketMultiplayerPeer`
//! because the latter injects its own `TAG_SET_ID` control envelope ahead of
//! application traffic, which would muddy the packet assertions.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use gdplatform::websocket_peer::{
    decode_frame, encode_frame, parse_handshake_key, parse_handshake_protocols,
    select_handshake_protocol, websocket_handshake_response_with_protocol, Frame, Opcode,
};
use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscene::{GDScriptNodeInstance, MainLoop};
use gdscript_interop::bindings::ScriptInstance;
use gdvariant::Variant;

/// What the echo server observed, reported back once the connection ends.
struct ServerReport {
    close_code: u16,
    close_reason: String,
}

/// A one-connection WebSocket echo server that negotiates `"chat"`.
///
/// Echoes every text/binary frame back with the same opcode, and reports the
/// close code/reason the client sent so the far-end half of `close(4000, "bye")`
/// can be asserted.
fn spawn_echo_server() -> (String, mpsc::Receiver<ServerReport>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind echo server");
    let addr = listener.local_addr().expect("local addr");
    let url = format!("ws://{addr}/");
    let (tx, rx) = mpsc::channel();

    thread::spawn(move || {
        let Ok((mut stream, _)) = listener.accept() else {
            return;
        };
        stream
            .set_read_timeout(Some(Duration::from_millis(50)))
            .ok();

        // --- opening handshake -------------------------------------------
        let mut buf = Vec::new();
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if Instant::now() > deadline {
                return;
            }
            let mut chunk = [0u8; 1024];
            match stream.read(&mut chunk) {
                Ok(0) => return,
                Ok(n) => buf.extend_from_slice(&chunk[..n]),
                Err(_) => {}
            }
            if buf.windows(4).any(|w| w == b"\r\n\r\n") {
                break;
            }
        }
        let head = String::from_utf8_lossy(&buf).to_string();
        let end = head.find("\r\n\r\n").map(|p| p + 4).unwrap_or(head.len());
        buf.drain(..end.min(buf.len()));

        let Some(key) = parse_handshake_key(&head) else {
            return;
        };
        let offered = parse_handshake_protocols(&head);
        let supported = vec!["chat".to_string()];
        let selected = select_handshake_protocol(&offered, &supported);
        let response = websocket_handshake_response_with_protocol(&key, selected.as_deref());
        if stream.write_all(response.as_bytes()).is_err() {
            return;
        }

        // --- echo loop ----------------------------------------------------
        let mut report = ServerReport {
            close_code: 0,
            close_reason: String::new(),
        };
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if Instant::now() > deadline {
                break;
            }
            match decode_frame(&buf) {
                Ok(Some((frame, consumed))) => {
                    buf.drain(..consumed);
                    match frame.opcode {
                        Opcode::Text | Opcode::Binary => {
                            // Server → client frames are unmasked (RFC 6455 §5.1).
                            let echo = encode_frame(&Frame::new(frame.opcode, frame.payload), None);
                            if stream.write_all(&echo).is_err() {
                                break;
                            }
                        }
                        Opcode::Close => {
                            if frame.payload.len() >= 2 {
                                report.close_code =
                                    u16::from_be_bytes([frame.payload[0], frame.payload[1]]);
                                report.close_reason =
                                    String::from_utf8_lossy(&frame.payload[2..]).to_string();
                            }
                            let _ = stream.write_all(&encode_frame(
                                &Frame::new(Opcode::Close, frame.payload),
                                None,
                            ));
                            break;
                        }
                        _ => {}
                    }
                    continue;
                }
                Ok(None) => {}
                Err(_) => break,
            }
            let mut chunk = [0u8; 1024];
            match stream.read(&mut chunk) {
                Ok(0) => break,
                Ok(n) => buf.extend_from_slice(&chunk[..n]),
                Err(_) => {}
            }
        }
        let _ = tx.send(report);
    });

    (url, rx)
}

fn script_var(tree: &SceneTree, node: NodeId, name: &str) -> Option<Variant> {
    tree.get_script(node)?
        .list_properties()
        .into_iter()
        .find(|p| p.name == name)
        .map(|p| p.default_value)
}

/// **The acceptance test.** A script creates the peer, polls it to OPEN, round-
/// trips text and binary, reads the negotiated subprotocol, and closes with a
/// code/reason the far end observes.
#[test]
fn websocket_peer_script_e2e() {
    let (url, reports) = spawn_echo_server();

    // Every step of the exchange is driven from GDScript — the point of the
    // bead is that a *script* can reach this class at all.
    let src = format!(
        "\
extends Node2D
var ws = null
var phase = 0
var state = -1
var protocol = \"\"
var text_echo = \"\"
var text_was_string = false
var binary_len = 0
var binary_was_string = true
func _ready():
    self.ws = WebSocketPeer.new()
    self.ws.set_supported_protocols([\"chat\"])
    self.ws.connect_to_url(\"{url}\")
func _process(delta):
    self.ws.poll()
    self.state = self.ws.get_ready_state()
    if self.state != 1:
        return
    if self.phase == 0:
        self.protocol = self.ws.get_selected_protocol()
        self.ws.send_text(\"hello\")
        self.phase = 1
        return
    if self.ws.get_available_packet_count() > 0:
        if self.phase == 1:
            self.text_echo = self.ws.get_packet().get_string_from_utf8()
            self.text_was_string = self.ws.was_string_packet()
            self.ws.send(PackedByteArray([1, 2, 3, 4]), 1)
            self.phase = 2
            return
        if self.phase == 2:
            self.binary_len = self.ws.get_packet().size()
            self.binary_was_string = self.ws.was_string_packet()
            self.ws.close(4000, \"bye\")
            self.phase = 3
"
    );

    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let main = tree.add_child(root, Node::new("Main", "Node2D")).unwrap();
    tree.attach_script(
        main,
        Box::new(GDScriptNodeInstance::from_source(&src, main).unwrap()),
    );
    // `MainLoop::step` drives `_process`, not `_ready` — the ready pass is an
    // explicit call, as `gdscript_preload_instantiate_spawns_live_scene` does.
    // Without it the peer is never constructed and every assertion below reads
    // its initial value.
    tree.process_script_ready(main);

    let mut ml = MainLoop::new(tree);
    // Real sockets: step until the script reaches its last phase rather than a
    // fixed count, with a wall-clock bound so a hang fails instead of spinning.
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        ml.step(1.0 / 60.0);
        let phase = script_var(ml.tree(), main, "phase");
        if matches!(phase, Some(Variant::Int(3))) || Instant::now() > deadline {
            break;
        }
        thread::sleep(Duration::from_millis(2));
    }

    assert_eq!(
        script_var(ml.tree(), main, "state"),
        Some(Variant::Int(1)),
        "the script must drive the peer to STATE_OPEN (1); \
         a script that cannot construct WebSocketPeer never leaves -1"
    );
    assert_eq!(
        script_var(ml.tree(), main, "protocol"),
        Some(Variant::String("chat".into())),
        "get_selected_protocol must report the subprotocol the server accepted"
    );
    assert_eq!(
        script_var(ml.tree(), main, "text_echo"),
        Some(Variant::String("hello".into())),
        "send_text must round-trip through the server"
    );
    assert_eq!(
        script_var(ml.tree(), main, "text_was_string"),
        Some(Variant::Bool(true)),
        "a text echo must report was_string_packet() == true"
    );
    assert_eq!(
        script_var(ml.tree(), main, "binary_len"),
        Some(Variant::Int(4)),
        "the 4-byte binary packet must round-trip intact"
    );
    assert_eq!(
        script_var(ml.tree(), main, "binary_was_string"),
        Some(Variant::Bool(false)),
        "a binary echo must report was_string_packet() == false"
    );

    // The far end sees the script's close code and reason.
    let report = reports
        .recv_timeout(Duration::from_secs(5))
        .expect("echo server should report the close it observed");
    assert_eq!(
        report.close_code, 4000,
        "close(4000, \"bye\") must surface code 4000 on the far end"
    );
    assert_eq!(
        report.close_reason, "bye",
        "close(4000, \"bye\") must surface the reason on the far end"
    );
}
