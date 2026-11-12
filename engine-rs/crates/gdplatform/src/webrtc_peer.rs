//! WebRTC: `WebRTCPeerConnection`, `WebRTCDataChannel` and
//! `WebRTCMultiplayerPeer` (pat-s3rrv.80).
//!
//! ## What this replaced
//!
//! This file was 149 lines of mock. `create_offer` returned the string literal
//! `"v=0\r\no=- 0 0 IN IP4 127.0.0.1\r\ns=Patina\r\n"`; the state machine flipped
//! to Connected on a `poll` that never looked at the remote description; and
//! `deliver_to` copied a `Vec<u8>` from one struct's queue into another's when a
//! test asked it to. Nothing in the engine used any of it — the only references
//! outside this file were the `pub mod` and `pub use` lines in `lib.rs`.
//!
//! ## What negotiation means here
//!
//! The session descriptions are **generated**, not canned: each connection mints
//! its own ICE credentials and writes a real datachannel SDP around them, and a
//! peer learns who it is talking to by *parsing* `a=ice-ufrag:` out of the SDP it
//! is given. That is what makes the exchange load-bearing — a connection that is
//! handed no remote description, or one whose ufrag names nobody, never reaches
//! `Connected` and its channels never open.
//!
//! ⚠️ **The transport is in-process loopback, and only in-process.** Two
//! connections in the same process find each other through [`REGISTRY`]; there is
//! no ICE agent, no DTLS and no SCTP, so a connection can never reach a peer in
//! another process or another machine. This is deliberate and is the honest
//! shape of the native build: the browser is where real WebRTC lives (Godot's own
//! web export binds `RTCPeerConnection` through `library_godot_webrtc.js`), and
//! the wasm32 binding is recorded as remaining scope on the bead. What is NOT
//! acceptable is the previous arrangement, where a hardcoded SDP and an
//! unconditional state flip made the absence look like a working stack.
//!
//! Godot ground truth: `modules/webrtc/webrtc_peer_connection.h` (the three state
//! enums), `webrtc_data_channel.h` (write mode + ready state),
//! `webrtc_multiplayer_peer.cpp` (`create_mesh`/`create_server`/`create_client`/
//! `add_peer` and the reserved channel layout).

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, Weak};

use crate::network::{
    ConnectionStatus, NetworkError, NetworkPeer, Packet, PeerId, Result, TransferMode,
};

/// Locks through a poisoned mutex rather than panicking.
///
/// A panic in one connection would otherwise poison the shared mailbox and take
/// down every other peer in the process with an unrelated error.
fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

// ---------------------------------------------------------------------------
// Enums — values and order from Godot's headers
// ---------------------------------------------------------------------------

/// `WebRTCPeerConnection.ConnectionState` (`webrtc_peer_connection.h:40`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionState {
    New = 0,
    Connecting = 1,
    Connected = 2,
    Disconnected = 3,
    Failed = 4,
    Closed = 5,
}

/// `WebRTCPeerConnection.GatheringState` (`webrtc_peer_connection.h:49`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GatheringState {
    New = 0,
    Gathering = 1,
    Complete = 2,
}

/// `WebRTCPeerConnection.SignalingState` (`webrtc_peer_connection.h:55`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignalingState {
    Stable = 0,
    HaveLocalOffer = 1,
    HaveRemoteOffer = 2,
    HaveLocalPranswer = 3,
    HaveRemotePranswer = 4,
    Closed = 5,
}

/// `WebRTCDataChannel.ChannelState` (`webrtc_data_channel.h:45`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChannelState {
    Connecting = 0,
    Open = 1,
    Closing = 2,
    Closed = 3,
}

/// `WebRTCDataChannel.WriteMode` (`webrtc_data_channel.h:40`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteMode {
    Text = 0,
    Binary = 1,
}

/// One `session_description_created` event: Godot emits `(type, sdp)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionDescription {
    /// `"offer"` or `"answer"`, the WebRTC session description type.
    pub kind: String,
    /// The SDP body.
    pub sdp: String,
}

/// One `ice_candidate_created` event: Godot emits `(media, index, name)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IceCandidate {
    pub media: String,
    pub index: i64,
    pub name: String,
}

/// The `create_data_channel` options dictionary
/// (`doc/classes/WebRTCPeerConnection.xml`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataChannelConfig {
    pub ordered: bool,
    pub max_packet_life_time: Option<i64>,
    pub max_retransmits: Option<i64>,
    pub negotiated: bool,
    pub id: Option<i64>,
    pub protocol: String,
}

impl Default for DataChannelConfig {
    /// Browser defaults: ordered, unlimited retransmits, not negotiated.
    fn default() -> Self {
        Self {
            ordered: true,
            max_packet_life_time: None,
            max_retransmits: None,
            negotiated: false,
            id: None,
            protocol: String::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// Loopback transport
// ---------------------------------------------------------------------------

/// A connection's mailbox, shared with every channel it owns and written to by
/// whichever peer has bound it as its remote.
#[derive(Debug, Default)]
struct Mailbox {
    /// Channel label to received packets, each flagged with whether it was
    /// written in text mode (Godot's `was_string_packet`).
    inbox: HashMap<String, VecDeque<(Vec<u8>, bool)>>,
    /// The remote's mailbox, resolved from the ufrag in its SDP.
    ///
    /// ⚠️ `Weak`, not `Arc`: two connected peers each hold the other, so a
    /// strong reference is a cycle — the pair would leak both mailboxes for the
    /// life of the process, and `{:?}` on either would recurse forever. The
    /// strong references live in [`REGISTRY`] and in the channels.
    remote: Option<Weak<Mutex<Mailbox>>>,
    /// Set by [`WebRTCPeerConnection::poll`] once negotiation completes; the
    /// channels read it rather than keeping their own copy.
    open: bool,
    /// Labels of the NON-negotiated channels this connection opened.
    ///
    /// A `negotiated: true` channel is configured identically on both sides by
    /// the application, so Godot fires no `data_channel_received` for it — only
    /// a channel the remote did not know about is announced. Godot's reserved
    /// multiplayer channels are all negotiated, which is why that event is
    /// silent for them.
    announced: Vec<String>,
}

/// Every live connection in this process, keyed by its own ICE ufrag.
///
/// This is what stands in for an ICE agent: a peer resolves its remote by
/// looking up the ufrag it parsed out of the SDP it was handed. Nothing else can
/// bind a remote, so a connection that was never given a description stays
/// unconnected.
static REGISTRY: OnceLock<Mutex<HashMap<String, Arc<Mutex<Mailbox>>>>> = OnceLock::new();

fn registry() -> &'static Mutex<HashMap<String, Arc<Mutex<Mailbox>>>> {
    REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Source of ICE credentials.
///
/// ⚠️ A process-local counter, NOT a random source: these values are unique
/// within a run, which is all the loopback registry needs, and being
/// deterministic makes a failing test reproducible. Real credentials come from
/// the browser's own `RTCPeerConnection` on the wasm32 path.
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

fn mint(prefix: &str) -> String {
    format!("{prefix}{:08x}", NEXT_ID.fetch_add(1, Ordering::Relaxed))
}

/// Reads an `a=<key>:<value>` attribute out of an SDP body.
fn sdp_attribute(sdp: &str, key: &str) -> Option<String> {
    let needle = format!("a={key}:");
    sdp.lines()
        .find_map(|line| line.trim().strip_prefix(&needle))
        .map(|v| v.trim().to_string())
}

// ---------------------------------------------------------------------------
// WebRTCPeerConnection
// ---------------------------------------------------------------------------

/// A WebRTC peer connection with Godot's method and event surface.
#[derive(Debug)]
pub struct WebRTCPeerConnection {
    ufrag: String,
    pwd: String,
    session_id: u64,
    state: ConnectionState,
    gathering: GatheringState,
    signaling: SignalingState,
    local_description: Option<SessionDescription>,
    remote_description: Option<SessionDescription>,
    /// Remote candidates handed to us by the signalling channel. Connecting
    /// requires at least one, so a signalling loop that forwards descriptions
    /// but drops candidates does not silently appear to work.
    remote_candidates: usize,
    /// Pending `session_description_created` events.
    descriptions: VecDeque<SessionDescription>,
    /// Pending `ice_candidate_created` events.
    candidates: VecDeque<IceCandidate>,
    /// Labels of channels the remote opened, drained as `data_channel_received`.
    received_channels: VecDeque<String>,
    mailbox: Arc<Mutex<Mailbox>>,
    /// Labels already opened, so a duplicate can be refused.
    channels: Vec<String>,
}

impl Default for WebRTCPeerConnection {
    fn default() -> Self {
        Self::new()
    }
}

impl WebRTCPeerConnection {
    /// Creates a connection and registers its mailbox under fresh ICE
    /// credentials (Godot's `WebRTCPeerConnection.create` + `initialize`).
    pub fn new() -> Self {
        let ufrag = mint("pat");
        let mailbox = Arc::new(Mutex::new(Mailbox::default()));
        lock(registry()).insert(ufrag.clone(), Arc::clone(&mailbox));
        Self {
            pwd: mint("pwd"),
            session_id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
            ufrag,
            state: ConnectionState::New,
            gathering: GatheringState::New,
            signaling: SignalingState::Stable,
            local_description: None,
            remote_description: None,
            remote_candidates: 0,
            descriptions: VecDeque::new(),
            candidates: VecDeque::new(),
            received_channels: VecDeque::new(),
            mailbox,
            channels: Vec::new(),
        }
    }

    /// This connection's ICE ufrag — the identity a remote resolves it by.
    pub fn ice_ufrag(&self) -> &str {
        &self.ufrag
    }

    /// Builds this connection's SDP.
    ///
    /// A datachannel-only session: one `m=application` media line carrying the
    /// SCTP port, with this connection's own ICE credentials. `a=setup:actpass`
    /// on an offer and `a=setup:active` on an answer, per RFC 5763.
    fn write_sdp(&self, kind: &str) -> String {
        let setup = if kind == "offer" { "actpass" } else { "active" };
        format!(
            "v=0\r\n\
             o=- {session} 1 IN IP4 127.0.0.1\r\n\
             s=-\r\n\
             t=0 0\r\n\
             a=group:BUNDLE 0\r\n\
             m=application 9 UDP/DTLS/SCTP webrtc-datachannel\r\n\
             c=IN IP4 0.0.0.0\r\n\
             a=mid:0\r\n\
             a=ice-ufrag:{ufrag}\r\n\
             a=ice-pwd:{pwd}\r\n\
             a=setup:{setup}\r\n\
             a=sctp-port:5000\r\n\
             a=max-message-size:262144\r\n",
            session = self.session_id,
            ufrag = self.ufrag,
            pwd = self.pwd,
        )
    }

    /// Generates an offer, queued as a `session_description_created` event.
    ///
    /// Godot does not return the SDP: the description arrives through the signal
    /// so the browser's asynchronous `createOffer` promise has the same shape as
    /// this one. Drain it with [`next_session_description`](Self::next_session_description).
    pub fn create_offer(&mut self) -> Result<()> {
        if self.state == ConnectionState::Closed {
            return Err(NetworkError::Transport(
                "create_offer on a closed connection".into(),
            ));
        }
        let sdp = self.write_sdp("offer");
        self.descriptions.push_back(SessionDescription {
            kind: "offer".into(),
            sdp,
        });
        Ok(())
    }

    /// Applies a description this connection generated, and starts gathering.
    pub fn set_local_description(&mut self, kind: &str, sdp: &str) -> Result<()> {
        self.local_description = Some(SessionDescription {
            kind: kind.to_string(),
            sdp: sdp.to_string(),
        });
        self.signaling = match kind {
            "offer" => SignalingState::HaveLocalOffer,
            _ => SignalingState::Stable,
        };
        if self.state == ConnectionState::New {
            self.state = ConnectionState::Connecting;
        }
        self.gather();
        Ok(())
    }

    /// Applies the remote's description, resolving who it is from its ufrag.
    ///
    /// ⚠️ Setting a remote **offer** generates the answer immediately, as the
    /// browser glue does (`library_godot_webrtc.js` chains `createAnswer` onto
    /// `setRemoteDescription`), so a signalling script never has to know that an
    /// answer is owed.
    pub fn set_remote_description(&mut self, kind: &str, sdp: &str) -> Result<()> {
        let Some(remote_ufrag) = sdp_attribute(sdp, "ice-ufrag") else {
            return Err(NetworkError::Transport(
                "remote description carries no a=ice-ufrag".into(),
            ));
        };
        let remote = lock(registry()).get(&remote_ufrag).map(Arc::clone);
        // Bound here rather than at poll time: a description naming a peer that
        // does not exist must not look like one that simply has not connected
        // yet.
        let Some(remote) = remote else {
            self.state = ConnectionState::Failed;
            return Err(NetworkError::Transport(format!(
                "no connection in this process answers to ice-ufrag {remote_ufrag}"
            )));
        };
        lock(&self.mailbox).remote = Some(Arc::downgrade(&remote));

        self.remote_description = Some(SessionDescription {
            kind: kind.to_string(),
            sdp: sdp.to_string(),
        });
        if self.state == ConnectionState::New {
            self.state = ConnectionState::Connecting;
        }
        if kind == "offer" {
            self.signaling = SignalingState::HaveRemoteOffer;
            let answer = self.write_sdp("answer");
            self.local_description = Some(SessionDescription {
                kind: "answer".into(),
                sdp: answer.clone(),
            });
            self.descriptions.push_back(SessionDescription {
                kind: "answer".into(),
                sdp: answer,
            });
            self.signaling = SignalingState::Stable;
            self.gather();
        } else {
            self.signaling = SignalingState::Stable;
        }
        Ok(())
    }

    /// Emits this connection's ICE candidate and completes gathering.
    fn gather(&mut self) {
        if self.gathering == GatheringState::Complete {
            return;
        }
        self.gathering = GatheringState::Gathering;
        self.candidates.push_back(IceCandidate {
            media: "0".into(),
            index: 0,
            name: format!(
                "candidate:1 1 UDP 2130706431 127.0.0.1 9 typ host ufrag {}",
                self.ufrag
            ),
        });
        self.gathering = GatheringState::Complete;
    }

    /// Accepts a candidate relayed from the remote.
    pub fn add_ice_candidate(&mut self, _media: &str, _index: i64, name: &str) -> Result<()> {
        if name.is_empty() {
            return Err(NetworkError::Transport("empty ICE candidate".into()));
        }
        self.remote_candidates += 1;
        Ok(())
    }

    /// Drains one queued `session_description_created` event.
    pub fn next_session_description(&mut self) -> Option<SessionDescription> {
        self.descriptions.pop_front()
    }

    /// Drains one queued `ice_candidate_created` event.
    pub fn next_ice_candidate(&mut self) -> Option<IceCandidate> {
        self.candidates.pop_front()
    }

    /// Drains one queued `data_channel_received` event (the channel's label).
    pub fn next_received_channel(&mut self) -> Option<String> {
        self.received_channels.pop_front()
    }

    /// Opens a data channel (`create_data_channel(label, config)`).
    ///
    /// ⚠️ Godot requires the connection to still be `STATE_NEW`
    /// (`webrtc_multiplayer_peer.cpp:297` refuses to `add_peer` otherwise),
    /// because a channel added after negotiation would not be in the SDP.
    pub fn create_data_channel(
        &mut self,
        label: &str,
        config: DataChannelConfig,
    ) -> Result<WebRTCDataChannel> {
        if self.state != ConnectionState::New {
            return Err(NetworkError::Transport(
                "data channels must be created before the connection negotiates".into(),
            ));
        }
        // ⚠️ The loopback mailbox routes by LABEL, so two channels sharing one
        // would read each other's packets out of a single queue — an aliasing
        // bug that looks like packet loss on whichever channel reads second.
        if self.channels.iter().any(|open| open == label) {
            return Err(NetworkError::Transport(format!(
                "a data channel labelled '{label}' is already open on this connection"
            )));
        }
        self.channels.push(label.to_string());
        if !config.negotiated {
            lock(&self.mailbox).announced.push(label.to_string());
        }
        Ok(WebRTCDataChannel {
            label: label.to_string(),
            config,
            write_mode: WriteMode::Binary,
            last_was_string: false,
            mailbox: Arc::clone(&self.mailbox),
        })
    }

    /// Builds a local channel object for a label the REMOTE opened.
    ///
    /// The mailbox routes by label, so a channel bound to our own mailbox under
    /// the remote's label reads exactly the packets the remote sends on it —
    /// which is what makes the `data_channel_received` event usable rather than
    /// merely informative.
    ///
    /// ⚠️ Deliberately skips the `STATE_NEW` check and the announcement that
    /// [`create_data_channel`](Self::create_data_channel) applies: this channel
    /// is the remote's, already negotiated, and announcing it back would tell
    /// the remote about its own channel.
    pub fn adopt_remote_channel(&self, label: &str) -> WebRTCDataChannel {
        WebRTCDataChannel {
            label: label.to_string(),
            config: DataChannelConfig::default(),
            write_mode: WriteMode::Binary,
            last_was_string: false,
            mailbox: Arc::clone(&self.mailbox),
        }
    }

    /// Advances the connection (`WebRTCPeerConnection.poll`).
    ///
    /// Connecting requires a local description, a remote description AND at
    /// least one remote ICE candidate — the same three things a real handshake
    /// needs, so a signalling loop that forwards only some of them fails here
    /// rather than appearing to work.
    pub fn poll(&mut self) -> Result<()> {
        if matches!(
            self.state,
            ConnectionState::Closed | ConnectionState::Failed
        ) {
            return Ok(());
        }
        let ready = self.local_description.is_some()
            && self.remote_description.is_some()
            && self.remote_candidates > 0;
        if ready && self.state != ConnectionState::Connected {
            self.state = ConnectionState::Connected;
            let remote = {
                let mut mailbox = lock(&self.mailbox);
                mailbox.open = true;
                mailbox.remote.clone()
            };
            // `data_channel_received` reports the channels the REMOTE opened,
            // read from its own announcement list — not our channels echoed
            // back, which would fire the event for channels we created
            // ourselves.
            if let Some(remote) = remote.and_then(|weak| weak.upgrade()) {
                let labels = lock(&remote).announced.clone();
                for label in labels {
                    self.received_channels.push_back(label);
                }
            }
        }
        Ok(())
    }

    pub fn get_connection_state(&self) -> ConnectionState {
        self.state
    }

    pub fn get_gathering_state(&self) -> GatheringState {
        self.gathering
    }

    pub fn get_signaling_state(&self) -> SignalingState {
        self.signaling
    }

    /// Closes the connection and drops its registry entry.
    pub fn close(&mut self) {
        self.state = ConnectionState::Closed;
        self.signaling = SignalingState::Closed;
        let mut mailbox = lock(&self.mailbox);
        mailbox.open = false;
        mailbox.remote = None;
        drop(mailbox);
        lock(registry()).remove(&self.ufrag);
    }
}

impl Drop for WebRTCPeerConnection {
    /// Without this the registry grows for the life of the process and a ufrag
    /// could resolve to a mailbox nobody is reading.
    fn drop(&mut self) {
        lock(registry()).remove(&self.ufrag);
    }
}

// ---------------------------------------------------------------------------
// WebRTCDataChannel
// ---------------------------------------------------------------------------

/// A data channel on a [`WebRTCPeerConnection`] — Godot's `PacketPeer` surface
/// plus the SCTP configuration.
#[derive(Debug)]
pub struct WebRTCDataChannel {
    label: String,
    config: DataChannelConfig,
    write_mode: WriteMode,
    last_was_string: bool,
    mailbox: Arc<Mutex<Mailbox>>,
}

impl WebRTCDataChannel {
    pub fn get_label(&self) -> &str {
        &self.label
    }

    pub fn is_ordered(&self) -> bool {
        self.config.ordered
    }

    pub fn get_max_packet_life_time(&self) -> Option<i64> {
        self.config.max_packet_life_time
    }

    pub fn get_max_retransmits(&self) -> Option<i64> {
        self.config.max_retransmits
    }

    pub fn is_negotiated(&self) -> bool {
        self.config.negotiated
    }

    pub fn get_id(&self) -> Option<i64> {
        self.config.id
    }

    pub fn get_protocol(&self) -> &str {
        &self.config.protocol
    }

    pub fn get_write_mode(&self) -> WriteMode {
        self.write_mode
    }

    pub fn set_write_mode(&mut self, mode: WriteMode) {
        self.write_mode = mode;
    }

    /// Whether the packet most recently returned by
    /// [`get_packet`](Self::get_packet) was written in text mode.
    pub fn was_string_packet(&self) -> bool {
        self.last_was_string
    }

    /// `Open` once the connection negotiated, `Connecting` before that.
    pub fn get_ready_state(&self) -> ChannelState {
        if lock(&self.mailbox).open {
            ChannelState::Open
        } else {
            ChannelState::Connecting
        }
    }

    /// Sends a packet to the remote end of this channel.
    pub fn put_packet(&mut self, data: Vec<u8>) -> Result<()> {
        // The remote handle is cloned out before it is locked: locking the
        // remote while still holding our own would deadlock two peers sending
        // to each other from different threads.
        let (open, remote) = {
            let mailbox = lock(&self.mailbox);
            (mailbox.open, mailbox.remote.clone())
        };
        // A `Weak` that no longer upgrades means the remote connection and all
        // its channels are gone — reported as "no remote" rather than silently
        // dropping the packet.
        let remote = remote.and_then(|weak| weak.upgrade());
        if !open {
            return Err(NetworkError::Transport(format!(
                "data channel '{}' is not open",
                self.label
            )));
        }
        let Some(remote) = remote else {
            return Err(NetworkError::Transport(format!(
                "data channel '{}' has no remote",
                self.label
            )));
        };
        let is_string = self.write_mode == WriteMode::Text;
        lock(&remote)
            .inbox
            .entry(self.label.clone())
            .or_default()
            .push_back((data, is_string));
        Ok(())
    }

    /// Pops the next received packet (`PacketPeer.get_packet`).
    pub fn get_packet(&mut self) -> Option<Vec<u8>> {
        let popped = lock(&self.mailbox)
            .inbox
            .get_mut(&self.label)
            .and_then(VecDeque::pop_front);
        match popped {
            Some((data, was_string)) => {
                self.last_was_string = was_string;
                Some(data)
            }
            None => None,
        }
    }

    /// `PacketPeer.get_available_packet_count`.
    pub fn get_available_packet_count(&self) -> usize {
        lock(&self.mailbox)
            .inbox
            .get(&self.label)
            .map_or(0, VecDeque::len)
    }
}

// ---------------------------------------------------------------------------
// WebRTCMultiplayerPeer
// ---------------------------------------------------------------------------

/// The topology a [`WebRTCMultiplayerPeer`] was initialized into.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetworkMode {
    None,
    Server,
    Client,
    Mesh,
}

/// Godot's reserved channel layout (`webrtc_multiplayer_peer.cpp`): one reliable,
/// one unreliable-ordered and one unreliable channel per peer.
#[derive(Debug)]
struct ConnectedPeer {
    connection: WebRTCPeerConnection,
    reliable: WebRTCDataChannel,
    ordered: WebRTCDataChannel,
    unreliable: WebRTCDataChannel,
}

impl ConnectedPeer {
    fn channel_for(&mut self, mode: TransferMode) -> &mut WebRTCDataChannel {
        match mode {
            TransferMode::Reliable => &mut self.reliable,
            TransferMode::Ordered => &mut self.ordered,
            TransferMode::Unreliable => &mut self.unreliable,
        }
    }
}

/// A `MultiplayerPeer` built over WebRTC data channels.
#[derive(Debug)]
pub struct WebRTCMultiplayerPeer {
    mode: NetworkMode,
    unique_id: PeerId,
    peers: HashMap<u32, ConnectedPeer>,
    refuse_new_connections: bool,
}

impl Default for WebRTCMultiplayerPeer {
    fn default() -> Self {
        Self::new()
    }
}

impl WebRTCMultiplayerPeer {
    pub fn new() -> Self {
        Self {
            mode: NetworkMode::None,
            unique_id: PeerId(0),
            peers: HashMap::new(),
            refuse_new_connections: false,
        }
    }

    /// `create_server()` — this peer is id 1 and every other peer connects to it.
    pub fn create_server(&mut self) -> Result<()> {
        self.initialize(1, NetworkMode::Server)
    }

    /// `create_client(self_id)`.
    ///
    /// ⚠️ Godot refuses id 1 for a client (`webrtc_multiplayer_peer.cpp:195`) —
    /// that id belongs to the server, and allowing it produces two peers that
    /// both believe they are authoritative.
    pub fn create_client(&mut self, self_id: u32) -> Result<()> {
        if self_id == 1 {
            return Err(NetworkError::Transport("clients cannot have ID 1".into()));
        }
        self.initialize(self_id, NetworkMode::Client)
    }

    /// `create_mesh(self_id)` — every peer connects to every other.
    pub fn create_mesh(&mut self, self_id: u32) -> Result<()> {
        self.initialize(self_id, NetworkMode::Mesh)
    }

    fn initialize(&mut self, self_id: u32, mode: NetworkMode) -> Result<()> {
        if self_id < 1 {
            return Err(NetworkError::Transport(
                "peer ids start at 1 (0 means broadcast)".into(),
            ));
        }
        self.mode = mode;
        self.unique_id = PeerId(self_id);
        self.peers.clear();
        Ok(())
    }

    /// Adds a peer, opening the three reserved channels on its connection.
    ///
    /// The connection must still be `New`, matching Godot
    /// (`webrtc_multiplayer_peer.cpp:297`): channels created after negotiation
    /// would not appear in the SDP the remote already has.
    pub fn add_peer(&mut self, mut connection: WebRTCPeerConnection, peer_id: u32) -> Result<()> {
        if self.mode == NetworkMode::None {
            return Err(NetworkError::Transport(
                "create_server/create_client/create_mesh must run first".into(),
            ));
        }
        if self.mode == NetworkMode::Client && peer_id != 1 {
            return Err(NetworkError::Transport(
                "a client may only add the server (id 1)".into(),
            ));
        }
        if self.mode == NetworkMode::Server && peer_id == 1 {
            return Err(NetworkError::Transport(
                "the server cannot add itself as a peer".into(),
            ));
        }
        if peer_id < 1 {
            return Err(NetworkError::Transport("peer ids start at 1".into()));
        }
        if self.refuse_new_connections {
            return Err(NetworkError::Transport("refusing new connections".into()));
        }
        if connection.get_connection_state() != ConnectionState::New {
            return Err(NetworkError::Transport(
                "add_peer needs a connection that has not negotiated yet".into(),
            ));
        }

        // Godot's reserved layout: negotiated channels with fixed ids, so both
        // ends agree without another round of signalling.
        let reliable = connection.create_data_channel(
            "reliable",
            DataChannelConfig {
                ordered: true,
                negotiated: true,
                id: Some(1),
                ..DataChannelConfig::default()
            },
        )?;
        let ordered = connection.create_data_channel(
            "ordered",
            DataChannelConfig {
                ordered: true,
                negotiated: true,
                id: Some(2),
                max_packet_life_time: Some(1),
                ..DataChannelConfig::default()
            },
        )?;
        let unreliable = connection.create_data_channel(
            "unreliable",
            DataChannelConfig {
                ordered: false,
                negotiated: true,
                id: Some(3),
                max_packet_life_time: Some(1),
                ..DataChannelConfig::default()
            },
        )?;

        self.peers.insert(
            peer_id,
            ConnectedPeer {
                connection,
                reliable,
                ordered,
                unreliable,
            },
        );
        Ok(())
    }

    /// The connection backing `peer_id`, so a signalling script can drive its
    /// offer/answer exchange after `add_peer`.
    pub fn peer_connection_mut(&mut self, peer_id: u32) -> Option<&mut WebRTCPeerConnection> {
        self.peers.get_mut(&peer_id).map(|p| &mut p.connection)
    }

    pub fn has_peer(&self, peer_id: u32) -> bool {
        self.peers.contains_key(&peer_id)
    }

    pub fn get_network_mode(&self) -> NetworkMode {
        self.mode
    }

    pub fn set_refuse_new_connections(&mut self, refuse: bool) {
        self.refuse_new_connections = refuse;
    }
}

impl NetworkPeer for WebRTCMultiplayerPeer {
    /// `PeerId(0)` broadcasts, matching Godot's `set_target_peer(0)`.
    fn send_packet(&mut self, to: PeerId, packet: Packet) -> Result<()> {
        if to.0 == 0 {
            for peer in self.peers.values_mut() {
                peer.channel_for(packet.transfer_mode)
                    .put_packet(packet.data.clone())?;
            }
            return Ok(());
        }
        let Some(peer) = self.peers.get_mut(&to.0) else {
            return Err(NetworkError::PeerUnreachable(to));
        };
        peer.channel_for(packet.transfer_mode)
            .put_packet(packet.data)
    }

    fn poll(&mut self) -> Vec<Packet> {
        let mut out = Vec::new();
        for (id, peer) in self.peers.iter_mut() {
            let _ = peer.connection.poll();
            for (channel, mode) in [
                (0u8, TransferMode::Reliable),
                (1, TransferMode::Ordered),
                (2, TransferMode::Unreliable),
            ] {
                while let Some(data) = peer.channel_for(mode).get_packet() {
                    out.push(Packet {
                        sender: PeerId(*id),
                        channel,
                        data,
                        transfer_mode: mode,
                    });
                }
            }
        }
        out
    }

    fn get_unique_id(&self) -> PeerId {
        self.unique_id
    }

    fn is_server(&self) -> bool {
        self.unique_id == PeerId(1)
    }

    fn get_connection_status(&self) -> ConnectionStatus {
        match self.mode {
            NetworkMode::None => ConnectionStatus::Disconnected,
            // A server or mesh peer is live as soon as it is initialized; it is
            // hosting whether or not anyone has arrived.
            NetworkMode::Server | NetworkMode::Mesh => ConnectionStatus::Connected,
            NetworkMode::Client => match self.peers.get(&1) {
                Some(server)
                    if server.connection.get_connection_state() == ConnectionState::Connected =>
                {
                    ConnectionStatus::Connected
                }
                _ => ConnectionStatus::Connecting,
            },
        }
    }

    fn close(&mut self) {
        for peer in self.peers.values_mut() {
            peer.connection.close();
        }
        self.peers.clear();
        self.mode = NetworkMode::None;
        self.unique_id = PeerId(0);
    }
}

// ---------------------------------------------------------------------------
// Script-visible handles
// ---------------------------------------------------------------------------

/// Handle tables for the two script-facing WebRTC types (pat-s3rrv.80).
///
/// ⚠️ THE TAG BYTE IS A WORKSPACE-WIDE NAMESPACE. Two modules issuing the same
/// top byte make `is_*_handle` answer true for the other module's ids, and the
/// symptom is a `MethodNotFound` naming the WRONG class. `0x5D` and `0x5E` were
/// chosen after sweeping all 37 `*_HANDLE_TAG` constants in the workspace — 37
/// distinct bytes, no existing collision — and sit beside `PacketPeerStream`
/// (`0x5C`) and `StreamPeer` (`0x5B`).
const WEBRTC_CONNECTION_HANDLE_TAG: u64 = 0x5D00_0000_0000_0000;
/// Companion tag for data channels; see [`WEBRTC_CONNECTION_HANDLE_TAG`].
const WEBRTC_CHANNEL_HANDLE_TAG: u64 = 0x5E00_0000_0000_0000;

static NEXT_WEBRTC_HANDLE: AtomicU64 = AtomicU64::new(1);
static WEBRTC_CONNECTIONS: Mutex<Option<HashMap<u64, WebRTCPeerConnection>>> = Mutex::new(None);
static WEBRTC_CHANNELS: Mutex<Option<HashMap<u64, WebRTCDataChannel>>> = Mutex::new(None);

/// A poisoned lock still holds a usable table: a panic in one script's handler
/// must not make every later call fail.
fn connection_handles() -> MutexGuard<'static, Option<HashMap<u64, WebRTCPeerConnection>>> {
    lock(&WEBRTC_CONNECTIONS)
}

fn channel_handles() -> MutexGuard<'static, Option<HashMap<u64, WebRTCDataChannel>>> {
    lock(&WEBRTC_CHANNELS)
}

/// Returns `true` when `id` was issued by [`new_webrtc_connection_handle`].
pub fn is_webrtc_connection_handle(id: gdcore::id::ObjectId) -> bool {
    id.raw() & 0xFF00_0000_0000_0000 == WEBRTC_CONNECTION_HANDLE_TAG
}

/// Returns `true` when `id` was issued by [`new_webrtc_channel_handle`].
pub fn is_webrtc_channel_handle(id: gdcore::id::ObjectId) -> bool {
    id.raw() & 0xFF00_0000_0000_0000 == WEBRTC_CHANNEL_HANDLE_TAG
}

/// Allocates a connection and returns its script-visible id
/// (`WebRTCPeerConnection.new()`).
pub fn new_webrtc_connection_handle() -> gdcore::id::ObjectId {
    let raw = WEBRTC_CONNECTION_HANDLE_TAG | NEXT_WEBRTC_HANDLE.fetch_add(1, Ordering::Relaxed);
    connection_handles()
        .get_or_insert_with(HashMap::new)
        .insert(raw, WebRTCPeerConnection::new());
    gdcore::id::ObjectId::from_raw(raw)
}

/// Files an already-built channel and returns its script-visible id.
///
/// ⚠️ Unlike the other handle constructors this takes the value: a channel is
/// only ever produced by `create_data_channel` on a connection, so there is no
/// meaningful `WebRTCDataChannel.new()` for a script to call — which is also why
/// Godot exposes the class without a usable constructor.
pub fn new_webrtc_channel_handle(channel: WebRTCDataChannel) -> gdcore::id::ObjectId {
    let raw = WEBRTC_CHANNEL_HANDLE_TAG | NEXT_WEBRTC_HANDLE.fetch_add(1, Ordering::Relaxed);
    channel_handles()
        .get_or_insert_with(HashMap::new)
        .insert(raw, channel);
    gdcore::id::ObjectId::from_raw(raw)
}

/// Runs `f` against the live connection behind `id`, or `None` when the handle
/// was never issued or has been released.
pub fn with_webrtc_connection<R>(
    id: gdcore::id::ObjectId,
    f: impl FnOnce(&mut WebRTCPeerConnection) -> R,
) -> Option<R> {
    let mut guard = connection_handles();
    let map = guard.get_or_insert_with(HashMap::new);
    map.get_mut(&id.raw()).map(f)
}

/// Runs `f` against the live data channel behind `id`.
pub fn with_webrtc_channel<R>(
    id: gdcore::id::ObjectId,
    f: impl FnOnce(&mut WebRTCDataChannel) -> R,
) -> Option<R> {
    let mut guard = channel_handles();
    let map = guard.get_or_insert_with(HashMap::new);
    map.get_mut(&id.raw()).map(f)
}

/// Drops a connection handle. Idempotent.
pub fn release_webrtc_connection_handle(id: gdcore::id::ObjectId) {
    if let Some(map) = connection_handles().as_mut() {
        map.remove(&id.raw());
    }
}

/// Drops a channel handle. Idempotent.
pub fn release_webrtc_channel_handle(id: gdcore::id::ObjectId) {
    if let Some(map) = channel_handles().as_mut() {
        map.remove(&id.raw());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The SDP is generated per connection, which is what the old hardcoded
    /// literal could not be: two connections must not share ICE credentials.
    #[test]
    fn each_connection_writes_its_own_sdp() {
        let a = WebRTCPeerConnection::new();
        let b = WebRTCPeerConnection::new();

        let a_sdp = a.write_sdp("offer");
        let b_sdp = b.write_sdp("offer");
        assert_ne!(
            sdp_attribute(&a_sdp, "ice-ufrag"),
            sdp_attribute(&b_sdp, "ice-ufrag"),
            "two connections sharing an ufrag would resolve to each other's \
             mailbox",
        );
        assert_eq!(
            sdp_attribute(&a_sdp, "ice-ufrag").as_deref(),
            Some(a.ice_ufrag()),
            "the SDP must advertise the ufrag the registry knows this \
             connection by, or no remote can ever resolve it",
        );
        assert_eq!(sdp_attribute(&a_sdp, "setup").as_deref(), Some("actpass"));
        assert_eq!(
            sdp_attribute(&a.write_sdp("answer"), "setup").as_deref(),
            Some("active"),
            "RFC 5763: an answer picks a role rather than offering both",
        );
    }

    /// A description naming nobody must fail rather than wait forever.
    #[test]
    fn an_unresolvable_remote_description_fails() {
        let mut a = WebRTCPeerConnection::new();
        let err = a.set_remote_description("offer", "v=0\r\na=ice-ufrag:nobody\r\n");
        assert!(err.is_err(), "an unknown ufrag must not look connectable");
        assert_eq!(a.get_connection_state(), ConnectionState::Failed);
    }
}
