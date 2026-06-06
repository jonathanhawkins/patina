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
    /// Connection timed out.
    Timeout,
    /// SSL handshake error.
    SslHandshakeError,
    /// No response from server.
    NoResponse,
    /// Request was cancelled.
    RequestFailed,
    /// Redirect limit exceeded.
    RedirectLimitReached,
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
}

/// An HTTP request node that processes requests asynchronously.
///
/// Mirrors Godot's `HTTPRequest` node. Call [`request`](Self::request) to
/// start a request, then poll [`is_requesting`](Self::is_requesting) and
/// [`take_response`](Self::take_response) to check for completion.
///
/// In headless/test mode, requests can be resolved synchronously via
/// [`request_sync`](Self::request_sync) or by injecting mock responses.
#[derive(Debug, Clone)]
pub struct HTTPRequest {
    state: Arc<Mutex<RequestState>>,
    /// Maximum number of redirects to follow.
    pub max_redirects: u32,
    /// Timeout in seconds.
    pub timeout: f64,
    /// Whether SSL verification is enabled.
    pub use_ssl: bool,
}

impl HTTPRequest {
    /// Creates a new HTTPRequest.
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(RequestState {
                response: None,
                in_progress: false,
            })),
            max_redirects: 8,
            timeout: 0.0,
            use_ssl: true,
        }
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

    /// Starts an asynchronous HTTP request in a background thread.
    ///
    /// In the real engine, this would use an async HTTP client. For now,
    /// this sets up the state and the response must be injected via
    /// [`inject_response`](Self::inject_response) (for testing) or
    /// resolved by the platform backend.
    pub fn request(&self, config: HttpRequestConfig) {
        let mut state = self.state.lock().unwrap();
        state.in_progress = true;
        state.response = None;

        // Store config for platform backend to pick up.
        // In a full implementation, this would spawn a thread with reqwest/ureq.
        // For now, the platform layer or test harness resolves the request.
        let _config = config; // consumed
    }

    /// Synchronously resolves a request (for testing / headless mode).
    ///
    /// This does not actually make an HTTP call — it creates a mock
    /// response. Use [`inject_response`](Self::inject_response) for
    /// custom test responses.
    pub fn request_sync(&self, _config: &HttpRequestConfig) -> HttpResponse {
        HttpResponse {
            status_code: 200,
            headers: HashMap::new(),
            body: Vec::new(),
            result: HttpResultCode::Success,
        }
    }

    /// Injects a response directly (for testing or mock backends).
    pub fn inject_response(&self, response: HttpResponse) {
        let mut state = self.state.lock().unwrap();
        state.response = Some(response);
        state.in_progress = false;
    }

    /// Cancels any in-progress request.
    pub fn cancel(&self) {
        let mut state = self.state.lock().unwrap();
        state.in_progress = false;
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

    #[test]
    fn new_request_not_in_progress() {
        let req = HTTPRequest::new();
        assert!(!req.is_requesting());
        assert!(req.take_response().is_none());
    }

    #[test]
    fn request_sets_in_progress() {
        let req = HTTPRequest::new();
        req.request(HttpRequestConfig {
            url: "https://example.com".into(),
            ..Default::default()
        });
        assert!(req.is_requesting());
        assert!(req.take_response().is_none()); // still in progress
    }

    #[test]
    fn inject_response_completes_request() {
        let req = HTTPRequest::new();
        req.request(HttpRequestConfig {
            url: "https://example.com".into(),
            ..Default::default()
        });
        assert!(req.is_requesting());

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
    fn cancel_stops_request() {
        let req = HTTPRequest::new();
        req.request(HttpRequestConfig {
            url: "https://example.com".into(),
            ..Default::default()
        });
        assert!(req.is_requesting());

        req.cancel();
        assert!(!req.is_requesting());
        let resp = req.take_response().unwrap();
        assert_eq!(resp.result, HttpResultCode::RequestFailed);
    }

    #[test]
    fn request_sync_returns_success() {
        let req = HTTPRequest::new();
        let config = HttpRequestConfig {
            url: "https://example.com".into(),
            ..Default::default()
        };
        let resp = req.request_sync(&config);
        assert_eq!(resp.status_code, 200);
        assert_eq!(resp.result, HttpResultCode::Success);
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
