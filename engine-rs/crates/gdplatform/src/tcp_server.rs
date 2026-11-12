//! `TCPServer` — a listening TCP socket that hands out [`StreamPeerTCP`]
//! connections (pat-s3rrv.178).
//!
//! Mirrors Godot's `TCPServer` (`doc/classes/TCPServer.xml`, which inherits the
//! shared `SocketServer` surface): `listen`, `is_listening`,
//! `is_connection_available`, `take_connection`, `stop` and `get_local_port`.
//!
//! # Why the accept is buffered
//!
//! Godot's `is_connection_available()` is `const`, but answering it truthfully
//! means asking the OS, and the only portable way to ask is to *accept*. So the
//! accepted socket is parked in `pending` and handed to the next
//! [`take_connection`](TcpServer::take_connection). Probing and then dropping
//! the socket would silently hang up on a client that had already connected.

use std::net::TcpListener;

use crate::stream_peer_tcp::StreamPeerTCP;

/// Godot's `Error.OK`.
pub const OK: i64 = 0;
/// Godot's `Error.FAILED`.
pub const FAILED: i64 = 1;
/// Godot's `Error.ERR_UNAVAILABLE` — enum position 2 in
/// `core/error/error_list.h`, *not* an errno-style value.
pub const ERR_UNAVAILABLE: i64 = 2;
/// Godot's `Error.ERR_ALREADY_IN_USE`.
pub const ERR_ALREADY_IN_USE: i64 = 22;

/// Whether this build can open sockets at all.
///
/// The web export has no socket layer; Godot keeps the classes present and
/// fails every operation with `ERR_UNAVAILABLE` rather than making them vanish,
/// so a script that merely *mentions* `TCPServer` still loads in a browser.
const SOCKETS_AVAILABLE: bool = !cfg!(target_arch = "wasm32");

/// A listening TCP socket.
///
/// No `Debug` derive: [`StreamPeerTCP`] does not implement it, and a socket's
/// debug output is not worth widening another type's public surface for.
#[derive(Default)]
pub struct TcpServer {
    listener: Option<TcpListener>,
    /// A connection accepted by `is_connection_available` and not yet taken.
    pending: Option<StreamPeerTCP>,
}

impl TcpServer {
    /// Creates a server that is not yet listening.
    pub fn new() -> Self {
        Self::default()
    }

    /// Binds and starts listening (`TCPServer.listen`).
    ///
    /// `bind_address` follows Godot: `"*"` means every interface. Returns a
    /// Godot `Error` code rather than a `Result`, because this is the value the
    /// script sees.
    pub fn listen(&mut self, port: u16, bind_address: &str) -> i64 {
        if !SOCKETS_AVAILABLE {
            return ERR_UNAVAILABLE;
        }
        if self.listener.is_some() {
            return ERR_ALREADY_IN_USE;
        }
        let host = match bind_address {
            "*" | "" => "0.0.0.0",
            other => other,
        };
        match TcpListener::bind((host, port)) {
            Ok(listener) => {
                // Non-blocking, or `is_connection_available` would park the
                // whole engine on the accept.
                if listener.set_nonblocking(true).is_err() {
                    return FAILED;
                }
                self.listener = Some(listener);
                OK
            }
            Err(_) => FAILED,
        }
    }

    /// Whether the server is bound (`SocketServer.is_listening`).
    pub fn is_listening(&self) -> bool {
        self.listener.is_some()
    }

    /// The bound port, or `0` when not listening (`TCPServer.get_local_port`).
    ///
    /// Binding port `0` asks the OS to choose one, so this is how a test learns
    /// where to connect.
    pub fn get_local_port(&self) -> u16 {
        self.listener
            .as_ref()
            .and_then(|l| l.local_addr().ok())
            .map(|a| a.port())
            .unwrap_or(0)
    }

    /// Whether a client is waiting (`SocketServer.is_connection_available`).
    ///
    /// Takes `&mut self` because answering means accepting; see the module
    /// note. Once a connection is parked, further calls are cheap and do not
    /// accept a second one, so a caller polling in a loop cannot starve the
    /// backlog into `pending`.
    pub fn is_connection_available(&mut self) -> bool {
        if self.pending.is_some() {
            return true;
        }
        let Some(listener) = self.listener.as_ref() else {
            return false;
        };
        // `accept_from_listener` already sets the socket non-blocking and
        // reports WouldBlock as an error when nobody is waiting, so an empty
        // backlog is simply `Err`.
        match StreamPeerTCP::accept_from_listener(listener) {
            Ok(peer) => {
                self.pending = Some(peer);
                true
            }
            Err(_) => false,
        }
    }

    /// Hands over the waiting connection (`TCPServer.take_connection`).
    ///
    /// Returns `None` when nobody is waiting — Godot returns a null object in
    /// that case, which the script layer turns into `Nil`.
    pub fn take_connection(&mut self) -> Option<StreamPeerTCP> {
        if self.pending.is_none() {
            self.is_connection_available();
        }
        self.pending.take()
    }

    /// Stops listening and drops any un-taken connection
    /// (`SocketServer.stop`).
    pub fn stop(&mut self) {
        self.listener = None;
        self.pending = None;
    }
}

/// Script-visible handle table for `TCPServer` (pat-s3rrv.178).
///
/// Mirrors `packet_peer_stream.rs` — same poison-tolerant table, same tag mask.
///
/// ⚠️ THE TAG BYTE IS A WORKSPACE-WIDE NAMESPACE. Two modules issuing the same
/// top byte makes `is_*_handle` answer true for the other module's ids, and the
/// symptom is a `MethodNotFound` naming the WRONG class. `0x61` was chosen after
/// sweeping every tag byte in the workspace — note `0x5D` looked free in a
/// truncated listing and is in fact WebRTC's.
const TCP_SERVER_HANDLE_TAG: u64 = 0x6100_0000_0000_0000;
static NEXT_TCP_SERVER_HANDLE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
static TCP_SERVER_HANDLES: std::sync::Mutex<Option<std::collections::HashMap<u64, TcpServer>>> =
    std::sync::Mutex::new(None);

fn tcp_server_handles(
) -> std::sync::MutexGuard<'static, Option<std::collections::HashMap<u64, TcpServer>>> {
    // A poisoned lock still holds a usable table: a panic in one script's
    // handler must not make every later call fail.
    TCP_SERVER_HANDLES
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Returns `true` when `id` was issued by [`new_tcp_server_handle`].
pub fn is_tcp_server_handle(id: gdcore::id::ObjectId) -> bool {
    id.raw() & 0xFF00_0000_0000_0000 == TCP_SERVER_HANDLE_TAG
}

/// Allocates a fresh [`TcpServer`] and returns its script-visible id
/// (Godot `TCPServer.new()`).
pub fn new_tcp_server_handle() -> gdcore::id::ObjectId {
    let seq = NEXT_TCP_SERVER_HANDLE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let raw = TCP_SERVER_HANDLE_TAG | seq;
    tcp_server_handles()
        .get_or_insert_with(std::collections::HashMap::new)
        .insert(raw, TcpServer::new());
    gdcore::id::ObjectId::from_raw(raw)
}

/// Runs `f` against the live server behind `id`, or returns `None` when the
/// handle was never issued or has already been released.
pub fn with_tcp_server<R>(
    id: gdcore::id::ObjectId,
    f: impl FnOnce(&mut TcpServer) -> R,
) -> Option<R> {
    let mut guard = tcp_server_handles();
    let map = guard.get_or_insert_with(std::collections::HashMap::new);
    map.get_mut(&id.raw()).map(f)
}

/// Drops the server behind `id`, closing its socket.
pub fn release_tcp_server_handle(id: gdcore::id::ObjectId) -> bool {
    let mut guard = tcp_server_handles();
    let map = guard.get_or_insert_with(std::collections::HashMap::new);
    map.remove(&id.raw()).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::net::TcpStream as StdStream;

    /// Binds to an OS-chosen port on loopback.
    fn ephemeral() -> TcpServer {
        let mut s = TcpServer::new();
        assert_eq!(s.listen(0, "127.0.0.1"), OK, "loopback bind should succeed");
        s
    }

    #[test]
    fn a_fresh_server_is_not_listening() {
        let s = TcpServer::new();
        assert!(!s.is_listening());
        assert_eq!(s.get_local_port(), 0, "no port before listening");
    }

    #[test]
    fn listen_binds_an_ephemeral_port() {
        let s = ephemeral();
        assert!(s.is_listening());
        assert_ne!(s.get_local_port(), 0, "port 0 means the OS picked one");
    }

    #[test]
    fn listening_twice_reports_already_in_use() {
        let mut s = ephemeral();
        assert_eq!(
            s.listen(0, "127.0.0.1"),
            ERR_ALREADY_IN_USE,
            "a second listen must not silently rebind and drop the first socket"
        );
    }

    #[test]
    fn no_connection_is_available_before_a_client_arrives() {
        let mut s = ephemeral();
        assert!(
            !s.is_connection_available(),
            "a non-blocking accept on an idle listener reports no client"
        );
        assert!(s.take_connection().is_none());
    }

    #[test]
    fn a_connected_client_is_accepted_and_handed_over() {
        let mut s = ephemeral();
        let port = s.get_local_port();
        let mut client = StdStream::connect(("127.0.0.1", port)).expect("client connects");
        client.write_all(b"hi").expect("client writes");

        // The accept may not have landed yet; poll briefly rather than sleeping
        // a fixed amount, so the test is not timing-fragile.
        let mut available = false;
        for _ in 0..200 {
            if s.is_connection_available() {
                available = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(available, "the server should see the connected client");

        let peer = s.take_connection();
        assert!(
            peer.is_some(),
            "take_connection hands over the accepted socket"
        );
        assert!(
            !s.is_connection_available(),
            "only one client connected, so the same connection must not be offered twice"
        );
        assert!(s.take_connection().is_none(), "and cannot be taken twice");

        // The client is deliberately still open until here: dropping it sooner
        // would let the OS tear the connection down, and the two assertions
        // above would then pass for the wrong reason.
        drop(client);
    }

    #[test]
    fn stop_releases_the_listener() {
        let mut s = ephemeral();
        s.stop();
        assert!(!s.is_listening());
        assert_eq!(s.get_local_port(), 0);
    }
}
