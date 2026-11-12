//! Tests for editor command palette with fuzzy search (pat-ugb0p).
//!
//! Verifies the command palette API endpoints: listing commands,
//! executing server-side commands, client-side command delegation,
//! error handling, and editor HTML integration.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;
use std::time::Duration;

use gdcore::math::Vector2;
use gdeditor::editor_server::{EditorServerHandle, EditorState};
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

    let mut player = Node::new("Player", "Node2D");
    player.set_property("position", Variant::Vector2(Vector2::new(10.0, 20.0)));
    tree.add_child(root, player).unwrap();

    let state = EditorState::new(tree);
    let handle = EditorServerHandle::start(port, state);
    thread::sleep(Duration::from_millis(300));
    (handle, port)
}

fn http_post(port: u16, path: &str, body: &str) -> String {
    let req = format!(
        "POST {path} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    http_request(port, &req)
}

fn http_get(port: u16, path: &str) -> String {
    let req = format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n");
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

// --- Command List Tests ---

#[test]
fn get_commands_returns_list() {
    let (handle, port) = make_test_server();

    let resp = http_get(port, "/api/commands");
    assert!(resp.contains("200 OK"), "Should return 200");
    let body = extract_body(&resp);
    let data: serde_json::Value = serde_json::from_str(body).unwrap();
    let commands = data["commands"].as_array().unwrap();
    assert!(
        commands.len() >= 20,
        "Should have at least 20 commands, got {}",
        commands.len()
    );

    // Each command should have id, label, category
    for cmd in commands {
        assert!(cmd["id"].is_string(), "Command should have id");
        assert!(cmd["label"].is_string(), "Command should have label");
        assert!(cmd["category"].is_string(), "Command should have category");
    }

    handle.stop();
}

#[test]
fn commands_include_expected_entries() {
    let (handle, port) = make_test_server();

    let resp = http_get(port, "/api/commands");
    let body = extract_body(&resp);
    let data: serde_json::Value = serde_json::from_str(body).unwrap();
    let commands = data["commands"].as_array().unwrap();

    let ids: Vec<&str> = commands.iter().map(|c| c["id"].as_str().unwrap()).collect();
    assert!(ids.contains(&"save_scene"), "Should have save_scene");
    assert!(ids.contains(&"undo"), "Should have undo");
    assert!(ids.contains(&"redo"), "Should have redo");
    assert!(ids.contains(&"add_node"), "Should have add_node");
    assert!(ids.contains(&"delete_node"), "Should have delete_node");
    assert!(ids.contains(&"zoom_in"), "Should have zoom_in");
    assert!(ids.contains(&"toggle_theme"), "Should have toggle_theme");
    assert!(ids.contains(&"play_scene"), "Should have play_scene");
    assert!(ids.contains(&"open_settings"), "Should have open_settings");

    handle.stop();
}

#[test]
fn commands_have_categories() {
    let (handle, port) = make_test_server();

    let resp = http_get(port, "/api/commands");
    let body = extract_body(&resp);
    let data: serde_json::Value = serde_json::from_str(body).unwrap();
    let commands = data["commands"].as_array().unwrap();

    let categories: std::collections::HashSet<&str> = commands
        .iter()
        .map(|c| c["category"].as_str().unwrap())
        .collect();
    assert!(categories.contains("File"), "Should have File category");
    assert!(categories.contains("Edit"), "Should have Edit category");
    assert!(categories.contains("View"), "Should have View category");
    assert!(categories.contains("Scene"), "Should have Scene category");
    assert!(categories.contains("Tool"), "Should have Tool category");

    handle.stop();
}

// --- Command Execution Tests ---

#[test]
fn execute_undo_command() {
    let (handle, port) = make_test_server();

    let resp = http_post(port, "/api/command/execute", r#"{"command":"undo"}"#);
    assert!(resp.contains("200 OK"), "Execute undo should succeed");
    let body = extract_body(&resp);
    let data: serde_json::Value = serde_json::from_str(body).unwrap();
    assert_eq!(data["ok"], true);
    assert_eq!(data["executed"], "undo");

    handle.stop();
}

#[test]
fn execute_redo_command() {
    let (handle, port) = make_test_server();

    let resp = http_post(port, "/api/command/execute", r#"{"command":"redo"}"#);
    assert!(resp.contains("200 OK"), "Execute redo should succeed");
    let body = extract_body(&resp);
    let data: serde_json::Value = serde_json::from_str(body).unwrap();
    assert_eq!(data["ok"], true);
    assert_eq!(data["executed"], "redo");

    handle.stop();
}

#[test]
fn execute_client_side_command() {
    let (handle, port) = make_test_server();

    let resp = http_post(port, "/api/command/execute", r#"{"command":"save_scene"}"#);
    assert!(
        resp.contains("200 OK"),
        "Execute client command should succeed"
    );
    let body = extract_body(&resp);
    let data: serde_json::Value = serde_json::from_str(body).unwrap();
    assert_eq!(data["ok"], true);
    assert_eq!(data["action"], "client");
    assert_eq!(data["command"], "save_scene");

    handle.stop();
}

#[test]
fn execute_unknown_command_returns_404() {
    let (handle, port) = make_test_server();

    let resp = http_post(
        port,
        "/api/command/execute",
        r#"{"command":"nonexistent_command"}"#,
    );
    assert!(
        resp.contains("404") || resp.contains("unknown command"),
        "Should return 404 for unknown command, got: {resp}"
    );

    handle.stop();
}

#[test]
fn execute_missing_command_field_returns_400() {
    let (handle, port) = make_test_server();

    let resp = http_post(port, "/api/command/execute", r#"{"foo":"bar"}"#);
    assert!(
        resp.contains("400") || resp.contains("missing command"),
        "Should return 400 for missing command field, got: {resp}"
    );

    handle.stop();
}

#[test]
fn execute_invalid_json_returns_400() {
    let (handle, port) = make_test_server();

    let resp = http_post(port, "/api/command/execute", "not json");
    assert!(
        resp.contains("400") || resp.contains("invalid JSON"),
        "Should return 400 for invalid JSON, got: {resp}"
    );

    handle.stop();
}

#[test]
fn execute_delete_node_no_selection() {
    let (handle, port) = make_test_server();

    let resp = http_post(port, "/api/command/execute", r#"{"command":"delete_node"}"#);
    assert!(resp.contains("200 OK"));
    let body = extract_body(&resp);
    let data: serde_json::Value = serde_json::from_str(body).unwrap();
    assert_eq!(
        data["ok"], false,
        "Delete with no selection should return ok:false"
    );

    handle.stop();
}

#[test]
fn execute_multiple_client_commands() {
    let (handle, port) = make_test_server();

    // All these should return action:client
    let client_cmds = [
        "zoom_in",
        "zoom_out",
        "toggle_grid",
        "open_help",
        "search_nodes",
        "toggle_theme",
    ];
    for cmd in &client_cmds {
        let body = format!(r#"{{"command":"{}"}}"#, cmd);
        let resp = http_post(port, "/api/command/execute", &body);
        assert!(resp.contains("200 OK"), "Command {} should succeed", cmd);
        let data: serde_json::Value = serde_json::from_str(extract_body(&resp)).unwrap();
        assert_eq!(
            data["action"], "client",
            "Command {} should be client-side",
            cmd
        );
    }

    handle.stop();
}

// --- Editor HTML Tests ---

#[test]
fn editor_html_contains_command_palette() {
    let (handle, port) = make_test_server();

    let resp = http_get(port, "/editor");
    assert!(resp.contains("200 OK"));
    assert!(
        resp.contains("command-palette"),
        "Editor HTML should contain command-palette element"
    );
    assert!(
        resp.contains("cmd-search"),
        "Should contain command search input"
    );
    assert!(
        resp.contains("cmd-results"),
        "Should contain command results container"
    );
    assert!(
        resp.contains("openCommandPalette"),
        "Should contain openCommandPalette function"
    );
    assert!(
        resp.contains("fuzzyMatch"),
        "Should contain fuzzyMatch function"
    );
    assert!(
        resp.contains("Ctrl+Shift+P"),
        "Should document the Ctrl+Shift+P shortcut"
    );

    handle.stop();
}
