//! pat-gpjmt: Bearer-token authentication on the editor HTTP server.
//!
//! Acceptance: every `/api/*` endpoint returns HTTP 401 without a valid
//! `Authorization: Bearer <token>` header and HTTP 200 with one; tokens come
//! from a config file or environment variable.

use gdeditor::editor_server::{editor_auth_token_from_env, EditorServerHandle, EditorState};
use gdscene::node::Node;
use gdscene::SceneTree;

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Mutex;
use std::thread;
use std::time::Duration;

/// Serialize env-var mutations so parallel tests in this binary can't race.
static ENV_LOCK: Mutex<()> = Mutex::new(());

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

/// Send a raw HTTP/1.1 request and return the full response body as a string.
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

/// Representative `/api/*` endpoints with a mix of GET and POST to prove the
/// auth gate is not method-specific. These are the safest endpoints to probe
/// on a fresh `EditorState` (no project loaded, no selection) — they always
/// return 200 with default state when authenticated.
const API_PROBES: &[(&str, &str)] = &[
    ("GET", "/api/scene"),
    ("GET", "/api/selected"),
    ("GET", "/api/runtime/status"),
    ("POST", "/api/undo"),
];

fn build_get(path: &str, token: Option<&str>) -> String {
    let auth = match token {
        Some(t) => format!("Authorization: Bearer {t}\r\n"),
        None => String::new(),
    };
    format!("GET {path} HTTP/1.1\r\nHost: localhost\r\n{auth}Connection: close\r\n\r\n")
}

fn build_post(path: &str, token: Option<&str>, body: &str) -> String {
    let auth = match token {
        Some(t) => format!("Authorization: Bearer {t}\r\n"),
        None => String::new(),
    };
    format!(
        "POST {path} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\n{auth}Connection: close\r\n\r\n{body}",
        body.len()
    )
}

fn probe(port: u16, method: &str, path: &str, token: Option<&str>) -> u16 {
    let req = if method == "POST" {
        build_post(path, token, "{}")
    } else {
        build_get(path, token)
    };
    let resp = http_raw(port, &req);
    status_code(&resp)
}

#[test]
fn missing_authorization_header_returns_401_on_every_api_endpoint() {
    let port = free_port();
    let handle =
        EditorServerHandle::start_with_auth(port, fixture_state(), Some("super-secret".into()));
    thread::sleep(Duration::from_millis(120));

    for (method, path) in API_PROBES {
        let code = probe(port, method, path, None);
        assert_eq!(
            code, 401,
            "{method} {path} should reject missing Bearer token, got HTTP {code}"
        );
    }
    handle.stop();
}

#[test]
fn wrong_bearer_token_returns_401() {
    let port = free_port();
    let handle =
        EditorServerHandle::start_with_auth(port, fixture_state(), Some("super-secret".into()));
    thread::sleep(Duration::from_millis(120));

    let code = probe(port, "GET", "/api/scene", Some("wrong-token"));
    assert_eq!(code, 401, "wrong token must be rejected, got HTTP {code}");

    // Also reject a missing scheme prefix (raw token without "Bearer ").
    let mut stream = connect_with_retry(port);
    stream.write_all(b"GET /api/scene HTTP/1.1\r\nHost: localhost\r\nAuthorization: super-secret\r\nConnection: close\r\n\r\n").unwrap();
    let mut resp = String::new();
    stream.read_to_string(&mut resp).ok();
    assert_eq!(status_code(&resp), 401, "non-Bearer scheme must be rejected");

    handle.stop();
}

#[test]
fn valid_bearer_token_returns_200_on_every_api_endpoint() {
    let port = free_port();
    let handle =
        EditorServerHandle::start_with_auth(port, fixture_state(), Some("super-secret".into()));
    thread::sleep(Duration::from_millis(120));

    for (method, path) in API_PROBES {
        let code = probe(port, method, path, Some("super-secret"));
        assert_eq!(
            code, 200,
            "{method} {path} should accept valid Bearer token, got HTTP {code}"
        );
    }
    handle.stop();
}

#[test]
fn open_access_mode_allows_unauthenticated_calls() {
    // Back-compat: when started without a token, every endpoint stays open.
    let port = free_port();
    let handle = EditorServerHandle::start(port, fixture_state());
    thread::sleep(Duration::from_millis(120));

    let code = probe(port, "GET", "/api/scene", None);
    assert_eq!(
        code, 200,
        "no-auth server should accept unauthenticated /api/scene"
    );
    handle.stop();
}

#[test]
fn auth_token_can_come_from_environment_variable() {
    let _guard = ENV_LOCK.lock().unwrap();
    let prev_tok = std::env::var("PATINA_EDITOR_TOKEN").ok();
    let prev_file = std::env::var("PATINA_EDITOR_TOKEN_FILE").ok();
    std::env::set_var("PATINA_EDITOR_TOKEN", "env-sourced-token");
    std::env::remove_var("PATINA_EDITOR_TOKEN_FILE");

    let token = editor_auth_token_from_env();
    assert_eq!(token.as_deref(), Some("env-sourced-token"));

    let port = free_port();
    let handle = EditorServerHandle::start_with_auth(port, fixture_state(), token);
    thread::sleep(Duration::from_millis(120));
    assert_eq!(probe(port, "GET", "/api/scene", None), 401);
    assert_eq!(probe(port, "GET", "/api/scene", Some("env-sourced-token")), 200);
    handle.stop();

    // Restore env to avoid leaking state into sibling tests.
    if let Some(v) = prev_tok {
        std::env::set_var("PATINA_EDITOR_TOKEN", v);
    } else {
        std::env::remove_var("PATINA_EDITOR_TOKEN");
    }
    if let Some(v) = prev_file {
        std::env::set_var("PATINA_EDITOR_TOKEN_FILE", v);
    }
}

#[test]
fn auth_token_can_come_from_config_file() {
    let _guard = ENV_LOCK.lock().unwrap();
    let prev_tok = std::env::var("PATINA_EDITOR_TOKEN").ok();
    let prev_file = std::env::var("PATINA_EDITOR_TOKEN_FILE").ok();

    let dir = std::env::temp_dir().join(format!("patina-auth-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("editor_token");
    std::fs::write(&path, "  file-sourced-token  \n").unwrap();

    std::env::remove_var("PATINA_EDITOR_TOKEN");
    std::env::set_var("PATINA_EDITOR_TOKEN_FILE", &path);

    let token = editor_auth_token_from_env();
    assert_eq!(token.as_deref(), Some("file-sourced-token"));

    let port = free_port();
    let handle = EditorServerHandle::start_with_auth(port, fixture_state(), token);
    thread::sleep(Duration::from_millis(120));
    assert_eq!(probe(port, "GET", "/api/scene", None), 401);
    assert_eq!(probe(port, "GET", "/api/scene", Some("file-sourced-token")), 200);
    handle.stop();

    std::fs::remove_file(&path).ok();
    std::env::remove_var("PATINA_EDITOR_TOKEN_FILE");
    if let Some(v) = prev_tok {
        std::env::set_var("PATINA_EDITOR_TOKEN", v);
    }
    if let Some(v) = prev_file {
        std::env::set_var("PATINA_EDITOR_TOKEN_FILE", v);
    }
}
