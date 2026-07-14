//! App-level test for the Signals dock **go-to-method** + **auto-create
//! receiver stub** flow (pat-jwuv1.4).
//!
//! Per `prd/EDITOR_PARITY_EXIT.md`, an editor-parity criterion is satisfied only
//! when an APP-LEVEL test boots the real editor (`EditorServerHandle::start`)
//! and drives it over HTTP the way a user does — a lib-model unit test on the
//! isolated `signal_goto_method` / `signal_receiver_stub` functions is
//! necessary but insufficient. This test exercises both features through the
//! running server's REST API against a real `.gd` file on disk.

use gdeditor::editor_server::{EditorServerHandle, EditorState};
use gdscene::SceneTree;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;
use std::time::Duration;

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn make_server() -> (EditorServerHandle, u16) {
    let port = free_port();
    let tree = SceneTree::new();
    let state = EditorState::new(tree);
    let handle = EditorServerHandle::start(port, state);
    thread::sleep(Duration::from_millis(100));
    (handle, port)
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

fn http_post(port: u16, path: &str, body: &str) -> String {
    let mut stream = connect_with_retry(port);
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
    let request = format!(
        "POST {path} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(request.as_bytes()).unwrap();
    let mut resp = Vec::new();
    let _ = stream.read_to_end(&mut resp);
    String::from_utf8_lossy(&resp).to_string()
}

fn extract_body(resp: &str) -> &str {
    resp.split("\r\n\r\n").nth(1).unwrap_or("")
}

/// A unique temp `.gd` path for this test run (port keeps it collision-free
/// across parallel test binaries).
fn temp_script_path(port: u16) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "patina_signals_goto_autocreate_{}_{}.gd",
        std::process::id(),
        port
    ))
}

/// Acceptance (pat-jwuv1.4): driving the running editor over HTTP,
/// auto-creating a receiver stub for a not-yet-defined method appends a
/// correctly-signatured `func` to the script, and go-to-method then resolves
/// that connection to the newly-created definition's line.
#[test]
fn editor_signals_goto_and_autocreate() {
    let (handle, port) = make_server();
    let path = temp_script_path(port);
    let path_str = path.to_string_lossy().to_string();

    // Receiver script exists but does NOT yet define the connected method.
    let initial = "extends Button\n\nfunc _ready():\n\tpass\n";
    std::fs::write(&path, initial).expect("write initial script");

    // --- Auto-create the receiver stub for a method that doesn't exist yet ---
    let create_body = format!(
        r#"{{"path":"{}","method":"_on_button_pressed","params":["pressed"]}}"#,
        path_str
    );
    let resp = http_post(port, "/api/signal/autocreate_receiver", &create_body);
    assert!(
        resp.contains("200 OK"),
        "autocreate_receiver should succeed, got: {resp}"
    );
    let v: serde_json::Value = serde_json::from_str(extract_body(&resp)).unwrap();
    assert_eq!(
        v["created"].as_bool(),
        Some(true),
        "a stub should be created for the new method"
    );

    // The stub was written to disk with the signal's parameter list.
    let after = std::fs::read_to_string(&path).expect("read updated script");
    assert!(
        after.contains("func _on_button_pressed(pressed):\n\tpass"),
        "script should now contain the signatured stub, got:\n{after}"
    );

    // --- Go-to-method resolves the connection to the new definition's line ---
    let goto_body = format!(
        r#"{{"path":"{}","method":"_on_button_pressed"}}"#,
        path_str
    );
    let resp = http_post(port, "/api/signal/goto_method", &goto_body);
    assert!(
        resp.contains("200 OK"),
        "goto_method should succeed, got: {resp}"
    );
    let v: serde_json::Value = serde_json::from_str(extract_body(&resp)).unwrap();
    let line = v["line"].as_u64().expect("goto returns a line") as usize;
    let expected_line = after
        .lines()
        .position(|l| l.trim_start().starts_with("func _on_button_pressed"))
        .map(|i| i + 1)
        .expect("the stub func is in the file");
    assert_eq!(
        line, expected_line,
        "navigation should target the receiver method definition"
    );
    assert_eq!(
        v["script"].as_str(),
        Some(path_str.as_str()),
        "navigation targets the receiver script"
    );

    // --- Auto-create is idempotent: connecting to an existing method is a no-op
    let resp = http_post(port, "/api/signal/autocreate_receiver", &create_body);
    let v: serde_json::Value = serde_json::from_str(extract_body(&resp)).unwrap();
    assert_eq!(
        v["created"].as_bool(),
        Some(false),
        "no stub should be created when the method already exists"
    );

    // --- Go-to-method for an undefined method yields no navigation (404) ---
    let missing_body = format!(r#"{{"path":"{}","method":"_on_missing"}}"#, path_str);
    let resp = http_post(port, "/api/signal/goto_method", &missing_body);
    assert!(
        resp.contains("404"),
        "goto_method for an undefined method should 404, got: {resp}"
    );

    let _ = std::fs::remove_file(&path);
    handle.stop();
}
