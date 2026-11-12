//! Regression test for pat-iixwj.
//!
//! `GET /api/filesystem` WITHOUT a `?path` query must respond promptly with a
//! JSON project tree instead of hanging forever. The bug: the bare-path
//! handler walks the editor's working directory via `scan_directory`, which
//! only skipped hidden dirs — so when rooted at a Cargo workspace it recursed
//! into the multi-gigabyte `target/` build tree and stat'd millions of
//! entries, so the request never completed. The served UI polls this endpoint
//! at 1 Hz, leaking a never-completing request per tick and saturating the
//! browser's per-host connection pool (the pat-vl1mk freeze).
//!
//! These are app-level tests: they boot the real `EditorServerHandle`, root it
//! at a project that contains a large synthetic `target/` build tree, and drive
//! it over HTTP. The harness (free_port / make_server / http_get / http_post /
//! extract_body) is copied from editor_parity_test.rs so this file is
//! self-contained.

use gdeditor::editor_server::{EditorServerHandle, EditorState};
use gdscene::node::Node;
use gdscene::SceneTree;
use gdvariant::Variant;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;
use std::time::{Duration, Instant};

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn make_server() -> (EditorServerHandle, u16) {
    let port = free_port();
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let mut main = Node::new("Main", "Node2D");
    main.set_property(
        "position",
        Variant::Vector2(gdcore::math::Vector2::new(10.0, 20.0)),
    );
    tree.add_child(root, main).unwrap();
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

fn http_request_str(port: u16, request: &str) -> String {
    let mut stream = connect_with_retry(port);
    // Generous read timeout so that if the handler hangs we still unblock the
    // test thread and observe a NON-200 / empty response rather than dead-
    // locking the whole test binary.
    stream.set_read_timeout(Some(Duration::from_secs(10))).ok();
    stream.write_all(request.as_bytes()).unwrap();
    let mut resp = Vec::new();
    let _ = stream.read_to_end(&mut resp);
    String::from_utf8_lossy(&resp).to_string()
}

fn http_get(port: u16, path: &str) -> String {
    http_request_str(
        port,
        &format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n"),
    )
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

fn extract_body(resp: &str) -> &str {
    resp.split("\r\n\r\n").nth(1).unwrap_or("")
}

/// Build a temporary directory that looks like a Godot project AND carries a
/// large synthetic Cargo `target/` build tree full of asset-extension files.
///
/// Pre-fix, `scan_directory` would recurse into `target/` and (a) include it in
/// the listing because it contains .gd/.png files and (b) walk the whole tree.
/// Post-fix, `target/` is skipped entirely.
fn make_bloated_project() -> tempfile::TempDir {
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path();

    // Minimal Godot project markers so POST /api/project/open accepts it.
    std::fs::write(root.join("project.godot"), "[application]\n").unwrap();
    std::fs::write(root.join("main.tscn"), "[gd_scene]\n").unwrap();
    std::fs::create_dir_all(root.join("scenes")).unwrap();
    std::fs::write(root.join("scenes/level1.tscn"), "[gd_scene]\n").unwrap();

    // Synthetic build dir. Fill it with many nested directories that each hold
    // asset-extension files so the (unfixed) walker both descends into and
    // surfaces it. This is the directory the fix must skip.
    let target = root.join("target");
    for a in 0..12 {
        for b in 0..12 {
            let dir = target.join(format!("debug{a}")).join(format!("deps{b}"));
            std::fs::create_dir_all(&dir).unwrap();
            // Asset-extension files so an UNFIXED scan would include `target`.
            std::fs::write(dir.join("artifact.gd"), "extends Node\n").unwrap();
            std::fs::write(dir.join("icon.png"), b"\x89PNG\r\n").unwrap();
            // Plus build junk to bulk up the stat workload.
            for c in 0..8 {
                std::fs::write(dir.join(format!("obj{c}.o")), b"junk").unwrap();
            }
        }
    }
    tmp
}

/// pat-iixwj — primary acceptance: a bare GET (NO query param) responds within
/// 5 s with HTTP 200 and a JSON tree, and the build directory is excluded.
#[test]
fn editor_filesystem_bare_get_responds() {
    let project = make_bloated_project();
    let project_path = project.path().display().to_string();

    let (handle, port) = make_server();

    // Root the editor at the bloated project so the bare-path listing walks a
    // directory that contains the heavy `target/` tree (mirrors the live
    // server, whose cwd is a Cargo workspace).
    let open = http_post(
        port,
        "/api/project/open",
        &format!(r#"{{"path":"{}"}}"#, project_path.replace('\\', "\\\\")),
    );
    assert!(
        open.contains("200 OK"),
        "POST /api/project/open should succeed, got: {open}"
    );

    // The bug: this bare GET (no `?path`) never returns. Assert it completes
    // quickly with a 200 and a well-formed body.
    let start = Instant::now();
    let resp = http_get(port, "/api/filesystem");
    let elapsed = start.elapsed();

    assert!(
        elapsed < Duration::from_secs(5),
        "bare GET /api/filesystem must respond within 5s, took {elapsed:?} \
         (regression pat-iixwj: handler walked target/ and hung)"
    );
    assert!(
        resp.contains("200 OK"),
        "bare GET /api/filesystem must return HTTP 200, got: {}",
        resp.chars().take(200).collect::<String>()
    );

    let body = extract_body(&resp);
    let v: serde_json::Value = serde_json::from_str(body)
        .unwrap_or_else(|e| panic!("response body must be JSON: {e}; body={body}"));
    assert!(
        v["root"].is_string(),
        "response must have a root path string"
    );
    assert!(v["files"].is_array(), "response must have a files array");

    // The build directory must be skipped entirely — directly verifies the fix
    // mechanism (and is the assertion that fails pre-fix, since `target/` here
    // contains .gd/.png files and would otherwise be surfaced).
    let names: Vec<&str> = v["files"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|e| e["name"].as_str())
        .collect();
    assert!(
        !names.contains(&"target"),
        "scan_directory must skip the Cargo target/ build tree, got dirs: {names:?}"
    );
    // Real project content is still listed.
    assert!(
        names.contains(&"main.tscn") || names.contains(&"scenes"),
        "real project files must still be listed, got: {names:?}"
    );

    handle.stop();
}

/// pat-iixwj — handler-leak regression: 50 sequential bare GETs all complete
/// (the UI polls this endpoint at ~1 Hz; a leaked/never-completing handler per
/// tick is what saturated the connection pool).
#[test]
fn editor_filesystem_bare_get_no_handler_leak() {
    let project = make_bloated_project();
    let project_path = project.path().display().to_string();

    let (handle, port) = make_server();
    let open = http_post(
        port,
        "/api/project/open",
        &format!(r#"{{"path":"{}"}}"#, project_path.replace('\\', "\\\\")),
    );
    assert!(
        open.contains("200 OK"),
        "project open should succeed: {open}"
    );

    let start = Instant::now();
    for i in 0..50 {
        let resp = http_get(port, "/api/filesystem");
        assert!(
            resp.contains("200 OK"),
            "bare GET #{i} must return HTTP 200 (no handler leak/hang), got: {}",
            resp.chars().take(160).collect::<String>()
        );
    }
    // 50 bounded scans must be quick; a single unbounded target/ walk alone
    // already blows past this.
    assert!(
        start.elapsed() < Duration::from_secs(20),
        "50 sequential bare GETs must complete promptly, took {:?}",
        start.elapsed()
    );

    handle.stop();
}
