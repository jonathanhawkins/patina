//! pat-aivim: Sandbox the editor filesystem endpoints to the project root.
//!
//! Acceptance: `/api/filesystem`, `/api/filesystem/tree`, `/api/filesystem/delete`,
//! `/api/filesystem/mkdir`, and `/api/filesystem/rename` all reject paths
//! containing `..` or pointing outside the project root with HTTP 403 and a
//! machine-readable error code (`E_PATH_SANDBOX`).

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

fn response_body(response: &str) -> &str {
    response.split("\r\n\r\n").nth(1).unwrap_or("")
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

/// Each filesystem endpoint must produce a 403 with `E_PATH_SANDBOX` when
/// asked to touch a path that contains `..` or escapes the project root.
#[test]
fn filesystem_endpoints_reject_traversal_with_403_and_machine_code() {
    let port = free_port();
    let handle = EditorServerHandle::start(port, fixture_state());
    thread::sleep(Duration::from_millis(120));

    // `..` traversal — every endpoint must refuse.
    let traversal_cases: &[(&str, &str, Option<&str>)] = &[
        ("GET", "/api/filesystem?path=res://../etc/passwd", None),
        ("GET", "/api/filesystem/tree?path=res://../etc/passwd", None),
        (
            "POST",
            "/api/filesystem/delete",
            Some(r#"{"path":"res://../etc/passwd"}"#),
        ),
        (
            "POST",
            "/api/filesystem/mkdir",
            Some(r#"{"path":"res://../escape"}"#),
        ),
        (
            "POST",
            "/api/filesystem/rename",
            Some(r#"{"old_path":"res://../etc/passwd","new_name":"x"}"#),
        ),
    ];
    for (method, path, body) in traversal_cases {
        let resp = if *method == "GET" {
            http_get(port, path)
        } else {
            http_post(port, path, body.unwrap_or("{}"))
        };
        let code = status_code(&resp);
        assert_eq!(
            code, 403,
            "{method} {path} should reject `..` traversal with HTTP 403, got {code}\n--- response ---\n{resp}",
        );
        let body = response_body(&resp);
        assert!(
            body.contains(r#""code":"E_PATH_SANDBOX""#),
            "{method} {path} response body must carry machine-readable code E_PATH_SANDBOX, got: {body}",
        );
    }

    // Absolute paths outside the project root — every endpoint must refuse.
    let absolute_cases: &[(&str, &str, Option<&str>)] = &[
        ("GET", "/api/filesystem?path=/etc/passwd", None),
        ("GET", "/api/filesystem/tree?path=/etc/passwd", None),
        (
            "POST",
            "/api/filesystem/delete",
            Some(r#"{"path":"/etc/passwd"}"#),
        ),
        (
            "POST",
            "/api/filesystem/mkdir",
            Some(r#"{"path":"/tmp/patina-escape"}"#),
        ),
        (
            "POST",
            "/api/filesystem/rename",
            Some(r#"{"old_path":"/etc/passwd","new_name":"x"}"#),
        ),
    ];
    for (method, path, body) in absolute_cases {
        let resp = if *method == "GET" {
            http_get(port, path)
        } else {
            http_post(port, path, body.unwrap_or("{}"))
        };
        let code = status_code(&resp);
        assert_eq!(
            code, 403,
            "{method} {path} should reject absolute paths with HTTP 403, got {code}\n--- response ---\n{resp}",
        );
        let body = response_body(&resp);
        assert!(
            body.contains(r#""code":"E_PATH_SANDBOX""#),
            "{method} {path} response body must carry machine-readable code E_PATH_SANDBOX, got: {body}",
        );
    }

    // `rename` also has to reject a `new_name` that itself contains traversal.
    let resp = http_post(
        port,
        "/api/filesystem/rename",
        r#"{"old_path":"res://Cargo.toml","new_name":"../escape"}"#,
    );
    let code = status_code(&resp);
    assert_eq!(
        code, 403,
        "rename should reject `..` in new_name with HTTP 403, got {code}\n--- response ---\n{resp}",
    );
    let body = response_body(&resp);
    assert!(
        body.contains(r#""code":"E_PATH_SANDBOX""#),
        "rename traversal new_name must carry E_PATH_SANDBOX, got: {body}",
    );

    handle.stop();
}
