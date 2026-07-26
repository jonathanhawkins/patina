//! TCP multiplayer peer — a real loopback-capable transport beyond MockNetwork.
//!
//! Implements a minimal length-prefixed framing protocol over `std::net` so
//! two peers can exchange multiplayer packets. Handshake and I/O are
//! poll-driven (non-blocking) so a single-threaded test can pump both sides.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream, ToSocketAddrs};
use std::time::Duration;

use crate::network::{
    ConnectionStatus, NetworkError, NetworkPeer, Packet, PeerId, Result, TransferMode,
};

const HANDSHAKE_MAGIC: &[u8; 8] = b"PATMP001";

/// TCP multiplayer peer (server or client).
pub struct TcpMultiplayerPeer {
    id: PeerId,
    status: ConnectionStatus,
    listener: Option<TcpListener>,
    peers: HashMap<PeerId, TcpStream>,
    next_peer_id: u32,
    inbox: Vec<Packet>,
    read_bufs: HashMap<PeerId, Vec<u8>>,
    /// Client-only: stream to server before peer id is assigned.
    pending_server: Option<TcpStream>,
    pending_server_buf: Vec<u8>,
    is_server_role: bool,
}

impl TcpMultiplayerPeer {
    /// Creates an unbound peer (disconnected).
    pub fn new() -> Self {
        Self {
            id: PeerId(0),
            status: ConnectionStatus::Disconnected,
            listener: None,
            peers: HashMap::new(),
            next_peer_id: 2,
            inbox: Vec::new(),
            read_bufs: HashMap::new(),
            pending_server: None,
            pending_server_buf: Vec::new(),
            is_server_role: false,
        }
    }

    /// Starts a server listening on `addr` (e.g. `"127.0.0.1:0"`).
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
        self.next_peer_id = 2;
        Ok(local)
    }

    /// Connects to a remote server as a client (handshake completes via [`poll`]).
    pub fn create_client<A: ToSocketAddrs>(&mut self, addr: A) -> Result<()> {
        self.close();
        let stream =
            TcpStream::connect(addr).map_err(|e| NetworkError::Transport(e.to_string()))?;
        stream
            .set_nonblocking(true)
            .map_err(|e| NetworkError::Transport(e.to_string()))?;
        write_frame(&stream, HANDSHAKE_MAGIC)
            .map_err(|e| NetworkError::Transport(e.to_string()))?;
        self.pending_server = Some(stream);
        self.pending_server_buf.clear();
        self.is_server_role = false;
        self.status = ConnectionStatus::Connecting;
        self.id = PeerId(0);
        Ok(())
    }

    /// Local address when this peer is a server.
    pub fn local_addr(&self) -> Option<SocketAddr> {
        self.listener.as_ref().and_then(|l| l.local_addr().ok())
    }

    fn accept_new_clients(&mut self) {
        let Some(listener) = self.listener.as_ref() else {
            return;
        };
        loop {
            match listener.accept() {
                Ok((stream, _)) => {
                    let _ = stream.set_nonblocking(true);
                    // Stash as pending until we read MAGIC, using peer id 0 slot temporarily.
                    // Read magic now if available.
                    let mut buf = Vec::new();
                    match try_read_frame(&stream, &mut buf) {
                        Ok(Some(frame)) if frame.as_slice() == HANDSHAKE_MAGIC => {
                            let id = PeerId(self.next_peer_id);
                            self.next_peer_id = self.next_peer_id.saturating_add(1);
                            let payload = id.0.to_be_bytes();
                            if write_frame(&stream, &payload).is_err() {
                                continue;
                            }
                            self.peers.insert(id, stream);
                            self.read_bufs.insert(id, Vec::new());
                        }
                        Ok(None) | Ok(Some(_)) => {
                            // Incomplete or wrong magic — keep stream with temp key using next id
                            // and complete on next poll via a pending list.
                            // For simplicity: spin briefly for magic (same thread as test will re-poll).
                            let mut got = false;
                            let mut local_buf = buf;
                            for _ in 0..20 {
                                match try_read_frame(&stream, &mut local_buf) {
                                    Ok(Some(frame)) if frame.as_slice() == HANDSHAKE_MAGIC => {
                                        got = true;
                                        break;
                                    }
                                    Ok(None) => std::thread::sleep(Duration::from_millis(2)),
                                    _ => break,
                                }
                            }
                            if got {
                                let id = PeerId(self.next_peer_id);
                                self.next_peer_id = self.next_peer_id.saturating_add(1);
                                let payload = id.0.to_be_bytes();
                                if write_frame(&stream, &payload).is_ok() {
                                    self.peers.insert(id, stream);
                                    self.read_bufs.insert(id, Vec::new());
                                }
                            }
                        }
                        Err(_) => {}
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(_) => break,
            }
        }
    }

    fn finish_client_handshake(&mut self) {
        let Some(stream) = self.pending_server.as_ref() else {
            return;
        };
        match try_read_frame(stream, &mut self.pending_server_buf) {
            Ok(Some(frame)) if frame.len() >= 4 => {
                let id = u32::from_be_bytes([frame[0], frame[1], frame[2], frame[3]]);
                self.id = PeerId(id);
                self.status = ConnectionStatus::Connected;
                if let Some(stream) = self.pending_server.take() {
                    self.peers.insert(PeerId::SERVER, stream);
                    self.read_bufs
                        .insert(PeerId::SERVER, std::mem::take(&mut self.pending_server_buf));
                }
            }
            Ok(None) | Ok(Some(_)) => {}
            Err(_) => {
                self.pending_server = None;
                self.status = ConnectionStatus::Disconnected;
            }
        }
    }

    fn pump_reads(&mut self) {
        let peer_ids: Vec<PeerId> = self.peers.keys().copied().collect();
        for pid in peer_ids {
            let Some(stream) = self.peers.get(&pid) else {
                continue;
            };
            let buf = self.read_bufs.entry(pid).or_default();
            loop {
                match try_read_frame(stream, buf) {
                    Ok(Some(data)) => {
                        if data.as_slice() == HANDSHAKE_MAGIC {
                            continue;
                        }
                        self.inbox.push(Packet {
                            sender: pid,
                            channel: 0,
                            data,
                            transfer_mode: TransferMode::Reliable,
                        });
                    }
                    Ok(None) => break,
                    Err(_) => {
                        self.peers.remove(&pid);
                        self.read_bufs.remove(&pid);
                        break;
                    }
                }
            }
        }
    }
}

impl Default for TcpMultiplayerPeer {
    fn default() -> Self {
        Self::new()
    }
}

impl NetworkPeer for TcpMultiplayerPeer {
    fn send_packet(&mut self, to: PeerId, packet: Packet) -> Result<()> {
        if self.status != ConnectionStatus::Connected {
            return Err(NetworkError::Transport("not connected".into()));
        }
        let target = if self.is_server_role {
            to
        } else {
            PeerId::SERVER
        };
        let stream = self
            .peers
            .get(&target)
            .ok_or(NetworkError::PeerUnreachable(target))?;
        let mut body = Vec::with_capacity(5 + packet.data.len());
        body.extend_from_slice(&packet.sender.0.to_be_bytes());
        body.push(packet.channel);
        body.extend_from_slice(&packet.data);
        write_frame(stream, &body).map_err(|e| NetworkError::Transport(e.to_string()))
    }

    fn poll(&mut self) -> Vec<Packet> {
        if self.is_server_role {
            self.accept_new_clients();
        } else if self.status == ConnectionStatus::Connecting {
            self.finish_client_handshake();
        }
        self.pump_reads();
        let raw = std::mem::take(&mut self.inbox);
        raw.into_iter()
            .map(|mut p| {
                if p.data.len() >= 5 {
                    let sid = u32::from_be_bytes([p.data[0], p.data[1], p.data[2], p.data[3]]);
                    let ch = p.data[4];
                    p.sender = PeerId(sid);
                    p.channel = ch;
                    p.data = p.data[5..].to_vec();
                }
                p
            })
            .collect()
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
        self.listener = None;
        self.peers.clear();
        self.read_bufs.clear();
        self.inbox.clear();
        self.pending_server = None;
        self.pending_server_buf.clear();
        self.status = ConnectionStatus::Disconnected;
        self.id = PeerId(0);
        self.is_server_role = false;
    }
}

fn write_frame(mut stream: &TcpStream, payload: &[u8]) -> std::io::Result<()> {
    let len = (payload.len() as u32).to_be_bytes();
    stream.write_all(&len)?;
    stream.write_all(payload)?;
    Ok(())
}

fn try_read_frame(mut stream: &TcpStream, buf: &mut Vec<u8>) -> std::io::Result<Option<Vec<u8>>> {
    let mut tmp = [0u8; 4096];
    match stream.read(&mut tmp) {
        Ok(0) => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "eof",
            ))
        }
        Ok(n) => buf.extend_from_slice(&tmp[..n]),
        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
        Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
        Err(e) => return Err(e),
    }
    if buf.len() < 4 {
        return Ok(None);
    }
    let len = u32::from_be_bytes([buf[0], buf[1], buf[2], buf[3]]) as usize;
    if buf.len() < 4 + len {
        return Ok(None);
    }
    let frame = buf[4..4 + len].to_vec();
    buf.drain(..4 + len);
    Ok(Some(frame))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pump_until_connected(server: &mut TcpMultiplayerPeer, client: &mut TcpMultiplayerPeer) {
        for _ in 0..100 {
            let _ = server.poll();
            let _ = client.poll();
            if client.get_connection_status() == ConnectionStatus::Connected
                && !server.peers.is_empty()
            {
                return;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!(
            "handshake failed: client={:?} server_peers={}",
            client.get_connection_status(),
            server.peers.len()
        );
    }

    #[test]
    fn server_client_exchange_packets() {
        let mut server = TcpMultiplayerPeer::new();
        let addr = server.create_server("127.0.0.1:0").unwrap();
        assert!(server.is_server());

        let mut client = TcpMultiplayerPeer::new();
        client.create_client(addr).unwrap();
        assert_eq!(client.get_connection_status(), ConnectionStatus::Connecting);

        pump_until_connected(&mut server, &mut client);
        assert!(!client.is_server());
        assert_ne!(client.get_unique_id(), PeerId::SERVER);

        let payload = b"hello-server".to_vec();
        client
            .send_packet(
                PeerId::SERVER,
                Packet {
                    sender: client.get_unique_id(),
                    channel: 1,
                    data: payload.clone(),
                    transfer_mode: TransferMode::Reliable,
                },
            )
            .unwrap();

        let mut got = None;
        for _ in 0..50 {
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
        let pkt = got.expect("server receives client packet");
        assert_eq!(pkt.sender, client.get_unique_id());
        assert_eq!(pkt.channel, 1);

        let reply = b"hello-client".to_vec();
        server
            .send_packet(
                client.get_unique_id(),
                Packet {
                    sender: PeerId::SERVER,
                    channel: 2,
                    data: reply.clone(),
                    transfer_mode: TransferMode::Reliable,
                },
            )
            .unwrap();

        let mut got_c = None;
        for _ in 0..50 {
            let _ = server.poll();
            for p in client.poll() {
                if p.data == reply {
                    got_c = Some(p);
                    break;
                }
            }
            if got_c.is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        let pkt = got_c.expect("client receives server packet");
        assert_eq!(pkt.sender, PeerId::SERVER);
        assert_eq!(pkt.channel, 2);

        client.close();
        server.close();
    }

    #[test]
    fn multiplayer_api_with_tcp_peers() {
        use crate::network::MultiplayerAPI;

        let mut server_peer = TcpMultiplayerPeer::new();
        let addr = server_peer.create_server("127.0.0.1:0").unwrap();
        let mut client_peer = TcpMultiplayerPeer::new();
        client_peer.create_client(addr).unwrap();
        pump_until_connected(&mut server_peer, &mut client_peer);

        let mut api_server = MultiplayerAPI::new();
        api_server.set_multiplayer_peer(Box::new(server_peer));
        assert!(api_server.is_server());

        let mut api_client = MultiplayerAPI::new();
        api_client.set_multiplayer_peer(Box::new(client_peer));
        assert!(!api_client.is_server());
    }
}
