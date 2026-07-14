//! Integration tests for the web editor's viewport and scene tree panels.
//!
//! Acceptance for pat-i3nk4 (Implement 3D viewport component in web editor):
//! - `viewport_frame_stream` exercises the end-to-end path the React viewport
//!   component uses to pull rendered frames from `editor_server`: push a
//!   `FrameBuffer` via `update_frame`, GET `/api/viewport/png` and `/api/viewport`,
//!   and confirm the bytes returned match the pushed frame. Pushing a second
//!   frame must produce a different payload so the client can poll for updates.
//!
//! Acceptance for pat-hr7d5 (Implement scene tree panel with node operations):
//! - `scene_tree_panel` exercises the end-to-end HTTP contract the React scene
//!   tree panel uses to fetch the tree and drive add/rename/reparent/delete
//!   mutations against `editor_server`. Each mutation must be observable via a
//!   subsequent `GET /api/scene` response and the tree endpoint must return the
//!   CORS header the browser client relies on.
//!
//! This file lives in the root `patina-engine` package so the verifier command
//! `cargo nextest run --test editor_integration_test -- <name>` resolves the
//! test target from the workspace root without `-p gdeditor`.

use gdcore::math::Color;
use gdeditor::asset_drag_drop::{
    AssetDragDrop, DragResourceType, DropAction, DropTarget, DropValidity,
};
use gdeditor::editor_server::{EditorServerHandle, EditorState};
use gdeditor::filesystem::{DirChild, EditorFileSystem, FileIcon, FileSystemDock};
use gdrender2d::renderer::FrameBuffer;
use gdscene::node::Node;
use gdscene::SceneTree;
use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;
use std::time::Duration;
use tempfile::TempDir;

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
    tree.add_child(root, Node::new("Main", "Node3D")).unwrap();
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
            Err(e) => panic!("failed to connect to editor_server on port {port}: {e}"),
        }
    }
    unreachable!()
}

fn http_request_raw(port: u16, request: &str) -> Vec<u8> {
    let mut stream = connect_with_retry(port);
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
    stream.write_all(request.as_bytes()).unwrap();
    let mut resp = Vec::new();
    let _ = stream.read_to_end(&mut resp);
    resp
}

fn http_get_raw(port: u16, path: &str) -> Vec<u8> {
    http_request_raw(
        port,
        &format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n"),
    )
}

fn http_post_raw(port: u16, path: &str, body: &str) -> Vec<u8> {
    http_request_raw(
        port,
        &format!(
            "POST {path} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        ),
    )
}

/// Splits a raw HTTP response into (status_line + headers, body_bytes).
fn split_response(resp: &[u8]) -> (String, Vec<u8>) {
    let split = resp
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .expect("response missing header/body separator");
    let head = String::from_utf8_lossy(&resp[..split]).to_string();
    let body = resp[split + 4..].to_vec();
    (head, body)
}

fn content_length(head: &str) -> usize {
    for line in head.lines() {
        if let Some(rest) = line
            .to_ascii_lowercase()
            .strip_prefix("content-length:")
            .map(|s| s.trim().to_string())
        {
            return rest.parse().expect("invalid Content-Length");
        }
    }
    panic!("Content-Length header missing: {head}");
}

/// Exercises the full frame-streaming pipeline consumed by the web viewport
/// component: pushed frames become available over HTTP and subsequent pushes
/// replace the cached payload so the client can poll for updates.
#[test]
fn viewport_frame_stream() {
    let (handle, port) = make_server();

    // Before any frame has been pushed the endpoints must surface 404 so the
    // client can distinguish "server up, no frame yet" from "server down".
    let empty = http_get_raw(port, "/api/viewport/png");
    let empty_str = String::from_utf8_lossy(&empty);
    assert!(
        empty_str.contains("404"),
        "expected 404 before any frame was pushed, got: {empty_str}"
    );

    // Push a red 8x8 frame and pull it back as PNG.
    let red = FrameBuffer::new(8, 8, Color::rgb(1.0, 0.0, 0.0));
    handle.update_frame(red);

    let resp = http_get_raw(port, "/api/viewport/png");
    let (head, body) = split_response(&resp);
    assert!(head.contains("200 OK"), "PNG GET failed: {head}");
    assert!(
        head.to_ascii_lowercase().contains("content-type: image/png"),
        "missing image/png content-type: {head}"
    );
    assert!(
        head.contains("Access-Control-Allow-Origin: *"),
        "CORS header required for browser client: {head}"
    );
    let len = content_length(&head);
    assert_eq!(
        body.len(),
        len,
        "body length ({}) must match Content-Length ({len})",
        body.len()
    );
    assert!(
        body.len() >= 8 && &body[..8] == b"\x89PNG\r\n\x1a\n",
        "body is not a valid PNG (missing magic bytes)"
    );
    let first_png = body.clone();

    // Fetching again without a new frame must return the same cached bytes.
    let resp2 = http_get_raw(port, "/api/viewport/png");
    let (_, body2) = split_response(&resp2);
    assert_eq!(
        body2, first_png,
        "polling twice without update_frame must yield identical bytes"
    );

    // Push a different frame — the stream must advance so the web client sees
    // the new frame on its next poll.
    let blue = FrameBuffer::new(16, 16, Color::rgb(0.0, 0.0, 1.0));
    handle.update_frame(blue);

    let resp3 = http_get_raw(port, "/api/viewport/png");
    let (head3, body3) = split_response(&resp3);
    assert!(head3.contains("200 OK"));
    assert_eq!(body3.len(), content_length(&head3));
    assert!(
        body3.len() >= 8 && &body3[..8] == b"\x89PNG\r\n\x1a\n",
        "second frame is not a valid PNG"
    );
    assert_ne!(
        body3, first_png,
        "update_frame must replace the cached frame for the next poll"
    );

    // The BMP endpoint serves the same cache for clients that need raw pixels
    // without a PNG decoder.
    let resp_bmp = http_get_raw(port, "/api/viewport");
    let (head_bmp, body_bmp) = split_response(&resp_bmp);
    assert!(head_bmp.contains("200 OK"), "BMP GET failed: {head_bmp}");
    assert!(
        head_bmp
            .to_ascii_lowercase()
            .contains("content-type: image/bmp"),
        "missing image/bmp content-type: {head_bmp}"
    );
    assert_eq!(body_bmp.len(), content_length(&head_bmp));
    assert!(
        body_bmp.len() >= 2 && &body_bmp[..2] == b"BM",
        "body is not a valid BMP (missing BM magic)"
    );

    // Confirm the editor state tracks the most-recent frame's dimensions, which
    // the client reads to size its <canvas>.
    {
        let state = handle.state().lock().unwrap();
        assert_eq!(state.viewport_width, 16);
        assert_eq!(state.viewport_height, 16);
    }

    handle.stop();
}

/// Parses a JSON body out of a raw HTTP response.
fn parse_json_body(resp: &[u8]) -> serde_json::Value {
    let (_, body) = split_response(resp);
    let text = String::from_utf8(body).expect("response body was not UTF-8");
    serde_json::from_str(&text)
        .unwrap_or_else(|e| panic!("response body was not valid JSON: {e}\n---\n{text}"))
}

/// Depth-first search for a node in a `node_to_json_tree` payload by name.
fn find_node_by_name<'a>(
    tree: &'a serde_json::Value,
    name: &str,
) -> Option<&'a serde_json::Value> {
    if tree.get("name").and_then(|v| v.as_str()) == Some(name) {
        return Some(tree);
    }
    if let Some(children) = tree.get("children").and_then(|v| v.as_array()) {
        for child in children {
            if let Some(hit) = find_node_by_name(child, name) {
                return Some(hit);
            }
        }
    }
    None
}

fn node_id_by_name(tree: &serde_json::Value, name: &str) -> u64 {
    find_node_by_name(tree, name)
        .and_then(|n| n.get("id").and_then(|v| v.as_u64()))
        .unwrap_or_else(|| panic!("node '{name}' not found in scene tree: {tree}"))
}

/// Exercises the full HTTP contract the React scene tree panel uses to drive
/// add / rename / reparent / delete mutations. Each mutation must be
/// observable via a subsequent `GET /api/scene`, and the tree endpoint must
/// return the CORS header the browser client relies on.
#[test]
fn scene_tree_panel() {
    let (handle, port) = make_server();

    // Initial fetch: the tree must include a root with one child "Main" of
    // class "Node3D", and expose the CORS header the browser needs.
    let resp = http_get_raw(port, "/api/scene");
    let (head, _) = split_response(&resp);
    assert!(head.contains("200 OK"), "scene GET failed: {head}");
    assert!(
        head.contains("Access-Control-Allow-Origin: *"),
        "CORS header required for browser client: {head}"
    );
    assert!(
        head.to_ascii_lowercase()
            .contains("content-type: application/json"),
        "scene endpoint must return JSON: {head}"
    );

    let initial = parse_json_body(&resp);
    let root = initial.get("nodes").expect("scene payload missing nodes");
    let root_id = root.get("id").and_then(|v| v.as_u64()).expect("root id");
    let root_children = root
        .get("children")
        .and_then(|v| v.as_array())
        .expect("root children");
    assert_eq!(
        root_children.len(),
        1,
        "expected a single initial child under root, got: {root_children:?}"
    );
    let main = &root_children[0];
    assert_eq!(main.get("name").and_then(|v| v.as_str()), Some("Main"));
    assert_eq!(main.get("class").and_then(|v| v.as_str()), Some("Node3D"));
    let main_id = main.get("id").and_then(|v| v.as_u64()).expect("main id");

    // Add a "Hero" Node3D under "Main" — the add endpoint must return the
    // created node's id so the client can follow up with a select/rename.
    let add_body = format!(
        r#"{{"parent_id":{main_id},"name":"Hero","class_name":"Node3D"}}"#
    );
    let resp_add = http_post_raw(port, "/api/node/add", &add_body);
    let (head_add, _) = split_response(&resp_add);
    assert!(head_add.contains("200 OK"), "add failed: {head_add}");
    let add_json = parse_json_body(&resp_add);
    let hero_id = add_json
        .get("id")
        .and_then(|v| v.as_u64())
        .expect("add response missing id");
    assert_ne!(hero_id, main_id, "new node must have a distinct id");

    // The next GET /api/scene must reflect the insertion.
    let after_add = parse_json_body(&http_get_raw(port, "/api/scene"));
    let root_after_add = after_add.get("nodes").unwrap();
    let hero = find_node_by_name(root_after_add, "Hero")
        .expect("Hero node missing after add");
    assert_eq!(
        hero.get("id").and_then(|v| v.as_u64()),
        Some(hero_id),
        "Hero id returned by add must match the id in GET /api/scene"
    );
    assert_eq!(hero.get("class").and_then(|v| v.as_str()), Some("Node3D"));

    // Rename Hero → Player.
    let rename_body = format!(r#"{{"node_id":{hero_id},"new_name":"Player"}}"#);
    let resp_rename = http_post_raw(port, "/api/node/rename", &rename_body);
    let (head_rename, _) = split_response(&resp_rename);
    assert!(head_rename.contains("200 OK"), "rename failed: {head_rename}");

    let after_rename = parse_json_body(&http_get_raw(port, "/api/scene"));
    let root_after_rename = after_rename.get("nodes").unwrap();
    assert!(
        find_node_by_name(root_after_rename, "Hero").is_none(),
        "old name 'Hero' must be gone after rename"
    );
    let player = find_node_by_name(root_after_rename, "Player")
        .expect("Player node missing after rename");
    assert_eq!(
        player.get("id").and_then(|v| v.as_u64()),
        Some(hero_id),
        "renamed node must retain its id"
    );

    // Reparent Player from under Main to directly under root.
    let reparent_body = format!(
        r#"{{"node_id":{hero_id},"new_parent_id":{root_id}}}"#
    );
    let resp_reparent = http_post_raw(port, "/api/node/reparent", &reparent_body);
    let (head_reparent, _) = split_response(&resp_reparent);
    assert!(
        head_reparent.contains("200 OK"),
        "reparent failed: {head_reparent}"
    );

    let after_reparent = parse_json_body(&http_get_raw(port, "/api/scene"));
    let root_after_reparent = after_reparent.get("nodes").unwrap();
    let root_kids = root_after_reparent
        .get("children")
        .and_then(|v| v.as_array())
        .unwrap();
    let top_level_names: Vec<&str> = root_kids
        .iter()
        .filter_map(|c| c.get("name").and_then(|v| v.as_str()))
        .collect();
    assert!(
        top_level_names.contains(&"Player"),
        "Player must be directly under root after reparent, got: {top_level_names:?}"
    );
    let main_after = find_node_by_name(root_after_reparent, "Main")
        .expect("Main still present");
    let main_kids: Vec<&str> = main_after
        .get("children")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|c| c.get("name").and_then(|v| v.as_str()))
                .collect()
        })
        .unwrap_or_default();
    assert!(
        !main_kids.contains(&"Player"),
        "Player must no longer be a child of Main, got: {main_kids:?}"
    );
    // Sanity: the id lookup helper still resolves the moved node.
    assert_eq!(node_id_by_name(root_after_reparent, "Player"), hero_id);

    // Delete Player.
    let delete_body = format!(r#"{{"node_id":{hero_id}}}"#);
    let resp_delete = http_post_raw(port, "/api/node/delete", &delete_body);
    let (head_delete, _) = split_response(&resp_delete);
    assert!(head_delete.contains("200 OK"), "delete failed: {head_delete}");

    let after_delete = parse_json_body(&http_get_raw(port, "/api/scene"));
    let root_after_delete = after_delete.get("nodes").unwrap();
    assert!(
        find_node_by_name(root_after_delete, "Player").is_none(),
        "Player must be gone after delete"
    );
    // Main must still be present so the tree panel shows the remaining node.
    assert!(
        find_node_by_name(root_after_delete, "Main").is_some(),
        "Main must survive deletion of Player"
    );

    handle.stop();
}

/// Acceptance for pat-e7n3f (Wire InspectorPanel to web UI with live property
/// editing): exercises the full HTTP contract the React inspector panel uses.
///
/// - `GET /api/inspector?node_id=<id>` must return the inspected node's
///   properties grouped into the fixed set of `InspectorPanel` categories
///   (Transform, Rendering, Physics, Script, Misc). Each category entry must
///   carry `{name, type, value}` so the React inline editors can render and
///   dispatch a typed `POST /api/property/set`.
/// - Writes sent via `POST /api/property/set` must be visible on the very next
///   `GET /api/inspector`, proving the read+write round-trip the React panel
///   depends on for live editing.
/// - The endpoint must return the CORS header the browser client relies on.
#[test]
fn inspector_round_trip() {
    let (handle, port) = make_server();

    // Discover the Main node's id via the scene endpoint.
    let initial_scene = parse_json_body(&http_get_raw(port, "/api/scene"));
    let root = initial_scene
        .get("nodes")
        .expect("scene payload missing nodes");
    let main_id = node_id_by_name(root, "Main");

    // Seed a Transform-category property (position) and a Misc-category
    // property (my_flag) so the grouped response is non-empty and covers
    // multiple categories.
    let set_pos_1 = format!(
        r#"{{"node_id":{main_id},"property":"position","value":{{"type":"Vector3","value":[1.0,2.0,3.0]}}}}"#
    );
    let resp_set_1 = http_post_raw(port, "/api/property/set", &set_pos_1);
    let (head_set_1, _) = split_response(&resp_set_1);
    assert!(
        head_set_1.contains("200 OK"),
        "seed position set failed: {head_set_1}"
    );

    let set_flag = format!(
        r#"{{"node_id":{main_id},"property":"my_flag","value":{{"type":"Bool","value":true}}}}"#
    );
    let resp_set_flag = http_post_raw(port, "/api/property/set", &set_flag);
    let (head_set_flag, _) = split_response(&resp_set_flag);
    assert!(
        head_set_flag.contains("200 OK"),
        "seed my_flag set failed: {head_set_flag}"
    );

    // Fetch the inspector view. Must expose CORS + application/json and
    // return the fixed set of category buckets.
    let resp = http_get_raw(port, &format!("/api/inspector?node_id={main_id}"));
    let (head, _) = split_response(&resp);
    assert!(head.contains("200 OK"), "inspector GET failed: {head}");
    assert!(
        head.contains("Access-Control-Allow-Origin: *"),
        "CORS header required for browser client: {head}"
    );
    assert!(
        head.to_ascii_lowercase()
            .contains("content-type: application/json"),
        "inspector endpoint must return JSON: {head}"
    );

    let payload = parse_json_body(&resp);
    assert_eq!(
        payload.get("id").and_then(|v| v.as_u64()),
        Some(main_id),
        "inspector payload must echo node id"
    );
    assert_eq!(
        payload.get("name").and_then(|v| v.as_str()),
        Some("Main"),
        "inspector payload must echo node name"
    );
    assert_eq!(
        payload.get("class").and_then(|v| v.as_str()),
        Some("Node3D"),
        "inspector payload must echo node class"
    );
    let categories = payload
        .get("categories")
        .and_then(|v| v.as_object())
        .expect("inspector payload missing categories object");
    for label in ["Transform", "Rendering", "Physics", "Script", "Misc"] {
        assert!(
            categories.get(label).and_then(|v| v.as_array()).is_some(),
            "inspector payload missing category '{label}': {payload}"
        );
    }

    // Transform bucket must contain `position` with the Vector3 we set.
    let transform = categories
        .get("Transform")
        .and_then(|v| v.as_array())
        .unwrap();
    let position = transform
        .iter()
        .find(|e| e.get("name").and_then(|n| n.as_str()) == Some("position"))
        .unwrap_or_else(|| {
            panic!("Transform bucket missing 'position': {transform:?}")
        });
    assert_eq!(
        position.get("type").and_then(|v| v.as_str()),
        Some("Vector3"),
        "position entry must report Vector3 type"
    );
    let pos_value = position
        .get("value")
        .and_then(|v| v.get("value"))
        .and_then(|v| v.as_array())
        .expect("position entry value array missing");
    let pos_nums: Vec<f64> = pos_value
        .iter()
        .filter_map(|v| v.as_f64())
        .collect();
    assert_eq!(
        pos_nums,
        vec![1.0, 2.0, 3.0],
        "position must round-trip through inspector"
    );

    // Misc bucket must contain `my_flag` — confirms uncategorized names land
    // in Misc instead of being dropped.
    let misc = categories.get("Misc").and_then(|v| v.as_array()).unwrap();
    let flag = misc
        .iter()
        .find(|e| e.get("name").and_then(|n| n.as_str()) == Some("my_flag"))
        .unwrap_or_else(|| panic!("Misc bucket missing 'my_flag': {misc:?}"));
    assert_eq!(
        flag.get("type").and_then(|v| v.as_str()),
        Some("Bool"),
        "my_flag entry must report Bool type"
    );
    assert_eq!(
        flag.get("value")
            .and_then(|v| v.get("value"))
            .and_then(|v| v.as_bool()),
        Some(true),
        "my_flag must round-trip through inspector"
    );

    // Write a new position and confirm the next inspector fetch reflects it.
    let set_pos_2 = format!(
        r#"{{"node_id":{main_id},"property":"position","value":{{"type":"Vector3","value":[7.0,8.0,9.0]}}}}"#
    );
    let resp_set_2 = http_post_raw(port, "/api/property/set", &set_pos_2);
    let (head_set_2, _) = split_response(&resp_set_2);
    assert!(
        head_set_2.contains("200 OK"),
        "second position set failed: {head_set_2}"
    );

    let resp2 = http_get_raw(port, &format!("/api/inspector?node_id={main_id}"));
    let payload2 = parse_json_body(&resp2);
    let transform2 = payload2
        .get("categories")
        .and_then(|v| v.get("Transform"))
        .and_then(|v| v.as_array())
        .expect("categories.Transform missing on second fetch");
    let position2 = transform2
        .iter()
        .find(|e| e.get("name").and_then(|n| n.as_str()) == Some("position"))
        .expect("position missing on second fetch");
    let pos2_nums: Vec<f64> = position2
        .get("value")
        .and_then(|v| v.get("value"))
        .and_then(|v| v.as_array())
        .unwrap()
        .iter()
        .filter_map(|v| v.as_f64())
        .collect();
    assert_eq!(
        pos2_nums,
        vec![7.0, 8.0, 9.0],
        "inspector must reflect live property updates for the React panel"
    );

    // A missing `node_id` query parameter must fall back to the server's
    // currently selected node. When nothing is selected, the endpoint must
    // surface 404 so the client can distinguish "no selection" from a live
    // payload.
    let resp_none = http_get_raw(port, "/api/inspector");
    let none_str = String::from_utf8_lossy(&resp_none);
    assert!(
        none_str.contains("404"),
        "inspector without node_id and no selection must 404, got: {none_str}"
    );

    // After selecting Main, the fallback path must return the same payload.
    let select_body = format!(r#"{{"node_id":{main_id}}}"#);
    let resp_select = http_post_raw(port, "/api/node/select", &select_body);
    let (head_select, _) = split_response(&resp_select);
    assert!(
        head_select.contains("200 OK"),
        "node select failed: {head_select}"
    );
    let resp_fallback = http_get_raw(port, "/api/inspector");
    let (head_fb, _) = split_response(&resp_fallback);
    assert!(
        head_fb.contains("200 OK"),
        "inspector fallback GET failed: {head_fb}"
    );
    let payload_fb = parse_json_body(&resp_fallback);
    assert_eq!(
        payload_fb.get("id").and_then(|v| v.as_u64()),
        Some(main_id),
        "fallback payload must target the currently selected node"
    );

    handle.stop();
}

// ---------------------------------------------------------------------------
// Acceptance for pat-eunxh (Implement filesystem/asset browser with drag-and-drop)
//
// These tests exercise both the library-level asset browser primitives
// (`EditorFileSystem::list_dir`, `AssetDragDrop` state machine) and the HTTP
// endpoints the React asset browser panel consumes
// (`/api/filesystem/tree`, `/api/filesystem/dir`, `/api/preview/file`,
// `/api/viewport/drop`). Every HTTP test changes the process cwd to a temp
// fixture — this is safe under nextest's one-process-per-test isolation,
// which is the verifier's execution model.
// ---------------------------------------------------------------------------

/// Builds a temp project fixture with a canonical asset layout. The returned
/// `TempDir` must be kept alive for the duration of the test (its Drop
/// removes the directory).
fn make_project_fixture() -> TempDir {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();

    fs::write(root.join("project.godot"), "config_version=5\n").unwrap();

    fs::create_dir_all(root.join("scenes")).unwrap();
    fs::write(
        root.join("scenes/main.tscn"),
        "[gd_scene format=3]\n[node name=\"Main\" type=\"Node2D\"]\n",
    )
    .unwrap();

    fs::create_dir_all(root.join("scripts")).unwrap();
    fs::write(
        root.join("scripts/player.gd"),
        "extends Node2D\n\nfunc _ready():\n    print(\"hello\")\n",
    )
    .unwrap();

    fs::create_dir_all(root.join("textures")).unwrap();
    // Minimal PNG: just the 8-byte signature is enough for name/stem tests
    // (drop handler does not read the file contents).
    let png_signature: [u8; 8] = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
    fs::write(root.join("textures/icon.png"), png_signature).unwrap();

    // Must be excluded by DEFAULT_IGNORED_DIRS.
    fs::create_dir_all(root.join("target")).unwrap();
    fs::write(root.join("target/junk.txt"), "junk").unwrap();

    // Must be excluded as hidden.
    fs::create_dir_all(root.join(".hidden")).unwrap();
    fs::write(root.join(".hidden/secret.txt"), "hidden").unwrap();

    fs::create_dir_all(root.join("nested/sub")).unwrap();
    fs::write(
        root.join("nested/sub/file.tres"),
        "[gd_resource type=\"Resource\" format=3]\n",
    )
    .unwrap();

    tmp
}

/// `EditorFileSystem::list_dir` must sort directories before files and return
/// both in alphabetical order — this is the order the asset browser panel
/// renders entries.
#[test]
fn asset_browser_list_dir_sorts_dirs_first() {
    let fixture = make_project_fixture();
    let fs = EditorFileSystem::new(fixture.path());

    let children = fs.list_dir("res://").expect("list root");
    let names: Vec<&str> = children.iter().map(|c| c.name.as_str()).collect();

    // Directories must come before files; among the dirs, we expect
    // "nested", "scenes", "scripts", "textures" in that order. Hidden
    // ".hidden" and the ignored "target" must not appear at all.
    let dir_prefix: Vec<&str> = children
        .iter()
        .take_while(|c| c.is_directory)
        .map(|c| c.name.as_str())
        .collect();
    assert_eq!(
        dir_prefix,
        vec!["nested", "scenes", "scripts", "textures"],
        "directory ordering wrong: {names:?}"
    );
    let file_suffix: Vec<&str> = children
        .iter()
        .skip_while(|c| c.is_directory)
        .map(|c| c.name.as_str())
        .collect();
    assert_eq!(
        file_suffix,
        vec!["project.godot"],
        "file ordering wrong: {names:?}"
    );

    // project.godot must be tagged as Config.
    let project_entry: &DirChild = children
        .iter()
        .find(|c| c.name == "project.godot")
        .unwrap();
    assert!(!project_entry.is_directory);
    assert_eq!(project_entry.icon, FileIcon::Config);
    assert!(
        project_entry.res_path.starts_with("res://"),
        "res_path must start with res://, got {}",
        project_entry.res_path
    );
}

/// Hidden entries (`.git`, `.hidden`, etc.) and `DEFAULT_IGNORED_DIRS`
/// entries (`target`, `node_modules`, ...) must never appear in a listing —
/// the asset browser panel relies on this to hide build artefacts.
#[test]
fn asset_browser_list_dir_skips_ignored_and_hidden() {
    let fixture = make_project_fixture();
    let fs = EditorFileSystem::new(fixture.path());

    let children = fs.list_dir("res://").expect("list root");
    let names: Vec<String> = children.iter().map(|c| c.name.clone()).collect();

    assert!(
        !names.iter().any(|n| n == "target"),
        "target dir must be hidden from asset browser: {names:?}"
    );
    assert!(
        !names.iter().any(|n| n == ".hidden"),
        "hidden dot dir must not appear: {names:?}"
    );
}

/// Listing a nested directory must return only that directory's direct
/// children, with correct file icons derived from extensions.
#[test]
fn asset_browser_list_dir_nested_subdir() {
    let fixture = make_project_fixture();
    let fs = EditorFileSystem::new(fixture.path());

    let scenes = fs.list_dir("res://scenes/").expect("list scenes");
    assert_eq!(scenes.len(), 1, "scenes/ has exactly one child");
    let main = &scenes[0];
    assert_eq!(main.name, "main.tscn");
    assert_eq!(main.icon, FileIcon::Scene);
    assert!(!main.is_directory);

    let scripts = fs.list_dir("res://scripts/").expect("list scripts");
    assert_eq!(scripts.len(), 1);
    assert_eq!(scripts[0].name, "player.gd");
    assert_eq!(scripts[0].icon, FileIcon::Script);

    let textures = fs.list_dir("res://textures/").expect("list textures");
    assert_eq!(textures.len(), 1);
    assert_eq!(textures[0].name, "icon.png");
    assert_eq!(textures[0].icon, FileIcon::Texture);
}

/// A missing path must surface as an error — the asset browser panel uses
/// this to show "directory not found" rather than silently returning an
/// empty list.
#[test]
fn asset_browser_list_dir_missing_path_returns_error() {
    let fixture = make_project_fixture();
    let fs = EditorFileSystem::new(fixture.path());

    let result = fs.list_dir("res://does/not/exist/");
    assert!(
        result.is_err(),
        "missing path must return Err, got: {result:?}"
    );
}

/// Dragging a texture onto the 2D viewport must drive the drag-drop state
/// machine through the full cycle: Idle → Dragging → Valid drop →
/// CompletedDrop with `CreateNodeFromResource`.
#[test]
fn asset_browser_drag_drop_texture_onto_viewport2d_creates_sprite() {
    let mut dnd = AssetDragDrop::new();
    assert!(!dnd.is_dragging(), "starts idle");

    dnd.begin_drag_from_path("res://textures/icon.png");
    assert!(dnd.is_dragging(), "begin_drag_from_path → dragging");
    let payload = dnd.payload().expect("payload after begin");
    assert_eq!(payload.resource_type, DragResourceType::Texture);

    dnd.update_hover(DropTarget::Viewport2D { x: 128.0, y: 96.0 });
    match dnd.current_validity() {
        Some(DropValidity::Valid(DropAction::CreateNodeFromResource)) => {}
        other => panic!(
            "expected Valid(CreateNodeFromResource) over Viewport2D, got {other:?}"
        ),
    }
    assert!(dnd.can_drop(), "valid hover must accept drop");

    let completed = dnd.drop().expect("drop returns CompletedDrop");
    assert_eq!(completed.res_path, "res://textures/icon.png");
    assert_eq!(completed.action, DropAction::CreateNodeFromResource);
    match completed.target {
        DropTarget::Viewport2D { x, y } => {
            assert_eq!(x, 128.0);
            assert_eq!(y, 96.0);
        }
        other => panic!("expected Viewport2D target, got {other:?}"),
    }
    assert!(!dnd.is_dragging(), "drop returns state to Idle");
    assert_eq!(dnd.drop_history().len(), 1, "history records one drop");
}

/// Dropping a `.gd` script onto a bare viewport must be rejected — scripts
/// need a node target, not empty canvas. The asset browser relies on this
/// to show the "invalid drop" cursor state.
#[test]
fn asset_browser_drag_drop_script_onto_viewport_is_rejected() {
    let mut dnd = AssetDragDrop::new();

    dnd.begin_drag_from_path("res://scripts/player.gd");
    assert_eq!(
        dnd.payload().unwrap().resource_type,
        DragResourceType::Script
    );

    dnd.update_hover(DropTarget::Viewport2D { x: 10.0, y: 10.0 });
    match dnd.current_validity() {
        Some(DropValidity::Invalid(reason)) => {
            assert!(
                reason.to_lowercase().contains("script"),
                "rejection reason should mention scripts: {reason}"
            );
        }
        other => panic!("expected Invalid for script-on-viewport, got {other:?}"),
    }
    assert!(
        !dnd.can_drop(),
        "can_drop must be false for invalid hover"
    );

    // Dropping while invalid still returns None (no completion).
    assert!(
        dnd.drop().is_none(),
        "invalid drop must not produce a CompletedDrop"
    );
}

/// `GET /api/filesystem/tree` must return a JSON payload with `root` and
/// `tree` keys — the React asset browser mounts this tree in its outline
/// panel.
#[test]
fn asset_browser_filesystem_tree_http_returns_entries() {
    let fixture = make_project_fixture();
    std::env::set_current_dir(fixture.path()).expect("chdir fixture");

    let (handle, port) = make_server();

    let resp = http_get_raw(port, "/api/filesystem/tree");
    let (head, _) = split_response(&resp);
    assert!(head.contains("200 OK"), "tree GET failed: {head}");
    assert!(
        head.to_ascii_lowercase()
            .contains("content-type: application/json"),
        "tree endpoint must return JSON: {head}"
    );

    let payload = parse_json_body(&resp);
    assert!(
        payload.get("root").and_then(|v| v.as_str()).is_some(),
        "missing 'root' field: {payload}"
    );
    let tree = payload
        .get("tree")
        .and_then(|v| v.as_array())
        .expect("missing 'tree' array");
    assert!(
        !tree.is_empty(),
        "tree must list project entries, got empty"
    );

    handle.stop();
}

/// `GET /api/filesystem/dir?path=res://` must list direct children with
/// `name`, `res_path`, `is_directory`, `icon`, and `size` fields. This is
/// the endpoint the React asset browser calls when the user expands a
/// directory node.
#[test]
fn asset_browser_filesystem_dir_http_lists_children() {
    let fixture = make_project_fixture();
    std::env::set_current_dir(fixture.path()).expect("chdir fixture");

    let (handle, port) = make_server();

    let resp = http_get_raw(port, "/api/filesystem/dir?path=res://scripts/");
    let (head, _) = split_response(&resp);
    assert!(head.contains("200 OK"), "dir GET failed: {head}");

    let payload = parse_json_body(&resp);
    let children = payload
        .get("children")
        .and_then(|v| v.as_array())
        .expect("children array");
    assert_eq!(
        children.len(),
        1,
        "scripts/ has exactly one child: {children:?}"
    );
    let child = &children[0];
    assert_eq!(child.get("name").and_then(|v| v.as_str()), Some("player.gd"));
    assert_eq!(
        child.get("is_directory").and_then(|v| v.as_bool()),
        Some(false)
    );
    assert_eq!(
        child.get("icon").and_then(|v| v.as_str()),
        Some("Script"),
        "player.gd must be typed as Script: {child}"
    );
    assert!(
        child
            .get("size")
            .and_then(|v| v.as_u64())
            .map(|n| n > 0)
            .unwrap_or(false),
        "size must be > 0 for a non-empty file: {child}"
    );

    handle.stop();
}

/// `GET /api/filesystem/dir?path=...` on a missing directory must respond
/// with HTTP 404, so the asset browser panel can distinguish
/// "no such folder" from "empty folder".
#[test]
fn asset_browser_filesystem_dir_http_missing_returns_404() {
    let fixture = make_project_fixture();
    std::env::set_current_dir(fixture.path()).expect("chdir fixture");

    let (handle, port) = make_server();

    let resp = http_get_raw(port, "/api/filesystem/dir?path=res://no/such/dir/");
    let (head, _) = split_response(&resp);
    assert!(
        head.contains("404"),
        "missing dir must return 404, got: {head}"
    );

    handle.stop();
}

/// `GET /api/preview/file?path=res://...` on a `.gd` script must return a
/// JSON payload with `type: "script"` and a non-empty preview. The asset
/// browser's file-preview hover uses this for script files.
#[test]
fn asset_browser_preview_file_http_returns_script_preview() {
    let fixture = make_project_fixture();
    std::env::set_current_dir(fixture.path()).expect("chdir fixture");

    let (handle, port) = make_server();

    let resp = http_get_raw(port, "/api/preview/file?path=res://scripts/player.gd");
    let (head, _) = split_response(&resp);
    assert!(head.contains("200 OK"), "preview GET failed: {head}");

    let payload = parse_json_body(&resp);
    assert_eq!(
        payload.get("type").and_then(|v| v.as_str()),
        Some("script"),
        "preview type must be script: {payload}"
    );
    assert!(
        payload
            .get("lines")
            .and_then(|v| v.as_u64())
            .map(|n| n > 0)
            .unwrap_or(false),
        "preview must report non-zero line count: {payload}"
    );
    let preview_text = payload
        .get("preview")
        .and_then(|v| v.as_str())
        .expect("preview field");
    assert!(
        preview_text.contains("extends Node2D"),
        "preview must carry file contents: {preview_text}"
    );

    handle.stop();
}

/// `POST /api/viewport/drop` with `asset_type: "texture"` must create a
/// Sprite2D under the requested parent and name it after the file stem.
/// This is the server-side landing for the React asset browser's
/// texture-to-viewport drop gesture.
#[test]
fn asset_browser_viewport_drop_texture_http_creates_sprite2d() {
    let fixture = make_project_fixture();
    std::env::set_current_dir(fixture.path()).expect("chdir fixture");

    let (handle, port) = make_server();

    // Resolve the Main node id so we can use it as the drop parent.
    let initial = parse_json_body(&http_get_raw(port, "/api/scene"));
    let root = initial.get("nodes").expect("scene nodes");
    let main_id = node_id_by_name(root, "Main");

    // The server reads `path` verbatim and uses file_stem for the node name;
    // "textures/icon.png" → Sprite2D named "icon".
    let drop_body = format!(
        r#"{{"asset_type":"texture","path":"textures/icon.png","parent_id":{main_id},"pixel_x":64.0,"pixel_y":32.0}}"#
    );
    let resp = http_post_raw(port, "/api/viewport/drop", &drop_body);
    let (head, _) = split_response(&resp);
    assert!(head.contains("200 OK"), "drop POST failed: {head}");

    let drop_json = parse_json_body(&resp);
    let created_id = drop_json
        .get("id")
        .and_then(|v| v.as_u64())
        .expect("drop response missing id");
    assert_ne!(created_id, main_id, "created id must be distinct");

    // The created Sprite2D must be observable via GET /api/scene.
    let after = parse_json_body(&http_get_raw(port, "/api/scene"));
    let root_after = after.get("nodes").unwrap();
    let icon_node = find_node_by_name(root_after, "icon")
        .expect("expected Sprite2D 'icon' after texture drop");
    assert_eq!(
        icon_node.get("class").and_then(|v| v.as_str()),
        Some("Sprite2D"),
        "texture drop must create a Sprite2D: {icon_node}"
    );
    assert_eq!(
        icon_node.get("id").and_then(|v| v.as_u64()),
        Some(created_id),
        "drop response id must match scene tree id"
    );

    handle.stop();
}

/// Acceptance test for pat-lhpw0 (GDScript editor panel with syntax
/// highlighting). Exercises the HTTP contract the web script editor relies
/// on: load an existing .gd file, save an edited version, reload it,
/// validate both well-formed and broken source, and request syntax
/// highlight spans. Each step uses the same `editor_server` endpoints the
/// React editor will call at runtime so a regression in the server-side
/// parse/highlight path trips this test.
#[test]
fn script_editor_round_trip() {
    let fixture = make_project_fixture();
    std::env::set_current_dir(fixture.path()).expect("chdir fixture");

    let (handle, port) = make_server();

    // ── 1. Load the existing script written by make_project_fixture. ──
    let resp = http_get_raw(port, "/api/script?path=res://scripts/player.gd");
    let (head, _) = split_response(&resp);
    assert!(head.contains("200 OK"), "script GET failed: {head}");
    let payload = parse_json_body(&resp);
    let original = payload
        .get("content")
        .and_then(|v| v.as_str())
        .expect("content field");
    assert!(
        original.contains("extends Node2D"),
        "initial content must carry fixture script: {original}"
    );
    assert!(
        payload
            .get("lines")
            .and_then(|v| v.as_u64())
            .map(|n| n > 0)
            .unwrap_or(false),
        "lines field must be reported: {payload}"
    );

    // ── 2. Save an edited version of the script. ──
    let edited = "extends Node2D\n\nvar counter: int = 0\n\nfunc _ready():\n    counter = 1\n    print(counter)\n";
    let save_body = serde_json::json!({
        "path": "res://scripts/player.gd",
        "content": edited,
    })
    .to_string();
    let save_resp = http_post_raw(port, "/api/script/save", &save_body);
    let (save_head, _) = split_response(&save_resp);
    assert!(
        save_head.contains("200 OK"),
        "script save POST failed: {save_head}"
    );
    let save_payload = parse_json_body(&save_resp);
    assert_eq!(
        save_payload.get("ok").and_then(|v| v.as_bool()),
        Some(true),
        "save response must be ok: {save_payload}"
    );

    // ── 3. Reload and confirm the file now holds the edited content. ──
    let reload = parse_json_body(&http_get_raw(
        port,
        "/api/script?path=res://scripts/player.gd",
    ));
    let reloaded = reload
        .get("content")
        .and_then(|v| v.as_str())
        .expect("content field");
    assert_eq!(
        reloaded, edited,
        "reloaded content must match the saved payload"
    );

    // ── 4. Validate well-formed GDScript — must report ok=true. ──
    let valid_body = serde_json::json!({ "content": edited }).to_string();
    let valid_resp = http_post_raw(port, "/api/script/validate", &valid_body);
    let (valid_head, _) = split_response(&valid_resp);
    assert!(
        valid_head.contains("200 OK"),
        "validate POST failed: {valid_head}"
    );
    let valid_payload = parse_json_body(&valid_resp);
    assert_eq!(
        valid_payload.get("ok").and_then(|v| v.as_bool()),
        Some(true),
        "well-formed script must validate: {valid_payload}"
    );
    let valid_diags = valid_payload
        .get("diagnostics")
        .and_then(|v| v.as_array())
        .expect("diagnostics array");
    assert!(
        valid_diags.is_empty(),
        "well-formed script must have no diagnostics: {valid_payload}"
    );

    // ── 5. Validate broken GDScript — must surface line/col diagnostics. ──
    // `func _ready(` with no closing paren/body is a UnexpectedEof/UnexpectedToken
    // case that the parser must flag; the client renders this in the gutter.
    let broken = "extends Node2D\n\nfunc _ready(\n";
    let broken_body = serde_json::json!({ "content": broken }).to_string();
    let broken_resp = http_post_raw(port, "/api/script/validate", &broken_body);
    let (broken_head, _) = split_response(&broken_resp);
    assert!(
        broken_head.contains("200 OK"),
        "validate POST (broken) failed: {broken_head}"
    );
    let broken_payload = parse_json_body(&broken_resp);
    assert_eq!(
        broken_payload.get("ok").and_then(|v| v.as_bool()),
        Some(false),
        "broken script must fail validation: {broken_payload}"
    );
    let broken_diags = broken_payload
        .get("diagnostics")
        .and_then(|v| v.as_array())
        .expect("diagnostics array");
    assert!(
        !broken_diags.is_empty(),
        "broken script must produce at least one diagnostic: {broken_payload}"
    );
    let first = &broken_diags[0];
    assert!(
        first.get("line").and_then(|v| v.as_u64()).is_some(),
        "diagnostic must carry a line field: {first}"
    );
    assert!(
        first.get("col").and_then(|v| v.as_u64()).is_some(),
        "diagnostic must carry a col field: {first}"
    );
    let message = first
        .get("message")
        .and_then(|v| v.as_str())
        .expect("diagnostic message");
    assert!(
        !message.is_empty(),
        "diagnostic message must not be empty: {first}"
    );

    // ── 6. Request highlight spans — must surface at least one Keyword
    //       span (e.g. `extends`, `func`, `var`, `return`). ──
    let hl_body = serde_json::json!({ "content": edited }).to_string();
    let hl_resp = http_post_raw(port, "/api/script/highlight", &hl_body);
    let (hl_head, _) = split_response(&hl_resp);
    assert!(
        hl_head.contains("200 OK"),
        "highlight POST failed: {hl_head}"
    );
    let hl_payload = parse_json_body(&hl_resp);
    let spans = hl_payload
        .get("spans")
        .and_then(|v| v.as_array())
        .expect("spans array");
    assert!(
        !spans.is_empty(),
        "highlighter must emit at least one span: {hl_payload}"
    );
    let has_keyword = spans.iter().any(|s| {
        s.get("kind").and_then(|v| v.as_str()) == Some("Keyword")
    });
    assert!(
        has_keyword,
        "highlight output must contain at least one Keyword span: {hl_payload}"
    );
    // Every span must carry positional fields the editor uses for rendering.
    for span in spans {
        assert!(
            span.get("line").and_then(|v| v.as_u64()).is_some(),
            "every span must carry a line: {span}"
        );
        assert!(
            span.get("col").and_then(|v| v.as_u64()).is_some(),
            "every span must carry a col: {span}"
        );
        assert!(
            span.get("text").and_then(|v| v.as_str()).is_some(),
            "every span must carry text: {span}"
        );
        assert!(
            span.get("kind").and_then(|v| v.as_str()).is_some(),
            "every span must carry kind: {span}"
        );
    }

    handle.stop();
}

// ---------------------------------------------------------------------------
// pat-etu5p: Asset browser panel with drag-and-drop
// ---------------------------------------------------------------------------

/// Creates a temp project directory with files typical for a Godot project,
/// then starts an editor server whose `asset_browser` is rooted at that dir.
fn make_asset_browser_server() -> (EditorServerHandle, u16, TempDir) {
    let dir = TempDir::new().unwrap();
    // Create project structure
    fs::write(dir.path().join("project.godot"), "[gd_resource]").unwrap();
    fs::create_dir_all(dir.path().join("scenes")).unwrap();
    fs::write(dir.path().join("scenes/main.tscn"), "[gd_scene]").unwrap();
    fs::write(dir.path().join("scenes/player.tscn"), "[gd_scene]").unwrap();
    fs::create_dir_all(dir.path().join("scripts")).unwrap();
    fs::write(dir.path().join("scripts/player.gd"), "extends Node2D").unwrap();
    fs::create_dir_all(dir.path().join("textures")).unwrap();
    fs::write(dir.path().join("textures/icon.png"), "PNG_DATA").unwrap();
    fs::write(dir.path().join("textures/bg.jpg"), "JPEG_DATA").unwrap();
    fs::create_dir_all(dir.path().join("audio")).unwrap();
    fs::write(dir.path().join("audio/bgm.ogg"), "OGG_DATA").unwrap();
    fs::create_dir_all(dir.path().join("models")).unwrap();
    fs::write(dir.path().join("models/tree.glb"), "GLB_DATA").unwrap();

    let port = free_port();
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    tree.add_child(root, Node::new("Main", "Node2D")).unwrap();
    let mut state = EditorState::new(tree);
    // Replace the default cwd-based asset browser with one rooted at the temp dir.
    state.asset_browser =
        FileSystemDock::new(EditorFileSystem::new(dir.path()));
    let handle = EditorServerHandle::start(port, state);
    thread::sleep(Duration::from_millis(100));
    (handle, port, dir)
}

/// Exercises the full asset browser panel lifecycle:
///
/// 1. Refresh — scan the project filesystem.
/// 2. Read state — verify entries, filter, favorites, selection.
/// 3. Expand/collapse directories.
/// 4. Filter entries by text.
/// 5. Select and navigate to files.
/// 6. Manage favorites.
/// 7. Drag-and-drop lifecycle: start, validate, drop, cancel.
#[test]
fn asset_browser() {
    let (handle, port, _dir) = make_asset_browser_server();

    // ── 1. Initial state should have no entries (not yet refreshed). ──
    let resp = http_get_raw(port, "/api/asset_browser");
    let (head, _) = split_response(&resp);
    assert!(head.contains("200 OK"), "GET /api/asset_browser failed: {head}");
    let state = parse_json_body(&resp);
    let entries = state.get("entries").and_then(|v| v.as_array()).unwrap();
    assert!(
        entries.is_empty(),
        "before refresh, entries should be empty: {state}"
    );
    assert_eq!(
        state.get("is_dragging").and_then(|v| v.as_bool()),
        Some(false),
    );

    // ── 2. Refresh — scan the filesystem. ──
    let resp = http_post_raw(port, "/api/asset_browser/refresh", "{}");
    let (head, _) = split_response(&resp);
    assert!(head.contains("200 OK"), "refresh failed: {head}");
    let body = parse_json_body(&resp);
    let count = body.get("count").and_then(|v| v.as_u64()).unwrap();
    // We created: project.godot + main.tscn + player.tscn + player.gd +
    //             icon.png + bg.jpg + bgm.ogg + tree.glb = 8 files
    assert!(
        count >= 8,
        "expected at least 8 files after scan, got {count}"
    );

    // ── 3. After refresh, root-level entries should appear. ──
    let resp = http_get_raw(port, "/api/asset_browser");
    let state = parse_json_body(&resp);
    let entries = state.get("entries").and_then(|v| v.as_array()).unwrap();
    assert!(
        !entries.is_empty(),
        "after refresh, entries should not be empty: {state}"
    );
    // Root level should have directories (audio, models, scenes, scripts, textures)
    // and the project.godot file.
    let dir_names: Vec<&str> = entries
        .iter()
        .filter(|e| e.get("is_directory").and_then(|v| v.as_bool()) == Some(true))
        .filter_map(|e| e.get("name").and_then(|v| v.as_str()))
        .collect();
    assert!(
        dir_names.contains(&"scenes"),
        "should have 'scenes' directory: {dir_names:?}"
    );
    assert!(
        dir_names.contains(&"textures"),
        "should have 'textures' directory: {dir_names:?}"
    );

    // ── 4. Expand a directory — "scenes" should reveal its children. ──
    let scenes_idx = entries
        .iter()
        .position(|e| e.get("name").and_then(|v| v.as_str()) == Some("scenes"))
        .expect("scenes directory not found in entries");

    let resp = http_post_raw(
        port,
        "/api/asset_browser/toggle_expand",
        &format!(r#"{{"index":{scenes_idx}}}"#),
    );
    let (head, _) = split_response(&resp);
    assert!(
        head.contains("200 OK"),
        "toggle_expand failed: {head}"
    );
    let body = parse_json_body(&resp);
    let new_count = body
        .get("entry_count")
        .and_then(|v| v.as_u64())
        .unwrap();
    assert!(
        new_count > entries.len() as u64,
        "expanding scenes should increase entry count (was {}, now {new_count})",
        entries.len()
    );

    // Verify expanded state shows scene files
    let resp = http_get_raw(port, "/api/asset_browser");
    let state = parse_json_body(&resp);
    let entries = state.get("entries").and_then(|v| v.as_array()).unwrap();
    let scene_files: Vec<&str> = entries
        .iter()
        .filter(|e| {
            e.get("icon").and_then(|v| v.as_str()) == Some("scene")
        })
        .filter_map(|e| e.get("name").and_then(|v| v.as_str()))
        .collect();
    assert!(
        scene_files.contains(&"main.tscn"),
        "expanded scenes dir should show main.tscn: {scene_files:?}"
    );
    assert!(
        scene_files.contains(&"player.tscn"),
        "expanded scenes dir should show player.tscn: {scene_files:?}"
    );

    // ── 5. Collapse the directory. ──
    // Re-read to get the current scenes index (may have shifted)
    let scenes_idx = entries
        .iter()
        .position(|e| e.get("name").and_then(|v| v.as_str()) == Some("scenes"))
        .expect("scenes directory not found");
    let resp = http_post_raw(
        port,
        "/api/asset_browser/toggle_expand",
        &format!(r#"{{"index":{scenes_idx}}}"#),
    );
    let body = parse_json_body(&resp);
    let collapsed_count = body.get("entry_count").and_then(|v| v.as_u64()).unwrap();
    assert!(
        collapsed_count < new_count,
        "collapsing should reduce entries (was {new_count}, now {collapsed_count})"
    );

    // ── 6. Filter entries. ──
    let resp = http_post_raw(
        port,
        "/api/asset_browser/filter",
        r#"{"filter":"main"}"#,
    );
    let (head, _) = split_response(&resp);
    assert!(head.contains("200 OK"), "filter failed: {head}");
    let body = parse_json_body(&resp);
    assert!(body.get("ok").and_then(|v| v.as_bool()) == Some(true));

    // Verify filter is applied
    let resp = http_get_raw(port, "/api/asset_browser");
    let state = parse_json_body(&resp);
    assert_eq!(
        state.get("filter").and_then(|v| v.as_str()),
        Some("main"),
        "filter should be 'main': {state}"
    );
    let entries = state.get("entries").and_then(|v| v.as_array()).unwrap();
    let file_names: Vec<&str> = entries
        .iter()
        .filter(|e| e.get("is_directory").and_then(|v| v.as_bool()) != Some(true))
        .filter_map(|e| e.get("name").and_then(|v| v.as_str()))
        .collect();
    assert!(
        file_names.iter().all(|n| n.to_lowercase().contains("main")),
        "all non-dir entries should match 'main' filter: {file_names:?}"
    );

    // Clear filter
    let resp = http_post_raw(
        port,
        "/api/asset_browser/filter",
        r#"{"filter":""}"#,
    );
    let body = parse_json_body(&resp);
    assert!(body.get("ok").and_then(|v| v.as_bool()) == Some(true));

    // ── 7. Select an entry. ──
    let resp = http_post_raw(
        port,
        "/api/asset_browser/select",
        r#"{"index":0}"#,
    );
    let body = parse_json_body(&resp);
    assert!(body.get("ok").and_then(|v| v.as_bool()) == Some(true));

    let resp = http_get_raw(port, "/api/asset_browser");
    let state = parse_json_body(&resp);
    assert_eq!(
        state.get("selected_index").and_then(|v| v.as_u64()),
        Some(0),
        "selected_index should be 0 after selection: {state}"
    );

    // Deselect
    let resp = http_post_raw(
        port,
        "/api/asset_browser/select",
        r#"{"index":null}"#,
    );
    let body = parse_json_body(&resp);
    assert!(body.get("ok").and_then(|v| v.as_bool()) == Some(true));
    let resp = http_get_raw(port, "/api/asset_browser");
    let state = parse_json_body(&resp);
    assert!(
        state.get("selected_index").is_some(),
        "selected_index field should exist: {state}"
    );

    // ── 8. Favorites management. ──
    let resp = http_post_raw(
        port,
        "/api/asset_browser/favorite",
        r#"{"path":"res://scenes/main.tscn","add":true}"#,
    );
    let body = parse_json_body(&resp);
    assert_eq!(body.get("ok").and_then(|v| v.as_bool()), Some(true));
    assert_eq!(body.get("changed").and_then(|v| v.as_bool()), Some(true));

    // Adding same favorite again should return changed=false
    let resp = http_post_raw(
        port,
        "/api/asset_browser/favorite",
        r#"{"path":"res://scenes/main.tscn","add":true}"#,
    );
    let body = parse_json_body(&resp);
    assert_eq!(body.get("changed").and_then(|v| v.as_bool()), Some(false));

    // Verify favorites list
    let resp = http_get_raw(port, "/api/asset_browser");
    let state = parse_json_body(&resp);
    let favs = state.get("favorites").and_then(|v| v.as_array()).unwrap();
    assert_eq!(favs.len(), 1);
    assert_eq!(favs[0].as_str(), Some("res://scenes/main.tscn"));

    // Remove favorite
    let resp = http_post_raw(
        port,
        "/api/asset_browser/favorite",
        r#"{"path":"res://scenes/main.tscn","add":false}"#,
    );
    let body = parse_json_body(&resp);
    assert_eq!(body.get("changed").and_then(|v| v.as_bool()), Some(true));

    let resp = http_get_raw(port, "/api/asset_browser");
    let state = parse_json_body(&resp);
    let favs = state.get("favorites").and_then(|v| v.as_array()).unwrap();
    assert!(favs.is_empty(), "favorites should be empty after removal");

    // ── 9. Navigate to a file (expands parents, selects target). ──
    let resp = http_post_raw(
        port,
        "/api/asset_browser/navigate",
        r#"{"path":"res://scripts/player.gd"}"#,
    );
    let (head, _) = split_response(&resp);
    assert!(head.contains("200 OK"), "navigate failed: {head}");
    let body = parse_json_body(&resp);
    assert!(body.get("ok").and_then(|v| v.as_bool()) == Some(true));
    let nav_idx = body.get("selected_index").and_then(|v| v.as_u64());
    assert!(nav_idx.is_some(), "navigate should select the target: {body}");

    // Verify the selected entry is player.gd
    let resp = http_get_raw(port, "/api/asset_browser");
    let state = parse_json_body(&resp);
    let entries = state.get("entries").and_then(|v| v.as_array()).unwrap();
    let sel_idx = state
        .get("selected_index")
        .and_then(|v| v.as_u64())
        .unwrap() as usize;
    let selected_entry = &entries[sel_idx];
    assert_eq!(
        selected_entry.get("name").and_then(|v| v.as_str()),
        Some("player.gd"),
        "navigate should select player.gd: {selected_entry}"
    );
    assert_eq!(
        selected_entry.get("icon").and_then(|v| v.as_str()),
        Some("script"),
        "player.gd should have script icon: {selected_entry}"
    );

    // ── 10. Drag-and-drop: start → validate → drop ──
    // Start drag from a scene file
    let resp = http_post_raw(
        port,
        "/api/asset_browser/drag_start",
        r#"{"res_path":"res://scenes/player.tscn"}"#,
    );
    let body = parse_json_body(&resp);
    assert_eq!(body.get("ok").and_then(|v| v.as_bool()), Some(true));
    assert_eq!(
        body.get("is_dragging").and_then(|v| v.as_bool()),
        Some(true)
    );

    // Verify is_dragging in state
    let resp = http_get_raw(port, "/api/asset_browser");
    let state = parse_json_body(&resp);
    assert_eq!(
        state.get("is_dragging").and_then(|v| v.as_bool()),
        Some(true),
        "is_dragging should be true during drag: {state}"
    );

    // Validate drop onto 2D viewport — scene file is valid here
    let resp = http_post_raw(
        port,
        "/api/asset_browser/drag_validate",
        r#"{"target":"viewport_2d","x":100,"y":200}"#,
    );
    let body = parse_json_body(&resp);
    assert_eq!(
        body.get("can_drop").and_then(|v| v.as_bool()),
        Some(true),
        "scene should be droppable on viewport_2d: {body}"
    );

    // Execute the drop
    let resp = http_post_raw(port, "/api/asset_browser/drop", "{}");
    let body = parse_json_body(&resp);
    assert_eq!(
        body.get("ok").and_then(|v| v.as_bool()),
        Some(true),
        "drop should succeed: {body}"
    );
    assert_eq!(
        body.get("res_path").and_then(|v| v.as_str()),
        Some("res://scenes/player.tscn"),
    );

    // After drop, should no longer be dragging
    let resp = http_get_raw(port, "/api/asset_browser");
    let state = parse_json_body(&resp);
    assert_eq!(
        state.get("is_dragging").and_then(|v| v.as_bool()),
        Some(false),
        "should not be dragging after drop: {state}"
    );

    // ── 11. Drag-and-drop validation: invalid drop target ──
    // Start drag with a script file
    let resp = http_post_raw(
        port,
        "/api/asset_browser/drag_start",
        r#"{"res_path":"res://scripts/player.gd"}"#,
    );
    let body = parse_json_body(&resp);
    assert_eq!(body.get("ok").and_then(|v| v.as_bool()), Some(true));

    // Validate against viewport — scripts can't be dropped on viewport
    let resp = http_post_raw(
        port,
        "/api/asset_browser/drag_validate",
        r#"{"target":"viewport_2d","x":0,"y":0}"#,
    );
    let body = parse_json_body(&resp);
    assert_eq!(
        body.get("can_drop").and_then(|v| v.as_bool()),
        Some(false),
        "script should NOT be droppable on viewport: {body}"
    );

    // Drop should fail since target is invalid
    let resp = http_post_raw(port, "/api/asset_browser/drop", "{}");
    let body = parse_json_body(&resp);
    assert_eq!(
        body.get("ok").and_then(|v| v.as_bool()),
        Some(false),
        "drop of script on viewport should fail: {body}"
    );

    // ── 12. Drag cancel ──
    let resp = http_post_raw(
        port,
        "/api/asset_browser/drag_start",
        r#"{"res_path":"res://textures/icon.png"}"#,
    );
    let body = parse_json_body(&resp);
    assert_eq!(body.get("ok").and_then(|v| v.as_bool()), Some(true));

    let resp = http_post_raw(port, "/api/asset_browser/drag_cancel", "{}");
    let body = parse_json_body(&resp);
    assert_eq!(body.get("ok").and_then(|v| v.as_bool()), Some(true));

    let resp = http_get_raw(port, "/api/asset_browser");
    let state = parse_json_body(&resp);
    assert_eq!(
        state.get("is_dragging").and_then(|v| v.as_bool()),
        Some(false),
        "should not be dragging after cancel: {state}"
    );

    // ── 13. Drag texture to inspector property (valid). ──
    let resp = http_post_raw(
        port,
        "/api/asset_browser/drag_start",
        r#"{"res_path":"res://textures/icon.png"}"#,
    );
    let body = parse_json_body(&resp);
    assert_eq!(body.get("ok").and_then(|v| v.as_bool()), Some(true));

    let resp = http_post_raw(
        port,
        "/api/asset_browser/drag_validate",
        r#"{"target":"inspector","node_id":1,"property_name":"texture"}"#,
    );
    let body = parse_json_body(&resp);
    assert_eq!(
        body.get("can_drop").and_then(|v| v.as_bool()),
        Some(true),
        "texture should be droppable on texture property: {body}"
    );

    let resp = http_post_raw(port, "/api/asset_browser/drop", "{}");
    let body = parse_json_body(&resp);
    assert_eq!(body.get("ok").and_then(|v| v.as_bool()), Some(true));

    // ── 14. Drag scene to scene tree (valid). ──
    let resp = http_post_raw(
        port,
        "/api/asset_browser/drag_start",
        r#"{"res_path":"res://scenes/main.tscn"}"#,
    );
    let body = parse_json_body(&resp);
    assert_eq!(body.get("ok").and_then(|v| v.as_bool()), Some(true));

    let resp = http_post_raw(
        port,
        "/api/asset_browser/drag_validate",
        r#"{"target":"scene_tree","parent_node_id":1,"sibling_index":-1}"#,
    );
    let body = parse_json_body(&resp);
    assert_eq!(
        body.get("can_drop").and_then(|v| v.as_bool()),
        Some(true),
        "scene should be droppable on scene tree: {body}"
    );

    // Cancel instead of drop to test interleaved operations
    let resp = http_post_raw(port, "/api/asset_browser/drag_cancel", "{}");
    let body = parse_json_body(&resp);
    assert_eq!(body.get("ok").and_then(|v| v.as_bool()), Some(true));

    handle.stop();
}
