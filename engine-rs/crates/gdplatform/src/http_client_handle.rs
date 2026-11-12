//! pat-s3rrv.78 — the script-visible `HTTPClient`.
//!
//! `HTTPClient` was registered in ClassDB (`class_db.rs`, the HTTP block) as a
//! `RefCounted` carrying ten methods, with **no dispatch anywhere**: no
//! `HTTPClient.new()` arm in the interpreter, no handle table, no method
//! routing. `has_method` therefore answered `true` for all ten while every call
//! returned `Nil` — the bead's own "looks supported, does nothing" failure, and
//! the reason its 639-line [`crate::http_client`] had no consumer.
//!
//! ⚠️ `HTTPClient` is `RefCounted`, **not a Node**, so it cannot ride the scene
//! tree's node-method table. It needs the handle pattern, mirrored here from
//! [`crate::packet_peer_stream`]: a tagged `ObjectId` into a process-wide table.
//!
//! # The blocking/polling mismatch
//!
//! 🔑 Godot's `HTTPClient` is **poll-driven**; [`HttpClient`] is **blocking**.
//! There is no way to present a truthful poll surface over it, so this wrapper
//! does the work eagerly and shapes the *readout* like a poll API:
//!
//! - `connect_to_host` connects synchronously, landing on `CONNECTED`.
//! - `poll` is a no-op returning `OK` — there is genuinely nothing to advance.
//! - `request` writes **and reads the whole response**, landing on `BODY`.
//! - `read_response_body_chunk` hands out successive slices of those bytes.
//!
//! ⚠️ A script writing `while c.get_status() == CONNECTING: c.poll()` simply
//! never loops. That is coarse but not false: every status reported is one
//! Godot really produces, and each is correct where it is reported. Nothing here
//! claims to be non-blocking, which is the one thing that would be a lie.

use std::collections::HashMap;

use crate::http_client::{HttpClient, HttpClientStatus};
use crate::http_request::{HttpMethod, HttpResultCode};

// ---------------------------------------------------------------- constants

/// Godot's `Error` ordinals, for the codes this surface can return.
///
/// ⚠️ Redeclared rather than imported: the workspace's table lives in
/// `gdscript-interop/src/global_constants.rs`, and `gdplatform` must not depend
/// on `gdscript-interop` — that is the wrong direction. Values transcribed from
/// `upstream/godot/core/error/error_list.h`, where `ERR_CANT_CONNECT` carries an
/// explicit `// (25)` anchor, and cross-checked against that table.
pub const OK: i32 = 0;
/// See [`OK`]. Returned when TLS is requested — there is no TLS backend.
pub const ERR_UNAVAILABLE: i32 = 2;
/// See [`OK`]. Returned when a call needs a connection that was never made.
pub const ERR_UNCONFIGURED: i32 = 3;
/// See [`OK`].
pub const ERR_CANT_CONNECT: i32 = 25;
/// See [`OK`].
pub const ERR_CANT_RESOLVE: i32 = 26;
/// See [`OK`].
pub const ERR_CONNECTION_ERROR: i32 = 27;
/// See [`OK`].
pub const ERR_INVALID_PARAMETER: i32 = 31;

/// Godot's `HTTPClient.read_chunk_size` default (`http_client.h`).
pub const DEFAULT_READ_CHUNK_SIZE: usize = 65536;

/// Maps a transport failure onto the Godot `Error` a script expects.
///
/// ⚠️ `SslHandshakeError` becomes `ERR_UNAVAILABLE`, not `ERR_CONNECTION_ERROR`:
/// nothing went wrong on the wire, the feature simply is not built. Reporting a
/// connection error would send a script debugging its network.
fn error_for(code: HttpResultCode) -> i32 {
    match code {
        HttpResultCode::Success | HttpResultCode::InProgress => OK,
        HttpResultCode::CantResolve => ERR_CANT_RESOLVE,
        HttpResultCode::CantConnect => ERR_CANT_CONNECT,
        HttpResultCode::SslHandshakeError => ERR_UNAVAILABLE,
        HttpResultCode::ConnectionError
        | HttpResultCode::NoResponse
        | HttpResultCode::Timeout
        | HttpResultCode::RequestFailed
        | HttpResultCode::RedirectLimitReached => ERR_CONNECTION_ERROR,
    }
}

/// Maps a Godot `HTTPClient.Method` ordinal onto a transport method.
///
/// ⚠️ Transcribed positionally from `core/io/http_client.h`'s `enum Method`:
/// GET 0, HEAD 1, POST 2, PUT 3, DELETE 4, OPTIONS 5, TRACE 6, CONNECT 7,
/// **PATCH 8**. Deriving the order from the more familiar
/// GET/POST/PUT/DELETE/PATCH listing puts PATCH at 6 — which is TRACE — so a
/// script's `HTTPClient.METHOD_PATCH` would silently send a TRACE.
///
/// `None` for OPTIONS/TRACE/CONNECT: there is no transport variant for them, and
/// silently downgrading to GET would look like a successful request while doing
/// something else entirely.
///
/// 🔑 **This is the single copy.** `gdscene::http_request_node::method_from_int`
/// used to repeat the same table; it now delegates here, because `HttpMethod` is
/// defined in this crate. Do not re-inline it there — two ordinal tables drift
/// silently, and the value that drifts is PATCH.
pub fn method_from_ordinal(ordinal: i64) -> Option<HttpMethod> {
    match ordinal {
        0 => Some(HttpMethod::Get),
        1 => Some(HttpMethod::Head),
        2 => Some(HttpMethod::Post),
        3 => Some(HttpMethod::Put),
        4 => Some(HttpMethod::Delete),
        8 => Some(HttpMethod::Patch),
        _ => None,
    }
}

// ------------------------------------------------------------------- client

/// One script-owned `HTTPClient`: the transport plus the readout cursor Godot's
/// chunked-body API needs.
pub struct ScriptHttpClient {
    inner: HttpClient,
    /// The response body, captured whole when the request ran.
    body: Vec<u8>,
    /// How much of `body` `read_response_body_chunk` has already handed out.
    cursor: usize,
    has_response: bool,
    read_chunk_size: usize,
    blocking_mode: bool,
}

impl Default for ScriptHttpClient {
    fn default() -> Self {
        Self::new()
    }
}

impl ScriptHttpClient {
    /// A disconnected client, as `HTTPClient.new()` produces.
    pub fn new() -> Self {
        Self {
            inner: HttpClient::new(),
            body: Vec::new(),
            cursor: 0,
            has_response: false,
            read_chunk_size: DEFAULT_READ_CHUNK_SIZE,
            blocking_mode: false,
        }
    }

    /// `connect_to_host(host, port, tls_options)`.
    ///
    /// ⚠️ `port <= 0` means "the scheme default", which is Godot's behaviour for
    /// the documented `port = -1` default. Only 80 is reachable here, since
    /// `use_tls` cannot succeed.
    pub fn connect_to_host(&mut self, host: &str, port: i32, use_tls: bool) -> i32 {
        if host.is_empty() {
            return ERR_INVALID_PARAMETER;
        }
        if use_tls {
            // Honest refusal rather than a plaintext connection to an https
            // port, which would look like it worked and then serve garbage.
            return ERR_UNAVAILABLE;
        }
        let port = if port <= 0 { 80 } else { port };
        let Ok(port) = u16::try_from(port) else {
            return ERR_INVALID_PARAMETER;
        };
        // A fresh connection invalidates any previous response.
        self.reset_response();
        match self.inner.connect_to_host(host, port) {
            Ok(()) => OK,
            Err(code) => error_for(code),
        }
    }

    /// `poll()` — a no-op that reports success.
    ///
    /// 🔑 There is nothing to advance: the transport completed its work inside
    /// [`connect_to_host`](Self::connect_to_host) and [`request`](Self::request).
    /// Returning `OK` keeps a Godot-shaped `while poll() == OK` loop from
    /// spinning forever on a false error.
    pub fn poll(&mut self) -> i32 {
        OK
    }

    /// `request(method, url, headers, body)`.
    ///
    /// `url` is the request path (`/index.html`), matching Godot — the host was
    /// fixed by `connect_to_host`. `headers` are `"Name: Value"` lines.
    pub fn request(
        &mut self,
        method: HttpMethod,
        url: &str,
        headers: &[String],
        body: &[u8],
    ) -> i32 {
        if self.inner.status() != HttpClientStatus::Connected {
            // Godot's own guard is `status != STATUS_CONNECTED`.
            return ERR_UNCONFIGURED;
        }
        self.reset_response();

        let mut map: HashMap<String, String> = HashMap::new();
        for line in headers {
            // Split on the FIRST colon: header values contain colons of their
            // own (Date, any URL).
            if let Some((key, value)) = line.split_once(':') {
                map.insert(key.trim().to_string(), value.trim().to_string());
            }
        }

        if let Err(code) = self.inner.request(method, url, &map, body) {
            return error_for(code);
        }
        // ⚠️ Read eagerly. Godot would leave this to `poll()`, but our transport
        // has no partial-read state to expose, so deferring it would just mean
        // `has_response()` never turning true.
        if let Err(code) = self.inner.read_response() {
            return error_for(code);
        }
        self.body = self.inner.response_body().to_vec();
        self.has_response = true;
        OK
    }

    /// `get_status()` — the Godot `HTTPClient.Status` ordinal.
    pub fn get_status(&self) -> i32 {
        self.inner.status().as_godot_code()
    }

    /// `has_response()`.
    pub fn has_response(&self) -> bool {
        self.has_response
    }

    /// `get_response_code()`.
    pub fn get_response_code(&self) -> i32 {
        i32::from(self.inner.response_code())
    }

    /// `get_response_headers()` — `"Name: Value"` lines, as Godot delivers them.
    pub fn get_response_headers(&self) -> Vec<String> {
        self.inner
            .response_headers()
            .iter()
            .map(|(name, value)| format!("{name}: {value}"))
            .collect()
    }

    /// `get_response_headers_as_dictionary()`.
    pub fn get_response_headers_as_dictionary(&self) -> HashMap<String, String> {
        self.inner.response_headers().iter().cloned().collect()
    }

    /// `get_response_body_length()`.
    ///
    /// ⚠️ The FULL body length, not what is left to read. Godot reports
    /// `Content-Length`, which does not shrink as chunks are consumed — a script
    /// sizing a progress bar from this must keep getting the same denominator.
    pub fn get_response_body_length(&self) -> i64 {
        self.body.len() as i64
    }

    /// `read_response_body_chunk()` — the next up-to-`read_chunk_size` bytes.
    ///
    /// Returns empty once drained, which is how a Godot read loop terminates.
    pub fn read_response_body_chunk(&mut self) -> Vec<u8> {
        if !self.has_response || self.cursor >= self.body.len() {
            return Vec::new();
        }
        let end = self
            .body
            .len()
            .min(self.cursor.saturating_add(self.read_chunk_size));
        let chunk = self.body[self.cursor..end].to_vec();
        self.cursor = end;
        chunk
    }

    /// `set_read_chunk_size(bytes)`.
    ///
    /// ⚠️ A zero or negative size is ignored rather than stored: honouring it
    /// would make [`read_response_body_chunk`](Self::read_response_body_chunk)
    /// return empty forever, which a read loop cannot distinguish from "done"
    /// and so would silently truncate every body to nothing.
    pub fn set_read_chunk_size(&mut self, bytes: i64) {
        if bytes > 0 {
            self.read_chunk_size = bytes as usize;
        }
    }

    /// `get_read_chunk_size()`.
    pub fn get_read_chunk_size(&self) -> i64 {
        self.read_chunk_size as i64
    }

    /// `blocking_mode_enabled` — stored and reported back.
    ///
    /// ⚠️ Observational only, and deliberately so: this transport is *always*
    /// blocking, so the property cannot change behaviour. It round-trips because
    /// Godot's property contract says it must, and the mismatch is documented
    /// here rather than hidden.
    pub fn set_blocking_mode(&mut self, enabled: bool) {
        self.blocking_mode = enabled;
    }

    /// See [`set_blocking_mode`](Self::set_blocking_mode).
    pub fn is_blocking_mode_enabled(&self) -> bool {
        self.blocking_mode
    }

    /// `close()`.
    pub fn close(&mut self) {
        self.inner.close();
        self.reset_response();
    }

    fn reset_response(&mut self) {
        self.body.clear();
        self.cursor = 0;
        self.has_response = false;
    }
}

// ------------------------------------------------------------------ handles
//
// The live clients sit in a process-wide table and scripts hold a tagged
// `ObjectId` into it.

/// Tags an `ObjectId` issued by [`new_http_client_handle`]. `0x48` is ASCII `H`,
/// beside `0x57` (`W`, WebSocket) and `0x5B` (StreamPeer).
///
/// ⚠️ THE TAG BYTE IS A WORKSPACE-WIDE NAMESPACE. Two modules issuing the same
/// top byte makes `is_*_handle` answer true for the other module's ids, and the
/// symptom is a `MethodNotFound` naming the WRONG class. `0x48` was chosen after
/// sweeping the workspace two ways — by value shape
/// (`0x[0-9A-Fa-f]{2}00_0000_0000_0000`) **and** by declaration shape
/// (`const [A-Z_]*(TAG|HANDLE)[A-Z_]*: u64`). Neither sweep alone is complete:
/// the first misses `0xA7_00_0000_0000_0000` (different underscore grouping),
/// the second misses tags declared in `bindings.rs`.
const HTTP_CLIENT_HANDLE_TAG: u64 = 0x4800_0000_0000_0000;

static NEXT_HTTP_CLIENT_HANDLE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

static HTTP_CLIENT_HANDLES: std::sync::Mutex<
    Option<std::collections::HashMap<u64, ScriptHttpClient>>,
> = std::sync::Mutex::new(None);

fn http_client_handles(
) -> std::sync::MutexGuard<'static, Option<std::collections::HashMap<u64, ScriptHttpClient>>> {
    // A poisoned lock still holds a usable table: a panic in one script's
    // handler must not make every later call fail.
    HTTP_CLIENT_HANDLES
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Returns `true` when `id` was issued by [`new_http_client_handle`].
pub fn is_http_client_handle(id: gdcore::id::ObjectId) -> bool {
    id.raw() & 0xFF00_0000_0000_0000 == HTTP_CLIENT_HANDLE_TAG
}

/// Allocates a fresh client and returns its script-visible id
/// (Godot `HTTPClient.new()`).
pub fn new_http_client_handle() -> gdcore::id::ObjectId {
    let seq = NEXT_HTTP_CLIENT_HANDLE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let raw = HTTP_CLIENT_HANDLE_TAG | seq;
    http_client_handles()
        .get_or_insert_with(std::collections::HashMap::new)
        .insert(raw, ScriptHttpClient::new());
    gdcore::id::ObjectId::from_raw(raw)
}

/// Runs `f` against the live client behind `id`, or returns `None` when the
/// handle was never issued or has already been released.
pub fn with_http_client<R>(
    id: gdcore::id::ObjectId,
    f: impl FnOnce(&mut ScriptHttpClient) -> R,
) -> Option<R> {
    let mut guard = http_client_handles();
    let map = guard.get_or_insert_with(std::collections::HashMap::new);
    map.get_mut(&id.raw()).map(f)
}

/// Drops a handle. Idempotent.
pub fn release_http_client_handle(id: gdcore::id::ObjectId) {
    let mut guard = http_client_handles();
    if let Some(map) = guard.as_mut() {
        map.remove(&id.raw());
    }
}
