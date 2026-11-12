//! `UDPServer` — a bound UDP socket that hands out [`PacketPeerUDP`]
//! connections (pat-s3rrv.178).
//!
//! Mirrors Godot's `UDPServer` (`doc/classes/UDPServer.xml`): `listen`, `poll`,
//! `is_listening`, `is_connection_available`, `take_connection`, `stop`,
//! `get_local_port` and the `max_pending_connections` property.
//!
//! # Divergence from Godot, stated plainly
//!
//! Godot's UDPServer multiplexes a **single** socket: every taken
//! `PacketPeerUDP` reads its own peer's datagrams off the server's socket,
//! demultiplexed by source address. Ours does not demultiplex — a taken
//! connection gets its own ephemeral socket with the destination pre-set, so it
//! can **send** to that peer, while **received** datagrams continue to arrive on
//! the server via [`poll`](UdpServer::poll) and
//! [`take_packet`](UdpServer::take_packet).
//!
//! That is a real behavioural difference, not a stub, and it is called out here
//! rather than buried: a script that takes a connection and then expects
//! `get_packet()` on it to yield that peer's traffic will see nothing. Closing
//! the gap means letting `PacketPeerUDP` be constructed around a shared socket
//! plus an inbox the server feeds, which is a larger change to that type than
//! this bead's acceptance calls for.

use std::collections::{HashMap, VecDeque};
use std::net::{SocketAddr, UdpSocket};

use crate::packet_peer_udp::PacketPeerUDP;

/// Godot's `Error.OK`.
pub const OK: i64 = 0;
/// Godot's `Error.FAILED`.
pub const FAILED: i64 = 1;
/// Godot's `Error.ERR_UNAVAILABLE` — enum position 2 in
/// `core/error/error_list.h`, *not* an errno-style value.
pub const ERR_UNAVAILABLE: i64 = 2;
/// Godot's `Error.ERR_UNCONFIGURED`, returned by `poll` when not listening.
pub const ERR_UNCONFIGURED: i64 = 3;
/// Godot's `Error.ERR_ALREADY_IN_USE`.
pub const ERR_ALREADY_IN_USE: i64 = 22;

/// Godot's default for `max_pending_connections`.
const DEFAULT_MAX_PENDING: usize = 16;

/// The largest datagram read in one `recv_from`.
const RECV_BUFFER: usize = 65_536;

/// Whether this build can open sockets at all. See `tcp_server.rs`.
const SOCKETS_AVAILABLE: bool = !cfg!(target_arch = "wasm32");

/// A bound UDP socket that tracks the peers it has heard from.
///
/// `Default` is written by hand rather than derived: a derived one would leave
/// `max_pending_connections` at zero, and a server that can never accept anyone
/// is a trap that only shows up under load.
#[derive(Debug)]
pub struct UdpServer {
    socket: Option<UdpSocket>,
    /// Source addresses seen by `poll`, oldest first, awaiting `take_connection`.
    pending: VecDeque<SocketAddr>,
    /// Datagrams received per source address, in arrival order.
    received: HashMap<SocketAddr, VecDeque<Vec<u8>>>,
    max_pending_connections: usize,
}

impl UdpServer {
    /// Creates a server that is not yet listening.
    pub fn new() -> Self {
        Self {
            socket: None,
            pending: VecDeque::new(),
            received: HashMap::new(),
            max_pending_connections: DEFAULT_MAX_PENDING,
        }
    }

    /// Binds the socket (`UDPServer.listen`).
    pub fn listen(&mut self, port: u16, bind_address: &str) -> i64 {
        if !SOCKETS_AVAILABLE {
            return ERR_UNAVAILABLE;
        }
        if self.socket.is_some() {
            return ERR_ALREADY_IN_USE;
        }
        let host = match bind_address {
            "*" | "" => "0.0.0.0",
            other => other,
        };
        match UdpSocket::bind((host, port)) {
            Ok(socket) => {
                if socket.set_nonblocking(true).is_err() {
                    return FAILED;
                }
                self.socket = Some(socket);
                OK
            }
            Err(_) => FAILED,
        }
    }

    /// Drains every datagram currently readable (`UDPServer.poll`).
    ///
    /// Returns `ERR_UNCONFIGURED` when not listening, which is how a script
    /// distinguishes "no traffic" from "you never called listen".
    pub fn poll(&mut self) -> i64 {
        let Some(socket) = self.socket.as_ref() else {
            return ERR_UNCONFIGURED;
        };
        let mut buf = vec![0u8; RECV_BUFFER];
        // WouldBlock (or any error) is the normal end of a non-blocking drain.
        while let Ok((len, from)) = socket.recv_from(&mut buf) {
            // A source not seen before becomes a pending connection.
            // The cap is on PENDING addresses, not on packets: dropping
            // traffic from a peer already known would be a silent data
            // loss that no error code reports.
            let known = self.received.contains_key(&from);
            if !known {
                if self.pending.len() >= self.max_pending_connections {
                    continue;
                }
                self.pending.push_back(from);
            }
            self.received
                .entry(from)
                .or_default()
                .push_back(buf[..len].to_vec());
        }
        OK
    }

    /// Whether a peer is waiting to be taken
    /// (`UDPServer.is_connection_available`).
    pub fn is_connection_available(&self) -> bool {
        !self.pending.is_empty()
    }

    /// Whether the socket is bound (`UDPServer.is_listening`).
    pub fn is_listening(&self) -> bool {
        self.socket.is_some()
    }

    /// The bound port, or `0` when not listening (`UDPServer.get_local_port`).
    pub fn get_local_port(&self) -> u16 {
        self.socket
            .as_ref()
            .and_then(|s| s.local_addr().ok())
            .map(|a| a.port())
            .unwrap_or(0)
    }

    /// How many unaccepted peers may queue up
    /// (`UDPServer.max_pending_connections`).
    pub fn max_pending_connections(&self) -> usize {
        self.max_pending_connections
    }

    /// Sets the pending-peer cap. Zero is rejected, because a server that can
    /// never accept anyone is always a mistake rather than a configuration.
    pub fn set_max_pending_connections(&mut self, max: usize) {
        if max > 0 {
            self.max_pending_connections = max;
        }
    }

    /// The address of the next peer awaiting acceptance, without taking it.
    pub fn peek_pending_address(&self) -> Option<SocketAddr> {
        self.pending.front().copied()
    }

    /// Takes the oldest waiting peer (`UDPServer.take_connection`).
    ///
    /// The returned peer can send to that address; see the module-level note on
    /// why it does not receive.
    pub fn take_connection(&mut self) -> Option<PacketPeerUDP> {
        let addr = self.pending.pop_front()?;
        let mut peer = PacketPeerUDP::new();
        // Bind on the same family as the peer, or a v6 client cannot be
        // answered from a v4 socket.
        let local = if addr.is_ipv6() {
            "[::]:0"
        } else {
            "0.0.0.0:0"
        };
        if peer.bind(local).is_err() {
            return None;
        }
        if peer.set_dest_address(addr).is_err() {
            return None;
        }
        Some(peer)
    }

    /// Pops the oldest datagram received from `addr`, if any.
    ///
    /// This is the receive half that [`take_connection`](Self::take_connection)
    /// cannot provide; see the module-level note.
    pub fn take_packet(&mut self, addr: SocketAddr) -> Option<Vec<u8>> {
        self.received.get_mut(&addr)?.pop_front()
    }

    /// Stops listening and forgets every peer (`UDPServer.stop`).
    pub fn stop(&mut self) {
        self.socket = None;
        self.pending.clear();
        self.received.clear();
    }
}

impl Default for UdpServer {
    fn default() -> Self {
        Self::new()
    }
}

/// Script-visible handle table for `UDPServer` (pat-s3rrv.178).
///
/// Mirrors `packet_peer_stream.rs` — same poison-tolerant table, same tag mask.
///
/// ⚠️ THE TAG BYTE IS A WORKSPACE-WIDE NAMESPACE. Two modules issuing the same
/// top byte makes `is_*_handle` answer true for the other module's ids, and the
/// symptom is a `MethodNotFound` naming the WRONG class. `0x62` was chosen after
/// sweeping every tag byte in the workspace — note `0x5D` looked free in a
/// truncated listing and is in fact WebRTC's.
const UDP_SERVER_HANDLE_TAG: u64 = 0x6200_0000_0000_0000;
static NEXT_UDP_SERVER_HANDLE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
static UDP_SERVER_HANDLES: std::sync::Mutex<Option<std::collections::HashMap<u64, UdpServer>>> =
    std::sync::Mutex::new(None);

fn udp_server_handles(
) -> std::sync::MutexGuard<'static, Option<std::collections::HashMap<u64, UdpServer>>> {
    // A poisoned lock still holds a usable table: a panic in one script's
    // handler must not make every later call fail.
    UDP_SERVER_HANDLES
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Returns `true` when `id` was issued by [`new_udp_server_handle`].
pub fn is_udp_server_handle(id: gdcore::id::ObjectId) -> bool {
    id.raw() & 0xFF00_0000_0000_0000 == UDP_SERVER_HANDLE_TAG
}

/// Allocates a fresh [`UdpServer`] and returns its script-visible id
/// (Godot `UDPServer.new()`).
pub fn new_udp_server_handle() -> gdcore::id::ObjectId {
    let seq = NEXT_UDP_SERVER_HANDLE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let raw = UDP_SERVER_HANDLE_TAG | seq;
    udp_server_handles()
        .get_or_insert_with(std::collections::HashMap::new)
        .insert(raw, UdpServer::new());
    gdcore::id::ObjectId::from_raw(raw)
}

/// Runs `f` against the live server behind `id`, or returns `None` when the
/// handle was never issued or has already been released.
pub fn with_udp_server<R>(
    id: gdcore::id::ObjectId,
    f: impl FnOnce(&mut UdpServer) -> R,
) -> Option<R> {
    let mut guard = udp_server_handles();
    let map = guard.get_or_insert_with(std::collections::HashMap::new);
    map.get_mut(&id.raw()).map(f)
}

/// Drops the server behind `id`, closing its socket.
pub fn release_udp_server_handle(id: gdcore::id::ObjectId) -> bool {
    let mut guard = udp_server_handles();
    let map = guard.get_or_insert_with(std::collections::HashMap::new);
    map.remove(&id.raw()).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Binds to an OS-chosen loopback port.
    fn ephemeral() -> UdpServer {
        let mut s = UdpServer::new();
        assert_eq!(s.listen(0, "127.0.0.1"), OK, "loopback bind should succeed");
        s
    }

    /// Sends one datagram to `port` from a throwaway socket, returning its addr.
    fn send_to(port: u16, payload: &[u8]) -> SocketAddr {
        let client = UdpSocket::bind("127.0.0.1:0").expect("client binds");
        let local = client.local_addr().expect("client has an address");
        client
            .send_to(payload, ("127.0.0.1", port))
            .expect("client sends");
        local
    }

    /// Polls until `f` holds, so the tests do not depend on delivery timing.
    fn poll_until(s: &mut UdpServer, f: impl Fn(&UdpServer) -> bool) -> bool {
        for _ in 0..200 {
            s.poll();
            if f(s) {
                return true;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        false
    }

    #[test]
    fn a_fresh_server_is_not_listening() {
        let s = UdpServer::new();
        assert!(!s.is_listening());
        assert_eq!(s.get_local_port(), 0);
        assert_eq!(
            s.max_pending_connections(),
            DEFAULT_MAX_PENDING,
            "Godot's default carries over"
        );
    }

    #[test]
    fn poll_without_listening_is_unconfigured_not_ok() {
        let mut s = UdpServer::new();
        assert_eq!(
            s.poll(),
            ERR_UNCONFIGURED,
            "a script must be able to tell 'no traffic' from 'never listened'"
        );
    }

    #[test]
    fn listen_binds_an_ephemeral_port() {
        let s = ephemeral();
        assert!(s.is_listening());
        assert_ne!(s.get_local_port(), 0);
    }

    #[test]
    fn listening_twice_reports_already_in_use() {
        let mut s = ephemeral();
        assert_eq!(s.listen(0, "127.0.0.1"), ERR_ALREADY_IN_USE);
    }

    #[test]
    fn no_connection_is_available_before_any_traffic() {
        let mut s = ephemeral();
        assert_eq!(s.poll(), OK, "polling an idle socket is not an error");
        assert!(!s.is_connection_available());
        assert!(s.take_connection().is_none());
    }

    #[test]
    fn a_datagram_creates_exactly_one_pending_peer() {
        let mut s = ephemeral();
        let port = s.get_local_port();
        let client = send_to(port, b"hello");

        assert!(
            poll_until(&mut s, |s| s.is_connection_available()),
            "the datagram should register its sender"
        );
        assert_eq!(s.peek_pending_address(), Some(client));
        assert_eq!(
            s.take_packet(client).as_deref(),
            Some(&b"hello"[..]),
            "the payload is kept, not just the address"
        );

        let peer = s.take_connection();
        assert!(peer.is_some(), "the pending peer can be taken");
        assert!(
            !s.is_connection_available(),
            "one datagram must not yield two connections"
        );
    }

    #[test]
    fn a_second_datagram_from_a_known_peer_does_not_queue_again() {
        let mut s = ephemeral();
        let port = s.get_local_port();
        let client = UdpSocket::bind("127.0.0.1:0").expect("client binds");
        let addr = client.local_addr().unwrap();
        client.send_to(b"one", ("127.0.0.1", port)).unwrap();
        assert!(poll_until(&mut s, |s| s.is_connection_available()));

        client.send_to(b"two", ("127.0.0.1", port)).unwrap();
        assert!(poll_until(&mut s, |s| s
            .received
            .get(&addr)
            .is_some_and(|q| q.len() == 2)));

        assert_eq!(
            s.pending.len(),
            1,
            "the same peer must not be offered as a second connection"
        );
        assert_eq!(s.take_packet(addr).as_deref(), Some(&b"one"[..]));
        assert_eq!(
            s.take_packet(addr).as_deref(),
            Some(&b"two"[..]),
            "both datagrams are kept, in arrival order"
        );
    }

    #[test]
    fn stop_releases_everything() {
        let mut s = ephemeral();
        let port = s.get_local_port();
        send_to(port, b"x");
        poll_until(&mut s, |s| s.is_connection_available());

        s.stop();
        assert!(!s.is_listening());
        assert!(!s.is_connection_available());
        assert_eq!(s.get_local_port(), 0);
    }

    #[test]
    fn zero_is_rejected_as_a_pending_cap() {
        let mut s = UdpServer::new();
        s.set_max_pending_connections(4);
        assert_eq!(s.max_pending_connections(), 4);
        s.set_max_pending_connections(0);
        assert_eq!(
            s.max_pending_connections(),
            4,
            "a cap of zero would make the server permanently unable to accept"
        );
    }
}
