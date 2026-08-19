//! WebSocketMultiplayerPeer — a `MultiplayerPeer` transport over RFC 6455 (pat-nzgdu.50).
//!
//! Mirrors Godot's `WebSocketMultiplayerPeer` (modules/websocket/websocket_multiplayer_peer.cpp):
//! `create_server` listens and completes the opening handshake for each inbound
//! connection, `create_client` dials a `ws://` URL, and both sides reach
//! `CONNECTION_CONNECTED` by pumping [`NetworkPeer::poll`] — no threads, so a
//! single-threaded test can drive both ends.
//!
//! The server assigns each client a peer id and pushes it down as a `SET_ID`
//! control message; the client stays `Connecting` until it arrives, matching
//! Godot's "connection is established once the id is assigned" semantics.
//! Peer bookkeeping and `peer_connected` / `peer_disconnected` signals are
//! delegated to [`MultiplayerPeerState`], so this module only owns the socket
//! and framing layers.
//!
//! Reuses the frame codec + handshake helpers from [`crate::websocket_peer`],
//! which keeps this transport dependency-free. Only plaintext `ws://` is wired:
//! `wss://` is rejected by [`WebSocketPeer::connect_to_url`], and the browser
//! (wasm) transport is not part of this module.

use std::collections::HashMap;
use std::io::{ErrorKind, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream, ToSocketAddrs};
use std::time::{Duration, Instant};

use crate::multiplayer_peer_state::{MultiplayerPeerSignal, MultiplayerPeerState};
use crate::network::{
    ConnectionStatus, NetworkError, NetworkPeer, Packet, PeerId, Result, TransferMode,
};
use crate::websocket_peer::{
    decode_frame, encode_frame, parse_handshake_key, parse_handshake_protocols,
    select_handshake_protocol, websocket_handshake_response_with_protocol, Frame, Opcode,
    WebSocketPeer, WebSocketState, WriteMode,
};

/// Control message: the server assigning this client its peer id.
/// Payload: `[TAG_SET_ID][id: u32 BE]`.
const TAG_SET_ID: u8 = 0x01;
/// Data message. Payload: `[TAG_DATA][sender: u32 BE][channel: u8][bytes…]`.
const TAG_DATA: u8 = 0x02;

/// Smallest valid `TAG_DATA` envelope (tag + sender + channel, empty body).
const DATA_HEADER_LEN: usize = 6;

/// First peer id handed out to a client (1 is reserved for the server).
const FIRST_CLIENT_ID: u32 = 2;

// ---------------------------------------------------------------------------
// Envelope codec
// ---------------------------------------------------------------------------

fn encode_set_id(id: u32) -> Vec<u8> {
    let mut out = Vec::with_capacity(5);
    out.push(TAG_SET_ID);
    out.extend_from_slice(&id.to_be_bytes());
    out
}

fn encode_data(sender: PeerId, channel: u8, data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(DATA_HEADER_LEN + data.len());
    out.push(TAG_DATA);
    out.extend_from_slice(&sender.0.to_be_bytes());
    out.push(channel);
    out.extend_from_slice(data);
    out
}

/// Decodes a `TAG_DATA` envelope. `from` identifies the connection it arrived
/// on and is used as the sender when the wire carries no explicit id.
fn decode_data(from: PeerId, payload: &[u8]) -> Option<Packet> {
    if payload.len() < DATA_HEADER_LEN || payload[0] != TAG_DATA {
        return None;
    }
    let sender = u32::from_be_bytes([payload[1], payload[2], payload[3], payload[4]]);
    Some(Packet {
        sender: if sender == 0 { from } else { PeerId(sender) },
        channel: payload[5],
        data: payload[DATA_HEADER_LEN..].to_vec(),
        transfer_mode: TransferMode::Reliable,
    })
}

// ---------------------------------------------------------------------------
// Socket helpers
// ---------------------------------------------------------------------------

/// Writes every byte to a non-blocking stream, spinning through `WouldBlock`.
fn write_all_nonblocking(mut stream: &TcpStream, mut data: &[u8]) -> std::io::Result<()> {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !data.is_empty() {
        match stream.write(data) {
            Ok(0) => {
                return Err(std::io::Error::new(
                    ErrorKind::WriteZero,
                    "write returned 0",
                ));
            }
            Ok(n) => data = &data[n..],
            Err(ref e) if e.kind() == ErrorKind::WouldBlock => {
                if Instant::now() >= deadline {
                    return Err(std::io::Error::new(ErrorKind::TimedOut, "write timed out"));
                }
                std::thread::sleep(Duration::from_millis(1));
            }
            Err(ref e) if e.kind() == ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

/// Drains everything currently readable into `buf`. Returns `true` on EOF.
fn read_available(mut stream: &TcpStream, buf: &mut Vec<u8>) -> std::io::Result<bool> {
    let mut tmp = [0u8; 4096];
    loop {
        match stream.read(&mut tmp) {
            Ok(0) => return Ok(true),
            Ok(n) => buf.extend_from_slice(&tmp[..n]),
            Err(ref e) if e.kind() == ErrorKind::WouldBlock => return Ok(false),
            Err(ref e) if e.kind() == ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        }
    }
}

/// Index of the first `\r\n\r\n` in `buf` (end of an HTTP header block).
fn find_header_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n")
}

// ---------------------------------------------------------------------------
// Connections
// ---------------------------------------------------------------------------

/// A handshaked server-side client connection.
struct ServerConn {
    stream: TcpStream,
    in_buf: Vec<u8>,
}

/// An accepted socket whose opening handshake has not completed yet.
struct PendingConn {
    stream: TcpStream,
    buf: Vec<u8>,
}

// ---------------------------------------------------------------------------
// WebSocketMultiplayerPeer
// ---------------------------------------------------------------------------

/// A `MultiplayerPeer` speaking WebSocket, usable as either server or client.
pub struct WebSocketMultiplayerPeer {
    id: PeerId,
    status: ConnectionStatus,
    listener: Option<TcpListener>,
    conns: HashMap<PeerId, ServerConn>,
    pending: Vec<PendingConn>,
    next_peer_id: u32,
    inbox: Vec<Packet>,
    client: Option<WebSocketPeer>,
    is_server_role: bool,
    peer_state: MultiplayerPeerState,
    /// Subprotocols this server will accept from a connecting client
    /// (pat-0q2kx.96). Empty — the default — means the server names none, so
    /// the handshake omits `Sec-WebSocket-Protocol` entirely, exactly as before.
    supported_protocols: Vec<String>,
}

impl WebSocketMultiplayerPeer {
    /// Creates an unbound peer (disconnected).
    pub fn new() -> Self {
        Self {
            id: PeerId(0),
            status: ConnectionStatus::Disconnected,
            listener: None,
            conns: HashMap::new(),
            pending: Vec::new(),
            next_peer_id: FIRST_CLIENT_ID,
            inbox: Vec::new(),
            client: None,
            is_server_role: false,
            peer_state: MultiplayerPeerState::new(),
            supported_protocols: Vec::new(),
        }
    }

    /// Sets the subprotocols this server accepts, in the server's own order of
    /// support (pat-0q2kx.96).
    ///
    /// The client's preference order wins when both name several, per RFC 6455
    /// §4.2.2 — see [`select_handshake_protocol`].
    pub fn set_supported_protocols(&mut self, protocols: &[String]) {
        self.supported_protocols = protocols.to_vec();
    }

    /// The subprotocols this server accepts.
    pub fn get_supported_protocols(&self) -> &[String] {
        &self.supported_protocols
    }

    /// Listens on `addr` (e.g. `"127.0.0.1:0"`) and returns the bound address.
    ///
    /// The peer is immediately `Connected` with id [`PeerId::SERVER`]; clients
    /// are accepted and handshaked by [`NetworkPeer::poll`].
    pub fn create_server<A: ToSocketAddrs>(&mut self, addr: A) -> Result<SocketAddr> {
        self.close();
        let listener =
            TcpListener::bind(addr).map_err(|e| NetworkError::Transport(e.to_string()))?;
        listener
            .set_nonblocking(true)
            .map_err(|e| NetworkError::Transport(e.to_string()))?;
        let local = listener
            .local_addr()
            .map_err(|e| NetworkError::Transport(e.to_string()))?;
        self.listener = Some(listener);
        self.id = PeerId::SERVER;
        self.is_server_role = true;
        self.status = ConnectionStatus::Connected;
        self.next_peer_id = FIRST_CLIENT_ID;
        Ok(local)
    }

    /// Dials a `ws://` URL. The peer stays [`ConnectionStatus::Connecting`]
    /// until [`NetworkPeer::poll`] completes the handshake **and** the server
    /// assigns a peer id.
    pub fn create_client(&mut self, url: &str) -> Result<()> {
        self.close();
        let mut client = WebSocketPeer::new();
        client
            .connect_to_url(url)
            .map_err(|e| NetworkError::Transport(e.to_string()))?;
        self.client = Some(client);
        self.is_server_role = false;
        self.status = ConnectionStatus::Connecting;
        self.id = PeerId(0);
        Ok(())
    }

    /// Local address when this peer is a server.
    pub fn local_addr(&self) -> Option<SocketAddr> {
        self.listener.as_ref().and_then(|l| l.local_addr().ok())
    }

    /// Ids of the currently connected peers.
    pub fn get_peers(&self) -> Vec<i32> {
        self.peer_state.get_peers()
    }

    /// Number of connected peers.
    /// Godot `WebSocketMultiplayerPeer.get_peer_address(id)`
    /// (websocket_multiplayer_peer.cpp:472-475), as a string.
    ///
    /// ⚠️ An unknown peer id answers with the EMPTY string, not an error.
    /// Godot's `ERR_FAIL_COND_V(!peers_map.has(id), IPAddress())` returns a
    /// default-constructed `IPAddress`, which is the invalid/empty one — so a
    /// script polling an id that has just disconnected gets "" rather than a
    /// raise. Godot's return type is `IPAddress`; Patina has no such Variant, and
    /// GDScript marshals it as a String either way.
    ///
    /// Only the SERVER role can answer: a client holds one socket to the server,
    /// not a roster, which is why `conns` is the only source consulted.
    pub fn get_peer_address(&self, peer: PeerId) -> String {
        self.conns
            .get(&peer)
            .and_then(|c| c.stream.peer_addr().ok())
            .map(|a| a.ip().to_string())
            .unwrap_or_default()
    }

    /// Godot `WebSocketMultiplayerPeer.get_peer_port(id)`
    /// (websocket_multiplayer_peer.cpp:477-480).
    ///
    /// ⚠️ An unknown peer id answers `0`, matching Godot's
    /// `ERR_FAIL_COND_V(!peers_map.has(id), 0)`.
    pub fn get_peer_port(&self, peer: PeerId) -> i64 {
        self.conns
            .get(&peer)
            .and_then(|c| c.stream.peer_addr().ok())
            .map(|a| i64::from(a.port()))
            .unwrap_or(0)
    }

    pub fn get_peer_count(&self) -> usize {
        self.peer_state.get_peer_count()
    }

    /// Drains queued `peer_connected` / `peer_disconnected` signals.
    pub fn drain_signals(&mut self) -> Vec<MultiplayerPeerSignal> {
        self.peer_state.drain_signals()
    }

    /// Accepts inbound sockets; they stay pending until their handshake lands.
    fn accept_new_clients(&mut self) {
        let Some(listener) = self.listener.as_ref() else {
            return;
        };
        loop {
            match listener.accept() {
                Ok((stream, _)) => {
                    let _ = stream.set_nonblocking(true);
                    self.pending.push(PendingConn {
                        stream,
                        buf: Vec::new(),
                    });
                }
                Err(ref e) if e.kind() == ErrorKind::WouldBlock => break,
                Err(_) => break,
            }
        }
    }

    /// Completes the opening handshake for any pending socket that has sent a
    /// full header block, then promotes it to a peer and assigns it an id.
    fn pump_pending_handshakes(&mut self) {
        let mut still_pending = Vec::new();
        for mut conn in std::mem::take(&mut self.pending) {
            let eof = match read_available(&conn.stream, &mut conn.buf) {
                Ok(eof) => eof,
                Err(_) => continue,
            };
            let Some(pos) = find_header_end(&conn.buf) else {
                if !eof {
                    still_pending.push(conn);
                }
                continue;
            };
            let head = String::from_utf8_lossy(&conn.buf[..pos + 4]).to_string();
            conn.buf.drain(..pos + 4);

            let Some(key) = parse_handshake_key(&head) else {
                continue;
            };
            // Echo back whichever subprotocol both sides name, so the client's
            // `get_selected_protocol()` reports something real (pat-0q2kx.96).
            // With no configured protocols this selects `None` and the response
            // is byte-identical to the pre-negotiation one.
            let offered = parse_handshake_protocols(&head);
            let selected = select_handshake_protocol(&offered, &self.supported_protocols);
            let response = websocket_handshake_response_with_protocol(&key, selected.as_deref());
            if write_all_nonblocking(&conn.stream, response.as_bytes()).is_err() {
                continue;
            }

            let id = PeerId(self.next_peer_id);
            self.next_peer_id = self.next_peer_id.saturating_add(1);
            let set_id = encode_frame(&Frame::new(Opcode::Binary, encode_set_id(id.0)), None);
            if write_all_nonblocking(&conn.stream, &set_id).is_err() {
                continue;
            }

            self.conns.insert(
                id,
                ServerConn {
                    stream: conn.stream,
                    in_buf: conn.buf,
                },
            );
            self.peer_state.add_peer(id.0 as i32);
        }
        self.pending = still_pending;
    }

    /// Reads frames from every handshaked client connection.
    fn pump_server_reads(&mut self) {
        let ids: Vec<PeerId> = self.conns.keys().copied().collect();
        let mut received: Vec<(PeerId, Vec<u8>)> = Vec::new();
        let mut dropped: Vec<PeerId> = Vec::new();

        for pid in ids {
            let Some(conn) = self.conns.get_mut(&pid) else {
                continue;
            };
            let mut drop_conn = match read_available(&conn.stream, &mut conn.in_buf) {
                Ok(eof) => eof,
                Err(_) => true,
            };
            loop {
                match decode_frame(&conn.in_buf) {
                    Ok(Some((frame, consumed))) => {
                        conn.in_buf.drain(..consumed);
                        match frame.opcode {
                            Opcode::Binary | Opcode::Text => received.push((pid, frame.payload)),
                            Opcode::Ping => {
                                let pong =
                                    encode_frame(&Frame::new(Opcode::Pong, frame.payload), None);
                                let _ = write_all_nonblocking(&conn.stream, &pong);
                            }
                            Opcode::Close => {
                                drop_conn = true;
                                break;
                            }
                            Opcode::Continuation | Opcode::Pong => {}
                        }
                    }
                    Ok(None) => break,
                    Err(_) => {
                        drop_conn = true;
                        break;
                    }
                }
            }
            if drop_conn {
                dropped.push(pid);
            }
        }

        for (pid, payload) in received {
            if let Some(packet) = decode_data(pid, &payload) {
                self.inbox.push(packet);
            }
        }
        for pid in dropped {
            self.conns.remove(&pid);
            self.peer_state.remove_peer(pid.0 as i32);
        }
    }

    /// Drives the client socket and applies inbound control/data messages.
    fn pump_client(&mut self) {
        let mut payloads: Vec<Vec<u8>> = Vec::new();
        let state = {
            let Some(client) = self.client.as_mut() else {
                return;
            };
            client.poll();
            while client.get_available_packet_count() > 0 {
                payloads.push(client.get_packet());
            }
            client.get_ready_state()
        };

        for payload in payloads {
            if payload.is_empty() {
                continue;
            }
            match payload[0] {
                TAG_SET_ID if payload.len() >= 5 => {
                    let id = u32::from_be_bytes([payload[1], payload[2], payload[3], payload[4]]);
                    self.id = PeerId(id);
                    self.status = ConnectionStatus::Connected;
                    self.peer_state.add_peer(PeerId::SERVER.0 as i32);
                }
                TAG_DATA => {
                    if let Some(packet) = decode_data(PeerId::SERVER, &payload) {
                        self.inbox.push(packet);
                    }
                }
                _ => {}
            }
        }

        if state == WebSocketState::Closed {
            self.status = ConnectionStatus::Disconnected;
            self.peer_state.remove_peer(PeerId::SERVER.0 as i32);
        }
    }
}

impl Default for WebSocketMultiplayerPeer {
    fn default() -> Self {
        Self::new()
    }
}

impl NetworkPeer for WebSocketMultiplayerPeer {
    /// The trait half of [`WebSocketMultiplayerPeer::drain_signals`]
    /// (pat-s3rrv.81).
    ///
    /// ⚠️ Both spellings are needed. The inherent method is what
    /// `gdplatform`'s own tests call on a concrete peer; this one is what
    /// `MultiplayerAPI::poll` can reach through `Box<dyn NetworkPeer>`, and
    /// without it the API silently used the trait's empty default — the
    /// handshake completed and no script ever heard `peer_connected`.
    fn drain_signals(&mut self) -> Vec<crate::multiplayer_peer_state::MultiplayerPeerSignal> {
        self.peer_state.drain_signals()
    }

    fn send_packet(&mut self, to: PeerId, packet: Packet) -> Result<()> {
        if self.status != ConnectionStatus::Connected {
            return Err(NetworkError::Transport("not connected".into()));
        }
        let body = encode_data(packet.sender, packet.channel, &packet.data);
        if self.is_server_role {
            let conn = self
                .conns
                .get(&to)
                .ok_or(NetworkError::PeerUnreachable(to))?;
            let bytes = encode_frame(&Frame::new(Opcode::Binary, body), None);
            write_all_nonblocking(&conn.stream, &bytes)
                .map_err(|e| NetworkError::Transport(e.to_string()))
        } else {
            let client = self
                .client
                .as_mut()
                .ok_or(NetworkError::PeerUnreachable(PeerId::SERVER))?;
            client
                .send(&body, WriteMode::Binary)
                .map_err(|e| NetworkError::Transport(e.to_string()))
        }
    }

    fn poll(&mut self) -> Vec<Packet> {
        if self.is_server_role {
            self.accept_new_clients();
            self.pump_pending_handshakes();
            self.pump_server_reads();
        } else {
            self.pump_client();
        }
        std::mem::take(&mut self.inbox)
    }

    fn get_unique_id(&self) -> PeerId {
        self.id
    }

    fn is_server(&self) -> bool {
        self.is_server_role || self.id == PeerId::SERVER
    }

    fn get_connection_status(&self) -> ConnectionStatus {
        self.status
    }

    fn close(&mut self) {
        if let Some(mut client) = self.client.take() {
            client.close(1000, "");
        }
        self.listener = None;
        self.conns.clear();
        self.pending.clear();
        self.inbox.clear();
        self.peer_state.reset();
        self.status = ConnectionStatus::Disconnected;
        self.id = PeerId(0);
        self.is_server_role = false;
        self.next_peer_id = FIRST_CLIENT_ID;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_id_envelope_roundtrips() {
        let bytes = encode_set_id(7);
        assert_eq!(bytes[0], TAG_SET_ID);
        assert_eq!(
            u32::from_be_bytes([bytes[1], bytes[2], bytes[3], bytes[4]]),
            7
        );
    }

    #[test]
    fn data_envelope_roundtrips() {
        let bytes = encode_data(PeerId(9), 3, b"payload");
        let packet = decode_data(PeerId(9), &bytes).expect("decodes");
        assert_eq!(packet.sender, PeerId(9));
        assert_eq!(packet.channel, 3);
        assert_eq!(packet.data, b"payload".to_vec());
    }

    #[test]
    fn data_envelope_allows_empty_body() {
        let bytes = encode_data(PeerId(2), 0, b"");
        let packet = decode_data(PeerId(2), &bytes).expect("decodes");
        assert!(packet.data.is_empty());
    }

    #[test]
    fn decode_data_rejects_short_or_mistagged_payloads() {
        assert!(decode_data(PeerId(2), &[]).is_none());
        assert!(decode_data(PeerId(2), &[TAG_DATA, 0, 0]).is_none());
        assert!(decode_data(PeerId(2), &encode_set_id(2)).is_none());
    }

    #[test]
    fn decode_data_falls_back_to_connection_id_when_sender_is_zero() {
        let bytes = encode_data(PeerId(0), 1, b"x");
        let packet = decode_data(PeerId(4), &bytes).expect("decodes");
        assert_eq!(packet.sender, PeerId(4));
    }

    #[test]
    fn new_peer_is_disconnected() {
        let peer = WebSocketMultiplayerPeer::new();
        assert_eq!(peer.get_connection_status(), ConnectionStatus::Disconnected);
        assert_eq!(peer.get_unique_id(), PeerId(0));
        assert!(!peer.is_server());
    }

    #[test]
    fn create_server_binds_and_reports_server_identity() {
        let mut server = WebSocketMultiplayerPeer::new();
        let addr = server.create_server("127.0.0.1:0").unwrap();
        assert_eq!(addr.ip().to_string(), "127.0.0.1");
        assert_ne!(addr.port(), 0);
        assert!(server.is_server());
        assert_eq!(server.get_unique_id(), PeerId::SERVER);
        assert_eq!(server.get_connection_status(), ConnectionStatus::Connected);
    }

    #[test]
    fn wss_url_is_rejected_until_tls_lands() {
        let mut client = WebSocketMultiplayerPeer::new();
        // TLS is out of scope for this transport slice; surface it as an error
        // rather than silently connecting in plaintext.
        assert!(client.create_client("wss://example.com/").is_err());
    }

    #[test]
    fn send_before_connected_errors() {
        let mut client = WebSocketMultiplayerPeer::new();
        let err = client.send_packet(
            PeerId::SERVER,
            Packet {
                sender: PeerId(0),
                channel: 0,
                data: b"x".to_vec(),
                transfer_mode: TransferMode::Reliable,
            },
        );
        assert!(err.is_err());
    }

    #[test]
    fn find_header_end_locates_crlf_pair() {
        assert_eq!(find_header_end(b"GET / HTTP/1.1\r\n\r\nbody"), Some(14));
        assert_eq!(find_header_end(b"GET / HTTP/1.1\r\n"), None);
    }
}
