//! Visual-fidelity (DOM-structure) tests for the FileSystem dock chrome.
//!
//! Patina's FileSystem dock was missing the permanent chrome that Godot's
//! `FileSystemDock` builds unconditionally in its constructor
//! (`upstream/godot/editor/docks/filesystem_dock.cpp`): a navigation/path
//! toolbar, a "Filter Files" box + sort button, and an always-present
//! "Favorites:" section header with a star icon. None of it depends on having
//! favorited or filtered anything — it is static structure, so a user comparing
//! the two editors side-by-side immediately sees the difference (the missing
//! `★ Favorites:` row in particular).
//!
//! These are *structural* parity assertions enumerated 1:1 from the Godot
//! source: every widget the dock's constructor always instantiates must have a
//! counterpart in the served editor HTML. They boot the real EditorServerHandle
//! and inspect the served `/editor` document — the methodology the planner now
//! uses for visual fidelity (see prd/EDITOR_PARITY_VISUAL_FIDELITY_METHODOLOGY.md).

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
    tree.add_child(root, Node::new("World", "Node2D")).unwrap();
    let state = EditorState::new(tree);
    let handle = EditorServerHandle::start(port, state);
    thread::sleep(Duration::from_millis(100));
    (handle, port)
}

fn editor_html(port: u16) -> String {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
    stream
        .write_all(b"GET /editor HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .unwrap();
    let mut buf = Vec::new();
    let _ = stream.read_to_end(&mut buf);
    String::from_utf8_lossy(&buf).to_string()
}

/// The headline gap from the parity screenshots: Godot always shows a
/// "Favorites:" section header with a star icon at the top of the dock, even
/// with zero favorites. Patina rendered nothing there.
#[test]
fn filesystem_dock_renders_favorites_section() {
    let (handle, port) = make_server();
    let html = editor_html(port);
    for needle in [
        "id=\"fs-favorites\"",  // the always-present Favorites section
        "id=\"fs-fav-header\"", // its clickable (collapsible) header
        "fs-fav-star",          // the star icon span
        "★",                    // Godot's Favorites star glyph, rendered statically
        "Favorites:",           // the exact Godot label (TTRC("Favorites:"))
        "id=\"fs-fav-list\"",   // where favorited dirs are pinned
    ] {
        assert!(
            html.contains(needle),
            "served FileSystem dock is missing Favorites chrome `{needle}` (Godot shows it unconditionally)"
        );
    }
    handle.stop();
}

/// Godot's dock toolbar (toolbar_hbc) always has: history prev/next buttons, a
/// path LineEdit, and a split-mode toggle. A second toolbar (toolbar2_hbc) has
/// the "Filter Files" box and the sort button. All are permanent chrome.
#[test]
fn filesystem_dock_renders_toolbar_chrome() {
    let (handle, port) = make_server();
    let html = editor_html(port);
    for needle in [
        "id=\"fs-toolbar\"",            // toolbar row 1
        "id=\"fs-nav-prev\"",           // history "previous folder/file"
        "id=\"fs-nav-next\"",           // history "next folder/file"
        "id=\"fs-path\"",               // current path LineEdit
        "id=\"fs-split-toggle\"",       // "Change Split Mode"
        "id=\"fs-toolbar2\"",           // toolbar row 2
        "placeholder=\"Filter Files\"", // tree_search_box placeholder (verbatim Godot text)
        "id=\"fs-sort\"",               // sort button
    ] {
        assert!(
            html.contains(needle),
            "served FileSystem dock is missing toolbar chrome `{needle}`"
        );
    }
    handle.stop();
}

/// The chrome must be wired, not just present: the served JS sets up the dock
/// (filter narrowing, sort, path/nav history, favorites collapse).
#[test]
fn filesystem_dock_chrome_is_wired() {
    let (handle, port) = make_server();
    let html = editor_html(port);
    for needle in [
        "function setupFileSystemChrome", // init entry point
        "setupFileSystemChrome()",        // actually invoked at boot
        "fsSubtreeMatches",               // filter predicate honored by the tree render
        "fsSortEntries",                  // sort applied before render
        "fsSetCurrentPath",               // path bar + nav-history tracking
    ] {
        assert!(
            html.contains(needle),
            "served editor JS is missing FileSystem chrome wiring `{needle}`"
        );
    }
    handle.stop();
}
