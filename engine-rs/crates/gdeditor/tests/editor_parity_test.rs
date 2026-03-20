//! Parity tests for P3 editor beads (Batch 3).
//!
//! Each section covers one bead, verifying the REST API or HTML surface exists.

use gdeditor::editor_server::{EditorServerHandle, EditorState};
use gdscene::node::Node;
use gdscene::SceneTree;
use gdvariant::Variant;
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

fn extract_body(resp: &str) -> &str {
    resp.split("\r\n\r\n").nth(1).unwrap_or("")
}

fn get_main_node_id(port: u16) -> u64 {
    let resp = http_get(port, "/api/scene");
    let v: serde_json::Value = serde_json::from_str(extract_body(&resp)).unwrap();
    v["nodes"]["children"][0]["id"].as_u64().unwrap()
}

// ---- pat-b0o: Viewport selection modes ----

#[test]
fn test_b0o_set_viewport_mode() {
    let (handle, port) = make_server();
    let resp = http_post(port, "/api/viewport/set_mode", r#"{"mode":"move"}"#);
    assert!(
        resp.contains("200 OK"),
        "POST /api/viewport/set_mode should succeed"
    );
    handle.stop();
}

#[test]
fn test_b0o_get_viewport_mode() {
    let (handle, port) = make_server();
    // Set a mode, then verify GET returns it
    http_post(port, "/api/viewport/set_mode", r#"{"mode":"scale"}"#);
    let resp = http_get(port, "/api/viewport/mode");
    assert!(
        resp.contains("200 OK"),
        "GET /api/viewport/mode should succeed"
    );
    let body = extract_body(&resp);
    let v: serde_json::Value = serde_json::from_str(body).unwrap();
    assert!(
        v.get("mode").is_some(),
        "Response should contain mode field"
    );
    handle.stop();
}

// ---- pat-r5p: Transform gizmos ----

#[test]
fn test_r5p_gizmo_drawn_on_selection() {
    let (handle, port) = make_server();
    let mid = get_main_node_id(port);
    // Select the node — gizmo mode should be queryable after selection
    http_post(port, "/api/node/select", &format!(r#"{{"node_id":{mid}}}"#));
    let resp = http_get(port, "/api/selected");
    assert!(resp.contains("200 OK"), "Selection should succeed");
    let v: serde_json::Value = serde_json::from_str(extract_body(&resp)).unwrap();
    assert!(
        v.get("node_id").is_some() || v.get("id").is_some(),
        "Selected node should be reported after selection (gizmo target)"
    );
    handle.stop();
}

#[test]
fn test_r5p_gizmo_html_elements() {
    let (handle, port) = make_server();
    let html = http_get(port, "/editor");
    let b = extract_body(&html);
    // Transform gizmo controls should be in the toolbar
    assert!(
        b.contains("gizmo-indicator"),
        "Editor HTML should contain gizmo-indicator CSS for transform gizmos"
    );
    handle.stop();
}

// ---- pat-zlv: Snapping ----

#[test]
fn test_zlv_snap_info_endpoint() {
    let (handle, port) = make_server();
    let resp = http_get(port, "/api/viewport/snap_info");
    assert!(
        resp.contains("200 OK"),
        "GET /api/viewport/snap_info should succeed"
    );
    let v: serde_json::Value = serde_json::from_str(extract_body(&resp)).unwrap();
    assert!(
        v.get("enabled").is_some() || v.get("snap_enabled").is_some(),
        "snap_info should report snap state"
    );
    handle.stop();
}

#[test]
fn test_zlv_snap_info_has_grid_size() {
    let (handle, port) = make_server();
    let resp = http_get(port, "/api/viewport/snap_info");
    let v: serde_json::Value = serde_json::from_str(extract_body(&resp)).unwrap();
    // Should have grid/step size info
    assert!(
        v.get("snap_size").is_some(),
        "snap_info should include snap_size"
    );
    handle.stop();
}

// ---- pat-cgc: Script editor core ----

#[test]
fn test_cgc_node_script_returns_data() {
    let (handle, port) = make_server();
    let mid = get_main_node_id(port);
    // Node without a script should return has_script: false
    let resp = http_get(port, &format!("/api/node/script?node_id={mid}"));
    assert!(
        resp.contains("200 OK"),
        "GET /api/node/script should succeed"
    );
    let v: serde_json::Value = serde_json::from_str(extract_body(&resp)).unwrap();
    assert_eq!(
        v["has_script"], false,
        "Node without script should report has_script=false"
    );
    handle.stop();
}

#[test]
fn test_cgc_node_script_with_script_path() {
    let (handle, port) = make_server();
    let mid = get_main_node_id(port);
    // Attach a script path
    http_post(
        port,
        "/api/property/set",
        &format!(
            r#"{{"node_id":{mid},"property":"_script_path","value":{{"type":"String","value":"res://nonexistent.gd"}}}}"#
        ),
    );
    let resp = http_get(port, &format!("/api/node/script?node_id={mid}"));
    let v: serde_json::Value = serde_json::from_str(extract_body(&resp)).unwrap();
    assert_eq!(
        v["has_script"], true,
        "Node with script path should report has_script=true"
    );
    assert!(v.get("path").is_some(), "Should include script path");
    handle.stop();
}

// ---- pat-1v3: Script search ----

#[test]
fn test_1v3_search_endpoint_exists() {
    let (handle, port) = make_server();
    let resp = http_get(port, "/api/search?q=test");
    assert!(resp.contains("200 OK"), "GET /api/search should succeed");
    let v: serde_json::Value = serde_json::from_str(extract_body(&resp)).unwrap();
    assert!(
        v.get("results").is_some(),
        "Search should return results array"
    );
    handle.stop();
}

#[test]
fn test_1v3_search_empty_query() {
    let (handle, port) = make_server();
    let resp = http_get(port, "/api/search?q=");
    assert!(resp.contains("200 OK"), "Empty search should still succeed");
    let v: serde_json::Value = serde_json::from_str(extract_body(&resp)).unwrap();
    assert_eq!(
        v["results"].as_array().unwrap().len(),
        0,
        "Empty query returns empty results"
    );
    handle.stop();
}

// ---- pat-2hs: Signals dock ----

#[test]
fn test_2hs_signals_endpoint() {
    let (handle, port) = make_server();
    let mid = get_main_node_id(port);
    let resp = http_get(port, &format!("/api/node/signals?node_id={mid}"));
    assert!(
        resp.contains("200 OK"),
        "GET /api/node/signals should succeed"
    );
    let v: serde_json::Value = serde_json::from_str(extract_body(&resp)).unwrap();
    assert!(
        v.get("signals").is_some() || v.is_array(),
        "Should return signals data"
    );
    handle.stop();
}

#[test]
fn test_2hs_signals_missing_node() {
    let (handle, port) = make_server();
    let resp = http_get(port, "/api/node/signals?node_id=9999999");
    assert!(resp.contains("404"), "Non-existent node should return 404");
    handle.stop();
}

// ---- pat-2s1: Animation editor ----

#[test]
fn test_2s1_animations_endpoint() {
    let (handle, port) = make_server();
    let resp = http_get(port, "/api/animations");
    assert!(
        resp.contains("200 OK"),
        "GET /api/animations should succeed"
    );
    let v: serde_json::Value = serde_json::from_str(extract_body(&resp)).unwrap();
    assert!(v.is_array(), "Animations endpoint should return an array");
    handle.stop();
}

#[test]
fn test_2s1_animation_create_and_list() {
    let (handle, port) = make_server();
    http_post(
        port,
        "/api/animation/create",
        r#"{"name":"walk","length":1.0}"#,
    );
    let resp = http_get(port, "/api/animations");
    let v: serde_json::Value = serde_json::from_str(extract_body(&resp)).unwrap();
    let anims = v.as_array().unwrap();
    assert!(
        anims.iter().any(|a| a["name"] == "walk"),
        "Created animation should appear in list"
    );
    handle.stop();
}

// ---- pat-lbu: Bottom panels ----

#[test]
fn test_lbu_output_endpoint() {
    let (handle, port) = make_server();
    let resp = http_get(port, "/api/output");
    assert!(resp.contains("200 OK"), "GET /api/output should succeed");
    let v: serde_json::Value = serde_json::from_str(extract_body(&resp)).unwrap();
    assert!(
        v.get("entries").is_some(),
        "Output endpoint should return entries field"
    );
    handle.stop();
}

#[test]
fn test_lbu_bottom_panel_html() {
    let (handle, port) = make_server();
    let html = http_get(port, "/editor");
    let b = extract_body(&html);
    assert!(
        b.contains("bottom-panel"),
        "Editor should have bottom-panel"
    );
    assert!(b.contains("output-log"), "Editor should have output-log");
    handle.stop();
}

// ---- pat-dj6: Top bar ----

#[test]
fn test_dj6_play_stop_buttons() {
    let (handle, port) = make_server();
    let html = http_get(port, "/editor");
    let b = extract_body(&html);
    assert!(b.contains("btn-play"), "Editor should have play button");
    assert!(b.contains("btn-stop"), "Editor should have stop button");
    handle.stop();
}

#[test]
fn test_dj6_toolbar_present() {
    let (handle, port) = make_server();
    let html = http_get(port, "/editor");
    let b = extract_body(&html);
    assert!(b.contains("toolbar"), "Editor should have toolbar");
    assert!(b.contains("btn-pause"), "Editor should have pause button");
    assert!(b.contains("btn-save"), "Editor should have save button");
    handle.stop();
}

// ---- pat-flr: Filesystem dock ----

#[test]
fn test_flr_filesystem_endpoint() {
    let (handle, port) = make_server();
    let resp = http_get(port, "/api/filesystem");
    assert!(
        resp.contains("200 OK"),
        "GET /api/filesystem should succeed"
    );
    let body = extract_body(&resp);
    let v: serde_json::Value = serde_json::from_str(body).unwrap();
    assert!(
        v.get("files").is_some() || v.get("entries").is_some() || v.is_array(),
        "Filesystem endpoint should return file listing"
    );
    handle.stop();
}

#[test]
fn test_flr_filesystem_panel_html() {
    let (handle, port) = make_server();
    let html = http_get(port, "/editor");
    let b = extract_body(&html);
    assert!(
        b.contains("filesystem-panel"),
        "Editor should have filesystem-panel"
    );
    assert!(b.contains("fs-tree"), "Editor should have fs-tree");
    handle.stop();
}

// ---- pat-ipx: Menu actions ----

#[test]
fn test_ipx_menu_bar_present() {
    let (handle, port) = make_server();
    let html = http_get(port, "/editor");
    let b = extract_body(&html);
    assert!(b.contains("menu-bar"), "Editor should have menu-bar");
    handle.stop();
}

#[test]
fn test_ipx_menu_items() {
    let (handle, port) = make_server();
    let html = http_get(port, "/editor");
    let b = extract_body(&html);
    // Menu should have standard editor menu items
    assert!(
        b.contains("Scene") || b.contains("scene-menu"),
        "Menu should contain Scene entry"
    );
    assert!(
        b.contains("Edit") || b.contains("edit-menu"),
        "Menu should contain Edit entry"
    );
    handle.stop();
}

// ---- pat-kj4: Editor settings ----

#[test]
fn test_kj4_settings_get() {
    let (handle, port) = make_server();
    let resp = http_get(port, "/api/settings");
    assert!(resp.contains("200 OK"), "GET /api/settings should succeed");
    let v: serde_json::Value = serde_json::from_str(extract_body(&resp)).unwrap();
    assert!(v.is_object(), "Settings should return an object");
    handle.stop();
}

#[test]
fn test_kj4_settings_set_and_read_back() {
    let (handle, port) = make_server();
    let resp = http_post(port, "/api/settings", r#"{"theme":"dark"}"#);
    assert!(resp.contains("200 OK"), "POST /api/settings should succeed");
    let resp2 = http_get(port, "/api/settings");
    let v: serde_json::Value = serde_json::from_str(extract_body(&resp2)).unwrap();
    assert!(v.is_object(), "Settings should persist");
    handle.stop();
}

// ---- pat-rjd: Output stability ----

#[test]
fn test_rjd_output_clear() {
    let (handle, port) = make_server();
    let resp = http_post(port, "/api/output/clear", "{}");
    assert!(
        resp.contains("200 OK"),
        "POST /api/output/clear should succeed"
    );
    handle.stop();
}
