//! pat-kts88: Append-only audit log for state-mutating editor REST calls.
//!
//! Acceptance: a log file at `.editor/audit.log` captures
//! `(timestamp, token_id, method, path, status, body_hash)` one line per
//! state-mutating call, and read-only routes (GET) are not recorded.

use gdeditor::editor_server::{audit_log_path, EditorServerHandle, EditorState};
use gdscene::node::Node;
use gdscene::SceneTree;

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Mutex;
use std::thread;
use std::time::Duration;

/// Cargo runs integration-test binaries in parallel by default. We mutate the
/// process-wide cwd here so we MUST serialise across the whole binary.
static AUDIT_TEST_LOCK: Mutex<()> = Mutex::new(());

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

fn http_get(port: u16, path: &str) -> String {
    let req = format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n");
    http_raw(port, &req)
}

fn http_post(port: u16, path: &str, body: &str) -> String {
    let req = format!(
        "POST {path} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    http_raw(port, &req)
}

/// Run `f` with `cwd` set to a freshly-created temp directory and any prior
/// `.editor/audit.log` removed. Restores the cwd on the way out.
fn with_isolated_cwd<F: FnOnce(&std::path::Path)>(f: F) {
    let _guard = AUDIT_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let prev = std::env::current_dir().expect("current_dir failed");
    let dir = std::env::temp_dir().join(format!(
        "patina-audit-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    std::env::set_current_dir(&dir).unwrap();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        f(&dir);
    }));
    let _ = std::env::set_current_dir(&prev);
    let _ = std::fs::remove_dir_all(&dir);
    if let Err(payload) = result {
        std::panic::resume_unwind(payload);
    }
}

fn read_log_lines() -> Vec<String> {
    match std::fs::read_to_string(audit_log_path()) {
        Ok(s) => s
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(|l| l.to_string())
            .collect(),
        Err(_) => Vec::new(),
    }
}

#[test]
fn mutating_calls_are_recorded_and_get_is_not() {
    with_isolated_cwd(|_dir| {
        let port = free_port();
        let handle = EditorServerHandle::start(port, fixture_state());
        thread::sleep(Duration::from_millis(120));

        // GET — must NOT appear in the audit log.
        let _ = http_get(port, "/api/scene");
        let _ = http_get(port, "/api/filesystem");

        // POST — at least one well-formed mutation. /api/undo takes an empty
        // body and always returns 200, which makes it the most reliable POST
        // probe for the audit-log shape.
        let _ = http_post(port, "/api/undo", "");

        // Give the server a beat to flush the log line.
        thread::sleep(Duration::from_millis(100));
        handle.stop();

        let lines = read_log_lines();
        assert!(
            !lines.is_empty(),
            "audit log should contain at least one entry after a POST"
        );
        // Every recorded entry must be from a mutating method — no GET.
        for line in &lines {
            assert!(
                !line.contains(r#""method":"GET""#),
                "GET requests must not appear in the audit log: {line}"
            );
        }
        // The /api/undo POST must be recorded with all six required fields.
        let post = lines
            .iter()
            .find(|l| l.contains(r#""path":"/api/undo""#))
            .expect("POST /api/undo should be recorded");
        for field in [
            "\"timestamp\":",
            "\"token_id\":",
            "\"method\":",
            "\"path\":",
            "\"status\":",
            "\"body_hash\":",
        ] {
            assert!(
                post.contains(field),
                "audit entry must carry field {field}: {post}"
            );
        }
        assert!(
            post.contains(r#""method":"POST""#),
            "method field must be POST: {post}"
        );
        // body_hash is a 16-hex digest of the (empty) body.
        assert!(
            post.contains(r#""body_hash":""#),
            "body_hash must be a quoted string: {post}"
        );
        // The status must be a numeric field (not quoted) so machine
        // consumers can parse it directly.
        assert!(
            post.contains(r#""status":2"#) || post.contains(r#""status":4"#),
            "status must be an unquoted HTTP status: {post}"
        );
    });
}

#[test]
fn audit_log_is_append_only_across_calls() {
    with_isolated_cwd(|_dir| {
        let port = free_port();
        let handle = EditorServerHandle::start(port, fixture_state());
        thread::sleep(Duration::from_millis(120));

        for _ in 0..3 {
            let _ = http_post(port, "/api/undo", "");
        }
        thread::sleep(Duration::from_millis(100));
        handle.stop();

        let lines = read_log_lines();
        assert!(
            lines.len() >= 3,
            "three POSTs should produce at least three audit lines, got {}",
            lines.len()
        );
    });
}
