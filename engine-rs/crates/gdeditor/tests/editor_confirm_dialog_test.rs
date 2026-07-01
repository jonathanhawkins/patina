//! Regression tests for the Godot-style confirmation dialog.
//!
//! The editor used to delete nodes (and other items) behind the browser's
//! native `confirm()` popup ("localhost:8080 says / Delete selected node?").
//! That popup is not styled like the editor and does not name the item being
//! deleted. These tests pin the replacement: an in-app ConfirmationDialog
//! ("Please Confirm...") wired through a promise-based `showConfirm()` helper,
//! and guard against any native `confirm()` call sneaking back in.

use gdeditor::editor_server::{EditorServerHandle, EditorState};
use gdscene::node::Node;
use gdscene::SceneTree;
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
    let main = Node::new("Ground", "Node2D");
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

fn http_get(port: u16, path: &str) -> String {
    let mut stream = connect_with_retry(port);
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
    stream
        .write_all(format!("GET {path} HTTP/1.1\r\nHost: localhost\r\n\r\n").as_bytes())
        .unwrap();
    let mut resp = Vec::new();
    let _ = stream.read_to_end(&mut resp);
    String::from_utf8_lossy(&resp).to_string()
}

fn extract_body(resp: &str) -> String {
    resp.split("\r\n\r\n").nth(1).unwrap_or("").to_string()
}

/// The custom confirmation dialog markup, header text, and buttons must exist.
#[test]
fn test_confirm_dialog_markup_present() {
    let (handle, port) = make_server();
    let b = extract_body(&http_get(port, "/editor"));
    handle.stop();

    assert!(b.contains("id=\"confirm-dialog\""), "Missing #confirm-dialog");
    assert!(
        b.contains("id=\"confirm-dialog-inner\""),
        "Missing #confirm-dialog-inner"
    );
    assert!(
        b.contains("id=\"confirm-dialog-ok\""),
        "Missing OK button #confirm-dialog-ok"
    );
    assert!(
        b.contains("id=\"confirm-dialog-cancel\""),
        "Missing Cancel button #confirm-dialog-cancel"
    );
    // Godot's ConfirmationDialog default window title.
    assert!(
        b.contains("Please Confirm..."),
        "Missing Godot-style 'Please Confirm...' title"
    );
}

/// The promise-based helper and its setup must be wired in.
#[test]
fn test_confirm_dialog_helper_wired() {
    let (handle, port) = make_server();
    let b = extract_body(&http_get(port, "/editor"));
    handle.stop();

    assert!(
        b.contains("function showConfirm("),
        "Missing showConfirm() helper"
    );
    assert!(
        b.contains("function setupConfirmDialog("),
        "Missing setupConfirmDialog()"
    );
    assert!(
        b.contains("setupConfirmDialog();"),
        "setupConfirmDialog() never called during init"
    );
}

/// Deleting a node must route through showConfirm and name the node (Godot parity:
/// `Delete node "Ground"?`), not the old generic native popup.
#[test]
fn test_delete_uses_named_confirm() {
    let (handle, port) = make_server();
    let b = extract_body(&http_get(port, "/editor"));
    handle.stop();

    assert!(
        b.contains("Delete node &quot;"),
        "Single-node delete should name the node like Godot ('Delete node \"X\"?')"
    );
    assert!(
        b.contains("await showConfirm(msg, { danger: true })"),
        "doDelete should await the styled confirm dialog with danger styling"
    );
}

/// No code path may fall back to the browser's native confirm() popup.
#[test]
fn test_no_native_confirm_calls() {
    let (handle, port) = make_server();
    let b = extract_body(&http_get(port, "/editor"));
    handle.stop();

    // Native usage looks like `confirm('...` or `confirm("...`. The replacement
    // helper is `showConfirm(`, which must not trip this guard.
    assert!(
        !b.contains("confirm('") && !b.contains("confirm(\""),
        "Native confirm() popup must not be used; route through showConfirm() instead"
    );
}
