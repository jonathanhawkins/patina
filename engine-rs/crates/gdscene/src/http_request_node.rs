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
}

impl Default for RequestSettings {
    fn default() -> Self {
        Self {
            timeout: DEFAULT_TIMEOUT,
            max_redirects: DEFAULT_MAX_REDIRECTS,
        }
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
        HttpRequestConfig {
            url: url.to_string(),
            method,
            body,
            max_redirects: redirects_for_transport(settings.max_redirects),
            timeout_seconds: settings.timeout,
            // `use_ssl` follows the scheme rather than being asked for
            // separately — a script calling `request("https://…")` has already
            // said so.
            use_ssl: url.starts_with("https://"),
            ..Default::default()
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
        self.inner
            .request(self.config_for(url, method, body, settings))
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
