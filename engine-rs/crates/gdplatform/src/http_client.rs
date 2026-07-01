//! HTTPClient — low-level HTTP/1.1 client over the platform TCP transport.
//!
//! Mirrors Godot's `HTTPClient` (core/io/http_client.*): a poll-driven
//! request → response state machine built directly on `std::net::TcpStream`.
//! The higher-level [`HTTPRequest`](crate::http_request::HTTPRequest) node
//! builds on this to provide an async request with a `request_completed`
//! signal.
//!
//! Only plaintext `http://` is wired here. `https://` reports a TLS handshake
//! error (matching Godot's behaviour when no TLS backend is available) so the
//! result codes stay faithful instead of silently downgrading.

use std::collections::HashMap;
use std::io::{ErrorKind, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

use crate::http_request::{HttpMethod, HttpRequestConfig, HttpResponse, HttpResultCode};

/// Connection / exchange status, mirroring Godot's `HTTPClient.Status`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HttpClientStatus {
    /// Not connected to any host.
    Disconnected,
    /// Resolving the hostname.
    Resolving,
    /// DNS resolution failed.
    CantResolve,
    /// Establishing the TCP connection.
    Connecting,
    /// TCP connection failed.
    CantConnect,
    /// Connected, ready to send a request.
    Connected,
    /// A request has been sent; awaiting the response head.
    Requesting,
    /// Reading the response body.
    Body,
    /// The connection errored mid-exchange.
    ConnectionError,
    /// The TLS handshake failed (or TLS is unavailable).
    TlsHandshakeError,
}

impl HttpClientStatus {
    /// Returns the integer value matching Godot's `HTTPClient.Status` enum.
    pub fn as_godot_code(&self) -> i32 {
        match self {
            HttpClientStatus::Disconnected => 0,
            HttpClientStatus::Resolving => 1,
            HttpClientStatus::CantResolve => 2,
            HttpClientStatus::Connecting => 3,
            HttpClientStatus::CantConnect => 4,
            HttpClientStatus::Connected => 5,
            HttpClientStatus::Requesting => 6,
            HttpClientStatus::Body => 7,
            HttpClientStatus::ConnectionError => 8,
            HttpClientStatus::TlsHandshakeError => 9,
        }
    }
}

/// A parsed URL broken into the components the client needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedUrl {
    /// The lowercased scheme (e.g. `"http"`, `"https"`, `"ws"`, `"wss"`).
    pub scheme: String,
    /// The host (no brackets for IPv6).
    pub host: String,
    /// The resolved port (scheme default applied when omitted).
    pub port: u16,
    /// The request path including any query string (always starts with `/`).
    pub path: String,
    /// `true` when the scheme implies TLS (`https`/`wss`).
    pub tls: bool,
}

/// Parses a `scheme://host[:port][/path][?query]` URL.
///
/// Returns `None` if the scheme separator is missing or the port is invalid.
/// IPv6 literals in `[..]` brackets are supported.
pub fn parse_url(url: &str) -> Option<ParsedUrl> {
    let (scheme, rest) = url.split_once("://")?;
    let scheme = scheme.to_ascii_lowercase();
    let tls = matches!(scheme.as_str(), "https" | "wss");
    let default_port: u16 = if tls { 443 } else { 80 };

    let (authority, path) = match rest.find('/') {
        Some(i) => (&rest[..i], rest[i..].to_string()),
        None => (rest, "/".to_string()),
    };
    // Drop any userinfo (user:pass@host).
    let authority = match authority.rsplit_once('@') {
        Some((_, h)) => h,
        None => authority,
    };

    let (host, port) = if let Some(close) = authority.strip_prefix('[').and_then(|a| a.find(']')) {
        // IPv6 literal: [::1] or [::1]:port
        let host = authority[1..1 + close].to_string();
        let after = &authority[1 + close + 1..];
        let port = match after.strip_prefix(':') {
            Some(p) => p.parse().ok()?,
            None => default_port,
        };
        (host, port)
    } else if let Some((h, p)) = authority.rsplit_once(':') {
        (h.to_string(), p.parse().ok()?)
    } else {
        (authority.to_string(), default_port)
    };

    if host.is_empty() {
        return None;
    }

    Some(ParsedUrl {
        scheme,
        host,
        port,
        path,
        tls,
    })
}

/// Finds the first occurrence of `needle` within `haystack`.
fn find_subsequence(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

/// Maps a read/write IO error to the closest Godot result code.
fn map_io_error(err: &std::io::Error) -> HttpResultCode {
    match err.kind() {
        ErrorKind::TimedOut | ErrorKind::WouldBlock => HttpResultCode::Timeout,
        ErrorKind::ConnectionRefused
        | ErrorKind::ConnectionReset
        | ErrorKind::ConnectionAborted
        | ErrorKind::NotConnected
        | ErrorKind::BrokenPipe => HttpResultCode::ConnectionError,
        _ => HttpResultCode::ConnectionError,
    }
}

/// A low-level HTTP/1.1 client mirroring Godot's `HTTPClient` state machine.
#[derive(Debug)]
pub struct HttpClient {
    stream: Option<TcpStream>,
    status: HttpClientStatus,
    host: String,
    port: u16,
    response_code: u16,
    response_headers: Vec<(String, String)>,
    response_body: Vec<u8>,
    read_timeout: Option<Duration>,
    connect_timeout: Duration,
}

impl Default for HttpClient {
    fn default() -> Self {
        Self::new()
    }
}

impl HttpClient {
    /// Creates a new, disconnected client.
    pub fn new() -> Self {
        Self {
            stream: None,
            status: HttpClientStatus::Disconnected,
            host: String::new(),
            port: 0,
            response_code: 0,
            response_headers: Vec::new(),
            response_body: Vec::new(),
            // A sane default so a misbehaving host can't hang the caller forever.
            connect_timeout: Duration::from_secs(30),
            read_timeout: None,
        }
    }

    /// Sets the read timeout applied to the connection (None = block).
    pub fn set_read_timeout(&mut self, timeout: Option<Duration>) {
        self.read_timeout = timeout;
    }

    /// Sets the connection-establishment timeout.
    pub fn set_connect_timeout(&mut self, timeout: Duration) {
        self.connect_timeout = timeout;
    }

    /// Returns the current status.
    pub fn status(&self) -> HttpClientStatus {
        self.status
    }

    /// Connects to `host:port` over plain TCP.
    ///
    /// Updates [`status`](Self::status) and returns the matching result code
    /// on failure (`CantResolve`, `CantConnect`, or `Timeout`).
    pub fn connect_to_host(&mut self, host: &str, port: u16) -> Result<(), HttpResultCode> {
        self.host = host.to_string();
        self.port = port;
        self.status = HttpClientStatus::Resolving;

        let addrs = match (host, port).to_socket_addrs() {
            Ok(addrs) => addrs.collect::<Vec<_>>(),
            Err(_) => {
                self.status = HttpClientStatus::CantResolve;
                return Err(HttpResultCode::CantResolve);
            }
        };
        if addrs.is_empty() {
            self.status = HttpClientStatus::CantResolve;
            return Err(HttpResultCode::CantResolve);
        }

        self.status = HttpClientStatus::Connecting;
        let mut last_err: Option<std::io::Error> = None;
        for addr in &addrs {
            match TcpStream::connect_timeout(addr, self.connect_timeout) {
                Ok(stream) => {
                    let _ = stream.set_read_timeout(self.read_timeout);
                    let _ = stream.set_write_timeout(self.read_timeout);
                    self.stream = Some(stream);
                    self.status = HttpClientStatus::Connected;
                    return Ok(());
                }
                Err(e) => last_err = Some(e),
            }
        }

        let code = match last_err.as_ref().map(|e| e.kind()) {
            Some(ErrorKind::TimedOut) => HttpResultCode::Timeout,
            _ => HttpResultCode::CantConnect,
        };
        self.status = HttpClientStatus::CantConnect;
        Err(code)
    }

    /// Sends a request on the established connection.
    ///
    /// `path` must already be URL-encoded; `headers` are sent verbatim.
    pub fn request(
        &mut self,
        method: HttpMethod,
        path: &str,
        headers: &HashMap<String, String>,
        body: &[u8],
    ) -> Result<(), HttpResultCode> {
        if self.stream.is_none() {
            return Err(HttpResultCode::ConnectionError);
        }

        let mut head = String::new();
        head.push_str(&format!("{} {} HTTP/1.1\r\n", method.as_str(), path));
        if self.port == 80 {
            head.push_str(&format!("Host: {}\r\n", self.host));
        } else {
            head.push_str(&format!("Host: {}:{}\r\n", self.host, self.port));
        }
        head.push_str("User-Agent: PatinaEngine/1.0 (gdplatform)\r\n");
        head.push_str("Accept: */*\r\n");
        head.push_str("Accept-Encoding: identity\r\n");
        head.push_str("Connection: close\r\n");

        let lower_keys: std::collections::HashSet<String> =
            headers.keys().map(|k| k.to_ascii_lowercase()).collect();
        for (k, v) in headers {
            head.push_str(&format!("{k}: {v}\r\n"));
        }
        if !body.is_empty() && !lower_keys.contains("content-length") {
            head.push_str(&format!("Content-Length: {}\r\n", body.len()));
        }
        head.push_str("\r\n");

        let mut packet = head.into_bytes();
        packet.extend_from_slice(body);

        // Scope the stream borrow so it ends before we touch `self.status`.
        let write_result = {
            let stream = self.stream.as_mut().expect("checked Some above");
            stream.write_all(&packet).and_then(|()| stream.flush())
        };
        match write_result {
            Ok(()) => {
                self.status = HttpClientStatus::Requesting;
                Ok(())
            }
            Err(e) => {
                self.status = HttpClientStatus::ConnectionError;
                Err(map_io_error(&e))
            }
        }
    }

    /// Reads the full response (head + body), populating the response fields.
    ///
    /// Relies on `Connection: close` so the body terminates at EOF; honours an
    /// explicit `Content-Length` or `Transfer-Encoding: chunked` when present.
    pub fn read_response(&mut self) -> Result<(), HttpResultCode> {
        let stream = self
            .stream
            .as_mut()
            .ok_or(HttpResultCode::ConnectionError)?;

        let mut buf: Vec<u8> = Vec::new();
        let mut tmp = [0u8; 8192];

        // 1. Read until the end of the header block.
        let header_end = loop {
            if let Some(pos) = find_subsequence(&buf, b"\r\n\r\n") {
                break pos + 4;
            }
            match stream.read(&mut tmp) {
                Ok(0) => {
                    if buf.is_empty() {
                        self.status = HttpClientStatus::ConnectionError;
                        return Err(HttpResultCode::NoResponse);
                    }
                    self.status = HttpClientStatus::ConnectionError;
                    return Err(HttpResultCode::ConnectionError);
                }
                Ok(n) => buf.extend_from_slice(&tmp[..n]),
                Err(e) => {
                    self.status = HttpClientStatus::ConnectionError;
                    return Err(map_io_error(&e));
                }
            }
            if buf.len() > 256 * 1024 {
                self.status = HttpClientStatus::ConnectionError;
                return Err(HttpResultCode::ConnectionError);
            }
        };

        // 2. Parse status line + headers.
        let head = String::from_utf8_lossy(&buf[..header_end]);
        let mut lines = head.split("\r\n");
        let status_line = lines.next().unwrap_or("");
        self.response_code = parse_status_code(status_line)?;

        self.response_headers.clear();
        let mut content_length: Option<usize> = None;
        let mut chunked = false;
        for line in lines {
            if line.is_empty() {
                continue;
            }
            if let Some((k, v)) = line.split_once(':') {
                let k = k.trim().to_string();
                let v = v.trim().to_string();
                let kl = k.to_ascii_lowercase();
                if kl == "content-length" {
                    content_length = v.parse().ok();
                } else if kl == "transfer-encoding" && v.to_ascii_lowercase().contains("chunked") {
                    chunked = true;
                }
                self.response_headers.push((k, v));
            }
        }

        self.status = HttpClientStatus::Body;

        // 3. Read the body.
        let mut body = buf[header_end..].to_vec();
        if let Some(cl) = content_length {
            while body.len() < cl {
                match stream.read(&mut tmp) {
                    Ok(0) => break,
                    Ok(n) => body.extend_from_slice(&tmp[..n]),
                    Err(e) => {
                        self.status = HttpClientStatus::ConnectionError;
                        return Err(map_io_error(&e));
                    }
                }
            }
            body.truncate(cl);
        } else {
            // Chunked or close-delimited: read to EOF, then de-chunk if needed.
            loop {
                match stream.read(&mut tmp) {
                    Ok(0) => break,
                    Ok(n) => body.extend_from_slice(&tmp[..n]),
                    Err(e) => {
                        self.status = HttpClientStatus::ConnectionError;
                        return Err(map_io_error(&e));
                    }
                }
            }
            if chunked {
                body = decode_chunked(&body)?;
            }
        }

        self.response_body = body;
        self.status = HttpClientStatus::Connected;
        Ok(())
    }

    /// Returns the parsed response status code (0 until a response is read).
    pub fn response_code(&self) -> u16 {
        self.response_code
    }

    /// Returns the response headers as ordered `(name, value)` pairs.
    pub fn response_headers(&self) -> &[(String, String)] {
        &self.response_headers
    }

    /// Returns the first header value matching `name` (case-insensitive).
    pub fn header(&self, name: &str) -> Option<&str> {
        let name = name.to_ascii_lowercase();
        self.response_headers
            .iter()
            .find(|(k, _)| k.to_ascii_lowercase() == name)
            .map(|(_, v)| v.as_str())
    }

    /// Returns the response body bytes.
    pub fn response_body(&self) -> &[u8] {
        &self.response_body
    }

    /// Closes the connection and resets to `Disconnected`.
    pub fn close(&mut self) {
        if let Some(stream) = self.stream.take() {
            let _ = stream.shutdown(std::net::Shutdown::Both);
        }
        self.status = HttpClientStatus::Disconnected;
    }

    /// Performs a complete blocking request described by `config`, following
    /// redirects up to `config.max_redirects`, and returns an [`HttpResponse`].
    ///
    /// This is the workhorse used by the [`HTTPRequest`](crate::http_request::HTTPRequest)
    /// node's background worker. Network failures are reported through
    /// [`HttpResponse::result`] rather than as `Err`.
    pub fn perform(config: &HttpRequestConfig) -> HttpResponse {
        let mut url = config.url.clone();
        let mut redirects: u32 = 0;

        loop {
            let parsed = match parse_url(&url) {
                Some(p) => p,
                None => return error_response(HttpResultCode::CantConnect),
            };
            if parsed.tls {
                // No TLS backend wired into gdplatform yet — be honest about it.
                return error_response(HttpResultCode::SslHandshakeError);
            }

            let mut client = HttpClient::new();
            if config.timeout_seconds > 0.0 {
                let d = Duration::from_secs_f64(config.timeout_seconds);
                client.set_read_timeout(Some(d));
                client.set_connect_timeout(d);
            }

            if let Err(code) = client.connect_to_host(&parsed.host, parsed.port) {
                return error_response(code);
            }
            if let Err(code) = client.request(config.method, &parsed.path, &config.headers, &config.body)
            {
                return error_response(code);
            }
            if let Err(code) = client.read_response() {
                return error_response(code);
            }

            let code = client.response_code;
            // Follow 3xx redirects (302/301/303/307/308), excluding 304.
            if (300..400).contains(&code) && code != 304 {
                if let Some(location) = client.header("location").map(|s| s.to_string()) {
                    if redirects >= config.max_redirects {
                        return error_response(HttpResultCode::RedirectLimitReached);
                    }
                    redirects += 1;
                    url = resolve_redirect(&parsed, &location);
                    continue;
                }
            }

            let headers: HashMap<String, String> =
                client.response_headers.iter().cloned().collect();
            return HttpResponse {
                status_code: code,
                headers,
                body: std::mem::take(&mut client.response_body),
                result: HttpResultCode::Success,
            };
        }
    }
}

/// Builds an [`HttpResponse`] carrying only an error result.
fn error_response(result: HttpResultCode) -> HttpResponse {
    HttpResponse {
        status_code: 0,
        headers: HashMap::new(),
        body: Vec::new(),
        result,
    }
}

/// Parses the numeric status code out of an HTTP status line.
fn parse_status_code(status_line: &str) -> Result<u16, HttpResultCode> {
    // "HTTP/1.1 200 OK" -> 200
    let mut parts = status_line.split_whitespace();
    let _version = parts.next();
    let code = parts
        .next()
        .and_then(|c| c.parse::<u16>().ok())
        .ok_or(HttpResultCode::NoResponse)?;
    Ok(code)
}

/// Decodes a `Transfer-Encoding: chunked` body into its raw bytes.
fn decode_chunked(data: &[u8]) -> Result<Vec<u8>, HttpResultCode> {
    let mut out = Vec::new();
    let mut i = 0;
    loop {
        let line_end = match find_subsequence(&data[i..], b"\r\n") {
            Some(p) => i + p,
            None => return Err(HttpResultCode::ConnectionError),
        };
        let size_str = String::from_utf8_lossy(&data[i..line_end]);
        let size_str = size_str.split(';').next().unwrap_or("").trim();
        let size = usize::from_str_radix(size_str, 16)
            .map_err(|_| HttpResultCode::ConnectionError)?;
        i = line_end + 2;
        if size == 0 {
            break;
        }
        if i + size > data.len() {
            return Err(HttpResultCode::ConnectionError);
        }
        out.extend_from_slice(&data[i..i + size]);
        i += size + 2; // skip the trailing CRLF after the chunk data
    }
    Ok(out)
}

/// Resolves a `Location` header against the URL that produced it.
fn resolve_redirect(base: &ParsedUrl, location: &str) -> String {
    if location.contains("://") {
        return location.to_string();
    }
    let authority = if base.port == 80 || base.port == 443 {
        base.host.clone()
    } else {
        format!("{}:{}", base.host, base.port)
    };
    if let Some(rest) = location.strip_prefix('/') {
        format!("{}://{}/{}", base.scheme, authority, rest)
    } else {
        format!("{}://{}/{}", base.scheme, authority, location)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_basic_http_url() {
        let u = parse_url("http://example.com/path?q=1").unwrap();
        assert_eq!(u.scheme, "http");
        assert_eq!(u.host, "example.com");
        assert_eq!(u.port, 80);
        assert_eq!(u.path, "/path?q=1");
        assert!(!u.tls);
    }

    #[test]
    fn parses_explicit_port_and_no_path() {
        let u = parse_url("http://127.0.0.1:8080").unwrap();
        assert_eq!(u.host, "127.0.0.1");
        assert_eq!(u.port, 8080);
        assert_eq!(u.path, "/");
    }

    #[test]
    fn parses_https_default_port_and_tls_flag() {
        let u = parse_url("https://secure.test/").unwrap();
        assert_eq!(u.port, 443);
        assert!(u.tls);
    }

    #[test]
    fn parses_ipv6_literal() {
        let u = parse_url("http://[::1]:9000/x").unwrap();
        assert_eq!(u.host, "::1");
        assert_eq!(u.port, 9000);
        assert_eq!(u.path, "/x");
    }

    #[test]
    fn rejects_url_without_scheme() {
        assert!(parse_url("example.com/path").is_none());
    }

    #[test]
    fn status_line_parsing() {
        assert_eq!(parse_status_code("HTTP/1.1 200 OK").unwrap(), 200);
        assert_eq!(parse_status_code("HTTP/1.0 404 Not Found").unwrap(), 404);
        assert!(parse_status_code("garbage").is_err());
    }

    #[test]
    fn chunked_decoding() {
        // "Wikipedia" in two chunks, per RFC 7230 example shape.
        let body = b"4\r\nWiki\r\n5\r\npedia\r\n0\r\n\r\n";
        let decoded = decode_chunked(body).unwrap();
        assert_eq!(decoded, b"Wikipedia");
    }

    #[test]
    fn https_perform_reports_tls_error() {
        let cfg = HttpRequestConfig {
            url: "https://example.com/".into(),
            ..Default::default()
        };
        let resp = HttpClient::perform(&cfg);
        assert_eq!(resp.result, HttpResultCode::SslHandshakeError);
    }

    #[test]
    fn status_enum_godot_codes() {
        assert_eq!(HttpClientStatus::Disconnected.as_godot_code(), 0);
        assert_eq!(HttpClientStatus::Connected.as_godot_code(), 5);
        assert_eq!(HttpClientStatus::Body.as_godot_code(), 7);
        assert_eq!(HttpClientStatus::TlsHandshakeError.as_godot_code(), 9);
    }
}
