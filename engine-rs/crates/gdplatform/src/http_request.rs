//! HTTPRequest node — async HTTP client for Patina Engine.
//!
//! Mirrors Godot's `HTTPRequest` node: enqueue a request, poll for
//! completion, and receive the response body + headers. Uses a
//! background thread for non-blocking I/O so the game loop doesn't stall.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// HTTP methods supported by HTTPRequest.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HttpMethod {
    /// GET request.
    Get,
    /// POST request.
    Post,
    /// PUT request.
    Put,
    /// DELETE request.
    Delete,
    /// HEAD request.
    Head,
    /// PATCH request.
    Patch,
}

impl HttpMethod {
    /// Returns the method string (e.g. "GET", "POST").
    pub fn as_str(&self) -> &'static str {
        match self {
            HttpMethod::Get => "GET",
            HttpMethod::Post => "POST",
            HttpMethod::Put => "PUT",
            HttpMethod::Delete => "DELETE",
            HttpMethod::Head => "HEAD",
            HttpMethod::Patch => "PATCH",
        }
    }
}

/// Result code for an HTTP request, mirroring Godot's HTTPRequest.Result enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HttpResultCode {
    /// Request completed successfully.
    Success,
    /// Request is still in progress.
    InProgress,
    /// Could not connect to host.
    CantConnect,
    /// Could not resolve hostname.
    CantResolve,
    /// The connection errored mid-exchange.
    ConnectionError,
    /// Connection timed out.
    Timeout,
    /// SSL handshake error.
    SslHandshakeError,
    /// No response from server.
    NoResponse,
    /// Request was cancelled / failed.
    RequestFailed,
    /// Redirect limit exceeded.
    RedirectLimitReached,
}

impl HttpResultCode {
    /// Returns the integer value matching Godot's `HTTPRequest.Result` enum.
    ///
    /// `InProgress` is internal (Godot has no such result); it maps to
    /// `RESULT_REQUEST_FAILED` since it should never be surfaced in a
    /// `request_completed` signal.
    pub fn as_godot_result_code(&self) -> i32 {
        match self {
            HttpResultCode::Success => 0,
            HttpResultCode::CantConnect => 2,
            HttpResultCode::CantResolve => 3,
            HttpResultCode::ConnectionError => 4,
            HttpResultCode::SslHandshakeError => 5,
            HttpResultCode::NoResponse => 6,
            HttpResultCode::RedirectLimitReached => 12,
            HttpResultCode::Timeout => 13,
            HttpResultCode::RequestFailed | HttpResultCode::InProgress => 9,
        }
    }
}

/// An HTTP response received from the server.
#[derive(Debug, Clone)]
pub struct HttpResponse {
    /// HTTP status code (e.g. 200, 404).
    pub status_code: u16,
    /// Response headers.
    pub headers: HashMap<String, String>,
    /// Response body as bytes.
    pub body: Vec<u8>,
    /// Result code indicating success or failure type.
    pub result: HttpResultCode,
}

impl HttpResponse {
    /// Returns the body as a UTF-8 string, if valid.
    pub fn body_as_string(&self) -> Option<String> {
        String::from_utf8(self.body.clone()).ok()
    }

    /// Returns true if the request completed with a 2xx status.
    pub fn is_success(&self) -> bool {
        self.result == HttpResultCode::Success && (200..300).contains(&self.status_code)
    }

    /// Returns the headers as sorted `"Name: Value"` lines.
    ///
    /// Matches the shape of Godot's `request_completed` `headers`
    /// `PackedStringArray`. Sorted so the output is deterministic despite the
    /// underlying `HashMap`.
    pub fn header_lines(&self) -> Vec<String> {
        let mut lines: Vec<String> = self
            .headers
            .iter()
            .map(|(k, v)| format!("{k}: {v}"))
            .collect();
        lines.sort();
        lines
    }
}

/// Configuration for an HTTP request.
#[derive(Debug, Clone)]
pub struct HttpRequestConfig {
    /// Target URL.
    pub url: String,
    /// HTTP method.
    pub method: HttpMethod,
    /// Request headers.
    pub headers: HashMap<String, String>,
    /// Request body (for POST/PUT/PATCH).
    pub body: Vec<u8>,
    /// Maximum number of redirects to follow.
    pub max_redirects: u32,
    /// Timeout in seconds (0 = no timeout).
    pub timeout_seconds: f64,
    /// Whether to use SSL verification.
    pub use_ssl: bool,
}

impl Default for HttpRequestConfig {
    fn default() -> Self {
        Self {
            url: String::new(),
            method: HttpMethod::Get,
            headers: HashMap::new(),
            body: Vec::new(),
            max_redirects: 8,
            timeout_seconds: 0.0,
            use_ssl: true,
        }
    }
}

// ---------------------------------------------------------------------------
// HTTPRequest (polling-based async)
// ---------------------------------------------------------------------------

/// Internal state shared between the HTTPRequest and the background thread.
#[derive(Debug)]
struct RequestState {
    response: Option<HttpResponse>,
    in_progress: bool,
    /// Whether the `request_completed` signal has already been emitted for the
    /// current response (so [`poll`](HTTPRequest::poll) emits exactly once).
    emitted: bool,
}

/// Payload delivered by the `request_completed` signal.
///
/// Mirrors Godot's `HTTPRequest.request_completed(result, response_code,
/// headers, body)` signal arguments.
#[derive(Debug, Clone)]
pub struct RequestCompleted {
    /// Godot `HTTPRequest.Result` code (see [`HttpResultCode::as_godot_result_code`]).
    pub result: i32,
    /// HTTP status code (0 when the request never reached the server).
    pub response_code: i32,
    /// Response headers as `"Name: Value"` lines (Godot `PackedStringArray`).
    pub headers: Vec<String>,
    /// Response body bytes (Godot `PackedByteArray`).
    pub body: Vec<u8>,
}

/// A callback connected to the `request_completed` signal.
type CompletedCallback = Box<dyn Fn(&RequestCompleted) + Send + 'static>;

/// An HTTP request node that processes requests asynchronously.
///
/// Mirrors Godot's `HTTPRequest` node:
/// 1. [`connect_request_completed`](Self::connect_request_completed) to observe
///    results (Godot's `request_completed` signal).
/// 2. [`request`](Self::request) to start a request — the work runs on a
///    background thread over the [`HttpClient`](crate::http_client::HttpClient)
///    transport.
/// 3. [`poll`](Self::poll) each frame; it emits `request_completed` once the
///    response is ready. [`wait_for_completion`](Self::wait_for_completion) is a
///    blocking convenience for tests/headless callers.
///
/// Responses can also be supplied directly via
/// [`inject_response`](Self::inject_response) for mock backends.
#[derive(Clone)]
pub struct HTTPRequest {
    state: Arc<Mutex<RequestState>>,
    callbacks: Arc<Mutex<Vec<CompletedCallback>>>,
    /// Maximum number of redirects to follow.
    pub max_redirects: u32,
    /// Timeout in seconds (0 = the transport default).
    pub timeout: f64,
    /// Whether SSL verification is enabled.
    pub use_ssl: bool,
}

impl std::fmt::Debug for HTTPRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let state = self.state.lock().unwrap();
        f.debug_struct("HTTPRequest")
            .field("in_progress", &state.in_progress)
            .field("has_response", &state.response.is_some())
            .field("max_redirects", &self.max_redirects)
            .field("timeout", &self.timeout)
            .field("use_ssl", &self.use_ssl)
            .finish()
    }
}

impl HTTPRequest {
    /// Creates a new HTTPRequest.
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(RequestState {
                response: None,
                in_progress: false,
                emitted: true,
            })),
            callbacks: Arc::new(Mutex::new(Vec::new())),
            max_redirects: 8,
            timeout: 0.0,
            use_ssl: true,
        }
    }

    /// Connects a callback to the `request_completed` signal.
    ///
    /// The callback fires from [`poll`](Self::poll) (i.e. on the polling
    /// thread, matching Godot's main-thread signal delivery) once per request.
    pub fn connect_request_completed<F>(&self, callback: F)
    where
        F: Fn(&RequestCompleted) + Send + 'static,
    {
        self.callbacks.lock().unwrap().push(Box::new(callback));
    }

    /// Returns true if a request is currently in progress.
    pub fn is_requesting(&self) -> bool {
        self.state.lock().unwrap().in_progress
    }

    /// Takes the completed response, if available. Returns `None` if still
    /// in progress or no request has been made.
    pub fn take_response(&self) -> Option<HttpResponse> {
        let mut state = self.state.lock().unwrap();
        if state.in_progress {
            return None;
        }
        state.response.take()
    }

    /// Starts an asynchronous HTTP request on a background thread.
    ///
    /// Returns the Godot `Error` code: `OK` (0) when the request was accepted,
    /// or `ERR_BUSY` (44) if a request is already running.
    pub fn request(&self, config: HttpRequestConfig) -> i32 {
        const OK: i32 = 0;
        const ERR_BUSY: i32 = 44;

        let mut state = self.state.lock().unwrap();
        if state.in_progress {
            return ERR_BUSY;
        }
        state.in_progress = true;
        state.response = None;
        state.emitted = false;
        drop(state);

        // Apply node-level overrides not carried on the config.
        let mut config = config;
        if self.timeout > 0.0 && config.timeout_seconds <= 0.0 {
            config.timeout_seconds = self.timeout;
        }
        if config.max_redirects == HttpRequestConfig::default().max_redirects {
            config.max_redirects = self.max_redirects;
        }

        let state = Arc::clone(&self.state);
        std::thread::spawn(move || {
            let response = crate::http_client::HttpClient::perform(&config);
            let mut state = state.lock().unwrap();
            state.response = Some(response);
            state.in_progress = false;
        });

        OK
    }

    /// Drives the node: when the in-flight request has finished, emits the
    /// `request_completed` signal to all connected callbacks exactly once.
    ///
    /// Returns `true` if a signal was emitted on this call.
    pub fn poll(&self) -> bool {
        let payload = {
            let mut state = self.state.lock().unwrap();
            if state.in_progress || state.emitted {
                return false;
            }
            // Build the owned payload first (releasing the immutable borrow of
            // `state.response`), then mark the signal as emitted.
            let payload = match state.response.as_ref() {
                Some(response) => RequestCompleted {
                    result: response.result.as_godot_result_code(),
                    response_code: response.status_code as i32,
                    headers: response.header_lines(),
                    body: response.body.clone(),
                },
                None => return false,
            };
            state.emitted = true;
            payload
        };

        for cb in self.callbacks.lock().unwrap().iter() {
            cb(&payload);
        }
        true
    }

    /// Blocks until the in-flight request finishes (driving [`poll`](Self::poll)
    /// so `request_completed` fires), or until `timeout` elapses.
    ///
    /// Returns `true` if the request completed within the deadline. Intended
    /// for tests and headless callers; the engine loop should call
    /// [`poll`](Self::poll) once per frame instead.
    pub fn wait_for_completion(&self, timeout: std::time::Duration) -> bool {
        let deadline = std::time::Instant::now() + timeout;
        loop {
            if self.poll() {
                return true;
            }
            if !self.is_requesting() {
                // Finished and already emitted (or nothing in flight).
                return self.state.lock().unwrap().response.is_some();
            }
            if std::time::Instant::now() >= deadline {
                return false;
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
    }

    /// Performs a blocking request and returns the response directly.
    ///
    /// Convenience for headless callers that don't need the signal/poll flow.
    pub fn request_sync(&self, config: &HttpRequestConfig) -> HttpResponse {
        crate::http_client::HttpClient::perform(config)
    }

    /// Injects a response directly (for testing or mock backends).
    ///
    /// The next [`poll`](Self::poll) will emit `request_completed` for it.
    pub fn inject_response(&self, response: HttpResponse) {
        let mut state = self.state.lock().unwrap();
        state.response = Some(response);
        state.in_progress = false;
        state.emitted = false;
    }

    /// Cancels any in-progress request, recording a failed result.
    pub fn cancel(&self) {
        let mut state = self.state.lock().unwrap();
        state.in_progress = false;
        state.emitted = false;
        state.response = Some(HttpResponse {
            status_code: 0,
            headers: HashMap::new(),
            body: Vec::new(),
            result: HttpResultCode::RequestFailed,
        });
    }
}

impl Default for HTTPRequest {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn new_request_not_in_progress() {
        let req = HTTPRequest::new();
        assert!(!req.is_requesting());
        assert!(req.take_response().is_none());
    }

    #[test]
    fn inject_then_poll_emits_signal_once() {
        let req = HTTPRequest::new();
        let seen = Arc::new(Mutex::new(Vec::<RequestCompleted>::new()));
        let sink = Arc::clone(&seen);
        req.connect_request_completed(move |rc| sink.lock().unwrap().push(rc.clone()));

        let mut headers = HashMap::new();
        headers.insert("Content-Type".to_string(), "text/plain".to_string());
        req.inject_response(HttpResponse {
            status_code: 201,
            headers,
            body: b"Hello".to_vec(),
            result: HttpResultCode::Success,
        });

        assert!(req.poll(), "first poll should emit");
        assert!(!req.poll(), "second poll must not re-emit");

        let seen = seen.lock().unwrap();
        assert_eq!(seen.len(), 1);
        assert_eq!(seen[0].result, 0);
        assert_eq!(seen[0].response_code, 201);
        assert_eq!(seen[0].body, b"Hello");
        assert_eq!(seen[0].headers, vec!["Content-Type: text/plain".to_string()]);
    }

    #[test]
    fn inject_response_completes_request() {
        let req = HTTPRequest::new();
        req.inject_response(HttpResponse {
            status_code: 200,
            headers: HashMap::new(),
            body: b"Hello".to_vec(),
            result: HttpResultCode::Success,
        });

        assert!(!req.is_requesting());
        let resp = req.take_response().expect("should have response");
        assert_eq!(resp.status_code, 200);
        assert_eq!(resp.body, b"Hello");
        assert!(resp.is_success());
    }

    #[test]
    fn body_as_string() {
        let resp = HttpResponse {
            status_code: 200,
            headers: HashMap::new(),
            body: b"Hello world".to_vec(),
            result: HttpResultCode::Success,
        };
        assert_eq!(resp.body_as_string(), Some("Hello world".into()));
    }

    #[test]
    fn cancel_records_failed_result() {
        let req = HTTPRequest::new();
        req.cancel();
        assert!(!req.is_requesting());
        let resp = req.take_response().unwrap();
        assert_eq!(resp.result, HttpResultCode::RequestFailed);
    }

    #[test]
    fn request_sync_https_reports_tls_error() {
        // No TLS backend is wired into gdplatform, so https must report a
        // handshake error rather than silently downgrading.
        let req = HTTPRequest::new();
        let config = HttpRequestConfig {
            url: "https://example.com".into(),
            ..Default::default()
        };
        let resp = req.request_sync(&config);
        assert_eq!(resp.result, HttpResultCode::SslHandshakeError);
    }

    #[test]
    fn result_code_godot_mapping() {
        assert_eq!(HttpResultCode::Success.as_godot_result_code(), 0);
        assert_eq!(HttpResultCode::CantConnect.as_godot_result_code(), 2);
        assert_eq!(HttpResultCode::CantResolve.as_godot_result_code(), 3);
        assert_eq!(HttpResultCode::ConnectionError.as_godot_result_code(), 4);
        assert_eq!(HttpResultCode::SslHandshakeError.as_godot_result_code(), 5);
        assert_eq!(HttpResultCode::Timeout.as_godot_result_code(), 13);
        assert_eq!(HttpResultCode::RedirectLimitReached.as_godot_result_code(), 12);
    }

    #[test]
    fn is_success_checks_status_range() {
        let ok = HttpResponse {
            status_code: 200,
            headers: HashMap::new(),
            body: Vec::new(),
            result: HttpResultCode::Success,
        };
        assert!(ok.is_success());

        let not_found = HttpResponse {
            status_code: 404,
            headers: HashMap::new(),
            body: Vec::new(),
            result: HttpResultCode::Success,
        };
        assert!(!not_found.is_success());

        let failed = HttpResponse {
            status_code: 200,
            headers: HashMap::new(),
            body: Vec::new(),
            result: HttpResultCode::CantConnect,
        };
        assert!(!failed.is_success());
    }

    #[test]
    fn http_method_as_str() {
        assert_eq!(HttpMethod::Get.as_str(), "GET");
        assert_eq!(HttpMethod::Post.as_str(), "POST");
        assert_eq!(HttpMethod::Put.as_str(), "PUT");
        assert_eq!(HttpMethod::Delete.as_str(), "DELETE");
        assert_eq!(HttpMethod::Head.as_str(), "HEAD");
        assert_eq!(HttpMethod::Patch.as_str(), "PATCH");
    }

    #[test]
    fn response_headers() {
        let mut headers = HashMap::new();
        headers.insert("Content-Type".into(), "application/json".into());
        let resp = HttpResponse {
            status_code: 200,
            headers,
            body: b"{}".to_vec(),
            result: HttpResultCode::Success,
        };
        assert_eq!(
            resp.headers.get("Content-Type"),
            Some(&"application/json".to_string())
        );
    }

    #[test]
    fn take_response_clears_it() {
        let req = HTTPRequest::new();
        req.inject_response(HttpResponse {
            status_code: 200,
            headers: HashMap::new(),
            body: Vec::new(),
            result: HttpResultCode::Success,
        });
        assert!(req.take_response().is_some());
        assert!(req.take_response().is_none()); // second take is None
    }

    #[test]
    fn default_config() {
        let config = HttpRequestConfig::default();
        assert_eq!(config.method, HttpMethod::Get);
        assert_eq!(config.max_redirects, 8);
        assert!(config.use_ssl);
        assert!(config.body.is_empty());
    }

    #[test]
    fn post_with_body() {
        let config = HttpRequestConfig {
            url: "https://api.example.com/data".into(),
            method: HttpMethod::Post,
            body: b"{\"key\":\"value\"}".to_vec(),
            ..Default::default()
        };
        assert_eq!(config.method, HttpMethod::Post);
        assert!(!config.body.is_empty());
    }
}
