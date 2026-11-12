//! StreamPeerTCP — byte stream peer matching Godot's `StreamPeerTCP` slice.
//!
//! Poll-driven non-blocking TCP connect and put/get data for loopback tests.

use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::time::Duration;

/// Connection status values aligned with Godot `StreamPeerTCP.Status`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum StreamPeerTcpStatus {
    /// Not connected.
    None = 0,
    /// Connection in progress.
    Connecting = 1,
    /// Connected and ready for I/O.
    Connected = 2,
    /// Error state.
    Error = 3,
}

/// Errors from TCP stream peer operations.
#[derive(Debug, thiserror::Error)]
pub enum StreamPeerTcpError {
    /// I/O failure.
    #[error("{0}")]
    Io(#[from] io::Error),
    /// Not connected.
    #[error("stream peer is not connected")]
    NotConnected,
    /// Address resolution failed.
    #[error("could not resolve address: {0}")]
    Resolve(String),
}

/// Result alias.
pub type Result<T> = std::result::Result<T, StreamPeerTcpError>;

/// Godot-compatible TCP stream peer.
pub struct StreamPeerTCP {
    status: StreamPeerTcpStatus,
    stream: Option<TcpStream>,
    remote: Option<SocketAddr>,
    /// Incoming bytes buffered after poll/read.
    rx: Vec<u8>,
}

impl StreamPeerTCP {
    /// Creates a disconnected peer.
    pub fn new() -> Self {
        Self {
            status: StreamPeerTcpStatus::None,
            stream: None,
            remote: None,
            rx: Vec::new(),
        }
    }

    /// Current connection status.
    pub fn get_status(&self) -> StreamPeerTcpStatus {
        self.status
    }

    /// Connects to `host:port` (blocking connect, then non-blocking I/O).
    ///
    /// Godot uses a non-blocking connect + poll; for the loopback slice we
    /// complete the OS connect immediately when the peer is reachable.
    pub fn connect_to_host(&mut self, host: &str, port: u16) -> Result<()> {
        self.close();
        self.status = StreamPeerTcpStatus::Connecting;
        let addr = format!("{host}:{port}");
        match TcpStream::connect(&addr) {
            Ok(stream) => {
                stream.set_nonblocking(true)?;
                self.remote = stream.peer_addr().ok();
                self.stream = Some(stream);
                self.status = StreamPeerTcpStatus::Connected;
                Ok(())
            }
            Err(e) => {
                self.status = StreamPeerTcpStatus::Error;
                Err(StreamPeerTcpError::Io(e))
            }
        }
    }

    /// Connects using a resolved socket address.
    pub fn connect_to_addr(&mut self, addr: SocketAddr) -> Result<()> {
        self.connect_to_host(&addr.ip().to_string(), addr.port())
    }

    /// Accepts an incoming connection from a listener (test helper / server side).
    pub fn accept_from_listener(listener: &TcpListener) -> Result<Self> {
        listener.set_nonblocking(true)?;
        match listener.accept() {
            Ok((stream, addr)) => {
                stream.set_nonblocking(true)?;
                Ok(Self {
                    status: StreamPeerTcpStatus::Connected,
                    stream: Some(stream),
                    remote: Some(addr),
                    rx: Vec::new(),
                })
            }
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => Err(StreamPeerTcpError::Io(
                io::Error::new(io::ErrorKind::WouldBlock, "no pending connection"),
            )),
            Err(e) => Err(StreamPeerTcpError::Io(e)),
        }
    }

    /// Remote peer address when connected.
    pub fn get_connected_host(&self) -> Option<SocketAddr> {
        self.remote
    }

    /// Polls the socket for readable data into the internal buffer.
    pub fn poll(&mut self) {
        let Some(stream) = self.stream.as_mut() else {
            return;
        };
        if self.status != StreamPeerTcpStatus::Connected
            && self.status != StreamPeerTcpStatus::Connecting
        {
            return;
        }
        let mut buf = [0u8; 8192];
        loop {
            match stream.read(&mut buf) {
                Ok(0) => {
                    // Peer closed.
                    self.status = StreamPeerTcpStatus::None;
                    break;
                }
                Ok(n) => self.rx.extend_from_slice(&buf[..n]),
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                Err(_) => {
                    self.status = StreamPeerTcpStatus::Error;
                    break;
                }
            }
        }
        if self.status == StreamPeerTcpStatus::Connecting {
            // Non-blocking connect completed if we can write/read without error path.
            self.status = StreamPeerTcpStatus::Connected;
        }
    }

    /// Writes all of `data` to the stream (loops until sent or error).
    pub fn put_data(&mut self, data: &[u8]) -> Result<()> {
        let stream = self
            .stream
            .as_mut()
            .ok_or(StreamPeerTcpError::NotConnected)?;
        if self.status != StreamPeerTcpStatus::Connected {
            return Err(StreamPeerTcpError::NotConnected);
        }
        let mut offset = 0;
        while offset < data.len() {
            match stream.write(&data[offset..]) {
                Ok(0) => {
                    return Err(StreamPeerTcpError::Io(io::Error::new(
                        io::ErrorKind::WriteZero,
                        "connection closed",
                    )));
                }
                Ok(n) => offset += n,
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                    // Brief spin for loopback tests.
                    std::thread::sleep(Duration::from_millis(1));
                }
                Err(e) => {
                    self.status = StreamPeerTcpStatus::Error;
                    return Err(StreamPeerTcpError::Io(e));
                }
            }
        }
        Ok(())
    }

    /// Number of bytes buffered and available via [`get_data`](Self::get_data).
    pub fn get_available_bytes(&self) -> usize {
        self.rx.len()
    }

    /// Reads up to `max` bytes from the receive buffer (after poll).
    pub fn get_data(&mut self, max: usize) -> Vec<u8> {
        let n = max.min(self.rx.len());
        self.rx.drain(..n).collect()
    }

    /// Closes the connection.
    pub fn close(&mut self) {
        self.stream = None;
        self.remote = None;
        self.rx.clear();
        self.status = StreamPeerTcpStatus::None;
    }
}

impl Default for StreamPeerTCP {
    fn default() -> Self {
        Self::new()
    }
}

/// Script-visible handle table for `StreamPeerTCP` (pat-s3rrv.178).
///
/// The class was registered in ClassDB and fully implemented here, but had no
/// interpreter dispatch, so `StreamPeerTCP.new()` from GDScript reached nothing.
/// Mirrors `packet_peer_stream.rs` exactly — same poison-tolerant table, same
/// tag mask.
///
/// ⚠️ THE TAG BYTE IS A WORKSPACE-WIDE NAMESPACE. Two modules issuing the same
/// top byte makes `is_*_handle` answer true for the other module's ids, and the
/// symptom is a `MethodNotFound` naming the WRONG class. `0x5F` was chosen after
/// sweeping every tag constant in the workspace; it is unused, and sits beside `StreamPeerBuffer` (`0x5B`) and `PacketPeerStream` (`0x5C`).
const STREAM_PEER_TCP_HANDLE_TAG: u64 = 0x5F00_0000_0000_0000;
static NEXT_STREAM_PEER_TCP_HANDLE: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(1);
static STREAM_PEER_TCP_HANDLES: std::sync::Mutex<
    Option<std::collections::HashMap<u64, StreamPeerTCP>>,
> = std::sync::Mutex::new(None);

fn stream_peer_tcp_handles(
) -> std::sync::MutexGuard<'static, Option<std::collections::HashMap<u64, StreamPeerTCP>>> {
    // A poisoned lock still holds a usable table: a panic in one script's
    // handler must not make every later call fail.
    STREAM_PEER_TCP_HANDLES
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Returns `true` when `id` was issued by [`new_stream_peer_tcp_handle`].
pub fn is_stream_peer_tcp_handle(id: gdcore::id::ObjectId) -> bool {
    id.raw() & 0xFF00_0000_0000_0000 == STREAM_PEER_TCP_HANDLE_TAG
}

/// Allocates a fresh [`StreamPeerTCP`] and returns its script-visible id
/// (Godot `StreamPeerTCP.new()`).
pub fn new_stream_peer_tcp_handle() -> gdcore::id::ObjectId {
    let seq = NEXT_STREAM_PEER_TCP_HANDLE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let raw = STREAM_PEER_TCP_HANDLE_TAG | seq;
    stream_peer_tcp_handles()
        .get_or_insert_with(std::collections::HashMap::new)
        .insert(raw, StreamPeerTCP::new());
    gdcore::id::ObjectId::from_raw(raw)
}

/// Runs `f` against the live peer behind `id`, or returns `None` when the
/// handle was never issued or has already been released.
pub fn with_stream_peer_tcp<R>(
    id: gdcore::id::ObjectId,
    f: impl FnOnce(&mut StreamPeerTCP) -> R,
) -> Option<R> {
    let mut guard = stream_peer_tcp_handles();
    let map = guard.get_or_insert_with(std::collections::HashMap::new);
    map.get_mut(&id.raw()).map(f)
}

/// Drops the peer behind `id`, closing its socket.
pub fn release_stream_peer_tcp_handle(id: gdcore::id::ObjectId) -> bool {
    let mut guard = stream_peer_tcp_handles();
    let map = guard.get_or_insert_with(std::collections::HashMap::new);
    map.remove(&id.raw()).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn tcp_loopback_byte_stream() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();

        let mut client = StreamPeerTCP::new();
        client.connect_to_addr(addr).unwrap();
        assert_eq!(client.get_status(), StreamPeerTcpStatus::Connected);

        // Accept on server side.
        let start = Instant::now();
        let mut server = loop {
            match StreamPeerTCP::accept_from_listener(&listener) {
                Ok(s) => break s,
                Err(_) if start.elapsed() < Duration::from_secs(2) => {
                    std::thread::sleep(Duration::from_millis(1));
                }
                Err(e) => panic!("accept failed: {e}"),
            }
        };

        client.put_data(b"hello-tcp").unwrap();

        let start = Instant::now();
        while server.get_available_bytes() < 9 && start.elapsed() < Duration::from_secs(2) {
            server.poll();
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(server.get_data(64), b"hello-tcp");

        server.put_data(b"ack").unwrap();
        let start = Instant::now();
        while client.get_available_bytes() < 3 && start.elapsed() < Duration::from_secs(2) {
            client.poll();
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(client.get_data(64), b"ack");
    }

    #[test]
    fn status_none_when_new() {
        let p = StreamPeerTCP::new();
        assert_eq!(p.get_status(), StreamPeerTcpStatus::None);
    }
}
