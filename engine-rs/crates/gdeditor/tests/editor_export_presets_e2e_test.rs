//! pat-0q2kx.11 acceptance: the served Export dialog's preset CRUD reaches a
//! real `export_presets.cfg` on disk.
//!
//! The dialog in `editor_ui.rs` has called `/api/export/presets`,
//! `/api/export/presets/update` and `/api/export/presets/delete` since
//! pat-4cexp, but `editor_server.rs` routed none of them — every call fell
//! through to `_ => serve_404`, so the dialog rendered empty and every edit was
//! silently dropped.
//!
//! These tests drive the **real HTTP server**, not the handler functions, and
//! assert against the **file on disk**, so a regression to "route exists but
//! persists nothing" fails. The last test is the guard that keeps the two sides
//! from drifting again: every `/api/export/...` path the client fetches must
//! appear in the server's own capabilities list.

use gdeditor::editor_server::{EditorServerHandle, EditorState};
use gdeditor::editor_ui::EDITOR_HTML;
use gdscene::node::Node;
use gdscene::SceneTree;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;
use std::time::Duration;

// ---------------------------------------------------------------------------
// HTTP harness (mirrors editor_suite_c/editor_parity_test.rs)
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

fn http_request_str(port: u16, request: &str) -> String {
    let mut stream = connect_with_retry(port);
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
    stream.write_all(request.as_bytes()).unwrap();
    let mut resp = Vec::new();
    let _ = stream.read_to_end(&mut resp);
    String::from_utf8_lossy(&resp).to_string()
}

fn http_get(port: u16, path: &str) -> String {
    http_request_str(
        port,
        &format!("GET {path} HTTP/1.1\r\nHost: localhost\r\n\r\n"),
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

fn body_of(resp: &str) -> &str {
    resp.split("\r\n\r\n").nth(1).unwrap_or("")
}

fn json_of(resp: &str) -> serde_json::Value {
    serde_json::from_str(body_of(resp))
        .unwrap_or_else(|e| panic!("response body is not JSON ({e}): {resp}"))
}

/// A throwaway project directory containing the `project.godot` that
/// `/api/project/open` requires.
fn temp_project(tag: &str) -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "patina_0q2kx11_{}_{}_{}",
        tag,
        std::process::id(),
        n
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("project.godot"),
        "[application]\nconfig/name=\"ExportPresetsE2E\"\n",
    )
    .unwrap();
    dir
}

/// Starts a server rooted at a fresh temp project. Returns the handle (kept
/// alive by the caller), the port, and the project dir.
fn server_on_temp_project(tag: &str) -> (EditorServerHandle, u16, PathBuf) {
    let dir = temp_project(tag);
    let port = free_port();
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    tree.add_child(root, Node::new("Main", "Node2D")).unwrap();
    let handle = EditorServerHandle::start(port, EditorState::new(tree));
    thread::sleep(Duration::from_millis(100));

    // Re-root the editor at the temp project so export_presets.cfg is written
    // there and not into the repo.
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

fn cfg_path(dir: &Path) -> PathBuf {
    dir.join("export_presets.cfg")
}

// ---------------------------------------------------------------------------
// The acceptance test
// ---------------------------------------------------------------------------

/// Full CRUD round-trip against the running server, verified on disk at every
/// step: create → GET → `export_presets.cfg` → update → delete.
#[test]
fn editor_export_presets_e2e() {
    let (_handle, port, dir) = server_on_temp_project("crud");

    // A fresh project has no presets, and the route must answer (not 404).
    let listed = http_get(port, "/api/export/presets");
    assert!(
        listed.contains("200 OK"),
        "GET /api/export/presets must be routed, got: {listed}"
    );
    assert_eq!(
        json_of(&listed).as_array().map(|a| a.len()),
        Some(0),
        "a fresh project starts with no presets"
    );

    // -- create ------------------------------------------------------------
    let created = http_post(
        port,
        "/api/export/presets",
        r#"{"name":"Web","platform":"web","build_profile":"release","export_path":"","app_name":"","include_filters":["*"],"exclude_filters":[],"custom_properties":{}}"#,
    );
    assert!(created.contains("200 OK"), "create must succeed: {created}");
    assert_eq!(json_of(&created)["ok"], serde_json::json!(true));

    // The preset comes back from a fresh GET ...
    let after_create = json_of(&http_get(port, "/api/export/presets"));
    let arr = after_create.as_array().expect("array of presets");
    assert_eq!(arr.len(), 1, "one preset after create, got {after_create}");
    assert_eq!(arr[0]["name"], serde_json::json!("Web"));
    assert_eq!(arr[0]["platform"], serde_json::json!("web"));

    // ... and it is on disk in export_presets.cfg, in Godot's ConfigFile shape.
    let cfg = std::fs::read_to_string(cfg_path(&dir))
        .expect("export_presets.cfg must exist in the project dir after create");
    assert!(
        cfg.contains("[preset.0]"),
        "cfg must use Godot's [preset.N] sections, got:\n{cfg}"
    );
    assert!(
        cfg.contains(r#"name="Web""#),
        "cfg must record the preset name, got:\n{cfg}"
    );

    // -- update (rename, by index — this is why the client sends one) -------
    let updated = http_post(
        port,
        "/api/export/presets/update",
        r#"{"index":0,"name":"Web Release","platform":"web","build_profile":"debug","export_path":"build/web/index.html","app_name":"MyGame","include_filters":["*"],"exclude_filters":["*.tmp"],"custom_properties":{}}"#,
    );
    assert!(updated.contains("200 OK"), "update must succeed: {updated}");

    let after_update = json_of(&http_get(port, "/api/export/presets"));
    let p = &after_update.as_array().expect("array")[0];
    assert_eq!(
        p["name"],
        serde_json::json!("Web Release"),
        "rename applied"
    );
    assert_eq!(p["build_profile"], serde_json::json!("debug"));
    assert_eq!(
        p["export_path"],
        serde_json::json!("build/web/index.html"),
        "edited export_path survives the round-trip through the .cfg"
    );
    assert_eq!(p["app_name"], serde_json::json!("MyGame"));
    assert_eq!(p["exclude_filters"], serde_json::json!(["*.tmp"]));

    let cfg = std::fs::read_to_string(cfg_path(&dir)).unwrap();
    assert!(
        cfg.contains(r#"name="Web Release""#),
        "the rename is persisted, not just held in memory:\n{cfg}"
    );

    // -- delete ------------------------------------------------------------
    let deleted = http_post(
        port,
        "/api/export/presets/delete",
        r#"{"name":"Web Release"}"#,
    );
    assert!(deleted.contains("200 OK"), "delete must succeed: {deleted}");

    let after_delete = json_of(&http_get(port, "/api/export/presets"));
    assert_eq!(
        after_delete.as_array().map(|a| a.len()),
        Some(0),
        "preset is gone after delete"
    );
    let cfg = std::fs::read_to_string(cfg_path(&dir)).unwrap();
    assert!(
        !cfg.contains("Web Release"),
        "the delete is persisted to disk too:\n{cfg}"
    );
}

/// Two presets coexist and are addressed independently — deleting one must not
/// disturb the other, which a name/index mix-up would break.
#[test]
fn export_presets_delete_targets_only_the_named_preset() {
    let (_handle, port, dir) = server_on_temp_project("multi");

    for (name, platform) in [("Web", "web"), ("Desktop", "linux")] {
        let resp = http_post(
            port,
            "/api/export/presets",
            &serde_json::json!({
                "name": name, "platform": platform, "build_profile": "release",
                "include_filters": ["*"], "exclude_filters": [], "custom_properties": {}
            })
            .to_string(),
        );
        assert!(resp.contains("200 OK"), "create {name} failed: {resp}");
    }
    assert_eq!(
        json_of(&http_get(port, "/api/export/presets"))
            .as_array()
            .map(|a| a.len()),
        Some(2)
    );

    http_post(port, "/api/export/presets/delete", r#"{"name":"Web"}"#);

    let left = json_of(&http_get(port, "/api/export/presets"));
    let arr = left.as_array().expect("array");
    assert_eq!(arr.len(), 1, "exactly one preset removed");
    assert_eq!(arr[0]["name"], serde_json::json!("Desktop"));
    assert_eq!(
        arr[0]["platform"],
        serde_json::json!("linux"),
        "the surviving preset keeps its own platform"
    );

    // Both presets were written as separate [preset.N] sections; after the
    // delete the survivor is renumbered to index 0, as Godot does.
    let cfg = std::fs::read_to_string(cfg_path(&dir)).unwrap();
    assert!(cfg.contains("[preset.0]"), "survivor renumbered:\n{cfg}");
    assert!(!cfg.contains("[preset.1]"), "no orphan section:\n{cfg}");
}

/// A duplicate name is rejected rather than silently shadowing the original.
#[test]
fn export_presets_reject_duplicate_names() {
    let (_handle, port, _dir) = server_on_temp_project("dupe");
    let body = r#"{"name":"Web","platform":"web","include_filters":["*"],"exclude_filters":[],"custom_properties":{}}"#;

    assert!(http_post(port, "/api/export/presets", body).contains("200 OK"));
    let second = http_post(port, "/api/export/presets", body);
    assert!(
        !second.contains("200 OK"),
        "a duplicate preset name must be rejected, got: {second}"
    );
    assert_eq!(
        json_of(&http_get(port, "/api/export/presets"))
            .as_array()
            .map(|a| a.len()),
        Some(1),
        "the rejected create must not have been persisted"
    );
}

/// An unknown platform is a client error, not a panic or a half-written file.
#[test]
fn export_presets_reject_unknown_platform() {
    let (_handle, port, dir) = server_on_temp_project("badplat");
    let resp = http_post(
        port,
        "/api/export/presets",
        r#"{"name":"Nope","platform":"gamecube"}"#,
    );
    assert!(
        resp.contains("400"),
        "unknown platform must be a 400, got: {resp}"
    );
    assert!(
        !cfg_path(&dir).exists(),
        "a rejected create must not write export_presets.cfg"
    );
}

// ---------------------------------------------------------------------------
// Drift guard: the client and the route table must agree
// ---------------------------------------------------------------------------

/// Every `/api/export/...` path the served dialog fetches is routed and listed
/// in `/api/capabilities`.
///
/// This is the assertion that would have caught the original bug: the dialog
/// shipped calling four routes that did not exist, and nothing failed.
#[test]
fn editor_html_export_fetches_are_all_routed() {
    // Collect every distinct /api/export/... path literal in the served HTML.
    let mut wanted: Vec<String> = Vec::new();
    let mut rest = EDITOR_HTML;
    while let Some(i) = rest.find("/api/export") {
        rest = &rest[i..];
        let end = rest
            .find(|c: char| c == '\'' || c == '"' || c == '`' || c.is_whitespace())
            .unwrap_or(rest.len());
        let path = rest[..end].to_string();
        if !wanted.contains(&path) {
            wanted.push(path);
        }
        rest = &rest[end..];
    }
    assert!(
        !wanted.is_empty(),
        "the Export dialog should fetch at least one /api/export path"
    );

    let (_handle, port, _dir) = server_on_temp_project("caps");
    let caps = http_get(port, "/api/capabilities");
    let caps_body = body_of(&caps).to_string();

    for path in &wanted {
        assert!(
            caps_body.contains(path.as_str()),
            "EDITOR_HTML fetches {path} but it is not in /api/capabilities — \
             the route table and the client have drifted. Capabilities: {caps_body}"
        );
    }

    // The one GET among them must actually answer rather than 404. (The others
    // are POST-only, so probing them with GET would 404 by design — the
    // capabilities check above is what covers those.)
    let listed = http_get(port, "/api/export/presets");
    assert!(
        listed.contains("200 OK"),
        "GET /api/export/presets must answer, got: {listed}"
    );
}
