//! WebSocketPeer — client-side RFC 6455 WebSocket over the platform TCP stack.
//!
//! Mirrors Godot's `WebSocketPeer` (modules/websocket/*): `connect_to_url`,
//! `send`/`send_text`, `get_available_packet_count`/`get_packet`, `poll`, a
//! `CONNECTING → OPEN → CLOSING → CLOSED` ready-state lifecycle, and close
//! codes/reasons matching Godot's API.
//!
//! The RFC 6455 handshake, frame masking, and the text/binary/close/ping/pong
//! codec are implemented from scratch (including a tiny SHA-1 + base64) so the
//! library pulls in **no** new runtime dependencies. On native targets only
//! plaintext `ws://` is wired; `wss://` reports
//! [`WebSocketError::TlsUnsupported`].
//!
//! On `wasm32` the socket comes from the browser instead
//! ([`crate::websocket_web`], pat-s3rrv.264), which is how Godot's Web export
//! works: `emws_peer.cpp` hands the URL to the browser's `WebSocket`, so the
//! browser owns the handshake, the framing and the TLS. **Both `ws://` and
//! `wss://` therefore work on wasm32** even though the codec below has no TLS
//! stack. The branch is taken inside [`WebSocketPeer::browser_connect`]; every
//! public method keeps a single definition, and the five `browser_*` helpers
//! below are the only places the two transports differ.

use std::collections::VecDeque;
use std::io::{ErrorKind, Read, Write};
use std::net::{Shutdown, TcpStream, ToSocketAddrs};
use std::time::Duration;

// `std::time::Instant::now()` panics on `wasm32-unknown-unknown` — and
// `WebSocketPeer::new()` calls it, so the browser backend could not construct a
// peer at all with the std clock. `web_time::Instant` re-exports
// `std::time::Instant` on native (identical behavior and cost) and is backed by
// `performance.now()` on wasm32; `os.rs` uses it for the same reason
// (pat-u3l8c.109).
use web_time::Instant;

use thiserror::Error;

use crate::http_client::parse_url;

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Errors raised by [`WebSocketPeer`] operations.
#[derive(Debug, Error)]
pub enum WebSocketError {
    /// The URL was malformed or used an unsupported scheme.
    #[error("invalid or unsupported URL: {0}")]
    InvalidUrl(String),
    /// `wss://` was requested but no TLS backend is wired in.
    #[error("TLS (wss://) is not supported")]
    TlsUnsupported,
    /// The TCP connection could not be established.
    #[error("could not connect: {0}")]
    Connect(String),
    /// No active connection.
    #[error("not connected")]
    NotConnected,
    /// The peer is not in the OPEN state.
    #[error("peer is not open")]
    NotOpen,
    /// A protocol violation was detected.
    #[error("protocol error: {0}")]
    Protocol(String),
    /// An underlying IO error.
    #[error("io error: {0}")]
    Io(String),
    /// A message does not fit the configured buffer — Godot's
    /// `ERR_OUT_OF_MEMORY` from `put_packet` (`wsl_peer.cpp:784`).
    #[error("message of {size} bytes exceeds outbound_buffer_size {limit}")]
    BufferFull { size: usize, limit: usize },
}

// ---------------------------------------------------------------------------
// Ready state / write mode (Godot-matching enums)
// ---------------------------------------------------------------------------

/// Ready-state lifecycle, mirroring Godot's `WebSocketPeer.State`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WebSocketState {
    /// Socket created / handshake in progress.
    Connecting,
    /// Handshake complete; messages may flow.
    Open,
    /// A close handshake is underway.
    Closing,
    /// Fully closed.
    Closed,
}

impl WebSocketState {
    /// Returns the integer matching Godot's `WebSocketPeer.State` enum.
    pub fn as_godot_code(&self) -> i32 {
        match self {
            WebSocketState::Connecting => 0,
            WebSocketState::Open => 1,
            WebSocketState::Closing => 2,
            WebSocketState::Closed => 3,
        }
    }
}

/// Outbound message framing mode, mirroring Godot's `WebSocketPeer.WriteMode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteMode {
    /// Send as a UTF-8 text frame.
    Text,
    /// Send as a binary frame.
    Binary,
}

impl WriteMode {
    /// Returns the integer matching Godot's `WebSocketPeer.WriteMode` enum.
    pub fn as_godot_code(&self) -> i32 {
        match self {
            WriteMode::Text => 0,
            WriteMode::Binary => 1,
        }
    }
}

// ---------------------------------------------------------------------------
// Frame codec (public so a test/echo server can reuse it)
// ---------------------------------------------------------------------------

/// A WebSocket frame opcode (RFC 6455 §5.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Opcode {
    /// Continuation of a fragmented message.
    Continuation,
    /// UTF-8 text payload.
    Text,
    /// Binary payload.
    Binary,
    /// Connection close control frame.
    Close,
    /// Ping control frame.
    Ping,
    /// Pong control frame.
    Pong,
}

impl Opcode {
    fn from_u8(value: u8) -> Option<Opcode> {
        match value {
            0x0 => Some(Opcode::Continuation),
            0x1 => Some(Opcode::Text),
            0x2 => Some(Opcode::Binary),
            0x8 => Some(Opcode::Close),
            0x9 => Some(Opcode::Ping),
            0xA => Some(Opcode::Pong),
            _ => None,
        }
    }

    fn to_u8(self) -> u8 {
        match self {
            Opcode::Continuation => 0x0,
            Opcode::Text => 0x1,
            Opcode::Binary => 0x2,
            Opcode::Close => 0x8,
            Opcode::Ping => 0x9,
            Opcode::Pong => 0xA,
        }
    }

    /// `true` for control opcodes (close/ping/pong).
    pub fn is_control(self) -> bool {
        matches!(self, Opcode::Close | Opcode::Ping | Opcode::Pong)
    }
}

/// A single decoded/encodable WebSocket frame.
#[derive(Debug, Clone)]
pub struct Frame {
    /// Whether this is the final fragment of a message.
    pub fin: bool,
    /// The frame opcode.
    pub opcode: Opcode,
    /// The (unmasked) payload bytes.
    pub payload: Vec<u8>,
}

impl Frame {
    /// Builds a final (unfragmented) frame.
    pub fn new(opcode: Opcode, payload: Vec<u8>) -> Self {
        Self {
            fin: true,
            opcode,
            payload,
        }
    }
}

/// Encodes a frame to bytes. Pass `Some(mask)` to mask the payload (required
/// for client → server frames); pass `None` for server → client frames.
pub fn encode_frame(frame: &Frame, mask: Option<[u8; 4]>) -> Vec<u8> {
    let mut out = Vec::with_capacity(frame.payload.len() + 14);
    let fin_bit = if frame.fin { 0x80 } else { 0x00 };
    out.push(fin_bit | frame.opcode.to_u8());

    let mask_bit = if mask.is_some() { 0x80 } else { 0x00 };
    let len = frame.payload.len();
    if len < 126 {
        out.push(mask_bit | len as u8);
    } else if len <= u16::MAX as usize {
        out.push(mask_bit | 126);
        out.extend_from_slice(&(len as u16).to_be_bytes());
    } else {
        out.push(mask_bit | 127);
        out.extend_from_slice(&(len as u64).to_be_bytes());
    }

    match mask {
        Some(key) => {
            out.extend_from_slice(&key);
            for (i, byte) in frame.payload.iter().enumerate() {
                out.push(byte ^ key[i % 4]);
            }
        }
        None => out.extend_from_slice(&frame.payload),
    }
    out
}

/// Attempts to decode one frame from the front of `buf`.
///
/// Returns `Ok(Some((frame, consumed)))` when a full frame is available,
/// `Ok(None)` when more bytes are needed, or `Err` on a protocol violation.
pub fn decode_frame(buf: &[u8]) -> Result<Option<(Frame, usize)>, WebSocketError> {
    if buf.len() < 2 {
        return Ok(None);
    }
    let b0 = buf[0];
    let b1 = buf[1];
    let fin = b0 & 0x80 != 0;
    let opcode = Opcode::from_u8(b0 & 0x0F)
        .ok_or_else(|| WebSocketError::Protocol(format!("bad opcode {:#x}", b0 & 0x0F)))?;
    let masked = b1 & 0x80 != 0;
    let mut len = (b1 & 0x7F) as usize;
    let mut idx = 2;

    if len == 126 {
        if buf.len() < idx + 2 {
            return Ok(None);
        }
        len = u16::from_be_bytes([buf[idx], buf[idx + 1]]) as usize;
        idx += 2;
    } else if len == 127 {
        if buf.len() < idx + 8 {
            return Ok(None);
        }
        let mut octets = [0u8; 8];
        octets.copy_from_slice(&buf[idx..idx + 8]);
        len = u64::from_be_bytes(octets) as usize;
        idx += 8;
    }

    let mask_key = if masked {
        if buf.len() < idx + 4 {
            return Ok(None);
        }
        let key = [buf[idx], buf[idx + 1], buf[idx + 2], buf[idx + 3]];
        idx += 4;
        Some(key)
    } else {
        None
    };

    if buf.len() < idx + len {
        return Ok(None);
    }
    let mut payload = buf[idx..idx + len].to_vec();
    if let Some(key) = mask_key {
        for (i, byte) in payload.iter_mut().enumerate() {
            *byte ^= key[i % 4];
        }
    }
    idx += len;

    Ok(Some((
        Frame {
            fin,
            opcode,
            payload,
        },
        idx,
    )))
}

// ---------------------------------------------------------------------------
// Handshake helpers (public so a test/echo server can reuse them)
// ---------------------------------------------------------------------------

/// The RFC 6455 magic GUID appended to the client key when computing accept.
const WS_GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";

/// Computes the `Sec-WebSocket-Accept` value for a given client key.
pub fn compute_accept_key(client_key: &str) -> String {
    let mut input = client_key.as_bytes().to_vec();
    input.extend_from_slice(WS_GUID.as_bytes());
    base64_encode(&sha1(&input))
}

/// Extracts the `Sec-WebSocket-Key` header value from a raw handshake request.
pub fn parse_handshake_key(request: &str) -> Option<String> {
    for line in request.split("\r\n") {
        if let Some((k, v)) = line.split_once(':') {
            if k.trim().eq_ignore_ascii_case("sec-websocket-key") {
                return Some(v.trim().to_string());
            }
        }
    }
    None
}

/// Builds the full HTTP 101 handshake response for a given client key.
pub fn websocket_handshake_response(client_key: &str) -> String {
    websocket_handshake_response_with_protocol(client_key, None)
}

/// [`websocket_handshake_response`] plus an accepted subprotocol echoed back in
/// `Sec-WebSocket-Protocol` (pat-0q2kx.96).
///
/// RFC 6455 §4.2.2: a server that accepts a subprotocol must name exactly one,
/// and must omit the header entirely when it accepts none — sending an empty
/// header value is a protocol violation, which is why this takes an `Option`
/// rather than a string that might be blank.
pub fn websocket_handshake_response_with_protocol(
    client_key: &str,
    selected_protocol: Option<&str>,
) -> String {
    let accept = compute_accept_key(client_key);
    let protocol_line = match selected_protocol {
        Some(p) if !p.is_empty() => format!("Sec-WebSocket-Protocol: {p}\r\n"),
        _ => String::new(),
    };
    format!(
        "HTTP/1.1 101 Switching Protocols\r\n\
         Upgrade: websocket\r\n\
         Connection: Upgrade\r\n\
         Sec-WebSocket-Accept: {accept}\r\n\
         {protocol_line}\r\n"
    )
}

/// The subprotocols a client offered in its `Sec-WebSocket-Protocol` request
/// header, in preference order (pat-0q2kx.96).
///
/// Returns empty when the header is absent. Values are comma-separated with
/// optional whitespace, per RFC 6455 §4.1.
pub fn parse_handshake_protocols(request: &str) -> Vec<String> {
    for line in request.split("\r\n") {
        if let Some((k, v)) = line.split_once(':') {
            if k.trim().eq_ignore_ascii_case("sec-websocket-protocol") {
                return v
                    .split(',')
                    .map(|p| p.trim().to_string())
                    .filter(|p| !p.is_empty())
                    .collect();
            }
        }
    }
    Vec::new()
}

/// Picks the first client-offered protocol the server also supports, or `None`
/// when they share none — RFC 6455 gives the server the choice, and Godot's
/// server side follows the client's preference order.
pub fn select_handshake_protocol(offered: &[String], supported: &[String]) -> Option<String> {
    offered
        .iter()
        .find(|p| supported.iter().any(|s| s == *p))
        .cloned()
}

// ---------------------------------------------------------------------------
// WebSocketPeer
// ---------------------------------------------------------------------------

/// A client-side WebSocket peer mirroring Godot's `WebSocketPeer`.
#[derive(Debug)]
pub struct WebSocketPeer {
    stream: Option<TcpStream>,
    state: WebSocketState,
    in_buf: Vec<u8>,
    packets: VecDeque<(Vec<u8>, bool)>,
    last_was_string: bool,
    sent_key: String,
    requested_url: String,
    close_code: i32,
    close_reason: String,
    close_frame_sent: bool,
    frag_opcode: Option<Opcode>,
    frag_payload: Vec<u8>,
    connect_timeout: Duration,
    /// Subprotocols offered in `Sec-WebSocket-Protocol` on the opening
    /// handshake (Godot `WebSocketPeer.supported_protocols`, pat-0q2kx.96).
    supported_protocols: Vec<String>,
    /// The one the server picked, echoed back in its 101 response. Empty when
    /// none was offered or the server declined to choose.
    selected_protocol: String,
    /// Extra `Key: Value` lines added to the opening handshake (Godot
    /// `WebSocketPeer.handshake_headers`).
    handshake_headers: Vec<String>,
    /// How often to send an unsolicited Ping; `ZERO` disables it (Godot
    /// `WebSocketPeer.heartbeat_interval`, default `0.0`).
    heartbeat_interval: Duration,
    /// When the outstanding Ping went out.
    last_heartbeat: Instant,
    /// A Ping is outstanding and no Pong has answered it.
    ///
    /// ⚠️ This flag is what makes the heartbeat a **liveness timeout** rather
    /// than a keepalive: if the next interval elapses while it is still set,
    /// the connection is dropped. See [`poll`](Self::poll).
    heartbeat_waiting: bool,
    /// Cap on queued inbound packets (Godot
    /// `WebSocketPeer.max_queued_packets`, default `4096`).
    max_queued_packets: usize,
    /// Godot `WebSocketPeer.inbound_buffer_size` (default `65535`): the
    /// longest message accepted (wslay's `max_recv_msg_length` — a longer one
    /// fails the connection with 1009 MESSAGE_TOO_BIG), and, rounded up to a
    /// power of two, the byte budget of the inbound packet queue
    /// (`in_buffer.resize(nearest_shift(size), ..)`, `wsl_peer.cpp:299-300`).
    inbound_buffer_size: usize,
    /// Godot `WebSocketPeer.outbound_buffer_size` (default `65535`): `send`
    /// refuses a message once queued-plus-new bytes would exceed it
    /// (`wsl_peer.cpp:784`). `send` writes straight to the socket and queues
    /// nothing, so the queued part is zero and the bound is per message.
    outbound_buffer_size: usize,
    /// Handle into the browser `WebSocket` registry in
    /// [`crate::websocket_web`], set instead of `stream` on wasm32.
    ///
    /// ⚠️ A `u64` rather than the socket itself because `js_sys::Closure` and
    /// `Rc` are neither `Send` nor `Sync`, and this struct lives inside the
    /// `static Mutex<HashMap<u64, WebSocketPeer>>` handle table further down
    /// this file — which requires `Send`. See the `websocket_web` module docs.
    #[cfg(target_arch = "wasm32")]
    browser: Option<u64>,
}

impl Default for WebSocketPeer {
    fn default() -> Self {
        Self::new()
    }
}

/// Releases the browser socket when the peer goes away (pat-s3rrv.264).
///
/// ⚠️ wasm32 only, and not optional there: the registry in
/// [`crate::websocket_web`] owns the JS `WebSocket` *and* the event closures JS
/// holds as its handlers, so a peer dropped without `close()` — a script
/// letting the `Ref` go, a `WebSocketMultiplayerPeer` replacing its client —
/// would strand both for the lifetime of the page. The browser cannot collect
/// them, because from JS's side the handlers are still live references.
#[cfg(target_arch = "wasm32")]
impl Drop for WebSocketPeer {
    fn drop(&mut self) {
        self.release_browser();
    }
}

impl WebSocketPeer {
    /// Creates a new, closed peer.
    pub fn new() -> Self {
        Self {
            stream: None,
            state: WebSocketState::Closed,
            in_buf: Vec::new(),
            packets: VecDeque::new(),
            last_was_string: false,
            sent_key: String::new(),
            requested_url: String::new(),
            close_code: -1,
            close_reason: String::new(),
            close_frame_sent: false,
            frag_opcode: None,
            frag_payload: Vec::new(),
            connect_timeout: Duration::from_secs(30),
            supported_protocols: Vec::new(),
            selected_protocol: String::new(),
            handshake_headers: Vec::new(),
            // Godot's documented defaults: no heartbeat, 4096 queued packets.
            heartbeat_interval: Duration::ZERO,
            last_heartbeat: Instant::now(),
            heartbeat_waiting: false,
            max_queued_packets: 4096,
            inbound_buffer_size: 65535,
            outbound_buffer_size: 65535,
            #[cfg(target_arch = "wasm32")]
            browser: None,
        }
    }

    /// Sets the connection-establishment timeout.
    pub fn set_connect_timeout(&mut self, timeout: Duration) {
        self.connect_timeout = timeout;
    }

    /// Sets the subprotocols offered on the next handshake (Godot
    /// `WebSocketPeer.supported_protocols`, pat-0q2kx.96).
    ///
    /// Must be set *before* [`connect_to_url`](Self::connect_to_url) — the list
    /// is serialized into that request's `Sec-WebSocket-Protocol` header, as in
    /// Godot, where the property is read at connect time.
    pub fn set_supported_protocols(&mut self, protocols: &[String]) {
        self.supported_protocols = protocols.to_vec();
    }

    /// The subprotocols this peer offers.
    pub fn get_supported_protocols(&self) -> &[String] {
        &self.supported_protocols
    }

    /// The subprotocol the server selected, or `""` when none was negotiated
    /// (Godot `WebSocketPeer.get_selected_protocol`).
    ///
    /// Only meaningful once the peer reaches [`WebSocketState::Open`]; the value
    /// comes from the server's `Sec-WebSocket-Protocol` response header.
    pub fn get_selected_protocol(&self) -> &str {
        &self.selected_protocol
    }

    /// Sets extra header lines sent with the opening handshake, each `"Key: Value"`
    /// (Godot `WebSocketPeer.handshake_headers`).
    pub fn set_handshake_headers(&mut self, headers: &[String]) {
        self.handshake_headers = headers.to_vec();
    }

    /// The extra handshake headers configured on this peer.
    /// Godot `WebSocketPeer.heartbeat_interval`, in seconds; `0` disables.
    ///
    /// ⚠️ Documented as having **no effect in Web exports** — a browser
    /// `WebSocket` sends its own pings and gives the page no control. A wasm
    /// backend must accept this setting and ignore it, not emulate it.
    pub fn set_heartbeat_interval(&mut self, seconds: f64) {
        self.heartbeat_interval = if seconds > 0.0 {
            Duration::from_secs_f64(seconds)
        } else {
            Duration::ZERO
        };
    }

    /// The configured ping interval in seconds; `0.0` when disabled.
    pub fn get_heartbeat_interval(&self) -> f64 {
        self.heartbeat_interval.as_secs_f64()
    }

    /// Godot `WebSocketPeer.max_queued_packets` (default 4096).
    ///
    /// ⚠️ A value below 1 is clamped to 1: a cap of zero would discard every
    /// packet the moment it arrived, which is indistinguishable from a dead
    /// connection.
    pub fn set_max_queued_packets(&mut self, max: i64) {
        self.max_queued_packets = max.max(1) as usize;
        // The browser's inbound queue is filled by an event handler, not by
        // `poll`, so it carries its own copy of the cap. Without this a peer
        // re-capped after connecting would keep buffering at the old limit
        // (pat-s3rrv.264).
        self.sync_browser_queue_cap();
    }

    /// The configured queue cap.
    pub fn get_max_queued_packets(&self) -> i64 {
        self.max_queued_packets as i64
    }

    /// Godot `WebSocketPeer.inbound_buffer_size` (default 65535). Negative
    /// values are refused, as Godot's `ERR_FAIL_COND(p_buffer_size < 0)`.
    pub fn set_inbound_buffer_size(&mut self, size: i64) {
        if size >= 0 {
            self.inbound_buffer_size = size as usize;
        }
    }

    pub fn get_inbound_buffer_size(&self) -> i64 {
        self.inbound_buffer_size as i64
    }

    /// Godot `WebSocketPeer.outbound_buffer_size` (default 65535); `0` means
    /// unbounded (`outbound_buffer_size > 0 && ...`, `wsl_peer.cpp:784`).
    pub fn set_outbound_buffer_size(&mut self, size: i64) {
        if size >= 0 {
            self.outbound_buffer_size = size as usize;
        }
    }

    pub fn get_outbound_buffer_size(&self) -> i64 {
        self.outbound_buffer_size as i64
    }

    /// The inbound queue's byte budget: `1 << nearest_shift(size)`, Godot's
    /// ring-buffer size for `inbound_buffer_size`.
    fn inbound_queue_capacity(&self) -> usize {
        let n = self.inbound_buffer_size as u32;
        if n == 0 {
            return 0;
        }
        // Godot's `nearest_shift`: one past the index of the highest set bit.
        let shift = 32 - n.leading_zeros();
        1usize << shift
    }

    fn queued_payload_bytes(&self) -> usize {
        self.packets.iter().map(|(p, _)| p.len()).sum()
    }

    pub fn get_handshake_headers(&self) -> &[String] {
        &self.handshake_headers
    }

    // -- browser transport seam (pat-s3rrv.264) -----------------------------
    //
    // Five shims, each with a wasm32 body and a native body that declines. They
    // exist as cfg'd *items* so every public method keeps ONE definition and
    // the native code path below stays live code on both targets — no
    // duplicated `connect_to_url`/`poll`/`close` to drift apart, and no
    // `unreachable_code` or `dead_code` warnings on either target.

    /// Opens the browser socket, or `None` on a build that has no browser.
    #[cfg(target_arch = "wasm32")]
    fn browser_connect(&mut self, url: &str) -> Option<Result<(), WebSocketError>> {
        // A reconnect must not leave the previous socket (and its JS event
        // closures) behind in the registry.
        self.release_browser();
        let handle = match crate::websocket_web::connect(
            url,
            &self.supported_protocols,
            self.max_queued_packets,
        ) {
            Ok(handle) => handle,
            Err(e) => return Some(Err(WebSocketError::Connect(e))),
        };
        self.browser = Some(handle);
        self.requested_url = url.to_string();
        self.state = WebSocketState::Connecting;
        // The browser performs the handshake, so there is no key to verify and
        // no inbound byte stream to buffer; the rest is the same per-connection
        // reset the TCP path does.
        self.sent_key.clear();
        self.in_buf.clear();
        self.packets.clear();
        self.last_was_string = false;
        self.close_frame_sent = false;
        self.close_code = -1;
        self.close_reason.clear();
        self.frag_opcode = None;
        self.frag_payload.clear();
        self.selected_protocol.clear();
        self.last_heartbeat = Instant::now();
        self.heartbeat_waiting = false;
        Some(Ok(()))
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn browser_connect(&mut self, _url: &str) -> Option<Result<(), WebSocketError>> {
        None
    }

    /// Drains the browser socket into the shared packet queue and republishes
    /// its `readyState`. Returns `false` when there is no browser socket.
    #[cfg(target_arch = "wasm32")]
    fn browser_poll(&mut self) -> bool {
        let Some(handle) = self.browser else {
            return false;
        };
        let Some(snapshot) = crate::websocket_web::poll(handle) else {
            // The registry entry is gone (released out from under us). Falling
            // through to the TCP path would find no socket either, so report
            // the peer closed rather than silently idle.
            self.browser = None;
            self.state = WebSocketState::Closed;
            return true;
        };
        // Queued first, and unconditionally: a close in the same tick must not
        // discard messages the browser had already delivered.
        for (payload, was_string) in snapshot.packets {
            self.queue_packet(payload, was_string);
        }
        if snapshot.state == WebSocketState::Open && self.state != WebSocketState::Open {
            // `socket.protocol` is only meaningful once the handshake has
            // completed, so it is read on the transition into OPEN.
            self.selected_protocol = crate::websocket_web::selected_protocol(handle);
        }
        self.state = snapshot.state;
        if snapshot.state == WebSocketState::Closed {
            // 1006 — "abnormal closure" — is what the native path reports for a
            // connection that died without a close frame, and it is what the
            // browser leaves us with when no `CloseEvent` carried a code.
            self.close_code = if snapshot.close_code >= 0 {
                snapshot.close_code
            } else {
                1006
            };
            self.close_reason = snapshot.close_reason;
            self.release_browser();
        }
        true
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn browser_poll(&mut self) -> bool {
        false
    }

    /// Sends one message through the browser, or `None` with no browser socket.
    #[cfg(target_arch = "wasm32")]
    fn browser_send(&mut self, data: &[u8], text: bool) -> Option<Result<(), WebSocketError>> {
        let handle = self.browser?;
        Some(crate::websocket_web::send(handle, data, text).map_err(WebSocketError::Io))
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn browser_send(&mut self, _data: &[u8], _text: bool) -> Option<Result<(), WebSocketError>> {
        None
    }

    /// Starts the browser's close handshake. Returns `false` with no socket.
    #[cfg(target_arch = "wasm32")]
    fn browser_close(&mut self, code: u16, reason: &str) -> bool {
        let Some(handle) = self.browser else {
            return false;
        };
        if matches!(
            self.state,
            WebSocketState::Open | WebSocketState::Connecting
        ) {
            let _ = crate::websocket_web::close(handle, code, reason);
            self.close_code = i32::from(code);
            self.close_reason = reason.to_string();
            self.close_frame_sent = true;
            // ⚠️ CLOSING, not CLOSED: the browser runs the close handshake and
            // only then fires `close`. [`browser_poll`] publishes the real
            // `readyState`, which is also what settles CLOSED and releases the
            // registry entry. Reporting CLOSED here would let a script believe
            // the socket was gone while the browser was still flushing.
            self.state = WebSocketState::Closing;
        }
        true
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn browser_close(&mut self, _code: u16, _reason: &str) -> bool {
        false
    }

    /// `WebSocket.bufferedAmount`, or `None` with no browser socket.
    #[cfg(target_arch = "wasm32")]
    fn browser_buffered_amount(&self) -> Option<i64> {
        self.browser.map(crate::websocket_web::buffered_amount)
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn browser_buffered_amount(&self) -> Option<i64> {
        None
    }

    /// Pushes `max_queued_packets` down to the browser's own inbound queue.
    #[cfg(target_arch = "wasm32")]
    fn sync_browser_queue_cap(&mut self) {
        if let Some(handle) = self.browser {
            crate::websocket_web::set_max_queued_packets(handle, self.max_queued_packets);
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn sync_browser_queue_cap(&mut self) {}

    /// Drops the browser socket and the JS closures it keeps alive.
    #[cfg(target_arch = "wasm32")]
    fn release_browser(&mut self) {
        if let Some(handle) = self.browser.take() {
            crate::websocket_web::release(handle);
        }
    }

    /// Connects to a `ws://` URL and begins the opening handshake.
    ///
    /// The peer enters [`WebSocketState::Connecting`]; call [`poll`](Self::poll)
    /// until it reaches [`WebSocketState::Open`].
    ///
    /// On wasm32 the browser supplies the socket, so `wss://` is accepted there
    /// and the TCP path below is never reached (pat-s3rrv.264).
    pub fn connect_to_url(&mut self, url: &str) -> Result<(), WebSocketError> {
        let parsed = parse_url(url).ok_or_else(|| WebSocketError::InvalidUrl(url.to_string()))?;
        if !matches!(parsed.scheme.as_str(), "ws" | "wss") {
            return Err(WebSocketError::InvalidUrl(format!(
                "scheme '{}' is not ws/wss",
                parsed.scheme
            )));
        }
        if let Some(result) = self.browser_connect(url) {
            return result;
        }
        if parsed.tls {
            return Err(WebSocketError::TlsUnsupported);
        }

        let addrs = (parsed.host.as_str(), parsed.port)
            .to_socket_addrs()
            .map_err(|e| WebSocketError::Connect(e.to_string()))?
            .collect::<Vec<_>>();
        let addr = addrs
            .first()
            .ok_or_else(|| WebSocketError::Connect("no address resolved".into()))?;
        let mut stream = TcpStream::connect_timeout(addr, self.connect_timeout)
            .map_err(|e| WebSocketError::Connect(e.to_string()))?;
        // ⚠️ Nagle OFF by default, matching Godot: `wsl_peer.cpp:146` calls
        // `tcp->set_no_delay(true)` on every connection, and the docs for
        // `set_no_delay` say so in passing — "(default)". Leaving Nagle on would
        // coalesce small frames and add up to 40 ms of latency to exactly the
        // chatty small-message traffic a game uses a WebSocket for, which reads
        // as server lag rather than as a socket option.
        let _ = stream.set_nodelay(true);
        // The heartbeat clock starts at the connection, not at construction: a
        // peer built long before it connects would otherwise be "overdue" on its
        // very first poll and fire a Ping immediately.
        self.last_heartbeat = Instant::now();
        self.heartbeat_waiting = false;

        let key = generate_key();
        let host_header = if parsed.port == 80 {
            parsed.host.clone()
        } else {
            format!("{}:{}", parsed.host, parsed.port)
        };
        // Offered subprotocols and any caller-supplied headers ride along on the
        // opening handshake, as they do in Godot (pat-0q2kx.96). Both are folded
        // in before the terminating blank line rather than appended after it —
        // an extra header past `\r\n\r\n` would land in the message stream.
        let mut extra = String::new();
        if !self.supported_protocols.is_empty() {
            extra.push_str(&format!(
                "Sec-WebSocket-Protocol: {}\r\n",
                self.supported_protocols.join(", ")
            ));
        }
        for header in &self.handshake_headers {
            let line = header.trim();
            if !line.is_empty() {
                extra.push_str(line);
                extra.push_str("\r\n");
            }
        }
        let request = format!(
            "GET {} HTTP/1.1\r\n\
             Host: {}\r\n\
             Upgrade: websocket\r\n\
             Connection: Upgrade\r\n\
             Sec-WebSocket-Key: {}\r\n\
             Sec-WebSocket-Version: 13\r\n\
             {}\r\n",
            parsed.path, host_header, key, extra
        );
        // Send the handshake while still blocking, then switch to non-blocking
        // so poll() never stalls the caller.
        stream
            .write_all(request.as_bytes())
            .map_err(|e| WebSocketError::Io(e.to_string()))?;
        let _ = stream.flush();
        stream
            .set_nonblocking(true)
            .map_err(|e| WebSocketError::Io(e.to_string()))?;

        self.stream = Some(stream);
        self.sent_key = key;
        self.requested_url = url.to_string();
        self.state = WebSocketState::Connecting;
        self.in_buf.clear();
        self.packets.clear();
        self.close_frame_sent = false;
        self.close_code = -1;
        self.close_reason.clear();
        self.frag_opcode = None;
        self.frag_payload.clear();
        // A reconnect renegotiates from scratch; the previous connection's
        // choice must not leak into the new one (pat-0q2kx.96).
        self.selected_protocol.clear();
        Ok(())
    }

    /// Drives the connection: completes the handshake, drains inbound frames
    /// into the packet queue, answers pings, and advances the close handshake.
    pub fn poll(&mut self) {
        if self.browser_poll() {
            return;
        }
        if self.stream.is_none() || self.state == WebSocketState::Closed {
            return;
        }

        let eof = match self.pump_reads() {
            Ok(eof) => eof,
            Err(_) => {
                self.transition_closed(1006, "");
                return;
            }
        };

        if self.state == WebSocketState::Connecting {
            if let Some(pos) = find_subsequence(&self.in_buf, b"\r\n\r\n") {
                let header_end = pos + 4;
                let head = String::from_utf8_lossy(&self.in_buf[..header_end]).to_string();
                self.in_buf.drain(..header_end);
                if self.verify_handshake(&head) {
                    self.state = WebSocketState::Open;
                } else {
                    self.transition_closed(1002, "handshake rejected");
                    return;
                }
            } else {
                if eof {
                    self.transition_closed(1006, "");
                }
                return;
            }
        }

        if matches!(self.state, WebSocketState::Open | WebSocketState::Closing) {
            loop {
                match decode_frame(&self.in_buf) {
                    Ok(Some((frame, consumed))) => {
                        // Backpressure, as Godot's recv callback (it reads only
                        // `payload_space_left()`, `wsl_peer.cpp:574`): a data
                        // frame the queue's byte budget cannot take stays in
                        // the socket buffer until the script drains packets.
                        // An over-long message is checked FIRST: wslay sees the
                        // length in the frame header, which fits in any space
                        // left, and fails the connection (handle_frame).
                        let is_data = matches!(
                            frame.opcode,
                            Opcode::Text | Opcode::Binary | Opcode::Continuation
                        );
                        let so_far = if frame.opcode == Opcode::Continuation {
                            self.frag_payload.len()
                        } else {
                            0
                        };
                        let too_long = so_far + frame.payload.len() > self.inbound_buffer_size;
                        if is_data
                            && !too_long
                            && !self.packets.is_empty()
                            && self.queued_payload_bytes() + frame.payload.len()
                                > self.inbound_queue_capacity()
                        {
                            break;
                        }
                        self.in_buf.drain(..consumed);
                        self.handle_frame(frame);
                        if self.state == WebSocketState::Closed {
                            break;
                        }
                    }
                    Ok(None) => break,
                    Err(_) => {
                        self.transition_closed(1002, "protocol error");
                        break;
                    }
                }
            }
        }

        if eof && self.state != WebSocketState::Closed {
            // Peer dropped the TCP connection without a close frame.
            self.transition_closed(1006, "");
        }

        // The heartbeat (Godot `heartbeat_interval`, `wsl_peer.cpp:715-731`).
        //
        // ⚠️ It is a LIVENESS TIMEOUT, not a keepalive. Sending a Ping every
        // interval is only half of it: if the interval elapses again while the
        // previous Ping is still unanswered, Godot tears the connection down
        // (`close(-1)` — abrupt, no close frame). An implementation that only
        // sends Pings keeps a dead connection open forever and looks correct
        // against every test that has a live server on the other end.
        //
        // Placed at the END of `poll` on purpose: a Pong that arrived in this
        // same tick has already cleared `heartbeat_waiting` in `handle_frame`,
        // so a peer that answered promptly is never mistaken for a dead one.
        if self.state == WebSocketState::Open
            && !self.heartbeat_interval.is_zero()
            && self.last_heartbeat.elapsed() > self.heartbeat_interval
        {
            if self.heartbeat_waiting {
                self.transition_closed(1006, "heartbeat timeout");
                return;
            }
            self.heartbeat_waiting = true;
            // `last_heartbeat` advances only on a successful write, matching
            // Godot, which sets it inside the `err == 0` branch — otherwise a
            // failing socket would silently reset the timeout every tick and
            // the timeout would never fire.
            if self.send_frame(Opcode::Ping, &[]).is_ok() {
                self.last_heartbeat = Instant::now();
            }
        }
    }

    /// Sends a payload as a text or binary message. Errors unless OPEN.
    pub fn send(&mut self, data: &[u8], mode: WriteMode) -> Result<(), WebSocketError> {
        if self.state != WebSocketState::Open {
            return Err(WebSocketError::NotOpen);
        }
        if self.outbound_buffer_size > 0 && data.len() > self.outbound_buffer_size {
            return Err(WebSocketError::BufferFull {
                size: data.len(),
                limit: self.outbound_buffer_size,
            });
        }
        if let Some(result) = self.browser_send(data, mode == WriteMode::Text) {
            return result;
        }
        let opcode = match mode {
            WriteMode::Text => Opcode::Text,
            WriteMode::Binary => Opcode::Binary,
        };
        self.send_frame(opcode, data)
    }

    /// Sends a UTF-8 text message. Errors unless OPEN.
    pub fn send_text(&mut self, text: &str) -> Result<(), WebSocketError> {
        self.send(text.as_bytes(), WriteMode::Text)
    }

    /// Number of complete inbound packets ready to read.
    pub fn get_available_packet_count(&self) -> usize {
        self.packets.len()
    }

    /// Pops the next inbound packet (empty `Vec` when none are queued).
    ///
    /// Updates [`was_string_packet`](Self::was_string_packet).
    pub fn get_packet(&mut self) -> Vec<u8> {
        match self.packets.pop_front() {
            Some((payload, was_string)) => {
                self.last_was_string = was_string;
                payload
            }
            None => Vec::new(),
        }
    }

    /// Whether the most recently read packet (via [`get_packet`](Self::get_packet))
    /// was a text frame.
    pub fn was_string_packet(&self) -> bool {
        self.last_was_string
    }

    /// Returns the current ready state.
    pub fn get_ready_state(&self) -> WebSocketState {
        self.state
    }

    /// Initiates a clean close with the given code and reason.
    pub fn close(&mut self, code: u16, reason: &str) {
        if self.browser_close(code, reason) {
            return;
        }
        match self.state {
            WebSocketState::Open => {
                let mut payload = Vec::with_capacity(2 + reason.len());
                payload.extend_from_slice(&code.to_be_bytes());
                payload.extend_from_slice(reason.as_bytes());
                let _ = self.send_frame(Opcode::Close, &payload);
                self.close_frame_sent = true;
                self.close_code = code as i32;
                self.close_reason = reason.to_string();
                self.state = WebSocketState::Closing;
            }
            WebSocketState::Connecting => self.transition_closed(code, reason),
            _ => {}
        }
    }

    /// The close code reported by the peer, or `-1` until CLOSED.
    pub fn get_close_code(&self) -> i32 {
        if self.state == WebSocketState::Closed {
            self.close_code
        } else {
            -1
        }
    }

    /// The close reason reported by the peer.
    pub fn get_close_reason(&self) -> String {
        self.close_reason.clone()
    }

    /// The URL passed to [`connect_to_url`](Self::connect_to_url).
    pub fn get_requested_url(&self) -> &str {
        &self.requested_url
    }

    /// The connected peer's IP address (Godot `WebSocketPeer.get_connected_host`).
    ///
    /// Empty when there is no connection — Godot's `StreamPeerTCP` answers the
    /// same way rather than erroring, so a script polling before the handshake
    /// completes reads `""` rather than raising.
    ///
    /// ⚠️ Godot documents this as **unavailable in the Web export**: a browser
    /// `WebSocket` never exposes the remote address. A wasm backend must report
    /// empty here rather than inventing a value.
    pub fn get_connected_host(&self) -> String {
        self.stream
            .as_ref()
            .and_then(|s| s.peer_addr().ok())
            .map(|a| a.ip().to_string())
            .unwrap_or_default()
    }

    /// The connected peer's remote port (Godot `WebSocketPeer.get_connected_port`).
    ///
    /// `0` when there is no connection. ⚠️ Also unavailable in Godot's Web
    /// export, for the same reason as [`get_connected_host`](Self::get_connected_host).
    pub fn get_connected_port(&self) -> u16 {
        self.stream
            .as_ref()
            .and_then(|s| s.peer_addr().ok())
            .map(|a| a.port())
            .unwrap_or(0)
    }

    /// Enables or disables Nagle's algorithm (Godot `WebSocketPeer.set_no_delay`).
    ///
    /// ⚠️ The argument's sense is inverted relative to the option it sets:
    /// `set_no_delay(true)` means *disable* Nagle, i.e. `TCP_NODELAY = true`.
    /// Connections start with it enabled, matching `wsl_peer.cpp:146`.
    ///
    /// A peer with no socket silently does nothing, and the setting does not
    /// persist across a later `connect_to_url` — Godot has the same shape,
    /// because the option lives on the socket rather than on the peer.
    pub fn set_no_delay(&mut self, enabled: bool) {
        if let Some(stream) = self.stream.as_ref() {
            let _ = stream.set_nodelay(enabled);
        }
    }

    /// Bytes still queued for sending (Godot
    /// `WebSocketPeer.get_current_outbound_buffered_amount`).
    ///
    /// ⚠️ Always `0` here, and that is an accurate answer rather than a stub:
    /// [`send`](Self::send) writes each frame straight to the socket and keeps
    /// no outbound queue, so there is never anything buffered to report. Godot's
    /// note — "Web exports use `WebSocket.bufferedAmount`, while other platforms
    /// use an internal buffer" — describes implementations that DO queue.
    /// **Anything that adds an outbound queue here must update this**, or a
    /// script throttling on back-pressure will read zero and never throttle.
    ///
    /// On wasm32 the browser IS that queue, so this reports its real
    /// `WebSocket.bufferedAmount` (pat-s3rrv.264).
    pub fn get_current_outbound_buffered_amount(&self) -> i64 {
        self.browser_buffered_amount().unwrap_or(0)
    }

    // -- internals ----------------------------------------------------------

    /// Drains all currently-available bytes into `in_buf`. Returns `true` on EOF.
    fn pump_reads(&mut self) -> std::io::Result<bool> {
        let stream = match self.stream.as_mut() {
            Some(s) => s,
            None => return Ok(true),
        };
        let mut tmp = [0u8; 4096];
        loop {
            match stream.read(&mut tmp) {
                Ok(0) => return Ok(true),
                Ok(n) => self.in_buf.extend_from_slice(&tmp[..n]),
                Err(ref e) if e.kind() == ErrorKind::WouldBlock => return Ok(false),
                Err(ref e) if e.kind() == ErrorKind::Interrupted => continue,
                Err(e) => return Err(e),
            }
        }
    }

    /// Validates the server's 101 response and records the negotiated
    /// subprotocol.
    ///
    /// Takes `&mut self` so the accepted `Sec-WebSocket-Protocol` can be stored
    /// (pat-0q2kx.96); the whole header block is scanned rather than returning
    /// at the accept-key line, because the protocol header may follow it.
    fn verify_handshake(&mut self, head: &str) -> bool {
        let mut lines = head.split("\r\n");
        let status = lines.next().unwrap_or("");
        if !status.contains(" 101") {
            return false;
        }
        let expected = compute_accept_key(&self.sent_key);
        let mut key_ok = false;
        let mut selected = String::new();
        for line in lines {
            if let Some((k, v)) = line.split_once(':') {
                let k = k.trim();
                if k.eq_ignore_ascii_case("sec-websocket-accept") {
                    key_ok = v.trim() == expected;
                } else if k.eq_ignore_ascii_case("sec-websocket-protocol") {
                    selected = v.trim().to_string();
                }
            }
        }
        if key_ok {
            // Godot reports only what the server actually chose. A server that
            // echoes a protocol we never offered is out of spec, so it is
            // ignored rather than reported as negotiated.
            self.selected_protocol = if self.supported_protocols.is_empty()
                || self.supported_protocols.iter().any(|p| p == &selected)
            {
                selected
            } else {
                String::new()
            };
        }
        key_ok
    }

    /// Queues an inbound packet, honouring `max_queued_packets`.
    ///
    /// ⚠️ A full queue REFUSES the newest packet; it does not evict the oldest.
    /// Godot's send-side cap is `ERR_FAIL_COND_V(queued >= max, ERR_OUT_OF_MEMORY)`
    /// (`wsl_peer.cpp:783`) — it rejects rather than making room. Evicting the
    /// oldest is the intuitive choice for a bounded queue and silently discards
    /// data the application had already been told it would receive.
    fn queue_packet(&mut self, payload: Vec<u8>, is_text: bool) {
        if self.packets.len() >= self.max_queued_packets {
            return;
        }
        self.packets.push_back((payload, is_text));
    }

    fn handle_frame(&mut self, frame: Frame) {
        // A message longer than `inbound_buffer_size` fails the connection
        // with 1009 MESSAGE_TOO_BIG — wslay's `max_recv_msg_length`
        // (`wsl_peer.cpp:299`). Counted across fragments.
        if matches!(
            frame.opcode,
            Opcode::Text | Opcode::Binary | Opcode::Continuation
        ) {
            let so_far = if frame.opcode == Opcode::Continuation {
                self.frag_payload.len()
            } else {
                0
            };
            if so_far + frame.payload.len() > self.inbound_buffer_size {
                self.frag_opcode = None;
                self.frag_payload.clear();
                self.close(1009, "");
                return;
            }
        }
        match frame.opcode {
            Opcode::Text | Opcode::Binary => {
                if frame.fin {
                    let is_text = frame.opcode == Opcode::Text;
                    self.queue_packet(frame.payload, is_text);
                } else {
                    self.frag_opcode = Some(frame.opcode);
                    self.frag_payload = frame.payload;
                }
            }
            Opcode::Continuation => {
                self.frag_payload.extend_from_slice(&frame.payload);
                if frame.fin {
                    let is_text = self.frag_opcode == Some(Opcode::Text);
                    let payload = std::mem::take(&mut self.frag_payload);
                    self.queue_packet(payload, is_text);
                    self.frag_opcode = None;
                }
            }
            Opcode::Ping => {
                let _ = self.send_frame(Opcode::Pong, &frame.payload);
            }
            // ⚠️ The answer to our own heartbeat Ping. Clearing this is what
            // stops the NEXT interval from reading the connection as dead.
            Opcode::Pong => {
                self.heartbeat_waiting = false;
            }
            Opcode::Close => {
                let (code, reason) = parse_close_payload(&frame.payload);
                if !self.close_frame_sent {
                    // Echo the close to complete the handshake politely.
                    let _ = self.send_frame(Opcode::Close, &frame.payload);
                    self.close_frame_sent = true;
                }
                self.close_code = code as i32;
                self.close_reason = reason;
                self.state = WebSocketState::Closed;
                self.shutdown_stream();
            }
        }
    }

    fn send_frame(&mut self, opcode: Opcode, payload: &[u8]) -> Result<(), WebSocketError> {
        let mask = generate_mask();
        let frame = Frame::new(opcode, payload.to_vec());
        let bytes = encode_frame(&frame, Some(mask));
        let stream = self.stream.as_mut().ok_or(WebSocketError::NotConnected)?;
        write_all_nonblocking(stream, &bytes)
    }

    fn transition_closed(&mut self, code: u16, reason: &str) {
        self.close_code = code as i32;
        self.close_reason = reason.to_string();
        self.state = WebSocketState::Closed;
        self.shutdown_stream();
    }

    fn shutdown_stream(&mut self) {
        if let Some(stream) = self.stream.take() {
            let _ = stream.shutdown(Shutdown::Both);
        }
    }
}

// ---------------------------------------------------------------------------
// Free helpers
// ---------------------------------------------------------------------------

fn parse_close_payload(payload: &[u8]) -> (u16, String) {
    if payload.len() >= 2 {
        let code = u16::from_be_bytes([payload[0], payload[1]]);
        let reason = String::from_utf8_lossy(&payload[2..]).to_string();
        (code, reason)
    } else {
        // 1005 = "no status code present" (RFC 6455 §7.4.1).
        (1005, String::new())
    }
}

fn find_subsequence(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn write_all_nonblocking(stream: &mut TcpStream, mut data: &[u8]) -> Result<(), WebSocketError> {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !data.is_empty() {
        match stream.write(data) {
            Ok(0) => return Err(WebSocketError::Io("write returned 0".into())),
            Ok(n) => data = &data[n..],
            Err(ref e) if e.kind() == ErrorKind::WouldBlock => {
                if Instant::now() >= deadline {
                    return Err(WebSocketError::Io("write timed out".into()));
                }
                std::thread::sleep(Duration::from_millis(1));
            }
            Err(ref e) if e.kind() == ErrorKind::Interrupted => continue,
            Err(e) => return Err(WebSocketError::Io(e.to_string())),
        }
    }
    Ok(())
}

/// Returns a fresh-ish 64-bit nonce without pulling in an RNG dependency.
fn next_nonce() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};
    // Golden-ratio increment so successive nonces differ widely.
    static CTR: AtomicU64 = AtomicU64::new(0x9E37_79B9_7F4A_7C15);
    let c = CTR.fetch_add(0x9E37_79B9_7F4A_7C15, Ordering::Relaxed);
    let t = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    c ^ t.rotate_left(32) ^ t.wrapping_mul(0xD6E8_FEB8_6659_FD93)
}

fn generate_mask() -> [u8; 4] {
    (next_nonce() as u32).to_le_bytes()
}

fn generate_key() -> String {
    let mut bytes = [0u8; 16];
    bytes[..8].copy_from_slice(&next_nonce().to_le_bytes());
    bytes[8..].copy_from_slice(&next_nonce().to_le_bytes());
    base64_encode(&bytes)
}

// ---------------------------------------------------------------------------
// Tiny SHA-1 + base64 (no external crates)
// ---------------------------------------------------------------------------

/// Standard base64 (RFC 4648) encoder with padding.
fn base64_encode(data: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = *chunk.get(1).unwrap_or(&0) as u32;
        let b2 = *chunk.get(2).unwrap_or(&0) as u32;
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(TABLE[((n >> 18) & 0x3F) as usize] as char);
        out.push(TABLE[((n >> 12) & 0x3F) as usize] as char);
        if chunk.len() > 1 {
            out.push(TABLE[((n >> 6) & 0x3F) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(TABLE[(n & 0x3F) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
}

/// Minimal SHA-1 implementation (RFC 3174) — used only for the WS accept key.
fn sha1(data: &[u8]) -> [u8; 20] {
    let mut h: [u32; 5] = [
        0x6745_2301,
        0xEFCD_AB89,
        0x98BA_DCFE,
        0x1032_5476,
        0xC3D2_E1F0,
    ];
    let bit_len = (data.len() as u64) * 8;

    let mut message = data.to_vec();
    message.push(0x80);
    while message.len() % 64 != 56 {
        message.push(0x00);
    }
    message.extend_from_slice(&bit_len.to_be_bytes());

    for block in message.chunks(64) {
        let mut w = [0u32; 80];
        for (i, word) in w.iter_mut().take(16).enumerate() {
            *word = u32::from_be_bytes([
                block[i * 4],
                block[i * 4 + 1],
                block[i * 4 + 2],
                block[i * 4 + 3],
            ]);
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }

        let (mut a, mut b, mut c, mut d, mut e) = (h[0], h[1], h[2], h[3], h[4]);
        for (i, &wi) in w.iter().enumerate() {
            let (f, k) = match i {
                0..=19 => ((b & c) | ((!b) & d), 0x5A82_7999u32),
                20..=39 => (b ^ c ^ d, 0x6ED9_EBA1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1B_BCDC),
                _ => (b ^ c ^ d, 0xCA62_C1D6),
            };
            let tmp = a
                .rotate_left(5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(wi);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = tmp;
        }
        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
    }

    let mut out = [0u8; 20];
    for (i, word) in h.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&word.to_be_bytes());
    }
    out
}

// ---------------------------------------------------------------------------
// RefCounted handle table (pat-0q2kx.96)
//
// Godot's `WebSocketPeer` is constructed from GDScript with `WebSocketPeer.new()`
// and is RefCounted, not a Node — so it cannot live in the scene tree. It gets
// the same treatment as ConfigFile / FileAccess / CameraFeed: the object lives
// in a process-wide table and scripts hold a tagged `ObjectId` into it.
// ---------------------------------------------------------------------------

/// Tags an `ObjectId` issued by [`new_websocket_peer_handle`]. `0x57` is ASCII
/// `'W'`; it sits alongside the ConfigFile / FileAccess / DirAccess / CameraFeed
/// / UndoRedo tags (`0xCF` / `0xFA` / `0xDA` / `0xCA` / `0xED`).
const WEBSOCKET_HANDLE_TAG: u64 = 0x5700_0000_0000_0000;
static NEXT_WEBSOCKET_HANDLE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
static WEBSOCKET_HANDLES: std::sync::Mutex<Option<std::collections::HashMap<u64, WebSocketPeer>>> =
    std::sync::Mutex::new(None);

fn websocket_handles(
) -> std::sync::MutexGuard<'static, Option<std::collections::HashMap<u64, WebSocketPeer>>> {
    // A poisoned lock still holds a usable table: a panic in one script's
    // handler must not make every later WebSocket call fail.
    WEBSOCKET_HANDLES
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Returns `true` when `id` was issued by [`new_websocket_peer_handle`].
pub fn is_websocket_peer_handle(id: gdcore::id::ObjectId) -> bool {
    id.raw() & 0xFF00_0000_0000_0000 == WEBSOCKET_HANDLE_TAG
}

/// Allocates a fresh, unconnected [`WebSocketPeer`] in the process-wide handle
/// table and returns its script-visible id (Godot `WebSocketPeer.new()`).
pub fn new_websocket_peer_handle() -> gdcore::id::ObjectId {
    let seq = NEXT_WEBSOCKET_HANDLE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let raw = WEBSOCKET_HANDLE_TAG | seq;
    websocket_handles()
        .get_or_insert_with(std::collections::HashMap::new)
        .insert(raw, WebSocketPeer::new());
    gdcore::id::ObjectId::from_raw(raw)
}

/// Runs `f` against the live peer behind `id`, or returns `None` when the
/// handle was never issued or has already been released.
pub fn with_websocket_peer<R>(
    id: gdcore::id::ObjectId,
    f: impl FnOnce(&mut WebSocketPeer) -> R,
) -> Option<R> {
    let mut guard = websocket_handles();
    let map = guard.get_or_insert_with(std::collections::HashMap::new);
    map.get_mut(&id.raw()).map(f)
}

/// Drops a handle, closing the socket if it is still open. Idempotent.
pub fn release_websocket_peer_handle(id: gdcore::id::ObjectId) {
    let mut guard = websocket_handles();
    if let Some(map) = guard.as_mut() {
        if let Some(mut peer) = map.remove(&id.raw()) {
            if peer.get_ready_state() != WebSocketState::Closed {
                peer.close(1001, "going away");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// pat-s3rrv.79 — the four connection accessors answer on an UNCONNECTED
    /// peer without panicking.
    ///
    /// ⚠️ This is the state a script sees most often: `WebSocketPeer.new()` then
    /// a poll loop before the handshake completes. Godot's `StreamPeerTCP`
    /// answers `""`/`0` there rather than erroring, so a script reading them
    /// early gets a value, not a raise.
    /// pat-s3rrv.79 — the heartbeat is a LIVENESS TIMEOUT, not a keepalive.
    ///
    /// ⚠️ This is the assertion that separates the two. A peer that sends a Ping
    /// every interval and never checks for the Pong passes any test that only
    /// counts pings; it keeps a dead connection open forever. Godot drops the
    /// connection at the SECOND elapsed interval when the first Ping went
    /// unanswered (`wsl_peer.cpp:716-721`), and this drives exactly that.
    #[test]
    fn an_unanswered_heartbeat_drops_the_connection_on_the_next_interval() {
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().unwrap().port();
        // A server that completes the handshake and then goes silent — never
        // answering a Ping. That is the case the timeout exists for; a server
        // that answers is indistinguishable from a working one.
        let server = std::thread::spawn(move || {
            if let Ok((mut sock, _)) = listener.accept() {
                let mut buf = [0u8; 2048];
                let n = std::io::Read::read(&mut sock, &mut buf).unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..n]).to_string();
                if let Some(key) = parse_handshake_key(&req) {
                    let _ = std::io::Write::write_all(
                        &mut sock,
                        websocket_handshake_response(&key).as_bytes(),
                    );
                }
                // Hold the socket open, deliberately answering nothing.
                std::thread::sleep(Duration::from_millis(600));
            }
        });

        let mut peer = WebSocketPeer::new();
        peer.set_heartbeat_interval(0.05);
        peer.connect_to_url(&format!("ws://127.0.0.1:{port}/"))
            .expect("connect");

        // Drive until the handshake completes.
        for _ in 0..100 {
            peer.poll();
            if peer.get_ready_state() == WebSocketState::Open {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(
            peer.get_ready_state(),
            WebSocketState::Open,
            "the fixture must reach OPEN before the heartbeat can be tested",
        );

        // First interval: a Ping goes out and the peer stays open.
        std::thread::sleep(Duration::from_millis(70));
        peer.poll();
        assert_eq!(
            peer.get_ready_state(),
            WebSocketState::Open,
            "one unanswered Ping is not yet a timeout — Godot drops on the SECOND",
        );

        // Second interval with no Pong: the connection is torn down.
        std::thread::sleep(Duration::from_millis(70));
        peer.poll();
        assert_eq!(
            peer.get_ready_state(),
            WebSocketState::Closed,
            "an unanswered heartbeat must drop the connection; staying open is \
             the keepalive-only bug this test exists for",
        );

        let _ = server.join();
    }

    /// A zero interval — Godot's default — never pings and never times out.
    #[test]
    fn a_zero_heartbeat_interval_is_disabled() {
        let mut peer = WebSocketPeer::new();
        assert_eq!(
            peer.get_heartbeat_interval(),
            0.0,
            "Godot's default is 0.0 = disabled",
        );
        // Negative values disable too rather than producing a nonsense Duration.
        peer.set_heartbeat_interval(-5.0);
        assert_eq!(peer.get_heartbeat_interval(), 0.0);
        peer.set_heartbeat_interval(2.5);
        assert_eq!(peer.get_heartbeat_interval(), 2.5);
    }

    /// ⚠️ A full inbound queue REFUSES the newest packet rather than evicting
    /// the oldest.
    ///
    /// Evicting is the intuitive choice for a bounded queue and is wrong: it
    /// discards data the application was already told it would receive. Godot's
    /// cap rejects instead (`wsl_peer.cpp:783`, `ERR_OUT_OF_MEMORY`). The
    /// assertion checks WHICH packets survived, not just how many — a count
    /// alone cannot tell the two policies apart.
    #[test]
    fn a_full_packet_queue_refuses_the_newest_not_the_oldest() {
        let mut peer = WebSocketPeer::new();
        assert_eq!(
            peer.get_max_queued_packets(),
            4096,
            "Godot's documented default",
        );
        peer.set_max_queued_packets(2);

        peer.queue_packet(b"first".to_vec(), true);
        peer.queue_packet(b"second".to_vec(), true);
        peer.queue_packet(b"third".to_vec(), true);

        assert_eq!(peer.get_available_packet_count(), 2, "the cap holds");
        assert_eq!(
            peer.get_packet(),
            b"first".to_vec(),
            "the OLDEST packet must survive — evicting it to make room for \
             `third` loses data the application was promised",
        );
        assert_eq!(peer.get_packet(), b"second".to_vec());
    }

    /// Connects a peer (configured by `setup` before connecting) to a loopback
    /// server that completes the handshake, sends `frames` (unmasked, as a
    /// server does) and then holds the socket open for `hold_ms`.
    fn open_peer_with_server_frames(
        frames: Vec<Frame>,
        hold_ms: u64,
        setup: impl FnOnce(&mut WebSocketPeer),
    ) -> (WebSocketPeer, std::thread::JoinHandle<()>) {
        use std::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            if let Ok((mut sock, _)) = listener.accept() {
                let mut buf = [0u8; 2048];
                let n = std::io::Read::read(&mut sock, &mut buf).unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..n]).to_string();
                if let Some(key) = parse_handshake_key(&req) {
                    let _ = std::io::Write::write_all(
                        &mut sock,
                        websocket_handshake_response(&key).as_bytes(),
                    );
                }
                for f in &frames {
                    let _ = std::io::Write::write_all(&mut sock, &encode_frame(f, None));
                }
                std::thread::sleep(Duration::from_millis(hold_ms));
            }
        });
        let mut peer = WebSocketPeer::new();
        setup(&mut peer);
        peer.connect_to_url(&format!("ws://127.0.0.1:{port}/"))
            .expect("connect");
        // Until the handshake resolves. Frames written right behind the 101
        // can arrive in the same read and act in the same poll, so a test
        // whose frames close the connection may already be past OPEN here.
        for _ in 0..200 {
            peer.poll();
            if peer.get_ready_state() != WebSocketState::Connecting {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_ne!(
            peer.get_ready_state(),
            WebSocketState::Connecting,
            "handshake resolved"
        );
        (peer, server)
    }

    /// Polls a few times so frames already written by the server arrive.
    fn settle(peer: &mut WebSocketPeer) {
        for _ in 0..20 {
            peer.poll();
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn the_buffer_sizes_default_to_godots_65535_and_refuse_negatives() {
        let mut peer = WebSocketPeer::new();
        assert_eq!(peer.get_inbound_buffer_size(), 65535);
        assert_eq!(peer.get_outbound_buffer_size(), 65535);
        // Godot's ring buffer for 65535 bytes is 1 << nearest_shift(65535).
        assert_eq!(peer.inbound_queue_capacity(), 65536);
        peer.set_inbound_buffer_size(-1);
        peer.set_outbound_buffer_size(-1);
        assert_eq!(peer.get_inbound_buffer_size(), 65535, "negative refused");
        assert_eq!(peer.get_outbound_buffer_size(), 65535, "negative refused");
    }

    /// `outbound_buffer_size` bounds a send: Godot's `ERR_OUT_OF_MEMORY` once
    /// queued + new bytes exceed it (`wsl_peer.cpp:784`), and `0` = unbounded.
    #[test]
    fn a_send_larger_than_outbound_buffer_size_is_refused() {
        let (mut peer, server) = open_peer_with_server_frames(vec![], 300, |p| {
            p.set_outbound_buffer_size(8);
        });
        assert!(matches!(
            peer.send(&[0u8; 9], WriteMode::Binary),
            Err(WebSocketError::BufferFull { size: 9, limit: 8 })
        ));
        assert!(
            peer.send(&[0u8; 8], WriteMode::Binary).is_ok(),
            "exactly the limit fits"
        );
        peer.set_outbound_buffer_size(0);
        assert!(
            peer.send(&[0u8; 100], WriteMode::Binary).is_ok(),
            "0 disables the bound"
        );
        let _ = server.join();
    }

    /// A message longer than `inbound_buffer_size` fails the connection with
    /// 1009 MESSAGE_TOO_BIG (wslay `max_recv_msg_length`, `wsl_peer.cpp:299`);
    /// one within it is delivered.
    #[test]
    fn a_message_longer_than_inbound_buffer_size_closes_with_1009() {
        let frames = vec![
            Frame::new(Opcode::Text, b"0123456789".to_vec()),
            Frame::new(Opcode::Text, vec![b'x'; 11]),
        ];
        let (mut peer, server) = open_peer_with_server_frames(frames, 300, |p| {
            p.set_inbound_buffer_size(10);
        });
        settle(&mut peer);
        assert!(
            peer.close_frame_sent,
            "the handshake completed and the peer failed it"
        );
        assert_eq!(
            peer.get_available_packet_count(),
            1,
            "the 10-byte message fits"
        );
        assert_eq!(peer.get_packet(), b"0123456789".to_vec());
        assert_ne!(
            peer.get_ready_state(),
            WebSocketState::Open,
            "an 11-byte message over a 10-byte limit must fail the connection"
        );
        assert_eq!(peer.close_code, 1009, "MESSAGE_TOO_BIG");
        let _ = server.join();
    }

    /// The inbound queue's byte budget (`1 << nearest_shift(size)`) applies
    /// BACKPRESSURE: a message that does not fit waits in the socket buffer
    /// and arrives once the script drains packets — it is not dropped
    /// (`wsl_peer.cpp:574` reads only `payload_space_left()`).
    #[test]
    fn a_full_inbound_byte_budget_delays_messages_rather_than_dropping_them() {
        // inbound 16 → capacity 32: two 12-byte messages fit, a third waits.
        let frames = (0..3u8)
            .map(|i| Frame::new(Opcode::Binary, vec![i; 12]))
            .collect();
        let (mut peer, server) = open_peer_with_server_frames(frames, 400, |p| {
            p.set_inbound_buffer_size(16);
        });
        settle(&mut peer);
        assert_eq!(peer.get_available_packet_count(), 2, "the budget holds two");
        assert_eq!(peer.get_packet(), vec![0u8; 12]);
        settle(&mut peer);
        assert_eq!(
            peer.get_available_packet_count(),
            2,
            "the third arrives once there is room"
        );
        assert_eq!(peer.get_packet(), vec![1u8; 12]);
        assert_eq!(peer.get_packet(), vec![2u8; 12], "nothing was dropped");
        assert_eq!(peer.get_ready_state(), WebSocketState::Open);
        let _ = server.join();
    }

    /// A cap below 1 is clamped, not honoured.
    #[test]
    fn a_zero_packet_cap_is_clamped_to_one() {
        let mut peer = WebSocketPeer::new();
        peer.set_max_queued_packets(0);
        assert_eq!(
            peer.get_max_queued_packets(),
            1,
            "a cap of zero would discard every packet on arrival, which is \
             indistinguishable from a dead connection",
        );
    }

    #[test]
    fn the_connection_accessors_answer_on_an_unconnected_peer() {
        let mut peer = WebSocketPeer::new();
        assert_eq!(peer.get_connected_host(), "", "no socket, no host");
        assert_eq!(peer.get_connected_port(), 0, "no socket, no port");
        assert_eq!(
            peer.get_current_outbound_buffered_amount(),
            0,
            "nothing sent, nothing queued",
        );
        // A no-op rather than a panic or an error — the option lives on the
        // socket, and there is no socket yet.
        peer.set_no_delay(true);
        peer.set_no_delay(false);
    }

    /// A connected peer reports the address it is actually connected to, and
    /// starts with Nagle disabled.
    ///
    /// ⚠️ The port assertion compares against the listener's OWN port rather
    /// than a literal: an ephemeral port is the only way to keep this test from
    /// colliding with a parallel one, so a hardcoded number would be both flaky
    /// and untrue.
    #[test]
    fn a_connected_peer_reports_its_remote_address_and_starts_nodelay() {
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().unwrap().port();
        // Accept and hold, so the connection stays up for the assertions. The
        // handshake never completes, which is fine — these four accessors are
        // socket-level and answer before the WebSocket upgrade does.
        let server = std::thread::spawn(move || {
            let held = listener.accept();
            std::thread::sleep(std::time::Duration::from_millis(300));
            drop(held);
        });

        let mut peer = WebSocketPeer::new();
        // The handshake will not finish against this bare listener; the socket
        // is connected either way, which is what is under test.
        let _ = peer.connect_to_url(&format!("ws://127.0.0.1:{port}/"));

        assert_eq!(
            peer.get_connected_host(),
            "127.0.0.1",
            "a connected peer must report the remote IP",
        );
        assert_eq!(
            peer.get_connected_port(),
            port,
            "…and the remote port, which is the listener's ephemeral one",
        );
        // ⚠️ Godot sets TCP_NODELAY on every connection (`wsl_peer.cpp:146`), so
        // this is the DEFAULT rather than something a script must ask for.
        // Leaving Nagle on adds up to 40 ms to small frames and reads as server
        // lag.
        assert!(
            peer.stream.as_ref().unwrap().nodelay().unwrap_or(false),
            "connect_to_url must disable Nagle, matching Godot's default",
        );
        // And it can be turned back on.
        peer.set_no_delay(false);
        assert!(!peer.stream.as_ref().unwrap().nodelay().unwrap_or(true));

        let _ = server.join();
    }

    #[test]
    fn base64_known_vectors() {
        assert_eq!(base64_encode(b"abc"), "YWJj");
        assert_eq!(base64_encode(b"ab"), "YWI=");
        assert_eq!(base64_encode(b"a"), "YQ==");
        assert_eq!(base64_encode(b""), "");
    }

    #[test]
    fn sha1_known_vector() {
        // SHA1("abc") = a9993e364706816aba3e25717850c26c9cd0d89d
        let digest = sha1(b"abc");
        let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(hex, "a9993e364706816aba3e25717850c26c9cd0d89d");
    }

    #[test]
    fn rfc6455_accept_key() {
        // The canonical example from RFC 6455 §1.3.
        assert_eq!(
            compute_accept_key("dGhlIHNhbXBsZSBub25jZQ=="),
            "s3pPLMBiTxaQ9kYGzzhZRbK+xOo="
        );
    }

    #[test]
    fn frame_round_trip_masked_and_unmasked() {
        let frame = Frame::new(Opcode::Text, b"hello".to_vec());

        // Unmasked (server -> client).
        let bytes = encode_frame(&frame, None);
        let (decoded, consumed) = decode_frame(&bytes).unwrap().unwrap();
        assert_eq!(consumed, bytes.len());
        assert_eq!(decoded.opcode, Opcode::Text);
        assert_eq!(decoded.payload, b"hello");
        assert!(decoded.fin);

        // Masked (client -> server) decodes back to the same payload.
        let masked = encode_frame(&frame, Some([0xAA, 0xBB, 0xCC, 0xDD]));
        assert_ne!(masked, bytes, "masking must alter the wire bytes");
        let (decoded, _) = decode_frame(&masked).unwrap().unwrap();
        assert_eq!(decoded.payload, b"hello");
    }

    #[test]
    fn decode_needs_more_bytes() {
        // A length-2 header advertising 5 bytes but only 2 present.
        let partial = [0x81u8, 0x05, b'h', b'i'];
        assert!(decode_frame(&partial).unwrap().is_none());
    }

    #[test]
    fn decode_extended_length_126() {
        let payload = vec![7u8; 200];
        let frame = Frame::new(Opcode::Binary, payload.clone());
        let bytes = encode_frame(&frame, None);
        // 200 bytes uses the 16-bit extended length form.
        assert_eq!(bytes[1] & 0x7F, 126);
        let (decoded, _) = decode_frame(&bytes).unwrap().unwrap();
        assert_eq!(decoded.payload, payload);
    }

    #[test]
    fn close_payload_parsing() {
        let mut payload = 1000u16.to_be_bytes().to_vec();
        payload.extend_from_slice(b"bye");
        let (code, reason) = parse_close_payload(&payload);
        assert_eq!(code, 1000);
        assert_eq!(reason, "bye");

        let (code, reason) = parse_close_payload(&[]);
        assert_eq!(code, 1005);
        assert!(reason.is_empty());
    }

    #[test]
    fn handshake_helpers_round_trip() {
        let request =
            "GET /chat HTTP/1.1\r\nHost: x\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\r\n";
        let key = parse_handshake_key(request).unwrap();
        assert_eq!(key, "dGhlIHNhbXBsZSBub25jZQ==");
        let response = websocket_handshake_response(&key);
        assert!(response.contains("101 Switching Protocols"));
        assert!(response.contains("Sec-WebSocket-Accept: s3pPLMBiTxaQ9kYGzzhZRbK+xOo="));
    }

    #[test]
    fn state_and_write_mode_godot_codes() {
        assert_eq!(WebSocketState::Connecting.as_godot_code(), 0);
        assert_eq!(WebSocketState::Open.as_godot_code(), 1);
        assert_eq!(WebSocketState::Closing.as_godot_code(), 2);
        assert_eq!(WebSocketState::Closed.as_godot_code(), 3);
        assert_eq!(WriteMode::Text.as_godot_code(), 0);
        assert_eq!(WriteMode::Binary.as_godot_code(), 1);
    }

    #[test]
    fn new_peer_is_closed_and_send_fails() {
        let mut peer = WebSocketPeer::new();
        assert_eq!(peer.get_ready_state(), WebSocketState::Closed);
        assert_eq!(peer.get_available_packet_count(), 0);
        assert!(peer.send_text("nope").is_err());
    }

    #[test]
    fn wss_is_rejected() {
        let mut peer = WebSocketPeer::new();
        let err = peer.connect_to_url("wss://example.com/").unwrap_err();
        assert!(matches!(err, WebSocketError::TlsUnsupported));
    }

    #[test]
    fn non_ws_scheme_is_rejected() {
        let mut peer = WebSocketPeer::new();
        assert!(peer.connect_to_url("http://example.com/").is_err());
    }
}
