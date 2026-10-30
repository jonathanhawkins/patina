//! Script-constructible `MultiplayerPeer` handles (pat-s3rrv.16).
//!
//! Every Godot multiplayer project opens with three lines of GDScript:
//!
//! ```gdscript
//! var peer = WebSocketMultiplayerPeer.new()
//! peer.create_server(8080)
//! multiplayer.multiplayer_peer = peer
//! ```
//!
//! Patina's transports were real and complete — [`OfflineMultiplayerPeer`] and
//! [`WebSocketMultiplayerPeer`] both implement [`NetworkPeer`] — but nothing in
//! a script could reach them: no `MultiplayerPeer` subclass was constructible,
//! and the only callers were Rust tests injecting a peer directly into the
//! tree's `MultiplayerAPI`.
//!
//! A `MultiplayerPeer` is RefCounted in Godot, not a Node, so it cannot live in
//! the scene tree. It gets the same treatment as `WebSocketPeer` / ConfigFile /
//! FileAccess: the object lives in a process-wide table and scripts hold a
//! tagged `ObjectId` into it.
//!
//! **Why a concrete enum instead of `Box<dyn NetworkPeer>`:**
//! `WebSocketMultiplayerPeer::create_server` is generic over `A: ToSocketAddrs`,
//! so it is not object-safe and cannot be reached through `dyn NetworkPeer`.
//! Storing the concrete variant keeps `create_server` / `create_client`
//! callable, and [`take_multiplayer_peer`] boxes it on the way out — which is
//! the only moment the trait object is actually needed.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard};

use gdcore::id::ObjectId;

use crate::network::{ConnectionStatus, NetworkError, NetworkPeer, OfflineMultiplayerPeer, PeerId};
use crate::websocket_multiplayer::WebSocketMultiplayerPeer;

/// Tags an `ObjectId` issued by [`new_multiplayer_peer_handle`]. `0x4D` is
/// ASCII `'M'`; it sits alongside the WebSocketPeer / ConfigFile / FileAccess /
/// DirAccess / CameraFeed / UndoRedo tags (`0x57` / `0xCF` / `0xFA` / `0xDA` /
/// `0xCA` / `0xED`).
const MULTIPLAYER_PEER_HANDLE_TAG: u64 = 0x4D00_0000_0000_0000;
static NEXT_MULTIPLAYER_PEER_HANDLE: AtomicU64 = AtomicU64::new(1);
static MULTIPLAYER_PEER_HANDLES: Mutex<Option<HashMap<u64, MultiplayerPeerHandle>>> =
    Mutex::new(None);

/// A peer living in the handle table, kept concrete so the transport-specific
/// constructors stay reachable.
pub enum MultiplayerPeerHandle {
    /// Godot `OfflineMultiplayerPeer` — the default single-player peer, always
    /// "connected" as the server with unique id 1.
    Offline(OfflineMultiplayerPeer),
    /// Godot `WebSocketMultiplayerPeer`.
    WebSocket(Box<WebSocketMultiplayerPeer>),
}

impl MultiplayerPeerHandle {
    /// The peer as a `NetworkPeer`, for the calls that are transport-agnostic.
    fn as_network_peer(&self) -> &dyn NetworkPeer {
        match self {
            MultiplayerPeerHandle::Offline(p) => p,
            MultiplayerPeerHandle::WebSocket(p) => p.as_ref(),
        }
    }

    /// Mutable counterpart of [`Self::as_network_peer`].
    fn as_network_peer_mut(&mut self) -> &mut dyn NetworkPeer {
        match self {
            MultiplayerPeerHandle::Offline(p) => p,
            MultiplayerPeerHandle::WebSocket(p) => p.as_mut(),
        }
    }

    /// Consumes the handle into the boxed trait object `MultiplayerAPI` stores.
    fn into_network_peer(self) -> Box<dyn NetworkPeer> {
        match self {
            MultiplayerPeerHandle::Offline(p) => Box::new(p),
            MultiplayerPeerHandle::WebSocket(p) => p,
        }
    }

    /// `MultiplayerPeer.get_unique_id()`.
    pub fn get_unique_id(&self) -> i64 {
        i64::from(self.as_network_peer().get_unique_id().0)
    }

    /// `WebSocketMultiplayerPeer.get_peer_address(id)` (pat-s3rrv.81).
    ///
    /// ⚠️ NOT on the `NetworkPeer` trait, deliberately. Godot declares these two
    /// on `WebSocketMultiplayerPeer`, not on `MultiplayerPeer` — ENet exposes
    /// the same information through `ENetPacketPeer` instead — so putting them
    /// on the shared trait would advertise them for every transport and force
    /// four implementors to answer a question their protocol does not ask.
    /// Matching on the handle keeps the surface exactly where Godot puts it.
    ///
    /// `Offline` answers with Godot's not-found values (`""` / `0`): it has no
    /// remote peers at all, so every id is unknown to it.
    pub fn get_peer_address(&self, peer: i64) -> String {
        match self {
            MultiplayerPeerHandle::Offline(_) => String::new(),
            MultiplayerPeerHandle::WebSocket(p) => match u32::try_from(peer) {
                Ok(id) => p.get_peer_address(PeerId(id)),
                Err(_) => String::new(),
            },
        }
    }

    /// `WebSocketMultiplayerPeer.get_peer_port(id)` (pat-s3rrv.81).
    pub fn get_peer_port(&self, peer: i64) -> i64 {
        match self {
            MultiplayerPeerHandle::Offline(_) => 0,
            MultiplayerPeerHandle::WebSocket(p) => match u32::try_from(peer) {
                Ok(id) => p.get_peer_port(PeerId(id)),
                Err(_) => 0,
            },
        }
    }

    /// `MultiplayerPeer.get_connection_status()` as Godot's
    /// `CONNECTION_DISCONNECTED` / `CONNECTING` / `CONNECTED` (0 / 1 / 2).
    pub fn get_connection_status(&self) -> i64 {
        match self.as_network_peer().get_connection_status() {
            ConnectionStatus::Disconnected => 0,
            ConnectionStatus::Connecting => 1,
            ConnectionStatus::Connected => 2,
        }
    }

    /// `MultiplayerAPI.is_server()` for this peer.
    pub fn is_server(&self) -> bool {
        self.as_network_peer().is_server()
    }

    /// `MultiplayerPeer.poll()` — drains and discards, since a peer that is not
    /// yet assigned to the tree has nowhere to deliver packets.
    pub fn poll(&mut self) {
        let _ = self.as_network_peer_mut().poll();
    }

    /// `MultiplayerPeer.close()`.
    pub fn close(&mut self) {
        self.as_network_peer_mut().close();
    }

    /// `WebSocketMultiplayerPeer.create_server(port)`. `Err` for a transport
    /// that has no server role (the offline peer, or any peer in a browser
    /// build), matching Godot's `ERR_*` return rather than panicking.
    ///
    /// Carries the [`NetworkError`] rather than a flattened `String` so the
    /// caller can reach [`NetworkError::godot_error_code`]: a browser build
    /// owes the script `ERR_UNAVAILABLE`, not `FAILED` (pat-s3rrv.264).
    pub fn create_server(&mut self, port: u16) -> Result<(), NetworkError> {
        match self {
            MultiplayerPeerHandle::WebSocket(p) => p.create_server(("127.0.0.1", port)).map(|_| ()),
            // ⚠️ `Transport`, not `Unavailable`: this keeps the offline peer's
            // script-visible code at `FAILED`, which is what it has always
            // returned. Only the browser's missing server role is
            // `ERR_UNAVAILABLE`.
            MultiplayerPeerHandle::Offline(_) => Err(NetworkError::Transport(
                "OfflineMultiplayerPeer has no server role".to_string(),
            )),
        }
    }

    /// `WebSocketMultiplayerPeer.create_client(url)`.
    pub fn create_client(&mut self, url: &str) -> Result<(), NetworkError> {
        match self {
            MultiplayerPeerHandle::WebSocket(p) => p.create_client(url),
            MultiplayerPeerHandle::Offline(_) => Err(NetworkError::Transport(
                "OfflineMultiplayerPeer has no client role".to_string(),
            )),
        }
    }
}

fn peer_handles() -> MutexGuard<'static, Option<HashMap<u64, MultiplayerPeerHandle>>> {
    // A poisoned lock still holds a usable table: a panic in one script's
    // handler must not make every later multiplayer call fail.
    MULTIPLAYER_PEER_HANDLES
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Returns `true` when `id` was issued by [`new_multiplayer_peer_handle`].
pub fn is_multiplayer_peer_handle(id: ObjectId) -> bool {
    id.raw() & 0xFF00_0000_0000_0000 == MULTIPLAYER_PEER_HANDLE_TAG
}

/// Registers `peer` in the process-wide table and returns its script-visible id
/// (Godot `OfflineMultiplayerPeer.new()` / `WebSocketMultiplayerPeer.new()`).
pub fn new_multiplayer_peer_handle(peer: MultiplayerPeerHandle) -> ObjectId {
    let seq = NEXT_MULTIPLAYER_PEER_HANDLE.fetch_add(1, Ordering::Relaxed);
    let raw = MULTIPLAYER_PEER_HANDLE_TAG | seq;
    peer_handles()
        .get_or_insert_with(HashMap::new)
        .insert(raw, peer);
    ObjectId::from_raw(raw)
}

/// Convenience constructor for `OfflineMultiplayerPeer.new()`.
pub fn new_offline_multiplayer_peer_handle() -> ObjectId {
    new_multiplayer_peer_handle(MultiplayerPeerHandle::Offline(OfflineMultiplayerPeer::new()))
}

/// Convenience constructor for `WebSocketMultiplayerPeer.new()`.
pub fn new_ws_multiplayer_peer_handle() -> ObjectId {
    new_multiplayer_peer_handle(MultiplayerPeerHandle::WebSocket(Box::new(
        WebSocketMultiplayerPeer::new(),
    )))
}

/// Runs `f` against the live peer behind `id`, or returns `None` when the handle
/// was never issued or has already been taken/released.
pub fn with_multiplayer_peer<R>(
    id: ObjectId,
    f: impl FnOnce(&mut MultiplayerPeerHandle) -> R,
) -> Option<R> {
    let mut guard = peer_handles();
    let map = guard.get_or_insert_with(HashMap::new);
    map.get_mut(&id.raw()).map(f)
}

/// Removes the peer from the table and hands over ownership, for
/// `multiplayer.multiplayer_peer = peer`.
///
/// The peer LEAVES the table because `MultiplayerAPI` takes it by value; a
/// script holding the old handle afterwards is addressing a peer the tree owns,
/// exactly as a Godot script holding a `Ref` the API has adopted.
pub fn take_multiplayer_peer(id: ObjectId) -> Option<Box<dyn NetworkPeer>> {
    let mut guard = peer_handles();
    let map = guard.as_mut()?;
    map.remove(&id.raw())
        .map(MultiplayerPeerHandle::into_network_peer)
}

/// Drops a handle, closing the transport if it is still open. Idempotent.
pub fn release_multiplayer_peer_handle(id: ObjectId) {
    let mut guard = peer_handles();
    if let Some(map) = guard.as_mut() {
        if let Some(mut peer) = map.remove(&id.raw()) {
            peer.close();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offline_handle_reports_server_identity() {
        let id = new_offline_multiplayer_peer_handle();
        assert!(is_multiplayer_peer_handle(id), "id must carry the tag");
        let (server, unique, status) = with_multiplayer_peer(id, |p| {
            (p.is_server(), p.get_unique_id(), p.get_connection_status())
        })
        .expect("handle is live");
        assert!(server, "the offline peer is always the server");
        assert_eq!(unique, 1, "and always has unique id 1");
        assert_eq!(status, 2, "and reports CONNECTION_CONNECTED");
    }

    #[test]
    fn taking_a_peer_empties_its_slot() {
        let id = new_offline_multiplayer_peer_handle();
        assert!(take_multiplayer_peer(id).is_some(), "first take hands over");
        assert!(
            take_multiplayer_peer(id).is_none(),
            "the slot must be empty afterwards — the tree owns the peer now"
        );
        assert!(
            with_multiplayer_peer(id, |_| ()).is_none(),
            "and the handle must no longer resolve"
        );
    }

    #[test]
    fn a_foreign_id_is_not_a_peer_handle() {
        assert!(
            !is_multiplayer_peer_handle(ObjectId::from_raw(42)),
            "a plain node id must not be mistaken for a peer handle"
        );
    }
}
