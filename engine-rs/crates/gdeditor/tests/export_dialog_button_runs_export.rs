//! pat-mgixk.109 — the served Export dialog can actually RUN an export.
//!
//! # What was broken
//!
//! `POST /api/export/run` existed and worked, but **nothing in the page ever
//! called it**: `grep -nE 'export/run|Export Project|Export PCK|Export All'
//! editor_ui.rs` found zero hits. The dialog could create, edit and delete
//! presets and never produce a build — the route and the UI had been built on
//! opposite sides of a gap.
//!
//! Godot's dialog (`editor/export/project_export.cpp`) offers `Export
//! Project...`, `Export PCK/ZIP...` and `Export All`, so all three are wired
//! here, plus a PCK/ZIP-only server mode that packs `res://` without a runtime.
//!
//! # What this test drives
//!
//! The **real HTTP server**, plus the served HTML itself for the half that is a
//! UI gap. Asserting only the route would have passed before this bead — the
//! route already worked; the missing piece was a caller.

use gdeditor::editor_server::{EditorServerHandle, EditorState};
use gdeditor::editor_ui::EDITOR_HTML;
use gdscene::node::Node;
use gdscene::SceneTree;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;
use std::time::Duration;

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

fn http_request_str(port: u16, request: &str) -> String {
    let mut stream = connect_with_retry(port);
    stream.set_read_timeout(Some(Duration::from_secs(20))).ok();
    stream.write_all(request.as_bytes()).unwrap();
    let mut resp = Vec::new();
    let _ = stream.read_to_end(&mut resp);
    String::from_utf8_lossy(&resp).to_string()
}

fn http_post(port: u16, path: &str, body: &str) -> String {
    http_request_str(
        port,
        &format!(
            "POST {path} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        ),
    )
}

fn body_of(resp: &str) -> &str {
    resp.split("\r\n\r\n").nth(1).unwrap_or("")
}

fn temp_project(tag: &str) -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "patina_mgixk109_{}_{}_{}",
        tag,
        std::process::id(),
        n
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("project.godot"),
        "[application]\nconfig/name=\"ExportRunE2E\"\n",
    )
    .unwrap();
    dir
}

fn server_on_temp_project(tag: &str) -> (EditorServerHandle, u16, PathBuf) {
    let dir = temp_project(tag);
    let port = free_port();
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    tree.add_child(root, Node::new("Main", "Node2D")).unwrap();
    let handle = EditorServerHandle::start(port, EditorState::new(tree));
    thread::sleep(Duration::from_millis(100));
    let open = http_post(
        port,
        "/api/project/open",
        &serde_json::json!({ "path": dir.display().to_string() }).to_string(),
    );
    assert!(
        open.contains("200 OK"),
        "/api/project/open must succeed for the temp project, got: {open}"
    );
    (handle, port, dir)
}

/// Creates a preset through the same route the dialog uses.
fn create_preset(port: u16, preset: serde_json::Value) {
    let resp = http_post(port, "/api/export/presets", &preset.to_string());
    assert!(
        resp.contains("200 OK"),
        "creating the preset must succeed, got: {resp}"
    );
}

/// **The acceptance test.**
#[test]
fn export_dialog_button_runs_export() {
    // -- 0. the UI half: the dialog must be able to invoke the route ----------
    //
    // 🔑 This is the part that was actually missing. The route below already
    // worked before this bead, so a test that only exercised HTTP would have
    // passed against a dialog with no way to reach it.
    for needle in [
        "id=\"exp-run\"",
        "id=\"exp-run-pck\"",
        "id=\"exp-run-all\"",
        "/api/export/run",
    ] {
        assert!(
            EDITOR_HTML.contains(needle),
            "the served editor must contain `{needle}` — without it the export \
             dialog cannot start an export, which is the gap this bead closes"
        );
    }
    // The buttons must carry Godot's own labels, so the dialog is recognisable.
    for label in ["Export Project", "Export PCK/ZIP", "Export All"] {
        assert!(
            EDITOR_HTML.contains(label),
            "the dialog must offer `{label}`, as Godot's project_export.cpp does"
        );
    }

    let (_handle, port, dir) = server_on_temp_project("run");

    // -- 1. a Web preset with NO wasm path reports the server's own error -----
    //
    // Checked before the success case: it proves the error text reaches the
    // caller, which is what the dialog surfaces inline.
    create_preset(
        port,
        serde_json::json!({
            "name": "WebNoWasm",
            "platform": "web",
            "build_profile": "release",
            "export_path": "export/nowasm/index.html",
            "app_name": "",
            "include_filters": [],
            "exclude_filters": [],
            "custom_properties": {}
        }),
    );
    let resp = http_post(
        port,
        "/api/export/run",
        &serde_json::json!({ "name": "WebNoWasm", "debug": false }).to_string(),
    );
    assert!(
        resp.contains("400"),
        "a web export with no wasm payload must fail, got: {resp}"
    );
    assert!(
        body_of(&resp).contains("needs a compiled wasm payload"),
        "the response must carry the server's explanation so the dialog can \
         show it inline; got body: {}",
        body_of(&resp)
    );

    // -- 2. a Web preset WITH a wasm path produces a bundle ------------------
    let wasm = dir.join("patina_web.wasm");
    std::fs::write(&wasm, b"\0asm\x01\0\0\0").unwrap();
    create_preset(
        port,
        serde_json::json!({
            "name": "WebOk",
            "platform": "web",
            "build_profile": "release",
            "export_path": "export/web/index.html",
            "app_name": "",
            "include_filters": [],
            "exclude_filters": [],
            "custom_properties": { "patina/wasm_path": wasm.display().to_string() }
        }),
    );
    let resp = http_post(
        port,
        "/api/export/run",
        &serde_json::json!({ "name": "WebOk", "debug": false }).to_string(),
    );
    assert!(
        resp.contains("200 OK"),
        "the web export must succeed once a wasm payload is named, got: {resp}"
    );
    let json: serde_json::Value = serde_json::from_str(body_of(&resp))
        .unwrap_or_else(|e| panic!("body is not JSON ({e}): {resp}"));
    assert_eq!(
        json.get("ok").and_then(|v| v.as_bool()),
        Some(true),
        "the run must report ok, got: {json}"
    );
    let out = json
        .get("output_path")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    assert!(
        !out.is_empty(),
        "the dialog shows `output_path`, so the response must carry one: {json}"
    );
    // 🔑 On DISK, not just in the response: a handler that reported a path it
    // never wrote would satisfy the assertion above.
    assert!(
        std::path::Path::new(out).exists(),
        "the reported bundle must exist on disk: {out}"
    );

    // -- 3. PCK/ZIP mode writes an archive containing project.godot ----------
    let resp = http_post(
        port,
        "/api/export/run",
        &serde_json::json!({ "name": "WebOk", "debug": false, "mode": "pck" }).to_string(),
    );
    assert!(
        resp.contains("200 OK"),
        "the PCK/ZIP mode must succeed, got: {resp}"
    );
    let json: serde_json::Value = serde_json::from_str(body_of(&resp))
        .unwrap_or_else(|e| panic!("body is not JSON ({e}): {resp}"));
    let zip = json
        .get("output_path")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    assert!(
        zip.ends_with(".zip"),
        "the pack mode replaces the preset's extension with .zip, got: {zip}"
    );
    assert!(
        std::path::Path::new(&zip).exists(),
        "the archive must exist on disk: {zip}"
    );
    // 🔑 Read the archive back rather than trusting its size: a zip with a
    // valid header and no entries is still a readable file.
    let mut reader = gdresource::ZipReader::new();
    reader
        .open(&zip)
        .unwrap_or_else(|e| panic!("the archive must be a readable zip: {e:?}"));
    assert!(
        reader.file_exists("project.godot", true),
        "the archive must contain project.godot — a pack without the project \
         file is the \"succeeded and produced nothing\" outcome. Entries: {:?}",
        reader.get_files()
    );

    let _ = std::fs::remove_dir_all(&dir);
}
