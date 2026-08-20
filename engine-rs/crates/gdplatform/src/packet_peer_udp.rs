//! PacketPeerUDP — datagram peer matching Godot's `PacketPeerUDP` slice.
//!
//! Supports bind, destination address, put/get packet, and available count
//! over non-blocking `std::net::UdpSocket` (loopback-capable).

use std::collections::VecDeque;
use std::io;
use std::net::{SocketAddr, ToSocketAddrs, UdpSocket};

/// Errors from UDP peer operations.
#[derive(Debug, thiserror::Error)]
pub enum PacketPeerUdpError {
    /// I/O failure.
    #[error("{0}")]
    Io(#[from] io::Error),
    /// Peer is not bound.
    #[error("UDP peer is not bound")]
    NotBound,
    /// Destination address not set.
    #[error("destination address not set")]
    NoDestination,
    /// Address resolution failed.
    #[error("could not resolve address: {0}")]
    Resolve(String),
}

/// Result alias for UDP peer ops.
pub type Result<T> = std::result::Result<T, PacketPeerUdpError>;

/// Godot-compatible UDP packet peer.
pub struct PacketPeerUDP {
    socket: Option<UdpSocket>,
    dest: Option<SocketAddr>,
    inbox: VecDeque<(SocketAddr, Vec<u8>)>,
    last_packet_from: Option<SocketAddr>,
}

impl PacketPeerUDP {
    /// Creates an unbound UDP peer.
    pub fn new() -> Self {
        Self {
            socket: None,
            dest: None,
            inbox: VecDeque::new(),
            last_packet_from: None,
        }
    }

    /// Binds to `addr` (e.g. `"127.0.0.1:0"` for an ephemeral loopback port).
    pub fn bind<A: ToSocketAddrs>(&mut self, addr: A) -> Result<SocketAddr> {
        let sock = UdpSocket::bind(addr)?;
        sock.set_nonblocking(true)?;
        let local = sock.local_addr()?;
        self.socket = Some(sock);
        Ok(local)
    }

    /// Local bound address, if bound.
    pub fn local_addr(&self) -> Option<SocketAddr> {
        self.socket.as_ref().and_then(|s| s.local_addr().ok())
    }

    /// Sets the default destination for [`put_packet`](Self::put_packet).
    pub fn set_dest_address<A: ToSocketAddrs>(&mut self, addr: A) -> Result<()> {
        let resolved = addr
            .to_socket_addrs()
            .map_err(|e| PacketPeerUdpError::Resolve(e.to_string()))?
            .next()
            .ok_or_else(|| PacketPeerUdpError::Resolve("empty resolution".into()))?;
        self.dest = Some(resolved);
        Ok(())
    }

    /// Destination set by [`set_dest_address`](Self::set_dest_address).
    pub fn dest_address(&self) -> Option<SocketAddr> {
        self.dest
    }

    /// Sends a datagram to the current destination.
    pub fn put_packet(&self, data: &[u8]) -> Result<()> {
        let sock = self.socket.as_ref().ok_or(PacketPeerUdpError::NotBound)?;
        let dest = self.dest.ok_or(PacketPeerUdpError::NoDestination)?;
        sock.send_to(data, dest)?;
        Ok(())
    }

    /// Sends a datagram to an explicit address (does not change the default dest).
    pub fn put_packet_to(&self, data: &[u8], addr: SocketAddr) -> Result<()> {
        let sock = self.socket.as_ref().ok_or(PacketPeerUdpError::NotBound)?;
        sock.send_to(data, addr)?;
        Ok(())
    }

    /// Polls the socket into the inbox (non-blocking).
    pub fn poll(&mut self) {
        let Some(sock) = self.socket.as_ref() else {
            return;
        };
        let mut buf = [0u8; 65536];
        loop {
            match sock.recv_from(&mut buf) {
                Ok((n, from)) => {
                    self.inbox.push_back((from, buf[..n].to_vec()));
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                Err(_) => break,
            }
        }
    }

    /// Number of complete packets waiting.
    pub fn get_available_packet_count(&self) -> usize {
        self.inbox.len()
    }

    /// Pops the next packet (empty if none). Call [`poll`](Self::poll) first.
    pub fn get_packet(&mut self) -> Vec<u8> {
        match self.inbox.pop_front() {
            Some((from, data)) => {
                self.last_packet_from = Some(from);
                data
            }
            None => Vec::new(),
        }
    }

    /// Source address of the last packet returned by [`get_packet`](Self::get_packet).
    pub fn get_packet_address(&self) -> Option<SocketAddr> {
        self.last_packet_from
    }

    /// Closes the socket.
    pub fn close(&mut self) {
        self.socket = None;
        self.inbox.clear();
        self.last_packet_from = None;
    }

    /// Whether the peer currently holds a bound socket.
    pub fn is_bound(&self) -> bool {
        self.socket.is_some()
    }
}

impl Default for PacketPeerUDP {
    fn default() -> Self {
        Self::new()
    }
}

/// Script-visible handle table for `PacketPeerUDP` (pat-s3rrv.178).
///
/// The class was registered in ClassDB and fully implemented here, but had no
/// interpreter dispatch, so `PacketPeerUDP.new()` from GDScript reached nothing.
/// Mirrors `packet_peer_stream.rs` exactly — same poison-tolerant table, same
/// tag mask.
///
/// ⚠️ THE TAG BYTE IS A WORKSPACE-WIDE NAMESPACE. Two modules issuing the same
/// top byte makes `is_*_handle` answer true for the other module's ids, and the
/// symptom is a `MethodNotFound` naming the WRONG class. `0x60` was chosen after
/// sweeping every tag constant in the workspace; it is unused, and follows the socket family (`0x5B`-`0x5F`); `0x5D` is WebRTC's connection tag.
const PACKET_PEER_UDP_HANDLE_TAG: u64 = 0x6000_0000_0000_0000;
static NEXT_PACKET_PEER_UDP_HANDLE: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(1);
static PACKET_PEER_UDP_HANDLES: std::sync::Mutex<
    Option<std::collections::HashMap<u64, PacketPeerUDP>>,
> = std::sync::Mutex::new(None);

fn packet_peer_udp_handles(
) -> std::sync::MutexGuard<'static, Option<std::collections::HashMap<u64, PacketPeerUDP>>> {
    // A poisoned lock still holds a usable table: a panic in one script's
    // handler must not make every later call fail.
    PACKET_PEER_UDP_HANDLES
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Returns `true` when `id` was issued by [`new_packet_peer_udp_handle`].
pub fn is_packet_peer_udp_handle(id: gdcore::id::ObjectId) -> bool {
    id.raw() & 0xFF00_0000_0000_0000 == PACKET_PEER_UDP_HANDLE_TAG
}

/// Allocates a fresh [`PacketPeerUDP`] and returns its script-visible id
/// (Godot `PacketPeerUDP.new()`).
pub fn new_packet_peer_udp_handle() -> gdcore::id::ObjectId {
    let seq = NEXT_PACKET_PEER_UDP_HANDLE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let raw = PACKET_PEER_UDP_HANDLE_TAG | seq;
    packet_peer_udp_handles()
        .get_or_insert_with(std::collections::HashMap::new)
        .insert(raw, PacketPeerUDP::new());
    gdcore::id::ObjectId::from_raw(raw)
}

/// Runs `f` against the live peer behind `id`, or returns `None` when the
/// handle was never issued or has already been released.
pub fn with_packet_peer_udp<R>(
    id: gdcore::id::ObjectId,
    f: impl FnOnce(&mut PacketPeerUDP) -> R,
) -> Option<R> {
    let mut guard = packet_peer_udp_handles();
    let map = guard.get_or_insert_with(std::collections::HashMap::new);
    map.get_mut(&id.raw()).map(f)
}

/// Drops the peer behind `id`, closing its socket.
pub fn release_packet_peer_udp_handle(id: gdcore::id::ObjectId) -> bool {
    let mut guard = packet_peer_udp_handles();
    let map = guard.get_or_insert_with(std::collections::HashMap::new);
    map.remove(&id.raw()).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn pump_until(a: &mut PacketPeerUDP, b: &mut PacketPeerUDP, timeout: Duration) -> bool {
        let start = Instant::now();
        while start.elapsed() < timeout {
            a.poll();
            b.poll();
            if a.get_available_packet_count() > 0 || b.get_available_packet_count() > 0 {
                return true;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        false
    }

    #[test]
    fn udp_loopback_echo() {
        let mut server = PacketPeerUDP::new();
        let local = server.bind("127.0.0.1:0").unwrap();

        let mut client = PacketPeerUDP::new();
        client.bind("127.0.0.1:0").unwrap();
        client.set_dest_address(local).unwrap();
        client.put_packet(b"ping").unwrap();

        assert!(
            pump_until(&mut server, &mut client, Duration::from_secs(2)),
            "server should receive packet"
        );
        server.poll();
        assert_eq!(server.get_available_packet_count(), 1);
        let pkt = server.get_packet();
        assert_eq!(pkt, b"ping");
        let from = server.get_packet_address().unwrap();

        server.put_packet_to(b"pong", from).unwrap();
        assert!(
            pump_until(&mut client, &mut server, Duration::from_secs(2)),
            "client should receive reply"
        );
        client.poll();
        assert_eq!(client.get_packet(), b"pong");
    }

    #[test]
    fn put_packet_without_dest_errors() {
        let mut p = PacketPeerUDP::new();
        p.bind("127.0.0.1:0").unwrap();
        assert!(matches!(
            p.put_packet(b"x"),
            Err(PacketPeerUdpError::NoDestination)
        ));
    }
}
