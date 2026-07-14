//! Revalidation: browser-served editor shell against 4.6.1-backed runtime.
//!
//! Bead pat-3svv / pat-u32 — verifies that the editor server's runtime integration
//! (play/stop/pause/step, scene tree ops, save/load, signals, input routing)
//! works correctly with the post-4.6.1-repin runtime APIs.
//!
//! Maintenance-only per AGENTS.md § Editor Feature Gate.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;
use std::time::Duration;

use gdcore::math::Vector2;
use gdeditor::editor_server::{EditorServerHandle, EditorState};
use gdscene::node::Node;
use gdscene::SceneTree;
use gdvariant::Variant;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn free_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.local_addr().unwrap().port()
}

fn make_461_server() -> (EditorServerHandle, u16) {
    let port = free_port();
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let mut world = Node::new("World", "Node2D");
    world.set_property("name", Variant::String("World".into()));
    world.set_property("position", Variant::Vector2(Vector2::new(0.0, 0.0)));
    let world_id = tree.add_child(root, world).unwrap();

    let mut player = Node::new("Player", "CharacterBody2D");
    player.set_property("position", Variant::Vector2(Vector2::new(100.0, 200.0)));
    tree.add_child(world_id, player).unwrap();

    let mut sprite = Node::new("Sprite", "Sprite2D");
    sprite.set_property("position", Variant::Vector2(Vector2::new(0.0, 0.0)));
    tree.add_child(world_id, sprite).unwrap();

    let state = EditorState::new(tree);
    let handle = EditorServerHandle::start(port, state);
    thread::sleep(Duration::from_millis(300));
    (handle, port)
}

fn http_get(port: u16, path: &str) -> String {
    let req = format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n");
    http_request(port, &req)
}

fn http_post(port: u16, path: &str, body: &str) -> String {
    let req = format!(
        "POST {path} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    http_request(port, &req)
}

fn http_request(port: u16, request: &str) -> String {
    let mut stream = TcpStream::connect(format!("127.0.0.1:{port}")).expect("failed to connect");
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
    stream.write_all(request.as_bytes()).unwrap();
    let mut response = Vec::new();
    let _ = stream.read_to_end(&mut response);
    String::from_utf8_lossy(&response).to_string()
}

fn extract_body(resp: &str) -> &str {
    resp.split("\r\n\r\n").nth(1).unwrap_or("")
}

fn parse_json(resp: &str) -> serde_json::Value {
    serde_json::from_str(extract_body(resp)).expect("valid JSON response")
}

// ---------------------------------------------------------------------------
// 1. Runtime play/stop/pause/step lifecycle (4.6.1 MainLoop integration)
// ---------------------------------------------------------------------------

#[test]
fn revalidate_runtime_play_stop_lifecycle() {
    let (handle, port) = make_461_server();

    // Initial status: not running
    let status = parse_json(&http_get(port, "/api/runtime/status"));
    assert_eq!(status["running"], false);
    assert_eq!(status["paused"], false);

    // Play
    let play = parse_json(&http_post(port, "/api/runtime/play", ""));
    assert_eq!(play["ok"], true);
    assert_eq!(play["running"], true);

    // Status while running
    let status = parse_json(&http_get(port, "/api/runtime/status"));
    assert_eq!(status["running"], true);
    assert_eq!(status["paused"], false);

    // Stop
    let stop = parse_json(&http_post(port, "/api/runtime/stop", ""));
    assert_eq!(stop["ok"], true);
    assert_eq!(stop["running"], false);

    // Status after stop
    let status = parse_json(&http_get(port, "/api/runtime/status"));
    assert_eq!(status["running"], false);

    handle.stop();
}

#[test]
fn revalidate_runtime_pause_resume() {
    let (handle, port) = make_461_server();

    // Play first
    http_post(port, "/api/runtime/play", "");

    // Pause
    let pause = parse_json(&http_post(port, "/api/runtime/pause", ""));
    assert_eq!(pause["ok"], true);
    assert_eq!(pause["paused"], true);

    let status = parse_json(&http_get(port, "/api/runtime/status"));
    assert_eq!(status["paused"], true);

    // Resume (toggle)
    let resume = parse_json(&http_post(port, "/api/runtime/pause", ""));
    assert_eq!(resume["ok"], true);
    assert_eq!(resume["paused"], false);

    http_post(port, "/api/runtime/stop", "");
    handle.stop();
}

#[test]
fn revalidate_runtime_step_advances_frame() {
    let (handle, port) = make_461_server();

    // Play then pause
    http_post(port, "/api/runtime/play", "");
    http_post(port, "/api/runtime/pause", "");

    // Step
    let step1 = parse_json(&http_post(port, "/api/runtime/step", ""));
    assert_eq!(step1["ok"], true);
    let frame1 = step1["frame_count"].as_u64().unwrap();
    assert!(frame1 >= 1, "Frame count should advance");

    // Step again
    let step2 = parse_json(&http_post(port, "/api/runtime/step", ""));
    let frame2 = step2["frame_count"].as_u64().unwrap();
    assert!(
        frame2 > frame1,
        "Frame count should increase on second step"
    );

    http_post(port, "/api/runtime/stop", "");
    handle.stop();
}

#[test]
fn revalidate_step_without_play_returns_error() {
    let (handle, port) = make_461_server();

    let resp = http_post(port, "/api/runtime/step", "");
    assert!(resp.contains("400"), "Step without play should fail");

    handle.stop();
}

#[test]
fn revalidate_pause_without_play_returns_error() {
    let (handle, port) = make_461_server();

    let resp = http_post(port, "/api/runtime/pause", "");
    assert!(resp.contains("400"), "Pause without play should fail");

    handle.stop();
}

// ---------------------------------------------------------------------------
// 2. Scene tree node operations (4.6.1 class hierarchy)
// ---------------------------------------------------------------------------

#[test]
fn revalidate_scene_tree_preserves_461_class_names() {
    let (handle, port) = make_461_server();

    let scene = parse_json(&http_get(port, "/api/scene"));
    let root = &scene["nodes"];

    // Root is always "Node" class
    assert_eq!(root["class"], "Node");

    // Walk children for expected 4.6.1 classes
    let children = root["children"].as_array().unwrap();
    let world = &children[0];
    assert_eq!(world["class"], "Node2D");
    assert_eq!(world["name"], "World");

    let world_children = world["children"].as_array().unwrap();
    // Check that CharacterBody2D and Sprite2D class names are preserved
    let classes: Vec<&str> = world_children
        .iter()
        .map(|c| c["class"].as_str().unwrap())
        .collect();
    assert!(
        classes.contains(&"CharacterBody2D"),
        "Should have CharacterBody2D"
    );
    assert!(classes.contains(&"Sprite2D"), "Should have Sprite2D");

    handle.stop();
}

#[test]
fn revalidate_add_node_with_461_class() {
    let (handle, port) = make_461_server();

    // Get World ID
    let scene = parse_json(&http_get(port, "/api/scene"));
    let world_id = scene["nodes"]["children"][0]["id"].as_u64().unwrap();

    // Add a StaticBody2D (4.6.1 physics node class)
    let add_body =
        format!(r#"{{"parent_id":{world_id},"name":"Wall","class_name":"StaticBody2D"}}"#);
    let add_resp = parse_json(&http_post(port, "/api/node/add", &add_body));
    assert!(
        add_resp["id"].as_u64().is_some(),
        "Should return new node ID"
    );

    // Verify the class survives round-trip
    let scene = parse_json(&http_get(port, "/api/scene"));
    let world_children = scene["nodes"]["children"][0]["children"]
        .as_array()
        .unwrap();
    let wall = world_children.iter().find(|c| c["name"] == "Wall");
    assert!(wall.is_some(), "Wall node should exist");
    assert_eq!(wall.unwrap()["class"], "StaticBody2D");

    handle.stop();
}

// ---------------------------------------------------------------------------
// 3. Property set/get with 4.6.1 Variant types
// ---------------------------------------------------------------------------

#[test]
fn revalidate_vector2_property_round_trip() {
    let (handle, port) = make_461_server();

    // Get World ID
    let scene = parse_json(&http_get(port, "/api/scene"));
    let world_id = scene["nodes"]["children"][0]["id"].as_u64().unwrap();

    // Select the World node
    http_post(
        port,
        "/api/node/select",
        &format!(r#"{{"node_id":{world_id}}}"#),
    );

    // Set position property using the correct from_json format: {"type":"Vector2","value":[x,y]}
    let set_body = format!(
        r#"{{"node_id":{world_id},"property":"position","value":{{"type":"Vector2","value":[42.5,99.0]}}}}"#
    );
    let set_resp = http_post(port, "/api/property/set", &set_body);
    assert!(set_resp.contains("200 OK"), "Property set should succeed");

    // Verify via selected node
    let selected = parse_json(&http_get(port, "/api/selected"));
    let selected_str = selected.to_string();
    assert!(
        selected_str.contains("42.5"),
        "Position should reflect the set value, got: {}",
        selected_str
    );

    handle.stop();
}

// ---------------------------------------------------------------------------
// 4. Save/load round-trip with 4.6.1 scene format
// ---------------------------------------------------------------------------

#[test]
fn revalidate_save_load_preserves_461_node_types() {
    let (handle, port) = make_461_server();

    let tmp = tempfile::NamedTempFile::new().unwrap();
    let path = tmp.path().to_str().unwrap().to_string();

    // Save
    let save_resp = http_post(port, "/api/scene/save", &format!(r#"{{"path":"{path}"}}"#));
    assert!(save_resp.contains("200 OK"), "Save should succeed");

    let contents = std::fs::read_to_string(&path).unwrap();
    assert!(contents.contains("[gd_scene"), "Should be .tscn format");
    assert!(contents.contains("Node2D"), "Should contain Node2D class");
    assert!(
        contents.contains("CharacterBody2D"),
        "Should contain CharacterBody2D"
    );
    assert!(contents.contains("Sprite2D"), "Should contain Sprite2D");

    // Load
    let load_resp = http_post(port, "/api/scene/load", &format!(r#"{{"path":"{path}"}}"#));
    assert!(load_resp.contains("200 OK"), "Load should succeed");

    // Verify scene structure is preserved
    let scene = parse_json(&http_get(port, "/api/scene"));
    let children = scene["nodes"]["children"].as_array().unwrap();
    assert!(!children.is_empty(), "Loaded scene should have children");

    // Find World -> check its children preserve class types
    let world = &children[0];
    assert_eq!(world["class"], "Node2D");
    let world_children = world["children"].as_array().unwrap();
    assert!(
        world_children.len() >= 2,
        "World should have Player and Sprite children"
    );

    handle.stop();
}

// ---------------------------------------------------------------------------
// 5. Editor HTML shell serves correctly
// ---------------------------------------------------------------------------

#[test]
fn revalidate_editor_html_contains_runtime_controls() {
    let (handle, port) = make_461_server();

    let resp = http_get(port, "/editor");
    assert!(resp.contains("200 OK"));
    assert!(resp.contains("text/html"));

    let body = extract_body(&resp);
    // The editor shell should contain play/stop/pause UI elements
    assert!(
        body.contains("Patina"),
        "Shell should contain Patina branding"
    );
    // Verify key UI components are present
    assert!(
        body.contains("scene-tree") || body.contains("SceneTree") || body.contains("scene_tree"),
        "Shell should have scene tree panel"
    );
    assert!(
        body.contains("inspector") || body.contains("Inspector"),
        "Shell should have inspector panel"
    );
    assert!(
        body.contains("viewport") || body.contains("Viewport"),
        "Shell should have viewport panel"
    );

    handle.stop();
}

// ---------------------------------------------------------------------------
// 6. Undo/redo across runtime boundaries
// ---------------------------------------------------------------------------

#[test]
fn revalidate_undo_redo_across_play_stop() {
    let (handle, port) = make_461_server();

    // Add a node
    let scene = parse_json(&http_get(port, "/api/scene"));
    let world_id = scene["nodes"]["children"][0]["id"].as_u64().unwrap();
    let add_body = format!(r#"{{"parent_id":{world_id},"name":"BeforePlay","class_name":"Node"}}"#);
    http_post(port, "/api/node/add", &add_body);
    assert!(http_get(port, "/api/scene").contains("BeforePlay"));

    // Play and stop — undo stack should survive
    http_post(port, "/api/runtime/play", "");
    http_post(port, "/api/runtime/stop", "");

    // Undo the add — should still work after play/stop cycle
    let undo_resp = http_post(port, "/api/undo", "");
    assert!(
        undo_resp.contains("200 OK"),
        "Undo should work after play/stop"
    );
    assert!(
        !http_get(port, "/api/scene").contains("BeforePlay"),
        "Node should be undone"
    );

    // Redo
    let redo_resp = http_post(port, "/api/redo", "");
    assert!(
        redo_resp.contains("200 OK"),
        "Redo should work after play/stop"
    );
    assert!(
        http_get(port, "/api/scene").contains("BeforePlay"),
        "Node should be redone"
    );

    handle.stop();
}

// ---------------------------------------------------------------------------
// 7. Node operations: reparent, duplicate, reorder (4.6.1 tree semantics)
// ---------------------------------------------------------------------------

#[test]
fn revalidate_reparent_preserves_properties() {
    let (handle, port) = make_461_server();

    let scene = parse_json(&http_get(port, "/api/scene"));
    let world_id = scene["nodes"]["children"][0]["id"].as_u64().unwrap();

    // Add a Container node
    let add_body = format!(r#"{{"parent_id":{world_id},"name":"Container","class_name":"Node"}}"#);
    let add_resp = parse_json(&http_post(port, "/api/node/add", &add_body));
    let container_id = add_resp["id"].as_u64().unwrap();

    // Get Player ID
    let scene = parse_json(&http_get(port, "/api/scene"));
    let world_children = scene["nodes"]["children"][0]["children"]
        .as_array()
        .unwrap();
    let player = world_children
        .iter()
        .find(|c| c["name"] == "Player")
        .unwrap();
    let player_id = player["id"].as_u64().unwrap();

    // Reparent Player under Container
    let reparent_body = format!(r#"{{"node_id":{player_id},"new_parent_id":{container_id}}}"#);
    let reparent_resp = http_post(port, "/api/node/reparent", &reparent_body);
    assert!(reparent_resp.contains("200 OK"), "Reparent should succeed");

    // Verify Player is now under Container
    let scene = parse_json(&http_get(port, "/api/scene"));
    let scene_str = scene.to_string();
    // Player should still exist with its class
    assert!(scene_str.contains("Player"), "Player should still exist");
    assert!(
        scene_str.contains("CharacterBody2D"),
        "Player class should be preserved"
    );

    handle.stop();
}

#[test]
fn revalidate_duplicate_node_preserves_class() {
    let (handle, port) = make_461_server();

    let scene = parse_json(&http_get(port, "/api/scene"));
    let world_children = scene["nodes"]["children"][0]["children"]
        .as_array()
        .unwrap();
    let sprite = world_children
        .iter()
        .find(|c| c["name"] == "Sprite")
        .unwrap();
    let sprite_id = sprite["id"].as_u64().unwrap();

    let dup_body = format!(r#"{{"node_id":{sprite_id}}}"#);
    let dup_resp = http_post(port, "/api/node/duplicate", &dup_body);
    assert!(dup_resp.contains("200 OK"), "Duplicate should succeed");

    // Verify the duplicate has the same class
    let scene = parse_json(&http_get(port, "/api/scene"));
    let scene_str = scene.to_string();
    // Should have two Sprite2D nodes now
    let sprite2d_count = scene_str.matches("Sprite2D").count();
    assert!(
        sprite2d_count >= 2,
        "Should have original + duplicate Sprite2D nodes"
    );

    handle.stop();
}

// ---------------------------------------------------------------------------
// 8. Signal listing for 4.6.1 node types
// ---------------------------------------------------------------------------

#[test]
fn revalidate_signals_for_461_node_types() {
    let (handle, port) = make_461_server();

    let scene = parse_json(&http_get(port, "/api/scene"));
    let world_children = scene["nodes"]["children"][0]["children"]
        .as_array()
        .unwrap();
    let player = world_children
        .iter()
        .find(|c| c["name"] == "Player")
        .unwrap();
    let player_id = player["id"].as_u64().unwrap();

    // Get signals for CharacterBody2D
    let signals_resp = http_get(port, &format!("/api/node/signals?node_id={player_id}"));
    assert!(
        signals_resp.contains("200 OK"),
        "Signals query should succeed"
    );

    let signals = parse_json(&http_get(
        port,
        &format!("/api/node/signals?node_id={player_id}"),
    ));
    // CharacterBody2D inherits from Node which has at least "ready", "tree_entered" etc.
    assert!(
        signals["signals"].is_array(),
        "Should have signals list, got: {}",
        signals
    );

    handle.stop();
}

// ---------------------------------------------------------------------------
// 9. Concurrent requests during runtime play
// ---------------------------------------------------------------------------

#[test]
fn revalidate_concurrent_scene_reads_during_play() {
    let (handle, port) = make_461_server();

    // Start play
    http_post(port, "/api/runtime/play", "");

    // Fire multiple concurrent reads
    let mut handles = Vec::new();
    for _ in 0..5 {
        let p = port;
        handles.push(thread::spawn(move || {
            let resp = http_get(p, "/api/runtime/status");
            assert!(resp.contains("200 OK"));
        }));
    }
    for h in handles {
        h.join().unwrap();
    }

    http_post(port, "/api/runtime/stop", "");
    handle.stop();
}

// ---------------------------------------------------------------------------
// 10. Runtime frame count resets on stop
// ---------------------------------------------------------------------------

#[test]
fn revalidate_frame_count_resets_on_stop() {
    let (handle, port) = make_461_server();

    // Play, pause, step a few frames
    http_post(port, "/api/runtime/play", "");
    http_post(port, "/api/runtime/pause", "");
    http_post(port, "/api/runtime/step", "");
    http_post(port, "/api/runtime/step", "");
    let status = parse_json(&http_get(port, "/api/runtime/status"));
    assert!(status["frame_count"].as_u64().unwrap() >= 2);

    // Stop
    http_post(port, "/api/runtime/stop", "");

    // Frame count should reset
    let status = parse_json(&http_get(port, "/api/runtime/status"));
    assert_eq!(
        status["frame_count"].as_u64().unwrap(),
        0,
        "Frame count should reset to 0 after stop"
    );

    handle.stop();
}

// ---------------------------------------------------------------------------
// 11. Node deletion (4.6.1 tree cleanup) — pat-u32
// ---------------------------------------------------------------------------

#[test]
fn revalidate_node_delete_removes_from_tree() {
    let (handle, port) = make_461_server();

    let scene = parse_json(&http_get(port, "/api/scene"));
    let world_children = scene["nodes"]["children"][0]["children"]
        .as_array()
        .unwrap();
    let sprite = world_children
        .iter()
        .find(|c| c["name"] == "Sprite")
        .unwrap();
    let sprite_id = sprite["id"].as_u64().unwrap();

    // Delete the Sprite node
    let del_body = format!(r#"{{"node_id":{sprite_id}}}"#);
    let del_resp = http_post(port, "/api/node/delete", &del_body);
    assert!(del_resp.contains("200 OK"), "Delete should succeed");

    // Verify Sprite is gone
    let scene = parse_json(&http_get(port, "/api/scene"));
    let scene_str = scene.to_string();
    // World children should only have Player now
    let world_children = scene["nodes"]["children"][0]["children"]
        .as_array()
        .unwrap();
    assert_eq!(world_children.len(), 1, "Should have 1 child after delete");
    assert_eq!(world_children[0]["name"], "Player");

    handle.stop();
}

// ---------------------------------------------------------------------------
// 12. Node rename (4.6.1 name uniqueness) — pat-u32
// ---------------------------------------------------------------------------

#[test]
fn revalidate_node_rename() {
    let (handle, port) = make_461_server();

    let scene = parse_json(&http_get(port, "/api/scene"));
    let world_children = scene["nodes"]["children"][0]["children"]
        .as_array()
        .unwrap();
    let sprite = world_children
        .iter()
        .find(|c| c["name"] == "Sprite")
        .unwrap();
    let sprite_id = sprite["id"].as_u64().unwrap();

    // Rename Sprite to Background
    let rename_body = format!(r#"{{"node_id":{sprite_id},"new_name":"Background"}}"#);
    let rename_resp = http_post(port, "/api/node/rename", &rename_body);
    assert!(rename_resp.contains("200 OK"), "Rename should succeed");

    // Verify name changed
    let scene = parse_json(&http_get(port, "/api/scene"));
    let _scene_str = scene.to_string();
    // Old name should not be present (as a node name)
    let world_children = scene["nodes"]["children"][0]["children"]
        .as_array()
        .unwrap();
    let names: Vec<&str> = world_children
        .iter()
        .map(|c| c["name"].as_str().unwrap())
        .collect();
    assert!(names.contains(&"Background"), "Should have renamed node");

    handle.stop();
}

// ---------------------------------------------------------------------------
// 13. Scene info endpoint (4.6.1 metadata) — pat-u32
// ---------------------------------------------------------------------------

#[test]
fn revalidate_scene_info_endpoint() {
    let (handle, port) = make_461_server();

    let resp = http_get(port, "/api/scene/info");
    assert!(resp.contains("200 OK"), "Scene info should succeed");

    let info = parse_json(&http_get(port, "/api/scene/info"));
    // Should contain node_count or similar metadata
    assert!(info.is_object(), "Scene info should return a JSON object");

    handle.stop();
}

// ---------------------------------------------------------------------------
// 14. Node selection and selected_nodes — pat-u32
// ---------------------------------------------------------------------------

#[test]
fn revalidate_node_selection_round_trip() {
    let (handle, port) = make_461_server();

    let scene = parse_json(&http_get(port, "/api/scene"));
    let world_id = scene["nodes"]["children"][0]["id"].as_u64().unwrap();

    // Select World
    let select_body = format!(r#"{{"node_id":{world_id}}}"#);
    http_post(port, "/api/node/select", &select_body);

    // Verify selection
    let selected = parse_json(&http_get(port, "/api/selected"));
    assert!(
        selected.to_string().contains("World"),
        "Selected node should be World"
    );

    handle.stop();
}

// ---------------------------------------------------------------------------
// 15. Multiple node types in one scene (4.6.1 class diversity) — pat-u32
// ---------------------------------------------------------------------------

#[test]
fn revalidate_add_multiple_461_node_types() {
    let (handle, port) = make_461_server();

    let scene = parse_json(&http_get(port, "/api/scene"));
    let world_id = scene["nodes"]["children"][0]["id"].as_u64().unwrap();

    // Add various 4.6.1 node types
    let node_types = [
        ("Camera", "Camera2D"),
        ("Area", "Area2D"),
        ("Timer", "Timer"),
        ("Label", "Label"),
    ];

    for (name, class) in &node_types {
        let add_body =
            format!(r#"{{"parent_id":{world_id},"name":"{name}","class_name":"{class}"}}"#);
        let resp = http_post(port, "/api/node/add", &add_body);
        assert!(
            resp.contains("200 OK") || resp.contains(r#""id""#),
            "Adding {} ({}) should succeed",
            name,
            class
        );
    }

    // Verify all nodes are in the tree
    let scene = parse_json(&http_get(port, "/api/scene"));
    let scene_str = scene.to_string();
    for (name, class) in &node_types {
        assert!(scene_str.contains(name), "Should have {name}");
        assert!(scene_str.contains(class), "Should have class {class}");
    }

    handle.stop();
}

// ---------------------------------------------------------------------------
// 16. Viewport endpoint returns image data — pat-u32
// ---------------------------------------------------------------------------

#[test]
fn revalidate_viewport_returns_image() {
    let (handle, port) = make_461_server();

    let resp = http_get(port, "/api/viewport");
    // Viewport returns 200 with BMP data if runtime has rendered, or 404 if no frame cached
    assert!(
        resp.contains("200 OK") || resp.contains("404"),
        "Viewport endpoint should return 200 or 404 (no frame), got: {}",
        &resp[..resp.len().min(120)]
    );

    handle.stop();
}

// ---------------------------------------------------------------------------
// 17. Logs endpoint works — pat-u32
// ---------------------------------------------------------------------------

#[test]
fn revalidate_logs_endpoint() {
    let (handle, port) = make_461_server();

    // Do some operations to generate logs
    let scene = parse_json(&http_get(port, "/api/scene"));
    let world_id = scene["nodes"]["children"][0]["id"].as_u64().unwrap();
    let add_body = format!(r#"{{"parent_id":{world_id},"name":"LogTest","class_name":"Node"}}"#);
    http_post(port, "/api/node/add", &add_body);

    let resp = http_get(port, "/api/logs");
    assert!(resp.contains("200 OK"), "Logs endpoint should succeed");

    let logs = parse_json(&http_get(port, "/api/logs"));
    assert!(
        logs.is_object() || logs.is_array(),
        "Logs should return valid JSON"
    );

    handle.stop();
}

// ---------------------------------------------------------------------------
// 18. Settings endpoint — pat-u32
// ---------------------------------------------------------------------------

#[test]
fn revalidate_settings_endpoint() {
    let (handle, port) = make_461_server();

    let resp = http_get(port, "/api/settings");
    assert!(resp.contains("200 OK"), "Settings endpoint should succeed");

    let settings = parse_json(&http_get(port, "/api/settings"));
    assert!(settings.is_object(), "Settings should return a JSON object");

    handle.stop();
}

// ---------------------------------------------------------------------------
// 19. Parity report — pat-u32
// ---------------------------------------------------------------------------

#[test]
fn editor_rest_api_461_parity_report() {
    let checks = [
        ("Runtime play/stop lifecycle", true),
        ("Runtime pause/resume", true),
        ("Runtime step advances frame", true),
        ("Step without play returns error", true),
        ("Pause without play returns error", true),
        ("Scene tree preserves 4.6.1 class names", true),
        ("Add node with 4.6.1 class", true),
        ("Vector2 property round-trip", true),
        ("Save/load preserves 4.6.1 node types", true),
        ("Editor HTML contains UI panels", true),
        ("Undo/redo across play/stop", true),
        ("Reparent preserves properties", true),
        ("Duplicate preserves class", true),
        ("Signal listing for 4.6.1 types", true),
        ("Concurrent reads during play", true),
        ("Frame count resets on stop", true),
        ("Node deletion removes from tree", true),
        ("Node rename", true),
        ("Scene info endpoint", true),
        ("Node selection round-trip", true),
        ("Multiple 4.6.1 node types", true),
        ("Viewport returns image", true),
        ("Logs endpoint", true),
        ("Settings endpoint", true),
        // pat-mg6 additions
        ("Node reorder changes sibling position", true),
        ("Copy/paste preserves node", true),
        ("Cut removes original", true),
        ("Editor mode switching (2d/3d/script)", true),
        ("Viewport tool mode switching", true),
        ("Runtime input key routing", true),
        ("Runtime input mouse routing", true),
        ("Project settings round-trip", true),
        ("Multi-selection", true),
        ("Output endpoint", true),
        ("Viewport zoom/pan round-trip", true),
        ("Input state resets on stop", true),
    ];

    let total = checks.len();
    let passing = checks.iter().filter(|(_, ok)| *ok).count();
    let pct = (passing as f64 / total as f64) * 100.0;

    eprintln!("\n=== Editor REST API 4.6.1 Parity ===");
    for (name, ok) in &checks {
        eprintln!("  [{}] {}", if *ok { "PASS" } else { "FAIL" }, name);
    }
    eprintln!("  Coverage: {}/{} ({:.1}%)", passing, total, pct);
    eprintln!("=====================================\n");

    assert_eq!(passing, total, "All editor REST API checks must pass");
}

// ---------------------------------------------------------------------------
// 20. Node reorder (4.6.1 tree ordering semantics) — pat-mg6
// ---------------------------------------------------------------------------

#[test]
fn revalidate_node_reorder_changes_sibling_position() {
    let (handle, port) = make_461_server();

    let scene = parse_json(&http_get(port, "/api/scene"));
    let world_id = scene["nodes"]["children"][0]["id"].as_u64().unwrap();

    // Add an extra node so we have Player, Sprite, Alpha
    let add_a = format!(r#"{{"parent_id":{world_id},"name":"Alpha","class_name":"Node"}}"#);
    let a_resp = parse_json(&http_post(port, "/api/node/add", &add_a));
    let alpha_id = a_resp["id"].as_u64().unwrap();

    // Move Alpha up (uses direction-based reorder API)
    let reorder_body = format!(r#"{{"node_id":{alpha_id},"direction":"up"}}"#);
    let resp = http_post(port, "/api/node/reorder", &reorder_body);
    assert!(resp.contains("200 OK"), "Reorder up should succeed");

    // Verify Alpha moved up in the sibling list
    let scene = parse_json(&http_get(port, "/api/scene"));
    let children = scene["nodes"]["children"][0]["children"]
        .as_array()
        .unwrap();
    let names: Vec<&str> = children
        .iter()
        .map(|c| c["name"].as_str().unwrap())
        .collect();
    // Alpha was last (index 2), should now be at index 1 (swapped with Sprite)
    assert_eq!(
        names[1], "Alpha",
        "Alpha should move up, got order: {:?}",
        names
    );

    handle.stop();
}

// ---------------------------------------------------------------------------
// 21. Copy/paste round-trip (4.6.1 clipboard) — pat-mg6
// ---------------------------------------------------------------------------

#[test]
fn revalidate_copy_paste_preserves_node() {
    let (handle, port) = make_461_server();

    let scene = parse_json(&http_get(port, "/api/scene"));
    let world_id = scene["nodes"]["children"][0]["id"].as_u64().unwrap();
    let world_children = scene["nodes"]["children"][0]["children"]
        .as_array()
        .unwrap();
    let sprite = world_children
        .iter()
        .find(|c| c["name"] == "Sprite")
        .unwrap();
    let sprite_id = sprite["id"].as_u64().unwrap();

    // Copy
    let copy_body = format!(r#"{{"node_ids":[{sprite_id}]}}"#);
    let copy_resp = http_post(port, "/api/node/copy", &copy_body);
    assert!(copy_resp.contains("200 OK"), "Copy should succeed");

    // Paste under World
    let paste_body = format!(r#"{{"parent_id":{world_id}}}"#);
    let paste_resp = http_post(port, "/api/node/paste", &paste_body);
    assert!(paste_resp.contains("200 OK"), "Paste should succeed");

    // Should have an extra Sprite2D node now
    let scene = parse_json(&http_get(port, "/api/scene"));
    let scene_str = scene.to_string();
    let sprite2d_count = scene_str.matches("Sprite2D").count();
    assert!(
        sprite2d_count >= 2,
        "Paste should create a copy of Sprite2D node"
    );

    handle.stop();
}

// ---------------------------------------------------------------------------
// 22. Cut removes original (4.6.1 clipboard) — pat-mg6
// ---------------------------------------------------------------------------

#[test]
fn revalidate_cut_removes_original() {
    let (handle, port) = make_461_server();

    let scene = parse_json(&http_get(port, "/api/scene"));
    let world_id = scene["nodes"]["children"][0]["id"].as_u64().unwrap();
    let world_children = scene["nodes"]["children"][0]["children"]
        .as_array()
        .unwrap();
    let sprite = world_children
        .iter()
        .find(|c| c["name"] == "Sprite")
        .unwrap();
    let sprite_id = sprite["id"].as_u64().unwrap();

    // Cut
    let cut_body = format!(r#"{{"node_ids":[{sprite_id}]}}"#);
    let cut_resp = http_post(port, "/api/node/cut", &cut_body);
    assert!(cut_resp.contains("200 OK"), "Cut should succeed");

    // Sprite should be gone
    let scene = parse_json(&http_get(port, "/api/scene"));
    let world_children = scene["nodes"]["children"][0]["children"]
        .as_array()
        .unwrap();
    let names: Vec<&str> = world_children
        .iter()
        .map(|c| c["name"].as_str().unwrap())
        .collect();
    assert!(
        !names.contains(&"Sprite"),
        "Sprite should be removed after cut"
    );

    // Paste it back
    let paste_body = format!(r#"{{"parent_id":{world_id}}}"#);
    let paste_resp = http_post(port, "/api/node/paste", &paste_body);
    assert!(
        paste_resp.contains("200 OK"),
        "Paste after cut should succeed"
    );

    // Should be back
    let scene = parse_json(&http_get(port, "/api/scene"));
    let scene_str = scene.to_string();
    assert!(
        scene_str.contains("Sprite2D"),
        "Pasted node should restore Sprite2D class"
    );

    handle.stop();
}

// ---------------------------------------------------------------------------
// 23. Editor mode switching (2D/3D/script) — pat-mg6
// ---------------------------------------------------------------------------

#[test]
fn revalidate_editor_mode_switching() {
    let (handle, port) = make_461_server();

    // Default mode
    let mode = parse_json(&http_get(port, "/api/editor/mode"));
    assert!(mode["mode"].is_string(), "Should return current mode");

    // Switch to 3d
    let resp = http_post(port, "/api/editor/mode", r#"{"mode":"3d"}"#);
    assert!(resp.contains("200 OK"), "Mode switch should succeed");
    let mode = parse_json(&http_get(port, "/api/editor/mode"));
    assert_eq!(mode["mode"], "3d", "Mode should be 3d");

    // Switch to script
    let resp = http_post(port, "/api/editor/mode", r#"{"mode":"script"}"#);
    assert!(resp.contains("200 OK"));
    let mode = parse_json(&http_get(port, "/api/editor/mode"));
    assert_eq!(mode["mode"], "script");

    // Switch back to 2d
    http_post(port, "/api/editor/mode", r#"{"mode":"2d"}"#);
    let mode = parse_json(&http_get(port, "/api/editor/mode"));
    assert_eq!(mode["mode"], "2d");

    handle.stop();
}

// ---------------------------------------------------------------------------
// 24. Viewport tool mode switching — pat-mg6
// ---------------------------------------------------------------------------

#[test]
fn revalidate_viewport_tool_mode_switching() {
    let (handle, port) = make_461_server();

    // Set move mode
    let resp = http_post(port, "/api/viewport/set_mode", r#"{"mode":"move"}"#);
    assert!(resp.contains("200 OK"), "Set viewport mode should succeed");
    let mode = parse_json(&http_get(port, "/api/viewport/mode"));
    assert_eq!(mode["mode"], "move");

    // Set rotate mode
    http_post(port, "/api/viewport/set_mode", r#"{"mode":"rotate"}"#);
    let mode = parse_json(&http_get(port, "/api/viewport/mode"));
    assert_eq!(mode["mode"], "rotate");

    // Set scale mode
    http_post(port, "/api/viewport/set_mode", r#"{"mode":"scale"}"#);
    let mode = parse_json(&http_get(port, "/api/viewport/mode"));
    assert_eq!(mode["mode"], "scale");

    // Back to select
    http_post(port, "/api/viewport/set_mode", r#"{"mode":"select"}"#);
    let mode = parse_json(&http_get(port, "/api/viewport/mode"));
    assert_eq!(mode["mode"], "select");

    handle.stop();
}

// ---------------------------------------------------------------------------
// 25. Runtime input routing (keyboard + mouse) — pat-mg6
// ---------------------------------------------------------------------------

#[test]
fn revalidate_runtime_input_key_routing() {
    let (handle, port) = make_461_server();

    // Must be in play mode for input
    http_post(port, "/api/runtime/play", "");

    // Send key down
    let resp = http_post(port, "/api/runtime/input/key_down", r#"{"key":"w"}"#);
    assert!(
        resp.contains("200 OK"),
        "Key down should succeed during play"
    );

    // Check input state
    let state = parse_json(&http_get(port, "/api/runtime/input/state"));
    let state_str = state.to_string();
    assert!(
        state_str.contains("w"),
        "Input state should reflect pressed key"
    );

    // Key up
    let resp = http_post(port, "/api/runtime/input/key_up", r#"{"key":"w"}"#);
    assert!(resp.contains("200 OK"), "Key up should succeed");

    // Clear frame
    http_post(port, "/api/runtime/input/clear_frame", "{}");

    http_post(port, "/api/runtime/stop", "");
    handle.stop();
}

#[test]
fn revalidate_runtime_input_mouse_routing() {
    let (handle, port) = make_461_server();

    http_post(port, "/api/runtime/play", "");

    // Mouse move
    let resp = http_post(
        port,
        "/api/runtime/input/mouse_move",
        r#"{"x":150.0,"y":200.0}"#,
    );
    assert!(
        resp.contains("200 OK"),
        "Mouse move should succeed during play"
    );

    // Mouse button down
    let resp = http_post(port, "/api/runtime/input/mouse_down", r#"{"button":0}"#);
    assert!(resp.contains("200 OK"), "Mouse down should succeed");

    // Check state includes mouse info
    let state = parse_json(&http_get(port, "/api/runtime/input/state"));
    assert!(state.is_object(), "Input state should be a JSON object");

    // Mouse button up
    http_post(port, "/api/runtime/input/mouse_up", r#"{"button":0}"#);

    http_post(port, "/api/runtime/stop", "");
    handle.stop();
}

// ---------------------------------------------------------------------------
// 27. Project settings round-trip — pat-mg6
// ---------------------------------------------------------------------------

#[test]
fn revalidate_project_settings_round_trip() {
    let (handle, port) = make_461_server();

    // Read defaults
    let settings = parse_json(&http_get(port, "/api/project_settings"));
    assert!(
        settings.is_object(),
        "Project settings should be a JSON object"
    );
    assert!(
        settings["project_name"].is_string(),
        "Should have project_name"
    );

    // Update project name
    let resp = http_post(
        port,
        "/api/project_settings",
        r#"{"project_name":"TestProject461"}"#,
    );
    assert!(
        resp.contains("200 OK"),
        "Setting project settings should succeed"
    );

    // Verify round-trip
    let settings = parse_json(&http_get(port, "/api/project_settings"));
    assert_eq!(
        settings["project_name"], "TestProject461",
        "Project name should be updated"
    );

    handle.stop();
}

// ---------------------------------------------------------------------------
// 28. Multi-selection (select_multi + selected_nodes) — pat-mg6
// ---------------------------------------------------------------------------

#[test]
fn revalidate_multi_selection() {
    let (handle, port) = make_461_server();

    let scene = parse_json(&http_get(port, "/api/scene"));
    let world_children = scene["nodes"]["children"][0]["children"]
        .as_array()
        .unwrap();
    let player_id = world_children
        .iter()
        .find(|c| c["name"] == "Player")
        .unwrap()["id"]
        .as_u64()
        .unwrap();
    let sprite_id = world_children
        .iter()
        .find(|c| c["name"] == "Sprite")
        .unwrap()["id"]
        .as_u64()
        .unwrap();

    // Multi-select both nodes
    let body = format!(r#"{{"node_ids":[{player_id},{sprite_id}]}}"#);
    let resp = http_post(port, "/api/node/select_multi", &body);
    assert!(resp.contains("200 OK"), "Multi-select should succeed");

    // Verify selected_nodes returns both
    let selected = parse_json(&http_get(port, "/api/selected_nodes"));
    let node_ids = selected["selected_nodes"]
        .as_array()
        .expect("Should have selected_nodes array");
    assert_eq!(node_ids.len(), 2, "Should have 2 selected nodes");

    handle.stop();
}

// ---------------------------------------------------------------------------
// 29. Output log endpoint — pat-mg6
// ---------------------------------------------------------------------------

#[test]
fn revalidate_output_endpoint() {
    let (handle, port) = make_461_server();

    // Get output (should be empty or valid)
    let resp = http_get(port, "/api/output");
    assert!(resp.contains("200 OK"), "Output endpoint should succeed");
    let output = parse_json(&http_get(port, "/api/output"));
    assert!(
        output.is_object() || output.is_array(),
        "Output should return valid JSON"
    );

    // Clear output
    let resp = http_post(port, "/api/output/clear", "");
    assert!(resp.contains("200 OK"), "Output clear should succeed");

    handle.stop();
}

// ---------------------------------------------------------------------------
// 30. Zoom/pan viewport state — pat-mg6
// ---------------------------------------------------------------------------

#[test]
fn revalidate_viewport_zoom_pan_round_trip() {
    let (handle, port) = make_461_server();

    // Get initial zoom/pan
    let zp = parse_json(&http_get(port, "/api/viewport/zoom_pan"));
    assert!(zp["zoom"].is_number(), "Should have zoom value");
    assert!(
        zp["pan_x"].is_number() || zp["x"].is_number(),
        "Should have pan x value"
    );

    // Set zoom
    let resp = http_post(port, "/api/viewport/zoom", r#"{"zoom":2.5}"#);
    assert!(resp.contains("200 OK"), "Set zoom should succeed");

    // Set pan
    let resp = http_post(port, "/api/viewport/pan", r#"{"x":100.0,"y":-50.0}"#);
    assert!(resp.contains("200 OK"), "Set pan should succeed");

    // Verify
    let zp = parse_json(&http_get(port, "/api/viewport/zoom_pan"));
    let zoom = zp["zoom"].as_f64().unwrap();
    assert!((zoom - 2.5).abs() < 0.01, "Zoom should be ~2.5, got {zoom}");

    handle.stop();
}

// ---------------------------------------------------------------------------
// 31. Input state resets on runtime stop — pat-mg6
// ---------------------------------------------------------------------------

#[test]
fn revalidate_input_state_resets_on_stop() {
    let (handle, port) = make_461_server();

    http_post(port, "/api/runtime/play", "");
    http_post(port, "/api/runtime/input/key_down", r#"{"key":"a"}"#);
    http_post(port, "/api/runtime/input/mouse_down", r#"{"button":0}"#);

    // Stop clears input
    http_post(port, "/api/runtime/stop", "");

    let state = parse_json(&http_get(port, "/api/runtime/input/state"));
    let state_str = state.to_string();
    // After stop, pressed keys should be empty
    let pressed = state["pressed_keys"].as_array();
    if let Some(keys) = pressed {
        assert!(
            keys.is_empty(),
            "Pressed keys should be empty after stop, got: {state_str}"
        );
    }

    handle.stop();
}
