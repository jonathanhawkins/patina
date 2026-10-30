//! Browser-backed WebSocket transport for [`WebSocketPeer`] (pat-s3rrv.264).
//!
//! Godot's Web export does not run its own RFC 6455 codec. `emws_peer.cpp` and
//! `library_godot_websocket.js` hand the URL straight to the browser's
//! `WebSocket` object, so the browser performs the opening handshake, the
//! framing, and — for `wss://` — the TLS. That is why `wss://` works in a Web
//! export while the native build in [`crate::websocket_peer`] still reports
//! [`WebSocketError::TlsUnsupported`](crate::websocket_peer::WebSocketError::TlsUnsupported):
//! the native path owns its own socket and has no TLS stack, the browser path
//! never sees a socket at all.
//!
//! This module is deliberately split in two:
//!
//! * The **portable** half — [`ready_state_from_browser`] and
//!   [`BrowserPacketQueue`] — is compiled on every target. It holds the logic a
//!   test cannot ask a browser about: the readyState mapping and the
//!   text/binary receive queue with its `max_queued_packets` cap. Covered by
//!   `tests/websocket_wasm_backend_contract.rs` on native.
//! * The **browser** half is `cfg(target_arch = "wasm32")` and holds only the
//!   `js_sys::Reflect` plumbing, which cannot run outside a browser.
//!
//! ⚠️ The browser half keeps its sockets in a `thread_local` registry and hands
//! out a `u64` handle, which is what `WebSocketPeer` stores. That indirection is
//! not stylistic: `js_sys::Closure` and `Rc` are neither `Send` nor `Sync`, and
//! `WebSocketPeer` lives inside the `static Mutex<HashMap<u64, WebSocketPeer>>`
//! handle table in `websocket_peer.rs`, which requires `Send`. Storing the
//! socket inline would stop that static from compiling on wasm32. The
//! JavaScriptBridge in [`crate::js_bridge`] is shaped the same way for the same
//! reason.

use std::collections::VecDeque;

use crate::websocket_peer::WebSocketState;

// ---------------------------------------------------------------------------
// Ready state (portable)
// ---------------------------------------------------------------------------

/// Browser `WebSocket.readyState` — the socket is still handshaking.
pub const READY_STATE_CONNECTING: i32 = 0;
/// Browser `WebSocket.readyState` — messages may flow.
pub const READY_STATE_OPEN: i32 = 1;
/// Browser `WebSocket.readyState` — a close handshake is underway.
pub const READY_STATE_CLOSING: i32 = 2;
/// Browser `WebSocket.readyState` — fully closed.
pub const READY_STATE_CLOSED: i32 = 3;

/// Maps a browser `readyState` onto Godot's `WebSocketPeer.State`.
///
/// The two enums happen to agree numerically (WHATWG HTML §9.3.3 and
/// `websocket_peer.h`'s `STATE_CONNECTING…STATE_CLOSED` are both 0..=3), but
/// this goes through a named mapping rather than an `as` cast for two reasons:
///
/// 1. ⚠️ An **unknown** value must read as CLOSED, not panic and not silently
///    become CONNECTING. `Reflect::get` on a socket the browser has already
///    torn down can answer `undefined`, which the caller turns into a sentinel;
///    treating that as "still connecting" would hang a client forever waiting
///    for a handshake that can never complete.
/// 2. The agreement between the two enums is a coincidence of two separate
///    specs, so it belongs in one asserted place instead of implicitly in every
///    call site.
pub fn ready_state_from_browser(raw: i32) -> WebSocketState {
    match raw {
        READY_STATE_CONNECTING => WebSocketState::Connecting,
        READY_STATE_OPEN => WebSocketState::Open,
        READY_STATE_CLOSING => WebSocketState::Closing,
        _ => WebSocketState::Closed,
    }
}

// ---------------------------------------------------------------------------
// Inbound packet queue (portable)
// ---------------------------------------------------------------------------

/// Godot's default `WebSocketPeer.max_queued_packets`.
pub const DEFAULT_MAX_QUEUED_PACKETS: usize = 4096;

/// The inbound queue the browser's `onmessage` handler writes into.
///
/// Each entry is `(payload, was_string)`, matching the pair
/// `WebSocketPeer::get_packet` / `WebSocketPeer::was_string_packet` hand back.
/// A browser `MessageEvent.data` is either a JS string (text frame) or an
/// `ArrayBuffer` (binary frame, once `binaryType` is `"arraybuffer"`), and the
/// distinction has to survive into the queue: Godot's
/// `WebSocketPeer.was_string_packet()` reports the frame type of the packet the
/// script just read, so a queue that stored only bytes could not answer it.
#[derive(Debug, Clone)]
pub struct BrowserPacketQueue {
    packets: VecDeque<(Vec<u8>, bool)>,
    max_queued_packets: usize,
    refused: u64,
}

impl Default for BrowserPacketQueue {
    fn default() -> Self {
        Self::new()
    }
}

impl BrowserPacketQueue {
    /// A queue with Godot's default cap of [`DEFAULT_MAX_QUEUED_PACKETS`].
    pub fn new() -> Self {
        Self::with_max(DEFAULT_MAX_QUEUED_PACKETS)
    }

    /// A queue capped at `max` packets.
    ///
    /// A `max` of `0` is raised to `1`: a zero cap would refuse every inbound
    /// message, which reads as a dead socket rather than as a configuration
    /// mistake. Godot clamps the property to `>= 0` and treats it as a limit on
    /// a queue it always pushes at least once into.
    pub fn with_max(max: usize) -> Self {
        Self {
            packets: VecDeque::new(),
            max_queued_packets: max.max(1),
            refused: 0,
        }
    }

    /// Re-caps the queue. Already-queued packets are kept.
    pub fn set_max_queued_packets(&mut self, max: usize) {
        self.max_queued_packets = max.max(1);
    }

    /// The current cap.
    pub fn max_queued_packets(&self) -> usize {
        self.max_queued_packets
    }

    /// Queues a text frame's payload (`MessageEvent.data` was a JS string).
    ///
    /// The bytes stored are the string's UTF-8 encoding, so a script reading it
    /// back with `get_packet().get_string_from_utf8()` sees what was sent.
    pub fn push_text(&mut self, text: &str) {
        self.push(text.as_bytes().to_vec(), true);
    }

    /// Queues a binary frame's payload (`MessageEvent.data` was an
    /// `ArrayBuffer`).
    pub fn push_binary(&mut self, bytes: Vec<u8>) {
        self.push(bytes, false);
    }

    /// ⚠️ A full queue REFUSES the newest packet; it does not evict the oldest.
    ///
    /// This matches `WebSocketPeer::queue_packet` in the native path, which in
    /// turn matches Godot's `ERR_FAIL_COND_V(queued >= max, ERR_OUT_OF_MEMORY)`
    /// (`wsl_peer.cpp:783`). Evicting the oldest is the intuitive choice for a
    /// bounded queue and silently discards data the application had already
    /// been told it would receive.
    fn push(&mut self, payload: Vec<u8>, was_string: bool) {
        if self.packets.len() >= self.max_queued_packets {
            self.refused = self.refused.saturating_add(1);
            return;
        }
        self.packets.push_back((payload, was_string));
    }

    /// Pops the oldest packet as `(payload, was_string)`.
    pub fn pop(&mut self) -> Option<(Vec<u8>, bool)> {
        self.packets.pop_front()
    }

    /// Drains every queued packet, oldest first.
    pub fn drain(&mut self) -> Vec<(Vec<u8>, bool)> {
        self.packets.drain(..).collect()
    }

    /// Number of packets waiting to be read.
    pub fn len(&self) -> usize {
        self.packets.len()
    }

    /// Whether nothing is queued.
    pub fn is_empty(&self) -> bool {
        self.packets.is_empty()
    }

    /// How many packets the cap has refused since this queue was created.
    ///
    /// Exposed so a dropped message is observable rather than silent — the cap
    /// discarding real data is otherwise indistinguishable from a quiet server.
    pub fn refused(&self) -> u64 {
        self.refused
    }

    /// Forgets every queued packet. The refusal count is kept.
    pub fn clear(&mut self) {
        self.packets.clear();
    }
}

// ---------------------------------------------------------------------------
// Browser transport (wasm32 only)
// ---------------------------------------------------------------------------

/// What one [`poll`] of a browser socket found.
#[cfg(target_arch = "wasm32")]
#[derive(Debug, Clone)]
pub struct BrowserPoll {
    /// The socket's live `readyState`, mapped by [`ready_state_from_browser`].
    pub state: WebSocketState,
    /// Packets the `onmessage` closure queued since the previous poll.
    pub packets: Vec<(Vec<u8>, bool)>,
    /// `CloseEvent.code`, or `-1` when no close event has arrived yet.
    pub close_code: i32,
    /// `CloseEvent.reason`, empty when none was given.
    pub close_reason: String,
}

#[cfg(target_arch = "wasm32")]
mod browser {
    use super::{ready_state_from_browser, BrowserPacketQueue, BrowserPoll, READY_STATE_CLOSED};

    use std::cell::{Cell, RefCell};
    use std::collections::HashMap;
    use std::rc::Rc;

    use wasm_bindgen::prelude::Closure;
    use wasm_bindgen::{JsCast, JsValue};

    /// State the event closures write and [`poll`] reads.
    struct Shared {
        queue: BrowserPacketQueue,
        close_code: i32,
        close_reason: String,
    }

    /// One live browser `WebSocket` plus the closures JS holds for it.
    struct BrowserSocket {
        socket: JsValue,
        shared: Rc<RefCell<Shared>>,
        /// ⚠️ Kept alive for exactly as long as the socket. JS holds these
        /// functions as `onmessage`/`onclose`/`onerror`; dropping a `Closure`
        /// frees the Rust box it wraps, and the next event would then call
        /// through a dangling pointer. `js_bridge.rs` keeps its callback
        /// closures alive for the lifetime of the module for the same reason.
        _closures: Vec<Closure<dyn FnMut(JsValue)>>,
    }

    thread_local! {
        static SOCKETS: RefCell<HashMap<u64, BrowserSocket>> = RefCell::new(HashMap::new());
        static NEXT_HANDLE: Cell<u64> = Cell::new(1);
    }

    /// Renders a thrown JS value as something a `WebSocketError` can carry.
    fn describe(e: JsValue) -> String {
        e.as_string()
            .or_else(|| js_sys::JSON::stringify(&e).ok().map(String::from))
            .unwrap_or_else(|| "unknown JavaScript error".to_string())
    }

    fn get(target: &JsValue, property: &str) -> Result<JsValue, String> {
        js_sys::Reflect::get(target, &JsValue::from_str(property)).map_err(describe)
    }

    fn set(target: &JsValue, property: &str, value: &JsValue) -> Result<(), String> {
        js_sys::Reflect::set(target, &JsValue::from_str(property), value)
            .map(|_| ())
            .map_err(describe)
    }

    /// `target.method(...args)`, with `target` as the receiver.
    fn call(target: &JsValue, method: &str, args: &js_sys::Array) -> Result<JsValue, String> {
        let f: js_sys::Function = get(target, method)?
            .dyn_into()
            .map_err(|_| format!("WebSocket has no method '{method}'"))?;
        js_sys::Reflect::apply(&f, target, args).map_err(describe)
    }

    /// Opens a browser `WebSocket` and returns its registry handle.
    ///
    /// `protocols` becomes the constructor's second argument (the subprotocol
    /// list), which is how a Web export offers `Sec-WebSocket-Protocol`: the
    /// browser owns the handshake, so the header cannot be written directly.
    /// ⚠️ For the same reason Godot documents `handshake_headers` as having **no
    /// effect** in a Web export — there is no API to add one — so this function
    /// takes no headers rather than accepting and discarding them.
    ///
    /// Both `ws://` and `wss://` are passed through untouched: the browser does
    /// the TLS for `wss://`.
    pub fn connect(
        url: &str,
        protocols: &[String],
        max_queued_packets: usize,
    ) -> Result<u64, String> {
        let global = js_sys::global();
        let ctor: js_sys::Function = get(&global, "WebSocket")?
            .dyn_into()
            .map_err(|_| "this environment has no global WebSocket constructor".to_string())?;

        let args = js_sys::Array::new();
        args.push(&JsValue::from_str(url));
        if !protocols.is_empty() {
            let list = js_sys::Array::new();
            for p in protocols {
                list.push(&JsValue::from_str(p));
            }
            args.push(&list);
        }
        let socket = js_sys::Reflect::construct(&ctor, &args).map_err(describe)?;

        // ⚠️ Without this, binary frames arrive as a `Blob`, whose contents are
        // only readable asynchronously — a packet queue drained synchronously
        // from `poll()` would never see the bytes. `"arraybuffer"` makes
        // `MessageEvent.data` readable in the handler itself, which is also what
        // `library_godot_websocket.js` sets.
        set(&socket, "binaryType", &JsValue::from_str("arraybuffer"))?;

        let shared = Rc::new(RefCell::new(Shared {
            queue: BrowserPacketQueue::with_max(max_queued_packets),
            close_code: -1,
            close_reason: String::new(),
        }));

        let mut closures: Vec<Closure<dyn FnMut(JsValue)>> = Vec::new();

        {
            let shared = Rc::clone(&shared);
            let on_message = Closure::wrap(Box::new(move |event: JsValue| {
                let data = get(&event, "data").unwrap_or(JsValue::UNDEFINED);
                let mut s = shared.borrow_mut();
                if let Some(text) = data.as_string() {
                    s.queue.push_text(&text);
                } else if let Ok(buf) = data.clone().dyn_into::<js_sys::ArrayBuffer>() {
                    s.queue.push_binary(js_sys::Uint8Array::new(&buf).to_vec());
                } else if let Ok(view) = data.dyn_into::<js_sys::Uint8Array>() {
                    // Not reachable with `binaryType = "arraybuffer"`, but a
                    // view is what a shimmed socket in a test harness hands
                    // over, and dropping it silently would look like packet
                    // loss.
                    s.queue.push_binary(view.to_vec());
                }
            }) as Box<dyn FnMut(JsValue)>);
            set(&socket, "onmessage", on_message.as_ref())?;
            closures.push(on_message);
        }

        {
            let shared = Rc::clone(&shared);
            let on_close = Closure::wrap(Box::new(move |event: JsValue| {
                // 1005 — "no status code present" (RFC 6455 §7.4.1) — is what a
                // `CloseEvent` without a code means, matching
                // `parse_close_payload` on the native path.
                let code = get(&event, "code")
                    .ok()
                    .and_then(|v| v.as_f64())
                    .map(|v| v as i32)
                    .unwrap_or(1005);
                let reason = get(&event, "reason")
                    .ok()
                    .and_then(|v| v.as_string())
                    .unwrap_or_default();
                let mut s = shared.borrow_mut();
                s.close_code = code;
                s.close_reason = reason;
            }) as Box<dyn FnMut(JsValue)>);
            set(&socket, "onclose", on_close.as_ref())?;
            closures.push(on_close);
        }

        {
            let shared = Rc::clone(&shared);
            let on_error = Closure::wrap(Box::new(move |_event: JsValue| {
                // ⚠️ The browser deliberately withholds the reason for a failed
                // WebSocket connection (it would leak cross-origin
                // information), so `onerror` carries nothing useful. A `close`
                // event always follows; this only pre-seeds 1006 — "abnormal
                // closure" — in case it arrives without a code, which is the
                // same code the native path reports for a dropped connection.
                let mut s = shared.borrow_mut();
                if s.close_code < 0 {
                    s.close_code = 1006;
                }
            }) as Box<dyn FnMut(JsValue)>);
            set(&socket, "onerror", on_error.as_ref())?;
            closures.push(on_error);
        }

        let handle = NEXT_HANDLE.with(|n| {
            let id = n.get();
            n.set(id.saturating_add(1));
            id
        });
        SOCKETS.with(|s| {
            s.borrow_mut().insert(
                handle,
                BrowserSocket {
                    socket,
                    shared,
                    _closures: closures,
                },
            )
        });
        Ok(handle)
    }

    /// Reads the socket's state and takes everything its handlers have queued.
    ///
    /// Returns `None` for a handle that was never issued or has been released.
    pub fn poll(handle: u64) -> Option<BrowserPoll> {
        SOCKETS.with(|sockets| {
            let sockets = sockets.borrow();
            let entry = sockets.get(&handle)?;
            let raw = get(&entry.socket, "readyState")
                .ok()
                .and_then(|v| v.as_f64())
                .map(|v| v as i32)
                .unwrap_or(READY_STATE_CLOSED);
            let mut shared = entry.shared.borrow_mut();
            Some(BrowserPoll {
                state: ready_state_from_browser(raw),
                packets: shared.queue.drain(),
                close_code: shared.close_code,
                close_reason: shared.close_reason.clone(),
            })
        })
    }

    /// Sends one message. `text` picks `send(string)` over `send(ArrayBuffer)`.
    pub fn send(handle: u64, data: &[u8], text: bool) -> Result<(), String> {
        SOCKETS.with(|sockets| {
            let sockets = sockets.borrow();
            let entry = sockets
                .get(&handle)
                .ok_or_else(|| "WebSocket handle is no longer live".to_string())?;
            let args = js_sys::Array::new();
            if text {
                // Lossy on purpose: a text frame MUST be valid UTF-8 (RFC 6455
                // §5.6), and `send_text` is the only caller that sets `text`.
                args.push(&JsValue::from_str(&String::from_utf8_lossy(data)));
            } else {
                args.push(&js_sys::Uint8Array::from(data));
            }
            call(&entry.socket, "send", &args).map(|_| ())
        })
    }

    /// `WebSocket.close(code, reason)`.
    ///
    /// ⚠️ The browser throws on a code outside 1000 or 3000..=4999, so an
    /// out-of-range code is sent as a bare `close()` rather than letting the
    /// exception escape into the caller's close path.
    pub fn close(handle: u64, code: u16, reason: &str) -> Result<(), String> {
        SOCKETS.with(|sockets| {
            let sockets = sockets.borrow();
            let entry = sockets
                .get(&handle)
                .ok_or_else(|| "WebSocket handle is no longer live".to_string())?;
            let args = js_sys::Array::new();
            if code == 1000 || (3000..=4999).contains(&code) {
                args.push(&JsValue::from_f64(f64::from(code)));
                if !reason.is_empty() {
                    args.push(&JsValue::from_str(reason));
                }
            }
            call(&entry.socket, "close", &args).map(|_| ())
        })
    }

    /// `WebSocket.bufferedAmount` — bytes queued by the browser but not yet
    /// written to the network.
    ///
    /// This is the one platform where Godot's
    /// `get_current_outbound_buffered_amount` reports something other than the
    /// native build's `0`: the browser owns the outbound queue.
    pub fn buffered_amount(handle: u64) -> i64 {
        SOCKETS.with(|sockets| {
            sockets
                .borrow()
                .get(&handle)
                .and_then(|entry| get(&entry.socket, "bufferedAmount").ok())
                .and_then(|v| v.as_f64())
                .map(|v| v as i64)
                .unwrap_or(0)
        })
    }

    /// `WebSocket.protocol` — the subprotocol the server accepted.
    ///
    /// Empty until the handshake completes, which is why the caller reads it on
    /// the transition to OPEN rather than at connect time. The browser performs
    /// the negotiation, so this is the only way a Web export can answer Godot's
    /// `WebSocketPeer.get_selected_protocol()`.
    pub fn selected_protocol(handle: u64) -> String {
        SOCKETS.with(|sockets| {
            sockets
                .borrow()
                .get(&handle)
                .and_then(|entry| get(&entry.socket, "protocol").ok())
                .and_then(|v| v.as_string())
                .unwrap_or_default()
        })
    }

    /// Re-caps the inbound queue behind `handle`.
    pub fn set_max_queued_packets(handle: u64, max: usize) {
        SOCKETS.with(|sockets| {
            if let Some(entry) = sockets.borrow().get(&handle) {
                entry.shared.borrow_mut().queue.set_max_queued_packets(max);
            }
        });
    }

    /// Drops the socket and its closures. Idempotent.
    ///
    /// ⚠️ Must be called for every handle [`connect`] returns, or the registry
    /// keeps the JS socket (and its event closures) alive for the lifetime of
    /// the page — a leak the browser cannot collect, because JS still holds the
    /// handler functions.
    pub fn release(handle: u64) {
        SOCKETS.with(|sockets| sockets.borrow_mut().remove(&handle));
    }

    /// How many sockets the registry currently holds.
    pub fn live_socket_count() -> usize {
        SOCKETS.with(|sockets| sockets.borrow().len())
    }
}

#[cfg(target_arch = "wasm32")]
pub use browser::{
    buffered_amount, close, connect, live_socket_count, poll, release, selected_protocol, send,
    set_max_queued_packets,
};
