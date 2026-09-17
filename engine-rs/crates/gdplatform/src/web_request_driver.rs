//! pat-s3rrv.78 — the poll/read state machine of Godot's web HTTP client.
//!
//! Godot ground truth: `platform/web/http_client_web.cpp`, specifically
//! `HTTPClientWeb::poll` and `HTTPClientWeb::read_response_body_chunk`. Those
//! two functions are the whole driver: [`fetch_state`](crate::fetch_state)
//! models what the JS side reports, and this module models what the client does
//! with it.
//!
//! ⚠️ **Why this is target-independent.** Same reason as [`fetch_state`]: code
//! behind `#[cfg(target_arch = "wasm32")]` is parsed but never type-checked by a
//! native build, and never *run* by any build in this repo. The ordering rules
//! below are exactly the kind that look right and truncate bodies in
//! production, so they are compiled and tested on every target. The wasm shell
//! implements [`FetchBackend`] over the JS interop and gets these rules for
//! free.
//!
//! 🔑 The backend is a trait rather than a concrete type so the rules can be
//! driven by a scripted fake here. It is used through generics, never as
//! `&mut dyn` — the driver mutates the backend while holding its own state, and
//! an unsized receiver makes that borrow pattern needlessly hard.

use crate::fetch_state::FetchState;

/// Mirrors the subset of `HTTPClient::Status` the web backend actually visits
/// (`http_client_web.cpp:206-250`).
///
/// ⚠️ `Disconnected` is both the initial state **and** the terminal success
/// state: upstream sets `status = STATUS_DISCONNECTED` when the body stream
/// reports `Done`. It does not have a separate "finished" status. Reading it as
/// "never started" is wrong in the one case that matters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WebClientStatus {
    /// Idle, or the response has been fully received.
    Disconnected,
    /// Faked DNS step. Always succeeds on the next poll.
    Resolving,
    /// Faked connect step. Always succeeds on the next poll.
    Connecting,
    /// Ready to issue a request.
    Connected,
    /// A `fetch()` is in flight; headers have not been read yet.
    Requesting,
    /// Headers are in; the body stream is draining.
    Body,
    /// Terminal failure.
    ConnectionError,
}

/// The error codes `poll` returns, mapped to Godot's `Error`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PollError {
    /// `ERR_UNCONFIGURED` — polled while disconnected.
    Unconfigured,
    /// `ERR_CONNECTION_ERROR`.
    ConnectionError,
}

/// What the driver needs from the JS `fetch` interop.
///
/// The wasm implementation forwards each method to the matching
/// `godot_js_fetch_*` import; tests supply a scripted fake.
pub trait FetchBackend {
    /// `godot_js_fetch_state_get`.
    fn state(&self) -> FetchState;
    /// `godot_js_fetch_http_status_get`.
    fn http_status(&self) -> u16;
    /// `godot_js_fetch_read_headers`. `None` signals a parse failure, which
    /// upstream treats as a connection error.
    fn read_headers(&mut self) -> Option<Vec<String>>;
    /// `godot_js_fetch_read_chunk`; returns the number of bytes written.
    fn read_chunk(&mut self, out: &mut [u8]) -> usize;

    /// Drives the backend one step, where that is not self-driving.
    ///
    /// Defaulted to nothing: upstream's JS reader re-arms itself from its own
    /// promise callbacks, and a scripted test backend has nothing to pump. Only
    /// the real wasm backend overrides it.
    ///
    /// ⚠️ Deliberately NOT folded into [`state`](Self::state): `state` mirrors
    /// `godot_js_fetch_state_get`, a pure read upstream, and the driver reads it
    /// twice per frame. Pumping there would make an observation change the thing
    /// being observed.
    fn pump(&mut self) {}
}

/// Splits `"Name: Value"` header lines into pairs.
///
/// ⚠️ Splits on the FIRST colon. A value routinely contains more of them — any
/// `Date`, any URL in `Location` — while a header name never does. Splitting on
/// the last one, or refusing lines with two, corrupts exactly the headers a
/// redirect or a cache check depends on.
///
/// Lines with no colon at all are dropped rather than admitted with an empty
/// value: they are not headers.
pub fn split_header_lines(lines: &[String]) -> Vec<(String, String)> {
    lines
        .iter()
        .filter_map(|line| line.split_once(':'))
        .map(|(name, value)| (name.trim().to_string(), value.trim().to_string()))
        .collect()
}

/// The default read buffer, mirroring `HTTPRequest.download_chunk_size`.
///
/// ⚠️ **65536, not 4096.** Godot defaults `read_limit` to 65536 in the web
/// client (`platform/web/http_client_web.h:66`) and `read_chunk_size` to the
/// same in the TCP one (`core/io/http_client_tcp.h:75`); the class reference
/// documents `download_chunk_size` as `default="65536"`. 4096 is what that same
/// doc suggests as a deliberate *reduction* — "set this to a lower value (e.g.
/// 4096 for 4 KiB) when downloading small files to decrease memory usage at the
/// cost of download speeds" — so picking it as the default silently gives every
/// download the memory-saving trade-off no one asked for, at 16x the read
/// iterations.
pub const DEFAULT_DOWNLOAD_CHUNK_SIZE: usize = 65536;

/// What a finished transfer produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransferOutcome {
    /// The body stream closed normally.
    Completed {
        /// The HTTP status the server sent.
        status: u16,
        /// Response headers, already split into pairs.
        headers: Vec<(String, String)>,
        /// The full body, accumulated across every frame.
        body: Vec<u8>,
    },
    /// The transfer failed. `status` is the code the server sent if it got far
    /// enough to send one, and 0 otherwise — never a fabricated value.
    Failed {
        /// The last sampled HTTP status.
        status: u16,
    },
}

/// One in-flight web request: a backend, the driver, and the body so far.
///
/// 🔑 This exists so the per-frame stepping is provable. The `#[cfg(target_arch
/// = "wasm32")]` layer that used to hold this logic is never type-checked by a
/// native build and never run by any build in this repo, so every rule in
/// [`step`](Self::step) — the handshake advance, the drain loop, the outcome
/// classification — was unreachable by any test. Generic over the trait, the
/// same code runs against a scripted backend here and the real one in the
/// browser.
pub struct WebTransfer<B: FetchBackend> {
    backend: B,
    driver: WebRequestDriver,
    body: Vec<u8>,
    /// Bytes read per iteration; `HTTPRequest.download_chunk_size`.
    chunk_size: usize,
    /// Reused read buffer. Upstream resizes one buffer rather than allocating
    /// per call (`http_client_web.cpp:166-168`); allocating `chunk_size` bytes
    /// every frame would cost a 64 KiB allocation per frame for the whole of a
    /// long download.
    buf: Vec<u8>,
    /// Set once an outcome has been handed out; see [`step`](Self::step).
    spent: bool,
}

// Hand-written so `B` needn't be `Debug`: `http_request::RequestState` derives
// `Debug` and holds a `WebTransfer<WebFetchBackend>` on wasm32, which broke the
// web build. The body and buffer are summarised by length, not dumped.
impl<B: FetchBackend> std::fmt::Debug for WebTransfer<B> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WebTransfer")
            .field("body_len", &self.body.len())
            .field("chunk_size", &self.chunk_size)
            .field("spent", &self.spent)
            .finish_non_exhaustive()
    }
}

impl<B: FetchBackend> WebTransfer<B> {
    /// Starts a transfer whose `fetch` has already been issued.
    ///
    /// ⚠️ Enters the faked handshake rather than going straight to the request:
    /// the client still owes `Resolving → Connecting → Connected`, one poll
    /// each, and skipping it would complete a web request earlier than the
    /// native path completes the same one.
    pub fn new(backend: B) -> Self {
        let mut driver = WebRequestDriver::new();
        driver.begin_connect();
        Self {
            backend,
            driver,
            body: Vec::new(),
            chunk_size: DEFAULT_DOWNLOAD_CHUNK_SIZE,
            buf: Vec::new(),
            spent: false,
        }
    }

    /// Sets the read buffer size (`HTTPRequest.set_download_chunk_size`).
    ///
    /// ⚠️ Clamped to at least 1: a zero-length buffer makes `read_chunk` return
    /// 0 forever, so the drain loop would break every frame and the body would
    /// never arrive — a hang with no error. Godot's own property hint floors
    /// this at 256.
    pub fn set_chunk_size(&mut self, size: usize) {
        self.chunk_size = size.max(1);
    }

    /// The current read buffer size.
    pub fn chunk_size(&self) -> usize {
        self.chunk_size
    }

    /// The backend, so the caller can release its handle once finished.
    pub fn backend(&self) -> &B {
        &self.backend
    }

    /// The client status, for callers that want to observe progress.
    pub fn status(&self) -> WebClientStatus {
        self.driver.status()
    }

    /// Advances the transfer by one frame, mirroring what Godot's
    /// `HTTPRequest` does to an `HTTPClientWeb` each tick.
    ///
    /// Returns `None` while the request is still in flight and `Some` once it
    /// has finished, exactly once.
    ///
    /// ⚠️ The body is MOVED OUT into the outcome, so a second call could only
    /// report the same completion with an empty body. Rather than let that
    /// silently truncate a response, the transfer is marked spent and every
    /// later call returns `None`.
    pub fn step(&mut self) -> Option<TransferOutcome> {
        if self.spent {
            return None;
        }

        self.backend.pump();

        // The handshake advances on its own; the request goes live the frame
        // after it reaches `Connected`.
        if self.driver.status() == WebClientStatus::Connected {
            self.driver.begin_request();
        }

        let errored = self.driver.poll(&mut self.backend).is_err();

        if !errored {
            // ⚠️ Drain in a LOOP. One `read_chunk` copies at most the buffer, so
            // a body that arrived as several stream chunks would otherwise
            // dribble in at one buffer per frame — correct-looking, and
            // arbitrarily slow on a large response.
            if self.buf.len() != self.chunk_size {
                self.buf.resize(self.chunk_size, 0);
            }
            while self.driver.status() == WebClientStatus::Body {
                // ⚠️ Bind the result BEFORE matching on it. A `&mut self.buf`
                // built inside a match scrutinee can stay borrowed across the
                // arms, and the arm needs to read that same buffer back.
                let read = self
                    .driver
                    .read_response_body_chunk(&mut self.backend, &mut self.buf);
                match read {
                    Ok(0) | Err(_) => break,
                    Ok(read) => self.body.extend_from_slice(&self.buf[..read]),
                }
            }
        }

        let outcome = match self.driver.status() {
            WebClientStatus::ConnectionError => TransferOutcome::Failed {
                status: self.driver.response_code(),
            },
            WebClientStatus::Disconnected => TransferOutcome::Completed {
                status: self.driver.response_code(),
                headers: split_header_lines(self.driver.headers()),
                body: std::mem::take(&mut self.body),
            },
            _ => return None,
        };

        self.spent = true;
        Some(outcome)
    }
}

/// Drives one web request through the poll/read lifecycle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebRequestDriver {
    status: WebClientStatus,
    /// `polled_response_code`. Sampled during `Requesting`, kept afterwards.
    response_code: u16,
    /// The header lines captured at the `Requesting` → `Body` transition.
    headers: Vec<String>,
}

impl Default for WebRequestDriver {
    fn default() -> Self {
        Self::new()
    }
}

impl WebRequestDriver {
    /// A fresh driver, disconnected with no response code
    /// (`http_client_web.cpp:114-115`).
    pub fn new() -> Self {
        Self {
            status: WebClientStatus::Disconnected,
            response_code: 0,
            headers: Vec::new(),
        }
    }

    /// The current client status.
    pub fn status(&self) -> WebClientStatus {
        self.status
    }

    /// The last sampled HTTP status code (`get_response_code`).
    pub fn response_code(&self) -> u16 {
        self.response_code
    }

    /// The header lines read at the start of the body phase.
    pub fn headers(&self) -> &[String] {
        &self.headers
    }

    /// `has_response` — true once headers have been captured
    /// (`http_client_web.cpp:128`).
    pub fn has_response(&self) -> bool {
        !self.headers.is_empty()
    }

    /// True once the response has been fully received or has failed, i.e. no
    /// further polling can make progress.
    pub fn is_finished(&self) -> bool {
        matches!(
            self.status,
            WebClientStatus::Disconnected | WebClientStatus::ConnectionError
        )
    }

    /// Begins a request, entering the faked handshake.
    ///
    /// ⚠️ Upstream really does walk `Resolving → Connecting → Connected`, one
    /// poll each, before the request is considered live — see the three
    /// fall-through arms at `http_client_web.cpp:208-217`. There is no network
    /// behind them on the web, but each still costs a frame. Collapsing them
    /// changes when `request_completed` can first fire.
    pub fn begin_connect(&mut self) {
        self.status = WebClientStatus::Resolving;
        self.response_code = 0;
        self.headers.clear();
    }

    /// Marks the `fetch()` as issued, moving from `Connected` to `Requesting`.
    pub fn begin_request(&mut self) {
        self.status = WebClientStatus::Requesting;
    }

    /// Ports `HTTPClientWeb::poll` (`http_client_web.cpp:205-256`).
    ///
    /// The order inside the `Requesting` arm is load-bearing and preserved
    /// verbatim:
    ///
    /// 1. ⚠️ **The status code is sampled first, on every poll** — including
    ///    polls where the fetch is still `Requesting` and even the poll that
    ///    goes on to report a connection error. Sampling it lazily at the
    ///    transition instead leaves `get_response_code()` at 0 for an errored
    ///    request, where upstream reports the real code.
    /// 2. still `Requesting` → nothing more to do this frame.
    /// 3. `Error` → connection error.
    /// 4. headers are read exactly once here, and a parse failure is itself a
    ///    connection error.
    /// 5. only then → `Body`.
    pub fn poll<B: FetchBackend>(&mut self, backend: &mut B) -> Result<(), PollError> {
        match self.status {
            WebClientStatus::Disconnected => Err(PollError::Unconfigured),

            // The faked handshake: one poll per step, never fails.
            WebClientStatus::Resolving => {
                self.status = WebClientStatus::Connecting;
                Ok(())
            }
            WebClientStatus::Connecting => {
                self.status = WebClientStatus::Connected;
                Ok(())
            }
            WebClientStatus::Connected => Ok(()),

            WebClientStatus::Requesting => {
                // (1) sampled before the state is even inspected.
                self.response_code = backend.http_status();

                match backend.state() {
                    FetchState::Requesting => return Ok(()), // (2)
                    FetchState::Error => {
                        // (3)
                        self.status = WebClientStatus::ConnectionError;
                        return Err(PollError::ConnectionError);
                    }
                    FetchState::Body | FetchState::Done => {}
                }

                // (4) ⚠️ Read once, here. A `Done` fetch still passes through
                // this arm — a short response can resolve and finish before the
                // first poll, and its headers must still be captured.
                match backend.read_headers() {
                    Some(headers) => self.headers = headers,
                    None => {
                        self.status = WebClientStatus::ConnectionError;
                        return Err(PollError::ConnectionError);
                    }
                }

                self.status = WebClientStatus::Body; // (5)
                Ok(())
            }

            WebClientStatus::Body => {
                match backend.state() {
                    FetchState::Done => self.status = WebClientStatus::Disconnected,
                    FetchState::Body => {}
                    // ⚠️ Anything else — including going BACK to `Requesting` —
                    // is a connection error, not a transient state to wait out.
                    _ => {
                        self.status = WebClientStatus::ConnectionError;
                        return Err(PollError::ConnectionError);
                    }
                }
                Ok(())
            }

            WebClientStatus::ConnectionError => Err(PollError::ConnectionError),
        }
    }

    /// Ports `HTTPClientWeb::read_response_body_chunk`
    /// (`http_client_web.cpp:159-182`), returning how many bytes landed in
    /// `out`.
    ///
    /// 🔑 **The chunk is read BEFORE the stream state is checked.** That order
    /// is the whole point of this function. Checking `Done` first and returning
    /// early discards whatever the final `read_chunk` would have produced —
    /// which is invisible for a body that arrives in one piece and truncates
    /// every body that does not.
    ///
    /// ⚠️ A read of 0 bytes is not an error and not completion: the state check
    /// above it has already recorded whichever of those actually applies.
    ///
    /// Returns `Err` if called outside the body phase (upstream's
    /// `ERR_FAIL_COND_V` guard, which yields an empty array).
    pub fn read_response_body_chunk<B: FetchBackend>(
        &mut self,
        backend: &mut B,
        out: &mut [u8],
    ) -> Result<usize, PollError> {
        if self.status != WebClientStatus::Body {
            return Err(PollError::ConnectionError);
        }

        // Read first — see the note above.
        let read = backend.read_chunk(out);

        match backend.state() {
            FetchState::Done => self.status = WebClientStatus::Disconnected,
            FetchState::Body => {}
            _ => self.status = WebClientStatus::ConnectionError,
        }

        Ok(read)
    }
}
