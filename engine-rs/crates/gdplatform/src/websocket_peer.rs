//! WebSocketPeer — client-side RFC 6455 WebSocket over the platform TCP stack.
//!
//! Mirrors Godot's `WebSocketPeer` (modules/websocket/*): `connect_to_url`,
//! `send`/`send_text`, `get_available_packet_count`/`get_packet`, `poll`, a
//! `CONNECTING → OPEN → CLOSING → CLOSED` ready-state lifecycle, and close
//! codes/reasons matching Godot's API.
//!
//! The RFC 6455 handshake, frame masking, and the text/binary/close/ping/pong
//! codec are implemented from scratch (including a tiny SHA-1 + base64) so the
//! library pulls in **no** new runtime dependencies. Only plaintext `ws://` is
//! wired; `wss://` reports [`WebSocketError::TlsUnsupported`].

use std::collections::VecDeque;
use std::io::{ErrorKind, Read, Write};
use std::net::{Shutdown, TcpStream, ToSocketAddrs};
use std::time::{Duration, Instant};

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

    Ok(Some((Frame { fin, opcode, payload }, idx)))
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
    let accept = compute_accept_key(client_key);
    format!(
        "HTTP/1.1 101 Switching Protocols\r\n\
         Upgrade: websocket\r\n\
         Connection: Upgrade\r\n\
         Sec-WebSocket-Accept: {accept}\r\n\r\n"
    )
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
}

impl Default for WebSocketPeer {
    fn default() -> Self {
        Self::new()
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
        }
    }

    /// Sets the connection-establishment timeout.
    pub fn set_connect_timeout(&mut self, timeout: Duration) {
        self.connect_timeout = timeout;
    }

    /// Connects to a `ws://` URL and begins the opening handshake.
    ///
    /// The peer enters [`WebSocketState::Connecting`]; call [`poll`](Self::poll)
    /// until it reaches [`WebSocketState::Open`].
    pub fn connect_to_url(&mut self, url: &str) -> Result<(), WebSocketError> {
        let parsed = parse_url(url).ok_or_else(|| WebSocketError::InvalidUrl(url.to_string()))?;
        if !matches!(parsed.scheme.as_str(), "ws" | "wss") {
            return Err(WebSocketError::InvalidUrl(format!(
                "scheme '{}' is not ws/wss",
                parsed.scheme
            )));
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

        let key = generate_key();
        let host_header = if parsed.port == 80 {
            parsed.host.clone()
        } else {
            format!("{}:{}", parsed.host, parsed.port)
        };
        let request = format!(
            "GET {} HTTP/1.1\r\n\
             Host: {}\r\n\
             Upgrade: websocket\r\n\
             Connection: Upgrade\r\n\
             Sec-WebSocket-Key: {}\r\n\
             Sec-WebSocket-Version: 13\r\n\r\n",
            parsed.path, host_header, key
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
        Ok(())
    }

    /// Drives the connection: completes the handshake, drains inbound frames
    /// into the packet queue, answers pings, and advances the close handshake.
    pub fn poll(&mut self) {
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
    }

    /// Sends a payload as a text or binary message. Errors unless OPEN.
    pub fn send(&mut self, data: &[u8], mode: WriteMode) -> Result<(), WebSocketError> {
        if self.state != WebSocketState::Open {
            return Err(WebSocketError::NotOpen);
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

    fn verify_handshake(&self, head: &str) -> bool {
        let mut lines = head.split("\r\n");
        let status = lines.next().unwrap_or("");
        if !status.contains(" 101") {
            return false;
        }
        let expected = compute_accept_key(&self.sent_key);
        for line in lines {
            if let Some((k, v)) = line.split_once(':') {
                if k.trim().eq_ignore_ascii_case("sec-websocket-accept") {
                    return v.trim() == expected;
                }
            }
        }
        false
    }

    fn handle_frame(&mut self, frame: Frame) {
        match frame.opcode {
            Opcode::Text | Opcode::Binary => {
                if frame.fin {
                    self.packets
                        .push_back((frame.payload, frame.opcode == Opcode::Text));
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
                    self.packets.push_back((payload, is_text));
                    self.frag_opcode = None;
                }
            }
            Opcode::Ping => {
                let _ = self.send_frame(Opcode::Pong, &frame.payload);
            }
            Opcode::Pong => {}
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
    let mut h: [u32; 5] = [0x6745_2301, 0xEFCD_AB89, 0x98BA_DCFE, 0x1032_5476, 0xC3D2_E1F0];
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

#[cfg(test)]
mod tests {
    use super::*;

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
        let request = "GET /chat HTTP/1.1\r\nHost: x\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\r\n";
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
