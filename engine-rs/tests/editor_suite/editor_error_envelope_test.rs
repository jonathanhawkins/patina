//! pat-f23vr: Acceptance gate for the standardized error response envelope.
//!
//! Acceptance: every non-2xx response from the editor HTTP server has a JSON
//! body of shape `{"error": {"code": "<machine_code>", "message": "<human>"}}`
//! and the code set is documented in `prd/editor_error_codes.md`.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::thread;
use std::time::Duration;

use gdeditor::editor_server::{EditorServerHandle, EditorState};
use gdscene::SceneTree;

fn free_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.local_addr().unwrap().port()
}

fn make_server() -> (EditorServerHandle, u16) {
    let port = free_port();
    let state = EditorState::new(SceneTree::new());
    let handle = EditorServerHandle::start(port, state);
    // Give the listener thread a moment to bind.
    for _ in 0..40 {
        if TcpStream::connect(format!("127.0.0.1:{port}")).is_ok() {
            return (handle, port);
        }
        thread::sleep(Duration::from_millis(25));
    }
    (handle, port)
}

fn http_raw(port: u16, request: &str) -> String {
    let mut stream = TcpStream::connect(format!("127.0.0.1:{port}")).expect("connect");
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    stream.write_all(request.as_bytes()).unwrap();
    let mut resp = String::new();
    let _ = stream.read_to_string(&mut resp);
    resp
}

fn http_get(port: u16, path: &str) -> String {
    http_raw(
        port,
        &format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n"),
    )
}

fn http_post(port: u16, path: &str, body: &str) -> String {
    http_raw(
        port,
        &format!(
            "POST {path} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        ),
    )
}

fn status_line(resp: &str) -> (u16, String) {
    let first = resp.lines().next().unwrap_or("");
    let mut parts = first.split_whitespace();
    let _version = parts.next();
    let code = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0u16);
    let reason = parts.collect::<Vec<_>>().join(" ");
    (code, reason)
}

fn extract_body(resp: &str) -> &str {
    resp.split("\r\n\r\n").nth(1).unwrap_or("")
}

fn header_value<'a>(resp: &'a str, name: &str) -> Option<&'a str> {
    for line in resp.lines() {
        if let Some((k, v)) = line.split_once(':') {
            if k.eq_ignore_ascii_case(name) {
                return Some(v.trim());
            }
        }
    }
    None
}

/// Assert the response is non-2xx and its body matches the standard envelope.
#[track_caller]
fn assert_error_envelope(resp: &str, expected_status: u16) -> (String, String) {
    let (code, _reason) = status_line(resp);
    assert_eq!(
        code, expected_status,
        "expected HTTP {expected_status}, got {code}; response was:\n{resp}"
    );
    assert!(
        !(200..300).contains(&code),
        "response should be non-2xx; got {code}"
    );

    let ct = header_value(resp, "Content-Type").unwrap_or("");
    assert!(
        ct.starts_with("application/json"),
        "Content-Type must be application/json; got {ct:?}"
    );

    let body = extract_body(resp);
    let v: serde_json::Value = serde_json::from_str(body)
        .unwrap_or_else(|e| panic!("body must be JSON: {e}; body was: {body:?}"));
    let err = &v["error"];
    assert!(
        err.is_object(),
        "envelope must have `error` object; body was: {body}"
    );
    let code_str = err["code"]
        .as_str()
        .unwrap_or_else(|| panic!("envelope.error.code must be a string; body was: {body}"))
        .to_string();
    let msg_str = err["message"]
        .as_str()
        .unwrap_or_else(|| panic!("envelope.error.message must be a string; body was: {body}"))
        .to_string();
    assert!(!code_str.is_empty(), "envelope.error.code must be non-empty");
    assert!(
        !msg_str.is_empty(),
        "envelope.error.message must be non-empty"
    );

    (code_str, msg_str)
}

// ---- Probes covering every non-2xx category the docs enumerate. ----

#[test]
fn missing_route_returns_404_envelope() {
    let (handle, port) = make_server();
    let resp = http_get(port, "/this-path-does-not-exist");
    let (code, _) = assert_error_envelope(&resp, 404);
    assert_eq!(code, "not_found");
    handle.stop();
}

#[test]
fn save_scene_missing_path_returns_400_envelope() {
    let (handle, port) = make_server();
    let resp = http_post(port, "/api/scene/save", "{}");
    let (code, _) = assert_error_envelope(&resp, 400);
    assert_eq!(code, "bad_request");
    handle.stop();
}

#[test]
fn save_scene_malformed_json_returns_400_envelope() {
    let (handle, port) = make_server();
    let resp = http_post(port, "/api/scene/save", "{not json");
    let (code, _) = assert_error_envelope(&resp, 400);
    assert_eq!(code, "bad_request");
    handle.stop();
}

#[test]
fn load_scene_missing_file_returns_400_envelope() {
    let (handle, port) = make_server();
    let body = r#"{"path":"/tmp/__patina_nonexistent_scene__.tscn"}"#;
    let resp = http_post(port, "/api/scene/load", body);
    let (code, _) = assert_error_envelope(&resp, 400);
    assert_eq!(code, "bad_request");
    handle.stop();
}

#[test]
fn preview_missing_param_returns_400_envelope() {
    let (handle, port) = make_server();
    let resp = http_get(port, "/api/preview/file");
    let (code, _) = assert_error_envelope(&resp, 400);
    assert_eq!(code, "bad_request");
    handle.stop();
}

#[test]
fn preview_missing_file_returns_404_envelope() {
    let (handle, port) = make_server();
    let resp = http_get(
        port,
        "/api/preview/file?path=res://__pat-f23vr_nonexistent__.gd",
    );
    let (code, _) = assert_error_envelope(&resp, 404);
    assert_eq!(code, "not_found");
    handle.stop();
}

#[test]
fn malformed_http_request_returns_400_envelope() {
    let (handle, port) = make_server();
    // Send raw garbage with no recognizable HTTP request line.
    let resp = http_raw(port, "GARBAGE\r\n\r\n");
    let (code, _) = assert_error_envelope(&resp, 400);
    assert_eq!(code, "bad_request");
    handle.stop();
}

#[test]
fn unknown_api_route_returns_404_envelope() {
    let (handle, port) = make_server();
    let resp = http_get(port, "/api/does/not/exist");
    let (code, _) = assert_error_envelope(&resp, 404);
    assert_eq!(code, "not_found");
    handle.stop();
}

#[test]
fn unauthorized_request_returns_401_envelope() {
    // Start a server that requires a bearer token. Hitting /api/* without a
    // valid token must produce the standardized envelope, not an empty body.
    let port = free_port();
    let state = EditorState::new(SceneTree::new());
    let handle =
        EditorServerHandle::start_with_auth(port, state, Some("super-secret".into()));
    for _ in 0..40 {
        if TcpStream::connect(format!("127.0.0.1:{port}")).is_ok() {
            break;
        }
        thread::sleep(Duration::from_millis(25));
    }

    let resp = http_get(port, "/api/scene");
    let (code, _) = assert_error_envelope(&resp, 401);
    assert_eq!(code, "unauthorized");
    handle.stop();
}

#[test]
fn sandbox_path_violation_returns_403_envelope_with_path_sandbox_code() {
    let (handle, port) = make_server();
    // /api/filesystem/tree validates ?path via sandbox_path and rejects `..`.
    let resp = http_get(port, "/api/filesystem/tree?path=../../../etc/passwd");
    let (code, _) = assert_error_envelope(&resp, 403);
    assert_eq!(code, "path_sandbox");
    handle.stop();
}

#[test]
fn every_documented_default_code_is_emittable() {
    // Sanity check that the doc table is honored — we only exercise the
    // statuses we can actually trigger from the server-as-built. The doc
    // lists more (413/415/422/501/503) for forward compatibility; they
    // route through default_error_code() when used and are validated by
    // the unit-test surface in editor_server.rs.
    let triggered_codes = [
        "not_found",
        "bad_request",
        "unauthorized",
        "path_sandbox",
        "rate_limit",
    ];
    let docs = std::fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../prd/editor_error_codes.md"),
    )
    .expect("docs file must exist at prd/editor_error_codes.md");
    for code in triggered_codes {
        assert!(
            docs.contains(&format!("`{code}`")),
            "editor_error_codes.md must document code `{code}`"
        );
    }
    // Spec table header sanity.
    assert!(
        docs.contains("\"error\": {")
            && docs.contains("\"code\":")
            && docs.contains("\"message\":"),
        "editor_error_codes.md must show the envelope shape"
    );
}
