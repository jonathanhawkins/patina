//! ENetMultiplayerPeer — real UDP multiplayer peer (pat-p0s82.3).
//!
//! Uses `std::net::UdpSocket` with a minimal framing protocol that gives
//! Godot-compatible transfer modes real delivery semantics:
//!
//! | TransferMode | Behavior |
//! |--------------|----------|
//! | Unreliable   | Fire-and-forget datagram (may drop; no retransmit) |
//! | Reliable     | Seq + ACK retransmit until acknowledged |
//! | Ordered      | Reliable delivery + in-order app delivery |
//!
//! This is intentionally **not** a full ENet port; it is a UDP peer that
//! honors TransferMode for LAN/loopback multiplayer and ClassDB parity.
//! Bandwidth limits / multi-host mesh are out of scope.

use std::collections::{HashMap, VecDeque};
use std::net::{SocketAddr, ToSocketAddrs, UdpSocket};
use std::time::{Duration, Instant};

use crate::network::{
    ConnectionStatus, NetworkError, NetworkPeer, Packet, PeerId, Result, TransferMode,
};

const MAGIC: &[u8; 4] = b"PEN1";
const FLAG_MODE_MASK: u8 = 0b0000_0011;
const FLAG_ACK: u8 = 0b0000_0100;
const FLAG_HELLO: u8 = 0b0000_1000;
const FLAG_WELCOME: u8 = 0b0001_0000;
const FLAG_DATA: u8 = 0b0010_0000;

const MODE_RELIABLE: u8 = 0;
const MODE_UNRELIABLE: u8 = 1;
const MODE_ORDERED: u8 = 2;

const RETRANSMIT_MS: u64 = 30;
const MAX_PACKET: usize = 60_000;

/// ClassDB name: `ENetMultiplayerPeer`.
///
/// Real UDP peer with reliable / unreliable / ordered channels.
pub struct ENetMultiplayerPeer {
    id: PeerId,
    status: ConnectionStatus,
    socket: Option<UdpSocket>,
    /// server role
    is_server: bool,
    /// peer_id → address (server: clients; client: server as id 1)
    peers: HashMap<PeerId, SocketAddr>,
    /// address → peer_id (server accept path)
    addr_to_id: HashMap<SocketAddr, PeerId>,
    next_peer_id: u32,
    inbox: VecDeque<Packet>,
    /// Outbound reliable frames waiting for ACK: (to, frame_bytes, last_sent, seq, channel)
    pending_reliable: Vec<PendingReliable>,
    /// Next outbound sequence number per (peer, channel).
    next_seq: HashMap<(u32, u8), u32>,
    /// Highest contiguous delivered ordered seq per (peer, channel).
    ordered_delivered: HashMap<(u32, u8), u32>,
    /// Ordered packets held until gap filled: (peer, channel) → seq → packet
    ordered_buffer: HashMap<(u32, u8), HashMap<u32, Packet>>,
    /// ACKs already received (for ignoring dups) — set of (peer, channel, seq)
    acked: HashMap<(u32, u8), u32>,
    /// Client: server address while connecting
    server_addr: Option<SocketAddr>,
}

struct PendingReliable {
    to: PeerId,
    addr: SocketAddr,
    bytes: Vec<u8>,
    last_sent: Instant,
    seq: u32,
    channel: u8,
}

impl ENetMultiplayerPeer {
    /// Creates an unbound peer (disconnected).
    pub fn new() -> Self {
        Self {
            id: PeerId(0),
            status: ConnectionStatus::Disconnected,
            socket: None,
            is_server: false,
            peers: HashMap::new(),
            addr_to_id: HashMap::new(),
            next_peer_id: 2,
            inbox: VecDeque::new(),
            pending_reliable: Vec::new(),
            next_seq: HashMap::new(),
            ordered_delivered: HashMap::new(),
            ordered_buffer: HashMap::new(),
            acked: HashMap::new(),
            server_addr: None,
        }
    }

    /// Starts a server listening on `addr` (e.g. `"127.0.0.1:0"`).
    pub fn create_server<A: ToSocketAddrs>(&mut self, addr: A) -> Result<SocketAddr> {
        self.close();
        let sock = UdpSocket::bind(addr).map_err(|e| NetworkError::Transport(e.to_string()))?;
        sock.set_nonblocking(true)
            .map_err(|e| NetworkError::Transport(e.to_string()))?;
        let local = sock
            .local_addr()
            .map_err(|e| NetworkError::Transport(e.to_string()))?;
        self.socket = Some(sock);
        self.id = PeerId::SERVER;
        self.is_server = true;
        self.status = ConnectionStatus::Connected;
        self.next_peer_id = 2;
        Ok(local)
    }

    /// Connects to a remote server as a client. Completes via [`NetworkPeer::poll`].
    pub fn create_client<A: ToSocketAddrs>(&mut self, addr: A) -> Result<()> {
        self.close();
        let mut addrs = addr
            .to_socket_addrs()
            .map_err(|e| NetworkError::Transport(e.to_string()))?;
        let server = addrs
            .next()
            .ok_or_else(|| NetworkError::Transport("no address".into()))?;
        let bind_addr = if server.is_ipv4() {
            "0.0.0.0:0"
        } else {
            "[::]:0"
        };
        let sock =
            UdpSocket::bind(bind_addr).map_err(|e| NetworkError::Transport(e.to_string()))?;
        sock.set_nonblocking(true)
            .map_err(|e| NetworkError::Transport(e.to_string()))?;
        // Send HELLO
        let hello = encode_control(FLAG_HELLO, 0, PeerId(0), 0, &[]);
        sock.send_to(&hello, server)
            .map_err(|e| NetworkError::Transport(e.to_string()))?;
        self.socket = Some(sock);
        self.server_addr = Some(server);
        self.is_server = false;
        self.status = ConnectionStatus::Connecting;
        self.id = PeerId(0);
        Ok(())
    }

    /// Local address (server bind or client ephemeral).
    pub fn local_addr(&self) -> Option<SocketAddr> {
        self.socket.as_ref().and_then(|s| s.local_addr().ok())
    }

    /// True when this peer uses UDP (always for this type).
    pub fn is_udp(&self) -> bool {
        true
    }

    fn next_sequence(&mut self, peer: PeerId, channel: u8) -> u32 {
        let key = (peer.0, channel);
        let s = self.next_seq.entry(key).or_insert(1);
        let v = *s;
        *s = s.wrapping_add(1);
        if *s == 0 {
            *s = 1;
        }
        v
    }

    fn send_raw(&self, addr: SocketAddr, bytes: &[u8]) -> Result<()> {
        let sock = self
            .socket
            .as_ref()
            .ok_or_else(|| NetworkError::Transport("no socket".into()))?;
        sock.send_to(bytes, addr)
            .map_err(|e| NetworkError::Transport(e.to_string()))?;
        Ok(())
    }

    fn retransmit_pending(&mut self) {
        let now = Instant::now();
        for p in &mut self.pending_reliable {
            if now.duration_since(p.last_sent) >= Duration::from_millis(RETRANSMIT_MS) {
                if let Some(sock) = self.socket.as_ref() {
                    let _ = sock.send_to(&p.bytes, p.addr);
                }
                p.last_sent = now;
            }
        }
    }

    fn handle_datagram(&mut self, src: SocketAddr, buf: &[u8]) {
        let Some(frame) = decode_frame(buf) else {
            return;
        };

        if frame.flags & FLAG_HELLO != 0 && self.is_server {
            // Assign peer id
            let id = PeerId(self.next_peer_id);
            self.next_peer_id = self.next_peer_id.saturating_add(1);
            self.peers.insert(id, src);
            self.addr_to_id.insert(src, id);
            let welcome = encode_control(FLAG_WELCOME, 0, id, 0, &id.0.to_be_bytes());
            let _ = self.send_raw(src, &welcome);
            return;
        }

        if frame.flags & FLAG_WELCOME != 0 && !self.is_server {
            if frame.payload.len() >= 4 {
                let assigned = u32::from_be_bytes(frame.payload[0..4].try_into().unwrap());
                self.id = PeerId(assigned);
                if let Some(server) = self.server_addr {
                    self.peers.insert(PeerId::SERVER, server);
                }
                self.status = ConnectionStatus::Connected;
            }
            return;
        }

        if frame.flags & FLAG_ACK != 0 {
            // Remove matching pending reliable
            let seq = frame.seq;
            let ch = frame.channel;
            let from = frame.sender;
            self.pending_reliable
                .retain(|p| !(p.seq == seq && p.channel == ch && p.to == from));
            let key = (from.0, ch);
            let entry = self.acked.entry(key).or_insert(0);
            if seq > *entry {
                *entry = seq;
            }
            return;
        }

        if frame.flags & FLAG_DATA == 0 {
            return;
        }

        let mode = match frame.flags & FLAG_MODE_MASK {
            MODE_UNRELIABLE => TransferMode::Unreliable,
            MODE_ORDERED => TransferMode::Ordered,
            _ => TransferMode::Reliable,
        };

        // ACK reliable/ordered
        if mode != TransferMode::Unreliable {
            let ack = encode_control(FLAG_ACK, frame.channel, self.id, frame.seq, &[]);
            let _ = self.send_raw(src, &ack);
        }

        // Map sender: prefer frame.sender; on server, bind addr
        let sender = if self.is_server {
            if let Some(&pid) = self.addr_to_id.get(&src) {
                pid
            } else {
                frame.sender
            }
        } else {
            frame.sender
        };

        let packet = Packet {
            sender,
            channel: frame.channel,
            data: frame.payload,
            transfer_mode: mode,
        };

        match mode {
            TransferMode::Unreliable | TransferMode::Reliable => {
                self.inbox.push_back(packet);
            }
            TransferMode::Ordered => {
                let key = (sender.0, frame.channel);
                let delivered = self.ordered_delivered.entry(key).or_insert(0);
                if frame.seq <= *delivered {
                    // duplicate
                    return;
                }
                if frame.seq == *delivered + 1 {
                    *delivered = frame.seq;
                    self.inbox.push_back(packet);
                    // flush buffer
                    loop {
                        let next = *delivered + 1;
                        if let Some(buf) = self.ordered_buffer.get_mut(&key) {
                            if let Some(p) = buf.remove(&next) {
                                *delivered = next;
                                self.inbox.push_back(p);
                                continue;
                            }
                        }
                        break;
                    }
                } else {
                    self.ordered_buffer
                        .entry(key)
                        .or_default()
                        .insert(frame.seq, packet);
                }
            }
        }
    }

    fn drain_socket(&mut self) {
        let Some(sock) = self.socket.as_ref() else {
            return;
        };
        let mut buf = [0u8; 65535];
        // Collect first to avoid borrow issues
        let mut received = Vec::new();
        loop {
            match sock.recv_from(&mut buf) {
                Ok((n, src)) => received.push((src, buf[..n].to_vec())),
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(_) => break,
            }
        }
        drop(sock);
        for (src, data) in received {
            self.handle_datagram(src, &data);
        }
    }
}

impl Default for ENetMultiplayerPeer {
    fn default() -> Self {
        Self::new()
    }
}

impl NetworkPeer for ENetMultiplayerPeer {
    fn send_packet(&mut self, to: PeerId, packet: Packet) -> Result<()> {
        if self.status != ConnectionStatus::Connected {
            return Err(NetworkError::Transport("not connected".into()));
        }
        let addr = if to == PeerId(0) || to.0 == 0 {
            // broadcast to all known peers
            None
        } else {
            Some(
                *self
                    .peers
                    .get(&to)
                    .ok_or_else(|| NetworkError::Transport(format!("unknown peer {to}")))?,
            )
        };

        let mode_bits = match packet.transfer_mode {
            TransferMode::Reliable => MODE_RELIABLE,
            TransferMode::Unreliable => MODE_UNRELIABLE,
            TransferMode::Ordered => MODE_ORDERED,
        };

        let seq = if packet.transfer_mode == TransferMode::Unreliable {
            0
        } else {
            self.next_sequence(to, packet.channel)
        };

        let flags = FLAG_DATA | mode_bits;
        let bytes = encode_control(flags, packet.channel, self.id, seq, &packet.data);
        if bytes.len() > MAX_PACKET {
            return Err(NetworkError::Transport("packet too large".into()));
        }

        let targets: Vec<(PeerId, SocketAddr)> = if let Some(a) = addr {
            vec![(to, a)]
        } else {
            self.peers.iter().map(|(id, a)| (*id, *a)).collect()
        };

        for (peer_id, a) in targets {
            self.send_raw(a, &bytes)?;
            if packet.transfer_mode != TransferMode::Unreliable {
                self.pending_reliable.push(PendingReliable {
                    to: peer_id,
                    addr: a,
                    bytes: bytes.clone(),
                    last_sent: Instant::now(),
                    seq,
                    channel: packet.channel,
                });
            }
        }
        Ok(())
    }

    fn poll(&mut self) -> Vec<Packet> {
        self.drain_socket();
        self.retransmit_pending();
        // Second drain for ACKs / retransmit responses
        self.drain_socket();
        self.inbox.drain(..).collect()
    }

    fn get_unique_id(&self) -> PeerId {
        self.id
    }

    fn is_server(&self) -> bool {
        self.is_server && self.id == PeerId::SERVER
    }

    fn get_connection_status(&self) -> ConnectionStatus {
        self.status
    }

    fn close(&mut self) {
        self.socket = None;
        self.peers.clear();
        self.addr_to_id.clear();
        self.inbox.clear();
        self.pending_reliable.clear();
        self.next_seq.clear();
        self.ordered_delivered.clear();
        self.ordered_buffer.clear();
        self.acked.clear();
        self.server_addr = None;
        self.id = PeerId(0);
        self.is_server = false;
        self.status = ConnectionStatus::Disconnected;
    }
}

// ---------------------------------------------------------------------------
// Framing
// ---------------------------------------------------------------------------

struct Frame {
    flags: u8,
    channel: u8,
    sender: PeerId,
    seq: u32,
    payload: Vec<u8>,
}

fn encode_control(flags: u8, channel: u8, sender: PeerId, seq: u32, payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(4 + 1 + 1 + 1 + 4 + 4 + 2 + payload.len());
    out.extend_from_slice(MAGIC);
    out.push(1); // version
    out.push(flags);
    out.push(channel);
    out.extend_from_slice(&sender.0.to_be_bytes());
    out.extend_from_slice(&seq.to_be_bytes());
    out.extend_from_slice(&(payload.len() as u16).to_be_bytes());
    out.extend_from_slice(payload);
    out
}

fn decode_frame(buf: &[u8]) -> Option<Frame> {
    if buf.len() < 4 + 1 + 1 + 1 + 4 + 4 + 2 {
        return None;
    }
    if &buf[0..4] != MAGIC {
        return None;
    }
    let _version = buf[4];
    let flags = buf[5];
    let channel = buf[6];
    let sender = PeerId(u32::from_be_bytes(buf[7..11].try_into().ok()?));
    let seq = u32::from_be_bytes(buf[11..15].try_into().ok()?);
    let plen = u16::from_be_bytes(buf[15..17].try_into().ok()?) as usize;
    if buf.len() < 17 + plen {
        return None;
    }
    let payload = buf[17..17 + plen].to_vec();
    Some(Frame {
        flags,
        channel,
        sender,
        seq,
        payload,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::network::TransferMode;

    fn pump_until_connected(server: &mut ENetMultiplayerPeer, client: &mut ENetMultiplayerPeer) {
        for _ in 0..50 {
            let _ = server.poll();
            let _ = client.poll();
            if client.get_connection_status() == ConnectionStatus::Connected {
                return;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        panic!(
            "client failed to connect; status={:?}",
            client.get_connection_status()
        );
    }

    #[test]
    fn udp_server_client_handshake() {
        let mut server = ENetMultiplayerPeer::new();
        let addr = server.create_server("127.0.0.1:0").unwrap();
        assert!(server.is_udp());
        assert!(server.is_server());

        let mut client = ENetMultiplayerPeer::new();
        client.create_client(addr).unwrap();
        pump_until_connected(&mut server, &mut client);

        assert_eq!(client.get_connection_status(), ConnectionStatus::Connected);
        assert_ne!(client.get_unique_id(), PeerId(0));
        assert_ne!(client.get_unique_id(), PeerId::SERVER);
    }

    #[test]
    fn unreliable_packet_roundtrip() {
        let mut server = ENetMultiplayerPeer::new();
        let addr = server.create_server("127.0.0.1:0").unwrap();
        let mut client = ENetMultiplayerPeer::new();
        client.create_client(addr).unwrap();
        pump_until_connected(&mut server, &mut client);

        let payload = b"ping-unreliable".to_vec();
        client
            .send_packet(
                PeerId::SERVER,
                Packet {
                    sender: client.get_unique_id(),
                    channel: 1,
                    data: payload.clone(),
                    transfer_mode: TransferMode::Unreliable,
                },
            )
            .unwrap();

        let mut got = None;
        for _ in 0..40 {
            for p in server.poll() {
                if p.data == payload {
                    got = Some(p);
                    break;
                }
            }
            if got.is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        let p = got.expect("server should receive unreliable packet");
        assert_eq!(p.transfer_mode, TransferMode::Unreliable);
        assert_eq!(p.channel, 1);
        assert_eq!(p.sender, client.get_unique_id());
    }

    #[test]
    fn reliable_packet_roundtrip_and_ack() {
        let mut server = ENetMultiplayerPeer::new();
        let addr = server.create_server("127.0.0.1:0").unwrap();
        let mut client = ENetMultiplayerPeer::new();
        client.create_client(addr).unwrap();
        pump_until_connected(&mut server, &mut client);

        client
            .send_packet(
                PeerId::SERVER,
                Packet {
                    sender: client.get_unique_id(),
                    channel: 0,
                    data: b"reliable-payload".to_vec(),
                    transfer_mode: TransferMode::Reliable,
                },
            )
            .unwrap();
        assert!(
            !client.pending_reliable.is_empty(),
            "reliable send must queue for ACK"
        );

        let mut got = false;
        for _ in 0..80 {
            for p in server.poll() {
                if p.data == b"reliable-payload" {
                    got = true;
                }
            }
            let _ = client.poll(); // receive ACK / retransmit timing
            if got && client.pending_reliable.is_empty() {
                break;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        assert!(got, "server must receive reliable packet");
        assert!(
            client.pending_reliable.is_empty(),
            "ACK must clear pending reliable queue"
        );
    }

    #[test]
    fn ordered_delivery_is_sequential() {
        let mut server = ENetMultiplayerPeer::new();
        let addr = server.create_server("127.0.0.1:0").unwrap();
        let mut client = ENetMultiplayerPeer::new();
        client.create_client(addr).unwrap();
        pump_until_connected(&mut server, &mut client);

        for i in 0u8..5 {
            client
                .send_packet(
                    PeerId::SERVER,
                    Packet {
                        sender: client.get_unique_id(),
                        channel: 2,
                        data: vec![i],
                        transfer_mode: TransferMode::Ordered,
                    },
                )
                .unwrap();
        }

        let mut received = Vec::new();
        for _ in 0..100 {
            for p in server.poll() {
                if p.channel == 2 {
                    received.push(p.data[0]);
                }
            }
            let _ = client.poll();
            if received.len() >= 5 {
                break;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        assert_eq!(received, vec![0, 1, 2, 3, 4], "ordered must preserve order");
    }

    #[test]
    fn transfer_mode_not_metadata_only() {
        // Unreliable packets are not queued in pending_reliable.
        let mut server = ENetMultiplayerPeer::new();
        let addr = server.create_server("127.0.0.1:0").unwrap();
        let mut client = ENetMultiplayerPeer::new();
        client.create_client(addr).unwrap();
        pump_until_connected(&mut server, &mut client);

        client
            .send_packet(
                PeerId::SERVER,
                Packet {
                    sender: client.get_unique_id(),
                    channel: 0,
                    data: b"u".to_vec(),
                    transfer_mode: TransferMode::Unreliable,
                },
            )
            .unwrap();
        assert!(
            client.pending_reliable.is_empty(),
            "unreliable must not enter retransmit queue"
        );

        client
            .send_packet(
                PeerId::SERVER,
                Packet {
                    sender: client.get_unique_id(),
                    channel: 0,
                    data: b"r".to_vec(),
                    transfer_mode: TransferMode::Reliable,
                },
            )
            .unwrap();
        assert_eq!(client.pending_reliable.len(), 1);
    }
}
