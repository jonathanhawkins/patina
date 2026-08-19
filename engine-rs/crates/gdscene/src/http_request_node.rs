//! pat-s3rrv.78 — the scene-side of Godot's `HTTPRequest` node.
//!
//! Godot ground truth: `HTTPRequest` is a **Node**. A script writes
//! ```gdscript
//! var r = HTTPRequest.new()
//! add_child(r)
//! r.request_completed.connect(_on_done)
//! r.request(url)
//! ```
//! and the node polls its client every frame, emitting
//! `request_completed(result, response_code, headers, body)` when the transport
//! reaches Done.
//!
//! ⚠️ `gdplatform::http_request::HTTPRequest` — 602 lines of working transport —
//! had **zero** consumers in the scene tree, the interpreter or the runner, while
//! `HTTPRequest` **was** registered in ClassDB. That is the worst shape of gap:
//! a `.tscn` carrying `type="HTTPRequest"` loads, reports the right class, and
//! does nothing. This module is the missing middle, and deliberately nothing
//! else — the transport is not reimplemented here.
//!
//! ⚠️ The platform type emits through Rust callbacks, not through the scene's
//! signal system, so a callback registered at construction parks each payload in
//! a queue that [`SceneTree`](crate::scene_tree::SceneTree) drains on the frame
//! tick. Emitting directly from the callback is not an option: it fires on the
//! transport's background thread, and `emit_signal` runs script code that must
//! only ever touch the tree from the main thread.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use gdplatform::http_request::{HTTPRequest, HttpMethod, HttpRequestConfig, RequestCompleted};

/// The per-request settings Godot keeps as HTTPRequest node properties.
///
/// Carried as a value rather than stored on [`HttpRequestNode`] so the node
/// properties stay the single source of truth — see [`HttpRequestNode::config_for`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RequestSettings {
    /// `HTTPRequest.timeout` in seconds; `0.0` never times out.
    pub timeout: f64,
    /// `HTTPRequest.max_redirects`; **negative means unlimited**.
    pub max_redirects: i64,
    /// `HTTPRequest.accept_gzip`; **defaults to `true`** (`HTTPRequest.xml:236`).
    ///
    /// Request-side it adds `Accept-Encoding`; response-side the matching field
    /// on [`DeliverySettings`] decides whether to decode. Both must agree, which
    /// they do because both read the same node property.
    pub accept_gzip: bool,
}

impl Default for RequestSettings {
    fn default() -> Self {
        Self {
            timeout: DEFAULT_TIMEOUT,
            max_redirects: DEFAULT_MAX_REDIRECTS,
            accept_gzip: DEFAULT_ACCEPT_GZIP,
        }
    }
}

/// Godot `HTTPRequest.Result` codes that the NODE produces and the transport
/// never can (`scene/main/http_request.h`, `enum Result`).
///
/// ⚠️ There is deliberately no `HttpResultCode` variant for these. The transport
/// is never told about a body-size limit or a download path — both are node
/// PROPERTIES enforced in `http_request.cpp`, so a request that exceeds the
/// limit is a perfectly successful HTTP exchange that the node then rejects.
/// The ordinals are positional in the upstream enum and cannot be counted from
/// this crate's own codes, which omit six of Godot's fourteen.
pub const RESULT_BODY_SIZE_LIMIT_EXCEEDED: i32 = 7;
/// See [`RESULT_BODY_SIZE_LIMIT_EXCEEDED`].
pub const RESULT_BODY_DECOMPRESS_FAILED: i32 = 8;
/// See [`RESULT_BODY_SIZE_LIMIT_EXCEEDED`].
pub const RESULT_DOWNLOAD_FILE_CANT_OPEN: i32 = 10;
/// See [`RESULT_BODY_SIZE_LIMIT_EXCEEDED`].
pub const RESULT_DOWNLOAD_FILE_WRITE_ERROR: i32 = 11;
/// `RESULT_SUCCESS`, the only result the post-processing below acts on.
const RESULT_SUCCESS: i32 = 0;

/// Godot's `HTTPRequest.body_size_limit` default (`HTTPRequest.xml:242`).
///
/// ⚠️ **Negative means no limit**, the same shape as `max_redirects` — and the
/// default is `-1`, so the guard is off unless a project turns it on.
pub const DEFAULT_BODY_SIZE_LIMIT: i64 = -1;

/// The HTTPRequest node properties that shape DELIVERY rather than the request.
///
/// ⚠️ Kept separate from [`RequestSettings`] on purpose. Those two fields reach
/// the transport through [`HttpRequestNode::config_for`]; these two never do —
/// the transport has no idea either exists. Folding them together would also
/// cost `RequestSettings` its `Copy`, since `download_file` is a `String`.
#[derive(Debug, Clone, PartialEq)]
pub struct DeliverySettings {
    /// `HTTPRequest.body_size_limit` in bytes; **negative means unlimited**.
    pub body_size_limit: i64,
    /// `HTTPRequest.download_file`; empty means "deliver the body in the signal".
    pub download_file: String,
    /// `HTTPRequest.accept_gzip`. See [`RequestSettings::accept_gzip`].
    ///
    /// ⚠️ Gates the RESPONSE decode as well as the request header: upstream reads
    /// `Content-Encoding` only `if (accept_gzip)` (`http_request.cpp:308`), so a
    /// server that compresses anyway is delivered still-compressed rather than
    /// silently decoded. Turning the property off must mean "hands off".
    pub accept_gzip: bool,
}

impl Default for DeliverySettings {
    fn default() -> Self {
        Self {
            body_size_limit: DEFAULT_BODY_SIZE_LIMIT,
            download_file: String::new(),
            accept_gzip: DEFAULT_ACCEPT_GZIP,
        }
    }
}

/// Reads [`DeliverySettings`] off the node, the way [`RequestSettings`] is read.
///
/// Single reader for the same reason `http_settings` is: a `.tscn` authoring
/// `body_size_limit = 64` and a script calling `set_body_size_limit(64)` must
/// land in one place, or the two disagree silently.
pub fn delivery_settings(node: &crate::node::Node) -> DeliverySettings {
    DeliverySettings {
        body_size_limit: match node.get_property("body_size_limit") {
            gdvariant::Variant::Int(i) => i,
            gdvariant::Variant::Float(f) => f as i64,
            _ => DEFAULT_BODY_SIZE_LIMIT,
        },
        download_file: match node.get_property("download_file") {
            gdvariant::Variant::String(s) => s,
            _ => String::new(),
        },
        accept_gzip: match node.get_property("accept_gzip") {
            gdvariant::Variant::Bool(b) => b,
            // ⚠️ Absent means the DEFAULT, which is `true` — not `false`. A
            // `bool` property that falls back to `Default::default()` would
            // silently disable compression on every node a `.tscn` does not
            // mention, which is all of them.
            _ => DEFAULT_ACCEPT_GZIP,
        },
    }
}

/// The value of `name` among `"Name: Value"` response header lines.
///
/// ⚠️ Split on the FIRST colon: header values legitimately contain colons
/// (`Date`, any URL), so splitting from the right would return a fragment.
fn header_value<'a>(headers: &'a [String], name: &str) -> Option<&'a str> {
    headers.iter().find_map(|line| {
        let (key, value) = line.split_once(':')?;
        key.trim().eq_ignore_ascii_case(name).then(|| value.trim())
    })
}

/// Advances past a NUL-terminated field, returning the index after the NUL.
fn skip_cstr(bytes: &[u8], from: usize) -> Option<usize> {
    let end = bytes.get(from..)?.iter().position(|b| *b == 0)?;
    from.checked_add(end)?.checked_add(1)
}

/// The raw DEFLATE payload inside an RFC 1952 gzip member.
///
/// ⚠️ `miniz_oxide` has no gzip entry point — it decodes raw DEFLATE and
/// zlib-wrapped DEFLATE only. gzip's variable-length header (optional extra
/// field, file name, comment and header CRC, in that order) has to be walked
/// here, and the 8-byte CRC32+ISIZE trailer sliced off, or the decoder is handed
/// bytes that are not a DEFLATE stream.
///
/// Returns `None` for anything malformed rather than guessing; the caller turns
/// that into `RESULT_BODY_DECOMPRESS_FAILED`.
fn gzip_deflate_payload(bytes: &[u8]) -> Option<&[u8]> {
    // 10-byte fixed header + 8-byte trailer is the smallest possible member.
    if bytes.len() < 18 || bytes[0] != 0x1f || bytes[1] != 0x8b || bytes[2] != 8 {
        return None;
    }
    let flags = bytes[3];
    let mut pos = 10usize;
    if flags & 0x04 != 0 {
        // FEXTRA: a little-endian u16 length followed by that many bytes.
        let len = u16::from_le_bytes([*bytes.get(pos)?, *bytes.get(pos + 1)?]) as usize;
        pos = pos.checked_add(2)?.checked_add(len)?;
    }
    if flags & 0x08 != 0 {
        pos = skip_cstr(bytes, pos)?; // FNAME
    }
    if flags & 0x10 != 0 {
        pos = skip_cstr(bytes, pos)?; // FCOMMENT
    }
    if flags & 0x02 != 0 {
        pos = pos.checked_add(2)?; // FHCRC
    }
    let end = bytes.len().checked_sub(8)?;
    if pos > end {
        return None;
    }
    Some(&bytes[pos..end])
}

/// The gzip trailer's ISIZE field: the uncompressed length, modulo 2^32.
fn gzip_isize(bytes: &[u8]) -> Option<u32> {
    let tail = bytes.get(bytes.len().checked_sub(4)?..)?;
    Some(u32::from_le_bytes([tail[0], tail[1], tail[2], tail[3]]))
}

/// The outcome of decoding a `Content-Encoding` body.
///
/// Three-way rather than `Option` because "too large" and "malformed" are
/// different Godot results (`RESULT_BODY_SIZE_LIMIT_EXCEEDED` vs
/// `RESULT_BODY_DECOMPRESS_FAILED`), and collapsing them would report a
/// perfectly valid response as corrupt.
#[derive(Debug)]
pub enum Decoded {
    /// Decoded bytes.
    Body(Vec<u8>),
    /// Decoding stopped because the output reached the body-size limit.
    TooLarge,
    /// The body is not valid for the encoding it claimed.
    Failed,
}

/// Decodes a `Content-Encoding` body, bounded by `body_size_limit`.
///
/// 🔑 **HTTP `deflate` is ZLIB-WRAPPED, not raw DEFLATE**, despite the name.
/// Godot picks the zlib window bits for it and the gzip ones for `gzip`:
/// `window_bits = p_is_deflate ? 15 : (15 + 16)` (`stream_peer_gzip.cpp:_start`),
/// where 15 is zlib and 15+16 is gzip. Reading `deflate` as raw — the reading
/// the name invites — fails on every real server, and only on real servers, so
/// a hand-rolled raw fixture would make the wrong version look correct.
///
/// 🔑 **The limit is enforced BY the decoder, not after it.** `miniz_oxide`'s
/// own docs say the unbounded `decompress_to_vec` "can result in an out of
/// memory error"; upstream never has the problem because it decompresses in
/// chunks and checks after each one, with an explicit comment that a "zip bomb
/// could result in a chunk of few kilos decompressing into gigabytes"
/// (`http_request.cpp:453`). Decoding first and measuring afterwards would
/// implement the check while removing the protection it exists to give — the
/// process dies before the check runs.
///
/// The cap is `limit + 1`, so a body of exactly the limit still decodes (upstream
/// refuses only what is strictly greater) and one byte more is caught here.
pub fn decompress_body(body: &[u8], encoding: &str, body_size_limit: i64) -> Decoded {
    use miniz_oxide::inflate::{
        decompress_to_vec_with_limit, decompress_to_vec_zlib_with_limit, TINFLStatus,
    };

    let max = if body_size_limit >= 0 {
        (body_size_limit as usize).saturating_add(1)
    } else {
        usize::MAX
    };

    let decoded = match encoding {
        "gzip" => match gzip_deflate_payload(body) {
            Some(raw) => decompress_to_vec_with_limit(raw, max),
            None => return Decoded::Failed,
        },
        "deflate" => decompress_to_vec_zlib_with_limit(body, max),
        _ => return Decoded::Failed,
    };

    match decoded {
        // ⚠️ gzip's ISIZE trailer is checked; its CRC32 is NOT. zlib with
        // `window_bits = 15 + 16` verifies both, so a body corrupted in a way
        // that survives DEFLATE but changes the bytes would be accepted here and
        // rejected by Godot. The length check catches truncation, which is the
        // common failure; a full CRC32 is the remaining gap, recorded rather
        // than left for someone to rediscover.
        Ok(bytes)
            if encoding == "gzip" && gzip_isize(body).is_some_and(|n| n != bytes.len() as u32) =>
        {
            Decoded::Failed
        }
        Ok(bytes) => Decoded::Body(bytes),
        // `HasMoreOutput` is exactly "output size exceeded the specified limit",
        // which is a size refusal, not a corrupt body.
        Err(e) if e.status == TINFLStatus::HasMoreOutput => Decoded::TooLarge,
        Err(_) => Decoded::Failed,
    }
}

/// Whether `len` bytes exceed the configured limit (negative = unlimited).
fn over_limit(settings: &DeliverySettings, len: usize) -> bool {
    settings.body_size_limit >= 0 && len as i64 > settings.body_size_limit
}

/// Applies the node-level `body_size_limit` and `download_file` properties to a
/// finished payload, returning what `request_completed` should actually carry.
///
/// Mirrors `HTTPRequest::_update_connection`'s body branch
/// (`http_request.cpp:395-485`). Three things there are easy to get wrong:
///
/// 🔑 **With `download_file` set, the signal body is EMPTY.** Upstream is
/// `if (file.is_valid()) { file->store_buffer(...) } else { body.append_array(chunk) }`
/// — an exclusive `else`, so bytes go to the file *instead of* into the payload.
/// Delivering both would look correct in every hand test and quietly double the
/// memory cost of the large downloads the property exists to avoid.
///
/// ⚠️ **The size check runs BEFORE the file is opened** (`:408` before `:413`),
/// so an over-limit response never creates the file. Opening first would leave
/// a stray empty file behind on every rejected download.
///
/// ⚠️ **Only a successful result is post-processed.** Upstream reaches this code
/// solely from `STATUS_BODY`; a transport failure goes straight to
/// `_defer_done`. Applying a size limit to a `CantConnect` payload would
/// overwrite its result code with a misleading one.
///
/// The rejection paths keep `response_code` and `headers` and clear only the
/// body, which is what upstream passes to `_defer_done`: a script can still see
/// that the server answered 200 and why the node refused it.
pub fn deliver(payload: RequestCompleted, settings: &DeliverySettings) -> RequestCompleted {
    if payload.result != RESULT_SUCCESS {
        return payload;
    }

    let reject = |result: i32, payload: RequestCompleted| RequestCompleted {
        result,
        body: Vec::new(),
        ..payload
    };

    // (1) `:408` — against the length as RECEIVED, still encoded. Upstream reads
    // it from Content-Length before a byte is downloaded, which is the point:
    // an obviously oversized response is refused without transferring it.
    if over_limit(settings, payload.body.len()) {
        return reject(RESULT_BODY_SIZE_LIMIT_EXCEEDED, payload);
    }

    // (2) `:413` — the file is opened BEFORE the body is decoded, so a bad path
    // is reported as CANT_OPEN even when the body would also have failed to
    // decode. Mirrored deliberately: it fixes which of two errors a script sees.
    let mut file = if settings.download_file.is_empty() {
        None
    } else {
        match gdcore::file_access::FileAccess::open(
            &settings.download_file,
            gdcore::file_access::FileMode::Write,
        ) {
            Ok(handle) => Some(handle),
            Err(_) => return reject(RESULT_DOWNLOAD_FILE_CANT_OPEN, payload),
        }
    };

    // (3) `:308-316` — `Content-Encoding` is consulted ONLY when accept_gzip is
    // set, so switching the property off means the bytes are passed through
    // untouched rather than decoded behind the script's back.
    let encoding = if settings.accept_gzip {
        header_value(&payload.headers, "Content-Encoding").map(str::to_ascii_lowercase)
    } else {
        None
    };
    let payload = match encoding.as_deref() {
        Some(enc @ ("gzip" | "deflate")) => {
            match decompress_body(&payload.body, enc, settings.body_size_limit) {
                Decoded::Body(body) => RequestCompleted { body, ..payload },
                Decoded::TooLarge => {
                    return reject(RESULT_BODY_SIZE_LIMIT_EXCEEDED, payload);
                }
                Decoded::Failed => return reject(RESULT_BODY_DECOMPRESS_FAILED, payload),
            }
        }
        // An encoding nothing here handles (`br`, `zstd`) is left alone: Godot
        // does not decode those either, and mangling them would be worse.
        _ => payload,
    };

    // (4) `:453`/`:463` — the same limit again, now against the DECODED size.
    // ⚠️ Not redundant with (1): upstream's comment calls it the "zip bomb"
    // guard. A few kilobytes of gzip can expand to gigabytes, so the check that
    // matters for safety is this one, and dropping it because (1) passed is the
    // natural-looking simplification that removes the protection entirely.
    if over_limit(settings, payload.body.len()) {
        return reject(RESULT_BODY_SIZE_LIMIT_EXCEEDED, payload);
    }

    // (5) `:471` — decoded bytes reach the file, and the signal body stays empty.
    let Some(file) = file.as_mut() else {
        return payload;
    };
    if file.store_buffer(&payload.body).is_err() {
        return reject(RESULT_DOWNLOAD_FILE_WRITE_ERROR, payload);
    }
    RequestCompleted {
        body: Vec::new(),
        ..payload
    }
}

/// A scene node's HTTP request: the platform client plus the queue that carries
/// finished responses back to the main thread.
pub struct HttpRequestNode {
    inner: HTTPRequest,
    /// Payloads produced by [`HTTPRequest::poll`] on the transport's thread,
    /// waiting for the frame tick to turn them into a scene signal.
    completed: Arc<Mutex<Vec<RequestCompleted>>>,
}

/// Godot's `HTTPRequest.timeout` default (`HTTPRequest.xml:255`).
pub const DEFAULT_TIMEOUT: f64 = 0.0;
/// Godot's `HTTPRequest.max_redirects` default (`HTTPRequest.xml:252`).
pub const DEFAULT_MAX_REDIRECTS: i64 = 8;
/// Godot's `HTTPRequest.accept_gzip` default (`HTTPRequest.xml:236`).
///
/// ⚠️ **`true`.** Compression is on unless a project turns it off, so the decode
/// path below is the normal path, not an opt-in corner.
pub const DEFAULT_ACCEPT_GZIP: bool = true;

/// Godot's `set_timeout` guard.
///
/// ⚠️ A negative value is REJECTED and leaves `current` in place rather than
/// being clamped to zero — `ERR_FAIL_COND(p_timeout < 0)` at
/// `http_request.cpp:616` returns before assigning. Clamping would turn a bad
/// argument into "never time out", which is the one setting a caller asking for
/// a timeout definitely did not want.
pub fn accepted_timeout(current: f64, requested: f64) -> f64 {
    if requested < 0.0 {
        current
    } else {
        requested
    }
}

/// Maps Godot's signed `max_redirects` onto the transport's unsigned counter.
///
/// ⚠️ **A negative means unlimited, not "none".** The redirect check is
/// `max_redirects >= 0 && redirections >= max_redirects`
/// (`http_request.cpp:255`) and the property's hint range is `-1,64`, so `-1` is
/// the documented spelling for no limit. Saturating preserves that; clamping to
/// `0` would invert it into "refuse every redirect", which surfaces as a server
/// or URL problem rather than a settings one, and a bare `as u32` cast would
/// reach a huge number by accident instead of by intent.
pub fn redirects_for_transport(max_redirects: i64) -> u32 {
    if max_redirects < 0 {
        u32::MAX
    } else {
        max_redirects.min(u32::MAX as i64) as u32
    }
}

impl Default for HttpRequestNode {
    fn default() -> Self {
        Self::new()
    }
}

impl HttpRequestNode {
    pub fn new() -> Self {
        let inner = HTTPRequest::new();
        let completed: Arc<Mutex<Vec<RequestCompleted>>> = Arc::new(Mutex::new(Vec::new()));
        let sink = completed.clone();
        inner.connect_request_completed(move |payload| {
            sink.lock().unwrap().push(RequestCompleted {
                result: payload.result,
                response_code: payload.response_code,
                headers: payload.headers.clone(),
                body: payload.body.clone(),
            });
        });
        Self { inner, completed }
    }

    /// The transport config this node would send for `url` with `settings`.
    ///
    /// ⚠️ The settings arrive as an argument rather than living on this struct.
    /// ClassDB already registers `timeout` and `max_redirects` as HTTPRequest
    /// **node properties** with Godot's defaults, so a `.tscn` authoring
    /// `timeout = 5.0` writes them there and `r.timeout = 5.0` from a script
    /// does too. Keeping a second copy here would make `r.timeout = 5.0` and
    /// `r.set_timeout(5.0)` — which Godot treats as the same thing — disagree,
    /// with whichever one the transport happened to read winning silently.
    ///
    /// Split out of [`request`](Self::request) so the values that reach the
    /// transport are observable without standing up a server: before this the
    /// config was built with `..Default::default()` and no setting reached the
    /// wire at all.
    pub fn config_for(
        &self,
        url: &str,
        method: HttpMethod,
        body: Vec<u8>,
        settings: RequestSettings,
    ) -> HttpRequestConfig {
        self.config_for_with_headers(url, method, body, &[], settings)
    }

    /// [`config_for`](Self::config_for) plus Godot's `custom_headers` argument.
    ///
    /// A separate method rather than a wider signature on `config_for`: the
    /// no-header form is what every existing caller and test uses, and widening
    /// it would edit call sites in four files to pass `&[]`.
    ///
    /// ⚠️ `Accept-Encoding` is added **only if the caller did not set one**
    /// (`http_request.cpp:137-139`, "If the user has specified an
    /// Accept-Encoding header, don't overwrite it"). Overwriting it would break
    /// a script that deliberately asked for an encoding — or for none.
    pub fn config_for_with_headers(
        &self,
        url: &str,
        method: HttpMethod,
        body: Vec<u8>,
        custom_headers: &[String],
        settings: RequestSettings,
    ) -> HttpRequestConfig {
        let mut headers: HashMap<String, String> = HashMap::new();
        for line in custom_headers {
            // Godot's custom_headers are `"Name: Value"` lines; the first colon
            // separates them, since values contain colons of their own.
            if let Some((key, value)) = line.split_once(':') {
                headers.insert(key.trim().to_string(), value.trim().to_string());
            }
        }
        if settings.accept_gzip
            && !headers
                .keys()
                .any(|k| k.eq_ignore_ascii_case("Accept-Encoding"))
        {
            headers.insert("Accept-Encoding".to_string(), "gzip, deflate".to_string());
        }

        HttpRequestConfig {
            url: url.to_string(),
            method,
            body,
            headers,
            max_redirects: redirects_for_transport(settings.max_redirects),
            timeout_seconds: settings.timeout,
            // `use_ssl` follows the scheme rather than being asked for
            // separately — a script calling `request("https://…")` has already
            // said so.
            use_ssl: url.starts_with("https://"),
        }
    }

    /// Starts a request. Returns Godot's error code — `0` for OK, `44`
    /// (`ERR_BUSY`) when one is already in flight, which is what Godot returns
    /// rather than queueing or cancelling.
    pub fn request(
        &self,
        url: &str,
        method: HttpMethod,
        body: Vec<u8>,
        settings: RequestSettings,
    ) -> i32 {
        self.request_with_headers(url, method, body, &[], settings)
    }

    /// [`request`](Self::request) plus Godot's `custom_headers` argument.
    pub fn request_with_headers(
        &self,
        url: &str,
        method: HttpMethod,
        body: Vec<u8>,
        custom_headers: &[String],
        settings: RequestSettings,
    ) -> i32 {
        self.inner.request(self.config_for_with_headers(
            url,
            method,
            body,
            custom_headers,
            settings,
        ))
    }

    pub fn is_requesting(&self) -> bool {
        self.inner.is_requesting()
    }

    /// Cancels an in-flight request. Godot emits **nothing** here.
    ///
    /// ⚠️ Delegating straight to the platform `cancel()` is wrong, and wrong in
    /// a way that looks right: that call records a `RequestFailed` response and
    /// leaves `emitted` false (`gdplatform/src/http_request.rs:387`), so the
    /// next `poll` — whoever makes it — builds a payload and hands the script a
    /// `request_completed` carrying a failure. Godot's `cancel_request`
    /// (`scene/main/http_request.cpp:186-210`) ends at `requesting = false` and
    /// emits no signal at all. So the recorded failure is consumed here and
    /// dropped.
    ///
    /// 🔑 Consuming it via `poll` rather than just clearing the queue is what
    /// suppresses the LATE completion too: `poll` sets `emitted`, and the
    /// background transport thread that is still in flight only writes
    /// `response`/`in_progress` when it finishes — it never clears `emitted`.
    /// A later poll therefore short-circuits, so a request cancelled mid-flight
    /// stays silent even once the server answers.
    ///
    /// ⚠️ The `is_requesting` guard mirrors `http_request.cpp:189` and is not
    /// politeness: the platform `cancel` OVERWRITES `response`, so cancelling an
    /// idle node would destroy a finished-but-undrained payload.
    pub fn cancel(&self) {
        if !self.inner.is_requesting() {
            return;
        }
        self.inner.cancel();
        // Snapshot AFTER cancelling, so the truncate below drops only what this
        // poll produces rather than a response that landed a moment earlier.
        let keep = self.completed.lock().unwrap().len();
        self.inner.poll();
        self.completed.lock().unwrap().truncate(keep);
    }

    /// Polls the transport and returns whatever finished this frame.
    ///
    /// Returns a `Vec` rather than an `Option` because a node whose script fires
    /// several requests in a row must not silently lose one — Godot refuses the
    /// second with `ERR_BUSY`, but a mock/injected response can complete out of
    /// step with the poll.
    pub fn drain_completed(&self) -> Vec<RequestCompleted> {
        self.inner.poll();
        std::mem::take(&mut *self.completed.lock().unwrap())
    }

    /// Supplies a response directly, bypassing the transport (Godot has no
    /// equivalent; it exists so a test can prove the SCENE wiring without
    /// standing up a server).
    pub fn inject_response(&self, response: gdplatform::http_request::HttpResponse) {
        self.inner.inject_response(response);
    }
}

/// Godot's `HTTPClient.Method` ordinals (`core/io/http_client.h:118-126`):
/// GET 0, HEAD 1, POST 2, PUT 3, DELETE 4, OPTIONS 5, TRACE 6, CONNECT 7,
/// PATCH 8.
///
/// ⚠️ Returns `None` for OPTIONS/TRACE/CONNECT rather than falling back to GET.
/// The platform transport has no variant for them, and quietly sending a GET
/// where a script asked for something else is the kind of substitution that
/// looks like a working request and corrupts whatever is on the other end. The
/// caller turns `None` into `ERR_UNAVAILABLE`.
///
/// ⚠️ PATCH is **8**, not 6 — 6 is TRACE. Counting the variants of Patina's own
/// `HttpMethod` enum instead of reading the upstream header gets this wrong,
/// because that enum omits three of Godot's nine.
pub fn method_from_int(v: i64) -> Option<HttpMethod> {
    match v {
        0 => Some(HttpMethod::Get),
        1 => Some(HttpMethod::Head),
        2 => Some(HttpMethod::Post),
        3 => Some(HttpMethod::Put),
        4 => Some(HttpMethod::Delete),
        8 => Some(HttpMethod::Patch),
        _ => None,
    }
}
