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
    /// Decoded inbound RPC calls (filled by [`Self::poll`]).
    inbound_rpcs: Vec<DecodedRPC>,
}

/// An inbound RPC decoded from the binary wire format (pat-p0s82.4).
#[derive(Debug, Clone, PartialEq)]
pub struct DecodedRPC {
    /// Hashed node path the call targets.
    pub node_path: u64,
    /// Method name.
    pub method_name: String,
    /// Peer that sent the call.
    pub sender_id: PeerId,
    /// Decoded arguments.
    pub args: Vec<Variant>,
}

/// Magic prefix for binary RPC packets (`PRPC`).
pub const RPC_MAGIC: &[u8; 4] = b"PRPC";

impl MultiplayerAPI {
    /// Create a new, peer-less [`MultiplayerAPI`].
    pub fn new() -> Self {
        Self {
            peer: None,
            rpc_configs: HashMap::new(),
            authority_map: HashMap::new(),
            inbound_rpcs: Vec::new(),
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
    ///
    /// Payload uses the binary [`encode_rpc_packet`] framing (not a text toy).
    pub fn rpc(&mut self, node_path: u64, method: &str, args: Vec<Variant>) -> Result<()> {
        let config = self
            .rpc_configs
            .get(method)
            .ok_or_else(|| NetworkError::UnregisteredRPC(method.to_string()))?
            .clone();

        let peer = self.peer.as_mut().ok_or(NetworkError::NoPeer)?;
        let sender_id = peer.get_unique_id();

        // Determine target: authority of the node, or server if unset.
        let target_id = self
            .authority_map
            .get(&node_path)
            .copied()
            .unwrap_or(PeerId::SERVER);

        let data = encode_rpc_packet(node_path, method, &args);

        let packet = Packet {
            sender: sender_id,
            channel: config.channel,
            data,
            transfer_mode: config.transfer_mode,
        };

        peer.send_packet(target_id, packet)
    }

    /// Poll the transport and decode inbound RPC / leave non-RPC packets to caller.
    ///
    /// Returns non-RPC packets (spawn/sync/etc.) for higher layers. Decoded
    /// RPCs are available via [`Self::take_inbound_rpcs`].
    pub fn poll(&mut self) -> Vec<Packet> {
        let Some(peer) = self.peer.as_mut() else {
            return Vec::new();
        };
        let mut other = Vec::new();
        for packet in peer.poll() {
            if let Some(rpc) = decode_rpc_packet(&packet.data) {
                self.inbound_rpcs.push(DecodedRPC {
                    node_path: rpc.0,
                    method_name: rpc.1,
                    sender_id: packet.sender,
                    args: rpc.2,
                });
            } else {
                other.push(packet);
            }
        }
        other
    }

    /// Drain decoded inbound RPCs.
    pub fn take_inbound_rpcs(&mut self) -> Vec<DecodedRPC> {
        std::mem::take(&mut self.inbound_rpcs)
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

    /// Mutable access to the peer for advanced integration (spawner/sync).
    pub fn peer_mut(&mut self) -> Option<&mut (dyn NetworkPeer + 'static)> {
        self.peer.as_deref_mut()
    }
}

// ---------------------------------------------------------------------------
// Binary RPC framing (Variant subset) — pat-p0s82.4
// ---------------------------------------------------------------------------

/// Encode an RPC into the binary wire format:
/// `PRPC | node_path:u64 | method_len:u16 | method | argc:u16 | args…`
pub fn encode_rpc_packet(node_path: u64, method: &str, args: &[Variant]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(RPC_MAGIC);
    out.extend_from_slice(&node_path.to_be_bytes());
    let mb = method.as_bytes();
    out.extend_from_slice(&(mb.len() as u16).to_be_bytes());
    out.extend_from_slice(mb);
    out.extend_from_slice(&(args.len() as u16).to_be_bytes());
    for a in args {
        encode_variant(a, &mut out);
    }
    out
}

/// Decode a binary RPC packet. Returns `(node_path, method, args)`.
pub fn decode_rpc_packet(data: &[u8]) -> Option<(u64, String, Vec<Variant>)> {
    if data.len() < 4 + 8 + 2 {
        return None;
    }
    if &data[0..4] != RPC_MAGIC {
        return None;
    }
    let mut i = 4;
    let node_path = u64::from_be_bytes(data[i..i + 8].try_into().ok()?);
    i += 8;
    let mlen = u16::from_be_bytes(data[i..i + 2].try_into().ok()?) as usize;
    i += 2;
    if i + mlen > data.len() {
        return None;
    }
    let method = String::from_utf8(data[i..i + mlen].to_vec()).ok()?;
    i += mlen;
    if i + 2 > data.len() {
        return None;
    }
    let argc = u16::from_be_bytes(data[i..i + 2].try_into().ok()?) as usize;
    i += 2;
    let mut args = Vec::with_capacity(argc);
    for _ in 0..argc {
        let (v, n) = decode_variant(&data[i..])?;
        args.push(v);
        i += n;
    }
    Some((node_path, method, args))
}

fn encode_variant(v: &Variant, out: &mut Vec<u8>) {
    match v {
        Variant::Nil => out.push(0),
        Variant::Bool(b) => {
            out.push(1);
            out.push(if *b { 1 } else { 0 });
        }
        Variant::Int(i) => {
            out.push(2);
            out.extend_from_slice(&i.to_be_bytes());
        }
        Variant::Float(f) => {
            out.push(3);
            out.extend_from_slice(&f.to_bits().to_be_bytes());
        }
        Variant::String(s) => {
            out.push(4);
            let b = s.as_bytes();
            out.extend_from_slice(&(b.len() as u32).to_be_bytes());
            out.extend_from_slice(b);
        }
        // Fallback: stringify unknown types
        other => {
            out.push(4);
            let s = format!("{other}");
            let b = s.as_bytes();
            out.extend_from_slice(&(b.len() as u32).to_be_bytes());
            out.extend_from_slice(b);
        }
    }
}

fn decode_variant(data: &[u8]) -> Option<(Variant, usize)> {
    if data.is_empty() {
        return None;
    }
    match data[0] {
        0 => Some((Variant::Nil, 1)),
        1 => {
            if data.len() < 2 {
                return None;
            }
            Some((Variant::Bool(data[1] != 0), 2))
        }
        2 => {
            if data.len() < 9 {
                return None;
            }
            let i = i64::from_be_bytes(data[1..9].try_into().ok()?);
            Some((Variant::Int(i), 9))
        }
        3 => {
            if data.len() < 9 {
                return None;
            }
            let bits = u64::from_be_bytes(data[1..9].try_into().ok()?);
            Some((Variant::Float(f64::from_bits(bits)), 9))
        }
        4 => {
            if data.len() < 5 {
                return None;
            }
            let len = u32::from_be_bytes(data[1..5].try_into().ok()?) as usize;
            if data.len() < 5 + len {
                return None;
            }
            let s = String::from_utf8(data[5..5 + len].to_vec()).ok()?;
            Some((Variant::String(s), 5 + len))
        }
        _ => None,
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
// MultiplayerSpawner / MultiplayerSynchronizer (pat-ikq04)
// ---------------------------------------------------------------------------

/// Magic byte for a spawn replication packet.
pub const REPLICATION_KIND_SPAWN: u8 = 0x01;
/// Magic byte for a property-sync replication packet.
pub const REPLICATION_KIND_SYNC: u8 = 0x02;

/// A recorded spawn request that was (or will be) advertised to peers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpawnRequest {
    /// Index into the spawner's spawnable-scene list.
    pub scene_index: u32,
    /// Scene path that should be instantiated on remote peers.
    pub scene_path: String,
    /// Opaque spawn data (mirrors Godot's `spawn(data)` argument as a string).
    pub data: String,
    /// Local net id assigned to this spawn (monotonic counter).
    pub net_id: u32,
}

/// Decoded spawn packet payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpawnPacket {
    /// Scene path to instantiate.
    pub scene_path: String,
    /// Opaque spawn data.
    pub data: String,
    /// Net id of the spawned instance.
    pub net_id: u32,
    /// Scene index in the remote spawner's list.
    pub scene_index: u32,
}

/// Decoded property-sync packet payload.
#[derive(Debug, Clone, PartialEq)]
pub struct SyncPacket {
    /// Net id of the synchronized root (0 when unset).
    pub net_id: u32,
    /// Property name → string-encoded value pairs.
    pub properties: Vec<(String, String)>,
}

/// Encode a spawn notification for multiplayer transport.
pub fn encode_spawn_packet(req: &SpawnRequest) -> Vec<u8> {
    let path_bytes = req.scene_path.as_bytes();
    let data_bytes = req.data.as_bytes();
    let mut out = Vec::with_capacity(1 + 4 + 4 + 2 + path_bytes.len() + 2 + data_bytes.len());
    out.push(REPLICATION_KIND_SPAWN);
    out.extend_from_slice(&req.net_id.to_be_bytes());
    out.extend_from_slice(&req.scene_index.to_be_bytes());
    out.extend_from_slice(&(path_bytes.len() as u16).to_be_bytes());
    out.extend_from_slice(path_bytes);
    out.extend_from_slice(&(data_bytes.len() as u16).to_be_bytes());
    out.extend_from_slice(data_bytes);
    out
}

/// Decode a spawn notification packet, or `None` if the payload is not a spawn.
pub fn decode_spawn_packet(data: &[u8]) -> Option<SpawnPacket> {
    if data.first().copied() != Some(REPLICATION_KIND_SPAWN) || data.len() < 1 + 4 + 4 + 2 {
        return None;
    }
    let mut i = 1usize;
    let net_id = u32::from_be_bytes(data[i..i + 4].try_into().ok()?);
    i += 4;
    let scene_index = u32::from_be_bytes(data[i..i + 4].try_into().ok()?);
    i += 4;
    let path_len = u16::from_be_bytes(data[i..i + 2].try_into().ok()?) as usize;
    i += 2;
    if data.len() < i + path_len + 2 {
        return None;
    }
    let scene_path = String::from_utf8(data[i..i + path_len].to_vec()).ok()?;
    i += path_len;
    let data_len = u16::from_be_bytes(data[i..i + 2].try_into().ok()?) as usize;
    i += 2;
    if data.len() < i + data_len {
        return None;
    }
    let spawn_data = String::from_utf8(data[i..i + data_len].to_vec()).ok()?;
    Some(SpawnPacket {
        scene_path,
        data: spawn_data,
        net_id,
        scene_index,
    })
}

/// Encode a property-sync packet for multiplayer transport.
pub fn encode_sync_packet(net_id: u32, properties: &[(String, String)]) -> Vec<u8> {
    let mut out = Vec::new();
    out.push(REPLICATION_KIND_SYNC);
    out.extend_from_slice(&net_id.to_be_bytes());
    out.extend_from_slice(&(properties.len() as u16).to_be_bytes());
    for (name, value) in properties {
        let nb = name.as_bytes();
        let vb = value.as_bytes();
        out.extend_from_slice(&(nb.len() as u16).to_be_bytes());
        out.extend_from_slice(nb);
        out.extend_from_slice(&(vb.len() as u16).to_be_bytes());
        out.extend_from_slice(vb);
    }
    out
}

/// Decode a property-sync packet, or `None` if the payload is not a sync.
pub fn decode_sync_packet(data: &[u8]) -> Option<SyncPacket> {
    if data.first().copied() != Some(REPLICATION_KIND_SYNC) || data.len() < 1 + 4 + 2 {
        return None;
    }
    let mut i = 1usize;
    let net_id = u32::from_be_bytes(data[i..i + 4].try_into().ok()?);
    i += 4;
    let count = u16::from_be_bytes(data[i..i + 2].try_into().ok()?) as usize;
    i += 2;
    let mut properties = Vec::with_capacity(count);
    for _ in 0..count {
        if data.len() < i + 2 {
            return None;
        }
        let name_len = u16::from_be_bytes(data[i..i + 2].try_into().ok()?) as usize;
        i += 2;
        if data.len() < i + name_len + 2 {
            return None;
        }
        let name = String::from_utf8(data[i..i + name_len].to_vec()).ok()?;
        i += name_len;
        let value_len = u16::from_be_bytes(data[i..i + 2].try_into().ok()?) as usize;
        i += 2;
        if data.len() < i + value_len {
            return None;
        }
        let value = String::from_utf8(data[i..i + value_len].to_vec()).ok()?;
        i += value_len;
        properties.push((name, value));
    }
    Some(SyncPacket { net_id, properties })
}

/// Manages replication of node spawns across peers.
///
/// Bounded slice of Godot's `MultiplayerSpawner` (modules/multiplayer):
/// tracks spawnable scene paths, records `spawn()` requests, and can emit a
/// spawn packet over a [`NetworkPeer`] so remote peers observe the request.
#[derive(Debug, Clone)]
pub struct MultiplayerSpawner {
    /// Scene paths that are eligible for automatic replication.
    ///
    /// Kept as `tracked_paths` for backward compatibility with earlier stubs.
    pub tracked_paths: Vec<String>,
    /// Maximum number of spawns this spawner will replicate (`0` = unlimited).
    pub spawn_limit: u32,
    /// Node path under which spawned children are parented (Godot `spawn_path`).
    pub spawn_path: String,
    /// Monotonic net-id counter for spawn requests.
    next_net_id: u32,
    /// Recorded spawn requests (local authority history).
    spawn_requests: Vec<SpawnRequest>,
}

impl MultiplayerSpawner {
    /// Create a new spawner with no tracked paths.
    pub fn new(spawn_limit: u32) -> Self {
        Self {
            tracked_paths: Vec::new(),
            spawn_limit,
            spawn_path: String::new(),
            next_net_id: 1,
            spawn_requests: Vec::new(),
        }
    }

    /// Track an additional scene path (alias for [`add_spawnable_scene`]).
    pub fn add_tracked_path(&mut self, path: impl Into<String>) {
        self.add_spawnable_scene(path);
    }

    /// Register a scene path that may be spawned and replicated (Godot API).
    pub fn add_spawnable_scene(&mut self, path: impl Into<String>) {
        self.tracked_paths.push(path.into());
    }

    /// Number of registered spawnable scenes.
    pub fn get_spawnable_scene_count(&self) -> usize {
        self.tracked_paths.len()
    }

    /// Scene path at `index`, or empty if out of range.
    pub fn get_spawnable_scene(&self, index: usize) -> String {
        self.tracked_paths.get(index).cloned().unwrap_or_default()
    }

    /// Clear all spawnable scenes.
    pub fn clear_spawnable_scenes(&mut self) {
        self.tracked_paths.clear();
    }

    /// Godot `spawn_path` getter.
    pub fn get_spawn_path(&self) -> &str {
        &self.spawn_path
    }

    /// Godot `spawn_path` setter.
    pub fn set_spawn_path(&mut self, path: impl Into<String>) {
        self.spawn_path = path.into();
    }

    /// Godot `spawn_limit` getter.
    pub fn get_spawn_limit(&self) -> u32 {
        self.spawn_limit
    }

    /// Godot `spawn_limit` setter (`0` = unlimited).
    pub fn set_spawn_limit(&mut self, limit: u32) {
        self.spawn_limit = limit;
    }

    /// Recorded spawn requests since creation.
    pub fn spawn_requests(&self) -> &[SpawnRequest] {
        &self.spawn_requests
    }

    /// Number of successful local spawns so far.
    pub fn spawn_count(&self) -> usize {
        self.spawn_requests.len()
    }

    /// Record a spawn request without sending a network packet.
    ///
    /// Uses the first spawnable scene when any are registered; otherwise fails
    /// with [`NetworkError::Transport`]. Honors `spawn_limit` when non-zero.
    pub fn spawn(&mut self, data: impl Into<String>) -> Result<SpawnRequest> {
        if self.spawn_limit > 0 && self.spawn_requests.len() as u32 >= self.spawn_limit {
            return Err(NetworkError::Transport("spawn limit reached".into()));
        }
        if self.tracked_paths.is_empty() {
            return Err(NetworkError::Transport(
                "no spawnable scenes registered".into(),
            ));
        }
        let scene_index = 0u32;
        let scene_path = self.tracked_paths[0].clone();
        let net_id = self.next_net_id;
        self.next_net_id = self.next_net_id.saturating_add(1);
        let req = SpawnRequest {
            scene_index,
            scene_path,
            data: data.into(),
            net_id,
        };
        self.spawn_requests.push(req.clone());
        Ok(req)
    }

    /// Spawn and notify `to` over `peer` with a spawn replication packet.
    ///
    /// This is the multiplayer-facing entry used by integration tests and the
    /// authority path: track locally, then send so remotes observe the spawn.
    pub fn spawn_and_notify(
        &mut self,
        peer: &mut dyn NetworkPeer,
        to: PeerId,
        data: impl Into<String>,
    ) -> Result<SpawnRequest> {
        let req = self.spawn(data)?;
        let packet = Packet {
            sender: peer.get_unique_id(),
            channel: 0,
            data: encode_spawn_packet(&req),
            transfer_mode: TransferMode::Reliable,
        };
        peer.send_packet(to, packet)?;
        Ok(req)
    }

    /// Apply an inbound spawn packet on the client side (records observation).
    pub fn observe_spawn_packet(&mut self, packet: &Packet) -> Option<SpawnPacket> {
        let decoded = decode_spawn_packet(&packet.data)?;
        // Record as a remote observation so spawn_count/history stay consistent.
        self.spawn_requests.push(SpawnRequest {
            scene_index: decoded.scene_index,
            scene_path: decoded.scene_path.clone(),
            data: decoded.data.clone(),
            net_id: decoded.net_id,
        });
        Some(decoded)
    }
}

/// Replicates a small property set across peers each poll.
///
/// Bounded slice of Godot's `MultiplayerSynchronizer`: watches named properties
/// (e.g. `position`), encodes them into sync packets, and applies inbound
/// updates. Full SceneReplicationConfig / visibility is out of scope.
#[derive(Debug, Clone)]
pub struct MultiplayerSynchronizer {
    /// Property paths to synchronize.
    pub properties_to_sync: Vec<String>,
    /// Minimum interval between sync updates, in milliseconds.
    pub sync_interval_ms: u32,
    /// Root path of the synchronized node (Godot `root_path`).
    pub root_path: String,
    /// Net id of the synchronized instance.
    pub net_id: u32,
    /// Current property values (authority-side writes / remote applies).
    property_values: HashMap<String, String>,
    /// Last time (ms) a sync packet was emitted (`0` = never).
    last_sync_ms: u64,
    /// Per-peer visibility overrides (Godot per-peer visibility). A peer absent
    /// from the map falls back to `public_visibility`.
    visibility: HashMap<PeerId, bool>,
    /// Default visibility for peers without an explicit override.
    public_visibility: bool,
    /// Optional replication config describing spawn/sync properties.
    replication_config: SceneReplicationConfig,
}

impl MultiplayerSynchronizer {
    /// Create a new synchronizer.
    pub fn new(sync_interval_ms: u32) -> Self {
        Self {
            properties_to_sync: Vec::new(),
            sync_interval_ms,
            root_path: String::from(".."),
            net_id: 0,
            property_values: HashMap::new(),
            last_sync_ms: 0,
            visibility: HashMap::new(),
            public_visibility: true,
            replication_config: SceneReplicationConfig::new(),
        }
    }

    /// Sets whether this synchronizer is visible to `peer`
    /// (Godot `set_visibility_for`); overrides the public default.
    pub fn set_visibility_for(&mut self, peer: PeerId, visible: bool) {
        self.visibility.insert(peer, visible);
    }

    /// Clears the per-peer visibility override for `peer`, reverting to the
    /// public default.
    pub fn clear_visibility_for(&mut self, peer: PeerId) {
        self.visibility.remove(&peer);
    }

    /// Returns whether this synchronizer is visible to `peer`
    /// (Godot `is_visible_to`).
    pub fn is_visible_to(&self, peer: PeerId) -> bool {
        self.visibility
            .get(&peer)
            .copied()
            .unwrap_or(self.public_visibility)
    }

    /// Sets the default visibility for peers without an override
    /// (Godot `set_visibility_public`).
    pub fn set_visibility_public(&mut self, visible: bool) {
        self.public_visibility = visible;
    }

    /// Returns the default (public) visibility.
    pub fn is_visibility_public(&self) -> bool {
        self.public_visibility
    }

    /// Returns the scene replication config (Godot `replication_config`).
    pub fn replication_config(&self) -> &SceneReplicationConfig {
        &self.replication_config
    }

    /// Mutable access to the scene replication config.
    pub fn replication_config_mut(&mut self) -> &mut SceneReplicationConfig {
        &mut self.replication_config
    }

    /// Add a property path to the synchronization set.
    pub fn add_property(&mut self, path: impl Into<String>) {
        let path = path.into();
        if !self.properties_to_sync.contains(&path) {
            self.properties_to_sync.push(path);
        }
    }

    /// Godot `root_path` getter.
    pub fn get_root_path(&self) -> &str {
        &self.root_path
    }

    /// Godot `root_path` setter.
    pub fn set_root_path(&mut self, path: impl Into<String>) {
        self.root_path = path.into();
    }

    /// Godot replication interval in seconds (converted from ms).
    pub fn get_replication_interval(&self) -> f64 {
        self.sync_interval_ms as f64 / 1000.0
    }

    /// Set replication interval in seconds.
    pub fn set_replication_interval(&mut self, seconds: f64) {
        self.sync_interval_ms = (seconds * 1000.0).max(0.0) as u32;
    }

    /// Net id of the synchronized root.
    pub fn get_net_id(&self) -> u32 {
        self.net_id
    }

    /// Set net id of the synchronized root.
    pub fn set_net_id(&mut self, net_id: u32) {
        self.net_id = net_id;
    }

    /// Write a property value on the authority (string-encoded for transport).
    pub fn set_property_value(&mut self, name: &str, value: impl Into<String>) {
        if !self.properties_to_sync.iter().any(|p| p == name) {
            self.properties_to_sync.push(name.to_string());
        }
        self.property_values.insert(name.to_string(), value.into());
    }

    /// Read a property value, if set.
    pub fn get_property_value(&self, name: &str) -> Option<&str> {
        self.property_values.get(name).map(|s| s.as_str())
    }

    /// Snapshot of all currently held property values in registration order.
    pub fn property_snapshot(&self) -> Vec<(String, String)> {
        self.properties_to_sync
            .iter()
            .filter_map(|name| {
                self.property_values
                    .get(name)
                    .map(|v| (name.clone(), v.clone()))
            })
            .collect()
    }

    /// Emit a sync packet to `to` if the interval has elapsed (or force).
    ///
    /// `now_ms` is a free-running millisecond clock supplied by the caller so
    /// tests can control timing without OS sleep. Returns `true` if a packet
    /// was sent.
    pub fn poll_outbound(
        &mut self,
        peer: &mut dyn NetworkPeer,
        to: PeerId,
        now_ms: u64,
        force: bool,
    ) -> Result<bool> {
        let props = self.property_snapshot();
        if props.is_empty() {
            return Ok(false);
        }
        // Godot per-peer visibility: a peer this synchronizer is hidden from
        // receives no sync packets.
        if !self.is_visible_to(to) {
            return Ok(false);
        }
        if !force
            && self.last_sync_ms > 0
            && now_ms.saturating_sub(self.last_sync_ms) < self.sync_interval_ms as u64
        {
            return Ok(false);
        }
        let packet = Packet {
            sender: peer.get_unique_id(),
            channel: 0,
            data: encode_sync_packet(self.net_id, &props),
            transfer_mode: TransferMode::Unreliable,
        };
        peer.send_packet(to, packet)?;
        self.last_sync_ms = now_ms;
        Ok(true)
    }

    /// Apply an inbound sync packet, updating local property values.
    ///
    /// Returns the decoded packet when the payload is a sync frame.
    pub fn apply_sync_packet(&mut self, packet: &Packet) -> Option<SyncPacket> {
        let decoded = decode_sync_packet(&packet.data)?;
        for (name, value) in &decoded.properties {
            if !self.properties_to_sync.iter().any(|p| p == name) {
                self.properties_to_sync.push(name.clone());
            }
            self.property_values.insert(name.clone(), value.clone());
        }
        if decoded.net_id != 0 {
            self.net_id = decoded.net_id;
        }
        Some(decoded)
    }
}

// ===========================================================================
// SceneReplicationConfig (pat-u3l8c.80)
// ===========================================================================

/// Per-property replication mode, mirroring Godot's
/// `SceneReplicationConfig.ReplicationMode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ReplicationMode {
    /// Never replicated (`REPLICATION_MODE_NEVER`, 0).
    Never,
    /// Replicated on every sync tick (`REPLICATION_MODE_ALWAYS`, 1).
    #[default]
    Always,
    /// Replicated only when the value changes (`REPLICATION_MODE_ON_CHANGE`, 2).
    OnChange,
}

impl ReplicationMode {
    /// The Godot integer value for this mode.
    pub fn to_godot(self) -> i64 {
        match self {
            ReplicationMode::Never => 0,
            ReplicationMode::Always => 1,
            ReplicationMode::OnChange => 2,
        }
    }
}

/// A single property entry in a [`SceneReplicationConfig`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplicationProperty {
    /// The node-relative property path (e.g. `.:position`).
    pub path: String,
    /// Whether the property is part of the spawn state.
    pub spawn: bool,
    /// Whether the property is part of the periodic sync state.
    pub sync: bool,
    /// When the sync value is sent.
    pub mode: ReplicationMode,
}

/// Bounded slice of Godot's `SceneReplicationConfig`: an ordered set of
/// properties, each flagged for spawn and/or sync replication with a mode.
#[derive(Debug, Clone, Default)]
pub struct SceneReplicationConfig {
    properties: Vec<ReplicationProperty>,
}

impl SceneReplicationConfig {
    /// Creates an empty config.
    pub fn new() -> Self {
        Self::default()
    }

    fn index_of(&self, path: &str) -> Option<usize> {
        self.properties.iter().position(|p| p.path == path)
    }

    /// Adds a property (defaults: spawn + sync, `Always`). No-op if present.
    pub fn add_property(&mut self, path: impl Into<String>) {
        let path = path.into();
        if self.index_of(&path).is_none() {
            self.properties.push(ReplicationProperty {
                path,
                spawn: true,
                sync: true,
                mode: ReplicationMode::Always,
            });
        }
    }

    /// Removes a property. No-op if absent.
    pub fn remove_property(&mut self, path: &str) {
        if let Some(i) = self.index_of(path) {
            self.properties.remove(i);
        }
    }

    /// Whether `path` is present.
    pub fn has_property(&self, path: &str) -> bool {
        self.index_of(path).is_some()
    }

    /// The registration index of `path` (Godot `property_get_index`).
    pub fn property_get_index(&self, path: &str) -> Option<usize> {
        self.index_of(path)
    }

    /// All property paths in registration order.
    pub fn get_properties(&self) -> Vec<String> {
        self.properties.iter().map(|p| p.path.clone()).collect()
    }

    /// Sets whether `path` is part of the spawn state.
    pub fn property_set_spawn(&mut self, path: &str, enabled: bool) {
        if let Some(i) = self.index_of(path) {
            self.properties[i].spawn = enabled;
        }
    }

    /// Whether `path` is part of the spawn state (false if absent).
    pub fn property_get_spawn(&self, path: &str) -> bool {
        self.index_of(path)
            .map(|i| self.properties[i].spawn)
            .unwrap_or(false)
    }

    /// Sets whether `path` is part of the periodic sync state.
    pub fn property_set_sync(&mut self, path: &str, enabled: bool) {
        if let Some(i) = self.index_of(path) {
            self.properties[i].sync = enabled;
        }
    }

    /// Whether `path` is part of the sync state (false if absent).
    pub fn property_get_sync(&self, path: &str) -> bool {
        self.index_of(path)
            .map(|i| self.properties[i].sync)
            .unwrap_or(false)
    }

    /// Sets the replication mode for `path`.
    pub fn property_set_replication_mode(&mut self, path: &str, mode: ReplicationMode) {
        if let Some(i) = self.index_of(path) {
            self.properties[i].mode = mode;
        }
    }

    /// The replication mode for `path` (`Always` if absent).
    pub fn property_get_replication_mode(&self, path: &str) -> ReplicationMode {
        self.index_of(path)
            .map(|i| self.properties[i].mode)
            .unwrap_or(ReplicationMode::Always)
    }

    /// Property paths flagged for the spawn state, in order.
    pub fn spawn_properties(&self) -> Vec<String> {
        self.properties
            .iter()
            .filter(|p| p.spawn)
            .map(|p| p.path.clone())
            .collect()
    }

    /// Property paths flagged for the sync state (excluding `Never`), in order.
    pub fn sync_properties(&self) -> Vec<String> {
        self.properties
            .iter()
            .filter(|p| p.sync && p.mode != ReplicationMode::Never)
            .map(|p| p.path.clone())
            .collect()
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // pat-u3l8c.80: per-peer visibility + SceneReplicationConfig.
    #[test]
    fn synchronizer_visibility_defaults_and_overrides() {
        let mut sync = MultiplayerSynchronizer::new(0);
        assert!(sync.is_visible_to(PeerId(2)), "public by default");
        assert!(sync.is_visibility_public());

        sync.set_visibility_for(PeerId(2), false);
        assert!(!sync.is_visible_to(PeerId(2)), "hidden from peer 2");
        assert!(sync.is_visible_to(PeerId(3)), "still visible to peer 3");

        sync.clear_visibility_for(PeerId(2));
        assert!(sync.is_visible_to(PeerId(2)), "override cleared");

        sync.set_visibility_public(false);
        assert!(!sync.is_visible_to(PeerId(3)), "private hides un-overridden peers");
        sync.set_visibility_for(PeerId(3), true);
        assert!(sync.is_visible_to(PeerId(3)), "explicit override wins");
    }

    #[test]
    fn synchronizer_hidden_peer_gets_no_sync() {
        let mut sync = MultiplayerSynchronizer::new(0);
        sync.set_property_value("position", "10,20");
        let mut peer = MockNetworkPeer::new(PeerId::SERVER);

        // Visible → a forced poll sends a packet.
        assert!(sync.poll_outbound(&mut peer, PeerId(2), 1000, true).unwrap());

        // Hidden from that peer → no packet even when forced.
        sync.set_visibility_for(PeerId(2), false);
        assert!(!sync.poll_outbound(&mut peer, PeerId(2), 2000, true).unwrap());
    }

    #[test]
    fn scene_replication_config_spawn_and_sync_partitioning() {
        let mut cfg = SceneReplicationConfig::new();
        cfg.add_property("position");
        cfg.add_property("health");
        cfg.add_property("name");
        cfg.add_property("position"); // duplicate is a no-op

        assert!(cfg.has_property("position"));
        assert_eq!(cfg.property_get_index("health"), Some(1));
        assert_eq!(cfg.get_properties().len(), 3);
        assert!(cfg.property_get_spawn("position"));
        assert!(cfg.property_get_sync("position"));

        // `name` becomes spawn-only; `health` becomes sync-only.
        cfg.property_set_sync("name", false);
        cfg.property_set_spawn("health", false);
        assert_eq!(
            cfg.spawn_properties(),
            vec!["position".to_string(), "name".to_string()]
        );
        assert_eq!(
            cfg.sync_properties(),
            vec!["position".to_string(), "health".to_string()]
        );

        // A `Never`-mode property is excluded from the sync set.
        cfg.property_set_replication_mode("position", ReplicationMode::Never);
        assert_eq!(
            cfg.property_get_replication_mode("position"),
            ReplicationMode::Never
        );
        assert_eq!(cfg.sync_properties(), vec!["health".to_string()]);
        assert_eq!(ReplicationMode::OnChange.to_godot(), 2);

        cfg.remove_property("health");
        assert!(!cfg.has_property("health"));
    }

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
    fn binary_rpc_roundtrip_not_text_toy() {
        let data = encode_rpc_packet(
            0xABCD,
            "set_hp",
            &[Variant::Int(100), Variant::String("ok".into()), Variant::Bool(true)],
        );
        // Must be binary PRPC, not "rpc:…"
        assert_eq!(&data[0..4], RPC_MAGIC);
        assert!(!data.starts_with(b"rpc:"));
        let (path, method, args) = decode_rpc_packet(&data).expect("decode");
        assert_eq!(path, 0xABCD);
        assert_eq!(method, "set_hp");
        assert_eq!(args.len(), 3);
        assert_eq!(args[0], Variant::Int(100));
        assert_eq!(args[1], Variant::String("ok".into()));
        assert_eq!(args[2], Variant::Bool(true));
    }

    #[test]
    fn two_peer_rpc_and_spawn_sync_pipeline() {
        // Server authority tree A; remote peer B receives RPC, spawn, and sync.
        let (server_peer, client_peer) = MockNetwork::create_pair(PeerId(2));
        let mut server_api = MultiplayerAPI::new();
        let mut client_api = MultiplayerAPI::new();
        server_api.set_multiplayer_peer(Box::new(server_peer));
        client_api.set_multiplayer_peer(Box::new(client_peer));

        server_api.register_rpc(
            "apply_damage",
            RPCConfig {
                mode: RPCMode::Any,
                transfer_mode: TransferMode::Reliable,
                channel: 1,
            },
        );
        // Target authority = client so server.rpc() delivers to peer 2.
        server_api.set_authority(99, PeerId(2));
        server_api
            .rpc(
                99,
                "apply_damage",
                vec![Variant::Int(25), Variant::Float(1.5)],
            )
            .unwrap();

        let _ = client_api.poll();
        let rpcs = client_api.take_inbound_rpcs();
        assert_eq!(rpcs.len(), 1);
        assert_eq!(rpcs[0].node_path, 99);
        assert_eq!(rpcs[0].method_name, "apply_damage");
        assert_eq!(rpcs[0].args[0], Variant::Int(25));
        assert_eq!(rpcs[0].args[1], Variant::Float(1.5));
        assert!(!format!("{:?}", rpcs[0]).contains("rpc:apply_damage"));

        // Spawn replication: server spawner → client observation
        let mut server_spawner = MultiplayerSpawner::new(0);
        server_spawner.add_spawnable_scene("res://player.tscn");
        let mut client_spawner = MultiplayerSpawner::new(0);
        {
            let peer = server_api.peer_mut().unwrap();
            server_spawner
                .spawn_and_notify(peer, PeerId(2), "skin=blue")
                .unwrap();
        }
        let packets = client_api.poll();
        assert!(!packets.is_empty());
        let observed = client_spawner
            .observe_spawn_packet(&packets[0])
            .expect("spawn packet");
        assert_eq!(observed.scene_path, "res://player.tscn");
        assert_eq!(observed.data, "skin=blue");
        assert_eq!(client_spawner.spawn_count(), 1);

        // Property sync across peers
        let mut auth_sync = MultiplayerSynchronizer::new(0);
        auth_sync.net_id = 7;
        auth_sync.add_property("position");
        auth_sync.set_property_value("position", "10,20");
        let mut remote_sync = MultiplayerSynchronizer::new(0);
        remote_sync.add_property("position");
        {
            let peer = server_api.peer_mut().unwrap();
            assert!(auth_sync
                .poll_outbound(peer, PeerId(2), 1000, true)
                .unwrap());
        }
        let sync_packets = client_api.poll();
        assert!(!sync_packets.is_empty());
        let applied = remote_sync
            .apply_sync_packet(&sync_packets[0])
            .expect("sync");
        assert_eq!(applied.net_id, 7);
        assert_eq!(remote_sync.get_property_value("position"), Some("10,20"));
    }

    #[test]
    fn spawner_basics() {
        let mut spawner = MultiplayerSpawner::new(10);
        assert!(spawner.tracked_paths.is_empty());
        assert_eq!(spawner.spawn_limit, 10);

        spawner.add_tracked_path("res://player.tscn");
        assert_eq!(spawner.tracked_paths.len(), 1);
        assert_eq!(spawner.get_spawnable_scene_count(), 1);
        assert_eq!(spawner.get_spawnable_scene(0), "res://player.tscn");
    }

    #[test]
    fn spawner_spawn_tracks_request() {
        let mut spawner = MultiplayerSpawner::new(2);
        spawner.add_spawnable_scene("res://enemy.tscn");
        spawner.set_spawn_path(".");
        let req = spawner.spawn("wave=1").unwrap();
        assert_eq!(req.scene_path, "res://enemy.tscn");
        assert_eq!(req.data, "wave=1");
        assert_eq!(req.net_id, 1);
        assert_eq!(spawner.spawn_count(), 1);
        // Second spawn ok, third hits limit.
        spawner.spawn("wave=2").unwrap();
        assert!(spawner.spawn("wave=3").is_err());
    }

    #[test]
    fn spawner_notify_over_mock_peer() {
        let (mut server, mut client) = MockNetwork::create_pair(PeerId(2));
        let mut spawner = MultiplayerSpawner::new(0);
        spawner.add_spawnable_scene("res://player.tscn");
        let req = spawner
            .spawn_and_notify(&mut server, PeerId(2), "team=red")
            .unwrap();

        let packets = client.poll();
        assert_eq!(packets.len(), 1);
        let decoded = decode_spawn_packet(&packets[0].data).expect("spawn packet");
        assert_eq!(decoded.scene_path, "res://player.tscn");
        assert_eq!(decoded.data, "team=red");
        assert_eq!(decoded.net_id, req.net_id);

        let mut remote = MultiplayerSpawner::new(0);
        let observed = remote.observe_spawn_packet(&packets[0]).unwrap();
        assert_eq!(observed.scene_path, "res://player.tscn");
        assert_eq!(remote.spawn_count(), 1);
    }

    #[test]
    fn spawn_packet_round_trip_codec() {
        let req = SpawnRequest {
            scene_index: 0,
            scene_path: "res://a.tscn".into(),
            data: "x".into(),
            net_id: 7,
        };
        let bytes = encode_spawn_packet(&req);
        let back = decode_spawn_packet(&bytes).unwrap();
        assert_eq!(back.scene_path, req.scene_path);
        assert_eq!(back.data, req.data);
        assert_eq!(back.net_id, 7);
        assert!(decode_spawn_packet(b"nope").is_none());
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

    #[test]
    fn synchronizer_poll_sends_and_applies() {
        let (mut server, mut client) = MockNetwork::create_pair(PeerId(2));
        let mut auth = MultiplayerSynchronizer::new(0);
        auth.set_net_id(42);
        auth.set_property_value("position", "1.0,2.0");
        assert!(auth
            .poll_outbound(&mut server, PeerId(2), 100, true)
            .unwrap());

        let packets = client.poll();
        assert_eq!(packets.len(), 1);
        let mut remote = MultiplayerSynchronizer::new(0);
        let applied = remote.apply_sync_packet(&packets[0]).unwrap();
        assert_eq!(applied.net_id, 42);
        assert_eq!(remote.get_property_value("position"), Some("1.0,2.0"));
        assert_eq!(remote.get_net_id(), 42);
    }

    #[test]
    fn synchronizer_respects_interval() {
        let (mut server, mut client) = MockNetwork::create_pair(PeerId(2));
        let mut auth = MultiplayerSynchronizer::new(100);
        auth.set_property_value("position", "0,0");
        assert!(auth
            .poll_outbound(&mut server, PeerId(2), 10, false)
            .unwrap());
        // Too soon for second send.
        assert!(!auth
            .poll_outbound(&mut server, PeerId(2), 50, false)
            .unwrap());
        assert!(auth
            .poll_outbound(&mut server, PeerId(2), 120, false)
            .unwrap());
        assert_eq!(client.poll().len(), 2);
    }
}
