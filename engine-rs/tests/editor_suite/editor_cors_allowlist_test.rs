//! pat-bof7u: Configurable CORS origin allowlist on the editor HTTP server.
//!
//! Acceptance: a request with `Origin` not in the configured allowlist returns
//! HTTP 403, and a request with an allowed `Origin` returns the matching
//! `Access-Control-Allow-Origin` header.

use gdeditor::editor_server::{
    CorsAllowlist, EditorServerHandle, EditorState, RateLimiter,
};
use gdscene::node::Node;
use gdscene::SceneTree;

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;
use std::time::Duration;

fn free_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.local_addr().unwrap().port()
}

fn fixture_state() -> EditorState {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    tree.add_child(root, Node::new("Main", "Node2D")).unwrap();
    EditorState::new(tree)
}

fn connect_with_retry(port: u16) -> TcpStream {
    for _ in 0..40 {
        if let Ok(s) = TcpStream::connect(format!("127.0.0.1:{port}")) {
            return s;
        }
        thread::sleep(Duration::from_millis(25));
    }
    panic!("could not connect to editor server on port {port}");
}

fn http_raw(port: u16, request: &str) -> String {
    let mut stream = connect_with_retry(port);
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    stream.write_all(request.as_bytes()).unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).ok();
    response
}

fn status_code(response: &str) -> u16 {
    let first = response.lines().next().unwrap_or("");
    let parts: Vec<&str> = first.split_whitespace().collect();
    parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(0)
}

fn header_value<'a>(response: &'a str, name: &str) -> Option<&'a str> {
    response.lines().find_map(|line| {
        let mut parts = line.splitn(2, ':');
        let key = parts.next()?.trim();
        if !key.eq_ignore_ascii_case(name) {
            return None;
        }
        Some(parts.next()?.trim())
    })
}

fn get_with_origin(port: u16, path: &str, origin: &str) -> String {
    let req = format!(
        "GET {path} HTTP/1.1\r\nHost: localhost\r\nOrigin: {origin}\r\nConnection: close\r\n\r\n"
    );
    http_raw(port, &req)
}

fn start_server_with_allowlist<I: IntoIterator<Item = &'static str>>(
    origins: I,
) -> (EditorServerHandle, u16) {
    let port = free_port();
    let handle = EditorServerHandle::start_full(
        port,
        fixture_state(),
        None,
        RateLimiter::default(),
        CorsAllowlist::new(origins.into_iter().map(String::from)),
    );
    thread::sleep(Duration::from_millis(120));
    (handle, port)
}

#[test]
fn origin_not_in_allowlist_is_rejected_with_403() {
    let (handle, port) = start_server_with_allowlist(["https://allowed.example.com"]);
    let resp = get_with_origin(port, "/api/scene", "https://evil.example.com");
    let code = status_code(&resp);
    assert_eq!(
        code, 403,
        "Origin outside the allowlist should be refused with HTTP 403; got {code}\n--- response ---\n{resp}"
    );
    // Body should carry a machine-readable code so clients/SREs can match on it.
    let body = resp.split("\r\n\r\n").nth(1).unwrap_or("");
    assert!(
        body.contains(r#""code":"E_CORS_ORIGIN""#),
        "403 body must include machine-readable code, got: {body}"
    );
    handle.stop();
}

#[test]
fn origin_in_allowlist_receives_matching_access_control_allow_origin() {
    let (handle, port) =
        start_server_with_allowlist(["https://allowed.example.com", "http://localhost:5173"]);
    let resp = get_with_origin(port, "/api/scene", "https://allowed.example.com");
    assert_eq!(
        status_code(&resp),
        200,
        "Allowed Origin must reach the handler with HTTP 200\n--- response ---\n{resp}"
    );
    let allow = header_value(&resp, "Access-Control-Allow-Origin");
    assert_eq!(
        allow,
        Some("https://allowed.example.com"),
        "Access-Control-Allow-Origin must echo the matched Origin, got {allow:?}"
    );
    // And a second allowed origin still gets its own echo (no wildcard regression).
    let resp2 = get_with_origin(port, "/api/scene", "http://localhost:5173");
    assert_eq!(
        header_value(&resp2, "Access-Control-Allow-Origin"),
        Some("http://localhost:5173"),
    );
    handle.stop();
}
