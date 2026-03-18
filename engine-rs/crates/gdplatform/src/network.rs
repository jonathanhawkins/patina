//! # Networking
//!
//! Multiplayer networking stubs: peer abstraction, RPC configuration,
//! spawner/synchronizer primitives, and an in-memory mock transport.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use gdvariant::Variant;
use thiserror::Error;

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Errors that can occur during networking operations.
#[derive(Debug, Error)]
pub enum NetworkError {
    /// No peer has been set on the [`MultiplayerAPI`].
    #[error("no multiplayer peer assigned")]
    NoPeer,
    /// The target peer is not reachable.
    #[error("peer {0} is not reachable")]
    PeerUnreachable(PeerId),
    /// The requested RPC method has not been registered.
    #[error("RPC method '{0}' is not registered")]
    UnregisteredRPC(String),
    /// Generic transport error.
    #[error("{0}")]
    Transport(String),
}

/// Convenience alias used throughout this module.
pub type Result<T> = std::result::Result<T, NetworkError>;

// ---------------------------------------------------------------------------
// PeerId
// ---------------------------------------------------------------------------

/// Unique identifier for a network peer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PeerId(pub u32);

impl PeerId {
    /// The well-known server peer identifier.
    pub const SERVER: PeerId = PeerId(1);
}

impl std::fmt::Display for PeerId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "PeerId({})", self.0)
    }
}

// ---------------------------------------------------------------------------
// TransferMode / ConnectionStatus
// ---------------------------------------------------------------------------

/// How a packet is delivered over the network.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferMode {
    /// Guaranteed delivery, any order.
    Reliable,
    /// Best-effort, no delivery guarantee.
    Unreliable,
    /// Guaranteed delivery **and** ordering within a channel.
    Ordered,
}

/// High-level connection status of a peer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionStatus {
    /// Not connected to any host.
    Disconnected,
    /// Handshake / connection in progress.
    Connecting,
    /// Fully connected.
    Connected,
}

// ---------------------------------------------------------------------------
// Packet
// ---------------------------------------------------------------------------

/// A single network packet.
#[derive(Debug, Clone)]
pub struct Packet {
    /// The peer that sent this packet.
    pub sender: PeerId,
    /// Logical channel number.
    pub channel: u8,
    /// Raw payload.
    pub data: Vec<u8>,
    /// Delivery guarantee for this packet.
    pub transfer_mode: TransferMode,
}

// ---------------------------------------------------------------------------
// NetworkPeer trait
// ---------------------------------------------------------------------------

/// Abstraction over a network transport (ENet, WebRTC, …).
pub trait NetworkPeer: Send {
    /// Send a packet to the given peer.
    fn send_packet(&mut self, to: PeerId, packet: Packet) -> Result<()>;
    /// Drain all packets that have arrived since the last call.
    fn poll(&mut self) -> Vec<Packet>;
    /// Return this peer's unique id on the network.
    fn get_unique_id(&self) -> PeerId;
    /// `true` when this peer is the server (id == 1).
    fn is_server(&self) -> bool;
    /// Current connection status.
    fn get_connection_status(&self) -> ConnectionStatus;
    /// Shut down the transport.
    fn close(&mut self);
}

// ---------------------------------------------------------------------------
// RPC types
// ---------------------------------------------------------------------------

/// Who is allowed to call a given RPC.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RPCMode {
    /// Only the node's authority may invoke this RPC.
    Authority,
    /// Any peer may invoke this RPC.
    Any,
}

/// Configuration for a registered RPC method.
#[derive(Debug, Clone)]
pub struct RPCConfig {
    /// Permission mode.
    pub mode: RPCMode,
    /// How the RPC packet is delivered.
    pub transfer_mode: TransferMode,
    /// Logical channel number.
    pub channel: u8,
}

/// A single RPC invocation.
#[derive(Debug, Clone)]
pub struct RPCCall {
    /// Name of the remote method.
    pub method_name: String,
    /// The peer that initiated the call.
    pub sender_id: PeerId,
    /// The peer that should execute the call.
    pub target_id: PeerId,
    /// Arguments encoded as Variants.
    pub args: Vec<Variant>,
}

// ---------------------------------------------------------------------------
// MultiplayerAPI
// ---------------------------------------------------------------------------

/// High-level multiplayer coordinator.
///
/// Wraps a [`NetworkPeer`] and manages RPC registration, authority tracking,
/// and convenience helpers used by higher-level scene replication.
pub struct MultiplayerAPI {
    peer: Option<Box<dyn NetworkPeer>>,
    rpc_configs: HashMap<String, RPCConfig>,
    /// Maps a node path (as `u64` key) to the authority peer.
    authority_map: HashMap<u64, PeerId>,
}

impl MultiplayerAPI {
    /// Create a new, peer-less [`MultiplayerAPI`].
    pub fn new() -> Self {
        Self {
            peer: None,
            rpc_configs: HashMap::new(),
            authority_map: HashMap::new(),
        }
    }

    /// Assign (or replace) the underlying transport.
    pub fn set_multiplayer_peer(&mut self, peer: Box<dyn NetworkPeer>) {
        self.peer = Some(peer);
    }

    /// Register an RPC method with its configuration.
    pub fn register_rpc(&mut self, method: impl Into<String>, config: RPCConfig) {
        self.rpc_configs.insert(method.into(), config);
    }

    /// Initiate an RPC call on `node_path` (hashed to `u64`).
    pub fn rpc(&mut self, node_path: u64, method: &str, args: Vec<Variant>) -> Result<()> {
        let config = self
            .rpc_configs
            .get(method)
            .ok_or_else(|| NetworkError::UnregisteredRPC(method.to_string()))?;

        let peer = self.peer.as_mut().ok_or(NetworkError::NoPeer)?;
        let sender_id = peer.get_unique_id();

        // Determine target: authority of the node, or server if unset.
        let target_id = self
            .authority_map
            .get(&node_path)
            .copied()
            .unwrap_or(PeerId::SERVER);

        let call = RPCCall {
            method_name: method.to_string(),
            sender_id,
            target_id,
            args: args.clone(),
        };

        // Serialize the RPC call into a simple packet payload.
        let data = format!("rpc:{}:{}", call.method_name, call.args.len()).into_bytes();

        let packet = Packet {
            sender: sender_id,
            channel: config.channel,
            data,
            transfer_mode: config.transfer_mode,
        };

        peer.send_packet(target_id, packet)
    }

    /// Get the authority peer for a given node path.
    pub fn get_authority(&self, node_path: u64) -> PeerId {
        self.authority_map
            .get(&node_path)
            .copied()
            .unwrap_or(PeerId::SERVER)
    }

    /// Set the authority peer for a given node path.
    pub fn set_authority(&mut self, node_path: u64, peer_id: PeerId) {
        self.authority_map.insert(node_path, peer_id);
    }

    /// `true` when the underlying peer is the server.
    pub fn is_server(&self) -> bool {
        self.peer.as_ref().map(|p| p.is_server()).unwrap_or(false)
    }
}

impl Default for MultiplayerAPI {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// MockNetworkPeer / MockNetwork
// ---------------------------------------------------------------------------

/// Shared queue used by [`MockNetworkPeer`] pairs.
type SharedQueue = Arc<Mutex<Vec<Packet>>>;

/// In-memory network peer for testing.
pub struct MockNetworkPeer {
    id: PeerId,
    status: ConnectionStatus,
    /// Packets waiting to be read by *this* peer.
    inbox: SharedQueue,
    /// Packets sent *by* this peer are pushed into the remote's inbox.
    remote_inbox: SharedQueue,
}

impl MockNetworkPeer {
    /// Create a standalone mock peer (no paired remote).
    pub fn new(id: PeerId) -> Self {
        Self {
            id,
            status: ConnectionStatus::Connected,
            inbox: Arc::new(Mutex::new(Vec::new())),
            remote_inbox: Arc::new(Mutex::new(Vec::new())),
        }
    }
}

impl NetworkPeer for MockNetworkPeer {
    fn send_packet(&mut self, _to: PeerId, packet: Packet) -> Result<()> {
        self.remote_inbox
            .lock()
            .expect("lock poisoned")
            .push(packet);
        Ok(())
    }

    fn poll(&mut self) -> Vec<Packet> {
        let mut inbox = self.inbox.lock().expect("lock poisoned");
        inbox.drain(..).collect()
    }

    fn get_unique_id(&self) -> PeerId {
        self.id
    }

    fn is_server(&self) -> bool {
        self.id == PeerId::SERVER
    }

    fn get_connection_status(&self) -> ConnectionStatus {
        self.status
    }

    fn close(&mut self) {
        self.status = ConnectionStatus::Disconnected;
    }
}

/// Creates paired [`MockNetworkPeer`]s that route packets to each other.
pub struct MockNetwork;

impl MockNetwork {
    /// Create two connected mock peers.
    ///
    /// The first peer is the **server** (`PeerId::SERVER`), the second is a
    /// client with the given `client_id`.
    pub fn create_pair(client_id: PeerId) -> (MockNetworkPeer, MockNetworkPeer) {
        let server_inbox: SharedQueue = Arc::new(Mutex::new(Vec::new()));
        let client_inbox: SharedQueue = Arc::new(Mutex::new(Vec::new()));

        let server = MockNetworkPeer {
            id: PeerId::SERVER,
            status: ConnectionStatus::Connected,
            inbox: Arc::clone(&server_inbox),
            remote_inbox: Arc::clone(&client_inbox),
        };

        let client = MockNetworkPeer {
            id: client_id,
            status: ConnectionStatus::Connected,
            inbox: Arc::clone(&client_inbox),
            remote_inbox: Arc::clone(&server_inbox),
        };

        (server, client)
    }
}

// ---------------------------------------------------------------------------
// MultiplayerSpawner / MultiplayerSynchronizer
// ---------------------------------------------------------------------------

/// Manages replication of node spawns across peers.
#[derive(Debug, Clone)]
pub struct MultiplayerSpawner {
    /// Scene paths that are eligible for automatic replication.
    pub tracked_paths: Vec<String>,
    /// Maximum number of spawns this spawner will replicate.
    pub spawn_limit: u32,
}

impl MultiplayerSpawner {
    /// Create a new spawner with no tracked paths.
    pub fn new(spawn_limit: u32) -> Self {
        Self {
            tracked_paths: Vec::new(),
            spawn_limit,
        }
    }

    /// Track an additional scene path.
    pub fn add_tracked_path(&mut self, path: impl Into<String>) {
        self.tracked_paths.push(path.into());
    }
}

/// Replicates property changes across peers at a configurable interval.
#[derive(Debug, Clone)]
pub struct MultiplayerSynchronizer {
    /// Property paths to synchronize.
    pub properties_to_sync: Vec<String>,
    /// Minimum interval between sync updates, in milliseconds.
    pub sync_interval_ms: u32,
}

impl MultiplayerSynchronizer {
    /// Create a new synchronizer.
    pub fn new(sync_interval_ms: u32) -> Self {
        Self {
            properties_to_sync: Vec::new(),
            sync_interval_ms,
        }
    }

    /// Add a property path to the synchronization set.
    pub fn add_property(&mut self, path: impl Into<String>) {
        self.properties_to_sync.push(path.into());
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn peer_id_server_constant() {
        assert_eq!(PeerId::SERVER.0, 1);
    }

    #[test]
    fn peer_id_equality() {
        assert_eq!(PeerId(42), PeerId(42));
        assert_ne!(PeerId(1), PeerId(2));
    }

    #[test]
    fn peer_id_display() {
        assert_eq!(PeerId(7).to_string(), "PeerId(7)");
    }

    #[test]
    fn transfer_mode_variants() {
        let modes = [
            TransferMode::Reliable,
            TransferMode::Unreliable,
            TransferMode::Ordered,
        ];
        assert_eq!(modes.len(), 3);
    }

    #[test]
    fn connection_status_variants() {
        let s = ConnectionStatus::Disconnected;
        assert_ne!(s, ConnectionStatus::Connected);
        assert_eq!(s, ConnectionStatus::Disconnected);
    }

    #[test]
    fn packet_creation() {
        let pkt = Packet {
            sender: PeerId(5),
            channel: 0,
            data: vec![1, 2, 3],
            transfer_mode: TransferMode::Reliable,
        };
        assert_eq!(pkt.sender, PeerId(5));
        assert_eq!(pkt.data.len(), 3);
    }

    #[test]
    fn mock_peer_basics() {
        let mut peer = MockNetworkPeer::new(PeerId::SERVER);
        assert!(peer.is_server());
        assert_eq!(peer.get_unique_id(), PeerId::SERVER);
        assert_eq!(peer.get_connection_status(), ConnectionStatus::Connected);
        peer.close();
        assert_eq!(peer.get_connection_status(), ConnectionStatus::Disconnected);
    }

    #[test]
    fn mock_peer_not_server() {
        let peer = MockNetworkPeer::new(PeerId(2));
        assert!(!peer.is_server());
    }

    #[test]
    fn mock_network_pair_routing() {
        let (mut server, mut client) = MockNetwork::create_pair(PeerId(2));

        // Client sends to server.
        let pkt = Packet {
            sender: PeerId(2),
            channel: 0,
            data: b"hello".to_vec(),
            transfer_mode: TransferMode::Reliable,
        };
        client.send_packet(PeerId::SERVER, pkt).unwrap();

        let received = server.poll();
        assert_eq!(received.len(), 1);
        assert_eq!(received[0].data, b"hello");
    }

    #[test]
    fn mock_network_bidirectional() {
        let (mut server, mut client) = MockNetwork::create_pair(PeerId(2));

        server
            .send_packet(
                PeerId(2),
                Packet {
                    sender: PeerId::SERVER,
                    channel: 1,
                    data: b"from-server".to_vec(),
                    transfer_mode: TransferMode::Ordered,
                },
            )
            .unwrap();

        let msgs = client.poll();
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].data, b"from-server");
        assert_eq!(msgs[0].channel, 1);
    }

    #[test]
    fn mock_poll_drains() {
        let (mut server, mut client) = MockNetwork::create_pair(PeerId(2));

        client
            .send_packet(
                PeerId::SERVER,
                Packet {
                    sender: PeerId(2),
                    channel: 0,
                    data: b"a".to_vec(),
                    transfer_mode: TransferMode::Unreliable,
                },
            )
            .unwrap();

        assert_eq!(server.poll().len(), 1);
        // Second poll should be empty.
        assert!(server.poll().is_empty());
    }

    #[test]
    fn multiplayer_api_default() {
        let api = MultiplayerAPI::default();
        assert!(!api.is_server());
    }

    #[test]
    fn multiplayer_api_set_peer() {
        let mut api = MultiplayerAPI::new();
        let (server, _client) = MockNetwork::create_pair(PeerId(2));
        api.set_multiplayer_peer(Box::new(server));
        assert!(api.is_server());
    }

    #[test]
    fn multiplayer_api_authority() {
        let mut api = MultiplayerAPI::new();
        // Default authority is SERVER.
        assert_eq!(api.get_authority(100), PeerId::SERVER);

        api.set_authority(100, PeerId(5));
        assert_eq!(api.get_authority(100), PeerId(5));
    }

    #[test]
    fn multiplayer_api_rpc_unregistered() {
        let mut api = MultiplayerAPI::new();
        let (server, _client) = MockNetwork::create_pair(PeerId(2));
        api.set_multiplayer_peer(Box::new(server));

        let err = api.rpc(1, "nonexistent", vec![]).unwrap_err();
        assert!(matches!(err, NetworkError::UnregisteredRPC(_)));
    }

    #[test]
    fn multiplayer_api_rpc_no_peer() {
        let mut api = MultiplayerAPI::new();
        api.register_rpc(
            "my_rpc",
            RPCConfig {
                mode: RPCMode::Any,
                transfer_mode: TransferMode::Reliable,
                channel: 0,
            },
        );
        let err = api.rpc(1, "my_rpc", vec![]).unwrap_err();
        assert!(matches!(err, NetworkError::NoPeer));
    }

    #[test]
    fn multiplayer_api_rpc_success() {
        let mut api = MultiplayerAPI::new();
        let (server, _client) = MockNetwork::create_pair(PeerId(2));
        api.set_multiplayer_peer(Box::new(server));
        api.register_rpc(
            "sync_pos",
            RPCConfig {
                mode: RPCMode::Authority,
                transfer_mode: TransferMode::Reliable,
                channel: 0,
            },
        );

        api.rpc(42, "sync_pos", vec![Variant::Int(10)]).unwrap();
    }

    #[test]
    fn spawner_basics() {
        let mut spawner = MultiplayerSpawner::new(10);
        assert!(spawner.tracked_paths.is_empty());
        assert_eq!(spawner.spawn_limit, 10);

        spawner.add_tracked_path("res://player.tscn");
        assert_eq!(spawner.tracked_paths.len(), 1);
    }

    #[test]
    fn synchronizer_basics() {
        let mut sync = MultiplayerSynchronizer::new(50);
        assert!(sync.properties_to_sync.is_empty());
        assert_eq!(sync.sync_interval_ms, 50);

        sync.add_property("position");
        sync.add_property("rotation");
        assert_eq!(sync.properties_to_sync.len(), 2);
    }
}
