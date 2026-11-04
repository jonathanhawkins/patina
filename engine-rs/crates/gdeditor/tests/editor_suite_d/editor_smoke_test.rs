//! App-boot smoke test: every advertised editor surface is backed by a
//! non-stub endpoint (pat-s46ka).
//!
//! Boots the assembled editor server, GETs `/editor`, and for each advertised
//! surface asserts BOTH that its DOM marker is served AND that its backing
//! endpoint returns real structured data — not a 404 and not an empty stub.
//! This catches the "button in the HTML, backend is just a string flag" class
//! of fake parity. Its pass is the `editor-app-smoke-all-modes` criterion.

use gdeditor::editor_server::{EditorServerHandle, EditorState};
use gdscene::node::Node;
use gdscene::SceneTree;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;
use std::time::Duration;

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
}

/// A loopback stand-in for the asset catalog host (Godot AssetLib
/// `{"result":[...]}` envelope).
///
/// `/api/assetlib/list` no longer serves a hardcoded catalog — its twelve
/// fabricated, uninstallable entries were removed for an honest
/// `configured:false` empty state (pat-0q2kx.240 / pat-s3rrv.227, see
/// `api_get_assetlib_list`). A catalog is opt-in via the project's
/// `[editor] assetlib/index_url`, so this smoke test configures one: the
/// "asset browser is not an empty stub" check below still has teeth, it just
/// has to come from a real fetch now.
fn serve_catalog() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let body = br#"{"result":[{"title":"Smoke Asset","author":"patina","category":"Scripts","cost":"MIT","version_string":"1.0","download_url":"http://example.invalid/a.zip"}]}"#;
    thread::spawn(move || {
        for _ in 0..4 {
            let Ok((mut sock, _)) = listener.accept() else {
                return;
            };
            let mut buf = [0u8; 2048];
            let _ = sock.read(&mut buf);
            let head = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = sock.write_all(head.as_bytes());
            let _ = sock.write_all(body);
            let _ = sock.flush();
        }
    });
    format!("http://127.0.0.1:{port}/catalog.json")
}

fn make_server() -> (EditorServerHandle, u16, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!("patina_smoke_all_modes_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("project.godot"),
        format!(
            "config_version=5\n\n[application]\n\nconfig/name=\"Smoke\"\n\n[editor]\n\nassetlib/index_url=\"{}\"\n",
            serve_catalog()
        ),
    )
    .unwrap();
    let port = free_port();
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let world = tree.add_child(root, Node::new("World", "Node2D")).unwrap();
    tree.add_child(world, Node::new("Hero", "Sprite2D")).unwrap();
    let mut state = EditorState::new(tree);
    state.project_root = Some(dir.to_string_lossy().to_string());
    let handle = EditorServerHandle::start(port, state);
    thread::sleep(Duration::from_millis(100));
    (handle, port, dir)
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

fn http_get(port: u16, path: &str) -> String {
    let mut stream = connect_with_retry(port);
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
    stream
        .write_all(format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n").as_bytes())
        .unwrap();
    let mut resp = Vec::new();
    let _ = stream.read_to_end(&mut resp);
    String::from_utf8_lossy(&resp).to_string()
}

fn json_of(resp: &str) -> serde_json::Value {
    assert!(resp.contains("200 OK"), "endpoint did not return 200:\n{}", resp.lines().next().unwrap_or(""));
    serde_json::from_str(resp.split("\r\n\r\n").nth(1).unwrap_or("")).unwrap_or(serde_json::Value::Null)
}

/// Resolve a "a.b.c" path into the JSON value.
fn dig<'a>(v: &'a serde_json::Value, path: &str) -> &'a serde_json::Value {
    let mut cur = v;
    for seg in path.split('.') {
        cur = &cur[seg];
    }
    cur
}

#[test]
fn editor_app_smoke_all_modes() {
    let (handle, port, dir) = make_server();

    let html = http_get(port, "/editor");
    assert!(html.contains("200 OK"), "GET /editor must serve the app");
    let doc = html.split("\r\n\r\n").nth(1).unwrap_or("");

    // (surface, DOM marker served in /editor, backing endpoint, json key that
    //  must be present/non-null — proving the endpoint is real, not a stub).
    let surfaces: &[(&str, &str, &str, &str)] = &[
        ("play controls",  r#"id="btn-play""#,            "/api/runtime/status",                   "running"),
        ("3D viewport",    r#"id="vp3d-toolbar""#,        "/api/viewport/3d/state",                "yaw"),
        ("script view",    r#"id="script-editor-panel""#, "/api/scene",                            "nodes"),
        ("signals dock",   r#"id="node-dock""#,           "/api/scene",                            "nodes"),
        ("asset browser",  r#"id="assetlib-grid""#,       "/api/assetlib/list",                    "assets"),
        ("profiler",       r#"id="profiler-panel""#,      "/api/profiler",                         "capturing"),
        ("monitors",       r#"id="monitor-tree""#,        "/api/monitors",                         "monitors.object/nodes"),
        ("debugger",       r#"id="debug-errors-list""#,   "/api/debug/info",                       "video_ram"),
        ("animation",      r#"id="animation-panel""#,     "/api/animations",                       ""),
        ("game toolbar",   r#"id="game-toolbar""#,        "/api/runtime/status",                   "time_scale"),
        ("input map",      r#"id="pset-input-map""#,      "/api/input_map",                        "actions"),
    ];

    for (label, marker, endpoint, key) in surfaces {
        // 1. The surface's DOM is actually served.
        assert!(doc.contains(marker), "[{label}] DOM marker {marker} missing from /editor");
        // 2. Its backing endpoint returns real data (200 + structured body).
        let v = json_of(&http_get(port, endpoint));
        assert!(!v.is_null(), "[{label}] {endpoint} returned no JSON body (stub/404)");
        if !key.is_empty() {
            let field = dig(&v, key);
            assert!(
                !field.is_null(),
                "[{label}] {endpoint} is a stub — missing `{key}` in the response"
            );
            // Arrays/objects backing a surface must not be empty stubs where we
            // seeded real content (asset catalog, monitors, input map).
            if let Some(arr) = field.as_array() {
                if *label == "asset browser" {
                    assert!(!arr.is_empty(), "[{label}] {endpoint} `{key}` is an empty array (stub)");
                }
            }
            if *label == "input map" {
                assert!(field.as_object().map(|o| !o.is_empty()).unwrap_or(false), "[{label}] input map has no actions (stub)");
            }
        } else {
            // No key: just require a JSON array/object came back.
            assert!(v.is_array() || v.is_object(), "[{label}] {endpoint} returned a scalar stub");
        }
    }

    handle.stop();
    let _ = std::fs::remove_dir_all(&dir);
}
