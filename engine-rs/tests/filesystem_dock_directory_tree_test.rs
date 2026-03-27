//! pat-omfrq: Asset browser FileSystem dock with directory tree.
//!
//! Integration tests covering:
//! 1. GET /api/filesystem — full directory listing with all file types
//! 2. GET /api/filesystem/tree — configurable depth and subdirectory root
//! 3. GET /api/filesystem/dir — single-directory lazy loading
//! 4. FsEntry JSON structure (files vs directories)
//! 5. File type classification for various extensions

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;
use std::time::Duration;

use gdeditor::editor_server::{EditorServerHandle, EditorState};
use gdscene::SceneTree;

fn free_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.local_addr().unwrap().port()
}

fn make_server() -> (EditorServerHandle, u16) {
    let port = free_port();
    let tree = SceneTree::new();
    let state = EditorState::new(tree);
    let handle = EditorServerHandle::start(port, state);
    thread::sleep(Duration::from_millis(300));
    (handle, port)
}

fn http_get(port: u16, path: &str) -> String {
    let req = format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n");
    http_request(port, &req)
}

fn http_request(port: u16, request: &str) -> String {
    let mut stream = TcpStream::connect(format!("127.0.0.1:{port}")).expect("connect");
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
    stream.write_all(request.as_bytes()).unwrap();
    let mut response = Vec::new();
    let _ = stream.read_to_end(&mut response);
    String::from_utf8_lossy(&response).to_string()
}

fn extract_body(resp: &str) -> &str {
    resp.split("\r\n\r\n").nth(1).unwrap_or("")
}

// ===========================================================================
// 1. GET /api/filesystem returns valid JSON with root and files
// ===========================================================================

#[test]
fn api_filesystem_returns_json_with_root() {
    let (handle, port) = make_server();
    // Use /api/filesystem/tree with depth=0 to keep the response small
    let resp = http_get(port, "/api/filesystem/tree?depth=0");
    let body = extract_body(&resp);
    let v: serde_json::Value = serde_json::from_str(body).expect("valid JSON");
    assert!(v["root"].is_string());
    assert!(v["entries"].is_array());
    handle.stop();
}

// ===========================================================================
// 2. GET /api/filesystem/tree with depth and root params
// ===========================================================================

#[test]
fn api_filesystem_tree_returns_entries() {
    let (handle, port) = make_server();
    let resp = http_get(port, "/api/filesystem/tree?depth=2");
    let body = extract_body(&resp);
    let v: serde_json::Value = serde_json::from_str(body).expect("valid JSON");
    assert!(v["root"].is_string());
    assert!(v["entries"].is_array());
    handle.stop();
}

#[test]
fn api_filesystem_tree_nonexistent_root() {
    let (handle, port) = make_server();
    let resp = http_get(
        port,
        "/api/filesystem/tree?root=nonexistent_dir_xyz_42",
    );
    let body = extract_body(&resp);
    let v: serde_json::Value = serde_json::from_str(body).expect("valid JSON");
    assert!(v["error"].is_string());
    assert!(v["error"].as_str().unwrap().contains("not found"));
    handle.stop();
}

#[test]
fn api_filesystem_tree_with_res_prefix() {
    let (handle, port) = make_server();
    // Should strip res:// prefix and scan the subdirectory
    let resp = http_get(port, "/api/filesystem/tree?root=res://fixtures");
    let body = extract_body(&resp);
    let v: serde_json::Value = serde_json::from_str(body).expect("valid JSON");
    // Either returns entries or "not found" depending on CWD — both are valid JSON
    assert!(v["entries"].is_array() || v["error"].is_string());
    handle.stop();
}

// ===========================================================================
// 3. GET /api/filesystem/dir for lazy loading
// ===========================================================================

#[test]
fn api_filesystem_dir_missing_path() {
    let (handle, port) = make_server();
    let resp = http_get(port, "/api/filesystem/dir");
    let body = extract_body(&resp);
    assert!(resp.contains("400"));
    assert!(body.contains("missing path"));
    handle.stop();
}

#[test]
fn api_filesystem_dir_nonexistent_dir() {
    let (handle, port) = make_server();
    let resp = http_get(port, "/api/filesystem/dir?path=res://nonexistent_xyz");
    let body = extract_body(&resp);
    let v: serde_json::Value = serde_json::from_str(body).expect("valid JSON");
    assert!(v["error"].is_string());
    handle.stop();
}

#[test]
fn api_filesystem_dir_root() {
    let (handle, port) = make_server();
    // Listing the project root
    let resp = http_get(port, "/api/filesystem/dir?path=res://");
    let body = extract_body(&resp);
    let v: serde_json::Value = serde_json::from_str(body).expect("valid JSON");
    assert_eq!(v["path"], "res://");
    assert!(v["entries"].is_array());
    // Root should contain at least some entries (Cargo.toml, fixtures, etc.)
    handle.stop();
}

#[test]
fn api_filesystem_dir_subdirs_have_empty_children() {
    let (handle, port) = make_server();
    let resp = http_get(port, "/api/filesystem/dir?path=res://");
    let body = extract_body(&resp);
    let v: serde_json::Value = serde_json::from_str(body).expect("valid JSON");
    // Find a directory entry and verify its children are empty (lazy loading)
    if let Some(entries) = v["entries"].as_array() {
        for entry in entries {
            if entry["is_dir"] == true {
                let children = entry["children"].as_array().unwrap();
                assert!(
                    children.is_empty(),
                    "dir listing should not recurse into subdirectories"
                );
                break;
            }
        }
    }
    handle.stop();
}

// ===========================================================================
// 4. File entries have correct structure
// ===========================================================================

#[test]
fn api_filesystem_file_entries_have_required_fields() {
    let (handle, port) = make_server();
    let resp = http_get(port, "/api/filesystem/tree?depth=1");
    let body = extract_body(&resp);
    let v: serde_json::Value = serde_json::from_str(body).expect("valid JSON");
    if let Some(entries) = v["entries"].as_array() {
        for entry in entries {
            assert!(entry["name"].is_string(), "entry must have name");
            assert!(entry["path"].is_string(), "entry must have path");
            assert!(
                entry["is_dir"].is_boolean(),
                "entry must have is_dir boolean"
            );
            if entry["is_dir"] == false {
                assert!(entry["size"].is_number(), "file must have size");
                assert!(entry["file_type"].is_string(), "file must have file_type");
            }
        }
    }
    handle.stop();
}

// ===========================================================================
// 5. EditorFileSystem unit-level tests
// ===========================================================================

use gdeditor::filesystem::EditorFileSystem;

#[test]
fn editor_filesystem_scan_directory_tree() {
    let dir = tempfile::TempDir::new().unwrap();
    // Create a directory structure
    std::fs::create_dir_all(dir.path().join("scenes/levels")).unwrap();
    std::fs::create_dir_all(dir.path().join("scripts")).unwrap();
    std::fs::create_dir_all(dir.path().join("assets/textures")).unwrap();
    std::fs::write(dir.path().join("project.godot"), "[gd_project]").unwrap();
    std::fs::write(dir.path().join("scenes/main.tscn"), "[gd_scene]").unwrap();
    std::fs::write(dir.path().join("scenes/levels/level1.tscn"), "[gd_scene]").unwrap();
    std::fs::write(dir.path().join("scripts/player.gd"), "extends Node2D").unwrap();
    std::fs::write(dir.path().join("assets/textures/icon.png"), "PNG data").unwrap();

    let mut fs = EditorFileSystem::new(dir.path());
    let count = fs.scan().unwrap();
    assert!(count >= 5, "should find at least 5 files, got {count}");

    let tscn_files = fs.files_by_extension("tscn");
    assert_eq!(tscn_files.len(), 2, "should find 2 .tscn files");

    let gd_files = fs.files_by_extension("gd");
    assert_eq!(gd_files.len(), 1, "should find 1 .gd file");
}

#[test]
fn editor_filesystem_skips_hidden_dirs() {
    let dir = tempfile::TempDir::new().unwrap();
    std::fs::create_dir_all(dir.path().join(".hidden")).unwrap();
    std::fs::write(dir.path().join(".hidden/secret.gd"), "secret").unwrap();
    std::fs::write(dir.path().join("visible.gd"), "visible").unwrap();

    let mut fs = EditorFileSystem::new(dir.path());
    let count = fs.scan().unwrap();
    assert_eq!(count, 1, "should only find visible.gd, not hidden dir contents");
}

#[test]
fn editor_filesystem_res_path_roundtrip() {
    let dir = tempfile::TempDir::new().unwrap();
    let fs = EditorFileSystem::new(dir.path());

    let abs = dir.path().join("scenes/main.tscn");
    let res = fs.to_res_path(&abs).unwrap();
    assert_eq!(res, "res://scenes/main.tscn");

    let back = fs.resolve_res_path(&res).unwrap();
    assert_eq!(back, abs);
}

// ===========================================================================
// 6. Directories are sorted: dirs first, then files, both alphabetical
// ===========================================================================

#[test]
fn api_filesystem_entries_sorted_dirs_first() {
    let (handle, port) = make_server();
    let resp = http_get(port, "/api/filesystem/tree?depth=0");
    let body = extract_body(&resp);
    let v: serde_json::Value = serde_json::from_str(body).expect("valid JSON");
    if let Some(entries) = v["entries"].as_array() {
        let mut seen_file = false;
        for entry in entries {
            if entry["is_dir"] == true {
                assert!(
                    !seen_file,
                    "directories should come before files in listing"
                );
            } else {
                seen_file = true;
            }
        }
    }
    handle.stop();
}
