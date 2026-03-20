//! Integration tests for Batch 1 editor beads (1-6).
//!
//! Tests cover: duplicate node ops, tree class/script indicators,
//! inspector Array/Dictionary/NodePath types, Array/Dict element operations,
//! inspector toolbar with history, and create-node dialog endpoint.

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

// ---- Bead 1: Scene tree node ops (duplicate) ----

#[test]
fn test_bead1_duplicate_creates_copy() {
    let (handle, port) = make_server();
    let mid = get_main_node_id(port);
    let oid = {
        let r = http_post(
            port,
            "/api/node/add",
            &format!(r#"{{"parent_id":{mid},"name":"Orig","class_name":"Sprite2D"}}"#),
        );
        serde_json::from_str::<serde_json::Value>(extract_body(&r)).unwrap()["id"]
            .as_u64()
            .unwrap()
    };
    let dr = http_post(
        port,
        "/api/node/duplicate",
        &format!(r#"{{"node_id":{oid}}}"#),
    );
    assert!(dr.contains("200 OK"));
    assert!(
        serde_json::from_str::<serde_json::Value>(extract_body(&dr)).unwrap()["id"]
            .as_u64()
            .is_some()
    );
    assert!(extract_body(&http_get(port, "/api/scene")).contains("Orig"));
    handle.stop();
}

#[test]
fn test_bead1_duplicate_missing_404() {
    let (handle, port) = make_server();
    assert!(http_post(port, "/api/node/duplicate", r#"{"node_id":9999999}"#).contains("404"));
    handle.stop();
}

#[test]
fn test_bead1_duplicate_keeps_class() {
    let (handle, port) = make_server();
    let mid = get_main_node_id(port);
    let oid = serde_json::from_str::<serde_json::Value>(extract_body(&http_post(
        port,
        "/api/node/add",
        &format!(r#"{{"parent_id":{mid},"name":"C","class_name":"Camera2D"}}"#),
    )))
    .unwrap()["id"]
        .as_u64()
        .unwrap();
    let did = serde_json::from_str::<serde_json::Value>(extract_body(&http_post(
        port,
        "/api/node/duplicate",
        &format!(r#"{{"node_id":{oid}}}"#),
    )))
    .unwrap()["id"]
        .as_u64()
        .unwrap();
    let n: serde_json::Value =
        serde_json::from_str(extract_body(&http_get(port, &format!("/api/node/{did}")))).unwrap();
    assert_eq!(n["class"], "Camera2D");
    handle.stop();
}

// ---- Bead 2: Tree indicators ----

#[test]
fn test_bead2_class_and_script_fields() {
    let (handle, port) = make_server();
    let v: serde_json::Value =
        serde_json::from_str(extract_body(&http_get(port, "/api/scene"))).unwrap();
    assert_eq!(v["nodes"]["children"][0]["class"], "Node2D");
    assert!(v["nodes"]["children"][0].get("has_script").is_some());
    handle.stop();
}

#[test]
fn test_bead2_script_indicator() {
    let (handle, port) = make_server();
    let mid = get_main_node_id(port);
    let nid = serde_json::from_str::<serde_json::Value>(extract_body(&http_post(
        port,
        "/api/node/add",
        &format!(r#"{{"parent_id":{mid},"name":"S","class_name":"Node2D"}}"#),
    )))
    .unwrap()["id"]
        .as_u64()
        .unwrap();
    http_post(
        port,
        "/api/property/set",
        &format!(
            r#"{{"node_id":{nid},"property":"_script_path","value":{{"type":"String","value":"res://p.gd"}}}}"#
        ),
    );
    let v: serde_json::Value =
        serde_json::from_str(extract_body(&http_get(port, "/api/scene"))).unwrap();
    let s = v["nodes"]["children"][0]["children"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["name"] == "S")
        .unwrap()
        .clone();
    assert_eq!(s["has_script"], true);
    handle.stop();
}

// ---- Bead 3: Inspector core property types ----

#[test]
fn test_bead3_array_property() {
    let (handle, port) = make_server();
    let mid = get_main_node_id(port);
    assert!(http_post(
        port,
        "/api/property/set",
        &format!(
            r#"{{"node_id":{mid},"property":"arr","value":{{"type":"Array","value":[{{"type":"Int","value":1}}]}}}}"#
        )
    )
    .contains("200 OK"));
    let v: serde_json::Value =
        serde_json::from_str(extract_body(&http_get(port, &format!("/api/node/{mid}")))).unwrap();
    assert_eq!(
        v["properties"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["name"] == "arr")
            .unwrap()["type"],
        "Array"
    );
    handle.stop();
}

#[test]
fn test_bead3_dict_property() {
    let (handle, port) = make_server();
    let mid = get_main_node_id(port);
    assert!(http_post(
        port,
        "/api/property/set",
        &format!(
            r#"{{"node_id":{mid},"property":"d","value":{{"type":"Dictionary","value":{{"k":{{"type":"String","value":"v"}}}}}}}}"#
        )
    )
    .contains("200 OK"));
    let v: serde_json::Value =
        serde_json::from_str(extract_body(&http_get(port, &format!("/api/node/{mid}")))).unwrap();
    assert_eq!(
        v["properties"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["name"] == "d")
            .unwrap()["type"],
        "Dictionary"
    );
    handle.stop();
}

#[test]
fn test_bead3_nodepath_property() {
    let (handle, port) = make_server();
    let mid = get_main_node_id(port);
    assert!(http_post(
        port,
        "/api/property/set",
        &format!(
            r#"{{"node_id":{mid},"property":"tgt","value":{{"type":"NodePath","value":"/root/P"}}}}"#
        )
    )
    .contains("200 OK"));
    let v: serde_json::Value =
        serde_json::from_str(extract_body(&http_get(port, &format!("/api/node/{mid}")))).unwrap();
    assert_eq!(
        v["properties"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["name"] == "tgt")
            .unwrap()["type"],
        "NodePath"
    );
    handle.stop();
}

// ---- Bead 4: Inspector advanced (array/dict counts, resource) ----

#[test]
fn test_bead4_array_count() {
    let (handle, port) = make_server();
    let mid = get_main_node_id(port);
    http_post(
        port,
        "/api/property/set",
        &format!(
            r#"{{"node_id":{mid},"property":"items","value":{{"type":"Array","value":[{{"type":"String","value":"a"}},{{"type":"String","value":"b"}},{{"type":"String","value":"c"}}]}}}}"#
        ),
    );
    let v: serde_json::Value =
        serde_json::from_str(extract_body(&http_get(port, &format!("/api/node/{mid}")))).unwrap();
    assert_eq!(
        v["properties"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["name"] == "items")
            .unwrap()["value"]["value"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    handle.stop();
}

#[test]
fn test_bead4_dict_count() {
    let (handle, port) = make_server();
    let mid = get_main_node_id(port);
    http_post(
        port,
        "/api/property/set",
        &format!(
            r#"{{"node_id":{mid},"property":"m","value":{{"type":"Dictionary","value":{{"a":{{"type":"Int","value":1}},"b":{{"type":"Int","value":2}}}}}}}}"#
        ),
    );
    let v: serde_json::Value =
        serde_json::from_str(extract_body(&http_get(port, &format!("/api/node/{mid}")))).unwrap();
    assert_eq!(
        v["properties"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["name"] == "m")
            .unwrap()["value"]["value"]
            .as_object()
            .unwrap()
            .len(),
        2
    );
    handle.stop();
}

#[test]
fn test_bead4_resource_in_html() {
    let (handle, port) = make_server();
    let html = http_get(port, "/editor");
    assert!(extract_body(&html).contains("Resource"));
    handle.stop();
}

// ---- Bead 5: Inspector toolbar (history, resource info) ----

#[test]
fn test_bead5_history_buttons() {
    let (handle, port) = make_server();
    let html = http_get(port, "/editor");
    let b = extract_body(&html);
    // pat-des: Inspector history uses inspectorBack/inspectorForward functions
    assert!(b.contains("inspectorBack"));
    assert!(b.contains("inspectorForward"));
    assert!(b.contains("insp-history"));
    handle.stop();
}

#[test]
fn test_bead5_selection_history_js() {
    let (handle, port) = make_server();
    let html = http_get(port, "/editor");
    let b = extract_body(&html);
    // pat-des: History state uses inspectorHistory array
    assert!(b.contains("inspectorHistory"));
    assert!(b.contains("pushInspectorHistory"));
    handle.stop();
}

// ---- Bead 6: Create node dialog ----

#[test]
fn test_bead6_create_dialog_classes() {
    let (handle, port) = make_server();
    let r = http_post(port, "/api/node/create_dialog", "{}");
    assert!(r.contains("200 OK"));
    let v: serde_json::Value = serde_json::from_str(extract_body(&r)).unwrap();
    let c = v["classes"].as_array().unwrap();
    assert!(c.len() >= 10);
    assert!(c.iter().any(|x| x == "Node2D"));
    assert!(c.iter().any(|x| x == "Sprite2D"));
    assert!(c.iter().any(|x| x == "Control"));
    handle.stop();
}

#[test]
fn test_bead6_create_dialog_physics() {
    let (handle, port) = make_server();
    let v: serde_json::Value = serde_json::from_str(extract_body(&http_post(
        port,
        "/api/node/create_dialog",
        "{}",
    )))
    .unwrap();
    let c = v["classes"].as_array().unwrap();
    assert!(c.iter().any(|x| x == "CharacterBody2D"));
    assert!(c.iter().any(|x| x == "RigidBody2D"));
    handle.stop();
}

#[test]
fn test_bead6_add_node_dialog_html() {
    let (handle, port) = make_server();
    let html = http_get(port, "/editor");
    let b = extract_body(&html);
    assert!(b.contains("add-node-dialog"));
    assert!(b.contains("add-node-search"));
    assert!(b.contains("openAddNodeDialog"));
    handle.stop();
}

// ---- Bead pat-qg1: Screenshot smoke checklist ----
// Verifies all critical panel IDs and UI elements exist in /editor HTML.

#[test]
fn test_qg1_all_panels_present() {
    let (handle, port) = make_server();
    let html = http_get(port, "/editor");
    let b = extract_body(&html);

    // Core layout panels
    let required_ids = [
        "menu-bar",
        "toolbar",
        "left-panel",
        "scene-panel",
        "scene-tree",
        "filesystem-panel",
        "fs-tree",
        "center-area",
        "viewport-panel",
        "viewport-container",
        "bottom-panel",
        "output-log",
        "inspector-panel",
        "inspector-content",
        "statusbar",
    ];
    for id in &required_ids {
        assert!(
            b.contains(id),
            "Editor HTML missing required panel id: {id}"
        );
    }
    handle.stop();
}

#[test]
fn test_qg1_toolbar_buttons_present() {
    let (handle, port) = make_server();
    let html = http_get(port, "/editor");
    let b = extract_body(&html);

    let required_buttons = [
        "btn-add",
        "btn-delete",
        "btn-undo",
        "btn-redo",
        "btn-save",
        "btn-play",
        "btn-pause",
        "btn-stop",
    ];
    for id in &required_buttons {
        assert!(b.contains(id), "Editor HTML missing toolbar button: {id}");
    }
    handle.stop();
}

#[test]
fn test_qg1_dialogs_present() {
    let (handle, port) = make_server();
    let html = http_get(port, "/editor");
    let b = extract_body(&html);

    // Dialogs and overlays
    assert!(b.contains("add-node-dialog"), "Missing add-node-dialog");
    assert!(b.contains("help-dialog"), "Missing help-dialog");
    assert!(b.contains("settings-dialog"), "Missing settings-dialog");
    assert!(b.contains("context-menu"), "Missing context-menu");
    assert!(
        b.contains("box-select-overlay"),
        "Missing box-select-overlay"
    );
    handle.stop();
}

#[test]
fn test_qg1_status_bar_fields() {
    let (handle, port) = make_server();
    let html = http_get(port, "/editor");
    let b = extract_body(&html);

    let status_fields = [
        "status-selected",
        "status-path",
        "status-nodes",
        "status-tool",
        "status-snap",
        "status-zoom",
        "status-cursor",
    ];
    for id in &status_fields {
        assert!(b.contains(id), "Editor HTML missing status bar field: {id}");
    }
    handle.stop();
}

#[test]
fn test_qg1_scene_tabs_and_info() {
    let (handle, port) = make_server();
    let html = http_get(port, "/editor");
    let b = extract_body(&html);

    assert!(b.contains("scene-tabs"), "Missing scene-tabs");
    assert!(b.contains("scene-info"), "Missing scene-info");
    assert!(b.contains("snap-indicator"), "Missing snap-indicator");
    assert!(b.contains("scene-search"), "Missing scene-search");
    handle.stop();
}
