//! pat-vxejb: Per-token rate limit on the editor HTTP server.
//!
//! Acceptance: more than 60 requests per second from one token returns HTTP
//! 429 with a `Retry-After` header, and a different token is unaffected.

use gdeditor::editor_server::{EditorServerHandle, EditorState};
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

fn get_with_token(port: u16, path: &str, token: &str) -> String {
    let req = format!(
        "GET {path} HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer {token}\r\nConnection: close\r\n\r\n"
    );
    http_raw(port, &req)
}

#[test]
fn one_token_over_60_rps_returns_429_with_retry_after_other_token_unaffected() {
    let port = free_port();
    // Default 60 req/s, open-access mode so any presented token is honored.
    let handle = EditorServerHandle::start(port, fixture_state());
    thread::sleep(Duration::from_millis(120));

    // Burst 80 requests from one token within a single second. After the 60th
    // success we must start seeing 429s.
    let mut seen_429 = false;
    let mut last_429_response = String::new();
    for _ in 0..80 {
        let resp = get_with_token(port, "/api/scene", "token-noisy");
        if status_code(&resp) == 429 {
            seen_429 = true;
            last_429_response = resp;
            break;
        }
    }
    assert!(
        seen_429,
        "noisy token should hit 429 within 80 rapid requests, but never did"
    );
    let retry = header_value(&last_429_response, "Retry-After");
    assert!(
        retry.is_some(),
        "429 response must include a Retry-After header, got:\n{last_429_response}"
    );
    let retry_secs: u32 = retry
        .and_then(|v| v.parse().ok())
        .expect("Retry-After must be an integer-seconds value");
    assert!(
        retry_secs >= 1,
        "Retry-After must suggest at least 1 second, got {retry_secs}"
    );

    // A different token has its own bucket and must succeed immediately.
    let other = get_with_token(port, "/api/scene", "token-quiet");
    assert_eq!(
        status_code(&other),
        200,
        "different token should not be rate-limited; got:\n{other}"
    );

    handle.stop();
}
