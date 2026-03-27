//! Integration tests for the 2D viewport pan, zoom, and grid functionality.
//!
//! Exercises the editor server API endpoints for viewport state management:
//! zoom level, pan offset, grid settings, and the rendered viewport output.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;
use std::time::Duration;

use gdcore::math::Vector2;
use gdeditor::editor_server::{EditorServerHandle, EditorState};
use gdeditor::scene_renderer;
use gdscene::node::Node;
use gdscene::SceneTree;
use gdvariant::Variant;

fn free_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.local_addr().unwrap().port()
}

fn make_viewport_server() -> (EditorServerHandle, u16) {
    let port = free_port();
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let mut world = Node::new("World", "Node2D");
    world.set_property("name", Variant::String("World".into()));
    let world_id = tree.add_child(root, world).unwrap();

    let mut sprite = Node::new("Player", "Sprite2D");
    sprite.set_property("position", Variant::Vector2(Vector2::new(100.0, 50.0)));
    tree.add_child(world_id, sprite).unwrap();

    let mut camera = Node::new("Camera", "Camera2D");
    camera.set_property("position", Variant::Vector2(Vector2::new(0.0, 0.0)));
    tree.add_child(world_id, camera).unwrap();

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

// ---------------------------------------------------------------------------
// Zoom API tests
// ---------------------------------------------------------------------------

#[test]
fn viewport_get_default_zoom_pan() {
    let (handle, port) = make_viewport_server();
    let resp = http_get(port, "/api/viewport/zoom_pan");
    assert!(resp.contains("200 OK"));
    let body = extract_body(&resp);
    let v: serde_json::Value = serde_json::from_str(body).unwrap();
    assert_eq!(v["zoom"], 1.0, "Default zoom should be 1.0");
    assert_eq!(v["pan_x"], 0.0, "Default pan_x should be 0");
    assert_eq!(v["pan_y"], 0.0, "Default pan_y should be 0");
    handle.stop();
}

#[test]
fn viewport_set_zoom_updates_state() {
    let (handle, port) = make_viewport_server();

    let resp = http_post(port, "/api/viewport/zoom", r#"{"zoom":2.5}"#);
    assert!(resp.contains("200 OK"));

    let resp = http_get(port, "/api/viewport/zoom_pan");
    let body = extract_body(&resp);
    let v: serde_json::Value = serde_json::from_str(body).unwrap();
    assert_eq!(v["zoom"], 2.5, "Zoom should be updated to 2.5");
    handle.stop();
}

#[test]
fn viewport_zoom_clamps_to_valid_range() {
    let (handle, port) = make_viewport_server();

    // Set zoom below minimum (0.1)
    http_post(port, "/api/viewport/zoom", r#"{"zoom":0.01}"#);
    let resp = http_get(port, "/api/viewport/zoom_pan");
    let v: serde_json::Value = serde_json::from_str(extract_body(&resp)).unwrap();
    let zoom = v["zoom"].as_f64().unwrap();
    assert!(zoom >= 0.1, "Zoom should be clamped to at least 0.1, got {zoom}");

    // Set zoom above maximum (16.0)
    http_post(port, "/api/viewport/zoom", r#"{"zoom":100.0}"#);
    let resp = http_get(port, "/api/viewport/zoom_pan");
    let v: serde_json::Value = serde_json::from_str(extract_body(&resp)).unwrap();
    let zoom = v["zoom"].as_f64().unwrap();
    assert!(zoom <= 16.0, "Zoom should be clamped to at most 16.0, got {zoom}");

    handle.stop();
}

// ---------------------------------------------------------------------------
// Pan API tests
// ---------------------------------------------------------------------------

#[test]
fn viewport_set_pan_updates_state() {
    let (handle, port) = make_viewport_server();

    let resp = http_post(port, "/api/viewport/pan", r#"{"x":150.0,"y":-75.0}"#);
    assert!(resp.contains("200 OK"));

    let resp = http_get(port, "/api/viewport/zoom_pan");
    let v: serde_json::Value = serde_json::from_str(extract_body(&resp)).unwrap();
    assert_eq!(v["pan_x"], 150.0, "Pan X should be 150");
    assert_eq!(v["pan_y"], -75.0, "Pan Y should be -75");
    handle.stop();
}

#[test]
fn viewport_zoom_and_pan_combined() {
    let (handle, port) = make_viewport_server();

    http_post(port, "/api/viewport/zoom", r#"{"zoom":3.0}"#);
    http_post(port, "/api/viewport/pan", r#"{"x":-200.0,"y":100.0}"#);

    let resp = http_get(port, "/api/viewport/zoom_pan");
    let v: serde_json::Value = serde_json::from_str(extract_body(&resp)).unwrap();
    assert_eq!(v["zoom"], 3.0);
    assert_eq!(v["pan_x"], -200.0);
    assert_eq!(v["pan_y"], 100.0);
    handle.stop();
}

// ---------------------------------------------------------------------------
// Grid settings tests
// ---------------------------------------------------------------------------

#[test]
fn viewport_grid_settings_default() {
    let (handle, port) = make_viewport_server();

    let resp = http_get(port, "/api/settings");
    assert!(resp.contains("200 OK"));
    let body = extract_body(&resp);
    let v: serde_json::Value = serde_json::from_str(body).unwrap();
    assert_eq!(v["grid_visible"], true, "Grid should be visible by default");
    assert_eq!(v["grid_snap_enabled"], false, "Snap should be off by default");
    assert_eq!(v["grid_snap_size"], 8, "Default snap size should be 8");
    handle.stop();
}

#[test]
fn viewport_grid_settings_update() {
    let (handle, port) = make_viewport_server();

    let resp = http_post(
        port,
        "/api/settings",
        r#"{"grid_visible":false,"grid_snap_enabled":true,"grid_snap_size":16}"#,
    );
    assert!(resp.contains("200 OK"));

    let resp = http_get(port, "/api/settings");
    let v: serde_json::Value = serde_json::from_str(extract_body(&resp)).unwrap();
    assert_eq!(v["grid_visible"], false, "Grid should be hidden after update");
    assert_eq!(v["grid_snap_enabled"], true, "Snap should be enabled after update");
    assert_eq!(v["grid_snap_size"], 16, "Snap size should be 16 after update");
    handle.stop();
}

// ---------------------------------------------------------------------------
// Scene renderer unit tests (zoom, pan, grid rendering)
// ---------------------------------------------------------------------------

#[test]
fn render_scene_default_produces_nonblank_framebuffer() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let mut node = Node::new("Sprite", "Sprite2D");
    node.set_property("position", Variant::Vector2(Vector2::new(50.0, 50.0)));
    tree.add_child(root, node).unwrap();

    let fb = scene_renderer::render_scene(&tree, None, 200, 200);
    // The framebuffer should have the expected dimensions.
    assert_eq!(fb.width, 200);
    assert_eq!(fb.height, 200);
    // Grid lines and nodes should make it non-uniform.
    let first_pixel = fb.pixels[0];
    let has_variation = fb.pixels.iter().any(|&px| px != first_pixel);
    assert!(has_variation, "Rendered scene should not be uniform — grid and nodes expected");
}

#[test]
fn render_scene_with_zoom_changes_output() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let mut node = Node::new("Sprite", "Sprite2D");
    node.set_property("position", Variant::Vector2(Vector2::new(50.0, 50.0)));
    tree.add_child(root, node).unwrap();

    let fb_1x = scene_renderer::render_scene_with_zoom_pan(&tree, None, 200, 200, 1.0, (0.0, 0.0));
    let fb_4x = scene_renderer::render_scene_with_zoom_pan(&tree, None, 200, 200, 4.0, (0.0, 0.0));

    // Different zoom levels should produce different pixel output.
    assert_ne!(fb_1x.pixels, fb_4x.pixels, "Zoom 1x and 4x should produce different output");
}

#[test]
fn render_scene_with_pan_changes_output() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let mut node = Node::new("Sprite", "Sprite2D");
    node.set_property("position", Variant::Vector2(Vector2::new(50.0, 50.0)));
    tree.add_child(root, node).unwrap();

    let fb_origin = scene_renderer::render_scene_with_zoom_pan(&tree, None, 200, 200, 1.0, (0.0, 0.0));
    let fb_panned = scene_renderer::render_scene_with_zoom_pan(&tree, None, 200, 200, 1.0, (100.0, 100.0));

    assert_ne!(fb_origin.pixels, fb_panned.pixels, "Pan should shift the rendered output");
}

#[test]
fn pixel_to_scene_with_zoom_pan_round_trip() {
    let tree = SceneTree::new();
    let w = 400;
    let h = 400;
    let zoom = 2.0;
    let pan = (50.0, -30.0);

    // Convert pixel to scene, then verify the inverse relationship holds.
    let scene_a = scene_renderer::pixel_to_scene_with_zoom_pan(&tree, w, h, zoom, pan, 200.0, 200.0);
    let scene_b = scene_renderer::pixel_to_scene_with_zoom_pan(&tree, w, h, zoom, pan, 210.0, 200.0);

    // 10 pixels at zoom=2 should be 5 scene units.
    let dx = (scene_b.x - scene_a.x).abs();
    assert!(
        (dx - 5.0).abs() < 0.01,
        "10 pixels at zoom 2.0 should be 5 scene units, got {dx}"
    );
}

#[test]
fn hit_test_with_zoom_pan_finds_node_at_zoom() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let mut node = Node::new("Target", "Node2D");
    node.set_property("position", Variant::Vector2(Vector2::new(50.0, 50.0)));
    tree.add_child(root, node).unwrap();

    // At default zoom, node should be hittable.
    let result = scene_renderer::hit_test_with_zoom_pan(&tree, 200, 200, 1.0, (0.0, 0.0), 100.0, 100.0);
    assert!(result.is_some(), "Should hit the node at default zoom");

    // After large pan, the same click position should miss.
    let result = scene_renderer::hit_test_with_zoom_pan(&tree, 200, 200, 1.0, (1000.0, 1000.0), 100.0, 100.0);
    assert!(result.is_none(), "Should miss after large pan offset");
}

#[test]
fn compute_scene_bounds_covers_all_nodes() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let mut a = Node::new("A", "Node2D");
    a.set_property("position", Variant::Vector2(Vector2::new(-100.0, -50.0)));
    tree.add_child(root, a).unwrap();

    let mut b = Node::new("B", "Node2D");
    b.set_property("position", Variant::Vector2(Vector2::new(200.0, 300.0)));
    tree.add_child(root, b).unwrap();

    let bounds = scene_renderer::compute_scene_bounds(&tree);
    assert!(bounds.position.x <= -100.0, "Bounds should include leftmost node");
    assert!(bounds.position.y <= -50.0, "Bounds should include topmost node");
    let right = bounds.position.x + bounds.size.x;
    let bottom = bounds.position.y + bounds.size.y;
    assert!(right >= 200.0, "Bounds should include rightmost node");
    assert!(bottom >= 300.0, "Bounds should include bottommost node");
}

// ---------------------------------------------------------------------------
// Viewport click-to-select with zoom/pan
// ---------------------------------------------------------------------------

#[test]
fn viewport_click_selects_node_through_api() {
    let (handle, port) = make_viewport_server();

    // Get the scene to find the Player node ID.
    let resp = http_get(port, "/api/scene");
    let body = extract_body(&resp);
    let v: serde_json::Value = serde_json::from_str(body).unwrap();

    // Find Player node in children.
    let children = v["nodes"]["children"].as_array().unwrap();
    let world = &children[0];
    let world_children = world["children"].as_array().unwrap();
    let player_id = world_children
        .iter()
        .find(|c| c["name"] == "Player")
        .and_then(|c| c["id"].as_u64());
    assert!(player_id.is_some(), "Player node should exist in scene tree");

    // Select the player via API.
    let select_body = format!(r#"{{"node_id":{}}}"#, player_id.unwrap());
    let resp = http_post(port, "/api/node/select", &select_body);
    assert!(resp.contains("200 OK"), "Select should succeed");

    // Verify selection.
    let resp = http_get(port, "/api/selected");
    let body = extract_body(&resp);
    assert!(
        body.contains(&player_id.unwrap().to_string()),
        "Player should be selected"
    );

    handle.stop();
}
