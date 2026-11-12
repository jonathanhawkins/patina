//! pat-nzgdu.172: distraction-free mode toggle + dock layout save/restore wired
//! into the served editor, with layout persistence through the settings store.
//!
//! Guards:
//!  1. Static — EDITOR_HTML defines the distraction-free toggle and the layout
//!     save/restore wiring against the `panel_sizes` settings store.
//!  2. Functional — a saved dock layout (panel_sizes) persists through the real
//!     `/api/settings` store: POST it, then GET reads it back unchanged.

use gdeditor::editor_server::{EditorServerHandle, EditorState};
use gdeditor::editor_ui::EDITOR_HTML;
use gdscene::SceneTree;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;
use std::time::Duration;

// ---------------------------------------------------------------------------
// (1) Static wiring guards.
// ---------------------------------------------------------------------------

#[test]
fn editor_html_wires_distraction_free_and_layout() {
    assert!(
        EDITOR_HTML.contains("distraction-free-toggle"),
        "EDITOR_HTML must contain the distraction-free toggle control"
    );
    assert!(
        EDITOR_HTML.contains("toggleDistractionFree"),
        "EDITOR_HTML must define the distraction-free toggle handler"
    );
    assert!(
        EDITOR_HTML.contains("saveLayout") && EDITOR_HTML.contains("restoreLayout"),
        "EDITOR_HTML must define layout save + restore"
    );
    assert!(
        EDITOR_HTML.contains("panel_sizes"),
        "layout save/restore must persist through the panel_sizes settings store"
    );
}

// ---------------------------------------------------------------------------
// (2) Functional: a saved layout persists through the settings store.
// ---------------------------------------------------------------------------

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn connect_with_retry(port: u16) -> TcpStream {
    for attempt in 0..20 {
        match TcpStream::connect(format!("127.0.0.1:{port}")) {
            Ok(s) => return s,
            Err(_) if attempt < 19 => thread::sleep(Duration::from_millis(50)),
            Err(e) => panic!("failed to connect: {e}"),
        }
    }
    unreachable!()
}

fn http_req(port: u16, method: &str, path: &str, body: &str) -> String {
    let mut stream = connect_with_retry(port);
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
    let req = if method == "GET" {
        format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
    } else {
        format!(
            "{method} {path} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
    };
    stream.write_all(req.as_bytes()).unwrap();
    let mut resp = Vec::new();
    let _ = stream.read_to_end(&mut resp);
    String::from_utf8_lossy(&resp).to_string()
}

fn extract_body(resp: &str) -> &str {
    resp.split("\r\n\r\n").nth(1).unwrap_or("")
}

#[test]
fn saved_layout_persists_through_settings_store() {
    let port = free_port();
    let handle = EditorServerHandle::start(port, EditorState::new(SceneTree::new()));
    thread::sleep(Duration::from_millis(100));

    // Save a dock layout (side docks + bottom panel sizes) through the store.
    let post = http_req(
        port,
        "POST",
        "/api/settings",
        r#"{"panel_sizes":{"left":250.0,"right":300.0,"bottom":180.0}}"#,
    );
    assert!(
        post.contains("200 OK"),
        "POST /api/settings should 200, got:\n{post}"
    );

    // Reload: GET returns the persisted panel_sizes unchanged.
    let get = http_req(port, "GET", "/api/settings", "");
    assert!(
        get.contains("200 OK"),
        "GET /api/settings should 200, got:\n{get}"
    );
    let v: serde_json::Value =
        serde_json::from_str(extract_body(&get)).expect("settings response should be JSON");
    assert_eq!(
        v["panel_sizes"]["left"].as_f64(),
        Some(250.0),
        "saved left dock size must persist through reload; response: {v}"
    );
    assert_eq!(
        v["panel_sizes"]["bottom"].as_f64(),
        Some(180.0),
        "saved bottom panel size must persist through reload"
    );

    handle.stop();
}
