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
