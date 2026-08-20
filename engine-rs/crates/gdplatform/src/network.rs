//! # Networking
//!
//! Multiplayer networking stubs: peer abstraction, RPC configuration,
//! spawner/synchronizer primitives, and an in-memory mock transport.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use gdvariant::Variant;
use thiserror::Error;

use crate::multiplayer_peer_state::{
    MultiplayerPeerSignal, MultiplayerPeerState, TARGET_PEER_BROADCAST,
};

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
    /// An RPC argument has no binary wire representation, so the call cannot be
    /// sent (pat-s3rrv.179).
    ///
    /// ⚠️ This exists so the failure is LOUD. The encoder previously skipped an
    /// unencodable argument while `argc` still counted it, producing a packet
    /// whose declared argument count did not match its payload — the receiver
    /// then read the following arguments from the wrong offsets.
    #[error("RPC argument {index} cannot be encoded: {source}")]
    UnencodableArgument {
        /// Zero-based position of the offending argument.
        index: usize,
        /// Why the codec refused it.
        source: gdvariant::binary::VariantEncodeError,
    },
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

    /// Roster changes this transport observed since the last call
    /// (pat-s3rrv.81).
    ///
    /// ⚠️ This was MISSING from the trait, and the consequence was total: a
    /// real handshake never produced `peer_connected` for a script. The
    /// transports record connections into their own `MultiplayerPeerState`,
    /// while `MultiplayerAPI::poll` drained only its OWN `peers` — two states
    /// that never met. Every green multiplayer test announced its peer with a
    /// Rust-side `add_peer`, which writes to the half that IS drained, so the
    /// gap survived a large test suite untouched.
    ///
    /// Defaulted to empty so a transport with no roster of its own (offline,
    /// mocks) is unaffected and no existing implementor has to change.
    fn drain_signals(&mut self) -> Vec<crate::multiplayer_peer_state::MultiplayerPeerSignal> {
        Vec::new()
    }
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
    // -- SceneMultiplayer surface (pat-0q2kx.94) --------------------------
    /// Connected peers + target-peer routing, shared with the transports.
    peers: MultiplayerPeerState,
    /// Script-visible events awaiting [`take_events`](Self::take_events).
    events: Vec<MultiplayerEvent>,
    /// Last observed transport status, so [`poll`](Self::poll) can turn a
    /// *transition* into `connected_to_server` / `connection_failed` /
    /// `server_disconnected`. Godot emits those once on the edge, not per frame.
    last_status: ConnectionStatus,
    /// Godot `SceneMultiplayer.refuse_new_connections`.
    refuse_new_connections: bool,
    /// Sender of the packet/RPC currently being delivered
    /// (`MultiplayerAPI.get_remote_sender_id`).
    remote_sender_id: PeerId,
    /// Whether peers must authenticate before counting as connected.
    auth_required: bool,
    /// Peers mid-authentication, with the seconds each has been waiting.
    pending_auth: HashMap<i32, f32>,
    /// Seconds before an unauthenticated peer is dropped (Godot
    /// `auth_timeout`, default 3s). `0` disables the timeout.
    auth_timeout: f32,
}

/// A script-visible multiplayer event — the signal set Godot's
/// `SceneMultiplayer` emits (pat-0q2kx.94).
///
/// Queued by [`MultiplayerAPI::poll`] and drained by the scene tree, which
/// re-emits each as the correspondingly-named signal. Modelled as data rather
/// than emitted directly because `gdplatform` has no scene tree to emit on —
/// the same split `MultiplayerPeerSignal` already uses one layer down.
#[derive(Debug, Clone, PartialEq)]
pub enum MultiplayerEvent {
    /// `peer_connected(id)` — a peer finished connecting (and authenticating,
    /// when an auth callback is set).
    PeerConnected(i32),
    /// `peer_disconnected(id)`.
    PeerDisconnected(i32),
    /// `connected_to_server()` — client-side, on reaching the server.
    ConnectedToServer,
    /// `connection_failed()` — client-side, when the attempt gave up.
    ConnectionFailed,
    /// `server_disconnected()` — client-side, when an established server drops.
    ServerDisconnected,
    /// `peer_packet(id, packet)` — raw bytes from [`send_bytes`](MultiplayerAPI::send_bytes).
    PeerPacket {
        /// Sending peer.
        id: i32,
        /// Payload as sent.
        packet: Vec<u8>,
    },
    /// `peer_authenticating(id)` — a peer is connected but not yet authorised.
    PeerAuthenticating(i32),
    /// `peer_authentication_failed(id)` — auth was rejected or timed out.
    PeerAuthenticationFailed(i32),
}

/// Godot's multiplayer packet command byte — the first byte of **every**
/// multiplayer packet (upstream/godot/modules/multiplayer/scene_multiplayer.h
/// `enum NetworkCommands`).
///
/// Patina previously framed RPCs with a bespoke `PRPC` magic and raw payloads
/// with `PRAW`, which meant no shared header, no room for the replication
/// commands, and no way to reject a packet of the wrong kind before parsing it
/// (pat-0q2kx.95). The discriminants are Godot's, so the values are wire
/// meaning, not an implementation detail — do not renumber them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum NetworkCommand {
    /// An RPC to a node (`NETWORK_COMMAND_REMOTE_CALL`).
    RemoteCall = 0,
    /// Sender announces `path -> id` so later calls can omit the path.
    SimplifyPath = 1,
    /// Receiver acknowledges a `SimplifyPath`.
    ConfirmPath = 2,
    /// An opaque `send_bytes` payload (`NETWORK_COMMAND_RAW`).
    Raw = 3,
    /// MultiplayerSpawner spawn notification.
    Spawn = 4,
    /// MultiplayerSpawner despawn notification.
    Despawn = 5,
    /// MultiplayerSynchronizer state update.
    Sync = 6,
    /// System command (auth / relay bookkeeping).
    Sys = 7,
}

impl NetworkCommand {
    /// The wire byte for this command.
    pub const fn to_byte(self) -> u8 {
        self as u8
    }

    /// Parses a wire byte, or `None` when it is not a known command — an
    /// unknown leading byte must not be guessed at.
    pub const fn from_byte(byte: u8) -> Option<NetworkCommand> {
        Some(match byte {
            0 => NetworkCommand::RemoteCall,
            1 => NetworkCommand::SimplifyPath,
            2 => NetworkCommand::ConfirmPath,
            3 => NetworkCommand::Raw,
            4 => NetworkCommand::Spawn,
            5 => NetworkCommand::Despawn,
            6 => NetworkCommand::Sync,
            7 => NetworkCommand::Sys,
            _ => return None,
        })
    }
}

/// Whether a decoder may materialize `Object` variants from the wire.
///
/// Godot gates this behind `allow_object_decoding` because decoding an object
/// from an untrusted peer is remote code execution
/// (core/io/marshalls.cpp). Patina's RPC path had no such gate because it could
/// not encode objects at all; now that it uses the real codec, the gate has to
/// exist (pat-0q2kx.95). Default is **deny**.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ObjectDecoding {
    /// Reject packets containing an encoded object.
    #[default]
    Deny,
    /// Allow objects — only for peers the host already trusts.
    Allow,
}

/// Godot's default `SceneMultiplayer.auth_timeout`, in seconds.
pub const DEFAULT_AUTH_TIMEOUT: f32 = 3.0;

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

impl MultiplayerAPI {
    /// Create a new, peer-less [`MultiplayerAPI`].
    pub fn new() -> Self {
        Self {
            peer: None,
            rpc_configs: HashMap::new(),
            authority_map: HashMap::new(),
            inbound_rpcs: Vec::new(),
            peers: MultiplayerPeerState::new(),
            events: Vec::new(),
            last_status: ConnectionStatus::Disconnected,
            refuse_new_connections: false,
            remote_sender_id: PeerId(0),
            auth_required: false,
            pending_auth: HashMap::new(),
            auth_timeout: DEFAULT_AUTH_TIMEOUT,
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

        let data = encode_rpc_packet(node_path, method, &args)?;

        let packet = Packet {
            sender: sender_id,
            channel: config.channel,
            data,
            transfer_mode: config.transfer_mode,
        };

        peer.send_packet(target_id, packet)
    }

    /// The registered config for `method`, if any.
    ///
    /// Lets a host confirm what a script's `@rpc` annotation actually
    /// configured, rather than inferring it from a successful send
    /// (pat-0q2kx.79).
    pub fn get_rpc_config(&self, method: &str) -> Option<&RPCConfig> {
        self.rpc_configs.get(method)
    }

    /// Initiate an RPC on `node_path` addressed to ONE peer (Godot
    /// `Node.rpc_id`).
    ///
    /// Identical to [`rpc`](Self::rpc) except the target is given rather than
    /// resolved from the node's authority, which is the whole point of the
    /// `_id` form: replying to a specific caller (pat-0q2kx.79).
    pub fn rpc_id(
        &mut self,
        node_path: u64,
        peer: i64,
        method: &str,
        args: Vec<Variant>,
    ) -> Result<()> {
        let config = self
            .rpc_configs
            .get(method)
            .ok_or_else(|| NetworkError::UnregisteredRPC(method.to_string()))?
            .clone();

        let peer_handle = self.peer.as_mut().ok_or(NetworkError::NoPeer)?;
        let sender_id = peer_handle.get_unique_id();
        let data = encode_rpc_packet(node_path, method, &args)?;

        let packet = Packet {
            sender: sender_id,
            channel: config.channel,
            data,
            transfer_mode: config.transfer_mode,
        };

        peer_handle.send_packet(PeerId(peer as u32), packet)
    }

    /// Poll the transport and decode inbound RPC / leave non-RPC packets to caller.
    ///
    /// Returns non-RPC packets (spawn/sync/etc.) for higher layers. Decoded
    /// RPCs are available via [`Self::take_inbound_rpcs`].
    pub fn poll(&mut self) -> Vec<Packet> {
        // Status transitions first: a client that just reached the server must
        // see `connected_to_server` before any packet that arrived with it.
        self.poll_status();

        let Some(peer) = self.peer.as_mut() else {
            return Vec::new();
        };
        // Taken while `peer` is in scope; applied to `self.peers` below, which
        // is a disjoint field so the two borrows do not overlap.
        let transport_signals = peer.drain_signals();
        let mut other = Vec::new();
        let mut raw = Vec::new();
        for packet in peer.poll() {
            if let Some(rpc) = decode_rpc_packet(&packet.data) {
                self.inbound_rpcs.push(DecodedRPC {
                    node_path: rpc.0,
                    method_name: rpc.1,
                    sender_id: packet.sender,
                    args: rpc.2,
                });
            } else if let Some(bytes) = decode_raw_packet(&packet.data) {
                raw.push((packet.sender.0 as i32, bytes));
            } else {
                other.push(packet);
            }
        }
        for (id, packet) in raw {
            self.remote_sender_id = PeerId(id.unsigned_abs());
            self.events
                .push(MultiplayerEvent::PeerPacket { id, packet });
        }

        // Pull the TRANSPORT's roster changes into the API's own peer state
        // first, so a real handshake reaches the same event path a Rust-side
        // `add_peer` does (pat-s3rrv.81). Without this the loop below drains a
        // state nothing outside the API ever writes to.
        for signal in transport_signals {
            match signal {
                MultiplayerPeerSignal::PeerConnected(id) => {
                    self.peers.add_peer(id);
                }
                MultiplayerPeerSignal::PeerDisconnected(id) => {
                    self.peers.disconnect_peer(id);
                }
            }
        }

        // Peer roster changes the transport recorded, promoted to API events.
        // An unauthenticated peer is held back as `peer_authenticating` — Godot
        // does not consider it connected until `complete_auth`.
        let mut refused: Vec<i32> = Vec::new();
        for signal in self.peers.drain_signals() {
            match signal {
                MultiplayerPeerSignal::PeerConnected(id) => {
                    if self.refuse_new_connections {
                        // Godot's `refuse_new_connections` drops the peer
                        // outright — no `peer_connected`, and no
                        // `peer_disconnected` either, because from the script's
                        // point of view it never joined. Storing the flag
                        // without acting on it here would make the property a
                        // lie (pat-0q2kx.94).
                        refused.push(id);
                        continue;
                    }
                    if self.auth_required {
                        self.pending_auth.insert(id, 0.0);
                        self.events.push(MultiplayerEvent::PeerAuthenticating(id));
                    } else {
                        self.events.push(MultiplayerEvent::PeerConnected(id));
                    }
                }
                MultiplayerPeerSignal::PeerDisconnected(id) => {
                    self.pending_auth.remove(&id);
                    self.events.push(MultiplayerEvent::PeerDisconnected(id));
                }
            }
        }
        // Evicting refused peers is deferred out of the loop above so it does
        // not mutate the roster mid-drain; the disconnect signals it queues are
        // discarded for the same reason no connect was emitted.
        if !refused.is_empty() {
            for id in refused {
                self.peers.remove_peer(id);
            }
            self.peers.drain_signals();
        }
        other
    }

    /// Turns a transport status *transition* into the matching client-side
    /// event. Godot emits these once on the edge, which is why the previous
    /// status is remembered rather than recomputed each frame.
    fn poll_status(&mut self) {
        let status = self
            .peer
            .as_ref()
            .map(|p| p.get_connection_status())
            .unwrap_or(ConnectionStatus::Disconnected);
        if status == self.last_status {
            return;
        }
        let previous = self.last_status;
        self.last_status = status;
        // The server never reports these — they describe a client's view of the
        // server it dialled.
        if self.peer.as_ref().map(|p| p.is_server()).unwrap_or(false) {
            return;
        }
        match (previous, status) {
            (_, ConnectionStatus::Connected) => {
                self.events.push(MultiplayerEvent::ConnectedToServer);
            }
            // Giving up mid-handshake is a failure; losing an established
            // connection is a disconnect. Godot distinguishes the two, and a
            // game reacts differently to each ("retry" vs "back to menu").
            (ConnectionStatus::Connecting, ConnectionStatus::Disconnected) => {
                self.events.push(MultiplayerEvent::ConnectionFailed);
            }
            (ConnectionStatus::Connected, ConnectionStatus::Disconnected) => {
                self.events.push(MultiplayerEvent::ServerDisconnected);
            }
            _ => {}
        }
    }

    // -- SceneMultiplayer surface (pat-0q2kx.94) --------------------------

    /// Drains the queued script-visible events. The scene tree calls this each
    /// frame and re-emits each as its Godot signal.
    pub fn take_events(&mut self) -> Vec<MultiplayerEvent> {
        std::mem::take(&mut self.events)
    }

    /// The connected peer ids (Godot `MultiplayerAPI.get_peers`).
    pub fn get_peers(&self) -> Vec<i32> {
        self.peers.get_peers()
    }

    /// This end's own id (`MultiplayerAPI.get_unique_id`), or `None` with no
    /// peer assigned. Godot answers `0` in that case; `None` is kept here so a
    /// caller can tell "no peer" from a genuine id, and the script binding maps
    /// it back to `0`.
    pub fn unique_id(&self) -> Option<PeerId> {
        self.peer.as_ref().map(|p| p.get_unique_id())
    }

    /// Mutable access to the peer roster, for a transport publishing its
    /// connects/disconnects into the API.
    pub fn peer_state_mut(&mut self) -> &mut MultiplayerPeerState {
        &mut self.peers
    }

    /// The sender of the packet or RPC currently being delivered
    /// (`MultiplayerAPI.get_remote_sender_id`); `0` when not inside one.
    pub fn get_remote_sender_id(&self) -> PeerId {
        self.remote_sender_id
    }

    /// Records the sender of the RPC about to be dispatched, so a handler can
    /// ask who called it.
    pub fn set_remote_sender_id(&mut self, id: PeerId) {
        self.remote_sender_id = id;
    }

    /// Godot `SceneMultiplayer.refuse_new_connections`.
    pub fn refuse_new_connections(&self) -> bool {
        self.refuse_new_connections
    }

    /// Sets whether further connections are refused.
    pub fn set_refuse_new_connections(&mut self, refuse: bool) {
        self.refuse_new_connections = refuse;
    }

    /// Sends raw bytes to `target` (Godot `SceneMultiplayer.send_bytes`), which
    /// arrive on the far side as a `peer_packet` signal.
    ///
    /// `target` follows Godot's peer addressing: `0` broadcasts, a positive id
    /// addresses one peer, and a negative id means "everyone except `-id`".
    pub fn send_bytes(
        &mut self,
        bytes: &[u8],
        target: i32,
        transfer_mode: TransferMode,
        channel: u8,
    ) -> Result<()> {
        let Some(peer) = self.peer.as_mut() else {
            return Err(NetworkError::NoPeer);
        };
        let data = encode_raw_packet(bytes);
        let from = peer.get_unique_id();
        // Broadcast and except-one both fan out over the roster; only a single
        // positive id is a direct send.
        let targets: Vec<i32> = if target > 0 {
            vec![target]
        } else {
            self.peers
                .get_peers()
                .into_iter()
                .filter(|p| target == TARGET_PEER_BROADCAST || *p != -target)
                .collect()
        };
        for to in targets {
            peer.send_packet(
                PeerId(to.unsigned_abs()),
                Packet {
                    sender: from,
                    channel,
                    data: data.clone(),
                    transfer_mode,
                },
            )?;
        }
        Ok(())
    }

    // -- authentication (Godot's SYS auth handshake) ----------------------

    /// Requires peers to authenticate before they count as connected. With this
    /// set, a connecting peer surfaces as `peer_authenticating` and only becomes
    /// `peer_connected` once [`complete_auth`](Self::complete_auth) accepts it.
    pub fn set_auth_required(&mut self, required: bool) {
        self.auth_required = required;
    }

    /// Whether an auth handshake gates new peers.
    pub fn is_auth_required(&self) -> bool {
        self.auth_required
    }

    /// Seconds an unauthenticated peer may wait before being dropped
    /// (`SceneMultiplayer.auth_timeout`). `0` disables the timeout.
    pub fn set_auth_timeout(&mut self, seconds: f32) {
        self.auth_timeout = seconds.max(0.0);
    }

    /// The current auth timeout in seconds.
    pub fn auth_timeout(&self) -> f32 {
        self.auth_timeout
    }

    /// Peers currently mid-authentication.
    pub fn pending_auth_peers(&self) -> Vec<i32> {
        let mut ids: Vec<i32> = self.pending_auth.keys().copied().collect();
        ids.sort_unstable();
        ids
    }

    /// Whether `id` is connected but not yet authorised.
    pub fn is_peer_authenticating(&self, id: i32) -> bool {
        self.pending_auth.contains_key(&id)
    }

    /// Completes the handshake for `id` (Godot `complete_auth`).
    ///
    /// Accepting promotes the peer to `peer_connected`; rejecting emits
    /// `peer_authentication_failed`. Returns `false` when the peer was not
    /// authenticating, so a double-complete cannot fire a second signal.
    pub fn complete_auth(&mut self, id: i32, accepted: bool) -> bool {
        if self.pending_auth.remove(&id).is_none() {
            return false;
        }
        self.events.push(if accepted {
            MultiplayerEvent::PeerConnected(id)
        } else {
            MultiplayerEvent::PeerAuthenticationFailed(id)
        });
        if !accepted {
            self.peers.remove_peer(id);
            // The removal queues a transport-level disconnect signal that would
            // otherwise surface as a spurious `peer_disconnected` for a peer the
            // script was never told had connected.
            self.peers.drain_signals();
        }
        true
    }

    /// Advances the auth timers by `delta` seconds, failing any peer that has
    /// waited longer than [`auth_timeout`](Self::auth_timeout).
    ///
    /// Driven from the frame loop alongside [`poll`](Self::poll); without it a
    /// peer that never answers the handshake would occupy a slot forever, which
    /// is exactly what Godot's timeout exists to prevent.
    pub fn poll_auth(&mut self, delta: f32) {
        if self.auth_timeout <= 0.0 || self.pending_auth.is_empty() {
            return;
        }
        let timeout = self.auth_timeout;
        let mut timed_out: Vec<i32> = Vec::new();
        for (id, waited) in self.pending_auth.iter_mut() {
            *waited += delta;
            if *waited >= timeout {
                timed_out.push(*id);
            }
        }
        timed_out.sort_unstable();
        for id in timed_out {
            self.complete_auth(id, false);
        }
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

    /// Shared access to the peer (pat-s3rrv.17).
    ///
    /// The authority check runs inside loops that already hold `&self`, where a
    /// `&mut` accessor cannot be used.
    pub fn peer(&self) -> Option<&(dyn NetworkPeer + 'static)> {
        self.peer.as_deref()
    }

    /// Mutable access to the peer for advanced integration (spawner/sync).
    pub fn peer_mut(&mut self) -> Option<&mut (dyn NetworkPeer + 'static)> {
        self.peer.as_deref_mut()
    }
}

// ---------------------------------------------------------------------------
// Binary RPC framing (Variant subset) — pat-p0s82.4
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Raw byte framing — Godot's NETWORK_COMMAND_RAW (pat-0q2kx.94)
// ---------------------------------------------------------------------------

/// Frames `bytes` as a raw `send_bytes` payload: `PRAW | bytes…`.
pub fn encode_raw_packet(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(1 + bytes.len());
    out.push(NetworkCommand::Raw.to_byte());
    out.extend_from_slice(bytes);
    out
}

/// Unwraps a raw payload, or `None` when `data` is not one.
///
/// An empty `send_bytes` still round-trips as `Some(vec![])` — the magic alone
/// is a valid frame, and treating it as "not raw" would silently reclassify it
/// as an unknown packet.
pub fn decode_raw_packet(data: &[u8]) -> Option<Vec<u8>> {
    if data.first().copied().and_then(NetworkCommand::from_byte) != Some(NetworkCommand::Raw) {
        return None;
    }
    Some(data[1..].to_vec())
}

// ---------------------------------------------------------------------------
// OfflineMultiplayerPeer (pat-0q2kx.94)
// ---------------------------------------------------------------------------

/// Godot's `OfflineMultiplayerPeer` (scene/main/multiplayer_peer.cpp): the
/// always-connected loopback peer a single-player game runs with.
///
/// It reports `unique_id == 1` and `is_server() == true` so the *same* scripts
/// that branch on `multiplayer.is_server()` work unchanged with no sockets —
/// which is the entire point of the class, and why `is_server()` must be true
/// rather than "no peer, so false".
///
/// Packets sent to it loop straight back, so a `send_bytes` in single-player
/// still produces a `peer_packet` rather than vanishing.
#[derive(Debug, Default)]
pub struct OfflineMultiplayerPeer {
    loopback: Vec<Packet>,
    closed: bool,
}

impl OfflineMultiplayerPeer {
    /// Creates the offline peer.
    pub fn new() -> Self {
        Self::default()
    }
}

impl NetworkPeer for OfflineMultiplayerPeer {
    fn send_packet(&mut self, _to: PeerId, packet: Packet) -> Result<()> {
        if self.closed {
            return Err(NetworkError::NoPeer);
        }
        // Loop back with ourselves as the sender, as a local peer must.
        self.loopback.push(Packet {
            sender: PeerId::SERVER,
            ..packet
        });
        Ok(())
    }

    fn poll(&mut self) -> Vec<Packet> {
        std::mem::take(&mut self.loopback)
    }

    fn get_unique_id(&self) -> PeerId {
        PeerId::SERVER
    }

    fn is_server(&self) -> bool {
        true
    }

    fn get_connection_status(&self) -> ConnectionStatus {
        if self.closed {
            ConnectionStatus::Disconnected
        } else {
            // Never "connecting": an offline peer is usable the instant it
            // exists, which is what lets `_ready` branch on it.
            ConnectionStatus::Connected
        }
    }

    fn close(&mut self) {
        self.closed = true;
        self.loopback.clear();
    }
}

/// How a `RemoteCall` addresses its target node.
///
/// Godot negotiates a small id for each node path so repeat calls stop paying
/// for the path (`scene_cache_interface.cpp`). The mode byte is what lets one
/// decoder read both forms.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum RpcAddressing {
    /// Full 8-byte hashed path — used until the peer has confirmed an id.
    FullPath = 0,
    /// 2-byte negotiated path id.
    CachedId = 1,
}

impl RpcAddressing {
    /// The wire byte for this mode.
    pub const fn to_byte(self) -> u8 {
        self as u8
    }

    /// Parses a wire byte, or `None` for an unknown mode.
    pub const fn from_byte(byte: u8) -> Option<RpcAddressing> {
        Some(match byte {
            0 => RpcAddressing::FullPath,
            1 => RpcAddressing::CachedId,
            _ => return None,
        })
    }
}

/// Per-peer `path <-> id` table backing `SIMPLIFY_PATH` / `CONFIRM_PATH`
/// (pat-0q2kx.95).
///
/// Godot's sender announces `path -> id` once, the receiver confirms, and every
/// later call carries the id instead of the path
/// (`scene_cache_interface.cpp`). One cache instance belongs to one peer
/// relationship: ids are only meaningful between the two endpoints that
/// negotiated them, and sharing a cache across peers would let peer B's id
/// resolve to peer A's node.
///
/// ⚠️ The sender only switches to the short form **after the receiver
/// confirms**. Switching on send would address the node by an id the receiver
/// has not learned yet, and that call would be dropped rather than dispatched.
#[derive(Debug, Clone, Default)]
pub struct RpcPathCache {
    next_id: u16,
    /// Sender side: hashed path -> id we assigned.
    by_path: HashMap<u64, u16>,
    /// Receiver side: id -> hashed path we were told it means.
    by_id: HashMap<u16, u64>,
    /// Ids the receiver has acknowledged; only these may be used to address.
    confirmed: HashSet<u16>,
}

impl RpcPathCache {
    /// A cache with no negotiated ids.
    pub fn new() -> Self {
        Self::default()
    }

    /// The id already assigned to `node_path`, if any.
    pub fn id_for(&self, node_path: u64) -> Option<u16> {
        self.by_path.get(&node_path).copied()
    }

    /// The path an id refers to, once learned from a `SimplifyPath`.
    pub fn path_for(&self, id: u16) -> Option<u64> {
        self.by_id.get(&id).copied()
    }

    /// Whether `node_path` may be addressed by its short id yet.
    pub fn is_confirmed(&self, node_path: u64) -> bool {
        self.id_for(node_path)
            .is_some_and(|id| self.confirmed.contains(&id))
    }

    /// Assigns an id to `node_path`, or returns the existing one.
    fn assign(&mut self, node_path: u64) -> u16 {
        if let Some(id) = self.by_path.get(&node_path) {
            return *id;
        }
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1);
        self.by_path.insert(node_path, id);
        id
    }

    /// Receiver side: records the mapping a `SimplifyPath` announced.
    pub fn learn(&mut self, id: u16, node_path: u64) {
        self.by_id.insert(id, node_path);
    }

    /// Sender side: records that the peer confirmed `id`.
    pub fn confirm(&mut self, id: u16) {
        self.confirmed.insert(id);
    }
}

/// `SIMPLIFY_PATH`: announces that `id` will mean `node_path` from now on.
pub fn encode_simplify_path(id: u16, node_path: u64) -> Vec<u8> {
    let mut out = Vec::with_capacity(1 + 2 + 8);
    out.push(NetworkCommand::SimplifyPath.to_byte());
    out.extend_from_slice(&id.to_be_bytes());
    out.extend_from_slice(&node_path.to_be_bytes());
    out
}

/// Decodes a `SIMPLIFY_PATH` into `(id, node_path)`.
pub fn decode_simplify_path(data: &[u8]) -> Option<(u16, u64)> {
    if data.len() < 1 + 2 + 8 {
        return None;
    }
    if NetworkCommand::from_byte(data[0])? != NetworkCommand::SimplifyPath {
        return None;
    }
    let id = u16::from_be_bytes(data[1..3].try_into().ok()?);
    let path = u64::from_be_bytes(data[3..11].try_into().ok()?);
    Some((id, path))
}

/// `CONFIRM_PATH`: the receiver acknowledges an announced id.
pub fn encode_confirm_path(id: u16) -> Vec<u8> {
    let mut out = Vec::with_capacity(3);
    out.push(NetworkCommand::ConfirmPath.to_byte());
    out.extend_from_slice(&id.to_be_bytes());
    out
}

/// Decodes a `CONFIRM_PATH` into the id it acknowledges.
pub fn decode_confirm_path(data: &[u8]) -> Option<u16> {
    if data.len() < 3 {
        return None;
    }
    if NetworkCommand::from_byte(data[0])? != NetworkCommand::ConfirmPath {
        return None;
    }
    Some(u16::from_be_bytes(data[1..3].try_into().ok()?))
}

/// Encodes an RPC, using the short path id when the peer has confirmed one.
///
/// Returns `(simplify, call)`: `simplify` is `Some` exactly once per path — the
/// `SIMPLIFY_PATH` that must be sent before (or with) the call so the receiver
/// can learn the id. Send it first; the call itself still carries the full path
/// until a `CONFIRM_PATH` comes back, so nothing is dropped while the handshake
/// is in flight.
pub fn encode_rpc_call(
    cache: &mut RpcPathCache,
    node_path: u64,
    method: &str,
    args: &[Variant],
) -> Result<(Option<Vec<u8>>, Vec<u8>)> {
    let known = cache.id_for(node_path).is_some();
    let id = cache.assign(node_path);
    let simplify = if known {
        None
    } else {
        Some(encode_simplify_path(id, node_path))
    };

    let call = if cache.confirmed.contains(&id) {
        encode_rpc_packet_with_id(id, method, args)?
    } else {
        encode_rpc_packet(node_path, method, args)?
    };
    Ok((simplify, call))
}

/// A `RemoteCall` addressed by its negotiated id rather than the full path.
pub fn encode_rpc_packet_with_id(id: u16, method: &str, args: &[Variant]) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    out.push(NetworkCommand::RemoteCall.to_byte());
    out.push(RpcAddressing::CachedId.to_byte());
    out.extend_from_slice(&id.to_be_bytes());
    encode_rpc_tail(&mut out, method, args)?;
    Ok(out)
}

/// Shared `method | argc | args…` tail of both `RemoteCall` forms.
fn encode_rpc_tail(out: &mut Vec<u8>, method: &str, args: &[Variant]) -> Result<()> {
    let mb = method.as_bytes();
    out.extend_from_slice(&(mb.len() as u16).to_be_bytes());
    out.extend_from_slice(mb);
    out.extend_from_slice(&(args.len() as u16).to_be_bytes());
    for (index, a) in args.iter().enumerate() {
        // ⚠️ Propagate, never skip. This used to be
        // `if let Ok(bytes) = encode_variant(a) { … }`, which dropped an
        // unencodable argument from the payload while the `argc` written above
        // still counted it. The receiver then decoded the REMAINING arguments
        // from the wrong offsets — a wrong-value dispatch, not a clean failure.
        //
        // Godot fails the call instead
        // (`MultiplayerAPI::decode_and_decompress_variants`), and so does this.
        let bytes = gdvariant::binary::encode_variant(a)
            .map_err(|source| NetworkError::UnencodableArgument { index, source })?;
        out.extend_from_slice(&bytes);
    }
    Ok(())
}

/// Encode an RPC into the binary wire format:
/// `cmd | mode:u8 | node_path:u64 | method_len:u16 | method | argc:u16 | args…`
pub fn encode_rpc_packet(node_path: u64, method: &str, args: &[Variant]) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    out.push(NetworkCommand::RemoteCall.to_byte());
    out.push(RpcAddressing::FullPath.to_byte());
    out.extend_from_slice(&node_path.to_be_bytes());
    encode_rpc_tail(&mut out, method, args)?;
    Ok(out)
}

/// Decode a binary RPC packet. Returns `(node_path, method, args)`.
///
/// Returns `None` for any packet whose command byte is not
/// [`NetworkCommand::RemoteCall`], so a RAW / SPAWN / SYNC packet can never
/// enter RPC dispatch (pat-0q2kx.95).
pub fn decode_rpc_packet(data: &[u8]) -> Option<(u64, String, Vec<Variant>)> {
    decode_rpc_packet_with(data, ObjectDecoding::Deny)
}

/// [`decode_rpc_packet`] with an explicit object-decoding policy.
///
/// With [`ObjectDecoding::Deny`] (the default) a packet carrying an encoded
/// `Object` is rejected outright rather than materialized, mirroring Godot's
/// `allow_object_decoding` guard.
pub fn decode_rpc_packet_with(
    data: &[u8],
    objects: ObjectDecoding,
) -> Option<(u64, String, Vec<Variant>)> {
    decode_rpc_packet_inner(data, None, objects)
}

/// [`decode_rpc_packet_with`] that can also resolve id-addressed calls.
///
/// A `CachedId` packet is meaningless without the cache that negotiated the id,
/// so the cache-less decoders reject it rather than inventing a path. Pass the
/// same peer's cache here to accept both forms.
pub fn decode_rpc_packet_cached(
    data: &[u8],
    cache: &RpcPathCache,
    objects: ObjectDecoding,
) -> Option<(u64, String, Vec<Variant>)> {
    decode_rpc_packet_inner(data, Some(cache), objects)
}

fn decode_rpc_packet_inner(
    data: &[u8],
    cache: Option<&RpcPathCache>,
    objects: ObjectDecoding,
) -> Option<(u64, String, Vec<Variant>)> {
    // 1 command + 1 mode + 2 id (the shorter address) + 2 method len + 2 argc
    if data.len() < 1 + 1 + 2 + 2 + 2 {
        return None;
    }
    if NetworkCommand::from_byte(data[0])? != NetworkCommand::RemoteCall {
        return None;
    }
    let mut i = 1;
    let mode = RpcAddressing::from_byte(data[i])?;
    i += 1;
    let node_path = match mode {
        RpcAddressing::FullPath => {
            if i + 8 > data.len() {
                return None;
            }
            let p = u64::from_be_bytes(data[i..i + 8].try_into().ok()?);
            i += 8;
            p
        }
        RpcAddressing::CachedId => {
            let id = u16::from_be_bytes(data[i..i + 2].try_into().ok()?);
            i += 2;
            // An id we never learned resolves to nothing. Dropping the call is
            // the only safe answer — guessing a path would dispatch a remote
            // call at whatever node happened to collide.
            cache?.path_for(id)?
        }
    };
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
        let (v, n) = gdvariant::binary::decode_variant(data.get(i..)?).ok()?;
        // `ObjectId` is this Variant's object reference — decoding one from an
        // untrusted peer is what Godot's `allow_object_decoding` guards.
        if objects == ObjectDecoding::Deny && matches!(v, Variant::ObjectId(_)) {
            return None;
        }
        args.push(v);
        i += n;
    }
    Some((node_path, method, args))
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
    /// When set, only packets addressed to this id reach `remote_inbox`;
    /// everything else is dropped. `None` (the default) delivers regardless of
    /// address. See [`set_addressed_delivery`](Self::set_addressed_delivery).
    deliver_only_to: Option<PeerId>,
}

impl MockNetworkPeer {
    /// Create a standalone mock peer (no paired remote).
    pub fn new(id: PeerId) -> Self {
        Self {
            id,
            status: ConnectionStatus::Connected,
            inbox: Arc::new(Mutex::new(Vec::new())),
            remote_inbox: Arc::new(Mutex::new(Vec::new())),
            deliver_only_to: None,
        }
    }

    /// Make this mock honour the `to` address instead of forwarding every
    /// packet to its single paired remote (pat-0q2kx.94).
    ///
    /// The pair created by [`MockNetwork::create_pair`] has exactly one remote
    /// inbox, so by default `send_packet` ignores `to` — which is fine for
    /// round-trip tests but makes the double unable to express *addressing*.
    /// A test that asserts "peer N was skipped" against the default double
    /// always sees the packet arrive, because it is delivered to the only
    /// remote there is, whoever it was addressed to.
    ///
    /// Opt in per test rather than changing `create_pair`: every existing
    /// caller relies on unconditional delivery, including paths that address
    /// `PeerId::SERVER` while paired with a client
    /// (`MultiplayerAPI::rpc` targets the node authority, defaulting to the
    /// server), so making routing the default would drop those packets.
    pub fn set_addressed_delivery(&mut self, to: Option<PeerId>) {
        self.deliver_only_to = to;
    }

    /// Drives the mock's connection status (pat-0q2kx.94).
    ///
    /// `connected_to_server` / `connection_failed` / `server_disconnected` are
    /// emitted on a status **transition**, so testing them needs a peer that can
    /// start `Connecting` and change — a double pinned at `Connected` can only
    /// ever prove the steady state.
    pub fn set_status(&mut self, status: ConnectionStatus) {
        self.status = status;
    }
}

impl NetworkPeer for MockNetworkPeer {
    fn send_packet(&mut self, to: PeerId, packet: Packet) -> Result<()> {
        // Dropping an unaddressed packet is still `Ok`: a real transport does
        // not error when a datagram has no route, and the caller
        // (`send_bytes`, `rpc`) fans out over a roster that may name peers this
        // pair never had.
        if self.deliver_only_to.is_some_and(|only| only != to) {
            return Ok(());
        }
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
            deliver_only_to: None,
        };

        let client = MockNetworkPeer {
            id: client_id,
            status: ConnectionStatus::Connected,
            inbox: Arc::clone(&client_inbox),
            remote_inbox: Arc::clone(&server_inbox),
            deliver_only_to: None,
        };

        (server, client)
    }
}

// ---------------------------------------------------------------------------
// MultiplayerSpawner / MultiplayerSynchronizer (pat-ikq04)
// ---------------------------------------------------------------------------

/// Command byte for a spawn replication packet — Godot's
/// `NETWORK_COMMAND_SPAWN`.
///
/// These used to be `0x01`/`0x02`, a private numbering that predated the shared
/// header. Once every packet leads with a [`NetworkCommand`] those values
/// **collide** with `SIMPLIFY_PATH` (1) and `CONFIRM_PATH` (2): nothing
/// misroutes today, because the RPC and RAW decoders only accept their own
/// bytes, but the moment the path cache lands a spawn packet and a
/// simplify-path packet would be indistinguishable. Aliasing them onto the real
/// commands keeps one namespace (pat-0q2kx.95).
pub const REPLICATION_KIND_SPAWN: u8 = NetworkCommand::Spawn.to_byte();
/// Command byte for a property-sync replication packet — Godot's
/// `NETWORK_COMMAND_SYNC`. See [`REPLICATION_KIND_SPAWN`].
pub const REPLICATION_KIND_SYNC: u8 = NetworkCommand::Sync.to_byte();

/// A recorded spawn request that was (or will be) advertised to peers.
///
/// ⚠️ No longer `Eq`: `properties` carries `Variant`s, which are only
/// `PartialEq` (floats). Nothing keyed a map or set on this type.
#[derive(Debug, Clone, PartialEq)]
pub struct SpawnRequest {
    /// Index into the spawner's spawnable-scene list.
    pub scene_index: u32,
    /// Scene path that should be instantiated on remote peers.
    pub scene_path: String,
    /// Opaque spawn data (mirrors Godot's `spawn(data)` argument as a string).
    pub data: String,
    /// Local net id assigned to this spawn (monotonic counter).
    pub net_id: u32,
    /// The spawn-flagged property values captured at send time (pat-s3rrv.17).
    ///
    /// ⚠️ Gathered by the SCENE layer, not here: this struct has no access to
    /// property values, which live on the nodes. Godot does the same, building
    /// the spawn state in `SceneReplicationInterface::_make_spawn_packet`
    /// rather than in the spawner. Empty is the correct value for a spawner
    /// whose config flags nothing for spawn.
    pub properties: Vec<(String, Variant)>,
}

/// Decoded spawn packet payload.
///
/// ⚠️ Not `Eq` — see [`SpawnRequest`].
#[derive(Debug, Clone, PartialEq)]
pub struct SpawnPacket {
    /// Scene path to instantiate.
    pub scene_path: String,
    /// Opaque spawn data.
    pub data: String,
    /// Net id of the spawned instance.
    pub net_id: u32,
    /// Scene index in the remote spawner's list.
    pub scene_index: u32,
    /// Spawn-state property values to apply to the new instance before it is
    /// first seen (pat-s3rrv.17).
    pub properties: Vec<(String, Variant)>,
}

/// Decoded property-sync packet payload.
#[derive(Debug, Clone, PartialEq)]
pub struct SyncPacket {
    /// Net id of the synchronized root (0 when unset).
    pub net_id: u32,
    /// Property name → replicated [`Variant`] value (pat-s3rrv.17).
    ///
    /// These are real Variants, so a `Vector3`, `Color` or `bool` survives the
    /// round trip with its type intact. They were `String` before, which only
    /// `Vector2` had a spelling for.
    pub properties: Vec<(String, Variant)>,
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
    // The spawn state, in exactly the framing `encode_sync_packet` uses
    // (pat-s3rrv.17): `u16 count`, then `u16 name_len | name | u16 value_len |
    // binary Variant` per entry.
    //
    // ⚠️ Entries are built first and counted after, for the same reason as the
    // sync packet: a value that fails to encode is skipped, so writing
    // `properties.len()` up front would claim more entries than the body
    // carries and desync the reader for the rest of the packet.
    let mut body = Vec::new();
    let mut written: u16 = 0;
    for (name, value) in &req.properties {
        let Ok(vb) = gdvariant::binary::encode_variant(value) else {
            continue;
        };
        let nb = name.as_bytes();
        body.extend_from_slice(&(nb.len() as u16).to_be_bytes());
        body.extend_from_slice(nb);
        body.extend_from_slice(&(vb.len() as u16).to_be_bytes());
        body.extend_from_slice(&vb);
        written = written.saturating_add(1);
    }
    out.extend_from_slice(&written.to_be_bytes());
    out.extend_from_slice(&body);
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
    i += data_len;

    // The spawn state. ⚠️ OPTIONAL on read: a packet that ends here is a spawn
    // with no spawn-flagged properties, which is both the pre-pat-s3rrv.17 wire
    // shape and the ordinary case for a spawner that flags none. Treating a
    // missing section as a decode FAILURE would drop those spawns entirely.
    let mut properties = Vec::new();
    if data.len() >= i + 2 {
        let count = u16::from_be_bytes(data[i..i + 2].try_into().ok()?) as usize;
        i += 2;
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
            // Same shape as `decode_sync_packet`: the length prefix is
            // authoritative for framing, so `i` advances by it regardless of
            // what `decode_variant` reports consuming, and one malformed value
            // cannot desync the entries after it.
            let (value, _consumed) =
                gdvariant::binary::decode_variant(&data[i..i + value_len]).ok()?;
            i += value_len;
            properties.push((name, value));
        }
    }

    Some(SpawnPacket {
        scene_path,
        data: spawn_data,
        net_id,
        scene_index,
        properties,
    })
}

/// Leading kind byte for a despawn notification (pat-s3rrv.17).
///
/// `NetworkCommand::Despawn` has existed since the transport was written and
/// nothing ever produced or consumed it, so a host that removed a spawned node
/// left the client's copy on screen forever.
pub const REPLICATION_KIND_DESPAWN: u8 = NetworkCommand::Despawn.to_byte();

/// Encode a despawn notification for the spawn identified by `net_id`.
///
/// Only the id travels: the receiver already knows which node it instantiated
/// for that id, and re-sending the scene path would invite the two peers to
/// disagree about which node to remove.
pub fn encode_despawn_packet(net_id: u32) -> Vec<u8> {
    let mut out = Vec::with_capacity(1 + 4);
    out.push(REPLICATION_KIND_DESPAWN);
    out.extend_from_slice(&net_id.to_be_bytes());
    out
}

/// Decode a despawn notification, or `None` if the payload is not one.
///
/// ⚠️ The length check is `!=`, not `<`. A spawn packet also begins with a
/// 4-byte `net_id`, so a `<` check would let a longer packet whose kind byte
/// happened to match decode as a despawn and silently delete a live node.
pub fn decode_despawn_packet(data: &[u8]) -> Option<u32> {
    if data.first().copied() != Some(REPLICATION_KIND_DESPAWN) || data.len() != 1 + 4 {
        return None;
    }
    Some(u32::from_be_bytes(data[1..5].try_into().ok()?))
}

/// Encode a property-sync packet for multiplayer transport.
pub fn encode_sync_packet(net_id: u32, properties: &[(String, Variant)]) -> Vec<u8> {
    // Values travel as **binary Variants**, as Godot's
    // `SceneReplicationInterface` sends them (pat-s3rrv.17). They used to be
    // stringified, which silently flattened the type: only `Vector2` had a
    // hand-rolled `"x,y"` spelling, and everything else — `Vector3`, `Color`,
    // `bool`, `int` — arrived on the far peer as a `Variant::String` that no
    // longer matched the property it was written to.
    //
    // The length-prefixed framing is unchanged; only the bytes between the
    // prefixes are now a Variant.
    //
    // ⚠️ Entries are built first and counted after. A value that fails to encode
    // is skipped, so writing `properties.len()` up front would claim more
    // entries than the body carries and desync `decode_sync_packet` for every
    // remaining property in the packet.
    let mut body = Vec::new();
    let mut written: u16 = 0;
    for (name, value) in properties {
        let Ok(vb) = gdvariant::binary::encode_variant(value) else {
            continue;
        };
        let nb = name.as_bytes();
        body.extend_from_slice(&(nb.len() as u16).to_be_bytes());
        body.extend_from_slice(nb);
        body.extend_from_slice(&(vb.len() as u16).to_be_bytes());
        body.extend_from_slice(&vb);
        written = written.saturating_add(1);
    }

    let mut out = Vec::with_capacity(1 + 4 + 2 + body.len());
    out.push(REPLICATION_KIND_SYNC);
    out.extend_from_slice(&net_id.to_be_bytes());
    out.extend_from_slice(&written.to_be_bytes());
    out.extend_from_slice(&body);
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
        // Binary Variant, not UTF-8 (pat-s3rrv.17). `decode_variant` reports how
        // many bytes it consumed; the length prefix is authoritative for framing,
        // so `i` advances by that regardless and one malformed value cannot
        // desync the properties after it.
        let (value, _consumed) = gdvariant::binary::decode_variant(&data[i..i + value_len]).ok()?;
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
        self.spawn_with_properties(data, Vec::new())
    }

    /// [`spawn`](Self::spawn) carrying the spawn-state property values.
    ///
    /// ⚠️ The values arrive from the caller because this struct cannot reach
    /// them: property values live on scene nodes, and this crate has no scene.
    /// Godot splits it the same way — `MultiplayerSpawner` allocates the id and
    /// `SceneReplicationInterface::_make_spawn_packet` gathers the state.
    pub fn spawn_with_properties(
        &mut self,
        data: impl Into<String>,
        properties: Vec<(String, Variant)>,
    ) -> Result<SpawnRequest> {
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
            properties,
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
            properties: decoded.properties.clone(),
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
    /// Property paths to synchronize every tick
    /// (`SceneReplicationConfig` REPLICATION_MODE_ALWAYS).
    pub properties_to_sync: Vec<String>,
    /// Property paths replicated ONLY when their value changes
    /// (REPLICATION_MODE_ON_CHANGE — Godot's `watch_props`).
    ///
    /// 🔑 Watch and sync are mutually exclusive upstream: both are views over the
    /// single `mode` field (`scene_replication_config.cpp`, `_update()` routes
    /// ALWAYS -> sync_props and ON_CHANGE -> watch_props). A path must never be
    /// in both lists.
    pub properties_to_watch: Vec<String>,
    /// Minimum interval between sync updates, in milliseconds.
    pub sync_interval_ms: u32,
    /// Root path of the synchronized node (Godot `root_path`).
    pub root_path: String,
    /// Net id of the synchronized instance.
    pub net_id: u32,
    /// Current property values (authority-side writes / remote applies).
    property_values: HashMap<String, Variant>,
    /// Last time (ms) a sync packet was emitted (`0` = never).
    last_sync_ms: u64,
    /// The value most recently PUT ON THE WIRE for each watched property.
    ///
    /// ⚠️ Updated only when a packet is actually sent, never when a change is
    /// merely observed. A value that changes while the interval gate is holding
    /// the packet back would otherwise be recorded as sent and never travel.
    watch_last_sent: HashMap<String, Variant>,
    /// Per-peer visibility overrides (Godot per-peer visibility). A peer absent
    /// from the map falls back to `public_visibility`.
    visibility: HashMap<PeerId, bool>,
    /// Default visibility for peers without an explicit override.
    public_visibility: bool,
    /// Optional replication config describing spawn/sync properties.
    ///
    /// ⚠️ NOT ON THE LIVE PATH (pat-s3rrv.177). Nothing reads this field:
    /// `poll_outbound` never consults it, and its two accessors below have zero
    /// callers in the workspace — no other crate, no test, not even this file.
    /// The replication that actually runs is driven by `SceneTree`'s
    /// `__replication__properties` reader in `gdscene/src/scene_tree.rs`, which
    /// is fed from the `.tscn` sub-resource.
    ///
    /// It is left in place rather than deleted because wiring it up is a real
    /// design decision with a live blast radius, not a cleanup: the existing
    /// path works, so replacing half of it would be a regression. Decide
    /// deliberately — wire it or remove it — and do not assume setting it here
    /// changes what goes on the wire, because today it does not.
    replication_config: SceneReplicationConfig,
}

impl MultiplayerSynchronizer {
    /// Create a new synchronizer.
    pub fn new(sync_interval_ms: u32) -> Self {
        Self {
            properties_to_sync: Vec::new(),
            properties_to_watch: Vec::new(),
            sync_interval_ms,
            root_path: String::from(".."),
            net_id: 0,
            property_values: HashMap::new(),
            last_sync_ms: 0,
            watch_last_sent: HashMap::new(),
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
    ///
    /// ⚠️ Reading this tells you what was *stored*, not what replicates — see
    /// the field's note. Zero callers today.
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

    /// Registers a property replicated ONLY when its value changes
    /// (REPLICATION_MODE_ON_CHANGE).
    ///
    /// ⚠️ Removes `path` from the every-tick sync list if it is there. Upstream's
    /// sync and watch sets are disjoint views of one `mode`, so a path in both
    /// would be sent unconditionally AND on change — the every-tick send winning,
    /// which is exactly the bug the mode exists to prevent.
    pub fn add_watch_property(&mut self, path: impl Into<String>) {
        let path = path.into();
        self.properties_to_sync.retain(|p| p != &path);
        if !self.properties_to_watch.contains(&path) {
            self.properties_to_watch.push(path);
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

    /// Write a property value on the authority, as the [`Variant`] it will be
    /// replicated as (pat-s3rrv.17).
    pub fn set_property_value(&mut self, name: &str, value: Variant) {
        // ⚠️ This auto-registers unknown names into the every-tick sync list, so
        // a WATCHED property must be excluded explicitly. Without this guard the
        // per-frame value copy — which has to record watched values in order to
        // detect changes — would silently move every watched property back into
        // `properties_to_sync`, restoring the every-tick send this mode exists to
        // avoid. The feature would look wired and do nothing.
        if !self.properties_to_watch.iter().any(|p| p == name)
            && !self.properties_to_sync.iter().any(|p| p == name)
        {
            self.properties_to_sync.push(name.to_string());
        }
        self.property_values.insert(name.to_string(), value);
    }

    /// Read a property value, if set.
    pub fn get_property_value(&self, name: &str) -> Option<&Variant> {
        self.property_values.get(name)
    }

    /// Snapshot of all currently held property values in registration order.
    pub fn property_snapshot(&self) -> Vec<(String, Variant)> {
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
        let mut props = self.property_snapshot();
        // 🔑 Watched properties join the packet ONLY when their value differs from
        // what was last sent (REPLICATION_MODE_ON_CHANGE). Before this they rode
        // in `properties_to_sync` and were retransmitted every tick, which is the
        // behaviour ON_CHANGE exists to avoid.
        let changed = self.changed_watch_properties();
        props.extend(changed.iter().cloned());
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
        // ⚠️ Recorded here, AFTER a successful send — not where the change was
        // detected. If a watched value changes while the interval gate is holding
        // the packet back, marking it sent at detection time would drop the
        // update permanently: the next poll would compare equal and skip it.
        for (name, value) in changed {
            self.watch_last_sent.insert(name, value);
        }
        Ok(true)
    }

    /// Watched properties whose current value differs from the last one sent.
    ///
    /// ⚠️ A property with no recorded last-sent value counts as changed, so the
    /// first update after configuration always travels. Treating "never sent" as
    /// unchanged would leave a watched property silent until it happened to be
    /// written twice.
    fn changed_watch_properties(&self) -> Vec<(String, Variant)> {
        self.properties_to_watch
            .iter()
            .filter_map(|name| self.property_values.get(name).map(|v| (name, v)))
            .filter(|(name, value)| {
                self.watch_last_sent
                    .get(name.as_str())
                    .map_or(true, |sent| sent != *value)
            })
            .map(|(name, value)| (name.clone(), value.clone()))
            .collect()
    }

    /// Apply an inbound sync packet, updating local property values.
    ///
    /// Returns the decoded packet when the payload is a sync frame.
    pub fn apply_sync_packet(&mut self, packet: &Packet) -> Option<SyncPacket> {
        let decoded = decode_sync_packet(&packet.data)?;
        for (name, value) in &decoded.properties {
            // Same guard as `set_property_value`: an inbound watched property
            // must not be reclassified as every-tick sync on the receiver.
            if !self.properties_to_watch.iter().any(|p| p == name)
                && !self.properties_to_sync.iter().any(|p| p == name)
            {
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

// --------------------------------------------------- SceneReplicationConfig handles
//
// `SceneReplicationConfig` is a `Resource`, so a script reaches one through a
// tagged `ObjectId` into a process-wide table — the same shape as
// `crate::packet_peer_stream` and `crate::http_client_handle`.

/// Tags an `ObjectId` issued by [`new_replication_config_handle`]. `0x53` is
/// ASCII `S`.
///
/// ⚠️ THE TAG BYTE IS A WORKSPACE-WIDE NAMESPACE, and it is filling up fast —
/// the occupied set grew from 31 to 54 distinct bytes in the time I was working
/// on this bead. Sweep IMMEDIATELY before claiming one, and sweep two ways: by
/// value shape (`0x[0-9A-Fa-f]{2}_?00_0000_0000_0000`, note some are written
/// `0xA7_00_…`) AND by declaration shape (`const [A-Z_]*(TAG|HANDLE)[A-Z_]*: u64`).
/// Neither alone is complete. A collision makes `is_*_handle` answer true for
/// another module's ids and surfaces as `MethodNotFound` naming the wrong class.
const REPLICATION_CONFIG_HANDLE_TAG: u64 = 0x5300_0000_0000_0000;

static NEXT_REPLICATION_CONFIG_HANDLE: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(1);

static REPLICATION_CONFIG_HANDLES: std::sync::Mutex<
    Option<std::collections::HashMap<u64, SceneReplicationConfig>>,
> = std::sync::Mutex::new(None);

fn replication_config_handles(
) -> std::sync::MutexGuard<'static, Option<std::collections::HashMap<u64, SceneReplicationConfig>>>
{
    // A poisoned lock still holds a usable table: a panic in one script's
    // handler must not make every later call fail.
    REPLICATION_CONFIG_HANDLES
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Returns `true` when `id` was issued by [`new_replication_config_handle`].
pub fn is_replication_config_handle(id: gdcore::id::ObjectId) -> bool {
    id.raw() & 0xFF00_0000_0000_0000 == REPLICATION_CONFIG_HANDLE_TAG
}

/// Allocates an empty config and returns its script-visible id
/// (Godot `SceneReplicationConfig.new()`).
pub fn new_replication_config_handle() -> gdcore::id::ObjectId {
    let seq = NEXT_REPLICATION_CONFIG_HANDLE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let raw = REPLICATION_CONFIG_HANDLE_TAG | seq;
    replication_config_handles()
        .get_or_insert_with(std::collections::HashMap::new)
        .insert(raw, SceneReplicationConfig::new());
    gdcore::id::ObjectId::from_raw(raw)
}

/// Runs `f` against the live config behind `id`, or returns `None` when the
/// handle was never issued or has already been released.
pub fn with_replication_config<R>(
    id: gdcore::id::ObjectId,
    f: impl FnOnce(&mut SceneReplicationConfig) -> R,
) -> Option<R> {
    let mut guard = replication_config_handles();
    let map = guard.get_or_insert_with(std::collections::HashMap::new);
    map.get_mut(&id.raw()).map(f)
}

/// Drops a handle. Idempotent.
pub fn release_replication_config_handle(id: gdcore::id::ObjectId) {
    let mut guard = replication_config_handles();
    if let Some(map) = guard.as_mut() {
        map.remove(&id.raw());
    }
}

/// A single property entry in a [`SceneReplicationConfig`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplicationProperty {
    /// The node-relative property path (e.g. `.:position`).
    pub path: String,
    /// Whether the property is part of the spawn state.
    ///
    /// Independent of [`mode`](Self::mode): upstream's `_update()` pushes into
    /// `spawn_props` outside the mode switch entirely.
    pub spawn: bool,
    /// Whether the value is sent every tick, only on change, or never.
    ///
    /// 🔑 There is no separate `sync` field, matching upstream: `sync` and
    /// `watch` are both DERIVED from this one value (`Always` and `OnChange`
    /// respectively). Storing a `sync` bool alongside it made the contradictory
    /// state `sync = true, mode = OnChange` representable, which upstream cannot
    /// express at all.
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

    /// Sets whether `path` is replicated every tick.
    ///
    /// ⚠️ Same asymmetry as [`property_set_watch`](Self::property_set_watch),
    /// and for the same reason: enabling sets `Always`, but disabling only
    /// downgrades to `Never` **if the mode was `Always`**. An unconditional
    /// `else` would clobber a watched property into `Never` merely because
    /// something cleared its sync flag.
    pub fn property_set_sync(&mut self, path: &str, enabled: bool) {
        if enabled {
            self.property_set_replication_mode(path, ReplicationMode::Always);
        } else if self.property_get_replication_mode(path) == ReplicationMode::Always {
            self.property_set_replication_mode(path, ReplicationMode::Never);
        }
    }

    /// Whether `path` is replicated every tick (false if absent).
    ///
    /// Derived from the mode, exactly as upstream's `property_get_sync` is.
    pub fn property_get_sync(&self, path: &str) -> bool {
        self.index_of(path)
            .map(|i| self.properties[i].mode == ReplicationMode::Always)
            .unwrap_or(false)
    }

    /// Whether `path` is replicated only on change (upstream
    /// `property_get_watch`).
    ///
    /// 🔑 Upstream stores no `watch` bool — this IS
    /// `mode == REPLICATION_MODE_ON_CHANGE`
    /// (`scene_replication_config.cpp`), which is why watch and sync can never
    /// both be true for one property.
    pub fn property_get_watch(&self, path: &str) -> bool {
        self.index_of(path)
            .map(|i| self.properties[i].mode == ReplicationMode::OnChange)
            .unwrap_or(false)
    }

    /// Sets whether `path` is replicated only on change.
    ///
    /// ⚠️ Mirrors upstream's asymmetry exactly: enabling sets `OnChange`, but
    /// DISABLING only downgrades to `Never` **if the mode was `OnChange`** — it
    /// must not clobber an `Always` property into `Never`. Writing the `else`
    /// branch unconditionally would silently switch off a perfectly good
    /// every-tick property whenever something cleared its watch flag.
    pub fn property_set_watch(&mut self, path: &str, enabled: bool) {
        if enabled {
            self.property_set_replication_mode(path, ReplicationMode::OnChange);
        } else if self.property_get_replication_mode(path) == ReplicationMode::OnChange {
            self.property_set_replication_mode(path, ReplicationMode::Never);
        }
    }

    /// Property paths replicated only on change — mode `OnChange`, in order.
    ///
    /// Disjoint from [`sync_properties`](Self::sync_properties) by construction.
    pub fn watch_properties(&self) -> Vec<String> {
        self.properties
            .iter()
            .filter(|p| p.mode == ReplicationMode::OnChange)
            .map(|p| p.path.clone())
            .collect()
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

    /// Property paths in the periodic sync set — mode `Always`, in order.
    ///
    /// ⚠️ Was `p.mode != Never`, which let `OnChange` into the every-tick set.
    /// Upstream's `_update()` routes ALWAYS into `sync_props` and ON_CHANGE into
    /// `watch_props`; the two are disjoint, so an `OnChange` property here would
    /// be retransmitted every tick AND treated as watched.
    pub fn sync_properties(&self) -> Vec<String> {
        self.properties
            .iter()
            .filter(|p| p.mode == ReplicationMode::Always)
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
        assert!(
            !sync.is_visible_to(PeerId(3)),
            "private hides un-overridden peers"
        );
        sync.set_visibility_for(PeerId(3), true);
        assert!(sync.is_visible_to(PeerId(3)), "explicit override wins");
    }

    #[test]
    fn synchronizer_hidden_peer_gets_no_sync() {
        let mut sync = MultiplayerSynchronizer::new(0);
        sync.set_property_value("position", Variant::Float(10.0));
        let mut peer = MockNetworkPeer::new(PeerId::SERVER);

        // Visible → a forced poll sends a packet.
        assert!(sync
            .poll_outbound(&mut peer, PeerId(2), 1000, true)
            .unwrap());

        // Hidden from that peer → no packet even when forced.
        sync.set_visibility_for(PeerId(2), false);
        assert!(!sync
            .poll_outbound(&mut peer, PeerId(2), 2000, true)
            .unwrap());
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

        // 🔑 ON_CHANGE partitions into WATCH, never into sync. Before this,
        // `sync_properties` filtered on `mode != Never`, so a watched property
        // was in the every-tick set as well.
        cfg.property_set_watch("health", true);
        assert!(cfg.property_get_watch("health"));
        assert_eq!(cfg.watch_properties(), vec!["health".to_string()]);
        assert!(
            cfg.sync_properties().is_empty(),
            "a watched property must leave the sync set: the two are disjoint \
             views of one mode"
        );

        // ⚠️ Disabling watch on a property that is NOT watched must not touch it.
        cfg.property_set_replication_mode("health", ReplicationMode::Always);
        cfg.property_set_watch("health", false);
        assert_eq!(
            cfg.property_get_replication_mode("health"),
            ReplicationMode::Always,
            "clearing watch on an Always property must leave it Always, not \
             downgrade it to Never"
        );

        // …and the mirror: clearing SYNC on a watched property must not clobber
        // it into Never either. The two setters share the asymmetry because they
        // are two views of one field.
        cfg.property_set_watch("health", true);
        cfg.property_set_sync("health", false);
        assert_eq!(
            cfg.property_get_replication_mode("health"),
            ReplicationMode::OnChange,
            "clearing sync on a watched property must leave it OnChange"
        );

        // 🔑 The contradictory state upstream cannot express is now unrepresentable:
        // enabling one mode necessarily disables the other.
        cfg.property_set_sync("health", true);
        assert!(cfg.property_get_sync("health"));
        assert!(
            !cfg.property_get_watch("health"),
            "sync and watch are one field — turning sync on must turn watch off"
        );

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

    // -- pat-0q2kx.95: Godot NETWORK_COMMAND framing + real Variant codec ----

    /// The acceptance's first clause: structured args must survive the wire
    /// **as Variants**. The old private codec's fallback arm was
    /// `format!("{other}")`, so a Vector2 arrived as the String "(1, 2)".
    #[test]
    fn rpc_args_roundtrip_structured_variants_not_strings() {
        let args = vec![
            Variant::Vector2(gdcore::math::Vector2::new(1.5, -2.5)),
            Variant::Array(vec![Variant::Int(7), Variant::Bool(true)]),
            Variant::Dictionary(
                vec![(Variant::String("hp".into()), Variant::Int(42))]
                    .into_iter()
                    .collect(),
            ),
        ];
        let data = encode_rpc_packet(0x1234, "take_damage", &args).unwrap();
        let (path, method, decoded) = decode_rpc_packet(&data).expect("decode");

        assert_eq!(path, 0x1234);
        assert_eq!(method, "take_damage");
        assert_eq!(
            decoded, args,
            "every arg must round-trip as the same Variant, not a stringified one"
        );
        for arg in &decoded {
            assert!(
                !matches!(arg, Variant::String(_)),
                "no structured arg may degrade to a String: {arg:?}"
            );
        }
    }

    /// The acceptance's third clause: a RAW packet must never enter RPC
    /// dispatch. With one shared header this is a command-byte check rather
    /// than two unrelated magics that could both miss.
    #[test]
    fn raw_packet_does_not_enter_rpc_dispatch() {
        let raw = encode_raw_packet(b"hello");
        assert_eq!(raw[0], NetworkCommand::Raw.to_byte());
        assert!(
            decode_rpc_packet(&raw).is_none(),
            "a RAW packet must not decode as an RPC"
        );
        assert_eq!(decode_raw_packet(&raw), Some(b"hello".to_vec()));

        // ...and the reverse: an RPC must not be consumed as a raw payload.
        let rpc = encode_rpc_packet(1, "ping", &[]).unwrap();
        assert!(decode_raw_packet(&rpc).is_none());
    }

    /// Every other command byte is reserved for the replication/sys traffic and
    /// must also stay out of RPC dispatch.
    #[test]
    fn non_remote_call_commands_are_rejected_by_the_rpc_decoder() {
        let mut packet = encode_rpc_packet(9, "m", &[]).unwrap();
        for cmd in [
            NetworkCommand::SimplifyPath,
            NetworkCommand::ConfirmPath,
            NetworkCommand::Spawn,
            NetworkCommand::Despawn,
            NetworkCommand::Sync,
            NetworkCommand::Sys,
        ] {
            packet[0] = cmd.to_byte();
            assert!(
                decode_rpc_packet(&packet).is_none(),
                "{cmd:?} must not decode as an RPC"
            );
        }
        // An unknown byte is rejected rather than guessed at.
        packet[0] = 0xFE;
        assert!(decode_rpc_packet(&packet).is_none());
    }

    /// Command bytes are Godot's wire values
    /// (scene_multiplayer.h `enum NetworkCommands`) and must not be renumbered.
    #[test]
    fn network_command_bytes_match_godot() {
        for (cmd, byte) in [
            (NetworkCommand::RemoteCall, 0u8),
            (NetworkCommand::SimplifyPath, 1),
            (NetworkCommand::ConfirmPath, 2),
            (NetworkCommand::Raw, 3),
            (NetworkCommand::Spawn, 4),
            (NetworkCommand::Despawn, 5),
            (NetworkCommand::Sync, 6),
            (NetworkCommand::Sys, 7),
        ] {
            assert_eq!(cmd.to_byte(), byte);
            assert_eq!(NetworkCommand::from_byte(byte), Some(cmd));
        }
        assert_eq!(NetworkCommand::from_byte(8), None);
    }

    /// Every packet kind must occupy a **distinct** command byte.
    ///
    /// The replication packets carried a private `0x01`/`0x02` numbering from
    /// before the shared header, which collides with `SIMPLIFY_PATH` (1) and
    /// `CONFIRM_PATH` (2). Nothing misrouted, because the RPC and RAW decoders
    /// only accept their own byte — but the collision would have become a real
    /// ambiguity the moment the path cache landed. This pins the namespace so a
    /// future kind cannot quietly reuse a taken byte (pat-0q2kx.95).
    #[test]
    fn packet_kinds_do_not_share_command_bytes() {
        assert_eq!(REPLICATION_KIND_SPAWN, NetworkCommand::Spawn.to_byte());
        assert_eq!(REPLICATION_KIND_SYNC, NetworkCommand::Sync.to_byte());
        assert_ne!(
            REPLICATION_KIND_SPAWN,
            NetworkCommand::SimplifyPath.to_byte()
        );
        assert_ne!(REPLICATION_KIND_SYNC, NetworkCommand::ConfirmPath.to_byte());

        // A spawn packet must not be mistaken for an RPC or a raw payload.
        let spawn = encode_spawn_packet(&SpawnRequest {
            net_id: 1,
            scene_index: 0,
            scene_path: "res://a.tscn".into(),
            data: String::new(),
            properties: Vec::new(),
        });
        assert!(decode_rpc_packet(&spawn).is_none(), "spawn is not an RPC");
        assert!(decode_raw_packet(&spawn).is_none(), "spawn is not raw");
        assert!(decode_spawn_packet(&spawn).is_some(), "spawn still decodes");

        let sync = encode_sync_packet(1, &[("hp".into(), Variant::Int(5))]);
        assert!(decode_rpc_packet(&sync).is_none(), "sync is not an RPC");
        assert!(decode_raw_packet(&sync).is_none(), "sync is not raw");
        assert!(decode_sync_packet(&sync).is_some(), "sync still decodes");
    }

    /// Object decoding is denied by default, as Godot's
    /// `allow_object_decoding` is — decoding an object from an untrusted peer
    /// is remote code execution.
    #[test]
    fn object_args_are_denied_unless_explicitly_allowed() {
        let args = vec![Variant::ObjectId(gdcore::id::ObjectId::from_raw(7))];
        let data = encode_rpc_packet(1, "grab", &args).unwrap();

        assert!(
            decode_rpc_packet(&data).is_none(),
            "the default policy must reject an encoded object"
        );
        let allowed = decode_rpc_packet_with(&data, ObjectDecoding::Allow);
        assert!(
            allowed.is_some(),
            "an explicitly trusting caller can still decode it"
        );
    }

    #[test]
    fn binary_rpc_roundtrip_not_text_toy() {
        let data = encode_rpc_packet(
            0xABCD,
            "set_hp",
            &[
                Variant::Int(100),
                Variant::String("ok".into()),
                Variant::Bool(true),
            ],
        )
        .unwrap();
        // Godot's command byte leads the packet, not a bespoke magic.
        assert_eq!(data[0], NetworkCommand::RemoteCall.to_byte());
        assert!(!data.starts_with(b"rpc:"));
        assert!(!data.starts_with(b"PRPC"), "the PRPC magic is gone");
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
        auth_sync.set_property_value("position", Variant::Float(10.0));
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
        assert_eq!(
            remote_sync.get_property_value("position"),
            Some(&Variant::Float(10.0))
        );
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
            // ⚠️ Typed values, not strings: the whole point of carrying binary
            // Variants is that a Vector2 arrives as a Vector2. A String-only
            // fixture would pass against a stringifying encoder.
            properties: vec![
                ("hp".to_string(), Variant::Int(42)),
                (
                    "pos".to_string(),
                    Variant::Vector2(gdcore::math::Vector2::new(1.5, -2.5)),
                ),
            ],
        };
        let bytes = encode_spawn_packet(&req);
        let back = decode_spawn_packet(&bytes).unwrap();
        assert_eq!(
            back.properties, req.properties,
            "spawn-state values must round-trip with their types intact",
        );
        // A spawn with no spawn-flagged properties still decodes — and so does a
        // packet truncated before the section, which is the pre-pat-s3rrv.17
        // wire shape.
        let bare = SpawnRequest {
            properties: Vec::new(),
            ..req.clone()
        };
        let bare_bytes = encode_spawn_packet(&bare);
        assert!(decode_spawn_packet(&bare_bytes).is_some());
        assert!(
            decode_spawn_packet(&bare_bytes[..bare_bytes.len() - 2])
                .is_some_and(|p| p.properties.is_empty()),
            "a packet ending before the property section is a spawn with none, \
             not a decode failure",
        );
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
        auth.set_property_value("position", Variant::Float(1.5));
        assert!(auth
            .poll_outbound(&mut server, PeerId(2), 100, true)
            .unwrap());

        let packets = client.poll();
        assert_eq!(packets.len(), 1);
        let mut remote = MultiplayerSynchronizer::new(0);
        let applied = remote.apply_sync_packet(&packets[0]).unwrap();
        assert_eq!(applied.net_id, 42);
        assert_eq!(
            remote.get_property_value("position"),
            Some(&Variant::Float(1.5))
        );
        assert_eq!(remote.get_net_id(), 42);
    }

    #[test]
    fn synchronizer_respects_interval() {
        let (mut server, mut client) = MockNetwork::create_pair(PeerId(2));
        let mut auth = MultiplayerSynchronizer::new(100);
        auth.set_property_value("position", Variant::Float(0.0));
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
