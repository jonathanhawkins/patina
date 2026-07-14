//! Peer-management + connection-signal model for `MultiplayerPeer` (pat-u3l8c.81).
//!
//! Godot's `MultiplayerPeer` tracks the set of connected peers, emits
//! `peer_connected(id)` / `peer_disconnected(id)` signals, exposes `get_peers()`,
//! and routes outgoing packets through a "target peer" selector
//! (`set_target_peer`). Before this module the transports (`TcpMultiplayerPeer`,
//! ENet, …) tracked raw streams but exposed **none** of that Godot-facing peer
//! surface (grep zero for peer signals / management methods).
//!
//! This provides that logic as a transport-agnostic state object any concrete
//! transport can embed. It is pure data — no sockets — so it is deterministically
//! unit-testable. Peer ids are `i32` to match Godot's signed peer-id semantics,
//! where a **negative** target peer means "all peers except `-id`".

use std::collections::BTreeSet;

/// Godot's `MultiplayerPeer.TARGET_PEER_BROADCAST` — send to every peer.
pub const TARGET_PEER_BROADCAST: i32 = 0;

/// A connection-lifecycle signal, mirroring Godot's `MultiplayerPeer`
/// `peer_connected(id)` / `peer_disconnected(id)` signals.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MultiplayerPeerSignal {
    /// A peer connected; carries the new peer's id.
    PeerConnected(i32),
    /// A peer disconnected; carries the departed peer's id.
    PeerDisconnected(i32),
}

/// Transport-agnostic peer registry, target-peer routing, and pending
/// connection-signal queue for a `MultiplayerPeer`.
///
/// A concrete transport calls [`add_peer`](Self::add_peer) /
/// [`remove_peer`](Self::remove_peer) as its sockets connect and drop, then
/// drains [`drain_signals`](Self::drain_signals) each frame to fire the
/// corresponding Godot signals. Outgoing sends consult
/// [`should_route_to`](Self::should_route_to) / [`routed_peers`](Self::routed_peers)
/// to honour the current target selector.
#[derive(Debug, Clone, Default)]
pub struct MultiplayerPeerState {
    peers: BTreeSet<i32>,
    target_peer: i32,
    pending_signals: Vec<MultiplayerPeerSignal>,
}

impl MultiplayerPeerState {
    /// Creates an empty state that broadcasts to all peers.
    pub fn new() -> Self {
        Self {
            peers: BTreeSet::new(),
            target_peer: TARGET_PEER_BROADCAST,
            pending_signals: Vec::new(),
        }
    }

    /// Registers a newly connected peer and queues a `peer_connected` signal.
    /// Re-adding an already-known peer is a no-op (no duplicate signal).
    /// Returns `true` if the peer was newly added.
    pub fn add_peer(&mut self, id: i32) -> bool {
        if self.peers.insert(id) {
            self.pending_signals
                .push(MultiplayerPeerSignal::PeerConnected(id));
            true
        } else {
            false
        }
    }

    /// Removes a peer and queues a `peer_disconnected` signal. Removing an
    /// unknown peer is a no-op. Returns `true` if a peer was actually removed.
    pub fn remove_peer(&mut self, id: i32) -> bool {
        if self.peers.remove(&id) {
            self.pending_signals
                .push(MultiplayerPeerSignal::PeerDisconnected(id));
            true
        } else {
            false
        }
    }

    /// Godot `disconnect_peer` — drops the peer and emits the disconnect signal.
    /// Alias for [`remove_peer`](Self::remove_peer).
    pub fn disconnect_peer(&mut self, id: i32) -> bool {
        self.remove_peer(id)
    }

    /// `true` if `id` is currently connected.
    pub fn has_peer(&self, id: i32) -> bool {
        self.peers.contains(&id)
    }

    /// Connected peer ids in ascending order (Godot `get_peers`).
    pub fn get_peers(&self) -> Vec<i32> {
        self.peers.iter().copied().collect()
    }

    /// Number of connected peers.
    pub fn get_peer_count(&self) -> usize {
        self.peers.len()
    }

    /// Sets the routing target for subsequent sends (Godot `set_target_peer`).
    /// `0` = broadcast, `> 0` = only that peer, `< 0` = all except `-id`.
    pub fn set_target_peer(&mut self, id: i32) {
        self.target_peer = id;
    }

    /// Current routing target (Godot `get_target_peer`).
    pub fn get_target_peer(&self) -> i32 {
        self.target_peer
    }

    /// Whether an outgoing packet under the current target selector should be
    /// delivered to `peer`. Implements Godot's routing rules: broadcast (`0`)
    /// → all; positive → exactly that peer; negative → all except `-target`.
    pub fn should_route_to(&self, peer: i32) -> bool {
        match self.target_peer {
            TARGET_PEER_BROADCAST => true,
            t if t > 0 => peer == t,
            t => peer != -t,
        }
    }

    /// The connected peers an outgoing packet should reach under the current
    /// target selector, in ascending id order.
    pub fn routed_peers(&self) -> Vec<i32> {
        self.peers
            .iter()
            .copied()
            .filter(|&p| self.should_route_to(p))
            .collect()
    }

    /// Drains and returns all queued connection signals in emission order.
    pub fn drain_signals(&mut self) -> Vec<MultiplayerPeerSignal> {
        std::mem::take(&mut self.pending_signals)
    }

    /// `true` if there are queued signals awaiting drain.
    pub fn has_pending_signals(&self) -> bool {
        !self.pending_signals.is_empty()
    }

    /// Clears all peers, pending signals, and the target selector (transport
    /// close/reset). Does **not** emit disconnect signals — this models a hard
    /// teardown rather than an orderly per-peer disconnect.
    pub fn reset(&mut self) {
        self.peers.clear();
        self.pending_signals.clear();
        self.target_peer = TARGET_PEER_BROADCAST;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_peer_queues_connected_signal_once() {
        let mut s = MultiplayerPeerState::new();
        assert!(s.add_peer(2));
        assert!(!s.add_peer(2)); // duplicate — no second signal
        assert_eq!(s.get_peers(), vec![2]);
        assert_eq!(
            s.drain_signals(),
            vec![MultiplayerPeerSignal::PeerConnected(2)]
        );
        assert!(s.drain_signals().is_empty()); // drained
    }

    #[test]
    fn remove_peer_queues_disconnected_signal() {
        let mut s = MultiplayerPeerState::new();
        s.add_peer(2);
        s.add_peer(3);
        let _ = s.drain_signals(); // clear the connect signals
        assert!(s.remove_peer(2));
        assert!(!s.remove_peer(2)); // already gone — no-op
        assert_eq!(s.get_peers(), vec![3]);
        assert_eq!(
            s.drain_signals(),
            vec![MultiplayerPeerSignal::PeerDisconnected(2)]
        );
    }

    #[test]
    fn get_peers_is_sorted_ascending() {
        let mut s = MultiplayerPeerState::new();
        s.add_peer(9);
        s.add_peer(2);
        s.add_peer(5);
        assert_eq!(s.get_peers(), vec![2, 5, 9]);
        assert_eq!(s.get_peer_count(), 3);
        assert!(s.has_peer(5));
        assert!(!s.has_peer(7));
    }

    #[test]
    fn target_peer_broadcast_routes_to_all() {
        let mut s = MultiplayerPeerState::new();
        s.add_peer(2);
        s.add_peer(3);
        assert_eq!(s.get_target_peer(), TARGET_PEER_BROADCAST);
        assert!(s.should_route_to(2));
        assert!(s.should_route_to(3));
        assert_eq!(s.routed_peers(), vec![2, 3]);
    }

    #[test]
    fn target_peer_positive_routes_to_one() {
        let mut s = MultiplayerPeerState::new();
        s.add_peer(2);
        s.add_peer(3);
        s.set_target_peer(3);
        assert!(!s.should_route_to(2));
        assert!(s.should_route_to(3));
        assert_eq!(s.routed_peers(), vec![3]);
    }

    #[test]
    fn target_peer_negative_excludes_one() {
        let mut s = MultiplayerPeerState::new();
        s.add_peer(2);
        s.add_peer(3);
        s.add_peer(4);
        s.set_target_peer(-3); // all peers except peer 3
        assert!(s.should_route_to(2));
        assert!(!s.should_route_to(3));
        assert!(s.should_route_to(4));
        assert_eq!(s.routed_peers(), vec![2, 4]);
    }

    #[test]
    fn disconnect_peer_alias_and_reset() {
        let mut s = MultiplayerPeerState::new();
        s.add_peer(2);
        s.set_target_peer(2);
        let _ = s.drain_signals();
        assert!(s.disconnect_peer(2));
        assert_eq!(
            s.drain_signals(),
            vec![MultiplayerPeerSignal::PeerDisconnected(2)]
        );
        s.add_peer(5);
        s.reset();
        assert!(s.get_peers().is_empty());
        assert!(!s.has_pending_signals());
        assert_eq!(s.get_target_peer(), TARGET_PEER_BROADCAST);
    }
}
