//! pat-vxejb: Per-token rate limit on the editor HTTP server.
//!
//! Acceptance: more than 60 requests per second from one token returns HTTP
//! 429 with a `Retry-After` header, and a different token is unaffected.

use gdeditor::editor_server::{
    EditorServerHandle, EditorState, RateLimiter, DEFAULT_RATE_LIMIT_PER_SECOND,
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

fn get_with_token(port: u16, path: &str, token: &str) -> String {
    let req = format!(
        "GET {path} HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer {token}\r\nConnection: close\r\n\r\n"
    );
    http_raw(port, &req)
}

#[test]
fn one_token_over_60_rps_returns_429_with_retry_after_other_token_unaffected() {
    // The shipped default is the bead's 60 req/s.
    assert_eq!(DEFAULT_RATE_LIMIT_PER_SECOND, 60);

    // ⚠️ Exercise the MECHANISM with a small limit, not the default. The
    // limiter counts requests per whole wall-clock second, so proving 429 at
    // 60/s needs 61 sequential connections inside one second (<16 ms each).
    // Under a loaded full-workspace run they are slower than that, the window
    // rolls over first, and the test never saw a 429 — it measured host speed,
    // not the limiter. At 5/s six requests in one second leaves a ~10x margin.
    let port = free_port();
    let handle = EditorServerHandle::start_with_auth_and_limiter(
        port,
        fixture_state(),
        None,
        RateLimiter::new(5),
    );
    thread::sleep(Duration::from_millis(120));

    // A window boundary can split at most one burst, so 80 attempts is ample.
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
