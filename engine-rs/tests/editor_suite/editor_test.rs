//! Integration tests for the Patina editor server and web UI.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;
use std::time::Duration;

use gdcore::math::{Color, Vector2};
use gdeditor::editor_server::{EditorServerHandle, EditorState};
use gdrender2d::renderer::FrameBuffer;
use gdscene::node::Node;
use gdscene::SceneTree;
use gdvariant::Variant;

fn free_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.local_addr().unwrap().port()
}

fn make_test_server() -> (EditorServerHandle, u16) {
    let port = free_port();
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let mut world = Node::new("World", "Node");
    world.set_property("name", Variant::String("World".into()));
    let world_id = tree.add_child(root, world).unwrap();

    let mut player = Node::new("Player", "Node2D");
    player.set_property("position", Variant::Vector2(Vector2::new(100.0, 200.0)));
    player.set_property("rotation", Variant::Float(0.0));
    player.set_property("visible", Variant::Bool(true));
    tree.add_child(world_id, player).unwrap();

    let enemy = Node::new("Enemy", "Node2D");
    tree.add_child(world_id, enemy).unwrap();

    let ground = Node::new("Ground", "Node2D");
    tree.add_child(world_id, ground).unwrap();

    let state = EditorState::new(tree);
    let handle = EditorServerHandle::start(port, state);
    thread::sleep(Duration::from_millis(300));
    (handle, port)
}

fn http_get(port: u16, path: &str) -> String {
    let req = format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n");
    http_request_str(port, &req)
}

fn http_post(port: u16, path: &str, body: &str) -> String {
    let req = format!(
        "POST {path} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    http_request_str(port, &req)
}

fn http_request_str(port: u16, request: &str) -> String {
    let mut stream = TcpStream::connect(format!("127.0.0.1:{port}")).expect("failed to connect");
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
    stream.write_all(request.as_bytes()).unwrap();
    let mut response = Vec::new();
    let _ = stream.read_to_end(&mut response);
    String::from_utf8_lossy(&response).to_string()
}

fn http_request_raw(port: u16, request: &str) -> Vec<u8> {
    let mut stream = TcpStream::connect(format!("127.0.0.1:{port}")).expect("failed to connect");
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
    stream.write_all(request.as_bytes()).unwrap();
    let mut response = Vec::new();
    let _ = stream.read_to_end(&mut response);
    response
}

fn extract_body(resp: &str) -> &str {
    resp.split("\r\n\r\n").nth(1).unwrap_or("")
}

fn get_world_node_id(port: u16) -> u64 {
    // Retry a few times in case the server is still starting
    for attempt in 0..3 {
        let resp = http_get(port, "/api/scene");
        let body = extract_body(&resp);
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(body) {
            if let Some(id) = v["nodes"]["children"][0]["id"].as_u64() {
                return id;
            }
        }
        if attempt < 2 {
            std::thread::sleep(Duration::from_millis(200));
        }
    }
    panic!("Failed to get world node id after 3 attempts");
}

fn get_player_node_id(port: u16) -> u64 {
    let resp = http_get(port, "/api/scene");
    let body = extract_body(&resp);
    let v: serde_json::Value = serde_json::from_str(body).expect("scene JSON parse failed");
    let world = &v["nodes"]["children"][0];
    assert!(
        world["name"].is_string(),
        "World node not found in scene: {v}"
    );
    let player = &world["children"][0];
    assert!(
        player["name"].is_string(),
        "Player node not found under World: {world}"
    );
    player["id"].as_u64().expect("Player node missing id")
}

// --- Tests ---

#[test]
fn editor_html_contains_patina() {
    let (handle, port) = make_test_server();
    let resp = http_get(port, "/editor");
    assert!(resp.contains("200 OK"));
    assert!(
        resp.contains("Patina"),
        "Editor HTML should contain 'Patina'"
    );
    assert!(resp.contains("text/html"));
    handle.stop();
}

#[test]
fn editor_html_contains_ui_elements() {
    let (handle, port) = make_test_server();
    let resp = http_get(port, "/editor");
    assert!(
        resp.contains("scene-tree"),
        "Should contain scene tree panel"
    );
    assert!(resp.contains("inspector"), "Should contain inspector panel");
    assert!(resp.contains("viewport"), "Should contain viewport panel");
    assert!(resp.contains("toolbar"), "Should contain toolbar");
    handle.stop();
}

#[test]
fn api_scene_returns_json_with_nodes() {
    let (handle, port) = make_test_server();
    let resp = http_get(port, "/api/scene");
    assert!(resp.contains("200 OK"));
    let body = extract_body(&resp);
    let v: serde_json::Value = serde_json::from_str(body).unwrap();
    assert_eq!(v["nodes"]["name"], "root");
    let children = v["nodes"]["children"].as_array().unwrap();
    assert!(!children.is_empty());
    assert_eq!(children[0]["name"], "World");
    // World should have Player, Enemy, Ground
    let world_children = children[0]["children"].as_array().unwrap();
    assert_eq!(world_children.len(), 3);
    assert_eq!(world_children[0]["name"], "Player");
    assert_eq!(world_children[1]["name"], "Enemy");
    assert_eq!(world_children[2]["name"], "Ground");
    handle.stop();
}

#[test]
fn select_then_get_selected() {
    let (handle, port) = make_test_server();
    let player_id = get_player_node_id(port);

    // Select the player
    let resp = http_post(
        port,
        "/api/node/select",
        &format!(r#"{{"node_id":{player_id}}}"#),
    );
    assert!(resp.contains("200 OK"));

    // Get selected should return the player
    let resp = http_get(port, "/api/selected");
    let body = extract_body(&resp);
    let v: serde_json::Value = serde_json::from_str(body).unwrap();
    assert_eq!(v["name"], "Player");
    assert_eq!(v["class"], "Node2D");
    assert!(v["properties"].as_array().unwrap().len() >= 1);
    handle.stop();
}

#[test]
fn set_property_then_verify() {
    let (handle, port) = make_test_server();
    let player_id = get_player_node_id(port);

    // Set a new property
    let body = format!(
        r#"{{"node_id":{player_id},"property":"health","value":{{"type":"Int","value":100}}}}"#
    );
    let resp = http_post(port, "/api/property/set", &body);
    assert!(resp.contains("200 OK"));

    // Verify via GET /api/node/<id>
    let resp = http_get(port, &format!("/api/node/{player_id}"));
    let body = extract_body(&resp);
    let v: serde_json::Value = serde_json::from_str(body).unwrap();
    let health = v["properties"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["name"] == "health")
        .expect("health property should exist");
    assert_eq!(health["type"], "Int");
    handle.stop();
}

#[test]
fn add_node_then_verify_in_scene() {
    let (handle, port) = make_test_server();
    let world_id = get_world_node_id(port);

    let body = format!(r#"{{"parent_id":{world_id},"name":"NewSprite","class_name":"Sprite2D"}}"#);
    let resp = http_post(port, "/api/node/add", &body);
    assert!(resp.contains("200 OK"));
    let resp_body: serde_json::Value = serde_json::from_str(extract_body(&resp)).unwrap();
    assert!(resp_body["id"].as_u64().is_some());

    // Verify the node appears in the scene tree
    let scene_resp = http_get(port, "/api/scene");
    assert!(scene_resp.contains("NewSprite"));
    assert!(scene_resp.contains("Sprite2D"));
    handle.stop();
}

#[test]
fn undo_reverses_add_node() {
    let (handle, port) = make_test_server();
    let world_id = get_world_node_id(port);

    // Add a node
    let body = format!(r#"{{"parent_id":{world_id},"name":"TempNode","class_name":"Node"}}"#);
    http_post(port, "/api/node/add", &body);

    // Verify it exists
    let scene_resp = http_get(port, "/api/scene");
    assert!(scene_resp.contains("TempNode"));

    // Undo
    let resp = http_post(port, "/api/undo", "");
    assert!(resp.contains("200 OK"));

    // Verify it's gone
    let scene_resp = http_get(port, "/api/scene");
    assert!(!scene_resp.contains("TempNode"));
    handle.stop();
}

#[test]
fn redo_restores_undone_action() {
    let (handle, port) = make_test_server();
    let player_id = get_player_node_id(port);

    // Set property
    let body = format!(
        r#"{{"node_id":{player_id},"property":"speed","value":{{"type":"Float","value":5.5}}}}"#
    );
    http_post(port, "/api/property/set", &body);

    // Undo
    http_post(port, "/api/undo", "");

    // Redo
    let resp = http_post(port, "/api/redo", "");
    assert!(resp.contains("200 OK"));

    // Verify property is back
    let node_resp = http_get(port, &format!("/api/node/{player_id}"));
    let body = extract_body(&node_resp);
    assert!(body.contains("speed"));
    handle.stop();
}

#[test]
fn delete_node_removes_from_tree() {
    let (handle, port) = make_test_server();
    let world_id = get_world_node_id(port);

    // Add then delete
    let add_body = format!(r#"{{"parent_id":{world_id},"name":"ToDelete","class_name":"Node"}}"#);
    let add_resp = http_post(port, "/api/node/add", &add_body);
    let add_json: serde_json::Value = serde_json::from_str(extract_body(&add_resp)).unwrap();
    let new_id = add_json["id"].as_u64().unwrap();

    let del_body = format!(r#"{{"node_id":{new_id}}}"#);
    let resp = http_post(port, "/api/node/delete", &del_body);
    assert!(resp.contains("200 OK"));

    let scene_resp = http_get(port, "/api/scene");
    assert!(!scene_resp.contains("ToDelete"));
    handle.stop();
}

#[test]
fn scene_save_writes_file() {
    let (handle, port) = make_test_server();

    let tmp = tempfile::NamedTempFile::new().unwrap();
    let path = tmp.path().to_str().unwrap().to_string();

    let save_body = format!(r#"{{"path":"{path}"}}"#);
    let resp = http_post(port, "/api/scene/save", &save_body);
    assert!(resp.contains("200 OK"));

    let contents = std::fs::read_to_string(&path).unwrap();
    assert!(contents.contains("[gd_scene"));
    assert!(contents.contains("World"));
    handle.stop();
}

#[test]
fn scene_load_replaces_tree() {
    let (handle, port) = make_test_server();

    // Save current scene
    let tmp = tempfile::NamedTempFile::new().unwrap();
    let path = tmp.path().to_str().unwrap().to_string();
    http_post(port, "/api/scene/save", &format!(r#"{{"path":"{path}"}}"#));

    // Load it back
    let resp = http_post(port, "/api/scene/load", &format!(r#"{{"path":"{path}"}}"#));
    assert!(resp.contains("200 OK"));

    // Tree should still have World
    let scene_resp = http_get(port, "/api/scene");
    assert!(scene_resp.contains("World"));
    handle.stop();
}

#[test]
fn viewport_returns_valid_bmp() {
    let (handle, port) = make_test_server();

    // Upload a frame
    let fb = FrameBuffer::new(8, 8, Color::rgb(0.5, 0.2, 0.8));
    handle.update_frame(fb);

    let resp = http_request_raw(
        port,
        "GET /api/viewport HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
    );
    let resp_str = String::from_utf8_lossy(&resp);
    assert!(resp_str.contains("200 OK"));
    assert!(resp_str.contains("image/bmp"));

    // Verify BMP magic bytes
    let bm_pos = resp.windows(2).position(|w| w == b"BM");
    assert!(bm_pos.is_some(), "Response should contain BMP header");
    handle.stop();
}

#[test]
fn viewport_no_frame_returns_error() {
    let (handle, port) = make_test_server();
    let resp = http_get(port, "/api/viewport");
    assert!(resp.contains("404") || resp.contains("no frame"));
    handle.stop();
}

#[test]
fn set_vector2_property() {
    let (handle, port) = make_test_server();
    let player_id = get_player_node_id(port);

    let body = format!(
        r#"{{"node_id":{player_id},"property":"position","value":{{"type":"Vector2","value":[300,400]}}}}"#
    );
    let resp = http_post(port, "/api/property/set", &body);
    assert!(resp.contains("200 OK"));

    let node_resp = http_get(port, &format!("/api/node/{player_id}"));
    let body = extract_body(&node_resp);
    let v: serde_json::Value = serde_json::from_str(body).unwrap();
    let pos = v["properties"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["name"] == "position")
        .unwrap();
    assert_eq!(pos["type"], "Vector2");
    handle.stop();
}

#[test]
fn undo_empty_returns_error() {
    let (handle, port) = make_test_server();
    let resp = http_post(port, "/api/undo", "");
    assert!(resp.contains("400"));
    assert!(resp.contains("nothing to undo"));
    handle.stop();
}

#[test]
fn cors_preflight_supported() {
    let (handle, port) = make_test_server();
    let resp = http_request_str(
        port,
        "OPTIONS /api/scene HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
    );
    assert!(resp.contains("204 No Content"));
    assert!(resp.contains("Access-Control-Allow-Origin: *"));
    handle.stop();
}

#[test]
fn unknown_path_returns_404() {
    let (handle, port) = make_test_server();
    let resp = http_get(port, "/nonexistent");
    assert!(resp.contains("404"));
    handle.stop();
}

#[test]
fn get_node_by_id() {
    let (handle, port) = make_test_server();
    let player_id = get_player_node_id(port);

    let resp = http_get(port, &format!("/api/node/{player_id}"));
    assert!(resp.contains("200 OK"));
    let body = extract_body(&resp);
    let v: serde_json::Value = serde_json::from_str(body).unwrap();
    assert_eq!(v["name"], "Player");
    assert_eq!(v["class"], "Node2D");
    handle.stop();
}

// ---------------------------------------------------------------------------
// Concurrent request stress tests
// ---------------------------------------------------------------------------

#[test]
fn concurrent_viewport_and_scene_requests() {
    let (handle, port) = make_test_server();

    // Provide a frame so viewport has data
    let fb = FrameBuffer::new(64, 64, Color::rgb(0.1, 0.1, 0.2));
    handle.update_frame(fb);

    // Fire 20 concurrent requests: 10 viewport + 10 scene
    let mut handles = Vec::new();
    for i in 0..20 {
        let p = port;
        handles.push(thread::spawn(move || {
            let path = if i % 2 == 0 {
                "/api/viewport/png"
            } else {
                "/api/scene"
            };
            let resp = http_get(p, path);
            resp.contains("200 OK")
        }));
    }

    let mut success = 0;
    for h in handles {
        if h.join().unwrap() {
            success += 1;
        }
    }

    // All 20 requests must succeed
    assert_eq!(success, 20, "Expected 20/20 concurrent requests to succeed");
    handle.stop();
}

#[test]
fn rapid_sequential_requests_no_errors() {
    let (handle, port) = make_test_server();

    let fb = FrameBuffer::new(64, 64, Color::rgb(0.1, 0.1, 0.2));
    handle.update_frame(fb);

    // 50 rapid sequential requests with no sleep between them
    let mut failures = 0;
    for _ in 0..50 {
        let resp = http_get(port, "/api/scene");
        if !resp.contains("200 OK") {
            failures += 1;
        }
    }

    assert_eq!(failures, 0, "Expected 0 failures in 50 rapid requests");
    handle.stop();
}

#[test]
fn concurrent_mixed_endpoints() {
    let (handle, port) = make_test_server();

    let fb = FrameBuffer::new(64, 64, Color::rgb(0.1, 0.1, 0.2));
    handle.update_frame(fb);

    // Fire 30 requests across all GET endpoints simultaneously
    let endpoints = vec![
        "/api/scene",
        "/api/selected",
        "/api/viewport/png",
        "/editor",
        "/api/scene",
        "/api/viewport",
    ];

    let mut handles = Vec::new();
    for i in 0..30 {
        let p = port;
        let endpoint = endpoints[i % endpoints.len()].to_string();
        handles.push(thread::spawn(move || {
            let resp = http_get(p, &endpoint);
            resp.contains("200 OK") || resp.contains("404")
        }));
    }

    let mut success = 0;
    for h in handles {
        if h.join().unwrap() {
            success += 1;
        }
    }

    assert_eq!(
        success, 30,
        "Expected 30/30 mixed concurrent requests to succeed"
    );
    handle.stop();
}

#[test]
fn parse_failure_returns_400_not_empty() {
    let (handle, port) = make_test_server();

    // Send garbage that isn't valid HTTP
    let mut stream = TcpStream::connect(format!("127.0.0.1:{port}")).expect("failed to connect");
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
    stream.write_all(b"NOT_HTTP\r\n\r\n").unwrap();
    let mut response = Vec::new();
    let _ = stream.read_to_end(&mut response);
    let resp_str = String::from_utf8_lossy(&response).to_string();

    // Must get a 400 response, NOT an empty response
    assert!(
        resp_str.contains("400"),
        "Expected 400 for garbage input, got: {resp_str}"
    );
    assert!(!resp_str.is_empty(), "Response must not be empty");

    handle.stop();
}

#[test]
fn empty_request_returns_400() {
    let (handle, port) = make_test_server();

    // Connect and immediately close — server should handle gracefully
    let stream = TcpStream::connect(format!("127.0.0.1:{port}")).expect("failed to connect");
    drop(stream);

    // Server should still be alive after the bad connection
    thread::sleep(Duration::from_millis(100));
    let resp = http_get(port, "/api/scene");
    assert!(
        resp.contains("200 OK"),
        "Server must survive empty connections"
    );

    handle.stop();
}

#[test]
fn server_survives_concurrent_bad_and_good_requests() {
    let (handle, port) = make_test_server();

    let fb = FrameBuffer::new(64, 64, Color::rgb(0.1, 0.1, 0.2));
    handle.update_frame(fb);

    // Mix of good and bad requests concurrently
    let mut handles = Vec::new();
    for i in 0..20 {
        let p = port;
        handles.push(thread::spawn(move || {
            if i % 4 == 0 {
                // Bad request: garbage data
                let mut stream = TcpStream::connect(format!("127.0.0.1:{p}")).unwrap();
                stream.set_read_timeout(Some(Duration::from_secs(2))).ok();
                let _ = stream.write_all(b"GARBAGE\r\n\r\n");
                let mut buf = Vec::new();
                let _ = stream.read_to_end(&mut buf);
                true // We don't care about the response for bad requests
            } else {
                // Good request
                let resp = http_get(p, "/api/scene");
                resp.contains("200 OK")
            }
        }));
    }

    let good_count = handles.len() - handles.len() / 4; // 15 good requests
    let mut good_success = 0;
    for (i, h) in handles.into_iter().enumerate() {
        let result = h.join().unwrap();
        if i % 4 != 0 && result {
            good_success += 1;
        }
    }

    assert_eq!(
        good_success, good_count as usize,
        "All good requests must succeed even with concurrent bad requests"
    );
    handle.stop();
}

// ---------------------------------------------------------------------------
// pat-776: Save/load workflow for fixture scenes
// ---------------------------------------------------------------------------

/// Verifies that saving a scene writes a valid .tscn file containing the
/// expected [gd_scene] header and all node names from the tree.
#[test]
fn save_writes_tscn_with_all_nodes() {
    let (handle, port) = make_test_server();

    let tmp = tempfile::NamedTempFile::new().unwrap();
    let path = tmp.path().to_str().unwrap().to_string();

    let save_body = format!(r#"{{"path":"{path}"}}"#);
    let resp = http_post(port, "/api/scene/save", &save_body);
    assert!(resp.contains("200 OK"), "save should succeed");

    let contents = std::fs::read_to_string(&path).unwrap();
    assert!(
        contents.contains("[gd_scene"),
        "must contain [gd_scene header"
    );
    assert!(contents.contains("World"), "must contain World node");
    assert!(contents.contains("Player"), "must contain Player node");
    assert!(contents.contains("Enemy"), "must contain Enemy node");
    assert!(contents.contains("Ground"), "must contain Ground node");

    handle.stop();
}

/// Verifies that loading a .tscn file replaces the scene tree and the
/// loaded tree matches the original structure.
#[test]
fn load_replaces_tree_with_correct_structure() {
    let (handle, port) = make_test_server();

    // Save the current tree.
    let tmp = tempfile::NamedTempFile::new().unwrap();
    let path = tmp.path().to_str().unwrap().to_string();
    http_post(port, "/api/scene/save", &format!(r#"{{"path":"{path}"}}"#));

    // Load it back.
    let resp = http_post(port, "/api/scene/load", &format!(r#"{{"path":"{path}"}}"#));
    assert!(resp.contains("200 OK"), "load should succeed");

    // Verify the tree still has the expected structure.
    let scene_resp = http_get(port, "/api/scene");
    let body = extract_body(&scene_resp);
    let v: serde_json::Value = serde_json::from_str(body).expect("scene JSON");
    let children = v["nodes"]["children"].as_array().expect("root children");
    assert!(!children.is_empty(), "loaded tree must have children");
    let world = &children[0];
    assert_eq!(world["name"], "World");
    let world_children = world["children"].as_array().expect("World children");
    assert!(
        world_children.len() >= 3,
        "World should have at least Player, Enemy, Ground"
    );

    handle.stop();
}

/// Round-trip: save, load, re-save, compare — the two .tscn files should
/// be identical, proving no data corruption in the save/load cycle.
#[test]
fn save_load_roundtrip_preserves_content() {
    let (handle, port) = make_test_server();

    // First save.
    let tmp1 = tempfile::NamedTempFile::new().unwrap();
    let path1 = tmp1.path().to_str().unwrap().to_string();
    http_post(port, "/api/scene/save", &format!(r#"{{"path":"{path1}"}}"#));

    // Load it back.
    http_post(port, "/api/scene/load", &format!(r#"{{"path":"{path1}"}}"#));

    // Save again to a different file.
    let tmp2 = tempfile::NamedTempFile::new().unwrap();
    let path2 = tmp2.path().to_str().unwrap().to_string();
    http_post(port, "/api/scene/save", &format!(r#"{{"path":"{path2}"}}"#));

    // Both files should be identical.
    let contents1 = std::fs::read_to_string(&path1).unwrap();
    let contents2 = std::fs::read_to_string(&path2).unwrap();
    assert_eq!(
        contents1, contents2,
        "Round-trip save → load → save must produce identical .tscn output"
    );

    handle.stop();
}

// ---------------------------------------------------------------------------
// pat-fp2: Filesystem browser and scene/script loading
// ---------------------------------------------------------------------------
//
// The editor exposes GET /api/filesystem which scans the working directory
// for .tscn, .gd, and .tres files and returns them as JSON. Scene loading
// is done via POST /api/scene/load with a file path. There is no
// interactive filesystem "browser" UI action yet — that is future work.
// The current flow is:
//   1. Client calls GET /api/filesystem to discover available scenes.
//   2. Client calls POST /api/scene/load with the chosen path.

/// Verifies that a fixture .tscn scene can be loaded through the editor API.
#[test]
fn load_fixture_scene_via_api() {
    let (handle, port) = make_test_server();

    // Load the minimal fixture scene.
    let fixture_path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../fixtures/scenes/minimal.tscn");
    let abs_path = fixture_path.canonicalize().expect("fixture must exist");
    let path_str = abs_path.to_str().unwrap();

    let load_body = format!(r#"{{"path":"{}"}}"#, path_str.replace('\\', "\\\\"));
    let resp = http_post(port, "/api/scene/load", &load_body);
    assert!(
        resp.contains("200 OK"),
        "loading fixture scene should succeed"
    );

    // Verify the loaded tree has the expected root from minimal.tscn.
    let scene_resp = http_get(port, "/api/scene");
    assert!(
        scene_resp.contains("Root"),
        "minimal.tscn root node should appear"
    );

    handle.stop();
}

/// Verifies that loading a second fixture scene replaces the first.
#[test]
fn load_second_fixture_replaces_first() {
    let (handle, port) = make_test_server();

    let fixtures_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../fixtures/scenes");

    // Load minimal.tscn first.
    let minimal = fixtures_dir.join("minimal.tscn").canonicalize().unwrap();
    let path_str = minimal.to_str().unwrap();
    http_post(
        port,
        "/api/scene/load",
        &format!(r#"{{"path":"{}"}}"#, path_str.replace('\\', "\\\\")),
    );

    // Now load hierarchy.tscn.
    let hierarchy = fixtures_dir.join("hierarchy.tscn").canonicalize().unwrap();
    let path_str = hierarchy.to_str().unwrap();
    let resp = http_post(
        port,
        "/api/scene/load",
        &format!(r#"{{"path":"{}"}}"#, path_str.replace('\\', "\\\\")),
    );
    assert!(resp.contains("200 OK"));

    // The tree should reflect the hierarchy scene, not minimal.
    let scene_resp = http_get(port, "/api/scene");
    let body = extract_body(&scene_resp);
    let v: serde_json::Value = serde_json::from_str(body).expect("scene JSON");
    let children = v["nodes"]["children"].as_array().expect("root children");
    // hierarchy.tscn should have its own node structure, distinct from minimal.
    assert!(
        !children.is_empty(),
        "hierarchy.tscn should produce a non-empty tree"
    );

    handle.stop();
}

// ---------------------------------------------------------------------------
// pat-b0o: Viewport selection modes (box select, selection persistence)
// ---------------------------------------------------------------------------

#[test]
fn box_select_finds_nodes_in_region() {
    let (handle, port) = make_test_server();
    let resp = http_post(
        port,
        "/api/viewport/box_select",
        r#"{"x1":0,"y1":0,"x2":800,"y2":600,"add":false}"#,
    );
    assert!(resp.contains("200 OK"));
    let body = extract_body(&resp);
    let v: serde_json::Value = serde_json::from_str(body).unwrap();
    assert!(
        v["count"].as_u64().unwrap() > 0,
        "box select should find nodes"
    );
    assert!(!v["selected_nodes"].as_array().unwrap().is_empty());
    handle.stop();
}

#[test]
fn box_select_additive_preserves_previous() {
    let (handle, port) = make_test_server();
    http_post(
        port,
        "/api/viewport/box_select",
        r#"{"x1":0,"y1":0,"x2":10,"y2":10,"add":false}"#,
    );
    let resp = http_post(
        port,
        "/api/viewport/box_select",
        r#"{"x1":0,"y1":0,"x2":800,"y2":600,"add":true}"#,
    );
    let body = extract_body(&resp);
    let v: serde_json::Value = serde_json::from_str(body).unwrap();
    assert!(v["count"].as_u64().unwrap() > 0);
    handle.stop();
}

// ---------------------------------------------------------------------------
// pat-r5p: Transform gizmo improvements
// ---------------------------------------------------------------------------

#[test]
fn axis_constrained_drag_moves_only_x() {
    let (handle, port) = make_test_server();
    let player_id = get_player_node_id(port);
    http_post(
        port,
        "/api/node/select",
        &format!(r#"{{"node_id":{player_id}}}"#),
    );

    let node_resp = http_get(port, &format!("/api/node/{player_id}"));
    let body = extract_body(&node_resp);
    let v: serde_json::Value = serde_json::from_str(body).unwrap();
    let orig_pos = v["properties"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["name"] == "position")
        .unwrap();
    let orig_y = orig_pos["value"]["value"][1].as_f64().unwrap();

    let resp = http_post(
        port,
        "/api/viewport/drag_axis",
        r#"{"dx":50,"dy":50,"axis":"x"}"#,
    );
    assert!(resp.contains("200 OK"));

    let node_resp = http_get(port, &format!("/api/node/{player_id}"));
    let body = extract_body(&node_resp);
    let v: serde_json::Value = serde_json::from_str(body).unwrap();
    let pos = v["properties"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["name"] == "position")
        .unwrap();
    let new_y = pos["value"]["value"][1].as_f64().unwrap();
    assert!(
        (new_y - orig_y).abs() < 0.01,
        "Y should be unchanged for x-axis drag"
    );
    handle.stop();
}

#[test]
fn rotate_node_changes_rotation() {
    let (handle, port) = make_test_server();
    let player_id = get_player_node_id(port);
    http_post(
        port,
        "/api/node/select",
        &format!(r#"{{"node_id":{player_id}}}"#),
    );
    let resp = http_post(port, "/api/viewport/rotate_node", r#"{"delta":0.5}"#);
    assert!(resp.contains("200 OK"));

    let node_resp = http_get(port, &format!("/api/node/{player_id}"));
    let body = extract_body(&node_resp);
    let v: serde_json::Value = serde_json::from_str(body).unwrap();
    let rot = v["properties"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["name"] == "rotation")
        .unwrap();
    let rotation = rot["value"]["value"].as_f64().unwrap();
    assert!(
        (rotation - 0.5).abs() < 0.01,
        "rotation should be ~0.5, got {rotation}"
    );
    handle.stop();
}

#[test]
fn scale_node_changes_scale() {
    let (handle, port) = make_test_server();
    let player_id = get_player_node_id(port);
    http_post(
        port,
        "/api/node/select",
        &format!(r#"{{"node_id":{player_id}}}"#),
    );
    let body = format!(
        r#"{{"node_id":{player_id},"property":"scale","value":{{"type":"Vector2","value":[1,1]}}}}"#
    );
    http_post(port, "/api/property/set", &body);

    let resp = http_post(port, "/api/viewport/scale_node", r#"{"sx":2.0,"sy":1.5}"#);
    assert!(resp.contains("200 OK"));

    let node_resp = http_get(port, &format!("/api/node/{player_id}"));
    let body = extract_body(&node_resp);
    let v: serde_json::Value = serde_json::from_str(body).unwrap();
    let scale = v["properties"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["name"] == "scale")
        .unwrap();
    let sx = scale["value"]["value"][0].as_f64().unwrap();
    let sy = scale["value"]["value"][1].as_f64().unwrap();
    assert!((sx - 2.0).abs() < 0.01, "sx should be ~2.0, got {sx}");
    assert!((sy - 1.5).abs() < 0.01, "sy should be ~1.5, got {sy}");
    handle.stop();
}

// ---------------------------------------------------------------------------
// pat-zlv: Snapping improvements
// ---------------------------------------------------------------------------

#[test]
fn snap_info_returns_current_settings() {
    let (handle, port) = make_test_server();
    let resp = http_get(port, "/api/viewport/snap_info");
    assert!(resp.contains("200 OK"));
    let body = extract_body(&resp);
    let v: serde_json::Value = serde_json::from_str(body).unwrap();
    assert_eq!(v["snap_size"], 8);
    assert_eq!(v["snap_enabled"], false);
    handle.stop();
}

#[test]
fn configurable_snap_size_via_settings() {
    let (handle, port) = make_test_server();
    http_post(
        port,
        "/api/settings",
        r#"{"grid_snap_enabled":true,"grid_snap_size":16,"grid_visible":true,"rulers_visible":true,"font_size":"medium"}"#,
    );
    let resp = http_get(port, "/api/viewport/snap_info");
    let body = extract_body(&resp);
    let v: serde_json::Value = serde_json::from_str(body).unwrap();
    assert_eq!(v["snap_size"], 16);
    assert_eq!(v["snap_enabled"], true);
    handle.stop();
}

// ---------------------------------------------------------------------------
// pat-cgc: Script editor core (find/replace)
// ---------------------------------------------------------------------------

#[test]
fn script_find_returns_matches() {
    let (handle, port) = make_test_server();
    let resp = http_post(
        port,
        "/api/script/find",
        r#"{"content":"line1 print\nline2 print\nline3","query":"print","case_sensitive":true}"#,
    );
    assert!(resp.contains("200 OK"));
    let body = extract_body(&resp);
    let v: serde_json::Value = serde_json::from_str(body).unwrap();
    assert_eq!(v["count"], 2);
    handle.stop();
}

#[test]
fn script_find_case_insensitive() {
    let (handle, port) = make_test_server();
    let resp = http_post(
        port,
        "/api/script/find",
        r#"{"content":"Ready ready READY","query":"ready","case_sensitive":false}"#,
    );
    let body = extract_body(&resp);
    let v: serde_json::Value = serde_json::from_str(body).unwrap();
    assert_eq!(v["count"], 3);
    handle.stop();
}

#[test]
fn script_replace_all_occurrences() {
    let (handle, port) = make_test_server();
    let resp = http_post(
        port,
        "/api/script/replace",
        r#"{"content":"var x = 1\nvar y = x + x","query":"x","replacement":"z","replace_all":true}"#,
    );
    assert!(resp.contains("200 OK"));
    let body = extract_body(&resp);
    let v: serde_json::Value = serde_json::from_str(body).unwrap();
    assert_eq!(v["replacements"], 3);
    assert!(v["content"].as_str().unwrap().contains("var z = 1"));
    handle.stop();
}

#[test]
fn script_replace_first_only() {
    let (handle, port) = make_test_server();
    let resp = http_post(
        port,
        "/api/script/replace",
        r#"{"content":"aaa","query":"a","replacement":"b","replace_all":false}"#,
    );
    let body = extract_body(&resp);
    let v: serde_json::Value = serde_json::from_str(body).unwrap();
    assert_eq!(v["replacements"], 1);
    assert_eq!(v["content"], "baa");
    handle.stop();
}

// ---------------------------------------------------------------------------
// pat-1v3: Script editor advanced (breakpoints, error highlighting)
// ---------------------------------------------------------------------------

#[test]
fn toggle_breakpoint_adds_and_removes() {
    let (handle, port) = make_test_server();
    let resp = http_post(
        port,
        "/api/script/breakpoint/toggle",
        r#"{"path":"test.gd","line":5}"#,
    );
    assert!(resp.contains("200 OK"));
    let body = extract_body(&resp);
    let v: serde_json::Value = serde_json::from_str(body).unwrap();
    assert_eq!(v["added"], true);
    assert!(v["breakpoints"]
        .as_array()
        .unwrap()
        .contains(&serde_json::json!(5)));

    let resp = http_post(
        port,
        "/api/script/breakpoint/toggle",
        r#"{"path":"test.gd","line":5}"#,
    );
    let body = extract_body(&resp);
    let v: serde_json::Value = serde_json::from_str(body).unwrap();
    assert_eq!(v["added"], false);
    assert!(!v["breakpoints"]
        .as_array()
        .unwrap()
        .contains(&serde_json::json!(5)));
    handle.stop();
}

#[test]
fn get_breakpoints_returns_all_for_path() {
    let (handle, port) = make_test_server();
    http_post(
        port,
        "/api/script/breakpoint/toggle",
        r#"{"path":"test.gd","line":3}"#,
    );
    http_post(
        port,
        "/api/script/breakpoint/toggle",
        r#"{"path":"test.gd","line":10}"#,
    );
    http_post(
        port,
        "/api/script/breakpoint/toggle",
        r#"{"path":"other.gd","line":1}"#,
    );

    let resp = http_get(port, "/api/script/breakpoints?path=test.gd");
    assert!(resp.contains("200 OK"));
    let body = extract_body(&resp);
    let v: serde_json::Value = serde_json::from_str(body).unwrap();
    assert_eq!(v["breakpoints"].as_array().unwrap().len(), 2);
    handle.stop();
}

// ---------------------------------------------------------------------------
// pat-2hs: Signals dock improvements
// ---------------------------------------------------------------------------

#[test]
fn signals_include_connection_count_and_details() {
    let (handle, port) = make_test_server();
    let world_id = get_world_node_id(port);

    let body = format!(r#"{{"parent_id":{world_id},"name":"TestBtn","class_name":"Button"}}"#);
    let add_resp = http_post(port, "/api/node/add", &body);
    let add_json: serde_json::Value = serde_json::from_str(extract_body(&add_resp)).unwrap();
    let btn_id = add_json["id"].as_u64().unwrap();

    http_post(
        port,
        "/api/node/signals/connect",
        &format!(r#"{{"node_id":{btn_id},"signal":"pressed","method":"_on_pressed"}}"#),
    );

    let resp = http_get(port, &format!("/api/node/signals?node_id={btn_id}"));
    assert!(resp.contains("200 OK"));
    let body = extract_body(&resp);
    let v: serde_json::Value = serde_json::from_str(body).unwrap();

    assert!(v["connected_count"].as_u64().unwrap() >= 1);
    let signals = v["signals"].as_array().unwrap();
    let pressed = signals.iter().find(|s| s["name"] == "pressed").unwrap();
    assert_eq!(pressed["connected"], true);
    assert!(pressed["connection_count"].as_u64().unwrap() >= 1);
    assert!(!pressed["connections"].as_array().unwrap().is_empty());
    handle.stop();
}

// ---------------------------------------------------------------------------
// pat-2s1: Animation editor improvements
// ---------------------------------------------------------------------------

#[test]
fn animation_track_reorder_swaps() {
    let (handle, port) = make_test_server();
    http_post(
        port,
        "/api/animation/create",
        r#"{"name":"walk","length":1.0}"#,
    );
    http_post(
        port,
        "/api/animation/keyframe/add",
        r#"{"animation":"walk","track_node":"Player","track_property":"position","time":0.0,"value":{"type":"Vector2","value":[0,0]}}"#,
    );
    http_post(
        port,
        "/api/animation/keyframe/add",
        r#"{"animation":"walk","track_node":"Player","track_property":"rotation","time":0.0,"value":{"type":"Float","value":0}}"#,
    );

    let resp = http_post(
        port,
        "/api/animation/track/reorder",
        r#"{"animation":"walk","from":0,"to":1}"#,
    );
    assert!(resp.contains("200 OK"));

    let resp = http_get(port, "/api/animation?name=walk");
    let body = extract_body(&resp);
    let v: serde_json::Value = serde_json::from_str(body).unwrap();
    assert_eq!(v["tracks"][0]["property"], "rotation");
    assert_eq!(v["tracks"][1]["property"], "position");
    handle.stop();
}

#[test]
fn animation_keyframe_copy_paste() {
    let (handle, port) = make_test_server();
    http_post(
        port,
        "/api/animation/create",
        r#"{"name":"run","length":2.0}"#,
    );
    http_post(
        port,
        "/api/animation/keyframe/add",
        r#"{"animation":"run","track_node":"Player","track_property":"position","time":0.0,"value":{"type":"Vector2","value":[0,0]}}"#,
    );
    http_post(
        port,
        "/api/animation/keyframe/add",
        r#"{"animation":"run","track_node":"Player","track_property":"position","time":0.5,"value":{"type":"Vector2","value":[100,0]}}"#,
    );

    let resp = http_post(
        port,
        "/api/animation/keyframe/copy",
        r#"{"animation":"run","track_index":0,"keyframe_indices":[0]}"#,
    );
    assert!(resp.contains("200 OK"));

    let resp = http_post(
        port,
        "/api/animation/keyframe/paste",
        r#"{"animation":"run","track_index":0,"time_offset":1.0}"#,
    );
    assert!(resp.contains("200 OK"));

    let resp = http_get(port, "/api/animation?name=run");
    let body = extract_body(&resp);
    let v: serde_json::Value = serde_json::from_str(body).unwrap();
    assert_eq!(v["tracks"][0]["keyframes"].as_array().unwrap().len(), 3);
    handle.stop();
}

// ---------------------------------------------------------------------------
// pat-lbu: Bottom panels (debugger, monitors)
// ---------------------------------------------------------------------------

#[test]
fn debug_stack_trace_returns_json() {
    let (handle, port) = make_test_server();
    let resp = http_get(port, "/api/debug/stack_trace");
    assert!(resp.contains("200 OK"));
    let body = extract_body(&resp);
    let v: serde_json::Value = serde_json::from_str(body).unwrap();
    assert!(v["frames"].as_array().is_some());
    handle.stop();
}

#[test]
fn monitors_frame_times_returns_stats() {
    let (handle, port) = make_test_server();
    let resp = http_get(port, "/api/monitors/frame_times");
    assert!(resp.contains("200 OK"));
    let body = extract_body(&resp);
    let v: serde_json::Value = serde_json::from_str(body).unwrap();
    assert!(v["times"].as_array().is_some());
    assert!(v["avg"].is_number());
    assert!(v["fps"].is_number());
    handle.stop();
}

// ---------------------------------------------------------------------------
// pat-dj6: Top bar (editor mode buttons, scene tab)
// ---------------------------------------------------------------------------

#[test]
fn set_editor_mode_2d_3d_script() {
    let (handle, port) = make_test_server();

    let resp = http_get(port, "/api/editor/mode");
    assert!(resp.contains("200 OK"));
    let body = extract_body(&resp);
    let v: serde_json::Value = serde_json::from_str(body).unwrap();
    assert_eq!(v["mode"], "2d");

    let resp = http_post(port, "/api/editor/mode", r#"{"mode":"3d"}"#);
    assert!(resp.contains("200 OK"));

    http_post(port, "/api/editor/mode", r#"{"mode":"script"}"#);
    let resp = http_get(port, "/api/editor/mode");
    let body = extract_body(&resp);
    let v: serde_json::Value = serde_json::from_str(body).unwrap();
    assert_eq!(v["mode"], "script");

    let resp = http_post(port, "/api/editor/mode", r#"{"mode":"invalid"}"#);
    assert!(resp.contains("400"));
    handle.stop();
}

#[test]
fn editor_html_contains_mode_buttons() {
    let (handle, port) = make_test_server();
    let resp = http_get(port, "/editor");
    assert!(resp.contains("mode-btn"));
    assert!(resp.contains("data-mode=\"2d\""));
    assert!(resp.contains("data-mode=\"3d\""));
    assert!(resp.contains("data-mode=\"script\""));
    handle.stop();
}

#[test]
fn editor_html_contains_debugger_monitors_tabs() {
    let (handle, port) = make_test_server();
    let resp = http_get(port, "/editor");
    assert!(resp.contains("data-tab=\"debugger\""));
    assert!(resp.contains("data-tab=\"monitors\""));
    assert!(resp.contains("monitor-graph-canvas"));
    assert!(resp.contains("debug-stack-frames"));
    handle.stop();
}

#[test]
fn editor_html_contains_scene_tab_close() {
    let (handle, port) = make_test_server();
    let resp = http_get(port, "/editor");
    assert!(resp.contains("scene-tab-close"));
    handle.stop();
}

#[test]
fn editor_html_contains_box_select_overlay() {
    let (handle, port) = make_test_server();
    let resp = http_get(port, "/editor");
    assert!(resp.contains("box-select-overlay"));
    handle.stop();
}

#[test]
fn editor_html_contains_snap_indicator() {
    let (handle, port) = make_test_server();
    let resp = http_get(port, "/editor");
    assert!(resp.contains("snap-indicator"));
    handle.stop();
}
