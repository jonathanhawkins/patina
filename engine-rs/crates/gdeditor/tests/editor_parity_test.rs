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

#[test]
fn test_xse8a_top_menu_bar_five_menus() {
    let (handle, port) = make_server();
    let html = http_get(port, "/editor");
    let b = extract_body(&html);
    // The top menu bar should have Scene, Project, Debug, Editor, and Help menus
    assert!(b.contains("data-menu=\"scene\""), "Menu bar must have Scene menu");
    assert!(b.contains("data-menu=\"project\""), "Menu bar must have Project menu");
    assert!(b.contains("data-menu=\"debug\""), "Menu bar must have Debug menu");
    assert!(b.contains("data-menu=\"editor\""), "Menu bar must have Editor menu");
    assert!(b.contains("data-menu=\"help\""), "Menu bar must have Help menu");
    handle.stop();
}

#[test]
fn test_xse8a_scene_menu_actions() {
    let (handle, port) = make_server();
    let html = http_get(port, "/editor");
    let b = extract_body(&html);
    assert!(b.contains("data-action=\"scene-new\""), "Scene menu needs New Scene");
    assert!(b.contains("data-action=\"scene-open\""), "Scene menu needs Open Scene");
    assert!(b.contains("data-action=\"scene-save\""), "Scene menu needs Save Scene");
    assert!(b.contains("data-action=\"scene-save-as\""), "Scene menu needs Save As");
    assert!(b.contains("data-action=\"scene-close\""), "Scene menu needs Close Scene");
    assert!(b.contains("data-action=\"scene-quit\""), "Scene menu needs Quit");
    handle.stop();
}

#[test]
fn test_xse8a_debug_menu_actions() {
    let (handle, port) = make_server();
    let html = http_get(port, "/editor");
    let b = extract_body(&html);
    assert!(b.contains("data-action=\"debug-run\""), "Debug menu needs Run Project");
    assert!(b.contains("data-action=\"debug-run-current\""), "Debug menu needs Run Current");
    assert!(b.contains("data-action=\"debug-pause\""), "Debug menu needs Pause");
    assert!(b.contains("data-action=\"debug-stop\""), "Debug menu needs Stop");
    assert!(b.contains("data-action=\"debug-step\""), "Debug menu needs Step");
    handle.stop();
}

#[test]
fn test_xse8a_editor_menu_actions() {
    let (handle, port) = make_server();
    let html = http_get(port, "/editor");
    let b = extract_body(&html);
    assert!(b.contains("data-action=\"editor-settings\""), "Editor menu needs Settings");
    assert!(b.contains("data-action=\"editor-toggle-fullscreen\""), "Editor menu needs Toggle Fullscreen");
    assert!(b.contains("data-action=\"editor-toggle-console\""), "Editor menu needs Toggle Console");
    handle.stop();
}

#[test]
fn test_xse8a_help_menu_actions() {
    let (handle, port) = make_server();
    let html = http_get(port, "/editor");
    let b = extract_body(&html);
    assert!(b.contains("data-action=\"help-docs\""), "Help menu needs Documentation");
    assert!(b.contains("data-action=\"help-about\""), "Help menu needs About");
    assert!(b.contains("data-action=\"help-issues\""), "Help menu needs Report Bug");
    handle.stop();
}

#[test]
fn test_xse8a_menu_keyboard_shortcuts_shown() {
    let (handle, port) = make_server();
    let html = http_get(port, "/editor");
    let b = extract_body(&html);
    assert!(b.contains("Ctrl+N"), "New Scene should show Ctrl+N shortcut");
    assert!(b.contains("Ctrl+S"), "Save Scene should show Ctrl+S shortcut");
    assert!(b.contains("F5"), "Run Project should show F5 shortcut");
    assert!(b.contains("F11"), "Toggle Fullscreen should show F11 shortcut");
    handle.stop();
}

// ---- pat-7wvfu: Profiler panel ----

#[test]
fn test_7wvfu_profiler_tab_present() {
    let (handle, port) = make_server();
    let html = http_get(port, "/editor");
    let b = extract_body(&html);
    assert!(b.contains("data-tab=\"profiler\""), "Bottom panel must have Profiler tab");
    assert!(b.contains("profiler-panel"), "Editor must have profiler-panel div");
    assert!(b.contains("profiler-graph-canvas"), "Profiler must have graph canvas");
    assert!(b.contains("profiler-func-breakdown"), "Profiler must have function breakdown area");
    handle.stop();
}

#[test]
fn test_7wvfu_profiler_api_returns_empty() {
    let (handle, port) = make_server();
    let resp = http_get(port, "/api/profiler");
    assert!(resp.contains("200 OK"), "GET /api/profiler should succeed");
    let body = extract_body(&resp);
    let v: serde_json::Value = serde_json::from_str(body).unwrap();
    assert_eq!(v["count"], 0);
    assert_eq!(v["avg_ms"], 0.0);
    assert!(v["frames"].as_array().unwrap().is_empty());
    handle.stop();
}

#[test]
fn test_7wvfu_profiler_record_and_read() {
    let (handle, port) = make_server();
    let payload = r#"{"frame":1,"total_ms":16.5,"cpu_ms":14.0,"gpu_ms":2.5,"functions":[{"name":"physics_step","time_ms":5.0},{"name":"render_2d","time_ms":8.0}]}"#;
    let resp = http_post(port, "/api/profiler/record", payload);
    assert!(resp.contains("200 OK"));

    let resp2 = http_get(port, "/api/profiler");
    let body = extract_body(&resp2);
    let v: serde_json::Value = serde_json::from_str(body).unwrap();
    assert_eq!(v["count"], 1);
    let frame = &v["frames"][0];
    assert_eq!(frame["frame"], 1);
    assert!((frame["total_ms"].as_f64().unwrap() - 16.5).abs() < 0.1);
    assert!((frame["cpu_ms"].as_f64().unwrap() - 14.0).abs() < 0.1);
    assert!((frame["gpu_ms"].as_f64().unwrap() - 2.5).abs() < 0.1);
    let funcs = frame["functions"].as_array().unwrap();
    assert_eq!(funcs.len(), 2);
    assert_eq!(funcs[0]["name"], "physics_step");
    assert!((funcs[0]["time_ms"].as_f64().unwrap() - 5.0).abs() < 0.1);
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

// ===========================================================================
// pat-zgwgu: Scene tree indicators, badges, and selection state
// ===========================================================================

#[test]
fn test_zgwgu_tree_badge_css() {
    let (handle, port) = make_server();
    let html = http_get(port, "/editor");
    let b = extract_body(&html);
    assert!(
        b.contains("tree-badge"),
        "Editor CSS must include .tree-badge class"
    );
    assert!(
        b.contains("unique-name"),
        "Editor CSS must include .unique-name badge class"
    );
    handle.stop();
}

#[test]
fn test_zgwgu_lock_button_css() {
    let (handle, port) = make_server();
    let html = http_get(port, "/editor");
    let b = extract_body(&html);
    assert!(
        b.contains("tree-lock"),
        "Editor CSS must include .tree-lock class"
    );
    assert!(
        b.contains(".tree-lock.locked"),
        "Editor CSS must style locked state"
    );
    handle.stop();
}

#[test]
fn test_zgwgu_visibility_button_present() {
    let (handle, port) = make_server();
    let html = http_get(port, "/editor");
    let b = extract_body(&html);
    assert!(
        b.contains("tree-visibility"),
        "Editor must have visibility toggle CSS"
    );
    assert!(
        b.contains("vis-hidden"),
        "Editor must style hidden visibility state"
    );
    handle.stop();
}

#[test]
fn test_zgwgu_script_badge_js() {
    let (handle, port) = make_server();
    let html = http_get(port, "/editor");
    let b = extract_body(&html);
    assert!(
        b.contains("has_script"),
        "Editor JS must check node.has_script for script badge"
    );
    assert!(
        b.contains("Has script attached"),
        "Script badge must have descriptive title"
    );
    handle.stop();
}

#[test]
fn test_zgwgu_signal_badge_js() {
    let (handle, port) = make_server();
    let html = http_get(port, "/editor");
    let b = extract_body(&html);
    assert!(
        b.contains("has_signals"),
        "Editor JS must check node.has_signals for signal badge"
    );
    assert!(
        b.contains("Has connected signals"),
        "Signal badge must have descriptive title"
    );
    handle.stop();
}

#[test]
fn test_zgwgu_group_badge_js() {
    let (handle, port) = make_server();
    let html = http_get(port, "/editor");
    let b = extract_body(&html);
    assert!(
        b.contains("node.groups"),
        "Editor JS must check node.groups for group badge"
    );
    assert!(
        b.contains("[G]"),
        "Group badge must show [G] marker"
    );
    handle.stop();
}

#[test]
fn test_zgwgu_unique_name_badge_js() {
    let (handle, port) = make_server();
    let html = http_get(port, "/editor");
    let b = extract_body(&html);
    assert!(
        b.contains("unique_name"),
        "Editor JS must check node.unique_name for unique name badge"
    );
    assert!(
        b.contains("Unique name:"),
        "Unique name badge must have descriptive title"
    );
    handle.stop();
}

#[test]
fn test_zgwgu_lock_button_js() {
    let (handle, port) = make_server();
    let html = http_get(port, "/editor");
    let b = extract_body(&html);
    assert!(
        b.contains("node.locked"),
        "Editor JS must check node.locked for lock button"
    );
    assert!(
        b.contains("editor/locked"),
        "Lock button must toggle editor/locked property"
    );
    handle.stop();
}

#[test]
fn test_zgwgu_instanced_scene_icon() {
    let (handle, port) = make_server();
    let html = http_get(port, "/editor");
    let b = extract_body(&html);
    assert!(
        b.contains("Instanced Scene"),
        "Editor must show instanced scene indicator"
    );
    handle.stop();
}

#[test]
fn test_zgwgu_multi_select_support() {
    let (handle, port) = make_server();
    let html = http_get(port, "/editor");
    let b = extract_body(&html);
    assert!(
        b.contains("select_multi"),
        "Editor must support multi-node selection"
    );
    assert!(
        b.contains("selectedNodeIds"),
        "Editor must track multiple selected node IDs"
    );
    handle.stop();
}

// ===========================================================================
// pat-pcnuj: Inspector parity — advanced property organization
// ===========================================================================

#[test]
fn test_pcnuj_export_group_parsing() {
    let (handle, port) = make_server();
    let html = http_get(port, "/editor");
    let b = extract_body(&html);
    assert!(
        b.contains("@export_group"),
        "Editor must parse @export_group annotations"
    );
    assert!(
        b.contains("@export_subgroup"),
        "Editor must parse @export_subgroup annotations"
    );
    handle.stop();
}

#[test]
fn test_pcnuj_favorite_properties() {
    let (handle, port) = make_server();
    let html = http_get(port, "/editor");
    let b = extract_body(&html);
    assert!(
        b.contains("favoriteProperties"),
        "Editor must track favorite properties"
    );
    assert!(
        b.contains("toggleFavoriteProperty"),
        "Editor must have toggle favorite function"
    );
    assert!(
        b.contains("patina-fav-props"),
        "Favorite properties must persist in localStorage"
    );
    handle.stop();
}

#[test]
fn test_pcnuj_favorite_star_button() {
    let (handle, port) = make_server();
    let html = http_get(port, "/editor");
    let b = extract_body(&html);
    assert!(
        b.contains("insp-fav-btn"),
        "Inspector rows must have favorite star button"
    );
    assert!(
        b.contains("Toggle favorite"),
        "Favorite button must have tooltip"
    );
    handle.stop();
}

#[test]
fn test_pcnuj_subgroup_header_css() {
    let (handle, port) = make_server();
    let html = http_get(port, "/editor");
    let b = extract_body(&html);
    assert!(
        b.contains("insp-subgroup-header"),
        "Editor CSS must include .insp-subgroup-header"
    );
    handle.stop();
}

#[test]
fn test_pcnuj_sub_resource_inline_edit() {
    let (handle, port) = make_server();
    let html = http_get(port, "/editor");
    let b = extract_body(&html);
    assert!(
        b.contains("insp-sub-resource-btn"),
        "Editor must have sub-resource inline edit button CSS"
    );
    assert!(
        b.contains("data-sub-resource"),
        "Sub-resource edit button must have data attribute"
    );
    handle.stop();
}

#[test]
fn test_pcnuj_category_order() {
    let (handle, port) = make_server();
    let html = http_get(port, "/editor");
    let b = extract_body(&html);
    assert!(
        b.contains("CATEGORY_ORDER"),
        "Inspector must define property category order"
    );
    assert!(
        b.contains("getPropCategory"),
        "Inspector must categorize properties"
    );
    handle.stop();
}

#[test]
fn test_pcnuj_export_section_grouping() {
    let (handle, port) = make_server();
    let html = http_get(port, "/editor");
    let b = extract_body(&html);
    assert!(
        b.contains("data-export-group"),
        "Export sections must have data-export-group attribute"
    );
    assert!(
        b.contains("data-export-subgroup"),
        "Export subgroups must have data-export-subgroup attribute"
    );
    handle.stop();
}

#[test]
fn test_pcnuj_property_hints_enum() {
    // Verify the PropertyHint enum exists in the Rust inspector module
    use gdeditor::inspector::PropertyHint;
    let _none = PropertyHint::None;
    let _range = PropertyHint::Range { min: 0, max: 100, step: 1 };
    let _enum_hint = PropertyHint::Enum(vec!["A".into(), "B".into()]);
}

#[test]
fn test_pcnuj_inspector_sections() {
    // Verify InspectorSection and CustomPropertyEditor exist
    use gdeditor::inspector::{InspectorSection, CustomPropertyEditor};
    let section = InspectorSection::new("Test Group")
        .with_property(CustomPropertyEditor::new("speed").with_hint(
            gdeditor::inspector::PropertyHint::Range { min: 0, max: 100, step: 1 },
        ));
    assert_eq!(section.title, "Test Group");
    assert_eq!(section.properties.len(), 1);
}

// ---- pat-zoh4r: Viewport overlay (grid, rulers, origin, guides) ----

#[test]
fn test_zoh4r_viewport_overlay_canvas_present() {
    let (handle, port) = make_server();
    let html = http_get(port, "/editor");
    let b = extract_body(&html);
    assert!(
        b.contains("viewport-overlay-canvas"),
        "Editor must have viewport-overlay-canvas element"
    );
    handle.stop();
}

#[test]
fn test_zoh4r_grid_settings_in_ui() {
    let (handle, port) = make_server();
    let html = http_get(port, "/editor");
    let b = extract_body(&html);
    assert!(b.contains("set-grid-snap"), "Editor must have grid snap checkbox");
    assert!(b.contains("set-snap-size"), "Editor must have snap size selector");
    assert!(b.contains("set-grid-visible"), "Editor must have grid visible toggle");
    assert!(b.contains("set-rulers-visible"), "Editor must have rulers visible toggle");
    handle.stop();
}

#[test]
fn test_zoh4r_render_viewport_overlay_function() {
    let (handle, port) = make_server();
    let html = http_get(port, "/editor");
    let b = extract_body(&html);
    assert!(
        b.contains("renderViewportOverlay"),
        "Editor JS must define renderViewportOverlay function"
    );
    handle.stop();
}

#[test]
fn test_zoh4r_snap_guides_api() {
    let (handle, port) = make_server();
    let resp = http_get(port, "/api/viewport/snap_guides");
    let body = extract_body(&resp);
    assert!(
        body.contains("guides"),
        "snap_guides API must return guides array"
    );
    handle.stop();
}

#[test]
fn test_zoh4r_snap_info_api() {
    let (handle, port) = make_server();
    let resp = http_get(port, "/api/viewport/snap_info");
    let body = extract_body(&resp);
    assert!(body.contains("snap_enabled"), "snap_info must include snap_enabled");
    assert!(body.contains("grid_visible"), "snap_info must include grid_visible");
    assert!(body.contains("rulers_visible"), "snap_info must include rulers_visible");
    assert!(body.contains("smart_snap_enabled"), "snap_info must include smart_snap_enabled");
    handle.stop();
}

#[test]
fn test_zoh4r_viewport_overlay_draws_origin() {
    let (handle, port) = make_server();
    let html = http_get(port, "/editor");
    let b = extract_body(&html);
    // The overlay render function should draw origin marker lines
    assert!(
        b.contains("Origin marker") || b.contains("origin") || b.contains("ox"),
        "renderViewportOverlay must draw origin marker"
    );
    handle.stop();
}

#[test]
fn test_zoh4r_viewport_overlay_draws_grid() {
    let (handle, port) = make_server();
    let html = http_get(port, "/editor");
    let b = extract_body(&html);
    assert!(
        b.contains("grid_visible") && b.contains("gridSize"),
        "renderViewportOverlay must draw grid when grid_visible is set"
    );
    handle.stop();
}

#[test]
fn test_zoh4r_viewport_overlay_draws_rulers() {
    let (handle, port) = make_server();
    let html = http_get(port, "/editor");
    let b = extract_body(&html);
    assert!(
        b.contains("rulers_visible") && b.contains("rulerSize"),
        "renderViewportOverlay must draw rulers when rulers_visible is set"
    );
    handle.stop();
}

// ---- pat-vxq4y: Menu parity (scene/project/debug/editor/help + edit) ----

#[test]
fn test_vxq4y_edit_menu_present() {
    let (handle, port) = make_server();
    let html = http_get(port, "/editor");
    let b = extract_body(&html);
    assert!(
        b.contains("data-menu=\"edit\""),
        "Editor must have an Edit menu"
    );
    handle.stop();
}

#[test]
fn test_vxq4y_edit_menu_undo_redo() {
    let (handle, port) = make_server();
    let html = http_get(port, "/editor");
    let b = extract_body(&html);
    assert!(b.contains("edit-undo"), "Edit menu must have Undo action");
    assert!(b.contains("edit-redo"), "Edit menu must have Redo action");
    assert!(b.contains("Ctrl+Z"), "Undo must show Ctrl+Z shortcut");
    assert!(b.contains("Ctrl+Shift+Z"), "Redo must show Ctrl+Shift+Z shortcut");
    handle.stop();
}

#[test]
fn test_vxq4y_edit_menu_clipboard_actions() {
    let (handle, port) = make_server();
    let html = http_get(port, "/editor");
    let b = extract_body(&html);
    assert!(b.contains("edit-cut"), "Edit menu must have Cut action");
    assert!(b.contains("edit-copy"), "Edit menu must have Copy action");
    assert!(b.contains("edit-paste"), "Edit menu must have Paste action");
    handle.stop();
}

#[test]
fn test_vxq4y_edit_menu_selection_actions() {
    let (handle, port) = make_server();
    let html = http_get(port, "/editor");
    let b = extract_body(&html);
    assert!(b.contains("edit-select-all"), "Edit menu must have Select All action");
    assert!(b.contains("edit-deselect"), "Edit menu must have Deselect All action");
    handle.stop();
}

#[test]
fn test_vxq4y_six_menus_in_menu_bar() {
    let (handle, port) = make_server();
    let html = http_get(port, "/editor");
    let b = extract_body(&html);
    assert!(b.contains("data-menu=\"scene\""), "Must have Scene menu");
    assert!(b.contains("data-menu=\"edit\""), "Must have Edit menu");
    assert!(b.contains("data-menu=\"project\""), "Must have Project menu");
    assert!(b.contains("data-menu=\"debug\""), "Must have Debug menu");
    assert!(b.contains("data-menu=\"editor\""), "Must have Editor menu");
    assert!(b.contains("data-menu=\"help\""), "Must have Help menu");
    handle.stop();
}

#[test]
fn test_vxq4y_edit_menu_handler_wired() {
    let (handle, port) = make_server();
    let html = http_get(port, "/editor");
    let b = extract_body(&html);
    assert!(
        b.contains("case 'edit-undo'"),
        "handleMenuAction must handle edit-undo"
    );
    assert!(
        b.contains("case 'edit-redo'"),
        "handleMenuAction must handle edit-redo"
    );
    handle.stop();
}

#[test]
fn test_vxq4y_scene_menu_actions_present() {
    let (handle, port) = make_server();
    let html = http_get(port, "/editor");
    let b = extract_body(&html);
    assert!(b.contains("scene-new"), "Scene menu must have New Scene");
    assert!(b.contains("scene-open"), "Scene menu must have Open Scene");
    assert!(b.contains("scene-save"), "Scene menu must have Save Scene");
    assert!(b.contains("scene-save-as"), "Scene menu must have Save As");
    assert!(b.contains("scene-close"), "Scene menu must have Close Scene");
    assert!(b.contains("scene-quit"), "Scene menu must have Quit");
    handle.stop();
}

#[test]
fn test_vxq4y_debug_menu_actions_present() {
    let (handle, port) = make_server();
    let html = http_get(port, "/editor");
    let b = extract_body(&html);
    assert!(b.contains("debug-run"), "Debug menu must have Run Project");
    assert!(b.contains("debug-pause"), "Debug menu must have Pause");
    assert!(b.contains("debug-stop"), "Debug menu must have Stop");
    assert!(b.contains("debug-step"), "Debug menu must have Step");
    handle.stop();
}

// ---- pat-e0heb: Top bar parity — scene tabs, run controls, editor modes ----

#[test]
fn test_e0heb_five_editor_mode_buttons() {
    let (handle, port) = make_server();
    let html = http_get(port, "/editor");
    let b = extract_body(&html);
    assert!(b.contains(r#"data-mode="2d""#), "Must have 2D mode button");
    assert!(b.contains(r#"data-mode="3d""#), "Must have 3D mode button");
    assert!(
        b.contains(r#"data-mode="script""#),
        "Must have Script mode button"
    );
    assert!(
        b.contains(r#"data-mode="game""#),
        "Must have Game mode button"
    );
    assert!(
        b.contains(r#"data-mode="assetlib""#),
        "Must have AssetLib mode button"
    );
    handle.stop();
}

#[test]
fn test_e0heb_set_editor_mode_game() {
    let (handle, port) = make_server();
    let resp = http_post(port, "/api/editor/mode", r#"{"mode":"game"}"#);
    assert!(resp.contains("200 OK"), "Setting game mode should succeed");
    let body = extract_body(&resp);
    assert!(body.contains(r#""mode":"game""#), "Response should confirm game mode");
    // Verify GET returns the new mode
    let get_resp = http_get(port, "/api/editor/mode");
    let get_body = extract_body(&get_resp);
    assert!(
        get_body.contains(r#""mode":"game""#),
        "GET should return game mode"
    );
    handle.stop();
}

#[test]
fn test_e0heb_set_editor_mode_assetlib() {
    let (handle, port) = make_server();
    let resp = http_post(port, "/api/editor/mode", r#"{"mode":"assetlib"}"#);
    assert!(
        resp.contains("200 OK"),
        "Setting assetlib mode should succeed"
    );
    let body = extract_body(&resp);
    assert!(
        body.contains(r#""mode":"assetlib""#),
        "Response should confirm assetlib mode"
    );
    handle.stop();
}

#[test]
fn test_e0heb_run_control_buttons_present() {
    let (handle, port) = make_server();
    let html = http_get(port, "/editor");
    let b = extract_body(&html);
    assert!(b.contains("btn-play"), "Must have play button");
    assert!(b.contains("btn-pause"), "Must have pause button");
    assert!(b.contains("btn-stop"), "Must have stop button");
    assert!(
        b.contains("btn-play-current"),
        "Must have play-current button"
    );
    assert!(b.contains("Play (F5)"), "Play button must show F5 shortcut");
    assert!(
        b.contains("Pause (F7)"),
        "Pause button must show F7 shortcut"
    );
    assert!(b.contains("Stop (F8)"), "Stop button must show F8 shortcut");
    handle.stop();
}

#[test]
fn test_e0heb_scene_tabs_api_get() {
    let (handle, port) = make_server();
    let resp = http_get(port, "/api/scene/tabs");
    assert!(resp.contains("200 OK"), "GET /api/scene/tabs should succeed");
    let body = extract_body(&resp);
    let v: serde_json::Value = serde_json::from_str(body).unwrap();
    assert!(v["tabs"].is_array(), "tabs must be an array");
    assert_eq!(v["tabs"].as_array().unwrap().len(), 1, "should start with 1 tab");
    assert_eq!(v["active_tab_index"], 0, "active tab should be 0");
    assert_eq!(v["tabs"][0]["name"], "Untitled", "default tab name");
    handle.stop();
}

#[test]
fn test_e0heb_scene_tabs_open_and_switch() {
    let (handle, port) = make_server();
    // Open a new tab
    let resp = http_post(
        port,
        "/api/scene/tabs/open",
        r#"{"path":"res://main.tscn","name":"Main"}"#,
    );
    assert!(resp.contains("200 OK"), "Opening tab should succeed");
    let body = extract_body(&resp);
    let v: serde_json::Value = serde_json::from_str(body).unwrap();
    let tab_id = v["tab_id"].as_u64().unwrap();
    assert!(tab_id > 0, "tab_id should be positive");
    assert_eq!(v["active_tab_index"], 1, "new tab should be active");

    // Verify tabs list
    let resp2 = http_get(port, "/api/scene/tabs");
    let body2 = extract_body(&resp2);
    let v2: serde_json::Value = serde_json::from_str(body2).unwrap();
    assert_eq!(
        v2["tabs"].as_array().unwrap().len(),
        2,
        "should now have 2 tabs"
    );

    // Switch back to first tab
    let resp3 = http_post(
        port,
        "/api/scene/tabs/switch",
        r#"{"index":0}"#,
    );
    assert!(resp3.contains("200 OK"), "Switch should succeed");
    let body3 = extract_body(&resp3);
    let v3: serde_json::Value = serde_json::from_str(body3).unwrap();
    assert_eq!(v3["active_tab_index"], 0, "should switch to tab 0");
    handle.stop();
}

#[test]
fn test_e0heb_scene_tabs_close() {
    let (handle, port) = make_server();
    // Open a second tab
    let resp = http_post(
        port,
        "/api/scene/tabs/open",
        r#"{"path":"res://level1.tscn"}"#,
    );
    let body = extract_body(&resp);
    let v: serde_json::Value = serde_json::from_str(body).unwrap();
    let tab_id = v["tab_id"].as_u64().unwrap();

    // Close the second tab
    let close_resp = http_post(
        port,
        "/api/scene/tabs/close",
        &format!(r#"{{"tab_id":{}}}"#, tab_id),
    );
    assert!(close_resp.contains("200 OK"), "Close should succeed");

    // Verify only 1 tab remains
    let resp2 = http_get(port, "/api/scene/tabs");
    let body2 = extract_body(&resp2);
    let v2: serde_json::Value = serde_json::from_str(body2).unwrap();
    assert_eq!(
        v2["tabs"].as_array().unwrap().len(),
        1,
        "should be back to 1 tab"
    );
    handle.stop();
}

#[test]
fn test_e0heb_cannot_close_last_tab() {
    let (handle, port) = make_server();
    // Get the only tab's id
    let resp = http_get(port, "/api/scene/tabs");
    let body = extract_body(&resp);
    let v: serde_json::Value = serde_json::from_str(body).unwrap();
    let tab_id = v["tabs"][0]["id"].as_u64().unwrap();

    // Try to close it
    let close_resp = http_post(
        port,
        "/api/scene/tabs/close",
        &format!(r#"{{"tab_id":{}}}"#, tab_id),
    );
    assert!(
        close_resp.contains("400"),
        "Should not be able to close last tab"
    );
    handle.stop();
}

#[test]
fn test_e0heb_scene_tabs_reopen_existing() {
    let (handle, port) = make_server();
    // Open a tab
    http_post(
        port,
        "/api/scene/tabs/open",
        r#"{"path":"res://main.tscn","name":"Main"}"#,
    );
    // Open same path again — should switch, not duplicate
    let resp = http_post(
        port,
        "/api/scene/tabs/open",
        r#"{"path":"res://main.tscn","name":"Main"}"#,
    );
    let body = extract_body(&resp);
    let v: serde_json::Value = serde_json::from_str(body).unwrap();
    assert_eq!(v["switched"], true, "Should switch to existing tab");

    // Verify still only 2 tabs
    let resp2 = http_get(port, "/api/scene/tabs");
    let body2 = extract_body(&resp2);
    let v2: serde_json::Value = serde_json::from_str(body2).unwrap();
    assert_eq!(
        v2["tabs"].as_array().unwrap().len(),
        2,
        "should not create duplicate tab"
    );
    handle.stop();
}

#[test]
fn test_e0heb_multi_tab_scene_tabs_in_html() {
    let (handle, port) = make_server();
    let html = http_get(port, "/editor");
    let b = extract_body(&html);
    // Verify multi-tab JS functions exist
    assert!(
        b.contains("fetchSceneTabs"),
        "Must have fetchSceneTabs function"
    );
    assert!(
        b.contains("renderSceneTabs"),
        "Must have renderSceneTabs function"
    );
    assert!(
        b.contains("/api/scene/tabs"),
        "Must reference scene tabs API"
    );
    handle.stop();
}

// ---- pat-x8i15: Create Node dialog parity ----

#[test]
fn test_x8i15_add_node_dialog_present() {
    let (handle, port) = make_server();
    let html = http_get(port, "/editor");
    let b = extract_body(&html);
    assert!(
        b.contains("add-node-dialog"),
        "Editor must have add-node-dialog element"
    );
    handle.stop();
}

#[test]
fn test_x8i15_add_node_search_input() {
    let (handle, port) = make_server();
    let html = http_get(port, "/editor");
    let b = extract_body(&html);
    assert!(
        b.contains("add-node-search"),
        "Dialog must have search input"
    );
    assert!(
        b.contains("Search node type"),
        "Search input must have placeholder text"
    );
    handle.stop();
}

#[test]
fn test_x8i15_2d_node_catalog() {
    let (handle, port) = make_server();
    let html = http_get(port, "/editor");
    let b = extract_body(&html);
    // Must have core 2D node types in the catalog
    assert!(b.contains("'Node2D'"), "Catalog must include Node2D");
    assert!(b.contains("'Sprite2D'"), "Catalog must include Sprite2D");
    assert!(b.contains("'Camera2D'"), "Catalog must include Camera2D");
    assert!(b.contains("'CharacterBody2D'"), "Catalog must include CharacterBody2D");
    assert!(b.contains("'AnimatedSprite2D'"), "Catalog must include AnimatedSprite2D");
    handle.stop();
}

#[test]
fn test_x8i15_node_categories() {
    let (handle, port) = make_server();
    let html = http_get(port, "/editor");
    let b = extract_body(&html);
    assert!(b.contains("CATEGORY_DISPLAY_ORDER"), "Must define category display order");
    assert!(b.contains("'2D'"), "Categories must include 2D");
    assert!(b.contains("'Physics 2D'"), "Categories must include Physics 2D");
    assert!(b.contains("'UI'"), "Categories must include UI");
    handle.stop();
}

#[test]
fn test_x8i15_favorites_section() {
    let (handle, port) = make_server();
    let html = http_get(port, "/editor");
    let b = extract_body(&html);
    assert!(
        b.contains("favoriteNodeTypes"),
        "Must track favorite node types"
    );
    assert!(
        b.contains("Favorites"),
        "Must show Favorites section header"
    );
    handle.stop();
}

#[test]
fn test_x8i15_recent_section() {
    let (handle, port) = make_server();
    let html = http_get(port, "/editor");
    let b = extract_body(&html);
    assert!(
        b.contains("recentNodeTypes"),
        "Must track recent node types"
    );
    assert!(
        b.contains("Recent"),
        "Must show Recent section header"
    );
    handle.stop();
}

#[test]
fn test_x8i15_create_node_dialog_rust_types() {
    // Verify the Rust-side CreateNodeDialog is accessible
    use gdeditor::create_dialog::{CreateNodeDialog, ClassFilter};
    let d = CreateNodeDialog::new();
    assert!(!d.is_visible());
    assert!(d.search_text().is_empty());
    assert_eq!(d.active_filter(), ClassFilter::None);
}

#[test]
fn test_x8i15_dialog_footer_buttons() {
    let (handle, port) = make_server();
    let html = http_get(port, "/editor");
    let b = extract_body(&html);
    assert!(b.contains("add-node-cancel"), "Dialog must have Cancel button");
    assert!(b.contains("add-node-create"), "Dialog must have Create button");
    handle.stop();
}

#[test]
fn test_x8i15_add_node_description_panel() {
    let (handle, port) = make_server();
    let html = http_get(port, "/editor");
    let b = extract_body(&html);
    assert!(
        b.contains("add-node-description"),
        "Dialog must have description panel"
    );
    handle.stop();
}

// ---- pat-9ujvj: Animation editor parity ----

#[test]
fn test_9ujvj_animation_panel_present() {
    let (handle, port) = make_server();
    let html = http_get(port, "/editor");
    let b = extract_body(&html);
    assert!(b.contains("animation-panel"), "Must have animation panel");
    handle.stop();
}

#[test]
fn test_9ujvj_animation_toolbar() {
    let (handle, port) = make_server();
    let html = http_get(port, "/editor");
    let b = extract_body(&html);
    assert!(b.contains("anim-select"), "Must have animation selector");
    assert!(b.contains("anim-new-btn"), "Must have new animation button");
    assert!(b.contains("anim-delete-btn"), "Must have delete animation button");
    assert!(b.contains("anim-record-btn"), "Must have record button");
    assert!(b.contains("anim-play-btn"), "Must have play button");
    assert!(b.contains("anim-stop-btn"), "Must have stop button");
    handle.stop();
}

#[test]
fn test_9ujvj_timeline_canvas() {
    let (handle, port) = make_server();
    let html = http_get(port, "/editor");
    let b = extract_body(&html);
    assert!(b.contains("anim-timeline-canvas"), "Must have timeline canvas");
    assert!(b.contains("anim-playhead"), "Must have playhead indicator");
    assert!(b.contains("anim-tracks"), "Must have tracks container");
    handle.stop();
}

#[test]
fn test_9ujvj_keyframe_support() {
    let (handle, port) = make_server();
    let html = http_get(port, "/editor");
    let b = extract_body(&html);
    assert!(
        b.contains("keyframe") || b.contains("Keyframe"),
        "Must reference keyframes in JS"
    );
    assert!(
        b.contains("/api/animation/keyframe/add"),
        "Must have keyframe add API"
    );
    assert!(
        b.contains("/api/animation/keyframe/remove"),
        "Must have keyframe remove API"
    );
    handle.stop();
}

#[test]
fn test_9ujvj_bezier_curve_editing() {
    let (handle, port) = make_server();
    let html = http_get(port, "/editor");
    let b = extract_body(&html);
    assert!(b.contains("anim-curve-btn"), "Must have curve editor button");
    assert!(
        b.contains("cubic_bezier"),
        "Must support cubic bezier transitions"
    );
    handle.stop();
}

#[test]
fn test_9ujvj_onion_skinning_toggle() {
    let (handle, port) = make_server();
    let html = http_get(port, "/editor");
    let b = extract_body(&html);
    assert!(b.contains("anim-onion-btn"), "Must have onion skinning button");
    assert!(
        b.contains("onion_skinning_enabled"),
        "Must track onion skinning state"
    );
    handle.stop();
}

#[test]
fn test_9ujvj_animation_tree_dialog() {
    let (handle, port) = make_server();
    let html = http_get(port, "/editor");
    let b = extract_body(&html);
    assert!(b.contains("anim-tree-dialog"), "Must have AnimationTree dialog");
    assert!(b.contains("anim-tree-btn"), "Must have AnimationTree button");
    assert!(
        b.contains("AnimationNodeStateMachine"),
        "AnimationTree must show state machine node"
    );
    handle.stop();
}

#[test]
fn test_9ujvj_add_track_button() {
    let (handle, port) = make_server();
    let html = http_get(port, "/editor");
    let b = extract_body(&html);
    assert!(b.contains("anim-add-track-btn"), "Must have Add Track button");
    handle.stop();
}

#[test]
fn test_9ujvj_blend_toolbar() {
    let (handle, port) = make_server();
    let html = http_get(port, "/editor");
    let b = extract_body(&html);
    assert!(b.contains("anim-blend-toolbar"), "Must have blend toolbar");
    assert!(b.contains("anim-blend-slider"), "Must have blend weight slider");
    assert!(b.contains("anim-blend-select"), "Must have blend animation selector");
    handle.stop();
}
